#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ai;
mod album;
mod browser;
mod cache;
mod config;
mod epic;
mod feat;
mod popup;
mod templates;
mod update;
mod demo;
mod desc;
mod fp;
mod fpt;
mod gchat;
mod imgcache;
mod log;
mod money;
mod parse;
mod shot;
mod stickers;
mod sys;
mod tgtheme;
mod tour;
mod tray;
mod ui;
mod worker;

slint::include_modules!();

use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use ui::{post, with_ui, Ui};
use worker::Cmd;

fn main() {
    if let Ok(spec) = std::env::var("FPT_SHOT") {
        shot::run(&spec);
        return;
    }
    // уже запущены (например, автозапуском): просто показываем то окно
    // после обновления старая копия ещё закрывается: ждём её, а не будим
    if std::env::args().any(|a| a == "--after-update") {
        tray::wait_free(20);
    }
    if std::env::var("FPT_DEMO").is_err() && tray::signal_running() {
        return;
    }
    update::cleanup();
    let start_hidden = std::env::args().any(|a| a == "--background");
    sys::register_app();

    // окно без системной рамки; системная тень только по FPT_SHADOW=1
    let _ = slint::BackendSelector::new()
        .with_winit_window_attributes_hook(|attrs| {
            // окно уведомления: без рамки, поверх всех, без кнопки на панели задач и без фокуса
            if popup::creating() {
                use slint::winit_030::winit::window::WindowLevel;
                let a = attrs.with_decorations(false).with_window_level(WindowLevel::AlwaysOnTop).with_active(false).with_resizable(false);
                #[cfg(windows)]
                {
                    use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
                    return a.with_skip_taskbar(true);
                }
                #[allow(unreachable_code)]
                return a;
            }
            #[cfg(windows)]
            if std::env::var("FPT_SHADOW").is_ok() {
                use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
                return attrs.with_undecorated_shadow(true);
            }
            #[allow(unreachable_code)]
            attrs
        })
        .select();

    let cfg = Arc::new(Mutex::new(config::Config::load()));
    let app = AppWindow::new().expect("не удалось создать окно");
    let demo = std::env::var("FPT_DEMO").is_ok();
    let (tx, rx) = std::sync::mpsc::channel::<Cmd>();

    setup(&app, cfg.clone(), tx, demo);

    if demo {
        let scene = std::env::var("FPT_DEMO").unwrap_or_default();
        demo::fill(&scene);
        let st = app.global::<St>();
        match scene.as_str() {
            "settings" => st.set_settings_open(true),
            "common" => {
                with_ui(|u| {
                    u.close_chat();
                    u.select_folder("common".into());
                });
            }
            "login" => st.set_logged_in(false),
            "login2" => {
                st.set_logged_in(false);
                st.set_login_page(1);
            }
            "login3" => {
                st.set_logged_in(false);
                st.set_login_page(2);
            }
            other => demo::scene(other),
        }
    } else {
        let wcfg = cfg.clone();
        std::thread::Builder::new()
            .name("funpay".into())
            .spawn(move || worker::run(rx, wcfg))
            .expect("worker");
    }

    {
        let c = cfg.lock().unwrap();
        if c.win_w >= 240.0 && c.win_h >= 170.0 {
            app.window().set_size(slint::LogicalSize::new(c.win_w, c.win_h));
        }
    }
    let wcfg = cfg.clone();
    let weak = app.as_weak();
    app.window().on_close_requested(move || {
        save_size(&weak, &wcfg);
        close_or_hide(&wcfg);
        slint::CloseRequestResponse::HideWindow
    });

    {
        use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
        app.window().on_winit_window_event(|w, ev| {
            match ev {
                winit::event::WindowEvent::Focused(f) => {
                    let f = *f;
                    with_ui(|u| {
                        u.focused = f;
                        u.send(Cmd::Focus(f));
                        if f {
                            u.apply_theme();
                        }
                    });
                }
                winit::event::WindowEvent::Resized(_) => {
                    let max = w.with_winit_window(|ww| ww.is_maximized()).unwrap_or(false);
                    with_ui(|u| {
                        u.st(|s| {
                            if s.get_maximized() != max {
                                s.set_maximized(max);
                            }
                        });
                    });
                }
                _ => {}
            }
            EventResult::Propagate
        });
    }

    let slow = slint::Timer::default();
    slow.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(3), || {
        with_ui(|u| {
            u.sweep_pending();
            u.apply_theme();
        });
    });
    let fast = slint::Timer::default();
    fast.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(160), || {
        with_ui(|u| u.browser_tick());
        match tray::poll() {
            Some(tray::Action::Show) => {
                with_ui(|u| u.show_window());
            }
            Some(tray::Action::Quit) => quit_app(),
            None => {}
        }
    });

    // фон: значок в трее, автозапуск, второй запуск показывает это окно
    tray::init();
    {
        let c = cfg.lock().unwrap();
        tray::set_autostart(c.autostart);
    }
    tray::listen(|| ui::post(|u| u.show_window()));

    if !(start_hidden && tray::available()) {
        app.show().expect("окно");
    }
    slint::run_event_loop_until_quit().expect("event loop");
    tray::remove();
}

