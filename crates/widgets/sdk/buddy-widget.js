// desk-buddy widget SDK (design §4.9), served at /sdk/buddy-widget.js by the widget server.
// Talks to the app over Tauri IPC only: a runtime capability lets the window `widget-<id>` at this
// exact loopback origin call the widget_* commands, nothing else. Widget pages get no host token.
//
//   const w = BuddyWidget.connect();
//   w.onAction(name, async (args) => result);   // result JSON goes back as the HTTP reply
//   w.setState(obj);                           // shown in `buddy widget list --json`
//   w.requestHide(); w.requestShow(); w.requestClose();
//   w.on(kind, fn);                            // app → page messages other than actions (e.g. "state")
//
// A handler may throw `{ code, message }` (or an Error) to refuse: the CLI exits 1 with the message.
// Any element with `data-widget-drag` moves the window (buttons inside it don't).
(function () {
  "use strict";
  var handlers = Object.create(null);
  var listeners = Object.create(null);
  var api = null;

  function invoke(cmd, args) {
    var t = window.__TAURI_INTERNALS__;
    if (!t || typeof t.invoke !== "function") return Promise.reject(new Error("no app IPC (a plain browser?)"));
    return t.invoke(cmd, args || {});
  }

  function errorOf(e) {
    if (e && typeof e === "object") return { code: String(e.code || "widget_error"), message: String(e.message || e.code || "failed") };
    return { code: "widget_error", message: String(e) };
  }

  // The app calls this through evaluateJavaScript; never from the page itself.
  Object.defineProperty(window, "__buddyWidget", {
    value: Object.freeze({
      dispatch: function (msg) {
        if (!msg || typeof msg !== "object") return;
        if (msg.kind === "action") {
          var h = handlers[msg.name];
          Promise.resolve()
            .then(function () {
              if (!h) throw { code: "no_action", message: "this widget has no action “" + msg.name + "”" };
              return h(msg.args || {});
            })
            .then(
              function (v) { return { ok: true, value: v === undefined ? {} : v }; },
              function (e) { var x = errorOf(e); return { ok: false, code: x.code, message: x.message }; }
            )
            .then(function (reply) { return invoke("widget_reply", { req: String(msg.req), reply: reply }); })
            .catch(function () {});
          return;
        }
        (listeners[msg.kind] || []).forEach(function (f) {
          try { f(msg.data); } catch (_) {}
        });
      },
    }),
  });

  window.BuddyWidget = Object.freeze({
    connect: function () {
      if (api) return api;
      api = Object.freeze({
        onAction: function (name, fn) { handlers[name] = fn; },
        on: function (kind, fn) { (listeners[kind] = listeners[kind] || []).push(fn); },
        setState: function (obj) { return invoke("widget_state", { state: obj || {} }).catch(function () {}); },
        requestHide: function () { return invoke("widget_visibility", { op: "hide" }).catch(function () {}); },
        requestShow: function () { return invoke("widget_visibility", { op: "show" }).catch(function () {}); },
        requestClose: function () { return invoke("widget_visibility", { op: "close" }).catch(function () {}); },
        control: function (op) { return invoke("widget_control", { op: String(op) }); },
        ipc: function () { return !!window.__TAURI_INTERNALS__; },
      });
      document.addEventListener("mousedown", function (e) {
        var t = e.target;
        if (e.button !== 0 || !t || !t.closest || !t.closest("[data-widget-drag]") || t.closest("button,a,input,[data-no-drag]")) return;
        invoke("widget_drag").catch(function () {});
      });
      // after the page's own script registered its handlers
      setTimeout(function () { invoke("widget_ready").catch(function () {}); }, 0);
      return api;
    },
  });
})();
