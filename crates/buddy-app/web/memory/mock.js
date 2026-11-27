// Browser-only parity mock: `index.html?mock=<state>` installs a fake `__TAURI__` that answers the
// memory_* commands for one of the What I remember window's states (the prototype's ids):
//   `loading` (memory_state never answers: the skeleton), `empty` (on, nothing kept), `few` (three groups,
//   seven rows), `many` (40 rows, long texts: the list scrolls, the bar stays), `disputed` (two questions
//   first, their rows marked), `off` (the switch off: rows stay, quieter), `error` (the store didn't open),
//   `partial` (the list loaded; forgetting "Sister: Lan" fails and the bar offers Try again).
// The overlays are reached through the page's own handlers: ⋯ (row menu, Erase completely… and its
// confirm), Edit / + Add (in-place forms; an add that looks like a password or key is refused with
// `secret`), Forget everything… (the typed-phrase dialog). Every person and place is invented.
// Inside the app (`__TAURI__` present) or without ?mock it does nothing. Commands the page invokes are
// recorded in `window.__calls`; `window.__mockState(s)` pushes a `memory://state` like the app.
"use strict";
(function () {
  const state = new URLSearchParams(location.search).get("mock");
  if (window.__TAURI__ || !state) return;

  const PHRASE = "forget everything";
  const LABEL = { home_city: "City", timezone: "Time zone", job: "Job", employer: "Employer", language: "Language", reply_style: "Reply style", checkin_hours: "Check-in hours", likes: "Likes", people: "Name", pets: "Name", projects: "Project" };
  let next = 100;
  // a live row: [text, source, since?, key?, value?]
  const R = (kind) => ([text, source, since, key, value]) => ({
    id: next++, kind, text, key: key || null, label: key ? LABEL[key.split(":")[0]] : null,
    value: value || null, since: since || null, source, disputed: false,
  });
  const P = R("profile"), H = R("preference"), D = R("promise");

  const FEW = {
    about: [
      P(["Lives in Hanoi", "chat", "2026-10", "home_city", "Hanoi"]),
      P(["Works as a software engineer", "chat", null, "job", "software engineer"]),
      P(["Has a cat, Mochi", "guess", null, "pets:cat", "Mochi"]),
      P(["Sister: Lan", "window"]),
    ],
    help: [
      H(["Short replies", "chat", null, "reply_style", "short"]),
      H(["No check-ins before 10:00", "chat", null, "checkin_hours", "after 10:00"]),
    ],
    promises: [D(["Ask how Mochi's vet visit went", "bear"])],
  };

  function many() {
    const about = [
      ["Is training for a half marathon in March and runs along the river before work", "chat", "2026-09"],
      ["Lives in Hanoi", "chat", "2026-10", "home_city", "Hanoi"],
      ["Works at Lanternfish Labs, a twelve-person studio making tools for hobby printers", "chat", null, "employer", "Lanternfish Labs"],
      ["Speaks Vietnamese", "chat", null, "language", "Vietnamese"],
      ["Speaks English", "guess", null, "language", "English"],
      ["Has a cat, Mochi, who knocks the water glass off the desk during morning calls", "guess", null, "pets:cat", "Mochi"],
      ["Sister: Lan", "window"],
      ["Is building a memory feature for a desktop bear and wants it to feel trustworthy", "chat", null, "projects", "desktop bear memory"],
      ["Time zone: Asia/Ho_Chi_Minh", "bear", null, "timezone", "Asia/Ho_Chi_Minh"],
      ["Plays badminton with friends from the old office on Thursday evenings near the lake", "guess"],
      ["Grew up in Saigon and moved north for university, then stayed for the street food", "chat", "2019-09"],
      ["Keeps a paper notebook for ideas and copies the good ones into a plain text file later", "guess"],
      ["Has a brother, Minh, who studies architecture and sends sketches of bridges", "chat", null, "people:brother", "Minh"],
      ["Rides a blue scooter to work unless it rains, then takes the number 32 bus instead", "guess"],
      ["Is learning to play the đàn bầu from weekly lessons with a teacher in the Old Quarter", "chat", "2026-08"],
      ["Prefers working in the café downstairs on Friday mornings when the office is loud", "guess"],
      ["Has a small balcony garden with basil, chili and a lemon tree that never fruits", "chat"],
      ["Is saving for a trip to Hội An in the spring with two friends from the running club", "chat"],
      ["Reads science fiction before bed, mostly older paperbacks found at the book street", "guess"],
      ["Volunteers at the animal shelter on the first Saturday of every month in the morning", "chat"],
      ["Cooks phở from scratch on Sundays and freezes the broth in small jars for the week", "guess"],
      ["Studied computer science and wrote a thesis about scheduling for tiny embedded chips", "chat"],
      ["Uses a split keyboard and a vertical mouse after a stretch of wrist pain last winter", "chat", "2025-12"],
      ["Has a neighbour, Thảo, who waters the plants whenever the trips run longer than a week", "window"],
    ].map(P);
    const help = [
      ["Short replies", "chat", null, "reply_style", "short"],
      ["Prefers code first and the explanation after, with no bullet lists inside replies", "chat"],
      ["Likes green tea", "guess", null, "likes", "green tea"],
      ["No check-ins before 10:00 on weekdays, and none at all during the Sunday long run", "chat", null, "checkin_hours", "after 10:00"],
      ["Wants a plain summary line at the end of anything longer than a couple of paragraphs", "window"],
      ["Likes it when a fix explains what was wrong first, then what changed, then how to test", "chat"],
      ["Would rather hear a short no than a long maybe when something isn't going to work", "chat"],
      ["Prefers examples with real file names from the project over made-up placeholders", "guess"],
      ["Asks for metric units and twenty-four-hour times, and dates written day before month", "chat"],
      ["Dislikes cheerful filler at the start of a reply; just get to the point right away", "chat"],
      ["Likes a gentle nudge to stand up and stretch after about ninety minutes of typing", "guess"],
    ].map(H);
    const promises = [
      ["Ask how the half marathon training went after the long Sunday run along the river", "bear"],
      ["Ask how Mochi's vet visit went", "bear"],
      ["Remind about the đàn bầu lesson on Wednesday afternoon if the calendar stays empty", "bear"],
      ["Check whether the balcony lemon tree finally flowered after the new fertiliser", "bear"],
      ["Bring up the Hội An trip budget again once the flights for the spring are announced", "bear"],
    ].map(D);
    return { about, help, promises };
  }

  function disputed() {
    const g = { about: FEW.about.map((r) => ({ ...r })), help: [{ ...FEW.help[0] }], promises: [] };
    g.about[0].disputed = true;
    g.about[1] = { ...g.about[1], source: "bear", disputed: true };
    const disputes = [
      { id: next++, key: "home_city", label: "City", value: "Da Nang", text: "Lives in Da Nang", live: { id: g.about[0].id, value: "Hanoi", source: "chat" } },
      { id: next++, key: "job", label: "Job", value: "engineering manager for the platform and developer tools teams at a small startup",
        text: "Works as an engineering manager for the platform and developer tools teams at a small startup", live: { id: g.about[1].id, value: "software engineer", source: "bear" } },
    ];
    return { g, disputes };
  }

  const clone = (g) => ({ about: g.about.map((r) => ({ ...r })), help: g.help.map((r) => ({ ...r })), promises: g.promises.map((r) => ({ ...r })) });
  let groups = { about: [], help: [], promises: [] };
  let disputes = [];
  let enabled = true;
  let error = null;
  let failForget = false;
  let silent = false;
  switch (state) {
    case "few": groups = clone(FEW); break;
    case "many": groups = many(); break;
    case "disputed": { const d = disputed(); groups = d.g; disputes = d.disputes; break; }
    case "off": groups = { about: FEW.about.slice(0, 3).map((r) => ({ ...r })), help: [{ ...FEW.help[0] }], promises: [] }; enabled = false; break;
    case "error": error = { code: "store", message: "unable to open database file", file: "~/.desk-buddy/memory/memory.sqlite" }; break;
    case "partial": groups = { about: [{ ...FEW.about[0] }, { ...FEW.about[3] }], help: [], promises: [] }; failForget = true; break;
    case "loading": silent = true; break;
    default: break; // empty
  }

  const GROUP_OF = { profile: "about", preference: "help", promise: "promises" };
  const all = () => [...groups.about, ...groups.help, ...groups.promises];
  const find = (id) => all().find((r) => r.id === id);
  function view() {
    const live = error ? 0 : all().length;
    const ds = error ? [] : disputes;
    const disputedIds = new Set(ds.map((d) => d.live.id));
    const g = error ? { about: [], help: [], promises: [] } : clone(groups);
    for (const k of Object.keys(g)) g[k].forEach((r) => { r.disputed = disputedIds.has(r.id); });
    return {
      v: 1, enabled, error, today: "2026-11-27", phrase: PHRASE, groups: g, disputes: ds.map((d) => ({ ...d, live: { ...d.live } })),
      history: null, historyMore: false, counts: { live, disputes: ds.length },
    };
  }
  const looksSecret = (t) => /\b(password|passcode|api[ _-]?key|token)\b/i.test(t) || /\b(sk|ghp|xox[bp])[-_][A-Za-z0-9]/.test(t);
  function checkText(t, max) {
    const v = (t || "").trim();
    if (!v) throw "empty";
    if ([...v].length > max) throw "too_long";
    if (looksSecret(v)) throw "secret";
    return v;
  }
  function remove(id) {
    for (const k of Object.keys(groups)) groups[k] = groups[k].filter((r) => r.id !== id);
    disputes = disputes.filter((d) => d.live.id !== id && d.id !== id);
  }

  const listeners = {};
  const push = () => listeners["memory://state"] && setTimeout(() => listeners["memory://state"]({ payload: view() }), 0);
  function answer(cmd, a) {
    switch (cmd) {
      case "memory_state": return view();
      case "memory_add": {
        if (error) throw "store";
        const text = checkText(a.text, 200);
        const g = GROUP_OF[a.kind];
        if (!g) throw "store";
        groups[g].unshift({ id: next++, kind: a.kind, text, key: null, label: null, value: null, since: null, source: "window", disputed: false });
        push();
        return { id: next - 1 };
      }
      case "memory_edit": {
        const r = find(a.id);
        if (!r) throw "not_found";
        const text = checkText(a.text, 200);
        if (a.value != null) checkText(a.value, 100);
        const n = { ...r, id: next++, text, value: r.key ? a.value || r.value : null, source: "window", since: a.value && a.value !== r.value ? null : r.since };
        const g = groups[GROUP_OF[r.kind]];
        g.splice(g.indexOf(r), 1, n);
        disputes = disputes.filter((d) => d.live.id !== r.id);
        push();
        return { id: n.id };
      }
      case "memory_forget": {
        if (failForget && !a.hard) throw "store";
        if (!find(a.id)) throw "not_found";
        remove(a.id);
        push();
        return {};
      }
      case "memory_resolve": {
        const d = disputes.find((x) => x.id === a.id);
        if (!d) throw "not_found";
        if (a.keep !== "new" && a.keep !== "old") throw "store";
        const live = find(d.live.id);
        if (a.keep === "new" && live) Object.assign(live, { id: d.id, text: d.text, value: d.value, source: "window", since: null });
        else if (live) live.source = "window";
        disputes = disputes.filter((x) => x.id !== a.id);
        push();
        return {};
      }
      case "memory_forget_all": {
        if ((a.confirm || "").trim() !== PHRASE) throw "bad_phrase";
        groups = { about: [], help: [], promises: [] };
        disputes = [];
        error = null;
        failForget = false;
        push();
        return {};
      }
      case "memory_set_enabled": enabled = !!a.on; push(); return {};
      case "memory_close": return {};
    }
    throw "not allowed";
  }

  window.__calls = [];
  window.__TAURI__ = {
    core: {
      invoke: (cmd, args) => {
        window.__calls.push([cmd, args]);
        if (silent && cmd === "memory_state") return new Promise(() => {});
        try { return Promise.resolve(answer(cmd, args || {})); } catch (e) { return Promise.reject(e); }
      },
    },
    event: { listen: (name, cb) => { listeners[name] = cb; return Promise.resolve(() => {}); } },
  };
  window.__mockState = (s) => listeners["memory://state"] && listeners["memory://state"]({ payload: s });
  // partial: the person pressed Forget on "Sister: Lan" and the store refused it
  if (state === "partial") addEventListener("DOMContentLoaded", () => setTimeout(function go(n) {
    const b = document.querySelector('[data-k="forget:' + groups.about[1].id + '"]');
    if (!b) return (n || 0) < 20 && setTimeout(() => go((n || 0) + 1), 30);
    b.click();
  }, 60));
})();
