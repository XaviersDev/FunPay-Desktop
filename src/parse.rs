//! Разбор HTML/JSON FunPay: список чатов, сообщения, шапка чата, главная страница.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

pub const FUNPAY: &str = "https://funpay.com";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("selector")
}

static S_CONTACT: Lazy<Selector> = Lazy::new(|| sel("a.contact-item"));
static S_UNAME: Lazy<Selector> = Lazy::new(|| sel(".media-user-name"));
static S_CMSG: Lazy<Selector> = Lazy::new(|| sel(".contact-item-message"));
static S_CTIME: Lazy<Selector> = Lazy::new(|| sel(".contact-item-time"));
static S_AVATAR: Lazy<Selector> = Lazy::new(|| sel(".avatar-photo"));
static S_IMG: Lazy<Selector> = Lazy::new(|| sel("img"));
static S_MSG_ITEM: Lazy<Selector> = Lazy::new(|| sel(".chat-msg-item"));
static S_AUTHOR_LINK: Lazy<Selector> = Lazy::new(|| sel(".chat-msg-author-link"));
static S_MEDIA_LEFT_AV: Lazy<Selector> = Lazy::new(|| sel(".media-left .avatar-photo"));
static S_DATE: Lazy<Selector> = Lazy::new(|| sel(".chat-msg-date"));
static S_TEXT: Lazy<Selector> = Lazy::new(|| sel(".chat-msg-text"));
static S_IMG_LINK: Lazy<Selector> = Lazy::new(|| sel(".chat-img-link"));
static S_LABEL: Lazy<Selector> = Lazy::new(|| sel(".chat-msg-author-label"));
static S_CHAT: Lazy<Selector> = Lazy::new(|| sel("div.chat[data-name]"));
static S_HEADER_LINK: Lazy<Selector> = Lazy::new(|| sel(".chat-header .media-user-name a"));
static S_HEADER_STATUS: Lazy<Selector> = Lazy::new(|| sel(".chat-header .media-user-status"));
static S_HEADER_AV: Lazy<Selector> = Lazy::new(|| sel(".chat-header .avatar-photo"));
static S_CPU: Lazy<Selector> = Lazy::new(|| sel(".param-item.chat-panel[data-type='c-p-u'] a, .chat-panel-mobile a"));
static S_BODY: Lazy<Selector> = Lazy::new(|| sel("body"));
static S_USER_NAME: Lazy<Selector> = Lazy::new(|| sel(".user-link-name"));
static S_USER_PHOTO: Lazy<Selector> = Lazy::new(|| sel(".user-link-photo"));
static S_BALANCE: Lazy<Selector> = Lazy::new(|| sel(".badge-balance"));
static S_BLOCKED: Lazy<Selector> = Lazy::new(|| sel(".account-blocked-box p"));
static S_PROFILE_PHOTO: Lazy<Selector> = Lazy::new(|| sel(".profile-photo, .avatar-photo"));

