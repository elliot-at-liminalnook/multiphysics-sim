"""Headless paired-run reference process. Written fixtures are not run by import.

Runner owns source copying, process-group cancellation and report publication.
This process independently loads the copied archive and uses direct Ops/services.
"""
import argparse
import datetime
import hashlib
import io
import json
import os
from pathlib import Path
import signal
import tempfile
import zipfile

from .document import Document
from .kernel import KernelError
from .experiments import RevisionConflict
from .parity_operations import Dispatcher, Incomplete
from .parity_paths import read_regular, publish_new
from .parity_observations import collect, observation, result_observation, required_field

IDENTITY = {'name': 'robocad-reference', 'version': '1',
    'implementation': 'python-direct-ops', 'kernel': 'python-occt',
    'derivation': 'robocad-physical-v1', 'independent': False}


def validate_model(manifest, model):
    """Validate ownership, declared dependency closure and bounded ZIP inputs.

    Historical component provenance paths are metadata, never operational reads.
    Archive node source IDs are links within the archive, not filesystem paths.
    External images and linked system reads require declared safe dependencies.
    """
    if manifest.get('schema_version') != 1:
        raise ValueError('schema_version: unsupported manifest schema')
    model = Path(model).absolute()
    source = Path(manifest['source']['path'])
    if source.is_absolute() or not source.parts or any(p in ('.', '..') for p in source.parts):
        raise ValueError('source.path: unsafe relative path')
    if tuple(model.parts[-len(source.parts):]) != source.parts:
        raise ValueError('model: isolated source suffix mismatch')
    root = model
    for _ in source.parts:
        root = root.parent
    for ancestor in (root, *root.parents):
        if ancestor.is_symlink():
            raise ValueError('workspace: symlink ancestor refused')
    marker = root / '.cad-parity-owner'
    if marker.is_symlink() or not marker.is_file():
        raise ValueError('workspace: missing exclusive ownership marker')
    if json.loads(read_regular(marker)).get('source_sha256') != manifest['source']['sha256']:
        raise ValueError('workspace: ownership source mismatch')
    declared = {}
    for item in [manifest['source'], *manifest['dependencies']]:
        relative = Path(item['path'])
        if relative.is_absolute() or not relative.parts or any(p in ('.', '..') for p in relative.parts):
            raise ValueError('dependency.path: unsafe relative path')
        if str(relative) in declared:
            raise ValueError('dependency.path: duplicate declaration')
        path = root
        for part in relative.parts:
            path = path / part
            if path.is_symlink():
                raise ValueError('dependency.path: symlink refused')
        if not path.is_file() or path.stat().st_size > 64 * 1024 * 1024:
            raise ValueError('dependency.path: missing regular bounded file')
        if hashlib.sha256(read_regular(path)).hexdigest() != item['sha256']:
            raise ValueError('dependency.sha256: digest mismatch')
        declared[str(relative)] = path
    with zipfile.ZipFile(io.BytesIO(read_regular(model))) as archive:
        entries = archive.infolist()
        if len(entries) > 20000 or sum(e.file_size for e in entries) > 128 * 1024 * 1024:
            raise ValueError('archive: decompressed corpus bound exceeded')
        names = [e.filename for e in entries]
        if len(set(names)) != len(names):
            raise ValueError('archive: duplicate entry')
        for entry in entries:
            path = Path(entry.filename)
            if path.is_absolute() or any(p == '..' for p in path.parts) or '\\' in entry.filename:
                raise ValueError('archive: unsafe entry path')
            if (entry.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError('archive: symlink entry refused')
        raw = json.loads(archive.read('manifest.json'))
        if manifest['model_schema'] != 'robocad/' + str(raw.get('version')) or raw.get('format') != 'robocad':
            raise ValueError('model_schema: archive mismatch')
        if manifest['units'] != 'mm' or manifest['frame'] != 'cad-world':
            raise ValueError('model units/frame: authoritative RoboCAD mm CAD-world required')
        if raw.get('document_id') != manifest['document_id']:
            raise ValueError('identity.document_id: archive mismatch')
        groups = [raw['nodes']] + [d['nodes'] for d in raw.get('component_definitions', {}).values()]
        for nodes in groups:
            ids = {n['id'] for n in nodes}
            for node in nodes:
                if node.get('source') is not None and node['source'] not in ids:
                    raise ValueError('node.source: unresolved archive identity')
                image = node.get('image')
                if image and image.get('path') and 'image/' + node['id'] not in names:
                    raise ValueError('image.path: external image input unsupported by bounded corpus')
        settings = raw.get('robot_settings', {})
        if settings.get('system_link'):
            raise ValueError('robot_settings.system_link: operational external read unsupported')
        # Current physical exports retain system metadata without resolving it;
        # no refresh/link operation is allowed by the reference dispatch.
    return root


def receipt(step, status, outcome, message, doc, observations, executed_at):
    observations['operation.outcome'] = observation(outcome, owner='robocad.command',
        provenance='derived: actual authoritative dispatch outcome')
    return {'step_id': step['id'], 'status': status, 'expected': step['expected'],
        'message': message, 'process_document_id': doc.document_id,
        'revision': doc.revision, 'observations': observations, 'executed_at': executed_at}


def run(manifest, scenario, model, cancelled=lambda: False):
    if manifest.get('schema_version') != 1:
        raise ValueError('schema_version: unsupported manifest schema')
    validate_model(manifest, model)
    verified_bytes = read_regular(model)
    digest = hashlib.sha256(verified_bytes).hexdigest()
    if digest != manifest['source']['sha256']:
        raise ValueError('source.sha256: isolated model digest mismatch')
    doc = Document.load(io.BytesIO(verified_bytes))
    # Document.load accepts an archive stream; keep the owned path only as source
    # provenance after loading the exact verified bytes, never reopen for loading.
    doc.path = str(model)
    if doc.document_id != manifest['document_id']:
        raise ValueError('identity.document_id: archive identity differs from manifest')
    dispatcher = Dispatcher(doc, cancelled)
    receipts = []
    try:
        for step in scenario['operations']:
            if cancelled():
                receipts.append({'step_id': step['id'], 'status': 'not_run',
                    'expected': step['expected'], 'message': 'Cancelled before dispatch',
                    'process_document_id': doc.document_id, 'revision': doc.revision,
                    'observations': {}})
                continue
            stamp = datetime.datetime.now(datetime.timezone.utc).isoformat()
            observations = {}
            try:
                result = dispatcher.execute(step['operation'])
                observations = collect(doc, digest)
                observations['operation.result'] = result_observation(result)
                kind = step['operation']['kind']
                if kind == 'physical':
                    for key in ('links', 'joints', 'materials', 'uncertainty', 'source'):
                        observations['physical.' + key] = required_field(result, key,
                            owner='robocad.physical', provenance='derived: authoritative physical export')
                    if 'format' not in result or 'version' not in result:
                        observations['physical.schema'] = observation(state='missing',
                            reason='Physical reply omitted format/version', owner='robocad.physical')
                    elif not isinstance(result['format'], str) or type(result['version']) is not int:
                        observations['physical.schema'] = observation(state='invalid',
                            reason='Physical format/version have invalid types', owner='robocad.physical')
                    else:
                        observations['physical.schema'] = observation({'format': result['format'],
                            'version': result['version']}, owner='robocad.physical',
                            provenance='derived: authoritative physical export')
                if kind == 'pose':
                    for key in ('positions', 'closure_error_mm', 'identity', 'prior_applied'):
                        observations['motion.' + key] = required_field(result, key,
                            owner='robocad.motion_service', unit='mm' if key == 'closure_error_mm' else 'mixed:mm,rad' if key == 'positions' else None,
                            frame='cad-world' if key in ('closure_error_mm', 'positions') else None,
                            provenance='derived: PoseModel reference kinematic solver')
                if kind == 'captured':
                    captured_identity = result.get('identity', result)
                    identity_keys = {'document_id', 'revision', 'source_kind', 'source_id', 'physical_hash', 'archive_hash'}
                    if not isinstance(captured_identity, dict) or not identity_keys <= set(captured_identity):
                        observations['captured.identity'] = observation(state='missing',
                            reason='Captured reply omitted identity fields', owner='robocad.captured_review')
                    elif any(captured_identity[k] is None for k in identity_keys):
                        observations['captured.identity'] = observation(state='invalid',
                            reason='Captured identity contains null required fields', owner='robocad.captured_review')
                    else:
                        observations['captured.identity'] = observation(captured_identity,
                            owner='robocad.captured_review', provenance='derived: immutable CAD snapshot')
                    from .motion_service import identity
                    observations['live.identity'] = observation(identity(doc),
                        owner='robocad.motion_service', provenance='derived: live CAD identity')
                status = 'passed' if step['expected'] == 'success' else 'failed'
                outcome, message = 'success', 'Authoritative operation completed'
            except (KernelError, RevisionConflict) as error:
                status = 'passed' if step['expected'] == 'rejected' else 'failed'
                outcome, message = 'rejected', str(error)
                try:
                    observations = collect(doc, digest)
                except Exception as observation_error:
                    status = 'incomplete'
                    message += '; rejected command observation unavailable: ' + str(observation_error)
                    observations['adapter.observations'] = observation(state='invalid',
                        reason=str(observation_error), owner='robocad.parity_reference')
            except NotImplementedError as error:
                status, outcome, message = 'unsupported', 'unsupported', str(error)
            except InterruptedError as error:
                status, outcome, message = 'cancelled', 'cancelled', str(error)
            except Incomplete as error:
                status, outcome, message = 'incomplete', 'incomplete', str(error)
            except Exception as error:
                # A command may have mutated before raising an unexpected error.
                status, outcome, message = 'uncertain', 'uncertain', str(error)
            receipts.append(receipt(step, status, outcome, message, doc, observations, stamp))
            if status in ('uncertain', 'incomplete', 'cancelled'):
                for pending in scenario['operations'][len(receipts):]:
                    receipts.append({'step_id': pending['id'], 'status': 'not_run',
                        'expected': pending['expected'], 'message': 'Prior operation did not complete safely',
                        'process_document_id': doc.document_id, 'revision': doc.revision,
                        'observations': {}})
                break
    finally:
        try:
            dispatcher.close()
        except Incomplete as error:
            # Shutdown uncertainty invalidates the last receipt, never manufactures completion.
            if receipts:
                if receipts[-1]['status'] not in ('uncertain', 'cancelled'):
                    receipts[-1]['status'] = 'incomplete'
                receipts[-1]['message'] += '; component shutdown incomplete: ' + str(error)
    return {'identity': IDENTITY, 'source': manifest['source'], 'receipts': receipts}


def main(argv=None):
    parser = argparse.ArgumentParser(description='Direct RoboCAD parity reference (no GUI)')
    parser.add_argument('--manifest', required=True)
    parser.add_argument('--scenario')
    parser.add_argument('--model')
    parser.add_argument('--validate-model')
    parser.add_argument('--output')
    args = parser.parse_args(argv)
    manifest = json.loads(read_regular(args.manifest))
    if args.validate_model:
        validate_model(manifest, args.validate_model)
        return
    if not args.model or not args.output or not args.scenario:
        parser.error('--model, --output and --scenario are required for execution')
    model = Path(args.model)
    if model.is_symlink() or not model.is_file():
        raise ValueError('model: require isolated regular file')
    for ancestor in model.parents:
        if ancestor.is_symlink():
            raise ValueError('model: symlink ancestor refused')
    root = validate_model(manifest, model)
    if Path(args.output).absolute().parent != root:
        raise ValueError('output: must be a new direct child of owned workspace root')
    os.environ['ROBOCAD_LOG_DIR'] = str(root / 'logs')
    scenarios = [s for s in manifest['scenarios'] if s['id'] == args.scenario]
    if len(scenarios) != 1:
        raise ValueError('scenario: require exactly one matching scenario')
    stopped = [False]
    def cancel(_signum, _frame):
        stopped[0] = True
    signal.signal(signal.SIGTERM, cancel)
    signal.signal(signal.SIGINT, cancel)
    publish_new(args.output, run(manifest, scenarios[0], model, lambda: stopped[0]))


if __name__ == '__main__':
    main()
