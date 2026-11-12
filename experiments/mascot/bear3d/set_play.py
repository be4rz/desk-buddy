"""Stage play (set module of the stage kit, stages/play.json): five toy sets, each enter -> loop -> exit, ori A + B.

    ball     ball_get 12, ball_bounce 24 (loop), ball_drop 12            a big striped beach ball
    bubbles  bubbles_get 12, bubbles_blow 32 (loop), bubbles_put 12      a bubble bottle + wand, baked bubbles
    yoyo     yoyo_get 10, yoyo 24 (loop, face layers), yoyo_put 10      a yoyo on a cord
    blocks   blocks_sit 12, blocks_build 48 (loop), blocks_stand 12     three toy blocks; the tower topples to frame 0
    balloon  balloon_get 12, balloon_bob 32 (loop), balloon_let_go 18   a balloon tethered to a toy weight

Set contract as set_lean.py (CLAUDE.md "Stage kit"): SETS, PARTS, PROPS, geometry, build, handoff, build_props,
prop_state, + mirror_state. Every frame is (q, st): q = the Stage K pose plus the set placement X Y Z yaw (ori A,
world units, yaw 0 = SW, 90 = SE); st = where every toy object is in that frame. Ori A: the hand-off spot H is the set
origin, the bear faces SE (+X), the toys lie on its left / front-left (screen E / NE) so at H they never cover it.
Ori B = the 3D mirror (anim_clips.mirror_k + mirror_state below; render_set mirrors the props' set root W0).

MOVING PROPS (the lead's decision for p5, no kit edit): render_set.py builds the props once (build_props) and then,
for every frame, poses the rig (scene.frame_set) and only THEN calls prop_state(set, st) for the layers' render keys.
This module relies on that order: prop_state (running inside Blender) also PLACES each toy's empty from st, reading
the posed paw socket bones paw_L / paw_R of the rig for held toys, and returns key = layer + hash of the placed
matrices, so every distinct toy state renders once and an unchanged one (the home state) reuses its render.
verify_play.py proves the result (hand-offs = home props + idle#0, joins exact, toys home at every hand-off).
A future kit hook should take over: (1) a per-frame "apply prop state" call (place(objects, st, rig) -> keys), (2) the
held-object resolver (paw socket + bear-local offset, both paws, a mix of two points), (3) strings between two points,
(4) hiding by scale, (5) the matrix-hash key. Everything below is private to this module.

st = {object: spec}. spec = ['x'] (hidden) | ['p', point, [rx, ry, rz] deg in the set frame, [sx, sy, sz]]
                          | ['s', point, point, radius] (a cord between two points)
point = ['f', [x, y, z]]                 set frame (ori A), mirrored with W0 in ori B
      | ['h', 'L'|'R'|'both', [fwd, left, up]]   the posed paw socket tip(s) + a bear-local offset
      | ['m', point, point, t]           a mix of two points
      | ['a', point, [dx, dy, dz]]       a point + a set-frame offset
      | ['z', point, z]                  a point with its world height replaced (z = 0 is the floor)
      | ['o', object, [x, y, z]]         a point in an object's local frame (where it is this frame; resolve it first)
"""
import math, hashlib
from anim_clips import REST_K, expand_k

REST_S = dict(REST_K, X=0.0, Y=0.0, Z=0.0, yaw=0.0)
HAND_YAW = 90.0
PXZ_DEFAULT = 0.0288


def _set(clips, counts, loop, face=(), patch=None, nxt=None, ori_note=None, note=''):
    j = [(clips[0], -1, clips[1], 0), (clips[2], 0, clips[1], 0)]
    return dict(clips=list(clips), counts=dict(zip(clips, counts)), loop=[clips[1]], joins=j,
                handoff={clips[0]: 'start', clips[2]: 'end'}, hand_yaw=HAND_YAW, face=list(face),
                patch=patch or {}, next=nxt or {clips[0]: clips[1], clips[1]: f'{clips[1]} or {clips[2]}',
                                                clips[2]: 'Stage A idle (rest pose, same spot and facing)'},
                ori_note=ori_note or {'A': 'the bear faces SE, the toy on its left / front-left (E / NE on screen)',
                                      'B': 'mirror of A: the bear faces SW, the toy on its right (W / NW on screen)'},
                note=note)


SETS = {
    'ball': _set(['ball_get', 'ball_bounce', 'ball_drop'], [12, 24, 12], True,
                 note='Stage play ball: ball_get -> ball_bounce (loop) -> ball_drop; a big striped beach ball.'),
    'bubbles': _set(['bubbles_get', 'bubbles_blow', 'bubbles_put'], [12, 32, 12], True,
                    note='Stage play bubbles: bubbles_get -> bubbles_blow (loop) -> bubbles_put; a bottle + wand, '
                         'baked bubbles (none at the loop seam).'),
    'yoyo': _set(['yoyo_get', 'yoyo', 'yoyo_put'], [10, 24, 10], True, face=['yoyo'], patch={'A': 'SE', 'B': 'SW'},
                 note='Stage play yoyo: yoyo_get -> yoyo (loop, face layers from face_play.json set_clips, patch in '
                      'face_patch_dir) -> yoyo_put.'),
    'blocks': _set(['blocks_sit', 'blocks_build', 'blocks_stand'], [12, 48, 12], True,
                   note='Stage play blocks: blocks_sit -> blocks_build (loop: three blocks stacked, the tower topples '
                        'back to frame 0) -> blocks_stand.'),
    'balloon': _set(['balloon_get', 'balloon_bob', 'balloon_let_go'], [12, 32, 18], True,
                    note='Stage play balloon: balloon_get -> balloon_bob (loop) -> balloon_let_go; the balloon is '
                         'tethered to a toy weight, let go it springs back to the tether.'),
}
NOTE = ('Stage B format: each frame is the whole set (toys + bear) baked on one canvas; anchor = pixel of the set '
        'origin (the hand-off spot). Hand-off: the enter clip\'s frame 0 and the exit clip\'s last frame are '
        'pixel-identical to the props-only frame + the Stage A idle sprite (frame 0) at handoff.px from the anchor.')
