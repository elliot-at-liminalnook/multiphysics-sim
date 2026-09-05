"""The geometry kernel behind an interface.

Everything above this layer (document, commands, UI) speaks in terms of
`Body` handles, face/edge/vertex references and plain data (points,
vectors, meshes, reports), never in kernel types. `OcctKernel` is the one
implementation; a second could be dropped in behind `GeometryKernel`.

Topology references are *geometric*, not pointers: a `FaceRef` carries the
surface kind, centroid, normal and area of the face it named, and a kernel
re-finds the closest match after an edit. That is what makes direct
modeling possible without a feature tree — the face you were editing is
still "the face at that place" after the boolean that rebuilt the solid.
"""

from __future__ import annotations

import math
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Iterable, Optional, Sequence

Vec3 = tuple[float, float, float]


def v_add(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def v_sub(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def v_scale(a: Vec3, s: float) -> Vec3:
    return (a[0] * s, a[1] * s, a[2] * s)


def v_dot(a: Vec3, b: Vec3) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def v_cross(a: Vec3, b: Vec3) -> Vec3:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def v_norm(a: Vec3) -> float:
    return math.sqrt(v_dot(a, a))


def v_unit(a: Vec3) -> Vec3:
    n = v_norm(a)
    return (0.0, 0.0, 1.0) if n < 1e-12 else v_scale(a, 1.0 / n)


def v_dist(a: Vec3, b: Vec3) -> float:
    return v_norm(v_sub(a, b))


class SurfaceKind(str, Enum):
    PLANE = "plane"
    CYLINDER = "cylinder"
    CONE = "cone"
    SPHERE = "sphere"
    TORUS = "torus"
    BSPLINE = "bspline"
    BEZIER = "bezier"
    OTHER = "other"


class CurveKind(str, Enum):
    LINE = "line"
    CIRCLE = "circle"
    ELLIPSE = "ellipse"
    BSPLINE = "bspline"
    OTHER = "other"


@dataclass(frozen=True)
class Plane:
    """A construction plane: origin, normal and an in-plane x axis."""

    origin: Vec3 = (0.0, 0.0, 0.0)
    normal: Vec3 = (0.0, 0.0, 1.0)
    x_axis: Vec3 = (1.0, 0.0, 0.0)

    @property
    def y_axis(self) -> Vec3:
        return v_unit(v_cross(self.normal, self.x_axis))

    def to_world(self, u: float, v: float, w: float = 0.0) -> Vec3:
        return v_add(v_add(v_add(self.origin, v_scale(self.x_axis, u)), v_scale(self.y_axis, v)), v_scale(self.normal, w))

    def to_local(self, p: Vec3) -> tuple[float, float, float]:
        d = v_sub(p, self.origin)
        return (v_dot(d, self.x_axis), v_dot(d, self.y_axis), v_dot(d, self.normal))

    def project(self, p: Vec3) -> Vec3:
        u, v, _ = self.to_local(p)
        return self.to_world(u, v)

    @staticmethod
    def xy(z: float = 0.0) -> "Plane":
        return Plane((0.0, 0.0, z), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0))

    @staticmethod
    def xz(y: float = 0.0) -> "Plane":
        return Plane((0.0, y, 0.0), (0.0, -1.0, 0.0), (1.0, 0.0, 0.0))

    @staticmethod
    def yz(x: float = 0.0) -> "Plane":
        return Plane((x, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0))

    @staticmethod
    def from_normal(origin: Vec3, normal: Vec3) -> "Plane":
        n = v_unit(normal)
        helper = (0.0, 0.0, 1.0) if abs(n[2]) < 0.9 else (1.0, 0.0, 0.0)
        x = v_unit(v_cross(helper, n))
        return Plane(origin, n, x)

    @staticmethod
    def from_three_points(a: Vec3, b: Vec3, c: Vec3) -> "Plane":
        n = v_unit(v_cross(v_sub(b, a), v_sub(c, a)))
        return Plane(a, n, v_unit(v_sub(b, a)))

    @staticmethod
    def midplane(p: "Plane", q: "Plane") -> "Plane":
        origin = v_scale(v_add(p.origin, q.origin), 0.5)
        n = p.normal if v_dot(p.normal, q.normal) >= 0 else v_scale(p.normal, -1.0)
        return Plane(origin, v_unit(v_add(n, q.normal if v_dot(p.normal, q.normal) >= 0 else v_scale(q.normal, -1.0))), p.x_axis)

    def to_json(self) -> dict:
        return {"origin": list(self.origin), "normal": list(self.normal), "x_axis": list(self.x_axis)}

    @staticmethod
    def from_json(d: dict) -> "Plane":
        return Plane(tuple(d["origin"]), tuple(d["normal"]), tuple(d["x_axis"]))


@dataclass(frozen=True)
class FaceRef:
    """A face named by where it is and what it is."""

    kind: SurfaceKind
    centroid: Vec3
    normal: Vec3
    area: float
    # For cylinders/cones/spheres/tori: axis point, axis direction, radius.
    axis_point: Optional[Vec3] = None
    axis_dir: Optional[Vec3] = None
    radius: Optional[float] = None
    index: int = -1  # index in the body's face list at the time of query
    # The surface point where `normal` was evaluated (a full cylinder's
    # centroid sits on its axis, so the centroid cannot serve).
    point: Optional[Vec3] = None

    def to_json(self) -> dict:
        return {"kind": self.kind.value, "centroid": list(self.centroid), "normal": list(self.normal), "area": self.area, "axis_point": list(self.axis_point) if self.axis_point else None, "axis_dir": list(self.axis_dir) if self.axis_dir else None, "radius": self.radius, "point": list(self.point) if self.point else None}

    @staticmethod
    def from_json(d: dict) -> "FaceRef":
        return FaceRef(SurfaceKind(d["kind"]), tuple(d["centroid"]), tuple(d["normal"]), d["area"], tuple(d["axis_point"]) if d.get("axis_point") else None, tuple(d["axis_dir"]) if d.get("axis_dir") else None, d.get("radius"), -1, tuple(d["point"]) if d.get("point") else None)


@dataclass(frozen=True)
class EdgeRef:
    kind: CurveKind
    midpoint: Vec3
    length: float
    start: Vec3
    end: Vec3
    center: Optional[Vec3] = None
    radius: Optional[float] = None
    index: int = -1


@dataclass(frozen=True)
class VertexRef:
    point: Vec3
    index: int = -1


@dataclass
class Mesh:
    """A tessellation: flat float lists, triangles as index triples, and
    the face index each triangle came from (stable per-face IDs for the
    bridge and for picking)."""

    vertices: list[Vec3] = field(default_factory=list)
    normals: list[Vec3] = field(default_factory=list)
    triangles: list[tuple[int, int, int]] = field(default_factory=list)
    triangle_face: list[int] = field(default_factory=list)
    face_count: int = 0

    def bounds(self) -> tuple[Vec3, Vec3]:
        if not self.vertices:
            return ((0.0, 0.0, 0.0), (0.0, 0.0, 0.0))
        xs = [v[0] for v in self.vertices]
        ys = [v[1] for v in self.vertices]
        zs = [v[2] for v in self.vertices]
        return ((min(xs), min(ys), min(zs)), (max(xs), max(ys), max(zs)))


@dataclass
class ValidationIssue:
    severity: str  # 'error' | 'warning'
    message: str
    location: Optional[Vec3] = None
    fix: Optional[str] = None


@dataclass
class ValidationReport:
    valid: bool
    watertight: bool
    issues: list[ValidationIssue] = field(default_factory=list)

    def summary(self) -> str:
        if self.valid and self.watertight and not self.issues:
            return "valid, watertight"
        return "; ".join(f"{i.severity}: {i.message}" + (f" ({i.fix})" if i.fix else "") for i in self.issues) or ("valid" if self.valid else "invalid")


@dataclass
class MassProperties:
    volume: float
    area: float
    centroid: Vec3
    inertia: tuple[tuple[float, float, float], tuple[float, float, float], tuple[float, float, float]]
    bbox_min: Vec3
    bbox_max: Vec3

    def mass(self, density_g_cm3: float) -> float:
        """Grams from a density in g/cm³ (volume is in mm³)."""
        return self.volume * 1.0e-3 * density_g_cm3

    @property
    def size(self) -> Vec3:
        return v_sub(self.bbox_max, self.bbox_min)


class Body:
    """An opaque kernel body. The kernel owns the shape; the document owns
    the identity, name, material and placement metadata."""

    __slots__ = ("shape", "kind")

    def __init__(self, shape: Any, kind: str = "solid"):
        self.shape = shape
        self.kind = kind  # 'solid' | 'sheet' | 'wire'


class KernelError(RuntimeError):
    """A modeling operation failed: the message says why and, where the
    kernel can tell, what to change (a fillet radius too large, a boolean
    with coincident faces)."""


class BooleanOp(str, Enum):
    NEW = "new"
    UNION = "union"
    SUBTRACT = "subtract"
    INTERSECT = "intersect"


@dataclass
class SweepOptions:
    twist_deg: float = 0.0
    scale_end: float = 1.0
    corner: str = "round"  # 'round' | 'miter'
    frenet: bool = False


@dataclass
class ChamferSpec:
    distance: float
    distance2: Optional[float] = None
    angle_deg: Optional[float] = None


class GeometryKernel(ABC):
    # ---- primitives -----------------------------------------------------
    @abstractmethod
    def box(self, corner: Vec3, size: Vec3) -> Body: ...
    @abstractmethod
    def cylinder(self, base: Vec3, axis: Vec3, radius: float, height: float) -> Body: ...
    @abstractmethod
    def sphere(self, center: Vec3, radius: float) -> Body: ...

    # ---- from sketches --------------------------------------------------
    @abstractmethod
    def face_from_wire(self, wire: Body) -> Body: ...
    @abstractmethod
    def extrude(self, profile: Body, direction: Vec3, distance: float, taper_deg: float = 0.0, symmetric: bool = False) -> Body: ...
    @abstractmethod
    def extrude_up_to(self, profile: Body, direction: Vec3, target: Body) -> Body: ...
    @abstractmethod
    def revolve(self, profile: Body, axis_point: Vec3, axis_dir: Vec3, angle_deg: float = 360.0) -> Body: ...
    @abstractmethod
    def sweep(self, profile: Body, path: Body, options: SweepOptions = SweepOptions()) -> Body: ...
    @abstractmethod
    def pipe(self, path: Body, diameter: float) -> Body: ...
    @abstractmethod
    def loft(self, profiles: Sequence[Body], guides: Sequence[Body] = (), solid: bool = True, ruled: bool = False) -> Body: ...
    @abstractmethod
    def fill_hole(self, edges: Body) -> Body: ...
    @abstractmethod
    def bridge(self, edge_a: Body, edge_b: Body) -> Body: ...

    # ---- direct edits ---------------------------------------------------
    @abstractmethod
    def boolean(self, a: Body, b: Body, op: BooleanOp) -> Body: ...
    @abstractmethod
    def split(self, body: Body, cutter: Body) -> list[Body]: ...
    @abstractmethod
    def cut_with_plane(self, body: Body, plane: Plane, keep: str = "both") -> list[Body]: ...
    @abstractmethod
    def push_pull(self, body: Body, face: FaceRef, distance: float) -> Body: ...
    @abstractmethod
    def offset_faces(self, body: Body, faces: Sequence[FaceRef], distance: float) -> Body: ...
    @abstractmethod
    def offset_face_to_body(self, body: Body, face: FaceRef, target: Body, clearance: float = 0.0) -> Body: ...
    @abstractmethod
    def move_faces(self, body: Body, faces: Sequence[FaceRef], translation: Vec3) -> Body: ...
    @abstractmethod
    def rotate_faces(self, body: Body, faces: Sequence[FaceRef], axis_point: Vec3, axis_dir: Vec3, angle_deg: float) -> Body: ...
    @abstractmethod
    def set_cylinder_radius(self, body: Body, face: FaceRef, radius: float) -> Body: ...
    @abstractmethod
    def draft_faces(self, body: Body, faces: Sequence[FaceRef], pull_dir: Vec3, angle_deg: float, neutral: Plane) -> Body: ...
    @abstractmethod
    def delete_faces(self, body: Body, faces: Sequence[FaceRef]) -> Body: ...
    @abstractmethod
    def imprint(self, body: Body, tool: Body) -> Body: ...
    @abstractmethod
    def shell(self, body: Body, thickness: float, open_faces: Sequence[FaceRef]) -> Body: ...
    @abstractmethod
    def thicken(self, sheet: Body, thickness: float) -> Body: ...
    @abstractmethod
    def fillet(self, body: Body, edges: Sequence[EdgeRef], radius: float, radius_end: Optional[float] = None) -> Body: ...
    @abstractmethod
    def fillet_chordal(self, body: Body, edges: Sequence[EdgeRef], chord: float) -> Body: ...
    @abstractmethod
    def fillet_all(self, body: Body, radius: float, tension: float = 1.0) -> Body: ...
    @abstractmethod
    def full_round(self, body: Body, edge_a: EdgeRef, edge_b: EdgeRef) -> Body: ...
    @abstractmethod
    def remove_fillets(self, body: Body, faces: Sequence[FaceRef]) -> Body: ...
    @abstractmethod
    def chamfer(self, body: Body, edges: Sequence[EdgeRef], spec: ChamferSpec) -> Body: ...
    @abstractmethod
    def transform(self, body: Body, translation: Vec3 = (0.0, 0.0, 0.0), rotation_axis: Optional[Vec3] = None, rotation_deg: float = 0.0, rotation_center: Vec3 = (0.0, 0.0, 0.0), scale: float = 1.0, scale_center: Vec3 = (0.0, 0.0, 0.0)) -> Body: ...
    @abstractmethod
    def mirror(self, body: Body, plane: Plane) -> Body: ...
    @abstractmethod
    def copy(self, body: Body) -> Body: ...
    @abstractmethod
    def join(self, bodies: Sequence[Body]) -> Body: ...
    @abstractmethod
    def unjoin(self, body: Body) -> list[Body]: ...
    @abstractmethod
    def dissolve(self, body: Body) -> Body: ...

    def solid_inventory(self, body: Body) -> list[dict]:
        raise NotImplementedError('Solid inventory is unavailable for this kernel')

    def extract_components(self, body: Body, components: list[list[int]]) -> tuple[Body, list[Body]]:
        raise NotImplementedError('Component extraction is unavailable for this kernel')
    @abstractmethod
    def project_curve(self, wire: Body, body: Body, direction: Vec3) -> Body: ...
    @abstractmethod
    def silhouette(self, body: Body, plane: Plane) -> Body: ...

    # ---- queries --------------------------------------------------------
    @abstractmethod
    def faces(self, body: Body) -> list[FaceRef]: ...
    @abstractmethod
    def edges(self, body: Body) -> list[EdgeRef]: ...
    @abstractmethod
    def vertices(self, body: Body) -> list[VertexRef]: ...
    @abstractmethod
    def edges_of_face(self, body: Body, face: FaceRef) -> list[EdgeRef]: ...
    @abstractmethod
    def faces_of_edge(self, body: Body, edge: EdgeRef) -> list[FaceRef]: ...
    @abstractmethod
    def find_face(self, body: Body, ref: FaceRef) -> FaceRef: ...
    @abstractmethod
    def mass_properties(self, body: Body) -> MassProperties: ...

    def bounding_box(self, body: Body):
        p=self.mass_properties(body)
        return p.bbox_min,p.bbox_max

    def cylindrical_faces(self, body: Body):
        return [f for f in self.faces(body) if f.kind == SurfaceKind.CYLINDER]

    def inertial_properties(self, body: Body) -> MassProperties:
        """Mass integrals and bounds for dynamics; area may be omitted (zero)."""
        return self.mass_properties(body)

    @abstractmethod
    def moment_of_inertia(self, body: Body, point: Vec3, axis: Vec3) -> float:
        """Volume moment of inertia (mm⁵) about the axis through `point`."""
    @abstractmethod
    def tessellate(self, body: Body, tolerance: float = 0.05, angular_deg: float = 20.0) -> Mesh: ...
    @abstractmethod
    def validate(self, body: Body) -> ValidationReport: ...
    @abstractmethod
    def distance(self, a: Body, b: Body) -> tuple[float, Vec3, Vec3]: ...

    @abstractmethod
    def contains(self, body: Body, point: Vec3, tolerance: float = 1e-6) -> bool:
        """True when `point` is inside or on the solid."""
    @abstractmethod
    def section(self, body: Body, plane: Plane) -> list[list[Vec3]]: ...
    @abstractmethod
    def ray_hits(self, body: Body, origin: Vec3, direction: Vec3) -> list[tuple[float, Vec3, int]]: ...
    @abstractmethod
    def face_normal_at(self, body: Body, face: FaceRef, point: Vec3) -> Vec3: ...
    @abstractmethod
    def surface_curvature(self, body: Body, face: FaceRef, point: Vec3) -> tuple[float, float]: ...
    @abstractmethod
    def continuity(self, body: Body, edge: EdgeRef) -> str: ...
    @abstractmethod
    def control_points(self, body: Body, face: FaceRef) -> list[list[Vec3]]: ...
    @abstractmethod
    def set_control_points(self, body: Body, face: FaceRef, points: list[list[Vec3]]) -> Body: ...
    @abstractmethod
    def raise_degree(self, body: Body, face: FaceRef, degree_u: int, degree_v: int) -> Body: ...
    @abstractmethod
    def rebuild_face(self, body: Body, face: FaceRef, spans_u: int, spans_v: int, degree: int = 3) -> Body: ...
    @abstractmethod
    def sample_edge(self, edge: EdgeRef, body: Body, count: int) -> list[Vec3]: ...
    @abstractmethod
    def curvature_comb(self, wire: Body, samples: int = 64) -> list[tuple[Vec3, Vec3, float]]: ...

    # ---- persistence ----------------------------------------------------
    @abstractmethod
    def serialize(self, body: Body) -> bytes: ...
    @abstractmethod
    def deserialize(self, data: bytes, kind: str = "solid") -> Body: ...


def match_face(candidates: Iterable[FaceRef], ref: FaceRef) -> FaceRef:
    """The candidate closest to `ref`: same surface kind first, then by
    centroid distance and area difference. Used after every edit."""
    best = None
    best_score = math.inf
    for c in candidates:
        score = v_dist(c.centroid, ref.centroid) + 0.05 * abs(c.area - ref.area) / max(ref.area, 1e-6)
        if c.kind != ref.kind:
            score += 1e6
        if ref.normal and c.normal:
            score += 0.5 * (1.0 - v_dot(v_unit(c.normal), v_unit(ref.normal)))
        if score < best_score:
            best, best_score = c, score
    if best is None:
        raise KernelError("no faces to match")
    return best


def match_edge(candidates: Iterable[EdgeRef], ref: EdgeRef) -> EdgeRef:
    best = None
    best_score = math.inf
    for c in candidates:
        score = v_dist(c.midpoint, ref.midpoint) + 0.1 * abs(c.length - ref.length)
        if c.kind != ref.kind:
            score += 1e6
        if score < best_score:
            best, best_score = c, score
    if best is None:
        raise KernelError("no edges to match")
    return best
