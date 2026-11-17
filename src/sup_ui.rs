//! Экран «Поддержка FunPay»: список заявок, переписка, новая заявка (см. support.rs и ui/support.slint).

use crate::support::{Details, Field, Page, Ticket};
use crate::ui::Ui;
use crate::worker::{Cmd, SupOp};
use crate::{SupField, SupFile, SupMsg, SupTicket, KV};
use slint::{Image, ModelRc, SharedString, VecModel};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const PANE: i32 = 6;
const AV: u32 = 96;

#[derive(Default)]
pub struct SupState {
    pub gen: u64,
    list: Vec<Ticket>,
    next: Option<u32>,
    total: usize,
    cur: Option<Details>,
    cats: Vec<(String, String)>,
    cat: String,
    fields: Vec<Field>,
    values: HashMap<String, String>,
    files: Vec<String>,
    file_names: Vec<String>,
    av_keys: HashSet<String>,
    uploading: bool,
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

fn tone(status: &str) -> i32 {
    match status {
        "Открыта" => 0,
        "Ждёт ответа" => 1,
        "Решена" => 2,
        _ => 3,
    }
}


fn files_label(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => "1 файл прикреплён".into(),
        2..=4 => format!("{n} файла прикреплено"),
        _ => format!("{n} файлов прикреплено"),
    }
}

impl Ui {
    fn sup_send(&mut self, op: SupOp) {
        self.send(Cmd::Sup(op));
    }

    fn sup_err(&mut self, e: &str) {
        let e = e.to_string();
        self.st(|s| s.set_sup_error(e.into()));
    }

    pub fn sup_open(&mut self) {
        if self.panes.last() != Some(&PANE) {
            self.panes.retain(|x| *x != PANE);
            self.panes.push(PANE);
        }
        self.st(|s| {
            s.set_pane(PANE);
            s.set_sup_view(0);
            s.set_card_open(false);
            s.set_settings_open(false);
        });
        self.sup_refresh();
    }

    pub fn sup_refresh(&mut self) {
        self.ext.sup.gen += 1;
        let gen = self.ext.sup.gen;
        self.st(|s| {
            s.set_sup_loading(true);
            s.set_sup_error("".into());
        });
        self.sup_send(SupOp::List { gen });
    }

    pub fn sup_listed(&mut self, gen: u64, r: Result<Page, String>, append: bool) {
        if gen != self.ext.sup.gen {
            return;
        }
        self.st(|s| {
            s.set_sup_loading(false);
            s.set_sup_more_busy(false);
        });
        match r {
            Ok(p) => {
                if append {
                    for t in p.items {
                        if !self.ext.sup.list.iter().any(|x| x.id == t.id) {
                            self.ext.sup.list.push(t);
                        }
                    }
                } else {
                    self.ext.sup.list = p.items;
                }
                self.ext.sup.next = p.next;
                self.ext.sup.total = p.total.max(self.ext.sup.list.len());
                self.sup_render_list();
            }
            Err(e) => self.sup_err(&e),
        }
    }

    pub fn sup_more(&mut self) {
        let Some(page) = self.ext.sup.next else { return };
        if self.st(|s| s.get_sup_more_busy()).unwrap_or(true) {
            return;
        }
        let gen = self.ext.sup.gen;
        self.st(|s| s.set_sup_more_busy(true));
        self.sup_send(SupOp::More { gen, page });
    }

    fn sup_render_list(&mut self) {
        let f = self.st(|s| s.get_sup_filter()).unwrap_or(0);
        let all = self.ext.sup.total as i32;
        let open = self.ext.sup.list.iter().filter(|t| t.active).count() as i32;
        let rows: Vec<SupTicket> = self
            .ext
            .sup
            .list
            .iter()
            .filter(|t| f == 0 || (f == 1) == t.active)
            .map(|t| SupTicket {
                id: t.id.clone().into(),
                title: t.title.clone().into(),
                status: t.status.clone().into(),
                date: t.date.clone().into(),
                tone: tone(&t.status),
            })
            .collect();
        let more = self.ext.sup.next.is_some() && f != 1;
        self.st(|s| {
            s.set_sup_n_all(all);
            s.set_sup_n_open(open);
            s.set_sup_n_closed((all - open).max(0));
            s.set_sup_more(more);
            s.set_sup_tickets(model(rows));
        });
    }

    pub fn sup_filter_set(&mut self, f: i32) {
        self.st(|s| s.set_sup_filter(f));
        self.sup_render_list();
    }

