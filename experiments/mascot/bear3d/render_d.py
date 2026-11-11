# Stage D: the desk station (low desk, chunky toy computer + keyboard, swivel chair) + the desk clips, rendered as
# per-layer data passes for the pixel step (render_b.py's method on the Stage L rig).
#
#   Blender -b <scratch>/bear_rig_l.blend -P render_d.py -- --ori A|B --mode layout|props|clip
#           [--clip NAME|all] --raw RAWDIR --px PXDIR [--k 8]
#
# Same camera, PPU (clips.json, i.e. Stage A), supersampling and passes as Stage A/B. The rig is the Stage L rig
# (rig_bear.py --stage l, built in scratch by run_d.sh): Stage C's rigid whole-pixel face bones plus the head bone,
# so the seated head turn IS the Stage L whole-head turn and the face patches match. The rig file is only read; the
# props are built in memory on every run (deterministic).
# Set origin = the desk footprint centre on the floor. ori A: desk long axis along world X, the chair SW of it; the
# bear types facing NE and talks with its body SE, head S. ori B = the mirror image (props reflected in 3D, so the
# light stays the same) and the bear in the mirrored POSE (clips_d.mirror_pose): the bear itself is never mirrored,
# so every ori B bear pixel (hand-offs, face frames) is a real render of the real bear.
# Layers: the bear and each prop (desk_table, chair, computer) are rendered ALONE and pixelized on their own; make_d.py
# composites them per pixel by depth. Layer renders are keyed by a hash of their state.
import bpy, bmesh, sys, os, math, json, hashlib
import numpy as np
from mathutils import Vector, Matrix, Euler

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import clips_d, anim_clips

argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
def arg(n, d=None): return argv[argv.index(n) + 1] if n in argv else d
ORI = arg('--ori', 'A'); MODE = arg('--mode', 'clip'); CLIP = arg('--clip', 'all')
RAW = arg('--raw'); PXD = arg('--px'); K = int(arg('--k', '8'))
SET = 'desk'; TAG = f'{SET}_{ORI}'
RD = os.path.join(RAW, TAG); PD = os.path.join(PXD, TAG)
os.makedirs(RD, exist_ok=True); os.makedirs(PD, exist_ok=True)

CJ = json.load(open(os.path.join(HERE, 'clips.json')))
PPU, PXZ = CJ['ppu'], CJ['PXZ']

sc = bpy.context.scene; vl = bpy.context.view_layer; O = bpy.data.objects
root = O['BearRoot']; rig = O['Rig']
assert 'head' in rig.data.bones, 'render_d.py needs the Stage L rig (rig_bear.py --stage l)'
BEAR = [o for o in bpy.data.collections['Bear'].all_objects if o.type == 'MESH']
BEAR_HIDDEN = {o.name for o in BEAR if o.hide_render}       # Stage C face-layer decals: never in a body render
if rig.animation_data: rig.animation_data.action = None
for pb in rig.pose.bones: pb.matrix_basis = Matrix.Identity(4)
root.rotation_euler = (0, 0, 0); vl.update()

ROT = Euler((math.radians(60), 0, math.radians(45)), 'XYZ')
Rm = ROT.to_matrix(); RIGHT, UP, BACK = Rm.col[0], Rm.col[1], Rm.col[2]
Rn, Un = np.array(RIGHT), np.array(UP)

def snap(x, y, z=0.0):
    """nearest set-frame point at height z whose screen offset from the set origin is a whole pixel."""
    dx = round(PPU * (RIGHT.x * x + RIGHT.y * y)); dy = round(-PPU * (UP.x * x + UP.y * y + UP.z * z))
    A = np.array([[RIGHT.x, RIGHT.y], [UP.x, UP.y]]); b = np.array([dx / PPU, -dy / PPU - UP.z * z])
    s = np.linalg.solve(A, b)
    return float(s[0]), float(s[1]), (int(dx), int(dy))

def px_of(x, y, z):
    return [PPU * (RIGHT.x * x + RIGHT.y * y), -PPU * (UP.x * x + UP.y * y + UP.z * z)]

