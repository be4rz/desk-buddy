"""Stage w (edge-art, aliveness p4): the bear sits on a window's top edge (stages/w.json, sh run_stage.sh w).

Pure Python: rig_bear.py --stage k keys build(V, PXZ); stagekit / verify_w.py read ORDER, CLIPS, CHAINS, SEATED and
BASE. Storyboard #sb-edge rows 66-70 are the contract (names, frame counts, dirs, loops, face, hand-offs, frame):

  clip         frames  loop  face    hand-offs                              what it shows
  edge_sit     10      no    baked   #0 = idle#0, last = edge_dangle#0     a little crouch, a hop up onto the seat
  edge_dangle  24      yes   layers  #0 = base pose                         stub feet swing in turn, a breath, ears
  edge_peer    24      no    baked   base pose both ends                    the head tips down over the edge, tilts each way
  edge_doze    36      yes   baked   #0 = base pose                         eyes close, the head dips, a nod, blinks
  edge_stand   10      no    baked   #0 = base pose, last = idle#0          mittens push, a hop off the seat, settle

Frame 84x139, anchor [42, 107] = the window edge line (Stage A's anchor row, 16 px more canvas below it). The base pose
(BASE = edge_dangle#0) is the seated plush: the whole body is raised 9 whole pixels (bob; the head 1 px back down, hb -1) so its lowest pixel sits on
the edge line, and the stub feet come forward and stay against the body's front-bottom (fL / hL), hanging over the
edge. The model has no legs: the stub feet are the "legs", and they never leave the body (a first probe that dropped
them further floated them free of the body; not cute). On every SEATED frame only the feet may be below the line
(verify_w.py projects the foot boxes and checks it).

The kit's '<clip>#<frame>' hand-off names Stage A clips only, so the base-pose joins are CHAINS (stagekit checks them
pixel for pixel): edge_sit[last] = edge_peer[0] = edge_peer[last] = edge_doze[0] = edge_stand[0] = edge_dangle[0].

Cute rules (design "Common to every art ticket"): one round mochi lump that never bends; stub feet swing but never
stretch; mitten arms swing at the shoulder; squash <= 6 %; the head tilts only through hp / hr (<= 20 / 15 deg);
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
# frames on the seat (clip -> frame indices; None = every frame): only the stub feet may hang below the edge line
SEATED = {'edge_sit': [-1], 'edge_dangle': None, 'edge_peer': None, 'edge_doze': None, 'edge_stand': [0]}
SEAT_PX = 9                  # whole pixels the body is raised in the base pose (its lowest pixel on the edge line)
HEAD_PX = -1                 # the head sits 1 whole px lower on the body (face raised 8 px): at a 9 px face raise the
                             # W eye patch's pixel (3,14) renders as fur shade, not light (verify_face exact-frame
                             # mismatch, probed: face raises 8 and 10 are exact); a breath is hb 0


def base(PXZ):
    """the seated base pose (edge_dangle#0) as Stage K keys over REST_K."""
    return dict(bob=SEAT_PX * PXZ, hb=HEAD_PX, fL=0.28, fR=0.28, hL=0.08, hR=0.08)


def _clip(n, keys, face, loop=False, handoff=None):
    return dict(frames=n, loop=loop, face=face, dirs=list(F5), handoff=handoff or None,
                frame=list(FRAME), anchor=list(ANCHOR), keys=keys)


def build(V, PXZ):
    """the 5 clips as Stage K keys (anim_clips.expand_k). PXZ = world z of one screen pixel."""
    px = PXZ
    B = base(PXZ)

    def k(**kw):
        d = dict(B); d.update(kw); return d

    R = {}                                                   # rest = REST_K (idle#0)
    clips = {}
    # ------------------------------------------------------------ sit down onto the edge (idle#0 -> base)
    clips['edge_sit'] = _clip(10, [
        (0, R),
        (2, dict(bob=-2 * px, sz=0.96, armL=-10.0, armR=-10.0, earL=2, earR=2)),          # a little crouch
        (4, dict(bob=10 * px, fL=0.12, fR=0.12, hL=0.22, hR=0.22, armL=-22.0, armR=-22.0,
                 earL=-4, earR=-4, clX=5)),                                                # hop up, feet tucked
        (6, k(bob=8 * px, sz=0.95, fL=0.34, fR=0.34, hL=0.12, hR=0.12, armL=-6.0, armR=-6.0,
              earL=4, earR=4, clX=-5)),                                                    # plop on the seat
        (8, k(fL=0.22, fR=0.22, hL=0.05, hR=0.05, earL=-2, earR=-2, clX=2)),             # feet swing back
        (9, B)], 'baked', handoff={'start': 'idle0'})
    # ------------------------------------------------------------ dangle (loop, face layers): the feet swing in turn
    clips['edge_dangle'] = _clip(24, [
        (0, B),
        (3, k(fL=0.38, hL=0.15, fR=0.20, hR=0.04, clY=3)),                               # left foot forward
        (6, k(fL=0.20, hL=0.04, fR=0.38, hR=0.15, clY=-3)),                              # then the right
        (9, k(fL=0.38, hL=0.15, fR=0.20, hR=0.04, earL=-3, earR=2)),
        (12, k(hb=0, earL=2, earR=-3, clX=3)),                                           # a slow breath in
        (15, k(hb=0, fL=0.34, hL=0.13, fR=0.34, hR=0.13, armL=8.0, armR=8.0, clX=-2)),   # both feet up, a happy kick
        (18, k(fL=0.22, hL=0.05, fR=0.22, hR=0.05, earL=-2, earR=-2)),
        (21, k(fR=0.34, hR=0.12, earL=1, earR=1, clY=2)),
        (23, k(fR=0.30, hR=0.10, clY=1))], 'layers', loop=True)
    # ------------------------------------------------------------ peer: look down over the edge (no lean: the all-yaw canvas check leaves ~2 px of head room)
    clips['edge_peer'] = _clip(24, [
        (0, B),
        (3, k(hp=6.0, sz=0.99, armL=14.0, armR=14.0, earL=-3, earR=-3, clX=4)),
        (6, k(hp=12.0, sz=0.97, armL=24.0, armR=24.0, fL=0.24, fR=0.24, hL=0.06, hR=0.06,
              earL=-6, earR=-6, clX=8)),                                                   # peers down, ears forward
        (9, k(hp=13.0, hr=10.0, sz=0.96, armL=24.0, armR=24.0, fL=0.24, fR=0.24, hL=0.06, hR=0.06,
              earL=-4, earR=-8, clX=6, clY=5)),                                            # curious tilt
        (13, k(hp=13.0, hr=10.0, sz=0.96, armL=24.0, armR=24.0, fL=0.24, fR=0.24, hL=0.06, hR=0.06,
               earL=-6, earR=-6, clX=6, clY=5)),
        (16, k(hp=13.0, hr=-10.0, sz=0.96, armL=24.0, armR=24.0, fL=0.24, fR=0.24, hL=0.06, hR=0.06,
               earL=-8, earR=-4, clX=6, clY=-5)),                                          # and the other way
        (19, k(hp=5.0, hr=-2.0, sz=0.99, armL=10.0, armR=10.0, earL=3, earR=3, clX=-4)),   # sits back up
        (21, k(earL=-2, earR=-2, clX=2)),
        (23, B)], 'baked')
    # ------------------------------------------------------------ doze (loop): eyes close, the head dips, a nod
    clips['edge_doze'] = _clip(36, [
        (0, B),
        (3, k(blink=0.5, hp=3.0, earL=2, earR=2)),                                        # heavy eyelids
        (6, k(blink=0.12, hp=6.0, earL=5, earR=5, armL=4.0, armR=4.0)),                   # eyes closed, ears droop
        (12, k(blink=0.12, hp=7.0, sz=0.99, earL=6, earR=6, clX=-3)),                        # slow breath in
        (18, k(blink=0.12, hp=10.0, sz=0.97, earL=7, earR=7, clX=2)),                     # breath out, head dips
        (24, k(blink=0.12, hp=10.0, sz=0.98, hr=2.0, earL=8, earR=7, clX=-3)),
        (28, k(blink=0.12, hp=11.0, sz=0.96, hr=2.0, earL=8, earR=8, clX=3)),             # nodding off...
        (30, k(blink=0.4, hp=5.0, hr=0.0, earL=-4, earR=-4, clX=6)),                      # ...a little nod-jerk
        (32, k(blink=1.0, hp=2.0, earL=1, earR=1, clX=-2)),                               # eyes flicker open
        (35, k(hp=0.5, clX=0.5))], 'baked', loop=True)
    # ------------------------------------------------------------ stand back up (base -> idle#0)
    clips['edge_stand'] = _clip(10, [
        (0, B),
        (2, k(bob=8 * px, sz=0.96, armL=-20.0, armR=-20.0, fL=0.36, fR=0.36, hL=0.14, hR=0.14,
              earL=2, earR=2)),                                                            # mittens push, feet swing
        (4, dict(bob=10 * px, fL=0.12, fR=0.12, hL=0.22, hR=0.22, armL=-14.0, armR=-14.0,
                 earL=-4, earR=-4, clX=5)),                                                # hop off the seat
        (6, dict(bob=-2 * px, sz=0.95, armL=-6.0, armR=-6.0, earL=4, earR=4, clX=-5)),   # land, a soft squash
        (8, dict(sz=1.01, earL=-1, earR=-1, clX=2)),
        (9, R)], 'baked', handoff={'end': 'idle0'})
    return clips


# nominal constants for readers that do not run Blender (stagekit, verify_w.py read frames / loop / dirs only)
CLIPS = build(2.0 / (0.7071 * 40.07), 1.0 / (0.8660 * 40.07))
