"""Print-oriented helpers: fastener library, wall-thickness check,
manifold validation with actionable errors, and build-plate orientation
with overhang shading."""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Optional, Sequence

from .kernel import Body, BooleanOp, GeometryKernel, KernelError, Vec3
from .kernel.base import Mesh, ValidationIssue, ValidationReport, v_add, v_cross, v_dot, v_norm, v_scale, v_sub, v_unit

# --------------------------------------------------------------- fasteners

# ISO metric: nominal, clearance (medium fit), tap drill, counterbore (socket
# head) diameter/depth, countersink diameter, heat-set insert pocket
# (typical brass inserts: hole diameter / depth).
METRIC = {
    "M2": {"clearance": 2.4, "tap": 1.6, "cbore": (4.4, 2.2), "csk": 4.0, "insert": (3.2, 4.0), "head": 3.8},
    "M2.5": {"clearance": 2.9, "tap": 2.05, "cbore": (5.4, 2.7), "csk": 5.0, "insert": (3.8, 5.0), "head": 4.5},
    "M3": {"clearance": 3.4, "tap": 2.5, "cbore": (6.5, 3.2), "csk": 6.3, "insert": (4.0, 5.7), "head": 5.5},
    "M4": {"clearance": 4.5, "tap": 3.3, "cbore": (8.0, 4.2), "csk": 8.4, "insert": (5.6, 8.0), "head": 7.0},
    "M5": {"clearance": 5.5, "tap": 4.2, "cbore": (10.0, 5.2), "csk": 10.4, "insert": (6.4, 9.5), "head": 8.5},
    "M6": {"clearance": 6.6, "tap": 5.0, "cbore": (11.0, 6.2), "csk": 12.6, "insert": (8.0, 12.7), "head": 10.0},
    "M8": {"clearance": 9.0, "tap": 6.8, "cbore": (15.0, 8.2), "csk": 16.5, "insert": (9.6, 12.7), "head": 13.0},
}


@dataclass
class FastenerSpec:
    size: str = "M3"
    kind: str = "clearance"  # clearance | tap | counterbore | countersink | insert
    extra_clearance: float = 0.0  # added to the hole diameter (print shrink)
    depth: Optional[float] = None  # None = through

    @property
    def label(self) -> str:
        return f"{self.size} {self.kind}"

    def diameter(self) -> float:
        t = METRIC[self.size]
        d = {"clearance": t["clearance"], "tap": t["tap"], "counterbore": t["clearance"], "countersink": t["clearance"], "insert": t["insert"][0]}[self.kind]
        return d + self.extra_clearance


def fastener_tool(k: GeometryKernel, point: Vec3, normal: Vec3, spec: FastenerSpec, depth: float) -> Body:
    """The cutter for a fastener hole entering the face at `point` along
    `-normal` (the face normal points out of the material)."""
    n = v_unit(normal)
    t = METRIC[spec.size]
    d = spec.diameter()
    depth = spec.depth or depth
    start = v_add(point, v_scale(n, 0.5))
    axis = v_scale(n, -1.0)
    tool = k.cylinder(start, axis, d / 2, depth + 0.5)
    if spec.kind == "counterbore":
        cb_d, cb_depth = t["cbore"]
        tool = k.boolean(tool, k.cylinder(start, axis, (cb_d + spec.extra_clearance) / 2, cb_depth + 0.5), BooleanOp.UNION)
    elif spec.kind == "countersink":
        csk = t["csk"] + spec.extra_clearance
        # 90° cone from csk diameter at the surface down to the hole diameter.
        cone_h = (csk - d) / 2
        from OCP.BRepPrimAPI import BRepPrimAPI_MakeCone
        from OCP.gp import gp_Ax2

        from .kernel.occt import D, P

        cone = Body(BRepPrimAPI_MakeCone(gp_Ax2(P(start), D(axis)), csk / 2 + 0.5, d / 2, cone_h + 0.5).Shape())
        tool = k.boolean(tool, cone, BooleanOp.UNION)
    elif spec.kind == "insert":
        ins_d, ins_depth = t["insert"]
        tool = k.cylinder(start, axis, (ins_d + spec.extra_clearance) / 2, ins_depth + 0.5)
        # A narrower pilot below the insert for the screw's tip.
        tool = k.boolean(tool, k.cylinder(start, axis, t["tap"] / 2 + 0.1, min(depth, ins_depth + 4.0) + 0.5), BooleanOp.UNION)
    return tool


def insert_boss(k: GeometryKernel, base: Vec3, axis: Vec3, spec: FastenerSpec, height: float, wall: float = 2.0) -> Body:
    """A boss sized for a heat-set insert: pocket diameter + 2·wall."""
    ins_d, ins_depth = METRIC[spec.size]["insert"]
    outer = k.cylinder(base, axis, ins_d / 2 + wall, height)
    top = v_add(base, v_scale(v_unit(axis), height))
    pocket = fastener_tool(k, top, v_unit(axis), FastenerSpec(spec.size, "insert", spec.extra_clearance), height)
    return k.boolean(outer, pocket, BooleanOp.SUBTRACT)


# ------------------------------------------------------ wall thickness


@dataclass
class ThinRegion:
    point: Vec3
    thickness: float
    face: int


