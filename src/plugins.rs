//! Плагины на JavaScript (движок QuickJS). Каждый плагин живёт в своём потоке и своём движке.
//!
//! Безопасность: плагин не видит golden_key и куки. Запросы к funpay.com он делает через
//! `fpt.network.*`, куки подставляются в Rust и в ответ не попадают. Чтения файлов нет.
//! Опасные действия с аккаунтом (смена пароля, почты, вывод денег) спрашивают разрешение.
//!
//! API совместим с плагинами Android-приложения (DAI.md), десктопные дополнения описаны в DAID.md.

use crate::config::Config;
use crate::fp::Fp;
use once_cell::sync::{Lazy, OnceCell};
use rquickjs::{CatchResultExt, Context, Function, Runtime};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DOCS_URL: &str = "https://funpay.tools/DAID.md";
pub const CATALOG_URL: &str = "https://raw.githubusercontent.com/XaviersDev/FunPayTools-Site/refs/heads/main/catalog/desktopplugins.json";

static CFG: OnceCell<Arc<Mutex<Config>>> = OnceCell::new();
static RUNNING: Lazy<Mutex<HashMap<String, Sender<Msg>>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static STORAGE: Lazy<Mutex<HashMap<String, BTreeMap<String, String>>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static LOGS: Lazy<Mutex<HashMap<String, VecDeque<String>>>> = Lazy::new(|| Mutex::new(HashMap::new()));
/// Своя сессия FunPay для плагинов (тот же аккаунт, отдельный поток запросов).
static PFP: Lazy<Mutex<Option<Fp>>> = Lazy::new(|| Mutex::new(None));
/// Разрешения на опасные действия: плагин → разрешено навсегда.
static ALLOWED: Lazy<Mutex<HashMap<String, bool>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub enum Msg {
    Event(String, String),
    Eval(String),
    SetState(String, String),
    Stop,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Meta {
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub banner: String,
    pub builtin: bool,
    pub file: String,
}

// ---------------------------------------------------------------- встроенные плагины

pub struct Builtin {
    pub id: &'static str,
    pub code: &'static str,
}

pub const BUILTINS: &[Builtin] = &[
    Builtin { id: "builtin.autoraise", code: include_str!("../plugins/autoraise.js") },
    Builtin { id: "builtin.reviewreply", code: include_str!("../plugins/reviewreply.js") },
    Builtin { id: "builtin.automsg", code: include_str!("../plugins/automsg.js") },
];

pub fn init(cfg: Arc<Mutex<Config>>) {
    let _ = CFG.set(cfg);
    let _ = std::fs::create_dir_all(dir());
    let _ = std::fs::create_dir_all(data_dir());
}

fn cfg() -> Option<Arc<Mutex<Config>>> {
    CFG.get().cloned()
}

pub fn dir() -> PathBuf {
    crate::config::dir().join("plugins")
}

fn data_dir() -> PathBuf {
    dir().join("data")
}

/// Папка для файлов плагина (картинки, которые выбрал пользователь).
pub fn files_dir(id: &str) -> PathBuf {
    let d = data_dir().join(safe(id));
    let _ = std::fs::create_dir_all(&d);
    d
}

fn safe(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// Метаданные из комментариев в начале файла: // @name, @author, @version, @description, @banner.
pub fn parse_meta(code: &str) -> Meta {
    let mut m = Meta::default();
    for line in code.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let Some(rest) = t.strip_prefix("//") else { break };
        let rest = rest.trim();
        let Some(rest) = rest.strip_prefix('@') else { continue };
        let (k, v) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let v = v.trim().to_string();
        match k {
            "name" => m.name = v,
            "author" => m.author = v,
            "version" => m.version = v,
            "description" => m.description = v,
            "banner" => m.banner = v,
            _ => {}
        }
    }
    m
}

/// Все плагины: встроенные и файлы из папки plugins.
pub fn list() -> Vec<Meta> {
    let mut out = Vec::new();
    for b in BUILTINS {
        let mut m = parse_meta(b.code);
        m.id = b.id.to_string();
        m.builtin = true;
        out.push(m);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir())
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "js").unwrap_or(false)).collect())
        .unwrap_or_default();
    files.sort();
    for p in files {
        let Ok(code) = std::fs::read_to_string(&p) else { continue };
        let mut m = parse_meta(&code);
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        m.id = format!("user.{stem}");
        if m.name.is_empty() {
            m.name = stem;
        }
        m.file = p.to_string_lossy().to_string();
        out.push(m);
    }
    out
}

fn code_of(id: &str) -> Option<String> {
    if let Some(b) = BUILTINS.iter().find(|b| b.id == id) {
        return Some(b.code.to_string());
    }
    let stem = id.strip_prefix("user.")?;
    std::fs::read_to_string(dir().join(format!("{stem}.js"))).ok()
}

/// Установить плагин из текста (файл или каталог). Возвращает id.
pub fn install(code: &str, hint: &str) -> Result<String, String> {
    let meta = parse_meta(code);
    if meta.name.is_empty() {
        return Err("Это не похоже на плагин: в начале файла нет строки // @name".into());
    }
    let base: String = {
        let src = if hint.is_empty() { meta.name.clone() } else { hint.to_string() };
        let s: String = src
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else if c.is_alphanumeric() { translit(c) } else { '_' })
            .collect();
        let s = s.trim_matches('_').to_string();
        if s.is_empty() { "plugin".into() } else { s.chars().take(40).collect() }
    };
    let path = dir().join(format!("{base}.js"));
    std::fs::write(&path, code).map_err(|e| format!("Не удалось сохранить плагин: {e}"))?;
    Ok(format!("user.{base}"))
}

/// Версия как список чисел: "1.0.6-android" → [1, 0, 6].
fn ver_nums(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
}

pub fn ver_newer(new: &str, old: &str) -> bool {
    let (a, b) = (ver_nums(new), ver_nums(old));
    !a.is_empty() && a > b
}

/// Обновить установленный плагин кодом из каталога, если там версия выше. Имя сверяется по @name.
/// Возвращает (id, название, новая версия). Настройки и данные плагина остаются: id не меняется.
pub fn update_from(code: &str) -> Option<(String, String, String)> {
    let fresh = parse_meta(code);
    if fresh.name.is_empty() || fresh.version.is_empty() {
        return None;
    }
    let cur = list().into_iter().find(|m| !m.builtin && m.name.to_lowercase() == fresh.name.to_lowercase())?;
    if !ver_newer(&fresh.version, &cur.version) {
        return None;
    }
    std::fs::write(&cur.file, code).ok()?;
    Some((cur.id, fresh.name, fresh.version))
}

