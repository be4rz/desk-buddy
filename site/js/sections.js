// Every section below the hero. Each one starts when it scrolls near and stops when it
// leaves, so only what's on screen animates.

import * as S from './sprites.js';

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

/* ---------- a canvas holding a room around one anchor (floor, scene, set, bear) ---------- */

class RoomCanvas {
  constructor(frame, { max = 3, fill = 1 } = {}) {
    this.frame = frame;
    this.canvas = frame.querySelector('canvas');
    this.max = max;
    this.fill = fill;
    this.actor = new S.Actor({ dir: 'SE' });
    this.floor = null;
    this.scene = null;
    this.props = null;
    this.hour = new Date().getHours();
    this.dirty = true;
  }

  async setup(layers) {
    await Promise.all(layers.map((l) => S.image(l.file)));
    this.b = await S.roomBounds(layers);
    this.ctx = S.pixelCanvas(this.canvas, this.b.w, this.b.h);
    new ResizeObserver(() => this.resize()).observe(this.frame);
    this.resize();
  }

  resize() {
    const avail = this.frame.clientWidth * this.fill;
    this.z = S.fitZoom(this.b.w, avail, this.max);
    this.canvas.style.width = `${Math.round(this.b.w * this.z)}px`;
    this.canvas.style.height = `${Math.round(this.b.h * this.z)}px`;
    this.dirty = true;
  }

  draw() {
    const { ctx, b } = this;
    ctx.clearRect(0, 0, b.w, b.h);
    const inSet = this.actor.c && this.actor.c.set;
    S.drawRoom(ctx, b.ax, b.ay, {
      floor: this.floor, scene: this.scene, hour: this.hour, actor: this.actor, props: inSet ? null : this.props,
    });
  }

  tick(dt) {
    if (this.actor.step(dt) || this.dirty) {
      this.dirty = false;
      this.draw();
    }
  }

  /** The top centre of the bear's box, in frame pixels. */
  head() {
    const cr = this.canvas.getBoundingClientRect();
    const fr = this.frame.getBoundingClientRect();
    const [x0, y0, x1] = this.actor.box();
    return {
      x: cr.left - fr.left + (this.b.ax + (x0 + x1) / 2) * this.z,
      y: cr.top - fr.top + (this.b.ay + y0) * this.z,
    };
  }
}

/** Start ticking `tick` while el is near the viewport. */
function animateWhileVisible(el, tick, margin = '150px') {
  let stop = null;
  S.whenVisible(el, () => { stop = S.onTick(tick); }, () => { if (stop) stop(); stop = null; }, margin);
}

/** Run fn once, the first time el comes near. */
function onceNear(el, fn, margin = '600px') {
  const io = new IntersectionObserver((es) => {
    if (es.some((e) => e.isIntersecting)) {
      io.disconnect();
      fn();
    }
  }, { rootMargin: margin });
  io.observe(el);
}

const layerOf = (thing) => ({ file: thing.file || thing.back, origin: thing.origin });

function sceneLayers(ids) {
  const out = [];
  for (const id of ids) {
    const s = S.sceneById(id);
    if (!s) continue;
    for (const v of s.variants) out.push({ file: v.back, origin: s.origin });
  }
  return out;
}

function propsLayer(set) {
  const s = S.manifest().sets[set];
  return { file: s.props, origin: [s.handoff[0] + s.ax, s.handoff[1] + s.ay] };
}

function hourLabel(h, m = 0) {
  const d = new Date(2026, 0, 1, h, m);
  return new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' }).format(d);
}

/* ---------- the speech bubble used on stages (the app's bubble style) ---------- */

class StageBubble {
  constructor(el, room) {
    this.el = el;
    this.room = room;
    this.body = el.querySelector('.bubble-body');
  }

  show(node) {
    this.body.replaceChildren(node);
    this.el.hidden = false;
    this.el.classList.remove('is-out');
    this.place();
  }

  text(t) {
    const p = document.createElement('p');
    p.className = 'bubble-text';
    p.textContent = t;
    this.show(p);
    return p;
  }

  async type(t, alive, cps = 38) {
    const p = this.text('');
    const chars = [...t];
    if (S.reduceMotion.matches) { p.textContent = t; this.place(); return true; }
    for (let i = 1; i <= chars.length; i++) {
      if (!alive()) return false;
      p.textContent = chars.slice(0, i).join('');
      this.place();
      await S.sleep(1000 / cps);
    }
    return alive();
  }

  hide() {
    if (this.el.hidden) return;
    this.el.classList.add('is-out');
    clearTimeout(this.t);
    this.t = setTimeout(() => { this.el.hidden = true; }, 180);
  }

