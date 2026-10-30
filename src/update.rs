//! Обновления с GitHub Releases (XaviersDev/FunPay-Desktop): проверка, скачивание и замена exe без участия пользователя.
//!
//! Как выкладывать релиз: тег v4.4.0 (или 4.4.0), к нему файл FunPayTools.exe
//! (или zip, внутри которого есть FunPayTools.exe). GitHub сам считает sha256 файла, мы его проверяем.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

const API: &str = "https://api.github.com/repos/XaviersDev/FunPay-Desktop/releases/latest";

#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub name: String,
    pub size: u64,
    pub sha256: Option<String>,
    pub notes: String,
}

#[derive(Debug)]
pub enum Check {
    UpToDate,
    NoRepo,
    New(Release),
}

pub fn current() -> semver::Version {
    semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(semver::Version::new(0, 0, 0))
}

fn parse_ver(tag: &str) -> Option<semver::Version> {
    let t = tag.trim().trim_start_matches(['v', 'V']);
    semver::Version::parse(t).ok().or_else(|| {
        // «4.4» → 4.4.0
        let parts: Vec<&str> = t.split('.').collect();
        if parts.len() == 2 {
            semver::Version::parse(&format!("{}.{}.0", parts[0], parts[1])).ok()
        } else {
            None
        }
    })
}

fn client(timeout: u64) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("FunPayTools-Desktop/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .unwrap_or_default()
}

pub fn check() -> Result<Check, String> {
    let r = client(20).get(API).header("Accept", "application/vnd.github+json").send().map_err(|_| "Нет связи с GitHub".to_string())?;
    let code = r.status().as_u16();
    if code == 404 {
        return Ok(Check::NoRepo);
    }
    if !r.status().is_success() {
        return Err(format!("GitHub ответил {code}"));
    }
    let v: serde_json::Value = r.json().map_err(|_| "Непонятный ответ GitHub".to_string())?;
    let tag = v.get("tag_name").and_then(|x| x.as_str()).unwrap_or("");
    let Some(ver) = parse_ver(tag) else { return Ok(Check::UpToDate) };
    if ver <= current() || v.get("draft").and_then(|x| x.as_bool()).unwrap_or(false) {
        return Ok(Check::UpToDate);
    }
    let assets = v.get("assets").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let pick = |pred: &dyn Fn(&str) -> bool| {
        assets.iter().find(|a| a.get("name").and_then(|n| n.as_str()).map(|n| pred(&n.to_lowercase())).unwrap_or(false)).cloned()
    };
    let asset = pick(&|n| n.ends_with(".exe") && !n.contains("setup") && !n.contains("installer"))
        .or_else(|| pick(&|n| n.ends_with(".zip") && n.contains("win") && !n.contains("src")))
        .or_else(|| pick(&|n| n.ends_with(".zip") && !n.contains("src") && !n.contains("source")));
    let Some(a) = asset else { return Ok(Check::UpToDate) };
    let sha256 = a.get("digest").and_then(|d| d.as_str()).and_then(|d| d.strip_prefix("sha256:")).map(|s| s.to_lowercase());
    Ok(Check::New(Release {
        version: ver.to_string(),
        url: a.get("browser_download_url").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        name: a.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        size: a.get("size").and_then(|x| x.as_u64()).unwrap_or(0),
        sha256,
        notes: v.get("body").and_then(|x| x.as_str()).unwrap_or("").chars().take(600).collect(),
    }))
}

fn dir() -> PathBuf {
    let d = crate::config::dir().join("update");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn is_exe(b: &[u8]) -> bool {
    b.len() > 1_000_000 && b.starts_with(b"MZ")
}

/// Скачать и проверить. Возвращает путь к новому exe.
pub fn download(rel: &Release, progress: impl Fn(u32)) -> Result<PathBuf, String> {
    if !rel.url.starts_with("https://github.com/") && !rel.url.starts_with("https://objects.githubusercontent.com/") {
        return Err("Странная ссылка на файл обновления".into());
    }
    let mut r = client(600).get(&rel.url).send().map_err(|_| "Не удалось скачать обновление".to_string())?;
    if !r.status().is_success() {
        return Err(format!("Скачивание: {}", r.status().as_u16()));
    }
    let total = r.content_length().unwrap_or(rel.size).max(1);
    let mut data = Vec::with_capacity(total as usize);
    let mut buf = [0u8; 64 * 1024];
    let mut last = 0;
    loop {
        let n = r.read(&mut buf).map_err(|_| "Обрыв при скачивании".to_string())?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        let p = (data.len() as u64 * 100 / total) as u32;
        if p != last {
            last = p;
            progress(p.min(100));
        }
    }
    if let Some(want) = &rel.sha256 {
        let got = format!("{:x}", Sha256::digest(&data));
        if &got != want {
            return Err("Файл обновления повреждён (не совпала контрольная сумма)".into());
        }
    }
    let exe = if rel.name.to_lowercase().ends_with(".zip") { exe_from_zip(&data)? } else { data };
    if !is_exe(&exe) {
        return Err("В релизе нет подходящего FunPayTools.exe".into());
    }
    let p = dir().join(format!("FunPayTools-{}.exe", rel.version));
    std::fs::write(&p, &exe).map_err(|e| format!("Не удалось сохранить: {e}"))?;
    Ok(p)
}

fn exe_from_zip(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| "Архив обновления не открылся".to_string())?;
    let mut best: Option<usize> = None;
    for i in 0..z.len() {
        let Ok(f) = z.by_index(i) else { continue };
        let n = f.name().to_lowercase();
        if n.ends_with("funpaytools.exe") {
            best = Some(i);
            break;
        }
        if n.ends_with(".exe") && best.is_none() {
            best = Some(i);
        }
    }
    let i = best.ok_or_else(|| "В архиве нет exe".to_string())?;
    let mut f = z.by_index(i).map_err(|_| "Архив повреждён".to_string())?;
    let mut out = Vec::new();
    f.read_to_end(&mut out).map_err(|_| "Архив повреждён".to_string())?;
    Ok(out)
}

/// Подменить свой exe новым: работающий файл Windows разрешает переименовать, но не перезаписать.
pub fn install(new_exe: &Path) -> Result<PathBuf, String> {
    let cur = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = cur.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&cur, &old).map_err(|e| format!("Нет доступа к папке программы: {e}"))?;
    if let Err(e) = std::fs::copy(new_exe, &cur) {
        // откатываем, чтобы не остаться без программы
        let _ = std::fs::rename(&old, &cur);
        return Err(format!("Не удалось заменить файл: {e}"));
    }
    let _ = std::fs::remove_file(new_exe);
    Ok(cur)
}

/// Запустить новую версию и выйти из этой.
pub fn relaunch(exe: &Path, background: bool) {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--after-update");
    if background {
        cmd.arg("--background");
    }
    let _ = cmd.spawn();
}

/// Убрать хвосты прошлого обновления.
pub fn cleanup() {
    if let Ok(cur) = std::env::current_exe() {
        let _ = std::fs::remove_file(cur.with_extension("exe.old"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_ver("v4.4.0").unwrap().to_string(), "4.4.0");
        assert_eq!(parse_ver("4.5").unwrap().to_string(), "4.5.0");
        assert!(parse_ver("v4.4.0").unwrap() <= current());
        assert!(parse_ver("v4.4.1").unwrap() > current());
    }
}
