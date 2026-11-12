"""The stage kit (python3 + numpy + Pillow): what every art stage after Stage D shares, so a new stage adds only
stages/<id>.json, clips_<id>.py or set_<id>.py, verify_<id>.py (and optionally palette-ext-<id>.json).

    python3 stagekit.py freeze [--dirs d ...] [--root DIR]      sha256 of every file under the dirs (default: all of
                                                                 out/bear3d) -> stdout, "<sha>  <path>" per line
    python3 stagekit.py check <file.sha> [--own d ...] [--root DIR]   exit 1 if a file listed there changed or vanished,
                                                                 or a file was added, outside the --own dirs
    python3 stagekit.py sheet <id> | preview <id>                (re)write sheet_<id>.png / preview_<id>.html of a
                                                                 rendered stage from its outputs
    python3 stagekit.py manifest <id> [--sh]                     the stage manifest (--sh: shell variables)
    python3 stagekit.py lock acquire --pid P | lock release <slot> --pid P    the 2-slot render lock (run_stage.sh)
    python3 stagekit.py --self-test                              in-memory checks of the helpers; exit 1 = they work

Interfaces (CLAUDE.md "Stage kit"; the design anchors are in brackets):

[#stage-manifest] stages/<id>.json (or $BEAR3D_STAGES/<id>.json):
    {"id": "lean", "kind": "set" | "clips", "module": "set_lean" | "clips_<id>", "verify": "verify_lean.py",
     "palette_ext": "palette-ext-<id>.json" (optional), "note": "..."}
  out dir: set stages -> out/bear3d/set_<id>/, clip stages -> out/bear3d/anim_<id>/ ($BEAR3D_OUT replaces out/bear3d
  as the swap target; inputs such as anim/ idle are always read from the real out/bear3d).

[#anim-stage] clip stage output anim_<id>/anim_<id>.json: Stage A's format ("anims", frames[i].dur and
  frames[i].root_px{dir}) plus per clip "stage", "face" ("layers" | "baked" | null), "handoff", "root_track" (bear-local
  [tx, ty_px] world units per frame + one after the last) and a clip-level "root_px" {dir: [[dx, dy], ...]}.
  Per clip the module may give "frame" [W, H] and "anchor" [W / 2, y] (default Stage A's [84, 123] / [42, 107]; e.g.
  [132, 123] / [66, 107] for floor and tumble clips): run_stage.sh renders at that canvas, make_stage.py writes it as
  frame_w / frame_h / anchor, check_clips verifies it. "handoff" {"start" | "end": "idle0" | "<Stage A clip>#<frame>"}
  (e.g. "run_stop#0"): that end is rebuilt as the Stage A frame (after the HANDOFF_NOISE check) and verified
  pixel-equal, anchors aligned (stage_a_frame). Rig K (anim_clips.check_k): a clip with face "baked" / "none" may use
  lean / roll (|x| <= 90, Stage A's body tilt at the floor, feet planted, the face riding rigidly) and tumble (deg,
  any, + = forward: the whole bear turns as one rigid lump about the body centre, feet included) with lift (world
  units, the whole bear up; keeps the lowest point on the floor); "layers" clips keep all four at 0.
  frames[i].root_px and root_px[dir][i] are ONE track written twice with identical values (Stage A's loader moves the
  sprite by frames[i].root_px): a loader applies one of them, never both. verify_main checks they are equal and equal
  the projection of root_track.

[#face-part] face_<id>.json beside the stage's anim json (verify_face.py reads every out/bear3d/*/face_*.json):
    {"face_part": 1, "stage": id, "anim": "<anim json file name>",
     "clips": {clip: {dir: {"eyes": [[x, y], ...], "mouth": [...]}}},          (clip stages, layers clips)
     "patch_dirs": {clip: {dir: patch_dir}},                                    (clip stages, when the head turns)
     "set_clips": {clip: {ori: {"patch": dir, "eyes": [...], "mouth": [...]}}},   (set stages)
     "exact": {clip: {dir_or_ori: [frame, ...]}}}       frames where drawn patches must equal the body exactly
  A clip may be in face/face.json or in one part, never in two places.

[#verify-stage] verify_<id>.py = stagekit.verify_main(id, extra=None, mutations=()) with --dir DIR and --self-test
  (exit 0 pass / 1 findings; self-test exit 1 = every mutation caught, 2 = one missed): pixel rules, frame counts,
  hand-offs (idle#0 aligned by the anchors), joins, loop seams, root_px, props, face part; the 5 standard mutations
  and the stage's own.

[#sha-freeze] freeze / check above; the committed baseline is stages/freeze_p1.sha.
"""
import sys, os, json, math, hashlib, time, importlib
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
EXP = os.path.abspath(os.path.join(HERE, '..', '..'))
REAL_OUT = os.path.abspath(os.path.join(HERE, '..', 'out', 'bear3d'))
DIRS = ['S', 'SE', 'E', 'NE', 'N', 'NW', 'W', 'SW']
YAW = {d: (45 + 45 * i) % 360 for i, d in enumerate(DIRS)}
YAW2DIR = {v: k for k, v in YAW.items()}
ORIS = ['A', 'B']
MAX_SET_W, MAX_SET_H = 176, 160
HANDOFF_NOISE = 8             # px a fresh rest render may differ from idle#0 before the kit replaces it (Stage B/D)
BG = (200, 196, 210, 255)
FPS = 12; DUR = round(1000 / FPS)
# camera (X60 Z45 ortho), exact: the columns of Euler((60, 0, 45)).to_matrix()
_c, _s = math.cos(math.radians(45)), math.sin(math.radians(45))
_c6, _s6 = math.cos(math.radians(60)), math.sin(math.radians(60))
RIGHT = np.array([_c, _s, 0.0]); UP = np.array([-_s * _c6, _c * _c6, _s6]); BACK = np.array([_s * _s6, -_c * _s6, _c6])


def out_root(): return os.path.abspath(os.environ.get('BEAR3D_OUT') or REAL_OUT)
def stages_dir(): return os.path.abspath(os.environ.get('BEAR3D_STAGES') or os.path.join(HERE, 'stages'))


