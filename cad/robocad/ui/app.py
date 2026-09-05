"""The application: main window, viewport, panels, tools, command
registry with a JSON keymap, palette, radial menus, multiple windows
with copy/paste-with-placement, import/export dialogs, the live bridge
and the simulator link."""

from __future__ import annotations

import json
import math
import os
import sys
import time
from typing import Callable, Optional

from PySide6.QtCore import QPoint, QRect, QSettings, Qt, QTimer, Signal
from PySide6.QtGui import QAction, QColor, QKeySequence, QPainter, QPen
from PySide6.QtWidgets import QApplication, QDockWidget, QFileDialog, QInputDialog, QLabel, QMainWindow, QMenu, QMessageBox, QRubberBand, QToolBar, QVBoxLayout, QWidget, QScrollArea, QFrame

from ..analysis import face_distance, measure_angle_edges, measure_angle_faces, measure_points, measure_radius
from ..commands import CommandStack, Ops
from ..document import Document, Measurement, Transform
from ..kernel import BooleanOp, KernelError, Plane, SurfaceKind
from ..kernel.base import v_add, v_dist, v_scale, v_sub, v_unit
from ..printing import FastenerSpec, validate_for_export, wall_thickness
from ..units import format_angle, format_length
from .strings import tr
from .tools import EdgeTool, ExtrudeTool, FastenerTool, ImageCalibrateTool, JointTool, MeasureTool, MotorTool, NumericField, PlaneTool, PrimitiveTool, PushPullTool, SectionTool, SelectTool, ShellTool, SketchTool, Tool, ToolContext, TransformTool
from .viewport import Viewport
from .comments import CommentsPanel, AnnotateTool
from .saved_views import SavedViewsPanel
from .references import ReferencesPanel
from .pose import PosePanel
from .experiments import ExperimentsPanel
from ..experiments import Experiments
from ..candidates import Candidates
from .widgets import ArrayDialog, CableDialog, CommandPalette, ExportDialog, FastenerDialog, JointDialog, MaterialsPanel, MotorDialog, PowerDialog, RobotPanel, SensorDialog, NumericBar, Outliner, PropertiesPanel, RadialMenu, UnitsDialog, disambiguation_menu

DARK_QSS = """
QMainWindow, QDockWidget, QWidget { background: #202226; color: #e4e6ea; font-size: 12px; }
QLineEdit, QComboBox, QSpinBox, QDoubleSpinBox, QListWidget, QTreeWidget { background: #2a2d33; border: 1px solid #3a3e46; padding: 3px; }
QPushButton { background: #33373f; border: 1px solid #454a54; padding: 4px 10px; } QPushButton:hover { background: #3f4550; }
QToolBar { background: #26292e; border: none; spacing: 2px; } QMenuBar { background: #26292e; } QMenu { background: #2a2d33; }
QDockWidget::title { background: #2a2d33; padding: 7px; } QStatusBar { background: #26292e; }
QPlainTextEdit { background: #171d25; border: 1px solid #526477; border-radius: 5px; padding: 7px; }
QPushButton#primaryAction { background: #216887; border: 1px solid #67b9dc; }
QPushButton:disabled { color: #777f89; }
QToolButton { padding: 6px; border-radius: 4px; }
QToolButton:checked { background: #245570; border: 1px solid #79cbed; }
QListWidget::item { padding: 7px; border-bottom: 1px solid #343c47; }
QListWidget::item:selected { background: #294f66; color: #e4e6ea; }
"""
HIGH_CONTRAST_QSS = """
QMainWindow, QDockWidget, QWidget { background: #ffffff; color: #000000; font-size: 13px; }
QLineEdit, QComboBox, QSpinBox, QDoubleSpinBox, QListWidget, QTreeWidget { background: #ffffff; border: 2px solid #000000; padding: 3px; }
QPushButton { background: #f0f0f0; border: 2px solid #000000; padding: 4px 10px; }
QToolBar, QMenuBar, QMenu, QStatusBar { background: #f4f4f4; color: #000; }
"""

WINDOWS: list["MainWindow"] = []