for _sd in SETS.values(): _sd['note'] = _sd['note'] + ' ' + NOTE

# prop pixel table (pixelize_set.py): pass id (50+), ramp + 3 indices dark -> light, material group, thresholds
_T = dict(rose=(0.44, 0.70), cream=(0.46, 0.70), sage=(0.44, 0.70), quilt=(0.46, 0.70), wood=(0.50, 0.74),
          plastic=(0.46, 0.70), bubble=(0.40, 0.66))
def _p(pid, ramp, idx, mg): return dict(pid=pid, ramp=ramp, idx=idx, mg=mg, thresh=_T[mg])
PARTS = {
    'ball_skin': _p(50, 'fabric_rose', (1, 2, 3), 'rose'), 'ball_band': _p(51, 'fabric_cream', (2, 3, 4), 'cream'),
    'bottle': _p(52, 'fabric_sage', (1, 2, 3), 'sage'), 'bottle_cap': _p(53, 'toy_plastic', (1, 2, 3), 'plastic'),
    'wand_stick': _p(54, 'toy_plastic', (1, 2, 3), 'plastic'), 'wand_ring': _p(55, 'fabric_quilt', (1, 2, 3), 'quilt'),
    'bubble': _p(56, 'screen', (2, 3, 4), 'bubble'),
    'yoyo_disc': _p(57, 'fabric_quilt', (1, 2, 3), 'quilt'), 'yoyo_hub': _p(58, 'fabric_cream', (2, 3, 4), 'cream'),
    'cord': _p(59, 'fabric_cream', (2, 3, 4), 'cream'),
    'block_rose': _p(60, 'fabric_rose', (1, 2, 3), 'rose'), 'block_sage': _p(61, 'fabric_sage', (1, 2, 3), 'sage'),
    'block_quilt': _p(62, 'fabric_quilt', (1, 2, 3), 'quilt'),
    'weight': _p(63, 'wood', (2, 3, 4), 'wood'), 'balloon': _p(64, 'fabric_rose', (1, 2, 3), 'rose'),
}

# ------------------------------------------------------------------ toy sizes and home spots (ori A, set frame)
R_BALL = 0.32
BALL_HOME = [1.25, 0.35, R_BALL]
BOTTLE_HOME = [0.95, 0.60, 0.0]
WAND_HOME_Z = 0.20                     # the wand stands in the bottle
YOYO_R = 0.11
YOYO_HOME = [1.00, 0.45, YOYO_R]
BLK = 0.17                             # block half size
BLOCK_HOME = [[0.55, 1.10, BLK], [1.10, 0.50, BLK], [1.05, 1.05, BLK]]
BLOCK_ROT = [12.0, -20.0, 35.0]        # their resting turn (deg about z)
WEIGHT_HOME = [0.30, 1.05, 0.0]
BALLOON_H = 2.35                       # balloon centre height when tethered
B_RX, B_RZ = 0.27, 0.32                # balloon radii (a slightly tall oval)
N_BUB = 6


def geometry(s, ppu, pxz):
    """the hand-off spot H is the set origin (0, 0): whole pixels from the anchor by construction; every other bear
    spot is snapped so the bear root lands on whole pixels (the yoyo loop's face patches need it)."""
    import stagekit
    g = dict(PXZ=pxz, ppu=ppu, hand=[0.0, 0.0], hand_px=[0, 0])
    spots = {'ball': (0.30, 0.10), 'bubbles': (0.25, 0.25), 'yoyo': (0.25, 0.20), 'blocks': (0.0, 0.0),
             'balloon': (0.0, 0.15)}
    x, y, px = stagekit.snap(ppu, *spots[s])
    g['spot'] = [x, y]; g['spot_px'] = list(px)
    return g


# ------------------------------------------------------------------ point / spec helpers (pure python)
def F(x, y, z): return ['f', [x, y, z]]
def Hd(side, fwd=0.0, left=0.0, up=0.0): return ['h', side, [fwd, left, up]]
def M(a, b, t): return ['m', a, b, t]
def A(p, dx=0.0, dy=0.0, dz=0.0): return ['a', p, [dx, dy, dz]]
def Z(p, z): return ['z', p, z]
def Ob(name, x=0.0, y=0.0, z=0.0): return ['o', name, [x, y, z]]
def P(point, rot=(0, 0, 0), scale=(1, 1, 1)): return ['p', point, list(rot), list(scale)]
def S(a, b, r=0.018): return ['s', a, b, r]
HIDE = ['x']


def lerp(a, b, t): return a + (b - a) * t
def ease(t): t = min(max(t, 0.0), 1.0); return t * t * (3 - 2 * t)


def poses(n, keys, base):
    """bear poses: Stage K keys [(frame, {param: value})] on the set rest (placement included), eased."""
    rest = dict(REST_S, **base)
    return expand_k(n, [(f, kv) for f, kv in keys], rest)


def _mirror_point(p):
    k = p[0]
    if k == 'h': return ['h', {'L': 'R', 'R': 'L'}.get(p[1], p[1]), [p[2][0], -p[2][1], p[2][2]]]
    if k == 'm': return ['m', _mirror_point(p[1]), _mirror_point(p[2]), p[3]]
    if k in ('a', 'z'): return [k, _mirror_point(p[1])] + p[2:]
    return p


def mirror_state(st):
    """ori B: held toys move to the other paw with the bear-local left offset negated; set-frame points stay (W0
    mirrors them)."""
    out = {}
    for n, sp in st.items():
        if sp[0] == 'p': out[n] = ['p', _mirror_point(sp[1]), sp[2], sp[3]]
        elif sp[0] == 's': out[n] = ['s', _mirror_point(sp[1]), _mirror_point(sp[2]), sp[3]]
        else: out[n] = sp
    return out


# ------------------------------------------------------------------ choreography
def build(s, g):
    PXZ = g['PXZ']; sx, sy = g['spot']
    out = globals()['_build_' + s](g, PXZ, sx, sy)
    for c, n in SETS[s]['counts'].items(): assert len(out[c]) == n, (c, len(out[c]), n)
    return out


