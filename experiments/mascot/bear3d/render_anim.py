# Render the rigged bear's clips as data passes for the pixel step (same camera, passes and settings as
# render_bear.py; only the pose changes).
#
#   Blender -b bear_rig.blend -P render_anim.py -- --raw DIR --tag big --clip walk [--fit 105] [--yaws 45,90,...] [--k 8]
#
# Pixel scale: PPU from the REST pose exactly as render_bear.py (fit px tall at yaw 112.5), so the bear is the
# same size as the cute 100 px renders. Canvas and feet row: the union over EVERY clip, frame and the 8
# directions, so all clips share one frame size and one anchor, and the rest pose renders identically everywhere.
# Frames whose pose equals an earlier frame of the same clip are not re-rendered (listed in meta 'alias').
# Writes DIR/<tag>_<clip>/f<NN>_yaw_<deg>.npz + meta.json.
import bpy, sys, os, math, json
import numpy as np
from mathutils import Vector, Euler, Matrix

argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
def arg(n, d=None): return argv[argv.index(n) + 1] if n in argv else d
RAW = arg('--raw'); TAG = arg('--tag', 'big'); CLIP = arg('--clip'); FIT = float(arg('--fit', '105'))
FIT_YAW = float(arg('--fit-yaw', '112.5')); K = int(arg('--k', '8'))
ALL_YAWS = [45.0 + 45.0 * i for i in range(8)]
ALL_YAWS = [y % 360 for y in ALL_YAWS]
YAWS = [float(y) for y in arg('--yaws').split(',')] if arg('--yaws') else ALL_YAWS
OD = os.path.join(RAW, f'{TAG}_{CLIP}'); os.makedirs(OD, exist_ok=True)

sc = bpy.context.scene; vl = bpy.context.view_layer
root = bpy.data.objects['BearRoot']; rig = bpy.data.objects['Rig']
meshes = [o for o in bpy.data.collections['Bear'].all_objects if o.type == 'MESH']
CLIPS = [a.name for a in bpy.data.actions]

ROT = Euler((math.radians(60), 0, math.radians(45)), 'XYZ')
Rm = ROT.to_matrix(); RIGHT, UP, BACK = Rm.col[0], Rm.col[1], Rm.col[2]

def use_clip(name):
    ad = rig.animation_data; ad.action = bpy.data.actions[name] if name else None
    if name and getattr(ad, 'action_slot', 1) is None and len(ad.action.slots): ad.action_slot = ad.action.slots[0]
    if not name:
        for pb in rig.pose.bones: pb.matrix_basis = Matrix.Identity(4)
    vl.update()

def at(frame, yaw):
    sc.frame_set(frame); root.rotation_euler = (0, 0, math.radians(yaw)); vl.update()

def verts_world():
    pts = []
    for o in meshes:
        co = np.empty(len(o.data.vertices) * 3, np.float32); o.data.vertices.foreach_get('co', co)
        M = o.matrix_world; pts.append(co.reshape(-1, 3) @ np.array(M.to_3x3()).T + np.array(M.translation))
    return np.concatenate(pts)
def proj(p): return p @ np.array(RIGHT), p @ np.array(UP)

use_clip(None); root.rotation_euler = (0, 0, math.radians(FIT_YAW)); vl.update()
u, v = proj(verts_world())
PPU = (FIT - 2) / (v.max() - v.min())
ext = dict(l=0, r=0, t=0, b=0)
nframes = {}
for c in CLIPS:
    use_clip(c); a = bpy.data.actions[c]; n = int(round(a.frame_range[1])) + 1; nframes[c] = n
    for f in range(n):
        for y in ALL_YAWS:
            at(f, y); u, v = proj(verts_world())
            ext['l'] = min(ext['l'], u.min()); ext['r'] = max(ext['r'], u.max())
            ext['t'] = max(ext['t'], v.max()); ext['b'] = min(ext['b'], v.min())
half = max(-ext['l'], ext['r']) * PPU
W = int(math.ceil(half)) * 2 + 6
H = int(math.ceil((ext['t'] - ext['b']) * PPU)) + 6
FOOT = H - 3 - int(math.ceil(-ext['b'] * PPU))
print(f'SIZE {TAG}: ppu {PPU:.4f}  canvas {W}x{H}  origin row {FOOT}  clips {nframes}')

