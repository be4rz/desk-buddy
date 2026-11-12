"""Stage kit: assemble a clip stage's deliverables (python3 + numpy + Pillow).

    python3 make_stage.py <id> PXROOT OUTDIR --clips <scratch>/clips_<id>.json

PXROOT/bear_<clip>/ and PXROOT/obj_<clip>/ (render_anim.py --canvas 84,123,107 + pixelize_set.py anim --depth): the
bear alone and the kit objects alone, per frame and direction, in Stage A's frame and anchor. Every frame = the two
layers composited per pixel by depth. A hand-off end marked 'idle0' is rebuilt as the Stage A idle sprite (frame 0);
exits 1 if the fresh render differs from it by more than stagekit.HANDOFF_NOISE px (not render noise) or a kit object
is visible there. (The root track at a hand-off is not checked: the clip module owns where idle takes over.) Writes OUTDIR (later swapped to out/bear3d/anim_<id>):
  <clip>_<dir>.png strips + .gif, anim_<id>.json (Stage A format + root_track, the root_px in BOTH forms: per frame
  frames[i].root_px[dir] and per clip root_px[dir] = the same list, ONE track: a loader applies it once),
  face_<id>.json (face part: clips with face 'layers'), sheet_<id>.png, preview_<id>.html
"""
import sys, os, json
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import stagekit as K

a = sys.argv[1:]
SID, PXR, OUT = a[0], a[1], a[2]
CJ = json.load(open(a[a.index('--clips') + 1]))
MAN = K.manifest(SID)
os.makedirs(OUT, exist_ok=True)
AJ = json.load(open(os.path.join(K.REAL_OUT, 'anim', 'anim.json')))['anims']['idle']
FW, FH, ANCHOR = AJ['frame_w'], AJ['frame_h'], AJ['anchor']
FJ = json.load(open(os.path.join(K.REAL_OUT, 'face', 'face.json')))
ALLOWED = K.palette(SID)
PPU = CJ['ppu']
FACE_KEYS = ['armL', 'armR', 'spL', 'spR', 'sz', 'hy', 'hp', 'hr', 'bob', 'hb', 'blink']


def layer(d, f, yaw):
    b = os.path.join(d, f'f{f:02d}_yaw_{yaw:06.2f}')
    if not os.path.exists(b + '.png'): return None
    im = K.load(b + '.png'); dep = np.load(b + '_d.npy')
    return im, np.where(im[..., 3] > 0, dep, np.inf)


anims, face_clips, patch_dirs, exact, fail, checks = {}, {}, {}, {}, [], []
for c in CJ['order']:
    C = CJ['clips'][c]; n = C['n']; dirs = C.get('dirs') or K.DIRS; poses = C['poses']
    track = C['root_track']; ho = C.get('handoff') or {}
    vis = [k for k in poses[0] if k.startswith('vis_')]
    an = dict(loop=C['loop'], fps=K.FPS, frame_w=FW, frame_h=FH, anchor=ANCHOR, frames_n=n, files={},
              frames=[dict(dur=K.DUR, root_px={}) for _ in range(n)], root_px={}, root_track=track,
              face=C.get('face') or 'none')
    if ho: an['handoff'] = ho
    for d in dirs:
        yaw = float(K.YAW[d]); frames = []
        for f in range(n):
            ls = [l for l in (layer(os.path.join(PXR, f'bear_{c}'), f, yaw), layer(os.path.join(PXR, f'obj_{c}'), f, yaw)) if l]
            if not ls: fail.append(f'{c} {d} f{f}: no rendered layer'); frames.append(np.zeros((FH, FW, 4), np.uint8)); continue
            if ls[0][0].shape[:2] != (FH, FW): fail.append(f'{c} {d}: frame {ls[0][0].shape[1]}x{ls[0][0].shape[0]} != Stage A {FW}x{FH}')
            frames.append(K.composite(ls)[0])
        spr, sa = K.idle0(d)
        for at, i in (('start', 0), ('end', n - 1)):
            if ho.get(at) != 'idle0': continue
            q = poses[i]
            if any(q.get(k, 0) for k in vis): fail.append(f'{c} {d}: hand-off ({at}) frame {i} shows a kit object')
            diff, hf = K.aligned_diff(frames[i], ANCHOR, spr, sa)
            checks.append(f'{c} {d}: hand-off ({at}) frame {i} = idle_{d}#0; the fresh render differs in {diff} px')
            if diff > K.HANDOFF_NOISE:
                fail.append(f'{c} {d}: the fresh hand-off ({at}) render differs from idle_{d}#0 in {diff} px (> {K.HANDOFF_NOISE})')
            frames[i] = hf
        steps = K.track_px(PPU, yaw, track)
        for f in range(n): an['frames'][f]['root_px'][d] = steps[f]
        an['root_px'][d] = steps
        an['files'][d] = f'{c}_{d}.png'
        K.save(K.join(frames), os.path.join(OUT, f'{c}_{d}.png')); K.gif(frames, os.path.join(OUT, f'{c}_{d}.gif'))
    if C.get('face') == 'layers':
        fd = [d for d in dirs if d in FJ['dirs']]
        pdmap = C.get('patch_dir') or {}
        if pdmap: patch_dirs[c] = {d: pdmap.get(d, d) for d in fd}
        face_clips[c] = {}; exact[c] = {}
        for d in fd:
            pd = pdmap.get(d, d)
            face_clips[c][d] = {sl: [[FJ['clips']['idle'][pd][sl][0][0], FJ['clips']['idle'][pd][sl][0][1] - int(C['face_px'][sl][f])]
                                     for f in range(n)] for sl in FJ['slots']}
            exact[c][d] = [f for f in range(n) if all(abs(poses[f].get(k, 0) - poses[0].get(k, 0)) < 1e-9 for k in FACE_KEYS + vis)]
    anims[c] = an

J = dict(note=(f'Stage {SID} (stage kit, {MAN["module"]}): Stage A frame and anchor; root motion is ONE track, written in '
               'two equal forms (frames[i].root_px[dir] and root_px[dir]): a loader applies it once. root_track = the '
               'bear-local [tx, ty_px] per frame + one after (world units).'),
         stage=SID, fps=K.FPS, ppu=PPU, directions={d: dict(yaw_deg=K.YAW[d]) for d in K.DIRS}, anims=anims)   # Stage A's map form
json.dump(J, open(os.path.join(OUT, f'anim_{SID}.json'), 'w'), indent=1)
if face_clips:
    K.write_face_part(os.path.join(OUT, f'face_{SID}.json'), SID, f'anim_{SID}.json', clips=face_clips,
                      patch_dirs=patch_dirs or None, exact=exact)
K.stage_sheet(SID, OUT); K.stage_preview(SID, OUT)
for p in sorted(os.listdir(OUT)):
    if p.endswith('.png') and not p.startswith('sheet_'):
        for e in K.pixel_errors(K.load(os.path.join(OUT, p)), p, ALLOWED): fail.append(e)
for line in checks: print(line)
if fail:
    for f in fail: print('FAIL', f)
    sys.exit(1)
print('ok', OUT, len(anims), 'clips')
