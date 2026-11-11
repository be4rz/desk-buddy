"""Assemble the Stage D deliverables (python3 + numpy + Pillow).

    python3 make_d.py PXROOT ANIM_OUT PROPS_OUT
PXROOT/desk_<ori>/ from render_d.py + pixelize_d.py: layout.json, props.json, clip_<name>.json and the pixel layers
(<key>.png + <key>_d.npy). The Stage A rest sprites come from ../out/bear3d/anim (idle_<DIR>.png frame 0).

Frames: every layer (bear, desk_table, chair, computer) is composited per pixel by depth (make_b.py's method). The
hand-off frames (desk_sit frame 0, desk_stand's last) are built as props-only frame + Stage A rest sprite pasted at the
hand-off pixel, so they match the runtime's free bear + prop sprites exactly. Exits 1 if a prop hides a bear pixel there,
or if the fresh render of a hand-off frame differs from that by more than HANDOFF_NOISE px (a few shade-step flips are
render noise, as in Stage B; the ori B bear is a real render too: render_d.py mirrors its pose, not its mesh).

ANIM_OUT:  <clip>_<ori>.png strips, <clip>_<ori>.gif (4x), seq_desk_<ori>.gif, sheet_desk.png (every clip, both oris,
           3x), anim_d.json (the stage_b format), preview_d.html
PROPS_OUT: desk_<ori>.png (props-only frame, clip canvas), desk_table_<ori>.png / chair_<ori>.png / computer_<ori>.png
           (cropped), props_d.json
"""
import sys, os, json
import numpy as np
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import pixelize_d as PXD
import clips_d

PXR, OUT, POUT = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(OUT, exist_ok=True); os.makedirs(POUT, exist_ok=True)
A_DIR = os.path.join(HERE, '..', 'out', 'bear3d', 'anim')
AJ = json.load(open(os.path.join(A_DIR, 'anim.json')))
A_ANCHOR = AJ['anims']['idle']['anchor']; A_FW = AJ['anims']['idle']['frame_w']
YAW2DIR = {v['yaw_deg'] % 360: d for d, v in AJ['directions'].items()}
BG = (200, 196, 210, 255); FPS = clips_d.FPS; DUR = round(1000 / FPS)
ORIS = ['A', 'B']; SET = 'desk'
HANDOFF_NOISE = 8      # px a fresh rest render may differ from the Stage A sprite (shade-step flips; Stage B: 1-4)
ORI_NOTE = {'A': 'desk long axis along world X, chair on its SW side; the bear types facing NE, talks with body SE '
                 'and head S',
            'B': 'mirror of A: chair on the desk\'s SE side; the bear types facing NW, talks with body SW and head S'}
NEXT = {'desk_sit': 'desk_type', 'desk_type': 'desk_type, desk_swivel_in or desk_stand via desk_swivel_in',
        'desk_swivel_in': 'desk_talk or desk_ask', 'desk_talk': 'desk_talk, desk_ask, desk_swivel_out or desk_stand',
        'desk_ask': 'desk_ask, desk_talk, desk_swivel_out or desk_stand', 'desk_swivel_out': 'desk_type',
        'desk_stand': 'Stage A idle (rest pose, same spot and facing)'}

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
    out = base.copy(); H, W = out.shape[:2]; h, w = spr.shape[:2]
    xs, ys = max(0, -x0), max(0, -y0); xe, ye = min(w, W - x0), min(h, H - y0)
    s = spr[ys:ye, xs:xe]; m = s[..., 3] > 0
    reg = out[y0 + ys:y0 + ye, x0 + xs:x0 + xe]; reg[m] = s[m]
    return out

def rest_sprite(yaw):
    d = YAW2DIR[round(yaw) % 360]
    im = np.asarray(Image.open(os.path.join(A_DIR, f'idle_{d}.png')).convert('RGBA'))
    return d, im[:, :A_FW].copy()

def up(im, s): return im.resize((im.width * s, im.height * s), Image.NEAREST)
def gif(frames, path, s=4, durs=None):
    out = []
    for f in frames:
        c = Image.new('RGBA', (f.shape[1], f.shape[0]), BG); c.alpha_composite(Image.fromarray(f))
        out.append(up(c.convert('RGB'), s).convert('P', palette=Image.ADAPTIVE, colors=64))
    out[0].save(path, save_all=True, append_images=out[1:], duration=durs or DUR, loop=0, disposal=1)

