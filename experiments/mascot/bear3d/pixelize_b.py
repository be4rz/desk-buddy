"""Stage B pixel step (python3 + numpy + Pillow): render_b.py layer passes -> palette pixel layers + depth.

    python3 pixelize_b.py RAW/<set>_<ori> PX/<set>_<ori> [--keep]

bear_*.npz  -> the unchanged Stage A bear pixel step (pixelize.py), so the bear looks exactly as in Stage A.
other *.npz -> the same algorithm with the furniture part table below (palette-ext.json ramps).
Writes <key>.png (1x, alpha 0/255, palette-exact) and <key>_d.npy (view depth per pixel; the 1px outline takes
the depth of the nearest interior neighbour, inf on empty pixels) for the per-pixel depth composite.
Raw passes are deleted once pixelized unless --keep.
"""
import sys, os, json, glob
import numpy as np
from PIL import Image
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pixelize as PB
from pixelize import shift, cells, N4, INK, LIGHT_CAM

HERE = os.path.dirname(os.path.abspath(__file__))
EXT = json.load(open(os.path.join(HERE, 'palette-ext.json')))
def ramp(name, idx): return [PB.hx(EXT[name][i]) for i in idx]
R_WOOD = ramp('wood', (2, 3, 4)); R_CREAM = ramp('fabric_cream', (2, 3, 4)); R_PAGE = ramp('fabric_cream', (1, 3, 4))
R_QUILT = ramp('fabric_quilt', (1, 2, 3)); R_LINING = ramp('fabric_cream', (1, 2, 3))
R_SAGE = ramp('fabric_sage', (1, 2, 3)); R_BTN = ramp('fabric_sage', (0, 1, 1)); R_ROSE = ramp('fabric_rose', (1, 2, 3))
ALLOWED = PB.ALLOWED | {PB.hx(h) for k, v in EXT.items() if k != 'note' for h in v}

# id: (name, ramp, material group, line group, thin-claim fraction or None, priority)
PARTS = {
    22: ('bed_base', R_WOOD, 'wood', 22, None, 0), 23: ('bed_head', R_WOOD, 'wood', 23, None, 0),
    24: ('bed_foot', R_WOOD, 'wood', 24, None, 0), 25: ('mattress', R_CREAM, 'cream', 25, None, 0),
    26: ('pillow', R_CREAM, 'cream', 26, None, 0), 27: ('quilt', R_QUILT, 'quilt', 27, None, 0),
    28: ('quilt_lining', R_LINING, 'cream', 28, None, 0),
    29: ('table_top', R_WOOD, 'wood', 29, None, 0), 30: ('table_legs', R_WOOD, 'wood', 30, None, 0),
    31: ('pouf', R_SAGE, 'sage', 31, None, 0), 32: ('pouf_btn', R_BTN, 'sage', 31, 0.30, 4),
    33: ('book_cover_B', R_ROSE, 'rose', 33, None, 0), 34: ('book_cover_F', R_ROSE, 'rose', 34, None, 0),
    35: ('book_pages_B', R_PAGE, 'cream', 35, None, 0), 36: ('book_pages_F', R_PAGE, 'cream', 36, None, 0),
    37: ('book_leaf', R_PAGE, 'cream', 37, 0.30, 5),
}
NID = 38
MG = {'wood': 1, 'cream': 2, 'quilt': 3, 'sage': 4, 'rose': 5}
THRESH = {'wood': (0.50, 0.74), 'cream': (0.46, 0.70), 'quilt': (0.46, 0.70), 'sage': (0.44, 0.70), 'rose': (0.44, 0.70)}

