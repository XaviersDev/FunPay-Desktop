//! Импорт тем Telegram: .tdesktop-theme (zip с colors.tdesktop-theme и обоями),
//! .tdesktop-palette (только цвета) и .attheme (Android, цвета + картинка после WPS).
//! Цвета темы раскладываются по палитре приложения; чего нет в теме, выводим из основных.

use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    fn f(self) -> [f32; 4] {
        [self.0 as f32, self.1 as f32, self.2 as f32, self.3 as f32]
    }
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}{:02X}", self.0, self.1, self.2, self.3)
    }
    pub fn parse_hex(s: &str) -> Option<Rgba> {
        let h = s.trim().trim_start_matches('#');
        let b = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            6 => Some(Rgba(b(0)?, b(2)?, b(4)?, 255)),
            8 => Some(Rgba(b(0)?, b(2)?, b(4)?, b(6)?)),
            3 => {
                let c = |i: usize| u8::from_str_radix(&h[i..i + 1].repeat(2), 16).ok();
                Some(Rgba(c(0)?, c(1)?, c(2)?, 255))
            }
            _ => None,
        }
    }
    fn lum(self) -> f32 {
        (0.2126 * self.0 as f32 + 0.7152 * self.1 as f32 + 0.0722 * self.2 as f32) / 255.0
    }
    fn alpha(self, a: f32) -> Rgba {
        Rgba(self.0, self.1, self.2, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
    /// Положить полупрозрачный цвет на непрозрачный фон.
    fn over(self, bg: Rgba) -> Rgba {
        let a = self.3 as f32 / 255.0;
        let m = |x: u8, y: u8| (x as f32 * a + y as f32 * (1.0 - a)).round() as u8;
        Rgba(m(self.0, bg.0), m(self.1, bg.1), m(self.2, bg.2), 255)
    }
}

/// Контраст по WCAG (1..21).
fn contrast(a: Rgba, b: Rgba) -> f32 {
    fn ch(c: u8) -> f32 {
        let c = c as f32 / 255.0;
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }
    let l = |c: Rgba| 0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2);
    let (x, y) = (l(a), l(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Отодвигает цвет от фона, пока контраст не станет не меньше нужного.
fn ensure_contrast(c: Rgba, bg: Rgba, min: f32) -> Rgba {
    if contrast(c, bg) >= min {
        return c;
    }
    let target = if bg.lum() < 0.5 { WHITE } else { BLACK };
    let mut t = 0.08;
    while t <= 1.0 {
        let m = mix(c, target, t);
        if contrast(m, bg) >= min {
            return m;
        }
        t += 0.08;
    }
    target
}

/// Чёрный или белый: что читается лучше поверх цвета.
fn readable_on(bg: Rgba) -> Rgba {
    if contrast(WHITE, bg) >= contrast(Rgba(0x10, 0x14, 0x1A, 255), bg) { WHITE } else { Rgba(0x10, 0x14, 0x1A, 255) }
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let (x, y) = (a.f(), b.f());
    let m = |i: usize| (x[i] + (y[i] - x[i]) * t).round().clamp(0.0, 255.0) as u8;
    Rgba(m(0), m(1), m(2), m(3))
}

const WHITE: Rgba = Rgba(255, 255, 255, 255);
const BLACK: Rgba = Rgba(0, 0, 0, 255);

#[derive(Debug, Default)]
pub struct Loaded {
    pub name: String,
    pub dark: bool,
    pub colors: BTreeMap<String, String>,
    /// картинка обоев: байты, расширение, плиткой ли
    pub wall: Option<(Vec<u8>, String, bool)>,
    pub fx: Fx,
}

/// Эффекты темы: прозрачность сообщений, матовое стекло, обработка гифки на фоне.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fx {
    pub msg_alpha: f32,
    pub glass: bool,
    pub glass_blur: f32,
    /// для анимированного фона размытие и яркость применяются при показе
    pub wall_blur: f32,
    pub wall_bright: f32,
}

impl Default for Fx {
    fn default() -> Self {
        Self { msg_alpha: 1.0, glass: false, glass_blur: 10.0, wall_blur: 0.0, wall_bright: 100.0 }
    }
}

// ------------------------------------------------------------ Telegram Desktop

/// Текст colors.tdesktop-theme: «ключ: #rrggbb[aa];» или «ключ: другойКлюч;», комментарии // и /* */.
pub fn parse_td_text(text: &str) -> HashMap<String, String> {
    let mut clean = String::with_capacity(text.len());
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '/' && it.peek() == Some(&'/') {
            for c2 in it.by_ref() {
                if c2 == '\n' {
                    clean.push('\n');
                    break;
                }
            }
        } else if c == '/' && it.peek() == Some(&'*') {
            it.next();
            let mut prev = ' ';
            for c2 in it.by_ref() {
                if prev == '*' && c2 == '/' {
                    break;
                }
                prev = c2;
            }
        } else {
            clean.push(c);
        }
    }
    let mut map = HashMap::new();
    for stmt in clean.split(|c| c == ';' || c == '\n') {
        if let Some((k, v)) = stmt.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if !k.is_empty() && !v.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                map.insert(k.to_string(), v.to_string());
            }
        }
    }
    map
}

