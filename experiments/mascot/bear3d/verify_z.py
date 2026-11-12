"""Verify Stage z (rare-art; out/bear3d/anim_z/, stages/z.json, clips_z.py): the stage kit's clip checks
(stagekit.verify_main: palette, alpha 0/255, 1 px ink outline, clip order, frame counts, dirs, hand-offs = idle#0
anchor-aligned, loop seams, root_px as ONE track) plus the storyboard rows #sb-rare:
  - the eleven clips with the storyboard's frame counts, Stage A's 84x123 frame and anchor [42, 107], the idle#0
    hand-off declared at BOTH ends; moonwalk is the only loop and plays in 8 dirs with face "none", the rest are
    one-shots in S SE E W SW with face "baked"; no face part (a stray face_z.json would put layers on a baked face);
  - every clip but moonwalk stands still (root_px 0); moonwalk moves backwards: in every dir each root step points
    against the facing (never forward) and the loop travels at least 8 px;
  - the effect objects show: on every frame where clips_z shows objects, their own colours (the parts' ramp colours
    the bear's Stage A sprites never use) cover at least MIN_FX px, every object shows in every dir, and no frame
    where every object is hidden has any of them;
  - the art never touches the canvas edge.

    python3 verify_z.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_z.py --self-test       exit 1 = every mutation (7 standard + 6 stage z) was caught
"""
import os, sys
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K
import anim_clips as A
import clips_z as Z

F5 = sorted(Z.F5)
D8 = sorted(Z.DIRS8)
MIN_FX = 6                # px of an object's own colours on a frame that shows it
MIN_TRAVEL = 8.0          # px the moonwalk loop slides back in every dir


def _bear_colours():
    """every colour the bear's own Stage A sprites use (the frozen anim/ strips)."""
    out = set()
    for f in sorted(os.listdir(os.path.join(K.REAL_OUT, 'anim'))):
        if not f.endswith('.png'): continue
        a = np.asarray(K.load(os.path.join(K.REAL_OUT, 'anim', f)))
        out |= {tuple(int(x) for x in c) for c in np.unique(a[a[..., 3] > 0][:, :3], axis=0)}
    return out


def fx_colours():
    """object -> its part's ramp colours (rgb), minus every colour the bear itself uses."""
    R = K.ramps('z'); bear = _bear_colours(); out = {}
    for n, p in Z.PARTS.items():
        out[n] = {R[p['ramp']][i] for i in p['idx']} - bear
    return out


def shown(c):
    """per frame of clip c: the set of kit objects clips_z shows."""
    rest = dict(A.REST_K)
    for n in Z.PARTS: rest.update({f'vis_{n}': 0, f'ox_{n}': 0.0, f'oy_{n}': 0.0, f'oz_{n}': 0.0, f'orz_{n}': 0.0})
    return [{n for n in Z.PARTS if q[f'vis_{n}']} for q in A.clip_poses(Z.CLIPS[c], rest)]


_FX = fx_colours()
_ALL_FX = set().union(*_FX.values())
_SHOWN = {c: shown(c) for c in Z.ORDER}


def count(f, cols):
    if not cols: return 0
    op = f[..., 3] > 0; rgb = f[..., :3]
    m = np.zeros(op.shape, bool)
    for c in cols: m |= np.all(rgb == np.array(c, np.uint8), -1)
    return int((m & op).sum())


