#!/bin/sh
# Stage L: listen_in / listen / listen_out (S SE E W SW; the whole round head mass turns toward the camera) and
# turn_ccw / turn_cw (8 directions), on the Stage C rig + a head bone (rig_bear.py --stage l), in Stage A's 84x123
# frame and anchor, 12 fps. Then the face pass for listen (face_l.py: face.json v2, additive) and the checks.
# Needs bear.blend (run.sh), anim/ (run_anim.sh) and face/ (run_face.sh). Stage A/B/C, props and the existing face
# files are not touched. Writes clips_l.json and ../out/bear3d/anim_l/ (+ face/face.json v2). ~2 min.
# anim_l/ is replaced only after make_l.py's hand-off check passes (a failed run keeps the previous one).
# run_face.sh and run_c.sh rewrite face.json as v1: run `python3 face_l.py` (or this script) after them.
#   sh run_l.sh [WORKDIR]     (WORKDIR = scratch for the rig, the render passes and pixel frames; removed at the end;
#                              an existing WORKDIR must be one this script made)
set -e
cd "$(dirname "$0")"
B=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
WK=${1:-${TMPDIR:-/tmp}/bear3d_l_work}
OUT=../out/bear3d/anim_l
if [ -e "$WK" ] && [ ! -f "$WK/.bear3d_l_work" ]; then echo "refusing to clear $WK: not a run_l.sh work dir"; exit 1; fi
CANVAS=$(python3 -c "import json;a=json.load(open('../out/bear3d/anim/anim.json'))['anims']['idle'];print(a['frame_w'],a['frame_h'],a['anchor'][1],sep=',')")
# "<variant>:<yaws>" per listen head-yaw variant (anim_clips.LISTEN_DIR_VARIANT x the direction yaws), and all 8 yaws
VARIANTS=$(python3 -c "
import json, anim_clips as a
Y = {d: v['yaw_deg'] for d, v in json.load(open('../out/bear3d/anim/anim.json'))['directions'].items()}
vs = {}
for d, v in a.LISTEN_DIR_VARIANT.items(): vs.setdefault(v, []).append('%g' % Y[d])
print(' '.join(v + ':' + ','.join(y) for v, y in vs.items()))
print(','.join('%g' % y for y in Y.values()))")
ALL_YAWS=$(echo "$VARIANTS" | tail -1); VARIANTS=$(echo "$VARIANTS" | head -1)
rm -rf "$WK"; mkdir -p "$WK/px"; touch "$WK/.bear3d_l_work"
rm -f clips_l.json   # Blender exits 0 on a Python error: a missing output is the failure signal
$B -b bear.blend -P rig_bear.py -- --stage l --out "$WK/bear_rig_l.blend" | grep -E "HEAD|CLIP|drift|SAVED|Error|Traceback" || true
[ -f "$WK/bear_rig_l.blend" ] && [ -f clips_l.json ] || { echo "rig failed (rig_bear.py --stage l)"; exit 1; }
r() {  # clip yaws
  $B -b "$WK/bear_rig_l.blend" -P render_anim.py -- --raw "$WK" --tag big --clip $1 --canvas $CANVAS --yaws $2 2>&1 \
    | grep -E "CANVAS|Error|Traceback" || true
  [ -f "$WK/big_$1/meta.json" ] || { echo "render failed: $1"; exit 1; }
  python3 pixelize_anim.py "$WK/big_$1" "$WK/px" >/dev/null; rm -rf "$WK/big_$1"
}
for c in listen_in listen listen_out; do
  for vy in $VARIANTS; do r "${c}_${vy%%:*}" "${vy#*:}"; done   # S: _0 (head 0); SE E: _cw (-45); SW W: _ccw (+45)
done
r turn_ccw "$ALL_YAWS"
r turn_cw "$ALL_YAWS"
python3 make_l.py "$WK/px" "$WK/anim_l" clips_l.json   # exits 1 on a hand-off mismatch: the old anim_l/ stays
rm -rf "$OUT"; mv "$WK/anim_l" "$OUT"
python3 face_l.py
python3 verify_l.py
python3 verify_face.py
rm -rf "$WK"
