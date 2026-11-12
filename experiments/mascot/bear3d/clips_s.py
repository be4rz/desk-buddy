"""Stage s (react-b, stages/s.json): six reaction clips in Stage A's 84x123 frame (anchor [42, 107]), all in place
(no root motion). Pure Python: rig_bear.py --stage k keys build(V, PXZ); make_stage.py / verify_s.py read CLIPS, ORDER,
CHAINS and the json.

    dangle          16  8 dirs   loop: held up by the scruff, the whole bear 10 px off the floor (lift), a little
                                 scrunched (3 %), mittens out, stub feet kicking in turn, a gentle sway (a slow twist on
                                 the hold + a small pendulum tip), ears and clover lag; face baked (back views show none).
                                 No hand-off: drag start cuts to it, drag end plays land_squash
    land_squash     10  8 dirs   frame 0 = dangle#0 (CHAINS, pixel-equal), drop to the floor, squash 6 %, rebound,
                                 settle on idle#0; face none
    dizzy           30  F5       three toy stars (kit object 'stars' on the head socket) circle above the head, eyes shut
                                 (the swirly-eyes stand-in), the body wobbles in a circle over planted feet; a head
                                 shake throws the stars off; baked
    morning_stretch 30  F5       wind-up, both mittens up high with a 5 % stretch and a big yawn, a bend to each side,
                                 arms down, a happy wiggle, a blink; baked
    evening_yawn    28  F5       a slow yawn (left mitten at the mouth), then the right mitten rubs an eye, sleepy
                                 half-lids, ears droop; baked
    tap_glass       20  S SE SW  leans in toward the camera and taps the 'glass' with the right mitten three times, a
                                 curious head tilt, leans back; baked

Every clip but dangle ends on idle#0 (hand-off idle0); dizzy, morning_stretch, evening_yawn and tap_glass start there.
Cute rules: one round mochi lump (tilts turn it rigidly, no neck, no bending), squash / stretch within 6 %, mitten arms
swing at the shoulder only, stub feet stay stubs (they only kick by fL / fR / hL / hR).
"""
import math
from anim_clips import REST_K    # the Stage K rest pose (never edited here)

F5 = ['S', 'SE', 'E', 'W', 'SW']
SMALL = dict(frame=[84, 123], anchor=[42, 107])
HB = 2.06                        # body height (anim_clips.build): n px of squash -> a scale factor
PXZ_NOMINAL = 0.0288             # world z per screen pixel at ppu 40.07; only for the module-level CLIPS metadata
ORDER = ['dangle', 'land_squash', 'dizzy', 'morning_stretch', 'evening_yawn', 'tap_glass']
IDLE = {'start': 'idle0', 'end': 'idle0'}
LIFT_PX = 10                     # dangle: the whole bear this many screen px off the floor
CHAINS = [('land_squash', 0, 'dangle', 0)]      # (clip, frame, clip, frame): pixel-equal in every direction

# kit object: three toy stars on the head socket (dizzy); pass id 50 = PARTS['stars']
PARTS = {'stars': dict(pid=50, ramp='accent', idx=(4, 4, 5), mg='star', thresh=(0.45, 0.72))}
STAR_COLOURS = ('#e8d49a', '#f4ecc8')            # accent[4], accent[5]: never on the bear
STAR_R, STAR_Z, STAR_SIZE = 0.78, 0.92, 0.15      # orbit radius, height above the head pivot, star half size (world)


def add_objects(kit):
    """the stars: three little 6-point jacks (concave superellipsoids) 120 deg apart round the head's vertical axis;
    orz_stars turns the ring about the head socket's pivot."""
    shapes = []
    for k in range(3):
        a = math.radians(90 + 120 * k)
        shapes.append(((STAR_R * math.cos(a), STAR_R * math.sin(a), STAR_Z), (STAR_SIZE,) * 3, 2.6, 2.6))
    kit.solid('stars', 'head', shapes, PARTS['stars']['pid'])


