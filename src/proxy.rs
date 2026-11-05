//! Прокси для аккаунтов: у каждого сохранённого аккаунта свой.
//! Строгий режим: без рабочего прокси аккаунт не запускается и запросы напрямую не идут.

use crate::config::Config;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

static CFG: OnceLock<Arc<Mutex<Config>>> = OnceLock::new();

pub fn init(cfg: Arc<Mutex<Config>>) {
    let _ = CFG.set(cfg);
}

/// Прокси аккаунта с этим golden_key: (адрес, строгий режим).
pub fn for_key(gk: &str) -> Option<(String, bool)> {
    let c = CFG.get()?.lock().ok()?;
    let a = c.accounts.iter().find(|a| a.golden_key == gk && !a.proxy.is_empty())?;
    Some((a.proxy.clone(), a.proxy_strict))
}

pub const KINDS: [&str; 4] = ["http", "https", "socks5", "socks4"];

/// Тип прокси из адреса (индекс в KINDS).
pub fn kind_of(url: &str) -> usize {
    let s = url.to_ascii_lowercase();
    if s.starts_with("socks4") {
        3
    } else if s.starts_with("socks5") {
        2
    } else if s.starts_with("https://") {
        1
    } else {
        0
    }
}

/// Адрес без схемы, как его удобно показывать в поле.
pub fn bare(url: &str) -> String {
    match url.split_once("://") {
        Some((_, rest)) => rest.to_string(),
        None => url.to_string(),
    }
}

fn is_port(s: &str) -> bool {
    !s.is_empty() && s.len() <= 5 && s.chars().all(|c| c.is_ascii_digit()) && s.parse::<u32>().map(|p| p > 0 && p < 65536).unwrap_or(false)
}

/// Приводит любую привычную запись к виду scheme://user:pass@host:port.
/// Понимает host:port, host:port:user:pass, user:pass:host:port, user:pass@host:port, host:port@user:pass
/// и полные адреса со схемой (тогда схема из адреса важнее выбранной).
pub fn normalize(kind: usize, raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err("Впишите адрес прокси".into());
    }
    let (mut scheme, rest) = match raw.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r.to_string()),
        None => (KINDS[kind.min(3)].to_string(), raw.to_string()),
    };
    // socks5 без h резолвит домены на этом ПК: адрес FunPay должен узнавать прокси, а не провайдер
    if scheme == "socks5" {
        scheme = "socks5h".into();
    }
    if scheme == "socks4" {
        scheme = "socks4a".into();
    }
    if !["http", "https", "socks5h", "socks4a"].contains(&scheme.as_str()) {
        return Err(format!("Тип {scheme} не поддерживается, подойдут HTTP, HTTPS, SOCKS5 и SOCKS4"));
    }
    let (auth, host) = if let Some((a, b)) = rest.rsplit_once('@') {
        // user:pass@host:port или host:port@user:pass
        let b_is_host = b.rsplit_once(':').map(|(_, p)| is_port(p)).unwrap_or(false);
        if b_is_host {
            (Some(a.to_string()), b.to_string())
        } else {
            (Some(b.to_string()), a.to_string())
        }
    } else {
        let parts: Vec<&str> = rest.split(':').collect();
        match parts.len() {
            2 => (None, rest.clone()),
            4 if is_port(parts[1]) => (Some(format!("{}:{}", parts[2], parts[3])), format!("{}:{}", parts[0], parts[1])),
            4 if is_port(parts[3]) => (Some(format!("{}:{}", parts[0], parts[1])), format!("{}:{}", parts[2], parts[3])),
            _ => return Err("Не понял адрес. Пример: 1.2.3.4:8080 или 1.2.3.4:8080:логин:пароль".into()),
        }
    };
    let Some((h, p)) = host.rsplit_once(':') else {
        return Err("Не хватает порта: адрес:порт".into());
    };
    if h.is_empty() || h.contains(' ') || !is_port(p) {
        return Err("Не понял адрес или порт прокси".into());
    }
    let auth = auth.filter(|a| !a.is_empty() && a != ":");
    let url = match auth {
        Some(a) => {
            let (u, pw) = a.split_once(':').unwrap_or((a.as_str(), ""));
            format!("{scheme}://{}:{}@{h}:{p}", enc(u), enc(pw))
        }
        None => format!("{scheme}://{h}:{p}"),
    };
    reqwest::Proxy::all(&url).map_err(|e| format!("Прокси не принят: {e}"))?;
    Ok(url)
}

fn enc(s: &str) -> String {
    // логин и пароль могли быть уже закодированы
    let s = dec(s);
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Проверка: открывается ли FunPay через прокси. Ответ: время ответа и внешний IP, если узнали.
pub fn check(url: &str) -> Result<String, String> {
    let c = crate::fp::client_with(12, false, Some(url))?;
    let t = Instant::now();
    let r = c.get("https://funpay.com/").send().map_err(|e| human(&e))?;
    let ms = t.elapsed().as_millis();
    if r.status().as_u16() >= 500 || r.status().as_u16() == 407 {
        return Err(if r.status().as_u16() == 407 { "Прокси просит логин и пароль".into() } else { format!("FunPay через прокси ответил {}", r.status()) });
    }
    let ip = c
        .get("https://api.ipify.org")
        .timeout(Duration::from_secs(6))
        .send()
        .ok()
        .and_then(|r| r.text().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() < 46 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '.' || c == ':'));
    Ok(match ip {
        Some(ip) => format!("Работает: IP {ip}, ответ за {ms} мс"),
        None => format!("Работает: FunPay ответил за {ms} мс"),
    })
}

fn human(e: &reqwest::Error) -> String {
    let s = format!("{e:?}").to_lowercase();
    if e.is_timeout() {
        "Прокси не ответил вовремя".into()
    } else if s.contains("407") || s.contains("auth") {
        "Прокси не принял логин или пароль".into()
    } else if s.contains("refused") {
        "Прокси отказал в подключении".into()
    } else if s.contains("dns") || s.contains("resolve") {
        "Не нашёл адрес прокси".into()
    } else {
        "Прокси не отвечает".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats() {
        assert_eq!(normalize(0, "1.2.3.4:8080").unwrap(), "http://1.2.3.4:8080");
        assert_eq!(normalize(2, "1.2.3.4:1080:user:pa ss").unwrap(), "socks5h://user:pa%20ss@1.2.3.4:1080");
        assert_eq!(normalize(0, "user:pass@host.net:3128").unwrap(), "http://user:pass@host.net:3128");
        assert_eq!(normalize(0, "host.net:3128@user:pass").unwrap(), "http://user:pass@host.net:3128");
        assert_eq!(normalize(0, "user:pass:1.2.3.4:3128").unwrap(), "http://user:pass@1.2.3.4:3128");
        assert_eq!(normalize(0, "socks5://u:p@1.2.3.4:1080").unwrap(), "socks5h://u:p@1.2.3.4:1080");
        assert_eq!(normalize(3, "1.2.3.4:1080").unwrap(), "socks4a://1.2.3.4:1080");
        assert!(normalize(0, "1.2.3.4").is_err());
        assert!(normalize(0, "").is_err());
        assert_eq!(kind_of("socks5h://x:1"), 2);
        assert_eq!(bare("http://u:p@h:1"), "u:p@h:1");
    }
}
