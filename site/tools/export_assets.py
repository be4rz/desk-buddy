#!/usr/bin/env python3
"""Export the bear's pipeline art into the landing page (site/assets/).

The site never draws the bear: every sprite is a clip rendered by the Blender pipeline
(experiments/mascot/out/bear3d/) and every room piece comes from rooms/starter/. This script
copies a curated subset, packs each clip's directions (or set placements) into one sheet
(one row per direction, one column per frame), stores it as an exact palette PNG, and writes
site/assets/bear.json, the manifest site/js/sprites.js reads.

Run it again after any re-render:  python3 site/tools/export_assets.py
It is deterministic: the same art gives the same bytes.
"""
import json
import os
import shutil
import sys

import numpy as np
from PIL import Image

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
ART = os.path.join(ROOT, "experiments", "mascot", "out", "bear3d")
ROOM = os.path.join(ROOT, "rooms", "starter")
OUT = os.path.join(ROOT, "site", "assets")
BEAR_OUT = os.path.join(OUT, "bear")
ROOM_OUT = os.path.join(OUT, "room")

DIRS8 = ["S", "SE", "E", "NE", "N", "NW", "W", "SW"]
FRONT = ["S", "SE", "SW"]  # gestures face the viewer; the page turns the bear to one of these first

# (stage json, clip names, rows). Rows are directions for clips and placements for sets.
CLIPS = [
    ("anim/anim.json", ["idle", "walk_start", "walk", "walk_stop"], DIRS8),
    ("anim_l/anim_l.json", ["turn_cw", "turn_ccw"], DIRS8),
    ("anim_l/anim_l.json", ["listen_in", "listen", "listen_out"], FRONT),
    ("anim_c/anim_c.json", ["think", "talk", "ask", "oops", "happy", "celebrate"], FRONT),
    ("anim_e/anim_e.json", ["wave", "nod", "shake", "greet", "sad", "surprised", "jump", "remind"], FRONT),
    ("anim_f/anim_f.json", ["fid_weight_shift", "fid_ear_twitch", "fid_look_up", "fid_look_down",
                            "fid_tummy_rub", "fid_wave_small", "fid_peek"], FRONT),
    ("anim_g/anim_g.json", ["fid_rock_heels", "fid_head_tilt", "fid_scratch", "stretch", "fid_yawn",
                            "fid_shake_off", "fid_sneeze", "fid_nod_off"], FRONT),
    ("anim_r/anim_r.json", ["head_bob", "foot_tap", "dance", "cursor_watch", "flinch", "giggle",
                            "check_watch"], FRONT),
    ("anim_s/anim_s.json", ["dangle", "land_squash", "dizzy", "morning_stretch", "evening_yawn",
                            "tap_glass"], FRONT),
    ("anim_w/anim_w.json", ["edge_sit", "edge_dangle", "edge_peer", "edge_doze", "edge_stand"], FRONT),
    ("anim_x/anim_x.json", ["shrug", "facepalm", "thumbs_up", "clap", "point_bubble", "shy_hide",
                            "laugh", "pout", "proud", "head_scratch"], FRONT),
    ("anim_t/anim_t.json", ["hop", "spin"], FRONT),
    ("anim_z/anim_z.json", ["windmill", "hiccups", "peekaboo", "find_coin", "snowflake_nose",
                            "butterfly_ear", "hat_spring", "hat_summer", "hat_autumn", "hat_winter"], ["S"]),
    ("anim_z/anim_z.json", ["moonwalk"], ["SE", "SW"]),
    ("anim_p/anim_p.json", ["sit_floor_down", "sit_floor", "sit_floor_up", "curl_down", "curl_nap",
                            "curl_up", "belly_down", "belly_kick", "belly_up"], FRONT),
    # sets: every frame is the whole set (props + bear) baked on one canvas
    ("anim_b/anim_b.json", ["sit_down", "read", "stand_up"], ["A"]),
    ("anim_d/anim_d.json", ["desk_sit", "desk_type", "desk_swivel_in", "desk_talk", "desk_ask",
                            "desk_swivel_out", "desk_stand"], ["A"]),
    ("set_sit_a/anim_sit_a.json", ["tea_sit", "tea_sip", "tea_stand", "snack_sit", "snack_munch",
                                   "snack_stand", "knit_sit", "knit", "knit_stand"], ["A"]),
    ("set_sit_b/anim_sit_b.json", ["note_sit", "note_doodle", "note_stand", "laptop_sit", "laptop_type",
                                   "laptop_stand", "phones_on", "phones_vibe", "phones_off",
                                   "umbrella_open", "umbrella_rain", "umbrella_close"], ["A"]),
    ("set_play/anim_play.json", ["ball_get", "ball_bounce", "ball_drop", "bubbles_get", "bubbles_blow",
                                 "bubbles_put", "yoyo_get", "yoyo", "yoyo_put", "blocks_sit",
                                 "blocks_build", "blocks_stand", "balloon_get", "balloon_bob",
                                 "balloon_let_go"], ["A"]),
    ("set_out/anim_out.json", ["plant_get", "plant_water", "plant_put", "sweep_get", "sweep", "sweep_put",
                               "snow_kneel", "snow_build", "snow_stand", "fish_sit", "fish_wait",
                               "fish_stand", "cap_get_in_bed", "cap_sleep", "cap_wake_up"], ["A"]),
]