# ------------------------------------------------------------------ bear pose: rig_bear.py's Stage L deltas + root
PIV = {b.name: b.head_local.copy() for b in rig.data.bones}
HEAD_Z0, HEAD_Z1 = 0.85, 1.40                                    # as rig_bear.py --stage l
def head_w(z): return anim_clips.ease(min(max((z - HEAD_Z0) / (HEAD_Z1 - HEAD_Z0), 0.0), 1.0))
ARM_W = head_w((PIV['arm_L'].z + 0.93) / 2)
def about(p, M): return Matrix.Translation(p) @ M @ Matrix.Translation(-p)
def rx(d): return Matrix.Rotation(math.radians(d), 4, 'X')
def ry(d): return Matrix.Rotation(math.radians(d), 4, 'Y')
def rz(d): return Matrix.Rotation(math.radians(d), 4, 'Z')
def scl(x, y, z): return Matrix.Diagonal((x, y, z, 1.0))

def face_px(p, D, hb=0): return int(round(((D['body'] @ p).z - p.z) / PXZ)) + int(hb)
def face_shift(p, D): return Matrix.Translation((0, 0, face_px(p, D) * PXZ))

def deltas(q):
    """identical to rig_bear.py deltas() for --stage l, plus the set placement X, Y, Z, yaw on the root bone."""
    assert q['roll'] == 0 and q['lean'] == 0 and q['sz'] == 1.0 and q['blink'] == 1.0, 'Stage C rules (face patches)'
    sxy = 1 / math.sqrt(q['sz'])
    D = {}
    rigid = Matrix.Translation((0, 0, q['bob'])) @ ry(q['roll']) @ rx(q['lean'])
    D['body'] = rigid @ scl(sxy, sxy, q['sz'])
    def carry(p):
        Dc = rigid @ scl(sxy, sxy, 1.0)
        dz = (D['body'] @ p).z - p.z
        return Matrix.Translation((0, 0, round(dz / PXZ) * PXZ + p.z - (Dc @ p).z)) @ Dc
    D['face'] = carry(PIV['face']); D['hat'] = carry(PIV['hat'])
    for s, k in (('L', 1), ('R', -1)):
        sh = PIV['arm_' + s]
        D['foot_' + s] = Matrix.Translation((0, -q['f' + s], q['h' + s]))
        D['arm_' + s] = carry(sh) @ about(sh, rx(-q['arm' + s])) @ about(sh, ry(-k * q.get('sp' + s, 0.0)))
        D['ear_' + s] = carry(PIV['ear_' + s]) @ about(PIV['ear_' + s], ry(k * q['ear' + s]))
    D['eyes'] = about(PIV['eyes'], scl(1, 1, q['blink']))
    D['clover'] = about(PIV['clover'], ry(q['clY']) @ rx(q['clX']))
    D['eyes'] = face_shift(PIV['eyes'], D) @ D['eyes']; D['mouth'] = face_shift(PIV['mouth'], D)
    Rh = Matrix.Translation((0, 0, q['hb'] * PXZ)) @ Matrix.Rotation(math.radians(q['hy']), 4, 'Z')
    D['head'] = D['body'] @ Rh
    for n in ('face', 'eyes', 'mouth', 'hat', 'ear_L', 'ear_R'): D[n] = Rh @ D[n]
    for n in ('arm_L', 'arm_R'): D[n] = Matrix.Rotation(math.radians(q['hy'] * ARM_W), 4, 'Z') @ D[n]
    D['root'] = Matrix.Translation((q['X'], q['Y'], q['Z'])) @ rz(q['yaw']) @ rz(q['ty'])
    return D

def apply_pose(q):
    for name, Dm in deltas(q).items():
        pb = rig.pose.bones[name]; B = rig.data.bones[name].matrix_local
        pb.rotation_mode = 'QUATERNION'
        pb.matrix_basis = B.inverted() @ Dm @ B
    vl.update()

def bear_sig():
    a = np.array([x for pb in rig.pose.bones for row in pb.matrix_basis for x in row])
    return hashlib.md5(np.round(a, 6).tobytes()).hexdigest()[:12]

