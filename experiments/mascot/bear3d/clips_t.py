"""Stage t (tumbles, stages/t.json): six one-shots in all 8 directions, face "none" (no face layers).
Pure Python: rig_bear.py --stage k keys build(V, PXZ); make_stage.py / verify_t.py read CLIPS, ORDER and the json.

    hop         10  84x123   a two-footed bunny hop forward (root_px): crouch -> air -> squash -> rest
    spin        16  84x123   one full ccw turn on the spot (root yaw ty 0 -> 360), arms out, a little hop at the half turn
    roll        20  132x135  a tucked forward somersault (whole-body tumble 0 -> 360, lift keeps the round body on the
                             floor), rolls forward (root_px), lands with a squash and a wobble
    trip        28  132x135  a toe catches (lean), flop onto the belly (tumble 90), a dazed look up and a foot kick, push
                             up, sit, stand, shake it off; on the spot (no root motion)
    pounce      18  132x123  from the run (frame 0 = Stage A run_stop#0): crouch, leap with both mittens reaching, land
                             low with the mittens on the floor, a happy wiggle, stand up
    slide_stop  14  132x123  from the run (frame 0 = run_stop#0): hop into a leaned-back skid with the arms out, the
                             slide slows, momentum tips it forward, settle

chase = run_start -> run -> pounce, slide = run_start -> run -> slide_stop. Every clip ends on Stage A's idle frame 0
(hand-off idle0); hop, roll, trip, pounce and slide_stop end displaced by their root track.
Cute rules: the body stays one round mochi lump (tilts and tumbles turn it rigidly: no bending, no neck), squash and
stretch within 6 %, stub feet leave the floor with the body (hL = hR = bob), mitten arms swing at the shoulder only.
"""
import math
import anim_clips
from anim_clips import REST_K, PK  # noqa: F401  (PK: the Stage K parameter list, the kit contract)

FPS = 12
SMALL = dict(frame=[84, 123], anchor=[42, 107])
WIDE = dict(frame=[132, 123], anchor=[66, 107])
TALL = dict(frame=[132, 135], anchor=[66, 107])     # roll, trip (lead, q-103334): 12 px more below the feet row
HB = 2.06                           # body height (anim_clips.build): n px of squash -> a scale factor
ORDER = ['hop', 'spin', 'roll', 'trip', 'pounce', 'slide_stop']
IDLE = {'start': 'idle0', 'end': 'idle0'}
RUN = {'start': 'run_stop#0', 'end': 'idle0'}

# static facts (stagekit.check_clips reads frames / loop / dirs / frame / anchor); the poses come from build()
CLIPS = {
    'hop': dict(frames=10, loop=False, face='none', dirs=None, handoff=IDLE, **SMALL),
    'spin': dict(frames=16, loop=False, face='none', dirs=None, handoff=IDLE, **SMALL),
    'roll': dict(frames=20, loop=False, face='none', dirs=None, handoff=IDLE, **TALL),
    'trip': dict(frames=28, loop=False, face='none', dirs=None, handoff=IDLE, **TALL),
    'pounce': dict(frames=18, loop=False, face='none', dirs=None, handoff=RUN, **WIDE),
    'slide_stop': dict(frames=14, loop=False, face='none', dirs=None, handoff=RUN, **WIDE),
}

# how far the whole bear must rise (world units) so its lowest point stays on the floor at a tumble angle (deg),
# measured on bear.blend's rest mesh about the body centre (every 15 deg, 0..345)
_LIFT = [0.017, 0.033, 0.015, -0.055, -0.182, -0.311, -0.351, -0.295, -0.046, 0.225, 0.416, 0.555, 0.612, 0.562,
         0.405, 0.155, -0.15, -0.361, -0.351, -0.295, -0.24, -0.194, -0.081, -0.008]


# measured on the posed rig (scratch margins probe): extra lift (world units) so a tumbled frame's lowest point sits
# where the rest pose's does (z -0.017); keyed (clip, frame)
LIFT_FIX = {('roll', 3): -0.037, ('roll', 4): -0.037, ('roll', 5): 0.035, ('roll', 6): 0.118, ('roll', 7): -0.034, ('roll', 8): -0.19, ('roll', 9): -0.328, ('roll', 10): -0.203, ('roll', 11): -0.017, ('roll', 12): -0.009, ('roll', 13): -0.033, ('trip', 4): -0.017, ('trip', 5): -0.017, ('trip', 6): -0.052, ('trip', 7): -0.055, ('trip', 8): -0.049, ('trip', 9): -0.049, ('trip', 10): -0.049, ('trip', 11): -0.049, ('trip', 12): -0.049, ('trip', 13): -0.049, ('trip', 14): -0.049, ('trip', 15): -0.049, ('trip', 16): -0.049, ('trip', 17): -0.042, ('trip', 18): -0.027, ('trip', 19): -0.011, ('trip', 20): -0.013}


