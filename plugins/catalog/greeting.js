// @name Приветствие
// @author FunPay Tools
// @version 1.0
// @description Здоровается с покупателем, когда он пишет первым. Свой текст, картинка, пауза между приветствиями и переменные $username, $date, $time.

var S = fpt.storage.getJson("settings", null) || {
    text: "Привет, $username! 👋\nСпасибо, что написали. Отвечу в ближайшее время.",
    hours: 24,          // не здороваться снова раньше, чем через столько часов
    skipIfTalked: true, // не здороваться, если я уже писал в этом чате
    image: "",
    imageFirst: false,
    workHours: false,   // здороваться только в выбранные часы
    from: 9,
    to: 23,
};
var greeted = fpt.storage.getJson("greeted", {}); // чат → время приветствия
var busy = {};

function save() { fpt.storage.setJson("settings", S); }
function two(n) { return (n < 10 ? "0" : "") + n; }

function fill(text, e) {
    var d = new Date();
    var vars = {
        "$username": e.username || e.chatName || "",
        "$date": two(d.getDate()) + "." + two(d.getMonth() + 1) + "." + d.getFullYear(),
        "$time": two(d.getHours()) + ":" + two(d.getMinutes()),
    };
    Object.keys(vars).forEach(function (k) { text = text.split(k).join(vars[k]); });
    return text;
}

function inHours() {
    if (!S.workHours) return true;
    var h = new Date().getHours();
    return S.from <= S.to ? (h >= S.from && h < S.to) : (h >= S.from || h < S.to);
}

function remember(chatId) {
    greeted[chatId] = Date.now();
    var keys = Object.keys(greeted);
    if (keys.length > 2000) {
        keys.sort(function (a, b) { return greeted[a] - greeted[b]; }).slice(0, keys.length - 2000).forEach(function (k) { delete greeted[k]; });
    }
    fpt.storage.setJson("greeted", greeted);
}

fpt.on("onNewMessage", function (e) {
    if (e.isMe || e.isSystem || !e.chatId) return;
    if (busy[e.chatId]) return;
    var last = greeted[e.chatId] || 0;
    if (last && Date.now() - last < S.hours * 3600000) return;
    if (!inHours()) return;
    busy[e.chatId] = true;
    try {
        if (S.skipIfTalked) {
            var h = fpt.chat.getHistory(e.chatId) || [];
            var talked = h.some(function (m) { return m.isMe && m.id < e.id; });
            if (talked) { remember(e.chatId); return; }
        }
        var text = fill(S.text || "", e);
        var ok = S.image ? fpt.chat.sendWithImage(e.chatId, text, S.image, S.imageFirst) : (text.trim() ? fpt.chat.send(e.chatId, text) : false);
        if (ok) {
            remember(e.chatId);
            fpt.app.log("Поздоровался с " + (e.username || e.chatName));
        } else {
            fpt.app.log("Не отправилось приветствие: " + fpt.lastError());
        }
    } finally {
        busy[e.chatId] = false;
    }
});

// ---------------- настройки
function render() {
    fpt.ui.setState("text", S.text);
    fpt.ui.setState("hours", String(S.hours));
    fpt.ui.setState("skip", String(S.skipIfTalked));
    fpt.ui.setState("first", String(S.imageFirst));
    fpt.ui.setState("work", String(S.workHours));
    fpt.ui.setState("from", String(S.from));
    fpt.ui.setState("to", String(S.to));
    var kids = [
        { type: "Text", text: "Переменные: $username, $date, $time", color: "#7A8699" },
        { type: "Card", children: [
            { type: "Input", label: "Текст приветствия", stateKey: "text", singleLine: false, onChange: "window.saveAll()" },
            S.image
                ? { type: "Image", url: "file://" + S.image, height: 90, radius: 8 }
                : { type: "Button", text: "Добавить картинку", onClick: "window.pick()", style: "flat" },
        ] },
    ];
    if (S.image) {
        kids[1].children.push({ type: "Row", children: [
            { type: "Button", text: "Другая картинка", onClick: "window.pick()", style: "flat" },
            { type: "Button", text: "Убрать картинку", onClick: "window.unpick()", style: "flat" },
        ] });
        kids[1].children.push({ type: "Checkbox", text: "Картинка перед текстом", stateKey: "first", onChange: "window.saveAll()" });
    }
    kids.push({ type: "Card", children: [
        { type: "Slider", label: "Здороваться снова не раньше, чем через часов", min: 1, max: 168, stateKey: "hours", onChange: "window.saveAll()" },
        { type: "Checkbox", text: "Не здороваться, если я уже писал в этом чате", stateKey: "skip", onChange: "window.saveAll()" },
    ] });
    var work = { type: "Card", children: [
        { type: "Switch", text: "Только в рабочие часы", stateKey: "work", onChange: "window.saveAll()" },
    ] };
    if (S.workHours) {
        work.children.push({ type: "Slider", label: "С какого часа", min: 0, max: 23, stateKey: "from", onChange: "window.saveAll()" });
        work.children.push({ type: "Slider", label: "До какого часа", min: 1, max: 24, stateKey: "to", onChange: "window.saveAll()" });
    }
    kids.push(work);
    fpt.ui.setSlot(PLUGIN_SLOT_KEY, { type: "Column", children: kids });
}

window.saveAll = function () {
    var wasWork = S.workHours;
    S.text = fpt.ui.getState("text");
    S.hours = Math.max(1, Math.round(Number(fpt.ui.getState("hours")) || 24));
    S.skipIfTalked = fpt.ui.getState("skip") === "true";
    S.imageFirst = fpt.ui.getState("first") === "true";
    S.workHours = fpt.ui.getState("work") === "true";
    S.from = Math.round(Number(fpt.ui.getState("from")) || 0);
    S.to = Math.round(Number(fpt.ui.getState("to")) || 24);
    save();
    if (wasWork !== S.workHours) render();
};
window.pick = function () {
    var p = fpt.app.pickImage();
    if (!p) return;
    S.image = p;
    save();
    render();
};
window.unpick = function () {
    S.image = "";
    save();
    render();
};

render();
