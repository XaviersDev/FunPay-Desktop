//! Встроенный браузер: WebView2 внутри окна приложения (дочернее окно поверх области чата),
//! с куками аккаунта. Используется для входа и для ссылок FunPay.
//! На других системах ссылки открываются во внешнем браузере.

#[cfg(windows)]
mod imp {
    use crate::ui::post;
    use crate::AppWindow;
    use slint::ComponentHandle;
    use std::cell::RefCell;
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::{NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder};

    struct View {
        id: i32,
        wv: WebView,
    }

    /// Один профиль WebView2 (общие куки) и по вкладке на страницу.
    struct Browser {
        ctx: WebContext,
        views: Vec<View>,
        active: i32,
        rect: (f32, f32, f32, f32),
        visible: bool,
    }

    thread_local! {
        static B: RefCell<Option<Browser>> = const { RefCell::new(None) };
    }

    static LOADER: &[u8] = include_bytes!("../patches/webview2-com-sys-0.39.1/x64/WebView2Loader.dll");

    /// Кладём WebView2Loader.dll в папку приложения и загружаем по полному пути.
    fn ensure_loader() -> bool {
        #[link(name = "kernel32")]
        extern "system" {
            fn LoadLibraryW(name: *const u16) -> isize;
        }
        let p = crate::config::dir().join("WebView2Loader.dll");
        let need = std::fs::metadata(&p).map(|m| m.len() != LOADER.len() as u64).unwrap_or(true);
        if need {
            let _ = std::fs::write(&p, LOADER);
        }
        let w: Vec<u16> = p.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { LoadLibraryW(w.as_ptr()) != 0 }
    }

    const NO_WV2: &str = "Не получилось открыть браузер: на компьютере нет Microsoft Edge WebView2. Установите его с сайта Microsoft или войдите другим способом.";

    fn new_view(app: &AppWindow, ctx: &mut WebContext, id: i32) -> Result<WebView, String> {
        use slint::winit_030::WinitWindowAccessor;
        app.window()
            .with_winit_window(|w| {
                WebViewBuilder::new_with_web_context(ctx)
                    .with_url("about:blank")
                    .with_visible(false)
                    .with_bounds(Rect { position: LogicalPosition::new(0, 0).into(), size: LogicalSize::new(10, 10).into() })
                    .with_document_title_changed_handler(move |t| post(move |u| u.tab_title(id, t)))
                    .with_on_page_load_handler(move |ev, url| {
                        let loading = matches!(ev, PageLoadEvent::Started);
                        post(move |u| u.tab_state(id, url, loading));
                    })
                    .with_new_window_req_handler(move |url, _| {
                        // ссылки «в новом окне» открываем новой вкладкой
                        post(move |u| u.open_url(url));
                        NewWindowResponse::Deny
                    })
                    .build_as_child(w)
                    .map_err(|e| e.to_string())
            })
            .ok_or_else(|| "Окно ещё не готово".to_string())?
    }

    fn set_cookies(wv: &WebView, cookies: &[(String, String)]) {
        for (k, v) in cookies {
            let c = wry::cookie::Cookie::build((k.clone(), v.clone()))
                .domain(".funpay.com")
                .path("/")
                .secure(true)
                .http_only(k == "golden_key" || k == "PHPSESSID")
                .build();
            let _ = wv.set_cookie(&c);
        }
    }

    /// Открывает (или переиспользует) вкладку id и грузит в неё адрес. Вкладка 0 — вход.
    pub fn open_tab(app: &AppWindow, id: i32, url: &str, cookies: &[(String, String)]) -> Result<(), String> {
        let exists = B.with(|b| b.borrow().is_some());
        if !exists {
            if !ensure_loader() {
                return Err(NO_WV2.into());
            }
            let ctx = WebContext::new(Some(crate::config::dir().join("WebView2")));
            B.with(|b| *b.borrow_mut() = Some(Browser { ctx, views: Vec::new(), active: -1, rect: (0.0, 0.0, 0.0, 0.0), visible: false }));
        }
        let has = B.with(|b| b.borrow().as_ref().map(|br| br.views.iter().any(|v| v.id == id)).unwrap_or(false));
        if !has {
            let res = B.with(|b| {
                let mut g = b.borrow_mut();
                let br = g.as_mut().unwrap();
                new_view(app, &mut br.ctx, id)
            });
            let wv = res.map_err(|e| {
                crate::log::write(&format!("webview2: {e}"));
                NO_WV2.to_string()
            })?;
            B.with(|b| b.borrow_mut().as_mut().unwrap().views.push(View { id, wv }));
        }
        B.with(|b| {
            let mut g = b.borrow_mut();
            let br = g.as_mut().unwrap();
            if let Some(v) = br.views.iter().find(|v| v.id == id) {
                set_cookies(&v.wv, cookies);
                let _ = v.wv.load_url(url);
            }
        });
        show_tab(id);
        Ok(())
    }

    /// Старый вход через браузер: вкладка 0.
    pub fn open(app: &AppWindow, url: &str, cookies: &[(String, String)]) -> Result<(), String> {
        open_tab(app, 0, url, cookies)
    }

