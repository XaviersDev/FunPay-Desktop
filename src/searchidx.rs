//! Локальный архив сообщений для глобального поиска: append-only jsonl на аккаунт,
//! %APPDATA%\FunPayTools\searchidx\<uid>.jsonl. Одна строка: {c: чат, n: имя, m: Message}.
//! Дедуп по id сообщения: на каждый чат храним сохранённые id в meta-файле.

use crate::parse::Message;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;

static STATE: Mutex<Option<St>> = Mutex::new(None);

struct St {
    uid: String,
    seen: HashMap<String, HashSet<u64>>,
}

#[derive(Serialize)]
struct Line<'a> {
    c: &'a str,
    k: i32,
    n: String,
    m: &'a Message,
}

#[derive(serde::Deserialize)]
struct LineIn {
    c: String,
    #[serde(default)]
    k: i32,
    n: String,
    m: Message,
}

fn dir() -> PathBuf {
    crate::config::dir().join("searchidx")
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn jsonl_path(uid: &str) -> PathBuf {
    dir().join(format!("{}.jsonl", safe(uid)))
}

fn meta_path(uid: &str) -> PathBuf {
    dir().join(format!("{}.meta.json", safe(uid)))
}

/// Сменить аккаунт: состояние перечитывается с диска. Вызывается при входе.
pub fn set_account(uid: &str) {
    let mut g = STATE.lock().unwrap();
    if g.as_ref().map(|s| s.uid == uid).unwrap_or(false) {
        return;
    }
    if uid.is_empty() {
        *g = None;
        return;
    }
    if let Err(e) = std::fs::create_dir_all(dir()) {
        crate::log::write(&format!("Не удалось создать каталог индекса сообщений: {e}"));
    }
    let seen = std::fs::read_to_string(meta_path(uid))
        .ok()
        .and_then(|s| serde_json::from_str::<HashMap<String, HashSet<u64>>>(&s).ok())
        .unwrap_or_else(|| {
            let mut seen: HashMap<String, HashSet<u64>> = HashMap::new();
            if let Ok(file) = std::fs::File::open(jsonl_path(uid)) {
                for line in BufReader::new(file).lines().flatten() {
                    if let Ok(row) = serde_json::from_str::<LineIn>(&line) {
                        seen.entry(row.c).or_default().insert(row.m.id);
                    }
                }
            }
            seen
        });
    *g = Some(St { uid: uid.to_string(), seen });
}

/// Добавить сообщения чата в архив. Уже сохранённые id пропускаются.
pub fn add(uid: &str, chat: &str, kind: i32, chat_name: &str, msgs: &[Message]) {
    if uid.is_empty() || chat.is_empty() || msgs.is_empty() {
        return;
    }
    let mut g = STATE.lock().unwrap();
    let Some(st) = g.as_mut() else { return };
    if st.uid != uid {
        return;
    }
    let seen = st.seen.entry(chat.to_string()).or_default();
    let mut new_ids = HashSet::new();
    let mut fresh: Vec<&Message> = msgs.iter().filter(|m| !seen.contains(&m.id) && new_ids.insert(m.id)).collect();
    if fresh.is_empty() {
        return;
    }
    fresh.sort_by_key(|m| m.id);
    let name = chat_name.to_string();
    let written = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(jsonl_path(uid))?;
        for m in &fresh {
            let l = Line { c: chat, k: kind, n: name.clone(), m };
            let row = serde_json::to_vec(&l).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            f.write_all(&row)?;
            f.write_all(b"\n")?;
        }
        Ok(())
    })();
    match written {
        Ok(()) => {
            seen.extend(fresh.iter().map(|m| m.id));
            if let Ok(s) = serde_json::to_string(&st.seen) {
                if let Err(e) = std::fs::write(meta_path(uid), s) {
                    crate::log::write(&format!("Не удалось сохранить метаданные индекса сообщений {uid}: {e}"));
                    let _ = std::fs::remove_file(meta_path(uid));
                }
            } else {
                crate::log::write(&format!("Не удалось сериализовать метаданные индекса сообщений для {uid}"));
                let _ = std::fs::remove_file(meta_path(uid));
            }
        }
        Err(e) => crate::log::write(&format!("Не удалось добавить сообщения в индекс {uid}/{chat}: {e}")),
    }
}

/// Поиск по архиву: возвращает (chat_id, имя чата, сообщение), не больше limit.
pub fn search(uid: &str, q: &str, limit: usize) -> Vec<(String, i32, String, Message)> {
    let Ok(file) = std::fs::File::open(jsonl_path(uid)) else { return vec![] };
    let ql = q.trim().to_lowercase();
    if ql.is_empty() || limit == 0 {
        return vec![];
    }
    let mut out = Vec::new();
    for line in BufReader::new(file).lines().flatten() {
        // Быстрый фильтр по сырой строке; JSON разбирается только для совпадений.
        if !line.to_lowercase().contains(&ql) {
            continue;
        }
        let Ok(l) = serde_json::from_str::<LineIn>(&line) else { continue };
        if l.m.text.to_lowercase().contains(&ql) || l.n.to_lowercase().contains(&ql) {
            out.push((l.c, l.k, l.n, l.m));
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

/// Убрать архив аккаунта (при удалении аккаунта с ПК).
pub fn forget(uid: &str) {
    let mut state = STATE.lock().unwrap();
    if state.as_ref().is_some_and(|st| st.uid == uid) {
        *state = None;
    }
    let _ = std::fs::remove_file(jsonl_path(uid));
    let _ = std::fs::remove_file(meta_path(uid));
}

#[cfg(test)]
mod tests {
    use crate::parse::Message;

    fn msg(id: u64, text: &str) -> Message {
        Message { id, text: text.into(), ..Default::default() }
    }

    #[test]
    fn add_dedups_and_search_finds() {
        let uid = format!("test-user-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default());
        let _ = std::fs::remove_file(super::jsonl_path(&uid));
        let _ = std::fs::remove_file(super::meta_path(&uid));
        super::set_account(&uid);
        let m1 = msg(10, "Где мой заказ");
        let m2 = msg(20, "Уже отправил");
        super::add(&uid, "chat1", 0, "Покупатель", &[m1.clone(), m2.clone()]);
        // повторно те же — не дублируются
        super::add(&uid, "chat1", 0, "Покупатель", &[m1.clone(), m2.clone()]);
        // Старые страницы могут приходить с id меньше уже проиндексированного.
        super::add(&uid, "chat1", 0, "Покупатель", &[msg(5, "старое")]);
        super::add(&uid, "chat2", 2, "Другой", &[msg(30, "Опять заказ")]);

        let hits = super::search(&uid, "заказ", 10);
        assert_eq!(hits.len(), 2, "дедуп: chat1 один + chat2 один");
        assert_eq!(hits[0].0, "chat1");
        assert_eq!(hits[0].3.text, "Где мой заказ");
        assert_eq!(super::search(&uid, "старое", 10).len(), 1);
        assert_eq!(super::search(&uid, "заказ", 10)[1].1, 2);

        super::set_account("");
        super::set_account(&uid);
        super::add(&uid, "chat1", 0, "Покупатель", &[m1.clone()]);
        assert_eq!(super::search(&uid, "заказ", 10).len(), 2);

        // новый текст в тот же чат индексируется
        super::add(&uid, "chat1", 0, "Покупатель", &[msg(40, "Проверка дедупа")]);
        assert_eq!(super::search(&uid, "дедупа", 10).len(), 1);

        super::forget(&uid);
    }
}
