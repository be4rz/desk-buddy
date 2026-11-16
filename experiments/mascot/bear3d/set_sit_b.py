"""Stage sit_b (set module of the stage kit, stages/sit_b.json): four prop sets, storyboard rows 74-77 (#sb-props-sit-b).

  notebook  note_sit 12 -> note_doodle 32 (loop, face layers) -> note_stand 12    open sketchbook + a fat toy crayon on a low toy tray-table
  laptop    laptop_sit 12 -> laptop_type 24 (loop, face layers) -> laptop_stand 12  a chunky toy laptop on a low toy tray-table
  phones    phones_on 14 -> phones_vibe 32 (loop, baked) -> phones_off 14         headphones on a little wooden stand
  umbrella  umbrella_open 14 -> umbrella_rain 32 (loop, baked) -> umbrella_close 14  umbrella in a bucket + baked rain

Pure Python at import (rig_bear.py keys build(), make_set.py / verify_sit_b.py read the tables); bpy is imported only
inside build_props / prop_state, which run in Blender (render_set.py). Set contract (CLAUDE.md "Stage kit"): SETS,
PARTS, PROPS, geometry, build, handoff, build_props, prop_state.

Every frame is (q, st): q = the Stage K pose (anim_clips.REST_K) + the set placement X, Y, Z, yaw (Stage A convention,
0 = SW, 90 = SE); st = the prop state of that frame (JSON). Ori A (written here): the bear faces SE (+X), its left is
+Y (NE on screen, behind it). Ori B = the kit's 3D mirror (anim_clips.mirror_k; st is copied unchanged).

MOVING PROPS (the lead's p5 decision, no kit hook): the kit builds set props once as static meshes under the set root
(render_set.py `W0`, mirrored for ori B). This module ALSO builds moving objects (crayon, headphones, umbrella, rain
drops) with no parent, and places them itself in prop_state(). That relies on render_set.py calling
MOD.prop_state(set, st) for every frame AFTER posing the rig for that frame (set_state: use(clip, f) then prop_state)
and BEFORE any render or extent pass of it (layout, props and clip modes all go through set_state). prop_state reads
the posed paw / head bones, writes each moving object's matrix_world, and returns per layer a key = layer + md5 of the
placed matrices (static layers: '<layer>_0'), so render_set's per-key cache renders each distinct placement once.
Ori B is detected from the set root's mirror (det < 0): set placements become M @ Q @ S (M the set mirror, S a local
x flip so the matrix stays proper; every moving prop is symmetric in its local x), and the bear's paw side swaps
(the B pose is mirrored, so the A right paw is the B left paw). Hidden objects (drops not falling) are scaled to 1e-6
inside a visible prop, so no layer is ever empty and the layout extents never grow. verify_sit_b.py proves the result
(hand-offs = home props + idle#0, joins exact, static props never move, a moved hand-off prop is caught).
A future kit hook should take over: (1) per-frame placement of non-static set objects from st (set-space or bone
attached: paw_L/paw_R/head, with the ori B mirror and paw swap), (2) hidden-object handling, (3) the placement key.

Choreography (ori A):
  (notebook / laptop: the props stand on a low toy tray-table, lead decision q-183110: seated, the stub mittens
            reach z ~0.85-1.0 only, so nothing on the floor can be touched)
  notebook  side shuffle from the hand-off spot H to the sit spot P (the bear's left), the shared plush floor sit (SIT_LEG / SIT_BOB),
            the right mitten picks the crayon off the page; doodle loop: small scribbles aimed at the page, breaths,
            ear wiggles and small leg swings, the face upright (set_clips layers, patch SE / SW); then the crayon goes back, stand,
            shuffle back to H.
  laptop    the same approach; laptop_type: alternating mitten taps on the keys, a breath, ear wiggles (layers).
  phones    standing at H: the left mitten takes the headphones off the stand, both mittens pop them on over the beanie;
            phones_vibe: head bobs to the beat (hp / hr), eyes closed, swaying arms, foot taps (baked); phones_off back.
  umbrella  standing at H: it starts to rain (drops from open frame 5), the left mitten lifts the umbrella out of the
            bucket, twirls it upright overhead and it pops open; umbrella_rain: swaying under the rain, the right mitten
            pokes out to catch drops (baked); umbrella_close: the rain stops (no new drops), it closes, twirls back down
            into the bucket. Rain time tau: open f -> f - 13, rain f -> f, close f -> f; drops fall in 8 frames, so
            open[13] = rain[0] = close[0] and rain's seam is periodic (32 = 4 x 8).
Joins (identical frames): <sit>[-1] = <loop>[0] = <stand>[0] per set; hand-offs: <sit>[0] and <stand>[-1] = props +
idle#0 at H facing SE (A) / SW (B).
"""
import math, hashlib
from anim_clips import REST_K, expand_k, SIT_LEG, SIT_BOB

REST_S = dict(REST_K, X=0.0, Y=0.0, Z=0.0, yaw=0.0)
HAND_YAW = 90.0
_ORI_NOTE = {'A': 'the bear faces SE, the props in front (SE) or on its left (NE on screen)',
             'B': 'mirror of A: the bear faces SW, the props in front (SW) or on its right (NW on screen)'}


def _set(clips, counts, face, note):
    return dict(clips=clips, counts=dict(zip(clips, counts)), loop=[clips[1]],
                joins=[(clips[0], -1, clips[1], 0), (clips[2], 0, clips[1], 0)],
                handoff={clips[0]: 'start', clips[2]: 'end'}, hand_yaw=HAND_YAW, face=face,
                patch={'A': 'SE', 'B': 'SW'}, ori_note=dict(_ORI_NOTE), note=note,
                next={clips[0]: clips[1], clips[1]: f'{clips[1]} or {clips[2]}',
                      clips[2]: 'Stage A idle (rest pose, same spot and facing)'})


