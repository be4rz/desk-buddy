"""Write ../out/bear3d/anim_c/preview_c.html: the Stage C clips (S, SE, E GIFs) and, once run_face.sh has run, the
face-layer contact sheets. Only links images that exist, so the page has no broken requests.

    python3 preview_c.py
"""
import os, json

HERE = os.path.dirname(os.path.abspath(__file__))
ART = os.path.join(HERE, '..', 'out', 'bear3d')
AC = os.path.join(ART, 'anim_c')
anims = json.load(open(os.path.join(AC, 'anim_c.json')))['anims']


def fig(src, cap): return f'<figure><img src="{src}" alt="{cap}"><figcaption>{cap}</figcaption></figure>'


parts = ['<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">',
         '<title>Bear Stage C</title><link rel="icon" href="data:,">',
         '<style>body{background:#1d1b24;color:#ece8f4;font:15px system-ui;padding:24px max(16px,4vw)}h1{font-size:20px}'
         'h2{font-size:15px;margin:22px 0 8px;font-family:ui-monospace,Menlo,monospace;color:#a9dccf}'
         '.row{display:flex;gap:12px;flex-wrap:wrap}figure{margin:0;background:#262331;border:1px solid #3a3548;'
         'border-radius:10px;padding:8px;text-align:center;max-width:100%}'
         'img{image-rendering:pixelated;display:block;max-width:100%;height:auto}'
         'figcaption{font:12px ui-monospace,Menlo,monospace;color:#9a93ad}p{color:#9a93ad;max-width:70ch}</style>',
         '<h1>Bear, Stage C: think / talk / ask / oops / happy / celebrate</h1>',
         '<p>Loops (think, talk, ask) and one-shots (oops, happy, celebrate) start, and one-shots end, on idle frame 0. '
         'Eyes stay open in the clips: blinking and talking come from the face layers below.</p>']
for c, a in anims.items():
    kind = 'loop' if a['loop'] else 'one-shot'
    parts.append(f'<section><h2>{c} ({kind}, {a["frames_n"]} frames)</h2><div class="row">'
                 + ''.join(fig(f'{c}_{d}.gif', d) for d in ('S', 'SE', 'E')) + '</div></section>')
parts.append('<section><h2>all clips, S / SE / E (2x)</h2>' + fig('contact_c.png', 'contact_c.png') + '</section>')
face = os.path.join(ART, 'face')
if os.path.exists(os.path.join(face, 'face.json')):
    parts.append('<h1>Face layers (D16)</h1><p>Each eyes / mouth state patch drawn over idle frame 0. The idle sheet '
                 'compares the shipped idle body, the body with the default patches, and the Blender reference.</p>')
    for f, cap in (('states_contact.png', 'states over idle frame 0: S, SE, E, SW, W'),
                   ('idle_compare.png', 'idle: body | body + default patches | reference'),
                   ('residual_worst.png', 'six worst tolerated frames: body | body + default | expected | residual')):
        if os.path.exists(os.path.join(face, f)):
            parts.append(f'<section><h2>{cap}</h2>' + fig(f'../face/{f}', f) + '</section>')
open(os.path.join(AC, 'preview_c.html'), 'w').write('\n'.join(parts) + '\n')
print('ok', os.path.join(AC, 'preview_c.html'))
