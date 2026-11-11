"""Assemble the Stage L clips (python3 + numpy + Pillow).

    python3 make_l.py PXDIR OUTDIR clips_l.json
PXDIR/big_<action>/f<NN>_yaw_<deg>.png from run_l.sh (pixelize_anim.py of render_anim.py passes on the Stage L rig).
The listen clips have one action per head-yaw variant (anim_clips.LISTEN_DIR_VARIANT: S -> _0, SE/E -> _cw,
SW/W -> _ccw); the turns one action for all 8 directions. Writes to OUTDIR (../out/bear3d/anim_l):
  <clip>_<DIR>.png   strip per clip and direction (listen_in / listen / listen_out: S SE E W SW; turn_ccw / turn_cw:
                     all 8, keyed by the direction the turn starts from)
  anim_l.json        the Stage A schema of anim/anim.json (fps, ppu, directions, per clip loop, fps, frame_w/h,
                     anchor = idle's, frames_n, files, per frame dur + root_world + root_px) + turn_steps per clip
                     (+1 turn_ccw, -1 turn_cw, 0 listen; the direction order S SE E NE N NW W SW)
  <clip>_<DIR>.gif   listen (in, 2 loops, out) per face dir and a full 8-step turn_ccw / turn_cw spin, 4x
  sheet_listen.png   every listen and turn clip, all dirs, 3x, idle frame 0 beside each (for the user)
  preview_l.html     the GIFs and the sheet
Exits 1, before writing anim_l.json, when a hand-off frame differs: listen_in frame 0 and listen_out's last frame
must equal anim/idle_<DIR> frame 0; a turn's frame 0 = idle_<from> frame 0, its last = idle_<from +- 1> frame 0.
"""
import sys, os, json
import numpy as np
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from anim_clips import LISTEN_DIR_VARIANT

PX, OUT, CJ = sys.argv[1], sys.argv[2], sys.argv[3]
A_DIR = os.path.join(HERE, '..', 'out', 'bear3d', 'anim')
os.makedirs(OUT, exist_ok=True)
C = json.load(open(CJ))
A = json.load(open(os.path.join(A_DIR, 'anim.json')))
IDLE = A['anims']['idle']
DIRS = ['S', 'SE', 'E', 'NE', 'N', 'NW', 'W', 'SW']
FACE_DIRS = ['S', 'SE', 'E', 'W', 'SW']
LISTEN = ['listen_in', 'listen', 'listen_out']
TURNS = {'turn_ccw': 1, 'turn_cw': -1}
YAW = {d: A['directions'][d]['yaw_deg'] for d in DIRS}
FPS = C['fps']; DUR = round(1000 / FPS)
BG = (200, 196, 210, 255)
W, H = IDLE['frame_w'], IDLE['frame_h']


def action(clip, d): return f'{clip}_{LISTEN_DIR_VARIANT[d]}' if clip in LISTEN else clip


def fr(clip, f, d):
    return Image.open(os.path.join(PX, f'big_{action(clip, d)}', f'f{f:02d}_yaw_{YAW[d]:06.2f}.png')).convert('RGBA')


def nframes(clip): return C['clips'][action(clip, 'SE')]['n']


def step(d, k): return DIRS[(DIRS.index(d) + k) % 8]


def up(im, s): return im.resize((im.width * s, im.height * s), Image.NEAREST)


rest = {d: np.asarray(Image.open(os.path.join(A_DIR, IDLE['files'][d])).convert('RGBA'))[:, :W] for d in DIRS}
anims, bad = {}, []
for clip in LISTEN + list(TURNS):
    dirs = FACE_DIRS if clip in LISTEN else DIRS
    n = nframes(clip); loop = C['clips'][action(clip, 'SE')]['loop']
    for d in dirs:
        cc = C['clips'][action(clip, d)]
        assert cc['n'] == n and cc['loop'] == loop, f'{clip}: the variants differ in length or looping'
        assert fr(clip, 0, d).size == (W, H), f'{clip}: frame size {fr(clip, 0, d).size} != {(W, H)}'
        assert all(r == 0 for r in cc['root']), f'{clip}: Stage L clips are in place'
        st = Image.new('RGBA', (W * n, H))
        for f in range(n): st.paste(fr(clip, f, d), (f * W, 0))
        st.save(os.path.join(OUT, f'{clip}_{d}.png'))
        k = TURNS.get(clip, 0)
        checks = {'listen_in': [(0, d)], 'listen': [], 'listen_out': [(n - 1, d)]}.get(clip, [(0, d), (n - 1, step(d, k))])
        for f, want in checks:
            if not np.array_equal(np.asarray(fr(clip, f, d)), rest[want]): bad.append(f'{clip} {d} frame {f} != idle_{want} frame 0')
    anims[clip] = dict(loop=loop, fps=FPS, frame_w=W, frame_h=H, anchor=IDLE['anchor'], frames_n=n,
                       turn_steps=TURNS.get(clip, 0), files={d: f'{clip}_{d}.png' for d in dirs},
                       frames=[dict(dur=DUR, root_world=0.0, root_px={d: [0.0, 0.0] for d in dirs}) for _ in range(n)])

if bad:   # before anim_l.json exists: the pack's optional source then stays absent instead of loading bad art
    print('HAND-OFF MISMATCH:', *bad, sep='\n  '); sys.exit(1)