anims, sets_json, props_json, checks, fail = {}, {}, {}, [], []
FR = {}; LAY = {}
for o in ORIS:
    d = os.path.join(PXR, f'{SET}_{o}'); lay = json.load(open(os.path.join(d, 'layout.json'))); LAY[o] = lay
    W, H = lay['W'], lay['H']; OX, OY = lay['origin']
    if W > 176 or H > 160: fail.append(f'canvas {o} {W}x{H} exceeds 176x160')
    pk = json.load(open(os.path.join(d, 'props.json')))
    props_frame = composite(d, list(pk.values()))
    Image.fromarray(props_frame).save(os.path.join(POUT, f'desk_{o}.png'))
    sets_json[SET] = sets_json.get(SET, {})
    sets_json[SET][o] = dict(frame_w=W, frame_h=H, anchor=[OX, OY], props_frame=f'../props_d/desk_{o}.png',
                             orientation=ORI_NOTE[o], seat_px=lay['seat_px'])
    for name, key in pk.items():
        a = layer(d, key)[0]; ys, xs = np.nonzero(a[..., 3])
        x0, x1, y0, y1 = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
        Image.fromarray(a[y0:y1, x0:x1]).save(os.path.join(POUT, f'{name}_{o}.png'))
        ppx = lay['prop_px'][name]
        props_json.setdefault(name, {})[o] = dict(file=f'{name}_{o}.png', size=[int(x1 - x0), int(y1 - y0)],
                                                 anchor=[int(OX + ppx[0] - x0), int(OY + ppx[1] - y0)],
                                                 offset_px_in_set=ppx, layer_key=key)
    for c in clips_d.ORDER:
        C = json.load(open(os.path.join(d, f'clip_{c}.json')))
        frames = [composite(d, list(fr['keys'].values())) for fr in C['frames']]
        h = lay['handoff'].get(c); ho = None
        if h:
            dname, spr = rest_sprite(h['yaw'])
            x0, y0 = OX + h['px'][0] - A_ANCHOR[0], OY + h['px'][1] - A_ANCHOR[1]
            hf = paste_over(props_frame, spr, x0, y0)
            i = 0 if h['at'] == 'start' else len(frames) - 1
            _, owner = composite(d, list(C['frames'][i]['keys'].values()), want_owner=True)
            bear_lost = int(((layer(d, C['frames'][i]['keys']['bear'])[0][..., 3] > 0) & (owner != 0)).sum())
            diff = int(np.any(frames[i] != hf, -1).sum())
            checks.append(f'{c} {o}: hand-off frame {i} = props + Stage A idle_{dname} f0; the fresh render differs in '
                          f'{diff} px (shade-step flips, as Stage B); bear pixels hidden by props there: {bear_lost}')
            if bear_lost: fail.append(f'{c} {o}: {bear_lost} bear pixels hidden by props at the hand-off spot')
            if diff > HANDOFF_NOISE: fail.append(f'{c} {o}: the fresh hand-off render differs from props + idle_{dname}#0 '
                                                 f'in {diff} px (> {HANDOFF_NOISE}: not render noise)')
            frames[i] = hf
            ho = dict(at=h['at'], frame=i, pos_world=[round(v, 4) for v in h['pos']], px=h['px'],
                      facing=dname, yaw_deg=h['yaw'], stage_a_sprite=f'../anim/idle_{dname}.png#0')
        FR[(c, o)] = frames
        n = len(frames)
        strip = np.zeros((H, W * n, 4), np.uint8)
        for f, im in enumerate(frames): strip[:, f * W:(f + 1) * W] = im
        Image.fromarray(strip).save(os.path.join(OUT, f'{c}_{o}.png'))
        gif(frames, os.path.join(OUT, f'{c}_{o}.gif'))
        a = anims.setdefault(c, dict(set=SET, loop=C['loop'], fps=FPS, frames_n=n, files={}, handoff={},
                                     frames=[dict(dur=DUR) for _ in C['frames']], next=NEXT[c]))
        if c in clips_d.FACE_CLIPS: a['face_patch_dir'] = clips_d.PATCH_DIR
        a['files'][o] = f'{c}_{o}.png'
        if ho: a['handoff'][o] = ho