# ------------------------------------------------------------------ geometry (ori A, set frame = desk centre)
DESK_W, DESK_D = 0.78, 0.42          # desk top half extents (x, y)
DESK_TOP = 1.30                      # desk top height: the seated paws reach the keys with no lean
SEAT_TOP = 0.37                      # chair cushion top
SEAT_Z = 0.35                        # seated root z (the round bottom sinks a little into the cushion)
BEAR_X = -0.20                       # the bear (and keyboard) sit left of centre, the monitor right of it
BELLY = 0.66                         # seated root -> desk front edge, along the typing facing (+Y)
sx, sy, seat_px = snap(BEAR_X, -DESK_D - BELLY, SEAT_Z)
hx, hy, hand_px = snap(sx + 1.20, sy)                            # hand-off spot: SE of the chair, clear of it
G = dict(PXZ=PXZ, hand=(hx, hy), seat=(sx, sy), seatZ=SEAT_Z)
CLIPS = clips_d.build(G)
HAND = clips_d.handoff(G)
KB = (sx, -DESK_D + 0.13)                                        # keyboard centre: under the paws (reach ~0.75)
MON = (0.20, 0.10)                                               # monitor centre

# ------------------------------------------------------------------ props
W0 = O.new('SetWorld', None); sc.collection.objects.link(W0)
PROPS = bpy.data.collections.new('Props'); sc.collection.children.link(PROPS)
PID = {'desk_top': 38, 'desk_legs': 39, 'chair_seat': 40, 'chair_back': 41, 'chair_stem': 42, 'chair_base': 43,
       'monitor': 44, 'screen': 45, 'keyboard': 47, 'keys': 48}
SCREEN_HI = 46                                                   # the screen's second glow tone (pass id swap)
LAYER_OBJS = {}

def empty(name, loc, parent):
    e = O.new(name, None); PROPS.objects.link(e); e.parent = parent; e.location = loc; return e

def finish(me, name, parent, layer, smooth=True):
    ob = O.new(name, me); PROPS.objects.link(ob); ob.parent = parent
    m = bpy.data.materials.new('MP_' + name); m.pass_index = PID[name]
    me.materials.append(m); ob.pass_index = PID[name]
    if smooth:
        for p in me.polygons: p.use_smooth = True
    LAYER_OBJS.setdefault(layer, []).append(ob)
    return ob

def sgp(w, e): return math.copysign(abs(w) ** e, w)
def superq_bm(bm, center, radii, e1, e2, board=False, segs=40, rings=20):
    """superellipsoid (e < 1 = boxier, soft edges). board=True: profile in x-z, thickness along y."""
    tmp = bmesh.new(); bmesh.ops.create_uvsphere(tmp, u_segments=segs, v_segments=rings, radius=1.0)
    for v in tmp.verts:
        x, y, z = v.co; phi = math.asin(max(-1.0, min(1.0, z))); th = math.atan2(y, x)
        X = sgp(math.cos(phi), e1) * sgp(math.cos(th), e2); Y = sgp(math.cos(phi), e1) * sgp(math.sin(th), e2)
        Z = sgp(math.sin(phi), e1)
        if board: X, Y, Z = X, -Z, Y
        v.co = (center[0] + X * radii[0], center[1] + Y * radii[1], center[2] + Z * radii[2])
    me = bpy.data.meshes.new('tmp'); tmp.to_mesh(me); tmp.free(); bm.from_mesh(me); bpy.data.meshes.remove(me)

def solid(name, parent, layer, parts):
    bm = bmesh.new()
    for p in parts: superq_bm(bm, *p[:4], **(p[4] if len(p) > 4 else {}))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new(name); bm.to_mesh(me); bm.free()
    return finish(me, name, parent, layer)

# ---- desk_table: a low chunky toy desk (soft slab top on four stubby round legs)
P_desk = empty('P_desk', (0, 0, 0), W0)
solid('desk_top', P_desk, 'desk_table', [((0, 0, DESK_TOP - 0.08), (DESK_W, DESK_D, 0.08), 0.30, 0.30)])
solid('desk_legs', P_desk, 'desk_table', [((lx * (DESK_W - 0.16), ly * (DESK_D - 0.13), (DESK_TOP - 0.14) / 2),
                                          (0.11, 0.11, (DESK_TOP - 0.14) / 2), 0.35, 1.0, dict(segs=20, rings=12))
                                         for lx in (-1, 1) for ly in (-1, 1)])
