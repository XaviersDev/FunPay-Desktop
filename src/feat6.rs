//! Статистика продаж, подробности операций, отложенные сообщения и режим "Не беспокоить".

use crate::money::SalesStats;
use crate::ui::Ui;
use crate::worker::Cmd;
use crate::config::Scheduled;
use crate::{SchedRow, StatTile, KV};
use slint::{ModelRc, VecModel};
use std::rc::Rc;

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

fn tile(label: &str, value: &str, sub: &str, long: bool) -> StatTile {
    StatTile { label: label.into(), value: value.into(), sub: sub.into(), long }
}

pub fn tiles(s: &SalesStats) -> Vec<StatTile> {
    vec![
        tile("ЗАКАЗОВ", &s.orders.to_string(), "", false),
        tile("ЗАРАБОТАНО", &s.earned, "", false),
        tile("СРЕДНИЙ ЧЕК", &s.avg, "", false),
        tile("ЗАКРЫТО", &s.closed.to_string(), "", false),
        tile("В ОЖИДАНИИ ПОДТВЕРЖДЕНИЯ", &s.pending.to_string(), if s.pending > 0 { &s.pending_sum } else { "" }, false),
        tile("ВОЗВРАТЫ", &s.refunds.to_string(), "", false),
        tile("УНИКАЛЬНЫХ ПОКУПАТЕЛЕЙ", &s.buyers.to_string(), "", false),
        tile("САМЫЙ АКТИВНЫЙ ПОКУПАТЕЛЬ", &s.top_buyer, "", true),
        tile("САМАЯ ДОРОГАЯ ПРОДАЖА", &s.max_sale, "", false),
        tile("САМЫЙ ПОПУЛЯРНЫЙ ТОВАР", &s.top_item, "", true),
        tile("САМАЯ ПОПУЛЯРНАЯ КАТЕГОРИЯ", &s.top_game, "", true),
    ]
}

impl Ui {
    // ------------------------------------------------------------ статистика продаж

    pub fn open_stats(&mut self) {
        self.st(|s| s.set_stats_open(true));
        let p = self.st(|s| s.get_stats_period()).unwrap_or(2);
        self.stats_period_set(p);
    }

    pub fn stats_period_set(&mut self, p: i32) {
        self.ext.stats_gen += 1;
        let gen = self.ext.stats_gen;
        self.st(|s| {
            s.set_stats_period(p);
            s.set_stats_loading(true);
            s.set_stats_note("Загружаю продажи".into());
        });
        let days = match p {
            0 => 1,
            1 => 7,
            2 => 30,
            _ => 0,
        };
        if self.demo {
            return;
        }
        self.send(Cmd::SalesStats { days, gen });
    }

    pub fn stats_progress(&mut self, gen: u64, n: usize) {
        if gen == self.ext.stats_gen {
            self.st(|s| s.set_stats_note(format!("Загружено продаж: {n}").into()));
        }
    }

    pub fn stats_loaded_full(&mut self, gen: u64, res: Result<(SalesStats, String), String>) {
        if gen != self.ext.stats_gen {
            return;
        }
        match res {
            Ok((st, note)) => self.show_stats(&st, &note),
            Err(e) => self.st(|s| {
                s.set_stats_loading(false);
                s.set_stats_tiles(model(vec![]));
                s.set_stats_note(e.into());
            })
            .unwrap_or(()),
        }
    }

    pub fn show_stats(&mut self, st: &SalesStats, note: &str) {
        let t = tiles(st);
        self.st(|s| {
            s.set_stats_tiles(model(t));
            s.set_stats_note(note.into());
            s.set_stats_loading(false);
        });
    }

    // ------------------------------------------------------------ операция из финансов

    pub fn open_txn(&mut self, id: String) {
        self.ext.txn_gen += 1;
        let gen = self.ext.txn_gen;
        self.st(|s| {
            s.set_txn_open(true);
            s.set_txn_loading(true);
            s.set_txn_error("".into());
            s.set_txn_link("".into());
            s.set_txn_rows(model(vec![]));
        });
        if !self.demo {
            self.send(Cmd::TxnInfo { id, gen });
        }
    }