fn td_resolve(map: &HashMap<String, String>, key: &str) -> Option<Rgba> {
    let mut cur = map.get(key)?.clone();
    for _ in 0..24 {
        if cur.starts_with('#') {
            return Rgba::parse_hex(&cur);
        }
        cur = map.get(cur.as_str())?.clone();
    }
    None
}

fn load_td_zip(bytes: &[u8]) -> Result<(HashMap<String, String>, Option<(Vec<u8>, String, bool)>), String> {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "Файл темы повреждён".to_string())?;
    let mut text = None;
    let mut wall = None;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|_| "Файл темы повреждён".to_string())?;
        let name = f.name().to_lowercase();
        let base = name.rsplit('/').next().unwrap_or(&name).to_string();
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            continue;
        }
        if base.ends_with(".tdesktop-theme") || base == "colors.tdesktop-theme" || base.ends_with(".tdesktop-palette") {
            text = Some(String::from_utf8_lossy(&buf).to_string());
        } else if base.starts_with("background.") || base.starts_with("tiled.") {
            let ext = base.rsplit('.').next().unwrap_or("jpg").to_string();
            wall = Some((buf, ext, base.starts_with("tiled.")));
        }
    }
    let text = text.ok_or("В архиве нет файла с цветами темы")?;
    Ok((parse_td_text(&text), wall))
}

// ------------------------------------------------------------ Android .attheme

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Значение .attheme: десятичный ARGB (может быть отрицательным) или #AARRGGBB / #RRGGBB.
fn at_color(v: &str) -> Option<Rgba> {
    let v = v.trim();
    if let Some(h) = v.strip_prefix('#') {
        return match h.len() {
            8 => {
                let n = u32::from_str_radix(h, 16).ok()?;
                Some(Rgba((n >> 16) as u8, (n >> 8) as u8, n as u8, (n >> 24) as u8))
            }
            6 => Rgba::parse_hex(h),
            _ => None,
        };
    }
    let n = v.parse::<i64>().ok()? as u32;
    Some(Rgba((n >> 16) as u8, (n >> 8) as u8, n as u8, (n >> 24) as u8))
}

pub fn parse_attheme(bytes: &[u8]) -> (HashMap<String, Rgba>, Option<(Vec<u8>, String, bool)>) {
    let (text_part, wall) = match find(bytes, b"WPS\n") {
        Some(i) => {
            let rest = &bytes[i + 4..];
            let end = find(rest, b"\nWPE").unwrap_or(rest.len());
            let img = rest[..end].to_vec();
            let ext = if img.starts_with(&[0x89, b'P', b'N', b'G']) { "png" } else { "jpg" }.to_string();
            (&bytes[..i], if img.len() > 64 { Some((img, ext, false)) } else { None })
        }
        None => (bytes, None),
    };
    let text = String::from_utf8_lossy(text_part);
    let mut map = HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            if let Some(c) = at_color(v) {
                map.insert(k.trim().to_string(), c);
            }
        }
    }
    (map, wall)
}

