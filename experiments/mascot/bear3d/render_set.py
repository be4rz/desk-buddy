# Stage kit: render a set stage's layers (render_d.py's method, generic): the bear and each prop layer rendered ALONE
# as data passes, keyed by a hash of their state, for pixelize_set.py and make_set.py.
#
#   Blender -b <scratch>/rig_<id>.blend -P render_set.py -- --module set_<id> --clips <scratch>/clips_<id>.json
#           --set NAME --ori A|B --mode layout|props|clip --raw RAWDIR --px PXDIR [--k 8]
#
# The rig is rig_bear.py --stage k with the set module: every clip is already keyed per ori as action '<clip>@<ori>'
# (ori B = the mirrored pose, anim_clips.mirror_k; the placement X Y Z yaw on the root bone), so this script only picks
# action + frame. It builds the module's props (build_props(set, kit, geom)) under a set root that ori B mirrors in 3D
# ((x, y) -> (-y, -x): light unchanged, the bear itself never mirrored). Same camera, PPU (Stage A's, from the clips
# json), supersampling and passes as Stage A/B/D.
import bpy, bmesh, sys, os, math, json, hashlib, importlib, time
import numpy as np
from mathutils import Vector, Matrix, Euler

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
if os.environ.get('BEAR3D_STAGES'): sys.path.insert(0, os.environ['BEAR3D_STAGES'])
import stagekit

argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
def arg(n, d=None): return argv[argv.index(n) + 1] if n in argv else d
MOD = importlib.import_module(arg('--module')); CJ = json.load(open(arg('--clips')))
SET = arg('--set'); ORI = arg('--ori', 'A'); MODE = arg('--mode', 'clip')
RAW = arg('--raw'); PXD = arg('--px'); K = int(arg('--k', '8'))
TAG = f'{SET}_{ORI}'; RD = os.path.join(RAW, TAG); PD = os.path.join(PXD, TAG)
os.makedirs(RD, exist_ok=True); os.makedirs(PD, exist_ok=True)
SD = MOD.SETS[SET]; KS = CJ['sets'][SET]; G = KS['geom']
PPU, PXZ = CJ['ppu'], CJ['PXZ']
assert abs(PPU - json.load(open(os.path.join(HERE, 'clips.json')))['ppu']) < 1e-9, 'rig PPU != Stage A PPU'

sc = bpy.context.scene; vl = bpy.context.view_layer; O = bpy.data.objects
root = O['BearRoot']; rig = O['Rig']
BEAR = [o for o in bpy.data.collections['Bear'].all_objects if o.type == 'MESH']
root.rotation_euler = (0, 0, 0)
ROT = Euler((math.radians(60), 0, math.radians(45)), 'XYZ')
Rm = ROT.to_matrix(); RIGHT, UP, BACK = Rm.col[0], Rm.col[1], Rm.col[2]
Rn, Un = np.array(RIGHT), np.array(UP)


def use(clip, f):
    ad = rig.animation_data; ad.action = bpy.data.actions[f'{clip}@{ORI}']
    if getattr(ad, 'action_slot', 1) is None and len(ad.action.slots): ad.action_slot = ad.action.slots[0]
    sc.frame_set(f); vl.update()


def bear_sig():
    a = np.array([x for pb in rig.pose.bones for row in pb.matrix_basis for x in row])
    return hashlib.md5(np.round(a, 6).tobytes()).hexdigest()[:12]


# ------------------------------------------------------------------ props (the module's toy shapes)
W0 = O.new('SetWorld', None); sc.collection.objects.link(W0)
PROPS = bpy.data.collections.new('Props'); sc.collection.children.link(PROPS)
LAYER_OBJS = {}


def sgp(w, e): return math.copysign(abs(w) ** e, w)
def superq_bm(bm, center, radii, e1, e2, board=False, segs=40, rings=20):     # as render_d.py
    tmp = bmesh.new(); bmesh.ops.create_uvsphere(tmp, u_segments=segs, v_segments=rings, radius=1.0)
    for v in tmp.verts:
        x, y, z = v.co; phi = math.asin(max(-1.0, min(1.0, z))); th = math.atan2(y, x)
        X = sgp(math.cos(phi), e1) * sgp(math.cos(th), e2); Y = sgp(math.cos(phi), e1) * sgp(math.sin(th), e2)
        Z = sgp(math.sin(phi), e1)
        if board: X, Y, Z = X, -Z, Y
        v.co = (center[0] + X * radii[0], center[1] + Y * radii[1], center[2] + Z * radii[2])
    me = bpy.data.meshes.new('tmp'); tmp.to_mesh(me); tmp.free(); bm.from_mesh(me); bpy.data.meshes.remove(me)


class Kit:
    """what build_props(set, kit, geom) gets."""
    W0 = W0
    def empty(self, name, loc, parent):
        e = O.new(name, None); PROPS.objects.link(e); e.parent = parent; e.location = loc; return e
    def solid(self, name, parent, layer, pid, shapes):
        assert pid >= 50, f'{name}: kit pass ids start at 50'
        bm = bmesh.new()
        for p in shapes: superq_bm(bm, *p[:4], **(p[4] if len(p) > 4 else {}))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        me = bpy.data.meshes.new(name); bm.to_mesh(me); bm.free()
        ob = O.new(name, me); PROPS.objects.link(ob); ob.parent = parent
        m = bpy.data.materials.new('MP_' + name); m.pass_index = pid; me.materials.append(m); ob.pass_index = pid
        for p in me.polygons: p.use_smooth = True
        LAYER_OBJS.setdefault(layer, []).append(ob)
        return ob