/// Закрытие окна: в фоне (если включено и есть трей) или полный выход.
fn close_or_hide(cfg: &Arc<Mutex<config::Config>>) {
    let keep = cfg.lock().unwrap().keep_background && tray::available();
    if !keep {
        quit_app();
        return;
    }
    with_ui(|u| u.hide_to_background());
}

fn quit_app() {
    tray::remove();
    let _ = slint::quit_event_loop();
}

fn save_size(weak: &slint::Weak<AppWindow>, cfg: &Arc<Mutex<config::Config>>) {
    if let Some(a) = weak.upgrade() {
        use slint::winit_030::WinitWindowAccessor;
        let max = a.window().with_winit_window(|w| w.is_maximized()).unwrap_or(false);
        if !max {
            let s = a.window().size().to_logical(a.window().scale_factor());
            let mut c = cfg.lock().unwrap();
            c.win_w = s.width;
            c.win_h = s.height;
            c.save();
        }
    }
}

pub fn setup(app: &AppWindow, cfg: Arc<Mutex<config::Config>>, tx: std::sync::mpsc::Sender<Cmd>, demo: bool) {
    let chats_model = Rc::new(VecModel::<ChatRow>::default());
    let msgs_model = Rc::new(VecModel::<MsgRow>::default());
    let st = app.global::<St>();
    st.set_chats(ModelRc::from(chats_model.clone()));
    st.set_messages(ModelRc::from(msgs_model.clone()));
    st.set_version(env!("CARGO_PKG_VERSION").into());
    {
        let c = cfg.lock().unwrap();
        st.set_theme_mode(c.theme);
        st.set_notifications(c.notifications);
        st.set_sound(c.sound);
        st.set_on_top(c.always_on_top);
        st.set_show_flood(c.show_flood);
        st.set_show_gchat(c.show_gchat);
        st.set_logged_in(!c.golden_key.is_empty() || demo);
        st.set_chats_loading(true);
        st.set_webview_ok(browser::available());
        st.set_keep_background(c.keep_background);
        st.set_autostart(c.autostart);
        st.set_tray_ok(cfg!(windows));
    }

    let loader = imgcache::ImgLoader::new(4, |d| {
        post(move |u| {
            let img = Image::from_rgba8(d.buf);
            u.image_ready(d.key, img, d.w, d.h);
        });
    });

    ui::install(Ui {
        app: app.as_weak(),
        tx,
        cfg: cfg.clone(),
        loader,
        imgs: Default::default(),
        chats: vec![],
        me: Default::default(),
        folder: "all".into(),
        search: String::new(),
        cur: None,
        pending: vec![],
        games: ui::load_games(),
        chats_model,
        msgs_model,
        next_local: 0,
        gen: 0,
        viewer_url: String::new(),
        viewer_list: vec![],
        gchat_visible: true,
        toast_n: 0,
        focused: true,
        demo,
        reply: None,
        panes: vec![],
        profile: None,
        pf_gen: 0,
        card_url: String::new(),
        order: None,
        od_gen: 0,
        sendbox: vec![],
        stickers: None,
        stickers_loading: false,
        login_poll: 0,
        ext: Default::default(),
    });
    with_ui(|u| {
        u.apply_theme();
        u.rebuild_chats();
    });

    let api = app.global::<Api>();

    // окно
    let w = app.as_weak();
    api.on_win_drag(move || {
        if let Some(a) = w.upgrade() {
            use slint::winit_030::WinitWindowAccessor;
            a.window().with_winit_window(|ww| {
                let _ = ww.drag_window();
            });
        }
    });
    let w = app.as_weak();
    api.on_win_min(move || {
        if let Some(a) = w.upgrade() {
            use slint::winit_030::WinitWindowAccessor;
            a.window().with_winit_window(|ww| ww.set_minimized(true));
        }
    });
    let w = app.as_weak();
    api.on_win_max(move || {
        if let Some(a) = w.upgrade() {
            use slint::winit_030::WinitWindowAccessor;
            let max = a.window().with_winit_window(|ww| {
                let m = !ww.is_maximized();
                ww.set_maximized(m);
                m
            });
            if let Some(m) = max {
                a.global::<St>().set_maximized(m);
            }
        }
    });
    let w = app.as_weak();
    let ccfg = cfg.clone();
    api.on_win_close(move || {
        save_size(&w, &ccfg);
        close_or_hide(&ccfg);
    });
    let w = app.as_weak();
    api.on_win_resize(move |dir| {
        if let Some(a) = w.upgrade() {
            use slint::winit_030::winit::window::ResizeDirection as D;
            use slint::winit_030::WinitWindowAccessor;
            let d = match dir {
                0 => D::North,
                1 => D::South,
                2 => D::East,
                3 => D::West,
                4 => D::NorthEast,
                5 => D::NorthWest,
                6 => D::SouthEast,
                _ => D::SouthWest,
            };
            a.window().with_winit_window(|ww| {
                let _ = ww.drag_resize_window(d);
            });
        }
    });

    // вход
    api.on_login_browser(|| {
        with_ui(|u| u.login_browser());
    });
    api.on_login_key(|gk| {
        with_ui(|u| {
            if gk.trim().is_empty() {
                return;
            }
            u.login_busy(true);
            u.send(Cmd::Login(gk.to_string()));
        });
    });
    api.on_login_import(|| {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Файл бэкапа FunPay Tools")
            .add_filter("Бэкап (.json, .txt)", &["json", "txt", "fpt", "bak"])
            .add_filter("Все файлы", &["*"])
            .pick_file()
        else {
            return;
        };
        let res = std::fs::read_to_string(&path).ok().and_then(|s| gk_from_backup(&s));
        with_ui(|u| match res {
            Some(gk) => {
                u.login_busy(true);
                u.send(Cmd::Login(gk));
            }
            None => {
                u.st(|s| s.set_login_error("Не нашёл golden_key в этом файле. Нужен бэкап из приложения FunPay Tools.".into()));
            }
        });
    });
    api.on_logout(|| {
        with_ui(|u| u.logout());
    });
    api.on_switch_account(|id| {
        with_ui(|u| u.switch_account(id.to_string()));
    });
    api.on_add_account(|| {
        with_ui(|u| u.add_account());
    });
    api.on_cancel_add_account(|| {
        with_ui(|u| u.cancel_add_account());
    });
    api.on_remove_account(|id| {
        with_ui(|u| u.remove_account(id.to_string()));
    });

    // список
    api.on_select_folder(|f| {
        with_ui(|u| u.select_folder(f.to_string()));
    });
    api.on_search_changed(|q| {
        with_ui(|u| {
            u.search = q.to_string();
            u.rebuild_chats();
        });
    });
    api.on_chat_menu(|id, kind, action, arg| {
        with_ui(|u| u.chat_menu(id.to_string(), kind, action.to_string(), arg.to_string()));
    });
    api.on_create_folder(|n| {
        with_ui(|u| u.create_folder(n.to_string()));
    });
    api.on_delete_folder(|id| {
        with_ui(|u| u.delete_folder(id.to_string()));
    });
    api.on_open_chat(|id, kind| {
        with_ui(|u| {
            if u.cur.as_ref().map(|c| c.id == id.as_str()).unwrap_or(false) {
                u.panes.clear();
                u.st(|s| s.set_pane(0));
                return;
            }
            u.open_chat(id.to_string(), kind)
        });
    });
    api.on_close_chat(|| {
        with_ui(|u| u.close_chat());
    });

    // переписка
    api.on_send_text(|t| {
        with_ui(|u| u.send_text(t.to_string()));
    });
    api.on_attach(|| {
        let files = rfd::FileDialog::new()
            .set_title("Отправить картинки")
            .add_filter("Картинки", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
            .pick_files();
        if let Some(files) = files {
            with_ui(|u| u.sendbox_add(files));
        }
    });
    api.on_paste_image(paste_image);
    api.on_paste_clipboard(paste_clipboard);
    api.on_copy_range(|t, a, b| {
        let part = byte_slice(&t, a, b);
        let ok = !part.is_empty() && set_clipboard_text(&part);
        with_ui(|u| u.toast(if ok { "Скопировано" } else { "Не получилось скопировать" }));
    });
    api.on_cut_range(|t, a, b| {
        let (lo, hi) = (a.min(b).max(0) as usize, a.max(b).max(0) as usize);
        if lo == hi || hi > t.len() || !t.is_char_boundary(lo) || !t.is_char_boundary(hi) {
            return t;
        }
        if set_clipboard_text(&t[lo..hi]) {
            format!("{}{}", &t[..lo], &t[hi..]).into()
        } else {
            t
        }
    });
    api.on_load_older(|| {
        with_ui(|u| u.load_older());
    });
    api.on_msg_action(|k, a, _arg| {
        with_ui(|u| u.msg_action(k.to_string(), a.to_string()));
    });
    api.on_cancel_reply(|| {
        with_ui(|u| u.cancel_reply());
    });
    api.on_jump_to(|k| {
        with_ui(|u| u.jump_to(k.to_string()));
    });
    api.on_copy_text(|t| {
        let ok = set_clipboard_text(t.trim_end_matches('\u{00A0}'));
        with_ui(|u| u.toast(if ok { "Скопировано" } else { "Не получилось скопировать" }));
    });
    api.on_retry(|k| {
        with_ui(|u| u.retry(k.to_string()));
    });
    api.on_undismiss_fpt(|| {
        with_ui(|u| u.undismiss_fpt());
    });
    api.on_dismiss_fpt(|| {
        with_ui(|u| u.dismiss_fpt());
    });
    api.on_gchat_login(|| {
        with_ui(|u| {
            u.gchat_status("Открываю бота…".into());
            u.send(Cmd::GChatLogin);
        });
    });
    api.on_open_card(|| {
        with_ui(|u| u.open_card());
    });

    // стикеры и окно отправки
    api.on_stickers_toggle(|| {
        with_ui(|u| u.stickers_toggle());
    });
    api.on_sticker_pack(|i| {
        with_ui(|u| u.sticker_pack(i));
    });
    api.on_sticker_send(|k| {
        with_ui(|u| u.sticker_send(k.to_string()));
    });
    api.on_sendbox_add(|| {
        let files = rfd::FileDialog::new()
            .set_title("Добавить картинки")
            .add_filter("Картинки", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
            .pick_files();
        if let Some(files) = files {
            with_ui(|u| u.sendbox_add(files));
        }
    });
    api.on_sendbox_remove(|i| {
        with_ui(|u| u.sendbox_remove(i.max(0) as usize));
    });
    api.on_sendbox_send(|c| {
        with_ui(|u| u.sendbox_send(c.to_string()));
    });
    api.on_sendbox_cancel(|| {
        with_ui(|u| u.sendbox_cancel());
    });

    // картинки
    api.on_open_image(|url| {
        with_ui(|u| u.open_image(url.to_string()));
    });
    api.on_viewer_step(|d| {
        with_ui(|u| u.viewer_step(d));
    });
    api.on_save_image(|url| save_image(url.to_string()));
    api.on_copy_image(|url| copy_image(url.to_string()));

    // экраны
    api.on_open_profile(|id| {
        with_ui(|u| u.open_profile(id.to_string()));
    });
    api.on_open_order(|id| {
        with_ui(|u| u.open_order(id.to_string()));
    });
    api.on_open_deals(|t| {
        with_ui(|u| u.open_deals(t));
    });
    api.on_deals_tab(|t| {
        with_ui(|u| u.deals_tab(t));
    });
    api.on_deals_filter(|f| {
        with_ui(|u| u.deals_filter(f.to_string()));
    });
    api.on_deals_more(|| {
        with_ui(|u| u.deals_more());
    });
    api.on_tab_select(|i| {
        with_ui(|u| u.tab_select(i));
    });
    api.on_tab_close(|i| {
        with_ui(|u| u.tab_close(i));
    });
    api.on_mini_collapse(|c| {
        with_ui(|u| u.mini_collapse(c));
    });
    api.on_mini_moved(|| {
        with_ui(|u| u.browser_tick());
    });
    api.on_mini_close_all(|| {
        with_ui(|u| u.mini_close_all());
    });
    api.on_set_keep_background(|v| {
        set_cfg(|c| c.keep_background = v);
    });
    api.on_set_autostart(|v| {
        set_cfg(|c| c.autostart = v);
        tray::set_autostart(v);
    });
    api.on_import_theme(|| {
        with_ui(|u| u.import_theme());
    });
    api.on_reset_theme(|| {
        with_ui(|u| u.reset_theme());
    });
    api.on_desc_edit(|| {
        with_ui(|u| u.desc_edit());
    });
    api.on_desc_save(|t| {
        with_ui(|u| u.desc_save(t.to_string()));
    });
    api.on_open_log(|| {
        let _ = std::fs::OpenOptions::new().create(true).append(true).open(log::path());
        sys::open_external(&log::path().to_string_lossy());
    });
    api.on_pane_back(|| {
        with_ui(|u| u.pane_back());
    });
    api.on_pf_more_reviews(|| {
        with_ui(|u| u.pf_more_reviews());
    });
    api.on_pf_write(|| {
        with_ui(|u| u.pf_write());
    });
    api.on_od_action(|a, t, r| {
        with_ui(|u| u.od_action(a.to_string(), t.to_string(), r));
    });

    // браузер
    api.on_open_url(|u| {
        let url = u.to_string();
        with_ui(|ui| ui.open_url(url));
    });
    api.on_open_external(|u| sys::open_external(&u));
    api.on_browser_back(|| browser::script("history.back()"));
    api.on_browser_forward(|| browser::script("history.forward()"));
    api.on_browser_reload(browser::reload);
    api.on_browser_close(|| {
        with_ui(|u| u.browser_close());
    });

    // настройки
    api.on_change_avatar(change_avatar);
    api.on_set_theme(|m| {
        with_ui(|u| {
            {
                let mut c = u.cfg.lock().unwrap();
                c.theme = m;
                // выбор своей темы снимает импортированную из Telegram
                c.tg_theme = None;
                c.save();
            }
            u.st(|s| s.set_theme_mode(m));
            u.apply_theme();
        });
    });
    api.on_set_notifications(|v| set_cfg(|c| c.notifications = v));
    api.on_set_sound(|v| {
        set_cfg(|c| c.sound = v);
        if v {
            sys::play_sound();
        }
    });
    api.on_set_on_top(|v| {
        set_cfg(|c| c.always_on_top = v);
        with_ui(|u| {
            u.st(|s| s.set_on_top(v));
        });
    });
    api.on_set_show_flood(|v| {
        set_cfg(|c| c.show_flood = v);
        with_ui(|u| u.rebuild_chats());
    });
    api.on_set_show_gchat(|v| {
        set_cfg(|c| c.show_gchat = v);
        with_ui(|u| u.rebuild_chats());
    });
    setup_features(app, cfg, demo);
}

/// Шаблоны, команды, ИИ, цветные ники, уведомления, кэш и обновления.
fn setup_features(app: &AppWindow, cfg: Arc<Mutex<config::Config>>, demo: bool) {
    let st = app.global::<St>();
    {
        let c = cfg.lock().unwrap();
        st.set_epic_anim(c.epic_anim);
        st.set_auto_update(c.auto_update);
    }
    with_ui(|u| {
        u.rebuild_templates();
        u.rebuild_autos();
        u.sync_pop_settings();
        // кэш: сразу показываем чаты аккаунта, пока FunPay отвечает
        if !demo {
            let (gk, accs) = {
                let c = u.cfg.lock().unwrap();
                (c.golden_key.clone(), c.accounts.clone())
            };
            if let Some(a) = accs.iter().find(|a| !gk.is_empty() && a.golden_key == gk) {
                u.me = ui::Me { id: a.id.clone(), name: a.name.clone(), avatar: a.avatar.clone(), balance: String::new() };
                u.update_me();
                let id = a.id.clone();
                u.load_cache_for(&id);
            }
        }
    });
    epic::load(|| post(|u| u.epic_loaded()));
    let api = app.global::<Api>();

    // обучение при первом запуске (в демо только по FPT_TOUR)
    let start_tour = if demo { std::env::var("FPT_TOUR").is_ok() } else { !cfg.lock().unwrap().tour_done };
    if start_tour {
        slint::Timer::single_shot(std::time::Duration::from_millis(900), || {
            with_ui(|u| u.tour_start());
        });
    }
    api.on_tour_next(|| {
        with_ui(|u| u.tour_next());
    });
    api.on_tour_skip(|| {
        with_ui(|u| u.tour_finish());
    });
    api.on_tour_event(|e| {
        // событие приходит изнутри обработчика Slint: откладываем, чтобы не мешать
        let e = e.to_string();
        post(move |u| u.tour_event(&e));
    });
    api.on_tour_start(|| {
        with_ui(|u| u.tour_start());
    });

    let api = app.global::<Api>();
    api.on_tpl_use(|id| with_ui(|u| u.tpl_use(&id)).unwrap_or_default().into());
    api.on_tpl_bind(|n| with_ui(|u| u.tpl_bind(n)).unwrap_or_default().into());
    api.on_cmd_query(|t| {
        with_ui(|u| u.cmd_query(&t));
    });
    api.on_open_avatar(|w| {
        with_ui(|u| u.open_avatar(w));
    });
    api.on_starts_slash(|t| feat::starts_slash(&t));
    api.on_msg_link(|k, off| with_ui(|u| u.msg_link(&k, off)).unwrap_or(false));
    api.on_tpl_new(|| {
        with_ui(|u| u.tpl_new());
    });
    api.on_tpl_edit(|id| {
        with_ui(|u| u.tpl_edit(&id));
    });
    api.on_tpl_save(|| {
        with_ui(|u| u.tpl_save());
    });
    api.on_tpl_delete(|id| {
        with_ui(|u| u.tpl_delete(&id));
    });
    api.on_tpl_move(|id, d| {
        with_ui(|u| u.tpl_move(&id, d));
    });
    api.on_tpl_pick_image(|| {
        with_ui(|u| u.tpl_pick_image());
    });
    api.on_tpl_clear_image(|| {
        with_ui(|u| u.tpl_clear_image());
    });
    api.on_tpl_set(|k, v| {
        with_ui(|u| u.tpl_set(&k, v));
    });
    api.on_auto_new(|| {
        with_ui(|u| u.auto_new());
    });
    api.on_auto_edit(|id| {
        with_ui(|u| u.auto_edit(&id));
    });
    api.on_auto_save(|| {
        with_ui(|u| u.auto_save());
    });
    api.on_auto_delete(|id| {
        with_ui(|u| u.auto_delete(&id));
    });
    api.on_auto_toggle(|id, v| {
        with_ui(|u| u.auto_toggle(&id, v));
    });
    api.on_set_auto_on(|v| {
        set_cfg(|c| c.auto_on = v);
        with_ui(|u| u.rebuild_autos());
    });
    api.on_ai_rewrite(|t| {
        with_ui(|u| u.ai_rewrite(t.to_string()));
    });
    api.on_set_epic_anim(|v| set_cfg(|c| c.epic_anim = v));
    api.on_epic_get(|| sys::open_external("https://t.me/FPToolsBot"));
    api.on_open_contact(|c| {
        sys::open_external(if c == "ds" { "https://discord.gg/c8A9v58CQw" } else { "https://t.me/fptools" });
    });
    api.on_pop_set(|k, v| {
        with_ui(|u| u.pop_set(&k, v));
    });
    api.on_pop_test(|| {
        with_ui(|u| u.pop_test());
    });
    api.on_set_auto_update(|v| {
        set_cfg(|c| c.auto_update = v);
        if v {
            with_ui(|u| u.upd_check(false));
        }
    });
    api.on_upd_check(|| {
        with_ui(|u| u.upd_check(true));
    });
    api.on_upd_restart(|| {
        with_ui(|u| u.upd_restart(false));
    });

    // проверка обновлений: через 15 секунд после запуска и потом каждые 4 часа
    if !demo {
        let c2 = cfg.clone();
        slint::Timer::single_shot(std::time::Duration::from_secs(15), move || {
            if c2.lock().unwrap().auto_update {
                with_ui(|u| u.upd_check(false));
            }
        });
        let c3 = cfg.clone();
        let t = slint::Timer::default();
        t.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(4 * 3600), move || {
            if c3.lock().unwrap().auto_update {
                with_ui(|u| u.upd_check(false));
            }
        });
        std::mem::forget(t);
    }
}

/// golden_key из бэкапа мобильного приложения: accounts.accounts[], активный или первый.
fn gk_from_backup(s: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    // 1) Привычная структура бэкапа приложения: берём активный аккаунт.
    if let Some(acc) = v.get("accounts") {
        if let Some(list) = acc.get("accounts").and_then(|a| a.as_array()) {
            let active = acc.get("active_account_id").and_then(|a| a.as_str()).unwrap_or("");
            let pick = list.iter().find(|a| a.get("id").and_then(|i| i.as_str()) == Some(active)).or_else(|| list.first());
            if let Some(gk) = pick.and_then(|p| gk_field(p)) {
                return Some(gk);
            }
        }
    }
    // 2) Любой другой формат: ищем golden_key где угодно в JSON.
    find_golden_key(&v)
}

/// Достаёт golden_key из объекта по разным вариантам написания ключа.
fn gk_field(v: &serde_json::Value) -> Option<String> {
    for name in ["golden_key", "goldenKey", "golden-key", "goldenkey"] {
        if let Some(k) = v.get(name).and_then(|x| x.as_str()) {
            let k = k.trim();
            if !k.is_empty() {
                return Some(k.to_string());
            }
        }
    }
    None
}

/// Рекурсивно обходит JSON и возвращает первый непустой golden_key.
fn find_golden_key(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Object(m) => {
            if let Some(k) = gk_field(v) {
                return Some(k);
            }
            m.values().find_map(find_golden_key)
        }
        serde_json::Value::Array(a) => a.iter().find_map(find_golden_key),
        _ => None,
    }
}

