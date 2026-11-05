// @name Авто-сообщения
// @author FunPay Tools
// @version 1.0
// @description Пишет покупателю сам: после оплаты, просьбу оставить отзыв и напоминание подтвердить заказ. С картинкой и переменными.

var KINDS = [
    { key: "paid", title: "После оплаты", hint: "Сразу, как покупатель оплатил заказ",
      text: "Спасибо за покупку, $username! 🤝\nСкоро выдам заказ #$order_id, ожидайте." , on: false },
    { key: "review", title: "Просьба оставить отзыв", hint: "Когда покупатель подтвердил заказ, но ещё не оставил отзыв",
      text: "Спасибо за заказ, $username! 🤝\nБуду очень благодарен, если оставите отзыв. Это очень поможет мне! 😊", on: true },
    { key: "remind", title: "Напоминание подтвердить заказ", hint: "Если заказ всё ещё не подтверждён через заданное время",
      text: "⚠️ $username, Вы забыли подтвердить заказ.\n\n⚡ Заказ выполнен. Пожалуйста, зайдите в раздел \"Покупки\", выберите его в списке и нажмите кнопку \"Подтвердить выполнение заказа\".\n\n⭐ Спасибо.", on: true },
];

var S = fpt.storage.getJson("settings", null) || {};
KINDS.forEach(function (k) {
    if (!S[k.key]) S[k.key] = { on: k.on, text: k.text, image: "", imageFirst: true, markRead: false };
});
if (S.remindHours === undefined) S.remindHours = 4;
var queue = fpt.storage.getJson("queue", []);   // отложенные напоминания
var sent = fpt.storage.getJson("sent", {});     // что уже отправлено: "review:ID" → время

function save() { fpt.storage.setJson("settings", S); }
function saveQueue() { fpt.storage.setJson("queue", queue); }
function mark(key) {
    sent[key] = Date.now();
    var keys = Object.keys(sent);
    if (keys.length > 500) keys.sort(function (a, b) { return sent[a] - sent[b]; }).slice(0, keys.length - 500).forEach(function (k) { delete sent[k]; });
    fpt.storage.setJson("sent", sent);
}

function two(n) { return (n < 10 ? "0" : "") + n; }
function fill(text, e, o) {
    var d = new Date();
    var vars = {
        "$username": (o && o.buyerName) || e.buyerName || "",
        "$order_id": e.orderId || "",
        "$order_link": "https://funpay.com/orders/" + (e.orderId || "") + "/",
        "$lot_name": (o && o.lotName) || "",
        "$date": two(d.getDate()) + "." + two(d.getMonth() + 1) + "." + d.getFullYear(),
        "$time": two(d.getHours()) + ":" + two(d.getMinutes()),
    };
    Object.keys(vars).sort(function (a, b) { return b.length - a.length; }).forEach(function (k) {
        text = text.split(k).join(vars[k]);
    });
    return text;
}

function deliver(kind, e, o) {
    var c = S[kind];
    var text = fill(c.text || "", e, o);
    var ok = c.image ? fpt.chat.sendWithImage(e.chatId, text, c.image, c.imageFirst) : (text.trim() ? fpt.chat.send(e.chatId, text) : false);
    if (ok) {
        mark(kind + ":" + e.orderId);
        fpt.app.log(titleOf(kind) + ": отправлено по заказу #" + e.orderId);
        if (c.markRead) fpt.chat.markRead(e.chatId);
    } else {
        fpt.app.log(titleOf(kind) + ": не отправилось по заказу #" + e.orderId + " (" + fpt.lastError() + ")");
    }
}

function titleOf(kind) {
    for (var i = 0; i < KINDS.length; i++) if (KINDS[i].key === kind) return KINDS[i].title;
    return kind;
}

fpt.on("onNewOrder", function (e) {
    if (e.iAmBuyer) return;
    if (S.paid.on && !sent["paid:" + e.orderId]) deliver("paid", e, null);
    if (S.remind.on && !sent["remind:" + e.orderId]) {
        queue = queue.filter(function (q) { return q.orderId !== e.orderId; });
        queue.push({ orderId: e.orderId, chatId: e.chatId, buyerName: e.buyerName, at: Date.now() + S.remindHours * 3600000 });
        saveQueue();
    }
});

