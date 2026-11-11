"""Stage L check (python3 + numpy + Pillow), independent of make_l.py. Exit 0 = PASS, 1 = a violation.

    python3 verify_l.py [ART_DIR]      (default ../out/bear3d)
    python3 verify_l.py --self-test    one hand-off pixel and one outline pixel mutated in memory, each must be caught
                                       (exit 1 = the checker works, exit 2 = it missed one)
From the published files only (anim_l/anim_l.json + strips, anim/anim.json + idle strips):
  - schema (prototype #anim-l): listen_in 3-5 frames one-shot, listen 12-18 loop, listen_out 3-5 one-shot, each for
    exactly S SE E W SW; turn_ccw / turn_cw 3-6 one-shot for all 8 dirs; turn_steps +1 / -1 / 0; frame 84x123
    (= idle's), anchor = idle's, fps = idle's, frames_n = strip width / frame_w = len(frames), in place (root_px 0);
  - hand-offs: listen_in frame 0 and listen_out's last frame = anim/idle_<dir> frame 0; turn frame 0 = idle_<from>
    frame 0, last = idle_<from + turn_steps> frame 0 (direction order S SE E NE N NW W SW), pixel for pixel;
  - pixel rules on every frame: alpha 0/255 only, colours in the pack palette (mascots/bear/pack.json `palettes`),
    a 1 px ink outline (every opaque pixel touching transparency or the frame edge is an ink colour), the sprite
    inside the frame with a 1 px transparent margin.
"""
import sys, os, json
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
argv = sys.argv[1:]
SELF_TEST = '--self-test' in argv
pos = [a for a in argv if not a.startswith('--')]
ART = os.path.abspath(pos[0] if pos else os.path.join(HERE, '..', 'out', 'bear3d'))
DIRS = ['S', 'SE', 'E', 'NE', 'N', 'NW', 'W', 'SW']
FACE_DIRS = ['S', 'SE', 'E', 'W', 'SW']
SPEC = {'listen_in': (3, 5, False, FACE_DIRS, 0), 'listen': (12, 18, True, FACE_DIRS, 0),
        'listen_out': (3, 5, False, FACE_DIRS, 0), 'turn_ccw': (3, 6, False, DIRS, 1), 'turn_cw': (3, 6, False, DIRS, -1)}


def hx(h): return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))


def palette():
    pk = os.path.join(REPO, 'mascots', 'bear', 'pack.json')
    cols, ink = set(), set()
    for p in json.load(open(pk))['palettes']:
        for k, v in json.load(open(os.path.join(os.path.dirname(pk), p))).items():
            if k != 'note' and isinstance(v, list):
                hs = {hx(h) for h in v if isinstance(h, str) and h.startswith('#')}
                cols |= hs
                if k == 'ink': ink |= hs
    return cols, ink


PAL, INK = palette()
A = json.load(open(os.path.join(ART, 'anim', 'anim.json')))
IDLE = A['anims']['idle']
W, H = IDLE['frame_w'], IDLE['frame_h']


