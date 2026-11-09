# Stage B: furniture props + bear interaction clips, rendered as per-layer data passes for the pixel step.
#
#   Blender -b bear_rig.blend -P render_b.py -- --set bed|read --ori A|B --mode layout|props|clip
#           [--clip NAME] --raw RAWDIR --px PXDIR [--k 8] [--save bear_b.blend]
#
# Same camera, PPU (clips.json, i.e. Stage A), supersampling and passes as Stage A. bear_rig.blend is only
# read: the props are built in memory on every run (deterministic), so the Stage A rig, clips and canvas
# are untouched.
#
# Sets (one fixed orientation each, plus its mirror):
#   bed    the bed (frame, mattress, pillow, foldable quilt); origin = bed footprint centre
#   read   table + pouf + book; origin = table footprint centre
#   ori A  default orientation; ori B = the mirror image (whole set reflected across the screen vertical,
#          rendered for real in 3D so the light stays the same; NOT a sprite flip)
# Layers: the bear and every prop are rendered ALONE (others hidden, no cross AO) and pixelized on their own,
# then make_b.py composites them per pixel by depth. So each prop sprite is exactly what the room shows, and
# the props-only frame + the Stage A rest sprite reproduce the hand-off frames exactly.
# Each layer render is keyed by a hash of its state (bear pose, quilt mesh, book angles), so identical layers
# are rendered once per set and orientation, across all clips.
import bpy, bmesh, sys, os, math, json, hashlib
import numpy as np
from mathutils import Vector, Matrix, Euler

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import clips_b

argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
def arg(n, d=None): return argv[argv.index(n) + 1] if n in argv else d
SET = arg('--set', 'bed'); ORI = arg('--ori', 'A'); MODE = arg('--mode', 'clip'); CLIP = arg('--clip')
FRAMES = [int(x) for x in arg('--frames').split(',')] if arg('--frames') else None
RAW = arg('--raw'); PXD = arg('--px'); K = int(arg('--k', '8')); SAVE = arg('--save')
TAG = f'{SET}_{ORI}'
RD = os.path.join(RAW, TAG); PD = os.path.join(PXD, TAG)
os.makedirs(RD, exist_ok=True); os.makedirs(PD, exist_ok=True)

CJ = json.load(open(os.path.join(HERE, 'clips.json')))
PPU, PXZ = CJ['ppu'], CJ['PXZ']

sc = bpy.context.scene; vl = bpy.context.view_layer; O = bpy.data.objects
root = O['BearRoot']; rig = O['Rig']
BEAR = [o for o in bpy.data.collections['Bear'].all_objects if o.type == 'MESH']
if rig.animation_data: rig.animation_data.action = None
for pb in rig.pose.bones: pb.matrix_basis = Matrix.Identity(4)
root.rotation_euler = (0, 0, 0); vl.update()

ROT = Euler((math.radians(60), 0, math.radians(45)), 'XYZ')
Rm = ROT.to_matrix(); RIGHT, UP, BACK = Rm.col[0], Rm.col[1], Rm.col[2]
Rn, Un = np.array(RIGHT), np.array(UP)

def wv(o, M=None):
    co = np.empty(len(o.data.vertices) * 3, np.float32); o.data.vertices.foreach_get('co', co)
    M = o.matrix_world if M is None else M
    return co.reshape(-1, 3) @ np.array(M.to_3x3()).T + np.array(M.translation)
REST_LOCAL = {o.name: wv(o) for o in BEAR}            # bear-local rest coordinates (root at identity)

def snap(x, y, z=0.0):
    """nearest set-frame floor point whose screen offset from the set origin is a whole pixel."""
    dx = round(PPU * (RIGHT.x * x + RIGHT.y * y)); dy = round(-PPU * (UP.x * x + UP.y * y + UP.z * z))
    A = np.array([[RIGHT.x, RIGHT.y], [UP.x, UP.y]]); b = np.array([dx / PPU, -dy / PPU - UP.z * z])
    s = np.linalg.solve(A, b)
    return float(s[0]), float(s[1]), (int(dx), int(dy))

