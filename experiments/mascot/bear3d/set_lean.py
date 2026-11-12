"""Stage lean (set module of the stage kit, stages/lean.json): the bear leans on a big toy block.
Pure Python (rig_bear.py --stage k keys the poses, render_set.py builds the props, make_set.py / verify_lean.py read
the same tables). Set contract (CLAUDE.md "Stage kit"): SETS, PARTS, PROPS, geometry, build, handoff, build_props,
prop_state.

Every frame is (q, st): q = the Stage K pose (anim_clips.REST_K) plus the set placement X, Y, Z (set frame, world
units) and yaw (Stage A convention, 0 = SW, 90 = SE); st = the prop state (the block never changes: {}).
Ori A (written here): the hand-off spot H, the bear facing SE; the block stands on the bear's LEFT (NE on screen, a
little behind it, so it never covers the bear). Ori B = the 3D mirror (anim_clips.mirror_k): facing SW, the block on
its right (NW on screen).

Choreography (ori A). A side lean (the design's fallback: a back lean hides the block behind the round body):
  lean_in  (10)  two little side shuffles toward the block, the left mitten swings out and comes to rest on the block
                 top, a 1 px settle.
  lean     (30, loop) relaxed against the block: a slow breath (head bob), ear and clover wiggles, a foot tap and a
                 contented little nod. Upright head, face patches (set_clips, patch SE / SW), mitten far from the face.
  lean_out (10)  the mitten lifts off, two shuffles back to H, rest.
Joins: lean_in last = lean 0 = lean_out 0 (LEAN0). Hand-offs: lean_in frame 0 and lean_out's last = props + idle#0.
"""
import math
from anim_clips import REST_K

REST_S = dict(REST_K, X=0.0, Y=0.0, Z=0.0, yaw=0.0)
HAND_YAW = 90.0
SETS = {
    'lean': dict(
        clips=['lean_in', 'lean', 'lean_out'],
        counts=dict(lean_in=10, lean=30, lean_out=10),
        loop=['lean'],
        joins=[('lean_in', -1, 'lean', 0), ('lean_out', 0, 'lean', 0)],
        handoff={'lean_in': 'start', 'lean_out': 'end'},
        hand_yaw=HAND_YAW,
        face=['lean'],
        patch={'A': 'SE', 'B': 'SW'},
        next={'lean_in': 'lean', 'lean': 'lean or lean_out', 'lean_out': 'Stage A idle (rest pose, same spot and facing)'},
        ori_note={'A': 'the bear faces SE, the toy block on its left (NE on screen)',
                  'B': 'mirror of A: the bear faces SW, the block on its right (NW on screen)'},
        note=('Stage lean: lean_in -> lean (loop) -> lean_out. Stage B format: each frame is the whole set (block + bear) '
              'baked on one canvas; anchor = pixel of the set origin (the block footprint centre on the floor). Hand-off: '
              'lean_in frame 0 and lean_out\'s last frame are pixel-identical to the props-only frame + the Stage A idle '
              'sprite (frame 0) at handoff.px from the anchor. Joins: lean_in last = lean 0 = lean_out 0. lean: face '
              'layers from face_lean.json set_clips (patch in face_patch_dir).'),
    ),
}
# prop pixel table (pixelize_set.py): pass id (kit range 50+), ramp (palette name) + 3 indices dark -> light,
# material group (shading thresholds, seams), thin-claim fraction or None
PARTS = {
    'block': dict(pid=50, ramp='wood', idx=(2, 3, 4), mg='wood', thresh=(0.50, 0.74)),
    'block_panel': dict(pid=51, ramp='fabric_rose', idx=(1, 2, 3), mg='rose', thresh=(0.44, 0.70)),
}
BLOCK_H = 0.48                   # half extents: a big toy block about 0.96 tall (the mitten rests on its top)
BLOCK_W = 0.55
PROPS = {'toy_block': dict(layers=['toy_block'],
                           footprint=dict(x=[-BLOCK_W, BLOCK_W], y=[-BLOCK_W, BLOCK_W]),
                           note='a big soft-cornered wooden toy block with rose inset panels')}
SIDE = 0.30                      # the bear's side shuffle toward the block (world units)
REACH = 0.74                     # lean root -> block face (the left mitten rests on the top edge)
SP_LEAN, ARM_LEAN = 62.0, 6.0    # left arm: sideways raise onto the block top, a little forward


def geometry(s, ppu, pxz):
    """set frame = the block footprint centre on the floor. Snapped so the bear's lean spot and the hand-off spot
    are whole screen pixels from the anchor (face patches and idle#0 land on whole pixels)."""
    import stagekit
    by = REACH + BLOCK_W                      # block centre, along +Y (the bear's left at yaw 90) from the lean spot
    lx, ly, lean_px = stagekit.snap(ppu, 0.10, -by)
    hx, hy, hand_px = stagekit.snap(ppu, lx, ly - SIDE)
    return dict(PXZ=pxz, ppu=ppu, hand=[hx, hy], lean=[lx, ly], hand_px=list(hand_px), lean_px=list(lean_px),
                block=[0.0, 0.0], block_half=[BLOCK_W, BLOCK_W, BLOCK_H])


