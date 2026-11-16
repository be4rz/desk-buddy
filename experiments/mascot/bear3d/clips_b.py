"""Stage B clips: bed (get_in_bed, sleep, wake_up) and reading (sit_down, read, stand_up). Pure Python.

Every frame is (q, st):
  q   bear pose = the Stage A parameters (anim_clips.REST) plus
        X, Y, Z   root position in the SET frame (prop origin on the floor), world units
        yaw       facing, deg, Stage A convention (0 = SW/-Y, 45 = S, 90 = SE/+X)
        lie       deg, lying back about the bear's back-bottom (PIV_LIE in render_b.py)
        look      face decals shifted down by whole pixels (looking down)
        spL/spR   arm spread outwards, deg (applied before the forward swing)
        legL/legR hip swing, deg, + = forward (seated, lying and hop frames; the foot pads ride the legs)
  st  prop state: quilt (folds = arc positions of its fold bends, >= Lq = none; cut = bear rest height that
      lifts the quilt, -9 = none; arms = arms under it; br = breath px), book (cover 0 closed .. 180 open, leaf -1 hidden or 0..180 page turn), zzz phase (-1 none)
12 fps. Transition clips start or end on the Stage A rest pose at the hand-off spot (pixel-identical there).
Following the Stage A convention, a clip that leads INTO a loop ends on the frame before the loop's frame 0,
and a clip that leads OUT of a loop starts with the loop's frame 0 pose.
"""
import math
from anim_clips import REST, ease

FPS = 12
REST_B = dict(REST, X=0.0, Y=0.0, Z=0.0, yaw=0.0, lie=0.0, look=0, spL=0.0, spR=0.0, legL=0.0, legR=0.0)
# hip swings (legL / legR, deg, + = forward; render_b.py adds the hip joints): seated, the real legs swing out
# and the foot pads ride them (fL / hL stay 0 on every swung frame; standing frames keep the Stage A pads)
SIT_LEG = 75.0    # sitting up in bed: legs out along the mattress
EDGE_LEG = 70.0   # perched on the bed edge: legs out over the side
LIE_LEG = -10.0   # lying back: legs eased down toward the mattress
SEAT_LEG = 65.0   # on the pouf: legs out over its front edge (short of the table)
ORDER = {'bed': ['get_in_bed', 'sleep', 'wake_up'], 'read': ['sit_down', 'read', 'stand_up']}
LOOP = {'sleep', 'read'}


def lerp(a, b, t): return a + (b - a) * t


