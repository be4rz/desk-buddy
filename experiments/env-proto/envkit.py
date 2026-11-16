"""Environment prototype kit (runs inside Blender 5 headless): model toy dioramas from primitives, render data passes.

    Blender -b -P <items>.py -- [--only name,name] [--k 8]

Everything drawn comes from Blender geometry + procedural textures; pix.py turns the passes into palette pixel art.
Passes per (variant, layer), supersampled K x K: normal | uid, view depth, AO | texture value + per-object random |
main sun (lambert x cast shadow) | glow lights (window light, lamps).
World frame = the bear set frame: origin = the bear's hand-off spot, +X = SE on screen, +Y = NE, +Z up.
"""
import bpy, bmesh, sys, os, math, json, time
import numpy as np
from mathutils import Vector, Matrix, Euler

HERE = os.path.dirname(os.path.abspath(__file__))
PPU = json.load(open(os.path.join(HERE, '..', 'mascot', 'bear3d', 'clips.json')))['ppu']
ROT = Euler((math.radians(60), 0, math.radians(45)), 'XYZ')
Rm = ROT.to_matrix(); RIGHT, UP, BACK = Rm.col[0].copy(), Rm.col[1].copy(), Rm.col[2].copy()
SUN = Vector((0.75, -0.35, 1.0)).normalized()          # surface -> light
PXH = 1.0 / (PPU * math.sqrt(0.5))                      # world length along X or Y of one screen pixel (horizontal)
PXV = 1.0 / (PPU * math.sin(math.radians(60)))          # world height of one screen pixel

argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
def arg(n, d=None): return argv[argv.index(n) + 1] if n in argv else d
OUT = arg('--raw', os.path.join(HERE, 'raw'))


def _rotm(rot): return Euler([math.radians(a) for a in rot], 'XYZ').to_matrix()
def sgp(w, e): return math.copysign(abs(w) ** e, w)


