//! Загрузка картинок в фоне: диск-кэш, декодирование, уменьшение. Готовое отдаётся в UI-поток.

use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

pub struct Decoded {
    pub key: String,
    pub buf: SharedPixelBuffer<Rgba8Pixel>,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone)]
pub struct ImgLoader {
    tx: Sender<(String, String, u32)>,
    inflight: Arc<Mutex<HashSet<String>>>,
}

pub fn cache_dir() -> PathBuf {
    let d = crate::config::dir().join("cache");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn hash(s: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Байты картинки: файл, диск-кэш или сеть.
pub fn fetch_bytes(client: &reqwest::blocking::Client, url: &str) -> Option<Vec<u8>> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read(path).ok();
    }
    let p = cache_dir().join(hash(url));
    if let Ok(b) = std::fs::read(&p) {
        if !b.is_empty() {
            return Some(b);
        }
    }
    let r = client.get(url).header("Referer", "https://funpay.com/").send().ok()?;
    if !r.status().is_success() {
        return None;
    }
    let b = r.bytes().ok()?.to_vec();
    if b.len() > 64 {
        let _ = std::fs::write(&p, &b);
    }
    Some(b)
}

pub fn decode(bytes: &[u8], max: u32) -> Option<(SharedPixelBuffer<Rgba8Pixel>, u32, u32)> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w0, h0) = (img.width(), img.height());
    let img = if w0.max(h0) > max {
        img.resize(max, max, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut rgba = img.to_rgba8();
    if max <= 256 {
        // аватарки: квадрат по центру и круглая маска (на случай программного рендера без скругления)
        let side = rgba.width().min(rgba.height());
        let x0 = (rgba.width() - side) / 2;
        let y0 = (rgba.height() - side) / 2;
        let mut sq = image::imageops::crop_imm(&rgba, x0, y0, side, side).to_image();
        let r = side as f32 / 2.0;
        for (x, y, p) in sq.enumerate_pixels_mut() {
            let d = ((x as f32 + 0.5 - r).powi(2) + (y as f32 + 0.5 - r).powi(2)).sqrt();
            let a = (r - d + 0.5).clamp(0.0, 1.0);
            p.0[3] = (p.0[3] as f32 * a) as u8;
        }
        rgba = sq;
    }
    let (w, h) = (rgba.width(), rgba.height());
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba.as_raw(), w, h);
    Some((buf, w0, h0))
}

pub fn key(max: u32, url: &str) -> String {
    format!("{max}|{url}")
}

impl ImgLoader {
    pub fn new<F>(threads: usize, on_done: F) -> Self
    where
        F: Fn(Decoded) + Send + Sync + 'static,
    {
        let (tx, rx): (Sender<(String, String, u32)>, Receiver<(String, String, u32)>) = channel();
        let rx = Arc::new(Mutex::new(rx));
        let on_done = Arc::new(on_done);
        let inflight = Arc::new(Mutex::new(HashSet::new()));
        for _ in 0..threads {
            let rx = rx.clone();
            let on_done = on_done.clone();
            std::thread::spawn(move || {
                let client = crate::fp::http_client_follow(40);
                loop {
                    let job = { rx.lock().unwrap().recv() };
                    let Ok((k, url, max)) = job else { break };
                    if let Some(bytes) = fetch_bytes(&client, &url) {
                        if let Some((buf, w, h)) = decode(&bytes, max) {
                            on_done(Decoded { key: k.clone(), buf, w, h });
                        }
                    }
                }
            });
        }
        Self { tx, inflight }
    }

    /// Поставить в очередь, если ещё не грузится. Возвращает ключ кэша.
    pub fn request(&self, url: &str, max: u32) -> String {
        let k = key(max, url);
        if url.is_empty() {
            return k;
        }
        let mut f = self.inflight.lock().unwrap();
        if f.insert(k.clone()) {
            let _ = self.tx.send((k.clone(), url.to_string(), max));
        }
        k
    }
}
