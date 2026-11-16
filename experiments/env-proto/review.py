"""review page: python3 review.py -> out/review/index.html + animated WebPs (scene + the bear's loop clip, 12 fps)."""
import os, json, html
from PIL import Image
import view
HERE = view.HERE; OD = os.path.join(HERE, 'out', 'review'); os.makedirs(OD, exist_ok=True)
FLOORS = [('floor_tatami_s', 'A · Tatami'), ('floor_parquet_s', 'B · Herringbone parquet'), ('floor_tile_s', 'C · Checker tile'),
          ('floor_moss_s', 'D · Moss garden'), ('floor_snow_s', 'E · Snow drift')]
SCENES = [('scene_nook', '1 · Window nook', 'laptop / notebook / reading'), ('scene_kitchen', '2 · Mini kitchen', 'snack / eating'),
          ('scene_tearoom', '3 · Tea room', 'tea'), ('scene_grove', '4 · Bamboo grove', 'sitting / napping outside'),
          ('scene_library', '5 · Fireside library', 'reading')]


def anim(name, variant):
    meta = json.load(open(os.path.join(HERE, 'out', name, 'meta.json'))); b = meta.get('bear')
    n = view.bear_frame(b)[2] if b else 1
    frames = [view.compose(name, variant, f) for f in range(n)]
    fn = '%s__%s.webp' % (name, variant or 'base')
    frames[0].save(os.path.join(OD, fn), save_all=True, append_images=frames[1:], duration=83, loop=0, lossless=True)
    return fn, frames[0].size, meta


def card(fn, size, title, sub, z):
    return ('<figure><div class="art"><img src="%s" width="%d" height="%d" alt="%s"></div><figcaption><b>%s</b><span>%s</span></figcaption></figure>'
            % (fn, size[0] * z, size[1] * z, html.escape(title), html.escape(title), html.escape(sub)))


rows = ['<h2>Floors</h2><p>Reusable floor patches; any mascot can stand on them. Shown with the idle loop and a contact shadow.</p><div class="row">']
for n, t in FLOORS:
    fn, sz, meta = anim(n, None); rows.append(card(fn, sz, t, meta['note'], 3))
rows.append('</div><h2>Pocket scenes</h2><p>Each one appears around the bear while he does the activity, then goes away.</p>')
for n, t, act in SCENES:
    meta = json.load(open(os.path.join(HERE, 'out', n, 'meta.json'))); rows.append('<h3>%s <small>%s</small></h3><p>%s</p><div class="row">' % (t, act, html.escape(meta['note'])))
    for v in meta['variants']:
        fn, sz, _ = anim(n, v or None); rows.append(card(fn, sz, v or 'base', '%d×%d px' % sz, 2))
    rows.append('</div>')
page = '''<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Floors and scenes</title>
<style>
:root{--bg:#2b2e3a;--fg:#eceef4;--mut:#9aa0b4;--card:#3a3e4e}
body{margin:0;padding:24px 16px 64px;background:var(--bg);color:var(--fg);font:15px/1.5 -apple-system,system-ui,sans-serif}
body.light{--bg:#eef0f4;--fg:#22242c;--mut:#666c80;--card:#dfe3ea} body.photo{--bg:#5b7f9c;--card:#6b8fab;--mut:#dfe8f0}
main{max-width:1500px;margin:auto} h1{margin:0 0 4px} h2{margin:40px 0 4px} h3{margin:28px 0 2px} h3 small{color:var(--mut);font-weight:400;margin-left:8px}
p{margin:0 0 12px;color:var(--mut)} .row{display:flex;flex-wrap:wrap;gap:16px;align-items:flex-end}
figure{margin:0;background:var(--card);border-radius:10px;padding:12px;max-width:100%} .art{overflow-x:auto}
img{image-rendering:pixelated;display:block} figcaption{margin-top:8px;display:flex;flex-direction:column} figcaption span{color:var(--mut);font-size:13px}
button{font:inherit;padding:4px 12px;margin-right:6px;border-radius:6px;border:1px solid var(--mut);background:none;color:var(--fg);cursor:pointer}
</style><main><h1>Floors and pocket scenes</h1><p>Prototypes, Blender-rendered and pixelized. Pick by letter / number.</p>
<p>Backdrop: <button onclick="document.body.className=''">dark</button><button onclick="document.body.className='light'">light</button><button onclick="document.body.className='photo'">blue</button></p>
@ROWS@</main>'''.replace('@ROWS@', '\n'.join(rows))
open(os.path.join(OD, 'index.html'), 'w').write(page); print(os.path.join(OD, 'index.html'))
