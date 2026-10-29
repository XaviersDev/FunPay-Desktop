//! Цветные ники донатеров, как в мобильном приложении (EpicNicksManager).
//! Список: funpay.tools/api/donaters/, запасной на GitHub. Ник → "FPT-STYLE-" + base64(JSON стиля).

use crate::NickStyle;
use base64::Engine;
use slint::Color;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, Default)]
pub struct Nick {
    pub c1: (u8, u8, u8),
    pub c2: (u8, u8, u8),
    pub c3: Option<(u8, u8, u8)>,
    pub ang: f32,
    pub spd: f32,
    pub an: Vec<String>,
    pub ov: String,
    pub pc: Option<(u8, u8, u8)>,
}

fn demo() -> &'static Mutex<Vec<(String, Nick)>> {
    static D: OnceLock<Mutex<Vec<(String, Nick)>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(Vec::new()))
}

fn set_map(mut m: HashMap<String, Nick>) {
    for (k, v) in demo().lock().unwrap().iter() {
        m.insert(k.clone(), v.clone());
    }
    *map().lock().unwrap() = m;
}

fn map() -> &'static Mutex<HashMap<String, Nick>> {
    static M: OnceLock<Mutex<HashMap<String, Nick>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

const URLS: [&str; 2] = [
    "https://funpay.tools/api/donaters/",
    "https://raw.githubusercontent.com/XaviersDev/FunPayTools-Site/main/donaters.json",
];

fn cache_path() -> std::path::PathBuf {
    crate::config::dir().join("donaters.json")
}

fn hex(s: &str) -> Option<(u8, u8, u8)> {
    crate::templates::parse_hex(s)
}

fn num(v: &serde_json::Value, d: f32) -> f32 {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(|x| x as f32).unwrap_or(d),
        serde_json::Value::String(s) => s.trim().parse().unwrap_or(d),
        _ => d,
    }
}

pub fn decode_style(s: &str) -> Option<Nick> {
    let b64 = s.strip_prefix("FPT-STYLE-")?;
    let raw = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let c1 = hex(v.get("c1")?.as_str()?)?;
    let c2 = v.get("c2").and_then(|x| x.as_str()).and_then(hex).unwrap_or(c1);
    Some(Nick {
        c1,
        c2,
        c3: v.get("c3").and_then(|x| x.as_str()).and_then(hex),
        ang: num(v.get("ang").unwrap_or(&serde_json::Value::Null), 90.0),
        spd: num(v.get("spd").unwrap_or(&serde_json::Value::Null), 3.0).clamp(0.4, 30.0),
        an: v.get("an").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        ov: v.get("ov").and_then(|x| x.as_str()).unwrap_or("none").to_string(),
        pc: v.get("pc").and_then(|x| x.as_str()).and_then(hex),
    })
}

fn parse_list(body: &str) -> HashMap<String, Nick> {
    let mut out = HashMap::new();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else { return out };
    if let Some(o) = v.as_object() {
        for (k, val) in o {
            if let Some(s) = val.as_str() {
                if let Some(n) = decode_style(s) {
                    out.insert(k.clone(), n);
                }
            }
        }
    }
    out
}

/// Сначала с диска (сразу при запуске), потом из сети.
pub fn load(on_ready: impl Fn() + Send + 'static) {
    if let Ok(s) = std::fs::read_to_string(cache_path()) {
        let m = parse_list(&s);
        if !m.is_empty() {
            set_map(m);
            on_ready();
        }
    }
    std::thread::spawn(move || {
        let client = crate::fp::http_client_follow(20);
        for u in URLS {
            let Ok(r) = client.get(u).send() else { continue };
            if !r.status().is_success() {
                continue;
            }
            let Ok(body) = r.text() else { continue };
            let m = parse_list(&body);
            if m.is_empty() {
                continue;
            }
            let _ = std::fs::write(cache_path(), &body);
            set_map(m);
            on_ready();
            break;
        }
    });
}

/// Для демо и скриншотов.
pub fn demo_insert(name: &str, style: &str) {
    if let Some(n) = decode_style(style) {
        demo().lock().unwrap().push((name.to_string(), n.clone()));
        map().lock().unwrap().insert(name.to_string(), n);
    }
}

pub fn get(name: &str) -> Option<Nick> {
    if name.is_empty() {
        return None;
    }
    map().lock().unwrap().get(name).cloned()
}

fn lum(c: (u8, u8, u8)) -> f32 {
    (0.2126 * c.0 as f32 + 0.7152 * c.1 as f32 + 0.0722 * c.2 as f32) / 255.0
}

