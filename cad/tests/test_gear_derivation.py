"""Worm-gear physics derived from CAD geometry, and the CAD edit → datasheet loop."""
import json
import math
import os
import subprocess

import pytest

from robocad.document import Document
from robocad.gear_derivation import build_worm_set, derive_worm_gear, export_physics

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
SIM_SYSTEM = os.path.join(ROOT, 'target', 'release', 'sim-system')


@pytest.mark.parametrize('module,starts,teeth,d1', [(0.5, 1, 30, 8.0), (0.8, 2, 40, 16.0), (1.0, 3, 25, 20.0)])
def test_geometry_gives_the_gear_parameters(module, starts, teeth, d1):
    doc = Document()
    build_worm_set(doc, module, starts, teeth, d1)
    p = derive_worm_gear(doc)['parameters']
    assert p['module']['value'] == pytest.approx(module * 1e-3, rel=1e-6)
    assert p['worm_pitch_diameter']['value'] == pytest.approx(d1 * 1e-3, rel=1e-6)
    assert p['worm_starts']['value'] == starts
    assert p['wheel_teeth']['value'] == teeth
    assert p['friction']['provenance']['kind'] == 'estimated'
    assert 'r_tip_mm' in p['module']['provenance']['inputs'], 'the rule and its measured inputs are recorded'
    # Every start is its own thread: no two thread bodies coincide.
    centroids = [tuple(round(c, 6) for c in doc.kernel.mass_properties(n.body).centroid)
                 for n in doc.nodes.values() if n.body is not None and n.name == 'worm thread']
    assert len(set(centroids)) == len(centroids)



def test_derivation_does_not_depend_on_where_the_set_sits():
    at_origin, moved = Document(), Document()
    build_worm_set(at_origin, 0.8, 2, 40, 16.0)
    build_worm_set(moved, 0.8, 2, 40, 16.0, centre=(-40.0, 22.0, -7.0))
    a, b = derive_worm_gear(at_origin)['parameters'], derive_worm_gear(moved)['parameters']
    for name in ('module', 'worm_pitch_diameter', 'worm_starts', 'wheel_teeth'):
        assert b[name]['value'] == pytest.approx(a[name]['value'], rel=1e-6), name

def efficiency(mu, starts, module, d1, phi=math.radians(20)):
    t = starts * module / d1
    return (math.cos(phi) - mu * t) / (math.cos(phi) + mu / t)


@pytest.mark.skipif(not os.path.exists(SIM_SYSTEM), reason='build target/release/sim-system first')
def test_a_cad_edit_updates_the_datasheet(tmp_path):
    sheets = {}
    for d1 in (8.0, 12.0):  # the edit: a fatter worm (shallower lead)
        doc = Document()
        build_worm_set(doc, 0.5, 1, 30, d1)
        rcad = str(tmp_path / f'worm-{d1}.rcad')
        doc.save(rcad)
        physics = str(tmp_path / f'worm-{d1}.physics.json')
        export_physics(doc, rcad, physics)
        out = subprocess.run([SIM_SYSTEM, 'datasheet', 'rotational.worm_gear', '--parameters', physics], cwd=ROOT, capture_output=True, text=True, check=True).stdout
        sheet = json.loads(out)
        assert sheet['derived_from']['cad']['path'].endswith(f'worm-{d1}.rcad')
        values = {v['name']: v['value'] for v in sheet['values']}
        sheets[d1] = values
        assert values['forward efficiency'] == pytest.approx(efficiency(0.07, 1, 0.5, d1), abs=0.01)
    assert sheets[12.0]['forward efficiency'] < sheets[8.0]['forward efficiency'] - 0.05
