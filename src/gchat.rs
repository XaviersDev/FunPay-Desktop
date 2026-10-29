//! Общий чат FunPay Tools (тот же, что в расширении и в приложении).
//! POST WORKER {action}: start → {code}; poll {code} → {token}; fetch {since} → {messages}; send {...}

use reqwest::blocking::Client;
use std::time::Duration;

pub const WORKER: &str = "https://fpt-chat.starobinskiy01.workers.dev";
pub const BOT: &str = "FPToolsBot";
pub const MAX_LEN: usize = 300;
const CFG_URLS: [&str; 2] = [
    "https://cdn.jsdelivr.net/gh/XaviersDev/FunPay-Tools@main/public-chat.json",
    "https://raw.githubusercontent.com/XaviersDev/FunPay-Tools/main/public-chat.json",
];

#[derive(Debug, Clone, Default)]
pub struct GMsg {
    pub id: String,
    pub ts: i64,
    pub nick: String,
    pub avatar: String,
    pub url: String,
    pub text: String,
    pub pro: String,
}

#[derive(Debug, Clone)]
pub struct GConfig {
    pub active: bool,
    pub display: bool,
    pub disabled_message: String,
}

impl Default for GConfig {
    fn default() -> Self {
        Self { active: true, display: true, disabled_message: "Общий чат временно выключен.".into() }
    }
}

pub struct GChat {
    http: Client,
}

impl GChat {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent(concat!("FunPayToolsDesktop/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("client"),
        }
    }

    pub fn call(&self, payload: serde_json::Value) -> Result<(bool, serde_json::Value), String> {
        self.call_status(payload).map(|(s, v)| ((200..300).contains(&s), v))
    }

    pub fn call_status(&self, payload: serde_json::Value) -> Result<(u16, serde_json::Value), String> {
        let r = self.http.post(WORKER).json(&payload).send().map_err(|_| "Сеть недоступна, попробуйте ещё раз".to_string())?;
        let status = r.status().as_u16();
        let v = r.json::<serde_json::Value>().unwrap_or(serde_json::Value::Null);
        Ok((status, v))
    }

    pub fn config(&self) -> GConfig {
        for u in CFG_URLS {
            let url = format!("{u}?t={}", chrono::Utc::now().timestamp());
            if let Ok(r) = self.http.get(url).send() {
                if let Ok(v) = r.json::<serde_json::Value>() {
                    return GConfig {
                        active: v.get("active").and_then(|x| x.as_bool()).unwrap_or(true),
                        display: v.get("display").and_then(|x| x.as_bool()).unwrap_or(true),
                        disabled_message: v
                            .get("disabledMessage")
                            .and_then(|x| x.as_str())
                            .filter(|s| !s.is_empty())
                            .unwrap_or("Общий чат временно выключен.")
                            .to_string(),
                    };
                }
            }
        }
        GConfig::default()
    }

    pub fn fetch(&self, since: i64) -> Result<Vec<GMsg>, String> {
        let (_, v) = self.call(serde_json::json!({ "action": "fetch", "since": since }))?;
        let mut out = Vec::new();
        if let Some(arr) = v.get("messages").and_then(|m| m.as_array()) {
            for o in arr {
                let s = |k: &str| o.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                let id = match o.get("id") {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(serde_json::Value::Number(n)) => n.to_string(),
                    _ => continue,
                };
                if id.is_empty() {
                    continue;
                }
                out.push(GMsg {
                    id,
                    ts: o.get("ts").and_then(|x| x.as_i64()).unwrap_or(0),
                    nick: { let n = s("nick"); if n.is_empty() { "FunPay user".into() } else { n } },
                    avatar: s("avatar"),
                    url: s("url"),
                    text: s("text"),
                    pro: s("pro"),
                });
            }
        }
        Ok(out)
    }

    pub fn start(&self) -> Result<String, String> {
        let (ok, v) = self.call(serde_json::json!({ "action": "start" }))?;
        let code = v.get("code").and_then(|c| c.as_str()).unwrap_or("");
        if ok && !code.is_empty() {
            Ok(code.to_string())
        } else {
            Err("Не удалось начать вход".into())
        }
    }

    pub fn poll(&self, code: &str) -> Option<String> {
        let (_, v) = self.call(serde_json::json!({ "action": "poll", "code": code })).ok()?;
        v.get("token").and_then(|t| t.as_str()).filter(|t| !t.is_empty() && *t != "null").map(|t| t.to_string())
    }

    /// Err((текст, токен протух))
    pub fn send(&self, token: &str, nick: &str, avatar: &str, url: &str, text: &str) -> Result<(), (String, bool)> {
        let (status, v) = self
            .call_status(serde_json::json!({
                "action": "send", "token": token, "nick": nick, "avatar": avatar, "url": url, "text": text
            }))
            .map_err(|e| (e, false))?;
        match status {
            200..=299 if v.get("ok").and_then(|o| o.as_bool()).unwrap_or(true) => Ok(()),
            429 => {
                let sec = (v.get("wait").and_then(|w| w.as_i64()).unwrap_or(5000) + 999) / 1000;
                Err((format!("Подождите {sec} сек перед следующим сообщением"), false))
            }
            401 => Err(("Вход истёк, войдите в чат заново".into(), true)),
            403 => Err(("Доступ к общему чату ограничен.".into(), false)),
            s => {
                let e = v.get("error").and_then(|e| e.as_str()).filter(|e| !e.is_empty()).map(|e| e.to_string()).unwrap_or(format!("ошибка {s}"));
                Err((format!("Не удалось отправить: {e}"), false))
            }
        }
    }

    pub fn bot_link(code: &str) -> String {
        format!("https://t.me/{BOT}?start=fptchat_{code}")
    }
}
