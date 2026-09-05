import pytest

from robocad.api import ApiError, Service
from robocad.candidates import Candidates
from robocad.commands import Ops
from robocad.document import Document
from robocad.experiments import RevisionConflict
from robocad.kernel import KernelError
from robocad.snapshots import capture


def test_atomic_batch_has_one_undo_and_resolves_prior_results(tmp_path):
    doc = Document(); ops = Ops(doc); candidates = Candidates(doc, ops, tmp_path)
    before = capture(doc)
    events = []; doc.listeners.append(lambda event, payload: events.append(event))
    record = candidates.batch({'expected_revision': doc.revision, 'operations': [
        {'op': 'box', 'args': [[0, 0, 0], [10, 20, 30]], 'as': 'part'},
        {'op': 'rename', 'args': [{'$ref': 'part'}, 'Candidate part']},
        {'op': 'set_material', 'args': [[{'$ref': 'part'}], 'petg']}]})
    part = record['results']['part']
    assert doc.nodes[part].name == 'Candidate part' and doc.nodes[part].material == 'petg'
    assert len(ops.stack.undo_stack) == 1 and events == ['changed']
    assert record['changes']['physical_changed']
    ops.undo(); assert capture(doc).physical_hash == before.physical_hash
    ops.redo(); assert doc.nodes[part].name == 'Candidate part'


def test_failed_batch_and_revision_conflict_leave_document_untouched(tmp_path):
    doc = Document(); ops = Ops(doc); candidates = Candidates(doc, ops, tmp_path)
    p = ops.box((0, 0, 0), (10, 10, 10)); before = capture(doc)
    history = list(ops.stack.undo_stack)
    with pytest.raises(KernelError, match='Operation 1'):
        candidates.batch({'expected_revision': doc.revision, 'operations': [
            {'op': 'transform', 'args': [[p]], 'kwargs': {'scale': 2}},
            {'op': 'rename', 'args': ['missing', 'oops']}]})
    assert capture(doc).data == before.data and ops.stack.undo_stack == history
    with pytest.raises(RevisionConflict):
        candidates.batch({'expected_revision': doc.revision - 1, 'operations': [{'op': 'delete', 'args': [[p]]}]})
    with pytest.raises(KernelError, match='not an atomic'):
        candidates.batch({'expected_revision': doc.revision, 'operations': [{'op': 'physical', 'kwargs': {'path': str(tmp_path/'external.json')}}]})
    assert not (tmp_path/'external.json').exists()


def test_candidate_runs_separately_and_accepts_exact_base_with_undo(tmp_path):
    doc = Document(); ops = Ops(doc); candidates = Candidates(doc, ops, tmp_path)
    p = ops.box((0, 0, 0), (10, 10, 10)); before = capture(doc)
    candidate = candidates.create({'expected_revision': doc.revision, 'operations': [
        {'op': 'transform', 'args': [[p]], 'kwargs': {'scale': 2}}]})
    assert capture(doc).data == before.data
    assert candidates.document(candidate['id']).kernel.mass_properties(candidates.document(candidate['id']).nodes[p].body).volume == pytest.approx(8000)
    loaded = Candidates(doc, ops, tmp_path)
    assert loaded.list()[0]['id'] == candidate['id']
    loaded.accept(candidate['id'], doc.revision)
    assert doc.kernel.mass_properties(doc.nodes[p].body).volume == pytest.approx(8000)
    assert capture(doc).physical_hash == candidate['physical_hash']
    ops.undo(); assert capture(doc).physical_hash == before.physical_hash
    ops.redo(); assert capture(doc).physical_hash == candidate['physical_hash']


def test_intervening_comment_rejects_candidate_without_losing_user_work(tmp_path):
    doc = Document(); ops = Ops(doc); candidates = Candidates(doc, ops, tmp_path)
    p = ops.box((0, 0, 0), (10, 10, 10))
    candidate = candidates.create({'expected_revision': doc.revision, 'operations': [{'op': 'rename', 'args': [p, 'Agent name']}]})
    tid = ops.create_thread(p, [0, 0, 0], 'Keep this concern')
    with pytest.raises(RevisionConflict, match='rebuild the candidate'):
        candidates.accept(candidate['id'], doc.revision)
    assert tid in doc.annotations and doc.nodes[p].name != 'Agent name'
    service = Service(doc, ops); service._candidates = candidates
    with pytest.raises(ApiError) as error:
        service.candidate_request('POST', ['candidates', candidate['id'], 'accept'], {'expected_revision': doc.revision})
    assert error.value.status == 409
