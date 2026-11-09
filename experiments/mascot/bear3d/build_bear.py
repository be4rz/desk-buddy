# Build the 3D polar-bear mascot (model test, no rig) and save bear.blend.
#
#   Blender -b -P build_bear.py -- [--out bear.blend]
#
# Units: feet on z=0, bear faces -Y, ~2.15 units to the head top (hat/clover extra).
# Every part is its own object with its own material and a unique pass_index (the render pipeline
# maps pass_index -> palette ramp, see PARTS below and parts.json). Face decals (eyes, brows, nose,
# blush, mouth) are geometry placed on the head surface by ray casting, so they turn with the head.
# Pivots are set where a rig would bend: shoulders, ankles, ear bases, hat base.
import bpy, bmesh, sys, os, json, math
from mathutils import Vector, Matrix

HERE = os.path.dirname(os.path.abspath(__file__))
argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
OUT = argv[argv.index('--out') + 1] if '--out' in argv else os.path.join(HERE, 'bear.blend')

# id, object name, material colour (viewport only; pixel colours come from palette ramps)
PARTS = {
    'body':      (1,  (0.97, 0.97, 0.96)),
    'arm_L':     (2,  (0.97, 0.97, 0.96)),
    'arm_R':     (3,  (0.97, 0.97, 0.96)),
    'ear_L':     (4,  (0.97, 0.97, 0.96)),
    'ear_R':     (5,  (0.97, 0.97, 0.96)),
    'ear_in_L':  (6,  (0.86, 0.85, 0.90)),
    'ear_in_R':  (7,  (0.86, 0.85, 0.90)),
    'foot_L':    (8,  (0.97, 0.97, 0.96)),
    'foot_R':    (9,  (0.97, 0.97, 0.96)),
    'pants':     (10, (0.80, 0.93, 0.90)),
    'hat':       (11, (0.83, 0.78, 0.92)),
    'stem':      (12, (0.10, 0.09, 0.12)),
    'clover':    (13, (0.50, 0.57, 0.44)),
    'eyes':      (14, (0.10, 0.09, 0.12)),
    'brows':     (15, (0.10, 0.09, 0.12)),
    'nose':      (16, (0.10, 0.09, 0.12)),
    'blush':     (17, (0.97, 0.83, 0.86)),
    'mouth':     (18, (0.10, 0.09, 0.12)),
    'hat_face':  (19, (0.10, 0.09, 0.12)),
    'stitch':    (20, (0.10, 0.09, 0.12)),
    'clover_b':  (21, (0.50, 0.57, 0.44)),
}

sc = bpy.context.scene
for ob in list(bpy.data.objects): bpy.data.objects.remove(ob, do_unlink=True)
for m in list(bpy.data.meshes): bpy.data.meshes.remove(m)
for m in list(bpy.data.metaballs): bpy.data.metaballs.remove(m)
for c in list(bpy.data.curves): bpy.data.curves.remove(c)
for m in list(bpy.data.materials): bpy.data.materials.remove(m)

col = bpy.data.collections.new('Bear'); sc.collection.children.link(col)
root = bpy.data.objects.new('BearRoot', None); col.objects.link(root)

MATS = {}
def mat(name):
    if name in MATS: return MATS[name]
    m = bpy.data.materials.new('M_' + name)
    m.diffuse_color = (*PARTS[name][1], 1)
    m.use_nodes = True
    bsdf = m.node_tree.nodes.get('Principled BSDF')
    if bsdf: bsdf.inputs['Base Color'].default_value = (*PARTS[name][1], 1); bsdf.inputs['Roughness'].default_value = 0.8
    m.pass_index = PARTS[name][0]
    MATS[name] = m
    return m

def finish(ob, name, parent=root, smooth=True):
    ob.name = name
    ob.pass_index = PARTS[name.split('.')[0]][0] if name in PARTS else ob.pass_index
    me = ob.data
    me.materials.clear(); me.materials.append(mat(name))
    if smooth:
        for p in me.polygons: p.use_smooth = True
    if ob.name not in col.objects: col.objects.link(ob)
    mw = ob.matrix_world.copy()
    ob.parent = parent
    ob.matrix_world = mw
    return ob

