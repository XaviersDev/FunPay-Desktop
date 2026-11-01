//! Системное: уведомления Windows (клик открывает чат), звук, тема системы, ссылки.

#[cfg(windows)]
pub const AUMID: &str = "XaviersDev.FunPayTools.Desktop";

static PUSH_MP3: &[u8] = include_bytes!("../assets/funpay_push.mp3");

/// Регистрирует имя и иконку приложения.
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

#[cfg(target_os = "linux")]
pub fn register_app() {
    let icon = crate::config::dir().join("icon.png");
    let _ = std::fs::write(&icon, ARROWS_PNG);

    if let Some(home) = directories::BaseDirs::new() {
        let icons_dir = home.data_local_dir().join("icons/hicolor/128x128/apps");
        let _ = std::fs::create_dir_all(&icons_dir);
        let _ = std::fs::write(icons_dir.join("funpay-tools.png"), ARROWS_PNG);

        let apps_dir = home.data_local_dir().join("applications");
        let _ = std::fs::create_dir_all(&apps_dir);
        let desktop_file = apps_dir.join("funpay-tools.desktop");
        if let Ok(exe) = std::env::current_exe() {
            let content = format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=FunPay Tools\n\
                 Comment=FunPay Desktop Client\n\
                 Exec=\"{}\" %u\n\
                 Icon={}\n\
                 Terminal=false\n\
                 Categories=Network;Chat;InstantMessaging;\n\
                 StartupWMClass=funpay-tools\n",
                exe.display(),
                icon.display()
            );
            let _ = std::fs::write(desktop_file, content);
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn register_app() {}

/// Уведомление. Клик по нему вызывает on_click в UI-потоке.
pub fn notify(title: &str, body: &str, on_click: impl Fn() + Send + Sync + 'static) {
    notify_with_icon(title, body, false, on_click);
}

/// Уведомление со стрелками FunPay слева (или без значка).
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
        let arrows = crate::config::dir().join("arrows.png");
        if icon && !arrows.exists() {
            let _ = std::fs::write(&arrows, ARROWS_PNG);
        }
        std::thread::spawn(move || {
            let mut notif = notify_rust::Notification::new();
            notif.summary(&title)
                .body(&body)
                .appname("FunPay Tools")
                .hint(notify_rust::Hint::DesktopEntry("funpay-tools".into()))
                .action("default", "Открыть");

            if icon && arrows.exists() {
                notif.icon(arrows.to_str().unwrap_or("funpay-tools"));
            } else {
                notif.icon("funpay-tools");
            }

            if let Ok(handle) = notif.show() {
                handle.wait_for_action(move |action| {
                    if action == "default" {
                        let _ = slint::invoke_from_event_loop(move || on_click());
                    }
                });
            }
        });
    }
}

static ARROWS_PNG: &[u8] = include_bytes!("../ui/icons/arrows128.png");

/// Звук уведомления из мобильного приложения (funpay_push.mp3).
pub fn play_sound() {
    std::thread::spawn(|| {
        let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else { return };
        let sink = rodio::Sink::connect_new(stream.mixer());
        if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(PUSH_MP3)) {
            sink.append(src);
            sink.sleep_until_end();
        }
    });
}

/// Тёмная ли тема у ОС для приложений.
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
    if let Ok(val) = std::env::var("GTK_THEME") {
        if val.to_lowercase().contains("dark") {
            return true;
        }
    }
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
        if s.contains("dark") {
            return true;
        }
        if s.contains("light") {
            return false;
        }
    }
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
        if s.contains("dark") {
            return true;
        }
    }
    if let Ok(out) = std::process::Command::new("busctl")
        .args([
            "--user",
            "call",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "Read",
            "ss",
            "org.freedesktop.appearance",
            "color-scheme",
        ])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        if s.ends_with("1\n") || s.contains("u 1") {
            return true;
        }
        if s.ends_with("2\n") || s.contains("u 2") {
            return false;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_dark() {
        let _ = os_dark();
    }

    #[test]
    fn test_register_app() {
        register_app();
        assert!(crate::config::dir().join("icon.png").exists());
    }
}