FACE_CLIPS = ["idle", "talk"]
FACE_LAYERS = [("eyes", "closed"), ("mouth", "open"), ("mouth", "wide")]


def rgba(path):
    return np.asarray(Image.open(path).convert("RGBA"))


def save_exact(arr, path):
    """Save an RGBA array as a palette PNG with the exact same pixels (alpha is 0 or 255)."""
    h, w, _ = arr.shape
    flat = arr.reshape(-1, 4).copy()
    flat[flat[:, 3] == 0] = 0  # every transparent pixel is one entry
    if np.any((flat[:, 3] != 0) & (flat[:, 3] != 255)):
        raise SystemExit(f"{path}: alpha other than 0/255")
    colors, idx = np.unique(flat, axis=0, return_inverse=True)
    if len(colors) > 256:
        Image.fromarray(arr, "RGBA").save(path, optimize=True)
        return
    # put the transparent entry first so tRNS stays one byte long
    order = np.argsort(colors[:, 3], kind="stable")
    remap = np.empty_like(order)
    remap[order] = np.arange(len(order))
    colors = colors[order]
    im = Image.fromarray(remap[idx.reshape(-1)].reshape(h, w).astype(np.uint8), "P")
    im.putpalette(colors[:, :3].reshape(-1).tolist())
    im.info["transparency"] = bytes(int(a) for a in colors[:, 3])
    im.save(path, optimize=True, transparency=bytes(int(a) for a in colors[:, 3]))
    back = np.asarray(Image.open(path).convert("RGBA")).copy()
    back[back[:, :, 3] == 0] = 0
    want = arr.copy()
    want[want[:, :, 3] == 0] = 0
    if not np.array_equal(back, want):
        raise SystemExit(f"{path}: palette round trip changed pixels")


