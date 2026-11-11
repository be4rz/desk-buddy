"""Stage D clips: the desk station (desk_sit, desk_type, desk_swivel_in, desk_talk, desk_ask, desk_swivel_out,
desk_stand). Pure Python: render_d.py poses the Stage L rig with them, face_d.py and verify_d.py read the poses.

Every frame is (q, st):
  q   bear pose = the Stage L parameters (anim_clips.REST_L: Stage C rules, hy head yaw, hb head bob) plus
        X, Y, Z   root position in the SET frame (desk footprint centre on the floor), world units
        yaw       body facing, deg, Stage A convention (0 = SW/-Y, 45 = S, 90 = SE/+X, 180 = NE/+Y)
  st  prop state: chair = the chair's facing (same convention), scr = screen glow tone (0 / 1)
Stage C rules hold on every frame: no squash, roll or lean; bob and hb in whole pixels; the eyes stay open.
The poses are written for ori A; render_d.py mirrors the set for ori B (a true 3D mirror, as Stage B) and gives the
bear the mirrored POSE (mirror_pose below), so every ori B bear pixel is a real render of the real bear.

Choreography (ori A). The chair's home faces SE; the hand-off spot is SE of it, the bear facing SE, back to the seat.
  desk_sit        two waddle steps backwards, crouch, hop up onto the seat, plop and a 1 px settle, then the chair
                  swivels 90 deg to the desk (NE) and the stubby paws come up onto the keys.
  desk_type       loop: alternating paw pats on the keys, an ear twitch; the screen flickers between two tones.
  desk_swivel_in  the chair swivels back to SE; the head adds the Stage L -45 deg whole-head turn, so it faces S
                  (face patch dir S, exactly as listen SE).
  desk_talk       loop: seated, body SE, head S, paws low; whole-pixel nods (hb), ear and clover wiggles.
  desk_ask        loop: as talk, one paw up and out to the side (the far, left paw: never across the face).
  desk_swivel_out the reverse of desk_swivel_in.
  desk_stand      head back, hop down forwards, two waddle steps to the hand-off spot.
Joins (identical frames, checked by verify_d.py):
  desk_sit last = desk_type f0 = desk_swivel_in f0 = desk_swivel_out last  (TYPE0)
  desk_swivel_in last = desk_talk f0 = desk_ask f0 = desk_swivel_out f0 = desk_stand f0  (TALK0)
"""
from anim_clips import REST_L

FPS = 12
REST_D = dict(REST_L, X=0.0, Y=0.0, Z=0.0, yaw=0.0)
ORDER = ['desk_sit', 'desk_type', 'desk_swivel_in', 'desk_talk', 'desk_ask', 'desk_swivel_out', 'desk_stand']
COUNTS = dict(desk_sit=20, desk_type=16, desk_swivel_in=6, desk_talk=20, desk_ask=20, desk_swivel_out=6, desk_stand=16)
LOOP = {'desk_type', 'desk_talk', 'desk_ask'}
FACE_CLIPS = ['desk_talk', 'desk_ask']          # face layers (face.json set_clips), head turned to S
PATCH_DIR = 'S'                                 # their face patch direction (body SE / SW, head turned 45 deg)
HAND_YAW = 90.0                                 # hand-off facing, ori A (SE); ori B: 90 - yaw = 0 (SW)
TYPE_YAW, TALK_YAW, TALK_HY = 180.0, 90.0, -45.0
TYPE_ARM = 30.0                                 # forward paw raise onto the keys (the paws are short)


def lerp(a, b, t): return a + (b - a) * t


def mirror_pose(q, st):
    """ori B: the 3D mirror (x, y) -> (-y, -x) of an ori A frame, as a pose of the real (unmirrored) bear."""
    m = dict(q)
    for a, b in (('armL', 'armR'), ('spL', 'spR'), ('fL', 'fR'), ('hL', 'hR'), ('earL', 'earR')):
        m[a], m[b] = q[b], q[a]
    m['X'], m['Y'] = -q['Y'], -q['X']
    m['yaw'] = (90.0 - q['yaw']) % 360
    m['hy'] = -q['hy']; m['clY'] = -q['clY']; m['ty'] = -q['ty']
    s = dict(st, chair=(90.0 - st['chair']) % 360)
    return m, s