/// Проверить каталог и обновить установленные из него плагины. Работает в фоновом потоке.
pub fn check_updates() -> Vec<(String, String, String)> {
    let mut done = Vec::new();
    if !list().iter().any(|m| !m.builtin) {
        return done;
    }
    let c = crate::fp::http_client_follow(30);
    let Some(cat) = c
        .get(CATALOG_URL)
        .send()
        .ok()
        .filter(|r| r.status().is_success())
        .and_then(|r| r.text().ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return done;
    };
    for e in cat.as_array().cloned().unwrap_or_default() {
        let url = e.get("pluginUrl").and_then(|v| v.as_str()).unwrap_or("");
        if !url.starts_with("https://") {
            continue;
        }
        let Some(code) = c.get(url).send().ok().filter(|r| r.status().is_success()).and_then(|r| r.text().ok()) else { continue };
        if let Some((id, name, ver)) = update_from(&code) {
            log(&id, &format!("Плагин обновлён из каталога до версии {ver}"));
            if is_running(&id) {
                restart(&id);
            } else if is_loaded(&id) {
                stop(&id);
            }
            done.push((id, name, ver));
        }
    }
    done
}

static UPDATER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Раз в три часа сверяем установленные плагины с каталогом. Запускается один раз.
pub fn start_updater() {
    if UPDATER.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("plugin updates".into()).spawn(|| {
        std::thread::sleep(Duration::from_secs(20));
        loop {
            let done = check_updates();
            if !done.is_empty() {
                crate::ui::post(move |u| {
                    u.rebuild_plugins();
                    let names: Vec<String> = done.iter().map(|(_, n, v)| format!("{n} {v}")).collect();
                    u.toast(&format!("Плагины обновлены: {}", names.join(", ")));
                });
            }
            std::thread::sleep(Duration::from_secs(3 * 3600));
        }
    });
}

fn translit(c: char) -> char {
    const RU: &str = "абвгдеёжзийклмнопрстуфхцчшщъыьэюя";
    const EN: &str = "abvgdeejziiklmnoprstufhccss_y_eua";
    RU.chars().position(|x| x == c).and_then(|i| EN.chars().nth(i)).unwrap_or('_')
}

pub fn remove(id: &str) {
    stop(id);
    if let Some(stem) = id.strip_prefix("user.") {
        let _ = std::fs::remove_file(dir().join(format!("{stem}.js")));
        let _ = std::fs::remove_file(data_dir().join(format!("{}.json", safe(id))));
        let _ = std::fs::remove_dir_all(data_dir().join(safe(id)));
    }
}

/// Плагины, запущенные только ради окна настроек: без событий, таймеров и запросов.
static SETTINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn settings_only(id: &str) -> bool {
    SETTINGS.lock().unwrap().iter().any(|x| x == id)
}

/// Плагин включён и работает.
pub fn is_running(id: &str) -> bool {
    RUNNING.lock().unwrap().contains_key(id) && !settings_only(id)
}

/// Плагин загружен: работает или открыт только для настроек.
pub fn is_loaded(id: &str) -> bool {
    RUNNING.lock().unwrap().contains_key(id)
}

/// Выключенный плагин открыли в настройках: запускаем его код так, чтобы он только нарисовал настройки
/// и сохранял их, но ничего не делал с аккаунтом.
pub fn start_settings(id: &str) {
    if is_loaded(id) {
        return;
    }
    SETTINGS.lock().unwrap().push(id.to_string());
    if let Err(e) = start_inner(id) {
        SETTINGS.lock().unwrap().retain(|x| x != id);
        log(id, &e);
    }
}

