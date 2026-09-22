"""Reuse a pinned physical derivation for an actuator-profile-only CAD revision.

Proves all CAD archive content except profiles/save metadata is identical. Keeps
historical derivation provenance explicitly; does not claim a fresh derivation.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import zipfile


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def verify_profile_only(parent, revised):
    with zipfile.ZipFile(parent) as a, zipfile.ZipFile(revised) as b:
        if set(a.namelist()) != set(b.namelist()):
            raise ValueError('CAD archive inventory changed')
        for name in a.namelist():
            if name != 'manifest.json' and a.read(name) != b.read(name):
                raise ValueError('CAD archive content changed: ' + name)
        old, new = (json.loads(z.read('manifest.json')) for z in (a, b))
    profiles = copy.deepcopy(new['robot_settings']['actuator_profiles'])
    for manifest in (old, new):
        manifest.pop('saved', None)
        manifest.pop('revision', None)
        manifest['robot_settings'].pop('actuator_profiles', None)
    if old != new:
        raise ValueError('CAD physical or other source definition changed')
    return profiles


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('parent_export', type=Path)
    p.add_argument('cad', type=Path)
    p.add_argument('output', type=Path)
    args = p.parse_args()
    receipt_path = args.output.with_suffix('.receipt.json')
    if args.output.exists() or receipt_path.exists():
        raise FileExistsError('Preserve existing export and receipt')
    parent_receipt_path = args.parent_export.with_suffix('.receipt.json')
    parent_receipt = json.loads(parent_receipt_path.read_text())
    parent = json.loads(args.parent_export.read_text())
    if sha(args.parent_export) != parent_receipt['output_sha256']:
        raise ValueError('Parent export digest changed')
    parent_cad = Path(parent_receipt['cad'])
    if sha(parent_cad) != parent_receipt['cad_sha256'] or parent['source']['cad_sha256'] != sha(parent_cad):
        raise ValueError('Parent CAD identity changed')
    source_hash = sha(args.cad)
    profiles = verify_profile_only(parent_cad, args.cad)
    parent['actuator_profiles'] = profiles
    parent['source'].update(file=str(args.cad), cad_sha256=source_hash,
        profile_only_derivation_reuse={'parent_export': str(args.parent_export),
            'parent_export_sha256': sha(args.parent_export), 'parent_cad_sha256': sha(parent_cad),
            'archive_equality_except_profiles_and_save_metadata': True})
    root = Path(__file__).resolve().parents[2]
    tool = Path(os.environ.get('ROBOCAD_ACTUATOR_PROFILE_TOOL', str(root / 'target/debug/sim-actuator-profiles')))
    resolved = json.loads(subprocess.run([str(tool)], input=json.dumps(parent), text=True,
        capture_output=True, check=True).stdout)
    if source_hash != sha(args.cad):
        raise ValueError('CAD changed during export')
    args.output.write_text(json.dumps(parent) + '\n')
    receipt = {'cad': str(args.cad), 'cad_sha256': source_hash, 'output': str(args.output),
        'output_sha256': sha(args.output), 'flex': parent_receipt['flex'],
        'derivation_source_sha256': parent_receipt['derivation_source_sha256'],
        'resolved_profiles': resolved, 'native_validator_sha256': sha(tool),
        'mode': 'verified_profile_only_reuse_of_pinned_derivation',
        'parent_export_receipt': str(parent_receipt_path), 'parent_export_receipt_sha256': sha(parent_receipt_path),
        'export_script_sha256': sha(__file__)}
    receipt_path.write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