fpt.on("onOrderConfirmed", function (e) {
    if (e.iAmBuyer) return;
    // напоминание больше не нужно
    queue = queue.filter(function (q) { return q.orderId !== e.orderId; });
    saveQueue();
    if (!S.review.on || sent["review:" + e.orderId]) return;
    setTimeout(function () {
        var o = fpt.orders.getDetails(e.orderId);
        if (o && (o.iAmBuyer || o.hasReview)) return;
        deliver("review", e, o);
    }, 5000);
});

fpt.on("onOrderRefunded", function (e) {
    queue = queue.filter(function (q) { return q.orderId !== e.orderId; });
    saveQueue();
});

// раз в минуту проверяем отложенные напоминания
setInterval(function () {
    if (!queue.length) return;
    var now = Date.now();
    var due = queue.filter(function (q) { return q.at <= now; });
    if (!due.length) return;
    queue = queue.filter(function (q) { return q.at > now; });
    saveQueue();
    if (!S.remind.on) return;
    due.forEach(function (q) {
        if (sent["remind:" + q.orderId]) return;
        var o = fpt.orders.getDetails(q.orderId);
        // напоминаем, только если заказ всё ещё оплачен и не закрыт
        if (!o || o.iAmBuyer || !/оплачен|paid|сплачен/i.test(o.status || "")) return;
        deliver("remind", q, o);
    });
}, 60000);

// ---------------- настройки
function render() {
    fpt.ui.setState("remindHours", String(S.remindHours));
    var kids = [
        { type: "Text", text: "Переменные: $username, $order_id, $order_link, $lot_name, $date, $time", color: "#7A8699" },
    ];
    KINDS.forEach(function (k) {
        var c = S[k.key];
        fpt.ui.setState(k.key + "_on", String(c.on));
        fpt.ui.setState(k.key + "_text", c.text);
        fpt.ui.setState(k.key + "_first", String(c.imageFirst));
        fpt.ui.setState(k.key + "_read", String(c.markRead));
        var card = { type: "Card", children: [
            { type: "Switch", text: k.title, stateKey: k.key + "_on", onChange: "window.saveAll()" },
            { type: "Text", text: k.hint, color: "#7A8699", fontSize: 12 },
        ] };
        if (c.on) {
            if (k.key === "remind") card.children.push({ type: "Slider", label: "Через сколько часов", min: 1, max: 48, stateKey: "remindHours", onChange: "window.saveAll()" });
            card.children.push({ type: "Input", label: "Текст", stateKey: k.key + "_text", singleLine: false, onChange: "window.saveAll()" });
            if (c.image) {
                card.children.push({ type: "Image", url: "file://" + c.image, height: 90, radius: 8 });
                card.children.push({ type: "Row", children: [
                    { type: "Button", text: "Другая картинка", onClick: "window.pick('" + k.key + "')", style: "flat" },
                    { type: "Button", text: "Убрать картинку", onClick: "window.unpick('" + k.key + "')", style: "flat" },
                ] });
                card.children.push({ type: "Checkbox", text: "Картинка перед текстом", stateKey: k.key + "_first", onChange: "window.saveAll()" });
            } else {
                card.children.push({ type: "Button", text: "Добавить картинку", onClick: "window.pick('" + k.key + "')", style: "flat" });
            }
            card.children.push({ type: "Checkbox", text: "Отмечать чат прочитанным", stateKey: k.key + "_read", onChange: "window.saveAll()" });
        }
        kids.push(card);
    });
    fpt.ui.setSlot(PLUGIN_SLOT_KEY, { type: "Column", children: kids });
}

window.saveAll = function () {
    var shape = KINDS.map(function (k) { return S[k.key].on; }).join();
    KINDS.forEach(function (k) {
        var c = S[k.key];
        c.on = fpt.ui.getState(k.key + "_on") === "true";
        if (c.on) {
            c.text = fpt.ui.getState(k.key + "_text");
            c.imageFirst = fpt.ui.getState(k.key + "_first") !== "false";
            c.markRead = fpt.ui.getState(k.key + "_read") === "true";
        }
    });
    S.remindHours = Math.max(1, Math.round(Number(fpt.ui.getState("remindHours")) || 4));
    save();
    if (shape !== KINDS.map(function (k) { return S[k.key].on; }).join()) render();
};
window.pick = function (key) {
    var p = fpt.app.pickImage();
    if (!p) return;
    S[key].image = p;
    save();
    render();
};
window.unpick = function (key) {
    S[key].image = "";
    save();
    render();
};

render();