    pub fn sup_ticket(&mut self, id: String) {
        self.ext.sup.gen += 1;
        let gen = self.ext.sup.gen;
        let same = self.ext.sup.cur.as_ref().map(|d| d.id == id).unwrap_or(false);
        if !same {
            self.ext.sup.cur = None;
            self.ext.sup.files.clear();
            self.ext.sup.file_names.clear();
        }
        let t = self.ext.sup.list.iter().find(|t| t.id == id).cloned().unwrap_or_default();
        self.st(|s| {
            s.set_sup_view(1);
            s.set_sup_id(id.clone().into());
            if !same {
                s.set_sup_title(t.title.clone().into());
                s.set_sup_status(t.status.clone().into());
                s.set_sup_tone(tone(&t.status));
                s.set_sup_msgs(model(vec![]));
                s.set_sup_info(model(vec![]));
                s.set_sup_files("".into());
            }
            s.set_sup_can_reply(same && s.get_sup_can_reply());
            s.set_sup_loading(true);
            s.set_sup_error("".into());
        });
        self.sup_send(SupOp::Open { id, gen });
    }

    pub fn sup_opened(&mut self, gen: u64, r: Result<Details, String>) {
        if gen != self.ext.sup.gen {
            return;
        }
        self.st(|s| s.set_sup_loading(false));
        match r {
            Ok(d) => {
                // статус в списке тоже обновим
                if let Some(t) = self.ext.sup.list.iter_mut().find(|t| t.id == d.id) {
                    t.status = d.status.clone();
                }
                self.ext.sup.cur = Some(d);
                self.sup_render_ticket();
            }
            Err(e) => self.sup_err(&e),
        }
    }

    fn sup_render_ticket(&mut self) {
        let Some(d) = self.ext.sup.cur.clone() else { return };
        let mut info: Vec<KV> = Vec::new();
        if !d.created.is_empty() {
            info.push(KV { k: "Создана".into(), v: d.created.clone().into() });
        }
        if !d.responsible.is_empty() {
            info.push(KV { k: "Ответственный".into(), v: d.responsible.clone().into() });
        }
        for (k, v) in &d.fields {
            info.push(KV { k: k.clone().into(), v: v.clone().into() });
        }
        let mut msgs = Vec::new();
        let mut prev = String::new();
        for c in &d.comments {
            let (av, has) = if c.mine || c.avatar.is_empty() {
                (Image::default(), false)
            } else {
                let (key, img) = self.img(&c.avatar, AV);
                self.ext.sup.av_keys.insert(key);
                match img {
                    Some((i, _, _)) => (i, true),
                    None => (Image::default(), false),
                }
            };
            let who = format!("{}{}", c.mine, c.author);
            let first = who != prev;
            prev = who;
            let files: Vec<SupFile> = c.files.iter().map(|f| SupFile { name: f.name.clone().into(), url: f.url.clone().into(), image: f.image }).collect();
            msgs.push(SupMsg {
                author: c.author.clone().into(),
                text: c.text.clone().into(),
                time: c.time.clone().into(),
                mine: c.mine,
                letter: c.author.chars().next().map(|x| x.to_uppercase().to_string()).unwrap_or_else(|| "П".into()).into(),
                avatar: av,
                has_avatar: has,
                files: model(files),
                first,
            });
        }
        self.st(|s| {
            if !d.title.is_empty() {
                s.set_sup_title(d.title.clone().into());
            }
            s.set_sup_status(d.status.clone().into());
            s.set_sup_tone(tone(&d.status));
            s.set_sup_info(model(info));
            s.set_sup_msgs(model(msgs));
            s.set_sup_can_reply(d.can_reply);
        });
    }

    pub fn sup_image_ready(&mut self, key: &str) {
        if self.ext.sup.av_keys.contains(key) && self.st(|s| s.get_pane() == PANE && s.get_sup_view() == 1).unwrap_or(false) {
            self.sup_render_ticket();
        }
    }

    pub fn sup_back(&mut self) {
        let view = self.st(|s| s.get_sup_view()).unwrap_or(0);
        if view == 0 {
            self.pane_back();
            return;
        }
        self.ext.sup.files.clear();
        self.ext.sup.file_names.clear();
        self.st(|s| {
            s.set_sup_view(0);
            s.set_sup_error("".into());
            s.set_sup_files("".into());
        });
        self.sup_render_list();
    }

    pub fn sup_reply(&mut self, msg: String) {
        let Some(id) = self.ext.sup.cur.as_ref().map(|d| d.id.clone()) else { return };
        if msg.trim().is_empty() || self.ext.sup.uploading {
            return;
        }
        let files = self.ext.sup.files.clone();
        self.st(|s| {
            s.set_sup_busy(true);
            s.set_sup_error("".into());
        });
        self.sup_send(SupOp::Reply { id, msg, files });
    }