static RE_URL: Lazy<Regex> = Lazy::new(|| Regex::new(r#"url\(\s*['"]?([^'")]+)['"]?\s*\)"#).unwrap());
static RE_USER_ID: Lazy<Regex> = Lazy::new(|| Regex::new(r"/users/(\d+)").unwrap());
static RE_BR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
static RE_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());

pub fn abs_url(u: &str) -> String {
    let u = u.trim();
    if u.starts_with("//") {
        format!("https:{u}")
    } else if u.starts_with('/') {
        format!("{FUNPAY}{u}")
    } else {
        u.to_string()
    }
}

/// Аватар из элемента: <img src> внутри или background-image в style.
fn avatar_of(el: ElementRef) -> Option<String> {
    if let Some(img) = el.select(&S_IMG).next() {
        if let Some(src) = img.value().attr("src") {
            if !src.is_empty() {
                return Some(abs_url(src));
            }
        }
    }
    let style = el.value().attr("style").unwrap_or("");
    RE_URL.captures(style).map(|c| abs_url(&c[1]))
}

fn text_of(el: ElementRef) -> String {
    el.text().collect::<String>().trim().to_string()
}

fn own_text(el: ElementRef) -> String {
    el.children()
        .filter_map(|n| n.value().as_text().map(|t| t.to_string()))
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn is_default_avatar(url: &str) -> bool {
    url.is_empty() || url.contains("/img/layout/avatar")
}

// ---------------------------------------------------------------- chat list

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ChatItem {
    pub id: String,
    pub name: String,
    pub preview: String,
    pub time: String,
    pub unread: bool,
    pub avatar: String,
    /// id последнего сообщения в чате (data-node-msg)
    pub node_msg: u64,
    /// id последнего прочитанного мной (data-user-msg)
    pub user_msg: u64,
}

pub fn parse_bookmarks(html: &str) -> Vec<ChatItem> {
    let doc = Html::parse_fragment(html);
    let mut out = Vec::new();
    for a in doc.select(&S_CONTACT) {
        let v = a.value();
        let id = v.attr("data-id").unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let name = a.select(&S_UNAME).next().map(text_of).unwrap_or_default();
        let preview = a.select(&S_CMSG).next().map(text_of).unwrap_or_default();
        let time = a.select(&S_CTIME).next().map(text_of).unwrap_or_default();
        let avatar = a.select(&S_AVATAR).next().and_then(avatar_of).unwrap_or_default();
        let node_msg = v.attr("data-node-msg").and_then(|s| s.parse().ok()).unwrap_or(0);
        let user_msg = v.attr("data-user-msg").and_then(|s| s.parse().ok()).unwrap_or(0);
        let unread = v.classes().any(|c| c == "unread");
        out.push(ChatItem { id, name, preview: crate::fpt::strip(&preview), time, unread, avatar, node_msg, user_msg });
    }
    out
}

// ---------------------------------------------------------------- messages

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Message {
    pub id: u64,
    pub author_id: String,
    pub author: String,
    pub avatar: String,
    /// Текст как на сайте (title даты), например «28 октября, 16:24:52»
    pub date_raw: String,
    pub ts: Option<NaiveDateTime>,
    /// У сообщения своя «шапка» (автор + время). Без шапки FunPay клеит его к предыдущему.
    pub has_head: bool,
    pub text: String,
    pub image: Option<String>,
    pub thumb: Option<String>,
    pub badge: String,
    /// В тексте была метка FunPay Tools
    pub fpt: bool,
}

/// Состояние «текущего автора», которое FunPay не повторяет в склеенных сообщениях.
#[derive(Debug, Clone, Default)]
pub struct AuthorCtx {
    pub author_id: String,
    pub author: String,
    pub avatar: String,
    pub date_raw: String,
    pub names: std::collections::HashMap<String, (String, String)>,
}

fn msg_text(el: ElementRef) -> String {
    let html = el.inner_html();
    let html = RE_BR.replace_all(&html, "\n");
    let stripped = RE_TAG.replace_all(&html, "");
    html_unescape(&stripped).trim_matches(|c| c == '\n' || c == '\r' || c == ' ').to_string()
}

pub fn html_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let frag = Html::parse_fragment(&format!("<p>{}</p>", s.replace('<', "&lt;")));
    frag.root_element().text().collect()
}

/// Разбирает один .chat-msg-item. `author_hint` — поле author из JSON (id автора).
pub fn parse_msg_item(item: ElementRef, author_hint: Option<&str>, ctx: &mut AuthorCtx, now: NaiveDateTime) -> Option<Message> {
    let id: u64 = item.value().attr("id")?.trim_start_matches("message-").parse().ok()?;
    let has_head = item.value().classes().any(|c| c == "chat-msg-with-head") || item.select(&S_UNAME).next().is_some();

    if let Some(av) = item.select(&S_MEDIA_LEFT_AV).next().and_then(avatar_of) {
        ctx.avatar = av;
    }
    if let Some(link) = item.select(&S_AUTHOR_LINK).next() {
        ctx.author = text_of(link);
        ctx.author_id = link
            .value()
            .attr("href")
            .and_then(|h| RE_USER_ID.captures(h).map(|c| c[1].to_string()))
            .unwrap_or_default();
    } else if let Some(name_el) = item.select(&S_UNAME).next() {
        let t = own_text(name_el);
        if !t.is_empty() {
            ctx.author = t;
            ctx.author_id = "0".into();
        }
    } else if let Some(h) = author_hint {
        if !h.is_empty() && h != ctx.author_id {
            ctx.author_id = h.to_string();
            if let Some((n, a)) = ctx.names.get(h) {
                ctx.author = n.clone();
                ctx.avatar = a.clone();
            }
        }
    }
    if has_head && item.select(&S_MEDIA_LEFT_AV).next().is_none() {
        // шапка без аватарки (гостевой режим/общий чат) — не тащим чужую аватарку
        if let Some((_, a)) = ctx.names.get(&ctx.author_id) {
            ctx.avatar = a.clone();
        } else {
            ctx.avatar.clear();
        }
    }
    if !ctx.author_id.is_empty() {
        ctx.names.insert(ctx.author_id.clone(), (ctx.author.clone(), ctx.avatar.clone()));
    }
    if let Some(d) = item.select(&S_DATE).next() {
        let t = d.value().attr("title").map(|s| s.to_string()).unwrap_or_else(|| text_of(d));
        ctx.date_raw = t;
    }
    let raw_text = item.select(&S_TEXT).next().map(msg_text).unwrap_or_default();
    let (image, thumb) = match item.select(&S_IMG_LINK).next() {
        Some(a) => {
            let full = a.value().attr("href").map(abs_url);
            let th = a.select(&S_IMG).next().and_then(|i| i.value().attr("src")).map(abs_url);
            (full.clone(), th.or(full))
        }
        None => (None, None),
    };
    if raw_text.is_empty() && image.is_none() {
        return None;
    }
    let badge = item.select(&S_LABEL).next().map(text_of).unwrap_or_default();
    let fpt = crate::fpt::has(&raw_text);
    Some(Message {
        id,
        author_id: ctx.author_id.clone(),
        author: ctx.author.clone(),
        avatar: ctx.avatar.clone(),
        date_raw: ctx.date_raw.clone(),
        ts: parse_fp_date(&ctx.date_raw, now),
        has_head,
        text: crate::fpt::strip(&raw_text),
        image,
        thumb,
        badge,
        fpt,
    })
}

pub fn parse_messages_html(html: &str, ctx: &mut AuthorCtx, now: NaiveDateTime) -> Vec<Message> {
    let doc = Html::parse_document(html);
    doc.select(&S_MSG_ITEM).filter_map(|it| parse_msg_item(it, None, ctx, now)).collect()
}

/// Массив messages из JSON (chat/history и runner chat_node): [{id, author, html}]
pub fn parse_messages_json(arr: &serde_json::Value, ctx: &mut AuthorCtx, now: NaiveDateTime) -> Vec<Message> {
    let mut out = Vec::new();
    let Some(list) = arr.as_array() else { return out };
    for m in list {
        let html = m.get("html").and_then(|h| h.as_str()).unwrap_or("");
        let author = m.get("author").map(|a| match a {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            _ => String::new(),
        });
        let frag = Html::parse_fragment(html);
        if let Some(item) = frag.select(&S_MSG_ITEM).next() {
            if let Some(mut msg) = parse_msg_item(item, author.as_deref(), ctx, now) {
                if msg.id == 0 {
                    msg.id = m.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                }
                out.push(msg);
            }
        }
    }
    out
}

// ---------------------------------------------------------------- chat header

#[derive(Debug, Clone, Default)]
pub struct ChatPage {
    pub node_name: String,
    pub node_id: String,
    pub peer_id: String,
    pub peer_name: String,
    pub peer_status: String,
    pub peer_avatar: String,
    pub lot_name: String,
    pub lot_url: String,
    pub messages: Vec<Message>,
}

pub fn parse_chat_page(html: &str, ctx: &mut AuthorCtx, now: NaiveDateTime) -> ChatPage {
    let doc = Html::parse_document(html);
    let mut p = ChatPage::default();
    if let Some(c) = doc.select(&S_CHAT).next() {
        p.node_name = c.value().attr("data-name").unwrap_or("").to_string();
        p.node_id = c.value().attr("data-id").unwrap_or("").to_string();
    }
    if let Some(a) = doc.select(&S_HEADER_LINK).next() {
        p.peer_name = text_of(a);
        p.peer_id = a
            .value()
            .attr("href")
            .and_then(|h| RE_USER_ID.captures(h).map(|c| c[1].to_string()))
            .unwrap_or_default();
    }
    p.peer_status = doc.select(&S_HEADER_STATUS).next().map(text_of).unwrap_or_default();
    p.peer_avatar = doc.select(&S_HEADER_AV).next().and_then(avatar_of).unwrap_or_default();
    if let Some(a) = doc.select(&S_CPU).next() {
        p.lot_name = text_of(a);
        p.lot_url = a.value().attr("href").map(abs_url).unwrap_or_default();
    }
    if !p.peer_id.is_empty() {
        ctx.names.insert(p.peer_id.clone(), (p.peer_name.clone(), p.peer_avatar.clone()));
    }
    p.messages = doc.select(&S_MSG_ITEM).filter_map(|it| parse_msg_item(it, None, ctx, now)).collect();
    p
}

// ---------------------------------------------------------------- main page

#[derive(Debug, Clone, Default)]
pub struct AppData {
    pub csrf: String,
    pub user_id: String,
    pub username: String,
    pub avatar: String,
    pub balance: String,
    pub blocked: Option<String>,
}

pub fn parse_main(html: &str) -> Option<AppData> {
    let doc = Html::parse_document(html);
    let body = doc.select(&S_BODY).next()?;
    let raw = body.value().attr("data-app-data")?;
    let json: serde_json::Value = serde_json::from_str(raw).ok()?;
    let mut d = AppData {
        csrf: json.get("csrf-token").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        user_id: match json.get("userId") {
            Some(serde_json::Value::Number(n)) => n.to_string(),
            Some(serde_json::Value::String(s)) => s.clone(),
            _ => "0".into(),
        },
        ..Default::default()
    };
    d.username = doc.select(&S_USER_NAME).next().map(text_of).unwrap_or_default();
    d.avatar = doc.select(&S_USER_PHOTO).next().and_then(avatar_of).unwrap_or_default();
    d.balance = doc.select(&S_BALANCE).next().map(text_of).unwrap_or_default();
    let blocked: Vec<String> = doc.select(&S_BLOCKED).map(text_of).collect();
    if html.contains("account-blocked-box") {
        let r = blocked.join("\n\n");
        d.blocked = Some(if r.is_empty() { "Аккаунт заблокирован администрацией FunPay.".into() } else { r });
    }
    Some(d)
}

pub fn parse_profile_avatar(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    doc.select(&S_PROFILE_PHOTO).find_map(avatar_of)
}

// ---------------------------------------------------------------- dates

const MONTHS: [&str; 12] = ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"];
const MONTHS_EN: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
static RE_TIME: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{1,2}):(\d{2})(?::(\d{2}))?").unwrap());
static RE_DMY: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{1,2})\.(\d{1,2})\.(\d{2,4})").unwrap());
static RE_DAY: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{1,2})\s+\p{L}+").unwrap());
static RE_YEAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(\d{4})\b").unwrap());

