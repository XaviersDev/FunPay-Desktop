//! Центр поддержки FunPay (support.funpay.com): заявки, переписка, новая заявка, вложения.
//! Вход через funpay.com/support/sso по кукам аккаунта, дальше у поддержки свои куки.
//! Разметку разбираем так же, как Android-приложение (FunPaySupport.kt).

use crate::fp::Fp;
use reqwest::blocking::multipart;
use reqwest::header::{HeaderMap, LOCATION, SET_COOKIE};
use scraper::{ElementRef, Html, Selector};
use std::collections::BTreeMap;

const BASE: &str = "https://support.funpay.com";
const PER_PAGE: usize = 10;
/// сколько страниц "Все" грузить за раз
pub const LIST_PAGES: u32 = 3;

#[derive(Clone, Debug, Default)]
pub struct Ticket {
    pub id: String,
    pub title: String,
    pub status: String,
    pub date: String,
    /// "Актуальные" на сайте поддержки: всё, что ещё не закрыто (в том числе "Решена")
    pub active: bool,
}

/// Страница списка заявок.
#[derive(Clone, Debug, Default)]
pub struct Page {
    pub items: Vec<Ticket>,
    /// следующая страница "Все", если есть
    pub next: Option<u32>,
    /// всего заявок
    pub total: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Attach {
    pub name: String,
    pub url: String,
    pub image: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Comment {
    pub author: String,
    pub text: String,
    pub time: String,
    pub mine: bool,
    pub avatar: String,
    pub files: Vec<Attach>,
}

#[derive(Clone, Debug, Default)]
pub struct Details {
    pub id: String,
    pub title: String,
    pub status: String,
    pub created: String,
    pub responsible: String,
    pub fields: Vec<(String, String)>,
    pub comments: Vec<Comment>,
    pub can_reply: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Field {
    pub name: String,
    /// text | textarea | select | radio | note (подсказка поддержки, без значения)
    pub kind: String,
    pub label: String,
    pub required: bool,
    pub options: Vec<(String, String)>,
    /// условие показа: (id поля, значение)
    pub cond: Option<(String, String)>,
    pub default: String,
}

pub enum SBody {
    None,
    Form(Vec<(String, String)>),
    File { name: String, mime: String, bytes: Vec<u8>, extra: Vec<(String, String)> },
}

#[derive(Default)]
pub struct Support {
    active: std::collections::HashSet<String>,
    jar: BTreeMap<String, String>,
    csrf: String,
    uid: String,
    gk: String,
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).unwrap()
}

fn text_of(e: &ElementRef) -> String {
    e.text().collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn has_class(e: &ElementRef, c: &str) -> bool {
    e.value().classes().any(|x| x == c)
}

/// Ближайший предок (или сам элемент) с классом или тегом.
fn closest<'a>(e: ElementRef<'a>, class: &str, tag: &str) -> Option<ElementRef<'a>> {
    let mut cur = Some(e);
    while let Some(n) = cur {
        if (!class.is_empty() && has_class(&n, class)) || (!tag.is_empty() && n.value().name() == tag) {
            return Some(n);
        }
        cur = n.parent().and_then(ElementRef::wrap);
    }
    None
}

fn status_of(badge: Option<ElementRef>) -> String {
    let Some(b) = badge else { return "Неизвестен".into() };
    if has_class(&b, "bg-danger") {
        "Открыта".into()
    } else if has_class(&b, "bg-warning") {
        "Ждёт ответа".into()
    } else if has_class(&b, "bg-success") {
        "Решена".into()
    } else if has_class(&b, "bg-secondary") {
        "Закрыта".into()
    } else {
        let t = text_of(&b);
        if t.is_empty() { "Неизвестен".into() } else { t }
    }
}

/// Список заявок со страницы: заявки, есть ли следующая страница, номер последней страницы.
pub fn parse_tickets(html: &str) -> (Vec<Ticket>, bool, u32) {
    let doc = Html::parse_document(html);
    let mut out = Vec::new();
    for item in doc.select(&sel("a.ticket-item")) {
        let id = item.value().attr("href").unwrap_or("").trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let title = item
            .select(&sel(".col-12.mt-2"))
            .next()
            .map(|e| text_of(&e))
            .filter(|t| !t.is_empty())
            .or_else(|| item.value().attr("title").map(|t| t.split_whitespace().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();
        let date = item.select(&sel(".text-secondary")).next().map(|e| text_of(&e)).unwrap_or_default();
        let status = status_of(item.select(&sel(".badge")).next());
        let active = status != "Закрыта";
        out.push(Ticket { id, title, status, date, active });
    }
    let next = doc.select(&sel("a.page-link[rel='next']")).next().is_some();
    static PG: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"[?&]page=(\d+)").unwrap());
    let last = doc
        .select(&sel("a.page-link"))
        .filter_map(|a| a.value().attr("href").and_then(|h| PG.captures(h)).and_then(|c| c[1].parse::<u32>().ok()))
        .max()
        .unwrap_or(1);
    (out, next, last)
}

pub fn parse_details(html: &str, id: &str, uid: &str) -> Details {
    let doc = Html::parse_document(html);
    let title = doc
        .select(&sel(".breadcrumb-item.active"))
        .next()
        .map(|e| text_of(&e).replace(&format!("Заявка #{id}"), "").trim().to_string())
        .unwrap_or_default();
    let badge = doc.select(&sel(".ticket-info-panel .badge")).next();
    let status = if badge.is_none() { "Открыта".to_string() } else { status_of(badge) };
    let mut comments = Vec::new();
    for c in doc.select(&sel(".ticket-comment")) {
        let author = c.select(&sel(".username")).next().map(|e| text_of(&e)).unwrap_or_else(|| "Поддержка".into());
        let text = c.select(&sel(".comment-text")).next().map(|e| html_text(&e.inner_html())).unwrap_or_default();
        let time = c
            .select(&sel(".comment-username span:nth-child(2)"))
            .next()
            .or_else(|| c.select(&sel(".d-sm-none span:nth-child(2)")).next())
            .map(|e| text_of(&e))
            .map(|t| match t.find(" (") {
                Some(i) if t.ends_with(')') => t[..i].to_string(),
                _ => t,
            })
            .unwrap_or_default();
        let style = c.select(&sel(".comment-avatar")).next().and_then(|e| e.value().attr("style").map(|s| s.to_string())).unwrap_or_default();
        static URL: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r#"url\(['"]?([^'")]+)['"]?\)"#).unwrap());
        let raw = URL.captures(&style).and_then(|m| m.get(1)).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
        let avatar = if raw.is_empty() { String::new() } else if raw.starts_with("http") { raw } else { format!("https://funpay.com{raw}") };
        // у сообщений поддержки фон bg-primary-subtle, у своих bg-light-subtle
        let body_cls = c.select(&sel(".comment-body")).next();
        let mine = match body_cls {
            Some(b) if has_class(&b, "bg-primary-subtle") => false,
            Some(b) if has_class(&b, "bg-light-subtle") => true,
            _ => !uid.is_empty() && (style.contains(&format!("/{}", uid)) || style.contains(&format!("/{}.", uid))),
        };
        let mut files = Vec::new();
        for a in c.select(&sel(".attachment-item")) {
            let Some(link) = a.select(&sel("a.attachment-link")).next() else { continue };
            let href = link.value().attr("href").unwrap_or("");
            let url = if href.starts_with("http") { href.to_string() } else { format!("{BASE}{href}") };
            let mut name = text_of(&link);
            if name.is_empty() {
                name = link.value().attr("data-bs-title").unwrap_or("файл").to_string();
            }
            let lu = url.to_lowercase();
            let image = a.select(&sel(".bi-file-earmark-image")).next().is_some() || [".jpg", ".jpeg", ".png", ".gif", ".webp"].iter().any(|x| lu.ends_with(x));
            files.push(Attach { name, url, image });
        }
        comments.push(Comment { author, text, time, mine, avatar, files });
    }
    let mut fields = Vec::new();
    let (mut created, mut responsible) = (String::new(), String::new());
    for row in doc.select(&sel(".ticket-info-panel .mb-3")) {
        let Some(k) = row.select(&sel(".fw-semibold")).next() else { continue };
        let key = text_of(&k);
        let val = if let Some(b) = row.select(&sel(".badge")).next() {
            text_of(&b)
        } else {
            row.select(&sel(".text-break, .col-xl-8, .col-sm-8")).last().map(|e| text_of(&e)).unwrap_or_default()
        };
        if key.is_empty() || val.is_empty() {
            continue;
        }
        match key.as_str() {
            "Статус" => {}
            "Создана" => created = val,
            "Ответственный" => responsible = val,
            _ => {
                if !fields.iter().any(|(a, _): &(String, String)| a == &key) {
                    fields.push((key, val));
                }
            }
        }
    }
    let can_reply = doc.select(&sel("input[name='add_comment[_token]']")).next().is_some();
    // на странице заявки отдельного заголовка нет, берём тему
    let title = if !title.is_empty() {
        title
    } else {
        fields.iter().rev().find(|(k, _)| k.starts_with("Тема")).map(|(_, v)| v.clone()).unwrap_or_default()
    };
    Details { id: id.to_string(), title, status, created, responsible, fields, comments, can_reply }
}

pub fn parse_fields(html: &str, my_nick: &str) -> Vec<Field> {
    let doc = Html::parse_document(html);
    let mut out: Vec<Field> = Vec::new();
    let cond_of = |e: &ElementRef| {
        e.value().attr("data-condition").and_then(|c| {
            let v: serde_json::Value = serde_json::from_str(c).ok()?;
            if v.get("type").and_then(|t| t.as_str()) != Some("equals") {
                return None;
            }
            let fid = v.get("fieldId").map(|x| x.to_string().trim_matches('"').to_string())?;
            let val = v.get("value").map(|x| x.to_string().trim_matches('"').to_string())?;
            Some((format!("ticket[fields][{fid}]"), val))
        })
    };
    for el in doc.select(&sel("input[name^='ticket[fields]'], select[name^='ticket[fields]'], textarea[name^='ticket[comment]'], .form-alert")) {
        // подсказка поддержки под выбранным вариантом
        if has_class(&el, "form-alert") {
            let text = html_text(&el.inner_html());
            if !text.is_empty() {
                out.push(Field { name: format!("note{}", out.len()), kind: "note".into(), label: text, cond: cond_of(&el), ..Default::default() });
            }
            continue;
        }
        let name = el.value().attr("name").unwrap_or("").to_string();
        if name.contains("_token") || name.contains("attachments") {
            continue;
        }
        let Some(boxe) = closest(el, "mb-3", "").or_else(|| closest(el, "", "fieldset")) else { continue };
        let eid = el.value().attr("id").unwrap_or("");
        let label_el = boxe
            .select(&sel("label"))
            .find(|l| !eid.is_empty() && l.value().attr("for") == Some(eid))
            .or_else(|| boxe.select(&sel("label")).next())
            .or_else(|| boxe.select(&sel("legend")).next());
        let raw_label = label_el.map(|l| text_of(&l)).unwrap_or_default();
        let required = label_el.map(|l| has_class(&l, "required")).unwrap_or(false) || raw_label.contains('*');
        let label = raw_label.replace('*', "").trim().to_string();
        let cond = cond_of(&boxe);
        let low = label.to_lowercase();
        let preset = if el.value().name() == "input" && el.value().attr("type") != Some("radio") { el.value().attr("value").unwrap_or("").trim().to_string() } else { String::new() };
        let default = if !preset.is_empty() {
            preset
        } else if low.contains("ник") || low.contains("логин") || low.contains("nickname") {
            my_nick.to_string()
        } else {
            String::new()
        };
        let tag = el.value().name();
        if tag == "select" {
            let options = el
                .select(&sel("option"))
                .filter_map(|o| {
                    let v = o.value().attr("value").unwrap_or("").to_string();
                    let t = text_of(&o);
                    if v.is_empty() || t.contains("Выберите") { None } else { Some((v, t)) }
                })
                .collect();
            out.push(Field { name, kind: "select".into(), label, required, options, cond, default });
        } else if el.value().attr("type") == Some("radio") {
            if out.iter().any(|f| f.name == name) {
                continue;
            }
            let fs = closest(el, "", "fieldset").unwrap_or(boxe);
            let legend = fs.select(&sel("legend")).next().map(|l| text_of(&l).replace('*', "").trim().to_string()).unwrap_or(label.clone());
            let options = fs
                .select(&sel("input[type='radio']"))
                .filter(|r| r.value().attr("name") == Some(name.as_str()))
                .filter_map(|r| {
                    let rid = r.value().attr("id").unwrap_or("");
                    let lab = fs.select(&sel("label")).find(|l| l.value().attr("for") == Some(rid)).map(|l| text_of(&l).replace('*', "").trim().to_string())?;
                    let v = r.value().attr("value").unwrap_or("").to_string();
                    if v.is_empty() { None } else { Some((v, lab)) }
                })
                .collect();
            out.push(Field { name, kind: "radio".into(), label: legend, required, options, cond, default });
        } else if tag == "textarea" {
            if el.value().attr("data-controller").is_some() {
                continue;
            }
            let label = if name.contains("comment") { "Сообщение".into() } else { label };
            out.push(Field { name, kind: "textarea".into(), label, required: false, options: vec![], cond, default });
        } else {
            let t = el.value().attr("type").unwrap_or("text");
            if t == "hidden" || t == "file" || t == "checkbox" {
                continue;
            }
            out.push(Field { name, kind: "text".into(), label, required, options: vec![], cond, default });
        }
    }
    out
}

/// HTML комментария → текст с переносами строк.
fn html_text(html: &str) -> String {
    static BR: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"(?i)<br\s*/?>|</p>|</div>|</li>").unwrap());
    let with_nl = BR.replace_all(html, "\n");
    let frag = Html::parse_fragment(&with_nl);
    let raw: String = frag.root_element().text().collect();
    let mut out = Vec::new();
    for line in raw.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() && out.last().map(|x: &String| x.is_empty()).unwrap_or(true) {
            continue;
        }
        out.push(l);
    }
    while out.last().map(|x| x.is_empty()).unwrap_or(false) {
        out.pop();
    }
    out.join("\n")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Текст сообщения → HTML поддержки: абзацы и переносы.