# ------------------------------------------------------------------ bear pose (Stage A rig + Stage B extras)
PIV = {b.name: b.head_local.copy() for b in rig.data.bones}
PIV_LIE = Vector((0.0, 0.55, 0.05))
def T(x, y, z): return Matrix.Translation((x, y, z))
def about(p, M): return Matrix.Translation(p) @ M @ Matrix.Translation(-p)
def rx(d): return Matrix.Rotation(math.radians(d), 4, 'X')
def ry(d): return Matrix.Rotation(math.radians(d), 4, 'Y')
def rz(d): return Matrix.Rotation(math.radians(d), 4, 'Z')
def scl(x, y, z): return Matrix.Diagonal((x, y, z, 1.0))

def deltas(q):
    sxy = 1 / math.sqrt(q['sz'])
    D = {}
    rigid = T(0, 0, q['bob']) @ ry(q['roll']) @ rx(q['lean'])
    D['body'] = rigid @ scl(sxy, sxy, q['sz'])
    def carry(p):                       # identical to rig_bear.py (no vertical squash, whole-pixel shift)
        Dc = rigid @ scl(sxy, sxy, 1.0)
        dz = (D['body'] @ p).z - p.z
        return Matrix.Translation((0, 0, round(dz / PXZ) * PXZ + p.z - (Dc @ p).z)) @ Dc
    D['face'] = T(0, 0, -q['look'] * PXZ) @ carry(PIV['face']); D['hat'] = carry(PIV['hat'])
    for s, k in (('L', 1), ('R', -1)):
        sh = PIV['arm_' + s]
        D['foot_' + s] = T(0, -q['f' + s], q['h' + s])
        D['arm_' + s] = carry(sh) @ about(sh, rx(-q['arm' + s])) @ about(sh, ry(-k * q['sp' + s]))
        D['ear_' + s] = carry(PIV['ear_' + s]) @ about(PIV['ear_' + s], ry(k * q['ear' + s]))
    D['eyes'] = about(PIV['eyes'], scl(1, 1, q['blink']))
    D['clover'] = about(PIV['clover'], ry(q['clY']) @ rx(q['clX']))
    D['root'] = T(q['X'], q['Y'], q['Z']) @ rz(q['yaw']) @ about(PIV_LIE, rx(-q['lie']))
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

# ------------------------------------------------------------------ geometry: lying placement in bed
Z_M = 0.52                     # mattress top
LIE = 64.0                     # lying angle (a fat pillow props the head up, so the face shows over the quilt)
def lie_pts(names, lie, pz):
    M = np.array(about(Vector((0, PIV_LIE.y, pz)), rx(-lie)))
    P = np.concatenate([REST_LOCAL[n] for n in names])
    return P @ M[:3, :3].T + M[:3, 3]
TORSO = ['body', 'pants']; TOP = ['hat', 'clover', 'clover_b', 'stem', 'ear_L', 'ear_R']
bottom0 = np.concatenate([REST_LOCAL[n] for n in TORSO])[:, 2].min()
lo, hi = -1.0, 1.5                                             # pivot height so sitting and lying rest at one z
for _ in range(50):
    m = (lo + hi) / 2
    if lie_pts(TORSO, LIE, m)[:, 2].min() > bottom0: hi = m
    else: lo = m
PIV_LIE = Vector((0.0, 0.55, (lo + hi) / 2))
SIT_Z = Z_M - 0.05 - bottom0                                   # root z, sitting or lying on the mattress
BED_L = 1.81                                                   # mattress half length (footprint y +-2.0)
HEAD_LIM = BED_L - 0.05                                        # clover tip stops short of the headboard
LIE_Y = HEAD_LIM - lie_pts(TOP, LIE, PIV_LIE.z)[:, 1].max()    # root y lying down
SIT_Y = -1.0                                                   # root y sitting up (feet short of the footboard)
def lie_point(p):
    M = np.array(T(0, LIE_Y, SIT_Z) @ about(PIV_LIE, rx(-LIE))); return M[:3, :3] @ np.array(p) + M[:3, 3]
