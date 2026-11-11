# Rig the cute bear and key the Stage A clips (idle, walk, run + start/stop). The model is not changed.
#
#   Blender -b bear.blend -P rig_bear.py -- [--out bear_rig.blend] [--clips clips.json]
#   Blender -b bear.blend -P rig_bear.py -- --stage c [--out bear_rig_c.blend] [--clips clips_c.json]
#   Blender -b bear.blend -P rig_bear.py -- --stage l --out <scratch>/bear_rig_l.blend [--clips clips_l.json]
#
# --stage c (Stage C, run_c.sh) keys anim_clips.ORDER_C plus the face-layer helpers instead of the Stage A clips,
# and changes only what the D16 face layers need (without --stage c the script behaves exactly as before):
#   - eyes and mouth get their own bones under root that ride the body by a WHOLE-PIXEL VERTICAL shift only
#     (no xy squash), so the eyes/mouth pixels never re-rasterise and a constant patch per direction matches;
#   - arms also take spL/spR (sideways raise, as Stage B's spread);
#   - extra actions: idle_ref (Stage A idle poses, eyes open, rigid eyes/mouth = verify_face.py's reference for
#     the shipped idle), face_rest (rest pose) and face_eyes_closed (rest, eyes shut);
#   - extra hidden meshes mouth_open / mouth_wide / tongue (face-layer states, shown only by render_anim.py --show);
#   - clips_c.json gains per clip face_px = {eyes, mouth}: whole-pixel upward shift per frame.
#
# --stage l (Stage L, run_l.sh) = everything --stage c does, keying anim_clips.ORDER_L instead, plus a head turn:
#   - a 'head' bone under root, pivot on the body's vertical axis (the axis the render yaw turns about);
#   - the body mesh alone is skinned to it (Armature modifier, dual quaternion so the twist keeps its volume):
#     weight smoothstep(HEAD_Z0 -> HEAD_Z1) of the rest height, 'body' the rest. Above HEAD_Z1 (just under the mouth)
#     the whole round head mass turns rigidly; the band down to the waist twists smoothly (no neck, no seam step);
#     the pants, stitches and feet stay;
#   - the face decals, eyes, mouth, beanie (with clover) and ears turn rigidly with the head (pose hy) and ride its
#     whole-pixel bob (hb); the arms follow the twist by ARM_W of it so they stay on the belly, under the face;
#   - the root bone takes the turn yaw ty (turn_ccw / turn_cw);
#   - clips_l.json face_px includes the head bob.

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
STAGE = arg('--stage', 'a')
if STAGE not in ('a', 'c', 'l'): raise SystemExit(f'rig_bear.py: --stage must be a, c or l, not {STAGE!r}')
STAGE_C = STAGE in ('c', 'l')   # Stage L builds on the Stage C rig
STAGE_L = STAGE == 'l'
if STAGE_L and not arg('--out'): raise SystemExit('rig_bear.py: --stage l needs --out (the Stage L rig is scratch)')
OUT = arg('--out', os.path.join(HERE, 'bear_rig_c.blend' if STAGE_C else 'bear_rig.blend'))
CLIPS_JSON = arg('--clips', os.path.join(HERE, {'a': 'clips.json', 'c': 'clips_c.json', 'l': 'clips_l.json'}[STAGE]))
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

