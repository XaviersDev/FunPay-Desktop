//! Продажи, покупки и финансы FunPay (orders/trade, orders/, users/transactions).
//! Разбор повторяет мобильное приложение (FpFinance.kt, fetchSalesPage).

use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

fn sel(s: &str) -> Selector {
    Selector::parse(s).unwrap()
}

static S_TC_ITEM: Lazy<Selector> = Lazy::new(|| sel("a.tc-item"));
static S_ORDER: Lazy<Selector> = Lazy::new(|| sel(".tc-order"));
static S_DESC: Lazy<Selector> = Lazy::new(|| sel(".order-desc > div"));
static S_MUTED: Lazy<Selector> = Lazy::new(|| sel(".order-desc .text-muted"));
static S_PRICE: Lazy<Selector> = Lazy::new(|| sel(".tc-price"));
static S_UNAME: Lazy<Selector> = Lazy::new(|| sel(".media-user-name"));
static S_HREF: Lazy<Selector> = Lazy::new(|| sel(".media-user-name [data-href], .avatar-photo[data-href], [data-href]"));
static S_AV: Lazy<Selector> = Lazy::new(|| sel(".avatar-photo"));
static S_DATE: Lazy<Selector> = Lazy::new(|| sel(".tc-date-time"));
static S_STATUS: Lazy<Selector> = Lazy::new(|| sel(".tc-status"));
static S_CONTINUE: Lazy<Selector> = Lazy::new(|| sel("input[name=continue]"));
static S_TXN: Lazy<Selector> = Lazy::new(|| sel(".tc-item[data-transaction]"));
static S_TITLE: Lazy<Selector> = Lazy::new(|| sel(".tc-title"));
static S_PAYNUM: Lazy<Selector> = Lazy::new(|| sel(".tc-payment-number"));

