"""Save a separate CAD revision with an explicit simulated PWM authority setting.

Uses the ordinary undoable profile command and native validator. No hardware I/O.
Physical motor, driver, feedback and geometry values remain unchanged.
"""
import argparse
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
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('parent', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--expect-parent-sha256', required=True)
    parser.add_argument('--limit-permille', type=int, required=True)
    args = parser.parse_args()
    if not 1 <= args.limit_permille <= 1000:
        raise ValueError('PWM authority must be 1..1000 permille')
    if sha(args.parent) != args.expect_parent_sha256:
        raise ValueError('Parent CAD identity changed')
    assumptions_path = args.output.with_suffix('.authority.json')
    receipt_path = args.output.with_suffix('.receipt.json')
    for path in [args.output, assumptions_path, receipt_path]:
        if path.exists():
            raise FileExistsError('Preserve existing artifact: ' + str(path))
    print('Loading parent CAD without changing the original', flush=True)
    doc = Document.load(str(args.parent))
    before = deepcopy(doc.robot_settings['actuator_profiles'])
    profiles = deepcopy(before)
    assumptions = {
        'scope': 'Simulation controller-authority comparison only; not hardware qualification or calibration.',
        'parent': str(args.parent), 'parent_sha256': sha(args.parent),
        'limit_permille': args.limit_permille,
        'previous_limits': {name: f['controller']['gains']['limit'] for name, f in before['families'].items()},
        'limitations': ['All motor and driver physical parameters remain provisional.',
                       'Changing the command limit does not establish available loaded torque or safe electrical ratings.',
                       'No motor or FPGA hardware is accessed by this operation.',
                       'Feedback, cadence, latency estimates and other controller gains remain unchanged.'],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    assumptions_path.write_text(json.dumps(assumptions, indent=2) + '\n')
    evidence = {'path': str(assumptions_path), 'sha256': sha(assumptions_path),
                'scope': assumptions['scope']}
    for name, family in profiles['families'].items():
        family['version'] += 1
        family['controller']['gains']['limit'] = args.limit_permille
        family['controller']['evidence'] = 'controller_authority_comparison'
        family['evidence']['controller_authority_comparison'] = evidence
        family['limitations'] = [s for s in family['limitations'] if not s.startswith('Controller limit ')]
        family['limitations'].append('Controller limit {}/1000 is an experimental simulation setting, not a motor or power-supply rating.'.format(args.limit_permille))
        for binding in profiles['bindings'].values():
            if binding['family'] == name:
                binding['version'] = family['version']
    ops = Ops(doc)
    ops.set_actuator_profiles(profiles)
    doc.save(str(args.output))
    restored = Document.load(str(args.output))
    if restored.robot_settings['actuator_profiles'] != profiles:
        raise ValueError('Profile archive round trip changed')
    with zipfile.ZipFile(args.parent) as old, zipfile.ZipFile(args.output) as new:
        entries = {p for p in old.namelist() if p.startswith(('brep/', 'mesh/'))}
        if entries != {p for p in new.namelist() if p.startswith(('brep/', 'mesh/'))}:
            raise ValueError('Geometry inventory changed')
        if any(old.read(p) != new.read(p) for p in entries):
            raise ValueError('Geometry bytes changed')
    if sha(args.parent) != args.expect_parent_sha256:
        raise ValueError('Parent changed during authoring')
    receipt = {'parent': str(args.parent), 'parent_sha256': sha(args.parent),
               'cad': str(args.output), 'cad_sha256': sha(args.output),
               'geometry_entries_unchanged': len(entries), 'archive_round_trip_verified': True,
               'controller_limit_permille': args.limit_permille,
               'motor_count': len(profiles['bindings']), 'calibrated': False,
               'authoring_script_sha256': sha(Path(__file__))}
    receipt_path.write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2), flush=True)


if __name__ == '__main__':
    main()
