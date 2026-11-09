"""Assemble the Stage B deliverables (python3 + numpy + Pillow).

    python3 make_b.py PXROOT ANIM_OUT PROPS_OUT
PXROOT/<set>_<ori>/ from render_b.py + pixelize_b.py: layout.json, props.json, clip_<name>.json and the pixel
layers (<key>.png + <key>_d.npy). The Stage A rest sprites come from ../out/bear3d/anim (idle_<DIR>.png frame 0).

Frames: every layer (bear, bed, table, pouf, book) is composited per pixel by depth. The hand-off frames (first
frame of get_in_bed / sit_down, last of wake_up / stand_up) are built as props-only frame + Stage A rest sprite
pasted at the hand-off pixel, so they match the runtime's free bear + prop sprites exactly (verify_b.py checks).
The Zzz effect is drawn on top (ink outline, beanie-lavender fill); it is also written as its own strip.

ANIM_OUT:  <clip>_<ori>.png strips, <clip>_<ori>.gif (4x), seq_bed_A.gif / seq_read_A.gif, zzz_<ori>.png,
           anim_b.json, preview_b.html
PROPS_OUT: set_<set>_<ori>.png (props-only frame, clip canvas), <prop>_<ori>.png (cropped), props.json,
           palette-ext.json (copy)
Prints the flicker checks (changed pixels per frame step, seams between chained clips).
"""
import sys, os, json, math, shutil
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import pixelize as PB
import pixelize_b as PX
import clips_b

PXR, OUT, POUT = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(OUT, exist_ok=True); os.makedirs(POUT, exist_ok=True)
A_DIR = os.path.join(HERE, '..', 'out', 'bear3d', 'anim')
AJ = json.load(open(os.path.join(A_DIR, 'anim.json')))
A_ANCHOR = AJ['anims']['idle']['anchor']; A_FW = AJ['anims']['idle']['frame_w']
YAW2DIR = {v['yaw_deg'] % 360: d for d, v in AJ['directions'].items()}
BG = (200, 196, 210, 255); FPS = clips_b.FPS; DUR = round(1000 / FPS)
SETS = ['bed', 'read']; ORIS = ['A', 'B']
ORI_NOTE = {'bed': {'A': 'headboard to the NE (bed long axis along world +Y), entered from its SE side',
                    'B': 'mirror of A: headboard to the NW, entered from its SW side'},
            'read': {'A': 'pouf on the NW side of the table, bear reads facing SE',
                     'B': 'mirror of A: pouf on the NE side, bear reads facing SW'}}
ZFILL = PB.hx('#d3c6ea'); ZINK = PB.INK

# ------------------------------------------------------------------ layers + composite
_cache = {}
def layer(d, key):
    p = os.path.join(d, key)
    if p not in _cache:
        a = np.asarray(Image.open(p + '.png').convert('RGBA')).copy(); dep = np.load(p + '_d.npy')
        _cache[p] = (a, np.where(a[..., 3] > 0, dep, np.inf))
    return _cache[p]

def composite(d, keys, want_owner=False):
    L = [layer(d, k) for k in keys]
    D = np.stack([x[1] for x in L]); w = D.argmin(0); anyv = np.isfinite(D).any(0)
    out = np.zeros_like(L[0][0])
    for j, (a, _) in enumerate(L):
        m = (w == j) & anyv; out[m] = a[m]
    return (out, np.where(anyv, w, -1)) if want_owner else out

def paste_over(base, spr, x0, y0):
    """opaque pixels of spr overwrite base (spr top-left at x0, y0)."""
    out = base.copy(); H, W = out.shape[:2]; h, w = spr.shape[:2]
    xs, ys = max(0, -x0), max(0, -y0); xe, ye = min(w, W - x0), min(h, H - y0)
    s = spr[ys:ye, xs:xe]; m = s[..., 3] > 0
    reg = out[y0 + ys:y0 + ye, x0 + xs:x0 + xe]; reg[m] = s[m]
    return out

def rest_sprite(yaw):
    d = YAW2DIR[round(yaw) % 360]
    im = np.asarray(Image.open(os.path.join(A_DIR, f'idle_{d}.png')).convert('RGBA'))
    return d, im[:, :A_FW].copy()

