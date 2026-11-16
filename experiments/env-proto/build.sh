#!/bin/sh
# sh build.sh   re-render every floor and scene (Blender), pixelize, write rooms/starter/ and verify it.
# The steps alone: sh run.sh floors|scenes [--only a,b]; python3 pix.py [name ...]; python3 export.py; python3 verify_env.py
set -e
cd "$(dirname "$0")"
sh run.sh floors
sh run.sh scenes
python3 pix.py
python3 export.py
python3 verify_env.py
