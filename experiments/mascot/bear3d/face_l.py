"""Stage L face pass (python3): add the listen clip to face/face.json, making it face.json v2 (additive).

    python3 face_l.py [ART_DIR]       (default ../out/bear3d; needs face/ from run_face.sh and anim_l/ from make_l.py)
No new pixels: listen draws the existing patches. On listen's held frames the head is turned by exactly 45 deg, so it
renders exactly as the neighbouring direction's head and the face patch dir follows the head
(anim_clips.LISTEN_PATCH_DIR: S->S, SE->S, SW->S, E->SE, W->SW), written as "patch_dirs": {"listen": {...}}.
Positions: the patch dir's rest top-left (idle frame 0's position) shifted up by the frame's whole-pixel face shift
(clips_l.json face_px: the body bob + the head bob).
Everything already in face.json stays as it is: the same keys, values and order (a v1 reader that ignored
face_version reads the existing entries unchanged); only face_version becomes 2 and clips.listen + patch_dirs are
added (or replaced on a re-run). Exits 1 if that would change anything else.
"""
import sys, os, json

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from anim_clips import LISTEN_DIR_VARIANT, LISTEN_PATCH_DIR

ART = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, '..', 'out', 'bear3d'))
FJ = os.path.join(ART, 'face', 'face.json')
F = json.load(open(FJ))
old = json.loads(json.dumps(F))
AL = json.load(open(os.path.join(ART, 'anim_l', 'anim_l.json')))['anims']
CL = json.load(open(os.path.join(HERE, 'clips_l.json')))['clips']
SLOTS = list(F['slots'])
DIRS = F['dirs']

# the shifts are relative to the rest pose: every variant's listen_in starts on it
assert all(CL[f'listen_in_{v}']['face_px'][s][0] == 0 for v in set(LISTEN_DIR_VARIANT.values()) for s in SLOTS)
listen = {}
for d in DIRS:
    pd, v = LISTEN_PATCH_DIR[d], LISTEN_DIR_VARIANT[d]
    shift = CL[f'listen_{v}']['face_px']
    assert len(shift['eyes']) == AL['listen']['frames_n'], 'clips_l.json and anim_l.json disagree on listen'
    listen[d] = {}
    for s in SLOTS:
        x0, y0 = F['clips']['idle'][pd][s][0]
        listen[d][s] = [[x0, y0 - k] for k in shift[s]]

F['face_version'] = 2
F['clips']['listen'] = listen
F['patch_dirs'] = dict(F.get('patch_dirs', {}), listen={d: LISTEN_PATCH_DIR[d] for d in DIRS})
# nothing but the additions may change
for k, v in old.items():
    if k in ('face_version', 'patch_dirs'): continue
    if k == 'clips':
        for c, cv in v.items():
            if c != 'listen' and F['clips'][c] != cv: sys.exit(f'face_l.py: would change clips.{c}')
    elif F[k] != v: sys.exit(f'face_l.py: would change {k}')
json.dump(F, open(FJ, 'w'), indent=1)
print('ok', FJ, 'v2: listen in', ', '.join(f'{d}->{LISTEN_PATCH_DIR[d]}' for d in DIRS))