def _frames(qs, sts):
    assert len(qs) == len(sts)
    return [(q, st) for q, st in zip(qs, sts)]


def _ball_home(): return P(F(*BALL_HOME))


def _build_ball(g, PXZ, sx, sy):
    base = dict(X=0.0, Y=0.0, yaw=HAND_YAW)
    # the hold: both mittens forward around the ball in front of the tummy, a happy open mouth
    HOLD = dict(X=sx, Y=sy, armL=30.0, armR=30.0, spL=-14.0, spR=-14.0, mouth='open')
    held = lambda dz=0.0, rx=0.0: P(Hd('both', 0.30, 0.0, -0.06 + dz), (rx, 0, 0))
    floor = lambda sq=1.0: P(Z(Hd('both', 0.16, 0.0, 0.0), R_BALL * sq), (0, 0, 0), (1 / math.sqrt(sq), 1 / math.sqrt(sq), sq))
    out = {}
    # ball_get: two little steps to the ball, bend, scoop it up into both mittens
    q = poses(12, [(0, {}), (2, dict(X=sx * 0.4, Y=sy * 0.4, fL=-0.04, hL=0.07, earL=2, earR=1, clY=3)),
                   (4, dict(X=sx, Y=sy, fR=-0.04, hR=0.07, earL=-1, earR=-1, clY=-2)),
                   (6, dict(X=sx, Y=sy, lean=30.0, bob=-2 * PXZ, armL=-4.0, armR=-4.0, spL=-14.0, spR=-14.0, hp=12.0,
                            earL=3, earR=3)),
                   (7, dict(X=sx, Y=sy, lean=30.0, bob=-2 * PXZ, armL=-4.0, armR=-4.0, spL=-14.0, spR=-14.0, hp=12.0,
                            mouth='open')),
                   (9, dict(X=sx, Y=sy, lean=10.0, armL=18.0, armR=18.0, spL=-14.0, spR=-14.0, mouth='open', earL=-2,
                            earR=-2)),
                   (11, HOLD)], base)
    st = [{'ball': _ball_home()} for _ in range(6)]
    st += [{'ball': P(M(F(*BALL_HOME), Z(Hd('both', 0.10, 0.0, 0.0), R_BALL), 0.6))}]
    st += [{'ball': P(Z(Hd('both', 0.22, 0.0), R_BALL))}, {'ball': P(Z(Hd('both', 0.26, 0.0), R_BALL + 0.08))}, {'ball': held(-0.02)}, {'ball': held()}, {'ball': held()}]
    out['ball_get'] = _frames(q, st)
    # ball_bounce: three dribbles (8 frames each): push down, the ball drops, squashes on the floor, comes back up
    keys, sts = [], []
    for c in range(3):
        o = 8 * c
        keys += [(o, HOLD), (o + 1, dict(HOLD, armL=18.0, armR=18.0, lean=8.0, bob=-PXZ, earL=2, earR=2)),
                 (o + 3, dict(HOLD, armL=8.0, armR=8.0, lean=14.0, hp=10.0, earL=-2, earR=-2, clY=4)),
                 (o + 5, dict(HOLD, armL=14.0, armR=14.0, lean=10.0, hp=8.0)),
                 (o + 7, dict(HOLD, bob=PXZ if c == 1 else 0.0, earL=1, earR=1, clX=2))]
        sts += [{'ball': held()}, {'ball': held(-0.05)}, {'ball': P(Z(Hd('both', 0.22, 0.0), 0.55))},
                {'ball': floor(0.86)}, {'ball': P(Z(Hd('both', 0.22, 0.0), 0.50))},
                {'ball': held(-0.04)}, {'ball': held(-0.01)}, {'ball': held()}]
    out['ball_bounce'] = _frames(poses(24, keys, base), sts)
    # ball_drop: let it fall, it bounces once and rolls home; the bear steps back
    q = poses(12, [(0, HOLD), (1, dict(HOLD, armL=36.0, armR=36.0, spL=-6.0, spR=-6.0)),
                   (2, dict(X=sx, Y=sy, armL=6.0, armR=6.0, spL=4.0, spR=4.0, mouth='open', earL=3, earR=3)),
                   (5, dict(X=sx, Y=sy, armL=2.0, armR=2.0, hp=8.0, earL=-1, earR=-1)),
                   (7, dict(X=sx * 0.6, Y=sy * 0.6, fR=0.04, hR=0.07, hp=6.0)),
                   (9, dict(X=sx * 0.2, Y=sy * 0.2, fL=0.04, hL=0.07, earL=1, earR=1)),
                   (11, {})], base)
    st = [{'ball': held()}, {'ball': held(0.04)}, {'ball': P(Z(Hd('both', 0.26, 0.0), 0.45))},
          {'ball': P(Z(Hd('both', 0.20, 0.0), R_BALL * 0.88), (0, 0, 0), (1.07, 1.07, 0.88))}]
    for i, t in enumerate((0.25, 0.5, 0.72, 0.88, 0.96, 1.0, 1.0, 1.0)):
        st.append({'ball': P(M(F(BALL_HOME[0] - 0.05, BALL_HOME[1], R_BALL + 0.10 * math.sin(math.pi * min(t * 1.6, 1))),
                               F(*BALL_HOME), ease(t)) if i else
                             M(Z(Hd('both', 0.32, 0.0), R_BALL + 0.18), F(*BALL_HOME), 0.3),
                             (0, -90 * (1 - ease(t)) if i else 0, 0) if t < 1 else (0, 0, 0))})
    st[-1] = {'ball': _ball_home()}
    out['ball_drop'] = _frames(q, st)
    return out


def _bubbles_home():
    return {'bottle': P(F(*BOTTLE_HOME)), 'wand': P(F(BOTTLE_HOME[0], BOTTLE_HOME[1], WAND_HOME_Z)),
            **{f'bub{i}': HIDE for i in range(N_BUB)}}


