"""Measure, section, mass, curvature and continuity — read-only queries
the inspection panels and annotations draw from."""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Optional, Sequence

from .document import Document, Measurement
from .kernel import Body, EdgeRef, FaceRef, GeometryKernel, Plane, Vec3
from .kernel.base import Mesh, v_add, v_cross, v_dist, v_dot, v_norm, v_scale, v_sub, v_unit


def measure_distance(k: GeometryKernel, a: Body, b: Body) -> Measurement:
    d, pa, pb = k.distance(a, b)
    return Measurement("distance", [pa, pb], d, f"{d:.3f} mm")


def measure_points(a: Vec3, b: Vec3) -> Measurement:
    d = v_dist(a, b)
    return Measurement("distance", [a, b], d, f"{d:.3f} mm")


def measure_radius(face_or_edge) -> Optional[Measurement]:
    r = getattr(face_or_edge, "radius", None)
    if r is None:
        return None
    center = getattr(face_or_edge, "center", None) or getattr(face_or_edge, "axis_point", None) or (0.0, 0.0, 0.0)
    at = getattr(face_or_edge, "midpoint", None) or getattr(face_or_edge, "point", None) or center
    return Measurement("radius", [center, at], r, f"R {r:.3f} mm  (Ø {2 * r:.3f})")


def measure_angle_faces(a: FaceRef, b: FaceRef) -> Measurement:
    ang = math.degrees(math.acos(max(-1.0, min(1.0, v_dot(v_unit(a.normal), v_unit(b.normal))))))
    return Measurement("angle", [a.centroid, b.centroid], ang, f"{ang:.2f}°")


def measure_angle_edges(a: EdgeRef, b: EdgeRef) -> Measurement:
    da, db = v_unit(v_sub(a.end, a.start)), v_unit(v_sub(b.end, b.start))
    ang = math.degrees(math.acos(max(-1.0, min(1.0, abs(v_dot(da, db))))))
    return Measurement("angle", [a.midpoint, b.midpoint], ang, f"{ang:.2f}°")


def face_distance(a: FaceRef, b: FaceRef) -> float:
    """Distance between two parallel planar faces along their normal."""
    return abs(v_dot(v_sub(b.centroid, a.centroid), v_unit(a.normal)))


@dataclass
class Selection:
    """Bounding box, volume, area and mass of a set of nodes."""

    bbox_min: Vec3
    bbox_max: Vec3
    volume: float
    area: float
    mass_g: float
    centroid: Vec3

    @property
    def size(self) -> Vec3:
        return v_sub(self.bbox_max, self.bbox_min)


def selection_properties(doc: Document, ids: Sequence[str]) -> Optional[Selection]:
    lo = [math.inf] * 3
    hi = [-math.inf] * 3
    vol = area = mass = 0.0
    cx = [0.0, 0.0, 0.0]
    for i in ids:
        b = doc.resolved_body(i)
        if b is None:
            m = doc.mesh_of(i)
            if m is None:
                continue
            bl, bh = m.bounds()
            lo = [min(lo[j], bl[j]) for j in range(3)]
            hi = [max(hi[j], bh[j]) for j in range(3)]
            continue
        p = doc.kernel.mass_properties(b)
        lo = [min(lo[j], p.bbox_min[j]) for j in range(3)]
        hi = [max(hi[j], p.bbox_max[j]) for j in range(3)]
        vol += p.volume
        area += p.area
        m = p.mass(doc.density_of(i))
        mass += m
        cx = [cx[j] + p.centroid[j] * max(m, 1e-9) for j in range(3)]
    if lo[0] is math.inf:
        return None
    total = max(mass, 1e-9)
    return Selection(tuple(lo), tuple(hi), vol, area, mass, (cx[0] / total, cx[1] / total, cx[2] / total))


