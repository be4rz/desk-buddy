// The speech bubble page (design §4.8). The app sends `bubble://show` (a BubbleModel) and
// `bubble://hide`; the page answers with prompt_submit, card_answer, bubble_dismiss, bubble_size,
// bubble_retry and reminder_snooze. All text goes in with textContent (plain text, never HTML).
"use strict";
(function () {
  const T = window.__TAURI__;
  const root = document.getElementById("root");
  const invoke = (cmd, args) => T.core.invoke(cmd, args || {}).catch((e) => console.error(cmd, e));
  let model = null;

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  function btn(label, cls, onclick, disabled) {
    const b = el("button", "btn" + (cls ? " " + cls : ""), label);
    b.type = "button";
    if (disabled) { b.disabled = true; b.title = typeof disabled === "string" ? disabled : ""; }
    b.addEventListener("click", onclick);
    return b;
  }
  function meta(left, right) {
    const m = el("div", "meta");
    m.append(left, right || el("span"));
    return m;
  }
  function close(label, glyph) {
    const x = el("button", "x", glyph || "✕");
    x.type = "button";
    x.setAttribute("aria-label", label || "close");
    x.addEventListener("click", () => invoke("bubble_dismiss"));
    return x;
  }
  function pill(text, cls) {
    return el("span", "pill" + (cls ? " " + cls : ""), text);
  }

  function prompt(b) {
    b.append(meta(el("span", null, "ask the bear"), close()));
    const ta = el("textarea");
    ta.setAttribute("aria-label", "prompt");
    ta.placeholder = "Ask me anything, or tell me to remind you…";
    const ask = btn("Ask", "primary", send, true);
    function send() {
      const t = ta.value.trim();
      if (t) invoke("prompt_submit", { text: t });
    }
    ta.addEventListener("input", () => { ask.disabled = !ta.value.trim(); });
    ta.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); }
    });
    const row = el("div", "row");
    row.append(el("span", "hint", "⏎ send · Esc close"), ask);
    b.append(ta, row);
    setTimeout(() => ta.focus(), 0);
  }

  function speech(b, m) {
    const text = el("div", "text", m.text || "");
    if (m.streaming) {
      b.append(meta(pill(m.text ? "thinking → talking" : "thinking…", "teal"), close("stop", "■")));
      text.append(el("span", "caret"));
    } else {
      b.append(meta(el("span", null, m.emotion ? "buddy say · " + m.emotion : "the bear"), close()));
    }
    b.append(text);
    if (m.streaming) requestAnimationFrame(() => { text.scrollTop = text.scrollHeight; });
  }

  function card(b, m) {
    const c = m.card || {};
    const tainted = !!c.taintNote;
    b.append(meta(pill("needs your OK", tainted ? "rose" : ""), el("span", null, tainted ? "web content read" : (c.index || 1) + " of " + (c.total || 1))));
    b.append(el("div", "card-title", c.title || "Allow this?"));
    if (c.detail) b.append(el("div", "text", c.detail));
    if (c.command) b.append(el("div", "cmd", c.command));
    if (tainted) b.append(el("div", "note taint", c.taintNote));
    const answer = (a) => () => invoke("card_answer", { callId: c.callId, answer: a });
    const row = el("div", "row");
    row.append(
      btn("Deny", "danger", answer("deny")),
      btn("Always", "", answer("allow_always"), !c.canAlways && (tainted ? "off while tainted" : "not offered for this command")),
      btn("Allow once", "primary", answer("allow")),
    );
    b.append(row);
  }

  function reminder(b, m) {
    // "set N min ago" only when the app knows (reminders stored before p4 don't say)
    const ago = typeof m.setMinAgo === "number" ? (m.setMinAgo < 1 ? " · set just now" : " · set " + m.setMinAgo + " min ago") : "";
    b.append(meta(pill("⏰ reminder" + ago, "teal"), close("dismiss")));
    b.append(el("div", "text", m.text || ""));
    const row = el("div", "row");
    row.append(
      btn("Snooze 10 min", "", () => invoke("reminder_snooze", { id: m.id || "", minutes: 10 })),
      btn("Done", "primary", () => invoke("bubble_dismiss")),
    );
    b.append(row);
  }

  function error(b, m) {
    const e = m.error || {};
    const signIn = e.action === "sign_in";
    b.append(meta(pill("couldn't answer", "rose"), close()));
    b.append(el("div", "text", e.reason === "keychain" ? "I can't read my keychain, so I can't reach a model." : signIn ? "I can't answer until you sign in to a model." : "The model didn't respond."));
    b.append(el("div", "note err", (e.reason || "error") + ": " + (e.message || "")));
    const row = el("div", "row");
    row.append(
      btn("Close", "", () => invoke("bubble_dismiss")),
      btn(signIn ? "Sign in…" : "Try again", "primary", () => invoke("bubble_retry")),
    );
    b.append(row);
  }

  const modes = { prompt, speech, card, reminder, error };

  function render(m) {
    model = m;
    root.replaceChildren();
    const b = el("div", "bubble");
    b.dataset.mode = m.mode;
    (modes[m.mode] || speech)(b, m);
    root.append(b);
    report(true); // the app shows a new model once it knows its size
  }

  function clear() {
    model = null;
    root.replaceChildren();
    last = "";
  }

  let last = "";
  // Measured right away (reading the rect lays the page out), not in a frame callback: the window
  // is hidden until the app has this size, and a hidden page may get no animation frames.
  function report(force) {
    const r = root.getBoundingClientRect();
    const w = Math.ceil(r.width) || 306, h = Math.ceil(r.height) || 1;
    if (!force && w + "x" + h === last) return;
    last = w + "x" + h;
    invoke("bubble_size", { w, h });
  }

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && model && model.mode !== "card") { e.preventDefault(); invoke("bubble_dismiss"); }
  });
  new ResizeObserver(() => { if (model) report(); }).observe(root);

  T.event.listen("bubble://show", (e) => render(e.payload));
  T.event.listen("bubble://hide", () => clear());
  report(true); // loaded: the app resends what's shown
})();
