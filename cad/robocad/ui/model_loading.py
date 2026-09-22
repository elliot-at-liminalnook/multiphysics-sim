"""Cancellable model preparation with an optional, disposable display cache.

A worker exclusively owns each new document until preparation completes. Cached
arrays are derived from the full archive hash and the renderer/kernel version;
source CAD geometry and its precision are never changed by this cache.
"""
from __future__ import annotations

import hashlib
import os
from pathlib import Path
import sys
import tempfile
import time
import zipfile
import uuid
import json
import subprocess
import queue
import threading

import numpy as np
from PySide6.QtCore import QObject, QTimer, Signal
from PySide6.QtWidgets import QApplication, QDialog, QLabel, QProgressBar, QPushButton, QVBoxLayout

from ..document import Document
from ..kernel.base import Mesh
from .viewport import RenderItem, prepare_render_item, _curve_item

CACHE_VERSION = 'display-1'
LOADERS = set()
LOAD_JOBS = {}


class LoadCancelled(Exception):
    pass


def default_cache_root():
    if sys.platform == 'darwin':
        return Path.home() / 'Library/Caches/robocad/display'
    return Path(os.environ.get('XDG_CACHE_HOME', Path.home() / '.cache')) / 'robocad/display'


def archive_key(path, check=lambda: None):
    import OCP
    digest = hashlib.sha256((CACHE_VERSION + OCP.__version__).encode())
    with open(path, 'rb') as stream:
        while True:
            check()
            chunk = stream.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


ARRAY_FIELDS = ('vertices', 'normals', 'indices', 'tri_face', 'edges',
                'edge_ref_index', 'vertex_points')


def write_display(path, mesh, item):
    arrays = {key: getattr(item, key) for key in ARRAY_FIELDS}
    arrays.update(mesh_vertices=np.asarray(mesh.vertices, dtype=np.float64),
                  mesh_normals=np.asarray(mesh.normals, dtype=np.float64),
                  mesh_triangles=np.asarray(mesh.triangles, dtype=np.int64),
                  mesh_faces=np.asarray(mesh.triangle_face, dtype=np.int64),
                  face_count=np.asarray(mesh.face_count), bbox=np.asarray(item.bbox),
                  sample_offsets=np.cumsum([0] + [len(s) for s in item.edge_samples]),
                  sample_points=np.concatenate(item.edge_samples) if item.edge_samples else np.empty((0, 3), np.float32))
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(dir=path.parent, suffix='.npz')
    try:
        with os.fdopen(fd, 'wb') as stream:
            np.savez(stream, **arrays)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def read_display(path, doc, node):
    with np.load(path, allow_pickle=False) as data:
        mesh = Mesh(data['mesh_vertices'].tolist(), data['mesh_normals'].tolist(),
                    data['mesh_triangles'].tolist(), data['mesh_faces'].tolist(), int(data['face_count']))
        arrays = {key: data[key] for key in ARRAY_FIELDS}
        for key in ('vertices', 'normals', 'indices', 'vertex_points'):
            if arrays[key].ndim != 2 or arrays[key].shape[1] != 3:
                raise ValueError('invalid cached display array')
        offsets, points = data['sample_offsets'], data['sample_points']
        if len(offsets) == 0 or offsets[0] != 0 or offsets[-1] != len(points) or np.any(np.diff(offsets) < 0):
            raise ValueError('invalid cached edge offsets')
        samples = [points[a:b] for a, b in zip(offsets[:-1], offsets[1:])]
        mat = doc.materials.get(node.material or '')
        color = node.color or (mat.color if mat else (0.72, 0.72, 0.75))
        item = RenderItem(node.id, **arrays, color=color, kind=node.kind,
                          bbox=tuple(map(tuple, data['bbox'])), face_count=mesh.face_count,
                          edge_samples=samples)
    return mesh, item


