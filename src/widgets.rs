//! Виджеты на рабочем столе: маленькие окна без рамки с балансом, продажами, чатами и лотами.

use crate::config::WidgetCfg;
use crate::WidgetWindow;
use slint::{Color, ComponentHandle};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const KINDS: usize = 4;
const W: f32 = 216.0;
const H: f32 = 112.0;
const GAP: f32 = 10.0;
const MARGIN: f32 = 16.0;

/// Данные для виджетов: обновляются из списка чатов и раз в несколько минут с FunPay.
#[derive(Clone, Debug, Default)]
pub struct Data {
    pub balance: String,
    pub balance_at: String,
    pub unread: i32,
    pub unread_names: Vec<String>,
    pub sales: Option<(usize, String, usize)>,
    pub lots: Option<(usize, usize)>,
}

struct Wid {
    kind: i32,
    win: WidgetWindow,
    last_pos: (i32, i32),
    /// окно сейчас внутри рабочего стола
    embedded: bool,
}

thread_local! {
    static WIDS: RefCell<Vec<Wid>> = const { RefCell::new(Vec::new()) };
    static DATA: RefCell<Data> = RefCell::new(Data::default());
    static TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    static DRAG_AT: RefCell<Option<Instant>> = const { RefCell::new(None) };
    static TICKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

static CREATING: AtomicBool = AtomicBool::new(false);
static TOP: AtomicBool = AtomicBool::new(false);
/// режим перестановки: виджеты временно обычные окна поверх всех
static EDIT: AtomicBool = AtomicBool::new(false);

/// Окно, которое сейчас создаётся, это виджет (для хука атрибутов winit).
pub fn creating() -> bool {
    CREATING.load(Ordering::Relaxed)
}

pub fn on_top() -> bool {
    TOP.load(Ordering::Relaxed)
}

pub fn data() -> Data {
    DATA.with(|d| d.borrow().clone())
}

pub fn update(f: impl FnOnce(&mut Data)) {
    DATA.with(|d| f(&mut d.borrow_mut()));
    refresh_all();
}

fn plural(n: usize, a: &str, b: &str, c: &str) -> String {
    let (m10, m100) = (n % 10, n % 100);
    let w = if m10 == 1 && m100 != 11 {
        a
    } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
        b
    } else {
        c
    };
    format!("{n} {w}")
}

/// Текст виджета: (заголовок, число, единица, строка под ним).
pub fn content(kind: i32, d: &Data) -> (String, String, String, String) {
    match kind {
        0 => {
            let b = d.balance.replace('\u{a0}', " ");
            let b = b.trim();
            let (num, unit) = match b.rsplit_once(' ') {
                Some((n, u)) if !u.chars().any(|c| c.is_ascii_digit()) => (n.to_string(), u.to_string()),
                _ => (b.to_string(), String::new()),
            };
            let num = if num.is_empty() { "0".to_string() } else { num };
            let sub = if d.balance_at.is_empty() { "Обновляется...".into() } else { format!("Обновлено в {}", d.balance_at) };
            ("Баланс".into(), num, if b.is_empty() { "₽".into() } else { unit }, sub)
        }
        1 => {
            let sub = if d.unread == 0 {
                "Всё прочитано".to_string()
            } else {
                let mut s = d.unread_names.iter().take(2).cloned().collect::<Vec<_>>().join(", ");
                let rest = d.unread as usize - d.unread_names.len().min(2);
                if rest > 0 {
                    s.push_str(&format!(" и ещё {rest}"));
                }
                s
            };
            ("Непрочитанные".into(), d.unread.to_string(), String::new(), sub)
        }
        2 => match &d.sales {
            Some((n, sum, paid)) => {
                let mut sub = if *n == 0 { "Пока без продаж".to_string() } else { format!("На {sum}") };
                if *n > 0 && *paid > 0 {
                    sub = format!("{sub}, ждут выполнения: {paid}");
                }
                ("Продажи за сегодня".into(), n.to_string(), String::new(), sub)
            }
            None => ("Продажи за сегодня".into(), "...".into(), String::new(), "Загружаю".into()),
        },
        _ => match d.lots {
            Some((on, all)) => ("Лоты".into(), on.to_string(), "активно".into(), if all > on { format!("Из {}, скрыто {}", all, all - on) } else { plural(all, "лот", "лота", "лотов") + " всего" }),
            None => ("Лоты".into(), "...".into(), String::new(), "Загружаю".into()),
        },
    }
}

