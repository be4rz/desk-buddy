"""Pocket scenes: decorations that appear around the bear for one activity. A scene has no floor and no walls of its
own: it is drawn over whichever floor is selected, and its windows, shelves and pictures hang where a wall would be.

    sh run.sh scenes [--only nook,kitchen,tearoom,grove,library]

World frame = the bear set frame: the bear's hand-off spot is the origin; the unseen back walls are the NW (x = const)
and NE (y = const) planes, so the view stays open toward the camera.
"""
import sys, os, math, random
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from envkit import M, Scene, only, PXH, PXV
import floors as F

X, Y, Z = (1, 0, 0), (0, 1, 0), (0, 0, 1)
WT = 0.16                                   # how far a window's sky sits behind the wall plane


# ================================================================ helpers
def wpt(axis, pos, a, d, z):
    """wall frame -> world: a = along the wall, d = depth (negative = into the room), z up."""
    return (a, pos + d, z) if axis == 'y' else (pos - d, a, z)


def wbox(m, axis, pos, a0, a1, z0, z1, d0, d1, bevel=0.0):
    p, q = wpt(axis, pos, a0, d0, z0), wpt(axis, pos, a1, d1, z1)
    return m.slab(p[0], q[0], p[1], q[1], p[2], q[2], bevel)


def sky(S, axis, pos, a0, a1, z0, z1, var, kind):
    """what the window shows for one time of day: gradient sky, far hills, sun or moon, clouds, stars."""
    rs = random.Random(656); v = '_' + kind      # the night's stars (a fixed seed: str hashes change per process)
    ramp = 'sky_' + kind; n = len(S.pal[ramp])
    S.part('sky' + v, ramp, tuple(range(n)), tex=('grad', Z, z1 + 0.1, z0 + (z1 - z0) * 0.12), sh='tex', dither=0.16,
           noline=True, shadow=False, gmax=0)
    S.set(group='sky', var=var)
    S.add('sky' + v, wbox(M(), axis, pos, a0, a1, z0, z1, WT - 0.012, WT))
    hr = 'hills_' + ('day' if kind == 'day' else kind)
    S.part('hill_far' + v, hr, (2, 3), tex=('grad', Z, z0, z0 + 0.9), sh='tex', noline=True, shadow=False, gmax=0)
    S.part('hill_near' + v, hr, (0, 1), tex=('grad', Z, z0, z0 + 0.6), sh='tex', noline=True, shadow=False, gmax=0)
    w = a1 - a0
    def ridge(base, amp, ph, k):
        pts = [(a0, z0)] + [(a0 + w * i / 24, z0 + base + amp * (math.sin(ph + i * k) * 0.6 + math.sin(2.3 * ph + i * k * 2.1) * 0.4))
                           for i in range(25)] + [(a1, z0)]
        return pts[::-1]
    def flat(m, pts, d):                    # a silhouette in the wall plane at depth d (4 mm thick)
        t = M().poly((0, 0, 0), [(p[0], p[1]) for p in pts], 0.004)
        import bmesh
        for vtx in t.bm.verts:
            a, z, e = vtx.co.x, vtx.co.y, vtx.co.z
            vtx.co = wpt(axis, pos, a, d + e, z)
        bmesh.ops.recalc_face_normals(t.bm, faces=list(t.bm.faces))
        import bpy
        me = bpy.data.meshes.new('t'); t.bm.to_mesh(me); t.bm.free(); m.bm.from_mesh(me); bpy.data.meshes.remove(me)
        return m
    def disc(cx, cz, r, n=20, ph=0): return [(cx + r * math.cos(ph + 2 * math.pi * i / n), cz + r * math.sin(ph + 2 * math.pi * i / n)) for i in range(n)]
    S.add('hill_far' + v, flat(M(), ridge(0.52, 0.16, 1.0, 0.33), WT - 0.03))
    S.add('hill_near' + v, flat(M(), ridge(0.26, 0.12, 4.0, 0.47), WT - 0.05))
    cz_hi = z0 + (z1 - z0) * 0.72
    if kind == 'day':
        S.part('cloud' + v, 'snow', (3, 4), tex=('grad', Z, z0, z1), sh='tex', noline=True, shadow=False, gmax=0)
        S.part('sun' + v, 'moon', (2, 3), sh='tex', tex=('const', 0.9), noline=True, shadow=False, gmax=0)
        S.add('sun' + v, flat(M(), disc(a0 + w * 0.80, cz_hi + 0.15, 0.17), WT - 0.02))
        clouds = [(0.26, 0.74, 1.0), (0.62, 0.52, 0.8), (0.86, 0.36, 0.6)]
    elif kind == 'dawn':
        S.part('cloud' + v, 'cloud_dawn', (1, 2, 3), tex=('grad', Z, z0 + 0.5, z1), sh='tex', noline=True, shadow=False, gmax=0)
        S.part('sun' + v, 'sky_dawn', (6, 6), sh='tex', noline=True, shadow=False, gmax=0)
        S.part('halo' + v, 'sky_dawn', (5, 5), sh='tex', noline=True, shadow=False, gmax=0)
        sx, sz = a0 + w * 0.36, z0 + 0.62
        S.add('halo' + v, flat(M(), disc(sx, sz, 0.42), WT - 0.016)); S.add('sun' + v, flat(M(), disc(sx, sz, 0.26), WT - 0.02))
        clouds = [(0.70, 0.66, 1.0), (0.30, 0.84, 0.7), (0.88, 0.44, 0.55)]
    elif kind == 'dusk':
        S.part('cloud' + v, 'cloud_dusk', (1, 2, 3, 4), tex=('grad', Z, z1 + 0.2, z0 + 0.5), sh='tex', noline=True, shadow=False, gmax=0)
        S.part('sun' + v, 'sky_dusk', (7, 7), sh='tex', noline=True, shadow=False, gmax=0)
        S.part('halo' + v, 'sky_dusk', (6, 6), sh='tex', noline=True, shadow=False, gmax=0)
        sx, sz = a0 + w * 0.66, z0 + 0.50
        S.add('halo' + v, flat(M(), disc(sx, sz, 0.46), WT - 0.016)); S.add('sun' + v, flat(M(), disc(sx, sz, 0.30), WT - 0.02))
        clouds = [(0.24, 0.60, 1.0), (0.60, 0.82, 0.8), (0.90, 0.56, 0.6)]
    else:
        S.part('cloud' + v, 'cloud_night', (1, 2), tex=('grad', Z, z0, z1), sh='tex', noline=True, shadow=False, gmax=0)
        S.part('moon' + v, 'moon', (1, 2), tex=('grad', (0.7, 0, 0.7), -9, 9), sh='tex', noline=True, shadow=False, gmax=0)
        S.part('star' + v, 'moon', (1, 2), tex=('noise', 40.0, (1, 1, 1), 0.0), sh='tex', thin=0.16, prio=3, noline=True, shadow=False, gmax=0)
        mx, mz = a0 + w * 0.70, cz_hi + 0.08
        cres = [(mx + 0.25 * math.cos(t), mz + 0.25 * math.sin(t)) for t in [math.radians(-110 + 220 * i / 14) for i in range(15)]]
        cres += [(mx - 0.13 + 0.27 * math.cos(t), mz + 0.27 * math.sin(t)) for t in [math.radians(62 - 124 * i / 10) for i in range(11)]]
        S.add('moon' + v, flat(M(), cres, WT - 0.02))
        st = M()
        for _ in range(26):
            a, z = rs.uniform(a0 + 0.08, a1 - 0.08), rs.uniform(z0 + 0.85, z1 - 0.06)
            if math.hypot(a - mx, z - mz) < 0.42: continue
            sz = rs.choice((1.0, 1.0, 1.5)); flat(st, [(a - PXH * sz * 0.6, z), (a, z - PXV * sz * 0.6), (a + PXH * sz * 0.6, z), (a, z + PXV * sz * 0.6)], WT - 0.02)
        S.add('star' + v, st)
        clouds = [(0.24, 0.50, 0.9)]
    cm = M()
    for fa, fz, sc in clouds:
        ca, cz = a0 + w * fa, z0 + (z1 - z0) * fz
        for da, dz, r in ((0, 0, 0.17), (0.22, -0.02, 0.13), (-0.21, -0.03, 0.12), (0.08, 0.08, 0.12), (0.40, -0.05, 0.08), (-0.38, -0.05, 0.07)):
            flat(cm, disc(ca + da * sc, cz + dz * sc, r * sc, 14), WT - 0.024)
        flat(cm, [(ca - 0.5 * sc, cz - 0.14 * sc), (ca + 0.52 * sc, cz - 0.14 * sc), (ca + 0.52 * sc, cz - 0.03 * sc), (ca - 0.5 * sc, cz - 0.03 * sc)], WT - 0.024)
    S.add('cloud' + v, cm)
    S.set(var=None)


def window(S, axis, pos, a0, a1, z0, z1, wood='oak', bars=(1, 1), sill=0.24):
    """frame, mullions and a deep sill around a window opening."""
    if 'win_frame' not in S.parts:
        S.part('win_frame', wood, (2, 3, 4, 5), seam=False); S.part('win_bar', wood, (2, 3, 4), seam=False)
    S.set(group='window_%s_%g' % (axis, a0)); f = 0.11; m = M()
    wbox(m, axis, pos, a0 - f, a0, z0 - f, z1 + f, -0.05, WT); wbox(m, axis, pos, a1, a1 + f, z0 - f, z1 + f, -0.05, WT)
    wbox(m, axis, pos, a0, a1, z1, z1 + f, -0.05, WT)
    wbox(m, axis, pos, a0 - f - 0.06, a1 + f + 0.06, z0 - 0.09, z0, -sill, WT, bevel=0.02)
    S.add('win_frame', m)
    b = M(); bw = 0.035
    for i in range(1, bars[0] + 1):
        a = a0 + (a1 - a0) * i / (bars[0] + 1); wbox(b, axis, pos, a - bw, a + bw, z0, z1, 0.03, 0.09)
    for i in range(1, bars[1] + 1):
        z = z0 + (z1 - z0) * i / (bars[1] + 1); wbox(b, axis, pos, a0, a1, z - bw, z + bw, 0.03, 0.09)
    S.add('win_bar', b)