for c, a in anims.items():
    if not a['handoff']: a.pop('handoff')

for o in ORIS:
    seq = [('desk_sit', 1), ('desk_type', 2), ('desk_swivel_in', 1), ('desk_talk', 1), ('desk_ask', 1),
           ('desk_swivel_out', 1), ('desk_type', 1), ('desk_swivel_in', 1), ('desk_stand', 1)]
    fs = []
    for c, r in seq: fs += FR[(c, o)] * r
    gif(fs, os.path.join(OUT, f'seq_desk_{o}.gif'), durs=[500] + [DUR] * (len(fs) - 2) + [700])

# ------------------------------------------------------------------ JSON
json.dump(dict(
    note=('Stage D desk station clips (desk_sit -> desk_type (loop) -> desk_swivel_in -> desk_talk / desk_ask (loop) '
          '-> desk_swivel_out -> desk_type, or desk_stand from the talk pose). Stage B format: each frame is the WHOLE '
          'set (desk, computer, chair + bear) baked on one canvas; anchor = pixel of the set origin (the desk footprint '
          'centre on the floor). ori A is the default placement, ori B its true 3D mirror (light unchanged, not a '
          'sprite flip; the bear in it is the real bear in the mirrored pose). Hand-off: desk_sit frame 0 and '
          'desk_stand\'s last frame are pixel-identical to the props-only frame + the Stage A idle sprite (frame 0) at '
          'handoff.px from the anchor, facing handoff.facing. Joins (identical frames): desk_sit last = desk_type 0 = '
          'desk_swivel_in 0 = desk_swivel_out last; desk_swivel_in last = desk_talk 0 = desk_ask 0 = desk_swivel_out 0 '
          '= desk_stand 0. desk_talk / desk_ask: body SE (A) / SW (B), head turned to S: face layers from face.json '
          'set_clips (patch dir in face_patch_dir and there).'),
    sorting=('As Stage B: outside these clips draw the props (props_d/*.png or desk_<ori>.png) and the free bear as '
             'separate sprites sorted by floor depth; from the first to the last frame of a desk clip hide them and '
             'draw the baked frame at the set anchor. At the hand-off spot the bear is in front of every prop pixel.'),
    fps=FPS, ppu=AJ['ppu'], sets=sets_json, anims=anims),
    open(os.path.join(OUT, 'anim_d.json'), 'w'), indent=1)

geo = LAY['A']['geom']
FOOT = {'desk_table': dict(x=[-geo['desk_half'][0], geo['desk_half'][0]], y=[-geo['desk_half'][1], geo['desk_half'][1]]),
        'computer': dict(on='desk_table', x=[-0.6, 0.6], y=[-0.42, 0.4]),
        'chair': dict(x=[-0.5, 0.5], y=[-0.5, 0.78], note='swivels about its origin; home facing SE (A) / SW (B)')}
for name in props_json:
    props_json[name]['footprint_world'] = dict(FOOT[name], frame='prop-local, ori A; ori B: (x, y) -> (-y, -x)')
    props_json[name]['origin_world_in_set'] = [round(v, 4) for v in LAY['A']['prop_pos'][name]]
    props_json[name]['set'] = SET
json.dump(dict(
    note=('Stage D props (render_d.py, chunky superellipsoid toy shapes): a low toy desk, a chubby rounded monitor + '
          'keyboard, a round swivel chair, pre-rendered with the Stage A camera and pixel step. Each prop sprite = that '
          'prop rendered alone with its own ink outline; anchor = pixel of the prop origin (desk_table: floor point at '
          'the footprint centre; computer: the desk top above it; chair: its swivel axis on the floor). desk_<ori>.png '
          '= the props-only frame on the clip canvas (chair at home, screen tone 0). Colours: palette-bear.json + moss '
          '+ palette-ext.json (wood, fabric_rose, toy_plastic, screen).'),
    ppu=AJ['ppu'], pass_ids=LAY['A']['pass_ids'], props=props_json),
    open(os.path.join(POUT, 'props_d.json'), 'w'), indent=1)