    pub fn txn_loaded(&mut self, gen: u64, res: Result<(String, Vec<(String, String)>, String), String>) {
        if gen != self.ext.txn_gen {
            return;
        }
        match res {
            Ok((title, rows, link)) => {
                let rows: Vec<KV> = rows.into_iter().map(|(k, v)| KV { k: k.into(), v: v.into() }).collect();
                self.st(|s| {
                    s.set_txn_title(title.into());
                    s.set_txn_rows(model(rows));
                    s.set_txn_link(link.into());
                    s.set_txn_loading(false);
                });
            }
            Err(e) => self.st(|s| {
                s.set_txn_error(e.into());
                s.set_txn_loading(false);
            })
            .unwrap_or(()),
        }
    }

    // ------------------------------------------------------------ отложенные сообщения

    pub fn open_sched(&mut self, draft: String) {
        self.st(|s| {
            s.set_sched_draft(draft.trim().to_string().into());
            s.set_sched_error("".into());
            if s.get_sched_time().is_empty() {
                s.set_sched_time("18:00".into());
            }
            s.set_sched_open(true);
        });
        self.rebuild_sched();
    }

    pub fn sched_add(&mut self, preset: i32) {
        let Some((cur_id, cur_title)) = self.cur.as_ref().map(|c| (c.id.clone(), c.title.clone())) else { return };
        let text = self.st(|s| s.get_sched_draft().to_string()).unwrap_or_default();
        if text.trim().is_empty() {
            return;
        }
        let now = chrono::Local::now();
        let at = match preset {
            0 => now + chrono::Duration::minutes(15),
            1 => now + chrono::Duration::hours(1),
            2 => now + chrono::Duration::hours(3),
            3 => {
                let d = now.date_naive() + chrono::Duration::days(1);
                match d.and_hms_opt(9, 0, 0).and_then(|t| t.and_local_timezone(chrono::Local).single()) {
                    Some(t) => t,
                    None => return,
                }
            }
            _ => {
                let (day, time) = self.st(|s| (s.get_sched_day(), s.get_sched_time().to_string())).unwrap_or_default();
                let parsed = time.trim().replace('.', ":").split_once(':').and_then(|(h, m)| Some((h.trim().parse::<u32>().ok()?, m.trim().parse::<u32>().ok()?)));
                let Some((h, m)) = parsed.filter(|(h, m)| *h < 24 && *m < 60) else {
                    self.st(|s| s.set_sched_error("Время пишется так: 18:30".into()));
                    return;
                };
                let d = now.date_naive() + chrono::Duration::days(day.clamp(0, 2) as i64);
                let Some(t) = d.and_hms_opt(h, m, 0).and_then(|t| t.and_local_timezone(chrono::Local).single()) else { return };
                if t <= now {
                    self.st(|s| s.set_sched_error("Это время уже прошло".into()));
                    return;
                }
                t
            }
        };
        {
            let mut c = self.cfg.lock().unwrap();
            c.scheduled.push(Scheduled {
                id: format!("s{}", chrono::Utc::now().timestamp_millis()),
                chat: cur_id,
                name: cur_title,
                text,
                at: at.timestamp(),
                ..Default::default()
            });
            c.save();
        }
        self.st(|s| {
            s.set_sched_draft("".into());
            s.set_sched_error("".into());
        });
        self.toast(&format!("Сообщение уйдёт {}", when_text(at.timestamp())));
        self.rebuild_sched();
    }

    pub fn sched_cancel(&mut self, id: String) {
        let back = {
            let mut c = self.cfg.lock().unwrap();
            let t = c.scheduled.iter().find(|x| x.id == id).map(|x| x.text.clone());
            c.scheduled.retain(|x| x.id != id);
            c.save();
            t
        };
        // отменённый текст возвращаем в окно, если там пусто
        if let Some(t) = back {
            self.st(|s| {
                if s.get_sched_draft().is_empty() {
                    s.set_sched_draft(t.into());
                }
            });
        }
        self.rebuild_sched();
    }

    pub fn sched_now(&mut self, id: String) {
        {
            let mut c = self.cfg.lock().unwrap();
            if let Some(x) = c.scheduled.iter_mut().find(|x| x.id == id) {
                x.at = 0;
            }
            c.save();
        }
        self.sched_tick();
    }

