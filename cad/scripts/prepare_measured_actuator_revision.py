"""Author a separate provisional quadruped revision from the pinned CAD baseline.

Run with the CAD Python environment and PYTHONPATH=cad. This only authors CAD;
the native Rust validator checks profiles. No simulator or motor hardware runs.
"""
import argparse
import hashlib
import json
import subprocess
import os
import zipfile
from pathlib import Path

from robocad.commands import Ops
from robocad.document import Document
from robocad.physical import export_physical_model


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--export', action='store_true', help='Also derive the full physical model (expensive)')
    parser.add_argument('--resume', action='store_true', help='Verify and reuse an existing identical CAD revision')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    folder = root / 'examples/full-robot/measured-actuator-integration'
    audit = json.loads((folder / 'baseline-audit.json').read_text())
    baseline = root / audit['cad_artifact']
    if sha(baseline) != audit['cad_sha256']:
        raise ValueError('Pinned CAD baseline changed; refusing to author from a different robot')
    output = folder / 'cad-profiles/robot-provisional.rcad'
    if output.exists() and not args.resume:
        raise FileExistsError(f'Preserve existing revision: {output}')
    if args.resume and not output.exists():
        raise FileNotFoundError(output)
    family_path = folder / 'cad-profiles/hx30hm-provisional-family.json'
    family = json.loads(family_path.read_text())
    for evidence in family['evidence'].values():
        if sha(root / evidence['path']) != evidence['sha256']:
            raise ValueError(f"Profile evidence changed: {evidence['path']}")
    print('Loading existing revision' if args.resume else 'Loading pinned CAD baseline', flush=True)
    doc = Document.load(str(output if args.resume else baseline))
    motors = {n.id for n in doc.bodies() if (n.robot or {}).get('kind') == 'motor'}
    expected = {m['cad_id'] for m in audit['motors']}
    if motors != expected:
        raise ValueError('CAD motor identities differ from baseline audit')
    profiles = {'version': 1, 'families': {'hx30hm-provisional': family},
                'bindings': {mid: {'family': 'hx30hm-provisional', 'version': 1,
                                   'physical_unit': None, 'deviations': {}}
                             for mid in sorted(motors)}}
    if args.resume:
        if doc.robot_settings.get('actuator_profiles') != profiles:
            raise ValueError('Existing revision differs; preserve it and explicitly author a new revision')
    else:
        Ops(doc).set_actuator_profiles(profiles)
        doc.save(str(output))
    with zipfile.ZipFile(baseline) as old, zipfile.ZipFile(output) as new:
        entries = [n for n in old.namelist() if n.startswith(('brep/', 'mesh/'))]
        if set(entries) != {n for n in new.namelist() if n.startswith(('brep/', 'mesh/'))}:
            raise ValueError('CAD geometry inventory changed')
        if any(old.read(n) != new.read(n) for n in entries):
            raise ValueError('CAD geometry bytes changed')
    print(f'Saved {output}; verifying reload', flush=True)
    restored = doc if args.resume else Document.load(str(output))
    if restored.robot_settings['actuator_profiles'] != profiles:
        raise ValueError('CAD profile round trip changed authored values')
    receipt = {'version': 1, 'baseline': audit['cad_artifact'],
               'baseline_sha256': sha(baseline), 'revision': str(output.relative_to(root)),
               'revision_sha256': sha(output), 'family_sha256': sha(family_path),
               'motor_count': len(motors), 'physical_units_assigned': 0,
               'calibrated': False, 'archive_round_trip_verified': True,
               'geometry_entries_unchanged': len(entries),
               'physical_export': None}
    receipt_path = output.parent / 'revision-receipt.json'
    if args.resume and receipt_path.exists():
        previous = json.loads(receipt_path.read_text())
        if previous.get('revision_sha256') == receipt['revision_sha256']:
            for key in ('physical_export', 'physical_export_error', 'physical_export_attempt_log'):
                if key in previous:
                    receipt[key] = previous[key]
    def save_receipt():
        receipt_path.write_text(json.dumps(receipt, indent=2) + '\n')
    # Save the archive evidence even if a later geometry derivation fails.
    save_receipt()
    if args.export:
        print('Deriving physical model from reloaded CAD', flush=True)
        model_path = output.with_suffix('.simrobot.json')
        from robocad.derivation_cache import DerivationCache
        source_identity = {str(p.relative_to(root)): sha(p) for p in
                           (root / 'cad/robocad').rglob('*.py')}
        cache = DerivationCache(output.parent / 'derived-cache', source_identity)
        try:
            model = export_physical_model(restored, str(model_path), flex=False, verbose=True, cache=cache)
        except Exception as error:
            receipt['physical_export_error'] = str(error)
            save_receipt()
            raise
        if model['actuator_profiles'] != profiles:
            raise ValueError('Export changed authored profiles')
        tool = os.environ.get('ROBOCAD_ACTUATOR_PROFILE_TOOL', str(root / 'target/debug/sim-actuator-profiles'))
        validation = subprocess.run([tool], input=json.dumps(model), text=True,
                                    capture_output=True, check=True)
        native_receipt = output.parent / 'resolved-export.json'
        native_receipt.write_text(validation.stdout + '\n')
        receipt['physical_export'] = {'path': str(model_path.relative_to(root)),
                                      'sha256': sha(model_path), 'flex': False,
                                      'resolved_receipt_sha256': sha(native_receipt)}
        receipt.pop('physical_export_error', None)
    save_receipt()
    print(json.dumps(receipt, indent=2), flush=True)


if __name__ == '__main__':
    main()
