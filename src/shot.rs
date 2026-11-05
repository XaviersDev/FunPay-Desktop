//! Скриншот без окна: FPT_SHOT="путь.png:ШxВ:сцена[:dark]". Нужен для проверки вёрстки.

use crate::AppWindow;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::Rgb8Pixel;
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, PhysicalSize};
use std::rc::Rc;

struct ShotPlatform(Rc<MinimalSoftwareWindow>);

impl Platform for ShotPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

pub fn run(spec: &str) {
    let parts: Vec<&str> = spec.split(':').collect();
    let path = parts.first().copied().unwrap_or("shot.png");
    let (w, h) = parts
        .get(1)
        .and_then(|s| s.split_once('x'))
        .map(|(a, b)| (a.parse().unwrap_or(1180), b.parse().unwrap_or(760)))
        .unwrap_or((1180u32, 760u32));
    let scene = parts.get(2).copied().unwrap_or("main");
    let dark = parts.get(3).map(|s| *s == "dark").unwrap_or(false);

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(PhysicalSize::new(w, h));
    slint::platform::set_platform(Box::new(ShotPlatform(window.clone()))).unwrap();

    // виджет рабочего стола: widget0..widget3, размер задаёт масштаб
    if let Some(k) = scene.strip_prefix("widget") {
        let size = if k.ends_with('c') { 0 } else if k.ends_with('l') { 2 } else { 1 };
        let kind: i32 = k.trim_end_matches(['c', 'l']).parse().unwrap_or(0);
        let wdg = crate::WidgetWindow::new().unwrap();
        wdg.set_size(size);
        let base = match size {
            0 => 200.0,
            2 => 216.0 * 1.22,
            _ => 216.0,
        };
        window.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: w as f32 / base });
        let d = crate::widgets::Data {
            balance: "12 480 ₽".into(),
            balance_at: "14:20".into(),
            unread: 3,
            unread_names: vec!["vlad_storm".into(), "darkness21".into(), "k1rill".into()],
            sales: Some((7, "4 230 ₽".into(), 2)),
            lots: Some((42, 50)),
        };
        let (label, big, unit, sub) = crate::widgets::content(kind, &d);
        let label = if size == 0 { ["Баланс", "Непрочитанные", "Продажи сегодня", "Активные лоты"][kind.clamp(0, 3) as usize].to_string() } else { label };
        let unit = if size == 0 && kind == 3 { String::new() } else { unit };
        wdg.set_label(label.into());
        wdg.set_big(big.into());
        wdg.set_unit(unit.into());
        wdg.set_sub(sub.into());
        wdg.set_dark(dark);
        wdg.set_shown(true);
        wdg.set_accent(slint::Color::from_rgb_u8(0x2D, 0x8C, 0xFF));
        wdg.show().unwrap();
        let mut buf = vec![Rgb8Pixel::default(); (w * h) as usize];
        for _ in 0..4 {
            slint::platform::update_timers_and_animations();
            window.request_redraw();
            window.draw_if_needed(|r| {
                r.render(&mut buf, w as usize);
            });
        }
        let mut img = image::RgbImage::new(w, h);
        for (i, p) in buf.iter().enumerate() {
            img.put_pixel(i as u32 % w, i as u32 / w, image::Rgb([p.r, p.g, p.b]));
        }
        img.save(path).unwrap();
        return;
    }

    let app = AppWindow::new().unwrap();
    let cfg = std::sync::Arc::new(std::sync::Mutex::new(crate::config::Config { theme: if dark { 2 } else { 1 }, ..Default::default() }));
    let (tx, _rx) = std::sync::mpsc::channel();
    crate::setup(&app, cfg, tx, true);
    let st = app.global::<crate::St>();
    match scene {
        "login" => {
            st.set_logged_in(false);
        }
        "typing" => crate::demo::fill("main"),
        _ => {
            crate::demo::fill(scene);
            crate::demo::scene(scene);
        }
    }
    if scene == "settings" {
        st.set_settings_open(true);
    }
    if scene == "unread" {
        app.global::<crate::Api>().invoke_select_folder("unread".into());
    }
    if scene == "common" {
        crate::ui::with_ui(|u| u.close_chat());
        app.global::<crate::Api>().invoke_select_folder("common".into());
    }
    if scene == "list" || scene == "narrowlist" {
        crate::ui::with_ui(|u| u.close_chat());
    }
    app.show().unwrap();
    if scene == "pluginoff" {
        app.window().dispatch_event(slint::platform::WindowEvent::PointerMoved { position: slint::LogicalPosition::new(520.0, 400.0) });
    }
    if scene == "typing" {
        use slint::platform::{Key, WindowEvent};
        let win = app.window();
        let render = |n: usize| {
            for _ in 0..n {
                slint::platform::update_timers_and_animations();
                window.request_redraw();
                window.draw_if_needed(|_| {});
            }
        };
        render(2);
        app.invoke_focus_input();
        let typ = |t: &str| {
            for ch in t.chars() {
                let s: slint::SharedString = ch.to_string().into();
                win.dispatch_event(WindowEvent::KeyPressed { text: s.clone() });
                win.dispatch_event(WindowEvent::KeyReleased { text: s });
            }
        };
        typ("Первая строка");
        win.dispatch_event(WindowEvent::KeyPressed { text: Key::Shift.into() });
        win.dispatch_event(WindowEvent::KeyPressed { text: Key::Return.into() });
        win.dispatch_event(WindowEvent::KeyReleased { text: Key::Return.into() });
        win.dispatch_event(WindowEvent::KeyReleased { text: Key::Shift.into() });
        typ("вторая строка");
        render(2);
        if std::env::var("FPT_SEND").is_ok() {
            win.dispatch_event(WindowEvent::KeyPressed { text: Key::Return.into() });
            win.dispatch_event(WindowEvent::KeyReleased { text: Key::Return.into() });
            render(2);
            let n = crate::ui::with_ui(|u| u.pending.iter().map(|p| format!("{:?}", p.text)).collect::<Vec<_>>()).unwrap();
            eprintln!("pending: {n:?}");
        }
    }
    let mut buf = vec![Rgb8Pixel::default(); (w * h) as usize];
    for _ in 0..8 {
        std::thread::sleep(std::time::Duration::from_millis(60));
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|r| {
            r.render(&mut buf, w as usize);
        });
        app.invoke_scroll_bottom();
    }
    let mut img = image::RgbImage::new(w, h);
    for (i, p) in buf.iter().enumerate() {
        img.put_pixel(i as u32 % w, i as u32 / w, image::Rgb([p.r, p.g, p.b]));
    }
    img.save(path).unwrap();
    eprintln!("saved {path}");
}