fn to_html(s: &str) -> String {
    s.split("\n\n").map(|p| format!("<p>{}</p>", esc(p.trim()).replace('\n', "<br>"))).collect::<Vec<_>>().join("")
}

fn abs(base_host: &str, loc: &str) -> String {
    if loc.starts_with("http") {
        loc.to_string()
    } else if loc.starts_with('/') {
        format!("https://{base_host}{loc}")
    } else {
        format!("https://{base_host}/{loc}")
    }
}

fn host_of(url: &str) -> String {
    url.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or("").to_string()
}

impl Support {
    fn jar_line(&self) -> String {
        self.jar.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
    }

    fn take(&mut self, h: &HeaderMap) {
        for v in h.get_all(SET_COOKIE) {
            let Ok(raw) = v.to_str() else { continue };
            let first = raw.split(';').next().unwrap_or("");
            let Some((n, val)) = first.split_once('=') else { continue };
            let lower = raw.to_ascii_lowercase();
            if val.is_empty() || val == "deleted" || lower.contains("max-age=0") {
                self.jar.remove(n.trim());
            } else {
                self.jar.insert(n.trim().to_string(), val.trim().to_string());
            }
        }
    }

    /// Запрос с переходами по редиректам: на funpay.com куки аккаунта, на support.funpay.com свои.
    pub fn req(&mut self, fp: &mut Fp, post: bool, url: &str, body: SBody, xhr: bool, referer: &str) -> Result<(u16, String), String> {
        let mut url = url.to_string();
        let mut post = post;
        let mut body = Some(body);
        for _ in 0..12 {
            let host = host_of(&url);
            let on_fp = host == "funpay.com" || (host.ends_with(".funpay.com") && host != "support.funpay.com");
            let client = fp.raw_http().clone();
            let mut rb = if post { client.post(&url) } else { client.get(&url) };
            let cookie = if on_fp { fp.cookie_line() } else { self.jar_line() };
            if !cookie.is_empty() {
                rb = rb.header("Cookie", cookie);
            }
            rb = rb.header("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8");
            if xhr {
                rb = rb.header("X-Requested-With", "XMLHttpRequest").header("Accept", "application/json, text/javascript, */*; q=0.01");
            }
            if !referer.is_empty() {
                rb = rb.header("Referer", referer);
            }
            if post {
                match body.take().unwrap_or(SBody::None) {
                    SBody::None => {}
                    SBody::Form(f) => rb = rb.form(&f),
                    SBody::File { name, mime, bytes, extra } => {
                        let part = multipart::Part::bytes(bytes).file_name(name).mime_str(&mime).map_err(|e| e.to_string())?;
                        let mut form = multipart::Form::new().part("file", part);
                        for (k, v) in extra {
                            form = form.text(k, v);
                        }
                        rb = rb.multipart(form);
                    }
                }
            }
            let resp = rb.send().map_err(|e| format!("Нет связи с поддержкой FunPay: {e}"))?;
            if on_fp {
                fp.absorb(resp.headers());
            } else {
                self.take(resp.headers());
            }
            let status = resp.status().as_u16();
            if (300..400).contains(&status) {
                if let Some(loc) = resp.headers().get(LOCATION).and_then(|l| l.to_str().ok()) {
                    url = abs(&host, loc);
                    post = false;
                    continue;
                }
            }
            let text = resp.text().unwrap_or_default();
            return Ok((status, text));
        }
        Err("Слишком много перенаправлений".into())
    }

