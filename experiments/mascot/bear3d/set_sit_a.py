"""Stage sit_a (set module of the stage kit, stages/sit_a.json): three floor-cushion pastimes, tea, snack and knit.
Pure Python at import (rig_bear.py --stage k keys the poses, render_set.py builds the props, make_set.py /
verify_sit_a.py read the same tables); build_props and prop_state run inside Blender (render_set.py) and import bpy
there. Set contract (CLAUDE.md "Stage kit"): SETS, PARTS, PROPS, geometry, build, handoff, build_props, prop_state,
mirror_state. Storyboard rows 71-73 (#sb-props-sit-a) are the contract: names, frame counts, loops, face handling.

Every frame is (q, st): q = the Stage K pose (anim_clips.REST_K) plus the set placement X, Y, Z (set frame, world
units) and yaw (0 = SW, 90 = SE); st = the prop state: {layer: [w, at]} for the held props, w = 0 at the prop's home
spot on the floor, 1 in the paws, at = 'paw_L' | 'paw_R' | 'both' (mirror_state swaps paw_L / paw_R for ori B), plus
'tilt' (deg, the held prop tipped toward the mouth).

Ori A (written here): the cushion centre is the set origin; the bear's hand-off spot H is in front of it (SE on
screen, so the cushion never covers the bear there); the bear faces SE; the set's small prop stands at the cushion's
right edge (the bear's right, SW on screen, in front of the seated bear so it stays visible), and the right mitten
reaches it. Ori B = the 3D mirror (anim_clips.mirror_k): facing SW.

Held props without kit changes: the kit keeps add_objects (paw-socket objects) for clip stages, so here a held prop is
a normal prop layer whose transform prop_state sets for the frame (render_set.py calls it right after posing the rig):
home transform (set frame) blended by w into the paw transform (the posed paw bone's delta applied to a grip point in
the bear's rest frame). The render key is a hash of that transform, so equal states share one render.
This relies on render_set.py's call order (lead decision df40a083, no kit hook): build_props runs before the ori B
mirror is put on SetWorld (so the homes are stored in the set frame and composed with W0's world matrix per frame),
and set_state calls prop_state right after use(clip, f) has posed the rig, before any pass renders. Should the kit
ever render without calling prop_state per frame, the held props would stay at their homes and verify_sit_a.py's
hand-off / join / seam checks would catch the jump.

Choreography (ori A), every set: <x>_sit (12) two backward waddle steps from H onto the cushion, a little hop, plop
into the plush sit (anim_clips.SIT_LEG: the leg stubs swing out in front over the cushion, pads on the legs,
fL = fR = hL = hR = 0; Z = the cushion top, bob SEAT_BOB_PX), then the paws fetch the prop;
<x>_stand (12) puts it back and reverses. Joins: sit last = loop 0 = stand 0. Hand-offs: sit frame 0 and stand's
last = props + idle#0 at H.
  tea_sip   (36, loop) the mug in both mittens on the lap, up to the mouth, a long sip with closed eyes, down, a
                       happy sigh (baked).
  snack_munch (30, loop) a big bite (mouth wide), chew, a second bite, chew, the cookie gone, the mitten dips into the
                       jar and comes back with a fresh cookie (= frame 0) (baked). Cookie variants park inside the jar.
  knit      (36, loop) the needles and a little rose swatch in both mittens at belly height, alternating needle clicks,
                       a yarn strand to the ball; head upright, mouth closed (set_clips face layers, patch SE / SW).
"""
import math
from anim_clips import REST_K, SIT_LEG, SIT_BOB

REST_S = dict(REST_K, X=0.0, Y=0.0, Z=0.0, yaw=0.0)
HAND_YAW = 90.0
SET_NAMES = ('tea', 'snack', 'knit')
LOOPS = dict(tea='tea_sip', snack='snack_munch', knit='knit')
COUNTS = dict(tea=(12, 36, 12), snack=(12, 30, 12), knit=(12, 36, 12))
HELD = dict(tea=['mug'], snack=['cookie', 'cookie_bit', 'cookie_bit2'], knit=['needles'])


