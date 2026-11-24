"""Stage out (set module of the stage kit, stages/out.json): five toy prop sets the bear uses on its own.
Pure Python (rig_bear.py --stage k keys the poses, render_set.py builds the props, make_set.py / verify_out.py read
the same tables). Set contract (CLAUDE.md "Stage kit"): SETS, PARTS, PROPS, geometry, build, handoff, build_props,
prop_state.

  plant    plant_get 12 -> plant_water 30 (loop) -> plant_put 12   a toy watering can and a potted sprout
  sweep    sweep_get 12 -> sweep 24 (loop) -> sweep_put 12         a toy broom
  snowman  snow_kneel 12 -> snow_build 48 (loop) -> snow_stand 12  a snow patch with a mound; a snowman grows and puffs
  fishing  fish_sit 12 -> fish_wait 36 (loop) -> fish_stand 12     an ice patch with a hole and a toy rod (wait: face
                                                                   layers, set_clips, patch SE / SW)
  bed_cap  cap_get_in_bed 16 -> cap_sleep 36 (loop) -> cap_wake_up 16  a small toy bed in Stage B's look + a nightcap

Every frame is (q, st): q = the Stage K pose (anim_clips.REST_K) plus the set placement X, Y, Z (set frame, world
units) and yaw (Stage A convention, 0 = SW, 90 = SE); st = the prop state {rigid: spec}. Ori A (written here): the
hand-off spot H is the set origin, the bear faces SE (+X), its left (+Y) is NE on screen. Ori B = the 3D mirror
(anim_clips.mirror_k; the set root W0 mirrors the props): facing SW.

Moving props (held, grown, thrown) without a kit change. RELIES ON render_set.py calling prop_state(set, st) for
each frame right AFTER posing the rig for that frame (use(clip, f) then set_state's prop_state; true today). Inside
Blender (_R filled by build_props) prop_state places every rigid of the set for that frame and returns, per layer, a
key hashed from the placed matrices, so the kit's render cache keys on what is drawn. Specs (JSON lists):
  ['home']                          the rigid's home placement (the props-only frame)
  ['paw', t, rx, ry, rz]            follows the posed left mitten (ori B: the right one, mirrored), modelled at its
                                    in-paw grip; t < 1 blends home -> paw (grab / put down); r* = extra grip turn, deg
  ['head', t, rx, ry, rz]           the same on the head bone (the nightcap)
  ['at', x, y, z, rx, ry, rz, s]    a set-frame placement, uniform scale s (0 = hidden: scale 1e-6)
  ['box', x, y, z, sx, sy, sz]      a set-frame placement with a per-axis scale (the quilt halves)
  ['hide']                          not drawn (the folded quilt halves while the quilt is pulled up)
  ['drape', h, out]                 the pulled-up quilt laid over the lying bear: a cloth surface rebuilt every
                                    frame from the POSED bear (_drape: it rests on the tummy, arms and feet, tents
                                    between them and hangs over the mattress sides), covering the bear up to height
                                    h along the posed body bone (0 = the seat, 1.2 = the chest), with a cream
                                    turned-down hem along that edge; h = 0 hides it; out = 1: the arms are out,
                                    over the quilt
Derived rigids (drops, dust, the fishing line) are placed from their host's matrix (spout tip, broom head, rod tip).
Out of Blender the key is a hash of st. verify_out.py proves the reliance: the kit's hand-off check (props + idle#0),
exact joins and seams, and a self-test mutation that moves a prop on a hand-off frame.
"""
import math, hashlib, json
from anim_clips import REST_K, SIT_LEG, SIT_BOB

REST_S = dict(REST_K, X=0.0, Y=0.0, Z=0.0, yaw=0.0)
HAND_YAW = 90.0


def _set(clips, counts, face=(), note='', ori=('', ''), nxt=None):
    a, l, b = clips
    return dict(clips=list(clips), counts=dict(zip(clips, counts)), loop=[l],
                joins=[(a, -1, l, 0), (b, 0, l, 0)], handoff={a: 'start', b: 'end'}, hand_yaw=HAND_YAW,
                face=list(face), patch={'A': 'SE', 'B': 'SW'},
                next={a: l, l: f'{l} or {b}', b: 'Stage A idle (rest pose, same spot and facing)'},
                ori_note={'A': ori[0], 'B': ori[1]},
                note=(f'Stage out {clips[0].split("_")[0]}: {a} -> {l} (loop) -> {b}. Stage B format: each frame is the '
                      f'whole set baked on one canvas; anchor = pixel of the set origin (the hand-off spot). Hand-off: {a} '
                      f'frame 0 and {b}\'s last frame are pixel-identical to the props-only frame + the Stage A idle '
                      f'sprite (frame 0). Joins: {a} last = {l} 0 = {b} 0. {note}'))


SETS = {
    'plant': _set(('plant_get', 'plant_water', 'plant_put'), (12, 30, 12), note='Face baked.',
                  ori=('the bear faces SE, the can on its left (NE), the potted sprout ahead-left (E)',
                       'mirror of A: faces SW, the can NW, the sprout W')),
    'sweep': _set(('sweep_get', 'sweep', 'sweep_put'), (12, 24, 12), note='Face baked.',
                  ori=('the bear faces SE, the broom lies on its left (NE)', 'mirror of A: faces SW, the broom NW')),
    'snowman': _set(('snow_kneel', 'snow_build', 'snow_stand'), (12, 48, 12), note='Face baked.',
                    ori=('the bear faces SE, the snow mound ahead-left (E)', 'mirror of A: faces SW, the mound W')),
    'fishing': _set(('fish_sit', 'fish_wait', 'fish_stand'), (12, 36, 12), face=['fish_wait'],
                    note='fish_wait: face layers from face_out.json set_clips (patch in face_patch_dir); the rest baked.',
                    ori=('the bear faces SE, the ice hole ahead-left (E), the rod on its left (NE)',
                         'mirror of A: faces SW, the hole W, the rod NW')),
    'bed_cap': _set(('cap_get_in_bed', 'cap_sleep', 'cap_wake_up'), (16, 36, 16), note='Face baked.',
                    ori=('the bear stands SW of the bed (head end NW) facing SE; the nightcap lies on the pillow',
                         'mirror of A: SE of the bed (head end NE) facing SW')),
}

T3 = (0.46, 0.70)
PARTS = {}
for _i, (_n, _r, _x, _mg, _th, _extra) in enumerate([
        ('pot', 'terracotta', (1, 2, 3), 'terracotta', T3, {}),
        ('pot_rim', 'terracotta', (2, 3, 4), 'terracotta', T3, {}),
        ('soil', 'wood', (0, 1, 2), 'wood', (0.50, 0.74), {}),
        ('leaf', 'pip', (2, 3, 4), 'leaf', T3, {}),
        ('can', 'fabric_quilt', (1, 2, 3), 'quilt', (0.46, 0.70), {}),
        ('can_cap', 'fabric_rose', (1, 2, 3), 'rose', (0.44, 0.70), {}),
        ('drop', 'ice', (2, 3, 4), 'ice', T3, {}),
        ('stick', 'wood', (2, 3, 4), 'wood', (0.50, 0.74), {}),
        ('straw', 'straw', (1, 2, 3), 'straw', T3, {}),
        ('band', 'fabric_rose', (1, 2, 3), 'rose', (0.44, 0.70), {}),
        ('dust', 'snow', (1, 2, 3), 'snow', T3, {}),
        ('snow', 'snow', (2, 3, 4), 'snow', T3, {}),
        ('carrot', 'carrot', (1, 2, 3), 'carrot', T3, {}),
        ('coal', 'void', (0, 1, 2), 'coal', T3, {}),
        ('ice', 'ice', (2, 3, 4), 'ice', T3, {}),
        ('hole', 'ice', (0, 0, 1), 'ice', T3, {}),
        ('rod', 'wood', (2, 3, 4), 'wood', (0.50, 0.74), {}),
        ('rod_tip', 'fabric_rose', (1, 2, 3), 'rose', (0.44, 0.70), {}),
        ('line', 'fabric_cream', (3, 4, 4), 'cream', (0.46, 0.70), dict(thin=0.30, prio=5)),
        ('bobber', 'fabric_rose', (1, 2, 3), 'rose', (0.44, 0.70), {}),
        ('fish', 'fish', (1, 2, 3), 'fish', T3, {}),
        ('bed_frame', 'wood', (2, 3, 4), 'wood', (0.50, 0.74), {}),
        ('mattress', 'fabric_cream', (2, 3, 4), 'cream', (0.46, 0.70), {}),
        ('quilt', 'fabric_quilt', (1, 2, 3), 'quilt', (0.46, 0.70), {}),
        ('cap', 'fabric_rose', (1, 2, 3), 'rose', (0.44, 0.70), {}),
        ('cap_trim', 'fabric_cream', (2, 3, 4), 'cream', (0.46, 0.70), {}),
        ('quilt_hem', 'fabric_cream', (2, 3, 4), 'cream', (0.46, 0.70), {}),
        ('quilt_lip', 'fabric_cream', (2, 3, 4), 'sheet', (0.46, 0.70), dict(thin=0.30, prio=5))]):
    PARTS[_n] = dict(pid=50 + _i, ramp=_r, idx=_x, mg=_mg, thresh=_th, **_extra)