  place() {
    if (this.el.hidden) return;
    const h = this.room.head();
    const fw = this.room.frame.clientWidth;
    const bw = this.el.offsetWidth;
    const bh = this.el.offsetHeight;
    const left = Math.max(10, Math.min(fw - bw - 10, h.x - bw / 2));
    const top = Math.max(10, h.y - bh - 14);
    this.el.style.transform = `translate3d(${Math.round(left)}px, ${Math.round(top)}px, 0)`;
    this.el.style.setProperty('--tail', `${Math.round(Math.max(18, Math.min(bw - 18, h.x - left)))}px`);
  }
}

/* ---------- A day with 2poke ---------- */

const DAY = [
  { h: 7, m: 0, phase: 'dawn', label: 'Morning', floor: 'parquet', scene: 'nook',
    seq: [['morning_stretch', 'S'], ['greet', 'S'], ['idle', 'S']],
    title: 'Up when you are', text: 'The first time you open your Mac in the morning, it gives a big stretch and says hello.' },
  { h: 10, m: 30, phase: 'day', label: 'Focus', floor: 'parquet', scene: 'nook',
    seq: [['laptop_sit'], ['laptop_type']],
    title: 'Works next to you', text: 'While you work, it types away on its own little laptop by the window.' },
  { h: 12, m: 30, phase: 'noon', label: 'Lunch', floor: 'tile', scene: 'kitchen',
    seq: [['snack_sit'], ['snack_munch']],
    title: 'Takes a snack break', text: 'Around lunchtime it finds a cookie in its tiny kitchen.' },
  { h: 15, m: 30, phase: 'afternoon', label: 'Tea', floor: 'tatami', scene: 'tearoom',
    seq: [['tea_sit'], ['tea_sip']],
    title: 'Tea in the afternoon', text: 'Mid-afternoon it sits on a cushion in the tea room and sips from a mug.' },
  { h: 20, m: 0, phase: 'evening', label: 'Evening', floor: 'parquet', scene: 'library',
    seq: [['sit_down'], ['read']],
    title: 'Reads by the fire', text: 'In the evening it settles into an armchair with a book.' },
  { h: 23, m: 0, phase: 'night', label: 'Night', floor: 'parquet', scene: 'nook',
    seq: [['cap_get_in_bed'], ['cap_sleep']],
    title: 'Lights out', text: `Late at night it pulls on a nightcap and goes to bed. Quiet hours start at ${hourLabel(22, 30)}, so it won’t check in until morning.` },
];
const DAY_MS = 9000;

async function initDay() {
  const sec = $('#day');
  if (!sec) return;
  const frame = $('.day-stage', sec);
  const room = new RoomCanvas(frame, { max: 3 });
  const tabs = $('.day-times', sec);
  const card = $('.day-card', sec);
  const bar = $('.day-progress span', sec);
  const clips = DAY.flatMap((d) => d.seq.map(([n]) => n));
  const floors = [...new Set(DAY.map((d) => d.floor))].map((id) => layerOf(S.floorById(id)));
  await Promise.all([S.preload(clips), room.setup([...floors, ...sceneLayers([...new Set(DAY.map((d) => d.scene))])])]);

  DAY.forEach((d, i) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.setAttribute('role', 'tab');
    b.dataset.i = i;
    b.innerHTML = `<span class="t">${hourLabel(d.h, d.m)}</span><span class="l">${d.label}</span>`;
    b.addEventListener('click', () => { held = performance.now() + 20000; show(i); });
    tabs.appendChild(b);
  });

  let cur = -1;
  let gen = 0;
  let shownAt = 0;
  let held = 0;
  let visible = false;

  async function show(i) {
    cur = i;
    const g = ++gen;
    const d = DAY[i];
    shownAt = performance.now();
    sec.dataset.phase = d.phase;
    $$('button', tabs).forEach((b, j) => {
      b.setAttribute('aria-selected', String(j === i));
      b.tabIndex = j === i ? 0 : -1;
    });
    card.innerHTML = `<p class="kicker">${hourLabel(d.h, d.m)}</p><h3>${d.title}</h3><p>${d.text}</p>`;
    room.floor = S.floorById(d.floor);
    room.scene = S.sceneById(d.scene);
    room.hour = d.h;
    room.props = null;
    room.dirty = true;
    const a = room.actor;
    a.dir = 'SE';
    for (const [name, row] of d.seq) {
      const last = name === d.seq[d.seq.length - 1][0];
      const ok = await a.play(name, { row, loops: last ? Infinity : 1 });
      if (!ok || g !== gen) return;
    }
  }

  tabs.addEventListener('keydown', (e) => {
    const k = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
    if (!k) return;
    e.preventDefault();
    held = performance.now() + 20000;
    const n = (cur + k + DAY.length) % DAY.length;
    show(n);
    $$('button', tabs)[n].focus();
  });

  // start at the visitor's own time of day
  const now = new Date();
  const mins = now.getHours() * 60 + now.getMinutes();
  let start = DAY.length - 1;
  for (let i = 0; i < DAY.length; i++) if (DAY[i].h * 60 + DAY[i].m <= mins) start = i;
  if (mins < DAY[0].h * 60) start = DAY.length - 1;
  $('.day-now', sec).textContent = `It’s ${hourLabel(now.getHours(), now.getMinutes())} where you are, so the bear starts at ${DAY[start].label}.`;
  show(start);

  animateWhileVisible(frame, (dt, t) => {
    room.tick(dt);
    const auto = !S.reduceMotion.matches && t > held;
    const p = Math.min(1, (t - shownAt) / DAY_MS);
    bar.style.transform = `scaleX(${auto ? p : 0})`;
    if (auto && p >= 1 && visible) show((cur + 1) % DAY.length);
  });
  S.whenVisible(frame, () => { visible = true; shownAt = performance.now(); }, () => { visible = false; }, '-20% 0px');
}

