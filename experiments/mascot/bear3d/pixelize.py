"""Render passes -> palette pixel art (system python3 + numpy + Pillow).

    python3 pixelize.py RAW/<name> OUTDIR [--prefix big]

Per yaw (RAW/<name>/yaw_*.npz from render_bear.py):
  1. K x K supersample cells -> one part id per output pixel: thin/decal parts (eyes, nose, brows, mouth,
     stem, hat face, stitches, blush, clover) win the cell once they cover a set fraction of it; otherwise the
     majority part wins; the cell is empty when most of it is background (unless a thin part claimed it).
  2. Toon shade: half-lambert with a light fixed to the camera (front, above, a bit right) times AO,
     cut into 2-3 steps of the part's palette ramp.
  3. Ink: 1px outline outside the silhouette; inside, a line on the far side of an occlusion edge between
     different parts, and always between different materials (fur | pants | hat | clover).
Writes OUTDIR/<prefix>_yaw_<deg>.png (1x, palette-exact, alpha 0/255).
"""
import sys, os, json, glob
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
EXP = os.path.abspath(os.path.join(HERE, '..', '..'))
BEAR = json.load(open(os.path.join(EXP, 'bear-proto', 'palette-bear.json')))
ROOM = json.load(open(os.path.join(EXP, 'scene-proto', 'palette.json')))
def hx(h): return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))
INK = hx(BEAR['ink'][0])
FUR = [hx(h) for h in BEAR['fur']]; PANTS = [hx(h) for h in BEAR['pants']]
HAT = [hx(h) for h in BEAR['hat']]; BLUSH = [hx(h) for h in BEAR['blush']]; MOSS = [hx(h) for h in ROOM['moss']]
ALLOWED = set([hx(h) for k, v in BEAR.items() if k != 'note' for h in v] + MOSS)

# ramp steps per part, dark -> light (index 0 used in deepest shadow)
R_FUR = [FUR[2], FUR[3], FUR[5]]          # b9b3cb, dcd8e6, fbfaf8
R_PANTS = [PANTS[2], PANTS[3], PANTS[4]]  # 7dbcb0, a9dccf, cdeee6
R_HAT = [HAT[2], HAT[3], HAT[4]]          # b4a3d4, d3c6ea, ead9ef
R_CLOVER = [MOSS[3], MOSS[4], MOSS[5]]    # 7f926f, a3ad84, cdd3a6 (lighter, fresher)
R_EARIN = [BLUSH[2], BLUSH[3], BLUSH[3]]   # pink inner ear
R_INK = [INK, INK, INK]
R_BLUSH = [BLUSH[2], BLUSH[3], BLUSH[3]]

# id: (name, ramp, material group, line group, thin-claim fraction or None, priority)
PARTS = {
    1: ('body', R_FUR, 'fur', 1, None, 0),
    2: ('arm_L', R_FUR, 'fur', 2, None, 1), 3: ('arm_R', R_FUR, 'fur', 3, None, 1),
    4: ('ear_L', R_FUR, 'fur', 4, None, 1), 5: ('ear_R', R_FUR, 'fur', 5, None, 1),
    6: ('ear_in_L', R_EARIN, 'fur', 4, 0.30, 3), 7: ('ear_in_R', R_EARIN, 'fur', 5, 0.30, 3),
    8: ('foot_L', R_FUR, 'fur', 8, None, 0), 9: ('foot_R', R_FUR, 'fur', 9, None, 0),
    10: ('pants', R_PANTS, 'pants', 10, None, 0),
    11: ('hat', R_HAT, 'hat', 11, None, 1),
    12: ('stem', R_INK, 'ink', 12, 0.14, 6),
    13: ('clover', R_CLOVER, 'clover', 13, 0.35, 5),
    14: ('eyes', R_INK, 'ink', 1, 0.12, 9),
    15: ('brows', R_INK, 'ink', 1, 0.16, 7),
    16: ('nose', R_INK, 'ink', 1, 0.18, 8),
    17: ('blush', R_BLUSH, 'fur', 1, 0.35, 4),
    18: ('mouth', R_INK, 'ink', 1, 0.16, 7),
    19: ('hat_face', R_INK, 'ink', 11, 0.16, 6),
    20: ('stitch', R_INK, 'ink', 10, 0.22, 5),
    21: ('clover_b', R_CLOVER, 'clover', 21, 0.35, 5),
}
NID = 22
# shade thresholds: value v in [0,1]; v >= T[1] -> light, v >= T[0] -> mid, else dark
THRESH = {'fur': (0.24, 0.50), 'pants': (0.42, 0.64), 'hat': (0.24, 0.42), 'clover': (0.22, 0.45), 'ink': (0, 0)}
LIGHT_CAM = (0.30, 0.95, 0.70)   # light from: camera-right, world-up, toward-camera weights