def _set(s, note, ori_prop):
    sit, loop, stand = f'{s}_sit', LOOPS[s], f'{s}_stand'
    n = COUNTS[s]
    return dict(
        clips=[sit, loop, stand],
        counts={sit: n[0], loop: n[1], stand: n[2]},
        loop=[loop],
        joins=[(sit, -1, loop, 0), (stand, 0, loop, 0)],
        handoff={sit: 'start', stand: 'end'},
        hand_yaw=HAND_YAW,
        face=[loop] if s == 'knit' else [],
        patch={'A': 'SE', 'B': 'SW'},
        next={sit: loop, loop: f'{loop} or {stand}', stand: 'Stage A idle (rest pose, same spot and facing)'},
        ori_note={'A': f'the bear faces SE on the floor cushion, {ori_prop} on its right (SW on screen)',
                  'B': f'mirror of A: the bear faces SW, {ori_prop} on its left (SE on screen)'},
        note=(f'Stage sit_a {s}: {sit} -> {loop} (loop) -> {stand}. {note} Stage B format: each frame is the whole set '
              f'(cushion + props + bear) baked on one canvas; anchor = pixel of the set origin (the cushion centre on '
              f'the floor). Hand-off: {sit} frame 0 and {stand}\'s last frame are pixel-identical to the props-only '
              f'frame + the Stage A idle sprite (frame 0) at handoff.px from the anchor. Joins: {sit} last = {loop} 0 '
              f'= {stand} 0.'),
    )


SETS = {
    'tea': _set('tea', 'The bear sips from a mug held in both mittens (face baked).', 'the mug'),
    'snack': _set('snack', 'The bear munches cookies from a cookie jar (face baked).', 'the cookie jar'),
    'knit': _set('knit', 'The bear knits a little swatch (face layers from face_sit_a.json set_clips, patch in '
                 'face_patch_dir).', 'the yarn ball'),
}

# prop pixel table (pixelize_set.py): pass id (kit range 50+), ramp + 3 indices dark -> light, material group
PARTS = {
    'cushion': dict(pid=52, ramp='fabric_sage', idx=(1, 2, 3), mg='sage', thresh=(0.44, 0.70)),
    'cushion_tuft': dict(pid=53, ramp='fabric_cream', idx=(1, 2, 3), mg='cream', thresh=(0.46, 0.70)),
    'mug': dict(pid=54, ramp='fabric_cream', idx=(1, 3, 4), mg='cream', thresh=(0.46, 0.70)),
    'mug_band': dict(pid=55, ramp='fabric_rose', idx=(1, 2, 3), mg='rose', thresh=(0.44, 0.70)),
    'tea': dict(pid=56, ramp='wood', idx=(0, 1, 2), mg='wood', thresh=(0.50, 0.74)),
    'jar': dict(pid=57, ramp='fabric_quilt', idx=(1, 2, 3), mg='quilt', thresh=(0.46, 0.70)),
    'jar_lid': dict(pid=58, ramp='fabric_cream', idx=(1, 2, 3), mg='cream', thresh=(0.46, 0.70)),
    'cookie': dict(pid=59, ramp='wood', idx=(2, 3, 4), mg='wood', thresh=(0.50, 0.74)),
    'chip': dict(pid=60, ramp='wood', idx=(0, 0, 1), mg='wood', thresh=(0.50, 0.74)),
    'yarn': dict(pid=61, ramp='fabric_rose', idx=(1, 2, 3), mg='rose', thresh=(0.44, 0.70)),
    'needle': dict(pid=62, ramp='wood', idx=(1, 2, 3), mg='wood', thresh=(0.50, 0.74)),
    'swatch': dict(pid=63, ramp='fabric_rose', idx=(2, 3, 4), mg='rose', thresh=(0.44, 0.70)),
}