# layers (= props) per set; the static ones (pot, ice, bed frame) never move (verify_out.py checks it)
LAYERS = {'plant': ['pot', 'can'], 'sweep': ['broom'], 'snowman': ['snow'], 'fishing': ['ice', 'rod'],
          'bed_cap': ['bed', 'cap']}
PROPS = {
    'pot': dict(set='plant', layers=['pot'], footprint=dict(x=[-0.26, 0.26], y=[-0.26, 0.26]),
                note='a terracotta pot with a two-leaf sprout (the sprout perks up when watered)'),
    'can': dict(set='plant', layers=['can'], note='a small toy watering can (+ the water drops it pours)'),
    'broom': dict(set='sweep', layers=['broom'], note='a toy broom, wood stick and straw head (+ dust puffs)'),
    'snow': dict(set='snowman', layers=['snow'], note='a snow patch with a mound (+ the snowman built on it)'),
    'ice': dict(set='fishing', layers=['ice'], note='an ice patch with a fishing hole (+ the bobber and a fish)'),
    'rod': dict(set='fishing', layers=['rod'], note='a toy fishing rod (+ its line while fishing)'),
    'bed': dict(set='bed_cap', layers=['bed'], footprint=dict(x=[-1.57, 1.57], y=[-0.88, 0.88]),
                note='a small toy bed in Stage B\'s look (wood frame, cream mattress and pillow, blue quilt)'),
    'cap': dict(set='bed_cap', layers=['cap'], note='a rose nightcap with a cream trim and pom-pom'),
}

# ------------------------------------------------------------------ geometry (ori A, set frame, H = origin)
POT = (1.05, 0.62)                 # potted sprout centre on the floor
POT_TOP = 0.40
CAN_HOME = (0.20, 0.82, 0.33, 90.0)       # can grip point (handle) on the floor, spout toward SE
SPOUT = (0.0, -0.40, -0.04)               # spout tip in the can's object frame (grip = origin, spout along -Y)
BROOM_HOME = (-0.38, 0.84, 0.075)         # the broom lying on the floor, stick along +X, head at the +X end
BROOM_HEAD = (0.0, 0.0, -0.98)            # broom head centre in its object frame (stick along -Z)
MOUND = (1.0, 0.42)
HOLE = (1.38, 0.30)
ROD_HOME = (-0.10, 0.80, 0.03)
ROD_TIP = (0.0, -0.66, 0.10)
BL, BW, Z_M = 1.40, 0.80, 0.46            # bed: mattress half length (along X), half width, mattress top
LIE = -64.0                               # tumble lying back onto the pillow
BED_HAND = (0.12, -1.66)                  # hand-off spot beside the bed (the bed origin = the bed centre)


def geometry(s, ppu, pxz):
    import stagekit
    if s == 'bed_cap':
        hx, hy, hpx = stagekit.snap(ppu, *BED_HAND)
    else:
        hx, hy, hpx = 0.0, 0.0, (0, 0)
    return dict(PXZ=pxz, ppu=ppu, hand=[hx, hy], hand_px=list(hpx))


def handoff(s, g):
    a, _, b = SETS[s]['clips']
    return {a: dict(at='start', pos=list(g['hand']), yaw=HAND_YAW), b: dict(at='end', pos=list(g['hand']), yaw=HAND_YAW)}


def _ease(t): return t * t * (3 - 2 * t)


def build(s, g):
    out = globals()['_b_' + s](g)
    for c, n in SETS[s]['counts'].items(): assert len(out[c]) == n, (c, len(out[c]), n)
    return out


def _F(g, st, base=None, **kw):
    q = dict(REST_S, X=g['hand'][0], Y=g['hand'][1], yaw=HAND_YAW)
    if base: q.update(base)
    q.update(kw); return (q, json.loads(json.dumps(st)))


HOME = ['home']


# ---------------------------------------------------------------- plant
def _b_plant(g):
    PXZ = g['PXZ']
    def S(can, drops=(-1, -1, -1), sprout=(0, 0, 1.0)):
        return {'can': can, 'sprout': ['at', POT[0], POT[1], POT_TOP, sprout[0], sprout[1], 0.0, sprout[2]],
                'drop0': ['fx', drops[0]], 'drop1': ['fx', drops[1]], 'drop2': ['fx', drops[2]]}
    P = lambda t, tilt=0.0: ['paw', t, tilt, 0.0, 0.0]
    POUR = dict(armL=42.0, spL=16.0, earL=1, earR=1)
    TILT = 66.0                                   # the grip turn that tips the spout down at POUR
    W0 = lambda **kw: _F(g, S(P(1, TILT)), POUR, **kw)
    get = [
        _F(g, S(HOME)),
        _F(g, S(HOME), lean=6.0, roll=-4.0, spL=10.0, earL=2, earR=2, clX=3),                  # eyes on the can
        _F(g, S(HOME), lean=14.0, roll=-10.0, spL=22.0, armL=-8.0, bob=-PXZ, earL=3, clX=5),    # reach down
        _F(g, S(P(0.35)), lean=18.0, roll=-13.0, spL=26.0, armL=-10.0, bob=-2 * PXZ, sz=0.97, clX=6),
        _F(g, S(P(0.75)), lean=16.0, roll=-11.0, spL=24.0, armL=-6.0, bob=-2 * PXZ, sz=0.97, earL=-2),  # grab
        _F(g, S(P(1.0)), lean=10.0, roll=-7.0, spL=20.0, armL=4.0, bob=-PXZ, clX=-3),           # lift it
        _F(g, S(P(1, 10)), lean=4.0, roll=-3.0, spL=18.0, armL=16.0, earL=2, earR=2, clY=3),
        _F(g, S(P(1, 30)), spL=17.0, armL=28.0, sz=1.02, earL=3, earR=2, clY=-3),                # up!
        _F(g, S(P(1, 48)), spL=16.0, armL=36.0, earL=2, earR=1, clX=3),
        _F(g, S(P(1, 60)), spL=16.0, armL=40.0, earL=1, earR=1, clX=-2),
        _F(g, S(P(1, 64)), spL=16.0, armL=42.0, earL=1, earR=1, clY=1),
        W0(),
    ]
    water = []
    for f in range(30):
        ph = (f % 15) / 15.0                       # two pour cycles
        tilt = TILT + 10 * math.sin(2 * math.pi * ph)
        drops = []
        for k, starts in enumerate(((3, 15), (7, 19), (11,))):     # each drop falls for 8 frames
            p = [(f - a) / 8.0 for a in starts if 0 <= f - a < 8]
            drops.append(round(p[0], 4) if p else -1)
        perk = 1.0 + 0.10 * math.sin(math.pi * min(1.0, max(0.0, (f - 6) / 20.0)))
        wig = 8 * math.sin(2 * math.pi * f / 10.0) if 6 <= f <= 25 else 0.0
        hb = -1 if f in (8, 9, 10, 23, 24, 25) else 0
        ears = dict(earL=1 + 2 * math.sin(2 * math.pi * f / 15), earR=1 - 2 * math.sin(2 * math.pi * f / 15))
        water.append(_F(g, S(P(1, round(tilt, 3)), drops, (round(wig, 3), 0, round(perk, 4))),
                        dict(POUR, **ears), hb=hb, clX=round(3 * math.sin(2 * math.pi * f / 30), 3),
                        bob=-PXZ if f in (12, 13, 14, 27) else 0.0))
    water[0] = W0()
    put = [W0()] + list(reversed(get[:-1]))
    return {'plant_get': get, 'plant_water': water, 'plant_put': put}


