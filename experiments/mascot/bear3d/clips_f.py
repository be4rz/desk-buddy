"""Stage f (fidgets-a, aliveness p2): eight small idle fidgets on the stage kit (stages/f.json, sh run_stage.sh f).

Storyboard #sb-fidgets-a (frozen: names, frame counts, dirs, face handling, hand-offs). Stage A's 84x123 frame and
anchor [42,107], 12 fps, every clip one-shot and starting + ending on the rest pose (= idle#0, make_stage.py swaps the
exact idle sprite in on those frames).

  clip               frames  dirs        face    what it shows
  fid_weight_shift   18      F5          layers  weight onto one foot (other heel up 4 px, 1 px settle), then the other
  fid_ear_twitch     14      F5          layers  one ear flicks twice, the other answers once
  fid_look_up        20      F5          baked   the round head tips back (hp -18), holds, comes down
  fid_look_down      20      F5          baked   the round head tips forward (hp +18) to look at its toes
  fid_tummy_rub      20      F5          layers  both mittens on the belly, rubbing small circles
  fid_wave_small     16      F5          layers  one mitten up beside the head (below the eye line), two little waves
  fid_peek           18      F5          baked   mittens up to the cheeks, head ducks, peeks up over them
  fid_butt_wiggle    14      N NE NW     none    seen from behind: the hips (pants) waggle side to side

Cute, not realistic: only the existing mitten blobs swing at the shoulder, the whole round head mass tilts (never a
neck), the ears stay inside +-15 deg, squash <= 6 %. 'layers' clips keep hp = hr = 0, mouth closed, eyes open, sz 1
(squash re-rasterises the fur round the eyes) and the mittens away from the face (verify_face.py).

Poses are anim_clips.REST_K + keys (anim_clips.expand_k eases numbers between keys) or explicit per-frame poses.
"""
import json, math, os
from anim_clips import REST_K, PK   # noqa: F401  (PK: the Stage K parameter list, the kit's contract)

FPS = 12
F5 = ['S', 'SE', 'E', 'W', 'SW']
BACK = ['N', 'NE', 'NW']
FRAME = (84, 123); ANCHOR = (42, 107)
HANDOFF = {'start': 'idle0', 'end': 'idle0'}
ORDER = ['fid_weight_shift', 'fid_ear_twitch', 'fid_look_up', 'fid_look_down', 'fid_tummy_rub', 'fid_wave_small',
         'fid_peek', 'fid_butt_wiggle']
LAYERS = ['fid_weight_shift', 'fid_ear_twitch', 'fid_tummy_rub', 'fid_wave_small']


def clip(frames, dirs, face, keys=None, poses=None):
    c = dict(frames=frames, loop=False, dirs=list(dirs), face=face, handoff=dict(HANDOFF))
    if poses is not None: c['poses'] = poses
    else: c['keys'] = keys
    return c