SETS = {
    'notebook': _set(['note_sit', 'note_doodle', 'note_stand'], [12, 32, 12], ['note_doodle'],
                     'Set notebook: note_sit -> note_doodle (loop, face layers) -> note_stand; an open sketchbook on a low '
                     'wooden toy tray-table in front of the floor-sitting bear and a fat toy crayon (on the page at the hand-offs, in the right mitten while doodling).'),
    'laptop': _set(['laptop_sit', 'laptop_type', 'laptop_stand'], [12, 24, 12], ['laptop_type'],
                   'Set laptop: laptop_sit -> laptop_type (loop, face layers) -> laptop_stand; a chunky toy laptop on '
                   'a low wooden toy tray-table in front of the floor-sitting bear, lid always open.'),
    'phones': _set(['phones_on', 'phones_vibe', 'phones_off'], [14, 32, 14], [],
                   'Set phones: phones_on -> phones_vibe (loop, baked) -> phones_off; headphones on a little wooden '
                   'stand on the bear\'s left at the hand-offs, on its head in phones_vibe.'),
    'umbrella': _set(['umbrella_open', 'umbrella_rain', 'umbrella_close'], [14, 32, 14], [],
                     'Set umbrella: umbrella_open -> umbrella_rain (loop, baked) -> umbrella_close; a closed umbrella in '
                     'a bucket on the bear\'s left at the hand-offs, open overhead in the rain loop; rain drops are baked '
                     'into the frames (none on the hand-off frames).'),
}

# prop pixel table (pixelize_set.py): pass id (kit range 50+), ramp + 3 indices dark -> light, material group (shading
# thresholds), thin-claim fraction (small details drawn over their base) + priority
_T = dict(quilt=(0.46, 0.70), cream=(0.46, 0.70), rose=(0.44, 0.70), wood=(0.50, 0.74), sage=(0.44, 0.70),
          mint=(0.44, 0.70), plastic=(0.46, 0.70), screen=(0.44, 0.70), rain=(0.40, 0.66))
def _p(pid, ramp, idx, mg, thin=None, prio=0):
    d = dict(pid=pid, ramp=ramp, idx=idx, mg=mg, thresh=_T[mg])
    if thin: d.update(thin=thin, prio=prio)
    return d
PARTS = {
    'nb_cover': _p(50, 'fabric_quilt', (1, 2, 3), 'quilt'),
    'nb_page': _p(51, 'fabric_cream', (2, 3, 4), 'cream'),
    'nb_doodle': _p(52, 'fabric_rose', (1, 2, 3), 'rose', 0.30, 4),
    'crayon': _p(53, 'pip', (2, 3, 4), 'mint'),
    'crayon_tip': _p(54, 'pip', (1, 2, 3), 'mint'),
    'lt_body': _p(55, 'toy_plastic', (1, 2, 3), 'plastic'),
    'lt_keys': _p(56, 'toy_plastic', (0, 1, 2), 'plastic', 0.30, 3),
    'lt_screen': _p(57, 'screen', (1, 2, 3), 'screen'),
    'lt_sticker': _p(58, 'fabric_rose', (1, 2, 3), 'rose', 0.30, 4),
    'stand_wood': _p(59, 'wood', (2, 3, 4), 'wood'),
    'tray': _p(68, 'wood', (2, 3, 4), 'wood'),
    'ph_band': _p(60, 'fabric_rose', (1, 2, 3), 'rose'),
    'ph_cup': _p(61, 'pip', (2, 3, 4), 'mint'),
    'ph_pad': _p(62, 'fabric_cream', (1, 2, 3), 'cream'),
    'bucket': _p(63, 'wood', (2, 3, 4), 'wood'),
    'bucket_band': _p(64, 'fabric_sage', (1, 2, 3), 'sage'),
    'um_canopy': _p(65, 'fabric_rose', (1, 2, 3), 'rose'),
    'um_shaft': _p(66, 'wood', (1, 2, 3), 'wood'),
    'drop': _p(67, 'fabric_quilt', (2, 3, 4), 'rain'),
}

# ------------------------------------------------------------------ dimensions (world units, ori A set frame)
SIDE = 0.88                       # notebook / laptop: hand-off spot H = sit spot P - SIDE along the bear's right (-Y)
# notebook / laptop sit on a low toy tray-table (lead q-183110: seated stub mittens reach z ~0.85-1.0 only, never the floor)
TRAY = dict(x=0.95, y=0.0, hd=0.27, hw=0.42, top=0.64, th=0.11)
NB = dict(x=0.90, y=-0.08, hw=0.30, hd=0.19)        # notebook centre, half width (Y) / half depth (X), on the tray
LT = dict(x=0.93, y=0.0, hw=0.30, hd=0.20, base=0.11, lid=0.38)
STAND = dict(x=-0.06, y=1.08, top=1.34)              # headphone stand: post top (the band rests on it)
BUCKET = dict(x=-0.04, y=1.02, r=0.24, h=0.40)
PH_C = (0.0, -0.06, 1.56)        # headphones' centre on the rest head (rest bear: faces -Y, its left = +X)
PH_R = 0.66                      # band radius / cup offset
UM = dict(shaft=1.22, canopy_c=1.12, tip=1.32)      # umbrella along its axis from the grip (handle) toward the tip
UM_TOP = (0.18, 0.30, 2.80)      # the open canopy's centre target overhead (set frame, ori A)
RAIN_TOP, RAIN_PERIOD, RAIN_START, RAIN_STOP = 2.80, 8, -8, 1
# drops: (angle deg about the bear, radius, phase frames); a ring outside the open canopy (radius ~0.85)
DROPS = [(a, r, p) for a, r, p in ((0, 1.10, 0), (35, 1.00, 5), (70, 1.14, 2), (105, 1.02, 7), (140, 1.12, 3),
                                    (175, 0.98, 6), (210, 1.10, 1), (245, 1.02, 4), (280, 1.14, 6), (315, 1.00, 2),
                                    (20, 1.18, 4), (160, 1.16, 0))]     # about the canopy centre (UM_TOP x, y)