# ------------------------------------------------------------------ metaball helper
SURF = 0.575  # an isolated BALL of radius 1 (threshold 0.6, stiffness 2) has a 0.575 surface radius
def meta_mesh(name, elems, res=0.022):
    mb = bpy.data.metaballs.new('MB_' + name); mb.resolution = res; mb.render_resolution = res
    tmp = bpy.data.objects.new('Mb' + name.replace('_', ''), mb); sc.collection.objects.link(tmp)
    for e in elems:
        el = mb.elements.new(type='ELLIPSOID' if e.get('t', 'e') == 'e' else 'CAPSULE')
        el.co = e['c']
        el.stiffness = e.get('s', 2.0)
        if 'q' in e: el.rotation = e['q']
        a, b, c = e['r']
        R = 1.0
        el.radius = R
        if el.type == 'CAPSULE':
            el.size_x = e['len'] / 2  # half length of the capsule segment
            el.radius = a / SURF
        else:
            el.size_x, el.size_y, el.size_z = a / SURF, b / SURF, c / SURF
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(tmp.evaluated_get(dg))
    me.name = name
    bpy.data.objects.remove(tmp, do_unlink=True)
    ob = bpy.data.objects.new(name, me)
    return ob

def set_origin(ob, p):
    """move the object origin (pivot) to world point p without moving the geometry"""
    bpy.context.view_layer.update()
    M = ob.matrix_world.copy()
    pl = M.inverted() @ Vector(p)
    ob.data.transform(Matrix.Translation(-pl))
    ob.matrix_world = M @ Matrix.Translation(pl)

def ellipsoid(name, center, radii, basis=None, segs=24, rings=14):
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=segs, v_segments=rings, radius=1.0)
    bmesh.ops.scale(bm, vec=Vector(radii), verts=bm.verts)
    bm.to_mesh(me); bm.free()
    ob = bpy.data.objects.new(name, me)
    M = Matrix.Identity(3) if basis is None else basis
    ob.matrix_world = Matrix.Translation(Vector(center)) @ M.to_4x4()
    return ob

def tube(name, pts, radius, res=6):
    cu = bpy.data.curves.new(name, 'CURVE'); cu.dimensions = '3D'
    cu.bevel_depth = radius; cu.bevel_resolution = 2; cu.use_fill_caps = True
    sp = cu.splines.new('POLY'); sp.points.add(len(pts) - 1)
    for i, p in enumerate(pts): sp.points[i].co = (*p, 1)
    tmp = bpy.data.objects.new(name + '_c', cu); sc.collection.objects.link(tmp)
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(tmp.evaluated_get(dg)); me.name = name
    bpy.data.objects.remove(tmp, do_unlink=True)
    return bpy.data.objects.new(name, me)

def join(name, obs):
    bm = bmesh.new()
    for o in obs:
        me = o.data.copy(); me.transform(o.matrix_world); bm.from_mesh(me); bpy.data.meshes.remove(me)
        bpy.data.objects.remove(o, do_unlink=True)
    me = bpy.data.meshes.new(name); bm.to_mesh(me); bm.free()
    return bpy.data.objects.new(name, me)

def basis_from_normal(n, up=Vector((0, 0, 1)), roll=0.0):
    """columns: tangent-x, tangent-y (towards up), normal. roll rotates around the normal (radians)."""
    n = Vector(n).normalized()
    ty = (up - n * up.dot(n)).normalized()
    tx = ty.cross(n).normalized()
    if roll:
        R = Matrix.Rotation(roll, 3, n)
        tx, ty = R @ tx, R @ ty
    return Matrix((tx, ty, n)).transposed()