def window_blocker(S, axis, pos, a0, a1, z0, z1):
    m = M(); B = 40.0
    for r in ((-B, a0, -B, B), (a1, B, -B, B), (a0, a1, -B, z0), (a0, a1, z1, B)): wbox(m, axis, pos, r[0], r[1], r[2], r[3], WT + 0.02, WT + 0.04)
    S.blocker(m)


def curtains(S, axis, pos, a0, a1, z0, z1, ramp='fabric_rose', idx=(1, 2, 3, 4), folds=3, name='curtain'):
    """two tied-back drapes on a rod."""
    if name not in S.parts:
        S.part(name, ramp, idx, seam=False); S.part(name + '_tie', 'gold', (1, 2, 3), seam=False)
        S.part('rod', 'walnut', (2, 3, 4), seam=False)
    S.set(group=name + '_%s_%g' % (axis, a0))
    S.add('rod', M().pipe([wpt(axis, pos, a0 - 0.5, -0.16, z1 + 0.30), wpt(axis, pos, a1 + 0.5, -0.16, z1 + 0.30)], 0.035)
          .sq(wpt(axis, pos, a0 - 0.5, -0.16, z1 + 0.30), (0.07, 0.07, 0.07)).sq(wpt(axis, pos, a1 + 0.5, -0.16, z1 + 0.30), (0.07, 0.07, 0.07)))
    for side, edge in ((-1, a0), (1, a1)):
        m = M(); tie_z = z0 + (z1 - z0) * 0.36; top, hem = z1 + 0.27, z0 - 0.46
        for i in range(folds):
            top_a = edge + side * (0.40 - i * 0.21); pts, rad = [], []
            for j in range(15):                 # full at the rod, gathered at the tie, flaring to the hem
                z = hem + (top - hem) * j / 14; g = math.exp(-((z - tie_z) / 0.50) ** 2)
                pts.append(wpt(axis, pos, top_a * (1 - g) + (edge + side * 0.32) * g, -0.15, z)); rad.append(0.125 * (1 - 0.5 * g))
            m.tube(pts, rad)
        S.add(name, m)
        S.add(name + '_tie', M().sq(wpt(axis, pos, edge + side * 0.32, -0.16, tie_z), (0.15, 0.15, 0.05), e1=0.6))


BOOKS = [('fabric_rose', (1, 2, 3)), ('fabric_quilt', (1, 2, 3)), ('fabric_sage', (1, 2, 3)), ('gold', (1, 2, 3)),
         ('lacquer', (1, 2, 3)), ('mauve', (1, 2, 3)), ('fabric_cream', (1, 2, 3)), ('heri', (2, 3, 4)), ('terracotta', (1, 2, 3))]
def books(S, p0, d, length, depth, rs, hmin=0.34, hmax=0.52, lean=True, gap=0.0):
    """a row of books standing from p0 along unit direction d (X or Y); spines face the room."""
    for i, (r, ix) in enumerate(BOOKS):
        if 'book%d' % i not in S.parts: S.part('book%d' % i, r, ix, seam=True)
    if 'book_band' not in S.parts: S.part('book_band', 'gold', (3, 4), thin=0.3, prio=3, noline=True)
    t = 0.0; bands = M(); nb = 0
    while t < length - 0.07:
        w = min(rs.uniform(0.075, 0.15), length - t); h = rs.uniform(hmin, hmax); dp = depth * rs.uniform(0.8, 1.0)
        if gap and rs.random() < gap: t += w; continue
        c = (p0[0] + d[0] * (t + w / 2), p0[1] + d[1] * (t + w / 2), p0[2] + h / 2)
        size = (w - 0.012, dp, h) if d[0] else (dp, w - 0.012, h)
        tilt = rs.choice((0, 0, 0, 0, 7)) if lean else 0
        S.add('book%d' % rs.randrange(len(BOOKS)), M().box(c, size, bevel=0.012, rot=(0, tilt, 0) if d[1] else (tilt, 0, 0)))
        t += w
    return t


def plant(S, at, pot=0.24, h=0.34, kind='leafy', ramp='terracotta', leaf='pip', scale=1.0, seed=1, name=None):
    """a potted plant: 'leafy' (round bush), 'tall' (long leaves), 'trail' (hanging vine), 'succulent', 'bonsai'."""
    rs = random.Random(seed); n = name or 'plant_%s_%d' % (kind, seed)
    for pn, (r, ix) in (('pot_' + ramp, (ramp, (1, 2, 3, 4))), ('soil', ('soil', (1, 2))), ('leaf_' + leaf, (leaf, (1, 2, 3, 4) if leaf != 'pip' else (1, 2, 3, 4))),
                        ('stem', ('walnut', (2, 3)))):
        if pn not in S.parts: S.part(pn, r, ix, seam=False)
    S.set(group=n); x, y, z = at; s = scale; L = 'leaf_' + leaf
    S.add('pot_' + ramp, M().lathe((x, y, z), [(pot * 0.72 * s, 0), (pot * s, h * s * 0.86), (pot * 1.1 * s, h * s * 0.86), (pot * 1.1 * s, h * s)]))
    S.add('soil', M().cyl((x, y, z + h * s * 0.94), pot * 0.98 * s, 0.03))
    top = z + h * s; m = M()
    if kind == 'leafy':
        for i in range(16):
            a = rs.uniform(0, 6.28); d = rs.uniform(0, 0.30) * s; hh = rs.uniform(0.12, 0.62) * s
            m.sq((x + d * math.cos(a), y + d * math.sin(a), top + hh), (0.17 * s, 0.17 * s, 0.13 * s), rot=(rs.uniform(-30, 30), rs.uniform(-30, 30), 0))
    elif kind == 'tall':                        # snake plant: upright pointed blades
        for i in range(8):
            a = 2 * math.pi * i / 8 + rs.uniform(-0.3, 0.3); ln = rs.uniform(0.75, 1.3) * s; d = rs.uniform(0.02, 0.12) * s
            tilt = rs.uniform(6, 20)
            m.sq((x + (d + 0.12 * ln) * math.cos(a), y + (d + 0.12 * ln) * math.sin(a), top + ln * 0.48), (0.085 * s, 0.03 * s, ln * 0.52),
                 rot=(0, tilt, math.degrees(a)))
    elif kind == 'trail':
        for i in range(5):
            a = rs.uniform(0, 6.28); m.sq((x + 0.14 * math.cos(a) * s, y + 0.14 * math.sin(a) * s, top + 0.10 * s), (0.14 * s, 0.14 * s, 0.10 * s))
        for i in range(4):
            a = rs.uniform(-0.6, 2.2); ln = rs.uniform(0.5, 1.3) * s; px, py = x + pot * 1.0 * s * math.cos(a), y + pot * 1.0 * s * math.sin(a)
            k = int(ln / 0.12)
            for j in range(k):
                zz = top - 0.04 - j * 0.12 * s; sway = 0.10 * (1 if j % 2 else -1) * s
                m.sq((px + sway * math.cos(a + 1.57), py + sway * math.sin(a + 1.57), zz), (0.07 * s, 0.07 * s, 0.045 * s))
    elif kind == 'succulent':
        for ring, (rr, hh, cnt) in enumerate(((0.18, 0.05, 7), (0.11, 0.11, 5), (0.0, 0.16, 1))):
            for i in range(cnt):
                a = 2 * math.pi * i / cnt + ring * 0.4
                m.sq((x + rr * s * math.cos(a), y + rr * s * math.sin(a), top + hh * s), (0.085 * s, 0.085 * s, 0.07 * s))
    elif kind == 'bonsai':
        pts = [(x, y, top), (x + 0.05 * s, y - 0.04 * s, top + 0.22 * s), (x - 0.10 * s, y + 0.06 * s, top + 0.42 * s), (x + 0.02 * s, y + 0.02 * s, top + 0.60 * s)]
        S.add('stem', M().pipe(pts, 0.05 * s, r2=0.025 * s).pipe([pts[1], (x + 0.28 * s, y - 0.10 * s, top + 0.36 * s)], 0.03 * s, r2=0.02 * s))
        for cx, cy, cz, r in ((0.02, 0.02, 0.68, 0.24), (-0.22, 0.10, 0.50, 0.17), (0.30, -0.12, 0.44, 0.16), (0.16, 0.10, 0.60, 0.15)):
            m.sq((x + cx * s, y + cy * s, top + cz * s), (r * s, r * s, r * 0.55 * s))
    S.add(L, m)


