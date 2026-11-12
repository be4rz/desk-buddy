"""Stage g (fidgets-b, stage kit): eight short idle fidgets with a baked face (pure Python, no bpy).

    sh run_stage.sh g        -> out/bear3d/anim_g/   (stages/g.json; verify_g.py)

Rig K (rig_bear.py --stage k): Stage L's pose + hp (head pitch, + = looks down, |hp| <= 20), hr (head roll, + = the
top of the head towards the bear's left, |hr| <= 15), mouth (closed/open/wide/tongue), blink (eye squint, 1 = open,
0.12 = closed). roll and lean stay 0 (tilts are head-only), squash/stretch |sz - 1| <= 6 %, so the bear stays a round
mochi toy: the head mass tilts, the mitten arms swing at the shoulder, the stub feet stay planted.

Every clip: one-shot, in place (no root motion), directions S SE E W SW, Stage A's 84x123 frame (anchor [42, 107]),
face "baked" (the head tilts, the eyes close or the mouth opens, so no face layers and no face part), hand-off
idle#0 at both ends (frame 0 and the last frame are the rest pose; make_stage.py swaps in the idle sprite).

  fid_rock_heels 20  up on the toes and back, twice (bob + a small stretch, arms swing back / forward)
  fid_head_tilt  18  a curious head tilt (hr 12), ears counter-tilt, the clover lags
  fid_scratch    20  the right mitten scratches the side of the head, three strokes, eyes squint happily
  stretch        24  both arms up, a 6 % stretch, eyes shut; settle (also the A5 stretch intent)
  fid_yawn       24  head back, mouth wide, eyes shut, the left mitten up at the mouth (in front, so it shows)
  fid_shake_off  18  a whole-body wobble (the bear twists left / right, the head counter-twists, ears flap)
  fid_sneeze     18  wind-up (head back, mouth open, squint), a jolt forward with the eyes shut, recover
  fid_nod_off    30  the eyes close, the head dips lower and lower, a jolt awake (1 px hop), blink
"""
from anim_clips import REST_K, PK  # noqa: F401  (the Stage K pose dict; never edited here)

FPS = 12
DIRS = ['S', 'SE', 'E', 'W', 'SW']
FRAME = (84, 123); ANCHOR = (42, 107)
ORDER = ['fid_rock_heels', 'fid_head_tilt', 'fid_scratch', 'stretch', 'fid_yawn', 'fid_shake_off', 'fid_sneeze',
         'fid_nod_off']
HANDOFF = {'start': 'idle0', 'end': 'idle0'}
PXZ_NOMINAL = 0.0288       # world z per screen pixel at ppu 40.07; only for the module-level CLIPS metadata


