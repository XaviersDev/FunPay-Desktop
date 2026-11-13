// Обвязка плагинов FunPay Tools для компьютера. Выполняется перед кодом плагина.
// Все вызовы идут в Rust через __host(имя, JSON аргументов) и возвращают уже разобранный результат.
var window = globalThis;
window.window = globalThis;

(function () {
    var lastError = null;
    function call(name, args) {
        var raw = __host(name, JSON.stringify(args || []));
        var r = JSON.parse(raw);
        if (r.err !== undefined) {
            lastError = r.err;
            return undefined;
        }
        lastError = null;
        return r.ok;
    }
    // успех/провал как в Android-приложении: true или false
    function ok(name, args) {
        var r = call(name, args);
        return r === undefined ? false : (r === null ? true : r);
    }
    function val(name, args, fallback) {
        var r = call(name, args);
        return r === undefined ? fallback : r;
    }

    // ---------------- события
    var handlers = {};
    var fpt = {
        on: function (event, fn) {
            if (typeof fn !== "function") return;
            (handlers[event] = handlers[event] || []).push(fn);
        },
        off: function (event, fn) {
            if (!handlers[event]) return;
            handlers[event] = fn ? handlers[event].filter(function (f) { return f !== fn; }) : [];
        },
        lastError: function () { return lastError; },
    };
    window.__emit = function (event, data) {
        var list = handlers[event] || [];
        for (var i = 0; i < list.length; i++) {
            try { list[i](data); } catch (e) { call("app.log", ["Ошибка в обработчике " + event + ": " + e + (e && e.stack ? "\n" + e.stack : "")]); }
        }
    };

    // ---------------- таймеры
    var timers = {};
    var nextId = 1;
    function addTimer(fn, ms, repeat, args) {
        var id = nextId++;
        timers[id] = { fn: fn, args: args, repeat: repeat };
        __timer(id, Number(ms) || 0, repeat);
        return id;
    }
    window.setTimeout = function (fn, ms) { return addTimer(fn, ms, false, Array.prototype.slice.call(arguments, 2)); };
    window.setInterval = function (fn, ms) { return addTimer(fn, ms, true, Array.prototype.slice.call(arguments, 2)); };
    window.clearTimeout = window.clearInterval = function (id) {
        if (timers[id]) { delete timers[id]; __cleartimer(id); }
    };
    window.__fire = function (id) {
        var t = timers[id];
        if (!t) return;
        if (!t.repeat) delete timers[id];
        try {
            if (typeof t.fn === "function") t.fn.apply(null, t.args || []);
            else if (typeof t.fn === "string") (0, eval)(t.fn);
        } catch (e) { call("app.log", ["Ошибка в таймере: " + e + (e && e.stack ? "\n" + e.stack : "")]); }
    };

    // ---------------- консоль
    function fmt(args) {
        return Array.prototype.map.call(args, function (a) {
            if (typeof a === "string") return a;
            try { return JSON.stringify(a); } catch (e) { return String(a); }
        }).join(" ");
    }
    window.console = {
        log: function () { call("app.log", [fmt(arguments)]); },
        info: function () { call("app.log", [fmt(arguments)]); },
        warn: function () { call("app.log", ["⚠ " + fmt(arguments)]); },
        error: function () { call("app.log", ["✖ " + fmt(arguments)]); },
    };

    // ---------------- интерфейс
    var state = {};
    window.__uiSet = function (k, v) { state[k] = String(v); };
    fpt.ui = {
        setSlot: function (key, ui) { call("ui.setSlot", [ui, state]); },
        removeSlot: function (key) { call("ui.removeSlot", []); },
        getState: function (key) { return state[key] === undefined ? "" : state[key]; },
        setState: function (key, value) { state[key] = String(value); call("ui.state", [key, String(value)]); },
    };

    // ---------------- остальное API
    fpt.app = {
        log: function (m) { call("app.log", [fmt([m])]); },
        toast: function (m) { call("app.toast", [String(m)]); },
        notify: function (t, m) { call("app.notify", [String(t), String(m === undefined ? "" : m)]); },
        openUrl: function (u) { call("app.openUrl", [String(u)]); },
        pickImage: function () { return val("app.pickImage", [], null); },
        saveBase64Image: function (b64) { return val("app.saveBase64Image", [b64], null); },
        vibrate: function () {},
        updateWidgets: function () {},
        version: function () { return val("app.version", [], ""); },
        platform: "desktop",
    };
    fpt.storage = {
        get: function (k) { var v = call("storage.get", [String(k)]); return v === undefined ? null : v; },
        set: function (k, v) { call("storage.set", [String(k), typeof v === "string" ? v : JSON.stringify(v)]); },
        remove: function (k) { call("storage.remove", [String(k)]); },
        keys: function () { return val("storage.keys", [], []); },
        // удобные обёртки для объектов
        getJson: function (k, def) {
            var v = call("storage.get", [String(k)]);
            if (v === undefined || v === null) return def === undefined ? null : def;
            try { return JSON.parse(v); } catch (e) { return def === undefined ? null : def; }
        },
        setJson: function (k, obj) { call("storage.set", [String(k), JSON.stringify(obj)]); },
    };
    fpt.chat = {
        getList: function () { return val("chat.getList", [], []); },
        getHistory: function (id) { return val("chat.getHistory", [id], []); },
        getInfo: function (id) { return val("chat.getInfo", [id], null); },
        resolveUserId: function (id) { return val("chat.resolveUserId", [id], ""); },
        send: function (id, text) { return ok("chat.send", [id, String(text)]) === true; },
        sendWithImage: function (id, text, img, imgFirst) { return ok("chat.sendWithImage", [id, String(text || ""), img, imgFirst !== false]) === true; },
        create: function (userId, text) { return ok("chat.create", [String(userId), String(text)]) === true; },
        markRead: function (id) { call("chat.markRead", [id]); },
    };
    fpt.orders = {
        getDetails: function (id) { return val("orders.getDetails", [String(id)], null); },
        getSales: function (opts) { return val("orders.getSales", [opts || {}], []); },
        confirm: function (id) { return ok("orders.confirm", [String(id)]) === true; },
        refund: function (id) { return ok("orders.refund", [String(id)]) === true; },
        review: {
            reply: function (id, text, stars) { return ok("orders.reviewReply", [String(id), String(text), stars || 5]) === true; },
            write: function (id, text, stars) { return ok("orders.reviewWrite", [String(id), String(text), stars || 5]) === true; },
            remove: function (id) { return ok("orders.reviewDelete", [String(id)]) === true; },
        },
    };
    fpt.lots = {
        getMy: function () { return val("lots.getMy", [], []); },
        getFields: function (id) { return val("lots.getFields", [String(id)], null); },
        save: function (id, fields) { return ok("lots.save", [String(id), fields]) === true; },
        raiseAll: function () { return val("lots.raiseAll", [], null); },
        toggle: function (id, on) { return ok("lots.toggle", [String(id), !!on]) === true; },
        delete: function (id) { return ok("lots.delete", [String(id)]) === true; },
        changePrice: function (id, price) { return ok("lots.changePrice", [String(id), String(price)]) === true; },
        copy: function (id, node) { return ok("lots.copy", [String(id), String(node || "")]) === true; },
    };
    fpt.users = {
        getProfile: function () { return val("users.getProfile", [], null); },
        get: function (id) { return val("users.get", [String(id)], null); },
        getRmtHub: function () { return null; },
        getSales: function (opts) { return val("orders.getSales", [opts || {}], []); },
        getOrdersWith: function () { return []; },
        setAvatar: function () { return false; },
    };
    fpt.accounts = {
        getAll: function () { return val("accounts.getAll", [], []); },
        getActive: function () { return val("accounts.getActive", [], null); },
        switch: function () { return false; },
    };
    function req(method, url, body, headers, opts) {
        var r = call("network.request", [method, String(url), body === undefined ? null : body, headers === undefined ? null : headers, opts || null]);
        return r === undefined ? { code: 0, body: "", error: lastError } : r;
    }
    fpt.network = {
        get: function (url, headers, opts) { return req("GET", url, null, headers, opts); },
        post: function (url, body, headers, opts) { return req("POST", url, body, headers, opts); },
        request: function (method, url, body, headers, opts) { return req(method, url, body, headers, opts); },
        fetchImageBase64: function (url) { return val("network.fetchImageBase64", [String(url)], null); },
    };
    // запросы к FunPay с готовым csrf_token и заголовками сайта
    fpt.funpay = {
        csrf: function () { return val("funpay.csrf", [], ""); },
        userId: function () { return val("funpay.userId", [], ""); },
        request: function (method, path, form, headers) {
            var url = /^https?:/.test(path) ? path : "https://funpay.com/" + String(path).replace(/^\//, "");
            var h = headers || {};
            if (!h["X-Requested-With"]) h["X-Requested-With"] = "XMLHttpRequest";
            if (String(method).toUpperCase() !== "GET") {
                form = form || {};
                if (typeof form === "object" && form.csrf_token === undefined) form.csrf_token = fpt.funpay.csrf();
            }
            return req(String(method).toUpperCase(), url, String(method).toUpperCase() === "GET" ? null : form, h);
        },
    };
    fpt.ai = {
        ask: function (p) { return val("ai.ask", [String(p)], null); },
        rewrite: function (t, ctx) { return val("ai.rewrite", [String(t), ctx ? String(ctx) : ""], null); },
        translate: function (t) { return val("ai.translate", [String(t)], null); },
    };
    // функций Android-приложения, которых на компьютере нет: пустые заглушки, чтобы плагины не падали
    var none = function () { return null; };
    fpt.autodelivery = { getSettings: none, saveSettings: none, getFileCount: function () { return 0; }, readFile: function () { return ""; }, saveFile: none };
    fpt.dumper = { getSettings: none, saveSettings: none, runCycle: none };
    fpt.support = { getTickets: function () { return []; }, getDetails: none, create: none, reply: function () { return false; } };
    fpt.settings = {};
    ["Folders", "Labels", "ChatLabels", "BusyMode", "Commands", "Templates", "Reminders"].forEach(function (n) {
        fpt.settings["get" + n] = function () { return null; };
        fpt.settings["save" + n] = function () {};
    });
    window.fpt = fpt;
})();
