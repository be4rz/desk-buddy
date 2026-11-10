"""Assemble the Stage C clips (python3 + numpy + Pillow).

    python3 make_c.py PXDIR OUTDIR clips_c.json
PXDIR/big_<clip>/f<NN>_yaw_<deg>.png from pixelize_anim.py (8 directions, Stage A's 84x123 frame, see
render_anim.py --canvas). Writes to OUTDIR (../out/bear3d/anim_c):
  <clip>_<DIR>.png   strip per clip and direction, frames left -> right
  anim_c.json        exactly the Stage A schema of anim/anim.json (fps, ppu, directions, per clip loop, fps,
                     frame_w/h, anchor = idle's, frames_n, files, per frame dur + root_world + root_px)
  <clip>_<DIR>.gif   4x, S / SE / E, on #c8c4d2
  contact_c.png      every clip in S, SE, E, 2x (frames left -> right)
Exits 1, before writing anim_c.json, when a hand-off frame (frame 0, and the last frame of a one-shot) differs from
anim/idle_<DIR> frame 0.
"""
import sys, os, json
import numpy as np
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
PX, OUT, CJ = sys.argv[1], sys.argv[2], sys.argv[3]
A_DIR = os.path.join(HERE, '..', 'out', 'bear3d', 'anim')
os.makedirs(OUT, exist_ok=True)
C = json.load(open(CJ))
A = json.load(open(os.path.join(A_DIR, 'anim.json')))
IDLE = A['anims']['idle']
CLIPS = ['think', 'talk', 'ask', 'oops', 'happy', 'celebrate']
DIRS = ['S', 'SE', 'E', 'NE', 'N', 'NW', 'W', 'SW']
YAW = {d: A['directions'][d]['yaw_deg'] for d in DIRS}
FPS = C['fps']; DUR = round(1000 / FPS)
BG = (200, 196, 210, 255)
W, H = IDLE['frame_w'], IDLE['frame_h']


def fr(clip, f, d):
    return Image.open(os.path.join(PX, f'big_{clip}', f'f{f:02d}_yaw_{YAW[d]:06.2f}.png')).convert('RGBA')


def up(im, s): return im.resize((im.width * s, im.height * s), Image.NEAREST)


rest = {d: np.asarray(Image.open(os.path.join(A_DIR, IDLE['files'][d])).convert('RGBA'))[:, :W] for d in DIRS}
anims, bad = {}, []
for c in CLIPS:
    cc = C['clips'][c]; n = cc['n']
    assert fr(c, 0, 'S').size == (W, H), f'{c}: frame size {fr(c, 0, "S").size} != {(W, H)}'
    for d in DIRS:
        st = Image.new('RGBA', (W * n, H))
        for f in range(n): st.paste(fr(c, f, d), (f * W, 0))
        st.save(os.path.join(OUT, f'{c}_{d}.png'))
        for f in ([0] if cc['loop'] else [0, n - 1]):
            if not np.array_equal(np.asarray(fr(c, f, d)), rest[d]): bad.append(f'{c} {d} frame {f}')
    frames = []
    for f in range(n):
        dd = cc['root'][f + 1] - cc['root'][f]
        assert dd == 0, f'{c}: Stage C clips are in place'
        frames.append(dict(dur=DUR, root_world=0.0, root_px={d: [0.0, 0.0] for d in DIRS}))
    anims[c] = dict(loop=cc['loop'], fps=FPS, frame_w=W, frame_h=H, anchor=IDLE['anchor'], frames_n=n,
                    files={d: f'{c}_{d}.png' for d in DIRS}, frames=frames)

if bad:   # before anim_c.json exists: the pack's optional source then stays absent instead of loading bad art
    print('HAND-OFF MISMATCH (frame != anim/idle frame 0):', *bad, sep='\n  '); sys.exit(1)
json.dump(dict(
    note=('Stage C clips of the cute bear (think, talk, ask loop; oops, happy, celebrate one-shot): the Stage A schema, '
          'rig, camera, pixel step, frame size and anchor. All in place (root_px 0). Frame 0, and the last frame of a '
          'one-shot, is pixel-identical to anim/idle frame 0 in that direction (rest-pose hand-off).'),
    fps=FPS, ppu=A['ppu'], directions=A['directions'], anims=anims),
    open(os.path.join(OUT, 'anim_c.json'), 'w'), indent=1)


def gif(frames, path, s=4):
    out = []
    for f in frames:
        c = Image.new('RGBA', f.size, BG); c.alpha_composite(f)
        out.append(up(c.convert('RGB'), s).convert('P', palette=Image.ADAPTIVE, colors=64))
    out[0].save(path, save_all=True, append_images=out[1:], duration=DUR, loop=0, disposal=1)


for c in CLIPS:
    n = C['clips'][c]['n']
    for d in ('S', 'SE', 'E'):
        hold = [] if C['clips'][c]['loop'] else [fr(c, n - 1, d)] * 6   # one-shots: a beat of rest before repeating
        gif([fr(c, f, d) for f in range(n)] + hold, os.path.join(OUT, f'{c}_{d}.gif'))

S2, lab = 2, 14
rows = [(c, d) for c in CLIPS for d in ('S', 'SE', 'E')]
nmax = max(C['clips'][c]['n'] for c in CLIPS)
sheet = Image.new('RGBA', (70 + W * S2 * nmax, (H * S2 + lab) * len(rows)), BG); dr = ImageDraw.Draw(sheet)
for r, (c, d) in enumerate(rows):
    y0 = r * (H * S2 + lab); dr.text((4, y0 + 2), f'{c} {d}', fill=(40, 36, 48, 255))
    for f in range(C['clips'][c]['n']): sheet.alpha_composite(up(fr(c, f, d), S2), (70 + f * W * S2, y0 + lab))
sheet.save(os.path.join(OUT, 'contact_c.png'))

for c in CLIPS:
    n = C['clips'][c]['n']; a = [np.asarray(fr(c, f, 'S')) for f in range(n)]
    ch = [int((a[i] != a[(i + 1) % n]).any(-1).sum()) for i in range(n if C['clips'][c]['loop'] else n - 1)]
    print(f'changed px per step, {c} S:', ch)
print('hand-off frames identical to idle frame 0: yes')
print('ok', OUT)