json.dump(dict(
    note=('Stage L clips of the cute bear: listen_in / listen (loop) / listen_out for the face dirs (the whole round '
          'head mass turns toward the camera, 45 deg, so the face patch dir follows the head: face.json patch_dirs) and '
          'turn_ccw / turn_cw for all 8 dirs, keyed by the starting dir (turn_steps +1 / -1 in the order '
          'S SE E NE N NW W SW). Stage A schema, rig, camera, pixel step, frame size and anchor; all in place. '
          'listen_in frame 0 and listen_out\'s last frame = anim/idle_<dir> frame 0; a turn starts on idle_<from> '
          'frame 0 and ends on idle_<from +- 1> frame 0.'),
    fps=FPS, ppu=A['ppu'], directions=A['directions'], anims=anims),
    open(os.path.join(OUT, 'anim_l.json'), 'w'), indent=1)


def gif(frames, path, s=4):
    out = []
    for f in frames:
        c = Image.new('RGBA', f.size, BG); c.alpha_composite(f)
        out.append(up(c.convert('RGB'), s).convert('P', palette=Image.ADAPTIVE, colors=64))
    out[0].save(path, save_all=True, append_images=out[1:], duration=DUR, loop=0, disposal=1)


def seq(clip, d): return [fr(clip, f, d) for f in range(nframes(clip))]


for d in FACE_DIRS:
    still = [Image.fromarray(rest[d])] * 8
    gif(still + seq('listen_in', d) + seq('listen', d) * 2 + seq('listen_out', d), os.path.join(OUT, f'listen_{d}.gif'))
for clip, k in TURNS.items():   # a full spin from S: 8 turns in a row, a beat of rest between them
    frames, d = [], 'S'
    for _ in range(8):
        frames += seq(clip, d) + [Image.fromarray(rest[step(d, k)])] * 3; d = step(d, k)
    gif(frames, os.path.join(OUT, f'{clip}_spin.gif'))

# ------------------------------------------------------------------ the sheet for the user
K, lab, gap = 3, 16, 12
rows = [('listen_in + listen + listen_out', d, ['listen_in', 'listen', 'listen_out']) for d in FACE_DIRS]
rows += [('turn_ccw | turn_cw', d, ['turn_ccw', 'turn_cw']) for d in DIRS]
cols = max(sum(nframes(c) for c in cl) for _, _, cl in rows)
sw = 110 + W * K * (1 + cols) + gap * 4
sheet = Image.new('RGBA', (sw, (H * K + lab) * len(rows)), BG); dr = ImageDraw.Draw(sheet)
ink = (40, 36, 48, 255)
for r, (title, d, cl) in enumerate(rows):
    y0 = r * (H * K + lab)
    dr.text((4, y0 + lab + 4), f'{d}\n{title.replace(" + ", chr(10) + "+ ").replace(" | ", chr(10) + "| ")}', fill=ink)
    dr.text((110 + 4, y0 + 2), f'idle_{d} f0', fill=ink)
    sheet.alpha_composite(up(Image.fromarray(rest[d]), K), (110, y0 + lab))
    x = 110 + W * K + gap
    for c in cl:
        dr.text((x + 4, y0 + 2), c if c not in TURNS else f'{c} (ends {step(d, TURNS[c])})', fill=ink)
        for f in range(nframes(c)):
            sheet.alpha_composite(up(fr(c, f, d), K), (x, y0 + lab)); x += W * K
        x += gap
sheet.save(os.path.join(OUT, 'sheet_listen.png'))


def fig(src, cap): return f'<figure><img src="{src}" alt="{cap}"><figcaption>{cap}</figcaption></figure>'


page = ['<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">',
        '<title>Bear Stage L</title><link rel="icon" href="data:,">',
        '<style>body{background:#1d1b24;color:#ece8f4;font:15px system-ui;padding:24px max(16px,4vw)}h1{font-size:20px}'
        'h2{font-size:15px;margin:22px 0 8px;font-family:ui-monospace,Menlo,monospace;color:#a9dccf}'
        '.row{display:flex;gap:12px;flex-wrap:wrap}figure{margin:0;background:#262331;border:1px solid #3a3548;'
        'border-radius:10px;padding:8px;text-align:center;max-width:100%}'
        'img{image-rendering:pixelated;display:block;max-width:100%;height:auto}'
        'figcaption{font:12px ui-monospace,Menlo,monospace;color:#9a93ad}p{color:#9a93ad;max-width:70ch}</style>',
        '<h1>Bear, Stage L: listen and turns</h1>',
        '<p>Listen: the whole round head turns toward you (45 deg) and holds, breathing, until you send or dismiss the '
        'prompt, then turns back. Each GIF: idle, listen_in, listen twice, listen_out. Turns: 45 deg steps; the spins '
        'chain all 8 from S.</p>',
        '<section><h2>listen (S, SE, E, W, SW)</h2><div class="row">'
        + ''.join(fig(f'listen_{d}.gif', d) for d in FACE_DIRS) + '</div></section>',
        '<section><h2>turns</h2><div class="row">' + fig('turn_ccw_spin.gif', 'turn_ccw x8') + fig('turn_cw_spin.gif', 'turn_cw x8')
        + '</div></section>',
        '<section><h2>every clip and direction, 3x, idle frame 0 first</h2>' + fig('sheet_listen.png', 'sheet_listen.png') + '</section>']
open(os.path.join(OUT, 'preview_l.html'), 'w').write('\n'.join(page) + '\n')

for clip in LISTEN:
    a = [np.asarray(fr(clip, f, 'SE')) for f in range(nframes(clip))]
    print(f'changed px per step, {clip} SE:', [int((a[i] != a[i + 1]).any(-1).sum()) for i in range(len(a) - 1)])
print('hand-off frames identical to idle frame 0: yes')
print('ok', OUT)
