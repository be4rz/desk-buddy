"""Stage w (edge-art, aliveness p4): the bear sits on a window's top edge (stages/w.json, sh run_stage.sh w).

Pure Python: rig_bear.py --stage k keys build(V, PXZ); stagekit / verify_w.py read ORDER, CLIPS, CHAINS, SEATED and
BASE. Storyboard #sb-edge rows 66-70 are the contract (names, frame counts, dirs, loops, face, hand-offs, frame):

  clip         frames  loop  face    hand-offs                              what it shows
  edge_sit     10      no    baked   #0 = idle#0, last = edge_dangle#0     a little crouch, a hop up onto the seat
  edge_dangle  24      yes   layers  #0 = base pose                         the legs swing in turn, a breath, ears
  edge_peer    24      no    baked   base pose both ends                    the head tips down over the edge, tilts each way
  edge_doze    36      yes   baked   #0 = base pose                         eyes close, the head dips, a nod, blinks
  edge_stand   10      no    baked   #0 = base pose, last = idle#0          mittens push, a hop off the seat, settle

Frame 84x139, anchor [42, 107] = the window edge line (Stage A's anchor row, 16 px more canvas below it). The base pose
(BASE = edge_dangle#0) is the seated plush on the edge: both hip joints swing the leg stubs forward-down over the edge
(SEAT_LEG, legL / legR; the pads ride their legs, fL = hL = 0) and the lump sits SEAT_PX whole pixels lower, so the body
above the hips (rig leg weight < 0.5: rig_bear.py LEGS) rests on the edge line in every direction (the E / W hip flank
is the lowest of it) and the legs with their pads hang over it. The dangle is a small alternate swing of the legs
(+-12 deg about SEAT_LEG, never a split over 30 deg). On every SEATED frame only the legs and their pads may be below the
line (verify_w.py projects the posed leg stubs and pads and checks it).

The kit's '<clip>#<frame>' hand-off names Stage A clips only, so the base-pose joins are CHAINS (stagekit checks them
pixel for pixel): edge_sit[last] = edge_peer[0] = edge_peer[last] = edge_doze[0] = edge_stand[0] = edge_dangle[0].

Cute rules (design "Common to every art ticket"): one round mochi lump that never bends; the stub legs swing at the hip
(pads on them, never apart) but never stretch; mitten arms swing at the shoulder; squash <= 6 %; the head tilts only through hp / hr (<= 20 / 15 deg);
edge_dangle (layers) keeps hp = hr = 0, sz 1, eyes open, mouth closed and the mittens low, away from the face.
"""
from anim_clips import REST_K, PK   # noqa: F401  (PK: the Stage K parameter list, the kit's contract)

F5 = ['S', 'SE', 'E', 'W', 'SW']
FRAME, ANCHOR = [84, 139], [42, 107]
ORDER = ['edge_sit', 'edge_dangle', 'edge_peer', 'edge_doze', 'edge_stand']
LAYERS = ['edge_dangle']
BASE_CLIP = 'edge_dangle'
CHAINS = [('edge_sit', -1, 'edge_dangle', 0), ('edge_peer', 0, 'edge_dangle', 0), ('edge_peer', -1, 'edge_dangle', 0),
          ('edge_doze', 0, 'edge_dangle', 0), ('edge_stand', 0, 'edge_dangle', 0)]
# frames on the seat (clip -> frame indices; None = every frame): only the legs and their pads may hang below the edge line
SEATED = {'edge_sit': [-1], 'edge_dangle': None, 'edge_peer': None, 'edge_doze': None, 'edge_stand': [0]}
SEAT_LEG = 45.0              # deg, both hips in the base pose: the stubs point forward-down over the edge (35..57 swinging)
SEAT_PX = -1                 # whole pixels the lump is moved (down) in the base pose: with the legs at 35..57 deg the body
                             # above the hips (the E / W hip flank) stays on or above the edge line (probed in Blender)
HEAD_PX = -1                 # the head sits 1 whole px lower on the body in the base pose (a breath is hb 0)


def base(PXZ):
    """the seated base pose (edge_dangle#0) as Stage K keys over REST_K."""
    return dict(bob=SEAT_PX * PXZ, hb=HEAD_PX, legL=SEAT_LEG, legR=SEAT_LEG)


def _clip(n, keys, face, loop=False, handoff=None):
    return dict(frames=n, loop=loop, face=face, dirs=list(F5), handoff=handoff or None,
                frame=list(FRAME), anchor=list(ANCHOR), keys=keys)