Y0 = -BED_L + 0.05                                             # quilt foot end (tucked at the footboard)
CHIN = lie_point((0, -0.58, 1.12)); LQ = float(CHIN[1] - Y0)     # quilt pulled up to just under the mouth
HEADB = lie_point((0, 0.45, 1.66))                             # back of the head -> pillow
EDGE_X = 0.62
bx, by, bpx = snap(1.75, -0.45)                                # hand-off spot = edge-sit y (clear of the quilt pile)
QT = 0.08; QRB = 0.75 * QT                                     # quilt thickness, fold bend radius
PA = (LQ - 2 * math.pi * QRB + 0.12) / 3                        # open quilt: 3-layer pile at the foot
PILE = [PA, PA + math.pi * QRB + PA - 0.06]
# reading set
pfx, pfy, ppx = snap(-1.32, 0.0)
rhx, rhy, rpx = snap(pfx, pfy - 1.15)
G = dict(PXZ=PXZ, bed_hand=(bx, by), bed_sitY=SIT_Y, bed_lieY=LIE_Y, bed_sitZ=SIT_Z, bed_edgeX=EDGE_X, lie=LIE, Lq=LQ, pile=PILE,
         pouf=(pfx, pfy), seatZ=0.40, read_hand=(rhx, rhy))
CLIPS = clips_b.build(G)
HAND = clips_b.handoff(G)

# ------------------------------------------------------------------ props
W0 = O.new('SetWorld', None); sc.collection.objects.link(W0)
root.parent = W0
PROPS = bpy.data.collections.new('Props'); sc.collection.children.link(PROPS)
PID = {'bed_base': 22, 'bed_head': 23, 'bed_foot': 24, 'mattress': 25, 'pillow': 26, 'quilt': 27, 'quilt_lining': 28,
       'table_top': 29, 'table_legs': 30, 'pouf': 31, 'pouf_btn': 32, 'book_cover_B': 33, 'book_cover_F': 34,
       'book_pages_B': 35, 'book_pages_F': 36, 'book_leaf': 37}
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
        if board: X, Y, Z = X, -Z, Y                       # rotation about X: profile plane -> x-z
        v.co = (center[0] + X * radii[0], center[1] + Y * radii[1], center[2] + Z * radii[2])
    me = bpy.data.meshes.new('tmp'); tmp.to_mesh(me); tmp.free(); bm.from_mesh(me); bpy.data.meshes.remove(me)

def solid(name, parent, layer, parts):
    bm = bmesh.new()
    for p in parts: superq_bm(bm, *p[:4], **(p[4] if len(p) > 4 else {}))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new(name); bm.to_mesh(me); bm.free()
    return finish(me, name, parent, layer)

# ---- bed (origin = footprint centre; footprint x +-1.0, y +-2.0)
P_bed = empty('P_bed', (0, 0, 0), W0)
solid('bed_base', P_bed, 'bed', [((0, 0, 0.19), (0.86, BED_L, 0.12), 0.30, 0.20)] +
      [((sx * 0.70, sy * (BED_L - 0.18), 0.05), (0.10, 0.10, 0.05), 0.5, 1.0, dict(segs=16, rings=8)) for sx in (-1, 1) for sy in (-1, 1)])