/* ---------- Talk: the bubble, approvals and three scripted asks ---------- */

const MODES = {
  ask: 'It asks before every command and every file it changes.',
  allowlist: 'Safe, read-only commands just run. Anything that changes your files asks first.',
  full: 'Everything runs except risky commands, sensitive files and anything you’ve said to ask about. Deny rules still deny.',
};

async function initTalk() {
  const sec = $('#talk');
  if (!sec) return;
  const frame = $('.talk-stage', sec);
  const room = new RoomCanvas(frame, { max: 3, fill: 0.92 });
  const bubble = new StageBubble($('.bubble', frame), room);
  const np = $('.np-widget', frame);
  const skip = $('.time-skip', frame);
  const clips = ['idle', 'turn_cw', 'turn_ccw', 'listen_in', 'listen', 'listen_out', 'think', 'ask', 'talk', 'nod',
    'shrug', 'thumbs_up', 'head_bob', 'remind', 'happy', 'fid_wave_small', 'desk_sit', 'desk_type', 'desk_swivel_in',
    'desk_talk', 'desk_swivel_out', 'desk_stand', 'point_bubble'];
  const floor = S.floorById('parquet');
  await Promise.all([S.preload(clips), room.setup([layerOf(floor), propsLayer('desk')])]);
  room.floor = floor;
  room.props = 'desk';
  const a = room.actor;
  a.dir = 'S';
  a.play('idle', { row: 'S' });

  let mode = 'allowlist';
  const note = $('.mode-note', sec);
  $$('input[name="mode"]', sec).forEach((r) => r.addEventListener('change', () => {
    mode = r.value;
    note.textContent = MODES[mode];
  }));
  note.textContent = MODES[mode];

  let gen = 0;
  const prompts = $$('[data-prompt]', sec);

  async function run(fn, btn) {
    const g = ++gen;
    const alive = () => g === gen;
    prompts.forEach((p) => p.setAttribute('aria-pressed', String(p === btn)));
    skip.hidden = true;
    try {
      await fn(alive);
    } catch (e) {
      console.error(e);
    }
    if (!alive()) return;
    prompts.forEach((p) => p.setAttribute('aria-pressed', 'false'));
    if (!(a.c && a.c.set)) a.play('idle');
  }

  const play = async (alive, name, opts) => (await a.play(name, opts)) && alive();
  const front = async (alive) => (await a.turnTo(S.nearestFront(a.dir))) && alive();

  /** Show the user's words typed into the bubble's input, then listen. */
  async function hear(alive, words) {
    if (a.c && a.c.set) {
      if (!(await play(alive, 'desk_stand'))) return false;
    }
    if (!(await front(alive))) return false;
    const box = document.createElement('div');
    box.className = 'bubble-input';
    box.innerHTML = '<span class="typed"></span><span class="caret" aria-hidden="true"></span>';
    bubble.show(box);
    a.play('listen_in').then((ok) => ok && alive() && a.play('listen'));
    const typed = $('.typed', box);
    for (const ch of words) {
      if (!alive()) return false;
      typed.textContent += ch;
      bubble.place();
      await S.sleep(S.reduceMotion.matches ? 0 : 42);
    }
    await S.sleep(500);
    if (!alive()) return false;
    if (!(await play(alive, 'listen_out'))) return false;
    return true;
  }

  async function think(alive, ms) {
    const dots = document.createElement('div');
    dots.className = 'dots';
    dots.setAttribute('aria-label', 'Thinking');
    dots.innerHTML = '<i></i><i></i><i></i>';
    bubble.show(dots);
    a.play('think');
    await S.sleep(ms);
    return alive();
  }

  async function speak(alive, text, { seated = false } = {}) {
    a.talking = !seated;
    a.play(seated ? 'desk_talk' : 'talk');
    const ok = await bubble.type(text, alive);
    a.talking = false;
    return ok;
  }

  function approval(alive) {
    return new Promise((resolve) => {
      const card = $('#approval-tpl').content.firstElementChild.cloneNode(true);
      bubble.show(card);
      a.play('ask');
      const done = (v) => { if (alive()) resolve(v); };
      $$('button', card).forEach((b) => b.addEventListener('click', () => done(b.dataset.a)));
      const watch = setInterval(() => { if (!alive()) { clearInterval(watch); resolve(null); } }, 200);
      $('button.primary', card).focus({ preventScroll: true });
    });
  }

  const scripts = {
    async tidy(alive, btn) {
      if (!(await hear(alive, btn.dataset.say))) return;
      if (!(await think(alive, 1400))) return;
      let choice = 'auto';
      if (mode !== 'full') {
        choice = await approval(alive);
        if (!choice || !alive()) return;
      }
      if (choice === 'deny') {
        a.play('shrug');
        await bubble.type('Okay, nothing ran. Your Downloads folder is just how you left it.', alive);
        if (!alive()) return;
        await S.sleep(2600);
        if (alive()) bubble.hide();
        return;
      }
      if (choice === 'always') {
        bubble.text('Got it. I won’t ask about this one again.');
        await S.sleep(1500);
        if (!alive()) return;
      } else if (choice === 'auto') {
        bubble.text('Full access is on, so this one just runs.');
        await S.sleep(1500);
        if (!alive()) return;
      }
      bubble.hide();
      if (!(await a.turnTo('SE')) || !alive()) return;
      if (!(await play(alive, 'desk_sit'))) return;
      const status = document.createElement('div');
      status.className = 'run-status';
      status.innerHTML = '<span class="spin" aria-hidden="true"></span><span>Sorting <b>0</b> of 23 files…</span>';
      bubble.show(status);
      a.play('desk_type');
      const n = $('b', status);
      for (let i = 1; i <= 23; i++) {
        await S.sleep(S.reduceMotion.matches ? 10 : 110);
        if (!alive()) return;
        n.textContent = String(i);
      }
      if (!(await play(alive, 'desk_swivel_in'))) return;
      if (!(await speak(alive, 'Done! 14 images, 6 documents and 3 installers, each in its own folder. Nothing was deleted.', { seated: true }))) return;
      await S.sleep(1800);
      if (!alive()) return;
      if (!(await play(alive, 'desk_swivel_out'))) return;
      bubble.hide();
      if (!(await play(alive, 'desk_stand'))) return;
      if (!(await a.turnTo('S')) || !alive()) return;
      await play(alive, 'thumbs_up');
    },

    async music(alive, btn) {
      if (!(await hear(alive, btn.dataset.say))) return;
      if (!(await think(alive, 900))) return;
      a.play('nod');
      if (!(await bubble.type('Putting on something calm. I’ll keep it low.', alive))) return;
      np.hidden = false;
      np.classList.add('is-playing');
      await S.sleep(900);
      if (!alive()) return;
      bubble.hide();
      await play(alive, 'head_bob', { loops: 5 });
    },

    async remind(alive, btn) {
      if (!(await hear(alive, btn.dataset.say))) return;
      if (!(await think(alive, 900))) return;
      a.play('nod');
      if (!(await bubble.type(`Okay! At ${hourLabel(15)} I’ll remind you to stretch and refill your water.`, alive))) return;
      await S.sleep(2200);
      if (!alive()) return;
      bubble.hide();
      if (!(await play(alive, 'idle', { loops: 1 }))) return;
      skip.textContent = `${hourLabel(15)}`;
      skip.hidden = false;
      await S.sleep(1400);
      if (!alive()) return;
      skip.hidden = true;
      a.play('remind');
      const r = document.createElement('div');
      r.className = 'reminder';
      r.innerHTML = '<p class="kicker">Reminder</p><p class="bubble-text">Stretch and refill your water.</p><div class="approve-actions"><button type="button" class="btn-s primary">Done</button></div>';
      bubble.show(r);
      const done = new Promise((res) => {
        $('button', r).addEventListener('click', () => res(true));
        setTimeout(() => res(false), 9000);
      });
      const clicked = await done;
      if (!alive()) return;
      bubble.hide();
      if (clicked) await play(alive, 'happy');
      else await play(alive, 'fid_wave_small');
    },

    async other(alive, btn, words) {
      if (!(await hear(alive, words))) return;
      if (!(await think(alive, 1300))) return;
      if (!(await speak(alive, 'On your Mac I’d answer that for real. This page is only a demo, so join the waitlist and ask me again soon!'))) return;
      await S.sleep(2400);
      if (!alive()) return;
      bubble.hide();
      await play(alive, 'point_bubble');
    },
  };

  prompts.forEach((btn) => btn.addEventListener('click', () => run((alive) => scripts[btn.dataset.prompt](alive, btn), btn)));
  const form = $('.ask-form', sec);
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    const input = $('input', form);
    const words = input.value.trim().slice(0, 80);
    if (!words) { input.focus(); return; }
    input.value = '';
    run((alive) => scripts.other(alive, null, words), null);
  });
  $('.np-close', np).addEventListener('click', () => { np.hidden = true; np.classList.remove('is-playing'); });

  animateWhileVisible(frame, (dt) => {
    room.tick(dt);
    bubble.place();
  });
}

