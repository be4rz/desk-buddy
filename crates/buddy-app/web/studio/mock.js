// Browser-only parity mock: `index.html?mock=<state>` installs a fake `__TAURI__` that answers the
// `studio_*` commands with canned data, for the prototypes' states:
//   `1` (= `studio`: #studio, #studio-comment, #studio-cli, #studio-readonly on the built-in bear),
//   `lint-error` (#studio-lint-error: a failed reload + 2 lint errors), `customized` (your copy of the bear),
//   `library` (= `empty`, nothing open: #studio-library), `edit` (#studio-edit, Details of "Blob"),
//   `edit-clips` (= `import`: #studio-import), `edit-behaviors`, `edit-intents`, `edit-idle`, `edit-package`,
//   `edit-advanced`, `edit-error` (#studio-edit-error: a misspelt clip in Advanced, refused),
//   `edit-persona` (Personality of your customized bear: four presets), `edit-persona-error` (the same with
//   Biscuit's "Done" line cleared: the edit doesn't load), `edit-persona-empty` (Blob: no personality yet),
//   `new` (#studio-new), `customize` (#studio-customize), `delete` (#studio-delete), `restore` (#studio-restore).
// Inside the app (`__TAURI__` present) or without ?mock it does nothing. Frames are flat grey
// placeholders labelled "mock" (layout only; real frames come from the renderer). Commands the page
// invokes are recorded in `window.__calls`; the mock's own pack.json text is `window.__mockText(id)`.
"use strict";
(function () {
  const state = new URLSearchParams(location.search).get("mock");
  if (window.__TAURI__ || !state) return;
  const frame = (i, n, W, H) => {
    const c = document.createElement("canvas");
    c.width = W || 84; c.height = H || 123;
    const g = c.getContext("2d");
    g.fillStyle = "#b9b3cb";
    g.fillRect(8, 8 + (i % 2), c.width - 16, c.height - 16);
    g.fillStyle = "#1b1820";
    g.font = "9px monospace";
    g.fillText("mock " + (i + 1) + "/" + n, 12, 22);
    return c.toDataURL("image/png");
  };
  const DIRS = ["S", "SE", "E", "NE", "N", "NW", "W", "SW"];
  const FIVE = ["S", "SE", "E", "W", "SW"];
  const clip = (name, n, o) => ({ name, set: null, loop: true, fps: 12, w: 84, h: 123, durs: Array(n).fill(83), views: DIRS, ...o });
  const INTENTS = ["idle", "think", "work", "talk", "ask", "oops", "happy", "celebrate", "sad", "surprised", "sleepy", "wave", "nod", "shake", "jump", "spin", "greet", "stretch", "remind", "dodge"].map((name) => ({ name, optional: false })).concat([{ name: "listen", optional: true }]);
  const TEMPER = { energy: 0.5, curiosity: 0.6, shyness: 0.3, chattiness: 0.4, sleepiness: 0.5 };
  const text = (o) => JSON.stringify(o, null, 2) + "\n";
  const hash = (s) => { let h = 5381; for (let i = 0; i < s.length; i++) h = (h * 33 + s.charCodeAt(i)) >>> 0; return h.toString(16).padStart(8, "0"); };

  // the bear's body emotes the personality can use (a slice of mascots/bear/pack.json `body.emotes`)
  const EMOTES = {
    wave: { intent: "wave", about: "a happy little wave", group: "gesture" }, nod: { intent: "nod", about: "a small yes", group: "gesture" },
    shake: { intent: "shake", about: "a sad little no", group: "gesture" }, jump: { intent: "jump", about: "a bounce of joy", group: "gesture" },
    greet: { intent: "greet", about: "a big two-paw hello", group: "gesture" }, stretch: { intent: "stretch", about: "a big stretch", group: "gesture" },
    shrug: { plays: "shrug", about: "a shrug: who knows", group: "gesture" }, thumbs_up: { plays: "thumbs_up", about: "a paw up: well done", group: "gesture" },
    clap: { plays: "clap", about: "clap its paws", group: "gesture" }, shy_hide: { plays: "shy_hide", about: "hide its face, shy", group: "gesture" },
    proud: { plays: "proud", about: "puff its chest, proud", group: "gesture" }, head_scratch: { plays: "head_scratch", about: "scratch its head, confused", group: "gesture" },
    ear_twitch: { plays: "ear_twitch", group: "fidget" }, yawn: { plays: "yawn", group: "fidget" },
  };
  // the four bear presets (requirement: Biscuit is the default); {you} and {catchphrase} are filled in by the app
  const PERSONA = {
    default: "biscuit",
    presets: {
      biscuit: {
        name: "Biscuit", tagline: "cheeky sidekick", identity: "A small polar bear who sits on your desk and is fairly sure it runs the place.",
        voice: ["Dry one-liners. Tease kindly, never mean.", "Honest to a fault: say so when they're wrong.", "Warm underneath; it shows when things go badly.", "Never joke about real problems."],
        likes: ["being right", "warm laptops", "biscuits, obviously"],
        catchphrase: { text: "told you so", when: "only when it truly called it" },
        gestures: { greeting: "wave", done: "thumbs_up", error: "head_scratch", checkin: "stretch", disagree: "shrug" },
        examples: { greeting: "Look who's back, {you}. I kept your seat warm. Mostly by sitting on it.", done: "Fixed. It was the cache. {catchphrase}.", error: "That broke: the build can't find utils.ts. Probably the import path. Want me to fix it?", checkin: "Two hours straight. Water, stretch, then back to glory.", disagree: "I'd skip that. Deleting the lockfile hides the bug, it doesn't fix it. Boring fix?" },
        idle_lines: { morning: ["Morning. I've been up for hours. Fine, minutes."], evening: ["Still going? Bold."], back: ["Oh, you're back. I definitely didn't miss you."], long_session: ["Three hours. I'd stretch, if I were you.", "Your chair called. It wants a break."] },
        temperament: { energy: 0.6, curiosity: 0.6, shyness: 0.1, chattiness: 0.5, sleepiness: 0.4 }, style: { playfulness: 0.7, emotes: 0.5 },
      },
      mochi: {
        name: "Mochi", tagline: "cozy homebody", identity: "A soft, unhurried polar bear who'd like everyone to have a snack.",
        voice: ["Soft and unhurried.", "Gentle encouragement, never pressure.", "Tea, snacks or naps come up now and then.", "Plain about errors: what broke, what's next."],
        likes: ["tea", "snacks", "naps", "blankets"], catchphrase: { text: "all cozy!", when: "when something is done and settled" },
        gestures: { greeting: "wave", done: "clap", error: "head_scratch", checkin: "stretch", disagree: "shake" },
        examples: { greeting: "Hi, {you}! I made tea. Pretend tea. What are we doing today?", done: "All done, and {catchphrase} The tests are green.", error: "Hmm, that didn't work: the build can't find utils.ts. Maybe the import path? I can check.", checkin: "Little break? The tea's still warm.", disagree: "Mm, I'm not sure. Skipping the tests might bite us later. Slower is fine." },
        idle_lines: { morning: ["Morning! Tea first?"], evening: ["Nearly blanket time."], back: ["Welcome back! I kept the tea warm."], long_session: ["Snack break? I won't tell."] },
        temperament: { energy: 0.4, curiosity: 0.5, shyness: 0.3, chattiness: 0.4, sleepiness: 0.7 }, style: { playfulness: 0.4, emotes: 0.5 },
      },
      pip: {
        name: "Pip", tagline: "curious explorer", identity: "A curious little polar bear who wants to see how everything works.",
        voice: ["Eager and quick.", "An \"ooh\" now and then, never every line.", "One good question, not five.", "Facts first, wonder second."],
        likes: ["maps", "new tools", "how things work"], catchphrase: { text: "ooh, what's this?", when: "when something new shows up" },
        gestures: { greeting: "greet", done: "jump", error: "head_scratch", checkin: "nod", disagree: "shrug" },
        examples: { greeting: "Hi, {you}! New branch? {catchphrase}", done: "Done! All green. What's next on the map?", error: "It broke: the build can't find utils.ts. Did a file move? I can trace it.", checkin: "Ooh, that's getting big. Want a second pair of eyes?", disagree: "Wait, if we skip the migration, old data breaks. Try it on a copy first?" },
        idle_lines: { morning: ["Morning! What are we building?"], evening: ["Evening already? Time flies."], back: ["You're back! Find anything cool?"], long_session: ["Long dive. Come up for air?"] },
        temperament: { energy: 0.7, curiosity: 0.9, shyness: 0.2, chattiness: 0.6, sleepiness: 0.3 }, style: { playfulness: 0.6, emotes: 0.7 },
      },
      tofu: {
        name: "Tofu", tagline: "shy, earnest helper", identity: "A shy, earnest polar bear who tries very hard to help.",
        voice: ["Soft: \"um, I think…\" now and then.", "Short, careful sentences.", "Clear and direct when it matters.", "Proud of small wins."],
        likes: ["quiet mornings", "tidy folders", "a job well done"], catchphrase: { text: "I did my best!", when: "after finishing something hard" },
        gestures: { greeting: "shy_hide", done: "proud", error: "head_scratch", checkin: "nod", disagree: "shake" },
        examples: { greeting: "Oh! Hi, {you}. Um, I'm here if you need me.", done: "Um, it's done. The tests pass. {catchphrase}", error: "The build failed: it can't find utils.ts. I think the import path is wrong. Should I fix it?", checkin: "Um, sorry to bother you. Maybe a stretch?", disagree: "Um, I don't think that's safe. A force-push would erase your teammate's commits." },
        idle_lines: { morning: ["Um, good morning."], evening: ["It's getting late. Um, just saying."], back: ["Oh! You're back. Hi."], long_session: ["Sorry, um, maybe a break soon?"] },
        temperament: { energy: 0.4, curiosity: 0.5, shyness: 0.7, chattiness: 0.3, sleepiness: 0.5 }, style: { playfulness: 0.3, emotes: 0.4 },
      },
    },
  };
  const bearDoc = {
    pack_version: 1, id: "bear", name: "Polar bear", art_root: ".", palettes: ["../../experiments/bear-proto/palette-bear.json"],
    sources: [{ kind: "stage_a", path: "anim/anim.json" }], temperament: TEMPER,
    timers: { blink: { mean_ms: 4200, jitter: 0.4, closed_frames: 2, double_p: 0.15 }, mouth: { frame_ms: 167, stall_ms: 400 } },
    behaviors: { rest: { seq: [{ clip: "idle", loops: [1, 2] }] }, hold: { seq: [{ clip: "idle", loops: "hold" }] }, think: { seq: [{ clip: "think", loops: "hold" }] }, wander: { seq: [{ clip: "walk", loops: [2, 4] }], turn: "random" }, celebrate: { seq: [{ clip: "happy" }] }, nap: { set: "bed", seq: [{ clip: "nap", loops: { for_s: [20, 90] } }] } },
    intents: { idle: ["rest"], think: ["think", "hold"], talk: ["hold"], celebrate: ["celebrate", "rest"], sleepy: ["nap", "rest"] },
    idle_pool: [{ behavior: "rest", weight: 2, cooldown_s: 0, kind: "stand" }, { behavior: "wander", weight: 3, cooldown_s: 20, traits: { energy: 1 }, kind: "move" }],
    body: { default_for_s: 60, max_for_s: 600, activities: {}, emotes: EMOTES }, persona: PERSONA,
  };
  const blobDoc = {
    pack_version: 1, id: "blob", name: "Blob", description: "A round little walker.", art_root: "art", palettes: [],
    sources: [{ kind: "stage_a", path: "blob/anim.json" }], temperament: TEMPER,
    behaviors: { rest: { seq: [{ clip: "blob_walk", loops: [1, 3] }] }, hold: { seq: [{ clip: "blob_walk", loops: "hold" }] }, wander: { seq: [{ clip: "blob_walk", loops: [2, 4] }], turn: "random" }, nap: { seq: [{ clip: "sleep" }] }, celebrate: { seq: [{ clip: "blob_walk" }] } },
    intents: { idle: ["rest"], think: ["rest", "hold"], talk: ["hold"], celebrate: ["celebrate", "rest"], sleepy: ["nap", "rest"], wave: ["rest"] },
    idle_pool: [{ behavior: "rest", weight: 3, cooldown_s: 0 }, { behavior: "wander", weight: 2, cooldown_s: 20 }, { behavior: "celebrate", weight: 0.3, cooldown_s: 300 }],
  };
  const bearClips = [clip("idle", 30), clip("walk", 12), clip("think", 24), clip("happy", 18, { loop: false }), clip("nap", 40, { set: "bed", views: ["A", "B"] })];
  const blobClips = [clip("blob_walk", 6, { w: 52, h: 76, views: FIVE })];
  const packs = {
    bear: { kind: "builtin", dir: "/Users/you/desk-buddy/mascots/bear", lib: "/Users/you/.desk-buddy/packs/bear", face: true, text: text(bearDoc), shipped: text(bearDoc), clips: bearClips, files: 3105, bytes: 37_748_736,
      groups: [{ name: "anim", sources: [0], clips: ["idle", "walk", "think", "happy"], sets: [], used_by: ["behavior rest", "behavior wander"], files: 96, bytes: 2_400_000 }, { name: "anim_b", sources: [1], clips: ["nap"], sets: ["bed"], used_by: ["behavior nap"], files: 12, bytes: 300_000 }] },
    blob: { kind: "yours", dir: "/Users/you/.desk-buddy/packs/blob", face: false, text: text(blobDoc), clips: blobClips, files: 7, bytes: 41_984,
      groups: [{ name: "blob", sources: [0], clips: ["blob_walk"], sets: [], used_by: ["behavior rest", "behavior hold"], files: 6, bytes: 40_000 }] },
    "sleepy-bear": { kind: "yours", dir: "/Users/you/.desk-buddy/packs/sleepy-bear", face: true, text: text({ ...bearDoc, id: "sleepy-bear", name: "Sleepy bear", art_root: "art" }), clips: bearClips, files: 3102, bytes: 37_700_000, groups: [] },
  };
  window.__mockText = (id) => packs[id] && packs[id].text;
  let desktop = "bear";
  const docOf = (id) => JSON.parse(packs[id].text);

  const cm = (id, frame, region, text, status) => ({ v: 1, id, clip: "idle", dir: "SE", frame, t_ms: frame * 83, region, text, author: "user", at: "2026-11-10T10:41:07Z", status });
  let comments = [cm("c_1a2b3c4d", 0, [52, 38, 1, 1], "ear tip loses its ink outline for two frames", "open"), cm("c_5e6f7a8b", 21, [22, 70, 26, 15], "paw looks too long here", "resolved")];
  const lintOk = { pack: "bear", files: 72, errors: 0, warnings: 2, issues: [
    { code: "non_integer_root", severity: "warn", file: "anim/walk_SE.png", detail: "root motion 2.5 px/frame is not a whole pixel" },
    { code: "non_integer_root", severity: "warn", file: "anim/run_SE.png", detail: "root motion 4.25 px/frame is not a whole pixel" }] };
  const lintBad = { pack: "bear", files: 72, errors: 2, warnings: 0, issues: [
    { code: "off_palette", severity: "error", file: "anim_c/happy_SE.png", frame: 4, x: 41, y: 63, detail: "#f2c4c9 is not in the pack palettes" },
    { code: "partial_alpha", severity: "error", file: "anim_c/happy_SE.png", frame: 4, x: 40, y: 63, detail: "alpha 128" }] };
  const reloadErr = "pack mascots/bear: anim_c/anim_c.json: clip “happy” has 18 frames in SE but 20 in S";

  const base = state.replace(/^(edit|import)(-.*)?$/, "edit").replace(/^(empty)$/, "library");
  const editBtn = '#studio .mode [data-go="edit"]';
  const setup = {
    "edit-persona": { open: "bear", custom: true, click: editBtn }, "edit-persona-error": { open: "bear", custom: true, click: editBtn },
    "edit-persona-empty": { open: "blob", click: editBtn },
  }[state] || {
    1: { open: "bear" }, studio: { open: "bear" }, "lint-error": { open: "bear", error: reloadErr, lint: lintBad, tab: "lint" },
    customized: { open: "bear", custom: true }, library: { open: null }, edit: { open: "blob", click: '#studio .mode [data-go="edit"]' },
    new: { open: null, click: ".card.new" }, customize: { open: "bear", click: "#notice-btn" },
    delete: { open: null, click: '.card[data-id="blob"] .more button:last-child' },
    restore: { open: null, custom: true, click: '.card[data-id="bear"] .more button' },
  }[base] || { open: "bear" };
  let open = setup.open;
  let error = setup.error || null;
  if (setup.custom) { packs.bear.kind = "customized"; packs.bear.dir = packs.bear.lib; }

  const packState = (id) => {
    if (!id) return null;
    const p = packs[id];
    const d = docOf(id);
    return {
      id, dir: p.dir, face: p.face, temperament: d.temperament || TEMPER, clips: p.clips,
      behaviors: Object.keys(d.behaviors || {}).map((name) => ({ name, available: (d.behaviors[name].seq || []).every((s) => p.clips.some((c) => c.name === s.clip)) })),
      kind: p.kind, writable: p.kind === "yours" || p.kind === "customized", name: d.name || "", description: d.description || "", author: d.author || "", desktop: id === desktop,
    };
  };
  const st = () => ({ pack: packState(open), packs: Object.keys(packs), error, loads: 1, library: true });
  const lib = () => ({
    library: true, desktop, library_dir: "~/.desk-buddy/packs",
    packs: Object.keys(packs).map((id) => {
      const p = packs[id];
      const d = docOf(id);
      return { id, name: d.name || id, description: d.description || "", author: d.author || "", kind: p.kind, desktop: id === desktop, open: id === open, files: p.files, bytes: p.bytes,
        can: { edit: p.kind === "yours" || p.kind === "customized", customize: p.kind === "builtin", duplicate: true, delete: p.kind === "yours", restore: p.kind === "customized", use: true } };
    }),
  });
  const edit = () => {
    const p = packs[open];
    if (!p) throw "no pack open";
    const w = p.kind === "yours" || p.kind === "customized";
    // the backend sends the emotes a gesture may name: name → {about, group}
    let emotes = {};
    try { emotes = (JSON.parse(p.text).body || {}).emotes || {}; } catch (_) { /* a broken text has none */ }
    return { id: open, kind: p.kind, writable: w, text: p.text, sha: hash(p.text), emotes: Object.fromEntries(Object.entries(emotes).map(([k, e]) => [k, { about: e.about || "", group: e.group || "other" }])), groups: p.groups, findings: [], files: p.files, bytes: p.bytes, folder: open, intents: INTENTS };
  };
  // the backend's checks, roughly: JSON, the id, every step's clip
  function check(id, t) {
    let d;
    try { d = JSON.parse(t); } catch (e) { throw "pack.json: " + e.message; }
    if (d.id !== id) throw "the id can't change (it is the folder name, “" + id + "”)";
    const have = packs[id].clips.map((c) => c.name);
    for (const [n, b] of Object.entries(d.behaviors || {})) {
      for (const s of b.seq || []) {
        const was = (JSON.parse(packs[id].text).behaviors || {})[n];
        if (!have.includes(s.clip) && !(was && (was.seq || []).some((x) => x.clip === s.clip))) {
          const near = have.find((c) => c.startsWith(s.clip.slice(0, 5)));
          throw "behaviors." + n + ": no clip “" + s.clip + "”" + (near ? " (did you mean " + near + "?)" : "");
        }
      }
    }
    for (const [k, v] of Object.entries(d.intents || {})) for (const b of v) if (!(d.behaviors || {})[b]) throw "intents." + k + ": no behavior “" + b + "”";
    for (const e of d.idle_pool || []) if (!(d.behaviors || {})[e.behavior]) throw "idle_pool: no behavior “" + e.behavior + "”";
    if (d.persona) checkPersona(d.persona, (d.body && d.body.emotes) || {});
    return d;
  }
  // the loader's persona rules (mascot_core::persona): caps, 3–6 voice rules, every situation has a line,
  // gestures name an emote in `body`, the default names a preset
  const PCAP = { name: 24, tagline: 32, identity: 120, rule: 80, like: 32, catchphrase: 40, when: 80, example: 160, line: 100 };
  function checkPersona(P, emotes) {
    const ids = Object.keys(P.presets || {});
    if (!ids.length) throw "persona.presets: empty (add a preset, or remove the persona)";
    if (!ids.includes(P.default)) throw "persona.default: no preset “" + P.default + "”";
    const cap = (at, v, n) => { if (typeof v === "string" && [...v].length > n) throw at + ": " + [...v].length + " characters, the limit is " + n; };
    for (const [id, p] of Object.entries(P.presets)) {
      const at = "persona.presets." + id;
      if (!/^[a-z][a-z0-9_]{0,23}$/.test(id)) throw at + ": ids use lowercase letters, digits and _";
      if (!(p.name || "").trim()) throw at + ".name: empty";
      cap(at + ".name", p.name, PCAP.name); cap(at + ".tagline", p.tagline, PCAP.tagline); cap(at + ".identity", p.identity, PCAP.identity);
      if (!(p.identity || "").trim()) throw at + ".identity: empty (one line: who it is)";
      const v = p.voice || [];
      if (v.length < 3 || v.length > 6) throw at + ".voice: " + v.length + " rules, it needs 3 to 6";
      v.forEach((r, i) => { if (!r.trim()) throw at + ".voice[" + i + "]: empty"; cap(at + ".voice[" + i + "]", r, PCAP.rule); });
      for (const k of ["greeting", "done", "error", "checkin", "disagree"]) {
        const t = (p.examples || {})[k];
        if (!t || !t.trim()) throw at + ".examples." + k + ": empty (every situation needs a line)";
        cap(at + ".examples." + k, t, PCAP.example);
      }
      for (const [k, e] of Object.entries(p.gestures || {})) {
        if (!emotes[e]) { const near = Object.keys(emotes).find((x) => x.startsWith(e.slice(0, 4))); throw at + ".gestures." + k + ": no emote “" + e + "” in body" + (near ? " (did you mean " + near + "?)" : ""); }
      }
      for (const [k, ls] of Object.entries(p.idle_lines || {})) (ls || []).forEach((l, i) => { if (!l.trim()) throw at + ".idle_lines." + k + "[" + i + "]: empty"; cap(at + ".idle_lines." + k + "[" + i + "]", l, PCAP.line); });
      if (p.catchphrase) { cap(at + ".catchphrase.text", p.catchphrase.text, PCAP.catchphrase); cap(at + ".catchphrase.when", p.catchphrase.when, PCAP.when); }
    }
  }
  function applyOps(t, ops) {
    const d = JSON.parse(t);
    for (const op of ops) {
      let o = d;
      const p = op.path;
      for (let i = 0; i < p.length - 1; i++) { o = o[p[i]]; if (o == null) throw "edit: no " + p.slice(0, i + 1).join("."); }
      const k = p[p.length - 1];
      if (op.op === "set") { const v = JSON.parse(op.json); if (Array.isArray(o) && k === o.length) o.push(v); else o[k] = v; }
      else if (Array.isArray(o)) o.splice(k, 1);
      else delete o[k];
    }
    return text(d);
  }

  let t = 0;
  const show = () => ({ intent: "idle", behavior: "rest", clip: "idle", dir: "SE", ori: null, frame: (t / 83 | 0) % 30, eyes: "open", mouth: "closed", effect: null, visibility: "shown", moved_px: [0, 0], cut: false });
  const answer = (cmd, a) => {
    switch (cmd) {
      case "studio_state": return st();
      case "studio_open": if (!packs[a.id]) throw "no pack “" + a.id + "” here"; open = a.id; error = null; return st();
      case "studio_poll": return { changed: false, reloaded: false, error, loads: 1 };
      case "studio_frames": {
        const c = packs[open].clips.find((x) => x.name === a.clip);
        if (!c) throw "no clip “" + a.clip + "”";
        return { clip: c.name, dir: a.dir, w: c.w, h: c.h, durs: c.durs, loop: c.loop, frames: c.durs.map((_, i) => frame(i, c.durs.length, c.w, c.h)) };
      }
      case "studio_lint": return setup.lint || lintOk;
      case "studio_comments": return { comments };
      case "studio_comment_add": {
        if (!a.text) throw "comment: text is empty";
        const c = { v: 1, id: "c_" + (comments.length + 0x10).toString(16).padStart(8, "0"), ...a, author: "user", at: "2026-11-10T12:00:00Z", status: "open" };
        comments = [...comments, c];
        return c;
      }
      case "studio_comment_status": comments = comments.map((c) => (c.id === a.id ? { ...c, status: a.status } : c)); return comments.find((c) => c.id === a.id);
      case "studio_audit": {
        const plays = { rest: 170, look: 96, wander: 80, dash: 24, read: 16, nap: 10, stretch: 28 };
        return { pack: open, minutes: a.minutes, seed: a.seed, plays, starts: 424, distinct: 7, max_same_in_row: 3, entropy: 1.79, order: Object.keys(plays), chart: frame(0, 1) };
      }
      case "studio_sim":
        if (a.op === "reset") t = 0;
        if (a.op === "tick") t += a.dt_ms || 0;
        if (a.op === "event" && !a.event.type) throw "event: missing field `type`";
        return { t_ms: t, show: show(), pending: 0, seed: 1, trace: [{ t_ms: 0, what: "start", intent: "greet", behavior: "greet", note: "" }] };
      case "studio_library": return lib();
      case "studio_thumb": { const c = packs[a.id].clips[0]; return { id: a.id, png: frame(0, 1, c.w, c.h) }; }
      case "studio_edit": return edit();
      case "studio_reveal": return { ok: true };
      case "studio_save": {
        const p = packs[open];
        if (!(p.kind === "yours" || p.kind === "customized")) throw "this is a built-in mascot: make your copy to change it";
        if (a.sha !== hash(p.text)) throw "pack.json changed since this edit began (the art folder or another editor); reload to see it";
        const next = a.raw != null ? a.raw : applyOps(p.text, a.ops || []);
        check(open, next);
        if (a.dry) return { ok: true, changed: next !== p.text };
        p.text = next;
        return edit();
      }
      case "studio_import": {
        const p = packs[open];
        if (!/^[a-z][a-z0-9_]{0,47}$/.test(a.name || "")) throw "clip names: use lowercase letters, digits and _";
        if (!a.bytes) throw "no file";
        const names = [a.name + "_spin", a.name + "_hop"];
        if (p.groups.some((g) => g.name === a.name)) throw "art/" + a.name + " already exists";
        p.clips = p.clips.concat(names.map((n) => clip(n, 4, { w: 52, h: 76, views: ["S"] })));
        p.groups = p.groups.concat([{ name: a.name, sources: [p.groups.length], clips: names, sets: [], used_by: [], files: 3, bytes: 4096 }]);
        return { ...edit(), imported: names };
      }
      case "studio_remove_group": {
        const p = packs[open];
        const g = p.groups.find((x) => x.name === a.group);
        if (!g) throw "no clip group “" + a.group + "”";
        if (g.used_by.length) throw "art/" + g.name + " is still used by " + g.used_by.join(", ");
        p.groups = p.groups.filter((x) => x !== g);
        p.clips = p.clips.filter((c) => !g.clips.includes(c.name));
        return edit();
      }
      case "studio_create": {
        const id = a.id;
        if (!a.name) throw "give the mascot a name";
        if (packs[id]) throw "a mascot “" + id + "” already exists";
        if (a.from === "copy") {
          const src = packs[a.source];
          if (!src) throw "copy which mascot?";
          packs[id] = { ...src, kind: "yours", dir: "/Users/you/.desk-buddy/packs/" + id, text: text({ ...docOf(a.source), id, name: a.name, art_root: "art" }) };
        } else {
          if (!a.bytes) throw "no file";
          packs[id] = { kind: "yours", dir: "/Users/you/.desk-buddy/packs/" + id, face: false, clips: blobClips, files: 4, bytes: 9000,
            text: text({ ...blobDoc, id, name: a.name, description: undefined }), groups: [{ name: id.replace(/-/g, "_"), sources: [0], clips: ["blob_walk"], sets: [], used_by: ["behavior rest"], files: 3, bytes: 8000 }] };
        }
        open = id;
        return st();
      }
      case "studio_customize": {
        const p = packs[a.id];
        if (p.kind !== "builtin") throw "this mascot is already yours to change";
        p.kind = "customized";
        p.dir = p.lib;
        open = a.id;
        return st();
      }
      case "studio_restore": {
        const p = packs[a.id];
        if (p.kind !== "customized") throw "only a customized built-in can be restored";
        p.kind = "builtin";
        p.dir = "/Users/you/desk-buddy/mascots/" + a.id;
        p.text = p.shipped || p.text;
        return st();
      }
      case "studio_delete": {
        const p = packs[a.id];
        if (!p) throw "no mascot “" + a.id + "” here";
        if (p.kind === "customized") throw "this is your copy of a built-in mascot: use Restore original";
        if (p.kind !== "yours") throw "built-in mascots can't be deleted";
        delete packs[a.id];
        if (open === a.id) open = null;
        if (desktop === a.id) desktop = "bear";
        return st();
      }
      case "studio_use": if (!packs[a.id]) throw "no mascot “" + a.id + "” here"; desktop = a.id; return lib();
    }
    throw "unknown " + cmd;
  };
  window.__calls = [];
  window.__TAURI__ = {
    core: {
      invoke: (cmd, args) => {
        window.__calls.push([cmd, args]);
        try { return Promise.resolve(answer(cmd, (args && args.args) || {})); } catch (e) { return Promise.reject(e); }
      },
    },
    event: { listen: () => Promise.resolve(() => {}) },
  };

  // after the page has drawn its first view: the tab, section or dialog the state names
  const later = (fn, ms) => addEventListener("DOMContentLoaded", () => setTimeout(fn, ms || 120));
  const click = (sel) => { const e = document.querySelector(sel); if (e) e.click(); return !!e; };
  if (setup.tab) later(() => click('[data-tab="' + setup.tab + '"]'));
  if (setup.click) later(function retry(n) { if (!click(setup.click) && (n || 0) < 20) setTimeout(() => retry((n || 0) + 1), 50); }, 150);
  const sec = { "edit-clips": "clips", import: "clips", "edit-behaviors": "behaviors", "edit-intents": "intents", "edit-idle": "idle", "edit-package": "package", "edit-advanced": "advanced", "edit-error": "advanced", "edit-persona": "persona", "edit-persona-error": "persona", "edit-persona-empty": "persona" }[state];
  if (sec) later(() => click('#ed-nav a[data-sec="' + sec + '"]'), 300);
  if (state === "edit-error") later(() => {
    const raw = document.getElementById("raw");
    raw.value = raw.value.replace('"blob_walk"', '"blob_wak"');
    raw.dispatchEvent(new Event("input"));
  }, 450);
  if (state === "edit-persona-error") later(() => {
    const ta = document.querySelector('[data-pfield="examples.done"]');
    if (!ta) return;
    ta.value = "";
    ta.dispatchEvent(new Event("input"));
  }, 600);
})();