solid('bed_head', P_bed, 'bed', [((0, BED_L + 0.105, 0.69), (1.0, 0.085, 0.69), 0.30, 0.50, dict(board=True))])
solid('bed_foot', P_bed, 'bed', [((0, -BED_L - 0.105, 0.43), (1.0, 0.085, 0.43), 0.30, 0.50, dict(board=True))])
solid('mattress', P_bed, 'bed', [((0, 0, Z_M - 0.12), (0.90, BED_L, 0.12), 0.40, 0.25)])
pz_ = max(Z_M + 0.09, float(HEADB[2]) - 0.08)
solid('pillow', P_bed, 'bed', [((0, float(HEADB[1]) + 0.02, pz_), (0.62, 0.30, pz_ - Z_M + 0.02), 0.55, 0.40)])
QC, QR, QHANG = 0.84, 0.08, 0.24
QWS = QC + math.pi * QR / 2 + QHANG
NU, NS = 51, 96
def slab_mesh(name):
    """closed slab over a NU x NS grid: upper sheet (verts 0..N-1), lower sheet (N..2N-1), rim."""
    me = bpy.data.meshes.new(name); N = NU * NS
    def ix(i, j, lo=0): return j * NU + i + lo * N
    fs = []
    for j in range(NS - 1):
        for i in range(NU - 1):
            fs.append((ix(i, j), ix(i + 1, j), ix(i + 1, j + 1), ix(i, j + 1)))
            fs.append((ix(i, j, 1), ix(i, j + 1, 1), ix(i + 1, j + 1, 1), ix(i + 1, j, 1)))
    for i in range(NU - 1):
        fs.append((ix(i + 1, 0), ix(i, 0), ix(i, 0, 1), ix(i + 1, 0, 1)))
        fs.append((ix(i, NS - 1), ix(i + 1, NS - 1), ix(i + 1, NS - 1, 1), ix(i, NS - 1, 1)))
    for j in range(NS - 1):
        fs.append((ix(0, j), ix(0, j + 1), ix(0, j + 1, 1), ix(0, j, 1)))
        fs.append((ix(NU - 1, j + 1), ix(NU - 1, j), ix(NU - 1, j, 1), ix(NU - 1, j + 1, 1)))
    me.from_pydata([(0, 0, 0)] * (2 * N), [], fs); return me
QUILT = finish(slab_mesh('quilt'), 'quilt', P_bed, 'bed')
LINING = finish(slab_mesh('quilt_lining'), 'quilt_lining', P_bed, 'bed')

# heightfield grid over the bed for the quilt drape
HX = np.arange(-1.0, 1.0001, 0.04); HY = np.arange(-1.88, 1.88001, 0.04)
def blur1(a, s, ax):
    r = int(3 * s); k = np.exp(-0.5 * (np.arange(-r, r + 1) / s) ** 2); k /= k.sum()
    pad = [(0, 0)] * a.ndim; pad[ax] = (r, r); ap = np.pad(a, pad, mode='edge')
    return np.apply_along_axis(lambda v: np.convolve(v, k, mode='valid'), ax, ap)
def maxf(a, r):
    out = a.copy()
    for dy in range(-r, r + 1):
        for dx in range(-r, r + 1):
            if dx * dx + dy * dy > r * r: continue
            out = np.maximum(out, np.roll(np.roll(a, dy, 0), dx, 1))
    return out
def bilin(Hg, x, y):
    fx = np.clip((x - HX[0]) / 0.04, 0, len(HX) - 1.001); fy = np.clip((y - HY[0]) / 0.04, 0, len(HY) - 1.001)
    ix, iy = fx.astype(int), fy.astype(int); tx, ty = fx - ix, fy - iy
    return (Hg[iy, ix] * (1 - tx) * (1 - ty) + Hg[iy, ix + 1] * tx * (1 - ty) + Hg[iy + 1, ix] * (1 - tx) * ty
            + Hg[iy + 1, ix + 1] * tx * ty)

def heightfield(st):
    names = ['body', 'pants', 'foot_L', 'foot_R'] + (['arm_L', 'arm_R'] if st['arms'] else [])
    Mi = np.array(W0.matrix_world.inverted())
    pts = []
    for n in names:
        p = wv(O[n]); p = p @ Mi[:3, :3].T + Mi[:3, 3]
        pts.append(p[REST_LOCAL[n][:, 2] < st['cut']])
    p = np.concatenate(pts)
    hb = np.full((len(HY), len(HX)), -9.0)
    ix = np.round((p[:, 0] - HX[0]) / 0.04).astype(int); iy = np.round((p[:, 1] - HY[0]) / 0.04).astype(int)
    ok = (ix >= 0) & (ix < len(HX)) & (iy >= 0) & (iy < len(HY)) & (p[:, 2] > Z_M - 0.2)
    np.maximum.at(hb, (iy[ok], ix[ok]), p[ok, 2])
    h1 = np.maximum(Z_M, maxf(hb, 1)) + 0.02                     # never below the bear (no poke-through)
    Hg = np.maximum(blur1(blur1(np.maximum(Z_M, maxf(hb, 2)) + 0.02, 1.5, 0), 1.5, 1), h1)
    Hg = blur1(blur1(Hg, 0.8, 0), 0.8, 1)
    if st['br'] and Hg.max() > Z_M + 0.1:
        lump = Hg - (Z_M + 0.02); Hg = Z_M + 0.02 + lump * (1 + st['br'] * PXZ / lump.max())
    return Hg

