//! Анимированные картинки (GIF, APNG, WebP). Кадры складываются в один лист сеткой,
//! а интерфейс сам переключает видимый кусок по таймеру, без работы в Rust на каждый кадр.

use image::{AnimationDecoder, ImageFormat, RgbaImage};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::io::Cursor;

/// Самая большая сторона листа. 8192 держат почти все видеокарты.
const MAX_TEX: u32 = 8192;
const MAX_FRAMES: usize = 400;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Info {
    pub frames: i32,
    pub cols: i32,
    pub fw: i32,
    pub fh: i32,
    /// длительность кадра, мс
    pub delay: i32,
}

pub struct Sheet {
    pub buf: SharedPixelBuffer<Rgba8Pixel>,
    pub info: Info,
}

impl Info {
    pub fn to_ui(self) -> crate::Anim {
        crate::Anim { frames: self.frames, cols: self.cols, fw: self.fw, fh: self.fh, delay: self.delay }
    }
}

/// Быстрая проверка без декодирования всех кадров.
pub fn is_animated(bytes: &[u8]) -> bool {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Gif) => {
            // второй блок изображения (0x2C) после первого
            let mut d = gif::DecodeOptions::new();
            d.set_color_output(gif::ColorOutput::Indexed);
            let Ok(mut r) = d.read_info(Cursor::new(bytes)) else { return false };
            let mut n = 0;
            while let Ok(Some(_)) = r.next_frame_info() {
                n += 1;
                if n > 1 {
                    return true;
                }
                if r.read_into_buffer(&mut vec![0u8; r.buffer_size()]).is_err() {
                    return false;
                }
            }
            false
        }
        Ok(ImageFormat::WebP) => bytes.len() > 40 && bytes[..bytes.len().min(256)].windows(4).any(|w| w == b"ANIM"),
        Ok(ImageFormat::Png) => {
            // acTL идёт до первого IDAT
            let end = bytes.windows(4).position(|w| w == b"IDAT").unwrap_or(bytes.len().min(4096));
            bytes[..end].windows(4).any(|w| w == b"acTL")
        }
        _ => false,
    }
}

/// Все кадры с задержками (мс). Кадры уже сведены с учётом прозрачности и очистки.
pub fn frames(bytes: &[u8]) -> Option<Vec<(RgbaImage, u32)>> {
    let fmt = image::guess_format(bytes).ok()?;
    let it: Box<dyn Iterator<Item = image::ImageResult<image::Frame>>> = match fmt {
        ImageFormat::Gif => Box::new(image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?.into_frames()),
        ImageFormat::WebP => Box::new(image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).ok()?.into_frames()),
        ImageFormat::Png => Box::new(image::codecs::png::PngDecoder::new(Cursor::new(bytes)).ok()?.apng().ok()?.into_frames()),
        _ => return None,
    };
    let mut out = Vec::new();
    for f in it {
        let Ok(f) = f else { break };
        let (n, d) = f.delay().numer_denom_ms();
        let ms = if d == 0 { 100 } else { n / d };
        // как в браузерах: слишком короткие задержки считаются за 100 мс
        let ms = if ms < 20 { 100 } else { ms };
        out.push((f.into_buffer(), ms));
        if out.len() >= MAX_FRAMES * 4 {
            break;
        }
    }
    if out.len() < 2 {
        return None;
    }
    Some(out)
}

