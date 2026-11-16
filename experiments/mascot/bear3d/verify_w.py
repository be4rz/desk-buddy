"""Verify stage w (edge-art; out/bear3d/anim_w/, stages/w.json, clips_w.py):
python3 verify_w.py [--dir DIR] [--self-test]   (exit 0 pass / 1 findings; --self-test exit 1 = every mutation
caught, 2 = one missed).

The kit's standard checks (stagekit.verify_main: pixel rules, counts, dirs, idle#0 hand-offs anchor-aligned, loop
seams, root_px, CHAINS = the base-pose joins) plus stage w's own:
  - every clip has the storyboard frame 84x139, anchor [42, 107] (the edge line), and its face handling;
  - hand-off fields: edge_sit starts on idle#0, edge_stand ends on idle#0, the others have none (their base-pose
    ends are clips_w.CHAINS, pixel-equal to edge_dangle#0);
  - no root motion (the bear stays on its spot of the edge) and the art never touches the canvas edge;
  - the face part face_w.json covers exactly the layers clip edge_dangle in S SE E W SW, with frame 0 exact;
  - seated: on every clips_w.SEATED frame, each opaque pixel below the edge line (rows > anchor y) lies on a leg or
    its pad: inside the screen hull of that side's posed leg stub + pad (one hull per side), within 1.5 px (the ink
    outline and rounding). The leg stub is the lower body that rig_bear.py LEGS swings (body + pants with a leg weight
    >= 0.5, z <= 0.52), kept as LEG_SLICES (bear.blend rest geometry, measured once: per 0.06 z slice, the convex
    outline of the left half as 12 support distances); each slice point is posed as the rig does it (its hip
    rotation blended by its leg weights, dual-quaternion style, then the body's bob / squash), the pad box (FOOT) rides
    its leg (blend over 15 deg, + fL / hL); then the yaw and the kit camera. Only the legs and their pads may hang; the
    body above the hips stays on or above the line.
Stage mutations: an ink-rimmed fur blob below the edge line under the legs of a seated frame; the seat sunk 3 px
below the line on a seated frame; a base-pose join broken (edge_peer's last frame shifted 1 px).
"""
import math
import numpy as np
import stagekit as K

FRAME, ANCHOR = [84, 139], [42, 107]
FACE_DIRS = ['S', 'SE', 'E', 'W', 'SW']
# the pads' rest bounds (bear.blend, world units, measured once: x, y, z) - bear-local, the bear faces -y
FOOT = {'L': ((0.09, 0.53), (-0.44, 0.14), (-0.01, 0.27)), 'R': ((-0.53, -0.09), (-0.44, 0.14), (-0.01, 0.27))}
SLACK = 1.5
# the leg stubs (rig_bear.py LEGS: hip axis x, y 0, z HIP_Z; weight ease((LEG_Z1 - z) / (LEG_Z1 - LEG_Z0)), split
# left / right by ease((x + LEG_SX) / (2 LEG_SX))) and their rest outline: body + pants vertices with z in each slice,
# the left half (x >= 0, the right half mirrored in), as max(x cos p + y sin p) for p = 0, 30, ... 330 deg (bear.blend,
# measured once). z <= 0.52 = leg weight >= 0.5 (the body above stays on or above the edge line).
HIP_Z, LEG_Z0, LEG_Z1, LEG_SX = 0.50, 0.36, 0.66, 0.08
LEG_SLICES = [
    (-0.02, 0.04, [0.496, 0.452, 0.339, 0.168, -0.002, -0.131, -0.185, -0.131, -0.002, 0.167, 0.339, 0.452]),
    (0.04, 0.10, [0.554, 0.516, 0.406, 0.245, 0.077, -0.052, -0.101, -0.055, 0.073, 0.243, 0.404, 0.515]),
    (0.10, 0.16, [0.584, 0.55, 0.445, 0.282, 0.126, 0.005, -0.032, 0.002, 0.118, 0.277, 0.445, 0.55]),
    (0.16, 0.22, [0.612, 0.58, 0.475, 0.325, 0.178, 0.084, -0.01, 0.073, 0.167, 0.32, 0.472, 0.577]),
    (0.22, 0.28, [0.661, 0.628, 0.531, 0.391, 0.322, 0.18, -0.008, 0.15, 0.27, 0.375, 0.523, 0.624]),
    (0.28, 0.34, [0.672, 0.641, 0.549, 0.471, 0.402, 0.226, -0.01, 0.2, 0.357, 0.419, 0.538, 0.636]),
    (0.34, 0.40, [0.676, 0.648, 0.565, 0.52, 0.444, 0.25, -0.01, 0.227, 0.404, 0.473, 0.546, 0.641]),
    (0.40, 0.46, [0.68, 0.654, 0.589, 0.573, 0.49, 0.277, -0.01, 0.255, 0.452, 0.528, 0.559, 0.646]),
    (0.46, 0.52, [0.684, 0.662, 0.619, 0.599, 0.513, 0.29, -0.01, 0.273, 0.484, 0.566, 0.588, 0.651]),
]