def prepare_model(path, progress=lambda *args: None, cancelled=lambda: False, cache_root=None):
    start = time.perf_counter()
    def check():
        if cancelled():
            raise LoadCancelled()
    before = os.stat(path)
    progress('Checking model', 0, 0, Path(path).name)
    key = archive_key(path, check)
    root = (Path(cache_root) if cache_root is not None else default_cache_root()) / key
    def reading(done, total, name):
        check()
        progress('Reading CAD', done, total, name)
    doc = Document.load(str(path), progress=reading)
    loaded = os.stat(path)
    if (before.st_size, before.st_mtime_ns, before.st_ino) != (loaded.st_size, loaded.st_mtime_ns, loaded.st_ino):
        raise ValueError('The CAD file changed while reading. Open it again to load the new version.')
    read_seconds = time.perf_counter() - start
    nodes = [n for n in doc.walk() if n.kind in ('body', 'sheet', 'curve', 'instance', 'mesh')]
    items, hits = {}, 0
    for index, node in enumerate(nodes):
        check()
        progress('Preparing display', index, len(nodes), node.name)
        # The archive hash includes IDs, geometry, transforms and tolerances.
        entry = root / (hashlib.sha256(node.id.encode()).hexdigest() + '.npz')
        mesh = item = None
        if node.kind == 'curve':
            item = _curve_item(doc, node)
        else:
            try:
                mesh, item = read_display(entry, doc, node)
                doc.mesh_cache[(node.id, node.tessellation_tolerance)] = mesh
                hits += 1
            except (OSError, ValueError, KeyError, EOFError, zipfile.BadZipFile):
                mesh = doc.mesh_of(node.id)
                if mesh is not None and mesh.vertices:
                    item = prepare_render_item(doc, node, mesh)
                    check()
                    try:
                        write_display(entry, mesh, item)
                    except OSError:
                        pass  # A read-only/full cache must not prevent opening CAD.
        if item is not None:
            items[node.id] = item
        progress('Preparing display', index + 1, len(nodes), node.name)
    check()
    after = os.stat(path)
    if (before.st_size, before.st_mtime_ns, before.st_ino) != (after.st_size, after.st_mtime_ns, after.st_ino):
        raise ValueError('The CAD file changed while loading. Open it again to load the new version.')
    return doc, items, {'read_seconds': read_seconds, 'prepare_seconds': time.perf_counter() - start,
                        'cache_hits': hits, 'parts': len(nodes)}


