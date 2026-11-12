#!/bin/sh
# D16 face layers: eyes open/closed and mouth closed/open/wide patches for the face directions (S SE E W SW), the
# per-frame face positions for idle + the Stage C clips, and the idle reference renders, then verify_face.py.
# Needs bear_rig_c.blend + clips_c.json (run_c.sh) and anim/ (run_anim.sh). Writes ../out/bear3d/face/.
#   sh run_face.sh [WORKDIR]     (WORKDIR = scratch for the render passes and pixel frames; removed at the end)
set -e
cd "$(dirname "$0")"
B=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
WK=${1:-${TMPDIR:-/tmp}/bear3d_face_work}
OUT=../out/bear3d/face
CANVAS=$(python3 -c "import json;a=json.load(open('../out/bear3d/anim/anim.json'))['anims']['idle'];print(a['frame_w'],a['frame_h'],a['anchor'][1],sep=',')")
YAWS=45,90,135,315,0     # S SE E W SW
rm -rf "$WK"; mkdir -p "$WK/px"
r() {  # tag clip [render_anim.py options]
  t=$1; c=$2; shift 2
  $B -b bear_rig_c.blend -P render_anim.py -- --raw "$WK" --tag $t --clip $c --canvas $CANVAS --yaws $YAWS "$@" \
    | grep -E "CANVAS|Error|Traceback" || true
  [ -f "$WK/${t}_$c/meta.json" ] || { echo "render failed: $t $c"; exit 1; }
  python3 pixelize_anim.py "$WK/${t}_$c" "$WK/px" >/dev/null; rm -rf "$WK/${t}_$c"
}
r rest face_rest
r closed face_eyes_closed
r open face_rest --hide mouth --show mouth_open
r wide face_rest --hide mouth --show mouth_wide,tongue
r ref idle_ref
rm -rf "$OUT"; python3 make_face.py "$WK/px" "$OUT" clips_c.json
# make_face.py writes face.json v1: re-add the Stage L (v2: clips.listen + patch_dirs) and Stage D (set_clips) parts
# before checking (stage-kit face parts, out/bear3d/*/face_*.json, are separate files and stay as they are)
[ -f ../out/bear3d/anim_l/anim_l.json ] && python3 face_l.py
[ -f ../out/bear3d/anim_d/anim_d.json ] && python3 face_d.py
python3 verify_face.py --sheet "$OUT/residual_worst.png"
python3 preview_c.py
rm -rf "$WK"