# ---------------------------------------------------------------- sweep
def _b_sweep(g):
    PXZ = g['PXZ']
    def S(br, dust=(0, 0)): return {'broom': br, 'dust0': ['fx', dust[0]], 'dust1': ['fx', dust[1]]}
    P = lambda t, swish=0.0, tilt=0.0: ['paw', t, tilt, round(swish, 3), 0.0]
    HOLD = dict(armL=14.0, spL=12.0)
    get = [
        _F(g, S(HOME)),
        _F(g, S(HOME), lean=6.0, roll=-5.0, spL=12.0, earL=2, earR=2, clX=3),
        _F(g, S(HOME), lean=14.0, roll=-12.0, spL=24.0, armL=-10.0, bob=-PXZ, earL=3, clX=5),
        _F(g, S(P(0.3)), lean=18.0, roll=-14.0, spL=28.0, armL=-12.0, bob=-2 * PXZ, sz=0.97, clX=6),
        _F(g, S(P(0.7)), lean=15.0, roll=-11.0, spL=24.0, armL=-6.0, bob=-2 * PXZ, sz=0.97, earL=-2),
        _F(g, S(P(1.0)), lean=9.0, roll=-7.0, spL=18.0, armL=4.0, bob=-PXZ, clX=-3),
        _F(g, S(P(1.0)), lean=3.0, roll=-2.0, spL=14.0, armL=10.0, earL=2, earR=2, clY=3),       # upright, a look
        _F(g, S(P(1.0)), **HOLD, sz=1.02, earL=3, earR=3, clY=-3),
        _F(g, S(P(1.0, 8)), **HOLD, roll=2.0, earL=1, earR=2, clX=3),
        _F(g, S(P(1.0, 4)), **HOLD, roll=1.0, earL=1, earR=1, clX=-2),
        _F(g, S(P(1.0, 1)), **HOLD, clY=1),
        _F(g, S(P(1.0)), **HOLD),
    ]
    sw = []
    for f in range(24):
        a = math.sin(2 * math.pi * f / 12.0)                      # two swishes a loop
        d0 = (f % 12) / 6.0 if 2 <= f % 12 <= 7 else 0.0          # a puff after each swing out
        d1 = ((f - 3) % 12) / 6.0 if 5 <= f % 12 <= 10 else 0.0
        sw.append(_F(g, S(P(1.0, 26 * a, 0.0), (round(min(d0, 1.0), 3), round(min(d1, 1.0), 3))), HOLD,
                     roll=round(5 * a, 3), spL=round(12 + 8 * a, 3), lean=4.0 * abs(a),
                     earL=round(2 * a, 3), earR=round(-2 * a, 3), clX=round(4 * a, 3),
                     hb=-1 if f % 12 in (3, 9) else 0))
    sw[0] = get[-1]
    put = [get[-1]] + list(reversed(get[:-1]))
    return {'sweep_get': get, 'sweep': sw, 'sweep_put': put}


# ---------------------------------------------------------------- snowman
SB = dict(base=(0.0, 0.0, 0.22), mid=(0.0, 0.0, 0.54), head=(0.0, 0.0, 0.79))


def _b_snowman(g):
    PXZ = g['PXZ']
    def S(base=0.0, mid=0.0, head=0.0, hoff=(0, 0, 0), hrot=0.0, nose=0.0):
        mx, my = MOUND
        def at(k, s, off=(0, 0, 0), r=0.0):
            p = SB[k]
            return ['at', mx + p[0] + off[0], my + p[1] + off[1], p[2] * max(s, 0.35) + off[2] if k != 'base' else p[2] * s,
                    r, 0.0, 0.0, round(s, 4)]
        st = {'sbase': at('base', base), 'smid': at('mid', mid), 'shead': at('head', head, hoff, hrot)}
        st['shead'].append(round(nose, 4))
        return st
    SQ = dict(bob=-5 * PXZ, sz=0.98, fL=0.10, fR=0.10, hp=4.5, armL=22.0, armR=22.0)
    K0 = lambda **kw: _F(g, S(), SQ, **kw)
    kneel = [
        _F(g, S()),
        _F(g, S(), earL=2, earR=2, clX=3, hp=1.2),                                              # spots the snow
        _F(g, S(), bob=-PXZ, sz=0.98, hp=2.4, armL=8.0, armR=8.0, earL=3, earR=3, clY=3),
        _F(g, S(), bob=-2 * PXZ, sz=0.98, hp=3.0, armL=12.0, armR=12.0, fL=0.04, fR=0.04, clX=-3),
        _F(g, S(), bob=-4 * PXZ, sz=0.98, hp=3.6, armL=16.0, armR=16.0, fL=0.08, fR=0.08, earL=-2, earR=-2),
        _F(g, S(), bob=-6 * PXZ, sz=0.98, hp=4.2, armL=20.0, armR=20.0, fL=0.10, fR=0.10, clX=4),   # a soft plop
        _F(g, S(), SQ, bob=-5 * PXZ, earL=2, earR=2, clX=-3),
        _F(g, S(), SQ, armL=34.0, earL=3, clY=3),                                               # a test pat
        _F(g, S(), SQ, armL=24.0, earL=1, clY=-2),
        _F(g, S(), SQ, armR=34.0, earR=3, clX=2),
        _F(g, S(), SQ, armR=24.0, earR=1, clX=-1),
        K0(),
    ]
    b = []
    def pat(f): return dict(armL=38.0, armR=22.0) if f % 4 == 1 else dict(armL=22.0, armR=38.0) if f % 4 == 3 else {}
    for f in range(48):
        base = mid = head = nose = 0.0; hoff = (0, 0, 0); hrot = 0.0; kw = {}
        if f <= 16: base = 0.0 if f < 4 else _ease(min(1.0, (f - 3) / 12.0))
        else: base = 1.0
        if 17 <= f: mid = [0.5, 0.85, 1.08, 1.0][min(f - 17, 3)]
        if 21 <= f: head = [0.5, 0.85, 1.10, 1.0][min(f - 21, 3)]
        if 25 <= f: nose = [0.5, 1.0][min(f - 25, 1)]
        if f <= 24: kw = pat(f)
        if 27 <= f <= 34:                                          # clap, looking up at the snowman
            kw = dict(armL=48.0 if f % 2 else 40.0, armR=48.0 if f % 2 else 40.0, spL=-8.0 if f % 2 else 0.0,
                      spR=-8.0 if f % 2 else 0.0, hp=1.2, earL=4 if f % 2 else 1, earR=4 if f % 2 else 1,
                      sz=1.0 if f % 2 else 0.98, mouth='open' if f in (28, 29, 32, 33) else 'closed')
        if 36 <= f <= 40:                                          # the head plops off to the side
            t = (f - 35) / 5.0
            hoff = (0.10 * t, 0.30 * t, -0.62 * t * t); hrot = 80 * t
            kw = dict(hp=5.4, hr=10.0 * t, earL=-3, earR=-3, clX=-4)
        if 41 <= f:                                                # ... and it all puffs back into the mound
            t = _ease((f - 40) / 7.0); hoff = (0.10, 0.30, -0.62); hrot = 80.0
            base = mid = head = round(1.0 - t, 4); nose = head
            kw = dict(hp=4.5, hr=round(10.0 * (1 - t), 3), earL=round(2 * (1 - t), 3), earR=round(2 * (1 - t), 3))
        if f == 35: kw = dict(hp=3.0, earL=2, earR=2, clY=3)
        q = dict(SQ); q.update(kw)
        b.append(_F(g, S(base, mid, head, hoff, hrot, nose), q))
    b[0] = K0()
    stand = [K0(), _F(g, S(), SQ, earL=2, earR=2, clY=2),
             _F(g, S(), SQ, bob=-6 * PXZ, armL=10.0, armR=10.0, earL=-2, earR=-2),           # push up
             _F(g, S(), bob=-4 * PXZ, sz=0.98, hp=3.0, armL=4.0, armR=4.0, fL=0.06, fR=0.06, clX=3),
             _F(g, S(), bob=-2 * PXZ, sz=0.99, hp=1.8, fL=0.03, fR=0.03, earL=2, earR=2),
             _F(g, S(), bob=PXZ, sz=1.03, hp=0.6, armL=-6.0, armR=-6.0, earL=3, earR=3, clX=-4),  # pop up
             _F(g, S(), sz=0.98, earL=-1, earR=-1, clX=2),
             _F(g, S(), earL=1, earR=1, clY=-2),
             _F(g, S(), roll=3.0, hr=4.0, earL=2, clX=2),                                       # a proud look back
             _F(g, S(), roll=1.0, hr=2.0, earR=1, clX=-1),
             _F(g, S(), clY=1),
             _F(g, S())]
    return {'snow_kneel': kneel, 'snow_build': b, 'snow_stand': stand}


