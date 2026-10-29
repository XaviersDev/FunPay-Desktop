//! Стикеры FunPay Tools: те же паки, что в мобильном приложении (GitHub, stickers/index.json).
//! В FunPay стикер уходит обычной картинкой PNG.

// Несколько зеркал: на части сетей (в т.ч. в РФ) GitHub raw и один из CDN могут не открываться,
// поэтому перебираем по очереди, пока какое-то не ответит. Картинки грузятся с того же зеркала.
const MIRRORS: [&str; 5] = [
    "https://cdn.jsdelivr.net/gh/XaviersDev/FunPay-Tools-Android@main/stickers/",
    "https://fastly.jsdelivr.net/gh/XaviersDev/FunPay-Tools-Android@main/stickers/",
    "https://gcore.jsdelivr.net/gh/XaviersDev/FunPay-Tools-Android@main/stickers/",
    "https://raw.githubusercontent.com/XaviersDev/FunPay-Tools-Android/main/stickers/",
    "https://raw.githack.com/XaviersDev/FunPay-Tools-Android/main/stickers/",
];

#[derive(Debug, Clone)]
pub struct Pack {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub thumbs: bool,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Store {
    pub base: String,
    pub packs: Vec<Pack>,
}

impl Store {
    pub fn thumb(&self, pack: &Pack, file: &str) -> String {
        if pack.thumbs {
            format!("{}{}/t/{}", self.base, pack.id, file)
        } else {
            self.full(pack, file)
        }
    }
    pub fn full(&self, pack: &Pack, file: &str) -> String {
        format!("{}{}/{}", self.base, pack.id, file)
    }
    pub fn find(&self, key: &str) -> Option<(&Pack, String)> {
        let (p, f) = key.split_once('/')?;
        let pack = self.packs.iter().find(|x| x.id == p)?;
        Some((pack, f.to_string()))
    }
}

pub fn parse(json: &str, base: &str) -> Option<Store> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut packs = Vec::new();
    for o in v.get("packs")?.as_array()? {
        let id = o.get("id").and_then(|x| x.as_str()).unwrap_or("").trim().trim_matches('/').to_string();
        if id.is_empty() {
            continue;
        }
        let files: Vec<String> = o
            .get("stickers")
            .and_then(|s| s.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|it| match it {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(m) => m.get("file").and_then(|f| f.as_str()).map(|s| s.to_string()),
                        _ => None,
                    })
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        if files.is_empty() {
            continue;
        }
        packs.push(Pack {
            title: o.get("title").and_then(|x| x.as_str()).filter(|s| !s.is_empty()).unwrap_or(&id).to_string(),
            icon: o.get("icon").and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| files[0].clone()),
            thumbs: o.get("thumbs").and_then(|x| x.as_bool()).unwrap_or(true),
            id,
            files,
        });
    }
    Some(Store { base: base.to_string(), packs })
}

/// Загружает список паков (сначала raw.githubusercontent, потом jsDelivr).
pub fn fetch() -> Option<Store> {
    let client = crate::fp::http_client_follow(20);
    let t = chrono::Utc::now().timestamp() / 60;
    for base in MIRRORS {
        if let Ok(r) = client.get(format!("{base}index.json?t={t}")).send() {
            if r.status().is_success() {
                if let Ok(txt) = r.text() {
                    if let Some(s) = parse(&txt, base) {
                        if !s.packs.is_empty() {
                            let _ = std::fs::write(crate::config::dir().join("stickers.json"), &txt);
                            let _ = std::fs::write(crate::config::dir().join("stickers_base.txt"), base);
                            return Some(s);
                        }
                    }
                }
            }
        }
    }
    // оффлайн: берём сохранённый список и то зеркало, что работало в прошлый раз
    let txt = std::fs::read_to_string(crate::config::dir().join("stickers.json")).ok()?;
    let base = std::fs::read_to_string(crate::config::dir().join("stickers_base.txt")).unwrap_or_else(|_| MIRRORS[0].to_string());
    parse(&txt, &base)
}

/// Готовит PNG стикера для отправки, возвращает путь к файлу.
pub fn prepare_png(full_url: &str, key: &str) -> Option<std::path::PathBuf> {
    let client = crate::fp::http_client(30);
    let bytes = crate::imgcache::fetch_bytes(&client, full_url)?;
    let img = image::load_from_memory(&bytes).ok()?;
    let img = if img.width().max(img.height()) > 512 { img.resize(512, 512, image::imageops::FilterType::Lanczos3) } else { img };
    let dir = crate::imgcache::cache_dir().join("outgoing");
    let _ = std::fs::create_dir_all(&dir);
    // имя как в мобильном приложении: sticker_<пак>_<файл без расширения>.png
    let (pack, file) = key.split_once('/').unwrap_or(("s", key));
    let stem = file.rsplit_once('.').map(|x| x.0).unwrap_or(file);
    let safe: String = format!("{pack}_{stem}").chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    let path = dir.join(format!("sticker_{safe}.png"));
    img.save_with_format(&path, image::ImageFormat::Png).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests_live {
    #[test]
    #[ignore]
    fn live_sticker_prepare() {
        let s = super::fetch().expect("index");
        let p = &s.packs[0];
        let f = &p.files[0];
        let url = s.full(p, f);
        println!("base={} url={}", s.base, url);
        let path = super::prepare_png(&url, &format!("{}/{}", p.id, f)).expect("prepare_png");
        let (bytes, name, mime) = crate::worker::prepare_image(&path).expect("prepare_image");
        println!("path={path:?} name={name} mime={mime} len={}", bytes.len());
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }
}
