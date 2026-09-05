"""Rhai authoring, background run history and synchronized captured-run review."""
import json
import math
from pathlib import Path
from copy import deepcopy
import time
import uuid
import threading

from PySide6.QtCore import QPointF, QRectF, Qt, QTimer, Signal
from PySide6.QtGui import QColor, QPainter, QPainterPath, QPen, QTextCursor
from PySide6.QtWidgets import (QCheckBox, QComboBox, QDialog, QFileDialog, QHBoxLayout,
    QLabel, QListWidget, QListWidgetItem, QPlainTextEdit, QProgressBar, QPushButton,
    QSlider, QSplitter, QTabWidget, QVBoxLayout, QWidget)

from ..experiments import DEFAULT_CONTROLLER, DEFAULT_SYSTEM, TERMINAL, sources, write_json
from ..experiment_results import compare, replay_flex, replay_matrices, sample_index, signals, value_at
from ..kernel import KernelError
from ..snapshots import capture
from .viewport import Viewport


class TracePlot(QWidget):
    time_selected = Signal(float)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.series = self.baseline = None
        self.time = 0.
        self.setMinimumHeight(180)
        self.setCursor(Qt.CrossCursor)
        self.setToolTip('Click or drag to inspect a recorded sample. Blue: selected run. Gold: baseline.')

    def set_series(self, series, baseline=None):
        self.series, self.baseline = series, baseline
        self.update()

    def bounds(self):
        series = [s for s in (self.series, self.baseline) if s and s['t']]
        if not series: return 0., 1., -1., 1.
        lo, hi = min(min(s['values']) for s in series), max(max(s['values']) for s in series)
        margin = max((hi-lo)*.08, abs(hi)*1e-12, 1e-12) if hi > lo else max(abs(hi)*.01, 1e-6)
        return min(s['t'][0] for s in series), max(s['t'][-1] for s in series), lo-margin, hi+margin

    def plot_rect(self): return QRectF(68, 32, max(1, self.width()-88), max(1, self.height()-66))

    def axis_offset(self, lo, hi):
        if self.series and self.series['values'] and max(abs(lo), abs(hi)) > 100*(hi-lo):
            return self.series['values'][0]
        return 0.

    def paintEvent(self, event):
        p = QPainter(self); p.setRenderHint(QPainter.Antialiasing)
        p.fillRect(self.rect(), QColor('#171d25'))
        r = self.plot_rect(); x0, x1, y0, y1 = self.bounds()
        offset = self.axis_offset(y0, y1)
        def point(t, v):
            return QPointF(r.left()+(t-x0)/max(x1-x0, 1e-12)*r.width(), r.bottom()-(v-y0)/(y1-y0)*r.height())
        p.setPen(QPen(QColor('#34414e'), 1))
        for k in range(5):
            y = r.top()+r.height()*k/4
            p.setPen(QPen(QColor('#34414e'), 1))
            p.drawLine(QPointF(r.left(), y), QPointF(r.right(), y))
            p.setPen(QColor('#becbd8'))
            p.drawText(QRectF(0, y-9, 61, 18), Qt.AlignRight, f'{y1-(y1-y0)*k/4-offset:.3g}')
        p.setPen(QColor('#becbd8'))
        if offset:
            p.drawText(QPointF(r.left(), 19), f'Change from {offset:.9g} {self.series["unit"]}')
        p.drawText(QPointF(r.left(), r.bottom()+23), f'{x0:.3g} s')
        p.drawText(QRectF(r.right()-85, r.bottom()+5, 85, 24), Qt.AlignRight, f'{x1:.3g} s')
        if not self.series:
            p.drawText(r, Qt.AlignCenter, 'Select a signal to inspect its trace')
            return
        p.save(); p.setClipRect(r)
        for series, color in ((self.baseline, '#edb85c'), (self.series, '#70d2f6')):
            if not series or not series['t']: continue
            path = QPainterPath(); path.moveTo(point(series['t'][0], series['values'][0]))
            previous = series['values'][0]
            for t, v in zip(series['t'][1:], series['values'][1:]):
                if series.get('interpolation') == 'hold': path.lineTo(point(t, previous))
                path.lineTo(point(t, v)); previous = v
            p.setPen(QPen(QColor(color), 1.5)); p.drawPath(path)
        x = point(self.time, 0).x()
        p.setPen(QPen(QColor('#f4f6f8'), 1, Qt.DashLine))
        p.drawLine(QPointF(x, r.top()), QPointF(x, r.bottom()))
        p.restore()

    def pick(self, e):
        if self.series:
            r = self.plot_rect(); lo, hi, _, _ = self.bounds()
            self.time_selected.emit(lo + max(0., min(1., (e.position().x()-r.left())/r.width()))*(hi-lo))

    def mousePressEvent(self, e):
        if e.button() == Qt.LeftButton: self.pick(e)

    def mouseMoveEvent(self, e):
        if e.buttons() & Qt.LeftButton: self.pick(e)


