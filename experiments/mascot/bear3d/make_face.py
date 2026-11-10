"""Assemble the D16 face layers (python3 + numpy + Pillow).

    python3 make_face.py PXDIR OUTDIR clips_c.json
PXDIR/<tag>_<clip>/f<NN>_yaw_<deg>.png from run_face.sh (pixelize_anim.py of render_anim.py passes on
bear_rig_c.blend), for the face directions S SE E W SW only:
  rest_face_rest          the rest pose (== anim/idle frame 0)
  closed_face_eyes_closed rest pose, eyes shut          open_face_rest / wide_face_rest   rest pose, mouth open / wide
  ref_idle_ref            the Stage A idle poses with the eyes and mouth carried rigidly and no baked blink
Every pixel comes from those Blender renders: a patch is a cut-out of one, nothing is drawn.

Per slot (eyes, mouth) and direction, the opaque mask is the union of the pixels where any state render differs from
the rest render, plus the pixels where the idle reference differs from the shipped idle (moved into rest-pose
coordinates by the frame's face shift), grown by 1 px and kept inside the rest silhouette. Every state's patch is its
render cut by that same mask (alpha 0 outside it), so all states of one slot+dir share size and mask, and the default
patch (eyes open, mouth closed) is the rest render itself.

Positions: the top-left of the patch per body frame = its rest-pose top-left shifted up by the rig's whole-pixel face
shift (clips_c.json face_px, written by rig_bear.py --stage c). Covered: idle (shift of idle_ref) and the six Stage C
clips. Not covered (no face drawn): walk*/run* (head roll/lean), walk_64, N/NE/NW, the Stage B sets.

Writes OUTDIR (../out/bear3d/face): face.json, <slot>_<state>_<DIR>.png, ref/idle_<DIR>.png, and for the human
states_contact.png (each state over idle frame 0, S SE E SW W) and idle_compare.png (body | body+default | ref, S SE E).
"""
import sys, os, json
import numpy as np
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
PX, OUT, CJ = sys.argv[1], sys.argv[2], sys.argv[3]
ART = os.path.join(HERE, '..', 'out', 'bear3d')
C = json.load(open(CJ))
A = json.load(open(os.path.join(ART, 'anim', 'anim.json')))
AC = json.load(open(os.path.join(ART, 'anim_c', 'anim_c.json')))
FACE_DIRS = ['S', 'SE', 'E', 'W', 'SW']
YAW = {d: A['directions'][d]['yaw_deg'] for d in FACE_DIRS}
SLOTS = {'eyes': ['open', 'closed'], 'mouth': ['closed', 'open', 'wide']}
DEFAULT = {'eyes': 'open', 'mouth': 'closed'}
STATE_TAG = {('eyes', 'closed'): 'closed_face_eyes_closed', ('mouth', 'open'): 'open_face_rest',
             ('mouth', 'wide'): 'wide_face_rest'}
C_CLIPS = ['think', 'talk', 'ask', 'oops', 'happy', 'celebrate']
BG = (200, 196, 210, 255)
os.makedirs(os.path.join(OUT, 'ref'), exist_ok=True)


def px(tag, d, f=0):
    return np.asarray(Image.open(os.path.join(PX, tag, f'f{f:02d}_yaw_{YAW[d]:06.2f}.png')).convert('RGBA'))