def build(g):
    """g: geometry from render_b.py (hand-off spots, seat/lying placement, quilt lengths), plus PXZ."""
    PXZ = g['PXZ']
    def szpx(n): return 1.0 + n * PXZ / 2.06
    clips = {}

    # ================================================================== bed
    hx, hy = g['bed_hand']; yS = g['bed_sitY']; yL = g['bed_lieY']; zS = g['bed_sitZ']; ex = g['bed_edgeX']; LIE = g['lie']
    Lq = g['Lq']
    QM = dict(folds=[Lq - 0.42, Lq + 1], cut=-9, arms=0, br=0)   # bed made: turned-down band at the pillow
    QO = dict(QM, folds=list(g['pile']), cut=0.62)               # pushed down: a soft 3-layer pile at the foot
    QF = dict(QM, folds=[Lq, Lq + 1], cut=1.15, arms=1)                  # pulled up to the chin, paws on top
    QL = QO                                                      # sitting up in bed: the pile is the lap quilt
    QE = dict(QO, cut=-9)                                        # perched on the edge: the pile lies flat
    def B(st=None, **kw):
        q = dict(REST_B, X=hx, Y=hy, Z=0.0, yaw=0.0); q.update(kw)
        return (q, dict(dict(QM, cover=0, leaf=-1, zzz=-1), **(st or {})))
    def Q(base, **kw): return dict(base, **kw)
    def legs(a, d=0.0): return dict(legL=a + d, legR=a - d)
    sit = dict(X=0.0, Y=yS, Z=zS, **legs(SIT_LEG))
    edge = dict(sit, X=ex, Y=hy, **legs(EDGE_LEG))               # perched on the bed edge, legs over the side
    lie = dict(sit, Y=yL, lie=LIE, **legs(LIE_LEG))
    def lb(t): return dict(sit, lie=LIE * t, Y=lerp(yS, yL, t), **legs(lerp(SIT_LEG, LIE_LEG, t)))   # lying back
    def mid(a, b, t):
        return Q(a, folds=[lerp(x, y, t) for x, y in zip(a['folds'], b['folds'])], cut=max(a['cut'], b['cut']))

    gib = [
        B(),                                                                     # rest at the hand-off spot
        B(sz=szpx(-1), spR=25, armR=10, roll=-3, earR=2),                        # reach for the quilt (bed on the right)
        B(spR=55, armR=20, roll=-6, earL=-2, earR=3, clY=-3),                    # grab the turned-down edge
        B(mid(QM, QO, 0.55), spR=25, armR=-15, roll=3, sz=szpx(1), clY=4),   # yank it back
        B(QO, sz=szpx(-2), spR=5, earL=2, earR=2, clY=-2),                       # quilt folded to the foot half
        B(QO, yaw=45.0),                                                          # turn: back to the bed
        B(QO, yaw=90.0, sz=szpx(-3), armL=-10, armR=-10, earL=2, earR=2),        # crouch
        B(QE, yaw=90.0, X=lerp(hx, ex, 0.5), Z=0.40, sz=szpx(2), armL=25, armR=25, **legs(30),
          earL=-4, earR=-4, clX=-6),                                              # hop up and back
        B(QE, yaw=90.0, **dict(edge, **legs(EDGE_LEG + 10)), sz=szpx(-3), armL=10, armR=10, earL=3, earR=3, clX=6),   # plop on the edge
        B(QE, yaw=90.0, **dict(edge, **legs(EDGE_LEG - 6)), sz=szpx(1), clX=-3),
        B(QO, yaw=55.0, **dict(sit, X=lerp(ex, 0, 0.5), Y=lerp(hy, yS, 0.5))),   # swing the legs in, scoot down
        B(QO, yaw=20.0, **dict(sit, X=lerp(ex, 0, 0.85), Y=lerp(hy, yS, 0.85))),
        B(QO, yaw=0.0, **sit, earL=3, earR=3, clY=3),                            # sitting in bed, facing the foot
        B(QO, yaw=0.0, **dict(sit, **legs(SIT_LEG, 8)), sz=szpx(1), earL=-1, earR=-1, clY=-2),   # happy wiggle
        B(QO, **lb(0.30), armL=-8, armR=-8, clX=-4),                             # lie back
        B(QO, **lb(0.70), armL=-4, armR=-4, clX=-6),
        B(QO, **lie, sz=szpx(-2), earL=3, earR=3, clX=5),                        # plop onto the pillow
        B(QO, **lie, sz=szpx(1), clX=-2),
        B(mid(QO, QF, 0.45), **lie, armL=25, armR=25),                           # pull the quilt up
        B(Q(mid(QO, QF, 0.80), arms=0), **lie, armL=48, armR=48),
        B(QF, **lie, armL=26, armR=26, blink=0.45),                               # tuck the paws in
        B(QF, **lie, armL=12, armR=12, sz=szpx(1), blink=0.12, earL=-2, earR=-2),   # snuggle; eyes shut
    ]
    clips['get_in_bed'] = gib

    # sleep loop: slow breathing (quilt rises 1-2 px), Zzz drifting up, a sleepy ear twitch
    slp_q = dict(lie, armL=12, armR=12, blink=0.12)
    BR = [0] * 6 + [1] * 4 + [2] * 10 + [1] * 4 + [0] * 12          # 36 frames, 3 s
    sleep = []
    for f in range(36):
        kw = dict(slp_q)
        if 27 <= f <= 28: kw.update(earL=-6 if f == 27 else -3)       # ear twitch
        if BR[f] == 2: kw.update(clX=-2)
        sleep.append(B(Q(QF, br=BR[f], zzz=f), **kw))
    clips['sleep'] = sleep

    wak = [
        B(Q(QF, zzz=0), **slp_q),                                                # sleep frame 0 (Zzz drawn there too)
        B(QF, **dict(slp_q, blink=0.45)),                                        # eyes open
        B(QF, **dict(lie, armL=12, armR=12, earL=-4, earR=-4, clX=-4)),          # ears perk
        B(mid(QF, QL, 0.5), **lb(0.6), armL=30, armR=30),                        # sit up, quilt slides down
        B(QL, **lb(0.25), armL=15, armR=15),
        B(QL, **sit, sz=szpx(-2), earL=3, earR=3, clX=5),                        # sitting up
        B(QL, **sit, armL=100, armR=100, spL=30, spR=30, sz=szpx(2), blink=0.12),  # big stretch
        B(QL, **sit, armL=150, armR=150, spL=55, spR=55, sz=szpx(4), blink=0.12, earL=-4, earR=-4, clX=-5),
        B(QL, **sit, armL=155, armR=155, spL=60, spR=60, sz=szpx(4), blink=0.12, roll=3, earL=-4, earR=-2, clY=4),
        B(QL, **sit, armL=155, armR=155, spL=60, spR=60, sz=szpx(4), blink=0.12, roll=-3, earL=-2, earR=-4, clY=-4),
        B(QL, **sit, armL=60, armR=60, spL=8, spR=8, sz=szpx(-2), blink=0.45, clX=4),   # relax
        B(QL, **sit, earL=2, earR=2),
        B(QO, **sit, armL=-10, armR=-10, sz=szpx(-1)),                            # push the quilt off
        B(QO, yaw=45.0, **dict(sit, X=lerp(0, ex, 0.5), Y=lerp(yS, hy, 0.5))),   # swing the legs out
        B(QE, yaw=90.0, **edge),
        B(QE, yaw=90.0, **dict(edge, **legs(EDGE_LEG - 8)), sz=szpx(-2), armL=-10, armR=-10, lean=6),   # scoot, lean forward
        B(QE, yaw=90.0, X=lerp(ex, hx, 0.55), Z=0.30, sz=szpx(2), armL=30, armR=30, **legs(40),
          earL=-4, earR=-4, clX=-6),                                              # hop off
        B(QO, yaw=90.0, sz=szpx(-3), armL=8, armR=8, earL=3, earR=3, clX=6),      # land
        B(QO, yaw=90.0, sz=szpx(1), clX=-3),
        B(QO, yaw=45.0),                                                          # turn back to the bed
        B(QO),
        B(QO, spR=50, armR=15, roll=-6, earR=3, clY=-3),                          # tidy: grab the quilt
        B(mid(QO, QM, 0.6), spR=30, armR=40, roll=3, sz=szpx(1), clY=4),   # flip it up
        B(QM, sz=szpx(-2), spR=5, earL=2, earR=2),                                # pat
        B(),                                                                      # rest at the hand-off spot
    ]
    clips['wake_up'] = wak

    # ================================================================== reading set (table + pouf + book)
    px_, py_ = g['pouf']; zP = g['seatZ']; rx_, ry_ = g['read_hand']
    def R(st=None, **kw):
        q = dict(REST_B, X=rx_, Y=ry_, Z=0.0, yaw=90.0); q.update(kw)
        return (q, dict(dict(QM, cover=0, leaf=-1, zzz=-1), **(st or {})))
    seat = dict(X=px_, Y=py_, Z=zP, **legs(SEAT_LEG))
    rd = dict(seat, lean=7, armL=24, armR=24, look=1, blink=0.75)
    O = dict(cover=180)
    sdn = [
        R(),
        R(sz=szpx(-3), armL=-10, armR=-10, earL=2, earR=2),                       # crouch
        R(Y=lerp(ry_, py_, 0.40), Z=0.36, sz=szpx(2), armL=25, armR=25, **legs(30),
          earL=-4, earR=-4, clX=-6, clY=4),                                         # hop sideways
        R(Y=lerp(ry_, py_, 0.80), Z=zP + 0.20, armL=15, armR=15, **legs(50), clX=-3, clY=5),
        R(**dict(seat, **legs(SEAT_LEG + 10)), sz=szpx(-3), earL=3, earR=3, clX=6, clY=-2),   # plop on the pouf
        R(**dict(seat, **legs(SEAT_LEG - 5)), sz=szpx(1), clX=-3),
        R(**seat, earL=-1, earR=-1),
        R(**dict(seat, lean=5, armL=18, armR=18)),                                # reach for the book
        R(**dict(seat, lean=7, armL=22, armR=30, spR=-6)),
        R(dict(cover=70), **dict(seat, lean=7, armL=22, armR=55, spR=-14, look=1)),   # open the cover
        R(dict(cover=135), **dict(seat, lean=7, armL=22, armR=38, spR=-8, look=1)),
        R(O, **dict(rd, blink=1.0, earL=2, earR=2)),
    ]
    clips['sit_down'] = sdn

    read = []
    for f in range(48):
        kw = dict(rd); st = dict(O)
        if f in (8, 10, 41, 43): kw['blink'] = 0.4
        if f in (9, 42): kw['blink'] = 0.12
        if 17 <= f <= 22:                                                         # page turn
            k = f - 17
            kw.update(armR=[30, 40, 50, 46, 36, 28][k], spR=[-4, -10, -16, -14, -8, -3][k])
            st['leaf'] = [0, 0, 55, 110, 160, 180][k] if k < 5 else -1
        if 30 <= f <= 35:                                                         # happy wiggle
            k = f - 30
            kw.update(blink=[0.12, 0.12, 0.12, 0.12, 0.45, 0.75][k], look=[1, 0, 0, 0, 1, 1][k],
                      bob=[1, 2, 1, 0, 0, 0][k] * PXZ, sz=szpx([1, 2, 0, -1, 0, 0][k]),
                      armL=[28, 34, 30, 24, 24, 24][k], armR=[28, 34, 30, 24, 24, 24][k],
                      earL=[3, 5, -2, 1, 0, 0][k], earR=[3, 5, -2, 1, 0, 0][k],
                      clX=[-3, -6, 5, -2, 1, 0][k], clY=[3, -4, 3, -1, 0, 0][k])
            kick = [6, 10, 4, 0, 0, 0][k]                                         # a little leg kick
            kw.update(legL=SEAT_LEG + kick, legR=SEAT_LEG - kick)
        read.append(R(st, **kw))
    clips['read'] = read

    sup = [
        R(O, **rd),
        R(dict(cover=120), **dict(rd, armR=50, spR=-14, blink=1.0)),               # close the book
        R(dict(cover=50), **dict(rd, armR=42, spR=-10, blink=1.0)),
        R(**dict(seat, lean=4, armL=18, armR=20)),
        R(**seat, earL=2, earR=2),
        R(**dict(seat, **legs(SEAT_LEG - 10)), sz=szpx(-2), armL=-10, armR=-10),   # push off
        R(Y=lerp(py_, ry_, 0.30), Z=zP + 0.22, sz=szpx(2), armL=25, armR=25, **legs(50),
          earL=-4, earR=-4, clX=-6, clY=-4),
        R(Y=lerp(py_, ry_, 0.72), Z=0.25, armL=15, armR=15, **legs(30), clX=-3, clY=-5),
        R(sz=szpx(-3), armL=8, armR=8, earL=3, earR=3, clX=6, clY=2),             # land
        R(sz=szpx(1), clX=-3),
        R(),
    ]
    clips['stand_up'] = sup
    return clips


def handoff(g):
    """hand-off spots (set frame) and facing of each transition clip."""
    return {'get_in_bed': dict(at='start', pos=g['bed_hand'], yaw=0.0),
            'wake_up': dict(at='end', pos=g['bed_hand'], yaw=0.0),
            'sit_down': dict(at='start', pos=g['read_hand'], yaw=90.0),
            'stand_up': dict(at='end', pos=g['read_hand'], yaw=90.0)}