/// «28 октября, 16:24:52», «сегодня, 14:02», «12.03.2024, 10:00», «16:24:52».
pub fn parse_fp_date(s: &str, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let s = s.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    let tm = RE_TIME.captures(&s).map(|c| {
        let h: u32 = c[1].parse().unwrap_or(0);
        let m: u32 = c[2].parse().unwrap_or(0);
        let sec: u32 = c.get(3).and_then(|x| x.as_str().parse().ok()).unwrap_or(0);
        NaiveTime::from_hms_opt(h.min(23), m.min(59), sec.min(59)).unwrap_or_default()
    });
    let time = tm.unwrap_or_default();
    let today = now.date();
    if s.contains("сегодня") || s.contains("today") {
        return Some(today.and_time(time));
    }
    if s.contains("вчера") || s.contains("yesterday") {
        return Some((today - Duration::days(1)).and_time(time));
    }
    if let Some(c) = RE_DMY.captures(&s) {
        let d: u32 = c[1].parse().ok()?;
        let m: u32 = c[2].parse().ok()?;
        let mut y: i32 = c[3].parse().ok()?;
        if y < 100 {
            y += 2000;
        }
        return NaiveDate::from_ymd_opt(y, m, d).map(|dt| dt.and_time(time));
    }
    let month = MONTHS.iter().position(|m| s.contains(m)).or_else(|| MONTHS_EN.iter().position(|m| s.contains(m)));
    match month {
        Some(mi) => {
            let day: u32 = RE_DAY.captures(&s).and_then(|c| c[1].parse().ok())?;
            let explicit = RE_YEAR.captures(&s).and_then(|c| c[1].parse::<i32>().ok());
            let y = explicit.unwrap_or(today.year());
            let mut dt = NaiveDate::from_ymd_opt(y, mi as u32 + 1, day)?.and_time(time);
            if explicit.is_none() && dt > now + Duration::hours(36) {
                dt = NaiveDate::from_ymd_opt(y - 1, mi as u32 + 1, day)?.and_time(time);
            }
            Some(dt)
        }
        None => tm.map(|t| today.and_time(t)),
    }
}

/// Короткое время для пузыря: «16:24».
pub fn short_time(ts: Option<NaiveDateTime>, raw: &str) -> String {
    match ts {
        Some(t) => format!("{:02}:{:02}", t.hour(), t.minute()),
        None => RE_TIME
            .captures(raw)
            .map(|c| format!("{:0>2}:{}", &c[1], &c[2]))
            .unwrap_or_default(),
    }
}

pub fn day_label(d: NaiveDate, today: NaiveDate) -> String {
    const MONTHS_GEN: [&str; 12] = [
        "января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря",
    ];
    if d == today {
        "сегодня".into()
    } else if d == today - Duration::days(1) {
        "вчера".into()
    } else if d.year() == today.year() {
        format!("{} {}", d.day(), MONTHS_GEN[d.month0() as usize])
    } else {
        format!("{} {} {}", d.day(), MONTHS_GEN[d.month0() as usize], d.year())
    }
}

#[cfg(test)]
mod link_tests {
    use super::*;

    #[test]
    fn links() {
        let t = "смотри https://funpay.com/lots/1/, и funpay.com/users/5 тоже.";
        let l = find_links(t);
        assert_eq!(l.len(), 2);
        assert_eq!(&t[l[0].0..l[0].1], "https://funpay.com/lots/1/");
        assert_eq!(l[1].2, "https://funpay.com/users/5");
        let md = links_markdown("a*b\n\n  x", &[], "");
        assert_eq!(md, "a\\*b\n\u{00A0}\n\u{00A0}\u{00A0}x");
        assert!(find_links("mail me a@b.com").is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 28).unwrap().and_hms_opt(18, 0, 0).unwrap()
    }

    #[test]
    fn dates() {
        let n = now();
        assert_eq!(parse_fp_date("28 октября, 16:24:52", n).unwrap().to_string(), "2026-10-28 16:24:52");
        assert_eq!(parse_fp_date("вчера, 10:05", n).unwrap().to_string(), "2026-10-27 10:05:00");
        assert_eq!(parse_fp_date("12.03.2024, 10:00", n).unwrap().to_string(), "2024-03-12 10:00:00");
        assert_eq!(parse_fp_date("29 декабря, 10:00", n).unwrap().to_string(), "2025-12-29 10:00:00");
        assert_eq!(parse_fp_date("16:24", n).unwrap().to_string(), "2026-10-28 16:24:00");
        assert!(parse_fp_date("", n).is_none());
    }

