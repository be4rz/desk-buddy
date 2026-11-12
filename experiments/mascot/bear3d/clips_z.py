"""Stage z (rare-art, aliveness p5): eleven rare surprises on the stage kit (stages/z.json, sh run_stage.sh z).

Storyboard #sb-rare (frozen: names, frame counts, dirs, face handling, hand-offs). Stage A's 84x123 frame and anchor
[42, 107], 12 fps. Every clip starts and ends on the rest pose (= idle#0; make_stage.py swaps the exact idle sprite in
on those frames, and every kit object is hidden there).

  clip            frames  dirs  face   what it shows
  windmill        24      F5    baked  both mittens circle like a windmill (two turns, one arm half a turn ahead)
  hiccups         30      F5    baked  three hiccup jolts, each puffs a little bubble out of the mouth that floats up
  find_coin       30      F5    baked  spots a coin at its feet, dips, the coin hops into the mitten, held up, pocketed
  snowflake_nose  30      F5    baked  a snowflake drifts down onto the nose, the eyes squint shut (cross-eyed), achoo
  butterfly_ear   32      F5    baked  a butterfly flutters in and lands on the left ear, the ear twitches, it flies off
  peekaboo        26      F5    baked  mittens cover the eyes, a little wiggle, then flung open: boo!, a giggle
  moonwalk        24      8     none   loop: the feet glide back in turn (the forward foot on its toe), the bear slides
                                       backwards (root_px opposite to the facing); frame 0 and 23 are the rest pose
  hat_spring      36      F5    baked  a straw bonnet with a pink flower pops on over the beanie, admire, pops off
  hat_summer      36      F5    baked  a wide-brim sun hat (blue ribbon)
  hat_autumn      36      F5    baked  a brown cap with a golden leaf on top
  hat_winter      36      F5    baked  a rose knit hat with a cream cuff and pom-pom (the pom-pom sits on the clover)

The effect objects are the kit's (add_objects): superellipsoids on the head / root / paw sockets, shown by vis_<name>,
moved by ox_/oy_/oz_<name> (world units, they ride the socket). Colours are existing pack palette ramps (PARTS), so the
pack palettes cover every pixel. Cute, not realistic: the arms only swing at the shoulder (mitten blobs, no elbows),
the whole round head mass tilts (never a neck), ears inside +-15 deg, squash <= 6 %; a stub arm cannot reach the floor,
so the coin hops into the mitten on the dip (toy logic).
"""
import json, math, os
from anim_clips import ease

FPS = 12
F5 = ['S', 'SE', 'E', 'W', 'SW']
DIRS8 = ['S', 'SE', 'E', 'NE', 'N', 'NW', 'W', 'SW']
FRAME = (84, 123); ANCHOR = (42, 107)
HANDOFF = {'start': 'idle0', 'end': 'idle0'}
ORDER = ['windmill', 'hiccups', 'find_coin', 'snowflake_nose', 'butterfly_ear', 'peekaboo', 'moonwalk',
         'hat_spring', 'hat_summer', 'hat_autumn', 'hat_winter']
FRAMES = dict(windmill=24, hiccups=30, find_coin=30, snowflake_nose=30, butterfly_ear=32, peekaboo=26, moonwalk=24,
              hat_spring=36, hat_summer=36, hat_autumn=36, hat_winter=36)
HATS = ['hat_spring', 'hat_summer', 'hat_autumn', 'hat_winter']
MOON_SPEED = 0.5            # x V per frame at full glide (V = the walk step, 2 px per frame on the iso axes)