# ------------------------------------------------------------------ Zzz effect
GLYPH = {1: ['1111', '0010', '0100', '1111'],
         2: ['11111', '00010', '00100', '01000', '11111'],
         3: ['111111', '000010', '000100', '001000', '010000', '111111']}
GL = {k: np.array([[c == '1' for c in r] for r in v]) for k, v in GLYPH.items()}
def zzz_glyphs(phase, sx):
    """[(x, y, size)] relative to the head point, for loop phase 0..35. sx = +1 drift right (A), -1 left (B)."""
    out = []
    for off in (0, 12, 24):
        t = ((phase - off) % 36) / 36.0
        if t < 0.06: continue                                           # just spawned: hidden for 2 frames
        size = 1 if t < 0.38 else 2 if t < 0.72 else 3
        n = GL[size].shape[0]
        x = 3 + 17 * t + 2.5 * math.sin(2 * math.pi * t * 1.5); y = -(5 + 29 * t)
        out.append((int(round(sx * x - (n - 1) / 2)), int(round(y - (n - 1) / 2)), size))
    return out
def draw_zzz(img, head, phase, sx):
    out = img.copy(); H, W = out.shape[:2]
    for gx, gy, size in zzz_glyphs(phase, sx):
        g = GL[size]; n = g.shape[0]
        pad = np.pad(g, 1); ring = np.zeros_like(pad)
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1): ring |= np.roll(np.roll(pad, dy, 0), dx, 1)
        x0, y0 = head[0] + gx - 1, head[1] + gy - 1
        for yy in range(n + 2):
            for xx in range(n + 2):
                X, Y = x0 + xx, y0 + yy
                if not (0 <= X < W and 0 <= Y < H): continue
                if pad[yy, xx]: out[Y, X] = (*ZFILL, 255)
                elif ring[yy, xx]: out[Y, X] = (*ZINK, 255)
    return out

# ------------------------------------------------------------------ build all frames
def up(im, s): return im.resize((im.width * s, im.height * s), Image.NEAREST)
def gif(frames, path, s=4, durs=None):
    out = []
    for f in frames:
        c = Image.new('RGBA', (f.shape[1], f.shape[0]), BG); c.alpha_composite(Image.fromarray(f))
        out.append(up(c.convert('RGB'), s).convert('P', palette=Image.ADAPTIVE, colors=64))
    out[0].save(path, save_all=True, append_images=out[1:], duration=durs or DUR, loop=0, disposal=1)