cd = bpy.data.cameras.new('IsoCam'); cd.type = 'ORTHO'
cd.ortho_scale = max(W, H) / PPU; cd.clip_start = 0.01; cd.clip_end = 100
cam = bpy.data.objects.new('IsoCam', cd); sc.collection.objects.link(cam)
cam.rotation_euler = ROT
cam.location = UP * ((FOOT - H / 2) / PPU) + BACK * 20.0
sc.camera = cam

# ------------------------------------------------------------------ render settings
sc.render.engine = 'CYCLES'
cy = sc.cycles
cy.samples = 1; cy.use_denoising = False; cy.use_adaptive_sampling = False
cy.max_bounces = 0; cy.diffuse_bounces = 0; cy.glossy_bounces = 0; cy.transmission_bounces = 0
cy.filter_width = 0.01
cy.pixel_filter_type = 'BOX'
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
    nt.links.new(em.outputs[0], out.inputs['Surface'])
    nt.links.new(build(nt), em.inputs['Color'])
    return m

def b_nrm(nt):
    g = nt.nodes.new('ShaderNodeNewGeometry')
    ma = nt.nodes.new('ShaderNodeVectorMath'); ma.operation = 'MULTIPLY_ADD'
    ma.inputs[1].default_value = (0.5, 0.5, 0.5); ma.inputs[2].default_value = (0.5, 0.5, 0.5)
    nt.links.new(g.outputs['Normal'], ma.inputs[0])
    return ma.outputs[0]

def b_idd(nt):
    oi = nt.nodes.new('ShaderNodeObjectInfo'); cdn = nt.nodes.new('ShaderNodeCameraData')
    ao = nt.nodes.new('ShaderNodeAmbientOcclusion'); ao.samples = 16; ao.only_local = False
    ao.inputs['Distance'].default_value = 0.35
    cb = nt.nodes.new('ShaderNodeCombineXYZ')
    nt.links.new(oi.outputs['Object Index'], cb.inputs[0])
    nt.links.new(cdn.outputs['View Z Depth'], cb.inputs[1])
    nt.links.new(ao.outputs['AO'], cb.inputs[2])
    return cb.outputs[0]

M_N = override('OV_normal', b_nrm); M_D = override('OV_iddepth', b_idd)

def read_exr(path):
    img = bpy.data.images.load(path); img.colorspace_settings.name = 'Non-Color'
    w, h = img.size
    a = np.empty(w * h * 4, np.float32); img.pixels.foreach_get(a)
    bpy.data.images.remove(img)
    return a.reshape(h, w, 4)[::-1]

tmp = os.path.join(OD, '_tmp.exr')
use_clip(CLIP)
sig_seen, alias = {}, {}
for f in range(nframes[CLIP]):
    sc.frame_set(f); vl.update()
    sig = tuple(round(x, 6) for pb in rig.pose.bones for row in pb.matrix_basis for x in row)
    if sig in sig_seen: alias[f] = sig_seen[sig]; continue
    sig_seen[sig] = f
    for yaw in YAWS:
        at(f, yaw)
        res = {}
        for key, m in (('n', M_N), ('d', M_D)):
            vl.material_override = m
            sc.render.filepath = tmp
            bpy.ops.render.render(write_still=True)
            res[key] = read_exr(tmp)
        np.savez_compressed(os.path.join(OD, f'f{f:02d}_yaw_{yaw:06.2f}.npz'),
                            nrm=res['n'][:, :, :3].astype(np.float16), alpha=(res['d'][:, :, 3] > 0.5),
                            pid=np.rint(res['d'][:, :, 0]).astype(np.int16), depth=res['d'][:, :, 1],
                            ao=res['d'][:, :, 2].astype(np.float16))
    print(f'FRAME {CLIP} {f} done')
os.remove(tmp)
json.dump(dict(name=TAG, clip=CLIP, n=nframes[CLIP], alias=alias, k=K, W=W, H=H, ppu=PPU, foot_row=FOOT,
               yaws=YAWS, right=list(RIGHT), up=list(UP), back=list(BACK)),
          open(os.path.join(OD, 'meta.json'), 'w'), indent=1)
print('ALL DONE')
