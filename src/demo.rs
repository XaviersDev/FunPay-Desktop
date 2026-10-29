//! Демо-данные для проверки вёрстки без входа (FPT_DEMO=1 или режим скриншотов).

use crate::parse::{ChatItem, Message};
use crate::ui::{with_ui, Me, OpenedChat, THUMB};
use chrono::NaiveDate;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

fn pic(w: u32, h: u32, a: [u8; 3], b: [u8; 3]) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    let px = buf.make_mut_slice();
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) as f32 / (w + h) as f32;
            let c = |i: usize| (a[i] as f32 * (1.0 - t) + b[i] as f32 * t) as u8;
            let ring = ((x as f32 - w as f32 * 0.6).powi(2) + (y as f32 - h as f32 * 0.4).powi(2)).sqrt() < (w.min(h) as f32 * 0.18);
            let k = if ring { 255 } else { 0 };
            px[(y * w + x) as usize] = Rgba8Pixel { r: c(0).max(k / 2), g: c(1).max(k / 2), b: c(2).max(k / 3), a: 255 };
        }
    }
    Image::from_rgba8(buf)
}

fn m(id: u64, author_id: &str, author: &str, sec: u32, text: &str, img: Option<&str>) -> Message {
    let base = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap().and_hms_opt(14, 0, 0).unwrap();
    Message {
        id,
        author_id: author_id.into(),
        author: author.into(),
        ts: Some(base + chrono::Duration::seconds(sec as i64)),
        has_head: true,
        text: text.into(),
        image: img.map(|s| s.to_string()),
        thumb: img.map(|s| s.to_string()),
        ..Default::default()
    }
}

