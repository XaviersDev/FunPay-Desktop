//! Настройки и сессия на диске: %APPDATA%\FunPayTools\desktop.json

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub golden_key: String,
    /// Прочие куки FunPay (PHPSESSID, golden_seal...), привязаны к golden_key.
    pub cookies: BTreeMap<String, String>,
    pub cookies_for: String,
    /// User-Agent встроенного браузера, через который вошли
    #[serde(default)]
    pub user_agent: String,
    /// включённые плагины
    #[serde(default)]
    pub plugins_on: BTreeMap<String, bool>,
    /// виджеты на рабочем столе
    pub widgets: Vec<WidgetCfg>,
    pub widgets_top: bool,
    pub widget_style: WidgetStyle,
    /// статус в Discord
    pub discord: DiscordCfg,
    /// отложенные сообщения
    pub scheduled: Vec<Scheduled>,
    /// режим "Не беспокоить"
    pub dnd: DndCfg,
    /// 0 как в системе, 1 светлая, 2 тёмная
    pub theme: i32,
    pub notifications: bool,
    pub notify_in_focus: bool,
    pub fpt_mark: bool,
    pub fpt_users: BTreeSet<String>,
    pub fpt_dismissed: BTreeSet<String>,
    pub always_on_top: bool,
    pub gchat_token: String,
    pub win_w: f32,
    pub win_h: f32,
    /// Последнее отправленное мной сообщение по чату (чтобы не уведомлять о своих).
    pub last_outgoing: BTreeMap<String, String>,
    pub show_flood: bool,
    pub show_gchat: bool,
    pub recent_games: Vec<String>,
    pub sound: bool,
    /// Закреплённые чаты по порядку
    pub pinned_chats: Vec<String>,
    pub muted: BTreeSet<String>,
    pub archived: BTreeSet<String>,
    pub folders: Vec<UserFolder>,
    /// Закреплённое сообщение по чату
    pub pinned_msgs: BTreeMap<String, PinnedMsg>,
    pub recent_stickers: Vec<String>,
    /// Имя и вид чатов, которые не из списка FunPay (для папок): id → (kind, name)
    pub known_chats: BTreeMap<String, (i32, String)>,
    /// Сессия сервера описаний FunPay Tools: (id на FunPay, токен)
    pub desc_session: (String, String),
    /// Импортированная тема Telegram
    pub tg_theme: Option<TgTheme>,
    /// Все добавленные аккаунты (текущий тоже), ключи и куки только здесь, на этом ПК
    pub accounts: Vec<SavedAccount>,
    /// При закрытии прятаться в трей и продолжать ловить сообщения
    pub keep_background: bool,
    /// Запускать вместе с Windows (в фоне)
    pub autostart: bool,
    /// Подсказка «работаю в фоне» уже показана
    pub bg_hint_shown: bool,
    /// Шаблоны сообщений и их вид
    pub templates: Vec<crate::templates::Template>,
    pub tpl_look: crate::templates::Look,
    /// Клик по шаблону сразу отправляет (иначе вставляет в поле)
    pub tpl_send_now: bool,
    /// Команды автоответа
    pub auto_cmds: Vec<crate::templates::AutoCmd>,
    pub auto_on: bool,
    /// Свои всплывающие уведомления
    pub popup: PopupCfg,
    /// Анимация цветных ников
    pub epic_anim: bool,
    /// Обновления с GitHub
    pub auto_update: bool,
    /// Обучение уже пройдено или пропущено
    pub tour_done: bool,
    /// Выключенные лоты по аккаунтам: FunPay не показывает их на странице профиля
    pub inactive_lots: BTreeMap<String, Vec<crate::lots::MyLot>>,
    /// Своя тема из редактора
    pub custom_theme: crate::tgtheme::FpTheme,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct PopupCfg {
    /// true свои окна, false уведомления Windows
    pub custom: bool,
    /// 0 слева сверху, 1 справа сверху, 2 слева снизу, 3 справа снизу
    pub corner: i32,
    /// сколько показывать одновременно (1..5)
    pub count: i32,
    pub show_icon: bool,
    pub show_name: bool,
    pub show_text: bool,
    /// показывать и когда окно активно
    pub in_focus: bool,
    /// громкость звука, 0..100
    #[serde(default = "vol80")]
    pub volume: i32,
}

fn vol80() -> i32 {
    80
}