/* ---------- senses: six little loops ---------- */

const SENSES = {
  music: [['head_bob', 'SE', 3], ['idle', 'SE', 1]],
  edge: [['edge_sit', 'SE', 1], ['edge_dangle', 'SE', 3], ['edge_peer', 'SE', 1], ['edge_dangle', 'SE', 2], ['edge_stand', 'SE', 1], ['idle', 'SE', 1]],
  cursor: [['cursor_watch', 'SW', 2], ['flinch', 'SW', 1], ['idle', 'SW', 1], ['giggle', 'SW', 1]],
  drag: [['dangle', 'S', 3], ['land_squash', 'S', 1], ['idle', 'S', 1], ['dangle', 'S', 2], ['land_squash', 'S', 1], ['dizzy', 'S', 1]],
  away: [['tap_glass', 'S', 1], ['idle', 'S', 1], ['curl_down', 'S', 1], ['curl_nap', 'S', 3], ['curl_up', 'S', 1]],
  wait: [['foot_tap', 'SE', 4], ['check_watch', 'SE', 1], ['foot_tap', 'SE', 2]],
};

async function initSenses() {
  const cards = $$('[data-sense]');
  for (const card of cards) {
    const seq = SENSES[card.dataset.sense];
    if (!seq) continue;
    const canvas = $('canvas', card);
    const ctx = S.pixelCanvas(canvas, 140, 150);
    const a = new S.Actor({ dir: seq[0][1] });
    let i = 0;
    let started = false;
    let ready = false;
    let dirty = true;
    const next = async () => {
      for (;;) {
        const [name, row, loops] = seq[i];
        i = (i + 1) % seq.length;
        const ok = await a.play(name, { row, loops });
        if (!ok) return;
      }
    };
    onceNear(card, async () => {
      await S.preload([...new Set(seq.map((s) => s[0]))]);
      a.pose(seq[0][0], 0, seq[0][1]);
      ready = true;
      dirty = true;
    });
    animateWhileVisible(card, (dt) => {
      if (!ready) return;
      if (!started) { started = true; next(); }
      if (a.step(dt) || dirty) {
        dirty = false;
        ctx.clearRect(0, 0, 140, 150);
        a.draw(ctx, 70, 116);
      }
    });
  }
}