CUSH_W, CUSH_H = 0.62, 0.065          # cushion half extents (x/y, z): a flat square floor cushion, top at 2 * CUSH_H
SEAT_Z = 2 * CUSH_H                   # world z of the seat (snapped to whole pixels in geometry)
SEAT_BACK = 0.12                      # the seat spot sits a little behind the cushion centre (feet stay on it)
HAND_GAP = 0.98                       # seat -> hand-off spot, forward
# the seated body height on the cushion top (whole pixels: the knit loop's face patches need them). anim_clips.SIT_BOB
# (-5.6 px) is fitted for the bare floor; on the soft cushion the seat sinks a little less, the legs' undersides rest
# on the cushion top and the round seat sinks into the cushion (hidden by it).
SEAT_BOB_PX = -5
SIDE_PROP = (0.0, -0.86)              # the small prop's spot (set frame, ori A): beside the cushion, the bear's right
PROPS = {
    'cushion': dict(layers=['cushion'], footprint=dict(x=[-CUSH_W, CUSH_W], y=[-CUSH_W, CUSH_W]),
                    note='a flat sage floor cushion with a cream button tuft'),
    'mug': dict(layers=['mug'], footprint=dict(x=[-0.16, 0.2], y=[-0.16, 0.16]),
                note='a cream mug with a rose band, tea inside; held in both mittens in tea_sip'),
    'cookie_jar': dict(layers=['cookie_jar'], footprint=dict(x=[-0.22, 0.22], y=[-0.22, 0.22]),
                       note='a round blue jar, cookies peeking over the rim'),
    'cookie': dict(layers=['cookie'], footprint=dict(x=[-0.1, 0.1], y=[-0.1, 0.1]),
                   note='the cookie the bear eats (parked inside the jar when not held)'),
    'cookie_bit': dict(layers=['cookie_bit'], footprint=dict(x=[-0.1, 0.1], y=[-0.1, 0.1]),
                       note='the cookie after one bite (parked inside the jar when not held)'),
    'cookie_bit2': dict(layers=['cookie_bit2'], footprint=dict(x=[-0.1, 0.1], y=[-0.1, 0.1]),
                        note='the cookie after two bites (parked inside the jar when not held)'),
    'yarn': dict(layers=['yarn'], footprint=dict(x=[-0.19, 0.19], y=[-0.19, 0.19]), note='a rose yarn ball'),
    'needles': dict(layers=['needles'], footprint=dict(x=[-0.2, 0.2], y=[-0.2, 0.2]),
                    note='two wooden knitting needles with a little rose swatch; rest on the yarn ball'),
    'strand': dict(layers=['strand'], footprint=dict(x=[-0.5, 0.5], y=[-0.5, 0.5]),
                   note='the yarn strand from the ball to the needles (follows the needles)'),
}
SET_PROPS = dict(tea=['cushion', 'mug'], snack=['cushion', 'cookie_jar', 'cookie', 'cookie_bit', 'cookie_bit2'],
                 knit=['cushion', 'yarn', 'needles', 'strand'])

# grip points in the bear's rest frame (faces -Y, left = +X; z up), per held prop: where the prop's origin sits when
# w = 1, relative to the attachment ('both' = the midpoint of the two paw sockets)
GRIP = dict(mug=(0.0, -0.10, -0.09), cookie=(0.0, -0.05, 0.03), cookie_bit=(0.0, -0.05, 0.03),
            cookie_bit2=(0.0, -0.05, 0.03), needles=(0.0, -0.07, -0.04))
KNIT_TILT = -35.0                     # held needles tip forward, away from the face (tilt < 0 = top forward)


def geometry(s, ppu, pxz):
    """set frame = the cushion centre on the floor. Snapped so the seat spot (at the cushion top) and the hand-off
    spot are whole screen pixels from the anchor (face patches and idle#0 land on whole pixels)."""
    import stagekit
    zs = round(SEAT_Z / pxz) * pxz
    sx, sy, seat_px = stagekit.snap(ppu, -SEAT_BACK, 0.0, zs)
    hx, hy, hand_px = stagekit.snap(ppu, sx + HAND_GAP, sy)
    return dict(PXZ=pxz, ppu=ppu, hand=[hx, hy], seat=[sx, sy, zs], hand_px=list(hand_px), seat_px=list(seat_px),
                prop=list(SIDE_PROP))


# ------------------------------------------------------------------ poses
def _lerp(a, b, t): return a + (b - a) * t


