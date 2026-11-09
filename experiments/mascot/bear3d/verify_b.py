"""Stage B hand-off check (python3 + numpy + Pillow), independent of make_b.py.

    python3 verify_b.py [ANIM_B_DIR]      (default ../out/bear3d/anim_b)
For every transition clip and orientation: rebuild "props-only frame + Stage A rest sprite (idle_<facing>.png
frame 0, Stage A anchor on the hand-off pixel)" from the published files only, and require the clip's first
(get_in_bed, sit_down) or last (wake_up, stand_up) frame to be pixel-identical. Also checks that sleep -> wake_up
and read -> stand_up start on the loop's frame 0. Exit code 1 on any mismatch.
"""
import sys, os, json
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
D = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, '..', 'out', 'bear3d', 'anim_b')
J = json.load(open(os.path.join(D, 'anim_b.json')))
AJ = json.load(open(os.path.join(D, '..', 'anim', 'anim.json')))
ax, ay = AJ['anims']['idle']['anchor']; afw = AJ['anims']['idle']['frame_w']
def img(p): return np.asarray(Image.open(p).convert('RGBA'))
def frame(clip, ori, i):
    a = J['anims'][clip]; s = J['sets'][a['set']][ori]; W = s['frame_w']
    return img(os.path.join(D, a['files'][ori]))[:, i * W:(i + 1) * W]
bad = 0
for clip, a in J['anims'].items():
    for ori, h in a.get('handoff', {}).items():
        s = J['sets'][a['set']][ori]; OX, OY = s['anchor']
        want = img(os.path.join(D, s['props_frame'])).copy()
        spr = img(os.path.join(D, '..', 'anim', f"idle_{h['facing']}.png"))[:, :afw]
        x0, y0 = OX + h['px'][0] - ax, OY + h['px'][1] - ay
        ys, xs = np.nonzero(spr[..., 3])   # place only the opaque part; the transparent margin may hang off the set
        spr = spr[ys.min():ys.max() + 1, xs.min():xs.max() + 1]; x0 += int(xs.min()); y0 += int(ys.min())
        H, W = want.shape[:2]; m = spr[..., 3] > 0
        assert 0 <= x0 and 0 <= y0 and x0 + spr.shape[1] <= W and y0 + spr.shape[0] <= H, (clip, ori, 'sprite off canvas')
        reg = want[y0:y0 + spr.shape[0], x0:x0 + spr.shape[1]]; reg[m] = spr[m]
        got = frame(clip, ori, h['frame'])
        n = int(np.any(got != want, -1).sum()); bad += n > 0
        print(f"{clip:10s} {ori} {h['at']:5s} frame {h['frame']:2d}  idle_{h['facing']} at {h['px']}: "
              f"{'IDENTICAL' if n == 0 else f'{n} px differ'}")
for lp, nx in (('sleep', 'wake_up'), ('read', 'stand_up')):
    for ori in ('A', 'B'):
        same = np.array_equal(frame(lp, ori, 0), frame(nx, ori, 0)); bad += not same
        print(f'{nx:10s} {ori} start == {lp} frame 0: {"IDENTICAL" if same else "DIFFERENT"}')
print('PASS' if not bad else f'FAIL ({bad})'); sys.exit(1 if bad else 0)
