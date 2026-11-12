"""Stage kit pixel step (python3 + numpy + Pillow): render passes -> palette pixel layers + depth.

    python3 pixelize_set.py set  RAW/<set>_<ori> PX/<set>_<ori> --module set_<id> [--keep]
    python3 pixelize_set.py anim RAW/<tag>_<clip> PX --module clips_<id> [--depth] [--keep]

bear layers (set: bear_*.npz; anim: tag 'bear') -> the unchanged Stage A bear step (pixelize.pixelize), so the bear
looks exactly as in Stage A. Prop and object layers -> pixelize_b.py's furniture step, unchanged (it is called, not
copied), with the part table taken from the module's PARTS: {name: {pid, ramp, idx (3 ramp indices dark -> light),
mg (material group), thresh (2 shade thresholds), thin (claim fraction, optional), prio (optional)}}. Ramps resolve from
palette-ext-<id>.json, palette-ext.json, then the bear palettes (stagekit.ramps). Pass ids are the kit's (50+).
set:  writes <key>.png + <key>_d.npy (depth; the 1 px outline takes its inner neighbour's depth) per layer.
anim: writes PX/<tag>_<clip>/f<NN>_yaw_<deg>.png (+ _d.npy with --depth) per frame, aliased frames copied, meta.json.
Raw passes are deleted once pixelized unless --keep.
"""
import sys, os, json, glob, shutil
import numpy as np
from PIL import Image
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import pixelize as PB
import pixelize_b as PX
import stagekit


def install(mod, sid):
    """extend pixelize_b's part table with the module's PARTS (at run time; pixelize_b.py is not edited)."""
    R = stagekit.ramps(sid)
    for name, p in getattr(mod, 'PARTS', {}).items():
        pid = p['pid']; assert pid >= 50, f'{name}: kit pass ids start at 50'
        assert pid not in PX.PARTS or PX.PARTS[pid][0] == name, f'pass id {pid} taken by {PX.PARTS[pid][0]}'
        ramp = [R[p['ramp']][i] for i in p['idx']]
        mg = p['mg']
        if mg not in PX.MG: PX.MG[mg] = max(PX.MG.values()) + 1
        PX.THRESH.setdefault(mg, tuple(p['thresh']))
        assert PX.THRESH[mg] == tuple(p['thresh']), f'{name}: material group {mg} already has thresholds {PX.THRESH[mg]}'
        PX.PARTS[pid] = (name, ramp, mg, pid, p.get('thin'), p.get('prio', 0))
        PX.NID = max(PX.NID, pid + 1)
    PX.ALLOWED |= stagekit.palette(sid)


def layer(f, meta, bear):
    if bear: rgba, dep = PB.pixelize(f, meta, want_depth=True); PB.check(rgba)
    else: rgba, dep = PX.pixelize_props(f, meta)
    PX.check(rgba)
    return rgba, PX.outline_depth(rgba, dep)


if __name__ == '__main__':
    a = sys.argv[1:]; mode, raw, px = a[0], a[1].rstrip('/'), a[2]
    modname = a[a.index('--module') + 1]; mod = stagekit.module(modname)
    sid = modname.split('_', 1)[1]
    install(mod, sid)
    meta = json.load(open(os.path.join(raw, 'meta.json')))
    keep = '--keep' in a
    if mode == 'set':
        os.makedirs(px, exist_ok=True); n = 0
        for f in sorted(glob.glob(os.path.join(raw, '*.npz'))):
            key = os.path.basename(f)[:-4]
            rgba, dep = layer(f, meta, key.startswith('bear_'))
            Image.fromarray(rgba).save(os.path.join(px, key + '.png')); np.save(os.path.join(px, key + '_d.npy'), dep)
            if not keep: os.remove(f)
            n += 1
        json.dump(meta, open(os.path.join(px, 'meta.json'), 'w'))
        print('ok', px, n, 'layers')
    elif mode == 'anim':
        od = os.path.join(px, os.path.basename(raw)); os.makedirs(od, exist_ok=True)
        bear = meta['name'] == 'bear'
        for f in sorted(glob.glob(os.path.join(raw, 'f*_yaw_*.npz'))):
            d = np.load(f)
            if not d['alpha'].any():          # an object layer frame with every object hidden
                rgba = np.zeros((meta['H'], meta['W'], 4), np.uint8); dep = np.full((meta['H'], meta['W']), np.inf, np.float32)
            else: rgba, dep = layer(f, meta, bear)
            b = os.path.join(od, os.path.basename(f)[:-4])
            Image.fromarray(rgba).save(b + '.png')
            if '--depth' in a: np.save(b + '_d.npy', dep)
            if not keep: os.remove(f)
        for fr, src in meta['alias'].items():
            for y in meta['yaws']:
                for ext in ('.png', '_d.npy'):
                    s = os.path.join(od, f'f{int(src):02d}_yaw_{y:06.2f}{ext}')
                    if os.path.exists(s): shutil.copy(s, os.path.join(od, f'f{int(fr):02d}_yaw_{y:06.2f}{ext}'))
        json.dump(meta, open(os.path.join(od, 'meta.json'), 'w'), indent=1)
        print('ok', od, meta['n'], 'frames')
    else:
        raise SystemExit(__doc__)