def _ease(t): return t * t * (3 - 2 * t)          # anim_clips.ease


def _outline(h):
    """the convex polygon of 12 support lines (corners of consecutive lines): an outer bound of the slice's half."""
    n = len(h); ph = [math.radians(360.0 * i / n) for i in range(n)]; pts = []
    for i in range(n):
        j = (i + 1) % n; a1, a2 = ph[i], ph[j]
        det = math.cos(a1) * math.sin(a2) - math.sin(a1) * math.cos(a2)
        pts.append(((h[i] * math.sin(a2) - h[j] * math.sin(a1)) / det, (math.cos(a1) * h[j] - math.cos(a2) * h[i]) / det))
    return pts


def _hip(y, z, a):
    """rotate (y, z) about the hip axis by a leg swing of a deg (+ = forward, rig_bear.py about(hip, rx(-a)))."""
    t = math.radians(-a); c, s = math.cos(t), math.sin(t); z -= HIP_Z
    return y * c - z * s, y * s + z * c + HIP_Z


def _body(q, x, y, z, scale=True):
    """the body's rigid motion (bob, roll, lean) and squash (rig_bear.py deltas: rigid @ scl)."""
    if scale:
        sxy = 1 / math.sqrt(q['sz']); x, y, z = x * sxy, y * sxy, z * q['sz']
    t = math.radians(q['lean']); y, z = y * math.cos(t) - z * math.sin(t), y * math.sin(t) + z * math.cos(t)
    t = math.radians(q['roll']); x, z = x * math.cos(t) + z * math.sin(t), -x * math.sin(t) + z * math.cos(t)
    return x, y, z + q['bob']


def leg_points(q, side):
    """world points (bear-local) outlining the posed leg stub + pad of side 'L' or 'R' in pose q."""
    k = 1 if side == 'L' else -1; pts = []
    a_l, a_r = math.radians(-q['legL']) / 2, math.radians(-q['legR']) / 2
    for z0, z1, h in LEG_SLICES:
        for z in (z0, (z0 + z1) / 2, z1):
            w = _ease(min(max((LEG_Z1 - z) / (LEG_Z1 - LEG_Z0), 0.0), 1.0))
            for x, y in _outline(h):
                x *= k
                sl = _ease(min(max((x + LEG_SX) / (2 * LEG_SX), 0.0), 1.0)); wl, wr = w * sl, w * (1 - sl)
                ang = 2 * math.degrees(math.atan2(wl * math.sin(a_l) + wr * math.sin(a_r),
                                                  (1 - wl - wr) + wl * math.cos(a_l) + wr * math.cos(a_r)))
                y2, z2 = _hip(y, z, -ang)
                pts.append(_body(q, x, y2, z2))
    a = q['leg' + side]; t = _ease(min(abs(a) / 15.0, 1.0)); xs, ys, zs = FOOT[side]
    for x in xs:
        for y in ys:
            for z in zs:
                y2, z2 = _hip(y, z, a)
                bx, by, bz = _body(q, x, y2, z2, scale=False)
                bx, by, bz = x + t * (bx - x), y2 + t * (by - y2), z2 + t * (bz - z2)
                pts.append((bx, by - q['f' + side], bz + q['h' + side]))
    return pts


