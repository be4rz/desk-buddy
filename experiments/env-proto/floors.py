"""Floor prototypes: five themed floor islands (diorama slabs), each rendered bare and with the bear's contact shadow.

    sh run.sh floors [--only tatami,parquet,moss,tile,snow] [--bare]
"""
import sys, os, math, random
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from envkit import M, Scene, only, PXH, PXV

X, Y, Z = (1, 0, 0), (0, 1, 0), (0, 0, 1)


def _done(S, shadow):
    if shadow: S.bear_shadow()
    S.bear = dict(clip='idle', dir='S', at=[0, 0])
    S.render()


# ---------------------------------------------------------------- shared builders (scenes.py reuses them)
def tatami_floor(S, a=2.25):
    """a four-and-a-half mat room (4.5 x 4.5, centred) on a dark wood sill."""
    weave = lambda d: ('stripe', d, 4 * PXH)
    for ax, d in (('x', Y), ('y', X)):
        S.part('mat_' + ax, 'tatami', (2, 3, 4, 5), tex=('mix', weave(d), ('noise', 6.0, (1, 1, 1), 2.0, 9.0), 0.35),
               tstep=(0.24, 2.0), rnd=0.06)
    S.part('heri', 'heri', (1, 2, 3), seam=False)
    S.part('sill', 'walnut', (2, 3, 4), tex=('noise', 3.0, (1, 8, 1), 2.0), amp=0.10)
    S.set(group='floor')
    u = a / 1.5
    mats = [(-a, -a, 2 * u, u, 'x'), (a - u, -a, u, 2 * u, 'y'), (-a + u, a - u, 2 * u, u, 'x'), (-a, -a + u, u, 2 * u, 'y'),
            (-a + u, -a + u, u, u, 'x')]
    hw = 3 * PXH
    for x0, y0, w, d, ax in mats:
        S.add('mat_' + ax, M().slab(x0, x0 + w, y0, y0 + d, -0.10, 0.0))
        for side in (0, 1):                 # cloth borders on the two long edges
            if ax == 'x': yy = y0 + (d - hw) * side; S.add('heri', M().slab(x0, x0 + w, yy, yy + hw, -0.02, 0.012))
            else: xx = x0 + (w - hw) * side; S.add('heri', M().slab(xx, xx + hw, y0, y0 + d, -0.02, 0.012))
    S.set(group='sill')
    S.add('sill', M().slab(-a - 0.14, a + 0.14, -a - 0.14, a + 0.14, -0.30, -0.04, bevel=0.03))


def parquet_floor(S, x0=-2.3, x1=2.3, y0=-2.3, y1=2.3, ramp='oak', plank=7):
    """herringbone parquet with a border course, on a darker base."""
    grain = lambda st: ('noise', 2.2, st, 3.0, 40.0)
    S.part('plank_x', ramp, (2, 3, 4, 5), tex=grain((1.2, 14, 1)), amp=0.16, rnd=0.34)
    S.part('plank_y', ramp, (2, 3, 4, 5), tex=grain((14, 1.2, 1)), amp=0.16, rnd=0.34)
    S.part('pq_border', ramp, (1, 2, 3, 4), tex=grain((6, 6, 1)), amp=0.10, rnd=0.10)
    S.part('pq_base', 'walnut', (1, 2, 3))
    S.set(group='floor')
    w = plank * PXH; L = 4 * w; b = 0.20
    ix0, ix1, iy0, iy1 = x0 + b, x1 - b, y0 + b, y1 - b
    def put(part, ax0, ax1, ay0, ay1):
        ax0, ax1, ay0, ay1 = max(ax0, ix0), min(ax1, ix1), max(ay0, iy0), min(ay1, iy1)
        if ax1 - ax0 > 0.02 and ay1 - ay0 > 0.02: S.add(part, M().slab(ax0, ax1, ay0, ay1, -0.08, 0.0))
    for n in range(-40, 40):
        for m in range(-12, 12):
            ox, oy = n * w + m * L, n * w - m * L
            if not (x0 - 2 * L < ox < x1 + L and y0 - 2 * L < oy < y1 + 2 * L): continue
            put('plank_x', ox, ox + L, oy, oy + w); put('plank_y', ox + L, ox + L + w, oy + w - L, oy + w)
    for sx0, sx1, sy0, sy1 in ((x0, x1, y0, iy0), (x0, x1, iy1, y1), (x0, ix0, iy0, iy1), (ix1, x1, iy0, iy1)):
        S.add('pq_border', M().slab(sx0, sx1, sy0, sy1, -0.08, 0.004))
    S.set(group='sill')
    S.add('pq_base', M().slab(x0 - 0.06, x1 + 0.06, y0 - 0.06, y1 + 0.06, -0.30, -0.05, bevel=0.03))