# ---------------------------------------------------------------- fishing
def _b_fishing(g):
    PXZ = g['PXZ']
    def S(rod, line=0, bob=0.0, fish=None):
        st = {'rod': rod, 'line': ['fx', line],
              'bobber': ['at', HOLE[0], HOLE[1], round(0.025 + bob, 4), 0.0, 0.0, 0.0, 1.0 if line else 0.0]}
        if fish is None: st['fish'] = ['at', HOLE[0], HOLE[1], -0.2, 0.0, 0.0, 0.0, 0.0]
        else: st['fish'] = ['at', HOLE[0] + fish[0], HOLE[1] + fish[1], fish[2], 0.0, fish[3], fish[4], 1.0]
        return st
    P = lambda t, tilt=0.0: ['paw', t, tilt, 0.0, 0.0]
    # the shared plush floor sit (anim_clips.SIT_LEG / SIT_BOB): legs out in front, seat on the ice, pads on the legs
    # fish_wait is a face 'layers' clip: its idle face patches only match a body moved by whole pixels (as clips_p.py)
    SIT_B = round(SIT_BOB / PXZ) * PXZ
    SIT = dict(bob=SIT_B, legL=SIT_LEG, legR=SIT_LEG, armL=18.0, armR=12.0)
    DOWN = dict(bob=round(SIT_BOB * 0.45 / PXZ) * PXZ, legL=34.0, legR=28.0)          # half way down (or up): the legs come forward
    W = lambda st, **kw: _F(g, st, SIT, **kw)
    W0 = W(S(P(1.0), 1))
    sit = [
        _F(g, S(HOME)),
        _F(g, S(HOME), bob=-2 * PXZ, sz=0.97, armL=-6.0, armR=-6.0, earL=2, earR=2),             # a little dip
        _F(g, S(HOME), DOWN, armL=-10.0, armR=-10.0, earL=-3, earR=-3, clX=4),
        _F(g, S(HOME), SIT, bob=SIT_B - PXZ, legL=SIT_LEG - 8, legR=SIT_LEG - 8, sz=0.95, armL=12.0, earL=4, earR=4,
           clX=-5),                                                                               # plop: soft squash
        _F(g, S(HOME), SIT, sz=0.97, legL=SIT_LEG + 6, legR=SIT_LEG + 6, armL=12.0, earL=-2, earR=-2, clX=3),  # legs bounce
        _F(g, S(HOME), SIT, armL=-4.0, spL=30.0, roll=-8.0, hr=-6.0, earL=2, clX=4),            # reach for the rod
        _F(g, S(P(0.5)), SIT, armL=-6.0, spL=34.0, roll=-10.0, hr=-6.0, earL=3, clX=5),
        _F(g, S(P(1.0)), SIT, armL=4.0, spL=22.0, roll=-5.0, hr=-3.0, clX=-3),                  # got it
        _F(g, S(P(1.0, -10)), SIT, armL=14.0, spL=8.0, earL=2, earR=2, clY=3),
        W(S(P(1.0), 1, 0.06), armL=22.0, earL=1, earR=1, clY=-3),                                # cast: plop
        W(S(P(1.0), 1, -0.02), earL=-1, earR=-1, clX=2),
        W0,
    ]
    wait = []
    for f in range(36):
        bob = 0.025 * math.sin(2 * math.pi * f / 12.0)
        fish = None; kw = {}
        if 14 <= f <= 22:                                          # a fish pops out of the hole and plops back
            t = (f - 14) / 8.0
            fish = (0.08 * t, -0.10 * t, round(-0.10 + 0.95 * math.sin(math.pi * t), 4), 0.0, round(-60 + 120 * t, 3))
            kw = dict(earL=6, earR=6, clY=5, hb=1 if 16 <= f <= 20 else 0)                       # ears perk
        if f in (23, 24): kw = dict(earL=3, earR=3, clY=-3)
        if f in (4, 5, 6): kw = dict(hb=-1)                                                       # a slow breath
        if f in (28, 29, 30): kw = dict(hb=-1, earL=-2, earR=-2)
        if f in (8, 9, 10, 11): kw = dict(legL=SIT_LEG + [8, 16, 8, 0][f - 8],                  # a little leg swing
                                          legR=SIT_LEG + [0, -6, 4, 0][f - 8])
        arm = 18.0 + (2.0 if bob > 0.015 else -2.0 if bob < -0.015 else 0.0)
        wait.append(W(S(P(1.0), 1, round(bob, 4), fish), armL=arm, **kw))
    wait[0] = W0
    stand = [W0, W(S(P(1.0), 1, 0.03), armL=16.0, earL=2, earR=2, clY=2),                        # reel in
             _F(g, S(P(1.0, -10)), SIT, armL=10.0, spL=10.0, clX=3),
             _F(g, S(P(1.0)), SIT, armL=2.0, spL=22.0, roll=-5.0, hr=-3.0, clX=-3),
             _F(g, S(P(0.5)), SIT, armL=-6.0, spL=34.0, roll=-10.0, hr=-6.0, earL=3, clX=5),      # rod down
             _F(g, S(HOME), SIT, armL=-4.0, spL=28.0, roll=-7.0, hr=-5.0, earL=2, clX=4),
             _F(g, S(HOME), SIT, legL=SIT_LEG - 12, legR=SIT_LEG - 12, sz=0.96, armL=-14.0, armR=-14.0, earL=2,
                earR=2),                                                                          # mittens push
             _F(g, S(HOME), DOWN, legL=30.0, legR=36.0, armL=-10.0, armR=-10.0, earL=-3, earR=-3, clX=4),
             _F(g, S(HOME), bob=2 * PXZ, sz=1.03, armL=-4.0, armR=-4.0, hL=0.04, hR=0.04, earL=3, earR=3, clX=-4),  # pop up
             _F(g, S(HOME), sz=0.98, earL=-1, earR=-1, clX=2),
             _F(g, S(HOME), clY=-1),
             _F(g, S(HOME))]
    return {'fish_sit': sit, 'fish_wait': wait, 'fish_stand': stand}


# ---------------------------------------------------------------- bed_cap
SIT_Z = Z_M + 0.045                 # the shared sit (SIT_BOB): the seat's lowest point (0.197 below the root) sinks
                                    # 0.15 into the soft mattress (as the old sit did), so the art keeps its 152x148 canvas
LIE_Z = Z_M - 0.175                 # tumble -64, legs LEG_LIE: the back's lowest point (0.135 above the root) sinks 0.04 in
LIE_X = -BL + 0.05 + 1.47           # the clover tip (1.47 behind the root when lying) stops short of the headboard
LEG_LIE = 26.0                      # lying: the legs (swung 26 deg against the -64 tumble) lie along the mattress
ARM_LIE, SP_LIE = -6.0, 0.0         # lying: the arms
Q_HOME = {'quilt_a': ['box', 1.02, 0.0, Z_M + 0.16, 0.28, BW - 0.02, 0.05],
          'quilt_b': ['box', 1.02, 0.0, Z_M + 0.07, 0.33, BW + 0.04, 0.08]}
Q_UP = {'quilt_a': ['box', 0.66, 0.0, Z_M - 0.02, 0.72, BW + 0.04, 0.08],      # where the folded halves head as the
        'quilt_b': ['box', 1.18, 0.0, Z_M + 0.02, 0.20, BW + 0.05, 0.11]}      # bear takes hold (they loosen a little)
Q_PULL = 0.2                         # from here on the folded halves are gone and the quilt is the drape
DRAPE_H = (0.30, 1.20)               # its turned-down edge travels from the seat up to across the chest
# the drape (bed frame, see _drape): a height field over the foot part of the bed. cell = grid step; gap = how far
# it lies off the bear, all round (>= the bear's 1 px outline; gap_min: never less); thick = its thickness on the bare
# mattress; soft = the radius of the ball that cannot get under it (it tents between the bumps and meets the mattress
# in a fillet of that radius; a plain slope limit instead makes it a pitched tent); puff = blur passes that take the
# creases out of it; over / skirt = it hangs `over` past the mattress side, down to z = skirt; round = the radius of
# that edge; voxel / relax = the even remesh of the cloth (face size, smoothing passes); hem = the width of the cream
# turned-down band, lift = how far it lies above the quilt, lip = the sheet edge showing beyond it where it lies on
# the bear (within lip_r of the bear; the pixel step inks the lip along the band: the line between the hem and the
# bear)
DRAPE = dict(x0=-0.62, cell=0.0125, gap=0.06, gap_min=0.04, thick=0.05, soft=0.26, over=0.06, skirt=0.30,
             puff=50, round=0.05, voxel=0.02, relax=3, hem=0.13, lift=0.02, lip=0.04, lip_r=0.10)
CAP_HOME = (-(BL - 0.36), 0.0, Z_M + 0.52)   # the nightcap lying on the pillow (bed frame: head end at -X)


def _quilt(t, br=0.0, out=0):
    """t: 0 = folded at the foot (home), 1 = pulled up to the chest. Below Q_PULL the folded halves only loosen;
    from Q_PULL on (only on frames where the bear lies flat) they are hidden and the quilt is the drape laid over
    the bear, its edge rising from the seat to the chest (out = 1: the arms come out over it)."""
    res = {}
    for k in ('quilt_a', 'quilt_b'):
        if t >= Q_PULL: res[k] = ['hide']; continue
        a, b = Q_HOME[k], Q_UP[k]; e = _ease(min(1.0, t / 0.6))
        v = [round(a[i] + (b[i] - a[i]) * e, 4) for i in range(1, 7)]
        v[5] = round(v[5] * (1 + br), 4)
        res[k] = ['box'] + v
    h = 0.0 if t < Q_PULL else DRAPE_H[0] + (DRAPE_H[1] - DRAPE_H[0]) * _ease((t - Q_PULL) / (1 - Q_PULL))
    res['quilt_m'] = ['drape', round(h, 4), int(out)]
    return res


