"""T44 source-review fixtures. Written only; not executed by this batch."""
from types import SimpleNamespace
import json
import pytest

from robocad.parity_observations import observation, collect, source_identity, required_field
from robocad.parity_reference import publish_new, receipt
from robocad.parity_operations import Dispatcher, Incomplete


def test_nonfinite_is_invalid_not_missing_or_unsupported():
    for value in (float('nan'), {'nested': [float('inf')]}):
        assert observation(value)['value']['state'] == 'invalid'
    assert observation(state='missing', reason='absent')['value'] == {'state': 'missing', 'value': 'absent'}
    assert observation(state='unsupported', reason='kernel')['value']['state'] == 'unsupported'


def test_missing_reply_keys_and_invalid_null_are_distinct():
    assert required_field({}, 'positions')['value']['state'] == 'missing'
    assert required_field({'positions': None}, 'positions')['value']['state'] == 'invalid'
    assert required_field({'positions': {}}, 'positions')['value']['state'] == 'present'


def test_identity_comes_from_real_file_not_expected_hash(tmp_path):
    path = tmp_path / 'source.rcad'
    path.write_bytes(b'actual source')
    doc = SimpleNamespace(path=str(path))
    with pytest.raises(ValueError, match='digest|hash mismatch'):
        source_identity(doc, '0' * 64)
    alias = tmp_path / 'alias.rcad'
    alias.symlink_to(path)
    with pytest.raises(ValueError, match='non-symlink'):
        source_identity(SimpleNamespace(path=str(alias)))


def test_missing_provenance_is_not_filled_from_manifest(tmp_path):
    path = tmp_path / 'source.rcad'
    path.write_bytes(b'actual source')
    doc = SimpleNamespace(path=str(path), document_id='actual-process-id',
        revision=4, robot_settings={}, walk=lambda: [])
    values = collect(doc)
    assert values['provenance']['value']['state'] == 'missing'
    assert values['provenance']['provenance'] is None
    assert values['identity.document_id']['value']['value'] == 'actual-process-id'
    assert values['model.units']['unit'] == 'mm'
    assert values['model.frame']['frame'] == 'cad-world'


def test_publication_refuses_overwrite_and_symlink_parent(tmp_path):
    path = tmp_path / 'record.json'
    publish_new(path, {'first': True})
    with pytest.raises(FileExistsError):
        publish_new(path, {'second': True})
    assert json.loads(path.read_text()) == {'first': True}
    alias = tmp_path / 'alias'
    alias.symlink_to(tmp_path, target_is_directory=True)
    with pytest.raises(ValueError, match='directory'):
        publish_new(alias / 'other.json', {})


def test_receipt_preserves_process_identity_and_actual_rejection():
    doc = SimpleNamespace(document_id='unexpected-document', revision=10)
    result = receipt({'id': 'stale', 'expected': 'rejected'}, 'passed', 'rejected',
        'Revision refused', doc, {}, 'actual-execution-time')
    assert result['process_document_id'] == 'unexpected-document'
    assert result['observations']['operation.outcome']['value']['value'] == 'rejected'


def test_dispatcher_cancel_before_mutation():
    dispatcher = object.__new__(Dispatcher)
    dispatcher.cancelled = lambda: True
    with pytest.raises(InterruptedError):
        dispatcher.execute({'kind': 'rename', 'node': 'n', 'name': 'changed'})


def test_reference_does_not_dispatch_filesystem_or_render_ops():
    dispatcher = object.__new__(Dispatcher)
    dispatcher.cancelled = lambda: False
    for name in ('physical', 'link_system', 'export_component', 'save', 'load_results'):
        with pytest.raises(NotImplementedError, match='allowlist'):
            dispatcher.execute({'kind': 'op', 'name': name, 'args': [], 'kwargs': {}})


