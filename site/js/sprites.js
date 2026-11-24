// 2poke sprite engine. Plays the bear's pre-rendered clips (assets/bear.json, written by
// tools/export_assets.py from the Blender pipeline) on small canvases. Nothing here draws
// the bear: it only copies frames out of the exported sheets, the way the app does
// (crates/mascot-render): a set clip is placed at feet - handoff, face patches go on top.

export const FPS = 12;
export const FRAME_MS = 1000 / FPS;
export const DIRS8 = ['S', 'SE', 'E', 'NE', 'N', 'NW', 'W', 'SW'];
export const FRONT = ['S', 'SE', 'SW'];
export const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)');

let M = null;
let base = '';
const pending = new Map(); // file -> Promise<HTMLImageElement>
const images = new Map(); // file -> HTMLImageElement, once decoded

export async function load(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`manifest ${res.status}`);
  M = await res.json();
  base = url.replace(/[^/]*$/, '');
  return M;
}

export const manifest = () => M;
export const clip = (name) => M && M.clips[name];
export const assetUrl = (file) => base + file;

export function image(file) {
  let p = pending.get(file);
  if (!p) {
    p = new Promise((ok, fail) => {
      const im = new Image();
      im.decoding = 'async';
      im.onload = () => { images.set(file, im); ok(im); };
      im.onerror = () => fail(new Error(`image ${file}`));
      im.src = base + file;
    });
    pending.set(file, p);
  }
  return p;
}

/** Fetch every sheet the named clips need (their set props and face layers too). */
export function preload(names) {
  const files = new Set();
  for (const n of names) {
    const c = M.clips[n];
    if (!c) throw new Error(`no clip ${n}`);
    files.add(c.file);
    if (c.set && M.sets[c.set] && M.sets[c.set].props) files.add(M.sets[c.set].props);
    if (c.face) for (const layer of Object.values(M.face.layers)) for (const d of Object.values(layer)) files.add(d.file);
  }
  return Promise.all([...files].map(image));
}

export const loaded = (file) => images.get(file);

/* ---------- one requestAnimationFrame for the whole page ---------- */

const tickers = new Set();
let raf = 0;
let last = 0;

function frame(now) {
  raf = 0;
  const dt = Math.min(now - last, 200);
  last = now;
  for (const t of [...tickers]) t(dt, now);
  if (tickers.size && !document.hidden) raf = requestAnimationFrame(frame);
}

function wake() {
  if (!raf && tickers.size && !document.hidden) {
    last = performance.now();
    raf = requestAnimationFrame(frame);
  }
}

document.addEventListener('visibilitychange', wake);

/** Run fn(dt, now) every animation frame until the returned function is called. */
export function onTick(fn) {
  tickers.add(fn);
  wake();
  return () => tickers.delete(fn);
}