# ------------------------------------------------------------------ 1. body (head + torso + legs, one mesh)
# CUTE variant of round 1: one soft mochi/bean mass (head + torso, no neck, no snout), about 9% shorter
# than round 1 (SQ) and a touch wider, with rounder top corners. The face is flat like a plush:
# the nose is a small button on the face instead of a muzzle. Legs are only a notch inside the pants.
SQ = 0.91
HEAD_C = Vector((0, -0.02, 1.80 * SQ))
body = meta_mesh('body', [
    dict(c=(0, 0.03, 0.66 * SQ), r=(0.60, 0.53, 0.42 * SQ)),    # hips
    dict(c=(0, -0.01, 1.00 * SQ), r=(0.63, 0.55, 0.46 * SQ)),   # belly (widest)
    dict(c=(0, 0.03, 1.38 * SQ), r=(0.56, 0.50, 0.42 * SQ)),    # chest
    dict(c=tuple(HEAD_C), r=(0.56, 0.48, 0.40 * SQ)),           # head
    dict(c=(0.21, 0.08, 1.92 * SQ), r=(0.28, 0.29, 0.25)),      # crown corners, pulled in for rounder corners
    dict(c=(-0.21, 0.08, 1.92 * SQ), r=(0.28, 0.29, 0.25)),
    dict(c=(0, -0.14, 1.62 * SQ), r=(0.50, 0.34, 0.30)),        # flat, broad face front (plush), no muzzle
    dict(c=(0.34, 0.0, 0.30), r=(0.26, 0.29, 0.30), s=2.4),     # legs: a notch between them at the cuffs
    dict(c=(-0.34, 0.0, 0.30), r=(0.26, 0.29, 0.30), s=2.4),
])
col.objects.link(body)
finish(body, 'body')
bpy.context.view_layer.update()

def cast(ob, origin, direction):
    """world-space ray cast on a mesh object -> (hit, normal) in world space."""
    bpy.context.view_layer.update()
    mi = ob.matrix_world.inverted()
    o = mi @ Vector(origin); d = (mi.to_3x3() @ Vector(direction)).normalized()
    ok, loc, nrm, _ = ob.ray_cast(o, d)
    if not ok: raise RuntimeError(f'ray miss {origin} {direction}')
    return ob.matrix_world @ loc, (ob.matrix_world.to_3x3() @ nrm).normalized()

def face_point(x, z, ob=None):
    """hit on the front (-Y) of the body at screen-front x, z."""
    return cast(ob or body, (x, -3, z), (0, 1, 0))

# ------------------------------------------------------------------ 2. pants: body shell below the waist
WAIST_Z, WAIST_TILT, CUFF_Z, OFFS = 0.86 * SQ, 0.06, 0.23, 0.03
pm = body.data.copy(); pm.name = 'pants'
bm = bmesh.new(); bm.from_mesh(pm)
bm.normal_update()
for v in bm.verts: v.co += v.normal * OFFS
# waist plane: lower at the front (-y) by WAIST_TILT
n_w = Vector((0, -WAIST_TILT, 1.0)).normalized()
geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
bmesh.ops.bisect_plane(bm, geom=geom, plane_co=(0, 0, WAIST_Z), plane_no=n_w, clear_outer=True)
geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
bmesh.ops.bisect_plane(bm, geom=geom, plane_co=(0, 0, CUFF_Z), plane_no=(0, 0, 1), clear_inner=True)
bm.to_mesh(pm); bm.free()
pants = bpy.data.objects.new('pants', pm); finish(pants, 'pants')
set_origin(pants, (0, 0, 0.5))

# waistband stitches: short ink dashes on the pants just below the waist
dashes = []
N_DASH = 22
for i in range(N_DASH):
    a0 = 2 * math.pi * (i + 0.18) / N_DASH; a1 = 2 * math.pi * (i + 0.62) / N_DASH
    pts = []
    for a in (a0, (a0 + a1) / 2, a1):
        d = Vector((math.sin(a), -math.cos(a), 0))
        z = WAIST_Z - 0.09 + WAIST_TILT * d.y * 0.6   # follow the tilted waist plane
        hit, nrm = cast(pants, d * 3 + Vector((0, 0, z)), -d)
        pts.append(tuple(hit + nrm * 0.004))
    dashes.append(tube('dash%d' % i, pts, 0.012))
stitch = join('stitch', dashes); finish(stitch, 'stitch')

