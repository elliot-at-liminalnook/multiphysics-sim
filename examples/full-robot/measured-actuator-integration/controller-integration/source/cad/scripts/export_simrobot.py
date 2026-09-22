"""Export a saved CAD artifact through the shared derivation/cache and Rust validation.

Run in the CAD Python environment with PYTHONPATH=cad. Work stays outside the UI.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

from robocad.document import Document
from robocad.derivation_cache import DerivationCache
from robocad.physical import export_physical_model


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('cad', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--expect-cad-sha256')
    parser.add_argument('--no-flex', action='store_true')
    parser.add_argument('--replace', action='store_true')
    args = parser.parse_args()
    if args.output.exists() and not args.replace:
        raise FileExistsError(args.output)
    source_hash = sha(args.cad)
    if args.expect_cad_sha256 and args.expect_cad_sha256 != source_hash:
        raise ValueError('CAD hash differs from the requested artifact')
    root = Path(__file__).resolve().parents[2]
    source_identity = {str(p.relative_to(root)): sha(p) for p in (root / 'cad/robocad').rglob('*.py')}
    cache = DerivationCache(args.cache, source_identity)
    print(f'Loading {args.cad}', flush=True)
    doc = Document.load(str(args.cad))
    model = export_physical_model(doc, flex=not args.no_flex, verbose=True, cache=cache)
    if sha(args.cad) != source_hash:
        raise ValueError('CAD file changed during export; no output replaced')
    model['source']['cad_sha256'] = source_hash
    model['source']['cad_derivation_source_sha256'] = hashlib.sha256(json.dumps(source_identity, sort_keys=True).encode()).hexdigest()
    tool = os.environ.get('ROBOCAD_ACTUATOR_PROFILE_TOOL', str(root / 'target/debug/sim-actuator-profiles'))
    result = subprocess.run([tool], input=json.dumps(model), text=True, capture_output=True, check=True)
    resolved = json.loads(result.stdout)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.output.with_suffix(args.output.suffix + '.tmp')
    temporary.write_text(json.dumps(model) + '\n')
    os.replace(temporary, args.output)
    receipt = {'cad': str(args.cad), 'cad_sha256': source_hash,
               'output': str(args.output), 'output_sha256': sha(args.output),
               'flex': not args.no_flex, 'cache': cache.stats,
               'derivation_source_sha256': model['source']['cad_derivation_source_sha256'],
               'resolved_profiles': resolved, 'native_validator_sha256': sha(Path(tool))}
    args.output.with_suffix('.receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(f'Exported {args.output}: {len(model["motors"])} motors, {len(model["links"])} links', flush=True)


if __name__ == '__main__':
    main()
