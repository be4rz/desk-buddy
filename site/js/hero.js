// The hero bear: lives on the mock desktop, sits on window edges, reacts to pokes, drags and
// the cursor the way the app's reactions do (mascots/bear/pack.json: flinch, pet, follow, dizzy,
// drag_hold, drag_land, edge_land), and talks through a speech bubble.

import * as S from './sprites.js';

const CW = 140; // logical canvas, anchor at the feet
const CH = 156;
const AX = 70;
const AY = 114;
const PERCH_H = 101; // the tallest perched pose (edge_sit / edge_stand) reaches this far above its seat

const CORE = ['idle', 'edge_sit', 'edge_dangle', 'edge_peer', 'edge_stand', 'dangle', 'land_squash', 'turn_cw', 'turn_ccw'];
const MORE = ['walk_start', 'walk', 'walk_stop', 'edge_doze', 'dizzy', 'giggle', 'flinch', 'surprised', 'laugh',
  'shy_hide', 'wave', 'jump', 'proud', 'clap', 'talk', 'cursor_watch', 'head_bob', 'happy', 'greet', 'hop', 'spin',
  'fid_weight_shift', 'fid_ear_twitch', 'fid_look_up', 'fid_tummy_rub', 'fid_wave_small', 'fid_peek',
  'fid_rock_heels', 'fid_head_tilt', 'fid_scratch', 'fid_yawn', 'fid_shake_off', 'fid_sneeze',
  'windmill', 'hiccups', 'peekaboo', 'find_coin', 'butterfly_ear', 'moonwalk', 'hat_spring', 'hat_summer',
  'hat_autumn', 'hat_winter', 'shrug', 'thumbs_up', 'nod'];
const FIDGETS = ['fid_weight_shift', 'fid_ear_twitch', 'fid_look_up', 'fid_tummy_rub', 'fid_wave_small', 'fid_peek',
  'fid_rock_heels', 'fid_head_tilt', 'fid_scratch', 'fid_yawn', 'fid_shake_off', 'fid_sneeze'];
const RARES = ['windmill', 'hiccups', 'peekaboo', 'find_coin', 'butterfly_ear'];
const POKES = [
  ['giggle', 'Hehe. That tickles.'],
  ['surprised', 'Oh! Hi there.'],
  ['wave', 'Hello, you.'],
  ['laugh', 'Okay, okay, I’m awake.'],
  ['jump', 'Boing!'],
  ['shy_hide', ''],
  ['proud', 'I’m a very good bear.'],
  ['spin', 'Wheee.'],
];

function seasonHat(d = new Date()) {
  const m = d.getMonth();
  return m >= 2 && m <= 4 ? 'hat_spring' : m >= 5 && m <= 7 ? 'hat_summer' : m >= 8 && m <= 10 ? 'hat_autumn' : 'hat_winter';
}

function store(key, value) {
  try {
    if (value === undefined) return localStorage.getItem(key);
    localStorage.setItem(key, value);
  } catch { /* private mode: the count just starts at 0 */ }
  return null;
}

export class HeroBear {
  constructor(desk) {
    this.desk = desk;
    this.canvas = desk.querySelector('.bear-canvas');
    this.hit = desk.querySelector('.bear-hit');
    this.bubble = desk.querySelector('.bubble');
    this.bubbleText = this.bubble.querySelector('.bubble-text');
    this.live = document.getElementById('hero-live');
    this.counter = document.querySelector('[data-pokes]');
    this.ctx = S.pixelCanvas(this.canvas, CW, CH);
    this.a = new S.Actor({ dir: 'S' });
    this.a.onMove = (dx, dy) => this.moveTo(this.x + dx * this.z, this.y + dy * this.z);
    this.z = 2;
    this.x = 0;
    this.y = 0;
    this.mode = 'perch';
    this.perch = null;
    this.perchAt = 0.62;
    this.rel = [0.5, 0.8];
    this.gen = 0;
    this.busy = false;
    this.nextIdle = 0;
    this.lastTurn = 0;
    this.cool = { flinch: 0, pet: 0, watch: 0 };
    this.pokes = Number(store('2poke.pokes')) || 0;
    this.pokeTimes = [];
    this.pointer = { x: -1e4, y: -1e4, t: 0, vx: 0, still: 0, flips: [], lastDx: 0, inside: false };
    this.hidden = false;
    this.dirty = true;
    this.full = false;
  }

