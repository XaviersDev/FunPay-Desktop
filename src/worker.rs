//! Фоновый поток: вся сеть FunPay и общего чата. С UI общается через ui::post.

use crate::config::Config;
use crate::fp::{self, Fp};
use crate::gchat::{GChat, GMsg};
use crate::parse::{AuthorCtx, ChatItem, Message};
use crate::ui::{post, Me, OpenedChat};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub enum Cmd {
    Login(String),
    Logout,
    Open { id: String, kind: i32, gen: u64 },
    Close,
    LoadOlder,
    SendText { local: u64, text: String },
    SendImage { local: u64, path: PathBuf },
    UploadAvatar(Vec<u8>),
    Refresh,
    GChatLogin,
    Focus(bool),
    /// Вход через встроенный браузер: все куки funpay.com
    LoginCookies(Vec<(String, String)>),
    LoadProfile { uid: String, gen: u64, card: bool },
    MoreReviews { uid: String, cont: String, gen: u64 },
    LoadOrder { id: String, gen: u64 },
    OrderAction { id: String, action: String, text: String, rating: i32, gen: u64 },
    MarkRead(String),
    /// Продажи (tab 0), покупки (1) или финансы (2); state: фильтр статуса для 0/1
    LoadTrade { tab: i32, state: String, cont: String, gen: u64 },
    /// Сохранить своё описание (при необходимости подтвердить аккаунт временным лотом)
    SaveDesc { text: String },
    /// После подтверждения: удалить временный лот и сохранить описание
    DescCleanup { code: String, session: Option<String>, text: String },
    /// Аватарка собеседника для экрана заказа (из его профиля)
    OrderAvatar { uid: String, gen: u64 },
    /// Переключиться на сохранённый аккаунт
    UseAccount { gk: String, cookies: std::collections::BTreeMap<String, String> },
    /// Сколько в заморозке: финансы и закрытые продажи за 48 часов
    LoadFrozen,
}

struct Open {
    gen: u64,
    id: String,
    kind: i32,
    hist_node: String,
    last_id: u64,
    first_id: u64,
    has_older: bool,
    peer_id: String,
    ctx: AuthorCtx,
    page_at: Instant,
}

pub struct Worker {
    cfg: Arc<Mutex<Config>>,
    fp: Option<Fp>,
    open: Option<Open>,
    known: HashMap<String, u64>,
    first_poll: bool,
    last_poll: Instant,
    fails: u32,
    focused: bool,
    force_poll: bool,
    gchat: GChat,
    g_active: bool,
    g_disabled: String,
    g_since: i64,
    g_seen: HashSet<String>,
    g_last: Instant,
    g_wait: Option<(String, Instant, Instant)>,
    last_image_at: Option<Instant>,
}

fn is_fp_chat(kind: i32) -> bool {
    kind != 3
}

impl Worker {
    fn new(cfg: Arc<Mutex<Config>>) -> Self {
        Self {
            cfg,
            fp: None,
            open: None,
            known: HashMap::new(),
            first_poll: true,
            last_poll: Instant::now() - Duration::from_secs(60),
            fails: 0,
            focused: true,
            force_poll: false,
            gchat: GChat::new(),
            g_active: true,
            g_disabled: String::new(),
            g_since: 0,
            g_seen: HashSet::new(),
            g_last: Instant::now() - Duration::from_secs(60),
            g_wait: None,
            last_image_at: None,
        }
    }

    fn save_cookies(&mut self) {
        if let Some(fp) = self.fp.as_mut() {
            if fp.cookies_dirty {
                fp.cookies_dirty = false;
                let mut c = self.cfg.lock().unwrap();
                c.cookies = fp.cookies.clone();
                c.cookies_for = fp.gk.clone();
                c.save();
            }
        }
    }

    fn me(&mut self) -> Me {
        let fp = self.fp.as_mut().unwrap();
        let mut avatar = fp.app.avatar.clone();
        if crate::parse::is_default_avatar(&avatar) {
            let uid = fp.app.user_id.clone();
            if let Some(a) = fp.profile_avatar(&uid) {
                avatar = a;
            }
        }
        Me { id: fp.app.user_id.clone(), name: fp.app.username.clone(), avatar, balance: fp.app.balance.clone() }
    }

