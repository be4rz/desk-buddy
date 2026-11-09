#!/bin/sh
# Stage B: furniture props + sleep / read clips with the Stage A rig, camera and pixel step.
# Renders both sets (bed, read) in both orientations (A, B = 3D mirror), pixelizes every layer, assembles
# ../out/bear3d/anim_b/ and ../out/bear3d/props/, then runs the hand-off check. Needs bear_rig.blend (run_anim.sh).
#   sh run_b.sh [WORKDIR]     (WORKDIR = scratch for raw passes + pixel layers; removed at the end)
set -e
cd "$(dirname "$0")"
B=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
WK=${1:-${TMPDIR:-/tmp}/bear3d_b_work}
OUT=../out/bear3d/anim_b; POUT=../out/bear3d/props
rm -rf "$WK"; mkdir -p "$WK/raw" "$WK/px"
for s in bed read; do
  case $s in bed) CL="get_in_bed sleep wake_up";; read) CL="sit_down read stand_up";; esac
  for o in A B; do
    A="--set $s --ori $o --raw $WK/raw --px $WK/px"
    $B -b bear_rig.blend -P render_b.py -- $A --mode layout | grep -E "LAYOUT|Error|Traceback" || true
    SV=""; [ "$s$o" = bedA ] && SV="--save $PWD/bear_b.blend"
    $B -b bear_rig.blend -P render_b.py -- $A --mode props $SV | grep -E "DONE|SAVED|Error|Traceback" || true
    for c in $CL; do
      $B -b bear_rig.blend -P render_b.py -- $A --mode clip --clip $c | grep -E "DONE|Error|Traceback" || true
      python3 pixelize_b.py "$WK/raw/${s}_$o" "$WK/px/${s}_$o"
    done
  done
done
rm -rf "$OUT" "$POUT"
python3 make_b.py "$WK/px" "$OUT" "$POUT"
python3 verify_b.py "$OUT"
rm -rf "$WK"
