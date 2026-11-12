"""Verify Stage x (gestures-b; out/bear3d/anim_x/, stages/x.json, clips_x.py): the stage kit's clip checks
(stagekit.verify_main: palette, alpha 0/255, 1 px ink outline, clip order, frame counts, dirs, hand-offs = idle#0
anchor-aligned, root_px as ONE track) plus the storyboard rows #sb-gestures-b:
  - every clip is a one-shot in S SE E W SW, Stage A's 84x123 frame with anchor [42, 107], with the storyboard's face
    handling (clips_x.LAYERS = 'layers', the rest 'baked') and the idle#0 hand-off declared at BOTH ends;
  - the gestures stand still: root_px is 0 on every frame;
  - the art never touches the canvas edge;
  - the face part face_x.json covers exactly the layers clips in every face dir, frame 0 and the last frame exact;
  - verify_face.py passes on the layers frames as they are in memory (a temp copy checked with `verify_face.py
    --part`), so a mitten across the face of a layers frame, or a face patch position off by 1 px, fails here.

    python3 verify_x.py [--dir DIR]       exit 0 = pass, 1 = errors
    python3 verify_x.py --self-test       exit 1 = every mutation (7 standard + 4 stage x) was caught
"""
import os, sys, json, hashlib, shutil, subprocess, tempfile
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagekit as K
import clips_x as X

HERE = os.path.dirname(os.path.abspath(__file__))
F5 = sorted(X.F5)
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
    tmp = tempfile.mkdtemp(prefix='verify_x_')
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
    e = []; layers = set(X.LAYERS)
    for c, a in st.J['anims'].items():
        want = 'layers' if c in layers else 'baked'
        if a.get('face') != want: e.append(f'{c}: face {a.get("face")!r} != storyboard {want!r} (#sb-gestures-b)')
        if a.get('loop'): e.append(f'{c}: a gesture is a one-shot, not a loop')
        if a.get('frames_n') != X.FRAMES.get(c): e.append(f'{c}: {a.get("frames_n")} frames != storyboard {X.FRAMES.get(c)}')
        h = a.get('handoff') or {}
        for at in ('start', 'end'):
            if h.get(at) != 'idle0': e.append(f'{c}: hand-off ({at}) {h.get(at)!r} != "idle0" (#sb-gestures-b)')
        if sorted(a['files']) != F5: e.append(f'{c}: dirs {sorted(a["files"])} != {F5}')
        if (a['frame_w'], a['frame_h']) != X.FRAME or tuple(a['anchor']) != X.ANCHOR:
            e.append(f'{c}: frame {a["frame_w"]}x{a["frame_h"]} anchor {a["anchor"]} != {X.FRAME} {list(X.ANCHOR)}')
        for d, steps in (a.get('root_px') or {}).items():
            if any(abs(x) > 1e-9 or abs(y) > 1e-9 for x, y in steps): e.append(f'{c} {d}: root_px moves the panel (a gesture stands still)')
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
            if sorted(pc[c]) != F5: e.append(f'face part {c}: dirs {sorted(pc[c])} != {F5}')
            n = st.J['anims'].get(c, {}).get('frames_n', 0)
            for d in pc[c]:
                ex = (part.get('exact') or {}).get(c, {}).get(d, [])
                if 0 not in ex or n - 1 not in ex: e.append(f'face part {c} {d}: hand-off frames 0 and {n - 1} must be exact')
                for s in ('eyes', 'mouth'):
                    if len(pc[c][d].get(s, [])) != n: e.append(f'face part {c} {d}: {s} positions != {n} frames')
        e += face_check(st)
    return e


def m_mouth_off(s):
    """the brief's stage-specific mutation: a layers frame whose mouth patch sits 1 px off (shrug S, frame 0, an exact
    frame): the drawn face no longer matches the body, verify_face.py must catch it."""
    s.part['clips']['shrug']['S']['mouth'][0][0] += 1
    return 'verify_face.py: '


def m_paw_on_face(s):
    """a white mitten (fur, ink rim) over the eyes of a proud S frame: the outline stays closed, so only the face
    check (verify_face.py on the in-memory frames) can catch it."""
    c, d = 'proud', 'S'; f = s.frames[(c, d)][9]
    ex, ey = s.part['clips'][c][d]['eyes'][9]
    op = f[f[..., 3] > 0][:, :3]; cols, n = np.unique(op, axis=0, return_counts=True)
    fur = cols[n.argmax()]
    ink = f[ey + 4, :, :3][f[ey + 4, :, 3] > 0][0]
    f[ey - 1:ey + 7, ex - 1:ex + 13, :3] = ink; f[ey:ey + 6, ex:ex + 12, :3] = fur
    return 'verify_face.py: '


def m_face_flag(s):
    s.J['anims']['pout']['face'] = 'layers'; return 'pout: face'


def m_root(s):
    a = s.J['anims']['clap']
    for d in a['files']:
        a['root_px'][d][3] = [1.0, 0.0]; a['frames'][3]['root_px'][d] = [1.0, 0.0]
    return 'moves the panel'


MUTATIONS = [('a layers frame with the mouth patch off by 1 px', m_mouth_off),
             ('a mitten moved onto the face of a layers frame', m_paw_on_face),
             ('a baked clip marked layers', m_face_flag),
             ('a gesture that moves the panel', m_root)]

if __name__ == '__main__':
    K.verify_main('x', extra=extra, mutations=MUTATIONS)