def build(s, g):
    PXZ = g['PXZ']; px = PXZ; hx, hy = g['hand']; sx, sy, sz_ = g['seat']
    held = HELD[s]

    def st(**kw):
        d = {h: [0.0, 'both'] for h in held}
        d.update(kw); return d

    def F(stt=None, **kw):
        q = dict(REST_S, X=hx, Y=hy, yaw=HAND_YAW); q.update(kw); return (q, stt if stt is not None else st())

    # the shared plush sit (anim_clips.SIT_LEG): legs out in front over the cushion, the pads stay on their legs. Never
    # a far fL: a pad pushed off its leg reads as a second, detached pair of legs.
    B0 = SEAT_BOB_PX * px
    SIT = dict(X=sx, Y=sy, Z=sz_, bob=B0, legL=SIT_LEG, legR=SIT_LEG, armL=12.0, armR=12.0)

    def S(stt=None, **kw):
        q = dict(REST_S, yaw=HAND_YAW, **SIT); q.update(kw); return (q, stt if stt is not None else st())

    def V(base, stt=None, **kw): return (dict(base[0], **kw), base[1] if stt is None else stt)
    x = lambda t: _lerp(hx, sx, t)
    # ---- sit down onto the cushion (shared start, set-specific fetch), stand up (shared end)
    walk_in = [
        F(),                                                                          # rest at the hand-off spot
        F(X=x(0.16), bob=PXZ, fL=-0.06, hL=0.07, earL=1, earR=1, clY=2),              # waddle back, left foot
        F(X=x(0.34), earL=-1, earR=-1, clY=-2),
        F(X=x(0.52), bob=PXZ, fR=-0.06, hR=0.07, earL=1, earR=1, clX=2),              # right foot
        F(X=x(0.66), bob=-2 * px, sz=0.96, armL=-8.0, armR=-8.0, earL=-2, earR=-2),   # crouch
        F(X=x(0.86), Z=sz_ + 4 * px, sz=1.03, legL=35.0, legR=35.0, armL=-14.0, armR=-14.0, earL=3, earR=3,
          clX=-4),                                                                    # hop up, the legs swing out
        S(bob=B0 - 2 * px, legL=80.0, legR=80.0, sz=0.95, armL=-4.0, armR=-4.0, earL=4, earR=4, clX=4),   # plop
        S(bob=B0 - px, legL=72.0, legR=72.0, sz=0.97, earL=-2, earR=-2, clX=-2),
    ]
    walk_out = [
        F(X=x(0.86), Z=sz_ + 4 * px, sz=1.03, legL=30.0, legR=30.0, armL=-14.0, armR=-14.0, earL=3, earR=3,
          clX=-3),                                                                    # hop off forward, legs come in
        F(X=x(0.62), bob=-2 * px, sz=0.96, armL=-6.0, armR=-6.0, earL=-3, earR=-3, clX=3),       # land
        F(X=x(0.42), bob=PXZ, fL=0.06, hL=0.07, earL=1, earR=1, clY=2),
        F(X=x(0.20), fR=0.06, hR=0.07, earL=-1, earR=-1, clY=-2),
        F(X=x(0.06), bob=PXZ, earL=1, earR=1, clY=1),
        F(),
    ]
    out = {}
    sit, loop, stand = SETS[s]['clips']
    if s == 'tea':
        HOLD = dict(armL=34.0, armR=34.0, spL=-12.0, spR=-12.0)            # mug on the lap, both mittens round it
        T0 = S(st(mug=[1.0, 'both']), **HOLD)
        out[sit] = walk_in + [
            S(spR=36.0, armR=20.0, earR=1, clY=2),                                    # the right mitten reaches out
            S(st(mug=[0.0, 'paw_R']), spR=44.0, armR=24.0, earL=1, earR=2, clY=3),    # takes the mug
            S(st(mug=[0.6, 'both']), spR=10.0, armR=30.0, armL=26.0, earR=-1, clY=-2),
            T0]
        lift = lambda t, **kw: S(st(mug=[1.0, 'both'], tilt=25.0 * t), armL=_lerp(34, 54, t), armR=_lerp(34, 54, t),
                                 spL=-12.0, spR=-12.0, **kw)
        sip = dict(hp=-8.0, blink=0.15)
        out[loop] = [
            T0, T0, V(T0, earL=1, earR=1, clY=2), T0,
            lift(0.3), lift(0.65, earL=2, earR=2), lift(0.9, hp=-4.0), lift(1.0, **sip),
            lift(1.0, **sip, earL=1), lift(1.0, **sip, hb=-1), lift(1.0, **sip, hb=-1, earR=2), lift(1.0, **sip),
            lift(1.0, **sip, earL=-1, clX=2), lift(1.0, **sip, hb=-1, clX=-2), lift(1.0, **sip, hb=-1), lift(1.0, **sip),
            lift(0.9, hp=-4.0, blink=0.15), lift(0.65), lift(0.3, earL=-1, earR=-1),
            V(T0, mouth='open', blink=0.15, hb=1, clY=4),                            # "ahh"
            V(T0, mouth='open', blink=0.15, hb=1, clY=5, earL=3, earR=3),
            V(T0, mouth='open', blink=0.15, hb=1, clY=3, earL=2, earR=2),
            V(T0, blink=0.15, clY=1), V(T0, blink=0.15), V(T0, hr=6.0, blink=0.15, earL=2, earR=-1),
            V(T0, hr=8.0, earL=3, earR=-2, clY=3), V(T0, hr=6.0, earL=2, earR=-1, clY=2), V(T0, hr=3.0, clY=1),
            T0, V(T0, earL=-1, earR=-1), T0, V(T0, clX=2), V(T0, clX=1), T0, V(T0, hb=1), T0,
        ]
        out[stand] = [
            T0,
            S(st(mug=[0.6, 'both']), spR=10.0, armR=30.0, armL=26.0, earR=1, clY=2),
            S(st(mug=[0.0, 'paw_R']), spR=44.0, armR=24.0, earL=1, earR=2, clY=3),  # puts the mug down
            S(spR=28.0, armR=16.0, earR=-1, clY=-2),
            S(armL=-14.0, armR=-14.0, bob=B0 - 2 * px, legL=80.0, legR=80.0, sz=0.96, earL=2, earR=2),         # mittens push up
            S(armL=-8.0, armR=-8.0, legL=58.0, legR=58.0, earL=-1, earR=-1),       # the legs start in
        ] + walk_out
    elif s == 'snack':
        HOLD = dict(armR=40.0, spR=-14.0, armL=14.0)                       # cookie in the right mitten, chest high
        K0 = S(st(cookie=[1.0, 'paw_R']), **HOLD)
        out[sit] = walk_in + [
            S(spR=38.0, armR=22.0, earR=1, clY=2),                                    # the right mitten to the jar
            S(st(cookie=[0.0, 'paw_R']), spR=46.0, armR=26.0, earR=2, clY=3),         # dips in
            S(st(cookie=[0.6, 'paw_R']), spR=20.0, armR=34.0, earR=-1, clY=-2),       # out comes a cookie
            K0]
        bite = lambda c, **kw: S(st(**{c: [1.0, 'paw_R']}), armR=82.0, spR=-16.0, armL=14.0, hp=-6.0, **kw)
        hold = lambda c, **kw: S(st(**{c: [1.0, 'paw_R']}), **dict(HOLD, **kw))
        out[loop] = [
            K0, V(K0, earL=1, earR=1, clY=2), K0,
            S(st(cookie=[1.0, 'paw_R']), armR=62.0, spR=-16.0, armL=14.0, earL=2, earR=2),    # up it goes
            bite('cookie', mouth='wide'), bite('cookie', mouth='wide', blink=0.15),           # chomp
            hold('cookie_bit', mouth='closed', blink=0.15, hb=-1),
            hold('cookie_bit', mouth='open', blink=0.15, earL=2, earR=2),                     # chew chew
            hold('cookie_bit', mouth='closed', blink=0.15, hb=-1, earL=-1, earR=-1),
            hold('cookie_bit', mouth='open', blink=0.15, earL=2, earR=2, clY=3),
            hold('cookie_bit', clY=1),
            S(st(cookie_bit=[1.0, 'paw_R']), armR=62.0, spR=-16.0, armL=14.0, earL=2, earR=2),
            bite('cookie_bit', mouth='wide'), bite('cookie_bit', mouth='wide', blink=0.15),
            hold('cookie_bit2', mouth='closed', blink=0.15, hb=-1),
            hold('cookie_bit2', mouth='open', blink=0.15, earL=2, earR=2),
            hold('cookie_bit2', mouth='closed', blink=0.15, hb=-1, earL=-1, earR=-1),
            S(st(cookie_bit2=[1.0, 'paw_R']), armR=70.0, spR=-16.0, armL=14.0, hp=-4.0, mouth='wide'),   # last bit
            S(armR=56.0, spR=-16.0, armL=14.0, mouth='open', blink=0.15, hb=-1, earL=2, earR=2),
            S(armR=24.0, armL=14.0, mouth='closed', blink=0.15, earL=-1, earR=-1),
            S(armR=12.0, armL=12.0, mouth='tongue', blink=0.15, hr=5.0, clY=3),                   # yum
            S(armR=12.0, armL=12.0, mouth='tongue', blink=0.15, hr=6.0, clY=4, earL=2),
            S(armR=12.0, armL=12.0, hr=3.0, clY=2),
            S(spR=38.0, armR=22.0, armL=12.0, earR=1, clY=2),                                    # into the jar again
            S(st(cookie=[0.0, 'paw_R']), spR=46.0, armR=26.0, armL=14.0, earR=2, clY=3),
            S(st(cookie=[0.0, 'paw_R']), spR=44.0, armR=24.0, armL=14.0, earL=1, earR=2, clY=2),
            S(st(cookie=[0.6, 'paw_R']), spR=20.0, armR=34.0, armL=14.0, earR=-1, clY=-2),
            V(K0, earL=-1, earR=-1), V(K0, clY=2), V(K0, clY=1),
        ]
        out[stand] = [
            K0,
            S(st(cookie=[0.6, 'paw_R']), spR=20.0, armR=34.0, armL=14.0, earR=1, clY=2),
            S(st(cookie=[0.0, 'paw_R']), spR=46.0, armR=26.0, earR=2, clY=3),         # back into the jar
            S(spR=28.0, armR=18.0, earR=-1, clY=-2),
            S(armL=-14.0, armR=-14.0, bob=B0 - 2 * px, legL=80.0, legR=80.0, sz=0.96, earL=2, earR=2),
            S(armL=-8.0, armR=-8.0, legL=58.0, legR=58.0, earL=-1, earR=-1),       # the legs start in
        ] + walk_out
    else:   # knit: face layers on the loop (head upright, mouth closed, eyes open; the mittens stay below the face)
        HOLD = dict(armL=20.0, armR=20.0, spL=0.0, spR=0.0)     # no inward spread: in ori B a mitten would cross the mouth patch
        N0 = S(st(needles=[1.0, 'both'], tilt=KNIT_TILT), **HOLD)
        out[sit] = walk_in + [
            S(spR=36.0, armR=22.0, earR=1, clY=2),                                    # the right mitten to the ball
            S(st(needles=[0.0, 'paw_R']), spR=44.0, armR=26.0, earR=2, clY=3),        # plucks the needles
            S(st(needles=[0.6, 'both'], tilt=0.6 * KNIT_TILT), spR=8.0, armR=26.0, armL=22.0, earR=-1, clY=-2),
            N0]
        cl = lambda a, b, **kw: S(st(needles=[1.0, 'both'], tilt=KNIT_TILT + 0.8 * (a - b)), armL=20.0 + a,
                                  armR=20.0 + b, spL=0.0, spR=0.0, **kw)
        out[loop] = [
            N0, cl(4, -2), cl(6, -4, earL=1), cl(2, 0), cl(-2, 4), cl(-4, 6, earR=1), cl(0, 2), N0,       # click-clack
            cl(4, -2, clY=2), cl(6, -4, clY=3), cl(2, 0, clY=1), cl(-2, 4), cl(-4, 6, clX=2), cl(0, 2), N0,
            V(N0, hb=1, earL=2, earR=2), V(N0, hb=1, earL=1, earR=1), N0,                            # a contented breath
            cl(4, -2), cl(6, -4, earL=-1), cl(2, 0), cl(-2, 4), cl(-4, 6, earR=-1), cl(0, 2), N0,
            cl(4, -2, clY=-2), cl(6, -4, clY=-3), cl(2, 0, clY=-1), cl(-2, 4), cl(-4, 6), cl(0, 2), N0,
            V(N0, earL=1, earR=-1, clX=2), V(N0, clX=2), V(N0, clX=1), V(N0, earL=-1, earR=1),
        ]
        out[stand] = [
            N0,
            S(st(needles=[0.6, 'both'], tilt=0.6 * KNIT_TILT), spR=8.0, armR=26.0, armL=22.0, earR=1, clY=2),
            S(st(needles=[0.0, 'paw_R']), spR=44.0, armR=26.0, earR=2, clY=3),        # back onto the ball
            S(spR=28.0, armR=16.0, earR=-1, clY=-2),
            S(armL=-14.0, armR=-14.0, bob=B0 - 2 * px, legL=80.0, legR=80.0, sz=0.96, earL=2, earR=2),
            S(armL=-8.0, armR=-8.0, legL=58.0, legR=58.0, earL=-1, earR=-1),       # the legs start in
        ] + walk_out
    for c, n in SETS[s]['counts'].items(): assert len(out[c]) == n, (c, len(out[c]), n)
    return out


