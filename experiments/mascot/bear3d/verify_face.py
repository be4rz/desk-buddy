"""Check the D16 face layers against the body renders (python3 + numpy + Pillow). Exit 0 = PASS, 1 = a violation.

    python3 verify_face.py [ART_DIR] [--sheet PNG]   (ART_DIR default ../out/bear3d; --sheet writes the 6 worst
                                                       frames: body | body+default | expected | residual in red)
    python3 verify_face.py --self-test               two in-memory mutations, each must be caught (exit 1 = the
                                                       checker works, exit 2 = it missed or over-reported):
      1. one opaque pixel of the default eyes patch (S) flipped: reported as exactly that patch pixel on the rest
         frames;
      2. 17 spread-out pixels inside the patch masks of one tolerated (non-rest) Stage C frame: the frame is named
         for exceeding the 16 px bound.

Invariant "T" (amends the wording of design §3.4; agreed with the lead for this art round). Draw the default patches
(eyes open, mouth closed) over the body frame at that frame's positions = "drawn". The expected frame is the anim_c
body itself for the Stage C clips (think talk ask oops happy celebrate), and for idle the Blender reference
face/ref/idle_<dir>.png: the same idle poses with the eyes and mouth carried rigidly and no baked blink, since the
shipped idle re-rasterises its eyes while it breathes and bakes one blink (Stage A bytes stay as they are).
  - EXACT (drawn == expected, and == the body itself for idle) on idle's rest frames (pose == frame 0) and on every
    Stage C frame whose pose leaves arms and squash at rest, which includes every hand-off frame.
  - TOLERATED on every other covered frame: drawn may differ from expected by at most 16 px in the frame, only
    inside the placed patch masks grown by 1 px, only between ink / fur / blush palette colours (or ink against
    transparent at the silhouette edge), and never as an 8-connected cluster of 5+ px (the size of an eye or mouth
    feature, so a paw crossing the face or a missing / extra eye fails). Why: the eyes and mouth are rigid, but their
    1 px ink outline and the fur shade ring around them come from the surrounding scene, so a paw near the face or
    the idle breath flips a few outline / shade pixels.
  - walk*/run* (head roll / lean), walk_64, N/NE/NW and the Stage B sets have no face entries.
Also: face_version 1, dirs S SE E W SW, slots eyes [open closed] / mouth [closed open wide]; every patch and reference
is alpha 0/255 in pack palette colours (mascots/bear/pack.json `palettes`); all states of one slot+dir share size and
opaque mask; one position per body frame, patch inside the frame; the covered clips are exactly idle + the six Stage C
clips in every face dir; Stage C hand-off frames (frame 0, and the last frame of a one-shot) equal anim/idle frame 0
in all 8 directions. Prints a residual table per clip and dir (tolerated frames: those with any residual, max px/frame, total).
"""
import sys, os, json
from collections import defaultdict
import numpy as np
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
argv = sys.argv[1:]
SELF_TEST = '--self-test' in argv
SHEET = argv[argv.index('--sheet') + 1] if '--sheet' in argv else None
pos_args = [a for i, a in enumerate(argv) if not a.startswith('--') and (i == 0 or argv[i - 1] != '--sheet')]
ART = os.path.abspath(pos_args[0] if pos_args else os.path.join(HERE, '..', 'out', 'bear3d'))
DIRS8 = ['S', 'SE', 'E', 'NE', 'N', 'NW', 'W', 'SW']
FACE_DIRS = ['S', 'SE', 'E', 'W', 'SW']
SLOTS = {'eyes': (['open', 'closed'], 'open'), 'mouth': (['closed', 'open', 'wide'], 'closed')}
C_CLIPS = ['think', 'talk', 'ask', 'oops', 'happy', 'celebrate']
REST_KEYS = ['armL', 'armR', 'spL', 'spR', 'sz']      # a Stage C frame with these at rest must match exactly
MAX_PX, MAX_CLUSTER = 16, 4


def hx(h): return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))


def palette():
    pk = os.path.join(REPO, 'mascots', 'bear', 'pack.json')
    cols, ramps = set(), defaultdict(set)
    for p in json.load(open(pk))['palettes']:
        for k, v in json.load(open(os.path.join(os.path.dirname(pk), p))).items():
            if k != 'note' and isinstance(v, list):
                for h in v:
                    if isinstance(h, str) and h.startswith('#'): cols.add(hx(h)); ramps[k].add(hx(h))
    return cols, ramps


PAL, RAMPS = palette()
SOFT = RAMPS['ink'] | RAMPS['fur'] | RAMPS['blush']   # colours an outline / shade flip may swap between
INK = RAMPS['ink']


def load(path): return np.asarray(Image.open(path).convert('RGBA')).copy()