fn set_cfg(f: impl FnOnce(&mut config::Config)) {
    with_ui(|u| {
        let mut c = u.cfg.lock().unwrap();
        f(&mut c);
        c.save();
    });
}

/// Ctrl+V: если в буфере картинка, открываем окно отправки. Иначе обычная вставка текста.
fn paste_image() -> bool {
    let Ok(mut cb) = arboard::Clipboard::new() else { return false };
    let Ok(img) = cb.get_image() else { return false };
    save_clipboard_image(img)
}

fn save_clipboard_image(img: arboard::ImageData) -> bool {
    let Some(buf) = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned()) else { return false };
    let dir = imgcache::cache_dir().join("outgoing");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("paste_{}.png", chrono::Utc::now().timestamp_millis()));
    if buf.save(&path).is_err() {
        return false;
    }
    with_ui(|u| u.sendbox_add(vec![path])).is_some()
}

/// Кусок строки по байтовым смещениям выделения Slint; пустое выделение означает весь текст.
/// Невидимый хвост из неразрывных пробелов (место под время в пузыре) отрезаем.
fn byte_slice(t: &str, a: i32, b: i32) -> String {
    let (lo, hi) = (a.min(b).max(0) as usize, a.max(b).max(0) as usize);
    let s = if lo < hi && hi <= t.len() && t.is_char_boundary(lo) && t.is_char_boundary(hi) { &t[lo..hi] } else { t };
    s.trim_end_matches('\u{00A0}').to_string()
}

