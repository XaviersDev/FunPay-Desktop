//! Плагины и каталог: связка с интерфейсом.

use crate::plugins;
use crate::ui::{Ui, THUMB};
use crate::{CatPack, CatPlugin, PCell, PUiRow, PluginRow};
use serde_json::Value;
use slint::{Color, ModelRc, SharedString, VecModel};
use std::rc::Rc;

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

#[derive(Debug, Clone, Default)]
pub struct CatEntry {
    pub name: String,
    pub description: String,
    pub image: String,
    pub url: String,
}

#[derive(Debug, Clone, Default)]
pub struct PackEntry {
    pub name: String,
    pub author: String,
    pub description: String,
    pub data: Value,
    pub use_tpl: bool,
    pub use_reviews: bool,
    pub use_review_ask: bool,
    pub added: bool,
}

fn hex(c: &str) -> Option<Color> {
    let h = c.trim().trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).ok()?;
    match h.len() {
        6 => Some(Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)),
        8 => Some(Color::from_argb_u8(v as u8, (v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8)),
        _ => None,
    }
}

fn st(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(x) => x.to_string(),
    }
}

fn num(v: &Value, k: &str) -> f32 {
    v.get(k).and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0) as f32
}

impl Ui {
    // ------------------------------------------------------------ список

    pub fn open_plugins(&mut self) {
        self.rebuild_plugins();
        self.st(|s| s.set_plugins_open(true));
    }

    pub fn rebuild_plugins(&mut self) {
        let rows: Vec<PluginRow> = plugins::list()
            .into_iter()
            .map(|m| PluginRow {
                on: plugins::is_running(&m.id),
                id: m.id.into(),
                name: m.name.into(),
                author: m.author.into(),
                version: m.version.into(),
                description: m.description.into(),
                builtin: m.builtin,
            })
            .collect();
        self.st(|s| s.set_plugins(model(rows)));
    }

    /// После входа: запускаем включённые плагины.
    pub fn start_plugins(&mut self) {
        let on: Vec<String> = self.cfg.lock().unwrap().plugins_on.iter().filter(|(_, v)| **v).map(|(k, _)| k.clone()).collect();
        let known: Vec<String> = plugins::list().into_iter().map(|m| m.id).collect();
        for id in on {
            if known.contains(&id) {
                if let Err(e) = plugins::start(&id) {
                    crate::log::write(&format!("плагин {id}: {e}"));
                }
            }
        }
        plugins::start_updater();
    }

    pub fn plugin_toggle(&mut self, id: String, on: bool) {
        {
            let mut c = self.cfg.lock().unwrap();
            c.plugins_on.insert(id.clone(), on);
            c.save();
        }
        if on {
            if let Err(e) = plugins::start(&id) {
                self.toast(&e);
            }
        } else {
            plugins::stop(&id);
            self.ext.plugin_slots.remove(&id);
        }
        if self.ext.plugin_open == id {
            self.st(|s| s.set_plugin_on(on));
            self.rebuild_plugin_ui();
        }
        self.rebuild_plugins();
    }

    pub fn plugin_stopped(&mut self, id: String) {
        // за это время плагин уже запустили заново (например, из режима настроек во включённый)
        if plugins::is_loaded(&id) {
            return;
        }
        self.ext.plugin_slots.remove(&id);
        if self.ext.plugin_open == id {
            self.st(|s| s.set_plugin_on(false));
            // окно настроек открыто: показываем настройки и у выключенного
            plugins::start_settings(&id);
            self.rebuild_plugin_ui();
        }
        self.rebuild_plugins();
    }

    pub fn plugin_open(&mut self, id: String) {
        let Some(m) = plugins::list().into_iter().find(|m| m.id == id) else { return };
        self.ext.plugin_open = id.clone();
        let logs = plugins::logs(&id).join("\n");
        let on = plugins::is_running(&id);
        self.st(|s| {
            s.set_plugin_id(id.clone().into());
            s.set_plugin_name(m.name.clone().into());
            s.set_plugin_author(m.author.clone().into());
            s.set_plugin_version(m.version.clone().into());
            s.set_plugin_desc(m.description.clone().into());
            s.set_plugin_builtin(m.builtin);
            s.set_plugin_on(on);
            s.set_plugin_logs(logs.into());
            s.set_plugin_del(false);
            s.set_plugin_logs_open(false);
        });
        if !on && !self.demo {
            plugins::start_settings(&id);
        }
        self.rebuild_plugin_ui();
    }