    #[test]
    fn stacked_messages_inherit_author() {
        let html = r#"<div class="chat-message-list">
<div class="chat-msg-item chat-msg-with-head" id="message-10"><div class="chat-message">
<div class="media-left"><a href="/users/5/"><div class="avatar-photo" style="background-image: url(/img/a.jpg);"></div></a></div>
<div class="media-user-name"><a href="https://funpay.com/users/5/" class="chat-msg-author-link">bob</a>
<div class="chat-msg-date" title="28 октября, 16:24:52">16:24:52</div></div>
<div class="chat-msg-body"><div class="chat-msg-text">привет<br>как дела &amp; всё</div></div></div></div>
<div class="chat-msg-item" id="message-11"><div class="chat-message"><div class="chat-msg-body">
<a class="chat-img-link" href="https://sfunpay.com/s/chat/a/b.jpg"><img class="chat-img" src="https://sfunpay.com/s/chat/a/b_t.jpg"></a></div></div></div>
<div class="chat-msg-item chat-msg-with-head" id="message-12"><div class="chat-message">
<div class="media-user-name">FunPay <span class="chat-msg-author-label label label-primary">оповещение</span>
<div class="chat-msg-date" title="28 октября, 16:30:00">16:30:00</div></div>
<div class="chat-msg-body"><div class="chat-msg-text">Покупатель bob оплатил заказ #ABC.</div></div></div></div>
</div>"#;
        let mut ctx = AuthorCtx::default();
        let m = parse_messages_html(html, &mut ctx, now());
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].author, "bob");
        assert_eq!(m[0].author_id, "5");
        assert_eq!(m[0].text, "привет\nкак дела & всё");
        assert_eq!(m[0].avatar, "https://funpay.com/img/a.jpg");
        assert_eq!(m[1].author_id, "5");
        assert!(!m[1].has_head);
        assert_eq!(m[1].image.as_deref(), Some("https://sfunpay.com/s/chat/a/b.jpg"));
        assert_eq!(m[1].thumb.as_deref(), Some("https://sfunpay.com/s/chat/a/b_t.jpg"));
        assert_eq!(m[2].author, "FunPay");
        assert_eq!(m[2].author_id, "0");
        assert_eq!(m[2].badge, "оповещение");
    }

    #[test]
    fn real_flood_page() {
        let p = std::path::Path::new("tests/data/flood.html");
        if !p.exists() {
            return;
        }
        let html = std::fs::read_to_string(p).unwrap();
        let mut ctx = AuthorCtx::default();
        let page = parse_chat_page(&html, &mut ctx, now());
        assert_eq!(page.node_name, "flood");
        assert!(page.messages.len() > 10);
        assert!(page.messages.iter().all(|m| !m.author.is_empty()));
        assert!(page.messages[0].ts.is_some());
    }

    #[test]
    fn real_history_json() {
        let p = std::path::Path::new("tests/data/hist.json");
        if !p.exists() {
            return;
        }
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let mut ctx = AuthorCtx::default();
        let m = parse_messages_json(&v["chat"]["messages"], &mut ctx, now());
        assert!(m.len() > 50);
        assert!(m.windows(2).all(|w| w[0].id < w[1].id));
        assert!(m.iter().all(|x| !x.author_id.is_empty()));
    }

    #[test]
    fn bookmarks() {
        let html = r#"<a href="https://funpay.com/chat/?node=123" class="contact-item unread" data-id="123" data-node-msg="500" data-user-msg="400">
<div class="contact-item-photo"><div class="avatar-photo" style="background-image: url(https://sfunpay.com/s/avatar/x.jpg);"></div></div>
<div class="media-user-name">darkness21</div><div class="contact-item-message">Оплатил. Ключ ещё есть?</div><div class="contact-item-time">14:18</div></a>
<a class="contact-item" data-id="77" data-node-msg="9" data-user-msg="9"><div class="media-user-name">sanych</div><div class="contact-item-message">ok</div><div class="contact-item-time">вчера</div></a>"#;
        let c = parse_bookmarks(html);
        assert_eq!(c.len(), 2);
        assert!(c[0].unread && !c[1].unread);
        assert_eq!(c[0].avatar, "https://sfunpay.com/s/avatar/x.jpg");
        assert_eq!(c[0].node_msg, 500);
        assert_eq!(c[1].name, "sanych");
    }

    #[test]
    fn main_page() {
        let html = r#"<html><body data-app-data='{"locale":"ru","csrf-token":"abc","userId":42}'>
<a class="user-link-dropdown"><div class="user-link-photo"><img src="/img/me.jpg"></div><div class="user-link-name">Xavier</div></a>
<span class="badge badge-balance">1 250 ₽</span></body></html>"#;
        let d = parse_main(html).unwrap();
        assert_eq!(d.csrf, "abc");
        assert_eq!(d.user_id, "42");
        assert_eq!(d.username, "Xavier");
        assert_eq!(d.avatar, "https://funpay.com/img/me.jpg");
        assert_eq!(d.balance, "1 250 ₽");
    }
}

// ---------------------------------------------------------------- профиль

#[derive(Debug, Clone, Default)]
pub struct Offer {
    pub title: String,
    pub price: String,
    pub server: String,
    pub url: String,
}

#[derive(Debug, Clone, Default)]
pub struct OfferGroup {
    pub name: String,
    pub url: String,
    pub offers: Vec<Offer>,
}

#[derive(Debug, Clone, Default)]
pub struct Review {
    pub rating: i32,
    pub text: String,
    pub date: String,
    pub detail: String,
    pub reply: String,
    pub author_avatar: String,
    /// номер заказа (только на своей странице)
    pub order: String,
    pub author: String,
}

#[derive(Debug, Clone, Default)]
pub struct Profile {
    pub user_id: String,
    pub name: String,
    pub avatar: String,
    pub online: bool,
    pub status: String,
    pub reg_date: String,
    pub reg_ago: String,
    pub rating: String,
    pub reviews_count: String,
    pub groups: Vec<OfferGroup>,
    pub reviews: Vec<Review>,
    pub cont: String,
    /// узел личного чата с этим человеком (users-a-b), если FunPay его отдал
    pub chat_node: String,
}