# ------------------------------------------------------------------ manifest + modules
def manifest(sid):
    p = os.path.join(stages_dir(), f'{sid}.json')
    if not os.path.exists(p) and os.environ.get('BEAR3D_STAGES'): p = os.path.join(HERE, 'stages', f'{sid}.json')
    if not os.path.exists(p): raise SystemExit(f'stagekit: no stage manifest {sid}.json in {stages_dir()}')
    m = json.load(open(p))
    for k in ('id', 'kind', 'module', 'verify'):
        if k not in m: raise SystemExit(f'stagekit: {p}: missing "{k}"')
    if m['id'] != sid: raise SystemExit(f'stagekit: {p}: id {m["id"]!r} != {sid!r}')
    if m['kind'] not in ('set', 'clips'): raise SystemExit(f'stagekit: {p}: kind must be "set" or "clips"')
    m['out'] = ('set_' if m['kind'] == 'set' else 'anim_') + sid
    m['path'] = p
    return m


def module(name):
    for d in [HERE] + ([os.environ['BEAR3D_STAGES']] if os.environ.get('BEAR3D_STAGES') else []):
        if d not in sys.path: sys.path.insert(0, d)
        elif d != HERE: sys.path.remove(d); sys.path.insert(0, d)
    return importlib.import_module(name)


# ------------------------------------------------------------------ palette + pixel rules
def hx(h): return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))


def palette_files(sid=None):
    fs = [os.path.join(EXP, 'bear-proto', 'palette-bear.json'), os.path.join(EXP, 'scene-proto', 'palette.json'),
          os.path.join(HERE, 'palette-ext.json')]
    if sid:
        for d in (stages_dir(), HERE):
            p = os.path.join(d, f'palette-ext-{sid}.json')
            if os.path.exists(p): fs.append(p); break
    return fs


def ramps(sid=None):
    """name -> [rgb, ...] (dark -> light) from the pack palettes + palette-ext.json (+ palette-ext-<id>.json)."""
    out = {}
    for f in palette_files(sid):
        for k, v in json.load(open(f)).items():
            if k != 'note' and isinstance(v, list): out[k] = [hx(h) for h in v if isinstance(h, str) and h.startswith('#')]
    return out


def palette(sid=None):
    return {c for v in ramps(sid).values() for c in v}


INK = hx(json.load(open(os.path.join(EXP, 'bear-proto', 'palette-bear.json')))['ink'][0])


def pixel_errors(a, what, allowed):
    """alpha 0/255, palette, 1 px ink outline (every opaque pixel next to a transparent one or the edge is ink)."""
    e = []; al = a[..., 3]
    if not set(np.unique(al)) <= {0, 255}: e.append(f'{what}: alpha not 0/255')
    bad = set(map(tuple, a[al == 255][:, :3])) - allowed
    if bad: e.append(f'{what}: {len(bad)} colours outside the palette, e.g. ' + ', '.join('#%02x%02x%02x' % c for c in list(bad)[:3]))
    op = al == 255; pad = np.pad(op, 1)
    edge = op & ~(pad[:-2, 1:-1] & pad[2:, 1:-1] & pad[1:-1, :-2] & pad[1:-1, 2:])
    notink = edge & np.any(a[..., :3] != np.array(INK, np.uint8), -1)
    if notink.any(): e.append(f'{what}: {int(notink.sum())} silhouette pixels are not ink (1 px outline)')
    return e


# ------------------------------------------------------------------ images
def load(p):
    from PIL import Image
    return np.asarray(Image.open(p).convert('RGBA')).copy()


def save(a, p):
    from PIL import Image
    Image.fromarray(a).save(p)


