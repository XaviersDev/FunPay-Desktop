//! Кэш чатов на диске: при запуске список и переписка видны сразу, а свежие данные подгружаются следом.
//! Лежит в %APPDATA%\FunPayTools\chatcache\<id аккаунта>\ (только на этом ПК).

use crate::parse::{ChatItem, Message};
use std::path::PathBuf;

const KEEP_MSGS: usize = 80;

fn dir(uid: &str) -> Option<PathBuf> {
    let uid: String = uid.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if uid.is_empty() {
        return None;
    }
    let d = crate::config::dir().join("chatcache").join(uid);
    let _ = std::fs::create_dir_all(d.join("msgs"));
    Some(d)
}

fn safe(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn write_atomic(p: PathBuf, data: String) {
    std::thread::spawn(move || {
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    });
}

pub fn load_chats(uid: &str) -> Vec<ChatItem> {
    let Some(d) = dir(uid) else { return vec![] };
    std::fs::read_to_string(d.join("chats.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_chats(uid: &str, chats: &[ChatItem]) {
    let Some(d) = dir(uid) else { return };
    if let Ok(s) = serde_json::to_string(chats) {
        write_atomic(d.join("chats.json"), s);
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
pub struct ChatSnap {
    pub peer_id: String,
    pub peer_name: String,
    pub peer_avatar: String,
    pub lot_name: String,
    pub lot_url: String,
    pub msgs: Vec<Message>,
}

pub fn load_msgs(uid: &str, chat: &str) -> Option<ChatSnap> {
    let d = dir(uid)?;
    let s = std::fs::read_to_string(d.join("msgs").join(format!("{}.json", safe(chat)))).ok()?;
    serde_json::from_str(&s).ok()
}

pub fn save_msgs(uid: &str, chat: &str, snap: &ChatSnap) {
    let Some(d) = dir(uid) else { return };
    let start = snap.msgs.len().saturating_sub(KEEP_MSGS);
    let trimmed = ChatSnap {
        peer_id: snap.peer_id.clone(),
        peer_name: snap.peer_name.clone(),
        peer_avatar: snap.peer_avatar.clone(),
        lot_name: snap.lot_name.clone(),
        lot_url: snap.lot_url.clone(),
        msgs: snap.msgs[start..].to_vec(),
    };
    if let Ok(s) = serde_json::to_string(&trimmed) {
        write_atomic(d.join("msgs").join(format!("{}.json", safe(chat))), s);
    }
}

/// Убрать кэш аккаунта (при удалении аккаунта с ПК).
pub fn forget(uid: &str) {
    if let Some(d) = dir(uid) {
        let _ = std::fs::remove_dir_all(d);
    }
}
