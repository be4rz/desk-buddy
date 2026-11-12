"""Stage m (gaits, aliveness p2): tiptoe, skip, shuffle (sleepy) and back_waddle, each <x>_start / loop / <x>_stop,
in all 8 directions, Stage A's 84x123 frame, no face (as walk / run). Pure Python (no bpy): rig_bear.py --stage k
--clips-module clips_m keys build(V, PXZ); verify_m.py rebuilds the same poses for its foot-slide check.

Storyboard (prototype #sb-gaits, frozen): tiptoe_start 4, tiptoe 12 (loop), tiptoe_stop 5; skip_start 4, skip 12,
skip_stop 5; shuffle_start 4, shuffle 16, shuffle_stop 6; back_start 4, back_waddle 12, back_stop 5. Hand-offs as
walk: <x>_start#0 = idle#0, <x>_start's last frame steps into the loop's frame 0, <x>_stop#0 = the loop's frame 0
(CHAINS, pixel-equal), <x>_stop's last frame = idle#0.

Root motion is Stage A's walk method: the rig stays in place, ty_px (forward, world units) is the root track and
the anim JSON's root_px moves the sprite. A planted foot (h = 0 on two neighbouring frames) slides back inside the
sprite by exactly the root step, so on the floor it stays put (no foot slide; slide_errors below, and verify_m.py
checks the rendered track against these poses). V = the walk step (2 px per frame on the iso axes): tiptoe and
shuffle 0.5 V (1 px), skip 1.5 V (3 px), back_waddle -V (moves opposite to the facing).

Rig K keeps roll = lean = 0 (tilts are head-only), so the waddle sway is a small head roll hr (<= 5 deg) with the
ears, clover and mittens. Cute rules: squash <= 4 px (< 6%), feet are the stub feet stepping (lift <= 6 px), no
stretched legs: when the body floats on the skip the feet rise with it.
"""
import math
from anim_clips import KP, REST_K, ease

HB = 2.06                       # body height (anim_clips.build), turns px of squash into sz

ORDER = ['tiptoe_start', 'tiptoe', 'tiptoe_stop', 'skip_start', 'skip', 'skip_stop',
         'shuffle_start', 'shuffle', 'shuffle_stop', 'back_start', 'back_waddle', 'back_stop']
GAITS = {'tiptoe': ('tiptoe_start', 'tiptoe', 'tiptoe_stop'), 'skip': ('skip_start', 'skip', 'skip_stop'),
         'shuffle': ('shuffle_start', 'shuffle', 'shuffle_stop'), 'back_waddle': ('back_start', 'back_waddle', 'back_stop')}
COUNTS = {'tiptoe_start': 4, 'tiptoe': 12, 'tiptoe_stop': 5, 'skip_start': 4, 'skip': 12, 'skip_stop': 5,
          'shuffle_start': 4, 'shuffle': 16, 'shuffle_stop': 6, 'back_start': 4, 'back_waddle': 12, 'back_stop': 5}
SPEED = {'tiptoe': 0.5, 'skip': 1.5, 'shuffle': 0.5, 'back_waddle': -1.0}     # x V per frame (signed: + = forward)
# the stop clip's first frame is the loop's frame 0; the start clip's last frame steps into it
CHAINS = [(GAITS[g][2], 0, GAITS[g][1], 0) for g in GAITS]

BODY = ('bob', 'sz', 'armL', 'armR', 'spL', 'spR', 'earL', 'earR', 'clX', 'clY', 'blink', 'hy', 'hp', 'hr')


def _mix(a, b, w):
    """body params of pose a moved a fraction w towards pose b (feet and root are set by the caller)."""
    q = dict(a)
    for k in BODY: q[k] = a[k] + (b[k] - a[k]) * w
    return q


# ---------------------------------------------------------------------------------------------- loops
def _walk_feet(fr, n, v, H, shape=1.0):
    """Stage A's walk feet: double support on frames 0 and n/2; a foot is planted for n/2 steps and slides back
    by v per frame (s = v * n/2), then swings forward with a lift H * sin(pi t) ** shape."""
    half = n // 2; s = v * half

    def foot(f):
        f %= n
        if f <= half: return s / 2 - v * f, 0.0
        t = (f - half) / half
        return -s / 2 + s * ease(t), H * math.sin(math.pi * t) ** shape
    (fL, hL), (fR, hR) = foot(fr), foot(fr + half)
    return dict(fL=fL, hL=hL, fR=fR, hR=hR), s