if STAGE_C:
    # ---------------------------------------------------------------- Stage C: face-layer mouths + rigid face bones
    def cast(ob, origin, direction):   # as build_bear.py
        mi = ob.matrix_world.inverted()
        o = mi @ Vector(origin); d = (mi.to_3x3() @ Vector(direction)).normalized()
        ok, loc, nrm, _ = ob.ray_cast(o, d)
        if not ok: raise RuntimeError(f'ray miss {origin} {direction}')
        return ob.matrix_world @ loc, (ob.matrix_world.to_3x3() @ nrm).normalized()
    def face_point(x, z): return cast(O['body'], (x, -3, z), (0, 1, 0))
    def basis_from_normal(n, up=Vector((0, 0, 1))):
        n = Vector(n).normalized(); ty = (up - n * up.dot(n)).normalized(); tx = ty.cross(n).normalized()
        return Matrix((tx, ty, n)).transposed()
    import bmesh
    def decal(name, pid, x, z, radii, lift=0.0):
        # a flat oval lying on the head surface (like build_bear.py's face decals), hidden from renders
        p, n = face_point(x, z)
        me = bpy.data.meshes.new(name); bm = bmesh.new()
        bmesh.ops.create_uvsphere(bm, u_segments=16, v_segments=10, radius=1.0)
        bmesh.ops.scale(bm, vec=Vector(radii), verts=bm.verts); bm.to_mesh(me); bm.free()
        for f in me.polygons: f.use_smooth = True
        ob = bpy.data.objects.new(name, me); col.objects.link(ob)
        ob.matrix_world = Matrix.Translation(p + n * lift) @ basis_from_normal(n).to_4x4()
        src = O['blush' if pid == 17 else 'mouth']
        me.materials.append(src.data.materials[0]); ob.pass_index = pid
        ob.hide_render = True
        return ob
    mz = wverts(O['mouth'])[:, 2]
    MZ = (mz.min() + mz.max()) / 2                                    # centre height of the little "^" mouth
    decal('mouth_open', 18, 0.0, MZ - 0.012, (0.042, 0.040, 0.012))  # small round "o"
    decal('mouth_wide', 18, 0.0, MZ - 0.022, (0.075, 0.058, 0.012))  # big happy open mouth
    decal('tongue', 17, 0.0, MZ - 0.048, (0.040, 0.022, 0.012), lift=0.008)   # pink tongue inside the wide mouth
    vl.update()
    meshes = [o for o in col.all_objects if o.type == 'MESH']
    rest = {o.name: o.matrix_world.copy() for o in meshes}
    mv = np.concatenate([wverts(O[n]) for n in ('mouth', 'mouth_open', 'mouth_wide', 'tongue')])
    PIV['mouth'] = Vector(((mv[:, 0].min() + mv[:, 0].max()) / 2, mv[:, 1].mean(), (mv[:, 2].min() + mv[:, 2].max()) / 2))
    PARENT['eyes'] = 'root'; PARENT['mouth'] = 'root'
    for n in ('mouth', 'mouth_open', 'mouth_wide', 'tongue'): BIND[n] = 'mouth'

if STAGE_L:
    # ---------------------------------------------------------------- Stage L: the head bone and the twist band
    HEAD_Z0, HEAD_Z1 = 0.85, 1.40     # waist band -> just under the mouth (the mouth's lowest point is ~1.44)
    def head_w(z): return anim_clips.ease(min(max((z - HEAD_Z0) / (HEAD_Z1 - HEAD_Z0), 0.0), 1.0))
    PIV['head'] = Vector((0, 0, HEAD_Z1)); PARENT['head'] = 'root'
    ARM_W = head_w((PIV['arm_L'].z + 0.93) / 2)   # the twist at mid-arm (shoulder -> paw on the belly)
    del BIND['body']                  # skinned below, not bone-parented
    print(f'HEAD twist z {HEAD_Z0}..{HEAD_Z1}, arms follow {ARM_W:.2f}')

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
if STAGE_L:
    bo = O['body']; mw = bo.matrix_world.copy()
    bo.parent = rig; bo.parent_type = 'OBJECT'; bo.matrix_parent_inverse = rig.matrix_world.inverted(); bo.matrix_world = mw
    vg_b, vg_h = bo.vertex_groups.new(name='body'), bo.vertex_groups.new(name='head')
    for vx in bo.data.vertices:
        w = head_w((mw @ vx.co).z)
        if w < 1: vg_b.add([vx.index], 1.0 - w, 'REPLACE')
        if w > 0: vg_h.add([vx.index], w, 'REPLACE')
    md = bo.modifiers.new('Rig', 'ARMATURE'); md.object = rig
    md.use_vertex_groups = True; md.use_bone_envelopes = False; md.use_deform_preserve_volume = True
    vl.update()
    # the skinned body at rest must be the unrigged mesh (matrix_world alone can't see the modifier)
    ev = bo.evaluated_get(bpy.context.evaluated_depsgraph_get()); me = ev.to_mesh()
    assert len(me.vertices) == len(bo.data.vertices)
    err = max(err, max((ev.matrix_world @ a.co - mw @ b.co).length for a, b in zip(me.vertices, bo.data.vertices)))
    ev.to_mesh_clear()
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
    if STAGE_C:
        assert q['roll'] == 0 and q['lean'] == 0, 'Stage C poses keep the head upright (face patches)'
        for s, k in (('L', 1), ('R', -1)):
            sh = PIV['arm_' + s]
            D['arm_' + s] = carry(sh) @ about(sh, rx(-q['arm' + s])) @ about(sh, ry(-k * q.get('sp' + s, 0.0)))
        D['eyes'] = face_shift(PIV['eyes'], D) @ D['eyes']; D['mouth'] = face_shift(PIV['mouth'], D)
    if STAGE_L:
        Rh = Matrix.Translation((0, 0, q['hb'] * PXZ)) @ Matrix.Rotation(math.radians(q['hy']), 4, 'Z')
        D['head'] = D['body'] @ Rh
        for n in ('face', 'eyes', 'mouth', 'hat', 'ear_L', 'ear_R'): D[n] = Rh @ D[n]
        for n in ('arm_L', 'arm_R'): D[n] = Matrix.Rotation(math.radians(q['hy'] * ARM_W), 4, 'Z') @ D[n]
        D['root'] = Matrix.Rotation(math.radians(q['ty']), 4, 'Z')
    return D