# ------------------------------------------------------------------ 3. feet (white pads poking out under the pants)
for side, sx in (('L', 1), ('R', -1)):
    f = ellipsoid('foot_' + side, (0.31 * sx, -0.15, 0.13), (0.22, 0.29, 0.14))   # further out front: peeks more
    finish(f, 'foot_' + side)
    set_origin(f, (0.32 * sx, 0.02, 0.2))   # ankle pivot

# ------------------------------------------------------------------ 4. arms: short bean arms resting on the belly
for side, sx in (('L', 1), ('R', -1)):
    sh = Vector((0.54 * sx, 0.0, 1.38 * SQ)); paw = Vector((0.34 * sx, -0.53, 1.02 * SQ))
    mid = sh.lerp(paw, 0.5) + Vector((0.07 * sx, 0, -0.04))
    # a smooth sausage: many small balls along a gentle curve (no bubbly segments)
    elems = []
    for i in range(9):
        t = i / 8
        c = sh.lerp(paw, t) + Vector((0.08 * sx, 0, -0.03)) * (4 * t * (1 - t))
        rr = 0.165 - 0.01 * t
        elems.append(dict(c=tuple(c), r=(rr, rr, rr), s=1.2))
    arm = meta_mesh('arm_' + side, elems, res=0.018)
    finish(arm, 'arm_' + side)
    set_origin(arm, sh)

# ------------------------------------------------------------------ 5. ears
for side, sx in (('L', 1), ('R', -1)):
    base, nrm = cast(body, (0.35 * sx, 0.13, 3.0), (0, 0, -1))
    c = base + Vector((0.03 * sx, 0, 0.06))
    B = Matrix.Rotation(math.radians(-22 * sx), 3, 'Y')
    e = ellipsoid('ear_' + side, c, (0.17, 0.11, 0.16), B)
    finish(e, 'ear_' + side)
    set_origin(e, base)
    ei = ellipsoid('ear_in_' + side, c + B @ Vector((0, -0.095, 0.0)), (0.10, 0.03, 0.095), B)
    finish(ei, 'ear_in_' + side, parent=e)

# ------------------------------------------------------------------ 6. face decals (geometry on the head surface)
def decal(name, x, z, radii, roll=0.0, lift=0.0, ob=None):
    p, n = face_point(x, z, ob)
    B = basis_from_normal(n, roll=roll)
    return ellipsoid(name, p + n * lift, radii, B, segs=16, rings=10)

EYE_X, EYE_Z = 0.20, 1.67   # a bit lower and wider apart than round 1 (0.18, 1.87 before the squash)
eyes = join('eyes', [decal('eL', EYE_X, EYE_Z, (0.060, 0.066, 0.022)),
                     decal('eR', -EYE_X, EYE_Z, (0.060, 0.066, 0.022))])
finish(eyes, 'eyes', parent=body)
brows = join('brows', [decal('bL', EYE_X + 0.01, EYE_Z + 0.11, (0.07, 0.016, 0.016), roll=math.radians(-22)),
                       decal('bR', -EYE_X - 0.01, EYE_Z + 0.11, (0.07, 0.016, 0.016), roll=math.radians(22))])
finish(brows, 'brows', parent=body)
blush = join('blush', [decal('blL', 0.36, EYE_Z - 0.09, (0.13, 0.09, 0.012)),
                       decal('blR', -0.36, EYE_Z - 0.09, (0.13, 0.09, 0.012))])
finish(blush, 'blush', parent=body)
# button nose: a small flat oval lying on the face (depth 0.03), not a snout
NOSE_Z = EYE_Z - 0.09
tip, tn = face_point(0, NOSE_Z)
nose = ellipsoid('nose', tip, (0.075, 0.052, 0.03), basis_from_normal(tn), segs=20, rings=12)
finish(nose, 'nose', parent=body)
# little "^" mouth under the nose
mpts = []
for x, z in ((-0.05, NOSE_Z - 0.125), (0.0, NOSE_Z - 0.09), (0.05, NOSE_Z - 0.125)):
    p, n = face_point(x, z); mpts.append(tuple(p + n * 0.004))