    fn get(&mut self, fp: &mut Fp, url: &str) -> Result<String, String> {
        self.ensure(fp)?;
        let (st, html) = self.req(fp, false, url, SBody::None, false, "")?;
        if st == 429 {
            return Err("Поддержка FunPay просит подождать: слишком много запросов. Попробуй через минуту.".into());
        }
        // сессия поддержки кончилась: заходим заново один раз
        if !html.contains("data-app-config") {
            self.gk.clear();
            self.ensure(fp)?;
            let (_, html) = self.req(fp, false, url, SBody::None, false, "")?;
            return Ok(html);
        }
        Ok(html)
    }

    /// Вход в поддержку через FunPay (один раз на аккаунт, повторно если сессия слетела).
    pub fn ensure(&mut self, fp: &mut Fp) -> Result<(), String> {
        if !self.gk.is_empty() && self.gk == fp.gk && !self.csrf.is_empty() {
            return Ok(());
        }
        self.jar.clear();
        self.csrf.clear();
        let (st, html) = self.req(fp, false, "https://funpay.com/support/sso?return_to=%2Ftickets", SBody::None, false, "")?;
        if st == 429 {
            return Err("Поддержка FunPay просит подождать: слишком много запросов. Попробуй через минуту.".into());
        }
        let doc = Html::parse_document(&html);
        let cfg = doc.select(&sel("body[data-app-config]")).next().and_then(|b| b.value().attr("data-app-config").map(|s| s.to_string()));
        let Some(cfg) = cfg else {
            return Err("Не получилось войти в поддержку FunPay. Обнови вход в аккаунт и попробуй ещё раз.".into());
        };
        let v: serde_json::Value = serde_json::from_str(&cfg).unwrap_or_default();
        self.csrf = v.get("csrfToken").and_then(|x| x.as_str()).unwrap_or("").to_string();
        self.uid = match v.get("userId") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        self.gk = fp.gk.clone();
        Ok(())
    }

