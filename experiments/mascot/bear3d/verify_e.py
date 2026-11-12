"""Verify Stage e (gestures-a; out/bear3d/anim_e/, stages/e.json, clips_e.py): the stage kit's clip checks
(stagekit.verify_main: palette, alpha 0/255, 1 px ink outline, frame counts, dirs, frame size, hand-offs = idle#0
anchor-aligned, root_px = one track = the projection of root_track) plus:
  - every clip has the storyboard's face handling (clips_e.FACE), and the face part face_e.json covers exactly the
    layers clips (wave, remind) in every face dir (S SE E W SW), each with frame 0 and the last frame exact;
  - verify_face.py passes on the layers frames as they are in memory (verify_f.face_check: a temp copy checked with
    `verify_face.py --part`), so a mitten across the face of a layers frame fails here as it does there;
  - root motion: only dodge moves the panel; its root track is purely sideways (ty_px 0, tx never decreasing) and ends
    displaced by clips_e.DODGE_STEPS * V to the bear's left, while its last frame is idle#0 in the frame (the kit's
    idle0 hand-off): so the bear ends on idle#0 at the displaced anchor, the displacement carried by root_px only;
  - the art never touches the canvas edge.

    python3 verify_e.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_e.py --self-test       exit 1 = every mutation (7 standard + 5 stage e) was caught
"""
import os, sys
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K
from verify_f import face_check

FACE_DIRS = ['S', 'SE', 'E', 'W', 'SW']


def extra(st):
    e = []; mod = st.mod; J = st.J
    layers = set(mod.LAYERS)
    for c, a in J['anims'].items():
        want = mod.FACE.get(c)
        if a.get('face') != want: e.append(f'{c}: face {a.get("face")!r} != storyboard {want!r}')
        tr = a.get('root_track') or []
        if c in mod.MOVES:
            dist = mod.DODGE_TX
            if any(abs(p[1]) > 1e-9 for p in tr) or any(q[0] < p[0] - 1e-9 for p, q in zip(tr, tr[1:])) \
                    or not tr or abs(tr[-1][0] - dist) > 1e-4:
                e.append(f'{c}: root track must step only sideways to the bear\'s left and end displaced by '
                         f'{mod.DODGE_STEPS} V (tx {dist:.4f}, ty_px 0)')
            for d, steps in (a.get('root_px') or {}).items():
                if abs(sum(x for x, _ in steps)) + abs(sum(y for _, y in steps)) < 1: e.append(f'{c} {d}: root_px does not displace the panel')
        else:
            for d, steps in (a.get('root_px') or {}).items():
                if any(abs(x) > 1e-9 or abs(y) > 1e-9 for x, y in steps): e.append(f'{c} {d}: root_px moves the panel (only dodge may)')
        for d in a['files']:
            for i, f in enumerate(st.frames.get((c, d), [])):
                edge = np.concatenate([f[0, :, 3], f[-1, :, 3], f[:, 0, 3], f[:, -1, 3]])
                if edge.any(): e.append(f'{c} {d} f{i}: art touches the canvas edge')
    part = st.part
    if part is None:
        e.append(f'face_{st.sid}.json missing (the layers clips {sorted(layers)} need a face part)')
    else:
        pc = part.get('clips') or {}
        if set(pc) != layers: e.append(f'face part clips {sorted(pc)} != the layers clips {sorted(layers)}')
        for c in sorted(set(pc) & layers):
            if sorted(pc[c]) != sorted(FACE_DIRS): e.append(f'face part {c}: dirs {sorted(pc[c])} != {FACE_DIRS}')
            n = J['anims'].get(c, {}).get('frames_n', 0)
            for d in pc[c]:
                ex = (part.get('exact') or {}).get(c, {}).get(d, [])
                if 0 not in ex or n - 1 not in ex: e.append(f'face part {c} {d}: hand-off frames 0 and {n - 1} must be exact')
                for s in ('eyes', 'mouth'):
                    if len(pc[c][d].get(s, [])) != n: e.append(f'face part {c} {d}: {s} positions != {n} frames')
        e += face_check(st)
    return e


def m_paw_on_face(s):
    """a white mitten (fur, ink rim) over the eyes of a wave S frame: the outline stays closed, so only the face check
    (verify_face.py on the in-memory frames) can catch it."""
    c, d = 'wave', 'S'; i = 9; f = s.frames[(c, d)][i]
    ex, ey = s.part['clips'][c][d]['eyes'][i]
    op = f[f[..., 3] > 0][:, :3]; cols, n = np.unique(op, axis=0, return_counts=True)
    fur = cols[n.argmax()]
    ink = f[ey + 4, :, :3][f[ey + 4, :, 3] > 0][0]
    f[ey - 1:ey + 7, ex - 1:ex + 13, :3] = ink; f[ey:ey + 6, ex:ex + 12, :3] = fur
    return 'verify_face.py: '


def m_face_flag(s):
    s.J['anims']['nod']['face'] = 'layers'; return 'nod: face'


def m_root(s):
    """a gesture that is not dodge moves the panel."""
    a = s.J['anims']['jump']
    for d in a['files']:
        a['root_px'][d][3] = [1.0, 0.0]; a['frames'][3]['root_px'][d] = [1.0, 0.0]
    return 'only dodge may'


def m_dodge_still(s):
    """dodge loses its root motion (the side-step would end where it started)."""
    a = s.J['anims']['dodge']
    a['root_track'] = [[0.0, 0.0] for _ in a['root_track']]
    for d in a['files']:
        a['root_px'][d] = [[0.0, 0.0] for _ in a['root_px'][d]]
        for f in a['frames']: f['root_px'][d] = [0.0, 0.0]
    return 'end displaced'


def m_dodge_baked(s):
    """the stage-specific check: dodge's last frame must be idle#0 at the displaced anchor, i.e. idle#0 in the frame
    with the move carried by root_px. Bake the side-step into the last frame instead (shift it by the rounded total
    root_px) and the idle0 hand-off must fail."""
    a = s.J['anims']['dodge']; d = 'SE'
    dx = int(round(sum(x for x, _ in a['root_px'][d]))); dy = int(round(sum(y for _, y in a['root_px'][d])))
    fs = s.frames[('dodge', d)]; fs[-1] = np.roll(np.roll(fs[-1], dx, 1), dy, 0)
    return 'dodge SE: hand-off (end)'


if __name__ == '__main__':
    K.verify_main('e', extra=extra, mutations=[('a mitten moved onto the face of a layers frame', m_paw_on_face),
                                               ('a baked clip marked layers', m_face_flag),
                                               ('a gesture other than dodge moves the panel', m_root),
                                               ('dodge ends where it started (no root motion)', m_dodge_still),
                                               ('dodge bakes its side-step into the last frame', m_dodge_baked)])
