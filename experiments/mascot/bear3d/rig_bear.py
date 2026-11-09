# Rig the cute bear and key the Stage A clips (idle, walk, run + start/stop). The model is not changed.
#
#   Blender -b bear.blend -P rig_bear.py -- [--out bear_rig.blend] [--clips clips.json]
#
# Armature 'Rig' (child of BearRoot). Every part is rigidly parented to one bone, so there is no skin
# weighting and the face decals can't smear:
#   root (floor)                    root motion lives in clips.json, not in the bones (clips are in place)
#   +- body  (pivot on the floor)   squash/stretch, bob, waddle roll, lean: body, pants, stitches
#   +- face (eye centre)            brows, nose, mouth, blush; rides the body (see carry() below)
#   |  +- eyes                      blink = vertical scale
#   +- hat (beanie base)            rides the body; carries the beanie face
#   |  +- clover (stem base)        wobble: stem + leaves
#   +- arm_L / arm_R (shoulder)     ride the body + swing
#   +- ear_L / ear_R (ear base)     ride the body + wobble
#   +- foot_L / foot_R              steps (a planted foot ignores the body's sway)
# Only the body mesh squashes. The attachments follow its motion rigidly (carry()), so they never deform.
# Every bone points straight up, so its local axes are a permutation of the world axes and a pose given as an
# armature-space delta D (about the bone's pivot) is keyed as matrix_basis = B^-1 @ D @ B (no shear).
# Clips: one action per clip (fake user), CONSTANT interpolation, scene at 12 fps, frames 0..n-1.
import bpy, sys, os, math, json
import numpy as np
from mathutils import Vector, Matrix, Euler

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import anim_clips

argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
def arg(n, d=None): return argv[argv.index(n) + 1] if n in argv else d
OUT = arg('--out', os.path.join(HERE, 'bear_rig.blend'))
CLIPS_JSON = arg('--clips', os.path.join(HERE, 'clips.json'))
FIT, FIT_YAW = 105.0, 112.5   # the cute 100 px renders: same pixel size

sc = bpy.context.scene; vl = bpy.context.view_layer
col = bpy.data.collections['Bear']; root = bpy.data.objects['BearRoot']
meshes = [o for o in col.all_objects if o.type == 'MESH']
O = bpy.data.objects

# ------------------------------------------------------------------ pixel scale (same maths as render_bear.py)
ROT = Euler((math.radians(60), 0, math.radians(45)), 'XYZ').to_matrix()
RIGHT, UP = ROT.col[0], ROT.col[1]
def verts_world():
    pts = []
    for o in meshes:
        co = np.empty(len(o.data.vertices) * 3, np.float32); o.data.vertices.foreach_get('co', co)
        M = o.matrix_world; pts.append(co.reshape(-1, 3) @ np.array(M.to_3x3()).T + np.array(M.translation))
    return np.concatenate(pts)
root.rotation_euler = (0, 0, math.radians(FIT_YAW)); vl.update()
v = verts_world() @ np.array(UP)
PPU = (FIT - 2) / (v.max() - v.min())
root.rotation_euler = (0, 0, 0); vl.update()
V = 2.0 / (RIGHT.x * PPU)          # 2 px along a 2:1 iso axis
PXZ = 1.0 / (UP.z * PPU)           # 1 px of screen height, in world z
print(f'PPU {PPU:.4f}  V {V:.5f}  PXZ {PXZ:.5f}')

rest = {o.name: o.matrix_world.copy() for o in meshes}

# ------------------------------------------------------------------ pivots from the model
def wverts(o):
    co = np.empty(len(o.data.vertices) * 3, np.float32); o.data.vertices.foreach_get('co', co)
    M = o.matrix_world; return co.reshape(-1, 3) @ np.array(M.to_3x3()).T + np.array(M.translation)
ev = wverts(O['eyes']); stem = wverts(O['stem'])
PIV = {
    'root': Vector((0, 0, 0)), 'body': Vector((0, 0, 0)),
    'foot_L': O['foot_L'].matrix_world.translation.copy(), 'foot_R': O['foot_R'].matrix_world.translation.copy(),
    'arm_L': O['arm_L'].matrix_world.translation.copy(), 'arm_R': O['arm_R'].matrix_world.translation.copy(),
    'ear_L': O['ear_L'].matrix_world.translation.copy(), 'ear_R': O['ear_R'].matrix_world.translation.copy(),
    'eyes': Vector(((ev[:, 0].min() + ev[:, 0].max()) / 2, ev[:, 1].mean(), (ev[:, 2].min() + ev[:, 2].max()) / 2)),
    'hat': O['hat'].matrix_world.translation.copy(),
    'face': Vector(((ev[:, 0].min() + ev[:, 0].max()) / 2, ev[:, 1].mean(), (ev[:, 2].min() + ev[:, 2].max()) / 2)),
    'clover': Vector(stem[stem[:, 2].argmin()]),
}
PARENT = {'root': None, 'body': 'root', 'foot_L': 'root', 'foot_R': 'root', 'arm_L': 'root', 'arm_R': 'root',
          'ear_L': 'root', 'ear_R': 'root', 'face': 'root', 'eyes': 'face', 'hat': 'root', 'clover': 'hat'}
BIND = {'body': 'body', 'pants': 'body', 'stitch': 'body', 'eyes': 'eyes', 'hat': 'hat',
        'brows': 'face', 'nose': 'face', 'mouth': 'face', 'blush': 'face',
        'stem': 'clover', 'clover': 'clover', 'clover_b': 'clover', 'arm_L': 'arm_L', 'arm_R': 'arm_R',
        'ear_L': 'ear_L', 'ear_R': 'ear_R', 'foot_L': 'foot_L', 'foot_R': 'foot_R'}

