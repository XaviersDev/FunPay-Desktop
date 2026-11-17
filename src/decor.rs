//! Рамки для аватарок и баннеры профиля FunPay Tools, как в расширении.
//! Кто что носит: decor.json на GitHub сайта (его пишет воркер fpt-promo). Картинки: jsDelivr (копия папки decor с GitHub), чтобы не тратить лимит Vercel.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const BASE: &str = "https://cdn.jsdelivr.net/gh/XaviersDev/FunPayTools-Site@main/decor/";

/// (номер, название, файл)
pub const FRAMES: [(i32, &str, &str); 30] = [
    (1, "Красный неон", "frames/f01.webp"),
    (2, "Голубой неон", "frames/f02.webp"),
    (3, "Фиолетовый неон", "frames/f03.webp"),
    (4, "Оранжевый неон", "frames/f04.webp"),
    (5, "Неоновый градиент", "frames/f05.webp"),
    (6, "Серебряная орбита", "frames/f06.webp"),
    (7, "Солнечная вспышка", "frames/f07.webp"),
    (8, "Нимб", "frames/f08.webp"),
    (9, "Молния", "frames/f09.webp"),
    (10, "Чёрные розы", "frames/f10.webp"),
    (11, "Белые розы", "frames/f11.webp"),
    (12, "Фиолетовые розы", "frames/f12.webp"),
    (13, "Лисичка", "frames/f13.webp"),
    (14, "Чёрная кошка", "frames/f14.webp"),
    (15, "Белая кошка", "frames/f15.webp"),
    (16, "Бирюзовый вихрь", "frames/f16.webp"),
    (17, "Листва", "frames/f17.webp"),
    (18, "Полумесяц", "frames/f18.webp"),
    (19, "Крылья", "frames/f19.webp"),
    (20, "Клыки", "frames/f20.webp"),
    (21, "Бабочки", "frames/f21.webp"),
    (22, "Галактика", "frames/f22.webp"),
    (23, "Звёздная орбита", "frames/f23.webp"),
    (24, "Планета", "frames/f24.webp"),
    (25, "Комета", "frames/f25.webp"),
    (26, "Сакура", "frames/f26.webp"),
    (27, "Ветка с луной", "frames/f27.webp"),
    (28, "Талисман", "frames/f28.webp"),
    (29, "Лёд", "frames/f29.webp"),
    (30, "Пламя", "frames/f30.webp"),
];

/// (номер, название, файл, ширина к высоте)
pub const BANNERS: [(i32, &str, &str, f32); 27] = [
    (1, "Белые розы", "banners/b01.webp", 13.68),
    (2, "Красный фонарь", "banners/b02.webp", 13.30),
    (3, "Мотылёк", "banners/b03.webp", 13.49),
    (4, "Лесное озеро", "banners/b04.webp", 13.12),
    (5, "Ночное небо", "banners/b05.webp", 13.12),
    (6, "Чёрные розы", "banners/b06.webp", 13.12),
    (7, "Фиолетовая волна", "banners/b07.webp", 13.49),
    (8, "Красная сакура", "banners/b08.webp", 14.49),
    (9, "Светлячки", "banners/b09.webp", 14.49),
    (10, "Розовая сакура", "banners/b10.webp", 13.87),
    (11, "Пламя", "banners/b11.webp", 14.71),
    (12, "Витраж", "banners/b12.webp", 14.28),
    (14, "Паутина", "banners/b14.webp", 13.87),
    (15, "Неоновые лучи", "banners/b15.webp", 13.49),
    (16, "Бирюзовая сеть", "banners/b16.webp", 13.68),
    (17, "Кровавая луна", "banners/b17.webp", 13.04),
    (18, "Глаза", "banners/b18.webp", 13.19),
    (19, "Сердце", "banners/b19.webp", 13.69),
    (20, "Лунный замок", "banners/b20.webp", 14.60),
    (21, "Вороны", "banners/b21.webp", 15.87),
    (22, "Призрачная роза", "banners/b22.webp", 16.34),
    (23, "Бабочки", "banners/b23.webp", 16.34),
    (24, "Руки", "banners/b24.webp", 15.42),
    (25, "Снежный барс", "banners/b25.webp", 16.34),
    (26, "Алый взгляд", "banners/b26.webp", 15.64),
    (27, "Белый волк", "banners/b27.webp", 14.80),
    (28, "Когти", "banners/b28.webp", 13.52),
];

