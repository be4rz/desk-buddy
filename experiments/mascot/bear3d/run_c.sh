#!/bin/sh
# Stage C: think / talk / ask (loop) + oops / happy / celebrate (one-shot) on the Stage A rig, camera and pixel step,
# 8 directions, 12 fps, in Stage A's 84x123 frame and anchor. Needs bear.blend (run.sh) and anim/ (run_anim.sh).
# Writes bear_rig_c.blend + clips_c.json (also used by run_face.sh) and ../out/bear3d/anim_c/.
#   sh run_c.sh [RAWDIR]     (RAWDIR = scratch for the render passes; each clip's passes are deleted once pixelized)
set -e
cd "$(dirname "$0")"
B=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
RAW=${1:-${TMPDIR:-/tmp}/bear3d_c_raw}
OUT=../out/bear3d/anim_c
CANVAS=$(python3 -c "import json;a=json.load(open('../out/bear3d/anim/anim.json'))['anims']['idle'];print(a['frame_w'],a['frame_h'],a['anchor'][1],sep=',')")
rm -rf "$RAW"; mkdir -p "$RAW/px"
rm -f bear_rig_c.blend clips_c.json   # Blender exits 0 on a Python error: a missing output is the failure signal
$B -b bear.blend -P rig_bear.py -- --stage c | grep -E "CLIP|drift|SAVED|Error|Traceback" || true
[ -f bear_rig_c.blend ] && [ -f clips_c.json ] || { echo "rig failed (rig_bear.py --stage c)"; exit 1; }
for c in think talk ask oops happy celebrate; do
  $B -b bear_rig_c.blend -P render_anim.py -- --raw "$RAW" --tag big --clip $c --canvas $CANVAS | grep -E "SIZE|CANVAS|ALL DONE|Error|Traceback" || true
  [ -f "$RAW/big_$c/meta.json" ] || { echo "render failed: $c"; exit 1; }
  python3 pixelize_anim.py "$RAW/big_$c" "$RAW/px"; rm -rf "$RAW/big_$c"
done
rm -rf ../out/bear3d/face   # its positions match the old clips (the pack rejects a stale face.json): rerun run_face.sh
rm -rf "$OUT"; python3 make_c.py "$RAW/px" "$OUT" clips_c.json
python3 preview_c.py
rm -rf "$RAW"