/// Закрыли настройки выключенного плагина или включили его: режим настроек больше не нужен.
pub fn stop_settings(id: &str) {
    if settings_only(id) {
        stop(id);
        for _ in 0..40 {
            if !RUNNING.lock().unwrap().contains_key(id) && !settings_only(id) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

pub fn send(id: &str, m: Msg) {
    if let Some(tx) = RUNNING.lock().unwrap().get(id) {
        let _ = tx.send(m);
    }
}

pub fn stop(id: &str) {
    if let Some(tx) = RUNNING.lock().unwrap().remove(id) {
        let _ = tx.send(Msg::Stop);
    }
}

pub fn stop_all() {
    let ids: Vec<String> = RUNNING.lock().unwrap().keys().cloned().collect();
    for id in ids {
        stop(&id);
    }
}

/// Событие всем запущенным плагинам.
pub fn emit(event: &str, data: Value) {
    let s = data.to_string();
    let set = SETTINGS.lock().unwrap().clone();
    for (id, tx) in RUNNING.lock().unwrap().iter() {
        if !set.contains(id) {
            let _ = tx.send(Msg::Event(event.to_string(), s.clone()));
        }
    }
}

pub fn any_running() -> bool {
    let set = SETTINGS.lock().unwrap().clone();
    RUNNING.lock().unwrap().keys().any(|k| !set.contains(k))
}

/// Сменился аккаунт: сессию плагинов создаём заново.
pub fn reset_session() {
    *PFP.lock().unwrap() = None;
}

pub fn logs(id: &str) -> Vec<String> {
    LOGS.lock().unwrap().get(id).map(|v| v.iter().cloned().collect()).unwrap_or_default()
}

thread_local! {
    /// плагин, чей вызов сейчас выполняется в этом потоке (для журнала запросов к FunPay)
    static CURRENT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Строка о запросе к FunPay в журнал плагина, который его сделал.
pub fn trace(line: &str) {
    let id = CURRENT.with(|c| c.borrow().clone());
    if !id.is_empty() {
        log_ex(&id, line, false);
    }
}

fn log(id: &str, line: &str) {
    log_ex(id, line, true);
}

fn log_ex(id: &str, line: &str, to_file: bool) {
    let t = chrono::Local::now().format("%H:%M:%S");
    let l = format!("{t} {line}");
    {
        let mut g = LOGS.lock().unwrap();
        let q = g.entry(id.to_string()).or_default();
        q.push_back(l.clone());
        while q.len() > 200 {
            q.pop_front();
        }
    }
    if to_file {
        crate::log::write(&format!("[плагин {id}] {line}"));
    }
    let id = id.to_string();
    crate::ui::post(move |u| u.plugin_log(id, l));
}

// ---------------------------------------------------------------- хранилище

fn storage_path(id: &str) -> PathBuf {
    data_dir().join(format!("{}.json", safe(id)))
}

fn storage_load(id: &str) -> BTreeMap<String, String> {
    let mut g = STORAGE.lock().unwrap();
    if let Some(m) = g.get(id) {
        return m.clone();
    }
    let m: BTreeMap<String, String> = std::fs::read_to_string(storage_path(id)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    g.insert(id.to_string(), m.clone());
    m
}

pub fn storage_get(id: &str, key: &str) -> Option<String> {
    storage_load(id).get(key).cloned()
}

pub fn storage_set(id: &str, key: &str, val: Option<&str>) {
    let mut m = storage_load(id);
    match val {
        Some(v) => {
            m.insert(key.to_string(), v.to_string());
        }
        None => {
            m.remove(key);
        }
    }
    let _ = std::fs::write(storage_path(id), serde_json::to_string_pretty(&m).unwrap_or_default());
    STORAGE.lock().unwrap().insert(id.to_string(), m);
}

// ---------------------------------------------------------------- запуск

const PRELUDE: &str = include_str!("../plugins/prelude.js");

/// Запустить плагин в своём потоке (если ещё не запущен).
pub fn start(id: &str) -> Result<(), String> {
    if is_running(id) {
        return Ok(());
    }
    stop_settings(id);
    start_inner(id)
}

fn start_inner(id: &str) -> Result<(), String> {
    let code = code_of(id).ok_or_else(|| "Файл плагина не найден".to_string())?;
    let (tx, rx) = channel::<Msg>();
    RUNNING.lock().unwrap().insert(id.to_string(), tx);
    let pid = id.to_string();
    std::thread::Builder::new()
        .name(format!("plugin {id}"))
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            if let Err(e) = run(&pid, &code, rx) {
                log(&pid, &format!("Плагин остановлен с ошибкой: {e}"));
            }
            RUNNING.lock().unwrap().remove(&pid);
            SETTINGS.lock().unwrap().retain(|x| *x != pid);
            let p = pid.clone();
            crate::ui::post(move |u| u.plugin_stopped(p));
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn restart(id: &str) {
    stop(id);
    // ждём, пока поток закроется
    for _ in 0..40 {
        if !is_running(id) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    if let Err(e) = start(id) {
        log(id, &e);
    }
}

struct Timer {
    id: i64,
    due: Instant,
    every: Option<Duration>,
}

fn run(id: &str, code: &str, rx: Receiver<Msg>) -> Result<(), String> {
    let rt = Runtime::new().map_err(|e| e.to_string())?;
    rt.set_memory_limit(96 * 1024 * 1024);
    rt.set_max_stack_size(4 * 1024 * 1024);
    // защита от вечных циклов: один вызов не дольше 20 секунд чистого JS
    let deadline = Arc::new(AtomicI64::new(i64::MAX));
    {
        let d = deadline.clone();
        rt.set_interrupt_handler(Some(Box::new(move || now_ms() > d.load(Ordering::Relaxed))));
    }
    let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
    let timers: std::rc::Rc<std::cell::RefCell<Vec<Timer>>> = Default::default();

    let pid = id.to_string();
    let setup = ctx.with(|ctx| -> Result<(), String> {
        let g = ctx.globals();
        {
            let pid = pid.clone();
            let d = deadline.clone();
            let f = Function::new(ctx.clone(), move |name: String, args: String| -> String {
                let res = host(&pid, &name, &args);
                // время сетевого запроса не считаем временем работы JS
                d.store(now_ms() + 20_000, Ordering::Relaxed);
                res
            })
            .map_err(|e| e.to_string())?;
            g.set("__host", f).map_err(|e| e.to_string())?;
        }
        {
            let t = timers.clone();
            let f = Function::new(ctx.clone(), move |tid: i64, ms: f64, repeat: bool| {
                let ms = ms.max(if repeat { 50.0 } else { 0.0 });
                let d = Duration::from_millis(ms as u64);
                t.borrow_mut().push(Timer { id: tid, due: Instant::now() + d, every: if repeat { Some(d) } else { None } });
            })
            .map_err(|e| e.to_string())?;
            g.set("__timer", f).map_err(|e| e.to_string())?;
        }
        {
            let t = timers.clone();
            let f = Function::new(ctx.clone(), move |tid: i64| {
                t.borrow_mut().retain(|x| x.id != tid);
            })
            .map_err(|e| e.to_string())?;
            g.set("__cleartimer", f).map_err(|e| e.to_string())?;
        }
        g.set("PLUGIN_ID", pid.clone()).map_err(|e| e.to_string())?;
        g.set("PLUGIN_SLOT_KEY", format!("settings_{pid}")).map_err(|e| e.to_string())?;
        Ok(())
    });
    setup?;

    let exec = |src: &str, what: &str| {
        deadline.store(now_ms() + 20_000, Ordering::Relaxed);
        let r = ctx.with(|ctx| ctx.eval::<(), _>(src).catch(&ctx).map_err(|e| e.to_string()));
        deadline.store(i64::MAX, Ordering::Relaxed);
        if let Err(e) = r {
            log(id, &format!("Ошибка ({what}): {e}"));
        }
        // промисы
        loop {
            deadline.store(now_ms() + 20_000, Ordering::Relaxed);
            match rt.execute_pending_job() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(_) => {
                    log(id, "Ошибка в асинхронном коде плагина");
                    continue;
                }
            }
        }
        deadline.store(i64::MAX, Ordering::Relaxed);
    };

    exec(PRELUDE, "запуск");
    // код плагина в отдельной области видимости, как в Android-приложении
    exec(&format!("(function(){{\n{code}\n}})();"), "запуск");
    if !settings_only(id) {
        log(id, "Плагин запущен");
    }

    loop {
        let next = timers.borrow().iter().map(|t| t.due).min();
        let wait = next.map(|d| d.saturating_duration_since(Instant::now())).unwrap_or(Duration::from_secs(3600));
        match rx.recv_timeout(wait) {
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Msg::Event(name, data)) => {
                exec(&format!("__emit({}, {});", js_str(&name), data), &name);
            }
            Ok(Msg::Eval(src)) => exec(&src, "интерфейс"),
            Ok(Msg::SetState(k, v)) => exec(&format!("__uiSet({}, {});", js_str(&k), js_str(&v)), "интерфейс"),
            Err(RecvTimeoutError::Timeout) => {}
        }
        // сработавшие таймеры
        let now = Instant::now();
        let due: Vec<i64> = {
            let mut t = timers.borrow_mut();
            let ids: Vec<i64> = t.iter().filter(|x| x.due <= now).map(|x| x.id).collect();
            t.retain_mut(|x| {
                if x.due > now {
                    return true;
                }
                match x.every {
                    Some(d) => {
                        x.due = now + d;
                        true
                    }
                    None => false,
                }
            });
            ids
        };
        if settings_only(id) {
            continue;
        }
        for tid in due {
            exec(&format!("__fire({tid});"), "таймер");
        }
    }
    let p = id.to_string();
    let quiet = settings_only(id);
    crate::ui::post(move |u| u.plugin_slot(p, None));
    if !quiet {
        log(id, "Плагин выключен");
    }
    Ok(())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

// ---------------------------------------------------------------- вызовы из плагина

fn host(id: &str, name: &str, args: &str) -> String {
    let a: Vec<Value> = serde_json::from_str(args).unwrap_or_default();
    CURRENT.with(|c| *c.borrow_mut() = id.to_string());
    let safe = name.starts_with("storage.") || name.starts_with("ui.") || matches!(name, "app.log" | "app.toast" | "app.pickImage" | "app.saveBase64Image" | "app.version" | "app.platform" | "app.vibrate" | "app.updateWidgets");
    let res = if !safe && settings_only(id) { Err(String::new()) } else { call(id, name, &a) };
    CURRENT.with(|c| c.borrow_mut().clear());
    let r = match res {
        Ok(v) => json!({ "ok": v }),
        Err(e) => {
            if !e.is_empty() {
                log(id, &format!("{name}: {e}"));
            }
            json!({ "err": e })
        }
    };
    r.to_string()
}

fn s(a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Null) | None => String::new(),
        Some(v) => v.to_string(),
    }
}

fn b(a: &[Value], i: usize) -> bool {
    match a.get(i) {
        Some(Value::Bool(x)) => *x,
        Some(Value::String(x)) => x == "true" || x == "1",
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
        _ => false,
    }
}

/// Сессия FunPay для плагинов. Создаётся из сохранённого ключа текущего аккаунта.
fn with_fp<R>(f: impl FnOnce(&mut Fp) -> Result<R, String>) -> Result<R, String> {
    let mut g = PFP.lock().unwrap();
    let (gk, cookies) = {
        let c = cfg().ok_or("Нет настроек")?;
        let c = c.lock().unwrap();
        (c.golden_key.clone(), if c.cookies_for == c.golden_key { c.cookies.clone() } else { Default::default() })
    };
    if gk.is_empty() {
        return Err("Сначала войдите в аккаунт FunPay".into());
    }
    if g.as_ref().map(|f| f.gk != gk).unwrap_or(true) {
        let mut fp = Fp::new(&gk, cookies);
        fp.trace = true;
        *g = Some(fp);
    }
    f(g.as_mut().unwrap())
}

fn call(id: &str, name: &str, a: &[Value]) -> Result<Value, String> {
    match name {
        // --- приложение
        "app.log" => {
            log(id, &s(a, 0));
            Ok(Value::Null)
        }
        "app.toast" => {
            let t = s(a, 0);
            crate::ui::post(move |u| u.toast(&t));
            Ok(Value::Null)
        }
        "app.notify" => {
            let (t, m) = (s(a, 0), s(a, 1));
            crate::ui::post(move |u| u.plugin_notify(t, m));
            Ok(Value::Null)
        }
        "app.openUrl" => {
            let u = s(a, 0);
            if u.starts_with("https://") || u.starts_with("http://") {
                crate::ui::post(move |ui| ui.open_url(u));
            }
            Ok(Value::Null)
        }
        "app.pickImage" => pick_image(id),
        "app.saveBase64Image" => save_data_uri(id, &s(a, 0)).map(Value::String),
        "app.version" => Ok(Value::String(env!("CARGO_PKG_VERSION").into())),
        "app.platform" => Ok(Value::String("desktop".into())),
        // мобильные функции на компьютере ничего не делают
        "app.vibrate" | "app.updateWidgets" => Ok(Value::Null),

        // --- хранилище
        "storage.get" => Ok(storage_get(id, &s(a, 0)).map(Value::String).unwrap_or(Value::Null)),
        "storage.set" => {
            storage_set(id, &s(a, 0), Some(&s(a, 1)));
            Ok(Value::Null)
        }
        "storage.remove" => {
            storage_set(id, &s(a, 0), None);
            Ok(Value::Null)
        }
        "storage.keys" => Ok(json!(storage_load(id).keys().cloned().collect::<Vec<_>>())),

        // --- интерфейс
        "ui.setSlot" => {
            let ui = a.first().cloned().unwrap_or(Value::Null);
            let state = a.get(1).cloned().unwrap_or(Value::Null);
            let p = id.to_string();
            crate::ui::post(move |u| u.plugin_slot(p, Some((ui, state))));
            Ok(Value::Null)
        }
        "ui.removeSlot" => {
            let p = id.to_string();
            crate::ui::post(move |u| u.plugin_slot(p, None));
            Ok(Value::Null)
        }
        "ui.state" => {
            let (p, k, v) = (id.to_string(), s(a, 0), s(a, 1));
            crate::ui::post(move |u| u.plugin_state(p, k, v));
            Ok(Value::Null)
        }

        // --- чаты
        "chat.getList" => with_fp(|fp| {
            let list = fp.bookmarks()?;
            Ok(json!(list
                .iter()
                .map(|c| json!({
                    "id": c.id, "chatId": c.id, "username": c.name, "lastMessage": c.preview,
                    "isUnread": c.unread, "avatarUrl": c.avatar, "date": c.time
                }))
                .collect::<Vec<_>>()))
        }),
        "chat.getHistory" => {
            let node = s(a, 0);
            with_fp(|fp| {
                let me = fp.user_id();
                let mut ctx = Default::default();
                let (msgs, _, _) = fp.history(&node, None, &mut ctx)?;
                Ok(json!(msgs.iter().rev().take(50).rev().map(|m| msg_json(m, &me, &node)).collect::<Vec<_>>()))
            })
        }
        "chat.send" => {
            let (node, text) = (s(a, 0), s(a, 1));
            with_fp(|fp| fp.send_message(&node, &text, None).map(|_| Value::Bool(true)))
        }
        "chat.sendWithImage" => {
            let (node, text, img, first) = (s(a, 0), s(a, 1), s(a, 2), a.get(3).map(|_| b(a, 3)).unwrap_or(true));
            let (bytes, name, mime) = image_bytes(id, &img)?;
            with_fp(|fp| {
                let image_id = fp.upload_chat_image(bytes, &name, &mime)?;
                if text.trim().is_empty() {
                    fp.send_message(&node, "", Some(&image_id))?;
                } else if first {
                    fp.send_message(&node, "", Some(&image_id))?;
                    fp.send_message(&node, &text, None)?;
                } else {
                    fp.send_message(&node, &text, None)?;
                    fp.send_message(&node, "", Some(&image_id))?;
                }
                Ok(Value::Bool(true))
            })
        }
        "chat.markRead" => {
            let node = s(a, 0);
            with_fp(|fp| fp.mark_read(&node).map(|_| Value::Null))
        }
        "chat.resolveUserId" => {
            let node = s(a, 0);
            with_fp(|fp| {
                let me = fp.user_id();
                Ok(Value::String(resolve_peer(&node, &me)))
            })
        }
        "chat.create" => {
            let (uid, text) = (s(a, 0), s(a, 1));
            with_fp(|fp| {
                fp.ensure_auth(false)?;
                let me: u64 = fp.user_id().parse().unwrap_or(0);
                let other: u64 = uid.parse().map_err(|_| "Нужен числовой ID пользователя".to_string())?;
                let node = format!("users-{}-{}", me.min(other), me.max(other));
                fp.send_message(&node, &text, None)?;
                Ok(Value::Bool(true))
            })
        }
        "chat.getInfo" => Ok(Value::Null),

        // --- заказы
        "orders.getDetails" => {
            let oid = s(a, 0).trim_start_matches('#').to_string();
            with_fp(|fp| {
                let o = fp.order(&oid)?;
                Ok(json!({
                    "orderId": o.id, "id": o.id, "status": o.status, "game": o.game, "lotName": o.short_desc,
                    "price": o.price, "buyerName": o.peer_name, "buyerId": o.peer_id, "peerName": o.peer_name, "peerId": o.peer_id,
                    "canRefund": o.can_refund, "canConfirm": o.can_confirm, "iAmBuyer": o.is_buyer,
                    "hasReview": o.has_review, "reviewStars": o.review_rating, "reviewText": o.review_text,
                    "reviewReply": o.review_reply, "replyIsMine": o.reply_mine, "chatId": o.chat_node, "lotUrl": o.lot_url,
                    "params": o.params.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
                    "link": format!("https://funpay.com/orders/{}/", o.id)
                }))
            })
        }
        "orders.refund" => {
            let oid = s(a, 0);
            ask_permission(id, &format!("вернуть деньги по заказу #{oid}"))?;
            with_fp(|fp| fp.refund(&oid).map(|_| Value::Bool(true)))
        }
        "orders.confirm" => {
            let oid = s(a, 0);
            ask_permission(id, &format!("подтвердить выполнение заказа #{oid}"))?;
            with_fp(|fp| fp.confirm_order(&oid).map(|_| Value::Bool(true)))
        }
        "orders.reviewReply" | "orders.reviewWrite" => {
            let (oid, text) = (s(a, 0), s(a, 1));
            let stars = a.get(2).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|x| x.parse().ok()))).unwrap_or(5) as i32;
            with_fp(|fp| fp.review(&oid, &text, stars).map(|_| Value::Bool(true)))
        }
        "orders.reviewDelete" => {
            let oid = s(a, 0);
            with_fp(|fp| fp.review_delete(&oid).map(|_| Value::Bool(true)))
        }

        // --- лоты
        "lots.getMy" => with_fp(|fp| {
            let lots = fp.my_lots()?;
            Ok(json!(lots
                .iter()
                .map(|l| json!({"id": l.id, "title": l.title, "node": l.node, "category": l.category, "price": l.price,
                    "amount": l.amount, "server": l.server, "active": l.active, "autoDelivery": l.auto}))
                .collect::<Vec<_>>()))
        }),
        "lots.getFields" => {
            let lid = s(a, 0);
            with_fp(|fp| {
                let f = fp.lot_form(&format!("offer={lid}"))?;
                let mut m = serde_json::Map::new();
                for x in &f.fields {
                    m.insert(x.name.clone(), Value::String(x.value.clone()));
                }
                Ok(json!({ "fields": m, "currency": f.currency }))
            })
        }
        "lots.save" => {
            let lid = s(a, 0);
            let fields = a.get(1).cloned().unwrap_or(Value::Null);
            with_fp(|fp| {
                let mut f = fp.lot_form(&format!("offer={lid}"))?;
                if let Value::Object(m) = fields {
                    for (k, v) in m {
                        let v = match v {
                            Value::String(x) => x,
                            Value::Bool(true) => "on".into(),
                            Value::Bool(false) => String::new(),
                            other => other.to_string(),
                        };
                        f.set(&k, &v);
                    }
                }
                fp.lot_save(&f, &format!("https://funpay.com/lots/offerEdit?offer={lid}"))?;
                Ok(Value::Bool(true))
            })
        }
        "lots.toggle" => {
            let (lid, on) = (s(a, 0), b(a, 1));
            with_fp(|fp| fp.lot_set_active(&lid, "", on).map(|_| Value::Bool(true)))
        }
        "lots.changePrice" => {
            let (lid, price) = (s(a, 0), s(a, 1));
            with_fp(|fp| {
                let mut f = fp.lot_form(&format!("offer={lid}"))?;
                f.set("price", &price);
                fp.lot_save(&f, &format!("https://funpay.com/lots/offerEdit?offer={lid}"))?;
                Ok(Value::Bool(true))
            })
        }
        "lots.delete" => {
            let lid = s(a, 0);
            ask_permission(id, &format!("удалить лот {lid}"))?;
            with_fp(|fp| fp.delete_offer(&lid).map(|_| Value::Bool(true)))
        }
        "lots.copy" => {
            let (lid, node) = (s(a, 0), s(a, 1));
            with_fp(|fp| fp.lot_copy(&lid, &node).map(|_| Value::Bool(true)))
        }
        "lots.raiseAll" => with_fp(|fp| fp.raise_lots().map(|r| json!(r))),

        // --- профиль и аккаунт
        "users.getProfile" => with_fp(|fp| {
            fp.ensure_auth(false)?;
            let uid = fp.user_id();
            let p = fp.profile(&uid)?;
            Ok(json!({ "id": uid, "username": fp.username(), "balance": fp.app.balance, "rating": p.rating,
                "reviews": p.reviews_count, "avatar": p.avatar }))
        }),
        "users.get" => {
            let uid = s(a, 0);
            with_fp(|fp| {
                let p = fp.profile(&uid)?;
                Ok(json!({ "id": uid, "username": p.name, "rating": p.rating, "reviews": p.reviews_count, "avatar": p.avatar,
                    "online": p.online, "registered": p.reg_date }))
            })
        }
        "accounts.getActive" => with_fp(|fp| {
            fp.ensure_auth(false)?;
            Ok(json!({ "id": fp.user_id(), "username": fp.username() }))
        }),
        "accounts.getAll" => {
            let c = cfg().ok_or("Нет настроек")?;
            let c = c.lock().unwrap();
            Ok(json!(c.accounts.iter().map(|x| json!({"id": x.id, "username": x.name})).collect::<Vec<_>>()))
        }
        "accounts.switch" => Err("Переключать аккаунт из плагина нельзя".into()),

        // --- сеть
        "network.request" => network(id, &s(a, 0), &s(a, 1), a.get(2), a.get(3)),
        "network.fetchImageBase64" => {
            let url = s(a, 0);
            let bytes = fetch(&url)?;
            use base64::Engine;
            let mime = match image::guess_format(&bytes) {
                Ok(image::ImageFormat::Png) => "image/png",
                Ok(image::ImageFormat::Gif) => "image/gif",
                Ok(image::ImageFormat::WebP) => "image/webp",
                _ => "image/jpeg",
            };
            Ok(Value::String(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
        }
        "funpay.csrf" => with_fp(|fp| Ok(Value::String(fp.csrf()))),
        "funpay.userId" => with_fp(|fp| {
            fp.ensure_auth(false)?;
            Ok(Value::String(fp.user_id()))
        }),

        // --- ИИ
        "ai.ask" => crate::ai::ask(&s(a, 0)).map(Value::String),
        "ai.rewrite" => {
            let ctx = crate::ai::Ctx { history: s(a, 1), ..Default::default() };
            crate::ai::rewrite(&s(a, 0), &ctx).map(Value::String)
        }
        "ai.translate" => crate::ai::ask(&format!(
            "Переведи на английский язык текст описания товара для маркетплейса. Сохрани смысл, эмодзи и переносы строк. В ответе только перевод.\n\n{}",
            s(a, 0)
        ))
        .map(Value::String),

        _ => Err(format!("Метода {name} нет в FunPay Tools для компьютера")),
    }
}

fn msg_json(m: &crate::parse::Message, me: &str, node: &str) -> Value {
    json!({
        "id": m.id, "chatId": node, "author": m.author, "username": m.author, "authorId": m.author_id,
        "text": m.text, "isMe": !me.is_empty() && m.author_id == me, "time": m.date_raw,
        "imageUrl": m.image.clone().unwrap_or_default(), "isSystem": m.author_id == "0" || !m.badge.is_empty()
    })
}

/// "users-123-456" → id собеседника.
fn resolve_peer(node: &str, me: &str) -> String {
    let parts: Vec<&str> = node.split('-').collect();
    if parts.len() == 3 && parts[0] == "users" {
        return if parts[1] == me { parts[2].to_string() } else { parts[1].to_string() };
    }
    node.to_string()
}

/// Опасные адреса funpay.com: настройки аккаунта, безопасность, вывод денег.
fn sensitive(url: &str, method: &str) -> bool {
    if method.eq_ignore_ascii_case("GET") {
        return false;
    }
    let path = url.splitn(4, '/').nth(3).unwrap_or("").to_lowercase();
    ["account/", "withdraw", "finance/withdraw", "security", "user/password", "user/email", "account/logout"]
        .iter()
        .any(|p| path.starts_with(p))
}

/// Спросить пользователя. Поток плагина ждёт ответа, интерфейс не блокируется.
fn ask_permission(id: &str, what: &str) -> Result<(), String> {
    if ALLOWED.lock().unwrap().get(id).copied().unwrap_or(false) {
        return Ok(());
    }
    let (tx, rx) = channel::<u8>();
    let (p, w) = (id.to_string(), what.to_string());
    crate::ui::post(move |u| u.plugin_ask(p, w, tx));
    match rx.recv_timeout(Duration::from_secs(300)) {
        Ok(2) => {
            ALLOWED.lock().unwrap().insert(id.to_string(), true);
            Ok(())
        }
        Ok(1) => Ok(()),
        _ => Err("Пользователь не разрешил это действие".into()),
    }
}

fn network(id: &str, method: &str, url: &str, body: Option<&Value>, headers: Option<&Value>) -> Result<Value, String> {
    let method = if method.is_empty() { "GET".to_string() } else { method.to_uppercase() };
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Нужен адрес, начинающийся с https://".into());
    }
    let mut hs: Vec<(String, String)> = Vec::new();
    let hv = match headers {
        Some(Value::String(t)) => serde_json::from_str(t).unwrap_or(Value::Null),
        Some(v) => v.clone(),
        None => Value::Null,
    };
    if let Value::Object(m) = hv {
        for (k, v) in m {
            hs.push((k, v.as_str().map(String::from).unwrap_or_else(|| v.to_string())));
        }
    }
    let ct = hs.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|x| x.1.clone());
    let body: Option<(String, Vec<u8>)> = match body {
        None | Some(Value::Null) => None,
        Some(Value::String(t)) => Some((ct.clone().unwrap_or_else(|| "application/x-www-form-urlencoded; charset=UTF-8".into()), t.clone().into_bytes())),
        // объект: отправляем как форму (так FunPay принимает почти всё)
        Some(Value::Object(m)) => {
            let mut parts = Vec::new();
            for (k, v) in m {
                let push = |parts: &mut Vec<String>, k: &str, v: &Value| {
                    let val = v.as_str().map(String::from).unwrap_or_else(|| v.to_string());
                    parts.push(format!("{}={}", urlenc(k), urlenc(&val)));
                };
                match v {
                    Value::Array(arr) => {
                        for x in arr {
                            push(&mut parts, k, x);
                        }
                    }
                    _ => push(&mut parts, k, v),
                }
            }
            Some(("application/x-www-form-urlencoded; charset=UTF-8".into(), parts.join("&").into_bytes()))
        }
        Some(v) => Some(("application/json".into(), v.to_string().into_bytes())),
    };
    hs.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
    if crate::fp::is_funpay_url(url) {
        if sensitive(url, &method) {
            ask_permission(id, &format!("{method} {url}"))?;
        }
        let (code, text, final_url) = with_fp(|fp| fp.raw(&method, url, body, &hs))?;
        let json_body = serde_json::from_str::<Value>(&text).ok();
        return Ok(json!({ "code": code, "body": text, "url": final_url, "json": json_body }));
    }
    // чужие сайты: без кук FunPay
    let client = crate::fp::http_client_follow(30);
    let m = reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| "Неизвестный метод запроса".to_string())?;
    let mut rb = client.request(m, url);
    for (k, v) in &hs {
        rb = rb.header(k.as_str(), v.as_str());
    }
    if let Some((ct, bytes)) = body {
        rb = rb.header("Content-Type", ct).body(bytes);
    }
    let r = rb.send().map_err(|e| if e.is_timeout() { "Сайт не ответил вовремя".to_string() } else { format!("Нет связи: {e}") })?;
    let code = r.status().as_u16();
    let mut headers = serde_json::Map::new();
    for (k, v) in r.headers() {
        if k.as_str().eq_ignore_ascii_case("set-cookie") {
            continue;
        }
        headers.insert(k.to_string(), Value::String(v.to_str().unwrap_or("").to_string()));
    }
    let text = r.text().unwrap_or_default();
    let json_body = serde_json::from_str::<Value>(&text).ok();
    Ok(json!({ "code": code, "body": text, "headers": headers, "json": json_body }))
}