  async start() {
    await S.preload(CORE);
    this.layout();
    this.perch = this.desk.querySelector('.win-np');
    this.placeOnPerch();
    this.a.play('edge_dangle');
    this.bind();
    this.stopTick = null;
    S.whenVisible(this.desk, () => { this.stopTick = S.onTick((dt, now) => this.tick(dt, now)); },
      () => { if (this.stopTick) this.stopTick(); this.stopTick = null; }, '0px');
    new ResizeObserver(() => this.layout()).observe(this.desk);
    this.updateCount();
    S.preload(MORE).then(() => { this.full = true; }).catch(() => {});
    setTimeout(() => this.say('Psst. You can poke me.', { hold: 4200 }), 1300);
    setTimeout(() => {
      if (this.pokes === 0 && this.mode === 'perch' && !this.busy) this.say('Or pick me up and put me somewhere.', { hold: 4200 });
    }, 14000);
  }

  /* ---------- geometry ---------- */

  layout() {
    const r = this.desk.getBoundingClientRect();
    this.W = r.width;
    this.H = r.height;
    this.z = r.width >= 980 ? 3 : 2;
    this.menuH = this.desk.querySelector('.menubar')?.offsetHeight ?? 32;
    this.canvas.style.width = `${CW * this.z}px`;
    this.canvas.style.height = `${CH * this.z}px`;
    if (this.mode === 'perch' && this.perch) this.placeOnPerch();
    else if (this.mode !== 'drag') this.moveTo(this.rel[0] * this.W, this.rel[1] * this.H);
    this.dirty = true;
  }

  perchRect(el) {
    const d = this.desk.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    return { left: r.left - d.left, right: r.right - d.left, top: r.top - d.top };
  }

  placeOnPerch() {
    const p = this.perchRect(this.perch);
    this.x = p.left + (p.right - p.left) * this.perchAt;
    this.y = p.top + 2 * this.z; // the edge clips sit their seat on the anchor; the window's border is 2px
    this.place();
  }

  moveTo(x, y) {
    const m = 34 * this.z;
    this.x = Math.max(m, Math.min(this.W - m, x));
    this.y = Math.max(104 * this.z + 30, Math.min(this.H - 8, y));
    if (this.W) this.rel = [this.x / this.W, this.y / this.H];
    this.place();
  }

  place() {
    this.canvas.style.transform = `translate3d(${Math.round(this.x - AX * this.z)}px, ${Math.round(this.y - AY * this.z)}px, 0)`;
    this.dirty = true;
  }

  /** The bear's opaque box in desk pixels. */
  box() {
    const [x0, y0, x1, y1] = this.a.box();
    return { x0: this.x + x0 * this.z, y0: this.y + y0 * this.z, x1: this.x + x1 * this.z, y1: this.y + y1 * this.z };
  }

  /** Is the bear's pixel under desk point (px, py) opaque? */
  opaqueAt(px, py) {
    const cx = Math.floor((px - (this.x - AX * this.z)) / this.z);
    const cy = Math.floor((py - (this.y - AY * this.z)) / this.z);
    if (cx < 0 || cy < 0 || cx >= CW || cy >= CH) return false;
    // forgive a near miss: a 3x3 neighbourhood counts
    const d = this.ctx.getImageData(Math.max(0, cx - 1), Math.max(0, cy - 1), 3, 3).data;
    for (let i = 3; i < d.length; i += 4) if (d[i] > 0) return true;
    return false;
  }

  /* ---------- behaviours ---------- */