def extra(st):
    e = []
    if list(st.J['anims']) != Z.ORDER: e.append(f'clips {list(st.J["anims"])} != storyboard {Z.ORDER} (#sb-rare)')
    for c, a in st.J['anims'].items():
        moon = c == 'moonwalk'
        want = 'none' if moon else 'baked'
        if a.get('face') != want: e.append(f'{c}: face {a.get("face")!r} != storyboard {want!r} (#sb-rare)')
        if bool(a.get('loop')) != moon: e.append(f'{c}: loop {a.get("loop")} != storyboard {moon} (only moonwalk loops)')
        if a.get('frames_n') != Z.FRAMES.get(c): e.append(f'{c}: {a.get("frames_n")} frames != storyboard {Z.FRAMES.get(c)}')
        h = a.get('handoff') or {}
        for at in ('start', 'end'):
            if h.get(at) != 'idle0': e.append(f'{c}: hand-off ({at}) {h.get(at)!r} != "idle0" (#sb-rare)')
        dirs = D8 if moon else F5
        if sorted(a['files']) != dirs: e.append(f'{c}: dirs {sorted(a["files"])} != {dirs}')
        if (a['frame_w'], a['frame_h']) != Z.FRAME or tuple(a['anchor']) != Z.ANCHOR:
            e.append(f'{c}: frame {a["frame_w"]}x{a["frame_h"]} anchor {a["anchor"]} != {Z.FRAME} {list(Z.ANCHOR)}')
        rp = a.get('root_px') or {}
        if not moon:
            for d, steps in rp.items():
                if any(abs(x) > 1e-9 or abs(y) > 1e-9 for x, y in steps): e.append(f'{c} {d}: root_px moves the panel (stands in place)')
        else:
            for d in a['files']:
                fx, fy = K.root_px(st.J['ppu'], K.YAW[d], 0.0, 1.0)          # one world unit forward, on screen
                n = (fx * fx + fy * fy) ** 0.5
                dots = [(x * fx + y * fy) / n for x, y in rp.get(d, [])]
                if any(v > 1e-6 for v in dots): e.append(f'{c} {d}: a root step moves forward (the moonwalk slides backwards)')
                if -sum(dots) < MIN_TRAVEL: e.append(f'{c} {d}: the loop slides back {-sum(dots):.1f} px (< {MIN_TRAVEL})')
        sh = _SHOWN.get(c)
        for d in a['files']:
            fs = st.frames.get((c, d), []); seen = set()
            for i, f in enumerate(fs):
                edge = np.concatenate([f[0, :, 3], f[-1, :, 3], f[:, 0, 3], f[:, -1, 3]])
                if edge.any(): e.append(f'{c} {d} f{i}: art touches the canvas edge')
                if sh is None or i >= len(sh): continue
                if sh[i]:
                    k = count(f, set().union(*(_FX[o] for o in sh[i])))
                    if k < MIN_FX: e.append(f'{c} {d} f{i}: effect objects {sorted(sh[i])} show only {k} px of their colours (< {MIN_FX})')
                    seen |= {o for o in sh[i] if count(f, _FX[o])}
                else:
                    k = count(f, _ALL_FX)
                    if k: e.append(f'{c} {d} f{i}: {k} px of effect-object colours on a frame with every object hidden')
            for o in sorted(set().union(*sh) - seen if sh and fs else ()):
                e.append(f'{c} {d}: effect object {o} never shows')
    if st.part is not None or os.path.exists(os.path.join(st.dir, f'face_{st.sid}.json')):
        e.append(f'face_{st.sid}.json present: stage z is baked / none face only (no face part)')
    return e


def m_moon_forward(s):
    """the moonwalk turned into a plain forward slide (both root_px forms and the root track flipped)."""
    a = s.J['anims']['moonwalk']
    for d in a['files']:
        a['root_px'][d] = [[-x, -y] for x, y in a['root_px'][d]]
        for i, f in enumerate(a['frames']): f['root_px'][d] = list(a['root_px'][d][i])
    a['root_track'] = [[-x, -y] for x, y in a['root_track']]
    return 'moves forward'


def m_moon_one_shot(s):
    s.J['anims']['moonwalk']['loop'] = False; return 'moonwalk: loop'


def m_hat_layers(s):
    """a hat clip marked face layers (the app would draw face patches over a baked, tilted face under a hat)."""
    s.J['anims']['hat_winter']['face'] = 'layers'; return 'hat_winter: face'


def m_no_end_handoff(s):
    """a clip that no longer declares its end hand-off (the kit would skip the end-frame comparison)."""
    del s.J['anims']['find_coin']['handoff']['end']; return 'find_coin: hand-off (end)'


def m_hat_missing(s):
    """the pom-pom hat drawn without its knit (every rose pixel painted with the cream trim colour) in hat_winter S:
    the object no longer reads."""
    trim = sorted(_FX['hat_winter_trim'])[0]
    for f in s.frames[('hat_winter', 'S')]:
        for col in _FX['hat_winter']:
            m = np.all(f[..., :3] == np.array(col, np.uint8), -1) & (f[..., 3] > 0); f[m, :3] = trim
    return 'hat_winter S: effect object hat_winter never shows'


def m_fx_leak(s):
    """a coin pixel left on a frame where every object is hidden (an object shown past its keys)."""
    c, d = 'windmill', 'SE'; f = s.frames[(c, d)][5]
    ys, xs = np.nonzero(f[..., 3] > 0); ys, xs = ys[len(ys) // 2:], xs[len(xs) // 2:]
    pad = np.pad(f[..., 3] > 0, 1); inner = pad[:-2, 1:-1] & pad[2:, 1:-1] & pad[1:-1, :-2] & pad[1:-1, 2:]
    k = next(j for j in range(len(ys)) if inner[ys[j], xs[j]])
    f[ys[k], xs[k], :3] = sorted(_FX['coin_floor'])[0]
    return 'effect-object colours on a frame with every object hidden'


MUTATIONS = [('the moonwalk sliding forward', m_moon_forward),
             ('the moonwalk as a one-shot', m_moon_one_shot),
             ('a hat clip marked face layers', m_hat_layers),
             ('a clip without its end hand-off', m_no_end_handoff),
             ('a hat frame without the hat', m_hat_missing),
             ('an effect colour on a frame with no object', m_fx_leak)]

if __name__ == '__main__':
    K.verify_main('z', extra=extra, mutations=MUTATIONS)