def strip(path, w):
    a = np.asarray(Image.open(path).convert('RGBA'))
    return [a[:, i * w:(i + 1) * w] for i in range(a.shape[1] // w)]


def diff(a, b): return (a != b).any(-1)


def grow(m, r=1):
    out = m.copy()
    for _ in range(r):
        g = out.copy()
        g[1:] |= out[:-1]; g[:-1] |= out[1:]; g[:, 1:] |= out[:, :-1]; g[:, :-1] |= out[:, 1:]
        out = g
    return out


W, H = A['anims']['idle']['frame_w'], A['anims']['idle']['frame_h']
shift = {c: C['clips'][c]['face_px'] for c in C_CLIPS}
shift['idle'] = C['clips']['idle_ref']['face_px']
layers, positions, errors, edge = {s: {st: {} for st in SLOTS[s]} for s in SLOTS}, {}, [], []
for d in FACE_DIRS:
    rest = px('rest_face_rest', d)
    idle = strip(os.path.join(ART, 'anim', A['anims']['idle']['files'][d]), W)
    if not np.array_equal(rest, idle[0]): errors.append(f'{d}: the rest render differs from anim/idle frame 0')
    ref = [px('ref_idle_ref', d, f) for f in range(len(idle))]
    Image.fromarray(np.concatenate(ref, 1)).save(os.path.join(OUT, 'ref', f'idle_{d}.png'))
    seed = {s: np.zeros((H, W), bool) for s in SLOTS}
    for (s, st), tag in STATE_TAG.items(): seed[s] |= diff(px(tag, d), rest)
    if (seed['eyes'] & seed['mouth']).any(): errors.append(f'{d}: a pixel changes with both the eyes and the mouth')
    zone = {s: grow(seed[s], 3) for s in SLOTS}
    def near(s, y, x):   # distance from (y, x) to the nearest seed pixel of slot s
        sy, sx = np.nonzero(seed[s]); return ((sy - y) ** 2 + (sx - x) ** 2).min()
    # where the shipped idle differs from the reference, in rest coordinates of each slot
    for f in range(len(idle)):
        dm = diff(ref[f], idle[f])
        for y, x in zip(*np.nonzero(dm)):
            hit = [s for s in SLOTS if 0 <= y + shift['idle'][s][f] < H and zone[s][y + shift['idle'][s][f], x]]
            if not hit: errors.append(f'{d}: idle frame {f} differs from its reference at ({x},{y}), outside any face slot'); continue
            s = min(hit, key=lambda s: near(s, y + shift['idle'][s][f], x))
            seed[s][y + shift['idle'][s][f], x] = True
    if (seed['eyes'] & seed['mouth']).any(): errors.append(f'{d}: the idle reference puts a pixel in both the eyes and the mouth')
    # one mask per slot: its seed plus a 1 px ring, opaque in the rest render and in every state render (in profile a
    # decal can touch the silhouette, and an opaque patch cannot erase a body pixel); ring pixels both slots claim are
    # dropped, so the two patches never overlap
    opaque = rest[..., 3] == 255
    for tag in STATE_TAG.values(): opaque &= px(tag, d)[..., 3] == 255
    ring = {s: grow(seed[s], 1) & opaque for s in SLOTS}
    both = ring['eyes'] & ring['mouth']
    for s in SLOTS:
        mask = ring[s] & ~(both & ~seed[s])
        lost = seed[s] & ~mask
        for (s2, st), tag in STATE_TAG.items():
            if s2 == s and (lost & diff(px(tag, d), rest)).any():
                n = int((lost & diff(px(tag, d), rest)).sum())
                edge.append(f'{d} {s} {st}: {n} px on the silhouette edge stay as in the rest render (transparent in that state)')
        ys, xs = np.nonzero(mask); y0, y1, x0, x1 = ys.min(), ys.max() + 1, xs.min(), xs.max() + 1
        m = mask[y0:y1, x0:x1]
        for st in SLOTS[s]:
            src = rest if st == DEFAULT[s] else px(STATE_TAG[(s, st)], d)
            p = src[y0:y1, x0:x1].copy()
            if (p[..., 3][m] != 255).any(): errors.append(f'{d}: {s} {st}: transparent pixel inside the mask')
            p[~m] = 0
            name = f'{s}_{st}_{d}.png'
            Image.fromarray(p).save(os.path.join(OUT, name))
            layers[s][st][d] = dict(file=name, w=int(x1 - x0), h=int(y1 - y0))
        for c in ['idle'] + C_CLIPS:
            positions.setdefault(c, {}).setdefault(d, {})[s] = [[int(x0), int(y0) - k] for k in shift[c][s]]
        print(f'{d} {s}: patch {x1 - x0}x{y1 - y0} at ({x0},{y0}), {int(m.sum())} px')

json.dump(dict(
    face_version=1,
    note=('D16 face layers (rendered by run_face.sh, checked by verify_face.py): opaque patches over the body\'s baked '
          'default face, drawn at the per-frame positions (top-left, frame pixels). Clip/dirs absent here draw no face.'),
    dirs=FACE_DIRS,
    slots={s: dict(states=SLOTS[s], default=DEFAULT[s]) for s in SLOTS},
    layers=layers, clips=positions), open(os.path.join(OUT, 'face.json'), 'w'), indent=1)


# ------------------------------------------------------------------ contact sheets for the human (no new pixels)
def patch(s, st, d): return Image.open(os.path.join(OUT, layers[s][st][d]['file'])).convert('RGBA')


def compose(body, c, d, f, eyes='open', mouth='closed'):
    im = Image.fromarray(body).copy()
    for s, st in (('eyes', eyes), ('mouth', mouth)):
        im.alpha_composite(patch(s, st, d), tuple(positions[c][d][s][f]))
    return im


def up(im, k): return im.resize((im.width * k, im.height * k), Image.NEAREST)


COMBOS = [('open', 'closed'), ('closed', 'closed'), ('open', 'open'), ('open', 'wide'), ('closed', 'wide')]
K, lab = 4, 16
sheet = Image.new('RGBA', (W * K * len(COMBOS), (H * K + lab) * 5), BG); dr = ImageDraw.Draw(sheet)
for r, d in enumerate(['S', 'SE', 'E', 'SW', 'W']):
    body = strip(os.path.join(ART, 'anim', A['anims']['idle']['files'][d]), W)[0]
    for i, (e, m) in enumerate(COMBOS):
        y0 = r * (H * K + lab); dr.text((i * W * K + 4, y0 + 2), f'{d}  eyes {e} / mouth {m}', fill=(40, 36, 48, 255))
        sheet.alpha_composite(up(compose(body, 'idle', d, 0, e, m), K), (i * W * K, y0 + lab))
sheet.save(os.path.join(OUT, 'states_contact.png'))

FR = [0, 6, 9, 12, 18, 21, 22, 23]
K = 3
sheet = Image.new('RGBA', (90 + W * K * len(FR), (H * K + lab) * 9), BG); dr = ImageDraw.Draw(sheet)
for r, d in enumerate(['S', 'SE', 'E']):
    body = strip(os.path.join(ART, 'anim', A['anims']['idle']['files'][d]), W)
    ref = strip(os.path.join(OUT, 'ref', f'idle_{d}.png'), W)
    for k, (lbl, get) in enumerate((('body', lambda f: Image.fromarray(body[f])),
                                    ('body+default', lambda f: compose(body[f], 'idle', d, f)),
                                    ('ref', lambda f: Image.fromarray(ref[f])))):
        y0 = (3 * r + k) * (H * K + lab); dr.text((4, y0 + lab + 4), f'{d}\n{lbl}', fill=(40, 36, 48, 255))
        for i, f in enumerate(FR):
            dr.text((90 + i * W * K + 4, y0 + 2), f'idle f{f}', fill=(40, 36, 48, 255))
            sheet.alpha_composite(up(get(f), K), (90 + i * W * K, y0 + lab))
sheet.save(os.path.join(OUT, 'idle_compare.png'))

if edge: print('silhouette-edge pixels left to the body (a state render differs there):', *edge, sep='\n  ')
if errors:
    print('FACE ERRORS:', *errors[:40], sep='\n  '); sys.exit(1)
print('ok', OUT)
