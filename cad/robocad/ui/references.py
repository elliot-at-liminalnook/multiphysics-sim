"""Reference-image workspace: import, align, calibrate and sketch."""
import numpy as np
from PySide6.QtCore import Qt
from PySide6.QtGui import QIcon, QPixmap
from PySide6.QtWidgets import (QWidget, QVBoxLayout, QHBoxLayout, QPushButton, QListWidget,
    QListWidgetItem, QLabel, QFormLayout, QComboBox, QDoubleSpinBox, QCheckBox, QFileDialog)
from ..kernel import Plane
from .tools import ImageCalibrateTool, SketchTool


class ReferencesPanel(QWidget):
    def __init__(self, app):
        super().__init__(app)
        self.app = app
        self.loading = False
        self.setAcceptDrops(True)
        layout = QVBoxLayout(self)
        intro = QLabel('Drop images here or in the viewport. Align a view, calibrate its scale, then sketch over it.')
        intro.setWordWrap(True)
        layout.addWidget(intro)
        self.add = QPushButton('＋ Add reference images…')
        self.add.clicked.connect(self.browse)
        layout.addWidget(self.add)
        self.list = QListWidget()
        self.list.setMinimumHeight(100)
        self.list.setMaximumHeight(180)
        self.list.currentItemChanged.connect(self.load)
        self.list.itemChanged.connect(self.visibility)
        layout.addWidget(self.list)
        self.preview = QLabel()
        self.preview.setAlignment(Qt.AlignCenter)
        self.preview.setFixedHeight(100)
        layout.addWidget(self.preview)
        self.fields = QWidget()
        form = QFormLayout(self.fields)
        self.plane = QComboBox()
        self.plane.addItems(['Keep current plane', 'Front (XZ)', 'Side (YZ)', 'Top (XY)', 'Active construction plane'])
        form.addRow('Plane', self.plane)
        self.width = self.spin(.001, 1e7, ' mm')
        self.angle = self.spin(-360, 360, '°')
        self.opacity = self.spin(0, 100, '%', 0)
        form.addRow('Width', self.width)
        self.origin = [self.spin(-1e7,1e7,' mm') for _ in range(3)]
        for label,w in zip(('Origin X','Origin Y','Origin Z'), self.origin): form.addRow(label,w)
        form.addRow('Rotation', self.angle)
        form.addRow('Opacity', self.opacity)
        self.locked = QCheckBox('Lock reference against selection')
        form.addRow(self.locked)
        layout.addWidget(self.fields)
        self.apply = QPushButton('Apply placement')
        self.apply.clicked.connect(lambda: app._safe(self.commit))
        layout.addWidget(self.apply)
        row = QHBoxLayout()
        for label, fn in [('Align view', self.align), ('Calibrate scale', self.calibrate), ('Sketch over this', self.sketch)]:
            b = QPushButton(label)
            b.clicked.connect(lambda checked=False, f=fn: app._safe(f))
            row.addWidget(b)
        layout.addLayout(row)
        self.remove = QPushButton('Remove reference')
        self.remove.clicked.connect(lambda: app._safe(self.delete))
        layout.addWidget(self.remove)
        note = QLabel('Scale calibration assumes a flat drawing or a view square to the reference. Perspective photos can distort dimensions.')
        note.setWordWrap(True)
        layout.addWidget(note)
        layout.addStretch()
        self.refresh()

    @staticmethod
    def spin(lo,hi,suffix,decimals=2):
        w = QDoubleSpinBox()
        w.setRange(lo,hi); w.setDecimals(decimals); w.setSuffix(suffix)
        w.setKeyboardTracking(False)
        return w

    def current_id(self):
        item = self.list.currentItem()
        return item.data(Qt.UserRole) if item else None

    def refresh(self):
        nid = self.current_id()
        self.loading = True
        self.list.blockSignals(True)
        self.list.clear()
        for n in self.app.doc.nodes.values():
            if n.image is None: continue
            pix = QPixmap(); pix.loadFromData(n.image['data'])
            item = QListWidgetItem(QIcon(pix.scaled(64,64,Qt.KeepAspectRatio,Qt.SmoothTransformation)), n.name)
            item.setData(Qt.UserRole,n.id)
            item.setFlags(item.flags() | Qt.ItemIsUserCheckable)
            item.setCheckState(Qt.Checked if n.visible else Qt.Unchecked)
            self.list.addItem(item)
            if n.id == nid: self.list.setCurrentItem(item)
        if self.list.currentRow() < 0 and self.list.count(): self.list.setCurrentRow(0)
        self.list.blockSignals(False)
        self.loading = False
        self.load()

    def load(self, *_):
        nid = self.current_id()
        for w in (self.fields, self.apply, self.remove): w.setEnabled(nid is not None)
        if not nid:
            self.preview.clear()
            return
        n = self.app.doc.nodes[nid]; im = n.image
        pix = QPixmap(); pix.loadFromData(im['data'])
        self.preview.setPixmap(pix.scaled(320,100,Qt.KeepAspectRatio,Qt.SmoothTransformation))
        self.width.setValue(im['width']); self.angle.setValue(im.get('rotation_deg',0.))
        self.opacity.setValue(im.get('opacity',.6)*100)
        self.locked.setChecked(n.locked); self.plane.setCurrentIndex(0)
        for w,v in zip(self.origin,im['plane'].origin): w.setValue(v)

    def visibility(self,item):
        if not self.loading:
            self.app._safe(lambda: self.app.ops.update_reference(item.data(Qt.UserRole),visible=item.checkState()==Qt.Checked))

    def browse(self):
        paths,_ = QFileDialog.getOpenFileNames(self,'Reference images','','Images (*.png *.jpg *.jpeg *.webp *.bmp)')
        if paths: self.app._safe(lambda: self.add_paths(paths))

    def add_paths(self, paths):
        self.app.pose_panel.stop()
        ids = self.app.ops.import_references(paths, self.app.viewport.active_plane or Plane.xy())
        self.refresh()
        if ids:
            for i in range(self.list.count()):
                if self.list.item(i).data(Qt.UserRole)==ids[-1]: self.list.setCurrentRow(i)
            self.app.references_dock.show(); self.app.references_dock.raise_()
            self.align()
            self.app.status(f'{len(ids)} reference image(s) added • Calibrate scale before tracing')
        return ids

    def commit(self):
        nid = self.current_id()
        if not nid: return
        planes = [None,Plane.xz(),Plane.yz(),Plane.xy(),self.app.viewport.active_plane or Plane.xy()]
        self.app.ops.update_reference(nid,width=self.width.value(),opacity=self.opacity.value()/100,
            origin=[w.value() for w in self.origin],plane=planes[self.plane.currentIndex()],
            rotation_deg=self.angle.value(),locked=self.locked.isChecked())
        self.app.status('Reference placement updated • Ctrl+Z undoes')

    def align(self):
        nid = self.current_id()
        if not nid: return
        self.app.pose_panel.stop()
        im = self.app.doc.nodes[nid].image; p = im['plane']
        vp = self.app.viewport
        vp.active_plane = p
        vp.camera.mode = 'trackball'
        vp.camera.rot = np.array([p.x_axis,p.y_axis,p.normal])
        vp.camera.orthographic = True
        vp.camera.target = p.to_world(im['width']/2,im['height']/2)
        aspect = vp.width()/max(1,vp.height())
        import math
        vp.camera.distance = max(im['height'], im['width']/aspect)*.6/math.tan(math.radians(vp.camera.fov)/2)
        vp.update()

    def calibrate(self):
        if self.current_id():
            self.align()
            self.app.set_tool(ImageCalibrateTool(self.app.ctx,self.current_id()))

    def sketch(self):
        if self.current_id():
            self.align()
            self.app.set_tool(SketchTool(self.app.ctx,'line'))

    def delete(self):
        if self.current_id(): self.app.ops.delete([self.current_id()])

    def dragEnterEvent(self,e):
        if e.mimeData().hasUrls() and all(u.isLocalFile() for u in e.mimeData().urls()): e.acceptProposedAction()

    def dropEvent(self,e):
        paths = [u.toLocalFile() for u in e.mimeData().urls() if u.isLocalFile()]
        if paths:
            self.app._safe(lambda: self.add_paths(paths)); e.acceptProposedAction()