static RE_ORDER_HREF: Lazy<Regex> = Lazy::new(|| Regex::new(r"/orders/([A-Z0-9]+)").unwrap());
static RE_USER: Lazy<Regex> = Lazy::new(|| Regex::new(r"/users/(\d+)").unwrap());
static RE_BG: Lazy<Regex> = Lazy::new(|| Regex::new(r#"url\(\s*['"]?([^'")]+)['"]?\s*\)"#).unwrap());
static RE_ORDER_ID: Lazy<Regex> = Lazy::new(|| Regex::new(r"#([A-Z0-9]{6,10})").unwrap());

fn text(el: ElementRef) -> String {
    el.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn first_text(el: ElementRef, s: &Selector) -> String {
    el.select(s).next().map(text).unwrap_or_default()
}

fn has_class(el: ElementRef, c: &str) -> bool {
    el.value().classes().any(|x| x == c)
}

/// Статус заказа в списке.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TradeState {
    #[default]
    Closed,
    Paid,
    Refunded,
}

#[derive(Debug, Clone, Default)]
pub struct TradeItem {
    pub id: String,
    pub state: TradeState,
    pub status_text: String,
    pub desc: String,
    pub game: String,
    pub price: String,
    pub peer: String,
    pub peer_id: String,
    pub peer_avatar: String,
    pub date: String,
}

/// Страница продаж или покупок: строки и токен следующей страницы.
pub fn parse_trade(html: &str) -> (Vec<TradeItem>, String) {
    let doc = Html::parse_document(html);
    let mut out = Vec::new();
    for row in doc.select(&S_TC_ITEM) {
        let href = row.value().attr("href").unwrap_or("");
        let mut id = RE_ORDER_HREF.captures(href).map(|c| c[1].to_string()).unwrap_or_default();
        if id.is_empty() {
            id = first_text(row, &S_ORDER).trim_start_matches('#').trim().to_string();
        }
        if id.is_empty() {
            continue;
        }
        let status_el = row.select(&S_STATUS).next();
        let status_text = status_el.map(text).unwrap_or_default();
        let state = if has_class(row, "warning") || status_text.contains("Возврат") || status_text.to_lowercase().contains("refund") {
            TradeState::Refunded
        } else if has_class(row, "info") || status_el.map(|e| has_class(e, "text-primary")).unwrap_or(false) {
            TradeState::Paid
        } else {
            TradeState::Closed
        };
        let desc = first_text(row, &S_DESC);
        let game = first_text(row, &S_MUTED);
        let desc = if !game.is_empty() && desc.ends_with(&game) { desc[..desc.len() - game.len()].trim().to_string() } else { desc };
        let mut peer_id = String::new();
        for e in row.select(&S_HREF) {
            if let Some(c) = e.value().attr("data-href").and_then(|h| RE_USER.captures(h)) {
                peer_id = c[1].to_string();
                break;
            }
        }
        let peer_avatar = row
            .select(&S_AV)
            .next()
            .and_then(|a| a.value().attr("style"))
            .and_then(|s| RE_BG.captures(s))
            .map(|c| crate::parse::abs_url(&c[1]))
            .unwrap_or_default();
        out.push(TradeItem {
            id,
            state,
            status_text: if status_text.is_empty() {
                match state {
                    TradeState::Paid => "Оплачен",
                    TradeState::Refunded => "Возврат",
                    TradeState::Closed => "Закрыт",
                }
                .into()
            } else {
                status_text
            },
            desc,
            game,
            price: first_text(row, &S_PRICE),
            peer: first_text(row, &S_UNAME),
            peer_id,
            peer_avatar,
            date: first_text(row, &S_DATE),
        });
    }
    let next = doc.select(&S_CONTINUE).next().and_then(|i| i.value().attr("value")).unwrap_or("").trim().to_string();
    (out, next)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TxnState {
    #[default]
    Complete,
    Waiting,
    Cancel,
}

#[derive(Debug, Clone, Default)]
pub struct Txn {
    pub id: String,
    pub title: String,
    pub state: TxnState,
    /// со знаком: приход > 0, расход < 0
    pub amount: f64,
    pub currency: String,
    pub amount_text: String,
    pub date: String,
    pub payment: String,
    pub order_id: String,
}

/// Сумма из «+ 1 234,50 ₽» или «− 300 ₽».
pub fn parse_amount(raw: &str) -> (f64, String) {
    let t = raw.replace('\u{a0}', " ");
    let t = t.trim();
    let neg = t.starts_with('−') || t.starts_with('-') || t.starts_with('–');
    let cur = if t.contains('₽') || t.to_lowercase().contains("руб") {
        "₽"
    } else if t.contains('$') {
        "$"
    } else if t.contains('€') {
        "€"
    } else {
        "₽"
    };
    let digits: String = t.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect::<String>().replace(',', ".");
    let norm = if digits.matches('.').count() > 1 {
        let (a, b) = digits.rsplit_once('.').unwrap();
        format!("{}.{}", a.replace('.', ""), b)
    } else {
        digits
    };
    let v: f64 = norm.parse().unwrap_or(0.0);
    (if neg { -v } else { v }, cur.to_string())
}

pub fn fmt_amount(v: f64, cur: &str) -> String {
    let abs = v.abs();
    let whole = abs.trunc() as u64;
    let frac = ((abs - abs.trunc()) * 100.0).round() as u64;
    let mut s = whole.to_string();
    let mut i = s.len() as isize - 3;
    while i > 0 {
        s.insert(i as usize, '\u{a0}');
        i -= 3;
    }
    if frac > 0 {
        s = format!("{s},{frac:02}");
    }
    format!("{}{s}\u{a0}{cur}", if v < 0.0 { "−" } else if v > 0.0 { "+" } else { "" })
}

static S_TXN_STATUS: Lazy<Selector> = Lazy::new(|| sel(".tc-status, [class*='status']"));

/// Статус операции: по классам строки и вложенных элементов, а если их нет, по тексту статуса.
/// FunPay помечает операции классами вида transaction-status-waiting / -cancel / -complete.
fn txn_state(el: ElementRef) -> TxnState {
    let mut classes: Vec<String> = el.value().classes().map(|c| c.to_lowercase()).collect();
    let mut texts = String::new();
    for e in el.select(&S_TXN_STATUS) {
        classes.extend(e.value().classes().map(|c| c.to_lowercase()));
        texts.push_str(&text(e).to_lowercase());
        texts.push(' ');
    }
    let has = |needle: &str| classes.iter().any(|c| c.contains("status") && c.contains(needle));
    if has("cancel") || has("refund") || texts.contains("отмен") || texts.contains("cancel") {
        TxnState::Cancel
    } else if has("wait") || has("pending") || has("process") || has("hold") || texts.contains("ожида") || texts.contains("обработ") || texts.contains("заморож") || texts.contains("waiting") || texts.contains("pending") {
        TxnState::Waiting
    } else {
        TxnState::Complete
    }
}

/// Страница «Финансов».
pub fn parse_transactions(html: &str) -> (Vec<Txn>, String) {
    let doc = Html::parse_document(html);
    let mut out = Vec::new();
    for el in doc.select(&S_TXN) {
        let id = el.value().attr("data-transaction").unwrap_or("").trim().to_string();
        if id.is_empty() {
            continue;
        }
        let state = txn_state(el);
        let title = first_text(el, &S_TITLE);
        let amount_text = first_text(el, &S_PRICE);
        let (amount, currency) = parse_amount(&amount_text);
        let order_id = RE_ORDER_ID.captures(&title).map(|c| c[1].to_string()).unwrap_or_default();
        out.push(Txn {
            id,
            title,
            state,
            amount,
            currency,
            amount_text,
            date: first_text(el, &S_DATE),
            payment: first_text(el, &S_PAYNUM),
            order_id,
        });
    }
    let next = doc.select(&S_CONTINUE).next().and_then(|i| i.value().attr("value")).unwrap_or("").trim().to_string();
    (out, next)
}

/// Сколько денег ещё в заморозке: приходы «в ожидании», по валютам.
pub fn frozen(list: &[Txn]) -> Vec<(String, f64)> {
    let mut by: Vec<(String, f64)> = Vec::new();
    for t in list.iter().filter(|t| t.state == TxnState::Waiting && t.amount > 0.0) {
        match by.iter_mut().find(|x| x.0 == t.currency) {
            Some(x) => x.1 += t.amount,
            None => by.push((t.currency.clone(), t.amount)),
        }
    }
    by
}

fn add(by: &mut Vec<(String, f64)>, cur: &str, v: f64) {
    match by.iter_mut().find(|x| x.0 == cur) {
        Some(x) => x.1 += v,
        None => by.push((cur.to_string(), v)),
    }
}

/// Деньги ещё не пролежали 48 часов: приходы за заказы за последние двое суток
/// (и всё, что FunPay сам пометил как ожидающее).
pub fn frozen_from_txns(list: &[Txn], now: chrono::NaiveDateTime) -> Vec<(String, f64)> {
    let mut by = Vec::new();
    for t in list.iter().filter(|t| t.amount > 0.0 && t.state != TxnState::Cancel) {
        let recent = crate::parse::parse_fp_date(&t.date, now).map(|d| now - d < chrono::Duration::hours(48)).unwrap_or(false);
        let is_order = !t.order_id.is_empty() || t.title.to_lowercase().contains("заказ");
        if t.state == TxnState::Waiting || (recent && is_order) {
            add(&mut by, &t.currency, t.amount);
        }
    }
    by
}

/// Запасной способ: закрытые продажи за последние 48 часов.
pub fn frozen_from_sales(list: &[TradeItem], now: chrono::NaiveDateTime) -> Vec<(String, f64)> {
    let mut by = Vec::new();
    for t in list.iter().filter(|t| t.state == TradeState::Closed) {
        let recent = crate::parse::parse_fp_date(&t.date, now).map(|d| now - d < chrono::Duration::hours(48)).unwrap_or(false);
        if recent {
            let (v, cur) = parse_amount(&t.price);
            if v > 0.0 {
                add(&mut by, &cur, v);
            }
        }
    }
    by
}

/// Берём большее из двух оценок по каждой валюте.
pub fn frozen_merge(a: Vec<(String, f64)>, b: Vec<(String, f64)>) -> Vec<(String, f64)> {
    let mut out = a;
    for (c, v) in b {
        match out.iter_mut().find(|x| x.0 == c) {
            Some(x) => x.1 = x.1.max(v),
            None => out.push((c, v)),
        }
    }
    out
}

pub fn frozen_text(v: &[(String, f64)]) -> String {
    v.iter().filter(|(_, x)| *x > 0.0).map(|(c, x)| fmt_amount(*x, c).trim_start_matches('+').to_string()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod frozen_tests {
    use super::*;

    #[test]
    fn last_48_hours() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 29).unwrap().and_hms_opt(20, 0, 0).unwrap();
        let t = |title: &str, date: &str, amount: f64| Txn { title: title.into(), date: date.into(), amount, currency: "₽".into(), ..Default::default() };
        let v = vec![t("Заказ #A", "сегодня, 14:31", 100.0), t("Заказ #B", "вчера, 21:00", 50.0), t("Заказ #C", "25 октября, 12:00", 900.0), t("Вывод денег", "сегодня, 10:00", -30.0)];
        assert_eq!(frozen_from_txns(&v, now), vec![("₽".to_string(), 150.0)]);
        let s = vec![TradeItem { state: TradeState::Closed, price: "290,94 ₽".into(), date: "сегодня, 11:00".into(), ..Default::default() }];
        assert_eq!(frozen_text(&frozen_merge(frozen_from_txns(&v, now), frozen_from_sales(&s, now))), "290,94\u{a0}₽");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRADE: &str = r#"<div class="tc">
<a href="https://funpay.com/orders/F88412AB/" class="tc-item info">
  <div class="tc-date"><div class="tc-date-time">сегодня, 14:30</div><div class="tc-date-left">5 минут назад</div></div>
  <div class="tc-order">#F88412AB</div>
  <div class="order-desc"><div>Steam-ключ Hollow Knight, 1 шт.</div><div class="text-muted">Hollow Knight, Ключи</div></div>
  <div class="tc-user"><div class="media media-user"><div class="media-left"><div class="avatar-photo pseudo-a" data-href="https://funpay.com/users/5512/" style="background-image: url(/img/layout/avatar.png);"></div></div>
  <div class="media-body"><div class="media-user-name"><span class="pseudo-a" data-href="https://funpay.com/users/5512/">darkness21</span></div></div></div></div>
  <div class="tc-status text-primary">Оплачен</div>
  <div class="tc-price text-nowrap tc-seller-sum">349 <span class="unit">₽</span></div>
</a>
<a href="https://funpay.com/orders/AB12CD34/" class="tc-item warning">
  <div class="tc-date-time">вчера, 10:15</div>
  <div class="order-desc"><div>Буст MMR</div><div class="text-muted">Dota 2, Буст</div></div>
  <div class="tc-status">Возврат</div>
  <div class="tc-price">1 200 ₽</div>
</a>
<a href="https://funpay.com/orders/ZZ99YY88/" class="tc-item">
  <div class="order-desc"><div>Аккаунт</div></div>
  <div class="tc-status text-success">Закрыт</div>
  <div class="tc-price">90 ₽</div>
</a>
<input type="hidden" name="continue" value="tok123">
</div>"#;

    #[test]
    fn trade_rows() {
        let (v, next) = parse_trade(TRADE);
        assert_eq!(next, "tok123");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].id, "F88412AB");
        assert_eq!(v[0].state, TradeState::Paid);
        assert_eq!(v[0].desc, "Steam-ключ Hollow Knight, 1 шт.");
        assert_eq!(v[0].game, "Hollow Knight, Ключи");
        assert_eq!(v[0].peer, "darkness21");
        assert_eq!(v[0].peer_id, "5512");
        assert_eq!(v[0].price, "349 ₽");
        assert_eq!(v[1].state, TradeState::Refunded);
        assert_eq!(v[2].state, TradeState::Closed);
    }

    const TXN: &str = r#"<div>
<div class="tc-item transaction-status-waiting" data-transaction="901">
  <div class="tc-date-time">сегодня, 14:31</div><div class="tc-title">Заказ #F88412AB</div><div class="tc-price">+ 349 ₽</div></div>
<div class="tc-item transaction-status-complete" data-transaction="900">
  <div class="tc-date-time">вчера, 9:00</div><div class="tc-title">Вывод денег</div>
  <div class="tc-payment"><span class="tc-payment-number">Карта *1234</span></div><div class="tc-price">− 1 500,50 ₽</div></div>
<div class="tc-item transaction-status-cancel" data-transaction="899">
  <div class="tc-title">Пополнение баланса</div><div class="tc-price">+ 10 $</div></div>
<input type="hidden" name="continue" value="">
</div>"#;

    #[test]
    fn transaction_states_by_text() {
        let html = r#"<div class="tc-item" data-transaction="1"><div class="tc-title">Заказ #AAAA1111</div><div class="tc-status text-warning">Ожидание</div><div class="tc-price">+ 100 ₽</div></div>
<div class="tc-item" data-transaction="2"><div class="tc-title">Вывод денег</div><div class="tc-status">Отменено</div><div class="tc-price">− 50 ₽</div></div>
<div class="tc-item transaction-status-pending" data-transaction="3"><div class="tc-title">Заказ #BBBB2222</div><div class="tc-price">+ 20 ₽</div></div>"#;
        let (v, _) = parse_transactions(html);
        assert_eq!(v[0].state, TxnState::Waiting);
        assert_eq!(v[1].state, TxnState::Cancel);
        assert_eq!(v[2].state, TxnState::Waiting);
        assert_eq!(frozen(&v), vec![("₽".to_string(), 120.0)]);
    }

    #[test]
    fn transactions() {
        let (v, next) = parse_transactions(TXN);
        assert_eq!(next, "");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].state, TxnState::Waiting);
        assert_eq!(v[0].order_id, "F88412AB");
        assert!((v[0].amount - 349.0).abs() < 1e-9);
        assert!((v[1].amount + 1500.5).abs() < 1e-9, "{}", v[1].amount);
        assert_eq!(v[1].payment, "Карта *1234");
        assert_eq!(v[2].currency, "$");
        assert_eq!(v[2].state, TxnState::Cancel);
        assert_eq!(frozen(&v), vec![("₽".to_string(), 349.0)]);
        assert_eq!(fmt_amount(-1500.5, "₽"), "−1\u{a0}500,50\u{a0}₽");
        assert_eq!(fmt_amount(12480.0, "₽"), "+12\u{a0}480\u{a0}₽");
    }
}

/// Для журнала: какие классы и статусы у строк «Финансов» (без сумм и названий).
pub fn txn_markup_summary(html: &str) -> String {
    let doc = Html::parse_document(html);
    let mut seen: Vec<String> = Vec::new();
    for el in doc.select(&S_TXN).take(30) {
        let cls: Vec<&str> = el.value().classes().collect();
        let st = el.select(&S_TXN_STATUS).next().map(|e| {
            let c: Vec<&str> = e.value().classes().collect();
            format!("{}:{}", c.join("."), text(e))
        });
        let line = format!("[{}] {}", cls.join("."), st.unwrap_or_default());
        if !seen.contains(&line) {
            seen.push(line);
        }
    }
    seen.join(" | ")
}
