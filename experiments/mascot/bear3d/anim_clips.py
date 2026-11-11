"""Stage A clips for the cute bear: idle, walk, run and their start/stop transitions (pure Python, no bpy).

Each clip is a list of per-frame pose parameters at 12 fps, plus the root track (forward distance in world
units, cumulative from the clip start). Root motion is NOT baked into the frames: the rig stays in place
and the engine moves the sprite by root[k+1] - root[k] after frame k.

Pose parameters (all relative to the rest pose, 0 / 1 = rest):
  bob        body lift, world units          sz        body vertical scale (xy scale = 1/sqrt(sz), keeps volume)
  roll       waddle, deg about world Y (+ = top towards the bear's left, +X)
  lean       deg about world X at the floor (+ = top forward, -Y)
  armL/armR  deg arm swing at the shoulder (+ = paw forward)
  fL/fR      foot forward offset, world units (+ = forward)      hL/hR  foot lift, world units
  earL/earR  deg ear tilt (+ = outward)     clX/clY  deg clover tilt (+X = forward, +Y = towards +X)
  blink      eye vertical scale (1 = open)

Speeds are chosen so that, on the 2:1 iso axes (SE, NE, NW, SW), the root moves a whole number of pixels per
frame at the ~100 px render size: walk 2 px, run 4 px (V = world units per 2 px along an iso axis). A planted
foot then moves exactly that many pixels backwards inside the sprite, so it stays put on the floor.
"""
import math

FPS = 12
REST = dict(bob=0.0, sz=1.0, roll=0.0, lean=0.0, armL=0.0, armR=0.0, fL=0.0, fR=0.0, hL=0.0, hR=0.0,
            earL=0.0, earR=0.0, clX=0.0, clY=0.0, blink=1.0)


def P(**kw):
    d = dict(REST); d.update(kw); return d


def ease(t):
    return t * t * (3 - 2 * t)