def lamp(S, at, h=3.0, var_on=(), shade='paper', name='lamp'):
    """a floor lamp; the shade is lit (and throws a pool of light) in the var_on variants."""
    x, y, z = at
    if 'lamp_pole' not in S.parts:
        S.part('lamp_pole', 'walnut', (2, 3, 4), seam=False)
        S.part('shade_off', shade, (1, 2, 3), seam=False)
        S.part('shade_on', shade, (3, 4, 5), sh='tex', tex=('grad', Z, z + h - 0.75, z + h - 0.05), noline=False, gmax=0, shadow=False)
    S.set(group=name)
    S.add('lamp_pole', M().cyl((x, y, z), 0.22, 0.05).pipe([(x, y, z + 0.04), (x, y, z + h - 0.5)], 0.03))
    prof = [(0.36, 0), (0.24, 0.62)]
    all_v = [v for v in S.variants if v]
    off = tuple(v for v in all_v if v not in var_on)
    if off or not all_v: S.add('shade_off', M().lathe((x, y, z + h - 0.7), prof), var=off or None)
    if var_on:
        S.add('shade_on', M().lathe((x, y, z + h - 0.7), prof), var=tuple(var_on))
        for v in var_on: S.glow_point((x, y, z + h - 0.50), 95.0, var=v)


def rug(S, at, r, rings=(('fabric_cream', 3), ('fabric_rose', 2), ('fabric_cream', 3), ('fabric_sage', 2)), squash=1.0, name='rug'):
    S.set(group=name); x, y = at; n = len(rings)
    for i, (ramp, ix) in enumerate(rings):
        pn = '%s_%d' % (name, i)
        S.part(pn, ramp, (ix - 2, ix - 1, ix), seam=False, tex=('noise', 14.0, (1, 1, 1), 1.0), amp=0.10)
        rr = r * (1 - i / n * 0.86) if i else r
        S.add(pn, M().poly((x, y, 0.0), [(rr * math.cos(2 * math.pi * k / 40), rr * squash * math.sin(2 * math.pi * k / 40)) for k in range(40)],
                           0.02 + 0.004 * i))


def picture(S, axis, pos, a, z, w, h, motif='hill', name='pic'):
    """a framed little painting on the (unseen) wall plane."""
    for pn, r, ix, kw in (('frame', 'walnut', (2, 3, 4), {}), ('mat', 'fabric_cream', (3, 4), {}),
                          ('art_sky', 'sky_dawn', (2, 3, 4, 5), dict(sh='tex', tex=('grad', Z, z + h / 2, z - h / 2), gmax=0, noline=True)),
                          ('art_hill', 'tile_sage', (1, 2), dict(sh='tex', gmax=0, noline=True, tex=('const', 0.7))),
                          ('art_sun', 'flame', (3, 4), dict(sh='tex', gmax=0, noline=True, tex=('const', 0.7))),
                          ('art_leaf', 'pip', (2, 3), dict(sh='tex', gmax=0, noline=True, tex=('const', 0.7)))):
        if pn not in S.parts: S.part(pn, r, ix, seam=False, **kw)
    S.set(group='%s_%g_%g' % (name, a, z)); f = 0.06
    S.add('frame', wbox(M(), axis, pos, a - w / 2 - f, a + w / 2 + f, z - h / 2 - f, z + h / 2 + f, -0.05, 0.0, bevel=0.012))
    if motif == 'hill':
        S.add('art_sky', wbox(M(), axis, pos, a - w / 2, a + w / 2, z - h / 2, z + h / 2, -0.056, -0.05))
        S.add('art_hill', wbox(wbox(M(), axis, pos, a - w / 2, a + w / 2, z - h / 2, z - h * 0.22, -0.062, -0.056),
                               axis, pos, a - w * 0.05, a + w / 2, z - h * 0.22, z - h * 0.06, -0.062, -0.056))
        S.add('art_sun', wbox(M(), axis, pos, a - w * 0.28, a - w * 0.08, z + h * 0.08, z + h * 0.30, -0.062, -0.056))
    else:
        S.add('mat', wbox(M(), axis, pos, a - w / 2, a + w / 2, z - h / 2, z + h / 2, -0.056, -0.05))
        m = M()
        for i in range(5):
            t = i / 4; wbox(m, axis, pos, a - w * 0.22 * (1 - t * 0.6), a + w * 0.22 * (1 - t * 0.6), z - h * 0.30 + t * h * 0.5, z - h * 0.30 + t * h * 0.5 + h * 0.14, -0.062, -0.056)
        S.add('art_leaf', m)


def bear_set(S, json_, set_, file, ori='A', at=(0, 0)):
    S.bear = dict(json=json_, set=set_, ori=ori, file=file, at=list(at)); S.bear_shadow(at=at)


# ================================================================ 1. window nook (4 times of day)
def nook():
    S = Scene('scene_nook', 'Window nook: a window seat by a big window; the sky follows the time of day')
    for v, kw in (('dawn', dict(amb=0)), ('day', dict(amb=0)), ('dusk', dict(amb=-1)), ('night', dict(amb=-1))): S.variant(v, **kw)
    NE, NW, HT = 2.2, -2.2, 4.3
    win = (-0.7, 1.9, 1.35, 3.55)
    window(S, 'y', NE, *win, bars=(2, 1))
    for v in ('dawn', 'day', 'dusk', 'night'): sky(S, 'y', NE, *win, var=v, kind=v)
    window_blocker(S, 'y', NE, *win)
    S.glow_sun((0.22, -1, -0.78), var='dawn'); S.glow_sun((0.42, -1, -1.30), var='day')
    S.glow_sun((0.58, -1, -0.62), var='dusk'); S.glow_sun((0.36, -1, -1.0), var='night', strength=1.0)
    curtains(S, 'y', NE, win[0], win[1], win[2], win[3], ramp='fabric_cream', idx=(1, 2, 3, 4))
    # window seat with cushions
    S.part('seat', 'oak', (2, 3, 4, 5), tex=('noise', 2.2, (1.2, 14, 1), 3.0), amp=0.14, seam=False)
    S.part('seat_cush', 'fabric_quilt', (1, 2, 3, 4), seam=False)
    S.part('pillow_a', 'fabric_rose', (1, 2, 3, 4), seam=False); S.part('pillow_b', 'fabric_sage', (1, 2, 3, 4), seam=False)
    S.part('pillow_c', 'gold', (2, 3, 4), seam=False)
    S.set(group='seat')
    S.add('seat', M().slab(-0.75, 1.50, 1.50, NE - 0.04, 0.0, 0.50, bevel=0.03))
    S.add('seat_cush', M().sq((0.375, 1.83, 0.60), (1.10, 0.33, 0.13), e1=0.5, e2=0.25))
    S.set(group='pillows')
    S.add('pillow_a', M().sq((1.12, 1.95, 0.98), (0.30, 0.12, 0.30), e1=0.6, e2=0.6, rot=(18, 0, -8)))
    S.add('pillow_b', M().sq((0.62, 1.97, 0.95), (0.27, 0.11, 0.27), e1=0.6, e2=0.6, rot=(16, 0, 10)), group='pillow2')
    S.add('pillow_c', M().sq((0.90, 1.74, 0.84), (0.20, 0.10, 0.20), e1=0.6, e2=0.6, rot=(28, 0, -4)), group='pillow3')
    # on the sill
    plant(S, (1.55, NE - 0.10, 1.35), pot=0.11, h=0.17, kind='succulent', ramp='fabric_cream', seed=3)
    plant(S, (-0.35, NE - 0.10, 1.35), pot=0.12, h=0.18, kind='trail', seed=4, scale=0.9)
    # bookcase on the NW wall
    rs = random.Random(21)
    S.part('case', 'oak', (2, 3, 4, 5), tex=('noise', 2.2, (14, 1.2, 1), 3.0), amp=0.12, seam=False)
    S.part('case_back', 'oak', (1, 2), seam=False)
    S.set(group='bookcase'); bx0, bx1, by0, by1, bh = NW, NW + 0.50, -1.95, -0.25, 1.62
    m = M()
    for z in (0.0, 0.78, bh - 0.07): m.slab(bx0, bx1, by0, by1, z, z + 0.07)
    m.slab(bx0, bx1, by0, by0 + 0.07, 0, bh); m.slab(bx0, bx1, by1 - 0.07, by1, 0, bh)
    S.add('case', m); S.add('case_back', M().slab(bx0, bx0 + 0.05, by0, by1, 0, bh))
    S.set(group='books')
    books(S, (bx0 + 0.24, by0 + 0.09, 0.07), Y, 1.50, 0.34, rs); books(S, (bx0 + 0.24, by0 + 0.09, 0.85), Y, 1.05, 0.34, rs)
    S.part('globe', 'water', (2, 3, 4, 5), tex=('noise', 5.0, (1, 1, 1), 2.0), amp=0.5, seam=False)
    S.part('globe_stand', 'gold', (1, 2, 3), seam=False)
    S.set(group='globe')
    S.add('globe', M().sq((bx0 + 0.25, by1 - 0.33, bh + 0.32), (0.21, 0.21, 0.21)))
    S.add('globe_stand', M().cyl((bx0 + 0.25, by1 - 0.33, bh), 0.13, 0.035).pipe([(bx0 + 0.25, by1 - 0.33, bh), (bx0 + 0.25, by1 - 0.33, bh + 0.12)], 0.025))
    plant(S, (bx0 + 0.26, by0 + 0.32, bh), pot=0.15, h=0.22, kind='leafy', ramp='fabric_cream', seed=8, scale=0.8)
    S.set(group='mug'); S.part('mug', 'fabric_rose', (1, 2, 3), seam=False)
    S.add('mug', M().cyl((bx0 + 0.26, by0 + 1.38, 0.85), 0.07, 0.13))
    picture(S, 'x', NW, -1.45, 2.95, 0.62, 0.50, 'hill'); picture(S, 'x', NW, -0.55, 2.70, 0.36, 0.46, 'leaf')
    picture(S, 'x', NW, -0.62, 3.42, 0.30, 0.24, 'hill')
    lamp(S, (2.05, 1.80, 0.0), h=2.75, var_on=('dusk', 'night'))
    rug(S, (0.45, -0.25), 1.55, squash=0.86)
    plant(S, (NW + 0.42, 0.30, 0.0), pot=0.25, h=0.36, kind='tall', seed=5, ramp='terracotta')
    # a high shelf in the back corner: books and a trailing plant
    S.set(group='shelf')
    S.add('case', M().slab(NW, NW + 0.34, 0.70, 1.95, 3.00, 3.07).box((NW + 0.15, 0.95, 2.90), (0.26, 0.05, 0.20)).box((NW + 0.15, 1.75, 2.90), (0.26, 0.05, 0.20)))
    S.set(group='shelf_books'); books(S, (NW + 0.17, 0.78, 3.07), Y, 0.55, 0.26, rs, hmin=0.26, hmax=0.40)
    plant(S, (NW + 0.19, 1.62, 3.07), pot=0.13, h=0.18, kind='trail', seed=9, ramp='fabric_cream')
    bear_set(S, 'set_sit_b/anim_sit_b.json', 'laptop', 'laptop_type_A.png')
    S.render()



