"""Windowless service ownership and queued-result fixtures; written, not run."""
from types import SimpleNamespace
import pytest
from robocad.component_jobs import ComponentJob, snapshot
from robocad.component_service import ComponentJobService, owner
from robocad.commands import Ops
from robocad.document import Document
from robocad.experiments import RevisionConflict


class QueuedJob(ComponentJob):
    """Real snapshot/commit code, fake preparation with no process or Qt."""
    def start(self):
        self.state = 'running'
        return self.id

    def ready(self):
        self.messages.put(('ready', (snapshot(self.input), {}, {'instance_id': 'fixture'})))


def fixture_owner():
    doc = Document()
    ops = Ops(doc)
    holder = SimpleNamespace(ops=ops)
    service = ComponentJobService(lambda: holder.ops, job_factory=QueuedJob)
    return doc, ops, holder, service


def test_headless_owner_commits_once_and_undoes():
    doc, ops, _, service = fixture_owner()
    status = service.start('new_parametric_component', expected_revision=doc.revision, document_id=doc.document_id)
    job = service.jobs[status['id']]
    job.ready()
    assert service.status(job.id)['state'] == 'applied'
    revision = doc.revision
    service.status(job.id)
    assert doc.revision == revision
    assert job.input is None and job.output is None
    assert ops.undo() is not None


def test_start_refuses_revision_and_document_before_preparation():
    doc, _, _, service = fixture_owner()
    for guard in ({'expected_revision': doc.revision + 1}, {'document_id': 'another-document'}):
        with pytest.raises(RevisionConflict): service.start('new_parametric_component', **guard)
    assert service.jobs == {}


def test_cancel_queued_ready_cannot_commit():
    doc, _, _, service = fixture_owner()
    status = service.start('new_parametric_component')
    job = service.jobs[status['id']]
    revision = doc.revision
    job.ready()
    assert service.status(job.id, cancel=True)['state'] == 'cancelled'
    assert doc.revision == revision and job.input is None and job.output is None


def test_stale_result_releases_snapshot_and_preserves_edit():
    doc, ops, _, service = fixture_owner()
    status = service.start('new_parametric_component')
    job = service.jobs[status['id']]
    body = ops.box((0, 0, 0), (1, 2, 3))
    job.ready()
    assert service.status(job.id)['state'] == 'failed'
    assert body in doc.nodes and job.input is None and job.output is None


def test_replacement_document_refuses_old_job_even_at_same_revision():
    doc, _, holder, service = fixture_owner()
    status = service.start('new_parametric_component')
    job = service.jobs[status['id']]
    holder.ops = Ops(Document())
    assert holder.ops.doc.revision == doc.revision
    job.ready()
    assert service.status(job.id)['state'] == 'failed'
    assert holder.ops.doc.revision == 0


def test_qt_and_rest_attachment_share_owner_without_qt_import():
    _, ops, _, _ = fixture_owner()
    assert owner(ops) is owner(ops)


def test_export_cancel_ready_preserves_destination(tmp_path):
    doc, _, _, service = fixture_owner()
    target = tmp_path / 'existing.rcomp'
    target.write_bytes(b'keep existing library')
    status = service.start('export_component', ['definition', str(target)])
    job = service.jobs[status['id']]
    temporary = tmp_path / '.prepared.rcomp'
    temporary.write_bytes(b'new library')
    job.export_target, job.export_temporary = str(target), str(temporary)
    job.ready()
    assert service.status(job.id, cancel=True)['state'] == 'cancelled'
    assert target.read_bytes() == b'keep existing library'
    assert not temporary.exists()


def test_service_api_headless_returns_identity_and_commits_same_owner():
    from robocad.api import Service
    doc, ops, _, _ = fixture_owner()
    api = Service(doc, ops)
    api.component_jobs.job_factory = QueuedJob
    response = api.op('new_parametric_component', [], {}, doc.revision, doc.document_id)
    status = response['job']
    assert status['document_id'] == doc.document_id and status['revision'] == doc.revision
    assert api.component_jobs is owner(ops)
    api.component_jobs.jobs[status['id']].ready()
    assert api.component_job_status(status['id'])['state'] == 'applied'


def test_cancel_after_poll_ready_releases_result_before_commit():
    doc, _, _, service = fixture_owner()
    status = service.start('new_parametric_component')
    job = service.jobs[status['id']]
    job.ready()
    job.poll()
    assert job.state == 'ready'
    revision = doc.revision
    assert service.status(job.id, cancel=True)['state'] == 'cancelled'
    assert doc.revision == revision and job.output is None


def test_failed_worker_releases_snapshot_with_named_diagnostics():
    _, _, _, service = fixture_owner()
    status = service.start('place_component', ['missing-definition'])
    job = service.jobs[status['id']]
    job.log_path = '/fixture/component-worker.log'
    job.messages.put(('failed', 'missing definition; Diagnostics: /fixture/component-worker.log'))
    result = service.status(job.id)
    assert result['state'] == 'failed' and 'missing definition' in result['error']
    assert result['log_path'] == job.log_path and job.input is None


def test_job_listing_recovers_lost_start_response_without_second_start():
    doc, _, _, service = fixture_owner()
    service.start('new_parametric_component', expected_revision=doc.revision, document_id=doc.document_id)
    statuses = service.discover()
    matches = [status for status in statuses if status['operation'] == 'new_parametric_component'
        and status['document_id'] == doc.document_id and status['revision'] == doc.revision]
    assert len(matches) == 1 and len(service.jobs) == 1


def test_discovery_of_ready_job_never_publishes_before_pending_cancel():
    doc, _, _, service = fixture_owner()
    status = service.start('new_parametric_component')
    job = service.jobs[status['id']]
    revision = doc.revision
    job.ready()
    assert service.discover()[0]['state'] == 'ready'
    assert doc.revision == revision
    assert service.status(job.id, cancel=True)['state'] == 'cancelled'
    assert doc.revision == revision
