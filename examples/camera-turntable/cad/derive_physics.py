"""Derive the turntable's simulation parameters from the saved CAD model.

    cad/.venv/bin/python examples/camera-turntable/cad/derive_physics.py

Writes, next to turntable.rcad, one ``sim.cad-physics/1`` record per system
instance (applied with ``sim-system cad-params``):

* ``belt-drive.physics.json``   part.belt_drive   (radii, belt stiffness)
* ``disc-inertia.physics.json`` rotational.inertia (everything that turns with the disc)
* ``pulley-inertia.physics.json`` rotational.inertia (drive pulley + horn, about the servo shaft)
* ``bearing-friction.physics.json`` rotational.coulomb_friction (bearing seals + slip ring)
* ``camera-rig.json`` sim.vision-rig/1: turning axis and the camera's optical
  centre, axis and up at disc angle 0 (metres), for the virtual camera

Printed parts: CAD gives solid-PLA mass. A print is lighter (walls and
top/bottom skins solid, sparse infill inside), so printed bodies are scaled by
PRINT_FILL (an estimate with its uncertainty). Weigh the printed disc and
replace the estimate; the inertia scales with the measured mass.
"""
import hashlib
import json
import math
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, os.path.join(REPO, 'cad'))
from robocad.belt_derivation import export_physics as export_belt  # noqa: E402
from robocad.document import Document  # noqa: E402
from robocad.physical import body_mass_properties  # noqa: E402

RCAD = os.path.join(HERE, 'turntable.rcad')
PRINT_FILL = (0.6, 0.15)  # fraction of solid mass for 3 walls, 4 skins, 20 % gyroid; ± absolute
# Seal drag of a 6808-2RS contact seal pair and a capsule slip ring's brush drag.
# Catalogue-order estimates, not measurements (measure with a spring scale on the disc rim).
FRICTION = {'bearing_seals_Nm': 2 * 0.008, 'slip_ring_Nm': 0.01, 'relative_uncertainty': 0.6}


def record(doc_sha, component_type, parameters, **extra):
    return {'schema': 'sim.cad-physics/1', 'cad': {'path': os.path.relpath(RCAD, REPO), 'sha256': doc_sha},
            'component_type': component_type, 'parameters': parameters, **extra}


def inertia_about_z(doc, names, axis_xy=(0.0, 0.0)):
    """Sum of I_zz about the axis for the named bodies; printed ones scaled by PRINT_FILL."""
    total, total_lo, total_hi, mass, parts = 0.0, 0.0, 0.0, 0.0, []
    for n in doc.nodes.values():
        if n.name not in names:
            continue
        m, com, inertia, source = body_mass_properties(doc, n)
        r2 = (com[0] - axis_xy[0] * 1e-3) ** 2 + (com[1] - axis_xy[1] * 1e-3) ** 2
        izz = float(inertia[2, 2]) + m * r2
        printed = bool((n.robot or {}).get('print'))
        f, df = PRINT_FILL if printed else (1.0, 0.0)
        total += f * izz; total_lo += (f - df) * izz; total_hi += (f + df) * izz; mass += f * m
        parts.append({'name': n.name, 'mass_kg': f * m, 'Izz_kg_m2': f * izz, 'source': source,
                      'printed_fill': f if printed else None})
    return total, (total_hi - total_lo) / 2, mass, parts