def build(V, PXZ, HB=2.06):
    """V: world units per 2 px along an iso axis (walk speed per frame). PXZ: world z per screen pixel.
    HB: body height, to turn 'n pixels of squash' into a scale factor."""
    def szpx(n): return 1.0 + n * PXZ / HB      # body top moves n px (n < 0 squashes)
    clips = {}

    # ------------------------------------------------------------------ idle (30 frames, 2.5 s, loop)
    R = P()
    seq = []
    seq += [R] * 6
    seq += [P(sz=szpx(1))] * 3                                         # breathe in
    seq += [P(sz=szpx(2), earL=-2, earR=-2, clX=-3)] * 6              # full breath, clover lags back
    seq += [P(sz=szpx(1), earL=3, earR=3, clX=4)] * 3                 # breathe out: ears/clover overshoot
    seq += [P(sz=szpx(-1), earL=1, earR=1, clX=-2)] * 3               # little mochi settle
    seq += [P(blink=0.45), P(blink=0.12), P(blink=0.45)]              # blink
    seq += [R] * 6
    clips['idle'] = dict(loop=True, frames=seq, root=[0.0] * len(seq), next_root=0.0)

    # ------------------------------------------------------------------ walk (12 frames, 1 s, loop): waddle
    v = V; s = 6 * v; h = 3 * PXZ                  # ~3 px foot lift
    BOB = [0, 1, 2, 2, 2, 1]                     # whole pixels, per half cycle
    SQ = [-2, -1, 0, 0, 0, 0]                    # squash on contact (px)

    def walk_pose(f):
        p = (f % 12) / 12
        def foot(fr):
            fr %= 12
            if fr <= 6: return s / 2 - v * fr, 0.0                     # stance: planted, slides back with the root
            t = (fr - 6) / 6
            return -s / 2 + s * ease(t), h * math.sin(math.pi * t)
        fL, hL = foot(f); fR, hR = foot(f + 6)
        hc = f % 6
        roll = 5 * math.sin(2 * math.pi * p)                         # sway over the stance foot
        return P(bob=BOB[hc] * PXZ, sz=szpx(SQ[hc]), roll=roll,
                 armL=-10 * math.cos(2 * math.pi * p), armR=10 * math.cos(2 * math.pi * p),
                 fL=fL, hL=hL, fR=fR, hR=hR,
                 earL=-3 * math.sin(2 * math.pi * (p - 1 / 12)), earR=3 * math.sin(2 * math.pi * (p - 1 / 12)),
                 clX=4 * math.cos(4 * math.pi * (p - 1 / 12)), clY=-5 * math.sin(2 * math.pi * (p - 1 / 12)))
    walk = [walk_pose(f) for f in range(12)]
    clips['walk'] = dict(loop=True, frames=walk, root=[v * f for f in range(12)], next_root=12 * v)

    # walk_start: rest -> (walk frame 0). Right foot stays planted, left foot swings to the front.
    w0 = walk[0]
    root = [0, 0, v, 2 * v]; nxt = 3 * v
    Lw = [0, 0, s * ease(1 / 3), s * ease(2 / 3)]                      # left foot, world
    st = [P(),
          P(sz=szpx(-2), roll=-3, armL=3, armR=-3, earL=2, earR=2),   # anticipation: weight onto the right foot
          P(bob=1 * PXZ, roll=-4, armL=-5, armR=5, fL=Lw[2] - root[2], hL=h * math.sin(math.pi / 3),
            fR=-root[2], earL=-1, earR=1, clX=-3, clY=3),
          P(bob=1 * PXZ, roll=-2, armL=-8, armR=8, fL=Lw[3] - root[3], hL=h * math.sin(2 * math.pi / 3),
            fR=-root[3], earL=-2, earR=2, clX=3, clY=4)]
    clips['walk_start'] = dict(loop=False, frames=st, root=root, next_root=nxt)

    # walk_stop: (after walk frame 11) walk frame 0 pose -> rest. Left foot planted, right foot catches up.
    root = [0, v, 2 * v, 3 * v, 3 * v, 3 * v]
    Rw = [-s / 2, -s / 2 + s * ease(1 / 3), -s / 2 + s * ease(2 / 3), s / 2]   # right foot, world
    sp = [dict(w0),
          P(bob=1 * PXZ, roll=3, armL=-6, armR=6, fL=s / 2 - root[1], fR=Rw[1] - root[1],
            hR=h * math.sin(math.pi / 3), earL=1, earR=-1, clX=2, clY=-4),
          P(bob=1 * PXZ, roll=2, armL=-3, armR=3, fL=s / 2 - root[2], fR=Rw[2] - root[2],
            hR=h * math.sin(2 * math.pi / 3), clX=-2, clY=-3),
          P(sz=szpx(-2), clX=5, clY=1, earL=3, earR=3),                # both feet down: settle squash
          P(sz=szpx(1), clX=-3, earL=-1, earR=-1),                     # tiny rebound
          P()]
    clips['walk_stop'] = dict(loop=False, frames=sp, root=root, next_root=3 * v)

    # ------------------------------------------------------------------ run (8 frames, 0.67 s, loop)
    vr = 2 * V; sr = 3 * vr; hr = 5 * PXZ
    RBOB = [0, 1, 3, 2]; RSQ = [-4, -1, 2, 0]; LEAN = [7, 8, 9, 8]

    def run_pose(f):
        p = (f % 8) / 8
        def foot(fr):
            fr %= 8
            if fr <= 3: return sr / 2 - vr * fr, 0.0
            t = (fr - 3) / 5
            return -sr / 2 + sr * ease(t), hr * math.sin(math.pi * t)
        fL, hL = foot(f); fR, hR = foot(f + 4)
        hc = f % 4
        return P(bob=RBOB[hc] * PXZ, sz=szpx(RSQ[hc]), lean=LEAN[hc], roll=2.5 * math.sin(2 * math.pi * p),
                 armL=-24 * math.cos(2 * math.pi * p), armR=24 * math.cos(2 * math.pi * p),
                 fL=fL, hL=hL, fR=fR, hR=hR,
                 earL=-4 + 3 * math.cos(4 * math.pi * p), earR=-4 + 3 * math.cos(4 * math.pi * p),
                 clX=-9 + 5 * math.cos(4 * math.pi * (p - 1 / 8)), clY=-4 * math.sin(2 * math.pi * (p - 1 / 8)))
    run = [run_pose(f) for f in range(8)]
    clips['run'] = dict(loop=True, frames=run, root=[vr * f for f in range(8)], next_root=8 * vr)

    # run_start: rest -> crouch -> push off (right foot planted) -> run frame 0
    root = [0, 0, 0.5 * vr]; nxt = 1.5 * vr
    clips['run_start'] = dict(loop=False, root=root, next_root=nxt, frames=[
        P(),
        P(sz=szpx(-4), lean=4, armL=12, armR=12, earL=3, earR=3, clX=4),               # crouch, arms ready
        P(bob=2 * PXZ, sz=szpx(2), lean=9, armL=-16, armR=16, fL=3 * vr * ease(0.5) - root[2],
          hL=hr, fR=-root[2], earL=-4, earR=-4, clX=-8)])

    # run_stop: run frame 0 -> brake (lean back) -> settle -> rest. Left foot planted, right catches up.
    root = [0, vr, 1.5 * vr, 1.5 * vr, 1.5 * vr]
    clips['run_stop'] = dict(loop=False, root=root, next_root=1.5 * vr, frames=[
        dict(run[0]),
        P(bob=2 * PXZ, lean=3, armL=-10, armR=10, fL=sr / 2 - root[1], fR=(-sr / 2 + sr * 0.5) - root[1],
          hR=hr * 0.8, earL=-3, earR=-3, clX=-6),
        P(sz=szpx(-4), lean=-4, armL=8, armR=8, clX=9, clY=2, earL=4, earR=4),     # brake: both feet down
        P(sz=szpx(1), lean=2, clX=-4, earL=-1, earR=-1),                           # wobble back
        P()])
    return clips