// ------------------------------------------------------------ сборка палитры

/// Раскладывает цвета темы по палитре приложения. `get` ищет первый найденный ключ темы.
fn build(get: &dyn Fn(&[&str]) -> Option<Rgba>, android: bool) -> (bool, BTreeMap<String, String>) {
    let panel = get(if android { &["windowBackgroundWhite"] } else { &["windowBg"] }).unwrap_or(WHITE).over(WHITE);
    let dark = panel.lum() < 0.45;
    let ink = get(if android { &["windowBackgroundWhiteBlackText", "chats_name"] } else { &["windowFg"] })
        .unwrap_or(if dark { WHITE } else { BLACK })
        .over(panel);
    let muted = get(if android { &["windowBackgroundWhiteGrayText2", "windowBackgroundWhiteGrayText", "chats_message"] } else { &["windowSubTextFg"] })
        .map(|c| c.over(panel))
        .unwrap_or_else(|| mix(ink, panel, 0.45));
    let accent = get(if android {
        &["featuredStickers_addButton", "chats_actionBackground", "chats_unreadCounter", "actionBarDefault"]
    } else {
        &["activeButtonBg", "windowBgActive", "dialogsBgActive"]
    })
    .map(|c| c.over(panel))
    .unwrap_or(Rgba(0x3A, 0x7F, 0xD5, 255));
    let field = get(if android { &["windowBackgroundGray"] } else { &["filterInputInactiveBg", "searchedBarBg"] })
        .map(|c| c.over(panel))
        .filter(|c| (c.lum() - panel.lum()).abs() > 0.01)
        .unwrap_or_else(|| mix(panel, ink, if dark { 0.07 } else { 0.05 }));
    let line = get(if android { &["divider"] } else { &["shadowFg", "dialogsRippleBg"] })
        .map(|c| c.over(panel))
        .filter(|c| (c.lum() - panel.lum()).abs() > 0.02)
        .unwrap_or_else(|| mix(panel, ink, 0.1));
    let sel = get(if android { &[] } else { &["dialogsBgActive"] }).map(|c| c.over(panel)).unwrap_or(accent);
    let hover = get(if android { &["listSelectorSDK21"] } else { &["dialogsBgOver"] })
        .map(|c| c.over(panel))
        .filter(|c| (c.lum() - panel.lum()).abs() > 0.008)
        .unwrap_or_else(|| mix(panel, ink, 0.05));
    let rail = get(if android { &["actionBarDefault"] } else { &["sideBarBg", "dialogsBg"] })
        .map(|c| c.over(panel))
        .map(|c| if (c.lum() - panel.lum()).abs() < 0.04 { mix(c, BLACK, if dark { 0.25 } else { 0.55 }) } else { c })
        .unwrap_or_else(|| if dark { mix(panel, BLACK, 0.3) } else { mix(accent, BLACK, 0.45) });
    let rail_dark = rail.lum() < 0.5;
    let rail_tx = get(if android { &["actionBarDefaultIcon"] } else { &["sideBarIconFg", "sideBarTextFg"] })
        .map(|c| c.over(rail))
        .map(|c| mix(c, rail, 0.25))
        .unwrap_or_else(|| mix(if rail_dark { WHITE } else { BLACK }, rail, 0.42));
    let rail_on = get(if android { &[] } else { &["sideBarBgActive"] })
        .map(|c| c.over(rail))
        .unwrap_or_else(|| mix(rail, if rail_dark { WHITE } else { BLACK }, 0.1));
    let bubble_in = get(if android { &["chat_inBubble"] } else { &["msgInBg"] }).map(|c| c.over(panel)).unwrap_or(panel);
    let bubble_out = get(if android { &["chat_outBubble"] } else { &["msgOutBg"] })
        .map(|c| c.over(panel))
        .unwrap_or_else(|| mix(panel, accent, 0.22));
    let text_in = get(if android { &["chat_messageTextIn"] } else { &["historyTextInFg"] }).map(|c| c.over(bubble_in)).unwrap_or(ink);
    let text_out = get(if android { &["chat_messageTextOut"] } else { &["historyTextOutFg"] }).map(|c| c.over(bubble_out)).unwrap_or(ink);
    let meta_in = get(if android { &["chat_inTimeText"] } else { &["msgInDateFg"] })
        .map(|c| c.over(bubble_in))
        .unwrap_or_else(|| mix(text_in, bubble_in, 0.45));
    let meta_out = get(if android { &["chat_outTimeText"] } else { &["msgOutDateFg"] })
        .map(|c| c.over(bubble_out))
        .unwrap_or_else(|| mix(text_out, bubble_out, 0.45));
    let check = get(if android { &["chat_outSentCheck", "chat_outSentCheckRead"] } else { &["historyOutIconFg", "msgOutDateFgSelected"] })
        .map(|c| c.over(bubble_out))
        .unwrap_or(accent);
    let title = get(if android { &[] } else { &["titleBg", "titleBgActive"] })
        .map(|c| c.over(panel))
        .unwrap_or_else(|| mix(panel, ink, 0.04));
    let wall1 = get(if android { &["chat_wallpaper"] } else { &["historyBg", "windowBgOver"] })
        .map(|c| c.over(panel))
        .unwrap_or_else(|| mix(panel, accent, 0.1));
    let wall2 = get(if android { &["chat_wallpaper_gradient_to", "chat_wallpaper"] } else { &[] })
        .map(|c| c.over(panel))
        .unwrap_or_else(|| mix(wall1, accent, 0.08));

    // читаемость: темы бывают с почти невидимыми кнопками и подписями
    let ink = ensure_contrast(ink, panel, 7.0);
    let muted = ensure_contrast(muted, panel, 3.2);
    let accent = ensure_contrast(accent, panel, 2.6);
    let sel = if contrast(sel, panel) < 1.35 { accent } else { sel };
    let text_in = ensure_contrast(text_in, bubble_in, 6.0);
    let text_out = ensure_contrast(text_out, bubble_out, 6.0);
    let rail_tx = ensure_contrast(rail_tx, rail, 3.0);
    let mut c = BTreeMap::new();
    let mut put = |k: &str, v: Rgba| {
        c.insert(k.to_string(), v.hex());
    };
    put("panel", panel);
    put("ink", ink);
    put("muted", muted);
    put("faint", mix(muted, panel, 0.42));
    put("accent", accent);
    put("on-accent", readable_on(accent));
    put("on-sel", readable_on(sel));
    put("accent-soft", accent.alpha(0.2));
    put("field", field);
    put("line", line);
    put("sel", sel);
    put("hover", hover);
    put("rail", rail);
    put("rail-tx", rail_tx);
    put("rail-on", rail_on);
    put("bubble-in", bubble_in);
    put("bubble-out", bubble_out);
    put("text-in", text_in);
    put("text-out", text_out);
    put("meta-in", meta_in);
    put("meta-out", meta_out);
    put("check", check);
    put("pill", if dark { BLACK.alpha(0.44) } else { mix(accent, BLACK, 0.45).alpha(0.44) });
    put("mbadge", mix(muted, panel, 0.3));
    put("danger", Rgba(0xE5, 0x48, 0x4D, 255));
    put("ok", Rgba(0x1F, 0xA4, 0x63, 255));
    put("title", title);
    put("kb", if dark { WHITE.alpha(0.12) } else { accent.alpha(0.4) });
    put("shadow", if dark { BLACK.alpha(0.5) } else { Rgba(0x0A, 0x1E, 0x3C, 0x26) });
    put("wall1", wall1);
    put("wall2", wall2);
    (dark, c)
}