def _b_bed_cap(g):
    PXZ = g['PXZ']; hx, hy = g['hand']
    edgeY = -BW + 0.30
    def S(cap=HOME, quilt=0.0, br=0.0, nod=0.0, out=0):
        st = dict(_quilt(quilt, br, out)); st['nightcap'] = cap if cap is HOME else ['head', cap, 0.0, round(nod, 3), 0.0]
        if cap is not HOME and cap < 1.0: st['nightcap'] = ['head', cap, 0.0, 0.0, 0.0]
        return st
    lerp = lambda a, b, t: a + (b - a) * t
    # sitting on the mattress: the shared plush sit (legs out in front, pads on the legs), the seat sunk SIT_Z
    SIT = dict(Z=SIT_Z, bob=SIT_BOB, legL=SIT_LEG, legR=SIT_LEG, armL=10.0, armR=10.0)
    IN = dict(SIT, X=LIE_X, Y=0.0, yaw=90.0)
    LY = dict(IN, Z=LIE_Z, tumble=LIE, legL=LEG_LIE, legR=LEG_LIE, armL=ARM_LIE, armR=ARM_LIE, spL=SP_LIE,
              spR=SP_LIE, blink=0.12)
    def tl(t, **kw):                                    # between sitting in bed and lying
        q = dict(IN, Z=lerp(SIT_Z, LIE_Z, t), tumble=LIE * t, legL=lerp(SIT_LEG, LEG_LIE, min(t, 1.0)),
                 legR=lerp(SIT_LEG, LEG_LIE, min(t, 1.0)))
        q.update(kw); return q
    L0 = _F(g, S(1.0, 1.0), LY)
    gib = [
        _F(g, S()),
        _F(g, S(), yaw=45.0, earL=2, earR=2, clX=3),                                             # turn: back to the bed
        _F(g, S(), yaw=0.0, sz=0.96, bob=-2 * PXZ, armL=-10.0, armR=-10.0, earL=2, earR=2),     # crouch
        _F(g, S(), yaw=0.0, Y=lerp(hy, edgeY, 0.5), Z=0.40, sz=1.03, armL=25.0, armR=25.0, legL=22.0, legR=22.0,
           earL=-3, earR=-3, clX=-5),                                                            # hop! legs tuck a little
        _F(g, S(), SIT, yaw=0.0, Y=edgeY, sz=0.95, legL=SIT_LEG - 15, legR=SIT_LEG - 15, armL=10.0, armR=10.0,
           earL=3, earR=3, clX=6),                                                               # plop on the edge
        _F(g, S(), SIT, yaw=0.0, Y=edgeY, sz=1.01, legL=SIT_LEG + 6, legR=SIT_LEG + 6, clX=-3),  # the legs bounce
        _F(g, S(), SIT, yaw=35.0, X=lerp(hx, LIE_X, 0.5), Y=lerp(edgeY, 0.0, 0.5), legL=SIT_LEG + 8, legR=SIT_LEG - 4,
           earL=2, clY=3),                                                                       # swing the legs in
        _F(g, S(), SIT, yaw=70.0, X=lerp(hx, LIE_X, 0.85), Y=lerp(edgeY, 0.0, 0.85), legL=SIT_LEG - 4,
           legR=SIT_LEG + 8, earR=2, clY=-3),
        _F(g, S(), IN, earL=3, earR=3, clY=3),                                                   # sitting in bed
        _F(g, S(), IN, sz=1.02, legL=SIT_LEG + 12, legR=SIT_LEG - 6, earL=-1, earR=-1, clY=-2, mouth='open'),  # happy wiggle
        _F(g, S(), tl(0.40, armL=-8.0, armR=-8.0, clX=-4)),                                      # lie back
        _F(g, S(0.4), tl(0.80, armL=-6.0, armR=-6.0, clX=-6)),
        _F(g, S(0.8, 0.12), tl(1.0, Z=LIE_Z - 0.02, armL=ARM_LIE, armR=ARM_LIE, spL=SP_LIE, spR=SP_LIE, clX=-4,
                               earL=3, earR=3)),                                                 # cap pops on, sink in
        _F(g, S(1.0, 0.45), LY, blink=1.0, clX=3, earL=1, earR=1),                               # the quilt comes up
        _F(g, S(1.0, 0.8), LY, blink=1.0, clX=-2),
        L0,
    ]
    sleep = []
    for f in range(36):
        b = math.sin(2 * math.pi * f / 18.0)                       # two slow breaths
        kw = dict(sz=round(1.0 + 0.02 * max(b, 0.0), 4))
        if 21 <= f <= 24: kw.update(earL=[5, -4, 3, 0][f - 21])                       # an ear twitch
        if 10 <= f <= 13: kw.update(clX=[3, -3, 2, 0][f - 10])
        nod = 10 * math.sin(2 * math.pi * f / 36.0)                # the cap tip nods once a loop
        sleep.append(_F(g, S(1.0, 1.0, round(0.03 * max(b, 0.0), 4), nod), LY, **kw))
    sleep[0] = L0
    wake = [
        L0,
        _F(g, S(1.0, 1.0), LY, blink=1.0, earL=2, earR=2),                                       # eyes open
        _F(g, S(1.0, 1.0, out=1), LY, blink=1.0, armL=30.0, armR=30.0, spL=0.0, spR=0.0, sz=1.03, mouth='wide', earL=4,
           earR=4),                                                                              # stretch, yawn
        _F(g, S(1.0, 0.5), LY, blink=1.0, clX=4, earL=2, earR=2),                                 # push the quilt down
        _F(g, S(0.6, 0.1), tl(0.55, armL=18.0, armR=18.0, clX=6)),                                # sit up, the cap hops off
        _F(g, S(0.2), tl(0.15, armL=12.0, armR=12.0, clX=-3)),
        _F(g, S(), IN, earL=2, earR=2, clY=3),
        _F(g, S(), SIT, yaw=60.0, X=lerp(hx, LIE_X, 0.85), Y=lerp(edgeY, 0.0, 0.85), legL=SIT_LEG + 8, legR=SIT_LEG - 4,
           clY=-3),                                                                              # swing the legs out
        _F(g, S(), SIT, yaw=25.0, X=lerp(hx, LIE_X, 0.5), Y=lerp(edgeY, 0.0, 0.5), legL=SIT_LEG - 4, legR=SIT_LEG + 8,
           earL=2),
        _F(g, S(), SIT, yaw=0.0, Y=edgeY, earL=1, earR=1),
        _F(g, S(), SIT, yaw=0.0, Y=edgeY, sz=0.97, legL=SIT_LEG - 25, legR=SIT_LEG - 25, armL=-10.0, armR=-10.0,
           lean=6.0),                                                                            # scoot, legs down
        _F(g, S(), yaw=0.0, Y=lerp(edgeY, hy, 0.55), Z=0.30, sz=1.03, armL=30.0, armR=30.0, legL=20.0, legR=20.0,
           earL=-3, earR=-3, clX=5),                                                              # hop off
        _F(g, S(), yaw=0.0, sz=0.95, armL=8.0, armR=8.0, earL=3, earR=3, clX=6),                  # land
        _F(g, S(), yaw=0.0, sz=1.01, clX=-3),
        _F(g, S(), yaw=45.0, earL=1, earR=1, clY=2),                                               # turn back
        _F(g, S()),
    ]
    return {'cap_get_in_bed': gib, 'cap_sleep': sleep, 'cap_wake_up': wake}


# ------------------------------------------------------------------ props (Blender only)
_R = {}


def _part(kit, name, parent, layer, part, shapes):
    return kit.solid(name, parent, layer, PARTS[part]['pid'], shapes)


