//! Мои лоты и свои темы: связка с интерфейсом.

use crate::lots::{Form, MyLot};
use crate::ui::{Ui, THUMB};
use crate::worker::Cmd;
use crate::{LotFieldRow, LotRow, LotSection, ThemeCard};
use slint::{Color, ModelRc, SharedString, VecModel};
use std::rc::Rc;

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

const THEMES_RAW: &str = "https://raw.githubusercontent.com/XaviersDev/fpt-themes/main/";

#[derive(Debug, Clone, Default)]
pub struct CatalogTheme {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub author: String,
    pub file: String,
    pub preview: String,
}

fn abs(u: &str) -> String {
    if u.starts_with("http") || u.is_empty() {
        u.to_string()
    } else {
        format!("{THEMES_RAW}{}", u.trim_start_matches('/'))
    }
}

pub fn parse_catalog(text: &str) -> Vec<CatalogTheme> {
    let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let arr = v.as_array().cloned().or_else(|| v.get("themes").and_then(|t| t.as_array()).cloned()).unwrap_or_default();
    let s = |o: &serde_json::Value, k: &str| o.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    arr.iter()
        .filter_map(|o| {
            let file = s(o, "file");
            if file.is_empty() {
                return None;
            }
            Some(CatalogTheme {
                id: if s(o, "id").is_empty() { file.clone() } else { s(o, "id") },
                name: s(o, "name"),
                desc: s(o, "desc"),
                author: s(o, "author"),
                file: abs(&file),
                preview: abs(&s(o, "preview")),
            })
        })
        .collect()
}

fn hex(c: &str) -> Color {
    crate::templates::parse_hex(c).map(|(r, g, b)| Color::from_rgb_u8(r, g, b)).unwrap_or(Color::from_rgb_u8(0x2F, 0x80, 0xED))
}

impl Ui {
    // ------------------------------------------------------------ мои лоты

    pub fn open_lots(&mut self) {
        if self.panes.last() != Some(&5) {
            self.panes.retain(|x| *x != 5);
            self.panes.push(5);
        }
        self.st(|s| {
            s.set_pane(5);
            s.set_card_open(false);
            s.set_settings_open(false);
        });
        self.lots_refresh();
    }

    pub fn lots_refresh(&mut self) {
        self.ext.lots_gen += 1;
        let gen = self.ext.lots_gen;
        self.st(|s| {
            s.set_lots_loading(true);
            s.set_lots_error("".into());
        });
        self.send(Cmd::LoadLots { gen });
    }

    fn inactive(&self) -> Vec<MyLot> {
        self.cfg.lock().unwrap().inactive_lots.get(&self.me.id).cloned().unwrap_or_default()
    }

    fn set_inactive(&mut self, f: impl FnOnce(&mut Vec<MyLot>)) {
        let id = self.me.id.clone();
        let mut c = self.cfg.lock().unwrap();
        let list = c.inactive_lots.entry(id).or_default();
        f(list);
        c.save();
    }

    pub fn lots_loaded(&mut self, gen: u64, res: Result<Vec<MyLot>, String>) {
        if gen != self.ext.lots_gen {
            return;
        }
        self.st(|s| s.set_lots_loading(false));
        match res {
            Ok(mut v) => {
                // выключенные FunPay не показывает: добавляем запомненные
                let live: std::collections::HashSet<String> = v.iter().map(|l| l.id.clone()).collect();
                self.set_inactive(|list| list.retain(|l| !live.contains(&l.id)));
                for l in self.inactive() {
                    v.push(MyLot { active: false, ..l });
                }
                self.ext.lots = v;
                self.rebuild_lots();
            }
            Err(e) => {
                self.st(|s| s.set_lots_error(e.into()));
            }
        }
    }

