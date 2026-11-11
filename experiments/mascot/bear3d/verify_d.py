"""Check the Stage D desk clips (python3 + numpy + Pillow). Exit 0 = PASS, 1 = a violation.

    python3 verify_d.py [ART_DIR]       (default ../out/bear3d: anim_d/, props_d/, anim/)
    python3 verify_d.py --self-test     in-memory mutations, each must be caught (exit 1 = the checker works,
                                        exit 2 = it missed one):
      1. one pixel of desk_stand B's last frame (a hand-off frame);
      2. one pixel of desk_type A frame 0 (breaks the desk_sit -> desk_type join and five others);
      3. a colour outside the palette in desk_ask B;
      4. the desk_talk A loop seam (its last frame replaced by a desk_sit frame).

Checks (verify_b.py's method, plus every join desk-brain relies on):
  - anim_d.json: the 7 clips with the frozen frame counts (clips_d.COUNTS) and loop flags, ori A and B strips of
    frames_n frames on the set canvas (<= 176x160), dur 1000/fps; desk_talk / desk_ask name face_patch_dir.
  - hand-offs: desk_sit frame 0 and desk_stand's last frame = the props-only frame + the Stage A idle sprite (frame 0)
    at handoff.px from the anchor, facing handoff.facing (SE in ori A, SW in ori B, as clips_d.HAND_YAW says).
  - joins (identical frames): desk_sit last = desk_type 0; desk_swivel_in 0 = desk_type 0; desk_swivel_in last =
    desk_talk 0; desk_ask 0 = desk_talk 0; desk_swivel_out 0 = desk_talk 0; desk_swivel_out last = desk_type 0;
    desk_stand 0 = desk_talk 0.
  - loop seams (desk_type, desk_talk, desk_ask): last -> frame 0, and the step into the last frame, change no more
    pixels than the largest step before the last frame.
  - pixels: alpha 0/255, palette (bear + moss + palette-ext.json), a 1 px ink outline (every opaque pixel next to a
    transparent one is ink), in every strip and prop sprite.
  - props_d.json: desk_table, chair, computer in A and B; files exist with the stated size; desk_<ori>.png is the
    canvas size.
"""
import sys, os, json
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import clips_d
import pixelize_d as PXD
from pixelize import INK

argv = sys.argv[1:]
SELF_TEST = '--self-test' in argv
pos = [a for a in argv if not a.startswith('--')]
ART = os.path.abspath(pos[0] if pos else os.path.join(HERE, '..', 'out', 'bear3d'))
AD_DIR, PD_DIR, A_DIR = (os.path.join(ART, x) for x in ('anim_d', 'props_d', 'anim'))
ORIS = ['A', 'B']
JOINS = [('desk_sit', -1, 'desk_type', 0), ('desk_swivel_in', 0, 'desk_type', 0), ('desk_swivel_in', -1, 'desk_talk', 0),
         ('desk_ask', 0, 'desk_talk', 0), ('desk_swivel_out', 0, 'desk_talk', 0), ('desk_swivel_out', -1, 'desk_type', 0),
         ('desk_stand', 0, 'desk_talk', 0)]
MAX_W, MAX_H = 176, 160


def load(p): return np.asarray(Image.open(p).convert('RGBA')).copy()


AJ = json.load(open(os.path.join(A_DIR, 'anim.json')))
A_ANCHOR = AJ['anims']['idle']['anchor']; A_FW = AJ['anims']['idle']['frame_w']
YAW2DIR = {v['yaw_deg'] % 360: d for d, v in AJ['directions'].items()}
J = json.load(open(os.path.join(AD_DIR, 'anim_d.json')))
PJ = json.load(open(os.path.join(PD_DIR, 'props_d.json')))


def paste_over(base, spr, x0, y0):
    out = base.copy(); H, W = out.shape[:2]; h, w = spr.shape[:2]
    xs, ys = max(0, -x0), max(0, -y0); xe, ye = min(w, W - x0), min(h, H - y0)
    s = spr[ys:ye, xs:xe]; m = s[..., 3] > 0
    reg = out[y0 + ys:y0 + ye, x0 + xs:x0 + xe]; reg[m] = s[m]
    return out