def quilt_path(s, folds):
    """y and stack offset along the quilt: straight runs from the foot, each fold a half-turn bend (radius QRB)
    that reverses direction and stacks one layer higher. folds = arc positions of the bends (>= LQ: none)."""
    y = np.zeros_like(s); off = np.full_like(s, QT / 2)
    s0, y0, o, d = 0.0, Y0, QT / 2, 1.0; prev = -1e9
    for f in list(folds) + [1e9]:
        f = max(f, prev + math.pi * QRB, s0); prev = f
        m = s >= s0; y[m] = y0 + d * (s[m] - s0); off[m] = o
        if f >= LQ: break
        yf = y0 + d * (f - s0); a = (s - f) / QRB; m = (s > f) & (s <= f + math.pi * QRB)
        y[m] = yf + d * QRB * np.sin(a[m]); off[m] = o + QRB * (1 - np.cos(a[m]))
        s0, y0, o, d = f + math.pi * QRB, yf, o + 2 * QRB, -d
    return y, off

def quilt_coords(st):
    Hg = heightfield(st)
    gx = np.gradient(Hg, 0.04, axis=1); gy = np.gradient(Hg, 0.04, axis=0)
    u = np.linspace(-QWS, QWS, NU); s = np.linspace(0, LQ, NS)
    y, off = quilt_path(s, st['folds'])
    U, Yg = np.meshgrid(u, y); OFF = np.meshgrid(u, off)[1]
    sg = np.sign(U); au = np.abs(U)
    xc = np.where(au <= QC, U, sg * QC)
    Ht = bilin(Hg, xc, Yg); nx = -bilin(gx, xc, Yg); ny = -bilin(gy, xc, Yg)
    nt = np.stack([nx, ny, np.ones_like(nx)], -1); nt /= np.linalg.norm(nt, axis=-1, keepdims=True)
    phi = np.clip((au - QC) / QR, 0, math.pi / 2)
    X = np.where(au <= QC, U, sg * (QC + QR * np.sin(phi)))
    Z = Ht - QR * (1 - np.cos(phi)) - np.clip(au - QC - math.pi * QR / 2, 0, None)
    X = np.where(au > QC + math.pi * QR / 2, sg * (QC + QR), X)
    n = nt * np.cos(phi)[..., None] + np.stack([sg, 0 * sg, 0 * sg], -1) * np.sin(phi)[..., None]
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    P = np.stack([X, Yg, Z], -1) + OFF[..., None] * n          # sheet midline (NS, NU, 3)
    sn = np.cross(np.gradient(P, axis=1), np.gradient(P, axis=0))   # sheet normal (quilt side), follows the fold
    sn /= np.maximum(np.linalg.norm(sn, axis=-1, keepdims=True), 1e-9)
    return P, sn

def set_quilt(st):
    P, sn = quilt_coords(st); h = QT / 2
    for ob, up, lo in ((QUILT, P + sn * h, P), (LINING, P, P - sn * h)):
        V = np.concatenate([up.reshape(-1, 3), lo.reshape(-1, 3)]).astype(np.float32)
        ob.data.vertices.foreach_set('co', V.ravel()); ob.data.update()
    return hashlib.md5(np.round(P, 4).tobytes()).hexdigest()[:12]