/// Короткая подпись для компактного вида.
fn short(kind: i32) -> &'static str {
    match kind {
        0 => "Баланс",
        1 => "Непрочитанные",
        2 => "Продажи сегодня",
        _ => "Активные лоты",
    }
}

fn fill(w: &WidgetWindow, kind: i32) {
    let d = data();
    let (label, big, unit, sub) = content(kind, &d);
    let compact = w.get_size() == 0;
    w.set_kind(kind);
    w.set_label(if compact { short(kind).into() } else { label.into() });
    w.set_big(big.into());
    // в компактном у лотов слово "активно" уже в подписи
    w.set_unit(if compact && kind == 3 { "".into() } else { unit.into() });
    w.set_sub(sub.into());
}

fn refresh_all() {
    WIDS.with(|ws| {
        for w in ws.borrow().iter() {
            fill(&w.win, w.kind);
        }
    });
}

/// Вид виджетов: тёмные ли, акцент, размер и непрозрачность фона.
#[derive(Clone, Copy, PartialEq)]
pub struct Look {
    pub dark: bool,
    pub accent: Color,
    pub size: i32,
    pub alpha: f32,
}

impl Look {
    pub fn from(st: &crate::config::WidgetStyle, app_dark: bool, accent: Color) -> Self {
        Self {
            dark: match st.theme {
                1 => false,
                2 => true,
                _ => app_dark,
            },
            accent,
            size: st.size.clamp(0, 2),
            alpha: st.opacity.clamp(0, 100) as f32 / 100.0,
        }
    }

    /// размер окна в логических пикселях
    fn wh(&self) -> (f32, f32) {
        match self.size {
            0 => (200.0, 40.0),
            2 => (W * 1.22, H * 1.22),
            _ => (W, H),
        }
    }
}

fn apply_look(w: &WidgetWindow, l: Look) {
    w.set_dark(l.dark);
    w.set_accent(l.accent);
    w.set_alpha(l.alpha);
}

/// Тема программы поменялась: цвета виджетов тоже (размер так не меняется).
pub fn set_look(l: Look) {
    WIDS.with(|ws| {
        for w in ws.borrow().iter() {
            if w.win.get_size() == l.size {
                apply_look(&w.win, l);
            }
        }
    });
}

