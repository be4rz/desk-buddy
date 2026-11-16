"""export: python3 export.py -> rooms/starter/ (room.json + PNGs), the room pack the app loads.
Copies the pixelized art in out/ as it is (no pixel changes); `sh build.sh` runs the whole chain
(render, pixelize, this, verify)."""
import os, json, shutil
HERE = os.path.dirname(os.path.abspath(__file__)); OUT = os.path.join(HERE, 'out')
ROOM = os.path.normpath(os.path.join(HERE, '..', '..', 'rooms', 'starter'))
FLOORS = [('tatami', 'Tatami'), ('parquet', 'Parquet'), ('tile', 'Checker tile'), ('moss', 'Moss garden'), ('snow', 'Snow')]
# scene id, name, the mascot behaviors (or sets) it appears for, variants (name, hours [from, to) or None)
SCENES = [
    ('nook', 'Window nook', ['laptop', 'notebook'], [('dawn', [5, 8]), ('day', [8, 17]), ('dusk', [17, 20]), ('night', [20, 5])]),
    ('kitchen', 'Mini kitchen', ['snack'], [('base', None)]),
    ('tearoom', 'Tea room', ['tea'], [('base', None)]),
    ('grove', 'Bamboo grove', ['sit_floor'], [('day', [6, 19]), ('night', [19, 6])]),
    ('library', 'Fireside library', ['read'], [('evening', None)]),
]
PALETTES = ['../../experiments/env-proto/palette-env.json', '../../experiments/scene-proto/palette.json',
            '../../experiments/bear-proto/palette-bear.json', '../../experiments/mascot/bear3d/palette-ext.json',
            '../../experiments/mascot/bear3d/palette-ext-out.json']


def main():
    for d in ('floors', 'scenes'):
        shutil.rmtree(os.path.join(ROOM, d), ignore_errors=True); os.makedirs(os.path.join(ROOM, d))
    room = dict(room_version=1, id='starter', name='Starter room', palettes=PALETTES, floors=[], scenes=[])

    def copy(src, dst):
        shutil.copyfile(os.path.join(OUT, src), os.path.join(ROOM, dst)); return dst

    for fid, name in FLOORS:
        meta = json.load(open(os.path.join(OUT, 'floor_%s_s' % fid, 'meta.json')))
        room['floors'].append(dict(id=fid, name=name, file=copy('floor_%s_s/base__all.png' % fid, 'floors/%s.png' % fid), origin=meta['origin']))
    for sid, name, for_, variants in SCENES:
        meta = json.load(open(os.path.join(OUT, 'scene_' + sid, 'meta.json'))); vs = []
        assert set(v for v, _ in variants) == set(k or 'base' for k in meta['variants']), (sid, meta['variants'])
        for v, hours in variants:
            e = dict(name=v, back=copy('scene_%s/%s__all.png' % (sid, v), 'scenes/%s_%s.png' % (sid, v)))
            if hours: e['hours'] = hours
            if 'front' in meta['layers']: e['front'] = copy('scene_%s/%s__front.png' % (sid, v), 'scenes/%s_%s_front.png' % (sid, v))
            vs.append(e)
        room['scenes'].append({'id': sid, 'name': name, 'for': for_, 'origin': meta['origin'], 'variants': vs})
    with open(os.path.join(ROOM, 'room.json'), 'w') as f: json.dump(room, f, indent=2); f.write('\n')
    print('wrote', ROOM, len(room['floors']), 'floors', len(room['scenes']), 'scenes')


if __name__ == '__main__': main()