PROP_INFO = MOD.build_props(SET, Kit(), G)
ALL_PROP_OBJS = [o for l in LAYER_OBJS.values() for o in l]
SET_LAYERS = sorted(LAYER_OBJS)
if ORI == 'B': W0.matrix_world = Matrix(((0, -1, 0, 0), (-1, 0, 0, 0), (0, 0, 1, 0), (0, 0, 0, 1)))
vl.update()


def mirw(p): return [-p[1], -p[0]] + list(p[2:]) if ORI == 'B' else list(p)
def mirp(p): return [-p[0], p[1]] if ORI == 'B' else list(p)


def set_state(clip, f):
    use(clip, f)
    st = CJ['clips'][f'{clip}@{ORI}']['st'][f]
    keys = {'bear': 'bear_' + bear_sig()}; keys.update(MOD.prop_state(SET, st))
    return keys


def bear_px(clip, f):
    q = CJ['clips'][f'{clip}@{ORI}']['poses'][f]
    return [round(v, 4) for v in stagekit.px_of(PPU, q['X'], q['Y'], q['Z'])], CJ['clips'][f'{clip}@{ORI}']['face_px']['eyes'][f]


def proj(P): return P @ Rn, P @ Un
def eval_pts(obs):
    dg = bpy.context.evaluated_depsgraph_get(); pts = []
    for o in obs:
        oe = o.evaluated_get(dg); me = oe.to_mesh()
        co = np.empty(len(me.vertices) * 3, np.float32); me.vertices.foreach_get('co', co); oe.to_mesh_clear()
        M = o.matrix_world; pts.append(co.reshape(-1, 3) @ np.array(M.to_3x3()).T + np.array(M.translation))
    return np.concatenate(pts)


LAYOUT = os.path.join(PD, 'layout.json')
if MODE == 'layout':
    ext = [1e9, -1e9, -1e9, 1e9]
    def grow(P):
        u, v = proj(P); ext[0] = min(ext[0], u.min()); ext[1] = max(ext[1], u.max())
        ext[2] = max(ext[2], v.max()); ext[3] = min(ext[3], v.min())
    for c in SD['clips']:
        for f in range(SD['counts'][c]):
            set_state(c, f); grow(eval_pts(BEAR + ALL_PROP_OBJS))
    OX = int(math.ceil(-ext[0] * PPU)) + 3; W = OX + int(math.ceil(ext[1] * PPU)) + 3
    OY = int(math.ceil(ext[2] * PPU)) + 3; H = OY + int(math.ceil(-ext[3] * PPU)) + 3
    W += W % 2; H += H % 2
    hand_px = G['hand_px']
    HY = SD['hand_yaw'] if ORI == 'A' else (90.0 - SD['hand_yaw']) % 360
    lay = dict(set=SET, ori=ORI, W=W, H=H, origin=[OX, OY], k=K, ppu=PPU, geom=G,
               handoff={c: dict(h, px=mirp(hand_px), yaw=HY, pos=mirw(h['pos'])) for c, h in KS['handoff'].items()},
               prop_px={n: mirp(v['px']) for n, v in PROP_INFO.items()},
               prop_pos={n: mirw(v['pos']) for n, v in PROP_INFO.items()},
               pass_ids={n: p['pid'] for n, p in MOD.PARTS.items()})
    json.dump(lay, open(LAYOUT, 'w'), indent=1)
    print('LAYOUT', TAG, W, H, OX, OY, 'hand px', hand_px)
    sys.exit(0)

lay = json.load(open(LAYOUT)); W, H = lay['W'], lay['H']; OX, OY = lay['origin']

# ------------------------------------------------------------------ camera + render settings (as Stage A/B/D)
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
    for o in BEAR + ALL_PROP_OBJS: o.hide_render = o not in show
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
t0 = time.time(); nr = 0
if MODE == 'props':
    keys = set_state(SD['clips'][0], 0)                 # the home prop state
    for l in SET_LAYERS: nr += render_layer(l, keys[l])
    json.dump({l: keys[l] for l in SET_LAYERS}, open(os.path.join(PD, 'props.json'), 'w'), indent=1)
else:
    for c in SD['clips']:
        frames = []
        for f in range(SD['counts'][c]):
            keys = set_state(c, f)
            rp, fpx = bear_px(c, f)
            frames.append(dict(keys=keys, root_px=rp, face_px=fpx))
            nr += render_layer('bear', keys['bear'])
            for l in SET_LAYERS: nr += render_layer(l, keys[l])
        json.dump(dict(clip=c, n=len(frames), loop=c in SD['loop'], frames=frames),
                  open(os.path.join(PD, f'clip_{c}.json'), 'w'), indent=1)
        print('CLIP', TAG, c, len(frames), f'{time.time() - t0:.0f}s', flush=True)
if os.path.exists(tmp): os.remove(tmp)
print(f'DONE {TAG} {MODE}: {nr} layer renders in {time.time() - t0:.1f}s')
