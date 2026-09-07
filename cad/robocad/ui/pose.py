"""Joint sliders and a bounded motion sweep in the CAD viewport."""
import math
import json
import time
import numpy as np
from PySide6.QtCore import Qt, QTimer, QPointF
from PySide6.QtGui import QColor, QPen
from PySide6.QtWidgets import (QWidget,QVBoxLayout,QHBoxLayout,QLabel,QPushButton,QComboBox,
    QSlider,QDoubleSpinBox,QPlainTextEdit,QCheckBox,QFileDialog)
from ..pose import PoseModel, joint_range, joint_motion
from ..motion import validate_program, sample_program, sweep_program
from .tools import SelectTool


class PosePanel(QWidget):
    def __init__(self,app):
        super().__init__(app)
        self.app = app
        self.model = None
        self.positions = {}
        self.active = False
        self.program = None
        self.playhead = 0.
        self.timer = QTimer(self)
        self.timer.setInterval(33)
        self.timer.timeout.connect(self.tick)
        layout = QVBoxLayout(self)
        self.info = QLabel('Preview the movement of connected parts. Geometry stays in its original CAD pose.')
        self.info.setWordWrap(True); layout.addWidget(self.info)
        self.start = QPushButton('Enter pose mode')
        self.start.clicked.connect(lambda: app._safe(self.enter))
        layout.addWidget(self.start)
        self.joint = QComboBox()
        self.joint.currentIndexChanged.connect(self.load_joint)
        layout.addWidget(self.joint)
        self.limits = QLabel('Add joints in the Robot panel to begin.')
        self.limits.setWordWrap(True); layout.addWidget(self.limits)
        self.value = QDoubleSpinBox()
        self.value.setDecimals(2); self.value.setKeyboardTracking(False)
        self.value.valueChanged.connect(self.value_changed)
        self.value.editingFinished.connect(self.pause)
        layout.addWidget(self.value)
        self.slider = QSlider(Qt.Horizontal)
        self.slider.setRange(0,1000)
        self.slider.valueChanged.connect(self.slide)
        layout.addWidget(self.slider)
        row = QHBoxLayout()
        self.play = QPushButton('Play pattern')
        self.play.clicked.connect(self.toggle_play)
        row.addWidget(self.play)
        self.period = QDoubleSpinBox()
        self.period.setRange(1,60); self.period.setValue(4); self.period.setSuffix(' s / cycle')
        row.addWidget(self.period); layout.addLayout(row)
        row = QHBoxLayout()
        focus = QPushButton('Focus mechanism')
        focus.clicked.connect(lambda: app._safe(self.focus_mechanism)); row.addWidget(focus)
        self.markers = QCheckBox('Show markers')
        self.markers.toggled.connect(self.update_markers); row.addWidget(self.markers)
        layout.addLayout(row)
        self.programs = QComboBox()
        self.programs.setToolTip('Named motion patterns saved with this CAD document')
        self.programs.currentIndexChanged.connect(self.choose_program)
        layout.addWidget(self.programs)
        self.editor = QPlainTextEdit()
        self.editor.setPlaceholderText('Motion pattern JSON: joint, unit, and [seconds, value] keys')
        self.editor.setMaximumHeight(170)
        self.edit_toggle = QPushButton('Edit pattern JSON')
        self.edit_toggle.setCheckable(True); self.edit_toggle.toggled.connect(self.editor.setVisible)
        layout.addWidget(self.edit_toggle)
        layout.addWidget(self.editor); self.editor.hide()
        row = QHBoxLayout()
        make = QPushButton('Make joint sweep')
        make.clicked.connect(lambda: app._safe(self.make_sweep)); row.addWidget(make)
        save = QPushButton('Save pattern')
        save.clicked.connect(lambda: app._safe(self.save_program)); row.addWidget(save)
        delete = QPushButton('Delete pattern')
        delete.clicked.connect(lambda: app._safe(self.delete_program)); row.addWidget(delete)
        layout.addLayout(row)
        self.timeline = QSlider(Qt.Horizontal); self.timeline.setRange(0,1000)
        self.timeline.setToolTip('Scrub the motion timeline')
        self.timeline.valueChanged.connect(self.scrub); layout.addWidget(self.timeline)
        self.clock = QLabel('0.00 s'); layout.addWidget(self.clock)
        self.readout = QPlainTextEdit('Driven joints and linkage closure appear during playback.')
        self.readout.setReadOnly(True); self.readout.setMaximumHeight(105); layout.addWidget(self.readout)
        self.refresh_programs()
        row = QHBoxLayout()
        self.video_size = QComboBox(); self.video_size.addItem('720p', (1280,720)); self.video_size.addItem('1080p', (1920,1080)); row.addWidget(self.video_size)
        self.video_fps = QComboBox()
        for fps in (24,30,60): self.video_fps.addItem(f'{fps} fps',fps)
        row.addWidget(self.video_fps);layout.addLayout(row)
        row = QHBoxLayout()
        self.export_video = QPushButton('Export MP4…');self.export_video.clicked.connect(lambda: app._safe(self.export_mp4));row.addWidget(self.export_video)
        self.cancel_video = QPushButton('Cancel export');self.cancel_video.setEnabled(False);row.addWidget(self.cancel_video)
        layout.addLayout(row)
        self.video_progress = QLabel('Exports one cycle from the current camera.');self.video_progress.setWordWrap(True);layout.addWidget(self.video_progress)
        from .motion_video import MotionVideo
        self.video = MotionVideo(self);self.cancel_video.clicked.connect(lambda: self.video.cancel())
        self.reset = QPushButton('Return to CAD pose')
        self.reset.clicked.connect(self.stop)
        layout.addWidget(self.reset)
        note = QLabel('Kinematic preview • ideal gears and belts; closed knee links follow automatically. No loads, collision checks, or inferred travel stops. Values are relative to the imported pose when joint home is zero.')
        note.setWordWrap(True); layout.addWidget(note)
        layout.addStretch()
        self.controls(False)
        app.viewport.overlays.append(self.draw_range)

    def controls(self,enabled):
        for w in (self.joint,self.value,self.slider,self.play,self.period,self.reset): w.setEnabled(enabled)
        self.start.setEnabled(not enabled)

    def enter(self):
        model = PoseModel(self.app.doc)
        if not model.home:
            self.app.error('Add a hinge or sliding joint in the Robot panel first')
            return
        self.app.set_tool(SelectTool(self.app.ctx))
        self.model = model
        self.positions = dict(model.home)
        self.joint.blockSignals(True); self.joint.clear()
        for jid in model.drivers: self.joint.addItem(self.app.doc.nodes[jid].name,jid)
        self.joint.blockSignals(False)
        self.active = True
        self.previous_pins = self.app.viewport.show_comment_pins
        self.update_markers()
        self.app.viewport.cancel_picks()
        self.app.viewport.hover = None
        self.app.properties.setEnabled(False)
        self.controls(True)
        self.load_joint()
        self.apply()
        if not self.editor.toPlainText(): self.make_sweep()
        self.app.pose_dock.show(); self.app.pose_dock.raise_()
        self.info.setText('Choose a motor/joint or a saved pattern. Play, pause, or scrub to inspect connected movement.')

    def load_joint(self,*_):
        self.pause()
        jid = self.joint.currentData()
        if not self.active or jid is None: return
        j = self.model.joints[jid]
        self.factor = 1. if j.type=='prismatic' else 180/math.pi
        lo,hi = joint_range(j)
        self.value.blockSignals(True)
        self.value.setRange(lo*self.factor,hi*self.factor)
        self.value.setSuffix(' mm' if j.type=='prismatic' else '°')
        self.value.setValue(self.positions[jid]*self.factor)
        self.value.blockSignals(False)
        units = 'mm' if j.type=='prismatic' else '°'
        fallback = ' • preview bounds; joint limits unset' if j.lower is None or j.upper is None else ''
        self.limits.setText(f'{lo*self.factor:.1f} to {hi*self.factor:.1f} {units}{fallback}')
        self.sync_slider()
        self.app.viewport.update()

    def draw_range(self, painter):
        if not self.active: return
        jid = self.joint.currentData()
        if jid is None: return
        j = self.model.joints[jid]
        vp = self.app.viewport
        axis = np.asarray(j.axis,dtype=float); axis /= np.linalg.norm(axis)
        lo,hi = joint_range(j)
        pivot = np.asarray(j.pivot)
        if j.type == 'prismatic':
            origin = pivot
        else:
            helper = np.array([0,0,1] if abs(axis[2])<.9 else [1,0,0])
            radial = np.cross(axis,helper); radial /= np.linalg.norm(radial)
            origin = pivot + radial*vp.camera.world_per_pixel(vp.height())*65
        def projected(value):
            matrix = joint_motion(j,value)
            p = matrix[:3,:3]@origin + matrix[:3,3]
            return vp.camera.project(vp.pose_point(j.parent,p),vp.width(),vp.height())
        painter.setPen(QPen(QColor('#7bd7f5'),2))
        samples = [projected(v) for v in np.linspace(lo,hi,65)]
        for a,b in zip(samples,samples[1:]):
            if a and b: painter.drawLine(QPointF(*a[:2]),QPointF(*b[:2]))
        current = projected(self.positions[jid])
        if current:
            painter.setBrush(QColor('#f7c777'))
            painter.drawEllipse(QPointF(*current[:2]),6,6)
            painter.drawText(QPointF(current[0]+10,current[1]-10),f'{self.positions[jid]*self.factor:.1f}{self.value.suffix()}')

    def sync_slider(self):
        lo,hi = self.value.minimum(),self.value.maximum()
        self.slider.blockSignals(True)
        self.slider.setValue(round(1000*(self.value.value()-lo)/(hi-lo)) if hi>lo else 0)
        self.slider.blockSignals(False)

    def slide(self,v):
        self.pause()
        self.value.setValue(self.value.minimum()+(self.value.maximum()-self.value.minimum())*v/1000)

    def value_changed(self,v):
        if not self.active: return
        self.pause()
        jid = self.joint.currentData()
        lo,hi = joint_range(self.model.joints[jid])
        self.positions[jid] = max(lo,min(hi,v/self.factor))
        self.sync_slider()
        try: self.apply()
        except Exception as error:
            self.positions = dict(self.model.last_positions)
            self.value.blockSignals(True); self.value.setValue(self.positions[jid]*self.factor); self.value.blockSignals(False)
            self.sync_slider(); self.pause(); self.info.setText(str(error))

    def apply(self):
        matrices = self.model.matrices(self.positions)
        self.positions = dict(self.model.last_positions)
        self.app.viewport.set_pose(matrices)
        changed = [i for i,v in self.positions.items() if abs(v-self.model.home[i]) > 1e-6]
        lines = []
        for i in changed[:10]:
            j = self.model.joints[i]; value = self.positions[i]
            lines.append(f'{self.model.names[i]}: {value if j.type=="prismatic" else math.degrees(value):.2f} {"mm" if j.type=="prismatic" else "°"}')
        self.readout.setPlainText('\n'.join(lines) + f'\nClosure residual: {self.model.last_error_mm:.5f} mm')
        self.app.viewport.tool_name = 'Pose preview'
        self.app.viewport.tool_hint = 'Drag a joint slider • right-drag to orbit • Esc returns to CAD pose'
        self.app.mode_label.setText('Pose preview · geometry unchanged')
        self.app.viewport.update()

    def export_mp4(self):
        from pathlib import Path
        self.prepare();self.pause()
        folder=Path(self.app.doc.path).parent if self.app.doc.path else Path.home()
        name=''.join(c if c.isalnum() or c in ' -_' else '_' for c in self.program['name'])
        path,_=QFileDialog.getSaveFileName(self,'Export motion video',str(folder/(name+'.mp4')),'MP4 video (*.mp4)')
        if path:
            width,height=self.video_size.currentData()
            self.video.start(path,self.video_fps.currentData(),width,height)

    def update_markers(self,*_):
        if self.active:
            self.app.viewport.show_comment_pins = self.markers.isChecked()
            self.app.viewport.pose_show_connectors = self.markers.isChecked()
            self.app.viewport.update()

    def focus_mechanism(self):
        if not self.active: self.enter()
        jid = self.joint.currentData()
        ids = [self.model.joints[jid].child]
        for driven,(driver,ratio) in self.model.transmissions.items():
            if driver == jid: ids.append(self.model.joints[driven].child)
        for lid,loop in self.model.loops.items():
            path = set(self.model._ancestors(loop.parent)) ^ set(self.model._ancestors(loop.child))
            if jid in path: ids.extend([loop.parent,loop.child])
        # Include rigidly attached geometry (e.g. the full sliding foot rod),
        # not just the small crosshead at the loop-closing pin.
        expanded = set(ids)
        while True:
            children = {child for child,(parent,joint) in self.model.parents.items()
                        if parent in expanded and (joint is None or self.model.joints[joint].type == 'fixed')}
            if children <= expanded: break
            expanded.update(children)
        self.app.viewport.focus_nodes(list(expanded))

    def refresh_programs(self, selected=None):
        self.programs.blockSignals(True); self.programs.clear()
        self.programs.addItem('Unsaved pattern', None)
        for name in self.app.doc.robot_settings.get('motion_programs', {}): self.programs.addItem(name,name)
        if selected: self.programs.setCurrentIndex(max(0,self.programs.findData(selected)))
        self.programs.blockSignals(False)

    def choose_program(self,*_):
        self.pause()
        p = self.app.doc.robot_settings.get('motion_programs',{}).get(self.programs.currentData())
        if p: self.editor.setPlainText(json.dumps(p,indent=2)); self.playhead=0.

    def make_sweep(self):
        if not self.active: self.enter(); return
        self.pause()
        p = sweep_program(self.model,self.joint.currentData(),self.period.value())
        self.editor.setPlainText(json.dumps(p,indent=2)); self.playhead=0.
        self.edit_toggle.setChecked(True)

    def save_program(self):
        p = self.app.ops.save_motion(json.loads(self.editor.toPlainText()))
        self.refresh_programs(p['name'])
        self.app.status('Saved motion pattern: '+p['name'])

    def delete_program(self):
        name = self.programs.currentData()
        if name:
            self.app.ops.delete_motion(name); self.refresh_programs()

    def prepare(self, program=None):
        p = validate_program(self.app.doc, program if program is not None else json.loads(self.editor.toPlainText()))
        if not self.active: self.enter()
        self.program = p
        self.edit_toggle.setChecked(False)
        self.refresh_programs(p['name'])
        self.joint.setCurrentIndex(self.joint.findData(p['tracks'][0]['joint']))
        self.editor.setPlainText(json.dumps(p,indent=2))
        return p

    def seek(self, seconds, program=None):
        if program is not None or self.program is None or not self.active: self.prepare(program)
        if not math.isfinite(seconds): raise ValueError('Time must be finite')
        previous = dict(self.positions)
        self.positions = {**self.model.home, **sample_program(self.program,seconds)}
        try: self.apply()
        except Exception:
            self.positions = previous
            self.pause()
            raise
        self.playhead = max(0.,min(self.program['duration'],seconds))
        self.timeline.blockSignals(True); self.timeline.setValue(round(1000*self.playhead/self.program['duration'])); self.timeline.blockSignals(False)
        self.clock.setText(f'{self.playhead:.2f} / {self.program["duration"]:.2f} s')
        jid=self.joint.currentData()
        self.value.blockSignals(True); self.value.setValue(self.positions[jid]*self.factor); self.value.blockSignals(False); self.sync_slider()
        return self.state()

    def scrub(self, value):
        self.pause()
        try:
            self.prepare(); self.seek(self.program['duration']*value/1000)
        except Exception as error: self.info.setText(str(error))

    def toggle_play(self):
        if self.timer.isActive(): self.pause(); return
        try:
            self.prepare()
            if self.playhead >= self.program['duration']: self.playhead=0.
            self.started = time.monotonic()-self.playhead
            self.timer.start(); self.play.setText('Pause')
        except Exception as error: self.info.setText(str(error))

    def tick(self):
        elapsed = time.monotonic()-self.started
        duration = self.program['duration']
        if self.program.get('loop'): elapsed %= duration
        try: self.seek(min(elapsed,duration))
        except Exception as error:
            self.pause(); self.info.setText(str(error)); return
        if elapsed >= duration: self.pause()

    def pause(self):
        self.timer.stop(); self.play.setText('Play pattern')

    def state(self):
        return {'active':self.active,'playing':self.timer.isActive(),'time':self.playhead,
                'program':self.program['name'] if self.program else None,
                'positions':dict(self.positions),'closure_error_mm':self.model.last_error_mm if self.model else None}

    def stop(self):
        if self.video.running: self.video.cancel(restore=False)
        if not self.active: return
        self.pause()
        self.active = False
        self.app.viewport.set_pose(None)
        self.app.viewport.show_comment_pins = self.previous_pins
        self.app.properties.setEnabled(True)
        self.controls(False)
        self.app._tool_feedback()
        self.info.setText('Back in CAD pose. Preview did not change the model.')
        self.app.status('Returned to CAD pose')
