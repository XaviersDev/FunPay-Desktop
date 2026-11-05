//! Прокси аккаунтов, виджеты на рабочем столе и статус в Discord.

use crate::config::WidgetCfg;
use crate::ui::Ui;
use crate::worker::Cmd;
use crate::{proxy, widgets, Theme};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

/// Виджеты на рабочем столе выключены: код остаётся, но в программе их нет.
const WIDGETS_ENABLED: bool = false;

/// Сводка с FunPay для виджетов и Discord.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub balance: String,
    pub sales: usize,
    pub sales_sum: String,
    pub paid: usize,
    pub lots: Option<(usize, usize)>,
}

impl Ui {
    // ------------------------------------------------------------ прокси

    pub fn open_proxy(&mut self, id: String) {
        let Some(a) = self.cfg.lock().unwrap().accounts.iter().find(|a| a.id == id).cloned() else { return };
        self.st(|s| {
            s.set_proxy_acc(id.into());
            s.set_proxy_name(a.name.clone().into());
            s.set_proxy_kind(if a.proxy.is_empty() { 0 } else { proxy::kind_of(&a.proxy) as i32 });
            s.set_proxy_addr(proxy::bare(&a.proxy).into());
            s.set_proxy_strict(a.proxy_strict);
            s.set_proxy_state(0);
            s.set_proxy_status("".into());
            s.set_settings_open(false);
            s.set_proxy_open(true);
        });
    }

    fn proxy_form(&self) -> Result<(String, bool), String> {
        let (kind, addr, strict) = self.st(|s| (s.get_proxy_kind(), s.get_proxy_addr().to_string(), s.get_proxy_strict())).unwrap_or_default();
        Ok((proxy::normalize(kind.max(0) as usize, &addr)?, strict))
    }

    pub fn proxy_check(&mut self) {
        let url = match self.proxy_form() {
            Ok((u, _)) => u,
            Err(e) => {
                self.st(|s| {
                    s.set_proxy_state(3);
                    s.set_proxy_status(e.into());
                });
                return;
            }
        };
        self.st(|s| s.set_proxy_state(1));
        std::thread::spawn(move || {
            let r = proxy::check(&url);
            crate::ui::post(move |u| {
                u.st(|s| match r {
                    Ok(t) => {
                        s.set_proxy_state(2);
                        s.set_proxy_status(t.into());
                    }
                    Err(e) => {
                        s.set_proxy_state(3);
                        s.set_proxy_status(e.into());
                    }
                });
            });
        });
    }

    pub fn proxy_save(&mut self) {
        let (url, strict) = match self.proxy_form() {
            Ok(x) => x,
            Err(e) => {
                self.st(|s| {
                    s.set_proxy_state(3);
                    s.set_proxy_status(e.into());
                });
                return;
            }
        };
        self.set_account_proxy(url, strict);
        self.toast("Прокси сохранён");
    }

    pub fn proxy_clear(&mut self) {
        self.set_account_proxy(String::new(), false);
        self.toast("Аккаунт работает без прокси");
    }

    fn set_account_proxy(&mut self, url: String, strict: bool) {
        let id = self.st(|s| s.get_proxy_acc().to_string()).unwrap_or_default();
        let is_current = id == self.me.id;
        {
            let mut c = self.cfg.lock().unwrap();
            let Some(a) = c.accounts.iter_mut().find(|a| a.id == id) else { return };
            a.proxy = url;
            a.proxy_strict = strict;
            c.save();
        }
        self.st(|s| s.set_proxy_open(false));
        self.rebuild_accounts();
        if is_current {
            // текущая сессия сразу переходит на новый прокси
            self.send(Cmd::ProxyChanged);
            crate::plugins::reset_session();
        }
    }

    // ------------------------------------------------------------ виджеты и Discord