def face_px(p, D, hb=0):
    """Stage C: whole screen pixels the body's motion lifts point p (the eyes / mouth shift up by this);
    Stage L adds the head's whole-pixel bob hb (pose key 'hb', applied by Rh in deltas)."""
    return int(round(((D['body'] @ p).z - p.z) / PXZ)) + int(hb)

def face_shift(p, D):
    return Matrix.Translation((0, 0, face_px(p, D) * PXZ))

def apply_pose(q):
    for name, Dm in deltas(q).items():
        pb = rig.pose.bones[name]; B = rig.data.bones[name].matrix_local
        pb.rotation_mode = 'QUATERNION'
        pb.matrix_basis = B.inverted() @ Dm @ B

# ------------------------------------------------------------------ actions
clips = anim_clips.build(V, PXZ)
ORDER = anim_clips.ORDER
if STAGE_C:
    idle = clips['idle']
    clips = anim_clips.build_c(V, PXZ)
    clips['idle_ref'] = dict(loop=True, root=idle['root'], next_root=idle['next_root'],
                             frames=[dict(anim_clips.REST_C, **dict(q, blink=1.0)) for q in idle['frames']])
    clips['face_rest'] = dict(loop=False, root=[0.0], next_root=0.0, frames=[anim_clips.PC()])
    clips['face_eyes_closed'] = dict(loop=False, root=[0.0], next_root=0.0, frames=[anim_clips.PC(blink=0.12)])
    ORDER = anim_clips.ORDER_C + ['idle_ref', 'face_rest', 'face_eyes_closed']
if STAGE_L:
    clips = anim_clips.build_l(V, PXZ); ORDER = anim_clips.ORDER_L
sc.render.fps = anim_clips.FPS; sc.render.fps_base = 1
bpy.context.preferences.edit.keyframe_new_interpolation_type = 'CONSTANT'
rig.animation_data_create()
meta = dict(fps=anim_clips.FPS, ppu=PPU, V=V, PXZ=PXZ, order=ORDER, clips={})
for cname in ORDER:
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
    if STAGE_C:
        meta['clips'][cname]['face_px'] = {s: [face_px(PIV[s], deltas(q), q.get('hb', 0)) for q in c['frames']] for s in ('eyes', 'mouth')}
    print('CLIP', cname, len(c['frames']), 'frames')
rig.animation_data.action = bpy.data.actions[ORDER[0]]
sc.frame_start, sc.frame_end = 0, meta['clips'][ORDER[0]]['n'] - 1
for pb in rig.pose.bones: pb.matrix_basis = Matrix.Identity(4)
json.dump(meta, open(CLIPS_JSON, 'w'), indent=1)
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.wm.save_as_mainfile(filepath=OUT, compress=True)
print('SAVED', OUT)