# ------------------------------------------------------------------ armature
arm = bpy.data.armatures.new('RigData'); rig = bpy.data.objects.new('Rig', arm); col.objects.link(rig)
rig.parent = root; rig.matrix_world = Matrix.Identity(4)
vl.objects.active = rig; rig.select_set(True)
bpy.ops.object.mode_set(mode='EDIT')
for name in PARENT:                                   # dict order: parents before children
    b = arm.edit_bones.new(name); p = PIV[name]
    b.head = p; b.tail = p + Vector((0, 0, 0.25 if name in ('root', 'body') else 0.12)); b.roll = 0
    if PARENT[name]: b.parent = arm.edit_bones[PARENT[name]]; b.use_connect = False
bpy.ops.object.mode_set(mode='OBJECT')
for name, bone in BIND.items():
    ob = O[name]; mw = ob.matrix_world.copy()
    ob.parent = rig; ob.parent_type = 'BONE'; ob.parent_bone = bone
    bn = rig.data.bones[bone]   # a bone parent is the bone's tail frame (rest pose here)
    ob.matrix_parent_inverse = (rig.matrix_world @ bn.matrix_local @ Matrix.Translation((0, bn.length, 0))).inverted()
    ob.matrix_basis = mw
vl.update()
err = max(max(abs(a - b) for ra, rb in zip(o.matrix_world, rest[o.name]) for a, b in zip(ra, rb)) for o in meshes)
print(f'rest pose drift after rigging: {err:.2e}'); assert err < 1e-5

# ------------------------------------------------------------------ pose from parameters
def about(p, M): return Matrix.Translation(p) @ M @ Matrix.Translation(-p)
def rx(d): return Matrix.Rotation(math.radians(d), 4, 'X')
def ry(d): return Matrix.Rotation(math.radians(d), 4, 'Y')
def scl(x, y, z): return Matrix.Diagonal((x, y, z, 1.0))

def deltas(q):
    sxy = 1 / math.sqrt(q['sz'])
    D = {'root': Matrix.Identity(4)}
    rigid = Matrix.Translation((0, 0, q['bob'])) @ ry(q['roll']) @ rx(q['lean'])
    D['body'] = rigid @ scl(sxy, sxy, q['sz'])
    def carry(p):
        # Attachments (face decals, beanie, ears, arms) ride on the squashing body WITHOUT being squashed
        # vertically, and their vertical shift is rounded to whole pixels. A 1-2 px breath then moves the face
        # by whole pixels instead of re-rasterising it (no shimmering eyes/nose), and the face stays readable.
        Dc = rigid @ scl(sxy, sxy, 1.0)
        dz = (D['body'] @ p).z - p.z
        return Matrix.Translation((0, 0, round(dz / PXZ) * PXZ + p.z - (Dc @ p).z)) @ Dc
    D['face'] = carry(PIV['face']); D['hat'] = carry(PIV['hat'])
    for s, k in (('L', 1), ('R', -1)):
        D['foot_' + s] = Matrix.Translation((0, -q['f' + s], q['h' + s]))
        D['arm_' + s] = carry(PIV['arm_' + s]) @ about(PIV['arm_' + s], rx(-q['arm' + s]))
        D['ear_' + s] = carry(PIV['ear_' + s]) @ about(PIV['ear_' + s], ry(k * q['ear' + s]))
    D['eyes'] = about(PIV['eyes'], scl(1, 1, q['blink']))
    D['clover'] = about(PIV['clover'], ry(q['clY']) @ rx(q['clX']))
    return D

def apply_pose(q):
    for name, Dm in deltas(q).items():
        pb = rig.pose.bones[name]; B = rig.data.bones[name].matrix_local
        pb.rotation_mode = 'QUATERNION'
        pb.matrix_basis = B.inverted() @ Dm @ B

# ------------------------------------------------------------------ actions
clips = anim_clips.build(V, PXZ)
sc.render.fps = anim_clips.FPS; sc.render.fps_base = 1
bpy.context.preferences.edit.keyframe_new_interpolation_type = 'CONSTANT'
rig.animation_data_create()
meta = dict(fps=anim_clips.FPS, ppu=PPU, V=V, PXZ=PXZ, order=anim_clips.ORDER, clips={})
for cname in anim_clips.ORDER:
    c = clips[cname]
    act = bpy.data.actions.new(cname); act.use_fake_user = True
    rig.animation_data.action = act
    for f, q in enumerate(c['frames']):
        apply_pose(q)
        for pb in rig.pose.bones:
            for path in ('location', 'rotation_quaternion', 'scale'):
                pb.keyframe_insert(path, frame=f, group=pb.name)
    meta['clips'][cname] = dict(loop=c['loop'], n=len(c['frames']), root=c['root'] + [c['next_root']],
                                poses=c['frames'])
    print('CLIP', cname, len(c['frames']), 'frames')
rig.animation_data.action = bpy.data.actions['idle']
sc.frame_start, sc.frame_end = 0, meta['clips']['idle']['n'] - 1
for pb in rig.pose.bones: pb.matrix_basis = Matrix.Identity(4)
json.dump(meta, open(CLIPS_JSON, 'w'), indent=1)
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.wm.save_as_mainfile(filepath=OUT, compress=True)
print('SAVED', OUT)