anims, sets_json, props_json, checks = {}, {}, {}, []
FR = {}                                   # (clip, ori) -> list of rgba frames
for s in SETS:
    sets_json[s] = {}
    for o in ORIS:
        d = os.path.join(PXR, f'{s}_{o}'); lay = json.load(open(os.path.join(d, 'layout.json')))
        W, H = lay['W'], lay['H']; OX, OY = lay['origin']; sx = -1 if o == 'B' else 1
        pk = json.load(open(os.path.join(d, 'props.json')))
        props_frame = composite(d, list(pk.values()))
        Image.fromarray(props_frame).save(os.path.join(POUT, f'set_{s}_{o}.png'))
        sets_json[s][o] = dict(frame_w=W, frame_h=H, anchor=[OX, OY], props_frame=f'../props/set_{s}_{o}.png',
                               orientation=ORI_NOTE[s][o])
        # individual prop sprites, cropped, anchor = the prop's origin pixel
        for name, key in pk.items():
            a = layer(d, key)[0]; ys, xs = np.nonzero(a[..., 3])
            x0, x1, y0, y1 = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
            Image.fromarray(a[y0:y1, x0:x1]).save(os.path.join(POUT, f'{name}_{o}.png'))
            ppx = lay['prop_px'][name]
            props_json.setdefault(name, {})[o] = dict(file=f'{name}_{o}.png', size=[int(x1 - x0), int(y1 - y0)],
                                                     anchor=[int(OX + ppx[0] - x0), int(OY + ppx[1] - y0)],
                                                     offset_px_in_set=ppx, layer_key=key)
        zhead = [int(round(v)) for v in lay['zzz_head']] if lay.get('zzz_head') else None
        for c in clips_b.ORDER[s]:
            C = json.load(open(os.path.join(d, f'clip_{c}.json')))
            frames = []
            for f, fr in enumerate(C['frames']):
                im = composite(d, list(fr['keys'].values()))
                if fr['zzz'] >= 0: im = draw_zzz(im, zhead, fr['zzz'], sx)
                frames.append(im)
            h = lay['handoff'].get(c)
            ho = None
            if h:
                dname, spr = rest_sprite(h['yaw'])
                x0, y0 = OX + h['px'][0] - A_ANCHOR[0], OY + h['px'][1] - A_ANCHOR[1]
                hf = paste_over(props_frame, spr, x0, y0)
                i = 0 if h['at'] == 'start' else len(frames) - 1
                b_rest, owner = composite(d, list(C['frames'][i]['keys'].values()), want_owner=True)
                bear_lost = int(((layer(d, C['frames'][i]['keys']['bear'])[0][..., 3] > 0) & (owner != 0)).sum())
                diff = int((np.any(b_rest != hf, -1)).sum())
                checks.append(f'{c} {o}: hand-off frame {i} = props + Stage A idle_{dname} f0; fresh B render of the rest '
                              f'bear differs in {diff} px; bear pixels hidden by props there: {bear_lost}')
                frames[i] = hf
                ho = dict(at=h['at'], frame=i, pos_world=[round(v, 4) for v in h['pos']], px=h['px'],
                          facing=dname, yaw_deg=h['yaw'], stage_a_sprite=f'../anim/idle_{dname}.png#0')
            FR[(c, o)] = frames
            n = len(frames)
            strip = np.zeros((H, W * n, 4), np.uint8)
            for f, im in enumerate(frames): strip[:, f * W:(f + 1) * W] = im
            Image.fromarray(strip).save(os.path.join(OUT, f'{c}_{o}.png'))
            gif(frames, os.path.join(OUT, f'{c}_{o}.gif'))
            a = anims.setdefault(c, dict(set=s, loop=C['loop'], fps=FPS, frames_n=n, files={}, handoff={},
                                         frames=[dict(dur=DUR, zzz=fr['zzz']) for fr in C['frames']]))
            a['files'][o] = f'{c}_{o}.png'
            if ho: a['handoff'][o] = ho
        # Zzz alone, as its own strip (anchor = the sleeping head point)
        g = [zzz_glyphs(p, sx) for p in range(36)]
        allx = [x + dx for gg in g for (x, y, sz) in gg for dx in (-1, GL[sz].shape[0])]
        ally = [y + dy for gg in g for (x, y, sz) in gg for dy in (-1, GL[sz].shape[0])]
        zx0, zy0 = min(allx), min(ally); zw, zh = max(allx) - zx0 + 1, max(ally) - zy0 + 1
        zs = np.zeros((zh, zw * 36, 4), np.uint8)
        for p in range(36):
            zs[:, p * zw:(p + 1) * zw] = draw_zzz(np.zeros((zh, zw, 4), np.uint8), (-zx0, -zy0), p, sx)
        Image.fromarray(zs).save(os.path.join(OUT, f'zzz_{o}.png'))
        if s == 'bed':
            sets_json[s][o]['zzz'] = dict(file=f'zzz_{o}.png', frame_w=zw, frame_h=zh, frames_n=36, anchor=[-zx0, -zy0],
                                          head_px_in_set=[zhead[0] - OX, zhead[1] - OY])

for c, a in anims.items():
    if not a.get('handoff'): a.pop('handoff', None)
    nxt = {'get_in_bed': 'sleep', 'sleep': 'sleep or wake_up', 'sit_down': 'read', 'read': 'read or stand_up'}
    a['next'] = nxt.get(c, 'Stage A idle (rest pose, same spot and facing)')

