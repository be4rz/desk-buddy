// Browser-only parity mock: `index.html?mock=<state>` installs a fake `__TAURI__` that answers the
// `studio_*` commands with canned data, for the prototype's states: `1` (= `studio`: #studio,
// #studio-comment, #studio-cli), `lint-error` (#studio-lint-error: a failed reload + 2 lint errors),
// `empty` (#studio-empty). Inside the app (`__TAURI__` present) or without ?mock it does nothing.
// Frames are flat grey placeholders labelled "mock" (layout only; real frames come from the renderer).
// Commands the page invokes are recorded in `window.__calls`.
"use strict";
(function () {
  const state = new URLSearchParams(location.search).get("mock");
  if (window.__TAURI__ || !state) return;
  const W = 84, H = 123;
  const frame = (i, n) => {
    const c = document.createElement("canvas");
    c.width = W; c.height = H;
    const g = c.getContext("2d");
    g.fillStyle = "#b9b3cb";
    g.fillRect(8, 8 + (i % 2), W - 16, H - 16);
    g.fillStyle = "#1b1820";
    g.font = "9px monospace";
    g.fillText("mock " + (i + 1) + "/" + n, 14, 22);
    return c.toDataURL("image/png");
  };
  const DIRS = ["S", "SE", "E", "NE", "N", "NW", "W", "SW"];
  const clip = (name, n, o) => ({ name, set: null, loop: true, fps: 12, w: W, h: H, durs: Array(n).fill(83), views: DIRS, ...o });
  const pack = {
    id: "bear", dir: "/Users/you/desk-buddy/mascots/bear", face: true,
    temperament: { energy: 0.5, curiosity: 0.6, shyness: 0.3, chattiness: 0.4, sleepiness: 0.5 },
    clips: [clip("idle", 30), clip("walk", 12), clip("think", 24), clip("happy", 18, { loop: false }), clip("nap", 40, { set: "bed", views: ["A", "B"] })],
    behaviors: [{ name: "rest", available: true }, { name: "nap", available: false }],
  };
  const cm = (id, frame, region, text, status) => ({ v: 1, id, clip: "idle", dir: "SE", frame, t_ms: frame * 83, region, text, author: "user", at: "2026-11-10T10:41:07Z", status });
  let comments = [cm("c_1a2b3c4d", 0, [52, 38, 1, 1], "ear tip loses its ink outline for two frames", "open"), cm("c_5e6f7a8b", 21, [22, 70, 26, 15], "paw looks too long here", "resolved")];
  const lintOk = { pack: "bear", files: 72, errors: 0, warnings: 2, issues: [
    { code: "non_integer_root", severity: "warn", file: "anim/walk_SE.png", detail: "root motion 2.5 px/frame is not a whole pixel" },
    { code: "non_integer_root", severity: "warn", file: "anim/run_SE.png", detail: "root motion 4.25 px/frame is not a whole pixel" }] };
  const lintBad = { pack: "bear", files: 72, errors: 2, warnings: 0, issues: [
    { code: "off_palette", severity: "error", file: "anim_c/happy_SE.png", frame: 4, x: 41, y: 63, detail: "#f2c4c9 is not in the pack palettes" },
    { code: "partial_alpha", severity: "error", file: "anim_c/happy_SE.png", frame: 4, x: 40, y: 63, detail: "alpha 128" }] };
  const reloadErr = "pack mascots/bear: anim_c/anim_c.json: clip “happy” has 18 frames in SE but 20 in S";
  const states = {
    1: { pack, error: null, lint: lintOk },
    "lint-error": { pack, error: reloadErr, lint: lintBad, tab: "lint" },
    empty: { pack: null, error: null, packs: ["bear", "blob"] },
  };
  states.studio = states[1];
  const S = states[state] || states[1];
  let t = 0;
  const show = () => ({ intent: "idle", behavior: "rest", clip: "idle", dir: "SE", ori: null, frame: (t / 83 | 0) % 30, eyes: "open", mouth: "closed", effect: null, visibility: "shown", moved_px: [0, 0], cut: false });
  const answer = (cmd, a) => {
    switch (cmd) {
      case "studio_state": return { pack: S.pack, packs: S.packs || ["bear"], error: S.error, loads: 1 };
      case "studio_open": S.pack = pack; return { pack, packs: S.packs || ["bear"], error: null, loads: 1 };
      case "studio_poll": return { changed: false, reloaded: false, error: S.error, loads: 1 };
      case "studio_frames": {
        const c = pack.clips.find((x) => x.name === a.clip);
        if (!c) throw "no clip “" + a.clip + "”";
        return { clip: c.name, dir: a.dir, w: W, h: H, durs: c.durs, loop: c.loop, frames: c.durs.map((_, i) => frame(i, c.durs.length)) };
      }
      case "studio_lint": return S.lint;
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
        return { pack: "bear", minutes: a.minutes, seed: a.seed, plays, starts: 424, distinct: 7, max_same_in_row: 3, entropy: 1.79, order: Object.keys(plays), chart: frame(0, 1) };
      }
      case "studio_sim":
        if (a.op === "reset") t = 0;
        if (a.op === "tick") t += a.dt_ms || 0;
        if (a.op === "event" && !a.event.type) throw "event: missing field `type`";
        return { t_ms: t, show: show(), pending: 0, seed: 1, trace: [{ t_ms: 0, what: "start", intent: "greet", behavior: "greet", note: "" }] };
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
  if (S.tab) addEventListener("DOMContentLoaded", () => setTimeout(() => { const t = document.querySelector('[data-tab="' + S.tab + '"]'); if (t) t.click(); }, 50));
})();
