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
}

impl Default for PopupCfg {
    fn default() -> Self {
        Self { custom: true, corner: 3, count: 3, show_icon: true, show_name: true, show_text: true, in_focus: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SavedAccount {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub golden_key: String,
    pub cookies: BTreeMap<String, String>,
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
