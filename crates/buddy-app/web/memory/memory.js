// The "What I remember" window (tray What I remember…, or `buddy memory open`). The app sends its whole
// state as `memory://state` and answers `memory_state`: `enabled` (the switch), `error` (the store didn't
// open: {code, message, file}, groups then empty), `phrase` (Forget everything's typed confirm), the
// three groups of live rows (about = profile, help = preference, promises = promise; each row {id, kind,
// text, key, label, value, since "YYYY-MM", source window|chat|bear|guess, disputed}), the open
// `disputes` ({id, label, value, text, live {id, value, source}}), `counts` {live, disputes, hidden, days},
// and History (p2): `history` = up to 14 days newest first, each {day "YYYY-MM-DD", items: [row + kind
// episode|insight, sources N]}, with `historyMore` when older days exist (memory_history {before} pages back
// 14 at a time: {days, more}).
// The page answers with memory_add {kind, text}, memory_edit {id, text, value?}, memory_forget {id, hard},
// memory_resolve {id, keep: "new"|"old"}, memory_forget_all {confirm}, memory_set_enabled {on},
// memory_history {before} and memory_close. A refusal is one of `not allowed|secret|empty|too_long|not_found|bad_phrase|store`; a
// refused change stays on screen (the row as it was, a form with its text) and the bar says why, with
// Try again. All text goes in with textContent.
"use strict";
(function () {
  const T = window.__TAURI__;
  if (!T) return;
  const root = document.getElementById("root");
  const call = (cmd, args) => T.core.invoke(cmd, args || {});
  const GROUPS = [["about", "About you", "profile"], ["help", "How you like help", "preference"], ["promises", "Things I said I'd do", "promise"]];
  const SOURCE = { window: "you added this", chat: "you told me", bear: "I noted this", guess: "I picked this up" };
  const ASKED = { window: "You said", chat: "You said", bear: "I noted", guess: "I guessed" };
  const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  const FORM_ERR = {
    secret: "That looks like a password or key, so I won't keep it. Keys belong in Accounts…, not in my memory.",
    empty: "Write something first.",
    too_long: "That's too long for one thing: keep it under 200 characters (a value under 100).",
    stale_text: "The sentence still says the old value: change it too.",
  };
  const NARROW = window.matchMedia("(max-width: 400px)");

  let s = null; // the app's last state
  /** the open form: {group, kind, id (null = a new row), text, value, keyed, label, was, orig, err, busy} */
  let form = null;
  /** the open row menu: {id, step: "menu" | "confirm"} */
  let menu = null;
  /** the Forget everything dialog: {typed, err, busy} */
  let dlg = null;
  /** the bar: idle | ok {text} | err {text, retry} */
  let status = { kind: "idle" };
  let inflight = false;
  let restore = null; // data-k keys to focus after the next render, the first that can take it
  /** History days loaded with Show older days (after the state's own 14): {days, more, busy} */
  let older = { days: [], more: false, busy: false };
  /** day → open, for the days the person opened or closed (the newest starts open) */
  const dayOpen = {};

  const bar = root.querySelector(".bar");
  const stat = bar.querySelector(".status");
  stat.setAttribute("role", "status");
  const faBtn = bar.querySelector(".btn");
  faBtn.dataset.k = "forget-all";
  faBtn.addEventListener("click", openDlg);

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  function btn(label, cls, k, onclick) {
    const b = el("button", "btn" + (cls ? " " + cls : ""), label);
    b.type = "button";
    if (k) b.dataset.k = k;
    b.addEventListener("click", onclick);
    return b;
  }
  const txt = (t) => document.createTextNode(t);
  const quoted = (t) => "“" + t + "”";

  // ---------- the state ----------
  function rows(name) {
    return (s && s.groups && s.groups[name]) || [];
  }
  function findRow(id) {
    for (const [name] of GROUPS) {
      const r = rows(name).find((x) => x.id === id);
      if (r) return r;
    }
    for (const d of days()) {
      const r = (d.items || []).find((x) => x.id === id);
      if (r) return r;
    }
    return null;
  }
  /** Every History day on screen, newest first: the state's, then the older ones loaded. */
  function days() {
    const own = (s && s.history) || [];
    const last = own.length ? own[own.length - 1].day : null;
    return own.concat(last ? older.days.filter((d) => d.day < last) : older.days);
  }
  function since(v) {
    const m = /^(\d{4})-(\d{2})/.exec(v || "");
    return m ? "since " + MONTHS[Number(m[2]) - 1] + " " + m[1] : "";
  }
  function meta(r) {
    return [since(r.since), SOURCE[r.source] || ""].filter(Boolean).join(" · ");
  }
  function plural(n, one) {
    return n + " " + (n === 1 ? one : one + "s");
  }

  function apply(st) {
    if (!st) return;
    s = st;
    if (form && form.id != null && !findRow(form.id)) form = null;
    if (menu && !findRow(menu.id)) menu = null;
    if (s.error) { form = null; menu = null; }
    render();
  }
  /** A new state, after the older days shown are reloaded for it; a newer state wins over a slower one. */
  let stateGen = 0;
  function take(st) {
    const g = ++stateGen;
    return reloadOlder(st, g).then(() => { if (g === stateGen) apply(st); });
  }
  const refresh = () => call("memory_state").then(take).catch(() => {});

  // ---------- render (whole page; focus and the caret are put back) ----------
  function render() {
    const a = document.activeElement;
    const keys = restore || (a && a.dataset && a.dataset.k ? [a.dataset.k] : []);
    const caret = a && a.tagName === "INPUT" && a.type === "text" ? [a.selectionStart, a.selectionEnd] : null;
    restore = null;
    root.removeAttribute("aria-busy");
    root.className = "body" + (s.enabled ? "" : " off");

    const content = el("div", "content");
    content.append(header());
    if (s.error) content.append(errorNote());
    else {
      const kept = ((s.counts && s.counts.live) || 0) + (s.disputes || []).length;
      if (!s.enabled) content.append(el("p", "note info", kept
        ? "Memory is off. I'm not learning anything new, and I don't use what's below in our chats. It stays here until you forget it."
        : "Memory is off. I'm not learning anything new."));
      content.append(...asks(), ...groups(), ...history());
    }
    const old = root.querySelector(".content");
    root.replaceChild(content, old);
    const veil = root.querySelector(".veil");
    if (veil) veil.remove();
    if (dlg) root.append(dialog());
    renderBar();
    focusFirst(keys, caret);
  }

  function focusFirst(keys, caret) {
    for (const k of keys) {
      const f = root.querySelector('[data-k="' + CSS.escape(k) + '"]');
      if (!f || f.disabled || f.offsetParent === null) continue;
      f.focus();
      // a field keeps its caret across a re-render; a newly opened one starts at the end of its text
      if (f.tagName === "INPUT" && f.type === "text") {
        const at = caret || [f.value.length, f.value.length];
        f.setSelectionRange(at[0], at[1]);
      }
      return;
    }
  }

  function header() {
    const h = el("header", "intro");
    const top = el("div", "top");
    top.append(el("h1", null, "What I remember"));
    const lab = el("label", "switch");
    const cb = el("input");
    cb.type = "checkbox";
    cb.setAttribute("role", "switch");
    cb.setAttribute("aria-label", "Remember new things");
    cb.checked = !!s.enabled;
    cb.dataset.k = "switch";
    cb.addEventListener("change", () => setEnabled(cb.checked));
    lab.append(cb, txt(" Remember new things"));
    top.append(lab);
    h.append(top, el("p", null, "Stored only on this Mac. Your chat history is kept separately and isn't erased here."));
    return h;
  }

  function errorNote() {
    const n = el("div", "note err");
    n.setAttribute("role", "alert");
    const newer = s.error.code === "newer";
    n.append(el("b", null, newer ? "My memory was saved by a newer version of desk-buddy." : "I can't open my memory right now."), el("br"));
    n.append(txt(newer
      ? "I'm not learning or using anything until it opens: update desk-buddy, or Forget everything to start fresh. The file is "
      : "I'm not learning or using anything until it opens. The file is "));
    n.append(el("code", null, s.error.file || "~/.desk-buddy/memory/memory.sqlite"), txt("."));
    const r = el("div", "row start");
    r.append(btn("Try again", "small", "err-retry", () => { status = { kind: "idle" }; restore = ["err-retry", "switch"]; refresh(); }));
    n.append(r);
    return n;
  }

  // ---------- Needs your answer ----------
  function asks() {
    const ds = s.disputes || [];
    if (!ds.length) return [];
    const h = el("h2", "group", "Needs your answer ");
    h.append(el("span", "badge rose", String(ds.length)));
    const out = [h];
    ds.forEach((d, i) => {
      const live = d.live || {};
      const liveV = live.value != null ? live.value : (findRow(live.id) || {}).text || "";
      const newV = d.value != null ? d.value : d.text;
      const card = el("div", "ask");
      card.setAttribute("role", "group");
      card.setAttribute("aria-label", d.label || "A question");
      const p = el("p");
      p.append(txt((ASKED[live.source] || "You said") + " "), el("b", null, liveV), txt(". Later I picked up "), el("b", null, newV), txt(". Which is right?"));
      const r = el("div", "row start");
      const next = ds[i + 1] || ds[i - 1];
      const after = next ? ["ask-old:" + next.id] : [];
      r.append(
        btn(liveV, "small", "ask-old:" + d.id, () => act("memory_resolve", { id: d.id, keep: "old" }, "Couldn't save your answer.", after.concat("switch"))),
        btn(newV, "small", "ask-new:" + d.id, () => act("memory_resolve", { id: d.id, keep: "new" }, "Couldn't save your answer.", after.concat("switch"))),
      );
      card.append(p, r);
      out.push(card);
    });
    return out;
  }

  // ---------- the groups ----------
  function groups() {
    const out = [];
    let any = false;
    for (const [name, title, kind] of GROUPS) {
      const rs = rows(name);
      const adding = form && form.id == null && form.group === name;
      if (!rs.length && !adding) continue;
      any = true;
      const h = el("h2", "group", title + " ");
      h.append(el("span", "n", String(rs.length)));
      const add = btn("+ Add", "small quiet add", "add:" + name, () => openAdd(name, kind));
      add.disabled = !!form;
      h.append(add);
      const ul = el("ul", "rows");
      if (adding) ul.append(formRow());
      rs.forEach((r, i) => ul.append(form && form.id === r.id ? formRow() : row(r, name, rs[i + 1] || rs[i - 1])));
      out.push(h, ul);
    }
    if (!any && !(s.disputes || []).length && !days().length) {
      const c = el("div", "empty-card");
      c.append(el("b", null, s.enabled ? "Nothing yet" : "Nothing saved"));
      c.append(el("p", null, s.enabled
        ? "Tell me about yourself in chat and I'll keep what matters: where you live, how you like help, what I said I'd do. A few minutes after we stop talking, it shows up here."
        : "Memory is off, so I won't keep anything new from our chats. You can still add something here."));
      const r = el("div", "row start");
      r.append(btn("Add something", "small", "add-first", () => openAdd("about", "profile")));
      c.append(r);
      out.push(c);
    }
    return out;
  }

  function row(r, group, near, day) {
    const hist = group === "history";
    const li = el("li", "mrow");
    const t = el("div", "t");
    t.append(el("span", "txt", r.text));
    const m = el("span", "meta");
    if (r.disputed) m.append(el("span", "pill rose", "asked above"));
    if (hist && r.kind === "insight") {
      m.append(el("span", "pill", "pattern"));
      if (r.sources) m.append(txt("from " + plural(r.sources, "moment")));
    } else m.append(txt(hist ? SOURCE[r.source] || "" : meta(r)));
    t.append(m);
    const acts = el("div", "acts");
    // after a Forget: the next row, else the day (its summary), else the switch
    const gone = (near ? ["forget:" + near.id] : []).concat(hist ? ["day:" + day] : []);
    const back = gone.concat(hist ? ["hist-more", "switch"] : ["add:" + group, "add-first", "switch"]);
    if (!hist) acts.append(btn("Edit", "small quiet edit", "edit:" + r.id, () => openEdit(r, group)));
    acts.append(
      btn("Forget", "small quiet", "forget:" + r.id, () => act("memory_forget", { id: r.id, hard: false }, "Couldn't forget " + quoted(r.text) + ".", back)),
    );
    const more = btn("⋯", "small quiet more", "more:" + r.id, () => toggleMenu(r));
    more.setAttribute("aria-haspopup", "menu");
    more.setAttribute("aria-expanded", String(!!menu && menu.id === r.id));
    more.setAttribute("aria-label", "More for " + r.text);
    acts.append(more);
    li.append(t, acts);
    if (menu && menu.id === r.id) li.append(menuEl(r, group, back));
    return li;
  }

  // ---------- History: days newest first, the newest open; then Show older days ----------
  function dayLabel(day) {
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day || "");
    if (!m) return { name: day || "", date: "" };
    const at = Date.UTC(+m[1], +m[2] - 1, +m[3]);
    const d = new Date(at);
    const t = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s.today || "");
    const today = t ? Date.UTC(+t[1], +t[2] - 1, +t[3]) : NaN;
    const date = DAYS[d.getUTCDay()] + " " + d.getUTCDate() + " " + MONTHS[d.getUTCMonth()] + (t && t[1] !== m[1] ? " " + m[1] : "");
    const ago = Math.round((today - at) / 86400000);
    return ago === 0 ? { name: "Today", date } : ago === 1 ? { name: "Yesterday", date } : { name: date, date: "" };
  }
  function history() {
    const ds = days();
    if (!ds.length) return [];
    const h = el("h2", "group", "History ");
    const n = (s.counts && s.counts.days) || ds.length;
    h.append(el("span", "n", plural(n, "day")));
    const out = [h];
    ds.forEach((d, i) => {
      const items = d.items || [];
      const det = el("details", "day");
      const want = d.day in dayOpen ? dayOpen[d.day] : i === 0;
      det.open = want;
      const sum = el("summary");
      sum.dataset.k = "day:" + d.day;
      const lab = dayLabel(d.day);
      sum.append(txt(lab.name + " "), el("small", null, lab.date ? lab.date + " · " + items.length : String(items.length)));
      det.append(sum);
      // only the person's own toggles are kept (setting `open` above fires one too), so the newest day
      // starts open and yesterday's closes again when a newer day arrives
      det.addEventListener("toggle", () => { if (det.open !== want || d.day in dayOpen) dayOpen[d.day] = det.open; });
      const ul = el("ul", "rows");
      items.forEach((r, j) => ul.append(row(r, "history", items[j + 1] || items[j - 1], d.day)));
      det.append(ul);
      out.push(det);
    });
    const more = older.days.length ? older.more : !!s.historyMore;
    if (more) {
      const p = el("p", "more-days");
      const b = btn(older.busy ? "Loading…" : "Show older days", "small quiet", "hist-more", showOlder);
      b.disabled = older.busy;
      p.append(b);
      out.push(p);
    }
    return out;
  }
  function showOlder() {
    const ds = days();
    if (!ds.length || older.busy) return;
    older.busy = true;
    status = { kind: "idle" };
    restore = ["hist-more"];
    render();
    const g = stateGen;
    call("memory_history", { before: ds[ds.length - 1].day }).then((r) => {
      if (g !== stateGen) { older.busy = false; render(); return; } // the list changed meanwhile: ask again
      older = { days: older.days.concat((r && r.days) || []), more: !!(r && r.more), busy: false };
      // the first newly shown day takes focus, so the list reads on from where it was
      const first = r && r.days && r.days[0];
      restore = first ? ["day:" + first.day, "hist-more"] : ["hist-more"];
      render();
    }).catch(() => {
      older.busy = false;
      status = { kind: "err", text: "Couldn't show older days.", retry: showOlder };
      restore = ["hist-more"];
      render();
    });
  }
  /** After a change: reload the older days already shown (a Forget there must leave the page too). */
  function reloadOlder(st, g) {
    if (!older.days.length) return Promise.resolve();
    const want = older.days.length;
    const own = (st && st.history) || [];
    if (!own.length) { older = { days: [], more: false, busy: false }; return Promise.resolve(); }
    const got = [];
    let more = false;
    const page = (before) => call("memory_history", { before }).then((r) => {
      const ds = (r && r.days) || [];
      got.push(...ds);
      more = !!(r && r.more);
      if (more && ds.length && got.length < want) return page(ds[ds.length - 1].day);
    });
    // on failure the older days go (a forgotten moment must not stay on screen); Show older days brings them back
    const set = (v) => { if (g === stateGen) older = v; };
    return page(own[own.length - 1].day)
      .then(() => set({ days: got, more, busy: false }))
      .catch(() => set({ days: [], more: false, busy: false }));
  }

  // ---------- the row menu ----------
  function toggleMenu(r) {
    if (menu && menu.id === r.id) { menu = null; restore = ["more:" + r.id]; }
    else { menu = { id: r.id, step: "menu" }; restore = NARROW.matches ? ["menu-edit", "menu-erase"] : ["menu-erase"]; }
    render();
  }
  function closeMenu() {
    const id = menu.id;
    menu = null;
    restore = ["more:" + id];
    render();
  }
  function menuEl(r, group, gone) {
    const m = el("div", "menu");
    if (menu.step === "menu") {
      m.setAttribute("role", "menu");
      m.setAttribute("aria-label", "More for " + r.text);
      const erase = el("button", "it danger", "Erase completely…");
      erase.type = "button";
      erase.dataset.k = "menu-erase";
      erase.setAttribute("role", "menuitem");
      erase.append(el("small", null, group === "history" ? "Erases this moment for good. Can't be undone." : "Erases this and its history. Can't be undone."));
      erase.addEventListener("click", () => { menu.step = "confirm"; restore = ["er-cancel"]; render(); });
      if (group === "history") m.append(erase);
      else {
        const edit = el("button", "it only-narrow", "Edit…");
        edit.type = "button";
        edit.dataset.k = "menu-edit";
        edit.setAttribute("role", "menuitem");
        edit.addEventListener("click", () => { menu = null; openEdit(r, group); });
        m.append(edit, el("div", "sep only-narrow"), erase);
      }
      m.addEventListener("keydown", (e) => {
        const items = [...m.querySelectorAll("button.it")].filter((b) => b.offsetParent !== null);
        const i = items.indexOf(document.activeElement);
        let j = -1;
        if (e.key === "ArrowDown") j = (i + 1) % items.length;
        else if (e.key === "ArrowUp") j = (i - 1 + items.length) % items.length;
        else if (e.key === "Home") j = 0;
        else if (e.key === "End") j = items.length - 1;
        if (j < 0) return;
        e.preventDefault();
        items[j].focus();
      });
    } else {
      m.setAttribute("role", "alertdialog");
      m.setAttribute("aria-labelledby", "er-h");
      m.setAttribute("aria-describedby", "er-p");
      const c = el("div", "confirm");
      const b = el("b", null, "Erase " + quoted(r.text) + "?");
      b.id = "er-h";
      const p = el("p", null, group === "history" ? "This moment is deleted for good, here and in my notes. Can't be undone." : "This and anything it replaced are deleted for good. Can't be undone.");
      p.id = "er-p";
      c.append(b, p);
      const rr = el("div", "row");
      rr.append(
        btn("Cancel", "small quiet", "er-cancel", closeMenu),
        btn("Erase", "small danger", "er-go", () => {
          act("memory_forget", { id: r.id, hard: true }, "Couldn't erase " + quoted(r.text) + ".", gone, () => { menu = null; });
        }),
      );
      m.append(c, rr);
    }
    return m;
  }
  document.addEventListener("mousedown", (e) => {
    if (!menu || e.target.closest(".menu") || e.target.closest(".more")) return;
    menu = null;
    render();
  });

  // ---------- edit and add, in place ----------
  function openAdd(group, kind) {
    menu = null;
    form = { group, kind, id: null, text: "", value: "", keyed: false, label: null, was: "", orig: "", err: null, busy: false };
    restore = ["f-text"];
    render();
  }
  function openEdit(r, group) {
    menu = null;
    form = { group, kind: r.kind, id: r.id, text: r.text, value: r.value || "", keyed: !!r.key, label: r.label, was: r.text, orig: r.value || "", err: null, busy: false };
    restore = ["f-text"];
    render();
  }
  function cancelForm() {
    const back = form.id == null ? ["add:" + form.group, "add-first"] : ["edit:" + form.id, "more:" + form.id];
    form = null;
    if (status.kind === "err") status = { kind: "idle" };
    restore = back;
    render();
  }
  function formRow() {
    const li = el("li", "mrow editing");
    const isNew = form.id == null;
    const lt = el("label", "lbl", isNew ? "Something new" : "What I remember");
    lt.htmlFor = "f-text";
    const it = el("input", "field");
    it.id = "f-text";
    it.type = "text";
    it.dataset.k = "f-text";
    it.autocomplete = "off";
    it.value = form.text;
    if (isNew) it.placeholder = "e.g. Likes a short summary at the end";
    if (form.err) {
      it.classList.add("bad");
      it.setAttribute("aria-invalid", "true");
      it.setAttribute("aria-describedby", "f-err");
    }
    li.append(lt, it);
    let iv = null;
    if (form.keyed) {
      const lv = el("label", "lbl", form.label || "Value");
      lv.htmlFor = "f-value";
      iv = el("input", "field");
      iv.id = "f-value";
      iv.type = "text";
      iv.dataset.k = "f-value";
      iv.autocomplete = "off";
      iv.value = form.value;
      li.append(lv, iv);
    }
    if (form.err) {
      const n = el("p", "note err", FORM_ERR[form.err]);
      n.id = "f-err";
      li.append(n);
    }
    const r = el("div", "row");
    if (!isNew) r.append(el("span", "hint", "Saved as yours: it replaces " + quoted(form.was) + ", which I keep as history."));
    const save = btn(isNew ? "Add" : "Save", "small primary", "f-save", saveForm);
    save.disabled = isNew && !form.text.trim();
    r.append(btn("Cancel", "small quiet", "f-cancel", cancelForm), save);
    li.append(r);
    it.addEventListener("input", () => {
      form.text = it.value;
      if (isNew) save.disabled = !it.value.trim();
    });
    if (iv) iv.addEventListener("input", () => { form.value = iv.value; });
    for (const f of [it, iv]) {
      if (f) f.addEventListener("keydown", (e) => {
        if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); saveForm(); }
      });
    }
    return li;
  }
  function saveForm() {
    if (!form || form.busy) return;
    const f = form;
    const text = f.text.trim();
    if (f.id == null && !text) return;
    if (f.id != null && text === f.was && (!f.keyed || f.value.trim() === f.orig)) return cancelForm();
    let saveText = text;
    if (f.id != null && f.keyed && f.value.trim() && f.value.trim() !== f.orig && text === f.was) {
      // a new value with the old sentence would keep saying the old fact: swap it in, or ask
      const i = f.orig ? text.toLowerCase().indexOf(f.orig.toLowerCase()) : -1;
      if (i < 0 || text.toLowerCase().length !== text.length) {
        f.err = "stale_text";
        restore = ["f-text"];
        return render();
      }
      saveText = text.slice(0, i) + f.value.trim() + text.slice(i + f.orig.length);
    }
    f.busy = true;
    status = { kind: "idle" };
    const p = f.id == null
      ? call("memory_add", { kind: f.kind, text })
      : call("memory_edit", { id: f.id, text: saveText, value: f.keyed && f.value.trim() ? f.value.trim() : null });
    p.then(() => {
      if (form === f) form = null;
      restore = f.id == null ? ["add:" + f.group] : ["switch"];
      return refresh();
    }).catch((e) => {
      f.busy = false;
      if (form !== f) return;
      if (FORM_ERR[e]) {
        f.err = e;
        restore = ["f-text"];
      } else if (e === "not_found") {
        form = null;
        status = { kind: "err", text: "That changed while you were editing it." };
        restore = ["switch"];
        return refresh();
      } else {
        f.err = null;
        status = { kind: "err", text: f.id == null ? "Couldn't add that." : "Couldn't save " + quoted(f.was) + ".", retry: saveForm };
      }
      render();
    });
  }

  // ---------- one change (Forget, Erase, an answer) ----------
  function act(cmd, args, failText, after, onOk) {
    if (inflight) return;
    inflight = true;
    status = { kind: "idle" };
    const again = () => act(cmd, args, failText, after, onOk);
    call(cmd, args).then(() => {
      inflight = false;
      if (onOk) onOk();
      restore = after;
      return refresh();
    }).catch((e) => {
      inflight = false;
      if (e === "not_found") {
        if (onOk) onOk();
        status = { kind: "err", text: "That was already gone." };
        restore = after;
        return refresh();
      }
      status = { kind: "err", text: failText, retry: again };
      if (menu) menu = null;
      render();
    });
  }

  function setEnabled(on) {
    status = { kind: "idle" };
    call("memory_set_enabled", { on }).then(() => { restore = ["switch"]; return refresh(); }).catch(() => {
      status = { kind: "err", text: on ? "Couldn't turn memory on." : "Couldn't turn memory off.", retry: () => setEnabled(on) };
      restore = ["switch"];
      render();
    });
  }

  // ---------- Forget everything ----------
  function openDlg() {
    if (!s) return;
    menu = null;
    dlg = { typed: "", err: null, busy: false };
    restore = ["fa-input"];
    render();
  }
  function closeDlg() {
    dlg = null;
    restore = ["forget-all", "switch"];
    render();
  }
  function dialog() {
    const veil = el("div", "veil");
    const d = el("div", "dlg");
    d.setAttribute("role", "dialog");
    d.setAttribute("aria-modal", "true");
    d.setAttribute("aria-labelledby", "fa-h");
    const h = el("h2", null, "Forget everything?");
    h.id = "fa-h";
    const n = (s.counts && s.counts.live) || 0;
    const what = s.error || !n ? "everything I remember" : n === 1 ? "the 1 thing I remember" : "all " + n + " things I remember";
    const p = el("p", null, "This erases " + what + " and the notes made from them, for good. Our chat history is kept separately and isn't erased.");
    const lab = el("label", "lbl");
    lab.htmlFor = "fa";
    lab.append(txt("Type "), el("b", null, s.phrase), txt(" to confirm"));
    const inp = el("input", "field");
    inp.id = "fa";
    inp.type = "text";
    inp.autocomplete = "off";
    inp.spellcheck = false;
    inp.dataset.k = "fa-input";
    inp.value = dlg.typed;
    const r = el("div", "row");
    const err = el("span", "err", dlg.err || "");
    err.setAttribute("role", "status");
    const go = btn("Forget everything", "small danger", "fa-go", forgetAll);
    const match = () => dlg.typed.trim() === s.phrase;
    go.disabled = !match() || dlg.busy;
    r.append(err, btn("Cancel", "small quiet", "fa-cancel", closeDlg), go);
    inp.addEventListener("input", () => { dlg.typed = inp.value; go.disabled = !match() || dlg.busy; });
    inp.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.isComposing && match()) { e.preventDefault(); forgetAll(); }
    });
    d.append(h, p, lab, inp, r);
    // Tab stays inside the dialog
    d.addEventListener("keydown", (e) => {
      if (e.key !== "Tab") return;
      const f = [...d.querySelectorAll("input, button")].filter((x) => !x.disabled);
      if (!f.length) return;
      const first = f[0], last = f[f.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    });
    veil.addEventListener("mousedown", (e) => { if (e.target === veil) closeDlg(); });
    veil.append(d);
    return veil;
  }
  function forgetAll() {
    if (!dlg || dlg.busy || dlg.typed.trim() !== s.phrase) return;
    dlg.busy = true;
    dlg.err = null;
    call("memory_forget_all", { confirm: dlg.typed.trim() }).then(() => {
      dlg = null;
      form = null;
      menu = null;
      status = { kind: "ok", text: "Forgot everything." };
      restore = ["add-first", "switch"];
      return refresh();
    }).catch((e) => {
      if (!dlg) return;
      dlg.busy = false;
      dlg.err = e === "bad_phrase" ? "Type the phrase exactly." : "Couldn't forget everything. Try again.";
      restore = ["fa-input"];
      render();
    });
  }

  // ---------- the bar ----------
  function renderBar() {
    let cls = "", text = null, retry = null, ok = false;
    if (s.error) { cls = " err"; text = "Memory isn't available."; }
    else if (status.kind === "err") { cls = " err"; text = status.text; retry = status.retry || null; }
    else if (status.kind === "ok") { ok = true; text = status.text; }
    else if (!s.enabled) { ok = true; text = "Memory is off."; }
    else {
      const c = s.counts || { live: 0, disputes: 0 };
      const parts = [];
      if (c.live) parts.push(plural(c.live, "thing"));
      if (c.days) parts.push(plural(c.days, "day"));
      if (c.disputes) parts.push(c.disputes + " to answer");
      text = parts.join(" · ");
    }
    bar.className = "bar" + cls;
    stat.className = "status" + (ok ? " ok" : "");
    stat.replaceChildren(txt(text));
    if (retry) {
      const b = el("button", "linkish", "Try again");
      b.type = "button";
      b.dataset.k = "retry";
      b.addEventListener("click", () => { restore = ["retry", "switch"]; retry(); });
      stat.append(b);
    }
    const c = s.counts || {};
    // also offered for what the list doesn't show (forgotten or replaced notes, chats not learned yet)
    faBtn.disabled = !s.error && !c.live && !c.disputes && !c.hidden;
  }

  // ---------- keys ----------
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || e.isComposing || !s) return;
    e.preventDefault();
    if (dlg) return closeDlg();
    if (menu) return closeMenu();
    if (form) return cancelForm();
    call("memory_close").catch(() => {});
  });
  NARROW.addEventListener("change", () => { if (s) render(); });

  T.event.listen("memory://state", (e) => { take(e.payload); });
  call("memory_state").then((st) => { if (st && !s) apply(st); }).catch(() => {});
})();