def build_props(s, kit, g):
    """kit (render_set.py): empty(name, loc, parent), solid(name, parent, layer, pid, shapes), W0 (the set root).
    Each rigid is an empty under W0 holding its solids in its own object frame; _apply places it per frame."""
    W0 = kit.W0; rig = {}; info = {}
    def E(name, layer, home, kind='at', host=None):
        e = kit.empty('P_' + name, (0, 0, 0), W0); rig[name] = dict(ob=e, layer=layer, home=home, kind=kind, host=host)
        return e
    if s == 'plant':
        P = kit.empty('P_pot', (POT[0], POT[1], 0), W0)
        _part(kit, 'pot', P, 'pot', 'pot', [((0, 0, 0.17), (0.20, 0.20, 0.17), 0.45, 1.0)])
        _part(kit, 'pot_rim', P, 'pot', 'pot_rim', [((0, 0, 0.35), (0.25, 0.25, 0.06), 0.4, 1.0)])
        _part(kit, 'soil', P, 'pot', 'soil', [((0, 0, 0.38), (0.19, 0.19, 0.03), 0.5, 1.0)])
        sp = E('sprout', 'pot', ['at', POT[0], POT[1], POT_TOP, 0, 0, 0, 1.0])
        _part(kit, 'sprout', sp, 'pot', 'leaf', [((0, 0, 0.10), (0.035, 0.035, 0.11), 0.8, 1.0),
                                                 ((0.11, 0.0, 0.22), (0.12, 0.06, 0.07), 0.8, 1.0),
                                                 ((-0.10, 0.0, 0.25), (0.11, 0.06, 0.07), 0.8, 1.0)])
        c = E('can', 'can', ['atyaw', CAN_HOME], 'paw')
        _part(kit, 'can', c, 'can', 'can', [((0, 0.03, -0.19), (0.17, 0.19, 0.15), 0.5, 0.8),
                                            ((0, 0.12, -0.04), (0.04, 0.10, 0.07), 0.6, 0.6)] +
              [((0, -0.17 - 0.06 * k, -0.17 + 0.035 * k), (0.045, 0.05, 0.045), 1.0, 1.0) for k in range(4)])
        _part(kit, 'can_cap', c, 'can', 'can_cap', [((0, -0.40, -0.04), (0.06, 0.04, 0.06), 0.8, 1.0)])
        for k in range(3):
            d = E(f'drop{k}', 'can', None, 'drop', 'can')
            _part(kit, f'drop{k}', d, 'can', 'drop', [((0, 0, 0), (0.07, 0.07, 0.09), 1.0, 1.0)])
        info = {'pot': dict(pos=[POT[0], POT[1], 0.0]), 'can': dict(pos=list(CAN_HOME[:3]))}
    elif s == 'sweep':
        b = E('broom', 'broom', ['atrot', BROOM_HOME, (90.0, 0.0, -90.0)], 'paw')
        _part(kit, 'stick', b, 'broom', 'stick', [((0, 0, -0.30), (0.05, 0.05, 0.58), 1.0, 1.0)])
        _part(kit, 'band', b, 'broom', 'band', [((0, 0, -0.86), (0.11, 0.08, 0.05), 0.6, 0.8)])
        _part(kit, 'straw', b, 'broom', 'straw', [((0, 0, -0.99), (0.21, 0.08, 0.13), 0.55, 0.6)])
        for k in range(2):
            d = E(f'dust{k}', 'broom', None, 'dust', 'broom')
            _part(kit, f'dust{k}', d, 'broom', 'dust', [((0, 0, 0), (0.07, 0.07, 0.06), 1.0, 1.0),
                                                        ((0.06, 0.04, 0.04), (0.05, 0.05, 0.045), 1.0, 1.0)])
        info = {'broom': dict(pos=list(BROOM_HOME))}
    elif s == 'snowman':
        P = kit.empty('P_snow', (MOUND[0], MOUND[1], 0), W0)
        _part(kit, 'snow_patch', P, 'snow', 'snow', [((-0.08, -0.02, -0.045), (0.62, 0.62, 0.025), 0.3, 1.0),
                                                     ((0, 0, 0.0), (0.26, 0.26, 0.11), 0.7, 1.0)])
        for k, r in (('sbase', 0.24), ('smid', 0.17)):
            e = E(k, 'snow', None)
            _part(kit, k, e, 'snow', 'snow', [((0, 0, 0), (r, r, r * 0.95), 1.0, 1.0)])
        e = E('shead', 'snow', None, 'shead')
        _part(kit, 'shead', e, 'snow', 'snow', [((0, 0, 0), (0.13, 0.13, 0.125), 1.0, 1.0)])
        n = kit.empty('P_nose', (0, 0, 0), e); rig['shead']['nose'] = n
        _part(kit, 'nose', n, 'snow', 'carrot', [((0.09, -0.09, -0.01), (0.05, 0.05, 0.04), 1.0, 1.0),
                                                 ((0.15, -0.15, -0.015), (0.035, 0.035, 0.03), 1.0, 1.0)])
        _part(kit, 'coal', n, 'snow', 'coal', [((0.11, 0.02, 0.05), (0.03, 0.03, 0.03), 1.0, 1.0),
                                               ((-0.02, -0.11, 0.05), (0.03, 0.03, 0.03), 1.0, 1.0)])
        info = {'snow': dict(pos=[MOUND[0], MOUND[1], 0.0])}
    elif s == 'fishing':
        P = kit.empty('P_ice', (HOLE[0] - 0.30, HOLE[1] - 0.05, 0), W0)
        _part(kit, 'ice_patch', P, 'ice', 'ice', [((0, 0, -0.045), (0.78, 0.70, 0.025), 0.3, 1.0)])
        _part(kit, 'hole', P, 'ice', 'hole', [((0.30, 0.05, -0.02), (0.20, 0.20, 0.012), 0.5, 1.0)])
        bo = E('bobber', 'ice', None)
        _part(kit, 'bobber', bo, 'ice', 'bobber', [((0, 0, 0.03), (0.065, 0.065, 0.05), 1.0, 1.0)])
        fi = E('fish', 'ice', None)
        _part(kit, 'fish', fi, 'ice', 'fish', [((0, 0, 0), (0.15, 0.07, 0.09), 1.0, 1.0),
                                               ((-0.19, 0, 0), (0.06, 0.03, 0.08), 0.8, 1.0)])
        r = E('rod', 'rod', ['atrot', ROD_HOME, (0.0, 0.0, 100.0)], 'paw')
        _part(kit, 'rod', r, 'rod', 'rod', [((0, -0.28, 0.04), (0.04, 0.40, 0.04), 1.0, 1.0)])
        _part(kit, 'rod_reel', r, 'rod', 'rod_tip', [((0.07, 0.02, -0.02), (0.04, 0.06, 0.06), 1.0, 1.0)])
        _part(kit, 'rod_tip', r, 'rod', 'rod_tip', [(ROD_TIP, (0.045, 0.045, 0.045), 1.0, 1.0)])
        ln = E('line', 'rod', None, 'line', 'rod')
        _part(kit, 'line', ln, 'rod', 'line', [((0, 0, -0.5), (0.018, 0.018, 0.5), 1.0, 1.0)])
        info = {'ice': dict(pos=[HOLE[0] - 0.30, HOLE[1] - 0.05, 0.0]),
                'rod': dict(pos=list(ROD_HOME))}
    elif s == 'bed_cap':
        P = kit.empty('P_bed', (0, 0, 0), W0)                # set origin = the bed centre
        P.rotation_euler = (0, 0, math.radians(90))          # built as Stage B's bed (length along Y, head at +Y)
        L = BL; Wd = BW
        _part(kit, 'bed_base', P, 'bed', 'bed_frame', [((0, 0, 0.18), (Wd - 0.04, L, 0.11), 0.30, 0.20)] +
              [((sx * (Wd - 0.16), sy * (L - 0.16), 0.05), (0.09, 0.09, 0.05), 0.5, 1.0, dict(segs=16, rings=8))
               for sx in (-1, 1) for sy in (-1, 1)])
        _part(kit, 'bed_head', P, 'bed', 'bed_frame', [((0, L + 0.095, 0.62), (Wd + 0.10, 0.075, 0.62), 0.30, 0.50,
                                                        dict(board=True))])
        _part(kit, 'bed_foot', P, 'bed', 'bed_frame', [((0, -L - 0.095, 0.38), (Wd + 0.10, 0.075, 0.38), 0.30, 0.50,
                                                        dict(board=True))])
        _part(kit, 'mattress', P, 'bed', 'mattress', [((0, 0, Z_M - 0.12), (Wd, L, 0.12), 0.40, 0.25)])
        _part(kit, 'pillow', P, 'bed', 'mattress', [((0, L - 0.36, Z_M + 0.20), (Wd - 0.20, 0.28, 0.22), 0.55, 0.40)])
        for k in ('quilt_a', 'quilt_b'):
            e = E(k, 'bed', None, 'box_bed')
            _part(kit, k, e, 'bed', 'quilt', [((0, 0, 0), (1.0, 1.0, 1.0), 0.45, 0.30)])
        e = E('quilt_m', 'bed', None, 'drape')                    # empty meshes, rebuilt per frame by _drape
        for k, part in (('mesh', 'quilt'), ('hem', 'quilt_hem'), ('lip', 'quilt_lip')):
            ob = _part(kit, 'quilt_' + k, e, 'bed', part, [((0, 0, 0), (1.0, 1.0, 1.0), 1.0, 1.0, dict(segs=4, rings=3))])
            ob.data.clear_geometry(); rig['quilt_m'][k] = ob
        import bpy
        src = bpy.data.objects.new('quilt_src', bpy.data.meshes.new('quilt_src'))    # the cloth before remeshing:
        bpy.context.scene.collection.objects.link(src); src.hide_render = True       # never rendered, no layer
        md = src.modifiers.new('even', 'REMESH'); md.mode = 'VOXEL'; md.voxel_size = DRAPE['voxel']; md.adaptivity = 0.0
        rig['quilt_m'].update(src=src, sig=[0.0])
        cap = E('nightcap', 'cap', None, 'head')
        hz = 1.4
        _part(kit, 'cap', cap, 'cap', 'cap', [((0, 0.12, 2.15 - hz), (0.31, 0.30, 0.19), 0.8, 1.0),
                                              ((0.13, 0.16, 2.36 - hz), (0.17, 0.16, 0.15), 1.0, 1.0),
                                              ((0.27, 0.18, 2.36 - hz), (0.10, 0.10, 0.09), 1.0, 1.0)])
        _part(kit, 'cap_trim', cap, 'cap', 'cap_trim', [((0, 0.11, 2.00 - hz), (0.36, 0.34, 0.075), 0.6, 1.0),
                                                        ((0.37, 0.19, 2.27 - hz), (0.085, 0.085, 0.085), 1.0, 1.0)])
        info = {'bed': dict(pos=[0.0, 0.0, 0.0]), 'cap': dict(pos=list(CAP_HOME))}
        _R['bed'] = P
    _R.update(W0=W0, rig=rig, set=s)
    import stagekit
    for v in info.values(): v['px'] = [int(round(c)) for c in stagekit.px_of(g['ppu'], *v['pos'])]
    return info


