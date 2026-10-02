"""Authoritative imported bindings and source guards; fixtures not executed."""
from copy import deepcopy
from types import SimpleNamespace
import pytest
from robocad.api import ApiError, Service
from robocad.component_graph import edit_graph
from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import KernelError

CATALOGUE = [{'type': 'fixture.multi', 'parameters': [], 'ports': [
    {'name': 'terminal.*', 'schema': {'Acausal': 'Thermal'}, 'direction': 'acausal'}]}]
IMPORTED = [{'binding': 'source/unit', 'name': 'Unit', 'type': 'fixture.multi',
    'parameters': {}, 'ports': [
        {'name': 'terminal.hot', 'schema': {'Acausal': 'Thermal'}, 'direction': 'acausal'},
        {'name': 'terminal.cold', 'schema': {'Acausal': 'Thermal'}, 'direction': 'acausal'}]}]


def test_bound_mutation_requires_authoritative_matching_import():
    doc = Document(); ops = Ops(doc)
    operation = {'action': 'add_component', 'id': 'stable-cad-id', 'component': {
        'name': 'Unit', 'type': 'fixture.multi', 'parameters': {}, 'binding': 'source/unit'}}
    with pytest.raises(KernelError, match='components.stable-cad-id.binding'):
        edit_graph(doc, ops, operation, doc.revision, CATALOGUE)
    assert not doc.component_graph['components']
    result = edit_graph(doc, ops, operation, doc.revision, CATALOGUE, IMPORTED)
    assert result['graph']['components']['stable-cad-id']['binding'] == 'source/unit'
    ops.undo()
    assert not doc.component_graph['components']


def test_imported_concrete_ports_are_used_for_connection_validation():
    doc = Document(); ops = Ops(doc)
    for identity in ('a', 'b', 'c'):
        # Use distinct source bindings, as graph storage prohibits duplicates.
        all_imported = [{**IMPORTED[0], 'binding': f'source/{key}', 'name': key} for key in ('a', 'b', 'c')]
        edit_graph(doc, ops, {'action': 'add_component', 'id': identity, 'component': {
            'name': identity, 'type': 'fixture.multi', 'binding': f'source/{identity}', 'parameters': {}}},
            doc.revision, CATALOGUE, all_imported)
    all_imported = [{**IMPORTED[0], 'binding': f'source/{key}', 'name': key} for key in ('a', 'b', 'c')]
    operation = {'action': 'connect', 'id': 'stable-net', 'ports': [
        {'component_id': identity, 'port': 'terminal.hot'} for identity in ('a', 'b', 'c')]}
    result = edit_graph(doc, ops, operation, doc.revision, CATALOGUE, all_imported)
    assert len(result['graph']['connections']['stable-net']['ports']) == 3
    assert result['graph']['connections']['stable-net']['id'] == 'stable-net'
    invalid = {'action': 'connect', 'ports': [{'component_id': 'a', 'port': 'terminal.missing'}]}
    revision = doc.revision
    with pytest.raises(KernelError, match='components.a.ports.terminal.missing'):
        edit_graph(doc, ops, invalid, revision, CATALOGUE, all_imported)
    assert doc.revision == revision


def test_completed_check_refuses_stale_physical_source_or_wrong_document():
    doc = Document(); api = Service(doc)
    record = {'document_id': doc.document_id}
    metadata = {'state': 'completed', 'stale': False, 'metadata_stale': False,
        'guard_document_id': doc.document_id, 'guard_revision': doc.revision, 'imported': deepcopy(IMPORTED)}
    api._experiments = SimpleNamespace(get=lambda _: record, components=lambda _: metadata)
    checked = api.checked_graph_imports('check')
    assert checked['document_id'] == doc.document_id and checked['imported'] == IMPORTED
    metadata['metadata_stale'] = True
    with pytest.raises(ApiError, match='structural metadata is stale'): api.checked_graph_imports('check')
    metadata['metadata_stale'] = False; record['document_id'] = 'other'
    with pytest.raises(ApiError, match='another document'): api.checked_graph_imports('check')


def test_checked_metadata_cannot_cross_intervening_owner_edit():
    doc = Document(); api = Service(doc)
    checked = {'document_id': doc.document_id, 'revision': doc.revision, 'imported': IMPORTED}
    api.ops.box((0, 0, 0), (1, 1, 1))
    with pytest.raises(ApiError, match='changed while reading'):
        api.system_request('POST', {'expected_revision': doc.revision}, ['system', 'components'], imported_check=checked)