def pixel_errors(a, what):
    e = []; al = a[..., 3]
    if not set(np.unique(al)) <= {0, 255}: e.append(f'{what}: alpha not 0/255')
    bad = set(map(tuple, a[al == 255][:, :3])) - PXD.ALLOWED
    if bad: e.append(f'{what}: {len(bad)} colours outside the palette, e.g. ' + ', '.join('#%02x%02x%02x' % c for c in list(bad)[:3]))
    op = al == 255; pad = np.pad(op, 1)
    edge = op & ~(pad[:-2, 1:-1] & pad[2:, 1:-1] & pad[1:-1, :-2] & pad[1:-1, 2:])
    notink = edge & np.any(a[..., :3] != np.array(INK, np.uint8), -1)
    if notink.any(): e.append(f'{what}: {int(notink.sum())} silhouette pixels are not ink (1 px outline)')
    return e


def load_all():
    errors, FR, PF = [], {}, {}
    if list(J['anims']) != clips_d.ORDER: errors.append(f'anim_d clips {list(J["anims"])} != {clips_d.ORDER}')
    st = J['sets'].get('desk', {})
    if sorted(st) != ORIS: errors.append(f'sets.desk oris {sorted(st)} != {ORIS}')
    for o in ORIS:
        so = st[o]; W, H = so['frame_w'], so['frame_h']
        if W > MAX_W or H > MAX_H: errors.append(f'canvas {o} {W}x{H} exceeds {MAX_W}x{MAX_H}')
        PF[o] = load(os.path.normpath(os.path.join(AD_DIR, so['props_frame'])))
        if PF[o].shape[:2] != (H, W): errors.append(f'{so["props_frame"]}: size != the {W}x{H} canvas')
        errors += pixel_errors(PF[o], so['props_frame'])
    for c in clips_d.ORDER:
        a = J['anims'].get(c)
        if not a: errors.append(f'{c}: missing'); continue
        if a['frames_n'] != clips_d.COUNTS[c]: errors.append(f'{c}: {a["frames_n"]} frames != frozen {clips_d.COUNTS[c]}')
        if a['loop'] != (c in clips_d.LOOP): errors.append(f'{c}: loop {a["loop"]}')
        if len(a['frames']) != a['frames_n'] or any(f['dur'] != round(1000 / a['fps']) for f in a['frames']):
            errors.append(f'{c}: frames / dur do not match frames_n at {a["fps"]} fps')
        if (c in clips_d.FACE_CLIPS) != ('face_patch_dir' in a) or a.get('face_patch_dir', clips_d.PATCH_DIR) != clips_d.PATCH_DIR:
            errors.append(f'{c}: face_patch_dir {a.get("face_patch_dir")}')
        for o in ORIS:
            so = st[o]; W = so['frame_w']; s = load(os.path.join(AD_DIR, a['files'][o]))
            if s.shape[0] != so['frame_h'] or s.shape[1] != W * a['frames_n']:
                errors.append(f'{a["files"][o]}: {s.shape[1]}x{s.shape[0]} != {a["frames_n"]} x {W}x{so["frame_h"]}')
            FR[(c, o)] = [s[:, i * W:(i + 1) * W].copy() for i in range(s.shape[1] // W)]
    for name in ('desk_table', 'chair', 'computer'):
        p = PJ['props'].get(name)
        if not p: errors.append(f'props_d.json: {name} missing'); continue
        for o in ORIS:
            im = load(os.path.join(PD_DIR, p[o]['file']))
            if list(im.shape[1::-1]) != p[o]['size']: errors.append(f'{p[o]["file"]}: size != {p[o]["size"]}')
            errors += pixel_errors(im, p[o]['file'])
    return errors, FR, PF


def check(FR, PF):
    errors, notes = [], []
    st = J['sets']['desk']
    for (c, o), fs in sorted(FR.items()):
        for i, f in enumerate(fs):
            e = pixel_errors(f, f'{c} {o} f{i}')
            errors += e
    for c in ('desk_sit', 'desk_stand'):
        h = J['anims'][c].get('handoff', {})
        for o in ORIS:
            if o not in h: errors.append(f'{c} {o}: no handoff'); continue
            ho = h[o]; want = YAW2DIR[round(clips_d.HAND_YAW if o == 'A' else (90 - clips_d.HAND_YAW) % 360) % 360]
            if ho['facing'] != want: errors.append(f'{c} {o}: hand-off facing {ho["facing"]} != {want}')
            if ho['at'] != ('start' if c == 'desk_sit' else 'end'): errors.append(f'{c} {o}: hand-off at {ho["at"]}')
            spr = load(os.path.join(A_DIR, f'idle_{ho["facing"]}.png'))[:, :A_FW]
            OX, OY = st[o]['anchor']
            exp = paste_over(PF[o], spr, OX + ho['px'][0] - A_ANCHOR[0], OY + ho['px'][1] - A_ANCHOR[1])
            i = ho['frame']; got = FR[(c, o)][i]
            if i != (0 if ho['at'] == 'start' else len(FR[(c, o)]) - 1): errors.append(f'{c} {o}: hand-off frame {i}')
            n = int(np.any(got != exp, -1).sum())
            if n: errors.append(f'{c} {o}: hand-off frame {i} differs from props + idle_{ho["facing"]}#0 in {n} px')
            else: notes.append(f'hand-off {c} {o} f{i} = props + idle_{ho["facing"]}#0')
    for o in ORIS:
        for a, fa, b, fb in JOINS:
            x, y = FR[(a, o)][fa], FR[(b, o)][fb]
            n = int(np.any(x != y, -1).sum())
            lab = f'{a} {"last" if fa < 0 else fa} = {b} {fb} ({o})'
            if n: errors.append(f'join {lab}: {n} px differ')
            else: notes.append(f'join {lab}')
        for c in sorted(clips_d.LOOP):
            fs = FR[(c, o)]
            steps = [int(np.any(fs[i] != fs[i + 1], -1).sum()) for i in range(len(fs) - 1)]
            # the bar is the clip's own steps before its last frame, so a bad last frame can't raise it
            seam, last, bar = int(np.any(fs[-1] != fs[0], -1).sum()), steps[-1], max(steps[:-1])
            if max(seam, last) > bar:
                errors.append(f'loop seam {c} {o}: {seam} px (step into the last frame {last}) > the largest step {bar}')
            else: notes.append(f'loop seam {c} {o}: {seam} px (largest step {bar})')
    return errors, notes


errors, FR, PF = load_all()
if SELF_TEST:
    if errors: print('SELF-TEST needs a valid Stage D set first:', *errors[:10], sep='\n  '); sys.exit(2)
    base, _ = check(FR, PF); ok = not base

    def mutate(desc, key, i, fn, expect):
        global ok
        m = dict(FR); m[key] = list(FR[key]); f = m[key][i].copy(); fn(f); m[key][i] = f
        e, _ = check(m, PF); hit = [x for x in e if expect in x]
        print(f'SELF-TEST {desc}:', 'ok: ' + hit[0] if hit else 'FAILED, got ' + str(e[:3]))
        ok &= bool(hit)

    def flip(f):
        ys, xs = np.nonzero(f[..., 3] == 255); k = len(ys) // 2; f[ys[k], xs[k], :3] = INK if tuple(f[ys[k], xs[k], :3]) != INK else (251, 250, 248)
    mutate('1 (hand-off pixel)', ('desk_stand', 'B'), -1, flip, 'desk_stand B: hand-off frame')
    mutate('2 (join pixel)', ('desk_type', 'A'), 0, flip, 'join desk_sit last = desk_type 0 (A)')

    def alien(f):
        ys, xs = np.nonzero(f[..., 3] == 255); f[ys[len(ys) // 3], xs[len(ys) // 3], :3] = (1, 2, 3)
    mutate('3 (palette)', ('desk_ask', 'B'), 5, alien, 'outside the palette')
    m = dict(FR); m[('desk_talk', 'A')] = FR[('desk_talk', 'A')][:-1] + [FR[('desk_sit', 'A')][7]]
    e, _ = check(m, PF); hit = [x for x in e if 'loop seam desk_talk A' in x]
    print('SELF-TEST 4 (loop seam):', 'ok: ' + hit[0] if hit else 'FAILED, got ' + str(e[:3])); ok &= bool(hit)
    if ok: print('SELF-TEST OK: all 4 mutations caught; exit 1'); sys.exit(1)
    print('SELF-TEST FAILED'); sys.exit(2)

e2, notes = check(FR, PF)
errors += e2
for n in notes: print(' ', n)
if errors:
    print('verify_d: FAIL'); [print(' ', e) for e in errors[:60]]
    sys.exit(1)
st = J['sets']['desk']
print(f'verify_d: PASS ({len(J["anims"])} clips x 2 oris, {sum(len(v) for v in FR.values())} frames, canvas '
      + ', '.join(f'{o} {st[o]["frame_w"]}x{st[o]["frame_h"]}' for o in ORIS) + '; hand-offs, joins, seams, pixels ok)')
