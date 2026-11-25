// The Personality window (tray Personality ▸ Customize…). The app sends its whole state as
// `persona://state` and answers `persona_state`: the mascot ({id, name}), its persona (the default
// preset and each preset's name, tagline, catchphrase, slider defaults, examples and gestures; null for
// a pack without one), the person's layer `user` (settings.persona: null = the preset's own value),
// `missing` (a preset the settings name that the pack no longer has) and the field caps.
// The page answers with persona_set {patch} (instant apply: text after a short pause or on leaving the
// field, sliders on release), persona_pick {preset}, persona_reset, persona_open_studio and persona_close.
// persona_set / persona_pick / persona_reset reply with the new state or refuse with a reason; a
// refused change stays on screen with Try again. The sample bubble is the preset's own example lines
// with the person's words filled in ({me}, {you}, {catchphrase}): no model call, nothing counted.
// All text goes in with textContent.
"use strict";
(function () {
  const T = window.__TAURI__;
  if (!T) return;
  const root = document.getElementById("root");
  const call = (cmd, args) => T.core.invoke(cmd, args || {});
  const fire = (cmd, args) => call(cmd, args).catch((e) => console.error(cmd, e));
  const SITUATIONS = [["greeting", "Hello"], ["done", "Done"], ["error", "Oops"], ["checkin", "Check-in"], ["disagree", "Push back"]];
  // the words are the ones `persona::prompt_text` gives each band of a slider (≤ 0.3, between, ≥ 0.7)
  const SLIDERS = [
    ["chattiness", "Chattiness", [["Brief", "one short sentence; check-ins mostly stay quiet"], ["Chatty", "a sentence or two"], ["Talkative", "up to three sentences; check-ins speak up more"]]],
    ["playfulness", "Playfulness", [["Straight", "plain answers, no bits"], ["Light", "a joke now and then"], ["Playful", "jokes, kind teasing and bits"]]],
    ["emotes", "Gestures", [["Rarely", "gestures only to say hello"], ["Sometimes", "a gesture now and then"], ["Often", "gestures a lot"]]],
  ];
  const TEXT = ["name", "call_me", "catchphrase"];
  const OWNED = ["name", "catchphrase", "chattiness", "playfulness", "emotes"];
  const PAUSE_MS = 500;

  let s = null; // the app's last state
  let sit = "greeting";
  let pending = {}; // field → value not yet sent (null = back to the preset's)
  let inflight = 0;
  let timer = 0;
  /** the bar: idle | saving | saved | error {text} | undo {text, prev} */
  let status = { kind: "idle" };
  let ui = null; // the mounted controls (one build per kind of page)
  let popOpen = false;

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  function btn(label, cls, onclick) {
    const b = el("button", "btn" + (cls ? " " + cls : ""), label);
    b.type = "button";
    b.addEventListener("click", onclick);
    return b;
  }
  const len = (t) => [...(t || "")].length;
  /** `persona::band`: low ≤ 0.3, high ≥ 0.7, mid between. */
  const band = (v) => (v <= 0.3 ? 0 : v >= 0.7 ? 2 : 1);

  // ---------- the effective persona ----------
  function preset() {
    const P = s.persona;
    const want = s.missing ? P.default : s.user.preset || P.default;
    return P.presets.find((p) => p.id === want) || P.presets.find((p) => p.id === P.default) || P.presets[0];
  }
  const own = (k) => (k in pending ? pending[k] : s.user[k]);
  function eff(k) {
    const v = own(k);
    if (v != null && v !== "") return v;
    const p = preset();
    return k in p ? p[k] : "";
  }
  const changed = (k) => own(k) != null;
  const customized = () => OWNED.some(changed);

  /** The example line with the person's words in: the shared rule in fill.js (`persona::fill`). */
  function fill(t) {
    return window.PersonaFill.fill(t, { me: eff("name"), you: eff("call_me") || null, catchphrase: eff("catchphrase") || null });
  }

  // ---------- saving ----------
  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(flush, PAUSE_MS);
  }
  /** Send what is pending. Resolves when it landed (or was refused: then it stays pending). */
  function flush() {
    clearTimeout(timer);
    const keys = Object.keys(pending);
    if (!keys.length) return Promise.resolve(true);
    const patch = pending;
    pending = {};
    inflight++;
    status = { kind: "saving" };
    renderBar();
    return call("persona_set", { patch }).then((st) => {
      inflight--;
      if (!inflight && status.kind === "saving") status = { kind: "saved" };
      apply(st);
      return true;
    }, (e) => {
      inflight--;
      // what was typed after it wins; the refused part waits for Try again
      pending = { ...patch, ...pending };
      status = { kind: "error", text: String(e) };
      update();
      return false;
    });
  }
  function put(k, v) {
    const p = s.persona && preset();
    // the preset's own value is no change at all
    if (v != null && p && OWNED.includes(k) && (typeof v === "number" ? Math.abs(v - p[k]) < 0.005 : v === p[k])) v = null;
    if (v === "" && k !== "notes") v = null;
    pending[k] = v;
  }
  async function pick(id) {
    if (s.user.preset === id && !s.missing && !customized()) return;
    const before = preset();
    if (!(await flush())) return;
    const was = customized();
    try {
      const st = await call("persona_pick", { preset: id });
      const now = st.persona.presets.find((p) => p.id === id);
      status = was && st.undo ? { kind: "undo", text: "Now " + now.name + " · your " + before.name + " changes were cleared", prev: st.undo } : { kind: "saved", text: "Now " + now.name };
      apply(st);
    } catch (e) {
      status = { kind: "error", text: String(e), retry: () => pick(id) };
      update();
    }
  }
  async function reset() {
    if (!(await flush())) return;
    const prev = { ...s.user };
    const p = preset();
    try {
      const st = await call("persona_reset");
      status = { kind: "undo", text: "Back to " + p.name + " as written", prev };
      apply(st);
    } catch (e) {
      status = { kind: "error", text: String(e), retry: reset };
      update();
    }
  }
  async function undo(prev) {
    const patch = {};
    for (const k of ["preset", ...OWNED]) patch[k] = prev[k];
    try {
      const st = await call("persona_set", { patch, undo: true });
      status = { kind: "saved", text: "Undone" };
      apply(st);
    } catch (e) {
      status = { kind: "error", text: String(e), retry: () => undo(prev) };
      update();
    }
  }

  // ---------- render ----------
  function apply(st) {
    if (!st) return;
    const kind = st.persona ? "persona" : "empty";
    s = st;
    root.setAttribute("aria-busy", "false");
    if (!ui || ui.kind !== kind) mount(kind);
    update();
  }

  function mount(kind) {
    root.replaceChildren();
    ui = { kind };
    const intro = el("header", "intro");
    ui.h1 = el("h1", null, "Personality");
    ui.sub = el("p");
    intro.append(ui.h1, ui.sub);
    root.append(intro);
    if (kind === "empty") {
      const card = el("section", "empty-card");
      ui.emptyTitle = el("b");
      card.append(ui.emptyTitle, el("p", null, "It talks in the app's plain voice. A personality comes with the mascot: add one in the Studio."));
      const r = el("div", "row");
      r.append(btn("Open Studio", "", () => fire("persona_open_studio")));
      card.append(r);
      root.append(card);
      return;
    }
    ui.notice = el("div", "note info");
    ui.notice.hidden = true;
    root.append(ui.notice);

    // presets: pick one
    ui.presets = el("div", "presets");
    ui.presets.setAttribute("role", "radiogroup");
    ui.presets.setAttribute("aria-label", "Preset");
    root.append(ui.presets);

    // the sample
    const h = el("h2", "group", "Sounds like");
    const info = el("button", "tip", "i");
    info.type = "button";
    info.setAttribute("aria-label", "About these examples");
    info.setAttribute("aria-expanded", "false");
    ui.pop = el("div", "pop");
    ui.pop.hidden = true;
    ui.pop.id = "about-sample";
    info.setAttribute("aria-controls", ui.pop.id);
    info.addEventListener("click", () => { popOpen = !popOpen; info.setAttribute("aria-expanded", String(popOpen)); ui.pop.hidden = !popOpen; });
    h.append(info);
    const stage = el("section", "stage");
    stage.setAttribute("aria-label", "Sample line");
    ui.seg = el("div", "seg");
    ui.seg.setAttribute("role", "group");
    ui.seg.setAttribute("aria-label", "Situation");
    ui.sits = SITUATIONS.map(([id, label]) => {
      const b = el("button", null, label);
      b.type = "button";
      b.addEventListener("click", () => { sit = id; renderSample(); });
      ui.seg.append(b);
      return [id, b];
    });
    const bub = el("div", "bubble");
    ui.text = el("div", "text");
    ui.text.setAttribute("aria-live", "polite");
    bub.append(ui.text);
    ui.said = el("div", "said");
    stage.append(ui.seg, bub, ui.said);
    root.append(h, ui.pop, stage);

    // make it yours
    root.append(el("h2", "group", "Make it yours"));
    ui.fields = {};
    const field = (k, label) => {
      const wrap = el("div");
      const lab = el("label", "lbl", label);
      lab.htmlFor = "f-" + k;
      const dot = el("span", "chg");
      dot.title = "Changed";
      dot.append(el("span", "vh", " (changed)"));
      lab.append(dot);
      const input = el("input", "field words");
      input.id = "f-" + k;
      input.dataset.field = k;
      input.autocomplete = "off";
      input.spellcheck = k === "catchphrase";
      input.maxLength = s.caps[k];
      input.addEventListener("input", () => { put(k, input.value.trim()); renderSample(); renderMarks(); schedule(); });
      input.addEventListener("change", () => flush());
      input.addEventListener("keydown", (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); flush(); } });
      wrap.append(lab, input);
      ui.fields[k] = { input, dot };
      return wrap;
    };
    const pair = el("div", "pair");
    pair.append(field("name", "Name"), field("call_me", "Calls you"));
    root.append(pair, field("catchphrase", "Catchphrase"));
    ui.fields.call_me.input.placeholder = "e.g. Sam";

    const sl = el("div", "sliders");
    ui.sliders = {};
    for (const [k, label, words] of SLIDERS) {
      const row = el("div", "slider");
      const name = el("span", null, label);
      const dot = el("span", "chg");
      dot.title = "Changed";
      dot.append(el("span", "vh", " (changed)"));
      name.append(dot);
      const r = el("input");
      Object.assign(r, { type: "range", min: 0, max: 100, step: 5 });
      r.dataset.field = k;
      r.setAttribute("aria-label", label);
      const word = el("span", "word");
      const show = () => {
        const [w, about] = words[band(r.value / 100)];
        word.textContent = w;
        word.title = about;
        r.setAttribute("aria-valuetext", w + ": " + about);
      };
      r.addEventListener("input", () => { show(); dot.hidden = false; });
      r.addEventListener("change", () => { put(k, Number(r.value) / 100); renderMarks(); flush(); });
      row.append(name, r, word);
      sl.append(row);
      ui.sliders[k] = { input: r, dot, show };
    }
    root.append(sl);

    // advanced: the notes
    const adv = el("details", "adv");
    const sum = el("summary", null, "Advanced");
    ui.advBadge = el("span", "badge");
    sum.append(ui.advBadge);
    const lab = el("label", "lbl", "Notes on its personality");
    lab.htmlFor = "f-notes";
    const ta = el("textarea", "field words");
    ta.id = "f-notes";
    ta.dataset.field = "notes";
    ta.rows = 5;
    ta.placeholder = "e.g. Keep it short before 10am. No jokes about my code reviews.";
    ta.setAttribute("aria-describedby", "notes-hint notes-count");
    ta.addEventListener("input", () => {
      if (len(ta.value) > s.caps.notes) delete pending.notes;
      else { put("notes", ta.value); schedule(); }
      renderNotes();
    });
    ta.addEventListener("change", () => { if (len(ta.value) <= s.caps.notes) flush(); });
    const r = el("div", "row");
    ui.notesHint = el("span", "hint");
    ui.notesHint.id = "notes-hint";
    ui.count = el("span", "count");
    ui.count.id = "notes-count";
    r.append(ui.notesHint, ui.count);
    adv.append(sum, lab, ta, r);
    ui.notes = ta;
    ui.adv = adv;
    root.append(adv);

    // the bar
    ui.bar = el("footer", "bar");
    ui.status = el("span", "status");
    ui.status.setAttribute("role", "status");
    ui.reset = btn("Reset", "", reset);
    ui.bar.append(ui.status, ui.reset);
    root.append(ui.bar);
  }

  function update() {
    if (!s || !ui) return;
    if (ui.kind === "empty") {
      ui.sub.textContent = "How " + s.pack.name + " talks to you.";
      ui.emptyTitle.textContent = s.pack.name + " has no personality yet";
      return;
    }
    const p = preset();
    ui.sub.textContent = "How " + eff("name") + " talks to you. Changes apply on its next line.";
    if (s.missing) {
      ui.notice.textContent = "The preset “" + s.missing + "” isn't in this mascot anymore. " + p.name + " stands in until you pick one.";
      ui.notice.hidden = false;
    } else ui.notice.hidden = true;
    // presets (rebuilt: four buttons; focus is put back on the one that had it)
    const focusId = document.activeElement && document.activeElement.dataset && document.activeElement.dataset.preset;
    ui.presets.replaceChildren(...s.persona.presets.map((q) => {
      const on = q.id === p.id && !s.missing;
      const b = el("button", "opt" + (on ? " on" : ""));
      b.type = "button";
      b.dataset.preset = q.id;
      b.setAttribute("role", "radio");
      b.setAttribute("aria-checked", String(on));
      // one tab stop for the group (the checked card, or the first when none is), arrows move inside it
      b.tabIndex = on || (s.missing && q === s.persona.presets[0]) ? 0 : -1;
      const t = el("span", "t");
      const name = el("b", null, q.name);
      if (on && customized()) name.append(el("span", "badge", "customized"));
      t.append(name, el("small", null, q.tagline));
      b.append(el("span", "dot"), t);
      b.setAttribute("aria-label", q.name + (on && customized() ? ", customized" : "") + ", " + q.tagline);
      b.addEventListener("click", () => pick(q.id));
      b.addEventListener("keydown", (e) => {
        const i = s.persona.presets.indexOf(q);
        const n = s.persona.presets.length;
        const to = { ArrowRight: 1, ArrowDown: 2, ArrowLeft: -1, ArrowUp: -2 }[e.key];
        if (!to) return;
        e.preventDefault();
        const next = ui.presets.children[(i + to + n * 2) % n];
        if (next) next.focus();
      });
      return b;
    }));
    if (focusId) { const f = ui.presets.querySelector('[data-preset="' + focusId + '"]'); if (f) f.focus(); }
    // fields: the app's values, except the one being typed in and what is still pending
    for (const k of TEXT) {
      const { input } = ui.fields[k];
      if (document.activeElement !== input && !(k in pending)) input.value = s.user[k] || "";
      if (k !== "call_me") input.placeholder = p[k];
    }
    for (const [k] of SLIDERS) {
      const { input, show } = ui.sliders[k];
      if (!(k in pending) && document.activeElement !== input) input.value = Math.round(eff(k) * 100);
      show();
    }
    if (document.activeElement !== ui.notes && !("notes" in pending)) ui.notes.value = s.user.notes || "";
    if (len(ui.notes.value) > s.caps.notes) ui.adv.open = true;
    ui.reset.textContent = "Reset to " + p.name;
    renderMarks();
    renderNotes();
    renderSample();
    renderBar();
    ui.pop.textContent = "These are " + p.name + "'s own example lines with your words filled in. The sliders and notes shape its real replies.";
  }

  function renderMarks() {
    for (const k of ["name", "catchphrase"]) ui.fields[k].dot.hidden = !changed(k);
    ui.fields.call_me.dot.hidden = true;
    for (const [k] of SLIDERS) ui.sliders[k].dot.hidden = !changed(k);
    const c = customized();
    ui.reset.disabled = !c;
    ui.reset.title = c ? "Name, catchphrase and sliders go back to " + preset().name + "'s own" : "Nothing changed from " + preset().name;
    const on = ui.presets.querySelector(".opt.on b");
    if (on) {
      const has = on.querySelector(".badge");
      if (c && !has) on.append(el("span", "badge", "customized"));
      if (!c && has) has.remove();
      const q = preset();
      on.closest(".opt").setAttribute("aria-label", q.name + (c ? ", customized" : "") + ", " + q.tagline);
    }
  }

  function renderNotes() {
    const n = len(ui.notes.value), cap = s.caps.notes;
    const over = n > cap;
    ui.count.textContent = n + " / " + cap;
    ui.count.classList.toggle("over", over);
    ui.notes.classList.toggle("over", over);
    ui.notes.setAttribute("aria-invalid", String(over));
    ui.notesHint.classList.toggle("err", over);
    ui.notesHint.textContent = over ? (n - cap) + " over the limit: notes save once they fit." : "Tone only: notes can't change what it's allowed to do.";
    ui.advBadge.hidden = !over && !n;
    ui.advBadge.textContent = over ? "too long" : "notes on";
    ui.advBadge.classList.toggle("rose", over);
    renderBar();
  }

  function renderSample() {
    const p = preset();
    for (const [id, b] of ui.sits) { b.classList.toggle("on", id === sit); b.setAttribute("aria-pressed", String(id === sit)); }
    ui.text.textContent = fill(p.examples[sit] || "");
    const g = p.gestures && p.gestures[sit];
    ui.said.replaceChildren(el("b", null, eff("name")));
    if (g) ui.said.append(document.createTextNode(" · " + (g.about || g.emote.replace(/_/g, " "))));
  }

  function renderBar() {
    if (!ui || ui.kind !== "persona") return;
    const st = ui.status;
    ui.bar.classList.toggle("err", status.kind === "error");
    st.className = "status" + (status.kind === "saved" || status.kind === "undo" ? " ok" : "");
    st.replaceChildren();
    switch (status.kind) {
      case "saving": st.textContent = "Saving…"; break;
      case "saved": st.textContent = status.text || "Saved"; break;
      case "undo": {
        st.textContent = status.text;
        const u = el("button", "linkish", "Undo");
        u.type = "button";
        u.addEventListener("click", () => undo(status.prev));
        st.append(u);
        break;
      }
      case "error": {
        st.append(el("b", null, "Couldn't save. "), document.createTextNode(status.text + ". Your change is still here."));
        st.append(btn("Try again", "small", () => (status.retry ? status.retry() : flush())));
        break;
      }
      default: st.textContent = ui.notes && len(ui.notes.value) > s.caps.notes ? "Everything else is saved. The notes wait until they fit." : "Saves as you go";
    }
  }

  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || e.isComposing) return;
    e.preventDefault();
    if (popOpen && ui && ui.pop) { popOpen = false; ui.pop.hidden = true; return; }
    flush().finally(() => fire("persona_close"));
  });
  T.event.listen("persona://state", (e) => apply(e.payload));
  call("persona_state").then((st) => { if (st && !s) apply(st); }).catch(() => {});
})();