    pub fn sup_replied(&mut self, id: String, r: Result<(), String>) {
        self.st(|s| s.set_sup_busy(false));
        match r {
            Ok(()) => {
                self.ext.sup.files.clear();
                self.ext.sup.file_names.clear();
                self.st(|s| {
                    s.set_sup_files("".into());
                    s.set_sup_sent_n(s.get_sup_sent_n() + 1);
                });
                self.sup_ticket(id);
            }
            Err(e) => self.sup_err(&e),
        }
    }

    pub fn sup_attach(&mut self) {
        if self.ext.sup.uploading {
            return;
        }
        let Some(path) = rfd::FileDialog::new().set_title("Файл для поддержки").pick_file() else { return };
        let ticket = self.ext.sup.cur.as_ref().map(|d| d.id.clone()).unwrap_or_default();
        let ticket = if self.st(|s| s.get_sup_view()).unwrap_or(0) == 2 { String::new() } else { ticket };
        self.ext.sup.uploading = true;
        self.st(|s| {
            s.set_sup_busy(true);
            s.set_sup_files("Загружаю файл…".into());
            s.set_sup_error("".into());
        });
        self.sup_send(SupOp::Upload { ticket, path });
    }

    pub fn sup_uploaded(&mut self, name: String, r: Result<String, String>) {
        self.ext.sup.uploading = false;
        self.st(|s| s.set_sup_busy(false));
        match r {
            Ok(id) => {
                self.ext.sup.files.push(id);
                self.ext.sup.file_names.push(name);
            }
            Err(e) => self.sup_err(&e),
        }
        let label = if self.ext.sup.file_names.len() == 1 { self.ext.sup.file_names[0].clone() } else { files_label(self.ext.sup.file_names.len()) };
        self.st(|s| s.set_sup_files(label.into()));
    }

    pub fn sup_files_clear(&mut self) {
        self.ext.sup.files.clear();
        self.ext.sup.file_names.clear();
        self.st(|s| s.set_sup_files("".into()));
    }

    pub fn sup_close_ticket(&mut self) {
        let Some(id) = self.ext.sup.cur.as_ref().map(|d| d.id.clone()) else { return };
        self.st(|s| s.set_sup_busy(true));
        self.sup_send(SupOp::Close { id });
    }

    pub fn sup_closed(&mut self, id: String, r: Result<(), String>) {
        self.st(|s| s.set_sup_busy(false));
        match r {
            Ok(()) => {
                self.toast("Заявка закрыта");
                self.sup_ticket(id);
            }
            Err(e) => self.sup_err(&e),
        }
    }

    pub fn sup_web(&mut self) {
        let id = self.st(|s| if s.get_sup_view() == 1 { s.get_sup_id().to_string() } else { String::new() }).unwrap_or_default();
        let ret = if id.is_empty() { "%2Ftickets".to_string() } else { format!("%2Ftickets%2F{id}") };
        self.open_url(format!("https://funpay.com/support/sso?return_to={ret}"));
    }

    pub fn sup_file_open(&mut self, url: String) {
        self.open_url(url);
    }

    // ------------------------------------------------------------ новая заявка

    pub fn sup_new(&mut self) {
        self.ext.sup.cat.clear();
        self.ext.sup.fields.clear();
        self.ext.sup.values.clear();
        self.ext.sup.files.clear();
        self.ext.sup.file_names.clear();
        self.st(|s| {
            s.set_sup_view(2);
            s.set_sup_cat(-1);
            s.set_sup_fields(model(vec![]));
            s.set_sup_files("".into());
            s.set_sup_error("".into());
        });
        if self.ext.sup.cats.is_empty() {
            self.st(|s| s.set_sup_loading(true));
            self.sup_send(SupOp::Cats);
        } else {
            self.sup_render_cats();
        }
    }

    pub fn sup_cats(&mut self, r: Result<Vec<(String, String)>, String>) {
        self.st(|s| s.set_sup_loading(false));
        match r {
            Ok(v) => {
                self.ext.sup.cats = v;
                self.sup_render_cats();
            }
            Err(e) => self.sup_err(&e),
        }
    }

    fn sup_render_cats(&mut self) {
        let names: Vec<SharedString> = self.ext.sup.cats.iter().map(|c| c.1.clone().into()).collect();
        self.st(|s| s.set_sup_cats(model(names)));
    }