    /// Настройки в интерфейс и окна виджетов по конфигу (при запуске).
    pub fn sync_desk(&mut self) {
        let (list, top, d, ws) = {
            let c = self.cfg.lock().unwrap();
            (c.widgets.clone(), c.widgets_top, c.discord.clone(), c.widget_style.clone())
        };
        let on: Vec<bool> = (0..widgets::KINDS as i32).map(|k| list.iter().any(|w| w.kind == k)).collect();
        self.st(|s| {
            s.set_widget_on(ModelRc::from(Rc::new(VecModel::from(on))));
            s.set_widgets_top(top);
            s.set_wg_size(ws.size);
            s.set_wg_theme(ws.theme);
            s.set_wg_opacity(ws.opacity as f32);
            s.set_rpc_on(d.on);
            s.set_rpc_nick(d.nick);
            s.set_rpc_sales(d.sales);
        });
        self.apply_widgets();
        self.discord_refresh();
    }

    fn widget_look(&self) -> Option<(widgets::Look, f32)> {
        let st = self.cfg.lock().unwrap().widget_style.clone();
        let app = self.app()?;
        let accent = app.global::<Theme>().get_accent();
        Some((widgets::Look::from(&st, self.dark(), accent), app.window().scale_factor()))
    }

    fn apply_widgets_ex(&mut self, force: bool) {
        // виджеты на рабочем столе убраны из программы: окна не создаём
        if self.demo || !WIDGETS_ENABLED {
            let _ = force;
            return;
        }
        let (list, top) = {
            let c = self.cfg.lock().unwrap();
            (c.widgets.clone(), c.widgets_top)
        };
        let Some((look, scale)) = self.widget_look() else { return };
        widgets::sync(&list, top, look, scale, force);
    }

    fn apply_widgets(&mut self) {
        self.apply_widgets_ex(false);
    }

    /// Окна виджетов пропали (перезапустили проводник): создаём заново.
    pub fn widgets_rebuild(&mut self) {
        self.apply_widgets_ex(true);
    }

    /// Размер, тема и прозрачность фона виджетов.
    pub fn wg_style(&mut self, key: &str, v: i32) {
        let size_changed = {
            let mut c = self.cfg.lock().unwrap();
            let before = c.widget_style.size;
            match key {
                "size" => c.widget_style.size = v.clamp(0, 2),
                "theme" => c.widget_style.theme = v.clamp(0, 2),
                "opacity" => c.widget_style.opacity = v.clamp(0, 100),
                _ => {}
            }
            c.save();
            before != c.widget_style.size
        };
        if size_changed {
            // у окна новый размер: места по умолчанию пересчитываются, свои сохраняются
            self.apply_widgets_ex(true);
        } else if let Some((look, _)) = self.widget_look() {
            widgets::set_look(look);
        }
    }

    pub fn widget_toggle(&mut self, kind: i32, on: bool) {
        {
            let mut c = self.cfg.lock().unwrap();
            c.widgets.retain(|w| w.kind != kind);
            if on {
                c.widgets.push(WidgetCfg { kind, pos: None });
                c.widgets.sort_by_key(|w| w.kind);
            }
            c.save();
        }
        self.st(|s| {
            let m = s.get_widget_on();
            use slint::Model;
            if (kind as usize) < m.row_count() {
                m.set_row_data(kind as usize, on);
            }
        });
        self.apply_widgets();
        if on && kind != 1 {
            self.stats_tick();
        }
    }

    pub fn desk_set(&mut self, key: &str, v: bool) {
        {
            let mut c = self.cfg.lock().unwrap();
            match key {
                "top" => c.widgets_top = v,
                "rpc" => c.discord.on = v,
                "nick" => c.discord.nick = v,
                "sales" => c.discord.sales = v,
                _ => {}
            }
            c.save();
        }
        match key {
            "top" => {
                self.st(|s| s.set_wg_editing(false));
                if let Some((_, scale)) = self.widget_look() {
                    widgets::set_editing(false, scale);
                }
                self.apply_widgets();
            }
            "edit" => {
                if let Some((_, scale)) = self.widget_look() {
                    let moved = widgets::set_editing(v, scale);
                    self.widgets_moved(moved);
                }
            }
            _ => {
                if key == "rpc" && !v {
                    self.st(|s| s.set_rpc_status("".into()));
                }
                if v && (key == "sales" || key == "rpc") {
                    self.stats_tick();
                }
                self.discord_refresh();
            }
        }
    }

