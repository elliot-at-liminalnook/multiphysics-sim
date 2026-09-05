"""Flexible links: a voxel finite-element model of a printed part reduced
to a handful of modes the simulator integrates as generalised coordinates.

The link's signed distance grid is coarsened to at most ~1200 cubic
8-node hexahedra (trilinear, 2×2×2 Gauss), the material is orthotropic
with the build direction weaker by the material's `anisotropy_z`, and
each joint or attachment on the link is a rigid patch (RBE2) with six
degrees of freedom. The link is clamped at its parent joint ("root")
frame; the lowest fixed-root modes are the flexible coordinates. Per mode
the export carries the frequency, the motion of every other frame
(displacement and rotation per unit modal coordinate), the modal
participation in the link's rigid acceleration, and the stress tensor at
element centroids so the simulator can recover a stress field and a yield
margin from the modal amplitudes. The static sag under 1 g is reported
as a sanity number.

Simplifications, stated: trilinear hexes are a few percent stiff in
bending at 4–5 elements through a wall; infill is homogenised (the
modulus is scaled by the infill-and-walls fraction); the FE density is
scaled so the voxel mass matches the exact link mass.
"""

from __future__ import annotations

import math
import time
from typing import Optional

import numpy as np

_MAX_ELEMENTS = 1200
_MAX_STRESS_CELLS = 2000
_MIN_MODES = 1

# Voigt order used throughout (matches the schema): xx, yy, zz, xy, yz, xz.
_VOIGT = [(0, 0), (1, 1), (2, 2), (0, 1), (1, 2), (0, 2)]


def orthotropic_stiffness(E: float, nu: float, anisotropy: float, orientation) -> np.ndarray:
    """6×6 stiffness (engineering shear) of a print: isotropic in the layer
    plane, `anisotropy·E` across layers, rotated so the material's 3-axis
    is the build direction `orientation` (link frame)."""
    E1 = E2 = E
    E3 = max(anisotropy, 0.05) * E
    G12 = E / (2 * (1 + nu))
    G13 = G23 = max(anisotropy, 0.05) * G12
    nu12, nu13, nu23 = nu, nu, nu
    nu31 = nu13 * E3 / E1
    nu32 = nu23 * E3 / E2
    nu21 = nu12 * E2 / E1
    # Compliance in material axes, then invert.
    S = np.zeros((6, 6))
    S[0, 0], S[1, 1], S[2, 2] = 1 / E1, 1 / E2, 1 / E3
    S[0, 1] = S[1, 0] = -nu21 / E2
    S[0, 2] = S[2, 0] = -nu31 / E3
    S[1, 2] = S[2, 1] = -nu32 / E3
    S[3, 3], S[4, 4], S[5, 5] = 1 / G12, 1 / G23, 1 / G13
    C = np.linalg.inv(S)
    # Rotate: tensor form, R maps material axes to link axes.
    o = np.asarray(orientation, dtype=float)
    if np.linalg.norm(o) < 1e-9:
        o = np.array([0.0, 0.0, 1.0])
    o = o / np.linalg.norm(o)
    helper = np.array([1.0, 0.0, 0.0]) if abs(o[0]) < 0.9 else np.array([0.0, 1.0, 0.0])
    a1 = np.cross(helper, o)
    a1 /= np.linalg.norm(a1)
    a2 = np.cross(o, a1)
    R = np.stack([a1, a2, o], axis=1)  # columns = material axes in link frame
    T4 = np.zeros((3, 3, 3, 3))
    for I, (i, j) in enumerate(_VOIGT):
        for J, (k, l) in enumerate(_VOIGT):
            T4[i, j, k, l] = T4[j, i, k, l] = T4[i, j, l, k] = T4[j, i, l, k] = C[I, J]
    T4r = np.einsum("ia,jb,kc,ld,abcd->ijkl", R, R, R, R, T4)
    Cr = np.zeros((6, 6))
    for I, (i, j) in enumerate(_VOIGT):
        for J, (k, l) in enumerate(_VOIGT):
            Cr[I, J] = T4r[i, j, k, l]
    return Cr


