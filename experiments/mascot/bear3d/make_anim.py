"""Assemble the Stage A animation deliverables (python3 + numpy + Pillow).

    python3 make_anim.py PXDIR OUTDIR clips.json
PXDIR/big_<clip>/f<NN>_yaw_<deg>.png from pixelize_anim.py (all 8 directions), optional PXDIR/small_walk (S, SE).
Writes to OUTDIR:
  <clip>_<DIR>.png      strip per clip and direction, frames left -> right, all clips share one frame size
  walk_64_<DIR>.png     walk at ~64 px (S, SE) when PXDIR/small_walk exists
  anim.json             fps, loop, frame size, anchor, per frame duration + root motion (world + screen px per dir)
  <clip>_<DIR>.gif      4x, S/SE/E, on #c8c4d2
  walk_iso.gif          walk_start + walk x3 + walk_stop towards SE, then towards E, root motion applied over a 2:1 grid
  walk_contact.png      walk, all 8 directions x 12 frames, 2x
Prints the flicker checks: rest frames identical across clips, and changed pixels per frame step.
"""
import sys, os, json, math
import numpy as np
from PIL import Image, ImageDraw

PX, OUT, CJ = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(OUT, exist_ok=True)
C = json.load(open(CJ))
BG = (200, 196, 210, 255)
DIRS = ['S', 'SE', 'E', 'NE', 'N', 'NW', 'W', 'SW']
YAW = {d: (45 + 45 * i) % 360 for i, d in enumerate(DIRS)}
FPS = C['fps']; DUR = round(1000 / FPS)

def fr(tag, clip, f, yaw):
    return Image.open(os.path.join(PX, f'{tag}_{clip}', f'f{f:02d}_yaw_{yaw:06.2f}.png')).convert('RGBA')

def screen(meta, ppu, yaw, d):
    """forward distance d (world) -> screen px (dx, dy), y down."""
    t = math.radians(yaw); fwd = np.array([math.sin(t), -math.cos(t), 0.0])
    return [round(float(ppu * d * fwd @ np.array(meta['right'])), 4), round(float(-ppu * d * fwd @ np.array(meta['up'])), 4)]

def up(im, s): return im.resize((im.width * s, im.height * s), Image.NEAREST)

