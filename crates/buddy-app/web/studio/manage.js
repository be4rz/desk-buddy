// The studio's library (#studio-library), Edit mode (#studio-edit) and dialogs (#studio-new,
// #studio-customize, #studio-delete, #studio-restore). Built on studio.js (`window.__studio`).
// Mascots are named by id only; no path ever goes from here to Rust. Edits are sent as ops on
// pack.json ({op: set|remove, path, json}) or, from Advanced, as the whole text; the backend checks
// either before it writes anything (`dry: true` only checks), so the forms never have to be right.
"use strict";
(function () {
  const S = window.__studio;
  if (!S) return;
  const { call, el, $ } = S;
  const TEMPER = ["energy", "curiosity", "shyness", "chattiness", "sleepiness"];
  const KIND = { builtin: ["Built-in", "grey"], customized: ["Built-in · customized", ""], yours: ["Yours", "teal"], folder: ["From a folder", "grey"] };
  const MAX_FILE = 32 << 20;

  const m = {
    lib: null, // studio_library
    ed: null, // studio_edit
    doc: null, // pack.json, parsed, with the pending ops applied
    ops: [], // pending form edits, in order
    raw: null, // pending Advanced text (null = none)
    sec: "details",
    bsel: null, // the behavior being edited
    check: null, // the last check of the pending edit: { ok, error }
    refused: null, // the backend's answer to the last Save, when it refused
    lint: null,
    gen: 0, checkGen: 0, checkT: 0,
    imp: null, impMsg: null, // the picked .aseprite for Clips
    pv: { clip: null, dir: null, timer: 0, frames: null, i: 0 },
    dlg: null,
  };
  const cur = () => (S.state() && S.state().pack) || null;
  const dirty = () => m.ops.length > 0 || m.raw !== null;
  const bytes = (b) => (b < 1024 ? b + " B" : b < 1 << 20 ? Math.round(b / 1024) + " KB" : (b / 1048576).toFixed(b < 10 << 20 ? 1 : 0) + " MB");
  const option = (value, label, sel) => { const o = el("option", "", label == null ? value : label); o.value = value; o.selected = !!sel; return o; };
  const button = (label, cls, fn) => { const b = el("button", "btn small" + (cls ? " " + cls : ""), label); b.type = "button"; if (fn) b.addEventListener("click", fn); return b; };

  // the id a name becomes (the same rule as `package::slug`)
  function slug(name) {
    let s = "";
    for (const c of name.toLowerCase()) {
      if (/[a-z0-9]/.test(c)) s += c;
      else if (s && !s.endsWith("-")) s += "-";
      if (s.length >= 32) break;
    }
    return s.replace(/-+$/, "");
  }
  // a clip-name prefix from a file name (the rule `aseprite::valid_name` checks)
  function prefixOf(file) {
    let p = file.replace(/\.[^.]*$/, "").toLowerCase().replace(/[^a-z0-9_]+/g, "_").replace(/^_+|_+$/g, "");
    if (!/^[a-z]/.test(p)) p = "clip" + (p ? "_" + p : "");
    return p.slice(0, 40);
  }
  function readFile(f) {
    return new Promise((ok, bad) => {
      if (f.size > MAX_FILE) return bad("that file is too big (the studio takes up to 32 MB)");
      const r = new FileReader();
      r.onload = () => ok(String(r.result).replace(/^data:[^,]*,/, ""));
      r.onerror = () => bad("couldn't read " + f.name);
      r.readAsDataURL(f);
    });
  }

  // ---------- library ----------
  async function loadLibrary() {
    const gen = ++m.gen;
    try {
      const lib = await call("studio_library");
      if (gen !== m.gen) return;
      m.lib = lib;
      $("lib-error").hidden = true;
    } catch (e) {
      $("lib-error").textContent = "Couldn't list the mascots: " + e;
      $("lib-error").hidden = false;
      return;
    }
    renderLibrary();
  }

  function renderLibrary() {
    const L = m.lib;
    $("lib-where").replaceChildren(...(L.library_dir ? [document.createTextNode("Your mascots live in "), el("code", "", L.library_dir)] : [document.createTextNode("No mascot library: this studio only shows mascots")]));
    const out = [];
    const thumbs = [];
    for (const [h, f] of [["Built-in", (p) => p.kind === "builtin" || p.kind === "customized"], ["Yours", (p) => p.kind === "yours"], ["From a folder", (p) => p.kind === "folder"]]) {
      const ps = L.packs.filter(f);
      const yours = h === "Yours";
      if (!ps.length && !(yours && L.library)) continue;
      out.push(el("h3", "", h));
      const cards = el("div", "cards");
      for (const p of ps) { const [c, img] = card(p); cards.append(c); thumbs.push([p.id, img]); }
      if (yours && L.library) cards.append(newCard());
      out.push(cards);
    }
    $("lib-groups").replaceChildren(...out);
    loadThumbs(thumbs);
  }

  async function loadThumbs(list) {
    for (const [id, img] of list) {
      try {
        const t = await call("studio_thumb", { id });
        img.onload = () => { img.width = img.naturalWidth * Math.max(1, Math.min(3, Math.floor(130 / img.naturalHeight))); };
        img.src = t.png;
      } catch (e) {
        img.replaceWith(el("span", "hint", "no preview: " + e));
      }
    }
  }

  function card(p) {
    const c = el("div", "card");
    c.dataset.id = p.id;
    const th = el("div", "thumb");
    const [label, tone] = KIND[p.kind] || KIND.folder;
    th.append(el("span", "pill " + tone, label));
    if (p.desktop) th.append(el("span", "pill teal inuse", "On your desktop"));
    const img = el("img", "px");
    img.alt = p.name;
    th.append(img);
    const body = el("div", "body");
    body.append(el("span", "name", p.name), el("span", "meta", p.id + " · " + p.files + " files · " + bytes(p.bytes)));
    if (p.description) body.append(el("span", "desc", p.description));
    const acts = el("div", "acts");
    acts.append(button("Open", "primary", () => open(p)));
    if (p.can.duplicate) acts.append(button("Duplicate", "", () => openNew("copy", p.id)));
    const more = el("span", "more");
    if (p.can.use && !p.desktop && p.kind !== "folder") more.append(button("Put on desktop", "quiet", () => use(p.id)));
    if (p.can.restore) more.append(button("Restore original…", "quiet", () => openRestore(p.id)));
    if (p.can.delete) more.append(button("Delete…", "quiet", () => openDelete(p.id)));
    acts.append(more);
    body.append(acts);
    c.append(th, body);
    return [c, img];
  }

  function newCard() {
    const c = el("div", "card new");
    c.setAttribute("role", "button");
    c.tabIndex = 0;
    c.append(el("div", "plus", "+"), el("b", "", "New mascot"));
    const h = el("span", "hint");
    h.append(document.createTextNode("From an "), el("code", "", ".aseprite"), document.createTextNode(" file, or as a copy of one you have"));
    c.append(h);
    c.addEventListener("click", () => openNew("aseprite"));
    c.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); openNew("aseprite"); } });
    return c;
  }

  async function open(p) {
    try {
      await S.openPack(p.id, p.can.edit ? "edit" : "review");
    } catch (e) {
      $("lib-error").textContent = "Couldn't open " + p.name + ": " + e;
      $("lib-error").hidden = false;
    }
  }

  async function use(id) {
    try {
      m.lib = await call("studio_use", { id });
      renderLibrary();
    } catch (e) {
      $("lib-error").textContent = "Couldn't put it on the desktop: " + e;
      $("lib-error").hidden = false;
    }
  }

  async function library() {
    if (!m.lib) m.lib = await call("studio_library");
    return m.lib;
  }
  const entry = (id) => (m.lib ? m.lib.packs.find((p) => p.id === id) : null);

  // ---------- dialogs ----------
  function openDlg(id) {
    closeDlg();
    m.dlg = id;
    $(id).hidden = false;
    const f = $(id).querySelector("input:not([type=file]):not([hidden]), button.danger, button.primary:not([disabled])");
    if (f) f.focus();
  }
  function closeDlg() {
    if (m.dlg) $(m.dlg).hidden = true;
    m.dlg = null;
  }
  for (const v of document.querySelectorAll(".veil")) {
    v.addEventListener("click", (e) => { if (e.target === v || e.target.closest("[data-close]")) closeDlg(); });
  }
  addEventListener("keydown", (e) => { if (e.key === "Escape" && m.dlg) closeDlg(); });

  // #studio-new
  const nw = { from: "aseprite", file: null, busy: false };
  async function openNew(from, source) {
    try { await library(); } catch (e) { return S.show("library"); }
    if (!m.lib.library) return;
    nw.from = from;
    nw.file = null;
    $("new-file").textContent = "No file chosen";
    $("new-source").replaceChildren(...m.lib.packs.map((p) => option(p.id, p.name + " (" + (KIND[p.kind] || KIND.folder)[0].toLowerCase() + ")", p.id === source)));
    const src = entry(source);
    $("new-name").value = from === "copy" && src ? src.name + " copy" : "";
    renderNew();
    openDlg("studio-new");
  }
  function renderNew() {
    for (const o of document.querySelectorAll("#studio-new .opt")) { o.classList.toggle("on", o.dataset.opt === nw.from); o.setAttribute("aria-checked", String(o.dataset.opt === nw.from)); }
    $("new-ase").hidden = nw.from !== "aseprite";
    $("new-copy").hidden = nw.from !== "copy";
    const name = $("new-name").value.trim();
    const id = slug(name);
    $("new-id").textContent = id || "—";
    const taken = m.lib.packs.find((p) => p.id === id);
    const why = !name ? "· type a name" : !id ? "· the name needs a letter or a digit" : taken ? (taken.kind === "builtin" || taken.kind === "customized" ? "· a built-in mascot has this id" : "· you already have a mascot with this id") : nw.from === "aseprite" && !nw.file ? "· choose an .aseprite file" : "";
    $("new-err").textContent = why;
    $("new-ok").disabled = !!why || nw.busy;
  }
  for (const o of document.querySelectorAll("#studio-new .opt")) {
    const pick = () => {
      nw.from = o.dataset.opt;
      if (nw.from === "copy" && !$("new-name").value.trim()) { const src = entry($("new-source").value); if (src) $("new-name").value = src.name + " copy"; }
      renderNew();
    };
    o.addEventListener("click", pick);
    o.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); pick(); } });
  }
  $("new-name").addEventListener("input", renderNew);
  $("new-source").addEventListener("change", renderNew);
  $("new-choose").addEventListener("click", () => $("new-file-in").click());
  $("new-file-in").addEventListener("change", async (e) => {
    const f = e.target.files[0];
    e.target.value = "";
    if (!f) return;
    try {
      nw.file = { name: f.name, b64: await readFile(f) };
      $("new-file").replaceChildren(el("b", "", f.name), document.createTextNode(" · " + bytes(f.size)));
      if (!$("new-name").value.trim()) $("new-name").value = f.name.replace(/\.[^.]*$/, "").replace(/[-_]+/g, " ").replace(/^./, (c) => c.toUpperCase());
    } catch (err) {
      nw.file = null;
      $("new-file").textContent = String(err);
    }
    renderNew();
  });
  $("new-ok").addEventListener("click", async () => {
    const name = $("new-name").value.trim();
    const args = { from: nw.from, name, id: slug(name) };
    if (nw.from === "aseprite") Object.assign(args, { bytes: nw.file && nw.file.b64, file_name: nw.file && nw.file.name });
    else args.source = $("new-source").value;
    nw.busy = true;
    renderNew();
    $("new-busy").textContent = nw.from === "copy" ? "Copying…" : "Making it…";
    try {
      const st = await call("studio_create", args);
      closeDlg();
      m.lib = null;
      m.sec = "details";
      await S.refresh(st, "edit");
    } catch (e) {
      $("new-err").textContent = "· " + e;
    } finally {
      nw.busy = false;
      $("new-busy").textContent = "";
      if (m.dlg === "studio-new") $("new-ok").disabled = !!$("new-err").textContent;
    }
  });

  // #studio-customize: a built-in becomes the person's own copy (same id); the original stays
  async function openCustomize(id) {
    try { await library(); } catch (e) { return; }
    const p = entry(id);
    if (!p) return;
    $("cus-h").textContent = "Make " + p.name + " yours?";
    $("cus-text").replaceChildren(
      document.createTextNode("The studio copies the built-in " + p.name + " into your mascots ("),
      el("code", "", (m.lib.library_dir || "your library") + "/" + p.id),
      document.createTextNode(", about " + bytes(p.bytes) + "). From then on desk-buddy uses your copy."),
    );
    $("cus-ok").onclick = async () => {
      $("cus-ok").disabled = true;
      $("cus-busy").textContent = "Copying…";
      try {
        const st = await call("studio_customize", { id });
        closeDlg();
        m.lib = null;
        m.sec = "details";
        await S.refresh(st, "edit");
      } catch (e) {
        $("cus-busy").textContent = String(e);
      } finally {
        $("cus-ok").disabled = false;
        if (!m.dlg) $("cus-busy").textContent = "";
      }
    };
    openDlg("studio-customize");
  }

  // #studio-delete: only mascots in Yours (the backend refuses any other)
  async function openDelete(id) {
    try { await library(); } catch (e) { return; }
    const p = entry(id);
    if (!p || !p.can.delete) return;
    const bear = entry("bear");
    $("del-h").textContent = "Delete “" + p.name + "”?";
    $("del-text").replaceChildren(document.createTextNode("This removes the folder "), el("code", "", (m.lib.library_dir || "your library") + "/" + p.id), document.createTextNode(" and everything in it: its art, behavior and comments. This can't be undone."));
    $("del-desk").textContent = p.desktop ? p.name + " is on your desktop now: desk-buddy goes back to the " + (bear ? bear.name : "bear") + "." : p.name + " isn't on your desktop right now.";
    $("del-err").textContent = "";
    $("del-ok").onclick = async () => {
      $("del-ok").disabled = true;
      try {
        const st = await call("studio_delete", { id });
        closeDlg();
        m.ops = []; m.raw = null; m.ed = null;
        m.lib = null;
        await S.refresh(st, "library");
      } catch (e) {
        $("del-err").textContent = String(e);
      } finally {
        $("del-ok").disabled = false;
      }
    };
    openDlg("studio-delete");
  }

  // #studio-restore: a customized built-in loses the person's copy and comes back as shipped
  async function openRestore(id) {
    try { await library(); } catch (e) { return; }
    const p = entry(id);
    if (!p || !p.can.restore) return;
    $("res-h").textContent = "Restore the original " + p.name + "?";
    $("res-text").replaceChildren(document.createTextNode("This deletes your version ("), el("code", "", (m.lib.library_dir || "your library") + "/" + p.id), document.createTextNode(") with every change you made to it. The built-in " + p.name + " comes back as shipped. This can't be undone."));
    $("res-err").textContent = "";
    $("res-ok").onclick = async () => {
      $("res-ok").disabled = true;
      try {
        const st = await call("studio_restore", { id });
        closeDlg();
        m.ops = []; m.raw = null; m.ed = null;
        m.lib = null;
        await S.refresh(st, st.pack && st.pack.id === id && S.view() !== "library" ? "review" : "library");
      } catch (e) {
        $("res-err").textContent = String(e);
      } finally {
        $("res-ok").disabled = false;
      }
    };
    openDlg("studio-restore");
  }

  // ---------- navigation ----------
  function leaveEdit() {
    if (S.view() === "edit" && dirty()) {
      $("save-state").textContent = "Save or discard your changes first.";
      $("savebar").classList.add("err");
      return false;
    }
    return true;
  }
  function go(what) {
    const p = cur();
    switch (what) {
      case "library": if (leaveEdit()) S.show("library"); break;
      case "review": if (leaveEdit()) S.show("review"); break;
      case "edit":
        if (!p) break;
        if (p.writable) S.show("edit");
        else if (p.kind === "builtin") openCustomize(p.id);
        else openNew("copy", p.id);
        break;
      case "customize": if (p) openCustomize(p.id); break;
      case "duplicate": if (p) openNew("copy", p.id); break;
      case "delete": if (p) openDelete(p.id); break;
      case "restore": if (p) openRestore(p.id); break;
      case "reveal": if (p) call("studio_reveal", { id: p.id }).catch((e) => { m.impMsg = { err: true, text: String(e) }; renderClips(); }); break;
    }
  }
  S.hooks.go = go;
  document.addEventListener("click", (e) => {
    const t = e.target.closest("[data-go]");
    if (t && !t.closest(".veil")) go(t.dataset.go);
  });
  for (const b of document.querySelectorAll(".back")) b.addEventListener("keydown", (e) => { if (e.key === "Enter") go("library"); });
  S.hooks.view = (v) => {
    if (v !== "edit") stopPreview();
    if (v === "library") loadLibrary();
    if (v === "edit") {
      const p = cur();
      if (m.ed && p && m.ed.id === p.id && dirty()) renderEdit();
      else loadEdit();
    }
  };

  // ---------- edit: the pending change ----------
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  function has(path) {
    let o = m.doc;
    for (const k of path) {
      if (o == null || typeof o !== "object" || !(k in o)) return false;
      o = o[k];
    }
    return true;
  }
  function getp(path) {
    let o = m.doc;
    for (const k of path) { if (o == null) return undefined; o = o[k]; }
    return o;
  }
  // set `path` to `value`; a missing object or array on the way is added whole
  function put(path, value) {
    let i = path.length - 1;
    while (i > 0 && !has(path.slice(0, i))) i--;
    let v = value;
    for (let j = path.length - 1; j > i; j--) v = typeof path[j] === "number" ? [v] : { [path[j]]: v };
    push({ op: "set", path: path.slice(0, i + 1), json: JSON.stringify(v) });
  }
  function del(path) {
    if (has(path)) push({ op: "remove", path });
  }
  function push(op) {
    const last = m.ops[m.ops.length - 1];
    if (last && last.op === "set" && op.op === "set" && same(last.path, op.path)) m.ops[m.ops.length - 1] = op;
    else m.ops.push(op);
    applyLocal(op);
    changed();
  }
  // the same edit on the page's copy (jsonedit's rules: an index equal to the length appends)
  function applyLocal(op) {
    const p = op.path;
    let o = m.doc;
    for (let i = 0; i < p.length - 1; i++) o = o[p[i]];
    const k = p[p.length - 1];
    if (op.op === "set") {
      const v = JSON.parse(op.json);
      if (Array.isArray(o) && k === o.length) o.push(v);
      else o[k] = v;
    } else if (Array.isArray(o)) o.splice(k, 1);
    else delete o[k];
  }
  function changed() {
    m.refused = null;
    m.flash = null;
    m.check = null;
    renderSavebar();
    renderCheck();
    clearTimeout(m.checkT);
    m.checkT = setTimeout(dryCheck, 350);
  }
  function saveArgs(dry) {
    const a = { sha: m.ed.sha };
    if (m.raw !== null) a.raw = m.raw;
    else a.ops = m.ops;
    if (dry) a.dry = true;
    return a;
  }
  async function dryCheck() {
    if (!dirty()) return renderCheck();
    const gen = ++m.checkGen;
    let r;
    try { await call("studio_save", saveArgs(true)); r = { ok: true }; } catch (e) { r = { ok: false, error: String(e) }; }
    if (gen !== m.checkGen || !dirty()) return;
    m.check = r;
    renderCheck();
    renderSavebar();
  }
  async function save() {
    if (!dirty()) return;
    clearTimeout(m.checkT);
    m.checkGen++;
    $("save").disabled = true;
    $("save-state").textContent = "Saving…";
    try {
      const ed = await call("studio_save", saveArgs(false));
      setEdit(ed);
      m.flash = "Saved";
      await S.refresh(null);
    } catch (e) {
      m.refused = String(e);
      m.check = { ok: false, error: String(e) };
    } finally {
      $("save").disabled = false;
      renderSavebar();
      renderCheck();
    }
  }
  $("save").addEventListener("click", save);
  $("discard").addEventListener("click", () => { m.ops = []; m.raw = null; m.refused = null; loadEdit(); });
  addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "s" && S.view() === "edit") { e.preventDefault(); save(); }
  });

  function renderSavebar() {
    const b = $("savebar");
    const bad = dirty() && m.check && !m.check.ok;
    b.hidden = !dirty() && !m.flash;
    b.classList.toggle("err", !!bad);
    const st = $("save-state");
    if (!dirty()) { st.textContent = m.flash || ""; $("save").disabled = true; $("discard").disabled = true; return; }
    $("save").disabled = false;
    $("discard").disabled = false;
    if (m.refused) st.replaceChildren(el("b", "", "Not saved. "), document.createTextNode(m.refused + ". The file on disk is unchanged."));
    else if (bad) st.replaceChildren(el("b", "", "This edit doesn't load. "), document.createTextNode(m.check.error));
    else st.textContent = m.check ? "Unsaved changes · they load" : "Unsaved changes";
  }

  function renderCheck() {
    const box = $("ed-check");
    if (!m.ed) return box.replaceChildren();
    if (dirty() && m.check && !m.check.ok) {
      const err = el("div", "issue", m.check.error);
      err.id = "studio-edit-error";
      box.replaceChildren(el("span", "err", "✕ This edit doesn't load"), err, el("p", "hint", "Nothing was written. The preview shows the last saved version."));
      return;
    }
    if (dirty()) {
      box.replaceChildren(m.check ? el("span", "ok", "✓ This edit loads") : el("span", "hint", "Checking…"), el("span", "hint", m.check ? " · not saved yet" : ""));
      return;
    }
    const parts = [el("span", "ok", "✓ Loads")];
    const l = m.lint;
    if (l) parts.push(el("span", "hint", " · lint: " + l.errors + (l.errors === 1 ? " error, " : " errors, ") + l.warnings + (l.warnings === 1 ? " warning" : " warnings")));
    if (l) for (const i of l.issues.slice(0, 3)) parts.push(el("div", "issue" + (i.severity === "error" ? "" : " warn"), i.code + " · " + i.file + " " + i.detail));
    if (m.ed.writable && m.ed.findings.length) parts.push(el("div", "issue", m.ed.findings.length + " package problem" + (m.ed.findings.length === 1 ? "" : "s") + ": see Folder & check"));
    box.replaceChildren(...parts);
  }

  // ---------- edit: loading ----------
  async function loadEdit() {
    const gen = ++m.gen;
    clearTimeout(m.checkT);
    try {
      const [ed, lint] = await Promise.all([call("studio_edit"), call("studio_lint").catch(() => null)]);
      if (gen !== m.gen) return;
      m.lint = lint;
      setEdit(ed);
    } catch (e) {
      $("ed-check").replaceChildren(el("span", "err", "Couldn't read the mascot: " + e));
    }
  }
  function setEdit(ed) {
    m.ed = ed;
    m.doc = JSON.parse(ed.text);
    m.ops = [];
    m.raw = null;
    m.check = null;
    m.refused = null;
    renderEdit();
    startPreview();
  }

  function renderEdit() {
    const ed = m.ed;
    if (!ed) return;
    const p = cur();
    $("ed-name").textContent = m.doc.name || ed.id;
    const [label, tone] = KIND[ed.kind] || KIND.folder;
    $("ed-pill").textContent = label;
    $("ed-pill").className = "pill " + tone;
    $("ed-path").textContent = p ? p.dir : "";
    $("ed-path").title = p ? p.dir : "";
    for (const a of document.querySelectorAll("#ed-nav a[data-sec]")) a.classList.toggle("on", a.dataset.sec === m.sec);
    $("nav-check").textContent = ed.findings.length ? String(ed.findings.length) : "ok";
    $("nav-check").className = "pill " + (ed.findings.length ? "rose" : "teal");
    $("nav-delete").hidden = ed.kind !== "yours";
    $("nav-restore").hidden = ed.kind !== "customized";
    $("nav-danger").hidden = ed.kind !== "yours" && ed.kind !== "customized";
    for (const f of document.querySelectorAll("#studio-edit .sec")) f.classList.toggle("on", f.id === "sec-" + m.sec);
    // one kind of pending change at a time: the forms' ops, or Advanced's whole text
    const lock = !ed.writable ? "This mascot is read-only here." : m.raw !== null && m.sec !== "advanced" ? "You have unsaved changes in Advanced. Save or discard them to use the forms." : m.ops.length && m.sec === "advanced" ? "You have unsaved form changes. Save or discard them to edit the raw file." : null;
    $("ed-lock").textContent = lock || "";
    $("ed-lock").hidden = !lock;
    $("sec-" + m.sec).disabled = !!lock && m.sec !== "package";
    ({ details: renderDetails, persona: renderPersona, clips: renderClips, behaviors: renderBehaviors, intents: renderIntents, idle: renderIdle, package: renderPackage, advanced: renderAdvanced })[m.sec]();
    renderSavebar();
    renderCheck();
  }
  for (const a of document.querySelectorAll("#ed-nav a[data-sec]")) {
    a.addEventListener("click", () => { m.sec = a.dataset.sec; m.flash = null; m.impMsg = null; renderEdit(); });
  }

  // ---------- edit: sections ----------
  function bindText(id, path, optional) {
    const i = $(id);
    i.value = getp(path) == null ? "" : String(getp(path));
    i.oninput = () => {
      if (optional && !i.value.trim()) { del(path); changed(); } else put(path, i.value);
      if (id === "f-name") $("ed-name").textContent = i.value || m.ed.id;
    };
  }
  function bindNum(id, path, int) {
    const i = $(id);
    const v = getp(path);
    i.value = v == null ? "" : v;
    i.placeholder = v == null ? "—" : "";
    i.onchange = () => {
      const n = Number(i.value);
      if (i.value === "" || !Number.isFinite(n)) { i.value = getp(path) == null ? "" : getp(path); return; }
      put(path, int ? Math.round(n) : n);
    };
  }

  function renderDetails() {
    bindText("f-name", ["name"]);
    bindText("f-description", ["description"], true);
    bindText("f-author", ["author"], true);
    $("f-id").textContent = m.doc.id;
    const box = $("f-temper");
    const t = m.doc.temperament || {};
    box.replaceChildren(...TEMPER.map((k) => {
      const row = el("div", "slider");
      const v = t[k] == null ? 0.5 : t[k];
      const lab = el("span", "mono", v.toFixed(2).replace(/^0/, ""));
      const r = el("input");
      Object.assign(r, { type: "range", min: 0, max: 100, value: Math.round(v * 100) });
      r.setAttribute("aria-label", k);
      r.addEventListener("input", () => { lab.textContent = (r.value / 100).toFixed(2).replace(/^0/, ""); });
      r.addEventListener("change", () => put(["temperament", k], Number(r.value) / 100));
      row.append(el("span", "", k), r, lab);
      return row;
    }));
    bindNum("f-blink", ["timers", "blink", "mean_ms"], true);
    bindNum("f-jitter", ["timers", "blink", "jitter"], false);
    bindNum("f-mouth", ["timers", "mouth", "frame_ms"], true);
  }

  const clipNames = () => { const p = cur(); return p ? p.clips.map((c) => c.name) : []; };
  const behaviorNames = () => Object.keys(m.doc.behaviors || {});
  // a behavior plays only when every clip it steps through exists (in the saved art)
  function playable(name) {
    const b = (m.doc.behaviors || {})[name];
    if (!b || !Array.isArray(b.seq)) return false;
    const have = clipNames();
    return b.seq.every((st) => have.includes(st.clip));
  }
  // where a behavior name is used outside `behaviors` (intents, idle life, reactions, stations…)
  function refs(name) {
    const out = [];
    const walk = (v, path) => {
      if (typeof v === "string") { if (v === name) out.push(path); return; }
      if (v && typeof v === "object") for (const [k, x] of Object.entries(v)) walk(x, path.concat(Array.isArray(v) ? Number(k) : k));
    };
    for (const [k, v] of Object.entries(m.doc)) if (k !== "behaviors" && k !== "id" && k !== "name") walk(v, [k]);
    return [...new Set(out.map((p) => (p[0] === "idle_pool" ? "idle life" : p.slice(0, 2).filter((x) => typeof x === "string").join("."))))];
  }

  function renderClips() {
    const ed = m.ed;
    const locked = dirty();
    const t = $("groups");
    const clips = (cur() && cur().clips) || [];
    const head = el("tr");
    for (const h of ["Group", "Clips", "Frame", "Used by", ""]) head.append(el("th", "", h));
    const rows = ed.groups.map((g) => {
      const tr = el("tr");
      const name = el("td", "mono", "art/" + g.name);
      const cl = el("td");
      for (const c of g.clips.slice(0, 8)) cl.append(el("span", "chip", c));
      if (g.clips.length > 8) cl.append(el("span", "hint", " +" + (g.clips.length - 8) + " more"));
      for (const s of g.sets) cl.append(el("span", "chip", "set " + s));
      const c0 = clips.find((c) => c.name === g.clips[0]);
      const fr = el("td", "mono", c0 ? c0.w + "×" + c0.h : "—");
      const used = el("td");
      if (g.used_by.length) used.textContent = g.used_by.slice(0, 3).join(", ") + (g.used_by.length > 3 ? " +" + (g.used_by.length - 3) : "");
      else used.append(el("small", "", "nothing yet"));
      const act = el("td", "right");
      const rm = button("Remove", "quiet");
      rm.disabled = locked || !ed.writable || g.used_by.length > 0;
      rm.title = g.used_by.length ? g.used_by.join(", ") + " still play" + (g.used_by.length === 1 ? "s" : "") + " its clips" : locked ? "save or discard your changes first" : "remove art/" + g.name + " and its clips";
      let armed = 0;
      rm.addEventListener("click", async () => {
        if (!armed) {
          rm.textContent = "Remove art/" + g.name + "?";
          rm.className = "btn small danger";
          armed = setTimeout(() => { armed = 0; rm.textContent = "Remove"; rm.className = "btn small quiet"; }, 4000);
          return;
        }
        clearTimeout(armed);
        rm.disabled = true;
        try {
          const r = await call("studio_remove_group", { group: g.name });
          m.impMsg = { text: "Removed art/" + g.name + "." };
          setEdit(r);
          await S.refresh(null);
        } catch (e) {
          m.impMsg = { err: true, text: "Not removed: " + e };
          renderClips();
        }
      });
      act.append(rm);
      tr.append(name, cl, fr, used, act);
      return tr;
    });
    t.replaceChildren(head, ...rows);
    if (!ed.groups.length) { const tr = el("tr"); tr.append(Object.assign(el("td", "hint", "No art yet: add an .aseprite file."), { colSpan: 5 })); t.append(tr); }
    $("imp-picked").hidden = !m.imp;
    if (m.imp) $("imp-label").textContent = m.imp.name + " · " + bytes(m.imp.size);
    $("imp-choose").disabled = locked;
    $("imp-go").disabled = locked || !m.imp;
    const msg = $("imp-msg");
    msg.className = m.impMsg && m.impMsg.err ? "err" : "hint";
    msg.textContent = locked ? "Save or discard your changes before adding or removing art." : m.impMsg ? m.impMsg.text : "Pixels are copied unchanged. Nothing is written unless the whole file checks out.";
  }
  async function pickImport(f) {
    if (!f || dirty()) return;
    try {
      m.imp = { name: f.name, size: f.size, b64: await readFile(f) };
      $("imp-prefix").value = prefixOf(f.name);
      m.impMsg = null;
    } catch (e) {
      m.imp = null;
      m.impMsg = { err: true, text: String(e) };
    }
    renderClips();
  }
  $("imp-choose").addEventListener("click", () => $("imp-file").click());
  $("imp-file").addEventListener("change", (e) => { const f = e.target.files[0]; e.target.value = ""; pickImport(f); });
  const drop = $("studio-import");
  drop.addEventListener("dragover", (e) => { e.preventDefault(); drop.classList.add("over"); });
  drop.addEventListener("dragleave", () => drop.classList.remove("over"));
  drop.addEventListener("drop", (e) => { e.preventDefault(); drop.classList.remove("over"); pickImport(e.dataTransfer.files[0]); });
  $("imp-go").addEventListener("click", async () => {
    if (!m.imp || dirty()) return;
    $("imp-go").disabled = true;
    $("imp-msg").className = "hint";
    $("imp-msg").textContent = "Adding…";
    try {
      const r = await call("studio_import", { name: $("imp-prefix").value.trim(), file_name: m.imp.name, bytes: m.imp.b64 });
      const n = (r.imported || []).length;
      m.impMsg = { text: "Added " + n + (n === 1 ? " clip: " : " clips: ") + (r.imported || []).join(", ") + "." };
      m.imp = null;
      setEdit(r);
      await S.refresh(null);
    } catch (e) {
      m.impMsg = { err: true, text: "Not added: " + e + ". Nothing was written." };
      renderClips();
    }
  });

  const LOOPS = [["once", "once"], ["range", "loops"], ["for", "for seconds"], ["hold", "until moved on"]];
  const loopKind = (l) => (l == null ? "once" : l === "hold" ? "hold" : Array.isArray(l) ? "range" : l && l.for_s ? "for" : "once");
  function renderBehaviors() {
    const B = m.doc.behaviors || {};
    const names = Object.keys(B);
    if (!names.includes(m.bsel)) m.bsel = names[0] || null;
    const list = $("b-list");
    const head = el("tr");
    head.append(el("th", "", "Behavior"), el("th", "", ""));
    list.replaceChildren(head, ...names.map((n) => {
      const tr = el("tr", n === m.bsel ? "sel" : "");
      const ok = playable(n);
      const pill = el("span", "pill " + (ok ? "teal" : "rose"), ok ? (B[n].set ? "set" : "ok") : "clip missing");
      if (!ok) pill.title = "a clip it plays is missing";
      tr.append(el("td", "mono", n), Object.assign(el("td", "right"), {}));
      tr.lastChild.append(pill);
      tr.addEventListener("click", () => { m.bsel = n; renderBehaviors(); const c = B[n].seq && B[n].seq[0]; if (c) previewClip(c.clip); });
      return tr;
    }));
    const box = $("b-edit");
    const n = m.bsel;
    if (!n) return box.replaceChildren(el("p", "hint", "No behaviors yet."));
    const b = B[n];
    const parts = [];
    const f = el("div", "field");
    f.append(el("label", "", "Name"), el("span", "mono", n));
    parts.push(f);
    if (b.set) parts.push(el("p", "hint", "Plays at the set “" + b.set + "”; its clips come from that set."));
    parts.push(el("h4", "", "Plays, in order"));
    const have = clipNames();
    const seq = Array.isArray(b.seq) ? b.seq : [];
    seq.forEach((st, i) => {
      const row = el("div", "step");
      const sel = el("select");
      sel.setAttribute("aria-label", "step " + (i + 1) + " clip");
      const opts = have.slice().sort();
      if (!have.includes(st.clip)) opts.unshift(st.clip);
      sel.append(...opts.map((c) => option(c, c + (have.includes(c) ? "" : " (missing)"), c === st.clip)));
      sel.addEventListener("change", () => setStep(n, i, { ...seq[i], clip: sel.value }));
      const lk = loopKind(st.loops);
      const lsel = el("select");
      lsel.setAttribute("aria-label", "step " + (i + 1) + " plays");
      lsel.append(...LOOPS.map(([v, l]) => option(v, l, v === lk)));
      const range = lk === "range" ? st.loops : lk === "for" ? st.loops.for_s : null;
      const loops = el("span", "loops");
      loops.append(lsel);
      if (range) {
        const a = Object.assign(el("input", "num"), { type: "number", min: 0, value: range[0] });
        const z = Object.assign(el("input", "num"), { type: "number", min: 0, value: range[1] });
        const set = () => {
          const r = [Number(a.value), Number(z.value)];
          if (!r.every(Number.isFinite)) return;
          setStep(n, i, { ...seq[i], loops: lk === "for" ? { for_s: r } : r });
        };
        a.addEventListener("change", set);
        z.addEventListener("change", set);
        loops.append(a, el("span", "hint", "to"), z, el("span", "hint", lk === "for" ? "s" : ""));
      }
      lsel.addEventListener("change", () => {
        const v = lsel.value;
        const s2 = { ...seq[i] };
        if (v === "once") delete s2.loops;
        else s2.loops = v === "hold" ? "hold" : v === "range" ? [1, 2] : { for_s: [20, 60] };
        setStep(n, i, s2);
      });
      const x = button("✕", "quiet", () => { del(["behaviors", n, "seq", i]); renderBehaviors(); });
      x.disabled = seq.length <= 1;
      x.title = seq.length <= 1 ? "a behavior plays at least one clip" : "remove this step";
      row.append(el("span", "n", String(i + 1)), sel, loops, x);
      parts.push(row);
    });
    parts.push(Object.assign(el("div", "row start"), {}));
    parts[parts.length - 1].append(button("+ Add step", "quiet", () => {
      put(["behaviors", n, "seq", seq.length], { clip: seq.length ? seq[seq.length - 1].clip : have[0] });
      renderBehaviors();
    }));
    const facing = el("div", "field");
    const fsel = el("select");
    fsel.setAttribute("aria-label", "facing");
    fsel.append(option("random", "turn to a random direction first", b.turn === "random"), option("keep", "keep facing", b.turn !== "random"));
    fsel.addEventListener("change", () => { if (fsel.value === "random") put(["behaviors", n, "turn"], "random"); else del(["behaviors", n, "turn"]); });
    facing.append(el("label", "", "Facing"), fsel);
    parts.push(facing);
    const used = refs(n);
    const acts = el("div", "row");
    const rm = button("Delete behavior", "quiet danger-text", () => { del(["behaviors", n]); m.bsel = null; renderBehaviors(); });
    rm.disabled = used.length > 0;
    rm.title = used.length ? "still used by " + used.join(", ") : "delete " + n;
    if (used.length) acts.append(el("span", "hint", "used by " + used.slice(0, 4).join(", ") + (used.length > 4 ? " +" + (used.length - 4) : "")));
    acts.append(rm, button("▶ Preview", "", () => previewClip(seq[0] && seq[0].clip)));
    parts.push(acts);
    box.replaceChildren(...parts);
  }
  function setStep(n, i, st) {
    put(["behaviors", n, "seq", i], st);
    renderBehaviors();
  }
  $("b-new").addEventListener("click", () => {
    const name = $("b-new-name").value.trim();
    const why = !/^[a-z][a-z0-9_]{0,47}$/.test(name) ? "Use lowercase letters, digits and _ (starting with a letter)." : behaviorNames().includes(name) ? "There is already a behavior “" + name + "”." : !clipNames().length ? "Add some clips first." : "";
    $("b-new-err").textContent = why;
    if (why) return;
    const first = clipNames().includes("idle") ? "idle" : clipNames()[0];
    put(["behaviors", name], { seq: [{ clip: first }] });
    m.bsel = name;
    $("b-new-name").value = "";
    renderBehaviors();
  });

  function renderIntents() {
    const t = $("i-table");
    const head = el("tr");
    head.append(Object.assign(el("th", "", "When it should"), { style: "width:130px" }), el("th", "", "It plays (first that exists)"));
    const B = m.doc.behaviors || {};
    const rows = m.ed.intents.map(({ name, optional }) => {
      const tr = el("tr");
      const td0 = el("td", "mono", name);
      if (optional) td0.append(el("small", "", " optional"));
      const td = el("td");
      const chain = (m.doc.intents || {})[name];
      if (chain != null && !Array.isArray(chain)) { td.append(el("span", "hint", "edit in Advanced")); tr.append(td0, td); return tr; }
      const list = chain || [];
      const setChain = (c) => { if (!c.length && optional) del(["intents", name]); else put(["intents", name], c); renderIntents(); };
      list.forEach((b, i) => {
        if (i) td.append(el("span", "arrow", "›"));
        const ok = B[b] && playable(b);
        const chip = el("span", "chip" + (ok ? "" : " miss"), b + " ");
        if (!ok) chip.title = B[b] ? b + " can't play: a clip it plays is missing" : "no behavior “" + b + "”";
        const x = el("button", "x", "✕");
        x.type = "button";
        x.setAttribute("aria-label", "remove " + b + " from " + name);
        x.addEventListener("click", () => setChain(list.filter((_, j) => j !== i)));
        chip.append(x);
        td.append(chip);
      });
      const add = el("select", "chip add");
      add.setAttribute("aria-label", "add a behavior to " + name);
      add.append(option("", "+ add", true), ...behaviorNames().filter((b) => !list.includes(b)).map((b) => option(b)));
      add.addEventListener("change", () => { if (add.value) setChain(list.concat(add.value)); });
      td.append(add);
      tr.append(td0, td);
      return tr;
    });
    t.replaceChildren(head, ...rows);
  }

  function renderIdle() {
    const t = $("idle-table");
    const head = el("tr");
    for (const h of ["Behavior", "Weight", "Cooldown", "", ""]) head.append(el("th", "", h));
    const pool = Array.isArray(m.doc.idle_pool) ? m.doc.idle_pool : [];
    const names = behaviorNames();
    const rows = pool.map((e, i) => {
      const tr = el("tr");
      const sel = el("select", "narrow-sel");
      sel.setAttribute("aria-label", "idle " + (i + 1) + " behavior");
      const opts = names.includes(e.behavior) ? names : [e.behavior].concat(names);
      sel.append(...opts.map((b) => option(b, b + (names.includes(b) ? "" : " (missing)"), b === e.behavior)));
      sel.addEventListener("change", () => put(["idle_pool", i, "behavior"], sel.value));
      const num = (key, step, label) => {
        const inp = Object.assign(el("input", "num"), { type: "number", min: 0, step, value: e[key] == null ? "" : e[key] });
        inp.setAttribute("aria-label", "idle " + (i + 1) + " " + label);
        inp.addEventListener("change", () => { const n = Number(inp.value); if (inp.value !== "" && Number.isFinite(n)) put(["idle_pool", i, key], n); });
        return inp;
      };
      const cd = el("td");
      cd.append(num("cooldown_s", 1, "cooldown"), el("span", "hint", " s"));
      const extra = Object.keys(e).filter((k) => !["behavior", "weight", "cooldown_s"].includes(k));
      const ex = el("td", "hint", extra.length ? "+ " + extra.join(", ") : "");
      const x = button("✕", "quiet", () => { del(["idle_pool", i]); renderIdle(); });
      x.setAttribute("aria-label", "remove idle " + (i + 1));
      const tdw = el("td");
      tdw.append(num("weight", 0.1, "weight"));
      const tdb = el("td");
      tdb.append(sel);
      const tdx = el("td", "right");
      tdx.append(x);
      tr.append(tdb, tdw, cd, ex, tdx);
      return tr;
    });
    t.replaceChildren(head, ...rows);
  }
  $("idle-add").addEventListener("click", () => {
    const names = behaviorNames();
    if (!names.length) return;
    const pool = Array.isArray(m.doc.idle_pool) ? m.doc.idle_pool : [];
    put(["idle_pool", pool.length], { behavior: names.includes("rest") ? "rest" : names[0], weight: 1, cooldown_s: 0 });
    renderIdle();
  });

  // ---------- edit: personality (`persona` in pack.json) ----------
  // One preset per way of talking; `persona.default` is the one a new user gets. The caps and the
  // 3–6 voice rules are the loader's (`mascot_core::persona`); the forms only guide, the check decides.
  const SITS = [["greeting", "Hello"], ["done", "Done"], ["error", "Oops"], ["checkin", "Check-in"], ["disagree", "Push back"]];
  const MOMENTS = [["morning", "Morning"], ["evening", "Evening"], ["back", "Back from away"], ["long_session", "Long session"]];
  const PCAP = { name: 24, tagline: 32, identity: 120, rule: 80, like: 32, catchphrase: 40, when: 80, example: 160, line: 100 };
  const PP = (id, ...rest) => ["persona", "presets", id, ...rest];
  const presetIds = () => Object.keys((m.doc.persona && m.doc.persona.presets) || {});
  function starter() {
    const name = [...(m.doc.name || m.ed.id)].slice(0, 24).join("");
    let id = slug(name).replace(/-/g, "_").slice(0, 24) || "main";
    if (!/^[a-z]/.test(id)) id = ("m_" + id).slice(0, 24); // a preset id starts with a letter
    return {
      default: id,
      presets: { [id]: {
        name, tagline: "friendly helper", identity: "A small desk buddy who keeps you company while you work.",
        voice: ["Short and friendly: a sentence or two.", "Plain words, no jargon.", "Honest: says so when something looks wrong."],
        likes: [],
        examples: { greeting: "Hi, {you}! I'm here if you need me.", done: "Done. It works now.", error: "That didn't work. Want me to look into it?", checkin: "You've been at it a while. Maybe a short break?", disagree: "I'd hold off on that: it could break something. Want a safer way?" },
        idle_lines: { morning: ["Morning!"], evening: ["Long day. Nearly there."], back: ["Welcome back."], long_session: ["Still here, still cheering you on."] },
        temperament: { ...(m.doc.temperament || {}) }, style: { playfulness: 0.5, emotes: 0.5 },
      } },
    };
  }
  $("p-add").addEventListener("click", () => { put(["persona"], starter()); m.psel = null; renderPersona(); });
  $("p-remove").addEventListener("click", () => { del(["persona"]); m.psel = null; renderPersona(); });
  $("p-new").addEventListener("click", () => {
    const id = $("p-new-name").value.trim();
    const why = !/^[a-z][a-z0-9_]{0,23}$/.test(id) ? "Use lowercase letters, digits and _ (starting with a letter)." : presetIds().includes(id) ? "There is already a preset “" + id + "”." : "";
    $("p-new-err").textContent = why;
    if (why) return;
    const from = m.doc.persona.presets[m.psel] || {};
    put(PP(id), { ...JSON.parse(JSON.stringify(from)), name: id[0].toUpperCase() + id.slice(1).replace(/_/g, " ") });
    m.psel = id;
    $("p-new-name").value = "";
    renderPersona();
  });

  function renderPersona() {
    const P = m.doc.persona;
    $("p-empty").hidden = !!P;
    $("p-main").hidden = !P;
    if (!P) return;
    const ids = presetIds();
    if (!ids.includes(m.psel)) m.psel = ids.includes(P.default) ? P.default : ids[0] || null;
    renderPresetList();
    const box = $("p-edit");
    const id = m.psel;
    if (!id) return box.replaceChildren(el("p", "hint", "No presets yet: add one on the left."));
    const p = P.presets[id];
    const parts = [];
    const field = (label, ctl, cls) => { const f = el("div", "field" + (cls ? " " + cls : "")); const l = el("label", "", label); if (ctl.id) l.htmlFor = ctl.id; f.append(l, ctl); return f; };
    const text = (key, cap, path, opt) => {
      const i = Object.assign(el("input", "txt"), { id: "p-" + key, maxLength: cap, value: getp(path) == null ? "" : getp(path) });
      if (opt && opt.ph) i.placeholder = opt.ph;
      i.addEventListener("input", () => {
        if (opt && opt.optional && !i.value.trim()) del(path); else put(path, i.value);
        if (key === "name") renderPresetList();
      });
      return i;
    };
    const idf = el("span", "mono", id);
    if (id === P.default) idf.append(" ", el("span", "pill teal", "default"));
    parts.push(field("Id", idf));
    parts.push(field("Name", text("name", PCAP.name, PP(id, "name"))));
    parts.push(field("Tagline", text("tagline", PCAP.tagline, PP(id, "tagline"), { optional: true, ph: "e.g. cheeky sidekick · shown in the tray" })));
    parts.push(field("Who it is", text("identity", PCAP.identity, PP(id, "identity"), { ph: "One line, e.g. a small polar bear who thinks it runs the desk" })));

    // voice: 3 to 6 short rules
    const voice = Array.isArray(p.voice) ? p.voice : [];
    parts.push(el("h4", "", "Voice · " + voice.length + (voice.length === 1 ? " rule" : " rules") + ", 3 to 6"));
    parts.push(...lineList(voice, PP(id, "voice"), PCAP.rule, "voice rule", { min: 3, max: 6, add: "+ Add rule", ph: "e.g. Dry one-liners. Tease kindly, never mean." }));

    // catchphrase: the words, and when it says them
    parts.push(el("h4", "", "Catchphrase"));
    const cp = p.catchphrase || {};
    const cpt = Object.assign(el("input", "txt"), { id: "p-cp", maxLength: PCAP.catchphrase, value: cp.text || "", placeholder: "e.g. told you so" });
    const cpw = Object.assign(el("input", "txt"), { id: "p-cpw", maxLength: PCAP.when, value: cp.when || "", placeholder: "e.g. only when it truly called it" });
    const setCp = () => { if (!cpt.value.trim() && !cpw.value.trim()) del(PP(id, "catchphrase")); else put(PP(id, "catchphrase"), { text: cpt.value, when: cpw.value }); };
    cpt.addEventListener("input", setCp);
    cpw.addEventListener("input", setCp);
    parts.push(field("Says", cpt), field("When", cpw));

    // likes: chips
    parts.push(el("h4", "", "Likes"));
    const likes = Array.isArray(p.likes) ? p.likes : [];
    const lk = el("div", "chips");
    likes.forEach((t, i) => {
      const c = el("span", "chip", t + " ");
      const x = el("button", "x", "✕");
      x.type = "button";
      x.setAttribute("aria-label", "remove " + t);
      x.addEventListener("click", () => { del(PP(id, "likes", i)); renderPersona(); });
      c.append(x);
      lk.append(c);
    });
    const add = Object.assign(el("input", "txt mono inline"), { maxLength: PCAP.like, placeholder: "+ a like, Enter" });
    add.setAttribute("aria-label", "add a like");
    add.addEventListener("keydown", (e) => {
      if (e.key !== "Enter" || !add.value.trim()) return;
      e.preventDefault();
      if (!p.likes) put(PP(id, "likes"), [add.value.trim()]); else put(PP(id, "likes", likes.length), add.value.trim());
      renderPersona();
      const again = document.querySelector('#p-edit .chips input');
      if (again) again.focus();
    });
    lk.append(add);
    parts.push(lk);

    // what it says, by situation: the Personality window shows these as its sample bubble
    parts.push(el("h4", "", "Example lines"));
    parts.push(el("p", "hint", "Bubble length, in its voice. {you} is what the person asked to be called, {catchphrase} its catchphrase. The model sees these as examples, and the Personality window shows them as the sample."));
    // the backend's list (a pack with no body block still has the built-in emotes), else the block itself
    const emotes = (m.ed && m.ed.emotes) || (m.doc.body && m.doc.body.emotes) || {};
    for (const [k, label] of SITS) {
      const path = PP(id, "examples", k);
      const ta = Object.assign(el("textarea", "sel"), { id: "p-ex-" + k, maxLength: PCAP.example, rows: 2, value: getp(path) || "" });
      ta.dataset.pfield = "examples." + k;
      const cnt = el("span", "count", "");
      const show = () => { cnt.textContent = [...ta.value].length + " / " + PCAP.example; };
      show();
      ta.addEventListener("input", () => { show(); put(path, ta.value); });
      const col = el("div", "");
      const g = gestureRow(id, k, emotes);
      g.append(cnt);
      col.append(ta, g);
      parts.push(field(label, col, "top"));
      parts[parts.length - 1].querySelector("label").htmlFor = ta.id;
    }

    // canned lines it says on its own (no model call)
    parts.push(el("h4", "", "Idle lines"));
    parts.push(el("p", "hint", "Said now and then with no model call, so they carry mood only, never facts. One is picked at random; it never repeats the last few."));
    for (const [k, label] of MOMENTS) {
      const path = PP(id, "idle_lines", k);
      const lines = Array.isArray(getp(path)) ? getp(path) : [];
      const col = el("div", "");
      col.append(...lineList(lines, path, PCAP.line, label.toLowerCase() + " line", { min: 1, max: 8, add: "+ Line", ph: "" }));
      parts.push(field(label, col, "top"));
    }

    // temperament and the window's sliders
    parts.push(el("h4", "", "Temperament"));
    parts.push(el("p", "hint", "Replaces the mascot's temperament (Details) while this preset is on: it moves the body as well as the voice."));
    const tbox = el("div", "narrow");
    const base = m.doc.temperament || {};
    tbox.append(...TEMPER.map((k) => slider(k, (p.temperament || {})[k] != null ? p.temperament[k] : base[k] != null ? base[k] : 0.5, (v) => put(PP(id, "temperament", k), v), !p.temperament)));
    parts.push(tbox);
    parts.push(el("h4", "", "Where the window's sliders start"));
    const sbox = el("div", "narrow");
    sbox.append(...["playfulness", "emotes"].map((k) => slider(k === "emotes" ? "gestures" : k, (p.style || {})[k] != null ? p.style[k] : 0.5, (v) => put(PP(id, "style", k), v), !p.style)));
    parts.push(sbox);

    // the preset itself
    const acts = el("div", "row");
    const isDef = id === P.default;
    const mk = button("Make default", "quiet", () => { put(["persona", "default"], id); renderPersona(); });
    mk.disabled = isDef;
    const rm = button("Delete preset", "quiet danger-text", () => { del(PP(id)); m.psel = null; renderPersona(); });
    rm.disabled = isDef;
    rm.title = isDef ? "the default can't be deleted: make another preset the default first" : "delete " + id;
    if (isDef) acts.append(el("span", "hint", "new users start with this one"));
    acts.append(mk, rm);
    parts.push(acts);
    box.replaceChildren(...parts);
  }
  function renderPresetList() {
    const P = m.doc.persona;
    const head = el("tr");
    head.append(el("th", "", "Preset"), el("th", "", ""));
    $("p-list").replaceChildren(head, ...presetIds().map((id) => {
      const q = P.presets[id] || {};
      const tr = el("tr", id === m.psel ? "sel" : "");
      const td = el("td", "");
      td.append(el("b", "", q.name || id), el("small", "", id + (q.tagline ? " · " + q.tagline : "")));
      const tdr = el("td", "right");
      if (id === P.default) tdr.append(el("span", "pill teal", "default"));
      tr.append(td, tdr);
      tr.tabIndex = 0;
      tr.addEventListener("click", () => { m.psel = id; renderPersona(); });
      tr.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); m.psel = id; renderPersona(); } });
      return tr;
    }));
  }
  // a numbered list of one-line inputs with ✕ and + (voice rules, idle lines)
  function lineList(list, path, cap, what, o) {
    const rows = list.map((t, i) => {
      const row = el("div", "line");
      const inp = Object.assign(el("input", "txt"), { maxLength: cap, value: t, placeholder: o.ph || "" });
      inp.setAttribute("aria-label", what + " " + (i + 1));
      inp.addEventListener("input", () => put(path.concat(i), inp.value));
      const x = button("✕", "quiet", () => { del(path.concat(i)); renderPersona(); });
      x.disabled = list.length <= o.min;
      x.title = list.length <= o.min ? "at least " + o.min : "remove";
      x.setAttribute("aria-label", "remove " + what + " " + (i + 1));
      row.append(el("span", "n", String(i + 1)), inp, x);
      return row;
    });
    const r = el("div", "row start");
    const b = button(o.add, "quiet", () => {
      put(Array.isArray(getp(path)) ? path.concat(list.length) : path, Array.isArray(getp(path)) ? "" : [""]);
      renderPersona();
      const ins = document.querySelectorAll("#p-edit input[aria-label^='" + what + " ']");
      if (ins.length) ins[ins.length - 1].focus();
    });
    b.disabled = list.length >= o.max;
    if (b.disabled) b.title = "at most " + o.max;
    r.append(b);
    return rows.concat(r);
  }
  // the gesture it does with a situation's line: an emote from the pack's body block
  function gestureRow(id, sit, emotes) {
    const path = PP(id, "gestures", sit);
    const cur = getp(path);
    const sel = el("select", "narrow-sel");
    sel.setAttribute("aria-label", sit + " gesture");
    sel.append(option("", "no gesture", !cur));
    if (cur && !emotes[cur]) sel.append(option(cur, cur + " (missing)", true));
    const groups = {};
    for (const [k, e] of Object.entries(emotes)) (groups[e.group || "other"] = groups[e.group || "other"] || []).push(k);
    for (const g of ["gesture", "fidget", "move", "surprise", "other"].filter((g) => groups[g]).concat(Object.keys(groups).filter((g) => !["gesture", "fidget", "move", "surprise", "other"].includes(g)))) {
      const og = el("optgroup");
      og.label = g + "s";
      og.append(...groups[g].map((k) => option(k, k, k === cur)));
      sel.append(og);
    }
    const about = el("span", "hint about", cur && emotes[cur] && emotes[cur].about ? emotes[cur].about : "");
    sel.addEventListener("change", () => {
      if (sel.value) put(path, sel.value); else del(path);
      about.textContent = sel.value && emotes[sel.value] && emotes[sel.value].about ? emotes[sel.value].about : "";
    });
    const r = el("div", "gest");
    r.append(el("span", "hint", "with"), sel, about);
    if (!Object.keys(emotes).length) { sel.disabled = true; about.textContent = "no emotes: the body block (Advanced) lists them"; }
    return r;
  }
  function slider(label, v, set, inherited) {
    const row = el("div", "slider");
    const lab = el("span", "mono", v.toFixed(2).replace(/^0/, ""));
    const r = el("input");
    Object.assign(r, { type: "range", min: 0, max: 100, value: Math.round(v * 100) });
    r.setAttribute("aria-label", label);
    r.addEventListener("input", () => { lab.textContent = (r.value / 100).toFixed(2).replace(/^0/, ""); });
    r.addEventListener("change", () => set(Number(r.value) / 100));
    if (inherited) row.title = "not set: from the mascot";
    row.append(el("span", "", label), r, lab);
    return row;
  }

  function renderPackage() {
    const ed = m.ed;
    const box = $("pkg-check");
    const parts = [];
    if (!ed.writable) parts.push(el("span", "hint", "Read-only here."));
    else if (!ed.findings.length) parts.push(el("span", "ok", "✓ Standard mascot package"), document.createTextNode(" · " + ed.files + " files · " + bytes(ed.bytes) + " · everything it reads is inside the folder"));
    else {
      parts.push(el("span", "err", "✕ Not a standard package yet (" + ed.findings.length + ")"));
      for (const f of ed.findings) parts.push(el("div", "issue", f.path + " · " + f.problem));
    }
    const row = el("div", "row start");
    row.append(button("Show in Finder", "", () => go("reveal")), button("Check again", "quiet", loadEdit));
    parts.push(row);
    box.replaceChildren(...parts);
    const tree = $("pkg-tree");
    const line = (text, note) => { tree.append(document.createTextNode(text.padEnd(26))); if (note) tree.append(el("em", "", note)); tree.append(document.createTextNode("\n")); };
    tree.replaceChildren();
    line((ed.folder || ed.id) + "/", "folder name = the mascot's id");
    line("├─ pack.json", "the manifest: details, behaviors, reactions, idle life");
    line("├─ art/", "all art: one folder per clip group");
    ed.groups.forEach((g, i) => line("│  " + (i === ed.groups.length - 1 ? "└─ " : "├─ ") + g.name + "/", g.files + " files · " + bytes(g.bytes)));
    line("├─ palettes/", "optional: the colours lint allows");
    line("└─ review/", "studio comments (not part of the mascot)");
  }

  function renderAdvanced() {
    const t = $("raw");
    const v = m.raw !== null ? m.raw : m.ed.text;
    if (t.value !== v) t.value = v;
    t.readOnly = m.ops.length > 0 || !m.ed.writable;
  }
  $("raw").addEventListener("input", () => {
    const v = $("raw").value;
    m.raw = v === m.ed.text ? null : v;
    changed();
  });

  // ---------- edit: the live preview (the saved pack, through studio_frames) ----------
  function stopPreview() {
    clearTimeout(m.pv.timer);
    m.pv.timer = 0;
  }
  function startPreview() {
    const p = cur();
    if (!p) return;
    const names = p.clips.map((c) => c.name);
    if (!names.includes(m.pv.clip)) m.pv.clip = names.includes("idle") ? "idle" : names[0];
    const sel = $("ed-clip");
    sel.replaceChildren(...p.clips.map((c) => option(c.name, c.name, c.name === m.pv.clip)));
    loadPreview();
  }
  function previewClip(name) {
    const p = cur();
    if (!name || !p || !p.clips.some((c) => c.name === name)) return;
    m.pv.clip = name;
    $("ed-clip").value = name;
    loadPreview();
  }
  $("ed-clip").addEventListener("change", (e) => { m.pv.clip = e.target.value; loadPreview(); });
  async function loadPreview() {
    stopPreview();
    const p = cur();
    const c = p && p.clips.find((x) => x.name === m.pv.clip);
    if (!c) return;
    const all = c.set ? ["A", "B"] : ["S", "SE", "E", "NE", "N", "NW", "W", "SW"];
    if (!c.views.includes(m.pv.dir)) m.pv.dir = ["S", "SE", "A"].find((v) => c.views.includes(v)) || c.views[0];
    const dirs = $("ed-dirs");
    dirs.replaceChildren(...all.map((d) => {
      const b = el("span", d === m.pv.dir ? "on" : c.views.includes(d) ? "" : "off", d);
      if (c.views.includes(d)) b.addEventListener("click", () => { m.pv.dir = d; loadPreview(); });
      return b;
    }));
    const key = m.pv.clip + "|" + m.pv.dir;
    try {
      const r = await call("studio_frames", { clip: m.pv.clip, dir: m.pv.dir, eyes: "open", mouth: "closed" });
      const imgs = await Promise.all(r.frames.map((src) => new Promise((ok) => { const i = new Image(); i.onload = () => ok(i); i.onerror = () => ok(i); i.src = src; })));
      if (key !== m.pv.clip + "|" + m.pv.dir || S.view() !== "edit") return;
      m.pv.frames = { imgs, durs: r.durs, w: r.w, h: r.h };
      m.pv.i = 0;
      tick();
    } catch (e) {
      m.pv.frames = null;
    }
  }
  function tick() {
    const f = m.pv.frames;
    const cv = $("ed-view");
    if (!f || !f.imgs.length) return;
    const z = Math.max(1, Math.min(3, Math.floor(220 / Math.max(f.w, f.h * 0.8))));
    if (cv.width !== f.w * z) { cv.width = f.w * z; cv.height = f.h * z; }
    const g = cv.getContext("2d");
    g.imageSmoothingEnabled = false;
    g.clearRect(0, 0, cv.width, cv.height);
    const img = f.imgs[m.pv.i];
    if (img && img.naturalWidth) g.drawImage(img, 0, 0, cv.width, cv.height);
    const d = f.durs[m.pv.i] || 83;
    m.pv.i = (m.pv.i + 1) % f.imgs.length;
    if (f.imgs.length > 1) m.pv.timer = setTimeout(() => { if (!document.hidden) tick(); else m.pv.timer = setTimeout(tick, 500); }, d);
  }

  // the view may already be on screen when this file runs
  if (S.view()) S.hooks.view(S.view());
})();
