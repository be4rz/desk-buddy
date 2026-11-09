"""Pixelize one rendered clip (render_anim.py output) with the cute pixel step (pixelize.py), python3.

    python3 pixelize_anim.py RAW/<tag>_<clip> PXDIR
Writes PXDIR/<tag>_<clip>/f<NN>_yaw_<deg>.png (1x, palette-exact, alpha 0/255). Aliased frames (same pose as
an earlier frame) are copies of that frame, so held poses are pixel-identical by construction.
"""
import sys, os, json, glob, shutil
from PIL import Image
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pixelize import pixelize, check

raw, px = sys.argv[1].rstrip('/'), sys.argv[2]
meta = json.load(open(os.path.join(raw, 'meta.json')))
od = os.path.join(px, os.path.basename(raw)); os.makedirs(od, exist_ok=True)
for f in sorted(glob.glob(os.path.join(raw, 'f*_yaw_*.npz'))):
    rgba = pixelize(f, meta); check(rgba)
    Image.fromarray(rgba).save(os.path.join(od, os.path.basename(f)[:-4] + '.png'))
for f, src in meta['alias'].items():
    for y in meta['yaws']:
        shutil.copy(os.path.join(od, f'f{int(src):02d}_yaw_{y:06.2f}.png'), os.path.join(od, f'f{int(f):02d}_yaw_{y:06.2f}.png'))
json.dump(meta, open(os.path.join(od, 'meta.json'), 'w'), indent=1)
print('ok', od, meta['n'], 'frames')
