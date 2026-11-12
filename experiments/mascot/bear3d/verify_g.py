"""Verify Stage g (fidgets-b, out/bear3d/anim_g/, stages/g.json): the stage kit's clip checks (stagekit.verify_main:
palette, alpha 0/255, 1 px ink outline, clip order, frame counts, dirs, hand-offs = idle#0 with the anchors aligned,
root_px as ONE track) plus the storyboard rows of #sb-fidgets-b:
  - every clip is a one-shot with face "baked", in place (root_px all [0, 0]) and declares the idle#0 hand-off at
    BOTH ends (so the kit's hand-off check runs on frame 0 and on the last frame of every strip);
  - the clips play in S SE E W SW only, in Stage A's 84x123 frame with anchor [42, 107];
  - no face part: a baked stage writes no face_g.json (a stray one would put layers on a baked face).

    python3 verify_g.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_g.py --self-test       exit 1 = every mutation (6 standard + 3 stage g) was caught
"""
import os, sys
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K
import clips_g as G

F5 = sorted(G.DIRS)


def extra(st):
    e = []
    for c, a in st.J['anims'].items():
        if a.get('face') != 'baked': e.append(f'{c}: face {a.get("face")!r} != "baked" (#sb-fidgets-b)')
        if a.get('loop'): e.append(f'{c}: a fidget is a one-shot, not a loop')
        h = a.get('handoff') or {}
        for at in ('start', 'end'):
            if h.get(at) != 'idle0': e.append(f'{c}: hand-off ({at}) {h.get(at)!r} != "idle0" (#sb-fidgets-b)')
        if sorted(a['files']) != F5: e.append(f'{c}: dirs {sorted(a["files"])} != {F5}')
        if (a['frame_w'], a['frame_h']) != G.FRAME or tuple(a['anchor']) != G.ANCHOR:
            e.append(f'{c}: frame {a["frame_w"]}x{a["frame_h"]} anchor {a["anchor"]} != {G.FRAME} {list(G.ANCHOR)}')
        moved = sorted({d for d, steps in (a.get('root_px') or {}).items() if any(p != [0, 0] for p in steps)})
        if moved: e.append(f'{c}: root_px moves the sprite in {moved} (fidgets stay in place)')
    if os.path.exists(os.path.join(st.dir, f'face_{st.sid}.json')) or st.part is not None:
        e.append(f'face_{st.sid}.json present: stage g is baked-face only (no face part)')
    return e


def m_end_handoff(s):
    """the END hand-off frame of the last clip, SW, 1 px lower than idle#0 (the standard mutation moves a start frame
    sideways): a hand-off that is 1 px off idle#0 must fail."""
    fs = s.frames[(G.ORDER[-1], 'SW')]; fs[-1] = np.roll(fs[-1], 1, 0)
    return f'{G.ORDER[-1]} SW: hand-off (end)'


def m_no_handoff(s):
    """a clip that no longer declares its end hand-off (the kit would then skip the end-frame comparison)."""
    del s.J['anims']['stretch']['handoff']['end']
    return 'stretch: hand-off (end)'


def m_face_layers(s):
    """a clip marked 'layers' (the app would draw face patches over a tilted, closed-eye face)."""
    s.J['anims']['fid_yawn']['face'] = 'layers'
    return 'fid_yawn: face'


MUTATIONS = [('an end hand-off frame 1 px off idle#0', m_end_handoff),
             ('a clip without its end hand-off', m_no_handoff),
             ('a baked clip marked face layers', m_face_layers)]

if __name__ == '__main__':
    K.verify_main('g', extra, MUTATIONS)