def _clips():
    def c(n, loop=False, face='baked', dirs=None, handoff=None):
        return dict(frames=n, loop=loop, face=face, dirs=dirs, handoff=handoff, **SMALL)
    return {
        'dangle': c(16, loop=True, handoff={}),
        'land_squash': c(10, face='none', handoff={'end': 'idle0'}),
        'dizzy': c(30, dirs=list(F5), handoff=dict(IDLE)),
        'morning_stretch': c(30, dirs=list(F5), handoff=dict(IDLE)),
        'evening_yawn': c(28, dirs=list(F5), handoff=dict(IDLE)),
        'tap_glass': c(20, dirs=['S', 'SE', 'SW'], handoff=dict(IDLE)),
    }


def build(V, PXZ):
    """V: world units per 2 px along an iso axis (unused: no root motion); PXZ: world z per screen pixel."""
    def sz(n): return 1.0 + n * PXZ / HB
    def K(**kw): return dict(REST_K, **kw)
    out = {}

    # ---------------------------------------------------------------- dangle: held by the scruff, 10 px up
    lift = LIFT_PX * PXZ
    held = dict(lift=lift, sz=0.97, armL=12.0, armR=12.0, spL=14.0, spR=14.0, hp=-3.0, mouth='open')
    dangle = []
    for f in range(16):
        s = math.sin(2 * math.pi * f / 16); c2 = math.sin(2 * math.pi * (f - 2) / 16)    # c2: a lagging copy
        k1 = math.sin(4 * math.pi * f / 16)                                               # feet kick twice a cycle
        dangle.append(K(**dict(held, ty=7.0 * s, tumble=2.5 * c2, armL=12.0 + 8.0 * c2, armR=12.0 - 8.0 * c2,
                               fL=0.05 * k1, fR=-0.05 * k1, hL=max(0.0, k1) * 2 * PXZ, hR=max(0.0, -k1) * 2 * PXZ,
                               earL=-3.0 * c2, earR=3.0 * c2, clY=-6.0 * c2, clX=2.0 * k1)))
    out['dangle'] = dangle

    # ---------------------------------------------------------------- land_squash: drop from dangle#0, squash, settle
    d0 = dict(dangle[0])
    out['land_squash'] = [
        d0,
        K(lift=lift * 0.6, sz=0.99, armL=24, armR=24, spL=18, spR=18, hp=-4, mouth='open', earL=-6, earR=-6, clX=-6),
        K(lift=lift * 0.2, sz=1.02, armL=28, armR=28, spL=16, spR=16, hp=-2, mouth='open', earL=-8, earR=-8, clX=-8),
        K(sz=sz(-4), armL=-8, armR=-8, spL=10, spR=10, hp=6, earL=10, earR=10, clX=10),           # thump: squash
        K(sz=sz(-3), armL=-4, armR=-4, spL=6, spR=6, hp=4, earL=7, earR=7, clX=7, blink=0.45),
        K(sz=sz(2), armL=6, armR=6, hp=-2, earL=-4, earR=-4, clX=-5),                               # rebound
        K(sz=sz(1), armL=3, armR=3, earL=-1, earR=-1, clX=-2),
        K(sz=sz(-1), earL=2, earR=2, clX=2),
        K(earL=1, earR=1, clX=1),
        K()]

    # ---------------------------------------------------------------- dizzy: circling stars, a wobbly circle
    dz = []
    for f in range(30):
        if f in (0, 29): dz.append(K()); continue
        amp = min(1.0, f / 4.0) * (1.0 if f <= 21 else max(0.0, (24 - f) / 3.0))
        ph = 2 * math.pi * (f - 1) / 12.0
        q = K(roll=6.0 * amp * math.sin(ph), lean=4.0 * amp * math.cos(ph),
              hr=-6.0 * amp * math.sin(ph), blink=0.12, mouth='open',
              earL=6.0 * amp * math.sin(ph), earR=-6.0 * amp * math.sin(ph), clY=8.0 * amp * math.cos(ph),
              armL=10.0 * amp, armR=10.0 * amp, spL=12.0 * amp, spR=12.0 * amp)
        if f <= 25: q.update(vis_stars=1, orz_stars=30.0 * (f - 1))
        if f in (24, 25, 26):                                                     # shake it off
            q.update(hy=[14.0, -14.0, 7.0][f - 24], earL=[12.0, -12.0, 6.0][f - 24], earR=[-12.0, 12.0, -6.0][f - 24])
        if f == 27: q.update(blink=1.0, mouth='closed', sz=sz(-1), earL=2.0, earR=2.0)
        if f == 28: q.update(blink=0.45, mouth='closed')
        dz.append(q)
    out['dizzy'] = dz

    # ---------------------------------------------------------------- morning_stretch: the big one
    up = dict(armL=148.0, armR=148.0, spL=18.0, spR=18.0, sz=1.05, bob=1 * PXZ, hp=-12.0, blink=0.12, mouth='wide',
              earL=-6.0, earR=-6.0, clX=-8.0)
    out['morning_stretch'] = [
        K(),
        K(sz=sz(-2), armL=-10, armR=-10, earL=3, earR=3, clX=3),                        # wind-up
        K(sz=0.96, armL=-16, armR=-16, earL=5, earR=5, clX=5, blink=0.6),
        K(sz=1.0, armL=60, armR=60, spL=8, spR=8, hp=-4, blink=0.45, mouth='open', earL=-2, earR=-2),
        K(**dict(up, armL=125, armR=125, sz=1.03, bob=0.0, hp=-8, mouth='open', clX=-5)),
        K(**up),
        K(**dict(up, spL=22, spR=22)),
        K(**dict(up, spL=20, spR=20, clX=-6)),
        K(**dict(up, roll=-4, hr=-3, clY=6)),                                            # bend to one side
        K(**dict(up, roll=-6, hr=-5, clY=9, spL=22, spR=14)),
        K(**dict(up, roll=-6, hr=-5, clY=8, spL=22, spR=14)),
        K(**dict(up, roll=-4, hr=-3, clY=2)),
        K(**dict(up, roll=4, hr=3, clY=-4)),                                             # ... and the other
        K(**dict(up, roll=6, hr=5, clY=-9, spL=14, spR=22)),
        K(**dict(up, roll=6, hr=5, clY=-8, spL=14, spR=22)),
        K(**dict(up, roll=4, hr=3, clY=-2)),
        K(**dict(up, sz=1.04, mouth='open', blink=0.3)),
        K(**dict(up, sz=1.04, mouth='closed', blink=0.45, hp=-8, armL=138, armR=138)),
        K(armL=100, armR=100, spL=14, spR=14, sz=1.0, hp=-3, blink=0.45, earL=-2, earR=-2, clX=-2),
        K(armL=40, armR=40, spL=6, spR=6, sz=0.97, hp=2, earL=4, earR=4, clX=5),            # arms down
        K(armL=4, armR=4, sz=0.96, earL=6, earR=6, clX=7),
        K(sz=sz(1), hy=10, earL=-6, earR=6, clY=-7),                                        # happy wiggle
        K(hy=-10, earL=6, earR=-6, clY=7),
        K(hy=6, earL=-4, earR=4, clY=-4),
        K(hy=-3, earL=2, earR=-2, clY=2),
        K(blink=0.45),
        K(blink=0.12),
        K(blink=0.45),
        K(sz=sz(-1)),
        K()]

    # ---------------------------------------------------------------- evening_yawn: yawn, then rub an eye
    gape = dict(hp=-10.0, mouth='wide', blink=0.12, armL=88.0, spL=8.0, sz=1.04, earL=-5.0, earR=-5.0, clX=-6.0)
    rub = dict(armR=126.0, spR=6.0, hr=-7.0, hp=4.0, blink=0.12, earL=5.0, earR=7.0, clY=-3.0)
    out['evening_yawn'] = [
        K(),
        K(blink=0.6, earL=1, earR=1),
        K(hp=-3, blink=0.45, mouth='open', armL=20, spL=2, earL=-1, earR=-1),
        K(hp=-6, blink=0.3, mouth='open', armL=45, spL=4, sz=1.02, earL=-3, earR=-3, clX=-3),
        K(hp=-8, blink=0.12, mouth='wide', armL=70, spL=6, sz=1.03, earL=-4, earR=-4, clX=-5),
        K(**gape),
        K(**dict(gape, armL=92, clX=-4)),
        K(**dict(gape, hp=-11, armL=90)),
        K(**dict(gape, hp=-9, armL=86, clX=-5)),
        K(hp=-4, mouth='open', blink=0.3, armL=50, spL=4, sz=1.0, earL=1, earR=1, clX=-1),
        K(hp=2, blink=0.45, armL=12, sz=0.98, earL=4, earR=4, clX=3),                           # mouth shut
        K(hp=3, blink=0.45, armR=60, spR=4, hr=-3, earL=4, earR=5),
        K(**dict(rub, armR=100, blink=0.3)),
        K(**rub),                                                                                # mitten at the eye
        K(**dict(rub, armR=132, spR=12, hr=-8)),
        K(**rub),
        K(**dict(rub, armR=132, spR=12, hr=-8)),
        K(**rub),
        K(**dict(rub, armR=132, spR=12, hr=-8)),
        K(**dict(rub, armR=110, blink=0.3)),
        K(hp=4, hr=-3, armR=50, spR=3, blink=0.45, earL=6, earR=6, clX=4),
        K(hp=5, blink=0.45, earL=7, earR=7, clX=5, sz=0.98),                                    # sleepy droop
        K(hp=5, blink=0.3, earL=7, earR=7, clX=5, sz=0.98),
        K(hp=4, blink=0.45, earL=6, earR=6, clX=4, sz=0.98),
        K(hp=2, blink=0.6, earL=4, earR=4, clX=2),
        K(blink=0.45, earL=2, earR=2),
        K(blink=0.8, earL=1, earR=1),
        K()]

    # ---------------------------------------------------------------- tap_glass: lean in, tap tap tap
    lean_in = dict(lean=10.0, armR=104.0, spR=4.0, hr=8.0, hp=-4.0, earL=-3.0, earR=3.0, clX=-4.0)
    def tap(i):                    # the mitten pushes forward with a little body pulse
        return dict(lean_in, lean=13.0, armR=96.0, sz=sz(-1), clX=-1.0 if i % 2 else 1.0)
    out['tap_glass'] = [
        K(),
        K(lean=3, armR=40, hr=3, earL=-1, earR=1),
        K(lean=7, armR=80, spR=4, hr=6, hp=-2, earL=-2, earR=2, clX=-2),
        K(**lean_in),
        K(**dict(lean_in, blink=0.45)),
        K(**lean_in),
        K(**tap(0)),                                                                               # tap 1
        K(**lean_in),
        K(**dict(lean_in, lean=11)),
        K(**tap(1)),                                                                               # tap 2
        K(**lean_in),
        K(**dict(lean_in, lean=11)),
        K(**tap(2)),                                                                               # tap 3
        K(**lean_in),
        K(**dict(lean_in, hr=11, earL=-5, earR=5, clY=4)),                                         # a puzzled tilt
        K(**dict(lean_in, hr=11, earL=-5, earR=5, clY=4, blink=0.45)),
        K(lean=6, armR=60, spR=3, hr=6, earL=-2, earR=2),
        K(lean=1, armR=20, hr=2, sz=sz(-1)),
        K(lean=-1, earL=1, earR=1),
        K()]

    res = {}
    cdefs = _clips()
    for c, ps in out.items():
        assert len(ps) == cdefs[c]['frames'], (c, len(ps), cdefs[c]['frames'])
        res[c] = dict(cdefs[c], poses=ps)
    return res


CLIPS = build(None, PXZ_NOMINAL)    # metadata (frames, loop, dirs, face, handoff, frame, anchor) for the verifier
