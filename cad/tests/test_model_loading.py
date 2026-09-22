"""Load correctness, cache invalidation, cancellation and UI event delivery."""
import hashlib
import time

import numpy as np
import pytest
from PySide6.QtCore import QTimer
from PySide6.QtWidgets import QApplication

from robocad.document import Document
from robocad.commands import Ops
from robocad.ui import model_loading as loading
from robocad.ui.viewport import Viewport


@pytest.fixture(autouse=True)
def isolated_logs(tmp_path, monkeypatch):
    monkeypatch.setenv('ROBOCAD_LOG_DIR', str(tmp_path / 'logs'))


@pytest.fixture
def model(tmp_path):
    doc = Document(); ops = Ops(doc)
    ops.box((0, 0, 0), (10, 20, 30))
    path = tmp_path / 'model.rcad'; doc.save(str(path))
    return path


def test_cached_open_matches_cold_and_never_edits_source(model, tmp_path, monkeypatch):
    digest = hashlib.sha256(model.read_bytes()).hexdigest()
    progress = []
    cold, items, stats = loading.prepare_model(model, lambda *args: progress.append(args), cache_root=tmp_path/'cache')
    assert stats['cache_hits'] == 0 and progress[-1][1:3] == (1, 1)
    def forbidden(*args, **kwargs):
        raise AssertionError('Warm opening must reuse display geometry')
    monkeypatch.setattr(loading, 'prepare_render_item', forbidden)
    warm, cached, stats = loading.prepare_model(model, cache_root=tmp_path/'cache')
    assert stats['cache_hits'] == 1
    assert warm.document_id == cold.document_id and warm.revision == cold.revision
    for nid, item in items.items():
        for field in loading.ARRAY_FIELDS:
            np.testing.assert_array_equal(getattr(item, field), getattr(cached[nid], field))
        assert item.face_count == cached[nid].face_count
        for a, b in zip(item.edge_samples, cached[nid].edge_samples):
            np.testing.assert_array_equal(a, b)
        old_mesh = cold.mesh_of(nid); new_mesh = warm.mesh_of(nid)
        for field in ('vertices', 'normals', 'triangles', 'triangle_face'):
            np.testing.assert_array_equal(getattr(old_mesh, field), getattr(new_mesh, field))
    assert not cold.dirty and not warm.dirty
    assert hashlib.sha256(model.read_bytes()).hexdigest() == digest


def test_cache_invalidation_and_corruption(model, tmp_path):
    root = tmp_path/'cache'
    doc, _, _ = loading.prepare_model(model, cache_root=root)
    entry = next(root.rglob('*.npz')); entry.write_bytes(b'broken cache')
    _, _, stats = loading.prepare_model(model, cache_root=root)
    assert stats['cache_hits'] == 0
    Ops(doc).box((50, 0, 0), (3, 4, 5)); doc.save(str(model))
    _, items, stats = loading.prepare_model(model, cache_root=root)
    assert stats['cache_hits'] == 0 and len(items) == 2


def test_cancelled_load_never_delivers_document(model, tmp_path):
    stop = False
    def progress(stage, *args):
        nonlocal stop
        if stage == 'Preparing display':
            stop = True
    with pytest.raises(loading.LoadCancelled):
        loading.prepare_model(model, progress, lambda: stop, cache_root=tmp_path/'cache')


def test_prepared_viewport_does_not_repeat_geometry(model, tmp_path, monkeypatch):
    app = QApplication.instance() or QApplication([])
    doc, items, _ = loading.prepare_model(model, cache_root=tmp_path/'cache')
    vp = Viewport(doc, prepared=items)
    def forbidden(*args, **kwargs):
        raise AssertionError('Opening prepared geometry must not recalculate it')
    monkeypatch.setattr(vp, 'rebuild_item', forbidden)
    vp.focus_all()
    assert len(vp.items) == 1
    vp.deleteLater(); app.processEvents()