def strip(path, w):
    a = np.asarray(Image.open(path).convert('RGBA')).copy()
    return a, [a[:, i * w:(i + 1) * w] for i in range(a.shape[1] // w)]


def load():
    errors = []
    L = json.load(open(os.path.join(ART, 'anim_l', 'anim_l.json')))
    if set(L['anims']) != set(SPEC): errors.append(f'clips {sorted(L["anims"])} != {sorted(SPEC)}')
    if L.get('fps') != A['fps']: errors.append(f'fps {L.get("fps")} != {A["fps"]}')
    fr = {}
    for c, (lo, hi, loop, dirs, ts) in SPEC.items():
        a = L['anims'].get(c)
        if not a: continue
        n = a['frames_n']
        if not lo <= n <= hi: errors.append(f'{c}: {n} frames, not {lo}-{hi}')
        if a['loop'] != loop: errors.append(f'{c}: loop {a["loop"]} != {loop}')
        if a.get('turn_steps') != ts: errors.append(f'{c}: turn_steps {a.get("turn_steps")} != {ts}')
        if (a['frame_w'], a['frame_h']) != (W, H) or a['anchor'] != IDLE['anchor'] or a['fps'] != IDLE['fps']:
            errors.append(f'{c}: frame {a["frame_w"]}x{a["frame_h"]} anchor {a["anchor"]} fps {a["fps"]} != idle\'s')
        if set(a['files']) != set(dirs): errors.append(f'{c}: dirs {sorted(a["files"])} != {sorted(dirs)}')
        if len(a['frames']) != n: errors.append(f'{c}: {len(a["frames"])} frame entries for frames_n {n}')
        for i, f in enumerate(a['frames']):
            if f['root_world'] != 0 or any(v != [0.0, 0.0] for v in f['root_px'].values()) or set(f['root_px']) != set(dirs):
                errors.append(f'{c} frame {i}: not in place / root_px dirs'); break
        for d in a['files']:
            full, frs = strip(os.path.join(ART, 'anim_l', a['files'][d]), W)
            if full.shape[:2] != (H, W * n): errors.append(f'{a["files"][d]}: {full.shape[1]}x{full.shape[0]} != {W * n}x{H}')
            fr[(c, d)] = frs
    rest = {d: strip(os.path.join(ART, 'anim', IDLE['files'][d]), W)[1][0] for d in DIRS}
    return errors, fr, rest


def check(fr, rest):
    errors = []
    for (c, d), frs in sorted(fr.items()):
        ts = SPEC[c][4]
        hand = {'listen_in': [(0, d)], 'listen': [], 'listen_out': [(len(frs) - 1, d)]}.get(
            c, [(0, d), (len(frs) - 1, DIRS[(DIRS.index(d) + ts) % 8])])
        for f, want in hand:
            n = int((frs[f] != rest[want]).any(-1).sum())
            if n: errors.append(f'hand-off: {c} {d} frame {f} != idle_{want} frame 0 ({n} px)')
        for f, a in enumerate(frs):
            lab = f'{c} {d} f{f}'
            al = a[..., 3]
            if not set(np.unique(al)) <= {0, 255}: errors.append(f'{lab}: alpha not 0/255'); continue
            op = al == 255
            bad = set(map(tuple, a[op][:, :3])) - PAL
            if bad: errors.append(f'{lab}: {len(bad)} colours outside the pack palette')
            if op[0].any() or op[-1].any() or op[:, 0].any() or op[:, -1].any(): errors.append(f'{lab}: touches the frame edge')
            p = np.pad(op, 1)
            edge = op & ~(p[:-2, 1:-1] & p[2:, 1:-1] & p[1:-1, :-2] & p[1:-1, 2:])
            nonink = [(int(x), int(y)) for y, x in zip(*np.nonzero(edge)) if tuple(a[y, x, :3]) not in INK]
            if nonink: errors.append(f'{lab}: {len(nonink)} silhouette pixels are not ink (no 1 px outline), e.g. {nonink[0]}')
    return errors


errors, fr, rest = load()
if SELF_TEST:
    if errors or check(fr, rest): print('SELF-TEST needs a valid Stage L first'); sys.exit(2)
    ok = True
    m1 = dict(fr); a = fr[('turn_cw', 'NW')][-1].copy(); ys, xs = np.nonzero(a[..., 3] == 255); k = len(ys) // 2
    a[ys[k], xs[k], :3] = (251, 250, 248) if tuple(a[ys[k], xs[k], :3]) != (251, 250, 248) else (27, 24, 32)
    m1[('turn_cw', 'NW')] = fr[('turn_cw', 'NW')][:-1] + [a]
    e1 = check(m1, rest); want = f'hand-off: turn_cw NW frame {len(fr[("turn_cw", "NW")]) - 1} != idle_N frame 0 (1 px)'
    print('SELF-TEST 1: one pixel of turn_cw NW last frame changed:', 'ok' if e1 == [want] else f'FAILED {e1[:3]}')
    ok &= e1 == [want]
    m2 = dict(fr); a = fr[('listen', 'SE')][5].copy(); op = a[..., 3] == 255
    y = int(np.nonzero(op.any(1))[0][0]); x = int(np.nonzero(op[y])[0][0])   # the top-left silhouette pixel
    a[y, x, :3] = sorted(PAL - INK)[0]; m2[('listen', 'SE')] = fr[('listen', 'SE')][:5] + [a] + fr[('listen', 'SE')][6:]
    e2 = check(m2, rest)
    hit = len(e2) == 1 and e2[0].startswith('listen SE f5: 1 silhouette pixels are not ink')
    print('SELF-TEST 2: one outline pixel of listen SE f5 made non-ink:', 'ok' if hit else f'FAILED {e2[:3]}')
    ok &= hit
    if ok: print('SELF-TEST OK: both mutations caught; exit 1'); sys.exit(1)
    print('SELF-TEST FAILED'); sys.exit(2)
errors += check(fr, rest)
if errors:
    print('verify_l: FAIL'); [print(' ', e) for e in errors[:60]]; sys.exit(1)
print(f'verify_l: PASS ({len(fr)} strips, {sum(len(v) for v in fr.values())} frames; hand-offs identical to idle '
      f'frame 0; alpha, palette and outline ok)')
