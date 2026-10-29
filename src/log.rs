//! Журнал ошибок в %APPDATA%\FunPayTools\log.txt.
//! Пишем только служебные события (ответы FunPay на отправку, загрузку и т.п.).
//! Ключи, куки и токены сюда не попадают: перед записью они вырезаются.

use std::io::Write;

const MAX: u64 = 512 * 1024;

pub fn path() -> std::path::PathBuf {
    crate::config::dir().join("log.txt")
}

/// Убирает из строки всё, что похоже на секреты.
fn scrub(s: &str) -> String {
    static BEARER: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"(?i)bearer\s+[A-Za-z0-9._\-]+").unwrap());
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r#"(?i)(golden_key|phpsessid|golden_seal|csrf_token|session|authorization)([=:"\s]+)[A-Za-z0-9._\-]+"#).unwrap()
    });
    let s = BEARER.replace_all(s, "Bearer ***");
    RE.replace_all(&s, "$1$2***").to_string()
}

pub fn write(line: &str) {
    let p = path();
    if std::fs::metadata(&p).map(|m| m.len() > MAX).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_extension("old.txt"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let t = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "{t} {}", scrub(line));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scrub_hides_secrets() {
        let s = super::scrub("golden_key=abc123def; PHPSESSID=zzz csrf_token: \"qq11\" Authorization: Bearer tok.en");
        assert!(!s.contains("abc123def") && !s.contains("zzz") && !s.contains("qq11") && !s.contains("tok.en"), "{s}");
    }
}
