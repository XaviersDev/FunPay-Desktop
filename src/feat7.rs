//! Глобальный поиск по всем сообщениям всех чатов и экспорт/бэкап чата.

use crate::searchidx;
use crate::ui::Ui;
use crate::worker::Cmd;
use crate::parse::Message;
use crate::SearchRow;
use slint::{ModelRc, VecModel};
use std::rc::Rc;

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

impl Ui {
    /// Сложить сообщения текущего чата в архив для поиска. Вызывается на
    /// chat_opened / new_messages / older_loaded.
    pub fn index_chat(&mut self, msgs: &[Message]) {
        let (Some(cur), false) = (self.cur.as_ref(), self.demo) else { return };
        if cur.kind == 3 || self.me.id.is_empty() || msgs.is_empty() {
            return;
        }
        searchidx::set_account(&self.me.id);
        searchidx::add(&self.me.id, &cur.id, cur.kind, &cur.title, msgs);
    }

    // ------------------------------------------------------------ поиск

    pub fn msg_search_query(&mut self, q: String) {
        let q = q.trim().to_string();
        let chars = q.chars().count();
        if chars < 2 {
            self.ext.ms_gen += 1;
            self.st(|s| {
                s.set_msg_search_loading(false);
                s.set_search_results(model(vec![]));
                s.set_msg_search_hint(if chars == 0 { "".into() } else { "Введите минимум 2 символа для поиска по сообщениям".into() });
            });
            return;
        }
        self.ext.ms_gen += 1;
        let gen = self.ext.ms_gen;
        let uid = self.me.id.clone();
        self.st(|s| {
            s.set_msg_search_loading(true);
            s.set_msg_search_hint("".into());
        });
        if uid.is_empty() {
            self.st(|s| {
                s.set_msg_search_loading(false);
                s.set_msg_search_hint("Нет входа в аккаунт".into());
            });
            return;
        }
        std::thread::spawn(move || {
            let hits = searchidx::search(&uid, &q, 200);
            crate::ui::post(move |u| u.msg_search_done(gen, uid, hits));
        });
    }

    pub fn msg_search_done(&mut self, gen: u64, uid: String, hits: Vec<(String, i32, String, Message)>) {
        if gen != self.ext.ms_gen || uid != self.me.id {
            return;
        }
        let rows: Vec<SearchRow> = hits
            .into_iter()
            .map(|(chat_id, kind, chat_name, m)| {
                let id = chat_id.clone();
                let message_id = m.id.to_string();
                let date = m
                    .ts
                    .map(|t| t.format("%d.%m.%Y %H:%M").to_string())
                    .unwrap_or_else(|| m.date_raw.clone());
                let text = m.text.split_whitespace().collect::<Vec<_>>().join(" ");
                SearchRow {
                    chat_id: id.into(),
                    kind,
                    message_id: message_id.into(),
                    chat_name: chat_name.into(),
                    author: m.author.into(),
                    date: date.into(),
                    text: text.into(),
                }
            })
            .collect();
        let n = rows.len();
        self.st(move |s| {
            s.set_msg_search_loading(false);
            s.set_search_results(model(rows));
            s.set_msg_search_hint(if n == 0 { "Ничего не найдено".into() } else { format!("Найдено: {n}").into() });
        });
    }

    pub fn msg_search_openchat(&mut self, chat_id: String, kind: i32, message_id: String) {
        let Some(message_id) = message_id.parse::<u64>().ok() else {
            self.toast("Не удалось открыть найденное сообщение");
            return;
        };
        self.ext.search_jump = Some((chat_id.clone(), message_id));
        self.open_chat(chat_id, kind);
    }

    pub fn check_msg_search_jump(&mut self) {
        let Some((chat_id, message_id)) = self.ext.search_jump.clone() else { return };
        if self.cur.as_ref().map(|c| c.id.as_str()) != Some(chat_id.as_str()) {
            self.ext.search_jump = None;
            return;
        }
        if self.cur.as_ref().is_some_and(|c| c.msgs.iter().any(|m| m.id == message_id)) {
            self.ext.search_jump = None;
            self.jump_to(message_id.to_string());
            return;
        }
        let (has_older, loading) = self.st(|s| (s.get_has_older(), s.get_loading_older())).unwrap_or_default();
        if has_older && !loading {
            self.load_older();
        } else if !has_older {
            self.ext.search_jump = None;
            self.toast("Найденное сообщение не входит в историю этого чата");
        }
    }

    // ------------------------------------------------------------ экспорт

    /// Клик по пункту меню чата «Экспорт». Диалог сохранения — в потоке UI.
    pub fn export_chat(&mut self, id: String, kind: i32, json: bool) {
        let name = self
            .chats
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .or_else(|| self.cfg.lock().unwrap().known_chats.get(&id).map(|(_, n)| n.clone()))
            .unwrap_or_else(|| id.clone());
        let safe = |s: &str| -> String { s.chars().map(|c| if "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZАБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдеёжзийклмнопрстуфхцчшщъыьэюя -_".contains(c) { c } else { '_' }).collect() };
        let fname = format!(
            "{} {}.{}",
            safe(&name.trim()),
            chrono::Local::now().format("%Y-%m-%d"),
            if json { "json" } else { "txt" }
        );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Экспорт переписки")
            .set_file_name(&fname)
            .add_filter(if json { "JSON" } else { "Текст" }, &[if json { "json" } else { "txt" }])
            .save_file()
        else {
            return;
        };
        self.st(|s| s.set_msg_search_hint("".into()));
        self.toast("Экспорт: догружаю историю…");
        self.send(Cmd::ExportChat { id, kind, path, json, name });
    }
}