def main():
    doc = Document.load(RCAD)
    with open(RCAD, 'rb') as fh:
        sha = hashlib.sha256(fh.read()).hexdigest()
    belt = export_belt(doc, RCAD, os.path.join(HERE, 'belt-drive.physics.json'), 'turntable')
    group = lambda name: {doc.nodes[c].name for c in doc.nodes[next(i for i, n in doc.nodes.items() if n.name == name)].children}
    turning = group('Turntable (rotating)')
    disc_I, disc_dI, disc_m, disc_parts = inertia_about_z(doc, turning)
    pulley = next(p for p in belt['pulleys'] if p['role'] == 'driver')
    pul_I, pul_dI, pul_m, pul_parts = inertia_about_z(doc, {'GT2 30T drive pulley', 'Servo horn (reference)'}, pulley['center_mm'])
    rule = 'I = Σ (I_zz,com + m·r²) over the bodies; printed bodies × fill fraction'
    out = {
        'disc-inertia': record(sha, 'rotational.inertia', {'inertia': {
            'value': disc_I, 'unit': 'kg·m²', 'uncertainty': disc_dI, 'provenance': {
                'kind': 'derived', 'rule': rule, 'inputs': {'bodies': disc_parts, 'print_fill': PRINT_FILL,
                                                          'rotating_mass_kg': disc_m}}}}),
        'pulley-inertia': record(sha, 'rotational.inertia', {'inertia': {
            'value': pul_I, 'unit': 'kg·m²', 'uncertainty': pul_dI, 'provenance': {
                'kind': 'derived', 'rule': rule + ' (about the servo shaft)', 'inputs': {'bodies': pul_parts}}}}),
        'bearing-friction': record(sha, 'rotational.coulomb_friction', {'torque': {
            'value': FRICTION['bearing_seals_Nm'] + FRICTION['slip_ring_Nm'], 'unit': 'N·m',
            'uncertainty': FRICTION['relative_uncertainty'] * (FRICTION['bearing_seals_Nm'] + FRICTION['slip_ring_Nm']),
            'provenance': {'kind': 'estimated', 'rule': 'two 6808-2RS contact-seal drags + capsule slip-ring brush drag',
                           'inputs': FRICTION}}}),
    }
    camera = next(n for n in doc.nodes.values() if n.name.startswith('Camera Module 3 Wide'))
    joint = next(n for n in doc.nodes.values() if n.kind == 'joint' and n.name == 'Turntable axis').joint
    optics = camera.robot['optics']
    rig = {'schema': 'sim.vision-rig/1',
           'axis_point': [x * 1e-3 for x in joint.pivot], 'axis': list(joint.axis),
           'optical_center': [x * 1e-3 for x in optics['optical_center_mm']], 'optical_axis': optics['optical_axis'], 'up': [0.0, 0.0, 1.0],
           'source': {'cad': os.path.relpath(RCAD, REPO), 'sha256': sha, 'camera_node': camera.name,
                      'rule': 'optical centre = lens front on the camera board (CAD reference body); axis from the turntable joint',
                      'uncertainty': 'lens entrance pupil position and camera tilt on the mount are unmeasured (±2 mm, ±1° estimated)'}}
    with open(os.path.join(HERE, 'camera-rig.json'), 'w') as fh:
        json.dump(rig, fh, indent=1); fh.write('\n')
    # Object scanning: the camera is fixed on the stand and the object turns, so in the
    # object's frame the camera orbits the other way (axis reversed).
    for ring in ('low', 'high'):
        cam = next((n for n in doc.nodes.values() if n.name == f'Object camera ({ring} ring, reference)'), None)
        if cam is None:
            continue
        o = cam.robot['optics']
        obj_rig = {'schema': 'sim.vision-rig/1', 'axis_point': [x * 1e-3 for x in joint.pivot], 'axis': [-a for a in joint.axis],
                   'optical_center': [x * 1e-3 for x in o['optical_center_mm']], 'optical_axis': o['optical_axis'], 'up': [0.0, 0.0, 1.0],
                   'source': {'cad': os.path.relpath(RCAD, REPO), 'sha256': sha, 'camera_node': cam.name,
                              'rule': 'object frame: camera fixed on the stand, object turning, so the camera orbits with the axis reversed',
                              'uncertainty': 'detent tilt and stand placement unmeasured (±1°, ±3 mm estimated)'}}
        with open(os.path.join(HERE, f'object-rig-{ring}.json'), 'w') as fh:
            json.dump(obj_rig, fh, indent=1); fh.write('\n')
    for name, rec in out.items():
        with open(os.path.join(HERE, f'{name}.physics.json'), 'w') as fh:
            json.dump(rec, fh, indent=1); fh.write('\n')
    print(json.dumps({'belt': {k: v['value'] for k, v in belt['parameters'].items()},
                      'disc_inertia_kg_m2': [disc_I, disc_dI], 'rotating_mass_kg': disc_m,
                      'pulley_inertia_kg_m2': [pul_I, pul_dI], 'layout': belt['belt']}, indent=1))


if __name__ == '__main__':
    main()