  /** Run one behaviour; a newer one cuts it. When it ends the bear rests. */
  async run(fn) {
    const g = ++this.gen;
    this.busy = true;
    const alive = () => g === this.gen;
    try {
      await fn(alive);
    } catch (e) {
      console.error(e);
    }
    if (alive()) this.rest();
  }

  rest() {
    this.busy = false;
    this.a.talking = false;
    if (this.mode === 'perch') this.a.play('edge_dangle');
    else this.a.play('idle');
    this.nextIdle = performance.now() + (S.reduceMotion.matches ? S.rand(14000, 24000) : S.rand(6000, 12000));
  }

  async front(alive, d) {
    const want = d || S.nearestFront(this.a.dir);
    return (await this.a.turnTo(want)) && alive();
  }

  idleAct() {
    if (!this.full) return;
    if (this.mode === 'perch') {
      this.run(async (alive) => {
        if (Math.random() < 0.6) await this.a.play('edge_peer');
        else await this.a.play('edge_doze', { loops: 2 });
      });
      return;
    }
    const r = Math.random();
    if (r < 0.28 && !S.reduceMotion.matches) this.run((alive) => this.wander(alive));
    else if (r < 0.4) {
      this.run(async (alive) => {
        if (!(await this.front(alive, 'S'))) return;
        await this.a.play(S.pick(RARES));
      });
    } else if (r < 0.45) {
      this.run(async (alive) => {
        if (!(await this.front(alive, 'S'))) return;
        await this.a.play(seasonHat());
      });
    } else {
      this.run(async (alive) => {
        if (!(await this.front(alive))) return;
        await this.a.play(S.pick(FIDGETS));
      });
    }
  }

  async wander(alive) {
    const m = 60 * this.z;
    const tx = S.rand(m, this.W - m);
    const ty = S.rand(this.H * 0.55, this.H - 20);
    const dx = tx - this.x;
    const dy = ty - this.y;
    const dist = Math.hypot(dx, dy);
    if (dist < 40 * this.z) return;
    const dir = S.dirToward(dx, dy);
    if (!(await this.a.turnTo(dir)) || !alive()) return;
    if (!(await this.a.play('walk_start', { row: dir })) || !alive()) return;
    const per = 24 * Math.SQRT2 * this.z; // a walk cycle covers ~34 px on screen at 1x
    const loops = Math.max(1, Math.min(4, Math.round(dist / per)));
    if (!(await this.a.play('walk', { row: dir, loops })) || !alive()) return;
    if (!(await this.a.play('walk_stop', { row: dir })) || !alive()) return;
    await this.front(alive);
  }

  poke() {
    if (this.hidden || this.mode === 'drag') return;
    const now = performance.now();
    this.pokes++;
    store('2poke.pokes', String(this.pokes));
    this.updateCount();
    this.pokeTimes = this.pokeTimes.filter((t) => now - t < 3000);
    this.pokeTimes.push(now);
    const fast = this.pokeTimes.length >= 5;
    if (fast) this.pokeTimes = [];
    if (this.mode === 'perch') {
      this.run(async (alive) => {
        if (this.pokes % 3 !== 0 && !fast) {
          this.say(S.pick(['Hm?', 'Comfy up here.', 'Nice view.']), { hold: 1800 });
          await this.a.play('edge_peer');
          return;
        }
        this.say('Fine, I’m up.', { hold: 1800 });
        if (!(await this.a.play('edge_stand')) || !alive()) return;
        this.mode = 'stand';
        this.perch = null;
        this.rel = [this.x / this.W, this.y / this.H];
        await this.a.play('wave');
      });
      return;
    }
    this.run(async (alive) => {
      if (!(await this.front(alive))) return;
      if (fast) {
        this.say('Whoa. The room is spinning.', { hold: 2600 });
        await this.a.play('dizzy');
        return;
      }
      if (this.pokes === 10) {
        this.say('Ten pokes. We’re friends now.', { hold: 3000 });
        await this.a.play('clap');
        return;
      }
      if (this.pokes === 25 && (await this.front(alive, 'S'))) {
        this.say('A hat, just for you.', { hold: 3000 });
        await this.a.play(seasonHat());
        return;
      }
      if (this.pokes === 50 && (await this.front(alive, 'SE'))) {
        this.say('Fifty! You found the moonwalk.', { hold: 3200 });
        await this.a.play('moonwalk', { loops: 2 });
        return;
      }
      const [clip, line] = POKES[(this.pokes - 1) % POKES.length];
      if (line) this.say(line, { hold: 2000 });
      await this.a.play(clip);
    });
  }