    /// Строки для окна и счётчик у кнопки в текущем чате.
    pub fn rebuild_sched(&mut self) {
        let list = self.cfg.lock().unwrap().scheduled.clone();
        let here_id = self.cur.as_ref().map(|c| c.id.clone()).unwrap_or_default();
        let now = chrono::Utc::now().timestamp();
        let mut rows: Vec<SchedRow> = list
            .iter()
            .map(|x| SchedRow {
                id: x.id.clone().into(),
                chat: x.name.clone().into(),
                text: x.text.replace('\n', " ").into(),
                when: if x.at <= now { if x.error.is_empty() { "Отправляю".into() } else { format!("Жду интернет: {}", x.error).into() } } else { when_text(x.at).into() },
                late: x.at <= now,
            })
            .collect();
        // сначала этот чат
        rows.sort_by_key(|r| (r.chat.as_str() != list.iter().find(|x| x.chat == here_id).map(|x| x.name.as_str()).unwrap_or(""), 0));
        let here = list.iter().filter(|x| x.chat == here_id).count() as i32;
        self.st(|s| {
            s.set_sched_list(model(rows));
            s.set_sched_here(here);
        });
    }

    /// Раз в 15 секунд: отправляем то, чему пришло время.
    pub fn sched_tick(&mut self) {
        if self.demo || self.me.id.is_empty() {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let due: Vec<Scheduled> = self.cfg.lock().unwrap().scheduled.iter().filter(|x| x.at <= now && !self.ext.sched_busy.contains(&x.id)).cloned().collect();
        for x in due {
            self.ext.sched_busy.push(x.id.clone());
            self.send(Cmd::SendTo { id: x.id, chat: x.chat, text: x.text });
        }
        self.dnd_tick();
    }

    pub fn sched_result(&mut self, id: String, res: Result<(), String>) {
        self.ext.sched_busy.retain(|x| *x != id);
        let mut note = None;
        {
            let mut c = self.cfg.lock().unwrap();
            match &res {
                Ok(()) => {
                    if let Some(x) = c.scheduled.iter().find(|x| x.id == id) {
                        note = Some(format!("Отложенное сообщение отправлено: {}", x.name));
                    }
                    c.scheduled.retain(|x| x.id != id);
                }
                Err(e) => {
                    let net = e.contains("связ") || e.contains("сет") || e.contains("ответил вовремя") || e.contains("Нет входа") || e.to_lowercase().contains("timeout");
                    if let Some(x) = c.scheduled.iter_mut().find(|x| x.id == id) {
                        x.tries += 1;
                        x.error = if net { "нет связи".into() } else { e.clone() };
                        // FunPay отказал несколько раз подряд: дальше не пробуем
                        if !net && x.tries >= 5 {
                            note = Some(format!("Не отправилось отложенное сообщение в чат {}: {e}", x.name));
                            x.at = i64::MAX;
                        }
                    }
                }
            }
            c.save();
        }
        if let Some(n) = note {
            self.toast(&n);
        }
        self.rebuild_sched();
    }

    // ------------------------------------------------------------ не беспокоить

    pub fn sync_dnd(&mut self) {
        let d = self.cfg.lock().unwrap().dnd.clone();
        let active = self.dnd_active();
        self.st(|s| {
            s.set_dnd_on(d.on);
            s.set_dnd_schedule(d.schedule);
            s.set_dnd_from(d.from);
            s.set_dnd_to(d.to);
            s.set_dnd_reply(d.reply);
            s.set_dnd_text(d.reply_text.clone().into());
            s.set_dnd_active(active);
        });
    }

    pub fn dnd_active(&self) -> bool {
        use chrono::Timelike;
        let h = chrono::Local::now().hour() as i32;
        self.cfg.lock().unwrap().dnd.active_at(h)
    }

    pub fn dnd_set(&mut self, key: &str, v: i32) {
        {
            let mut c = self.cfg.lock().unwrap();
            match key {
                "on" => c.dnd.on = v != 0,
                "schedule" => c.dnd.schedule = v != 0,
                "from" => c.dnd.from = v.clamp(0, 23),
                "to" => c.dnd.to = v.clamp(0, 23),
                "reply" => c.dnd.reply = v != 0,
                _ => {}
            }
            c.save();
        }
        self.dnd_tick();
    }

    pub fn dnd_text_set(&mut self, t: String) {
        let mut c = self.cfg.lock().unwrap();
        c.dnd.reply_text = t;
        c.save();
    }

    /// Режим включился или выключился: обновляем экран, автоответы начинаются заново.
    pub fn dnd_tick(&mut self) {
        let active = self.dnd_active();
        if !active {
            self.ext.dnd_replied.clear();
        }
        self.st(|s| s.set_dnd_active(active));
    }

    /// Новые входящие в режиме "Не беспокоить": автоответ один раз в чат.
    pub fn dnd_autoreply(&mut self, chats: &[(String, String)]) {
        let (reply, text) = {
            let c = self.cfg.lock().unwrap();
            (c.dnd.reply, c.dnd.reply_text.clone())
        };
        if !reply || text.trim().is_empty() || self.demo {
            return;
        }
        for (id, _name) in chats {
            if self.ext.dnd_replied.contains(id) {
                continue;
            }
            self.ext.dnd_replied.push(id.clone());
            self.send(Cmd::SendTo { id: String::new(), chat: id.clone(), text: text.clone() });
        }
    }
}

/// "сегодня в 18:30", "завтра в 9:00", "7 ноября в 12:00".
pub fn when_text(at: i64) -> String {
    use chrono::TimeZone;
    let Some(t) = chrono::Local.timestamp_opt(at, 0).single() else { return String::new() };
    let today = chrono::Local::now().date_naive();
    let d = t.date_naive();
    let day = if d == today {
        "сегодня".to_string()
    } else if d == today + chrono::Duration::days(1) {
        "завтра".to_string()
    } else {
        const M: [&str; 12] = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"];
        use chrono::Datelike;
        format!("{} {}", d.day(), M[d.month0() as usize])
    };
    format!("{day} в {}", t.format("%-H:%M"))
}

// ---------------------------------------------------------------- предпросмотр пака шаблонов

/// Строка пака: что это и что в ней можно поправить.
#[derive(Clone, Debug, Default)]
pub struct PackPart {
    /// 0 шаблон, 1 автоответ, 2 ответ на отзыв с оценкой star, 3 просьба оставить отзыв
    pub kind: u8,
    pub star: String,
    pub section: String,
    pub title: String,
    pub a: String,
    pub a_label: String,
    pub b: String,
    pub on: bool,
    pub exact: bool,
}

fn st(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

/// Всё, что есть в паке, по разделам.
pub fn pack_parts(data: &serde_json::Value) -> Vec<PackPart> {
    let mut out = Vec::new();
    let section = |name: &str, first: &mut bool| -> String {
        if *first {
            *first = false;
            name.to_string()
        } else {
            String::new()
        }
    };
    if let Some(list) = data.get("templates").and_then(|t| t.as_array()) {
        let mut first = true;
        for t in list {
            let text = st(t, "text");
            if text.is_empty() {
                continue;
            }
            let name = st(t, "name");
            out.push(PackPart { kind: 0, section: section("Шаблоны", &mut first), title: if name.is_empty() { "Шаблон".into() } else { name.clone() }, a: name, a_label: "Название кнопки".into(), b: text, on: true, ..Default::default() });
        }
    }
    if let Some(list) = data.get("auto_responses").and_then(|t| t.as_array()) {
        let mut first = true;
        for t in list {
            let (tr, resp) = (st(t, "trigger"), st(t, "response"));
            if tr.is_empty() || resp.is_empty() {
                continue;
            }
            out.push(PackPart {
                kind: 1,
                section: section("Автоответы на слова", &mut first),
                title: format!("Когда пишут \"{tr}\""),
                a: tr,
                a_label: "Слово или фраза".into(),
                b: resp,
                on: true,
                exact: t.get("exactMatch").and_then(|b| b.as_bool()).unwrap_or(false),
                ..Default::default()
            });
        }
    }
    if let Some(r) = data.get("review_reply_settings").filter(|v| v.is_object()) {
        let mut first = true;
        let disabled: Vec<String> = r.get("disabledStars").and_then(|d| d.as_array()).map(|a| a.iter().map(|x| x.to_string().trim_matches('"').to_string()).collect()).unwrap_or_default();
        if let Some(t) = r.get("manualTemplates").and_then(|t| t.as_object()) {
            let mut keys: Vec<&String> = t.keys().collect();
            keys.sort_by(|a, b| b.cmp(a));
            for k in keys {
                let text = t[k].as_str().unwrap_or("").to_string();
                if text.is_empty() {
                    continue;
                }
                let n: usize = k.parse().unwrap_or(5);
                out.push(PackPart {
                    kind: 2,
                    star: k.clone(),
                    section: section("Ответы на отзывы, в плагин \"Авто-ответы на отзывы\"", &mut first),
                    title: format!("Ответ на {} ({k})", "★".repeat(n.clamp(1, 5))),
                    b: text,
                    on: !disabled.contains(k),
                    ..Default::default()
                });
            }
        }
    }
    if let Some(r) = data.get("order_confirm_settings").filter(|v| v.is_object()) {
        let text = st(r, "text");
        if !text.is_empty() {
            out.push(PackPart { kind: 3, section: "Просьба оставить отзыв, в плагин \"Авто-сообщения\"".into(), title: "Когда покупатель подтвердил заказ".into(), b: text, on: true, ..Default::default() });
        }
    }
    if let Some(r) = data.get("greeting_settings").filter(|v| v.is_object()) {
        let text = st(r, "text");
        if !text.is_empty() {
            let hours = r.get("cooldownHours").and_then(|h| h.as_f64()).unwrap_or(24.0).max(1.0) as i64;
            out.push(PackPart {
                kind: 4,
                star: hours.to_string(),
                section: "Приветствие, в плагин \"Приветствие\" из каталога".into(),
                title: format!("Первое сообщение покупателю, не чаще раза в {hours} ч"),
                b: text,
                on: true,
                ..Default::default()
            });
        }
    }
    out
}

impl Ui {
    pub fn pack_open(&mut self, idx: i32) {
        let Some(p) = self.ext.cat_packs.get(idx.max(0) as usize).cloned() else { return };
        self.ext.pack_parts = pack_parts(&p.data);
        self.ext.pack_idx = idx;
        self.st(|s| {
            s.set_pack_name(p.name.clone().into());
            s.set_pack_author(p.author.clone().into());
            s.set_pack_view(idx);
        });
        self.rebuild_pack();
    }

    fn rebuild_pack(&mut self) {
        let rows: Vec<crate::PackItem> = self
            .ext
            .pack_parts
            .iter()
            .enumerate()
            .map(|(i, p)| crate::PackItem {
                idx: i as i32,
                section: p.section.clone().into(),
                title: p.title.clone().into(),
                a: p.a.clone().into(),
                a_label: p.a_label.clone().into(),
                b: p.b.clone().into(),
                on: p.on,
            })
            .collect();
        let n = self.ext.pack_parts.iter().filter(|p| p.on).count() as i32;
        self.st(|s| {
            s.set_pack_items(model(rows));
            s.set_pack_count(n);
        });
    }

    pub fn pack_item_toggle(&mut self, idx: i32, on: bool) {
        if let Some(p) = self.ext.pack_parts.get_mut(idx.max(0) as usize) {
            p.on = on;
        }
        self.rebuild_pack();
    }

    /// Правка текста: только в данных, чтобы не сбивать курсор в поле.
    pub fn pack_item_set(&mut self, idx: i32, field: i32, value: String) {
        if let Some(p) = self.ext.pack_parts.get_mut(idx.max(0) as usize) {
            if field == 0 {
                p.a = value;
            } else {
                p.b = value;
            }
        }
    }

    pub fn pack_apply(&mut self) {
        let parts: Vec<PackPart> = self.ext.pack_parts.iter().filter(|p| p.on).cloned().collect();
        let (mut tpl, mut autos, mut stars, mut ask) = (0, 0, 0, false);
        {
            let mut c = self.cfg.lock().unwrap();
            for p in parts.iter() {
                match p.kind {
                    0 => {
                        if p.b.trim().is_empty() || c.templates.iter().any(|x| x.name == p.a && x.text == p.b) {
                            continue;
                        }
                        c.templates.push(crate::templates::Template {
                            id: crate::templates::new_id(),
                            name: if p.a.trim().is_empty() { p.b.chars().take(20).collect() } else { p.a.clone() },
                            text: p.b.clone(),
                            color: String::new(),
                            image: String::new(),
                            image_first: true,
                            bind: 0,
                        });
                        tpl += 1;
                    }
                    1 => {
                        if p.a.trim().is_empty() || c.auto_cmds.iter().any(|x| x.trigger == p.a) {
                            continue;
                        }
                        c.auto_cmds.push(crate::templates::AutoCmd { id: crate::templates::new_id(), trigger: p.a.clone(), response: p.b.clone(), exact: p.exact, ..Default::default() });
                        autos += 1;
                    }
                    _ => {}
                }
            }
            c.save();
        }
        // ответы на отзывы: только отмеченные оценки
        let star_parts: Vec<&PackPart> = parts.iter().filter(|p| p.kind == 2).collect();
        if !star_parts.is_empty() {
            let id = "builtin.reviewreply";
            let mut s: serde_json::Value = crate::plugins::storage_get(id, "settings").and_then(|x| serde_json::from_str(&x).ok()).unwrap_or(serde_json::json!({
                "stars": {"1": true, "2": true, "3": true, "4": true, "5": true},
                "texts": {}, "useAi": false, "aiStyle": "", "aiFallback": "Спасибо за отзыв! 🤝", "aiLength": 3, "overwrite": false, "markRead": false
            }));
            for p in star_parts {
                s["texts"][p.star.as_str()] = serde_json::Value::String(p.b.clone());
                s["stars"][p.star.as_str()] = serde_json::Value::Bool(true);
                stars += 1;
            }
            crate::plugins::storage_set(id, "settings", Some(&s.to_string()));
            if crate::plugins::is_loaded(id) {
                crate::plugins::restart(id);
            }
        }
        if let Some(p) = parts.iter().find(|p| p.kind == 3) {
            let id = "builtin.automsg";
            let mut s: serde_json::Value = crate::plugins::storage_get(id, "settings").and_then(|x| serde_json::from_str(&x).ok()).unwrap_or(serde_json::json!({}));
            s["review"] = serde_json::json!({ "on": true, "text": p.b, "image": "", "imageFirst": true, "markRead": false });
            crate::plugins::storage_set(id, "settings", Some(&s.to_string()));
            if crate::plugins::is_loaded(id) {
                crate::plugins::restart(id);
            }
            ask = true;
        }
        let mut greet = false;
        if let Some(p) = parts.iter().find(|p| p.kind == 4) {
            let id = "user.greeting";
            let mut s: serde_json::Value = crate::plugins::storage_get(id, "settings").and_then(|x| serde_json::from_str(&x).ok()).unwrap_or(serde_json::json!({
                "skipIfTalked": true, "image": "", "imageFirst": false, "workHours": false, "from": 9, "to": 23
            }));
            s["text"] = serde_json::Value::String(p.b.clone());
            s["hours"] = serde_json::json!(p.star.parse::<i64>().unwrap_or(24));
            crate::plugins::storage_set(id, "settings", Some(&s.to_string()));
            if crate::plugins::is_loaded(id) {
                crate::plugins::restart(id);
            }
            greet = true;
        }
        let mut done = Vec::new();
        if greet {
            done.push("приветствие".to_string());
        }
        if tpl > 0 {
            done.push(format!("шаблонов: {tpl}"));
        }
        if autos > 0 {
            done.push(format!("автоответов: {autos}"));
        }
        if stars > 0 {
            done.push(format!("ответов на отзывы: {stars}"));
        }
        if ask {
            done.push("просьба оставить отзыв".into());
        }
        if done.is_empty() {
            self.toast("Всё это у вас уже есть");
            return;
        }
        let idx = self.ext.pack_idx;
        if let Some(e) = self.ext.cat_packs.get_mut(idx.max(0) as usize) {
            e.added = true;
        }
        self.st(|s| s.set_pack_view(-1));
        self.rebuild_catalog();
        self.rebuild_templates();
        self.toast(&format!("Добавлено: {}", done.join(", ")));
    }
}