fn urlenc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    if crate::fp::is_funpay_url(url) && !url.contains("sfunpay.com") {
        return with_fp(|fp| fp.raw("GET", url, None, &[]).map(|r| r.1.into_bytes()));
    }
    let client = crate::fp::http_client_follow(30);
    crate::imgcache::fetch_bytes(&client, url).ok_or_else(|| "Не удалось скачать картинку".to_string())
}

fn pick_image(id: &str) -> Result<Value, String> {
    // окно выбора файла открываем в потоке интерфейса
    let (tx, rx) = channel::<Option<PathBuf>>();
    crate::ui::post(move |_| {
        let p = rfd::FileDialog::new().set_title("Картинка").add_filter("Картинки", &["png", "jpg", "jpeg", "gif", "webp", "bmp"]).pick_file();
        let _ = tx.send(p);
    });
    let Some(p) = rx.recv().ok().flatten() else { return Ok(Value::Null) };
    let bytes = std::fs::read(&p).map_err(|_| "Не удалось прочитать файл".to_string())?;
    if image::guess_format(&bytes).is_err() {
        return Err("Это не картинка".into());
    }
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "png".into());
    let name = format!("img_{}.{ext}", chrono::Utc::now().timestamp_millis());
    let dst = files_dir(id).join(name);
    std::fs::write(&dst, bytes).map_err(|e| e.to_string())?;
    Ok(Value::String(dst.to_string_lossy().to_string()))
}

