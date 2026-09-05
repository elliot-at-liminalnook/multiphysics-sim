"""2D sketching on a construction plane, producing kernel wires.

A `Sketch` holds curves in plane coordinates (u, v); `to_body` turns them
into a wire (or several) in world space through the plane. Editing
operations — trim, split, extend, corner fillet, offset, join — work on
the plane-space curve list so the sketch stays simple to persist and undo.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Optional, Sequence

from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge, BRepBuilderAPI_MakeFace, BRepBuilderAPI_MakeWire
from OCP.GC import GC_MakeArcOfCircle
from OCP.Geom import Geom_BSplineCurve, Geom_TrimmedCurve
from OCP.GeomAPI import GeomAPI_Interpolate, GeomAPI_PointsToBSpline
from OCP.gp import gp_Ax2, gp_Circ, gp_Dir, gp_Elips, gp_Pnt, gp_Vec
from OCP.TColgp import TColgp_Array1OfPnt, TColgp_HArray1OfPnt
from OCP.TColStd import TColStd_Array1OfInteger, TColStd_Array1OfReal
from OCP.TopoDS import TopoDS_Wire

from .base import Body, KernelError, Plane, Vec3, v_add, v_cross, v_dist, v_dot, v_norm, v_scale, v_sub, v_unit

Vec2 = tuple[float, float]


def _p2(a: Vec2, b: Vec2) -> float:
    return math.hypot(a[0] - b[0], a[1] - b[1])


@dataclass
class Curve:
    """One sketch curve. `kind`: line | polyline | circle | arc | ellipse |
    spline (interpolated) | control (control-point curve) | text."""

    kind: str
    points: list[Vec2] = field(default_factory=list)
    center: Optional[Vec2] = None
    radius: float = 0.0
    radius2: float = 0.0
    start_angle: float = 0.0  # degrees
    end_angle: float = 360.0
    rotation: float = 0.0
    degree: int = 3
    closed: bool = False
    text: str = ""
    height: float = 10.0
    font: str = ""
    name: str = ""

    def to_json(self) -> dict:
        return {"kind": self.kind, "points": [list(p) for p in self.points], "center": list(self.center) if self.center else None, "radius": self.radius, "radius2": self.radius2, "start_angle": self.start_angle, "end_angle": self.end_angle, "rotation": self.rotation, "degree": self.degree, "closed": self.closed, "text": self.text, "height": self.height, "font": self.font, "name": self.name}

    @staticmethod
    def from_json(d: dict) -> "Curve":
        return Curve(d["kind"], [tuple(p) for p in d.get("points", [])], tuple(d["center"]) if d.get("center") else None, d.get("radius", 0.0), d.get("radius2", 0.0), d.get("start_angle", 0.0), d.get("end_angle", 360.0), d.get("rotation", 0.0), d.get("degree", 3), d.get("closed", False), d.get("text", ""), d.get("height", 10.0), d.get("font", ""), d.get("name", ""))

    # -- geometry helpers (plane space) --
    def start(self) -> Vec2:
        if self.kind in ("line", "polyline", "spline", "control"):
            return self.points[0]
        if self.kind == "arc":
            a = math.radians(self.start_angle)
            return (self.center[0] + self.radius * math.cos(a), self.center[1] + self.radius * math.sin(a))
        return self.points[0] if self.points else self.center

    def end(self) -> Vec2:
        if self.kind in ("line", "polyline", "spline", "control"):
            return self.points[-1]
        if self.kind == "arc":
            a = math.radians(self.end_angle)
            return (self.center[0] + self.radius * math.cos(a), self.center[1] + self.radius * math.sin(a))
        return self.points[-1] if self.points else self.center

    def sample(self, n: int = 32) -> list[Vec2]:
        if self.kind == "line":
            return [self.points[0], self.points[1]]
        if self.kind == "polyline":
            return list(self.points) + ([self.points[0]] if self.closed else [])
        if self.kind in ("circle", "arc", "ellipse"):
            a0 = math.radians(self.start_angle if self.kind == "arc" else 0.0)
            a1 = math.radians(self.end_angle if self.kind == "arc" else 360.0)
            r2 = self.radius2 if self.kind == "ellipse" else self.radius
            rot = math.radians(self.rotation)
            out = []
            for i in range(n + 1):
                a = a0 + (a1 - a0) * i / n
                x, y = self.radius * math.cos(a), r2 * math.sin(a)
                out.append((self.center[0] + x * math.cos(rot) - y * math.sin(rot), self.center[1] + x * math.sin(rot) + y * math.cos(rot)))
            return out
        if self.kind in ("spline", "control"):
            return _sample_spline(self.points, self.degree, self.closed, n, interpolate=self.kind == "spline")
        return list(self.points)

    def reversed(self) -> "Curve":
        c = Curve(**{**self.__dict__})
        if c.kind in ("line", "polyline", "spline", "control"):
            c.points = list(reversed(self.points))
        elif c.kind == "arc":
            c.start_angle, c.end_angle = self.end_angle, self.start_angle
        return c


def _sample_spline(points: list[Vec2], degree: int, closed: bool, n: int, interpolate: bool) -> list[Vec2]:
    if len(points) < 2:
        return list(points)
    if interpolate and len(points) >= 2:
        # Catmull-Rom style through-points sampling for display; the kernel
        # builds the exact interpolating B-spline.
        pts = list(points) + ([points[0]] if closed else [])
        out = []
        for i in range(len(pts) - 1):
            p0 = pts[i - 1] if i > 0 else pts[i]
            p1, p2 = pts[i], pts[i + 1]
            p3 = pts[i + 2] if i + 2 < len(pts) else pts[i + 1]
            for k in range(n // max(len(pts) - 1, 1) + 1):
                t = k / (n // max(len(pts) - 1, 1) + 1)
                t2, t3 = t * t, t * t * t
                x = 0.5 * ((2 * p1[0]) + (-p0[0] + p2[0]) * t + (2 * p0[0] - 5 * p1[0] + 4 * p2[0] - p3[0]) * t2 + (-p0[0] + 3 * p1[0] - 3 * p2[0] + p3[0]) * t3)
                y = 0.5 * ((2 * p1[1]) + (-p0[1] + p2[1]) * t + (2 * p0[1] - 5 * p1[1] + 4 * p2[1] - p3[1]) * t2 + (-p0[1] + 3 * p1[1] - 3 * p2[1] + p3[1]) * t3)
                out.append((x, y))
        out.append(pts[-1])
        return out
    # de Boor for a control-point curve (clamped uniform knots)
    pts = list(points)
    k = min(degree, len(pts) - 1)
    m = len(pts) + k + 1
    knots = [0.0] * (k + 1) + [i / (len(pts) - k) for i in range(1, len(pts) - k)] + [1.0] * (k + 1)
    out = []
    for s in range(n + 1):
        t = s / n
        if t >= 1.0:
            out.append(pts[-1])
            continue
        # find span
        span = k
        while span < len(pts) - 1 and t >= knots[span + 1]:
            span += 1
        d = [pts[j + span - k] for j in range(k + 1)]
        for r in range(1, k + 1):
            for j in range(k, r - 1, -1):
                i = j + span - k
                denom = knots[i + k - r + 1] - knots[i]
                alpha = 0.0 if denom == 0 else (t - knots[i]) / denom
                d[j] = ((1 - alpha) * d[j - 1][0] + alpha * d[j][0], (1 - alpha) * d[j - 1][1] + alpha * d[j][1])
        out.append(d[k])
    return out


@dataclass
class Sketch:
    plane: Plane = field(default_factory=Plane.xy)
    curves: list[Curve] = field(default_factory=list)
    name: str = "Sketch"

    # -- creation --------------------------------------------------------
    def line(self, a: Vec2, b: Vec2) -> Curve:
        return self._add(Curve("line", [a, b]))

    def polyline(self, points: Sequence[Vec2], closed: bool = False) -> Curve:
        return self._add(Curve("polyline", list(points), closed=closed))

    def spline(self, points: Sequence[Vec2], closed: bool = False) -> Curve:
        return self._add(Curve("spline", list(points), closed=closed))

    def control_curve(self, points: Sequence[Vec2], degree: int = 3, closed: bool = False) -> Curve:
        return self._add(Curve("control", list(points), degree=degree, closed=closed))

    def circle(self, center: Vec2, radius: float) -> Curve:
        return self._add(Curve("circle", center=center, radius=radius))

    def circle_two_point(self, a: Vec2, b: Vec2) -> Curve:
        return self.circle(((a[0] + b[0]) / 2, (a[1] + b[1]) / 2), _p2(a, b) / 2)

    def circle_three_point(self, a: Vec2, b: Vec2, c: Vec2) -> Curve:
        center, r = circumcircle(a, b, c)
        return self.circle(center, r)

    def circle_tangent(self, curves: Sequence[Curve], radius: Optional[float] = None, near: Vec2 = (0.0, 0.0)) -> Curve:
        """Circle tangent to two or three curves, solved numerically from the
        sampled curves: the center equidistant (by `radius`) from each."""
        samples = [c.sample(64) for c in curves]

        def dist_to(poly: list[Vec2], p: Vec2) -> float:
            best = math.inf
            for i in range(len(poly) - 1):
                best = min(best, _seg_dist(poly[i], poly[i + 1], p))
            return best

        if radius is None:
            if len(curves) < 3:
                raise KernelError("a tangent circle to two curves needs a radius; give three curves for a fully determined circle")

            def residual(c: Vec2):
                ds = [dist_to(s, c) for s in samples]
                return (ds[0] - ds[1]) ** 2 + (ds[1] - ds[2]) ** 2, sum(ds) / 3
        else:
            def residual(c: Vec2):
                ds = [dist_to(s, c) for s in samples]
                return sum((d - radius) ** 2 for d in ds), radius
        center = _minimize2(lambda c: residual(c)[0], near)
        return self.circle(center, residual(center)[1])

    def ellipse(self, center: Vec2, radius_x: float, radius_y: float, rotation: float = 0.0) -> Curve:
        return self._add(Curve("ellipse", center=center, radius=radius_x, radius2=radius_y, rotation=rotation))

    def arc(self, center: Vec2, radius: float, start_deg: float, end_deg: float) -> Curve:
        return self._add(Curve("arc", center=center, radius=radius, start_angle=start_deg, end_angle=end_deg))

    def arc_three_point(self, a: Vec2, b: Vec2, c: Vec2) -> Curve:
        center, r = circumcircle(a, b, c)
        a0 = math.degrees(math.atan2(a[1] - center[1], a[0] - center[0]))
        a1 = math.degrees(math.atan2(b[1] - center[1], b[0] - center[0]))
        a2 = math.degrees(math.atan2(c[1] - center[1], c[0] - center[0]))
        # Sweep from a through b to c.
        sweep = (a2 - a0) % 360
        mid = (a1 - a0) % 360
        if mid > sweep:
            sweep -= 360
        return self.arc(center, r, a0, a0 + sweep)

    def arc_tangent(self, prev: Curve, end: Vec2) -> Curve:
        """Arc starting at the end of `prev`, tangent to it, ending at `end`."""
        p0 = prev.end()
        pts = prev.sample(8)
        t = v2_unit((pts[-1][0] - pts[-2][0], pts[-1][1] - pts[-2][1]))
        chord = (end[0] - p0[0], end[1] - p0[1])
        n = (-t[1], t[0])
        d = chord[0] * n[0] + chord[1] * n[1]
        if abs(d) < 1e-9:
            return self.line(p0, end)
        r = (chord[0] ** 2 + chord[1] ** 2) / (2 * d)
        center = (p0[0] + n[0] * r, p0[1] + n[1] * r)
        a0 = math.degrees(math.atan2(p0[1] - center[1], p0[0] - center[0]))
        a1 = math.degrees(math.atan2(end[1] - center[1], end[0] - center[0]))
        if r > 0:
            sweep = (a1 - a0) % 360
        else:
            sweep = -((a0 - a1) % 360)
        return self.arc(center, abs(r), a0, a0 + sweep)

    def rectangle(self, corner: Vec2, size: Vec2) -> Curve:
        x, y = corner
        w, h = size
        return self.polyline([(x, y), (x + w, y), (x + w, y + h), (x, y + h)], closed=True)

    def rectangle_center(self, center: Vec2, size: Vec2) -> Curve:
        return self.rectangle((center[0] - size[0] / 2, center[1] - size[1] / 2), size)

    def rectangle_three_point(self, a: Vec2, b: Vec2, c: Vec2) -> Curve:
        ab = (b[0] - a[0], b[1] - a[1])
        n = v2_unit((-ab[1], ab[0]))
        h = (c[0] - b[0]) * n[0] + (c[1] - b[1]) * n[1]
        d = (n[0] * h, n[1] * h)
        return self.polyline([a, b, (b[0] + d[0], b[1] + d[1]), (a[0] + d[0], a[1] + d[1])], closed=True)

    last_polygon_sides: int = 6

    def polygon(self, center: Vec2, radius: float, sides: Optional[int] = None, rotation: float = 0.0) -> Curve:
        sides = sides or Sketch.last_polygon_sides
        Sketch.last_polygon_sides = sides
        pts = [(center[0] + radius * math.cos(math.radians(rotation) + 2 * math.pi * i / sides), center[1] + radius * math.sin(math.radians(rotation) + 2 * math.pi * i / sides)) for i in range(sides)]
        return self.polyline(pts, closed=True)

    def slot(self, a: Vec2, b: Vec2, width: float) -> Curve:
        """A stadium: a straight open curve offset symmetrically with capped ends."""
        d = v2_unit((b[0] - a[0], b[1] - a[1]))
        n = (-d[1], d[0])
        r = width / 2
        ang = math.degrees(math.atan2(d[1], d[0]))
        c = Curve("slot", points=[a, b], radius=r)
        c.closed = True
        c.rotation = ang
        return self._add(c)

    def spiral(self, center: Vec2, start_radius: float, end_radius: float, turns: float, points_per_turn: int = 36) -> Curve:
        n = max(int(turns * points_per_turn), 8)
        pts = []
        for i in range(n + 1):
            t = i / n
            a = 2 * math.pi * turns * t
            r = start_radius + (end_radius - start_radius) * t
            pts.append((center[0] + r * math.cos(a), center[1] + r * math.sin(a)))
        return self.spline(pts)

    def text(self, origin: Vec2, text: str, height: float = 10.0, font: str = "") -> list[Curve]:
        """Text as closed outlines from a system font (fontTools)."""
        outlines = text_outlines(text, height, font)
        out = []
        for poly in outlines:
            out.append(self._add(Curve("polyline", [(origin[0] + x, origin[1] + y) for x, y in poly], closed=True)))
        return out

    def _add(self, c: Curve) -> Curve:
        self.curves.append(c)
        return c

    # -- editing ---------------------------------------------------------
    def remove(self, curve: Curve):
        self.curves.remove(curve)

    def reverse(self, curve: Curve):
        i = self.curves.index(curve)
        self.curves[i] = curve.reversed()

    def split_at(self, curve: Curve, point: Vec2) -> list[Curve]:
        """Split at the parameter nearest `point`; returns the two pieces."""
        pieces = _split_curve(curve, point)
        i = self.curves.index(curve)
        self.curves[i : i + 1] = pieces
        return pieces

    def trim(self, curve: Curve, cutters: Sequence[Curve], click: Vec2) -> Optional[Curve]:
        """Remove the part of `curve` under `click` between its intersections with `cutters`."""
        xs = []
        for c in cutters:
            xs.extend(intersections(curve, c))
        if not xs:
            self.remove(curve)
            return None
        pts = curve.sample(128)
        # parameter along the sampled polyline
        def param(p: Vec2) -> float:
            best, bt = math.inf, 0.0
            acc = 0.0
            lens = [_p2(pts[i], pts[i + 1]) for i in range(len(pts) - 1)]
            total = sum(lens) or 1.0
            for i in range(len(pts) - 1):
                d = _seg_dist(pts[i], pts[i + 1], p)
                if d < best:
                    best = d
                    proj = _seg_param(pts[i], pts[i + 1], p)
                    bt = (acc + proj * lens[i]) / total
                acc += lens[i]
            return bt

        tx = sorted(param(x) for x in xs)
        tc = param(click)
        lo = max([t for t in tx if t <= tc], default=0.0)
        hi = min([t for t in tx if t >= tc], default=1.0)
        kept = []
        for a, b in ((0.0, lo), (hi, 1.0)):
            if b - a > 1e-6:
                kept.append(_sub_curve(curve, a, b))
        i = self.curves.index(curve)
        self.curves[i : i + 1] = kept
        return kept[0] if kept else None

    def extend(self, curve: Curve, targets: Sequence[Curve], both: bool = True) -> Curve:
        """Extend the ends of a line/polyline/arc to the nearest target curve."""
        if curve.kind not in ("line", "polyline", "arc"):
            raise KernelError("only lines, polylines and arcs can be extended")
        ends = [True, False] if both else [False]
        for at_start in ends:
            self._extend_end(curve, targets, at_start)
        return curve

    def _extend_end(self, curve: Curve, targets: Sequence[Curve], at_start: bool):
        if curve.kind == "arc":
            # Grow the arc angle until it meets a target.
            for delta in range(1, 360):
                trial = Curve("arc", center=curve.center, radius=curve.radius, start_angle=curve.start_angle - (delta if at_start else 0), end_angle=curve.end_angle + (0 if at_start else delta))
                hits = [x for t in targets for x in intersections(trial, t)]
                if hits:
                    if at_start:
                        curve.start_angle -= delta
                    else:
                        curve.end_angle += delta
                    return
            return
        pts = curve.points
        if at_start:
            p, q = pts[0], pts[1]
        else:
            p, q = pts[-1], pts[-2]
        d = v2_unit((p[0] - q[0], p[1] - q[1]))
        ray = Curve("line", [p, (p[0] + d[0] * 1e4, p[1] + d[1] * 1e4)])
        hits = [x for t in targets for x in intersections(ray, t)]
        if not hits:
            return
        best = min(hits, key=lambda h: _p2(h, p))
        if at_start:
            pts[0] = best
        else:
            pts[-1] = best

    def fillet_corner(self, curve: Curve, vertex_index: int, radius: float) -> Curve:
        """Round one corner of a polyline (or the corner of two lines joined)."""
        if curve.kind != "polyline":
            raise KernelError("corner fillet works on polylines; join the two lines first")
        pts = list(curve.points)
        n = len(pts)
        i = vertex_index % n
        if not curve.closed and (i == 0 or i == n - 1):
            raise KernelError("cannot fillet an end vertex")
        p0, p1, p2 = pts[i - 1], pts[i], pts[(i + 1) % n]
        d0 = v2_unit((p0[0] - p1[0], p0[1] - p1[1]))
        d1 = v2_unit((p2[0] - p1[0], p2[1] - p1[1]))
        cosang = max(-1.0, min(1.0, d0[0] * d1[0] + d0[1] * d1[1]))
        ang = math.acos(cosang)
        if ang < 1e-6 or abs(ang - math.pi) < 1e-6:
            raise KernelError("corner is straight")
        t = radius / math.tan(ang / 2)
        if t > _p2(p0, p1) or t > _p2(p1, p2):
            raise KernelError(f"fillet radius {radius} is too large for this corner")
        a = (p1[0] + d0[0] * t, p1[1] + d0[1] * t)
        b = (p1[0] + d1[0] * t, p1[1] + d1[1] * t)
        bis = v2_unit((d0[0] + d1[0], d0[1] + d1[1]))
        dist_c = radius / math.sin(ang / 2)
        center = (p1[0] + bis[0] * dist_c, p1[1] + bis[1] * dist_c)
        # Replace the corner with an arc: the sketch keeps it as points on the arc.
        a0 = math.atan2(a[1] - center[1], a[0] - center[0])
        a1 = math.atan2(b[1] - center[1], b[0] - center[0])
        sweep = (a1 - a0 + math.pi) % (2 * math.pi) - math.pi
        arc_pts = [(center[0] + radius * math.cos(a0 + sweep * k / 8), center[1] + radius * math.sin(a0 + sweep * k / 8)) for k in range(9)]
        pts[i : i + 1] = arc_pts
        curve.points = pts
        return curve

    def offset(self, curve: Curve, distance: float) -> Curve:
        """Offset a closed or open curve by `distance` (positive = left of travel)."""
        pts = curve.sample(64 if curve.kind not in ("line", "polyline") else 1)
        if curve.kind == "polyline" and curve.closed:
            pts = pts[:-1]
        out = _offset_polyline(pts, distance, curve.closed or curve.kind in ("circle", "ellipse"))
        return self._add(Curve("polyline", out, closed=curve.closed or curve.kind in ("circle", "ellipse")))

    def join(self, curves: Sequence[Curve]) -> Curve:
        """Chain curves end to end into one polyline (sampled for arcs/splines)."""
        pts: list[Vec2] = []
        remaining = list(curves)
        current = remaining.pop(0)
        pts.extend(current.sample(32) if current.kind not in ("line", "polyline") else current.points)
        while remaining:
            end = pts[-1]
            best = min(remaining, key=lambda c: min(_p2(c.start(), end), _p2(c.end(), end)))
            remaining.remove(best)
            seq = best.sample(32) if best.kind not in ("line", "polyline") else list(best.points)
            if _p2(best.end(), end) < _p2(best.start(), end):
                seq = list(reversed(seq))
            pts.extend(seq[1:])
        closed = _p2(pts[0], pts[-1]) < 1e-6
        if closed:
            pts = pts[:-1]
        for c in curves:
            if c in self.curves:
                self.curves.remove(c)
        return self._add(Curve("polyline", pts, closed=closed))

    def unjoin(self, curve: Curve) -> list[Curve]:
        if curve.kind != "polyline":
            return [curve]
        pts = curve.points + ([curve.points[0]] if curve.closed else [])
        lines = [Curve("line", [pts[i], pts[i + 1]]) for i in range(len(pts) - 1)]
        i = self.curves.index(curve)
        self.curves[i : i + 1] = lines
        return lines

    def insert_vertex(self, curve: Curve, after: int, point: Vec2):
        curve.points.insert(after + 1, point)

    def remove_vertex(self, curve: Curve, index: int):
        if len(curve.points) <= 2:
            raise KernelError("a curve needs at least two points")
        del curve.points[index]

    def rebuild(self, curve: Curve, degree: int, spans: int) -> Curve:
        pts = curve.sample(max(spans * 4, 16))
        step = max(1, len(pts) // (spans + degree))
        ctrl = pts[::step]
        if ctrl[-1] != pts[-1]:
            ctrl.append(pts[-1])
        i = self.curves.index(curve)
        new = Curve("control", ctrl, degree=degree, closed=curve.closed)
        self.curves[i] = new
        return new

    # -- to the kernel ---------------------------------------------------
    def world(self, p: Vec2) -> Vec3:
        return self.plane.to_world(p[0], p[1])

    def to_wires(self, curves: Optional[Sequence[Curve]] = None) -> list[Body]:
        return [Body(self._wire(c), "wire") for c in (curves or self.curves)]

    def to_body(self, curves: Optional[Sequence[Curve]] = None) -> Body:
        """One body holding all curves as a compound of wires (a face is
        made by the kernel from a closed one)."""
        wires = [self._wire(c) for c in (curves or self.curves)]
        if len(wires) == 1:
            return Body(wires[0], "wire")
        from .occt import _compound

        return Body(_compound(wires), "wire")

    def to_face(self, outer: Curve, holes: Sequence[Curve] = ()) -> Body:
        mk = BRepBuilderAPI_MakeFace(self._wire(outer), True)
        if not mk.IsDone():
            raise KernelError("the outer curve is not closed")
        face = mk.Face()
        if holes:
            # Holes as face booleans: orientation-proof.
            from OCP.BRepAlgoAPI import BRepAlgoAPI_Cut

            for h in holes:
                hk = BRepBuilderAPI_MakeFace(self._wire(h), True)
                if not hk.IsDone():
                    raise KernelError("a hole curve is not closed")
                cut = BRepAlgoAPI_Cut(face, hk.Face())
                cut.Build()
                if not cut.IsDone():
                    raise KernelError("could not cut the hole from the profile")
                from OCP.TopAbs import TopAbs_FACE
                from OCP.TopExp import TopExp_Explorer
                from OCP.TopoDS import TopoDS

                ex = TopExp_Explorer(cut.Shape(), TopAbs_FACE)
                face = TopoDS.Face_s(ex.Current())
        return Body(face, "sheet")

    def _pt(self, p: Vec2) -> gp_Pnt:
        w = self.world(p)
        return gp_Pnt(*w)

    def _wire(self, c: Curve) -> TopoDS_Wire:
        mk = BRepBuilderAPI_MakeWire()
        n = gp_Dir(*self.plane.normal)
        if c.kind == "line":
            mk.Add(BRepBuilderAPI_MakeEdge(self._pt(c.points[0]), self._pt(c.points[1])).Edge())
        elif c.kind == "polyline":
            pts = c.points + ([c.points[0]] if c.closed else [])
            for i in range(len(pts) - 1):
                if _p2(pts[i], pts[i + 1]) > 1e-9:
                    mk.Add(BRepBuilderAPI_MakeEdge(self._pt(pts[i]), self._pt(pts[i + 1])).Edge())
        elif c.kind == "circle":
            circ = gp_Circ(gp_Ax2(self._pt(c.center), n, gp_Dir(*self.plane.x_axis)), c.radius)
            mk.Add(BRepBuilderAPI_MakeEdge(circ).Edge())
        elif c.kind == "arc":
            circ = gp_Circ(gp_Ax2(self._pt(c.center), n, gp_Dir(*self.plane.x_axis)), c.radius)
            a0, a1 = math.radians(c.start_angle), math.radians(c.end_angle)
            if a1 < a0:
                circ = gp_Circ(gp_Ax2(self._pt(c.center), gp_Dir(*v_scale(self.plane.normal, -1.0)), gp_Dir(*self.plane.x_axis)), c.radius)
                a0, a1 = -a0, -a1
            mk.Add(BRepBuilderAPI_MakeEdge(circ, a0, a1).Edge())
        elif c.kind == "ellipse":
            xdir = (math.cos(math.radians(c.rotation)), math.sin(math.radians(c.rotation)))
            xw = v_add(v_scale(self.plane.x_axis, xdir[0]), v_scale(self.plane.y_axis, xdir[1]))
            big, small = max(c.radius, c.radius2), min(c.radius, c.radius2)
            if c.radius < c.radius2:
                xw = v_cross(self.plane.normal, xw)
            el = gp_Elips(gp_Ax2(self._pt(c.center), n, gp_Dir(*xw)), big, small)
            mk.Add(BRepBuilderAPI_MakeEdge(el).Edge())
        elif c.kind == "spline":
            pts = c.points
            arr = TColgp_HArray1OfPnt(1, len(pts))
            for i, p in enumerate(pts, start=1):
                arr.SetValue(i, self._pt(p))
            interp = GeomAPI_Interpolate(arr, c.closed, 1e-6)
            interp.Perform()
            mk.Add(BRepBuilderAPI_MakeEdge(interp.Curve()).Edge())
        elif c.kind == "control":
            pts = c.points + ([c.points[0]] if c.closed else [])
            k = min(c.degree, len(pts) - 1)
            arr = TColgp_Array1OfPnt(1, len(pts))
            for i, p in enumerate(pts, start=1):
                arr.SetValue(i, self._pt(p))
            nk = len(pts) - k + 1
            knots = TColStd_Array1OfReal(1, nk)
            mults = TColStd_Array1OfInteger(1, nk)
            for i in range(1, nk + 1):
                knots.SetValue(i, (i - 1) / (nk - 1))
                mults.SetValue(i, k + 1 if i in (1, nk) else 1)
            curve = Geom_BSplineCurve(arr, knots, mults, k)
            mk.Add(BRepBuilderAPI_MakeEdge(curve).Edge())
        elif c.kind == "slot":
            a, b = c.points
            d = v2_unit((b[0] - a[0], b[1] - a[1]))
            nn = (-d[1], d[0])
            r = c.radius
            pA1 = (a[0] + nn[0] * r, a[1] + nn[1] * r)
            pB1 = (b[0] + nn[0] * r, b[1] + nn[1] * r)
            pB2 = (b[0] - nn[0] * r, b[1] - nn[1] * r)
            pA2 = (a[0] - nn[0] * r, a[1] - nn[1] * r)
            mk.Add(BRepBuilderAPI_MakeEdge(self._pt(pA1), self._pt(pB1)).Edge())
            capB = GC_MakeArcOfCircle(self._pt(pB1), self._pt((b[0] + d[0] * r, b[1] + d[1] * r)), self._pt(pB2)).Value()
            mk.Add(BRepBuilderAPI_MakeEdge(capB).Edge())
            mk.Add(BRepBuilderAPI_MakeEdge(self._pt(pB2), self._pt(pA2)).Edge())
            capA = GC_MakeArcOfCircle(self._pt(pA2), self._pt((a[0] - d[0] * r, a[1] - d[1] * r)), self._pt(pA1)).Value()
            mk.Add(BRepBuilderAPI_MakeEdge(capA).Edge())
        else:
            raise KernelError(f"unknown curve kind {c.kind}")
        if not mk.IsDone():
            raise KernelError(f"could not build the {c.kind}")
        return mk.Wire()

    def to_json(self) -> dict:
        return {"name": self.name, "plane": self.plane.to_json(), "curves": [c.to_json() for c in self.curves]}

    @staticmethod
    def from_json(d: dict) -> "Sketch":
        return Sketch(Plane.from_json(d["plane"]), [Curve.from_json(c) for c in d["curves"]], d.get("name", "Sketch"))


# ------------------------------------------------------------- 2D helpers


def v2_unit(v: Vec2) -> Vec2:
    n = math.hypot(v[0], v[1])
    return (1.0, 0.0) if n < 1e-12 else (v[0] / n, v[1] / n)


def circumcircle(a: Vec2, b: Vec2, c: Vec2) -> tuple[Vec2, float]:
    d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]))
    if abs(d) < 1e-12:
        raise KernelError("the three points are collinear")
    ux = ((a[0] ** 2 + a[1] ** 2) * (b[1] - c[1]) + (b[0] ** 2 + b[1] ** 2) * (c[1] - a[1]) + (c[0] ** 2 + c[1] ** 2) * (a[1] - b[1])) / d
    uy = ((a[0] ** 2 + a[1] ** 2) * (c[0] - b[0]) + (b[0] ** 2 + b[1] ** 2) * (a[0] - c[0]) + (c[0] ** 2 + c[1] ** 2) * (b[0] - a[0])) / d
    return (ux, uy), _p2((ux, uy), a)


def _seg_param(a: Vec2, b: Vec2, p: Vec2) -> float:
    ab = (b[0] - a[0], b[1] - a[1])
    l2 = ab[0] ** 2 + ab[1] ** 2
    if l2 < 1e-18:
        return 0.0
    return max(0.0, min(1.0, ((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2))


def _seg_dist(a: Vec2, b: Vec2, p: Vec2) -> float:
    t = _seg_param(a, b, p)
    q = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)
    return _p2(q, p)


def _seg_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Optional[Vec2]:
    r = (b[0] - a[0], b[1] - a[1])
    s = (d[0] - c[0], d[1] - c[1])
    den = r[0] * s[1] - r[1] * s[0]
    if abs(den) < 1e-12:
        return None
    qp = (c[0] - a[0], c[1] - a[1])
    t = (qp[0] * s[1] - qp[1] * s[0]) / den
    u = (qp[0] * r[1] - qp[1] * r[0]) / den
    if -1e-9 <= t <= 1 + 1e-9 and -1e-9 <= u <= 1 + 1e-9:
        return (a[0] + r[0] * t, a[1] + r[1] * t)
    return None


def intersections(p: Curve, q: Curve) -> list[Vec2]:
    A = p.sample(128)
    B = q.sample(128)
    out = []
    for i in range(len(A) - 1):
        for j in range(len(B) - 1):
            x = _seg_intersect(A[i], A[i + 1], B[j], B[j + 1])
            if x is not None and all(_p2(x, y) > 1e-6 for y in out):
                out.append(x)
    return out


def _sub_curve(c: Curve, t0: float, t1: float) -> Curve:
    if c.kind == "arc":
        a0 = c.start_angle + (c.end_angle - c.start_angle) * t0
        a1 = c.start_angle + (c.end_angle - c.start_angle) * t1
        return Curve("arc", center=c.center, radius=c.radius, start_angle=a0, end_angle=a1)
    if c.kind == "circle":
        return Curve("arc", center=c.center, radius=c.radius, start_angle=360 * t0, end_angle=360 * t1)
    pts = c.sample(128)
    lens = [_p2(pts[i], pts[i + 1]) for i in range(len(pts) - 1)]
    total = sum(lens) or 1.0
    out = []
    acc = 0.0
    for i in range(len(pts) - 1):
        s0, s1 = acc / total, (acc + lens[i]) / total
        if s1 >= t0 and s0 <= t1:
            u0 = max(0.0, (t0 - s0) / (s1 - s0)) if s1 > s0 else 0.0
            u1 = min(1.0, (t1 - s0) / (s1 - s0)) if s1 > s0 else 1.0
            a = (pts[i][0] + (pts[i + 1][0] - pts[i][0]) * u0, pts[i][1] + (pts[i + 1][1] - pts[i][1]) * u0)
            b = (pts[i][0] + (pts[i + 1][0] - pts[i][0]) * u1, pts[i][1] + (pts[i + 1][1] - pts[i][1]) * u1)
            if not out:
                out.append(a)
            out.append(b)
        acc += lens[i]
    if c.kind == "line" and len(out) >= 2:
        return Curve("line", [out[0], out[-1]])
    return Curve("polyline", out)


def _split_curve(c: Curve, point: Vec2) -> list[Curve]:
    pts = c.sample(128)
    lens = [_p2(pts[i], pts[i + 1]) for i in range(len(pts) - 1)]
    total = sum(lens) or 1.0
    best, bt, acc = math.inf, 0.0, 0.0
    for i in range(len(pts) - 1):
        d = _seg_dist(pts[i], pts[i + 1], point)
        if d < best:
            best = d
            bt = (acc + _seg_param(pts[i], pts[i + 1], point) * lens[i]) / total
        acc += lens[i]
    return [_sub_curve(c, 0.0, bt), _sub_curve(c, bt, 1.0)]


def _offset_polyline(pts: list[Vec2], d: float, closed: bool) -> list[Vec2]:
    n = len(pts)
    out = []
    for i in range(n):
        p = pts[i]
        prev = pts[i - 1] if (i > 0 or closed) else None
        nxt = pts[(i + 1) % n] if (i < n - 1 or closed) else None
        normals = []
        for a, b in ((prev, p), (p, nxt)):
            if a is None or b is None:
                continue
            t = v2_unit((b[0] - a[0], b[1] - a[1]))
            normals.append((-t[1], t[0]))
        if not normals:
            out.append(p)
            continue
        nx = sum(v[0] for v in normals) / len(normals)
        ny = sum(v[1] for v in normals) / len(normals)
        nn = v2_unit((nx, ny))
        # Miter scaling so the offset stays parallel to both segments.
        scale = 1.0
        if len(normals) == 2:
            cosang = normals[0][0] * nn[0] + normals[0][1] * nn[1]
            scale = 1.0 / max(cosang, 0.2)
        out.append((p[0] + nn[0] * d * scale, p[1] + nn[1] * d * scale))
    return out


def _minimize2(f, x0: Vec2, iters: int = 200) -> Vec2:
    """Nelder-Mead in the plane."""
    pts = [x0, (x0[0] + 5.0, x0[1]), (x0[0], x0[1] + 5.0)]
    vals = [f(p) for p in pts]
    for _ in range(iters):
        order = sorted(range(3), key=lambda i: vals[i])
        pts = [pts[i] for i in order]
        vals = [vals[i] for i in order]
        cx = ((pts[0][0] + pts[1][0]) / 2, (pts[0][1] + pts[1][1]) / 2)
        refl = (cx[0] + (cx[0] - pts[2][0]), cx[1] + (cx[1] - pts[2][1]))
        fr = f(refl)
        if fr < vals[0]:
            exp = (cx[0] + 2 * (cx[0] - pts[2][0]), cx[1] + 2 * (cx[1] - pts[2][1]))
            fe = f(exp)
            pts[2], vals[2] = (exp, fe) if fe < fr else (refl, fr)
        elif fr < vals[1]:
            pts[2], vals[2] = refl, fr
        else:
            con = (cx[0] + 0.5 * (pts[2][0] - cx[0]), cx[1] + 0.5 * (pts[2][1] - cx[1]))
            fc = f(con)
            if fc < vals[2]:
                pts[2], vals[2] = con, fc
            else:
                for i in (1, 2):
                    pts[i] = ((pts[0][0] + pts[i][0]) / 2, (pts[0][1] + pts[i][1]) / 2)
                    vals[i] = f(pts[i])
    return min(zip(vals, pts))[1]


def text_outlines(text: str, height: float, font: str = "") -> list[list[Vec2]]:
    """Glyph outlines as polylines via fontTools, from a system font."""
    import glob
    import os

    from fontTools.pens.recordingPen import RecordingPen
    from fontTools.ttLib import TTFont

    candidates = [font] if font else []
    for pattern in ("/System/Library/Fonts/Supplemental/Arial.ttf", "/System/Library/Fonts/Helvetica.ttc", "/Library/Fonts/Arial.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "C:/Windows/Fonts/arial.ttf"):
        candidates.append(pattern)
    candidates.extend(glob.glob("/usr/share/fonts/**/*.ttf", recursive=True)[:3])
    path = next((c for c in candidates if c and os.path.exists(c)), None)
    if path is None:
        raise KernelError("no TrueType font found for text")
    tt = TTFont(path, fontNumber=0)
    glyphset = tt.getGlyphSet()
    cmap = tt.getBestCmap()
    upm = tt["head"].unitsPerEm
    scale = height / upm
    outlines = []
    x = 0.0
    for ch in text:
        name = cmap.get(ord(ch))
        if name is None:
            x += 0.5 * upm * scale
            continue
        g = glyphset[name]
        pen = RecordingPen()
        g.draw(pen)
        contour: list[Vec2] = []
        last = (0.0, 0.0)
        for op, args in pen.value:
            if op == "moveTo":
                if len(contour) > 2:
                    outlines.append(contour)
                contour = [(x + args[0][0] * scale, args[0][1] * scale)]
                last = args[0]
            elif op == "lineTo":
                contour.append((x + args[0][0] * scale, args[0][1] * scale))
                last = args[0]
            elif op in ("qCurveTo", "curveTo"):
                pts = [last] + list(args)
                for k in range(1, 9):
                    t = k / 8
                    # de Casteljau on the control polygon
                    tmp = [(p[0], p[1]) for p in pts]
                    while len(tmp) > 1:
                        tmp = [((1 - t) * tmp[i][0] + t * tmp[i + 1][0], (1 - t) * tmp[i][1] + t * tmp[i + 1][1]) for i in range(len(tmp) - 1)]
                    contour.append((x + tmp[0][0] * scale, tmp[0][1] * scale))
                last = args[-1]
            elif op in ("closePath", "endPath"):
                if len(contour) > 2:
                    outlines.append(contour)
                contour = []
        if len(contour) > 2:
            outlines.append(contour)
        x += g.width * scale
    return outlines