/// Рабочая область главного монитора без панели задач, физические пиксели.
#[cfg(windows)]
fn work_area() -> (i32, i32, i32, i32) {
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
fn work_area() -> (i32, i32, i32, i32) {
    (0, 0, 1920, 1080)
}

/// Место по умолчанию: столбиком в правом верхнем углу.
fn default_pos(slot: usize, scale: f32, look: Look) -> (i32, i32) {
    let (_, t, r, _) = work_area();
    let (lw, lh) = look.wh();
    let (w, h, gap, m) = ((lw * scale) as i32, (lh * scale) as i32, (GAP * scale) as i32, (MARGIN * scale) as i32);
    (r - m - w, t + m + slot as i32 * (h + gap))
}

/// Скругление углов окна на Windows 11 (на Windows 10 ничего не делает).
pub fn round_corners(win: &slint::Window) {
    #[cfg(windows)]
    {
        #[link(name = "dwmapi")]
        extern "system" {
            fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const core::ffi::c_void, size: u32) -> i32;
        }
        if let Some(h) = desk::hwnd(win) {
            let pref: u32 = 2; // DWMWCP_ROUND
            unsafe {
                DwmSetWindowAttribute(h, 33, &pref as *const u32 as *const core::ffi::c_void, 4);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = win;
}

/// Рабочий стол Windows: виджеты встают внутрь слоя с обоями, под ярлыки, как живые обои.
#[cfg(windows)]
mod desk {
    type Hwnd = isize;

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        l: i32,
        t: i32,
        r: i32,
        b: i32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn FindWindowW(class: *const u16, name: *const u16) -> Hwnd;
        fn FindWindowExW(parent: Hwnd, after: Hwnd, class: *const u16, name: *const u16) -> Hwnd;
        fn EnumWindows(cb: extern "system" fn(Hwnd, isize) -> i32, l: isize) -> i32;
        fn SendMessageTimeoutW(h: Hwnd, msg: u32, w: usize, l: isize, flags: u32, timeout: u32, res: *mut usize) -> isize;
        fn SetParent(child: Hwnd, parent: Hwnd) -> Hwnd;
        fn GetParent(h: Hwnd) -> Hwnd;
        fn IsWindow(h: Hwnd) -> i32;
        fn SetWindowPos(h: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn GetWindowLongPtrW(h: Hwnd, idx: i32) -> isize;
        fn SetWindowLongPtrW(h: Hwnd, idx: i32, v: isize) -> isize;
        fn GetWindowRect(h: Hwnd, r: *mut Rect) -> i32;
    }

    const GWL_STYLE: i32 = -16;
    const GWL_EXSTYLE: i32 = -20;
    const WS_POPUP: isize = 0x8000_0000u32 as i32 as isize;
    const WS_CHILD: isize = 0x4000_0000;
    const WS_EX_TOOLWINDOW: isize = 0x80;
    const WS_EX_APPWINDOW: isize = 0x40000;
    const WS_EX_NOACTIVATE: isize = 0x0800_0000;
    const SWP_NOSIZE: u32 = 0x1;
    const SWP_NOACTIVATE: u32 = 0x10;
    const SWP_SHOWWINDOW: u32 = 0x40;
    const SWP_FRAMECHANGED: u32 = 0x20;
    const HWND_TOP: Hwnd = 0;
    const HWND_TOPMOST: Hwnd = -1;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn hwnd(win: &slint::Window) -> Option<Hwnd> {
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use slint::winit_030::WinitWindowAccessor;
        win.with_winit_window(|w| match w.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
            _ => None,
        })
        .flatten()
    }

    static mut FOUND: Hwnd = 0;

    extern "system" fn find_defview(h: Hwnd, _: isize) -> i32 {
        let dv = w("SHELLDLL_DefView");
        if unsafe { FindWindowExW(h, 0, dv.as_ptr(), std::ptr::null()) } != 0 {
            unsafe { FOUND = h };
            return 0;
        }
        1
    }

    /// Слой с обоями за ярлыками (WorkerW). Windows создаёт его по особому сообщению Progman.
    pub fn layer() -> Option<Hwnd> {
        let pm = w("Progman");
        let ww = w("WorkerW");
        let progman = unsafe { FindWindowW(pm.as_ptr(), std::ptr::null()) };
        if progman == 0 {
            return None;
        }
        let mut res = 0usize;
        unsafe {
            SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, 0, 1000, &mut res);
            SendMessageTimeoutW(progman, 0x052C, 0, 0, 0, 1000, &mut res);
        }
        // Windows 11 24H2 и новее: слой внутри самого Progman
        let inner = unsafe { FindWindowExW(progman, 0, ww.as_ptr(), std::ptr::null()) };
        if inner != 0 {
            return Some(inner);
        }
        // раньше: отдельное окно WorkerW сразу за окном с ярлыками
        unsafe {
            FOUND = 0;
            EnumWindows(find_defview, 0);
            let icons = FOUND;
            if icons != 0 {
                let layer = FindWindowExW(0, icons, ww.as_ptr(), std::ptr::null());
                if layer != 0 {
                    return Some(layer);
                }
            }
        }
        Some(progman)
    }

    fn style_tool(h: Hwnd) {
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            SetWindowLongPtrW(h, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) & !WS_EX_APPWINDOW);
        }
    }

    /// Встроить окно в рабочий стол в точке (x, y) экрана.
    pub fn embed(h: Hwnd, x: i32, y: i32) -> bool {
        let Some(layer) = layer() else { return false };
        style_tool(h);
        unsafe {
            let st = GetWindowLongPtrW(h, GWL_STYLE);
            SetWindowLongPtrW(h, GWL_STYLE, (st & !WS_POPUP) | WS_CHILD);
            if SetParent(h, layer) == 0 && GetParent(h) != layer {
                SetWindowLongPtrW(h, GWL_STYLE, st);
                return false;
            }
            let mut r = Rect::default();
            GetWindowRect(layer, &mut r);
            SetWindowPos(h, HWND_TOP, x - r.l, y - r.t, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED);
        }
        true
    }

    /// Вернуть обычным окном поверх всех (для перестановки или режима поверх окон).
    pub fn unembed(h: Hwnd, x: i32, y: i32) {
        style_tool(h);
        unsafe {
            if GetParent(h) != 0 {
                SetParent(h, 0);
                let st = GetWindowLongPtrW(h, GWL_STYLE);
                SetWindowLongPtrW(h, GWL_STYLE, (st & !WS_CHILD) | WS_POPUP);
            }
            SetWindowPos(h, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED);
        }
    }

    /// Окно пропало вместе с рабочим столом (например, перезапустили проводник).
    pub fn alive(h: Hwnd) -> bool {
        unsafe { IsWindow(h) != 0 }
    }

    /// Окно сейчас внутри рабочего стола и слой на месте.
    pub fn still_embedded(h: Hwnd) -> bool {
        unsafe {
            let p = GetParent(h);
            p != 0 && IsWindow(p) != 0
        }
    }
}