def tiptoe_loop(V, PXZ):
    """sneaking on tiptoe: body up on its toes, mittens held up at the chest, slow high careful steps, a peek to
    one side on the first step (only in the first half, so the loop seam stays the calmest step)."""
    n, v, H = 12, SPEED['tiptoe'] * V, 5 * PXZ
    # up on the toes 2-3 px; in the second half the body settles one frame earlier, so the step into the last frame
    # (a twin of step 4 otherwise) only moves the feet and the loop seam stays smooth
    BOB = [2, 2, 3, 3, 3, 2, 2, 2, 3, 3, 2, 2]; SQ = [-1, 0, 1, 1, 1, 0, -1, 0, 1, 1, 0, 0]
    out = []
    for f in range(n):
        p = f / n; feet, s = _walk_feet(f, n, v, H, shape=0.6)
        out.append(KP(bob=BOB[f] * PXZ, sz=1 + SQ[f] * PXZ / HB,
                      armL=62 + 6 * math.sin(2 * math.pi * p), armR=62 - 6 * math.sin(2 * math.pi * p),
                      spL=6, spR=6, earL=-3, earR=-3,
                      clX=-2 + 3 * math.cos(4 * math.pi * p), clY=3 * math.sin(2 * math.pi * p),
                      hy=14 * math.sin(math.pi * f / 6) if f <= 6 else 0.0,
                      hr=-2 * math.sin(math.pi * f / 6) if f <= 6 else 0.0, **feet))
    return out, s


def shuffle_loop(V, PXZ):
    """sleepy shuffle: slumped mochi, head drooping, heavy lids, mittens hanging, feet barely leave the floor."""
    n, v, H = 16, SPEED['shuffle'] * V, 1 * PXZ
    out = []
    for f in range(n):
        p = f / n; feet, s = _walk_feet(f, n, v, H)
        out.append(KP(bob=(1 if f % 8 in (3, 4, 5) else 0) * PXZ, sz=1 - 2 * PXZ / HB,
                      armL=-4 - 6 * math.cos(2 * math.pi * p), armR=-4 + 6 * math.cos(2 * math.pi * p),
                      spL=-3, spR=-3, earL=9, earR=9,
                      clX=6 + 2 * math.cos(4 * math.pi * p), clY=-4 * math.sin(2 * math.pi * p),
                      blink=0.4, hp=12 + 2 * math.cos(4 * math.pi * p), hr=4 * math.sin(2 * math.pi * p), **feet))
    return out, s


def back_loop(V, PXZ):
    """back waddle: the walk run backwards, mittens out in front for balance, a head waddle."""
    n, v, H = 12, SPEED['back_waddle'] * V, 3 * PXZ
    # the landing at frame 0 is a touch softer than at frame 6, and the sways peak mid-step: the seam stays calm
    BOB = [0, 1, 2, 2, 2, 1] * 2; SQ = [-1, -1, 0, 0, 0, 0, -2, -1, 0, 0, 0, 0]
    out = []
    for f in range(n):
        p = f / n; feet, s = _walk_feet(f, n, v, H)
        out.append(KP(bob=BOB[f] * PXZ, sz=1 + SQ[f] * PXZ / HB,
                      armL=28 + 8 * math.cos(2 * math.pi * p), armR=28 - 8 * math.cos(2 * math.pi * p),
                      spL=18, spR=18, earL=-2, earR=-2,
                      clX=-3 * math.cos(4 * math.pi * p), clY=-5 * math.cos(2 * math.pi * p),
                      hp=-4, hr=-5 * math.cos(2 * math.pi * p), **feet))
    return out, s