# sequences (ori A) for the preview
for s, seq in (('bed', [('get_in_bed', 1), ('sleep', 2), ('wake_up', 1)]), ('read', [('sit_down', 1), ('read', 1), ('stand_up', 1)])):
    fs = []
    for c, r in seq: fs += FR[(c, 'A')] * r
    durs = [500] + [DUR] * (len(fs) - 2) + [700]
    gif(fs, os.path.join(OUT, f'seq_{s}_A.gif'), durs=durs)

# ------------------------------------------------------------------ JSON
json.dump(dict(
    note=('Stage B clips (bed: get_in_bed -> sleep (loop) -> wake_up; reading: sit_down -> read (loop) -> stand_up). '
          'Same iso camera, PPU and palette rules as Stage A. Each frame is the WHOLE set (props + bear) baked on one '
          'canvas; anchor = pixel of the set origin (bed: footprint centre; reading: table footprint centre) on the floor. '
          'ori A is the default placement, ori B its true 3D mirror (light unchanged, not a sprite flip). '
          'Hand-off: the first frame of get_in_bed / sit_down and the last of wake_up / stand_up are pixel-identical to '
          'the props-only frame + the Stage A idle sprite (frame 0) at handoff.px from the anchor (Stage A anchor on '
          'that pixel), facing handoff.facing; pos_world is the same spot in world units from the set origin.'),
    sorting=('Outside these clips the runtime draws the props (props/*.png, or set_<set>_<ori>.png) and the free bear '
             'sprite as separate sprites and sorts whole sprites back to front by floor depth (world x - y of each '
             'anchor, i.e. screen y of the floor point; bear vs bed: use the bed footprint in props.json). From the '
             'first to the last frame of a Stage B clip, hide the set\'s prop sprites and the bear sprite and draw the '
             'baked frame instead at the set anchor, sorted as the set (the bear never leaves the set footprint plus '
             'the hand-off spot). At the hand-off spots the bear is in front of every prop pixel, so swapping back to '
             'separate sprites cannot change the picture. Zzz: baked into sleep (and wake_up frame 0); zzz_<ori>.png '
             'is the same effect alone if the runtime wants to draw it as an overlay instead.'),
    fps=FPS, ppu=AJ['ppu'], sets=sets_json, anims=anims),
    open(os.path.join(OUT, 'anim_b.json'), 'w'), indent=1)

FOOT = {'bed': dict(x=[-1.0, 1.0], y=[-2.0, 2.0]), 'table': dict(x=[-0.65, 0.65], y=[-0.65, 0.65]),
        'pouf': dict(x=[-0.46, 0.46], y=[-0.46, 0.46]), 'book': dict(on='table', x=[-0.33, 0.33], y=[-0.5, 0.5])}
lay_bed = json.load(open(os.path.join(PXR, 'bed_A', 'layout.json'))); lay_read = json.load(open(os.path.join(PXR, 'read_A', 'layout.json')))
for name in props_json:
    lay = lay_bed if name == 'bed' else lay_read
    props_json[name]['footprint_world'] = dict(FOOT[name], note='prop-local, ori A; ori B: (x, y) -> (-y, -x)')
    props_json[name]['origin_world_in_set'] = [round(v, 4) for v in lay['prop_pos'][name]]
    props_json[name]['set'] = 'bed' if name == 'bed' else 'read'
json.dump(dict(
    note=('Stage B props, modelled in Blender (render_b.py, chunky superellipsoid toy shapes), pre-rendered with the '
          'Stage A camera and pixel step. Each prop sprite = that prop rendered alone with its own ink outline; '
          'anchor = pixel of the prop origin (bed / table / pouf: floor point at the footprint centre; book: its '
          'origin on the table top, z = 1.0). set_<set>_<ori>.png = the props-only frame on the clip canvas. '
          'Footprints in world units (1 unit = ppu px along the iso axes). ori B = true 3D mirror of ori A. '
          'Colours: palette-bear.json + moss + palette-ext.json (wood, fabric ramps; local extension). '
          'Each prop object has its own material / pass index (pass_ids).'),
    ppu=AJ['ppu'], pass_ids=lay_bed['pass_ids'], props=props_json),
    open(os.path.join(POUT, 'props.json'), 'w'), indent=1)