    pub fn rebuild_lots(&mut self) {
        let q = self.ext.lots_query.trim().to_lowercase();
        let filter = self.ext.lots_filter;
        let all = &self.ext.lots;
        let (on, off, auto) = (all.iter().filter(|l| l.active).count(), all.iter().filter(|l| !l.active).count(), all.iter().filter(|l| l.auto).count());
        let list: Vec<&MyLot> = all
            .iter()
            .filter(|l| q.is_empty() || l.title.to_lowercase().contains(&q) || l.category.to_lowercase().contains(&q))
            .filter(|l| match filter {
                1 => l.active,
                2 => !l.active,
                _ => true,
            })
            .collect();
        // по категориям, внутри как на FunPay
        let mut sections: Vec<(String, Vec<&MyLot>)> = Vec::new();
        for l in list {
            match sections.iter_mut().find(|(c, _)| *c == l.category) {
                Some((_, v)) => v.push(l),
                None => sections.push((l.category.clone(), vec![l])),
            }
        }
        let rows: Vec<LotSection> = sections
            .into_iter()
            .map(|(cat, lots)| {
                let (game, kind) = split_category(&cat);
                let (c1, c2) = crate::ui::pair(&game);
                let mono = monogram(&game);
                let items: Vec<LotRow> = lots
                    .iter()
                    .map(|l| LotRow {
                        header: false,
                        id: l.id.clone().into(),
                        title: l.title.clone().into(),
                        category: l.category.clone().into(),
                        price: fmt_price(&l.price).into(),
                        amount: l.amount.clone().into(),
                        server: l.server.clone().into(),
                        meta: l.server.clone().into(),
                        active: l.active,
                        auto: l.auto,
                        c1,
                        c2,
                        mono: mono.clone().into(),
                        kind: kind.clone().into(),
                        game: game.clone().into(),
                    })
                    .collect();
                LotSection {
                    name: cat.clone().into(),
                    game: game.into(),
                    kind: kind.into(),
                    mono: mono.into(),
                    c1,
                    c2,
                    count: items.len() as i32,
                    lots: model(items),
                }
            })
            .collect();
        let n = self.ext.lots.len() as i32;
        self.st(|s| {
            s.set_lot_sections(model(rows));
            s.set_lots_count(n);
            s.set_lots_on(on as i32);
            s.set_lots_off(off as i32);
            s.set_lots_auto(auto as i32);
            s.set_lots_filter(filter);
        });
    }

    pub fn lots_filter(&mut self, i: i32) {
        self.ext.lots_filter = i.clamp(0, 2);
        self.rebuild_lots();
    }

    pub fn lots_search(&mut self, q: String) {
        self.ext.lots_query = q;
        self.rebuild_lots();
    }

    fn lot(&self, id: &str) -> Option<MyLot> {
        self.ext.lots.iter().find(|l| l.id == id).cloned()
    }

    pub fn lot_edit(&mut self, id: String) {
        let title = self.lot(&id).map(|l| l.title).unwrap_or_default();
        self.ext.lot_gen += 1;
        let gen = self.ext.lot_gen;
        self.ext.lot_form = None;
        self.st(|s| {
            s.set_lot_open(true);
            s.set_lot_loading(true);
            s.set_lot_busy(false);
            s.set_lot_id(id.clone().into());
            s.set_lot_title(title.into());
            s.set_lot_status("".into());
            s.set_lot_tab(0);
        });
        self.ext.lot_lang = 0;
        let cat = self.lot(&id).map(|l| l.category).unwrap_or_default();
        let price = self.lot(&id).map(|l| fmt_price(&l.price)).unwrap_or_default();
        let (game, _) = split_category(&cat);
        let (c1, c2) = crate::ui::pair(&game);
        let mono = monogram(&game);
        self.st(|s| {
            s.set_lot_c1(c1);
            s.set_lot_c2(c2);
            s.set_lot_mono(mono.into());
            s.set_lot_cat(cat.into());
            s.set_lot_prev_price(price.into());
            s.set_lot_lang(0);
            for m in [0, 1, 2, 3, 4] {
                let e: ModelRc<LotFieldRow> = model(vec![]);
                match m {
                    0 => s.set_lot_main(e),
                    1 => s.set_lot_flags(e),
                    2 => s.set_lot_text(e),
                    3 => s.set_lot_params(e),
                    _ => s.set_lot_after(e),
                }
            }
        });
        self.send(Cmd::LotForm { id, gen });
    }