class RunReview(QDialog):
    """Owns a separate captured document; no modelling operations are installed."""
    def __init__(self, panel, run_id, baseline_id=None):
        super().__init__(panel.app)
        self.panel, self.run_id = panel, run_id
        self.result = panel.manager.result(run_id)
        self.catalogue = signals(self.result)
        self.baseline = panel.manager.result(baseline_id) if baseline_id and baseline_id != run_id else None
        self.baseline_signals = signals(self.baseline) if self.baseline else {}
        self.doc = panel.manager.captured_document(run_id)
        self.viewport = None
        self.flex_arrows = []
        self.render_samples = []
        self.source_windows = []
        self.setWindowTitle(f'Experiment {run_id[:8]} — captured result')
        self.resize(1150, 820)
        self.setAttribute(Qt.WA_DeleteOnClose)
        layout = QVBoxLayout(self)
        job = panel.manager.get(run_id)
        self.summary = QLabel(); self.summary.setWordWrap(True)
        settings = self.result.get('settings', {})
        evaluation = self.result.get('evaluation', {}).get('status', 'unchecked')
        stale = 'Live CAD has changed' if self.result.get('stale') else 'Matches live CAD inputs'
        if self.doc is None: stale = 'Script-only system'
        self.summary.setText(f"{job['label']} · {evaluation} · {stale}\n"
            f"Captured revision {job.get('revision')} · {self.result.get('profile', 'legacy')} · seed {self.result.get('seed', 0)} · "
            f"contact {'on' if settings.get('contact') else 'off'} · CAD sensor noise {'on' if settings.get('noise') else 'off'} · "
            f"flex {'on' if settings.get('flex') else 'off'} · step {settings.get('step')} s · "
            f"sample {settings.get('sample')} s\nSimulated replay · geometry in mm, physical signals in SI units · "
            f"controller: {self.result.get('controller_interface', 'position_target')}")
        layout.addWidget(self.summary)
        self.summary_text = self.summary.text()
        self.association_timer = QTimer(self); self.association_timer.setSingleShot(True)
        self.association_timer.timeout.connect(self.refresh_association)
        panel.app.doc.listeners.append(self.live_changed)
        row = QHBoxLayout()
        self.part = QComboBox(); self.part.addItem('All parts and signals', None)
        if self.doc:
            for node in self.doc.walk(): self.part.addItem(node.name, node.id)
        self.part.currentIndexChanged.connect(self.filter_signals)
        row.addWidget(self.part)
        self.signal = QComboBox(); self.signal.currentIndexChanged.connect(self.select_signal)
        row.addWidget(self.signal, 1); layout.addLayout(row)
        actions = QHBoxLayout()
        self.component_action = QPushButton('Inspect live component')
        self.component_action.setToolTip('Open this component in the live document inspector. The captured result remains unchanged.')
        self.component_action.clicked.connect(lambda: panel.app._safe(self.inspect_component)); actions.addWidget(self.component_action)
        self.source_action = QPushButton('View captured source')
        self.source_action.clicked.connect(lambda: panel.app._safe(self.view_source)); actions.addWidget(self.source_action)
        self.frame_action = QPushButton('Frame replay')
        self.frame_action.setEnabled(self.doc is not None)
        self.frame_action.setToolTip('Fit the current simulated poses and scaled flex arrows in the viewport.')
        self.frame_action.clicked.connect(self.frame_replay); actions.addWidget(self.frame_action)
        actions.addStretch(); layout.addLayout(actions)
        split = QSplitter(Qt.Vertical)
        if self.doc:
            self.viewport = Viewport(self.doc, self)
            self.viewport.frameSwapped.connect(lambda: self.render_samples.append(self.viewport.frame_ms))
            self.viewport.tool_name = 'Simulated replay'
            self.viewport.tool_hint = 'Click a part to filter signals • right-drag to orbit • model is captured'
            self.viewport.dragged.connect(self.pick_part)
            self.viewport.overlays.append(self.draw_flex)
            self.viewport.focus_all()
            split.addWidget(self.viewport)
        self.plot = TracePlot(self)
        self.plot.time_selected.connect(self.seek)
        split.addWidget(self.plot); split.setSizes([420, 200]); layout.addWidget(split, 1)
        controls = QHBoxLayout()
        self.play = QPushButton('Play'); self.play.clicked.connect(self.toggle_play); controls.addWidget(self.play)
        self.slider = QSlider(Qt.Horizontal); self.slider.valueChanged.connect(self.show_sample)
        self.times = self.result.get('trace', {}).get('t', [])
        self.slider.setRange(0, max(0, len(self.times)-1)); controls.addWidget(self.slider, 1)
        self.readout = QLabel(); self.readout.setMinimumWidth(170); controls.addWidget(self.readout)
        layout.addLayout(controls)
        self.flex_scale = QComboBox()
        for scale in (1, 10, 100, 1000):
            self.flex_scale.addItem(f'Flex boundary arrows ×{scale} · rigid CAD mesh', scale)
        self.flex_scale.setToolTip('Arrows show world-space displacement at captured attachment frames. Only arrows are magnified; plotted values remain in metres. No full surface deformation is reconstructed.')
        self.flex_scale.setVisible(bool(self.result.get('trace', {}).get('flex')))
        self.flex_scale.currentIndexChanged.connect(lambda: self.show_sample(self.slider.value()))
        layout.addWidget(self.flex_scale)
        self.metrics = QPlainTextEdit(); self.metrics.setReadOnly(True); self.metrics.setMaximumHeight(110)
        lines = [f"{ 'PASS' if m['passed'] else 'FAIL'} · {m['name']}: {m['value']:.6g} {m['unit']} "
                 f"(min {m.get('min', '—')}, max {m.get('max', '—')})"
                 for m in self.result.get('evaluation', {}).get('metrics', [])]
        lines.extend(f"{name}: {value['value']:.6g} {value['unit']}" for name, value in self.result.get('objectives', {}).items() if name.startswith('mass/'))
        for derivation in self.result.get('component_derivations', []):
            values = ', '.join(f"{name} = {value['value']:.6g} {value['unit']}" for name, value in derivation['outputs'].items())
            lines.append(f"{derivation['name']} · {derivation['formula']} · {values}")
        lines.extend('Model limit: '+str(limit) for limit in self.result.get('limitations', []))
        lines.extend('Simulation warning: '+str(warning) for warning in self.result.get('warnings', []))
        if self.baseline:
            report = compare(self.baseline, self.result)
            lines.insert(0, f"Blue: selected run · Gold: baseline {baseline_id[:8]} · " + ('; '.join(report['caveats']) or 'Same scenario and settings'))
            labels = {'source_hash': 'system script', 'parameters_hash': 'scenario parameters',
                'controller_hash': 'controller', 'physical_hash': 'physical model',
                'component_graph_hash': 'components and connections', 'cad_derivation_hash': 'CAD properties',
                'binary_hash': 'simulator version', 'seed': 'random seed'}
            lines.append('Changed inputs: ' + (', '.join(labels.get(k, k) for k in report['changed_inputs']) or 'none'))
            lines.extend(f"Δ {name}: {value['delta']:+.6g} {value['unit']}" if value['comparable'] else f"{name}: {value['reason']}"
                         for name, value in report['objectives'].items())
        self.metrics.setPlainText('\n'.join(lines) or 'No measured expectations declared in configure(#{ expectations: [...] }).')
        layout.addWidget(self.metrics)
        notes = QHBoxLayout()
        self.note = QPlainTextEdit(); self.note.setMaximumHeight(58); self.note.setPlaceholderText('Discuss this run at the selected signal and time…')
        notes.addWidget(self.note, 1)
        button = QPushButton('Annotate sample'); button.clicked.connect(lambda: panel.app._safe(self.annotate)); notes.addWidget(button)
        layout.addLayout(notes)
        self.timer = QTimer(self); self.timer.setInterval(33); self.timer.timeout.connect(self.tick)
        self.filter_signals(); self.show_sample(0)
        self.frame_replay()

    def filter_signals(self):
        nid = self.part.currentData()
        if self.viewport:
            self.viewport.selection.set_nodes([nid] if nid else []); self.viewport.update()
        previous = self.signal.currentData()
        self.signal.blockSignals(True); self.signal.clear()
        for name, series in self.catalogue.items():
            if nid is None or nid in series['node_ids']:
                self.signal.addItem(f"{name} [{series['unit']}]", name)
        index = self.signal.findData(previous)
        if index >= 0: self.signal.setCurrentIndex(index)
        self.signal.blockSignals(False); self.select_signal()

    def live_changed(self, event, payload):
        if event == 'changed': self.association_timer.start(0)

    def refresh_association(self):
        self.refresh_component_actions()
        if self.doc is None: return
        stale = capture(self.panel.app.doc).physical_hash != self.result.get('provenance', {}).get('physical_hash')
        self.result['stale'] = stale
        text = self.summary_text.replace('Live CAD has changed', 'Matches live CAD inputs')
        if stale: text = text.replace('Matches live CAD inputs', 'Live CAD has changed')
        self.summary.setText(text)

    def select_signal(self):
        name = self.signal.currentData()
        series = self.catalogue.get(name)
        baseline = next((s for s in self.baseline_signals.values() if series and s['identity'] == series['identity']), None)
        if baseline and series and baseline['unit'] != series['unit']: baseline = None
        self.plot.set_series(series, baseline)
        self.refresh_component_actions()
        self.show_sample(self.slider.value())

    def refresh_component_actions(self):
        series = self.catalogue.get(self.signal.currentData(), {})
        self.component_action.setEnabled(series.get('component_id') in self.panel.app.doc.component_graph['components'])
        self.source_action.setEnabled(bool(series.get('source')))

    def inspect_component(self):
        identity = self.catalogue.get(self.signal.currentData(), {}).get('component_id')
        if identity not in self.panel.app.doc.component_graph['components']:
            raise KernelError('This captured component is absent from the live document')
        graph = self.panel.graph_panel
        graph.choose_component(identity); self.panel.tabs.setCurrentWidget(graph)
        self.panel.app.experiments_dock.show(); self.panel.app.experiments_dock.raise_()
        self.panel.app.raise_(); self.panel.app.activateWindow()

    def view_source(self):
        source = self.catalogue.get(self.signal.currentData(), {}).get('source')
        if not source: raise KernelError('This signal has no captured script declaration')
        files = self.panel.manager.source_bundles(self.run_id)['system']['files']
        if source['path'] not in files: raise KernelError(f"Captured source {source['path']} is unavailable")
        dialog = QDialog(self); dialog.setAttribute(Qt.WA_DeleteOnClose)
        dialog.setWindowTitle(f"Captured {source['path']}:{source['line']} · run {self.run_id[:8]}")
        dialog.resize(850, 600); layout = QVBoxLayout(dialog)
        label = QLabel('Captured run input · read only'); layout.addWidget(label)
        dialog.editor = QPlainTextEdit(); dialog.editor.setReadOnly(True); dialog.editor.setPlainText(files[source['path']])
        block = dialog.editor.document().findBlockByNumber(source['line']-1)
        if block.isValid():
            cursor = QTextCursor(block); cursor.select(QTextCursor.LineUnderCursor)
            dialog.editor.setTextCursor(cursor); dialog.editor.ensureCursorVisible()
        layout.addWidget(dialog.editor)
        self.source_windows.append(dialog)
        dialog.destroyed.connect(lambda *_: self.source_windows.remove(dialog) if dialog in self.source_windows else None)
        dialog.show(); return dialog

    def pick_part(self, event):
        kind, _, _, _, position = event
        if kind == 'release':
            def picked(result):
                nid = result['hit'][1] if result.get('hit') else None
                self.part.setCurrentIndex(max(0, self.part.findData(nid)))
            self.viewport.request_pick(position.x(), position.y(), picked)

    def seek(self, time):
        if self.times: self.slider.setValue(sample_index(self.times, time))

    def show_sample(self, index):
        if not self.times: return
        t = self.times[index]
        if self.viewport:
            self.flex_arrows = replay_flex(self.result, index, self.flex_scale.currentData())
            self.viewport.set_pose(replay_matrices(self.result, index))
        self.plot.time = t; self.plot.update()
        series = self.catalogue.get(self.signal.currentData())
        value = ''
        if series and series['t'] and series['t'][0] <= t <= series['t'][-1]:
            value = f" · {value_at(series, t):.9g} {series['unit']}"
        self.readout.setText(f'{t:.3f} s{value}')

    def draw_flex(self, painter):
        selected = self.part.currentData()
        camera = self.viewport.camera
        w, h = self.viewport.width(), self.viewport.height()
        painter.save()
        painter.setPen(QPen(QColor('#edb85c'), 2))
        for arrow in self.flex_arrows:
            if selected and selected not in arrow['node_ids']: continue
            start = camera.project(arrow['point_mm'], w, h)
            end = camera.project(arrow['tip_mm'], w, h)
            if start is None or end is None: continue
            a, b = QPointF(*start[:2]), QPointF(*end[:2])
            painter.drawEllipse(a, 3, 3); painter.drawLine(a, b)
            dx, dy = b.x()-a.x(), b.y()-a.y()
            length = math.hypot(dx, dy)
            if length > 2:
                ux, uy = dx/length, dy/length
                for side in (-1, 1):
                    painter.drawLine(b, b-QPointF(7*ux+side*3*uy, 7*uy-side*3*ux))
            value = math.hypot(*arrow['displacement_m'])
            painter.drawText(b+QPointF(8, -6), f'{arrow["name"]}: {value:.3g} m')
        painter.restore()

    def frame_replay(self):
        if not self.viewport: return
        lo, hi = self.viewport.scene_bounds()
        points = [lo, hi, *(a[key] for a in self.flex_arrows for key in ('point_mm', 'tip_mm'))]
        self.viewport.camera.focus(tuple(min(p[i] for p in points) for i in range(3)),
                                   tuple(max(p[i] for p in points) for i in range(3)))
        self.viewport.update()

    def toggle_play(self):
        if self.timer.isActive(): self.timer.stop(); self.play.setText('Play')
        elif self.times:
            if self.slider.value() == self.slider.maximum(): self.slider.setValue(0)
            self.started = time.monotonic(); self.start_time = self.times[self.slider.value()]
            self.timer.start(); self.play.setText('Pause')

    def tick(self):
        t = self.start_time + time.monotonic() - self.started
        self.seek(t)
        if t >= self.times[-1]: self.toggle_play()

    def annotate(self):
        evidence = {'run_id': self.run_id, 'physical_hash': self.result.get('provenance', {}).get('physical_hash')}
        evidence = {k: v for k, v in evidence.items() if v is not None}
        if self.times:
            t = self.times[self.slider.value()]; evidence['time_range'] = [t, t]
        if self.signal.currentData(): evidence['signal'] = self.signal.currentData()
        series = self.catalogue.get(self.signal.currentData(), {})
        if series.get('source'): evidence['source'] = series['source']
        if self.part.currentData(): evidence['node_ids'] = [self.part.currentData()]
        tid = self.panel.app.ops.create_thread(body=self.note.toPlainText(), evidence=evidence)
        self.note.clear(); self.panel.app.comments.select(tid)

    def closeEvent(self, event):
        self.timer.stop()
        self.association_timer.stop()
        self.panel.app.doc.listeners.remove(self.live_changed)
        if self.viewport and self.viewport._on_doc_event in self.doc.listeners:
            self.doc.listeners.remove(self.viewport._on_doc_event)
        if self.render_samples:
            write_json(self.panel.manager.root/self.run_id/('review-'+uuid.uuid4().hex+'.json'),
                {'run_id': self.run_id, 'frames': len(self.render_samples),
                 'render_cpu_mean_ms': sum(self.render_samples)/len(self.render_samples),
                 'render_cpu_max_ms': max(self.render_samples),
                 'definition': 'Viewport paintGL CPU time including scene synchronization; excludes GPU completion and display latency'})
        super().closeEvent(event)


