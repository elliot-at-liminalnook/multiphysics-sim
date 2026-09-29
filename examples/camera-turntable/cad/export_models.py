"""Export the turntable's display models for the physical viewer.

    cad/.venv/bin/python examples/camera-turntable/cad/export_models.py

Reads turntable.rcad and writes OBJ/MTL (metres, +Y up) plus catalog.json to
examples/camera-turntable/models/, which the viewer merges with the shared
catalog when it opens turntable.system.json. Each model is in the frame of
the system instance that shows it (the disc and base about the turntable
axis, the pulley about the servo shaft), so the viewer turns it about +Y.
Presentation only; physics comes from the *.physics.json derivations.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, os.path.join(REPO, 'cad'))
from robocad.document import Document  # noqa: E402
from robocad.io.exporters import ObjSettings, export_obj  # noqa: E402

OUT = os.path.join(HERE, '..', 'models')
MODELS = {
    # model: (description, group, names (subtract from the group, or the bodies when no group), origin in CAD mm)
    'turntable_disc': ('Turntable disc with camera, ESP32 board and slip ring (turns)', 'Turntable (rotating)', None, 'disc'),
    'turntable_base': ('Turntable base, bearing post, servo and cradle (fixed)', 'Base (fixed)', {'GT2 30T drive pulley', 'Servo horn (reference)', 'GT2 closed belt 610 mm (reference)'}, 'base'),
    'turntable_pulley': ('GT2 30-tooth drive pulley on the servo horn (turns with the servo)', None, {'GT2 30T drive pulley', 'Servo horn (reference)'}, 'pulley'),
    'turntable_belt': ('GT2 610 mm closed belt', None, {'GT2 closed belt 610 mm (reference)'}, 'belt'),
}


def main():
    doc = Document.load(os.path.join(HERE, 'turntable.rcad'))
    os.makedirs(OUT, exist_ok=True)
    by_name = {n.name: n for n in doc.nodes.values()}
    def box_of(name):
        lo, hi = doc.kernel.bounding_box(by_name[name].body)
        return lo, hi
    plo, phi = box_of('GT2 30T drive pulley')
    blo, bhi = box_of('GT2 closed belt 610 mm (reference)')
    dlo, dhi = box_of('Turntable disc with GT2 ring and bearing hub')
    base_lo, base_hi = box_of('Base and bearing post')
    shaft = ((plo[0] + phi[0]) / 2, (plo[1] + phi[1]) / 2)
    disc_top = dhi[2]
    # Each model's origin is where its system instance sits (on its turning axis).
    origins = {'disc': (0.0, 0.0, disc_top - 2.0), 'base': (0.0, 0.0, 2.5),
               'pulley': (shaft[0], shaft[1], (plo[2] + phi[2]) / 2), 'belt': (0.0, 0.0, (blo[2] + bhi[2]) / 2)}
    catalog = {'schema': 'sim.models/1', 'units': 'm', 'up': '+Y', 'source': 'examples/camera-turntable/cad/export_models.py',
               'models': {}, 'origins_mm': {}}
    for model, (description, group, names, origin) in MODELS.items():
        if group is not None:
            members = {doc.nodes[c].name for c in by_name[group].children}
            names = members - names if names else members
        o = origins[origin]
        local = Document()
        for name in sorted(names):
            n = by_name[name]
            c = local.add_body(doc.kernel.transform(n.body, translation=(-o[0], -o[1], -o[2])), n.name, n.material)
            c.color = n.color
        export_obj(local, os.path.join(OUT, f'{model}.obj'), None, ObjSettings(tolerance=0.06, scale=0.001, up_axis='Y', uvs=False))
        catalog['models'][model] = {'file': f'{model}.obj', 'description': description}
        catalog['origins_mm'][model] = list(o)
    with open(os.path.join(OUT, 'catalog.json'), 'w') as fh:
        json.dump(catalog, fh, indent=1); fh.write('\n')
    print(json.dumps(catalog, indent=1))


if __name__ == '__main__':
    main()
