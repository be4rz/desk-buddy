"""Stage p (postures, clip module of the stage kit, stages/p.json): floor postures in S SE E W SW.

Pure Python: rig_bear.py --stage k keys build(V, PXZ); stagekit / verify_p.py read ORDER, CLIPS and CHAINS.
Storyboard rows 18-23 (#sb-postures) are the contract: names, frame counts, loops, face, hand-offs, frames.

  sit_floor   sit_floor_down 10 -> sit_floor 24 (loop, face layers) -> sit_floor_up 10
  sit_sway    sit_floor_down    -> sit_sway 24 (loop, baked)        -> sit_floor_up
  lie_belly   belly_down 14     -> belly_kick 24 (loop, baked)      -> belly_up 14
  lie_back    back_down 14      -> back_lie 30 (loop, face none)    -> back_up 14
  floor_nap   curl_down 14      -> curl_nap 36 (loop, baked)        -> curl_up 16
  squat       squat_down 8      -> squat_inspect 24 (loop, baked)   -> squat_up 8     (84x123)

Hand-offs: every down clip's frame 0 and every up clip's last frame are the rest pose (idle#0, make_stage.py pastes
the Stage A sprite). Chains (CHAINS, checked by stagekit + verify_p.py): down[last] = loop[0] = up[0], one shared
pose per family (SIT, SQ, BELLY, BACK, CURL), so the renders are pixel-identical. Each loop ends one small step
before its frame 0 (the kit's seam check).

Cute rules (design "Common to every art ticket"): one round mochi lump that never bends; mitten arms swing at the
shoulder; squash <= 6%; head tilts only through hp / hr (<= 20 / 15 deg). Legs: sitting and lying poses swing the
body's own leg stubs at the hip (legL / legR, rig_bear.py LEGS) and keep fL / hL = 0, so the pads ride their legs (a pad
pushed away from its leg reads as a second, detached pair of legs); the sit is the shared anim_clips.SIT_LEG with a
whole-pixel bob near SIT_BOB; wiggles, kicks and pedalling are small leg swings (about 10-20 deg), in turn. Only the
squat stands on its feet (legs at rest, fL / hL as in Stage A).
"""
from anim_clips import REST_K, SIT_LEG

F5 = ['S', 'SE', 'E', 'W', 'SW']
WIDE, WIDE_ANCHOR = [132, 123], [66, 107]
NARROW, NARROW_ANCHOR = [84, 123], [42, 107]
ORDER = ['sit_floor_down', 'sit_floor', 'sit_floor_up', 'sit_sway',
         'belly_down', 'belly_kick', 'belly_up',
         'back_down', 'back_lie', 'back_up',
         'curl_down', 'curl_nap', 'curl_up',
         'squat_down', 'squat_inspect', 'squat_up']
CHAINS = []
for _down, _loop, _up in (('sit_floor_down', 'sit_floor', 'sit_floor_up'), ('belly_down', 'belly_kick', 'belly_up'),
                          ('back_down', 'back_lie', 'back_up'), ('curl_down', 'curl_nap', 'curl_up'),
                          ('squat_down', 'squat_inspect', 'squat_up')):
    CHAINS += [(_down, -1, _loop, 0), (_up, 0, _loop, 0)]
CHAINS += [('sit_floor_down', -1, 'sit_sway', 0), ('sit_floor_up', 0, 'sit_sway', 0)]
FAMILIES = {'sit_floor': ('sit_floor_down', 'sit_floor', 'sit_floor_up'),
            'sit_sway': ('sit_floor_down', 'sit_sway', 'sit_floor_up'),
            'lie_belly': ('belly_down', 'belly_kick', 'belly_up'),
            'lie_back': ('back_down', 'back_lie', 'back_up'),
            'floor_nap': ('curl_down', 'curl_nap', 'curl_up'),
            'squat': ('squat_down', 'squat_inspect', 'squat_up')}


def _clip(n, keys, face, loop=False, handoff=None, narrow=False):
    return dict(frames=n, loop=loop, face=face, dirs=list(F5), handoff=handoff or None,
                frame=list(NARROW if narrow else WIDE), anchor=list(NARROW_ANCHOR if narrow else WIDE_ANCHOR), keys=keys)


