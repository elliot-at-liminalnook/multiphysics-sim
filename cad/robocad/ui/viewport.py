"""The 3D viewport: a QOpenGLWidget drawing the document's tessellations
with fixed-function OpenGL (portable and fast enough for a few hundred
thousand triangles), an ID-buffer pass for picking bodies, faces, edges
and vertices, snapping, a section plane, a build plate with overhang
shading, matcap and render modes, and a view cube."""

from __future__ import annotations

import math
import time
from dataclasses import dataclass, field, replace
from typing import Callable, Optional

import numpy as np
from OpenGL import GL
from PySide6.QtCore import QPoint, QPointF, Qt, QTimer, Signal
from PySide6.QtGui import QColor, QFont, QImage, QMouseEvent, QPainter, QPen, QSurfaceFormat, QWheelEvent
from PySide6.QtOpenGLWidgets import QOpenGLWidget
from PySide6.QtWidgets import QWidget

from ..document import Document, Node
from ..kernel import EdgeRef, FaceRef, Plane, Vec3
from ..kernel.base import Mesh, v_add, v_cross, v_dist, v_dot, v_norm, v_scale, v_sub, v_unit
from ..printing import overhangs

# ------------------------------------------------------------------ camera


class ViewportOverlay(QWidget):
    """Raster overlay keeps labels visible with legacy OpenGL on macOS."""
    def __init__(self, viewport):
        super().__init__(viewport)
        self.setAttribute(Qt.WA_TransparentForMouseEvents)
        self.setAttribute(Qt.WA_NoSystemBackground)

    def paintEvent(self, event):
        self.parent()._draw_overlay_text(self)


@dataclass
class Camera:
    target: Vec3 = (0.0, 0.0, 0.0)
    distance: float = 250.0
    yaw: float = -35.0  # degrees about +Z
    pitch: float = 28.0  # degrees above the XY plane
    fov: float = 40.0
    orthographic: bool = False
    mode: str = "turntable"  # turntable | trackball
    # trackball state: an explicit rotation matrix (world → view)
    rot: np.ndarray = field(default_factory=lambda: np.eye(3))

    def direction(self) -> Vec3:
        """Unit vector from the target toward the eye."""
        if self.mode == "trackball":
            return tuple(float(x) for x in self.rot.T @ np.array([0.0, 0.0, 1.0]))
        y, p = math.radians(self.yaw), math.radians(self.pitch)
        return (math.cos(p) * math.cos(y), math.cos(p) * math.sin(y), math.sin(p))

    def eye(self) -> Vec3:
        return v_add(self.target, v_scale(self.direction(), self.distance))

    def up(self) -> Vec3:
        if self.mode == "trackball":
            return tuple(float(x) for x in self.rot.T @ np.array([0.0, 1.0, 0.0]))
        return (0.0, 0.0, 1.0)

    def basis(self) -> tuple[Vec3, Vec3, Vec3]:
        """Right, up, back (toward the eye) unit vectors of the view."""
        back = v_unit(self.direction())
        up = self.up()
        right = v_unit(v_cross(up, back))
        up2 = v_unit(v_cross(back, right))
        return right, up2, back

    def sync_trackball(self):
        r, u, b = self.basis()
        self.rot = np.array([r, u, b])

    def orbit(self, dx: float, dy: float):
        if self.mode == "trackball":
            ax = np.array(self.basis()[1])
            ay = np.array(self.basis()[0])
            self.rot = _rot_about(ax, -dx * 0.4) @ _rot_about(ay, -dy * 0.4) @ self.rot if False else self.rot @ _rot_about(ax, dx * 0.4) @ _rot_about(ay, dy * 0.4)
            return
        self.yaw -= dx * 0.4
        self.pitch = max(-89.5, min(89.5, self.pitch + dy * 0.4))

    def pan(self, dx: float, dy: float, height_px: int):
        right, up, _ = self.basis()
        scale = self.world_per_pixel(height_px)
        self.target = v_add(self.target, v_add(v_scale(right, -dx * scale), v_scale(up, dy * scale)))

    def world_per_pixel(self, height_px: int) -> float:
        h = 2.0 * self.distance * math.tan(math.radians(self.fov) / 2)
        return h / max(height_px, 1)

    def zoom(self, factor: float, anchor: Optional[Vec3] = None):
        if anchor is not None:
            # Zoom toward the point under the cursor: move the target so the
            # anchor stays fixed on screen.
            d = v_sub(anchor, self.target)
            self.target = v_add(self.target, v_scale(d, 1.0 - factor))
        self.distance = max(0.5, min(1.0e6, self.distance * factor))

    def focus(self, lo: Vec3, hi: Vec3):
        self.target = v_scale(v_add(lo, hi), 0.5)
        radius = 0.5 * v_dist(lo, hi) or 10.0
        self.distance = radius / math.sin(math.radians(self.fov) / 2) * 1.1

    def set_view(self, name: str):
        views = {"front": (-90.0, 0.0), "back": (90.0, 0.0), "right": (0.0, 0.0), "left": (180.0, 0.0), "top": (-90.0, 89.5), "bottom": (-90.0, -89.5), "iso": (-35.0, 28.0)}
        if name in views:
            self.mode = "turntable"
            self.yaw, self.pitch = views[name]

    def snap_orthographic(self):
        """Snap to the nearest axis-aligned view (Alt while orbiting)."""
        self.mode = "turntable"
        self.yaw = round(self.yaw / 90.0) * 90.0
        self.pitch = 89.5 if self.pitch > 45 else (-89.5 if self.pitch < -45 else 0.0)

    def opposite(self):
        self.yaw += 180.0
        self.pitch = -self.pitch

    # matrices (column-major for OpenGL)
    def projection(self, aspect: float, near: float, far: float) -> np.ndarray:
        if self.orthographic:
            h = self.distance * math.tan(math.radians(self.fov) / 2)
            w = h * aspect
            m = np.zeros((4, 4))
            m[0, 0], m[1, 1], m[2, 2] = 1 / w, 1 / h, -2 / (far - near)
            m[2, 3] = -(far + near) / (far - near)
            m[3, 3] = 1.0
            return m
        f = 1.0 / math.tan(math.radians(self.fov) / 2)
        m = np.zeros((4, 4))
        m[0, 0], m[1, 1] = f / aspect, f
        m[2, 2] = (far + near) / (near - far)
        m[2, 3] = 2 * far * near / (near - far)
        m[3, 2] = -1.0
        return m

    def view(self) -> np.ndarray:
        right, up, back = self.basis()
        eye = self.eye()
        m = np.eye(4)
        m[0, :3], m[1, :3], m[2, :3] = right, up, back
        m[0, 3], m[1, 3], m[2, 3] = -v_dot(right, eye), -v_dot(up, eye), -v_dot(back, eye)
        return m

    def ray(self, px: float, py: float, w: int, h: int) -> tuple[Vec3, Vec3]:
        """World ray through a pixel."""
        right, up, back = self.basis()
        nx = (2.0 * px / max(w, 1)) - 1.0
        ny = 1.0 - (2.0 * py / max(h, 1))
        aspect = w / max(h, 1)
        if self.orthographic:
            hh = self.distance * math.tan(math.radians(self.fov) / 2)
            origin = v_add(self.eye(), v_add(v_scale(right, nx * hh * aspect), v_scale(up, ny * hh)))
            return origin, v_scale(back, -1.0)
        t = math.tan(math.radians(self.fov) / 2)
        d = v_unit(v_add(v_add(v_scale(right, nx * t * aspect), v_scale(up, ny * t)), v_scale(back, -1.0)))
        return self.eye(), d

    def project(self, p: Vec3, w: int, h: int) -> Optional[tuple[float, float, float]]:
        right, up, back = self.basis()
        d = v_sub(p, self.eye())
        x, y, z = v_dot(d, right), v_dot(d, up), -v_dot(d, back)
        aspect = w / max(h, 1)
        if self.orthographic:
            hh = self.distance * math.tan(math.radians(self.fov) / 2)
            nx, ny = x / (hh * aspect), y / hh
        else:
            if z <= 1e-6:
                return None
            t = math.tan(math.radians(self.fov) / 2)
            nx, ny = x / (z * t * aspect), y / (z * t)
        return ((nx + 1) * 0.5 * w, (1 - ny) * 0.5 * h, z)


def _rot_about(axis: np.ndarray, deg: float) -> np.ndarray:
    a = math.radians(deg)
    x, y, z = axis / (np.linalg.norm(axis) or 1.0)
    c, s, t = math.cos(a), math.sin(a), 1 - math.cos(a)
    return np.array([[t * x * x + c, t * x * y - s * z, t * x * z + s * y], [t * x * y + s * z, t * y * y + c, t * y * z - s * x], [t * x * z - s * y, t * y * z + s * x, t * z * z + c]])


# ------------------------------------------------------------- render items


@dataclass
class RenderItem:
    node_id: str
    vertices: np.ndarray  # (n,3) float32
    normals: np.ndarray
    indices: np.ndarray  # (m,3) uint32
    tri_face: np.ndarray  # (m,) int32
    edges: np.ndarray  # (k,2) uint32 boundary/crease edges for display
    edge_ref_index: np.ndarray  # (k,) index into kernel EdgeRef list, or -1
    vertex_points: np.ndarray  # (v,3) kernel vertices for snapping/picking
    color: tuple[float, float, float]
    kind: str
    bbox: tuple[Vec3, Vec3]
    face_count: int
    edge_samples: list[np.ndarray] = field(default_factory=list)  # per kernel edge, sampled polyline
    overhang: Optional[np.ndarray] = None  # per-triangle 0/1
    stress_colors: Optional[np.ndarray] = None  # (v,3) float32 from loaded results, see Viewport.show_stress


@dataclass
class Selection:
    """What is selected: (node_id, kind, index) — kind in body/face/edge/vertex/point."""

    items: list[tuple[str, str, int]] = field(default_factory=list)

    def nodes(self) -> list[str]:
        out = []
        for n, _, _ in self.items:
            if n not in out:
                out.append(n)
        return out

    def faces(self) -> list[tuple[str, int]]:
        return [(n, i) for n, k, i in self.items if k == "face"]

    def edges(self) -> list[tuple[str, int]]:
        return [(n, i) for n, k, i in self.items if k == "edge"]

    def clear(self):
        self.items.clear()

    def set_nodes(self, ids):
        """Replace the selection with whole nodes."""
        self.items[:] = [(i, "body", -1) for i in ids]

    def toggle(self, item):
        if item in self.items:
            self.items.remove(item)
        else:
            self.items.append(item)