  updateCount() {
    if (!this.counter) return;
    this.counter.textContent = this.pokes === 1 ? '1 poke' : `${this.pokes.toLocaleString()} pokes`;
  }

  /** The music window started playing. */
  music(on) {
    if (!on || this.hidden) return;
    if (this.mode === 'stand' && this.full) {
      this.say('Ooh, I like this one.', { hold: 2400 });
      this.run(async (alive) => {
        if (!(await this.front(alive))) return;
        await this.a.play('head_bob', { loops: 4 });
      });
    } else if (this.mode === 'perch') {
      this.say('♪ Good one.', { hold: 2000 });
    }
  }

  /** Someone joined the waitlist from the hero form. */
  cheer() {
    if (this.hidden) return;
    this.say('Yay! You’re on the list.', { hold: 3200 });
    if (this.mode === 'perch') {
      this.run(async () => { await this.a.play('edge_peer'); });
      return;
    }
    if (!this.full) return;
    this.run(async (alive) => {
      if (!(await this.front(alive))) return;
      if (await this.a.play('happy')) await this.a.play('clap');
    });
  }

  setHidden(h) {
    this.hidden = h;
    this.canvas.hidden = h;
    this.hit.hidden = h;
    if (h) this.hideBubble();
    else if (this.mode === 'stand') this.run(async () => { await this.a.play('wave'); });
  }

  /* ---------- talk ---------- */

  say(text, { hold = 2600, talk = false } = {}) {
    if (this.hidden || !text) return;
    clearTimeout(this.sayTimer);
    clearInterval(this.typeTimer);
    this.live.textContent = text;
    this.bubble.hidden = false;
    this.bubble.classList.remove('is-out');
    this.bubbleText.textContent = '';
    let i = 0;
    const chars = [...text];
    const fast = S.reduceMotion.matches;
    if (talk && this.mode === 'stand' && !this.busy) this.run(async (alive) => {
      if (!(await this.front(alive))) return;
      this.a.talking = true;
      await this.a.play('talk', { loops: Math.max(1, Math.ceil(chars.length / 22)) });
      this.a.talking = false;
    });
    const tick = () => {
      i = fast ? chars.length : i + 1;
      this.bubbleText.textContent = chars.slice(0, i).join('');
      this.placeBubble();
      if (i >= chars.length) {
        clearInterval(this.typeTimer);
        this.sayTimer = setTimeout(() => this.hideBubble(), hold);
      }
    };
    tick();
    this.typeTimer = setInterval(tick, 34);
  }

  hideBubble() {
    clearTimeout(this.sayTimer);
    clearInterval(this.typeTimer);
    if (this.bubble.hidden) return;
    this.bubble.classList.add('is-out');
    this.sayTimer = setTimeout(() => { this.bubble.hidden = true; }, 180);
  }

  placeBubble() {
    if (this.bubble.hidden) return;
    const b = this.box();
    const bw = this.bubble.offsetWidth;
    const bh = this.bubble.offsetHeight;
    const cx = (b.x0 + b.x1) / 2;
    const left = Math.max(8, Math.min(this.W - bw - 8, cx - bw / 2));
    const top = Math.max(38, b.y0 - bh - 14);
    this.bubble.style.transform = `translate3d(${Math.round(left)}px, ${Math.round(top)}px, 0)`;
    this.bubble.style.setProperty('--tail', `${Math.round(Math.max(18, Math.min(bw - 18, cx - left)))}px`);
  }