fn save_data_uri(id: &str, uri: &str) -> Result<String, String> {
    use base64::Engine;
    let b64 = uri.split(',').nth(1).unwrap_or(uri);
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| "Неверный base64".to_string())?;
    let ext = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Png) => "png",
        Ok(image::ImageFormat::Gif) => "gif",
        Ok(image::ImageFormat::WebP) => "webp",
        Ok(_) => "jpg",
        Err(_) => return Err("Это не картинка".into()),
    };
    let dst = files_dir(id).join(format!("img_{}.{ext}", chrono::Utc::now().timestamp_millis()));
    std::fs::write(&dst, bytes).map_err(|e| e.to_string())?;
    Ok(dst.to_string_lossy().to_string())
}

/// Картинка для отправки: data:URI, ссылка или файл из папки плагина (или выбранный пользователем).
fn image_bytes(id: &str, img: &str) -> Result<(Vec<u8>, String, String), String> {
    let bytes = if img.starts_with("data:") {
        use base64::Engine;
        let b64 = img.split(',').nth(1).unwrap_or("");
        base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| "Неверный base64".to_string())?
    } else if img.starts_with("http") {
        fetch(img)?
    } else {
        let p = PathBuf::from(img.trim_start_matches("file://"));
        // только файлы плагина: чужие файлы с диска плагин отправить не может
        let base = files_dir(id).canonicalize().unwrap_or_else(|_| files_dir(id));
        let full = p.canonicalize().map_err(|_| "Файл картинки не найден".to_string())?;
        if !full.starts_with(&base) {
            return Err("Отправлять можно только картинки, выбранные через fpt.app.pickImage".into());
        }
        std::fs::read(&full).map_err(|_| "Файл картинки не найден".to_string())?
    };
    let tmp = std::env::temp_dir().join(format!("fpt_plugin_{}.img", chrono::Utc::now().timestamp_millis()));
    let ext = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Png) => "png",
        Ok(image::ImageFormat::Gif) => "gif",
        Ok(_) => "jpg",
        Err(_) => return Err("Это не картинка".into()),
    };
    let tmp = tmp.with_extension(ext);
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    let r = crate::worker::prepare_image(&tmp);
    let _ = std::fs::remove_file(&tmp);
    r
}

