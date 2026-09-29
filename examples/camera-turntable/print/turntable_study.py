"""Print study for the camera turntable's printed parts (``sim.print-study/1``).

    cad/.venv/bin/python examples/camera-turntable/print/turntable_study.py [OUT_DIR]
    target/release/sim-print analyze OUT_DIR/study.json      # strength as designed
    target/release/sim-print plan OUT_DIR/study.json         # orientation and settings
    cad/.venv/bin/python examples/camera-turntable/print/turntable_study.py [OUT_DIR] --plates   # 3MF plates

Loads come from two places, each recorded in the result:

* the turntable simulation (``turntable.system.json``): the belt's hub load
  (tight plus slack run tension), which pulls the ring toward the drive
  pulley, the pulley toward the ring, and loads the bearings and the cradle;
* design assumptions stated here: the heaviest object on the platform
  (``OBJECT_KG``) and an accidental knock on the camera mount.

Regions are found from the CAD geometry: faces of one body touching another
(the load path through a contact), and the faces a part stands on.
Output (STL + study.json) goes to runs/, which is not kept in git; this
script is what reproduces it.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, os.path.join(REPO, 'cad'))
from robocad.document import Document  # noqa: E402
from robocad import print_study as ps  # noqa: E402

RCAD = os.path.join(HERE, '..', 'cad', 'turntable.rcad')
SYSTEM = os.path.join(HERE, '..', 'turntable.system.json')
OBJECT_KG = 1.0          # design assumption: heaviest object to scan
KNOCK_N = 10.0           # design assumption: a hand knocking the camera mount sideways
SIM_SECONDS = 8.0        # two stop-and-shoot moves


def by_name(doc, start):
    found = [n for n in doc.bodies() if n.name.startswith(start)]
    if len(found) != 1:
        raise KeyError(f'{start!r}: {len(found)} bodies match')
    return found[0].id


def hub_load(scale=1.0, why=''):
    return {'observe': 'belt.hub_load', 'reduce': 'max_abs', 'scale': scale, 'why': why}


def build(doc, out_dir):
    base, disc, cradle = by_name(doc, 'Base and bearing post'), by_name(doc, 'Turntable disc'), by_name(doc, 'Servo cradle')
    servo, belt, platform = by_name(doc, 'HX-30HM servo'), by_name(doc, 'GT2 closed belt'), by_name(doc, 'Object platform')
    lower, upper = by_name(doc, '6808-2RS bearing (lower)'), by_name(doc, '6808-2RS bearing (upper)')
    mount, camera = by_name(doc, 'Camera mount'), by_name(doc, 'Camera Module 3 Wide')
    # The belt runs in the pulley's mid-plane; the bearings share its radial pull by lever arms.
    pulley = doc.nodes[by_name(doc, 'GT2 30T drive pulley')]
    (plo, phi) = doc.mesh_of(pulley.id).bounds()
    z_belt = (plo[2] + phi[2]) / 2
    zl = sum(doc.mesh_of(lower).bounds()[i][2] for i in (0, 1)) / 2
    zu = sum(doc.mesh_of(upper).bounds()[i][2] for i in (0, 1)) / 2
    upper_share = (z_belt - zl) / (zu - zl)
    lever = f'belt plane z={z_belt:.1f} mm between bearings at z={zl:.1f} and {zu:.1f} mm'
    toward_pulley = (1.0, 0.0, 0.0)   # the pulley sits on +x of the disc axis
    disc_kg = 0.0  # the disc's own weight enters through its acceleration; platform + object here
    weight = (OBJECT_KG + 0.06) * 9.81
    parts = [
        ps.PartSpec(cradle, fixtures=[{'name': 'screwed to the base', 'region': ps.bottom_region(doc, cradle)}],
                    loads=[{'name': 'belt pull through the servo', 'region': ps.contact_region(doc, cradle, servo, gap=0.4),
                            'direction': [-1, 0, 0], 'magnitude': hub_load(1.0, 'the belt pulls the pulley, and so the servo, toward the ring')}]),
        ps.PartSpec(base, fixtures=[{'name': 'standing on the table', 'region': ps.bottom_region(doc, base)}],
                    loads=[{'name': 'upper bearing: belt pull', 'region': ps.contact_region(doc, base, upper, gap=0.4),
                            'direction': list(toward_pulley), 'magnitude': hub_load(upper_share, f'upper bearing share {upper_share:.3f}: {lever}')},
                           {'name': 'lower bearing: belt pull', 'region': ps.contact_region(doc, base, lower, gap=0.4),
                            'direction': list(toward_pulley), 'magnitude': hub_load(1 - upper_share, f'lower bearing share {1 - upper_share:.3f}: {lever}')},
                           {'name': 'disc, platform and object weight', 'region': ps.contact_region(doc, base, lower, gap=0.4),
                            'direction': [0, 0, -1], 'magnitude': weight},
                           {'name': 'cradle reaction (belt pull)', 'region': ps.contact_region(doc, base, cradle, gap=0.4),
                            'direction': [-1, 0, 0], 'magnitude': hub_load(1.0, 'the cradle holds the servo against the belt')}]),
        ps.PartSpec(disc, fixtures=[{'name': 'on its bearings', 'region': ps.contact_region(doc, disc, upper, gap=0.4)},
                                    {'name': 'lower bearing', 'region': ps.contact_region(doc, disc, lower, gap=0.4)}],
                    loads=[{'name': 'belt wrapped on the ring', 'region': ps.contact_region(doc, disc, belt, gap=0.6),
                            'direction': list(toward_pulley), 'magnitude': hub_load(1.0, 'resultant of both belt runs on the ring')},
                           {'name': 'platform and object', 'region': ps.contact_region(doc, disc, platform, gap=0.4),
                            'direction': [0, 0, -1], 'magnitude': weight}]),
        ps.PartSpec(mount, fixtures=[{'name': 'screwed to the disc', 'region': ps.bottom_region(doc, mount)}],
                    loads=[{'name': 'a knock on the camera', 'region': ps.contact_region(doc, mount, camera, gap=0.6),
                            'direction': [1, 0, 0], 'magnitude': KNOCK_N}]),
    ]
    return ps.write_study(doc, parts, out_dir, simulation={'system': SYSTEM, 'seconds': SIM_SECONDS}, voxels=60000,
                          plan={'walls': [2, 3, 4], 'infill': [0.1, 0.15, 0.25, 0.4], 'search_voxels': 8000},
                          provenance={'cad': os.path.relpath(RCAD, REPO), 'object_kg': OBJECT_KG, 'knock_n': KNOCK_N,
                                      'disc_kg_note': 'the disc weighs itself through its acceleration (gravity)'})


def plates(doc, out_dir, printer='bambu-h2c'):
    """After `sim-print plan OUT/study.json`: lay the parts out on plates as 3MF."""
    import json
    from robocad.print_plan import write_plates
    plan = json.load(open(os.path.join(out_dir, 'print-plan', 'plan.json')))
    study = json.load(open(os.path.join(out_dir, 'study.json')))
    by_name = {n.name: n for n in doc.bodies()}
    pieces = []
    for part in plan['parts']:
        c, v = part['chosen'], part.get('verified') or {}
        pieces.append({'name': part['name'], 'body': by_name[part['name']].body, 'build_direction': c['build_direction'],
                       'settings': c['settings'], 'estimate': c['estimate'], 'safety_factor': v.get('safety_factor')})
    return write_plates(doc.kernel, pieces, os.path.join(out_dir, 'plates'), printer, study['material'], os.path.join(out_dir, 'print-plan', 'plan.json'))


if __name__ == '__main__':
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    out = args[0] if args else os.path.join(REPO, 'runs', 'camera-turntable', 'print')
    doc = Document.load(RCAD)
    if '--plates' in sys.argv:
        import json
        print(json.dumps(plates(doc, out), indent=1)[:4000])
    else:
        print(build(doc, out))