def hex_element(h: float, C: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Stiffness (24×24) of a cube of side `h` and the strain matrix at its
    centre (6×24), node order (−−−, +−−, ++−, −+−, −−+, +−+, +++, −++)."""
    corners = np.array([[-1, -1, -1], [1, -1, -1], [1, 1, -1], [-1, 1, -1], [-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1]], dtype=float)

    def bmat(xi, eta, zeta):
        dN = np.zeros((8, 3))
        for a in range(8):
            cx, cy, cz = corners[a]
            dN[a, 0] = 0.125 * cx * (1 + cy * eta) * (1 + cz * zeta)
            dN[a, 1] = 0.125 * cy * (1 + cx * xi) * (1 + cz * zeta)
            dN[a, 2] = 0.125 * cz * (1 + cx * xi) * (1 + cy * eta)
        dN *= 2.0 / h  # d/dx = d/dxi · 2/h for a cube
        B = np.zeros((6, 24))
        for a in range(8):
            bx, by, bz = dN[a]
            B[0, 3 * a] = bx
            B[1, 3 * a + 1] = by
            B[2, 3 * a + 2] = bz
            B[3, 3 * a], B[3, 3 * a + 1] = by, bx
            B[4, 3 * a + 1], B[4, 3 * a + 2] = bz, by
            B[5, 3 * a], B[5, 3 * a + 2] = bz, bx
        return B

    g = 1.0 / math.sqrt(3.0)
    K = np.zeros((24, 24))
    w = (h / 2.0) ** 3
    for xi in (-g, g):
        for eta in (-g, g):
            for zeta in (-g, g):
                B = bmat(xi, eta, zeta)
                K += B.T @ C @ B * w
    return K, bmat(0.0, 0.0, 0.0)


def _voxelize(sdf: dict, max_elements: int) -> tuple[np.ndarray, float, np.ndarray]:
    """Element centres (inside the solid) on a cubic grid of spacing h,
    coarsened until at most `max_elements`. Returns (centres, h, origin)."""
    from scipy.interpolate import RegularGridInterpolator

    dims = sdf["dims"]
    cell = sdf["cell"]
    origin = np.asarray(sdf["origin"], dtype=float)
    values = np.asarray(sdf["values"], dtype=float).reshape(dims)
    axes = [origin[i] + cell * np.arange(dims[i]) for i in range(3)]
    interp = RegularGridInterpolator(axes, values, bounds_error=False, fill_value=cell * 4)
    lo = origin
    hi = origin + cell * (np.array(dims) - 1)
    h = cell
    for _ in range(20):
        counts = np.maximum(np.floor((hi - lo) / h).astype(int), 1)
        cs = [lo[i] + h * (np.arange(counts[i]) + 0.5) for i in range(3)]
        gx, gy, gz = np.meshgrid(*cs, indexing="ij")
        centres = np.stack([gx.ravel(), gy.ravel(), gz.ravel()], axis=1)
        inside = interp(centres) < 0.0
        if inside.sum() <= max_elements or h > 0.5 * float((hi - lo).max()):
            return centres[inside], h, lo
        h *= 1.2
    return centres[inside], h, lo


def flexible_link(sdf: Optional[dict], link: dict, material, frames: list[dict], modes: int = 6, verbose: bool = False) -> Optional[dict]:
    """The reduced flexible model of a link (see the module docstring).
    `link` is the export's link block (mass, com, bbox, print), `frames`
    are `{"name", "point" (link frame), "role": root|outboard|attachment,
    "radius"?}`; without a root frame the frame nearest the COM is the root."""
    import scipy.sparse as sp
    import scipy.sparse.linalg as spla

    if sdf is None or not frames:
        return None
    t0 = time.time()
    props = material.props()
    centres, h, origin = _voxelize(sdf, _MAX_ELEMENTS)
    ne = len(centres)
    if ne < 8:
        return None
    # Nodes: element corners on the lattice.
    key = np.round((centres - origin) / h - 0.5).astype(np.int64)  # element lattice index
    corner_offsets = np.array([[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]])
    corners = key[:, None, :] + corner_offsets[None, :, :]
    flat = corners.reshape(-1, 3)
    uniq, inverse = np.unique(flat, axis=0, return_inverse=True)
    conn = inverse.reshape(ne, 8)
    nodes = origin + h * uniq.astype(float)
    nn = len(nodes)
    ndof = 3 * nn
    # Material: homogenised infill and walls.
    print_info = link.get("print") or {}
    infill = float(print_info.get("infill", 0.3))
    walls = float(print_info.get("walls", 3))
    extent = float(np.max(np.array(link["bbox"][1]) - np.array(link["bbox"][0])))
    wall_fraction = min(1.0, 2 * walls * 0.4e-3 / max(extent * 0.3, 1e-3))
    fill = min(1.0, wall_fraction + (1 - wall_fraction) * infill) if props.get("print") else 1.0
    E = props["youngs_modulus"] * (0.3 + 0.7 * fill)
    aniso = (props.get("print") or {}).get("anisotropy_z", 1.0)
    C = orthotropic_stiffness(E, props["poisson"], aniso, print_info.get("orientation", [0, 0, 1]))
    Ke, B0 = hex_element(h, C)
    # Assemble K (COO) and lumped M scaled to the exact mass.
    dof_map = (3 * conn[:, :, None] + np.arange(3)[None, None, :]).reshape(ne, 24)
    rows = np.repeat(dof_map, 24, axis=1).ravel()
    cols = np.tile(dof_map, (1, 24)).ravel()
    K = sp.coo_matrix((np.tile(Ke.ravel(), ne), (rows, cols)), shape=(ndof, ndof)).tocsr()
    density = link["mass"] / (ne * h ** 3)
    m_node = np.zeros(nn)
    np.add.at(m_node, conn.ravel(), density * h ** 3 / 8.0)
    M = sp.diags(np.repeat(m_node, 3)).tocsr()
    # Frames: rigid patches.
    order = list(frames)
    root_idx = next((i for i, f in enumerate(order) if f.get("role") == "root"), None)
    if root_idx is None:
        com = np.zeros(3)
        root_idx = int(np.argmin([np.linalg.norm(np.array(f["point"]) - com) for f in order]))
    patches = []
    patch_evidence = []
    taken = np.zeros(nn, dtype=bool)
    for f in order:
        p = np.array(f["point"], dtype=float)
        r = float(f.get("radius") or max(2.2 * h, 4.0e-3))
        d = np.linalg.norm(nodes - p, axis=1)
        idx = np.where((d <= r) & ~taken)[0]
        selection = 'within_radius'
        if len(idx) < 4:
            selection = 'nearest_available_nodes'
            available = np.flatnonzero(~taken)
            if len(available) < 4:
                raise ValueError(f'Flex boundary {f["name"]} has fewer than four available mesh nodes')
            idx = available[np.argsort(d[available])[:8]]
        taken[idx] = True
        patches.append(idx)
        patch_evidence.append({'radius_m': r, 'radius_source': f.get('radius_source', 'declared' if f.get('radius') else 'mesh_default'),
            'selection': selection, 'node_count': len(idx), 'bounds_m': [nodes[idx].min(axis=0).tolist(), nodes[idx].max(axis=0).tolist()]})
    # Transformation T: q = [6 per frame (in order) | 3 per free node] → full u.
    free_nodes = np.where(~taken)[0]
    nf = len(order)
    nq = 6 * nf + 3 * len(free_nodes)
    tr, tc, tv = [], [], []
    for fi, (f, idx) in enumerate(zip(order, patches)):
        p = np.array(f["point"], dtype=float)
        for n in idx:
            r = nodes[n] - p
            base = 6 * fi
            for a in range(3):
                tr.append(3 * n + a)
                tc.append(base + a)
                tv.append(1.0)
            # u = θ × r  → u_x = θ_y r_z − θ_z r_y, etc.
            cross = [(1, 2, r[2], -r[1]), (2, 0, r[0], -r[2]), (0, 1, r[1], -r[0])]
            for a, (i1, i2, c1, c2) in enumerate(cross):
                tr += [3 * n + a, 3 * n + a]
                tc += [base + 3 + i1, base + 3 + i2]
                tv += [c1, c2]
    for k, n in enumerate(free_nodes):
        for a in range(3):
            tr.append(3 * n + a)
            tc.append(6 * nf + 3 * k + a)
            tv.append(1.0)
    T = sp.coo_matrix((tv, (tr, tc)), shape=(ndof, nq)).tocsr()
    Kr = (T.T @ K @ T).tocsr()
    Mr = (T.T @ M @ T).tocsr()
    keep = np.ones(nq, dtype=bool)
    keep[6 * root_idx:6 * root_idx + 6] = False
    keep_idx = np.where(keep)[0]
    Kc = Kr[keep_idx][:, keep_idx]
    Mc = Mr[keep_idx][:, keep_idx] + sp.diags(np.full(len(keep_idx), 1e-12 * link["mass"]))
    m = int(max(_MIN_MODES, min(modes, Kc.shape[0] - 1)))
    try:
        lam, phi = spla.eigsh(Kc, k=m, M=Mc, sigma=0.0, which="LM")
    except Exception:
        lam, phi = spla.eigsh(Kc.toarray() if hasattr(Kc, "toarray") else Kc, k=m, M=Mc.toarray(), sigma=0.0, which="LM")
    order_l = np.argsort(lam)
    lam, phi = np.maximum(lam[order_l], 0.0), phi[:, order_l]
    # Mass-normalise.
    for i in range(m):
        s = math.sqrt(max(float(phi[:, i] @ (Mc @ phi[:, i])), 1e-30))
        phi[:, i] /= s
    freqs = np.sqrt(lam) / (2 * math.pi)
    # Back to full coordinates.
    q_full = np.zeros((nq, m))
    q_full[keep_idx] = phi
    U = T @ q_full  # ndof × m
    # Rigid-body modes about the link COM (origin of the link frame) for participation.
    Rb = np.zeros((ndof, 6))
    for n in range(nn):
        r = nodes[n]
        Rb[3 * n:3 * n + 3, 0:3] = np.eye(3)
        Rb[3 * n:3 * n + 3, 3:6] = np.array([[0, r[2], -r[1]], [-r[2], 0, r[0]], [r[1], -r[0], 0]])
    participation = (U.T @ (M @ Rb))  # m × 6
    boundary_shapes = np.zeros((m, nf, 6))
    for fi in range(nf):
        boundary_shapes[:, fi, :] = q_full[6 * fi:6 * fi + 6, :].T
    # Stress per mode at element centroids (subsampled).
    sel = np.arange(ne) if ne <= _MAX_STRESS_CELLS else np.linspace(0, ne - 1, _MAX_STRESS_CELLS).astype(int)
    stress = np.zeros((m, len(sel), 6))
    for i in range(m):
        ue = U[:, i][dof_map[sel]]  # nsel × 24
        stress[i] = (C @ (B0 @ ue.T)).T
    # Gravity sag: static response at 1 g along each axis, worst case.
    sag = 0.0
    lu = spla.splu(Kc.tocsc())
    for axis in range(3):
        f = (T.T @ (M @ Rb[:, axis])) * 9.81
        u = np.zeros(nq)
        u[keep_idx] = lu.solve(f[keep_idx])
        full = (T @ u).reshape(-1, 3)
        sag = max(sag, float(np.linalg.norm(full, axis=1).max()))
    tg = props.get("glass_transition_c", 60.0)
    out = {
        "normalization": "mass_normalized",
        "modes": m, "frequencies_hz": [float(x) for x in freqs], "damping_ratio": 0.03,
        "boundary_frames": [{"id": f.get('id'), "name": f["name"], "point": [float(c) for c in f["point"]], "role": "root" if i == root_idx else f.get("role", "outboard"), "patch": patch_evidence[i]} for i, f in enumerate(order)],
        "modal_stiffness": [float(x) for x in lam], "modal_mass": [1.0] * m,
        "boundary_shapes": [[[float(v) for v in boundary_shapes[i, fi]] for fi in range(nf)] for i in range(m)],
        "participation": [[float(v) for v in participation[i]] for i in range(m)],
        "stress_cells": [[float(c) for c in centres[j]] for j in sel],
        "stress_per_mode": [[[float(v) for v in stress[i, j]] for j in range(len(sel))] for i in range(m)],
        "gravity_sag_m": sag,
        "softening": {"tg_c": tg, "width_c": 10.0, "ratio_above": 0.05},
        "fe": {"elements": int(ne), "nodes": int(nn), "spacing": float(h), "modulus": float(E), "fill_fraction": float(fill), "anisotropy_z": float(aniso)},
    }
    if verbose:
        print(f"    flex {link['name']}: {ne} elements, {nn} nodes, h {h * 1e3:.2f} mm, f1 {freqs[0]:.1f} Hz, sag {sag * 1e3:.3f} mm in {time.time() - t0:.1f} s")
    return out