/// Лист кадров. `max` ограничивает сторону кадра, `budget` общее число пикселей листа.
/// `fx` обрабатывает каждый кадр (размытие, яркость) уже после уменьшения.
pub fn sheet(bytes: &[u8], max: u32, budget: u64, fx: &dyn Fn(RgbaImage) -> RgbaImage) -> Option<Sheet> {
    if !is_animated(bytes) {
        return None;
    }
    let list = frames(bytes)?;
    let (w0, h0) = list[0].0.dimensions();
    if w0 == 0 || h0 == 0 {
        return None;
    }
    let total: u32 = list.iter().map(|x| x.1).sum();
    let mut scale = (max as f32 / w0.max(h0) as f32).min(1.0);
    // сначала пробуем оставить все кадры, уменьшая размер, но не меньше половины
    let mut step = 1usize;
    loop {
        let fw = ((w0 as f32 * scale).round() as u32).max(1);
        let fh = ((h0 as f32 * scale).round() as u32).max(1);
        let n = list.len().div_ceil(step).min(MAX_FRAMES);
        let cols = (MAX_TEX / fw).max(1) as usize;
        let rows = n.div_ceil(cols);
        let fits_tex = rows as u32 * fh <= MAX_TEX;
        let fits_mem = (n as u64) * (fw as u64) * (fh as u64) <= budget;
        if fits_tex && fits_mem && list.len().div_ceil(step) <= MAX_FRAMES {
            break;
        }
        if scale > 0.55 * (max as f32 / w0.max(h0) as f32).min(1.0) && scale > 0.2 {
            scale *= 0.85;
        } else {
            step += 1;
        }
        if step > 64 {
            return None;
        }
    }
    let fw = ((w0 as f32 * scale).round() as u32).max(1);
    let fh = ((h0 as f32 * scale).round() as u32).max(1);
    let picked: Vec<&(RgbaImage, u32)> = list.iter().step_by(step).collect();
    let n = picked.len();
    let cols = ((MAX_TEX / fw).max(1) as usize).min(n);
    let rows = n.div_ceil(cols);
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(cols as u32 * fw, rows as u32 * fh);
    let sw = buf.width() as usize;
    {
        let px = buf.make_mut_bytes();
        for (i, (img, _)) in picked.iter().enumerate() {
            let small = if fw != w0 || fh != h0 { image::imageops::resize(img, fw, fh, image::imageops::FilterType::Triangle) } else { img.clone() };
            let small = fx(small);
            let ox = (i % cols) * fw as usize;
            let oy = (i / cols) * fh as usize;
            let raw = small.as_raw();
            let row = fw as usize * 4;
            for y in 0..fh as usize {
                let dst = ((oy + y) * sw + ox) * 4;
                px[dst..dst + row].copy_from_slice(&raw[y * row..y * row + row]);
            }
        }
    }
    let delay = ((total as f32 / list.len() as f32) * step as f32).round().max(20.0) as i32;
    Some(Sheet { buf, info: Info { frames: n as i32, cols: cols as i32, fw: fw as i32, fh: fh as i32, delay } })
}

pub fn to_buf(img: &RgbaImage) -> SharedPixelBuffer<Rgba8Pixel> {
    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(img.as_raw(), img.width(), img.height())
}

/// Яркость в процентах, на месте.
pub fn brighten(mut img: RgbaImage, pct: f32) -> RgbaImage {
    let k = (pct / 100.0).clamp(0.2, 1.5);
    if (k - 1.0).abs() > 0.01 {
        for p in img.pixels_mut() {
            for c in p.0[..3].iter_mut() {
                *c = ((*c as f32) * k).min(255.0) as u8;
            }
        }
    }
    img
}

pub fn blur(img: RgbaImage, r: f32) -> RgbaImage {
    if r > 0.5 {
        image::imageops::fast_blur(&img, r.min(40.0))
    } else {
        img
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gif(n: usize, w: u16, h: u16) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut e = gif::Encoder::new(&mut out, w, h, &[0, 0, 0, 255, 0, 0]).unwrap();
            e.set_repeat(gif::Repeat::Infinite).unwrap();
            for i in 0..n {
                let px = vec![(i % 2) as u8; w as usize * h as usize];
                let mut f = gif::Frame::from_indexed_pixels(w, h, px, None);
                f.delay = 5;
                e.write_frame(&f).unwrap();
            }
        }
        out
    }

    #[test]
    fn animated_gif_to_sheet() {
        let b = gif(6, 40, 30);
        assert!(is_animated(&b));
        let s = sheet(&b, 1000, 10_000_000, &|i| i).unwrap();
        assert_eq!(s.info.frames, 6);
        assert_eq!((s.info.fw, s.info.fh), (40, 30));
        assert_eq!(s.info.delay, 50);
        assert_eq!(s.buf.width(), 240);
        // второй кадр красный
        let px = s.buf.as_bytes();
        let i = (40 * 4) as usize;
        assert_eq!(&px[i..i + 3], &[255, 0, 0]);
        assert!(!is_animated(&gif(1, 10, 10)));
    }

    #[test]
    fn sheet_respects_budget() {
        let b = gif(100, 200, 200);
        let s = sheet(&b, 200, 1_000_000, &|i| i).unwrap();
        assert!(s.info.frames as u64 * s.info.fw as u64 * s.info.fh as u64 <= 1_000_000);
        assert!(s.buf.width() <= MAX_TEX && s.buf.height() <= MAX_TEX);
    }
}