def _mat_at(x, y, z, rx=0.0, ry=0.0, rz=0.0, s=1.0):
    from mathutils import Matrix, Euler
    s = s if s > 0 else 1e-6
    return (Matrix.Translation((x, y, z)) @ Euler((math.radians(rx), math.radians(ry), math.radians(rz)), 'XYZ')
            .to_matrix().to_4x4() @ Matrix.Diagonal((s, s, s, 1.0)))


def _blend(A, B, t):
    from mathutils import Matrix
    if t <= 0: return A.copy()
    if t >= 1: return B.copy()
    la, ra, sa = A.decompose(); lb, rb, sb = B.decompose()
    if ra.dot(rb) < 0: rb.negate()
    return Matrix.LocRotScale(la.lerp(lb, t), ra.slerp(rb, t), sa.lerp(sb, t))


def _home(r):
    from mathutils import Matrix, Euler
    h = r['home']
    if h is None: return _mat_at(0, 0, -1, s=0.0)
    if h[0] == 'at': return _mat_at(*h[1:])
    if h[0] == 'atyaw': x, y, z, yaw = h[1]; return _mat_at(x, y, z, 0, 0, yaw)
    if h[0] == 'atrot':
        (x, y, z), (a, b, c) = h[1], h[2]
        return Matrix.Translation((x, y, z)) @ Euler((math.radians(a), math.radians(b), math.radians(c)), 'XYZ').to_matrix().to_4x4()
    raise ValueError(h)


def _follow(bone_A, bone_B, extra):
    """W0-local matrix of a rigid modelled at bone_A's rest head (ori A), following the posed bone."""
    import bpy
    from mathutils import Matrix, Euler
    W0 = _R['W0']; B = W0.matrix_world.determinant() < 0
    Cm = Matrix(((0, -1, 0, 0), (-1, 0, 0, 0), (0, 0, 1, 0), (0, 0, 0, 1))) if B else Matrix.Identity(4)
    rig = bpy.data.objects['Rig']; bn = bone_B if B else bone_A
    pb = rig.pose.bones[bn]; bb = rig.data.bones[bn]
    Dw = rig.matrix_world @ pb.matrix @ bb.matrix_local.inverted() @ rig.matrix_world.inverted()
    rest = (rig.matrix_world @ rig.data.bones[bone_A].matrix_local).translation
    Sx = Matrix.Diagonal((-1, 1, 1, 1)) if B else Matrix.Identity(4)
    ex = Euler([math.radians(v) for v in extra], 'XYZ').to_matrix().to_4x4()
    return Cm @ Dw @ Sx @ Matrix.Translation(rest) @ ex


def _apply(st):
    import bpy
    from mathutils import Matrix, Vector
    rig = _R['rig']; M = {}
    order = sorted(rig, key=lambda n: rig[n]['host'] is not None)
    for n in order:
        r = rig[n]; sp = st.get(n, ['home'])
        if r['kind'] == 'drop':                                   # a water drop: spout tip -> pot top, falling
            p = sp[1]
            if p < 0: m = _mat_at(0, 0, -1, s=0.0)
            else:
                a = M['can'] @ Vector(SPOUT); b = Vector((POT[0], POT[1], POT_TOP + 0.06))
                pos = a.lerp(b, p); pos.z = a.z + (b.z - a.z) * p * p
                m = _mat_at(pos.x, pos.y, pos.z)
        elif r['kind'] == 'dust':                                 # a dust puff by the broom head, on the floor
            s = sp[1]; k = int(n[-1])
            hp = M['broom'] @ Vector(BROOM_HEAD)
            off = (0.10, 0.18) if k == 0 else (0.22, -0.04)
            m = _mat_at(hp.x + off[0] * (0.6 + s), hp.y + off[1] * (0.6 + s), 0.07 + 0.08 * s,
                        s=0.0 if s <= 0 or s >= 1 else round(math.sin(math.pi * s), 4))
        elif r['kind'] == 'line':                                 # the fishing line: rod tip -> bobber
            if sp[1]:
                a = M['rod'] @ Vector(ROD_TIP); b = M['bobber'].translation + Vector((0, 0, 0.07))
                d = b - a; q = Vector((0, 0, -1)).rotation_difference(d.normalized())
                m = Matrix.Translation(a) @ q.to_matrix().to_4x4() @ Matrix.Diagonal((1, 1, max(d.length, 1e-3), 1))
            else: m = _mat_at(0, 0, -1, s=0.0)
        elif r['kind'] == 'box_bed':                               # a quilt half: set-frame box
            if sp[0] == 'hide': m = _mat_at(0, 0, -1, s=0.0)
            else:
                _, x, y, z, sx, sy, sz = sp
                m = _bed() @ Matrix.Translation((x, y, z)) @ Matrix.Diagonal((sx, sy, sz, 1.0))
        elif r['kind'] == 'drape':                                 # the pulled-up quilt laid over the lying bear
            m = Matrix.Identity(4)
            r['sig'] = _drape(r, sp[1], bool(sp[2])) if sp[0] == 'drape' else _drape(r, 0.0)
        elif r['kind'] == 'head':                                  # the nightcap: pillow -> head
            home = _bed() @ _mat_at(*CAP_HOME, 0.0, -80.0, 0.0) @ Matrix.Translation((-0.10, -0.14, -0.78))
            if sp[0] == 'home': m = home
            else:
                m = _blend(home, _follow('head', 'head', sp[2:5]), sp[1])
        elif sp[0] == 'hide': m = _mat_at(0, 0, -1, s=0.0)
        elif sp[0] == 'home': m = _home(r)
        elif sp[0] == 'paw': m = _blend(_home(r), _follow('paw_L', 'paw_R', sp[2:5]) @ _grip(n), sp[1])
        elif sp[0] == 'at':
            m = _mat_at(*sp[1:8])
            if n == 'shead' and 'nose' in r:
                ns = sp[8] if len(sp) > 8 else 0.0
                r['nose'].matrix_basis = Matrix.Diagonal((ns if ns > 0 else 1e-6,) * 3 + (1.0,)) if ns < 1 else Matrix.Identity(4)
        else: raise ValueError((n, sp))
        M[n] = m; r['ob'].matrix_basis = m
    bpy.context.view_layer.update()
    keys = {}
    for l in LAYERS[_R['set']]:
        sig = [round(x, 5) for n in sorted(rig) if rig[n]['layer'] == l for row in M[n] for x in row]
        if 'shead' in rig and l == 'snow': sig += [round(x, 5) for row in rig['shead']['nose'].matrix_basis for x in row]
        if 'quilt_m' in rig and l == 'bed': sig += rig['quilt_m']['sig']
        keys[l] = f'{l}_' + hashlib.md5(json.dumps(sig).encode()).hexdigest()[:10]
    return keys