class MainWindow(QMainWindow):
    def __init__(self, doc: Optional[Document] = None, path: Optional[str] = None):
        super().__init__()
        self.doc = doc or (Document.load(path) if path else Document())
        self.ops = Ops(self.doc)
        self.experiments = Experiments(self.doc)
        self.candidates = Candidates(self.doc, self.ops, self.experiments.root/'candidates')
        self.settings = QSettings("robocad", "robocad")
        self.export_settings: dict[str, dict] = json.loads(self.settings.value("export_settings", "{}") or "{}")
        self.viewport = Viewport(self.doc, self)
        self.setCentralWidget(self.viewport)
        self.setWindowTitle(self._title())
        self.resize(1400, 900)
        self.ctx = ToolContext(self)
        self.tool: Tool = SelectTool(self.ctx)
        self.commands: dict[str, dict] = {}
        self.rubber = QRubberBand(QRubberBand.Rectangle, self.viewport)
        self.last_fastener = {"size": "M3", "kind": "clearance", "extra": 0.0, "depth": 0.0}
        self.bridge = None
        self.sim_link = None
        self.api = None
        self._build_panels()
        self._build_commands()
        self._build_menus()
        self._geometry_refresh = False
        self._refresh_timer = QTimer(self)
        self._refresh_timer.setSingleShot(True)
        self._refresh_timer.timeout.connect(self._refresh_panels)
        self.load_keymap()
        self.viewport.dragged.connect(self._on_drag)
        self.viewport.context_requested.connect(self._context_menu)
        self.ops.stack.listeners.append(self._on_stack)
        self.doc.listeners.append(self._on_doc)
        self._autosave_timer = QTimer(self)
        self._autosave_timer.timeout.connect(self._autosave)
        from concurrent.futures import ThreadPoolExecutor
        self._autosave_executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix='robocad-archive')
        self._autosave_pending = None
        self._autosave_last_revision = None
        self._autosave_poll = QTimer(self)
        self._autosave_poll.setInterval(25)
        self._autosave_poll.timeout.connect(self._finish_autosave)
        self._start_autosave(float(self.settings.value("autosave_interval", 120)))
        self.high_contrast = bool(int(self.settings.value("high_contrast", 0)))
        self.apply_theme()
        self.tool.activate()
        self._tool_feedback()
        self.viewport.focus_all()
        self._spacemouse = None
        QTimer.singleShot(500, self._init_spacemouse)
        self.status(tr("status.ready"))
        WINDOWS.append(self)
        self.start_api()

    # ---- building ----------------------------------------------------------
    def _title(self):
        return f"{tr('app.title')} — {os.path.basename(self.doc.path) if self.doc.path else 'untitled'}{' *' if self.doc.dirty else ''}"

    def _start_autosave(self, interval):
        # Capture immutable state on Qt's thread; compress/write in a worker.
        self.doc.stop_autosave()
        self.doc.autosave_interval = float(interval)
        self._autosave_timer.start(max(1, round(float(interval)*1000)))

    def _autosave(self):
        if not self.doc.dirty or self._autosave_pending is not None or self._autosave_last_revision == (id(self.doc), self.doc.revision):
            return
        from ..document import write_archive
        try:
            revision, entries = self.doc.archive_snapshot()
            path = self.doc.autosave_path()
            future = self._autosave_executor.submit(write_archive, path, entries)
            self._autosave_pending = (future, self.doc, revision, path)
            self._autosave_poll.start()
        except Exception as error:
            self.status(f'Autosave failed: {error}')

    def _finish_autosave(self):
        pending = self._autosave_pending
        if pending is None or not pending[0].done():
            return
        self._autosave_pending = None
        self._autosave_poll.stop()
        future, doc, revision, path = pending
        try:
            future.result()
            self._autosave_last_revision = (id(doc), revision)
            if self.doc is doc:
                doc.notify('autosaved', path)
        except Exception as error:
            self.status(f'Autosave failed: {error}')

    def _build_panels(self):
        def scroll_panel(widget):
            scroll = QScrollArea()
            scroll.setWidgetResizable(True); scroll.setFrameShape(QFrame.NoFrame)
            scroll.setWidget(widget)
            return scroll
        self.numeric = NumericBar(self)
        self.numeric.committed.connect(self._numeric_commit)
        self.numeric.cancelled.connect(self._numeric_cancel)
        bar = QToolBar("Numeric")
        bar.setMovable(False)
        bar.addWidget(self.numeric)
        self.addToolBar(Qt.BottomToolBarArea, bar)
        self.readout_label = QLabel("")
        self.statusBar().addPermanentWidget(self.readout_label)
        self.outliner = Outliner(self)
        d1 = QDockWidget(tr("outliner.title"), self)
        d1.setWidget(self.outliner)
        self.addDockWidget(Qt.LeftDockWidgetArea, d1)
        self.properties = PropertiesPanel(self)
        d2 = QDockWidget(tr("properties.title"), self)
        d2.setWidget(scroll_panel(self.properties))
        self.addDockWidget(Qt.RightDockWidgetArea, d2)
        self.materials = MaterialsPanel(self)
        d3 = QDockWidget(tr("materials.title"), self)
        d3.setWidget(scroll_panel(self.materials))
        self.addDockWidget(Qt.RightDockWidgetArea, d3)
        self.robot_panel = RobotPanel(self)
        d4 = QDockWidget("Robot", self)
        d4.setWidget(scroll_panel(self.robot_panel))
        self.addDockWidget(Qt.RightDockWidgetArea, d4)
        self.tabifyDockWidget(d3, d4)
        d3.raise_()
        self.comments = CommentsPanel(self)
        self.comments_dock = QDockWidget("Comments", self)
        self.comments_dock.setWidget(scroll_panel(self.comments))
        self.addDockWidget(Qt.RightDockWidgetArea, self.comments_dock)
        self.tabifyDockWidget(d2, self.comments_dock)
        self.saved_views_panel = SavedViewsPanel(self)
        self.saved_views_dock = QDockWidget('Saved Views', self)
        self.saved_views_dock.setWidget(self.saved_views_panel)
        self.addDockWidget(Qt.RightDockWidgetArea, self.saved_views_dock)
        self.tabifyDockWidget(d2, self.saved_views_dock)
        d2.raise_()
        self.viewport.comment_hit = self.comments.pin_at
        self.viewport.comment_clicked.connect(self.comments.select)
        self.pose_panel = PosePanel(self)
        self.pose_dock = QDockWidget('Pose', self)
        self.pose_dock.setWidget(scroll_panel(self.pose_panel))
        self.addDockWidget(Qt.RightDockWidgetArea, self.pose_dock)
        self.tabifyDockWidget(d3, self.pose_dock)
        self.experiments_panel = ExperimentsPanel(self)
        self.experiments_dock = QDockWidget('Experiments', self)
        self.experiments_dock.setWidget(scroll_panel(self.experiments_panel))
        self.addDockWidget(Qt.RightDockWidgetArea, self.experiments_dock)
        self.tabifyDockWidget(d2, self.experiments_dock)
        self.tabifyDockWidget(d2, d3)
        self.tabifyDockWidget(d2, d4)
        self.tabifyDockWidget(d2, self.pose_dock)
        d2.raise_()
        self.references = ReferencesPanel(self)
        self.references_dock = QDockWidget('References', self)
        self.references_dock.setWidget(scroll_panel(self.references))
        self.addDockWidget(Qt.LeftDockWidgetArea, self.references_dock)
        self.tabifyDockWidget(d1, self.references_dock)
        d1.raise_()
        d2.raise_()
        self.resizeDocks([d1, d2], [220, 460], Qt.Horizontal)
        self.comments.refresh()
        self.outliner.refresh()
        if hasattr(self, 'robot_panel'):
            self.robot_panel.refresh()
        self.materials.refresh()
        self.properties.refresh()
        self.viewport.setAcceptDrops(True)
        self.viewport.dragEnterEvent = lambda e: e.acceptProposedAction() if e.mimeData().hasText() or e.mimeData().hasUrls() else None
        self.viewport.dropEvent = self._drop_material

    def _cmd(self, cid: str, label: str, run: Callable, category: str = "General", keys: Optional[list[str]] = None):
        act = QAction(label, self)
        act.triggered.connect(lambda *_: self._safe(run, label))
        self.addAction(act)
        self.commands[cid] = {"label": label, "run": lambda: self._safe(run, label), "category": category, "keys": keys or [], "action": act}

    def _safe(self, fn, label=None):
        if label:
            self.status(f"{label}…")
            self.statusBar().repaint()
        QApplication.setOverrideCursor(Qt.WaitCursor)
        try:
            return fn()
        except KernelError as e:
            self.error(str(e))
        except Exception as e:  # keep the app alive; report
            self.error(f"{type(e).__name__}: {e}")
        finally:
            QApplication.restoreOverrideCursor()

    def _build_commands(self):
        c = self._cmd
        c('view.references', 'References', lambda: (self.references_dock.show(), self.references_dock.raise_()), 'View')
        c('reference.import', 'Add reference images…', self.references.browse, 'File')
        c('view.pose', 'Pose', lambda: (self.pose_dock.show(), self.pose_dock.raise_()), 'View')
        c('view.experiments', 'Experiments', lambda: (self.experiments_dock.show(), self.experiments_dock.raise_()), 'View')
        c('simulation.experiment', 'Run captured experiment', self.experiments_panel.run, 'Simulation', ['Ctrl+Return'])
        c('robot.pose', 'Preview joint motion', self.pose_panel.enter, 'Robot')
        c("tool.annotate", "Annotate", lambda: self.set_tool(AnnotateTool(self.ctx)), "Inspect", ["N"])
        c("view.comments", "Comments panel", lambda: (self.comments_dock.show(), self.comments_dock.raise_()), "View")
        c('view.saved_views', 'Saved Views', lambda: (self.saved_views_dock.show(), self.saved_views_dock.raise_()), 'View')
        c("view.comment_pins", "Toggle comment pins", self.toggle_comment_pins, "View")
        c("command_palette", "Command palette", self.open_palette, "General")
        c("file.new", tr("file.new"), lambda: MainWindow().show(), "File")
        c("file.open", tr("file.open"), self.open_file, "File")
        c("file.save", tr("file.save"), self.save, "File")
        c("file.save_as", tr("file.save_as"), self.save_as, "File")
        c("file.import", tr("file.import"), self.import_file, "File")
        c("file.export", tr("file.export"), self.export_file, "File")
        c("file.export_drawing", tr("file.export_drawing"), self.export_drawing, "File")
        c("file.quit", tr("file.quit"), self.close, "File")
        c("edit.undo", tr("edit.undo"), lambda: self.status(f"Undo {self.ops.undo() or ''}"), "Edit")
        c("edit.redo", tr("edit.redo"), lambda: self.status(f"Redo {self.ops.redo() or ''}"), "Edit")
        c("edit.delete", tr("edit.delete"), self.delete_selection, "Edit")
        c("edit.copy", tr("edit.copy"), self.copy_with_placement, "Edit")
        c("edit.paste", tr("edit.paste"), self.paste_with_placement, "Edit")
        c("edit.select_all", tr("edit.select_all"), self.select_all, "Edit")
        c("edit.invert", tr("edit.invert"), self.invert_selection, "Edit")
        c("edit.select_same_material", tr("edit.select_same_material"), self.select_same_material, "Edit")
        c("edit.convert_faces", "Selection: edges → bounding faces", self.convert_edges_to_faces, "Edit")
        c("edit.preferences", tr("edit.preferences"), self.preferences, "Edit")
        c("view.fit", tr("view.fit"), self.viewport.focus_all, "View")
        c("view.focus", tr("view.focus"), self.viewport.focus_selection, "View")
        for name in ("front", "back", "top", "bottom", "right", "left", "iso"):
            c(f"view.{name}", f"View {name}", lambda n=name: (self.viewport.camera.set_view(n), self.viewport.update()), "View")
        c("view.ortho", tr("view.ortho"), self.toggle_ortho, "View")
        c("view.grid", tr("view.grid"), self.toggle_grid, "View")
        c("view.mode_next", "Next display mode", self.next_display_mode, "View")
        for m in Viewport.MODES:
            c(f"view.mode.{m}", f"Display: {m.replace('_', ' ')}", lambda mm=m: self.set_display_mode(mm), "View")
        c("view.orbit_mode", "Toggle orbit: turntable / trackball", self.toggle_orbit_mode, "View")
        c("view.fov", "Set field of view…", self.set_fov, "View")
        c("view.isolate", tr("view.isolate"), lambda: self.ops.isolate(self.viewport.selection.nodes()), "View")
        c("view.show_all", tr("view.show_all"), self.ops.show_all, "View")
        c("view.hide", tr("view.hide"), lambda: self.ops.set_visible(self.viewport.selection.nodes(), False), "View")
        c("view.section", tr("view.section"), self.toggle_section, "Inspect")
        c("view.build_plate", tr("view.build_plate"), self.toggle_build_plate, "Print")
        c("view.high_contrast", tr("view.high_contrast"), self.toggle_high_contrast, "View")
        c("view.radial", "View radial menu", self.view_radial, "View")
        for mode in ("body", "face", "edge", "vertex", "point"):
            c(f"select.{mode}", f"Select {mode}s", lambda mm=mode: self.set_selection_mode(mm), "Select")
        c("select.mode_radial", "Selection-mode radial menu", self.selection_radial, "Select")
        c("tool.select", "Select tool", lambda: self.set_tool(SelectTool(self.ctx)), "Tools")
        c("tool.move", "Move", lambda: self.set_tool(TransformTool(self.ctx, "move")), "Tools")
        c("tool.rotate", "Rotate", lambda: self.set_tool(TransformTool(self.ctx, "rotate")), "Tools")
        c("tool.scale", "Scale", lambda: self.set_tool(TransformTool(self.ctx, "scale")), "Tools")
        c("tool.push_pull", "Push/Pull face", lambda: self.set_tool(PushPullTool(self.ctx)), "Modify")
        c("tool.offset_face", "Offset face", lambda: self.set_tool(PushPullTool(self.ctx, offset=True)), "Modify")
        c("tool.box", "Box (corner)", lambda: self.set_tool(PrimitiveTool(self.ctx, "box")), "Create")
        c("tool.box_center", "Box (centre)", lambda: self.set_tool(PrimitiveTool(self.ctx, "box", True)), "Create")
        c("tool.cylinder", "Cylinder", lambda: self.set_tool(PrimitiveTool(self.ctx, "cylinder")), "Create")
        c("tool.sphere", "Sphere", lambda: self.set_tool(PrimitiveTool(self.ctx, "sphere")), "Create")
        c("tool.extrude", "Extrude", lambda: self.set_tool(ExtrudeTool(self.ctx)), "Create")
        c("tool.revolve", "Revolve", lambda: self.set_tool(ExtrudeTool(self.ctx, revolve=True)), "Create")
        c("tool.sweep", "Sweep (profile + path from selection)", self.sweep_selection, "Create")
        c("tool.pipe", "Pipe along selected curve…", self.pipe_selection, "Create")
        c("tool.loft", "Loft selected sketches", self.loft_selection, "Create")
        c("tool.fill", "Fill / patch selected curve", self.fill_selection, "Create")
        c("tool.fillet", "Fillet", lambda: self.set_tool(EdgeTool(self.ctx, "fillet")), "Modify")
        c("tool.fillet_variable", "Variable fillet", lambda: self.set_tool(EdgeTool(self.ctx, "variable")), "Modify")
        c("tool.fillet_chordal", "Chordal fillet", lambda: self.set_tool(EdgeTool(self.ctx, "chordal")), "Modify")
        c("tool.fillet_all", "Fillet all edges…", self.fillet_all, "Modify")
        c("tool.full_round", "Full round (two edges)", self.full_round, "Modify")
        c("tool.remove_fillets", "Remove fillets (selected faces)", self.remove_fillets, "Modify")
        c("tool.chamfer", "Chamfer", lambda: self.set_tool(EdgeTool(self.ctx, "chamfer")), "Modify")
        c("tool.shell", "Hollow / shell", lambda: self.set_tool(ShellTool(self.ctx)), "Modify")
        c("tool.thicken", "Thicken sheet…", self.thicken, "Modify")
        c("tool.draft", "Draft faces…", self.draft_faces, "Modify")
        c("tool.delete_face", "Delete faces (heal)", self.delete_faces, "Modify")
        c("tool.measure", "Measure", lambda: self.set_tool(MeasureTool(self.ctx)), "Inspect")
        c("tool.plane", "Plane from face", lambda: self.set_tool(PlaneTool(self.ctx, "face")), "Planes")
        c("tool.plane_three", "Plane from three points", lambda: self.set_tool(PlaneTool(self.ctx, "three")), "Planes")
        c("tool.plane_camera", "Plane from two points (camera)", lambda: self.set_tool(PlaneTool(self.ctx, "camera")), "Planes")
        c("tool.plane_mid", "Midplane between two faces", lambda: self.set_tool(PlaneTool(self.ctx, "mid")), "Planes")
        c("tool.plane_xy", "Active plane: XY", lambda: self.set_active_plane(None, Plane.xy()), "Planes")
        c("tool.plane_xz", "Active plane: XZ", lambda: self.set_active_plane(None, Plane.xz()), "Planes")
        c("tool.plane_yz", "Active plane: YZ", lambda: self.set_active_plane(None, Plane.yz()), "Planes")
        c("tool.plane_2d_snap", "Toggle 2D snapping to the active plane", self.toggle_plane_snapping, "Planes")
        c("tool.fastener", "Fastener hole…", self.fastener, "Print")
        c("tool.clearance", "Clearance offset…", self.clearance, "Print")
        c("tool.mirror", "Mirror (about active plane)", lambda: self.mirror(False), "Modify")
        c("tool.mirror_live", "Mirror as live instance", lambda: self.mirror(True), "Modify")
        c("tool.instance", "Instance selected", self.instance_selection, "Modify")
        c("tool.array", "Array…", self.array, "Modify")
        c("tool.cut_plane", "Cut with active plane", self.cut_with_plane, "Modify")
        c("tool.cut_sheet", "Cut with selected sheet/curve", self.cut_with_selection, "Modify")
        c("tool.split_face", "Split faces with active plane", self.split_face, "Modify")
        c("tool.imprint", "Imprint selected curve/body", self.imprint, "Modify")
        c("tool.project_curve", "Project curve onto body", self.project_curve, "Modify")
        c("tool.silhouette", "Silhouette onto active plane", self.silhouette, "Modify")
        c("tool.control_points", "Show/edit control points (advanced)", self.control_points, "Advanced")
        c("tool.raise_degree", "Raise face degree", self.raise_degree, "Advanced")
        c("tool.rebuild_face", "Rebuild face…", self.rebuild_face, "Advanced")
        c("tool.dependent_offset", "Dependent offset (face to body)…", self.dependent_offset, "Modify")
        c("tool.set_pivot", "Set pivot at cursor snap", self.set_pivot, "Tools")
        for shape, label in (("line", "Line"), ("rectangle", "Rectangle"), ("rectangle_center", "Rectangle (centre)"), ("circle", "Circle"), ("circle_2pt", "Circle (two points)"), ("circle_3pt", "Circle (three points)"), ("arc_3pt", "Arc (three points)"), ("polygon", "Polygon"), ("slot", "Slot"), ("spline", "Spline"), ("ellipse", "Ellipse"), ("spiral", "Spiral"), ("text", "Text")):
            c(f"sketch.{shape}", f"Sketch: {label}", lambda s=shape: self.start_sketch(s), "Sketch")
        c("sketch.offset", "Sketch: offset selected curve…", self.sketch_offset, "Sketch")
        c("sketch.fillet", "Sketch: fillet corner…", self.sketch_fillet, "Sketch")
        c("sketch.join", "Sketch: join curves", self.sketch_join, "Sketch")
        c("modify.union", "Union", lambda: self.boolean(BooleanOp.UNION), "Modify")
        c("modify.subtract", "Subtract", lambda: self.boolean(BooleanOp.SUBTRACT), "Modify")
        c("modify.intersect", "Intersect", lambda: self.boolean(BooleanOp.INTERSECT), "Modify")
        c("modify.region", "Region (overlap as new body)", self.region, "Modify")
        c("modify.join", "Join", lambda: self.ops.join(self.viewport.selection.nodes()), "Modify")
        c("modify.unjoin", "Unjoin", lambda: [self.ops.unjoin(i) for i in self.viewport.selection.nodes()], "Modify")
        c("modify.dissolve", "Dissolve redundant topology", lambda: [self.ops.dissolve(i) for i in self.viewport.selection.nodes()], "Modify")
        c("modify.make_unique", "Make instance unique", lambda: [self.ops.make_unique(i) for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind == "instance"], "Modify")
        c("print.wall_check", "Wall thickness check…", self.wall_check, "Print")
        c("print.validate", "Validate for printing", self.validate, "Print")
        c("print.overhangs", "Toggle overhang shading", self.toggle_overhangs, "Print")
        c("inspect.curvature", "Curvature comb on selected curve", self.curvature_comb, "Inspect")
        c("inspect.continuity", "Continuity check (G0/G1/G2)", self.continuity, "Inspect")
        c("inspect.draft", "Draft-angle shading", self.draft_shading, "Inspect")
        c("inspect.normals", "Normal-direction shading", self.normal_shading, "Inspect")
        c("bridge.start", "Live link: start (Blender)", self.start_bridge, "Bridge")
        c("bridge.stop", "Live link: stop", self.stop_bridge, "Bridge")
        c("bridge.share", "Web share: publish viewer…", self.web_share, "Bridge")
        c("robot.add_motor", "Robot: add motor from library…", self.robot_add_motor, "Robot", ["Ctrl+Shift+M"])
        c("robot.add_joint", "Robot: add joint (click parent, child, axis face)", self.robot_add_joint, "Robot", ["Ctrl+Shift+J"])
        c("robot.joint_dialog", "Robot: joint from the two selected bodies…", self.robot_joint_dialog, "Robot")
        c("robot.infer", "Robot: infer joints from coaxial holes and pins", self.robot_infer, "Robot")
        c("robot.assign_motor", "Robot: assign selected motor to a joint…", self.robot_assign_motor, "Robot")
        c("robot.fixed", "Robot: fix selected bodies together (first is the parent)", self.robot_fixed, "Robot")
        c("robot.ground", "Robot: toggle ground on selected bodies", self.robot_ground, "Robot")
        c("robot.validate", "Robot: validate", self.robot_validate, "Robot")
        c("robot.motors", "Robot: motor library…", self.robot_motor_library, "Robot")
        c("robot.add_sensor", "Robot: add sensor (IMU, encoder, current, force)…", self.robot_add_sensor, "Robot")
        c("robot.add_cable", "Robot: add cable between bodies…", self.robot_add_cable, "Robot")
        c("robot.power", "Robot: battery, control loop and uncertainty…", self.robot_power, "Robot")
        c("robot.load_results", "Robot: load simulation results…", self.robot_load_results, "Robot")
        c("robot.apply_identification", "Robot: apply identified joint parameters…", self.robot_apply_identification, "Robot")
        c("view.stress", "Toggle stress overlay (from loaded results)", self.toggle_stress, "Inspect")
        c("sim.export_physical", "Simulation: export physical model (v3, with flexible links)…", self.sim_export_physical, "Simulation")
        c("sim.export", "Simulation: export robot model…", self.sim_export, "Simulation")
        c("sim.link", "Simulation: live link (watch + run viewer)", self.sim_link_toggle, "Simulation")
        c("api.address", "REST API: show address", self.show_api, "Bridge")
        c("group.set_active", "Set selected group as active", lambda: self.ops.set_active_group(next((i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind == "group"), None)), "Outliner")
        c("group.group", "Group selection", lambda: self.ops.group(self.viewport.selection.nodes()), "Outliner")
        c("numeric.entry", "Numeric entry (Tab)", self.numeric.focus_first, "General")
        c("help.guide", "User guide", self.show_guide, "Help")

    def _build_menus(self):
        mb = self.menuBar()
        menus = {}
        for cat in ("File", "Edit", "View", "Select", "Create", "Sketch", "Modify", "Planes", "Inspect", "Print", "Advanced", "Outliner", "Robot", "Bridge", "Simulation", "Help"):
            menus[cat] = mb.addMenu(f"&{cat}")
        for cid, c in self.commands.items():
            menus.get(c["category"], menus["Help"]).addAction(c["action"])
        tb = QToolBar("Tools")
        self.addToolBar(tb)
        for cid in ("tool.select", "tool.annotate", "view.saved_views", "view.references", "view.pose", "view.experiments", "tool.move", "tool.rotate", "tool.scale", "tool.box", "tool.cylinder", "tool.sphere", "sketch.rectangle", "sketch.circle", "sketch.slot", "tool.extrude", "tool.push_pull", "tool.fillet", "tool.shell", "modify.union", "modify.subtract", "tool.fastener", "tool.measure", "view.section", "print.validate"):
            action = self.commands[cid]["action"]
            action.setToolTip(self.commands[cid]["label"])
            if cid.startswith("tool."):
                action.setCheckable(True)
            tb.addAction(action)
        self.mode_label = QLabel()
        self.mode_label.setStyleSheet("padding: 6px 12px; font-weight: bold; color: #74c8ef")
        self.statusBar().addPermanentWidget(self.mode_label)

    # ---- keymap ----------------------------------------------------------
    def load_keymap(self):
        base = os.path.join(os.path.dirname(__file__), "keymap.json")
        user = os.path.join(os.path.expanduser("~"), ".robocad", "keymap.json")
        keymap = {}
        for p in (base, user):
            if os.path.exists(p):
                with open(p) as f:
                    keymap.update({k: v for k, v in json.load(f).items() if not k.startswith("_")})
        for cid, keys in keymap.items():
            if cid not in self.commands:
                continue
            keys = keys if isinstance(keys, list) else [keys]
            self.commands[cid]["keys"] = keys
            self.commands[cid]["action"].setShortcuts([QKeySequence(k) for k in keys])
            self.commands[cid]["action"].setShortcutContext(Qt.WindowShortcut)
        # Tab is consumed by focus by default: route it ourselves.
        self.commands["numeric.entry"]["action"].setShortcut(QKeySequence())
        QApplication.instance().focusChanged.connect(self._focus_changed)

    def _focus_changed(self, old, current):
        from PySide6.QtWidgets import QLineEdit, QTextEdit, QPlainTextEdit, QAbstractSpinBox
        typing = isinstance(current, (QLineEdit, QTextEdit, QPlainTextEdit, QAbstractSpinBox))
        for cid, item in self.commands.items():
            if cid.startswith(("tool.", "sketch.", "select.", "view.")) or cid in ("edit.delete", "edit.copy", "edit.paste", "edit.select_all", "edit.undo", "edit.redo"):
                item["action"].setEnabled(not typing)

    def keyPressEvent(self, e):
        if e.key() == Qt.Key_Escape and self.comments._inspection_context is not None:
            self.comments.end_inspection()
            return
        if e.key() == Qt.Key_Tab and not self.numeric.hasFocus():
            if self.numeric.isVisible():
                self.numeric.focus_first()
                return
        if e.key() == Qt.Key_Escape:
            if self.pose_panel.active:
                self.pose_panel.stop()
                return
            self.tool.cancel()
            if not isinstance(self.tool, SelectTool):
                self.set_tool(SelectTool(self.ctx))
            else:
                self.viewport.selection.clear()
                self.selection_changed(None)
            return
        if self.tool.key(e.key(), e.modifiers()):
            return
        super().keyPressEvent(e)

    # ---- tools ------------------------------------------------------------
    def set_tool(self, tool: Tool):
        if hasattr(self, 'pose_panel'):
            self.pose_panel.stop()
        self.viewport.cancel_picks()
        self.tool.deactivate()
        self.tool = tool
        tool.activate()
        self._tool_feedback()
        self.viewport.update()

    def _tool_feedback(self):
        if hasattr(self, 'pose_panel') and self.pose_panel.active:
            self.viewport.tool_name = 'Pose preview'
            self.viewport.tool_hint = 'Drag a joint slider • right-drag to orbit • Esc returns to CAD pose'
            self.mode_label.setText('Pose preview · geometry unchanged')
            return
        name = self.tool.name
        cursor = Qt.ArrowCursor if isinstance(self.tool, SelectTool) else Qt.SizeAllCursor if isinstance(self.tool, TransformTool) else Qt.CrossCursor
        self.viewport.tool_cursor = cursor
        self.viewport.setCursor(cursor)
        self.viewport.tool_name = name.replace("_", " ").title()
        self.viewport.tool_hint = self.tool.hint or "Click or drag to edit • Tab for dimensions • Esc cancels"
        if hasattr(self, "mode_label"):
            self.mode_label.setText(f"{self.viewport.tool_name}  ·  {self.viewport.selection_mode.title()}")
        for cid, item in self.commands.items():
            if item["action"].isCheckable():
                item["action"].setChecked(cid == "tool." + name)

    def toggle_comment_pins(self):
        self.viewport.show_comment_pins = not self.viewport.show_comment_pins
        self.viewport.update()

    def _on_drag(self, payload):
        if self.pose_panel.active:
            return
        phase, _, handle, mods, pos = payload
        p = pos.toPoint() if hasattr(pos, "toPoint") else pos
        t = self.tool
        if phase == "press":
            self.viewport._tool_dragging = True
            t.press(p, mods)
        elif phase == "drag":
            t.drag(p, mods)
        elif phase == "release":
            self.viewport._tool_dragging = False
            t.release(p, mods)
        elif phase == "hover":
            t.hover(p, mods)
            self._snap_marker(p, mods)
        elif phase == "double":
            t.double(p, mods)
        elif phase in ("start", "move", "end"):
            t.gizmo(phase, handle, p, mods)

    def _snap_marker(self, p, mods):
        if isinstance(self.tool, (SketchTool, MeasureTool, PrimitiveTool)):
            s = self.viewport.snap(p.x(), p.y(), suppress=bool(mods & Qt.AltModifier))
            self.readout(f"{s.kind}  ({s.point[0]:.2f}, {s.point[1]:.2f}, {s.point[2]:.2f})")

    def numeric_fields(self, fields, on_commit):
        self._numeric_cb = on_commit
        self.numeric.set_fields(fields)

    def _numeric_commit(self, values):
        cb = getattr(self, "_numeric_cb", None)
        if cb:
            self._safe(lambda: cb(values))
        self.viewport.setFocus()

    def _numeric_cancel(self):
        self.viewport.setFocus()
        self.tool.cancel()

    def set_rubber_band(self, a, b):
        if a is None:
            self.rubber.hide()
            return
        self.rubber.setGeometry(QRect(a, b).normalized())
        self.rubber.show()

    def disambiguate(self, candidates, pos, on_pick):
        disambiguation_menu(self, candidates, self.doc, self.viewport.mapToGlobal(pos), on_pick)

    # ---- selection / dimensions -------------------------------------------------
    def selection_changed(self, world, from_outliner=False):
        self.properties.refresh()
        self.comments.refresh()
        if not from_outliner:
            self.outliner.sync_selection()
        self.viewport.update()
        n = len(self.viewport.selection.items)
        self.status(f"{n} selected" if n else tr("status.ready"))

    def set_selection_mode(self, mode: str):
        self.viewport.selection_mode = mode
        self.viewport.selection.clear()
        self.status(f"Selection mode: {mode}")
        self._tool_feedback()
        self.viewport.update()

    def select_all(self):
        sel = self.viewport.selection
        sel.clear()
        for n in self.doc.walk():
            if n.kind in ("body", "sheet", "curve", "instance", "mesh") and self.doc.is_visible(n.id):
                sel.items.append((n.id, "body", 0))
        self.selection_changed(None)

    def invert_selection(self):
        sel = self.viewport.selection
        current = set(sel.nodes())
        sel.clear()
        for n in self.doc.walk():
            if n.kind in ("body", "sheet", "curve", "instance", "mesh") and n.id not in current and self.doc.is_visible(n.id):
                sel.items.append((n.id, "body", 0))
        self.selection_changed(None)

    def select_same_material(self):
        ids = self.viewport.selection.nodes()
        if not ids:
            return
        same = self.doc.same_material(ids[0])
        self.viewport.selection.clear()
        for i in same:
            self.viewport.selection.items.append((i, "body", 0))
        self.selection_changed(None)

    def convert_edges_to_faces(self):
        sel = self.viewport.selection
        new = []
        for nid, ei in sel.edges():
            body = self.doc.resolved_body(nid)
            k = self.doc.kernel
            edges = k.edges(body)
            if ei < len(edges):
                for f in k.faces_of_edge(body, edges[ei]):
                    item = (nid, "face", f.index)
                    if item not in new:
                        new.append(item)
        sel.items = new
        self.viewport.selection_mode = "face"
        self.selection_changed(None)

    def body_under_selection(self) -> Optional[str]:
        ids = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind == "body"]
        if ids:
            return ids[0]
        bodies = self.doc.bodies(visible_only=True)
        return bodies[0].id if len(bodies) == 1 else None

    def live_dimensions(self) -> list[tuple[str, float, bool, Callable[[float], None]]]:
        """Editable dimensions for the current selection (faces / edges)."""
        out = []
        sel = self.viewport.selection
        k = self.doc.kernel
        faces = sel.faces()
        refs = []
        for nid, fi in faces:
            body = self.doc.resolved_body(nid)
            fl = k.faces(body)
            if fi < len(fl):
                refs.append((nid, fl[fi]))
        for nid, f in refs:
            if f.kind == SurfaceKind.CYLINDER and f.radius:
                out.append((f"Ø {self.doc.nodes[nid].name}", 2 * f.radius, False, lambda v, n=nid, ff=f: self.ops.set_diameter(n, ff, v)))
            elif f.kind in (SurfaceKind.SPHERE, SurfaceKind.TORUS) and f.radius:
                out.append((f"R {self.doc.nodes[nid].name}", f.radius, False, lambda v, n=nid, ff=f: self.status("Sphere/torus radius editing: use Scale about the centre")))
        if len(refs) == 2 and refs[0][0] == refs[1][0]:
            nid = refs[0][0]
            a, b = refs[0][1], refs[1][1]
            if a.kind == SurfaceKind.PLANE and b.kind == SurfaceKind.PLANE:
                if abs(abs(sum(x * y for x, y in zip(a.normal, b.normal))) - 1.0) < 1e-3:
                    out.append(("Distance", face_distance(a, b), False, lambda v, n=nid, fa=a, fb=b: self.ops.set_distance(n, fa, fb, v)))
                else:
                    out.append(("Angle", measure_angle_faces(a, b).value, True, lambda v, n=nid, fa=a, fb=b: self.ops.set_angle(n, fa, fb, v)))
        for nid, ei in sel.edges():
            body = self.doc.resolved_body(nid)
            edges = k.edges(body)
            if ei < len(edges) and edges[ei].radius:
                e = edges[ei]
                faces_of = k.faces_of_edge(body, e)
                cyl = next((f for f in faces_of if f.kind == SurfaceKind.CYLINDER), None)
                if cyl:
                    out.append((f"Ø edge {ei}", 2 * e.radius, False, lambda v, n=nid, ff=cyl: self.ops.set_diameter(n, ff, v)))
        # Whole-body selection is a navigation action. Enumerating every face
        # here freezes large imports and cannot reliably infer box dimensions
        # for arbitrary compounds. Select the relevant face(s) to edit them.
        return out

    def edit_dimension_at(self, nid, kind, idx, world):
        """Double-click on a face: put its dimension in the numeric bar."""
        if kind != "face":
            return
        body = self.doc.resolved_body(nid)
        k = self.doc.kernel
        faces = k.faces(body)
        if idx >= len(faces):
            return
        f = faces[idx]
        self.viewport.selection.clear()
        self.viewport.selection.items.append((nid, "face", idx))
        if f.kind == SurfaceKind.CYLINDER and f.radius:
            self.numeric_fields([NumericField("diameter", 2 * f.radius)], lambda v: self.ops.set_diameter(nid, f, v[0]))
            self.numeric.focus_first()
        elif f.kind == SurfaceKind.PLANE:
            opposite = next((g for g in faces if g.kind == SurfaceKind.PLANE and abs(sum(a * b for a, b in zip(g.normal, f.normal)) + 1.0) < 1e-3), None)
            if opposite:
                self.numeric_fields([NumericField("distance", face_distance(f, opposite))], lambda v: self.ops.set_distance(nid, opposite, f, v[0]))
                self.numeric.focus_first()
        self.selection_changed(world)

    def measure_between(self, a, b, keep: bool):
        k = self.doc.kernel
        m = None
        (ha, pa), (hb, pb) = a, b
        if ha and hb and ha[0] == "face" and hb[0] == "face":
            fa = k.faces(self.doc.resolved_body(ha[1]))[ha[2]]
            fb = k.faces(self.doc.resolved_body(hb[1]))[hb[2]]
            if abs(abs(sum(x * y for x, y in zip(fa.normal, fb.normal))) - 1.0) < 1e-3 and fa.kind == fb.kind == SurfaceKind.PLANE:
                m = Measurement("distance", [fa.centroid, fb.centroid], face_distance(fa, fb), f"{face_distance(fa, fb):.3f} mm")
            else:
                m = measure_angle_faces(fa, fb)
        elif ha and hb and ha[0] == "edge" and hb[0] == "edge":
            ea = k.edges(self.doc.resolved_body(ha[1]))[ha[2]]
            eb = k.edges(self.doc.resolved_body(hb[1]))[hb[2]]
            m = measure_angle_edges(ea, eb) if ea.kind.value == "line" and eb.kind.value == "line" else measure_points(ea.midpoint, eb.midpoint)
        elif ha and ha[0] == "edge" and ha == hb:
            e = k.edges(self.doc.resolved_body(ha[1]))[ha[2]]
            m = measure_radius(e)
        if m is None:
            m = measure_points(pa, pb)
        QApplication.clipboard().setText(f"{m.value:.4f}")
        self.status(f"{m.label}  — {tr('measure.copied')}")
        self.viewport.annotations = [a for a in self.viewport.annotations if not a[1].startswith("~")] + [(v_scale(v_add(m.points[0], m.points[-1]), 0.5), ("~" if not keep else "") + m.label)]
        if keep:
            self.ops.add_measurement(m)
        self.viewport.update()

    # ---- commands: creation / modify -------------------------------------------
    def start_sketch(self, shape):
        t = SketchTool(self.ctx, shape)
        if shape == "text":
            text, ok = QInputDialog.getText(self, "Text", "Text to sketch:")
            if not ok:
                return
            t.text = text
        self.set_tool(t)

    def _selected_sketch(self) -> Optional[str]:
        for nid in self.viewport.selection.nodes():
            if self.doc.nodes[nid].kind == "sketch":
                return nid
        return next((n.id for n in self.doc.nodes.values() if n.kind == "sketch" and self.doc.is_visible(n.id)), None)

    def sketch_offset(self):
        sid = self._selected_sketch()
        if not sid:
            return self.error("Select a sketch")
        v, ok = QInputDialog.getDouble(self, "Offset", "Distance (mm):", 1.0, -1000, 1000, 3)
        if ok:
            self.ops.edit_sketch(sid, lambda sk: [sk.offset(c, v) for c in list(sk.curves)], "Offset curves")

    def sketch_fillet(self):
        sid = self._selected_sketch()
        if not sid:
            return self.error("Select a sketch")
        v, ok = QInputDialog.getDouble(self, "Corner fillet", "Radius (mm):", 2.0, 0.01, 1000, 3)
        if ok:
            def fn(sk):
                for c in sk.curves:
                    if c.kind == "polyline" and c.closed:
                        for i in range(len(c.points) - 1, -1, -1):
                            try:
                                sk.fillet_corner(c, i, v)
                            except KernelError:
                                pass
            self.ops.edit_sketch(sid, fn, "Fillet corners")

    def sketch_join(self):
        sid = self._selected_sketch()
        if sid:
            self.ops.edit_sketch(sid, lambda sk: sk.join(list(sk.curves)) if len(sk.curves) > 1 else None, "Join curves")

    def boolean(self, op: BooleanOp):
        ids = self.viewport.selection.nodes()
        if len(ids) < 2:
            return self.error("Select the target body first, then the tools")
        self.ops.boolean(ids[0], ids[1:], op)
        self.viewport.selection.clear()
        self.selection_changed(None)

    def region(self):
        ids = self.viewport.selection.nodes()
        if len(ids) != 2:
            return self.error("Select exactly two bodies")
        self.ops.region(ids[0], ids[1])

    def sweep_selection(self):
        ids = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind in ("sketch", "curve")]
        if len(ids) < 2:
            return self.error("Select the profile sketch, then the path sketch")
        from ..kernel import SweepOptions

        twist, ok = QInputDialog.getDouble(self, "Sweep", "Twist (degrees):", 0.0, -3600, 3600, 1)
        if ok:
            self.ops.sweep(ids[0], ids[1], SweepOptions(twist_deg=twist))

    def pipe_selection(self):
        ids = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind in ("sketch", "curve")]
        if not ids:
            return self.error("Select a curve or sketch")
        d, ok = QInputDialog.getDouble(self, "Pipe", "Diameter (mm):", 4.0, 0.01, 1000, 3)
        if ok:
            for i in ids:
                self.ops.pipe(i, d)

    def loft_selection(self):
        ids = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind in ("sketch", "curve")]
        if len(ids) < 2:
            return self.error("Select two or more sketches to loft")
        self.ops.loft(ids)

    def fill_selection(self):
        ids = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind in ("sketch", "curve")]
        if not ids:
            return self.error("Select a closed curve")
        self.ops.fill(ids[0])

    def fillet_all(self):
        ids = self.viewport.selection.nodes()
        r, ok = QInputDialog.getDouble(self, "Fillet all edges", "Radius (mm):", 1.0, 0.01, 100, 3)
        if ok:
            for i in ids:
                self.ops.fillet_all(i, r)

    def full_round(self):
        edges = self.viewport.selection.edges()
        if len(edges) != 2 or edges[0][0] != edges[1][0]:
            return self.error("Select two edges of the same body")
        nid = edges[0][0]
        k = self.doc.kernel
        el = k.edges(self.doc.resolved_body(nid))
        self.ops.full_round(nid, el[edges[0][1]], el[edges[1][1]])

    def remove_fillets(self):
        faces = self.viewport.selection.faces()
        by = {}
        for nid, fi in faces:
            by.setdefault(nid, []).append(fi)
        for nid, idxs in by.items():
            fl = self.doc.kernel.faces(self.doc.resolved_body(nid))
            self.ops.remove_fillets(nid, [fl[i] for i in idxs])

    def thicken(self):
        ids = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind == "sheet"]
        if not ids:
            return self.error("Select a sheet")
        t, ok = QInputDialog.getDouble(self, "Thicken", "Thickness (mm):", 2.0, 0.01, 100, 3)
        if ok:
            for i in ids:
                self.ops.thicken(i, t)

    def draft_faces(self):
        faces = self.viewport.selection.faces()
        if not faces:
            return self.error("Select faces to draft")
        ang, ok = QInputDialog.getDouble(self, "Draft", "Angle (degrees):", 2.0, -45, 45, 2)
        if not ok:
            return
        by = {}
        for nid, fi in faces:
            by.setdefault(nid, []).append(fi)
        for nid, idxs in by.items():
            fl = self.doc.kernel.faces(self.doc.resolved_body(nid))
            self.ops.draft(nid, [fl[i] for i in idxs], (0, 0, 1), ang, self.viewport.active_plane or Plane.xy())

    def delete_faces(self):
        by = {}
        for nid, fi in self.viewport.selection.faces():
            by.setdefault(nid, []).append(fi)
        for nid, idxs in by.items():
            fl = self.doc.kernel.faces(self.doc.resolved_body(nid))
            self.ops.delete_faces(nid, [fl[i] for i in idxs])
        self.viewport.selection.clear()

    def fastener(self):
        d = FastenerDialog(self, self.last_fastener)
        if d.exec():
            spec = d.spec()
            self.last_fastener = {"size": spec.size, "kind": spec.kind, "extra": spec.extra_clearance, "depth": spec.depth or 0.0}
            self.set_tool(FastenerTool(self.ctx, spec))

    def clearance(self):
        faces = self.viewport.selection.faces()
        if not faces:
            return self.error("Select holes, bosses or faces to offset")
        v, ok = QInputDialog.getDouble(self, "Clearance", "Grow holes / shrink bosses by (mm):", self.ops.last_clearance, -5, 5, 2)
        if not ok:
            return
        by = {}
        for nid, fi in faces:
            by.setdefault(nid, []).append(fi)
        for nid, idxs in by.items():
            fl = self.doc.kernel.faces(self.doc.resolved_body(nid))
            self.ops.clearance(nid, [fl[i] for i in idxs], v)

    def mirror(self, live: bool):
        ids = self.viewport.selection.nodes()
        if not ids:
            return self.error("Select bodies to mirror")
        self.ops.mirror(ids, self.viewport.active_plane or Plane.yz(), live=live)

    def instance_selection(self):
        for i in self.viewport.selection.nodes():
            self.ops.instance(i, Transform((20.0, 0.0, 0.0)))

    def array(self):
        ids = self.viewport.selection.nodes()
        if not ids:
            return self.error("Select bodies to array")
        d = ArrayDialog(self)
        if not d.exec():
            return
        from ..units import evaluate

        try:
            if d.kind.currentText() == "rectangular":
                vals = (evaluate(d.sx.text()), evaluate(d.sy.text()), evaluate(d.sz.text()))
                count = (d.cx.value(), d.cy.value(), d.cz.value())
                if d.mode.currentIndex() == 0:
                    self.ops.array_rect(ids, count, spacing=vals, as_instances=d.instances.isChecked(), merge=d.merge.isChecked())
                else:
                    self.ops.array_rect(ids, count, extent=vals, as_instances=d.instances.isChecked(), merge=d.merge.isChecked())
            else:
                p = self.viewport.active_plane or Plane.xy()
                self.ops.array_radial(ids, d.count.value(), p.origin, p.normal, evaluate(d.angle.text(), angle=True), as_instances=d.instances.isChecked(), merge=d.merge.isChecked())
        except Exception as e:
            self.error(str(e))

    def cut_with_plane(self):
        for i in self.viewport.selection.nodes():
            self.ops.cut(i, self.viewport.active_plane or Plane.xy())

    def cut_with_selection(self):
        ids = self.viewport.selection.nodes()
        if len(ids) < 2:
            return self.error("Select the body, then the cutter")
        self.ops.cut(ids[0], ids[1])

    def split_face(self):
        for i in self.viewport.selection.nodes():
            self.ops.split_face(i, self.viewport.active_plane or Plane.xy())

    def imprint(self):
        ids = self.viewport.selection.nodes()
        if len(ids) < 2:
            return self.error("Select the body, then the tool")
        self.ops.imprint(ids[0], ids[1])

    def project_curve(self):
        ids = self.viewport.selection.nodes()
        if len(ids) < 2:
            return self.error("Select the curve/sketch, then the body")
        _, _, back = self.viewport.camera.basis()
        self.ops.project_curve(ids[0], ids[1], v_scale(back, -1.0))

    def silhouette(self):
        for i in self.viewport.selection.nodes():
            self.ops.silhouette(i, self.viewport.active_plane or Plane.xy())

    def control_points(self):
        faces = self.viewport.selection.faces()
        if not faces:
            return self.error("Select a face")
        nid, fi = faces[0]
        body = self.doc.resolved_body(nid)
        f = self.doc.kernel.faces(body)[fi]
        pts = self.doc.kernel.control_points(body, f)
        self.viewport.temp_shapes = [("point", (p, (1.0, 0.5, 0.9), 8.0)) for row in pts for p in row] + [("poly", (row, (0.9, 0.4, 0.8))) for row in pts]
        self.status(f"{sum(len(r) for r in pts)} control points (edit via Ops.set_control_points; proportional falloff in scripting)")
        self.viewport.update()

    def raise_degree(self):
        faces = self.viewport.selection.faces()
        if faces:
            nid, fi = faces[0]
            f = self.doc.kernel.faces(self.doc.resolved_body(nid))[fi]
            self.ops.raise_degree(nid, f, 4, 4)

    def rebuild_face(self):
        faces = self.viewport.selection.faces()
        if not faces:
            return
        n, ok = QInputDialog.getInt(self, "Rebuild face", "Spans per direction:", 4, 1, 64)
        if ok:
            nid, fi = faces[0]
            f = self.doc.kernel.faces(self.doc.resolved_body(nid))[fi]
            self.ops.rebuild_face(nid, f, n, n)

    def dependent_offset(self):
        faces = self.viewport.selection.faces()
        ids = self.viewport.selection.nodes()
        targets = [i for i in ids if not any(n == i for n, _ in faces)]
        if not faces or not targets:
            return self.error("Select a face, then the body to offset it to")
        c, ok = QInputDialog.getDouble(self, "Dependent offset", "Clearance (mm):", 0.2, -10, 10, 2)
        if ok:
            nid, fi = faces[0]
            f = self.doc.kernel.faces(self.doc.resolved_body(nid))[fi]
            self.ops.offset_face_to(nid, f, targets[0], c)

    def set_pivot(self):
        ids = self.viewport.selection.nodes()
        if ids:
            pos = self.viewport.mapFromGlobal(self.cursor().pos())
            s = self.viewport.snap(pos.x(), pos.y())
            self.ops.set_pivot(ids[0], s.point)

    def set_active_plane(self, node_id: Optional[str], plane: Optional[Plane] = None):
        if node_id:
            plane = self.doc.nodes[node_id].plane
        self.viewport.active_plane = plane
        self.status(f"Active plane set")
        self.viewport.update()

    def toggle_plane_snapping(self):
        self.viewport.plane_snapping = not self.viewport.plane_snapping
        self.status(f"2D snapping {'on' if self.viewport.plane_snapping else 'off'}")

    # ---- view ---------------------------------------------------------------------
    def toggle_ortho(self):
        self.viewport.camera.orthographic = not self.viewport.camera.orthographic
        self.viewport.update()

    def toggle_grid(self):
        self.viewport.show_grid = not self.viewport.show_grid
        self.viewport.update()

    def next_display_mode(self):
        modes = Viewport.MODES
        self.set_display_mode(modes[(modes.index(self.viewport.display_mode) + 1) % len(modes)])

    def set_display_mode(self, mode):
        self.viewport.display_mode = mode
        self.viewport.update()

    def toggle_orbit_mode(self):
        cam = self.viewport.camera
        if cam.mode == "turntable":
            cam.sync_trackball()
            cam.mode = "trackball"
        else:
            cam.mode = "turntable"
        self.status(f"Orbit: {cam.mode}")

    def set_fov(self):
        v, ok = QInputDialog.getDouble(self, "Field of view", "Degrees:", self.viewport.camera.fov, 5, 120, 1)
        if ok:
            self.viewport.camera.fov = v
            self.viewport.update()

    def toggle_section(self):
        if self.viewport.section_enabled and isinstance(self.tool, SectionTool):
            self.viewport.section_enabled = False
            self.set_tool(SelectTool(self.ctx))
        else:
            self.set_tool(SectionTool(self.ctx))

    def toggle_build_plate(self):
        vp = self.viewport
        vp.build_plate = None if vp.build_plate else (220.0, 220.0)
        vp.show_overhangs = vp.build_plate is not None
        vp.dirty_nodes.update(vp.items.keys())
        vp.update()

    def toggle_overhangs(self):
        vp = self.viewport
        vp.show_overhangs = not vp.show_overhangs
        vp.dirty_nodes.update(vp.items.keys())
        vp.update()

    def toggle_high_contrast(self):
        self.high_contrast = not self.high_contrast
        self.settings.setValue("high_contrast", int(self.high_contrast))
        self.apply_theme()

    def apply_theme(self):
        QApplication.instance().setStyleSheet(HIGH_CONTRAST_QSS if self.high_contrast else DARK_QSS)
        self.viewport.background_high_contrast = self.high_contrast
        self.viewport.update()

    def view_radial(self):
        entries = [("Front", lambda: self.commands["view.front"]["run"]()), ("Top", lambda: self.commands["view.top"]["run"]()), ("Right", lambda: self.commands["view.right"]["run"]()), ("Iso", lambda: self.commands["view.iso"]["run"]()), ("Ortho", self.toggle_ortho), ("Grid", self.toggle_grid), ("Mode", self.next_display_mode), ("Fit", self.viewport.focus_all)]
        RadialMenu(entries, self).open_at(self.cursor().pos())

    def selection_radial(self):
        entries = [(m.capitalize(), lambda mm=m: self.set_selection_mode(mm)) for m in ("body", "face", "edge", "vertex", "point")]
        RadialMenu(entries, self).open_at(self.cursor().pos())

    def _context_menu(self, pos):
        menu = QMenu(self)
        for cid in ("tool.annotate", "view.comments", "tool.push_pull", "tool.fillet", "tool.chamfer", "tool.shell", "modify.union", "modify.subtract", "tool.mirror", "tool.array", "tool.measure", "view.isolate", "view.hide", "edit.delete"):
            menu.addAction(self.commands[cid]["action"])
        menu.exec(self.viewport.mapToGlobal(pos.toPoint()))

    def open_palette(self):
        CommandPalette(self.commands, self).open_at(self.mapToGlobal(QPoint(self.width() // 2, 80)))

    # ---- inspect / print ------------------------------------------------------------
    def wall_check(self):
        t, ok = QInputDialog.getDouble(self, "Wall thickness", "Flag walls thinner than (mm):", 1.2, 0.1, 20, 2)
        if not ok:
            return
        ids = self.viewport.selection.nodes() or [n.id for n in self.doc.bodies(True)]
        marks = []
        for i in ids:
            b = self.doc.resolved_body(i)
            if b is None:
                continue
            for r in wall_thickness(self.doc.kernel, b, t):
                marks.append(("point", (r.point, (1.0, 0.2, 0.2), 9.0)))
        self.viewport.temp_shapes = marks
        self.status(f"{len(marks)} thin region(s) under {t} mm" if marks else f"No walls thinner than {t} mm")
        self.viewport.update()

    def validate(self):
        items = [(n.name, n.body) for n in self.doc.bodies(True)]
        ok, messages = validate_for_export(self.doc.kernel, items)
        if ok:
            QMessageBox.information(self, "Validation", f"{len(items)} body(ies): valid and watertight.")
        else:
            QMessageBox.warning(self, "Validation", "\n".join(messages))

    def curvature_comb(self):
        from ..analysis import curvature_comb

        for i in self.viewport.selection.nodes():
            b = self.doc.resolved_body(i)
            if b is not None and self.doc.nodes[i].kind in ("curve", "sketch"):
                self.viewport.temp_shapes = [("line", (a, c, (0.9, 0.5, 1.0))) for a, c in curvature_comb(self.doc.kernel, b)]
        self.viewport.update()

    def continuity(self):
        from ..analysis import continuity_report

        for i in self.viewport.selection.nodes():
            b = self.doc.resolved_body(i)
            if b is None:
                continue
            rep = continuity_report(self.doc.kernel, b)
            colors = {"G0": (1.0, 0.3, 0.3), "G1": (1.0, 0.8, 0.3), "G2": (0.3, 0.9, 0.4), "boundary": (0.5, 0.5, 0.5)}
            shapes = []
            for e, g in rep:
                pts = self.doc.kernel.sample_edge(e, b, 16)
                shapes.append(("poly", (pts, colors[g])))
            self.viewport.temp_shapes = shapes
            counts = {g: sum(1 for _, x in rep if x == g) for g in colors}
            self.status(f"Continuity: {counts}")
        self.viewport.update()

    def draft_shading(self):
        self.status("Draft shading: green = positive draft, red = negative, yellow = vertical (pull direction +Z)")
        for i in self.viewport.selection.nodes():
            it = self.viewport.items.get(i)
            if it is not None:
                from ..analysis import draft_angle_colors

                m = self.doc.mesh_of(i)
                cols = draft_angle_colors(m, (0, 0, 1))
                import numpy as np

                it.overhang = np.array([c == (0.85, 0.3, 0.3) for c in cols])
        self.viewport.show_overhangs = True
        self.viewport.update()

    def normal_shading(self):
        self.status("Normal shading: blue faces point at you, orange away")
        self.viewport.display_mode = "xray"
        self.viewport.update()

    # ---- file -----------------------------------------------------------------------
    def open_file(self):
        p, _ = QFileDialog.getOpenFileName(self, "Open", "", "robocad (*.rcad)")
        if p:
            MainWindow(path=p).show()

    def save(self):
        if not self.doc.path:
            return self.save_as()
        self.doc.save(thumbnail=self.thumbnail())
        self.setWindowTitle(self._title())
        self.status("Saved")

    def save_as(self):
        p, _ = QFileDialog.getSaveFileName(self, "Save as", "", "robocad (*.rcad)")
        if p:
            if not p.endswith(".rcad"):
                p += ".rcad"
            self.doc.save(p, thumbnail=self.thumbnail())
            self.setWindowTitle(self._title())

    def thumbnail(self) -> bytes:
        try:
            img = self.viewport.grabFramebuffer().scaled(256, 192, Qt.KeepAspectRatio, Qt.SmoothTransformation)
            from PySide6.QtCore import QBuffer, QByteArray

            buf = QBuffer()
            buf.open(QBuffer.WriteOnly)
            img.save(buf, "PNG")
            return bytes(buf.data())
        except Exception:
            return b""

    def import_file(self):
        p, _ = QFileDialog.getOpenFileName(self, "Import", "", "CAD & mesh (*.step *.stp *.iges *.igs *.stl *.obj *.3mf *.fbx *.svg *.png *.jpg *.jpeg)")
        if p:
            self.import_path(p)

    def import_path(self, p: str):
        from ..io import importers

        ext = os.path.splitext(p)[1].lower()
        if ext in (".step", ".stp"):
            importers.import_step(self.doc, p)
        elif ext in (".iges", ".igs"):
            importers.import_iges(self.doc, p)
        elif ext in (".stl", ".obj", ".3mf", ".fbx", ".ply", ".glb", ".gltf"):
            try:
                mesh, extent = importers.load_mesh_file(p, "mm")
            except Exception as e:
                return self.error(f"Could not read mesh: {e} (FBX needs the optional fbx loader; convert to OBJ/glTF)")
            d = UnitsDialog(importers.mesh_units_guess(extent), self)
            if d.exec():
                importers.import_mesh(self.doc, p, d.unit())
        elif ext == ".svg":
            importers.import_svg(self.doc, p, self.viewport.active_plane or Plane.xy())
        elif ext in (".png", ".jpg", ".jpeg"):
            self.references.add_paths([p])
            return
        self.viewport.focus_all()

    def export_file(self):
        p, flt = QFileDialog.getSaveFileName(self, "Export", self.settings.value("last_export", ""), "STL (*.stl);;3MF (*.3mf);;STEP (*.step);;IGES (*.iges);;OBJ (*.obj);;Sketch SVG (*.svg)")
        if not p:
            return
        self.settings.setValue("last_export", p)
        self.export_path(p)

    def export_path(self, p: str, ids=None):
        from ..io import exporters

        ext = os.path.splitext(p)[1].lower().lstrip(".")
        settings = self.export_settings.setdefault(ext, {})
        try:
            if ext == "stl":
                d = ExportDialog("stl", settings, self)
                if not d.exec():
                    return
                v = d.values()
                w = exporters.export_stl(self.doc, p, ids, exporters.StlSettings(v["binary"], v["unit"], v["tolerance"], v["angular_deg"]))
            elif ext == "3mf":
                d = ExportDialog("3mf", settings, self)
                if not d.exec():
                    return
                v = d.values()
                w = exporters.export_3mf(self.doc, p, ids, exporters.ThreeMfSettings(v["tolerance"], v["colors"], v["names"]))
            elif ext in ("step", "stp"):
                d = ExportDialog("step", settings, self)
                if not d.exec():
                    return
                v = d.values()
                exporters.export_step(self.doc, p, ids, exporters.StepSettings(v["schema"], v["names"], v["colors"]))
                w = []
            elif ext in ("iges", "igs"):
                exporters.export_iges(self.doc, p, ids)
                w = []
            elif ext == "obj":
                d = ExportDialog("obj", settings, self)
                if not d.exec():
                    return
                v = d.values()
                w = exporters.export_obj(self.doc, p, ids, exporters.ObjSettings(v["tolerance"], v["scale"], v["up_axis"], v["quads"], v["ngons"], v["mtl"], v["uvs"]))
            elif ext == "svg":
                sid = self._selected_sketch()
                if not sid:
                    return self.error("Select a sketch to export as SVG")
                exporters.export_sketch_svg(self.doc, p, sid)
                w = []
            else:
                return self.error(f"Unknown export format {ext}")
            self.settings.setValue("export_settings", json.dumps(self.export_settings))
            self.status(f"{tr('export.done')}: {p}" + (f"  ({len(w)} warning(s))" if w else ""))
        except exporters.ExportError as e:
            QMessageBox.warning(self, tr("export.blocked"), str(e))

    def export_drawing(self):
        from ..io.drawing import STANDARD_VIEWS, View, export_drawing_svg

        p, _ = QFileDialog.getSaveFileName(self, "Export drawing", "", "SVG (*.svg)")
        if not p:
            return
        views = [STANDARD_VIEWS["front"], STANDARD_VIEWS["top"], STANDARD_VIEWS["right"], STANDARD_VIEWS["iso"]]
        if self.viewport.section_enabled and self.viewport.section_plane is not None:
            views.append(View("Section A-A", self.viewport.section_plane.normal, section=self.viewport.section_plane))
        export_drawing_svg(self.doc, p, views, title=os.path.basename(self.doc.path or "untitled"))
        self.status(f"Drawing exported: {p}")

    def delete_selection(self):
        ids = self.viewport.selection.nodes()
        if ids:
            self.ops.delete(ids)
            self.viewport.selection.clear()
            self.selection_changed(None)

    def copy_with_placement(self):
        clip = self.doc.copy_nodes(self.viewport.selection.nodes())
        QApplication.clipboard().setText(json.dumps(clip))
        self.status(f"Copied {len(clip['items'])} item(s) with placement")

    def paste_with_placement(self):
        try:
            clip = json.loads(QApplication.clipboard().text())
        except Exception:
            return self.error("Clipboard has no robocad content")
        if not clip.get("robocad_clipboard"):
            return
        from ..commands import AddNodes

        nodes = self.doc.paste_nodes(clip, keep_placement=True)
        # Make the paste undoable as one step.
        for n in nodes:
            self.doc.remove(n.id)
        self.ops.stack.push(AddNodes("Paste", nodes))
        self.status(f"Pasted {len(nodes)} item(s)")

    def preferences(self):
        v, ok = QInputDialog.getInt(self, "Preferences", "Autosave interval (seconds):", int(self.doc.autosave_interval), 10, 3600)
        if ok:
            self.settings.setValue("autosave_interval", v)
            self._start_autosave(v)
        g, ok = QInputDialog.getDouble(self, "Preferences", "Grid step (mm):", self.viewport.grid_step, 0.1, 1000, 2)
        if ok:
            self.viewport.grid_step = g
            self.viewport.update()

    # ---- bridge / share / simulation ------------------------------------------------------
    def start_bridge(self):
        from ..bridge.server import BridgeServer

        if self.bridge is None:
            self.bridge = BridgeServer(self.doc)
            self.bridge.start()
        self.status(f"Live link listening on ws://127.0.0.1:{self.bridge.port} — install blender_addon/robocad_link.py in Blender")

    def stop_bridge(self):
        if self.bridge:
            self.bridge.stop()
            self.bridge = None
            self.status("Live link stopped")

    def web_share(self):
        from ..bridge.webshare import publish

        p, _ = QFileDialog.getSaveFileName(self, "Publish web viewer", "", "HTML (*.html)")
        if p:
            publish(self.doc, p)
            self.status(f"Viewer written: {p} (static; no editable source inside)")

    # ---- robotics ---------------------------------------------------------
    def _robot_bodies(self):
        return [(n.id, n.name) for n in self.doc.bodies() if not (n.robot or {}).get("kind") == "motor"]

    def _robot_motors(self):
        return [(n.id, n.name) for n in self.doc.walk() if n.robot and n.robot.get("kind") == "motor"]

    def robot_add_motor(self):
        d = MotorDialog(self, self._robot_bodies(), getattr(self, "last_motor", None))
        if d.exec():
            params = d.values()
            self.last_motor = params
            self.set_tool(MotorTool(self.ctx, params))

    def robot_add_joint(self):
        def done(preset):
            self.set_tool(SelectTool(self.ctx))
            self._robot_joint_from(preset)

        self.set_tool(JointTool(self.ctx, done))

    def robot_joint_dialog(self):
        ids = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind == "body"]
        preset = {}
        if len(ids) >= 2:
            preset = {"parent": ids[0], "child": ids[1]}
        elif len(ids) == 1:
            preset = {"child": ids[0]}
        pl = self.viewport.active_plane
        if pl is not None:
            preset.update({"pivot": pl.origin, "axis": pl.normal})
        self._robot_joint_from(preset)

    def _robot_joint_from(self, preset: dict):
        d = JointDialog(self, self._robot_bodies(), self._robot_motors(), preset)
        if d.exec():
            v = d.values()
            if not v["child"]:
                self.error("a joint needs a child body")
                return
            jid = self.ops.add_joint(v["type"], v["parent"], v["child"], v["pivot"], v["axis"], v["lower"], v["upper"], v["motor"], v["gear_ratio"], v["name"])
            if v["damping"]:
                self.ops.set_joint(jid, damping=v["damping"])
            self.viewport.selection.set_nodes([jid])
            self.status(f"joint {self.doc.nodes[jid].name} added")

    def robot_edit_joint(self, jid: str):
        j = self.doc.nodes[jid].joint
        preset = {**j.to_json(), "name": self.doc.nodes[jid].name}
        d = JointDialog(self, self._robot_bodies(), self._robot_motors(), preset, title="Edit joint")
        if d.exec():
            v = d.values()
            name = v.pop("name")
            self.ops.set_joint(jid, **v)
            if name and name != self.doc.nodes[jid].name:
                self.ops.rename(jid, name)
            self.status(f"joint {self.doc.nodes[jid].name} updated")

    def robot_infer(self):
        made = self.ops.infer_joints()
        if made:
            self.viewport.selection.set_nodes(made)
        self.status(f"{len(made)} joint(s) inferred from coaxial hole/pin pairs" if made else "no new coaxial hole/pin pairs found (each pair needs a hole in the parent and a matching pin in the child)")

    def robot_assign_motor(self):
        sel = self.viewport.selection.nodes()
        motors = [i for i in sel if (self.doc.nodes[i].robot or {}).get("kind") == "motor"]
        joints = [i for i in sel if self.doc.nodes[i].kind == "joint"]
        d = QDialog(self)
        d.setWindowTitle("Assign motor to joint")
        form = QFormLayout(d)
        mbox, jbox = QComboBox(), QComboBox()
        for mid, name in self._robot_motors():
            mbox.addItem(name, mid)
        for n in self.doc.walk():
            if n.kind == "joint":
                jbox.addItem(n.name, n.id)
        if motors:
            mbox.setCurrentIndex(max(0, mbox.findData(motors[0])))
        if joints:
            jbox.setCurrentIndex(max(0, jbox.findData(joints[0])))
        gear = QDoubleSpinBox()
        gear.setRange(0.01, 10000.0)
        gear.setValue(1.0)
        form.addRow("Motor", mbox)
        form.addRow("Joint", jbox)
        form.addRow("Extra gear ratio", gear)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(d.accept)
        bb.rejected.connect(d.reject)
        form.addRow(bb)
        if mbox.count() == 0 or jbox.count() == 0:
            self.error("add a motor and a joint first")
            return
        if d.exec():
            self.ops.attach_motor(jbox.currentData(), mbox.currentData(), gear.value())
            self.status(f"{mbox.currentText()} now drives {jbox.currentText()}")

    def robot_fixed(self):
        ids = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind == "body"]
        if len(ids) < 2:
            self.error("select the parent body first, then the bodies to fix to it")
            return
        for c in ids[1:]:
            self.ops.connect_fixed(ids[0], c)
        self.status(f"{len(ids) - 1} bod{'y' if len(ids) == 2 else 'ies'} fixed to {self.doc.nodes[ids[0]].name}")

    def robot_ground(self):
        ids = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind == "body"]
        if not ids:
            self.error("select the body that is fixed to the world")
            return
        for i in ids:
            self.ops.set_ground(i, not (self.doc.nodes[i].robot or {}).get("ground", False))
        self.status("ground toggled on " + ", ".join(self.doc.nodes[i].name for i in ids))

    def robot_validate(self):
        info = self.ops.robot()
        if not info["issues"]:
            mobility = f"{info['dof']} DoF" if info['dof'] is not None else 'closed-loop mobility requires constraint analysis'
            self.status(f"robot valid: {info['links']} bodies, {len(info['joints'])} joints, {mobility}")
        else:
            QMessageBox.warning(self, "Robot validation", "\n".join(f"[{i['severity']}] {i['message']}" for i in info["issues"]))
        self.robot_panel.refresh()

    def robot_motor_library(self):
        lib = self.ops.motor_library()
        rows = [f"{m['name']:<28} {m['kind']:<13} {m['stall_torque']:>7g} N·m {m['no_load_speed']:>6g} rad/s {m['mass_g']:>6g} g   {m['notes']}" for m in lib.values()]
        QMessageBox.information(self, "Motor library", "<pre>" + "\n".join(rows) + "</pre>")

    def robot_add_sensor(self):
        sel = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind == "body"]
        joints = [(n.id, n.name) for n in self.doc.walk() if n.kind == "joint"]
        d = SensorDialog(self, self._robot_bodies(), joints, {"body": sel[0] if sel else None})
        if d.exec():
            v = d.values()
            sid = self.ops.add_sensor(v["kind"], v["body"], v["point"], None, v["name"], v["joint"], rate_hz=v["rate_hz"])
            self.viewport.selection.set_nodes([sid])
            self.status(f"sensor {self.doc.nodes[sid].name} added")

    def robot_add_cable(self):
        sel = [i for i in self.viewport.selection.nodes() if self.doc.nodes[i].kind == "body"]
        preset = {"from_body": sel[0] if sel else None, "to_body": sel[1] if len(sel) > 1 else None}
        d = CableDialog(self, self._robot_bodies(), preset)
        if d.exec():
            v = d.values()
            cid = self.ops.add_cable(v["from_body"], v["from_point"], v["to_body"], v["to_point"], v["length"], v["mass"], None, v["name"])
            self.viewport.selection.set_nodes([cid])
            self.status(f"cable {self.doc.nodes[cid].name} added")

    def robot_power(self):
        joints = [(n.id, n.name) for n in self.doc.walk() if n.kind == "joint" and n.joint is not None and n.joint.type in ("revolute", "continuous", "prismatic")]
        d = PowerDialog(self, self.doc.robot_settings, joints)
        if d.exec():
            d.apply(self.ops)
            self.status("battery, control and uncertainty updated")

    def robot_load_results(self):
        default = ""
        if self.doc.path:
            root, _ = os.path.splitext(self.doc.path)
            cand = root + ".simresult.json"
            default = cand if os.path.exists(cand) else os.path.dirname(self.doc.path)
        p, _ = QFileDialog.getOpenFileName(self, "Load simulation results", default, "Simulation results (*.simresult.json);;JSON (*.json)")
        if p:
            self.ops.load_results(p)
            self.viewport.clear_stress_colors()
            self.viewport.show_stress = True
            self.status(f"results loaded: {p} (stress overlay on; margins in the Robot panel)")

    def robot_apply_identification(self):
        p, _ = QFileDialog.getOpenFileName(self, "Apply identification", os.path.dirname(self.doc.path or "") or "", "Fit / results (*.json)")
        if p:
            fitted = self.ops.apply_identification(p)
            self.status(f"identified parameters stored for {', '.join(fitted)}; they ride along with the next export")

    def toggle_stress(self):
        self.viewport.show_stress = not self.viewport.show_stress
        self.viewport.clear_stress_colors()
        self.status("stress overlay " + ("on: blue 0 → red at yield, from the loaded results" if self.viewport.show_stress else "off"))

    def sim_export_physical(self):
        from ..physical import export_physical_model

        p, _ = QFileDialog.getSaveFileName(self, "Export physical model", "", "Sim model (*.simrobot.json)")
        if p:
            model = export_physical_model(self.doc, p, flex=True)
            self.status(f"physical model written: {p} ({len(model['links'])} links, {sum(1 for l in model['links'] if l['flex'])} flexible)")

    def sim_export(self):
        from ..simbridge import export_sim_model

        p, _ = QFileDialog.getSaveFileName(self, "Export simulation model", "", "Sim model (*.simrobot.json)")
        if p:
            export_sim_model(self.doc, p)
            self.status(f"Simulation model written: {p}")

    def sim_link_toggle(self):
        from ..simbridge import SimLink

        if self.sim_link is None:
            if not self.doc.path:
                return self.error("Save the document first: the link watches the saved file")
            self.sim_link = SimLink(self.doc, self)
            self.sim_link.start()
            self.status("Simulation link: the model is re-exported on every save and the sim viewer reloads it")
        else:
            self.sim_link.stop()
            self.sim_link = None
            self.status("Simulation link stopped")

    def start_api(self):
        """The local REST API (`robocad/api.py`), one port per window from 8420 up."""
        from ..api import ApiServer

        port = int(os.environ.get("ROBOCAD_API_PORT", 8420))
        for attempt in range(20):
            try:
                self.api = ApiServer(self.doc, self.ops, self, port=port + attempt).start()
                break
            except OSError:
                continue
        if self.api:
            self.status(f"REST API on {self.api.url}")

    def show_api(self):
        QMessageBox.information(self, "REST API", f"{self.api.url if self.api else 'not running'}\n\nGET /doc, /nodes, /render?view=iso … see robocad/api.py")

    # ---- misc -----------------------------------------------------------------------
    def _drop_material(self, e):
        if e.mimeData().hasUrls():
            self.references.dropEvent(e)
            return
        self.pose_panel.stop()
        text = e.mimeData().text()
        if text.startswith("material:"):
            mid = text.split(":", 1)[1]
            pos = e.position().toPoint()

            def done(result):
                if result["hit"]:
                    self.ops.set_material([result["hit"][1]], mid)

            self.viewport.request_pick(pos.x(), pos.y(), done)
            e.acceptProposedAction()

    def _on_stack(self):
        self.setWindowTitle(self._title())
        # One refresh for a command batch, rather than expensive repeated mass
        # and robot validation queries on every node notification.
        self._refresh_timer.start(0)

    def _refresh_panels(self):
        if self._geometry_refresh:
            self.outliner.refresh()
            self.robot_panel.refresh()
            self.properties.refresh()
            self.references.refresh()
        self._geometry_refresh = False
        self.comments.refresh()

    def _on_doc(self, event, payload):
        if event == 'saved_views':
            self.saved_views_panel.refresh()
            self.setWindowTitle(self._title())
            return
        if event in ('changed', 'added', 'removed', 'moved'):
            self.pose_panel.stop()
        if event in ("annotations", "changed", "removed", "saved"):
            self.setWindowTitle(self._title())
            self._geometry_refresh |= event in ("changed", "removed")
            self._refresh_timer.start(0)
        if event == "results" and hasattr(self, "robot_panel"):
            self.robot_panel.refresh()
            self.viewport.clear_stress_colors()
        if event == "autosaved":
            self.status(f"Autosaved to {payload}")
        if event == "saved" and self.bridge:
            pass

    def status(self, text: str):
        self.statusBar().showMessage(text)

    def readout(self, text: str):
        self.readout_label.setText(text)

    def error(self, text: str):
        self.statusBar().showMessage(f"⚠ {text}", 8000)
        QApplication.beep()

    def show_guide(self):
        p = os.path.join(os.path.dirname(__file__), "..", "..", "USER_GUIDE.md")
        QMessageBox.information(self, "User guide", f"See {os.path.abspath(p)}")

    def _init_spacemouse(self):
        """3Dconnexion support through `pyspacemouse` when installed; buttons
        map through ~/.robocad/spacemouse.json ({"0": "view.fit", "1": "view.iso"})."""
        try:
            import pyspacemouse  # type: ignore
        except Exception:
            return
        try:
            if not pyspacemouse.open():
                return
        except Exception:
            return
        mapping = {"0": "view.fit", "1": "view.iso"}
        user = os.path.join(os.path.expanduser("~"), ".robocad", "spacemouse.json")
        if os.path.exists(user):
            with open(user) as f:
                mapping.update(json.load(f))
        self._sm_buttons = [0] * 8

        def poll():
            st = pyspacemouse.read()
            cam = self.viewport.camera
            cam.orbit(st.yaw * 6.0, -st.pitch * 6.0)
            cam.pan(-st.x * 8.0, st.y * 8.0, self.viewport.height())
            cam.zoom(1.0 - st.z * 0.05)
            for i, b in enumerate(st.buttons):
                if b and not self._sm_buttons[i] and str(i) in mapping and mapping[str(i)] in self.commands:
                    self.commands[mapping[str(i)]]["run"]()
                self._sm_buttons[i] = b
            self.viewport.update()

        self._spacemouse = QTimer(self)
        self._spacemouse.timeout.connect(poll)
        self._spacemouse.start(16)

    def closeEvent(self, e):
        if self.doc.dirty:
            r = QMessageBox.question(self, "Unsaved changes", "Save before closing?", QMessageBox.Save | QMessageBox.Discard | QMessageBox.Cancel)
            if r == QMessageBox.Cancel:
                e.ignore()
                return
            if r == QMessageBox.Save:
                self.save()
        # Discard picks queued by the last frame before child widgets are
        # destroyed; their callbacks can otherwise access deleted panels.
        self.viewport.cancel_picks()
        self.properties.cancel_measurement()
        self.doc.stop_autosave()
        self._autosave_timer.stop()
        self._autosave_poll.stop()
        self._autosave_executor.shutdown(wait=False)
        self.experiments_panel.shutdown()
        self.pose_panel.stop()
        self.stop_bridge()
        if self.api:
            self.api.stop()
        if self.sim_link:
            self.sim_link.stop()
        if self in WINDOWS:
            WINDOWS.remove(self)
        e.accept()


def main(argv=None):
    argv = argv if argv is not None else sys.argv
    app = QApplication.instance() or QApplication(argv)
    app.setApplicationName("robocad")
    path = next((a for a in argv[1:] if a.endswith(".rcad")), None)
    w = MainWindow(path=path)
    w.show()
    for a in argv[1:]:
        if os.path.exists(a) and not a.endswith(".rcad"):
            w.import_path(a)
    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