// ------------------------------------------------------------ .fptheme (темы FunPay Tools)

/// Тема в формате FunPay Tools (JSON): фон из двух цветов или картинки, цвет блоков, текста и ссылок.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct FpTheme {
    pub bg_color1: String,
    pub bg_color2: String,
    pub container_bg_color: String,
    pub container_bg_opacity: f32,
    pub text_color: String,
    pub link_color: String,
    pub bg_image: String,
    pub font: String,
    pub bg_blur: f32,
    pub bg_brightness: f32,
    pub border_radius: f32,
    pub enable_glassmorphism: bool,
    pub glassmorphism_blur: f32,
}

impl Default for FpTheme {
    fn default() -> Self {
        Self {
            bg_color1: "#2F80ED".into(),
            bg_color2: "#8EC5FF".into(),
            container_bg_color: "#FFFFFF".into(),
            container_bg_opacity: 1.0,
            text_color: "#0E1116".into(),
            link_color: "#2F80ED".into(),
            bg_image: String::new(),
            font: "Onest".into(),
            bg_blur: 0.0,
            bg_brightness: 100.0,
            border_radius: 12.0,
            enable_glassmorphism: false,
            glassmorphism_blur: 10.0,
        }
    }
}

fn hex6(s: &str) -> Option<Rgba> {
    let h = s.trim().trim_start_matches('#');
    let h = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect::<String>() } else { h.to_string() };
    Rgba::parse_hex(&format!("#{h}"))
}

