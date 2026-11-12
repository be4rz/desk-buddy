"""Stage x (gestures-b, aliveness p3): ten emote gestures on the stage kit (stages/x.json, sh run_stage.sh x).

Storyboard #sb-gestures-b (frozen: names, frame counts, dirs, face handling, hand-offs). Stage A's 84x123 frame and
anchor [42,107], 12 fps, every clip a one-shot in S SE E W SW that starts and ends on the rest pose (= idle#0,
make_stage.py swaps the exact idle sprite in on those frames) and stands in place (no root motion).

  clip           frames  face    what it shows
  shrug          16      layers  both mittens out to the sides, the head sinks between the shoulders (hb -1), back
  facepalm       22      baked   one mitten up onto the face, the head drops into it, eyes shut, a slow head shake
  thumbs_up      16      layers  one mitten raised out in front beside the body (the plain mitten), two pumps
  clap           18      baked   both mittens forward, meeting below the chin three times, a happy squint
  point_bubble   18      layers  the mitten on screen right in S (armL) goes out and up-right, toward the bubble
  shy_hide       24      baked   both mittens over the face, the head ducks, a squirm, one mitten lowers to peek
  laugh          20      baked   little hops with a squash, eyes shut, mouth wide, head back, mittens on the tummy
  pout           20      baked   the head turns 45 deg away (hy), chin up, arms crossed low over the belly, a hmph
  proud          20      layers  chest puff (body up 2 px, head up 1 px more), mittens back on the hips, ears perk
  head_scratch   22      baked   confused tilt (hr 10), one mitten on top of the head scratching, eyes half shut

The bear's arms are mitten blobs that only swing at the shoulder (armX forward raise, spX sideways spread): no
elbows, so "on the hips" / "crossed" / "on the face" are whole-arm swings. In this rig armR is on screen LEFT in S
(anim_f fid_wave_small S), so point_bubble uses armL to point where the bubble sits (screen right in S).

Cute, not realistic: the whole round head mass tilts (never a neck), ears inside +-15 deg, squash <= 6 %. 'layers'
clips keep hp = hr = 0, mouth closed, eyes open, sz 1 (squash re-rasterises the fur round the eyes) and the mittens
away from the face (verify_face.py); arms reach up sideways first so in profile they never sweep past the mouth.
"""
import json, os
from anim_clips import REST_K, PK   # noqa: F401  (PK: the Stage K parameter list, the kit's contract)

FPS = 12
F5 = ['S', 'SE', 'E', 'W', 'SW']
FRAME = (84, 123); ANCHOR = (42, 107)
HANDOFF = {'start': 'idle0', 'end': 'idle0'}
ORDER = ['shrug', 'facepalm', 'thumbs_up', 'clap', 'point_bubble', 'shy_hide', 'laugh', 'pout', 'proud',
         'head_scratch']
LAYERS = ['shrug', 'thumbs_up', 'point_bubble', 'proud']
FRAMES = dict(shrug=16, facepalm=22, thumbs_up=16, clap=18, point_bubble=18, shy_hide=24, laugh=20, pout=20, proud=20,
              head_scratch=22)


