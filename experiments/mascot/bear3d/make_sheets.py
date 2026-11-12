"""Writes experiments/mascot/out/bear3d/sheets.html: one index of every stage sheet and preview of the
aliveness run (A4-A6), with each clip's gif beside idle, for the user's cuteness review (AC16).
Usage: python3 make_sheets.py [OUT_DIR] (default ../out/bear3d); re-run it after a re-render or a new stage."""
import json, os, sys, html
OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "out", "bear3d")
STAGES = [  # (dir, anim json, title, phase, what)
 ("set_lean", "anim_lean.json", "lean", "p1 · A4", "leans on a big toy block"),
 ("anim_f", "anim_f.json", "f · fidgets", "p2 · A4", "weight shift, ear twitch, tummy rub, small wave, look up/down, peek, butt wiggle"),
 ("anim_g", "anim_g.json", "g · fidgets", "p2 · A4", "rock on heels, head tilt, scratch, stretch, yawn, shake off, sneeze, nod off"),
 ("anim_p", "anim_p.json", "p · floor postures", "p2 · A4", "sit, sway, lie on the belly or back, curl up, squat"),
 ("anim_m", "anim_m.json", "m · gaits", "p2 · A4", "tiptoe, skip, shuffle, back waddle"),
 ("anim_t", "anim_t.json", "t · tumbles", "p2 · A4", "hop, spin, roll, trip, pounce, slide stop"),
 ("anim_e", "anim_e.json", "e · gestures", "p3 · A5", "wave, nod, shake, greet, sad, surprised, jump, dodge, remind"),
 ("anim_x", "anim_x.json", "x · gestures", "p3 · A5", "shrug, facepalm, thumbs up, clap, point, shy, laugh, pout, proud, head scratch"),
 ("anim_r", "anim_r.json", "r · reactions", "p4 · A6", "dangle, land, dizzy, flinch, giggle, cursor watch, morning stretch"),
 ("anim_s", "anim_s.json", "s · reactions", "p4 · A6", "head bob, foot tap, dance, evening yawn, tap glass, check watch"),
 ("anim_w", "anim_w.json", "w · window edge", "p4 · A6", "sit on a window edge, dangle, peer, doze, stand"),
 ("set_sit_a", "anim_sit_a.json", "sit_a · cushion props", "p5 · A6", "tea, snack, knit"),
 ("set_sit_b", "anim_sit_b.json", "sit_b · table and standing props", "p5 · A6", "notebook, laptop, headphones, umbrella"),
 ("set_play", "anim_play.json", "play · toys", "p5 · A6", "ball, bubbles, yoyo, blocks, balloon"),
 ("set_out", "anim_out.json", "out · chores and outdoors", "p5 · A6", "plant, sweep, snowman, fishing, nightcap"),
 ("anim_z", "anim_z.json", "z · rare surprises", "p5 · A6", "windmill, hiccups, coin, snowflake, butterfly, peekaboo, moonwalk, four seasonal hats"),
]
FRONT = ["S", "SE", "SW", "E", "W", "N", "NE", "NW", "A", "B"]
def esc(s): return html.escape(s, quote=True)
sections, missing = [], []
for d, aj, title, phase, what in STAGES:
    path = os.path.join(OUT, d, aj)
    if not os.path.exists(path):
        missing.append(d); continue
    anims = json.load(open(path))["anims"]
    sid = d.split("_", 1)[1]
    files = sorted(os.listdir(os.path.join(OUT, d)))
    sheet = next((f for f in files if f.startswith("sheet") and f.endswith(".png")), None)
    prev = next((f for f in files if f.startswith("preview") and f.endswith(".html")), None)
    cells = []
    for name, a in anims.items():
        dirs = a.get("files", {})
        pick = [k for k in FRONT if k in dirs][:2] if d.startswith("set_") else [k for k in FRONT if k in dirs][:1]
        for k in pick:
            g = f"{name}_{k}.gif"
            if g in files:
                loop = " loop" if a.get("loop") else ""
                cells.append(f'<figure><img src="{d}/{esc(g)}" alt="{esc(name)} {k}" loading="lazy"><figcaption>{esc(name)} <b>{k}</b>{loop}</figcaption></figure>')
    links = " · ".join(x for x in [
        f'<a href="{d}/{esc(sheet)}">sheet {esc(sheet)}</a>' if sheet else "",
        f'<a href="{d}/{esc(prev)}">preview</a>' if prev else ""] if x)
    sections.append(f'''<section id="{d}">
<h2>{esc(title)} <small>{esc(phase)} · <code>{d}/</code> · {len(anims)} clips</small></h2>
<p>{esc(what)}. {links}</p>
<div class="row"><figure class="idle"><img src="anim/idle_S.gif" alt="idle S"><figcaption>idle <b>S</b> (reference)</figcaption></figure>{"".join(cells)}</div>
</section>''')
toc = " ".join(f'<a href="#{d}">{esc(t.split(" ·")[0])}</a>' for d, _, t, _, _ in STAGES if d not in missing)
page = f'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Bear sheets</title>
<style>
:root {{ --bg:#f4f1ea; --fg:#222; --muted:#666; --card:#fff; --line:#ddd; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#1d1d20; --fg:#eee; --muted:#aaa; --card:#2a2a2e; --line:#444; }} }}
body {{ background:var(--bg); color:var(--fg); font:15px/1.45 system-ui, sans-serif; margin:0 auto; max-width:1200px; padding:16px; }}
h1 {{ margin:0 0 4px; }} h2 {{ margin:28px 0 4px; font-size:19px; }} small {{ color:var(--muted); font-weight:normal; font-size:13px; }}
nav a {{ margin-right:10px; }} a {{ color:inherit; }}
.row {{ display:flex; flex-wrap:wrap; gap:8px; }}
figure {{ margin:0; background:var(--card); border:1px solid var(--line); border-radius:6px; padding:6px; text-align:center; }}
figure.idle {{ border-color:#c9a227; }}
img {{ image-rendering:pixelated; width:auto; height:150px; display:block; margin:0 auto; }}
figcaption {{ font-size:12px; color:var(--muted); max-width:180px; overflow-wrap:anywhere; }}
</style></head><body>
<h1>Bear sheets: aliveness run (A4-A6)</h1>
<p>Every stage rendered in this run, each clip's gif next to <code>idle</code>, scaled up with pixelated rendering.
Judge cuteness here and then on the real desktop (AC16). Serve with
<code>python3 -m http.server 8771 --directory experiments/mascot/out</code> and open <code>bear3d/sheets.html</code>.
The full sheet PNGs are big (up to 18 000 px tall): open them one at a time.</p>
<nav>{toc}</nav>
{"".join(sections)}
{"<p><small>Not rendered yet: " + ", ".join(missing) + "</small></p>" if missing else ""}
</body></html>
'''
open(os.path.join(OUT, "sheets.html"), "w").write(page)
print("sections", len(sections), "missing", missing)