def skip_loop(V, PXZ):
    """skip: a bouncy hop-step. Each half (6 frames): land on one foot (squash), push off it, float with both feet
    up (the trailing foot rises with the body, the other is raised in front), land on the other foot."""
    n, v = 12, SPEED['skip'] * V
    # a 6 px float with the feet up under the body; the frame-0 landing is a touch softer than frame 6's, so the
    # loop seam is not the biggest step
    BOB = [0, 0, 2, 5, 6, 4, 0, 0, 2, 5, 6, 3]; SQ = [-2, -1, 1, 2, 1, 0, -3, -1, 1, 2, 1, 0]
    # one foot over the 12 frames, phase 0 = it lands: planted 0..2 (slides back v per frame), off the floor 3..11
    LIFT = [0, 0, 0, 4, 5, 4, 6, 7, 6, 6, 6, 3]
    def foot(ph):
        ph %= 12
        if ph <= 2: return v - v * ph, LIFT[ph] * PXZ
        t = min(1.0, (ph - 2) / 5)                      # forward by phase 7, then held out in front to land
        return -v + 2 * v * ease(t), LIFT[ph] * PXZ
    out = []
    for f in range(n):
        p = f / n; hc = f % 6
        (fL, hL), (fR, hR) = foot(f), foot(f + 6)
        sw = math.cos(2 * math.pi * p)
        out.append(KP(bob=BOB[f] * PXZ, sz=1 + SQ[f] * PXZ / HB,
                      armL=25 - 55 * sw, armR=25 + 55 * sw, spL=14, spR=14,
                      earL=[6, 3, 0, -5, -7, -3][hc], earR=[6, 3, 0, -5, -7, -3][hc],
                      clX=[8, 4, -2, -8, -10, -4][hc], clY=5 * math.cos(2 * math.pi * p),
                      hr=3 * math.cos(2 * math.pi * p), fL=fL, hL=hL, fR=fR, hR=hR))
    return out, n * v


# ---------------------------------------------------------------------------------------------- start / stop
def walk_start(loop, s, H, anticip):
    """rest -> the loop's frame 0 (the walk_start method): the right foot stays planted (slides back with the root),
    the left foot swings from rest to the front (world 0 -> s). Root 0, 0, s/6, s/3, then s/2 into the loop."""
    L0 = loop[0]; r = [0.0, 0.0, s / 6, s / 3]
    Lw = [0.0, 0.0, s * ease(1 / 3), s * ease(2 / 3)]
    w = [0.0, 0.35, 0.7, 0.9]
    out = []
    for k in range(4):
        q = _mix(KP(), L0, w[k])
        if k == 1:
            for a, b in anticip.items(): q[a] = q[a] + b
        q.update(fL=Lw[k] - r[k], hL=(0.0 if k < 2 else H * math.sin(math.pi * (k - 1) / 3)), fR=-r[k], hR=0.0,
                 ty_px=r[k])
        out.append(q)
    return out, s / 2


def walk_stop(loop, s, PXZ, H, n):
    """the loop's frame 0 -> rest (the walk_stop method): the left foot stays planted, the right foot catches up
    beside it, a settle squash, rest. Root 0 .. s/2 over n-2 frames, then held."""
    L0 = loop[0]; m = n - 2
    r = [s / 2 * min(k / (m - 1), 1.0) for k in range(n)]
    Rw = [-s / 2 + s * ease(min(k / (m - 1), 1.0)) for k in range(n)]
    out = []
    for k in range(n):
        if k == 0: q = dict(L0); q['ty_px'] = 0.0; out.append(q); continue
        if k == n - 1: out.append(KP(ty_px=r[k])); continue
        if k == n - 2: q = KP(sz=1 - 2 * PXZ / HB, earL=3, earR=3, clX=5, clY=1)       # both feet down: settle
        else: q = _mix(KP(), L0, 1 - k / (m - 1))
        lift = 0.0 if k >= m - 1 else H * max(0.4, math.sin(math.pi * k / (m - 1)))
        q.update(fL=s / 2 - r[k], hL=0.0, fR=Rw[k] - r[k], hR=lift, ty_px=r[k])
        out.append(q)
    return out


def skip_start(loop, v, PXZ):
    """rest -> crouch -> push off the right foot -> airborne -> lands on the left foot (the loop's frame 0)."""
    L3 = loop[3]
    r = [0.0, 0.0, v, 2 * v]
    out = [KP(),
           KP(sz=1 - 3 * PXZ / HB, armL=-20, armR=-20, spL=8, spR=8, earL=4, earR=4, clX=6),
           dict(_mix(KP(), L3, 0.6), fR=-r[2], hR=0.0, fL=0.3 * v, hL=3 * PXZ, bob=2 * PXZ, armL=40, armR=-10),
           dict(_mix(KP(), L3, 1.0), fR=-0.6 * v, hR=4 * PXZ, fL=0.8 * v, hL=5 * PXZ)]
    for k, q in enumerate(out): q['ty_px'] = r[k]
    return out, 3 * v