# ---- reading set (origin = table centre; footprint +-0.65)
TOPZ = 1.0
P_table = empty('P_table', (0, 0, 0), W0)
solid('table_top', P_table, 'table', [((0, 0, TOPZ - 0.075), (0.62, 0.62, 0.075), 0.30, 0.30)])
solid('table_legs', P_table, 'table', [((sx * 0.42, sy * 0.42, 0.465), (0.105, 0.105, 0.465), 0.35, 1.0, dict(segs=20, rings=12))
                                       for sx in (-1, 1) for sy in (-1, 1)])
P_pouf = empty('P_pouf', (pfx, pfy, 0), W0)
solid('pouf', P_pouf, 'pouf', [((0, 0, 0.225), (0.46, 0.46, 0.225), 0.45, 1.0)])
solid('pouf_btn', P_pouf, 'pouf', [((0, 0, 0.452), (0.065, 0.065, 0.025), 1.0, 1.0, dict(segs=16, rings=8))])
BW, BL, TC, TP = 0.48, 0.62, 0.03, 0.05                  # page width, length along the spine, cover, page block
ZH = TC + TP                                             # hinge height (top of the back page block)
kx, ky, bkpx = snap(-0.24, 0.02, TOPZ)
P_book = empty('P_book', (kx, ky, TOPZ), W0)
solid('book_cover_B', P_book, 'book', [((0, -BW / 2 - 0.01, TC / 2), (BL / 2 + 0.02, BW / 2 + 0.02, TC / 2), 0.3, 0.2)])
solid('book_pages_B', P_book, 'book', [((0, -BW / 2, TC + TP / 2), (BL / 2, BW / 2 - 0.005, TP / 2), 0.25, 0.12)])
P_front = empty('P_book_front', (0, 0, ZH), P_book)
solid('book_pages_F', P_front, 'book', [((0, -BW / 2, TP / 2), (BL / 2, BW / 2 - 0.005, TP / 2), 0.25, 0.12)])
solid('book_cover_F', P_front, 'book', [((0, -BW / 2 - 0.01, TP + TC / 2), (BL / 2 + 0.02, BW / 2 + 0.02, TC / 2), 0.3, 0.2)])
P_leaf = empty('P_book_leaf', (0, 0, ZH + 0.006), P_book)
solid('book_leaf', P_leaf, 'book', [((0, -BW / 2, 0), (BL / 2 - 0.01, BW / 2 - 0.01, 0.006), 0.2, 0.15)])
LEAF = O['book_leaf']

def set_book(st):
    P_front.rotation_euler = (math.radians(-st['cover']), 0, 0)
    P_leaf.rotation_euler = (math.radians(-max(st['leaf'], 0)), 0, 0)
    LEAF.hide_render = st['leaf'] < 0
    return f"c{st['cover']:g}_l{st['leaf']:g}"

SET_LAYERS = {'bed': ['bed'], 'read': ['table', 'pouf', 'book']}[SET]
ALL_PROP_OBJS = [o for l in LAYER_OBJS.values() for o in l]
for l, obs in LAYER_OBJS.items():
    if l not in SET_LAYERS:
        for o in obs: o.hide_render = True
if ORI == 'B': W0.matrix_world = Matrix(((0, -1, 0, 0), (-1, 0, 0, 0), (0, 0, 1, 0), (0, 0, 0, 1)))
vl.update()

def set_state(q, st):
    apply_pose(q)
    keys = {'bear': 'bear_' + bear_sig()}
    if SET == 'bed': keys['bed'] = 'bed_' + set_quilt(st)
    else:
        keys['book'] = 'book_' + set_book(st); keys['table'] = 'table_0'; keys['pouf'] = 'pouf_0'
    vl.update()
    return keys

def default_state():
    c = CLIPS[clips_b.ORDER[SET][0]][0]; return c