    /// Первая загрузка: все актуальные заявки целиком и первые страницы "Все" (сначала новые).
    pub fn tickets(&mut self, fp: &mut Fp) -> Result<Page, String> {
        let mut active = Vec::new();
        for page in 1..=5 {
            let html = self.get(fp, &format!("{BASE}/tickets?status=active&order=newest_first&page={page}"))?;
            let (items, next, _) = parse_tickets(&html);
            active.extend(items);
            if !next {
                break;
            }
        }
        self.active = active.iter().map(|t| t.id.clone()).collect();
        let mut page = self.more(fp, 1, LIST_PAGES)?;
        // актуальные, которых нет среди загруженных, всё равно показываем
        for t in active.into_iter().rev() {
            if !page.items.iter().any(|x| x.id == t.id) {
                page.items.insert(0, Ticket { active: true, ..t });
            }
        }
        page.total = page.total.max(page.items.len());
        Ok(page)
    }

    /// Следующие страницы "Все".
    pub fn more(&mut self, fp: &mut Fp, from: u32, n: u32) -> Result<Page, String> {
        let mut out = Page::default();
        let mut last = from;
        let mut page = from;
        let mut on_page;
        loop {
            let html = self.get(fp, &format!("{BASE}/tickets?status=all&order=newest_first&page={page}"))?;
            let (items, next, lp) = parse_tickets(&html);
            last = last.max(lp).max(page);
            on_page = items.len();
            out.items.extend(items);
            if !next {
                out.next = None;
                break;
            }
            out.next = Some(page + 1);
            if page + 1 >= from + n {
                break;
            }
            page += 1;
        }
        for t in &mut out.items {
            t.active = self.active.contains(&t.id);
        }
        // всего: полные страницы до последней плюс то, что на последней
        out.total = if out.next.is_none() {
            (page as usize - 1) * PER_PAGE + on_page
        } else {
            let html = self.get(fp, &format!("{BASE}/tickets?status=all&order=newest_first&page={last}"))?;
            (last as usize - 1) * PER_PAGE + parse_tickets(&html).0.len()
        };
        Ok(out)
    }

