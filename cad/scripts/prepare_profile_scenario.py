"""Author a separate CAD actuator/power-profile scenario via shared commands.

The input is a complete profile declaration, including provenance and evidence.
This helper supplies no physical parameters and never accesses motor hardware.
"""
import argparse
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
    parser.add_argument('profiles', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--expect-parent-sha256', required=True)
    args = parser.parse_args()
    receipt_path = args.output.with_suffix('.receipt.json')
    for path in (args.output, receipt_path):
        if path.exists():
            raise FileExistsError('Preserve existing artifact: ' + str(path))
    if sha(args.parent) != args.expect_parent_sha256:
        raise ValueError('Parent CAD identity changed')
    profile_hash = sha(args.profiles)
    profiles = json.loads(args.profiles.read_text())
    doc = Document.load(str(args.parent))
    Ops(doc).set_actuator_profiles(profiles)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    doc.save(str(args.output))
    restored = Document.load(str(args.output))
    if restored.robot_settings['actuator_profiles'] != profiles:
        raise ValueError('Profile archive round trip changed')
    with zipfile.ZipFile(args.parent) as old, zipfile.ZipFile(args.output) as new:
        entries = {p for p in old.namelist() if p.startswith(('brep/', 'mesh/'))}
        if entries != {p for p in new.namelist() if p.startswith(('brep/', 'mesh/'))}:
            raise ValueError('Geometry inventory changed')
        if any(old.read(p) != new.read(p) for p in entries):
            raise ValueError('Geometry changed')
    if sha(args.parent) != args.expect_parent_sha256 or sha(args.profiles) != profile_hash:
        raise ValueError('Source changed during authoring')
    receipt = {'parent': str(args.parent), 'parent_sha256': sha(args.parent),
               'profiles': str(args.profiles), 'profiles_sha256': profile_hash,
               'cad': str(args.output), 'cad_sha256': sha(args.output),
               'geometry_entries_unchanged': len(entries),
               'archive_round_trip_verified': True, 'calibrated': False,
               'authoring_script_sha256': sha(Path(__file__))}
    receipt_path.write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2), flush=True)


if __name__ == '__main__':
    main()
