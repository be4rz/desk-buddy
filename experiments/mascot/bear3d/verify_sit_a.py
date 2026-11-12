"""Verify Stage sit_a (out/bear3d/set_sit_a/, stages/sit_a.json): the stage kit's set checks (stagekit.verify_main:
palette, alpha 0/255, 1 px ink outline, counts, loop seams, joins, hand-offs = props + idle#0, props sprites, face
part) plus:
  - the floor cushion never moves: where the props-only frame shows the cushion, a frame pixel in a cushion colour
    must be the props frame's own pixel, and the cushion is never left empty (the bear and the held props only ever
    cover it; no other art uses the cushion's sage colours);
  - the art never touches the canvas edge (a 1 px transparent margin on every side).

    python3 verify_sit_a.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_sit_a.py --self-test       exit 1 = every mutation (5 standard + 4 sit_a) was caught
"""
import os, sys, glob
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K


def cushion_colours(st):
    R = K.ramps(st.sid); P = st.mod.PARTS['cushion']
    cols = {R[P['ramp']][i] for i in P['idx']}
    other = set()                   # every colour the Stage A bear and the other props use: no marker
    for f in glob.glob(os.path.join(K.REAL_OUT, 'anim', 'idle_*.png')) + glob.glob(os.path.join(K.REAL_OUT, 'anim', 'walk_*.png')):
        a = K.load(f); other |= {tuple(int(v) for v in c) for c in np.unique(a[a[..., 3] > 0][:, :3], axis=0)}
    for n, p in st.mod.PARTS.items():
        if n != 'cushion': other |= {R[p['ramp']][i] for i in p['idx']}
    return np.array(sorted(r * 65536 + g * 256 + b for (r, g, b) in cols - other), np.int32)


def rgb(f): return f[..., :3].astype(np.int32) @ np.array([65536, 256, 1])


def extra(st):
    e = []
    if getattr(st, 'cush_cols', None) is None: st.cush_cols = cushion_colours(st)
    cc = st.cush_cols
    for (c, o), fs in sorted(st.frames.items()):
        s = st.J['anims'][c]['set']; pf = st.props_frame.get((s, o))
        if pf is None or fs[0].shape != pf.shape: continue
        cm = np.isin(rgb(pf), cc) & (pf[..., 3] > 0)           # the cushion's own (non-outline) pixels
        for i, f in enumerate(fs):
            bad = cm & np.any(f != pf, -1) & np.isin(rgb(f), cc) & (f[..., 3] > 0)
            if bad.any(): e.append(f'{c} {o} f{i}: {int(bad.sum())} cushion pixels differ from the props-only frame (cushion moved)')
            gone = cm & (f[..., 3] == 0)
            if gone.any(): e.append(f'{c} {o} f{i}: {int(gone.sum())} cushion pixels are empty in the frame (cushion moved)')
            edge = np.concatenate([f[0, :, 3], f[-1, :, 3], f[:, 0, 3], f[:, -1, 3]])
            if edge.any(): e.append(f'{c} {o} f{i}: art touches the canvas edge')
    return e


def m_props_pixel(s):
    """change one props-only pixel (not covered by the bear) of a hand-off frame to another palette colour."""
    for c, a in s.J['anims'].items():
        for o, h in (a.get('handoff') or {}).items():
            f = s.frames[(c, o)][h['frame']]; pf = s.props_frame[(a['set'], o)]
            spr, sa = K.idle0(h['facing']); so = s.J['sets'][a['set']][o]
            anc = (so['anchor'][0] + h['px'][0], so['anchor'][1] + h['px'][1])
            _, want = K.aligned_diff(f, anc, spr, sa, pf)
            bear = np.any(want != pf, -1)
            ys, xs = np.nonzero((pf[..., 3] > 0) & ~bear & np.all(f == pf, -1) & np.any(pf[..., :3] != np.array(K.INK, np.uint8), -1))
            y, x = int(ys[len(ys) // 2]), int(xs[len(xs) // 2])
            new = next(col for col in sorted(s.allowed) if col != tuple(int(v) for v in f[y, x, :3]) and col != K.INK)
            f[y, x, :3] = new
            return 'hand-off'
    raise AssertionError('no hand-off frame')


def m_cushion(s):
    """shift the visible cushion pixels of one loop frame down by 1 px (a moved cushion)."""
    fs = s.frames[('tea_sip', 'A')]; pf = s.props_frame[('tea', 'A')]; f = fs[5]
    if getattr(s, 'cush_cols', None) is None: s.cush_cols = cushion_colours(s)
    cm = np.isin(rgb(pf), s.cush_cols) & np.all(f == pf, -1)
    sh = np.roll(np.where(cm[..., None], pf, 0), 1, 0); m = np.roll(cm, 1, 0)
    f[m] = sh[m]; f[cm & ~m] = 0
    return 'cushion'


def m_prop_moved(s):
    """move the small prop (the mug, not the cushion) 1 px right in tea_sit's hand-off frame 0 (ori A)."""
    c = 'tea_sit'; h = s.J['anims'][c]['handoff']['A']; f = s.frames[(c, 'A')][h['frame']]
    pf = s.props_frame[('tea', 'A')]
    if getattr(s, 'cush_cols', None) is None: s.cush_cols = cushion_colours(s)
    spr, sa = K.idle0(h['facing']); so = s.J['sets']['tea']['A']
    _, want = K.aligned_diff(f, (so['anchor'][0] + h['px'][0], so['anchor'][1] + h['px'][1]), spr, sa, pf)
    bear = np.any(want != pf, -1)
    mug = (pf[..., 3] > 0) & ~bear & ~np.isin(rgb(pf), s.cush_cols) & np.all(f == pf, -1)
    ys, xs = np.nonzero(mug); x0 = int(np.median(xs))
    mug &= (np.abs(np.arange(f.shape[1]) - x0) <= 12)[None, :]       # the mug's own blob (its ink outline included)
    src = np.where(mug[..., None], pf, 0); sh = np.roll(src, 1, 1); m = np.roll(mug, 1, 1)
    f[mug & ~m] = 0; f[m] = sh[m]
    return 'hand-off'


def m_edge(s):
    f = s.frames[('knit_sit', 'B')][3]; f[0, f.shape[1] // 2] = (*K.INK, 255); return 'canvas edge'


if __name__ == '__main__':
    K.verify_main('sit_a', extra=extra, mutations=[('a props-only pixel changed in a hand-off frame', m_props_pixel),
                                                   ('the mug moved 1 px in a hand-off frame', m_prop_moved),
                                                   ('the cushion moved 1 px in one frame', m_cushion),
                                                   ('art on the canvas edge', m_edge)])