impl Default for PopupCfg {
    fn default() -> Self {
        Self { custom: true, corner: 3, count: 3, show_icon: true, show_name: true, show_text: true, in_focus: false, volume: 80 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct WidgetCfg {
    /// 0 баланс, 1 непрочитанные, 2 продажи за сегодня, 3 лоты
    pub kind: i32,
    /// положение в физических пикселях; None, пока виджет не двигали
    #[serde(default)]
    pub pos: Option<(i32, i32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct WidgetStyle {
    /// 0 компактный, 1 обычный, 2 крупный
    pub size: i32,
    /// 0 как в программе, 1 светлая, 2 тёмная
    pub theme: i32,
    /// непрозрачность фона, 0..100
    pub opacity: i32,
}

impl Default for WidgetStyle {
    fn default() -> Self {
        Self { size: 1, theme: 0, opacity: 92 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Scheduled {
    pub id: String,
    /// чат и его название
    pub chat: String,
    pub name: String,
    pub text: String,
    /// когда отправить, unix-секунды
    pub at: i64,
    pub tries: u32,
    pub error: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DndCfg {
    /// включён вручную
    pub on: bool,
    /// по расписанию, часы с from до to
    pub schedule: bool,
    pub from: i32,
    pub to: i32,
    /// автоответ один раз в каждый чат
    pub reply: bool,
    pub reply_text: String,
}

impl Default for DndCfg {
    fn default() -> Self {
        Self {
            on: false,
            schedule: false,
            from: 23,
            to: 9,
            reply: false,
            reply_text: "Здравствуйте! Сейчас я не на связи, отвечу, как только вернусь.".into(),
        }
    }
}

impl DndCfg {
    pub fn active_at(&self, hour: i32) -> bool {
        if self.on {
            return true;
        }
        if !self.schedule || self.from == self.to {
            return false;
        }
        if self.from < self.to {
            hour >= self.from && hour < self.to
        } else {
            hour >= self.from || hour < self.to
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct DiscordCfg {
    pub on: bool,
    pub nick: bool,
    pub sales: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SavedAccount {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub golden_key: String,
    pub cookies: BTreeMap<String, String>,
    /// прокси этого аккаунта (scheme://user:pass@host:port), пусто без прокси
    #[serde(default)]
    pub proxy: String,
    /// без рабочего прокси аккаунт не запускается
    #[serde(default)]
    pub proxy_strict: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TgTheme {
    pub name: String,
    pub dark: bool,
    /// имя цвета приложения → #RRGGBBAA
    pub colors: BTreeMap<String, String>,
    /// путь к картинке обоев (копия в папке приложения)
    pub wall: String,
    pub wall_tiled: bool,
    /// непрозрачность пузырей сообщений (1 сплошные)
    #[serde(default = "one")]
    pub msg_alpha: f32,
    /// матовое стекло под сообщениями
    #[serde(default)]
    pub glass: bool,
    #[serde(default = "ten")]
    pub glass_blur: f32,
    /// для анимированного фона
    #[serde(default)]
    pub wall_blur: f32,
    #[serde(default = "hundred")]
    pub wall_bright: f32,
}

fn one() -> f32 {
    1.0
}
fn ten() -> f32 {
    10.0
}
fn hundred() -> f32 {
    100.0
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct UserFolder {
    pub id: String,
    pub name: String,
    pub chats: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PinnedMsg {
    pub key: String,
    pub author: String,
    pub text: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            golden_key: String::new(),
            cookies: BTreeMap::new(),
            cookies_for: String::new(),
            user_agent: String::new(),
            plugins_on: BTreeMap::new(),
            widgets: Vec::new(),
            widgets_top: false,
            widget_style: WidgetStyle::default(),
            discord: DiscordCfg::default(),
            scheduled: Vec::new(),
            dnd: DndCfg::default(),
            theme: 0,
            notifications: true,
            notify_in_focus: false,
            fpt_mark: true,
            fpt_users: BTreeSet::new(),
            fpt_dismissed: BTreeSet::new(),
            always_on_top: false,
            gchat_token: String::new(),
            win_w: 1180.0,
            win_h: 760.0,
            last_outgoing: BTreeMap::new(),
            show_flood: true,
            show_gchat: true,
            recent_games: Vec::new(),
            sound: true,
            pinned_chats: Vec::new(),
            muted: BTreeSet::new(),
            archived: BTreeSet::new(),
            folders: Vec::new(),
            pinned_msgs: BTreeMap::new(),
            recent_stickers: Vec::new(),
            known_chats: BTreeMap::new(),
            desc_session: (String::new(), String::new()),
            tg_theme: None,
            accounts: Vec::new(),
            keep_background: true,
            autostart: true,
            bg_hint_shown: false,
            templates: Vec::new(),
            tpl_look: Default::default(),
            tpl_send_now: false,
            auto_cmds: Vec::new(),
            auto_on: true,
            popup: Default::default(),
            epic_anim: true,
            auto_update: true,
            tour_done: false,
            inactive_lots: BTreeMap::new(),
            custom_theme: Default::default(),
        }
    }
}

pub fn dir() -> PathBuf {
    let base = directories::BaseDirs::new()
        .map(|b| b.config_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir());
    let d = base.join("FunPayTools");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn path() -> PathBuf {
    dir().join("desktop.json")
}

impl Config {
    pub fn load() -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let tmp = path().with_extension("tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, path());
            }
        }
    }
}