def test_nonmodal_cancel_keeps_ui_alive_and_preserves_existing_edits(model, tmp_path, monkeypatch):
    app = QApplication.instance() or QApplication([])
    existing = Document(); Ops(existing).box((0, 0, 0), (1, 2, 3))
    before = existing.to_manifest(); ready = []
    dialog = loading.ModelLoadDialog(model, lambda *args: ready.append(args), tmp_path/'cache')
    ticks = []
    timer = QTimer(); timer.setInterval(10); timer.timeout.connect(lambda: ticks.append(time.monotonic())); timer.start()
    QTimer.singleShot(80, dialog.reject)
    deadline = time.monotonic() + 5
    while (dialog.worker.isRunning() or dialog.isVisible()) and time.monotonic() < deadline:
        app.processEvents(); time.sleep(.002)
    timer.stop(); dialog.worker.wait(1000); app.processEvents()
    assert len(ticks) >= 4 and not ready
    assert dialog.worker.process.poll() is not None
    assert not dialog.isVisible() and dialog not in loading.LOADERS
    assert existing.dirty and existing.nodes.keys() == {n['id'] for n in before['nodes']}


def test_failed_load_is_reported_and_can_close(tmp_path):
    app = QApplication.instance() or QApplication([])
    ready = []
    dialog = loading.ModelLoadDialog(tmp_path/'missing.rcad', lambda *args: ready.append(args))
    deadline = time.monotonic() + 3
    while dialog.worker.isRunning() and time.monotonic() < deadline:
        app.processEvents(); time.sleep(.002)
    app.processEvents()
    assert not ready and dialog.label.text() == 'Could not open model'
    dialog.cancel()
    assert dialog not in loading.LOADERS


def test_process_load_reports_ready_without_mutating_original(model, tmp_path):
    app = QApplication.instance() or QApplication([])
    ready = []
    digest = hashlib.sha256(model.read_bytes()).hexdigest()
    dialog = loading.ModelLoadDialog(model, ready.append, tmp_path/'cache', benchmark=True)
    deadline = time.monotonic() + 20
    while dialog.worker.isRunning() and time.monotonic() < deadline:
        app.processEvents(); time.sleep(.002)
    app.processEvents()
    assert len(ready) == 1, dialog.detail.text()
    assert dialog.job['state'] == 'ready'
    assert ready[0]['parts'] == 1 and ready[0]['api_url']
    from pathlib import Path
    log_path = Path(ready[0]['log_path'])
    assert log_path.is_file()
    assert hashlib.sha256(model.read_bytes()).hexdigest() == digest
    dialog.worker.process.wait(timeout=10)
    log = log_path.read_text()
    assert 'editor_ready' in log and 'process_exit' in log


def test_rest_open_uses_same_async_job_and_cancel(model, monkeypatch):
    from robocad.ui.app import MainWindow
    from robocad.api import Service
    app = QApplication.instance() or QApplication([])
    monkeypatch.setattr(MainWindow, 'start_api', lambda self: None)
    window = MainWindow()
    service = Service(window.doc, window.ops, window)
    start = time.monotonic()
    opened = service.open(str(model))
    assert time.monotonic() - start < 1 and opened['loading']
    state = service.load_status(opened['load_id'], cancel=True)
    assert state['state'] in ('cancelling', 'cancelled')
    app.processEvents()
    assert service.load_status(opened['load_id'])['state'] == 'cancelled'
    assert len(window.doc.nodes) == 0 and not window.doc.dirty
    window.close()


def test_handed_off_window_is_never_terminated_by_launcher():
    class IndependentWindow:
        def terminate(self):
            raise AssertionError('Must preserve the independent editor and its unsaved edits')
        def wait(self, **kwargs):
            raise AssertionError('Launcher must not wait for an independent editor to close')
    worker = loading.LoadProcess('unused.rcad')
    worker.process = IndependentWindow()
    worker.active = True
    worker.handed_off = True
    worker.requestInterruption()
    worker.wait()
    assert worker.active