pub fn fill(scene: &str) {
    // цветные ники для демо (стили как у настоящих донатеров)
    crate::epic::demo_insert("vlad_storm", "FPT-STYLE-eyJjMSI6IiM5OTQwZmYiLCJjMiI6IiNmZjAwZmYiLCJjMyI6IiMwMDAwZmYiLCJhbmciOiI5MCIsInNjbCI6IjI2NyIsInNwZCI6IjYiLCJhbiI6WyJnbG93Iiwid2F2ZSJdLCJvdiI6Im1hdHJpeCIsInBjIjoiI2ZmMDBmZiJ9");
    crate::epic::demo_insert("XaviersDev", "FPT-STYLE-eyJjMSI6IiM5OTQwZmYiLCJjMiI6IiNmZjAwZmYiLCJjMyI6IiMwMDAwZmYiLCJhbmciOiI5MCIsInNjbCI6IjI2NyIsInNwZCI6IjYiLCJhbiI6WyJnbG93Iiwid2F2ZSJdLCJvdiI6Im1hdHJpeCIsInBjIjoiI2ZmMDBmZiJ9");
    crate::epic::demo_insert("darkness21", "FPT-STYLE-eyJjMSI6IiNmZmNjMDAiLCJjMiI6IiNlMDg3MGIiLCJjMyI6IiNmZjU1MDAiLCJhbmciOiI0NSIsInNjbCI6IjEwMCIsInNwZCI6IjMiLCJhbiI6WyJ3YXZlIiwiZ2xvdyJdLCJvdiI6ImZpcmUiLCJwYyI6IiNmZjg4MDAifQ==");
    with_ui(|u| {
        {
            let mut c = u.cfg.lock().unwrap();
            if c.templates.is_empty() {
                let t = |id: &str, name: &str, text: &str, color: &str, bind: i32| crate::templates::Template {
                    id: id.into(),
                    name: name.into(),
                    text: text.into(),
                    color: color.into(),
                    image: String::new(),
                    image_first: true,
                    bind,
                };
                c.templates = vec![
                    t("t1", "Привет", "$welcome, $username! Чем могу помочь?", "", 1),
                    t("t2", "Выдача", "Ваш ключ выше. Заказ $order_id, активируйте в Steam. Хорошей игры!", "#2EA44F", 2),
                    t("t3", "Отзыв", "Спасибо за покупку! Буду рад отзыву: $order_link", "#E5A50A", 3),
                    t("t4", "Подождите", "Минутку, проверяю наличие", "#8E5CF7", 4),
                ];
                c.auto_cmds = vec![
                    crate::templates::AutoCmd { id: "a1".into(), trigger: "!помощь".into(), response: "$welcome! Опишите проблему, отвечу в течение часа.".into(), ..Default::default() },
                    crate::templates::AutoCmd { id: "a2".into(), trigger: "гарантия".into(), response: "Гарантия 7 дней на все ключи.".into(), exact: false, ..Default::default() },
                ];
            }
        }
        u.logged_in(Me { id: "1".into(), name: "XaviersDev".into(), avatar: String::new(), balance: "12 480 ₽".into() });
        {
            let mut c = u.cfg.lock().unwrap();
            c.accounts = vec![
                crate::config::SavedAccount { id: "1".into(), name: "XaviersDev".into(), ..Default::default() },
                crate::config::SavedAccount { id: "2".into(), name: "keys_store24".into(), ..Default::default() },
            ];
        }
        u.rebuild_accounts();
        let chat = |id: &str, name: &str, preview: &str, time: &str, unread: bool| ChatItem {
            id: id.into(),
            name: name.into(),
            preview: preview.into(),
            time: time.into(),
            unread,
            ..Default::default()
        };
        {
            let mut c = u.cfg.lock().unwrap();
            c.last_outgoing.insert("104".into(), "Готово, проверяйте".into());
        }
        u.chats_updated(vec![
            chat("101", "darkness21", "Оплатил. Ключ ещё есть?", "14:18", false),
            chat("102", "vlad_storm", "А на месяц можно?", "14:21", true),
            chat("103", "k1rill_play", "Оплатил, жду выдачу", "14:20", true),
            chat("104", "m1lkyway", "Готово, проверяйте", "12:30", false),
            chat("105", "sanych", "Спасибо, всё работает 🔥", "11:18", false),
            chat("106", "terranex", "Скинь скрины, пожалуйста", "вчера", false),
            chat("107", "ghostin", "Ок, жду", "Пн", false),
            chat("108", "nasty_mew", "Изображение", "Вс", false),
        ]);
        // картинки альбома
        let urls = ["demo://1", "demo://2", "demo://3", "demo://4"];
        let colors = [([58, 127, 213], [118, 87, 230]), ([255, 138, 122], [228, 80, 127]), ([98, 214, 190], [44, 158, 149]), ([255, 203, 115], [242, 151, 58])];
        for (i, url) in urls.iter().enumerate() {
            let (w, h) = if i == 0 { (640, 400) } else { (400, 400) };
            u.imgs.insert(crate::imgcache::key(THUMB, url), (pic(w, h, colors[i].0, colors[i].1), w, h));
        }
        if scene == "list" {
            return;
        }
        u.open_chat("101".into(), 0);
        let gen = u.gen;
        let me = "1";
        let peer = "5";
        let mut msgs = vec![
            m(1, peer, "darkness21", 0, "Добрый вечер, Hades есть?", None),
            m(2, me, "XaviersDev", 60, "Закончился, завтра днём будет. Напишу, как появится", None),
            m(3, peer, "darkness21", 900, "Здравствуйте! Хочу взять Hollow Knight", None),
            m(4, me, "XaviersDev", 960, "Здравствуйте, в наличии. Оплачивайте, выдам сразу. Отзывы тут: funpay.com/users/1/ и https://t.me/fptools", None),
            m(5, "0", "FunPay", 1020, "Покупатель darkness21 оплатил заказ #F88412AB. Steam-ключ Hollow Knight, 1 шт., 349 ₽. darkness21, не забудьте потом нажать «Подтвердить выполнение заказа».", None),
            m(6, peer, "darkness21", 1030, "Оплатил. Ключ ещё есть?", None),
            m(7, peer, "darkness21", 1032, "", Some("demo://1")),
            m(8, peer, "darkness21", 1033, "", Some("demo://2")),
            m(9, peer, "darkness21", 1034, "", Some("demo://3")),
            m(10, peer, "darkness21", 1040, "Вот скрины, если что", None),
            m(11, me, "XaviersDev", 1100, "", Some("demo://4")),
            m(12, me, "XaviersDev", 1102, "╭─ ⤸ Оплатил. Ключ ещё есть?\n╰ Ключ выше, активируйте в Steam. Хорошей игры! ⤸ ━━ ☆", None),
        ];
        msgs[0].ts = msgs[0].ts.map(|t| t - chrono::Duration::days(1));
        msgs[1].ts = msgs[1].ts.map(|t| t - chrono::Duration::days(1));
        msgs[4].badge = "оповещение".into();
        msgs[7].has_head = false;
        msgs[8].has_head = false;
        u.chat_opened(OpenedChat {
            gen,
            peer_id: peer.into(),
            peer_name: "darkness21".into(),
            peer_status: "Онлайн".into(),
            lot_name: "Steam-ключ Hollow Knight, 349 ₽".into(),
            lot_url: "https://funpay.com/lots/offer?id=1".into(),
            msgs,
            has_older: false,
            ..Default::default()
        });
        {
            let mut c = u.cfg.lock().unwrap();
            c.fpt_users.insert(peer.into());
        }
        u.update_fpt_banner();
        {
            let mut c = u.cfg.lock().unwrap();
            c.pinned_msgs.insert("101".into(), crate::config::PinnedMsg { key: "5".into(), author: "FunPay".into(), text: "Покупатель darkness21 оплатил заказ #F88412AB".into() });
            c.folders = vec![crate::config::UserFolder { id: "f1".into(), name: "Работа".into(), chats: vec!["101".into(), "103".into()] }];
            c.pinned_chats = vec!["102".into()];
            c.muted.insert("105".into());
        }
        u.rebuild_chats();
        u.pending.push(crate::ui::Pending {
            local: 1,
            chat: "101".into(),
            text: "Если что-то не так, пишите".into(),
            image: None,
            state: 1,
            at: std::time::Instant::now(),
        });
        u.rebuild_msgs();
        u.rebuild_templates();
        u.rebuild_autos();
        u.epic_loaded();
    });
}