def build(V, PXZ):
    """the clip table; PXZ = world units per screen pixel of height (bob / foot lift in whole pixels)."""
    def px(n): return n * PXZ
    C = {}

    def clip(name, face, keys=None, poses=None):
        n = FRAMES[name]
        if keys is not None:
            assert keys[0] == (0, {}) and keys[-1] == (n - 1, {}), f'{name}: frame 0 and last must be the rest pose'
        c = dict(frames=n, loop=False, dirs=list(F5), face=face, handoff=dict(HANDOFF))
        if poses is not None: c['poses'] = poses
        else: c['keys'] = keys
        C[name] = c

    # shrug: both mittens swing out to the sides (a little forward), the round head sinks 1 px between the
    # "shoulders", ears flop out; a tiny second bounce of the arms, then down
    out = dict(armL=40, armR=40, spL=32, spR=32, hb=-1, earL=9, earR=9, clX=-4)
    clip('shrug', 'layers', keys=[
        (0, {}), (3, dict(out, armL=26, armR=26, spL=22, spR=22, hb=0, earL=4, earR=4)), (5, out),
        (7, dict(out, armL=44, armR=44, spL=33, spR=33, earL=11, earR=11, clX=-2)), (9, dict(out, spL=30, spR=30, earL=8, earR=8, clX=3)),
        (11, dict(out, clY=4)), (13, dict(armL=8, armR=8, spL=14, spR=14, earL=2, earR=2, clX=2)), (15, {})])

    # facepalm: the right mitten comes up onto the face, the head drops forward into it (eyes shut, ears droop),
    # a slow side-to-side "oh no" shake of the head inside the paw, then the paw drops and the head comes up
    palm = dict(armR=96, spR=-24, hp=16, blink=0.12, earL=9, earR=9, clX=6, sz=0.98, armL=-6)
    clip('facepalm', 'baked', keys=[
        (0, {}), (3, dict(armR=60, spR=-6, earL=2, earR=2)), (6, palm), (8, dict(palm, hp=18, clX=8)),
        (10, dict(palm, hy=9, earL=11, earR=7, clY=5)), (12, dict(palm, hy=-9, earL=7, earR=11, clY=-5)),
        (14, dict(palm, hy=7, clY=3)), (16, dict(palm, hy=0, hp=15)),
        (18, dict(armR=40, spR=4, hp=4, blink=0.45, earL=3, earR=3, clX=2)), (20, dict(earL=-1, earR=-1, clX=-1)),
        (21, {})])

    # thumbs up: the left mitten (screen right in S) rises out to the side and forward to chest height (it is the
    # plain mitten: the rig has no thumb), two little pumps, a proud ear perk; down
    up = dict(armL=60, spL=37, earL=-4, earR=-4, clX=-3)
    clip('thumbs_up', 'layers', keys=[
        (0, {}), (2, dict(armL=26, spL=30)), (4, up), (6, dict(up, armL=64, spL=39, clY=-3)), (7, dict(up, armL=58)),
        (9, dict(up, armL=64, spL=39, clY=3)), (10, dict(up, armL=58)), (12, dict(up, clX=1)),
        (14, dict(armL=24, spL=24, earL=-1, earR=-1)), (15, {})])

    # clap: both mittens forward, apart then meeting in front below the chin, three claps (eyes in a happy squint,
    # mouth open on the claps, a 1 px bounce on each), down
    apart = dict(armL=72, armR=72, spL=16, spR=16, blink=0.45, earL=-3, earR=-3)
    meet = dict(armL=76, armR=76, spL=-12, spR=-12, blink=0.45, mouth='open', bob=px(1), earL=3, earR=3, clX=4)
    clip('clap', 'baked', keys=[
        (0, {}), (3, dict(apart, blink=1.0)), (5, meet), (7, apart), (9, dict(meet, clX=-3)), (11, apart),
        (13, dict(meet, clX=5)), (15, dict(armL=30, armR=30, spL=6, spR=6, blink=0.6, earL=1, earR=1)), (17, {})])

    # point at the bubble: armL (screen right in S) goes out sideways first, then up to point high and to the side
    # (up-right in S, where the speech bubble sits), two little jabs, ears perk toward it, down the same way
    point = dict(armL=138, spL=42, earL=-5, earR=-2, clY=4)
    clip('point_bubble', 'layers', keys=[
        (0, {}), (2, dict(armL=46, spL=34)), (3, dict(armL=78, spL=48)), (4, dict(armL=104, spL=44, earL=-2)), (6, point),
        (8, dict(point, armL=146, spL=40, clY=6)), (9, point), (11, dict(point, armL=146, spL=40, clY=2)), (12, point),
        (14, dict(armL=104, spL=44, earL=-1, clY=1)), (15, dict(armL=78, spL=48)), (16, dict(armL=46, spL=34)), (17, {})])

    # shy hide: both mittens up over the face, the head ducks behind them (eyes shut), a bashful squirm side to side,
    # the right mitten lowers so one eye peeks out, hides again, the mittens come down with a blush-y bob
    hide = dict(armL=82, armR=82, spL=-26, spR=-26, hp=6, blink=0.12, earL=6, earR=6, clX=5, sz=0.98)
    clip('shy_hide', 'baked', keys=[
        (0, {}), (3, dict(armL=60, armR=60, spL=-4, spR=-4, earL=2, earR=2)), (6, hide),
        (8, dict(hide, ty=8, hy=-4, hr=4, clY=5)), (10, dict(hide, ty=-8, hy=4, hr=-4, clY=-5)), (12, dict(hide)),
        (14, dict(hide, armR=66, spR=4, hp=4, blink=1.0, hr=-6, earL=4, earR=4)),
        (16, dict(hide, armR=68, spR=4, hp=4, blink=1.0, hr=-6, earL=3, earR=5)),
        (18, dict(hide, earL=7, earR=7)), (21, dict(armL=40, armR=40, spL=4, spR=4, hp=3, earL=2, earR=2)),
        (23, {})])

    # laugh: the head tips back, eyes shut, mouth wide; three little hops with a squash on each landing, mittens
    # holding the tummy (low, forward and in), ears bouncing; settles with a smile-squint
    def hop(n): return dict(bob=px(n), hL=px(n), hR=px(n), sz=1.03)
    base = dict(hp=-10, blink=0.12, mouth='wide', armL=30, armR=30, spL=-12, spR=-12, earL=-4, earR=-4, clX=-5)
    land = dict(base, sz=0.95, earL=8, earR=8, clX=6)
    clip('laugh', 'baked', keys=[
        (0, {}), (2, dict(hp=-4, blink=0.45, mouth='open', armL=16, armR=16, spL=-6, spR=-6)),
        (4, dict(base, **hop(3))), (6, land), (8, dict(base, **hop(3), hr=5)), (10, dict(land, hr=4)),
        (12, dict(base, **hop(2), hr=-5)), (14, dict(land, hr=-3, sz=0.96)),
        (16, dict(hp=-3, blink=0.45, mouth='open', armL=14, armR=14, spL=-4, spR=-4, sz=1.01, earL=2, earR=2)),
        (18, dict(blink=0.6, earL=-1, earR=-1)), (19, {})])

    # pout: arms fold low over the belly (both mittens forward and crossing in), the whole round head turns 45 deg
    # away (hy) with the chin up, eyes half shut, ears back; a "hmph" 1 px bob, hold, then it turns back
    fold = dict(armL=34, armR=34, spL=-30, spR=-30, hy=45, hp=-8, blink=0.45, earL=-7, earR=-7, clX=-4)
    clip('pout', 'baked', keys=[
        (0, {}), (3, dict(armL=26, armR=26, spL=-18, spR=-18, earL=-2, earR=-2)),
        (6, fold), (8, dict(fold, bob=px(1), sz=1.02, clX=-6)), (10, dict(fold, sz=0.98, clX=2)),
        (12, dict(fold, hp=-10, clY=3)), (14, dict(fold, clY=-2)), (16, dict(fold, hy=30, hp=-4, blink=0.7)),
        (18, dict(armL=16, armR=16, spL=-8, spR=-8, earL=-1, earR=-1)), (19, {})])

    # proud: a chest puff (the body rises 2 px, the head 1 px more), mittens swing back and out onto the hips, ears
    # perk; a small smug sway of the clover, then it all settles
    puff = dict(bob=px(2), hb=1, armL=-22, armR=-22, spL=36, spR=36, earL=-6, earR=-6, clX=-5)
    clip('proud', 'layers', keys=[
        (0, {}), (3, dict(armL=-10, armR=-10, spL=14, spR=14, earL=-2, earR=-2)), (6, puff),
        (8, dict(puff, clY=5)), (10, dict(puff, clY=-5, earL=-4)), (12, dict(puff, clY=4, earR=-4)),
        (14, dict(puff, clY=0)), (16, dict(armL=-8, armR=-8, spL=10, spR=10, earL=-1, earR=-1, clX=2)),
        (18, dict(clX=-1)), (19, {})])

    # head scratch: puzzled tilt (hr 10), the right mitten up onto the top of the head, three slow scratches, eyes
    # half shut, the other arm a little out, ears asymmetric; a last puzzled pause, then down
    reach = dict(armR=158, spR=12, hr=10, hp=-3, blink=0.6, armL=10, spL=12, earL=-4, earR=6, clY=6)
    lo = dict(reach, armR=148, spR=16); hi = dict(reach, armR=164, spR=8, earR=8)
    clip('head_scratch', 'baked', keys=[
        (0, {}), (3, dict(armR=96, spR=18, hr=4, earR=2)), (6, reach), (8, lo), (10, hi), (12, lo), (14, hi),
        (16, dict(reach, blink=1.0, clY=3)), (19, dict(armR=60, spR=10, hr=3, armL=4, spL=4, earR=1)), (21, {})])
    return C


_PXZ = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'clips.json')))['PXZ']
CLIPS = build(None, _PXZ)     # the table verify_x.py / stagekit read (rig_bear.py calls build with its own PXZ)