    pub fn lot_form_loaded(&mut self, gen: u64, res: Result<Form, String>) {
        if gen != self.ext.lot_gen {
            return;
        }
        self.st(|s| s.set_lot_loading(false));
        match res {
            Ok(f) => {
                self.st(|s| s.set_lot_currency(f.currency.clone().into()));
                self.ext.lot_form = Some(f);
                self.rebuild_lot_fields();
            }
            Err(e) => {
                self.st(|s| s.set_lot_status(e.into()));
            }
        }
    }

    /// Поля для редактора по вкладкам: скрытые не показываем, условные по текущим значениям.
    pub fn rebuild_lot_fields(&mut self) {
        let Some(f) = self.ext.lot_form.as_ref() else { return };
        let lang = if self.ext.lot_lang == 1 { "en" } else { "ru" };
        let cur = f.currency.clone();
        let fields: Vec<&crate::lots::Field> = f.fields.iter().filter(|x| x.kind != "hidden" && f.visible(x)).collect();
        let has_en = fields.iter().any(|x| field_lang(x) == "en");
        let row = |x: &crate::lots::Field| -> LotFieldRow {
            let opts: Vec<SharedString> = x.options.iter().map(|(_, l)| SharedString::from(if l.is_empty() { "-" } else { l.as_str() })).collect();
            let sel = x.options.iter().position(|(v, _)| *v == x.value).map(|i| i as i32).unwrap_or(0);
            let mut label = crate::lots::nice_label(x);
            // язык и так виден по переключателю
            for suf in [" (RU)", " (EN)"] {
                label = label.trim_end_matches(suf).to_string();
            }
            if x.name == "secrets" {
                label = "Товары, для переноса строки используйте /n".into();
            }
            LotFieldRow {
                name: x.name.clone().into(),
                kind: x.kind.clone().into(),
                label: label.into(),
                value: x.value.clone().into(),
                hint: x.hint.clone().into(),
                placeholder: if x.name == "price" && x.placeholder.is_empty() { cur.clone().into() } else { x.placeholder.clone().into() },
                options: model(opts),
                sel,
            }
        };
        let (mut main, mut flags, mut text, mut params, mut after) = (vec![], vec![], vec![], vec![], vec![]);
        for x in &fields {
            let n = x.name.as_str();
            let l = field_lang(x);
            if n == "price" || n == "amount" {
                main.push((if n == "price" { 0 } else { 1 }, row(x)));
            } else if matches!(n, "active" | "auto_delivery" | "deactivate_after_sale") {
                flags.push((match n { "active" => 0, "auto_delivery" => 1, _ => 2 }, row(x)));
            } else if n.starts_with("fields[summary]") || n.starts_with("fields[desc]") {
                if l.is_empty() || l == lang {
                    text.push((if n.starts_with("fields[summary]") { 0 } else { 1 }, row(x)));
                }
            } else if n.starts_with("fields[payment_msg]") || n.starts_with("secrets") {
                if l.is_empty() || l == lang || n.starts_with("secrets") {
                    after.push((if n.starts_with("secrets") { 1 } else { 0 }, row(x)));
                }
            } else {
                params.push((0, row(x)));
            }
        }
        let sorted = |mut v: Vec<(i32, LotFieldRow)>| -> ModelRc<LotFieldRow> {
            v.sort_by_key(|x| x.0);
            model(v.into_iter().map(|x| x.1).collect())
        };
        let (main, flags, text, params, after) = (sorted(main), sorted(flags), sorted(text), sorted(params), sorted(after));
        let lang_i = self.ext.lot_lang;
        self.st(|s| {
            s.set_lot_main(main);
            s.set_lot_flags(flags);
            s.set_lot_text(text);
            s.set_lot_params(params);
            s.set_lot_after(after);
            s.set_lot_has_en(has_en);
            s.set_lot_lang(lang_i);
        });
        self.lot_preview();
    }

