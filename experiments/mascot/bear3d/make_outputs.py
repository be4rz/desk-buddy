"""Assemble the deliverables from pixelized frames (python3 + Pillow).

    python3 make_outputs.py PXDIR OUTDIR
PXDIR holds big_yaw_*.png (32 yaws, ~105 px) and small_yaw_*.png (8 yaws, ~64 px) from pixelize.py.
Writes to OUTDIR:
  bear3d_100_8dir.png / bear3d_64_8dir.png   strips, frames left->right S SE E NE N NW W SW, equal-size cells
  sheet.json                                  frame size, order, feet anchor per strip
  compare.png                                 reference | v01 | closest 3D angle, same pixel height, nearest-neighbour
  compare_v1_v2.png                           reference | round-1 model | round-2 model, same angle and scale
  turntable.gif                               32 angles, 4x, on #c8c4d2
  preview_8dir.png                            both strips at 4x on #c8c4d2 (for eyeballing)
"""
import sys, os, glob, json
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
MASCOT = os.path.abspath(os.path.join(HERE, '..'))
PX, OUT = sys.argv[1], sys.argv[2]
os.makedirs(OUT, exist_ok=True)
BG = (200, 196, 210, 255)
DIRS = ['S', 'SE', 'E', 'NE', 'N', 'NW', 'W', 'SW']
DIR_YAW = {d: (45 + 45 * i) % 360 for i, d in enumerate(DIRS)}
CLOSEST = 112.5   # between SE and E; the reference's 3/4 view

def frame(prefix, yaw): return Image.open(os.path.join(PX, f'{prefix}_yaw_{yaw:06.2f}.png')).convert('RGBA')

meta = {}
for prefix, label, raw in (('big', '100', 'big'), ('small', '64', 'small')):
    fr = [frame(prefix, DIR_YAW[d]) for d in DIRS]
    w, h = fr[0].size
    strip = Image.new('RGBA', (w * len(fr), h))
    for i, f in enumerate(fr): strip.paste(f, (i * w, 0))
    strip.save(os.path.join(OUT, f'bear3d_{label}_8dir.png'))
    rm = json.load(open(os.path.join(sys.argv[3] if len(sys.argv) > 3 else PX, raw, 'meta.json'))) if len(sys.argv) > 3 else {}
    boxes = [f.getbbox() for f in fr]
    meta[label] = dict(file=f'bear3d_{label}_8dir.png', frame_w=w, frame_h=h, order=DIRS,
                       yaw_deg={d: DIR_YAW[d] for d in DIRS},
                       anchor_feet=[w // 2, rm.get('foot_row')], sprite_height_px=[b[3] - b[1] for b in boxes])
json.dump(dict(note='Iso 2:1 (camera X60 Z45, ortho). Directions are screen directions: S faces the viewer, E faces screen right. '
                    'anchor_feet = pixel (x, y) of the world origin between the feet.', strips=meta),
          open(os.path.join(OUT, 'sheet.json'), 'w'), indent=1)

def up(im, s): return im.resize((im.width * s, im.height * s), Image.NEAREST)
def row(items, pad=16, bg=BG):
    H = max(i.height for i in items)
    c = Image.new('RGBA', (sum(i.width for i in items) + pad * (len(items) + 1), H + 2 * pad), bg)
    x = pad
    for i in items: c.alpha_composite(i, (x, pad + H - i.height)); x += i.width + pad
    return c

# comparison: ref (downscaled smoothly) | v01 | 3D, all at the same pixel height then x6
v01 = Image.open(os.path.join(MASCOT, 'out', 'v01.png')).convert('RGBA')
mine = frame('big', CLOSEST); mine = mine.crop(mine.getbbox())
S = 6
ref = Image.open(os.path.join(MASCOT, 'in', 'chatgpt-01.png')).convert('RGBA').crop((330, 50, 900, 1180))
ref = ref.resize((round(ref.width * v01.height * S / ref.height), v01.height * S), Image.LANCZOS)
row([ref, up(v01, S), up(mine, S)]).save(os.path.join(OUT, 'compare.png'))
# round 1 vs round 2 at the same angle and scale (round1_yaw112.5.png = the round-1 sprite, kept for this)
r1p = os.path.join(HERE, 'round1_yaw112.5.png')
if os.path.exists(r1p):
    r1 = Image.open(r1p).convert('RGBA')
    row([ref, up(r1, S), up(mine, S)]).save(os.path.join(OUT, 'compare_v1_v2.png'))
print('compare heights: v01', v01.height, '3D', mine.height)

# preview of both strips
big = [frame('big', DIR_YAW[d]) for d in DIRS]; small = [frame('small', DIR_YAW[d]) for d in DIRS]
a = row([up(f, 4) for f in big]); b = row([up(f, 4) for f in small])
pv = Image.new('RGBA', (max(a.width, b.width), a.height + b.height), BG); pv.paste(a, (0, 0)); pv.paste(b, (0, a.height))
pv.save(os.path.join(OUT, 'preview_8dir.png'))

# turntable: every big yaw, starting at S, turning toward SE
fs = sorted(glob.glob(os.path.join(PX, 'big_yaw_*.png')))
yaws = sorted([float(os.path.basename(f)[8:-4]) for f in fs], key=lambda y: (y - 45) % 360)
gif = []
for y in yaws:
    f = frame('big', y); c = Image.new('RGBA', f.size, BG); c.alpha_composite(f)
    gif.append(up(c.convert('RGB'), 4).convert('P', palette=Image.ADAPTIVE, colors=64))
gif[0].save(os.path.join(OUT, 'turntable.gif'), save_all=True, append_images=gif[1:], duration=110, loop=0, disposal=1)
print('ok', OUT, len(gif), 'turntable frames')