PROPS = {
    'notebook': dict(footprint=dict(x=[TRAY['x'] - TRAY['hd'], TRAY['x'] + TRAY['hd']], y=[-TRAY['hw'], TRAY['hw']]),
                     note='a low wooden toy tray-table with an open sketchbook (quilt-blue cover, cream pages, a rose doodle) lying on it'),
    'crayon': dict(footprint={}, note='a fat toy crayon: on the page at the hand-offs, in the right mitten while doodling'),
    'laptop': dict(footprint=dict(x=[TRAY['x'] - TRAY['hd'], TRAY['x'] + TRAY['hd']], y=[-TRAY['hw'], TRAY['hw']]),
                   note='a low wooden toy tray-table with a chunky rounded toy laptop (cream-pink plastic, a rose heart sticker), lid open'),
    'phone_stand': dict(footprint=dict(x=[STAND['x'] - 0.26, STAND['x'] + 0.26], y=[STAND['y'] - 0.26, STAND['y'] + 0.26]),
                        note='a little wooden headphone stand (round base, post, knob)'),
    'headphones': dict(footprint={}, note='mint headphones with a rose band: on the stand at the hand-offs, on the head in phones_vibe'),
    'bucket': dict(footprint=dict(x=[BUCKET['x'] - BUCKET['r'], BUCKET['x'] + BUCKET['r']],
                                  y=[BUCKET['y'] - BUCKET['r'], BUCKET['y'] + BUCKET['r']]),
                   note='a wooden umbrella bucket with a sage band'),
    'umbrella': dict(footprint={}, note='a rose umbrella (closed in the bucket at the hand-offs) and the rain drops '
                                        '(hidden at the hand-offs)'),
}
# layer -> the PARTS it is made of (verify_sit_b.py: a static layer's own colours mark where it must stay)
LAYER_PARTS = {'notebook': ['tray', 'nb_cover', 'nb_page', 'nb_doodle'], 'crayon': ['crayon', 'crayon_tip'],
               'laptop': ['tray', 'lt_body', 'lt_keys', 'lt_screen', 'lt_sticker'], 'phone_stand': ['stand_wood'],
               'headphones': ['ph_band', 'ph_cup', 'ph_pad'], 'bucket': ['bucket', 'bucket_band'],
               'umbrella': ['um_canopy', 'um_shaft', 'drop']}
STATIC = {'notebook': 'notebook', 'laptop': 'laptop', 'phones': 'phone_stand', 'umbrella': 'bucket'}
MOVING = {'notebook': ['crayon'], 'laptop': [], 'phones': ['headphones'], 'umbrella': ['umbrella']}


def geometry(s, ppu, pxz):
    """set frame = the bear's work spot P on the floor (a whole pixel: the anchor). notebook / laptop: the hand-off
    spot H is SIDE to the bear's right, snapped to whole pixels; phones / umbrella: H = P."""
    import stagekit
    if s in ('notebook', 'laptop'): hx, hy, hand_px = stagekit.snap(ppu, 0.0, -SIDE)
    else: hx, hy, hand_px = 0.0, 0.0, (0, 0)
    return dict(PXZ=pxz, ppu=ppu, hand=[hx, hy], spot=[0.0, 0.0], hand_px=list(hand_px), spot_px=[0, 0])


# ------------------------------------------------------------------ poses
# the shared plush floor sit (anim_clips SIT_LEG / SIT_BOB: the legs swung out in front, the pads riding them, the
# seat on the floor). bob is in screen px here (_poses scales it by PXZ); build() sets it from SIT_BOB for this PXZ.
SIT = dict(bob=None, legL=SIT_LEG, legR=SIT_LEG, armL=12.0, armR=12.0)
DOWN_LEG = (34.0, 28.0)          # half way down to the floor sit (or up from it): the legs start to swing out


def _sit_px(PXZ):
    SIT['bob'] = round(SIT_BOB / PXZ)        # whole px: the face 'layers' patches only match a body moved by whole px


def _poses(n, keys, PXZ):
    """keys [(frame, {param: value})] -> n poses (anim_clips.expand_k on REST_S; bob given in whole screen px)."""
    ks = []
    for f, kv in keys:
        kv = dict(kv)
        if 'bob' in kv: kv['bob'] = kv['bob'] * PXZ
        ks.append((f, kv))
    return expand_k(n, ks, REST_S)


def _fix(poses, i, q):
    """frame i = exactly q (joins must be pixel-identical: no easing residue)."""
    poses[i] = dict(q)


def _approach(g, PXZ, sit_keys):
    """frames 0-5 side shuffle H -> P (Stage lean's), 6-9 plop sit, then sit_keys (frame 10, 11)."""
    hy = g['hand'][1]; hx = g['hand'][0]
    y = lambda t: hy + (0.0 - hy) * t
    x = lambda t: hx + (0.0 - hx) * t
    S = dict(SIT, X=0.0, Y=0.0)
    keys = [(0, dict(X=hx, Y=hy)),
            (1, dict(X=x(.12), Y=y(.12), earL=1, earR=1, clY=2, fL=-0.04, hL=0.07)),
            (2, dict(X=x(.35), Y=y(.35), bob=1, earL=-1, earR=-1, clY=-2)),
            (3, dict(X=x(.60), Y=y(.60), earL=1, earR=1, clY=2, fR=-0.04, hR=0.07)),
            (4, dict(X=x(.85), Y=y(.85), bob=1, earL=-1, earR=-1, clX=2)),
            (5, dict(X=0.0, Y=0.0, earL=1, earR=1)),
            (6, dict(X=0.0, Y=0.0, bob=-2, sz=0.97, armL=-6.0, armR=-6.0, earL=2, earR=2)),       # a little dip
            (7, dict(X=0.0, Y=0.0, bob=round(SIT['bob'] * 0.45), legL=DOWN_LEG[0], legR=DOWN_LEG[1], armL=-10.0, armR=-10.0,
                     earL=-3, earR=-3, clX=4)),                                                   # legs swing out
            (8, dict(S, bob=SIT['bob'] - 2, sz=0.95, legL=SIT_LEG - 8, legR=SIT_LEG - 8, earL=4, earR=4, clX=-5)),  # plop
            (9, dict(S, bob=SIT['bob'] - 1, sz=0.97, legL=SIT_LEG + 6, legR=SIT_LEG + 6, earL=-2, earR=-2, clX=3))] + sit_keys
    return keys