# ------------------------------------------------------------------ layout (canvas shared by all clips of a set)
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
    zzz = None
    for c in clips_b.ORDER[SET]:
        for f, (q, st) in enumerate(CLIPS[c]):
            set_state(q, st)
            grow(eval_pts(BEAR + [o for l in SET_LAYERS for o in LAYER_OBJS[l]]))
            if c == 'sleep' and f == 0:
                hp = np.array(O['body'].matrix_world @ Vector((0, -0.02, 1.64)))
                zzz = [float(hp @ Rn), float(hp @ Un)]
    if zzz:   # room for the Zzz effect (up to 30 px right/left and 40 px above the head)
        for sx in (-1, 1):
            ext[0] = min(ext[0], zzz[0] + sx * 34 / PPU); ext[1] = max(ext[1], zzz[0] + sx * 34 / PPU)
        ext[2] = max(ext[2], zzz[1] + 44 / PPU)
    OX = int(math.ceil(-ext[0] * PPU)) + 3; W = OX + int(math.ceil(ext[1] * PPU)) + 3
    OY = int(math.ceil(ext[2] * PPU)) + 3; H = OY + int(math.ceil(-ext[3] * PPU)) + 3
    W += W % 2; H += H % 2
    sx_ = -1 if ORI == 'B' else 1
    def mirp(p): return [sx_ * p[0], p[1]]
    lay = dict(set=SET, ori=ORI, W=W, H=H, origin=[OX, OY], k=K, ppu=PPU,
               zzz_head=[OX + zzz[0] * PPU, OY - zzz[1] * PPU] if zzz else None,
               geom={k: (list(v) if isinstance(v, tuple) else v) for k, v in G.items()},
               handoff={c: dict(h, px=mirp(bpx if SET == 'bed' else rpx),
                                yaw=(90.0 - h['yaw']) % 360 if ORI == 'B' else h['yaw'],
                                pos=[-h['pos'][1], -h['pos'][0]] if ORI == 'B' else list(h['pos']))
                        for c, h in HAND.items() if c in clips_b.ORDER[SET]},
               prop_px=dict(bed=[0, 0], table=[0, 0], pouf=mirp(ppx), book=mirp(bkpx)),
               prop_pos=dict(bed=[0, 0, 0], table=[0, 0, 0], pouf=[pfx, pfy, 0], book=[kx, ky, TOPZ]),
               pass_ids=PID)
    json.dump(lay, open(LAYOUT, 'w'), indent=1)
    print('LAYOUT', TAG, W, H, OX, OY)
    sys.exit(0)

lay = json.load(open(LAYOUT)); W, H = lay['W'], lay['H']; OX, OY = lay['origin']

# ------------------------------------------------------------------ camera + render settings (as Stage A)
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
    for o in show: o.hide_render = keep[o.name] if o in ALL_PROP_OBJS and o.name == 'book_leaf' else False
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
    q, st = default_state()
    keys = set_state(q, st)
    for l in SET_LAYERS: nr += render_layer(l, keys[l])
    json.dump({l: keys[l] for l in SET_LAYERS}, open(os.path.join(PD, 'props.json'), 'w'), indent=1)
    if SAVE:
        vl.material_override = None
        bpy.context.preferences.filepaths.save_version = 0
        bpy.ops.wm.save_as_mainfile(filepath=SAVE, compress=True); print('SAVED', SAVE)
else:
    frames = []
    for f, (q, st) in enumerate(CLIPS[CLIP]):
        keys = set_state(q, st)
        frames.append(dict(keys=keys, zzz=st['zzz']))
        if FRAMES is not None and f not in FRAMES: continue
        nr += render_layer('bear', keys['bear'])
        for l in SET_LAYERS:
            if l in ('table', 'pouf'): continue          # static: rendered once in props mode
            nr += render_layer(l, keys[l])
    json.dump(dict(clip=CLIP, n=len(frames), loop=CLIP in clips_b.LOOP, frames=frames),
              open(os.path.join(PD, f'clip_{CLIP}.json'), 'w'), indent=1)
if os.path.exists(tmp): os.remove(tmp)
print(f'DONE {TAG} {MODE} {CLIP or ""}: {nr} layer renders in {time.time() - t0:.1f}s')
