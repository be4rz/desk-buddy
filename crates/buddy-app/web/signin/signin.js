// The Sign in… window (design `## accounts`). The app sends its whole state as `signin://state`
// (providers and status, the sign-in in progress, the keychain error, the last outcome; never a
// secret). The page answers with signin_state, signin_save_key, signin_start, signin_answer,
// signin_cancel, signin_remove, signin_reopen and signin_close. All text goes in with textContent.
// A pasted key or code is sent once and its field cleared at once; the page never keeps it.
"use strict";
(function () {
  const T = window.__TAURI__;
  const root = document.getElementById("root");
  const invoke = (cmd, args) => T.core.invoke(cmd, args || {}).catch((e) => console.error(cmd, e));
  let state = null;
  /** The provider whose "Add key" form is open (page-only). */
  let keyFor = null;
  /** Field text to carry over the render in progress (label → text). */
  let keepFor = {};

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  function btn(label, cls, onclick, disabled) {
    const b = el("button", "btn" + (cls ? " " + cls : ""), label);
    b.type = "button";
    if (disabled) { b.disabled = true; if (typeof disabled === "string") b.title = disabled; }
    b.addEventListener("click", onclick);
    return b;
  }
  function note(text, ok) {
    return el("div", "note " + (ok ? "ok" : "err"), text);
  }
  /** A secret field: sent with Enter or the button, then emptied. */
  function secretForm(label, placeholder, type, submitLabel, onsubmit, hint) {
    const box = el("div", "form");
    const lbl = el("label", "lbl", label);
    const input = el("input", "field");
    input.type = type;
    input.autocomplete = "off";
    input.spellcheck = false;
    input.setAttribute("aria-label", label);
    if (placeholder) input.placeholder = placeholder;
    const go = btn(submitLabel, "primary", send, true);
    function send() {
      const v = input.value.trim();
      input.value = "";
      go.disabled = true;
      if (v) onsubmit(v);
    }
    input.addEventListener("input", () => { go.disabled = !input.value.trim(); });
    input.addEventListener("keydown", (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); send(); } });
    const row = el("div", "row");
    row.append(hint ? el("span", "hint", hint) : el("span"), go);
    box.append(lbl, input, row);
    setTimeout(() => input.focus(), 0);
    return box;
  }

  function statusText(p) {
    const m = p.model ? " · " + p.model : "";
    switch (p.status) {
      case "signed_in": return "Signed in" + m;
      case "key_saved": return "Key saved" + m;
      case "session": return "Dev key for this run only" + m;
      default: return p.detail;
    }
  }

  function flowText(f) {
    switch (f.phase) {
      case "starting": return "starting the sign-in…";
      case "browser": return "opening your browser…";
      case "code": return "waiting for the code from your browser…";
      case "device": return "waiting for you to enter the code…";
      case "select": return "choose an option below";
      case "error": return "the sign-in didn't work";
      default: return f.progress || "";
    }
  }

  function providerRow(p, s) {
    const f = s.flow;
    const mine = f && f.providerId === p.id;
    const busy = f && f.phase !== "error";
    const row = el("div", "provider" + (mine ? " active" : ""));
    row.dataset.provider = p.id;
    const left = el("div");
    left.append(el("b", null, p.name), el("small", null, mine ? (f.progress && f.phase !== "error" ? f.progress : flowText(f)) : statusText(p)));
    const acts = el("div", "acts");
    const oauth = p.methods.includes("oauth"), key = p.methods.includes("api_key");
    if (mine && busy) {
      acts.append(btn("Cancel", "", () => invoke("signin_cancel")));
    } else if (p.status === "signed_in") {
      acts.append(btn("Sign out", "", () => invoke("signin_remove", { providerId: p.id })));
    } else if (p.status === "key_saved") {
      acts.append(btn("Remove", "", () => invoke("signin_remove", { providerId: p.id })));
    } else if (p.status === "session") {
      // a dev key from the environment: nothing stored to remove
    } else {
      if (oauth) {
        const why = busy ? "another sign-in is in progress" : (!s.canOAuth ? "the agent isn't running" : "");
        acts.append(btn("Sign in", s.firstOAuth === p.id ? "primary" : "", () => { keyFor = null; invoke("signin_start", { providerId: p.id }); }, why || false));
      }
      if (key) {
        acts.append(btn("Add key", oauth ? "" : (keyFor === p.id ? "primary" : ""), () => { keyFor = keyFor === p.id ? null : p.id; render(state); }));
      }
    }
    row.append(left, acts);
    return row;
  }

  function flowBox(s) {
    const f = s.flow;
    const box = el("div", "flow");
    if (f.url) {
      // the sign-in page is shown as text too: what opened is never a surprise
      const u = el("div");
      u.append(el("span", null, (f.instructions || "Sign in in your browser.") + " "));
      box.append(u, el("div", "url", f.url));
      if (f.phase !== "error") {
        const r = el("div", "row");
        r.append(btn("Open again", "", () => invoke("signin_reopen")));
        box.append(r);
      }
    }
    if (f.phase === "device") {
      const line = el("div");
      line.style.marginTop = "10px";
      line.append(el("span", null, f.name + ": enter this code at "), el("span", "url", f.verificationUri || ""));
      box.append(line, el("div", "code-box", f.userCode || ""));
      const r = el("div", "row");
      r.append(btn("Open again", "", () => invoke("signin_reopen")));
      box.append(r);
    }
    if (f.phase === "code") {
      box.append(secretForm(f.message || "Paste the code shown after sign-in", f.placeholder || "", "text", "Continue", (v) => invoke("signin_answer", { value: v })));
    }
    if (f.phase === "select") {
      box.append(el("label", "lbl", f.message || "Choose"));
      const r = el("div", "row");
      r.style.justifyContent = "flex-start";
      for (const o of f.options || []) r.append(btn(o.label, "", () => invoke("signin_answer", { value: o.id })));
      box.append(r);
    }
    if (f.phase === "error") {
      box.append(note(f.error || "The sign-in didn't work."));
      const r = el("div", "row");
      r.append(btn("Close", "", () => invoke("signin_close")), btn("Start again", "primary", () => invoke("signin_start", { providerId: f.providerId }), !s.canOAuth && "the agent isn't running"));
      box.append(r);
    }
    return box;
  }

  // which field a typed text belongs to: its label, and for a sign-in flow's field the flow's
  // step too (a restarted sign-in or a new prompt with the same label starts empty)
  function fieldKey(f, s) {
    const fl = f.closest(".flow") && s && s.flow;
    return f.getAttribute("aria-label") + (fl ? "|" + [fl.providerId, fl.phase, fl.url, fl.message].join("|") : "");
  }

  function render(s) {
    // a state update (progress, a notice, the window opened again) must not wipe what the person
    // is typing: carry each field's text over to the same field rebuilt, in the page only
    const typing = {};
    for (const f of root.querySelectorAll("input.field")) if (f.value) typing[fieldKey(f, state)] = f.value;
    state = s;
    root.replaceChildren();
    if (!s) return;
    keepFor = typing;
    const providers = s.providers || [];
    s.firstOAuth = (providers.find((p) => p.status === "none" && p.methods.includes("oauth")) || {}).id;
    if (s.keychain) root.append(note("Can't read the keychain: " + s.keychain + ". desk-buddy needs “Always Allow” for its keychain item."));
    const set = providers.filter((p) => p.status !== "none");
    const rest = providers.filter((p) => p.status === "none");
    for (const p of set) root.append(providerRow(p, s));
    if (set.length && rest.length) root.append(el("div", "sep"));
    for (const p of rest) root.append(providerRow(p, s));
    if (!rest.length) root.append(el("div", "empty", "No other providers yet."));
    const kp = keyFor && providers.find((p) => p.id === keyFor && p.status === "none");
    if (kp) {
      root.append(secretForm("API key for " + kp.name, "", "password", "Save", (v) => { keyFor = null; invoke("signin_save_key", { providerId: kp.id, key: v }); },
        "Stored in your login keychain. Never shown to the agent or its commands."));
    } else {
      keyFor = null;
    }
    if (s.flow) root.append(flowBox(s));
    if (s.notice) root.append(note(s.notice.text, s.notice.ok));
    // a failed sign-in's own row already has Close (prototype #signin-error: Close + Start again)
    if (!(s.flow && s.flow.phase === "error")) {
      const r = el("div", "row");
      r.append(btn("Close", "", () => invoke("signin_close")));
      root.append(r);
    }
    for (const f of root.querySelectorAll("input.field")) {
      const v = keepFor[fieldKey(f, s)];
      if (v) {
        f.value = v;
        f.dispatchEvent(new Event("input"));
      }
    }
    keepFor = {};
  }

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") { e.preventDefault(); invoke("signin_close"); }
  });
  T.event.listen("signin://state", (e) => render(e.payload));
  T.core.invoke("signin_state").then((s) => { if (s && !state) render(s); }).catch(() => {});
})();
