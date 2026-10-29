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