def test_component_start_receives_revision_and_document_guard():
    dispatcher = object.__new__(Dispatcher)
    dispatcher.doc = SimpleNamespace(revision=7, document_id='actual')
    seen = []
    class Owner:
        def start(self, name, args, kwargs, revision, document_id):
            seen.append((name, revision, document_id))
            return {'id': 'job'}
        def status(self, identity, cancel=False):
            assert identity == 'job' and cancel
            return {'state': 'cancelled'}
    dispatcher.components = Owner()
    dispatcher.cancelled = lambda: False
    with pytest.raises(InterruptedError, match='acknowledged'):
        dispatcher.component({'operation': {'operation': 'overrides',
            'instance_id': 'root', 'overrides': {}, 'placement': None},
            'revision_offset': -1, 'cancel': True, 'interfere': None})
    assert seen == [('set_component_overrides', 6, 'actual')]


def test_pose_continuation_only_publishes_validated_service_result(monkeypatch):
    from robocad import parity_operations
    dispatcher = object.__new__(Dispatcher)
    dispatcher.doc = SimpleNamespace(revision=3, document_id='source')
    dispatcher.cancelled = lambda: False
    dispatcher.prior = {'identity': {'document_id': 'source'}, 'positions': {'passive': .2}}
    sent = []
    def sample(doc, body):
        sent.append(body)
        raise ValueError('malformed prior')
    monkeypatch.setattr(parity_operations.motion_service, 'sample', sample)
    previous = dispatcher.prior
    with pytest.raises(ValueError):
        dispatcher.execute({'kind': 'pose', 'positions': {}, 'time': 0,
            'continuation': True, 'revision_offset': 0})
    assert sent[0]['prior'] is previous
    assert dispatcher.prior is previous


def test_captured_source_missing_never_falls_back_to_live():
    from robocad.kernel import KernelError
    dispatcher = object.__new__(Dispatcher)
    dispatcher.captured = {}
    with pytest.raises(KernelError, match='Captured source not found'):
        dispatcher.capture({'capture': 'absent', 'action': 'geometry', 'args': {}})


def test_owned_real_model_validation_requires_digest_schema_and_dependency_closure(tmp_path):
    """Reads a repository-owned manifest fixture; copies only into pytest ownership."""
    import hashlib
    from pathlib import Path
    from robocad.parity_reference import validate_model
    repository = Path(__file__).resolve().parents[2]
    manifest = json.loads((repository / 'examples/cad-parity/printable.json').read_text())
    workspace = tmp_path / 'isolated'
    workspace.mkdir()
    (workspace / '.cad-parity-owner').write_text(json.dumps({'source_sha256': manifest['source']['sha256']}))
    for item in [manifest['source'], *manifest['dependencies']]:
        original = repository / item['path']
        data = original.read_bytes()
        assert hashlib.sha256(data).hexdigest() == item['sha256']
        target = workspace / item['path']
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    model = workspace / manifest['source']['path']
    assert validate_model(manifest, model) == workspace
    wrong = dict(manifest, units='m')
    with pytest.raises(ValueError, match='units/frame'):
        validate_model(wrong, model)
    wrong = dict(manifest, document_id='different')
    with pytest.raises(ValueError, match='identity.document_id'):
        validate_model(wrong, model)
    wrong = dict(manifest, dependencies=[{'path': '../escape', 'sha256': '0' * 64}])
    with pytest.raises(ValueError, match='unsafe relative path'):
        validate_model(wrong, model)
    model.write_bytes(b'corrupted source')
    with pytest.raises(ValueError, match='digest mismatch'):
        validate_model(manifest, model)


def test_component_crash_is_incomplete_not_expected_guard_rejection():
    dispatcher = object.__new__(Dispatcher)
    dispatcher.doc = SimpleNamespace(revision=7, document_id='actual')
    dispatcher.cancelled = lambda: False
    class Owner:
        def start(self, *args): return {'id': 'job'}
        def status(self, *args, **kwargs):
            return {'state': 'failed', 'error': 'Component worker failed',
                    'document_id': 'actual', 'revision': 7}
    dispatcher.components = Owner()
    with pytest.raises(Incomplete, match='worker failed'):
        dispatcher.component({'operation': {'operation': 'overrides',
            'instance_id': 'root', 'overrides': {}, 'placement': None},
            'revision_offset': 0, 'cancel': False, 'interfere': None})


