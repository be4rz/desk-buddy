#!/bin/sh
# Stage D: the desk station (low toy desk, chunky computer + keyboard, swivel chair) and the desk clips (desk_sit,
# desk_type, desk_swivel_in, desk_talk, desk_ask, desk_swivel_out, desk_stand) in both orientations (A, B = 3D mirror)
# on the Stage L rig with hip joints (built here in scratch: rig_bear.py --stage l --legs; the committed clips_l.json
# is not touched), with Stage B's set/props method (render_d.py -> pixelize_d.py -> make_d.py). Then the face pass
# (face_d.py: face.json set_clips, additive) and the checks. Needs bear.blend (run.sh), anim/ (run_anim.sh), face/ (run_face.sh + face_l.py).
# Stage A/B/C/L, props and the existing face files are not touched. ~2 min.
# anim_d/, props_d/ and face.json are replaced only if make_d.py's hand-off check, face_d.py, verify_d.py and
# verify_face.py all pass (a failed run restores the previous ones).
#   sh run_d.sh [WORKDIR]     (WORKDIR = scratch for the rig, the render passes and pixel layers; removed at the end;
#                              an existing WORKDIR must be one this script made)
set -e
cd "$(dirname "$0")"
B=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
WK=${1:-${TMPDIR:-/tmp}/bear3d_d_work}
OUT=../out/bear3d/anim_d; POUT=../out/bear3d/props_d
if [ -e "$WK" ] && [ ! -f "$WK/.bear3d_d_work" ]; then echo "refusing to clear $WK: not a run_d.sh work dir"; exit 1; fi
rm -rf "$WK"; mkdir -p "$WK/raw" "$WK/px"; touch "$WK/.bear3d_d_work"
# Blender exits 0 on a Python error: a missing output is the failure signal
$B -b bear.blend -P rig_bear.py -- --stage l --legs --out "$WK/bear_rig_l.blend" --clips "$WK/clips_l_scratch.json" \
  | grep -E "HEAD|drift|SAVED|Error|Traceback" || true
[ -f "$WK/bear_rig_l.blend" ] || { echo "rig failed (rig_bear.py --stage l --legs)"; exit 1; }
for o in A B; do
  A="--ori $o --raw $WK/raw --px $WK/px"
  for m in layout props clip; do
    $B -b "$WK/bear_rig_l.blend" -P render_d.py -- $A --mode $m --clip all 2>&1 | grep -E "LAYOUT|DONE|Error|Traceback" || true
  done
  for c in desk_sit desk_type desk_swivel_in desk_talk desk_ask desk_swivel_out desk_stand; do
    [ -f "$WK/px/desk_$o/clip_$c.json" ] || { echo "render failed: $c $o"; exit 1; }
  done
  python3 pixelize_d.py "$WK/raw/desk_$o" "$WK/px/desk_$o"
done
python3 make_d.py "$WK/px" "$WK/anim_d" "$WK/props_d"   # exits 1 on a hand-off mismatch: the old outputs stay
# swap in the new outputs, keeping the old ones (and face.json) in WK until face_d + the checks pass
FJ=../out/bear3d/face/face.json
mkdir -p "$WK/old"; cp "$FJ" "$WK/old/face.json"
[ -d "$OUT" ] && mv "$OUT" "$WK/old/anim_d"; [ -d "$POUT" ] && mv "$POUT" "$WK/old/props_d"
mv "$WK/anim_d" "$OUT"; mv "$WK/props_d" "$POUT"
if ! { python3 face_d.py && python3 verify_d.py && python3 verify_face.py; }; then
  echo "Stage D checks failed: restoring the previous anim_d/, props_d/ and face.json"
  rm -rf "$OUT" "$POUT"; cp "$WK/old/face.json" "$FJ"
  [ -d "$WK/old/anim_d" ] && mv "$WK/old/anim_d" "$OUT"; [ -d "$WK/old/props_d" ] && mv "$WK/old/props_d" "$POUT"
  exit 1
fi
rm -rf "$WK"
