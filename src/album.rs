//! Склейка картинок в альбомы, как в Telegram. На FunPay каждая картинка отдельное сообщение,
//! поэтому группируем подряд идущие картинки одного автора по времени отправки.

use crate::parse::Message;

/// Сколько секунд может пройти между картинками одного альбома.
pub const ALBUM_GAP_SECS: i64 = 90;
/// Текст сразу после альбома в пределах этого окна становится подписью.
pub const CAPTION_GAP_SECS: i64 = 30;
pub const ALBUM_MAX: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// Обычное сообщение (индекс в исходном списке)
    Single(usize),
    /// Альбом: индексы картинок + индекс подписи
    Album { images: Vec<usize>, caption: Option<usize> },
}

fn close(a: &Message, b: &Message, gap: i64) -> bool {
    if a.author_id != b.author_id || a.author != b.author {
        return false;
    }
    if !b.has_head {
        // FunPay склеил сообщения без шапки: тот же автор подряд
        return true;
    }
    match (a.ts, b.ts) {
        (Some(x), Some(y)) => (y - x).num_seconds().abs() <= gap,
        _ => false,
    }
}

pub fn group(msgs: &[Message]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < msgs.len() {
        let m = &msgs[i];
        if m.image.is_none() {
            out.push(Block::Single(i));
            i += 1;
            continue;
        }
        let mut images = vec![i];
        let mut j = i + 1;
        while j < msgs.len() && images.len() < ALBUM_MAX {
            let prev = &msgs[*images.last().unwrap()];
            let x = &msgs[j];
            if x.image.is_some() && close(prev, x, ALBUM_GAP_SECS) {
                images.push(j);
                j += 1;
            } else {
                break;
            }
        }
        let mut caption = None;
        if j < msgs.len() {
            let last = &msgs[*images.last().unwrap()];
            let x = &msgs[j];
            if x.image.is_none() && !x.text.is_empty() && close(last, x, CAPTION_GAP_SECS) && x.text.chars().count() <= 1024 {
                caption = Some(j);
                j += 1;
            }
        }
        if images.len() == 1 && caption.is_none() {
            out.push(Block::Album { images, caption: None });
        } else {
            out.push(Block::Album { images, caption });
        }
        i = j;
    }
    out
}

/// Плитка альбома: координаты в долях ширины альбома (и x, и y).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileGeom {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Раскладка n картинок. `aspect` — h/w первой картинки (для одиночной).
/// Возвращает (плитки, высота альбома в долях ширины).
pub fn layout(n: usize, aspect: f32) -> (Vec<TileGeom>, f32) {
    let t = |x, y, w, h| TileGeom { x, y, w, h };
    match n {
        0 => (vec![], 0.0),
        1 => {
            let r = aspect.clamp(0.45, 1.35);
            (vec![t(0.0, 0.0, 1.0, r)], r)
        }
        2 => (vec![t(0.0, 0.0, 0.5, 0.62), t(0.5, 0.0, 0.5, 0.62)], 0.62),
        3 => (
            vec![t(0.0, 0.0, 0.62, 0.8), t(0.62, 0.0, 0.38, 0.4), t(0.62, 0.4, 0.38, 0.4)],
            0.8,
        ),
        4 => (
            vec![t(0.0, 0.0, 1.0, 0.56), t(0.0, 0.56, 1.0 / 3.0, 0.3), t(1.0 / 3.0, 0.56, 1.0 / 3.0, 0.3), t(2.0 / 3.0, 0.56, 1.0 / 3.0, 0.3)],
            0.86,
        ),
        _ => {
            // строки по 2 и 3 штуки, двойки сверху
            let rows: Vec<usize> = match n {
                5 => vec![2, 3],
                6 => vec![3, 3],
                7 => vec![2, 2, 3],
                8 => vec![2, 3, 3],
                9 => vec![3, 3, 3],
                _ => vec![2, 2, 3, 3],
            };
            let mut tiles = Vec::new();
            let mut y = 0.0;
            for cnt in rows {
                let h = if cnt == 2 { 0.42 } else { 0.3 };
                let w = 1.0 / cnt as f32;
                for k in 0..cnt {
                    tiles.push(t(k as f32 * w, y, w, h));
                }
                y += h;
            }
            (tiles, y)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn m(id: u64, author: &str, head: bool, sec: u32, img: bool, text: &str) -> Message {
        Message {
            id,
            author_id: author.into(),
            author: author.into(),
            has_head: head,
            ts: Some(NaiveDate::from_ymd_opt(2026, 10, 28).unwrap().and_hms_opt(12, 0, sec).unwrap()),
            image: if img { Some(format!("https://x/{id}.jpg")) } else { None },
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn groups_by_time_and_author() {
        let v = vec![
            m(1, "a", true, 0, false, "смотри"),
            m(2, "a", true, 10, true, ""),
            m(3, "a", false, 10, true, ""),
            m(4, "a", true, 50, true, ""),
            m(5, "a", true, 55, false, "вот так"),
            m(6, "b", true, 56, true, ""),
            m(7, "a", true, 59, true, ""),
            m(8, "a", true, 59 , false, ""),
        ];
        let g = group(&v);
        assert_eq!(g[0], Block::Single(0));
        assert_eq!(g[1], Block::Album { images: vec![1, 2, 3], caption: Some(4) });
        assert_eq!(g[2], Block::Album { images: vec![5], caption: None });
        assert_eq!(g[3], Block::Album { images: vec![6], caption: None });
    }

    #[test]
    fn far_apart_images_split() {
        let v = vec![m(1, "a", true, 0, true, ""), m(2, "a", true, 0, true, "")];
        let mut v2 = v.clone();
        v2[1].ts = Some(v2[1].ts.unwrap() + chrono::Duration::minutes(5));
        assert_eq!(group(&v).len(), 1);
        assert_eq!(group(&v2).len(), 2);
    }

    #[test]
    fn layouts_fill_width() {
        for n in 1..=10 {
            let (tiles, h) = layout(n, 0.75);
            assert_eq!(tiles.len(), n);
            for t in &tiles {
                assert!(t.x + t.w <= 1.0001 && t.y + t.h <= h + 0.0001);
            }
        }
    }
}