/** Call on() while el is near the viewport and off() when it leaves. */
export function whenVisible(el, on, off, margin = '200px') {
  let shown = false;
  const io = new IntersectionObserver((entries) => {
    const v = entries.some((e) => e.isIntersecting);
    if (v !== shown) {
      shown = v;
      (v ? on : off)?.();
    }
  }, { rootMargin: margin });
  io.observe(el);
  return () => io.disconnect();
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export const rand = (a, b) => a + Math.random() * (b - a);
export const pick = (list) => list[Math.floor(Math.random() * list.length)];

/* ---------- directions ---------- */

/** The 8-direction facing whose walk vector points most along (dx, dy). */
export function dirToward(dx, dy) {
  const walk = M.clips.walk.root;
  let best = 'S';
  let score = -Infinity;
  for (const d of DIRS8) {
    const t = walk[d];
    let wx = 0;
    let wy = 0;
    for (const [x, y] of t) { wx += x; wy += y; }
    const s = (wx * dx + wy * dy) / (Math.hypot(wx, wy) * Math.hypot(dx, dy) || 1);
    if (s > score) { score = s; best = d; }
  }
  return best;
}

/** The turn clips that take `from` to `to` the short way round: [[clip, row], …]. */
export function turnPath(from, to) {
  if (from === to) return [];
  const steps = (kind) => {
    const out = [];
    let d = from;
    for (let i = 0; i < 8 && d !== to; i++) { out.push([`turn_${kind}`, d]); d = M.turn[kind][d]; }
    return out;
  };
  const a = steps('ccw');
  const b = steps('cw');
  return a.length <= b.length ? a : b;
}

/** The front-facing direction nearest to d (gestures exist only in S, SE and SW). */
export function nearestFront(d, avail = FRONT) {
  if (avail.includes(d)) return d;
  let best = avail[0];
  let n = 99;
  for (const f of avail) {
    const k = turnPath(d, f).length;
    if (k < n) { n = k; best = f; }
  }
  return best;
}

/* ---------- the actor: one bear playing one clip at a time ---------- */

export class Actor {
  constructor({ dir = 'S' } = {}) {
    this.dir = dir;
    this.c = null;
    this.name = '';
    this.row = dir;
    this.ri = 0;
    this.k = 0;
    this.acc = 0;
    this.loops = 1;
    this.held = false;
    this.done = null;
    this.onMove = null; // (dx, dy) in sprite pixels, from the clip's root motion
    this.talking = false;
    this.eyes = false;
    this.mouth = null;
    this.blinkIn = rand(1500, 4000);
    this.blinkLeft = 0;
    this.mouthLeft = 0;
  }

  /** Play a clip. Resolves true when it ends, false when another play() or stop() cut it. */
  play(name, opts = {}) {
    const c = M.clips[name];
    if (!c) return Promise.resolve(false);
    const row = opts.row && c.rows.includes(opts.row) ? opts.row : c.rows.includes(this.dir) ? this.dir : c.rows[0];
    this.finish(false);
    this.c = c;
    this.name = name;
    this.row = row;
    this.ri = c.rows.indexOf(row);
    this.k = opts.from || 0;
    this.acc = 0;
    this.held = false;
    this.loops = opts.loops ?? (c.loop ? Infinity : 1);
    if (!c.set && DIRS8.includes(row) && !name.startsWith('turn_')) this.dir = row;
    return new Promise((res) => { this.done = res; });
  }

  /** Show one frame and stay on it. */
  pose(name, k = 0, row) {
    this.play(name, { row, loops: 1, from: k });
    this.held = true;
    this.finish(false);
  }

  finish(ok) {
    const d = this.done;
    this.done = null;
    if (d) d(ok);
  }

  stop() { this.finish(false); }

  /** Play the turn clips from this.dir to `to`. */
  async turnTo(to) {
    for (const [name, row] of turnPath(this.dir, to)) {
      if (!(await this.play(name, { row }))) return false;
      this.dir = M.turn[name.slice(5)][row];
    }
    return true;
  }

  /** Advance by dt ms; true when the picture changed. */
  step(dt) {
    let changed = false;
    if (this.c && !this.held) {
      this.acc += dt;
      while (this.acc >= FRAME_MS && !this.held) {
        this.acc -= FRAME_MS;
        this.advance();
        changed = true;
      }
    }
    // blinks: a closed-eyes patch for two frames every few seconds (idle and talk only)
    if (this.c && this.c.face) {
      if (this.blinkLeft > 0) {
        this.blinkLeft -= dt;
        if (this.blinkLeft <= 0) { this.eyes = false; changed = true; }
      } else if ((this.blinkIn -= dt) <= 0) {
        this.eyes = true;
        this.blinkLeft = FRAME_MS * 2;
        this.blinkIn = Math.random() < 0.15 ? 260 : rand(2200, 5600);
        changed = true;
      }
      if (this.talking) {
        if ((this.mouthLeft -= dt) <= 0) {
          this.mouth = pick(['mouth_open', 'mouth_wide', 'mouth_open', null]);
          this.mouthLeft = FRAME_MS * (this.mouth ? 2 : 1);
          changed = true;
        }
      } else if (this.mouth) {
        this.mouth = null;
        changed = true;
      }
    }
    return changed;
  }

  advance() {
    const c = this.c;
    const r = c.root && c.root[this.row];
    if (r && this.onMove) {
      const m = r[this.k];
      if (m && (m[0] || m[1])) this.onMove(m[0], m[1]);
    }
    if (this.k + 1 < c.n) {
      this.k++;
      return;
    }
    if (--this.loops > 0) {
      this.k = 0;
      return;
    }
    this.held = true;
    this.finish(true);
  }

  /** The clip's opaque box relative to the standing feet: [x0, y0, x1, y1] in sprite pixels. */
  box() {
    const c = this.c;
    if (!c) return [-30, -100, 30, 10];
    const [hx, hy] = this.handoff();
    const [x0, y0, x1, y1] = c.bbox;
    return [x0 - c.ax - hx, y0 - c.ay - hy, x1 - c.ax - hx, y1 - c.ay - hy];
  }

  handoff() {
    const c = this.c;
    if (!c || !c.set) return [0, 0];
    return (M.sets[c.set] && M.sets[c.set].handoff) || [0, 0];
  }

  /** Draw at (fx, fy), the standing bear's feet on ctx. */
  draw(ctx, fx, fy) {
    const c = this.c;
    if (!c) return;
    const im = images.get(c.file);
    if (!im) return;
    const [hx, hy] = this.handoff();
    const ox = Math.round(fx - hx - c.ax);
    const oy = Math.round(fy - hy - c.ay);
    ctx.drawImage(im, this.k * c.w, this.ri * c.h, c.w, c.h, ox, oy, c.w, c.h);
    if (c.face) {
      const f = M.face.clips[c.face] && M.face.clips[c.face][this.row];
      if (!f) return;
      if (this.eyes) this.patch(ctx, 'eyes_closed', f.eyes[this.k], ox, oy);
      if (this.mouth) this.patch(ctx, this.mouth, f.mouth[this.k], ox, oy);
    }
  }

  patch(ctx, layer, at, ox, oy) {
    if (!at) return;
    const l = M.face.layers[layer] && M.face.layers[layer][this.row];
    const im = l && images.get(l.file);
    if (im) ctx.drawImage(im, ox + at[0], oy + at[1]);
  }
}

/* ---------- rooms: floors and scenes, composed in the app's order ---------- */

export function sceneVariant(scene, hour) {
  const timed = scene.variants.filter((v) => v.hours);
  for (const v of timed) {
    const [a, b] = v.hours;
    if (a < b ? hour >= a && hour < b : hour >= a || hour < b) return v;
  }
  return scene.variants[0];
}

export const floorById = (id) => M.room.floors.find((f) => f.id === id);
export const sceneById = (id) => M.room.scenes.find((s) => s.id === id);

/** The canvas box that holds every listed layer [{file, origin}] around one anchor. */
export async function roomBounds(layers, pad = [8, 8, 8, 8]) {
  let x0 = -70; let y0 = -130; let x1 = 70; let y1 = 30;
  for (const l of layers) {
    const im = await image(l.file);
    x0 = Math.min(x0, -l.origin[0]);
    y0 = Math.min(y0, -l.origin[1]);
    x1 = Math.max(x1, im.naturalWidth - l.origin[0]);
    y1 = Math.max(y1, im.naturalHeight - l.origin[1]);
  }
  x0 -= pad[3]; y0 -= pad[0]; x1 += pad[1]; y1 += pad[2];
  return { w: x1 - x0, h: y1 - y0, ax: -x0, ay: -y0 };
}

/** Draw one composed room frame: floor, scene back, set props or the bear, scene front. */
export function drawRoom(ctx, ax, ay, { floor, scene, hour = new Date().getHours(), actor, props }) {
  if (floor) {
    const im = images.get(floor.file);
    if (im) ctx.drawImage(im, ax - floor.origin[0], ay - floor.origin[1]);
  }
  let v = null;
  if (scene) {
    v = sceneVariant(scene, hour);
    const im = images.get(v.back);
    if (im) ctx.drawImage(im, ax - scene.origin[0], ay - scene.origin[1]);
  }
  if (props) {
    const s = M.sets[props];
    const im = s && images.get(s.props);
    if (im) ctx.drawImage(im, ax - s.handoff[0] - s.ax, ay - s.handoff[1] - s.ay);
  }
  if (actor) actor.draw(ctx, ax, ay);
  if (v && v.front) {
    const im = images.get(v.front);
    if (im) ctx.drawImage(im, ax - scene.origin[0], ay - scene.origin[1]);
  }
}

/** Every file a room needs (floor, every scene variant). */
export function roomFiles({ floor, scene }) {
  const out = [];
  if (floor) out.push(floor.file);
  if (scene) for (const v of scene.variants) { out.push(v.back); if (v.front) out.push(v.front); }
  return out;
}

/** A pixel canvas: logical size w x h, shown at an integer zoom (or fitted below 2x). */
export function pixelCanvas(canvas, w, h) {
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext('2d', { willReadFrequently: true });
  ctx.imageSmoothingEnabled = false;
  return ctx;
}

/** Fit a logical width into a box: whole-pixel zoom when it can be 2 or more. */
export function fitZoom(logical, avail, max = 3) {
  const z = avail / logical;
  if (z >= 2) return Math.min(max, Math.floor(z));
  return Math.min(z, max);
}
