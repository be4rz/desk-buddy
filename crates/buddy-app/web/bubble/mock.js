// Browser-only parity mock: `index.html?mock=<state>` installs a fake `__TAURI__` and shows one of
// the prototype's bubble states. Inside the app (`__TAURI__` present) or without ?mock it does nothing.
// Commands the page invokes are recorded in `window.__calls`.
"use strict";
(function () {
  const state = new URLSearchParams(location.search).get("mock");
  if (window.__TAURI__ || !state) return;
  const card = { callId: "c1", title: "Run a shell command", detail: "It wants to download a file into your work folder.", command: "curl -sL https://example.com/notes.txt -o notes.txt", canAlways: true, index: 1, total: 1 };
  const models = {
    prompt: { mode: "prompt" },
    "empty-prompt": { mode: "prompt" },
    streaming: { mode: "speech", streaming: true, text: "After lunch you have design review at 14:00 and a 1:1 at 15:30. Want a nudge ten min" },
    speech: { mode: "speech", streaming: false, emotion: "happy", text: "Build passed on main. Nice one!" },
    long: { mode: "speech", streaming: false, text: Array.from({ length: 12 }, (_, i) => "Line " + (i + 1) + " of a long answer.").join("\n") },
    card: { mode: "card", card },
    "card-queued": { mode: "card", card: { ...card, callId: "c3", title: "Read a file", detail: "", command: "notes.txt", index: 2, total: 3 } },
    "card-tainted": { mode: "card", card: { callId: "c2", title: "Send data out", detail: "", command: "buddy browser type e7 \"my order number…\"", canAlways: false, index: 1, total: 1, taintNote: "This chat read a web page, so “Always” is off until you clear it (tray → Clear web taint)." } },
    reminder: { mode: "reminder", id: "r_1", text: "Stretch and refill your water.", setMinAgo: 20 },
    "reminder-old": { mode: "reminder", id: "r_0", text: "Stretch and refill your water." },
    error: { mode: "error", error: { reason: "model_error", message: "529 overloaded. Nothing ran.", action: "retry" } },
    "error-no-model": { mode: "error", error: { reason: "no_model", message: "There's no model to talk to yet. Sign in to one first.", action: "sign_in" } },
    html: { mode: "speech", streaming: false, text: "<img src=x onerror=alert(1)><b>not bold</b>" },
  };
  const listeners = {};
  window.__calls = [];
  window.__TAURI__ = {
    core: { invoke: (cmd, args) => { window.__calls.push([cmd, args]); return Promise.resolve(null); } },
    event: { listen: (name, cb) => { listeners[name] = cb; return Promise.resolve(() => {}); } },
  };
  window.__mockShow = (m) => listeners["bubble://show"] && listeners["bubble://show"]({ payload: m });
  if (new URLSearchParams(location.search).get("side") === "below") document.documentElement.dataset.side = "below";
  document.documentElement.style.background = "#eae9ec";
  addEventListener("DOMContentLoaded", () => setTimeout(() => {
    const m = models[state];
    if (m) window.__mockShow(m);
    if (state === "prompt") {
      const ta = document.querySelector("textarea");
      ta.value = "what's on my calendar after lunch?";
      ta.dispatchEvent(new Event("input"));
    }
  }, 0));
})();