    /// Живое превью в шапке редактора: цена из поля.
    fn lot_preview(&mut self) {
        let Some(f) = self.ext.lot_form.as_ref() else { return };
        let price = f.get("price").map(|x| x.value.trim().to_string()).unwrap_or_default();
        let cur = if f.currency.is_empty() { "₽".to_string() } else { f.currency.clone() };
        let text = if price.is_empty() { String::new() } else { fmt_price(&format!("{price} {cur}")) };
        self.st(|s| s.set_lot_prev_price(text.into()));
    }

    pub fn lot_lang(&mut self, i: i32) {
        self.ext.lot_lang = i.clamp(0, 1);
        self.rebuild_lot_fields();
    }

    pub fn lot_field(&mut self, name: String, value: String) {
        let Some(f) = self.ext.lot_form.as_mut() else { return };
        let kind = f.get(&name).map(|x| x.kind.clone()).unwrap_or_default();
        f.set(&name, &value);
        // от чекбоксов могут зависеть другие поля
        if kind == "checkbox" {
            self.rebuild_lot_fields();
        } else if name == "price" {
            self.lot_preview();
        }
    }

    pub fn lot_pick(&mut self, name: String, idx: i32) {
        let Some(f) = self.ext.lot_form.as_mut() else { return };
        let Some(v) = f.get(&name).and_then(|x| x.options.get(idx.max(0) as usize)).map(|o| o.0.clone()) else { return };
        let before: Vec<bool> = f.fields.iter().map(|x| f.visible(x)).collect();
        f.set(&name, &v);
        let after: Vec<bool> = f.fields.iter().map(|x| f.visible(x)).collect();
        if before != after {
            self.rebuild_lot_fields();
        }
    }

    pub fn lot_save(&mut self) {
        let Some(f) = self.ext.lot_form.clone() else { return };
        let id = self.st(|s| s.get_lot_id().to_string()).unwrap_or_default();
        if f.get("price").map(|p| p.value.trim().is_empty()).unwrap_or(false) {
            self.st(|s| s.set_lot_status("Укажите цену".into()));
            return;
        }
        self.st(|s| {
            s.set_lot_busy(true);
            s.set_lot_status("".into());
        });
        self.send(Cmd::LotSave { id, form: f });
    }

    pub fn lot_saved(&mut self, id: String, res: Result<(), String>) {
        self.st(|s| s.set_lot_busy(false));
        match res {
            Ok(()) => {
                let active = self.ext.lot_form.as_ref().map(|f| matches!(f.value("active").as_str(), "on" | "1" | "true")).unwrap_or(true);
                if let Some(l) = self.lot(&id) {
                    if active {
                        self.set_inactive(|v| v.retain(|x| x.id != id));
                    } else {
                        self.set_inactive(|v| {
                            v.retain(|x| x.id != id);
                            v.push(MyLot { active: false, ..l });
                        });
                    }
                }
                self.st(|s| {
                    s.set_lot_status("Сохранено".into());
                    s.set_lot_open(false);
                });
                self.toast("Лот сохранён");
                self.lots_refresh();
            }
            Err(e) => {
                self.st(|s| s.set_lot_status(e.into()));
            }
        }
    }

    pub fn lot_toggle(&mut self, id: String, on: bool) {
        let node = self.lot(&id).map(|l| l.node).unwrap_or_default();
        if let Some(l) = self.ext.lots.iter_mut().find(|l| l.id == id) {
            l.active = on;
        }
        self.rebuild_lots();
        self.send(Cmd::LotActive { id, node, on });
    }