N4 = ((0, 1), (0, -1), (1, 0), (-1, 0))
def shift(a, dy, dx, fill):
    out = np.full_like(a, fill); H, W = a.shape[:2]
    ys, yd = (slice(0, H - dy), slice(dy, H)) if dy >= 0 else (slice(-dy, H), slice(0, H + dy))
    xs, xd = (slice(0, W - dx), slice(dx, W)) if dx >= 0 else (slice(-dx, W), slice(0, W + dx))
    out[yd, xd] = a[ys, xs]
    return out

def cells(a, k):
    H, W = a.shape[0] // k, a.shape[1] // k
    return a[:H * k, :W * k].reshape(H, k, W, k, *a.shape[2:]).swapaxes(1, 2).reshape(H, W, k * k, *a.shape[2:])

def pixelize(npz, meta, want_depth=False):
    k = meta['k']
    d = np.load(npz)
    pid = np.where(d['alpha'], d['pid'], 0).astype(np.int32)
    pc = cells(pid, k); n2 = k * k
    cnt = np.stack([(pc == i).sum(-1) for i in range(NID)], -1)          # H,W,NID
    H, W = cnt.shape[:2]
    # majority among solid parts (background = 0 competes for the silhouette)
    solid = np.array([i == 0 or PARTS[i][4] is None for i in range(NID)])
    opaque = cnt[:, :, 0] < n2 / 2
    cs = np.where(solid[None, None, :], cnt, 0).copy(); cs[:, :, 0] = 0
    # cells that are mostly opaque but whose majority is a decal: fall back to the solid part under it
    base = cs.argmax(-1)
    base[base == 0] = 0
    choice = np.where(opaque, base, 0)
    # thin / decal parts claim the cell above their fraction, highest priority wins
    best_pri = np.full((H, W), -1)
    for i, p in PARTS.items():
        if p[4] is None: continue
        claim = cnt[:, :, i] >= p[4] * n2
        if p[2] != 'ink' and p[1] is not R_CLOVER:   # blush / inner ear only on top of the bear
            claim &= opaque
        take = claim & (p[5] > best_pri)
        choice[take] = i; best_pri[take] = p[5]
    # where opaque but no solid part found (all decal), pick the most covered decal
    fix = opaque & (choice == 0)
    choice[fix] = cnt[fix][:, 1:].argmax(-1) + 1
    # per-pixel normal / depth / ao of the chosen part
    nc = cells(d['nrm'].astype(np.float32) * 2 - 1, k); dc = cells(d['depth'], k); ac = cells(d['ao'].astype(np.float32), k)
    sel = pc == choice[:, :, None]
    ns = (nc * sel[..., None]).sum(2); nn = np.linalg.norm(ns, axis=-1, keepdims=True); nrm = ns / np.maximum(nn, 1e-6)
    depth = np.where(sel, dc, 1e9).min(-1)
    ao = (ac * sel).sum(-1) / np.maximum(sel.sum(-1), 1)
    # light
    R, U, B = (np.array(meta[x]) for x in ('right', 'up', 'back'))
    L = LIGHT_CAM[0] * R + LIGHT_CAM[1] * np.array([0, 0, 1.0]) + LIGHT_CAM[2] * np.array([B[0], B[1], 0])
    L /= np.linalg.norm(L)
    v = (nrm @ L) * 0.5 + 0.5
    v = v * (0.55 + 0.45 * ao)
    # colour
    rgb = np.zeros((H, W, 3), np.uint8); mat = np.full((H, W), '', object)
    for i, p in PARTS.items():
        m = choice == i
        if not m.any(): continue
        t0, t1 = THRESH[p[2]]
        step = np.where(v[m] >= t1, 2, np.where(v[m] >= t0, 1, 0))
        rgb[m] = np.array(p[1])[step]
        mat[m] = p[2]
    alpha = choice > 0
    # ---------------------------------------------------------------- ink lines
    lg = np.zeros((H, W), np.int32); mg = np.zeros((H, W), np.int32)
    MG = {'fur': 1, 'pants': 2, 'hat': 3, 'clover': 4, 'ink': 5}
    for i, p in PARTS.items():
        lg[choice == i] = p[3]; mg[choice == i] = MG[p[2]]
    # decals take the material group of the part they sit on (so blush/eyes never get outlines)
    mg_line = mg.copy()
    for i in (6, 7, 14, 15, 16, 17, 18): mg_line[choice == i] = 1
    mg_line[choice == 19] = 3; mg_line[choice == 20] = 2
    ink = np.zeros((H, W), bool)
    EPS_PART, EPS_SELF = 0.06, 0.30
    for dy, dx in N4:
        nlg = shift(lg, dy, dx, 0); nmg = shift(mg_line, dy, dx, 0); nd = shift(depth, dy, dx, 1e9)
        other = alpha & (nlg > 0)
        closer = nd < depth - EPS_PART
        ink |= other & (nlg != lg) & closer                       # part in front of another part
        ink |= other & (nmg != mg_line) & (nmg != 5) & (mg_line != 5) & (nd <= depth + 1e-3) & (nmg != 4)  # material seam, far side
        ink |= other & (nlg == lg) & (nd < depth - EPS_SELF)      # self occlusion (snout over cheek, belly)
    ink &= ~np.isin(choice, [12, 14, 15, 16, 18, 19, 20])          # already ink
    ink &= ~((mg == 4))                                           # clover leaves stay green
    rgb[ink] = INK
    # cross between the two clover leaf pairs, in dark moss
    cl = np.isin(choice, [13, 21]); seam = np.zeros((H, W), bool)
    for dy, dx in N4:
        nc_ = shift(choice, dy, dx, 0)
        seam |= cl & np.isin(nc_, [13, 21]) & (nc_ != choice) & (choice == 13)
    rgb[seam] = MOSS[2]
    # outer silhouette, 1px outside
    out = np.zeros((H, W), bool)
    for dy, dx in N4: out |= shift(alpha, dy, dx, False)
    out &= ~alpha
    rgb[out] = INK; alpha = alpha | out
    rgba = np.zeros((H, W, 4), np.uint8); rgba[..., :3] = rgb; rgba[..., 3] = alpha * 255
    if want_depth:   # Stage B layer compositing: view depth of the interior pixels (inf elsewhere)
        return rgba, np.where(choice > 0, depth, np.inf)
    return rgba

def check(rgba):
    a = rgba[..., 3]; assert set(np.unique(a)) <= {0, 255}
    cols = set(map(tuple, rgba[a == 255][:, :3]))
    bad = cols - ALLOWED
    assert not bad, bad

if __name__ == '__main__':
    raw, out = sys.argv[1], sys.argv[2]
    prefix = sys.argv[sys.argv.index('--prefix') + 1] if '--prefix' in sys.argv else os.path.basename(raw.rstrip('/'))
    meta = json.load(open(os.path.join(raw, 'meta.json')))
    os.makedirs(out, exist_ok=True)
    for f in sorted(glob.glob(os.path.join(raw, 'yaw_*.npz'))):
        rgba = pixelize(f, meta); check(rgba)
        yaw = os.path.basename(f)[4:-4]
        Image.fromarray(rgba).save(os.path.join(out, f'{prefix}_yaw_{yaw}.png'))
    print('ok', out)