def tile_floor(S, n=6, t=0.75, a='tile_cream', b='tile_sage', cx=0.0, cy=0.0):
    """checker tiles with little diamond insets at the corners, on a stone base."""
    S.part('tile_a', a, (2, 3, 4, 5), rnd=0.05)
    S.part('tile_b', b, (2, 3, 4, 5), rnd=0.05)
    S.part('tile_inset', 'terracotta', (2, 3), thin=0.45, prio=4, noline=True)
    S.part('tile_base', 'stone', (1, 2, 3))
    S.set(group='floor'); h = n * t / 2
    for i in range(n):
        for j in range(n):
            x, y = cx - h + i * t, cy - h + j * t
            S.add('tile_a' if (i + j) % 2 else 'tile_b', M().slab(x, x + t, y, y + t, -0.08, 0.0))
    ins = M()
    for i in range(1, n):
        for j in range(1, n): ins.box((cx - h + i * t, cy - h + j * t, 0.004), (0.15, 0.15, 0.01), rot=(0, 0, 45))
    S.add('tile_inset', ins)
    S.set(group='sill')
    S.add('tile_base', M().slab(cx - h - 0.07, cx + h + 0.07, cy - h - 0.07, cy + h + 0.07, -0.30, -0.05, bevel=0.03))


def moss_floor(S, r=2.45, seed=1.3, extras=True, ground=True):
    """a mossy garden island: moss over soil, stepping stones, fallen bamboo leaves, clover, tiny flowers, tufts."""
    rs = random.Random(7)
    S.part('moss', 'mossy', (2, 3, 4, 5), tex=('mix', ('noise', 1.3, (1, 1, 1), 3.0), ('noise', 9.0, (1, 1, 1), 2.0), 0.35),
           amp=0.62, seam=False, thresh=(0.50, 0.66, 0.88))
    S.part('soil', 'soil', (1, 2, 3), tex=('noise', 4.0, (1, 1, 5), 2.0), amp=0.30, seam=False)
    S.part('stone', 'stone', (2, 3, 4, 5), tex=('noise', 5.0, (1, 1, 1), 2.0), amp=0.16)
    S.part('pebble', 'stone', (3, 4, 5))
    S.part('fallen', 'bamboo', (4, 5), thin=0.34, prio=4, noline=True)
    S.part('fallen_dry', 'straw', (2, 3), thin=0.34, prio=4, noline=True)
    S.part('tuft', 'mossy', (3, 4, 5, 6), seam=False)
    S.part('clover', 'pip', (2, 3, 4), seam=False)
    S.part('petal', 'snow', (3, 4), thin=0.30, prio=6, noline=True)
    S.part('pollen', 'gold', (2, 3), thin=0.20, prio=7, noline=True)
    S.set(group='floor')
    if ground:                                  # a scene takes the tufts only (ground=False): it has no floor of its own
        S.add('moss', M().blob_poly((0, 0, 0), r, 0.14, wob=0.07, seed=seed, bevel=0.05))
        S.add('soil', M().blob_poly((0, 0, -0.10), r - 0.10, 0.34, wob=0.07, seed=seed, bevel=0.06))
    def inside(x, y, m=0.35): return math.hypot(x, y) < r * 0.93 - m
    def tuft(x, y, s=1.0):
        t = M()
        for i in range(5):
            a = rs.uniform(0, 6.28); d = rs.uniform(0, 0.07) * s; hgt = rs.uniform(0.16, 0.34) * s
            t.cyl((x + d * math.cos(a), y + d * math.sin(a), 0), 0.035 * s, hgt, r2=0.004, segs=6,
                  rot=(rs.uniform(-22, 22), rs.uniform(-22, 22), 0))
        S.add('tuft', t)
    if not extras: return tuft
    stones = [(-1.25, 0.95, 0.46, 0.36, 20), (-0.35, 1.45, 0.34, 0.28, -30), (0.95, -1.1, 0.50, 0.38, 60),
              (1.55, -0.2, 0.36, 0.30, 0), (-1.5, -0.75, 0.40, 0.30, 100)]
    S.set(group='stones')
    for x, y, a, b, rot in stones: S.add('stone', M().sq((x, y, 0.035), (a, b, 0.10), e1=0.55, rot=(0, 0, rot)))
    S.set(group='floor')
    peb = M()
    for _ in range(14):
        a = rs.uniform(0, 6.28); d = rs.uniform(0.9, r * 0.86); s = rs.uniform(0.045, 0.085)
        peb.sq((d * math.cos(a), d * math.sin(a), 0.02), (s, s * 0.8, s * 0.6))
    S.add('pebble', peb)
    for part, n in (('fallen', 16), ('fallen_dry', 7)):
        lv = M()
        for _ in range(n):
            a = rs.uniform(0, 6.28); d = rs.uniform(0.3, r * 0.82)
            lv.poly((d * math.cos(a), d * math.sin(a), 0.012), [(-0.15, 0), (0, -0.04), (0.15, 0), (0, 0.04)], 0.01,
                    rot=(0, 0, rs.uniform(0, 180)))
        S.add(part, lv)
    for cx, cy in ((0.9, 1.2), (-0.6, -1.5), (1.7, 0.9), (-1.9, 0.2)):
        c = M(); pt = M(); po = M()
        for i in range(7):
            a = rs.uniform(0, 6.28); d = rs.uniform(0, 0.24); x, y = cx + d * math.cos(a), cy + d * math.sin(a)
            for kk in range(3):
                b = a + kk * 2.094; c.sq((x + 0.045 * math.cos(b), y + 0.045 * math.sin(b), 0.035), (0.05, 0.05, 0.03))
        for i in range(3):
            a = rs.uniform(0, 6.28); d = rs.uniform(0.1, 0.3); x, y = cx + d * math.cos(a), cy + d * math.sin(a)
            for kk in range(5):
                b = kk * 1.2566; pt.sq((x + 0.05 * math.cos(b), y + 0.05 * math.sin(b), 0.10), (0.04, 0.04, 0.02))
            po.sq((x, y, 0.115), (0.03, 0.03, 0.02))
        S.add('clover', c); S.add('petal', pt); S.add('pollen', po)
    for i in range(26):
        a = 2 * math.pi * i / 26 + rs.uniform(-0.08, 0.08)
        wv = 1 + 0.07 * (math.sin(3 * a + seed) * 0.6 + math.sin(5 * a + 2.1 * seed) * 0.4)
        d = r * wv - rs.uniform(0.10, 0.32)
        tuft(d * math.cos(a), d * math.sin(a), rs.uniform(0.8, 1.25))
    for _ in range(9):
        a = rs.uniform(0, 6.28); d = rs.uniform(0.9, r * 0.75); tuft(d * math.cos(a), d * math.sin(a), 0.7)
    return tuft


