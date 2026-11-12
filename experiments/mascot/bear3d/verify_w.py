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
  - seated: on every clips_w.SEATED frame, each opaque pixel below the edge line (rows > anchor y) lies on a stub
    foot: inside the screen hull of that foot's posed box (its rest bounds in bear.blend, moved by fL / hL and the
    yaw, projected with the kit camera; 1.5 px slack for the ink outline and rounding). Only the feet may hang.
Stage mutations: an ink-rimmed fur blob below the edge line between the feet of a seated frame; a base-pose join
broken (edge_peer's last frame shifted 1 px).
"""
import math
import numpy as np
import stagekit as K

FRAME, ANCHOR = [84, 139], [42, 107]
FACE_DIRS = ['S', 'SE', 'E', 'W', 'SW']
# the stub feet's rest bounds (bear.blend, world units, measured once: x, y, z) - bear-local, the bear faces -y
FOOT = {'L': ((0.09, 0.53), (-0.44, 0.14), (-0.01, 0.27)), 'R': ((-0.53, -0.09), (-0.44, 0.14), (-0.01, 0.27))}
SLACK = 1.5


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


def foot_hulls(ppu, yaw, q):
    """screen hulls (px from the anchor, y down) of the two stub feet in pose q at yaw."""
    t = math.radians(yaw); c, s = math.cos(t), math.sin(t)
    out = []
    for side, (xs, ys, zs) in FOOT.items():
        pts = []
        for x in xs:
            for y in ys:
                for z in zs:
                    y2 = y - q['f' + side]; z2 = z + q['h' + side]
                    pts.append(tuple(K.px_of(ppu, x * c - y2 * s, x * s + y2 * c, z2)))
        out.append(_hull(pts))
    return out


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
                hs = foot_hulls(ppu, K.YAW[d], poses[i])
                bad = [(x, y) for y, x in zip(ys, xs) if not any(_near(h, x + 0.5 - ax, y + 0.5 - ay) for h in hs)]
                if bad:
                    e.append(f'{c} {d} f{i}: {len(bad)} opaque px below the edge line that are not a stub foot '
                             f'(e.g. x{bad[0][0]} y{bad[0][1]}; only the feet may hang)')
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
    """an ink-rimmed fur blob 4 px below the edge line, centred between the feet, on edge_dangle S frame 5."""
    f = st.frames[('edge_dangle', 'S')][5]; ax, ay = st.J['anims']['edge_dangle']['anchor']
    op = f[f[..., 3] > 0][:, :3]; cols, n = np.unique(op, axis=0, return_counts=True)
    fur = cols[n.argmax()]; y0 = ay + 14
    f[y0:y0 + 6, ax - 3:ax + 3, :3] = K.INK; f[y0:y0 + 6, ax - 3:ax + 3, 3] = 255
    f[y0 + 1:y0 + 5, ax - 2:ax + 2, :3] = fur
    return 'not a stub foot'


def m_chain(st):
    fs = st.frames[('edge_peer', 'SE')]; fs[-1] = np.roll(fs[-1], 1, 1); return 'chain edge_peer[-1]'


MUTATIONS = [('a body blob below the edge line on a seated frame', m_body_below),
             ('a base-pose join broken (edge_peer last frame shifted 1 px)', m_chain)]

if __name__ == '__main__':
    K.verify_main('w', extra, MUTATIONS)
