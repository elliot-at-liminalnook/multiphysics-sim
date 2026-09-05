"""The physical assembly description (simrobot v3): everything the
simulator needs, derived from the document's geometry and materials.

Python owns geometry and derivation, Rust owns dynamics. This module turns
bodies, joints, motors, sensors and cables into `PHYSICAL_MODEL.md`'s
schema: links with full inertia tensors, collision meshes and signed
distance grids; joints with the clearance, backlash, friction and wall
compliance a printed pin-in-hole actually has (inferred from the coaxial
features and the material pair); fastened fixed joints; flexible links as
reduced modal models (`flex.py`); motors with their electrical, gearbox,
thermal and firmware blocks; battery, sensors, cables, control targets
and the uncertainty the Monte Carlo runs sample. SI units throughout.

It also carries results back: `load_results` reads the simulator's
`*.simresult.json` onto the nodes, and `apply_identification` stores
fitted joint parameters that the next export carries.
"""

from __future__ import annotations

import json
import math
import os
import time
from dataclasses import asdict
from typing import Any, Optional

import numpy as np

from .document import Document, Node
from .kernel import KernelError, SurfaceKind, Vec3
from .kernel.base import v_add, v_cross, v_dist, v_dot, v_norm, v_scale, v_sub, v_unit

MM = 1.0e-3
G = 9.81
SCHEMA_VERSION = 3

# Tightening torque (N·m) into plastic / heat-set inserts, screw stress area (mm²), class 8.8 yield (Pa).
_SCREW = {
    "M2": (0.15, 2.07), "M2.5": (0.3, 3.39), "M3": (0.6, 5.03), "M4": (1.5, 8.78),
    "M5": (3.0, 14.2), "M6": (5.0, 20.1), "M8": (12.0, 36.6),
}
_SCREW_YIELD = 640e6
_TORQUE_COEFFICIENT = 0.2
_DEFAULT_WALL = 2.4e-3  # m, three perimeters at 0.4 mm plus infill
_DEFAULT_SERVO_HORN_CLEARANCE = 0.05e-3
_MAX_COLLISION_VERTICES = 3000
_MAX_SDF_DIM = 48
_MAX_STRESS_CELLS = 2000


# ------------------------------------------------------------------ helpers


def _to_m(p) -> list:
    return [float(c) * MM for c in p]


def _friction_pair(doc: Document, a_mat: Optional[str], b_mat: Optional[str]) -> tuple[float, float]:
    """(static, kinetic) friction between two material ids. A missing id is steel."""
    ma = doc.materials.get(a_mat or "")
    pa = ma.props() if ma else None
    if pa is None:
        mb = doc.materials.get(b_mat or "")
        pb = mb.props() if mb else None
        if pb is None:
            return 0.45, 0.4
        f = pb["friction"].get("steel") or pb["friction"]["self"]
        return f["static"], f["kinetic"]
    key = "self" if (b_mat == a_mat) else (b_mat or "steel")
    f = pa["friction"].get(key)
    if f is None:
        mb = doc.materials.get(b_mat or "")
        if mb is not None:
            f2 = mb.props()["friction"].get(a_mat or "steel") or mb.props()["friction"]["self"]
            f1 = pa["friction"]["self"]
            return 0.5 * (f1["static"] + f2["static"]), 0.5 * (f1["kinetic"] + f2["kinetic"])
        f = pa["friction"].get("steel") or pa["friction"]["self"]
    return f["static"], f["kinetic"]


def material_block(doc: Document, mid: str) -> dict:
    m = doc.materials[mid]
    p = m.props()
    friction = {}
    for other in list(doc.materials) + ["steel", "world"]:
        s, k = _friction_pair(doc, mid, other if other != mid else mid)
        friction[other] = {"static": s, "kinetic": k}
    return {
        "id": m.id, "name": m.name, "density": m.density * 1000.0,
        "youngs_modulus": p["youngs_modulus"], "poisson": p["poisson"], "yield_strength": p["yield_strength"], "ultimate_strength": p["ultimate_strength"],
        "glass_transition_c": p["glass_transition_c"], "thermal_conductivity": p["thermal_conductivity"], "specific_heat": p["specific_heat"], "thermal_expansion": p["thermal_expansion"],
        "bearing_pressure": p["bearing_pressure"], "friction": friction, "print": p.get("print"),
    }


# ------------------------------------------------------------------ links


def _inertia_about_com(props, density_g_cm3: float) -> tuple[float, np.ndarray]:
    """(mass kg, 3x3 inertia about the centroid in kg·m²) from the kernel's
    volume properties (mm³; OCCT's matrix of inertia is the volume moment
    about the centre of mass in mm⁵)."""
    rho = density_g_cm3 * 1e-12  # mm⁵ × g/cm³ × 1e-12 = kg·m²
    V = props.volume
    Ic = np.array(props.inertia, dtype=float)
    return V * density_g_cm3 * 1e-6, Ic * rho


def validate_mass_metadata(meta, doc):
    """Validate SI mass/inertia and mm COM declarations without kernel work."""
    regions=meta.get('solid_materials', {})
    if not isinstance(regions,dict) or any(not str(k).isdigit() or v not in doc.materials for k,v in regions.items()):
        raise KernelError('solid_materials must map nonnegative solid indices to existing materials')
    m=meta.get('mass_properties')
    if m is None:return
    try:
        mass=float(m['mass_kg']); com=np.asarray(m['com_mm'],dtype=float); inertia=np.asarray(m['inertia_kg_m2'],dtype=float)
        valid=(math.isfinite(mass) and mass>=0 and com.shape==(3,) and np.all(np.isfinite(com)) and inertia.shape==(3,3) and np.all(np.isfinite(inertia)))
        if valid:
            eig=np.linalg.eigvalsh(inertia)
            valid=np.allclose(inertia,inertia.T,atol=1e-14) and eig.min()>=-1e-14 and eig.max()<=sum(eig)-eig.max()+1e-12
        if not valid or not m.get('source'):raise ValueError()
        if mass==0 and (not np.allclose(inertia,0,atol=1e-15) or not m.get('included_in')):raise ValueError()
        if m.get('included_in') is not None and m['included_in'] not in doc.nodes:raise ValueError()
    except (KeyError,TypeError,ValueError):
        raise KernelError('mass_properties requires nonnegative mass_kg, finite com_mm, physical symmetric inertia_kg_m2 and source; zero-mass geometry must identify included_in')


