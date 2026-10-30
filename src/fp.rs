//! Клиент FunPay: куки (golden_key, PHPSESSID, golden_seal), runner, чаты, отправка, загрузки.
//! Логика сессии повторяет FpSession.kt из Android-приложения.

use crate::parse::{self, AppData, AuthorCtx, ChatItem, ChatPage, Message};
use reqwest::blocking::{multipart, Client};
use reqwest::header::{HeaderMap, LOCATION, SET_COOKIE};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
/// Метка клиента, которую просит администрация FunPay: уходит кукой в каждом запросе.
pub const CLIENT_COOKIE: (&str, &str) = ("fpt_client", "funpay-tools-desktop");
const FRESH_SESSION_EVERY: Duration = Duration::from_secs(50 * 60);
pub const NOT_KNOWN: &str = "FunPay не принял golden_key. Проверьте, что скопировали его целиком. Если при входе на сайте FunPay просит код, войдите через браузер: там можно ввести код.";
const AUTH_TTL: Duration = Duration::from_secs(20 * 60);

pub enum Body {
    None,
    Form(Vec<(String, String)>),
    File { field: String, name: String, mime: String, bytes: Vec<u8>, extra: Vec<(String, String)> },
}

pub struct Resp {
    pub status: u16,
    pub text: String,
}

pub struct Fp {
    http: Client,
    pub gk: String,
    pub cookies: BTreeMap<String, String>,
    pub app: AppData,
    auth_at: Option<Instant>,
    fresh_at: Option<Instant>,
    pub cookies_dirty: bool,
}

/// User-Agent браузера, через который вошли: FunPay видит те же куки с тем же браузером.
static UA_OVERRIDE: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

pub fn set_ua(ua: &str) {
    let ua = ua.trim();
    if ua.starts_with("Mozilla/") && ua.len() < 400 {
        *UA_OVERRIDE.write().unwrap() = ua.to_string();
    }
}

pub fn ua() -> String {
    let o = UA_OVERRIDE.read().unwrap();
    if o.is_empty() { UA.to_string() } else { o.clone() }
}

/// Чистим то, что вставили в поле ключа: кавычки, "golden_key=", пробелы, кусок строки кук.
pub fn clean_key(raw: &str) -> String {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"golden_key\x22?\s*[=:]\s*\x22?([A-Za-z0-9]+)").unwrap());
    if let Some(c) = RE.captures(raw) {
        return c[1].to_string();
    }
    raw.trim().trim_matches(|c: char| c == '"' || c == '\'' || c == ';' || c.is_whitespace()).to_string()
}

/// Заполняет пустые поля формы чем угодно подходящим: список берёт первый вариант, текст и число "1".
/// force: поля, на которые пожаловался FunPay (их меняем, даже если что-то уже стоит).
fn fill_required(f: &mut crate::lots::Form, code: &str, force: &[String]) {
    for _ in 0..3 {
        let mut changed = false;
        let names: Vec<String> = f.fields.iter().map(|x| x.name.clone()).collect();
        for n in names {
            let Some(x) = f.get(&n).cloned() else { continue };
            if x.kind == "hidden" || x.kind == "checkbox" || !f.visible(&x) {
                continue;
            }
            let forced = force.iter().any(|k| *k == n || n.contains(&format!("[{k}]")));
            if !x.value.trim().is_empty() && !forced {
                continue;
            }
            let v = match x.kind.as_str() {
                "select" => x.options.iter().map(|o| o.0.clone()).find(|v| !v.is_empty() && *v != x.value).or_else(|| x.options.iter().map(|o| o.0.clone()).find(|v| !v.is_empty())),
                "textarea" => Some(code.to_string()),
                _ => Some("1".to_string()),
            };
            if let Some(v) = v {
                if v != x.value {
                    f.set(&n, &v);
                    changed = true;
                }
            }
        }
        // выбор в списке может открыть новые поля
        if !changed {
            break;
        }
    }
}

pub fn http_client(timeout: u64) -> Client {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(timeout))
        .connect_timeout(Duration::from_secs(12))
        .gzip(true)
        .brotli(true)
        .user_agent(ua())
        .build()
        .expect("http client")
}

/// Клиент для картинок и стикеров: идёт по редиректам (нужно для зеркал GitHub/CDN).
pub fn http_client_follow(timeout: u64) -> Client {
    Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(Duration::from_secs(timeout))
        .connect_timeout(Duration::from_secs(12))
        .gzip(true)
        .brotli(true)
        .user_agent(ua())
        .build()
        .expect("http client")
}

impl Fp {
    pub fn new(gk: &str, cookies: BTreeMap<String, String>) -> Self {
        Self {
            http: http_client(30),
            gk: gk.trim().to_string(),
            cookies,
            app: AppData::default(),
            auth_at: None,
            fresh_at: None,
            cookies_dirty: false,
        }
    }

    fn cookie_header(&self, no_session: bool) -> String {
        let mut s = format!("golden_key={}", self.gk);
        if !self.cookies.contains_key("cookie_prefs") {
            s.push_str("; cookie_prefs=1");
        }
        for (k, v) in &self.cookies {
            if k == "golden_key" || (no_session && k == "PHPSESSID") || k == CLIENT_COOKIE.0 {
                continue;
            }
            s.push_str(&format!("; {k}={v}"));
        }
        s.push_str(&format!("; {}={}", CLIENT_COOKIE.0, CLIENT_COOKIE.1));
        s
    }