/// Запись текста в буфер с парой повторов (на Windows буфер бывает занят).
fn set_clipboard_text(t: &str) -> bool {
    for _ in 0..6 {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if cb.set_text(t.to_string()).is_ok() {
                return true;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    false
}

/// Единая вставка: картинка -> в окно отправки (возврат ""), иначе текст.
/// Всё читается здесь, одним открытием буфера, с парой повторов — на Windows
/// системный буфер иногда занят соседним обращением и срабатывает не с первого раза.
fn paste_clipboard() -> slint::SharedString {
    for attempt in 0..6 {
        let Ok(mut cb) = arboard::Clipboard::new() else {
            std::thread::sleep(std::time::Duration::from_millis(25));
            continue;
        };
        if let Ok(img) = cb.get_image() {
            save_clipboard_image(img);
            return slint::SharedString::new();
        }
        match cb.get_text() {
            Ok(t) if !t.is_empty() => return t.into(),
            Ok(_) => return slint::SharedString::new(),
            Err(_) => {
                if attempt < 5 {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            }
        }
    }
    slint::SharedString::new()
}

fn save_image(url: String) {
    let name = url.rsplit('/').next().unwrap_or("image.jpg").split('?').next().unwrap_or("image.jpg").to_string();
    let Some(path) = rfd::FileDialog::new().set_title("Сохранить картинку").set_file_name(&name).save_file() else { return };
    std::thread::spawn(move || {
        let client = fp::http_client(40);
        let ok = imgcache::fetch_bytes(&client, &url).map(|b| std::fs::write(&path, b).is_ok()).unwrap_or(false);
        post(move |u| u.toast(if ok { "Картинка сохранена" } else { "Не удалось сохранить" }));
    });
}

fn copy_image(url: String) {
    std::thread::spawn(move || {
        let client = fp::http_client(40);
        let ok = (|| {
            let b = imgcache::fetch_bytes(&client, &url)?;
            let img = image::load_from_memory(&b).ok()?.to_rgba8();
            let (w, h) = (img.width() as usize, img.height() as usize);
            let mut cb = arboard::Clipboard::new().ok()?;
            cb.set_image(arboard::ImageData { width: w, height: h, bytes: std::borrow::Cow::Owned(img.into_raw()) }).ok()
        })()
        .is_some();
        post(move |u| u.toast(if ok { "Картинка скопирована" } else { "Не удалось скопировать" }));
    });
}

fn change_avatar() {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Новая аватарка")
        .add_filter("Картинки", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
        .pick_file()
    else {
        return;
    };
    with_ui(|u| {
        u.st(|s| s.set_avatar_busy(true));
    });
    std::thread::spawn(move || {
        let res = (|| -> Result<Vec<u8>, String> {
            let img = image::open(&path).map_err(|_| "Это не картинка".to_string())?;
            let side = img.width().min(img.height());
            let x = (img.width() - side) / 2;
            let y = (img.height() - side) / 2;
            let sq = img.crop_imm(x, y, side, side).resize_exact(400, 400, image::imageops::FilterType::Lanczos3);
            let mut out = Vec::new();
            sq.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
            Ok(out)
        })();
        match res {
            Ok(png) => post(move |u| u.send(Cmd::UploadAvatar(png))),
            Err(e) => post(move |u| u.avatar_uploaded(Err(e))),
        }
    });
}