def _build_bubbles(g, PXZ, sx, sy):
    base = dict(X=0.0, Y=0.0, yaw=HAND_YAW)
    # the hold: the left mitten up beside the mouth with the wand, the ring just in front of the face's left side
    HOLD = dict(X=sx, Y=sy, armL=58.0, spL=18.0)
    wand_held = P(Hd('L', 0.02, 0.02, -0.12), (-22, 0, 0))
    ring = Hd('L', 0.20, 0.03, 0.32)            # where the wand's ring is while held (bubbles start there)
    out = {}
    q = poses(12, [(0, {}), (2, dict(X=sx * 0.5, Y=sy * 0.5, fL=-0.04, hL=0.07, earL=2, clY=3)),
                   (4, dict(X=sx, Y=sy, spL=30.0, armL=20.0, lean=8.0, fR=-0.04, hR=0.07, hp=8.0)),
                   (6, dict(X=sx, Y=sy, spL=34.0, armL=24.0, lean=12.0, hp=10.0, earL=-2, earR=-2)),
                   (8, dict(X=sx, Y=sy, spL=24.0, armL=40.0, hp=4.0, mouth='open')),
                   (11, HOLD)], base)
    home = _bubbles_home()
    st = [dict(home) for _ in range(6)]
    st += [dict(home, wand=P(M(F(BOTTLE_HOME[0], BOTTLE_HOME[1], WAND_HOME_Z), Hd('L', 0.02, 0.02, -0.12), 0.5)))]
    st += [dict(home, wand=P(M(F(BOTTLE_HOME[0], BOTTLE_HOME[1], WAND_HOME_Z + 0.25), Hd('L', 0.02, 0.02, -0.12), 0.85),
                             (-10, 0, 0)))]
    st += [dict(home, wand=wand_held) for _ in range(4)]
    out['bubbles_get'] = _frames(q, st)
    # bubbles_blow: breathe in, puff (round open mouth), bubbles float up and drift; twice per loop
    keys = [(0, HOLD), (3, dict(HOLD, hb=1, earL=-2, earR=-2, clY=2)), (5, dict(HOLD, hb=1, earL=-2, earR=-2)),
            (6, dict(HOLD, mouth='open', hp=-4.0, earL=2, earR=2)), (9, dict(HOLD, mouth='open', hp=-4.0)),
            (11, dict(HOLD, hp=-6.0, mouth='wide', earL=3, earR=3, clX=3)), (14, dict(HOLD, hp=-8.0, mouth='wide')),
            (16, HOLD), (19, dict(HOLD, hb=1, earL=-2, earR=-2, clY=-2)), (21, dict(HOLD, hb=1)),
            (22, dict(HOLD, mouth='open', hp=-4.0, earL=2, earR=2)), (25, dict(HOLD, mouth='open', hp=-4.0)),
            (27, dict(HOLD, hp=-6.0, mouth='wide', earL=3, earR=3, clX=-3)), (29, dict(HOLD, hp=-4.0)), (31, HOLD)]
    q = poses(32, keys, base)
    # bubble i: born at frame b, lives L frames, drifts up and away (+x, -y = toward the camera's right) with a wobble
    births = [(6, 0.00), (7, 0.9), (9, 1.7), (22, 0.4), (23, 1.3), (24, 2.2)]
    life = 7
    sizes = [0.075, 0.060, 0.085, 0.070, 0.085, 0.060]
    st = []
    for f in range(32):
        d = dict(home, wand=wand_held)
        for i, (b, ph) in enumerate(births):
            a = f - b
            if 0 <= a < life:
                t = a / (life - 1)
                r = sizes[i] * (0.55 + 0.45 * min(1.0, a / 2))
                d[f'bub{i}'] = P(A(ring, 0.10 + 0.55 * t + 0.04 * math.sin(ph + a), -0.10 - 0.25 * t,
                                   0.05 + 0.55 * t + 0.03 * math.cos(ph * 2 + a)), (0, 0, 0), (r, r, r))
        st.append(d)
    out['bubbles_blow'] = _frames(q, st)
    q = poses(12, [(0, HOLD), (3, dict(X=sx, Y=sy, spL=24.0, armL=40.0, hp=4.0)),
                   (5, dict(X=sx, Y=sy, spL=34.0, armL=24.0, lean=12.0, hp=10.0, earL=-2, earR=-2)),
                   (6, dict(X=sx, Y=sy, spL=30.0, armL=20.0, lean=8.0, hp=8.0)),
                   (8, dict(X=sx, Y=sy, earL=1, earR=1)),
                   (9, dict(X=sx * 0.5, Y=sy * 0.5, fR=0.04, hR=0.07, clY=-2)), (11, {})], base)
    st = [dict(home, wand=wand_held) for _ in range(4)]
    st += [dict(home, wand=P(M(F(BOTTLE_HOME[0], BOTTLE_HOME[1], WAND_HOME_Z + 0.25), Hd('L', 0.02, 0.02, -0.12), 0.85),
                             (-10, 0, 0)))]
    st += [dict(home, wand=P(M(F(BOTTLE_HOME[0], BOTTLE_HOME[1], WAND_HOME_Z), Hd('L', 0.02, 0.02, -0.12), 0.4)))]
    st += [dict(home) for _ in range(6)]
    out['bubbles_put'] = _frames(q, st)
    return out


def _yoyo_home(): return {'yoyo': P(F(*YOYO_HOME), (90, 0, 25)), 'cord': HIDE}


