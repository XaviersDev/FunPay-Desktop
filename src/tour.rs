//! Обучение при первом запуске: вход, шаблоны, первый шаблон, «переписать с ИИ».

use crate::ui::{with_ui, Ui};
use std::time::Duration;

struct Step {
    id: &'static str,
    n: i32,
    title: &'static str,
    text: &'static str,
    next: &'static str,
    target: &'static str,
}

const STEPS: &[Step] = &[
    Step {
        id: "login",
        n: 1,
        title: "Привет! Я Рыцарь FunPay",
        text: "Помогу освоиться за минуту. Сначала войди в аккаунт: нажми «Войти через браузер» и войди как на сайте. Ключ останется только на этом компьютере.",
        next: "",
        target: "login",
    },
    Step {
        id: "hello",
        n: 0,
        title: "Ты внутри!",
        text: "Покажу пару неочевидных вещей: где шаблоны, как сделать первый и как переписать сообщение с ИИ.",
        next: "Поехали",
        target: "",
    },
    Step {
        id: "gear",
        n: 1,
        title: "Шаблоны в настройках",
        text: "Нажми на шестерёнку.",
        next: "",
        target: "gear",
    },
    Step {
        id: "tplrow",
        n: 2,
        title: "Вот они",
        text: "Открывай «Шаблоны».",
        next: "",
        target: "tpl-row",
    },
    Step {
        id: "tplnew",
        n: 3,
        title: "Первый шаблон",
        text: "Жми «Новый шаблон».",
        next: "",
        target: "tpl-new",
    },
    Step {
        id: "tplsave",
        n: 4,
        title: "Я уже заполнил пример",
        text: "$welcome и $username сами станут приветствием и ником покупателя. Поправь как хочешь и сохрани.",
        next: "",
        target: "tpl-save",
    },
    Step {
        id: "tplbar",
        n: 5,
        title: "Шаблон в каждом чате",
        text: "Кнопки шаблонов теперь над полем ввода. Клик вставляет текст, Alt с цифрой срабатывает с клавиатуры, а «/» в начале ищет шаблон по названию. В узком окне они прячутся в кнопку у стикеров.",
        next: "Дальше",
        target: "tpl-bar",
    },
    Step {
        id: "ai",
        n: 6,
        title: "Переписать с ИИ",
        text: "Пиши как привык, а правый клик по кнопке отправки перепишет вежливо. Смотри.",
        next: "",
        target: "send",
    },
    Step {
        id: "done",
        n: 0,
        title: "Готово, ты в теме",
        text: "Ещё мелочи: ссылки в чатах кликаются, аватарки открываются, а пройти обучение заново можно в настройках. Удачных продаж!",
        next: "Готово",
        target: "",
    },
];

const DEMO_RESULT: &str = "Здравствуйте! 🙃";

impl Ui {
    pub fn tour_id(&self) -> String {
        self.st(|s| s.get_tour().to_string()).unwrap_or_default()
    }

    pub fn tour_go(&mut self, id: &str) {
        let Some(st) = STEPS.iter().find(|s| s.id == id) else {
            self.tour_finish();
            return;
        };
        self.st(|s| {
            s.set_tour(st.id.into());
            s.set_tour_n(st.n);
            s.set_tour_title(st.title.into());
            s.set_tour_text(st.text.into());
            s.set_tour_next(st.next.into());
            s.set_tour_seen("".into());
            s.set_tour_target(st.target.into());
        });
        match id {
            "tplbar" => {
                self.st(|s| {
                    s.set_tpl_open(false);
                    s.set_tpl_editing(false);
                    s.set_settings_open(false);
                });
                if !self.tour_ensure_chat() {
                    self.tour_go("done");
                }
            }
            "ai" => {
                if !self.tour_ensure_chat() {
                    self.tour_go("done");
                    return;
                }
                self.tour_type(String::new(), "привет".chars().collect(), 0);
            }
            _ => {}
        }
    }

