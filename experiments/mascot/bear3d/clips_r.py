"""Stage r (react-a, aliveness p4): seven reaction clips on the stage kit (stages/r.json, sh run_stage.sh r).

Storyboard #sb-react-a (frozen: names, frame counts, loops, dirs, face handling, hand-offs). Stage A's 84x123 frame
and anchor [42,107], 12 fps, directions S SE E W SW, in place (no root motion). Every clip, loops included, starts and
ends on the rest pose (= idle#0: make_stage.py swaps the exact idle sprite in on those frames), so each loop pass
begins and ends on idle#0 and the brain can leave a loop after any pass.

  clip           frames  loop  face    what it shows
  head_bob       16      yes   baked   two nods to the beat (head mass pitches, a side tilt each beat), body bounce
  foot_tap       12      yes   layers  one stub foot taps twice, a tiny body bob, ears and clover answer
  dance          32      yes   baked   sway left / right with one mitten up, both mittens up with bounces, a hip
                                       twist, a little bow; mouth open, happy squint
  cursor_watch   20      yes   layers  attentive: the body rises 1 px, ears perk, mittens together, the attention
                                       (ears, clover, weight) drifts one way and back the other
  flinch         12      no    baked   snap into a duck (small squash, mittens up, eyes shut, ears back, a lean
                                       back), peek, relief shake
  giggle         20      no    baked   mittens on the tummy, squirming (body roll, counter head tilt), eyes closed,
                                       mouth open / wide, hiccup bounces
  check_watch    22      no    baked   a tiny toy wristwatch (kit objects watch + watch_face on paw_L, shown on
                                       frames 1-20) comes up in front of the chest, the head looks down at it,
                                       blink, a wrist shake, a second look ("oh!"), down

cursor_watch: "head follows" is the brain's turn toward the cursor (face_toward, then this loop); a 'layers' clip keeps
hp = hr = hy = 0 and no lean (the drawn face patches must match), so the attention reads through ears, clover, the
1 px rise and the weight shift.

Cute, not realistic: only the mitten blobs swing at the shoulder, the whole round head mass tilts (never a neck),
ears inside +-15 deg, squash <= 6 %, stub feet lift by whole pixels. 'layers' clips keep the mittens below the face.
"""
import json, math, os
import numpy as np
from anim_clips import REST_K, PK  # noqa: F401  (PK: the Stage K parameter list, the kit's contract)

FPS = 12
F5 = ['S', 'SE', 'E', 'W', 'SW']
FRAME = (84, 123); ANCHOR = (42, 107)
HANDOFF = {'start': 'idle0', 'end': 'idle0'}
ORDER = ['head_bob', 'foot_tap', 'dance', 'cursor_watch', 'flinch', 'giggle', 'check_watch']
LOOPS = ['head_bob', 'foot_tap', 'dance', 'cursor_watch']
LAYERS = ['foot_tap', 'cursor_watch']
OBJECTS = ['watch', 'watch_face']

# kit object pixel table (pixelize_set.py): pass id (kit range 50+), ramp + 3 indices dark -> light, material group,
# shade thresholds
PARTS = {
    'watch': dict(pid=50, ramp='fabric_rose', idx=(1, 2, 3), mg='watch_strap', thresh=(0.44, 0.70)),
    'watch_face': dict(pid=51, ramp='fabric_cream', idx=(2, 3, 4), mg='watch_face', thresh=(0.40, 0.66)),
}

# the left arm as build_bear.py makes it (world units, rest pose): shoulder -> paw centre, bowed outward
_SQ = 0.91
_SH = np.array([0.54, 0.0, 1.38 * _SQ]); _PAW = np.array([0.34, -0.53, 1.02 * _SQ])
_BOW = np.array([0.08, 0.0, -0.03])
WRIST_T = 0.74                   # along the arm (0 shoulder, 1 paw centre): the wrist, just above the mitten
STRAP_R, STRAP_BEAD, STRAP_N = 0.160, 0.036, 16
BEZEL_R, FACE_R = 0.090, 0.066


def _arm_point(t): return _SH + (_PAW - _SH) * t + _BOW * (4 * t * (1 - t))


def watch_geometry():
    """wrist centre, arm axis, the watch-face normal (perpendicular to the axis, toward the front / top: after the arm
    raises forward the face looks up at the bear) and the strap's second axis, in world units."""
    w = _arm_point(WRIST_T)
    ax = (_PAW - _SH) + _BOW * 4 * (1 - 2 * WRIST_T); ax = ax / np.linalg.norm(ax)
    pref = np.array([0.15, -1.0, 0.35])
    n = pref - pref.dot(ax) * ax; n = n / np.linalg.norm(n)
    b = np.cross(ax, n)
    return w, ax, n, b