    pub fn sup_cat_pick(&mut self, i: i32) {
        let Some((id, _)) = self.ext.sup.cats.get(i.max(0) as usize).cloned() else { return };
        self.ext.sup.cat = id.clone();
        self.ext.sup.fields.clear();
        self.ext.sup.values.clear();
        self.st(|s| {
            s.set_sup_cat(i);
            s.set_sup_fields(model(vec![]));
            s.set_sup_loading(true);
            s.set_sup_error("".into());
        });
        let nick = self.me.name.clone();
        self.sup_send(SupOp::Fields { cat: id, nick });
    }

    pub fn sup_fields_loaded(&mut self, cat: String, r: Result<Vec<Field>, String>) {
        if cat != self.ext.sup.cat {
            return;
        }
        self.st(|s| s.set_sup_loading(false));
        match r {
            Ok(v) => {
                // сообщение пишется отдельным полем внизу формы
                self.ext.sup.fields = v.into_iter().filter(|f| !f.name.starts_with("ticket[comment]")).collect();
                for f in &self.ext.sup.fields {
                    if !f.default.is_empty() {
                        self.ext.sup.values.insert(f.name.clone(), f.default.clone());
                    }
                }
                self.sup_render_fields();
            }
            Err(e) => self.sup_err(&e),
        }
    }

    fn sup_visible(&self, f: &Field) -> bool {
        let mut cur = f;
        // условие может зависеть от поля, которое само спрятано
        for _ in 0..8 {
            let Some((name, val)) = &cur.cond else { return true };
            if self.ext.sup.values.get(name) != Some(val) {
                return false;
            }
            match self.ext.sup.fields.iter().find(|x| &x.name == name) {
                Some(p) => cur = p,
                None => return true,
            }
        }
        true
    }

    fn sup_render_fields(&mut self) {
        let rows: Vec<SupField> = self
            .ext.sup
            .fields
            .iter()
            .map(|f| {
                let value = self.ext.sup.values.get(&f.name).cloned().unwrap_or_default();
                let sel = f.options.iter().position(|o| o.0 == value).map(|x| x as i32).unwrap_or(-1);
                SupField {
                    name: f.name.clone().into(),
                    kind: f.kind.clone().into(),
                    label: f.label.clone().into(),
                    required: f.required,
                    options: model(f.options.iter().map(|o| SharedString::from(o.1.clone())).collect()),
                    sel,
                    value: value.into(),
                    visible: self.sup_visible(f),
                }
            })
            .collect();
        self.st(|s| s.set_sup_fields(model(rows)));
    }

    pub fn sup_field(&mut self, name: String, value: String) {
        self.ext.sup.values.insert(name, value);
    }

    pub fn sup_pick(&mut self, name: String, i: i32) {
        let Some(f) = self.ext.sup.fields.iter().find(|f| f.name == name) else { return };
        let Some((v, _)) = f.options.get(i.max(0) as usize).cloned() else { return };
        self.ext.sup.values.insert(name, v);
        // выбор может открыть или скрыть другие поля
        self.sup_render_fields();
    }

    pub fn sup_create(&mut self, msg: String) {
        if self.ext.sup.cat.is_empty() || self.ext.sup.uploading {
            return;
        }
        let mut values = Vec::new();
        let mut missing = Vec::new();
        for f in &self.ext.sup.fields {
            if f.kind == "note" || !self.sup_visible(f) {
                continue;
            }
            let v = self.ext.sup.values.get(&f.name).cloned().unwrap_or_default();
            if f.required && v.trim().is_empty() {
                missing.push(f.label.clone());
            }
            values.push((f.name.clone(), v));
        }
        if !missing.is_empty() {
            self.sup_err(&format!("Заполни: {}", missing.join(", ")));
            return;
        }
        if msg.trim().is_empty() {
            self.sup_err("Опиши проблему в сообщении.");
            return;
        }
        let cat = self.ext.sup.cat.clone();
        let files = self.ext.sup.files.clone();
        self.st(|s| {
            s.set_sup_busy(true);
            s.set_sup_error("".into());
        });
        self.sup_send(SupOp::Create { cat, values, msg, files });
    }

    pub fn sup_created(&mut self, r: Result<String, String>) {
        self.st(|s| s.set_sup_busy(false));
        match r {
            Ok(id) => {
                self.toast("Заявка отправлена");
                self.ext.sup.files.clear();
                self.ext.sup.file_names.clear();
                self.sup_refresh();
                if !id.is_empty() {
                    self.sup_ticket(id);
                } else {
                    self.st(|s| s.set_sup_view(0));
                }
            }
            Err(e) => self.sup_err(&e),
        }
    }
}