    pub fn lot_active_done(&mut self, id: String, on: bool, res: Result<(), String>) {
        match res {
            Ok(()) => {
                if let Some(l) = self.lot(&id) {
                    if on {
                        self.set_inactive(|v| v.retain(|x| x.id != id));
                    } else {
                        self.set_inactive(|v| {
                            v.retain(|x| x.id != id);
                            v.push(MyLot { active: false, ..l });
                        });
                    }
                }
                self.toast(if on { "Лот включён" } else { "Лот выключен" });
            }
            Err(e) => {
                if let Some(l) = self.ext.lots.iter_mut().find(|l| l.id == id) {
                    l.active = !on;
                }
                self.rebuild_lots();
                self.toast(&e);
            }
        }
    }

    pub fn lot_copy(&mut self, id: String) {
        let node = self.lot(&id).map(|l| l.node).unwrap_or_default();
        self.toast("Делаю копию…");
        self.send(Cmd::LotCopy { id, node });
    }

    pub fn lot_simple_done(&mut self, ok_text: &str, res: Result<(), String>) {
        match res {
            Ok(()) => {
                self.toast(ok_text);
                self.lots_refresh();
            }
            Err(e) => self.toast(&e),
        }
    }

    pub fn lot_delete(&mut self, id: String) {
        self.send(Cmd::LotDelete { id });
    }

    pub fn lot_deleted(&mut self, id: String, res: Result<(), String>) {
        match res {
            Ok(()) => {
                self.set_inactive(|v| v.retain(|x| x.id != id));
                self.ext.lots.retain(|l| l.id != id);
                self.rebuild_lots();
                self.toast("Лот удалён");
            }
            Err(e) => self.toast(&e),
        }
    }

    pub fn lot_web(&mut self, id: String) {
        let url = if id.is_empty() { format!("https://funpay.com/users/{}/", self.me.id) } else { format!("https://funpay.com/lots/offerEdit?offer={id}") };
        self.open_url(url);
    }

    // ------------------------------------------------------------ темы

    pub fn open_themes(&mut self) {
        let t = self.cfg.lock().unwrap().custom_theme.clone();
        self.st(|s| {
            s.set_themes_open(true);
            s.set_ct_bg1(t.bg_color1.clone().into());
            s.set_ct_bg2(t.bg_color2.clone().into());
            s.set_ct_panel(t.container_bg_color.clone().into());
            s.set_ct_text(t.text_color.clone().into());
            s.set_ct_accent(t.link_color.clone().into());
            s.set_ct_opacity(t.container_bg_opacity);
            s.set_ct_blur(t.bg_blur);
            s.set_ct_bright(t.bg_brightness);
            s.set_ct_glass(t.enable_glassmorphism);
            s.set_ct_glass_blur(t.glassmorphism_blur);
        });
        self.ct_load_image(&t.bg_image);
        if self.ext.catalog.is_empty() {
            self.st(|s| {
                s.set_themes_loading(true);
                s.set_themes_error("".into());
            });
            std::thread::spawn(|| {
                let c = crate::fp::http_client_follow(20);
                let res = c
                    .get(format!("{THEMES_RAW}index.json"))
                    .send()
                    .ok()
                    .filter(|r| r.status().is_success())
                    .and_then(|r| r.text().ok())
                    .map(|t| parse_catalog(&t))
                    .filter(|v| !v.is_empty());
                crate::ui::post(move |u| u.catalog_loaded(res));
            });
        } else {
            self.rebuild_themes();
        }
    }

    fn catalog_loaded(&mut self, res: Option<Vec<CatalogTheme>>) {
        self.st(|s| s.set_themes_loading(false));
        match res {
            Some(v) => {
                self.ext.catalog = v;
                self.rebuild_themes();
            }
            None => {
                self.st(|s| s.set_themes_error("Каталог тем не загрузился, проверьте интернет".into()));
            }
        }
    }