def _leave(g, sit_keys):
    """the reverse: sit_keys (frames 0, 1), 2-5 stand up, 6-11 shuffle P -> H."""
    hy = g['hand'][1]; hx = g['hand'][0]
    y = lambda t: hy + (0.0 - hy) * t
    x = lambda t: hx + (0.0 - hx) * t
    S = dict(SIT, X=0.0, Y=0.0)
    return sit_keys + [
        (2, dict(S, bob=SIT['bob'] - 2, sz=0.96, legL=SIT_LEG - 12, legR=SIT_LEG - 12, armL=-14.0, armR=-14.0, earL=2,
                 earR=2)),                                                                         # mittens push
        (3, dict(X=0.0, Y=0.0, bob=round(SIT['bob'] * 0.45), legL=DOWN_LEG[1], legR=DOWN_LEG[0], armL=-8.0, armR=-8.0, earL=-3,
                 earR=-3, clX=-4)),                                                               # legs swing in
        (4, dict(X=0.0, Y=0.0, bob=1, sz=1.03, earL=3, earR=3, clX=4)),                            # up with a little hop
        (5, dict(X=0.0, Y=0.0, earL=-1, earR=-1)),
        (6, dict(X=x(.85), Y=y(.85), earL=1, earR=1, clY=-2, fR=0.04, hR=0.07)),
        (7, dict(X=x(.60), Y=y(.60), bob=1, earL=-1, earR=-1, clY=2)),
        (8, dict(X=x(.35), Y=y(.35), earL=1, earR=1, fL=0.04, hL=0.07)),
        (9, dict(X=x(.12), Y=y(.12), bob=1, earL=-1, earR=-1, clY=-1)),
        (10, dict(X=hx, Y=hy, earL=1, earR=1)),
        (11, dict(X=hx, Y=hy))]


def _yaw(poses):
    for q in poses: q['yaw'] = HAND_YAW
    return poses


# page points the crayon aims at (set frame, ori A): a little scribble on the right-hand page
def _page(u, v):
    return [NB['x'] - 0.06 + 0.09 * u, NB['y'] - 0.12 + 0.10 * v, TRAY['top'] + 0.12]