def body_mass_properties(doc, node, props=None):
    """Mass/COM/inertia from declared measurements or per-solid materials."""
    meta=node.robot or {};validate_mass_metadata(meta,doc)
    declared=meta.get('mass_properties')
    if declared:
        return float(declared['mass_kg']), np.asarray(declared['com_mm'])*MM, np.asarray(declared['inertia_kg_m2']), declared['source']
    regions=meta.get('solid_materials')
    if regions:
        solids=doc.kernel.unjoin(node.body)
        if any(int(i)>=len(solids) for i in regions):raise KernelError(f'{node.name}: solid material index is stale')
        terms=[]
        for i,solid in enumerate(solids):
            p=doc.kernel.inertial_properties(solid);mid=regions.get(str(i),node.material)
            mass,inertia=_inertia_about_com(p,doc.materials[mid].density)
            terms.append((mass,np.asarray(p.centroid)*MM,inertia))
        mass=sum(t[0] for t in terms);com=sum(m*c for m,c,_ in terms)/max(mass,1e-15)
        inertia=sum(I+m*(np.eye(3)*np.dot(c-com,c-com)-np.outer(c-com,c-com)) for m,c,I in terms)
        return mass,com,inertia,'CAD volumes with per-solid material densities (provisional unless calibrated)'
    p=props or doc.kernel.inertial_properties(node.body)
    mass,inertia=_inertia_about_com(p,doc.density_of(node.id))
    return mass,np.asarray(p.centroid)*MM,inertia,'CAD volume and material density'


def link_groups(doc: Document, joints: list[dict]) -> dict[str, str]:
    """Which body each body is rigidly merged into: fixed-joint children and
    mounted motors collapse into their parent link (by node id)."""
    into: dict[str, str] = {}
    ids = {n.id for n in doc.bodies()}
    for j in joints:
        if j["type"] == "fixed" and j["parent"] in ids and j["child"] in ids:
            into[j["child"]] = j["parent"]
    for n in doc.bodies():
        if n.robot and n.robot.get("kind") == "motor" and n.robot.get("mounted_on") in ids and n.id not in into:
            into[n.id] = n.robot["mounted_on"]

    def resolve(i: str) -> str:
        seen = set()
        while i in into and i not in seen:
            seen.add(i)
            i = into[i]
        return i

    return {i: resolve(i) for i in ids}


def _is_ground(n: Node) -> bool:
    return n.name.lower() == "ground" or "ground" in n.name.lower().split() or bool((n.robot or {}).get("ground"))


def _weld_np(mesh) -> tuple[np.ndarray, np.ndarray]:
    from .printing import weld

    w = weld(mesh, 1e-4)
    return np.asarray(w.vertices, dtype=float), np.asarray(w.triangles, dtype=np.int64).reshape(-1, 3)


def _decimate(verts: np.ndarray, tris: np.ndarray, target: int) -> tuple[np.ndarray, np.ndarray]:
    """Vertex clustering to at most `target` vertices; each cluster keeps a
    real surface vertex (the one nearest the cluster mean), so corners and
    edges survive as sample points for contact."""
    if len(verts) <= target:
        return verts, tris
    lo, hi = verts.min(axis=0), verts.max(axis=0)
    extent = np.maximum(hi - lo, 1e-9)
    cell = float((extent.prod() / target) ** (1.0 / 3.0)) * 0.8
    for _ in range(12):
        key = np.floor((verts - lo) / cell).astype(np.int64)
        _, inverse = np.unique(key, axis=0, return_inverse=True)
        inverse = inverse.reshape(-1)
        count = inverse.max() + 1
        if count <= target:
            break
        cell *= 1.25
    sums = np.zeros((count, 3))
    np.add.at(sums, inverse, verts)
    counts = np.bincount(inverse, minlength=count).astype(float)
    means = sums / counts[:, None]
    d = np.linalg.norm(verts - means[inverse], axis=1)
    rep = np.full(count, -1, dtype=np.int64)
    best = np.full(count, np.inf)
    order = np.argsort(d)
    for i in order:
        c = inverse[i]
        if d[i] < best[c]:
            best[c] = d[i]
            rep[c] = i
    new_verts = verts[rep]
    t = inverse[tris]
    keep = (t[:, 0] != t[:, 1]) & (t[:, 1] != t[:, 2]) & (t[:, 0] != t[:, 2])
    return new_verts, t[keep]


def _point_triangle_distance(p: np.ndarray, a: np.ndarray, b: np.ndarray, c: np.ndarray) -> np.ndarray:
    """Vectorised point–triangle distance (Ericson, Real-Time Collision Detection)."""
    ab, ac, ap = b - a, c - a, p - a
    d1, d2 = np.einsum("ij,ij->i", ab, ap), np.einsum("ij,ij->i", ac, ap)
    out = np.full(len(p), np.nan)
    m = (d1 <= 0) & (d2 <= 0)
    out[m] = np.linalg.norm(ap[m], axis=1)
    bp = p - b
    d3, d4 = np.einsum("ij,ij->i", ab, bp), np.einsum("ij,ij->i", ac, bp)
    m = np.isnan(out) & (d3 >= 0) & (d4 <= d3)
    out[m] = np.linalg.norm(bp[m], axis=1)
    vc = d1 * d4 - d3 * d2
    m = np.isnan(out) & (vc <= 0) & (d1 >= 0) & (d3 <= 0)
    v = np.where((d1 - d3) != 0, d1 / np.where((d1 - d3) != 0, d1 - d3, 1), 0)
    q = a + v[:, None] * ab
    out[m] = np.linalg.norm(p[m] - q[m], axis=1)
    cp = p - c
    d5, d6 = np.einsum("ij,ij->i", ab, cp), np.einsum("ij,ij->i", ac, cp)
    m = np.isnan(out) & (d6 >= 0) & (d5 <= d6)
    out[m] = np.linalg.norm(cp[m], axis=1)
    vb = d5 * d2 - d1 * d6
    m = np.isnan(out) & (vb <= 0) & (d2 >= 0) & (d6 <= 0)
    w = np.where((d2 - d6) != 0, d2 / np.where((d2 - d6) != 0, d2 - d6, 1), 0)
    q = a + w[:, None] * ac
    out[m] = np.linalg.norm(p[m] - q[m], axis=1)
    va = d3 * d6 - d5 * d4
    m = np.isnan(out) & (va <= 0) & ((d4 - d3) >= 0) & ((d5 - d6) >= 0)
    den = (d4 - d3) + (d5 - d6)
    w = np.where(den != 0, (d4 - d3) / np.where(den != 0, den, 1), 0)
    q = b + w[:, None] * (c - b)
    out[m] = np.linalg.norm(p[m] - q[m], axis=1)
    m = np.isnan(out)
    if m.any():
        denom = va + vb + vc
        denom = np.where(denom != 0, denom, 1)
        v = vb / denom
        w = vc / denom
        q = a + v[:, None] * ab + w[:, None] * ac
        out[m] = np.linalg.norm(p[m] - q[m], axis=1)
    return out


