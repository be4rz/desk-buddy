"""Verify stage m (gaits): python3 verify_m.py [--dir DIR] [--self-test]   (exit 0 pass / 1 findings;
--self-test exit 1 = every mutation caught, 2 = one slipped through).

The kit's standard checks (stagekit.verify_main: pixels, 8 dirs, counts, idle#0 hand-offs, CHAINS stop#0 = loop#0,
loop seams, root_px = the projection of root_track in both forms) plus stage m's own:
  - foot slide (feet contact frames): every step where a foot is planted on both frames (clips_m poses, rebuilt
    from the anim JSON's ppu) - inside each clip, across each loop seam, start -> loop and loop -> stop - must keep
    that foot on the floor: its move inside the sprite + the RENDERED root_track step <= 0.5 px;
  - travel: tiptoe / skip / shuffle move forward over the loop, back_waddle backwards (opposite to the facing),
    tx stays 0 (straight lines);
  - Stage A frame 84x123, anchor [42, 107], face none, no face part.
Stage mutations: the tiptoe loop's root_track shifted 2 px after a planted step (both root_px forms recomputed, so
only the slide check can see it), and back_waddle's track flipped to move forward.
"""
import math
import stagekit as K
import clips_m as M

FRAME, ANCHOR = [84, 123], [42, 107]


def _vp(ppu):
    return 2.0 / (K.RIGHT[0] * ppu), 1.0 / (K.UP[2] * ppu)      # V (2 px on an iso axis), PXZ (as rig_bear.py)


def extra(st):
    e = []; J = st.J; ppu = J['ppu']; V, PXZ = _vp(ppu)
    C = M.build(V, PXZ)
    tr = {}
    for c, a in J['anims'].items():
        if [a['frame_w'], a['frame_h']] != FRAME or list(a['anchor']) != ANCHOR:
            e.append(f'{c}: frame {a["frame_w"]}x{a["frame_h"]} anchor {a["anchor"]} != Stage A {FRAME} {ANCHOR}')
        if a.get('face') not in (None, 'none'): e.append(f'{c}: face {a.get("face")} (gaits have no face)')
        t = a.get('root_track') or []
        if c not in C: e.append(f'{c}: not a stage m clip (clips_m.ORDER)'); continue
        if len(t) != C[c]['frames'] + 1:
            e.append(f'{c}: root_track has {len(t)} entries, clips_m has {C[c]["frames"]} frames + 1'); continue
        if any(abs(p[0]) > 1e-9 for p in t): e.append(f'{c}: root_track tx != 0 (gaits walk straight)')
        tr[c] = [p[1] for p in t]
        e += [f'foot slide: {x}' for x in M.slide_errors(c, C[c], V, tr=tr[c])]
    if st.part is not None: e.append(f'face_{st.sid}.json present (gaits have no face)')
    for g, (s0, lp, s1) in M.GAITS.items():
        if not all(c in tr for c in (s0, lp, s1)): continue
        sign = 1 if M.SPEED[g] > 0 else -1
        travel = tr[lp][-1] - tr[lp][0]
        if travel * sign <= 0: e.append(f'{lp}: moves {"backwards" if travel < 0 else "forwards"} over the loop '
                                        f'({travel:+.3f}); {g} must move {"forward" if sign > 0 else "opposite to the facing"}')
        for ca, cb in ((s0, lp), (lp, s1)):           # the hand-over steps between clips keep planted feet too
            qa, qb = C[ca]['poses'][-1], C[cb]['poses'][0]
            dr = tr[ca][-1] - tr[ca][-2]
            for ft in ('L', 'R'):
                if abs(qa['h' + ft]) < 1e-9 and abs(qb['h' + ft]) < 1e-9:
                    w = qb['f' + ft] - qa['f' + ft] + dr
                    if abs(w) / V * 2 > 0.5: e.append(f'foot slide: {ca} -> {cb} foot {ft} slides {abs(w) / V * 2:.2f} px')
    return e


def _retrack(a, ppu, track):
    a['root_track'] = track
    for d in a['files']:
        steps = K.track_px(ppu, K.YAW[d], track)
        a['root_px'][d] = steps
        for i, f in enumerate(a['frames']): f['root_px'][d] = steps[i]


def m_slide(st):
    a = st.J['anims']['tiptoe']; ppu = st.J['ppu']; V, PXZ = _vp(ppu)
    i = next(i for i, ft, w in M.slide_steps(M.build(V, PXZ)['tiptoe']) if i < a['frames_n'] - 1)
    t = [list(p) for p in a['root_track']]
    for k in range(i + 1, len(t)): t[k][1] += V          # +2 px along an iso axis from frame i+1 on
    _retrack(a, ppu, t); return 'foot slide'


def m_backwards(st):
    a = st.J['anims']['back_waddle']
    _retrack(a, st.J['ppu'], [[p[0], -p[1]] for p in a['root_track']]); return 'opposite to the facing'


MUTATIONS = [('tiptoe root_px sum off by 2 px over the loop (feet slide)', m_slide),
             ('back_waddle moving forwards', m_backwards)]

if __name__ == '__main__':
    K.verify_main('m', extra, MUTATIONS)