def build(s, g):
    PXZ = g['PXZ']; out = {}; _sit_px(PXZ)
    if s == 'notebook':
        DOODLE = dict(SIT, X=0.0, Y=0.0, armR=10.0, spR=-4.0, armL=6.0)
        sit = _yaw(_poses(12, _approach(g, PXZ, [(10, dict(SIT, X=0.0, Y=0.0, armR=16.0, spR=-6.0, earL=1, earR=1)),
                                                (11, DOODLE)]), PXZ))
        D0 = sit[-1]
        st_sit = [dict(crayon='page')] * 11 + [dict(crayon='paw', aim=_page(0, 0))]
        # doodle loop: 4 scribble strokes of 8 frames; the paw sways a little with each stroke
        kd = [(0, DOODLE)]
        stroke = [(2, 0.6, 0.0, 2.0, 1.0), (4, 1.0, 0.6, -1.0, 2.0), (6, 0.4, 1.0, 1.5, -1.5), (7, 0.1, 0.4, 0.5, 0.0)]
        aims = {0: (0, 0)}
        for b in range(4):
            for df, u, v, ar, sp in stroke:
                f = b * 8 + df
                if f >= 32: continue
                kv = dict(DOODLE, armR=DOODLE['armR'] + ar, spR=DOODLE['spR'] + sp)
                if b == 1 and df == 4: kv.update(earL=3, earR=-2, clY=3)
                if b == 2 and df == 2: kv.update(hb=1, earL=-2, earR=-2)
                if b == 2 and df == 6: kv.update(legL=SIT_LEG + 14, legR=SIT_LEG - 4, clX=3)       # a little leg swing
                if b == 3 and df == 4: kv.update(legR=SIT_LEG + 14, legL=SIT_LEG - 4, earR=2, clX=-2)
                kd.append((f, kv)); aims[f] = (u, v)
        doodle = _yaw(_poses(32, kd, PXZ)); _fix(doodle, 0, D0)
        af = sorted(aims)
        st_d = []
        for i in range(32):
            j = max(k for k in af if k <= i); n_ = min([k for k in af if k > i] or [32])
            u0, v0 = aims[j]; u1, v1 = aims.get(n_, aims[0]) if n_ < 32 else aims[0]
            t = (i - j) / (n_ - j)
            st_d.append(dict(crayon='paw', aim=[round(c, 4) for c in _page(u0 + (u1 - u0) * t, v0 + (v1 - v0) * t)]))
        stand = _yaw(_poses(12, _leave(g, [(0, DOODLE), (1, dict(SIT, X=0.0, Y=0.0, armR=16.0, spR=-6.0, earL=-1, earR=-1))]), PXZ))
        _fix(stand, 0, D0)
        st_stand = [dict(crayon='paw', aim=_page(0, 0))] + [dict(crayon='page')] * 11
        out = dict(note_sit=list(zip(sit, st_sit)), note_doodle=list(zip(doodle, st_d)), note_stand=list(zip(stand, st_stand)))
    elif s == 'laptop':
        TYPE = dict(SIT, X=0.0, Y=0.0, armL=12.0, armR=12.0)
        sit = _yaw(_poses(12, _approach(g, PXZ, [(10, dict(SIT, X=0.0, Y=0.0, armL=16.0, armR=10.0, earL=1, earR=1)),
                                                (11, TYPE)]), PXZ))
        T0 = sit[-1]
        kt = [(0, TYPE)]
        for b in range(6):                      # alternating taps, 4 frames each: L down, up, R down, up
            f = b * 4
            if f + 1 < 24: kt.append((f + 1, dict(TYPE, armL=8.0, armR=15.0, earL=1 if b % 2 else -1)))
            if f + 3 < 24: kt.append((f + 3, dict(TYPE, armL=15.0, armR=8.0, earR=1 if b % 2 else -1,
                                                  hb=1 if b in (2, 3) else 0, clY=2 if b == 4 else 0)))
        typ = _yaw(_poses(24, kt, PXZ)); _fix(typ, 0, T0)
        stand = _yaw(_poses(12, _leave(g, [(0, TYPE), (1, dict(SIT, X=0.0, Y=0.0, armL=10.0, armR=16.0, earL=-1, earR=-1))]), PXZ))
        _fix(stand, 0, T0)
        e = lambda n: [dict()] * n
        out = dict(laptop_sit=list(zip(sit, e(12))), laptop_type=list(zip(typ, e(24))), laptop_stand=list(zip(stand, e(12))))
    elif s == 'phones':
        H = dict(X=0.0, Y=0.0)
        VIBE = dict(H, blink=0.12, armL=4.0, armR=4.0)
        on = _yaw(_poses(14, [
            (0, H),
            (2, dict(H, spL=58.0, armL=6.0, roll=-3.0, earL=2, earR=2, clY=2)),       # reach to the stand top
            (4, dict(H, spL=72.0, armL=2.0, roll=-4.0, earL=-1, earR=-1)),            # grab the band
            (6, dict(H, spL=118.0, spR=60.0, armL=-6.0, hp=-8.0, earL=2, earR=2, clX=3)),   # up they go
            (8, dict(H, spL=132.0, spR=128.0, hp=-6.0, earL=-3, earR=-3, clY=-3)),
            (10, dict(H, spL=124.0, spR=124.0, bob=-2, sz=0.97, earL=3, earR=3)),     # on, a soft squash
            (11, dict(H, spL=96.0, spR=96.0, bob=-1, sz=0.99, mouth='open', earL=-2, earR=-2)),
            (13, VIBE)], PXZ))
        V0 = on[-1]
        st_on = [dict(ph='stand')] * 5 + [dict(ph='lerp', t=t) for t in (0.2, 0.42, 0.64, 0.84, 1.0)] + [dict(ph='head')] * 4
        kv = [(0, VIBE)]
        for b in range(4):                     # four beats of 8 frames: nod down on the beat, sway across
            f = b * 8; side = 1 if b % 2 == 0 else -1
            kv += [(f + 2, dict(VIBE, hp=9.0, hr=6.0 * side, roll=2.0 * side, bob=-1, armL=4.0 + 8 * side, armR=4.0 - 8 * side,
                                hR=0.08 if side > 0 else 0.0, hL=0.08 if side < 0 else 0.0, earL=3, earR=3, clY=4 * side,
                                mouth='open' if b in (1, 3) else 'closed')),
                   (f + 5, dict(VIBE, hp=-2.0, hr=-3.0 * side, roll=-1.0 * side, armL=4.0 - 4 * side, armR=4.0 + 4 * side,
                                earL=-2, earR=-2, clX=2 * side))]
        kv = [k for k in kv if k[0] < 32]
        vibe = _yaw(_poses(32, kv + [(31, dict(VIBE, hp=-1.0, earL=-1, earR=-1))], PXZ)); _fix(vibe, 0, V0)
        off = _yaw(_poses(14, [
            (0, VIBE),
            (2, dict(H, spL=110.0, spR=110.0, blink=1.0, earL=2, earR=2)),           # mittens up to the cups
            (4, dict(H, spL=130.0, spR=126.0, hp=-6.0, earL=-2, earR=-2)),
            (6, dict(H, spL=112.0, spR=50.0, hp=-4.0, clX=3)),                       # lift off
            (8, dict(H, spL=76.0, spR=10.0, armL=2.0, roll=-4.0, earL=2, earR=2)),   # back onto the stand
            (10, dict(H, spL=60.0, armL=6.0, roll=-3.0, earL=-1, earR=-1)),
            (12, dict(H, earL=1, earR=1, clY=-1)),
            (13, H)], PXZ)); _fix(off, 0, V0)
        st_off = [dict(ph='head')] * 4 + [dict(ph='lerp', t=t) for t in (0.84, 0.62, 0.40, 0.18)] + [dict(ph='stand')] * 6
        out = dict(phones_on=list(zip(on, st_on)), phones_vibe=list(zip(vibe, [dict(ph='head')] * 32)),
                   phones_off=list(zip(off, st_off)))
    elif s == 'umbrella':
        H = dict(X=0.0, Y=0.0)
        HOLD = dict(H, armL=16.0, spL=128.0, hp=-10.0, mouth='open')                # umbrella overhead in the left mitten
        op = _yaw(_poses(14, [
            (0, H),
            (2, dict(H, spL=56.0, armL=8.0, roll=-3.0, earL=2, earR=2)),             # reach the handle
            (3, dict(H, spL=60.0, armL=8.0, roll=-3.0, earL=-1, earR=-1)),
            (5, dict(H, spL=74.0, armL=10.0, hp=-12.0, mouth='open', earL=3, earR=3, clY=3)),   # lift; a drop! look up
            (7, dict(H, spL=112.0, armL=18.0, hp=-12.0, mouth='wide', earL=-3, earR=-3, clX=4)),  # twirl it upright
            (9, dict(H, spL=130.0, armL=18.0, hp=-10.0, mouth='open', bob=-1, sz=0.98)),       # pop
            (11, dict(H, spL=124.0, armL=16.0, hp=-10.0, mouth='open', bob=1, sz=1.02, earL=2, earR=2)),
            (13, HOLD)], PXZ))
        R0 = op[-1]
        um_op = [dict(at='bucket', open=0.0)] * 3 + [
            dict(at='paw', dir=[0.0, 0.0, -1.0], open=0.0),
            dict(at='paw', dir=[0.0, 0.0, -1.0], open=0.0, lift=0.06),
            dict(at='paw', dir=[0.15, 0.25, -0.95], open=0.0),
            dict(at='paw', dir=[0.25, 0.97, 0.0], open=0.0),                         # twirl: sideways ...
            dict(at='paw', dir=[0.1, 0.45, 0.89], open=0.0),                         # ... upright
            dict(at='paw', aim=UM_TOP, open=0.1),
            dict(at='paw', aim=UM_TOP, open=0.6),
            dict(at='paw', aim=UM_TOP, open=1.12),                                   # pop, with an overshoot
            dict(at='paw', aim=UM_TOP, open=0.95),
            dict(at='paw', aim=UM_TOP, open=1.02),
            dict(at='paw', aim=UM_TOP, open=1.0)]
        st_op = [dict(u, rain=f - 13, start=RAIN_START) for f, u in enumerate(um_op)]
        kr = [(0, HOLD)]
        for b, (f, kv) in enumerate([(4, dict(roll=3.0, hr=4.0, hL=0.06, earL=2, earR=2)),
                                     (8, dict(roll=-2.0, hr=-3.0, hR=0.06, mouth='closed', earL=-2, earR=-2, clY=3)),
                                     (12, dict(roll=1.0, spR=34.0, armR=20.0, hp=-14.0, mouth='wide')),     # catch a drop
                                     (15, dict(roll=1.0, spR=44.0, armR=26.0, hp=-14.0, mouth='wide', earR=3)),
                                     (18, dict(roll=0.0, spR=30.0, armR=18.0, hp=-12.0, mouth='open', blink=0.12)),
                                     (21, dict(roll=-3.0, hr=-4.0, hL=0.06, earL=3, earR=3, clX=3)),
                                     (25, dict(roll=2.0, hr=3.0, hR=0.06, bob=-1, earL=-2, earR=-2)),
                                     (28, dict(roll=-1.0, hr=-1.0, earL=1, earR=1, clY=-2)),
                                     (31, dict(earL=1, clY=1))]):
            kr.append((f, dict(HOLD, **kv)))
        rain = _yaw(_poses(32, kr, PXZ)); _fix(rain, 0, R0)
        st_rain = [dict(at='paw', aim=UM_TOP, open=1.0, rain=f) for f in range(32)]
        cl = _yaw(_poses(14, [
            (0, HOLD),
            (2, dict(HOLD, spR=36.0, armR=22.0, hp=-14.0, mouth='closed', earR=3)),   # is it stopping? paw out
            (4, dict(HOLD, spR=10.0, hp=-8.0, mouth='closed', earL=-2, earR=-2)),
            (6, dict(H, spL=104.0, armL=16.0, hp=-6.0, earL=2, earR=2, clX=-3)),       # closed, twirl down
            (8, dict(H, spL=74.0, armL=10.0, hp=-2.0, roll=-3.0, earL=-2, earR=-2)),
            (10, dict(H, spL=60.0, armL=8.0, roll=-3.0, earL=1, earR=1)),             # into the bucket, let go
            (12, dict(H, spL=14.0, armL=2.0, earL=-1, earR=-1, clY=-1)),
            (13, H)], PXZ)); _fix(cl, 0, R0)
        um_cl = [dict(at='paw', aim=UM_TOP, open=1.0), dict(at='paw', aim=UM_TOP, open=1.0),
                 dict(at='paw', aim=UM_TOP, open=1.0), dict(at='paw', aim=UM_TOP, open=0.55),
                 dict(at='paw', aim=UM_TOP, open=0.15), dict(at='paw', dir=[0.1, 0.45, 0.89], open=0.0),
                 dict(at='paw', dir=[0.25, 0.97, 0.0], open=0.0), dict(at='paw', dir=[0.15, 0.25, -0.95], open=0.0),
                 dict(at='paw', dir=[0.0, 0.0, -1.0], open=0.0, lift=0.06), dict(at='paw', dir=[0.0, 0.0, -1.0], open=0.0)] \
            + [dict(at='bucket', open=0.0)] * 4
        st_cl = [dict(u, rain=f, stop=RAIN_STOP) for f, u in enumerate(um_cl)]
        out = dict(umbrella_open=list(zip(op, st_op)), umbrella_rain=list(zip(rain, st_rain)),
                   umbrella_close=list(zip(cl, st_cl)))
    for c, n in SETS[s]['counts'].items(): assert len(out[c]) == n, (c, len(out[c]), n)
    return out