  /* ---------- input ---------- */

  local(e) {
    const r = this.desk.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  bind() {
    const hit = this.hit;
    hit.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return;
      const [px, py] = this.local(e);
      if (!this.opaqueAt(px, py)) return;
      e.preventDefault();
      hit.setPointerCapture(e.pointerId);
      this.press = { id: e.pointerId, x0: px, y0: py, px, py, t: performance.now(), path: 0, drag: false,
        dy: Math.max(30 * this.z, Math.min(96 * this.z, this.y - py)) };
    });
    hit.addEventListener('pointermove', (e) => {
      const p = this.press;
      if (!p || p.id !== e.pointerId) {
        const [px, py] = this.local(e);
        hit.style.cursor = this.opaqueAt(px, py) ? 'grab' : 'default';
        return;
      }
      const [px, py] = this.local(e);
      p.path += Math.hypot(px - p.px, py - p.py);
      p.px = px;
      p.py = py;
      if (!p.drag && Math.hypot(px - p.x0, py - p.y0) > 6) this.startDrag();
    });
    const up = (e) => {
      const p = this.press;
      if (!p || p.id !== e.pointerId) return;
      this.press = null;
      if (p.drag) this.endDrag(p, e.type === 'pointercancel');
      else if (e.type === 'pointerup') this.poke();
    };
    hit.addEventListener('pointerup', up);
    hit.addEventListener('pointercancel', up);
    hit.addEventListener('lostpointercapture', up);