/// Кусок обоев под виджетом: на рабочем столе он фон окна, и виджет выглядит частью обоев.
fn put_wall(w: &WidgetWindow, pos: (i32, i32), scale: f32) {
    let size = w.window().size();
    let (pw, ph) = if size.width > 1 {
        (size.width, size.height)
    } else {
        let (lw, lh) = match w.get_size() {
            0 => (200.0, 40.0),
            2 => (W * 1.22, H * 1.22),
            _ => (W, H),
        };
        ((lw * scale) as u32, (lh * scale) as u32)
    };
    match crate::wallpaper::crop(pos.0, pos.1, pw, ph) {
        Some(buf) => {
            w.set_wall(slint::Image::from_rgba8(buf));
            w.set_has_wall(true);
        }
        None => w.set_has_wall(false),
    }
}

/// Положить виджет туда, где он должен жить по настройкам.
fn place(w: &mut Wid, scale: f32) {
    let top = TOP.load(Ordering::Relaxed);
    let edit = EDIT.load(Ordering::Relaxed);
    w.win.set_editing(edit && !top);
    w.win.set_interactive(top || edit);
    #[cfg(windows)]
    if let Some(h) = desk::hwnd(w.win.window()) {
        if top || edit {
            desk::unembed(h, w.last_pos.0, w.last_pos.1);
            w.embedded = false;
            w.win.set_has_wall(false);
        } else {
            put_wall(&w.win, w.last_pos, scale);
            w.embedded = desk::embed(h, w.last_pos.0, w.last_pos.1);
            if !w.embedded {
                w.win.set_has_wall(false);
                w.win.set_interactive(true);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = scale;
}

/// Приводит окна к настройкам: создаёт недостающие, закрывает лишние.
/// force пересоздаёт все окна (поменялся размер или место).
pub fn sync(list: &[WidgetCfg], top: bool, look: Look, scale: f32, force: bool) {
    let top_changed = TOP.swap(top, Ordering::Relaxed) != top;
    WIDS.with(|ws| {
        let mut ws = ws.borrow_mut();
        ws.retain(|w| {
            let keep = list.iter().any(|c| c.kind == w.kind) && !top_changed && !force && w.win.get_size() == look.size;
            if !keep {
                let _ = w.win.hide();
            }
            keep
        });
        for w in ws.iter() {
            apply_look(&w.win, look);
        }
    });
    for (slot, c) in list.iter().enumerate() {
        let exists = WIDS.with(|ws| ws.borrow().iter().any(|w| w.kind == c.kind));
        if exists {
            continue;
        }
        let Ok(win) = WidgetWindow::new() else { continue };
        win.set_size(look.size);
        fill(&win, c.kind);
        apply_look(&win, look);
        win.set_shown(false);
        let kind = c.kind;
        win.on_clicked(move || {
            // клик сразу после перетаскивания не считаем
            if DRAG_AT.with(|d| d.borrow().map(|t| t.elapsed() < Duration::from_millis(400)).unwrap_or(false)) {
                return;
            }
            if EDIT.load(Ordering::Relaxed) {
                return;
            }
            crate::ui::post(move |u| u.widget_clicked(kind));
        });
        let weak = win.as_weak();
        win.on_drag(move || {
            if let Some(w) = weak.upgrade() {
                use slint::winit_030::WinitWindowAccessor;
                w.window().with_winit_window(|ww| {
                    let _ = ww.drag_window();
                });
                DRAG_AT.with(|d| *d.borrow_mut() = Some(Instant::now()));
            }
        });
        win.on_closed(move || crate::ui::post(move |u| u.widget_toggle(kind, false)));
        let pos = c.pos.unwrap_or_else(|| default_pos(slot, scale, look));
        win.window().set_position(slint::PhysicalPosition::new(pos.0, pos.1));
        CREATING.store(true, Ordering::Relaxed);
        let _ = win.show();
        CREATING.store(false, Ordering::Relaxed);
        win.window().set_position(slint::PhysicalPosition::new(pos.0, pos.1));
        let mut wid = Wid { kind, win, last_pos: pos, embedded: false };
        // сначала окно встаёт на место (в рабочий стол), и только потом появляется карточка
        place(&mut wid, scale);
        let weak = wid.win.as_weak();
        slint::Timer::single_shot(Duration::from_millis(150), move || {
            if let Some(w) = weak.upgrade() {
                w.set_shown(true);
            }
        });
        WIDS.with(|ws| ws.borrow_mut().push(wid));
    }
    ensure_timer(scale);
}

/// Перестановка: виджеты становятся обычными окнами, по "Готово" возвращаются в рабочий стол.
pub fn set_editing(on: bool, scale: f32) -> Vec<(i32, (i32, i32))> {
    // перед возвратом запоминаем, куда их перетащили
    let moved = if !on { positions() } else { vec![] };
    EDIT.store(on, Ordering::Relaxed);
    WIDS.with(|ws| {
        for w in ws.borrow_mut().iter_mut() {
            if let Some((_, p)) = moved.iter().find(|(k, _)| *k == w.kind) {
                w.last_pos = *p;
            }
            place(w, scale);
        }
    });
    moved
}

fn positions() -> Vec<(i32, (i32, i32))> {
    WIDS.with(|ws| {
        ws.borrow()
            .iter()
            .filter(|w| !w.embedded)
            .map(|w| {
                let p = w.win.window().position();
                (w.kind, (p.x, p.y))
            })
            .collect()
    })
}

/// Раз в полсекунды: запоминаем, куда передвинули виджеты; раз в 10 секунд проверяем обои и рабочий стол.
fn ensure_timer(scale: f32) {
    TIMER.with(|t| {
        if t.borrow().is_some() {
            return;
        }
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, Duration::from_millis(500), move || {
            let n = TICKS.with(|t| {
                let v = t.get() + 1;
                t.set(v);
                v
            });
            // передвинутые окна (только когда они обычные окна)
            let moved: Vec<(i32, (i32, i32))> = WIDS.with(|ws| {
                let mut out = Vec::new();
                for w in ws.borrow_mut().iter_mut() {
                    if w.embedded {
                        continue;
                    }
                    let p = w.win.window().position();
                    let p = (p.x, p.y);
                    if p != w.last_pos {
                        w.last_pos = p;
                        out.push((w.kind, p));
                    }
                }
                out
            });
            if n % 20 == 0 {
                #[cfg(windows)]
                {
                    let wall = crate::wallpaper::changed();
                    let mut lost = false;
                    WIDS.with(|ws| {
                        for w in ws.borrow_mut().iter_mut() {
                            let Some(h) = desk::hwnd(w.win.window()) else { continue };
                            if !desk::alive(h) {
                                lost = true;
                                continue;
                            }
                            // проводник перезапустили или поменяли обои: встраиваем заново
                            if w.embedded && (!desk::still_embedded(h) || wall) {
                                place(w, scale);
                            }
                        }
                    });
                    if lost {
                        crate::ui::post(|u| u.widgets_rebuild());
                    }
                }
            }
            if n % 4 == 0 || !moved.is_empty() {
                // заодно подхватываем тему программы
                crate::ui::post(move |u| u.widgets_moved(moved));
            }
        });
        *t.borrow_mut() = Some(timer);
    });
}

pub fn any() -> bool {
    WIDS.with(|ws| !ws.borrow().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn texts() {
        let d = Data { balance: "12 480 ₽".into(), balance_at: "14:20".into(), unread: 3, unread_names: vec!["Ivan".into(), "keys".into(), "x".into()], sales: Some((5, "1 250 ₽".into(), 2)), lots: Some((42, 50)) };
        assert_eq!(content(0, &d).1, "12 480");
        assert_eq!(content(0, &d).2, "₽");
        assert_eq!(content(1, &d).3, "Ivan, keys и ещё 1");
        assert_eq!(content(2, &d).3, "На 1 250 ₽, ждут выполнения: 2");
        assert_eq!(content(3, &d).3, "Из 50, скрыто 8");
        assert_eq!(plural(21, "лот", "лота", "лотов"), "21 лот");
    }
}