static S_H1_SPAN: Lazy<Selector> = Lazy::new(|| sel("h1 > span.mr4, h1 > span"));
static S_PROFILE_AV: Lazy<Selector> = Lazy::new(|| sel(".profile-header .avatar-photo, .avatar-photo"));
static S_USER_STATUS: Lazy<Selector> = Lazy::new(|| sel("h1 .media-user-status, .media-user-status"));
static S_PARAM_ITEM: Lazy<Selector> = Lazy::new(|| sel(".param-item"));
static S_H5: Lazy<Selector> = Lazy::new(|| sel("h5"));
static S_NOWRAP: Lazy<Selector> = Lazy::new(|| sel(".text-nowrap"));
static S_RATING_BIG: Lazy<Selector> = Lazy::new(|| sel(".rating-value .big"));
static S_RATING_COUNT: Lazy<Selector> = Lazy::new(|| sel(".rating-full-count a, .rating-full-count"));
static S_OFFER: Lazy<Selector> = Lazy::new(|| sel(".offer"));
static S_OFFER_TITLE: Lazy<Selector> = Lazy::new(|| sel(".offer-list-title a"));
static S_TC_ITEM: Lazy<Selector> = Lazy::new(|| sel("a.tc-item"));
static S_TC_DESC: Lazy<Selector> = Lazy::new(|| sel(".tc-desc-text"));
static S_TC_PRICE: Lazy<Selector> = Lazy::new(|| sel(".tc-price"));
static S_TC_SERVER: Lazy<Selector> = Lazy::new(|| sel(".tc-server"));
static S_REVIEW: Lazy<Selector> = Lazy::new(|| sel(".review-container"));
static S_REVIEW_RATING: Lazy<Selector> = Lazy::new(|| sel(".review-item-rating .rating > div"));
static S_REVIEW_TEXT: Lazy<Selector> = Lazy::new(|| sel(".review-item-text"));
static S_REVIEW_DATE: Lazy<Selector> = Lazy::new(|| sel(".review-item-date"));
static S_REVIEW_DETAIL: Lazy<Selector> = Lazy::new(|| sel(".review-item-detail"));
static S_REVIEW_ANSWER: Lazy<Selector> = Lazy::new(|| sel(".review-item-answer > div, .review-item-answer"));
static S_REVIEW_PHOTO: Lazy<Selector> = Lazy::new(|| sel(".review-item-photo img"));
static S_CONTINUE: Lazy<Selector> = Lazy::new(|| sel("input[name=continue]"));
static RE_RATING: Lazy<Regex> = Lazy::new(|| Regex::new(r"rating(\d)").unwrap());

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn parse_reviews(doc: &Html) -> Vec<Review> {
    doc.select(&S_REVIEW)
        .filter_map(|r| {
            let rating = r
                .select(&S_REVIEW_RATING)
                .next()
                .and_then(|d| d.value().attr("class").and_then(|c| RE_RATING.captures(c).and_then(|m| m[1].parse().ok())))
                .unwrap_or(0);
            let text = r.select(&S_REVIEW_TEXT).next().map(text_with_breaks).unwrap_or_default();
            let date = r.select(&S_REVIEW_DATE).next().map(text_of).unwrap_or_default();
            let detail = r.select(&S_REVIEW_DETAIL).next().map(|e| squash(&text_of(e))).unwrap_or_default();
            let reply = r
                .select(&S_REVIEW_ANSWER_TEXT)
                .next()
                .or_else(|| r.select(&S_REVIEW_ANSWER).next())
                .map(text_with_breaks)
                .map(|t| strip_controls(&t))
                .unwrap_or_default();
            let author_avatar = r.select(&S_REVIEW_PHOTO).next().and_then(|i| i.value().attr("src")).map(abs_url).unwrap_or_default();
            // на своей странице FunPay показывает, к какому заказу отзыв
            let order = r
                .select(&S_REVIEW_ORDER)
                .next()
                .and_then(|a| a.value().attr("href"))
                .and_then(|h| RE_ORDER_HREF.captures(h).map(|c| c[1].to_string()))
                .unwrap_or_default();
            let author = r.select(&S_REVIEW_AUTHOR).next().map(text_of).unwrap_or_default();
            if text.is_empty() && date.is_empty() {
                None
            } else {
                Some(Review { rating, text, date, detail, reply, author_avatar, order, author })
            }
        })
        .collect()
}

pub fn parse_continue(doc: &Html) -> String {
    doc.select(&S_CONTINUE).next().and_then(|i| i.value().attr("value")).unwrap_or("").to_string()
}

pub fn parse_profile(html: &str, user_id: &str) -> Profile {
    let doc = Html::parse_document(html);
    let mut p = Profile { user_id: user_id.to_string(), ..Default::default() };
    p.name = doc.select(&S_H1_SPAN).next().map(text_of).unwrap_or_default();
    p.avatar = doc.select(&S_PROFILE_AV).next().and_then(avatar_of).unwrap_or_default();
    p.status = doc.select(&S_USER_STATUS).next().map(|e| squash(&text_of(e))).unwrap_or_default();
    p.online = p.status.to_lowercase().contains("онлайн") || p.status.to_lowercase().contains("online");
    for it in doc.select(&S_PARAM_ITEM) {
        let label = it.select(&S_H5).next().map(text_of).unwrap_or_default().to_lowercase();
        if label.contains("регистрац") || label.contains("registration") {
            if let Some(v) = it.select(&S_NOWRAP).next() {
                let lines: Vec<String> = v.text().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
                p.reg_date = lines.first().cloned().unwrap_or_default();
                p.reg_ago = lines.get(1).cloned().unwrap_or_default();
            }
        }
    }
    p.rating = doc.select(&S_RATING_BIG).next().map(text_of).unwrap_or_default();
    p.reviews_count = doc.select(&S_RATING_COUNT).next().map(|e| squash(&text_of(e))).unwrap_or_default();
    for o in doc.select(&S_OFFER) {
        let Some(t) = o.select(&S_OFFER_TITLE).next() else { continue };
        let offers: Vec<Offer> = o
            .select(&S_TC_ITEM)
            .map(|row| Offer {
                title: row.select(&S_TC_DESC).next().map(|e| squash(&text_of(e))).filter(|s| !s.is_empty()).unwrap_or_else(|| "Без названия".into()),
                price: row.select(&S_TC_PRICE).next().map(|e| squash(&text_of(e))).unwrap_or_default(),
                server: row.select(&S_TC_SERVER).next().map(|e| squash(&text_of(e))).unwrap_or_default(),
                url: row.value().attr("href").map(abs_url).unwrap_or_default(),
            })
            .collect();
        if !offers.is_empty() {
            p.groups.push(OfferGroup { name: text_of(t), url: t.value().attr("href").map(abs_url).unwrap_or_default(), offers });
        }
    }
    p.reviews = parse_reviews(&doc);
    p.cont = parse_continue(&doc);
    if let Some(c) = doc.select(&S_CHAT).next() {
        let n = c.value().attr("data-name").unwrap_or("");
        if n.starts_with("users-") {
            p.chat_node = n.to_string();
        }
    }
    p
}

