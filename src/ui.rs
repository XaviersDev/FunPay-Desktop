//! Состояние интерфейса (живёт в UI-потоке) и сборка строк для Slint.

use crate::album::{self, Block};
use crate::config::{Config, PinnedMsg, UserFolder};
use crate::imgcache::{self, ImgLoader};
use crate::parse::{self, ChatItem, Message};
use crate::worker::Cmd;
use crate::{AccountRow, AppWindow, BrowserTab, ChatRow, FolderRow, KV, MsgRow, OfferRow, PackRow, ReviewRow, St, StickerCell, Theme, Tile, TradeRow, TxnRow};
use slint::{Color, ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const AV: u32 = 128;
pub const BIG_AV: u32 = 256;
pub const THUMB: u32 = 900;
pub const FULL: u32 = 2560;
pub const STK: u32 = 160;

#[derive(Debug, Clone, Default)]
pub struct Me {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub balance: String,
}

#[derive(Debug, Clone, Default)]
pub struct OpenedChat {
    pub gen: u64,
    pub peer_id: String,
    pub peer_name: String,
    pub peer_status: String,
    pub peer_avatar: String,
    pub lot_name: String,
    pub lot_url: String,
    pub msgs: Vec<Message>,
    pub has_older: bool,
}

#[derive(Debug, Clone)]
pub struct Pending {
    pub local: u64,
    pub chat: String,
    pub text: String,
    pub image: Option<PathBuf>,
    /// 1 отправляется, 2 ошибка, 0 ушло (ждём, пока придёт настоящее)
    pub state: i32,
    pub at: Instant,
}

pub struct Cur {
    pub id: String,
    pub kind: i32,
    pub gen: u64,
    pub title: String,
    pub peer_id: String,
    pub peer_avatar: String,
    pub msgs: Vec<Message>,
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub key: String,
    pub author: String,
    pub text: String,
}

pub struct Ui {
    pub app: slint::Weak<AppWindow>,
    pub tx: Sender<Cmd>,
    pub cfg: Arc<Mutex<Config>>,
    pub loader: ImgLoader,
    pub imgs: HashMap<String, (Image, u32, u32)>,
    pub chats: Vec<ChatItem>,
    pub me: Me,
    pub folder: String,
    pub search: String,
    pub cur: Option<Cur>,
    pub pending: Vec<Pending>,
    pub games: Vec<(String, String)>,
    pub chats_model: Rc<VecModel<ChatRow>>,
    pub msgs_model: Rc<VecModel<MsgRow>>,
    pub next_local: u64,
    pub gen: u64,
    pub viewer_url: String,
    pub viewer_list: Vec<String>,
    pub gchat_visible: bool,
    pub toast_n: u64,
    pub focused: bool,
    pub demo: bool,
    pub reply: Option<Reply>,
    pub panes: Vec<i32>,
    pub profile: Option<parse::Profile>,
    pub pf_gen: u64,
    pub card_url: String,
    pub order: Option<parse::Order>,
    pub od_gen: u64,
    pub sendbox: Vec<PathBuf>,
    pub stickers: Option<crate::stickers::Store>,
    pub stickers_loading: bool,
    pub login_poll: u32,
    pub ext: Ext,
}

/// Состояние новых экранов: сделки, описания, мини-браузер.
#[derive(Default)]
pub struct Ext {
    /// листы кадров анимированных картинок по ключу кэша
    pub anims: HashMap<String, (Image, crate::anim::Info)>,
    /// вход через браузер: ключи, которые уже проверили на этой странице, и идёт ли проверка
    pub login_tried: Vec<String>,
    pub login_checking: bool,
    pub wall_gen: u64,
    pub deals_gen: u64,
    pub deals_tab: i32,
    pub deals_state: String,
    pub deals_next: String,
    pub trades: Vec<crate::money::TradeItem>,
    pub txns: Vec<crate::money::Txn>,
    pub desc_gen: u64,
    pub banner_url: String,
    pub descs: HashMap<String, (std::time::Instant, crate::desc::Desc)>,
    pub card_uid: String,
    pub tabs: Vec<crate::ui::Tab>,
    pub tab_next: i32,
    pub tab_active: i32,
    pub tg_applied: bool,
    pub adding: bool,
    pub add_tried: bool,
    pub add_prev: String,
    /// картинка шаблона в редакторе (путь в папке приложения)
    pub tpl_image: String,
    /// сумма в заморозке (посчитана отдельно)
    pub frozen: String,
    pub lots_gen: u64,
    pub lots: Vec<crate::lots::MyLot>,
    pub lots_query: String,
    pub lots_filter: i32,
    pub lot_lang: i32,
    pub lot_gen: u64,
    pub lot_form: Option<crate::lots::Form>,
    pub catalog: Vec<crate::feat2::CatalogTheme>,
    /// обновление скачано и установлено: путь к новому exe
    pub upd_exe: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct Tab {
    pub id: i32,
    pub title: String,
    pub url: String,
    pub loading: bool,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

pub fn install(ui: Ui) {
    UI.with(|u| *u.borrow_mut() = Some(ui));
}

pub fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|u| u.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Выполнить в UI-потоке (из любого потока).
pub fn post(f: impl FnOnce(&mut Ui) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || run_or_retry(Box::new(f), 0));
}

/// Если интерфейс сейчас занят (идёт другой обработчик), не теряем событие, а пробуем чуть позже.
fn run_or_retry(f: Box<dyn FnOnce(&mut Ui)>, tries: u32) {
    let busy = UI.with(|u| u.try_borrow_mut().is_err());
    if busy && tries < 50 {
        slint::Timer::single_shot(std::time::Duration::from_millis(15), move || run_or_retry(f, tries + 1));
        return;
    }
    with_ui(f);
}

// ---------------------------------------------------------------- цвета и мелочи

const PAIRS: [(u32, u32); 8] = [
    (0xFF8A7A, 0xE4507F),
    (0x62D6BE, 0x2C9E95),
    (0xB39BFF, 0x7657E6),
    (0xFFCB73, 0xF2973A),
    (0x72C6FF, 0x3A86E0),
    (0x97DB78, 0x45A956),
    (0xFF9FCB, 0xE2609C),
    (0x7FD0FF, 0x2E8BE0),
];
const NAME_COLORS_LIGHT: [u32; 7] = [0xD54B5A, 0xD9822B, 0x7A5CD6, 0x3E9C3A, 0x2A9AA0, 0x3A7FD5, 0xC94C8E];
const NAME_COLORS_DARK: [u32; 7] = [0xFF7A85, 0xFFAA5C, 0xB29BFF, 0x7FD46E, 0x5FD3D8, 0x6AB2FF, 0xFF85C0];

fn hash(s: &str) -> usize {
    s.bytes().fold(7usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize))
}

fn rgb(c: u32) -> Color {
    Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

pub fn pair(seed: &str) -> (Color, Color) {
    let (a, b) = PAIRS[hash(seed) % PAIRS.len()];
    (rgb(a), rgb(b))
}

fn letter(name: &str) -> String {
    name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
}

fn nbsp(n: usize) -> String {
    "\u{00A0}".repeat(n)
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

static GAMES_JSON: &str = include_str!("../assets/funpay_chats.json");

pub fn load_games() -> Vec<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(GAMES_JSON).unwrap_or_default();
    let mut out: Vec<(String, String)> = v
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|g| {
                    let name = g.get("name")?.as_str()?.to_string();
                    let url = g.get("chat_url")?.as_str()?;
                    let node = url.split("node=").nth(1)?.split('&').next()?.to_string();
                    Some((name, node))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|(n, _)| n.to_lowercase());
    out.dedup_by(|a, b| a.1 == b.1);
    out
}

const BUILTIN: [&str; 5] = ["all", "personal", "unread", "common", "archive"];

impl Ui {
    pub fn app(&self) -> Option<AppWindow> {
        self.app.upgrade()
    }

    pub fn st<R>(&self, f: impl FnOnce(&St) -> R) -> Option<R> {
        self.app().map(|a| f(&a.global::<St>()))
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    /// Картинка из кэша; если нет, ставим в очередь загрузки.
    pub fn img(&mut self, url: &str, max: u32) -> (String, Option<(Image, u32, u32)>) {
        let k = imgcache::key(max, url);
        if url.is_empty() {
            return (k, None);
        }
        if let Some(v) = self.imgs.get(&k) {
            return (k, Some(v.clone()));
        }
        if !url.starts_with("demo://") {
            self.loader.request(url, max);
        }
        (k, None)
    }

    fn img_opt(&mut self, url: &str, max: u32) -> Option<Image> {
        if parse::is_default_avatar(url) {
            return None;
        }
        self.img(url, max).1.map(|x| x.0)
    }

    pub fn toast(&mut self, text: &str) {
        self.toast_n += 1;
        let n = self.toast_n;
        self.st(|s| s.set_toast(text.into()));
        slint::Timer::single_shot(std::time::Duration::from_millis(2600), move || {
            with_ui(|u| {
                if u.toast_n == n {
                    u.st(|s| s.set_toast("".into()));
                }
            });
        });
    }

    // ------------------------------------------------------------ вход

    pub fn show_login(&mut self, err: String) {
        self.chats.clear();
        self.cur = None;
        self.pending.clear();
        self.panes.clear();
        self.chats_model.set_vec(vec![]);
        self.msgs_model.set_vec(vec![]);
        self.st(|s| {
            s.set_logged_in(false);
            s.set_login_busy(false);
            s.set_login_error(err.into());
            s.set_chat_open(false);
            s.set_settings_open(false);
            s.set_pane(0);
            s.set_browser_login(false);
        });
    }

    pub fn login_busy(&mut self, b: bool) {
        if b && self.ext.adding {
            self.ext.add_tried = true;
        }
        self.st(|s| {
            s.set_login_busy(b);
            if b {
                s.set_login_error("".into());
            }
        });
    }

    pub fn logged_in(&mut self, me: Me) {
        if !self.me.id.is_empty() && self.me.id != me.id {
            self.reset_account_view();
        }
        self.me = me;
        self.ext.adding = false;
        self.ext.add_tried = false;
        self.st(|s| s.set_adding_account(false));
        self.snapshot_account();
        self.rebuild_accounts();
        self.st(|s| {
            s.set_logged_in(true);
            s.set_login_busy(false);
            s.set_login_error("".into());
            s.set_login_page(0);
            s.set_chats_loading(true);
            s.set_net_error("".into());
            s.set_browser_login(false);
        });
        self.update_me();
        self.rebuild_chats();
        self.tour_event("logged-in");
    }

    pub fn update_me(&mut self) {
        let me = self.me.clone();
        let av = self.img_opt(&me.avatar, BIG_AV);
        let nick = self.nick(&me.name);
        self.st(|s| {
            s.set_me_name(me.name.clone().into());
            s.set_me_letter(letter(&me.name).into());
            s.set_me_id(me.id.clone().into());
            s.set_me_balance(me.balance.clone().into());
            s.set_me_nick(nick.clone());
            s.set_me_has_avatar(av.is_some());
            if let Some(i) = av {
                s.set_me_avatar(i);
            }
        });
    }

    pub fn net_error(&mut self, e: String) {
        self.st(|s| s.set_net_error(e.into()));
    }

    pub fn attention(&mut self) {
        if !self.focused && !self.demo {
            if let Some(app) = self.app() {
                use slint::winit_030::WinitWindowAccessor;
                app.window().with_winit_window(|w| {
                    w.request_user_attention(Some(slint::winit_030::winit::window::UserAttentionType::Informational));
                });
            }
        }
    }

    /// Клик по уведомлению: окно на передний план и сразу нужный чат.
    /// Показать окно (из трея, по уведомлению, при втором запуске).
    pub fn show_window(&mut self) {
        if let Some(app) = self.app() {
            use slint::winit_030::WinitWindowAccessor;
            let _ = app.show();
            app.window().with_winit_window(|w| {
                w.set_visible(true);
                w.set_minimized(false);
                w.focus_window();
            });
        }
    }

    /// Спрятать окно в трей: опрос FunPay и уведомления продолжают работать.
    pub fn hide_to_background(&mut self) {
        if let Some(app) = self.app() {
            let _ = app.hide();
        }
        self.focused = false;
        self.send(Cmd::Focus(false));
        let first = {
            let mut c = self.cfg.lock().unwrap();
            let first = !c.bg_hint_shown;
            c.bg_hint_shown = true;
            c.save();
            first
        };
        if first {
            crate::sys::notify("FunPay Tools работает в фоне", "Уведомления о сообщениях продолжат приходить. Открыть окно можно значком в трее рядом с часами.", || {
                crate::ui::post(|u| u.show_window());
            });
        }
        self.upd_on_hide();
    }

    pub fn notification_clicked(&mut self, id: String) {
        self.show_window();
        self.panes.clear();
        self.st(|s| {
            s.set_pane(0);
            s.set_settings_open(false);
            s.set_viewer_open(false);
        });
        if self.cur.as_ref().map(|c| c.id != id).unwrap_or(true) {
            self.open_chat(id, 0);
        }
    }

    // ------------------------------------------------------------ папки и список чатов

    fn chat_key_kind(&self, id: &str) -> i32 {
        if id == "flood" {
            1
        } else if id == "fpt" {
            3
        } else if id.starts_with("game-") {
            2
        } else {
            0
        }
    }

    pub fn chats_updated(&mut self, chats: Vec<ChatItem>) {
        let changed = self.chats != chats;
        self.chats = chats;
        if changed {
            self.save_chats_cache();
        }
        self.st(|s| s.set_chats_loading(false));
        self.rebuild_chats();
    }

    pub fn set_gchat_visible(&mut self, v: bool) {
        self.gchat_visible = v;
        self.rebuild_chats();
    }

    fn special_row(&mut self, id: &str, kind: i32, name: &str, preview: &str, c: &Config) -> ChatRow {
        let (c1, c2) = match kind {
            1 => (rgb(0x6FB2FF), rgb(0x2F6FD0)),
            3 => (rgb(0x5B8DEF), rgb(0x6A5BE2)),
            _ => pair(name),
        };
        ChatRow {
            id: id.into(),
            kind,
            name: name.into(),
            preview: preview.into(),
            letter: letter(name).into(),
            c1,
            c2,
            selected: self.cur.as_ref().map(|c| c.id == id).unwrap_or(false),
            pinned: c.pinned_chats.iter().any(|p| p == id),
            ..Default::default()
        }
    }

    fn personal_row(&mut self, ch: &ChatItem, c: &Config) -> ChatRow {
        let (c1, c2) = pair(&ch.name);
        let mut prefix = String::new();
        let mut check = 0;
        let mut preview = parse::preview_text(&ch.preview);
        if let Some(mine) = c.last_outgoing.get(&ch.id) {
            let p = parse::preview_text(&ch.preview).trim_end_matches("...").trim().to_lowercase();
            let mine_p = parse::preview_text(mine).to_lowercase();
            let is_mine = if mine == "__image__" {
                !ch.unread && (p.contains("изображение") || p.contains("image") || p.is_empty())
            } else {
                !p.is_empty() && (mine_p.starts_with(&p) || mine.to_lowercase().starts_with(&p))
            };
            if is_mine && !ch.unread {
                prefix = "Вы:".into();
                check = 2;
            }
        }
        if preview.is_empty() {
            preview = "Изображение".into();
        }
        let (key, av) = if parse::is_default_avatar(&ch.avatar) { (String::new(), None) } else { self.img(&ch.avatar, AV) };
        ChatRow {
            id: ch.id.clone().into(),
            kind: 0,
            name: ch.name.clone().into(),
            preview: preview.into(),
            prefix: prefix.into(),
            time: ch.time.clone().into(),
            unread: if ch.unread { -1 } else { 0 },
            has_avatar: av.is_some(),
            avatar: av.map(|a| a.0).unwrap_or_default(),
            avatar_key: key.into(),
            letter: letter(&ch.name).into(),
            c1,
            c2,
            selected: self.cur.as_ref().map(|x| x.id == ch.id).unwrap_or(false),
            muted: c.muted.contains(&ch.id),
            pinned: c.pinned_chats.iter().any(|p| *p == ch.id),
            archived: c.archived.contains(&ch.id),
            fpt: false,
            check,
            nick: self.nick(&ch.name),
        }
    }

    /// Строка для любого чата по id (для закреплённых и своих папок).
    fn row_for(&mut self, id: &str, c: &Config) -> Option<ChatRow> {
        match self.chat_key_kind(id) {
            1 => Some(self.special_row("flood", 1, "Общий чат FunPay", "Чат всех пользователей FunPay", c)),
            3 => {
                if self.gchat_visible {
                    Some(self.special_row("fpt", 3, "Чат FunPay Tools", "Общий чат пользователей FunPay Tools", c))
                } else {
                    None
                }
            }
            2 => {
                let name = self.games.iter().find(|(_, n)| n == id).map(|(n, _)| n.clone()).unwrap_or_else(|| id.to_string());
                Some(self.special_row(id, 2, &name, "Игровой чат", c))
            }
            _ => {
                if let Some(ch) = self.chats.iter().find(|x| x.id == id).cloned() {
                    Some(self.personal_row(&ch, c))
                } else {
                    let (_, name) = c.known_chats.get(id).cloned()?;
                    let ch = ChatItem { id: id.to_string(), name, ..Default::default() };
                    Some(self.personal_row(&ch, c))
                }
            }
        }
    }

    fn folder_rows(&self, c: &Config) -> Vec<FolderRow> {
        let unread_in = |pred: &dyn Fn(&ChatItem) -> bool| -> i32 {
            self.chats.iter().filter(|x| x.unread && !c.muted.contains(&x.id) && pred(x)).count() as i32
        };
        let main = unread_in(&|x| !c.archived.contains(&x.id));
        let mut v = vec![
            FolderRow { id: "all".into(), name: "Все чаты".into(), icon: 0, unread: main },
            FolderRow { id: "personal".into(), name: "Личные".into(), icon: 1, unread: main },
            FolderRow { id: "unread".into(), name: "Непрочит.".into(), icon: 2, unread: main },
            FolderRow { id: "common".into(), name: "Общие".into(), icon: 3, unread: 0 },
        ];
        for f in &c.folders {
            let set: HashSet<&String> = f.chats.iter().collect();
            v.push(FolderRow { id: f.id.clone().into(), name: f.name.clone().into(), icon: 5, unread: unread_in(&|x| set.contains(&x.id)) });
        }
        if !c.archived.is_empty() {
            v.push(FolderRow { id: "archive".into(), name: "Архив".into(), icon: 4, unread: unread_in(&|x| c.archived.contains(&x.id)) });
        }
        v
    }

    pub fn rebuild_chats(&mut self) {
        let c = self.cfg.lock().unwrap().clone();
        let q = self.search.trim().to_lowercase();
        let matches = |row: &ChatRow| q.is_empty() || row.name.to_lowercase().contains(&q) || row.preview.to_lowercase().contains(&q);
        let mut rows: Vec<ChatRow> = Vec::new();
        let chats = self.chats.clone();
        let folder = self.folder.clone();
        // если выбранной папки больше нет, возвращаемся во «Все чаты»
        if !BUILTIN.contains(&folder.as_str()) && !c.folders.iter().any(|f| f.id == folder) || (folder == "archive" && c.archived.is_empty()) {
            self.folder = "all".into();
            self.st(|s| s.set_folder("all".into()));
            return self.rebuild_chats();
        }
        let pinned: Vec<String> = c.pinned_chats.clone();
        match folder.as_str() {
            "all" | "personal" => {
                let personal_only = folder == "personal";
                for id in &pinned {
                    if personal_only && self.chat_key_kind(id) != 0 {
                        continue;
                    }
                    if c.archived.contains(id) {
                        continue;
                    }
                    if let Some(r) = self.row_for(id, &c) {
                        rows.push(r);
                    }
                }
                if !personal_only {
                    let mut specials: Vec<String> = Vec::new();
                    if c.show_flood {
                        specials.push("flood".into());
                    }
                    if c.show_gchat && self.gchat_visible {
                        specials.push("fpt".into());
                    }
                    specials.extend(c.recent_games.iter().cloned());
                    for id in specials {
                        if pinned.contains(&id) {
                            continue;
                        }
                        if let Some(r) = self.row_for(&id, &c) {
                            rows.push(r);
                        }
                    }
                }
                for ch in chats.iter().filter(|x| !c.archived.contains(&x.id) && !pinned.contains(&x.id)) {
                    rows.push(self.personal_row(ch, &c));
                }
            }
            "unread" => {
                for ch in chats.iter().filter(|x| x.unread && !c.archived.contains(&x.id)) {
                    rows.push(self.personal_row(ch, &c));
                }
            }
            "archive" => {
                for ch in chats.iter().filter(|x| c.archived.contains(&x.id)) {
                    rows.push(self.personal_row(ch, &c));
                }
            }
            "common" => {
                rows.push(self.special_row("flood", 1, "Общий чат FunPay", "Чат всех пользователей FunPay", &c));
                if self.gchat_visible {
                    rows.push(self.special_row("fpt", 3, "Чат FunPay Tools", "Общий чат пользователей FunPay Tools", &c));
                }
                let games: Vec<(String, String)> = self.games.clone();
                let mut header = false;
                for (name, node) in games {
                    let r = self.special_row(&node, 2, &name, "Игровой чат", &c);
                    if matches(&r) {
                        if !header {
                            rows.push(ChatRow { kind: 9, name: "Игровые чаты".into(), ..Default::default() });
                            header = true;
                        }
                        rows.push(r);
                    }
                }
            }
            fid => {
                if let Some(f) = c.folders.iter().find(|f| f.id == fid).cloned() {
                    let mut ids: Vec<String> = pinned.iter().filter(|p| f.chats.contains(p)).cloned().collect();
                    ids.extend(f.chats.iter().filter(|x| !pinned.contains(x)).cloned());
                    for id in ids {
                        if let Some(r) = self.row_for(&id, &c) {
                            rows.push(r);
                        }
                    }
                }
            }
        }
        rows.retain(|r| r.kind == 9 || matches(r));
        let folders = self.folder_rows(&c);
        let total: i32 = folders.first().map(|f| f.unread).unwrap_or(0);
        self.st(|s| s.set_folders(model(folders)));
        if let Some(app) = self.app() {
            app.set_win_title(if total > 0 { format!("FunPay Tools ({total})") } else { "FunPay Tools".into() }.into());
        }
        sync_model(&self.chats_model, rows, |a, b| a.id == b.id && a.kind == b.kind);
        self.sync_cur_flags();
    }

    fn sync_cur_flags(&self) {
        let Some(cur) = &self.cur else { return };
        let c = self.cfg.lock().unwrap();
        let muted = c.muted.contains(&cur.id);
        let pinned = c.pinned_chats.contains(&cur.id);
        drop(c);
        self.st(|s| {
            s.set_cur_muted(muted);
            s.set_cur_pinned(pinned);
        });
    }

    pub fn select_folder(&mut self, id: String) {
        self.folder = id.clone();
        self.st(|s| s.set_folder(id.into()));
        self.rebuild_chats();
    }

    pub fn chat_menu(&mut self, id: String, kind: i32, action: String, arg: String) {
        let name = self.chats.iter().find(|c| c.id == id).map(|c| c.name.clone());
        {
            let mut c = self.cfg.lock().unwrap();
            match action.as_str() {
                "pin" => {
                    c.pinned_chats.retain(|x| *x != id);
                    c.pinned_chats.insert(0, id.clone());
                    c.archived.remove(&id);
                }
                "unpin" => c.pinned_chats.retain(|x| *x != id),
                "mute" => {
                    c.muted.insert(id.clone());
                }
                "unmute" => {
                    c.muted.remove(&id);
                }
                "archive" => {
                    c.archived.insert(id.clone());
                    c.pinned_chats.retain(|x| *x != id);
                }
                "unarchive" => {
                    c.archived.remove(&id);
                }
                "folder-add" => {
                    if let Some(f) = c.folders.iter_mut().find(|f| f.id == arg) {
                        if !f.chats.contains(&id) {
                            f.chats.push(id.clone());
                        }
                    }
                    if let Some(n) = &name {
                        c.known_chats.insert(id.clone(), (kind, n.clone()));
                    }
                }
                "folder-remove" => {
                    if let Some(f) = c.folders.iter_mut().find(|f| f.id == arg) {
                        f.chats.retain(|x| *x != id);
                    }
                }
                "forget" => {
                    // убрать игровой/общий чат из недавних
                    c.recent_games.retain(|x| *x != id);
                }
                _ => {}
            }
            c.save();
        }
        match action.as_str() {
            "read" => {
                if let Some(ch) = self.chats.iter_mut().find(|c| c.id == id) {
                    ch.unread = false;
                }
                self.send(Cmd::MarkRead(id.clone()));
            }
            "web" => {
                if kind != 3 {
                    self.open_url(format!("https://funpay.com/chat/?node={id}"));
                }
            }
            "archive" => self.toast("Чат в архиве"),
            "mute" => self.toast("Уведомления выключены"),
            "folder-add" => self.toast("Добавлено в папку"),
            "forget" => {
                if self.cur.as_ref().map(|c| c.id == id).unwrap_or(false) {
                    self.close_chat();
                }
                self.toast("Убрал из недавних");
            }
            _ => {}
        }
        self.rebuild_chats();
    }

    pub fn create_folder(&mut self, name: String) {
        let name = squash(&name);
        if name.is_empty() {
            return;
        }
        let chat = self.st(|s| s.get_folder_dialog_chat().to_string()).unwrap_or_default();
        let kind = self.chat_key_kind(&chat);
        let chat_name = self.chats.iter().find(|c| c.id == chat).map(|c| c.name.clone());
        {
            let mut c = self.cfg.lock().unwrap();
            let n = c.folders.iter().filter_map(|f| f.id.trim_start_matches('f').parse::<u32>().ok()).max().unwrap_or(0) + 1;
            let mut f = UserFolder { id: format!("f{n}"), name: name.chars().take(24).collect(), chats: vec![] };
            if !chat.is_empty() {
                f.chats.push(chat.clone());
                if let Some(nm) = chat_name {
                    c.known_chats.insert(chat.clone(), (kind, nm));
                }
            }
            c.folders.push(f);
            c.save();
        }
        self.st(|s| s.set_folder_dialog_chat("".into()));
        self.rebuild_chats();
    }

    pub fn delete_folder(&mut self, id: String) {
        {
            let mut c = self.cfg.lock().unwrap();
            c.folders.retain(|f| f.id != id);
            c.save();
        }
        if self.folder == id {
            self.folder = "all".into();
            self.st(|s| s.set_folder("all".into()));
        }
        self.rebuild_chats();
    }

    // ------------------------------------------------------------ открытие чата

    pub fn open_chat(&mut self, id: String, kind: i32) {
        let name = self.chats.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
        self.open_chat_named(id, kind, name);
    }

    pub fn open_chat_named(&mut self, id: String, kind: i32, name: String) {
        if id.is_empty() || kind == 9 {
            return;
        }
        self.gen += 1;
        let gen = self.gen;
        let (title, subtitle) = match kind {
            1 => ("Общий чат FunPay".to_string(), "чат всех пользователей".to_string()),
            2 => (
                self.games.iter().find(|(_, n)| *n == id).map(|(n, _)| n.clone()).unwrap_or_else(|| id.clone()),
                "игровой чат".to_string(),
            ),
            3 => ("Чат FunPay Tools".to_string(), "общий чат пользователей FunPay Tools".to_string()),
            _ => (name, String::new()),
        };
        let avatar = self.chats.iter().find(|c| c.id == id).map(|c| c.avatar.clone()).unwrap_or_default();
        if kind == 2 {
            let mut c = self.cfg.lock().unwrap();
            c.recent_games.retain(|n| *n != id);
            c.recent_games.insert(0, id.clone());
            c.recent_games.truncate(5);
            c.save();
        }
        self.cur = Some(Cur { id: id.clone(), kind, gen, title: title.clone(), peer_id: String::new(), peer_avatar: avatar, msgs: vec![] });
        self.reply = None;
        self.msgs_model.set_vec(vec![]);
        self.panes.clear();
        self.st(|s| {
            s.set_pane(0);
            s.set_chat_open(true);
            s.set_cur_id(id.clone().into());
            s.set_cur_kind(kind);
            s.set_cur_title(title.clone().into());
            s.set_cur_subtitle(subtitle.into());
            s.set_cur_online(false);
            s.set_cur_peer_id("".into());
            s.set_lot_name("".into());
            s.set_lot_url("".into());
            s.set_show_fpt_banner(false);
            s.set_cur_fpt(false);
            s.set_pin_key("".into());
            s.set_msgs_loading(true);
            s.set_has_older(false);
            s.set_loading_older(false);
            s.set_send_error("".into());
            s.set_reply_key("".into());
            s.set_gchat_need_login(false);
            s.set_gchat_status("".into());
            s.set_gchat_disabled("".into());
            s.set_card_open(false);
            s.set_ai_orig("".into());
            s.set_cmd_hits(Default::default());
        });
        let nick = if kind == 0 { self.nick(&title) } else { Default::default() };
        self.st(|s| s.set_cur_nick(nick));
        self.rebuild_chats();
        // сохранённая переписка видна сразу, свежая придёт следом
        self.show_cached_msgs();
        self.rebuild_templates();
        if let Some(app) = self.app() {
            app.invoke_scroll_bottom();
            app.invoke_focus_input();
        }
        self.send(Cmd::Open { id, kind, gen });
    }

    pub fn close_chat(&mut self) {
        self.cur = None;
        self.reply = None;
        self.gen += 1;
        self.msgs_model.set_vec(vec![]);
        self.st(|s| {
            s.set_chat_open(false);
            s.set_stickers_open(false);
        });
        self.rebuild_chats();
        self.send(Cmd::Close);
    }

    pub fn chat_opened(&mut self, o: OpenedChat) {
        let Some(cur) = self.cur.as_mut() else { return };
        if cur.gen != o.gen {
            return;
        }
        cur.msgs = o.msgs;
        cur.peer_id = o.peer_id.clone();
        if !o.peer_avatar.is_empty() {
            cur.peer_avatar = o.peer_avatar.clone();
        }
        if cur.kind == 0 && !o.peer_name.is_empty() {
            cur.title = o.peer_name.clone();
            let id = cur.id.clone();
            let name = o.peer_name.clone();
            let mut c = self.cfg.lock().unwrap();
            if c.known_chats.get(&id).map(|x| x.1 != name).unwrap_or(true) && !id.is_empty() {
                c.known_chats.insert(id, (0, name));
            }
        }
        let cur = self.cur.as_ref().unwrap();
        let title = cur.title.clone();
        let kind = cur.kind;
        let status = o.peer_status.to_lowercase();
        let online = status.contains("онлайн") || status.contains("online");
        self.st(|s| {
            s.set_msgs_loading(false);
            s.set_has_older(o.has_older);
            if kind == 0 {
                s.set_cur_title(title.clone().into());
                s.set_cur_subtitle(status.clone().into());
                s.set_cur_online(online);
                s.set_cur_peer_id(o.peer_id.clone().into());
                s.set_lot_name(o.lot_name.clone().into());
                s.set_lot_url(o.lot_url.clone().into());
            }
        });
        self.sync_cur_flags();
        self.update_fpt_banner();
        self.rebuild_msgs();
        if let Some(app) = self.app() {
            app.invoke_scroll_bottom();
        }
        let nick = self.nick(&title);
        self.st(|s| s.set_cur_nick(nick));
        self.save_msgs_cache();
        self.rebuild_templates();
    }

    pub fn chat_failed(&mut self, gen: u64, e: String) {
        if self.cur.as_ref().map(|c| c.gen) != Some(gen) {
            return;
        }
        self.st(|s| {
            s.set_msgs_loading(false);
            s.set_send_error(format!("Не удалось открыть чат: {e}").into());
        });
    }

    pub fn header_refreshed(&mut self, gen: u64, status: String, lot_name: String, lot_url: String) {
        if self.cur.as_ref().map(|c| c.gen) != Some(gen) {
            return;
        }
        let online = status.to_lowercase().contains("онлайн");
        self.st(|s| {
            s.set_cur_subtitle(status.to_lowercase().into());
            s.set_cur_online(online);
            s.set_lot_name(lot_name.into());
            s.set_lot_url(lot_url.into());
        });
    }

    pub fn fpt_users_added(&mut self, _ids: Vec<String>) {
        self.update_fpt_banner();
    }

    pub fn update_fpt_banner(&mut self) {
        let Some(cur) = &self.cur else { return };
        let (fpt, show) = {
            let c = self.cfg.lock().unwrap();
            let fpt = cur.kind == 0 && !cur.peer_id.is_empty() && c.fpt_users.contains(&cur.peer_id);
            (fpt, fpt && !c.fpt_dismissed.contains(&cur.peer_id))
        };
        self.st(|s| {
            s.set_cur_fpt(fpt);
            s.set_show_fpt_banner(show);
        });
    }

    pub fn undismiss_fpt(&mut self) {
        if let Some(cur) = &self.cur {
            let mut c = self.cfg.lock().unwrap();
            c.fpt_dismissed.remove(&cur.peer_id);
            c.save();
        }
        self.update_fpt_banner();
    }

    pub fn dismiss_fpt(&mut self) {
        if let Some(cur) = &self.cur {
            let mut c = self.cfg.lock().unwrap();
            c.fpt_dismissed.insert(cur.peer_id.clone());
            c.save();
        }
        self.st(|s| s.set_show_fpt_banner(false));
    }

    pub fn new_messages(&mut self, gen: u64, msgs: Vec<Message>) {
        let my = self.me.id.clone();
        let Some(cur) = self.cur.as_mut() else { return };
        if cur.gen != gen {
            return;
        }
        let known: HashSet<u64> = cur.msgs.iter().map(|m| m.id).collect();
        let mut mine_texts = Vec::new();
        let mut mine_images = 0;
        for m in msgs {
            if known.contains(&m.id) {
                continue;
            }
            if m.author_id == my {
                if m.image.is_some() {
                    mine_images += 1;
                } else {
                    mine_texts.push(squash(&m.text));
                }
            }
            cur.msgs.push(m);
        }
        let chat = cur.id.clone();
        self.pending.retain(|p| {
            if p.chat != chat || p.state != 0 {
                return true;
            }
            if p.image.is_some() {
                if mine_images > 0 {
                    mine_images -= 1;
                    return false;
                }
                return true;
            }
            if let Some(i) = mine_texts.iter().position(|t| *t == squash(&p.text)) {
                mine_texts.remove(i);
                return false;
            }
            true
        });
        self.rebuild_msgs();
        self.save_msgs_cache();
        self.rebuild_templates();
    }

    pub fn gchat_messages(&mut self, gen: u64, msgs: Vec<Message>, initial: bool) {
        if let Some(cur) = self.cur.as_ref() {
            if cur.gen == gen && initial {
                self.st(|s| s.set_msgs_loading(false));
            }
        }
        self.new_messages(gen, msgs);
        if let Some(cur) = self.cur.as_mut() {
            cur.msgs.sort_by_key(|m| m.id);
        }
        if initial {
            self.rebuild_msgs();
            if let Some(app) = self.app() {
                app.invoke_scroll_bottom();
            }
        }
    }

    pub fn gchat_gate(&mut self, need_login: bool, disabled: String) {
        self.st(|s| {
            s.set_gchat_need_login(need_login);
            s.set_gchat_disabled(disabled.into());
        });
    }

    pub fn gchat_status(&mut self, t: String) {
        self.st(|s| s.set_gchat_status(t.into()));
    }

    pub fn load_older(&mut self) {
        let busy = self.st(|s| s.get_loading_older() || !s.get_has_older()).unwrap_or(true);
        if busy || self.cur.is_none() {
            return;
        }
        self.st(|s| s.set_loading_older(true));
        self.send(Cmd::LoadOlder);
    }

    pub fn older_loaded(&mut self, gen: u64, msgs: Vec<Message>, has_older: bool) {
        let Some(cur) = self.cur.as_mut() else { return };
        if cur.gen != gen {
            return;
        }
        let n = msgs.len();
        if n > 0 {
            let mut all = msgs;
            all.extend(std::mem::take(&mut cur.msgs));
            cur.msgs = all;
        }
        if n > 0 {
            if let Some(app) = self.app() {
                app.invoke_begin_prepend();
            }
        }
        self.st(|s| {
            s.set_loading_older(false);
            s.set_has_older(has_older);
        });
        if n > 0 {
            self.rebuild_msgs();
        }
    }

    // ------------------------------------------------------------ отправка

    pub fn send_text(&mut self, text: String) {
        let text = text.trim_end().to_string();
        if text.trim().is_empty() {
            return;
        }
        let Some(cur) = &self.cur else { return };
        let chat = cur.id.clone();
        let text = match self.reply.take() {
            Some(r) => {
                let mut q = squash(&r.text);
                if q.is_empty() {
                    q = "Сообщение".into();
                }
                if q.chars().count() > 300 {
                    q = q.chars().take(300).collect::<String>() + "…";
                }
                format!("╭─ ⤸ {q}\n╰ {}", text.trim())
            }
            None => text,
        };
        self.next_local += 1;
        let local = self.next_local;
        self.pending.push(Pending { local, chat, text: text.clone(), image: None, state: 1, at: Instant::now() });
        self.st(|s| {
            s.set_send_error("".into());
            s.set_reply_key("".into());
        });
        self.rebuild_msgs();
        self.send(Cmd::SendText { local, text });
    }

    /// Отправить картинки сразу (стикеры, окно отправки).
    pub fn send_images(&mut self, paths: Vec<PathBuf>) {
        let Some(cur) = &self.cur else { return };
        let chat = cur.id.clone();
        if cur.kind == 3 {
            self.toast("В чат FunPay Tools можно отправлять только текст");
            return;
        }
        for p in paths {
            self.next_local += 1;
            let local = self.next_local;
            self.pending.push(Pending { local, chat: chat.clone(), text: String::new(), image: Some(p.clone()), state: 1, at: Instant::now() });
            self.send(Cmd::SendImage { local, path: p });
        }
        self.rebuild_msgs();
        if let Some(app) = self.app() {
            app.invoke_scroll_bottom();
        }
    }

    pub fn send_result(&mut self, local: u64, res: Result<(), String>) {
        if let Some(p) = self.pending.iter_mut().find(|p| p.local == local) {
            match &res {
                Ok(()) => {
                    p.state = 0;
                    p.at = Instant::now();
                }
                Err(_) => p.state = 2,
            }
        }
        if let Err(e) = res {
            self.st(|s| s.set_send_error(e.into()));
        }
        self.rebuild_msgs();
        self.rebuild_chats();
    }

    pub fn retry(&mut self, key: String) {
        let Some(local) = key.strip_prefix('p').and_then(|k| k.parse::<u64>().ok()) else { return };
        let Some(p) = self.pending.iter_mut().find(|p| p.local == local) else { return };
        p.state = 1;
        let cmd = match &p.image {
            Some(path) => Cmd::SendImage { local, path: path.clone() },
            None => Cmd::SendText { local, text: p.text.clone() },
        };
        self.st(|s| s.set_send_error("".into()));
        self.rebuild_msgs();
        self.send(cmd);
    }

    pub fn sweep_pending(&mut self) {
        let before = self.pending.len();
        self.pending.retain(|p| !(p.state == 0 && p.at.elapsed().as_secs() > 12));
        if self.pending.len() != before {
            self.rebuild_msgs();
        }
    }

    // ------------------------------------------------------------ окно отправки картинок

    pub fn sendbox_add(&mut self, paths: Vec<PathBuf>) {
        if self.cur.as_ref().map(|c| c.kind == 3).unwrap_or(true) {
            self.toast("В чат FunPay Tools можно отправлять только текст");
            return;
        }
        self.sendbox.extend(paths);
        self.sendbox.truncate(10);
        self.rebuild_sendbox();
    }

    pub fn sendbox_remove(&mut self, i: usize) {
        if i < self.sendbox.len() {
            self.sendbox.remove(i);
        }
        if self.sendbox.is_empty() {
            self.st(|s| s.set_sendbox_open(false));
        } else {
            self.rebuild_sendbox();
        }
    }

    fn rebuild_sendbox(&mut self) {
        let paths = self.sendbox.clone();
        let urls: Vec<String> = paths.iter().map(|p| format!("file://{}", p.to_string_lossy())).collect();
        let aspect = urls
            .first()
            .and_then(|u| self.imgs.get(&imgcache::key(THUMB, u)).map(|(_, w, h)| *h as f32 / (*w).max(1) as f32))
            .unwrap_or(0.75);
        let (geo, ratio) = album::layout(urls.len(), aspect);
        let mut tiles = Vec::new();
        for (i, u) in urls.iter().enumerate() {
            let (key, im) = self.img(u, THUMB);
            let g = geo[i];
            let has = im.is_some();
            let (img, anim) = self.tile_img(&key, im.map(|x| x.0));
            tiles.push(Tile { has, img, anim, x: g.x, y: g.y, w: g.w, h: g.h, url: u.clone().into(), key: key.into() });
        }
        let n = tiles.len() as i32;
        self.st(|s| {
            s.set_sendbox_tiles(model(tiles));
            s.set_sendbox_ratio(ratio);
            s.set_sendbox_count(n);
            s.set_sendbox_open(n > 0);
        });
    }

    pub fn sendbox_send(&mut self, caption: String) {
        let paths = std::mem::take(&mut self.sendbox);
        self.st(|s| s.set_sendbox_open(false));
        self.send_images(paths);
        if !caption.trim().is_empty() {
            self.send_text(caption);
        }
    }

    pub fn sendbox_cancel(&mut self) {
        self.sendbox.clear();
        self.st(|s| s.set_sendbox_open(false));
    }

    // ------------------------------------------------------------ строки сообщений

    pub fn rebuild_msgs(&mut self) {
        let Some(cur) = self.cur.as_ref() else { return };
        let kind = cur.kind;
        let group_chat = kind != 0;
        let my = self.me.id.clone();
        let mut msgs = cur.msgs.clone();
        let chat = cur.id.clone();
        let now = if kind == 3 { chrono::Local::now().naive_local() } else { crate::fp::now_msk() };
        let pinned = self.cfg.lock().unwrap().pinned_msgs.get(&chat).cloned();
        let mut pend_state: HashMap<u64, i32> = HashMap::new();
        for p in self.pending.iter().filter(|p| p.chat == chat) {
            let id = u64::MAX - p.local;
            pend_state.insert(id, p.state);
            let file = p.image.as_ref().map(|path| format!("file://{}", path.to_string_lossy()));
            msgs.push(Message {
                id,
                author_id: my.clone(),
                author: self.me.name.clone(),
                has_head: false,
                ts: Some(now),
                text: p.text.clone(),
                image: file.clone(),
                thumb: file,
                ..Default::default()
            });
        }
        let blocks = album::group(&msgs);
        let mut rows: Vec<MsgRow> = Vec::with_capacity(blocks.len() + 8);
        let mut last_day = None;
        let dark = self.app().map(|a| a.global::<Theme>().get_dark()).unwrap_or(false);
        let first_of = |b: &Block| -> usize {
            match b {
                Block::Single(i) => *i,
                Block::Album { images, .. } => images[0],
            }
        };
        let last_of = |b: &Block| -> usize {
            match b {
                Block::Single(i) => *i,
                Block::Album { images, caption } => caption.unwrap_or(*images.last().unwrap()),
            }
        };
        let same_group = |a: &Message, b: &Message| -> bool {
            if a.author_id != b.author_id || a.author != b.author || a.author_id == "0" {
                return false;
            }
            if !b.has_head {
                return true;
            }
            match (a.ts, b.ts) {
                (Some(x), Some(y)) => (y - x).num_seconds().abs() < 600 && x.date() == y.date(),
                _ => false,
            }
        };
        // для поиска цитат: (ключ, автор, текст в нижнем регистре)
        let mut seen: Vec<(String, String, String)> = Vec::new();
        let mut pin_seen = false;
        for (bi, b) in blocks.iter().enumerate() {
            let first = &msgs[first_of(b)];
            let last = &msgs[last_of(b)];
            if let Some(ts) = first.ts {
                let d = ts.date();
                if last_day != Some(d) {
                    last_day = Some(d);
                    rows.push(MsgRow { kind: 1, key: format!("d{d}").into(), text: parse::day_label(d, now.date()).into(), ..Default::default() });
                }
            }
            let prev = if bi > 0 { Some(&msgs[last_of(&blocks[bi - 1])]) } else { None };
            let next = blocks.get(bi + 1).map(|nb| &msgs[first_of(nb)]);
            let day_break_next = next.map(|n| n.ts.map(|t| t.date()) != last.ts.map(|t| t.date())).unwrap_or(false);
            let group_start = prev.map(|p| !same_group(p, first)).unwrap_or(true) || rows.last().map(|r| r.kind == 1).unwrap_or(false);
            let tail = next.map(|n| !same_group(last, n) || day_break_next).unwrap_or(true);
            let out = first.author_id == my && !my.is_empty();
            let system = first.author_id == "0";
            let show_author = (group_chat && !out && group_start) || (system && group_start);
            let show_avatar = group_chat && !out && tail;
            let (av_key, av_img) = if show_avatar && !parse::is_default_avatar(&first.avatar) { self.img(&first.avatar, AV) } else { (String::new(), None) };
            let (c1, c2) = pair(&first.author);
            let colors = if dark { NAME_COLORS_DARK } else { NAME_COLORS_LIGHT };
            let author_color = if system { if dark { rgb(0x6AB2FF) } else { rgb(0x3A7FD5) } } else { rgb(colors[hash(&first.author_id) % colors.len()]) };
            let is_pending = pend_state.contains_key(&first.id) || pend_state.contains_key(&last.id);
            let state = pend_state.get(&last.id).copied().or_else(|| pend_state.get(&first.id).copied()).unwrap_or(0);
            let time = if is_pending && state != 0 { parse::short_time(Some(now), "") } else { parse::short_time(last.ts, &last.date_raw) };
            let time_full = if is_pending && state != 0 { now.format("%H:%M:%S").to_string() } else { last.ts.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_default() };
            let (raw_text, tiles, ratio, is_album) = match b {
                Block::Single(i) => (msgs[*i].text.clone(), vec![], 0.0, false),
                Block::Album { images, caption } => {
                    let aspect = if images.len() == 1 {
                        let u = msgs[images[0]].thumb.clone().unwrap_or_default();
                        self.imgs.get(&imgcache::key(THUMB, &u)).map(|(_, w, h)| *h as f32 / (*w).max(1) as f32).unwrap_or(0.75)
                    } else {
                        0.75
                    };
                    let (geo, h) = album::layout(images.len(), aspect);
                    let mut tiles = Vec::new();
                    for (k, idx) in images.iter().enumerate() {
                        let m = &msgs[*idx];
                        let thumb = m.thumb.clone().or(m.image.clone()).unwrap_or_default();
                        let (key, im) = self.img(&thumb, THUMB);
                        let g = geo[k];
                        let has = im.is_some();
                        let (img, anim) = self.tile_img(&key, im.map(|x| x.0));
                        tiles.push(Tile {
                            has,
                            img,
                            anim,
                            x: g.x,
                            y: g.y,
                            w: g.w,
                            h: g.h,
                            url: m.image.clone().unwrap_or(thumb).into(),
                            key: key.into(),
                        });
                    }
                    (caption.map(|c| msgs[c].text.clone()).unwrap_or_default(), tiles, h, true)
                }
            };
            // ответ «╭─ ⤸ цитата\n╰ текст»
            let (quote, body) = parse::split_reply(&raw_text);
            let text = parse::display_text(&body);
            let (mut has_reply, mut reply_author, mut reply_text, mut reply_key) = (false, String::new(), String::new(), String::new());
            if let Some(q) = quote {
                has_reply = true;
                let qn = squash(&q).trim_end_matches('…').to_lowercase();
                reply_text = parse::display_text(&squash(&q));
                reply_author = "Ответ".into();
                let mut partial = None;
                for (k, a, t) in seen.iter().rev() {
                    if t.is_empty() {
                        continue;
                    }
                    if *t == qn {
                        partial = Some((k.clone(), a.clone()));
                        break;
                    }
                    if partial.is_none() && (t.starts_with(&qn) || qn.starts_with(t.as_str())) {
                        partial = Some((k.clone(), a.clone()));
                    }
                }
                if let Some((k, a)) = partial {
                    reply_key = k;
                    reply_author = a;
                }
            }
            let key = if is_pending { format!("p{}", u64::MAX - first.id) } else { first.id.to_string() };
            let author_name = if system { "FunPay".to_string() } else { first.author.clone() };
            seen.push((
                key.clone(),
                if out { "Вы".into() } else { author_name.clone() },
                if is_album && text.is_empty() { "изображение".into() } else { squash(&text).to_lowercase() },
            ));
            let spacer = if text.is_empty() {
                String::new()
            } else {
                let w = time.chars().count() as f32 * 6.4 + if out { 22.0 } else { 6.0 } + if state == 2 { 18.0 } else { 0.0 } + 12.0
                    + if pinned.as_ref().map(|p| p.key == key).unwrap_or(false) { 16.0 } else { 0.0 };
                nbsp((w / 3.9).ceil() as usize)
            };
            let orders = if system { parse::find_order_ids(&raw_text) } else { vec![] };
            let (has_links, link_spans, rich) = Ui::msg_links(&text, &spacer);
            let author_nick = if show_author && !system { self.nick(&author_name) } else { Default::default() };
            let is_pinned = pinned.as_ref().map(|p| p.key == key).unwrap_or(false);
            pin_seen |= is_pinned;
            rows.push(MsgRow {
                kind: 0,
                key: key.into(),
                out,
                group_start,
                tail,
                show_author,
                author: author_name.into(),
                author_id: first.author_id.clone().into(),
                author_color,
                show_avatar,
                has_avatar: av_img.is_some(),
                avatar: av_img.map(|a| a.0).unwrap_or_default(),
                avatar_key: av_key.into(),
                letter: letter(&first.author).into(),
                c1,
                c2,
                text: text.into(),
                spacer: spacer.into(),
                time: time.into(),
                time_full: time_full.into(),
                badge: first.badge.clone().into(),
                album: is_album,
                tiles: model(tiles),
                ratio,
                state,
                system,
                has_reply,
                reply_author: reply_author.into(),
                reply_text: reply_text.into(),
                reply_key: reply_key.into(),
                orders: model(orders.into_iter().map(SharedString::from).collect()),
                link: parse::first_url(&raw_text).unwrap_or_default().into(),
                has_links: has_links,
                links: link_spans,
                rich,
                nick: author_nick,
                pinned: is_pinned,
            });
        }
        let _ = pin_seen;
        self.st(|s| match &pinned {
            Some(p) => {
                s.set_pin_key(p.key.clone().into());
                s.set_pin_author(p.author.clone().into());
                s.set_pin_text(p.text.clone().into());
            }
            None => s.set_pin_key("".into()),
        });
        sync_model(&self.msgs_model, rows, |a, b| a.key == b.key);
    }

    fn row_by_key(&self, key: &str) -> Option<MsgRow> {
        (0..self.msgs_model.row_count()).filter_map(|i| self.msgs_model.row_data(i)).find(|r| r.key == key)
    }

    pub fn msg_action(&mut self, key: String, action: String) {
        let Some(row) = self.row_by_key(&key) else { return };
        let chat = self.cur.as_ref().map(|c| c.id.clone()).unwrap_or_default();
        let author = if row.out { "Вы".to_string() } else { row.author.to_string() };
        let text = if row.text.is_empty() && row.album { "Изображение".to_string() } else { row.text.to_string() };
        match action.as_str() {
            "reply" => {
                self.reply = Some(Reply { key: key.clone(), author: author.clone(), text: text.clone() });
                self.st(|s| {
                    s.set_reply_key(key.clone().into());
                    s.set_reply_author(if row.out { "себе".into() } else { author.clone().into() });
                    s.set_reply_text(squash(&text).into());
                });
                if let Some(app) = self.app() {
                    app.invoke_focus_input();
                }
            }
            "pin" => {
                let mut c = self.cfg.lock().unwrap();
                c.pinned_msgs.insert(chat, PinnedMsg { key, author, text: squash(&text).chars().take(200).collect() });
                c.save();
                drop(c);
                self.rebuild_msgs();
            }
            "unpin" => {
                let mut c = self.cfg.lock().unwrap();
                c.pinned_msgs.remove(&chat);
                c.save();
                drop(c);
                self.rebuild_msgs();
            }
            _ => {}
        }
    }

    pub fn cancel_reply(&mut self) {
        self.reply = None;
        self.st(|s| s.set_reply_key("".into()));
    }

    pub fn jump_to(&mut self, key: String) {
        if key.is_empty() {
            return;
        }
        if self.row_by_key(&key).is_none() {
            self.toast("Сообщение выше по истории, прокрутите вверх");
            return;
        }
        self.st(|s| {
            s.set_jump_key(key.clone().into());
            s.set_jump_n(s.get_jump_n() + 1);
        });
        slint::Timer::single_shot(std::time::Duration::from_millis(1600), move || {
            with_ui(|u| {
                u.st(|s| {
                    if s.get_jump_key() == key.as_str() {
                        s.set_jump_key("".into());
                    }
                });
            });
        });
    }

    // ------------------------------------------------------------ картинки

    pub fn image_ready(&mut self, key: String, img: Image, w: u32, h: u32) {
        self.imgs.insert(key.clone(), (img.clone(), w, h));
        if !self.ext.catalog.is_empty() {
            self.theme_preview_ready(&key);
        }
        for i in 0..self.chats_model.row_count() {
            if let Some(mut r) = self.chats_model.row_data(i) {
                if r.avatar_key == key.as_str() {
                    r.avatar = img.clone();
                    r.has_avatar = true;
                    self.chats_model.set_row_data(i, r);
                }
            }
        }
        let mut need_rebuild = false;
        for i in 0..self.msgs_model.row_count() {
            if let Some(mut r) = self.msgs_model.row_data(i) {
                if r.avatar_key == key.as_str() {
                    r.avatar = img.clone();
                    r.has_avatar = true;
                    self.msgs_model.set_row_data(i, r.clone());
                }
                let tiles = r.tiles.clone();
                for t in 0..tiles.row_count() {
                    if let Some(mut tile) = tiles.row_data(t) {
                        if tile.key == key.as_str() {
                            let (ti, ta) = self.tile_img(&key, Some(img.clone()));
                            tile.img = ti;
                            tile.anim = ta;
                            tile.has = true;
                            tiles.set_row_data(t, tile);
                            if tiles.row_count() == 1 {
                                need_rebuild = true;
                            }
                        }
                    }
                }
            }
        }
        if need_rebuild {
            self.rebuild_msgs();
        }
        if key == imgcache::key(BIG_AV, &self.me.avatar) {
            self.update_me();
        }
        if !self.card_url.is_empty() && key == imgcache::key(BIG_AV, &self.card_url) {
            self.st(|s| {
                s.set_card_avatar(img.clone());
                s.set_card_has_avatar(true);
            });
        }
        if let Some(p) = &self.profile {
            if key == imgcache::key(BIG_AV, &p.avatar) {
                self.st(|s| {
                    s.set_pf_avatar(img.clone());
                    s.set_pf_has_avatar(true);
                });
            }
        }
        if let Some(o) = &self.order {
            if key == imgcache::key(AV, &o.peer_avatar) {
                self.st(|s| {
                    s.set_od_peer_avatar(img.clone());
                    s.set_od_peer_has_avatar(true);
                });
            }
        }
        if key.starts_with(&format!("{STK}|")) {
            self.refresh_sticker_images(&key, &img);
        }
        if !self.ext.trades.is_empty() {
            self.deals_image(&key);
        }
        self.desc_image(&key, &img);
        self.accounts_image(&key);
        if !self.sendbox.is_empty() && key.starts_with(&format!("{THUMB}|file://")) {
            self.rebuild_sendbox();
        }
        if !self.viewer_url.is_empty() && (imgcache::key(FULL, &self.viewer_url) == key || self.ext.anims.contains_key(&key) && key.ends_with(&format!("|{}", self.viewer_url))) {
            let (vi, va) = self.viewer_anim(&self.viewer_url.clone()).unwrap_or((img.clone(), Default::default()));
            self.st(|s| {
                s.set_viewer_img(vi);
                s.set_viewer_anim(va);
                s.set_viewer_has(true);
            });
        }
    }

    pub fn open_image(&mut self, url: String) {
        // список всех картинок переписки, чтобы листать стрелками
        let mut list = Vec::new();
        for i in 0..self.msgs_model.row_count() {
            if let Some(r) = self.msgs_model.row_data(i) {
                for t in 0..r.tiles.row_count() {
                    if let Some(tile) = r.tiles.row_data(t) {
                        list.push(tile.url.to_string());
                    }
                }
            }
        }
        if !list.contains(&url) {
            list = vec![url.clone()];
        }
        self.viewer_list = list;
        self.show_viewer(url);
    }

    fn show_viewer(&mut self, url: String) {
        self.viewer_url = url.clone();
        let idx = self.viewer_list.iter().position(|u| *u == url).unwrap_or(0) as i32;
        let count = self.viewer_list.len() as i32;
        let (_, im) = self.img(&url, FULL);
        let thumb = self.imgs.iter().find(|(k, _)| k.ends_with(&format!("|{url}"))).map(|(_, v)| v.0.clone());
        let anim = self.viewer_anim(&url);
        self.st(|s| {
            s.set_viewer_url(url.clone().into());
            s.set_viewer_open(true);
            s.set_viewer_index(idx);
            s.set_viewer_count(count);
            match anim.map(|(i, a)| (i, a)).or(im.map(|x| (x.0, Default::default()))).or(thumb.map(|t| (t, Default::default()))) {
                Some((i, a)) => {
                    s.set_viewer_img(i);
                    s.set_viewer_anim(a);
                    s.set_viewer_has(true);
                }
                None => s.set_viewer_has(false),
            }
        });
    }

    pub fn viewer_step(&mut self, d: i32) {
        let idx = self.viewer_list.iter().position(|u| *u == self.viewer_url).unwrap_or(0) as i32 + d;
        if idx >= 0 && (idx as usize) < self.viewer_list.len() {
            let u = self.viewer_list[idx as usize].clone();
            self.show_viewer(u);
        }
    }

    pub fn avatar_uploaded(&mut self, res: Result<Me, String>) {
        self.st(|s| s.set_avatar_busy(false));
        match res {
            Ok(me) => {
                self.me = me;
                self.update_me();
                self.toast("Аватарка обновлена");
            }
            Err(e) => self.toast(&e),
        }
    }

    pub fn apply_theme(&mut self) {
        let (mode, tg) = {
            let c = self.cfg.lock().unwrap();
            (c.theme, c.tg_theme.clone())
        };
        if let Some(t) = tg {
            if !self.ext.tg_applied {
                self.apply_tg(&t);
            }
            return;
        }
        if self.ext.tg_applied {
            self.ext.tg_applied = false;
            if let Some(app) = self.app() {
                app.global::<Theme>().set_custom(false);
                app.global::<Theme>().set_c_has_wall_img(false);
            }
            self.st(|s| s.set_tg_theme("".into()));
            self.rebuild_msgs();
        }
        let dark = match mode {
            1 => false,
            2 => true,
            _ => crate::sys::os_dark(),
        };
        if let Some(app) = self.app() {
            if app.global::<Theme>().get_dark() != dark {
                app.global::<Theme>().set_dark(dark);
                // цвета ников зависят от темы
                self.epic_loaded();
            }
        }
    }

    // ------------------------------------------------------------ экраны: стек

    fn push_pane(&mut self, p: i32) {
        if self.panes.last() != Some(&p) {
            self.panes.retain(|x| *x != p);
            self.panes.push(p);
        }
        self.st(|s| {
            s.set_pane(p);
            s.set_card_open(false);
            s.set_settings_open(false);
        });
    }

    pub fn pane_back(&mut self) {
        self.panes.pop();
        let p = self.panes.last().copied().unwrap_or(0);
        self.st(|s| s.set_pane(p));
    }

    // ------------------------------------------------------------ карточка и профиль

    pub fn open_card(&mut self) {
        let Some(cur) = &self.cur else { return };
        let (kind, title, peer, avatar) = (cur.kind, cur.title.clone(), cur.peer_id.clone(), cur.peer_avatar.clone());
        let subtitle = self.st(|s| s.get_cur_subtitle().to_string()).unwrap_or_default();
        let online = self.st(|s| s.get_cur_online()).unwrap_or(false);
        let (c1, c2) = match kind {
            1 => (rgb(0x6FB2FF), rgb(0x2F6FD0)),
            3 => (rgb(0x5B8DEF), rgb(0x6A5BE2)),
            _ => pair(&title),
        };
        self.card_url = if parse::is_default_avatar(&avatar) { String::new() } else { avatar.clone() };
        let av = self.img_opt(&self.card_url.clone(), BIG_AV);
        let dark_now = self.dark();
        self.st(|s| {
            s.set_card_name(title.clone().into());
            s.set_card_nick(crate::epic::style(&title, dark_now));
            s.set_card_status(subtitle.into());
            s.set_card_online(online);
            s.set_card_letter(letter(&title).into());
            s.set_card_c1(c1);
            s.set_card_c2(c2);
            s.set_card_has_avatar(av.is_some());
            if let Some(i) = av {
                s.set_card_avatar(i);
            }
            s.set_card_id(peer.clone().into());
            s.set_card_reg("".into());
            s.set_card_rating("".into());
            s.set_card_reviews("".into());
            s.set_card_loading(kind == 0 && !peer.is_empty());
            s.set_card_open(true);
        });
        self.ext.card_uid = if kind == 0 { peer.clone() } else { String::new() };
        self.st(|s| s.set_card_desc("".into()));
        if kind == 0 && !peer.is_empty() {
            self.load_desc(peer.clone());
            self.pf_gen += 1;
            self.send(Cmd::LoadProfile { uid: peer, gen: self.pf_gen, card: true });
        }
    }

    pub fn open_profile(&mut self, uid: String) {
        if uid.is_empty() || uid == "0" || !uid.chars().all(|c| c.is_ascii_digit()) {
            return;
        }
        self.pf_gen += 1;
        self.profile = None;
        let (c1, c2) = pair(&uid);
        let is_me = uid == self.me.id;
        self.push_pane(1);
        self.st(|s| {
            s.set_pf_loading(true);
            s.set_pf_error("".into());
            s.set_pf_is_me(is_me);
            s.set_pf_id(uid.clone().into());
            s.set_pf_tab(0);
            s.set_pf_has_avatar(false);
            s.set_pf_c1(c1);
            s.set_pf_c2(c2);
            s.set_pf_offers(model(vec![]));
            s.set_pf_reviews(model(vec![]));
        });
        self.ext.banner_url.clear();
        self.st(|s| {
            s.set_pf_desc("".into());
            s.set_pf_has_banner(false);
        });
        self.load_desc(uid.clone());
        self.send(Cmd::LoadProfile { uid, gen: self.pf_gen, card: false });
    }

    pub fn profile_loaded(&mut self, gen: u64, card: bool, res: Result<parse::Profile, String>) {
        if card {
            self.st(|s| s.set_card_loading(false));
            if let Ok(p) = res {
                if !p.avatar.is_empty() && self.card_url.is_empty() && !parse::is_default_avatar(&p.avatar) {
                    self.card_url = p.avatar.clone();
                    if let Some(i) = self.img_opt(&p.avatar, BIG_AV) {
                        self.st(|s| {
                            s.set_card_avatar(i);
                            s.set_card_has_avatar(true);
                        });
                    }
                }
                self.st(|s| {
                    s.set_card_reg(p.reg_date.clone().into());
                    s.set_card_rating(if p.rating.is_empty() { "Нет оценок".into() } else { format!("{} из 5", p.rating).into() });
                    s.set_card_reviews(if p.reviews_count.is_empty() { "рейтинг".into() } else { p.reviews_count.to_lowercase().into() });
                    if !p.status.is_empty() {
                        s.set_card_status(p.status.to_lowercase().into());
                        s.set_card_online(p.online);
                    }
                });
            }
            return;
        }
        if gen != self.pf_gen {
            return;
        }
        match res {
            Err(e) => {
                self.st(|s| {
                    s.set_pf_loading(false);
                    s.set_pf_error(e.into());
                });
            }
            Ok(p) => {
                let mut offers = Vec::new();
                for g in &p.groups {
                    offers.push(OfferRow { header: true, title: g.name.clone().into(), ..Default::default() });
                    for o in &g.offers {
                        offers.push(OfferRow { header: false, title: o.title.clone().into(), price: o.price.clone().into(), server: o.server.clone().into(), url: o.url.clone().into() });
                    }
                }
                let reviews: Vec<ReviewRow> = p.reviews.iter().map(review_row).collect();
                let av = self.img_opt(&p.avatar, BIG_AV);
                let count_num: String = p.reviews_count.chars().filter(|c| c.is_ascii_digit()).collect();
                let is_me = p.user_id == self.me.id;
                let (c1, c2) = pair(&p.name);
                let dark_now = self.dark();
                self.st(|s| {
                    s.set_pf_loading(false);
                    s.set_pf_name(p.name.clone().into());
                    s.set_pf_nick(crate::epic::style(&p.name, dark_now));
                    s.set_pf_letter(letter(&p.name).into());
                    s.set_pf_c1(c1);
                    s.set_pf_c2(c2);
                    s.set_pf_status(p.status.to_lowercase().into());
                    s.set_pf_online(p.online);
                    s.set_pf_reg(p.reg_date.clone().into());
                    s.set_pf_reg_ago(p.reg_ago.clone().into());
                    s.set_pf_rating(p.rating.clone().into());
                    s.set_pf_reviews_count(count_num.into());
                    s.set_pf_offers(model(offers));
                    s.set_pf_reviews(model(reviews));
                    s.set_pf_more(!p.cont.is_empty());
                    s.set_pf_can_write(!is_me);
                    s.set_pf_has_avatar(av.is_some());
                    if let Some(i) = av {
                        s.set_pf_avatar(i);
                    }
                });
                self.profile = Some(p);
            }
        }
    }

    pub fn pf_more_reviews(&mut self) {
        let Some(p) = &self.profile else { return };
        if p.cont.is_empty() {
            return;
        }
        let (uid, cont) = (p.user_id.clone(), p.cont.clone());
        self.st(|s| s.set_pf_more_loading(true));
        self.send(Cmd::MoreReviews { uid, cont, gen: self.pf_gen });
    }

    pub fn reviews_loaded(&mut self, gen: u64, res: Result<(Vec<parse::Review>, String), String>) {
        self.st(|s| s.set_pf_more_loading(false));
        if gen != self.pf_gen {
            return;
        }
        let Ok((list, cont)) = res else { return };
        if let Some(p) = self.profile.as_mut() {
            p.reviews.extend(list);
            p.cont = cont.clone();
            let rows: Vec<ReviewRow> = p.reviews.iter().map(review_row).collect();
            self.st(|s| {
                s.set_pf_reviews(model(rows));
                s.set_pf_more(!cont.is_empty());
            });
        }
    }

    pub fn pf_write(&mut self) {
        let Some(p) = self.profile.clone() else { return };
        let node = if !p.chat_node.is_empty() {
            p.chat_node.clone()
        } else {
            let (a, b) = (p.user_id.parse::<u64>().unwrap_or(0), self.me.id.parse::<u64>().unwrap_or(0));
            format!("users-{}-{}", a.min(b), a.max(b))
        };
        // если чат уже есть в списке, открываем его по обычному id
        let existing = self.chats.iter().find(|c| c.name == p.name).map(|c| c.id.clone());
        self.open_chat_named(existing.unwrap_or(node), 0, p.name.clone());
    }

    // ------------------------------------------------------------ заказ

    pub fn open_order(&mut self, id: String) {
        if id.is_empty() {
            return;
        }
        self.od_gen += 1;
        self.order = None;
        self.push_pane(2);
        self.st(|s| {
            s.set_od_loading(true);
            s.set_od_error("".into());
            s.set_od_id(id.clone().into());
            s.set_od_dialog(0);
            s.set_od_busy(false);
        });
        self.send(Cmd::LoadOrder { id, gen: self.od_gen });
    }

    pub fn order_loaded(&mut self, gen: u64, res: Result<parse::Order, String>) {
        if gen != self.od_gen {
            return;
        }
        match res {
            Err(e) => {
                self.st(|s| {
                    s.set_od_loading(false);
                    s.set_od_busy(false);
                    s.set_od_error(e.into());
                });
            }
            Ok(mut o) => {
                // на странице заказа аватарки может не быть: берём известную из сделок или чатов,
                // иначе спрашиваем профиль собеседника
                if o.peer_avatar.is_empty() || parse::is_default_avatar(&o.peer_avatar) {
                    let known = self
                        .ext
                        .trades
                        .iter()
                        .find(|t| (t.id == o.id || (!o.peer_id.is_empty() && t.peer_id == o.peer_id)) && !t.peer_avatar.is_empty() && !parse::is_default_avatar(&t.peer_avatar))
                        .map(|t| t.peer_avatar.clone())
                        .or_else(|| {
                            self.chats
                                .iter()
                                .find(|c| !o.peer_name.is_empty() && c.name == o.peer_name && !c.avatar.is_empty() && !parse::is_default_avatar(&c.avatar))
                                .map(|c| c.avatar.clone())
                        });
                    match known {
                        Some(u) => o.peer_avatar = u,
                        None if !o.peer_id.is_empty() => self.send(Cmd::OrderAvatar { uid: o.peer_id.clone(), gen }),
                        None => {}
                    }
                }
                self.order = Some(o.clone());
                let av = self.img_opt(&o.peer_avatar, AV);
                let params: Vec<KV> = o.params.iter().map(|(k, v)| KV { k: k.clone().into(), v: v.clone().into() }).collect();
                self.st(|s| {
                    s.set_od_loading(false);
                    s.set_od_busy(false);
                    s.set_od_id(o.id.clone().into());
                    s.set_od_status(o.status.clone().into());
                    s.set_od_game(o.game.clone().into());
                    s.set_od_desc(o.short_desc.clone().into());
                    s.set_od_price(o.price.clone().into());
                    s.set_od_peer(o.peer_name.clone().into());
                    s.set_od_peer_letter(letter(&o.peer_name).into());
                    s.set_od_peer_id(o.peer_id.clone().into());
                    s.set_od_peer_has_avatar(av.is_some());
                    if let Some(i) = av {
                        s.set_od_peer_avatar(i);
                    }
                    s.set_od_params(model(params));
                    s.set_od_can_refund(o.can_refund);
                    s.set_od_can_confirm(o.can_confirm);
                    s.set_od_is_buyer(o.is_buyer);
                    s.set_od_has_review(o.has_review);
                    s.set_od_rating(o.review_rating);
                    s.set_od_review(parse::display_text(&o.review_text).into());
                    s.set_od_reply(parse::display_text(&o.review_reply).into());
                    s.set_od_reply_mine(o.reply_mine || (!o.is_buyer && !o.review_reply.is_empty()));
                });
                self.order = Some(o);
            }
        }
    }

    pub fn od_action(&mut self, action: String, text: String, rating: i32) {
        let Some(o) = self.order.clone() else { return };
        if action == "chat" {
            let node = if !o.chat_node.is_empty() {
                o.chat_node.clone()
            } else {
                let (a, b) = (o.peer_id.parse::<u64>().unwrap_or(0), self.me.id.parse::<u64>().unwrap_or(0));
                format!("users-{}-{}", a.min(b), a.max(b))
            };
            self.open_chat_named(node, 0, o.peer_name.clone());
            return;
        }
        self.st(|s| s.set_od_busy(true));
        self.send(Cmd::OrderAction { id: o.id.clone(), action, text, rating, gen: self.od_gen });
    }

    // ------------------------------------------------------------ браузер

    pub fn open_url(&mut self, url: String) {
        if url.is_empty() {
            return;
        }
        let is_fp = url.contains("funpay.com");
        if !is_fp || !crate::browser::available() {
            crate::sys::open_external(&url);
            return;
        }
        // уже открыта такая вкладка: просто показываем её
        if let Some(t) = self.ext.tabs.iter().find(|t| t.url.trim_end_matches('/') == url.trim_end_matches('/')) {
            let id = t.id;
            self.tab_activate(id);
            self.st(|s| {
                s.set_mini_open(true);
                s.set_mini_collapsed(false);
            });
            return;
        }
        let cookies = {
            let c = self.cfg.lock().unwrap();
            let mut v = vec![("golden_key".to_string(), c.golden_key.clone())];
            v.extend(c.cookies.iter().filter(|(k, _)| *k != "golden_key").map(|(k, v)| (k.clone(), v.clone())));
            v
        };
        let Some(app) = self.app() else { return };
        self.ext.tab_next += 1;
        let id = self.ext.tab_next;
        match crate::browser::open_tab(&app, id, &url, &cookies) {
            Ok(()) => {
                // не больше 8 вкладок: закрываем самую старую
                while self.ext.tabs.len() >= 8 {
                    let old = self.ext.tabs.remove(0);
                    crate::browser::close_tab(old.id);
                }
                self.ext.tabs.push(Tab { id, title: String::new(), url: url.clone(), loading: true });
                self.ext.tab_active = id;
                self.st(|s| {
                    s.set_mini_open(true);
                    s.set_mini_collapsed(false);
                    s.set_card_open(false);
                    s.set_settings_open(false);
                });
                self.rebuild_tabs();
            }
            Err(e) => {
                self.toast(&e);
                crate::sys::open_external(&url);
            }
        }
    }

    fn rebuild_tabs(&mut self) {
        let active = self.ext.tab_active;
        let idx = self.ext.tabs.iter().position(|t| t.id == active).map(|i| i as i32).unwrap_or(-1);
        let rows: Vec<BrowserTab> = self
            .ext
            .tabs
            .iter()
            .map(|t| BrowserTab {
                id: t.id,
                title: tab_title_text(t).into(),
                url: t.url.clone().into(),
                loading: t.loading,
            })
            .collect();
        let cur_url = self.ext.tabs.iter().find(|t| t.id == active).map(|t| t.url.clone()).unwrap_or_default();
        self.st(|s| {
            s.set_tabs(model(rows));
            s.set_tab_active(idx);
            if !s.get_browser_login() {
                s.set_browser_url(cur_url.into());
            }
        });
    }

    fn tab_activate(&mut self, id: i32) {
        self.ext.tab_active = id;
        crate::browser::show_tab(id);
        self.rebuild_tabs();
    }

    /// Заголовок страницы во вкладке (вкладка 0 — вход через сайт).
    pub fn tab_title(&mut self, id: i32, t: String) {
        if id == 0 {
            self.st(|s| s.set_browser_title(t.into()));
            return;
        }
        if let Some(tab) = self.ext.tabs.iter_mut().find(|x| x.id == id) {
            tab.title = t;
            self.rebuild_tabs();
        }
    }

    pub fn tab_state(&mut self, id: i32, url: String, loading: bool) {
        if id == 0 {
            self.st(|s| {
                s.set_browser_url(url.into());
                s.set_browser_loading(loading);
            });
            if self.st(|s| s.get_browser_login()).unwrap_or(false) {
                self.login_poll = 0;
                // новая страница (например, после ввода кода): тот же ключ можно проверить ещё раз
                if !loading {
                    self.ext.login_tried.clear();
                }
            }
            return;
        }
        if let Some(tab) = self.ext.tabs.iter_mut().find(|x| x.id == id) {
            if !url.is_empty() && url != "about:blank" {
                tab.url = url;
            }
            tab.loading = loading;
            self.rebuild_tabs();
        }
    }

    pub fn tab_select(&mut self, i: i32) {
        if let Some(t) = self.ext.tabs.get(i.max(0) as usize) {
            let id = t.id;
            self.tab_activate(id);
            self.st(|s| s.set_mini_collapsed(false));
        }
    }

    pub fn tab_close(&mut self, i: i32) {
        let i = i.max(0) as usize;
        if i >= self.ext.tabs.len() {
            return;
        }
        let t = self.ext.tabs.remove(i);
        crate::browser::close_tab(t.id);
        if self.ext.tabs.is_empty() {
            self.ext.tab_active = -1;
            self.st(|s| {
                s.set_mini_open(false);
                s.set_mini_collapsed(false);
                s.set_mini_big(false);
            });
        } else if t.id == self.ext.tab_active {
            let next = self.ext.tabs[i.min(self.ext.tabs.len() - 1)].id;
            self.tab_activate(next);
            return;
        }
        self.rebuild_tabs();
    }

    pub fn mini_collapse(&mut self, collapsed: bool) {
        self.st(|s| s.set_mini_collapsed(collapsed));
    }

    pub fn mini_close_all(&mut self) {
        for t in std::mem::take(&mut self.ext.tabs) {
            crate::browser::close_tab(t.id);
        }
        self.ext.tab_active = -1;
        self.st(|s| {
            s.set_mini_open(false);
            s.set_mini_collapsed(false);
            s.set_mini_big(false);
        });
        self.rebuild_tabs();
    }

    pub fn browser_close(&mut self) {
        let login = self.st(|s| s.get_browser_login()).unwrap_or(false);
        if login {
            self.st(|s| s.set_browser_login(false));
            crate::browser::close_tab(0);
            return;
        }
        // крестик в мини-окне закрывает текущую вкладку
        if let Some(i) = self.ext.tabs.iter().position(|t| t.id == self.ext.tab_active) {
            self.tab_close(i as i32);
        }
    }

    pub fn login_browser(&mut self) {
        let Some(app) = self.app() else { return };
        if !crate::browser::available() {
            crate::sys::open_external("https://funpay.com/account/login");
            self.st(|s| {
                s.set_login_page(2);
                s.set_login_error("Войдите на сайте, затем скопируйте golden_key сюда".into());
            });
            return;
        }
        // старые куки из прошлого входа убираем, иначе их примут за новый вход
        match crate::browser::open(&app, "about:blank", &[]) {
            Ok(()) => {
                crate::browser::forget_funpay();
                crate::browser::load_tab(0, "https://funpay.com/account/login");
                self.login_poll = 0;
                self.ext.login_tried.clear();
                self.ext.login_checking = false;
                self.st(|s| {
                    s.set_browser_login(true);
                    s.set_login_error("".into());
                    s.set_browser_url("https://funpay.com/account/login".into());
                });
            }
            Err(e) => self.st(|s| s.set_login_error(e.into())).unwrap_or(()),
        }
    }

    /// Таймер: положение встроенного браузера и проверка, вошёл ли человек на сайте.
    pub fn browser_tick(&mut self) {
        let Some(app) = self.app() else { return };
        let st = app.global::<St>();
        let login = st.get_browser_login();
        let mini = st.get_logged_in() && st.get_mini_open() && !st.get_mini_collapsed();
        let show = (login || mini) && !app.get_overlay_open();
        if login {
            crate::browser::show_tab(0);
        } else if mini {
            crate::browser::show_tab(self.ext.tab_active);
        }
        crate::browser::sync(&app, show);
        if login {
            self.login_poll += 1;
            // Ключ в куках ещё не значит, что вход завершён: при входе с кодом FunPay может выдать его раньше.
            // Поэтому окно не закрываем, а тихо проверяем ключ. Закроется само, когда FunPay его примет.
            if self.login_poll % 6 == 0 && !st.get_browser_loading() && !self.ext.login_checking && !st.get_login_busy() {
                if crate::browser::user_agent().is_empty() {
                    crate::browser::request_ua();
                }
                let cookies = crate::browser::funpay_cookies();
                let gk = cookies.iter().find(|(k, _)| k == "golden_key").map(|(_, v)| v.clone()).unwrap_or_default();
                if !gk.is_empty() && !self.ext.login_tried.contains(&gk) {
                    self.ext.login_tried.push(gk);
                    self.ext.login_checking = true;
                    self.send(Cmd::LoginCookies { cookies, ua: crate::browser::user_agent() });
                }
            }
        }
    }

    /// Итог проверки ключа из браузера входа.
    pub fn login_browser_done(&mut self, ok: bool, err: String) {
        self.ext.login_checking = false;
        if ok {
            self.st(|s| s.set_browser_login(false));
            crate::browser::close_tab(0);
        } else if !err.is_empty() {
            // человек ещё входит (например, вводит код): ждём дальше, окно не трогаем
            crate::log::write(&format!("вход через браузер: ключ пока не принят: {err}"));
        }
    }

    // ------------------------------------------------------------ стикеры

    pub fn stickers_toggle(&mut self) {
        let open = !self.st(|s| s.get_stickers_open()).unwrap_or(false);
        self.st(|s| s.set_stickers_open(open));
        if open && self.stickers.is_none() && !self.stickers_loading {
            self.stickers_loading = true;
            self.st(|s| s.set_stickers_loading(true));
            std::thread::spawn(|| {
                let store = crate::stickers::fetch();
                post(move |u| u.stickers_loaded(store));
            });
        }
        if open {
            self.rebuild_stickers();
        }
    }

    pub fn stickers_loaded(&mut self, store: Option<crate::stickers::Store>) {
        self.stickers_loading = false;
        self.st(|s| s.set_stickers_loading(false));
        if store.is_none() {
            self.toast("Стикеры не загрузились, проверьте интернет");
        }
        self.stickers = store;
        let has_recent = !self.cfg.lock().unwrap().recent_stickers.is_empty();
        if !has_recent {
            self.st(|s| s.set_sticker_pack(1));
        }
        self.rebuild_stickers();
    }

    pub fn sticker_pack(&mut self, i: i32) {
        self.st(|s| s.set_sticker_pack(i));
        self.rebuild_stickers();
    }

    fn rebuild_stickers(&mut self) {
        let Some(store) = self.stickers.clone() else { return };
        let mut packs = vec![PackRow { id: "recent".into(), title: "Недавние".into(), recent: true, ..Default::default() }];
        for p in &store.packs {
            let icon = self.img_opt(&store.thumb(p, &p.icon), STK);
            packs.push(PackRow { id: p.id.clone().into(), title: p.title.clone().into(), has_icon: icon.is_some(), icon: icon.unwrap_or_default(), recent: false });
        }
        let sel = self.st(|s| s.get_sticker_pack()).unwrap_or(0).max(0) as usize;
        let keys: Vec<String> = if sel == 0 {
            self.cfg.lock().unwrap().recent_stickers.clone()
        } else {
            store.packs.get(sel - 1).map(|p| p.files.iter().map(|f| format!("{}/{}", p.id, f)).collect()).unwrap_or_default()
        };
        let mut cells = Vec::new();
        for k in keys {
            if let Some((p, f)) = store.find(&k) {
                let im = self.img_opt(&store.thumb(p, &f), STK);
                cells.push(StickerCell { key: k.clone().into(), has: im.is_some(), img: im.unwrap_or_default() });
            }
        }
        self.st(|s| {
            s.set_sticker_packs(model(packs));
            s.set_sticker_cells(model(cells));
        });
    }

    fn refresh_sticker_images(&mut self, _key: &str, _img: &Image) {
        if self.st(|s| s.get_stickers_open()).unwrap_or(false) {
            self.rebuild_stickers();
        }
    }

    pub fn sticker_send(&mut self, key: String) {
        let Some(store) = self.stickers.clone() else { return };
        let Some((p, f)) = store.find(&key) else { return };
        let url = store.full(p, &f);
        {
            let mut c = self.cfg.lock().unwrap();
            c.recent_stickers.retain(|k| *k != key);
            c.recent_stickers.insert(0, key.clone());
            c.recent_stickers.truncate(24);
            c.save();
        }
        std::thread::spawn(move || match crate::stickers::prepare_png(&url, &key) {
            Some(path) => post(move |u| u.send_images(vec![path])),
            None => post(|u| u.toast("Стикер не загрузился")),
        });
    }
}

fn review_row(r: &parse::Review) -> ReviewRow {
    ReviewRow {
        rating: r.rating,
        text: parse::display_text(&r.text).into(),
        date: r.date.clone().into(),
        detail: r.detail.clone().into(),
        reply: parse::display_text(&r.reply).into(),
        order: r.order.clone().into(),
        author: r.author.clone().into(),
    }
}

/// Обновляет модель минимально: совпадающий префикс по ключу правим на месте, хвост дописываем.
pub fn sync_model<T: Clone + PartialEq + 'static>(model: &Rc<VecModel<T>>, rows: Vec<T>, same: impl Fn(&T, &T) -> bool) {
    let old = model.row_count();
    let common = old.min(rows.len());
    let mut prefix_ok = true;
    for i in 0..common {
        if !same(&model.row_data(i).unwrap(), &rows[i]) {
            prefix_ok = false;
            break;
        }
    }
    if !prefix_ok || old == 0 {
        model.set_vec(rows);
        return;
    }
    for (i, r) in rows.iter().enumerate().take(common) {
        if model.row_data(i).as_ref() != Some(r) {
            model.set_row_data(i, r.clone());
        }
    }
    if rows.len() > old {
        for r in rows.into_iter().skip(old) {
            model.push(r);
        }
    } else {
        for i in (rows.len()..old).rev() {
            model.remove(i);
        }
    }
}

// ---------------------------------------------------------------- сделки и финансы

impl Ui {
    pub fn open_deals(&mut self, tab: i32) {
        self.push_pane(4);
        self.deals_load(tab, self.ext.deals_state.clone());
        self.send(Cmd::LoadFrozen);
    }

    pub fn frozen_loaded(&mut self, text: String) {
        self.ext.frozen = text.clone();
        self.st(|s| s.set_fin_frozen(text.into()));
    }

    pub fn deals_tab(&mut self, tab: i32) {
        if tab == self.ext.deals_tab && (!self.ext.trades.is_empty() || !self.ext.txns.is_empty()) {
            return;
        }
        self.deals_load(tab, self.ext.deals_state.clone());
    }

    pub fn deals_filter(&mut self, state: String) {
        self.deals_load(self.ext.deals_tab, state);
    }

    fn deals_load(&mut self, tab: i32, state: String) {
        self.ext.deals_gen += 1;
        self.ext.deals_tab = tab;
        self.ext.deals_state = state.clone();
        self.ext.deals_next.clear();
        self.ext.trades.clear();
        self.ext.txns.clear();
        self.st(|s| {
            s.set_deals_tab(tab);
            s.set_deals_state(state.clone().into());
            s.set_deals_loading(true);
            s.set_deals_error("".into());
            s.set_deals_more(false);
            s.set_deals(model(vec![]));
            s.set_txns(model(vec![]));
        });
        let state = if tab == 2 { String::new() } else { state };
        self.send(Cmd::LoadTrade { tab, state, cont: String::new(), gen: self.ext.deals_gen });
    }

    pub fn deals_more(&mut self) {
        if self.ext.deals_next.is_empty() || self.st(|s| s.get_deals_more_loading()).unwrap_or(true) {
            return;
        }
        self.st(|s| s.set_deals_more_loading(true));
        let tab = self.ext.deals_tab;
        let state = if tab == 2 { String::new() } else { self.ext.deals_state.clone() };
        self.send(Cmd::LoadTrade { tab, state, cont: self.ext.deals_next.clone(), gen: self.ext.deals_gen });
    }

    pub fn trade_loaded(&mut self, gen: u64, append: bool, res: Result<(Vec<crate::money::TradeItem>, String), String>) {
        if gen != self.ext.deals_gen {
            return;
        }
        self.st(|s| {
            s.set_deals_loading(false);
            s.set_deals_more_loading(false);
        });
        match res {
            Ok((rows, next)) => {
                if !append {
                    self.ext.trades.clear();
                }
                for r in rows {
                    if !self.ext.trades.iter().any(|x| x.id == r.id) {
                        self.ext.trades.push(r);
                    }
                }
                self.ext.deals_next = next;
                self.rebuild_deals();
            }
            Err(e) => {
                if append {
                    self.toast(&e);
                } else {
                    self.st(|s| s.set_deals_error(e.into()));
                }
            }
        }
    }

    pub fn txns_loaded(&mut self, gen: u64, append: bool, res: Result<(Vec<crate::money::Txn>, String), String>) {
        if gen != self.ext.deals_gen {
            return;
        }
        self.st(|s| {
            s.set_deals_loading(false);
            s.set_deals_more_loading(false);
        });
        match res {
            Ok((rows, next)) => {
                if !append {
                    self.ext.txns.clear();
                }
                for r in rows {
                    if !self.ext.txns.iter().any(|x| x.id == r.id) {
                        self.ext.txns.push(r);
                    }
                }
                self.ext.deals_next = next;
                self.rebuild_deals();
            }
            Err(e) => {
                if append {
                    self.toast(&e);
                } else {
                    self.st(|s| s.set_deals_error(e.into()));
                }
            }
        }
    }

    fn rebuild_deals(&mut self) {
        let more = !self.ext.deals_next.is_empty();
        if self.ext.deals_tab == 2 {
            let rows: Vec<TxnRow> = self
                .ext
                .txns
                .iter()
                .map(|t| TxnRow {
                    id: t.id.clone().into(),
                    title: t.title.clone().into(),
                    state: match t.state {
                        crate::money::TxnState::Complete => 0,
                        crate::money::TxnState::Waiting => 1,
                        crate::money::TxnState::Cancel => 2,
                    },
                    amount: crate::money::fmt_amount(t.amount, &t.currency).into(),
                    income: t.amount > 0.0,
                    date: t.date.clone().into(),
                    payment: t.payment.clone().into(),
                    order: t.order_id.clone().into(),
                })
                .collect();
            let mut frozen = self.ext.frozen.clone();
            if frozen.is_empty() {
                frozen = crate::money::frozen_text(&crate::money::frozen_from_txns(&self.ext.txns, crate::fp::now_msk()));
            }
            self.st(|s| {
                s.set_txns(model(rows));
                s.set_fin_frozen(frozen.into());
                s.set_deals_more(more);
            });
            return;
        }
        let list = self.ext.trades.clone();
        let mut rows = Vec::with_capacity(list.len());
        for t in &list {
            let (c1, c2) = pair(if t.peer.is_empty() { &t.id } else { &t.peer });
            let av = if t.peer_avatar.is_empty() { None } else { self.img_opt(&t.peer_avatar, AV) };
            rows.push(TradeRow {
                id: t.id.clone().into(),
                state: match t.state {
                    crate::money::TradeState::Closed => 0,
                    crate::money::TradeState::Paid => 1,
                    crate::money::TradeState::Refunded => 2,
                },
                status: t.status_text.clone().into(),
                desc: t.desc.clone().into(),
                game: t.game.clone().into(),
                price: t.price.clone().into(),
                peer: t.peer.clone().into(),
                peer_id: t.peer_id.clone().into(),
                date: t.date.clone().into(),
                letter: letter(if t.peer.is_empty() { "#" } else { &t.peer }).into(),
                c1,
                c2,
                has_avatar: av.is_some(),
                avatar: av.unwrap_or_default(),
            });
        }
        self.st(|s| {
            s.set_deals(model(rows));
            s.set_deals_more(more);
        });
    }

    /// Пришла аватарка: если она из списка сделок, обновляем его.
    fn deals_image(&mut self, key: &str) {
        if self.ext.deals_tab != 2 && self.ext.trades.iter().any(|t| !t.peer_avatar.is_empty() && imgcache::key(AV, &t.peer_avatar) == key) {
            self.rebuild_deals();
        }
    }
}

// ---------------------------------------------------------------- описания профилей

impl Ui {
    /// Подгрузить описание пользователя с сервера FunPay Tools (с кэшем на 10 минут).
    pub fn load_desc(&mut self, uid: String) {
        if uid.is_empty() || uid == "0" {
            return;
        }
        if let Some((at, d)) = self.ext.descs.get(&uid) {
            if at.elapsed() < std::time::Duration::from_secs(600) {
                let d = d.clone();
                self.desc_loaded(uid, d);
                return;
            }
        }
        std::thread::spawn(move || {
            if let Ok(d) = crate::desc::get(&uid) {
                post(move |u| u.desc_loaded(uid, d));
            }
        });
    }

    pub fn desc_loaded(&mut self, uid: String, d: crate::desc::Desc) {
        self.ext.descs.insert(uid.clone(), (std::time::Instant::now(), d.clone()));
        let pf = self.st(|s| s.get_pf_id().to_string()).unwrap_or_default();
        if pf == uid {
            self.ext.banner_url = d.banner.clone();
            let banner = if d.banner.is_empty() { None } else { self.img_opt(&d.banner, THUMB) };
            self.st(|s| {
                s.set_pf_desc(d.text.clone().into());
                s.set_pf_has_banner(banner.is_some());
                s.set_pf_banner(banner.unwrap_or_default());
            });
        }
        if self.ext.card_uid == uid {
            self.st(|s| s.set_card_desc(d.text.clone().into()));
        }
    }

    fn desc_image(&mut self, key: &str, img: &Image) {
        if !self.ext.banner_url.is_empty() && imgcache::key(THUMB, &self.ext.banner_url) == key {
            let img = img.clone();
            self.st(|s| {
                s.set_pf_banner(img);
                s.set_pf_has_banner(true);
            });
        }
    }

    pub fn desc_edit(&mut self) {
        let cur = self.st(|s| s.get_pf_desc().to_string()).unwrap_or_default();
        self.st(|s| {
            s.set_desc_draft(cur.into());
            s.set_desc_status("".into());
            s.set_desc_busy(false);
            s.set_desc_open(true);
        });
    }

    pub fn desc_save(&mut self, text: String) {
        let text = text.trim().to_string();
        self.st(|s| {
            s.set_desc_busy(true);
            s.set_desc_status("".into());
            s.set_desc_draft(text.clone().into());
        });
        self.send(Cmd::SaveDesc { text });
    }

    pub fn desc_progress(&mut self, s: &str) {
        let s = s.to_string();
        self.st(|st| st.set_desc_status(s.into()));
    }

    pub fn desc_done(&mut self, res: Result<String, String>) {
        self.st(|s| s.set_desc_busy(false));
        match res {
            Ok(text) => {
                let uid = self.me.id.clone();
                let banner = self.ext.descs.get(&uid).map(|x| x.1.banner.clone()).unwrap_or_default();
                self.ext.descs.insert(uid, (std::time::Instant::now(), crate::desc::Desc { text: text.clone(), banner }));
                self.st(|s| {
                    s.set_desc_open(false);
                    if s.get_pf_is_me() {
                        s.set_pf_desc(text.into());
                    }
                });
                self.toast("Описание сохранено");
            }
            Err(e) => self.st(|s| s.set_desc_status(e.into())).unwrap_or(()),
        }
    }
}

// ---------------------------------------------------------------- тема из Telegram

fn color_of(hex: &str) -> Option<Color> {
    let t = crate::tgtheme::Rgba::parse_hex(hex)?;
    Some(Color::from_argb_u8(t.3, t.0, t.1, t.2))
}

impl Ui {
    /// Выставляет цвета и обои импортированной темы.
    pub fn apply_tg(&mut self, t: &crate::config::TgTheme) {
        let Some(app) = self.app() else { return };
        let th = app.global::<Theme>();
        let c = |k: &str| t.colors.get(k).and_then(|h| color_of(h));
        macro_rules! set {
            ($key:literal, $f:ident) => {
                if let Some(v) = c($key) {
                    th.$f(v);
                }
            };
        }
        set!("accent", set_c_accent);
        set!("accent-soft", set_c_accent_soft);
        set!("panel", set_c_panel);
        set!("field", set_c_field);
        set!("line", set_c_line);
        set!("ink", set_c_ink);
        set!("muted", set_c_muted);
        set!("faint", set_c_faint);
        set!("sel", set_c_sel);
        set!("hover", set_c_hover);
        set!("rail", set_c_rail);
        set!("rail-tx", set_c_rail_tx);
        set!("rail-on", set_c_rail_on);
        set!("bubble-in", set_c_bubble_in);
        set!("bubble-out", set_c_bubble_out);
        set!("text-in", set_c_text_in);
        set!("text-out", set_c_text_out);
        set!("on-accent", set_c_on_accent);
        set!("on-sel", set_c_on_sel);
        set!("meta-in", set_c_meta_in);
        set!("meta-out", set_c_meta_out);
        set!("check", set_c_check);
        set!("pill", set_c_pill);
        set!("mbadge", set_c_mbadge);
        set!("danger", set_c_danger);
        set!("ok", set_c_ok);
        set!("title", set_c_title);
        set!("kb", set_c_kb);
        set!("shadow", set_c_shadow);
        set!("wall1", set_c_wall1);
        set!("wall2", set_c_wall2);
        self.apply_wall(t);
        th.set_dark(t.dark);
        th.set_custom(true);
        self.ext.tg_applied = true;
        let name = t.name.clone();
        self.st(|s| s.set_tg_theme(name.into()));
        self.rebuild_msgs();
    }

    /// Окно выбора файла темы и применение.
    pub fn import_theme(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Файл темы")
            .add_filter("Темы", &["fptheme", "tdesktop-theme", "tdesktop-palette", "attheme"])
            .add_filter("Все файлы", &["*"])
            .pick_file()
        else {
            return;
        };
        self.import_theme_path(&path);
    }

    pub fn import_theme_path(&mut self, path: &std::path::Path) {
        let loaded = match crate::tgtheme::load(path) {
            Ok(l) => l,
            Err(e) => {
                self.toast(&e);
                return;
            }
        };
        self.apply_loaded(loaded);
    }

    pub fn reset_theme(&mut self) {
        {
            let mut c = self.cfg.lock().unwrap();
            c.tg_theme = None;
            c.save();
        }
        self.apply_theme();
    }
}

/// Подпись вкладки: заголовок страницы или адрес без https://.
fn tab_title_text(t: &Tab) -> String {
    let title = t.title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    let u = t.url.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
    if u.is_empty() { "Загрузка…".into() } else { u.to_string() }
}

impl Ui {
    /// Вкладки для демо-скриншотов (на Linux нет WebView2).
    pub fn demo_tabs(&mut self, collapsed: bool) {
        self.ext.tabs = vec![
            Tab { id: 1, title: "User Kr1smo / FunPay".into(), url: "https://funpay.com/users/7610778/".into(), loading: false },
            Tab { id: 2, title: "Steam-ключ Hollow Knight / FunPay".into(), url: "https://funpay.com/lots/offer?id=1".into(), loading: true },
        ];
        self.ext.tab_next = 2;
        self.ext.tab_active = 1;
        self.rebuild_tabs();
        self.st(|s| {
            s.set_mini_open(true);
            s.set_mini_collapsed(collapsed);
        });
    }
}

// ---------------------------------------------------------------- несколько аккаунтов

impl Ui {
    /// Сохраняет текущий аккаунт (ключ, куки, имя, аватарку) в список.
    pub fn snapshot_account(&mut self) {
        let me = self.me.clone();
        if me.id.is_empty() || me.id == "0" {
            return;
        }
        let mut c = self.cfg.lock().unwrap();
        if c.golden_key.is_empty() {
            return;
        }
        let cookies = if c.cookies_for == c.golden_key || c.cookies_for.is_empty() { c.cookies.clone() } else { Default::default() };
        let acc = crate::config::SavedAccount { id: me.id.clone(), name: me.name.clone(), avatar: me.avatar.clone(), golden_key: c.golden_key.clone(), cookies };
        match c.accounts.iter_mut().find(|a| a.id == me.id) {
            Some(a) => *a = acc,
            None => c.accounts.push(acc),
        }
        c.save();
    }

    pub fn rebuild_accounts(&mut self) {
        let list = self.cfg.lock().unwrap().accounts.clone();
        let my = self.me.id.clone();
        let mut rows = Vec::new();
        for a in &list {
            let av = if a.avatar.is_empty() { None } else { self.img_opt(&a.avatar, AV) };
            let (c1, c2) = pair(&a.name);
            let _ = (c1, c2);
            rows.push(AccountRow {
                id: a.id.clone().into(),
                name: a.name.clone().into(),
                letter: letter(&a.name).into(),
                has_avatar: av.is_some(),
                avatar: av.unwrap_or_default(),
                active: a.id == my,
            });
        }
        self.st(|s| s.set_accounts(model(rows)));
    }

    fn accounts_image(&mut self, key: &str) {
        let hit = self.cfg.lock().unwrap().accounts.iter().any(|a| !a.avatar.is_empty() && imgcache::key(AV, &a.avatar) == key);
        if hit {
            self.rebuild_accounts();
        }
    }

    /// Сбрасывает всё, что относится к прежнему аккаунту: чаты, экраны, вкладки браузера.
    pub fn reset_account_view(&mut self) {
        self.chats.clear();
        self.cur = None;
        self.pending.clear();
        self.panes.clear();
        self.profile = None;
        self.order = None;
        self.reply = None;
        self.ext.trades.clear();
        self.ext.txns.clear();
        self.ext.descs.clear();
        self.chats_model.set_vec(vec![]);
        self.msgs_model.set_vec(vec![]);
        self.mini_close_all();
        crate::browser::clear_cookies();
        self.st(|s| {
            s.set_chat_open(false);
            s.set_pane(0);
            s.set_card_open(false);
            s.set_settings_open(false);
            s.set_chats_loading(true);
            s.set_stickers_open(false);
        });
    }

    pub fn switch_account(&mut self, id: String) {
        if id == self.me.id {
            self.st(|s| s.set_settings_open(false));
            return;
        }
        self.snapshot_account();
        let Some(acc) = self.cfg.lock().unwrap().accounts.iter().find(|a| a.id == id).cloned() else { return };
        {
            let mut c = self.cfg.lock().unwrap();
            c.golden_key = acc.golden_key.clone();
            c.cookies = acc.cookies.clone();
            c.cookies_for = acc.golden_key.clone();
            c.save();
        }
        self.reset_account_view();
        self.me = Me { id: acc.id.clone(), name: acc.name.clone(), avatar: acc.avatar.clone(), balance: String::new() };
        self.update_me();
        self.rebuild_accounts();
        self.toast(&format!("Аккаунт {}", acc.name));
        self.send(Cmd::UseAccount { gk: acc.golden_key, cookies: acc.cookies });
    }

    pub fn add_account(&mut self) {
        self.snapshot_account();
        self.ext.adding = true;
        self.ext.add_tried = false;
        self.ext.add_prev = self.me.id.clone();
        self.st(|s| {
            s.set_adding_account(true);
            s.set_settings_open(false);
            s.set_logged_in(false);
            s.set_login_page(0);
            s.set_login_error("".into());
            s.set_login_busy(false);
        });
    }

    pub fn cancel_add_account(&mut self) {
        self.ext.adding = false;
        self.st(|s| {
            s.set_adding_account(false);
            s.set_browser_login(false);
            s.set_login_error("".into());
        });
        crate::browser::close_tab(0);
        let prev = self.ext.add_prev.clone();
        let acc = self.cfg.lock().unwrap().accounts.iter().find(|a| a.id == prev).cloned();
        if !self.ext.add_tried {
            self.st(|s| s.set_logged_in(true));
            return;
        }
        // была попытка входа: фоновый поток уже отпустил прежний аккаунт, входим в него снова
        if let Some(acc) = acc {
            {
                let mut c = self.cfg.lock().unwrap();
                c.golden_key = acc.golden_key.clone();
                c.cookies = acc.cookies.clone();
                c.cookies_for = acc.golden_key.clone();
                c.save();
            }
            self.me = Me::default();
            self.send(Cmd::UseAccount { gk: acc.golden_key, cookies: acc.cookies });
        }
    }

    pub fn remove_account(&mut self, id: String) {
        if id == self.me.id {
            self.logout();
            return;
        }
        {
            let mut c = self.cfg.lock().unwrap();
            c.accounts.retain(|a| a.id != id);
            c.save();
        }
        crate::cache::forget(&id);
        self.rebuild_accounts();
        self.toast("Аккаунт убран с этого компьютера");
    }

    /// Выход из текущего аккаунта: если есть другие, переключаемся на следующий.
    pub fn logout(&mut self) {
        let my = self.me.id.clone();
        let next = {
            let mut c = self.cfg.lock().unwrap();
            c.accounts.retain(|a| a.id != my);
            c.save();
            c.accounts.first().cloned()
        };
        crate::browser::clear_cookies();
        match next {
            Some(acc) => {
                self.me = Me::default();
                self.switch_account(acc.id);
            }
            None => {
                self.me = Me::default();
                self.rebuild_accounts();
                self.send(Cmd::Logout);
            }
        }
    }
}

impl Ui {
    /// Аватарка собеседника заказа, найденная в его профиле.
    pub fn order_avatar(&mut self, gen: u64, url: String) {
        if gen != self.od_gen || url.is_empty() || parse::is_default_avatar(&url) {
            return;
        }
        if let Some(o) = self.order.as_mut() {
            o.peer_avatar = url.clone();
        }
        if let Some(i) = self.img_opt(&url, AV) {
            self.st(|s| {
                s.set_od_peer_avatar(i);
                s.set_od_peer_has_avatar(true);
            });
        }
    }
}