class ExperimentsPanel(QWidget):
    run_changed = Signal(str)
    catalogue_ready = Signal(object)

    def heightForWidth(self, width):
        # Editors, parameter tables and the graph manage their own scrolling.
        # Their preferred content heights must not enlarge the dock's scroll page.
        return self.minimumSizeHint().height()

    def __init__(self, app):
        super().__init__(app)
        self.app, self.manager = app, app.experiments
        self.reviews = []
        self.linked = {}
        self.baseline_id = None
        self.auto_run_id = None
        self.check_run_id = None; self.loaded_check_id = None
        self.manager.changed.append(self.run_changed.emit)
        self.run_changed.connect(self.refresh)
        layout = QVBoxLayout(self)
        intro = QLabel('Edit CAD, system or controller, then run a captured experiment. Continue modelling while it runs.')
        intro.setWordWrap(True); layout.addWidget(intro)
        self.tabs = QTabWidget(); layout.addWidget(self.tabs, 1)
        self.system = QPlainTextEdit(DEFAULT_SYSTEM)
        self.controller = QPlainTextEdit(DEFAULT_CONTROLLER)
        self.parameters = QPlainTextEdit(json.dumps({'system': {}, 'controller': {'target': .3},
            'settings': {}, 'seed': 0}, indent=2))
        self.editor_layouts = {}
        for title, editor in [('System · Rhai', self.system), ('Controller · Rhai', self.controller), ('Parameters', self.parameters)]:
            page = QWidget(); page_layout = QVBoxLayout(page); page_layout.setContentsMargins(0, 4, 0, 0)
            page_layout.addWidget(editor, 1)
            self.editor_layouts[title] = page_layout
            self.tabs.addTab(page, title)
            editor.textChanged.connect(self.edited)
        history = QWidget(); hist = QVBoxLayout(history)
        self.history = QListWidget(); self.history.currentItemChanged.connect(self.selected)
        self.history.itemDoubleClicked.connect(lambda *_: app._safe(self.review))
        hist.addWidget(self.history)
        self.tabs.addTab(history, 'Runs')
        candidates = QPushButton('Review design candidates…')
        candidates.clicked.connect(lambda: app._safe(self.review_candidates))
        hist.addWidget(candidates)
        self.status = QLabel('Ready · position-target controller'); self.status.setWordWrap(True); layout.addWidget(self.status)
        self.progress = QProgressBar(); self.progress.setRange(0, 100); self.progress.setValue(0); layout.addWidget(self.progress)
        row = QHBoxLayout()
        run = QPushButton('Run experiment'); run.setObjectName('primaryAction'); run.clicked.connect(lambda: app._safe(self.run))
        row.addWidget(run)
        check = QPushButton('Check system'); check.setObjectName('checkSystem')
        check.setToolTip('Capture and compile the system, discover imported CAD components and open the controller contract. No simulation samples are taken.')
        check.clicked.connect(lambda: app._safe(self.check_system)); row.addWidget(check)
        self.cancel = QPushButton('Cancel run'); self.cancel.setEnabled(False); self.cancel.clicked.connect(lambda: app._safe(self.cancel_selected)); row.addWidget(self.cancel)
        layout.addLayout(row)
        row = QHBoxLayout()
        self.inspect = QPushButton('Inspect / compare'); self.inspect.clicked.connect(lambda: app._safe(self.review)); row.addWidget(self.inspect)
        self.baseline = QPushButton('Set baseline'); self.baseline.clicked.connect(self.set_baseline); row.addWidget(self.baseline)
        hist.addLayout(row)
        for key, title in [('system', 'System · Rhai'), ('controller', 'Controller · Rhai')]:
            load = QPushButton('Link Rhai file…')
            load.clicked.connect(lambda checked=False, key=key: app._safe(lambda: self.link_file(key)))
            self.editor_layouts[title].addWidget(load)
        restore = QPushButton('Restore run inputs'); restore.clicked.connect(lambda: app._safe(self.restore_inputs)); hist.addWidget(restore)
        self.auto = QCheckBox('Rerun after edits (750 ms debounce)'); self.auto.toggled.connect(self.edited)
        self.editor_layouts['Parameters'].addWidget(self.auto)
        self.controller_enabled = QCheckBox('Use sampled controller'); self.controller_enabled.setChecked(True)
        self.controller_enabled.setToolTip('Turn off for script-only systems without an external controller seam.')
        self.controller_enabled.toggled.connect(self.edited); self.editor_layouts['Controller · Rhai'].insertWidget(0, self.controller_enabled)
        self.controller_language = QComboBox()
        self.controller_language.addItem('Rhai controller source', 'rhai')
        self.controller_language.addItem('Captured process bundle · JSON', 'process')
        self.controller_mode = 'rhai'
        self.controller_texts = {'rhai': DEFAULT_CONTROLLER, 'process': json.dumps({
            'runtime': 'python', 'entry': 'controller.py', 'files': {'controller.py': {'path': 'controller.py'}}, 'arguments': []}, indent=2)}
        self.controller_language.currentIndexChanged.connect(self.change_controller_language); self.editor_layouts['Controller · Rhai'].insertWidget(1, self.controller_language)
        self.interface = QComboBox()
        self.interface.addItem('Position targets · rad · servo firmware', 'position_target')
        self.interface.addItem('Driver duty · −1 to 1 · external feedback', 'driver_duty')
        self.interface.setToolTip('Driver duty bypasses position firmware and exposes motor current, torque and speed. The Rhai controller must close feedback.')
        self.interface.currentIndexChanged.connect(self.edited); self.editor_layouts['Controller · Rhai'].insertWidget(2, self.interface)
        self.profile = QComboBox()
        self.profile.addItem('Quick check · rigid · contact/noise off', 'quick_check')
        self.profile.addItem('Validation · contact/flex/noise · finer step', 'validation')
        self.profile.setToolTip('Profile defaults can be overridden in Parameters → settings or in Rhai configure. Validation enables models; it does not certify their accuracy.')
        self.profile.currentIndexChanged.connect(self.edited); self.editor_layouts['Parameters'].insertWidget(0, self.profile)
        discovery = QWidget(); discovery_layout = QVBoxLayout(discovery)
        self.discover = QPushButton('Load registered Rust components'); self.discover.clicked.connect(self.load_catalogue)
        discovery_layout.addWidget(self.discover)
        self.component = QComboBox(); self.component.currentIndexChanged.connect(self.show_component); discovery_layout.addWidget(self.component)
        self.component.setSizeAdjustPolicy(QComboBox.AdjustToMinimumContentsLengthWithIcon)
        self.component.setMinimumContentsLength(24)
        self.component_details = QPlainTextEdit(); self.component_details.setReadOnly(True); discovery_layout.addWidget(self.component_details)
        self.tabs.addTab(discovery, 'Components')
        from .system_graph import SystemGraphPanel
        self.graph_panel = SystemGraphPanel(app)
        self.tabs.addTab(self.graph_panel, 'System graph')
        self.tabs.currentChanged.connect(lambda index: self.load_catalogue()
            if self.tabs.widget(index) is self.graph_panel and not self.graph_panel.catalogue and self.discover.isEnabled() else None)
        self.catalogue_ready.connect(self.receive_catalogue)
        self.diagnostics = QPlainTextEdit(); self.diagnostics.setReadOnly(True); self.diagnostics.setMaximumHeight(95)
        self.diagnostics.setPlaceholderText('Build errors and measured checks appear here. Completed runs remain available.')
        hist.addWidget(self.diagnostics)
        self.debounce = QTimer(self); self.debounce.setSingleShot(True); self.debounce.setInterval(750)
        self.debounce.timeout.connect(lambda: app._safe(lambda: self.run(automatic=True)))
        self.watcher = QTimer(self); self.watcher.setInterval(500); self.watcher.timeout.connect(self.poll_sources); self.watcher.start()
        app.doc.listeners.append(self.document_changed)
        self.refresh()

    def current_id(self):
        item = self.history.currentItem()
        return item.data(Qt.UserRole) if item else None

    def refresh(self, run_id=None):
        current = self.current_id()
        self.history.blockSignals(True); self.history.clear()
        for job in self.manager.list():
            evaluation = job.get('evaluation', {}).get('status', '')
            label = ('★ ' if job['id'] == self.baseline_id else '') + f"{job['label']} · {job['state']} {evaluation}\n{job['id'][:8]} · revision {job.get('revision')}"
            item = QListWidgetItem(label); item.setData(Qt.UserRole, job['id']); self.history.addItem(item)
            if job['id'] == current: self.history.setCurrentItem(item)
        self.history.blockSignals(False); self.selected()
        if self.check_run_id and self.loaded_check_id != self.check_run_id:
            check = self.manager.get(self.check_run_id)
            if check['state'] in TERMINAL:
                self.loaded_check_id = self.check_run_id
                try:
                    self.graph_panel.receive_imports(self.manager.components(self.check_run_id))
                    if self.request() != self.check_request: self.graph_panel.mark_imports_stale()
                except (KernelError, ValueError):
                    self.graph_panel.mark_imports_stale()
                    self.graph_panel.status.setText(check.get('error') or 'Check inputs changed; inspect the captured check in Runs.')

    def selected(self, *_):
        run_id = self.current_id()
        job = self.manager.get(run_id) if run_id else None
        completed = bool(job and job['state'] == 'completed' and not job.get('preflight'))
        self.inspect.setEnabled(completed); self.baseline.setEnabled(completed)
        self.cancel.setEnabled(bool(job and job['state'] not in TERMINAL))
        if not job: return
        self.status.setText(f"{job['label']} · {job['state']} · {job.get('stage', '')}")
        self.progress.setValue(round(100*job.get('fraction', 0)))
        if job.get('error'): self.diagnostics.setPlainText(job['error'])
        elif job.get('preflight') and job['state'] == 'completed':
            self.diagnostics.setPlainText('System compiled and controller contract opened. No simulation samples were taken; measured expectations have not been evaluated. Imported CAD components are available in System graph.')
        elif job.get('evaluation'):
            metrics = job['evaluation'].get('metrics', [])
            self.diagnostics.setPlainText('\n'.join(f"{'PASS' if m['passed'] else 'FAIL'} · {m['name']}: {m['value']:.5g} {m['unit']}" for m in metrics) or 'No measured expectations declared.')
        else: self.diagnostics.clear()

    def source_input(self, key, editor):
        if key not in self.linked:
            restored = getattr(self, 'restored_sources', {}).get(key)
            if restored:
                source = deepcopy(restored)
                source['files'][source['entry']] = editor.toPlainText()
                return source
            return editor.toPlainText()
        path = self.linked[key]['path']
        return {'entry': path.name, 'files': {str(p.relative_to(path.parent)): p.read_text() for p in sorted(path.parent.rglob('*.rhai'))}}

    def request(self, document=None):
        parameters = json.loads(self.parameters.toPlainText())
        if not isinstance(parameters, dict) or set(parameters) - {'system', 'controller', 'settings', 'seed'}:
            raise KernelError('Parameters accept system, controller and settings objects, and an integer seed')
        controller = None
        if self.controller_enabled.isChecked():
            language = self.controller_language.currentData()
            controller = {'language': language, 'parameters': parameters.get('controller', {}), 'interface': self.interface.currentData()}
            if language == 'rhai': controller['sources'] = self.source_input('controller', self.controller)
            else: controller['process'] = json.loads(self.controller.toPlainText())
        return {'expected_revision': (document or self.app.doc).revision,
            'system': self.source_input('system', self.system),
            'controller': controller,
            'parameters': parameters.get('system', {}), 'settings': parameters.get('settings', {}),
            'seed': parameters.get('seed', 0), 'profile': self.profile.currentData(),
            'parent_run': self.baseline_id}

    def change_controller_language(self):
        self.controller_texts[self.controller_mode] = self.controller.toPlainText()
        self.controller_mode = self.controller_language.currentData()
        self.linked.pop('controller', None)
        self.controller.setReadOnly(False)
        self.controller.setPlainText(self.controller_texts[self.controller_mode])
        self.tabs.setTabText(1, 'Controller · Rhai' if self.controller_mode == 'rhai' else 'Controller · process JSON')
        self.edited()

    def load_catalogue(self):
        self.discover.setEnabled(False)
        self.component_details.setPlainText('Loading component types, ports, units and parameter declarations…')
        def load():
            try: value = self.manager.catalogue()
            except Exception as error: value = {'error': str(error)}
            self.catalogue_ready.emit(value)
        threading.Thread(target=load, daemon=True).start()

    def receive_catalogue(self, value):
        self.discover.setEnabled(True)
        if isinstance(value, dict): self.component_details.setPlainText(value['error']); return
        self.component.clear()
        for item in value: self.component.addItem(f"{item['type']} · {item['name']}", item)
        self.graph_panel.receive_catalogue(value)

    def show_component(self):
        item = self.component.currentData()
        if not item: return
        lines = [item['name'], item['type'], '', 'Ports']
        for port in item['ports']:
            units = port.get('unit') or ', '.join(f"{lane['across']} [{lane['across_unit']}] / {lane['through']} [{lane['through_unit']}]" for lane in port.get('lanes', []))
            lines.append(f"{port['name']} · {port.get('direction', '')} · {units}")
        lines.extend(['', 'Parameters'])
        if not item.get('parameters_complete'): lines.append('Parameter discovery is incomplete for this native component. Consult its Rust definition.')
        for parameter in item.get('parameters') or []:
            default = 'required' if parameter['required'] else ('optional alternative' if parameter.get('default_label') is None and parameter['default'] is None else f"default {parameter.get('default_label') or parameter['default']}")
            lines.append(f"{parameter['name']} [{parameter['unit']}] · {default}")
        self.component_details.setPlainText('\n'.join(lines))

    def run(self, automatic=False):
        if automatic and self.auto_run_id:
            previous = self.manager.get(self.auto_run_id)
            if previous['state'] == 'queued': self.manager.cancel(self.auto_run_id)
        job = self.manager.create(self.request())
        if automatic: self.auto_run_id = job['id']
        self.refresh(); self.select_run(job['id']); self.tabs.setCurrentIndex(3)
        return job

    def check_system(self):
        self.check_request = self.request()
        job = self.manager.create({**self.check_request, 'preflight': True, 'label': 'System check'})
        self.check_run_id = job['id']
        self.refresh(); self.select_run(job['id'])
        self.status.setText('Checking the captured system… modelling remains available.')
        return job

    def select_run(self, run_id):
        for i in range(self.history.count()):
            if self.history.item(i).data(Qt.UserRole) == run_id:
                self.history.setCurrentRow(i); break

    def cancel_selected(self):
        if self.current_id(): self.manager.cancel(self.current_id()); self.refresh()

    def set_baseline(self):
        self.baseline_id = self.current_id(); self.refresh()

    def review(self, run_id=None):
        run_id = run_id or self.current_id()
        if not run_id: return None
        if self.manager.get(run_id).get('preflight'):
            self.graph_panel.receive_imports(self.manager.components(run_id))
            self.tabs.setCurrentWidget(self.graph_panel)
            return None
        dialog = RunReview(self, run_id, self.baseline_id)
        self.reviews.append(dialog)
        dialog.destroyed.connect(lambda *_: self.reviews.remove(dialog) if dialog in self.reviews else None)
        dialog.show(); return dialog

    def open_evidence(self, evidence):
        dialog = self.review(evidence['run_id'])
        if evidence.get('signal'):
            dialog.signal.setCurrentIndex(dialog.signal.findData(evidence['signal']))
        if evidence.get('time_range'): dialog.seek(evidence['time_range'][0])

    def review_candidates(self):
        dialog = CandidateReview(self)
        self.reviews.append(dialog)
        dialog.destroyed.connect(lambda *_: self.reviews.remove(dialog) if dialog in self.reviews else None)
        dialog.show()

    def restore_inputs(self):
        if not self.current_id(): return
        spec = self.manager.inputs(self.current_id())
        self.auto.setChecked(False)
        if 'component_graph' in spec:
            self.app.ops.set_component_graph(spec['component_graph'])
        self.controller_enabled.setChecked(spec.get('controller') is not None)
        self.controller_language.setCurrentIndex(max(0, self.controller_language.findData((spec.get('controller') or {}).get('language', 'rhai'))))
        self.interface.setCurrentIndex(max(0, self.interface.findData((spec.get('controller') or {}).get('interface', 'position_target'))))
        self.profile.setCurrentIndex(max(0, self.profile.findData(spec.get('profile', 'quick_check'))))
        for key, editor in [('system', self.system), ('controller', self.controller)]:
            source = spec['system'] if key == 'system' else (spec.get('controller') or {}).get('sources')
            if source:
                # Preserve imported modules when editing a restored entry.
                self.linked.pop(key, None); editor.setReadOnly(False)
                editor.setPlainText(source['files'][source['entry']])
                self.restored_sources = getattr(self, 'restored_sources', {})
                self.restored_sources[key] = source
        if (spec.get('controller') or {}).get('language') == 'process':
            self.controller.setPlainText(json.dumps(spec['controller']['process'], indent=2))
        self.parameters.setPlainText(json.dumps({'system': spec.get('parameters', {}),
            'controller': (spec.get('controller') or {}).get('parameters', {}), 'settings': spec['settings'], 'seed': spec.get('seed', 0)}, indent=2))
        self.tabs.setCurrentIndex(0)

    def link_file(self, key=None):
        key = key or ('controller' if self.tabs.currentIndex() == 1 else 'system')
        if key == 'controller' and self.controller_language.currentData() == 'process':
            raise KernelError('Set files in the process bundle JSON to {"path":"…"}; these files are captured on each run. Rhai file linking applies to Rhai editors.')
        path, _ = QFileDialog.getOpenFileName(self, f'Link {key} entry', '', 'Rhai (*.rhai)')
        if path:
            self.linked[key] = {'path': Path(path), 'text': None}
            self.poll_sources()
            self.status.setText(f'Linked {key}: {path}. Edit externally; imports are captured on each run.')

    def poll_sources(self):
        for key, linked in self.linked.items():
            try:
                source = self.source_input(key, None)
                fingerprint = json.dumps(source, sort_keys=True)
                if fingerprint != linked['text']:
                    linked['text'] = fingerprint
                    editor = self.system if key == 'system' else self.controller
                    editor.setReadOnly(True); editor.setPlainText(source['files'][source['entry']])
                    self.edited()
            except (OSError, ValueError) as error:
                self.status.setText(f'Linked source unavailable: {error}')

    def edited(self, *_):
        if hasattr(self, 'graph_panel'): self.graph_panel.mark_imports_stale()
        if hasattr(self, 'debounce'):
            if self.auto.isChecked(): self.debounce.start()
            else: self.debounce.stop()

    def document_changed(self, event, payload):
        if event == 'changed': self.edited()

    def shutdown(self):
        self.graph_panel.shutdown()
        self.debounce.stop(); self.watcher.stop()
        self.manager.changed.remove(self.run_changed.emit)
        self.app.doc.listeners.remove(self.document_changed)
        for dialog in list(self.reviews): dialog.close()
        self.manager.close()