def frames(path, w):
    a = load(path); return [a[:, i * w:(i + 1) * w].copy() for i in range(a.shape[1] // w)]


def grow(m):
    g = m.copy(); g[1:] |= m[:-1]; g[:-1] |= m[1:]; g[:, 1:] |= m[:, :-1]; g[:, :-1] |= m[:, 1:]; return g


def clusters(m):
    """sizes of the 8-connected components of a bool mask"""
    seen, out = np.zeros_like(m), []
    for y, x in zip(*np.nonzero(m)):
        if seen[y, x]: continue
        st, n = [(y, x)], 0; seen[y, x] = True
        while st:
            a, b = st.pop(); n += 1
            for c in range(max(a - 1, 0), min(a + 2, m.shape[0])):
                for d in range(max(b - 1, 0), min(b + 2, m.shape[1])):
                    if m[c, d] and not seen[c, d]: seen[c, d] = True; st.append((c, d))
        out.append(n)
    return out


A = json.load(open(os.path.join(ART, 'anim', 'anim.json')))['anims']
AC = json.load(open(os.path.join(ART, 'anim_c', 'anim_c.json')))['anims']
F = json.load(open(os.path.join(ART, 'face', 'face.json')))
IDLE_POSES = json.load(open(os.path.join(HERE, 'clips.json')))['clips']['idle']['poses']
C_POSES = {c: v['poses'] for c, v in json.load(open(os.path.join(HERE, 'clips_c.json')))['clips'].items() if c in C_CLIPS}
COVER = {'idle'} | set(C_CLIPS)


def exact_frame(c, f):
    if c == 'idle': return IDLE_POSES[f] == IDLE_POSES[0]
    p, r = C_POSES[c][f], C_POSES[c][0]
    return all(abs(p.get(k, 0) - r.get(k, 0)) < 1e-9 for k in REST_KEYS)


def pixel_rules(a, what, errors):
    al = a[..., 3]
    if not set(np.unique(al)) <= {0, 255}: errors.append(f'{what}: alpha not 0/255')
    bad = set(map(tuple, a[al == 255][:, :3])) - PAL
    if bad: errors.append(f'{what}: {len(bad)} colours outside the pack palette, e.g. '
                          + ', '.join('#%02x%02x%02x' % c for c in list(bad)[:3]))


def load_all():
    """structure checks + the patches and body/expected frames (in memory, so the self-test can mutate them)"""
    errors = []
    if F.get('face_version') != 1: errors.append(f'face_version {F.get("face_version")} != 1')
    if F.get('dirs') != FACE_DIRS: errors.append(f'dirs {F.get("dirs")} != {FACE_DIRS}')
    for s, (states, dflt) in SLOTS.items():
        sl = F['slots'].get(s, {})
        if sl.get('states') != states or sl.get('default') != dflt: errors.append(f'slot {s}: {sl} != {states}, default {dflt}')
    if set(F['clips']) != COVER: errors.append(f'covered clips {sorted(F["clips"])} != {sorted(COVER)} (walk/run have no face)')
    patch = {}
    for s, (states, _) in SLOTS.items():
        for d in FACE_DIRS:
            mask = None
            for st in states:
                l = F['layers'].get(s, {}).get(st, {}).get(d)
                if not l: errors.append(f'layer {s}/{st}/{d} missing'); continue
                a = load(os.path.join(ART, 'face', l['file']))
                if a.shape[:2] != (l['h'], l['w']): errors.append(f'{l["file"]}: size {a.shape[1]}x{a.shape[0]} != {l["w"]}x{l["h"]}')
                pixel_rules(a, l['file'], errors)
                m = a[..., 3] == 255
                if mask is None: mask = m
                elif m.shape != mask.shape or (m != mask).any(): errors.append(f'{s}/{d}: state {st} has another size or opaque mask')
                patch[(s, st, d)] = a
    body, ref = {}, {}
    for c in sorted(COVER & set(F['clips'])):
        meta = A[c] if c == 'idle' else AC[c]
        if set(F['clips'][c]) != set(FACE_DIRS): errors.append(f'{c}: face dirs {sorted(F["clips"][c])} != {FACE_DIRS}')
        for d in FACE_DIRS:
            if d not in F['clips'][c]: continue
            for s in SLOTS:
                n = len(F['clips'][c][d].get(s, []))
                if n != meta['frames_n']: errors.append(f'{c} {d} {s}: {n} positions for {meta["frames_n"]} frames')
            body[(c, d)] = frames(os.path.join(ART, 'anim' if c == 'idle' else 'anim_c', meta['files'][d]), meta['frame_w'])
            if c == 'idle':
                ref[d] = frames(os.path.join(ART, 'face', 'ref', f'idle_{d}.png'), meta['frame_w'])
                pixel_rules(np.concatenate(ref[d], 1), f'ref/idle_{d}.png', errors)
                if len(ref[d]) != len(body[(c, d)]): errors.append(f'ref/idle_{d}.png: {len(ref[d])} frames for {len(body[(c, d)])}')
    W = A['idle']['frame_w']
    for d in DIRS8:   # Stage C hand-offs
        rest = frames(os.path.join(ART, 'anim', A['idle']['files'][d]), W)[0]
        for c in C_CLIPS:
            fr = frames(os.path.join(ART, 'anim_c', AC[c]['files'][d]), W)
            for f in ([0] if AC[c]['loop'] else [0, len(fr) - 1]):
                if not np.array_equal(fr[f], rest): errors.append(f'hand-off: anim_c {c} {d} frame {f} != anim/idle frame 0')
    return errors, patch, body, ref


def check(patch, body, ref):
    """the invariant: (exact-mismatch pixels {key: [frames]}, tolerance errors [(msg, keys)], residuals, worst)"""
    exact, tol, resid, worst = defaultdict(list), [], defaultdict(list), []
    for (c, d), frs in sorted(body.items()):
        for f, b in enumerate(frs):
            lab = f'{c} {d} f{f}'
            drawn, owner = b.copy(), {}
            H, W = b.shape[:2]; inm = np.zeros((H, W), bool)
            for s, (_, dflt) in SLOTS.items():
                x, y = F['clips'][c][d][s][f]; p = patch[(s, dflt, d)]; h, w = p.shape[:2]
                if x < 0 or y < 0 or x + w > W or y + h > H: tol.append((f'{lab}: {s} patch at ({x},{y}) leaves the frame', set())); continue
                m = p[..., 3] == 255
                drawn[y:y + h, x:x + w][m] = p[m]; inm[y:y + h, x:x + w] |= m
                for py, px in zip(*np.nonzero(m)): owner[(y + py, x + px)] = f'{s}/{dflt}/{d} patch pixel ({px},{py})'
            exp = ref[d][f] if c == 'idle' else b

            def key(y, x): return owner.get((y, x), f'{c} {d} body pixel ({x},{y}) outside the patches')
            dm = (drawn != exp).any(-1)
            if exact_frame(c, f):
                for want, tag in ((exp, ''), (b, ' (vs body)') if c == 'idle' else (None, None)):
                    if want is None: continue
                    for y, x in zip(*np.nonzero((drawn != want).any(-1))): exact[key(y, x)].append(lab + tag)
                continue
            n = int(dm.sum()); resid[(c, d)].append(n)
            if n: worst.append((n, c, d, f, b, drawn, exp))
            keys = {key(y, x) for y, x in zip(*np.nonzero(dm))}
            if n > MAX_PX: tol.append((f'{lab}: {n} px differ from the expected frame (> {MAX_PX})', keys))
            out = dm & ~grow(inm)
            if out.any(): tol.append((f'{lab}: {int(out.sum())} px differ outside the patch masks + 1 px', keys))
            for y, x in zip(*np.nonzero(dm)):
                a, e = drawn[y, x], exp[y, x]
                ok = (a[3] == 255 and e[3] == 255 and tuple(a[:3]) in SOFT and tuple(e[:3]) in SOFT) or \
                     (a[3] != e[3] and tuple((a if a[3] else e)[:3]) in INK)
                if not ok:
                    tol.append((f'{lab}: {key(y, x)} is #%02x%02x%02x/%d, expected #%02x%02x%02x/%d (not an ink/fur/blush flip)'
                                % (*a[:3], a[3], *e[:3], e[3]), {key(y, x)})); break
            big = [k for k in clusters(dm) if k > MAX_CLUSTER]
            if big: tol.append((f'{lab}: a cluster of {max(big)} px differs (an eye/mouth-sized change)', keys))
    return exact, tol, resid, worst


def report(errors, exact, tol):
    out = list(errors)
    for k, v in sorted(exact.items()): out.append(f'MISMATCH (exact frame) {k}: {len(v)} frame(s), e.g. ' + ', '.join(v[:3]))
    out += [m for m, _ in tol]
    return out


def table(resid):
    print('residuals on tolerated frames (bound: <= %d px/frame, inside masks + 1 px, ink/fur/blush, clusters <= %d px)'
          % (MAX_PX, MAX_CLUSTER))
    print('  %-10s %-3s %9s %8s %7s %6s' % ('clip', 'dir', 'tolerated', 'w/ resid', 'max px', 'total'))
    for (c, d), v in sorted(resid.items(), key=lambda kv: (['idle'] + C_CLIPS).index(kv[0][0]) * 9 + FACE_DIRS.index(kv[0][1])):
        print('  %-10s %-3s %9d %8d %7d %6d' % (c, d, len(v), sum(1 for n in v if n), max(v, default=0), sum(v)))


def sheet(worst, path):
    worst = sorted(worst, key=lambda t: -t[0])[:6]
    K, lab, BG = 4, 16, (200, 196, 210, 255)
    if not worst: return
    W, H = worst[0][4].shape[1], worst[0][4].shape[0]
    im = Image.new('RGBA', (W * K * 4, (H * K + lab) * len(worst)), BG); dr = ImageDraw.Draw(im)
    for r, (n, c, d, f, b, drawn, exp) in enumerate(worst):
        hl = drawn.copy(); hl[(drawn != exp).any(-1)] = (230, 30, 60, 255)
        y0 = r * (H * K + lab)
        for i, (t, a) in enumerate((('body', b), ('body+default', drawn), ('expected', exp), (f'residual {n} px', hl))):
            dr.text((i * W * K + 4, y0 + 2), f'{c} {d} f{f}: {t}', fill=(40, 36, 48, 255))
            tile = Image.new('RGBA', (W, H), BG); tile.alpha_composite(Image.fromarray(a))
            im.paste(tile.resize((W * K, H * K), Image.NEAREST), (i * W * K, y0 + lab))
    im.save(path); print('wrote', path)


errors, patch, body, ref = load_all()
if SELF_TEST:
    if errors: print('SELF-TEST needs a valid face set first:', *errors[:10], sep='\n  '); sys.exit(2)
    ok = True
    # case 1: one default-patch pixel on the rest frames
    p = patch[('eyes', 'open', 'S')].copy(); ys, xs = np.nonzero(p[..., 3] == 255); k = len(ys) // 2
    mx, my = int(xs[k]), int(ys[k]); old = tuple(p[my, mx, :3])
    p[my, mx, :3] = hx('#1b1820') if old != hx('#1b1820') else hx('#fbfaf8')
    want = f'eyes/open/S patch pixel ({mx},{my})'
    base = check(patch, body, ref)
    ex, tl, _, _ = check({**patch, ('eyes', 'open', 'S'): p}, body, ref)
    new_tol = [m for m, ks in tl if m not in {b for b, _ in base[1]}]
    stray = [m for m, ks in tl if m in new_tol and want not in ks]
    print(f'SELF-TEST 1: eyes/open/S patch pixel ({mx},{my}) flipped in memory')
    if set(ex) == {want} and not stray: print(f'  ok: reported exactly {want} on {len(ex[want])} exact frames')
    else: ok = False; print('  FAILED: exact-frame report', sorted(ex), 'stray', stray[:5])
    # case 2: 17 spread-out in-mask pixels on one tolerated Stage C frame, swapped between ink and fur only
    c, d = 'happy', 'S'; f = next(f for f in range(len(body[(c, d)])) if not exact_frame(c, f))
    b = body[(c, d)][f].copy(); n = 0
    for s, (_, dflt) in SLOTS.items():
        x, y = F['clips'][c][d][s][f]; m = patch[(s, dflt, d)][..., 3] == 255
        for py, px in zip(*np.nonzero(m)):
            if n < 17 and py % 2 == 0 and px % 2 == 0:
                q = b[y + py, x + px]
                q[:3] = sorted(RAMPS['fur'])[0] if tuple(q[:3]) in INK else sorted(INK)[0]; n += 1
    mut = dict(body); mut[(c, d)] = list(body[(c, d)]); mut[(c, d)][f] = b
    _, tl, _, _ = check(patch, mut, ref)
    lab = f'{c} {d} f{f}'
    hits = [m for m, _ in tl if m.startswith(lab + ':') and m not in {b for b, _ in base[1]}]
    print(f'SELF-TEST 2: {n} spread-out pixels inside the patch masks of {lab} changed in memory')
    if n == 17 and any(f'(> {MAX_PX})' in m for m in hits): print('  ok: ' + '; '.join(hits))
    else: ok = False; print('  FAILED: expected the frame to be named for > 16 px, got', hits)
    if ok: print('SELF-TEST OK: both mutations caught; exit 1'); sys.exit(1)
    print('SELF-TEST FAILED'); sys.exit(2)

exact, tol, resid, worst = check(patch, body, ref)
table(resid)
if SHEET: sheet(worst, SHEET)
msgs = report(errors, exact, tol)
nf = sum(len(v) for v in body.values())
if msgs:
    print('verify_face: FAIL'); [print(' ', e) for e in msgs[:60]]
    if len(msgs) > 60: print(f'  ... {len(msgs) - 60} more')
    sys.exit(1)
ne = sum(exact_frame(c, f) for (c, d), v in body.items() for f in range(len(v)))
print(f'verify_face: PASS ({len(F["clips"])} clips x {len(FACE_DIRS)} dirs, {nf} frames: {ne} exact, {nf - ne} within '
      f'the bound; hand-offs ok)')