// ---------------------------------------------------------------- события из переписки

/// Чаты, где появились новые сообщения: (чат, прошлый id последнего сообщения).
/// Достаём новые сообщения и раздаём события плагинам. Работает в своём потоке.
pub fn chats_changed(list: Vec<(crate::parse::ChatItem, u64)>) {
    if !any_running() || list.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for (chat, prev) in list {
            let res = with_fp(|fp| {
                let me = fp.user_id();
                let my_name = fp.username();
                let mut ctx = Default::default();
                let (msgs, _, _) = fp.history(&chat.id, None, &mut ctx)?;
                Ok((me, my_name, msgs))
            });
            let Ok((me, my_name, msgs)) = res else { continue };
            for m in msgs.into_iter().filter(|m| m.id > prev) {
                let mut data = msg_json(&m, &me, &chat.id);
                data["username"] = Value::String(if m.author_id == "0" || m.author.is_empty() { chat.name.clone() } else { m.author.clone() });
                data["chatName"] = Value::String(chat.name.clone());
                emit("onNewMessage", data);
                if let Some((ev, order, buyer)) = system_event(&m.text) {
                    let i_am_buyer = !my_name.is_empty() && buyer.eq_ignore_ascii_case(&my_name);
                    emit(
                        ev,
                        json!({ "orderId": order, "chatId": chat.id, "buyerName": buyer, "text": m.text, "iAmBuyer": i_am_buyer, "messageId": m.id }),
                    );
                }
            }
        }
    });
}