ORDER = ['idle', 'walk_start', 'walk', 'walk_stop', 'run_start', 'run', 'run_stop']


# ====================================================================== Stage C (rig_bear.py --stage c)
# Expressive clips for the brain's intents: think, talk, ask (loop) and oops, happy, celebrate (one-shot).
# Extra pose parameters (Stage C only, rest 0): spL/spR = deg sideways arm spread at the shoulder (+ = outward),
# applied after the forward raise armL/armR (150-165 = paw up above the shoulder). Arms go UP by the forward raise
# with a small spread: a wide sideways raise leaves Stage A's 84 px frame (render_anim.py --canvas checks it).
# Stage C rules, so the D16 face patches stay pixel-exact (verify_face.py):
#   - no roll / lean (the eyes and mouth ride the body by a whole-pixel vertical shift only, see rig_bear.py);
#   - bob in whole pixels; a hop lifts the feet with the body (hL = hR = bob);
#   - frame 0, and the last frame of a one-shot, is the rest pose (hand-off to idle);
#   - the eyes stay open (blinks are face layers, picked by the brain); no paw over the eyes or mouth.
REST_C = dict(REST, spL=0.0, spR=0.0)


def PC(**kw):
    d = dict(REST_C); d.update(kw); return d


# Stage C has no squash: D16's face patches must match every Stage C frame pixel for pixel, and squashing the body
# (xy 1/sqrt(sz)) re-rasterises the fur around the eyes (2-22 px per frame, measured). Hops, ears, clover and arms
# carry the motion instead. szpx() keeps the squash beats in the poses, at strength SQUASH_C.
SQUASH_C = 0.0