def skip_stop(loop, v, PXZ):
    """lands on the left foot (the loop's frame 0) -> the right foot comes down beside it -> settle -> rest."""
    L0 = loop[0]
    q0 = dict(L0, ty_px=0.0)
    q1 = dict(_mix(KP(), L0, 0.6), bob=1 * PXZ, fL=v - v / 2, hL=0.0, fR=0.3 * v, hR=2 * PXZ, ty_px=v / 2)
    q2 = KP(sz=1 - 2 * PXZ / HB, earL=4, earR=4, clX=6, clY=2, ty_px=v)
    q3 = KP(sz=1 + 1 * PXZ / HB, earL=-1, earR=-1, clX=-3, ty_px=v)
    q4 = KP(ty_px=v)
    return [q0, q1, q2, q3, q4]


# ---------------------------------------------------------------------------------------------- build
def build(V, PXZ):
    """the rig's entry point: clip name -> {frames, loop, poses, face, handoff, root_after}."""
    C = {}

    def put(name, poses, loop=False, after=None, ho=None):
        assert len(poses) == COUNTS[name], (name, len(poses), COUNTS[name])
        d = dict(frames=len(poses), loop=loop, face='none', poses=[dict(q, tx=0.0) for q in poses])
        if after is not None: d['root_after'] = [0.0, after]
        if ho: d['handoff'] = ho
        C[name] = d

    for g, fn, H, anticip in (('tiptoe', tiptoe_loop, 5 * PXZ, dict(sz=-2 * PXZ / HB, armL=-10, armR=-10)),
                              ('shuffle', shuffle_loop, 1 * PXZ, dict(sz=-1 * PXZ / HB, earL=2, earR=2)),
                              ('back_waddle', back_loop, 3 * PXZ, dict(sz=-2 * PXZ / HB, armL=6, armR=6, earL=2, earR=2))):
        st, lp, sp = GAITS[g]
        loop, s = fn(V, PXZ)
        n = len(loop); v = SPEED[g] * V
        put(lp, [dict(q, ty_px=v * f) for f, q in enumerate(loop)], loop=True, after=v * n)
        start, after = walk_start(loop, s, H, anticip)
        put(st, start, after=after, ho={'start': 'idle0'})
        put(sp, walk_stop(loop, s, PXZ, H, COUNTS[sp]), ho={'end': 'idle0'})
    loop, _ = skip_loop(V, PXZ); v = SPEED['skip'] * V
    put('skip', [dict(q, ty_px=v * f) for f, q in enumerate(loop)], loop=True, after=v * 12)
    start, after = skip_start(loop, v, PXZ)
    put('skip_start', start, after=after, ho={'start': 'idle0'})
    put('skip_stop', skip_stop(loop, v, PXZ), ho={'end': 'idle0'})
    errs = [e for c in ORDER for e in slide_errors(c, C[c], V)]
    assert not errs, 'clips_m: foot slide: ' + '; '.join(errs[:5])
    return {c: C[c] for c in ORDER}


def track(c):
    """the clip's root track (forward, world units): one per frame + one after the last."""
    ps = [dict(REST_K, **q) for q in c['poses']]
    return [q['ty_px'] for q in ps] + [c.get('root_after', [0.0, ps[-1]['ty_px']])[1]]


def slide_steps(c, tr=None):
    """[(i, foot, world slide)] for every step i -> i+1 (and the loop seam) where the foot is planted on both frames:
    world slide = the foot's move inside the sprite + the root step (0 = it stays put on the floor)."""
    ps = [dict(REST_K, **q) for q in c['poses']]; tr = tr or track(c); n = len(ps); out = []
    pairs = [(i, i + 1) for i in range(n - 1)] + ([(n - 1, 0)] if c.get('loop') else [])
    for i, j in pairs:
        dr = tr[i + 1] - tr[i]
        for ft in ('L', 'R'):
            if abs(ps[i]['h' + ft]) < 1e-9 and abs(ps[j]['h' + ft]) < 1e-9:
                out.append((i, ft, ps[j]['f' + ft] - ps[i]['f' + ft] + dr))
    return out


def slide_errors(name, c, V, tol_px=0.5, tr=None):
    """planted feet that move more than tol_px on the 2:1 iso axes (V = 2 px there)."""
    return [f'{name} f{i}->{i + 1} foot {ft} slides {abs(w) / V * 2:.2f} px'
            for i, ft, w in slide_steps(c, tr) if abs(w) / V * 2 > tol_px]
