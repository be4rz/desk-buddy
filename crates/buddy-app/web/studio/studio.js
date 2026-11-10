// The studio window (design `## studio`). It only shows and checks the art: every command goes
// through the `studio_*` Tauri commands, which render with the app's own renderer. Paths never go
// from here to Rust: packs are opened by id, comments carry only clip/dir/frame/time/region/text.
// The poll (500 ms) and the preview loop live only as long as this window.
"use strict";
(function () {
  const T = window.__TAURI__;
  const $ = (id) => document.getElementById(id);
  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  };
  const call = (cmd, args) => T.core.invoke(cmd, { args: args || {} });
  const DIRS = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
  const TEMPER = ["energy", "curiosity", "shyness", "chattiness", "sleepiness"];
  const POLL_MS = 500;
  const SIM_MS = 250;

  const s = {
    st: null, // studio_state
    clip: null, dir: "SE", eyes: "open", mouth: "closed", zoom: 2,
    anim: null, // { key, w, h, durs, loop, imgs }
    frame: 0, playing: true, acc: 0, last: 0, raf: 0,
    comments: [], draft: null, // draft region [x,y,w,h]
    lint: null, tab: "comments",
    sim: null, simTimer: 0, senses: {},
  };
  const cache = new Map();
  let cacheGen = 0; // bumped on every clear: a render begun before a reload never refills it
  const clearCache = () => { cache.clear(); cacheGen++; };

  // ---------- state ----------
  async function loadState() {
    s.st = await call("studio_state");
    render();
    if (s.st.pack) {
      await Promise.all([loadFrames(), loadLint(), loadComments(), simOp({ op: "tick", dt_ms: 0 })]);
    }
  }

  function render() {
    const st = s.st;
    banner(st && st.error);
    const open = !!(st && st.pack);
    $("studio").hidden = !open;
    $("studio-empty").hidden = open;
    if (!open) return renderEmpty();
    const p = st.pack;
    $("title").textContent = "Studio — " + p.id + "  ·  " + p.dir;
    // pack picker
    const ps = $("pack");
    ps.replaceChildren(...(st.packs || [p.id]).map((id) => { const o = el("option", "", id); o.value = id; o.selected = id === p.id; return o; }));
    // clip picker
    const names = p.clips.map((c) => c.name);
    if (!names.includes(s.clip)) s.clip = names.includes("idle") ? "idle" : names[0];
    const cs = $("clip");
    cs.replaceChildren(...p.clips.map((c) => {
      const o = el("option", "", c.name + (c.set ? " (set: " + c.set + ")" : "") + (c.loop ? "" : " · once"));
      o.value = c.name;
      o.selected = c.name === s.clip;
      return o;
    }));
    renderDirs();
    $("face-box").hidden = !p.face;
    seg($("eyes"), ["open", "closed"], s.eyes, (v) => { s.eyes = v; loadFrames(); }, (v) => "eyes " + v);
    seg($("mouth"), ["closed", "open", "wide"], s.mouth, (v) => { s.mouth = v; loadFrames(); }, (v) => "mouth " + v);
    seg($("zoom"), [1, 2, 3, 4], s.zoom, (v) => { s.zoom = v; draw(); }, (v) => "×" + v);
    renderTemper(p.temperament);
  }

  function clipMeta() {
    return s.st && s.st.pack ? s.st.pack.clips.find((c) => c.name === s.clip) : null;
  }

  function renderDirs() {
    const c = clipMeta();
    const views = c ? c.views : [];
    const all = c && c.set ? ["A", "B"] : DIRS;
    if (!views.includes(s.dir)) s.dir = views.includes("SE") ? "SE" : views[0] || "SE";
    seg($("dirs"), all, s.dir, (v) => { s.dir = v; loadFrames(); }, String, (v) => !views.includes(v));
  }

  function seg(box, values, cur, pick, label, off) {
    box.replaceChildren(...values.map((v) => {
      const b = el("span", v === cur ? "on" : off && off(v) ? "off" : "", label(v));
      b.setAttribute("role", "button");
      if (!(off && off(v))) b.addEventListener("click", () => { pick(v); seg(box, values, v, pick, label, off); });
      return b;
    }));
  }

  function renderTemper(t) {
    const box = $("temper");
    if (box.childElementCount) return;
    for (const k of TEMPER) {
      const row = el("div", "slider");
      const v = el("span", "mono", (t[k] || 0).toFixed(2).replace(/^0/, ""));
      const r = el("input");
      Object.assign(r, { type: "range", min: 0, max: 100, value: Math.round((t[k] || 0) * 100) });
      r.setAttribute("aria-label", k);
      r.dataset.k = k;
      r.addEventListener("input", () => { v.textContent = (r.value / 100).toFixed(2).replace(/^0/, ""); });
      r.addEventListener("change", () => simOp({ op: "temperament", temperament: temper() }));
      row.append(el("span", "", k), r, v);
      box.append(row);
    }
  }
  function temper() {
    const t = {};
    for (const r of $("temper").querySelectorAll("input")) t[r.dataset.k] = r.value / 100;
    return t;
  }

  function renderEmpty() {
    const st = s.st || {};
    const list = $("pack-list");
    const packs = st.packs || [];
    list.replaceChildren(...packs.map((id, i) => {
      const b = el("button", "btn" + (i === 0 ? " primary" : ""), "Open " + id);
      b.addEventListener("click", () => openPack(id));
      return b;
    }));
    if (!packs.length) list.append(el("span", "hint", "No pack folders found next to the app."));
    $("empty-error").textContent = st.error ? "Couldn't open the pack: " + st.error : "";
  }

  async function openPack(id) {
    try {
      s.st = await call("studio_open", { id });
      clearCache();
      $("temper").replaceChildren();
      render();
      await Promise.all([loadFrames(), loadLint(), loadComments(), simOp({ op: "reset", seed: 1 })]);
    } catch (e) {
      banner("Couldn't open " + id + ": " + e);
    }
  }

  function banner(err) {
    const b = $("studio-lint-error");
    b.hidden = !err;
    document.body.classList.toggle("has-error", !!err);
    if (err) b.textContent = "Reload failed: " + err + (s.st && s.st.pack ? ". Showing the last good pack." : "");
  }

  // ---------- frames and preview ----------
  // Shows the current pick's frames; false when a newer pick or a reload overtook this call
  // (that call shows its own).
  async function loadFrames() {
    if (!s.clip) return false;
    const key = [s.clip, s.dir, s.eyes, s.mouth].join("|");
    let a = cache.get(key);
    if (!a) {
      const gen = cacheGen;
      const r = await call("studio_frames", { clip: s.clip, dir: s.dir, eyes: s.eyes, mouth: s.mouth });
      const imgs = await Promise.all(r.frames.map((src) => new Promise((ok) => { const i = new Image(); i.onload = () => ok(i); i.onerror = () => ok(i); i.src = src; })));
      if (gen !== cacheGen) return false;
      a = { key, w: r.w, h: r.h, durs: r.durs, loop: r.loop, imgs };
      cache.set(key, a);
      if ([s.clip, s.dir, s.eyes, s.mouth].join("|") !== key) return false;
    }
    const same = s.anim && s.anim.key.split("|")[0] === s.clip;
    s.anim = a;
    if (!same || s.frame >= a.imgs.length) s.frame = 0;
    $("loop").textContent = a.loop ? "⟲" : "once";
    renderTicks();
    draw();
    play(s.playing);
    return true;
  }

  function draw() {
    const a = s.anim;
    const c = $("view");
    if (!a) return;
    const z = s.zoom;
    if (c.width !== a.w * z || c.height !== a.h * z) { c.width = a.w * z; c.height = a.h * z; }
    const g = c.getContext("2d");
    g.imageSmoothingEnabled = false;
    g.clearRect(0, 0, c.width, c.height);
    const img = a.imgs[s.frame];
    if (img && img.naturalWidth) g.drawImage(img, 0, 0, a.w * z, a.h * z);
    $("fpos").textContent = s.frame + "/" + a.imgs.length;
    const n = a.imgs.length;
    $("head").style.left = (n > 1 ? (s.frame / (n - 1)) * 100 : 0) + "%";
    marks();
    cli();
  }

  function tMs(frame) {
    const d = s.anim ? s.anim.durs : [];
    let t = 0;
    for (let i = 0; i < frame && i < d.length; i++) t += d[i];
    return t;
  }

  function play(on) {
    s.playing = on;
    $("play").textContent = on ? "❚❚" : "▶";
    cancelAnimationFrame(s.raf);
    s.raf = 0;
    if (on && s.anim && s.anim.imgs.length > 1) { s.last = performance.now(); s.acc = 0; s.raf = requestAnimationFrame(step); }
  }

  function step(now) {
    const a = s.anim;
    if (!a || !s.playing) return;
    s.acc += now - s.last;
    s.last = now;
    let moved = false;
    while (s.acc >= (a.durs[s.frame] || 83)) {
      s.acc -= a.durs[s.frame] || 83;
      if (s.frame + 1 < a.imgs.length) s.frame += 1;
      else if (a.loop) s.frame = 0;
      else { draw(); return play(false); }
      moved = true;
    }
    if (moved) draw();
    s.raf = requestAnimationFrame(step);
  }

  function renderTicks() {
    const box = $("ticks");
    const n = s.anim ? s.anim.imgs.length : 0;
    box.querySelectorAll(".tick,.cm").forEach((e) => e.remove());
    for (let i = 1; i < n - 1; i++) { const t = el("div", "tick"); t.style.left = (i / (n - 1)) * 100 + "%"; box.append(t); }
    for (const c of s.comments) {
      if (c.clip !== s.clip || c.dir !== s.dir || c.frame >= n) continue;
      const m = el("div", "cm");
      m.title = "#" + num(c) + " " + c.text;
      m.style.left = (n > 1 ? (c.frame / (n - 1)) * 100 : 0) + "%";
      box.append(m);
    }
  }

  $("ticks").addEventListener("click", (e) => {
    const n = s.anim ? s.anim.imgs.length : 0;
    if (!n) return;
    const r = e.currentTarget.getBoundingClientRect();
    play(false);
    s.frame = Math.max(0, Math.min(n - 1, Math.round(((e.clientX - r.left) / r.width) * (n - 1))));
    draw();
  });
  $("play").addEventListener("click", () => play(!s.playing));
  addEventListener("keydown", (e) => {
    if (e.target.closest("textarea,input,select") || !s.anim) return;
    const n = s.anim.imgs.length;
    if (e.key === " ") { e.preventDefault(); play(!s.playing); }
    else if (e.key === "ArrowRight") { play(false); s.frame = (s.frame + 1) % n; draw(); }
    else if (e.key === "ArrowLeft") { play(false); s.frame = (s.frame + n - 1) % n; draw(); }
  });
  addEventListener("resize", () => marks());

  // pins and regions, in frame pixels
  function box(region, cls, label) {
    const c = $("view");
    const z = s.zoom;
    const [x, y, w, h] = region;
    const left = c.offsetLeft + x * z, top = c.offsetTop + y * z;
    if (w <= 1 && h <= 1) {
      const p = el("div", "pin" + cls);
      p.append(el("b", "", label));
      p.style.left = left + z / 2 + "px";
      p.style.top = top + z / 2 + "px";
      return p;
    }
    const r = el("div", "region" + cls);
    Object.assign(r.style, { left: left + "px", top: top + "px", width: w * z + "px", height: h * z + "px" });
    return r;
  }

  function marks() {
    const m = $("marks");
    m.replaceChildren();
    if (!s.anim) return;
    for (const c of s.comments) {
      if (c.region && c.clip === s.clip && c.dir === s.dir && c.frame === s.frame && c.status === "open") m.append(box(c.region, "", String(num(c))));
    }
    if (s.draft) m.append(box(s.draft, " draft", "+"));
    noteAt();
  }

  function framePx(e) {
    const r = $("view").getBoundingClientRect();
    const a = s.anim;
    return [Math.max(0, Math.min(a.w - 1, Math.floor(((e.clientX - r.left) / r.width) * a.w))), Math.max(0, Math.min(a.h - 1, Math.floor(((e.clientY - r.top) / r.height) * a.h)))];
  }

  let down = null;
  $("view").addEventListener("mousedown", (e) => {
    if (!s.anim) return;
    e.preventDefault();
    play(false);
    down = framePx(e);
  });
  addEventListener("mousemove", (e) => {
    if (!down) return;
    const [x, y] = framePx(e);
    s.draft = [Math.min(x, down[0]), Math.min(y, down[1]), Math.abs(x - down[0]) + 1, Math.abs(y - down[1]) + 1];
    marks();
  });
  addEventListener("mouseup", (e) => {
    if (!down) return;
    const [x, y] = framePx(e);
    s.draft = x === down[0] && y === down[1] ? [x, y, 1, 1] : [Math.min(x, down[0]), Math.min(y, down[1]), Math.abs(x - down[0]) + 1, Math.abs(y - down[1]) + 1];
    down = null;
    marks();
    $("note").focus();
  });

  // ---------- comments ----------
  const num = (c) => s.comments.indexOf(c) + 1;
  async function loadComments() {
    try {
      s.comments = (await call("studio_comments")).comments || [];
    } catch (e) {
      s.comments = [];
      $("note-at").textContent = String(e);
    }
    renderComments();
  }

  function where(c) {
    return c.clip + " " + c.dir + " f" + c.frame + " · " + c.t_ms + " ms" + (c.region ? (c.region[2] <= 1 && c.region[3] <= 1 ? " · pin" : " · region") : "");
  }

  function renderComments() {
    const box = $("comments");
    box.replaceChildren(...s.comments.map((c, i) => {
      const d = el("div", "comment" + (c.status === "resolved" ? " resolved" : "") + (c.clip === s.clip && c.dir === s.dir ? " here" : ""));
      const h = el("div", "h");
      const st = el("button", "pill " + (c.status === "open" ? "rose" : "teal"), c.status);
      st.title = c.status === "open" ? "mark resolved" : "reopen";
      st.addEventListener("click", async (e) => {
        e.stopPropagation();
        try {
          await call("studio_comment_status", { id: c.id, status: c.status === "open" ? "resolved" : "open" });
          await loadComments();
        } catch (err) { $("note-at").textContent = String(err); }
      });
      h.append(el("span", "", "#" + (i + 1) + " " + where(c)), st);
      d.append(h, document.createTextNode(c.text));
      d.addEventListener("click", () => jump(c));
      return d;
    }));
    if (!s.comments.length) box.append(el("p", "hint", "No comments yet. They are saved in the pack's review/comments.jsonl."));
    renderTicks();
    marks();
  }

  async function jump(c) {
    const meta = s.st.pack.clips.find((x) => x.name === c.clip);
    if (!meta) return;
    play(false);
    s.clip = c.clip;
    s.dir = c.dir;
    render();
    if (!(await loadFrames())) return;
    s.frame = Math.min(c.frame, s.anim.imgs.length - 1);
    draw();
  }

  function noteAt() {
    if (!s.anim) return;
    const r = s.draft;
    $("note-at").textContent = s.clip + " " + s.dir + " f" + s.frame + " · " + tMs(s.frame) + " ms" + (r ? (r[2] <= 1 && r[3] <= 1 ? " · pin " + r[0] + "," + r[1] : " · region " + r[0] + "," + r[1] + " " + r[2] + "×" + r[3]) : "");
  }

  $("note-clear").addEventListener("click", () => { s.draft = null; marks(); });
  $("note-add").addEventListener("click", async () => {
    const text = $("note").value.trim();
    if (!text || !s.anim) return;
    try {
      await call("studio_comment_add", { clip: s.clip, dir: s.dir, frame: s.frame, t_ms: tMs(s.frame), region: s.draft, text });
      $("note").value = "";
      s.draft = null;
      await loadComments();
    } catch (e) {
      $("note-at").textContent = "Not saved: " + e;
    }
  });

  // ---------- lint ----------
  async function loadLint() {
    try {
      s.lint = await call("studio_lint");
    } catch (e) {
      s.lint = null;
      $("lint-sum").textContent = String(e);
    }
    renderLint();
  }

  function renderLint() {
    const l = s.lint;
    const tab = $("lint-tab");
    if (!l) { tab.textContent = "Lint"; return; }
    tab.textContent = l.errors ? "Lint · " + l.errors + (l.errors === 1 ? " error" : " errors") : "Lint";
    $("lint-sum").textContent = l.files + " files · " + l.errors + " errors · " + l.warnings + " warnings";
    const issues = [...l.issues].sort((a, b) => (a.severity === b.severity ? 0 : a.severity === "error" ? -1 : 1));
    $("issues").replaceChildren(...issues.map((i) => {
      const d = el("div", "issue" + (i.severity === "error" ? "" : " warn"));
      const at = i.file + (i.frame != null ? " f" + i.frame : "") + (i.x != null ? " (" + i.x + "," + i.y + ")" : "");
      d.append(el("code", "", i.code), el("span", "", at + " " + i.detail));
      return d;
    }));
    for (const code of ["stray_pixel", "off_palette"]) {
      if (!issues.some((i) => i.code === code)) {
        const d = el("div", "issue");
        d.append(el("code", "", code), el("span", "mono hint", "none"));
        $("issues").append(d);
      }
    }
    cli();
  }
  $("lint-run").addEventListener("click", loadLint);

  // ---------- audit ----------
  $("audit-run").addEventListener("click", async () => {
    const minutes = Number($("audit-min").value) || 30;
    const seed = Math.max(0, Math.floor(Number($("audit-seed").value) || 0));
    $("audit-h").textContent = "Audit · running…";
    try {
      const a = await call("studio_audit", { minutes, seed });
      $("audit-h").textContent = "Audit · " + a.minutes + " min · seed " + a.seed;
      const order = a.order && a.order.length ? a.order : Object.keys(a.plays);
      const max = Math.max(1, ...order.map((b) => a.plays[b] || 0));
      $("chart").replaceChildren(...order.map((b) => {
        const bar = el("div");
        bar.style.height = Math.max(1, ((a.plays[b] || 0) / max) * 100) + "%";
        bar.title = b + ": " + (a.plays[b] || 0);
        bar.append(el("span", "", b));
        return bar;
      }));
      $("audit-sum").textContent = "distinct " + a.distinct + " · max same in a row " + a.max_same_in_row + " · entropy " + a.entropy + " · starts " + a.starts;
      const img = $("audit-png");
      img.src = a.chart;
      img.hidden = false;
    } catch (e) {
      $("audit-h").textContent = "Audit";
      $("audit-sum").textContent = String(e);
    }
    cli();
  });

  // ---------- simulator ----------
  const EVENTS = [
    ["thinking", { type: "thinking" }],
    ["working:web", { type: "working", kind: "web", tool: "curl" }],
    ["awaiting_permission", { type: "awaiting_permission", callId: "sim", title: "Run a command?" }],
    ["speaking", { type: "speaking", text: "Hello there" }],
    ["error", { type: "error", reason: "sim", message: "the tool failed" }],
    ["say happy", { type: "say", text: "Yay", emotion: "happy" }],
    ["emote wave", { type: "emote", name: "wave" }],
    ["reminder", { type: "reminder", id: "sim", text: "Stretch" }],
    ["idle", { type: "idle" }],
    ["settled", { type: "settled" }],
  ];
  const SENSES = [
    ["DND", "dnd_toggle", true, false],
    ["quiet hours", "quiet_hours", true, false],
    ["fullscreen", "fullscreen", true, false],
    ["screen share", "screen_share", true, false],
    ["front: Game", "frontmost", { bundle_id: "sim.game", category: "game" }, null],
    ["idle 6 min", "user_idle_ms", 360000, 0],
  ];
  $("events").replaceChildren(
    ...EVENTS.map(([label, ev]) => { const b = el("span", "", label); b.addEventListener("click", () => simOp({ op: "event", event: ev })); return b; }),
    ...[["poke", "click"], ["drag", "drag_start"], ["drop", "drag_end"]].map(([label, p]) => { const b = el("span", "", label); b.addEventListener("click", () => simOp({ op: "poke", poke: p })); return b; }),
  );
  $("senses").replaceChildren(...SENSES.map(([label, k, on, off]) => {
    const b = el("span", "", label);
    b.addEventListener("click", async () => {
      const now = !s.senses[k];
      if (await simOp({ op: "senses", patch: { [k]: now ? on : off } })) { s.senses[k] = now; b.classList.toggle("on", now); }
    });
    return b;
  }));
  $("timeline").value = JSON.stringify({ timeline_version: 1, seed: 5, duration_ms: 20000, start_local: "10:00", weekday: 0, items: [{ at_ms: 1000, event: { type: "thinking" } }, { at_ms: 4000, event: { type: "say", text: "Done!", emotion: "happy" } }, { at_ms: 9000, event: { type: "idle" } }] }, null, 1);

  async function simOp(a) {
    try {
      const r = await call("studio_sim", a);
      $("sim-err").textContent = "";
      showSim(r);
      return true;
    } catch (e) {
      $("sim-err").textContent = String(e);
      return false;
    }
  }

  function showSim(r) {
    const prev = s.sim && s.sim.show;
    s.sim = r;
    const sh = r.show;
    $("sim-show").textContent = "t " + (r.t_ms / 1000).toFixed(1) + " s · " + sh.intent + " · " + sh.behavior + " · " + sh.clip + " " + (sh.ori || sh.dir) + " f" + sh.frame + " · eyes " + sh.eyes + " · mouth " + sh.mouth + (sh.effect ? " · " + sh.effect : "") + " · " + sh.visibility + (r.pending ? " · " + r.pending + " pending" : "");
    $("trace").textContent = (r.trace || []).map((l) => (l.t_ms / 1000).toFixed(1).padStart(6) + "  " + l.what.padEnd(5) + " " + l.intent + " " + l.behavior + (l.note ? "  " + l.note : "")).join("\n");
    if ($("sim-follow").checked && sh && (!prev || prev.clip !== sh.clip || prev.dir !== sh.dir || prev.ori !== sh.ori || prev.frame !== sh.frame || prev.eyes !== sh.eyes || prev.mouth !== sh.mouth)) follow(sh);
  }

  async function follow(sh) {
    const view = sh.ori || sh.dir;
    play(false);
    if (s.clip !== sh.clip || s.dir !== view || s.eyes !== sh.eyes || s.mouth !== sh.mouth) {
      Object.assign(s, { clip: sh.clip, dir: view, eyes: sh.eyes, mouth: sh.mouth });
      render();
      if (!(await loadFrames())) return;
    }
    if (s.anim) { s.frame = Math.min(sh.frame, s.anim.imgs.length - 1); draw(); }
  }

  function simRun(on) {
    clearInterval(s.simTimer);
    s.simTimer = 0;
    $("sim-run").textContent = on ? "Pause" : "Run";
    if (on) s.simTimer = setInterval(() => { if (!document.hidden) simOp({ op: "tick", dt_ms: SIM_MS }); }, SIM_MS);
  }
  $("sim-run").addEventListener("click", () => simRun(!s.simTimer));
  $("sim-reset").addEventListener("click", () => {
    s.senses = {};
    $("senses").querySelectorAll("span").forEach((b) => b.classList.remove("on"));
    simOp({ op: "reset", seed: 1 }).then(() => simOp({ op: "temperament", temperament: temper() }));
  });
  $("timeline-run").addEventListener("click", () => {
    let tl;
    try { tl = JSON.parse($("timeline").value); } catch (e) { $("sim-err").textContent = "timeline: " + e.message; return; }
    simOp({ op: "timeline", timeline: tl }).then((ok) => ok && simRun(true));
  });

  // ---------- tabs ----------
  for (const t of document.querySelectorAll("[data-tab]")) {
    t.addEventListener("click", () => {
      s.tab = t.dataset.tab;
      for (const u of document.querySelectorAll("[data-tab]")) u.classList.toggle("on", u === t);
      for (const name of ["comments", "sim", "lint", "audit"]) $("tab-" + name).hidden = name !== s.tab;
      if (s.tab === "comments") loadComments();
      if (s.tab !== "sim") simRun(false);
    });
  }

  // ---------- the headless equivalents ----------
  const q = (v) => (/^[\w./@:+-]+$/.test(v) ? v : "'" + String(v).replace(/'/g, "'\\''") + "'");
  function cli() {
    if (!s.st || !s.st.pack || !s.anim) return;
    const pk = q(s.st.pack.dir);
    const face = (s.eyes !== "open" ? " --eyes " + s.eyes : "") + (s.mouth !== "closed" ? " --mouth " + s.mouth : "");
    const lines = [["d", "$ buddy studio render --pack " + pk + " --clip " + q(s.clip) + " --dir " + s.dir + " --frame " + s.frame + " --zoom " + s.zoom + face + " --out frame.png"]];
    lines.push(["ok", "ok frame.png " + s.anim.w * s.zoom + "×" + s.anim.h * s.zoom]);
    lines.push(["d", "$ buddy studio lint --pack " + pk]);
    if (s.lint) lines.push(s.lint.errors ? ["e", s.lint.errors + " errors, " + s.lint.warnings + " warnings  (exit 1)"] : ["ok", "ok " + s.lint.files + " files, " + s.lint.warnings + " warnings"]);
    lines.push(["d", "$ buddy studio audit --pack " + pk + " --minutes " + ($("audit-min").value || 30) + " --seed " + ($("audit-seed").value || 3) + " --out audit/"]);
    lines.push(["d", "$ buddy studio comments --pack " + pk + " --open"]);
    $("studio-cli").replaceChildren(...lines.flatMap(([c, t]) => [el("span", c, t), document.createTextNode("\n")]));
  }

  // ---------- pickers ----------
  $("clip").addEventListener("change", (e) => { s.clip = e.target.value; s.draft = null; renderDirs(); loadFrames(); renderComments(); });
  $("pack").addEventListener("change", (e) => openPack(e.target.value));

  // ---------- hot reload ----------
  let busy = false;
  async function poll() {
    if (busy || document.hidden || !s.st || !s.st.pack) return;
    busy = true;
    try {
      const r = await call("studio_poll");
      if (r.changed) {
        s.st.error = r.error;
        if (r.reloaded) {
          clearCache();
          s.st = await call("studio_state");
          render();
          const p = $("reloaded");
          p.textContent = "reloaded " + new Date().toTimeString().slice(0, 8);
          p.hidden = false;
          await Promise.all([loadFrames(), loadLint(), loadComments(), simOp({ op: "tick", dt_ms: 0 })]);
        } else {
          banner(r.error);
        }
      }
    } catch (e) {
      console.error("studio_poll", e);
    } finally {
      busy = false;
    }
  }

  function start() {
    loadState().catch((e) => { s.st = { pack: null, packs: [], error: String(e) }; render(); });
    setInterval(poll, POLL_MS);
  }
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", start);
  else start();
})();
