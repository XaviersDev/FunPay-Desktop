//! Метка FunPay Tools: три невидимых символа в конце своего сообщения.
//! Порт fpt_identifier.js из расширения и FptMark.kt из приложения, посимвольно те же правила.

use once_cell::sync::Lazy;
use regex::Regex;

pub const SIGNATURE: &str = "\u{200B}\u{200D}\u{200C}";

static LINK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(https?://|www\.|ftp://)").unwrap());
static FUNPAY: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)funpay\.com").unwrap());
static WIN_PATH: Lazy<Regex> = Lazy::new(|| Regex::new(r"[A-Za-z]:\\").unwrap());
static UNIX_PATH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^/[a-z]").unwrap());
static ZERO_WIDTH: Lazy<Regex> = Lazy::new(|| Regex::new("[\u{200B}\u{200C}\u{200D}\u{FEFF}]").unwrap());
static SIG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(&format!(" ?{}", SIGNATURE)).unwrap());
const BLOCKED_FIRST: [char; 12] = ['/', '.', '!', '+', '№', '\\', '"', ':', '(', ')', '?', '#'];

pub fn should_inject(text: &str) -> bool {
    if text.trim().chars().count() < 4 {
        return false;
    }
    if LINK.is_match(text) || FUNPAY.is_match(text) {
        return false;
    }
    if WIN_PATH.is_match(text) || UNIX_PATH.is_match(text) {
        return false;
    }
    if ZERO_WIDTH.is_match(text) {
        return false;
    }
    match text.trim_start().chars().next() {
        Some(c) if BLOCKED_FIRST.contains(&c) => return false,
        None => return false,
        _ => {}
    }
    text.chars().filter(|c| c.is_ascii_alphabetic()).count() <= 24
}

pub fn sign(text: &str, enabled: bool) -> String {
    if !enabled || !should_inject(text) || text.ends_with(SIGNATURE) {
        return text.to_string();
    }
    if text.ends_with(' ') {
        format!("{text}{SIGNATURE}")
    } else {
        format!("{text} {SIGNATURE}")
    }
}

pub fn has(text: &str) -> bool {
    text.contains(SIGNATURE)
}

pub fn strip(text: &str) -> String {
    if !text.contains(SIGNATURE) {
        return text.to_string();
    }
    SIG_RE.replace_all(text, "").trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        assert!(should_inject("Привет, ключ выдал"));
        assert!(!should_inject("ok"));
        assert!(!should_inject("https://example.com"));
        assert!(!should_inject("смотри funpay.com/lots"));
        assert!(!should_inject("!позвать"));
        assert!(!should_inject("/start"));
        assert!(!should_inject("C:\\Users\\a"));
        assert!(!should_inject("ABCDE-FGHIJ-KLMNO-PQRST-UVWXY"));
        let s = sign("Готово, проверяйте", true);
        assert!(has(&s));
        assert_eq!(strip(&s), "Готово, проверяйте");
        assert_eq!(sign("Готово, проверяйте", false), "Готово, проверяйте");
        assert_eq!(sign(&s, true), s);
    }
}
