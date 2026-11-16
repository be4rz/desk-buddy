"""Environment prototype pixel step (python3 + numpy + Pillow): envkit passes -> palette pixel art.

    python3 pix.py [name ...]        raw/<name>/*.npz -> out/<name>/<variant>__<layer>.png  (1x, alpha 0/255)

Per output pixel (K x K supersample cell): the majority part wins (decal parts claim a cell at their coverage);
shade = lambert sun with cast shadows + a half-lambert fill, times AO, plus the part's procedural texture and
per-object jitter, cut into the part's ramp tones; glow lights lift tones; the variant's ambient shifts them.
Lines: 1 px ink outside the silhouette and on the far side of a real occlusion; soft lines in the ramp's own
line colour (index 0) where objects touch or parts meet, and between the objects of one group (planks, tiles).
"""
import sys, os, json, glob
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
RAW, OUT = os.path.join(HERE, 'raw'), os.path.join(HERE, 'out')
INK = (0x1b, 0x18, 0x20)
BAYER = np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]], np.float32) / 16.0 - 0.47
N4 = ((-1, 0), (1, 0), (0, -1), (0, 1))
AUTO = {1: (), 2: (0.56,), 3: (0.50, 0.74), 4: (0.36, 0.50, 0.74), 5: (0.33, 0.44, 0.58, 0.76), 6: (0.30, 0.40, 0.50, 0.62, 0.78)}


def hx(h): return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))
def cells(a, k):
    H, W = a.shape[0] // k, a.shape[1] // k
    return a.reshape(H, k, W, k, *a.shape[2:]).swapaxes(1, 2).reshape(H, W, k * k, *a.shape[2:])
def shift(a, dy, dx, fill):
    o = np.full_like(a, fill); H, W = a.shape[:2]
    ys, yd = (slice(0, H - dy), slice(dy, H)) if dy >= 0 else (slice(-dy, H), slice(0, H + dy))
    xs, xd = (slice(0, W - dx), slice(dx, W)) if dx >= 0 else (slice(-dx, W), slice(0, W + dx))
    o[yd, xd] = a[ys, xs]; return o


