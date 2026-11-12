"""Verify Stage lean (out/bear3d/set_lean/, stages/lean.json): the stage kit's set checks (stagekit.verify_main: palette,
alpha 0/255, 1 px ink outline, counts, loop seams, joins, hand-offs = props + idle#0, props sprites, face part) plus:
  - every frame shows the toy block exactly where the props-only frame has it, except where the bear covers it
    (a frame pixel that is neither the props frame's nor transparent-over-props must be a bear pixel: the block
    never moves, so a block pixel that differs from the props frame is a render error);
  - the bear never leaves the canvas (a 1 px transparent margin on every side).

    python3 verify_lean.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_lean.py --self-test       exit 1 = every mutation (5 standard + 2 lean) was caught
"""
import os, sys
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K


def block_mask(pf):
    return pf[..., 3] > 0


def extra(st):
    e = []
    if getattr(st, 'wood_cols', None) is None: st.wood_cols = wood_colours(st)
    wood = st.wood_cols
    for (c, o), fs in sorted(st.frames.items()):
        s = st.J['anims'][c]['set']; pf = st.props_frame.get((s, o))
        if pf is None or fs[0].shape != pf.shape: continue
        bm = block_mask(pf)
        for i, f in enumerate(fs):
            # where the block is drawn and the frame is NOT the props frame, the bear must be in front: the bear's
            # pixels never use the block's wood/rose colours, so a block colour there that differs = a moved block
            d = bm & np.any(f != pf, -1)
            bad = d & np.isin(f[..., :3].astype(np.int32) @ np.array([65536, 256, 1]), wood) & (f[..., 3] > 0)
            if bad.any(): e.append(f'{c} {o} f{i}: {int(bad.sum())} block pixels differ from the props-only frame (block moved)')
            gone = bm & (f[..., 3] == 0)   # nothing (bear or block) drawn where the block is: the block moved or vanished
            if gone.any(): e.append(f'{c} {o} f{i}: {int(gone.sum())} block pixels are empty in the frame (block moved)')
            edge = np.concatenate([f[0, :, 3], f[-1, :, 3], f[:, 0, 3], f[:, -1, 3]])
            if edge.any(): e.append(f'{c} {o} f{i}: art touches the canvas edge')
    return e


def m_block(s):
    """shift the block pixels of one lean frame by 1 px (a moved block)."""
    fs = s.frames[('lean', 'A')]; pf = s.props_frame[('lean', 'A')]; f = fs[5]
    bm = block_mask(pf) & np.all(f == pf, -1)
    sh = np.roll(np.where(bm[..., None], pf, 0), 1, 0); m = np.roll(bm, 1, 0) & ~bm & (f[..., 3] == 0)
    m2 = np.roll(bm, 1, 0) & bm
    f[m2] = sh[m2]; f[m] = sh[m]
    return 'block pixels differ'


def m_erase(s):
    """erase the visible block pixels of one lean frame (the block vanished; its outline goes too)."""
    fs = s.frames[('lean', 'B')]; pf = s.props_frame[('lean', 'B')]; f = fs[10]
    f[block_mask(pf) & np.all(f == pf, -1)] = 0
    return 'are empty'


def m_edge(s):
    f = s.frames[('lean_in', 'B')][3]; f[0, f.shape[1] // 2] = (*K.INK, 255); return 'canvas edge'


def wood_colours(st):
    R = K.ramps(st.sid); cols = set()
    for p in st.mod.PARTS.values():
        for i in p['idx']: cols.add(R[p['ramp']][i])
    bear = set()                    # every colour the Stage A bear uses: a block colour the bear also has is no marker
    import glob
    for f in glob.glob(os.path.join(K.REAL_OUT, 'anim', 'idle_*.png')) + glob.glob(os.path.join(K.REAL_OUT, 'anim', 'walk_*.png')):
        a = K.load(f); bear |= {tuple(int(v) for v in c) for c in np.unique(a[a[..., 3] > 0][:, :3], axis=0)}
    return np.array(sorted(r * 65536 + g * 256 + b for (r, g, b) in cols - bear), np.int32)


if __name__ == '__main__':
    K.verify_main('lean', extra=extra, mutations=[('the block moved 1 px in one frame', m_block),
                                                  ('the block erased in one frame', m_erase),
                                                  ('art on the canvas edge', m_edge)])