def handoff(s, g):
    a, b, c = SETS[s]['clips']
    return {a: dict(at='start', pos=list(g['hand']), yaw=HAND_YAW), c: dict(at='end', pos=list(g['hand']), yaw=HAND_YAW)}


# ------------------------------------------------------------------ rain (pure: verify_sit_b.py reads it too)
def drop_state(tau, start=None, stop=None):
    """[(index, set-frame point or None)] of every drop at rain time tau: a drop falls RAIN_TOP -> floor in
    RAIN_PERIOD - 1 frames and splashes on the last; it shows only when its current fall began in [start, stop)."""
    out = []
    for i, (a, r, ph) in enumerate(DROPS):
        k = (tau + ph) % RAIN_PERIOD; born = tau - k
        if (start is not None and born < start) or (stop is not None and born >= stop): out.append((i, None)); continue
        z = RAIN_TOP * (1 - k / (RAIN_PERIOD - 1))
        t = math.radians(a)
        out.append((i, [round(UM_TOP[0] + r * math.cos(t), 4), round(UM_TOP[1] + r * math.sin(t), 4), round(z, 4), k == RAIN_PERIOD - 1]))
    return out


# ------------------------------------------------------------------ Blender side (render_set.py only)
_REG = {}            # layer -> [objects]; 'set' -> the set name; filled by build_props


def _sq(c, r, e1=0.35, e2=0.35, **o):
    return (tuple(c), tuple(r), e1, e2, o) if o else (tuple(c), tuple(r), e1, e2)