# ---- computer: a chubby rounded monitor on a little foot, mint screen facing the chair (-Y), keyboard with keys
P_comp = empty('P_computer', (0, 0, DESK_TOP), W0)
MZC = 0.44
solid('monitor', P_comp, 'computer', [((MON[0], MON[1], MZC), (0.40, 0.24, 0.32), 0.40, 0.40),
                                      ((MON[0], MON[1] + 0.05, 0.05), (0.20, 0.15, 0.05), 0.5, 0.6, dict(segs=24, rings=12)),
                                      ((MON[0], MON[1] + 0.05, 0.12), (0.07, 0.06, 0.08), 1.0, 1.0, dict(segs=16, rings=8))])
SCREEN = solid('screen', P_comp, 'computer', [((MON[0], MON[1] - 0.235, MZC + 0.02), (0.31, 0.02, 0.23), 0.35, 0.35)])
solid('keyboard', P_comp, 'computer', [((KB[0], KB[1], 0.03), (0.36, 0.11, 0.03), 0.30, 0.25)])
solid('keys', P_comp, 'computer', [((KB[0] + kx * 0.105, KB[1] + ky * 0.06, 0.065), (0.04, 0.022, 0.012), 0.6, 0.6,
                                    dict(segs=12, rings=6)) for kx in (-2.5, -1.5, -0.5, 0.5, 1.5, 2.5) for ky in (-0.5, 0.5)])
# ---- chair: round swivel chair; local frame faces -Y (yaw 0) like the bear; origin = the swivel axis on the floor
P_chair = empty('P_chair', (sx, sy, 0), W0)
solid('chair_base', P_chair, 'chair', [((0, 0, 0.05), (0.40, 0.40, 0.05), 0.5, 1.0, dict(segs=32, rings=12))])
solid('chair_stem', P_chair, 'chair', [((0, 0, 0.16), (0.08, 0.08, 0.10), 1.0, 1.0, dict(segs=16, rings=8))])
solid('chair_seat', P_chair, 'chair', [((0, 0.06, SEAT_TOP - 0.07), (0.50, 0.54, 0.07), 0.45, 1.0)])
solid('chair_back', P_chair, 'chair', [((0, 0.70, 0.78), (0.42, 0.08, 0.34), 0.40, 0.50, dict(board=True)),
                                       ((0, 0.64, 0.38), (0.08, 0.06, 0.07), 1.0, 1.0, dict(segs=16, rings=8))])

ALL_PROP_OBJS = [o for l in LAYER_OBJS.values() for o in l]
SET_LAYERS = ['desk_table', 'chair', 'computer']
if ORI == 'B': W0.matrix_world = Matrix(((0, -1, 0, 0), (-1, 0, 0, 0), (0, 0, 1, 0), (0, 0, 0, 1)))
vl.update()

def ori_frame(q, st):
    return clips_d.mirror_pose(q, st) if ORI == 'B' else (q, st)

def set_state(q, st):
    """q, st: the ori A frame. Returns the layer keys of this frame in this ori."""
    qo, so = ori_frame(q, st)
    apply_pose(qo)
    P_chair.rotation_euler = (0, 0, math.radians(st['chair']))       # chair under W0: the mirror applies
    SCREEN.pass_index = SCREEN_HI if st['scr'] else PID['screen']
    vl.update()
    return {'bear': 'bear_' + bear_sig(), 'desk_table': 'desk_table_0', 'chair': f"chair_{st['chair']:g}",
            'computer': f"computer_s{st['scr']}"}

def bear_px(q):
    """screen offset (px) of the bear root from the set origin in this ori, and the whole-pixel face lift."""
    qo, _ = ori_frame(q, dict(chair=0, scr=0))
    p = px_of(qo['X'], qo['Y'], qo['Z'])
    return [round(p[0], 4), round(p[1], 4)], int(round(q['bob'] / PXZ)) + int(q['hb'])

# ------------------------------------------------------------------ layout (one canvas for every clip of the set)
def proj(P): return P @ Rn, P @ Un
def eval_pts(obs):
    dg = bpy.context.evaluated_depsgraph_get(); pts = []
    for o in obs:
        if o.hide_render: continue
        oe = o.evaluated_get(dg); me = oe.to_mesh()
        co = np.empty(len(me.vertices) * 3, np.float32); me.vertices.foreach_get('co', co); oe.to_mesh_clear()
        M = o.matrix_world; pts.append(co.reshape(-1, 3) @ np.array(M.to_3x3()).T + np.array(M.translation))
    return np.concatenate(pts)