    pub fn details(&mut self, fp: &mut Fp, id: &str) -> Result<Details, String> {
        let html = self.get(fp, &format!("{BASE}/tickets/{id}"))?;
        Ok(parse_details(&html, id, &self.uid))
    }

    pub fn categories(&mut self, fp: &mut Fp) -> Result<Vec<(String, String)>, String> {
        let html = self.get(fp, &format!("{BASE}/tickets/new"))?;
        let doc = Html::parse_document(&html);
        let mut out = Vec::new();
        for o in doc.select(&sel("select#ticket_select_form option")) {
            let v = o.value().attr("value").unwrap_or("").to_string();
            let t = text_of(&o);
            if !v.is_empty() && !t.starts_with("Выберите") {
                out.push((v, t));
            }
        }
        if out.is_empty() {
            return Err("Не нашёл разделы поддержки. Попробуй открыть поддержку в браузере.".into());
        }
        Ok(out)
    }

    pub fn fields(&mut self, fp: &mut Fp, cat: &str, my_nick: &str) -> Result<Vec<Field>, String> {
        let html = self.get(fp, &format!("{BASE}/tickets/new/{cat}"))?;
        Ok(parse_fields(&html, my_nick))
    }

    pub fn create(&mut self, fp: &mut Fp, cat: &str, values: Vec<(String, String)>, message: &str, files: &[String]) -> Result<String, String> {
        let html = self.get(fp, &format!("{BASE}/tickets/new/{cat}"))?;
        let doc = Html::parse_document(&html);
        let token = doc.select(&sel("input[name='ticket[_token]']")).next().and_then(|e| e.value().attr("value").map(|s| s.to_string())).unwrap_or(self.csrf.clone());
        let mut form: Vec<(String, String)> = values.into_iter().filter(|(_, v)| !v.is_empty()).collect();
        if !message.trim().is_empty() {
            form.push(("ticket[comment][body_html]".into(), to_html(message)));
        }
        form.push(("ticket[comment][attachments]".into(), files.join(",")));
        form.push(("ticket[_token]".into(), token));
        let (st, body) = self.req(fp, true, &format!("{BASE}/tickets/create/{cat}"), SBody::Form(form), true, &format!("{BASE}/tickets/new/{cat}"))?;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        if st == 200 {
            if let Some(u) = v.pointer("/action/url").and_then(|x| x.as_str()) {
                return Ok(u.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string());
            }
        }
        let msg = v.get("error").or_else(|| v.get("message")).and_then(|x| x.as_str()).unwrap_or("").to_string();
        if !msg.is_empty() {
            return Err(msg);
        }
        Err(match st {
            400..=499 => "Поддержка не приняла заявку: проверь, что заполнены все поля со звёздочкой.".into(),
            500..=599 => "Ошибка на стороне поддержки FunPay. Попробуй позже.".into(),
            _ => format!("Неожиданный ответ поддержки ({st})"),
        })
    }

