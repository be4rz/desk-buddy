"""Verify Stage out (out/bear3d/set_out/, stages/out.json): the stage kit's set checks (stagekit.verify_main: palette,
alpha 0/255, 1 px ink outline, counts, loop seams, joins, hand-offs = props + idle#0, props sprites, face part) plus:
  - static props never move: a static prop colour (the pot, the ice patch, the bed's wood frame) never shows where the
    props-only frame is empty, and a static prop pixel of the props-only frame is never empty (shade flips next to a
    moving prop are allowed);
  - each set's carried prop is drawn in every frame of its loop clip (its signature colours never vanish; in the
    enter/exit clips the bear may stand in front of it at home);
  - art never touches the canvas edge (a 1 px transparent margin on every side).
Moving props are placed by set_out.prop_state inside render_set.py (see set_out.py); the kit's hand-off check is what
proves a hand-off frame holds every prop at home, and the self-test moves one there to show it is caught.

    python3 verify_out.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_out.py --self-test       exit 1 = every mutation (5 standard + 4 out) was caught
"""
import os, sys
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K

R = K.ramps('out')
def cols(*spec):
    """[(ramp, (indices))] -> packed rgb ints."""
    return np.array(sorted({(c[0] << 16) | (c[1] << 8) | c[2] for r, ix in spec for i in ix for c in [R[r][i]]}), np.int64)


STATIC = {'plant': cols(('terracotta', range(5)), ('wood', (0, 1, 2))),
          'fishing': cols(('ice', (2, 3, 4))),
          'bed_cap': cols(('wood', (2, 3, 4)))}
CARRIED = {'plant': cols(('fabric_quilt', (1, 2, 3))), 'sweep': cols(('straw', (1, 2, 3))),
           'snowman': cols(('snow', (2, 3, 4))), 'fishing': cols(('wood', (2, 3, 4))),
           'bed_cap': cols(('fabric_rose', (1, 2, 3)))}


def _pk(f):
    a = f.astype(np.int64); return (a[..., 0] << 16) | (a[..., 1] << 8) | a[..., 2]


def extra(st):
    e = []
    for (c, o), fs in sorted(st.frames.items()):
        s = st.J['anims'][c]['set']; pf = st.props_frame.get((s, o))
        if pf is None or fs[0].shape != pf.shape: continue
        sm = None
        if s in STATIC:
            sm = np.isin(_pk(pf), STATIC[s]) & (pf[..., 3] > 0)
        for i, f in enumerate(fs):
            pk = _pk(f); op = f[..., 3] > 0
            if sm is not None:
                bad = np.isin(pk, STATIC[s]) & op & (pf[..., 3] == 0)
                if bad.any(): e.append(f'{c} {o} f{i}: {int(bad.sum())} static prop colour pixels where the props-only frame is empty (a static prop moved)')
                gone = sm & ~op
                if gone.any(): e.append(f'{c} {o} f{i}: {int(gone.sum())} static prop pixels are empty in the frame (a static prop moved)')
            if s in CARRIED and st.J['anims'][c].get('loop') and not (np.isin(pk, CARRIED[s]) & op).any():
                e.append(f'{c} {o} f{i}: the carried prop is missing (no {s} signature colour)')
            edge = np.concatenate([f[0, :, 3], f[-1, :, 3], f[:, 0, 3], f[:, -1, 3]])
            if edge.any(): e.append(f'{c} {o} f{i}: art touches the canvas edge')
    return e


def _shift(f, m, dy=1, dx=0):
    """move the pixels under mask m by (dy, dx), leaving transparency behind."""
    src = np.where(m[..., None], f, 0); sh = np.roll(np.roll(src, dy, 0), dx, 1); ms = np.roll(np.roll(m, dy, 0), dx, 1)
    f[m] = 0; f[ms] = sh[ms]


def m_static(s):
    """shift the pot pixels of one plant_water frame by 1 px (a static prop moved)."""
    fs = s.frames[('plant_water', 'A')]; pf = s.props_frame[('plant', 'A')]; f = fs[5]
    _shift(f, np.isin(_pk(pf), STATIC['plant']) & np.all(f == pf, -1) & (pf[..., 3] > 0))
    return 'static prop'


def m_carried(s):
    """erase the watering can in one plant_water frame (its colours turn transparent)."""
    f = s.frames[('plant_water', 'B')][10]
    f[np.isin(_pk(f), CARRIED['plant']) & (f[..., 3] > 0)] = 0
    return 'carried prop is missing'


def m_edge(s):
    f = s.frames[('sweep', 'B')][3]; f[0, f.shape[1] // 2] = (*K.INK, 255); return 'canvas edge'


def m_handoff_prop(s):
    """move the watering can by 1 px on plant_get's hand-off frame (a prop not at home on a hand-off)."""
    f = s.frames[('plant_get', 'A')][0]; pf = s.props_frame[('plant', 'A')]
    can = np.isin(_pk(pf), CARRIED['plant']) & (pf[..., 3] > 0) & np.all(f == pf, -1)
    ys, xs = np.nonzero(can); y0, y1, x0, x1 = ys.min() - 1, ys.max() + 2, xs.min() - 1, xs.max() + 2
    box = np.zeros(can.shape, bool); box[y0:y1, x0:x1] = True
    _shift(f, box & np.all(f == pf, -1) & (pf[..., 3] > 0), 0, 1)
    return 'hand-off frame 0 differs'


if __name__ == '__main__':
    K.verify_main('out', extra=extra, mutations=[('a static prop moved 1 px', m_static),
                                                  ('the carried can erased', m_carried),
                                                  ('art on the canvas edge', m_edge),
                                                  ('a prop moved on a hand-off frame', m_handoff_prop)])
