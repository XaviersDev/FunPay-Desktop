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

/// Читает файл темы любого из поддерживаемых видов.
pub fn load(path: &std::path::Path) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|_| "Не удалось прочитать файл".to_string())?;
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Тема").to_string();
    let lower = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
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
    Ok(Loaded { name, dark, colors, wall })
}

#[cfg(test)]
mod tests {
    use super::*;

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
