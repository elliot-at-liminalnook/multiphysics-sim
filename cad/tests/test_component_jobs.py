import time
from pathlib import Path
import pytest
from robocad.commands import Ops
from robocad.document import Document
from robocad.component_jobs import ComponentJob, pack, unpack, snapshot


@pytest.fixture(autouse=True)
def isolated_logs(tmp_path, monkeypatch):
    monkeypatch.setenv('ROBOCAD_LOG_DIR', str(tmp_path / 'logs'))


def finish(job):
    deadline=time.monotonic()+30
    while job.state in ('running','pending') and time.monotonic()<deadline:
        job.poll(); time.sleep(.01)
    assert job.state=='ready',job.status()


def test_process_prepares_then_commits_and_undoes():
    doc=Document(); ops=Ops(doc)
    job=ComponentJob(doc,'new_parametric_component'); job.start(); finish(job)
    assert not doc.component_definitions
    job.commit(ops)
    assert job.state=='applied'
    definition=job.result
    placed=ComponentJob(doc,'place_component',[definition]); placed.start(); finish(placed)
    assert not doc.nodes
    placed.commit(ops)
    assert len(doc.bodies())==1 and len(doc.mesh_cache)==1
    assert placed.prepared
    ops.undo(); assert not doc.nodes
    ops.redo(); assert len(doc.bodies())==1


def test_stale_background_result_preserves_intervening_edits():
    doc=Document(); ops=Ops(doc)
    job=ComponentJob(doc,'new_parametric_component'); job.start()
    body=ops.box((0,0,0),(1,2,3))
    finish(job); job.commit(ops)
    assert job.state=='failed' and 'changed' in job.error
    assert list(doc.nodes)==[body] and not doc.component_definitions


def test_cancel_does_not_apply():
    doc=Document(); ops=Ops(doc)
    job=ComponentJob(doc,'new_parametric_component'); job.start(); job.cancel()
    deadline=time.monotonic()+15
    while job.state in ('running','pending') and time.monotonic()<deadline: job.poll(); time.sleep(.01)
    assert job.state=='cancelled' and not doc.nodes and not doc.component_definitions


def test_failed_worker_keeps_traceback_after_temporary_job_cleanup():
    doc = Document()
    job = ComponentJob(doc, 'place_component', ['missing-definition'])
    job.start()
    deadline = time.monotonic() + 15
    while job.state in ('running', 'pending') and time.monotonic() < deadline:
        job.poll(); time.sleep(.01)
    assert job.state == 'failed'
    assert job.log_path and Path(job.log_path).is_file()
    text = Path(job.log_path).read_text()
    assert 'component_operation' in text and 'Traceback' in text
    assert job.log_path in job.error
    assert not doc.nodes