def build_props(s, kit, g):
    """kit (render_set.py): empty(name, loc, parent), solid(name, parent, layer, pid, shapes), W0 (the set root).
    Static props go under the set root; moving ones have no parent (prop_state places them every frame)."""
    _REG.clear(); _REG['set'] = s
    pid = lambda n: PARTS[n]['pid']
    def solid(name, parent, layer, part, shapes):
        ob = kit.solid(name, parent, layer, pid(part), shapes); _REG.setdefault(layer, []).append(ob); return ob
    info = {}
    if s == 'notebook':
        _tray(kit, solid, 'notebook')
        P = kit.empty('P_notebook', (NB['x'], NB['y'], TRAY['top']), kit.W0)
        hw, hd = NB['hw'], NB['hd']
        solid('nb_cover', P, 'notebook', 'nb_cover', [_sq((0, 0, 0.035), (hd, hw, 0.035), 0.2, 0.25)])
        solid('nb_page', P, 'notebook', 'nb_page', [_sq((0, -hw * 0.49, 0.085), (hd - 0.035, hw * 0.46, 0.03), 0.25, 0.3),
                                                     _sq((0, hw * 0.49, 0.085), (hd - 0.035, hw * 0.46, 0.03), 0.25, 0.3)])
        solid('nb_doodle', P, 'notebook', 'nb_doodle', [_sq((-0.02, -hw * 0.5, 0.118), (0.075, 0.075, 0.006), 1.0, 1.0),
                                                         _sq((0.10, -hw * 0.38, 0.118), (0.035, 0.035, 0.006), 1.0, 1.0)])
        # the crayon: grip at the origin, +z toward the tip
        solid('crayon', None, 'crayon', 'crayon', [_sq((0, 0, 0.13), (0.06, 0.06, 0.19), 0.5, 1.0)])
        solid('crayon_tip', None, 'crayon', 'crayon_tip', [_sq((0, 0, 0.34), (0.045, 0.045, 0.06), 1.0, 1.0)])
        info['notebook'] = dict(px=_px(g, TRAY['x'], TRAY['y']), pos=[TRAY['x'], TRAY['y'], 0.0])
        info['crayon'] = dict(px=_px(g, *_page(0, 0)[:2]), pos=_page(0, 0))
    elif s == 'laptop':
        _tray(kit, solid, 'laptop')
        P = kit.empty('P_laptop', (LT['x'], LT['y'], TRAY['top']), kit.W0)
        hw, hd, bh, lh = LT['hw'], LT['hd'], LT['base'], LT['lid']
        solid('lt_body', P, 'laptop', 'lt_body', [
            _sq((0, 0, bh / 2), (hd, hw, bh / 2), 0.25, 0.25),                                    # base
            _sq((hd - 0.02, 0, bh + lh / 2 - 0.02), (0.035, hw - 0.01, lh / 2), 0.25, 0.25)])     # lid (hinge far side)
        solid('lt_keys', P, 'laptop', 'lt_keys', [_sq((-0.05 + 0.11 * i, -0.18 + 0.12 * j, bh + 0.005), (0.04, 0.045, 0.012), 0.5, 0.5)
                                                   for i in range(2) for j in range(4)])
        solid('lt_screen', P, 'laptop', 'lt_screen', [_sq((hd - 0.058, 0, bh + lh / 2 + 0.01), (0.008, hw - 0.06, lh / 2 - 0.07), 0.3, 0.3)])
        solid('lt_sticker', P, 'laptop', 'lt_sticker', [_sq((hd + 0.017, -0.05, bh + lh * 0.55), (0.006, 0.055, 0.05), 1.0, 1.0),
                                                         _sq((hd + 0.017, 0.05, bh + lh * 0.55), (0.006, 0.055, 0.05), 1.0, 1.0),
                                                         _sq((hd + 0.017, 0.0, bh + lh * 0.45), (0.006, 0.06, 0.06), 1.0, 1.0)])
        info['laptop'] = dict(px=_px(g, TRAY['x'], TRAY['y']), pos=[TRAY['x'], TRAY['y'], 0.0])
    elif s == 'phones':
        P = kit.empty('P_stand', (STAND['x'], STAND['y'], 0), kit.W0)
        solid('stand', P, 'phone_stand', 'stand_wood', [
            _sq((0, 0, 0.04), (0.24, 0.24, 0.04), 0.3, 1.0),
            _sq((0, 0, STAND['top'] / 2), (0.05, 0.05, STAND['top'] / 2), 0.5, 1.0),
            _sq((0, 0, STAND['top']), (0.09, 0.09, 0.06), 1.0, 1.0)])
        _headphones(solid)
        info['phone_stand'] = dict(px=_px(g, STAND['x'], STAND['y']), pos=[STAND['x'], STAND['y'], 0.0])
        info['headphones'] = dict(px=_px(g, STAND['x'], STAND['y'], STAND['top']), pos=[STAND['x'], STAND['y'], STAND['top']])
    elif s == 'umbrella':
        P = kit.empty('P_bucket', (BUCKET['x'], BUCKET['y'], 0), kit.W0)
        r, h = BUCKET['r'], BUCKET['h']
        solid('bucket', P, 'bucket', 'bucket', [_sq((0, 0, h / 2), (r, r, h / 2), 0.3, 1.0)])
        solid('bucket_band', P, 'bucket', 'bucket_band', [_sq((0, 0, h * 0.62), (r + 0.015, r + 0.015, 0.05), 0.3, 1.0)])
        _umbrella(solid)
        info['bucket'] = dict(px=_px(g, BUCKET['x'], BUCKET['y']), pos=[BUCKET['x'], BUCKET['y'], 0.0])
        info['umbrella'] = dict(px=_px(g, BUCKET['x'], BUCKET['y'], _grip_z()), pos=[BUCKET['x'], BUCKET['y'], _grip_z()])
    return info


def _px(g, x, y, z=0.0):
    import stagekit
    return [int(round(v)) for v in stagekit.px_of(g['ppu'], x, y, z)]


def _tray(kit, solid, layer):
    """a low, chunky wooden toy tray-table (rounded top, four stubby round legs), no chair: not Stage D's desk."""
    T = kit.empty('P_tray', (TRAY['x'], TRAY['y'], 0), kit.W0)
    hd, hw, top, th = TRAY['hd'], TRAY['hw'], TRAY['top'], TRAY['th']
    solid('tray', T, layer, 'tray', [_sq((0, 0, top - th / 2), (hd, hw, th / 2), 0.25, 0.3)] +
          [_sq((sx * (hd - 0.08), sy * (hw - 0.09), (top - th) / 2), (0.075, 0.075, (top - th) / 2 + 0.01), 0.6, 1.0)
           for sx in (1, -1) for sy in (1, -1)])


def _headphones(solid):
    """built around the headphones' centre (origin), in the rest bear's axes (x = cup to cup, z = up)."""
    band = []
    for i in range(17):
        a = math.radians(-64 + 8 * i)
        band.append(_sq((PH_R * 0.94 * math.sin(a), 0, 0.10 + PH_R * 0.82 * math.cos(a)), (0.075, 0.075, 0.075), 1.0, 1.0))
    solid('ph_band', None, 'headphones', 'ph_band', band)
    solid('ph_cup', None, 'headphones', 'ph_cup', [_sq((sx * PH_R, 0, 0), (0.09, 0.17, 0.19), 0.6, 0.6) for sx in (1, -1)])
    solid('ph_pad', None, 'headphones', 'ph_pad', [_sq((sx * (PH_R - 0.07), 0, 0), (0.04, 0.14, 0.16), 0.8, 0.8) for sx in (1, -1)])


def _grip_z(): return UM['tip'] + 0.04          # in the bucket: the tip just above the bucket floor, the handle up


def _umbrella(solid):
    """grip (handle bend) at the origin, +z toward the tip; the canopy pieces are separate objects so prop_state can
    scale them open / closed."""
    sh = UM['shaft']
    solid('um_shaft', None, 'umbrella', 'um_shaft', [_sq((0, 0, sh / 2), (0.03, 0.03, sh / 2), 1.0, 1.0),
                                                       _sq((0, 0, UM['tip']), (0.045, 0.045, 0.06), 1.0, 1.0),
                                                       _sq((0.02, 0, -0.06), (0.035, 0.035, 0.08), 1.0, 1.0),     # the crook
                                                       _sq((0.09, 0, -0.13), (0.06, 0.035, 0.035), 1.0, 1.0),
                                                       _sq((0.16, 0, -0.08), (0.035, 0.035, 0.05), 1.0, 1.0)])
    solid('um_canopy', None, 'umbrella', 'um_canopy', [_sq((0, 0, 0), (1.0, 1.0, 1.0), 1.0, 1.0)])   # unit; scaled
    for i in range(8):
        solid(f'um_scallop{i}', None, 'umbrella', 'um_canopy', [_sq((0, 0, 0), (0.16, 0.16, 0.11), 1.0, 1.0)])
    for i in range(len(DROPS)):
        solid(f'drop{i}', None, 'umbrella', 'drop', [_sq((0, 0, 0), (0.05, 0.05, 0.085), 1.0, 0.9)])


