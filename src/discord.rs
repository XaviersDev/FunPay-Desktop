//! Статус в Discord (Rich Presence) через локальный канал Discord.
//! Работает своим потоком: подключается сам, когда Discord запущен, и переподключается.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// ID приложения в Discord Developer Portal: его имя Discord показывает как "Играет в ...".
/// Пустой: статус выключен, в настройках об этом написано.
pub const APP_ID: &str = "1067105848813420724";

pub fn ready() -> bool {
    !APP_ID.is_empty()
}
const LOGO: &str = "https://funpay.tools/assets/logo.png";

#[derive(Clone, Debug, PartialEq)]
pub struct Act {
    pub details: String,
    pub state: String,
    pub buttons: Vec<(String, String)>,
    pub start: i64,
}

static TX: OnceLock<Mutex<Sender<Option<Act>>>> = OnceLock::new();
static LAST: Mutex<Option<Act>> = Mutex::new(None);

/// Новый статус. None убирает статус и отключается от Discord.
pub fn set(a: Option<Act>) {
    {
        let mut l = LAST.lock().unwrap();
        if *l == a {
            return;
        }
        *l = a.clone();
    }
    let tx = TX.get_or_init(|| {
        let (tx, rx) = channel::<Option<Act>>();
        std::thread::Builder::new()
            .name("discord".into())
            .spawn(move || {
                let mut cur: Option<Act> = None;
                let mut conn: Option<Box<dyn Pipe>> = None;
                let mut dirty = false;
                let mut said = String::from("-");
                loop {
                    match rx.recv_timeout(Duration::from_secs(15)) {
                        Ok(a) => {
                            cur = a;
                            dirty = true;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    while let Ok(a) = rx.try_recv() {
                        cur = a;
                        dirty = true;
                    }
                    let Some(a) = cur.clone() else {
                        if let Some(mut c) = conn.take() {
                            let _ = send(&mut c, 1, &activity_cmd(None));
                        }
                        status(&mut said, "");
                        continue;
                    };
                    if !ready() {
                        status(&mut said, "Статус в Discord появится в следующем обновлении.");
                        continue;
                    }
                    if conn.is_none() {
                        conn = connect();
                        dirty = true;
                        if conn.is_none() {
                            status(&mut said, "Discord не найден. Как только он запустится на этом компьютере, статус появится сам.");
                            continue;
                        }
                    }
                    if dirty {
                        let ok = conn.as_mut().map(|c| send(c, 1, &activity_cmd(Some(&a))).is_ok()).unwrap_or(false);
                        if ok {
                            dirty = false;
                            status(&mut said, "Статус виден в вашем профиле Discord.");
                        } else {
                            conn = None;
                        }
                    }
                }
            })
            .ok();
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap().send(a);
}

fn status(said: &mut String, s: &str) {
    if said != s {
        *said = s.to_string();
        let s = s.to_string();
        crate::ui::post(move |u| u.rpc_status(s));
    }
}

fn activity_cmd(a: Option<&Act>) -> Value {
    let activity = a.map(|a| {
        let mut v = json!({
            "details": clip(&a.details, 120),
            "state": clip(&a.state, 120),
            "timestamps": { "start": a.start },
            "assets": { "large_image": LOGO, "large_text": "FunPay Tools" },
            "instance": false,
        });
        if !a.buttons.is_empty() {
            v["buttons"] = Value::Array(a.buttons.iter().take(2).map(|(l, u)| json!({ "label": clip(l, 31), "url": u })).collect());
        }
        v
    });
    json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": std::process::id(), "activity": activity },
        "nonce": format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)),
    })
}

fn clip(s: &str, n: usize) -> String {
    let mut t: String = s.chars().take(n).collect();
    // Discord не принимает строки короче двух символов
    while t.chars().count() < 2 {
        t.push(' ');
    }
    t
}

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

fn open(i: u32) -> Option<Box<dyn Pipe>> {
    #[cfg(windows)]
    {
        let f = std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{i}")).ok()?;
        Some(Box::new(f))
    }
    #[cfg(unix)]
    {
        let dir = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().find_map(|k| std::env::var(k).ok()).unwrap_or_else(|| "/tmp".into());
        let s = std::os::unix::net::UnixStream::connect(format!("{dir}/discord-ipc-{i}")).ok()?;
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        Some(Box::new(s))
    }
}

fn connect() -> Option<Box<dyn Pipe>> {
    for i in 0..10 {
        let Some(mut p) = open(i) else { continue };
        if write_frame(&mut p, 0, &json!({ "v": 1, "client_id": APP_ID })).is_err() {
            continue;
        }
        match read_frame(&mut p) {
            Ok((_, v)) if v.get("evt").and_then(|e| e.as_str()) == Some("READY") => return Some(p),
            Ok((_, v)) => crate::log::write(&format!("discord: не принял подключение: {v}")),
            Err(_) => {}
        }
    }
    None
}

fn send(p: &mut Box<dyn Pipe>, op: u32, v: &Value) -> Result<(), String> {
    write_frame(p, op, v)?;
    // ответ читаем всегда, иначе канал забьётся
    let (rop, r) = read_frame(p)?;
    if rop == 2 {
        return Err("Discord закрыл подключение".into());
    }
    if r.get("evt").and_then(|e| e.as_str()) == Some("ERROR") {
        crate::log::write(&format!("discord: {r}"));
    }
    Ok(())
}

fn write_frame(p: &mut Box<dyn Pipe>, op: u32, v: &Value) -> Result<(), String> {
    let body = v.to_string().into_bytes();
    let mut buf = Vec::with_capacity(8 + body.len());
    buf.extend_from_slice(&op.to_le_bytes());
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    p.write_all(&buf).map_err(|e| e.to_string())?;
    p.flush().map_err(|e| e.to_string())
}

fn read_frame(p: &mut Box<dyn Pipe>) -> Result<(u32, Value), String> {
    let mut head = [0u8; 8];
    p.read_exact(&mut head).map_err(|e| e.to_string())?;
    let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if len > 1 << 20 {
        return Err("слишком длинный ответ".into());
    }
    let mut body = vec![0u8; len];
    p.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames() {
        let a = Act { details: "x".into(), state: "Продаж сегодня: 3".into(), buttons: vec![("FunPay Tools".into(), "https://funpay.tools".into())], start: 1 };
        let v = activity_cmd(Some(&a));
        assert_eq!(v["args"]["activity"]["details"], "x ");
        assert_eq!(v["args"]["activity"]["buttons"][0]["label"], "FunPay Tools");
        assert!(activity_cmd(None)["args"]["activity"].is_null());
    }
}