def build(V, PXZ):
    """the 16 clips as Stage K keys (anim_clips.expand_k). PXZ = world z of one screen pixel."""
    px = PXZ
    R = {}                                                   # rest = REST_K (a key lists only what it changes)

    def k(base, **kw):
        d = dict(base); d.update(kw); return d

    clips = {}
    # ------------------------------------------------------------ sit on the floor (the shared plush sit: legs out in
    # front, seat on the floor). The body's own legs swing (SIT_LEG); the pads ride them (fL = hL = 0). sit_floor is a
    # face 'layers' clip: its idle face patches only match a body moved by whole pixels, so the sit bob is a whole
    # number of screen pixels. -4 px (-0.115, 1.5 px above the shared SIT_BOB -0.16): at -5 and -6 px one shade pixel
    # under the W eye flips at the render threshold and verify_face's exact-frame check fails; -7 px leaves no canvas
    # margin. Probed: -3 / -4 / -7 px match exactly in every dir.
    SITB = -4 * px
    SIT = dict(bob=SITB, legL=SIT_LEG, legR=SIT_LEG, armL=12.0, armR=12.0)
    clips['sit_floor_down'] = _clip(10, [
        (0, R),
        (2, dict(bob=-2 * px, sz=0.97, armL=-6.0, armR=-6.0, earL=2, earR=2)),           # a little knee-less dip
        (5, dict(bob=-0.08, legL=40.0, legR=34.0, armL=-10.0, armR=-10.0, earL=-3, earR=-3, clX=4)),   # legs swing up
        (7, k(SIT, legL=SIT_LEG + 8, legR=SIT_LEG + 8, sz=0.95, earL=4, earR=4, clX=-5)),  # plop: a soft squash
        (8, k(SIT, legL=SIT_LEG - 4, legR=SIT_LEG - 4, sz=0.97, earL=-2, earR=-2, clX=3)),
        (9, SIT)], 'baked', handoff={'start': 'idle0'})
    clips['sit_floor'] = _clip(24, [
        (0, SIT),
        (4, k(SIT, hb=1, earL=-2, earR=-2, clY=3)),                                     # slow breath in
        (8, k(SIT, earL=2, earR=2, clY=-3)),
        (11, k(SIT, legL=SIT_LEG + 12, clX=3)),                                         # left leg wiggles up
        (13, k(SIT, legR=SIT_LEG + 12, clX=-3)),                                        # then the right
        (15, k(SIT, armL=18.0, armR=18.0, earL=-3)),                                    # paws pat the lap
        (17, k(SIT, hb=1, earR=-3, clY=4)),
        (20, k(SIT, earL=1, earR=1, clY=-2)),
        (23, k(SIT, clY=1))], 'layers', loop=True)
    clips['sit_floor_up'] = _clip(10, [
        (0, SIT),
        (2, k(SIT, legL=SIT_LEG - 5, legR=SIT_LEG - 5, sz=0.96, armL=-14.0, armR=-14.0, earL=2, earR=2)),  # mittens push
        (4, dict(bob=-0.08, legL=34.0, legR=40.0, armL=-10.0, armR=-10.0, earL=-3, earR=-3, clX=4)),
        (6, dict(bob=2 * px, sz=1.03, armL=-4.0, armR=-4.0, hL=0.04, hR=0.04, earL=3, earR=3, clX=-4)),   # pop up
        (8, dict(bob=0.0, sz=0.98, earL=-1, earR=-1, clX=2)),
        (9, R)], 'baked', handoff={'end': 'idle0'})
    clips['sit_sway'] = _clip(24, [
        (0, SIT),
        (3, k(SIT, roll=4.0, hr=5.0, earL=3, earR=-2, clY=5, blink=0.15)),             # content, eyes closed; it rocks
        # on its round seat (up 1 px at the widest rock, so the low side stays on the floor)
        (7, k(SIT, roll=7.0, bob=SITB + px, hr=8.0, earL=5, earR=-4, clY=8, blink=0.15, armR=20.0, legR=SIT_LEG + 4)),
        (12, k(SIT, hb=1, earL=0, earR=0, clY=0, blink=0.15)),
        (16, k(SIT, roll=-7.0, bob=SITB + px, hr=-8.0, earL=-4, earR=5, clY=-8, blink=0.15, armL=20.0, legL=SIT_LEG + 4)),
        (20, k(SIT, roll=-3.0, hr=-4.0, earL=-2, earR=3, clY=-4)),
        (23, k(SIT, roll=-1.0, hr=-1.0, clY=-1))], 'baked', loop=True)
    # ------------------------------------------------------------ squat and look at something on the floor (84x123)
    # standing on its feet (legs at rest, pads planted): the small fL is the standing pose's own, not a fake leg.
    SQ = dict(bob=-5 * px, sz=0.95, fL=0.10, fR=0.10, hp=15.0, armL=22.0, armR=22.0)
    clips['squat_down'] = _clip(8, [
        (0, R),
        (3, dict(bob=-2 * px, sz=0.97, hp=6.0, armL=10.0, armR=10.0, earL=2, earR=2)),
        (5, k(SQ, bob=-5 * px, earL=-3, earR=-3, clX=4)),
        (7, SQ)], 'baked', handoff={'start': 'idle0'}, narrow=True)
    clips['squat_inspect'] = _clip(24, [
        (0, SQ),
        (4, k(SQ, hr=8.0, earL=4, clY=4)),                                              # curious head tilt
        (7, k(SQ, hr=8.0, armR=46.0, earL=4)),                                          # poke!
        (9, k(SQ, hr=8.0, armR=30.0, earR=-3)),
        (11, k(SQ, hr=6.0, armR=46.0, earL=-3)),                                        # poke again
        (14, k(SQ, hr=0.0, hp=18.0, armR=22.0, clX=-4)),
        (18, k(SQ, hr=-8.0, earR=4, clY=-4)),                                           # tilt the other way
        (21, k(SQ, hr=-4.0, armL=34.0, earL=-2)),
        (23, k(SQ, hr=-1.0, clY=-1))], 'baked', loop=True, narrow=True)
    clips['squat_up'] = _clip(8, [
        (0, SQ),
        (2, k(SQ, bob=-5 * px, hp=10.0, earL=2, earR=2)),
        (4, dict(bob=1 * px, sz=1.03, hp=2.0, armL=-6.0, armR=-6.0, earL=-3, earR=-3, clX=4)),
        (6, dict(sz=0.99, earL=1, earR=1, clX=-2)),
        (7, R)], 'baked', handoff={'end': 'idle0'}, narrow=True)
    clips.update(_lying(px, k, R))
    return clips