def pixelize_props(npz, meta):
    k = meta['k']; d = np.load(npz)
    pid = np.where(d['alpha'], d['pid'], 0).astype(np.int32)
    pc = cells(pid, k); n2 = k * k
    cnt = np.stack([(pc == i).sum(-1) for i in range(NID)], -1)
    H, W = cnt.shape[:2]
    solid = np.array([i == 0 or (i in PARTS and PARTS[i][4] is None) for i in range(NID)])
    opaque = cnt[:, :, 0] < n2 / 2
    cs = np.where(solid[None, None, :], cnt, 0).copy(); cs[:, :, 0] = 0
    choice = np.where(opaque, cs.argmax(-1), 0)
    best = np.full((H, W), -1)
    for i, p in PARTS.items():
        if p[4] is None: continue
        take = (cnt[:, :, i] >= p[4] * n2) & (p[5] > best) & (opaque | (cnt[:, :, i] >= 0.5 * n2))
        choice[take] = i; best[take] = p[5]
    fix = opaque & (choice == 0); choice[fix] = cnt[fix][:, 1:].argmax(-1) + 1
    nc = cells(d['nrm'].astype(np.float32) * 2 - 1, k); dc = cells(d['depth'], k); ac = cells(d['ao'].astype(np.float32), k)
    sel = pc == choice[:, :, None]
    ns = (nc * sel[..., None]).sum(2); nrm = ns / np.maximum(np.linalg.norm(ns, axis=-1, keepdims=True), 1e-6)
    depth = np.where(sel, dc, 1e9).min(-1)
    ao = (ac * sel).sum(-1) / np.maximum(sel.sum(-1), 1)
    R, U, B = (np.array(meta[x]) for x in ('right', 'up', 'back'))
    L = LIGHT_CAM[0] * R + LIGHT_CAM[1] * np.array([0, 0, 1.0]) + LIGHT_CAM[2] * np.array([B[0], B[1], 0])
    L /= np.linalg.norm(L)
    v = ((nrm @ L) * 0.5 + 0.5) * (0.55 + 0.45 * ao)
    rgb = np.zeros((H, W, 3), np.uint8)
    lg = np.zeros((H, W), np.int32); mg = np.zeros((H, W), np.int32)
    for i, p in PARTS.items():
        m = choice == i
        if not m.any(): continue
        t0, t1 = THRESH[p[2]]
        rgb[m] = np.array(p[1])[np.where(v[m] >= t1, 2, np.where(v[m] >= t0, 1, 0))]
        lg[m] = p[3]; mg[m] = MG[p[2]]
    alpha = choice > 0
    ink = np.zeros((H, W), bool)
    for dy, dx in N4:
        nlg = shift(lg, dy, dx, 0); nmg = shift(mg, dy, dx, 0); nd = shift(depth, dy, dx, 1e9)
        other = alpha & (nlg > 0)
        ink |= other & (nlg != lg) & (nd < depth - 0.06)                 # part in front of another part
        ink |= other & (nmg != mg) & (nd <= depth + 1e-3)                 # material seam, far side
        ink |= other & (nlg == lg) & (nd < depth - 0.30)                  # self occlusion
    ink &= ~np.isin(choice, [32, 37])
    rgb[ink] = INK
    out = np.zeros((H, W), bool)
    for dy, dx in N4: out |= shift(alpha, dy, dx, False)
    out &= ~alpha; rgb[out] = INK
    rgba = np.zeros((H, W, 4), np.uint8); rgba[..., :3] = rgb; rgba[..., 3] = (alpha | out) * 255
    return rgba, np.where(alpha, depth, np.inf)

def outline_depth(rgba, dep):
    """give the 1px outline pixels the depth of their nearest interior neighbour."""
    a = rgba[..., 3] > 0; d = dep.copy(); o = a & ~np.isfinite(dep)
    nb = np.full(d.shape, np.inf)
    for dy, dx in N4: nb = np.minimum(nb, shift(dep, dy, dx, np.inf))
    d[o] = nb[o]
    return d.astype(np.float32)

def check(rgba):
    a = rgba[..., 3]; assert set(np.unique(a)) <= {0, 255}
    bad = set(map(tuple, rgba[a == 255][:, :3])) - ALLOWED
    assert not bad, bad

if __name__ == '__main__':
    raw, px = sys.argv[1].rstrip('/'), sys.argv[2]
    meta = json.load(open(os.path.join(raw, 'meta.json'))); os.makedirs(px, exist_ok=True)
    n = 0
    for f in sorted(glob.glob(os.path.join(raw, '*.npz'))):
        key = os.path.basename(f)[:-4]
        if key.startswith('bear_'): rgba, dep = PB.pixelize(f, meta, want_depth=True); PB.check(rgba)
        else: rgba, dep = pixelize_props(f, meta)
        check(rgba)
        Image.fromarray(rgba).save(os.path.join(px, key + '.png'))
        np.save(os.path.join(px, key + '_d.npy'), outline_depth(rgba, dep))
        if '--keep' not in sys.argv: os.remove(f)
        n += 1
    json.dump(meta, open(os.path.join(px, 'meta.json'), 'w'))
    print('ok', px, n, 'layers')