    pub fn widgets_reset(&mut self) {
        {
            let mut c = self.cfg.lock().unwrap();
            for w in c.widgets.iter_mut() {
                w.pos = None;
            }
            c.save();
        }
        // пересоздаём окна на местах по умолчанию
        self.apply_widgets_ex(true);
    }

    pub fn widgets_moved(&mut self, moved: Vec<(i32, (i32, i32))>) {
        if !moved.is_empty() {
            let mut c = self.cfg.lock().unwrap();
            for (kind, p) in moved {
                if let Some(w) = c.widgets.iter_mut().find(|w| w.kind == kind) {
                    w.pos = Some(p);
                }
            }
            c.save();
        }
        // заодно цвета темы
        if let Some((look, _)) = self.widget_look() {
            widgets::set_look(look);
        }
    }

    pub fn widget_clicked(&mut self, kind: i32) {
        self.show_window();
        match kind {
            0 => self.open_deals(2),
            2 => self.open_deals(0),
            3 => self.open_lots(),
            _ => {}
        }
    }

    /// Непрочитанные чаты для виджета.
    pub fn widgets_unread(&mut self, total: i32) {
        if !widgets::any() {
            return;
        }
        let c = self.cfg.lock().unwrap();
        let names: Vec<String> = self.chats.iter().filter(|x| x.unread && !c.muted.contains(&x.id) && !c.archived.contains(&x.id)).map(|x| x.name.clone()).take(3).collect();
        drop(c);
        widgets::update(|d| {
            d.unread = total;
            d.unread_names = names;
        });
    }

    /// Раз в несколько минут: баланс, продажи и лоты для виджетов и Discord.
    pub fn stats_tick(&mut self) {
        if self.demo || self.me.id.is_empty() {
            return;
        }
        let (list, d) = {
            let c = self.cfg.lock().unwrap();
            (c.widgets.clone(), c.discord.clone())
        };
        let list: Vec<WidgetCfg> = if WIDGETS_ENABLED { list } else { Vec::new() };
        let need_sales = list.iter().any(|w| w.kind == 0 || w.kind == 2) || (d.on && d.sales);
        let need_lots = list.iter().any(|w| w.kind == 3);
        if need_sales || need_lots {
            self.send(Cmd::Stats { lots: need_lots });
        }
    }

    pub fn stats_loaded(&mut self, r: Result<Stats, String>) {
        let Ok(s) = r else { return };
        let now = chrono::Local::now().format("%H:%M").to_string();
        if !s.balance.is_empty() {
            self.me.balance = s.balance.clone();
            self.st(|st| st.set_me_balance(s.balance.clone().into()));
        }
        let lots = s.lots;
        widgets::update(|d| {
            if !s.balance.is_empty() {
                d.balance = s.balance.clone();
            }
            d.balance_at = now;
            d.sales = Some((s.sales, s.sales_sum.clone(), s.paid));
            if lots.is_some() {
                d.lots = lots;
            }
        });
        self.ext.sales_today = Some(s.sales);
        self.discord_refresh();
    }

    pub fn discord_refresh(&mut self) {
        let d = self.cfg.lock().unwrap().discord.clone();
        if !d.on || self.demo || self.me.id.is_empty() {
            crate::discord::set(None);
            return;
        }
        let details = if d.nick && !self.me.name.is_empty() { format!("{} на FunPay", self.me.name) } else { "Торгует на FunPay".to_string() };
        let state = match (d.sales, self.ext.sales_today) {
            (true, Some(0)) => "Сегодня пока без продаж".to_string(),
            (true, Some(n)) => format!("Продаж сегодня: {n}"),
            _ => "Работает в FunPay Tools".to_string(),
        };
        let mut buttons = vec![("FunPay Tools".to_string(), "https://funpay.tools".to_string())];
        if d.nick && !self.me.id.is_empty() {
            buttons.insert(0, ("Профиль на FunPay".to_string(), format!("https://funpay.com/users/{}/", self.me.id)));
        }
        if self.ext.rpc_start == 0 {
            self.ext.rpc_start = chrono::Utc::now().timestamp();
        }
        crate::discord::set(Some(crate::discord::Act { details, state, buttons, start: self.ext.rpc_start }));
    }

