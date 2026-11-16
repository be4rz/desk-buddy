"""Stage p verifier (postures): python3 verify_p.py [--dir DIR] [--self-test]   (exit 0 pass / 1 findings;
--self-test exit 1 = every mutation caught, 2 = one missed).

The kit's standard checks (stagekit.verify_main: pixel rules, counts, dirs, idle#0 hand-offs, loop seams, root_px,
CHAINS) plus, for this stage:
  - every clip has its storyboard frame (132x123 anchor [66,107]; squat 84x123 anchor [42,107]) in the JSON and the strips;
  - every posture family is complete: down[last] = loop[0] = up[0] in every dir (sit_sway shares sit_floor's down / up),
    the down clip starts and the up clip ends on idle#0 (hand-off field set), the loop has no hand-off;
  - no root motion (postures stay on the spot);
  - art never touches the canvas edge (1 px clear border), so nothing is clipped;
  - face: sit_floor is 'layers' with a face part entry for every dir, every other clip is not in the face part;
  - legs (the poses in clips_p.CLIPS, every frame): a pad never leaves its leg. Wherever a leg swings (legL / legR != 0)
    or the lump is tipped (tumble != 0), that side's fL / hL stay within PAD_MAX (a pad pushed off its leg reads as a
    second, detached pair of legs); the two legs never split by more than SPLIT_MAX deg; every frame of a lying loop
    (tumble != 0) swings both legs at least RIDE_MIN deg, so the pads ride their legs and not the tipped floor.
Stage mutations: a loop strip that starts one frame late (its seam broken by one frame) must fail the chain check;
back_lie pedalling with the pads again (legs at rest, fL / hL pushed out) must fail the leg check.
"""
import copy, types
import numpy as np
import anim_clips
import stagekit

PAD_MAX, SPLIT_MAX, RIDE_MIN = 0.04, 30.0, 15.0


def leg_errors(clips):
    """the leg rule on every expanded pose of clips (name -> clip dict)."""
    e = []
    for c, cd in clips.items():
        poses = anim_clips.clip_poses(cd)
        lying_loop = cd.get('loop') and any(q.get('tumble', 0) for q in poses)
        for i, q in enumerate(poses):
            for s in ('L', 'R'):
                leg = q.get('leg' + s, 0.0)
                if (leg or q.get('tumble', 0)) and max(abs(q['f' + s]), abs(q['h' + s])) > PAD_MAX + 1e-9:
                    e.append(f'{c} f{i}: pad {s} off its leg (f{s} {q["f" + s]:.2f} h{s} {q["h" + s]:.2f}, leg{s} {leg:.0f}; '
                             f'swing the leg, keep the pad within {PAD_MAX})'); break
                if lying_loop and abs(leg) < RIDE_MIN - 1e-9:
                    e.append(f'{c} f{i}: lying with leg{s} {leg:.1f} (< {RIDE_MIN} deg: the pad stays on the tipped floor)'); break
            if abs(q.get('legL', 0.0) - q.get('legR', 0.0)) > SPLIT_MAX + 1e-9:
                e.append(f'{c} f{i}: legs split {abs(q["legL"] - q["legR"]):.0f} deg > {SPLIT_MAX} (pants crease)')
    return e


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
    return e + leg_errors(mod.CLIPS)


def mutate_loop_late(st):
    """sit_floor S starts one frame late: frame 0 is the old frame 1 (the loop's seam broken by one frame)."""
    fs = st.frames[('sit_floor', 'S')]
    st.frames[('sit_floor', 'S')] = fs[1:] + fs[:1]
    return 'chain'


def mutate_pad_legs(st):
    """back_lie pedals with the pads again (the old fake legs): legs at rest, fL / hL pushed away from the body."""
    clips = copy.deepcopy(st.mod.CLIPS)
    for _, key in clips['back_lie']['keys']:
        key.update(legL=0.0, legR=0.0, fL=0.30, fR=0.30, hL=0.20, hR=0.20)
    st.mod = types.SimpleNamespace(**{k: getattr(st.mod, k) for k in dir(st.mod) if not k.startswith('__')})
    st.mod.CLIPS = clips
    return 'off its leg'


MUTATIONS = [('a loop seam broken by one frame (sit_floor S starts one frame late)', mutate_loop_late),
             ('back_lie pedals with detached pads again (legs at rest, fL / hL pushed out)', mutate_pad_legs)]

if __name__ == '__main__':
    stagekit.verify_main('p', extra, MUTATIONS)