def handoff(s, g):
    sit, _, stand = SETS[s]['clips']
    return {sit: dict(at='start', pos=list(g['hand']), yaw=HAND_YAW),
            stand: dict(at='end', pos=list(g['hand']), yaw=HAND_YAW)}


def mirror_state(st):
    sw = {'paw_L': 'paw_R', 'paw_R': 'paw_L'}
    return {k: ([v[0], sw.get(v[1], v[1])] if isinstance(v, list) else v) for k, v in st.items()}


# ------------------------------------------------------------------ props (Blender only)
_HOME = {}      # layer -> the empty that carries a held prop
_OBJS = {}      # layer -> [objects]
_STRAND = []    # the strand bead objects (knit)


def _shapes_mug():
    return dict(mug=[((0, 0, 0.13), (0.13, 0.13, 0.13), 0.30, 0.9),                            # the body
                     ((0.155, 0, 0.14), (0.06, 0.03, 0.07), 0.9, 0.9)],                          # the handle
                mug_band=[((0, 0, 0.135), (0.135, 0.135, 0.035), 0.30, 0.9)],
                tea=[((0, 0, 0.255), (0.10, 0.10, 0.012), 0.30, 0.9)])


def build_props(s, kit, g):
    """kit (render_set.py): empty(name, loc, parent), solid(name, parent, layer, pid, shapes), W0 (the set root)."""
    P = PARTS; out = {}
    C = kit.empty('P_cushion', (0, 0, 0), kit.W0)
    kit.solid('cushion', C, 'cushion', P['cushion']['pid'], [((0, 0, CUSH_H), (CUSH_W, CUSH_W, CUSH_H), 0.35, 0.25)])
    kit.solid('cushion_tuft', C, 'cushion', P['cushion_tuft']['pid'], [((0, 0, 2 * CUSH_H + 0.006), (0.075, 0.075, 0.028), 0.6, 0.6)])
    out['cushion'] = dict(px=[0, 0], pos=[0.0, 0.0, 0.0])
    px_, py_ = SIDE_PROP
    import stagekit
    ppx = [round(v) for v in stagekit.px_of(g['ppu'], px_, py_)]
    if s == 'tea':
        o = kit.empty('P_mug', (px_, py_, 0), kit.W0)
        for part, shapes in _shapes_mug().items():
            _OBJS.setdefault('mug', []).append(kit.solid(part + '_m', o, 'mug', P[part]['pid'], shapes))
        _HOME['mug'] = o
        out['mug'] = dict(px=ppx, pos=[px_, py_, 0.0])
    elif s == 'snack':
        J = kit.empty('P_jar', (px_, py_, 0), kit.W0)
        kit.solid('jar', J, 'cookie_jar', P['jar']['pid'], [((0, 0, 0.2), (0.2, 0.2, 0.2), 0.45, 0.9)])
        kit.solid('jar_lid', J, 'cookie_jar', P['jar_lid']['pid'], [((0, 0, 0.395), (0.15, 0.15, 0.025), 0.4, 0.9)])
        # two cookies peeking over the rim (static)
        kit.solid('jar_cookies', J, 'cookie_jar', P['cookie']['pid'],
                  [((-0.06, 0.02, 0.44), (0.08, 0.024, 0.08), 0.35, 0.9), ((0.06, -0.02, 0.43), (0.075, 0.024, 0.075), 0.35, 0.9)])
        out['cookie_jar'] = dict(px=ppx, pos=[px_, py_, 0.0])
        for i, name in enumerate(('cookie', 'cookie_bit', 'cookie_bit2')):
            o = kit.empty('P_' + name, (px_, py_, 0.2), kit.W0)       # parked inside the jar (hidden by it)
            r = 0.095
            body = [((0, 0, 0), (r, 0.026, r), 0.35, 0.9)]
            if i >= 1: body = [((-0.015 * i, 0, -0.012 * i), (r - 0.018 * i, 0.026, r - 0.018 * i), 0.35, 0.9)]
            chips = [((cx, -0.026, cz), (0.018, 0.01, 0.018), 0.9, 0.9)
                     for cx, cz in ((-0.038, 0.028), (0.033, -0.03), (-0.01, -0.05))[:3 - i]]
            _OBJS.setdefault(name, []).append(kit.solid(name + '_c', o, name, P['cookie']['pid'], body))
            if chips: _OBJS[name].append(kit.solid(name + '_chip', o, name, P['chip']['pid'], chips))
            _HOME[name] = o
            out[name] = dict(px=ppx, pos=[px_, py_, 0.2])
    else:
        Y = kit.empty('P_yarn', (px_, py_, 0), kit.W0)
        kit.solid('yarn', Y, 'yarn', P['yarn']['pid'], [((0, 0, 0.17), (0.17, 0.17, 0.17), 1.0, 1.0)])
        out['yarn'] = dict(px=ppx, pos=[px_, py_, 0.0])
        # needles + swatch: home = stuck in the top of the ball, leaning
        o = kit.empty('P_needles', (px_, py_, 0.31), kit.W0)
        o.rotation_euler = (math.radians(-30), 0, 0)
        objs = [kit.solid('needle_a', o, 'needles', P['needle']['pid'],
                          [((-0.055, 0, 0.06), (0.016, 0.016, 0.22), 1.0, 1.0), ((-0.055, 0, 0.29), (0.03, 0.03, 0.03), 1.0, 1.0)]),
                kit.solid('needle_b', o, 'needles', P['needle']['pid'],
                          [((0.055, 0, 0.06), (0.016, 0.016, 0.22), 1.0, 1.0), ((0.055, 0, 0.29), (0.03, 0.03, 0.03), 1.0, 1.0)]),
                kit.solid('swatch', o, 'needles', P['swatch']['pid'], [((0, -0.008, 0.0), (0.12, 0.026, 0.1), 0.5, 0.6)])]
        _OBJS['needles'] = objs; _HOME['needles'] = o
        out['needles'] = dict(px=ppx, pos=[px_, py_, 0.31])
        S = kit.empty('P_strand', (0, 0, 0), kit.W0)
        for i in range(STRAND_N):
            _STRAND.append(kit.solid(f'strand_{i:02d}', S, 'strand', P['yarn']['pid'],
                                     [((0, 0, 0), (STRAND_R, STRAND_R, STRAND_R), 1.0, 1.0)]))
        out['strand'] = dict(px=ppx, pos=[px_, py_, 0.0])
    for name, o in _HOME.items(): o['home'] = [list(r) for r in o.matrix_basis]
    return out