def add_objects(kit):
    """the toy wristwatch on paw_L: a strap of overlapping beads round the wrist + a chunky bezel (rose), and a cream
    face standing just proud of the bezel. Positions are world points made socket-relative."""
    p0 = np.array(kit.PIV['paw_L'])
    w, ax, n, b = watch_geometry()
    assert np.linalg.norm(w - p0) < 0.45, f'paw_L socket {p0} is not near the modelled wrist {w}'
    rel = lambda v: tuple(float(x) for x in (v - p0))
    strap = []
    for i in range(STRAP_N):
        a = 2 * math.pi * i / STRAP_N
        c = w + STRAP_R * (math.cos(a) * n + math.sin(a) * b)
        strap.append((rel(c), (STRAP_BEAD,) * 3, 1.0, 1.0))
    strap.append((rel(w + (STRAP_R + 0.030) * n), (BEZEL_R,) * 3, 1.0, 1.0))      # the bezel
    kit.solid('watch', 'paw_L', strap, PARTS['watch']['pid'])
    kit.solid('watch_face', 'paw_L', [(rel(w + (STRAP_R + 0.072) * n), (FACE_R,) * 3, 1.0, 1.0)],
              PARTS['watch_face']['pid'])


def clip(frames, face, keys, loop=False):
    assert keys[0] == (0, {}) and keys[-1] == (frames - 1, {}), 'frame 0 and the last frame must be the rest pose'
    return dict(frames=frames, loop=loop, dirs=list(F5), face=face, handoff=dict(HANDOFF), keys=keys)