    pub fn plugin_close(&mut self) {
        let id = std::mem::take(&mut self.ext.plugin_open);
        if !id.is_empty() && !plugins::is_running(&id) {
            plugins::stop(&id);
        }
        self.st(|s| s.set_plugin_id("".into()));
    }

    pub fn plugin_ui_click(&mut self, code: String) {
        if code.trim().is_empty() || self.ext.plugin_open.is_empty() {
            return;
        }
        plugins::send(&self.ext.plugin_open, plugins::Msg::Eval(code));
    }

    pub fn plugin_ui_set(&mut self, key: String, value: String, change: String) {
        let id = self.ext.plugin_open.clone();
        if id.is_empty() {
            return;
        }
        if let Some((_, state)) = self.ext.plugin_slots.get_mut(&id) {
            if !state.is_object() {
                *state = Value::Object(Default::default());
            }
            state[key.as_str()] = Value::String(value.clone());
        }
        // галочки и переключатели меняются на экране сразу, поля ввода не трогаем (там курсор)
        self.st(|s| {
            use slint::Model;
            let m = s.get_plugin_ui();
            for i in 0..m.row_count() {
                let Some(mut r) = m.row_data(i) else { continue };
                if r.key.as_str() == key && r.kind.as_str() != "input" && r.value.as_str() != value {
                    r.value = value.clone().into();
                    m.set_row_data(i, r);
                } else if r.kind.as_str() == "row" {
                    for j in 0..r.cells.row_count() {
                        if let Some(mut c) = r.cells.row_data(j) {
                            if c.key.as_str() == key && c.value.as_str() != value {
                                c.value = value.clone().into();
                                r.cells.set_row_data(j, c);
                            }
                        }
                    }
                }
            }
        });
        plugins::send(&id, plugins::Msg::SetState(key, value));
        if !change.trim().is_empty() {
            plugins::send(&id, plugins::Msg::Eval(change));
        }
    }

    pub fn plugin_slot(&mut self, id: String, slot: Option<(Value, Value)>) {
        match slot {
            Some(s) => {
                self.ext.plugin_slots.insert(id.clone(), s);
            }
            None => {
                self.ext.plugin_slots.remove(&id);
            }
        }
        if self.ext.plugin_open == id {
            self.rebuild_plugin_ui();
        }
    }

    pub fn plugin_state(&mut self, id: String, key: String, value: String) {
        let mut changed = false;
        if let Some((_, state)) = self.ext.plugin_slots.get_mut(&id) {
            if !state.is_object() {
                *state = Value::Object(Default::default());
            }
            changed = state.get(key.as_str()).and_then(|v| v.as_str()) != Some(value.as_str());
            state[key.as_str()] = Value::String(value);
        }
        // плагин поменял значение сам: поле на экране тоже меняется
        if changed && self.ext.plugin_open == id {
            self.rebuild_plugin_ui();
        }
    }

    pub fn plugin_log(&mut self, id: String, line: String) {
        if self.ext.plugin_open != id {
            return;
        }
        let cur = self.st(|s| s.get_plugin_logs().to_string()).unwrap_or_default();
        let mut lines: Vec<&str> = cur.lines().collect();
        lines.push(&line);
        let start = lines.len().saturating_sub(120);
        let text = lines[start..].join("\n");
        self.st(|s| s.set_plugin_logs(text.into()));
    }

    pub fn plugin_notify(&mut self, title: String, msg: String) {
        let pop = self.cfg.lock().unwrap().popup.clone();
        self.show_note("", &title, &msg, "", &pop);
    }

    pub fn plugin_ask(&mut self, id: String, what: String, tx: std::sync::mpsc::Sender<u8>) {
        // если уже спрашиваем другое, этот запрос сразу отклоняем
        if self.ext.plugin_ask_tx.is_some() {
            let _ = tx.send(0);
            return;
        }
        let name = plugins::list().into_iter().find(|m| m.id == id).map(|m| m.name).unwrap_or(id);
        self.ext.plugin_ask_tx = Some(tx);
        self.st(|s| {
            s.set_plugin_ask_name(name.into());
            s.set_plugin_ask(what.into());
        });
    }