STRAND_N, STRAND_R = 11, 0.028


def _held_matrix(layer, spec):
    """the world matrix of a held prop: its home (w = 0) blended into the grip at the paws (w = 1)."""
    import bpy
    from mathutils import Matrix
    O = bpy.data.objects; rig = O['Rig']; W0 = O['SetWorld']
    o = _HOME[layer]
    home = W0.matrix_world @ Matrix(o['home'])
    w, at = spec[0], spec[1]
    if w <= 0: return home
    pb = rig.pose.bones; bones = rig.data.bones

    def delta(b): return pb[b].matrix @ bones[b].matrix_local.inverted()
    mirror = W0.matrix_world.to_3x3().determinant() < 0
    if at == 'both':
        hL = delta('paw_L') @ bones['paw_L'].head_local; hR = delta('paw_R') @ bones['paw_R'].head_local
        D = delta('root'); base = Matrix.Translation((hL + hR) / 2) @ D.to_3x3().to_4x4()
    else:
        D = delta(at); base = Matrix.Translation(D @ bones[at].head_local) @ delta('root').to_3x3().to_4x4()
    tilt = Matrix.Rotation(math.radians(-spec[2] if len(spec) > 2 else 0.0), 4, 'X')
    held = rig.matrix_world @ base @ tilt @ Matrix.Translation(GRIP[layer])
    if mirror: held = held @ Matrix.Diagonal((-1, 1, 1, 1))
    if w >= 1: return held
    return home.lerp(held, w)