def wall_thickness(k: GeometryKernel, body: Body, threshold: float, samples_per_face: int = 12) -> list[ThinRegion]:
    """Sample points on faces and shoot a ray inward; where the far wall is
    closer than `threshold`, report a thin region. Cheap, robust, and what
    a slicer would flag."""
    mesh = k.tessellate(body, 0.2)
    thin: list[ThinRegion] = []
    # Sample triangle centroids, a few per face, weighted by area.
    by_face: dict[int, list[int]] = {}
    for ti, fi in enumerate(mesh.triangle_face):
        by_face.setdefault(fi, []).append(ti)
    for fi, tris in by_face.items():
        tris_sorted = sorted(tris, key=lambda ti: -_tri_area(mesh, ti))
        for ti in tris_sorted[:samples_per_face]:
            a, b, c = (mesh.vertices[i] for i in mesh.triangles[ti])
            centroid = v_scale(v_add(v_add(a, b), c), 1 / 3)
            n = v_unit(v_cross(v_sub(b, a), v_sub(c, a)))
            origin = v_sub(centroid, v_scale(n, 1e-3))
            hits = k.ray_hits(body, origin, v_scale(n, -1.0))
            # A chord triangle sits a hair off its true surface: skip hits on
            # the same face closer than the tessellation's own error.
            hits = [h for h in hits if not (h[2] == fi and h[0] < 0.25)]
            if hits:
                d = hits[0][0]
                if 0.02 < d < threshold:
                    thin.append(ThinRegion(centroid, d, fi))
    return thin


def _tri_area(mesh: Mesh, ti: int) -> float:
    a, b, c = (mesh.vertices[i] for i in mesh.triangles[ti])
    return 0.5 * v_norm(v_cross(v_sub(b, a), v_sub(c, a)))


# ------------------------------------------------------------- validation


def validate_for_export(k: GeometryKernel, bodies: Sequence[tuple[str, Body]]) -> tuple[bool, list[str]]:
    """Every body must be a valid watertight solid; the list is actionable."""
    ok = True
    messages = []
    for name, b in bodies:
        rep = k.validate(b)
        if not (rep.valid and rep.watertight):
            ok = False
            for issue in rep.issues:
                where = f" near ({issue.location[0]:.1f}, {issue.location[1]:.1f}, {issue.location[2]:.1f})" if issue.location else ""
                messages.append(f"{name}: {issue.message}{where}" + (f" — {issue.fix}" if issue.fix else ""))
        mesh = k.tessellate(b, 0.05)
        open_edges = mesh_open_edges(mesh)
        if open_edges:
            ok = False
            messages.append(f"{name}: tessellation has {open_edges} open edge(s) — increase the tolerance or heal the body")
    return ok, messages


def weld(mesh: Mesh, tolerance: float = 1e-5) -> Mesh:
    """Merge coincident vertices (a B-rep tessellation duplicates them
    along face seams) so edges are shared and the mesh is checkable."""
    q = 1.0 / tolerance
    index: dict[tuple[int, int, int], int] = {}
    remap = []
    verts: list[Vec3] = []
    norms: list[Vec3] = []
    for i, v in enumerate(mesh.vertices):
        key = (round(v[0] * q), round(v[1] * q), round(v[2] * q))
        j = index.get(key)
        if j is None:
            j = len(verts)
            index[key] = j
            verts.append(v)
            norms.append(mesh.normals[i] if i < len(mesh.normals) else (0.0, 0.0, 1.0))
        remap.append(j)
    tris = []
    faces = []
    for t, f in zip(mesh.triangles, mesh.triangle_face or [0] * len(mesh.triangles)):
        a, b, c = remap[t[0]], remap[t[1]], remap[t[2]]
        if a == b or b == c or a == c:
            continue  # degenerate after welding
        tris.append((a, b, c))
        faces.append(f)
    return Mesh(verts, norms, tris, faces, mesh.face_count)


def mesh_open_edges(mesh: Mesh) -> int:
    """Edges used by other than two triangles after welding (a manifold
    closed mesh has none)."""
    welded = weld(mesh)
    count: dict[tuple[int, int], int] = {}
    for a, b, c in welded.triangles:
        for e in ((a, b), (b, c), (c, a)):
            key = (min(e), max(e))
            count[key] = count.get(key, 0) + 1
    return sum(1 for v in count.values() if v != 2)


# ------------------------------------------------------------ orientation


@dataclass
class Overhang:
    triangle: int
    angle_deg: float  # angle between the face normal and straight down (0 = facing down)


def overhangs(mesh: Mesh, up: Vec3 = (0.0, 0.0, 1.0), threshold_deg: float = 45.0) -> list[Overhang]:
    """Triangles whose normal faces the build plate more than `threshold`
    degrees from horizontal need support."""
    out = []
    down = v_scale(v_unit(up), -1.0)
    for ti, (a, b, c) in enumerate(mesh.triangles):
        pa, pb, pc = mesh.vertices[a], mesh.vertices[b], mesh.vertices[c]
        n = v_unit(v_cross(v_sub(pb, pa), v_sub(pc, pa)))
        cosang = v_dot(n, down)
        if cosang <= 0:
            continue
        # angle from horizontal: 90 - angle(n, down)
        from_down = math.degrees(math.acos(max(-1.0, min(1.0, cosang))))
        overhang_from_horizontal = 90.0 - from_down
        if overhang_from_horizontal > threshold_deg and from_down < 89.0:
            out.append(Overhang(ti, from_down))
    return out


def build_plate_placement(k: GeometryKernel, body: Body, up: Vec3 = (0.0, 0.0, 1.0)) -> tuple[Vec3, float]:
    """Translation that puts the body on the plate (lowest point at z=0,
    centred in x/y) and the height it will be printed at."""
    p = k.mass_properties(body)
    lo, hi = p.bbox_min, p.bbox_max
    t = (-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -lo[2])
    return t, hi[2] - lo[2]


def support_area(mesh: Mesh, threshold_deg: float = 45.0) -> float:
    return sum(_tri_area(mesh, o.triangle) for o in overhangs(mesh, threshold_deg=threshold_deg))