    pub fn rpc_status(&mut self, s: String) {
        self.st(|st| st.set_rpc_status(s.into()));
    }
}

// ---------------------------------------------------------------- демо-экраны для скриншотов

impl Ui {
    pub fn demo_desk(&mut self, name: &str) {
        use crate::feat4::{CatEntry, PackEntry};
        use crate::PluginRow;
        use serde_json::json;
        let row = |id: &str, n: &str, d: &str, on: bool, builtin: bool| PluginRow {
            id: id.into(),
            name: n.into(),
            author: "FunPay Tools".into(),
            version: "1.0".into(),
            description: d.into(),
            on,
            builtin,
        };
        let rows = vec![
            row("builtin.autoraise", "Авто-поднятие", "", true, true),
            row("builtin.reviewreply", "Авто-ответы на отзывы", "", true, true),
            row("builtin.automsg", "Авто-сообщения", "", false, true),
            row("user.greeting", "Приветствие", "", true, false),
            row("user.sniper", "Unique's Sniper", "", false, false),
        ];
        match name {
            "plugins" => {
                self.st(|s| {
                    s.set_plugins(ModelRc::from(Rc::new(VecModel::from(rows))));
                    s.set_plugins_open(true);
                });
            }
            "plugin" | "pluginoff" => {
                self.st(|s| {
                    s.set_plugins(ModelRc::from(Rc::new(VecModel::from(rows))));
                    s.set_plugins_open(true);
                });
                self.plugin_open("builtin.automsg".into());
                let on = name == "plugin";
                self.st(|s| s.set_plugin_on(on));
                let ui = json!({ "type": "Column", "children": [
                    { "type": "Text", "text": "Переменные: $username, $order_id, $order_link, $lot_name, $date, $time", "color": "#7A8699" },
                    { "type": "Card", "children": [
                        { "type": "Switch", "text": "После оплаты", "stateKey": "paid_on" },
                        { "type": "Text", "text": "Сразу, как покупатель оплатил заказ", "color": "#7A8699", "fontSize": 12 },
                        { "type": "Input", "label": "Текст", "stateKey": "paid_text", "singleLine": false },
                        { "type": "Row", "children": [
                            { "type": "Button", "text": "Другая картинка", "style": "flat" },
                            { "type": "Button", "text": "Убрать картинку", "style": "flat" },
                        ] },
                        { "type": "Checkbox", "text": "Картинка перед текстом", "stateKey": "paid_first" },
                        { "type": "Checkbox", "text": "Отмечать чат прочитанным", "stateKey": "paid_read" },
                    ] },
                    { "type": "Card", "children": [
                        { "type": "Switch", "text": "Просьба оставить отзыв", "stateKey": "review_on" },
                        { "type": "Text", "text": "Когда покупатель подтвердил заказ, но ещё не оставил отзыв", "color": "#7A8699", "fontSize": 12 },
                    ] },
                    { "type": "Card", "children": [
                        { "type": "Switch", "text": "Напоминание подтвердить заказ", "stateKey": "remind_on" },
                        { "type": "Text", "text": "Если заказ всё ещё не подтверждён через заданное время", "color": "#7A8699", "fontSize": 12 },
                        { "type": "Slider", "label": "Через сколько часов", "min": 1, "max": 48, "stateKey": "remindHours" },
                    ] },
                ] });
                let state = json!({
                    "paid_on": "true", "paid_text": "Спасибо за покупку, $username! 🤝\nСкоро выдам заказ #$order_id, ожидайте.",
                    "paid_read": "false", "paid_first": "true", "review_on": "false", "remind_on": "true", "remindHours": "4"
                });
                self.plugin_slot("builtin.automsg".into(), Some((ui, state)));
            }
            "catalog" => {
                self.ext.cat_plugins = vec![
                    CatEntry { name: "Приветствие".into(), description: "Здоровается с покупателем, когда он пишет первым. Свой текст и картинка, пауза между приветствиями и рабочие часы.".into(), image: String::new(), url: String::new() },
                    CatEntry { name: "Курс валют в ответах".into(), description: "Подставляет в шаблоны свежий курс доллара и евро.".into(), image: String::new(), url: String::new() },
                    CatEntry { name: "Отчёт за день".into(), description: "Вечером присылает уведомление: сколько продаж, на какую сумму и какие лоты ушли лучше всего.".into(), image: String::new(), url: String::new() },
                ];
                self.st(|s| {
                    s.set_cat_open(true);
                    s.set_cat_tab(0);
                });
                self.rebuild_catalog();
            }
            "packs" | "tplcat" => {
                let pack = |n: &str, a: &str, d: &str, data: serde_json::Value| PackEntry {
                    name: n.into(),
                    author: a.into(),
                    description: d.into(),
                    data,
                    use_tpl: true,
                    use_reviews: true,
                    use_review_ask: true,
                    added: false,
                };
                let packs = vec![
                    pack("Вежливый продавец", "AlliSighs", "Приветствие, выдача, просьба об отзыве и ответы на все оценки.", json!({
                        "templates": [{"name": "Привет", "text": "Здравствуйте! Чем могу помочь?"}, {"name": "Выдал", "text": "Готово! Данные выше, проверьте, пожалуйста."}],
                        "auto_responses": [{"trigger": "!продавец", "response": "Позвал продавца, скоро ответит", "exactMatch": false}],
                        "review_reply_settings": {"manualTemplates": {"5": "Спасибо, $username! Рад, что всё понравилось", "4": "Спасибо за отзыв!", "1": "Жаль, что так вышло, напишите мне, решим"}, "disabledStars": [1]},
                        "order_confirm_settings": {"text": "Спасибо за заказ, $username! Буду благодарен за отзыв"},
                        "greeting_settings": {"text": "Привет, $username! Отвечу в течение пары минут", "cooldownHours": 48}
                    })),
                    pack("Ключи Steam", "keys_store24", "Шаблоны для продажи ключей: инструкция по активации, гарантия, замена.", json!({ "templates": [{"name": "Активация", "text": "..."}, {"name": "Гарантия", "text": "..."}] })),
                    pack("Буст", "vlad_storm", "Сроки, вход в аккаунт, отчёт о прогрессе.", json!({ "templates": [{"name": "Сроки", "text": "..."}], "review_reply_settings": {} })),
                ];
                self.pcat_loaded(None, Some(Some(packs)));
                if name == "packs" {
                    self.pack_open(0);
                }
                self.st(|s| {
                    if name == "tplcat" {
                        s.set_tpl_open(true);
                    }
                    s.set_cat_open(true);
                    s.set_cat_tab(1);
                });
                self.rebuild_catalog();
            }
            "proxy" => {
                self.open_proxy("2".into());
                self.st(|s| {
                    s.set_proxy_state(2);
                    s.set_proxy_status("Работает: IP 185.22.14.7, ответ за 212 мс".into());
                });
            }
            "desk" => {
                self.st(|s| {
                    s.set_widget_on(ModelRc::from(Rc::new(VecModel::from(vec![true, true, true, false]))));
                    s.set_rpc_on(true);
                    s.set_rpc_nick(true);
                    s.set_rpc_sales(false);
                    s.set_rpc_status("Статус виден в вашем профиле Discord.".into());
                    s.set_widgets_open(true);
                });
            }
            "discord" => {
                self.st(|s| {
                    s.set_rpc_on(true);
                    s.set_rpc_nick(true);
                    s.set_rpc_status("Статус виден в вашем профиле Discord.".into());
                    s.set_rpc_open(true);
                });
            }
            _ => {}
        }
    }
}
