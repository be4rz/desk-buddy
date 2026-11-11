"""Stage D face pass (python3): add the seated desk_talk / desk_ask face positions to face/face.json as "set_clips"
(additive; face.json must already be v2, i.e. face_l.py ran).

    python3 face_d.py [ART_DIR]       (default ../out/bear3d; needs face/ v2 and anim_d/ from make_d.py)
No new pixels: the seated talk / ask frames draw the existing patches. On them the body faces SE (ori A) / SW (ori B)
and the head is turned by exactly 45 deg toward the camera (Stage L's whole-head turn), so it renders exactly as the
S head: patch dir S (clips_d.PATCH_DIR, also anim_d.json's face_patch_dir).
Positions (patch top-left in the anim_d set canvas): the S rest top-left (idle frame 0) measured from the Stage A
anchor, moved to the bear's seated root pixel (anim_d.json sets.desk.<ori>.anchor + seat_px), shifted up by the
frame's whole-pixel face lift (body bob + head bob, from clips_d's poses).
Written as "set_clips": {clip: {ori: {"patch": "S", "eyes": [[x, y], ...], "mouth": [...]}}} (prototype #face; the
set comes from anim_d.json).
Everything already in face.json stays as it is (same keys, values, order); only set_clips is added (or replaced on a
re-run). Exits 1 if that would change anything else.
"""
import sys, os, json

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import clips_d

ART = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, '..', 'out', 'bear3d'))
FJ = os.path.join(ART, 'face', 'face.json')
F = json.load(open(FJ))
old = json.loads(json.dumps(F))
if F.get('face_version') != 2: sys.exit('face_d.py: face.json is not v2 (run python3 face_l.py first)')
AD = json.load(open(os.path.join(ART, 'anim_d', 'anim_d.json')))
A_ANCHOR = json.load(open(os.path.join(ART, 'anim', 'anim.json')))['anims']['idle']['anchor']
PXZ = json.load(open(os.path.join(HERE, 'clips.json')))['PXZ']
# the face lift only depends on bob and hb; every face-clip frame sits on the seat (checked below)
G = dict(PXZ=PXZ, hand=(9.0, 9.0), seat=(0.0, 0.0), seatZ=0.0)
CL = clips_d.build(G)
pd = clips_d.PATCH_DIR

set_clips = {}
for c in clips_d.FACE_CLIPS:
    a = AD['anims'][c]
    assert a['frames_n'] == len(CL[c]), f'clips_d.py and anim_d.json disagree on {c}'
    assert a.get('face_patch_dir') == pd, f'anim_d.json {c} face_patch_dir {a.get("face_patch_dir")} != {pd}'
    set_clips[c] = {}
    for o in ('A', 'B'):
        so = AD['sets'][a['set']][o]
        rx, ry = so['anchor'][0] + so['seat_px'][0], so['anchor'][1] + so['seat_px'][1]
        set_clips[c][o] = dict(patch=pd)
        for s in F['slots']:
            x0, y0 = F['clips']['idle'][pd][s][0]
            pos = []
            for q, _ in CL[c]:
                assert (q['X'], q['Y'], q['Z'], q['hy']) == (0.0, 0.0, 0.0, clips_d.TALK_HY), f'{c}: not seated, head S'
                lift = int(round(q['bob'] / PXZ)) + int(q['hb'])
                pos.append([rx + x0 - A_ANCHOR[0], ry + y0 - A_ANCHOR[1] - lift])
            set_clips[c][o][s] = pos

F['set_clips'] = set_clips
for k, v in old.items():
    if k != 'set_clips' and F[k] != v: sys.exit(f'face_d.py: would change {k}')
json.dump(F, open(FJ, 'w'), indent=1)
print('ok', FJ, 'set_clips:', ', '.join(f'{c} A/B patch {pd}' for c in set_clips))