def strip_frames(arr, w, h, n):
    """A strip may wrap onto several rows; return the frames in order."""
    cols = arr.shape[1] // w
    out = []
    for i in range(n):
        x, y = (i % cols) * w, (i // cols) * h
        out.append(arr[y:y + h, x:x + w])
    return out


def root_track(clip, row):
    frames = clip.get("frames") or []
    track = [f.get("root_px", {}).get(row) for f in frames]
    if not track or any(t is None for t in track):
        track = (clip.get("root_px") or {}).get(row)
    if not track:
        return None
    track = [[round(float(x), 4), round(float(y), 4)] for x, y in track]
    return track if any(x or y for x, y in track) else None


def export_clips(manifest):
    for rel, names, rows in CLIPS:
        stage_path = os.path.join(ART, rel)
        stage = json.load(open(stage_path))
        base = os.path.dirname(stage_path)
        for name in names:
            clip = stage["anims"][name]
            files = clip["files"]
            if isinstance(files, list):
                files = {r: f"{name}_{r}.png" for r in files}
            if "set" in clip:
                geo = stage["sets"][clip["set"]][rows[0]]
                w, h, (ax, ay) = geo["frame_w"], geo["frame_h"], geo["anchor"]
            else:
                w, h, (ax, ay) = clip["frame_w"], clip["frame_h"], clip["anchor"]
            n = clip["frames_n"]
            have = [r for r in rows if r in files]
            if not have:
                raise SystemExit(f"{rel} {name}: none of {rows} rendered")
            sheet = np.zeros((h * len(have), w * n, 4), np.uint8)
            roots = {}
            for ri, r in enumerate(have):
                arr = rgba(os.path.join(base, files[r]))
                for fi, fr in enumerate(strip_frames(arr, w, h, n)):
                    if fr.shape[:2] != (h, w):
                        raise SystemExit(f"{rel} {name} {r}: frame {fi} is {fr.shape}, want {h}x{w}")
                    sheet[ri * h:(ri + 1) * h, fi * w:(fi + 1) * w] = fr
                t = root_track(clip, r)
                if t:
                    roots[r] = t[:n]
            out = os.path.join(BEAR_OUT, f"{name}.png")
            save_exact(sheet, out)
            # the opaque box over every frame of every row, in frame pixels: [x0, y0, x1, y1)
            cover = sheet[:, :, 3].reshape(len(have), h, n, w).any(axis=(0, 2))
            ys, xs = np.nonzero(cover)
            entry = {"file": f"bear/{name}.png", "w": w, "h": h, "ax": ax, "ay": ay, "n": n,
                     "loop": bool(clip.get("loop")), "rows": have,
                     "bbox": [int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1]}
            if roots:
                entry["root"] = roots
            if "set" in clip:
                entry["set"] = clip["set"]
                sets = manifest.setdefault("sets", {})
                if clip["set"] not in sets:
                    geo = stage["sets"][clip["set"]][have[0]]
                    sets[clip["set"]] = {"w": geo["frame_w"], "h": geo["frame_h"],
                                         "ax": geo["anchor"][0], "ay": geo["anchor"][1]}
                    pf = geo.get("props_frame")
                    if pf:
                        out_p = f"bear/props_{clip['set']}.png"
                        save_exact(rgba(os.path.join(base, pf)), os.path.join(OUT, out_p))
                        sets[clip["set"]]["props"] = out_p
                ho = (clip.get("handoff") or {}).get(have[0]) if isinstance(clip.get("handoff"), dict) else None
                if ho:
                    # where the standing bear's feet are, from the set anchor, when this clip starts/ends
                    entry["handoff"] = {"at": ho["at"], "px": ho["px"], "facing": ho["facing"]}
                    sets[clip["set"]].setdefault("handoff", ho["px"])
                    sets[clip["set"]].setdefault("facing", ho["facing"])
            if name in FACE_CLIPS:
                entry["face"] = name
            manifest["clips"][name] = entry


def export_turns(manifest):
    """Which direction each 45-degree turn clip ends in, found by matching its last frame to idle."""
    idle = json.load(open(os.path.join(ART, "anim", "anim.json")))["anims"]["idle"]
    rest = {d: rgba(os.path.join(ART, "anim", idle["files"][d]))[:123, :84].astype(int) for d in DIRS8}
    stage = json.load(open(os.path.join(ART, "anim_l", "anim_l.json")))["anims"]
    turns = {}
    for kind in ("cw", "ccw"):
        clip = stage[f"turn_{kind}"]
        turns[kind] = {}
        for d in DIRS8:
            arr = rgba(os.path.join(ART, "anim_l", clip["files"][d]))
            last = strip_frames(arr, 84, 123, clip["frames_n"])[-1].astype(int)
            best = min(DIRS8, key=lambda t: np.abs(rest[t] - last).sum() if t != d else 1 << 60)
            turns[kind][d] = best
    for kind in ("cw", "ccw"):
        if sorted(turns[kind].values()) != sorted(DIRS8):
            raise SystemExit(f"turn_{kind} does not visit every direction once: {turns[kind]}")
    manifest["turn"] = turns


def export_face(manifest):
    face = json.load(open(os.path.join(ART, "face", "face.json")))
    layers = {}
    for slot, state in FACE_LAYERS:
        key = f"{slot}_{state}"
        layers[key] = {}
        for d in FRONT:
            meta = face["layers"][slot][state][d]
            out = f"face_{key}_{d}.png"
            save_exact(rgba(os.path.join(ART, "face", meta["file"])), os.path.join(BEAR_OUT, out))
            layers[key][d] = {"file": f"bear/{out}", "w": meta["w"], "h": meta["h"]}
    clips = {}
    for c in FACE_CLIPS:
        clips[c] = {d: {"eyes": face["clips"][c][d]["eyes"], "mouth": face["clips"][c][d]["mouth"]}
                    for d in FRONT if d in face["clips"][c]}
    manifest["face"] = {"layers": layers, "clips": clips}


def export_room(manifest):
    room = json.load(open(os.path.join(ROOM, "room.json")))
    floors = []
    for f in room["floors"]:
        out = f"room/floor_{f['id']}.png"
        save_exact(rgba(os.path.join(ROOM, f["file"])), os.path.join(OUT, out))
        floors.append({"id": f["id"], "name": f["name"], "file": out, "origin": f["origin"]})
    scenes = []
    for s in room["scenes"]:
        variants = []
        for v in s["variants"]:
            entry = {"name": v["name"]}
            for layer in ("back", "front"):
                if layer in v:
                    out = f"room/{os.path.basename(v[layer])}"
                    save_exact(rgba(os.path.join(ROOM, v[layer])), os.path.join(OUT, out))
                    entry[layer] = out
            if "hours" in v:
                entry["hours"] = v["hours"]
            variants.append(entry)
        scenes.append({"id": s["id"], "name": s["name"], "for": s["for"], "origin": s["origin"],
                       "variants": variants})
    manifest["room"] = {"floors": floors, "scenes": scenes}


def export_icons():
    """Favicons cut from the real idle sprite (frame 0, facing the viewer): the bear's head."""
    idle = rgba(os.path.join(ART, "anim", "idle_S.png"))[:123, :84]
    ys, xs = np.nonzero(idle[:, :, 3])
    top = ys.min()
    head = idle[top:top + 64]
    hy, hx = np.nonzero(head[:, :, 3])
    x0, x1 = hx.min(), hx.max() + 1
    side = max(x1 - x0, 1)
    side = max(side, 56)
    cx = (x0 + x1) // 2
    crop = np.zeros((side, side, 4), np.uint8)
    src = head[:side, max(cx - side // 2, 0):max(cx - side // 2, 0) + side]
    crop[:src.shape[0], :src.shape[1]] = src
    im = Image.fromarray(crop, "RGBA")
    big = im.resize((side * 3, side * 3), Image.NEAREST)
    big.save(os.path.join(OUT, "icon-head.png"), optimize=True)
    # apple touch icon: the head on paper, centred, nearest-neighbour only
    touch = Image.new("RGBA", (180, 180), (241, 239, 244, 255))
    k = max(1, 150 // side)
    head_k = im.resize((side * k, side * k), Image.NEAREST)
    touch.alpha_composite(head_k, ((180 - head_k.width) // 2, (180 - head_k.height) // 2 + 6))
    touch.convert("RGB").save(os.path.join(OUT, "apple-touch-icon.png"), optimize=True)
    # favicon: 64 px, nearest-neighbour; browsers scale it for the tab
    fav = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    fav.alpha_composite(im)
    fav.resize((64, 64), Image.NEAREST).save(os.path.join(OUT, "favicon.png"), optimize=True)
    # the full bear for the page (frame 0 at 1x, used by the app icon line in the footer)
    Image.fromarray(idle, "RGBA").save(os.path.join(OUT, "bear-idle.png"), optimize=True)


def main():
    if not os.path.isdir(ART):
        sys.exit(f"no art at {ART}: render the bear first (experiments/mascot/bear3d)")
    for d in (BEAR_OUT, ROOM_OUT):
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d)
    manifest = {
        "note": "Written by site/tools/export_assets.py from the bear pipeline output; do not edit. "
                "Each clip is one sheet: a row per direction or set placement (rows), a column per "
                "frame. (ax, ay) is the anchor pixel (the feet, or a set's origin). root[row] is the "
                "per-frame root motion in pixels, applied after showing that frame.",
        "fps": 12,
        "dirs8": DIRS8,
        "clips": {},
    }
    export_clips(manifest)
    export_turns(manifest)
    export_face(manifest)
    export_room(manifest)
    export_icons()
    with open(os.path.join(OUT, "bear.json"), "w") as f:
        json.dump(manifest, f, separators=(",", ":"), sort_keys=True)
    total = sum(os.path.getsize(os.path.join(dp, fn)) for dp, _, fns in os.walk(OUT) for fn in fns
                if not fn.endswith(".woff2"))
    print(f"{len(manifest['clips'])} clips, {total / 1e6:.2f} MB in site/assets (fonts excluded)")
    print("turns:", manifest["turn"])


if __name__ == "__main__":
    main()