def test_component_guard_rejection_requires_captured_live_mismatch():
    from robocad.experiments import RevisionConflict
    dispatcher = object.__new__(Dispatcher)
    dispatcher.doc = SimpleNamespace(revision=8, document_id='actual')
    dispatcher.cancelled = lambda: False
    class Owner:
        def start(self, *args): return {'id': 'job'}
        def status(self, *args, **kwargs):
            return {'state': 'failed',
                    'error': 'The document changed during preparation. Your edits are preserved; retry the component operation.',
                    'document_id': 'actual', 'revision': 7}
    dispatcher.components = Owner()
    with pytest.raises(RevisionConflict, match='document changed'):
        dispatcher.component({'operation': {'operation': 'overrides',
            'instance_id': 'root', 'overrides': {}, 'placement': None},
            'revision_offset': 0, 'cancel': False, 'interfere': None})


def test_publication_keeps_owned_directory_after_name_is_replaced(tmp_path, monkeypatch):
    """A same-UID rename/symlink race must not redirect record publication."""
    import os
    from robocad.parity_paths import publish_new
    owned = tmp_path / 'owned'
    owned.mkdir()
    elsewhere = tmp_path / 'elsewhere'
    elsewhere.mkdir()
    retained = tmp_path / 'retained'
    original = os.link
    def swapped_link(source, destination, **kwargs):
        owned.rename(retained)
        owned.symlink_to(elsewhere, target_is_directory=True)
        return original(source, destination, **kwargs)
    monkeypatch.setattr(os, 'link', swapped_link)
    publish_new(owned / 'receipt.json', {'actual': True})
    assert not (elsewhere / 'receipt.json').exists()
    assert json.loads((retained / 'receipt.json').read_text()) == {'actual': True}


def test_verified_stream_and_late_observation_failure_retain_receipts(tmp_path, monkeypatch):
    import hashlib
    import io
    from robocad import parity_reference
    from robocad.kernel import KernelError
    model = tmp_path / 'model.rcad'
    model.write_bytes(b'verified archive fixture')
    digest = hashlib.sha256(model.read_bytes()).hexdigest()
    manifest = {'schema_version': 1, 'document_id': 'durable',
                'source': {'path': 'model.rcad', 'sha256': digest}}
    doc = SimpleNamespace(document_id='durable', revision=3, path=None)
    loaded = []
    def load(stream):
        assert isinstance(stream, io.BytesIO)
        loaded.append(stream.getvalue())
        return doc
    monkeypatch.setattr(parity_reference.Document, 'load', load)
    monkeypatch.setattr(parity_reference, 'validate_model', lambda *args: tmp_path)
    class Direct:
        def __init__(self, *args): pass
        def execute(self, op):
            if op['kind'] == 'rename': raise KernelError('Authoritative refusal')
            return None
        def close(self): pass
    monkeypatch.setattr(parity_reference, 'Dispatcher', Direct)
    observations = []
    def collect(*args):
        observations.append(True)
        if len(observations) > 1: raise ValueError('late observation failure')
        return {}
    monkeypatch.setattr(parity_reference, 'collect', collect)
    scenario = {'operations': [
        {'id': 'first', 'operation': {'kind': 'observe'}, 'expected': 'success'},
        {'id': 'refused', 'operation': {'kind': 'rename'}, 'expected': 'rejected'}]}
    run = parity_reference.run(manifest, scenario, model)
    assert loaded == [b'verified archive fixture']
    assert run['receipts'][0]['status'] == 'passed'
    assert run['receipts'][1]['status'] == 'incomplete'
    assert run['receipts'][1]['observations']['operation.outcome']['value']['value'] == 'rejected'
    assert run['receipts'][1]['observations']['adapter.observations']['value']['state'] == 'invalid'
