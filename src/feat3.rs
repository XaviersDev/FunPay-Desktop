//! Гифки (в переписке, в просмотре, на фоне темы) и прозрачные сообщения с матовым стеклом.

use crate::anim;
use crate::ui::{Ui, FULL};
use crate::{Anim, Theme};
use slint::{ComponentHandle, Image};

impl Ui {
    /// Пришёл лист кадров анимированной картинки.
    pub fn anim_ready(&mut self, key: &str, sheet: Image, info: anim::Info) {
        // большие листы для просмотра держим только для последней картинки
        if key.starts_with(&format!("{FULL}|")) {
            self.ext.anims.retain(|k, _| !k.starts_with(&format!("{FULL}|")));
        }
        self.ext.anims.insert(key.to_string(), (sheet, info));
    }

    /// Картинка для плитки: лист кадров, если есть, иначе обычная.
    pub fn tile_img(&self, key: &str, im: Option<Image>) -> (Image, Anim) {
        match self.ext.anims.get(key) {
            Some((s, i)) => (s.clone(), i.to_ui()),
            None => (im.unwrap_or_default(), Anim::default()),
        }
    }

    /// Для просмотра: большая гифка, если уже есть, иначе уменьшенная.
    pub fn viewer_anim(&self, url: &str) -> Option<(Image, Anim)> {
        self.ext
            .anims
            .get(&crate::imgcache::key(FULL, url))
            .or_else(|| self.ext.anims.get(&crate::imgcache::key(crate::ui::THUMB, url)))
            .map(|(s, i)| (s.clone(), i.to_ui()))
    }

    /// Фон темы: картинка или гифка, плюс размытая копия для стекла под сообщениями.
    pub fn apply_wall(&mut self, t: &crate::config::TgTheme) {
        let Some(app) = self.app() else { return };
        let th = app.global::<Theme>();
        self.ext.wall_gen += 1;
        let gen = self.ext.wall_gen;
        let alpha = if t.msg_alpha <= 0.01 { 1.0 } else { t.msg_alpha.clamp(0.2, 1.0) };
        th.set_c_msg_alpha(alpha);
        th.set_c_glass(t.glass);
        th.set_c_wall_anim(Anim::default());
        th.set_c_glass_anim(Anim::default());
        th.set_c_wall_tiled(t.wall_tiled);
        let bytes = if t.wall.is_empty() { None } else { std::fs::read(&t.wall).ok() };
        let first = bytes.as_ref().and_then(|b| image::load_from_memory(b).ok());
        let (Some(bytes), Some(first)) = (bytes, first) else {
            th.set_c_has_wall_img(false);
            th.set_c_wall_img(Image::default());
            th.set_c_glass_img(Image::default());
            return;
        };
        let animated = anim::is_animated(&bytes);
        let (wb, wbr) = if animated { (t.wall_blur, t.wall_bright) } else { (0.0, 100.0) };
        let first = if first.width().max(first.height()) > 2560 { first.resize(2560, 2560, image::imageops::FilterType::Triangle) } else { first };
        let mut rgba = first.to_rgba8();
        if animated {
            rgba = anim::brighten(anim::blur(rgba, wb), wbr);
        }
        th.set_c_wall_img(Image::from_rgba8(anim::to_buf(&rgba)));
        th.set_c_has_wall_img(true);
        let gsig = glass_sigma(t.glass_blur);
        if t.glass {
            th.set_c_glass_img(Image::from_rgba8(anim::to_buf(&glass_of(&rgba, gsig))));
        } else {
            th.set_c_glass_img(Image::default());
        }
        if !animated {
            return;
        }
        let glass = t.glass;
        std::thread::spawn(move || {
            let wall = anim::sheet(&bytes, 1100, 24_000_000, &|i| anim::brighten(anim::blur(i, wb), wbr));
            let gl = if glass { anim::sheet(&bytes, 360, 4_000_000, &|i| anim::blur(anim::brighten(i, wbr), gsig)) } else { None };
            let wall = wall.map(|s| (s.buf, s.info));
            let gl = gl.map(|s| (s.buf, s.info));
            crate::ui::post(move |u| {
                if u.ext.wall_gen != gen {
                    return;
                }
                let Some(app) = u.app() else { return };
                let th = app.global::<Theme>();
                if let Some((b, i)) = wall {
                    th.set_c_wall_img(Image::from_rgba8(b));
                    th.set_c_wall_anim(i.to_ui());
                }
                if let Some((b, i)) = gl {
                    th.set_c_glass_img(Image::from_rgba8(b));
                    th.set_c_glass_anim(i.to_ui());
                }
            });
        });
    }
}

/// Сила размытия стекла в пикселях уменьшенной копии (она примерно втрое меньше экрана).
fn glass_sigma(css_px: f32) -> f32 {
    (css_px.max(6.0) * 0.4).clamp(2.5, 16.0)
}

/// Уменьшенная размытая копия фона. Размытое всё равно мягкое, поэтому хватает 360 px.
pub fn glass_of(img: &image::RgbaImage, sigma: f32) -> image::RgbaImage {
    let (w, h) = img.dimensions();
    let k = 360.0 / w.max(h).max(1) as f32;
    let small = if k < 1.0 {
        image::imageops::resize(img, ((w as f32 * k) as u32).max(1), ((h as f32 * k) as u32).max(1), image::imageops::FilterType::Triangle)
    } else {
        img.clone()
    };
    anim::blur(small, sigma)
}
