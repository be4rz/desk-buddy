// Browser-only parity mock: `index.html?mock=<state>` installs a fake `__TAURI__` that answers the
// persona_* commands for one of the Personality window's states:
//   `default` (Biscuit, nothing changed), `customized` (a renamed Biscuit with its own words and sliders),
//   `mochi`, `pip`, `tofu` (another preset picked), `notes-too-long` (540 characters pasted into the notes),
//   `saving` (a change on its way to settings.json), `save-error` (settings.json refused the write),
//   `preset-missing` (settings name a preset the mascot no longer has), `no-persona` (a mascot pack
//   without a persona block), `loading` (before the first state arrives).
// Inside the app (`__TAURI__` present) or without ?mock it does nothing. Commands the page invokes
// are recorded in `window.__calls`; `window.__mockState(s)` pushes a `persona://state` like the app.
"use strict";
(function () {
  const state = new URLSearchParams(location.search).get("mock");
  if (window.__TAURI__ || !state) return;

  // the bear pack's emotes the presets use (`body.emotes[<id>].about`)
  const ABOUT = {
    wave: "a happy little wave", greet: "a big two-paw hello", thumbs_up: "a paw up: well done", clap: "clap its paws",
    jump: "a bounce of joy", proud: "puff its chest, proud", head_scratch: "scratch its head, confused", shrug: "a shrug: who knows",
    shake: "a sad little no", nod: "a small yes", stretch: "a big stretch", shy_hide: "hide its face, shy",
  };
  const G = (o) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k, { emote: v, about: ABOUT[v] }]));
  // {me} = its name, {you} = what it calls the person (dropped with its comma when unset), {catchphrase}
  const PRESETS = [
    {
      id: "biscuit", name: "Biscuit", tagline: "cheeky sidekick", catchphrase: "told you so",
      chattiness: 0.5, playfulness: 0.7, emotes: 0.5,
      examples: {
        greeting: "Look who's back, {you}. I kept your seat warm. Mostly by sitting on it.",
        done: "Fixed. It was the cache. {catchphrase}.",
        error: "That broke: the build can't find utils.ts. Probably the import path. Want me to fix it?",
        checkin: "Two hours straight. Water, stretch, then back to glory.",
        disagree: "I'd skip that. Deleting the lockfile hides the bug, it doesn't fix it. Boring fix?",
      },
      gestures: G({ greeting: "wave", done: "thumbs_up", error: "head_scratch", checkin: "stretch", disagree: "shrug" }),
    },
    {
      id: "mochi", name: "Mochi", tagline: "cozy homebody", catchphrase: "all cozy!",
      chattiness: 0.4, playfulness: 0.4, emotes: 0.5,
      examples: {
        greeting: "Hi, {you}! I made tea. Pretend tea. What are we doing today?",
        done: "All done, and {catchphrase} The tests are green.",
        error: "Hmm, that didn't work: the build can't find utils.ts. Maybe the import path? I can check.",
        checkin: "Little break? The tea's still warm.",
        disagree: "Mm, I'm not sure. Skipping the tests might bite us later. Slower is fine.",
      },
      gestures: G({ greeting: "wave", done: "clap", error: "head_scratch", checkin: "stretch", disagree: "shake" }),
    },
    {
      id: "pip", name: "Pip", tagline: "curious explorer", catchphrase: "ooh, what's this?",
      chattiness: 0.6, playfulness: 0.6, emotes: 0.7,
      examples: {
        greeting: "Hi, {you}! New branch? {catchphrase}",
        done: "Done! All green. What's next on the map?",
        error: "It broke: the build can't find utils.ts. Did a file move? I can trace it.",
        checkin: "Ooh, that's getting big. Want a second pair of eyes?",
        disagree: "Wait, if we skip the migration, old data breaks. Try it on a copy first?",
      },
      gestures: G({ greeting: "greet", done: "jump", error: "head_scratch", checkin: "nod", disagree: "shrug" }),
    },
    {
      id: "tofu", name: "Tofu", tagline: "shy, earnest helper", catchphrase: "I did my best!",
      chattiness: 0.3, playfulness: 0.3, emotes: 0.4,
      examples: {
        greeting: "Oh! Hi, {you}. Um, I'm here if you need me.",
        done: "Um, it's done. The tests pass. {catchphrase}",
        error: "The build failed: it can't find utils.ts. I think the import path is wrong. Should I fix it?",
        checkin: "Um, sorry to bother you. Maybe a stretch?",
        disagree: "Um, I don't think that's safe. A force-push would erase your teammate's commits.",
      },
      gestures: G({ greeting: "shy_hide", done: "proud", error: "head_scratch", checkin: "nod", disagree: "shake" }),
    },
  ];
  const blank = { preset: null, name: null, call_me: null, catchphrase: null, chattiness: null, playfulness: null, emotes: null, notes: "" };
  const caps = { name: 24, call_me: 24, catchphrase: 40, notes: 500 };
  const bear = { id: "bear", name: "Polar bear" };
  const persona = { default: "biscuit", presets: PRESETS };
  const long = "Keep it short before 10am. No jokes about my code reviews, they're a sore spot. Call bugs “critters”. " +
    "If I say I'm tired, suggest a break instead of more work. I like cats, synthwave and terrible puns, so those are fair game. " +
    "When I paste a stack trace, start with the one line that matters. Never tell me to “just” do something. " +
    "On Fridays you can be a bit sillier. If I ask the same question twice, gently remind me we covered it. ";
  const setups = {
    default: { user: { ...blank } },
    customized: { user: { ...blank, preset: "biscuit", name: "Sir Reginald Fluffington", call_me: "Captain", catchphrase: "called it", chattiness: 0.2, playfulness: 0.9, notes: "No jokes about my code reviews. Call bugs “critters”." } },
    mochi: { user: { ...blank, preset: "mochi", call_me: "Sam" } },
    pip: { user: { ...blank, preset: "pip" } },
    tofu: { user: { ...blank, preset: "tofu", call_me: "Sam" } },
    "notes-too-long": { user: { ...blank, call_me: "Sam" }, type: { field: "notes", value: (long + long).slice(0, 540) } },
    saving: { user: { ...blank, call_me: "Sa" }, type: { field: "call_me", value: "Sam" }, hang: true },
    "save-error": { user: { ...blank, call_me: "Sam" }, type: { field: "catchphrase", value: "nailed it" }, fail: "couldn't write ~/.desk-buddy/settings.json: Permission denied (os error 13)" },
    "preset-missing": { user: { ...blank, preset: "biscuit" }, missing: "sprout" },
    "no-persona": { pack: { id: "blob", name: "Blob" }, persona: null, user: { ...blank } },
    loading: { never: true },
  };
  const setup = setups[state] || setups.default;
  let user = { ...setup.user };
  const view = () => ({
    pack: setup.pack || bear,
    persona: setup.persona === null ? null : persona,
    user: { ...user },
    // settings.persona.preset named a preset the pack doesn't have: the app runs the default instead
    missing: setup.missing || null,
    caps,
  });
  const PRESET_OWNED = ["name", "catchphrase", "chattiness", "playfulness", "emotes"];
  function check(patch) {
    for (const [k, v] of Object.entries(patch)) {
      if (!(k in blank)) throw "unknown field “" + k + "”";
      if (typeof v === "string" && caps[k] && [...v].length > caps[k]) throw k + ": " + [...v].length + " characters, the limit is " + caps[k];
      if (typeof v === "number" && !(v >= 0 && v <= 1)) throw k + ": must be between 0 and 1";
    }
  }
  const answer = (cmd, a) => {
    switch (cmd) {
      case "persona_state": return setup.never ? new Promise(() => {}) : view();
      case "persona_set": {
        check(a.patch || {});
        if (setup.hang) return new Promise(() => {});
        if (setup.fail) throw setup.fail;
        const p = a.patch || {};
        if ("preset" in p && p.preset != null && !PRESETS.some((x) => x.id === p.preset)) {
          // only the page's Undo may bring back a preset the mascot lacks
          if (!a.undo) throw "no preset “" + p.preset + "” in this mascot";
          setup.missing = p.preset;
        }
        user = { ...user, ...p };
        return view();
      }
      case "persona_pick": {
        if (!PRESETS.some((p) => p.id === a.preset)) throw "no preset “" + a.preset + "” in this mascot";
        if (setup.fail) throw setup.fail;
        const prev = { ...user };
        user = { ...user, preset: a.preset };
        for (const k of PRESET_OWNED) user[k] = null;
        setup.missing = null;
        return { ...view(), undo: prev };
      }
      case "persona_reset": {
        if (setup.fail) throw setup.fail;
        for (const k of PRESET_OWNED) user[k] = null;
        return view();
      }
      case "persona_open_studio": case "persona_close": return null;
    }
    throw "unknown " + cmd;
  };
  const listeners = {};
  window.__calls = [];
  window.__TAURI__ = {
    core: {
      invoke: (cmd, args) => {
        window.__calls.push([cmd, args]);
        try { return Promise.resolve(answer(cmd, args || {})); } catch (e) { return Promise.reject(e); }
      },
    },
    event: { listen: (name, cb) => { listeners[name] = cb; return Promise.resolve(() => {}); } },
  };
  window.__mockState = (s) => listeners["persona://state"] && listeners["persona://state"]({ payload: s });
  // after the first render: what the person typed in this state, through the page's own handlers
  if (setup.type) addEventListener("DOMContentLoaded", () => setTimeout(function go(n) {
    const f = document.querySelector('[data-field="' + setup.type.field + '"]');
    if (!f) return (n || 0) < 20 && setTimeout(() => go((n || 0) + 1), 30);
    const adv = f.closest("details");
    if (adv) adv.open = true;
    f.focus();
    f.value = setup.type.value;
    f.dispatchEvent(new Event("input"));
    if (setup.type.field !== "notes") f.dispatchEvent(new Event("change"));
  }, 60));
})();
