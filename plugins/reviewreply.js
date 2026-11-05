// @name Авто-ответы на отзывы
// @author FunPay Tools
// @version 1.0
// @description Отвечает на новые отзывы: свой текст для каждой оценки или ответ от ИИ. Переменные $username, $order_id, $lot_name, $stars.

var DEF = "Спасибо за отзыв, $username! 🤝";
var S = fpt.storage.getJson("settings", null) || {
    stars: { "1": true, "2": true, "3": true, "4": true, "5": true },
    texts: { "1": DEF, "2": DEF, "3": DEF, "4": DEF, "5": DEF },
    useAi: false,
    aiStyle: "",
    aiFallback: "Спасибо за отзыв! 🤝",
    aiLength: 3,
    overwrite: false,
    markRead: false,
};
var done = fpt.storage.getJson("done", []);   // заказы, на отзывы которых уже ответили

function save() { fpt.storage.setJson("settings", S); }

function two(n) { return (n < 10 ? "0" : "") + n; }
function fill(text, o) {
    var d = new Date();
    var vars = {
        "$username": o.buyerName || "",
        "$order_id": o.orderId || "",
        "$order_link": "https://funpay.com/orders/" + (o.orderId || "") + "/",
        "$lot_name": o.lotName || "",
        "$stars": String(o.reviewStars || ""),
        "$review_text": o.reviewText || "",
        "$date": two(d.getDate()) + "." + two(d.getMonth() + 1) + "." + d.getFullYear(),
        "$time": two(d.getHours()) + ":" + two(d.getMinutes()),
    };
    // длинные ключи первыми, чтобы $order_link не съел $order_id
    Object.keys(vars).sort(function (a, b) { return b.length - a.length; }).forEach(function (k) {
        text = text.split(k).join(vars[k]);
    });
    return text;
}

function aiText(o) {
    var prompt = "Ты продавец на FunPay и отвечаешь на отзыв покупателя. Напиши короткий тёплый ответ, не длиннее " + S.aiLength +
        " предложений, без кавычек и пояснений.\nОценка: " + o.reviewStars + " из 5\nТовар: " + (o.lotName || "товар") +
        "\nОтзыв: " + (o.reviewText || "без текста") + "\nИмя покупателя: " + (o.buyerName || "") +
        (S.aiStyle ? "\nСтиль и пожелания продавца: " + S.aiStyle : "") +
        "\nЕсли оценка низкая, извинись и предложи решить вопрос в чате.";
    for (var i = 0; i < 2; i++) {
        var r = fpt.ai.ask(prompt);
        if (r && r.trim()) return r.trim();
    }
    fpt.app.log("ИИ не ответил, отправляю запасной текст");
    return S.aiFallback;
}

function handle(e) {
    if (e.iAmBuyer) return;
    if (done.indexOf(e.orderId) >= 0) return;
    var o = fpt.orders.getDetails(e.orderId);
    if (!o) { fpt.app.log("Не открылся заказ #" + e.orderId); return; }
    if (o.iAmBuyer || !o.hasReview) return;
    if (o.reviewReply && !S.overwrite) return;
    var stars = String(o.reviewStars || 5);
    if (!S.stars[stars]) { fpt.app.log("Ответ на " + stars + "★ выключен, пропускаю #" + e.orderId); return; }
    o.buyerName = o.buyerName || e.buyerName;
    var text = S.useAi ? aiText(o) : (S.texts[stars] || "");
    if (!text.trim()) return;
    text = fill(text, o);
    if (fpt.orders.review.reply(o.orderId, text, 5)) {
        done.push(o.orderId);
        if (done.length > 300) done = done.slice(-300);
        fpt.storage.setJson("done", done);
        fpt.app.log("Ответил на отзыв " + stars + "★ к заказу #" + o.orderId);
        if (S.markRead && e.chatId) fpt.chat.markRead(e.chatId);
    } else {
        fpt.app.log("Не получилось ответить на отзыв #" + o.orderId + ": " + fpt.lastError());
    }
}

fpt.on("onNewReview", function (e) {
    // FunPay сохраняет отзыв не мгновенно
    setTimeout(function () { handle(e); }, 4000);
});

// ---------------- настройки
function render() {
    fpt.ui.setState("useAi", String(S.useAi));
    fpt.ui.setState("aiStyle", S.aiStyle);
    fpt.ui.setState("aiFallback", S.aiFallback);
    fpt.ui.setState("aiLength", String(S.aiLength));
    fpt.ui.setState("overwrite", String(S.overwrite));
    fpt.ui.setState("markRead", String(S.markRead));
    var kids = [
        { type: "Text", text: "Переменные: $username, $order_id, $order_link, $lot_name, $stars, $review_text, $date, $time", color: "#7A8699" },
        { type: "Spacer", size: 6 },
        { type: "Switch", text: "Отвечать с помощью ИИ", stateKey: "useAi", onChange: "window.saveAll()" },
    ];
    if (S.useAi) {
        kids.push({ type: "Card", children: [
            { type: "Input", label: "Стиль и пожелания (по желанию)", stateKey: "aiStyle", singleLine: false, onChange: "window.saveAll()" },
            { type: "Slider", label: "Длина ответа, предложений", min: 1, max: 6, stateKey: "aiLength", onChange: "window.saveAll()" },
            { type: "Input", label: "Если ИИ не ответил", stateKey: "aiFallback", singleLine: false, onChange: "window.saveAll()" },
        ] });
    }
    for (var n = 5; n >= 1; n--) {
        fpt.ui.setState("on" + n, String(!!S.stars[n]));
        fpt.ui.setState("text" + n, S.texts[n] || "");
        var card = { type: "Card", children: [
            { type: "Switch", text: "Отвечать на " + "★★★★★".slice(0, n) + " (" + n + ")", stateKey: "on" + n, onChange: "window.saveAll()" },
        ] };
        if (!S.useAi && S.stars[n]) card.children.push({ type: "Input", label: "Текст ответа", stateKey: "text" + n, singleLine: false, onChange: "window.saveAll()" });
        kids.push(card);
    }
    kids.push({ type: "Checkbox", text: "Заменять уже написанный ответ", stateKey: "overwrite", onChange: "window.saveAll()" });
    kids.push({ type: "Checkbox", text: "Отмечать чат прочитанным после ответа", stateKey: "markRead", onChange: "window.saveAll()" });
    fpt.ui.setSlot(PLUGIN_SLOT_KEY, { type: "Column", children: kids });
}

window.saveAll = function () {
    var wasAi = S.useAi;
    var was = JSON.stringify(S.stars);
    S.useAi = fpt.ui.getState("useAi") === "true";
    S.aiStyle = fpt.ui.getState("aiStyle");
    S.aiFallback = fpt.ui.getState("aiFallback");
    S.aiLength = Math.max(1, Math.round(Number(fpt.ui.getState("aiLength")) || 3));
    S.overwrite = fpt.ui.getState("overwrite") === "true";
    S.markRead = fpt.ui.getState("markRead") === "true";
    for (var n = 1; n <= 5; n++) {
        S.stars[n] = fpt.ui.getState("on" + n) === "true";
        if (S.stars[n]) S.texts[n] = fpt.ui.getState("text" + n) || S.texts[n] || "";
    }
    save();
    // перерисовываем, только если поменялся набор полей
    if (wasAi !== S.useAi || was !== JSON.stringify(S.stars)) render();
};

render();
