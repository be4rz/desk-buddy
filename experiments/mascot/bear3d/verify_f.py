"""Verify Stage f (fidgets-a; out/bear3d/anim_f/, stages/f.json, clips_f.py): the stage kit's clip checks
(stagekit.verify_main: palette, alpha 0/255, 1 px ink outline, frame counts, dirs, hand-offs = idle#0 anchor-aligned,
root_px) plus:
  - every clip has the storyboard's face handling (clips_f.LAYERS = 'layers'), and the face part face_f.json covers
    exactly the layers clips in every face dir (S SE E W SW), each with frame 0 and the last frame exact;
  - the fidgets stand still: root_px is 0 on every frame (no clip moves the panel);
  - the art never touches the canvas edge;
  - verify_face.py passes on the layers frames as they are in memory (written to a temp copy and checked with
    `verify_face.py --part`), so a mitten that crosses the face of a layers frame fails here as it does there.

    python3 verify_f.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_f.py --self-test       exit 1 = every mutation (6 standard + 3 stage f) was caught
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
    tmp = tempfile.mkdtemp(prefix='verify_f_')
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


def extra(st):
    e = []; mod = st.mod
    layers = set(mod.LAYERS)
    for c, a in st.J['anims'].items():
        want = mod.CLIPS[c]['face']
        if a.get('face') != want: e.append(f'{c}: face {a.get("face")!r} != storyboard {want!r}')
        for d, steps in (a.get('root_px') or {}).items():
            if any(abs(x) > 1e-9 or abs(y) > 1e-9 for x, y in steps): e.append(f'{c} {d}: root_px moves the panel (a fidget stands still)')
        for d in a['files']:
            for i, f in enumerate(st.frames.get((c, d), [])):
                edge = np.concatenate([f[0, :, 3], f[-1, :, 3], f[:, 0, 3], f[:, -1, 3]])
                if edge.any(): e.append(f'{c} {d} f{i}: art touches the canvas edge')
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


def m_paw_on_face(s):
    """a white mitten (fur, ink rim) over the eyes of a tummy_rub S frame: the outline stays closed, so only the
    face check (verify_face.py on the in-memory frames) can catch it."""
    c, d = 'fid_tummy_rub', 'S'; f = s.frames[(c, d)][9]
    ex, ey = s.part['clips'][c][d]['eyes'][9]
    op = f[f[..., 3] > 0][:, :3]; cols, n = np.unique(op, axis=0, return_counts=True)
    fur = cols[n.argmax()]                           # the most common body colour
    ink = f[ey + 4, :, :3][f[ey + 4, :, 3] > 0][0]   # the silhouette's left edge on that row
    f[ey - 1:ey + 7, ex - 1:ex + 13, :3] = ink; f[ey:ey + 6, ex:ex + 12, :3] = fur
    return 'verify_face.py: '


def m_face_flag(s):
    s.J['anims']['fid_peek']['face'] = 'layers'; return 'fid_peek: face'


def m_root(s):
    a = s.J['anims']['fid_ear_twitch']
    for d in a['files']:
        a['root_px'][d][3] = [1.0, 0.0]; a['frames'][3]['root_px'][d] = [1.0, 0.0]
    return 'moves the panel'


if __name__ == '__main__':
    K.verify_main('f', extra=extra, mutations=[('a mitten moved onto the face of a layers frame', m_paw_on_face),
                                               ('a baked clip marked layers', m_face_flag),
                                               ('a fidget that moves the panel', m_root)])
