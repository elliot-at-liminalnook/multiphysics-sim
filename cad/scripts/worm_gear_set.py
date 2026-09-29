"""Model the winch's worm set in CAD and derive its physics from the geometry.

    cd cad && .venv/bin/python scripts/worm_gear_set.py ../examples/systems-builder/worm-drive/cad [--worm-diameter 8]

Writes worm-set.rcad (the CAD model: steel worm, brass wheel) and
worm-set.physics.json (module, starts, teeth, pitch diameter measured from
the bodies; friction from the material pair). Bench it with:

    target/release/sim-system datasheet rotational.worm_gear --parameters examples/systems-builder/worm-drive/cad/worm-set.physics.json
"""
import argparse
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))
from robocad.document import Document  # noqa: E402
from robocad.gear_derivation import build_worm_set, export_physics  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('out')
    ap.add_argument('--module', type=float, default=0.5)
    ap.add_argument('--starts', type=int, default=1)
    ap.add_argument('--teeth', type=int, default=30)
    ap.add_argument('--worm-diameter', type=float, default=8.0)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    doc = Document()
    build_worm_set(doc, a.module, a.starts, a.teeth, a.worm_diameter)
    rcad = os.path.join(a.out, 'worm-set.rcad')
    doc.save(rcad)
    record = export_physics(doc, rcad, os.path.join(a.out, 'worm-set.physics.json'))
    print(json.dumps({k: v['value'] for k, v in record['parameters'].items()}, indent=1))


if __name__ == '__main__':
    main()
