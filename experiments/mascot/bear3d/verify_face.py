"""Check the D16 face layers against the body renders (python3 + numpy + Pillow). Exit 0 = PASS, 1 = a violation.

    python3 verify_face.py [ART_DIR] [--sheet PNG]   (ART_DIR default ../out/bear3d; --sheet writes the 6 worst
                                                       frames: body | body+default | expected | residual in red)
    python3 verify_face.py --self-test               in-memory mutations, each must be caught (exit 1 = the
                                                       checker works, exit 2 = it missed or over-reported):
      1. one opaque pixel of the default eyes patch (S) flipped: reported as exactly that patch pixel on the rest
         frames;
      2. 17 spread-out pixels inside the patch masks of one tolerated (non-rest) Stage C frame: the frame is named
         for exceeding the 16 px bound;
      3. (face.json v2) one body pixel under the S eyes patch on a held listen SE frame (rest pose, head turned to
         S): reported as exactly that patch pixel on exactly that frame;
      4. (set_clips) one body pixel under the S eyes patch on an exact desk_talk B frame (seated, body SW, head turned
         to S): reported as exactly that patch pixel on exactly that frame.

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
  - listen (Stage L, face.json v2): drawn with the patches of patch_dirs.listen[dir] (the head is turned 45 deg on its
    held frames: SE->S, SW->S, E->SE, W->SW, S->S); the expected frame is the anim_l body; EXACT on the frames whose
    pose has arms, squash and the head bob at rest (every held frame but the 1 px breath), TOLERATED on the others.
    listen_in / listen_out (baked face) and the turns (no face) have no entries.
  - set_clips (Stage D, face.json v2, additive): desk_talk and desk_ask in ori A and B, positions in the anim_d set
    canvas, patches of set_clips[clip][ori].patch (S: the body faces SE / SW with the head turned 45 deg); the expected
    frame is the anim_d frame; EXACT on the frames whose pose has arms, squash, bob, head bob and head yaw as in the
    clip's frame 0, TOLERATED on the others. Required once anim_d/ exists.
  - walk*/run* (head roll / lean), walk_64, N/NE/NW and the Stage B sets have no face entries.
Also: face_version 1 (or 2 = v1 + clips.listen + patch_dirs.listen exactly as above), dirs S SE E W SW, slots eyes [open closed] / mouth [closed open wide]; every patch and reference
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
V2 = F.get('face_version') == 2
SET = F.get('set_clips', {})                         # Stage D (face.json v2): clip -> ori -> {patch, eyes, mouth}
HAS_D = os.path.exists(os.path.join(ART, 'anim_d', 'anim_d.json'))
ORIS = ['A', 'B']
if SET or HAS_D:
    sys.path.insert(0, HERE)
    import clips_d
    AD = json.load(open(os.path.join(ART, 'anim_d', 'anim_d.json'))) if HAS_D else {'anims': {}, 'sets': {}}
    _g = dict(PXZ=json.load(open(os.path.join(HERE, 'clips.json')))['PXZ'], hand=(0.0, 0.0), seat=(0.0, 0.0), seatZ=0.0)
    D_POSES = {c: [q for q, _ in fr] for c, fr in clips_d.build(_g).items() if c in clips_d.FACE_CLIPS}
if V2:   # Stage L: listen, with the patch dir following the turned head
    sys.path.insert(0, HERE)
    from anim_clips import LISTEN_DIR_VARIANT, LISTEN_PATCH_DIR
    AL = json.load(open(os.path.join(ART, 'anim_l', 'anim_l.json')))['anims']
    L_POSES = {d: json.load(open(os.path.join(HERE, 'clips_l.json')))['clips'][f'listen_{LISTEN_DIR_VARIANT[d]}']['poses']
               for d in FACE_DIRS}
    COVER |= {'listen'}


def patch_dir(c, d):
    if c in SET: return SET[c][d]['patch']
    return F.get('patch_dirs', {}).get(c, {}).get(d, d)


def positions(c, d): return SET[c][d] if c in SET else F['clips'][c][d]


def exact_frame(c, f, d=None):
    if c in SET:
        p, r = D_POSES[c][f], D_POSES[c][0]
        return all(abs(p.get(k, 0) - r.get(k, 0)) < 1e-9 for k in REST_KEYS + ['hb', 'bob', 'hy'])
    if c == 'listen':
        p, r = L_POSES[d][f], L_POSES[d][0]
        return all(abs(p.get(k, 0) - r.get(k, 0)) < 1e-9 for k in REST_KEYS + ['hb', 'bob', 'hy'])
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
    if F.get('face_version') not in (1, 2): errors.append(f'face_version {F.get("face_version")} is not 1 or 2')
    if V2 and F.get('patch_dirs') != {'listen': LISTEN_PATCH_DIR}:
        errors.append(f'patch_dirs {F.get("patch_dirs")} != {{"listen": {LISTEN_PATCH_DIR}}}')
    if not V2 and 'patch_dirs' in F: errors.append('patch_dirs in a v1 face.json')
    if not V2 and os.path.exists(os.path.join(ART, 'anim_l', 'anim_l.json')):
        errors.append('face.json is v1 but anim_l/ exists (run_face.sh / run_c.sh rewrite it): run python3 face_l.py')
    if (SET or HAS_D) and not V2: errors.append('set_clips need face.json v2 (run python3 face_l.py, then face_d.py)')
    if HAS_D and set(SET) != set(clips_d.FACE_CLIPS):
        errors.append(f'set_clips {sorted(SET)} != {sorted(clips_d.FACE_CLIPS)} (anim_d/ exists: run python3 face_d.py)')
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
        meta, sub = (A[c], 'anim') if c == 'idle' else (AL[c], 'anim_l') if c == 'listen' else (AC[c], 'anim_c')
        if set(F['clips'][c]) != set(FACE_DIRS): errors.append(f'{c}: face dirs {sorted(F["clips"][c])} != {FACE_DIRS}')
        for d in FACE_DIRS:
            if d not in F['clips'][c]: continue
            for s in SLOTS:
                n = len(F['clips'][c][d].get(s, []))
                if n != meta['frames_n']: errors.append(f'{c} {d} {s}: {n} positions for {meta["frames_n"]} frames')
            body[(c, d)] = frames(os.path.join(ART, sub, meta['files'][d]), meta['frame_w'])
            if c == 'idle':
                ref[d] = frames(os.path.join(ART, 'face', 'ref', f'idle_{d}.png'), meta['frame_w'])
                pixel_rules(np.concatenate(ref[d], 1), f'ref/idle_{d}.png', errors)
                if len(ref[d]) != len(body[(c, d)]): errors.append(f'ref/idle_{d}.png: {len(ref[d])} frames for {len(body[(c, d)])}')
    for c in sorted(SET):
        if c not in clips_d.FACE_CLIPS: errors.append(f'set_clips {c}: not a Stage D face clip {clips_d.FACE_CLIPS}'); continue
        if c not in AD['anims']: errors.append(f'set_clips {c}: no such clip in anim_d/anim_d.json'); continue
        meta = AD['anims'][c]
        if sorted(SET[c]) != ORIS: errors.append(f'set_clips {c}: oris {sorted(SET[c])} != {ORIS}')
        for o in ORIS:
            if o not in SET[c]: continue
            if SET[c][o].get('patch') not in FACE_DIRS: errors.append(f'set_clips {c} {o}: patch {SET[c][o].get("patch")}'); continue
            if SET[c][o]['patch'] != meta.get('face_patch_dir'):
                errors.append(f'set_clips {c} {o}: patch {SET[c][o]["patch"]} != anim_d face_patch_dir {meta.get("face_patch_dir")}')
            for s in SLOTS:
                n = len(SET[c][o].get(s, []))
                if n != meta['frames_n']: errors.append(f'set_clips {c} {o} {s}: {n} positions for {meta["frames_n"]} frames')
            body[(c, o)] = frames(os.path.join(ART, 'anim_d', meta['files'][o]), AD['sets'][meta['set']][o]['frame_w'])
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
                pd = patch_dir(c, d)
                x, y = positions(c, d)[s][f]; p = patch[(s, dflt, pd)]; h, w = p.shape[:2]
                if x < 0 or y < 0 or x + w > W or y + h > H: tol.append((f'{lab}: {s} patch at ({x},{y}) leaves the frame', set())); continue
                m = p[..., 3] == 255
                drawn[y:y + h, x:x + w][m] = p[m]; inm[y:y + h, x:x + w] |= m
                for py, px in zip(*np.nonzero(m)): owner[(y + py, x + px)] = f'{s}/{dflt}/{pd} patch pixel ({px},{py})'
            exp = ref[d][f] if c == 'idle' else b

            def key(y, x): return owner.get((y, x), f'{c} {d} body pixel ({x},{y}) outside the patches')
            dm = (drawn != exp).any(-1)
            if exact_frame(c, f, d):
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
    order = ['idle'] + C_CLIPS + ['listen'] + sorted(SET)
    for (c, d), v in sorted(resid.items(), key=lambda kv: order.index(kv[0][0]) * 9 + (FACE_DIRS + ORIS).index(kv[0][1])):
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
    c, d = 'happy', 'S'; f = next(f for f in range(len(body[(c, d)])) if not exact_frame(c, f, d))
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
    # case 3 (v2): one body pixel under the S eyes patch on a held listen SE frame
    if V2:
        c, d = 'listen', 'SE'; f = next(f for f in range(len(body[(c, d)])) if exact_frame(c, f, d) and f > 0)
        x, y = F['clips'][c][d]['eyes'][f]; m = patch[('eyes', 'open', 'S')][..., 3] == 255
        ys, xs = np.nonzero(m); k = len(ys) // 3; py, px = int(ys[k]), int(xs[k])
        b = body[(c, d)][f].copy(); q = b[y + py, x + px]
        q[:3] = hx('#fbfaf8') if tuple(q[:3]) != hx('#fbfaf8') else hx('#1b1820')
        mut = dict(body); mut[(c, d)] = list(body[(c, d)]); mut[(c, d)][f] = b
        ex, tl, _, _ = check(patch, mut, ref)
        want = f'eyes/open/S patch pixel ({px},{py})'
        new_tol = [m for m, _ in tl if m not in {b for b, _ in base[1]}]
        print(f'SELF-TEST 3: body pixel under {want} on {c} {d} f{f} changed in memory')
        if set(ex) == {want} and ex[want] == [f'{c} {d} f{f}'] and not new_tol: print(f'  ok: reported exactly {want} on {c} {d} f{f}')
        else: ok = False; print('  FAILED: exact-frame report', dict(ex), 'new tolerance', new_tol[:5])
    # case 4 (set_clips): one body pixel under the S eyes patch on an exact desk_talk B frame
    if 'desk_talk' in SET:
        c, d = 'desk_talk', 'B'; f = next((f for f in range(len(body[(c, d)])) if exact_frame(c, f, d) and f > 0), None)
    if 'desk_talk' in SET and f is None: ok = False; print('SELF-TEST 4: FAILED: desk_talk B has no exact frame after f0')
    elif 'desk_talk' in SET:
        pd = patch_dir(c, d); x, y = positions(c, d)['eyes'][f]; m = patch[('eyes', 'open', pd)][..., 3] == 255
        ys, xs = np.nonzero(m); k = len(ys) // 3; py, px = int(ys[k]), int(xs[k])
        b = body[(c, d)][f].copy(); q = b[y + py, x + px]
        q[:3] = hx('#fbfaf8') if tuple(q[:3]) != hx('#fbfaf8') else hx('#1b1820')
        mut = dict(body); mut[(c, d)] = list(body[(c, d)]); mut[(c, d)][f] = b
        ex, tl, _, _ = check(patch, mut, ref)
        want = f'eyes/open/{pd} patch pixel ({px},{py})'
        new_tol = [m for m, _ in tl if m not in {b for b, _ in base[1]}]
        print(f'SELF-TEST 4: body pixel under {want} on {c} {d} f{f} changed in memory')
        if set(ex) == {want} and ex[want] == [f'{c} {d} f{f}'] and not new_tol: print(f'  ok: reported exactly {want} on {c} {d} f{f}')
        else: ok = False; print('  FAILED: exact-frame report', dict(ex), 'new tolerance', new_tol[:5])
    elif HAS_D: ok = False; print('SELF-TEST 4: anim_d/ exists but face.json has no set_clips.desk_talk')
    n_cases = 2 + V2 + ('desk_talk' in SET)
    if ok: print(f'SELF-TEST OK: all {n_cases} mutations caught; exit 1'); sys.exit(1)
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
ne = sum(exact_frame(c, f, d) for (c, d), v in body.items() for f in range(len(v)))
print(f'verify_face: PASS ({len(F["clips"])} clips x {len(FACE_DIRS)} dirs + {len(SET)} set clips x {len(ORIS)} oris, {nf} frames: {ne} exact, {nf - ne} within '
      f'the bound; hand-offs ok)')
