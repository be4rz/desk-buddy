"""Verify Stage s (react-b; out/bear3d/anim_s/, stages/s.json): the stage kit's clip checks (stagekit.verify_main:
palette, alpha 0/255, 1 px ink outline, counts, dirs, frame sizes, idle0 hand-offs, the CHAINS join land_squash[0] =
dangle[0] in all 8 dirs, loop seams, root_px = one track) plus:
  - each clip's face, frame and anchor are clips_s.CLIPS' (84x123 [42, 107]; dangle / dizzy / ... baked,
    land_squash none) and the stage writes no face part (no layers clip);
  - no root motion anywhere (every clip plays on the spot);
  - dangle is off the ground: in every frame and direction the lowest opaque row is above the anchor line and at
    least LIFT_MIN px above idle#0's lowest row in that direction (the brief's own check);
  - dangle has no hand-off (drag start hard-cuts to it), land_squash ends on idle#0, the others are idle0 both ends;
  - dizzy shows its toy stars (STAR_COLOURS, never on the bear) on frames STARS_ON and none on its ends;
  - the art keeps >= 2 px from every canvas edge (render_anim.py's --canvas rule).

    python3 verify_s.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_s.py --self-test       exit 1 = every mutation (the kit's + the stage's own) was caught
"""
import os, sys
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K

EDGE = 2
LIFT_MIN = 8                     # px; the clip lifts 10 (clips_s.LIFT_PX), the feet kick by up to 2
STARS_ON = range(4, 24)          # dizzy frames that must show stars
STAR_MIN = 4                     # star px per such frame


def star_px(f, cols):
    return int(sum((np.all(f[..., :3] == np.array(c, np.uint8), -1) & (f[..., 3] == 255)).sum() for c in cols))


def extra(st):
    e = []; J = st.J; C = st.mod.CLIPS
    if st.part is not None: e.append('face_s.json exists: stage s has no face layers clip')
    cols = [K.hx(h) for h in st.mod.STAR_COLOURS]
    for c, a in J['anims'].items():
        cd = C.get(c, {})
        if (a.get('face') or 'none') != (cd.get('face') or 'none'): e.append(f'{c}: face {a.get("face")!r} != {cd.get("face")!r}')
        if ([a['frame_w'], a['frame_h']], list(a['anchor'])) != ([84, 123], [42, 107]):
            e.append(f'{c}: frame {a["frame_w"]}x{a["frame_h"]} anchor {a["anchor"]} != 84x123 [42, 107]')
        if (a.get('handoff') or {}) != (cd.get('handoff') or {}):
            e.append(f'{c}: hand-off {a.get("handoff")} != storyboard {cd.get("handoff")}')
        if any(abs(v) > 1e-9 for p in (a.get('root_track') or []) for v in p): e.append(f'{c}: root motion (must play on the spot)')
        for d in a['files']:
            fs = st.frames.get((c, d))
            if not fs: continue
            for i, f in enumerate(fs):
                al = f[..., 3]
                if al[:EDGE].any() or al[-EDGE:].any() or al[:, :EDGE].any() or al[:, -EDGE:].any():
                    e.append(f'{c} {d} f{i}: art within {EDGE} px of the canvas edge')
            if c == 'dangle':
                spr, sa = K.idle0(d)
                ib = int(np.nonzero(spr[..., 3])[0].max()) + (a['anchor'][1] - sa[1])
                for i, f in enumerate(fs):
                    ys = np.nonzero(f[..., 3])[0]
                    lo = int(ys.max()) if len(ys) else -1
                    if lo >= a['anchor'][1] or lo > ib - LIFT_MIN:
                        e.append(f'dangle {d} f{i}: feet touch the ground (lowest row {lo}; anchor line {a["anchor"][1]}, '
                                 f'idle lowest {ib}, must be <= {ib - LIFT_MIN}): the bear must be off the ground')
            if c == 'dizzy':
                for i, f in enumerate(fs):
                    n = star_px(f, cols)
                    if i in STARS_ON and n < STAR_MIN: e.append(f'dizzy {d} f{i}: {n} star px (< {STAR_MIN}): the stars are missing')
                    if i in (0, len(fs) - 1) and n: e.append(f'dizzy {d} f{i}: {n} star px on a hand-off frame')
    return e


def m_ground(s):
    """a dangle frame dropped back onto the floor (shifted down by the lift)."""
    fs = s.frames[('dangle', 'SE')]; fs[5] = np.roll(fs[5], 10, 0)
    return 'must be off the ground'


def m_chain(s):
    fs = s.frames[('land_squash', 'S')]; fs[0] = np.roll(fs[0], 1, 1)
    return 'chain land_squash[0] = dangle[0]'


def m_stars(s):
    cols = [K.hx(h) for h in s.mod.STAR_COLOURS]; f = s.frames[('dizzy', 'S')][10]
    for c in cols: f[np.all(f[..., :3] == np.array(c, np.uint8), -1), :3] = K.INK
    return 'the stars are missing'


def m_edge(s):
    f = s.frames[('tap_glass', 'S')][3]; f[0, f.shape[1] // 2] = (*K.INK, 255)
    return 'of the canvas edge'


def m_root(s):
    a = s.J['anims']['dizzy']; a['root_track'] = [[0.0, 0.01 * i] for i in range(len(a['root_track']))]
    return 'root motion'


def m_handoff_dangle(s):
    s.J['anims']['dangle']['handoff'] = {'start': 'idle0'}
    return "hand-off {'start': 'idle0'} != storyboard"


MUTATIONS = [('a dangle frame back on the ground', m_ground), ('land_squash[0] != dangle[0]', m_chain),
             ('dizzy without its stars', m_stars), ('art on the canvas edge', m_edge),
             ('dizzy drifts (root motion)', m_root), ('dangle claims an idle0 hand-off', m_handoff_dangle)]

if __name__ == '__main__':
    K.verify_main('s', extra=extra, mutations=MUTATIONS)