/* ---------- playground: floors, scenes, activities, emotes, turning ---------- */

const ACTS = {
  tea: ['tea_sit', 'tea_sip', 'tea_stand'],
  snack: ['snack_sit', 'snack_munch', 'snack_stand'],
  knit: ['knit_sit', 'knit', 'knit_stand'],
  laptop: ['laptop_sit', 'laptop_type', 'laptop_stand'],
  notebook: ['note_sit', 'note_doodle', 'note_stand'],
  read: ['sit_down', 'read', 'stand_up'],
  computer: ['desk_sit', 'desk_type', 'desk_stand'],
  headphones: ['phones_on', 'phones_vibe', 'phones_off'],
  umbrella: ['umbrella_open', 'umbrella_rain', 'umbrella_close'],
  ball: ['ball_get', 'ball_bounce', 'ball_drop'],
  bubbles: ['bubbles_get', 'bubbles_blow', 'bubbles_put'],
  yoyo: ['yoyo_get', 'yoyo', 'yoyo_put'],
  blocks: ['blocks_sit', 'blocks_build', 'blocks_stand'],
  balloon: ['balloon_get', 'balloon_bob', 'balloon_let_go'],
  plant: ['plant_get', 'plant_water', 'plant_put'],
  sweep: ['sweep_get', 'sweep', 'sweep_put'],
  snowman: ['snow_kneel', 'snow_build', 'snow_stand'],
  fishing: ['fish_sit', 'fish_wait', 'fish_stand'],
  nightcap: ['cap_get_in_bed', 'cap_sleep', 'cap_wake_up'],
};
const EMOTES = ['wave', 'nod', 'jump', 'spin', 'clap', 'laugh', 'proud', 'shrug', 'facepalm', 'thumbs_up', 'shy_hide',
  'pout', 'head_scratch', 'greet', 'hop', 'dance', 'moonwalk', 'peekaboo', 'windmill', 'find_coin', 'hiccups'];

