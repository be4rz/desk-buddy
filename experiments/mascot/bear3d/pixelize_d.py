"""Stage D pixel step (python3 + numpy + Pillow): render_d.py layer passes -> palette pixel layers + depth.

    python3 pixelize_d.py RAW/desk_<ori> PX/desk_<ori> [--keep]

pixelize_b.py's step, unchanged: bear_*.npz -> the Stage A bear pixel step (pixelize.py); other layers -> the
furniture step with Stage B's part table EXTENDED (at run time, pixelize_b.py is not edited) by the desk, chair and
computer parts below (pass ids 38-48, palette-ext.json ramps wood / fabric_rose / toy_plastic / screen).
Writes <key>.png (1x, alpha 0/255, palette-exact) and <key>_d.npy (depth per pixel). Raw passes are deleted once
pixelized unless --keep.
"""
import sys, os, json, glob
import numpy as np
from PIL import Image
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pixelize as PB
import pixelize_b as PX

R_WOOD = PX.ramp('wood', (2, 3, 4)); R_WOOD_D = PX.ramp('wood', (1, 2, 3)); R_ROSE = PX.ramp('fabric_rose', (1, 2, 3))
R_TOY = PX.ramp('toy_plastic', (2, 3, 4)); R_KEY = PX.ramp('toy_plastic', (1, 2, 3))
R_SCR = PX.ramp('screen', (1, 2, 3)); R_SCR_HI = PX.ramp('screen', (2, 3, 4))
PX.PARTS.update({
    38: ('desk_top', R_WOOD, 'wood', 38, None, 0), 39: ('desk_legs', R_WOOD, 'wood', 39, None, 0),
    40: ('chair_seat', R_ROSE, 'rose', 40, None, 0), 41: ('chair_back', R_ROSE, 'rose', 41, None, 0),
    42: ('chair_stem', R_WOOD_D, 'wood', 42, None, 0), 43: ('chair_base', R_WOOD_D, 'wood', 43, None, 0),
    44: ('monitor', R_TOY, 'toy', 44, None, 0), 45: ('screen', R_SCR, 'screen', 45, None, 0),
    46: ('screen_hi', R_SCR_HI, 'screen', 45, None, 0), 47: ('keyboard', R_TOY, 'toy', 47, None, 0),
    48: ('keys', R_KEY, 'toy', 48, 0.30, 4),
})
PX.NID = 49
PX.MG.update({'toy': 6, 'screen': 7})
PX.THRESH.update({'toy': (0.46, 0.70), 'screen': (0.40, 0.66)})
ALLOWED = PX.ALLOWED

if __name__ == '__main__':
    raw, px = sys.argv[1].rstrip('/'), sys.argv[2]
    meta = json.load(open(os.path.join(raw, 'meta.json'))); os.makedirs(px, exist_ok=True)
    n = 0
    for f in sorted(glob.glob(os.path.join(raw, '*.npz'))):
        key = os.path.basename(f)[:-4]
        if key.startswith('bear_'): rgba, dep = PB.pixelize(f, meta, want_depth=True); PB.check(rgba)
        else: rgba, dep = PX.pixelize_props(f, meta)
        PX.check(rgba)
        Image.fromarray(rgba).save(os.path.join(px, key + '.png'))
        np.save(os.path.join(px, key + '_d.npy'), PX.outline_depth(rgba, dep))
        if '--keep' not in sys.argv: os.remove(f)
        n += 1
    json.dump(meta, open(os.path.join(px, 'meta.json'), 'w'))
    print('ok', px, n, 'layers')