def test_structural_import_guard_survives_graph_edits_but_not_cad_edits():
    from robocad.component_imports import metadata_guard
    from robocad.snapshots import capture
    doc = Document(); ops = Ops(doc)
    ops.box((0, 0, 0), (1, 1, 1))
    before = capture(doc)
    record = {'state': 'completed', 'document_id': doc.document_id, 'provenance': {
        'physical_hash': before.physical_hash, 'cad_derivation_hash': before.cad_derivation_hash}}
    edit_graph(doc, ops, {'action': 'add_component', 'id': 'first', 'component': {
        'name': 'first', 'type': 'fixture.multi', 'parameters': {}}}, doc.revision, CATALOGUE)
    after = capture(doc)
    assert after.physical_hash != before.physical_hash
    assert after.cad_derivation_hash == before.cad_derivation_hash
    guard = metadata_guard(doc, record, after)
    assert not guard['metadata_stale'] and guard['guard_revision'] == doc.revision
    # Captured result/value freshness continues to use the full physical hash.
    assert record['provenance']['physical_hash'] != after.physical_hash
    edit_graph(doc, ops, {'action': 'add_component', 'id': 'bound', 'component': {
        'name': 'bound', 'type': 'fixture.multi', 'binding': 'source/unit', 'parameters': {}}},
        guard['guard_revision'], CATALOGUE, IMPORTED)
    ops.box((3, 0, 0), (1, 1, 1))
    assert metadata_guard(doc, record)['metadata_stale']


def test_legacy_import_record_conservatively_uses_full_physical_hash():
    from robocad.component_imports import metadata_guard
    from robocad.snapshots import capture
    doc = Document(); ops = Ops(doc)
    before = capture(doc)
    record = {'state': 'completed', 'document_id': doc.document_id,
        'provenance': {'physical_hash': before.physical_hash, 'cad_derivation_hash': ''}}
    assert not metadata_guard(doc, record)['metadata_stale']
    edit_graph(doc, ops, {'action': 'add_component', 'id': 'first', 'component': {
        'name': 'first', 'type': 'fixture.multi', 'parameters': {}}}, doc.revision, CATALOGUE)
    assert metadata_guard(doc, record)['metadata_stale']


def test_components_route_labels_results_stale_while_structure_is_current(tmp_path):
    import json
    from robocad.experiments import Experiments
    from robocad.snapshots import capture
    doc = Document(); ops = Ops(doc)
    before = capture(doc)
    record = {'state': 'completed', 'document_id': doc.document_id, 'revision': doc.revision,
        'provenance': {'physical_hash': before.physical_hash, 'cad_derivation_hash': before.cad_derivation_hash}}
    folder = tmp_path / 'check'; folder.mkdir()
    (folder / 'imported_components.json').write_text(json.dumps(IMPORTED))
    manager = SimpleNamespace(doc=doc, root=tmp_path, get=lambda _: record)
    edit_graph(doc, ops, {'action': 'add_component', 'id': 'first', 'component': {
        'name': 'first', 'type': 'fixture.multi', 'parameters': {}}}, doc.revision, CATALOGUE)
    response = Experiments.components(manager, 'check')
    assert response['stale'] and not response['metadata_stale']
    assert response['guard_document_id'] == doc.document_id and response['guard_revision'] == doc.revision
    ops.box((0, 0, 0), (1, 1, 1))
    assert Experiments.components(manager, 'check')['metadata_stale']


def test_metadata_worker_returns_run_generation_without_touching_draft(monkeypatch):
    import threading
    from robocad.component_imports import refresh_async
    started = []
    class FakeThread:
        def __init__(self, target, **kwargs): self.target = target
        def start(self): started.append(self.target)
    monkeypatch.setattr(threading, 'Thread', FakeThread)
    draft = {'name': 'unsaved name', 'parameter': 'rejected expression'}
    events = []
    response = {'run_id': 'check', 'metadata_stale': False, 'stale': True}
    manager = SimpleNamespace(components=lambda _: response)
    refresh_async(manager, 'check', 7, events.append)
    assert not events and draft['name'] == 'unsaved name'
    started[0]()
    assert events == [(7, response)]
    assert draft == {'name': 'unsaved name', 'parameter': 'rejected expression'}


def test_graph_mutation_refuses_replacement_document_even_at_same_revision():
    doc = Document(); api = Service(doc)
    source = deepcopy(doc.component_graph)
    for method, parts, body, query in (
        ('PUT', ['system'], {'expected_revision': doc.revision, 'document_id': 'replaced-doc', 'graph': source}, {}),
        ('DELETE', ['system', 'components', 'missing'], {}, {'expected_revision': str(doc.revision), 'document_id': 'replaced-doc'}),
    ):
        with pytest.raises(ApiError) as refusal:
            api.system_request(method, body, parts, query)
        assert refusal.value.status == 409
        assert doc.component_graph == source
    assert api.system_request('GET', {}, ['system'])['document_id'] == doc.document_id