# ================================================================ 2. mini kitchen
def jars(S, p0, d, n, rs, step=0.26):
    for pn, r, ix in (('jar_glass', 'ice', (2, 3, 4)), ('jar_lid', 'oak', (2, 3, 4)), ('jar_a', 'terracotta', (2, 3)), ('jar_b', 'straw', (2, 3)),
                      ('jar_c', 'fabric_rose', (2, 3)), ('jar_d', 'pip', (2, 3))):
        if pn not in S.parts: S.part(pn, r, ix, seam=False)
    for i in range(n):
        x, y, z = p0[0] + d[0] * i * step, p0[1] + d[1] * i * step, p0[2]; h = rs.uniform(0.20, 0.32); r = rs.uniform(0.075, 0.10)
        S.add('jar_' + rs.choice('abcd'), M().cyl((x, y, z), r, h * 0.72)); S.add('jar_glass', M().cyl((x, y, z + h * 0.72), r, h * 0.28))
        S.add('jar_lid', M().cyl((x, y, z + h), r * 1.08, 0.05))


def kitchen():
    S = Scene('scene_kitchen', 'Mini kitchen: counter, stove with a steaming kettle, shelves, a little fridge')
    rs = random.Random(5)
    NW, NE, HT = -2.15, 2.30, 4.0
    win = (-1.75, -0.55, 2.15, 3.25)
    window(S, 'y', NE, *win, bars=(1, 1), sill=0.16); sky(S, 'y', NE, *win, var=None, kind='day')
    window_blocker(S, 'y', NE, *win); S.glow_sun((0.30, -1, -1.2))
    # counter run along the NE wall
    S.part('cab', 'fabric_sage', (1, 2, 3, 4), seam=False); S.part('cab_door', 'fabric_sage', (2, 3, 4), seam=True)
    S.part('knob', 'gold', (2, 3, 4), seam=False); S.part('top', 'oak', (3, 4, 5, 6), tex=('noise', 2.2, (1.2, 14, 1), 3.0), amp=0.12, seam=False)
    S.part('toe', 'walnut', (1, 2), seam=False)
    cx0, cx1, cy0 = NW + 0.03, 1.42, 1.62
    S.set(group='counter')
    S.add('toe', M().slab(cx0, cx1, cy0 + 0.06, NE, 0, 0.10)); S.add('cab', M().slab(cx0, cx1, cy0, NE, 0.10, 0.98))
    nd = 5; dw = (cx1 - cx0) / nd
    for i in range(nd):
        a = cx0 + i * dw
        S.add('cab_door', M().slab(a + 0.04, a + dw - 0.04, cy0 - 0.03, cy0, 0.16, 0.92, bevel=0.012))
        S.add('knob', M().sq((a + dw - 0.14, cy0 - 0.05, 0.76), (0.035, 0.035, 0.035)))
    S.add('top', M().slab(cx0, cx1 + 0.05, cy0 - 0.07, NE, 0.98, 1.07, bevel=0.02))
    # stove + kettle + pot
    S.part('hob', 'metal', (1, 2, 3), seam=False); S.part('burner', 'stone', (1, 2), seam=False)
    S.part('kettle', 'copper', (1, 2, 3, 4, 5), seam=False); S.part('kettle_h', 'walnut', (1, 2, 3), seam=False)
    S.part('pot', 'metal', (2, 3, 4, 5), seam=False); S.part('steam', 'snow', (3, 4), sh='tex', tex=('const', 0.9), noline=True, shadow=False, gmax=0)
    S.set(group='hob'); hx0, hx1 = 0.15, 1.25
    S.add('hob', M().slab(hx0, hx1, cy0 + 0.02, NE - 0.08, 1.07, 1.11, bevel=0.012))
    for bx in (0.43, 0.97): S.add('burner', M().cyl((bx, 1.95, 1.11), 0.19, 0.025))
    S.set(group='kettle'); kx, ky, kz = 0.43, 1.95, 1.135
    S.add('kettle', M().lathe((kx, ky, kz), [(0.19, 0), (0.22, 0.08), (0.19, 0.22), (0.11, 0.30), (0.06, 0.32)])
          .pipe([(kx + 0.17, ky, kz + 0.14), (kx + 0.30, ky, kz + 0.22), (kx + 0.36, ky, kz + 0.30)], 0.04, r2=0.025))
    S.add('kettle_h', M().pipe([(kx - 0.13, ky, kz + 0.26), (kx - 0.10, ky, kz + 0.42), (kx + 0.10, ky, kz + 0.42), (kx + 0.13, ky, kz + 0.26)], 0.025)
          .sq((kx, ky, kz + 0.335), (0.035, 0.035, 0.03)))
    st = M()
    for i, (dx, dz, r) in enumerate(((0.40, 0.46, 0.05), (0.44, 0.60, 0.065), (0.38, 0.76, 0.08), (0.45, 0.94, 0.06), (0.40, 1.08, 0.04))):
        st.sq((kx + dx, ky, kz + dz), (r, r, r))
    S.add('steam', st, group='steam')
    S.set(group='pot'); px = 0.97
    S.add('pot', M().cyl((px, 1.95, 1.135), 0.18, 0.20).cyl((px, 1.95, 1.335), 0.19, 0.03).sq((px, 1.95, 1.39), (0.04, 0.04, 0.03))
          .box((px - 0.22, 1.95, 1.29), (0.10, 0.05, 0.03)).box((px + 0.22, 1.95, 1.29), (0.10, 0.05, 0.03)))
    # board with a loaf, utensil crock, fruit bowl
    S.part('board', 'oak', (2, 3, 4), seam=False); S.part('loaf', 'straw', (1, 2, 3, 4), seam=False); S.part('crust', 'terracotta', (2, 3), thin=0.3, prio=3, noline=True)
    S.part('crock', 'fabric_cream', (1, 2, 3, 4), seam=False); S.part('spoon', 'oak', (3, 4, 5), seam=False)
    S.part('bowl', 'fabric_quilt', (1, 2, 3, 4), seam=False); S.part('fruit_a', 'carrot', (1, 2, 3), seam=False); S.part('fruit_b', 'lacquer', (2, 3, 4), seam=False)
    S.part('fruit_c', 'gold', (2, 3, 4), seam=False)
    S.set(group='board'); S.add('board', M().box((-0.42, 1.90, 1.09), (0.62, 0.40, 0.04), bevel=0.012, rot=(0, 0, 8)))
    S.set(group='loaf'); S.add('loaf', M().sq((-0.44, 1.90, 1.22), (0.25, 0.14, 0.13), e2=0.7, rot=(0, 0, 8)))
    cr = M()
    for i in (-1, 0, 1): cr.box((-0.44 + i * 0.12, 1.90, 1.345), (0.02, 0.16, 0.01), rot=(0, 0, 30))
    S.add('crust', cr)
    S.set(group='crock'); S.add('crock', M().lathe((-1.05, 2.02, 1.07), [(0.10, 0), (0.12, 0.22), (0.11, 0.24)]))
    S.add('spoon', M().pipe([(-1.05, 2.02, 1.25), (-1.12, 2.03, 1.58)], 0.018).sq((-1.125, 2.03, 1.62), (0.05, 0.02, 0.07))
          .pipe([(-1.03, 2.0, 1.25), (-0.96, 1.98, 1.52)], 0.018).box((-0.95, 1.98, 1.57), (0.08, 0.02, 0.10)))
    S.set(group='bowl'); S.add('bowl', M().lathe((-1.62, 1.92, 1.07), [(0.10, 0), (0.24, 0.13), (0.22, 0.13)]))
    for (dx, dy, dz), pn in (((-0.07, 0, 0.19), 'fruit_a'), ((0.08, 0.03, 0.19), 'fruit_b'), ((0.0, -0.06, 0.26), 'fruit_c')):
        S.add(pn, M().sq((-1.62 + dx, 1.92 + dy, 1.07 + dz), (0.085, 0.085, 0.08)), group='fruit' + pn)
    # shelf + rail over the stove
    S.set(group='kshelf')
    S.add('top', M().slab(0.0, 1.42, NE - 0.34, NE, 2.72, 2.79).box((0.12, NE - 0.14, 2.62), (0.05, 0.24, 0.20)).box((1.30, NE - 0.14, 2.62), (0.05, 0.24, 0.20)))
    S.set(group='jars'); jars(S, (0.18, NE - 0.17, 2.79), X, 5, rs)
    S.part('rail', 'metal', (1, 2, 3), seam=False); S.part('pan', 'copper', (1, 2, 3, 4), seam=False); S.part('mitt', 'fabric_rose', (1, 2, 3), seam=False)
    S.set(group='rail'); S.add('rail', M().pipe([(0.05, NE - 0.08, 2.30), (1.40, NE - 0.08, 2.30)], 0.02))
    S.set(group='pan1'); S.add('pan', M().cyl((0.35, NE - 0.10, 1.92), 0.17, 0.05, rot=(90, 0, 0)).pipe([(0.35, NE - 0.10, 2.06), (0.35, NE - 0.10, 2.30)], 0.02))
    S.set(group='pan2'); S.add('pan', M().cyl((0.78, NE - 0.10, 1.98), 0.12, 0.05, rot=(90, 0, 0)).pipe([(0.78, NE - 0.10, 2.08), (0.78, NE - 0.10, 2.30)], 0.02))
    S.set(group='mitt'); S.add('mitt', M().sq((1.18, NE - 0.10, 2.06), (0.10, 0.04, 0.17), e1=0.7).sq((1.09, NE - 0.10, 2.02), (0.04, 0.035, 0.07)))
    # fridge
    S.part('fridge', 'fabric_rose', (1, 2, 3, 4), seam=False); S.part('fridge_h', 'metal', (2, 3, 4, 5), seam=False)
    S.part('magnet', 'gold', (2, 3), thin=0.3, prio=3, noline=True); S.part('note', 'fabric_cream', (3, 4), thin=0.3, prio=3, noline=True)
    S.set(group='fridge'); fx0, fx1, fy0 = 1.52, 2.32, 1.52
    S.add('fridge', M().sq(((fx0 + fx1) / 2, (fy0 + NE) / 2, 1.38), ((fx1 - fx0) / 2, (NE - fy0) / 2, 1.34), e1=0.22, e2=0.22))
    S.set(group='fridge_d')
    S.add('fridge_h', M().box((fx0 + 0.12, fy0 - 0.03, 1.55), (0.04, 0.05, 0.42), bevel=0.012).box((fx0 + 0.12, fy0 - 0.03, 2.28), (0.04, 0.05, 0.22), bevel=0.012)
          .box(((fx0 + fx1) / 2, fy0 - 0.005, 1.98), (fx1 - fx0 - 0.06, 0.02, 0.025)))
    S.add('note', M().box((fx1 - 0.26, fy0 - 0.012, 2.34), (0.16, 0.01, 0.18))); S.add('magnet', M().box((fx1 - 0.26, fy0 - 0.02, 2.43), (0.05, 0.01, 0.045)))
    plant(S, ((fx0 + fx1) / 2, (fy0 + NE) / 2 + 0.05, 2.72), pot=0.16, h=0.22, kind='trail', seed=12, ramp='fabric_cream')
    # NW wall: open shelves with crockery, a low cabinet with a rice cooker, a clock
    S.part('plate', 'fabric_cream', (2, 3, 4), seam=True); S.part('mugs', 'fabric_quilt', (1, 2, 3), seam=False); S.part('mugs_b', 'fabric_rose', (1, 2, 3), seam=False)
    for z in (2.15, 2.85):
        S.set(group='nwshelf%g' % z)
        S.add('top', M().slab(NW, NW + 0.36, -1.75, 0.35, z, z + 0.07).box((NW + 0.15, -1.55, z - 0.10), (0.26, 0.05, 0.20)).box((NW + 0.15, 0.15, z - 0.10), (0.26, 0.05, 0.20)))
    S.set(group='plates')
    for i in range(5): S.add('plate', M().cyl((NW + 0.19, -1.40, 2.22 + i * 0.035), 0.17, 0.03))
    for i in range(3): S.add('bowl', M().lathe((NW + 0.19, -0.88, 2.22 + i * 0.06), [(0.07, 0), (0.15, 0.09), (0.14, 0.09)]), group='bowls')
    for i, pn in enumerate(('mugs', 'mugs_b', 'mugs')):
        S.add(pn, M().cyl((NW + 0.19, -0.42 + i * 0.25, 2.22), 0.075, 0.15).box((NW + 0.19, -0.42 + i * 0.25 + 0.09, 2.30), (0.025, 0.06, 0.08)), group='mug%d' % i)
    S.set(group='jars2'); jars(S, (NW + 0.19, -1.50, 2.92), Y, 4, rs, step=0.27)
    plant(S, (NW + 0.19, -0.10, 2.92), pot=0.13, h=0.18, kind='leafy', seed=14, ramp='terracotta', scale=0.7)
    S.set(group='lowcab'); ly0, ly1 = -1.80, -0.30
    S.add('toe', M().slab(NW, NW + 0.58, ly0, ly1, 0, 0.10)); S.add('cab', M().slab(NW, NW + 0.64, ly0, ly1, 0.10, 0.98))
    for i in range(2):
        a = ly0 + i * 0.75
        S.add('cab_door', M().slab(NW + 0.64, NW + 0.67, a + 0.04, a + 0.71, 0.16, 0.92, bevel=0.012)); S.add('knob', M().sq((NW + 0.69, a + (0.60 if i == 0 else 0.15), 0.76), (0.035, 0.035, 0.035)))
    S.add('top', M().slab(NW, NW + 0.71, ly0 - 0.04, ly1 + 0.04, 0.98, 1.07, bevel=0.02))
    S.part('cooker', 'fabric_cream', (1, 2, 3, 4), seam=False); S.part('cooker_b', 'fabric_rose', (1, 2, 3), seam=False)
    S.set(group='cooker'); S.add('cooker', M().sq((NW + 0.36, -1.32, 1.27), (0.24, 0.24, 0.21), e1=0.45))
    S.add('cooker_b', M().box((NW + 0.59, -1.32, 1.22), (0.03, 0.14, 0.08), bevel=0.01).sq((NW + 0.36, -1.32, 1.49), (0.08, 0.08, 0.03)))
    plant(S, (NW + 0.34, -0.66, 1.07), pot=0.14, h=0.20, kind='leafy', seed=17, ramp='terracotta', scale=0.75, leaf='bamboo')
    S.part('clock', 'fabric_cream', (2, 3, 4), sh='tex', tex=('const', 0.8), seam=False, gmax=0); S.part('clock_rim', 'walnut', (2, 3, 4), seam=False)
    S.part('clock_hand', 'ink', (0, 0), thin=0.2, prio=5, noline=True)
    S.set(group='clock'); cy_, cz_ = 1.20, 3.05
    S.add('clock_rim', M().cyl((NW, cy_, cz_), 0.30, 0.06, rot=(0, 90, 0))); S.add('clock', M().cyl((NW + 0.06, cy_, cz_), 0.25, 0.01, rot=(0, 90, 0)))
    S.add('clock_hand', M().box((NW + 0.075, cy_, cz_ + 0.08), (0.01, 0.03, 0.17)).box((NW + 0.075, cy_ + 0.06, cz_), (0.01, 0.13, 0.03)))
    # floor mat
    S.part('kmat', 'fabric_rose', (1, 2, 3), tex=('stripe', X, 6 * PXH), tstep=(0.5, 2.0), seam=False)
    S.set(group='kmat'); S.add('kmat', M().slab(-1.35, 0.55, 0.85, 1.48, 0.0, 0.025, bevel=0.01))
    bear_set(S, 'set_sit_a/anim_sit_a.json', 'snack', 'snack_munch_A.png')
    S.render()