    pub fn rebuild_themes(&mut self) {
        let list = self.ext.catalog.clone();
        let rows: Vec<ThemeCard> = list
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let im = if t.preview.is_empty() { None } else { self.img(&t.preview, THUMB).1 };
                let (c1, c2) = crate::ui::pair(&format!("{}{i}", t.id));
                ThemeCard {
                    id: t.id.clone().into(),
                    name: t.name.clone().into(),
                    desc: t.desc.clone().into(),
                    author: t.author.clone().into(),
                    has_preview: im.is_some(),
                    preview: im.map(|x| x.0).unwrap_or_default(),
                    c1,
                    c2,
                }
            })
            .collect();
        self.st(|s| s.set_themes(model(rows)));
    }

    /// Картинка превью темы догрузилась.
    pub fn theme_preview_ready(&mut self, key: &str) {
        if self.ext.catalog.iter().any(|t| !t.preview.is_empty() && crate::imgcache::key(THUMB, &t.preview) == key) {
            self.rebuild_themes();
        }
    }

    pub fn theme_apply(&mut self, id: String) {
        if self.demo {
            eprintln!("theme_apply {id}");
        }
        let Some(t) = self.ext.catalog.iter().find(|t| t.id == id).cloned() else { return };
        self.st(|s| s.set_theme_applying(id.clone().into()));
        std::thread::spawn(move || {
            let c = crate::fp::http_client_follow(30);
            let text = c.get(&t.file).send().ok().filter(|r| r.status().is_success()).and_then(|r| r.text().ok());
            let res = match text {
                Some(txt) => crate::tgtheme::from_fptheme(&txt, &t.name).map(|x| x.0),
                None => Err("Тема не скачалась".into()),
            };
            crate::ui::post(move |u| {
                u.st(|s| s.set_theme_applying("".into()));
                match res {
                    Ok(l) => u.apply_loaded(l),
                    Err(e) => u.toast(&e),
                }
            });
        });
    }

    /// Применить и запомнить готовую тему (из файла, каталога или редактора).
    pub fn apply_loaded(&mut self, loaded: crate::tgtheme::Loaded) {
        let mut wall_path = String::new();
        let mut tiled = false;
        if let Some((bytes, ext, t)) = loaded.wall {
            for old in ["jpg", "jpeg", "png", "gif", "webp"] {
                let _ = std::fs::remove_file(crate::config::dir().join(format!("tg_wall.{old}")));
            }
            let p = crate::config::dir().join(format!("tg_wall.{ext}"));
            if std::fs::write(&p, bytes).is_ok() {
                wall_path = p.to_string_lossy().to_string();
                tiled = t;
            }
        }
        let fx = loaded.fx;
        let theme = crate::config::TgTheme {
            name: loaded.name,
            dark: loaded.dark,
            colors: loaded.colors,
            wall: wall_path,
            wall_tiled: tiled,
            msg_alpha: fx.msg_alpha,
            glass: fx.glass,
            glass_blur: fx.glass_blur,
            wall_blur: fx.wall_blur,
            wall_bright: fx.wall_bright,
        };
        {
            let mut c = self.cfg.lock().unwrap();
            c.tg_theme = Some(theme.clone());
            c.save();
        }
        self.apply_tg(&theme);
        self.epic_loaded();
        self.toast("Тема применена");
    }

    fn ct_collect(&self) -> crate::tgtheme::FpTheme {
        let mut t = self.cfg.lock().unwrap().custom_theme.clone();
        if let Some((gl, glb)) = self.st(|s| (s.get_ct_glass(), s.get_ct_glass_blur())) {
            t.enable_glassmorphism = gl;
            t.glassmorphism_blur = glb.round();
        }
        if let Some((a, b, p, x, l, o, bl, br)) = self.st(|s| {
            (
                s.get_ct_bg1().to_string(),
                s.get_ct_bg2().to_string(),
                s.get_ct_panel().to_string(),
                s.get_ct_text().to_string(),
                s.get_ct_accent().to_string(),
                s.get_ct_opacity(),
                s.get_ct_blur(),
                s.get_ct_bright(),
            )
        }) {
            t.bg_color1 = a;
            t.bg_color2 = b;
            t.container_bg_color = p;
            t.text_color = x;
            t.link_color = l;
            t.container_bg_opacity = o;
            t.bg_blur = bl.round();
            t.bg_brightness = br.round();
        }
        if t.font.is_empty() {
            t.font = "Onest".into();
        }
        t
    }

    pub fn ct_apply(&mut self) {
        let t = self.ct_collect();
        for (n, v) in [("Фон 1", &t.bg_color1), ("Фон 2", &t.bg_color2), ("Панели", &t.container_bg_color), ("Текст", &t.text_color), ("Акцент", &t.link_color)] {
            if crate::templates::parse_hex(v).is_none() {
                self.toast(&format!("{n}: цвет в виде #RRGGBB"));
                return;
            }
        }
        {
            let mut c = self.cfg.lock().unwrap();
            c.custom_theme = t.clone();
            c.save();
        }
        // картинку (и тем более гифку) готовим не в потоке интерфейса
        self.toast("Применяю тему…");
        std::thread::spawn(move || {
            let loaded = crate::tgtheme::build_fp(&t, "Своя тема", true);
            crate::ui::post(move |u| u.apply_loaded(loaded));
        });
    }

    fn ct_load_image(&mut self, src: &str) {
        let bytes = if src.is_empty() || src.starts_with("http") || src.starts_with("data:") { None } else { std::fs::read(src).ok() };
        let img = bytes.as_ref().and_then(|b| crate::imgcache::decode(b, 640));
        // превью стекла: размытая копия первого кадра
        let glass = bytes.as_ref().and_then(|b| image::load_from_memory(b).ok()).map(|i| crate::feat3::glass_of(&i.to_rgba8(), 8.0));
        let anim = bytes.as_ref().and_then(|b| crate::anim::sheet(b, 480, 4_000_000, &|i| i));
        self.st(|s| {
            s.set_ct_has_img(img.is_some());
            s.set_ct_anim(Default::default());
            if let Some((b, _, _)) = img {
                s.set_ct_img(slint::Image::from_rgba8(b));
            }
            if let Some(a) = anim {
                s.set_ct_img(slint::Image::from_rgba8(a.buf));
                s.set_ct_anim(a.info.to_ui());
            }
            if let Some(g) = glass {
                s.set_ct_glass_img(slint::Image::from_rgba8(crate::anim::to_buf(&g)));
            }
        });
    }

    pub fn ct_pick_image(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title("Картинка фона").add_filter("Картинки", &["png", "jpg", "jpeg", "webp", "bmp", "gif"]).pick_file() else { return };
        let Ok(bytes) = std::fs::read(&path) else { return };
        if image::load_from_memory(&bytes).is_err() {
            self.toast("Это не похоже на картинку");
            return;
        }
        let dst = crate::config::dir().join("custom_bg.img");
        if std::fs::write(&dst, &bytes).is_err() {
            return;
        }
        let p = dst.to_string_lossy().to_string();
        {
            let mut c = self.cfg.lock().unwrap();
            c.custom_theme.bg_image = p.clone();
            c.save();
        }
        self.ct_load_image(&p);
    }

    pub fn ct_clear_image(&mut self) {
        {
            let mut c = self.cfg.lock().unwrap();
            c.custom_theme.bg_image.clear();
            c.save();
        }
        self.st(|s| s.set_ct_has_img(false));
    }

    /// Сохранить свою тему в .fptheme (картинка встраивается в файл).
    pub fn ct_export(&mut self) {
        let mut t = self.ct_collect();
        if !t.bg_image.is_empty() && !t.bg_image.starts_with("http") && !t.bg_image.starts_with("data:") {
            if let Ok(b) = std::fs::read(&t.bg_image) {
                use base64::Engine;
                let mime = match image::guess_format(&b) {
                    Ok(image::ImageFormat::Png) => "image/png",
                    Ok(image::ImageFormat::Gif) => "image/gif",
                    Ok(image::ImageFormat::WebP) => "image/webp",
                    _ => "image/jpeg",
                };
                t.bg_image = format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(b));
            }
        }
        let Some(path) = rfd::FileDialog::new().set_title("Сохранить тему").set_file_name("my-theme.fptheme").add_filter("Тема FunPay Tools", &["fptheme"]).save_file() else { return };
        match serde_json::to_string_pretty(&t).ok().map(|s| std::fs::write(&path, s)) {
            Some(Ok(())) => self.toast("Тема сохранена в файл"),
            _ => self.toast("Не удалось сохранить файл"),
        }
    }
}