shutil.copy(os.path.join(HERE, 'palette-ext.json'), os.path.join(POUT, 'palette-ext.json'))

# ------------------------------------------------------------------ preview
def fig(src, cap): return f'<figure><img src="{src}"><figcaption>{cap}</figcaption></figure>'
sec = []
sec.append('<section><h2>sequences (ori A)</h2><div class="row">' + fig('seq_bed_A.gif', 'get_in_bed, sleep x2, wake_up')
           + fig('seq_read_A.gif', 'sit_down, read, stand_up') + '</div></section>')
for c in [x for s in SETS for x in clips_b.ORDER[s]]:
    a = anims[c]
    cap = f"{a['frames_n']} frames{' loop' if a['loop'] else ''}"
    sec.append(f'<section><h2>{c}</h2><div class="row">' + fig(f'{c}_A.gif', f'A, {cap}') + fig(f'{c}_B.gif', f'B (mirror), {cap}')
               + '</div></section>')
sec.append('<section><h2>props (A / B)</h2><div class="row">' + ''.join(
    fig(f'../props/{n}_{o}.png', f'{n} {o}') for n in props_json for o in ORIS) + '</div></section>')
sec.append('<section><h2>props-only frames</h2><div class="row">' + ''.join(
    fig(f'../props/set_{s}_{o}.png', f'set_{s}_{o}') for s in SETS for o in ORIS) + '</div></section>')
sec.append('<section><h2>Zzz strip</h2><div class="row">' + fig('zzz_A.png', 'zzz_A') + fig('zzz_B.png', 'zzz_B') + '</div></section>')
html = ('<!doctype html><meta charset="utf-8"><title>Bear Stage B</title>\n'
        '<style>body{background:#1d1b24;color:#ece8f4;font:15px system-ui;padding:24px max(16px,4vw)}h1{font-size:20px}'
        'h2{font-size:15px;margin:22px 0 8px;font-family:ui-monospace,Menlo,monospace;color:#a9dccf}\n'
        '.row{display:flex;gap:12px;flex-wrap:wrap}figure{margin:0;background:#262331;border:1px solid #3a3548;'
        'border-radius:10px;padding:8px;text-align:center}\n'
        'img{image-rendering:pixelated;display:block;max-width:100%}.row img[src$=".png"]{width:auto;zoom:3}'
        'figcaption{font:12px ui-monospace,Menlo,monospace;color:#9a93ad}</style>\n'
        '<h1>Bear, Stage B: bed + reading set, sleep / read</h1>\n' + '\n'.join(sec) + '\n')
open(os.path.join(OUT, 'preview_b.html'), 'w').write(html)

# ------------------------------------------------------------------ checks
for p in [os.path.join(OUT, x) for x in os.listdir(OUT)] + [os.path.join(POUT, x) for x in os.listdir(POUT)]:
    if p.endswith('.png'): PX.check(np.asarray(Image.open(p).convert('RGBA')))
print('palette: all strips and prop sprites use only bear + moss + palette-ext colours')
for line in checks: print(line)
def ch(a, b): return int(np.any(a != b, -1).sum())
for (c, o), fs in sorted(FR.items()):
    steps = [ch(fs[i], fs[i + 1]) for i in range(len(fs) - 1)]
    seam = f', loop seam {ch(fs[-1], fs[0])}' if anims[c]['loop'] else ''
    print(f'changed px per step {c} {o}: max {max(steps)} median {int(np.median(steps))}{seam}')
for o in ORIS:
    for a_, b_ in (('get_in_bed', 'sleep'), ('sit_down', 'read')):
        print(f'chain {a_} -> {b_} {o}: {ch(FR[(a_, o)][-1], FR[(b_, o)][0])} px')
    for lp, nx in (('sleep', 'wake_up'), ('read', 'stand_up')):
        same = np.array_equal(FR[(lp, o)][0], FR[(nx, o)][0])
        print(f'{nx} {o} frame 0 identical to {lp} frame 0: {"yes" if same else "NO"}; '
              f'{lp} last -> {nx} 0: {ch(FR[(lp, o)][-1], FR[(nx, o)][0])} px')
print('ok', OUT, POUT)
