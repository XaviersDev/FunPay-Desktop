// @name Авто-поднятие
// @author FunPay Tools
// @version 1.0
// @description Поднимает все ваши лоты сам. Умный режим поднимает ровно тогда, когда FunPay разрешает, по каждой игре отдельно.

var S = {
    mode: fpt.storage.get("mode") || "smart",          // smart или fixed
    minutes: Number(fpt.storage.get("minutes") || 60),
    toast: fpt.storage.get("toast") === "true",
    last: Number(fpt.storage.get("last") || 0),        // время последней попытки, мс
    next: fpt.storage.getJson("next", {}),             // игра → когда можно снова, мс
    results: fpt.storage.getJson("results", []),       // последние результаты по играм
};

function save() {
    fpt.storage.set("mode", S.mode);
    fpt.storage.set("minutes", String(S.minutes));
    fpt.storage.set("toast", String(S.toast));
    fpt.storage.set("last", String(S.last));
    fpt.storage.setJson("next", S.next);
    fpt.storage.setJson("results", S.results);
}

function two(n) { return (n < 10 ? "0" : "") + n; }
function clock(ms) { var d = new Date(ms); return two(d.getHours()) + ":" + two(d.getMinutes()); }
function plural(n, a, b, c) {
    var m10 = n % 10, m100 = n % 100;
    return n + " " + (m10 === 1 && m100 !== 11 ? a : m10 >= 2 && m10 <= 4 && (m100 < 10 || m100 >= 20) ? b : c);
}

// когда пора снова
function dueAt() {
    if (S.mode === "fixed") return S.last + S.minutes * 60000;
    var keys = Object.keys(S.next);
    if (!keys.length) return 0;
    var min = Infinity;
    keys.forEach(function (k) { if (S.next[k] < min) min = S.next[k]; });
    return min;
}

var busy = false;
function raise(manual) {
    if (busy) return;
    busy = true;
    S.last = Date.now();
    fpt.app.log(manual ? "Поднимаю лоты по кнопке" : "Пора поднимать лоты");
    var res = fpt.lots.raiseAll();
    busy = false;
    if (!res) {
        fpt.app.log("Не получилось поднять лоты: " + (fpt.lastError() || "нет ответа FunPay"));
        // пробуем снова через 5 минут
        S.next = { retry: Date.now() + 5 * 60000 };
        save();
        render();
        return;
    }
    var raised = 0;
    S.next = {};
    S.results = res.map(function (r) {
        var waitMs = Math.max(30, r.wait || 0) * 1000;
        S.next[r.game] = Date.now() + waitMs + 10000;
        if (r.status === "raised") raised++;
        fpt.app.log(r.title + ": " + (r.status === "raised" ? "поднято" : r.status === "wait" ? "FunPay просит подождать, следующая попытка в " + clock(S.next[r.game]) : "ошибка, " + (r.msg || "нет ответа")));
        return { title: r.title, status: r.status, msg: r.msg, at: Date.now(), next: S.next[r.game] };
    });
    if (!res.length) S.next = { empty: Date.now() + 30 * 60000 };
    save();
    if (raised) {
        fpt.app.log("Подняты лоты: " + plural(raised, "категория", "категории", "категорий"));
        if (S.toast || manual) fpt.app.toast("Лоты подняты: " + plural(raised, "категория", "категории", "категорий"));
    } else if (manual) {
        fpt.app.toast(res.length ? "FunPay пока не даёт поднять, попробую позже" : "Нет лотов для поднятия");
    }
    render();
}

function tick() {
    if (Date.now() >= dueAt()) raise(false);
}

// ---------------- настройки
function render() {
    fpt.ui.setState("mode", S.mode === "fixed" ? "1" : "0");
    fpt.ui.setState("minutes", String(S.minutes));
    fpt.ui.setState("toast", String(S.toast));
    var due = dueAt();
    var rows = S.results.map(function (r) {
        var st = r.status === "raised" ? "поднято в " + clock(r.at)
            : r.status === "wait" ? "можно в " + clock(r.next)
            : "ошибка: " + (r.msg || "нет ответа");
        return { type: "Row", children: [
            { type: "Text", text: r.title, bold: true },
            { type: "Text", text: st, color: r.status === "error" ? "#E5484D" : "#7A8699" },
        ] };
    });
    var ui = { type: "Column", children: [
        { type: "Text", text: due > Date.now() ? "Следующее поднятие в " + clock(due) : "Подниму при следующей проверке", color: "#7A8699" },
        { type: "Spacer", size: 6 },
        { type: "Select", label: "Когда поднимать", stateKey: "mode", options: ["Умно: когда FunPay разрешит", "Через равные промежутки"], onChange: "window.setMode()" },
        S.mode === "fixed"
            ? { type: "Slider", label: "Каждые, минут", min: 15, max: 240, stateKey: "minutes", onChange: "window.setMinutes()" }
            : { type: "Spacer", size: 0 },
        { type: "Checkbox", text: "Сообщать, когда лоты подняты", stateKey: "toast", onChange: "window.setToast()" },
        { type: "Spacer", size: 6 },
        { type: "Button", text: "Поднять сейчас", busyText: "Запускаю...", onClick: "window.raiseNow()" },
    ] };
    if (rows.length) {
        ui.children.push({ type: "Divider" });
        ui.children.push({ type: "Text", text: "Последнее поднятие", bold: true });
        ui.children = ui.children.concat(rows);
    }
    fpt.ui.setSlot(PLUGIN_SLOT_KEY, ui);
}

window.setMode = function () {
    S.mode = fpt.ui.getState("mode") === "1" ? "fixed" : "smart";
    save();
    render();
};
window.setMinutes = function () {
    S.minutes = Math.max(15, Math.round(Number(fpt.ui.getState("minutes")) || 60));
    save();
    render();
};
window.setToast = function () {
    S.toast = fpt.ui.getState("toast") === "true";
    save();
};
window.raiseNow = function () { raise(true); };

render();
setTimeout(tick, 8000);
setInterval(tick, 60000);