def build_c(V, PXZ, HB=2.06):
    def szpx(n): return 1.0 + SQUASH_C * n * PXZ / HB
    def hop(n, **kw): return PC(bob=n * PXZ, hL=n * PXZ, hR=n * PXZ, **kw)
    R = PC()
    clips = {}

    # think (loop, 28 frames, 2.33 s): paw up to the cheek, tap-tap, ears and clover ponder side to side
    up1 = PC(armR=45, spR=8, earR=2)
    up2 = PC(armR=90, spR=22, earR=3, clY=3)
    a = PC(armR=110, spR=15, earL=-2, earR=4, clY=6, clX=-2)
    a2 = PC(armR=120, spR=15, earL=-2, earR=4, clY=6, clX=-2)                  # tap
    b = PC(armR=110, spR=15, sz=szpx(1), earL=3, earR=-1, clY=-5, clX=2)
    b2 = PC(armR=120, spR=15, sz=szpx(1), earL=3, earR=-1, clY=-5, clX=2)
    seq = [R, up1, up2] + [a] * 3 + [a2, a, a2] + [a] * 2 + [b] * 3 + [b2, b, b2] + [b] * 2 + [a] * 4 + [up2, up1, R, R, R]
    clips['think'] = dict(loop=True, frames=seq)

    # talk (loop, 16 frames, 1.33 s): little alternating paw gestures with a breath and ear flicks
    gl = PC(armL=55, spL=20, sz=szpx(1), earL=-2, earR=2, clX=-3)
    gl2 = PC(armL=30, spL=10, earL=1, earR=1, clX=2)
    gr = PC(armR=55, spR=20, sz=szpx(1), earL=2, earR=-2, clX=-3)
    gr2 = PC(armR=30, spR=10, earL=1, earR=1, clX=2)
    seq = [R, gl2, gl, gl, gl, gl2, R, PC(sz=szpx(-1)), gr2, gr, gr, gr, gr2, R, PC(sz=szpx(1)), R]
    clips['talk'] = dict(loop=True, frames=seq)

    # ask (loop, 24 frames, 2 s): one paw raised high (hand up), perked ears, a toe bounce, a paw wiggle
    hi = dict(armL=120, spL=25, earL=-4, earR=-4)
    seq = [R, PC(armL=60, spL=25, earL=-1, earR=-1), PC(armL=100, spL=24, sz=szpx(1), earL=-3, earR=-3, clX=-3)]
    for _ in range(2):
        seq += [hop(1, sz=szpx(2), clX=-4, **hi)] * 3 + [PC(sz=szpx(1), clX=3, **hi)] * 3
    w = dict(hi); w['armL'] = 132
    seq += [PC(clX=2, **w), PC(clX=-2, **hi), PC(clX=2, **w), PC(clX=-2, **hi)]
    seq += [PC(armL=100, spL=24, sz=szpx(1), earL=-3, earR=-3, clX=3), PC(armL=60, spL=25, earL=-1, earR=-1, clX=-2),
            PC(sz=szpx(-1), clX=2), R, R]
    clips['ask'] = dict(loop=True, frames=seq)

    # oops (one-shot, 18 frames, 1.5 s): flinch, startle up with paws out, sheepish droop, recover
    seq = [R,
           PC(sz=szpx(-3), armL=-15, armR=-15, earL=6, earR=6, clX=8),
           hop(2, sz=szpx(2), armL=-20, armR=-20, spL=20, spR=20, earL=-5, earR=-5, clX=-10),
           PC(sz=szpx(-2), armL=10, armR=10, spL=10, spR=10, earL=6, earR=6, clX=9, clY=3)]
    droop = PC(sz=szpx(-2), armL=25, armR=25, spL=4, spR=4, earL=9, earR=9, clX=11, clY=5)
    droop2 = PC(sz=szpx(-2), armL=25, armR=25, spL=4, spR=4, earL=8, earR=8, clX=10, clY=-3)
    seq += [droop] * 4 + [droop2] * 3
    seq += [PC(sz=szpx(-1), armL=15, armR=15, earL=6, earR=6, clX=7),
            PC(sz=szpx(1), armL=6, armR=6, earL=2, earR=2, clX=-3),
            PC(earL=-1, earR=-1, clX=2), PC(sz=szpx(-1), clX=-1), R, R, R]
    clips['oops'] = dict(loop=False, frames=seq)

    # happy (one-shot, 18 frames, 1.5 s): two little hops with the paws out, ears flapping
    out = dict(armL=45, armR=45, spL=25, spR=25)
    out2 = dict(armL=38, armR=38, spL=20, spR=20)
    seq = [R, PC(sz=szpx(-3), armL=20, armR=20, earL=2, earR=2, clX=3),
           hop(4, sz=szpx(2), earL=-4, earR=-4, clX=-8, **out),
           hop(5, sz=szpx(1), earL=-5, earR=-5, clX=-6, **out),
           hop(3, earL=-2, earR=-2, clX=2, **out2),
           PC(sz=szpx(-3), earL=5, earR=5, clX=8, **out2),
           PC(sz=szpx(-1), armL=40, armR=40, spL=12, spR=12, earL=3, earR=3, clX=4),
           hop(3, sz=szpx(2), earL=-4, earR=-4, clX=-7, **out),
           hop(4, sz=szpx(1), earL=-4, earR=-4, clX=-4, **out),
           hop(2, earL=-1, earR=-1, clX=3, **out2),
           PC(sz=szpx(-3), armL=40, armR=40, spL=12, spR=12, earL=5, earR=5, clX=8),
           PC(sz=szpx(-1), armL=25, armR=25, spL=6, spR=6, earL=3, earR=3, clX=3),
           PC(sz=szpx(1), armL=10, armR=10, earL=-1, earR=-1, clX=-3),
           PC(earL=1, earR=1, clX=2), PC(sz=szpx(-1)), R, R, R]
    clips['happy'] = dict(loop=False, frames=seq)

    # celebrate (one-shot, 28 frames, 2.33 s): crouch, big jump with both paws up, land, a hop, paw waves, settle
    up = dict(armL=160, armR=160, spL=12, spR=12)
    seq = [R, PC(sz=szpx(-2), armL=-8, armR=-8, earL=2, earR=2),
           PC(sz=szpx(-4), armL=-15, armR=-15, earL=4, earR=4, clX=5),
           hop(6, sz=szpx(3), armL=110, armR=110, spL=15, spR=15, earL=-5, earR=-5, clX=-12),
           hop(7, sz=szpx(2), earL=-6, earR=-6, clX=-9, **up),
           hop(8, sz=szpx(1), earL=-5, earR=-5, clX=-4, **up),
           hop(7, earL=-3, earR=-3, clX=2, **up),
           hop(4, earL=0, earR=0, clX=6, **up),
           PC(sz=szpx(-4), armL=145, armR=145, spL=12, spR=12, earL=6, earR=6, clX=11),
           PC(sz=szpx(-2), armL=150, armR=150, spL=12, spR=12, earL=4, earR=4, clX=6),
           hop(3, sz=szpx(2), earL=-4, earR=-4, clX=-6, **up),
           hop(4, sz=szpx(1), earL=-4, earR=-4, clX=-3, **up),
           hop(2, earL=-1, earR=-1, clX=3, **up),
           PC(sz=szpx(-3), armL=150, armR=150, spL=12, spR=12, earL=5, earR=5, clX=8)]
    wa = PC(armL=165, armR=140, spL=22, spR=8, earL=-2, earR=2, clY=5)
    wb = PC(armL=140, armR=165, spL=8, spR=22, earL=2, earR=-2, clY=-5)
    seq += [wa, wa, wb, wb, wa, wa, wb, wb]
    seq += [PC(armL=100, armR=100, spL=10, spR=10, earL=1, earR=1, clX=-3),
            PC(armL=40, armR=40, spL=4, spR=4, sz=szpx(-2), earL=3, earR=3, clX=4),
            PC(sz=szpx(1), earL=-1, earR=-1, clX=-2), PC(sz=szpx(-1), clX=1), R, R]
    clips['celebrate'] = dict(loop=False, frames=seq)

    for c in clips.values():
        c['root'] = [0.0] * len(c['frames']); c['next_root'] = 0.0   # all in place
    return clips