def section_outline(doc: Document, plane: Plane, ids: Optional[Sequence[str]] = None) -> list[list[Vec3]]:
    """Section polylines through solids, sheets and reference meshes."""
    out: list[list[Vec3]] = []
    for n in doc.walk():
        if ids is not None and n.id not in ids:
            continue
        if not doc.is_visible(n.id):
            continue
        b = doc.resolved_body(n.id)
        if b is not None:
            try:
                out.extend(doc.kernel.section(b, plane))
            except Exception:
                pass
        elif n.kind == "mesh":
            out.extend(mesh_section(doc.mesh_of(n.id), plane))
    return out


def mesh_section(mesh: Mesh, plane: Plane) -> list[list[Vec3]]:
    """Segments where triangles cross the plane."""
    segs = []
    n = v_unit(plane.normal)
    for a, b, c in mesh.triangles:
        pts = [mesh.vertices[a], mesh.vertices[b], mesh.vertices[c]]
        ds = [v_dot(v_sub(p, plane.origin), n) for p in pts]
        cross = []
        for i in range(3):
            j = (i + 1) % 3
            if (ds[i] < 0) != (ds[j] < 0):
                t = ds[i] / (ds[i] - ds[j])
                cross.append(v_add(pts[i], v_scale(v_sub(pts[j], pts[i]), t)))
        if len(cross) == 2:
            segs.append(cross)
    return segs


def draft_angle_colors(mesh: Mesh, pull: Vec3, min_deg: float = 1.0) -> list[tuple[float, float, float]]:
    """Per-triangle colour: green = positive draft, red = negative, yellow = vertical."""
    p = v_unit(pull)
    out = []
    for a, b, c in mesh.triangles:
        n = v_unit(v_cross(v_sub(mesh.vertices[b], mesh.vertices[a]), v_sub(mesh.vertices[c], mesh.vertices[a])))
        ang = math.degrees(math.asin(max(-1.0, min(1.0, v_dot(n, p)))))
        if abs(ang) < min_deg:
            out.append((0.95, 0.85, 0.2))
        elif ang > 0:
            out.append((0.3, 0.75, 0.4))
        else:
            out.append((0.85, 0.3, 0.3))
    return out


def normal_direction_colors(mesh: Mesh, view_dir: Vec3) -> list[tuple[float, float, float]]:
    d = v_unit(view_dir)
    out = []
    for a, b, c in mesh.triangles:
        n = v_unit(v_cross(v_sub(mesh.vertices[b], mesh.vertices[a]), v_sub(mesh.vertices[c], mesh.vertices[a])))
        out.append((0.3, 0.6, 0.9) if v_dot(n, d) < 0 else (0.9, 0.5, 0.3))
    return out


def curvature_comb(k: GeometryKernel, wire: Body, scale: float = 5.0, samples: int = 48) -> list[tuple[Vec3, Vec3]]:
    """Comb lines: point → point + normal·curvature·scale."""
    return [(p, v_add(p, v_scale(n, kappa * scale))) for p, n, kappa in k.curvature_comb(wire, samples)]


def continuity_report(k: GeometryKernel, body: Body) -> list[tuple[EdgeRef, str]]:
    return [(e, k.continuity(body, e)) for e in k.edges(body)]


def surface_curvature_colors(k: GeometryKernel, body: Body, mesh: Mesh, faces: Sequence[FaceRef]) -> list[tuple[float, float, float]]:
    """Mean-curvature colouring per triangle (blue flat → red tight)."""
    out = []
    for ti, (a, b, c) in enumerate(mesh.triangles):
        fi = mesh.triangle_face[ti]
        face = faces[fi] if fi < len(faces) else None
        if face is None:
            out.append((0.5, 0.5, 0.5))
            continue
        p = v_scale(v_add(v_add(mesh.vertices[a], mesh.vertices[b]), mesh.vertices[c]), 1 / 3)
        try:
            kmin, kmax = k.surface_curvature(body, face, p)
        except Exception:
            kmin = kmax = 0.0
        h = abs(kmin + kmax) / 2
        t = min(1.0, h * 20.0)
        out.append((0.2 + 0.7 * t, 0.4 - 0.2 * t, 0.9 - 0.8 * t))
    return out