LAYOUT = os.path.join(PD, 'layout.json')
if MODE == 'layout':
    ext = [1e9, -1e9, -1e9, 1e9]          # u min, u max, v max, v min
    def grow(P):
        u, v = proj(P); ext[0] = min(ext[0], u.min()); ext[1] = max(ext[1], u.max())
        ext[2] = max(ext[2], v.max()); ext[3] = min(ext[3], v.min())
    for c in clips_d.ORDER:
        for f, (q, st) in enumerate(CLIPS[c]):
            set_state(q, st)
            grow(eval_pts(BEAR + ALL_PROP_OBJS))
    OX = int(math.ceil(-ext[0] * PPU)) + 3; W = OX + int(math.ceil(ext[1] * PPU)) + 3
    OY = int(math.ceil(ext[2] * PPU)) + 3; H = OY + int(math.ceil(-ext[3] * PPU)) + 3
    W += W % 2; H += H % 2
    sx_ = -1 if ORI == 'B' else 1
    def mirp(p): return [sx_ * p[0], p[1]]
    def mirw(p): return [-p[1], -p[0]] if ORI == 'B' else list(p)
    lay = dict(set=SET, ori=ORI, W=W, H=H, origin=[OX, OY], k=K, ppu=PPU,
               geom=dict(G, hand=list(G['hand']), seat=list(G['seat']), desk_top=DESK_TOP, seat_top=SEAT_TOP,
                         desk_half=[DESK_W, DESK_D], keyboard=list(KB), monitor=list(MON)),
               handoff={c: dict(h, px=mirp(hand_px), yaw=(90.0 - h['yaw']) % 360 if ORI == 'B' else h['yaw'],
                                pos=mirw(h['pos'])) for c, h in HAND.items()},
               seat_px=mirp(seat_px),
               prop_px=dict(desk_table=[0, 0], computer=mirp([0, round(-PPU * UP.z * DESK_TOP)]),
                            chair=mirp(snap(sx, sy)[2])),
               prop_pos=dict(desk_table=[0, 0, 0], computer=[0, 0, DESK_TOP], chair=mirw([sx, sy]) + [0]),
               pass_ids=dict(PID, screen_hi=SCREEN_HI))
    json.dump(lay, open(LAYOUT, 'w'), indent=1)
    print('LAYOUT', TAG, W, H, OX, OY, 'seat px', seat_px, 'hand px', hand_px)
    sys.exit(0)

lay = json.load(open(LAYOUT)); W, H = lay['W'], lay['H']; OX, OY = lay['origin']

# ------------------------------------------------------------------ camera + render settings (as Stage A/B)
cd = bpy.data.cameras.new('IsoCam'); cd.type = 'ORTHO'
cd.ortho_scale = max(W, H) / PPU; cd.clip_start = 0.01; cd.clip_end = 100
cam = O.new('IsoCam', cd); sc.collection.objects.link(cam)
cam.rotation_euler = ROT
cam.location = RIGHT * ((W / 2 - OX) / PPU) + UP * ((OY - H / 2) / PPU) + BACK * 20.0
sc.camera = cam
sc.render.engine = 'CYCLES'
cy = sc.cycles
cy.samples = 1; cy.use_denoising = False; cy.use_adaptive_sampling = False
cy.max_bounces = 0; cy.diffuse_bounces = 0; cy.glossy_bounces = 0; cy.transmission_bounces = 0
cy.filter_width = 0.01; cy.pixel_filter_type = 'BOX'
sc.render.film_transparent = True
sc.render.resolution_x = W * K; sc.render.resolution_y = H * K; sc.render.resolution_percentage = 100
sc.view_settings.view_transform = 'Raw'
im = sc.render.image_settings
im.file_format = 'OPEN_EXR'; im.color_mode = 'RGBA'; im.color_depth = '32'; im.exr_codec = 'ZIP'

def override(name, build):
    m = bpy.data.materials.new(name); m.use_nodes = True
    nt = m.node_tree; nt.nodes.clear()
    out = nt.nodes.new('ShaderNodeOutputMaterial'); em = nt.nodes.new('ShaderNodeEmission')
    em.inputs['Strength'].default_value = 1.0
    nt.links.new(em.outputs[0], out.inputs['Surface']); nt.links.new(build(nt), em.inputs['Color'])
    return m
