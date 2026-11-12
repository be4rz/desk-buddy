"""Verify Stage play (out/bear3d/set_play/, stages/play.json): the stage kit's set checks (stagekit.verify_main: palette,
alpha 0/255, 1 px ink outline, counts, loop seams, joins, hand-offs = props + idle#0, prop sprites, face part) plus:
  - every toy is home on every hand-off frame (the enter clip's first frame, the exit clip's last frame): each props-only
    pixel is either unchanged or covered by the bear, never empty and never another toy colour (set_play.prop_state
    places the toys after render_set poses the rig; this proves the placed home state is the props frame);
  - the blocks loop ends with the blocks back home (its last frame's toy pixels = the props frame's, where uncovered);
  - art never touches the canvas edge (a 1 px transparent margin on every side).

    python3 verify_play.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_play.py --self-test       exit 1 = every mutation (5 standard + 3 play) was caught
"""
import os, sys, glob
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K


def toy_colours(st):
    """packed RGB of every toy colour the Stage A bear never uses (a marker of a toy pixel)."""
    R = K.ramps(st.sid); cols = set()
    for p in st.mod.PARTS.values():
        for i in p['idx']: cols.add(tuple(R[p['ramp']][i]))
    bear = set()
    for f in glob.glob(os.path.join(K.REAL_OUT, 'anim', 'idle_*.png')) + glob.glob(os.path.join(K.REAL_OUT, 'anim', 'walk_*.png')):
        a = K.load(f); bear |= {tuple(int(v) for v in c) for c in np.unique(a[a[..., 3] > 0][:, :3], axis=0)}
    return np.array(sorted(r * 65536 + g * 256 + b for (r, g, b) in cols - bear), np.int32)


def home_errors(f, pf, toy, label):
    e = []
    pm = pf[..., 3] > 0
    gone = pm & (f[..., 3] == 0)
    if gone.any(): e.append(f'{label}: {int(gone.sum())} toy pixels of the props frame are empty (toy not home)')
    packed = f[..., :3].astype(np.int32) @ np.array([65536, 256, 1])
    moved = np.any(f != pf, -1) & (f[..., 3] > 0) & np.isin(packed, toy)
    if moved.any(): e.append(f'{label}: {int(moved.sum())} toy-coloured pixels differ from the props frame (toy not home)')
    return e


def checks_frames(st):
    """[(clip, ori, frame index)] whose toys must be home: hand-offs, + the blocks loop's last frame."""
    out = []
    for s, sd in st.mod.SETS.items():
        for c, at in sd['handoff'].items():
            for o in K.ORIS: out.append((s, c, o, 0 if at == 'start' else -1))
    for o in K.ORIS: out.append(('blocks', 'blocks_build', o, -1))
    return out


def extra(st):
    e = []
    if getattr(st, 'toy_cols', None) is None: st.toy_cols = toy_colours(st)
    for s, c, o, i in checks_frames(st):
        fs = st.frames.get((c, o)); pf = st.props_frame.get((s, o))
        if fs is None or pf is None or fs[i].shape != pf.shape: continue
        e += home_errors(fs[i], pf, st.toy_cols, f'{c} {o} f{i % len(fs)}')
    for (c, o), fs in sorted(st.frames.items()):
        for i, f in enumerate(fs):
            edge = np.concatenate([f[0, :, 3], f[-1, :, 3], f[:, 0, 3], f[:, -1, 3]])
            if edge.any(): e.append(f'{c} {o} f{i}: art touches the canvas edge')
    return e


def _shift_toy(s, c, o, i, set_id):
    """move the uncovered toy pixels of one frame by 1 px to the right (a toy off home on that frame)."""
    f = s.frames[(c, o)][i]; pf = s.props_frame[(set_id, o)]
    tm = (pf[..., 3] > 0) & np.all(f == pf, -1)
    src = np.where(tm[..., None], f, 0)
    f[tm] = 0
    sh = np.roll(src, 1, 1); m = np.roll(tm, 1, 1)
    f[m] = sh[m]


def m_handoff_toy(s):
    _shift_toy(s, 'ball_get', 'A', 0, 'ball'); return 'not home'


def m_blocks_seam(s):
    _shift_toy(s, 'blocks_build', 'B', -1, 'blocks'); return 'not home'


def m_edge(s):
    f = s.frames[('yoyo', 'B')][3]; f[f.shape[0] // 2, 0] = (*K.INK, 255); return 'canvas edge'


if __name__ == '__main__':
    K.verify_main('play', extra=extra, mutations=[('a toy moved 1 px on a hand-off frame', m_handoff_toy),
                                                  ('the blocks not home at the loop seam', m_blocks_seam),
                                                  ('art on the canvas edge', m_edge)])