def build(V, PXZ):
    """the clip table; PXZ = world units per screen pixel of height (bob / foot lift in whole pixels)."""
    C = {}

    # weight onto the right foot (the left heel lifts 4 px, the mittens swing out, a 1 px settle), back through rest,
    # then the other side
    def side(s):          # s = 'L' lifts the left foot (weight on the right)
        o = 'R' if s == 'L' else 'L'; sg = 1 if s == 'L' else -1
        return {f'h{s}': 4 * PXZ, f'f{s}': 0.02, f'arm{s}': -12, f'arm{o}': 10, f'sp{s}': 10, f'sp{o}': 4,
                f'ear{s}': 7, f'ear{o}': -4, 'clY': 9 * sg, 'clX': -3}

    def settle(s):
        o = 'R' if s == 'L' else 'L'; sg = 1 if s == 'L' else -1
        return {f'h{s}': 3 * PXZ, f'f{s}': 0.02, 'bob': -PXZ, f'arm{s}': -9, f'arm{o}': 8, f'sp{s}': 8, f'sp{o}': 3,
                f'ear{s}': 3, f'ear{o}': -2, 'clY': 4 * sg, 'clX': 2}
    C['fid_weight_shift'] = clip(18, F5, 'layers', keys=[
        (0, {}), (4, side('L')), (7, settle('L')), (9, {'clY': -2}),
        (13, side('R')), (15, settle('R')), (17, {})])

    # ear twitch: the left ear flicks out twice (snappy, explicit frames), the right one answers
    E = []
    for q in [{}, {'earL': 6}, {'earL': 15, 'clY': 5}, {'earL': -4}, {'earL': 14, 'clY': 4}, {'earL': 2},
              {'earL': 0, 'clY': -1}, {}, {'earR': 7}, {'earR': 15, 'clY': -5}, {'earR': -2}, {'earR': 3},
              {'clY': 1}, {}]:
        E.append(dict(REST_K, **q))
    C['fid_ear_twitch'] = clip(14, F5, 'layers', poses=E)

    # look up / look down: the whole round head mass pitches (no neck), ears and clover lag, a hold, back
    C['fid_look_up'] = clip(20, F5, 'baked', keys=[
        (0, {}), (4, {'hp': -18, 'earL': -3, 'earR': -3, 'clX': -6}), (6, {'hp': -18, 'clX': 3, 'earL': -1, 'earR': -1}),
        (10, {'hp': -18, 'earL': 2, 'earR': -2}), (13, {'hp': -17, 'earL': -1, 'earR': 1}),
        (17, {'earL': 1, 'earR': 1, 'clX': -2}), (19, {})])
    C['fid_look_down'] = clip(20, F5, 'baked', keys=[
        (0, {}), (4, {'hp': 18, 'earL': 3, 'earR': 3, 'clX': 7}), (6, {'hp': 18, 'clX': 2, 'earL': 1, 'earR': 1}),
        (10, {'hp': 18, 'earL': -2, 'earR': 2, 'clY': 3}), (13, {'hp': 17, 'earL': 2, 'earR': -2, 'clY': -3}),
        (17, {'earL': -1, 'earR': -1, 'clX': -2}), (19, {})])

    # tummy rub: both mittens forward and in onto the lower belly (kept low: in profile a higher circle reaches the
    # mouth), rubbing circles (opposite phase), then down
    P = []
    for i in range(20):
        if i in (0, 19): P.append(dict(REST_K)); continue
        if i <= 3: k = i / 4; a = dict(armL=30 * k, armR=30 * k, spL=-14 * k, spR=-14 * k)
        elif i >= 16: k = (19 - i) / 4; a = dict(armL=30 * k, armR=30 * k, spL=-14 * k, spR=-14 * k)
        else:
            t = 2 * math.pi * (i - 4) / 6          # two circles over frames 4-15
            a = dict(armL=30 + 7 * math.sin(t), spL=-14 + 6 * math.cos(t),
                     armR=30 - 7 * math.sin(t), spR=-14 - 6 * math.cos(t),
                     earL=2 * math.sin(t), earR=-2 * math.sin(t), clY=3 * math.sin(t))
        P.append(dict(REST_K, **a))
    C['fid_tummy_rub'] = clip(20, F5, 'layers', poses=P)

    # small wave: the right mitten goes out sideways first (so in profile it never sweeps past the mouth), up beside
    # the head (below the eye line), waves twice, and down the same way
    C['fid_wave_small'] = clip(16, F5, 'layers', keys=[
        (0, {}), (2, {'armR': 35, 'spR': 30}), (4, {'armR': 92, 'spR': 32, 'earR': -2, 'clY': -2}),
        (6, {'armR': 98, 'spR': 40, 'earR': -3}), (7, {'armR': 98, 'spR': 24, 'clY': 2}),
        (9, {'armR': 98, 'spR': 40, 'earR': -3}), (11, {'armR': 98, 'spR': 24, 'clY': 2}),
        (12, {'armR': 62, 'spR': 32, 'earR': 1}), (13, {'armR': 28, 'spR': 20}), (15, {})])

    # peek: mittens up to the cheeks, the head ducks behind them (eyes shut), then peeks up over them
    C['fid_peek'] = clip(18, F5, 'baked', keys=[
        (0, {}), (3, {'armL': 120, 'armR': 120, 'spL': 6, 'spR': 6, 'earL': 2, 'earR': 2}),
        (5, {'armL': 128, 'armR': 128, 'spL': 2, 'spR': 2, 'hp': 12, 'blink': 0.15, 'earL': 4, 'earR': 4, 'clX': 6}),
        (8, {'armL': 128, 'armR': 128, 'spL': 2, 'spR': 2, 'hp': 12, 'blink': 0.15, 'earL': 3, 'earR': 3, 'clX': 4}),
        (10, {'armL': 124, 'armR': 124, 'spL': 2, 'spR': 2, 'hp': -10, 'earL': -4, 'earR': -4, 'clX': -6}),
        (13, {'armL': 124, 'armR': 124, 'spL': 2, 'spR': 2, 'hp': -10, 'earL': -2, 'earR': -2, 'clX': 2}),
        (15, {'armL': 60, 'armR': 60, 'spL': 4, 'spR': 4, 'earL': 1, 'earR': 1}), (17, {})])

    # butt wiggle (back views): the whole bear swivels about the floor (ty) while the head turns back (hy = -ty),
    # so the round bottom and the pants waggle under a still head; the lifted foot follows, a tiny squash beat
    W = [(0, {})]
    for f, s in ((2, 1), (4, -1), (6, 1), (8, -1), (10, 0.7)):
        lift = 'hR' if s > 0 else 'hL'
        W.append((f, {'ty': 12 * s, 'hy': -12 * s, lift: PXZ, 'sz': 1.025 if f % 4 == 2 else 0.98,
                      'clY': -4 * s, 'earL': 2 * s, 'earR': -2 * s}))
    W += [(12, {'sz': 0.985}), (13, {})]
    C['fid_butt_wiggle'] = clip(14, BACK, 'none', keys=W)
    return C


_PXZ = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'clips.json')))['PXZ']
CLIPS = build(None, _PXZ)     # the table verify_f.py / stagekit read (rig_bear.py calls build with its own PXZ)