def b_nrm(nt):
    g = nt.nodes.new('ShaderNodeNewGeometry'); ma = nt.nodes.new('ShaderNodeVectorMath'); ma.operation = 'MULTIPLY_ADD'
    ma.inputs[1].default_value = (0.5, 0.5, 0.5); ma.inputs[2].default_value = (0.5, 0.5, 0.5)
    nt.links.new(g.outputs['Normal'], ma.inputs[0]); return ma.outputs[0]
def b_idd(nt):
    oi = nt.nodes.new('ShaderNodeObjectInfo'); cdn = nt.nodes.new('ShaderNodeCameraData')
    ao = nt.nodes.new('ShaderNodeAmbientOcclusion'); ao.samples = 16; ao.only_local = False
    ao.inputs['Distance'].default_value = 0.35
    cb = nt.nodes.new('ShaderNodeCombineXYZ')
    nt.links.new(oi.outputs['Object Index'], cb.inputs[0]); nt.links.new(cdn.outputs['View Z Depth'], cb.inputs[1])
    nt.links.new(ao.outputs['AO'], cb.inputs[2]); return cb.outputs[0]
M_N = override('OV_normal', b_nrm); M_D = override('OV_iddepth', b_idd)
def read_exr(path):
    img = bpy.data.images.load(path); img.colorspace_settings.name = 'Non-Color'
    w, h = img.size; a = np.empty(w * h * 4, np.float32); img.pixels.foreach_get(a); bpy.data.images.remove(img)
    return a.reshape(h, w, 4)[::-1]

tmp = os.path.join(RD, '_tmp.exr')
def render_layer(layer, key):
    if os.path.exists(os.path.join(PD, key + '.png')) or os.path.exists(os.path.join(RD, key + '.npz')): return False
    show = BEAR if layer == 'bear' else LAYER_OBJS[layer]
    keep = {o.name: o.hide_render for o in BEAR + ALL_PROP_OBJS}
    for o in BEAR + ALL_PROP_OBJS: o.hide_render = True
    for o in show: o.hide_render = o.name in BEAR_HIDDEN
    res = {}
    for kk, m in (('n', M_N), ('d', M_D)):
        vl.material_override = m; sc.render.filepath = tmp
        bpy.ops.render.render(write_still=True); res[kk] = read_exr(tmp)
    np.savez_compressed(os.path.join(RD, key + '.npz'), nrm=res['n'][:, :, :3].astype(np.float16),
                        alpha=(res['d'][:, :, 3] > 0.5), pid=np.rint(res['d'][:, :, 0]).astype(np.int16),
                        depth=res['d'][:, :, 1], ao=res['d'][:, :, 2].astype(np.float16))
    for o in BEAR + ALL_PROP_OBJS: o.hide_render = keep[o.name]
    return True

json.dump(dict(k=K, W=W, H=H, ppu=PPU, foot_row=OY, right=list(RIGHT), up=list(UP), back=list(BACK)),
          open(os.path.join(RD, 'meta.json'), 'w'))
import time
t0 = time.time(); nr = 0
if MODE == 'props':
    q, st = CLIPS['desk_sit'][0]                        # home state: chair facing SE (A) / SW (B), screen tone 0
    keys = set_state(q, st)
    for l in SET_LAYERS: nr += render_layer(l, keys[l])
    json.dump({l: keys[l] for l in SET_LAYERS}, open(os.path.join(PD, 'props.json'), 'w'), indent=1)
else:
    for c in (clips_d.ORDER if CLIP == 'all' else [CLIP]):
        frames = []
        for f, (q, st) in enumerate(CLIPS[c]):
            keys = set_state(q, st)
            rp, fpx = bear_px(q)
            frames.append(dict(keys=keys, q=q, st=st, root_px=rp, face_px=fpx))
            nr += render_layer('bear', keys['bear'])
            for l in ('chair', 'computer'): nr += render_layer(l, keys[l])
        json.dump(dict(clip=c, n=len(frames), loop=c in clips_d.LOOP, frames=frames),
                  open(os.path.join(PD, f'clip_{c}.json'), 'w'), indent=1)
        print('CLIP', TAG, c, len(frames), f'{time.time() - t0:.0f}s', flush=True)
if os.path.exists(tmp): os.remove(tmp)
print(f'DONE {TAG} {MODE} {CLIP if MODE == "clip" else ""}: {nr} layer renders in {time.time() - t0:.1f}s')