def _vp(ppu):
    return 2.0 / (K.RIGHT[0] * ppu), 1.0 / (K.UP[2] * ppu)      # V (2 px on an iso axis), PXZ (as rig_bear.py)


def _hull(pts):
    pts = sorted(set(pts))
    def cross(o, a, b): return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lo, hi = [], []
    for p in pts:
        while len(lo) >= 2 and cross(lo[-2], lo[-1], p) <= 0: lo.pop()
        lo.append(p)
    for p in reversed(pts):
        while len(hi) >= 2 and cross(hi[-2], hi[-1], p) <= 0: hi.pop()
        hi.append(p)
    return lo[:-1] + hi[:-1]                      # counter-clockwise


def leg_hulls(ppu, yaw, q):
    """screen hulls (px from the anchor, y down) of the two posed legs (stub + pad) in pose q at yaw."""
    t = math.radians(yaw); c, s = math.cos(t), math.sin(t)
    return [_hull([tuple(K.px_of(ppu, x * c - y * s, x * s + y * c, z)) for x, y, z in leg_points(q, side)])
            for side in ('L', 'R')]


def _near(h, x, y):
    """(x, y) inside the convex hull h or within SLACK px of it."""
    inside = True
    for i in range(len(h)):
        (ax, ay), (bx, by) = h[i], h[(i + 1) % len(h)]
        cr = (bx - ax) * (y - ay) - (by - ay) * (x - ax)
        if cr < 0: inside = False
        L2 = (bx - ax) ** 2 + (by - ay) ** 2
        u = 0.0 if L2 == 0 else max(0.0, min(1.0, ((x - ax) * (bx - ax) + (y - ay) * (by - ay)) / L2))
        if math.hypot(x - ax - u * (bx - ax), y - ay - u * (by - ay)) <= SLACK: return True
    return inside


def seat_errors(st, C):
    e = []; ppu = st.J['ppu']; mod = st.mod
    for c, sel in mod.SEATED.items():
        a = st.J['anims'].get(c)
        if not a or c not in C: continue
        poses = __import__('anim_clips').clip_poses(C[c])
        idx = range(a['frames_n']) if sel is None else [i % a['frames_n'] for i in sel]
        ax, ay = a['anchor']
        for d in a['files']:
            fs = st.frames.get((c, d))
            if not fs: continue
            for i in idx:
                op = fs[i][..., 3] > 0
                ys, xs = np.nonzero(op[ay + 1:]); ys = ys + ay + 1
                if not len(ys): continue
                hs = leg_hulls(ppu, K.YAW[d], poses[i])
                bad = [(x, y) for y, x in zip(ys, xs) if not any(_near(h, x + 0.5 - ax, y + 0.5 - ay) for h in hs)]
                if bad:
                    e.append(f'{c} {d} f{i}: {len(bad)} opaque px below the edge line that are not a leg '
                             f'(e.g. x{bad[0][0]} y{bad[0][1]}; only the legs and their pads may hang)')
    return e