class CandidateReview(QDialog):
    def __init__(self, panel):
        super().__init__(panel.app)
        self.panel = panel
        self.candidates = panel.app.candidates
        self.record = self.doc = self.viewport = None
        self.setWindowTitle('Review design candidates'); self.resize(1050, 760)
        self.setAttribute(Qt.WA_DeleteOnClose)
        layout = QVBoxLayout(self)
        self.list = QComboBox()
        for record in self.candidates.list():
            self.list.addItem(f"{record['label']} · {record['state']} · base revision {record['base_revision']}", record['id'])
        layout.addWidget(self.list)
        self.info = QLabel('No candidates yet. Agent edit batches appear here for review.'); self.info.setWordWrap(True); layout.addWidget(self.info)
        self.body = QVBoxLayout(); layout.addLayout(self.body, 1)
        self.changes = QPlainTextEdit(); self.changes.setReadOnly(True); self.changes.setMaximumHeight(170); layout.addWidget(self.changes)
        row = QHBoxLayout()
        self.run_button = QPushButton('Run candidate'); self.run_button.clicked.connect(lambda: panel.app._safe(self.run)); row.addWidget(self.run_button)
        self.accept_button = QPushButton('Accept into CAD'); self.accept_button.clicked.connect(lambda: panel.app._safe(self.accept_candidate)); row.addWidget(self.accept_button)
        self.discard_button = QPushButton('Discard candidate'); self.discard_button.clicked.connect(lambda: panel.app._safe(self.discard)); row.addWidget(self.discard_button)
        layout.addLayout(row)
        self.list.currentIndexChanged.connect(self.select_candidate); self.select_candidate()

    def select_candidate(self):
        candidate_id = self.list.currentData()
        for button in (self.run_button, self.accept_button, self.discard_button): button.setEnabled(bool(candidate_id))
        if not candidate_id: return
        self.record = self.candidates.get(candidate_id)
        self.doc = self.candidates.document(candidate_id)
        if self.viewport:
            self.viewport.doc.listeners.remove(self.viewport._on_doc_event)
            self.body.removeWidget(self.viewport); self.viewport.deleteLater()
        self.viewport = Viewport(self.doc, self); self.body.addWidget(self.viewport)
        self.viewport.tool_name = 'Candidate review'; self.viewport.tool_hint = 'Captured proposed design • right-drag to orbit'
        self.viewport.focus_all()
        record = self.record
        conflict = self.panel.app.doc.revision != record['base_revision']
        self.info.setText(f"{record['label']} · {record['state']} · base revision {record['base_revision']}\n" +
                         ('Live document has intervening edits. Rebuild this candidate before accepting.' if conflict else 'Matches the live base revision. Acceptance is one undoable edit.'))
        self.accept_button.setEnabled(record['state'] == 'draft' and not conflict)
        self.discard_button.setEnabled(record['state'] == 'draft')
        lines = []
        for change in record['changes']['nodes']:
            lines.append(f"{change['kind'].title()} · {change['name']} · {change['id']}" + (' · geometry changed' if change['geometry_changed'] else ''))
            before, after = change['before'] or {}, change['after'] or {}
            for key in sorted(before.keys() | after.keys()):
                if before.get(key) != after.get(key): lines.append(f"  {key}: {before.get(key)} → {after.get(key)}")
        for key, change in record['changes']['document'].items():
            lines.append(f"{key}: {change['before']} → {change['after']}")
        self.changes.setPlainText('\n'.join(lines) or 'No design changes')

    def run(self):
        request = self.panel.request(self.doc); request['candidate_id'] = self.record['id']
        job = self.panel.manager.create(request, document=self.doc)
        self.panel.refresh(); self.panel.select_run(job['id']); self.panel.tabs.setCurrentIndex(3)
        self.info.setText(f"Candidate queued as run {job['id'][:8]}. The live CAD document is unchanged.")

    def accept_candidate(self):
        self.candidates.accept(self.record['id'], self.panel.app.doc.revision)
        self.select_candidate()

    def discard(self):
        self.candidates.discard(self.record['id']); self.select_candidate()

    def closeEvent(self, event):
        if self.viewport: self.viewport.doc.listeners.remove(self.viewport._on_doc_event)
        super().closeEvent(event)