# ================================================================ 3. tea room
def tearoom():
    S = Scene('scene_tearoom', 'Tea room: tatami, glowing shoji screens, a round window, an alcove with a scroll')
    a = 2.25
    NW, NE, HT = -a - 0.14, a + 0.14, 3.9
    S.part('beam', 'walnut', (2, 3, 4, 5), tex=('noise', 3.0, (1, 1, 8), 2.0), amp=0.10, seam=False)
    S.part('shoji', 'paper', (3, 4, 5), sh='tex', tex=('grad', Z, -0.6, 3.6), gmax=0, shadow=True, seam=False)
    S.part('koshi', 'oak', (2, 3, 4), tex=('stripe', X, 5 * PXH), tstep=(0.2, 2.0), seam=False)
    S.part('kumiko', 'walnut', (3, 4), thin=0.26, prio=4, noline=True)
    S.set(group='beams')
    S.add('beam', wbox(wbox(wbox(M(), 'y', NE, -1.64, a + 0.14, 3.05, 3.22, -0.06, WT), 'y', NE, -1.64, a + 0.14, 0.0, 0.13, -0.06, WT),
                       'y', NE, -1.64, -1.50, 0.0, 3.05, -0.06, WT))      # the frame the shoji screens slide in
    pw = (a + 0.14 + 1.50) / 3
    for i in range(3):
        p0 = -1.50 + i * pw; p1 = p0 + pw; S.set(group='shoji%d' % i); d = 0.04 if i % 2 else 0.10
        fr = M()
        for r in ((p0, p0 + 0.07, 0.13, 3.05), (p1 - 0.07, p1, 0.13, 3.05), (p0, p1, 2.96, 3.05), (p0, p1, 0.13, 0.22), (p0, p1, 0.78, 0.85)):
            wbox(fr, 'y', NE, r[0], r[1], r[2], r[3], d - 0.04, d + 0.03)
        S.add('beam', fr)
        S.add('koshi', wbox(M(), 'y', NE, p0 + 0.07, p1 - 0.07, 0.22, 0.78, d, d + 0.02))
        S.add('shoji', wbox(M(), 'y', NE, p0 + 0.07, p1 - 0.07, 0.85, 2.96, d, d + 0.02))
        k = M()
        for j in range(1, 3): aa = p0 + 0.07 + (pw - 0.14) * j / 3; wbox(k, 'y', NE, aa - 0.02, aa + 0.02, 0.85, 2.96, d - 0.012, d)
        for j in range(1, 6): zz = 0.85 + 2.11 * j / 6; wbox(k, 'y', NE, p0 + 0.07, p1 - 0.07, zz - 0.02, zz + 0.02, d - 0.012, d)
        S.add('kumiko', k)
    S.glow_sun((0.25, -1, -0.95)); m = M(); B = 40
    for r in ((-B, -1.50, -B, B), (-1.50, B, 2.96, B), (-1.50, B, -B, 0.85)): wbox(m, 'y', NE, r[0], r[1], r[2], r[3], WT + 0.3, WT + 0.32)
    S.blocker(m)
    for o in S.objs:                                    # the paper lets the outside light through
        if o['ob'].name.startswith(('shoji', 'kumiko')): o['ob'].visible_shadow = False
    # round window on the NW wall
    S.part('maru_sky', 'sky_dusk', (2, 3, 4, 5, 6), sh='tex', tex=('grad', Z, 3.35, 1.75), dither=0.16, gmax=0, noline=True, seam=False)
    S.part('maru_branch', 'pine', (0, 1), sh='tex', gmax=0, noline=True, seam=False)
    S.part('maru_moon', 'moon', (2, 3), sh='tex', gmax=0, noline=True, seam=False)
    S.set(group='maru'); my, mz, mr = 0.55, 2.52, 0.74
    S.add('beam', M().lathe((NW - 0.02, my, mz), [(mr, 0), (mr + 0.10, 0), (mr + 0.10, 0.10), (mr, 0.10), (mr, 0)], rot=(0, 90, 0), cap=False, segs=36))
    S.add('maru_sky', M().cyl((NW + 0.005, my, mz), mr, 0.012, rot=(0, 90, 0), segs=36))
    S.add('maru_moon', M().cyl((NW + 0.02, my + 0.22, mz + 0.26), 0.17, 0.006, rot=(0, 90, 0)))
    br = M().pipe([(NW + 0.03, my - mr + 0.02, mz - 0.40), (NW + 0.03, my - 0.25, mz - 0.22), (NW + 0.03, my + 0.10, mz - 0.28), (NW + 0.03, my + 0.42, mz - 0.10)], 0.025, r2=0.012)
    for dy, dz, r in ((-0.30, -0.14, 0.13), (-0.02, -0.18, 0.12), (0.22, -0.12, 0.11), (0.44, -0.03, 0.09), (-0.50, -0.26, 0.10)):
        br.sq((NW + 0.03, my + dy, mz + dz), (0.012, r, r * 0.42))
    S.add('maru_branch', br)
    kb = M()
    for dy in (-0.28, 0.28): kb.box((NW + 0.05, my + dy, mz), (0.02, 0.035, 2 * mr))
    for dz in (-0.28, 0.28): kb.box((NW + 0.05, my, mz + dz), (0.02, 2 * mr, 0.035))
    S.part('maru_bar', 'walnut', (2, 3), seam=False, noline=True); S.add('maru_bar', kb)
    # alcove: a low platform, a hanging scroll, a bonsai
    S.part('toko', 'oak', (2, 3, 4, 5), tex=('noise', 2.2, (14, 1.2, 1), 3.0), amp=0.12, seam=False)
    S.part('scroll', 'paper', (2, 3, 4), seam=False); S.part('scroll_cloth', 'heri', (2, 3, 4), seam=False)
    S.part('scroll_sun', 'lacquer', (2, 3), thin=0.3, prio=4, noline=True); S.part('scroll_ink', 'stone', (1, 2), thin=0.3, prio=3, noline=True)
    S.set(group='toko'); S.add('toko', M().slab(NW, NW + 0.62, -2.20, -0.55, 0.0, 0.16, bevel=0.02))
    S.set(group='scroll'); sy, sz0, sz1 = -1.40, 1.25, 2.85
    S.add('scroll_cloth', wbox(M(), 'x', NW, sy - 0.33, sy + 0.33, sz0, sz1, -0.025, 0.0)); S.add('scroll', wbox(M(), 'x', NW, sy - 0.25, sy + 0.25, sz0 + 0.22, sz1 - 0.22, -0.035, -0.025))
    S.add('beam', M().pipe([(NW + 0.04, sy - 0.38, sz0), (NW + 0.04, sy + 0.38, sz0)], 0.035).pipe([(NW + 0.04, sy - 0.36, sz1), (NW + 0.04, sy + 0.36, sz1)], 0.025))
    S.add('scroll_sun', M().cyl((NW + 0.035, sy + 0.06, 2.30), 0.11, 0.008, rot=(0, 90, 0)))
    ink_ = M()
    for dy, dz, w, h in ((-0.08, 1.70, 0.26, 0.05), (0.00, 1.78, 0.14, 0.05), (0.04, 1.86, 0.06, 0.05), (-0.02, 1.58, 0.36, 0.035)): ink_.box((NW + 0.04, sy + dy, dz), (0.008, w, h))
    S.add('scroll_ink', ink_)
    S.part('dish', 'heri', (1, 2, 3, 4), seam=False)
    S.set(group='dish'); S.add('dish', M().lathe((NW + 0.32, -1.02, 0.16), [(0.20, 0), (0.26, 0.10), (0.24, 0.10)]))
    plant(S, (NW + 0.32, -1.02, 0.14), pot=0.10, h=0.10, kind='bonsai', leaf='pine', ramp='heri', scale=1.15, seed=2)
    # andon lantern
    S.part('andon', 'paper', (4, 5), sh='tex', tex=('grad', Z, 0.2, 1.3), gmax=0, shadow=False, seam=False)
    S.set(group='andon'); ax_, ay_ = 1.80, 1.72
    fr = M()
    for dx in (-0.2, 0.2):
        for dy in (-0.2, 0.2): fr.box((ax_ + dx, ay_ + dy, 0.62), (0.045, 0.045, 1.24))
    fr.slab(ax_ - 0.22, ax_ + 0.22, ay_ - 0.22, ay_ + 0.22, 1.20, 1.25).slab(ax_ - 0.22, ax_ + 0.22, ay_ - 0.22, ay_ + 0.22, 0.24, 0.29)
    S.add('beam', fr); S.add('andon', M().slab(ax_ - 0.18, ax_ + 0.18, ay_ - 0.18, ay_ + 0.18, 0.29, 1.20)); S.glow_point((ax_, ay_ - 0.1, 0.8), 60.0)
    # low table with a teapot and dango, in front of the bear
    S.part('chabu', 'lacquer', (1, 2, 3, 4), seam=False); S.part('teapot', 'heri', (1, 2, 3, 4), seam=False); S.part('teacup', 'fabric_cream', (2, 3, 4), seam=False)
    S.part('tea', 'pip', (2, 3), seam=False, noline=True); S.part('plate', 'fabric_cream', (2, 3, 4), seam=False)
    S.part('dango_a', 'fabric_rose', (2, 3, 4), seam=False); S.part('dango_b', 'fabric_cream', (3, 4), seam=False); S.part('dango_c', 'pip', (3, 4, 5), seam=False)
    S.part('skewer', 'straw', (2, 3), seam=False)
    S.set(group='chabu', layer='front'); tx, ty = 1.62, -0.72
    tb = M().cyl((tx, ty, 0.40), 0.62, 0.07, segs=36)
    for i in range(4): aa = 0.785 + i * 1.571; tb.box((tx + 0.40 * math.cos(aa), ty + 0.40 * math.sin(aa), 0.20), (0.08, 0.08, 0.40), bevel=0.02)
    S.add('chabu', tb)
    S.set(group='teapot'); px_, py_ = tx - 0.16, ty + 0.14
    S.add('teapot', M().lathe((px_, py_, 0.47), [(0.11, 0), (0.17, 0.07), (0.16, 0.17), (0.08, 0.22), (0.07, 0.24)])
          .pipe([(px_ + 0.13, py_, 0.56), (px_ + 0.26, py_, 0.64)], 0.035, r2=0.022).pipe([(px_, py_ - 0.14, 0.60), (px_, py_ - 0.30, 0.66)], 0.03).sq((px_, py_, 0.73), (0.035, 0.035, 0.03)))
    S.set(group='cup'); S.add('teacup', M().lathe((tx + 0.26, ty + 0.20, 0.47), [(0.05, 0), (0.08, 0.09), (0.075, 0.09)])); S.add('tea', M().cyl((tx + 0.26, ty + 0.20, 0.545), 0.065, 0.012))
    S.set(group='dango'); S.add('plate', M().lathe((tx + 0.12, ty - 0.24, 0.47), [(0.10, 0), (0.20, 0.03), (0.19, 0.03)]))
    for k, dy in enumerate((-0.07, 0.07)):
        S.add('skewer', M().pipe([(tx - 0.10, ty - 0.24 + dy, 0.54), (tx + 0.34, ty - 0.24 + dy, 0.54)], 0.012))
        for j, pn in enumerate(('dango_a', 'dango_b', 'dango_c')): S.add(pn, M().sq((tx + 0.02 + j * 0.10, ty - 0.24 + dy, 0.55), (0.055, 0.055, 0.05)), group='dg%d%d' % (k, j))
    S.set(layer='back')
    S.part('zabuton', 'fabric_rose', (1, 2, 3, 4), seam=False); S.part('zabuton2', 'heri', (2, 3, 4), seam=False)
    S.add('zabuton2', M().sq((0.4, 1.55, 0.07), (0.44, 0.44, 0.08), e1=0.5, e2=0.35), group='zab1'); S.add('zabuton', M().sq((0.43, 1.52, 0.20), (0.42, 0.42, 0.08), e1=0.5, e2=0.35, rot=(0, 0, 12)), group='zab2')
    bear_set(S, 'set_sit_a/anim_sit_a.json', 'tea', 'tea_sip_A.png')
    S.render()