# kit objects (pixelize_set.py part table): pass id 50+, ramp from the pack palettes + 3 indices dark -> light,
# material group (shading thresholds), as set_lean.py
def _p(pid, ramp, idx, mg, thresh=(0.46, 0.70)): return dict(pid=pid, ramp=ramp, idx=idx, mg=mg, thresh=thresh)
PARTS = {
    'coin_floor': _p(50, 'accent', (4, 4, 5), 'z_gold'),
    'coin_paw': _p(51, 'accent', (4, 4, 5), 'z_gold'),
    'snowflake': _p(52, 'fabric_quilt', (2, 3, 4), 'z_ice'),
    'bfly_open': _p(53, 'fabric_rose', (2, 3, 4), 'z_wing'),
    'bfly_shut': _p(54, 'fabric_rose', (2, 3, 4), 'z_wing'),
    'hic_bubble': _p(55, 'screen', (2, 3, 4), 'z_bubble'),
    'hat_spring': _p(56, 'fabric_cream', (1, 2, 3), 'z_bonnet'),
    'hat_spring_petals': _p(57, 'fabric_rose', (2, 3, 4), 'z_petal'),
    'hat_spring_bud': _p(58, 'accent', (4, 4, 5), 'z_gold'),
    'hat_summer': _p(59, 'wood', (2, 3, 4), 'z_straw'),
    'hat_summer_ribbon': _p(60, 'fabric_quilt', (1, 2, 3), 'z_ribbon'),
    'hat_autumn': _p(61, 'wood', (1, 2, 3), 'z_cap'),
    'hat_autumn_leaf': _p(62, 'accent', (4, 4, 5), 'z_gold'),
    'hat_winter': _p(63, 'fabric_rose', (1, 2, 3), 'z_knit'),
    'hat_winter_trim': _p(64, 'fabric_cream', (2, 3, 4), 'z_trim'),
}
# a hat is several objects shown together
HAT_OBJS = {'hat_spring': ['hat_spring', 'hat_spring_petals', 'hat_spring_bud'],
            'hat_summer': ['hat_summer', 'hat_summer_ribbon'],
            'hat_autumn': ['hat_autumn', 'hat_autumn_leaf'],
            'hat_winter': ['hat_winter', 'hat_winter_trim']}

# geometry (bear.blend, world units; the head socket pivot is (0, 0, 1.40), the face looks along -Y):
# beanie dome centre (0, 0.12, 2.00) radii (0.30, 0.29, 0.22); clover leaves z 2.45..2.64 about (0, -0.08);
# ear_L top (0.40, 0.13, 2.23); nose tip (0, -0.594, 1.58); mouth (0, -0.57, 1.47)
HEAD_Z = 1.40
BEANIE = (0.0, 0.12, 2.00 - HEAD_Z)
NOSE = (0.0, -0.66, 1.60 - HEAD_Z)          # just in front of the nose tip
MOUTH = (0.0, -0.66, 1.47 - HEAD_Z)
EAR_L_TOP = (0.40, 0.13, 2.30 - HEAD_Z)      # where the butterfly sits (on top of the left ear)
COIN_FLOOR = (-0.16, -0.52, 0.03)            # root socket: on the floor in front of the right foot