// ---------------------------------------------------------------- заказ

#[derive(Debug, Clone, Default)]
pub struct Order {
    pub id: String,
    pub status: String,
    pub game: String,
    pub short_desc: String,
    pub price: String,
    pub peer_name: String,
    pub peer_id: String,
    pub peer_avatar: String,
    pub can_refund: bool,
    pub can_confirm: bool,
    pub is_buyer: bool,
    pub has_review: bool,
    pub review_rating: i32,
    pub review_text: String,
    pub review_reply: String,
    /// ответ на отзыв мой (FunPay показывает «Редактировать / Удалить»)
    pub reply_mine: bool,
    pub params: Vec<(String, String)>,
    pub lot_url: String,
    pub chat_node: String,
}

static S_PAGE_HEADER: Lazy<Selector> = Lazy::new(|| sel("h1.page-header"));
static S_REVIEW_ORDER: Lazy<Selector> = Lazy::new(|| sel(".review-item-order a"));
static S_REVIEW_AUTHOR: Lazy<Selector> = Lazy::new(|| sel(".review-item-user .media-user-name"));
static RE_ORDER_HREF: Lazy<Regex> = Lazy::new(|| Regex::new(r"/orders/([A-Z0-9]+)").unwrap());
static S_REVIEW_ANSWER_TEXT: Lazy<Selector> = Lazy::new(|| sel(".review-item-answer > div:first-child"));

/// Текст элемента с переносами строк на месте <br> и блоков, пробелы внутри строк сжаты.
pub fn text_with_breaks(el: ElementRef) -> String {
    let mut out = String::new();
    for n in el.descendants() {
        if let Some(t) = n.value().as_text() {
            // переносы в исходнике HTML — это просто пробелы, строку рвёт только <br>
            out.push_str(&t.replace(['\n', '\r', '\t'], " "));
        } else if let Some(e) = n.value().as_element() {
            if e.name() == "br" {
                out.push('\n');
            }
        }
    }
    out.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n").trim().to_string()
}

/// Убирает хвост «Редактировать Удалить», который FunPay кладёт рядом с ответом.
fn strip_controls(s: &str) -> String {
    let mut t = s.trim().to_string();
    loop {
        let before = t.len();
        for w in ["Редактировать", "Удалить", "Edit", "Delete"] {
            if let Some(x) = t.strip_suffix(w) {
                t = x.trim_end().to_string();
            }
        }
        if t.len() == before {
            break;
        }
    }
    t
}
static S_STATUS_SUCCESS: Lazy<Selector> = Lazy::new(|| sel(".page-header .text-success, .page-header .text-warning, .page-header .text-danger, .page-header span"));
static S_DIV: Lazy<Selector> = Lazy::new(|| sel("div"));
static S_SPAN_H1: Lazy<Selector> = Lazy::new(|| sel("span.h1"));
static S_STRONG: Lazy<Selector> = Lazy::new(|| sel("strong"));
static S_CHAT_FLOAT_LINK: Lazy<Selector> = Lazy::new(|| sel(".chat-float .media-user-name a, .chat-header .media-user-name a"));
static S_CHAT_FLOAT_AV: Lazy<Selector> = Lazy::new(|| sel(".chat-float .avatar-photo, .chat-header .avatar-photo, .chat-float .media-left img"));
static S_BTN_REFUND: Lazy<Selector> = Lazy::new(|| sel("button.btn-refund"));
static S_BTN_COMPLETE: Lazy<Selector> = Lazy::new(|| sel("button.btn-complete"));
static S_LOT_LINK: Lazy<Selector> = Lazy::new(|| sel("a[href*='/lots/offer'], a[href*='/chips/offer']"));
static S_REVIEW_H5: Lazy<Selector> = Lazy::new(|| sel(".order-review h5, .review-list h5"));
static RE_ORDER_ID: Lazy<Regex> = Lazy::new(|| Regex::new(r"#([A-Z0-9]{6,10})").unwrap());

pub fn find_order_ids(text: &str) -> Vec<String> {
    let mut v: Vec<String> = RE_ORDER_ID.captures_iter(text).map(|c| c[1].to_string()).collect();
    v.dedup();
    v
}

static RE_LINK: Lazy<Regex> = Lazy::new(|| Regex::new(r"https?://[^\s<>\]\)]+").unwrap());

/// Все ссылки в тексте: байтовые границы и адрес (хвостовые знаки препинания не входят).
/// Ловит и голые адреса вида funpay.com/... без http.
pub fn find_links(text: &str) -> Vec<(usize, usize, String)> {
    static RE_ANY: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)\b(?:https?://[^\s<>\]\)]+|(?:www\.)?[a-z0-9][a-z0-9-]*(?:\.[a-z0-9-]+)*\.(?:com|ru|me|gg|net|org|io|tools|su|ua|kz|by|tv|app|dev|xyz|shop|store|pro)(?:/[^\s<>\]\)]*)?)").unwrap()
    });
    let mut out = Vec::new();
    for m in RE_ANY.find_iter(text) {
        let s = m.as_str();
        let t = s.trim_end_matches(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '»' | '"' | '\'' | '…'));
        if t.len() < 4 {
            continue;
        }
        // e-mail и части слов не трогаем
        if m.start() > 0 && text[..m.start()].ends_with('@') {
            continue;
        }
        let url = if t.to_lowercase().starts_with("http") { t.to_string() } else { format!("https://{t}") };
        out.push((m.start(), m.start() + t.len(), url));
    }
    out
}