def signed_distance_grid(meshes: list[tuple[np.ndarray, np.ndarray]], cell: float, pad: float = 2.0) -> dict:
    """A signed distance grid (negative inside) over the union of watertight
    meshes: nearest surface sample by k-d tree, refined to exact
    point–triangle distance within two cells of the surface, sign by ray
    parity per member mesh."""
    import trimesh
    from scipy.spatial import cKDTree

    allv = np.vstack([v for v, _ in meshes])
    lo = allv.min(axis=0) - pad * cell
    hi = allv.max(axis=0) + pad * cell
    dims = np.maximum(np.ceil((hi - lo) / cell).astype(int) + 1, 2)
    xs, ys, zs = (lo[i] + cell * np.arange(dims[i]) for i in range(3))
    gx, gy, gz = np.meshgrid(xs, ys, zs, indexing="ij")
    pts = np.stack([gx.ravel(), gy.ravel(), gz.ravel()], axis=1)
    tms = [trimesh.Trimesh(v, t, process=False) for v, t in meshes]
    # Surface samples (vertices plus even samples at half-cell spacing).
    samples = [allv]
    tri_centroids, tri_index = [], []
    for k, tm in enumerate(tms):
        n = int(max(200, min(60000, tm.area / (0.5 * cell) ** 2)))
        # Geometry derivation has its own fixed stream, independent of scenario
        # sensor seeds and NumPy's global state. Cold exports must reproduce.
        s, _ = trimesh.sample.sample_surface(tm, n, seed=0)
        samples.append(np.asarray(s))
        tri_centroids.append(tm.triangles_center)
        tri_index.append(np.full(len(tm.faces), k))
    samples = np.vstack(samples)
    dist, _ = cKDTree(samples).query(pts, k=1)
    # Exact distance near the surface.
    near = dist < 2.0 * cell
    if near.any():
        cents = np.vstack(tri_centroids)
        owner = np.concatenate(tri_index)
        tris = np.vstack([tms[k].triangles for k in range(len(tms))])
        _, idx = cKDTree(cents).query(pts[near], k=min(8, len(cents)))
        idx = np.atleast_2d(idx)
        p = pts[near]
        best = np.full(len(p), np.inf)
        for j in range(idx.shape[1]):
            tri = tris[idx[:, j]]
            best = np.minimum(best, _point_triangle_distance(p, tri[:, 0], tri[:, 1], tri[:, 2]))
        dist[near] = best
    inside = np.zeros(len(pts), dtype=bool)
    for tm in tms:
        try:
            # Explicit direction disables trimesh's random retry for ambiguous
            # ray intersections. Distances on the surface round to zero below.
            inside |= trimesh.ray.ray_util.contains_points(tm.ray, pts,
                check_direction=[0.4395064455, 0.617598629942, 0.652231566745])
        except Exception:
            pass
    values = np.where(inside, -dist, dist)
    return {"origin": lo.tolist(), "cell": float(cell), "dims": [int(d) for d in dims], "values": [round(float(v), 6) for v in values]}


def collision_block(doc: Document, members: list[Node], com_m: np.ndarray, cache=None, geometry_keys=None) -> tuple[dict, list[tuple[np.ndarray, np.ndarray]]]:
    """Collision geometry of a link in its frame (metres, origin at the COM):
    decimated surface mesh, convex hull and signed distance grid."""
    import trimesh

    meshes = []
    for n in members:
        def tessellate():
            m = doc.mesh_of(n.id, 0.15)
            if m is None or not m.vertices: return None
            v, t = _weld_np(m)
            return [v.tolist(), t.tolist()]
        mesh = cache.get('body_mesh', {'geometry': geometry_keys[n.id], 'tolerance_mm': .15}, tessellate) if cache else tessellate()
        if mesh is None:
            continue
        v, t = np.asarray(mesh[0]), np.asarray(mesh[1], dtype=int)
        meshes.append((v * MM - com_m, t))
    if not meshes:
        return {"vertices": [], "triangles": [], "hull": [], "sdf": None}, []
    verts = np.vstack([v for v, _ in meshes])
    tris = np.vstack([t + off for (v, t), off in zip(meshes, np.cumsum([0] + [len(v) for v, _ in meshes[:-1]]))])
    dv, dt = _decimate(verts, tris, _MAX_COLLISION_VERTICES)
    try:
        hull = trimesh.convex.convex_hull(verts).vertices
    except Exception:
        hull = dv
    extent = verts.max(axis=0) - verts.min(axis=0)
    cell = max(1.0e-3, float(extent.max()) / (_MAX_SDF_DIM - 5))
    cell = min(cell, 2.0e-3) if float(extent.max()) / 2.0e-3 <= _MAX_SDF_DIM - 5 else cell
    sdf = signed_distance_grid(meshes, cell)
    block = {
        "vertices": [[round(float(c), 6) for c in p] for p in dv],
        "triangles": [[int(i) for i in t] for t in dt],
        "hull": [[round(float(c), 6) for c in p] for p in hull],
        "sdf": sdf,
    }
    return block, meshes


# ------------------------------------------------------------------ joints


def _joint_records(doc: Document) -> list[dict]:
    """Joints as dicts (ids), from joint nodes and legacy `joint:` planes."""
    from .simbridge import joints_of

    by_name = {n.name: n.id for n in doc.bodies()}
    out = []
    for j in joints_of(doc):
        out.append(dict(j, child=by_name.get(j["child"], j["child"]), parent=by_name.get(j["parent"]) if j["parent"] else None))
    return out


def _cylinders_near_axis(doc: Document, nodes: list[Node], pivot: Vec3, axis: Vec3, tol: float = 1.0):
    """Cylindrical faces of `nodes` coaxial with the joint axis: (node, face,
    is_hole, span start, span length)."""
    k = doc.kernel
    a = v_unit(axis)
    out = []
    for n in nodes:
        if n.body is None:
            continue
        cache=getattr(doc,'_physics_cylinder_cache',{})
        doc._physics_cylinder_cache=cache
        hit=cache.get(n.id)
        if hit is None or hit[0] is not n.body:
            hit=(n.body,k.cylindrical_faces(n.body));cache[n.id]=hit
        for f in hit[1]:
            if f.kind != SurfaceKind.CYLINDER or not f.radius or f.axis_point is None or f.axis_dir is None:
                continue
            if abs(abs(v_dot(v_unit(f.axis_dir), a)) - 1.0) > 2e-3:
                continue
            off = v_sub(f.axis_point, pivot)
            if v_norm(v_sub(off, v_scale(a, v_dot(off, a)))) > tol:
                continue
            try:
                hole = k._cylinder_is_hole(n.body, f)
                base, height = k._cylinder_span(n.body, f, 0.0, current_reference=True)
            except Exception:
                continue
            t0 = v_dot(v_sub(base, pivot), a)
            sign = 1.0 if v_dot(v_unit(f.axis_dir), a) > 0 else -1.0
            lo, hi = sorted((t0, t0 + sign * height))
            out.append((n, f, hole, lo, hi))
    return out