def build(V, PXZ):
    """the 5 clips as Stage K keys (anim_clips.expand_k). PXZ = world z of one screen pixel.
    The pads never leave their legs: a swung leg (|leg| >= 15) carries its pad (fL = hL = 0); a key with the legs at 0
    (standing, landing) has the pads planted. A swung -> rest change of the legs happens between two adjacent keys
    (no eased in-between frame where a half-planted pad could lag behind the hopping body)."""
    px = PXZ
    B = base(PXZ)
    S = SEAT_LEG

    def k(**kw):
        d = dict(B); d.update(kw); return d

    R = {}                                                   # rest = REST_K (idle#0)
    clips = {}
    # ------------------------------------------------------------ sit down onto the edge (idle#0 -> base)
    clips['edge_sit'] = _clip(10, [
        (0, R),
        (2, dict(bob=-2 * px, sz=0.96, armL=-10.0, armR=-10.0, earL=2, earR=2)),          # a little crouch
        (3, dict(bob=4 * px, sz=1.02, legL=16.0, legR=16.0, armL=-18.0, armR=-18.0,
                 earL=-2, earR=-2, clX=3)),                                                # spring up, legs lift
        (4, dict(bob=7 * px, legL=30.0, legR=30.0, armL=-22.0, armR=-22.0,
                 earL=-4, earR=-4, clX=5)),                                                # a little hop, legs forward
        (6, k(bob=-2 * px, sz=0.95, legL=S + 12, legR=S + 12, armL=-6.0, armR=-6.0,
              earL=4, earR=4, clX=-5)),                                                    # plop on the seat, legs kick out
        (8, k(legL=S - 6, legR=S - 6, earL=-2, earR=-2, clX=2)),                         # legs swing back over the edge
        (9, B)], 'baked', handoff={'start': 'idle0'})
    # ------------------------------------------------------------ dangle (loop, face layers): the legs swing in turn
    clips['edge_dangle'] = _clip(24, [
        (0, B),
        (3, k(legL=S + 12, legR=S - 8, clY=3)),                                          # left leg forward
        (6, k(legL=S - 8, legR=S + 12, clY=-3)),                                         # then the right
        (9, k(legL=S + 12, legR=S - 8, earL=-3, earR=2)),
        (12, k(hb=0, earL=2, earR=-3, clX=3)),                                           # a slow breath in
        (15, k(hb=0, legL=S + 10, legR=S + 10, armL=8.0, armR=8.0, clX=-2)),             # both legs up, a happy kick
        (18, k(legL=S - 6, legR=S - 6, earL=-2, earR=-2)),
        (21, k(legR=S + 10, earL=1, earR=1, clY=2)),
        (23, k(legR=S + 6, clY=1))], 'layers', loop=True)
    # ------------------------------------------------------------ peer: look down over the edge (no lean: the all-yaw canvas check leaves ~2 px of head room)
    clips['edge_peer'] = _clip(24, [
        (0, B),
        (3, k(hp=6.0, sz=0.99, armL=14.0, armR=14.0, earL=-3, earR=-3, clX=4)),
        (6, k(hp=12.0, sz=0.97, armL=24.0, armR=24.0, legL=S - 6, legR=S - 6,
              earL=-6, earR=-6, clX=8)),                                                   # peers down, ears forward
        (9, k(hp=13.0, hr=10.0, sz=0.96, armL=24.0, armR=24.0, legL=S - 6, legR=S - 6,
              earL=-4, earR=-8, clX=6, clY=5)),                                            # curious tilt
        (13, k(hp=13.0, hr=10.0, sz=0.96, armL=24.0, armR=24.0, legL=S - 6, legR=S - 6,
               earL=-6, earR=-6, clX=6, clY=5)),
        (16, k(hp=13.0, hr=-10.0, sz=0.96, armL=24.0, armR=24.0, legL=S - 6, legR=S - 6,
               earL=-8, earR=-4, clX=6, clY=-5)),                                          # and the other way
        (19, k(hp=5.0, hr=-2.0, sz=0.99, armL=10.0, armR=10.0, earL=3, earR=3, clX=-4)),   # sits back up
        (21, k(earL=-2, earR=-2, clX=2)),
        (23, B)], 'baked')
    # ------------------------------------------------------------ doze (loop): eyes close, the head dips, a nod
    clips['edge_doze'] = _clip(36, [
        (0, B),
        (3, k(blink=0.5, hp=3.0, earL=2, earR=2)),                                        # heavy eyelids
        (6, k(blink=0.12, hp=6.0, earL=5, earR=5, armL=4.0, armR=4.0)),                   # eyes closed, ears droop
        (12, k(blink=0.12, hp=7.0, sz=0.99, legL=S - 4, legR=S - 4, earL=6, earR=6, clX=-3)),  # slow breath in, legs go slack
        (18, k(blink=0.12, hp=10.0, sz=0.97, legL=S - 6, legR=S - 6, earL=7, earR=7, clX=2)),  # breath out, head dips
        (24, k(blink=0.12, hp=10.0, sz=0.98, hr=2.0, legL=S - 6, legR=S - 6, earL=8, earR=7, clX=-3)),
        (28, k(blink=0.12, hp=11.0, sz=0.96, hr=2.0, legL=S - 6, legR=S - 6, earL=8, earR=8, clX=3)),  # nodding off...
        (30, k(blink=0.4, hp=5.0, hr=0.0, legL=S + 6, legR=S + 6, earL=-4, earR=-4, clX=6)),  # ...a little nod-jerk, legs twitch
        (32, k(blink=1.0, hp=2.0, earL=1, earR=1, clX=-2)),                               # eyes flicker open
        (35, k(hp=0.5, clX=0.5))], 'baked', loop=True)
    # ------------------------------------------------------------ stand back up (base -> idle#0)
    clips['edge_stand'] = _clip(10, [
        (0, B),
        (2, k(bob=1 * px, sz=0.96, armL=-20.0, armR=-20.0, legL=S + 10, legR=S + 10,
              earL=2, earR=2)),                                                            # mittens push, legs swing up
        (4, dict(bob=7 * px, legL=24.0, legR=24.0, armL=-14.0, armR=-14.0,
                 earL=-4, earR=-4, clX=5)),                                                # hop off the seat
        (5, dict(bob=3 * px, legL=15.0, legR=15.0, armL=-10.0, armR=-10.0, earL=0, earR=0, clX=0)),  # legs come down
        (6, dict(bob=-2 * px, sz=0.95, armL=-6.0, armR=-6.0, earL=4, earR=4, clX=-5)),   # land on the feet, a soft squash
        (8, dict(sz=1.01, earL=-1, earR=-1, clX=2)),
        (9, R)], 'baked', handoff={'end': 'idle0'})
    return clips


# nominal constants for readers that do not run Blender (stagekit, verify_w.py read frames / loop / dirs only)
CLIPS = build(2.0 / (0.7071 * 40.07), 1.0 / (0.8660 * 40.07))