def prop_state(s, st):
    """layer -> render key of this frame's prop state. Inside Blender (render_set.py) it also places the held props
    (and the yarn strand) for the frame; the key is a hash of their placement, so equal states share one render."""
    import bpy, hashlib
    keys = {}
    for layer in SET_PROPS[s]:
        if layer in _HOME and layer in st:
            spec = list(st[layer]) + ([st['tilt']] if 'tilt' in st else [])
            M = _held_matrix(layer, spec)
            o = _HOME[layer]
            o.matrix_world = M
            keys[layer] = layer + '_' + hashlib.md5(str([round(v, 5) for r in M for v in r]).encode()).hexdigest()[:10]
        elif layer == 'strand':
            keys[layer] = _place_strand()
        else:
            keys[layer] = layer + '_0'
    bpy.context.view_layer.update()
    return keys


def _place_strand():
    """beads on a drooping curve from the yarn ball's top to the needles' swatch."""
    import bpy, hashlib
    from mathutils import Vector, Matrix
    bpy.context.view_layer.update()
    O = bpy.data.objects
    a = O['yarn'].matrix_world @ Vector((0.0, 0.0, 0.24))
    b = _HOME['needles'].matrix_world @ Vector((0.0, 0.0, -0.04))
    pts = []
    for i in range(STRAND_N):
        t = (i + 0.5) / STRAND_N
        p = a.lerp(b, t); p.z -= 0.18 * math.sin(math.pi * t) * min(1.0, (a - b).length)
        p.z = max(p.z, STRAND_R)
        pts.append(p)
    for ob, p in zip(_STRAND, pts):
        ob.matrix_world = Matrix.Translation(p)
    return 'strand_' + hashlib.md5(str([round(v, 4) for p in pts for v in p]).encode()).hexdigest()[:10]
