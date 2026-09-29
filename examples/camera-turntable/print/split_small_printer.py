"""Split the turntable disc for a small printer (A1 mini, 176 mm usable) and
check every seam's joints against the loads the disc carries in the running
simulation.

    cad/.venv/bin/python examples/camera-turntable/print/split_small_printer.py [OUT_DIR]
    target/release/sim-print analyze OUT_DIR/study.json

The pieces are written as STL next to the study; the seam checks appear in
the result under the disc's `seams`; the assembly guide is in OUT_DIR/assembly and test coupons in OUT_DIR/coupons.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, os.path.join(REPO, 'cad'))
sys.path.insert(0, HERE)
from robocad.document import Document  # noqa: E402
from robocad import print_study as ps  # noqa: E402
from robocad.print_split import SplitOptions, split_for_printing  # noqa: E402
import turntable_study as ts  # noqa: E402

PRINTER = 'bambu-a1-mini'


def build(doc, out_dir):
    disc = ts.by_name(doc, 'Turntable disc')
    result = split_for_printing(doc, disc, SplitOptions(printer=PRINTER))
    os.makedirs(out_dir, exist_ok=True)
    k = doc.kernel
    from robocad.printing import weld
    for i, piece in enumerate(result.pieces):
        ps.write_stl(weld(k.tessellate(piece, 0.1)), os.path.join(out_dir, f'disc-piece-{i + 1}.stl'))
    with open(os.path.join(out_dir, 'split.json'), 'w') as f:
        json.dump(result.summary(k), f, indent=1)
    # The disc's own study (loads from the simulation), with the seams added.
    full = ts.build(doc, os.path.join(out_dir, 'whole'))
    study = json.load(open(full))
    part = next(p for p in study['parts'] if p['name'].startswith('Turntable disc'))
    part['seams'] = ps.seams_from_split(result)
    part['mesh'] = os.path.join('whole', part['mesh'])
    study['parts'] = [part]
    study['printer'] = PRINTER
    study['simulation']['system'] = os.path.relpath(os.path.join(HERE, '..', 'turntable.system.json'), out_dir)
    path = os.path.join(out_dir, 'study.json')
    json.dump(study, open(path, 'w'), indent=1)
    # Assembly: the same split as document nodes (in memory; the CAD file is not changed).
    from robocad.commands import Ops
    from robocad.print_split import apply_split
    from robocad.print_assembly import plan_assembly, write_guide
    group = apply_split(Ops(doc), result)
    guide = write_guide(doc, plan_assembly(doc, group), os.path.join(out_dir, 'assembly'))
    print(guide)
    # Coupons copying this split's joints, to measure before trusting the estimates.
    from robocad.print_coupons import write_coupon_kit
    kit = write_coupon_kit(doc.kernel, os.path.join(out_dir, 'coupons'), study['material'], PRINTER, doc.nodes[group].robot['print_split'])
    print(kit['protocol'])
    return path, result


if __name__ == '__main__':
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(REPO, 'runs', 'camera-turntable', 'print-a1-mini')
    path, result = build(Document.load(ts.RCAD), out)
    print(path)
    print(json.dumps(result.hardware()))
