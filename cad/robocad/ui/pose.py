"""Joint sliders and a bounded motion sweep in the CAD viewport."""
import math
import time
import numpy as np
from PySide6.QtCore import Qt, QTimer, QPointF
from PySide6.QtGui import QColor, QPen
from PySide6.QtWidgets import (QWidget,QVBoxLayout,QHBoxLayout,QLabel,QPushButton,QComboBox,
    QSlider,QDoubleSpinBox)
from ..pose import PoseModel, joint_range, joint_motion
from .tools import SelectTool


class PosePanel(QWidget):
    def __init__(self,app):
        super().__init__(app)
        self.app = app
        self.model = None
        self.positions = {}
        self.active = False
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
        self.play = QPushButton('Play sweep')
        self.play.clicked.connect(self.toggle_play)
        row.addWidget(self.play)
        self.period = QDoubleSpinBox()
        self.period.setRange(1,60); self.period.setValue(4); self.period.setSuffix(' s / cycle')
        row.addWidget(self.period); layout.addLayout(row)
        self.reset = QPushButton('Return to CAD pose')
        self.reset.clicked.connect(self.stop)
        layout.addWidget(self.reset)
        note = QLabel('Pose preview • no loads or contact forces. Fixed joints and mounted motors follow their parent. Unconnected parts stay stationary.')
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
        for jid in model.home: self.joint.addItem(self.app.doc.nodes[jid].name,jid)
        self.joint.blockSignals(False)
        self.active = True
        self.app.viewport.cancel_picks()
        self.app.viewport.hover = None
        self.app.properties.setEnabled(False)
        self.controls(True)
        self.load_joint()
        self.apply()
        self.app.pose_dock.show(); self.app.pose_dock.raise_()
        self.info.setText('Pose mode is active. Choose a joint, then drag its slider. Other joint positions are retained.')

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
        jid = self.joint.currentData()
        lo,hi = joint_range(self.model.joints[jid])
        self.positions[jid] = max(lo,min(hi,v/self.factor))
        self.sync_slider(); self.apply()

    def apply(self):
        self.app.viewport.set_pose(self.model.matrices(self.positions))
        self.app.viewport.tool_name = 'Pose preview'
        self.app.viewport.tool_hint = 'Drag a joint slider • right-drag to orbit • Esc returns to CAD pose'
        self.app.mode_label.setText('Pose preview · geometry unchanged')
        self.app.viewport.update()

    def toggle_play(self):
        if self.timer.isActive(): self.pause()
        elif self.active:
            self.started = time.monotonic()
            self.timer.start(); self.play.setText('Pause sweep')

    def tick(self):
        phase = (time.monotonic()-self.started)/self.period.value()
        fraction = (1-math.cos(2*math.pi*phase))/2
        self.value.setValue(self.value.minimum()+(self.value.maximum()-self.value.minimum())*fraction)

    def pause(self):
        self.timer.stop(); self.play.setText('Play sweep')

    def stop(self):
        if not self.active: return
        self.pause()
        self.active = False
        self.app.viewport.set_pose(None)
        self.app.properties.setEnabled(True)
        self.controls(False)
        self.app._tool_feedback()
        self.info.setText('Back in CAD pose. Preview did not change the model.')
        self.app.status('Returned to CAD pose')