/// Текст для StyledText: всё экранировано, ссылки в разметке. Строки сохраняются как есть.
pub fn links_markdown(text: &str, links: &[(usize, usize, String)], tail: &str) -> String {
    fn esc(s: &str, out: &mut String) {
        for ch in s.chars() {
            if ch.is_ascii_punctuation() {
                out.push('\\');
            }
            out.push(ch);
        }
    }
    let mut out = String::with_capacity(text.len() + 64);
    let mut pos = 0;
    let mut body = String::new();
    for (a, b, url) in links {
        esc(&text[pos..*a], &mut body);
        body.push('[');
        esc(&text[*a..*b], &mut body);
        body.push_str("](<");
        body.push_str(&url.replace('>', "%3E").replace('<', "%3C").replace(' ', "%20"));
        body.push_str(">)");
        pos = *b;
    }
    esc(&text[pos..], &mut body);
    body.push_str(tail);
    // пустые строки и отступы markdown съел бы: заменяем неразрывными пробелами
    for (i, line) in body.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let lead = line.len() - line.trim_start_matches(' ').len();
        if line.trim().is_empty() {
            out.push('\u{00A0}');
        } else {
            for _ in 0..lead {
                out.push('\u{00A0}');
            }
            out.push_str(&line[lead..]);
        }
    }
    out
}

/// Первая ссылка в тексте сообщения, с обрезанными хвостовыми знаками препинания.
pub fn first_url(text: &str) -> Option<String> {
    let m = RE_LINK.find(text)?;
    let s = m.as_str().trim_end_matches(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '»' | '"' | '\''));
    if s.len() < 10 {
        None
    } else {
        Some(s.to_string())
    }
}

pub fn parse_order(html: &str) -> Order {
    let doc = Html::parse_document(html);
    let mut o = Order::default();
    if let Some(h) = doc.select(&S_PAGE_HEADER).next() {
        let t = text_of(h);
        o.id = t.split('#').nth(1).map(|s| s.split_whitespace().next().unwrap_or("").to_string()).unwrap_or_default();
    }
    o.status = doc.select(&S_STATUS_SUCCESS).last().map(|e| squash(&text_of(e))).unwrap_or_default();
    for it in doc.select(&S_PARAM_ITEM) {
        let label = it.select(&S_H5).next().map(|e| squash(&text_of(e))).unwrap_or_default();
        if label.is_empty() || label.contains("Действия") || label.to_lowercase().contains("отзыв") {
            continue;
        }
        let value = it.select(&S_DIV).last().map(|e| squash(&text_of(e))).unwrap_or_default();
        let ll = label.to_lowercase();
        if ll.contains("игра") || ll.contains("game") {
            o.game = value.clone();
        }
        if ll.contains("краткое описание") || ll.contains("short description") {
            o.short_desc = value.clone();
        }
        if ll.contains("сумма") || ll.contains("amount") || ll.contains("total") {
            let big = it.select(&S_SPAN_H1).next().map(text_of).unwrap_or_default();
            let cur = it.select(&S_STRONG).last().map(text_of).unwrap_or_default();
            o.price = squash(&format!("{big} {cur}"));
            if o.price.is_empty() {
                o.price = value.clone();
            }
            continue;
        }
        if ll.contains("продавец") || ll.contains("seller") {
            o.is_buyer = true;
        }
        if !value.is_empty() {
            o.params.push((label, value));
        }
    }
    if let Some(a) = doc.select(&S_CHAT_FLOAT_LINK).next() {
        o.peer_name = text_of(a);
        o.peer_id = a.value().attr("href").and_then(|h| RE_USER_ID.captures(h).map(|c| c[1].to_string())).unwrap_or_default();
    }
    o.peer_avatar = doc.select(&S_CHAT_FLOAT_AV).next().and_then(avatar_of).unwrap_or_default();
    o.can_refund = doc.select(&S_BTN_REFUND).next().is_some();
    o.can_confirm = doc.select(&S_BTN_COMPLETE).next().is_some();
    if o.can_confirm || html.contains("js-balance-pay") {
        o.is_buyer = true;
    }
    if doc.select(&S_REVIEW_H5).any(|h| text_of(h).to_lowercase().contains("для продавца")) {
        o.is_buyer = true;
    }
    if let Some(r) = doc.select(&S_REVIEW).next() {
        let rating = r
            .value()
            .attr("data-rating")
            .and_then(|v| v.parse().ok())
            .or_else(|| r.select(&S_REVIEW_RATING).next().and_then(|d| d.value().attr("class").and_then(|c| RE_RATING.captures(c).and_then(|m| m[1].parse().ok()))))
            .unwrap_or(0);
        o.review_rating = rating;
        o.has_review = rating > 0;
        if o.has_review {
            o.review_text = r.select(&S_REVIEW_TEXT).next().map(text_with_breaks).unwrap_or_default();
            o.review_reply = r.select(&S_REVIEW_ANSWER_TEXT).next().or_else(|| r.select(&S_REVIEW_ANSWER).next()).map(text_with_breaks).map(|t| strip_controls(&t)).unwrap_or_default();
            o.reply_mine = r.select(&S_REVIEW_ANSWER).next().map(|a| text_of(a).contains("Редактировать") || text_of(a).contains("Edit")).unwrap_or(false);
        }
    }
    o.lot_url = doc.select(&S_LOT_LINK).next().and_then(|a| a.value().attr("href")).map(abs_url).unwrap_or_default();
    if let Some(c) = doc.select(&S_CHAT).next() {
        o.chat_node = c.value().attr("data-id").unwrap_or("").to_string();
    }
    o
}

/// Текст для показа: без вариаторов эмодзи (рисуются квадратами) и прочих невидимых хвостов.
pub fn display_text(s: &str) -> String {
    s.chars().filter(|c| !matches!(*c, '\u{FE0F}' | '\u{FE0E}' | '\u{200B}' | '\u{2060}' | '\u{FEFF}')).collect()
}

/// Ответ в формате расширения: «╭─ ⤸ цитата\n╰ ответ» → (цитата, ответ).
pub fn split_reply(text: &str) -> (Option<String>, String) {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)^╭─\s*⤸\s?(.*?)\n╰\s?(.*)$").unwrap());
    match RE.captures(text) {
        Some(c) => (Some(c[1].trim().to_string()), c[2].to_string()),
        None => (None, text.to_string()),
    }
}

/// Превью в списке чатов: одна строка, ответы как «↩ текст».
pub fn preview_text(raw: &str) -> String {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)╭─\s*⤸\s*(.*?)\s*╰\s*(.*)$").unwrap());
    let s = match RE.captures(raw) {
        Some(c) => format!("↩ {}", &c[2]),
        None => raw.to_string(),
    };
    display_text(&squash(&s))
}

#[cfg(test)]
mod tests2 {
    use super::*;