def _build_yoyo(g, PXZ, sx, sy):
    base = dict(X=0.0, Y=0.0, yaw=HAND_YAW)
    # the yoyo loop (face layers): upright head, mouth closed, the left mitten held out to the side, the yoyo on its
    # cord below the mitten, beside the body (never in front of the face)
    UP = dict(X=sx, Y=sy, spL=40.0, armL=8.0)
    paw = Hd('L', 0.0, 0.06, 0.0)
    out = {}
    q = poses(10, [(0, {}), (2, dict(X=sx * 0.5, Y=sy * 0.5, fL=-0.04, hL=0.07, earL=2, clY=3)),
                   (4, dict(X=sx, Y=sy, lean=26.0, bob=-2 * PXZ, spL=26.0, armL=-2.0, hp=12.0, fR=-0.04, hR=0.07)),
                   (5, dict(X=sx, Y=sy, lean=26.0, bob=-2 * PXZ, spL=26.0, armL=-2.0, hp=12.0)),
                   (7, dict(X=sx, Y=sy, lean=8.0, spL=36.0, armL=6.0, hp=4.0, earL=-2, earR=-2)),
                   (9, UP)], base)
    home = _yoyo_home()
    hang = lambda d, spin=0.0: {'yoyo': P(A(paw, 0, 0, -0.05 - d), (90, spin, 0)), 'cord': S(paw, A(paw, 0, 0, -0.05 - d))}
    st = [dict(home) for _ in range(5)]
    st += [{'yoyo': P(M(F(*YOYO_HOME), A(paw, 0, 0, -0.10), 0.5), (90, 0, 12)), 'cord': HIDE}]
    st += [hang(0.10), hang(0.12), hang(0.06), hang(0.0)]
    out['yoyo_get'] = _frames(q, st)
    # yoyo: down and up twice (12 frames each); the mitten flicks at the bottom; a little happy bounce
    keys = [(0, UP), (2, dict(UP, armL=4.0, earL=1, earR=1)), (5, dict(UP, armL=0.0, spL=36.0, clY=3)),
            (6, dict(UP, armL=12.0, spL=42.0, earL=-2, earR=-2)), (9, dict(UP, armL=10.0, hb=1, clY=-3)),
            (11, UP), (12, UP), (14, dict(UP, armL=4.0, earL=-1, earR=2)), (17, dict(UP, armL=0.0, spL=36.0, clX=3)),
            (18, dict(UP, armL=12.0, spL=42.0, earL=2, earR=-1)), (21, dict(UP, armL=10.0, hb=1, clX=-3)), (23, UP)]
    q = poses(24, keys, base)
    depth = [0.0, 0.10, 0.28, 0.48, 0.62, 0.70, 0.66, 0.50, 0.30, 0.13, 0.03, 0.0]
    st = [hang(depth[f % 12], spin=(f % 12) * 60.0 if 0 < f % 12 < 11 else 0.0) for f in range(24)]
    out['yoyo'] = _frames(q, st)
    q = poses(10, [(0, UP), (2, dict(X=sx, Y=sy, lean=8.0, spL=36.0, armL=6.0, hp=4.0)),
                   (4, dict(X=sx, Y=sy, lean=26.0, bob=-2 * PXZ, spL=26.0, armL=-2.0, hp=12.0)),
                   (5, dict(X=sx, Y=sy, lean=26.0, bob=-2 * PXZ, spL=26.0, armL=-2.0, hp=12.0, earL=2, earR=2)),
                   (7, dict(X=sx * 0.5, Y=sy * 0.5, fR=0.04, hR=0.07, clY=-2)), (9, {})], base)
    st = [hang(0.0), hang(0.06), hang(0.10), hang(0.12)]
    st += [{'yoyo': P(M(F(*YOYO_HOME), A(paw, 0, 0, -0.10), 0.5), (90, 0, 12)), 'cord': HIDE}]
    st += [dict(home) for _ in range(5)]
    out['yoyo_put'] = _frames(q, st)
    return out


TOWER = [0.74, 0.66]                     # the tower spot, in front of the seated bear (it sits facing E)
SIT_YAW = 135.0


def _blocks_home():
    return {f'blk{i}': P(F(*BLOCK_HOME[i]), (0, 0, BLOCK_ROT[i])) for i in range(3)}