def write_set(tag, clips, dirs, prefix, out):
    meta = json.load(open(os.path.join(PX, f'{tag}_{clips[0]}', 'meta.json')))
    W, H, ppu = meta['W'], meta['H'], meta['ppu']
    for c in clips:
        n = C['clips'][c]['n']
        for d in dirs:
            st = Image.new('RGBA', (W * n, H))
            for f in range(n): st.paste(fr(tag, c, f, YAW[d]), (f * W, 0))
            st.save(os.path.join(OUT, f'{prefix}{c}_{d}.png'))
        root = C['clips'][c]['root']
        frames = []
        for f in range(n):
            dd = root[f + 1] - root[f]
            frames.append(dict(dur=DUR, root_world=round(dd, 6),
                               root_px={d: screen(meta, ppu, YAW[d], dd) for d in dirs}))
        out[prefix + c] = dict(loop=C['clips'][c]['loop'], fps=FPS, frame_w=W, frame_h=H, anchor=[W // 2, meta['foot_row']],
                               frames_n=n, files={d: f'{prefix}{c}_{d}.png' for d in dirs}, frames=frames)
    return meta

anims = {}
meta = write_set('big', C['order'], DIRS, '', anims)
has_small = os.path.isdir(os.path.join(PX, 'small_walk'))
if has_small:
    sm = {}
    m2 = write_set('small', ['walk'], ['S', 'SE'], '64_', sm)
    a = sm['64_walk']; a['files'] = {}
    for d in ('S', 'SE'):
        os.replace(os.path.join(OUT, f'64_walk_{d}.png'), os.path.join(OUT, f'walk_64_{d}.png')); a['files'][d] = f'walk_64_{d}.png'
    a['note'] = 'same walk at ~64 px; root px here are NOT whole pixels (speed is set for the 100 px size)'
    anims['walk_64'] = a
json.dump(dict(
    note=('Stage A clips of the cute bear. Iso 2:1, camera X60 Z45 ortho; directions are screen directions '
          '(S faces the viewer). Frames are in place: after showing frame k, move the sprite by frames[k].root_px[dir] '
          '(screen pixels, y down) = frames[k].root_world (world units along the facing direction). '
          'anchor = pixel of the floor point between the feet. Transitions start/end at the rest pose; '
          'walk_start -> walk -> walk_stop and run_start -> run -> run_stop chain.'),
    fps=FPS, ppu=meta['ppu'], directions={d: dict(yaw_deg=YAW[d]) for d in DIRS}, anims=anims),
    open(os.path.join(OUT, 'anim.json'), 'w'), indent=1)

# ------------------------------------------------------------------ GIFs (S, SE, E at 4x)
def gif(frames, path, s=4, dur=DUR):
    out = []
    for f in frames:
        c = Image.new('RGBA', f.size, BG); c.alpha_composite(f)
        out.append(up(c.convert('RGB'), s).convert('P', palette=Image.ADAPTIVE, colors=64))
    out[0].save(path, save_all=True, append_images=out[1:], duration=dur, loop=0, disposal=1)
for c in C['order']:
    for d in ('S', 'SE', 'E'):
        gif([fr('big', c, f, YAW[d]) for f in range(C['clips'][c]['n'])], os.path.join(OUT, f'{c}_{d}.gif'))

# ------------------------------------------------------------------ walk_iso.gif: root motion on a 2:1 floor grid
W, H, FOOT = meta['W'], meta['H'], meta['foot_row']; ax = W // 2
seq = []
for d in ('SE', 'E'):
    seq += [('walk_start', f, d) for f in range(C['clips']['walk_start']['n'])]
    seq += [('walk', f % 12, d) for f in range(36)]
    seq += [('walk_stop', f, d) for f in range(C['clips']['walk_stop']['n'])]
    seq += [('walk_stop', C['clips']['walk_stop']['n'] - 1, d)] * 4
pos = []; x = y = 0.0
for c, f, d in seq:
    pos.append((x, y))
    dx, dy = anims[c]['frames'][f]['root_px'][d]
    if not (c == 'walk_stop' and f == C['clips']['walk_stop']['n'] - 1): x += dx; y += dy
xs = [p[0] for p in pos]; ys = [p[1] for p in pos]
pad = 12
CW = int(max(xs) - min(xs)) + W + 2 * pad; CH = int(max(ys) - min(ys)) + H + 2 * pad
ox = pad - min(xs) + ax; oy = pad - min(ys) + FOOT          # canvas px of the world start point
grid = Image.new('RGBA', (CW, CH), BG); g = ImageDraw.Draw(grid)
R, U, ppu = np.array(meta['right']), np.array(meta['up']), meta['ppu']
def to_px(X, Y): return ox + ppu * (X * R[0] + Y * R[1]), oy - ppu * (X * U[0] + Y * U[1])
G = 0.5; span = 24
for k in range(-span, span + 1):
    for a, b in (((k * G, -span * G), (k * G, span * G)), ((-span * G, k * G), (span * G, k * G))):
        g.line([to_px(*a), to_px(*b)], fill=(189, 184, 202, 255), width=1)
frames = []
for (c, f, d), (px_, py_) in zip(seq, pos):
    im = grid.copy(); s = fr('big', c, f, YAW[d])
    im.alpha_composite(s, (int(round(ox + px_ - ax)), int(round(oy + py_ - FOOT))))
    frames.append(im)
gif(frames, os.path.join(OUT, 'walk_iso.gif'), s=3)

# ------------------------------------------------------------------ walk contact sheet
n = C['clips']['walk']['n']; S2 = 2; lab = 14
sheet = Image.new('RGBA', (W * S2 * n + 40, (H * S2 + lab) * 8), BG); dr = ImageDraw.Draw(sheet)
for r, d in enumerate(DIRS):
    y0 = r * (H * S2 + lab); dr.text((4, y0 + 2), d, fill=(40, 36, 48, 255))
    for f in range(n): sheet.alpha_composite(up(fr('big', 'walk', f, YAW[d]), S2), (40 + f * W * S2, y0 + lab))
sheet.save(os.path.join(OUT, 'walk_contact.png'))

# ------------------------------------------------------------------ flicker checks
rests = [('idle', 0), ('walk_start', 0), ('walk_stop', C['clips']['walk_stop']['n'] - 1), ('run_start', 0),
         ('run_stop', C['clips']['run_stop']['n'] - 1)]
bad = 0
for d in DIRS:
    ref = np.asarray(fr('big', *rests[0], YAW[d]))
    for c, f in rests[1:]:
        if not np.array_equal(ref, np.asarray(fr('big', c, f, YAW[d]))): bad += 1; print('REST MISMATCH', c, f, d)
print('rest frames identical across clips:', 'yes' if not bad else f'NO ({bad})')
for c in C['order']:
    n = C['clips'][c]['n']; a = [np.asarray(fr('big', c, f, YAW['S'])) for f in range(n)]
    ch = [int((a[i] != a[(i + 1) % n]).any(-1).sum()) for i in range(n if C['clips'][c]['loop'] else n - 1)]
    print(f'changed px per step, {c} S:', ch)
print('ok', OUT)
