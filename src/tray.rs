//! Работа в фоне: значок в трее, автозапуск с Windows, один экземпляр программы.

/// Что попросили в трее.
pub enum Action {
    Show,
    Quit,
}

#[cfg(any(windows, target_os = "linux"))]
mod imp {
    use super::Action;
    use std::cell::RefCell;
    use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    struct Tray {
        _icon: TrayIcon,
        open: MenuId,
        quit: MenuId,
    }

    thread_local! {
        static T: RefCell<Option<Tray>> = const { RefCell::new(None) };
    }

    static LOGO: &[u8] = include_bytes!("../ui/icons/logo64.png");

    pub fn init() {
        #[cfg(target_os = "linux")]
        {
            if gtk::init().is_err() {
                crate::log::write("трей: не удалось инициализировать GTK");
                return;
            }
        }
        let Ok(img) = image::load_from_memory(LOGO) else { return };
        let img = img.resize(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8();
        let (w, h) = (img.width(), img.height());
        let Ok(icon) = Icon::from_rgba(img.into_raw(), w, h) else { return };
        let menu = Menu::new();
        let open = MenuItem::new("Открыть FunPay Tools", true, None);
        let quit = MenuItem::new("Выход", true, None);
        let _ = menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit]);
        let built = TrayIconBuilder::new()
            .with_tooltip("FunPay Tools")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build();
        match built {
            Ok(t) => T.with(|c| *c.borrow_mut() = Some(Tray { _icon: t, open: open.id().clone(), quit: quit.id().clone() })),
            Err(e) => crate::log::write(&format!("трей: {e}")),
        }
    }

    pub fn available() -> bool {
        T.with(|c| c.borrow().is_some())
    }

    pub fn poll() -> Option<Action> {
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }

        let ids = T.with(|c| c.borrow().as_ref().map(|t| (t.open.clone(), t.quit.clone())))?;
        if let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == ids.1 {
                return Some(Action::Quit);
            }
            if ev.id == ids.0 {
                return Some(Action::Show);
            }
        }
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            match ev {
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => return Some(Action::Show),
                TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => return Some(Action::Show),
                _ => {}
            }
        }
        None
    }

    pub fn remove() {
        T.with(|c| *c.borrow_mut() = None);
    }

    /// Автозапуск через реестр Windows: программа стартует в фоне.
    #[cfg(windows)]
    pub fn set_autostart(on: bool) {
        use winreg::enums::*;
        use winreg::RegKey;
        let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") else { return };
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let _ = key.set_value("FunPayTools", &format!("\"{}\" --background", exe.display()));
            }
        } else {
            let _ = key.delete_value("FunPayTools");
        }
    }

    /// Автозапуск на Linux через ~/.config/autostart.
    #[cfg(target_os = "linux")]
    pub fn set_autostart(on: bool) {
        if let Some(base) = directories::BaseDirs::new() {
            let dir = base.config_dir().join("autostart");
            let file = dir.join("funpay-tools.desktop");
            if on {
                let _ = std::fs::create_dir_all(&dir);
                if let Ok(exe) = std::env::current_exe() {
                    let icon = crate::config::dir().join("icon.png");
                    let content = format!(
                        "[Desktop Entry]\n\
                         Type=Application\n\
                         Name=FunPay Tools\n\
                         Comment=FunPay Desktop Client\n\
                         Exec=\"{}\" --background\n\
                         Icon={}\n\
                         Terminal=false\n\
                         Categories=Network;Chat;InstantMessaging;\n\
                         StartupWMClass=funpay-tools\n\
                         X-GNOME-Autostart-enabled=true\n",
                        exe.display(),
                        icon.display()
                    );
                    let _ = std::fs::write(file, content);
                }
            } else {
                let _ = std::fs::remove_file(file);
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::Action;
    pub fn init() {}
    pub fn available() -> bool {
        false
    }
    pub fn poll() -> Option<Action> {
        None
    }
    pub fn remove() {}
    pub fn set_autostart(_on: bool) {}
}

pub use imp::*;

// ------------------------------------------------------------ один экземпляр

const PORT: u16 = 48217;

/// Если программа уже запущена, просим её показать окно и возвращаем true.
pub fn signal_running() -> bool {
    use std::io::Write;
    match std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], PORT)), std::time::Duration::from_millis(300)) {
        Ok(mut s) => {
            let _ = s.write_all(b"show");
            true
        }
        Err(_) => false,
    }
}

/// Слушаем второй запуск: он присылает «show», показываем окно.
pub fn listen(on_show: impl Fn() + Send + 'static) {
    let Ok(l) = std::net::TcpListener::bind(("127.0.0.1", PORT)) else { return };
    std::thread::spawn(move || {
        use std::io::Read;
        for mut c in l.incoming().flatten() {
            let mut buf = [0u8; 8];
            let n = c.read(&mut buf).unwrap_or(0);
            if &buf[..n] == b"show" {
                on_show();
            }
        }
    });
}

/// После обновления: ждём, пока старая копия программы закроется и освободит порт.
pub fn wait_free(secs: u64) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < until {
        if std::net::TcpListener::bind(("127.0.0.1", PORT)).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autostart_toggle() {
        set_autostart(true);
        #[cfg(target_os = "linux")]
        {
            let base = directories::BaseDirs::new().unwrap();
            let p = base.config_dir().join("autostart/funpay-tools.desktop");
            assert!(p.exists());
        }
        set_autostart(false);
        #[cfg(target_os = "linux")]
        {
            let base = directories::BaseDirs::new().unwrap();
            let p = base.config_dir().join("autostart/funpay-tools.desktop");
            assert!(!p.exists());
        }
    }
}
