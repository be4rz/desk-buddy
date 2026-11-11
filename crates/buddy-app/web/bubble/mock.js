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
    "prompt-busy": { mode: "prompt", busy: true, recall: true },
    "prompt-recall": { mode: "prompt", recall: true },
    thinking: { mode: "speech", streaming: true, text: "" },
    working: { mode: "speech", streaming: true, text: "", activity: "Running a command" },
    streaming: { mode: "speech", streaming: true, text: "After lunch you have design review at 14:00 and a 1:1 at 15:30. Want a nudge ten min" },
    speech: { mode: "speech", streaming: false, emotion: "happy", text: "Build passed on main. Nice one!" },
    answer: { mode: "speech", streaming: false, reply: true, text: "After lunch you have **two things**:\n\n- Design review at `14:00`\n- A 1:1 at 15:30\n\nWant a nudge *ten minutes* before each? See [the calendar](https://example.com/cal)." },
    code: { mode: "speech", streaming: false, reply: true, text: "Run this:\n```sh\ncargo test -p buddy-app\n```\n1. Wait for it\n2. Read the output" },
    long: { mode: "speech", streaming: false, reply: true, text: Array.from({ length: 16 }, (_, i) => "Line " + (i + 1) + " of a long answer.").join("\n") },
    setup: { mode: "setup" },
    card: { mode: "card", card },
    "card-queued": { mode: "card", card: { ...card, callId: "c3", title: "Read a file", detail: "", command: "notes.txt", index: 2, total: 3 } },
    "card-once": { mode: "card", card: { ...card, callId: "c4", canAlways: false } },
    "card-tainted": { mode: "card", card: { callId: "c2", title: "Send data out", detail: "", command: "buddy browser type e7 \"my order number…\"", canAlways: false, index: 1, total: 1, taintNote: "I read a web page in this chat, so “Always allow” is off and I ask before anything risky. To switch that off: menu-bar bear → Reset web caution." } },
    reminder: { mode: "reminder", id: "r_1", text: "Stretch and refill your water.", setMinAgo: 20 },
    "reminder-old": { mode: "reminder", id: "r_0", text: "Stretch and refill your water." },
    error: { mode: "error", error: { reason: "model_error", message: "529 overloaded. Nothing ran.", action: "retry" } },
    "error-unprompted": { mode: "error", error: { reason: "model_error", message: "529 overloaded. Nothing ran." } },
    "error-no-model": { mode: "error", error: { reason: "no_model", message: "There's no model to talk to yet. Sign in to one first.", action: "sign_in" } },
    "error-key": { mode: "error", error: { reason: "model_error", message: "401 invalid x-api-key", action: "sign_in" } },
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
