"""Stage kit: assemble a set stage's deliverables (make_d.py's method, generic; python3 + numpy + Pillow).

    python3 make_set.py <id> PXROOT OUTDIR --clips <scratch>/clips_<id>.json

PXROOT/<set>_<ori>/ from render_set.py + pixelize_set.py: layout.json, props.json, clip_<name>.json and the pixel
layers (<key>.png + <key>_d.npy). The Stage A rest sprites come from the real out/bear3d/anim (idle_<DIR>.png frame 0).
Every frame = all its layers composited per pixel by depth. Hand-off frames are rebuilt as the props-only frame + the
Stage A idle sprite (frame 0) pasted at the hand-off pixel; exits 1 if a prop hides a bear pixel there or the fresh
render differs from that by more than stagekit.HANDOFF_NOISE px. Writes to OUTDIR (later swapped to out/bear3d/set_<id>):
  <clip>_<ori>.png strips, <clip>_<ori>.gif (4x), props_<set>_<ori>.png (props-only frame, clip canvas),
  prop_<name>_<ori>.png (cropped prop sprites), anim_<id>.json (Stage B format), props_<id>.json (Stage B props format,
  "set" per prop), face_<id>.json (face part: set_clips + exact), sheet_<id>.png, preview_<id>.html
"""
import sys, os, json
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import stagekit as K

a = sys.argv[1:]
SID, PXR, OUT = a[0], a[1], a[2]
CJ = json.load(open(a[a.index('--clips') + 1]))
MAN = K.manifest(SID); MOD = K.module(MAN['module'])
os.makedirs(OUT, exist_ok=True)
AJ = json.load(open(os.path.join(K.REAL_OUT, 'anim', 'anim.json')))
A_ANCHOR = AJ['anims']['idle']['anchor']
FJ = json.load(open(os.path.join(K.REAL_OUT, 'face', 'face.json')))
ALLOWED = K.palette(SID)

_cache = {}
def layer(d, key):
    p = os.path.join(d, key)
    if p not in _cache:
        im = K.load(p + '.png'); dep = np.load(p + '_d.npy')
        _cache[p] = (im, np.where(im[..., 3] > 0, dep, np.inf))
    return _cache[p]
def comp(d, keys): return K.composite([layer(d, k) for k in keys])

anims, sets_json, props_json, face_sets, exact, checks, fail = {}, {}, {}, {}, {}, [], []
FR = {}; LAY = {}
for s, SD in MOD.SETS.items():
    for o in K.ORIS:
        d = os.path.join(PXR, f'{s}_{o}'); lay = json.load(open(os.path.join(d, 'layout.json'))); LAY[(s, o)] = lay
        W, H = lay['W'], lay['H']; OX, OY = lay['origin']
        if W > K.MAX_SET_W or H > K.MAX_SET_H: fail.append(f'canvas {s} {o} {W}x{H} exceeds {K.MAX_SET_W}x{K.MAX_SET_H}')
        pk = json.load(open(os.path.join(d, 'props.json')))
        props_frame, _ = comp(d, list(pk.values()))
        pf_name = f'props_{s}_{o}.png'; K.save(props_frame, os.path.join(OUT, pf_name))
        sets_json.setdefault(s, {})[o] = dict(frame_w=W, frame_h=H, anchor=[OX, OY], props_frame=pf_name,
                                              orientation=SD.get('ori_note', {}).get(o, ''))
        for name, key in pk.items():
            im = layer(d, key)[0]; ys, xs = np.nonzero(im[..., 3])
            x0, x1, y0, y1 = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
            fn = f'prop_{name}_{o}.png'; K.save(im[y0:y1, x0:x1], os.path.join(OUT, fn))
            ppx = lay['prop_px'][name]
            props_json.setdefault(name, dict(set=s))[o] = dict(file=fn, size=[int(x1 - x0), int(y1 - y0)],
                                                              anchor=[int(OX + ppx[0] - x0), int(OY + ppx[1] - y0)],
                                                              offset_px_in_set=ppx, layer_key=key)
        for c in SD['clips']:
            C = json.load(open(os.path.join(d, f'clip_{c}.json')))
            frames = [comp(d, list(fr['keys'].values()))[0] for fr in C['frames']]
            h = lay['handoff'].get(c); ho = None
            if h:
                dname = K.YAW2DIR[round(h['yaw']) % 360]; spr, sa = K.idle0(dname)
                i = 0 if h['at'] == 'start' else len(frames) - 1
                anc = (OX + h['px'][0], OY + h['px'][1])
                diff, hf = K.aligned_diff(frames[i], anc, spr, sa, props_frame)
                _, owner = comp(d, list(C['frames'][i]['keys'].values()))
                bear_lost = int(((layer(d, C['frames'][i]['keys']['bear'])[0][..., 3] > 0) & (owner != 0)).sum())
                checks.append(f'{c} {o}: hand-off frame {i} = props + idle_{dname}#0; the fresh render differs in {diff} px; '
                              f'bear pixels hidden by props there: {bear_lost}')
                if bear_lost: fail.append(f'{c} {o}: {bear_lost} bear pixels hidden by props at the hand-off spot')
                if diff > K.HANDOFF_NOISE:
                    fail.append(f'{c} {o}: the fresh hand-off render differs from props + idle_{dname}#0 in {diff} px '
                                f'(> {K.HANDOFF_NOISE}: not render noise)')
                frames[i] = hf
                ho = dict(at=h['at'], frame=i, pos_world=[round(v, 4) for v in h['pos']], px=h['px'], facing=dname,
                          yaw_deg=h['yaw'], stage_a_sprite=f'../anim/idle_{dname}.png#0')
            FR[(c, o)] = frames
            n = len(frames)
            K.save(K.join(frames), os.path.join(OUT, f'{c}_{o}.png')); K.gif(frames, os.path.join(OUT, f'{c}_{o}.gif'))
            an = anims.setdefault(c, dict(set=s, loop=C['loop'], fps=K.FPS, frames_n=n, files={}, handoff={},
                                          frames=[dict(dur=K.DUR) for _ in C['frames']], next=SD.get('next', {}).get(c, '')))
            an['files'][o] = f'{c}_{o}.png'
            if ho: an['handoff'][o] = ho
            if c in SD.get('face', []):
                # face part: patches at the bear's root pixel (whole pixels) + idle#0's patch offset - the face lift
                pd = SD['patch'][o]; an.setdefault('face_patch_dir', {})[o] = pd
                poses = CJ['clips'][f'{c}@{o}']['poses']
                ent = dict(patch=pd)
                for sl in FJ['slots']:
                    x0, y0 = FJ['clips']['idle'][pd][sl][0]; pos = []
                    for f, fr in enumerate(C['frames']):
                        rx, ry = OX + fr['root_px'][0], OY + fr['root_px'][1]
                        if abs(rx - round(rx)) > 1e-3 or abs(ry - round(ry)) > 1e-3:
                            fail.append(f'{c} {o} f{f}: the bear root is not on a whole pixel ({rx}, {ry}): face patches would not match')
                        pos.append([int(round(rx)) + x0 - A_ANCHOR[0], int(round(ry)) + y0 - A_ANCHOR[1] - int(CJ['clips'][f'{c}@{o}']['face_px'][sl][f])])
                    ent[sl] = pos
                face_sets.setdefault(c, {})[o] = ent
                keys = ['armL', 'armR', 'spL', 'spR', 'sz', 'hy', 'hp', 'hr', 'bob', 'hb', 'X', 'Y', 'Z', 'yaw']
                exact.setdefault(c, {})[o] = [f for f, q in enumerate(poses) if all(abs(q[k] - poses[0][k]) < 1e-9 for k in keys)]