# ------------------------------------------------------------------ sheet_desk.png: every clip, both oris, 3x
LBL = 12
rows = []
for c in clips_d.ORDER:
    for o in ORIS:
        fs = FR[(c, o)]; W, H = fs[0].shape[1], fs[0].shape[0]
        r = Image.new('RGBA', (W * 20, H + LBL), BG)
        ImageDraw.Draw(r).text((2, 0), f'{c} {o} ({len(fs)}{" loop" if anims[c]["loop"] else ""})', fill=(40, 34, 52, 255))
        for i, f in enumerate(fs): r.alpha_composite(Image.fromarray(f), (i * W, LBL))
        rows.append(r)
sheet = Image.new('RGBA', (max(r.width for r in rows), sum(r.height for r in rows)), BG)
y = 0
for r in rows: sheet.alpha_composite(r, (0, y)); y += r.height
up(sheet.convert('RGB'), 3).save(os.path.join(OUT, 'sheet_desk.png'))

# ------------------------------------------------------------------ preview
def fig(src, cap): return f'<figure><img src="{src}"><figcaption>{cap}</figcaption></figure>'
sec = ['<section><h2>sequences</h2><div class="row">' + fig('seq_desk_A.gif', 'A: sit, type, swivel in, talk, ask, '
       'swivel out, type, swivel in, stand') + fig('seq_desk_B.gif', 'B (mirror)') + '</div></section>']
for c in clips_d.ORDER:
    a = anims[c]; cap = f"{a['frames_n']} frames{' loop' if a['loop'] else ''}"
    sec.append(f'<section><h2>{c}</h2><div class="row">' + fig(f'{c}_A.gif', f'A, {cap}')
               + fig(f'{c}_B.gif', f'B (mirror), {cap}') + '</div></section>')
sec.append('<section><h2>props (A / B)</h2><div class="row">' + ''.join(
    fig(f'../props_d/{n}_{o}.png', f'{n} {o}') for n in props_json for o in ORIS) + '</div></section>')
sec.append('<section><h2>props-only frames</h2><div class="row">' + ''.join(
    fig(f'../props_d/desk_{o}.png', f'desk_{o}') for o in ORIS) + '</div></section>')
sec.append('<section><h2>sheet (3x)</h2><img src="sheet_desk.png" style="zoom:.5"></section>')
html = ('<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">'
        '<title>Bear Stage D</title>\n'
        '<style>body{background:#1d1b24;color:#ece8f4;font:15px system-ui;padding:24px max(16px,4vw)}h1{font-size:20px}'
        'h2{font-size:15px;margin:22px 0 8px;font-family:ui-monospace,Menlo,monospace;color:#a9dccf}\n'
        '.row{display:flex;gap:12px;flex-wrap:wrap}figure{margin:0;background:#262331;border:1px solid #3a3548;'
        'border-radius:10px;padding:8px;text-align:center}\n'
        'img{image-rendering:pixelated;display:block;max-width:100%}.row img[src$=".png"]{width:auto;zoom:3}'
        'figcaption{font:12px ui-monospace,Menlo,monospace;color:#9a93ad}</style>\n'
        '<h1>Bear, Stage D: the desk station</h1>\n' + '\n'.join(sec) + '\n')
open(os.path.join(OUT, 'preview_d.html'), 'w').write(html)

# ------------------------------------------------------------------ checks
for p in [os.path.join(OUT, x) for x in os.listdir(OUT)] + [os.path.join(POUT, x) for x in os.listdir(POUT)]:
    if p.endswith('.png') and not p.endswith('sheet_desk.png'): PXD.PX.check(np.asarray(Image.open(p).convert('RGBA')))
print('palette: all strips and prop sprites use only bear + moss + palette-ext colours')
for line in checks: print(line)
def ch(a, b): return int(np.any(a != b, -1).sum())
for (c, o), fs in sorted(FR.items()):
    steps = [ch(fs[i], fs[i + 1]) for i in range(len(fs) - 1)]
    seam = f', loop seam {ch(fs[-1], fs[0])}' if anims[c]['loop'] else ''
    print(f'changed px per step {c} {o}: max {max(steps)} median {int(np.median(steps))}{seam}')
for o in ORIS: print(f'canvas {o}: {LAY[o]["W"]}x{LAY[o]["H"]}')
if fail:
    for f in fail: print('FAIL', f)
    sys.exit(1)
print('ok', OUT, POUT)