class M:
    """mesh builder: every primitive is placed at world coordinates (rot = XYZ euler degrees about its own centre c)."""
    def __init__(s): s.bm = bmesh.new()

    def _merge(s, tmp, c, rot, smooth):
        R = _rotm(rot); c = Vector(c)
        for v in tmp.verts: v.co = R @ v.co + c
        if smooth is not None:
            for f in tmp.faces: f.smooth = smooth
        me = bpy.data.meshes.new('tmp'); tmp.to_mesh(me); tmp.free(); s.bm.from_mesh(me); bpy.data.meshes.remove(me)
        return s

    def box(s, c, size, bevel=0.0, rot=(0, 0, 0), seg=1):
        """box centred at c; bevel > 0 rounds every edge (toy look)."""
        t = bmesh.new(); bmesh.ops.create_cube(t, size=1.0)
        for v in t.verts: v.co = Vector((v.co.x * size[0], v.co.y * size[1], v.co.z * size[2]))
        if bevel > 0:
            bmesh.ops.bevel(t, geom=list(t.edges), offset=min(bevel, min(size) * 0.49), segments=seg, profile=0.5,
                            affect='EDGES')
        return s._merge(t, c, rot, False)

    def slab(s, x0, x1, y0, y1, z0, z1, bevel=0.0):
        return s.box(((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2), (abs(x1 - x0), abs(y1 - y0), abs(z1 - z0)), bevel)

    def cyl(s, c, r, h, r2=None, segs=28, rot=(0, 0, 0), smooth=True):
        """cylinder / cone frustum standing on c (base centre), axis +Z before rot."""
        t = bmesh.new()
        bmesh.ops.create_cone(t, cap_ends=True, cap_tris=False, segments=segs, radius1=r,
                              radius2=r if r2 is None else r2, depth=h)
        for v in t.verts: v.co.z += h / 2
        for f in t.faces: f.smooth = smooth and abs(f.normal.z) < 0.9
        return s._merge(t, c, rot, None)

    def sq(s, c, radii, e1=1.0, e2=1.0, rot=(0, 0, 0), segs=32, rings=16):
        """superquadric blob centred at c (e = 1 ellipsoid, < 1 boxier, the bear props' shape language)."""
        t = bmesh.new(); bmesh.ops.create_uvsphere(t, u_segments=segs, v_segments=rings, radius=1.0)
        for v in t.verts:
            x, y, z = v.co; phi = math.asin(max(-1.0, min(1.0, z))); th = math.atan2(y, x)
            v.co = (sgp(math.cos(phi), e1) * sgp(math.cos(th), e2) * radii[0],
                    sgp(math.cos(phi), e1) * sgp(math.sin(th), e2) * radii[1], sgp(math.sin(phi), e1) * radii[2])
        return s._merge(t, c, rot, True)

    def tube(s, pts, radii, segs=14, flat=1.0, yaw=0.0):
        """one smooth skin through horizontal rings at pts (a mostly vertical path): drapes, trunks, vines.
        flat < 1 squashes the rings along the direction yaw + 90 deg."""
        t = bmesh.new(); rings = []; cy, sy = math.cos(math.radians(yaw)), math.sin(math.radians(yaw))
        for p, r in zip(pts, radii):
            ring = []
            for i in range(segs):
                a = 2 * math.pi * i / segs; u, v = r * math.cos(a), r * flat * math.sin(a)
                ring.append(t.verts.new((p[0] + u * cy - v * sy, p[1] + u * sy + v * cy, p[2])))
            rings.append(ring)
        for a, b in zip(rings, rings[1:]):
            for i in range(segs): t.faces.new((a[i], a[(i + 1) % segs], b[(i + 1) % segs], b[i])).smooth = True
        t.faces.new(rings[0]); t.faces.new(rings[-1])
        bmesh.ops.recalc_face_normals(t, faces=list(t.faces))
        return s._merge(t, (0, 0, 0), (0, 0, 0), None)

    def lathe(s, c, prof, segs=28, rot=(0, 0, 0), smooth=True, cap=True):
        """surface of revolution about +Z through c; prof = [(r, z), ...] bottom -> top (closed with caps)."""
        t = bmesh.new(); rings = []
        for r, z in prof:
            rings.append([t.verts.new((r * math.cos(2 * math.pi * i / segs), r * math.sin(2 * math.pi * i / segs), z))
                          for i in range(segs)])
        for a, b in zip(rings, rings[1:]):
            for i in range(segs):
                f = t.faces.new((a[i], a[(i + 1) % segs], b[(i + 1) % segs], b[i])); f.smooth = smooth
        if cap: t.faces.new(list(reversed(rings[0]))); t.faces.new(rings[-1])
        bmesh.ops.recalc_face_normals(t, faces=list(t.faces))
        return s._merge(t, c, rot, None)

    def poly(s, c, pts, depth, rot=(0, 0, 0), bevel=0.0):
        """2D polygon (XY, counter-clockwise) extruded +Z by depth, placed at c."""
        t = bmesh.new()
        lo = [t.verts.new((x, y, 0.0)) for x, y in pts]; hi = [t.verts.new((x, y, depth)) for x, y in pts]
        n = len(pts)
        t.faces.new(list(reversed(lo))); t.faces.new(hi)
        for i in range(n): t.faces.new((lo[i], lo[(i + 1) % n], hi[(i + 1) % n], hi[i]))
        bmesh.ops.recalc_face_normals(t, faces=list(t.faces))
        if bevel > 0: bmesh.ops.bevel(t, geom=list(t.edges), offset=bevel, segments=1, profile=0.5, affect='EDGES')
        return s._merge(t, c, rot, False)

    def pipe(s, pts, r, r2=None, segs=12):
        """round rod through the points (a capped cylinder per span + a ball per joint)."""
        n = len(pts) - 1
        for i in range(n):
            a, b = Vector(pts[i]), Vector(pts[i + 1]); d = b - a; L = d.length
            if L < 1e-6: continue
            if isinstance(r, (list, tuple)): ra, rb = r[i], r[i + 1]
            else: ra = r if r2 is None else r + (r2 - r) * i / n; rb = r if r2 is None else r + (r2 - r) * (i + 1) / n
            t = bmesh.new()
            bmesh.ops.create_cone(t, cap_ends=True, segments=segs, radius1=ra, radius2=rb, depth=L)
            for v in t.verts: v.co.z += L / 2
            q = Vector((0, 0, 1)).rotation_difference(d.normalized()).to_matrix()
            for v in t.verts: v.co = q @ v.co + a
            for f in t.faces: f.smooth = True
            me = bpy.data.meshes.new('tmp'); t.to_mesh(me); t.free(); s.bm.from_mesh(me); bpy.data.meshes.remove(me)
            if 0 < i: s.sq(pts[i], (ra, ra, ra), segs=segs, rings=6)
        return s

    def blob_poly(s, c, radius, depth, n=28, wob=0.12, seed=0, squash=1.0, bevel=0.0):
        """organic rounded island outline (a wobbly circle) extruded by depth, top at c.z."""
        pts = []
        for i in range(n):
            a = 2 * math.pi * i / n
            w = 1 + wob * (math.sin(3 * a + seed) * 0.6 + math.sin(5 * a + 2.1 * seed) * 0.4)
            pts.append((radius * w * math.cos(a), radius * w * squash * math.sin(a)))
        return s.poly((c[0], c[1], c[2] - depth), pts, depth, bevel=bevel)


class Scene:
    def __init__(s, name, note=''):
        bpy.ops.wm.read_factory_settings(use_empty=True)
        s.name, s.note = name, note
        s.sc = bpy.context.scene; s.vl = bpy.context.view_layer
        s.parts = {}; s.pnames = []; s.uids = [[0, 0]]; s.groups = {'': 0}; s.objs = []
        s.lights = []; s.variants = {'': dict(amb=0)}
        s.ext = [1e9, -1e9, -1e9, 1e9]
        s.cur = dict(group='', layer='back', var=None)
        s.bear = None; s.pal = _palette()

    # ---------------------------------------------------------------- parts
    def part(s, name, ramp, idx, tex=None, amp=0.0, rnd=0.0, tstep=None, dither=0.0, sh='lit', gmax=2, seam=True,
             thin=None, prio=0, noline=False, shadow=True, thresh=None):
        """a material: ramp name + the ramp indices of its tones (dark -> light). tex = procedural value in [0, 1]
        (amp: added to the shade; tstep (lo, hi): tone -1 below lo, +1 above hi); rnd = per-object tone jitter;
        sh 'tex' = unlit (tone from tex alone, + ordered dither); gmax = max glow tone steps; seam = soft lines
        between its objects; thin = decal (claims a pixel at that coverage); noline = no inner lines on or around it."""
        assert name not in s.parts, name
        pid = len(s.pnames) + 1; s.pnames.append(name)
        s.parts[name] = dict(pid=pid, ramp=ramp, colors=s.pal[ramp], idx=list(idx), amp=amp, rnd=rnd, tstep=tstep,
                             dither=dither, sh=sh, gmax=gmax, seam=seam, thin=thin, prio=prio, noline=noline,
                             thresh=thresh, shadow=shadow, mat=_tex_material('T_' + name, tex))
        return name

    def set(s, **kw): s.cur.update(kw); return s

    def add(s, part, m, group=None, layer=None, var=None, cam=True, only=None):
        """one object (= one uid: seams run between objects of a group; ink runs between groups)."""
        p = s.parts[part]; g = s.cur['group'] if group is None else group
        gid = s.groups.setdefault(g, len(s.groups))
        me = bpy.data.meshes.new(part); m.bm.to_mesh(me); m.bm.free()
        ob = bpy.data.objects.new(f'{part}_{len(s.uids)}', me); s.sc.collection.objects.link(ob)
        me.materials.append(p['mat']); ob.pass_index = len(s.uids); s.uids.append([p['pid'], gid])
        ob.visible_camera = cam; ob.visible_shadow = p['shadow']
        rec = dict(ob=ob, layer=layer or s.cur['layer'], var=var if var is not None else s.cur['var'], cam=cam, only=only)
        s.objs.append(rec)
        if cam:
            co = np.empty(len(me.vertices) * 3, np.float32); me.vertices.foreach_get('co', co); co = co.reshape(-1, 3)
            u, v = co @ np.array(RIGHT), co @ np.array(UP)
            s.ext = [min(s.ext[0], u.min()), max(s.ext[1], u.max()), max(s.ext[2], v.max()), min(s.ext[3], v.min())]
        return ob

    # ---------------------------------------------------------------- lights
    def glow_sun(s, direction, var=None, strength=math.pi):
        """a parallel glow light (window light); direction = the way the light travels."""
        ld = bpy.data.lights.new('GlowSun', 'SUN'); ld.energy = strength; ld.angle = 0.0
        ob = bpy.data.objects.new('GlowSun', ld); s.sc.collection.objects.link(ob)
        ob.rotation_euler = Vector(direction).normalized().to_track_quat('-Z', 'Y').to_euler()
        s.lights.append(dict(ob=ob, var=var)); return ob

    def glow_point(s, loc, power, var=None, radius=0.0):
        ld = bpy.data.lights.new('Glow', 'POINT'); ld.energy = power; ld.shadow_soft_size = radius
        ob = bpy.data.objects.new('Glow', ld); s.sc.collection.objects.link(ob); ob.location = loc
        s.lights.append(dict(ob=ob, var=var)); return ob

    def blocker(s, m):
        """shadow-only geometry for the glow pass (keeps window light inside the window shape)."""
        if '_blk' not in s.parts: s.part('_blk', 'stone', (1, 2))
        return s.add('_blk', m, cam=False, only='G')

    def bear_shadow(s, at=(0, 0), r=(0.74, 0.74, 0.16), z=0.20):
        """an unseen low blob where the bear is: bakes its contact shadow and AO into the floor."""
        if '_blk' not in s.parts: s.part('_blk', 'stone', (1, 2))
        return s.add('_blk', M().sq((at[0], at[1], z), r), cam=False)

    def variant(s, name, **kw): s.variants.pop('', None); s.variants[name] = dict(dict(amb=0), **kw)

    # ---------------------------------------------------------------- render
    def render(s, k=None):
        k = int(arg('--k', k or 8)); sc, vl = s.sc, s.vl
        OX = int(math.ceil(-s.ext[0] * PPU)) + 3; W = OX + int(math.ceil(s.ext[1] * PPU)) + 3
        OY = int(math.ceil(s.ext[2] * PPU)) + 3; H = OY + int(math.ceil(-s.ext[3] * PPU)) + 3
        W += W % 2; H += H % 2
        cd = bpy.data.cameras.new('IsoCam'); cd.type = 'ORTHO'
        cd.ortho_scale = max(W, H) / PPU; cd.clip_start = 0.01; cd.clip_end = 200
        cam = bpy.data.objects.new('IsoCam', cd); sc.collection.objects.link(cam); cam.rotation_euler = ROT
        cam.location = RIGHT * ((W / 2 - OX) / PPU) + UP * ((OY - H / 2) / PPU) + BACK * 60.0
        sc.camera = cam
        sun = bpy.data.objects.new('Sun', bpy.data.lights.new('Sun', 'SUN')); sc.collection.objects.link(sun)
        sun.data.energy = math.pi; sun.data.angle = 0.0
        sun.rotation_euler = (-SUN).to_track_quat('-Z', 'Y').to_euler()
        sc.render.engine = 'CYCLES'; cy = sc.cycles
        cy.samples = 1; cy.use_denoising = False; cy.use_adaptive_sampling = False
        cy.max_bounces = 0; cy.diffuse_bounces = 0; cy.glossy_bounces = 0; cy.transmission_bounces = 0
        cy.filter_width = 0.01; cy.pixel_filter_type = 'BOX'
        sc.render.film_transparent = True
        sc.render.resolution_x = W * k; sc.render.resolution_y = H * k; sc.render.resolution_percentage = 100
        sc.view_settings.view_transform = 'Raw'
        im = sc.render.image_settings
        im.file_format = 'OPEN_EXR'; im.color_mode = 'RGBA'; im.color_depth = '32'; im.exr_codec = 'ZIP'
        M_N = _override('OV_normal', _b_nrm); M_D = _override('OV_iddepth', _b_idd); M_W = _white()
        rd = os.path.join(OUT, s.name); os.makedirs(rd, exist_ok=True); tmp = os.path.join(rd, '_tmp.exr')

        def shot(mat):
            vl.material_override = mat; sc.render.filepath = tmp
            bpy.ops.render.render(write_still=True)
            img = bpy.data.images.load(tmp); img.colorspace_settings.name = 'Non-Color'
            w, h = img.size; a = np.empty(w * h * 4, np.float32); img.pixels.foreach_get(a); bpy.data.images.remove(img)
            return a.reshape(h, w, 4)[::-1]

        def on(rec, v): return rec['var'] is None or v == rec['var'] or (isinstance(rec['var'], (tuple, list)) and v in rec['var'])
        layers = ['all'] + sorted({o['layer'] for o in s.objs if o['layer'] != 'back' and o['cam']})
        t0 = time.time()
        for v in s.variants:
            has_glow = any(on(l, v) for l in s.lights)
            for layer in layers:
                for o in s.objs:
                    o['ob'].hide_render = not on(o, v) or o['only'] is not None
                    o['ob'].visible_camera = o['cam'] and (layer == 'all' or o['layer'] == layer)
                for l in s.lights: l['ob'].hide_render = True
                sun.hide_render = True
                n = shot(M_N); d = shot(M_D); t = shot(None)
                sun.hide_render = False; sv = shot(M_W)[:, :, 0]; sun.hide_render = True
                g = np.zeros_like(sv)
                if has_glow:
                    for o in s.objs: o['ob'].hide_render = not on(o, v) or o['only'] not in (None, 'G')
                    for l in s.lights: l['ob'].hide_render = not on(l, v)
                    g = shot(M_W)[:, :, 0]
                tag = (v or 'base') + '__' + layer
                np.savez_compressed(os.path.join(rd, tag + '.npz'), nrm=n[:, :, :3].astype(np.float16),
                                    alpha=d[:, :, 3] > 0.5, uid=np.rint(d[:, :, 0]).astype(np.int16), depth=d[:, :, 1],
                                    ao=d[:, :, 2].astype(np.float16), tex=t[:, :, :2].astype(np.float16),
                                    sun=sv.astype(np.float16), glow=g.astype(np.float16))
                print(f'  pass {s.name} {tag} {time.time() - t0:.0f}s', flush=True)
        if os.path.exists(tmp): os.remove(tmp)
        parts = {p['pid']: dict({kk: vv for kk, vv in p.items() if kk != 'mat'}, name=n) for n, p in s.parts.items()}
        json.dump(dict(name=s.name, note=s.note, k=k, W=W, H=H, origin=[OX, OY], ppu=PPU, sun=list(SUN),
                       right=list(RIGHT), up=list(UP), back=list(BACK), parts=parts, uids=s.uids,
                       variants=s.variants, layers=layers, bear=s.bear), open(os.path.join(rd, 'meta.json'), 'w'), indent=1)
        print(f'DONE {s.name} {W}x{H} {len(s.objs)} objects {time.time() - t0:.0f}s', flush=True)


def _palette():
    E = os.path.join(HERE, '..'); pal = {}
    for f in (os.path.join(E, 'scene-proto', 'palette.json'), os.path.join(E, 'bear-proto', 'palette-bear.json'),
              os.path.join(E, 'mascot', 'bear3d', 'palette-ext.json'), os.path.join(E, 'mascot', 'bear3d', 'palette-ext-out.json'),
              os.path.join(HERE, 'palette-env.json')):
        pal.update({k: v for k, v in json.load(open(f)).items() if k != 'note'})
    return pal


# ---------------------------------------------------------------- pass materials
def _override(name, build):
    m = bpy.data.materials.new(name); m.use_nodes = True; nt = m.node_tree; nt.nodes.clear()
    out = nt.nodes.new('ShaderNodeOutputMaterial'); em = nt.nodes.new('ShaderNodeEmission')
    nt.links.new(em.outputs[0], out.inputs['Surface']); nt.links.new(build(nt), em.inputs['Color'])
    return m
def _b_nrm(nt):
    g = nt.nodes.new('ShaderNodeNewGeometry'); ma = nt.nodes.new('ShaderNodeVectorMath'); ma.operation = 'MULTIPLY_ADD'
    ma.inputs[1].default_value = (0.5, 0.5, 0.5); ma.inputs[2].default_value = (0.5, 0.5, 0.5)
    nt.links.new(g.outputs['Normal'], ma.inputs[0]); return ma.outputs[0]
def _b_idd(nt):
    oi = nt.nodes.new('ShaderNodeObjectInfo'); cdn = nt.nodes.new('ShaderNodeCameraData')
    ao = nt.nodes.new('ShaderNodeAmbientOcclusion'); ao.samples = 16; ao.only_local = False
    ao.inputs['Distance'].default_value = 0.45
    cb = nt.nodes.new('ShaderNodeCombineXYZ')
    nt.links.new(oi.outputs['Object Index'], cb.inputs[0]); nt.links.new(cdn.outputs['View Z Depth'], cb.inputs[1])
    nt.links.new(ao.outputs['AO'], cb.inputs[2]); return cb.outputs[0]
def _white():
    m = bpy.data.materials.new('OV_white'); m.use_nodes = True; nt = m.node_tree; nt.nodes.clear()
    out = nt.nodes.new('ShaderNodeOutputMaterial'); d = nt.nodes.new('ShaderNodeBsdfDiffuse')
    d.inputs['Color'].default_value = (1, 1, 1, 1); nt.links.new(d.outputs[0], out.inputs['Surface']); return m


def _tex_material(name, spec):
    m = bpy.data.materials.new(name); m.use_nodes = True; nt = m.node_tree; nt.nodes.clear()
    out = nt.nodes.new('ShaderNodeOutputMaterial'); em = nt.nodes.new('ShaderNodeEmission')
    nt.links.new(em.outputs[0], out.inputs['Surface'])
    oi = nt.nodes.new('ShaderNodeObjectInfo'); cb = nt.nodes.new('ShaderNodeCombineXYZ')
    nt.links.new(_tex(nt, spec, oi), cb.inputs[0]); nt.links.new(oi.outputs['Random'], cb.inputs[1])
    nt.links.new(cb.outputs[0], em.inputs['Color']); return m


def _math(nt, op, a, b=None, clamp=False):
    n = nt.nodes.new('ShaderNodeMath'); n.operation = op; n.use_clamp = clamp
    for i, x in enumerate((a, b)):
        if x is None: continue
        if isinstance(x, (int, float)): n.inputs[i].default_value = x
        else: nt.links.new(x, n.inputs[i])
    return n.outputs[0]


def _coord(nt, oi, perobj=0.0):
    """world position (objects sit at the origin, unrotated), optionally shifted per object."""
    g = nt.nodes.new('ShaderNodeNewGeometry'); p = g.outputs['Position']
    if perobj:
        a = nt.nodes.new('ShaderNodeVectorMath'); a.operation = 'ADD'
        sv = nt.nodes.new('ShaderNodeVectorMath'); sv.operation = 'SCALE'
        c = nt.nodes.new('ShaderNodeCombineXYZ')
        for i in range(3): nt.links.new(oi.outputs['Random'], c.inputs[i])
        nt.links.new(c.outputs[0], sv.inputs[0]); sv.inputs['Scale'].default_value = perobj
        nt.links.new(p, a.inputs[0]); nt.links.new(sv.outputs[0], a.inputs[1]); p = a.outputs[0]
    return p


def _dot(nt, p, vec):
    d = nt.nodes.new('ShaderNodeVectorMath'); d.operation = 'DOT_PRODUCT'
    nt.links.new(p, d.inputs[0]); d.inputs[1].default_value = vec; return d.outputs['Value']


def _tex(nt, spec, oi):
    """spec -> a float socket in [0, 1]. Kinds:
    None | ('const', v) | ('grad', dir, lo, hi) | ('noise', scale, stretch, detail[, perobj]) |
    ('stripe', dir, period) triangle wave | ('grid', dirA, perA, dirB, perB, width) 0 on the lines |
    ('dots', scale, radius[, stretch]) 1 inside sparse dots | ('mix', a, b, w) | ('mul', a, b)"""
    if spec is None: spec = ('const', 0.5)
    k = spec[0]
    if k == 'const':
        v = nt.nodes.new('ShaderNodeValue'); v.outputs[0].default_value = spec[1]; return v.outputs[0]
    if k == 'grad':
        d = _dot(nt, _coord(nt, oi), spec[1])
        return _math(nt, 'DIVIDE', _math(nt, 'SUBTRACT', d, spec[2]), spec[3] - spec[2], clamp=True)
    if k == 'noise':
        mp = nt.nodes.new('ShaderNodeMapping'); mp.inputs['Scale'].default_value = spec[2]
        nt.links.new(_coord(nt, oi, spec[4] if len(spec) > 4 else 0.0), mp.inputs['Vector'])
        n = nt.nodes.new('ShaderNodeTexNoise'); n.inputs['Scale'].default_value = spec[1]
        n.inputs['Detail'].default_value = spec[3]; nt.links.new(mp.outputs[0], n.inputs['Vector'])
        return _math(nt, 'ADD', _math(nt, 'MULTIPLY', _math(nt, 'SUBTRACT', n.outputs[0], 0.5), 1.6), 0.5, clamp=True)
    if k == 'stripe':
        f = _math(nt, 'FRACT', _math(nt, 'DIVIDE', _dot(nt, _coord(nt, oi), spec[1]), spec[2]))
        return _math(nt, 'ABSOLUTE', _math(nt, 'SUBTRACT', _math(nt, 'MULTIPLY', f, 2.0), 1.0))
    if k == 'grid':
        p = _coord(nt, oi); outs = []
        for dv, per in ((spec[1], spec[2]), (spec[3], spec[4])):
            f = _math(nt, 'FRACT', _math(nt, 'DIVIDE', _dot(nt, p, dv), per))
            outs.append(_math(nt, 'GREATER_THAN', _math(nt, 'MULTIPLY', f, per), spec[5]))
        return _math(nt, 'MINIMUM', outs[0], outs[1])
    if k == 'dots':
        mp = nt.nodes.new('ShaderNodeMapping'); mp.inputs['Scale'].default_value = spec[3] if len(spec) > 3 else (1, 1, 1)
        nt.links.new(_coord(nt, oi), mp.inputs['Vector'])
        vo = nt.nodes.new('ShaderNodeTexVoronoi'); vo.inputs['Scale'].default_value = spec[1]
        nt.links.new(mp.outputs[0], vo.inputs['Vector'])
        return _math(nt, 'LESS_THAN', vo.outputs['Distance'], spec[2])
    if k == 'mix':
        a, b, w = _tex(nt, spec[1], oi), _tex(nt, spec[2], oi), spec[3]
        return _math(nt, 'ADD', _math(nt, 'MULTIPLY', a, 1 - w), _math(nt, 'MULTIPLY', b, w))
    if k == 'mul':
        return _math(nt, 'MULTIPLY', _tex(nt, spec[1], oi), _tex(nt, spec[2], oi))
    raise ValueError(spec)


def only(name):
    o = arg('--only'); return o is None or name in o.split(',')
