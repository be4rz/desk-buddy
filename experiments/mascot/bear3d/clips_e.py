"""Stage e (gestures-a, aliveness p3): nine intent gestures on the stage kit (stages/e.json, sh run_stage.sh e).

Storyboard #sb-gestures-a (frozen: names, frame counts, dirs, face handling, hand-offs). Stage A's 84x123 frame and
anchor [42,107], 12 fps, every clip one-shot in the five face directions S SE E W SW, starting and ending on the rest
pose (= idle#0; make_stage.py swaps the exact idle sprite in on those frames).

  clip       frames  face    what it shows
  wave       18      layers  the right mitten swings out and up beside the head (higher than fid_wave_small), three
                             big side-to-side waves, a 1 px toe bounce, down the same way
  nod        14      baked   "yes": the round head mass nods (hp +12 / -12) twice, the ears and clover lag
  shake      14      baked   "no": the head mass turns side to side (hy +-25), twice, the second smaller; ears flop
  greet      22      baked   hello: crouch, a little bounce with both mittens up and an open smile, a second small
                             bounce, the paws come down, the mouth closes
  sad        24      baked   the head sinks (hp +16), the body slumps (squash 4 %), ears droop, half-lidded eyes, a
                             little sigh, then a slow recovery
  surprised  16      baked   a startle on the spot: recoil back (lean -), a 2 px hop with the mittens flung out, head
                             back, mouth wide -> open, settle
  jump       18      baked   crouch, a both-feet jump (8 px) with both mittens straight up, land squash, rebound
  dodge      14      baked   a quick side-step: crouch, a sideways hop to the bear's left (root tx in whole V steps, feet in the
                             air while the root moves), land, settle; ends on idle#0 displaced by root_px (the brain
                             moves the panel as for walks)
  remind     20      layers  "don't forget": the left mitten raised to the side of the head, wagging side to side, the
                             ears answering each wag, the other mitten out low; a 1 px toe bounce on each wag

Cute, not realistic: only the existing mitten blobs swing at the shoulder, the whole round head mass tilts or turns
(never a neck), the ears stay inside +-15 deg, squash <= 6 %. 'layers' clips keep hp = hr = 0, mouth closed, eyes open,
sz 1 (squash re-rasterises the fur round the eyes) and the mittens beside or above the face, never across it
(verify_face.py); their raised arm keeps its spread >= 18 deg (Stage C's ask holds armL 120-132 / spL 24-25 safely).

Poses are anim_clips.REST_K + keys (anim_clips.expand_k eases numbers between keys).
"""
import json, os
from anim_clips import REST_K, PK   # noqa: F401  (PK: the Stage K parameter list, the kit's contract)

FPS = 12
F5 = ['S', 'SE', 'E', 'W', 'SW']
FRAME = [84, 123]; ANCHOR = [42, 107]
HANDOFF = {'start': 'idle0', 'end': 'idle0'}
HB = 2.06                    # body height (anim_clips.build): n px of squash -> a scale factor
ORDER = ['wave', 'nod', 'shake', 'greet', 'sad', 'surprised', 'jump', 'dodge', 'remind']
LAYERS = ['wave', 'remind']
FACE = {c: ('layers' if c in LAYERS else 'baked') for c in ORDER}
FRAMES = dict(wave=18, nod=14, shake=14, greet=22, sad=24, surprised=16, jump=18, dodge=14, remind=20)
DODGE_STEPS = 4              # dodge's sideways travel, in V (world units per 2 px along an iso axis): 8 px on SE / SW
MOVES = ['dodge']            # the only clip with root motion (sideways, tx)


def clip(name, keys):
    n = FRAMES[name]
    assert keys[0] == (0, {}) and keys[-1][0] == n - 1, f'{name}: frame 0 must be the rest pose, the last key frame {n - 1}'
    return dict(frames=n, loop=False, dirs=list(F5), face=FACE[name], handoff=dict(HANDOFF),
                frame=list(FRAME), anchor=list(ANCHOR), keys=keys)