for c, an in anims.items():
    if not an['handoff']: an.pop('handoff')

json.dump(dict(note=' '.join(sd['note'] for sd in MOD.SETS.values()),
               sorting=('As Stage B: outside these clips draw the props (prop_*.png or props_<set>_<ori>.png) and the free '
                        'bear as separate sprites sorted by floor depth; from the first to the last frame of a set clip hide '
                        'them and draw the baked frame at the set anchor. face_patch_dir is per ori.'),
               stage=SID, fps=K.FPS, ppu=CJ['ppu'], sets=sets_json, anims=anims),
          open(os.path.join(OUT, f'anim_{SID}.json'), 'w'), indent=1)
for name, v in props_json.items():
    pi = getattr(MOD, 'PROPS', {}).get(name, {})
    v['footprint_world'] = dict(pi.get('footprint', {}), frame='prop-local, ori A; ori B: (x, y) -> (-y, -x)')
    v['origin_world_in_set'] = [round(x, 4) for x in LAY[(v['set'], 'A')]['prop_pos'][name]]
    if pi.get('note'): v['note'] = pi['note']
json.dump(dict(note=(f'Stage {SID} props (render_set.py, chunky superellipsoid toy shapes), pre-rendered with the Stage A '
                     'camera and pixel step. Each prop sprite = that prop rendered alone with its own ink outline; anchor = '
                     'pixel of the prop origin. props_<set>_<ori>.png = the props-only frame on the clip canvas.'),
               ppu=CJ['ppu'], pass_ids={n: p['pid'] for n, p in getattr(MOD, 'PARTS', {}).items()}, props=props_json),
          open(os.path.join(OUT, f'props_{SID}.json'), 'w'), indent=1)
if face_sets:
    K.write_face_part(os.path.join(OUT, f'face_{SID}.json'), SID, f'anim_{SID}.json', set_clips=face_sets, exact=exact)

K.contact_sheet([(f'{c} {o} ({len(FR[(c, o)])}{" loop" if anims[c]["loop"] else ""})', FR[(c, o)])
                 for s, SD in MOD.SETS.items() for c in SD['clips'] for o in K.ORIS], os.path.join(OUT, f'sheet_{SID}.png'))
K.stage_preview(SID, OUT)

for p in sorted(os.listdir(OUT)):
    if p.endswith('.png') and not p.startswith('sheet_'):
        for e in K.pixel_errors(K.load(os.path.join(OUT, p)), p, ALLOWED): fail.append(e)
for line in checks: print(line)
for (c, o), fs in sorted(FR.items()):
    steps = [K.changed(fs[i], fs[i + 1]) for i in range(len(fs) - 1)]
    seam = f', loop seam {K.changed(fs[-1], fs[0])}' if anims[c]['loop'] else ''
    print(f'changed px per step {c} {o}: max {max(steps)} median {int(np.median(steps))}{seam}')
for (s, o), l in sorted(LAY.items()): print(f'canvas {s} {o}: {l["W"]}x{l["H"]}')
if fail:
    for f in fail: print('FAIL', f)
    sys.exit(1)
print('ok', OUT)