# ================================================================ 4. bamboo grove (day / night)
def grove():
    S = Scene('scene_grove', 'Bamboo grove: tall bamboo, a stone lantern, a koi pond; fireflies at night')
    S.variant('day', amb=0); S.variant('night', amb=-1)
    rs = random.Random(3); R = 2.9
    tuft = F.moss_floor(S, R, extras=False, ground=False)
    S.part('cane', 'bamboo', (1, 2, 3, 4, 5), seam=False); S.part('node', 'bamboo', (1, 2), seam=False)
    S.part('bleaf', 'bamboo', (2, 3, 4, 5), seam=False, tex=('noise', 3.0, (1, 1, 1), 1.0), amp=0.5)
    canes = [(-2.25, 0.2, 5.0), (-1.95, 0.95, 5.6), (-1.5, 1.55, 4.7), (-2.35, 1.35, 5.3), (-0.75, 2.1, 5.5), (-0.1, 2.35, 4.8), (0.7, 2.25, 5.7),
             (1.35, 1.95, 5.0), (1.95, 1.35, 5.4), (0.25, 1.95, 4.3), (-1.15, 2.35, 5.1), (2.3, 0.55, 4.6)]
    for i, (x, y, h) in enumerate(canes):
        S.set(group='cane%d' % i); r = rs.uniform(0.075, 0.115); lx, ly = rs.uniform(-0.05, 0.05), rs.uniform(-0.05, 0.05)
        seg = rs.uniform(0.50, 0.62); n = int(h / seg); c = M(); nd = M()
        for j in range(n):
            z0 = j * seg; px, py = x + lx * z0, y + ly * z0; rr = r * (1 - 0.35 * j / n)
            c.cyl((px, py, z0), rr, seg, segs=12, rot=(math.degrees(-ly), math.degrees(lx), 0)); nd.cyl((px, py, z0 - 0.02), rr * 1.16, 0.045, segs=12)
        S.add('cane', c); S.add('node', nd)
        lv = M()
        for j in range(int(n * 0.45), n + 1):
            z0 = j * seg; px, py = x + lx * z0, y + ly * z0
            for kk in range(3 if j < n else 5):
                aa = rs.uniform(0, 6.28); ln = rs.uniform(0.34, 0.6); dr = rs.uniform(25, 60)
                cx_, cy_ = px + 0.30 * ln * math.cos(aa), py + 0.30 * ln * math.sin(aa)
                lv.sq((cx_, cy_, z0 - 0.10 * ln + rs.uniform(0, 0.2)), (ln * 0.5, 0.055, 0.02), rot=(rs.uniform(-25, 25), dr, math.degrees(aa)))
        S.add('bleaf', lv, group='bleaf%d' % i)
    # stone lantern
    S.part('lantern', 'stone', (1, 2, 3, 4, 5), tex=('noise', 6.0, (1, 1, 1), 2.0), amp=0.14, seam=False)
    S.part('lmoss', 'mossy', (3, 4, 5), seam=False, noline=True)
    S.part('lfire_off', 'stone', (1, 1), sh='tex', gmax=0, seam=False); S.part('lfire_on', 'flame', (3, 4, 5), sh='tex', tex=('grad', Z, 1.0, 1.5), gmax=0, seam=False, shadow=False)
    S.set(group='lantern'); lx_, ly_ = -1.45, -0.35
    S.add('lantern', M().lathe((lx_, ly_, 0.0), [(0.36, 0), (0.36, 0.10), (0.26, 0.20), (0.15, 0.26), (0.13, 0.88), (0.30, 0.98), (0.30, 1.06)], segs=6))
    fb = M()
    for i in range(4): aa = 0.785 + i * 1.571; fb.box((lx_ + 0.20 * math.cos(aa), ly_ + 0.20 * math.sin(aa), 1.26), (0.09, 0.09, 0.40))
    S.add('lantern', fb, group='lantern_fb')
    S.add('lfire_off', M().box((lx_, ly_, 1.26), (0.30, 0.30, 0.36)), var='day'); S.add('lfire_on', M().box((lx_, ly_, 1.26), (0.30, 0.30, 0.36)), var='night')
    S.add('lantern', M().lathe((lx_, ly_, 1.46), [(0.50, 0), (0.44, 0.07), (0.14, 0.30), (0.10, 0.32), (0.11, 0.40), (0.03, 0.50)], segs=6), group='lantern_roof')
    S.add('lmoss', M().sq((lx_ + 0.05, ly_ - 0.03, 1.74), (0.20, 0.20, 0.06)).sq((lx_ + 0.2, ly_ - 0.2, 0.12), (0.16, 0.12, 0.07)))
    S.glow_point((lx_ + 0.25, ly_ - 0.25, 1.25), 55.0, var='night')
    # koi pond
    S.part('pond', 'water', (2, 3, 4), tex=('noise', 2.5, (2, 2, 1), 2.0), amp=0.9, sh='tex', gmax=0, seam=False, noline=True)
    S.part('pond_rim', 'stone', (2, 3, 4, 5), tex=('noise', 6.0, (1, 1, 1), 2.0), amp=0.14, seam=True)
    S.part('lily', 'pip', (3, 4), thin=0.4, prio=4, noline=True); S.part('lotus', 'fabric_rose', (3, 4), thin=0.25, prio=6, noline=True)
    S.part('koi', 'carrot', (2, 3), thin=0.3, prio=5, noline=True); S.part('koi_w', 'snow', (3, 4), thin=0.3, prio=5, noline=True)
    S.set(group='pond'); px_, py_ = 1.25, -1.15
    S.add('pond', M().blob_poly((px_, py_, 0.03), 0.78, 0.03, wob=0.10, seed=5.0, squash=0.82))
    for i in range(11):
        aa = 2 * math.pi * i / 11; wv = 1 + 0.10 * (math.sin(3 * aa + 5.0) * 0.6 + math.sin(5 * aa + 10.5) * 0.4)
        S.add('pond_rim', M().sq((px_ + 0.80 * wv * math.cos(aa), py_ + 0.80 * 0.82 * wv * math.sin(aa), 0.05), (rs.uniform(0.15, 0.22), rs.uniform(0.12, 0.17), rs.uniform(0.07, 0.11)), e1=0.6, rot=(0, 0, math.degrees(aa) + 90)))
    S.add('lily', M().cyl((px_ - 0.30, py_ + 0.22, 0.045), 0.15, 0.01).cyl((px_ + 0.28, py_ - 0.24, 0.045), 0.12, 0.01).cyl((px_ - 0.08, py_ - 0.36, 0.045), 0.09, 0.01))
    S.add('lotus', M().sq((px_ - 0.30, py_ + 0.22, 0.08), (0.055, 0.055, 0.04)))
    S.add('koi', M().sq((px_ + 0.18, py_ + 0.16, 0.04), (0.14, 0.05, 0.02), rot=(0, 0, 30))); S.add('koi_w', M().sq((px_ + 0.02, py_ - 0.08, 0.04), (0.11, 0.04, 0.02), rot=(0, 0, -50)))
    # mushrooms + ferns near the canes
    S.part('shroom', 'lacquer', (2, 3, 4), seam=False); S.part('shroom_s', 'fabric_cream', (2, 3, 4), seam=False)
    for i, (x, y, s) in enumerate(((-0.55, 1.35, 1.0), (-0.36, 1.48, 0.7), (1.75, 0.55, 0.8))):
        S.set(group='shroom%d' % i); S.add('shroom_s', M().cyl((x, y, 0), 0.04 * s, 0.14 * s)); S.add('shroom', M().sq((x, y, 0.15 * s), (0.11 * s, 0.11 * s, 0.07 * s)))
    S.set(group='floor')
    for x, y in ((-2.0, 0.6), (-1.2, 1.9), (0.5, 2.2), (1.7, 1.6), (2.2, 0.9), (-0.4, 2.2), (1.1, 2.2)): tuft(x, y, 1.5)
    # fireflies
    S.part('firefly', 'gold', (3, 4), sh='tex', tex=('noise', 30.0, (1, 1, 1), 0.0), thin=0.14, prio=9, noline=True, shadow=False, gmax=0)
    ff = M()
    for _ in range(16):
        aa = rs.uniform(0, 6.28); d = rs.uniform(0.6, 2.6); ff.sq((d * math.cos(aa), d * math.sin(aa), rs.uniform(0.5, 3.4)), (0.035, 0.035, 0.035))
    S.add('firefly', ff, var='night', group='fireflies')
    S.bear = dict(json='anim_p/anim_p.json', clip='sit_floor', dir='SE', at=[0, 0]); S.bear_shadow(r=(0.85, 0.85, 0.16))
    S.render()