/// Картинка фона: ссылка или base64 (data:image/...;base64,...). Размытие и яркость применяем сразу.
pub fn fp_wall(t: &FpTheme) -> Option<(Vec<u8>, String, bool)> {
    let src = t.bg_image.trim();
    if src.is_empty() {
        return None;
    }
    let bytes = if let Some(rest) = src.strip_prefix("data:") {
        use base64::Engine;
        let b64 = rest.split(',').nth(1)?;
        base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?
    } else if src.starts_with("http") {
        let c = crate::fp::http_client_follow(30);
        let r = c.get(src).send().ok()?;
        if !r.status().is_success() {
            return None;
        }
        r.bytes().ok()?.to_vec()
    } else {
        std::fs::read(src).ok()?
    };
    // гифку оставляем как есть, размытие и яркость наложатся на каждый кадр при показе
    if crate::anim::is_animated(&bytes) {
        let ext = match image::guess_format(&bytes) {
            Ok(image::ImageFormat::WebP) => "webp",
            Ok(image::ImageFormat::Png) => "png",
            _ => "gif",
        };
        return Some((bytes, ext.into(), false));
    }
    let img = image::load_from_memory(&bytes).ok()?;
    // размытый фон всё равно мягкий: уменьшаем, так размытие идёт мгновенно
    let max = if t.bg_blur > 0.5 { 1280 } else { 2200 };
    let img = if img.width().max(img.height()) > max { img.resize(max, max, image::imageops::FilterType::Triangle) } else { img };
    let img = if t.bg_blur > 0.5 { img.fast_blur(t.bg_blur.min(30.0)) } else { img };
    let mut rgb = img.to_rgb8();
    let k = (t.bg_brightness / 100.0).clamp(0.2, 1.5);
    if (k - 1.0).abs() > 0.01 {
        for p in rgb.pixels_mut() {
            for c in p.0.iter_mut() {
                *c = ((*c as f32) * k).min(255.0) as u8;
            }
        }
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(rgb).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg).ok()?;
    Some((out, "jpg".into(), false))
}