class LoadProcess(QObject):
    """Poll a detached child without blocking Qt or sharing the Python GIL."""
    progress = Signal(str, int, int, str)
    ready = Signal(object)
    failed = Signal(str)
    cancelled = Signal()
    finished = Signal()
    opening = Signal()

    def __init__(self, path, cache_root=None, benchmark=False, imports=()):
        super().__init__()
        self.path, self.cache_root = path, cache_root
        self.benchmark = benchmark
        self.imports = list(imports)
        self.process = None
        self.active = False
        self.handed_off = False
        self.messages = queue.Queue()
        self.diagnostics = ''
        self.timer = QTimer(self)
        self.timer.setInterval(16)
        self.timer.timeout.connect(self.poll)

    def start(self):
        env = os.environ.copy()
        env['PYTHONPATH'] = str(Path(__file__).resolve().parents[2]) + os.pathsep + env.get('PYTHONPATH', '')
        try:
            if not Path(self.path).is_file():
                raise FileNotFoundError(f'Model file does not exist: {self.path}')
            self.process = subprocess.Popen([sys.executable, '-u', '-m', 'robocad.ui.load_process',
                                             self.path, str(self.cache_root or ''), '--imports-json', json.dumps(self.imports)]
                                             + (['--benchmark'] if self.benchmark else []),
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                env=env, start_new_session=True)
            # Only pipe IO runs in this thread. All geometry stays in the child.
            # Unlike nonblocking pipe descriptors this also works on Windows.
            threading.Thread(target=self.read_messages, daemon=True).start()
            self.active = True
            self.timer.start()
        except Exception as error:
            QTimer.singleShot(0, lambda message=str(error): self.fail(message))

    def isRunning(self):
        return self.active

    def read_messages(self):
        try:
            while True:
                line = self.process.stdout.readline(1024 * 1024)
                if not line:
                    break
                self.messages.put(line)
        except (OSError, ValueError):
            pass
        finally:
            self.process.stdout.close()
            self.messages.put(None)

    def poll(self):
        if not self.active:
            return
        deadline = time.perf_counter() + .004
        while self.active and time.perf_counter() < deadline:
            try:
                line = self.messages.get_nowait()
            except queue.Empty:
                break
            if line is None:
                self.fail(self.diagnostics or 'The model loading process exited unexpectedly.')
                break
            try:
                message = json.loads(line)
            except (ValueError, UnicodeDecodeError):
                self.diagnostics = (self.diagnostics + line.decode(errors='replace'))[-2000:]
                continue
            event = message.get('event') if isinstance(message, dict) else None
            if event == 'diagnostics':
                self.diagnostics = 'Diagnostics: ' + message['log_path']
            elif event == 'progress':
                self.progress.emit(message['stage'], message['done'], message['total'], message['name'])
            elif event == 'prepared':
                # After this transition the child may own unsaved edits. Never
                # terminate it in response to a late Cancel or launcher exit.
                try:
                    self.process.stdin.write(b'show\n'); self.process.stdin.flush()
                except OSError as error:
                    self.fail(str(error))
                    break
                self.process.stdin.close()
                self.handed_off = True
                self.opening.emit()
            elif event == 'ready':
                self.active = False
                self.timer.stop()
                self.ready.emit(message['stats'])
                self.finished.emit()
            elif event == 'failed':
                self.fail(message['error'])

    def fail(self, message):
        self.active = False
        self.timer.stop()
        self.failed.emit(message)
        self.finished.emit()

    def requestInterruption(self):
        if not self.active or self.handed_off:
            return
        self.active = False
        self.timer.stop()
        self.process.terminate()
        self.cancelled.emit()
        self.finished.emit()
        self.process.stdin.close()

    def wait(self, timeout=None):
        # Called on launcher shutdown. A handed-off window is independent.
        if self.process and not self.handed_off:
            try:
                self.process.wait(timeout=1 if timeout is None else timeout / 1000)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()