ORDER_C = ['think', 'talk', 'ask', 'oops', 'happy', 'celebrate']


# ---------------------------------------------------------------------- Stage L (listen + turns, rig_bear.py --stage l)
# Stage C rules plus two parameters, both 0 at rest:
#   hy  head yaw, deg about the body's vertical axis (the axis the render yaw uses): the whole round head mass turns
#       (rigid above the mouth, a smooth twist down to the waist band, rig_bear.py --stage l); + = counter-clockwise
#       from above, the same sense as the direction yaw. Hold frames use exactly +-45 so the head renders exactly as
#       the neighbouring direction's head (face patch dir: SE->S, E->SE with -45; SW->S, W->SW with +45).
#   hb  head bob, whole pixels up (the head mass and its face; the twist band takes up the 1 px), a breath.
#   ty  turn yaw, deg of the whole bear about the floor origin (turn clips only; their last frame is the rest pose
#       of the next direction).
REST_L = dict(REST_C, hy=0.0, hb=0, ty=0.0)


def PL(**kw):
    d = dict(REST_L); d.update(kw); return d


# head yaw of each listen variant: _cw turns the head clockwise (-45, body SE/E), _ccw counter-clockwise (+45, SW/W),
# _0 none (S already faces the camera; its listen is the ear perk and the breath)
LISTEN_VARIANTS = {'cw': -45.0, 'ccw': 45.0, '0': 0.0}
LISTEN_DIR_VARIANT = {'S': '0', 'SE': 'cw', 'E': 'cw', 'SW': 'ccw', 'W': 'ccw'}
LISTEN_PATCH_DIR = {'S': 'S', 'SE': 'S', 'SW': 'S', 'E': 'SE', 'W': 'SW'}