def build(V, PXZ):
    """the clips, with bob in whole screen pixels of this render (rig_bear.py passes PXZ)."""
    def px(n): return n * PXZ
    C = {}

    def clip(name, frames, keys):
        assert keys[0] == (0, {}) and keys[-1] == (frames - 1, {}), f'{name}: frame 0 and last must be the rest pose'
        C[name] = dict(frames=frames, loop=False, face='baked', dirs=list(DIRS), handoff=dict(HANDOFF), keys=keys)

    # rock_heels: up on the toes (body lifts 2 px, a little stretch, arms swing back, head up), back down on the
    # heels (a little squash, arms forward, head nods), twice, the second smaller
    up = dict(bob=px(2), sz=1.03, armL=-18, armR=-18, hp=-5, earL=-3, earR=-3, clX=-4)
    back = dict(sz=0.97, armL=14, armR=14, hp=5, earL=4, earR=4, clX=5)
    clip('fid_rock_heels', 20, [
        (0, {}), (3, up), (5, dict(up, clX=-1)), (8, back), (10, dict(back, clX=2)),
        (13, dict(up, bob=px(2), sz=1.025, armL=-12, armR=-12, hp=-4)), (15, dict(up, armL=-12, armR=-12, clX=0)),
        (17, dict(sz=0.985, armL=6, armR=6, hp=2, earL=2, earR=2, clX=3)), (19, {})])

    # head_tilt: a curious tilt to one side (hr 12), the ears counter-tilt, the clover lags; a small overshoot back
    tilt = dict(hr=12, hp=-3, earL=6, earR=-4, clY=7)
    clip('fid_head_tilt', 18, [
        (0, {}), (4, tilt), (8, dict(tilt, earL=4, earR=-2, clY=4)), (11, dict(tilt, earL=5, earR=-3, clY=5)),
        (14, dict(hr=-2, earL=-1, earR=1, clY=-3)), (17, {})])

    # scratch: the right mitten up beside the head, the head leans into it, three strokes, a happy squint
    reach = dict(armR=150, spR=26, hr=-9, earR=4, earL=-1, clY=-4)
    lo = dict(reach, armR=138, blink=0.45)
    hi = dict(reach, armR=154, blink=0.45, earR=6)
    clip('fid_scratch', 20, [
        (0, {}), (3, dict(armR=110, spR=18, hr=-5, earR=2)), (5, reach), (7, lo), (9, hi), (11, lo), (13, hi),
        (15, dict(reach, blink=0.7)), (17, dict(armR=70, spR=10, hr=-2)), (19, {})])

    # stretch: a squash wind-up, both arms up high with a 6 % stretch, eyes shut, mouth open; hold with a tremble;
    # the arms come down with a little settle
    reach = dict(armL=160, armR=160, spL=14, spR=14, sz=1.06, bob=px(1), hp=-10, blink=0.12, mouth='open',
                 earL=-6, earR=-6, clX=-6)
    clip('stretch', 24, [
        (0, {}), (3, dict(sz=0.97, armL=20, armR=20, earL=3, earR=3, clX=3)), (8, reach),
        (11, dict(reach, spL=18, spR=18, clX=-3)), (14, dict(reach, spL=12, spR=12, clX=-5)),
        (16, dict(reach, sz=1.05, blink=0.45, mouth='closed')),
        (19, dict(armL=40, armR=40, sz=0.97, hp=3, earL=4, earR=4, clX=5)),
        (21, dict(sz=1.01, earL=-1, earR=-1, clX=-2)), (23, {})])

    # yawn: head back, mouth opens wide, eyes shut, the left mitten comes up to the mouth; it closes, a sleepy blink
    gape = dict(hp=-10, mouth='wide', blink=0.12, armL=88, spL=8, sz=1.04, earL=-6, earR=-6, clX=-6)
    clip('fid_yawn', 24, [
        (0, {}), (4, dict(hp=-7, mouth='open', blink=0.45, sz=1.02, earL=-3, earR=-3, clX=-3, armL=40, spL=4)),
        (8, gape), (12, dict(gape, armL=92, clX=-4)), (15, dict(gape, hp=-9, armL=86)),
        (17, dict(hp=-3, mouth='closed', blink=0.45, armL=40, spL=4, sz=0.98, earL=4, earR=4, clX=4)),
        (20, dict(blink=1.0, earL=1, earR=1)), (21, dict(blink=0.45)), (23, {})])

    # shake_off: a short squash, then the whole bear twists left / right (ty) with the head counter-twisting (hy),
    # ears and arms flapping, the swing dying away
    def sw(a, s):   # a: twist amplitude (deg), s: +1 / -1 side
        return dict(ty=a * s, hy=-a * 1.2 * s, earL=14 * s, earR=-14 * s, armL=30 if s > 0 else -12,
                    armR=-12 if s > 0 else 30, spL=25, spR=25, clY=10 * s, bob=px(1) if s > 0 else 0.0)
    clip('fid_shake_off', 18, [
        (0, {}), (2, dict(sz=0.97, earL=3, earR=3, clX=3)), (4, sw(20, 1)), (6, sw(20, -1)), (8, sw(16, 1)),
        (10, sw(11, -1)), (12, dict(sw(6, 1), earL=7, earR=-7)), (14, dict(sw(2, -1), earL=-3, earR=3, spL=6, spR=6)),
        (16, dict(sz=1.01, earL=2, earR=2, clX=-2)), (17, {})])

    # sneeze: ah- (head back, mouth open, eyes squint shut, a stretch), -choo (one-frame jolt forward, eyes shut,
    # mouth wide, squash, ears and paws flung forward), recover with a blink
    ah = dict(hp=-14, blink=0.3, mouth='open', sz=1.04, earL=-5, earR=-5, clX=-6, armL=10, armR=10)
    choo = dict(hp=16, blink=0.12, mouth='wide', sz=0.95, earL=10, earR=10, clX=10, armL=30, armR=30, spL=10, spR=10)
    clip('fid_sneeze', 18, [
        (0, {}), (2, dict(hp=-6, blink=0.6, mouth='open', earL=-2, earR=-2, clX=-2)), (5, ah),
        (7, dict(ah, hp=-16, blink=0.12, sz=1.05)), (8, choo),
        (10, dict(choo, hp=12, mouth='closed', sz=0.97, clX=6, armL=20, armR=20)),
        (13, dict(hp=2, blink=0.45, earL=2, earR=2, clX=-2, armL=4, armR=4)), (15, dict(earL=-1, earR=-1, clX=1)),
        (17, {})])

    # nod_off: the eyes grow heavy, the head dips, it fights back once, then sinks fully (eyes shut, ears droop);
    # a jolt awake (one frame: head up, eyes wide, 2 px hop, paws out), settle, a blink
    dip = dict(blink=0.12, hp=19, earL=9, earR=9, sz=0.97, clX=8, armL=8, armR=8)
    clip('fid_nod_off', 30, [
        (0, {}), (4, dict(blink=0.6, earL=2, earR=2)), (8, dict(blink=0.3, hp=6, earL=4, earR=4, armL=4, armR=4)),
        (10, dict(blink=0.6, hp=3, earL=3, earR=3)), (14, dict(blink=0.12, hp=12, earL=7, earR=7, sz=0.98, clX=5)),
        (19, dip), (21, dict(dip, clX=9)),
        (22, dict(hp=-6, blink=1.0, bob=px(2), sz=1.04, earL=-8, earR=-8, clX=-8, armL=-15, armR=-15, spL=15, spR=15)),
        (24, dict(hp=-4, bob=px(1), sz=1.02, earL=-4, earR=-4, clX=-3, armL=-5, armR=-5, spL=5, spR=5)),
        (26, dict(blink=0.45, earL=2, earR=2, clX=2)), (27, dict(blink=1.0)), (29, {})])
    return C


CLIPS = build(None, PXZ_NOMINAL)    # metadata (frames, loop, dirs, face, handoff) for the verifier