def split(strip, w): return [strip[:, i * w:(i + 1) * w].copy() for i in range(strip.shape[1] // w)]
def join(frames): return np.concatenate(frames, 1)
def changed(a, b): return int(np.any(a != b, -1).sum())


_IDLE0 = {}
def idle0(d):
    """Stage A idle frame 0 in direction d and its anchor (always from the real out/bear3d; read once per run)."""
    k = (REAL_OUT, d)
    if k not in _IDLE0:
        a = json.load(open(os.path.join(REAL_OUT, 'anim', 'anim.json')))['anims']['idle']
        _IDLE0[k] = (load(os.path.join(REAL_OUT, 'anim', a['files'][d]))[:, :a['frame_w']].copy(), tuple(a['anchor']))
    spr, anc = _IDLE0[k]
    return spr.copy(), anc


_STAGE_A = {}
def stage_a_frame(clip, frame, d):
    """Stage A <clip> frame <frame> in direction d and its anchor (the real out/bear3d): the target of a
    '<clip>#<frame>' hand-off (idle0 = stage_a_frame('idle', 0, d))."""
    if clip == 'idle' and frame == 0: return idle0(d)
    k = (REAL_OUT, clip, frame, d)
    if k not in _STAGE_A:
        a = json.load(open(os.path.join(REAL_OUT, 'anim', 'anim.json')))['anims'][clip]
        if not 0 <= frame < a['frames_n']: raise ValueError(f'hand-off {clip}#{frame}: {clip} has {a["frames_n"]} frames')
        w = a['frame_w']
        _STAGE_A[k] = (load(os.path.join(REAL_OUT, 'anim', a['files'][d]))[:, frame * w:(frame + 1) * w].copy(), tuple(a['anchor']))
    spr, anc = _STAGE_A[k]
    return spr.copy(), anc


def paste_over(base, spr, x0, y0):
    out = base.copy(); H, W = out.shape[:2]; h, w = spr.shape[:2]
    xs, ys = max(0, -x0), max(0, -y0); xe, ye = min(w, W - x0), min(h, H - y0)
    if xe <= xs or ye <= ys: return out
    s = spr[ys:ye, xs:xe]; m = s[..., 3] > 0
    reg = out[y0 + ys:y0 + ye, x0 + xs:x0 + xe]; reg[m] = s[m]
    return out


def aligned_diff(frame, anchor, spr, spr_anchor, base=None):
    """pixels where frame differs from spr pasted so the anchors meet (over base, default transparent)."""
    b = np.zeros_like(frame) if base is None else base
    want = paste_over(b, spr, int(anchor[0] - spr_anchor[0]), int(anchor[1] - spr_anchor[1]))
    return changed(frame, want), want


def composite(layers):
    """layers [(rgba, depth)], depth inf where empty -> per-pixel nearest (Stage B/D method), owner index (-1 empty)."""
    D = np.stack([d for _, d in layers]); w = D.argmin(0); anyv = np.isfinite(D).any(0)
    out = np.zeros_like(layers[0][0])
    for j, (a, _) in enumerate(layers):
        m = (w == j) & anyv; out[m] = a[m]
    return out, np.where(anyv, w, -1)


def seam_errors(frames, what):
    """a loop's seam (last -> 0) and its step into the last frame change no more px than the largest earlier step."""
    if len(frames) < 3: return []
    steps = [changed(frames[i], frames[i + 1]) for i in range(len(frames) - 2)]
    m = max(steps); e = []
    s1, s0 = changed(frames[-2], frames[-1]), changed(frames[-1], frames[0])
    if s0 > m: e.append(f'{what}: loop seam last -> 0 changes {s0} px (> largest step {m})')
    if s1 > m: e.append(f'{what}: step into the last frame changes {s1} px (> largest step {m})')
    return e


# ------------------------------------------------------------------ geometry (pure: rig K, render_set, make_set)
def snap(ppu, x, y, z=0.0):
    """nearest point (x', y') at height z whose screen offset from the origin is a whole pixel -> (x', y', (dx, dy))."""
    dx = round(ppu * (RIGHT[0] * x + RIGHT[1] * y)); dy = round(-ppu * (UP[0] * x + UP[1] * y + UP[2] * z))
    A = np.array([[RIGHT[0], RIGHT[1]], [UP[0], UP[1]]]); b = np.array([dx / ppu, -dy / ppu - UP[2] * z])
    s = np.linalg.solve(A, b)
    return float(s[0]), float(s[1]), (int(dx), int(dy))


def px_of(ppu, x, y, z=0.0):
    return [ppu * (RIGHT[0] * x + RIGHT[1] * y), -ppu * (UP[0] * x + UP[1] * y + UP[2] * z)]


def root_px(ppu, yaw, d_tx, d_ty):
    """a bear-local root step (tx to the bear's left, ty_px forward; world units) -> screen px [dx, dy] at yaw."""
    t = math.radians(yaw)
    fwd = np.array([math.sin(t), -math.cos(t), 0.0]); left = np.array([math.cos(t), math.sin(t), 0.0])
    w = d_ty * fwd + d_tx * left
    return [round(float(ppu * w @ RIGHT), 4), round(float(-ppu * w @ UP), 4)]


def track_px(ppu, yaw, track):
    return [root_px(ppu, yaw, track[i + 1][0] - track[i][0], track[i + 1][1] - track[i][1]) for i in range(len(track) - 1)]


# ------------------------------------------------------------------ sheets, GIFs, preview
def up(im, s):
    from PIL import Image
    return im.resize((im.width * s, im.height * s), Image.NEAREST)


def gif(frames, path, s=4, durs=None):
    from PIL import Image
    out = []
    for f in frames:
        c = Image.new('RGBA', (f.shape[1], f.shape[0]), BG); c.alpha_composite(Image.fromarray(f))
        out.append(up(c.convert('RGB'), s).convert('P', palette=Image.ADAPTIVE, colors=64))
    out[0].save(path, save_all=True, append_images=out[1:], duration=durs or DUR, loop=0, disposal=1)


def contact_sheet(rows, path, scale=3, per_row=20):
    """rows [(label, [frames])] -> one PNG, a labelled row per entry, frames left to right, at scale."""
    from PIL import Image, ImageDraw
    LBL = 12; out = []
    for lab, fs in rows:
        W, H = fs[0].shape[1], fs[0].shape[0]
        r = Image.new('RGBA', (W * max(per_row, len(fs)), H + LBL), BG)
        ImageDraw.Draw(r).text((2, 0), lab, fill=(40, 34, 52, 255))
        for i, f in enumerate(fs): r.alpha_composite(Image.fromarray(f), (i * W, LBL))
        out.append(r)
    sheet = Image.new('RGBA', (max(r.width for r in out), sum(r.height for r in out)), BG)
    y = 0
    for r in out: sheet.alpha_composite(r, (0, y)); y += r.height
    up(sheet.convert('RGB'), scale).save(path)


def preview_html(title, sections):
    """sections [(heading, [(src, caption)])] -> the preview page (the Stage B/D look)."""
    def fig(src, cap): return f'<figure><img src="{src}"><figcaption>{cap}</figcaption></figure>'
    sec = []
    for h, figs in sections:
        if len(figs) == 1 and figs[0][0].startswith('sheet_'):
            sec.append(f'<section><h2>{h}</h2><img src="{figs[0][0]}" style="zoom:.5"></section>')
        else:
            sec.append(f'<section><h2>{h}</h2><div class="row">' + ''.join(fig(s, c) for s, c in figs) + '</div></section>')
    return ('<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">'
            f'<title>{title}</title>\n'
            '<style>body{background:#1d1b24;color:#ece8f4;font:15px system-ui;padding:24px max(16px,4vw)}h1{font-size:20px}'
            'h2{font-size:15px;margin:22px 0 8px;font-family:ui-monospace,Menlo,monospace;color:#a9dccf}\n'
            '.row{display:flex;gap:12px;flex-wrap:wrap}figure{margin:0;background:#262331;border:1px solid #3a3548;'
            'border-radius:10px;padding:8px;text-align:center}\n'
            'img{image-rendering:pixelated;display:block;max-width:100%}.row img[src$=".png"]{width:auto;zoom:3}'
            'figcaption{font:12px ui-monospace,Menlo,monospace;color:#9a93ad}</style>\n'
            f'<h1>{title}</h1>\n' + '\n'.join(sec) + '\n')


def stage_sheet(sid, d=None):
    """sheet_<id>.png from a rendered stage dir: every clip x ori (sets) or x dir (clip stages), 3x."""
    m = manifest(sid); d = d or os.path.join(out_root(), m['out'])
    J = json.load(open(os.path.join(d, f'anim_{sid}.json'))); rows = []
    for c, a in J['anims'].items():
        for k, f in a['files'].items():
            w = J['sets'][a['set']][k]['frame_w'] if m['kind'] == 'set' else a['frame_w']
            rows.append((f'{c} {k} ({a["frames_n"]}{" loop" if a["loop"] else ""})', split(load(os.path.join(d, f)), w)))
    p = os.path.join(d, f'sheet_{sid}.png'); contact_sheet(rows, p); return p


def stage_preview(sid, d=None):
    m = manifest(sid); d = d or os.path.join(out_root(), m['out'])
    J = json.load(open(os.path.join(d, f'anim_{sid}.json'))); secs = []
    for c, a in J['anims'].items():
        cap = f"{a['frames_n']} frames{' loop' if a['loop'] else ''}"
        secs.append((c, [(f'{c}_{k}.gif', f'{k}, {cap}') for k in a['files'] if os.path.exists(os.path.join(d, f'{c}_{k}.gif'))]))
    if m['kind'] == 'set':
        P = json.load(open(os.path.join(d, f'props_{sid}.json')))['props']
        secs.append(('props (A / B)', [(P[n][o]['file'], f'{n} {o}') for n in P for o in ORIS]))
        secs.append(('props-only frames', [(os.path.basename(J['sets'][s][o]['props_frame']), f'{s} {o}')
                                           for s in J['sets'] for o in ORIS]))
    secs.append(('sheet (3x)', [(f'sheet_{sid}.png', '')]))
    p = os.path.join(d, f'preview_{sid}.html')
    open(p, 'w').write(preview_html(f'Bear, stage {sid}', secs)); return p


# ------------------------------------------------------------------ face part writer
def write_face_part(path, sid, anim_file, clips=None, patch_dirs=None, set_clips=None, exact=None):
    part = dict(face_part=1, stage=sid, anim=anim_file,
                note=('Face part (stage kit #face-part): face patches from face/face.json drawn at these positions on '
                      'this stage\'s frames; exact = frames where they must equal the body exactly.'))
    if clips: part['clips'] = clips
    if patch_dirs: part['patch_dirs'] = patch_dirs
    if set_clips: part['set_clips'] = set_clips
    part['exact'] = exact or {}
    json.dump(part, open(path, 'w'), indent=1)
    return part


# ------------------------------------------------------------------ sha freeze
def freeze(root=None, dirs=None):
    root = os.path.abspath(root or out_root()); out = []
    for d in (dirs or sorted(os.listdir(root))):
        base = os.path.join(root, d)
        if os.path.isfile(base): out.append((hashlib.sha256(open(base, 'rb').read()).hexdigest(), d)); continue
        for dp, dn, fn in os.walk(base):
            dn.sort()
            for f in sorted(fn):
                p = os.path.join(dp, f)
                out.append((hashlib.sha256(open(p, 'rb').read()).hexdigest(), os.path.relpath(p, root)))
    return sorted(out, key=lambda t: t[1])


def check_freeze(shafile, own=(), root=None):
    """errors: files in shafile changed / vanished, or files added under root, outside the own dirs."""
    want = {}
    for line in open(shafile):
        if line.strip() and not line.startswith('#'):
            h, p = line.rstrip('\n').split('  ', 1); want[p] = h
    have = dict((p, h) for h, p in freeze(root))
    own = [o.rstrip('/') + '/' for o in own]
    def mine(p): return any(p.startswith(o) for o in own)
    e = [f'changed: {p}' for p in sorted(want) if p in have and have[p] != want[p] and not mine(p)]
    e += [f'vanished: {p}' for p in sorted(want) if p not in have and not mine(p)]
    e += [f'added: {p}' for p in sorted(have) if p not in want and not mine(p)]
    return e


# ------------------------------------------------------------------ the 2-slot render lock
def lock_dir(): return os.environ.get('BEAR3D_LOCK_DIR') or os.environ.get('TMPDIR') or '/tmp'


def _alive(pid):
    try: os.kill(pid, 0); return True
    except ProcessLookupError: return False
    except PermissionError: return True


def lock_acquire(pid, poll=2.0, slots=2):
    """block until one of the slots is free (a slot whose owner pid is gone is reclaimed); returns the slot path."""
    said = False
    while True:
        for i in range(slots):
            s = os.path.join(lock_dir(), f'bear3d_render.slot{i}')
            try:
                os.mkdir(s)
            except FileExistsError:
                pf = os.path.join(s, 'pid')
                try: owner = int(open(pf).read().strip())
                except (OSError, ValueError):
                    if time.time() - os.path.getmtime(s) > 10: owner = -1    # half-made and abandoned
                    else: continue
                if owner > 0 and _alive(owner): continue
                print(f'[stagekit] reclaiming stale render slot {i} (pid {owner} is gone)', file=sys.stderr)
                try:
                    if os.path.exists(pf): os.remove(pf)
                    os.rmdir(s); os.mkdir(s)
                except OSError: continue
            open(os.path.join(s, 'pid'), 'w').write(str(pid))
            return s
        if not said: print('[stagekit] waiting for a render slot (2 in use)', file=sys.stderr, flush=True); said = True
        time.sleep(poll)


def lock_release(slot, pid):
    pf = os.path.join(slot, 'pid')
    try:
        if int(open(pf).read().strip()) != pid: return False
        os.remove(pf); os.rmdir(slot); return True
    except (OSError, ValueError): return False


# ------------------------------------------------------------------ verify_main (#verify-stage)
class Stage:
    """everything a verifier checks, in memory (the self-test mutates a copy)."""
    def __init__(self, **kw): self.__dict__.update(kw)
    def copy(self):
        import copy
        return Stage(**{k: (v if k == 'mod' else copy.deepcopy(v)) for k, v in self.__dict__.items()})


def load_stage(sid, d):
    m = manifest(sid); mod = module(m['module'])
    J = json.load(open(os.path.join(d, f'anim_{sid}.json')))
    st = Stage(sid=sid, man=m, mod=mod, dir=d, J=J, frames={}, props=None, props_frame={}, prop_sprites={}, part=None,
               allowed=palette(sid), load_errors=[])
    for c, a in J['anims'].items():
        for k, f in a['files'].items():
            p = os.path.join(d, f)
            if not os.path.exists(p): st.load_errors.append(f'{c} {k}: {f} missing'); continue
            w = J['sets'][a['set']][k]['frame_w'] if m['kind'] == 'set' else a['frame_w']
            st.frames[(c, k)] = split(load(p), w)
    if m['kind'] == 'set':
        pp = os.path.join(d, f'props_{sid}.json')
        if os.path.exists(pp):
            st.props = json.load(open(pp))
            for n, v in st.props['props'].items():
                for o in ORIS:
                    if o in v and os.path.exists(os.path.join(d, v[o]['file'])): st.prop_sprites[(n, o)] = load(os.path.join(d, v[o]['file']))
        else: st.load_errors.append(f'props_{sid}.json missing')
        for s, so in J['sets'].items():
            for o in ORIS:
                if o in so:
                    p = os.path.join(d, os.path.basename(so[o]['props_frame']))
                    if os.path.exists(p): st.props_frame[(s, o)] = load(p)
                    else: st.load_errors.append(f'{s} {o}: props frame {so[o]["props_frame"]} missing')
    fp = os.path.join(d, f'face_{sid}.json')
    if os.path.exists(fp): st.part = json.load(open(fp))
    return st


def check_stage(st):
    e = list(st.load_errors); J = st.J; m = st.man; mod = st.mod
    for (c, k), fs in sorted(st.frames.items()):
        for i, f in enumerate(fs): e += pixel_errors(f, f'{c} {k} f{i}', st.allowed)
    if m['kind'] == 'set':
        e += check_set(st)
    else:
        e += check_clips(st)
    return e


def check_set(st):
    e = []; J = st.J; mod = st.mod
    for s, sd in mod.SETS.items():
        so = J['sets'].get(s)
        if not so: e.append(f'set {s} missing from anim_{st.sid}.json'); continue
        for o in ORIS:
            if o not in so: e.append(f'set {s}: ori {o} missing'); continue
            W, H = so[o]['frame_w'], so[o]['frame_h']
            if W > MAX_SET_W or H > MAX_SET_H: e.append(f'set {s} {o}: canvas {W}x{H} exceeds {MAX_SET_W}x{MAX_SET_H}')
            pf = st.props_frame.get((s, o))
            if pf is not None:
                if pf.shape[:2] != (H, W): e.append(f'set {s} {o}: props frame {pf.shape[1]}x{pf.shape[0]} != canvas {W}x{H}')
                e += pixel_errors(pf, f'props frame {s} {o}', st.allowed)
        for c in sd['clips']:
            a = J['anims'].get(c)
            if not a: e.append(f'{c}: missing from anim_{st.sid}.json'); continue
            if a.get('set') != s: e.append(f'{c}: set {a.get("set")} != {s}')
            if a['frames_n'] != sd['counts'][c]: e.append(f'{c}: {a["frames_n"]} frames != {sd["counts"][c]}')
            if a['loop'] != (c in sd['loop']): e.append(f'{c}: loop {a["loop"]} != {c in sd["loop"]}')
            if len(a['frames']) != a['frames_n'] or any(f['dur'] != DUR for f in a['frames']): e.append(f'{c}: frames / dur')
            if sorted(a['files']) != ORIS: e.append(f'{c}: oris {sorted(a["files"])} != {ORIS}')
            for o in ORIS:
                fs = st.frames.get((c, o))
                if fs is None: continue
                W, H = so[o]['frame_w'], so[o]['frame_h']
                if len(fs) != a['frames_n'] or fs[0].shape[:2] != (H, W):
                    e.append(f'{c} {o}: strip {len(fs)} frames of {fs[0].shape[1]}x{fs[0].shape[0]} != {a["frames_n"]} x {W}x{H}')
                    continue
                h = a.get('handoff', {}).get(o)
                if h and (s, o) in st.props_frame:
                    spr, sa = idle0(h['facing'])
                    anc = (so[o]['anchor'][0] + h['px'][0], so[o]['anchor'][1] + h['px'][1])
                    n, _ = aligned_diff(fs[h['frame']], anc, spr, sa, st.props_frame[(s, o)])
                    if n: e.append(f'{c} {o}: hand-off frame {h["frame"]} differs from props + idle_{h["facing"]}#0 in {n} px')
                if c in sd['loop']: e += seam_errors(fs, f'{c} {o}')
            for o, h in (a.get('handoff') or {}).items():
                yaw = sd['hand_yaw'] if o == 'A' else (90.0 - sd['hand_yaw']) % 360
                if h['facing'] != YAW2DIR[round(yaw) % 360]: e.append(f'{c} {o}: hand-off facing {h["facing"]} != {YAW2DIR[round(yaw) % 360]}')
            want = sd.get('handoff', {}).get(c)
            if want and sorted((a.get('handoff') or {})) != ORIS: e.append(f'{c}: hand-off ({want}) missing in an ori')
        for c1, f1, c2, f2 in sd.get('joins', []):
            for o in ORIS:
                x, y = st.frames.get((c1, o)), st.frames.get((c2, o))
                if x and y and not np.array_equal(x[f1], y[f2]): e.append(f'join {c1}[{f1}] = {c2}[{f2}] {o}: {changed(x[f1], y[f2])} px differ')
    if st.props is None: return e
    for n, v in st.props['props'].items():
        if v.get('set') not in mod.SETS: e.append(f'prop {n}: set {v.get("set")} unknown')
        for o in ORIS:
            if o not in v: e.append(f'prop {n}: ori {o} missing'); continue
            spr = st.prop_sprites.get((n, o))
            if spr is None: e.append(f'prop {n} {o}: {v[o]["file"]} missing'); continue
            if list(spr.shape[1::-1]) != v[o]['size']: e.append(f'prop {n} {o}: size {list(spr.shape[1::-1])} != {v[o]["size"]}')
            e += pixel_errors(spr, f'prop {n} {o}', st.allowed)
    face = [c for sd in mod.SETS.values() for c in sd.get('face', [])]
    sc = (st.part or {}).get('set_clips', {})
    if face and st.part is None: e.append(f'face_{st.sid}.json missing (face clips {face})')
    for c in face:
        if c not in sc: e.append(f'face part: set_clips {c} missing'); continue
        for o in ORIS:
            if sc[c].get(o, {}).get('patch') != st.J['anims'][c].get('face_patch_dir', {}).get(o):
                e.append(f'face part {c} {o}: patch {sc[c].get(o, {}).get("patch")} != anim face_patch_dir')
            for s in ('eyes', 'mouth'):
                if len(sc[c].get(o, {}).get(s, [])) != st.J['anims'][c]['frames_n']: e.append(f'face part {c} {o} {s}: positions != frames')
    return e


def check_clips(st):
    e = []; J = st.J; mod = st.mod; ppu = J['ppu']
    order = list(mod.ORDER)
    if list(J['anims']) != order: e.append(f'clips {list(J["anims"])} != {order}')
    cdefs = mod.CLIPS if hasattr(mod, 'CLIPS') else {}
    for c, a in J['anims'].items():
        cd = cdefs.get(c, {})
        dirs = cd.get('dirs') or DIRS
        if sorted(a['files']) != sorted(dirs): e.append(f'{c}: dirs {sorted(a["files"])} != {sorted(dirs)}')
        if cd and a['frames_n'] != cd['frames']: e.append(f'{c}: {a["frames_n"]} frames != {cd["frames"]}')
        if cd and a['loop'] != cd.get('loop', False): e.append(f'{c}: loop flag')
        fw = [a['frame_w'], a['frame_h'], *a['anchor']]
        if cd and fw != [*(cd.get('frame') or (84, 123)), *(cd.get('anchor') or (42, 107))]:
            e.append(f'{c}: frame {fw[0]}x{fw[1]} anchor {fw[2:]} != the module\'s {cd.get("frame") or [84, 123]} {cd.get("anchor") or [42, 107]}')
        tr = a.get('root_track')
        if not tr or len(tr) != a['frames_n'] + 1: e.append(f'{c}: root_track missing or not frames_n + 1 long'); tr = None
        for d in a['files']:
            fs = st.frames.get((c, d))
            if fs is None: continue
            if len(fs) != a['frames_n']: e.append(f'{c} {d}: {len(fs)} frames != {a["frames_n"]}')
            if fs[0].shape[:2] != (a['frame_h'], a['frame_w']): e.append(f'{c} {d}: frames {fs[0].shape[1]}x{fs[0].shape[0]} != frame_w x frame_h')
            for at, i in (('start', 0), ('end', len(fs) - 1)):
                h = (a.get('handoff') or {}).get(at)
                if h == 'idle0':
                    spr, sa = idle0(d)
                    n, _ = aligned_diff(fs[i], a['anchor'], spr, sa)
                    if n: e.append(f'{c} {d}: hand-off ({at}) frame {i} differs from idle_{d}#0 in {n} px (anchors aligned)')
                elif h and '#' in h:
                    src, fr = h.split('#')[0], int(h.split('#')[1])
                    spr, sa = stage_a_frame(src, fr, d)
                    n, _ = aligned_diff(fs[i], a['anchor'], spr, sa)
                    if n: e.append(f'{c} {d}: hand-off ({at}) frame {i} differs from {src}_{d}#{fr} in {n} px (anchors aligned)')
            if a['loop']: e += seam_errors(fs, f'{c} {d}')
            one = [f['root_px'].get(d) for f in a['frames']]; two = (a.get('root_px') or {}).get(d)
            if one != two: e.append(f'{c} {d}: frames[].root_px and root_px[{d}] differ (they are ONE track)')
            if tr:
                want = track_px(ppu, YAW[d], tr)
                drift = max(max(abs(p[0] - q[0]), abs(p[1] - q[1])) for p, q in zip(one, want)) if one and None not in one else 99
                if drift > 0.01: e.append(f'{c} {d}: root_px is not the projection of root_track (off by {drift:.2f} px)')
    for c1, f1, c2, f2 in getattr(mod, 'CHAINS', []):
        for d in DIRS:
            x, y = st.frames.get((c1, d)), st.frames.get((c2, d))
            if x and y and not np.array_equal(x[f1], y[f2]): e.append(f'chain {c1}[{f1}] = {c2}[{f2}] {d}: {changed(x[f1], y[f2])} px differ')
    return e


def _std_mutations(st):
    """the 5 standard mutations: (name, mutate(stage), substring the report must contain)."""
    keys = sorted(st.frames)
    ck = keys[len(keys) // 2]

    def opaque_inner(a):
        op = a[..., 3] == 255; pad = np.pad(op, 1)
        inner = op & pad[:-2, 1:-1] & pad[2:, 1:-1] & pad[1:-1, :-2] & pad[1:-1, 2:]
        ys, xs = np.nonzero(inner); k = len(ys) // 2
        return int(ys[k]), int(xs[k])

    def m_alpha(s):
        y, x = opaque_inner(s.frames[ck][0]); s.frames[ck][0][y, x, 3] = 128; return 'alpha not 0/255'
    def m_palette(s):
        y, x = opaque_inner(s.frames[ck][0]); s.frames[ck][0][y, x, :3] = (1, 2, 3); return 'outside the palette'
    def m_outline(s):
        f = s.frames[ck][0]; op = f[..., 3] == 255; pad = np.pad(op, 1)
        edge = op & ~(pad[:-2, 1:-1] & pad[2:, 1:-1] & pad[1:-1, :-2] & pad[1:-1, 2:])
        ys, xs = np.nonzero(edge); y, x = int(ys[0]), int(xs[0])
        f[y, x, :3] = sorted(s.allowed - {INK})[0]; return 'not ink'
    def m_handoff(s):
        for c, a in s.J['anims'].items():
            h = a.get('handoff') or {}
            if s.man['kind'] == 'set':
                for o, hh in h.items():
                    fs = s.frames[(c, o)]; fs[hh['frame']] = np.roll(fs[hh['frame']], 1, 1); return 'hand-off'
            else:
                for at in ('start', 'end'):
                    if h.get(at) == 'idle0':
                        d = next(iter(a['files'])); fs = s.frames[(c, d)]; i = 0 if at == 'start' else len(fs) - 1
                        fs[i] = np.roll(fs[i], 1, 1); return 'hand-off'
        raise AssertionError('no hand-off frame to mutate')
    def m_missing(s):
        c = next(iter(s.J['anims'])); a = s.J['anims'][c]; k = sorted(a['files'])[-1]
        del a['files'][k]; s.frames.pop((c, k), None); return 'oris' if s.man['kind'] == 'set' else 'dirs'
    def m_rootpx(s):
        c = next(iter(s.J['anims'])); a = s.J['anims'][c]; d = sorted(a['files'])[0]
        a['root_px'][d] = [[p[0] + 1, p[1]] for p in a['root_px'][d]]; return 'ONE track'
    def m_frame(s):
        c = next(iter(s.J['anims'])); s.J['anims'][c]['frame_w'] += 2; return 'frame'
    def m_stagea(s):
        for c, a in s.J['anims'].items():
            for at, h in (a.get('handoff') or {}).items():
                if isinstance(h, str) and '#' in h:
                    d = next(iter(a['files'])); fs = s.frames[(c, d)]; i = 0 if at == 'start' else len(fs) - 1
                    fs[i] = np.roll(fs[i], 1, 1); return f'differs from {h.split("#")[0]}_'
    extra = [('the two root_px forms differ', m_rootpx), ('a clip frame size != its module frame', m_frame)] \
        if st.man['kind'] == 'clips' else []
    if st.man['kind'] == 'clips' and any('#' in str(h) for a in st.J['anims'].values() for h in (a.get('handoff') or {}).values()):
        extra.append(('a <clip>#<frame> hand-off frame shifted 1 px', m_stagea))
    return extra + [('alpha 128 on one pixel', m_alpha), ('a colour outside the palette', m_palette),
            ('a gap in the 1 px ink outline', m_outline), ('a hand-off frame shifted 1 px', m_handoff),
            ('a missing ' + ('ori' if st.man['kind'] == 'set' else 'dir'), m_missing)]


def verify_main(sid, extra=None, mutations=()):
    """the verifier of stage sid: python3 verify_<id>.py [--dir DIR] [--self-test]. extra(stage) -> errors (the
    stage's own checks); mutations [(name, mutate(stage) -> expected report substring)] add to the 5 standard ones."""
    argv = sys.argv[1:]
    m = manifest(sid)
    d = os.path.abspath(argv[argv.index('--dir') + 1]) if '--dir' in argv else os.path.join(out_root(), m['out'])
    st = load_stage(sid, d)
    def run(s): return check_stage(s) + (extra(s) if extra else [])
    if '--self-test' in argv:
        base = run(st)
        if base: print('SELF-TEST needs a valid stage first:', *base[:10], sep='\n  '); sys.exit(2)
        ok = True; cases = _std_mutations(st) + list(mutations)
        for i, (name, mut) in enumerate(cases, 1):
            s = st.copy()
            try: want = mut(s); got = run(s)
            except Exception as ex:   # a crash is not a caught mutation (exit 1 must mean every one was caught)
                print(f'SELF-TEST {i}: {name}: CRASHED ({type(ex).__name__}: {ex})'); ok = False; continue
            hit = [x for x in got if want in x]
            print(f'SELF-TEST {i}: {name}: ' + (f'ok: {hit[0]}' if hit else f'MISSED (want "{want}", got {got[:3]})'))
            ok &= bool(hit)
        if ok: print(f'SELF-TEST OK: all {len(cases)} mutations caught; exit 1'); sys.exit(1)
        print('SELF-TEST FAILED'); sys.exit(2)
    errs = run(st)
    nf = sum(len(v) for v in st.frames.values())
    if errs:
        print(f'verify_{sid}: FAIL'); [print(' ', x) for x in errs[:60]]; sys.exit(1)
    print(f'verify_{sid}: PASS ({len(st.J["anims"])} clips, {len(st.frames)} strips, {nf} frames; pixels, counts, '
          f'hand-offs, joins, seams{", props, face part" if m["kind"] == "set" else ", root_px"} ok)')


# ------------------------------------------------------------------ self-test
def self_test():
    import tempfile, shutil
    ok = True
    def t(name, cond):
        nonlocal ok
        print(('  ok   ' if cond else '  FAIL ') + name); ok &= bool(cond)
    pal = palette()
    a = np.zeros((6, 6, 4), np.uint8); a[1:5, 1:5] = (*INK, 255); a[2:4, 2:4] = (*sorted(pal - {INK})[0], 255)
    t('pixel_errors: a clean sprite passes', pixel_errors(a, 'x', pal) == [])
    b = a.copy(); b[2, 2, 3] = 128; t('pixel_errors: alpha 128 caught', any('alpha' in x for x in pixel_errors(b, 'x', pal)))
    b = a.copy(); b[2, 2, :3] = (1, 2, 3); t('pixel_errors: off-palette caught', any('palette' in x for x in pixel_errors(b, 'x', pal)))
    b = a.copy(); b[1, 2, :3] = a[2, 2, :3]; t('pixel_errors: outline gap caught', any('ink' in x for x in pixel_errors(b, 'x', pal)))
    spr = a.copy(); base = np.zeros((10, 12, 4), np.uint8)
    n, want = aligned_diff(paste_over(base, spr, 3, 2), (6, 5), spr, (3, 3))
    t('aligned_diff: anchors aligned -> 0 px', n == 0)
    t('aligned_diff: 1 px shift caught', aligned_diff(np.roll(want, 1, 1), (6, 5), spr, (3, 3))[0] > 0)
    l1 = (a, np.where(a[..., 3] > 0, 1.0, np.inf)); b = np.zeros_like(a); b[0:3, 0:3] = (*INK, 255)
    out, own = composite([l1, (b, np.where(b[..., 3] > 0, 0.5, np.inf))])
    t('composite: the nearer layer wins', own[1, 1] == 1 and own[4, 4] == 0 and own[5, 5] == -1)
    fs = [np.zeros((4, 4, 4), np.uint8) for _ in range(4)]; fs[1][0, 0] = 255; fs[2][0, 0] = 255
    t('seam: a smooth loop passes', seam_errors(fs, 'x') == [])
    fs[3][:] = 255; t('seam: a jump at the seam caught', seam_errors(fs, 'x') != [])
    x, y, (dx, dy) = snap(40.07, 0.31, -0.77)
    p = px_of(40.07, x, y)
    t('snap: whole-pixel screen offset', abs(p[0] - dx) < 1e-6 and abs(p[1] - dy) < 1e-6)
    t('root_px: forward at SE (yaw 90) moves right/down', root_px(40.0, 90, 0, 1.0)[0] > 0 and root_px(40.0, 90, 0, 1.0)[1] > 0)
    t('root_px: forward at S is straight down', abs(root_px(40.0, 45, 0, 1.0)[0]) < 1e-3 and root_px(40.0, 45, 0, 1.0)[1] > 0)
    import anim_clips as AC
    def k_ok(q, face):
        try: AC.check_k(q, 'x', face); return True
        except AssertionError: return False
    t('check_k: a layers clip with lean != 0 is refused', not k_ok(AC.KP(lean=5.0), 'layers'))
    t('check_k: a layers clip with tumble / lift != 0 is refused', not k_ok(AC.KP(tumble=30.0), 'layers')
      and not k_ok(AC.KP(lift=0.1), 'layers'))
    t('check_k: a baked / none clip may lean, roll (<= 90) and tumble', k_ok(AC.KP(lean=40.0, roll=-20.0, tumble=270.0), 'none')
      and k_ok(AC.KP(lean=7.0), 'baked') and k_ok(AC.KP(lean=7.0), None))
    t('check_k: lean beyond 90 is refused', not k_ok(AC.KP(lean=95.0), 'none'))
    tmp = tempfile.mkdtemp(prefix='stagekit_')
    try:
        os.makedirs(os.path.join(tmp, 'out', 'x')); os.makedirs(os.path.join(tmp, 'out', 'mine'))
        open(os.path.join(tmp, 'out', 'x', 'a.txt'), 'w').write('a')
        open(os.path.join(tmp, 'out', 'mine', 'm.txt'), 'w').write('m')
        sha = os.path.join(tmp, 'f.sha')
        open(sha, 'w').write(''.join(f'{h}  {p}\n' for h, p in freeze(os.path.join(tmp, 'out'))))
        t('freeze/check: unchanged -> clean', check_freeze(sha, root=os.path.join(tmp, 'out')) == [])
        open(os.path.join(tmp, 'out', 'x', 'a.txt'), 'w').write('b')
        t('check: a changed file caught', check_freeze(sha, root=os.path.join(tmp, 'out')) == ['changed: x/a.txt'])
        open(os.path.join(tmp, 'out', 'x', 'a.txt'), 'w').write('a'); open(os.path.join(tmp, 'out', 'x', 'n.txt'), 'w').write('n')
        t('check: an added file caught', check_freeze(sha, root=os.path.join(tmp, 'out')) == ['added: x/n.txt'])
        os.remove(os.path.join(tmp, 'out', 'x', 'n.txt')); os.remove(os.path.join(tmp, 'out', 'mine', 'm.txt'))
        open(os.path.join(tmp, 'out', 'mine', 'z.txt'), 'w').write('z')
        r = check_freeze(sha, root=os.path.join(tmp, 'out'))
        t('check: vanished caught outside --own', 'vanished: mine/m.txt' in r)
        t('check: --own dir changes allowed', check_freeze(sha, own=['mine'], root=os.path.join(tmp, 'out')) == [])
        os.environ['BEAR3D_LOCK_DIR'] = tmp
        s0 = lock_acquire(os.getpid()); s1 = lock_acquire(os.getpid())
        t('lock: two slots', {os.path.basename(s0), os.path.basename(s1)} == {'bear3d_render.slot0', 'bear3d_render.slot1'})
        import subprocess
        p = subprocess.Popen(['true']); p.wait()
        open(os.path.join(s1, 'pid'), 'w').write(str(p.pid))         # a slot held by a pid that is gone
        s2 = lock_acquire(os.getpid(), poll=0.1)
        t('lock: a stale slot is reclaimed', s2 == s1)
        t('lock: release needs the owner pid', not lock_release(s0, 1) and lock_release(s0, os.getpid()))
        t('lock: released slot is free again', lock_acquire(os.getpid()) == s0)
        fp = os.path.join(tmp, 'face_x.json')
        write_face_part(fp, 'x', 'anim_x.json', set_clips={'c': {'A': dict(patch='SE', eyes=[[1, 2]], mouth=[[3, 4]])}},
                        exact={'c': {'A': [0]}})
        P = json.load(open(fp)); t('face part: written', P['face_part'] == 1 and P['exact'] == {'c': {'A': [0]}})
    finally:
        os.environ.pop('BEAR3D_LOCK_DIR', None); shutil.rmtree(tmp)
    if ok: print('SELF-TEST OK: every helper behaves; exit 1'); sys.exit(1)
    print('SELF-TEST FAILED'); sys.exit(2)


def main(argv):
    if not argv or argv[0] in ('-h', '--help'): print(__doc__); return 2
    if argv[0] == '--self-test': self_test()
    def opt(n, many=False):
        if n not in argv: return [] if many else None
        i = argv.index(n) + 1
        if not many: return argv[i]
        out = []
        while i < len(argv) and not argv[i].startswith('--'): out.append(argv[i]); i += 1
        return out
    cmd = argv[0]
    if cmd == 'freeze':
        for h, p in freeze(opt('--root'), opt('--dirs', True) or None): print(f'{h}  {p}')
        return 0
    if cmd == 'check':
        e = check_freeze(argv[1], opt('--own', True), opt('--root'))
        for x in e[:80]: print(x)
        print(f'freeze check: {"FAIL, " + str(len(e)) + " differences" if e else "clean"} ({argv[1]})')
        return 1 if e else 0
    if cmd == 'sheet': print('wrote', stage_sheet(argv[1])); return 0
    if cmd == 'preview': print('wrote', stage_preview(argv[1])); return 0
    if cmd == 'manifest':
        m = manifest(argv[1])
        if '--sh' in argv:
            for k in ('id', 'kind', 'module', 'verify', 'out'): print(f'{k.upper()}={m[k]}')
        else: print(json.dumps(m, indent=1))
        return 0
    if cmd == 'lock':
        pid = int(opt('--pid') or os.getppid())
        if argv[1] == 'acquire': print(lock_acquire(pid)); return 0
        if argv[1] == 'release': return 0 if lock_release(argv[2], pid) else 1
    print(__doc__); return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