def _build_blocks(g, PXZ, sx, sy):
    base = dict(X=0.0, Y=0.0, yaw=HAND_YAW)
    SIT = dict(yaw=SIT_YAW, bob=-3 * PXZ, fL=0.34, fR=0.34, hL=0.03, hR=0.03, armL=12.0, armR=12.0)
    home = _blocks_home()
    out = {}
    # blocks_sit: turn toward the blocks (E), plop down to sit (Stage p's SIT pose)
    q = poses(12, [(0, {}), (2, dict(yaw=105.0, fL=-0.04, hL=0.07, earL=2, clY=3)),
                   (4, dict(yaw=125.0, fR=-0.04, hR=0.07, earL=-1, clY=-2)), (5, dict(yaw=SIT_YAW)),
                   (7, dict(SIT, bob=-5 * PXZ, sz=0.95, earL=4, earR=4, clX=-5, mouth='open')),
                   (8, dict(SIT, bob=-4 * PXZ, sz=0.97, earL=-2, earR=-2, clX=3)), (11, SIT)], base)
    out['blocks_sit'] = _frames(q, [dict(home) for _ in range(12)])
    # blocks_build: pick up each block in both mittens and stack it (12 frames each), a proud wiggle, then the
    # tower wobbles and topples: the blocks tumble back to where they lay (frame 0)
    keys, st = [], []
    slot = lambda j: [TOWER[0], TOWER[1], BLK + 2 * BLK * j + 0.002 * j]
    held = lambda: P(Hd('both', 0.22, 0.0, -0.02), (0, 0, 0))
    placed = {}
    for j in range(3):
        o = 12 * j
        keys += [(o, SIT), (o + 2, dict(SIT, lean=24.0, armL=0.0, armR=0.0, spL=-10.0, spR=-10.0, hp=12.0, earL=2, earR=2)),
                 (o + 4, dict(SIT, lean=24.0, armL=2.0, armR=2.0, spL=-14.0, spR=-14.0, hp=12.0)),
                 (o + 6, dict(SIT, lean=6.0, armL=26.0 + 6 * j, armR=26.0 + 6 * j, spL=-14.0, spR=-14.0, hp=4.0 - 4 * j,
                              mouth='open')),
                 (o + 8, dict(SIT, lean=10.0, armL=22.0 + 8 * j, armR=22.0 + 8 * j, spL=-14.0, spR=-14.0, hp=6.0 - 4 * j)),
                 (o + 9, dict(SIT, lean=10.0, armL=22.0 + 8 * j, armR=22.0 + 8 * j, spL=-8.0, spR=-8.0, hp=6.0 - 4 * j)),
                 (o + 11, dict(SIT, earL=-2, earR=-2, clY=3 if j % 2 else -3))]
        for k in range(12):
            d = dict(home); d.update(placed)
            b = f'blk{j}'
            if k in (3,): d[b] = P(M(F(*BLOCK_HOME[j]), Z(Hd('both', 0.22, 0.0, 0.0), BLK), 0.5), (0, 0, BLOCK_ROT[j] * 0.5))
            elif 4 <= k <= 7: d[b] = held() if k > 4 else P(Z(Hd('both', 0.22, 0.0, 0.0), BLK + 0.04))
            elif k == 8: d[b] = P(M(Hd('both', 0.22, 0.0, -0.02), F(*slot(j)), 0.6))
            elif k >= 9: d[b] = P(F(*slot(j)))
            st.append(d)
        placed[f'blk{j}'] = P(F(*slot(j)))
    # topple (frames 36-47): wobble, tip over away from the bear, tumble home
    keys += [(36, dict(SIT, hp=-6.0, mouth='open', earL=3, earR=3, clX=4)), (37, dict(SIT, hp=-8.0, mouth='wide', hb=1)),
             (39, dict(SIT, hp=-6.0, mouth='wide')), (40, dict(SIT, hp=4.0, mouth='open', earL=-3, earR=-3, armL=30.0, armR=30.0)),
             (42, dict(SIT, hp=8.0, mouth='open', earL=4, earR=4, armL=24.0, armR=24.0)), (44, dict(SIT, hp=4.0, clY=-4)),
             (46, SIT), (47, SIT)]
    tip = [0, 6, -6, 12, 30, 60, 1, 1, 1, 1, 1, 1]
    for k in range(12):
        d = {}
        for j in range(3):
            b = f'blk{j}'; t0 = slot(j)
            if k < 4:   # wobble about the tower's foot (the higher block leans more)
                a = math.radians(tip[k]); h = t0[2]
                d[b] = P(F(t0[0] + math.sin(a) * h * 0.6, t0[1] - math.sin(a) * h * 0.2, t0[2]), (0, tip[k] * (j + 1) * 0.5, 0))
            elif k < 6:  # falling over
                a = math.radians(tip[k]); h = t0[2]
                d[b] = P(F(t0[0] + math.sin(a) * h * (0.6 + 0.3 * j), t0[1] - math.sin(a) * h * 0.2 * j,
                           max(BLK, t0[2] * math.cos(a))), (0, tip[k] * (0.6 + 0.4 * j), 0))
            else:        # tumbling home with a little hop, landing on their resting turn
                t = ease((k - 5) / 5)
                hx, hy, hz = BLOCK_HOME[j]
                fx = t0[0] + math.sin(math.radians(60)) * t0[2] * (0.6 + 0.3 * j)
                d[b] = P(F(lerp(fx, hx, t), lerp(t0[1], hy, t), hz + 0.18 * math.sin(math.pi * t) * (1 - t * 0.3)),
                         (0, lerp(36.0 * (j + 1), 0.0, t) if k < 10 else 0, lerp(0, BLOCK_ROT[j], t) if k < 10 else BLOCK_ROT[j]))
            if k >= 10: d[b] = home[b]
        st.append(d)
    out['blocks_build'] = _frames(poses(48, keys, base), st)
    q = poses(12, [(0, SIT), (2, dict(SIT, bob=-5 * PXZ, sz=0.96, armL=-14.0, armR=-14.0, earL=2, earR=2)),
                   (4, dict(yaw=SIT_YAW, bob=PXZ, sz=1.03, earL=-3, earR=-3, clY=4)), (6, dict(yaw=SIT_YAW, clY=-2)),
                   (8, dict(yaw=115.0, fL=0.04, hL=0.07)), (10, dict(yaw=95.0, fR=0.04, hR=0.07, earL=1)), (11, {})], base)
    out['blocks_stand'] = _frames(q, [dict(home) for _ in range(12)])
    return out


def _balloon_home():
    w = F(*WEIGHT_HOME)
    top = F(WEIGHT_HOME[0], WEIGHT_HOME[1], BALLOON_H - B_RZ)
    knot = F(WEIGHT_HOME[0], WEIGHT_HOME[1], 0.5 * (0.12 + BALLOON_H - B_RZ))
    return {'weight': P(w), 'balloon': P(F(WEIGHT_HOME[0], WEIGHT_HOME[1], BALLOON_H)),
            'cord_lo': S(A(w, 0, 0, 0.12), knot), 'cord_hi': S(knot, top)}


