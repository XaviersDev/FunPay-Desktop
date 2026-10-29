//! Системное: уведомления Windows (клик открывает чат), звук, тема системы, ссылки.

pub const AUMID: &str = "XaviersDev.FunPayTools.Desktop";

static PUSH_MP3: &[u8] = include_bytes!("../assets/funpay_push.mp3");

/// Регистрирует имя и иконку приложения, чтобы в уведомлении было «FunPay Tools», а не PowerShell.
#[cfg(windows)]
pub fn register_app() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let icon = crate::config::dir().join("icon.png");
    // значок в уведомлениях: стрелки FunPay
    let _ = std::fs::write(&icon, ARROWS_PNG);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((key, _)) = hkcu.create_subkey(format!("Software\\Classes\\AppUserModelId\\{AUMID}")) {
        let _ = key.set_value("DisplayName", &"FunPay Tools");
        let _ = key.set_value("IconUri", &icon.to_string_lossy().to_string());
        let _ = key.set_value("IconBackgroundColor", &"FFFFFFFF");
    }
    unsafe {
        let wide: Vec<u16> = AUMID.encode_utf16().chain(std::iter::once(0)).collect();
        SetCurrentProcessExplicitAppUserModelID(wide.as_ptr());
    }
}

#[cfg(windows)]
#[link(name = "shell32")]
extern "system" {
    fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
}

#[cfg(not(windows))]
pub fn register_app() {}

/// Уведомление. Клик по нему вызывает on_click в UI-потоке.
pub fn notify(title: &str, body: &str, on_click: impl Fn() + Send + Sync + 'static) {
    let title = title.to_string();
    let body: String = body.chars().take(240).collect();
    #[cfg(windows)]
    {
        use tauri_winrt_notification::Toast;
        let cb = std::sync::Arc::new(on_click);
        std::thread::spawn(move || {
            let cb2 = cb.clone();
            let _ = Toast::new(AUMID)
                .title(&title)
                .text1(&body)
                .sound(None)
                .on_activated(move |_| {
                    let cb3 = cb2.clone();
                    let _ = slint::invoke_from_event_loop(move || cb3());
                    Ok(())
                })
                .show();
            // обработчик живёт, пока жив поток COM: держим поток немного, чтобы клик успел прийти
            std::thread::sleep(std::time::Duration::from_secs(25));
        });
    }
    #[cfg(not(windows))]
    {
        let _ = on_click;
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new().summary(&title).body(&body).appname("FunPay Tools").show();
        });
    }
}

/// Уведомление Windows со стрелками FunPay слева (или без значка).
pub fn notify_with_icon(title: &str, body: &str, icon: bool, on_click: impl Fn() + Send + Sync + 'static) {
    let title = title.to_string();
    let body: String = body.chars().take(240).collect();
    #[cfg(windows)]
    {
        use tauri_winrt_notification::{IconCrop, Toast};
        let cb = std::sync::Arc::new(on_click);
        let arrows = crate::config::dir().join("arrows.png");
        if icon && !arrows.exists() {
            let _ = std::fs::write(&arrows, ARROWS_PNG);
        }
        std::thread::spawn(move || {
            let cb2 = cb.clone();
            let mut t = Toast::new(AUMID).title(&title).text1(&body).sound(None);
            if icon {
                t = t.icon(&arrows, IconCrop::Square, "FunPay");
            }
            let _ = t
                .on_activated(move |_| {
                    let cb3 = cb2.clone();
                    let _ = slint::invoke_from_event_loop(move || cb3());
                    Ok(())
                })
                .show();
            std::thread::sleep(std::time::Duration::from_secs(25));
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (icon, on_click);
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new().summary(&title).body(&body).appname("FunPay Tools").show();
        });
    }
}

static ARROWS_PNG: &[u8] = include_bytes!("../ui/icons/arrows128.png");

/// Звук уведомления из мобильного приложения (funpay_push.mp3).
pub fn play_sound() {
    #[cfg(windows)]
    std::thread::spawn(|| {
        let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else { return };
        let sink = rodio::Sink::connect_new(stream.mixer());
        if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(PUSH_MP3)) {
            sink.append(src);
            sink.sleep_until_end();
        }
    });
    #[cfg(not(windows))]
    let _ = PUSH_MP3;
}

/// Тёмная ли тема у Windows для приложений.
#[cfg(windows)]
pub fn os_dark() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|v| v == 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn os_dark() -> bool {
    false
}

/// Ссылка в системном браузере.
pub fn open_external(url: &str) {
    if url.is_empty() {
        return;
    }
    let u = url.to_string();
    std::thread::spawn(move || {
        let _ = open::that_detached(u);
    });
}