/// "Hollow Knight, Ключи" → ("Hollow Knight", "Ключи")
pub fn split_category(cat: &str) -> (String, String) {
    match cat.split_once(',') {
        Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
        None => (cat.trim().to_string(), String::new()),
    }
}

/// Монограмма игры для обложки: Hollow Knight → HK, Dota 2 → D2, Spotify → SP.
pub fn monogram(game: &str) -> String {
    let words: Vec<&str> = game.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let up = |s: &str, n: usize| s.chars().take(n).collect::<String>().to_uppercase();
    match words.len() {
        0 => "?".into(),
        1 => up(words[0], 2),
        _ => format!("{}{}", up(words[0], 1), up(words[1], 1)),
    }
}

/// Цена коротко: 850.001426 ₽ → 850 ₽, 150.1222 ₽ → 150.12 ₽, 1190 → 1 190.
pub fn fmt_price(raw: &str) -> String {
    let raw = raw.trim();
    let num: String = raw.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect::<String>().replace(',', ".");
    let cur: String = raw.chars().filter(|c| !(c.is_ascii_digit() || *c == '.' || *c == ',' || c.is_whitespace())).collect();
    let Ok(v) = num.parse::<f64>() else { return raw.to_string() };
    let cents = (v * 100.0).round() as i64;
    let (whole, frac) = (cents / 100, cents % 100);
    let digits = whole.to_string();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push('\u{2009}');
        }
        grouped.push(ch);
    }
    let mut out = grouped;
    if frac != 0 {
        out.push_str(&format!(".{frac:02}"));
        if out.ends_with('0') {
            out.pop();
        }
    }
    if cur.is_empty() { out } else { format!("{out} {cur}") }
}