def snow_floor(S, r=2.45, seed=4.0, extras=True):
    """a snow field island over blue ice: drifts, a frozen puddle, paw prints, a snowy pine sprout."""
    rs = random.Random(11)
    S.part('snow', 'snow', (1, 2, 3, 4), tex=('mix', ('noise', 1.6, (1, 1, 1), 2.0), ('dots', 9.0, 0.10), 0.25), amp=0.16,
           seam=False, thresh=(0.42, 0.60, 0.80))
    S.part('ice_side', 'ice', (1, 2, 3), tex=('stripe', Z, 4 * PXV), amp=0.25, seam=False)
    S.part('ice_top', 'ice', (3, 4), tex=('noise', 2.0, (3, 3, 1), 2.0), amp=0.30, seam=False, noline=True)
    S.part('ice_rim', 'ice', (2, 3), seam=False)
    S.part('glint', 'snow', (4, 4), thin=0.30, prio=6, noline=True)
    S.part('print', 'snow', (2, 2), thin=0.40, prio=5, noline=True)
    S.part('pine', 'pine', (1, 2, 3, 4), seam=False)
    S.part('trunk', 'walnut', (2, 3))
    S.part('pebble', 'stone', (2, 3, 4))
    S.set(group='floor')
    S.add('snow', M().blob_poly((0, 0, 0), r, 0.16, wob=0.08, seed=seed, bevel=0.07))
    S.add('ice_side', M().blob_poly((0, 0, -0.12), r - 0.12, 0.30, wob=0.08, seed=seed, bevel=0.05))
    for x, y, a, b, h in ((-1.5, 1.0, 0.75, 0.55, 0.20), (1.45, 0.9, 0.6, 0.5, 0.16), (-0.7, -1.6, 0.6, 0.42, 0.13),
                          (1.7, -0.7, 0.45, 0.36, 0.11)):
        S.add('snow', M().sq((x, y, 0.0), (a, b, h)))
    if not extras: return
    px, py = 0.95, -1.15
    S.add('ice_rim', M().blob_poly((px, py, 0.012), 0.66, 0.02, wob=0.10, seed=2.0, squash=0.8))
    S.add('ice_top', M().blob_poly((px, py, 0.02), 0.58, 0.02, wob=0.10, seed=2.0, squash=0.8))
    g = M()
    for dx, ln in ((-0.16, 0.34), (0.06, 0.20)): g.box((px + dx, py + 0.02, 0.03), (ln, 0.035, 0.01), rot=(0, 0, 45))
    S.add('glint', g)
    pr = M()
    for i in range(5):
        x, y = -1.75 + i * 0.36, -0.55 + i * 0.10 + (0.12 if i % 2 else -0.12)
        pr.sq((x, y, 0.0), (0.085, 0.07, 0.03))
        for kk in (-1, 0, 1): pr.sq((x + 0.10, y + kk * 0.055, 0.0), (0.03, 0.03, 0.03))
    S.add('print', pr)
    S.set(group='pine')
    tx, ty = -0.9, 1.55
    S.add('trunk', M().cyl((tx, ty, 0.05), 0.05, 0.2))
    for i, (rr, z, hh) in enumerate(((0.34, 0.18, 0.34), (0.26, 0.38, 0.30), (0.17, 0.58, 0.28))):
        S.add('pine', M().cyl((tx, ty, z), rr, hh, r2=0.02, segs=10))
        S.add('snow', M().sq((tx + 0.02, ty - 0.02, z + hh * 0.72), (rr * 0.42, rr * 0.42, 0.05)), group='pine')
    S.set(group='floor')
    peb = M()
    for _ in range(8):
        a = rs.uniform(0, 6.28); d = rs.uniform(1.0, r * 0.84); s = rs.uniform(0.05, 0.09)
        peb.sq((d * math.cos(a), d * math.sin(a), 0.02), (s, s * 0.8, s * 0.6))
    S.add('pebble', peb)


def tatami(sh): S = Scene('floor_tatami' + '_s' * sh, 'Tatami: a four-and-a-half mat room on a dark wood sill'); tatami_floor(S); _done(S, sh)
def parquet(sh): S = Scene('floor_parquet' + '_s' * sh, 'Honey oak herringbone parquet'); parquet_floor(S, plank=9); _done(S, sh)
def tile(sh): S = Scene('floor_tile' + '_s' * sh, 'Cream and sage checker tiles with terracotta insets'); tile_floor(S); _done(S, sh)
def moss(sh): S = Scene('floor_moss' + '_s' * sh, 'Moss garden: stepping stones, bamboo leaves, clover'); moss_floor(S); _done(S, sh)
def snow(sh): S = Scene('floor_snow' + '_s' * sh, 'Snow field over blue ice, with a frozen puddle'); snow_floor(S); _done(S, sh)


if __name__ == '__main__' or os.path.basename(sys.argv[sys.argv.index('-P') + 1] if '-P' in sys.argv else '') == 'floors.py':
    for n, f in (('tatami', tatami), ('parquet', parquet), ('tile', tile), ('moss', moss), ('snow', snow)):
        for sh in (False, True):
            if only(n) and (sh or '--bare' in sys.argv): f(sh)