/// .fptheme → цвета приложения. Недостающее выводится так же, как для остальных тем.
pub fn from_fptheme(text: &str, name: &str) -> Result<(Loaded, FpTheme), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|_| "Файл темы повреждён".to_string())?;
    if v.get("bgColor1").is_none() || v.get("font").is_none() {
        return Err("Это не похоже на тему FunPay Tools".into());
    }
    // числа в файлах тем бывают строками ("92"), поэтому читаем по полю
    let st = |k: &str, d: &str| v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).unwrap_or(d).to_string();
    let num = |k: &str, d: f32| match v.get(k) {
        Some(serde_json::Value::Number(n)) => n.as_f64().map(|x| x as f32).unwrap_or(d),
        Some(serde_json::Value::String(x)) => x.trim().parse().unwrap_or(d),
        _ => d,
    };
    let def = FpTheme::default();
    let t = FpTheme {
        bg_color1: st("bgColor1", &def.bg_color1),
        bg_color2: st("bgColor2", &st("bgColor1", &def.bg_color2)),
        container_bg_color: st("containerBgColor", &def.container_bg_color),
        container_bg_opacity: num("containerBgOpacity", 1.0),
        text_color: st("textColor", &def.text_color),
        link_color: st("linkColor", &def.link_color),
        bg_image: st("bgImage", ""),
        font: st("font", "Onest"),
        bg_blur: num("bgBlur", 0.0),
        bg_brightness: num("bgBrightness", 100.0),
        border_radius: num("borderRadius", 12.0),
        enable_glassmorphism: match v.get("enableGlassmorphism") {
            Some(serde_json::Value::Bool(b)) => *b,
            Some(serde_json::Value::String(x)) => x.trim() == "true" || x.trim() == "1",
            Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
            _ => false,
        },
        glassmorphism_blur: num("glassmorphismBlur", 10.0),
    };
    Ok((build_fp(&t, name, true), t))
}

pub fn build_fp(t: &FpTheme, name: &str, with_wall: bool) -> Loaded {
    let bg1 = hex6(&t.bg_color1).unwrap_or(Rgba(0x2F, 0x80, 0xED, 255));
    let bg2 = hex6(&t.bg_color2).unwrap_or(bg1);
    let cont = hex6(&t.container_bg_color).unwrap_or(WHITE);
    // полупрозрачные блоки ложатся на фон: смешиваем, чтобы панели были сплошными
    let op = t.container_bg_opacity.clamp(0.3, 1.0);
    let panel = mix(mix(bg1, bg2, 0.5), cont, op);
    let text = hex6(&t.text_color);
    let link = hex6(&t.link_color);
    let get = |keys: &[&str]| -> Option<Rgba> {
        keys.iter().find_map(|k| match *k {
            "windowBg" => Some(panel),
            "windowFg" => text,
            "activeButtonBg" => link,
            "historyBg" => Some(bg1),
            _ => None,
        })
    };
    let (dark, mut colors) = build(&get, false);
    colors.insert("wall1".into(), bg1.hex());
    colors.insert("wall2".into(), bg2.hex());
    let wall = if with_wall { fp_wall(t) } else { None };
    let animated = wall.as_ref().map(|w| w.1 != "jpg").unwrap_or(false);
    let fx = Fx {
        msg_alpha: t.container_bg_opacity.clamp(0.2, 1.0),
        glass: t.enable_glassmorphism,
        glass_blur: t.glassmorphism_blur.clamp(0.0, 40.0),
        wall_blur: if animated { t.bg_blur } else { 0.0 },
        wall_bright: if animated { t.bg_brightness } else { 100.0 },
    };
    Loaded { name: name.to_string(), dark, colors, wall, fx }
}

