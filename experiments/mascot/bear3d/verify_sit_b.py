"""Verify Stage sit_b (out/bear3d/set_sit_b/, stages/sit_b.json): the stage kit's set checks (stagekit.verify_main:
palette, alpha 0/255, 1 px ink outline, counts, loop seams, joins, hand-offs = home props + idle#0, props sprites, face
part) plus, because set_sit_b.prop_state places moving props itself (no kit hook, see the module header):
  - each set's static prop (notebook, laptop, phone_stand, bucket) never moves: where the props-only frame shows one of
    its own colours (a colour no bear frame, no moving prop of that set uses), every frame must show that same pixel
    or something else in front of it, never a different static colour and never nothing;
  - umbrella_rain: rain drops in every frame; no drop colour on a hand-off frame;
  - no art on the canvas edge.

    python3 verify_sit_b.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_sit_b.py --self-test       exit 1 = every mutation (5 standard + 4 sit_b) was caught
"""
import os, sys, glob
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K

RGB = np.array([65536, 256, 1], np.int64)


def code(a): return a[..., :3].astype(np.int64) @ RGB


def _bear_cols():
    cols = set()
    for f in glob.glob(os.path.join(K.REAL_OUT, 'anim', 'idle_*.png')) + glob.glob(os.path.join(K.REAL_OUT, 'anim', 'walk_*.png')):
        a = K.load(f); cols |= set(int(v) for v in np.unique(code(a)[a[..., 3] > 0]))
    return cols


def _cols(st, parts):
    R = K.ramps(st.sid); out = set()
    for n in parts:
        p = st.mod.PARTS[n]
        for i in p['idx']:
            r, g, b = R[p['ramp']][i]; out.add(r * 65536 + g * 256 + b)
    return out


def tables(st):
    if getattr(st, 'tab', None) is None:
        mod = st.mod; bear = _bear_cols(); tab = {}
        for s in mod.SETS:
            mov = set().union(*[_cols(st, mod.LAYER_PARTS[l]) for l in mod.MOVING[s]]) if mod.MOVING[s] else set()
            stat = _cols(st, mod.LAYER_PARTS[mod.STATIC[s]]) - bear - mov
            tab[s] = np.array(sorted(stat), np.int64)
        st.tab = tab; st.drop = np.array(sorted(_cols(st, ['drop']) - bear - _cols(st, ['bucket', 'bucket_band', 'um_canopy', 'um_shaft'])), np.int64)
    return st.tab


def extra(st):
    e = []; tab = tables(st)
    for (c, o), fs in sorted(st.frames.items()):
        a = st.J['anims'][c]; s = a['set']; pf = st.props_frame.get((s, o))
        if pf is None or fs[0].shape != pf.shape: continue
        sm = (pf[..., 3] > 0) & np.isin(code(pf), tab[s])
        if not sm.any(): e.append(f'{c} {o}: no static {st.mod.STATIC[s]} colour in the props frame'); continue
        hf = (a.get('handoff') or {}).get(o, {}).get('frame')
        for i, f in enumerate(fs):
            fc = code(f)
            bad = sm & np.any(f != pf, -1) & np.isin(fc, tab[s]) & (f[..., 3] > 0)
            if bad.any(): e.append(f'{c} {o} f{i}: {int(bad.sum())} static prop pixels differ from the props frame (prop moved)')
            gone = sm & (f[..., 3] == 0)
            if gone.any(): e.append(f'{c} {o} f{i}: {int(gone.sum())} static prop pixels are empty (prop moved)')
            edge = np.concatenate([f[0, :, 3], f[-1, :, 3], f[:, 0, 3], f[:, -1, 3]])
            if edge.any(): e.append(f'{c} {o} f{i}: art touches the canvas edge')
            if s == 'umbrella':
                nd = int((np.isin(fc, st.drop) & (f[..., 3] > 0)).sum())
                if c == 'umbrella_rain' and nd == 0: e.append(f'{c} {o} f{i}: no rain drop')
                if i == hf and nd: e.append(f'{c} {o} f{i}: {nd} rain drop pixels on a hand-off frame')
    return e


def _static_mask(s, sid, o):
    pf = s.props_frame[(sid, o)]; return pf, (pf[..., 3] > 0) & np.isin(code(pf), tables(s)[sid])


def m_static(s):
    """shift the visible notebook pixels of one doodle frame by 1 px (a moved static prop)."""
    pf, sm = _static_mask(s, 'notebook', 'A'); f = s.frames[('note_doodle', 'A')][7]
    vis = sm & np.all(f == pf, -1); sh = np.roll(np.where(vis[..., None], pf, 0), 1, 1); m = np.roll(vis, 1, 1)
    f[m] = sh[m]; return 'static prop pixels'


def m_moved(s):
    """lead condition (b): a moving prop (the crayon at home) shifted 1 px on a hand-off frame."""
    f = s.frames[('note_sit', 'A')][0]; R = K.ramps(s.sid)
    cr = np.array([r * 65536 + g * 256 + b for r, g, b in (R['pip'][i] for i in s.mod.PARTS['crayon']['idx'])], np.int64)
    m = np.isin(code(f), cr) & (f[..., 3] > 0); assert m.any(), 'no crayon on note_sit A f0'
    sh = np.roll(np.where(m[..., None], f, 0), 1, 0); mm = np.roll(m, 1, 0); f[mm] = sh[mm]; return 'hand-off'


def m_drop(s):
    """a rain drop pixel painted on umbrella_close's last (hand-off) frame."""
    tables(s); f = s.frames[('umbrella_close', 'B')][-1]; c = int(s.drop[0])
    ys, xs = np.nonzero(f[..., 3] > 0); y, x = int(ys[len(ys) // 3]), int(xs[len(xs) // 3])
    f[y, x, :3] = (c >> 16 & 255, c >> 8 & 255, c & 255); return 'rain drop pixels on a hand-off'


def m_edge(s):
    f = s.frames[('laptop_type', 'B')][3]; f[f.shape[0] // 2, 0] = (*K.INK, 255); return 'canvas edge'


if __name__ == '__main__':
    K.verify_main('sit_b', extra=extra, mutations=[('the notebook moved 1 px in one doodle frame', m_static),
                                                   ('the crayon moved 1 px on a hand-off frame', m_moved),
                                                   ('a rain drop on a hand-off frame', m_drop),
                                                   ('art on the canvas edge', m_edge)])
