"""Verify Stage t (tumbles; out/bear3d/anim_t/, stages/t.json): the stage kit's clip checks (stagekit.verify_main:
palette, alpha 0/255, 1 px ink outline, counts, dirs, per-clip frame sizes, idle0 and '<clip>#<frame>' hand-offs, e.g.
pounce / slide_stop start = run_stop#0 anchor-aligned, root_px = one track) plus:
  - every clip has face "none" and the stage writes no face part;
  - each clip's frame size and anchor are clips_t.CLIPS' (hop, spin 84x123; pounce, slide_stop 132x123; roll, trip
    132x135 (lead, q-103334); anchor [W/2, 107] everywhere);
  - root motion: hop / roll / pounce / slide_stop move only forward (tx = 0, ty_px never decreasing, ending > 0);
    spin / trip stay on the spot;
  - the art keeps >= 2 px from every canvas edge on every clip (EDGE; render_anim.py's --canvas rule).

    python3 verify_t.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_t.py --self-test       exit 1 = every mutation (6 standard + the stage's own) was caught
"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K

FORWARD = ('hop', 'roll', 'pounce', 'slide_stop')
IN_PLACE = ('spin', 'trip')
EDGE = 2


def extra(st):
    e = []; J = st.J; C = st.mod.CLIPS
    if st.part is not None: e.append('face_t.json exists: stage t has no face layers (face "none")')
    for c, a in J['anims'].items():
        cd = C.get(c, {})
        if a.get('face') not in (None, 'none'): e.append(f'{c}: face {a.get("face")!r} != "none"')
        want = (list(cd.get('frame', [])), list(cd.get('anchor', [])))
        if want[0] and ([a['frame_w'], a['frame_h']], list(a['anchor'])) != want:
            e.append(f'{c}: frame {a["frame_w"]}x{a["frame_h"]} anchor {a["anchor"]} != storyboard {want[0]} {want[1]}')
        tr = a.get('root_track') or []
        if tr and c in FORWARD:
            if any(abs(p[0]) > 1e-9 for p in tr) or any(q[1] < p[1] - 1e-9 for p, q in zip(tr, tr[1:])) or tr[-1][1] <= 0:
                e.append(f'{c}: root track must move only forward (tx 0, ty_px rising, ending > 0)')
        if tr and c in IN_PLACE and any(abs(v) > 1e-9 for p in tr for v in p):
            e.append(f'{c}: root track must stay on the spot')
        for d in a['files']:
            fs = st.frames.get((c, d))
            if not fs: continue
            for i, f in enumerate(fs):
                al = f[..., 3]
                if al[:EDGE].any() or al[-EDGE:].any() or al[:, :EDGE].any() or al[:, -EDGE:].any():
                    e.append(f'{c} {d} f{i}: art within {EDGE} px of the canvas edge')
    return e


def m_backward(s):
    """the hop's root track runs backwards (a hop that slides the bear back)."""
    a = s.J['anims']['hop']; a['root_track'] = [[p[0], -p[1]] for p in a['root_track']]
    return 'only forward'


def m_edge(s):
    k = sorted(s.frames)[0]; f = s.frames[k][3]; f[0, f.shape[1] // 2] = (*K.INK, 255)
    return 'of the canvas edge'


def m_margin(s):
    """one ink pixel 1 px in from the bottom edge (inside the canvas, but no room for the outline)."""
    k = sorted(s.frames)[-1]; f = s.frames[k][0]; f[-2, f.shape[1] // 2] = (*K.INK, 255)
    return 'of the canvas edge'


def m_face(s):
    a = next(iter(s.J['anims'].values())); a['face'] = 'layers'
    return '!= "none"'


MUTATIONS = [('the hop root track runs backwards', m_backward), ('art on the canvas edge', m_edge),
             ('art 1 px from the bottom edge', m_margin),
             ('a clip claims face layers', m_face)]

if __name__ == '__main__':
    K.verify_main('t', extra=extra, mutations=MUTATIONS)