def pixelize(npz, meta, variant):
    k = meta['k']; d = np.load(npz); V = meta['variants'][variant]
    parts = {int(i): p for i, p in meta['parts'].items()}; NP = max(parts) + 1
    U = np.array(meta['uids']); uid = np.where(d['alpha'], d['uid'], 0).astype(np.int32)
    uid = np.clip(uid, 0, len(U) - 1)
    pc = cells(U[uid, 0], k); uc = cells(uid, k); n2 = k * k
    cnt = np.stack([(pc == i).sum(-1) for i in range(NP)], -1)
    H, W = cnt.shape[:2]
    solid = np.array([i == 0 or (i in parts and parts[i]['thin'] is None) for i in range(NP)])
    opaque = cnt[:, :, 0] < n2 / 2
    cs = np.where(solid[None, None, :], cnt, 0).copy(); cs[:, :, 0] = 0
    choice = np.where(opaque, cs.argmax(-1), 0)
    best = np.full((H, W), -1)
    for i, p in parts.items():
        if p['thin'] is None: continue
        take = (cnt[:, :, i] >= p['thin'] * n2) & (p['prio'] > best) & (opaque | (cnt[:, :, i] >= 0.5 * n2))
        choice[take] = i; best[take] = p['prio']
    fix = opaque & (choice == 0); choice[fix] = cnt[fix][:, 1:].argmax(-1) + 1
    alpha = choice > 0
    sel = pc == choice[:, :, None]; ns = np.maximum(sel.sum(-1), 1)
    def mean(a): return (cells(a.astype(np.float32), k) * sel).sum(-1) / ns
    nc = cells(d['nrm'].astype(np.float32) * 2 - 1, k)
    nv = (nc * sel[..., None]).sum(2); nrm = nv / np.maximum(np.linalg.norm(nv, axis=-1, keepdims=True), 1e-6)
    depth = np.where(sel, cells(d['depth'], k), 1e9).min(-1)
    ao, sun, glow = mean(d['ao']), mean(d['sun']), mean(d['glow'])
    tex, rnd = mean(d['tex'][:, :, 0]), mean(d['tex'][:, :, 1])
    first = sel.argmax(-1); upx = np.take_along_axis(uc, first[..., None], -1)[..., 0]
    gid = U[upx, 1]
    hl = (nrm @ np.array(meta['sun'])) * 0.5 + 0.5
    lit = (0.60 * hl + 0.40 * np.clip(sun, 0, 1)) * (0.70 + 0.30 * ao)
    yy, xx = np.mgrid[0:H, 0:W]; bay = BAYER[yy % 4, xx % 4]
    rgb = np.zeros((H, W, 3), np.uint8); line = np.zeros((H, W, 3), np.uint8)
    seam_ok = np.zeros((H, W), bool); noline = np.zeros((H, W), bool)
    g1, g2 = V.get('g1', 0.16), V.get('g2', 0.55)
    for i, p in parts.items():
        m = choice == i
        if not m.any(): continue
        col = np.array([hx(c) for c in p['colors']]); idx = np.array(p['idx']); n = len(idx)
        if p['sh'] == 'tex':
            v = tex[m] + bay[m] * p['dither']; th = [(j + 1) / n for j in range(n - 1)]; tone = np.digitize(v, th)
            pos = idx[np.clip(tone, 0, n - 1)]
        else:
            v = lit[m] + (tex[m] - 0.5) * p['amp'] + (rnd[m] - 0.5) * p['rnd'] + bay[m] * p['dither']
            tone = np.digitize(v, p['thresh'] or AUTO[n])
            if p['tstep']: tone = tone - (tex[m] < p['tstep'][0]) + (tex[m] > p['tstep'][1])
            tone = np.clip(tone, 0, n - 1)
            gl = np.minimum((glow[m] > g1).astype(int) + (glow[m] > g2), p['gmax'])
            pos = idx[tone] + gl + V.get('amb', 0)
            lo = 1 if len(col) > 2 else 0
            pos = np.clip(pos, lo, len(col) - 1)
        rgb[m] = col[pos]
        line[m] = col[np.maximum(np.minimum(pos, idx[0]) - 1, 0)]
        seam_ok[m] = p['seam'] and not p['noline']; noline[m] = p['noline']
    ink = np.zeros((H, W), bool); soft = np.zeros((H, W), bool)
    for dy, dx in N4:
        ng = shift(gid, dy, dx, 0); nd = shift(depth, dy, dx, 1e9); nu = shift(upx, dy, dx, 0)
        npart = shift(choice, dy, dx, 0); nnl = shift(noline, dy, dx, True); nso = shift(seam_ok, dy, dx, False)
        both = alpha & (npart > 0) & ~noline & ~nnl
        near = nd < depth - 0.04
        ink |= both & (nd < depth - 0.22)                                   # a real occlusion: far side gets ink
        soft |= both & near & ((ng != gid) | (npart != choice))             # things touching / parts meeting
        if (dy, dx) in ((1, 0), (0, 1)):                                    # seams: one side only -> 1 px lines
            soft |= both & (nu != upx) & (ng == gid) & seam_ok & nso & (np.abs(nd - depth) < 0.22)
    rgb[soft & ~ink] = line[soft & ~ink]; rgb[ink] = INK
    out = np.zeros((H, W), bool)
    for dy, dx in N4: out |= shift(alpha, dy, dx, False)
    out &= ~alpha; rgb[out] = INK
    rgba = np.zeros((H, W, 4), np.uint8); rgba[..., :3] = rgb; rgba[..., 3] = (alpha | out) * 255
    return rgba


def allowed():
    E = os.path.join(HERE, '..'); s = {INK}
    for f in (os.path.join(E, 'scene-proto', 'palette.json'), os.path.join(E, 'bear-proto', 'palette-bear.json'),
              os.path.join(E, 'mascot', 'bear3d', 'palette-ext.json'), os.path.join(E, 'mascot', 'bear3d', 'palette-ext-out.json'),
              os.path.join(HERE, 'palette-env.json')):
        s |= {hx(h) for k, v in json.load(open(f)).items() if k != 'note' for h in v}
    return s


def check(rgba, ok):
    a = rgba[..., 3]; assert set(np.unique(a)) <= {0, 255}
    bad = set(map(tuple, rgba[a == 255][:, :3])) - ok
    assert not bad, bad


if __name__ == '__main__':
    names = sys.argv[1:] or sorted(os.path.basename(p) for p in glob.glob(os.path.join(RAW, '*')))
    ok = allowed()
    for nm in names:
        meta = json.load(open(os.path.join(RAW, nm, 'meta.json'))); od = os.path.join(OUT, nm); os.makedirs(od, exist_ok=True)
        for v in meta['variants']:
            for layer in meta['layers']:
                tag = (v or 'base') + '__' + layer
                rgba = pixelize(os.path.join(RAW, nm, tag + '.npz'), meta, v); check(rgba, ok)
                Image.fromarray(rgba).save(os.path.join(od, tag + '.png'))
        json.dump({k: meta[k] for k in ('name', 'note', 'W', 'H', 'origin', 'ppu', 'variants', 'layers', 'bear')},
                  open(os.path.join(od, 'meta.json'), 'w'), indent=1)
        print('ok', nm, meta['W'], meta['H'], list(meta['variants']), meta['layers'])