    this.desk.addEventListener('pointermove', (e) => this.sense(e));
    this.desk.addEventListener('pointerleave', () => { this.pointer.inside = false; });
  }

  startDrag() {
    const p = this.press;
    p.drag = true;
    p.t = performance.now();
    ++this.gen;
    this.busy = true;
    this.mode = 'drag';
    this.perch = null;
    this.hideBubble();
    this.desk.classList.add('is-dragging');
    this.hit.style.cursor = 'grabbing';
    this.a.play('dangle');
  }

  dropTarget() {
    for (const el of this.desk.querySelectorAll('[data-perch]')) {
      const r = this.perchRect(el);
      if (r.top - PERCH_H * this.z < this.menuH) continue; // a seated bear there would hide under the menu bar
      if (Math.abs(this.y - r.top) < 30 && this.x > r.left + 22 && this.x < r.right - 22) return el;
    }
    return null;
  }

  endDrag(p, cancelled) {
    this.desk.classList.remove('is-dragging');
    for (const el of this.desk.querySelectorAll('.is-target')) el.classList.remove('is-target');
    this.hit.style.cursor = '';
    const target = cancelled ? null : this.dropTarget();
    const dizzy = performance.now() - p.t >= 1500 && p.path >= 600;
    if (target) {
      this.mode = 'perch';
      this.perch = target;
      const r = this.perchRect(target);
      this.perchAt = (this.x - r.left) / (r.right - r.left);
      this.placeOnPerch();
      this.run(async (alive) => {
        if (!(await this.a.play('edge_sit')) || !alive()) return;
        if (dizzy) this.say('Wobbly… but I made it.', { hold: 2200 });
        else this.say(S.pick(['Ooh, a good spot.', 'Comfy.', 'I can see everything from here.']), { hold: 2200 });
      });
      return;
    }
    this.mode = 'stand';
    this.a.dir = 'S';
    this.moveTo(this.x, this.y);
    this.run(async (alive) => {
      if (!(await this.a.play('land_squash')) || !alive()) return;
      if (dizzy && this.full) {
        this.say('Whoa. Everything’s spinning.', { hold: 2600 });
        await this.a.play('dizzy');
      } else if (Math.random() < 0.5) {
        this.say(S.pick(['Whee.', 'Again!', 'Nice landing, right?']), { hold: 1800 });
      }
    });
  }

  /** The cursor senses: watch, flinch and pet (pack.json reactions), from pointer moves only. */
  sense(e) {
    if (e.pointerType === 'touch' || this.hidden) return;
    const [px, py] = this.local(e);
    const now = performance.now();
    const q = this.pointer;
    const dt = Math.max(1, now - q.t);
    const speed = Math.hypot(px - q.x, py - q.y) / dt * 1000;
    const dx = px - q.x;
    if (q.t && Math.sign(dx) && Math.sign(dx) !== Math.sign(q.lastDx) && Math.abs(dx) > 1) {
      if (this.opaqueAt(px, py)) q.flips.push(now);
    }
    if (Math.abs(dx) > 1) q.lastDx = dx;
    q.flips = q.flips.filter((t) => now - t < 2000);
    q.x = px; q.y = py; q.t = now; q.still = now; q.inside = true;
    if (this.busy || this.mode !== 'stand' || !this.full || this.press) return;
    const b = this.box();
    const cx = (b.x0 + b.x1) / 2;
    const cy = (b.y0 + b.y1) / 2;
    const near = Math.hypot(px - cx, py - cy);
    if (q.flips.length >= 3 && now > this.cool.pet) {
      q.flips = [];
      this.cool.pet = now + 10000;
      this.say('Hehe.', { hold: 1400 });
      this.run(async (alive) => { if (await this.front(alive)) await this.a.play('giggle'); });
      return;
    }
    if (speed > 1500 && near < 70 * this.z && now > this.cool.flinch) {
      this.cool.flinch = now + 20000;
      this.say('Eep!', { hold: 1200 });
      this.run(async (alive) => { if (await this.front(alive)) await this.a.play('flinch'); });
      return;
    }
    // look toward the cursor (front directions only), not more than every 0.9 s
    if (this.a.name === 'idle' && now - this.lastTurn > 900) {
      const off = px - cx;
      const want = off < -110 ? 'SW' : off > 110 ? 'SE' : 'S';
      if (want !== this.a.dir) {
        this.lastTurn = now;
        this.run(async (alive) => { await this.front(alive, want); });
      }
    }
  }

  /* ---------- frame ---------- */

  tick(dt, now) {
    if (this.mode === 'drag' && this.press) {
      const k = 1 - Math.exp(-dt / 55);
      const tx = this.press.px;
      const ty = this.press.py + this.press.dy;
      this.x += (tx - this.x) * k;
      this.y += (ty - this.y) * k;
      this.place();
      const t = this.dropTarget();
      for (const el of this.desk.querySelectorAll('[data-perch]')) el.classList.toggle('is-target', el === t);
    }
    if (this.a.step(dt)) this.dirty = true;
    if (this.dirty) {
      this.dirty = false;
      this.ctx.clearRect(0, 0, CW, CH);
      this.a.draw(this.ctx, AX, AY);
      const b = this.box();
      const pad = 6;
      this.hit.style.transform = `translate3d(${Math.round(b.x0 - pad)}px, ${Math.round(b.y0 - pad)}px, 0)`;
      this.hit.style.width = `${Math.round(b.x1 - b.x0 + pad * 2)}px`;
      this.hit.style.height = `${Math.round(b.y1 - b.y0 + pad * 2)}px`;
      this.placeBubble();
    }
    const q = this.pointer;
    if (!this.busy && this.mode === 'stand' && this.full && q.inside && now - q.still > 1500 && now > this.cool.watch) {
      const b = this.box();
      if (Math.hypot(q.x - (b.x0 + b.x1) / 2, q.y - (b.y0 + b.y1) / 2) < 120 * this.z / 2 + 60) {
        this.cool.watch = now + 20000;
        this.run(async (alive) => { if (await this.front(alive)) await this.a.play('cursor_watch', { loops: 2 }); });
      }
    }
    if (!this.busy && this.mode !== 'drag' && now > this.nextIdle && this.nextIdle) this.idleAct();
    if (!this.busy && !this.nextIdle) this.nextIdle = now + 8000;
  }
}