    pub fn comment(&mut self, fp: &mut Fp, id: &str, message: &str, files: &[String]) -> Result<(), String> {
        let html = self.get(fp, &format!("{BASE}/tickets/{id}"))?;
        let doc = Html::parse_document(&html);
        let token = doc
            .select(&sel("input[name='add_comment[_token]']"))
            .next()
            .and_then(|e| e.value().attr("value").map(|s| s.to_string()))
            .ok_or("Отвечать в эту заявку уже нельзя (она закрыта).")?;
        let form = vec![
            ("add_comment[comment][body_html]".to_string(), to_html(message)),
            ("add_comment[comment][attachments]".to_string(), files.join(",")),
            ("add_comment[_token]".to_string(), token),
        ];
        let (st, body) = self.req(fp, true, &format!("{BASE}/tickets/{id}/comments/create"), SBody::Form(form), true, &format!("{BASE}/tickets/{id}"))?;
        if (200..300).contains(&st) {
            return Ok(());
        }
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        let msg = v.get("error").or_else(|| v.get("message")).and_then(|x| x.as_str()).unwrap_or("").to_string();
        Err(if msg.is_empty() { format!("Поддержка не приняла сообщение ({st})") } else { msg })
    }

    pub fn upload(&mut self, fp: &mut Fp, referer_id: &str, name: &str, mime: &str, bytes: Vec<u8>) -> Result<String, String> {
        self.ensure(fp)?;
        let referer = if referer_id.is_empty() { format!("{BASE}/tickets/new") } else { format!("{BASE}/tickets/{referer_id}") };
        let mut tries = 0;
        loop {
            tries += 1;
            let body = SBody::File { name: name.to_string(), mime: mime.to_string(), bytes: bytes.clone(), extra: vec![("csrf_token".into(), self.csrf.clone())] };
            let (st, text) = self.req(fp, true, &format!("{BASE}/attachment/upload"), body, true, &referer)?;
            if (st == 401 || st == 403) && tries == 1 {
                self.gk.clear();
                self.ensure(fp)?;
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if let Some(id) = v.pointer("/attachment/id") {
                return Ok(id.to_string().trim_matches('"').to_string());
            }
            let msg = v.get("error").and_then(|x| x.as_str()).unwrap_or("").to_string();
            return Err(if msg.is_empty() { format!("Файл не загрузился ({st})") } else { msg });
        }
    }

    pub fn close(&mut self, fp: &mut Fp, id: &str) -> Result<(), String> {
        let html = self.get(fp, &format!("{BASE}/tickets/{id}"))?;
        let doc = Html::parse_document(&html);
        let token = doc
            .select(&sel("body[data-app-config]"))
            .next()
            .and_then(|b| b.value().attr("data-app-config"))
            .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
            .and_then(|v| v.get("csrfToken").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .filter(|s| !s.is_empty())
            .or_else(|| doc.select(&sel("input[name='close_ticket[_token]']")).next().and_then(|e| e.value().attr("value").map(|s| s.to_string())))
            .ok_or("Не нашёл кнопку закрытия заявки.")?;
        let (st, _) = self.req(fp, true, &format!("{BASE}/tickets/{id}/close"), SBody::Form(vec![("csrf_token".into(), token)]), true, &format!("{BASE}/tickets/{id}"))?;
        if (200..300).contains(&st) {
            Ok(())
        } else {
            Err(format!("Заявка не закрылась ({st})"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_text() {
        assert_eq!(html_text("<p>Привет</p><p>Строка 1<br>Строка 2</p>"), "Привет\nСтрока 1\nСтрока 2");
        assert_eq!(to_html("a<b\n\nc\nd"), "<p>a&lt;b</p><p>c<br>d</p>");
    }

    #[test]
    fn parse_list_and_fields() {
        let doc = Html::parse_document(r#"<div class="mb-3" data-condition='{"type":"equals","fieldId":5,"value":2}'><label for="x" class="required">Ваш ник на FunPay *</label><input id="x" name="ticket[fields][7]" type="text"></div>"#);
        let el = doc.select(&sel("input")).next().unwrap();
        assert!(closest(el, "mb-3", "").is_some());
        assert_eq!(host_of("https://support.funpay.com/tickets"), "support.funpay.com");
    }

    /// Разбор настоящих страниц поддержки: FPSUP_DIR=папка с t1.html, a1.html, n1.html, d*.html, f*.html
    #[test]
    #[ignore]
    fn real_pages() {
        let Ok(dir) = std::env::var("FPSUP_DIR") else { return };
        let rd = |f: &str| std::fs::read_to_string(format!("{dir}/{f}")).unwrap_or_default();
        for f in ["n1.html", "a1.html", "l14.html"] {
            let (v, next, last) = parse_tickets(&rd(f));
            println!("== {f}: {} шт, дальше {next}, последняя {last}", v.len());
            for t in v {
                println!("  {} | {} | {} | {} | {}", t.id, t.status, t.active, t.date, t.title);
            }
        }
        for e in std::fs::read_dir(&dir).unwrap() {
            let name = e.unwrap().file_name().to_string_lossy().to_string();
            if let Some(id) = name.strip_prefix('d').and_then(|x| x.strip_suffix(".html")) {
                let d = parse_details(&rd(&name), id, "");
                println!("== {id}: {:?} / {:?} / создана {:?} / отв. {:?} / ответ {}", d.title, d.status, d.created, d.responsible, d.can_reply);
                for (k, v) in &d.fields {
                    println!("  поле {k}: {v}");
                }
                for c in &d.comments {
                    println!("  [{}] {} {} ав={} файлы={:?}\n    {:?}", c.time, c.author, c.mine, c.avatar, c.files.iter().map(|f| (&f.name, f.image)).collect::<Vec<_>>(), c.text.chars().take(120).collect::<String>());
                }
            }
            if name.starts_with('f') && name.ends_with(".html") {
                println!("== форма {name}");
                for f in parse_fields(&rd(&name), "nick") {
                    println!("  {} {} {:?} req={} cond={:?} def={:?} opts={:?}", f.kind, f.name, f.label.chars().take(60).collect::<String>(), f.required, f.cond, f.default, f.options);
                }
            }
        }
    }

    /// Живая проверка только на чтение: FPT_GK=golden_key
    #[test]
    #[ignore]
    fn live_read() {
        let Ok(gk) = std::env::var("FPT_GK") else { return };
        let mut fp = Fp::new(&gk, Default::default());
        let mut sup = Support::default();
        let p = sup.tickets(&mut fp).expect("список");
        println!("всего {} загружено {} дальше {:?} актуальных {}", p.total, p.items.len(), p.next, p.items.iter().filter(|t| t.active).count());
        for t in p.items.iter().take(6) {
            println!("  {} {} {} {}", t.id, t.status, t.active, t.title);
        }
        if let Some(n) = p.next {
            let m = sup.more(&mut fp, n, LIST_PAGES).expect("ещё");
            println!("ещё {} дальше {:?} всего {}", m.items.len(), m.next, m.total);
        }
        let open = p.items.iter().find(|t| t.status == "Открыта").map(|t| t.id.clone()).unwrap_or_else(|| p.items[0].id.clone());
        let d = sup.details(&mut fp, &open).expect("заявка");
        println!("заявка {} {:?} {:?} ответ {} сообщений {} свои {}", d.id, d.title, d.status, d.can_reply, d.comments.len(), d.comments.iter().filter(|c| c.mine).count());
        let cats = sup.categories(&mut fp).expect("разделы");
        println!("разделы {:?}", cats);
        let f = sup.fields(&mut fp, &cats[0].0, "nick").expect("поля");
        println!("полей {}", f.len());
    }
}
