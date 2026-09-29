"""Whole or split? The object-scan camera stand (a post on a base plate).

    cad/.venv/bin/python examples/camera-turntable/print/stand_strength_split.py [OUT_DIR]

Printed upright, a sideways knock bends the post across its layers; lying
down, the post is strong but the base plate stands on edge and the post
floats on supports. The comparison plans the whole stand and the stand cut
at the post/base junction, each piece in its own best orientation.

Design assumption (not from a simulation): a 40 N sideways knock at the top
of the post, e.g. a hand or elbow catching it.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, os.path.join(REPO, 'cad'))
from robocad.document import Document  # noqa: E402
from robocad.print_strength_split import compare  # noqa: E402

RCAD = os.path.join(HERE, '..', 'cad', 'turntable.rcad')
KNOCK_N = 40.0


def main(out):
    doc = Document.load(RCAD)
    stand = next(n for n in doc.bodies() if n.name.startswith('Camera stand'))
    lo, hi = doc.mesh_of(stand.id).bounds()
    top = {'box': {'min': [lo[0] - 1, lo[1] - 1, hi[2] - 15], 'max': [hi[0] + 1, hi[1] + 1, hi[2] + 1]}}
    part = {'name': 'Camera stand', 'fixtures': [{'name': 'on the table', 'region': {'bottom': True}}],
            'loads': [{'name': 'a knock at the top', 'region': top, 'direction': [-1, 0, 0], 'magnitude': KNOCK_N}]}
    verdict = compare(doc, stand.id, part, out, space={'walls': [2, 3, 4], 'infill': [0.15, 0.3, 0.6], 'search_voxels': 6000},
                      voxels=30000, on_progress=lambda f, m: print(f'{f:.2f} {m}', flush=True))
    print(json.dumps({k: verdict[k] for k in ('recommendation', 'why')}, indent=1))
    w = verdict['whole']
    print('whole:', w['chosen']['build_direction'], w['chosen']['settings'], round(w['safety_factor'], 2), round(w['hours'], 2), 'h')
    for o in verdict['options']:
        if 'error' in o:
            print('cut', o['plane']['why'], o['error'])
            continue
        print('cut', o['plane']['why'], 'safety', round(o['safety_factor'], 2), round(o['hours'], 2), 'h', o['hardware'])
        for p in o['pieces']:
            print('   ', p['name'], p['chosen']['build_direction'], p['chosen']['settings']['walls'], p['chosen']['settings']['infill'], p['safety_factor'])
        for s in o['seams']:
            print('   seam', s['name'], s['safety_factor'], s['notes'])


if __name__ == '__main__':
    main(sys.argv[1] if len(sys.argv) > 1 else os.path.join(REPO, 'runs', 'camera-turntable', 'stand-strength'))