def add_objects(kit):
    """the clip effects (rig_bear.py --stage k calls this once; every object starts hidden)."""
    def sh(c, r, e1=1.0, e2=1.0): return (c, r, e1, e2)
    disc = (0.11, 0.11, 0.028)
    kit.solid('coin_floor', 'root', [sh(COIN_FLOOR, disc, 0.4)], PARTS['coin_floor']['pid'])
    kit.solid('coin_paw', 'paw_R', [sh((0.0, -0.05, -0.03), disc, 0.4)], PARTS['coin_paw']['pid'])
    # a six-pointed star blob: a superellipsoid with exponents > 2 is concave (an astroid)
    kit.solid('snowflake', 'head', [sh(NOSE, (0.11, 0.11, 0.11), 2.6, 2.6)], PARTS['snowflake']['pid'])
    bx, by, bz = EAR_L_TOP
    kit.solid('bfly_open', 'head', [sh((bx - 0.12, by, bz + 0.03), (0.12, 0.06, 0.105)),
                                    sh((bx + 0.12, by, bz + 0.03), (0.12, 0.06, 0.105)),
                                    sh((bx, by, bz), (0.04, 0.04, 0.08))], PARTS['bfly_open']['pid'])
    kit.solid('bfly_shut', 'head', [sh((bx, by, bz + 0.10), (0.07, 0.09, 0.14)),
                                    sh((bx, by, bz), (0.04, 0.04, 0.08))], PARTS['bfly_shut']['pid'])
    kit.solid('hic_bubble', 'head', [sh(MOUTH, (0.085, 0.085, 0.085))], PARTS['hic_bubble']['pid'])
    x0, y0, z0 = BEANIE
    # spring: a straw bonnet over the beanie, a pink five-petal flower with a gold centre at the front left
    kit.solid('hat_spring', 'head', [sh((x0, y0, z0 + 0.02), (0.35, 0.34, 0.27)),
                                     sh((x0, y0 - 0.01, z0 - 0.02), (0.40, 0.39, 0.06), 0.5)], PARTS['hat_spring']['pid'])
    fc = (0.16, -0.16, z0 + 0.19)
    petals = [sh((fc[0] + 0.085 * math.cos(math.radians(90 + 72 * k)), fc[1] - 0.02,
                  fc[2] + 0.085 * math.sin(math.radians(90 + 72 * k))), (0.065, 0.05, 0.065)) for k in range(5)]
    kit.solid('hat_spring_petals', 'head', petals, PARTS['hat_spring_petals']['pid'])
    kit.solid('hat_spring_bud', 'head', [sh((fc[0], fc[1] - 0.06, fc[2]), (0.05, 0.04, 0.05))], PARTS['hat_spring_bud']['pid'])
    # summer: a wide flat brim and a round crown over the beanie, a blue ribbon round the crown
    kit.solid('hat_summer', 'head', [sh((x0, y0 - 0.02, z0 + 0.05), (0.62, 0.60, 0.035), 0.35),
                                     sh((x0, y0, z0 + 0.07), (0.33, 0.32, 0.22))], PARTS['hat_summer']['pid'])
    kit.solid('hat_summer_ribbon', 'head', [sh((x0, y0, z0 + 0.12), (0.338, 0.328, 0.05), 0.4)],
              PARTS['hat_summer_ribbon']['pid'])
    # autumn: a brown cap and a big golden leaf lying on its top, pointing forward
    kit.solid('hat_autumn', 'head', [sh((x0, y0, z0 + 0.02), (0.34, 0.33, 0.27)),
                                     sh((x0, y0 - 0.30, z0 - 0.02), (0.26, 0.16, 0.03), 0.4)], PARTS['hat_autumn']['pid'])
    kit.solid('hat_autumn_leaf', 'head', [sh((x0 + 0.02, y0 - 0.04, z0 + 0.29), (0.16, 0.28, 0.035), 0.6, 1.6)],
              PARTS['hat_autumn_leaf']['pid'])
    # winter: a rose knit dome, a cream cuff and a cream pom-pom right where the clover leaves are
    kit.solid('hat_winter', 'head', [sh((x0, y0, z0 + 0.03), (0.35, 0.34, 0.31))], PARTS['hat_winter']['pid'])
    kit.solid('hat_winter_trim', 'head', [sh((x0, y0 - 0.01, z0 - 0.02), (0.385, 0.375, 0.08), 0.5),
                                          sh((0.0, -0.08, 2.55 - HEAD_Z), (0.18, 0.18, 0.17))],
              PARTS['hat_winter_trim']['pid'])


def show(names, **off):
    """pose keys that show these objects (vis 1) with the same offsets for all of them (ox / oy / oz)."""
    d = {}
    for n in names:
        d[f'vis_{n}'] = 1
        for k, v in off.items(): d[f'{k}_{n}'] = v
    return d