mouth = tube('mouth', mpts, 0.016); finish(mouth, 'mouth', parent=body)

# ------------------------------------------------------------------ 7. beanie with its own sleepy face
top, _ = cast(body, (0, 0.06, 3.0), (0, 0, -1))
HAT_C = Vector((0, 0.12, top.z - 0.05))
hm = bpy.data.meshes.new('hat')
bm = bmesh.new()
bmesh.ops.create_uvsphere(bm, u_segments=32, v_segments=18, radius=1.0)
bmesh.ops.scale(bm, vec=(0.30, 0.29, 0.22), verts=bm.verts)
geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
bmesh.ops.bisect_plane(bm, geom=geom, plane_co=(0, 0, -0.02), plane_no=(0, 0, 1), clear_inner=True)
bm.to_mesh(hm); bm.free()
hat = bpy.data.objects.new('hat', hm); hat.matrix_world = Matrix.Translation(HAT_C)
finish(hat, 'hat', parent=body)
bpy.context.view_layer.update()

def hat_pt(ax, az):
    """point on the hat surface at azimuth ax (deg, 0 = front) and elevation az (deg)."""
    d = Vector((math.sin(math.radians(ax)) * math.cos(math.radians(az)),
                -math.cos(math.radians(ax)) * math.cos(math.radians(az)), math.sin(math.radians(az))))
    p, n = cast(hat, HAT_C + d * 2, -d)
    return tuple(p + n * 0.004)
hf = []
for sx in (1, -1):  # closed eyes: little downward arcs
    hf.append(tube('hfe', [hat_pt(sx * a, e) for a, e in ((14, 40), (20, 36), (26, 40))], 0.011))
hf.append(tube('hfm', [hat_pt(a, e) for a, e in ((-5, 27), (0, 24), (5, 27))], 0.011))
hat_face = join('hat_face', hf); finish(hat_face, 'hat_face', parent=hat)

# ------------------------------------------------------------------ 8. clover sprout
htop = HAT_C.z + 0.22
stem_pts = [(0, 0.07, htop - 0.02), (0, 0.05, htop + 0.10), (0, 0.0, htop + 0.20), (0, -0.07, htop + 0.27)]
stem = tube('stem', stem_pts, 0.013); finish(stem, 'stem', parent=hat)
cc = Vector((0, -0.08, htop + 0.29))
leaves = []
# a cupped rosette: four leaves around a near-horizontal disc, each leaf tipped up 35 deg along its own
# radius, so the 4-leaf shape reads from every iso angle (a flat card would vanish edge-on).
Bc = Matrix.Rotation(math.radians(25), 3, 'X')   # tipped toward the front so all 4 leaves read in S/SE
for k in range(4):
    a = math.radians(45 + 90 * k)
    Ba = Bc @ Matrix.Rotation(a, 3, 'Z') @ Matrix.Rotation(math.radians(-35), 3, 'Y')
    off = Bc @ Matrix.Rotation(a, 3, 'Z') @ Vector((0.10, 0, 0.03))
    leaves.append(ellipsoid('lf', cc + off, (0.105, 0.095, 0.03), Ba, segs=16, rings=8))   # bigger, rounder
# two objects (alternate leaves) so the pixel step can draw the cross between the leaves
clover = join('clover', leaves[0::2]); finish(clover, 'clover', parent=hat)
clover_b = join('clover_b', leaves[1::2]); finish(clover_b, 'clover_b', parent=hat)

# ------------------------------------------------------------------ save
info = {n: dict(id=v[0]) for n, v in PARTS.items()}
json.dump(info, open(os.path.join(HERE, 'parts.json'), 'w'), indent=1)
for ob in col.objects:
    if ob.type == 'MESH':
        print('PART', ob.name, ob.pass_index, len(ob.data.vertices), [round(x, 3) for x in ob.dimensions])
bpy.context.preferences.filepaths.save_version = 0   # no bear.blend1 backups
bpy.ops.wm.save_as_mainfile(filepath=OUT, compress=True)
print('SAVED', OUT)