def _build_balloon(g, PXZ, sx, sy):
    base = dict(X=0.0, Y=0.0, yaw=HAND_YAW)
    HOLD = dict(X=sx, Y=sy, spL=44.0, armL=24.0, hp=-12.0, mouth='open')
    home = _balloon_home()
    w = F(*WEIGHT_HOME); wtop = A(w, 0, 0, 0.12)
    paw = Hd('L', 0.0, 0.04, 0.02)
    hknot = F(WEIGHT_HOME[0], WEIGHT_HOME[1], 0.5 * (0.12 + BALLOON_H - B_RZ))
    hball = F(WEIGHT_HOME[0], WEIGHT_HOME[1], BALLOON_H)

    def held(dx=0.0, dy=0.0, dz=0.0, knot=None, t=1.0):
        k = knot or paw
        b = A(k, -0.05 + dx, 0.15 + dy, 1.05 + dz)          # the balloon floats above the paw, a little behind
        if t < 1.0: b = M(hball, b, t)
        return {'weight': P(w), 'balloon': P(b), 'cord_lo': S(wtop, k), 'cord_hi': S(k, A(b, 0, 0, -B_RZ))}
    out = {}
    q = poses(12, [(0, {}), (2, dict(spL=20.0, armL=10.0, hp=-6.0, earL=2, clY=3)),
                   (4, dict(X=sx, Y=sy, spL=52.0, armL=16.0, hp=-8.0, fL=-0.04, hL=0.07)),
                   (6, dict(X=sx, Y=sy, spL=56.0, armL=18.0, hp=-10.0, earL=-2, earR=-2)),
                   (8, dict(X=sx, Y=sy, spL=48.0, armL=22.0, hp=-12.0, mouth='open', earL=3, earR=3)),
                   (11, HOLD)], base)
    st = [dict(home) for _ in range(6)]
    for i, t in enumerate((0.3, 0.55, 0.8, 0.95, 1.0, 1.0)):
        st.append(held(knot=M(hknot, paw, ease(t)) if t < 1 else None, t=ease(t)))
    st[-1] = held()
    out['balloon_get'] = _frames(q, st)
    # balloon_bob: a little happy bounce on the spot, the balloon bobs and sways above
    keys = [(0, HOLD), (4, dict(HOLD, bob=PXZ, earL=2, earR=2, clY=3)), (8, dict(HOLD, hp=-14.0, earL=-1, earR=-1)),
            (12, dict(HOLD, armL=30.0, hb=1, clX=3)), (16, dict(HOLD, hr=6.0, earL=3, earR=-2)),
            (20, dict(HOLD, bob=PXZ, hp=-14.0, clY=-3)), (24, dict(HOLD, armL=28.0, hr=-6.0, earL=-2, earR=3)),
            (28, dict(HOLD, earL=1, earR=1)), (31, HOLD)]
    q = poses(32, keys, base)
    st = [held(0.06 * math.sin(2 * math.pi * f / 32), 0.04 * math.sin(2 * math.pi * f / 16),
               0.05 * math.sin(2 * math.pi * f / 16 + 0.5) - 0.05 * math.sin(0.5)) for f in range(32)]
    st[0] = held()
    out['balloon_bob'] = _frames(q, st)
    # balloon_let_go: the mitten opens, the cord goes slack and the balloon floats back up to its tether (a little
    # overshoot and bob); the bear watches it go up, then waves at it and rests
    q = poses(18, [(0, HOLD), (1, dict(HOLD, spL=50.0, armL=30.0, mouth='open')),
                   (3, dict(X=sx, Y=sy, spL=20.0, armL=4.0, hp=-16.0, mouth='wide', earL=4, earR=4)),
                   (6, dict(X=sx, Y=sy, hp=-18.0, mouth='wide', earL=-2, earR=-2, clY=4)),
                   (9, dict(X=sx, Y=sy, hp=-16.0, mouth='open', bob=PXZ)),
                   (11, dict(X=sx, Y=sy, hp=-10.0, armL=40.0, spL=24.0, earL=2, earR=2)),
                   (13, dict(X=sx, Y=sy, hp=-8.0, armL=48.0, spL=34.0, clX=3)),
                   (15, dict(X=sx * 0.4, Y=sy * 0.4, hp=-2.0, fR=0.04, hR=0.07)), (17, {})], base)
    st = [held(), held(0, 0, 0.03)]
    # after the release: the knot sags below the straight line (slack), the balloon rises past home and settles
    over = [0.0, 0.0, 0.12, 0.22, 0.18, 0.08, -0.02, -0.04, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    for i in range(16):
        t = ease(min(1.0, (i + 1) / 7))
        if i >= 9: st.append(dict(home)); continue
        b0 = A(paw, -0.05, 0.15, 1.05) if i == 0 else A(F(WEIGHT_HOME[0], WEIGHT_HOME[1], BALLOON_H), 0, 0, over[i + 2])
        b = M(A(paw, -0.05, 0.15, 1.05), b0, t) if i else b0
        sag = 0.25 * (1 - ease(min(1.0, i / 6)))
        k = A(M(paw, hknot, t), 0.10 * (1 - t), 0, -sag)
        st.append({'weight': P(w), 'balloon': P(b), 'cord_lo': S(wtop, k), 'cord_hi': S(k, A(b, 0, 0, -B_RZ))})
    out['balloon_let_go'] = _frames(q, st)
    return out


def handoff(s, g):
    return {SETS[s]['clips'][0]: dict(at='start', pos=list(g['hand']), yaw=HAND_YAW),
            SETS[s]['clips'][2]: dict(at='end', pos=list(g['hand']), yaw=HAND_YAW)}


# ------------------------------------------------------------------ props (Blender side: render_set.py's kit)
PROPS = {
    'ball': dict(footprint=dict(x=[-R_BALL, R_BALL], y=[-R_BALL, R_BALL]), note='a big striped toy beach ball'),
    'bubbles': dict(footprint=dict(x=[-0.13, 0.13], y=[-0.13, 0.13]),
                    note='a bubble bottle with the wand standing in it (bubbles only while blowing)'),
    'yoyo': dict(footprint=dict(x=[-YOYO_R, YOYO_R], y=[-0.06, 0.06]), note='a chunky yoyo on its cord'),
    'blocks': dict(footprint=dict(x=[-BLK, BLK], y=[-BLK, BLK]), note='three soft toy blocks (rose, sage, blue)'),
    'balloon': dict(footprint=dict(x=[-0.1, 0.1], y=[-0.1, 0.1]), note='a balloon on a cord tied to a toy weight'),
}
LAYER_OBJS = {'ball': ['ball'], 'bubbles': ['bottle', 'wand'] + [f'bub{i}' for i in range(N_BUB)],
              'yoyo': ['yoyo', 'cord'], 'blocks': ['blk0', 'blk1', 'blk2'],
              'balloon': ['weight', 'balloon', 'cord_lo', 'cord_hi']}
_REG = {}          # object name -> its empty (this Blender process only)


def build_props(s, kit, g):
    """kit (render_set.py): empty(name, loc, parent), solid(name, parent, layer, pid, shapes), W0. Every toy object is
    a mesh centred on its own empty (so prop_state can place it); one prop layer per set."""
    _REG.clear(); _REG['W0'] = kit.W0
    pid = lambda n: PARTS[n]['pid']

    def obj(name, parts):
        e = kit.empty('E_' + name, (0, 0, 0), kit.W0); _REG[name] = e
        for part, shapes in parts: kit.solid(f'{name}_{part}', e, s, pid(part), shapes)
        return e
    if s == 'ball':
        obj('ball', [('ball_skin', [((0, 0, 0), (R_BALL,) * 3, 1.0, 1.0)]),
                     ('ball_band', [((0, 0, 0), (R_BALL * 1.02, R_BALL * 1.02, R_BALL * 0.22), 0.5, 1.0)])])
    elif s == 'bubbles':
        obj('bottle', [('bottle', [((0, 0, 0.17), (0.12, 0.12, 0.17), 0.45, 1.0)]),
                       ('bottle_cap', [((0, 0, 0.36), (0.075, 0.075, 0.04), 0.4, 1.0)])])
        obj('wand', [('wand_stick', [((0, 0, 0.12), (0.022, 0.022, 0.20), 0.3, 1.0)]),
                     ('wand_ring', [((0, 0, 0.40), (0.10, 0.03, 0.10), 1.0, 1.0)])])
        for i in range(N_BUB): obj(f'bub{i}', [('bubble', [((0, 0, 0), (1.0, 1.0, 1.0), 1.0, 1.0)])])
    elif s == 'yoyo':
        obj('yoyo', [('yoyo_disc', [((0, 0, 0.035), (YOYO_R, YOYO_R, 0.035), 0.6, 1.0),
                                    ((0, 0, -0.035), (YOYO_R, YOYO_R, 0.035), 0.6, 1.0)]),
                     ('yoyo_hub', [((0, 0, 0), (YOYO_R * 0.45, YOYO_R * 0.45, 0.075), 0.4, 1.0)])])
        obj('cord', [('cord', [((0, 0, 0), (1.0, 1.0, 0.5), 0.15, 1.0, dict(segs=12, rings=8))])])
    elif s == 'blocks':
        for i, part in enumerate(('block_rose', 'block_sage', 'block_quilt')):
            obj(f'blk{i}', [(part, [((0, 0, 0), (BLK, BLK, BLK), 0.25, 0.25)])])
    elif s == 'balloon':
        obj('weight', [('weight', [((0, 0, 0.06), (0.10, 0.10, 0.06), 0.3, 0.3)])])
        obj('balloon', [('balloon', [((0, 0, 0), (B_RX, B_RX, B_RZ), 1.0, 1.0),
                                     ((0, 0, -B_RZ - 0.01), (0.05, 0.05, 0.04), 1.0, 1.0)])])
        for n in ('cord_lo', 'cord_hi'): obj(n, [('cord', [((0, 0, 0), (1.0, 1.0, 0.5), 0.15, 1.0, dict(segs=12, rings=8))])])
    home = {'ball': lambda: {'ball': _ball_home()}, 'bubbles': _bubbles_home, 'yoyo': _yoyo_home, 'blocks': _blocks_home,
            'balloon': _balloon_home}[s]()
    anchor_obj = {'ball': 'ball', 'bubbles': 'bottle', 'yoyo': 'yoyo', 'blocks': 'blk0', 'balloon': 'weight'}[s]
    p = home[anchor_obj][1][1]
    return {s: dict(px=_px_of(g['ppu'], p[0], p[1]), pos=[p[0], p[1], 0.0])}


def _px_of(ppu, x, y):
    import stagekit
    return [round(v) for v in stagekit.px_of(ppu, x, y, 0.0)]


def prop_state(s, st):
    """place every toy object of this frame (render_set.py has already posed the rig), return {layer: key}."""
    import bpy
    from mathutils import Matrix, Vector, Euler
    W0 = _REG['W0']; Wm = W0.matrix_world.copy()
    rig = bpy.data.objects['Rig']; pb = rig.pose.bones
    Rroot = (rig.matrix_world @ pb['root'].matrix @ rig.data.bones['root'].matrix_local.inverted()).to_3x3()
    Rroot.normalize()
    W3 = Wm.to_3x3()
    placed = {}

    def tip(side): return rig.matrix_world @ pb['paw_' + side].head

    def pt(p):
        k = p[0]
        if k == 'f': return Wm @ Vector(p[1])
        if k == 'h':
            base = (tip('L') + tip('R')) / 2 if p[1] == 'both' else tip(p[1])
            fwd, left, up = p[2]
            return base + Rroot @ Vector((left, -fwd, up))
        if k == 'm': a, b = pt(p[1]), pt(p[2]); return a.lerp(b, p[3])
        if k == 'a': return pt(p[1]) + W3 @ Vector(p[2])
        if k == 'z': v = pt(p[1]); return Vector((v.x, v.y, Wm.translation.z + p[2]))
        if k == 'o': return placed[p[1]] @ Vector(p[2])
        raise ValueError(p)
    for name in LAYER_OBJS[s]:
        sp = st.get(name, HIDE); e = _REG[name]
        if sp[0] == 'x':
            m = Wm @ Matrix.Translation((0, 0, 0.05)) @ Matrix.Diagonal((1e-6, 1e-6, 1e-6, 1))
        elif sp[0] == 'p':
            rot = Euler([math.radians(a) for a in sp[2]], 'XYZ').to_matrix().to_4x4()
            m = Matrix.Translation(pt(sp[1])) @ (Matrix(W3).to_4x4() @ rot) @ Matrix.Diagonal((*sp[3], 1))
        else:
            a, b = pt(sp[1]), pt(sp[2]); d = b - a; L = max(d.length, 1e-4); r = sp[3]
            q = d.normalized().to_track_quat('Z', 'Y').to_matrix().to_4x4()
            m = Matrix.Translation((a + b) / 2) @ q @ Matrix.Diagonal((r, r, L, 1))
        e.matrix_world = m; placed[name] = m
    bpy.context.view_layer.update()
    h = hashlib.md5()
    for name in LAYER_OBJS[s]:
        h.update(','.join(f'{v:.5f}' for row in placed[name] for v in row).encode())
    return {s: f'{s}_{h.hexdigest()[:12]}'}