def build(V, PXZ):
    """the clip table; PXZ = world units per screen pixel of height, V = the walk step (moonwalk)."""
    def px(n): return n * PXZ
    C = {}

    def clip(name, face='baked', keys=None, poses=None, dirs=F5, loop=False, after=None):
        n = FRAMES[name]
        if keys is not None:
            assert keys[0] == (0, {}) and keys[-1] == (n - 1, {}), f'{name}: frame 0 and last must be the rest pose'
        c = dict(frames=n, loop=loop, dirs=list(dirs), face=face, handoff=dict(HANDOFF))
        if poses is not None: c['poses'] = poses
        else: c['keys'] = keys
        if after is not None: c['root_after'] = after
        C[name] = c

    # windmill: both mittens circle forward (over the top and down the front) two whole turns, the left half a turn
    # behind the right, spread out so they clear the head; a happy squint, mouth open, a bob on each half turn
    n = FRAMES['windmill']; ps = []
    for f in range(n):
        if f in (0, n - 1): ps.append({}); continue
        t = f / (n - 1); w = math.sin(math.pi * t) ** 0.7          # 0 at the ends, 1 mid-clip
        a = 720.0 * ease(t)
        ps.append(dict(armR=a, armL=a - 180.0 * w, spL=36 * w, spR=36 * w, blink=1.0 - 0.55 * w,
                       mouth='open' if 0.25 < t < 0.85 else 'closed',
                       bob=px(1) if w > 0.5 and math.cos(math.radians(2 * a)) > 0 else 0.0,
                       sz=1.0 + 0.02 * w, earL=-8 * w * math.cos(math.radians(a)), earR=8 * w * math.cos(math.radians(a)),
                       clX=-6 * w * math.sin(math.radians(a)), clY=5 * w * math.cos(math.radians(a)),
                       hy=6 * w * math.sin(math.radians(a))))
    clip('windmill', poses=ps)

    # hiccups: three jolts (f5, f14, f23): the body hops 2 px, the head jerks back, eyes pop open, mouth 'open', the
    # mittens jerk out; each puffs a bubble out of the mouth that floats up and away and pops (hidden). Between them a
    # puzzled tilt with heavy eyes; after the third a relieved sigh
    def hic(f0, side):
        jolt = dict(bob=px(2), sz=1.04, hp=-8, mouth='open', earL=-10, earR=-10, armL=12, armR=12, spL=14, spR=14,
                    clX=-8, hr=3 * side)
        return [(f0, dict(jolt, **show(['hic_bubble'], oz=0.0, ox=0.0))),
                (f0 + 1, dict(sz=0.97, hp=3, earL=4, earR=4, clX=5, mouth='closed', **show(['hic_bubble'], oz=0.12, ox=0.03 * side))),
                (f0 + 3, dict(blink=0.6, hr=6 * side, earL=2, earR=2, **show(['hic_bubble'], oz=0.34, ox=0.08 * side))),
                (f0 + 5, dict(blink=0.6, hr=5 * side, **show(['hic_bubble'], oz=0.55, ox=0.12 * side, oy=-0.04))),
                (f0 + 6, dict(blink=0.6, hr=4 * side))]
    clip('hiccups', keys=[(0, {}), (3, dict(blink=0.8, hp=2))] + hic(5, 1) + hic(14, -1) + hic(23, 1)[:4]
         + [(29, {})])

    # find_coin: a coin lies at the feet; the bear spots it (looks down, ears perk), a little surprised hop, dips
    # (head down, squash, the right mitten swings forward and down), the coin hops into the mitten, held up high and
    # admired (happy squint, mouth open, a hop and a head turn), then the mitten drops to the pocket and the coin is
    # gone, a satisfied pat
    flo = show(['coin_floor']); paw = show(['coin_paw'])
    clip('find_coin', keys=[
        (0, {}), (1, dict(flo)), (3, dict(flo, hp=14, earL=-6, earR=-6, clX=-4)),
        (5, dict(flo, hp=16, mouth='open', bob=px(1), sz=1.03, earL=-9, earR=-9, clX=-7, armL=-10, armR=-10, spL=10, spR=10)),
        (6, dict(flo, hp=16, mouth='open', earL=-8, earR=-8)),
        (9, dict(flo, hp=19, sz=0.95, armR=38, spR=-8, armL=-8, earL=6, earR=6, clX=8)),
        (10, dict(paw, hp=18, sz=0.95, armR=42, spR=-8, armL=-8, earL=6, earR=6, clX=8)),
        (13, dict(paw, hp=-10, armR=122, spR=12, blink=0.45, mouth='open', earL=-4, earR=-4, clX=-5)),
        (15, dict(paw, hp=-12, armR=128, spR=12, blink=0.45, mouth='wide', bob=px(2), sz=1.03, earL=-8, earR=-8, clX=-8)),
        (17, dict(paw, hp=-10, armR=124, spR=12, blink=0.45, mouth='open', sz=0.97, earL=5, earR=5, clX=6)),
        (19, dict(paw, hp=-8, hy=14, armR=124, spR=12, blink=0.45, mouth='open', clY=4)),
        (21, dict(paw, hp=-6, hy=-8, armR=118, spR=12, blink=0.45, clY=-3)),
        (23, dict(paw, armR=40, spR=14, hp=2, blink=0.6)),
        (24, dict(armR=-12, spR=18, hp=4, blink=0.45)),
        (26, dict(armR=-6, spR=14, hp=2, blink=0.45, bob=px(1))), (27, dict(armR=-10, spR=16, blink=0.45)),
        (29, {})])

    # snowflake_nose: a flake twirls down from above (side to side), the bear looks up at it, it lands on the nose:
    # the eyes squint shut (cross-eyed), the head dips to look; ah- -choo! the flake pops up off the nose, a shake, a smile
    def flake(oz, ox=0.0, oy=0.0): return show(['snowflake'], oz=oz, ox=ox, oy=oy)
    clip('snowflake_nose', keys=[
        (0, {}), (2, dict(flake(1.15, 0.22, 0.10))), (5, dict(flake(0.85, -0.10, 0.06), hp=-8, earL=-3, earR=-3)),
        (8, dict(flake(0.50, 0.14, 0.03), hp=-12, earL=-5, earR=-5, clX=-4)),
        (10, dict(flake(0.22, -0.05, 0.0), hp=-8, earL=-4, earR=-4)),
        (12, dict(flake(0.0), hp=6, blink=0.3, earL=4, earR=4, clX=4)),
        (16, dict(flake(0.0), hp=8, blink=0.3, hr=3, earL=5, earR=5, clX=5)),
        (18, dict(flake(0.0), hp=-12, blink=0.3, mouth='open', sz=1.04, earL=-7, earR=-7, clX=-7)),
        (20, dict(flake(-0.02, 0.0, -0.06), hp=16, blink=0.12, mouth='wide', sz=0.95, earL=11, earR=11, clX=10,
                  armL=26, armR=26, spL=10, spR=10)),
        (21, dict(flake(0.42, 0.14, 0.0), hp=12, blink=0.12, mouth='wide', sz=0.96, earL=9, earR=9, clX=8,
                  armL=20, armR=20, spL=8, spR=8)),
        (22, dict(hp=6, blink=0.45, earL=4, earR=4)), (24, dict(hy=-10, earL=8, earR=-8, clY=-6, blink=0.45)),
        (25, dict(hy=10, earL=-8, earR=8, clY=6, blink=0.45)), (26, dict(hy=0, blink=0.45, mouth='open')),
        (29, {})])

    # butterfly_ear: a butterfly flutters in from the bear's left (wings open / shut every 2 frames), the eyes
    # follow it (head turn and tilt), it lands on the left ear (wings open), the ear twitches, a delighted squint;
    # it takes off and flutters away past the face to the other side
    def wings(o, ox, oy, oz):
        # both wing objects get the offsets (so the hidden one never eases somewhere else), only o is shown
        d = show(['bfly_open', 'bfly_shut'], ox=ox, oy=oy, oz=oz)
        d['vis_bfly_open' if o == 'bfly_shut' else 'vis_bfly_shut'] = 0
        return d
    def bf(f, ox, oy, oz): return wings('bfly_open' if (f // 2) % 2 == 0 else 'bfly_shut', ox, oy, oz)
    # the ear top sits ~10 px under the canvas top: the flight stays at face height and rises onto the ear
    path = {1: (0.30, -0.40, -0.30), 3: (0.32, -0.55, -0.15), 5: (0.24, -0.45, -0.22), 7: (0.20, -0.35, -0.06),
            9: (0.10, -0.15, 0.0), 11: (0.0, 0.0, 0.0)}
    head = {1: {}, 3: dict(hy=12, hp=-4), 5: dict(hy=20, hp=-8, hr=4), 7: dict(hy=22, hp=-10, hr=8),
            9: dict(hy=16, hp=-10, hr=10), 11: dict(hy=10, hp=-6, hr=10, blink=0.6)}
    keys = [(0, {})]
    for f in sorted(path):    # it lands (f11) with its wings open: closed wings on the ear hide behind it in SE
        w = wings('bfly_open', *path[f]) if f == 11 else bf(f, *path[f])
        keys.append((f, dict(w, **head[f], earL=-4 if f >= 5 else 0)))
    rest_on = [(13, 'bfly_open', dict(hy=10, hp=-6, hr=10, blink=0.45, mouth='open', earL=0)),
               (16, 'bfly_open', dict(hy=10, hp=-6, hr=10, blink=0.45, mouth='open', earL=0)),
               (18, 'bfly_open', dict(hy=10, hp=-6, hr=10, blink=0.45, earL=12)),     # ear twitch
               (19, 'bfly_open', dict(hy=10, hp=-6, hr=10, blink=0.45, earL=-2)),
               (20, 'bfly_open', dict(hy=10, hp=-6, hr=10, blink=0.45, earL=10)),
               (22, 'bfly_open', dict(hy=8, hp=-6, hr=8, blink=0.45, earL=0, mouth='open'))]
    for f, o, h in rest_on: keys.append((f, dict(wings(o, 0.0, 0.0, 0.0), **h)))
    away = {23: (-0.05, -0.15, 0.02), 25: (-0.25, -0.30, 0.0), 27: (-0.40, -0.40, -0.05), 28: (-0.50, -0.45, -0.08)}
    for f in sorted(away):
        keys.append((f, dict(bf(f, *away[f]), hy=-6 * (f - 22) / 6, hp=-12, hr=-4, blink=1.0, earL=-4, earR=-4)))
    keys += [(29, dict(hy=-6, hp=-6, blink=0.45, mouth='open', earL=-2, earR=-2)), (31, {})]
    clip('butterfly_ear', keys=keys)

    # peekaboo: both mittens up over the eyes (the shy_hide pose), a hidden wiggle, an anticipation squash, then BOO:
    # mittens flung out wide, head up, eyes wide, mouth wide, a 2 px pop; a giggle, down
    hide = dict(armL=82, armR=82, spL=-26, spR=-26, hp=6, blink=0.12, earL=6, earR=6, clX=5, sz=0.98)
    boo = dict(armL=72, armR=72, spL=46, spR=46, hp=-10, mouth='wide', bob=px(2), sz=1.05, earL=-12, earR=-12, clX=-9)
    clip('peekaboo', keys=[
        (0, {}), (3, dict(armL=60, armR=60, spL=-4, spR=-4, earL=2, earR=2)), (6, hide),
        (8, dict(hide, hy=-6, hr=4, clY=5)), (10, dict(hide, hy=6, hr=-4, clY=-5)),
        (12, dict(hide, sz=0.96, hp=9, clX=7)), (13, boo), (15, dict(boo, bob=px(1), clX=-5)),
        (17, dict(boo, mouth='open', blink=0.45, bob=0.0, sz=1.0, earL=-4, earR=-4, armL=60, armR=60, spL=40, spR=40, clX=2)),
        (19, dict(blink=0.45, mouth='open', bob=px(1), armL=30, armR=30, spL=26, spR=26, earL=2, earR=2, clY=4)),
        (21, dict(blink=0.45, armL=14, armR=14, spL=12, spR=12, clY=-3)), (23, dict(blink=0.7)), (25, {})])

    # moonwalk (loop, 8 dirs, no face): the feet glide back in turn, the forward-moving foot up on its toe (<= 2 px),
    # the body slides backwards smoothly (root track opposite to the facing), a slight forward lean (it looks like it
    # walks forward), a cool head nod and arm swing on the beat. An ease-in/out envelope makes frame 0 and 23 the rest
    # pose: each pass is glide-glide-settle, the seam rest -> rest
    n = FRAMES['moonwalk']
    def env(f): return ease(min(f / 4, 1.0)) * ease(min((n - 1 - f) / 4, 1.0))
    A, H, c = 1.6 * V, 2 * PXZ, MOON_SPEED * V
    ty, ps = 0.0, []
    for f in range(n):
        e = env(f); ph = 2 * math.pi * f / 12
        if f in (0, n - 1): ps.append(dict(ty_px=ty, tx=0.0))
        else:
            fl = A * e * math.sin(ph)
            ps.append(dict(fL=fl, fR=-fl, hL=H * e * max(0.0, math.cos(ph)) ** 0.5, hR=H * e * max(0.0, -math.cos(ph)) ** 0.5,
                           lean=5 * e, armL=-14 * e * math.sin(ph) + 6 * e, armR=14 * e * math.sin(ph) + 6 * e,
                           spL=8 * e, spR=8 * e, hp=-4 * e * abs(math.cos(ph)), hy=-8 * e,
                           bob=px(1) if e > 0.5 and math.cos(2 * ph) > 0.5 else 0.0,
                           earL=4 * e * math.sin(ph), earR=-4 * e * math.sin(ph), clX=4 * e, clY=-4 * e * math.sin(ph),
                           ty_px=ty, tx=0.0))
        ty -= c * e
    clip('moonwalk', face='none', poses=ps, dirs=DIRS8, loop=True, after=[0.0, ty])

    # hats: both mittens up to the head, the hat pops on from above (lands with a squash), the mittens come down,
    # admire: a head turn to each side with a happy squint and a proud chin-up hop; mittens up again, the hat lifts
    # off and pops away (hidden), down
    POP = dict(hat_spring=1.0, hat_summer=1.0, hat_autumn=1.0, hat_winter=0.6)   # the pom-pom tops the canvas
    for h in HATS:
        def hat(oz, **kw): return dict(show(HAT_OBJS[h], oz=oz * POP[h]), **kw)
        up = dict(armL=162, armR=162, spL=20, spR=20, earL=-4, earR=-4, clX=-3)
        clip(h, keys=[
            (0, {}), (3, dict(armL=110, armR=110, spL=24, spR=24, sz=0.98, earL=2, earR=2)), (5, up),
            (6, hat(0.42, **up)), (8, hat(0.0, **dict(up, sz=0.96, earL=8, earR=8, clX=5, hb=-1))),
            (10, hat(0.0, **dict(up, armL=150, armR=150, sz=1.02, earL=-2, earR=-2, clX=-2))),
            (13, hat(0.0, armL=24, armR=24, spL=10, spR=10, blink=0.45, mouth='open')),
            (16, hat(0.0, hy=28, hr=6, blink=0.45, mouth='open', earL=-3, earR=3, clY=5, armL=-8, armR=-8, spL=26, spR=26)),
            (19, hat(0.0, hy=28, hr=6, blink=0.45, clY=3, armL=-8, armR=-8, spL=26, spR=26)),
            (21, hat(0.0, hy=-28, hr=-6, blink=0.45, mouth='open', earL=3, earR=-3, clY=-5, armL=-8, armR=-8, spL=26, spR=26)),
            (23, hat(0.0, hy=-28, hr=-6, blink=0.45, clY=-3, armL=-8, armR=-8, spL=26, spR=26)),
            (25, hat(0.0, hp=-8, bob=px(2), sz=1.03, blink=0.45, mouth='wide', earL=-6, earR=-6, clX=-6, armL=-14, armR=-14, spL=32, spR=32)),
            (26, hat(0.0, sz=0.97, blink=0.45, earL=5, earR=5, clX=4)),
            (28, hat(0.0, **dict(up, armL=150, armR=150))), (30, hat(0.0, **up)),
            (31, hat(0.36, **dict(up, sz=1.02, earL=-6, earR=-6))), (32, dict(up, blink=0.45)),
            (34, dict(armL=40, armR=40, spL=10, spR=10, blink=0.7)), (35, {})])
    return C


_J = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'clips.json')))
CLIPS = build(_J['V'], _J['PXZ'])   # the table verify_z.py / stagekit read (rig_bear.py calls build with its own V, PXZ)