def build(V, PXZ):
    """the clip table; PXZ = world units per screen pixel of height (bob / foot lift in whole pixels)."""
    def px(k): return k * PXZ
    C = {}

    # head_bob: nod down on the beat (head mass pitches, body dips 1 px, ears flop, happy squint), up between beats;
    # the side tilt alternates per beat, the mittens swing a little
    def down(s): return dict(hp=13, hr=6 * s, bob=-px(1), sz=0.975, earL=7, earR=7, clX=6, clY=5 * s, blink=0.45,
                             armL=8, armR=8)
    def upb(s): return dict(hp=-6, hr=2 * s, bob=px(1), sz=1.02, earL=-4, earR=-4, clX=-5, clY=-3 * s, armL=-6, armR=-6)
    C['head_bob'] = clip(16, 'baked', [(0, {}), (3, down(1)), (6, upb(1)), (10, down(-1)), (13, upb(-1)), (15, {})],
                         loop=True)

    # foot_tap: the right stub foot lifts 5 px (toe forward) and taps down, twice; a 1 px dip on each tap, ears / clover answer
    lift = dict(hR=px(5), fR=0.07, armR=-8, armL=6, earR=4, clY=5)
    tap = dict(bob=-px(1), armR=4, armL=-3, earL=-3, earR=-4, clY=-4)
    C['foot_tap'] = clip(12, 'layers', [(0, {}), (2, lift), (4, tap), (6, lift), (8, tap), (11, {})], loop=True)

    # dance: sway left (left mitten up, right foot up), sway right, both mittens up with bounces, a hip twist, a bow
    O = dict(mouth='open')
    def sway(s):     # s = 1: lean to the bear's left
        hi, lo = ('L', 'R') if s > 0 else ('R', 'L')
        return dict(O, roll=-6 * s, hr=8 * s, **{f'arm{hi}': 125, f'sp{hi}': 35, f'arm{lo}': 22, f'sp{lo}': 6,
                                                  f'h{lo}': px(2), f'ear{hi}': -5, f'ear{lo}': 7},
                    clY=-8 * s, blink=0.5)
    up_both = dict(O, armL=125, armR=125, spL=35, spR=35, bob=px(2), sz=1.03, blink=0.4, earL=-4, earR=-4, clX=-5)
    C['dance'] = clip(32, 'baked', [
        (0, {}), (3, sway(1)), (6, dict(sway(1), bob=px(1))),
        (8, dict(O, armL=40, armR=40, bob=-px(1), sz=0.97, earL=4, earR=4)),
        (11, sway(-1)), (14, dict(sway(-1), bob=px(1))),
        (16, dict(O, armL=40, armR=40, bob=-px(1), sz=0.97, earL=4, earR=4)),
        (19, up_both), (21, dict(up_both, bob=0.0, sz=0.97, clX=4, earL=5, earR=5)), (23, up_both),
        (25, dict(O, ty=15, hy=-10, armL=55, armR=30, spL=20, spR=20, hL=px(2), clY=8, earL=4, earR=-4)),
        (27, dict(O, ty=-15, hy=10, armL=30, armR=55, spL=20, spR=20, hR=px(2), clY=-8, earL=-4, earR=4)),
        (29, dict(hp=10, sz=0.98, armL=18, armR=18, blink=0.45, earL=5, earR=5, clX=6)),
        (31, {})], loop=True)

    # cursor_watch: up 1 px, ears perk, mittens together in front (below the face); the attention drifts to one
    # side (clover, ears, weight onto one foot) and across to the other, then settles
    att = dict(bob=px(1), earL=-6, earR=-6, armL=16, armR=16, spL=-8, spR=-8)
    C['cursor_watch'] = clip(20, 'layers', [
        (0, {}), (3, att), (6, dict(att, clY=7, earL=-2, earR=-9, hR=px(1))), (9, dict(att, clY=5, earL=-3, earR=-8, hR=px(1))),
        (12, dict(att, clY=-7, earL=-9, earR=-2, hL=px(1))), (14, dict(att, clY=-5, earL=-8, earR=-3, hL=px(1))),
        (16, dict(att, clY=0)), (18, dict(armL=6, armR=6, spL=-3, spR=-3, earL=-2, earR=-2)), (19, {})], loop=True)

    # flinch: snap down and back (small squash, mittens up before the face, eyes shut, ears back, mouth open), hold,
    # peek, relief shake, rest
    duck = dict(sz=0.95, bob=-px(1), lean=-10, armL=112, armR=112, spL=-4, spR=-4, hp=14, blink=0.12,
                earL=-13, earR=-13, mouth='open', clX=-8)
    C['flinch'] = clip(12, 'baked', [
        (0, {}), (1, duck), (4, dict(duck, sz=0.955, clX=4, earL=-11, earR=-11)),
        (6, dict(sz=0.97, lean=-6, armL=88, armR=88, spL=-2, spR=-2, hp=8, mouth='open', earL=-7, earR=-7, clX=2)),
        (8, dict(sz=1.03, bob=px(1), lean=2, armL=18, armR=18, hp=-4, earL=6, earR=6, clX=6)),
        (10, dict(sz=0.99, earL=-3, earR=3, clX=-3)), (11, {})])

    # giggle: mittens to the tummy, then a ticklish squirm (body roll one way, head tilt the other), eyes closed,
    # mouth open / wide in turns, little hiccup bounces, settle
    tum = dict(armL=32, armR=32, spL=-14, spR=-14)
    def sq(s, m, k=1.0):
        return dict(tum, roll=5 * s * k, hr=-8 * s * k, blink=0.12, mouth=m, earL=6 * s * k, earR=-6 * s * k,
                    clY=8 * s * k, sz=0.97 if s > 0 else 1.025, bob=0.0 if s > 0 else px(1))
    C['giggle'] = clip(20, 'baked', [
        (0, {}), (2, dict(tum, blink=0.4)), (4, sq(1, 'open')), (6, sq(-1, 'wide')), (8, sq(1, 'open')),
        (10, sq(-1, 'wide')), (12, sq(1, 'open', 0.8)), (14, sq(-1, 'open', 0.6)),
        (16, dict(armL=20, armR=20, spL=-8, spR=-8, blink=0.45, earL=2, earR=-2)), (19, {})])

    # check_watch: the watch shows from frame 1; the left wrist comes up in front of the chest, the head looks down
    # at it (tilting toward it), a blink, a wrist shake, a second look with an "oh!", down
    W = dict(vis_watch=1, vis_watch_face=1)
    look = dict(W, armL=48, spL=-6, hp=16, hr=6, earL=3, earR=3, clX=5)
    C['check_watch'] = clip(22, 'baked', [
        (0, {}), (1, dict(W, armL=8)), (4, dict(W, armL=44, spL=-6, hp=6, earL=2)), (6, look), (9, look),
        (10, dict(look, blink=0.12)), (11, look),
        (13, dict(look, armL=54, spL=-2, hp=12, hr=4, clX=-3)), (14, dict(look, armL=42, spL=-12, hp=12, hr=4)),
        (15, dict(look, armL=54, spL=-2, hp=12, hr=4, clX=3)),
        (16, dict(look, hp=17, hr=7, mouth='open')), (18, dict(look, hp=17, hr=7, mouth='open',
                                                                          earL=-4, earR=-4)),
        (20, dict(W, armL=10, earL=1, earR=1)), (21, {})])
    return C


_PXZ = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'clips.json')))['PXZ']
CLIPS = build(None, _PXZ)     # the table verify_r.py / stagekit read (rig_bear.py calls build with its own PXZ)