def build(V, PXZ):
    """the clip table; V = world units per 2 px along an iso axis (dodge's step), PXZ = world z per screen pixel."""
    def px(n): return n * PXZ
    def sz(n): return 1.0 + n * PXZ / HB                 # the body top moves n px (n < 0 squashes)
    def air(n, **kw): return dict(bob=px(n), hL=px(n), hR=px(n), **kw)    # both stub feet leave the floor
    C = {}

    # ---- wave (layers): out sideways first, then one snap up high beside the head (no frame at mouth height: in
    # profile a half-raised mitten sweeps across the face, verify_face.py); three big waves (the spread swings), a
    # toe bounce on the first two, and the same snap on the way down
    hi = dict(armR=150, spR=52, earR=-3, clY=-3)
    lo = dict(armR=146, spR=26, earR=2, earL=-1, clY=3)
    C['wave'] = clip('wave', [
        (0, {}), (2, {'armR': 50, 'spR': 30, 'clY': -2}), (3, dict(hi, armR=130)),
        (5, hi), (7, dict(lo, **air(1))), (9, hi), (11, dict(lo, **air(1))), (13, hi), (14, dict(lo, spR=30)),
        (15, {'armR': 50, 'spR': 30, 'earR': 1}), (16, {'armR': 24, 'spR': 22}), (17, {})])

    # ---- nod (baked): hp +12 / -12 twice, the ears and clover lag a frame behind the head
    C['nod'] = clip('nod', [
        (0, {}), (2, {'hp': 12, 'earL': 3, 'earR': 3, 'clX': 6}), (4, {'hp': -12, 'earL': -3, 'earR': -3, 'clX': -5}),
        (6, {'hp': 12, 'earL': 4, 'earR': 4, 'clX': 7}), (8, {'hp': -12, 'earL': -2, 'earR': -2, 'clX': -4}),
        (10, {'hp': 4, 'earL': 1, 'earR': 1, 'clX': 3}), (12, {'clX': -1}), (13, {})])

    # ---- shake (baked): the head mass turns +-25, twice (the second smaller), ears flop outward with each turn
    C['shake'] = clip('shake', [
        (0, {}), (2, {'hy': 25, 'earL': 5, 'earR': -1, 'clY': -6, 'hp': 3}),
        (4, {'hy': -25, 'earL': -1, 'earR': 5, 'clY': 6, 'hp': 3}),
        (6, {'hy': 25, 'earL': 5, 'earR': -1, 'clY': -6, 'hp': 3}),
        (8, {'hy': -22, 'earL': -1, 'earR': 5, 'clY': 5, 'hp': 2}),
        (10, {'hy': 10, 'earL': 2, 'earR': 1, 'clY': -3, 'hp': 1}), (12, {'clY': 1}), (13, {})])

    # ---- greet (baked, smile open): crouch, bounce with both mittens up (spread wide, no paw over the face), smile,
    # wiggle the paws, a second small bounce, paws down, the mouth closes on the way to rest
    up = dict(armL=150, armR=150, spL=30, spR=30)
    C['greet'] = clip('greet', [
        (0, {}), (2, {'sz': sz(-3), 'armL': 30, 'armR': 30, 'spL': 18, 'spR': 18, 'earL': 3, 'earR': 3, 'clX': 4,
                      'mouth': 'open'}),
        (4, dict(air(3), sz=sz(2), earL=-5, earR=-5, clX=-8, **up)),
        (6, dict(air(2), earL=-3, earR=-3, clX=-3, **up)),
        (8, dict(sz=sz(-2), earL=5, earR=5, clX=7, **up)),
        (10, dict(up, armL=158, armR=142, spL=38, spR=24, earL=-2, earR=2, clY=4)),
        (12, dict(up, armL=142, armR=158, spL=24, spR=38, earL=2, earR=-2, clY=-4)),
        (13, dict(sz=sz(-2), earL=3, earR=3, clX=4, **up)),
        (15, dict(air(2), earL=-4, earR=-4, clX=-6, **up)),
        (17, dict(sz=sz(-2), armL=90, armR=90, spL=22, spR=22, earL=4, earR=4, clX=5)),
        (19, {'armL': 25, 'armR': 25, 'spL': 8, 'spR': 8, 'clX': -2, 'mouth': 'closed'}), (21, {})])

    # ---- sad (baked): the head sinks, the body slumps, ears droop outward, eyes half-lidded, arms hang in; a small
    # sigh (the body rises 1 px and sinks again), then a slow recovery
    slump = dict(hp=16, sz=sz(-4), armL=8, armR=8, spL=-6, spR=-6, earL=12, earR=12, clX=10, blink=0.55)
    C['sad'] = clip('sad', [
        (0, {}), (5, slump), (9, dict(slump, clY=3)),
        (12, dict(slump, sz=sz(-2), hp=13, clX=7, clY=-2)), (15, dict(slump, clY=2, blink=0.5)),
        (18, dict(slump, hp=14, clY=-1)), (21, {'hp': 5, 'sz': sz(-1), 'earL': 4, 'earR': 4, 'clX': 3, 'blink': 0.8}),
        (23, {})])

    # ---- surprised (baked): a startle on the spot: recoil (lean back), a 2 px hop with the mittens flung out and up,
    # the head goes back, the mouth wide; land, mouth open, settle and close
    fling = dict(armL=70, armR=70, spL=45, spR=45)
    C['surprised'] = clip('surprised', [
        (0, {}), (1, {'sz': sz(-2), 'lean': -4, 'earL': 3, 'earR': 3, 'clX': 5}),
        (3, dict(air(2), sz=sz(1), lean=-8, hp=-10, earL=-8, earR=-8, clX=-10, mouth='wide', **fling)),
        (5, dict(air(2), lean=-7, hp=-10, earL=-6, earR=-6, clX=-6, **fling)),
        (7, dict(sz=sz(-2), lean=-5, hp=-6, earL=4, earR=4, clX=6, mouth='open', armL=55, armR=55, spL=38, spR=38)),
        (10, dict(lean=-3, hp=-4, earL=-2, earR=-2, clX=-2, armL=40, armR=40, spL=28, spR=28)),
        (13, {'lean': -1, 'armL': 12, 'armR': 12, 'spL': 8, 'spR': 8, 'clX': 2, 'mouth': 'closed'}), (15, {})])

    # ---- jump (baked): crouch, both feet leave the floor together (8 px), mittens straight up, a happy open mouth at
    # the top, land with a squash, rebound
    arms_up = dict(armL=165, armR=165, spL=14, spR=14)
    C['jump'] = clip('jump', [
        (0, {}), (2, {'sz': sz(-2), 'armL': -10, 'armR': -10, 'earL': 2, 'earR': 2}),
        (3, {'sz': sz(-4), 'armL': -18, 'armR': -18, 'earL': 4, 'earR': 4, 'clX': 6}),
        (5, dict(air(6), sz=sz(3), armL=120, armR=120, spL=16, spR=16, earL=-6, earR=-6, clX=-12, mouth='open')),
        (7, dict(air(8), sz=sz(1), earL=-5, earR=-5, clX=-6, **arms_up)),
        (9, dict(air(7), earL=-2, earR=-2, clX=2, **arms_up)),
        (11, dict(air(2), earL=3, earR=3, clX=7, **arms_up)),
        (12, dict(sz=sz(-4), armL=120, armR=120, spL=16, spR=16, earL=8, earR=8, clX=11)),
        (14, dict(sz=sz(2), armL=40, armR=40, spL=10, spR=10, earL=-3, earR=-3, clX=-5, mouth='closed')),
        (16, {'sz': sz(-1), 'earL': 1, 'earR': 1, 'clX': 2}), (17, {})])

    # ---- dodge (baked): crouch leaning away, hop sideways to the bear's left (+tx; the root moves only while both
    # feet are off the floor, so nothing slides), roll into the landing, settle; the last frame is rest, displaced
    D = DODGE_STEPS * V if V else 0.0
    C['dodge'] = clip('dodge', [
        (0, {}), (2, {'sz': sz(-3), 'roll': -6, 'armL': 10, 'armR': -10, 'spL': 10, 'spR': 25, 'earL': 2, 'earR': 4,
                      'clY': -4}),
        (3, dict(air(3), roll=6, armL=-10, armR=20, spL=35, spR=20, earL=-4, earR=-2, clY=6, hp=-4, tx=0.25 * D)),
        (5, dict(air(4), roll=8, armL=-6, armR=16, spL=38, spR=24, earL=-5, earR=-3, clY=8, hp=-4, tx=0.75 * D)),
        (6, dict(air(2), roll=5, armL=0, armR=10, spL=30, spR=18, earL=-1, earR=0, clY=4, tx=D)),
        (7, dict(sz=sz(-3), roll=3, armL=8, armR=6, spL=20, spR=10, earL=6, earR=4, clY=-3, tx=D)),
        (9, dict(sz=sz(1), roll=-2, earL=-2, earR=-1, clY=2, tx=D)),
        (11, dict(sz=sz(-1), roll=1, clY=-1, tx=D)), (13, dict(tx=D))])

    # ---- remind (layers): the left mitten snaps up beside the head (spread kept wide, no frame at mouth height: in
    # profile a half-raised mitten crosses the face),
    # wagging side to side four times like a raised finger; the ears answer each wag, a 1 px toe bounce on the
    # outward wags; the right mitten out low, then both down
    up_l = dict(armL=140, spL=30, armR=-6, spR=26, earL=-2)
    out_l = dict(up_l, spL=44, earL=2, earR=-2, clY=4)
    in_l = dict(up_l, spL=22, earL=-3, earR=2, clY=-4)
    C['remind'] = clip('remind', [
        (0, {}), (2, {'armL': 40, 'spL': 34, 'armR': -4, 'spR': 14}), (3, dict(up_l, armL=120, spL=36)),
        (5, up_l), (7, dict(out_l, **air(1))), (9, in_l), (11, dict(out_l, **air(1))), (13, in_l),
        (14, dict(up_l, spL=30)), (15, {'armL': 45, 'spL': 34, 'armR': -3, 'spR': 14}),
        (17, {'armL': 20, 'spL': 14}), (19, {})])
    return C


_J = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'clips.json')))
CLIPS = build(_J['V'], _J['PXZ'])   # the table verify_e.py / stagekit read (rig_bear.py calls build with its own V, PXZ)
DODGE_TX = DODGE_STEPS * _J['V']    # dodge's final root tx (world units), checked by verify_e.py
