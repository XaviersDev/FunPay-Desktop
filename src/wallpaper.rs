//! Кусок обоев под виджетом: виджет рисует его у себя фоном и выглядит частью рабочего стола.
//! Сами обои не трогаем, только читаем текущую картинку.

use image::RgbaImage;
use std::cell::RefCell;

thread_local! {
    /// обои, разложенные на главный экран так же, как их раскладывает Windows; и отметка, от чего собрано
    static FULL: RefCell<Option<(String, RgbaImage)>> = const { RefCell::new(None) };
}

#[cfg(windows)]
fn screen() -> (u32, u32) {
    #[link(name = "user32")]
    extern "system" {
        fn GetSystemMetrics(i: i32) -> i32;
    }
    unsafe { (GetSystemMetrics(0).max(1) as u32, GetSystemMetrics(1).max(1) as u32) }
}

#[cfg(windows)]
fn source() -> Option<(std::path::PathBuf, String)> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let desk = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Control Panel\\Desktop").ok()?;
    let style: String = desk.get_value("WallpaperStyle").unwrap_or_else(|_| "10".into());
    let tile: String = desk.get_value("TileWallpaper").unwrap_or_else(|_| "0".into());
    let bg: String = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Control Panel\\Colors")
        .ok()
        .and_then(|k| k.get_value("Background").ok())
        .unwrap_or_else(|| "0 0 0".into());
    // текущая картинка рабочего стола, в том числе из слайд-шоу
    let transcoded = std::env::var("APPDATA").ok().map(|a| std::path::Path::new(&a).join("Microsoft\\Windows\\Themes\\TranscodedWallpaper"));
    let path = transcoded.filter(|p| p.exists()).or_else(|| {
        let w: String = desk.get_value("WallPaper").ok()?;
        (!w.is_empty()).then(|| std::path::PathBuf::from(w))
    });
    let path = path.unwrap_or_default();
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).map(|t| format!("{t:?}")).unwrap_or_default();
    let (w, h) = screen();
    Some((path.clone(), format!("{}|{mtime}|{style}|{tile}|{bg}|{w}x{h}", path.display())))
}

#[cfg(windows)]
fn compose(path: &std::path::Path, mark: &str) -> Option<RgbaImage> {
    let parts: Vec<&str> = mark.split('|').collect();
    let style = parts.get(2).copied().unwrap_or("10");
    let tile = parts.get(3).copied().unwrap_or("0") == "1";
    let bg: Vec<u8> = parts.get(4).unwrap_or(&"0 0 0").split_whitespace().filter_map(|x| x.parse().ok()).collect();
    let bgc = image::Rgba([*bg.first().unwrap_or(&0), *bg.get(1).unwrap_or(&0), *bg.get(2).unwrap_or(&0), 255]);
    let (sw, sh) = screen();
    let mut out = RgbaImage::from_pixel(sw, sh, bgc);
    let Ok(src) = image::open(path) else { return Some(out) };
    let src = src.to_rgba8();
    let (iw, ih) = (src.width() as f32, src.height() as f32);
    let filter = image::imageops::FilterType::Triangle;
    match (style, tile) {
        // замостить
        ("0", true) => {
            let mut y = 0;
            while y < sh {
                let mut x = 0;
                while x < sw {
                    image::imageops::overlay(&mut out, &src, x as i64, y as i64);
                    x += src.width().max(1);
                }
                y += src.height().max(1);
            }
        }
        // по центру без масштаба
        ("0", false) => {
            image::imageops::overlay(&mut out, &src, (sw as i64 - src.width() as i64) / 2, (sh as i64 - src.height() as i64) / 2);
        }
        // растянуть
        ("2", _) => out = image::imageops::resize(&src, sw, sh, filter),
        // вписать целиком
        ("6", _) => {
            let k = (sw as f32 / iw).min(sh as f32 / ih);
            let (w, h) = ((iw * k).round().max(1.0) as u32, (ih * k).round().max(1.0) as u32);
            let r = image::imageops::resize(&src, w, h, filter);
            image::imageops::overlay(&mut out, &r, (sw as i64 - w as i64) / 2, (sh as i64 - h as i64) / 2);
        }
        // заполнение (10), растянуть на все экраны (22) и всё остальное
        _ => {
            let k = (sw as f32 / iw).max(sh as f32 / ih);
            let (w, h) = ((iw * k).round().max(1.0) as u32, (ih * k).round().max(1.0) as u32);
            let r = image::imageops::resize(&src, w, h, filter);
            image::imageops::overlay(&mut out, &r, (sw as i64 - w as i64) / 2, (sh as i64 - h as i64) / 2);
        }
    }
    Some(out)
}

/// Поменялись ли обои с прошлого раза (проверяется дёшево, по дате файла и настройкам).
pub fn changed() -> bool {
    #[cfg(windows)]
    {
        let Some((_, mark)) = source() else { return false };
        return FULL.with(|f| f.borrow().as_ref().map(|x| x.0 != mark).unwrap_or(true));
    }
    #[allow(unreachable_code)]
    false
}

/// Кусок обоев в физических пикселях экрана. None, если обои не прочитались или виджет не на главном экране.
pub fn crop(x: i32, y: i32, w: u32, h: u32) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
    #[cfg(windows)]
    {
        let (path, mark) = source()?;
        let fresh = FULL.with(|f| f.borrow().as_ref().map(|x| x.0 == mark).unwrap_or(false));
        if !fresh {
            let img = compose(&path, &mark)?;
            FULL.with(|f| *f.borrow_mut() = Some((mark, img)));
        }
        return FULL.with(|f| {
            let f = f.borrow();
            let (_, full) = f.as_ref()?;
            if x < 0 || y < 0 || x as u32 + w > full.width() || y as u32 + h > full.height() {
                return None;
            }
            let part = image::imageops::crop_imm(full, x as u32, y as u32, w, h).to_image();
            Some(slint::SharedPixelBuffer::clone_from_slice(part.as_raw(), w, h))
        });
    }
    #[allow(unreachable_code)]
    {
        let _ = (x, y, w, h);
        None
    }
}