async function initPlayground() {
  const sec = $('#rooms');
  if (!sec) return;
  const frame = $('.play-stage', sec);
  const room = new RoomCanvas(frame, { max: 3 });
  const M = S.manifest();
  const floors = M.room.floors;
  const sceneFor = (act) => M.room.scenes.find((s) => s.for.includes(act));
  const scenes = ['nook', 'kitchen', 'tearoom', 'library'];
  await Promise.all([
    S.preload(['idle', 'turn_cw', 'turn_ccw']),
    room.setup([...floors.map(layerOf), ...sceneLayers(scenes)]),
  ]);
  room.floor = floors.find((f) => f.id === 'parquet');
  room.actor.dir = 'SE';
  room.actor.play('idle', { row: 'SE' });
  const a = room.actor;
  const cmd = $('[data-cmd]', sec);
  const dirOut = $('[data-dir]', sec);
  const scenesOn = $('[data-scenes]', sec);
  let gen = 0;

  const say = (line) => {
    cmd.textContent = line;
    cmd.parentElement.classList.remove('flash');
    void cmd.parentElement.offsetWidth;
    cmd.parentElement.classList.add('flash');
  };

  // floors
  const floorBox = $('[data-floors]', sec);
  const floorChips = [{ id: 'none', name: 'None' }, ...floors];
  for (const f of floorChips) {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'chip';
    b.textContent = f.name;
    b.setAttribute('aria-pressed', String(f.id === 'parquet'));
    b.addEventListener('click', () => {
      room.floor = f.id === 'none' ? null : f;
      room.dirty = true;
      $$('.chip', floorBox).forEach((c) => c.setAttribute('aria-pressed', String(c === b)));
      say(`Tray ▸ Floor ▸ ${f.name}`);
    });
    floorBox.appendChild(b);
  }

  const EXITS = new Set(Object.values(ACTS).map((l) => l[2]));

  async function stopActivity(alive) {
    if (a.c && a.c.set) {
      const chain = Object.values(ACTS).find((l) => l.includes(a.name));
      if (chain && chain[2] !== a.name) {
        if (!(await a.play(chain[2])) || !alive()) return false;
      }
      a.play('idle', { row: 'SE' });
    }
    room.scene = null;
    room.dirty = true;
    return alive();
  }

  async function run(fn) {
    const g = ++gen;
    const alive = () => g === gen;
    try {
      await fn(alive);
    } catch (e) {
      console.error(e);
    }
    if (alive() && (!(a.c && a.c.set) || EXITS.has(a.name))) a.play('idle');
  }

  // activities
  const actBox = $('[data-acts]', sec);
  for (const [act, [enter, loop, exit]] of Object.entries(ACTS)) {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'chip';
    b.textContent = act;
    b.addEventListener('click', () => run(async (alive) => {
      say(`buddy mascot do ${act} --for 5m`);
      b.classList.add('is-loading');
      await S.preload([enter, loop, exit]);
      b.classList.remove('is-loading');
      if (!alive()) return;
      if (!(await stopActivity(alive))) return;
      if (!(await a.turnTo('SE')) || !alive()) return;
      const sc = sceneFor(act);
      room.scene = scenesOn.checked && sc ? sc : null;
      room.hour = new Date().getHours();
      room.dirty = true;
      if (!(await a.play(enter)) || !alive()) return;
      const c = S.clip(loop);
      const loops = Math.max(1, Math.round(6500 / (c.n * S.FRAME_MS)));
      if (!(await a.play(loop, { loops })) || !alive()) return;
      if (!(await a.play(exit)) || !alive()) return;
      room.scene = null;
      room.dirty = true;
    }));
    actBox.appendChild(b);
  }

  // emotes
  const emoBox = $('[data-emotes]', sec);
  for (const name of EMOTES) {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'chip';
    b.textContent = name.replace('_', ' ');
    b.addEventListener('click', () => run(async (alive) => {
      say(`buddy mascot emote ${name}`);
      await S.preload([name]);
      if (!alive()) return;
      if (!(await stopActivity(alive))) return;
      const c = S.clip(name);
      if (!(await a.turnTo(S.nearestFront(a.dir, c.rows))) || !alive()) return;
      await a.play(name, { loops: c.loop ? 2 : 1 });
    }));
    emoBox.appendChild(b);
  }

  // turning: drag across the stage, or the arrow buttons
  const turn = (kind) => run(async (alive) => {
    if (!(await stopActivity(alive))) return;
    const to = M.turn[kind][a.dir];
    if (await a.turnTo(to)) dirOut.textContent = to;
  });
  $('[data-turn="left"]', sec).addEventListener('click', () => turn('cw'));
  $('[data-turn="right"]', sec).addEventListener('click', () => turn('ccw'));
  let drag = null;
  const canvas = room.canvas;
  canvas.addEventListener('pointerdown', (e) => {
    drag = { x: e.clientX, id: e.pointerId };
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener('pointermove', (e) => {
    if (!drag || drag.id !== e.pointerId) return;
    const dx = e.clientX - drag.x;
    if (Math.abs(dx) > 46) {
      drag.x = e.clientX;
      turn(dx > 0 ? 'ccw' : 'cw');
    }
  });
  const end = () => { drag = null; };
  canvas.addEventListener('pointerup', end);
  canvas.addEventListener('pointercancel', end);

  animateWhileVisible(frame, (dt) => room.tick(dt));
  S.preload(EMOTES).catch(() => {});
}

/* ---------- studio: an auto-rotating turntable ---------- */

async function initStudio() {
  const sec = $('#studio');
  if (!sec) return;
  const canvas = $('.turntable canvas', sec);
  const label = $('[data-clip]', sec);
  const ctx = S.pixelCanvas(canvas, 120, 132);
  const a = new S.Actor({ dir: 'S' });
  await S.preload(['idle', 'walk', 'turn_ccw', 'wave']);
  a.pose('idle', 0, 'S');
  let running = false;
  const loop = async () => {
    running = true;
    for (;;) {
      for (const clip of ['idle', 'walk']) {
        const c = S.clip(clip);
        label.textContent = `${clip} · ${a.dir} · ${c.n} frames`;
        if (!(await a.play(clip, { loops: clip === 'idle' ? 1 : 2 }))) { running = false; return; }
      }
      const from = a.dir;
      label.textContent = `turn · ${from} → ${S.manifest().turn.ccw[from]}`;
      if (!(await a.play('turn_ccw', { row: from }))) { running = false; return; }
      a.dir = S.manifest().turn.ccw[from];
    }
  };
  let dirty = true;
  animateWhileVisible(canvas, (dt) => {
    if (!running) loop();
    if (a.step(dt) || dirty) {
      dirty = false;
      ctx.clearRect(0, 0, 120, 132);
      a.draw(ctx, 60, 118);
    }
  });
}

/* ---------- the activities strip: CSS sprite loops ---------- */

const STRIP = [
  ['tea_sip', 'tea'], ['knit', 'knit'], ['laptop_type', 'laptop'], ['ball_bounce', 'ball'], ['umbrella_rain', 'umbrella'],
  ['snack_munch', 'snack'], ['bubbles_blow', 'bubbles'], ['note_doodle', 'notebook'], ['fish_wait', 'fishing'],
  ['balloon_bob', 'balloon'], ['phones_vibe', 'headphones'], ['plant_water', 'plant'], ['yoyo', 'yoyo'],
  ['snow_build', 'snowman'], ['blocks_build', 'blocks'], ['sweep', 'sweep'], ['read', 'read'], ['desk_type', 'computer'],
  ['dance', 'dance'], ['cap_sleep', 'nightcap'],
];

function initStrip() {
  const track = $('.strip-track');
  if (!track) return;
  const M = S.manifest();
  const z = 2;
  const make = () => {
    const frag = document.createDocumentFragment();
    for (const [name, act] of STRIP) {
      const c = M.clips[name];
      if (!c) continue;
      const [x0, y0, x1, y1] = c.bbox;
      const fig = document.createElement('figure');
      fig.className = 'act';
      fig.style.setProperty('--bw', `${(x1 - x0) * z}px`);
      const spr = document.createElement('div');
      spr.className = 'spr';
      spr.dataset.src = S.assetUrl(c.file);
      spr.style.cssText = `--w:${c.w * z}px;--h:${c.h * z}px;--n:${c.n};--sheet:${c.w * c.n * z}px;` +
        `left:calc(50% - ${((x0 + x1) / 2) * z}px);top:calc(var(--base) - ${y1 * z}px)`;
      const cap = document.createElement('figcaption');
      cap.innerHTML = `<b>${act}</b><code>do ${act}</code>`;
      fig.append(spr, cap);
      frag.appendChild(fig);
    }
    return frag;
  };
  track.appendChild(make());
  const copy = make();
  for (const f of copy.children) f.setAttribute('aria-hidden', 'true');
  track.appendChild(copy);
  onceNear(track, () => {
    for (const s of $$('.spr', track)) s.style.backgroundImage = `url("${s.dataset.src}")`;
  }, '400px');
  S.whenVisible(track, () => track.classList.add('is-on'), () => track.classList.remove('is-on'));
}

/* ---------- numbers that count up once ---------- */

function initCounts() {
  for (const el of $$('[data-count]')) {
    const to = Number(el.dataset.count);
    const fmt = new Intl.NumberFormat();
    if (S.reduceMotion.matches) { el.textContent = fmt.format(to); continue; }
    el.textContent = '0';
    onceNear(el, () => {
      const t0 = performance.now();
      const stop = S.onTick((dt, now) => {
        const p = Math.min(1, (now - t0) / 1400);
        el.textContent = fmt.format(Math.round(to * (1 - (1 - p) ** 3)));
        if (p >= 1) stop();
      });
    }, '-10% 0px');
  }
}

/* ---------- the night scene at the end, which wakes up when you join ---------- */

let nightWake = null;

async function initNight() {
  const sec = $('#join');
  if (!sec) return;
  const frame = $('.night-stage', sec);
  const room = new RoomCanvas(frame, { max: 3 });
  const bubble = new StageBubble($('.bubble', frame), room);
  const floor = S.floorById('parquet');
  const nook = S.sceneById('nook');
  await Promise.all([
    S.preload(['cap_sleep', 'cap_wake_up', 'idle', 'wave', 'happy', 'clap', 'cap_get_in_bed']),
    room.setup([layerOf(floor), ...sceneLayers(['nook'])]),
  ]);
  room.floor = floor;
  room.scene = nook;
  room.hour = 23;
  room.actor.play('cap_sleep');
  let woke = false;
  nightWake = async () => {
    if (woke) return;
    woke = true;
    const a = room.actor;
    await a.play('cap_wake_up');
    room.scene = nook;
    a.dir = 'SE';
    bubble.text('Yay! See you at launch.');
    await a.play('wave', { row: 'SE' });
    await a.play('happy', { row: 'SE' });
    await a.play('clap', { row: 'SE' });
    bubble.hide();
    await a.play('idle', { row: 'SE', loops: 3 });
    await a.play('cap_get_in_bed');
    woke = false;
    a.play('cap_sleep');
  };
  animateWhileVisible(frame, (dt) => { room.tick(dt); bubble.place(); });
}

/* ---------- waitlist forms ---------- */

function initForms(hero) {
  const meta = document.querySelector('meta[name="waitlist-endpoint"]');
  const endpoint = meta ? meta.content.trim() : '';
  for (const form of $$('form[data-waitlist]')) {
    const msg = $('.form-msg', form);
    const input = $('input[type="email"]', form);
    const btn = $('button[type="submit"]', form);
    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      const email = input.value.trim();
      if (!input.checkValidity() || !email) {
        msg.textContent = 'That email doesn’t look right. Try again?';
        msg.dataset.state = 'error';
        input.focus();
        return;
      }
      btn.disabled = true;
      msg.dataset.state = 'busy';
      msg.textContent = 'Sending…';
      let ok = false;
      let text = '';
      if (!endpoint) {
        ok = true;
        text = 'Preview only: nothing was sent. On the live site this puts you on the list.';
      } else {
        try {
          const res = await fetch(endpoint, {
            method: 'POST',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ email, company: $('input[name="company"]', form).value, source: form.dataset.waitlist }),
          });
          const body = await res.json().catch(() => ({}));
          ok = res.ok && body.ok;
          text = ok ? (body.already ? 'You were already on the list. The bear remembers you.' : 'You’re on the list! The bear says thanks.')
            : body.error || 'Something went wrong on our side. Please try again in a minute.';
        } catch {
          text = 'We couldn’t reach the server. Check your connection and try again.';
        }
      }
      btn.disabled = false;
      msg.dataset.state = ok ? 'ok' : 'error';
      msg.textContent = text;
      if (ok) {
        form.classList.add('is-done');
        input.value = '';
        if (form.dataset.waitlist === 'final' && nightWake) nightWake();
        else if (hero) hero.cheer();
      }
    });
  }
}

/* ---------- FAQ: one open at a time ---------- */

function initFaq() {
  const items = $$('.faq details');
  for (const d of items) d.addEventListener('toggle', () => {
    if (d.open) for (const o of items) if (o !== d) o.open = false;
  });
}

export function initSections(hero) {
  initForms(hero);
  initFaq();
  initCounts();
  initStrip();
  const lazy = [['#day', initDay], ['#talk', initTalk], ['#senses', initSenses], ['#rooms', initPlayground],
    ['#studio', initStudio], ['#join', initNight]];
  for (const [sel, fn] of lazy) {
    const el = $(sel);
    if (el) onceNear(el, () => fn().catch((e) => console.error(sel, e)), '800px');
  }
}
