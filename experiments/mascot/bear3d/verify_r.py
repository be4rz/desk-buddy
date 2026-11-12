"""Verify Stage r (react-a; out/bear3d/anim_r/, stages/r.json, clips_r.py): the stage kit's clip checks
(stagekit.verify_main: palette, alpha 0/255, 1 px ink outline, frame counts, dirs, loop flags and seams, hand-offs =
idle#0 anchor-aligned, root_px) plus:
  - every clip has the storyboard's face handling (clips_r.CLIPS) and hand-offs idle0 at both ends;
  - every loop (clips_r.LOOPS) starts AND ends on idle#0 pixel for pixel (anchor-aligned), checked on the frames
    themselves, independent of the json's hand-off field (so each pass of the loop begins and ends on idle#0);
  - the clips stand still (root_px 0 on every frame) and the art never touches the canvas edge;
  - the toy watch (kit objects, rose / cream) shows only in check_watch, never on its hand-off frames;
  - the face part face_r.json covers exactly the layers clips in S SE E W SW (frame 0 and the last frame exact), and
    verify_face.py passes on those frames as they are in memory (a mitten over the face of a layers frame fails).

    python3 verify_r.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_r.py --self-test       exit 1 = every mutation (standard + 4 stage r) was caught
"""
import os, sys, json, hashlib, shutil, subprocess, tempfile
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K

HERE = os.path.dirname(os.path.abspath(__file__))
FACE_DIRS = ['S', 'SE', 'E', 'W', 'SW']
_FACE_CACHE = {}


def face_check(st):
    """run verify_face.py --part on the stage's layers frames as they are in memory -> its failure lines."""
    if st.part is None: return []
    layers = sorted(k for k in st.frames if k[0] in (st.part.get('clips') or {}))
    h = hashlib.sha256(json.dumps(st.part, sort_keys=True).encode())
    for k in layers:
        h.update(repr(k).encode())
        for f in st.frames[k]: h.update(f.tobytes())
    key = h.hexdigest()
    if key in _FACE_CACHE: return _FACE_CACHE[key]
    tmp = tempfile.mkdtemp(prefix='verify_r_')
    try:
        anim = st.part.get('anim') or f'anim_{st.sid}.json'
        J = dict(st.J, anims={c: a for c, a in st.J['anims'].items() if c in (st.part.get('clips') or {})})
        json.dump(J, open(os.path.join(tmp, anim), 'w'))
        for c, d in layers:
            K.save(K.join(st.frames[(c, d)]), os.path.join(tmp, st.J['anims'][c]['files'][d]))
        fp = os.path.join(tmp, f'face_{st.sid}.json'); json.dump(st.part, open(fp, 'w'))
        r = subprocess.run([sys.executable, os.path.join(HERE, 'verify_face.py'), '--part', fp],
                           capture_output=True, text=True, cwd=HERE)
        lines = r.stdout.splitlines()
        tail = lines[lines.index('verify_face: FAIL') + 1:] if 'verify_face: FAIL' in lines else []
        out = [] if r.returncode == 0 else \
            [f'verify_face.py: {l.strip()}' for l in tail] or \
            [f'verify_face.py: exit {r.returncode}: {(r.stdout + r.stderr)[-300:]}']
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    _FACE_CACHE[key] = out
    return out


def watch_colours(sid='r'):
    """the palette colours the watch objects pixelize to (clips_r.PARTS ramps)."""
    import clips_r
    R = K.ramps(sid)
    return {tuple(R[p['ramp']][i]) for p in clips_r.PARTS.values() for i in p['idx']} - \
        {tuple(c) for n, v in R.items() if n in ('ink', 'fur', 'pants', 'hat', 'blush') for c in v}