def build(s, g):
    PXZ = g['PXZ']; hx, hy = g['hand']; lx, ly = g['lean']
    out = {}

    def F(**kw):
        q = dict(REST_S, X=hx, Y=hy, yaw=HAND_YAW); q.update(kw); return (q, {})

    def L(**kw):                         # at the lean spot, the left mitten on the block
        return F(**dict(dict(X=lx, Y=ly, spL=SP_LEAN, armL=ARM_LEAN), **kw))

    def V(base, **kw): return (dict(base[0], **kw), base[1])
    y = lambda t: hy + (ly - hy) * t
    LEAN0 = L(bob=-PXZ)                                   # settled 1 px against the block
    out['lean_in'] = [
        F(),                                                                         # rest at the hand-off spot
        F(Y=y(0.10), earL=1, earR=1, clY=2, fL=-0.04, hL=0.07),                      # left foot steps sideways
        F(Y=y(0.30), bob=PXZ, earL=-1, earR=-1, clY=-2),
        F(Y=y(0.50), earL=1, earR=1, clY=2, fR=-0.04, hR=0.07),                      # the right foot follows
        F(Y=y(0.80), bob=PXZ, spL=14, armL=4, earL=-1, earR=-1, clX=2),
        F(Y=ly, spL=34, armL=8, earL=2, earR=1, clY=3),                              # the mitten swings out
        L(spL=SP_LEAN + 8, armL=8, earL=3, earR=2, clX=-3),                          # up onto the top
        L(earL=1, earR=1, clX=2),                                                    # it rests there
        L(bob=-PXZ, earL=-1, earR=-1, clY=-2),                                       # 1 px settle
        LEAN0,
    ]
    breath = dict(hb=-1)
    out['lean'] = [
        LEAN0, LEAN0, LEAN0, V(LEAN0, **breath), V(LEAN0, **breath), V(LEAN0, **breath), LEAN0, LEAN0,
        V(LEAN0, earL=2, earR=-1, clY=3), V(LEAN0, earL=-1, earR=2, clY=-3), V(LEAN0, clY=1), LEAN0,
        V(LEAN0, fR=0.10, hR=0.05), V(LEAN0, fR=0.10, hR=0.10), V(LEAN0, fR=0.10), V(LEAN0, fR=0.10, hR=0.08),  # tap
        V(LEAN0, fR=0.10), LEAN0, LEAN0,
        V(LEAN0, **breath), V(LEAN0, **breath), V(LEAN0, **breath), LEAN0,
        V(LEAN0, hb=-1, earL=2, earR=2, clX=3), V(LEAN0, hb=-1, earL=1, earR=1, clX=-2),   # contented nod
        LEAN0, V(LEAN0, clX=1), V(LEAN0, earL=-1, earR=-1), LEAN0, LEAN0,
    ]
    out['lean_out'] = [
        LEAN0,
        L(earL=1, earR=1, clX=2),
        L(spL=SP_LEAN + 8, armL=8, earL=2, earR=2, clY=3),                           # lift the mitten off
        F(Y=ly, spL=34, armL=8, earL=1, earR=1, clY=1),
        F(Y=y(0.80), spL=14, armL=4, bob=PXZ, earL=-1, earR=-1, clX=-2),
        F(Y=y(0.50), earL=1, earR=1, clY=-2, fR=0.04, hR=0.07),                      # shuffle back
        F(Y=y(0.30), bob=PXZ, earL=-1, earR=-1, clY=2),
        F(Y=y(0.10), earL=1, earR=1, fL=0.04, hL=0.07),
        F(earL=-1, earR=-1, clY=-1),
        F(),                                                                         # rest at the hand-off spot
    ]
    for c, n in SETS[s]['counts'].items(): assert len(out[c]) == n, (c, len(out[c]), n)
    return out


def handoff(s, g):
    return {'lean_in': dict(at='start', pos=list(g['hand']), yaw=HAND_YAW),
            'lean_out': dict(at='end', pos=list(g['hand']), yaw=HAND_YAW)}


def build_props(s, kit, g):
    """kit (render_set.py): empty(name, loc, parent), solid(name, parent, layer, pid, shapes), W0 (the set root)."""
    P = kit.empty('P_toy_block', (0, 0, 0), kit.W0)
    W, H = BLOCK_W, BLOCK_H
    kit.solid('block', P, 'toy_block', PARTS['block']['pid'], [((0, 0, H), (W, W, H), 0.22, 0.22)])
    inset, t = 0.36, 0.035                     # rose inset panels standing proud of the four side faces
    panels = []
    for ax, sg in (('x', 1), ('x', -1), ('y', 1), ('y', -1)):
        c = (sg * (W + t * 0.2), 0, H) if ax == 'x' else (0, sg * (W + t * 0.2), H)
        r = (t, inset, H * 0.62) if ax == 'x' else (inset, t, H * 0.62)
        panels.append((c, r, 0.30, 0.30))
    panels.append(((0, 0, 2 * H + t * 0.2), (inset, inset, t), 0.30, 0.30))       # and the top
    kit.solid('block_panel', P, 'toy_block', PARTS['block_panel']['pid'], panels)
    return {'toy_block': dict(px=[0, 0], pos=[0.0, 0.0, 0.0])}


def prop_state(s, st):
    """layer -> render key of this frame's prop state (the block is static)."""
    return {'toy_block': 'toy_block_0'}
