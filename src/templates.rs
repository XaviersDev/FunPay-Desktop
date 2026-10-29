//! Шаблоны сообщений, бинды и команды автоответа.
//! Переменные как в мобильном приложении ($username, $lot_name...) и в расширении ({buyername}, {lotname}, {welcome}).

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub text: String,
    /// #RRGGBB или пусто (цвет темы)
    pub color: String,
    /// путь к картинке на этом ПК (копия в папке приложения)
    pub image: String,
    pub image_first: bool,
    /// 0 нет, 1..9 = Alt+1..Alt+9
    pub bind: i32,
}

/// Вид кнопок-шаблонов (как в настройках расширения).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Look {
    /// 0 над полем, 1 под полем, 2 в панели сверху, 3 в панели снизу, 4 меню у стикеров
    pub pos: i32,
    /// 0 скруглённые, 1 таблетки, 2 квадратные
    pub shape: i32,
    /// 0 маленькие, 1 средние, 2 большие
    pub size: i32,
    /// 0 сплошная, 1 мягкая, 2 контур, 3 призрак
    pub fill: i32,
    /// 0 слева, 1 по центру, 2 справа
    pub align: i32,
    pub full_width: bool,
    pub compact: bool,
    pub caps: bool,
    pub preview: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self { pos: 0, shape: 0, size: 1, fill: 1, align: 0, full_width: false, compact: false, caps: false, preview: true }
    }
}

/// Команда автоответа: покупатель пишет trigger, приложение отвечает response.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AutoCmd {
    pub id: String,
    pub trigger: String,
    pub response: String,
    pub exact: bool,
    pub case_sensitive: bool,
    pub enabled: bool,
}

impl Default for AutoCmd {
    fn default() -> Self {
        Self { id: String::new(), trigger: String::new(), response: String::new(), exact: true, case_sensitive: false, enabled: true }
    }
}

impl AutoCmd {
    pub fn matches(&self, msg: &str) -> bool {
        if !self.enabled || self.trigger.trim().is_empty() {
            return false;
        }
        let (m, t) = if self.case_sensitive {
            (msg.trim().to_string(), self.trigger.trim().to_string())
        } else {
            (msg.trim().to_lowercase(), self.trigger.trim().to_lowercase())
        };
        if self.exact {
            m == t
        } else {
            m.contains(&t)
        }
    }
}

/// Что известно о чате для подстановки.
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    pub username: String,
    pub chat_id: String,
    pub lot_name: String,
    pub message_text: String,
    pub order_id: String,
    pub order_title: String,
    pub order_desc: String,
    pub order_params: String,
    pub game: String,
    pub category: String,
}

const MONTHS: [&str; 12] = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"];

pub fn greeting(hour: u32) -> &'static str {
    match hour {
        5..=11 => "Доброе утро",
        12..=17 => "Добрый день",
        18..=23 => "Добрый вечер",
        _ => "Доброй ночи",
    }
}

/// Подставляет переменные. Сначала длинные ключи, чтобы $order_desc не съел $order_desc_and_params.
pub fn fill(text: &str, c: &Ctx) -> String {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    let date_text = format!("{} {}", now.day(), MONTHS[now.month0() as usize]);
    let full_date = format!("{} года", format!("{date_text} {}", now.year()));
    let desc_and_params = [c.order_desc.as_str(), c.order_params.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(", ");
    let desc_or_params = if !c.order_desc.is_empty() { c.order_desc.clone() } else { c.order_params.clone() };
    let order_link = if c.order_id.is_empty() { String::new() } else { format!("https://funpay.com/orders/{}/", c.order_id) };
    let lot = if !c.lot_name.is_empty() { c.lot_name.clone() } else { c.order_title.clone() };
    let category_full = [c.game.as_str(), c.category.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(", ");
    let welcome = greeting(now.hour());
    let mut pairs: Vec<(&str, String)> = vec![
        ("$order_desc_and_params", desc_and_params),
        ("$order_desc_or_params", desc_or_params),
        ("$category_fullname", category_full),
        ("$full_date_text", full_date.clone()),
        ("$full_date", full_date),
        ("$date_text", date_text),
        ("$full_time", now.format("%H:%M:%S").to_string()),
        ("$order_desc", c.order_desc.clone()),
        ("$order_title", c.order_title.clone()),
        ("$order_params", c.order_params.clone()),
        ("$order_link", order_link),
        ("$order_id", c.order_id.clone()),
        ("$lot_name", lot.clone()),
        ("$message_text", c.message_text.clone()),
        ("$chat_name", c.username.clone()),
        ("$chat_id", c.chat_id.clone()),
        ("$category", c.category.clone()),
        ("$username", c.username.clone()),
        ("$welcome", welcome.to_string()),
        ("$game", c.game.clone()),
        ("$date", now.format("%d.%m.%Y").to_string()),
        ("$time", now.format("%H:%M").to_string()),
        ("{buyername}", c.username.clone()),
        ("{username}", c.username.clone()),
        ("{lotname}", lot),
        ("{orderid}", c.order_id.clone()),
        ("{date}", now.format("%d.%m.%Y").to_string()),
        ("{time}", now.format("%H:%M").to_string()),
        ("{welcome}", format!("{welcome}!")),
    ];
    pairs.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    let mut out = text.to_string();
    for (k, v) in pairs {
        if out.contains(k) {
            out = out.replace(k, &v);
        }
    }
    out
}

pub fn new_id() -> String {
    format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0))
}

pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_first() {
        let c = Ctx { order_desc: "desc".into(), order_params: "p".into(), username: "Bob".into(), ..Default::default() };
        assert_eq!(fill("$order_desc_and_params|$order_desc|$username|{buyername}", &c), "desc, p|desc|Bob|Bob");
    }

    #[test]
    fn auto_match() {
        let a = AutoCmd { trigger: "!help".into(), response: "x".into(), ..Default::default() };
        assert!(a.matches(" !HELP "));
        assert!(!a.matches("!help me"));
        let b = AutoCmd { exact: false, ..a.clone() };
        assert!(b.matches("please !help me"));
    }
}