def extra(st):
    e = []; mod = st.mod
    layers = set(mod.LAYERS); loops = set(mod.LOOPS)
    wc = np.array(sorted(watch_colours(st.sid)), np.uint8)
    for c, a in st.J['anims'].items():
        cd = mod.CLIPS[c]
        if a.get('face') != cd['face']: e.append(f'{c}: face {a.get("face")!r} != storyboard {cd["face"]!r}')
        if (a.get('handoff') or {}) != {'start': 'idle0', 'end': 'idle0'}:
            e.append(f'{c}: hand-off {a.get("handoff")} != idle0 at both ends (storyboard)')
        for d, steps in (a.get('root_px') or {}).items():
            if any(abs(x) > 1e-9 or abs(y) > 1e-9 for x, y in steps): e.append(f'{c} {d}: root_px moves the panel (a reaction stands still)')
        for d in a['files']:
            fs = st.frames.get((c, d), [])
            if c in loops and fs:
                spr, sa = K.idle0(d)
                for at, i in (('frame 0', 0), ('last frame', len(fs) - 1)):
                    n, _ = K.aligned_diff(fs[i], a['anchor'], spr, sa)
                    if n: e.append(f'{c} {d}: loop {at} is not idle#0 ({n} px differ, anchors aligned)')
            for i, f in enumerate(fs):
                edge = np.concatenate([f[0, :, 3], f[-1, :, 3], f[:, 0, 3], f[:, -1, 3]])
                if edge.any(): e.append(f'{c} {d} f{i}: art touches the canvas edge')
                op = f[f[..., 3] > 0][:, :3]
                nw = int((op[:, None, :] == wc[None]).all(-1).any(-1).sum()) if len(wc) and len(op) else 0
                if c != 'check_watch' and nw: e.append(f'{c} {d} f{i}: {nw} watch-coloured px outside check_watch')
                if c == 'check_watch' and i in (0, len(fs) - 1) and nw: e.append(f'{c} {d} f{i}: the watch shows on a hand-off frame')
        if a.get('loop') != (c in loops): e.append(f'{c}: loop {a.get("loop")} != storyboard {c in loops}')
    part = st.part
    if part is None:
        e.append(f'face_{st.sid}.json missing (the layers clips {sorted(layers)} need a face part)')
    else:
        pc = part.get('clips') or {}
        if set(pc) != layers: e.append(f'face part clips {sorted(pc)} != the layers clips {sorted(layers)}')
        for c in sorted(set(pc) & layers):
            if sorted(pc[c]) != sorted(FACE_DIRS): e.append(f'face part {c}: dirs {sorted(pc[c])} != {FACE_DIRS}')
            n = st.J['anims'].get(c, {}).get('frames_n', 0)
            for d in pc[c]:
                ex = (part.get('exact') or {}).get(c, {}).get(d, [])
                if 0 not in ex or n - 1 not in ex: e.append(f'face part {c} {d}: hand-off frames 0 and {n - 1} must be exact')
                for s in ('eyes', 'mouth'):
                    if len(pc[c][d].get(s, [])) != n: e.append(f'face part {c} {d}: {s} positions != {n} frames')
        e += face_check(st)
    return e


def m_loop_start(s):
    """a loop whose frame 0 is not idle#0 (its json hand-off dropped too, so only the loop rule can catch it)."""
    c, d = 'head_bob', 'SE'
    s.J['anims'][c].pop('handoff', None)
    fs = s.frames[(c, d)]; fs[0] = fs[2].copy()
    return f'{c} {d}: loop frame 0 is not idle#0'


def m_paw_on_face(s):
    """a white mitten (fur, ink rim) over the eyes of a foot_tap S frame: the outline stays closed, so only the face
    check (verify_face.py on the in-memory frames) can catch it."""
    c, d = 'foot_tap', 'S'; i = 5; f = s.frames[(c, d)][i]
    ex, ey = s.part['clips'][c][d]['eyes'][i]
    op = f[f[..., 3] > 0][:, :3]; cols, n = np.unique(op, axis=0, return_counts=True)
    fur = cols[n.argmax()]
    ink = f[ey + 4, :, :3][f[ey + 4, :, 3] > 0][0]
    f[ey - 1:ey + 7, ex - 1:ex + 13, :3] = ink; f[ey:ey + 6, ex:ex + 12, :3] = fur
    return 'verify_face.py: '


def m_watch_elsewhere(s):
    """watch-coloured pixels inside a giggle frame (as if the toy watch leaked into another clip)."""
    c, d = 'giggle', 'S'; f = s.frames[(c, d)][8]
    wc = sorted(watch_colours(s.sid))[0]
    ys, xs = np.nonzero(f[..., 3] > 0); cy, cx = int(ys.mean()), int(xs.mean())
    f[cy:cy + 3, cx:cx + 3, :3] = wc
    return 'watch-coloured px outside check_watch'


def m_face_flag(s):
    s.J['anims']['giggle']['face'] = 'layers'; return 'giggle: face'


if __name__ == '__main__':
    K.verify_main('r', extra=extra, mutations=[('a loop whose frame 0 is not idle#0', m_loop_start),
                                               ('a mitten moved onto the face of a layers frame', m_paw_on_face),
                                               ('the watch leaking into another clip', m_watch_elsewhere),
                                               ('a baked clip marked layers', m_face_flag)])