/// На светлой теме слишком светлые цвета затемняем, иначе ник не прочитать (как в приложении).
fn adapt(c: (u8, u8, u8), dark: bool) -> Color {
    if dark {
        return Color::from_rgb_u8(c.0, c.1, c.2);
    }
    let l = lum(c);
    if l > 0.8 {
        return Color::from_rgb_u8(0x2E, 0x34, 0x40);
    }
    if l > 0.55 {
        let m = |x: u8| (x as f32 * 0.55) as u8;
        return Color::from_rgb_u8(m(c.0), m(c.1), m(c.2));
    }
    Color::from_rgb_u8(c.0, c.1, c.2)
}

pub fn to_style(n: &Nick, dark: bool) -> NickStyle {
    let ov = match n.ov.as_str() {
        "fire" => 1,
        "snow" => 2,
        "sparks" => 3,
        "matrix" => 4,
        "stars" => 5,
        "orbs" => 6,
        "lightning" => 7,
        _ => 0,
    };
    let c1 = adapt(n.c1, dark);
    let pc = n.pc.map(|p| Color::from_rgb_u8(p.0, p.1, p.2)).unwrap_or(c1);
    NickStyle {
        on: true,
        c1,
        c2: adapt(n.c2, dark),
        c3: n.c3.map(|c| adapt(c, dark)).unwrap_or(c1),
        ang: n.ang,
        spd: n.spd,
        glow: n.an.iter().any(|a| a == "glow"),
        pulse: n.an.iter().any(|a| a == "pulse"),
        wave: n.an.iter().any(|a| a == "wave"),
        glitch: n.an.iter().any(|a| a == "glitch"),
        ov,
        pc,
    }
}

pub fn style(name: &str, dark: bool) -> NickStyle {
    get(name).map(|n| to_style(&n, dark)).unwrap_or_default()
}

/// Примеры для окна «Цветной ник»: разные эффекты из настоящих стилей, но с нейтральными подписями.
pub fn examples(dark: bool) -> (Vec<NickStyle>, Vec<String>) {
    let m = map().lock().unwrap();
    let mut picked: Vec<Nick> = Vec::new();
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort();
    let want = ["glow", "wave", "pulse", "glitch"];
    for w in want {
        if let Some(k) = keys.iter().find(|k| {
            let n = &m[**k];
            n.an.iter().any(|a| a == w) && !picked.iter().any(|p| p.c1 == n.c1)
        }) {
            picked.push(m[*k].clone());
        }
    }
    // запасные стили, если список ещё не загрузился
    let fallback = [
        Nick { c1: (0x99, 0x40, 0xff), c2: (0xff, 0x00, 0xff), c3: Some((0x00, 0x66, 0xff)), ang: 90.0, spd: 4.0, an: vec!["glow".into(), "wave".into()], ov: "stars".into(), pc: None },
        Nick { c1: (0xff, 0xcc, 0x00), c2: (0xe0, 0x87, 0x0b), c3: None, ang: 45.0, spd: 3.0, an: vec!["wave".into()], ov: "fire".into(), pc: Some((0xff, 0x88, 0x00)) },
        Nick { c1: (0x00, 0xd5, 0xff), c2: (0x22, 0x77, 0xff), c3: None, ang: 120.0, spd: 2.0, an: vec!["pulse".into()], ov: "snow".into(), pc: Some((0xcf, 0xef, 0xff)) },
        Nick { c1: (0x2e, 0xe5, 0x9d), c2: (0x00, 0x99, 0x66), c3: None, ang: 0.0, spd: 1.0, an: vec!["glitch".into()], ov: "matrix".into(), pc: Some((0x2e, 0xe5, 0x9d)) },
    ];
    let mut i = 0;
    while picked.len() < 4 {
        picked.push(fallback[i].clone());
        i += 1;
    }
    let names = ["Ваш ник", "ProSeller", "Night_Shop", "Lucky777"].iter().map(|s| s.to_string()).collect();
    (picked.iter().take(4).map(|n| to_style(n, dark)).collect(), names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_string_numbers() {
        let s = "FPT-STYLE-eyJjMSI6IiM5OTQwZmYiLCJjMiI6IiNmZjAwZmYiLCJjMyI6IiMwMDAwZmYiLCJhbmciOiI5MCIsInNjbCI6IjI2NyIsInNwZCI6IjYiLCJhbiI6WyJnbG93Iiwid2F2ZSJdLCJvdiI6Im1hdHJpeCIsInBjIjoiI2ZmMDBmZiJ9";
        let n = decode_style(s).unwrap();
        assert_eq!(n.c1, (0x99, 0x40, 0xff));
        assert_eq!(n.ang, 90.0);
        assert_eq!(n.spd, 6.0);
        assert_eq!(n.ov, "matrix");
        assert!(n.an.contains(&"wave".to_string()));
    }
}