def _subtree_mass(doc: Document, link_of: dict, links: dict, joints: list[dict], child_link: str) -> tuple[float, np.ndarray]:
    """Mass (kg) and COM (m) of everything outboard of a joint."""
    out = [child_link]
    frontier = [child_link]
    while frontier:
        b = frontier.pop()
        for j in joints:
            if j["type"].startswith("loop_") or j["type"] == "fixed":
                continue
            p = link_of.get(j["parent"]) if j["parent"] else None
            c = link_of.get(j["child"])
            if p == b and c not in out and c in links:
                out.append(c)
                frontier.append(c)
    mass = sum(links[l]["mass"] for l in out)
    com = sum(links[l]["mass"] * np.array(links[l]["com"]) for l in out) / max(mass, 1e-12)
    return mass, com


def joint_physics(doc: Document, j: dict, parent_members: list[Node], child_members: list[Node], parent_mat: Optional[str], child_mat: Optional[str], outboard_mass: float, outboard_com: np.ndarray, motor: Optional[dict]) -> dict:
    """What the printer made of this joint: pin/hole radii, contact length,
    clearance, backlash and wobble, friction from the material pair under
    the outboard weight, and the wall compliance around the hole."""
    k = doc.kernel
    pivot, axis = tuple(j["pivot"]), v_unit(tuple(j["axis"]))
    pair = None
    pc = [(c, "parent") for c in _cylinders_near_axis(doc, parent_members, pivot, axis)]
    cc = [(c, "child") for c in _cylinders_near_axis(doc, child_members, pivot, axis)]
    holes = [(c, side) for c, side in pc + cc if c[2]]
    pins = [(c, side) for c, side in cc + pc if not c[2]]
    best = -1.0
    for (hn, hf, _, hlo, hhi), hside in holes:
        for (pn, pf, _, plo, phi), pside in pins:
            if hn.id == pn.id or hside == pside:
                continue  # a bearing pair straddles the joint
            overlap = min(hhi, phi) - max(hlo, plo)
            if overlap > best and overlap > 0.5 and abs(hf.radius - pf.radius) < 0.4:
                best = overlap
                pair = (hn, hf, pn, pf, overlap)
    source = "inferred" if pair else "declared"
    pin_mat, hole_mat = child_mat, parent_mat
    if pair:
        hn, hf, pn, pf, overlap = pair
        pin_r, hole_r, contact = pf.radius * MM, hf.radius * MM, overlap * MM
        pin_mat, hole_mat = pn.material, hn.material
        kind = "printed_pin"
        if (pn.robot or {}).get("kind") == "motor":
            pin_mat = "steel"
            kind = "servo_horn" if (pn.robot or {}).get("spec", "").startswith(("sg", "mg", "ds")) else "printed_pin"
    else:
        if motor is not None:
            pin_r = 0.5 * motor["shaft_diameter"] * MM
            kind = "servo_horn" if motor["kind"] == "servo" else "printed_pin"
            pin_mat = "steel"
        else:
            pin_r, kind = 2.0e-3, "printed_pin"
        hole_r = pin_r + (_DEFAULT_SERVO_HORN_CLEARANCE if kind == "servo_horn" else 0.15e-3)
        contact = 6.0e-3
    clearance = max(0.0, hole_r - pin_r)
    lever = float(np.linalg.norm(outboard_com - np.array(_to_m(pivot)))) if outboard_mass > 0 else 0.0
    backlash = clearance / max(lever, 5.0e-3)
    wobble = math.atan2(2.0 * clearance, max(contact, 1.0e-4))
    mu_s, mu_k = _friction_pair(doc, hole_mat, pin_mat)
    N = outboard_mass * G
    coulomb = mu_k * N * pin_r
    stribeck = max(0.0, (mu_s - mu_k) * N * pin_r)
    viscous = 2.0e-4 * (pin_r / 2.0e-3)
    hole_props = doc.materials[hole_mat].props() if hole_mat in doc.materials else doc.materials["pla"].props()
    E = hole_props["youngs_modulus"]
    wall = _DEFAULT_WALL
    if pair:
        try:
            from .printing import wall_thickness

            thin = wall_thickness(k, pair[0].body, 6.0)
            near = [t for t in thin if v_dist(tuple(t.point), pivot) < (hole_r / MM) * 3 + 6]
            if near:
                wall = max(0.8e-3, min(t.thickness for t in near) * MM)
        except Exception:
            pass
    radial = E * contact * wall / max(hole_r, 1e-4)
    if kind == "servo_horn":
        radial *= 4.0  # a steel spline in a plastic horn is stiffer than a printed pin
    physics = {
        "source": source, "pin_radius": pin_r, "hole_radius": hole_r, "contact_length": contact,
        "clearance": clearance, "backlash": backlash, "wobble": wobble,
        "friction": {"coulomb": coulomb, "viscous": viscous, "stribeck": stribeck, "stribeck_speed": 0.1, "static_ratio": mu_s / max(mu_k, 1e-6)},
        "stiffness": {"radial": radial, "axial": 0.5 * radial, "bending": radial * contact * contact / 12.0}, "damping_ratio": 0.05,
        "bearing": {"kind": kind, "allowable_pressure": hole_props["bearing_pressure"], "pressure": N / max(2 * pin_r * contact, 1e-9)},
        "materials": {"pin": pin_mat, "hole": hole_mat}, "outboard_mass": outboard_mass, "lever": lever,
    }
    if j["type"] == "prismatic":
        physics["friction"] = {"coulomb": mu_k * N, "viscous": 5.0, "stribeck": (mu_s - mu_k) * N, "stribeck_speed": 0.01, "static_ratio": mu_s / max(mu_k, 1e-6)}
    return physics