    pub fn plugin_ask_answer(&mut self, answer: i32) {
        if let Some(tx) = self.ext.plugin_ask_tx.take() {
            let _ = tx.send(answer.clamp(0, 2) as u8);
        }
        self.st(|s| s.set_plugin_ask("".into()));
    }

    pub fn plugin_install_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title("Файл плагина").add_filter("Плагин FunPay Tools", &["js"]).pick_file() else { return };
        let Ok(code) = std::fs::read_to_string(&path) else {
            self.toast("Не удалось прочитать файл");
            return;
        };
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        match plugins::install(&code, &stem) {
            Ok(id) => {
                self.rebuild_plugins();
                self.toast("Плагин установлен. Включите его, когда будете готовы");
                self.plugin_open(id);
            }
            Err(e) => self.toast(&e),
        }
    }

    pub fn plugin_delete(&mut self) {
        let id = self.ext.plugin_open.clone();
        if id.is_empty() || id.starts_with("builtin.") {
            return;
        }
        plugins::remove(&id);
        {
            let mut c = self.cfg.lock().unwrap();
            c.plugins_on.remove(&id);
            c.save();
        }
        self.plugin_close();
        self.rebuild_plugins();
        self.toast("Плагин удалён");
    }

    pub fn plugin_docs(&mut self) {
        crate::sys::open_external(plugins::DOCS_URL);
    }

    /// Интерфейс плагина: дерево из JSON разворачиваем в плоский список строк.
    pub fn rebuild_plugin_ui(&mut self) {
        let id = self.ext.plugin_open.clone();
        let (ui, state) = match self.ext.plugin_slots.get(&id) {
            Some((u, s)) if plugins::is_loaded(&id) || self.demo => (u.clone(), s.clone()),
            _ => (Value::Null, Value::Null),
        };
        let mut rows = Vec::new();
        self.ext.plugin_img_keys.clear();
        if !ui.is_null() {
            self.flatten_ui(&ui, &state, false, &mut rows);
        }
        // границы карточек: строки одной карточки сливаются в один блок
        let mut out: Vec<PUiRow> = Vec::new();
        let mut brk = true;
        for mut r in rows {
            if r.kind == "cardbreak" {
                if let Some(l) = out.last_mut() {
                    if l.in_card {
                        l.card_last = true;
                    }
                }
                brk = true;
                continue;
            }
            if r.in_card {
                r.card_first = brk || !out.last().map(|l| l.in_card).unwrap_or(false);
            } else if let Some(l) = out.last_mut() {
                if l.in_card {
                    l.card_last = true;
                }
            }
            brk = false;
            out.push(r);
        }
        if let Some(l) = out.last_mut() {
            if l.in_card {
                l.card_last = true;
            }
        }
        self.st(|s| s.set_plugin_ui(model(out)));
    }

    fn flatten_ui(&mut self, v: &Value, state: &Value, in_card: bool, out: &mut Vec<PUiRow>) {
        if let Value::Array(a) = v {
            for x in a {
                self.flatten_ui(x, state, in_card, out);
            }
            return;
        }
        let kind = st(v, "type").to_lowercase();
        let key = st(v, "stateKey");
        let value = state.get(&key).map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).unwrap_or_default();
        let color = hex(&st(v, "color"));
        let mut row = PUiRow {
            kind: kind.clone().into(),
            text: st(v, "text").into(),
            label: st(v, "label").into(),
            key: key.clone().into(),
            value: value.clone().into(),
            click: st(v, "onClick").into(),
            change: st(v, "onChange").into(),
            style: st(v, "style").into(),
            bold: v.get("bold").and_then(|b| b.as_bool()).unwrap_or(false),
            size: num(v, "fontSize").max(num(v, "size")),
            has_color: color.is_some(),
            color: color.unwrap_or_default(),
            single: v.get("singleLine").and_then(|b| b.as_bool()).unwrap_or(true),
            min: num(v, "min"),
            max: if num(v, "max") > num(v, "min") { num(v, "max") } else { num(v, "min") + 100.0 },
            options: model(vec![]),
            height: num(v, "height"),
            radius: num(v, "radius"),
            busy_text: st(v, "busyText").into(),
            in_card,
            ..Default::default()
        };
        match kind.as_str() {
            "column" => {
                if let Some(c) = v.get("children") {
                    self.flatten_ui(c, state, in_card, out);
                }
            }
            "card" => {
                // карточка внутри карточки просто продолжает её
                if !in_card {
                    out.push(PUiRow { kind: "cardbreak".into(), ..Default::default() });
                }
                if let Some(c) = v.get("children") {
                    self.flatten_ui(c, state, true, out);
                }
                if !in_card {
                    out.push(PUiRow { kind: "cardbreak".into(), ..Default::default() });
                }
            }
            "row" => {
                let mut cells = Vec::new();
                for c in v.get("children").and_then(|c| c.as_array()).cloned().unwrap_or_default() {
                    let ck = st(&c, "type").to_lowercase();
                    if !matches!(ck.as_str(), "text" | "button" | "switch" | "checkbox") {
                        continue;
                    }
                    let key = st(&c, "stateKey");
                    let col = hex(&st(&c, "color"));
                    cells.push(PCell {
                        kind: ck.into(),
                        text: st(&c, "text").into(),
                        value: state.get(&key).and_then(|x| x.as_str()).unwrap_or("").into(),
                        key: key.into(),
                        click: st(&c, "onClick").into(),
                        change: st(&c, "onChange").into(),
                        style: st(&c, "style").into(),
                        bold: c.get("bold").and_then(|b| b.as_bool()).unwrap_or(false),
                        busy_text: st(&c, "busyText").into(),
                        has_color: col.is_some(),
                        color: col.unwrap_or_default(),
                    });
                }
                if !cells.is_empty() {
                    row.cells = model(cells);
                    out.push(row);
                }
            }
            "select" => {
                let opts: Vec<SharedString> = v
                    .get("options")
                    .and_then(|o| o.as_array())
                    .map(|a| a.iter().map(|x| SharedString::from(x.as_str().map(String::from).unwrap_or_else(|| x.to_string()))).collect())
                    .unwrap_or_default();
                row.options = model(opts);
                if value.is_empty() {
                    row.value = "0".into();
                }
                out.push(row);
            }
            "image" => {
                let url = st(v, "url");
                if !url.is_empty() {
                    let (k, im) = self.img(&url, THUMB);
                    self.ext.plugin_img_keys.push(k);
                    if let Some((img, _, _)) = im {
                        row.img = img;
                        row.has_img = true;
                    }
                    out.push(row);
                }
            }
            "text" | "button" | "switch" | "checkbox" | "input" | "slider" | "divider" | "spacer" => {
                if kind == "input" && row.text.is_empty() {
                    row.text = st(v, "placeholder").into();
                }
                out.push(row);
            }
            _ => {}
        }
    }

    pub fn plugin_image_ready(&mut self, key: &str) {
        if !self.ext.plugin_open.is_empty() && self.ext.plugin_img_keys.iter().any(|k| k == key) {
            self.rebuild_plugin_ui();
        }
        if self.ext.cat_plugins.iter().any(|p| crate::imgcache::key(THUMB, &p.image) == key) {
            self.rebuild_catalog();
        }
    }

    // ------------------------------------------------------------ каталог

    pub fn open_catalog(&mut self, tab: i32) {
        self.st(|s| {
            s.set_cat_open(true);
            s.set_cat_tab(tab);
            s.set_cat_error("".into());
        });
        let need = if tab == 0 { self.ext.cat_plugins.is_empty() } else { self.ext.cat_packs.is_empty() };
        if !need {
            self.rebuild_catalog();
            return;
        }
        self.st(|s| s.set_cat_loading(true));
        std::thread::spawn(move || {
            let c = crate::fp::http_client_follow(25);
            if tab == 0 {
                let res = c
                    .get(plugins::CATALOG_URL)
                    .send()
                    .ok()
                    .filter(|r| r.status().is_success())
                    .and_then(|r| r.text().ok())
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok());
                let list: Option<Vec<CatEntry>> = res.and_then(|v| v.as_array().cloned()).map(|a| {
                    a.iter()
                        .map(|x| CatEntry { name: st(x, "name"), description: st(x, "description"), image: st(x, "imageUrl"), url: st(x, "pluginUrl") })
                        .filter(|x| !x.url.is_empty())
                        .collect()
                });
                crate::ui::post(move |u| u.pcat_loaded(Some(list), None));
            } else {
                let res = c
                    .get("https://funpay.tools/api/catalog")
                    .send()
                    .ok()
                    .filter(|r| r.status().is_success())
                    .and_then(|r| r.text().ok())
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok());
                let list: Option<Vec<PackEntry>> = res.and_then(|v| v.as_array().cloned()).map(|a| {
                    a.iter()
                        .map(|x| {
                            let data = x.get("pack_data").cloned().unwrap_or(Value::Null);
                            let data = if let Value::String(s) = &data { serde_json::from_str(s).unwrap_or(Value::Null) } else { data };
                            PackEntry {
                                name: st(x, "name"),
                                author: st(x, "author"),
                                description: st(x, "description"),
                                data,
                                use_tpl: true,
                                use_reviews: true,
                                use_review_ask: true,
                                added: false,
                            }
                        })
                        .collect()
                });
                crate::ui::post(move |u| u.pcat_loaded(None, Some(list)));
            }
        });
    }

    pub fn pcat_loaded(&mut self, plugins_list: Option<Option<Vec<CatEntry>>>, packs: Option<Option<Vec<PackEntry>>>) {
        self.st(|s| s.set_cat_loading(false));
        if let Some(p) = plugins_list {
            match p {
                Some(v) => self.ext.cat_plugins = v,
                None => self.st(|s| s.set_cat_error("Каталог не загрузился, проверьте интернет".into())).unwrap_or(()),
            }
        }
        if let Some(p) = packs {
            match p {
                Some(v) => self.ext.cat_packs = v,
                None => self.st(|s| s.set_cat_error("Каталог не загрузился, проверьте интернет".into())).unwrap_or(()),
            }
        }
        self.rebuild_catalog();
    }

    pub fn rebuild_catalog(&mut self) {
        let installed: Vec<String> = plugins::list().into_iter().map(|m| m.name.to_lowercase()).collect();
        let list = self.ext.cat_plugins.clone();
        let busy = self.ext.cat_busy - 1;
        let rows: Vec<CatPlugin> = list
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let im = if p.image.is_empty() { None } else { self.img(&p.image, THUMB).1 };
                CatPlugin {
                    idx: i as i32,
                    name: p.name.clone().into(),
                    description: p.description.clone().into(),
                    has_banner: im.is_some(),
                    banner: im.map(|x| x.0).unwrap_or_default(),
                    installed: installed.contains(&p.name.to_lowercase()),
                    busy: busy == i as i32,
                }
            })
            .collect();
        let packs: Vec<CatPack> = self
            .ext
            .cat_packs
            .iter()
            .enumerate()
            .map(|(i, p)| CatPack {
                idx: i as i32,
                name: p.name.clone().into(),
                author: p.author.clone().into(),
                description: p.description.clone().into(),
                tpl_count: p.data.get("templates").and_then(|t| t.as_array()).map(|a| a.len()).unwrap_or(0) as i32,
                has_reviews: p.data.get("review_reply_settings").map(|v| v.is_object()).unwrap_or(false),
                has_review_ask: p.data.get("order_confirm_settings").map(|v| v.is_object()).unwrap_or(false),
                use_tpl: p.use_tpl,
                use_reviews: p.use_reviews,
                use_review_ask: p.use_review_ask,
                added: p.added,
            })
            .collect();
        self.st(|s| {
            s.set_cat_plugins(model(rows));
            s.set_cat_packs(model(packs));
        });
    }

    pub fn cat_install(&mut self, idx: i32) {
        let Some(p) = self.ext.cat_plugins.get(idx.max(0) as usize).cloned() else { return };
        self.ext.cat_busy = idx + 1;
        self.rebuild_catalog();
        std::thread::spawn(move || {
            let c = crate::fp::http_client_follow(30);
            let code = c.get(&p.url).send().ok().filter(|r| r.status().is_success()).and_then(|r| r.text().ok());
            crate::ui::post(move |u| {
                u.ext.cat_busy = 0;
                match code {
                    Some(code) => match plugins::install(&code, "") {
                        Ok(id) => {
                            u.rebuild_plugins();
                            u.toast(&format!("Плагин \"{}\" установлен. Включите его в меню плагинов", p.name));
                            let _ = id;
                        }
                        Err(e) => u.toast(&e),
                    },
                    None => u.toast("Плагин не скачался, проверьте интернет"),
                }
                u.rebuild_catalog();
            });
        });
    }

    pub fn cat_pack_flag(&mut self, idx: i32, which: i32, on: bool) {
        if let Some(p) = self.ext.cat_packs.get_mut(idx.max(0) as usize) {
            match which {
                0 => p.use_tpl = on,
                1 => p.use_reviews = on,
                _ => p.use_review_ask = on,
            }
        }
        self.rebuild_catalog();
    }

    /// Добавить выбранное из пака: шаблоны в программу, ответы на отзывы и просьбу отзыва в плагины.
    pub fn cat_pack_add(&mut self, idx: i32) {
        let Some(p) = self.ext.cat_packs.get(idx.max(0) as usize).cloned() else { return };
        let mut done = Vec::new();
        if p.use_tpl {
            if let Some(list) = p.data.get("templates").and_then(|t| t.as_array()) {
                let mut c = self.cfg.lock().unwrap();
                let mut n = 0;
                for t in list {
                    let name = st(t, "name");
                    let text = st(t, "text");
                    if text.is_empty() || c.templates.iter().any(|x| x.name == name && x.text == text) {
                        continue;
                    }
                    c.templates.push(crate::templates::Template {
                        id: crate::templates::new_id(),
                        name: if name.is_empty() { text.chars().take(20).collect() } else { name },
                        text,
                        color: String::new(),
                        image: String::new(),
                        image_first: true,
                        bind: 0,
                    });
                    n += 1;
                }
                c.save();
                if n > 0 {
                    done.push(format!("шаблонов: {n}"));
                }
            }
        }
        if p.use_reviews {
            if let Some(r) = p.data.get("review_reply_settings").filter(|v| v.is_object()) {
                let id = "builtin.reviewreply";
                let mut s: Value = plugins::storage_get(id, "settings").and_then(|x| serde_json::from_str(&x).ok()).unwrap_or(serde_json::json!({
                    "stars": {"1": true, "2": true, "3": true, "4": true, "5": true},
                    "texts": {}, "useAi": false, "aiStyle": "", "aiFallback": "Спасибо за отзыв! 🤝", "aiLength": 3, "overwrite": false, "markRead": false
                }));
                if let Some(t) = r.get("manualTemplates").and_then(|t| t.as_object()) {
                    for (k, v) in t {
                        s["texts"][k] = v.clone();
                    }
                }
                let disabled: Vec<String> = r.get("disabledStars").and_then(|d| d.as_array()).map(|a| a.iter().map(|x| x.to_string()).collect()).unwrap_or_default();
                for n in 1..=5 {
                    s["stars"][n.to_string()] = Value::Bool(!disabled.contains(&n.to_string()));
                }
                if let Some(b) = r.get("useAi").and_then(|b| b.as_bool()) {
                    s["useAi"] = Value::Bool(b);
                }
                let fallback = st(r, "aiFallbackText");
                if !fallback.is_empty() {
                    s["aiFallback"] = Value::String(fallback);
                }
                let style = [st(r, "aiWritingStyle"), st(r, "aiCustomInstruction")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(". ");
                if !style.is_empty() {
                    s["aiStyle"] = Value::String(style);
                }
                plugins::storage_set(id, "settings", Some(&s.to_string()));
                if plugins::is_running(id) {
                    plugins::restart(id);
                }
                done.push("ответы на отзывы".into());
            }
        }
        if p.use_review_ask {
            if let Some(r) = p.data.get("order_confirm_settings").filter(|v| v.is_object()) {
                let id = "builtin.automsg";
                let mut s: Value = plugins::storage_get(id, "settings").and_then(|x| serde_json::from_str(&x).ok()).unwrap_or(serde_json::json!({}));
                let text = st(r, "text");
                if !text.is_empty() {
                    let on = r.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
                    s["review"] = serde_json::json!({ "on": on, "text": text, "image": "", "imageFirst": true, "markRead": false });
                    plugins::storage_set(id, "settings", Some(&s.to_string()));
                    if plugins::is_running(id) {
                        plugins::restart(id);
                    }
                    done.push("просьба оставить отзыв".into());
                }
            }
        }
        if let Some(e) = self.ext.cat_packs.get_mut(idx.max(0) as usize) {
            e.added = !done.is_empty();
        }
        self.rebuild_catalog();
        if done.is_empty() {
            self.toast("Нечего добавлять: отметьте, что взять из пака");
        } else {
            self.rebuild_templates();
            self.toast(&format!("Добавлено: {}", done.join(", ")));
        }
    }
}