/// Дополнительные демо-экраны: профиль, заказ, карточка, окно отправки.
pub fn scene(name: &str) {
    with_ui(|u| match name {
        "profile" => {
            u.open_profile("2846982".into());
            let gen = u.pf_gen;
            let html = std::fs::read_to_string("tests/data/user.html").unwrap_or_default();
            let p = crate::parse::parse_profile(&html, "2846982");
            u.profile_loaded(gen, false, Ok(p));
        }
        "reviews" => {
            u.open_profile("2846982".into());
            let gen = u.pf_gen;
            let html = std::fs::read_to_string("tests/data/user.html").unwrap_or_default();
            let p = crate::parse::parse_profile(&html, "2846982");
            u.profile_loaded(gen, false, Ok(p));
            u.st(|s| s.set_pf_tab(1));
        }
        "order" | "order_plain" => {
            u.open_order("F88412AB".into());
            let gen = u.od_gen;
            u.order_loaded(
                gen,
                Ok(crate::parse::Order {
                    id: "F88412AB".into(),
                    status: "Оплачен".into(),
                    game: "Hollow Knight, Steam".into(),
                    short_desc: "Steam-ключ Hollow Knight, активация в любом регионе".into(),
                    price: "349 ₽".into(),
                    peer_name: "darkness21".into(),
                    peer_id: "5".into(),
                    can_refund: true,
                    has_review: true,
                    review_rating: 5,
                    review_text: "nice servers".into(),
                    reply_mine: true,
                    review_reply: if name == "order_plain" { "Thanks for the review, come back again any time, we have lots of templates and fast delivery every day of the week".into() } else { "Спасибо за отзыв! 🤝\n⸨ ⚡ ⸩◾🔫 ▶🧩 |🎃| 3 ГОТОВЫХ ШАБЛОНОВ ДЛЯ ⸨ ОБЩЕНИЯ ⸩ / 𝙌𝙐𝙄𝘾𝙆𝙇𝙔 /💎◾⸨ ⚡ ⸩ 💥 [ 24/7 АВТОВЫДАЧА...".into() },
                    params: vec![("Количество".into(), "1 шт.".into()), ("Регион".into(), "Россия и СНГ".into()), ("Подробное описание".into(), "▶ 📘 Добро пожаловать в наш магазин, уважаемый покупатель! У нас только качественный товар, и мы всегда готовы ответить на ваши вопросы и помочь решить любые проблемы с ним, если они возникнут. 📋 ИНФОРМАЦИЯ ОБ УСЛУГЕ: ►• Шаблон сервера по ссылке ►• Привлекательное оформление ►• Роли ►• Права для ролей ►• Категории".into()), ("Оплаченный товар".into(), "XXXXX-XXXXX-XXXXX".into())],
                    ..Default::default()
                }),
            );
        }
        "card" => u.open_card(),
        "templates" | "tpllook" | "autos" => {
            u.st(|s| {
                s.set_tpl_open(true);
                s.set_tpl_tab(if name == "tpllook" { 1 } else if name == "autos" { 2 } else { 0 });
            });
        }
        "tpledit" => {
            u.st(|s| s.set_tpl_open(true));
            u.tpl_edit("t2");
        }
        "epic" => {
            u.st(|s| {
                s.set_settings_open(true);
                s.set_epic_open(true);
            });
        }
        "notes" => u.st(|s| s.set_notes_open(true)).unwrap_or(()),
        "cmd" => u.cmd_query("/"),
        "ai" => u.st(|s| {
            s.set_ai_busy(false);
            s.set_ai_orig("щас скину ключ".into());
        }).unwrap_or(()),
        "popup" => {
            slint::Timer::single_shot(std::time::Duration::from_millis(1500), || {
                with_ui(|u| {
                    u.pop_test();
                    let pop = u.cfg.lock().unwrap().popup.clone();
                    u.show_note("102", "vlad_storm", "А на месяц можно? И есть ли скидка, если взять сразу три ключа?", "", &pop);
                });
            });
        }
        "pos2" | "pos3" | "pos1" | "full" => {
            u.tpl_set("pos", match name { "pos2" => 2, "pos3" => 3, "pos1" => 1, _ => 0 });
            if name == "full" {
                u.tpl_set("full", 1);
                u.tpl_set("fill", 0);
                u.tpl_set("shape", 1);
            }
        }
        "sendbox" => {
            let dir = std::env::temp_dir();
            let mut paths = vec![];
            for (i, c) in [[58u8, 127, 213], [228, 80, 127], [44, 158, 149]].iter().enumerate() {
                let img = image::RgbImage::from_fn(400, 300, |x, y| image::Rgb([c[0].saturating_add((x / 8) as u8), c[1], c[2].saturating_add((y / 8) as u8)]));
                let p = dir.join(format!("fpt_demo_{i}.png"));
                let _ = img.save(&p);
                paths.push(p);
            }
            u.sendbox_add(paths);
        }
        "reply" => u.msg_action("6".into(), "reply".into()),
        "mydesc" | "descedit" => {
            let banner = std::env::temp_dir().join("fpt_demo_banner.png");
            let img = image::RgbImage::from_fn(900, 300, |x, y| {
                let t = x as f32 / 900.0;
                image::Rgb([(40.0 + 120.0 * t) as u8, (90.0 + 40.0 * (y as f32 / 300.0)) as u8, (200.0 - 60.0 * t) as u8])
            });
            let _ = img.save(&banner);
            let d = crate::desc::Desc {
                text: "Продаю ключи Steam и буст в Dota 2 с 2020 года. Выдача ключей сразу после оплаты, буст в течение дня. Пишите в любое время, отвечаю быстро.".into(),
                banner: format!("file://{}", banner.display()),
            };
            u.ext.descs.insert("1".into(), (std::time::Instant::now(), d));
            u.open_profile("1".into());
            let gen = u.pf_gen;
            let html = std::fs::read_to_string("tests/data/user.html").unwrap_or_default();
            let mut p = crate::parse::parse_profile(&html, "1");
            p.name = "XaviersDev".into();
            u.profile_loaded(gen, false, Ok(p));
            if name == "descedit" {
                u.desc_edit();
                u.desc_progress("Жду, пока сервер увидит лот…");
                u.st(|s| s.set_desc_busy(true));
            }
        }
        "mini" | "minibar" => {
            u.demo_tabs(name == "minibar");
        }
        "tgtheme" => {
            if let Ok(p) = std::env::var("FPT_THEME_FILE") {
                u.import_theme_path(std::path::Path::new(&p));
            }
            if std::env::var("FPT_OPEN_SETTINGS").is_ok() {
                u.st(|s| s.set_settings_open(true));
            }
        }
        "deals" | "finance" => {
            let tab = if name == "finance" { 2 } else { 0 };
            u.open_deals(tab);
            let gen = u.ext.deals_gen;
            if tab == 2 {
                let html = r#"<div>
<div class="tc-item transaction-status-waiting" data-transaction="905"><div class="tc-date-time">сегодня, 14:31</div><div class="tc-title">Заказ #F88412AB</div><div class="tc-price">+ 349 ₽</div></div>
<div class="tc-item transaction-status-waiting" data-transaction="904"><div class="tc-date-time">сегодня, 11:02</div><div class="tc-title">Заказ #K2LM55QA</div><div class="tc-price">+ 1 190 ₽</div></div>
<div class="tc-item transaction-status-complete" data-transaction="903"><div class="tc-date-time">вчера, 18:40</div><div class="tc-title">Заказ #ZZ99YY88</div><div class="tc-price">+ 90 ₽</div></div>
<div class="tc-item transaction-status-complete" data-transaction="902"><div class="tc-date-time">вчера, 9:00</div><div class="tc-title">Вывод денег</div><div class="tc-payment"><span class="tc-payment-number">Карта *1234</span></div><div class="tc-price">− 5 000 ₽</div></div>
<div class="tc-item transaction-status-cancel" data-transaction="901"><div class="tc-date-time">25 октября, 12:00</div><div class="tc-title">Вывод денег</div><div class="tc-payment"><span class="tc-payment-number">СБП</span></div><div class="tc-price">− 2 000 ₽</div></div>
<div class="tc-item transaction-status-complete" data-transaction="900"><div class="tc-date-time">24 октября, 20:15</div><div class="tc-title">Пополнение баланса</div><div class="tc-price">+ 500 ₽</div></div>
<input type="hidden" name="continue" value="next"></div>"#;
                u.txns_loaded(gen, false, Ok(crate::money::parse_transactions(html)));
            } else {
                let row = |id: &str, cls: &str, desc: &str, game: &str, peer: &str, st: &str, price: &str, date: &str| {
                    format!(r#"<a href="https://funpay.com/orders/{id}/" class="tc-item {cls}"><div class="tc-date-time">{date}</div><div class="order-desc"><div>{desc}</div><div class="text-muted">{game}</div></div><div class="media-user-name"><span data-href="https://funpay.com/users/5/">{peer}</span></div><div class="tc-status">{st}</div><div class="tc-price">{price}</div></a>"#)
                };
                let html = [
                    row("F88412AB", "info", "Steam-ключ Hollow Knight, 1 шт.", "Hollow Knight, Ключи", "darkness21", "Оплачен", "349 ₽", "сегодня, 14:30"),
                    row("K2LM55QA", "info", "Буст MMR 4000-4500, Solo", "Dota 2, Буст", "vlad_storm", "Оплачен", "1 190 ₽", "сегодня, 11:02"),
                    row("ZZ99YY88", "", "Аккаунт с 120 часами", "CS2, Аккаунты", "k1rill_play", "Закрыт", "90 ₽", "вчера, 18:40"),
                    row("AB12CD34", "warning", "Ключ Elden Ring", "Elden Ring, Ключи", "terranex", "Возврат", "2 390 ₽", "25 октября, 12:00"),
                    row("QW77ER88", "", "Подписка на месяц", "Spotify, Premium", "m1lkyway", "Закрыт", "199 ₽", "24 октября, 20:15"),
                ]
                .join("");
                u.trade_loaded(gen, false, Ok(crate::money::parse_trade(&format!("{html}<input name=\"continue\" value=\"x\">"))));
            }
        }
        _ => {}
    });
}
