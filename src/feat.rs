//! Связка новых возможностей с интерфейсом: шаблоны, команды, ИИ, цветные ники, уведомления, кэш, обновления.

use crate::parse::{self, ChatItem};
use crate::templates::{self, AutoCmd, Template};
use crate::ui::{post, Ui, AV};
use crate::{AutoRow, LinkSpan, NickStyle, Theme, TplRow};
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

fn hex_color(s: &str) -> Option<Color> {
    templates::parse_hex(s).map(|(r, g, b)| Color::from_rgb_u8(r, g, b))
}

/// Текст поверх сплошной заливки: тёмный на светлых цветах, белый на тёмных.
fn fg_for(c: Color) -> Color {
    let l = 0.2126 * c.red() as f32 + 0.7152 * c.green() as f32 + 0.0722 * c.blue() as f32;
    if l > 150.0 {
        Color::from_rgb_u8(0x1A, 0x1F, 0x26)
    } else {
        Color::from_rgb_u8(0xFF, 0xFF, 0xFF)
    }
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Ui {
    pub fn dark(&self) -> bool {
        self.app().map(|a| a.global::<Theme>().get_dark()).unwrap_or(false)
    }

    pub fn nick(&self, name: &str) -> NickStyle {
        crate::epic::style(name, self.dark())
    }

    // ------------------------------------------------------------ шаблоны

    /// Что подставлять в переменные для открытого чата.
    pub fn tpl_ctx(&self) -> templates::Ctx {
        let mut c = templates::Ctx::default();
        let Some(cur) = self.cur.as_ref() else { return c };
        c.chat_id = cur.id.clone();
        if cur.kind == 0 {
            c.username = cur.title.clone();
        }
        c.lot_name = self.st(|s| s.get_lot_name().to_string()).unwrap_or_default();
        let my = &self.me.id;
        if let Some(m) = cur.msgs.iter().rev().find(|m| m.author_id != *my && m.author_id != "0" && !m.text.trim().is_empty()) {
            c.message_text = squash(&parse::split_reply(&m.text).1);
        }
        // последний заказ из оповещений FunPay в этом чате
        for m in cur.msgs.iter().rev() {
            if m.author_id == "0" {
                if let Some(id) = parse::find_order_ids(&m.text).into_iter().next() {
                    c.order_id = id;
                    break;
                }
            }
        }
        if let Some(o) = self.order.as_ref() {
            if !c.order_id.is_empty() && o.id == c.order_id {
                c.order_title = o.short_desc.clone();
                c.game = o.game.clone();
                c.order_params = o.params.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ");
            }
        }
        if c.order_title.is_empty() {
            c.order_title = c.lot_name.clone();
        }
        c
    }

    pub fn rebuild_templates(&mut self) {
        let (list, look, send_now) = {
            let c = self.cfg.lock().unwrap();
            (c.templates.clone(), c.tpl_look.clone(), c.tpl_send_now)
        };
        let ctx = self.tpl_ctx();
        let rows: Vec<TplRow> = list
            .iter()
            .map(|t| {
                let color = hex_color(&t.color);
                let img = if t.image.is_empty() { None } else { std::fs::read(&t.image).ok().and_then(|b| crate::imgcache::decode(&b, 256)) };
                TplRow {
                    id: t.id.clone().into(),
                    name: t.name.clone().into(),
                    label: if look.caps { t.name.to_uppercase().into() } else { t.name.clone().into() },
                    text: squash(&t.text).into(),
                    preview: templates::fill(&t.text, &ctx).into(),
                    color: color.unwrap_or_default(),
                    fg: color.map(fg_for).unwrap_or(Color::from_rgb_u8(255, 255, 255)),
                    has_color: color.is_some(),
                    hex: t.color.clone().into(),
                    has_image: img.is_some(),
                    image: img.map(|(b, _, _)| slint::Image::from_rgba8(b)).unwrap_or_default(),
                    image_first: t.image_first,
                    bind: t.bind,
                }
            })
            .collect();
        self.st(|s| {
            s.set_templates(model(rows));
            s.set_tpl_pos(look.pos);
            s.set_tpl_shape(look.shape);
            s.set_tpl_size(look.size);
            s.set_tpl_fill(look.fill);
            s.set_tpl_align(look.align);
            s.set_tpl_full(look.full_width);
            s.set_tpl_compact(look.compact);
            s.set_tpl_caps(look.caps);
            s.set_tpl_preview(look.preview);
            s.set_tpl_send_now(send_now);
        });
    }

    /// Шаблон по клику. Возвращает текст для поля или "" если уже отправлено.
    pub fn tpl_use(&mut self, id: &str) -> String {
        let (t, send_now) = {
            let c = self.cfg.lock().unwrap();
            (c.templates.iter().find(|t| t.id == id).cloned(), c.tpl_send_now)
        };
        let Some(t) = t else { return String::new() };
        if self.cur.is_none() {
            return String::new();
        }
        let text = templates::fill(&t.text, &self.tpl_ctx());
        let image = if t.image.is_empty() || !std::path::Path::new(&t.image).exists() { None } else { Some(std::path::PathBuf::from(&t.image)) };
        let can_image = self.cur.as_ref().map(|c| c.kind != 3).unwrap_or(false);
        if let (Some(img), true) = (image.clone(), can_image) {
            if t.image_first {
                self.send_images(vec![img]);
                if !text.trim().is_empty() {
                    self.send_text(text);
                }
            } else {
                if !text.trim().is_empty() {
                    self.send_text(text);
                }
                self.send_images(vec![img]);
            }
            if let Some(app) = self.app() {
                app.invoke_scroll_bottom();
            }
            return String::new();
        }
        if send_now {
            self.send_text(text);
            if let Some(app) = self.app() {
                app.invoke_scroll_bottom();
            }
            return String::new();
        }
        text
    }

    pub fn tpl_bind(&mut self, n: i32) -> String {
        let id = self.cfg.lock().unwrap().templates.iter().find(|t| t.bind == n).map(|t| t.id.clone());
        match id {
            Some(id) => self.tpl_use(&id),
            None => {
                self.toast(&format!("На Alt+{n} пока нет шаблона"));
                String::new()
            }
        }
    }

    pub fn cmd_query(&mut self, text: &str) {
        let q = text.trim_start();
        let hits: Vec<TplRow> = if let Some(rest) = q.strip_prefix('/') {
            if rest.contains(char::is_whitespace) && rest.trim().len() != rest.len() {
                vec![]
            } else {
                let rest = rest.trim().to_lowercase();
                let all = self.st(|s| s.get_templates()).map(|m| m.iter().collect::<Vec<_>>()).unwrap_or_default();
                let mut v: Vec<TplRow> = all.iter().filter(|t| t.name.to_lowercase().starts_with(&rest)).cloned().collect();
                v.extend(all.iter().filter(|t| !t.name.to_lowercase().starts_with(&rest) && t.name.to_lowercase().contains(&rest)).cloned());
                v.truncate(5);
                v
            }
        } else {
            vec![]
        };
        self.st(|s| {
            if hits.is_empty() && s.get_cmd_hits().row_count() == 0 {
                return;
            }
            s.set_cmd_hits(model(hits));
        });
    }

    pub fn tpl_new(&mut self) {
        let n = self.cfg.lock().unwrap().templates.len();
        let used: Vec<i32> = self.cfg.lock().unwrap().templates.iter().map(|t| t.bind).collect();
        let bind = (1..=9).find(|b| !used.contains(b)).unwrap_or(0);
        let tour = !self.tour_id().is_empty();
        self.st(|s| {
            s.set_tpl_e_id("".into());
            s.set_tpl_e_name(if n == 0 || tour { "Привет".into() } else { "".into() });
            s.set_tpl_e_text(if n == 0 || tour { "$welcome, $username! Чем могу помочь?".into() } else { "".into() });
            s.set_tpl_e_hex("".into());
            s.set_tpl_e_has_image(false);
            s.set_tpl_e_image_first(true);
            s.set_tpl_e_bind(bind);
            s.set_tpl_editing(true);
        });
        self.ext.tpl_image = String::new();
    }

    pub fn tpl_edit(&mut self, id: &str) {
        let Some(t) = self.cfg.lock().unwrap().templates.iter().find(|t| t.id == id).cloned() else { return };
        let img = if t.image.is_empty() { None } else { std::fs::read(&t.image).ok().and_then(|b| crate::imgcache::decode(&b, 256)) };
        self.ext.tpl_image = t.image.clone();
        self.st(|s| {
            s.set_tpl_e_id(t.id.clone().into());
            s.set_tpl_e_name(t.name.clone().into());
            s.set_tpl_e_text(t.text.clone().into());
            s.set_tpl_e_hex(t.color.clone().into());
            s.set_tpl_e_has_image(img.is_some());
            if let Some((b, _, _)) = img {
                s.set_tpl_e_image(slint::Image::from_rgba8(b));
            }
            s.set_tpl_e_image_first(t.image_first);
            s.set_tpl_e_bind(t.bind);
            s.set_tpl_editing(true);
        });
    }

    pub fn tpl_save(&mut self) {
        let Some((id, name, text, hex, first, bind)) = self.st(|s| {
            (
                s.get_tpl_e_id().to_string(),
                s.get_tpl_e_name().trim().to_string(),
                s.get_tpl_e_text().to_string(),
                s.get_tpl_e_hex().trim().to_string(),
                s.get_tpl_e_image_first(),
                s.get_tpl_e_bind(),
            )
        }) else {
            return;
        };
        if name.is_empty() {
            self.toast("Дайте шаблону название");
            return;
        }
        if text.trim().is_empty() && self.ext.tpl_image.is_empty() {
            self.toast("Добавьте текст или картинку");
            return;
        }
        let color = if hex.is_empty() {
            String::new()
        } else if let Some((r, g, b)) = templates::parse_hex(&hex) {
            format!("#{r:02X}{g:02X}{b:02X}")
        } else {
            self.toast("Цвет в виде #RRGGBB, например #3390EC");
            return;
        };
        {
            let mut c = self.cfg.lock().unwrap();
            let id = if id.is_empty() { templates::new_id() } else { id };
            // бинд уникальный: у другого шаблона снимаем
            if bind > 0 {
                for t in c.templates.iter_mut() {
                    if t.bind == bind && t.id != id {
                        t.bind = 0;
                    }
                }
            }
            let t = Template { id: id.clone(), name, text, color, image: self.ext.tpl_image.clone(), image_first: first, bind };
            if let Some(x) = c.templates.iter_mut().find(|x| x.id == id) {
                *x = t;
            } else {
                c.templates.push(t);
            }
            c.save();
        }
        self.st(|s| s.set_tpl_editing(false));
        self.rebuild_templates();
        self.toast("Шаблон сохранён");
        self.tour_event("saved");
    }

    pub fn tpl_delete(&mut self, id: &str) {
        {
            let mut c = self.cfg.lock().unwrap();
            if let Some(t) = c.templates.iter().find(|t| t.id == id) {
                if !t.image.is_empty() && t.image.starts_with(&*crate::config::dir().to_string_lossy()) {
                    let _ = std::fs::remove_file(&t.image);
                }
            }
            c.templates.retain(|t| t.id != id);
            c.save();
        }
        self.st(|s| s.set_tpl_editing(false));
        self.rebuild_templates();
    }

    pub fn tpl_move(&mut self, id: &str, d: i32) {
        {
            let mut c = self.cfg.lock().unwrap();
            let Some(i) = c.templates.iter().position(|t| t.id == id) else { return };
            let j = i as i32 + d;
            if j < 0 || j >= c.templates.len() as i32 {
                return;
            }
            c.templates.swap(i, j as usize);
            c.save();
        }
        self.rebuild_templates();
    }

    pub fn tpl_pick_image(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Картинка для шаблона")
            .add_filter("Картинки", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
            .pick_file()
        else {
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else { return };
        let Some((buf, _, _)) = crate::imgcache::decode(&bytes, 256) else {
            self.toast("Это не похоже на картинку");
            return;
        };
        // копия в папке приложения, чтобы шаблон не сломался, если исходник удалят
        let d = crate::config::dir().join("templates");
        let _ = std::fs::create_dir_all(&d);
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "png".into());
        let dst = d.join(format!("{}.{ext}", templates::new_id()));
        if std::fs::write(&dst, &bytes).is_err() {
            self.toast("Не удалось сохранить картинку");
            return;
        }
        self.ext.tpl_image = dst.to_string_lossy().to_string();
        self.st(|s| {
            s.set_tpl_e_image(slint::Image::from_rgba8(buf));
            s.set_tpl_e_has_image(true);
        });
    }

    pub fn tpl_clear_image(&mut self) {
        self.ext.tpl_image.clear();
        self.st(|s| s.set_tpl_e_has_image(false));
    }

    pub fn tpl_set(&mut self, key: &str, v: i32) {
        {
            let mut c = self.cfg.lock().unwrap();
            let l = &mut c.tpl_look;
            match key {
                "pos" => l.pos = v,
                "shape" => l.shape = v,
                "size" => l.size = v,
                "fill" => l.fill = v,
                "align" => l.align = v,
                "full" => l.full_width = v != 0,
                "compact" => l.compact = v != 0,
                "caps" => l.caps = v != 0,
                "preview" => l.preview = v != 0,
                "send-now" => c.tpl_send_now = v != 0,
                _ => {}
            }
            c.save();
        }
        self.rebuild_templates();
    }

    // ------------------------------------------------------------ автоответ

    pub fn rebuild_autos(&mut self) {
        let (list, on) = {
            let c = self.cfg.lock().unwrap();
            (c.auto_cmds.clone(), c.auto_on)
        };
        let rows: Vec<AutoRow> = list
            .iter()
            .map(|a| AutoRow {
                id: a.id.clone().into(),
                trigger: a.trigger.clone().into(),
                response: squash(&a.response).into(),
                exact: a.exact,
                cs: a.case_sensitive,
                enabled: a.enabled,
            })
            .collect();
        self.st(|s| {
            s.set_autos(model(rows));
            s.set_auto_on(on);
        });
    }

    pub fn auto_new(&mut self) {
        self.st(|s| {
            s.set_au_e_id("".into());
            s.set_au_e_trigger("".into());
            s.set_au_e_response("".into());
            s.set_au_e_exact(true);
            s.set_au_e_cs(false);
            s.set_auto_editing(true);
        });
    }

    pub fn auto_edit(&mut self, id: &str) {
        let Some(a) = self.cfg.lock().unwrap().auto_cmds.iter().find(|a| a.id == id).cloned() else { return };
        self.st(|s| {
            s.set_au_e_id(a.id.clone().into());
            s.set_au_e_trigger(a.trigger.clone().into());
            s.set_au_e_response(a.response.clone().into());
            s.set_au_e_exact(a.exact);
            s.set_au_e_cs(a.case_sensitive);
            s.set_auto_editing(true);
        });
    }

    pub fn auto_save(&mut self) {
        let Some((id, trigger, response, exact, cs)) = self.st(|s| {
            (s.get_au_e_id().to_string(), s.get_au_e_trigger().trim().to_string(), s.get_au_e_response().to_string(), s.get_au_e_exact(), s.get_au_e_cs())
        }) else {
            return;
        };
        if trigger.is_empty() || response.trim().is_empty() {
            self.toast("Нужны и команда, и ответ");
            return;
        }
        {
            let mut c = self.cfg.lock().unwrap();
            let id = if id.is_empty() { templates::new_id() } else { id };
            let enabled = c.auto_cmds.iter().find(|a| a.id == id).map(|a| a.enabled).unwrap_or(true);
            let a = AutoCmd { id: id.clone(), trigger, response, exact, case_sensitive: cs, enabled };
            if let Some(x) = c.auto_cmds.iter_mut().find(|x| x.id == id) {
                *x = a;
            } else {
                c.auto_cmds.push(a);
            }
            c.save();
        }
        self.st(|s| s.set_auto_editing(false));
        self.rebuild_autos();
        self.toast("Команда сохранена");
    }

    pub fn auto_delete(&mut self, id: &str) {
        {
            let mut c = self.cfg.lock().unwrap();
            c.auto_cmds.retain(|a| a.id != id);
            c.save();
        }
        self.st(|s| s.set_auto_editing(false));
        self.rebuild_autos();
    }

    pub fn auto_toggle(&mut self, id: &str, v: bool) {
        {
            let mut c = self.cfg.lock().unwrap();
            if let Some(a) = c.auto_cmds.iter_mut().find(|a| a.id == id) {
                a.enabled = v;
            }
            c.save();
        }
        self.rebuild_autos();
    }

    // ------------------------------------------------------------ ИИ

    pub fn ai_rewrite(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        let Some(cur) = self.cur.as_ref() else { return };
        let my = self.me.id.clone();
        let peer = cur.title.clone();
        let history: Vec<String> = cur
            .msgs
            .iter()
            .rev()
            .filter(|m| !m.text.trim().is_empty())
            .take(12)
            .map(|m| {
                let who = if m.author_id == my { "Продавец (я)".to_string() } else if m.author_id == "0" { "FunPay".into() } else { m.author.clone() };
                format!("{who}: {}", squash(&parse::split_reply(&m.text).1).chars().take(300).collect::<String>())
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let tctx = self.tpl_ctx();
        let ctx = crate::ai::Ctx {
            history: history.join("\n"),
            username: if cur.kind == 0 { peer } else { String::new() },
            chat_id: cur.id.clone(),
            lot_name: tctx.lot_name,
            last_message: tctx.message_text,
        };
        let gen = cur.gen;
        self.st(|s| {
            s.set_ai_busy(true);
            s.set_ai_orig(text.clone().into());
        });
        std::thread::spawn(move || {
            let res = crate::ai::rewrite(&text, &ctx);
            post(move |u| {
                u.st(|s| s.set_ai_busy(false));
                let same = u.cur.as_ref().map(|c| c.gen == gen).unwrap_or(false);
                match res {
                    Ok(t) if same => {
                        u.st(|s| {
                            s.set_ai_result(t.into());
                            s.set_ai_n(s.get_ai_n() + 1);
                        });
                    }
                    Ok(_) => u.st(|s| s.set_ai_orig("".into())).unwrap_or(()),
                    Err(e) => {
                        u.st(|s| s.set_ai_orig("".into()));
                        u.toast(&e);
                    }
                }
            });
        });
    }

    /// Аватарка из карточки (0) или профиля (1) в просмотрщике, там же сохранить и копировать.
    pub fn open_avatar(&mut self, which: i32) {
        let url = if which == 0 { self.card_url.clone() } else { self.profile.as_ref().map(|p| p.avatar.clone()).unwrap_or_default() };
        if url.is_empty() || parse::is_default_avatar(&url) {
            return;
        }
        self.open_image(url);
    }

    // ------------------------------------------------------------ ссылки в сообщениях

    pub fn msg_link(&mut self, key: &str, off: i32) -> bool {
        let row = (0..self.msgs_model.row_count()).filter_map(|i| self.msgs_model.row_data(i)).find(|r| r.key == key);
        let Some(row) = row else { return false };
        let url = row.links.iter().find(|l| off > l.a && off < l.b).map(|l| l.url.to_string());
        match url {
            Some(u) => {
                if self.demo {
                    eprintln!("link: {u}");
                }
                self.open_url(u);
                true
            }
            None => false,
        }
    }

    /// Ссылки и разметка для строки сообщения.
    pub fn msg_links(text: &str, spacer: &str) -> (bool, ModelRc<LinkSpan>, slint::StyledText) {
        let links = parse::find_links(text);
        if links.is_empty() {
            return (false, ModelRc::default(), slint::StyledText::default());
        }
        let md = parse::links_markdown(text, &links, spacer);
        let rich = slint::StyledText::from_markdown(&md).unwrap_or_else(|_| slint::StyledText::from_plain_text(&format!("{text}{spacer}")));
        let spans = links.into_iter().map(|(a, b, url)| LinkSpan { a: a as i32, b: b as i32, url: url.into() }).collect();
        (true, model(spans), rich)
    }

    // ------------------------------------------------------------ цветные ники

    pub fn epic_loaded(&mut self) {
        let dark = self.dark();
        let me = self.me.name.clone();
        let cur = self.cur.as_ref().filter(|c| c.kind == 0).map(|c| c.title.clone()).unwrap_or_default();
        let (ex, names) = crate::epic::examples(dark);
        let pf = self.st(|s| s.get_pf_name().to_string()).unwrap_or_default();
        let card = self.st(|s| s.get_card_name().to_string()).unwrap_or_default();
        self.st(|s| {
            s.set_me_nick(crate::epic::style(&me, dark));
            s.set_cur_nick(crate::epic::style(&cur, dark));
            s.set_pf_nick(crate::epic::style(&pf, dark));
            s.set_card_nick(crate::epic::style(&card, dark));
            s.set_epic_examples(model(ex));
            s.set_epic_names(model(names.into_iter().map(SharedString::from).collect()));
        });
        self.rebuild_chats();
        self.rebuild_msgs();
    }

    // ------------------------------------------------------------ уведомления

    /// Новые входящие от воркера: всплывающие окна и звук по настройкам.
    pub fn incoming(&mut self, fresh: Vec<ChatItem>) {
        let (enabled, sound, pop) = {
            let c = self.cfg.lock().unwrap();
            (c.notifications, c.sound, c.popup.clone())
        };
        if fresh.is_empty() {
            return;
        }
        let open = self.cur.as_ref().map(|c| c.id.clone());
        let show = enabled && (!self.focused || pop.in_focus);
        if show {
            let list: Vec<ChatItem> = fresh.iter().filter(|c| !(self.focused && open.as_deref() == Some(c.id.as_str()))).take(pop.count.clamp(1, 5) as usize).cloned().collect();
            for c in list {
                self.show_note(&c.id, &c.name, &parse::preview_text(&c.preview), &c.avatar, &pop);
            }
        }
        // звук отдельно: играет, даже если всплывающие выключены
        if sound {
            crate::sys::play_sound();
        }
        self.attention();
    }

    pub fn show_note(&mut self, chat: &str, name: &str, text: &str, avatar: &str, pop: &crate::config::PopupCfg) {
        if pop.custom && !self.demo_no_popups() {
            let av = if parse::is_default_avatar(avatar) { None } else { self.img(avatar, AV).1.map(|x| x.0) };
            let (c1, c2) = crate::ui::pair(name);
            let accent = self.app().map(|a| a.global::<Theme>().get_accent()).unwrap_or(Color::from_rgb_u8(0x3A, 0x7F, 0xD5));
            let dark = self.dark();
            if let Some(app) = self.app() {
                crate::popup::show(
                    &app,
                    crate::popup::Note {
                        chat: chat.to_string(),
                        title: name.to_string(),
                        text: text.to_string(),
                        avatar: av,
                        letter: name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into()),
                        c1,
                        c2,
                    },
                    pop,
                    dark,
                    accent,
                );
            }
        } else {
            let id = chat.to_string();
            let title = if pop.show_name { name.to_string() } else { "Новое сообщение".into() };
            let body = if pop.show_text { text.to_string() } else { "Откройте, чтобы прочитать".into() };
            crate::sys::notify_with_icon(&title, &body, pop.show_icon, move || {
                let id = id.clone();
                crate::ui::with_ui(move |u| u.notification_clicked(id));
            });
        }
    }

    fn demo_no_popups(&self) -> bool {
        false
    }

    pub fn pop_set(&mut self, key: &str, v: i32) {
        {
            let mut c = self.cfg.lock().unwrap();
            let p = &mut c.popup;
            match key {
                "custom" => p.custom = v != 0,
                "corner" => p.corner = v,
                "count" => p.count = v.clamp(1, 5),
                "icon" => p.show_icon = v != 0,
                "name" => p.show_name = v != 0,
                "text" => p.show_text = v != 0,
                "focus" => p.in_focus = v != 0,
                _ => {}
            }
            c.save();
        }
        self.sync_pop_settings();
    }

    pub fn sync_pop_settings(&mut self) {
        let p = self.cfg.lock().unwrap().popup.clone();
        self.st(|s| {
            s.set_pop_custom(p.custom);
            s.set_pop_corner(p.corner);
            s.set_pop_count(p.count);
            s.set_pop_icon(p.show_icon);
            s.set_pop_name(p.show_name);
            s.set_pop_text(p.show_text);
            s.set_pop_focus(p.in_focus);
        });
    }

    pub fn pop_test(&mut self) {
        let pop = self.cfg.lock().unwrap().popup.clone();
        let name = self.chats.first().map(|c| c.name.clone()).unwrap_or_else(|| "Покупатель".into());
        let avatar = self.chats.first().map(|c| c.avatar.clone()).unwrap_or_default();
        let chat = self.chats.first().map(|c| c.id.clone()).unwrap_or_default();
        self.show_note(&chat, &name, "Здравствуйте! Так выглядит уведомление о новом сообщении.", &avatar, &pop);
    }

    // ------------------------------------------------------------ кэш чатов

    pub fn load_cache_for(&mut self, uid: &str) {
        let chats = crate::cache::load_chats(uid);
        if !chats.is_empty() && self.chats.is_empty() {
            self.chats = chats;
            self.st(|s| s.set_chats_loading(false));
            self.rebuild_chats();
        }
    }

    pub fn save_chats_cache(&self) {
        if !self.me.id.is_empty() && !self.demo {
            crate::cache::save_chats(&self.me.id, &self.chats);
        }
    }

    pub fn save_msgs_cache(&self) {
        let Some(cur) = self.cur.as_ref() else { return };
        if cur.kind == 3 || self.me.id.is_empty() || self.demo || cur.msgs.is_empty() {
            return;
        }
        let snap = crate::cache::ChatSnap {
            peer_id: cur.peer_id.clone(),
            peer_name: cur.title.clone(),
            peer_avatar: cur.peer_avatar.clone(),
            lot_name: self.st(|s| s.get_lot_name().to_string()).unwrap_or_default(),
            lot_url: self.st(|s| s.get_lot_url().to_string()).unwrap_or_default(),
            msgs: cur.msgs.clone(),
        };
        crate::cache::save_msgs(&self.me.id, &cur.id, &snap);
    }

    /// Переписка из кэша сразу при открытии чата.
    pub fn show_cached_msgs(&mut self) -> bool {
        if self.me.id.is_empty() || self.demo {
            return false;
        }
        let Some(cur) = self.cur.as_mut() else { return false };
        if cur.kind == 3 {
            return false;
        }
        let Some(snap) = crate::cache::load_msgs(&self.me.id, &cur.id) else { return false };
        if snap.msgs.is_empty() {
            return false;
        }
        cur.msgs = snap.msgs;
        if cur.peer_id.is_empty() {
            cur.peer_id = snap.peer_id.clone();
        }
        let kind = cur.kind;
        self.st(|s| {
            s.set_msgs_loading(false);
            if kind == 0 {
                s.set_cur_peer_id(snap.peer_id.clone().into());
                s.set_lot_name(snap.lot_name.clone().into());
                s.set_lot_url(snap.lot_url.clone().into());
            }
        });
        self.rebuild_msgs();
        true
    }
}

/// Проверка «начинается с /» для поля ввода (в Slint нет срезов строк).
pub fn starts_slash(s: &str) -> bool {
    s.trim_start().starts_with('/')
}

// ---------------------------------------------------------------- обновления

impl Ui {
    pub fn upd_check(&mut self, manual: bool) {
        if self.demo || self.st(|s| s.get_upd_busy()).unwrap_or(false) || self.ext.upd_exe.is_some() {
            if manual && self.ext.upd_exe.is_some() {
                self.toast("Обновление уже скачано, осталось перезапустить");
            }
            return;
        }
        self.st(|s| {
            s.set_upd_busy(true);
            if manual {
                s.set_upd_status("Проверяю обновления…".into());
            }
        });
        std::thread::spawn(move || {
            let res = crate::update::check();
            match res {
                Ok(crate::update::Check::New(rel)) => {
                    let ver = rel.version.clone();
                    let v2 = ver.clone();
                    post(move |u| u.st(|s| s.set_upd_status(format!("Скачиваю версию {v2}…").into())).unwrap_or(()));
                    let dl = crate::update::download(&rel, |p| {
                        let v = ver.clone();
                        post(move |u| u.st(|s| s.set_upd_status(format!("Скачиваю версию {v}: {p}%").into())).unwrap_or(()));
                    });
                    let res = dl.and_then(|p| crate::update::install(&p));
                    post(move |u| u.upd_installed(ver, res));
                }
                Ok(crate::update::Check::UpToDate) => post(move |u| {
                    u.st(|s| {
                        s.set_upd_busy(false);
                        s.set_upd_status(format!("У вас последняя версия {}", env!("CARGO_PKG_VERSION")).into());
                    });
                    if manual {
                        u.toast("У вас последняя версия");
                    }
                }),
                Ok(crate::update::Check::NoRepo) => post(move |u| {
                    u.st(|s| {
                        s.set_upd_busy(false);
                        s.set_upd_status(format!("Версия {}. Новых выпусков пока нет", env!("CARGO_PKG_VERSION")).into());
                    });
                    if manual {
                        u.toast("Новых версий пока нет");
                    }
                }),
                Err(e) => post(move |u| {
                    u.st(|s| {
                        s.set_upd_busy(false);
                        if manual {
                            s.set_upd_status(e.clone().into());
                        }
                    });
                    if manual {
                        u.toast(&e);
                    }
                }),
            }
        });
    }

    fn upd_installed(&mut self, ver: String, res: Result<std::path::PathBuf, String>) {
        self.st(|s| s.set_upd_busy(false));
        match res {
            Ok(exe) => {
                crate::log::write(&format!("обновление {ver} установлено"));
                self.ext.upd_exe = Some(exe);
                self.st(|s| {
                    s.set_upd_ready(true);
                    s.set_upd_version(ver.clone().into());
                    s.set_upd_status(format!("Версия {ver} скачана и установлена").into());
                });
                let visible = self.app().map(|a| a.window().is_visible()).unwrap_or(false);
                if !visible {
                    // окно спрятано: перезапускаемся сразу, никто не заметит
                    self.upd_restart(true);
                } else {
                    self.toast(&format!("Обновление {ver} готово. Перезапустить можно в настройках"));
                }
            }
            Err(e) => {
                crate::log::write(&format!("обновление {ver}: {e}"));
                self.st(|s| s.set_upd_status(e.into()));
            }
        }
    }

    pub fn upd_restart(&mut self, background: bool) {
        let Some(exe) = self.ext.upd_exe.clone() else { return };
        crate::update::relaunch(&exe, background);
        crate::popup::close_all();
        crate::tray::remove();
        let _ = slint::quit_event_loop();
    }

    /// Спрятали окно в трей, а обновление ждёт: тихо перезапускаемся в фоне.
    pub fn upd_on_hide(&mut self) {
        if self.ext.upd_exe.is_some() {
            self.upd_restart(true);
        }
    }
}
