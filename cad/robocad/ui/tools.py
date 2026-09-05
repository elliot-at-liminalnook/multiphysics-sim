"""Interaction tools: small state machines fed by the viewport's mouse
events, each with a live readout and Tab-to-type numeric fields.

A tool gets `ctx` (the app's ToolContext: document, ops, viewport, status
and numeric-entry hooks). It handles `press`, `drag`, `release`, `hover`,
gizmo `start/move/end`, `double`, keys, and `commit(values)` when the
user presses Enter in the numeric field.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Callable, Optional

from PySide6.QtCore import Qt

from ..kernel import BooleanOp, ChamferSpec, EdgeRef, FaceRef, KernelError, Plane, Sketch, SurfaceKind, Vec3
from ..kernel.base import v_add, v_cross, v_dist, v_dot, v_norm, v_scale, v_sub, v_unit
from ..printing import FastenerSpec
from ..units import format_angle, format_length


@dataclass
class NumericField:
    name: str
    value: float
    angle: bool = False
    unit: str = "mm"


class ToolContext:
    """What tools need from the app, kept narrow so tools stay testable."""

    def __init__(self, app):
        self.app = app

    @property
    def doc(self):
        return self.app.doc

    @property
    def ops(self):
        return self.app.ops

    @property
    def vp(self):
        return self.app.viewport

    def status(self, text: str):
        self.app.status(text)

    def readout(self, text: str):
        self.app.readout(text)

    def numeric(self, fields: list[NumericField], on_commit: Callable[[list[float]], None]):
        self.app.numeric_fields(fields, on_commit)

    def close_numeric(self):
        self.app.numeric_fields([], None)

    def error(self, text: str):
        self.app.error(text)

    def snap(self, pos, suppress=False, plane: Optional[Plane] = None):
        return self.vp.snap(pos.x(), pos.y(), suppress=suppress, want_plane=plane)

    def active_plane(self) -> Plane:
        return self.vp.active_plane or Plane.xy()

    def selection(self):
        return self.vp.selection

    def refresh(self):
        self.vp.update()


class Tool:
    name = "tool"
    hint = ""

    def __init__(self, ctx: ToolContext):
        self.ctx = ctx
        self.fields: list[NumericField] = []

    def activate(self):
        self.ctx.status(self.hint or self.name)

    def deactivate(self):
        self.ctx.vp.temp_shapes.clear()
        self.ctx.vp.gizmo = None
        self.ctx.close_numeric()

    def press(self, pos, mods): ...
    def drag(self, pos, mods): ...
    def release(self, pos, mods): ...
    def hover(self, pos, mods): ...
    def double(self, pos, mods): ...
    def gizmo(self, phase: str, handle: int, pos, mods): ...
    def key(self, key: int, mods) -> bool:
        return False
    def commit(self, values: list[float]): ...
    def cancel(self):
        self.ctx.vp.temp_shapes.clear()
        self.ctx.refresh()


# ------------------------------------------------------------- selection


class SelectTool(Tool):
    name = "select"
    hint = "Click to select • Shift adds • Ctrl toggles • drag for box select • double-click a face for its dimension"

    def __init__(self, ctx):
        super().__init__(ctx)
        self._box_start = None
        self._pending = None

    def press(self, pos, mods):
        self._box_start = pos
        self._moved = False

    def drag(self, pos, mods):
        if self._box_start is None:
            return
        if (pos - self._box_start).manhattanLength() > 6:
            self._moved = True
            self.ctx.app.set_rubber_band(self._box_start, pos)

    def release(self, pos, mods):
        vp = self.ctx.vp
        if self._box_start is not None and self._moved:
            self.ctx.app.set_rubber_band(None, None)
            self._box_select(self._box_start, pos, mods)
            self._box_start = None
            return
        self._box_start = None

        def done(result):
            hit = result["hit"]
            sel = vp.selection
            cands = result["candidates"]
            if len(cands) > 1 and (mods & Qt.AltModifier):
                self.ctx.app.disambiguate(cands, pos, lambda c: self._apply(c, mods, result["world"]))
                return
            self._apply(hit, mods, result["world"])

        vp.request_pick(pos.x(), pos.y(), done)

    def _apply(self, hit, mods, world):
        vp = self.ctx.vp
        sel = vp.selection
        if hit is None:
            if not (mods & (Qt.ShiftModifier | Qt.ControlModifier)):
                sel.clear()
        else:
            kind, nid, idx = hit
            item = (nid, kind, idx)
            if mods & Qt.ControlModifier:
                sel.toggle(item)
            elif mods & Qt.ShiftModifier:
                if item not in sel.items:
                    sel.items.append(item)
            else:
                sel.clear()
                sel.items.append(item)
        self.ctx.app.selection_changed(world)
        vp.update()

    def hover(self, pos, mods):
        vp = self.ctx.vp

        def done(result):
            new = None
            if result["hit"]:
                kind, nid, idx = result["hit"]
                new = (nid, kind, idx)
            if new != vp.hover:
                vp.hover = new
                vp.update()

        vp.request_hover(pos.x(), pos.y(), done)

    def double(self, pos, mods):
        vp = self.ctx.vp

        def done(result):
            if result["hit"]:
                kind, nid, idx = result["hit"]
                self.ctx.app.edit_dimension_at(nid, kind, idx, result["world"])

        prev = vp.selection_mode
        if prev == "body":
            vp.selection_mode = "face"
        vp.request_pick(pos.x(), pos.y(), lambda r: (setattr(vp, "selection_mode", prev), done(r)))

    def _box_select(self, a, b, mods):
        vp = self.ctx.vp
        x0, x1 = sorted((a.x(), b.x()))
        y0, y1 = sorted((a.y(), b.y()))
        w, h = vp.width(), vp.height()
        found = []
        for nid, it in vp.items.items():
            if not vp.doc.is_visible(nid):
                continue
            if vp.selection_mode == "vertex":
                for i, p in enumerate(it.vertex_points):
                    sp = vp.camera.project(tuple(map(float, p)), w, h)
                    if sp and x0 <= sp[0] <= x1 and y0 <= sp[1] <= y1:
                        found.append((nid, "vertex", i))
            elif vp.selection_mode == "edge":
                for i, seg in enumerate(it.edge_samples):
                    if all((sp := vp.camera.project(tuple(map(float, p)), w, h)) and x0 <= sp[0] <= x1 and y0 <= sp[1] <= y1 for p in seg):
                        found.append((nid, "edge", i))
            else:
                lo, hi = it.bbox
                corners = [(x, y, z) for x in (lo[0], hi[0]) for y in (lo[1], hi[1]) for z in (lo[2], hi[2])]
                inside = [vp.camera.project(c, w, h) for c in corners]
                if all(sp and x0 <= sp[0] <= x1 and y0 <= sp[1] <= y1 for sp in inside):
                    found.append((nid, "body", 0))
        if not (mods & (Qt.ShiftModifier | Qt.ControlModifier)):
            vp.selection.clear()
        for f in found:
            if f not in vp.selection.items:
                vp.selection.items.append(f)
        self.ctx.app.selection_changed(None)
        vp.update()


# --------------------------------------------------------------- transform


class TransformTool(Tool):
    """Move / rotate / scale with the gizmo; Tab types the exact amount."""

    def __init__(self, ctx, mode: str):
        super().__init__(ctx)
        self.mode = mode
        self.name = mode
        self.hint = {"move": "Drag an axis (Ctrl snaps to grid) • Tab for exact distance • click the centre handle for screen-space move", "rotate": "Drag a ring (Ctrl snaps 15°) • Tab for exact angle", "scale": "Drag an axis handle • Tab for exact factor"}[mode]
        self.origin = None
        self.start_pt = None
        self.axis_index = None
        self.accum = (0.0, 0.0, 0.0)
        self.accum_angle = 0.0
        self.accum_scale = 1.0
        self.free_mode = False

    def activate(self):
        super().activate()
        self._place_gizmo()

    def _place_gizmo(self):
        vp = self.ctx.vp
        ids = vp.selection.nodes()
        if not ids:
            vp.gizmo = None
            self.ctx.status("Select something to transform")
            return
        from ..analysis import selection_properties

        node = self.ctx.doc.nodes[ids[0]]
        if node.pivot is not None:
            o = node.pivot
        else:
            props = selection_properties(self.ctx.doc, ids)
            o = props.centroid if props else (0.0, 0.0, 0.0)
        self.origin = o
        axes = [((1.0, 0.0, 0.0), (0.85, 0.3, 0.3)), ((0.0, 1.0, 0.0), (0.3, 0.75, 0.3)), ((0.0, 0.0, 1.0), (0.3, 0.45, 0.95))]
        vp.gizmo = {"origin": o, "axes": axes, "mode": self.mode}
        vp.update()
        self.ctx.numeric(self._fields(), self.commit)

    def _fields(self):
        if self.mode == "move":
            return [NumericField("dx", 0.0), NumericField("dy", 0.0), NumericField("dz", 0.0)]
        if self.mode == "rotate":
            return [NumericField("angle", 0.0, angle=True)]
        return [NumericField("factor", 1.0, unit="")]

    def gizmo(self, phase, handle, pos, mods):
        vp = self.ctx.vp
        if self.origin is None:
            return
        if phase == "start":
            self.axis_index = handle
            self.free_mode = handle == 3
            self.start_pt = self._drag_point(pos)
            self.accum = (0.0, 0.0, 0.0)
            self.accum_angle = 0.0
            self.accum_scale = 1.0
            self._drag_pos0 = pos
            return
        if self.start_pt is None:
            return
        p = self._drag_point(pos)
        if p is None:
            return
        if self.mode == "move":
            delta = v_sub(p, self.start_pt)
            if not self.free_mode:
                a = v_unit(vp.gizmo["axes"][self.axis_index][0])
                d = v_dot(delta, a)
                if mods & Qt.ControlModifier:
                    d = round(d / vp.grid_step) * vp.grid_step
                delta = v_scale(a, d)
            self.accum = delta
            self.ctx.readout(f"Δ = ({format_length(delta[0])}, {format_length(delta[1])}, {format_length(delta[2])})  |{v_norm(delta):.3f} mm|")
            vp.temp_shapes = [("line", (self.origin, v_add(self.origin, delta), (1.0, 0.9, 0.3)))]
        elif self.mode == "rotate":
            a = v_unit(vp.gizmo["axes"][self.axis_index][0]) if not self.free_mode else v_scale(vp.camera.basis()[2], 1.0)
            v0 = v_sub(self.start_pt, self.origin)
            v1 = v_sub(p, self.origin)
            v0 = v_sub(v0, v_scale(a, v_dot(v0, a)))
            v1 = v_sub(v1, v_scale(a, v_dot(v1, a)))
            if v_norm(v0) > 1e-9 and v_norm(v1) > 1e-9:
                ang = math.degrees(math.atan2(v_dot(v_cross(v0, v1), a), v_dot(v0, v1)))
                if mods & Qt.ControlModifier:
                    ang = round(ang / 15.0) * 15.0
                self.accum_angle = ang
                self.ctx.readout(f"angle = {format_angle(ang)}")
        else:
            d0 = v_dist(self.start_pt, self.origin) or 1.0
            d1 = v_dist(p, self.origin)
            f = d1 / d0
            if mods & Qt.ControlModifier:
                f = round(f * 10) / 10
            self.accum_scale = max(f, 0.01)
            self.ctx.readout(f"scale = ×{self.accum_scale:.3f}")
        self._preview()
        if phase == "end":
            self._apply()

    def _drag_point(self, pos):
        vp = self.ctx.vp
        if self.free_mode or self.mode == "rotate":
            plane = vp.screen_plane(self.origin) if self.free_mode else Plane.from_normal(self.origin, vp.gizmo["axes"][self.axis_index][0])
            return vp.world_on_plane(pos.x(), pos.y(), plane)
        # A plane containing the axis and facing the camera as much as possible.
        a = v_unit(vp.gizmo["axes"][self.axis_index][0])
        _, _, back = vp.camera.basis()
        n = v_cross(a, v_cross(back, a))
        if v_norm(n) < 1e-6:
            n = back
        plane = Plane.from_normal(self.origin, n)
        return vp.world_on_plane(pos.x(), pos.y(), plane)

    def _preview(self):
        self.ctx.refresh()

    def _apply(self):
        ids = self.ctx.vp.selection.nodes()
        if not ids:
            return
        try:
            if self.mode == "move" and any(abs(c) > 1e-9 for c in self.accum):
                self.ctx.ops.transform(ids, translation=self.accum)
                self.origin = v_add(self.origin, self.accum)
            elif self.mode == "rotate" and abs(self.accum_angle) > 1e-9:
                a = self.ctx.vp.gizmo["axes"][self.axis_index][0] if not self.free_mode else self.ctx.vp.camera.basis()[2]
                self.ctx.ops.transform(ids, axis=a, angle_deg=self.accum_angle, center=self.origin)
            elif self.mode == "scale" and abs(self.accum_scale - 1.0) > 1e-9:
                self.ctx.ops.transform(ids, scale=self.accum_scale, center=self.origin)
        except KernelError as e:
            self.ctx.error(str(e))
        self.start_pt = None
        self.ctx.vp.temp_shapes.clear()
        self._place_gizmo()

    def commit(self, values):
        ids = self.ctx.vp.selection.nodes()
        if not ids:
            return
        try:
            if self.mode == "move":
                self.ctx.ops.transform(ids, translation=(values[0], values[1], values[2]))
            elif self.mode == "rotate":
                a = self.ctx.vp.gizmo["axes"][self.axis_index or 2][0]
                self.ctx.ops.transform(ids, axis=a, angle_deg=values[0], center=self.origin)
            else:
                self.ctx.ops.transform(ids, scale=values[0], center=self.origin)
        except KernelError as e:
            self.ctx.error(str(e))
        self._place_gizmo()


# --------------------------------------------------------------- primitives


class PrimitiveTool(Tool):
    """Box / cylinder / sphere by click-drag on the active plane, then
    height by drag; Tab at any point for exact sizes."""

    def __init__(self, ctx, kind: str, center_mode: bool = False):
        super().__init__(ctx)
        self.kind = kind
        self.center_mode = center_mode
        self.name = kind
        self.hint = f"Click-drag the base on the plane, then drag the height • Tab for exact sizes • {'centre' if center_mode else 'corner'} mode"
        self.stage = 0
        self.p0 = None
        self.p1 = None
        self.h = 10.0

    def activate(self):
        super().activate()
        self.stage = 0
        self.ctx.numeric(self._fields(), self.commit)

    def _fields(self):
        if self.kind == "box":
            return [NumericField("width", 20.0), NumericField("depth", 20.0), NumericField("height", 10.0)]
        if self.kind == "cylinder":
            return [NumericField("diameter", 10.0), NumericField("height", 10.0)]
        return [NumericField("diameter", 10.0)]

    def press(self, pos, mods):
        plane = self.ctx.active_plane()
        s = self.ctx.snap(pos, suppress=bool(mods & Qt.AltModifier), plane=plane)
        if self.stage == 0:
            self.p0 = s.point
            self.p1 = s.point
            self.stage = 1
        elif self.stage == 2:
            self._finish()

    def drag(self, pos, mods):
        if self.stage == 1:
            plane = self.ctx.active_plane()
            s = self.ctx.snap(pos, suppress=bool(mods & Qt.AltModifier), plane=plane)
            self.p1 = s.point
            self._preview()

    def release(self, pos, mods):
        if self.stage == 1:
            if self.kind == "sphere" or v_dist(self.p0, self.p1) < 1e-6:
                if self.kind == "sphere":
                    self._finish()
                return
            self.stage = 2
            self.ctx.status("Drag the height, click to confirm • Tab for exact height")

    def hover(self, pos, mods):
        if self.stage == 2:
            plane = self.ctx.active_plane()
            # Height along the plane normal: intersect with a plane through p1 facing the camera.
            n = v_unit(plane.normal)
            _, _, back = self.ctx.vp.camera.basis()
            side = v_cross(n, v_cross(back, n))
            if v_norm(side) < 1e-6:
                side = back
            hp = self.ctx.vp.world_on_plane(pos.x(), pos.y(), Plane.from_normal(self.p1, side))
            if hp is not None:
                h = v_dot(v_sub(hp, self.p1), n)
                if mods & Qt.ControlModifier:
                    h = round(h / self.ctx.vp.grid_step) * self.ctx.vp.grid_step
                self.h = h if abs(h) > 1e-6 else 0.001
                self._preview()

    def _dims(self):
        plane = self.ctx.active_plane()
        u0, v0, _ = plane.to_local(self.p0)
        u1, v1, _ = plane.to_local(self.p1)
        return plane, (u0, v0), (u1, v1)

    def _preview(self):
        vp = self.ctx.vp
        plane, a, b = self._dims()
        if self.kind == "box":
            if self.center_mode:
                w, d = 2 * abs(b[0] - a[0]), 2 * abs(b[1] - a[1])
                x0, y0 = a[0] - w / 2, a[1] - d / 2
            else:
                w, d = b[0] - a[0], b[1] - a[1]
                x0, y0 = a[0], a[1]
            corners = [plane.to_world(x0, y0), plane.to_world(x0 + w, y0), plane.to_world(x0 + w, y0 + d), plane.to_world(x0, y0 + d)]
            top = [v_add(c, v_scale(plane.normal, self.h if self.stage == 2 else 0.0)) for c in corners]
            vp.temp_shapes = [("poly", (corners + [corners[0]], (0.4, 0.9, 1.0))), ("poly", (top + [top[0]], (0.4, 0.9, 1.0)))] + [("line", (c, t, (0.4, 0.9, 1.0))) for c, t in zip(corners, top)]
            self.ctx.readout(f"{format_length(abs(w))} × {format_length(abs(d))} × {format_length(self.h if self.stage == 2 else 0)}")
        else:
            r = math.hypot(b[0] - a[0], b[1] - a[1])
            ring = [plane.to_world(a[0] + r * math.cos(t), a[1] + r * math.sin(t)) for t in [2 * math.pi * k / 48 for k in range(49)]]
            shapes = [("poly", (ring, (0.4, 0.9, 1.0)))]
            if self.kind == "cylinder" and self.stage == 2:
                shapes.append(("poly", ([v_add(p, v_scale(plane.normal, self.h)) for p in ring], (0.4, 0.9, 1.0))))
            vp.temp_shapes = shapes
            self.ctx.readout(f"Ø {format_length(2 * r)}" + (f" × {format_length(self.h)}" if self.kind == "cylinder" and self.stage == 2 else ""))
        self.ctx.refresh()

    def _finish(self):
        plane, a, b = self._dims()
        try:
            if self.kind == "box":
                if self.center_mode:
                    w, d = 2 * abs(b[0] - a[0]), 2 * abs(b[1] - a[1])
                    x0, y0 = a[0] - w / 2, a[1] - d / 2
                else:
                    w, d = b[0] - a[0], b[1] - a[1]
                    x0, y0 = min(a[0], b[0]), min(a[1], b[1])
                    w, d = abs(w), abs(d)
                self._make_box(plane, x0, y0, w, d, self.h)
            elif self.kind == "cylinder":
                r = math.hypot(b[0] - a[0], b[1] - a[1])
                base = plane.to_world(a[0], a[1])
                n = plane.normal if self.h > 0 else v_scale(plane.normal, -1.0)
                self.ctx.ops.cylinder(base, n, max(r, 1e-3), abs(self.h))
            else:
                r = max(math.hypot(b[0] - a[0], b[1] - a[1]), 1e-3)
                self.ctx.ops.sphere(self.p0, r)
        except KernelError as e:
            self.ctx.error(str(e))
        self.stage = 0
        self.ctx.vp.temp_shapes.clear()
        self.ctx.refresh()

    def _make_box(self, plane, x0, y0, w, d, h):
        sk = Sketch(plane)
        sk.rectangle((x0, y0), (max(w, 1e-3), max(d, 1e-3)))
        self.ctx.ops.extrude(sk.to_body(), abs(h) if abs(h) > 1e-6 else 1.0, plane.normal if h >= 0 else v_scale(plane.normal, -1.0), name="Box")

    def commit(self, values):
        plane = self.ctx.active_plane()
        anchor = self.p0 or plane.origin
        u, v, _ = plane.to_local(anchor)
        try:
            if self.kind == "box":
                w, d, h = values
                if self.center_mode:
                    u, v = u - w / 2, v - d / 2
                self._make_box(plane, u, v, w, d, h)
            elif self.kind == "cylinder":
                dia, h = values
                self.ctx.ops.cylinder(plane.to_world(u, v), plane.normal, dia / 2, h)
            else:
                self.ctx.ops.sphere(anchor, values[0] / 2)
        except KernelError as e:
            self.ctx.error(str(e))
        self.stage = 0
        self.ctx.vp.temp_shapes.clear()
        self.ctx.refresh()


# ---------------------------------------------------------------- push/pull


class PushPullTool(Tool):
    name = "push_pull"
    hint = "Drag a face along its normal • Tab for exact distance • Shift: offset instead of push"

    def __init__(self, ctx, offset: bool = False):
        super().__init__(ctx)
        self.offset = offset
        self.target = None  # (node_id, FaceRef)
        self.start = None
        self.dist = 0.0

    def activate(self):
        super().activate()
        self.ctx.vp.selection_mode = "face"
        self.ctx.numeric([NumericField("distance", 0.0)], self.commit)
        faces = self.ctx.vp.selection.faces()
        if faces:
            nid, idx = faces[0]
            self._set_target(nid, idx)

    def _set_target(self, nid, idx):
        body = self.ctx.doc.resolved_body(nid)
        if body is None:
            return
        faces = self.ctx.doc.kernel.faces(body)
        if idx < len(faces):
            self.target = (nid, faces[idx])

    def press(self, pos, mods):
        vp = self.ctx.vp

        def done(result):
            if result["hit"]:
                kind, nid, idx = result["hit"]
                if kind == "face":
                    self._set_target(nid, idx)
                    vp.selection.clear()
                    vp.selection.items.append((nid, "face", idx))
                    self.start = result["world"]
                    self.dist = 0.0
                    self._dragging = True

        vp.request_pick(pos.x(), pos.y(), done)

    def drag(self, pos, mods):
        if not self.target or self.start is None:
            return
        nid, face = self.target
        n = v_unit(face.normal)
        _, _, back = self.ctx.vp.camera.basis()
        side = v_cross(n, v_cross(back, n))
        if v_norm(side) < 1e-6:
            side = back
        p = self.ctx.vp.world_on_plane(pos.x(), pos.y(), Plane.from_normal(self.start, side))
        if p is None:
            return
        d = v_dot(v_sub(p, self.start), n)
        if mods & Qt.ControlModifier:
            d = round(d / self.ctx.vp.grid_step) * self.ctx.vp.grid_step
        self.dist = d
        self.ctx.readout(f"{'offset' if self.offset else 'push/pull'} {format_length(d)}")
        self.ctx.vp.temp_shapes = [("line", (face.centroid, v_add(face.centroid, v_scale(n, d)), (1.0, 0.9, 0.3)))]
        self.ctx.refresh()

    def release(self, pos, mods):
        if self.target and self.start is not None and abs(self.dist) > 1e-6:
            self._apply(self.dist, bool(mods & Qt.ShiftModifier) or self.offset)
        self.start = None
        self.ctx.vp.temp_shapes.clear()
        self.ctx.refresh()

    def _apply(self, d, offset):
        nid, face = self.target
        try:
            if offset or face.kind != SurfaceKind.PLANE:
                self.ctx.ops.offset_faces(nid, [face], d)
            else:
                self.ctx.ops.push_pull(nid, face, d)
            body = self.ctx.doc.resolved_body(nid)
            self.target = (nid, self.ctx.doc.kernel.find_face(body, face))
        except KernelError as e:
            self.ctx.error(str(e))

    def commit(self, values):
        if self.target:
            self._apply(values[0], self.offset)


# ----------------------------------------------------------------- sketching


class SketchTool(Tool):
    """Sketch primitives on the active plane. `shape`: line | rectangle |
    rectangle_center | circle | circle_2pt | circle_3pt | arc_3pt |
    polygon | slot | spline | ellipse | spiral | text."""

    def __init__(self, ctx, shape: str):
        super().__init__(ctx)
        self.shape = shape
        self.name = f"sketch.{shape}"
        self.points: list[Vec3] = []
        self.hint = f"{shape}: click points on the plane • Enter/double-click to finish • Tab for exact values • Esc cancels"
        self.sketch_id: Optional[str] = None

    def activate(self):
        super().activate()
        self.ctx.vp.plane_snapping = True
        self.points = []
        self._ensure_sketch()
        self.ctx.numeric(self._fields(), self.commit)

    def deactivate(self):
        super().deactivate()
        self.ctx.vp.plane_snapping = False

    def _fields(self):
        return {
            "line": [NumericField("length", 20.0), NumericField("angle", 0.0, angle=True)],
            "rectangle": [NumericField("width", 20.0), NumericField("height", 10.0)],
            "rectangle_center": [NumericField("width", 20.0), NumericField("height", 10.0)],
            "circle": [NumericField("diameter", 10.0)],
            "polygon": [NumericField("radius", 10.0), NumericField("sides", float(Sketch.last_polygon_sides), unit="")],
            "slot": [NumericField("length", 20.0), NumericField("width", 4.0)],
            "ellipse": [NumericField("radius x", 10.0), NumericField("radius y", 5.0)],
            "spiral": [NumericField("start radius", 2.0), NumericField("end radius", 10.0), NumericField("turns", 3.0, unit="")],
            "text": [NumericField("height", 10.0)],
        }.get(self.shape, [])

    def _ensure_sketch(self):
        plane = self.ctx.active_plane()
        for nid, kind, _ in self.ctx.vp.selection.items:
            n = self.ctx.doc.nodes.get(nid)
            if n and n.kind == "sketch" and n.sketch.plane == plane:
                self.sketch_id = nid
                return
        for n in self.ctx.doc.nodes.values():
            if n.kind == "sketch" and n.sketch and n.sketch.plane == plane and self.ctx.doc.is_visible(n.id):
                self.sketch_id = n.id
                return
        self.sketch_id = self.ctx.ops.new_sketch(plane)

    def _local(self, p: Vec3):
        u, v, _ = self.ctx.active_plane().to_local(p)
        return (u, v)

    def press(self, pos, mods):
        s = self.ctx.snap(pos, suppress=bool(mods & Qt.AltModifier), plane=self.ctx.active_plane())
        self.points.append(s.point)
        needed = {"line": 2, "rectangle": 2, "rectangle_center": 2, "circle": 2, "circle_2pt": 2, "circle_3pt": 3, "arc_3pt": 3, "polygon": 2, "slot": 3, "ellipse": 3, "spiral": 2, "text": 1}.get(self.shape, 99)
        if len(self.points) >= needed:
            self._finish()

    def hover(self, pos, mods):
        if not self.points:
            return
        s = self.ctx.snap(pos, suppress=bool(mods & Qt.AltModifier), plane=self.ctx.active_plane())
        pts = self.points + [s.point]
        a, b = self._local(pts[0]), self._local(pts[-1])
        plane = self.ctx.active_plane()
        preview = Sketch(plane)
        try:
            self._build(preview, [self._local(p) for p in pts])
        except Exception:
            pass
        shapes = []
        for c in preview.curves:
            sp = c.sample(48)
            if c.kind == "slot":
                from ..io.exporters import _slot_points

                sp = _slot_points(c) + [_slot_points(c)[0]]
            shapes.append(("poly", ([plane.to_world(*p) for p in sp], (0.4, 0.9, 1.0))))
        self.ctx.vp.temp_shapes = shapes
        if self.shape in ("line", "spline"):
            self.ctx.readout(f"length {format_length(math.hypot(b[0]-a[0], b[1]-a[1]))}  angle {format_angle(math.degrees(math.atan2(b[1]-a[1], b[0]-a[0])))}")
        elif self.shape in ("circle", "polygon"):
            self.ctx.readout(f"radius {format_length(math.hypot(b[0]-a[0], b[1]-a[1]))}")
        else:
            self.ctx.readout(f"{format_length(abs(b[0]-a[0]))} × {format_length(abs(b[1]-a[1]))}")
        self.ctx.refresh()

    def _build(self, sk: Sketch, pts):
        a = pts[0]
        b = pts[-1] if len(pts) > 1 else a
        if self.shape == "line":
            sk.line(a, b)
        elif self.shape == "rectangle":
            sk.rectangle((min(a[0], b[0]), min(a[1], b[1])), (abs(b[0] - a[0]), abs(b[1] - a[1])))
        elif self.shape == "rectangle_center":
            sk.rectangle_center(a, (2 * abs(b[0] - a[0]), 2 * abs(b[1] - a[1])))
        elif self.shape == "circle":
            sk.circle(a, math.hypot(b[0] - a[0], b[1] - a[1]))
        elif self.shape == "circle_2pt":
            sk.circle_two_point(a, b)
        elif self.shape == "circle_3pt" and len(pts) >= 3:
            sk.circle_three_point(pts[0], pts[1], pts[2])
        elif self.shape == "arc_3pt" and len(pts) >= 3:
            sk.arc_three_point(pts[0], pts[1], pts[2])
        elif self.shape == "polygon":
            sk.polygon(a, math.hypot(b[0] - a[0], b[1] - a[1]), rotation=math.degrees(math.atan2(b[1] - a[1], b[0] - a[0])))
        elif self.shape == "slot":
            width = 4.0 if len(pts) < 3 else 2 * abs((pts[2][0] - pts[1][0]) * -(pts[1][1] - pts[0][1]) + (pts[2][1] - pts[1][1]) * (pts[1][0] - pts[0][0])) / max(math.hypot(pts[1][0] - pts[0][0], pts[1][1] - pts[0][1]), 1e-6)
            sk.slot(pts[0], pts[1] if len(pts) > 1 else b, max(width, 0.5))
        elif self.shape == "ellipse":
            rx = math.hypot(b[0] - a[0], b[1] - a[1]) if len(pts) < 3 else math.hypot(pts[1][0] - a[0], pts[1][1] - a[1])
            ry = rx / 2 if len(pts) < 3 else math.hypot(pts[2][0] - a[0], pts[2][1] - a[1])
            sk.ellipse(a, rx, ry, math.degrees(math.atan2(pts[1][1] - a[1], pts[1][0] - a[0])) if len(pts) > 1 else 0.0)
        elif self.shape == "spline":
            if len(pts) >= 2:
                sk.spline(pts)
        elif self.shape == "spiral":
            sk.spiral(a, 0.15 * math.hypot(b[0] - a[0], b[1] - a[1]), math.hypot(b[0] - a[0], b[1] - a[1]), 3.0)
        elif self.shape == "text":
            sk.text(a, getattr(self, "text", "robocad"), getattr(self, "text_height", 10.0))

    def _finish(self):
        pts = [self._local(p) for p in self.points]
        try:
            self.ctx.ops.edit_sketch(self.sketch_id, lambda sk: self._build(sk, pts), label=f"Sketch {self.shape}")
        except Exception as e:
            self.ctx.error(str(e))
        self.points = [] if self.shape != "line" else [self.points[-1]]  # lines chain
        self.ctx.vp.temp_shapes.clear()
        self.ctx.refresh()

    def double(self, pos, mods):
        if self.shape == "spline" and len(self.points) >= 2:
            self._finish()
            self.points = []

    def key(self, key, mods):
        if key in (Qt.Key_Return, Qt.Key_Enter) and self.shape == "spline" and len(self.points) >= 2:
            self._finish()
            self.points = []
            return True
        return False

    def commit(self, values):
        plane = self.ctx.active_plane()
        anchor = self._local(self.points[0]) if self.points else (0.0, 0.0)

        def build(sk: Sketch):
            a = anchor
            if self.shape == "line":
                L, ang = values
                sk.line(a, (a[0] + L * math.cos(math.radians(ang)), a[1] + L * math.sin(math.radians(ang))))
            elif self.shape == "rectangle":
                sk.rectangle(a, (values[0], values[1]))
            elif self.shape == "rectangle_center":
                sk.rectangle_center(a, (values[0], values[1]))
            elif self.shape == "circle":
                sk.circle(a, values[0] / 2)
            elif self.shape == "polygon":
                sk.polygon(a, values[0], int(values[1]))
            elif self.shape == "slot":
                sk.slot(a, (a[0] + values[0], a[1]), values[1])
            elif self.shape == "ellipse":
                sk.ellipse(a, values[0], values[1])
            elif self.shape == "spiral":
                sk.spiral(a, values[0], values[1], values[2])
            elif self.shape == "text":
                sk.text(a, getattr(self, "text", "robocad"), values[0])

        try:
            self.ctx.ops.edit_sketch(self.sketch_id, build, label=f"Sketch {self.shape}")
        except Exception as e:
            self.ctx.error(str(e))
        self.points = []
        self.ctx.vp.temp_shapes.clear()
        self.ctx.refresh()


# ---------------------------------------------------------------- extrude


class ExtrudeTool(Tool):
    name = "extrude"
    hint = "Select a sketch (or closed curves) • drag the height • Tab for exact • Shift: subtract, Ctrl: union, Alt: intersect with the body under the cursor"

    def __init__(self, ctx, revolve: bool = False):
        super().__init__(ctx)
        self.revolve = revolve
        self.h = 10.0
        self.taper = 0.0
        self.source = None
        self.start = None
        self.target_body = None

    def activate(self):
        super().activate()
        for nid, kind, _ in self.ctx.vp.selection.items:
            n = self.ctx.doc.nodes.get(nid)
            if n and n.kind in ("sketch", "curve", "sheet"):
                self.source = nid
        if self.source is None:
            self.source = next((n.id for n in self.ctx.doc.nodes.values() if n.kind == "sketch" and n.sketch and n.sketch.curves and self.ctx.doc.is_visible(n.id)), None)
        if self.revolve:
            self.ctx.numeric([NumericField("angle", 360.0, angle=True)], self.commit)
        else:
            self.ctx.numeric([NumericField("distance", 10.0), NumericField("taper", 0.0, angle=True)], self.commit)
        self._preview()

    def _plane(self) -> Plane:
        n = self.ctx.doc.nodes.get(self.source) if self.source else None
        if n and n.sketch:
            return n.sketch.plane
        return self.ctx.active_plane()

    def press(self, pos, mods):
        self.start = self.ctx.vp.world_on_plane(pos.x(), pos.y(), self._plane())
        self._mods = mods

    def drag(self, pos, mods):
        if self.start is None:
            return
        plane = self._plane()
        n = v_unit(plane.normal)
        _, _, back = self.ctx.vp.camera.basis()
        side = v_cross(n, v_cross(back, n))
        if v_norm(side) < 1e-6:
            side = back
        p = self.ctx.vp.world_on_plane(pos.x(), pos.y(), Plane.from_normal(self.start, side))
        if p is not None:
            h = v_dot(v_sub(p, self.start), n)
            if mods & Qt.ControlModifier:
                h = round(h / self.ctx.vp.grid_step) * self.ctx.vp.grid_step
            self.h = h if abs(h) > 1e-6 else 0.001
            self.ctx.readout(f"extrude {format_length(self.h)}")
            self._preview()

    def release(self, pos, mods):
        if self.start is not None and abs(self.h) > 1e-6:
            self._apply(self.h, self.taper, mods)
        self.start = None

    def _boolean_for(self, mods) -> tuple[BooleanOp, Optional[str]]:
        target = self.ctx.app.body_under_selection()
        if mods & Qt.ShiftModifier and target:
            return BooleanOp.SUBTRACT, target
        if mods & Qt.ControlModifier and target:
            return BooleanOp.UNION, target
        if mods & Qt.AltModifier and target:
            return BooleanOp.INTERSECT, target
        return BooleanOp.NEW, None

    def _preview(self):
        if not self.source:
            return
        try:
            prof, plane = self.ctx.ops._profile(self.source)
            body = self.ctx.doc.kernel.extrude(prof, (plane or self._plane()).normal, self.h, self.taper) if not self.revolve else None
            if body is not None:
                mesh = self.ctx.doc.kernel.tessellate(body, 0.2)
                import numpy as np

                from .viewport import RenderItem

                it = RenderItem(self.source, np.asarray(mesh.vertices, np.float32), np.asarray(mesh.normals, np.float32), np.asarray(mesh.triangles, np.uint32), np.zeros(0, np.int32), np.zeros((0, 2), np.uint32), np.zeros(0, np.int32), np.zeros((0, 3), np.float32), (0.4, 0.8, 1.0), "preview", mesh.bounds(), 0)
                self.ctx.vp.temp_shapes = [("mesh", it)]
        except Exception:
            self.ctx.vp.temp_shapes = []
        self.ctx.refresh()

    def _apply(self, h, taper, mods, angle=None):
        if not self.source:
            self.ctx.error("Select a sketch or closed curve first")
            return
        op, target = self._boolean_for(mods)
        try:
            if self.revolve:
                plane = self._plane()
                self.ctx.ops.revolve(self.source, plane.origin, plane.x_axis, angle or 360.0, op, target)
            else:
                self.ctx.ops.extrude(self.source, h, None, taper, False, op, target)
        except KernelError as e:
            self.ctx.error(str(e))
        self.ctx.vp.temp_shapes.clear()
        self.ctx.refresh()

    def commit(self, values):
        mods = getattr(self, "_mods", Qt.NoModifier)
        if self.revolve:
            self._apply(0, 0, mods, angle=values[0])
        else:
            self._apply(values[0], values[1] if len(values) > 1 else 0.0, mods)


# ------------------------------------------------------------- edge tools


class EdgeTool(Tool):
    """Fillet / chamfer on the selected edges with a numeric field."""

    def __init__(self, ctx, kind: str):
        super().__init__(ctx)
        self.kind = kind
        self.name = kind
        self.hint = f"{kind}: select edges (click adds) then type the size • Enter applies"

    def activate(self):
        super().activate()
        self.ctx.vp.selection_mode = "edge"
        fields = {"fillet": [NumericField("radius", 1.0)], "chamfer": [NumericField("distance", 1.0), NumericField("angle", 45.0, angle=True)], "variable": [NumericField("start radius", 1.0), NumericField("end radius", 2.0)], "chordal": [NumericField("chord", 1.0)]}[self.kind]
        self.ctx.numeric(fields, self.commit)

    def press(self, pos, mods):
        vp = self.ctx.vp

        def done(result):
            if result["hit"] and result["hit"][0] == "edge":
                kind, nid, idx = result["hit"]
                vp.selection.toggle((nid, "edge", idx))
                self.ctx.app.selection_changed(result["world"])
                vp.update()

        vp.request_pick(pos.x(), pos.y(), done)

    def hover(self, pos, mods):
        SelectTool.hover(self, pos, mods)

    def commit(self, values):
        by_node: dict[str, list[EdgeRef]] = {}
        for nid, idx in self.ctx.vp.selection.edges():
            body = self.ctx.doc.resolved_body(nid)
            edges = self.ctx.doc.kernel.edges(body)
            if idx < len(edges):
                by_node.setdefault(nid, []).append(edges[idx])
        if not by_node:
            self.ctx.error("Select one or more edges first")
            return
        try:
            for nid, edges in by_node.items():
                if self.kind == "fillet":
                    self.ctx.ops.fillet(nid, edges, values[0])
                elif self.kind == "variable":
                    self.ctx.ops.fillet(nid, edges, values[0], values[1])
                elif self.kind == "chordal":
                    self.ctx.ops.fillet_chordal(nid, edges, values[0])
                else:
                    self.ctx.ops.chamfer(nid, edges, ChamferSpec(values[0], angle_deg=values[1] if abs(values[1] - 45.0) > 1e-9 else None))
            self.ctx.vp.selection.clear()
        except KernelError as e:
            self.ctx.error(str(e))


class ShellTool(Tool):
    name = "shell"
    hint = "Select the faces to open (click adds), then type the wall thickness"

    def activate(self):
        super().activate()
        self.ctx.vp.selection_mode = "face"
        self.ctx.numeric([NumericField("wall", 2.0)], self.commit)

    def press(self, pos, mods):
        EdgeTool.press(self, pos, mods)

    def hover(self, pos, mods):
        SelectTool.hover(self, pos, mods)

    def commit(self, values):
        faces = self.ctx.vp.selection.faces()
        nodes = self.ctx.vp.selection.nodes()
        if not nodes:
            self.ctx.error("Select a body or its faces to open")
            return
        try:
            for nid in nodes:
                body = self.ctx.doc.resolved_body(nid)
                refs = self.ctx.doc.kernel.faces(body)
                open_faces = [refs[i] for n, i in faces if n == nid and i < len(refs)]
                self.ctx.ops.shell(nid, values[0], open_faces)
            self.ctx.vp.selection.clear()
        except KernelError as e:
            self.ctx.error(str(e))


# ------------------------------------------------------------- measure


class MeasureTool(Tool):
    name = "measure"
    hint = "Click two points/faces/edges • the value is copied to the clipboard • Shift+click keeps it as an annotation"

    def __init__(self, ctx):
        super().__init__(ctx)
        self.picks = []

    def activate(self):
        super().activate()
        self.picks = []

    def press(self, pos, mods):
        vp = self.ctx.vp
        keep = bool(mods & Qt.ShiftModifier)

        def done(result):
            s = self.ctx.snap(pos)
            world = s.point if s.kind not in ("free", "plane") else (result["world"] or s.point)
            self.picks.append((result["hit"], world))
            vp.temp_shapes = [("point", (world, (1.0, 0.9, 0.3), 9.0))]
            if len(self.picks) == 2:
                self.ctx.app.measure_between(self.picks[0], self.picks[1], keep)
                self.picks = []
            vp.update()

        vp.request_pick(pos.x(), pos.y(), done)

    def hover(self, pos, mods):
        s = self.ctx.snap(pos)
        self.ctx.vp.temp_shapes = [("point", (s.point, (0.5, 1.0, 0.6), 8.0))] + ([("point", (self.picks[0][1], (1.0, 0.9, 0.3), 9.0)), ("line", (self.picks[0][1], s.point, (1.0, 0.9, 0.3)))] if self.picks else [])
        if self.picks:
            self.ctx.readout(f"{format_length(v_dist(self.picks[0][1], s.point))}  ({s.kind})")
        else:
            self.ctx.readout(f"{s.kind}")
        self.ctx.refresh()


class PlaneTool(Tool):
    """Construction planes: from a face (click), three points, two points
    aligned to the camera, or a midplane between two faces."""

    def __init__(self, ctx, mode: str):
        super().__init__(ctx)
        self.mode = mode
        self.name = f"plane.{mode}"
        self.hint = {"face": "Click a face", "three": "Click three points", "camera": "Click two points (the plane faces the camera)", "mid": "Click two parallel faces"}[mode]
        self.picks = []

    def activate(self):
        super().activate()
        self.picks = []
        self.ctx.vp.selection_mode = "face" if self.mode in ("face", "mid") else "vertex"

    def press(self, pos, mods):
        vp = self.ctx.vp

        def done(result):
            if self.mode in ("face", "mid"):
                if not result["hit"] or result["hit"][0] != "face":
                    return
                kind, nid, idx = result["hit"]
                body = self.ctx.doc.resolved_body(nid)
                face = self.ctx.doc.kernel.faces(body)[idx]
                self.picks.append((nid, face))
                if self.mode == "face":
                    pid = self.ctx.ops.plane_from_face(nid, face)
                    self.ctx.app.set_active_plane(pid)
                    self.picks = []
                elif len(self.picks) == 2:
                    pid = self.ctx.ops.plane_midplane(self.picks[0][0], self.picks[0][1], self.picks[1][1])
                    self.ctx.app.set_active_plane(pid)
                    self.picks = []
            else:
                s = self.ctx.snap(pos)
                self.picks.append(s.point)
                if self.mode == "three" and len(self.picks) == 3:
                    pid = self.ctx.ops.plane_three_points(*self.picks)
                    self.ctx.app.set_active_plane(pid)
                    self.picks = []
                elif self.mode == "camera" and len(self.picks) == 2:
                    _, _, back = vp.camera.basis()
                    pid = self.ctx.ops.plane_two_points_camera(self.picks[0], self.picks[1], v_scale(back, -1.0))
                    self.ctx.app.set_active_plane(pid)
                    self.picks = []
            vp.update()

        vp.request_pick(pos.x(), pos.y(), done)

    def hover(self, pos, mods):
        SelectTool.hover(self, pos, mods)


class FastenerTool(Tool):
    name = "fastener"
    hint = "Click a face to place a hole: size and kind from the panel (M2–M8; clearance, tap, counterbore, countersink, heat-set insert)"

    def __init__(self, ctx, spec: FastenerSpec):
        super().__init__(ctx)
        self.spec = spec

    def activate(self):
        super().activate()
        self.ctx.vp.selection_mode = "face"
        self.ctx.numeric([NumericField("extra clearance", self.spec.extra_clearance)], self.commit)

    def press(self, pos, mods):
        vp = self.ctx.vp

        def done(result):
            if result["hit"] and result["hit"][0] == "face":
                kind, nid, idx = result["hit"]
                body = self.ctx.doc.resolved_body(nid)
                face = self.ctx.doc.kernel.faces(body)[idx]
                s = self.ctx.snap(pos)
                point = s.point if s.kind in ("vertex", "midpoint", "center", "endpoint") else result["world"]
                try:
                    self.ctx.ops.fastener_hole(nid, face, point, self.spec)
                except KernelError as e:
                    self.ctx.error(str(e))
                vp.update()

        vp.request_pick(pos.x(), pos.y(), done)

    def hover(self, pos, mods):
        SelectTool.hover(self, pos, mods)

    def commit(self, values):
        self.spec.extra_clearance = values[0]


class SectionTool(Tool):
    name = "section"
    hint = "Drag the plane along its normal • Tab for exact position • R rotates 90° about Z"

    def activate(self):
        super().activate()
        vp = self.ctx.vp
        if vp.section_plane is None:
            lo, hi = vp.scene_bounds()
            vp.section_plane = Plane.xz((lo[1] + hi[1]) / 2)
        vp.section_enabled = True
        self.ctx.numeric([NumericField("offset", 0.0)], self.commit)
        self._start = None
        vp.update()

    def press(self, pos, mods):
        self._start = self.ctx.vp.world_on_plane(pos.x(), pos.y(), self.ctx.vp.screen_plane(self.ctx.vp.section_plane.origin))
        self._origin0 = self.ctx.vp.section_plane.origin

    def drag(self, pos, mods):
        if self._start is None:
            return
        vp = self.ctx.vp
        p = vp.world_on_plane(pos.x(), pos.y(), vp.screen_plane(self._origin0))
        if p is None:
            return
        n = v_unit(vp.section_plane.normal)
        d = v_dot(v_sub(p, self._start), n)
        vp.section_plane = Plane(v_add(self._origin0, v_scale(n, d)), vp.section_plane.normal, vp.section_plane.x_axis)
        self.ctx.readout(f"section offset {format_length(d)}")
        vp.update()

    def release(self, pos, mods):
        self._start = None

    def key(self, key, mods):
        vp = self.ctx.vp
        if key == Qt.Key_R:
            p = vp.section_plane
            n = v_unit(v_cross((0, 0, 1), p.normal)) if abs(p.normal[2]) < 0.9 else (1.0, 0.0, 0.0)
            vp.section_plane = Plane.from_normal(p.origin, n)
            vp.update()
            return True
        return False

    def commit(self, values):
        vp = self.ctx.vp
        p = vp.section_plane
        vp.section_plane = Plane(v_add(p.origin, v_scale(v_unit(p.normal), values[0])), p.normal, p.x_axis)
        vp.update()


class ImageCalibrateTool(Tool):
    name = "calibrate"
    hint = "Click two points on the image, then type their real distance"

    def __init__(self, ctx, node_id):
        super().__init__(ctx)
        self.node_id = node_id
        self.picks = []

    def deactivate(self):
        self.ctx.vp.annotations = []
        super().deactivate()

    def press(self, pos, mods):
        n = self.ctx.doc.nodes[self.node_id]
        if len(self.picks) >= 2:
            self.ctx.status('Type the real distance below, then press Enter • Esc cancels')
            return
        p = self.ctx.vp.world_on_plane(pos.x(), pos.y(), n.image["plane"])
        if p is None:
            return
        self.picks.append(p)
        self.ctx.vp.annotations = [(point,f'Point {i+1}') for i,point in enumerate(self.picks)]
        self.ctx.vp.update()
        if len(self.picks) == 2:
            self.ctx.numeric([NumericField("real distance", v_dist(*self.picks))], self.commit)

    def commit(self, values):
        if len(self.picks) == 2:
            self.ctx.ops.calibrate_reference(self.node_id, *self.picks, values[0])
            self.ctx.app.set_tool(SelectTool(self.ctx))
            self.ctx.status('Reference calibrated • Ctrl+Z undoes')


class MotorTool(Tool):
    """Click a face: the motor's shaft face lands on the click point, the
    housing sits outside the body (along the face normal) and the shaft
    points into it. The face's body becomes the mounting body unless the
    dialog chose one."""

    name = "motor"
    hint = "Click a face to mount the motor there (housing outside, shaft into the body) • Esc to finish"

    def __init__(self, ctx, params: dict):
        super().__init__(ctx)
        self.params = params

    def activate(self):
        super().activate()
        self.ctx.vp.selection_mode = "face"

    def press(self, pos, mods):
        vp = self.ctx.vp

        def done(result):
            if not result["hit"] or result["hit"][0] != "face":
                return
            kind, nid, idx = result["hit"]
            body = self.ctx.doc.resolved_body(nid)
            face = self.ctx.doc.kernel.faces(body)[idx]
            s = self.ctx.snap(pos)
            point = s.point if s.kind in ("vertex", "midpoint", "center", "endpoint") else result["world"]
            n = v_unit(face.normal)
            if face.kind == SurfaceKind.CYLINDER and face.axis_point is not None and face.axis_dir is not None:
                # On a curved face the local normal is radial from the axis.
                a = v_unit(face.axis_dir)
                d = v_sub(point, face.axis_point)
                n = v_unit(v_sub(d, v_scale(a, v_dot(d, a))))
            p = self.params
            try:
                mid = self.ctx.ops.add_motor(p["spec"], point, v_scale(n, -1.0), p.get("rotation", 0.0), p.get("mount_on") or nid, p.get("cut", True), p.get("name"))
                self.ctx.vp.selection.set_nodes([mid])
                self.ctx.status(f"motor placed on {self.ctx.doc.nodes[nid].name}; Assign motor… links it to a joint")
            except KernelError as e:
                self.ctx.error(str(e))
            self.ctx.refresh()
            vp.update()

        vp.request_pick(pos.x(), pos.y(), done)

    def hover(self, pos, mods):
        SelectTool.hover(self, pos, mods)


class JointTool(Tool):
    """Three clicks: the parent body, the child body, then a face that gives
    the axis (a cylindrical face: its axis; a flat face: its normal through
    the click point). Ctrl-click for the parent means the world. The joint
    dialog then opens prefilled."""

    name = "joint"
    hint = "Click the parent body (Ctrl-click = world), then the child, then a cylindrical face or a flat face for the axis"

    def __init__(self, ctx, on_done: Callable[[dict], None]):
        super().__init__(ctx)
        self.on_done = on_done
        self.parent: Optional[str] = None
        self.child: Optional[str] = None
        self.stage = 0

    def activate(self):
        super().activate()
        self.ctx.vp.selection_mode = "body"
        self.ctx.status("Joint: click the parent body (Ctrl-click for the world)")

    def press(self, pos, mods):
        vp = self.ctx.vp

        def done(result):
            hit = result["hit"]
            if self.stage == 0:
                if mods & Qt.ControlModifier:
                    self.parent = None
                elif hit:
                    self.parent = hit[1]
                else:
                    return
                self.stage = 1
                self.ctx.status(f"Joint: parent = {self.ctx.doc.nodes[self.parent].name if self.parent else 'world'}; now click the child body")
                return
            if self.stage == 1:
                if not hit or hit[1] == self.parent:
                    return
                self.child = hit[1]
                self.stage = 2
                vp.selection_mode = "face"
                self.ctx.status(f"Joint: child = {self.ctx.doc.nodes[self.child].name}; click a cylindrical face (axis) or a flat face (normal) for the joint axis")
                return
            if not hit or hit[0] != "face":
                return
            kind, nid, idx = hit
            face = self.ctx.doc.kernel.faces(self.ctx.doc.resolved_body(nid))[idx]
            if face.kind == SurfaceKind.CYLINDER and face.axis_point is not None and face.axis_dir is not None:
                a = v_unit(face.axis_dir)
                d = v_sub(result["world"], face.axis_point)
                pivot = v_add(face.axis_point, v_scale(a, v_dot(d, a)))
                axis = a
            else:
                pivot, axis = result["world"], v_unit(face.normal)
            self.stage = 3
            self.on_done({"parent": self.parent, "child": self.child, "pivot": pivot, "axis": axis})

        vp.request_pick(pos.x(), pos.y(), done)

    def hover(self, pos, mods):
        SelectTool.hover(self, pos, mods)

    def key(self, key, mods):
        if key == Qt.Key_Escape:
            self.cancel()
            return True
        return False