def extra(st):
    e = []; mod = st.mod; J = st.J
    V, PXZ = _vp(J['ppu']); C = mod.build(V, PXZ)
    for c, a in J['anims'].items():
        cd = mod.CLIPS.get(c)
        if not cd: e.append(f'{c}: not in clips_w.CLIPS'); continue
        if [a['frame_w'], a['frame_h']] != FRAME or list(a['anchor']) != ANCHOR:
            e.append(f'{c}: frame {a["frame_w"]}x{a["frame_h"]} anchor {a["anchor"]} != storyboard {FRAME} {ANCHOR}')
        if (a.get('face') or 'none') != cd['face']: e.append(f'{c}: face {a.get("face")!r} != storyboard {cd["face"]!r}')
        if (a.get('handoff') or None) != (cd.get('handoff') or None):
            e.append(f'{c}: hand-off {a.get("handoff")} != storyboard {cd.get("handoff")}')
        for d in a['files']:
            if any(abs(x) > 1e-9 or abs(y) > 1e-9 for x, y in (a.get('root_px') or {}).get(d) or []):
                e.append(f'{c} {d}: root_px moves the panel (the bear stays on its spot of the edge)')
            for i, f in enumerate(st.frames.get((c, d), [])):
                al = f[..., 3]
                if al[0].any() or al[-1].any() or al[:, 0].any() or al[:, -1].any():
                    e.append(f'{c} {d} f{i}: art touches the canvas edge'); break
    want = {'edge_sit': {'start': 'idle0'}, 'edge_stand': {'end': 'idle0'}}
    for c in mod.ORDER:
        if (mod.CLIPS[c].get('handoff') or None) != want.get(c):
            e.append(f'{c}: clips_w hand-off {mod.CLIPS[c].get("handoff")} != storyboard {want.get(c)}')
    for c1, f1, c2, f2 in mod.CHAINS:                  # the kit checks these too; name a missing strip here
        for d in FACE_DIRS:
            if (c1, d) not in st.frames or (c2, d) not in st.frames: e.append(f'chain {c1}[{f1}] = {c2}[{f2}] {d}: strip missing')
    part = st.part
    if part is None:
        e.append(f'face_{st.sid}.json missing (the layers clip {mod.LAYERS} needs a face part)')
    else:
        pc = part.get('clips') or {}
        if sorted(pc) != sorted(mod.LAYERS): e.append(f'face part clips {sorted(pc)} != the layers clips {sorted(mod.LAYERS)}')
        for c in sorted(set(pc) & set(mod.LAYERS)):
            if sorted(pc[c]) != sorted(FACE_DIRS): e.append(f'face part {c}: dirs {sorted(pc[c])} != {FACE_DIRS}')
            n = J['anims'].get(c, {}).get('frames_n', 0)
            for d in pc[c]:
                if 0 not in (part.get('exact') or {}).get(c, {}).get(d, []): e.append(f'face part {c} {d}: frame 0 must be exact')
                for s in ('eyes', 'mouth'):
                    if len(pc[c][d].get(s, [])) != n: e.append(f'face part {c} {d}: {s} positions != {n} frames')
    e += seat_errors(st, C)
    return e


def m_body_below(st):
    """an ink-rimmed fur blob hanging 3 px under the legs (centred), on edge_dangle S frame 5."""
    f = st.frames[('edge_dangle', 'S')][5]; ax, ay = st.J['anims']['edge_dangle']['anchor']
    op = f[f[..., 3] > 0][:, :3]; cols, n = np.unique(op, axis=0, return_counts=True)
    fur = cols[n.argmax()]; y0 = int(np.nonzero(f[..., 3].any(1))[0].max()) + 3
    f[y0:y0 + 6, ax - 3:ax + 3, :3] = K.INK; f[y0:y0 + 6, ax - 3:ax + 3, 3] = 255
    f[y0 + 1:y0 + 5, ax - 2:ax + 2, :3] = fur
    return 'not a leg'


def m_seat_sunk(st):
    """the whole seated bear 3 px lower (its seat below the edge line), on edge_dangle E frame 0."""
    fs = st.frames[('edge_dangle', 'E')]; fs[0] = np.roll(fs[0], 3, 0); return 'not a leg'


def m_chain(st):
    fs = st.frames[('edge_peer', 'SE')]; fs[-1] = np.roll(fs[-1], 1, 1); return 'chain edge_peer[-1]'


MUTATIONS = [('a body blob below the edge line on a seated frame', m_body_below),
             ('the seat sunk 3 px below the edge line', m_seat_sunk),
             ('a base-pose join broken (edge_peer last frame shifted 1 px)', m_chain)]

if __name__ == '__main__':
    K.verify_main('w', extra, MUTATIONS)