    /// Для шагов в чате: если чат не открыт, открываем первый личный (ничего не отправляется).
    fn tour_ensure_chat(&mut self) -> bool {
        if self.cur.as_ref().map(|c| c.kind != 3).unwrap_or(false) {
            return true;
        }
        let first = self.chats.first().map(|c| c.id.clone());
        match first {
            Some(id) => {
                self.open_chat(id, 0);
                true
            }
            None => false,
        }
    }

    fn tour_draft(&mut self, t: &str) {
        self.st(|s| {
            s.set_tour_draft(t.into());
            s.set_tour_draft_n(s.get_tour_draft_n() + 1);
        });
    }

    /// Печатает «привет» по буквам, потом показывает переписывание.
    fn tour_type(&mut self, mut done: String, rest: Vec<char>, i: usize) {
        if self.tour_id() != "ai" {
            return;
        }
        if i == 0 {
            self.st(|s| s.set_ai_orig("".into()));
        }
        if i < rest.len() {
            done.push(rest[i]);
            self.tour_draft(&done);
            let delay = 140 + (i as u64 * 37) % 90;
            slint::Timer::single_shot(Duration::from_millis(delay), move || {
                with_ui(move |u| u.tour_type(done, rest, i + 1));
            });
            return;
        }
        // напечатали: «нажимаем» правой кнопкой
        slint::Timer::single_shot(Duration::from_millis(900), || {
            with_ui(|u| {
                if u.tour_id() != "ai" {
                    return;
                }
                u.st(|s| {
                    s.set_ai_busy(true);
                    s.set_ai_orig("привет".into());
                });
                slint::Timer::single_shot(Duration::from_millis(1500), || {
                    with_ui(|u| {
                        u.st(|s| s.set_ai_busy(false));
                        if u.tour_id() != "ai" {
                            u.st(|s| s.set_ai_orig("".into()));
                            return;
                        }
                        u.tour_draft(DEMO_RESULT);
                        u.st(|s| {
                            s.set_tour_text("Было «привет», стало «Здравствуйте! 🙃». Так работает с любым черновиком. Кнопка «Вернуть как было» отменит.".into());
                            s.set_tour_next("Дальше".into());
                        });
                    });
                });
            });
        });
    }

    pub fn tour_next(&mut self) {
        let cur = self.tour_id();
        let i = STEPS.iter().position(|s| s.id == cur);
        match i.and_then(|i| STEPS.get(i + 1)) {
            Some(n) if cur != "done" => {
                let id = n.id;
                self.tour_go(id);
            }
            _ => self.tour_finish(),
        }
    }

    pub fn tour_event(&mut self, ev: &str) {
        let cur = self.tour_id();
        match (cur.as_str(), ev) {
            ("gear", "settings") => self.tour_go("tplrow"),
            ("tplrow", "tpl-open") | ("gear", "tpl-open") => self.tour_go("tplnew"),
            ("tplnew", "tpl-editing") => self.tour_go("tplsave"),
            ("tplsave", "saved") => self.tour_go("tplbar"),
            ("login", "logged-in") => {
                slint::Timer::single_shot(Duration::from_millis(1200), || {
                    with_ui(|u| u.tour_go("hello"));
                });
            }
            _ => {}
        }
    }

    pub fn tour_finish(&mut self) {
        let in_chat_demo = self.st(|s| s.get_tour_draft() == DEMO_RESULT || s.get_tour_draft() == "привет" || s.get_tour_draft() == "прив").unwrap_or(false);
        self.st(|s| {
            s.set_tour("".into());
            s.set_tour_target("".into());
            s.set_ai_busy(false);
            if in_chat_demo {
                s.set_ai_orig("".into());
            }
        });
        if in_chat_demo {
            self.tour_draft("");
        }
        let mut c = self.cfg.lock().unwrap();
        c.tour_done = true;
        c.save();
    }

    pub fn tour_start(&mut self) {
        let logged = self.st(|s| s.get_logged_in()).unwrap_or(false);
        self.tour_go(if logged { "hello" } else { "login" });
    }
}
