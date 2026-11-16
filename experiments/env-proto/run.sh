#!/bin/sh
# sh run.sh floors|scenes [--only a,b] [--bare]   -> raw/ (passes) -> out/ (pixel art)
cd "$(dirname "$0")"
B="${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}"
s="$1"; shift
"$B" -b -P "$s.py" -- "$@" 2>&1 | rg -v "^(Fra:|Saved:| Time:|Blender|Read |$)" 