static RE_ORDER: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"#([A-Z0-9]{6,12})").unwrap());
static RE_BUYER: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"(?i)(?:покупатель|buyer|покупець)\s+(\S+)").unwrap());

/// Оповещения FunPay → событие, номер заказа, ник покупателя.
pub fn system_event(text: &str) -> Option<(&'static str, String, String)> {
    let l = text.to_lowercase();
    let ev = if l.contains("оплатил заказ") || l.contains("has paid for order") || l.contains("сплатив замовлення") {
        "onNewOrder"
    } else if l.contains("подтвердил успешное выполнение заказа") || l.contains("confirmed that order") || l.contains("підтвердив успішне виконання") {
        "onOrderConfirmed"
    } else if l.contains("написал отзыв к заказу") || l.contains("изменил отзыв к заказу") || l.contains("has given feedback") || l.contains("написав відгук") || l.contains("змінив відгук") {
        "onNewReview"
    } else if l.contains("вернул деньги покупателю") || l.contains("has refunded") || l.contains("повернув гроші") {
        "onOrderRefunded"
    } else {
        return None;
    };
    let order = RE_ORDER.captures(text)?.get(1)?.as_str().to_string();
    let buyer = RE_BUYER.captures(text).map(|c| c[1].trim_end_matches(['.', ',']).to_string()).unwrap_or_default();
    Some((ev, order, buyer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_runs_plugin() {
        let code = "// @name Тест движка\n\
            fpt.storage.set('a', '1');\n\
            setTimeout(function () { fpt.storage.set('b', String([1, 2, 3].map(v => v * 2))); }, 50);\n\
            var n = 0; var t = setInterval(function () { n++; if (n === 3) { clearInterval(t); fpt.storage.set('n', String(n)); } }, 60);\n\
            fpt.on('onNewOrder', function (e) { fpt.storage.set('o', e.orderId + ':' + PLUGIN_ID); });\n\
            fpt.storage.setJson('j', { x: 1 });\n\
            Promise.resolve(5).then(v => fpt.storage.set('p', String(v)));\n\
            window.loop = function () { while (true) {} };";
        std::fs::create_dir_all(dir()).unwrap();
        let id = install(code, "t_engine_test").unwrap();
        start(&id).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        emit("onNewOrder", json!({ "orderId": "ABC123" }));
        // вечный цикл должен прерваться, а плагин продолжить жить
        send(&id, Msg::Eval("window.loop()".into()));
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(storage_get(&id, "a").as_deref(), Some("1"));
        assert_eq!(storage_get(&id, "b").as_deref(), Some("2,4,6"));
        assert_eq!(storage_get(&id, "n").as_deref(), Some("3"));
        assert_eq!(storage_get(&id, "p").as_deref(), Some("5"));
        assert_eq!(storage_get(&id, "o").as_deref(), Some("ABC123:user.t_engine_test"));
        assert_eq!(storage_get(&id, "j").as_deref(), Some(r#"{"x":1}"#));
        assert!(is_running(&id));
        remove(&id);
    }

    #[test]
    fn builtins_parse() {
        let extra = [("catalog.greeting", include_str!("../plugins/catalog/greeting.js"))];
        let all = BUILTINS.iter().map(|b| (b.id, b.code)).chain(extra);
        for (id, code) in all {
            let b = Builtin { id, code };
            let m = parse_meta(b.code);
            assert!(!m.name.is_empty(), "{}", b.id);
            // синтаксис встроенных плагинов
            let rt = Runtime::new().unwrap();
            let ctx = Context::full(&rt).unwrap();
            ctx.with(|ctx| {
                let src = format!("(function(){{ return; {} }})", b.code);
                let r: Result<(), _> = ctx.eval::<(), _>(src).catch(&ctx).map_err(|e| e.to_string());
                assert!(r.is_ok(), "{}: {:?}", b.id, r);
            });
        }
    }

    #[test]
    fn meta_and_events() {
        assert!(ver_newer("1.1.0", "1.0.6") && ver_newer("1.0.6-android", "1.0.5") && ver_newer("1.1", "1.0.9") && ver_newer("1.0.1", "1.0"));
        assert!(!ver_newer("1.0.0", "1.0.0") && !ver_newer("1.0", "1.0.1") && !ver_newer("", "1.0") && !ver_newer("2.2.2", "2.2.2"));
        let m = parse_meta("// @name Тест\n// @author Я\n// @version 1.2\n// @description Делает дело\nfpt.app.log('x');\n// @name потом");
        assert_eq!((m.name.as_str(), m.author.as_str(), m.version.as_str()), ("Тест", "Я", "1.2"));
        let (e, o, b) = system_event("Покупатель darkness21 оплатил заказ #F88412AB. Steam-ключ").unwrap();
        assert_eq!((e, o.as_str(), b.as_str()), ("onNewOrder", "F88412AB", "darkness21"));
        assert_eq!(system_event("Покупатель VifitRay подтвердил успешное выполнение заказа #L2HZYKWX и отправил деньги").unwrap().0, "onOrderConfirmed");
        assert_eq!(system_event("Покупатель VifitRay написал отзыв к заказу #L2HZYKWX.").unwrap().0, "onNewReview");
        assert!(system_event("привет").is_none());
        assert!(sensitive("https://funpay.com/account/settings", "POST"));
        assert!(!sensitive("https://funpay.com/account/settings", "GET"));
        assert!(!sensitive("https://funpay.com/lots/raise", "POST"));
        assert_eq!(resolve_peer("users-5-9", "5"), "9");
    }
}