    fn startup(&mut self) {
        let (gk, cookies, for_gk) = {
            let c = self.cfg.lock().unwrap();
            (c.golden_key.clone(), c.cookies.clone(), c.cookies_for.clone())
        };
        // конфиг общего чата
        let gc = self.gchat.config();
        self.g_active = gc.active;
        self.g_disabled = if gc.active { String::new() } else { gc.disabled_message.clone() };
        let display = gc.display;
        post(move |u| u.set_gchat_visible(display));
        if gk.is_empty() {
            post(|u| u.show_login(String::new()));
            return;
        }
        let cookies = if for_gk == gk { cookies } else { Default::default() };
        self.fp = Some(Fp::new(&gk, cookies));
        self.auth_and_announce(false);
    }

    fn auth_and_announce(&mut self, from_login: bool) {
        let res = self.fp.as_mut().unwrap().ensure_auth(true);
        self.save_cookies();
        match res {
            Ok(()) => {
                let me = self.me();
                self.save_cookies();
                if from_login {
                    let mut c = self.cfg.lock().unwrap();
                    c.golden_key = self.fp.as_ref().unwrap().gk.clone();
                    c.save();
                }
                self.first_poll = true;
                self.force_poll = true;
                post(move |u| u.logged_in(me));
            }
            Err(e) => {
                let fatal = e.contains("golden_key") || e.contains("заблокирован") || from_login;
                if fatal {
                    self.fp = None;
                    if !from_login {
                        let mut c = self.cfg.lock().unwrap();
                        c.golden_key.clear();
                        c.cookies.clear();
                        c.save();
                    }
                    post(move |u| u.show_login(e));
                } else {
                    // нет сети: показываем интерфейс и пробуем дальше
                    post(move |u| u.net_error(e));
                }
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Login(gk) => {
                let gk = gk.trim().trim_start_matches("golden_key=").trim().to_string();
                post(|u| u.login_busy(true));
                self.fp = Some(Fp::new(&gk, Default::default()));
                self.auth_and_announce(true);
            }
            Cmd::OrderAvatar { uid, gen } => {
                let Some(fp) = self.fp.as_mut() else { return };
                if let Some(url) = fp.profile_avatar(&uid) {
                    post(move |u| u.order_avatar(gen, url));
                }
            }
            Cmd::UseAccount { gk, cookies } => {
                self.fp = None;
                self.open = None;
                self.known.clear();
                post(|u| u.login_busy(true));
                self.fp = Some(Fp::new(&gk, cookies));
                self.auth_and_announce(true);
            }
            Cmd::LoginCookies(list) => {
                let gk = list.iter().find(|(k, _)| k == "golden_key").map(|(_, v)| v.clone()).unwrap_or_default();
                if gk.is_empty() {
                    return;
                }
                post(|u| u.login_busy(true));
                let cookies = list.into_iter().filter(|(k, _)| k != "golden_key").collect();
                self.fp = Some(Fp::new(&gk, cookies));
                self.auth_and_announce(true);
            }
            Cmd::LoadProfile { uid, gen, card } => {
                let Some(fp) = self.fp.as_mut() else { return };
                let res = fp.profile(&uid);
                self.save_cookies();
                post(move |u| u.profile_loaded(gen, card, res));
            }
            Cmd::MoreReviews { uid, cont, gen } => {
                let Some(fp) = self.fp.as_mut() else { return };
                let res = fp.more_reviews(&uid, &cont);
                post(move |u| u.reviews_loaded(gen, res));
            }
            Cmd::LoadTrade { tab, state, cont, gen } => {
                let Some(fp) = self.fp.as_mut() else { return };
                let append = !cont.is_empty();
                if tab == 2 {
                    let res = fp.transactions_page(&cont);
                    self.save_cookies();
                    post(move |u| u.txns_loaded(gen, append, res));
                } else {
                    let res = fp.trade_page(tab == 0, &state, &cont);
                    self.save_cookies();
                    post(move |u| u.trade_loaded(gen, append, res));
                }
            }
            Cmd::LoadFrozen => {
                let Some(fp) = self.fp.as_mut() else { return };
                let now = crate::fp::now_msk();
                let mut txns = Vec::new();
                let mut cont = String::new();
                // две страницы финансов хватает на двое суток почти всегда
                for _ in 0..2 {
                    match fp.transactions_page(&cont) {
                        Ok((v, next)) => {
                            txns.extend(v);
                            cont = next;
                            if cont.is_empty() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let sales = fp.trade_page(true, "closed", "").map(|x| x.0).unwrap_or_default();
                self.save_cookies();
                let a = crate::money::frozen_from_txns(&txns, now);
                let b = crate::money::frozen_from_sales(&sales, now);
                crate::log::write(&format!("заморозка: финансы {} строк {:?}, продажи {} строк {:?}", txns.len(), a, sales.len(), b));
                let text = crate::money::frozen_text(&crate::money::frozen_merge(a, b));
                post(move |u| u.frozen_loaded(text));
            }
            Cmd::SaveDesc { text } => self.save_desc(text),
            Cmd::DescCleanup { code, session, text } => self.desc_cleanup(code, session, text),
            Cmd::LoadOrder { id, gen } => {
                let Some(fp) = self.fp.as_mut() else { return };
                let res = fp.order(&id);
                self.save_cookies();
                post(move |u| u.order_loaded(gen, res));
            }
            Cmd::OrderAction { id, action, text, rating, gen } => {
                let Some(fp) = self.fp.as_mut() else { return };
                let res = match action.as_str() {
                    "refund" => fp.refund(&id).map(|_| "Деньги возвращены покупателю"),
                    "confirm" => fp.confirm_order(&id).map(|_| "Выполнение заказа подтверждено"),
                    "review" => fp.review(&id, &text, rating).map(|_| "Отзыв сохранён"),
                    "review-reply" => fp.review(&id, &text, 5).map(|_| "Ответ на отзыв отправлен"),
                    "review-delete" => fp.review_delete(&id).map(|_| "Удалено"),
                    _ => Err("Неизвестное действие".into()),
                };
                let reload = fp.order(&id);
                self.save_cookies();
                post(move |u| {
                    match res {
                        Ok(t) => u.toast(t),
                        Err(e) => u.toast(&e),
                    }
                    u.order_loaded(gen, reload);
                });
            }
            Cmd::MarkRead(id) => {
                if let Some(fp) = self.fp.as_mut() {
                    let _ = fp.mark_read(&id);
                }
                self.force_poll = true;
            }
            Cmd::Logout => {
                self.fp = None;
                self.open = None;
                self.known.clear();
                let mut c = self.cfg.lock().unwrap();
                c.golden_key.clear();
                c.cookies.clear();
                c.save();
                drop(c);
                post(|u| u.show_login(String::new()));
            }
            Cmd::Open { id, kind, gen } => self.open_chat(id, kind, gen),
            Cmd::Close => self.open = None,
            Cmd::LoadOlder => self.load_older(),
            Cmd::SendText { local, text } => self.send_text(local, text),
            Cmd::SendImage { local, path } => self.send_image(local, path),
            Cmd::UploadAvatar(png) => self.upload_avatar(png),
            Cmd::Refresh => {
                if let Some(o) = &self.open {
                    let (id, kind, gen) = (o.id.clone(), o.kind, o.gen);
                    self.open_chat(id, kind, gen);
                }
                self.force_poll = true;
            }
            Cmd::GChatLogin => match self.gchat.start() {
                Ok(code) => {
                    crate::sys::open_external(&GChat::bot_link(&code));
                    self.g_wait = Some((code, Instant::now(), Instant::now()));
                    post(|u| u.gchat_status("Откройте бота, нажмите «Старт» и вернитесь сюда. Жду подтверждения…".into()));
                }
                Err(e) => post(move |u| u.gchat_status(e)),
            },
            Cmd::Focus(f) => {
                self.focused = f;
                if f {
                    self.force_poll = true;
                }
            }
        }
    }

    fn open_chat(&mut self, id: String, kind: i32, gen: u64) {
        if kind == 3 {
            self.open = Some(Open {
                gen,
                id: id.clone(),
                kind,
                hist_node: String::new(),
                last_id: 0,
                first_id: 0,
                has_older: false,
                peer_id: String::new(),
                ctx: AuthorCtx::default(),
                page_at: Instant::now(),
            });
            self.g_since = 0;
            self.g_seen.clear();
            let token = self.cfg.lock().unwrap().gchat_token.clone();
            let disabled = self.g_disabled.clone();
            post(move |u| u.gchat_gate(token.is_empty(), disabled));
            self.gchat_tick(true);
            return;
        }
        let Some(fp) = self.fp.as_mut() else { return };
        let mut ctx = AuthorCtx::default();
        let mut opened = OpenedChat { gen, ..Default::default() };
        let mut hist_node = id.clone();
        let mut has_older = true;
        let res: Result<Vec<Message>, String> = if kind == 0 {
            match fp.chat_page(&id, &mut ctx) {
                Ok(page) => {
                    if !page.node_name.is_empty() {
                        hist_node = page.node_name.clone();
                    }
                    opened.peer_id = page.peer_id.clone();
                    opened.peer_name = page.peer_name.clone();
                    opened.peer_status = page.peer_status.clone();
                    opened.peer_avatar = page.peer_avatar.clone();
                    opened.lot_name = page.lot_name.clone();
                    opened.lot_url = page.lot_url.clone();
                    let mut msgs = page.messages;
                    if msgs.is_empty() {
                        if let Ok((m, more, _)) = fp.history(&hist_node, None, &mut ctx) {
                            msgs = m;
                            has_older = more;
                        }
                    }
                    Ok(msgs)
                }
                Err(e) => Err(e),
            }
        } else {
            fp.history(&id, None, &mut ctx).map(|(m, more, name)| {
                hist_node = name;
                has_older = more;
                m
            })
        };
        self.save_cookies();
        match res {
            Ok(msgs) => {
                self.note_fpt(&msgs);
                let last_id = msgs.last().map(|m| m.id).unwrap_or(0);
                let first_id = msgs.first().map(|m| m.id).unwrap_or(0);
                if msgs.is_empty() {
                    has_older = false;
                }
                self.open = Some(Open {
                    gen,
                    id,
                    kind,
                    hist_node,
                    last_id,
                    first_id,
                    has_older,
                    peer_id: opened.peer_id.clone(),
                    ctx,
                    page_at: Instant::now(),
                });
                opened.msgs = msgs;
                opened.has_older = has_older;
                post(move |u| u.chat_opened(opened));
            }
            Err(e) => {
                post(move |u| u.chat_failed(gen, e));
            }
        }
    }

    fn note_fpt(&mut self, msgs: &[Message]) {
        let my = self.fp.as_ref().map(|f| f.app.user_id.clone()).unwrap_or_default();
        let mut added = Vec::new();
        {
            let mut c = self.cfg.lock().unwrap();
            for m in msgs {
                if m.fpt && m.author_id != my && m.author_id.chars().all(|ch| ch.is_ascii_digit()) && !m.author_id.is_empty() {
                    if c.fpt_users.insert(m.author_id.clone()) {
                        added.push(m.author_id.clone());
                    }
                }
            }
            if !added.is_empty() {
                c.save();
            }
        }
        if !added.is_empty() {
            post(move |u| u.fpt_users_added(added));
        }
    }

    fn load_older(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        if !o.has_older || !is_fp_chat(o.kind) || o.first_id == 0 {
            let gen = o.gen;
            post(move |u| u.older_loaded(gen, vec![], false));
            return;
        }
        let Some(fp) = self.fp.as_mut() else { return };
        let gen = o.gen;
        let mut ctx = AuthorCtx { names: o.ctx.names.clone(), ..Default::default() };
        match fp.history(&o.hist_node.clone(), Some(o.first_id), &mut ctx) {
            Ok((mut msgs, more, _)) => {
                msgs.retain(|m| m.id < o.first_id);
                if let Some(f) = msgs.first() {
                    o.first_id = f.id;
                }
                o.ctx.names.extend(ctx.names);
                o.has_older = more && !msgs.is_empty();
                let has = o.has_older;
                self.note_fpt(&msgs);
                post(move |u| u.older_loaded(gen, msgs, has));
            }
            Err(_) => post(move |u| u.older_loaded(gen, vec![], true)),
        }
    }

    fn send_text(&mut self, local: u64, text: String) {
        let Some(o) = self.open.as_ref() else { return };
        let (kind, node) = (o.kind, o.id.clone());
        if kind == 3 {
            let (token, name, avatar, url) = {
                let c = self.cfg.lock().unwrap();
                let fp = self.fp.as_ref();
                (
                    c.gchat_token.clone(),
                    fp.map(|f| f.app.username.clone()).unwrap_or_else(|| "FunPay user".into()),
                    fp.map(|f| f.app.avatar.clone()).unwrap_or_default(),
                    fp.map(|f| format!("https://funpay.com/users/{}/", f.app.user_id)).unwrap_or_default(),
                )
            };
            if text.chars().count() > crate::gchat::MAX_LEN {
                post(move |u| u.send_result(local, Err(format!("Слишком длинно: максимум {} символов", crate::gchat::MAX_LEN))));
                return;
            }
            let res = self.gchat.send(&token, &name, &avatar, &url, &text);
            let res = match res {
                Ok(()) => Ok(()),
                Err((e, expired)) => {
                    if expired {
                        let mut c = self.cfg.lock().unwrap();
                        c.gchat_token.clear();
                        c.save();
                        post(|u| u.gchat_gate(true, String::new()));
                    }
                    Err(e)
                }
            };
            post(move |u| u.send_result(local, res));
            self.gchat_tick(true);
            return;
        }
        let Some(fp) = self.fp.as_mut() else { return };
        let mark = self.cfg.lock().unwrap().fpt_mark;
        let signed = crate::fpt::sign(&text, mark);
        let res = fp.send_message(&node, &signed, None);
        self.save_cookies();
        if res.is_ok() {
            let mut c = self.cfg.lock().unwrap();
            c.last_outgoing.insert(node.clone(), text.trim().to_string());
            c.save();
        }
        post(move |u| u.send_result(local, res));
        self.force_poll = true;
        self.tick();
    }

    fn send_image(&mut self, local: u64, path: PathBuf) {
        let Some(o) = self.open.as_ref() else { return };
        if o.kind == 3 {
            post(move |u| u.send_result(local, Err("В чат FunPay Tools можно отправлять только текст".into())));
            return;
        }
        let node = o.id.clone();
        let Some(fp) = self.fp.as_mut() else { return };
        // FunPay не любит пачку картинок подряд (так же делает мобильное приложение)
        if let Some(t) = self.last_image_at {
            let gap = Duration::from_millis(700);
            let el = t.elapsed();
            if el < gap {
                std::thread::sleep(gap - el);
            }
        }
        let res = (|| -> Result<(), String> {
            let (bytes, name, mime) = prepare_image(&path)?;
            let id = fp.upload_chat_image(bytes, &name, &mime)?;
            fp.send_message(&node, "", Some(&id))
        })();
        self.last_image_at = Some(std::time::Instant::now());
        if let Err(e) = &res {
            crate::log::write(&format!("картинка {:?} в {node}: {e}", path.file_name().unwrap_or_default()));
        }
        self.save_cookies();
        if res.is_ok() {
            let mut c = self.cfg.lock().unwrap();
            c.last_outgoing.insert(node.clone(), "__image__".into());
            c.save();
        }
        post(move |u| u.send_result(local, res));
        self.force_poll = true;
        self.tick();
    }

    fn save_desc(&mut self, text: String) {
        let Some(fp) = self.fp.as_mut() else { return };
        if fp.ensure_auth(false).is_err() {
            post(|u| u.desc_done(Err("Нет связи с FunPay".into())));
            return;
        }
        let uid = fp.user_id();
        let sess = {
            let c = self.cfg.lock().unwrap();
            if c.desc_session.0 == uid { c.desc_session.1.clone() } else { String::new() }
        };
        if !sess.is_empty() {
            let cfg = self.cfg.clone();
            std::thread::spawn(move || match crate::desc::put(&sess, &text) {
                Ok(()) => post(move |u| u.desc_done(Ok(text))),
                Err(e) if e == "auth" => {
                    // сессия протухла: забываем и подтверждаем заново
                    {
                        let mut c = cfg.lock().unwrap();
                        c.desc_session = (String::new(), String::new());
                        c.save();
                    }
                    post(move |u| u.send(Cmd::SaveDesc { text }));
                }
                Err(e) => post(move |u| u.desc_done(Err(e))),
            });
            return;
        }
        post(|u| u.desc_progress("Подтверждаю, что аккаунт ваш…"));
        let code = match crate::desc::link_start(&uid) {
            Ok(c) => c,
            Err(e) => {
                post(move |u| u.desc_done(Err(e)));
                return;
            }
        };
        post(|u| u.desc_progress("Создаю временный лот с кодом…"));
        let res = fp.create_verify_lot(&code);
        self.save_cookies();
        if let Err(e) = res {
            crate::log::write(&format!("лот подтверждения: {e}"));
            post(move |u| u.desc_done(Err(format!("Не получилось создать лот подтверждения: {e}"))));
            return;
        }
        post(|u| u.desc_progress("Жду, пока сервер увидит лот…"));
        std::thread::spawn(move || {
            let mut session = None;
            for _ in 0..24 {
                std::thread::sleep(Duration::from_millis(2500));
                match crate::desc::link_confirm(&uid) {
                    Ok(Some(s)) => {
                        session = Some(s);
                        break;
                    }
                    Ok(None) => {}
                    Err(e) => crate::log::write(&format!("link/confirm: {e}")),
                }
            }
            post(move |u| u.send(Cmd::DescCleanup { code, session, text }));
        });
    }

    fn desc_cleanup(&mut self, code: String, session: Option<String>, text: String) {
        let Some(fp) = self.fp.as_mut() else { return };
        post(|u| u.desc_progress("Удаляю временный лот…"));
        let uid = fp.user_id();
        let needle = format!("FPT Verify {code}");
        let mut deleted = false;
        for _ in 0..3 {
            if let Some(id) = fp.find_my_offer(&needle) {
                match fp.delete_offer(&id) {
                    Ok(()) => deleted = true,
                    Err(e) => crate::log::write(&format!("удаление лота {id}: {e}")),
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(1500));
        }
        self.save_cookies();
        if !deleted {
            crate::log::write("временный лот подтверждения не найден или не удалён");
        }
        let Some(session) = session else {
            let note = if deleted { "" } else { " Если лот «FPT Verify» остался, удалите его на FunPay." };
            post(move |u| u.desc_done(Err(format!("Сервер не увидел лот подтверждения. Попробуйте ещё раз через минуту.{note}"))));
            return;
        };
        {
            let mut c = self.cfg.lock().unwrap();
            c.desc_session = (uid, session.clone());
            c.save();
        }
        let left = !deleted;
        std::thread::spawn(move || match crate::desc::put(&session, &text) {
            Ok(()) => post(move |u| {
                u.desc_done(Ok(text));
                if left {
                    u.toast("Лот «FPT Verify» не удалился сам, удалите его на FunPay");
                }
            }),
            Err(e) => post(move |u| u.desc_done(Err(if e == "auth" { "Сервер не принял подтверждение".into() } else { e }))),
        });
    }

    fn upload_avatar(&mut self, png: Vec<u8>) {
        let Some(fp) = self.fp.as_mut() else { return };
        let res = fp.upload_avatar(png);
        self.save_cookies();
        match res {
            Ok(()) => {
                let _ = self.fp.as_mut().unwrap().ensure_auth(true);
                let me = self.me();
                post(move |u| u.avatar_uploaded(Ok(me)));
            }
            Err(e) => post(move |u| u.avatar_uploaded(Err(e))),
        }
    }

    fn interval(&self) -> Duration {
        let base = if self.focused { 3 } else { 6 };
        let backoff = if self.fails > 0 { (3u64 << self.fails.min(3)).min(30) } else { base };
        Duration::from_secs(backoff.max(base))
    }

    fn tick(&mut self) {
        if self.fp.is_some() && (self.force_poll || self.last_poll.elapsed() >= self.interval()) {
            self.force_poll = false;
            self.poll();
        }
        self.gchat_tick(false);
    }

    fn poll(&mut self) {
        self.last_poll = Instant::now();
        let open_q = self.open.as_ref().filter(|o| is_fp_chat(o.kind)).map(|o| (o.id.clone(), o.last_id, o.gen));
        let fp = self.fp.as_mut().unwrap();
        let mut ctx = self.open.as_mut().map(|o| std::mem::take(&mut o.ctx)).unwrap_or_default();
        let mut res = fp.poll(open_q.as_ref().map(|(n, l, _)| (n.as_str(), *l)), &mut ctx);
        if res.is_err() && open_q.is_some() {
            // открытый чат мог сломать запрос: список чатов всё равно обновим
            res = fp.poll(None, &mut ctx);
        }
        if let Some(o) = self.open.as_mut() {
            o.ctx = ctx;
        }
        self.save_cookies();
        match res {
            Ok((chats, msgs)) => {
                if self.fails > 0 {
                    post(|u| u.net_error(String::new()));
                }
                self.fails = 0;
                if let Some(chats) = chats {
                    self.notify_new(&chats);
                    post(move |u| u.chats_updated(chats));
                }
                if let (Some(msgs), Some((_, _, gen))) = (msgs, open_q) {
                    if !msgs.is_empty() {
                        if let Some(o) = self.open.as_mut() {
                            if o.gen == gen {
                                o.last_id = msgs.last().unwrap().id.max(o.last_id);
                                if o.first_id == 0 {
                                    o.first_id = msgs.first().unwrap().id;
                                }
                            }
                        }
                        self.note_fpt(&msgs);
                        post(move |u| u.new_messages(gen, msgs));
                    }
                }
                // «Смотрит лот» и статус обновляем со страницы раз в 40 секунд
                let refresh = self.open.as_ref().map(|o| o.kind == 0 && o.page_at.elapsed() > Duration::from_secs(40)).unwrap_or(false);
                if refresh && self.focused {
                    self.refresh_page();
                }
            }
            Err(e) => {
                self.fails += 1;
                if self.fails >= 2 || e.contains("golden_key") || e.contains("заблокирован") || e.contains("428") {
                    post(move |u| u.net_error(e));
                }
            }
        }
    }

    fn refresh_page(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        o.page_at = Instant::now();
        let (id, gen) = (o.id.clone(), o.gen);
        let mut ctx = AuthorCtx::default();
        if let Some(fp) = self.fp.as_mut() {
            if let Ok(p) = fp.chat_page(&id, &mut ctx) {
                let (lot_name, lot_url, status) = (p.lot_name, p.lot_url, p.peer_status);
                post(move |u| u.header_refreshed(gen, status, lot_name, lot_url));
            }
        }
    }

    fn notify_new(&mut self, chats: &[ChatItem]) {
        let (outgoing, muted) = {
            let c = self.cfg.lock().unwrap();
            (c.last_outgoing.clone(), c.muted.clone())
        };
        let open_id = self.open.as_ref().map(|o| o.id.clone());
        let mut fresh = Vec::new();
        for c in chats {
            let prev = self.known.insert(c.id.clone(), c.node_msg);
            if self.first_poll {
                continue;
            }
            let is_new = match prev {
                Some(p) => c.node_msg > p,
                None => true,
            };
            if !is_new {
                continue;
            }
            if let Some(mine) = outgoing.get(&c.id) {
                let p = c.preview.trim_end_matches("...").trim().to_lowercase();
                if !p.is_empty() && mine.to_lowercase().starts_with(&p) {
                    continue;
                }
            }
            if !c.unread || muted.contains(&c.id) {
                continue;
            }
            if self.focused && open_id.as_deref() == Some(c.id.as_str()) {
                continue;
            }
            fresh.push(c.clone());
        }
        self.first_poll = false;
        if !fresh.is_empty() {
            post(move |u| u.incoming(fresh));
        }
    }

    fn gchat_tick(&mut self, force: bool) {
        // ожидание входа через бота
        if let Some((code, started, last)) = self.g_wait.clone() {
            if started.elapsed() > Duration::from_secs(400) {
                self.g_wait = None;
                post(|u| u.gchat_status("Время вышло. Нажмите «Войти» ещё раз.".into()));
            } else if last.elapsed() >= Duration::from_millis(2500) {
                self.g_wait = Some((code.clone(), started, Instant::now()));
                if let Some(token) = self.gchat.poll(&code) {
                    self.g_wait = None;
                    let mut c = self.cfg.lock().unwrap();
                    c.gchat_token = token;
                    c.save();
                    drop(c);
                    post(|u| {
                        u.gchat_gate(false, String::new());
                        u.toast("Вход в чат FunPay Tools подтверждён");
                    });
                }
            }
        }
        let Some(o) = self.open.as_ref() else { return };
        if o.kind != 3 || !self.g_active {
            return;
        }
        if !force && self.g_last.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.g_last = Instant::now();
        let gen = o.gen;
        match self.gchat.fetch(self.g_since) {
            Ok(list) => {
                let fresh: Vec<GMsg> = list.into_iter().filter(|m| self.g_seen.insert(m.id.clone())).collect();
                if let Some(mx) = fresh.iter().map(|m| m.ts).max() {
                    self.g_since = self.g_since.max(mx);
                }
                let initial = self.open.as_ref().map(|o| o.last_id == 0).unwrap_or(false);
                if let Some(o) = self.open.as_mut() {
                    o.last_id = 1;
                }
                if !fresh.is_empty() || initial {
                    let (my_id, my_name) = self.fp.as_ref().map(|f| (f.app.user_id.clone(), f.app.username.clone())).unwrap_or_default();
                    let msgs: Vec<Message> = fresh.into_iter().map(|g| gmsg_to_message(g, &my_id, &my_name)).collect();
                    post(move |u| u.gchat_messages(gen, msgs, initial));
                }
            }
            Err(_) => {}
        }
    }
}

pub fn gmsg_to_message(g: GMsg, my_id: &str, my_name: &str) -> Message {
    let uid = once_cell::sync::Lazy::force(&USER_RE).captures(&g.url).map(|c| c[1].to_string());
    let mine = (!my_id.is_empty() && uid.as_deref() == Some(my_id)) || (!my_name.is_empty() && g.nick == my_name);
    let ts = chrono::DateTime::from_timestamp_millis(g.ts).map(|d| d.with_timezone(&chrono::Local).naive_local());
    Message {
        id: g.ts.max(0) as u64,
        author_id: if mine { my_id.to_string() } else { uid.unwrap_or_else(|| format!("g:{}", g.nick)) },
        author: g.nick,
        avatar: g.avatar,
        date_raw: String::new(),
        ts,
        has_head: true,
        text: g.text,
        image: None,
        thumb: None,
        badge: if g.pro.is_empty() { String::new() } else { "PRO".into() },
        fpt: true,
    }
}

static USER_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"/users/(\d+)").unwrap());

/// Готовит картинку к загрузке: jpg/png/gif как есть (если не огромные), остальное в PNG.
pub fn prepare_image(path: &PathBuf) -> Result<(Vec<u8>, String, String), String> {
    let bytes = std::fs::read(path).map_err(|_| "Не удалось прочитать файл".to_string())?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image.png").to_string();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        _ => None,
    };
    if let Some(m) = mime {
        if bytes.len() < 7 * 1024 * 1024 {
            return Ok((bytes, name, m.to_string()));
        }
    }
    let img = image::load_from_memory(&bytes).map_err(|_| "Это не картинка или формат не поддерживается".to_string())?;
    let img = if img.width().max(img.height()) > 2560 { img.resize(2560, 2560, image::imageops::FilterType::Lanczos3) } else { img };
    let mut out = Vec::new();
    img.to_rgb8()
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    Ok((out, "image.jpg".into(), "image/jpeg".into()))
}

pub fn run(rx: Receiver<Cmd>, cfg: Arc<Mutex<Config>>) {
    let mut w = Worker::new(cfg);
    w.startup();
    loop {
        match rx.recv_timeout(Duration::from_millis(400)) {
            Ok(cmd) => w.handle(cmd),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // сначала разгребаем очередь команд, чтобы клики не ждали опроса
        while let Ok(cmd) = rx.try_recv() {
            w.handle(cmd);
        }
        w.tick();
    }
}

#[allow(dead_code)]
fn _assert_fp_used() {
    let _ = fp::UA;
}