    #[test]
    fn profile_real() {
        let p = std::path::Path::new("tests/data/user.html");
        if !p.exists() {
            return;
        }
        let pr = parse_profile(&std::fs::read_to_string(p).unwrap(), "2846982");
        assert_eq!(pr.name, "oneprice555");
        assert!(pr.avatar.starts_with("https://sfunpay.com/"));
        assert!(pr.reg_date.contains("2020"), "{}", pr.reg_date);
        assert_eq!(pr.rating, "5");
        assert!(pr.reviews_count.contains("14"), "{}", pr.reviews_count);
        assert!(!pr.groups.is_empty());
        assert!(pr.groups[0].offers[0].url.contains("offer?id="));
        assert!(!pr.reviews.is_empty());
        assert_eq!(pr.reviews[0].rating, 5);
        assert!(!pr.status.is_empty());
    }

    #[test]
    fn replies_and_previews() {
        let (q, t) = split_reply("╭─ ⤸ Есть ключ?\n╰ Да, есть");
        assert_eq!(q.as_deref(), Some("Есть ключ?"));
        assert_eq!(t, "Да, есть");
        assert_eq!(split_reply("просто").0, None);
        assert_eq!(preview_text("╭─ ⤸ Есть ключ?\n╰ Да,\nесть"), "↩ Да, есть");
        assert_eq!(preview_text("💛\nСпасибо   за заказ"), "💛 Спасибо за заказ");
        assert_eq!(find_order_ids("Покупатель оплатил заказ #F88412AB. Товар"), vec!["F88412AB"]);
    }
}

// ---------------------------------------------------------------- форма лота (lots/offerEdit)

static S_OFFER_FORM: Lazy<Selector> = Lazy::new(|| sel("form.form-offer-editor"));
static S_ANY_FORM: Lazy<Selector> = Lazy::new(|| sel("form"));
static S_FORM_INPUT: Lazy<Selector> = Lazy::new(|| sel("input[name], textarea[name], select[name]"));
static S_OPTION: Lazy<Selector> = Lazy::new(|| sel("option"));

/// Все поля формы лота так, как их отправил бы браузер (как Account.get_lot_fields в Cardinal):
/// выключенные чекбоксы пропускаем, у select берём выбранный или первый вариант.
pub fn parse_offer_form(html: &str) -> Vec<(String, String)> {
    let doc = Html::parse_document(html);
    let form = doc.select(&S_OFFER_FORM).next().or_else(|| {
        doc.select(&S_ANY_FORM).find(|f| f.select(&S_FORM_INPUT).any(|i| i.value().attr("name") == Some("node_id")))
    });
    let Some(form) = form else { return vec![] };
    let mut out: Vec<(String, String)> = Vec::new();
    for el in form.select(&S_FORM_INPUT) {
        let v = el.value();
        let name = v.attr("name").unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let value = match v.name() {
            "textarea" => el.text().collect::<String>(),
            "select" => {
                let opts: Vec<_> = el.select(&S_OPTION).collect();
                opts.iter()
                    .find(|o| o.value().attr("selected").is_some())
                    .or(opts.first())
                    .map(|o| o.value().attr("value").map(|s| s.to_string()).unwrap_or_else(|| text_of(*o)))
                    .unwrap_or_default()
            }
            _ => {
                let ty = v.attr("type").unwrap_or("text").to_ascii_lowercase();
                if matches!(ty.as_str(), "submit" | "button" | "file" | "image" | "reset") {
                    continue;
                }
                if ty == "checkbox" || ty == "radio" {
                    if v.attr("checked").is_none() {
                        continue;
                    }
                    v.attr("value").unwrap_or("on").to_string()
                } else {
                    v.attr("value").unwrap_or("").to_string()
                }
            }
        };
        out.push((name, value));
    }
    out
}

/// Ставит значение поля (заменяет все одноимённые, иначе добавляет).
pub fn form_set(form: &mut Vec<(String, String)>, name: &str, value: &str) {
    let mut found = false;
    form.retain_mut(|(k, v)| {
        if k == name {
            if found {
                return false;
            }
            found = true;
            *v = value.to_string();
        }
        true
    });
    if !found {
        form.push((name.to_string(), value.to_string()));
    }
}

#[cfg(test)]
mod tests_form {
    use super::*;
    #[test]
    fn offer_form_like_browser() {
        let html = r#"<form class="form-offer-editor" action="/lots/offerSave" method="post">
<input type="hidden" name="csrf_token" value="tok"><input type="hidden" name="offer_id" value="0"><input type="hidden" name="node_id" value="2046">
<input type="hidden" name="location" value="">
<select name="fields[type]"><option value="">Выберите</option><option value="a" selected>A</option></select>
<select name="server_id"><option value="11">S1</option><option value="12">S2</option></select>
<input type="text" name="fields[summary][ru]" value="">
<textarea name="fields[desc][ru]">старое
описание</textarea>
<input type="text" name="price" value="">
<input type="checkbox" name="active" checked><input type="checkbox" name="auto_delivery">
<button type="submit" name="go">Сохранить</button></form>"#;
        let mut f = parse_offer_form(html);
        let get = |f: &Vec<(String, String)>, k: &str| f.iter().find(|x| x.0 == k).map(|x| x.1.clone());
        assert_eq!(get(&f, "csrf_token").as_deref(), Some("tok"));
        assert_eq!(get(&f, "fields[type]").as_deref(), Some("a"));
        assert_eq!(get(&f, "server_id").as_deref(), Some("11"));
        assert_eq!(get(&f, "fields[desc][ru]").as_deref(), Some("старое\nописание"));
        assert_eq!(get(&f, "active").as_deref(), Some("on"));
        assert!(get(&f, "auto_delivery").is_none());
        assert!(get(&f, "go").is_none());
        form_set(&mut f, "price", "1000");
        form_set(&mut f, "amount", "1");
        assert_eq!(get(&f, "price").as_deref(), Some("1000"));
        assert_eq!(get(&f, "amount").as_deref(), Some("1"));
    }
}

#[cfg(test)]
mod tests_reviews {
    use super::*;
    #[test]
    fn own_reviews_with_orders() {
        let html = std::fs::read_to_string("tests/data/my_reviews.html").unwrap();
        let doc = Html::parse_document(&html);
        let r = parse_reviews(&doc);
        assert!(r.len() >= 2);
        assert_eq!(r[0].order, "QK6Z4MYV");
        assert_eq!(r[0].author, "dimaTDM1337");
        assert_eq!(r[0].text, "nice servers");
        assert!(r[0].reply.starts_with("Спасибо за отзыв! 🤝\n⸨"), "{}", r[0].reply);
        assert!(!r[0].reply.contains("Редактировать"));
        assert_eq!(r[1].order, "JPAL2G1C");
    }
}