    fn capture(&mut self, headers: &HeaderMap) {
        for v in headers.get_all(SET_COOKIE) {
            let Ok(raw) = v.to_str() else { continue };
            let first = raw.split(';').next().unwrap_or("");
            let Some((name, value)) = first.split_once('=') else { continue };
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || name == "golden_key" {
                continue;
            }
            let lower = raw.to_ascii_lowercase();
            let expired = value.is_empty() || value == "deleted" || lower.contains("max-age=0") || lower.contains("expires=thu, 01-jan-1970");
            if expired {
                if self.cookies.remove(name).is_some() {
                    self.cookies_dirty = true;
                }
            } else if self.cookies.get(name).map(|x| x.as_str()) != Some(value) {
                self.cookies.insert(name.to_string(), value.to_string());
                self.cookies_dirty = true;
            }
        }
    }

    fn once(&mut self, post: bool, url: &str, body: &Body, xhr: bool, no_session: bool) -> Result<Resp, String> {
        let mut url = url.to_string();
        let mut post = post;
        for _ in 0..6 {
            let mut rb = if post { self.http.post(&url) } else { self.http.get(&url) };
            rb = rb
                .header("Cookie", self.cookie_header(no_session))
                .header("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8");
            if xhr {
                rb = rb
                    .header("X-Requested-With", "XMLHttpRequest")
                    .header("Accept", "application/json, text/javascript, */*; q=0.01")
                    .header("Origin", "https://funpay.com")
                    .header("Referer", "https://funpay.com/chat/");
            }
            if post {
                rb = match body {
                    Body::None => rb,
                    Body::Form(f) => rb.form(f),
                    Body::File { field, name, mime, bytes, extra } => {
                        let part = multipart::Part::bytes(bytes.clone())
                            .file_name(name.clone())
                            .mime_str(mime)
                            .map_err(|e| e.to_string())?;
                        let mut form = multipart::Form::new().part(field.clone(), part);
                        for (k, v) in extra {
                            form = form.text(k.clone(), v.clone());
                        }
                        rb.multipart(form)
                    }
                };
            }
            let resp = rb.send().map_err(net_err)?;
            self.capture(resp.headers());
            let status = resp.status().as_u16();
            if (300..400).contains(&status) {
                if let Some(loc) = resp.headers().get(LOCATION).and_then(|l| l.to_str().ok()) {
                    url = if loc.starts_with("http") { loc.to_string() } else { parse::abs_url(loc) };
                    post = false;
                    continue;
                }
            }
            let text = resp.text().map_err(net_err)?;
            return Ok(Resp { status, text });
        }
        Err("Слишком много перенаправлений".into())
    }

    /// Запрос с повтором при 428 (нет golden_seal): обновляем печать и пробуем ещё раз.
    pub fn request(&mut self, post: bool, url: &str, body: Body, xhr: bool) -> Result<Resp, String> {
        let r = self.once(post, url, &body, xhr, false)?;
        if r.status == 428 {
            self.refresh_seal();
            let r2 = self.once(post, url, &body, xhr, false)?;
            if r2.status == 428 {
                return Err("FunPay требует повторный вход (428). Войдите на сайте и обновите golden_key.".into());
            }
            return Ok(r2);
        }
        Ok(r)
    }