    /// Делает вкладку активной (остальные прячем при следующей синхронизации).
    pub fn show_tab(id: i32) {
        B.with(|b| {
            let mut g = b.borrow_mut();
            let Some(br) = g.as_mut() else { return };
            if br.active == id {
                return;
            }
            br.active = id;
            for v in &br.views {
                let on = v.id == id && br.visible;
                let _ = v.wv.set_visible(on);
                if on {
                    let r = br.rect;
                    let _ = v.wv.set_bounds(Rect {
                        position: LogicalPosition::new(r.0 as f64, r.1 as f64).into(),
                        size: LogicalSize::new(r.2.max(1.0) as f64, r.3.max(1.0) as f64).into(),
                    });
                }
            }
        });
    }

    pub fn close_tab(id: i32) {
        B.with(|b| {
            let mut g = b.borrow_mut();
            let Some(br) = g.as_mut() else { return };
            if let Some(i) = br.views.iter().position(|v| v.id == id) {
                let v = br.views.remove(i);
                let _ = v.wv.set_visible(false);
                let _ = v.wv.load_url("about:blank");
                drop(v);
            }
            if br.active == id {
                br.active = -1;
            }
        });
    }

    fn with_active(f: impl FnOnce(&WebView)) {
        B.with(|b| {
            if let Some(br) = b.borrow().as_ref() {
                if let Some(v) = br.views.iter().find(|v| v.id == br.active) {
                    f(&v.wv);
                }
            }
        });
    }

    pub fn load(url: &str) {
        with_active(|wv| {
            let _ = wv.load_url(url);
        });
    }

    pub fn script(js: &str) {
        with_active(|wv| {
            let _ = wv.evaluate_script(js);
        });
    }

    pub fn reload() {
        with_active(|wv| {
            let _ = wv.reload();
        });
    }

    /// Положение и видимость берём из окна Slint.
    pub fn sync(app: &AppWindow, show: bool) {
        B.with(|b| {
            let mut g = b.borrow_mut();
            let Some(br) = g.as_mut() else { return };
            let r = (app.get_browser_x(), app.get_browser_y(), app.get_browser_w(), app.get_browser_h());
            let moved = r != br.rect;
            br.rect = r;
            let changed_vis = show != br.visible;
            br.visible = show;
            for v in &br.views {
                let on = show && v.id == br.active;
                if on && (moved || changed_vis) {
                    let _ = v.wv.set_bounds(Rect {
                        position: LogicalPosition::new(r.0 as f64, r.1 as f64).into(),
                        size: LogicalSize::new(r.2.max(1.0) as f64, r.3.max(1.0) as f64).into(),
                    });
                }
                if changed_vis || !on {
                    let _ = v.wv.set_visible(on);
                }
            }
            if changed_vis && !show {
                if let Some(v) = br.views.first() {
                    let _ = v.wv.focus_parent();
                }
            }
        });
    }

    /// Куки funpay.com из браузера (вход через сайт).
    pub fn funpay_cookies() -> Vec<(String, String)> {
        B.with(|b| {
            b.borrow()
                .as_ref()
                .and_then(|br| br.views.first())
                .and_then(|v| v.wv.cookies_for_url("https://funpay.com/").ok())
                .map(|v| v.into_iter().map(|c| (c.name().to_string(), c.value().to_string())).collect())
                .unwrap_or_default()
        })
    }

    pub fn clear_cookies() {
        B.with(|b| {
            if let Some(br) = b.borrow().as_ref() {
                if let Some(v) = br.views.first() {
                    if let Ok(list) = v.wv.cookies() {
                        for c in list {
                            let _ = v.wv.delete_cookie(&c);
                        }
                    }
                }
                for v in &br.views {
                    let _ = v.wv.load_url("about:blank");
                }
            }
        });
    }

    /// Проверяем, установлен ли WebView2 Runtime. Способ из документации Microsoft:
    /// запись EdgeUpdate с непустым полем pv. Иначе встроенный браузер не поднимется.
    pub fn available() -> bool {
        use once_cell::sync::OnceCell;
        static CACHE: OnceCell<bool> = OnceCell::new();
        *CACHE.get_or_init(webview2_installed)
    }

    fn webview2_installed() -> bool {
        use winreg::enums::*;
        use winreg::RegKey;
        const GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
        let spots = [
            (HKEY_LOCAL_MACHINE, format!("SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{GUID}")),
            (HKEY_LOCAL_MACHINE, format!("SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{GUID}")),
            (HKEY_CURRENT_USER, format!("SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{GUID}")),
        ];
        for (root, path) in spots {
            if let Ok(key) = RegKey::predef(root).open_subkey(&path) {
                if let Ok(pv) = key.get_value::<String, _>("pv") {
                    let pv = pv.trim();
                    if !pv.is_empty() && pv != "0.0.0.0" {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::AppWindow;
    pub fn open(_app: &AppWindow, url: &str, _cookies: &[(String, String)]) -> Result<(), String> {
        crate::sys::open_external(url);
        Err("external".into())
    }
    pub fn open_tab(_app: &AppWindow, _id: i32, url: &str, _cookies: &[(String, String)]) -> Result<(), String> {
        crate::sys::open_external(url);
        Err("external".into())
    }
    pub fn show_tab(_id: i32) {}
    pub fn close_tab(_id: i32) {}
    pub fn load(url: &str) {
        crate::sys::open_external(url);
    }
    pub fn script(_js: &str) {}
    pub fn reload() {}
    pub fn sync(_app: &AppWindow, _show: bool) {}
    pub fn funpay_cookies() -> Vec<(String, String)> {
        vec![]
    }
    pub fn clear_cookies() {}
    pub fn available() -> bool {
        false
    }
}

pub use imp::*;