def build_l(V, PXZ):
    clips = {}
    for v, Y in LISTEN_VARIANTS.items():
        perk = dict(earL=-4, earR=-4, clX=-3)
        # listen_in (4, one-shot): the head mass yaws toward the camera, a tiny ear perk on the last frame
        clips['listen_in_' + v] = dict(loop=False, frames=[
            PL(), PL(hy=Y * (1 / 3), earL=1, earR=1, clX=2), PL(hy=Y * (2 / 3), earL=-1, earR=-1, clX=-1),
            PL(hy=Y, **perk)])
        # listen (16, loop): held yaw; a slow 1 px breath (f4-f7), soft ear wiggle (f8-f11); rest frames exact
        H = PL(hy=Y)
        seq = [H] * 4 + [PL(hy=Y, hb=1, earL=-1, earR=-1, clX=-2)] * 4
        seq += [PL(hy=Y, earL=3, earR=-2, clY=3), PL(hy=Y, earL=-2, earR=3, clY=-3)] * 2
        seq += [H] * 4
        clips['listen_' + v] = dict(loop=True, frames=seq)
        # listen_out (4, one-shot): the head returns to the body's facing; last frame = rest
        clips['listen_out_' + v] = dict(loop=False, frames=[
            PL(hy=Y * (2 / 3), earL=1, earR=1, clX=2), PL(hy=Y * (1 / 3), earL=2, earR=2, clX=1),
            PL(earL=1, earR=1, clX=-1), PL()])
    # turns (4, one-shot): feet shuffle, the body rotates 45 deg, a 1 px hop at f2; f0 / f3 are rest poses
    for name, s in (('turn_ccw', 1), ('turn_cw', -1)):
        lead, trail = ('hL', 'hR') if s > 0 else ('hR', 'hL')
        clips[name] = dict(loop=False, frames=[
            PL(), PL(ty=15 * s, **{lead: 2 * PXZ}, earL=1, earR=1, clY=-3 * s),
            PL(ty=30 * s, bob=PXZ, **{lead: PXZ, trail: 3 * PXZ}, earL=-2, earR=-2, clY=3 * s),   # 1 px hop
            PL(ty=45 * s)])
    for c in clips.values():
        c['root'] = [0.0] * len(c['frames']); c['next_root'] = 0.0
    return clips


ORDER_L = [f'{c}_{v}' for v in LISTEN_VARIANTS for c in ('listen_in', 'listen', 'listen_out')] + ['turn_ccw', 'turn_cw']
