"""Author explicit provisional encoder frames and initial references in a new CAD revision."""
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import zipfile

from robocad.commands import Ops
from robocad.document import Document


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    root = Path(__file__).resolve().parents[2]
    folder = root / 'examples/full-robot/measured-actuator-integration'
    original = folder / 'cad-profiles/robot-provisional.rcad'
    output = folder / 'controller-integration'
    output.mkdir(exist_ok=True)
    cad = output / 'robot-controller-provisional.rcad'
    if cad.exists():
        raise FileExistsError(f'Preserve existing controller revision: {cad}')
    source_receipt = json.loads((folder / 'cad-profiles/revision-receipt.json').read_text())
    if sha(original) != source_receipt['revision_sha256']:
        raise ValueError('Provisional CAD source changed')
    audit = json.loads((folder / 'baseline-audit.json').read_text())
    gait_path = root / audit['baseline_input']
    if sha(gait_path) != audit['baseline_input_sha256']:
        raise ValueError('Pinned gait input changed')
    gait = json.loads(gait_path.read_text())['runtime']
    coordinates = gait['config']['motors']['target_coordinates']
    targets = {coordinate.removeprefix('joint.'): boundary['target_rad']
               for coordinate, boundary in zip(coordinates, gait['config']['motors']['servos'])}
    assumptions = {
        'status': 'Provisional simulation coordinate choices; not physical encoder calibration',
        'encoder_zero_counts_at_cad_joint_zero': 2048,
        'encoder_direction_relative_to_cad_positive_joint': 1,
        'sample_phase_s': 0,
        'initial_reference': 'Exact initial target of the preserved gait for each named CAD motor joint',
        'limitations': ['Physical motor-to-joint assignments are unknown.',
                        'Encoder origins, polarity and clock phase need physical measurement before deployment.',
                        'Gait initial references are configuration data, not motor measurements.'],
    }
    assumptions_path = output / 'feedback-assumptions.json'
    assumptions_path.write_text(json.dumps(assumptions, indent=2) + '\n')
    print('Loading separate provisional CAD revision', flush=True)
    doc = Document.load(str(original))
    profiles = deepcopy(doc.robot_settings['actuator_profiles'])
    for family in profiles['families'].values():
        family['evidence']['feedback_assumptions'] = {
            'path': str(assumptions_path.relative_to(root)), 'sha256': sha(assumptions_path),
            'scope': assumptions['status']}
        family['evidence']['historical_gait'] = {
            'path': str(gait_path.relative_to(root)), 'sha256': sha(gait_path),
            'scope': 'Pinned initial joint references; no physical encoder calibration'}
        family['limitations'].append('Encoder frame and phase bindings are provisional simulation choices, not measured hardware calibration.')
    for motor in audit['motors']:
        mid = motor['cad_id']
        if mid not in profiles['bindings'] or motor['joint'] not in targets:
            raise ValueError('Missing explicit CAD motor or historical joint reference')
        def value(v, unit, evidence='feedback_assumptions', provenance='estimated'):
            return {'value': v, 'unit': unit, 'evidence': evidence,
                    'provenance': provenance, 'uncertainty': None}
        profiles['bindings'][mid]['feedback'] = {
            'encoder_zero': value(2048, '1'), 'encoder_direction': value(1, '1'),
            'sample_phase': value(0, 's'),
            'initial_target': value(targets[motor['joint']], 'rad', 'historical_gait', 'derived')}
    Ops(doc).set_actuator_profiles(profiles)
    doc.save(str(cad))
    print('Verifying controller CAD revision', flush=True)
    restored = Document.load(str(cad))
    if restored.robot_settings['actuator_profiles'] != profiles:
        raise ValueError('Controller binding round trip changed')
    with zipfile.ZipFile(original) as old, zipfile.ZipFile(cad) as new:
        geometry = [p for p in old.namelist() if p.startswith(('brep/', 'mesh/'))]
        if any(old.read(p) != new.read(p) for p in geometry):
            raise ValueError('Geometry changed during controller authoring')
    (output / 'profiles.json').write_text(json.dumps(profiles, indent=2) + '\n')
    receipt = {'version': 1, 'cad': str(cad.relative_to(root)), 'cad_sha256': sha(cad),
               'parent_cad_sha256': sha(original), 'geometry_entries_unchanged': len(geometry),
               'archive_round_trip_verified': True, 'motor_count': len(profiles['bindings']),
               'calibrated': False, 'physical_unit_assignments': 0}
    (output / 'cad-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2), flush=True)


if __name__ == '__main__':
    main()