def fastened_block(doc: Document, j: dict, parent_members: list[Node], child_members: list[Node]) -> Optional[dict]:
    """Screws recorded on either body whose axis passes through both bodies
    become a bolted flange with preload, stiffness and shear capacity."""
    k = doc.kernel
    found = []
    for n in parent_members + child_members:
        for f in (n.robot or {}).get("fasteners", []):
            p, d = tuple(f["point"]), v_unit(tuple(f["direction"]))
            # Probe the wall around the hole (the hole itself is void), along the screw.
            from .printing import METRIC

            r_off = METRIC.get(f.get("size", "M3"), METRIC["M3"])["clearance"] / 2 + 0.6
            helper = (0.0, 0.0, 1.0) if abs(d[2]) < 0.9 else (1.0, 0.0, 0.0)
            u = v_unit(v_cross(helper, d))
            w = v_cross(d, u)
            probes = [v_add(v_add(p, v_scale(d, t)), v_scale(s_, r_off)) for t in (-1.0, 1.0, 2.0, 4.0, 8.0, 12.0, 16.0, 24.0) for s_ in (u, w, v_scale(u, -1.0), v_scale(w, -1.0))]
            in_parent = any(k.contains(m.body, q) for m in parent_members if m.body is not None for q in probes)
            in_child = any(k.contains(m.body, q) for m in child_members if m.body is not None for q in probes)
            if in_parent and in_child:
                found.append(f)
    if not found:
        return None
    size = found[0]["size"]
    torque, area_mm2 = _SCREW.get(size, _SCREW["M3"])
    d = float(size[1:]) * MM
    preload = torque / (_TORQUE_COEFFICIENT * d)
    mats = [m.material for m in parent_members + child_members if m.material in doc.materials]
    E_m = min(doc.materials[m].props()["youngs_modulus"] for m in mats) if mats else 2.0e9
    grip = 6.0e-3
    k_b = 200e9 * area_mm2 * 1e-6 / grip
    k_m = 2.0 * E_m * d
    per_bolt = k_b * k_m / (k_b + k_m)
    pts = np.array([f["point"] for f in found], dtype=float)
    radius = float(np.linalg.norm(pts - pts.mean(axis=0), axis=1).max()) * MM if len(found) > 1 else d
    return {"screw": size, "count": len(found), "preload": preload, "stiffness": per_bolt * len(found), "shear_capacity": 0.6 * _SCREW_YIELD * area_mm2 * 1e-6 * len(found), "pattern_radius": radius, "points": [_to_m(f["point"]) for f in found]}


def glued_fixed_block(doc: Document, parent_members: list[Node], child_members: list[Node]) -> dict:
    """A fixed joint without screws: a printed or glued interface whose
    stiffness is the smaller body's smallest bbox face over a 1 mm layer."""
    k = doc.kernel
    areas = []
    mats = []
    for m in child_members + parent_members:
        if m.body is None:
            continue
        lo, hi = k.bounding_box(m.body)
        s = [max(hi[i] - lo[i], 1e-3) for i in range(3)]
        faces = sorted([s[0] * s[1], s[1] * s[2], s[0] * s[2]])
        areas.append(faces[0] * 1e-6)
        if m.material in doc.materials:
            mats.append(m.material)
    E = min(doc.materials[m].props()["youngs_modulus"] for m in mats) if mats else 2.0e9
    area = min(areas) if areas else 1e-4
    return {"screw": None, "count": 0, "preload": 0.0, "stiffness": E * area / 1.0e-3, "shear_capacity": 0.5 * E * 0.01 * area, "pattern_radius": math.sqrt(area / math.pi), "points": []}


# ------------------------------------------------------------------ motors


def motor_block(doc: Document, n: Node, joint: Optional[dict], gear_extra: float) -> dict:
    from .robotics import MOTOR_LIBRARY, motor_physics

    meta = n.robot or {}
    spec = MOTOR_LIBRARY[meta["spec"]]
    phys = motor_physics(spec, gear_extra)
    mounted = doc.nodes.get(meta.get("mounted_on") or "")
    return {
        "name": n.name, "id": n.id, "spec": spec.id, "kind": spec.kind, "joint": joint["name"] if joint else None,
        "mounted_on": mounted.name if mounted else None, "mount_point": _to_m(meta["mount_point"]), "shaft_axis": [float(c) for c in v_unit(tuple(meta["shaft_axis"]))],
        "gear_ratio": gear_extra, "mass": spec.mass_g * 1e-3, "stall_torque": spec.stall_torque * gear_extra, "no_load_speed": spec.no_load_speed / max(gear_extra, 1e-9),
        **phys,
    }


# ------------------------------------------------------------------ sensors, cables, settings