# ================================================================ 5. fireside library
def library():
    S = Scene('scene_library', 'Fireside library: a wall of books, a brick fireplace, a deep rug')
    S.variant('evening', amb=-1)
    rs = random.Random(9); ox, oy = -0.30, -0.20
    x0, x1, y0, y1 = ox - 2.2, ox + 2.3, oy - 2.2, oy + 2.3
    NW, NE, HT = x0, y1, 4.3
    # bookshelf wall (NW)
    S.part('case', 'walnut', (2, 3, 4, 5), tex=('noise', 2.2, (14, 1.2, 1), 3.0), amp=0.12, seam=False); S.part('case_back', 'walnut', (1, 2), seam=False)
    S.set(group='bookcase'); by0, by1, bd, bh = y0 + 0.10, y0 + 3.05, 0.52, 3.55; m = M(); nlev = 5
    lev = [0.0] + [0.10 + (bh - 0.2) * i / nlev for i in range(1, nlev)] + [bh - 0.10]
    for z in lev: m.slab(NW, NW + bd, by0, by1, z, z + 0.08)
    for yy in (by0, (by0 + by1) / 2 - 0.04, by1 - 0.08): m.slab(NW, NW + bd, yy, yy + 0.08, 0, bh)
    m.slab(NW, NW + bd + 0.04, by0 - 0.04, by1 + 0.04, bh, bh + 0.10, bevel=0.02)
    S.add('case', m); S.add('case_back', M().slab(NW, NW + 0.05, by0, by1, 0, bh))
    S.part('vase', 'heri', (1, 2, 3, 4), seam=False); S.part('candle', 'fabric_cream', (2, 3, 4), seam=False)
    S.part('flame_o', 'flame', (1, 2), sh='tex', gmax=0, noline=True, shadow=False); S.part('flame_m', 'flame', (2, 3), sh='tex', gmax=0, noline=True, shadow=False)
    S.part('flame_i', 'flame', (4, 5), sh='tex', gmax=0, noline=True, shadow=False)
    for li, z in enumerate(lev[:-1]):
        for bi, (ya, yb) in enumerate(((by0 + 0.08, (by0 + by1) / 2 - 0.04), ((by0 + by1) / 2 + 0.04, by1 - 0.08))):
            S.set(group='books%d%d' % (li, bi)); room = lev[li + 1] - z - 0.10
            if (li, bi) == (3, 1):
                plant(S, (NW + 0.27, ya + 0.30, z + 0.08), pot=0.13, h=0.17, kind='trail', seed=31, ramp='fabric_cream'); S.set(group='books%d%d' % (li, bi))
                books(S, (NW + 0.28, ya + 0.62, z + 0.08), Y, yb - ya - 0.62, 0.34, rs, hmin=room * 0.6, hmax=room * 0.95)
            elif (li, bi) == (2, 0):
                books(S, (NW + 0.28, ya, z + 0.08), Y, (yb - ya) * 0.6, 0.34, rs, hmin=room * 0.6, hmax=room * 0.95)
                S.add('vase', M().lathe((NW + 0.27, yb - 0.28, z + 0.08), [(0.08, 0), (0.14, 0.14), (0.07, 0.32), (0.09, 0.38)]), group='vase')
            else: books(S, (NW + 0.28, ya, z + 0.08), Y, yb - ya, 0.34, rs, hmin=room * 0.6, hmax=room * 0.95)
    # fireplace (NE)
    S.part('brick', 'brick', (1, 2, 3, 4), tex=('grid', X, 9 * PXH, Z, 5 * PXV, 0.035), tstep=(0.55, 2.0), seam=False)
    S.part('hearth', 'stone', (1, 2, 3), seam=False); S.part('firebox', 'void', (0, 1), seam=False, gmax=0)
    S.part('mantel', 'walnut', (2, 3, 4, 5), seam=False); S.part('log', 'soil', (1, 2, 3, 4), seam=True)
    fx0, fx1, fd = ox + 0.05, ox + 1.95, 0.50
    S.set(group='fireplace'); m = M()
    m.slab(fx0, fx0 + 0.42, NE - fd, NE, 0, 1.75).slab(fx1 - 0.42, fx1, NE - fd, NE, 0, 1.75).slab(fx0, fx1, NE - fd, NE, 1.30, 1.75)
    m.slab(fx0 + 0.25, fx1 - 0.25, NE - 0.30, NE, 1.75, HT)
    S.add('brick', m); S.add('firebox', M().slab(fx0 + 0.42, fx1 - 0.42, NE - 0.12, NE, 0, 1.30))
    S.add('hearth', M().slab(fx0 - 0.12, fx1 + 0.12, NE - fd - 0.36, NE - fd + 0.02, 0, 0.09, bevel=0.02), group='hearth')
    S.add('mantel', M().slab(fx0 - 0.14, fx1 + 0.14, NE - fd - 0.12, NE, 1.75, 1.87, bevel=0.025), group='mantel')
    fc = (fx0 + fx1) / 2; fy = NE - 0.30
    S.set(group='logs')
    for i, (dx, rot) in enumerate(((-0.22, 12), (0.22, -12), (0.0, 0))):
        S.add('log', M().cyl((fc + dx, fy - 0.26, 0.11 + (0.16 if i == 2 else 0)), 0.09, 0.56, rot=(-90, 0, rot), segs=10))
    S.set(group='fire')
    for pn, sc, zz in (('flame_o', 1.0, 0.0), ('flame_m', 0.68, -0.03), ('flame_i', 0.38, -0.06)):
        f = M()
        for dx, h, r in ((0.0, 0.62, 0.23), (-0.22, 0.42, 0.15), (0.21, 0.46, 0.16)):
            f.lathe((fc + dx * sc ** 0.5 + (1 - sc) * 0.16, fy - (1 - sc) * 0.16, 0.30 + zz), [(0.02, 0), (r * sc, h * sc * 0.22), (r * sc * 0.8, h * sc * 0.5), (0.015, h * sc)], segs=12)
        S.add(pn, f)
    S.glow_point((fc, fy - 0.45, 0.55), 150.0)
    # on the mantel: candles, a clock, a framed picture above
    S.set(group='mantel_things')
    for i, dx in enumerate((-0.72, -0.58)):
        S.add('candle', M().cyl((fc + dx, NE - 0.36, 1.87), 0.04, 0.26 - i * 0.07), group='candle%d' % i)
        S.add('flame_m', M().sq((fc + dx, NE - 0.36, 2.19 - i * 0.07), (0.03, 0.03, 0.055)), group='cflame%d' % i)
    S.part('clock_rim', 'gold', (1, 2, 3), seam=False); S.part('clock', 'fabric_cream', (3, 4), sh='tex', tex=('const', 0.8), gmax=0, seam=False)
    S.add('clock_rim', M().sq((fc + 0.55, NE - 0.34, 2.07), (0.20, 0.10, 0.21), e1=0.6), group='mclock'); S.add('clock', M().cyl((fc + 0.55, NE - 0.445, 2.09), 0.12, 0.012, rot=(90, 0, 0)), group='mclock')
    plant(S, (fc - 0.05, NE - 0.34, 1.87), pot=0.11, h=0.15, kind='succulent', ramp='fabric_cream', seed=40)
    picture(S, 'y', NE - 0.30, fc, 3.0, 0.9, 0.62, 'hill', name='bigpic')
    picture(S, 'y', NE, x1 - 0.55, 2.7, 0.42, 0.56, 'leaf'); picture(S, 'y', NE, fx0 - 0.75, 2.75, 0.5, 0.4, 'hill')
    # rug, armchair, floor stack of books, basket of logs
    rug(S, (ox + 0.55, oy + 0.15), 1.85, rings=(('lacquer', 3), ('gold', 3), ('lacquer', 3), ('heri', 3), ('lacquer', 4)), squash=0.9)
    S.part('pouf', 'fabric_sage', (1, 2, 3, 4), seam=False); S.part('throw', 'gold', (1, 2, 3, 4), seam=False)
    S.add('pouf', M().sq((x1 - 0.55, oy + 0.1, 0.24), (0.40, 0.40, 0.24), e1=0.55, e2=0.8), group='pouf')
    S.add('throw', M().sq((x1 - 0.55, oy + 0.1, 0.49), (0.30, 0.22, 0.05), e1=0.6, e2=0.5, rot=(0, 0, 20)), group='throw')
    S.set(group='stack'); sx, sy_ = ox + 1.55, oy - 1.45; z = 0.0
    for i in range(5):
        h = rs.uniform(0.09, 0.14); S.add('book%d' % rs.randrange(len(BOOKS)), M().box((sx + rs.uniform(-0.03, 0.03), sy_ + rs.uniform(-0.03, 0.03), z + h / 2), (0.46, 0.34, h), bevel=0.012, rot=(0, 0, rs.uniform(-14, 14)))); z += h
    S.add('mug', M().cyl((sx, sy_, z), 0.07, 0.13), group='mug') if 'mug' in S.parts else (S.part('mug', 'fabric_rose', (1, 2, 3), seam=False), S.add('mug', M().cyl((sx, sy_, z), 0.07, 0.13), group='mug'))
    S.part('basket', 'straw', (1, 2, 3, 4), tex=('stripe', Z, 3 * PXV), tstep=(0.3, 2.0), seam=False)
    S.set(group='basket'); bx_, by_ = fx1 + 0.42, NE - 0.50
    S.add('basket', M().lathe((bx_, by_, 0), [(0.24, 0), (0.30, 0.34), (0.28, 0.36)]))
    for i, (dx, dy) in enumerate(((-0.08, 0.0), (0.09, 0.05), (0.0, -0.09))): S.add('log', M().cyl((bx_ + dx, by_ + dy, 0.12), 0.08, 0.42, rot=(rs.uniform(-10, 10), rs.uniform(-10, 10), 0), segs=10), group='blog%d' % i)
    bear_set(S, 'anim_b/anim_b.json', 'read', 'read_A.png')
    S.render()


SCENES = {'nook': nook, 'kitchen': kitchen, 'tearoom': tearoom, 'grove': grove, 'library': library}

if '-P' in sys.argv and os.path.basename(sys.argv[sys.argv.index('-P') + 1]) == 'scenes.py':
    for n, f in SCENES.items():
        if only(n): f()