def _tip(pose, f, **kw):
    """the lying pose part-way (f of 0..1): the whole lump tips as one rigid body (tumble + lean + roll + lift scale
    together), each key passes its own floor-contact bob (fitted per family, about pose['bob'] * f**1.5), limbs and head
    keep their own keys from kw."""
    d = {p: pose[p] * f for p in ('tumble', 'lean', 'roll', 'lift') if p in pose}
    d.update(kw); return d


def _lying(px, k, R):
    """lie_belly, lie_back, floor_nap: the whole lump tips (kit-p2 tumble about the body centre) and never bends.
    Poses fitted so the art keeps a clear border in the 132x123 frame over all 8 yaws (render_anim fits them all).
    The legs are the body's own (legL / legR, at least 15 deg once lying so the pads ride them; fL = hL = 0): kicks and
    pedalling are small swings of the legs, in turn, never a split over about 20 deg."""
    clips = {}
    # ------------------------------------------------------------ lie on the belly, chin propped, legs kick back
    BELLY = dict(tumble=65.0, lean=15.0, bob=-0.2, lift=0.03, hp=-20.0, armL=24.0, armR=24.0, legL=-20.0, legR=-20.0)
    clips['belly_down'] = _clip(14, [
        (0, R),
        (2, dict(bob=-2 * px, sz=0.97, armL=20.0, armR=20.0, earL=2, earR=2)),          # a little crouch
        (5, _tip(BELLY, 0.3, bob=-0.01, armL=60.0, armR=60.0, hp=-6.0, legL=-6.0, legR=-6.0, earL=-3, earR=-3, clX=4)),
        (8, _tip(BELLY, 0.7, bob=-0.10, armL=55.0, armR=55.0, hp=-14.0, legL=-16.0, legR=-16.0, earL=4, earR=4)),
        (10, k(BELLY, sz=0.96, hp=-14.0, legL=-30.0, legR=-30.0, earL=-4, earR=-4, clX=-5)),   # flop! legs bounce up
        (12, k(BELLY, hp=-18.0, legL=-16.0, legR=-16.0, earL=2, earR=2, clX=2)),
        (13, BELLY)], 'baked', handoff={'start': 'idle0'})
    clips['belly_kick'] = _clip(24, [
        (0, BELLY),
        (3, k(BELLY, legL=-34.0, legR=-16.0, earL=2, clY=3)),                          # left leg kicks up
        (6, k(BELLY, legL=-16.0, legR=-34.0, hr=4.0, earR=2, clY=-3)),                 # then the right
        (9, k(BELLY, legL=-34.0, legR=-16.0, hr=6.0, earL=-2)),
        (12, k(BELLY, legL=-16.0, legR=-34.0, hr=4.0, hb=1, clX=3)),
        (15, k(BELLY, hp=-16.0, armR=36.0, earL=3, earR=3)),                            # chin up, a little wave
        (18, k(BELLY, legL=-30.0, hr=-4.0, armR=26.0, earR=-2, clY=-3)),
        (21, k(BELLY, legR=-30.0, hr=-2.0, clY=2)),
        (23, k(BELLY, legL=-24.0, legR=-24.0))], 'baked', loop=True)
    clips['belly_up'] = _clip(14, [
        (0, BELLY),
        (2, k(BELLY, armL=10.0, armR=10.0, hp=-12.0, legL=-24.0, legR=-24.0, earL=2, earR=2)),   # mittens push
        (5, _tip(BELLY, 0.7, bob=-0.10, armL=-6.0, armR=-6.0, hp=-8.0, legL=-14.0, legR=-14.0, earL=-3, earR=-3, clX=4)),
        (8, _tip(BELLY, 0.3, bob=-0.01, armL=-10.0, armR=-10.0, hp=-2.0, legL=-5.0, legR=-5.0, earL=3, earR=3)),
        (10, dict(bob=1 * px, sz=1.03, armL=-4.0, armR=-4.0, earL=-2, earR=-2, clX=-4)),  # pop up
        (12, dict(sz=0.98, earL=1, earR=1, clX=2)),
        (13, R)], 'baked', handoff={'end': 'idle0'})
    # ------------------------------------------------------------ lie on the back, legs up, pedalling (face none)
    BACK = dict(tumble=-65.0, lean=-15.0, bob=-0.2, lift=0.03, hp=15.0, armL=30.0, armR=30.0, legL=20.0, legR=20.0)
    clips['back_down'] = _clip(14, [
        (0, R),
        (2, dict(bob=-2 * px, sz=0.97, armL=-8.0, armR=-8.0, earL=2, earR=2)),
        (5, _tip(BACK, 0.3, bob=0.0, armL=10.0, armR=10.0, legL=8.0, legR=8.0, hp=5.0, earL=-3, earR=-3, clX=-4)),
        (8, _tip(BACK, 0.7, bob=-0.10, armL=25.0, armR=25.0, legL=18.0, legR=18.0, hp=10.0, earL=4, earR=4)),
        (10, k(BACK, sz=0.96, legL=30.0, legR=30.0, earL=-4, earR=-4, clX=5)),          # plop, legs bounce up
        (12, k(BACK, legL=16.0, legR=16.0, earL=2, earR=2, clX=-2)),
        (13, BACK)], 'none', handoff={'start': 'idle0'})
    clips['back_lie'] = _clip(30, [
        (0, BACK),
        (4, k(BACK, hb=1, legL=32.0, legR=16.0, earL=-2, earR=-2, clY=3)),              # slow pedal, breath in
        (8, k(BACK, legL=16.0, legR=32.0, armL=40.0, earL=2, clY=-3)),
        (12, k(BACK, legL=32.0, legR=16.0, armR=40.0, hr=5.0, earR=2)),
        (16, k(BACK, hb=1, legL=16.0, legR=32.0, hr=5.0, clX=3)),
        (20, k(BACK, armL=44.0, armR=44.0, hr=-4.0, earL=3, earR=3)),                    # mittens up to the sky
        (24, k(BACK, legL=27.0, legR=27.0, hr=-4.0, clY=-3)),
        (27, k(BACK, hr=-1.0, clY=1)),
        (29, k(BACK, armL=32.0, armR=32.0))], 'none', loop=True)
    clips['back_up'] = _clip(14, [
        (0, BACK),
        (2, k(BACK, legL=32.0, legR=32.0, armL=50.0, armR=50.0, earL=2, earR=2)),        # wind up, rock back
        (5, _tip(BACK, 0.7, bob=-0.10, legL=14.0, legR=14.0, armL=40.0, armR=40.0, hp=8.0, earL=-3, earR=-3, clX=4)),
        (8, _tip(BACK, 0.3, bob=0.0, legL=5.0, legR=5.0, armL=20.0, armR=20.0, hp=3.0, earL=3, earR=3)),   # rock up
        (10, dict(bob=1 * px, sz=1.03, armL=-4.0, armR=-4.0, earL=-2, earR=-2, clX=-4)),
        (12, dict(sz=0.98, earL=1, earR=1, clX=2)),
        (13, R)], 'none', handoff={'end': 'idle0'})
    # ------------------------------------------------------------ curl up for a nap: a loaf, legs tucked, eyes closed
    CURL = dict(tumble=42.0, lean=14.0, bob=-0.1, roll=6.0, lift=0.06, hp=20.0, blink=0.12, armL=30.0, armR=30.0,
                legL=25.0, legR=25.0)
    clips['curl_down'] = _clip(14, [
        (0, R),
        (2, dict(bob=-2 * px, sz=0.97, armL=10.0, armR=10.0, earL=2, earR=2)),
        (4, dict(blink=0.5, hp=6.0, armL=14.0, armR=14.0, earL=-3, earR=-3)),            # a big sleepy blink
        (7, _tip(CURL, 0.4, bob=0.0, blink=0.4, hp=10.0, armL=24.0, armR=24.0, legL=10.0, legR=10.0, earL=3, earR=3)),
        (10, _tip(CURL, 0.8, bob=-0.06, blink=0.2, hp=16.0, armL=28.0, armR=28.0, legL=20.0, legR=20.0, clX=-4)),
        (12, k(CURL, sz=0.97, earL=-3, earR=-3, clX=3)),                                 # settle in
        (13, CURL)], 'baked', handoff={'start': 'idle0'})
    clips['curl_nap'] = _clip(36, [
        (0, CURL),
        (6, k(CURL, hb=1, sz=1.02, earL=-2, earR=-2, clY=2)),                           # slow sleepy breath in
        (12, k(CURL, sz=0.99, earL=1, earR=1, clY=-2)),
        (16, k(CURL, earL=-6, legL=33.0, clX=2)),                                        # an ear and a leg twitch
        (18, k(CURL, earL=2)),
        (24, k(CURL, hb=1, sz=1.02, earR=-2, clY=2)),                                    # breath in
        (30, k(CURL, sz=0.99, hr=-3.0, clY=-2)),
        (35, k(CURL, sz=0.995))], 'baked', loop=True)
    clips['curl_up'] = _clip(16, [
        (0, CURL),
        (3, k(CURL, blink=0.4, earL=3, earR=3, clY=3)),                                  # eyes open slowly
        (5, k(CURL, blink=1.0, hp=10.0, armL=40.0, armR=40.0)),
        (8, _tip(CURL, 0.5, bob=-0.02, hp=4.0, armL=70.0, armR=70.0, legL=12.0, legR=12.0, earL=-3, earR=-3, clX=4)),
        (11, dict(bob=1 * px, sz=1.04, armL=80.0, armR=80.0, earL=4, earR=4, clX=-4)),
        (13, dict(sz=0.98, armL=10.0, armR=10.0, earL=-1, earR=-1)),
        (15, R)], 'baked', handoff={'end': 'idle0'})
    return clips


# nominal constants for readers that do not run Blender (stagekit, verify_p.py read frames / loop / dirs only)
CLIPS = build(2.0 / (0.7071 * 40.07), 1.0 / (0.8660 * 40.07))