class ModelLoadDialog(QDialog):
    """Nonmodal progress; existing documents remain usable and untouched."""
    def __init__(self, path, on_ready, cache_root=None, *, benchmark=False, imports=()):
        super().__init__()
        self.setWindowTitle('Opening ' + Path(path).name)
        self.setMinimumWidth(460)
        layout = QVBoxLayout(self)
        self.label = QLabel('Checking model…')
        self.detail = QLabel(''); self.detail.setWordWrap(True)
        self.progress = QProgressBar(); self.progress.setRange(0, 0)
        self.progress.setMinimumHeight(24)
        self.progress.setStyleSheet('QProgressBar { border: 1px solid #526477; border-radius: 3px; '
                                   'background: #171d25; color: #ffffff; text-align: center; } '
                                   'QProgressBar::chunk { background: #216887; }')
        self.cancel_button = QPushButton('Cancel')
        for widget in (self.label, self.progress, self.detail, self.cancel_button):
            layout.addWidget(widget)
        self.started = time.perf_counter()
        self.load_id = uuid.uuid4().hex
        self.job = {'id': self.load_id, 'path': str(path), 'state': 'loading',
                    'stage': 'Checking model', 'completed': 0, 'total': 0}
        LOAD_JOBS[self.load_id] = self.job
        # Retain recent terminal statuses for REST clients, without keeping
        # their documents or windows alive.
        for job_id in list(LOAD_JOBS):
            if len(LOAD_JOBS) <= 100:
                break
            if LOAD_JOBS[job_id]['state'] in ('ready', 'failed', 'cancelled'):
                del LOAD_JOBS[job_id]
        self._stage = 'Checking model'
        self.elapsed_timer = QTimer(self)
        self.elapsed_timer.setInterval(100)
        self.elapsed_timer.timeout.connect(self.update_elapsed)
        self.elapsed_timer.start()
        self.cancel_requested = False
        self.result = None
        self.on_ready = on_ready
        self.worker = LoadProcess(str(path), cache_root, benchmark, imports)
        self.worker.progress.connect(self.update_progress)
        self.worker.ready.connect(self.complete)
        self.worker.failed.connect(self.failure)
        self.worker.cancelled.connect(self.cancelled)
        self.worker.finished.connect(self.worker_finished)
        self.worker.opening.connect(self.opening)
        QApplication.instance().aboutToQuit.connect(self.stop_on_exit)
        self.cancel_button.clicked.connect(self.cancel)
        LOADERS.add(self)
        self.show()
        self.worker.start()

    def update_progress(self, stage, done, total, name):
        if self.cancel_requested:
            return
        self.job.update(stage=stage, completed=done, total=total, part=name)
        self._stage = {'Reading CAD': '1 / 2 · Reading CAD',
                       'Preparing display': '2 / 2 · Preparing display'}.get(stage, stage)
        self.update_elapsed()
        self.progress.setRange(0, total)
        self.progress.setValue(done)
        self.progress.setFormat(f'{done} / {total} ' + ('parts' if stage == 'Preparing display' else 'items') + ' (%p%)')
        self.detail.setText(name)

    def update_elapsed(self):
        self.job['elapsed_seconds'] = time.perf_counter() - self.started
        if not self.cancel_requested:
            self.label.setText(f'{self._stage} — {time.perf_counter() - self.started:.0f}s elapsed')

    def complete(self, result):
        if self.cancel_requested:
            return
        self.result = result
        self.elapsed_timer.stop()
        self.label.setText('Opening prepared model…')
        try:
            self.on_ready(result)
        except Exception as error:
            self.failure(str(error))
        else:
            self.job.update(state='ready', stats=result, elapsed_seconds=time.perf_counter() - self.started)
            self.accept()

    def failure(self, message):
        self.elapsed_timer.stop()
        if self.cancel_requested:
            QDialog.reject(self)
            return
        self.label.setText('Could not open model')
        self.job.update(state='failed', error=message)
        self.detail.setText(message)
        self.progress.setRange(0, 1); self.progress.setValue(0)
        self.cancel_button.setText('Close')

    def cancelled(self):
        self.job['state'] = 'cancelled'
        QDialog.reject(self)

    def reject(self):
        # Escape is cancellation too, not merely hiding a live loading job.
        self.cancel()

    def worker_finished(self):
        # Keep the supervisor until preparation or cancellation has finished.
        self.elapsed_timer.stop()
        if self.cancel_requested:
            self.job['state'] = 'cancelled'
            QDialog.reject(self)
        if not self.isVisible():
            LOADERS.discard(self)

    def cancel(self):
        self.elapsed_timer.stop()
        if self.worker.handed_off:
            if not self.worker.isRunning():
                LOADERS.discard(self)
                QDialog.reject(self)
            return
        if not self.worker.isRunning():
            if self.job['state'] in ('loading', 'cancelling'):
                # The worker may have queued ready just before Escape arrived.
                self.cancel_requested = True
                self.job['state'] = 'cancelled'
            LOADERS.discard(self)
            QDialog.reject(self)
            return
        self.cancel_requested = True
        self.job['state'] = 'cancelling'
        self.label.setText('Cancelling…')
        self.detail.setText('Stopping model preparation.')
        self.cancel_button.setEnabled(False)
        self.worker.requestInterruption()

    def opening(self):
        self._stage = 'Opening prepared model'
        self.job['stage'] = self._stage
        self.progress.setRange(0, 0)
        self.detail.setText('The full model is ready. Opening its editor window…')
        self.cancel_button.setEnabled(False)

    def stop_on_exit(self):
        self.worker.requestInterruption()
        self.worker.wait()

    def closeEvent(self, event):
        if self.worker.isRunning():
            self.cancel()
            event.ignore()
        else:
            LOADERS.discard(self)
            super().closeEvent(event)
