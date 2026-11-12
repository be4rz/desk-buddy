"""Stage p verifier (postures): python3 verify_p.py [--dir DIR] [--self-test]   (exit 0 pass / 1 findings;
--self-test exit 1 = every mutation caught, 2 = one missed).

The kit's standard checks (stagekit.verify_main: pixel rules, counts, dirs, idle#0 hand-offs, loop seams, root_px,
CHAINS) plus, for this stage:
  - every clip has its storyboard frame (132x123 anchor [66,107]; squat 84x123 anchor [42,107]) in the JSON and the strips;
  - every posture family is complete: down[last] = loop[0] = up[0] in every dir (sit_sway shares sit_floor's down / up),
    the down clip starts and the up clip ends on idle#0 (hand-off field set), the loop has no hand-off;
  - no root motion (postures stay on the spot);
  - art never touches the canvas edge (1 px clear border), so nothing is clipped;
  - face: sit_floor is 'layers' with a face part entry for every dir, every other clip is not in the face part.
Stage mutation: a loop strip that starts one frame late (its seam broken by one frame) must fail the chain check.
"""
import numpy as np
import stagekit


def extra(st):
    e = []; mod = st.mod; J = st.J
    for c, a in J['anims'].items():
        cd = mod.CLIPS.get(c)
        if not cd: e.append(f'{c}: not in clips_p.CLIPS'); continue
        if [a['frame_w'], a['frame_h']] != cd['frame'] or list(a['anchor']) != cd['anchor']:
            e.append(f'{c}: frame {a["frame_w"]}x{a["frame_h"]} anchor {a["anchor"]} != storyboard {cd["frame"]} {cd["anchor"]}')
        if (a.get('face') or 'none') != cd['face']: e.append(f'{c}: face {a.get("face")} != {cd["face"]}')
        for d in a['files']:
            fs = st.frames.get((c, d))
            if not fs: continue
            for i, f in enumerate(fs):
                if f.shape[:2] != (cd['frame'][1], cd['frame'][0]):
                    e.append(f'{c} {d} f{i}: {f.shape[1]}x{f.shape[0]} != {cd["frame"]}'); break
                al = f[..., 3]
                if al[0].any() or al[-1].any() or al[:, 0].any() or al[:, -1].any():
                    e.append(f'{c} {d} f{i}: art touches the canvas edge'); break
            rp = (a.get('root_px') or {}).get(d) or []
            if any(abs(x) > 1e-9 or abs(y) > 1e-9 for x, y in rp): e.append(f'{c} {d}: root_px moves (postures stay put)')
    for fam, (down, loop, up) in mod.FAMILIES.items():
        for c, want in ((down, {'start': 'idle0'}), (up, {'end': 'idle0'}), (loop, None)):
            if c not in J['anims']: e.append(f'{fam}: {c} missing'); continue
            if (J['anims'][c].get('handoff') or None) != want: e.append(f'{fam}: {c} hand-off {J["anims"][c].get("handoff")} != {want}')
        if not J['anims'].get(loop, {}).get('loop'): e.append(f'{fam}: {loop} is not a loop')
        for d in stagekit.DIRS:
            x, y, z = st.frames.get((down, d)), st.frames.get((loop, d)), st.frames.get((up, d))
            if x is None or y is None or z is None: continue
            if not np.array_equal(x[-1], y[0]): e.append(f'{fam} {d}: chain {down}[last] != {loop}[0] ({stagekit.changed(x[-1], y[0])} px)')
            if not np.array_equal(z[0], y[0]): e.append(f'{fam} {d}: chain {up}[0] != {loop}[0] ({stagekit.changed(z[0], y[0])} px)')
    part = st.part or {}
    layers = [c for c, cd in mod.CLIPS.items() if cd['face'] == 'layers']
    for c in layers:
        if sorted((part.get('clips') or {}).get(c, {})) != sorted(mod.CLIPS[c]['dirs']):
            e.append(f'face part: {c} dirs {sorted((part.get("clips") or {}).get(c, {}))} != {sorted(mod.CLIPS[c]["dirs"])}')
    for c in (part.get('clips') or {}):
        if c not in layers: e.append(f'face part: {c} is not a layers clip')
    return e


def mutate_loop_late(st):
    """sit_floor S starts one frame late: frame 0 is the old frame 1 (the loop's seam broken by one frame)."""
    fs = st.frames[('sit_floor', 'S')]
    st.frames[('sit_floor', 'S')] = fs[1:] + fs[:1]
    return 'chain'


MUTATIONS = [('a loop seam broken by one frame (sit_floor S starts one frame late)', mutate_loop_late)]

if __name__ == '__main__':
    stagekit.verify_main('p', extra, MUTATIONS)