    fn refresh_seal(&mut self) {
        let uid = if self.app.user_id.is_empty() { "0".to_string() } else { self.app.user_id.clone() };
        let tag: String = (0..8).map(|_| "0123456789abcdef".as_bytes()[rand::random::<usize>() % 16] as char).collect();
        let objects = format!(r#"[{{"type":"chat_counter","id":"{uid}","tag":"{tag}","data":false}}]"#);
        let _ = self.once(
            true,
            "https://funpay.com/runner/",
            &Body::Form(vec![("objects".into(), objects), ("request".into(), "false".into())]),
            true,
            false,
        );
    }

    pub fn is_authed(&self) -> bool {
        !self.app.csrf.is_empty() && self.app.user_id != "0" && !self.app.user_id.is_empty()
    }

    /// Главная страница: csrf, userId, ник, аватар, баланс. Раз в 50 минут — новая сессия по golden_key.
    pub fn ensure_auth(&mut self, force: bool) -> Result<(), String> {
        if !force && self.is_authed() && self.auth_at.map(|t| t.elapsed() < AUTH_TTL).unwrap_or(false) {
            return Ok(());
        }
        if self.gk.is_empty() {
            return Err("Нет golden_key".into());
        }
        // Сначала пробуем уже открытую сессию (после входа через браузер в ней пройдена проверка кодом),
        // свежую сессию по одному golden_key заводим только по таймеру или если старой нет.
        // Если свежая не вышла, возвращаемся к старой: при включённом входе по коду FunPay может её не дать.
        let saved = self.cookies.get("PHPSESSID").cloned();
        let due = self.fresh_at.map(|t| t.elapsed() > FRESH_SESSION_EVERY).unwrap_or(false);
        let mut modes: Vec<bool> = match (&saved, due) {
            (None, _) => vec![true, true],
            (Some(_), false) => vec![false, true],
            (Some(_), true) => vec![true, false],
        };
        let mut last_err = String::from("FunPay не ответил");
        let mut not_known = false;
        let mut attempt = 0u64;
        while !modes.is_empty() && attempt < 5 {
            attempt += 1;
            let fresh = modes[0];
            if fresh {
                if self.cookies.remove("PHPSESSID").is_some() {
                    self.cookies_dirty = true;
                }
            } else if let Some(v) = &saved {
                if self.cookies.get("PHPSESSID") != Some(v) {
                    self.cookies.insert("PHPSESSID".into(), v.clone());
                    self.cookies_dirty = true;
                }
            }
            let r = match self.once(false, "https://funpay.com/", &Body::None, false, fresh) {
                Ok(r) => r,
                Err(e) => {
                    last_err = e;
                    std::thread::sleep(Duration::from_millis(1500 * attempt));
                    continue;
                }
            };
            if fresh {
                self.fresh_at = Some(Instant::now());
            }
            if r.status == 428 {
                self.refresh_seal();
                continue;
            }
            if is_cloudflare(&r.text) {
                last_err = "FunPay временно закрыт защитой Cloudflare, пробую ещё".into();
                std::thread::sleep(Duration::from_millis(2500 * attempt));
                continue;
            }
            let Some(d) = parse::parse_main(&r.text) else {
                last_err = format!("Не удалось прочитать страницу FunPay (HTTP {})", r.status);
                modes.remove(0);
                continue;
            };
            if let Some(reason) = d.blocked.clone() {
                return Err(format!("Аккаунт заблокирован: {reason}"));
            }
            if d.user_id == "0" || d.user_id.is_empty() || d.username.is_empty() {
                crate::log::write(&format!("вход: FunPay не узнал ключ ({} сессия, HTTP {})", if fresh { "новая" } else { "старая" }, r.status));
                not_known = true;
                modes.remove(0);
                continue;
            }
            if fresh && self.fresh_at.is_none() {
                self.fresh_at = Some(Instant::now());
            }
            if !fresh && self.fresh_at.is_none() {
                // старая сессия рабочая: следующую свежую попробуем по таймеру
                self.fresh_at = Some(Instant::now());
            }
            self.app = d;
            self.auth_at = Some(Instant::now());
            return Ok(());
        }
        if not_known {
            return Err(NOT_KNOWN.into());
        }
        Err(last_err)
    }

    pub fn runner(&mut self, objects: &str, request: &str) -> Result<serde_json::Value, String> {
        self.ensure_auth(false)?;
        let body = Body::Form(vec![
            ("objects".into(), objects.to_string()),
            ("request".into(), request.to_string()),
            ("csrf_token".into(), self.app.csrf.clone()),
        ]);
        let r = self.request(true, "https://funpay.com/runner/", body, true)?;
        if r.text.contains("account-blocked-box") {
            return Err("Аккаунт заблокирован администрацией FunPay".into());
        }
        let v: serde_json::Value = serde_json::from_str(&r.text).map_err(|_| format!("FunPay ответил не JSON (HTTP {})", r.status))?;
        if v.get("error").and_then(|e| e.as_bool()).unwrap_or(false) {
            // чаще всего протух csrf: обновим и пусть вызывающий повторит
            self.auth_at = None;
            let msg = v.get("msg").and_then(|m| m.as_str()).unwrap_or("ошибка runner");
            return Err(msg.to_string());
        }
        Ok(v)
    }

    pub fn bookmarks(&mut self) -> Result<Vec<ChatItem>, String> {
        let uid = self.ensure_auth(false).map(|_| self.app.user_id.clone())?;
        let objects = format!(r#"[{{"type":"chat_bookmarks","id":"{uid}","tag":"00000000","data":false}}]"#);
        let v = match self.runner(&objects, "false") {
            Ok(v) => v,
            Err(_) => {
                self.ensure_auth(true)?;
                self.runner(&objects, "false")?
            }
        };
        let mut out = Vec::new();
        if let Some(objs) = v.get("objects").and_then(|o| o.as_array()) {
            for o in objs {
                if o.get("type").and_then(|t| t.as_str()) == Some("chat_bookmarks") {
                    if let Some(html) = o.pointer("/data/html").and_then(|h| h.as_str()) {
                        out = parse::parse_bookmarks(html);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Один запрос runner, как делает сам сайт: список чатов + новые сообщения открытого чата.
    pub fn poll(&mut self, open: Option<(&str, u64)>, ctx: &mut AuthorCtx) -> Result<(Option<Vec<ChatItem>>, Option<Vec<Message>>), String> {
        self.ensure_auth(false)?;
        let uid = self.app.user_id.clone();
        let mut objs = vec![format!(r#"{{"type":"chat_bookmarks","id":"{uid}","tag":"00000000","data":false}}"#)];
        if let Some((node, last)) = open {
            let last = if last == 0 { -1i64 } else { last as i64 };
            objs.push(format!(
                r#"{{"type":"chat_node","id":"{node}","tag":"00000000","data":{{"node":"{node}","last_message":{last},"content":""}}}}"#
            ));
        }
        let objects = format!("[{}]", objs.join(","));
        let v = match self.runner(&objects, "false") {
            Ok(v) => v,
            Err(_) => {
                self.ensure_auth(true)?;
                self.runner(&objects, "false")?
            }
        };
        let mut chats = None;
        let mut msgs = None;
        if let Some(arr) = v.get("objects").and_then(|o| o.as_array()) {
            for o in arr {
                match o.get("type").and_then(|t| t.as_str()) {
                    Some("chat_bookmarks") => {
                        if let Some(html) = o.pointer("/data/html").and_then(|h| h.as_str()) {
                            chats = Some(parse::parse_bookmarks(html));
                        }
                    }
                    Some("chat_node") => {
                        if let Some(m) = o.pointer("/data/messages") {
                            let mut list = parse::parse_messages_json(m, ctx, now_msk());
                            if let Some((_, last)) = open {
                                list.retain(|x| x.id > last);
                            }
                            msgs = Some(list);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok((chats, msgs))
    }

    pub fn chat_page(&mut self, node: &str, ctx: &mut AuthorCtx) -> Result<ChatPage, String> {
        self.ensure_auth(false)?;
        let r = self.request(false, &format!("https://funpay.com/chat/?node={node}"), Body::None, false)?;
        if r.status >= 400 {
            return Err(format!("Чат не открылся (HTTP {})", r.status));
        }
        Ok(parse::parse_chat_page(&r.text, ctx, now_msk()))
    }

    /// Сообщения до `before` (или последние 100, если before = None). Возвращает (сообщения, есть ли ещё).
    pub fn history(&mut self, node: &str, before: Option<u64>, ctx: &mut AuthorCtx) -> Result<(Vec<Message>, bool, String), String> {
        let last = before.map(|b| b.to_string()).unwrap_or_else(|| "99999999999999".into());
        let url = format!("https://funpay.com/chat/history?node={node}&last_message={last}");
        let r = self.request(false, &url, Body::None, true)?;
        let v: serde_json::Value = serde_json::from_str(&r.text).map_err(|_| format!("История не загрузилась (HTTP {})", r.status))?;
        let chat = v.get("chat").ok_or("История не загрузилась")?;
        let name = chat.pointer("/node/name").and_then(|n| n.as_str()).unwrap_or(node).to_string();
        let more = chat.get("hasHistory").and_then(|h| h.as_bool()).unwrap_or(false);
        let msgs = parse::parse_messages_json(chat.get("messages").unwrap_or(&serde_json::Value::Null), ctx, now_msk());
        Ok((msgs, more, name))
    }

    /// Новые сообщения после last_id через runner (заодно отмечает чат прочитанным).
    pub fn poll_node(&mut self, node: &str, last_id: u64, ctx: &mut AuthorCtx) -> Result<Vec<Message>, String> {
        let last = if last_id == 0 { -1i64 } else { last_id as i64 };
        let objects = format!(
            r#"[{{"type":"chat_node","id":"{node}","tag":"00000000","data":{{"node":"{node}","last_message":{last},"content":""}}}}]"#
        );
        let v = self.runner(&objects, "false")?;
        let mut out = Vec::new();
        if let Some(objs) = v.get("objects").and_then(|o| o.as_array()) {
            for o in objs {
                if o.get("type").and_then(|t| t.as_str()) == Some("chat_node") {
                    if let Some(m) = o.pointer("/data/messages") {
                        out = parse::parse_messages_json(m, ctx, now_msk());
                    }
                }
            }
        }
        out.retain(|m| m.id > last_id);
        Ok(out)
    }

    pub fn send_message(&mut self, node: &str, text: &str, image_id: Option<&str>) -> Result<(), String> {
        let mut data = serde_json::json!({ "node": node, "last_message": -1 });
        match image_id {
            Some(id) => {
                data["image_id"] = serde_json::Value::String(id.to_string());
                data["content"] = serde_json::Value::String(String::new());
            }
            None => data["content"] = serde_json::Value::String(text.to_string()),
        }
        let request = serde_json::json!({ "action": "chat_message", "data": data }).to_string();
        let v = match self.runner("[]", &request) {
            Ok(v) => v,
            Err(_) => {
                self.ensure_auth(true)?;
                self.runner("[]", &request)?
            }
        };
        match runner_send_error(&v) {
            Some(e) => {
                let s = v.to_string();
                crate::log::write(&format!(
                    "отправка {} в {node} не прошла: {e}; ответ: {}",
                    if image_id.is_some() { "картинки" } else { "текста" },
                    s.chars().take(300).collect::<String>()
                ));
                Err(e)
            }
            None => Ok(()),
        }
    }

    pub fn upload_chat_image(&mut self, bytes: Vec<u8>, name: &str, mime: &str) -> Result<String, String> {
        self.ensure_auth(false)?;
        let body = Body::File {
            field: "file".into(),
            name: name.into(),
            mime: mime.into(),
            bytes,
            extra: vec![("file_id".into(), "0".into())],
        };
        let r = self.request(true, "https://funpay.com/file/addChatImage", body, true)?;
        let v: serde_json::Value = match serde_json::from_str(&r.text) {
            Ok(v) => v,
            Err(_) => {
                crate::log::write(&format!("addChatImage HTTP {} не JSON: {}", r.status, r.text.chars().take(300).collect::<String>()));
                return Err(format!("Картинка не загрузилась (HTTP {})", r.status));
            }
        };
        // как в мобильном приложении: fileId, а если его нет, то url
        for key in ["fileId", "url"] {
            if let Some(id) = v.get(key) {
                let id = match id {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => continue,
                    other => other.to_string(),
                };
                if !id.is_empty() {
                    return Ok(id);
                }
            }
        }
        crate::log::write(&format!("addChatImage отказ: {}", r.text.chars().take(300).collect::<String>()));
        Err(v.get("msg").and_then(|m| m.as_str()).unwrap_or("FunPay не принял картинку").to_string())
    }

    pub fn upload_avatar(&mut self, png: Vec<u8>) -> Result<(), String> {
        self.ensure_auth(false)?;
        let body = Body::File {
            field: "file".into(),
            name: "avatar.png".into(),
            mime: "image/png".into(),
            bytes: png,
            extra: vec![("file_id".into(), "0".into())],
        };
        let r = self.request(true, "https://funpay.com/file/avatar", body, true)?;
        if r.status >= 400 {
            return Err(format!("FunPay не принял аватарку (HTTP {})", r.status));
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&r.text) {
            if v.get("error").and_then(|e| e.as_bool()).unwrap_or(false) {
                return Err(v.get("msg").and_then(|m| m.as_str()).unwrap_or("FunPay не принял аватарку").to_string());
            }
        }
        Ok(())
    }

    pub fn profile(&mut self, user_id: &str) -> Result<parse::Profile, String> {
        self.ensure_auth(false)?;
        let r = self.request(false, &format!("https://funpay.com/users/{user_id}/"), Body::None, false)?;
        if r.status >= 400 {
            return Err(format!("Профиль не открылся (HTTP {})", r.status));
        }
        Ok(parse::parse_profile(&r.text, user_id))
    }

    pub fn more_reviews(&mut self, user_id: &str, cont: &str) -> Result<(Vec<parse::Review>, String), String> {
        self.ensure_auth(false)?;
        let body = Body::Form(vec![("user_id".into(), user_id.into()), ("continue".into(), cont.into()), ("filter".into(), String::new())]);
        let r = self.request(true, "https://funpay.com/users/reviews", body, true)?;
        let doc = scraper::Html::parse_document(&r.text);
        Ok((parse::parse_reviews(&doc), parse::parse_continue(&doc)))
    }

    pub fn order(&mut self, id: &str) -> Result<parse::Order, String> {
        self.ensure_auth(false)?;
        let r = self.request(false, &format!("https://funpay.com/orders/{id}/"), Body::None, false)?;
        if r.status >= 400 {
            return Err(format!("Заказ не открылся (HTTP {})", r.status));
        }
        let mut o = parse::parse_order(&r.text);
        if o.id.is_empty() {
            o.id = id.to_string();
        }
        Ok(o)
    }

    fn post_form(&mut self, url: &str, mut fields: Vec<(String, String)>) -> Result<serde_json::Value, String> {
        self.ensure_auth(false)?;
        fields.push(("csrf_token".into(), self.app.csrf.clone()));
        let r = self.request(true, url, Body::Form(fields), true)?;
        if r.status >= 400 {
            return Err(format!("FunPay ответил ошибкой (HTTP {})", r.status));
        }
        let v: serde_json::Value = serde_json::from_str(&r.text).unwrap_or(serde_json::Value::Null);
        if v.get("error").map(|e| e.as_bool().unwrap_or(false) || e.as_i64().unwrap_or(0) != 0).unwrap_or(false) {
            return Err(v.get("msg").and_then(|m| m.as_str()).unwrap_or("FunPay не выполнил действие").to_string());
        }
        Ok(v)
    }

    pub fn refund(&mut self, id: &str) -> Result<(), String> {
        self.post_form("https://funpay.com/orders/refund", vec![("id".into(), id.into())]).map(|_| ())
    }

    pub fn confirm_order(&mut self, id: &str) -> Result<(), String> {
        self.post_form("https://funpay.com/orders/complete", vec![("id".into(), id.into())]).map(|_| ())
    }

    pub fn review(&mut self, id: &str, text: &str, rating: i32) -> Result<(), String> {
        let uid = self.app.user_id.clone();
        self.post_form(
            "https://funpay.com/orders/review",
            vec![("orderId".into(), id.into()), ("text".into(), text.into()), ("rating".into(), rating.to_string()), ("authorId".into(), uid)],
        )
        .map(|_| ())
    }

    pub fn review_delete(&mut self, id: &str) -> Result<(), String> {
        let uid = self.app.user_id.clone();
        self.post_form("https://funpay.com/orders/reviewDelete", vec![("orderId".into(), id.into()), ("authorId".into(), uid)]).map(|_| ())
    }

    /// Пометить чат прочитанным: открыть его в runner с last_message = -1.
    pub fn mark_read(&mut self, node: &str) -> Result<(), String> {
        let objects = format!(r#"[{{"type":"chat_node","id":"{node}","tag":"00000000","data":{{"node":"{node}","last_message":-1,"content":""}}}}]"#);
        self.runner(&objects, "false").map(|_| ())
    }

    /// Все куки для встроенного браузера.
    pub fn all_cookies(&self) -> Vec<(String, String)> {
        let mut v = vec![("golden_key".to_string(), self.gk.clone())];
        for (k, val) in &self.cookies {
            if k != "golden_key" {
                v.push((k.clone(), val.clone()));
            }
        }
        v
    }

    /// Продажи (sales=true) или покупки. Первая страница обычным GET, дальше POST с continue.
    pub fn trade_page(&mut self, sales: bool, state: &str, cont: &str) -> Result<(Vec<crate::money::TradeItem>, String), String> {
        self.ensure_auth(false)?;
        let base = if sales { "https://funpay.com/orders/trade" } else { "https://funpay.com/orders/" };
        let url = if state.is_empty() { base.to_string() } else { format!("{base}?state={state}") };
        let r = if cont.is_empty() {
            self.request(false, &url, Body::None, false)?
        } else {
            self.request(true, &url, Body::Form(vec![("continue".into(), cont.into())]), true)?
        };
        if r.status >= 400 {
            return Err(format!("Список заказов не открылся (HTTP {})", r.status));
        }
        Ok(crate::money::parse_trade(&r.text))
    }

    /// Страница «Финансов» (users/transactions).
    pub fn transactions_page(&mut self, cont: &str) -> Result<(Vec<crate::money::Txn>, String), String> {
        self.ensure_auth(false)?;
        let body = Body::Form(vec![
            ("user_id".into(), self.app.user_id.clone()),
            ("filter".into(), String::new()),
            ("continue".into(), cont.into()),
        ]);
        let r = self.request(true, "https://funpay.com/users/transactions", body, true)?;
        if r.status >= 400 {
            return Err(format!("Финансы не открылись (HTTP {})", r.status));
        }
        if cont.is_empty() {
            crate::log::write(&format!("финансы, разметка строк: {}", crate::money::txn_markup_summary(&r.text)));
        }
        Ok(crate::money::parse_transactions(&r.text))
    }

    pub fn user_id(&self) -> String {
        self.app.user_id.clone()
    }

    /// POST lots/offerSave. Ok(ответ), если FunPay не сообщил об ошибке.
    fn offer_save(&mut self, form: Vec<(String, String)>, referer: &str) -> Result<serde_json::Value, String> {
        let _ = referer;
        let r = self.request(true, "https://funpay.com/lots/offerSave", Body::Form(form), true)?;
        if r.status >= 400 {
            crate::log::write(&format!("offerSave HTTP {}: {}", r.status, r.text.chars().take(300).collect::<String>()));
            return Err(format!("FunPay не сохранил лот (HTTP {})", r.status));
        }
        let v: serde_json::Value = serde_json::from_str(&r.text).unwrap_or(serde_json::Value::Null);
        let err_flag = match v.get("error") {
            Some(serde_json::Value::Bool(b)) => *b,
            Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
            Some(serde_json::Value::String(s)) => !s.is_empty() && s != "0" && s != "false",
            _ => false,
        };
        let errors = v.get("errors").and_then(|e| e.as_array()).cloned().unwrap_or_default();
        if !err_flag && errors.is_empty() {
            return Ok(v);
        }
        let mut parts: Vec<String> = Vec::new();
        if let Some(m) = v.get("msg").and_then(|m| m.as_str()).filter(|m| !m.is_empty()) {
            parts.push(m.to_string());
        }
        for e in errors {
            match e {
                serde_json::Value::Array(a) if a.len() >= 2 => parts.push(format!("{}: {}", a[0].as_str().unwrap_or(""), a[1].as_str().unwrap_or(""))),
                serde_json::Value::String(s) => parts.push(s),
                _ => {}
            }
        }
        crate::log::write(&format!("offerSave отказ: {}", r.text.chars().take(300).collect::<String>()));
        Err(if parts.is_empty() { "FunPay не сохранил лот".into() } else { parts.join("; ") })
    }

    /// Временный лот с кодом для подтверждения аккаунта на сервере описаний
    /// (node 2046, «Прочее для прочих игр», цена 1000, название «FPT Verify код»).
    pub fn create_verify_lot(&mut self, code: &str) -> Result<(), String> {
        self.ensure_auth(false)?;
        let node = "2046";
        let r = self.request(false, &format!("https://funpay.com/lots/offerEdit?node={node}"), Body::None, false)?;
        if r.status >= 400 {
            return Err(format!("Форма лота не открылась (HTTP {})", r.status));
        }
        let mut f = crate::lots::parse_form(&r.text).map_err(|e| {
            crate::log::write(&format!("offerEdit: {e}"));
            "FunPay не отдал форму лота".to_string()
        })?;
        if f.csrf.is_empty() {
            f.csrf = self.app.csrf.clone();
        }
        let title = format!("FPT Verify {code}");
        f.set("offer_id", "0");
        f.set("node_id", node);
        f.set("price", "1000");
        f.set("fields[summary][ru]", &title);
        if f.get("fields[summary][en]").is_some() {
            f.set("fields[summary][en]", &title);
        }
        f.set("fields[desc][ru]", code);
        f.set("active", "on");
        f.set("amount", "1");
        f.fields.retain(|x| x.name != "deleted");
        fill_required(&mut f, code, &[]);
        let referer = format!("https://funpay.com/lots/offerEdit?node={node}");
        // FunPay может назвать ещё поле, которое ему не понравилось: заполняем и пробуем снова
        let mut last = String::new();
        for _ in 0..3 {
            match self.offer_save(f.body(), &referer) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    let named: Vec<String> = e.split(';').filter_map(|p| p.split_once(':').map(|x| x.0.trim().to_string())).filter(|n| !n.is_empty()).collect();
                    last = e;
                    if named.is_empty() {
                        break;
                    }
                    fill_required(&mut f, code, &named);
                }
            }
        }
        Err(last)
    }

    /// Ищет свой лот по тексту в названии и возвращает его id.
    pub fn find_my_offer(&mut self, needle: &str) -> Option<String> {
        let uid = self.app.user_id.clone();
        let p = self.profile(&uid).ok()?;
        for g in &p.groups {
            for o in &g.offers {
                if o.title.contains(needle) {
                    if let Some(id) = o.url.split("id=").nth(1).map(|s| s.split('&').next().unwrap_or("").to_string()).filter(|s| !s.is_empty()) {
                        return Some(id);
                    }
                }
            }
        }
        None
    }

    /// Удаляет свой лот.
    pub fn delete_offer(&mut self, offer_id: &str) -> Result<(), String> {
        self.ensure_auth(false)?;
        let r = self.request(false, &format!("https://funpay.com/lots/offerEdit?offer={offer_id}"), Body::None, false)?;
        let form = parse::parse_offer_form(&r.text);
        let csrf = form.iter().find(|(k, _)| k == "csrf_token").map(|x| x.1.clone()).unwrap_or_else(|| self.app.csrf.clone());
        let body = vec![("csrf_token".to_string(), csrf), ("offer_id".to_string(), offer_id.to_string()), ("deleted".to_string(), "1".to_string())];
        self.offer_save(body, &format!("https://funpay.com/lots/offerEdit?offer={offer_id}")).map(|_| ())
    }

    // ------------------------------------------------------------ мои лоты

    pub fn my_lots(&mut self) -> Result<Vec<crate::lots::MyLot>, String> {
        self.ensure_auth(false)?;
        let uid = self.app.user_id.clone();
        let r = self.request(false, &format!("https://funpay.com/users/{uid}/"), Body::None, false)?;
        if r.status >= 400 {
            return Err(format!("Профиль не открылся (HTTP {})", r.status));
        }
        Ok(crate::lots::parse_my_lots(&r.text))
    }

    /// Форма лота: offer=ID для своего лота, node=ID для нового в категории.
    pub fn lot_form(&mut self, query: &str) -> Result<crate::lots::Form, String> {
        self.ensure_auth(false)?;
        let r = self.request(false, &format!("https://funpay.com/lots/offerEdit?{query}"), Body::None, false)?;
        if r.status == 401 || r.status == 403 {
            return Err("FunPay не пустил: войдите в аккаунт заново".into());
        }
        if r.status >= 400 {
            return Err(format!("Форма лота не открылась (HTTP {})", r.status));
        }
        let mut f = crate::lots::parse_form(&r.text)?;
        if f.csrf.is_empty() {
            f.csrf = self.app.csrf.clone();
        }
        Ok(f)
    }

    pub fn lot_save(&mut self, form: &crate::lots::Form, referer: &str) -> Result<(), String> {
        self.offer_save(form.body(), referer).map(|_| ())
    }

    /// Включить или выключить лот.
    pub fn lot_set_active(&mut self, id: &str, node: &str, on: bool) -> Result<(), String> {
        let mut f = self.lot_form(&format!("offer={id}"))?;
        f.set("active", if on { "on" } else { "" });
        if on && f.db_amount == Some(0) && f.get("amount").map(|a| a.value.trim().is_empty() || a.value.trim() == "0").unwrap_or(false) {
            f.set("amount", "1");
        }
        if f.value("node_id").is_empty() && !node.is_empty() {
            f.set("node_id", node);
        }
        self.lot_save(&f, &format!("https://funpay.com/lots/offerEdit?offer={id}"))
    }

    /// Копия лота в той же категории: пустая форма категории, значения из оригинала.
    pub fn lot_copy(&mut self, id: &str, node: &str) -> Result<(), String> {
        let src = self.lot_form(&format!("offer={id}"))?;
        let node = if src.value("node_id").is_empty() { node.to_string() } else { src.value("node_id") };
        if node.is_empty() {
            return Err("Не понял, в какой категории лот".into());
        }
        let mut blank = self.lot_form(&format!("node={node}"))?;
        let names: Vec<String> = blank.fields.iter().map(|f| f.name.clone()).collect();
        for n in names {
            match n.as_str() {
                "offer_id" => blank.set(&n, "0"),
                "node_id" => blank.set(&n, &node),
                "deleted" => {}
                _ => {
                    if let Some(v) = src.get(&n) {
                        let v = v.value.clone();
                        blank.set(&n, &v);
                    }
                }
            }
        }
        blank.fields.retain(|f| f.name != "deleted");
        self.lot_save(&blank, &format!("https://funpay.com/lots/offerEdit?node={node}"))
    }

    /// Аватарка со страницы профиля (на главной её иногда нет).
    pub fn profile_avatar(&mut self, user_id: &str) -> Option<String> {
        let r = self.request(false, &format!("https://funpay.com/users/{user_id}/"), Body::None, false).ok()?;
        parse::parse_profile_avatar(&r.text)
    }
}

fn net_err(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "FunPay долго не отвечает".into()
    } else if e.is_connect() {
        "Нет связи с FunPay".into()
    } else {
        format!("Ошибка сети: {e}")
    }
}

pub fn is_cloudflare(html: &str) -> bool {
    html.contains("cf-browser-verification") || html.contains("challenge-platform") && !html.contains("data-app-data")
}

/// Причина отказа в отправке из ответа runner, None если всё ушло.
pub fn runner_send_error(v: &serde_json::Value) -> Option<String> {
    let resp = v.get("response");
    if let Some(serde_json::Value::Object(o)) = resp {
        if let Some(e) = o.get("error") {
            let s = match e {
                serde_json::Value::String(s) => s.trim().to_string(),
                serde_json::Value::Bool(true) => "FunPay не принял сообщение".into(),
                serde_json::Value::Number(n) if n.as_i64() != Some(0) => "FunPay не принял сообщение".into(),
                _ => String::new(),
            };
            if !s.is_empty() && s != "null" && s != "false" && s != "0" {
                return Some(if s == "true" || s == "1" { "FunPay не принял сообщение".into() } else { s });
            }
        }
        return None;
    }
    if v.get("error").and_then(|e| e.as_bool()).unwrap_or(false) {
        return Some(v.get("msg").and_then(|m| m.as_str()).unwrap_or("FunPay не принял сообщение").to_string());
    }
    if matches!(resp, Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false))) {
        return Some("FunPay не принял сообщение".into());
    }
    None
}

/// Время FunPay — московское (UTC+3).
pub fn now_msk() -> chrono::NaiveDateTime {
    (chrono::Utc::now() + chrono::Duration::hours(3)).naive_utc()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_form_filled() {
        let html = r#"<form class="form-offer-editor"><input type="hidden" name="csrf_token" value="t">
<div class="form-group"><label>Сервер</label><select name="server_id"><option value="">Выберите</option><option value="5">EU</option></select></div>
<div class="form-group"><label>Ник</label><input type="text" name="fields[nick]" value=""></div>
<div class="form-group"><label>Цена</label><input type="text" name="price" value=""></div></form>"#;
        let mut f = crate::lots::parse_form(html).unwrap();
        fill_required(&mut f, "123", &[]);
        assert_eq!(f.value("server_id"), "5");
        assert_eq!(f.value("fields[nick]"), "1");
    }

    #[test]
    fn key_cleanup() {
        let k = "abcdefghijklmnopqrstuvwxyz012345";
        assert_eq!(clean_key(k), k);
        assert_eq!(clean_key(&format!("  \"{k}\"; ")), k);
        assert_eq!(clean_key(&format!("golden_key={k}")), k);
        assert_eq!(clean_key(&format!("PHPSESSID=zz; golden_key={k}; cookie_prefs=1")), k);
        assert_eq!(clean_key(&format!("\"golden_key\": \"{k}\"")), k);
    }

    #[test]
    fn send_errors() {
        let ok: serde_json::Value = serde_json::from_str(r#"{"objects":[],"response":{"error":null}}"#).unwrap();
        assert_eq!(runner_send_error(&ok), None);
        let e: serde_json::Value = serde_json::from_str(r#"{"response":{"error":"Вы пока что не можете писать в этот чат."}}"#).unwrap();
        assert_eq!(runner_send_error(&e).unwrap(), "Вы пока что не можете писать в этот чат.");
        let f: serde_json::Value = serde_json::from_str(r#"{"response":false}"#).unwrap();
        assert!(runner_send_error(&f).is_some());
    }

    #[test]
    fn cookie_has_client_tag() {
        let mut c = BTreeMap::new();
        c.insert("PHPSESSID".to_string(), "abc".to_string());
        let fp = Fp::new("KEY", c);
        let h = fp.cookie_header(false);
        assert!(h.starts_with("golden_key=KEY"));
        assert!(h.contains("PHPSESSID=abc"));
        assert!(h.contains("fpt_client=funpay-tools-desktop"));
        assert!(!fp.cookie_header(true).contains("PHPSESSID"));
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Живой запрос к FunPay без входа: история общего чата. cargo test live_ -- --ignored
    #[test]
    #[ignore]
    fn live_flood_history() {
        let mut fp = Fp::new("guest", Default::default());
        let mut ctx = AuthorCtx::default();
        let (msgs, more, name) = fp.history("flood", None, &mut ctx).expect("history");
        assert_eq!(name, "flood");
        assert!(msgs.len() > 20, "{}", msgs.len());
        assert!(more);
        let first = msgs[0].id;
        let (older, _, _) = fp.history("flood", Some(first), &mut ctx).expect("older");
        assert!(older.iter().all(|m| m.id < first));
        println!("ok: {} msgs, {} older, last: {} {:?}", msgs.len(), older.len(), msgs.last().unwrap().author, msgs.last().unwrap().ts);
    }

    #[test]
    #[ignore]
    fn live_gchat_fetch() {
        let g = crate::gchat::GChat::new();
        let list = g.fetch(0).expect("fetch");
        println!("gchat: {} msgs; cfg active={}", list.len(), g.config().active);
        if let Some(m) = list.last() {
            println!("{} {} {}", m.nick, m.ts, m.text.chars().take(40).collect::<String>());
        }
    }
}