/// Читает файл темы любого из поддерживаемых видов.
pub fn load(path: &std::path::Path) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|_| "Не удалось прочитать файл".to_string())?;
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Тема").to_string();
    let lower = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if lower == "fptheme" || lower == "json" || bytes.first() == Some(&b'{') {
        return from_fptheme(&String::from_utf8_lossy(&bytes), &name).map(|x| x.0);
    }
    let is_zip = bytes.starts_with(b"PK");
    let (dark, colors, wall) = if is_zip || lower == "tdesktop-theme" || lower == "tdesktop-palette" {
        let (map, wall) = if is_zip { load_td_zip(&bytes)? } else { (parse_td_text(&String::from_utf8_lossy(&bytes)), None) };
        if td_resolve(&map, "windowBg").is_none() && td_resolve(&map, "windowFg").is_none() {
            return Err("Это не похоже на тему Telegram Desktop".into());
        }
        let get = |keys: &[&str]| keys.iter().find_map(|k| td_resolve(&map, k));
        let (d, c) = build(&get, false);
        (d, c, wall)
    } else {
        let (map, wall) = parse_attheme(&bytes);
        if map.len() < 3 {
            return Err("Это не похоже на тему Telegram (.attheme или .tdesktop-theme)".into());
        }
        let get = |keys: &[&str]| keys.iter().find_map(|k| map.get(*k).copied());
        let (d, c) = build(&get, true);
        (d, c, wall)
    };
    Ok(Loaded { name, dark, colors, wall, fx: Fx::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fptheme_colors() {
        let j = r##"{"bgColor1":"#ff6d15","bgColor2":"#f4cf78","containerBgColor":"#0b0b0b","containerBgOpacity":0.8,"textColor":"#f0f0f0","linkColor":"#2d6bb3","bgImage":"","font":"Roboto","bgBlur":"5","bgBrightness":"80"}"##;
        let (l, t) = from_fptheme(j, "x").unwrap();
        assert!(l.dark);
        assert_eq!(t.link_color, "#2d6bb3");
        assert_eq!(l.colors["wall1"], "#FF6D15FF");
        assert!(from_fptheme(r#"{"a":1}"#, "x").is_err());
    }

    #[test]
    fn td_text_and_refs() {
        let m = parse_td_text("// comment\nwindowBg: #17212b; /* block\n comment */ windowFg: #f5f5f5;\nmsgInBg: windowBg;\nmsgOutBg: #2b5278ff;\nactiveButtonBg: #2f6ea5;");
        assert_eq!(td_resolve(&m, "msgInBg"), Some(Rgba(0x17, 0x21, 0x2b, 255)));
        let get = |keys: &[&str]| keys.iter().find_map(|k| td_resolve(&m, k));
        let (dark, c) = build(&get, false);
        assert!(dark);
        assert_eq!(c["panel"], "#17212BFF");
        assert_eq!(c["bubble-out"], "#2B5278FF");
        assert_eq!(c["accent"], "#2F6EA5FF");
        assert_eq!(c["text-in"], "#F5F5F5FF");
    }

    #[test]
    fn td_zip_with_wallpaper() {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("colors.tdesktop-theme", o).unwrap();
            z.write_all(b"windowBg: #ffffff;\nwindowFg: #000000;\nmsgOutBg: #effdde;").unwrap();
            z.start_file("tiled.png", o).unwrap();
            z.write_all(&[0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
            z.finish().unwrap();
        }
        let p = std::env::temp_dir().join("fpt_test.tdesktop-theme");
        std::fs::write(&p, buf.into_inner()).unwrap();
        let t = load(&p).unwrap();
        assert!(!t.dark);
        assert_eq!(t.colors["bubble-out"], "#EFFDDEFF");
        let (_, ext, tiled) = t.wall.unwrap();
        assert_eq!(ext, "png");
        assert!(tiled);
    }

    #[test]
    fn attheme_ints_hex_and_wps() {
        let mut b = b"windowBackgroundWhite=-15527149\nwindowBackgroundWhiteBlackText=-1\nchat_outBubble=#ff2b5278\nchat_inBubble=-14540254\nfeaturedStickers_addButton=-12088333\nWPS\n".to_vec();
        b.extend_from_slice(&[0xFF, 0xD8, 0xFF].repeat(40));
        b.extend_from_slice(b"\nWPE\n");
        let (m, wall) = parse_attheme(&b);
        assert_eq!(m["windowBackgroundWhiteBlackText"], Rgba(255, 255, 255, 255));
        assert_eq!(m["chat_outBubble"], Rgba(0x2b, 0x52, 0x78, 0xff));
        assert_eq!(m["windowBackgroundWhite"], Rgba(0x13, 0x13, 0x13, 255));
        assert_eq!(wall.unwrap().1, "jpg");
        let get = |keys: &[&str]| keys.iter().find_map(|k| m.get(*k).copied());
        let (dark, c) = build(&get, true);
        assert!(dark);
        assert_eq!(c["bubble-out"], "#2B5278FF");
    }
}