@dataclass
class SnapResult:
    point: Vec3
    kind: str  # endpoint | midpoint | center | grid | vertex | face | plane | free | intersection | perpendicular | tangent
    node_id: Optional[str] = None


class Viewport(QOpenGLWidget):
    picked = Signal(object)  # (kind, node_id, index, world_point, modifiers)
    hovered = Signal(object)
    dragged = Signal(object)  # (phase, world_point, delta, modifiers) phase: start/move/end
    view_changed = Signal()
    context_requested = Signal(object)

    comment_clicked = Signal(str)

    MODES = ("shaded", "shaded_edges", "wireframe", "xray", "matcap", "render")

    def __init__(self, doc: Document, parent=None):
        fmt = QSurfaceFormat()
        fmt.setDepthBufferSize(24)
        fmt.setSamples(4)
        fmt.setVersion(2, 1)
        fmt.setProfile(QSurfaceFormat.CompatibilityProfile)
        QSurfaceFormat.setDefaultFormat(fmt)
        super().__init__(parent)
        self.setFormat(fmt)
        self.doc = doc
        self.camera = Camera()
        self.items: dict[str, RenderItem] = {}
        self._item_meshes = {}
        self.selection = Selection()
        self.inspection_ids = None  # temporary part isolation; never edits document visibility
        self.selection_mode = "body"  # body | face | edge | vertex | point
        self.hover: Optional[tuple[str, str, int]] = None
        self.display_mode = "shaded_edges"
        self.show_grid = True
        self.grid_step = 10.0
        self.grid_color = (0.36, 0.38, 0.42)
        self.background = (0.13, 0.14, 0.16)
        self.background_high_contrast = False
        self.section_plane: Optional[Plane] = None
        self.section_enabled = False
        from .section_preview import SectionPreview
        self._section_preview = SectionPreview()
        self.build_plate: Optional[tuple[float, float]] = None  # (w, d) mm
        self.overhang_threshold = 45.0
        self.show_overhangs = False
        # Stress overlay from the last loaded simulation results (results hotspot cells → nearest-cell vertex colours).
        self.show_stress = False
        self.active_plane: Optional[Plane] = None
        self.plane_snapping = False
        self.snapping = True
        self.snap_pixels = 12
        self.dirty_nodes: set[str] = set(doc.nodes)
        self._pick_request: Optional[tuple[int, int, Callable]] = None
        self._hover_request = None
        self._pick_generation = 0
        self._hover_timer = QTimer(self)
        self._hover_timer.setSingleShot(True)
        self._hover_timer.timeout.connect(self._hover_due)
        self._hover_ready = False
        self.tool_cursor = Qt.ArrowCursor
        self.tool_name = "Select"
        self.tool_hint = "Click to select • drag to box select"
        self.show_comment_pins = True
        self.pose_matrices = {}
        self._pose_base = None
        self._image_textures = {}
        self.comment_hit = lambda x, y: None
        self._consume_left = False
        self.frame_ms = 0.0
        self._last_mouse = QPoint()
        self._drag_button = None
        self._drag_world_start: Optional[Vec3] = None
        self.setMouseTracking(True)
        self.setFocusPolicy(Qt.StrongFocus)
        self.overlays: list[Callable[[QPainter], None]] = []
        self.overlay_widget = ViewportOverlay(self)
        self.gizmo: Optional[dict] = None  # {"origin": Vec3, "axes": [(Vec3, color)], "mode": move|rotate|scale}
        self.gizmo_hit: Optional[int] = None
        self.temp_shapes: list[tuple[str, object]] = []  # ("line", (a,b,color)) | ("points", ...) | ("poly", ...) | ("mesh", RenderItem)
        self.annotations: list[tuple[Vec3, str]] = []
        self.matcap_tex = None
        self.fps = 0.0
        self._frames = 0
        self._fps_t = time.time()
        self.doc.listeners.append(self._on_doc_event)
        self.render_lights = 3
        self.ground_shadow = True
        self.stats_text = ""

    def is_visible(self, node_id):
        if self.inspection_ids is not None:
            return node_id in self.inspection_ids and node_id in self.doc.nodes
        return self.doc.is_visible(node_id)

    # ---- document sync -------------------------------------------------
    def pose_point(self, node_id, point):
        matrix = self.pose_matrices.get(node_id)
        return tuple(matrix[:3,:3] @ np.asarray(point) + matrix[:3,3]) if matrix is not None else tuple(point)

    def set_pose(self, matrices):
        if matrices is None:
            if self._pose_base is not None:
                self.items = self._pose_base
            self._pose_base = None
            self.pose_matrices = {}
            self.update()
            return
        if self._pose_base is None:
            self.sync()
            self._pose_base = self.items
        self.pose_matrices = matrices
        posed = {}
        for nid,it in self._pose_base.items():
            matrix = matrices.get(nid)
            if matrix is None or np.allclose(matrix, np.eye(4)):
                posed[nid] = it
                continue
            rotation, offset = matrix[:3,:3], matrix[:3,3]
            def points(arr):
                return np.ascontiguousarray(np.asarray(arr).reshape(-1,3) @ rotation.T + offset, dtype=np.float32)
            vertices = points(it.vertices)
            posed[nid] = replace(it, vertices=vertices,
                normals=np.ascontiguousarray(it.normals @ rotation.T, dtype=np.float32),
                vertex_points=points(it.vertex_points), edge_samples=[points(a) for a in it.edge_samples],
                bbox=(tuple(vertices.min(axis=0)),tuple(vertices.max(axis=0))))
        self.items = posed
        self.update()

    def _on_doc_event(self, event: str, payload):
        if event in ("changed", "added", "removed", "moved"):
            if payload:
                self.dirty_nodes.add(payload)
                self.dirty_nodes.update(n.id for n in self.doc.nodes.values() if n.kind == "instance" and n.source == payload)
            else:
                self.dirty_nodes.update(self.doc.nodes.keys())
            self.update()

    def rebuild_item(self, node: Node) -> Optional[RenderItem]:
        mesh = self.doc.mesh_of(node.id)
        if mesh is None or not mesh.vertices:
            return None
        mat = self.doc.materials.get(node.material or "")
        color = node.color or (mat.color if mat else (0.72, 0.72, 0.75))
        cached = self._item_meshes.get(node.id)
        settings = (node.kind, self.show_overhangs, self.overhang_threshold)
        if cached is not None and cached[0] is mesh and cached[1] == settings:
            return replace(cached[2], color=color)
        verts = np.asarray(mesh.vertices, dtype=np.float32)
        norms = np.asarray(mesh.normals, dtype=np.float32) if len(mesh.normals) == len(mesh.vertices) else _face_normals(verts, mesh.triangles)
        idx = np.asarray(mesh.triangles, dtype=np.uint32)
        tf = np.asarray(mesh.triangle_face, dtype=np.int32) if mesh.triangle_face else np.zeros(len(idx), dtype=np.int32)
        edges, edge_ref, samples, vpts = _display_edges(self.doc, node, mesh, verts, idx, tf)
        mat = self.doc.materials.get(node.material or "")
        color = node.color or (mat.color if mat else (0.72, 0.72, 0.75))
        lo, hi = mesh.bounds()
        item = RenderItem(node.id, verts, norms, idx, tf, edges, edge_ref, vpts, color, node.kind, (lo, hi), mesh.face_count, samples)
        if self.show_overhangs:
            item.overhang = _overhang_mask(mesh, self.overhang_threshold)
        self._item_meshes[node.id] = (mesh, settings, item)
        return item

    def sync(self):
        for nid in list(self.dirty_nodes):
            self.items.pop(nid, None)
            node = self.doc.nodes.get(nid)
            if node is not None and node.kind in ("body", "sheet", "curve", "instance", "mesh"):
                if node.kind == "curve":
                    self.items[nid] = _curve_item(self.doc, node)
                else:
                    it = self.rebuild_item(node)
                    if it is not None:
                        self.items[nid] = it
        self.dirty_nodes.clear()
        for nid in list(self.items):
            if nid not in self.doc.nodes:
                del self.items[nid]
        for nid in list(self._item_meshes):
            if nid not in self.doc.nodes:
                del self._item_meshes[nid]
    def scene_bounds(self) -> tuple[Vec3, Vec3]:
        lo, hi = [math.inf] * 3, [-math.inf] * 3
        for it in self.items.values():
            if not self.is_visible(it.node_id):
                continue
            for j in range(3):
                lo[j] = min(lo[j], it.bbox[0][j])
                hi[j] = max(hi[j], it.bbox[1][j])
        if lo[0] is math.inf:
            return (-50.0, -50.0, 0.0), (50.0, 50.0, 50.0)
        return tuple(lo), tuple(hi)

    def focus_all(self):
        self.sync()
        lo, hi = self.scene_bounds()
        self.camera.focus(lo, hi)
        self.update()

    def focus_selection(self):
        ids = self.selection.nodes()
        if not ids:
            return self.focus_all()
        return self.focus_nodes(ids)

    def focus_nodes(self, ids):
        """Frame current geometry, including all descendants of folders."""
        self.sync()
        expanded = set(ids)
        for nid in ids:
            if nid in self.doc.nodes:
                expanded.update(n.id for n in self.doc.walk(nid))
        lo, hi = [math.inf] * 3, [-math.inf] * 3
        for nid in expanded:
            it = self.items.get(nid)
            if it:
                for j in range(3):
                    lo[j] = min(lo[j], it.bbox[0][j])
                    hi[j] = max(hi[j], it.bbox[1][j])
        if math.isfinite(lo[0]):
            self.camera.focus(tuple(lo), tuple(hi))
            self.update()
            return True
        return False

    # ---- GL -------------------------------------------------------------
    def initializeGL(self):
        GL.glEnable(GL.GL_DEPTH_TEST)
        GL.glEnable(GL.GL_NORMALIZE)
        GL.glEnable(GL.GL_MULTISAMPLE)
        GL.glShadeModel(GL.GL_SMOOTH)
        GL.glLightModeli(GL.GL_LIGHT_MODEL_TWO_SIDE, GL.GL_TRUE)
        GL.glEnable(GL.GL_COLOR_MATERIAL)
        GL.glColorMaterial(GL.GL_FRONT_AND_BACK, GL.GL_AMBIENT_AND_DIFFUSE)
        self.matcap_tex = _make_matcap()

    def resizeGL(self, w: int, h: int):
        GL.glViewport(0, 0, max(w, 1), max(h, 1))

    def _apply_camera(self):
        w, h = self.width(), self.height()
        aspect = w / max(h, 1)
        lo, hi = self.scene_bounds()
        extent = max(v_dist(lo, hi), 10.0)
        near = max(0.05, self.camera.distance * 0.002)
        far = self.camera.distance + extent * 4 + 1000.0
        GL.glMatrixMode(GL.GL_PROJECTION)
        GL.glLoadMatrixf(self.camera.projection(aspect, near, far).T.astype(np.float32))
        GL.glMatrixMode(GL.GL_MODELVIEW)
        GL.glLoadMatrixf(self.camera.view().T.astype(np.float32))

    def paintGL(self):
        started = time.perf_counter()
        self.sync()
        if self._pick_request is None and self._hover_ready and self._hover_request is not None:
            self._pick_request, self._hover_request = self._hover_request, None
            self._hover_ready = False
        dpr = self.devicePixelRatioF()
        GL.glViewport(0, 0, int(self.width() * dpr), int(self.height() * dpr))
        if self._pick_request is not None:
            x, y, cb = self._pick_request
            self._pick_request = None
            # The widget's own framebuffer is multisampled, which glReadPixels
            # refuses on some drivers: render the ID pass into a plain FBO.
            from PySide6.QtOpenGL import QOpenGLFramebufferObject

            pw, ph = int(self.width() * dpr), int(self.height() * dpr)
            fbo = getattr(self, "_pick_fbo", None)
            if fbo is None or fbo.width() != pw or fbo.height() != ph:
                fbo = QOpenGLFramebufferObject(pw, ph, QOpenGLFramebufferObject.Depth)
                self._pick_fbo = fbo
            fbo.bind()
            try:
                result = self._pick_pass(int(x * dpr), int((self.height() - y) * dpr))
            except Exception as e:  # never let a pick kill the frame
                result = {"hit": None, "candidates": [], "world": None, "error": str(e)}
            finally:
                fbo.release()
                GL.glBindFramebuffer(GL.GL_FRAMEBUFFER, self.defaultFramebufferObject())
            GL.glViewport(0, 0, pw, ph)
            # Deliver after painting; tools may mutate the model or open widgets.
            generation = self._pick_generation
            QTimer.singleShot(0, lambda r=result, callback=cb, g=generation: callback(r) if g == self._pick_generation else None)
        bg = (0.98, 0.98, 0.99) if self.background_high_contrast else self.background
        GL.glClearColor(*bg, 1.0)
        GL.glClear(GL.GL_COLOR_BUFFER_BIT | GL.GL_DEPTH_BUFFER_BIT)
        self._apply_camera()
        self._setup_lights()
        if self.section_enabled and self.section_plane is not None:
            p = self.section_plane
            n = v_scale(v_unit(p.normal), -1.0)
            GL.glClipPlane(GL.GL_CLIP_PLANE0, [n[0], n[1], n[2], -v_dot(n, p.origin)])
            GL.glEnable(GL.GL_CLIP_PLANE0)
        if self.show_grid:
            self._draw_grid()
        if self.build_plate:
            self._draw_build_plate()
        self._draw_planes()
        self._draw_images()
        self._draw_items()
        self._draw_sketches()
        GL.glDisable(GL.GL_CLIP_PLANE0)
        if self.section_enabled and self.section_plane is not None:
            self._draw_section_outline()
        self._draw_robotics()
        self._draw_temp()
        self._draw_gizmo()
        self._draw_view_cube()
        self.overlay_widget.setGeometry(self.rect())
        self.overlay_widget.raise_()
        self.overlay_widget.update()
        self.frame_ms = (time.perf_counter() - started) * 1000.0
        self._frames += 1
        now = time.time()
        if now - self._fps_t > 1.0:
            self.fps = self._frames / (now - self._fps_t)
            self._frames, self._fps_t = 0, now

    def _setup_lights(self):
        GL.glEnable(GL.GL_LIGHTING)
        if self.display_mode == "render":
            lights = [((0.4, -0.7, 0.9), (1.0, 0.98, 0.95)), ((-0.8, 0.3, 0.4), (0.35, 0.4, 0.5)), ((0.2, 0.9, -0.3), (0.25, 0.22, 0.2))]
        else:
            lights = [((0.3, -0.5, 0.8), (0.9, 0.9, 0.9)), ((-0.6, 0.4, 0.3), (0.35, 0.35, 0.38))]
        for i in range(3):
            GL.glDisable(GL.GL_LIGHT0 + i)
        # Lights fixed to the camera: set with identity modelview.
        GL.glPushMatrix()
        GL.glLoadIdentity()
        for i, (d, c) in enumerate(lights[: self.render_lights]):
            GL.glEnable(GL.GL_LIGHT0 + i)
            GL.glLightfv(GL.GL_LIGHT0 + i, GL.GL_POSITION, [d[0], d[1], d[2], 0.0])
            GL.glLightfv(GL.GL_LIGHT0 + i, GL.GL_DIFFUSE, [c[0], c[1], c[2], 1.0])
            GL.glLightfv(GL.GL_LIGHT0 + i, GL.GL_SPECULAR, [0.3, 0.3, 0.3, 1.0])
        GL.glPopMatrix()
        amb = 0.32 if self.display_mode != "render" else 0.22
        GL.glLightModelfv(GL.GL_LIGHT_MODEL_AMBIENT, [amb, amb, amb, 1.0])
        GL.glMaterialfv(GL.GL_FRONT_AND_BACK, GL.GL_SPECULAR, [0.25, 0.25, 0.25, 1.0])
        GL.glMaterialf(GL.GL_FRONT_AND_BACK, GL.GL_SHININESS, 32.0)

    def _draw_grid(self):
        GL.glDisable(GL.GL_LIGHTING)
        step = self.grid_step
        n = 20
        GL.glLineWidth(1.0)
        GL.glBegin(GL.GL_LINES)
        for i in range(-n, n + 1):
            major = i % 5 == 0
            c = self.grid_color if major else tuple(x * 0.7 for x in self.grid_color)
            if self.background_high_contrast:
                c = (0.55, 0.55, 0.6) if major else (0.8, 0.8, 0.85)
            GL.glColor3f(*c)
            GL.glVertex3f(i * step, -n * step, 0)
            GL.glVertex3f(i * step, n * step, 0)
            GL.glVertex3f(-n * step, i * step, 0)
            GL.glVertex3f(n * step, i * step, 0)
        GL.glEnd()
        GL.glLineWidth(2.0)
        GL.glBegin(GL.GL_LINES)
        GL.glColor3f(0.8, 0.3, 0.3)
        GL.glVertex3f(0, 0, 0)
        GL.glVertex3f(n * step, 0, 0)
        GL.glColor3f(0.3, 0.75, 0.3)
        GL.glVertex3f(0, 0, 0)
        GL.glVertex3f(0, n * step, 0)
        GL.glColor3f(0.3, 0.45, 0.9)
        GL.glVertex3f(0, 0, 0)
        GL.glVertex3f(0, 0, n * step * 0.5)
        GL.glEnd()
        GL.glLineWidth(1.0)
        GL.glEnable(GL.GL_LIGHTING)

    def _draw_build_plate(self):
        w, d = self.build_plate
        GL.glDisable(GL.GL_LIGHTING)
        GL.glEnable(GL.GL_BLEND)
        GL.glBlendFunc(GL.GL_SRC_ALPHA, GL.GL_ONE_MINUS_SRC_ALPHA)
        GL.glColor4f(0.25, 0.3, 0.4, 0.35)
        GL.glBegin(GL.GL_QUADS)
        for x, y in ((-w / 2, -d / 2), (w / 2, -d / 2), (w / 2, d / 2), (-w / 2, d / 2)):
            GL.glVertex3f(x, y, -0.05)
        GL.glEnd()
        GL.glDisable(GL.GL_BLEND)
        GL.glEnable(GL.GL_LIGHTING)

    def _draw_planes(self):
        GL.glDisable(GL.GL_LIGHTING)
        GL.glEnable(GL.GL_BLEND)
        GL.glBlendFunc(GL.GL_SRC_ALPHA, GL.GL_ONE_MINUS_SRC_ALPHA)
        for node in self.doc.nodes.values():
            if node.kind != "plane" or node.plane is None or not self.is_visible(node.id):
                continue
            p = node.plane
            active = self.active_plane is not None and p == self.active_plane
            size = 60.0
            corners = [p.to_world(-size, -size), p.to_world(size, -size), p.to_world(size, size), p.to_world(-size, size)]
            GL.glColor4f(0.3, 0.6, 0.9, 0.18 if active else 0.08)
            GL.glBegin(GL.GL_QUADS)
            for c in corners:
                GL.glVertex3f(*c)
            GL.glEnd()
            GL.glColor4f(0.4, 0.7, 1.0, 0.8 if active else 0.4)
            GL.glBegin(GL.GL_LINE_LOOP)
            for c in corners:
                GL.glVertex3f(*c)
            GL.glEnd()
        GL.glDisable(GL.GL_BLEND)
        GL.glEnable(GL.GL_LIGHTING)

    def _draw_images(self):
        for nid in list(self._image_textures):
            if nid not in self.doc.nodes:
                GL.glDeleteTextures([int(self._image_textures.pop(nid)[1])])
        for node in self.doc.nodes.values():
            if node.kind != "image" or node.image is None or not self.is_visible(node.id):
                continue
            source = node.image.get('data') or node.image.get('path')
            cached = self._image_textures.get(node.id)
            if cached is None or cached[0] != source:
                if cached: GL.glDeleteTextures([int(cached[1])])
                tex = _upload_image(node.image.get("data"), node.image.get("path"))
                self._image_textures[node.id] = (source, tex)
            else:
                tex = cached[1]
            if not tex:
                continue
            p = node.image["plane"]
            w, h = node.image["width"], node.image["height"]
            GL.glDisable(GL.GL_LIGHTING)
            GL.glEnable(GL.GL_TEXTURE_2D)
            GL.glBindTexture(GL.GL_TEXTURE_2D, tex)
            GL.glEnable(GL.GL_BLEND)
            GL.glBlendFunc(GL.GL_SRC_ALPHA, GL.GL_ONE_MINUS_SRC_ALPHA)
            GL.glColor4f(1, 1, 1, node.image.get("opacity", 0.6))
            GL.glBegin(GL.GL_QUADS)
            for (u, v), (s, t) in zip(((0, 0), (w, 0), (w, h), (0, h)), ((0, 1), (1, 1), (1, 0), (0, 0))):
                GL.glTexCoord2f(s, t)
                GL.glVertex3f(*p.to_world(u, v))
            GL.glEnd()
            GL.glDisable(GL.GL_BLEND)
            GL.glDisable(GL.GL_TEXTURE_2D)
            GL.glEnable(GL.GL_LIGHTING)

    def _draw_items(self):
        mode = self.display_mode
        selected_nodes = set(self.selection.nodes())
        sel_faces = set(self.selection.faces())
        sel_edges = set(self.selection.edges())
        sel_vertices = {(n, i) for n, k, i in self.selection.items if k == "vertex"}
        hover = self.hover
        if mode == "matcap" and self.matcap_tex:
            GL.glEnable(GL.GL_TEXTURE_2D)
            GL.glBindTexture(GL.GL_TEXTURE_2D, self.matcap_tex)
            GL.glTexGeni(GL.GL_S, GL.GL_TEXTURE_GEN_MODE, GL.GL_SPHERE_MAP)
            GL.glTexGeni(GL.GL_T, GL.GL_TEXTURE_GEN_MODE, GL.GL_SPHERE_MAP)
            GL.glEnable(GL.GL_TEXTURE_GEN_S)
            GL.glEnable(GL.GL_TEXTURE_GEN_T)
            GL.glDisable(GL.GL_LIGHTING)
        if mode == "render" and self.ground_shadow:
            self._draw_shadows()
        for nid, it in self.items.items():
            if not self.is_visible(nid):
                continue
            node = self.doc.nodes[nid]
            if it.kind == "curve":
                self._draw_curve_item(it, nid in selected_nodes)
                continue
            GL.glEnableClientState(GL.GL_VERTEX_ARRAY)
            GL.glEnableClientState(GL.GL_NORMAL_ARRAY)
            GL.glVertexPointer(3, GL.GL_FLOAT, 0, it.vertices)
            GL.glNormalPointer(GL.GL_FLOAT, 0, it.normals)
            base = it.color
            if nid in selected_nodes and self.selection_mode == "body":
                base = (min(1.0, base[0] * 0.6 + 0.35), min(1.0, base[1] * 0.6 + 0.5), min(1.0, base[2] * 0.6 + 0.9))
            elif hover and hover[0] == nid and hover[1] == "body":
                base = tuple(min(1.0, c * 1.15 + 0.05) for c in base)
            if node.locked:
                base = tuple(0.5 * c + 0.25 for c in base)
            if mode != "wireframe":
                if mode == "xray":
                    GL.glEnable(GL.GL_BLEND)
                    GL.glBlendFunc(GL.GL_SRC_ALPHA, GL.GL_ONE_MINUS_SRC_ALPHA)
                    GL.glDepthMask(GL.GL_FALSE)
                    GL.glColor4f(*base, 0.35)
                else:
                    GL.glColor3f(*base)
                GL.glEnable(GL.GL_POLYGON_OFFSET_FILL)
                GL.glPolygonOffset(1.0, 1.0)
                if self.show_stress and self._stress_colors(it, node) is not None:
                    self._draw_tris_stress(it)
                elif it.overhang is not None and self.show_overhangs:
                    self._draw_tris_colored(it, base)
                else:
                    GL.glDrawElements(GL.GL_TRIANGLES, it.indices.size, GL.GL_UNSIGNED_INT, it.indices)
                # Selected / hovered faces on top.
                highlighted = [(f, (1.0, 0.65, 0.2)) for (n, f) in sel_faces if n == nid]
                if hover and hover[0] == nid and hover[1] == "face":
                    highlighted.append((hover[2], (1.0, 0.85, 0.5)))
                for f, c in highlighted:
                    mask = it.tri_face == f
                    if mask.any():
                        GL.glColor3f(*c)
                        sub = it.indices[mask]
                        GL.glDrawElements(GL.GL_TRIANGLES, sub.size, GL.GL_UNSIGNED_INT, np.ascontiguousarray(sub))
                GL.glDisable(GL.GL_POLYGON_OFFSET_FILL)
                if mode == "xray":
                    GL.glDepthMask(GL.GL_TRUE)
                    GL.glDisable(GL.GL_BLEND)
            if mode in ("shaded_edges", "wireframe", "xray") or nid in selected_nodes or it.kind == "mesh" and mode == "wireframe":
                GL.glDisable(GL.GL_LIGHTING)
                GL.glDisable(GL.GL_TEXTURE_2D) if mode == "matcap" else None
                GL.glColor3f(0.08, 0.08, 0.1) if not self.background_high_contrast else GL.glColor3f(0.0, 0.0, 0.0)
                if mode == "wireframe":
                    GL.glColor3f(*tuple(0.6 * c + 0.3 for c in base))
                GL.glLineWidth(1.2)
                if it.edges.size and it.kind != "mesh":
                    GL.glDisableClientState(GL.GL_NORMAL_ARRAY)
                    GL.glDrawElements(GL.GL_LINES, it.edges.size, GL.GL_UNSIGNED_INT, it.edges)
                elif mode == "wireframe":
                    GL.glPolygonMode(GL.GL_FRONT_AND_BACK, GL.GL_LINE)
                    GL.glDrawElements(GL.GL_TRIANGLES, it.indices.size, GL.GL_UNSIGNED_INT, it.indices)
                    GL.glPolygonMode(GL.GL_FRONT_AND_BACK, GL.GL_FILL)
                GL.glEnable(GL.GL_LIGHTING)
                if mode == "matcap":
                    GL.glEnable(GL.GL_TEXTURE_2D)
                    GL.glDisable(GL.GL_LIGHTING)
            # selected / hovered edges and vertices
            GL.glDisable(GL.GL_LIGHTING)
            GL.glDisable(GL.GL_DEPTH_TEST)
            for (n, e) in sel_edges:
                if n == nid and e < len(it.edge_samples):
                    self._draw_polyline(it.edge_samples[e], (1.0, 0.65, 0.2), 3.0)
            if hover and hover[0] == nid and hover[1] == "edge" and hover[2] < len(it.edge_samples):
                self._draw_polyline(it.edge_samples[hover[2]], (1.0, 0.85, 0.5), 3.0)
            if self.selection_mode == "vertex" and it.vertex_points.size:
                GL.glPointSize(6.0)
                GL.glColor3f(0.9, 0.9, 0.95)
                GL.glBegin(GL.GL_POINTS)
                for i, p in enumerate(it.vertex_points):
                    if (nid, i) in sel_vertices:
                        continue
                    GL.glVertex3f(*p)
                GL.glEnd()
            for (n, i) in sel_vertices:
                if n == nid and i < len(it.vertex_points):
                    GL.glPointSize(10.0)
                    GL.glColor3f(1.0, 0.65, 0.2)
                    GL.glBegin(GL.GL_POINTS)
                    GL.glVertex3f(*it.vertex_points[i])
                    GL.glEnd()
            GL.glEnable(GL.GL_DEPTH_TEST)
            GL.glEnable(GL.GL_LIGHTING)
            GL.glDisableClientState(GL.GL_VERTEX_ARRAY)
            GL.glDisableClientState(GL.GL_NORMAL_ARRAY)
        if mode == "matcap":
            GL.glDisable(GL.GL_TEXTURE_GEN_S)
            GL.glDisable(GL.GL_TEXTURE_GEN_T)
            GL.glDisable(GL.GL_TEXTURE_2D)
            GL.glEnable(GL.GL_LIGHTING)

    def _draw_tris_colored(self, it: RenderItem, base):
        GL.glDisableClientState(GL.GL_NORMAL_ARRAY)
        GL.glBegin(GL.GL_TRIANGLES)
        for ti, (a, b, c) in enumerate(it.indices):
            col = (0.9, 0.35, 0.3) if it.overhang[ti] else base
            GL.glColor3f(*col)
            for v in (a, b, c):
                GL.glNormal3f(*it.normals[v])
                GL.glVertex3f(*it.vertices[v])
        GL.glEnd()
        GL.glEnableClientState(GL.GL_NORMAL_ARRAY)

    def _stress_colors(self, it: RenderItem, node) -> Optional[np.ndarray]:
        """Per-vertex colours from the node's results hotspot (blue = 0, red =
        yield); None when the node carries no stress field."""
        if it.stress_colors is not None:
            return it.stress_colors
        r = node.results if node.results and node.results.get("section") == "links" else None
        hot = (r or {}).get("hotspot") or {}
        cells, stress = hot.get("cells"), hot.get("stress_pa")
        if not cells or not stress:
            return None
        try:
            from scipy.spatial import cKDTree
        except Exception:
            return None
        com = r.get("com")
        if com is None:
            body = self.doc.resolved_body(node.id)
            com = list(self.doc.kernel.mass_properties(body).centroid) if body is not None else [0.0, 0.0, 0.0]
            com = [c * 1e-3 for c in com]
        pts = np.asarray(cells, dtype=float) * 1e3 + np.asarray(com, dtype=float) * 1e3  # link frame m → world mm
        _, idx = cKDTree(pts).query(np.asarray(it.vertices, dtype=float))
        val = np.asarray(stress, dtype=float)[idx]
        mat = self.doc.materials.get(node.material or "")
        yield_pa = (mat.props().get("yield_strength") if mat else None) or max(float(val.max()), 1.0)
        t = np.clip(val / yield_pa, 0.0, 1.0)
        # blue → cyan → green → yellow → red
        r_ = np.clip(1.5 * t - 0.5, 0, 1)
        g_ = np.clip(1.0 - abs(2.0 * t - 1.0) * 1.2 + 0.2, 0, 1)
        b_ = np.clip(1.0 - 2.0 * t, 0, 1)
        it.stress_colors = np.stack([r_, g_, b_], axis=1).astype(np.float32)
        return it.stress_colors

    def _draw_tris_stress(self, it: RenderItem):
        GL.glEnableClientState(GL.GL_COLOR_ARRAY)
        GL.glColorPointer(3, GL.GL_FLOAT, 0, it.stress_colors)
        GL.glDrawElements(GL.GL_TRIANGLES, it.indices.size, GL.GL_UNSIGNED_INT, it.indices)
        GL.glDisableClientState(GL.GL_COLOR_ARRAY)

    def clear_stress_colors(self):
        for it in self.items.values():
            it.stress_colors = None
        self.update()

    def _draw_shadows(self):
        """Planar projected shadow onto z=0 (or the build plate)."""
        light = (0.4, -0.7, 0.9)
        GL.glDisable(GL.GL_LIGHTING)
        GL.glEnable(GL.GL_BLEND)
        GL.glBlendFunc(GL.GL_SRC_ALPHA, GL.GL_ONE_MINUS_SRC_ALPHA)
        GL.glDepthMask(GL.GL_FALSE)
        GL.glColor4f(0.0, 0.0, 0.0, 0.28)
        GL.glPushMatrix()
        lx, ly, lz = light
        m = np.array([[lz, 0, -lx, 0], [0, lz, -ly, 0], [0, 0, 0.0001, 0], [0, 0, 0, lz]], dtype=np.float32)
        GL.glMultMatrixf(m.T)
        for nid, it in self.items.items():
            if it.kind == "curve" or not self.is_visible(nid):
                continue
            GL.glEnableClientState(GL.GL_VERTEX_ARRAY)
            GL.glVertexPointer(3, GL.GL_FLOAT, 0, it.vertices)
            GL.glDrawElements(GL.GL_TRIANGLES, it.indices.size, GL.GL_UNSIGNED_INT, it.indices)
            GL.glDisableClientState(GL.GL_VERTEX_ARRAY)
        GL.glPopMatrix()
        GL.glDepthMask(GL.GL_TRUE)
        GL.glDisable(GL.GL_BLEND)
        GL.glEnable(GL.GL_LIGHTING)

    def _draw_curve_item(self, it: RenderItem, selected: bool):
        GL.glDisable(GL.GL_LIGHTING)
        color = (1.0, 0.65, 0.2) if selected else it.color
        for seg in it.edge_samples:
            self._draw_polyline(seg, color, 2.0)
        GL.glEnable(GL.GL_LIGHTING)

    def _draw_polyline(self, pts, color, width):
        GL.glLineWidth(width)
        GL.glColor3f(*color)
        GL.glBegin(GL.GL_LINE_STRIP)
        for p in pts:
            GL.glVertex3f(*p)
        GL.glEnd()
        GL.glLineWidth(1.0)

    def _draw_sketches(self):
        GL.glDisable(GL.GL_LIGHTING)
        GL.glDisable(GL.GL_DEPTH_TEST)
        selected = set(self.selection.nodes())
        for node in self.doc.nodes.values():
            if node.kind != "sketch" or node.sketch is None or not self.is_visible(node.id):
                continue
            sk = node.sketch
            for ci, c in enumerate(sk.curves):
                pts = c.sample(48)
                if c.kind == "slot":
                    from ..io.exporters import _slot_points

                    pts = _slot_points(c) + [_slot_points(c)[0]]
                color = (1.0, 0.65, 0.2) if node.id in selected else (0.35, 0.8, 1.0)
                self._draw_polyline([sk.world(p) for p in pts], color, 2.0)
        GL.glEnable(GL.GL_DEPTH_TEST)
        GL.glEnable(GL.GL_LIGHTING)

    def _draw_section_outline(self):
        # Exact OCCT sectioning can take minutes on an imported assembly and
        # must never run inside paintGL. Reuse the display tessellation instead.
        visible = {nid: it for nid, it in self.items.items()
                   if self.is_visible(nid) and it.kind != "curve"}
        segments = self._section_preview.segments(visible, self.section_plane)
        GL.glDisable(GL.GL_LIGHTING)
        GL.glColor3f(1.0, 0.4, 0.3)
        GL.glLineWidth(2.5)
        GL.glEnableClientState(GL.GL_VERTEX_ARRAY)
        for points in segments.values():
            if len(points):
                GL.glVertexPointer(3, GL.GL_FLOAT, 0, points)
                GL.glDrawArrays(GL.GL_LINES, 0, len(points))
        GL.glDisableClientState(GL.GL_VERTEX_ARRAY)
        # The section plane as a translucent quad with a handle.
        p = self.section_plane
        lo, hi = self.scene_bounds()
        size = max(v_dist(lo, hi), 20.0) * 0.6
        GL.glEnable(GL.GL_BLEND)
        GL.glBlendFunc(GL.GL_SRC_ALPHA, GL.GL_ONE_MINUS_SRC_ALPHA)
        GL.glColor4f(1.0, 0.4, 0.3, 0.08)
        GL.glBegin(GL.GL_QUADS)
        for u, v in ((-size, -size), (size, -size), (size, size), (-size, size)):
            GL.glVertex3f(*p.to_world(u, v))
        GL.glEnd()
        GL.glDisable(GL.GL_BLEND)
        GL.glEnable(GL.GL_LIGHTING)

    def _draw_robotics(self):
        """Joint glyphs and motor shaft axes."""
        from ..robotics import joint_glyph

        shapes = []
        selected = set(self.selection.nodes())
        for node in self.doc.nodes.values():
            if node.kind == "joint" and node.joint is not None and self.is_visible(node.id):
                size = self.camera.world_per_pixel(self.height()) * (26 if node.id in selected else 18)
                j = node.joint
                matrix = self.pose_matrices.get(j.parent)
                if matrix is not None:
                    j = replace(j, pivot=self.pose_point(j.parent, j.pivot), axis=tuple(matrix[:3,:3] @ np.asarray(j.axis)))
                shapes.extend(joint_glyph(j, size))
            elif node.robot and node.robot.get("kind") == "motor" and self.is_visible(node.id):
                a, b = tuple(node.robot["mount_point"]), tuple(node.robot["shaft_tip"])
                a, b = self.pose_point(node.id,a), self.pose_point(node.id,b)
                shapes.append(("line", (a, b, (1.0, 0.4, 0.2))))
            elif node.kind == "sensor" and node.robot and self.is_visible(node.id):
                p = tuple(node.robot["point"])
                body_id = node.robot.get('body')
                p = self.pose_point(body_id,p)
                size = self.camera.world_per_pixel(self.height()) * (22 if node.id in selected else 14)
                axes = node.robot.get("axes") or [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
                matrix = self.pose_matrices.get(body_id)
                if matrix is not None:
                    axes = [matrix[:3,:3] @ np.asarray(ax) for ax in axes]
                for ax, col in zip(axes, ((1.0, 0.3, 0.3), (0.3, 1.0, 0.3), (0.3, 0.5, 1.0))):
                    shapes.append(("line", (p, (p[0] + ax[0] * size, p[1] + ax[1] * size, p[2] + ax[2] * size), col)))
                shapes.append(("point", (p, (0.9, 0.9, 0.3), 8.0)))
            elif node.kind == "cable" and node.robot and self.is_visible(node.id):
                a, b = tuple(node.robot["from_point"]), tuple(node.robot["to_point"])
                a = self.pose_point(node.robot.get('from_body'),a)
                b = self.pose_point(node.robot.get('to_body'),b)
                # A sagging arc so a slack cable reads as one.
                mid = tuple(0.5 * (a[i] + b[i]) for i in range(3))
                drop = 0.15 * math.dist(a, b)
                pts = []
                for k in range(13):
                    t = k / 12.0
                    q = [(1 - t) ** 2 * a[i] + 2 * (1 - t) * t * mid[i] + t * t * b[i] for i in range(3)]
                    q[2] -= drop * 4 * t * (1 - t)
                    pts.append(tuple(q))
                shapes.append(("poly", (pts, (0.95, 0.6, 0.2) if node.id in selected else (0.8, 0.45, 0.15))))
        if not shapes:
            return
        keep = self.temp_shapes
        self.temp_shapes = shapes
        self._draw_temp()
        self.temp_shapes = keep

    def _draw_temp(self):
        GL.glDisable(GL.GL_LIGHTING)
        GL.glDisable(GL.GL_DEPTH_TEST)
        for kind, data in self.temp_shapes:
            if kind == "line":
                a, b, color = data
                self._draw_polyline([a, b], color, 2.0)
            elif kind == "poly":
                pts, color = data
                self._draw_polyline(pts, color, 2.0)
            elif kind == "point":
                p, color, size = data
                GL.glPointSize(size)
                GL.glColor3f(*color)
                GL.glBegin(GL.GL_POINTS)
                GL.glVertex3f(*p)
                GL.glEnd()
            elif kind == "mesh":
                it = data
                GL.glEnable(GL.GL_DEPTH_TEST)
                GL.glEnable(GL.GL_LIGHTING)
                GL.glEnable(GL.GL_BLEND)
                GL.glBlendFunc(GL.GL_SRC_ALPHA, GL.GL_ONE_MINUS_SRC_ALPHA)
                GL.glColor4f(0.4, 0.8, 1.0, 0.45)
                GL.glEnableClientState(GL.GL_VERTEX_ARRAY)
                GL.glEnableClientState(GL.GL_NORMAL_ARRAY)
                GL.glVertexPointer(3, GL.GL_FLOAT, 0, it.vertices)
                GL.glNormalPointer(GL.GL_FLOAT, 0, it.normals)
                GL.glDrawElements(GL.GL_TRIANGLES, it.indices.size, GL.GL_UNSIGNED_INT, it.indices)
                GL.glDisableClientState(GL.GL_VERTEX_ARRAY)
                GL.glDisableClientState(GL.GL_NORMAL_ARRAY)
                GL.glDisable(GL.GL_BLEND)
                GL.glDisable(GL.GL_LIGHTING)
                GL.glDisable(GL.GL_DEPTH_TEST)
        GL.glEnable(GL.GL_DEPTH_TEST)
        GL.glEnable(GL.GL_LIGHTING)

    def _draw_gizmo(self):
        g = self.gizmo
        if not g:
            return
        o = g["origin"]
        size = self.camera.world_per_pixel(self.height()) * 90
        GL.glDisable(GL.GL_LIGHTING)
        GL.glDisable(GL.GL_DEPTH_TEST)
        for i, (axis, color) in enumerate(g["axes"]):
            c = (1.0, 0.9, 0.3) if self.gizmo_hit == i else color
            GL.glLineWidth(4.0 if self.gizmo_hit == i else 2.5)
            if g.get("mode") == "rotate":
                # ring perpendicular to the axis
                a = v_unit(axis)
                helper = (0, 0, 1) if abs(a[2]) < 0.9 else (1, 0, 0)
                u = v_unit(v_cross(helper, a))
                v = v_cross(a, u)
                GL.glColor3f(*c)
                GL.glBegin(GL.GL_LINE_LOOP)
                for k in range(48):
                    t = 2 * math.pi * k / 48
                    GL.glVertex3f(*v_add(o, v_add(v_scale(u, size * math.cos(t)), v_scale(v, size * math.sin(t)))))
                GL.glEnd()
            else:
                tip = v_add(o, v_scale(v_unit(axis), size))
                self._draw_polyline([o, tip], c, 3.0)
                GL.glPointSize(10.0 if g.get("mode") == "scale" else 7.0)
                GL.glColor3f(*c)
                GL.glBegin(GL.GL_POINTS)
                GL.glVertex3f(*tip)
                GL.glEnd()
        # free-move handle at the origin
        GL.glPointSize(9.0)
        GL.glColor3f(0.95, 0.95, 0.95) if self.gizmo_hit != 3 else GL.glColor3f(1.0, 0.9, 0.3)
        GL.glBegin(GL.GL_POINTS)
        GL.glVertex3f(*o)
        GL.glEnd()
        GL.glLineWidth(1.0)
        GL.glEnable(GL.GL_DEPTH_TEST)
        GL.glEnable(GL.GL_LIGHTING)

    def gizmo_hit_test(self, px: float, py: float) -> Optional[int]:
        """Which gizmo handle is under the pixel (0/1/2 axes, 3 = free)."""
        g = self.gizmo
        if not g:
            return None
        o = g["origin"]
        w, h = self.width(), self.height()
        size = self.camera.world_per_pixel(h) * 90
        po = self.camera.project(o, w, h)
        if po and math.hypot(po[0] - px, po[1] - py) < 10:
            return 3
        best, best_d = None, 14.0
        for i, (axis, _) in enumerate(g["axes"]):
            if g.get("mode") == "rotate":
                a = v_unit(axis)
                helper = (0, 0, 1) if abs(a[2]) < 0.9 else (1, 0, 0)
                u = v_unit(v_cross(helper, a))
                v = v_cross(a, u)
                for k in range(48):
                    t = 2 * math.pi * k / 48
                    p = self.camera.project(v_add(o, v_add(v_scale(u, size * math.cos(t)), v_scale(v, size * math.sin(t)))), w, h)
                    if p:
                        d = math.hypot(p[0] - px, p[1] - py)
                        if d < best_d:
                            best, best_d = i, d
            else:
                tip = self.camera.project(v_add(o, v_scale(v_unit(axis), size)), w, h)
                if po and tip:
                    d = _seg_dist_2d((po[0], po[1]), (tip[0], tip[1]), (px, py))
                    if d < best_d:
                        best, best_d = i, d
        return best

    def _draw_view_cube(self):
        """A small axis cube in the corner drawn with its own projection."""
        w, h = self.width(), self.height()
        dpr = self.devicePixelRatioF()
        size = 84
        GL.glViewport(int((w - size - 12) * dpr), int((h - size - 12) * dpr), int(size * dpr), int(size * dpr))
        GL.glMatrixMode(GL.GL_PROJECTION)
        GL.glPushMatrix()
        GL.glLoadIdentity()
        GL.glOrtho(-1.6, 1.6, -1.6, 1.6, -10, 10)
        GL.glMatrixMode(GL.GL_MODELVIEW)
        GL.glPushMatrix()
        v = self.camera.view()
        v[:3, 3] = 0
        GL.glLoadMatrixf(v.T.astype(np.float32))
        GL.glClear(GL.GL_DEPTH_BUFFER_BIT)
        GL.glDisable(GL.GL_LIGHTING)
        faces = [((1, 0, 0), (0.85, 0.4, 0.4)), ((-1, 0, 0), (0.6, 0.3, 0.3)), ((0, 1, 0), (0.4, 0.8, 0.4)), ((0, -1, 0), (0.3, 0.55, 0.3)), ((0, 0, 1), (0.4, 0.55, 0.95)), ((0, 0, -1), (0.3, 0.4, 0.65))]
        for n, c in faces:
            a = np.array(n, dtype=float)
            helper = np.array([0, 0, 1.0]) if abs(a[2]) < 0.9 else np.array([1.0, 0, 0])
            u = np.cross(helper, a)
            u /= np.linalg.norm(u)
            vv = np.cross(a, u)
            GL.glColor3f(*c)
            GL.glBegin(GL.GL_QUADS)
            for s, t in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                p = a + u * s + vv * t
                GL.glVertex3f(*p)
            GL.glEnd()
        GL.glColor3f(0.1, 0.1, 0.12)
        GL.glLineWidth(1.5)
        GL.glBegin(GL.GL_LINES)
        for x in (-1, 1):
            for y in (-1, 1):
                GL.glVertex3f(x, y, -1)
                GL.glVertex3f(x, y, 1)
                GL.glVertex3f(x, -1, y)
                GL.glVertex3f(x, 1, y)
                GL.glVertex3f(-1, x, y)
                GL.glVertex3f(1, x, y)
        GL.glEnd()
        GL.glEnable(GL.GL_LIGHTING)
        GL.glPopMatrix()
        GL.glMatrixMode(GL.GL_PROJECTION)
        GL.glPopMatrix()
        GL.glMatrixMode(GL.GL_MODELVIEW)
        GL.glViewport(0, 0, int(w * dpr), int(h * dpr))

    def view_cube_hit(self, px: float, py: float) -> Optional[str]:
        w, h = self.width(), self.height()
        size = 84
        if not (w - size - 12 <= px <= w - 12 and 12 <= py <= size + 12):
            return None
        # Which face is most toward the camera at that pixel: use the view
        # direction and the pixel offset from the cube centre.
        right, up, back = self.camera.basis()
        cx, cy = w - 12 - size / 2, 12 + size / 2
        dx, dy = (px - cx) / (size / 2), -(py - cy) / (size / 2)
        d = v_add(v_add(v_scale(right, dx), v_scale(up, dy)), v_scale(back, 0.9))
        names = {(1, 0, 0): "right", (-1, 0, 0): "left", (0, 1, 0): "back", (0, -1, 0): "front", (0, 0, 1): "top", (0, 0, -1): "bottom"}
        best = max(names, key=lambda n: v_dot(n, d))
        return names[best]

    def _draw_overlay_text(self, target):
        painter = QPainter(target)
        painter.setRenderHint(QPainter.Antialiasing)
        w, h = self.width(), self.height()
        light = self.background_high_contrast
        painter.setPen(QPen(QColor(20, 20, 20) if light else QColor(220, 220, 225)))
        painter.setFont(QFont("Helvetica", 10))
        painter.fillRect(12, 12, min(w - 24, 650), 54, QColor(20, 29, 39, 230))
        painter.setPen(QColor("#7ed0ef"))
        painter.setFont(QFont("Helvetica", 11, QFont.Bold))
        painter.drawText(24, 33, f"{self.tool_name}  ·  {self.selection_mode.title()}")
        painter.setFont(QFont("Helvetica", 9))
        painter.setPen(QColor("#d4dce5"))
        hint = painter.fontMetrics().elidedText(self.tool_hint, Qt.ElideRight, max(40, min(w - 48, 626)))
        painter.drawText(24, 54, hint)
        for p, text in self.annotations:
            sp = self.camera.project(p, w, h)
            if sp:
                painter.fillRect(int(sp[0]) + 6, int(sp[1]) - 16, 8 + 6 * len(text), 18, QColor(30, 30, 34, 200) if not light else QColor(255, 255, 255, 220))
                painter.drawText(int(sp[0]) + 10, int(sp[1]) - 3, text)
        for cb in list(self.overlays):
            cb(painter)
        painter.setPen(QPen(QColor(150, 150, 160)))
        painter.setFont(QFont("Helvetica", 9))
        painter.drawText(8, h - 8, f"Right-drag orbit · Shift+right-drag pan · Wheel zoom · F focus  |  {self.frame_ms:.0f} ms/frame  {self.stats_text}")
        painter.end()

    # ---- picking ------------------------------------------------------------
    def _hover_due(self):
        self._hover_ready = True
        self.update()

    def cancel_picks(self):
        self._pick_generation += 1
        self._pick_request = self._hover_request = None
        self._hover_ready = False
        self._hover_timer.stop()

    def request_hover(self, x: float, y: float, callback: Callable):
        # Coalesce high-rate mouse movement and never overwrite a click pick.
        self._hover_request = (x, y, callback)
        if not self._hover_timer.isActive():
            self._hover_timer.start(33)

    def request_pick(self, x: float, y: float, callback: Callable):
        self._hover_request = None
        self._hover_ready = False
        self._hover_timer.stop()
        self._pick_request = (x, y, callback)
        self.update()

    def _pick_pass(self, x: int, y: int):
        """Render IDs into the back buffer and read a neighbourhood.
        Returns (kind, node_id, index) or None; also candidates for
        disambiguation."""
        GL.glClearColor(0, 0, 0, 1)
        GL.glClear(GL.GL_COLOR_BUFFER_BIT | GL.GL_DEPTH_BUFFER_BIT)
        self._apply_camera()
        GL.glDisable(GL.GL_LIGHTING)
        GL.glDisable(GL.GL_MULTISAMPLE)
        GL.glDisable(GL.GL_BLEND)
        GL.glDisable(GL.GL_TEXTURE_2D)
        if self.section_enabled and self.section_plane is not None:
            GL.glEnable(GL.GL_CLIP_PLANE0)
        node_ids = [nid for nid in self.items if self.is_visible(nid) and not self.doc.nodes[nid].locked]
        # Encoding: R = node index+1 (up to 255), G,B = sub index+1 (16 bits); vertices/edges use kind bits via separate passes.
        mode = self.selection_mode
        for ni, nid in enumerate(node_ids):
            it = self.items[nid]
            if it.kind == "curve":
                for ei, seg in enumerate(it.edge_samples):
                    GL.glColor3ub(ni + 1, (ei + 1) & 0xFF, ((ei + 1) >> 8) & 0xFF)
                    GL.glLineWidth(8.0)
                    GL.glBegin(GL.GL_LINE_STRIP)
                    for p in seg:
                        GL.glVertex3f(*p)
                    GL.glEnd()
                continue
            GL.glEnableClientState(GL.GL_VERTEX_ARRAY)
            GL.glVertexPointer(3, GL.GL_FLOAT, 0, it.vertices)
            if mode in ("body", "face", "point") or it.kind == "mesh":
                if mode == "body" or it.kind == "mesh":
                    GL.glColor3ub(ni + 1, 1, 0)
                    GL.glDrawElements(GL.GL_TRIANGLES, it.indices.size, GL.GL_UNSIGNED_INT, it.indices)
                else:
                    for f in range(it.face_count):
                        mask = it.tri_face == f
                        if not mask.any():
                            continue
                        GL.glColor3ub(ni + 1, (f + 1) & 0xFF, ((f + 1) >> 8) & 0xFF)
                        sub = np.ascontiguousarray(it.indices[mask])
                        GL.glDrawElements(GL.GL_TRIANGLES, sub.size, GL.GL_UNSIGNED_INT, sub)
            else:
                # occluders in black so hidden edges/vertices are not picked
                GL.glColor3ub(0, 0, 0)
                GL.glEnable(GL.GL_POLYGON_OFFSET_FILL)
                GL.glPolygonOffset(2.0, 2.0)
                GL.glDrawElements(GL.GL_TRIANGLES, it.indices.size, GL.GL_UNSIGNED_INT, it.indices)
                GL.glDisable(GL.GL_POLYGON_OFFSET_FILL)
            GL.glDisableClientState(GL.GL_VERTEX_ARRAY)
            if mode == "edge":
                GL.glLineWidth(7.0)
                for ei, seg in enumerate(it.edge_samples):
                    GL.glColor3ub(ni + 1, (ei + 1) & 0xFF, ((ei + 1) >> 8) & 0xFF)
                    GL.glBegin(GL.GL_LINE_STRIP)
                    for p in seg:
                        GL.glVertex3f(*p)
                    GL.glEnd()
                GL.glLineWidth(1.0)
            elif mode == "vertex":
                GL.glPointSize(12.0)
                GL.glBegin(GL.GL_POINTS)
                for vi, p in enumerate(it.vertex_points):
                    GL.glColor3ub(ni + 1, (vi + 1) & 0xFF, ((vi + 1) >> 8) & 0xFF)
                    GL.glVertex3f(*p)
                GL.glEnd()
        GL.glDisable(GL.GL_CLIP_PLANE0)
        GL.glFlush()
        r = 3
        data = GL.glReadPixels(max(x - r, 0), max(y - r, 0), 2 * r + 1, 2 * r + 1, GL.GL_RGB, GL.GL_UNSIGNED_BYTE)
        arr = np.frombuffer(data, dtype=np.uint8).reshape(2 * r + 1, 2 * r + 1, 3)
        depth = GL.glReadPixels(x, y, 1, 1, GL.GL_DEPTH_COMPONENT, GL.GL_FLOAT)
        GL.glEnable(GL.GL_MULTISAMPLE)
        GL.glEnable(GL.GL_LIGHTING)
        candidates = []
        order = sorted(((abs(i - r) + abs(j - r), i, j) for i in range(2 * r + 1) for j in range(2 * r + 1)))
        for _, i, j in order:
            R, G, B = (int(v) for v in arr[i, j])
            if R == 0:
                continue
            ni = R - 1
            if ni >= len(node_ids):
                continue
            sub = (G | (B << 8)) - 1
            nid = node_ids[ni]
            kind = "body" if (mode == "body" or self.items[nid].kind == "mesh") else ("curve" if self.items[nid].kind == "curve" else mode)
            cand = (kind, nid, sub)
            if cand not in candidates:
                candidates.append(cand)
        world = None
        d = float(np.frombuffer(depth, dtype=np.float32)[0]) if depth is not None else 1.0
        if d < 1.0:
            world = self._unproject(x, y, d)
        return {"hit": candidates[0] if candidates else None, "candidates": candidates, "world": world}

    def _unproject(self, x: int, y: int, depth: float) -> Vec3:
        dpr = self.devicePixelRatioF()
        w, h = self.width(), self.height()
        origin, d = self.camera.ray(x / dpr, h - y / dpr, w, h)
        aspect = w / max(h, 1)
        lo, hi = self.scene_bounds()
        extent = max(v_dist(lo, hi), 10.0)
        near = max(0.05, self.camera.distance * 0.002)
        far = self.camera.distance + extent * 4 + 1000.0
        if self.camera.orthographic:
            z = near + depth * (far - near)
            # ortho depth is linear
            z_ndc = depth * 2 - 1
            z = (z_ndc * (far - near) + (far + near)) / 2
            return v_add(origin, v_scale(d, z))
        z_ndc = depth * 2 - 1
        z = 2 * far * near / (far + near - z_ndc * (far - near))
        _, _, back = self.camera.basis()
        cosang = -v_dot(d, back)
        return v_add(origin, v_scale(d, z / max(cosang, 1e-6)))

    # ---- snapping ---------------------------------------------------------------
    def snap(self, px: float, py: float, suppress: bool = False, want_plane: Optional[Plane] = None) -> SnapResult:
        """Best snap under the cursor: vertices, midpoints, centres, grid,
        mesh vertices, then a face hit or the active plane, then free space."""
        w, h = self.width(), self.height()
        origin, d = self.camera.ray(px, py, w, h)
        plane = want_plane or (self.active_plane if self.plane_snapping else None)
        best: Optional[SnapResult] = None
        best_d = self.snap_pixels
        if self.snapping and not suppress:
            for nid, it in self.items.items():
                if not self.is_visible(nid):
                    continue
                cands = []
                for p in it.vertex_points:
                    cands.append((tuple(map(float, p)), "vertex"))
                for seg in it.edge_samples:
                    if len(seg) >= 2:
                        cands.append((tuple(map(float, seg[len(seg) // 2])), "midpoint"))
                node = self.doc.nodes.get(nid)
                for c in getattr(it, "centers", []):
                    cands.append((c, "center"))
                for p, kind in cands:
                    sp = self.camera.project(p, w, h)
                    if sp is None:
                        continue
                    dist = math.hypot(sp[0] - px, sp[1] - py)
                    if dist < best_d:
                        best, best_d = SnapResult(p, kind, nid), dist
            # sketch endpoints / centres
            for node in self.doc.nodes.values():
                if node.kind == "sketch" and node.sketch and self.is_visible(node.id):
                    for c in node.sketch.curves:
                        pts = []
                        if c.kind in ("line", "polyline", "spline", "control"):
                            pts = [(p, "endpoint") for p in c.points]
                        if c.center:
                            pts.append((c.center, "center"))
                        for p2, kind in pts:
                            p = node.sketch.world(p2)
                            sp = self.camera.project(p, w, h)
                            if sp and math.hypot(sp[0] - px, sp[1] - py) < best_d:
                                best, best_d = SnapResult(p, kind, node.id), math.hypot(sp[0] - px, sp[1] - py)
            if best is not None and plane is not None:
                best = SnapResult(plane.project(best.point), best.kind, best.node_id)
            if best is not None:
                return best
        if plane is not None:
            hit = _ray_plane(origin, d, plane)
            if hit is not None:
                if self.snapping and not suppress and self.show_grid:
                    u, v, _ = plane.to_local(hit)
                    gu, gv = round(u / self.grid_step) * self.grid_step, round(v / self.grid_step) * self.grid_step
                    gp = plane.to_world(gu, gv)
                    sp = self.camera.project(gp, w, h)
                    if sp and math.hypot(sp[0] - px, sp[1] - py) < self.snap_pixels:
                        return SnapResult(gp, "grid")
                return SnapResult(hit, "plane")
        # ground plane fallback
        hit = _ray_plane(origin, d, Plane.xy())
        if hit is not None and not self.camera.orthographic or hit is not None:
            if self.snapping and not suppress and self.show_grid:
                gp = (round(hit[0] / self.grid_step) * self.grid_step, round(hit[1] / self.grid_step) * self.grid_step, 0.0)
                sp = self.camera.project(gp, w, h)
                if sp and math.hypot(sp[0] - px, sp[1] - py) < self.snap_pixels:
                    return SnapResult(gp, "grid")
            return SnapResult(hit, "free")
        return SnapResult(v_add(origin, v_scale(d, self.camera.distance)), "free")

    def world_on_plane(self, px: float, py: float, plane: Plane) -> Optional[Vec3]:
        origin, d = self.camera.ray(px, py, self.width(), self.height())
        return _ray_plane(origin, d, plane)

    def screen_plane(self, through: Vec3) -> Plane:
        """A plane facing the camera through a point (free-style dragging)."""
        _, _, back = self.camera.basis()
        return Plane.from_normal(through, back)

    # ---- mouse / keys --------------------------------------------------------------
    def mousePressEvent(self, e: QMouseEvent):
        self._last_mouse = e.position().toPoint()
        self._drag_button = e.button()
        self.setFocus()
        if e.button() == Qt.LeftButton:
            self._consume_left = False
            pin = self.comment_hit(e.position().x(), e.position().y())
            if pin:
                self._consume_left = True
                self.comment_clicked.emit(pin)
                return
            cube = self.view_cube_hit(e.position().x(), e.position().y())
            if cube:
                self._consume_left = True
                current = {"front": (-90.0, 0.0), "back": (90.0, 0.0), "right": (0.0, 0.0), "left": (180.0, 0.0), "top": (-90.0, 89.5), "bottom": (-90.0, -89.5)}[cube]
                if abs(self.camera.yaw - current[0]) < 1e-6 and abs(self.camera.pitch - current[1]) < 1e-6:
                    self.camera.opposite()
                else:
                    self.camera.set_view(cube)
                self.view_changed.emit()
                self.update()
                return
            hit = self.gizmo_hit_test(e.position().x(), e.position().y())
            if hit is not None:
                self.gizmo_hit = hit
                self.setCursor(Qt.SizeAllCursor)
                self.dragged.emit(("start", None, hit, e.modifiers(), e.position()))
                return
            self.dragged.emit(("press", None, None, e.modifiers(), e.position()))
        elif e.button() == Qt.RightButton:
            self.setCursor(Qt.ClosedHandCursor)
            self._right_press = e.position().toPoint()

    def mouseMoveEvent(self, e: QMouseEvent):
        pos = e.position().toPoint()
        dx, dy = pos.x() - self._last_mouse.x(), pos.y() - self._last_mouse.y()
        buttons = e.buttons()
        mods = e.modifiers()
        # Orbit: right-drag (two-finger drag on a trackpad), Alt+left-drag, or
        # Shift+middle-drag; pan: middle-drag or Shift+right-drag; hold Alt
        # while orbiting with the right button to snap to an axis view.
        alt_left = buttons & Qt.LeftButton and mods & Qt.AltModifier and self.gizmo_hit is None and not getattr(self, "_tool_dragging", False)
        if buttons & Qt.RightButton or alt_left or buttons & Qt.MiddleButton:
            self.setCursor(Qt.ClosedHandCursor)
            orbit = (buttons & Qt.RightButton and not mods & Qt.ShiftModifier) or alt_left or (buttons & Qt.MiddleButton and mods & Qt.ShiftModifier)
            if orbit:
                self.camera.orbit(dx, dy)
                if mods & Qt.AltModifier and buttons & Qt.RightButton:
                    self.camera.snap_orthographic()
            else:
                self.camera.pan(dx, dy, self.height())
            self.view_changed.emit()
            self.update()
        elif buttons & Qt.LeftButton and self.gizmo_hit is not None:
            self.dragged.emit(("move", None, self.gizmo_hit, mods, e.position()))
        elif buttons & Qt.LeftButton:
            self.dragged.emit(("drag", None, None, mods, e.position()))
        else:
            if self.comment_hit(pos.x(), pos.y()) or self.view_cube_hit(pos.x(), pos.y()):
                self.setCursor(Qt.PointingHandCursor)
            elif self.gizmo_hit_test(pos.x(), pos.y()) is not None:
                self.setCursor(Qt.SizeAllCursor)
            else:
                self.setCursor(self.tool_cursor)
            self.dragged.emit(("hover", None, None, mods, e.position()))
        self._last_mouse = pos

    def mouseReleaseEvent(self, e: QMouseEvent):
        self.setCursor(self.tool_cursor)
        if e.button() == Qt.LeftButton and self._consume_left:
            self._consume_left = False
            self._drag_button = None
            return
        if e.button() == Qt.LeftButton:
            if self.gizmo_hit is not None:
                self.dragged.emit(("end", None, self.gizmo_hit, e.modifiers(), e.position()))
                self.gizmo_hit = None
            else:
                self.dragged.emit(("release", None, None, e.modifiers(), e.position()))
        elif e.button() == Qt.RightButton:
            if (e.position().toPoint() - getattr(self, "_right_press", e.position().toPoint())).manhattanLength() < 4:
                self.context_requested.emit(e.position())
        self._drag_button = None

    def mouseDoubleClickEvent(self, e: QMouseEvent):
        if e.button() == Qt.LeftButton:
            self.dragged.emit(("double", None, None, e.modifiers(), e.position()))

    def keyPressEvent(self, e):
        """Arrow keys orbit by 10° (Shift: pan, Ctrl: 90° steps); everything else goes to the window."""
        key = e.key()
        mods = e.modifiers()
        if key in (Qt.Key_Left, Qt.Key_Right, Qt.Key_Up, Qt.Key_Down):
            step = 90.0 if mods & Qt.ControlModifier else 10.0
            dx = (step if key == Qt.Key_Right else -step if key == Qt.Key_Left else 0.0)
            dy = (step if key == Qt.Key_Up else -step if key == Qt.Key_Down else 0.0)
            if mods & Qt.ShiftModifier:
                self.camera.pan(-dx * 4, dy * 4, self.height())
            else:
                self.camera.orbit(-dx / 0.4, dy / 0.4)
            self.view_changed.emit()
            self.update()
            return
        e.ignore()
        super().keyPressEvent(e)

    def wheelEvent(self, e: QWheelEvent):
        delta = e.angleDelta().y()
        if delta == 0:
            return
        factor = 0.9 if delta > 0 else 1.1
        anchor = None
        pos = e.position()
        snap = self.snap(pos.x(), pos.y(), suppress=True)
        if snap.kind in ("free", "plane"):
            anchor = snap.point
        self.camera.zoom(factor, anchor)
        self.view_changed.emit()
        self.update()


# ------------------------------------------------------------- helpers


def _ray_plane(origin: Vec3, d: Vec3, plane: Plane) -> Optional[Vec3]:
    n = v_unit(plane.normal)
    denom = v_dot(d, n)
    if abs(denom) < 1e-9:
        return None
    t = v_dot(v_sub(plane.origin, origin), n) / denom
    if t < 0:
        return None
    return v_add(origin, v_scale(d, t))


def _seg_dist_2d(a, b, p) -> float:
    ab = (b[0] - a[0], b[1] - a[1])
    l2 = ab[0] ** 2 + ab[1] ** 2
    t = 0.0 if l2 < 1e-12 else max(0.0, min(1.0, ((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2))
    q = (a[0] + ab[0] * t, a[1] + ab[1] * t)
    return math.hypot(q[0] - p[0], q[1] - p[1])


def _face_normals(verts: np.ndarray, tris) -> np.ndarray:
    n = np.zeros_like(verts)
    for a, b, c in tris:
        fn = np.cross(verts[b] - verts[a], verts[c] - verts[a])
        n[a] += fn
        n[b] += fn
        n[c] += fn
    lens = np.linalg.norm(n, axis=1)
    lens[lens == 0] = 1
    return (n / lens[:, None]).astype(np.float32)


def _face_boundary_edges(verts, idx, tf):
    """Weld display seams and find face boundaries without per-triangle Python loops."""
    if not len(idx) or not len(tf):
        return np.empty((0, 2), dtype=np.uint32)
    _, remap = np.unique(np.round(verts * 1e4).astype(np.int64), axis=0, return_inverse=True)
    edges = np.concatenate((idx[:, [0, 1]], idx[:, [1, 2]], idx[:, [2, 0]]))
    keys = np.sort(remap[edges], axis=1)
    faces = np.tile(tf, 3)
    order = np.lexsort((keys[:, 1], keys[:, 0]))
    keys, faces = keys[order], faces[order]
    starts = np.r_[0, np.flatnonzero(np.any(keys[1:] != keys[:-1], axis=1)) + 1]
    boundary = np.minimum.reduceat(faces, starts) != np.maximum.reduceat(faces, starts)
    return np.ascontiguousarray(edges[order[starts[boundary]]], dtype=np.uint32)


def _display_edges(doc: Document, node: Node, mesh: Mesh, verts: np.ndarray, idx: np.ndarray, tf: np.ndarray):
    """Edges between different B-rep faces (from the tessellation) plus the
    kernel's edges sampled for picking/snapping, and its vertices."""
    arr = _face_boundary_edges(verts, idx, tf)
    samples: list[np.ndarray] = []
    vpts = np.zeros((0, 3), dtype=np.float32)
    centers = []
    body = doc.resolved_body(node.id) if node.kind != "mesh" else None
    if body is not None:
        try:
            k = doc.kernel
            for e in k.edges(body):
                pts = k.sample_edge(e, body, 24 if e.kind.value != "line" else 2)
                samples.append(np.asarray(pts, dtype=np.float32))
                if e.center is not None:
                    centers.append(e.center)
            vpts = np.asarray([v.point for v in k.vertices(body)], dtype=np.float32).reshape(-1, 3)
        except Exception:
            pass
    else:
        vpts = verts[:: max(1, len(verts) // 4000)]
    item_centers = centers
    out = (arr, np.full(len(arr), -1, dtype=np.int32), samples, vpts)
    _display_edges.last_centers = item_centers
    return out


def _curve_item(doc: Document, node: Node) -> RenderItem:
    body = doc.resolved_body(node.id)
    k = doc.kernel
    samples = []
    vpts = []
    if body is not None:
        for e in k.edges(body):
            samples.append(np.asarray(k.sample_edge(e, body, 32), dtype=np.float32))
        vpts = [v.point for v in k.vertices(body)]
    pts = np.concatenate(samples) if samples else np.zeros((0, 3), dtype=np.float32)
    lo = tuple(map(float, pts.min(axis=0))) if len(pts) else (0, 0, 0)
    hi = tuple(map(float, pts.max(axis=0))) if len(pts) else (0, 0, 0)
    return RenderItem(node.id, np.zeros((0, 3), np.float32), np.zeros((0, 3), np.float32), np.zeros((0, 3), np.uint32), np.zeros(0, np.int32), np.zeros((0, 2), np.uint32), np.zeros(0, np.int32), np.asarray(vpts, dtype=np.float32).reshape(-1, 3), node.color or (0.35, 0.8, 1.0), "curve", (lo, hi), 0, samples)


def _overhang_mask(mesh: Mesh, threshold: float) -> np.ndarray:
    mask = np.zeros(len(mesh.triangles), dtype=bool)
    for o in overhangs(mesh, threshold_deg=threshold):
        mask[o.triangle] = True
    return mask


def _make_matcap(size: int = 128):
    """A procedural clay matcap: a lit sphere image."""
    img = np.zeros((size, size, 3), dtype=np.uint8)
    for y in range(size):
        for x in range(size):
            nx, ny = (x / size) * 2 - 1, 1 - (y / size) * 2
            r2 = nx * nx + ny * ny
            if r2 > 1:
                img[y, x] = (40, 40, 44)
                continue
            nz = math.sqrt(1 - r2)
            l = max(0.0, nx * -0.4 + ny * 0.6 + nz * 0.7)
            rim = (1 - nz) ** 3 * 0.5
            base = 0.28 + 0.62 * l + rim
            img[y, x] = (min(255, int(235 * base)), min(255, int(225 * base)), min(255, int(210 * base)))
    tex = GL.glGenTextures(1)
    GL.glBindTexture(GL.GL_TEXTURE_2D, tex)
    GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MIN_FILTER, GL.GL_LINEAR)
    GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MAG_FILTER, GL.GL_LINEAR)
    GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGB, size, size, 0, GL.GL_RGB, GL.GL_UNSIGNED_BYTE, img.tobytes())
    return tex


def _upload_image(data: Optional[bytes], path: Optional[str]):
    img = QImage()
    if data:
        img.loadFromData(data)
    elif path:
        img.load(path)
    if img.isNull():
        return 0
    img = img.convertToFormat(QImage.Format_RGBA8888)
    w, h = img.width(), img.height()
    ptr = img.constBits()
    buf = bytes(ptr)[: w * h * 4]
    tex = GL.glGenTextures(1)
    GL.glBindTexture(GL.GL_TEXTURE_2D, tex)
    GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MIN_FILTER, GL.GL_LINEAR)
    GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MAG_FILTER, GL.GL_LINEAR)
    GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGBA, w, h, 0, GL.GL_RGBA, GL.GL_UNSIGNED_BYTE, buf)
    return tex