def _drape(r, h, arms_out=False):
    """rebuild the pulled-up quilt for this frame (the rig is already posed) and return its render signature.
    The cloth is a height field z(x, y) in the set frame over the foot part of the bed: it lies `gap` off
    everything of the bear below the edge plane (the plane across the posed body bone at height h), falls from
    there like cloth (a ball of radius `soft` cannot get under it), lies `thick` on the bare mattress and hangs over
    the mattress sides and foot.
    arms_out: the arms are not covered (they come out over the quilt: the stretch). Cut by the edge plane it gives
    three meshes: the quilt, the cream hem band along the cut (lifted off the quilt)
    and the sheet lip just beyond it. Because the cloth covers the bear from above with room to spare, the depth
    composite hides every covered bear pixel, in any pose."""
    import bpy, bmesh
    import numpy as np
    from mathutils import Vector
    from mathutils.bvhtree import BVHTree
    obs = [r['mesh'], r['hem'], r['lip']]
    for ob in obs: ob.data.clear_geometry()
    if h <= 0: return [0.0]
    D = DRAPE; cs = D['cell']
    F = _follow('body', 'body', (0.0, 0.0, 0.0))
    n = Vector(F.col[2][:3]).normalized(); c = F @ Vector((0.0, 0.0, h))
    Wi = np.array(_R['W0'].matrix_world.inverted()); dg = bpy.context.evaluated_depsgraph_get()
    vs, ts, nv = [], [], 0
    for o in bpy.data.collections['Bear'].all_objects:                 # the posed bear, in the set frame
        if o.type != 'MESH' or (arms_out and o.name.startswith('arm_')): continue
        oe = o.evaluated_get(dg); me = oe.to_mesh(); me.calc_loop_triangles()
        co = np.empty(len(me.vertices) * 3, np.float32); me.vertices.foreach_get('co', co)
        tr = np.empty(len(me.loop_triangles) * 3, np.int32); me.loop_triangles.foreach_get('vertices', tr)
        oe.to_mesh_clear()
        M = Wi @ np.array(o.matrix_world)
        vs.append(co.reshape(-1, 3).astype(np.float64) @ M[:3, :3].T + M[:3, 3]); ts.append(tr.reshape(-1, 3) + nv)
        nv += len(vs[-1])
    tree = BVHTree.FromPolygons(np.concatenate(vs).tolist(), np.concatenate(ts).tolist(), all_triangles=True)
    yw = BW + D['over']; x0 = D['x0']; x1 = BL
    nx = int(round((x1 - x0) / cs)); ny = int(round(2 * yw / cs))
    gx = x0 + cs * np.arange(nx + 1); gy = -yw + cs * np.arange(ny + 1)
    # T: per grid point the top of what the quilt covers there = the bear's column below the edge plane
    T = np.full((nx + 1, ny + 1), -1e9); under = np.zeros(T.shape, bool); up, dn = Vector((0.0, 0.0, 1.0)), Vector((0.0, 0.0, -1.0))
    for i in range(nx + 1):
        zp = c.z - (gx[i] - c.x) * n.x / n.z                         # the edge plane's height over this row
        for j in range(ny + 1):
            top = tree.ray_cast(Vector((gx[i], gy[j], 9.0)), dn)[0]
            if top is None: continue
            under[i, j] = True
            bot = tree.ray_cast(Vector((gx[i], gy[j], -9.0)), up)[0]
            if bot.z <= zp: T[i, j] = min(top.z, zp)

    def ball(rad):                                                   # grid offsets within rad + the ball's height there
        m = int(rad / cs)
        return [(u, v, math.sqrt(max(rad * rad - (u * u + v * v) * cs * cs, 0.0)))
                for u in range(-m, m + 1) for v in range(-m, m + 1) if (u * u + v * v) * cs * cs <= rad * rad]
    def roll_on(a, rad, sg):                                         # sg +1: a ball rolled over a (grow), -1: under it
        p = np.pad(a, int(rad / cs), constant_values=-sg * 1e9); m = int(rad / cs)
        st = [p[m + u:m + u + a.shape[0], m + v:m + v + a.shape[1]] + sg * hgt for u, v, hgt in ball(rad)]
        return np.maximum.reduce(st) if sg > 0 else np.minimum.reduce(st)
    floor = Z_M + D['thick']
    low = np.maximum(roll_on(T, D['gap_min'], 1), floor)             # never below this: the bear + gap_min all round
    Z = np.maximum(roll_on(T, D['gap'], 1), floor)                   # the bear + gap all round, on the mattress
    Z = roll_on(roll_on(Z, D['soft'], 1), D['soft'], -1)             # cloth: a ball of radius soft cannot get under it
    for k in range(D['puff']):                                       # padded cloth: no sharp creases between the bumps
        p = np.pad(Z, 1, mode='edge')                                # (each pass is a 3x3 binomial blur)
        Z = (4 * Z + 2 * (p[2:, 1:-1] + p[:-2, 1:-1] + p[1:-1, 2:] + p[1:-1, :-2]) +
             p[2:, 2:] + p[2:, :-2] + p[:-2, 2:] + p[:-2, :-2]) / 16.0
    Z = np.maximum(Z, low)
    rc = D['round']                                                  # round the edges it hangs from
    def roll(d): t = np.clip(rc - d, 0.0, rc); return rc - np.sqrt(rc * rc - t * t)
    Z = np.maximum(Z - roll(yw - np.abs(gy))[None, :] - roll(x1 - gx)[:, None], np.where(T > -1e8, low, Z_M))

    # the cloth as a closed solid: the height field, walls straight down to z = skirt all round, a flat bottom.
    # A height field cannot hold its steep sides evenly (they would shade in streaks), so the solid is remeshed
    # into even voxel-size faces and relaxed; the cuts below open it again.
    eps = 0.004; I = np.arange(-1, nx + 2); J = np.arange(-1, ny + 2)
    X = gx[np.clip(I, 0, nx)] + eps * ((I > nx) * 1.0 - (I < 0) * 1.0)
    Y = gy[np.clip(J, 0, ny)] + eps * ((J > ny) * 1.0 - (J < 0) * 1.0)
    ZZ = np.full((nx + 3, ny + 3), D['skirt']); ZZ[1:nx + 2, 1:ny + 2] = Z
    V = np.stack([np.repeat(X[:, None], ny + 3, 1), np.repeat(Y[None, :], nx + 3, 0), ZZ], -1).reshape(-1, 3)
    vid = np.arange((nx + 3) * (ny + 3)).reshape(nx + 3, ny + 3)
    Fq = np.stack([vid[:-1, :-1], vid[1:, :-1], vid[1:, 1:], vid[:-1, 1:]], -1).reshape(-1, 4)
    src = r['src']; me = src.data; me.clear_geometry(); me.from_pydata(V.tolist(), [], Fq.tolist())
    bm = bmesh.new(); bm.from_mesh(me)
    bmesh.ops.holes_fill(bm, edges=[e for e in bm.edges if e.is_boundary], sides=0)
    bm.to_mesh(me); bm.free(); me.update(); src.update_tag()
    bpy.context.view_layer.update()
    ev = bpy.data.meshes.new_from_object(src.evaluated_get(bpy.context.evaluated_depsgraph_get()))
    bm = bmesh.new(); bm.from_mesh(ev); bpy.data.meshes.remove(ev); me.clear_geometry()
    for k in range(D['relax']):
        bmesh.ops.smooth_vert(bm, verts=bm.verts[:], factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
    bm.normal_update()
    sig = [hashlib.md5(np.round(Z, 4).tobytes()).hexdigest()[:12], round(h, 4)] + [round(v, 5) for v in (*c, *n)]

    near = roll_on(np.where(under, 0.0, -1e9), D['lip_r'], 1) > -1e8  # the bear is under here, or close by
    def cut(lift, lo, hi, on_bear=False):                            # the cloth with lo <= s <= hi (s: past the edge plane)
        b = bm.copy()
        if on_bear:                                                  # the lip: only where the hem lies on the bear
            def off(f):
                m = f.calc_center_median()
                return not near[min(max(int(round((m.x - x0) / cs)), 0), nx), min(max(int(round((m.y + yw) / cs)), 0), ny)]
            bmesh.ops.delete(b, geom=[f for f in b.faces if off(f)], context='FACES')
        if lift:
            for v in b.verts: v.co += v.normal * lift
        for s0, sg in ((hi, 1.0), (lo, -1.0)):
            if s0 is None: continue
            bmesh.ops.bisect_plane(b, geom=b.verts[:] + b.edges[:] + b.faces[:], dist=1e-7, plane_co=c + n * s0,
                                   plane_no=n * sg, clear_outer=True)
        return b
    for ob, b in zip(obs, (cut(0.0, None, 0.0), cut(D['lift'], -D['hem'], 0.0), cut(0.0, 0.0, D['lip'], True))):
        me = ob.data; me.clear_geometry(); b.to_mesh(me); b.free(); me.update()
        for p in me.polygons: p.use_smooth = True
    bm.free()
    return sig


def _bed():
    """the set-frame matrix of the bed's own frame (bed centre, length along X, head end at -X)."""
    from mathutils import Matrix
    return Matrix.Translation(_R['bed'].location)


def _grip(n):
    """object frame -> paw frame (origin at the left paw's rest head, bear rest axes: forward -Y, left +X)."""
    from mathutils import Matrix, Euler
    E = lambda a, b, c: Euler((math.radians(a), math.radians(b), math.radians(c)), 'XYZ').to_matrix().to_4x4()
    if n == 'can': return Matrix.Translation((0.0, -0.04, -0.02))
    if n == 'broom': return Matrix.Translation((0.0, -0.03, 0.0)) @ E(-32.0, 0.0, 0.0)
    if n == 'rod': return Matrix.Translation((0.0, -0.02, 0.0)) @ E(-28.0, 0.0, 0.0)
    return Matrix.Identity(4)


def prop_state(s, st):
    """layer -> render key of this frame's prop state. In Blender (after build_props) this also PLACES the set's
    moving props for the frame (see the module header: render_set calls it right after posing the rig)."""
    if _R.get('set') == s: return _apply(st)
    return {l: f'{l}_' + hashlib.md5(json.dumps(st, sort_keys=True).encode()).hexdigest()[:10] for l in LAYERS[s]}