def prop_state(s, st):
    """layer -> render key of this frame; places the moving objects (see the module header). Runs in Blender after
    render_set.py posed the rig for this frame."""
    keys = {STATIC[s]: STATIC[s] + '_0'}
    if not MOVING[s]: return keys
    import bpy
    from mathutils import Matrix, Vector, Quaternion
    O = bpy.data.objects; rig = O['Rig']; W0 = O['SetWorld']
    B = W0.matrix_world.determinant() < 0
    Mw = W0.matrix_world.copy(); Sx = Matrix.Diagonal((-1.0, 1.0, 1.0, 1.0))

    def setm(Q):            # a set-frame (ori A) placement -> world (proper in both oris)
        return Mw @ Q @ Sx if B else Q

    def setp(p):            # a set-frame point -> world
        return Mw @ Vector(p)

    def paw(side):          # world point of the A-side paw (swapped for B)
        n = {'R': 'paw_L', 'L': 'paw_R'}[side] if B else f'paw_{side}'
        return rig.matrix_world @ rig.pose.bones[n].head

    def delta(n):
        pb = rig.pose.bones[n]; b = rig.data.bones[n]
        return rig.matrix_world @ pb.matrix @ b.matrix_local.inverted() @ rig.matrix_world.inverted()

    def aim(d):             # rotation taking +z to the world direction d
        return d.normalized().to_track_quat('Z', 'Y').to_matrix().to_4x4()

    def place(ob, M): ob.matrix_world = M

    if s == 'notebook':
        cr = [o for o in _REG['crayon']]
        if st['crayon'] == 'page':
            p = _page(0.3, 0.5)
            Q = Matrix.Translation(Vector((p[0], p[1], p[2] + 0.06))) @ Matrix.Rotation(math.radians(-25), 4, 'Z') \
                @ Matrix.Rotation(math.radians(90), 4, 'Y') @ Matrix.Translation(Vector((0, 0, -0.17)))
            M = setm(Q)
        else:
            g = paw('R'); t = setp(st['aim']); M = Matrix.Translation(g) @ aim(t - g)
        for o in cr: place(o, M)
    elif s == 'phones':
        hp = _REG['headphones']
        Mst = setm(Matrix.Translation(Vector((STAND['x'], STAND['y'], STAND['top'] + 0.07 - (0.10 + PH_R * 0.82 + 0.075))))
                   @ Matrix.Rotation(math.radians(45 + 90), 4, 'Z'))
        Mhd = delta('head') @ Matrix.Translation(Vector(PH_C))
        if st['ph'] == 'stand': M = Mst
        elif st['ph'] == 'head': M = Mhd
        else:
            t = st['t']; a, b = Mst.decompose(), Mhd.decompose()
            loc = a[0].lerp(b[0], t) + Vector((0, 0, 0.45 * 4 * t * (1 - t))); rot = a[1].slerp(b[1], t)
            M = Matrix.Translation(loc) @ rot.to_matrix().to_4x4()
        for o in hp: place(o, M)
    elif s == 'umbrella':
        objs = {o.name: o for o in _REG['umbrella']}
        if st['at'] == 'bucket':
            Mg = setm(Matrix.Translation(Vector((BUCKET['x'], BUCKET['y'], _grip_z()))) @ Matrix.Rotation(math.pi, 4, 'X'))
        else:
            g = paw('L')
            if 'aim' in st: d = setp(st['aim']) - g
            else:
                v = st['dir']; d = (Mw.to_3x3() @ Vector(v)) if B else Vector(v)
            k = 1.0
            if 'aim' in st: k = max(1.0, d.length / UM['canopy_c'])     # stretch the shaft so the canopy lands at the target
            Mg = Matrix.Translation(g + Vector((0, 0, st.get('lift', 0.0)))) @ aim(d) @ Matrix.Diagonal((1.0, 1.0, k, 1.0))
        o_ = st['open']
        place(objs['um_shaft'], Mg)
        # canopy: closed = a slim furl along the shaft; open = a wide low dome at the shaft top, turning upright as it
        # opens (an open canopy is always level over the head, so it never hides the face)
        oc = min(o_, 1.0)
        R = 0.12 + (0.84 - 0.12) * min(o_, 1.2); Hh = 0.40 + (0.25 - 0.40) * oc
        zc = 0.80 + (UM['canopy_c'] - 0.80) * oc
        c = Mg @ Vector((0, 0, zc))
        q = Mg.to_quaternion().slerp(Quaternion(), oc)
        F = Matrix.Translation(c) @ q.to_matrix().to_4x4()
        place(objs['um_canopy'], F @ Matrix.Diagonal((R, R, Hh, 1.0)))
        for i in range(8):
            a = math.radians(22.5 + 45 * i); rr = R * 0.93
            sc = 0.25 + 0.75 * oc
            place(objs[f'um_scallop{i}'], F @ Matrix.Translation(Vector((rr * math.cos(a), rr * math.sin(a), -Hh * 0.45)))
                  @ Matrix.Diagonal((sc, sc, sc, 1.0)))
        hide = setp((BUCKET['x'], BUCKET['y'], BUCKET['h'] * 0.5))
        for i, p in drop_state(st['rain'], st.get('start'), st.get('stop')) if 'rain' in st else [(i, None) for i in range(len(DROPS))]:
            ob = objs[f'drop{i}']
            if p is None: place(ob, Matrix.Translation(hide) @ Matrix.Diagonal((1e-6, 1e-6, 1e-6, 1.0)))
            else:
                w = setp(p[:3])
                sc = Matrix.Diagonal((1.7, 1.7, 0.4, 1.0)) if p[3] else Matrix.Identity(4)
                place(ob, Matrix.Translation(w + Vector((0, 0, 0.035 if p[3] else 0.0))) @ sc)
    bpy.context.view_layer.update()
    for layer in MOVING[s]:
        h = hashlib.md5()
        for o in sorted(_REG[layer], key=lambda o: o.name):
            h.update(o.name.encode()); h.update(repr([round(x, 5) for row in o.matrix_world for x in row]).encode())
        keys[layer] = f'{layer}_{h.hexdigest()[:12]}'
    return keys
