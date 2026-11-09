# Render the bear's data passes for the pixel step (Cycles, 1 sample/px, no AA filter, supersampled K x).
#
#   Blender -b bear.blend -P render_bear.py -- --raw DIR --name big --fit 105 --fit-yaw 112.5 --yaws 32 [--k 8]
#
# Camera: orthographic iso, Euler XYZ (60, 0, 45) deg = 30 deg elevation, which projects floor axes to a
# 2:1 pixel slope (checked and printed below). The camera and light are fixed; the bear root turns (yaw).
# Yaw convention (bear modelled facing -Y): 45 = S (faces the viewer), 90 = SE, 135 = E (faces screen
# right), 180 = NE, 225 = N, 270 = NW, 315 = W, 0 = SW.
# Two passes per yaw, both through view-layer material overrides (part ids come from object pass_index):
#   N pass: world normal * 0.5 + 0.5
#   D pass: (pass_index, view depth, ambient occlusion)
# Each is written as float EXR, re-read here and stored as RAW/<name>/yaw_<deg>.npz.
import bpy, sys, os, math, json
import numpy as np
from mathutils import Vector, Euler, Matrix

argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
def arg(n, d=None): return argv[argv.index(n) + 1] if n in argv else d
RAW = arg('--raw'); NAME = arg('--name', 'big'); FIT = float(arg('--fit', '100'))
FIT_YAW = float(arg('--fit-yaw', '112.5')); NY = int(arg('--yaws', '8')); K = int(arg('--k', '8'))
YAW0 = 45.0
YAWS = [(YAW0 + 360.0 * i / NY) % 360 for i in range(NY)]
OD = os.path.join(RAW, NAME); os.makedirs(OD, exist_ok=True)

sc = bpy.context.scene; vl = bpy.context.view_layer
root = bpy.data.objects['BearRoot']
meshes = [o for o in bpy.data.collections['Bear'].all_objects if o.type == 'MESH']

# ------------------------------------------------------------------ camera
ROT = Euler((math.radians(60), 0, math.radians(45)), 'XYZ')
Rm = ROT.to_matrix(); RIGHT, UP, BACK = Rm.col[0], Rm.col[1], Rm.col[2]
# 2:1 check: world +x and +y axes on screen
for ax in (Vector((1, 0, 0)), Vector((0, 1, 0))):
    sx, sy = ax.dot(RIGHT), ax.dot(UP)
    print(f'ISO axis {tuple(ax)} -> screen ({sx:.4f}, {sy:.4f}) slope {abs(sy / sx):.4f}')

PIVOT = bpy.data.objects.get('clover_pivot')
CAM_BASIS = Matrix((RIGHT, -BACK, UP)).transposed()   # pivot local x -> screen right, -y -> toward camera, z -> up
def pose(yaw):
    """turn the bear; the clover pivot faces the camera (stand-in for a 'face camera' bone)."""
    root.rotation_euler = (0, 0, math.radians(yaw)); vl.update()
    if PIVOT:
        PIVOT.matrix_world = Matrix.Translation(PIVOT.matrix_world.translation) @ CAM_BASIS.to_4x4(); vl.update()

def verts_world(yaw):
    pose(yaw)
    pts = []
    for o in meshes:
        M = o.matrix_world
        co = np.empty(len(o.data.vertices) * 3, np.float32); o.data.vertices.foreach_get('co', co)
        co = co.reshape(-1, 3)
        R = np.array(M.to_3x3()); t = np.array(M.translation)
        pts.append(co @ R.T + t)
    return np.concatenate(pts)

def proj(pts):
    return pts @ np.array(RIGHT), pts @ np.array(UP)

u, v = proj(verts_world(FIT_YAW))
PPU = (FIT - 2) / (v.max() - v.min())          # -2: the 1px outline above and below
ext = dict(l=0, r=0, t=0, b=0)
for y in [(YAW0 + 360.0 * i / 32) % 360 for i in range(32)]:
    u, v = proj(verts_world(y))
    ext['l'] = min(ext['l'], u.min()); ext['r'] = max(ext['r'], u.max())
    ext['t'] = max(ext['t'], v.max()); ext['b'] = min(ext['b'], v.min())
half = max(-ext['l'], ext['r']) * PPU
W = int(math.ceil(half)) * 2 + 6
H = int(math.ceil((ext['t'] - ext['b']) * PPU)) + 6
FOOT = H - 3 - int(math.ceil(-ext['b'] * PPU))     # pixel row of the world origin (feet contact)
print(f'SIZE {NAME}: ppu {PPU:.3f}  canvas {W}x{H}  origin row {FOOT}')

cd = bpy.data.cameras.new('IsoCam'); cd.type = 'ORTHO'
cd.ortho_scale = max(W, H) / PPU; cd.clip_start = 0.01; cd.clip_end = 100
cam = bpy.data.objects.new('IsoCam', cd); sc.collection.objects.link(cam)
cam.rotation_euler = ROT
# origin at screen x = canvas centre, y = FOOT row (screen centre is H/2 rows from the top)
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
for yaw in YAWS:
    pose(yaw)
    res = {}
    for key, m in (('n', M_N), ('d', M_D)):
        vl.material_override = m
        sc.render.filepath = tmp
        bpy.ops.render.render(write_still=True)
        res[key] = read_exr(tmp)
    np.savez_compressed(os.path.join(OD, f'yaw_{yaw:06.2f}.npz'),
                        nrm=res['n'][:, :, :3].astype(np.float16), alpha=(res['d'][:, :, 3] > 0.5),
                        pid=np.rint(res['d'][:, :, 0]).astype(np.int16), depth=res['d'][:, :, 1],
                        ao=res['d'][:, :, 2].astype(np.float16))
    print(f'YAW {yaw:.2f} done')
os.remove(tmp)
# light/camera metadata for the pixel step (light is fixed relative to the camera)
json.dump(dict(name=NAME, k=K, W=W, H=H, ppu=PPU, foot_row=FOOT, yaws=YAWS, right=list(RIGHT), up=list(UP),
               back=list(BACK)), open(os.path.join(OD, 'meta.json'), 'w'), indent=1)
print('ALL DONE')