/// Кольцо каждой рамки (номер 1..30): доля аватарки от размера картинки и сдвиг центра. Замерено по картинкам.
pub const RING: [(f32, f32, f32); 30] = [
    (0.682, 0.023, 0.059),
    (0.674, 0.012, 0.055),
    (0.682, 0.004, 0.059),
    (0.682, 0.004, 0.059),
    (0.682, 0.004, 0.059),
    (0.728, -0.004, 0.043),
    (0.713, 0.027, 0.027),
    (0.728, 0.016, 0.016),
    (0.705, 0.004, 0.012),
    (0.713, 0.012, 0.012),
    (0.721, 0.008, 0.016),
    (0.713, -0.020, 0.004),
    (0.658, 0.012, 0.031),
    (0.674, 0.008, 0.031),
    (0.674, 0.000, 0.031),
    (0.682, -0.016, 0.008),
    (0.713, -0.004, 0.031),
    (0.680, 0.000, 0.030),
    (0.585, 0.000, -0.053),
    (0.658, -0.008, 0.008),
    (0.705, 0.004, 0.000),
    (0.666, 0.008, 0.012),
    (0.682, 0.016, 0.035),
    (0.705, -0.027, 0.016),
    (0.713, 0.027, -0.055),
    (0.713, 0.027, -0.035),
    (0.744, -0.012, -0.047),
    (0.760, -0.012, -0.055),
    (0.705, -0.020, -0.055),
    (0.705, 0.020, -0.051),
];

const URLS: [&str; 2] = [
    "https://raw.githubusercontent.com/XaviersDev/FunPayTools-Site/main/decor.json",
    "https://cdn.jsdelivr.net/gh/XaviersDev/FunPayTools-Site@main/decor.json",
];

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub n: String,
    pub f: i32,
    pub b: i32,
    pub fu: Option<i64>,
    pub bu: Option<i64>,
}

fn map() -> &'static Mutex<HashMap<String, Entry>> {
    static M: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Своё только что сохранённое (id, рамка, баннер, когда): видно сразу, пока decor.json не обновился
fn local() -> &'static Mutex<Option<(String, i32, i32, i64)>> {
    static L: OnceLock<Mutex<Option<(String, i32, i32, i64)>>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(None))
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn frame_url(id: i32) -> String {
    FRAMES.iter().find(|f| f.0 == id).map(|f| format!("{BASE}{}", f.2)).unwrap_or_default()
}

pub fn banner_url(id: i32) -> String {
    BANNERS.iter().find(|b| b.0 == id).map(|b| format!("{BASE}{}", b.2)).unwrap_or_default()
}

pub fn banner_ar(id: i32) -> f32 {
    BANNERS.iter().find(|b| b.0 == id).map(|b| b.3).unwrap_or(14.0)
}

fn cache_path() -> std::path::PathBuf {
    crate::config::dir().join("decor.json")
}

fn int(v: Option<&serde_json::Value>) -> Option<i64> {
    match v? {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|x| x as i64)),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn parse(body: &str) -> Option<HashMap<String, Entry>> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let o = v.as_object()?;
    let mut out = HashMap::new();
    for (k, e) in o {
        out.insert(
            k.clone(),
            Entry {
                n: e.get("n").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                f: int(e.get("f")).unwrap_or(0) as i32,
                b: int(e.get("b")).unwrap_or(0) as i32,
                fu: int(e.get("fu")),
                bu: int(e.get("bu")),
            },
        );
    }
    Some(out)
}

/// С диска сразу, потом из сети. on_ready зовётся после каждой удачной загрузки.
pub fn load(on_ready: impl Fn() + Send + 'static) {
    if let Ok(s) = std::fs::read_to_string(cache_path()) {
        if let Some(m) = parse(&s) {
            *map().lock().unwrap() = m;
            on_ready();
        }
    }
    std::thread::spawn(move || {
        let client = crate::fp::http_client_follow(20);
        for u in URLS {
            let url = format!("{u}?t={}", now_ms());
            let Ok(r) = client.get(&url).send() else { continue };
            if !r.status().is_success() {
                continue;
            }
            let Ok(body) = r.text() else { continue };
            let Some(m) = parse(&body) else { continue };
            let _ = std::fs::write(cache_path(), &body);
            *map().lock().unwrap() = m;
            on_ready();
            break;
        }
    });
}

/// Действующие рамка и баннер пользователя (0 если нет или срок вышел).
pub fn active(uid: &str) -> (i32, i32) {
    if uid.is_empty() {
        return (0, 0);
    }
    let now = now_ms();
    if let Some((id, f, b, at)) = local().lock().unwrap().clone() {
        if id == uid && now - at < 15 * 60 * 1000 {
            return (f, b);
        }
    }
    let m = map().lock().unwrap();
    let Some(e) = m.get(uid) else { return (0, 0) };
    let f = if e.f > 0 && e.fu.map(|t| t > now).unwrap_or(true) { e.f } else { 0 };
    let b = if e.b > 0 && e.bu.map(|t| t > now).unwrap_or(true) { e.b } else { 0 };
    (f, b)
}

pub fn set_local(uid: &str, f: i32, b: i32) {
    *local().lock().unwrap() = Some((uid.to_string(), f, b, now_ms()));
}

/// id на FunPay по нику (в списке чатов FunPay не отдаёт id собеседника, только ник).
pub fn uid_by_name(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let m = map().lock().unwrap();
    m.iter().find(|(_, e)| e.n.eq_ignore_ascii_case(name)).map(|(k, _)| k.clone()).unwrap_or_default()
}

/// Для демо и скриншотов.
pub fn demo_set(uid: &str, name: &str, f: i32, b: i32) {
    map().lock().unwrap().insert(uid.to_string(), Entry { n: name.to_string(), f, b, fu: None, bu: None });
}