fn field_lang(x: &crate::lots::Field) -> &str {
    if !x.locale.is_empty() {
        return x.locale.as_str();
    }
    if x.name.ends_with("[en]") {
        "en"
    } else if x.name.ends_with("[ru]") {
        "ru"
    } else {
        ""
    }
}

pub fn hex_color(s: &str) -> Color {
    hex(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_prices() {
        assert_eq!(fmt_price("850.001426 ₽"), "850 ₽");
        assert_eq!(fmt_price("150.1222 ₽"), "150.12 ₽");
        assert_eq!(fmt_price("219.226069 ₽"), "219.23 ₽");
        assert_eq!(fmt_price("61.955193 ₽"), "61.96 ₽");
        assert_eq!(fmt_price("89.6 ₽"), "89.6 ₽");
        assert_eq!(fmt_price("1 190 ₽"), "1\u{2009}190 ₽");
        assert_eq!(fmt_price("12.5 $"), "12.5 $");
        assert_eq!(monogram("Hollow Knight"), "HK");
        assert_eq!(monogram("Dota 2"), "D2");
        assert_eq!(monogram("Spotify"), "SP");
        assert_eq!(split_category("Dota 2, Буст").1, "Буст");
    }

    #[test]
    fn catalog_both_shapes() {
        let a = parse_catalog(r#"[{"id":"x","name":"X","file":"themes/x.fptheme","preview":"previews/x.jpg"}]"#);
        assert_eq!(a[0].file, "https://raw.githubusercontent.com/XaviersDev/fpt-themes/main/themes/x.fptheme");
        let b = parse_catalog(r#"{"themes":[{"name":"Y","file":"https://e.com/y.fptheme"}]}"#);
        assert_eq!(b[0].id, "https://e.com/y.fptheme");
    }
}
