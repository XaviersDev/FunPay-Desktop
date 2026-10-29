//! Свои всплывающие уведомления: маленькие окна в углу экрана, как в Telegram Desktop.

use crate::config::PopupCfg;
use crate::NotifyWindow;
use slint::{Color, ComponentHandle, Image};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub struct Note {
    pub chat: String,
    pub title: String,
    pub text: String,
    pub avatar: Option<Image>,
    pub letter: String,
    pub c1: Color,
    pub c2: Color,
}

struct Pop {
    win: NotifyWindow,
    id: u64,
    until: Instant,
}

thread_local! {
    static POPS: RefCell<Vec<Pop>> = const { RefCell::new(Vec::new()) };
    static NEXT: RefCell<u64> = const { RefCell::new(0) };
    static TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    static MAIN: RefCell<Option<(slint::Weak<crate::AppWindow>, i32)>> = const { RefCell::new(None) };
}

/// Окно, которое сейчас создаётся, это уведомление (для хука атрибутов winit).
static CREATING: AtomicBool = AtomicBool::new(false);

pub fn creating() -> bool {
    CREATING.load(Ordering::Relaxed)
}

const W: f32 = 340.0;
const H: f32 = 78.0;
const GAP: f32 = 10.0;
const MARGIN: f32 = 14.0;
const LIFE: Duration = Duration::from_secs(7);

/// Рабочая область экрана (без панели задач) в физических пикселях.
#[cfg(windows)]
fn work_area(_main: &crate::AppWindow) -> (i32, i32, i32, i32) {
    #[repr(C)]
    struct Rect {
        l: i32,
        t: i32,
        r: i32,
        b: i32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, pv: *mut core::ffi::c_void, win_ini: u32) -> i32;
    }
    let mut r = Rect { l: 0, t: 0, r: 1920, b: 1040 };
    unsafe {
        SystemParametersInfoW(0x0030, 0, &mut r as *mut Rect as *mut core::ffi::c_void, 0);
    }
    (r.l, r.t, r.r, r.b)
}

#[cfg(not(windows))]
fn work_area(main: &crate::AppWindow) -> (i32, i32, i32, i32) {
    use slint::winit_030::WinitWindowAccessor;
    main.window()
        .with_winit_window(|w| {
            w.current_monitor().or_else(|| w.primary_monitor()).map(|m| {
                let p = m.position();
                let s = m.size();
                (p.x, p.y, p.x + s.width as i32, p.y + s.height as i32)
            })
        })
        .flatten()
        .unwrap_or((0, 0, 1920, 1080))
}

fn layout(main: &crate::AppWindow, corner: i32) {
    let scale = main.window().scale_factor().max(0.5);
    let (l, t, r, b) = work_area(main);
    let (w, h, gap, m) = ((W * scale) as i32, (H * scale) as i32, (GAP * scale) as i32, (MARGIN * scale) as i32);
    POPS.with(|p| {
        let p = p.borrow();
        // новое ближе всего к углу, старые отъезжают
        for (k, pop) in p.iter().rev().enumerate() {
            let k = k as i32;
            let x = if corner == 0 || corner == 2 { l + m } else { r - m - w };
            let y = if corner < 2 { t + m + k * (h + gap) } else { b - m - h - k * (h + gap) };
            pop.win.window().set_position(slint::PhysicalPosition::new(x, y));
        }
    });
}

fn close_id(id: u64) {
    let main = MAIN.with(|m| m.borrow().as_ref().and_then(|(w, c)| w.upgrade().map(|a| (a, *c))));
    let removed = POPS.with(|p| {
        let mut p = p.borrow_mut();
        let before = p.len();
        p.retain(|x| {
            if x.id == id {
                let _ = x.win.hide();
                false
            } else {
                true
            }
        });
        before != p.len()
    });
    if removed {
        if let Some((a, c)) = main {
            layout(&a, c);
        }
    }
}

fn ensure_timer() {
    TIMER.with(|t| {
        if t.borrow().is_some() {
            return;
        }
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, Duration::from_millis(300), || {
            let now = Instant::now();
            let expired: Vec<u64> = POPS.with(|p| {
                let mut p = p.borrow_mut();
                for x in p.iter_mut() {
                    if x.win.get_hovered() {
                        x.until = now + Duration::from_secs(3);
                    }
                }
                p.iter().filter(|x| x.until <= now).map(|x| x.id).collect()
            });
            for id in expired {
                close_id(id);
            }
        });
        *t.borrow_mut() = Some(timer);
    });
}

pub fn show(main: &crate::AppWindow, n: Note, cfg: &PopupCfg, dark: bool, accent: Color) {
    MAIN.with(|m| *m.borrow_mut() = Some((main.as_weak(), cfg.corner)));
    let Ok(win) = NotifyWindow::new() else { return };
    let id = NEXT.with(|x| {
        *x.borrow_mut() += 1;
        *x.borrow()
    });
    win.set_head(n.title.into());
    win.set_body(n.text.chars().take(220).collect::<String>().into());
    win.set_has_avatar(n.avatar.is_some());
    if let Some(a) = n.avatar {
        win.set_avatar(a);
    }
    win.set_letter(n.letter.into());
    win.set_c1(n.c1);
    win.set_c2(n.c2);
    win.set_show_icon(cfg.show_icon);
    win.set_show_name(cfg.show_name);
    win.set_show_text(cfg.show_text);
    win.set_dark(dark);
    win.set_accent(accent);
    let chat = n.chat.clone();
    win.on_clicked(move || {
        close_id(id);
        let chat = chat.clone();
        if !chat.is_empty() {
            crate::ui::post(move |u| u.notification_clicked(chat));
        } else {
            crate::ui::post(|u| u.show_window());
        }
    });
    win.on_closed(move || close_id(id));

    // лишние старые закрываем
    let limit = cfg.count.clamp(1, 5) as usize;
    let old: Vec<u64> = POPS.with(|p| {
        let p = p.borrow();
        if p.len() >= limit {
            p.iter().take(p.len() + 1 - limit).map(|x| x.id).collect()
        } else {
            vec![]
        }
    });
    for o in old {
        close_id(o);
    }
    POPS.with(|p| p.borrow_mut().push(Pop { win: win.clone_strong(), id, until: Instant::now() + LIFE }));
    layout(main, cfg.corner);
    CREATING.store(true, Ordering::Relaxed);
    let _ = win.show();
    CREATING.store(false, Ordering::Relaxed);
    // позиция ещё раз: окно уже создано системой
    layout(main, cfg.corner);
    ensure_timer();
}

pub fn close_all() {
    let ids: Vec<u64> = POPS.with(|p| p.borrow().iter().map(|x| x.id).collect());
    for id in ids {
        close_id(id);
    }
}