def sensor_block(doc: Document, n: Node, link_of_body: dict, links: dict) -> Optional[dict]:
    r = n.robot or {}
    body = r.get("body")
    link = link_of_body.get(body)
    if link is None or link not in links:
        return None
    com = np.array(links[link]["com"])
    point = np.array(_to_m(r["point"])) - com
    axes = r.get("axes") or [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
    kind = r.get("kind", "imu")
    defaults = {
        "imu": {"rate_hz": 200.0, "noise": {"accel": 0.02, "gyro": 0.002}, "bias": {"accel": [0.05, -0.03, 0.04], "gyro": [0.003, -0.002, 0.001]}, "bias_walk": 1e-4, "quantization": {"accel": 0.0006, "gyro": 6e-5}, "range": {"accel": 16.0, "gyro": 34.9}},
        "encoder": {"rate_hz": 1000.0, "noise": {"angle": 0.0}, "quantization": {"angle": 2 * math.pi / 4096}},
        "current": {"rate_hz": 1000.0, "noise": {"current": 0.01}, "quantization": {"current": 0.001}},
        "force": {"rate_hz": 500.0, "noise": {"force": 0.05}, "quantization": {"force": 0.01}},
    }[kind]
    block = {"name": n.name, "id": n.id, "kind": kind, "link": links[link]["name"], "point": [float(c) for c in point], "axes": axes, "joint": r.get("joint_name"), **defaults}
    for key, v in r.items():
        if key in ("rate_hz", "noise", "bias", "bias_walk", "quantization", "range"):
            block[key] = v
    return block


def cable_block(doc: Document, n: Node, link_of_body: dict, links: dict) -> Optional[dict]:
    r = n.robot or {}
    a, b = link_of_body.get(r.get("from_body")), link_of_body.get(r.get("to_body"))
    if a is None or b is None:
        return None
    pa, pb = np.array(_to_m(r["from_point"])), np.array(_to_m(r["to_point"]))
    length = r.get("length") or float(np.linalg.norm(pb - pa)) * 1.1
    mass = r.get("mass") or 0.004 * length / 0.1  # 4 g per 100 mm of 3-wire servo lead
    return {
        "name": n.name, "id": n.id, "from": {"link": links[a]["name"], "point": (pa - np.array(links[a]["com"])).tolist()}, "to": {"link": links[b]["name"], "point": (pb - np.array(links[b]["com"])).tolist()},
        "length": length, "mass": mass, "stiffness": r.get("stiffness") or 2000.0, "damping": r.get("damping") or 0.5, "segments": int(r.get("segments") or 4),
    }


def default_settings() -> dict:
    return {
        "battery": None,
        "control": {"period_s": 0.02, "latency_s": 0.004, "targets": {}, "mode": "hold", "trajectory": []},
        "uncertainty": {"dimension_m": {"sigma": 0.15e-3}, "mass": {"sigma_fraction": 0.05}, "friction": {"sigma_fraction": 0.2}, "stiffness": {"sigma_fraction": 0.15}, "backlash": {"sigma_fraction": 0.3}, "motor_torque": {"sigma_fraction": 0.1}, "com_m": {"sigma": 0.5e-3}, "seed": 0},
        "world": {"floor_z": None, "floor_material": "world", "floor_stiffness": 2.0e5, "floor_damping": 2.0e3, "terrain": None},
        "identification": {},
    }


def settings(doc: Document) -> dict:
    d = default_settings()
    for k, v in (doc.robot_settings or {}).items():
        if isinstance(v, dict) and isinstance(d.get(k), dict):
            d[k] = {**d[k], **v}
        else:
            d[k] = v
    return d


# ------------------------------------------------------------------ export


def _assembly_properties(doc, cache=None, joint_ids=None, verbose=False):
    """Shared mass and joint derivation, without collision meshes or flex."""
    joints = _joint_records(doc)
    groups = link_groups(doc, joints)
    k = doc.kernel
    node_by_id = {n.id: n for n in doc.bodies()}
    geometry_keys = {}
    if cache:
        from .snapshots import digest
        for n in doc.bodies():
            captured = doc._snapshot_body_cache.get(n.id)
            data = captured[1] if captured and captured[0] is n.body else k.serialize(n.body)
            doc._snapshot_body_cache[n.id] = (n.body, data)
            geometry_keys[n.id] = digest(data)
    def cached(stage, dependencies, build):
        return cache.get(stage, dependencies, build) if cache else build()
    def member_inputs(members):
        return [{'id': n.id, 'geometry': geometry_keys.get(n.id), 'material': n.material, 'robot': n.robot}
                for n in members]
    material_inputs = {mid: material_block(doc, mid) for mid in doc.materials} if cache else None
    materials_used = set()
    # Per-body mass properties.
    raw: dict[str, dict] = {}
    for n in doc.bodies():
        from .kernel import MassProperties
        if verbose:print("Mass:",n.name,flush=True)
        if (n.robot or {}).get('mass_properties'):
            lo,hi=k.bounding_box(n.body)
            p=MassProperties(0.,0.,tuple((a+b)/2 for a,b in zip(lo,hi)),((0.,0.,0.),)*3,lo,hi)
        else:
            p = MassProperties(**cached('body_properties', {'geometry': geometry_keys.get(n.id)}, lambda: asdict(k.inertial_properties(n.body))))
        mass, com, I, mass_source = body_mass_properties(doc, n, p)
        raw[n.id] = {"mass": mass, "com": com, "I": I, "mass_source": mass_source, "bbox": (np.array(p.bbox_min) * MM, np.array(p.bbox_max) * MM), "node": n}
        if n.material in doc.materials:
            materials_used.add(n.material)
        materials_used.update((n.robot or {}).get("solid_materials",{}).values())
    # Links.
    links: dict[str, dict] = {}
    members_of: dict[str, list[Node]] = {}
    for lid, r in raw.items():
        if groups[lid] != lid:
            continue
        mem = [raw[m]["node"] for m in raw if groups[m] == lid]
        members_of[lid] = mem
        mass = sum(raw[m.id]["mass"] for m in mem)
        com = sum(raw[m.id]["mass"] * raw[m.id]["com"] for m in mem) / max(mass, 1e-12)
        I = np.zeros((3, 3))
        for m in mem:
            d = raw[m.id]["com"] - com
            I += raw[m.id]["I"] + raw[m.id]["mass"] * (float(d @ d) * np.eye(3) - np.outer(d, d))
        lo = np.min([raw[m.id]["bbox"][0] for m in mem], axis=0) - com
        hi = np.max([raw[m.id]["bbox"][1] for m in mem], axis=0) - com
        n = r["node"]
        links[lid] = {
            "name": n.name, "id": lid, "members": [m.id for m in mem], "member_names": [m.name for m in mem], "material": n.material if n.material in doc.materials else "pla",
            "mass_sources": {m.id: raw[m.id]["mass_source"] for m in mem},
            "ground": any(_is_ground(m) for m in mem), "mass": mass, "com": com.tolist(), "inertia": I.tolist(), "bbox": [lo.tolist(), hi.tolist()],
        }
    link_of_body = {bid: groups[bid] for bid in raw}
    link_names = {lid: l["name"] for lid, l in links.items()}
    # Motors (by joint).
    motor_nodes = [n for n in doc.bodies() if n.robot and n.robot.get("kind") == "motor"]
    joint_of_motor = {}
    for j in joints:
        if j.get("motor") and j["motor"].get("id"):
            joint_of_motor[j["motor"]["id"]] = j
    # Joints.
    out_joints = []
    for j in joints:
        if joint_ids is not None and j["id"] not in joint_ids:
            continue
        child_link = link_of_body.get(j["child"])
        parent_link = link_of_body.get(j["parent"]) if j["parent"] else None
        if child_link is None or (j["parent"] and parent_link is None):
            continue
        if child_link == parent_link and j["type"] != "fixed":
            continue
        pm = members_of.get(parent_link, []) if parent_link else []
        cm = members_of.get(child_link, [])
        if j["type"] == "fixed":
            # Fixed joints are merged into links; export them as compliant attachments for reference.
            child_members = [node_by_id[j["child"]]] if j["child"] in node_by_id else []
            parent_members = [node_by_id[j["parent"]]] if j["parent"] in node_by_id else []
            fast = cached('attachment', {'parent': member_inputs(parent_members), 'child': member_inputs(child_members), 'materials': material_inputs},
                          lambda: fastened_block(doc, j, parent_members, child_members) or glued_fixed_block(doc, parent_members, child_members))
            out_joints.append({
                "name": j["name"], "id": j["id"], "type": "fixed", "parent": link_names[parent_link] if parent_link else None, "child": node_by_id[j["child"]].name if j["child"] in node_by_id else j["child"],
                "merged_into": link_names[child_link], "origin": _to_m(j["pivot"]), "axis": [float(c) for c in v_unit(tuple(j["axis"]))], "limits": None, "home": 0.0,
                "physics": {"source": "inferred" if fast.get("count") else "declared", "stiffness": {"radial": fast["stiffness"], "axial": fast["stiffness"], "bending": fast["stiffness"] * fast["pattern_radius"] ** 2}, "damping_ratio": 0.05, "friction": {"coulomb": 0.0, "viscous": 0.0, "stribeck": 0.0, "stribeck_speed": 0.1, "static_ratio": 1.0}, "clearance": 0.0, "backlash": 0.0, "wobble": 0.0, "pin_radius": 0.0, "hole_radius": 0.0, "contact_length": 0.0, "bearing": {"kind": "bolt" if fast.get("count") else "printed_pin", "allowable_pressure": 0.0, "pressure": 0.0}},
                "fastened": fast, "motor": None,
            })
            continue
        outboard_mass, outboard_com = _subtree_mass(doc, link_of_body, links, joints, child_link)
        motor_meta = None
        if j.get("motor") and j["motor"].get("id") in doc.nodes:
            from .robotics import MOTOR_LIBRARY

            spec = MOTOR_LIBRARY[doc.nodes[j["motor"]["id"]].robot["spec"]]
            motor_meta = {"shaft_diameter": spec.shaft_diameter, "kind": spec.kind}
        phys = cached('joint', {'joint': j, 'parent': member_inputs(pm), 'child': member_inputs(cm), 'materials': material_inputs,
                               'mass': outboard_mass, 'com': outboard_com.tolist(), 'motor': motor_meta},
                      lambda: joint_physics(doc, j, pm, cm, links[parent_link]["material"] if parent_link else None, links[child_link]["material"], outboard_mass, outboard_com, motor_meta))
        override = (doc.nodes[j["id"]].robot or {}).get("physics") if j["id"] in doc.nodes else None
        for key, v in (override or {}).items():
            if isinstance(v, dict) and isinstance(phys.get(key), dict):
                phys[key] = {**phys[key], **v}
            else:
                phys[key] = v
        if override:
            phys["source"] = "declared"
        radius = phys.get('flex_patch_radius')
        if radius is not None and (isinstance(radius, bool) or not isinstance(radius, (int, float))
                                   or not math.isfinite(radius) or radius <= 0):
            raise KernelError(f'{j["name"]}: flex_patch_radius must be finite and positive in metres')
        phys['flex_patch_source'] = 'inferred' if radius is None else 'declared'
        phys['flex_patch_radius'] = max(phys.get('hole_radius', 0.) + _DEFAULT_WALL, .004) if radius is None else radius
        ident = settings(doc)["identification"].get(j["name"])
        if ident:
            phys["identified"] = ident
        limits = j.get("limits")
        if limits and j["type"] == "prismatic":
            limits = [None if v is None else v * MM for v in limits]
        out_joints.append({
            "name": j["name"], "id": j["id"], "type": j["type"], "parent": link_names[parent_link] if parent_link else None, "child": link_names[child_link],
            "origin": _to_m(j["pivot"]), "axis": [float(c) for c in v_unit(tuple(j["axis"]))], "limits": limits, "home": j.get("home", 0.0),
            "physics": phys, "fastened": None, "motor": j["motor"]["name"] if j.get("motor") else None, "declared": {"damping": j.get("damping", 0.0), "friction": j.get("friction", 0.0), "stroke": j.get("stroke", 0.0) * MM},
        })
    return joints, raw, links, members_of, link_of_body, motor_nodes, joint_of_motor, out_joints, materials_used, geometry_keys, cached


def inspect_joint_physics(doc, joint_id):
    """Infer one joint using the same physical rules as simulation export."""
    records = _assembly_properties(doc, joint_ids={joint_id})[7]
    return next((j["physics"] for j in records if j["id"] == joint_id), {})


def export_physical_model(doc: Document, path: Optional[str] = None, planar=None, flex: bool = True, verbose: bool = False, cache=None) -> dict:
    """The v3 physical assembly description; written to `path` when given.
    `planar` is a Plane hint the simulator may project onto; `flex=False`
    skips the modal reduction (fast exports for the live link)."""
    t_start = time.time()
    transmissions=[]
    for t in doc.robot_settings.get('transmissions',[]):
        pair=[doc.nodes.get(t.get(k)) for k in ('driver_joint','driven_joint')]
        if any(n is None or n.joint is None or n.joint.type not in ('revolute','continuous') for n in pair):raise KernelError('Transmission requires two rotational joints')
        ratio=float(t['ratio'])
        if not math.isfinite(ratio) or abs(ratio)<1e-12:raise KernelError('Transmission ratio must be finite and nonzero')
        transmissions.append({'name':t['name'],'driver_joint':pair[0].name,'driven_joint':pair[1].name,'ratio':ratio})
    joints, raw, links, members_of, link_of_body, motor_nodes, joint_of_motor, out_joints, materials_used, geometry_keys, cached = _assembly_properties(doc, cache, verbose=verbose)
    # Motors.
    out_motors = []
    for n in motor_nodes:
        j = joint_of_motor.get(n.id)
        out_motors.append(motor_block(doc, n, j, j["motor"]["gear_ratio"] / max(__import__("robocad.robotics", fromlist=["MOTOR_LIBRARY"]).MOTOR_LIBRARY[n.robot["spec"]].gear_ratio, 1e-9) if j else 1.0))
    # Collision + flex per link.
    out_links = []
    for lid, l in links.items():
        t0 = time.time()
        collision_inputs = {'members': [geometry_keys.get(m.id) for m in members_of[lid]], 'com': l['com']}
        def build_collision():
            block, meshes = collision_block(doc, members_of[lid], np.array(l['com']), cache, geometry_keys)
            return {'block': block, 'meshes': [[v.tolist(), t.tolist()] for v, t in meshes]}
        artifact = cached('collision', collision_inputs, build_collision)
        block, meshes = artifact['block'], artifact['meshes']
        l["collision"] = block
        mat = doc.materials[l["material"]]
        root = raw[lid]["node"]
        l["print"] = {"orientation": list((root.robot or {}).get("print_orientation", [0.0, 0.0, 1.0])), "infill": (root.robot or {}).get("infill", 0.3), "walls": (root.robot or {}).get("walls", 3), "layer_height": 0.2e-3}
        l["flex"] = None
        is_motor = (root.robot or {}).get("kind") == "motor"
        extent = float(np.max(np.array(l["bbox"][1]) - np.array(l["bbox"][0])))
        if flex and mat.props().get("print") is not None and not is_motor and extent >= 5e-3 and meshes:
            frames = []
            for j in out_joints:
                if j["type"] == "fixed":
                    continue
                if j["child"] == l["name"] or j["parent"] == l["name"]:
                    ph = j["physics"]
                    frames.append({"id": j['id'], "name": j["name"], "point": (np.array(j["origin"]) - np.array(l["com"])).tolist(), "role": "root" if j["child"] == l["name"] else "outboard", "radius": ph['flex_patch_radius'], "radius_source": ph['flex_patch_source']})
            for n in doc.walk():
                if n.kind in ("sensor", "cable") and n.robot:
                    for key in ("body", "from_body", "to_body"):
                        if link_of_body.get(n.robot.get(key)) == lid:
                            pt = n.robot["point" if key == "body" else ("from_point" if key == "from_body" else "to_point")]
                            frames.append({"id": n.id if key == 'body' else n.id+'/'+key, "name": n.name, "point": (np.array(_to_m(pt)) - np.array(l["com"])).tolist(), "role": "attachment"})
            try:
                from .flex import flexible_link

                l["flex"] = cached('flex', {'collision': collision_inputs, 'link': {k: v for k, v in l.items() if k != 'collision'},
                                            'material': material_block(doc, l['material']), 'frames': frames},
                                   lambda: flexible_link(block['sdf'], l, mat, frames, verbose=verbose))
            except Exception as e:  # a link that cannot be reduced stays rigid, and says why
                l["flex"] = None
                l["flex_error"] = str(e)
        if verbose:
            print(f"  link {l['name']}: {len(block['vertices'])} vertices, sdf {block['sdf']['dims'] if block['sdf'] else None}, flex {'yes' if l['flex'] else 'no'} in {time.time() - t0:.1f} s")
        out_links.append(l)
    # Sensors, cables.
    sensors = [b for b in (sensor_block(doc, n, link_of_body, links) for n in doc.walk() if n.kind == "sensor" and n.robot) if b]
    cables = [b for b in (cable_block(doc, n, link_of_body, links) for n in doc.walk() if n.kind == "cable" and n.robot) if b]
    st = settings(doc)
    control = dict(st["control"])
    targets = {}
    for jn, v in (control.get("targets") or {}).items():
        targets[jn] = v
    for j in out_joints:
        if j["type"] in ("revolute", "continuous", "prismatic") and j["name"] not in targets:
            targets[j["name"]] = 0.0
    control["targets"] = targets
    world = dict(st["world"])
    if world.get("floor_z") is None:
        floor = min((np.array(l["bbox"][0]) + np.array(l["com"]))[2] for l in out_links) if out_links else 0.0
        # A robot standing free rests its lowest point on the floor; one bolted
        # to the world (a bench arm, a hanging leg) gets the floor a little below.
        if any(l.get("ground") for l in out_links):
            floor -= 0.02
        world["floor_z"] = float(floor)
    floor_mat = world.pop("floor_material", "world")
    mus = [_friction_pair(doc, l["material"], floor_mat) for l in out_links]
    world["floor_friction"] = float(np.mean([m[1] for m in mus])) if mus else 0.6
    world["floor_friction_static"] = float(np.mean([m[0] for m in mus])) if mus else 0.7
    model = {
        "format": "simrobot", "version": SCHEMA_VERSION,
        "source": {"file": doc.path, "exported": time.strftime("%Y-%m-%dT%H:%M:%S")},
        "gravity": [0.0, 0.0, -G],
        "world": world,
        "materials": {mid: material_block(doc, mid) for mid in sorted(materials_used | {l["material"] for l in out_links})},
        "links": out_links, "joints": out_joints, "motors": out_motors, "transmissions": transmissions,
        "battery": st["battery"], "sensors": sensors, "cables": cables,
        "control": control, "uncertainty": st["uncertainty"], "identification": st["identification"],
        "planar": {"normal": [float(c) for c in v_unit(planar.normal)], "origin": _to_m(planar.origin)} if planar is not None else None,
    }
    if path:
        with open(path, "w") as f:
            json.dump(model, f)
    if verbose:
        print(f"physical model: {len(out_links)} links, {len(out_joints)} joints, {len(out_motors)} motors, {len(sensors)} sensors, {len(cables)} cables in {time.time() - t_start:.1f} s")
    return model


# ------------------------------------------------------------------ results and identification


def load_results(doc: Document, path: str) -> dict:
    """Read a `*.simresult.json` and hang each link/joint/motor block on its
    node (`node.results`); the whole file is kept as `doc.results`."""
    with open(path) as f:
        res = json.load(f)
    by_name = {n.name: n for n in doc.walk()}
    mapping = {(m.get('section', 'links'), m['name']): m for m in res.get('cad_mapping', [])}
    for n in doc.walk():
        n.results = None
    for section in ("links", "joints", "motors"):
        for name, block in (res.get(section) or {}).items():
            mapped = mapping.get((section, name))
            ids = [mapped.get('id'), *mapped.get('members', [])] if mapped else []
            nodes = [doc.nodes[nid] for nid in dict.fromkeys(ids) if nid in doc.nodes]
            if not mapped and name in by_name:
                nodes = [by_name[name]]
            for n in nodes:
                # A mounted motor has its own summary in addition to belonging
                # to a merged link. The more specific motor block wins below.
                n.results = {"section": section, **block}
    res["path"] = path
    res["loaded"] = time.strftime("%Y-%m-%dT%H:%M:%S")
    from .snapshots import capture
    identity = res.get('provenance', {}).get('physical_hash')
    res['stale'] = identity is None or identity != capture(doc).physical_hash
    doc.results = res
    doc.dirty = True
    doc.notify("results", path)
    return doc.results


def results_margins(doc: Document) -> dict:
    """Per node id: the margins the panel and outliner show."""
    out = {}
    for n in doc.walk():
        r = n.results
        if not r:
            continue
        if r["section"] == "links":
            out[n.id] = {"yield_margin": r.get("yield_margin"), "peak_stress_pa": r.get("peak_stress_pa"), "peak_temperature_c": r.get("peak_temperature_c"), "tg_margin_c": r.get("tg_margin_c")}
        elif r["section"] == "joints":
            out[n.id] = {"bearing_margin": r.get("bearing_margin"), "screw_shear_margin": r.get("screw_shear_margin"), "peak_reaction_force_n": r.get("peak_reaction_force_n")}
        else:
            out[n.id] = {"stall_margin": r.get("stall_margin"), "peak_current_a": r.get("peak_current_a"), "peak_winding_c": r.get("peak_winding_c"), "mount_tg_margin_c": r.get("mount_tg_margin_c")}
    return out


def apply_identification(doc: Document, path: str) -> dict:
    """Store fitted joint parameters (`identification` block of a fit file or
    a results file) in the document; the next export carries them."""
    with open(path) as f:
        data = json.load(f)
    ident = data.get("identification", data)
    if not isinstance(ident, dict):
        raise ValueError("no identification block in the file")
    st = doc.robot_settings.setdefault("identification", {})
    for jn, block in ident.items():
        if isinstance(block, dict):
            block = dict(block)
            block.setdefault("source_log", data.get("source_log"))
            block.setdefault("fitted_at", data.get("fitted_at", time.strftime("%Y-%m-%dT%H:%M:%S")))
            st[jn] = block
    doc.touch()
    return st