def build(g):
    """g (render_d.py): hand = hand-off spot, seat = the snapped seated root (on the chair's swivel axis),
    seatZ, PXZ."""
    PXZ = g['PXZ']; hx, hy = g['hand']; sx, sy = g['seat']; Zs = g['seatZ']
    seat = dict(X=sx, Y=sy, Z=Zs, fL=0.30, fR=0.30, hL=0.12, hR=0.12)        # plush legs dangling forward
    clips = {}

    def F(st=None, **kw):
        q = dict(REST_D, X=hx, Y=hy, yaw=HAND_YAW); q.update(kw)
        s = dict(chair=HAND_YAW, scr=0); s.update(st or {})
        return (q, s)

    def C(yaw, scr=0, **kw):            # seated, the chair (and the bear on it) swivelled to yaw
        return F(dict(chair=yaw, scr=scr), **dict(seat, yaw=yaw, **kw))

    def V(base, scr=None, **kw):        # variation of a held frame
        q, s = base; s = dict(s) if scr is None else dict(s, scr=scr)
        return (dict(q, **kw), s)

    TYPE0 = C(TYPE_YAW, armL=TYPE_ARM, armR=TYPE_ARM)
    TALK0 = C(TALK_YAW, hy=TALK_HY)

    # ------------------------------------------------------------------ desk_sit (20)
    bx = hx - 0.30                                                # after two steps back
    clips['desk_sit'] = [
        F(),                                                                      # rest at the hand-off spot
        F(X=hx - 0.05, fR=-0.12, hR=0.08, earL=1, earR=1, clX=2),                 # step back (right foot)
        F(X=hx - 0.12, fR=-0.05, bob=PXZ, earL=-1, earR=-1, clX=-1),
        F(X=hx - 0.20, fL=-0.12, hL=0.08, earL=1, earR=1, clX=2),                 # step back (left foot)
        F(X=hx - 0.27, fL=-0.05, bob=PXZ, earL=-1, earR=-1),
        F(X=bx, earL=2, earR=2, clY=3),                                           # a peek at the seat
        F(X=bx, bob=-2 * PXZ, armL=-12, armR=-12, earL=3, earR=3, clX=4),          # crouch
        F(X=lerp(bx, sx, 0.45), Z=Zs + 0.22, fL=0.12, fR=0.12, hL=0.10, hR=0.10,
          armL=30, armR=30, earL=-4, earR=-4, clX=-6),                            # hop up, backwards
        F(X=lerp(bx, sx, 0.85), Z=Zs + 0.12, fL=0.25, fR=0.25, hL=0.12, hR=0.12,
          armL=20, armR=20, earL=-3, earR=-3, clX=-4),
        C(HAND_YAW, bob=-PXZ, earL=4, earR=4, clX=7),                             # plop
        C(HAND_YAW, earL=1, earR=1, clX=-2),                                      # 1 px settle
        C(HAND_YAW, earL=-1, earR=-1, clY=3),                                     # happy wiggle
        C(102.0, armL=4, armR=4, earL=1, earR=1, clY=-3),                         # swivel to the desk
        C(118.0, armL=8, armR=8, earL=2, earR=2, clY=-4),
        C(135.0, armL=12, armR=12, earL=2, earR=2, clY=-3),
        C(152.0, armL=16, armR=16, earL=1, earR=1, clY=-1),
        C(166.0, armL=22, armR=22, clY=2),
        C(TYPE_YAW, armL=34, armR=34, earL=-1, earR=-1, clX=-2),                  # paws up
        C(TYPE_YAW, armL=32, armR=32, clX=1),
        TYPE0,
    ]

    # ------------------------------------------------------------------ desk_type (16, loop)
    L = dict(armL=TYPE_ARM + 7, armR=TYPE_ARM - 3); R = dict(armL=TYPE_ARM - 3, armR=TYPE_ARM + 7)
    clips['desk_type'] = [
        TYPE0,
        V(TYPE0, **L), V(TYPE0, 1, **L), V(TYPE0, 1, **R), V(TYPE0, **R),
        V(TYPE0, **L), V(TYPE0, 1, **R), V(TYPE0, 1, **L), V(TYPE0, **R),
        V(TYPE0, earL=-3, earR=1, clY=-3), V(TYPE0, 1, earL=2, earR=-1, clY=2),   # ear twitch
        V(TYPE0, 1, **L), V(TYPE0, **R), V(TYPE0, **L), V(TYPE0, 1, **R),
        V(TYPE0, clX=-1),
    ]

    # ------------------------------------------------------------------ desk_swivel_in (6)
    clips['desk_swivel_in'] = [
        TYPE0,
        C(158.0, armL=18, armR=18, earL=1, earR=1, clY=2),
        C(130.0, armL=8, armR=8, earL=2, earR=2, clY=3),
        C(106.0, earL=1, earR=1, clY=1),
        C(TALK_YAW, hy=-25.0, earL=-1, earR=-1, clX=-2),
        TALK0,
    ]

    # ------------------------------------------------------------------ desk_talk (20, loop)
    nod = dict(hb=-1, earL=2, earR=2, clX=3)
    gl = dict(armL=16, spL=12, earL=-1, earR=1, clY=2)
    clips['desk_talk'] = [
        TALK0, TALK0, V(TALK0, **nod), V(TALK0, **nod), TALK0, V(TALK0, earL=-2, earR=-2, clX=-2),
        V(TALK0, armL=8, spL=6), V(TALK0, **gl), V(TALK0, **gl), V(TALK0, armL=8, spL=6), TALK0,
        V(TALK0, earL=2, earR=-1, clY=3), V(TALK0, earL=-1, earR=2, clY=-3), TALK0,
        V(TALK0, **nod), V(TALK0, **nod), TALK0, V(TALK0, hb=1, earL=-2, earR=-2, clX=-3),
        V(TALK0, hb=1, earL=-1, earR=-1), TALK0,
    ]

    # ------------------------------------------------------------------ desk_ask (20, loop)
    up = dict(armL=24, spL=40, earL=-3, earR=-3, clX=-2)
    up2 = dict(up, spL=46, clX=2)
    clips['desk_ask'] = [
        TALK0, V(TALK0, armL=10, spL=16, earL=-1, earR=-1), V(TALK0, **up), V(TALK0, **up), V(TALK0, **up2),
        V(TALK0, **up), V(TALK0, **up2), V(TALK0, **up), V(TALK0, **dict(up, hb=1)), V(TALK0, **dict(up, hb=1)),
        V(TALK0, **up), V(TALK0, **up2), V(TALK0, **up), V(TALK0, **up2), V(TALK0, **up),
        V(TALK0, **dict(up, earL=-1, earR=-4, clY=3)), V(TALK0, **dict(up, earL=-4, earR=-1, clY=-3)),
        V(TALK0, armL=10, spL=16, earL=-1, earR=-1), TALK0, TALK0,
    ]

    # ------------------------------------------------------------------ desk_swivel_out (6)
    clips['desk_swivel_out'] = [
        TALK0,
        C(TALK_YAW, hy=-25.0, earL=-1, earR=-1, clX=-2),
        C(106.0, earL=1, earR=1, clY=1),
        C(130.0, armL=8, armR=8, earL=2, earR=2, clY=3),
        C(158.0, armL=18, armR=18, earL=1, earR=1, clY=2),
        TYPE0,
    ]

    # ------------------------------------------------------------------ desk_stand (16)
    fx = hx - 0.30
    clips['desk_stand'] = [
        TALK0,
        C(TALK_YAW, hy=-25.0, earL=1, earR=1),
        C(TALK_YAW, earL=2, earR=2, clY=2),                                       # head back to the body
        C(TALK_YAW, armL=-10, armR=-10, bob=-PXZ, earL=3, earR=3, clX=4),         # get ready
        F(X=lerp(sx, fx, 0.40), Z=Zs + 0.14, fL=0.18, fR=0.18, hL=0.10, hR=0.10,
          armL=28, armR=28, earL=-4, earR=-4, clX=-6),                            # hop down, forwards
        F(X=lerp(sx, fx, 0.80), Z=Zs * 0.40, fL=0.06, fR=0.06, hL=0.06, hR=0.06,
          armL=20, armR=20, earL=-3, earR=-3, clX=-4),
        F(X=fx, bob=-2 * PXZ, armL=-8, armR=-8, earL=4, earR=4, clX=7),           # land
        F(X=fx, earL=-1, earR=-1, clX=-2),
        F(X=fx + 0.05, fL=0.12, hL=0.08, earL=1, earR=1, clX=2),                  # step (left foot)
        F(X=fx + 0.12, fL=0.05, bob=PXZ, earL=-1, earR=-1, clX=-1),
        F(X=fx + 0.20, fR=0.12, hR=0.08, earL=1, earR=1, clX=2),                  # step (right foot)
        F(X=fx + 0.27, fR=0.05, bob=PXZ, earL=-1, earR=-1),
        F(earL=2, earR=2, clY=3),
        F(earL=-1, earR=-1, clY=-2),
        F(),
        F(),                                                                      # rest at the hand-off spot
    ]
    for c, n in COUNTS.items(): assert len(clips[c]) == n, (c, len(clips[c]), n)
    return clips


def handoff(g):
    """hand-off spot (set frame) and facing of the two transition clips (ori A)."""
    return {'desk_sit': dict(at='start', pos=list(g['hand']), yaw=HAND_YAW),
            'desk_stand': dict(at='end', pos=list(g['hand']), yaw=HAND_YAW)}