def floor_lift(deg):
    t = (deg % 360) / 15.0; i = int(t); f = t - i
    return _LIFT[i % 24] * (1 - f) + _LIFT[(i + 1) % 24] * f


def build(V, PXZ):
    """V: world units per 2 px along an iso axis; PXZ: world z per screen pixel -> {clip: dict(CLIPS[c], poses=...)}."""
    def sz(n): return 1.0 + n * PXZ / HB
    def K(**kw): return dict(REST_K, **kw)
    def air(px, **kw):                       # both stub feet leave the floor with the body
        return K(bob=px * PXZ, hL=px * PXZ, hR=px * PXZ, **kw)
    def tum(deg, extra=0.0, **kw):           # whole-bear tumble, kept on the floor (+ extra world units up)
        return K(tumble=float(deg % 360), lift=floor_lift(deg) + extra if deg % 360 else extra, **kw)
    A = anim_clips.build(V, PXZ)
    run0 = dict(REST_K, **A['run_stop']['frames'][0])           # = Stage A run_stop#0 = run#0 (lean 7, mid-stride)
    vr = 2 * V                                                    # the run's root step per frame
    sr = 3 * vr                                                   # the run's stride
    out = {}

    # ---------------------------------------------------------------- hop: 3 V forward (6 px on the iso axes)
    D = 3 * V
    out['hop'] = [
        K(),
        K(sz=sz(-3), armL=-14, armR=-14, earL=3, earR=3, clX=4),                     # crouch, arms back
        air(3, sz=sz(2), armL=20, armR=20, earL=-4, earR=-4, clX=-6, ty_px=D * 0.25),  # push off, stretch
        air(6, armL=28, armR=28, earL=-3, earR=-3, clX=-4, ty_px=D * 0.55),           # apex
        air(5, armL=22, armR=22, earL=1, earR=1, clX=2, ty_px=D * 0.8),
        air(2, armL=10, armR=10, earL=4, earR=4, clX=5, ty_px=D * 0.95),              # coming down
        K(sz=sz(-3), armL=-6, armR=-6, earL=6, earR=6, clX=7, ty_px=D),               # land: squash, ears flop
        K(sz=sz(1), armL=4, armR=4, earL=-2, earR=-2, clX=-3, ty_px=D),               # rebound
        K(sz=sz(-1), earL=1, earR=1, clX=1, ty_px=D),
        K(ty_px=D)]

    # ---------------------------------------------------------------- spin: one full ccw turn on the spot
    turn = [0, 0, -12, 15, 45, 80, 115, 150, 185, 220, 255, 290, 325, 350, 360, 360]
    hop1 = {7: 1, 8: 2, 9: 1}               # a little 1-2 px hop at the half turn
    poses = []
    for f, t in enumerate(turn):
        if f in (0, 15): poses.append(K()); continue
        if f == 1: poses.append(K(sz=sz(-2), armL=-6, armR=-6, earL=2, earR=2)); continue      # wind-up squash
        if f == 14: poses.append(K(sz=sz(-2), earL=5, earR=5, clY=-4)); continue              # stop: settle
        h = hop1.get(f, 0)
        q = air(h) if h else K()
        q.update(ty=float(t), spL=28.0, spR=28.0, armL=8.0, armR=8.0, earL=9.0, earR=9.0, clY=7.0)
        if f == 2: q.update(spL=10.0, spR=10.0, earL=3.0, earR=3.0, clY=-3.0)
        if f == 13: q.update(spL=14.0, spR=14.0, earL=6.0, earR=6.0, clY=3.0)
        poses.append(q)
    out['spin'] = poses

    # ---------------------------------------------------------------- roll: a tucked forward somersault, 10 V forward
    D = 10 * V
    ball = dict(sz=sz(-4), armL=30.0, armR=30.0, spL=4.0, spR=4.0, hp=20.0, earL=-6.0, earR=-6.0, clX=-8.0)   # tucked: paws on the tummy
    roll = [K(),
            K(sz=sz(-3), armL=30, armR=30, hp=12, earL=3, earR=3, clX=4),                  # crouch, paws forward
            K(sz=sz(-4), lean=12, armL=60, armR=60, hp=20, earL=-3, earR=-3, ty_px=0.3 * V)]  # tuck and tip in
    angles = [30, 60, 90, 120, 150, 180, 210, 240, 270, 300, 330]                         # frames 3..13
    for i, a in enumerate(angles):
        roll.append(tum(a, 0.02, **ball, ty_px=0.3 * V + (D - 0.6 * V) * (i + 1) / len(angles)))
    roll += [K(sz=sz(-4), armL=24, armR=24, hp=8, earL=10, earR=10, clX=8, ty_px=D - 0.3 * V),   # land: squash
             K(sz=sz(2), armL=-6, armR=-6, earL=-4, earR=-4, clX=-5, ty_px=D),                 # rebound
             K(sz=sz(-1), hr=7, earL=3, earR=-2, clY=5, ty_px=D),                               # dizzy wobble
             K(hr=-5, earL=-2, earR=3, clY=-4, ty_px=D),
             K(sz=sz(-1), hr=2, clY=2, ty_px=D),
             K(ty_px=D)]
    out['roll'] = roll

    # ---------------------------------------------------------------- trip: stumble, flop on the belly, get up
    F = 0.0                                              # on the spot: the tumble pivots about the body centre
    flop = dict(armL=98.0, armR=98.0, spL=12.0, spR=12.0, earL=12.0, earR=12.0, clX=10.0)
    trip = [K(),
            K(lean=5, hL=3 * PXZ, fL=0.06, armL=-8, armR=8, ty_px=0.1 * F),                   # a step...
            K(lean=14, fL=0.08, hR=2 * PXZ, armL=-30, armR=-30, earL=-4, earR=-4, clX=-6, ty_px=0.2 * F),  # toe catches
            K(lean=16, armL=50, armR=32, spL=30, spR=30, earL=-8, earR=-8, clX=-10, hp=-10, ty_px=0.35 * F),  # flail
            tum(45, armL=110, armR=90, spL=25, spR=25, earL=-6, earR=-6, clX=-6, hp=-12, ty_px=0.55 * F),
            tum(75, armL=100, armR=90, spL=15, spR=15, earL=4, earR=4, hp=-12, ty_px=0.8 * F),
            tum(92, sz=sz(-4), **flop, hp=-20, ty_px=F),                                              # flop! squash
            tum(89, 0.01, **dict(flop, earL=-4, earR=-4, clX=-6), hp=-20, ty_px=F),         # little bounce
            tum(90, **flop, hp=-20, ty_px=F),
            tum(90, **flop, hp=-20, ty_px=F),
            tum(90, **dict(flop, earL=8, earR=8), fL=-0.10, hp=-20, ty_px=F),                         # foot kick
            tum(90, **flop, fR=-0.10, hp=-20, ty_px=F),
            tum(90, **dict(flop, earL=8, earR=8), fL=-0.10, hp=-20, ty_px=F),
            tum(90, **flop, hp=-20, ty_px=F),
            tum(90, **dict(flop, armL=95, armR=95), hp=-20, ty_px=F),                       # dazed: looks up
            tum(90, **dict(flop, armL=95, armR=95), hp=-20, hr=8, ty_px=F),
            tum(90, **dict(flop, armL=95, armR=95), hp=-20, hr=-8, ty_px=F),
            tum(88, armL=95, armR=95, earL=6, earR=6, hp=-20, ty_px=F),                       # paws under: push
            tum(65, armL=70, armR=70, earL=4, earR=4, hp=-6, ty_px=F),
            tum(40, armL=45, armR=45, earL=2, earR=2, ty_px=F),
            tum(18, armL=20, armR=20, ty_px=F),
            K(sz=sz(-4), lean=-6, fL=0.10, fR=0.10, armL=6, armR=6, earL=6, earR=6, clX=6, ty_px=F),  # plop: sat up
            K(sz=sz(1), hy=18, earL=-6, earR=6, clY=-8, ty_px=F),                             # shake it off
            K(hy=-18, earL=6, earR=-6, clY=8, ty_px=F),
            K(hy=12, earL=-4, earR=4, clY=-5, ty_px=F),
            K(hy=-6, earL=2, earR=-2, clY=3, ty_px=F),
            K(sz=sz(-1), ty_px=F),
            K(ty_px=F)]
    out['trip'] = trip

    # ---------------------------------------------------------------- pounce: from the run, leap and land on the mittens
    r1 = dict(fL=sr / 2 - vr, fR=0.0, hR=4 * PXZ)
    p = vr * 1.0
    pounce = [dict(run0),
              K(lean=10, sz=sz(-2), armL=-14, armR=-14, earL=-3, earR=-3, clX=-6, **r1, ty_px=p),
              K(lean=12, sz=sz(-4), armL=-26, armR=-26, earL=4, earR=4, clX=6, ty_px=1.4 * p),       # crouch
              K(lean=6, sz=sz(2), bob=3 * PXZ, hL=3 * PXZ, hR=3 * PXZ, armL=58, armR=58, spL=8, spR=8,
                earL=-8, earR=-8, clX=-10, ty_px=2.4 * p),                                          # leap
              K(lean=7, bob=4 * PXZ, hL=4 * PXZ, hR=4 * PXZ, armL=62, armR=62, spL=10, spR=10,
                earL=-10, earR=-10, clX=-12, ty_px=3.4 * p),
              K(lean=10, bob=3 * PXZ, hL=3 * PXZ, hR=3 * PXZ, armL=62, armR=62, spL=10, spR=10,
                earL=-6, earR=-6, clX=-8, ty_px=4.3 * p),
              K(lean=14, bob=1 * PXZ, hL=1 * PXZ, hR=1 * PXZ, armL=60, armR=60, spL=8, spR=8, earL=2, earR=2,
                ty_px=5.0 * p),
              K(lean=26, sz=sz(-4), armL=62, armR=62, spL=8, spR=8, hp=10, earL=12, earR=12, clX=10, ty_px=5.3 * p),  # land
              K(lean=25, sz=sz(-3), armL=60, armR=60, spL=8, spR=8, hp=16, earL=6, earR=6, clX=4, ty_px=5.3 * p),     # got it
              K(lean=25, roll=6, sz=sz(-3), armL=60, armR=60, spL=8, spR=8, hp=16, earL=3, earR=8, clY=6, ty_px=5.3 * p),  # wiggle
              K(lean=25, roll=-6, sz=sz(-3), armL=60, armR=60, spL=8, spR=8, hp=16, earL=8, earR=3, clY=-6, ty_px=5.3 * p),
              K(lean=25, roll=5, sz=sz(-3), armL=60, armR=60, spL=8, spR=8, hp=16, earL=3, earR=7, clY=5, ty_px=5.3 * p),
              K(lean=24, roll=-3, sz=sz(-3), armL=60, armR=60, spL=8, spR=8, hp=12, clY=-3, ty_px=5.3 * p),
              K(lean=16, armL=34, armR=34, hp=4, earL=-3, earR=-3, ty_px=5.3 * p),                     # up
              K(lean=8, sz=sz(1), armL=12, armR=12, earL=-4, earR=-4, clX=-5, ty_px=5.3 * p),
              K(lean=-3, sz=sz(1), armL=-4, armR=-4, earL=3, earR=3, clX=5, ty_px=5.3 * p),            # sway back
              K(sz=sz(-1), earL=1, earR=1, clX=-2, ty_px=5.3 * p),
              K(ty_px=5.3 * p)]
    out['pounce'] = pounce

    # ---------------------------------------------------------------- slide_stop: from the run, skid to a stop
    steps = [vr, 0.9 * vr, 0.8 * vr, 0.7 * vr, 0.55 * vr, 0.4 * vr, 0.25 * vr, 0.1 * vr]   # after frames 0..7
    at = [0.0]
    for s_ in steps: at.append(at[-1] + s_)
    skid = dict(fL=0.12, fR=0.12, spL=38.0, spR=38.0, armL=-8.0, armR=-8.0, earL=-6.0, earR=-6.0, clX=7.0)
    slide = [dict(run0),
             K(lean=-4, bob=2 * PXZ, hL=2 * PXZ, hR=2 * PXZ, fL=0.06, fR=0.06, spL=25, spR=25, earL=-4, earR=-4,
               clX=-4, ty_px=at[1]),                                                              # hop into the skid
             K(lean=-12, sz=sz(-2), **skid, ty_px=at[2]),
             K(lean=-14, sz=sz(-1), hr=5, **skid, ty_px=at[3]),
             K(lean=-14, hr=-4, **skid, ty_px=at[4]),
             K(lean=-12, hr=3, **skid, ty_px=at[5]),
             K(lean=-10, **dict(skid, spL=32, spR=32), ty_px=at[6]),
             K(lean=-7, **dict(skid, spL=26, spR=26), ty_px=at[7]),
             K(lean=9, sz=sz(1), fL=0.12, fR=0.12, spL=18, spR=18, armL=24, armR=24, earL=-8, earR=-8, clX=-9,
               ty_px=at[8]),                                                                      # stop: tips forward
             K(lean=-4, sz=sz(-3), fL=0.06, fR=0.06, spL=8, spR=8, armL=-6, armR=-6, earL=6, earR=6, clX=6, ty_px=at[8]),
             K(lean=2, sz=sz(1), fL=0.03, fR=0.03, earL=-3, earR=-3, clX=-3, ty_px=at[8]),
             K(sz=sz(-1), earL=2, earR=2, clX=2, ty_px=at[8]),
             K(earL=-1, earR=-1, ty_px=at[8]),
             K(ty_px=at[8])]
    out['slide_stop'] = slide

    for (c, f), dz in LIFT_FIX.items(): out[c][f]['lift'] = out[c][f]['lift'] + dz
    res = {}
    for c, ps in out.items():
        assert len(ps) == CLIPS[c]['frames'], (c, len(ps), CLIPS[c]['frames'])
        res[c] = dict(CLIPS[c], poses=ps)
    return res
