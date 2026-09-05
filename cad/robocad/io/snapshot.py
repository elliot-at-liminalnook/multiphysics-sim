"""Headless screenshots: a small software rasteriser (painter's algorithm
with flat shading) so scripts and CI can picture a document without a
GPU or a window. The interactive viewport uses OpenGL; this is for
thumbnails, the acceptance walkthrough and documentation."""

from __future__ import annotations

import math
from typing import Optional, Sequence

from PIL import Image, ImageDraw

from ..document import Document
from ..kernel import Plane, Vec3
from ..kernel.base import v_add, v_cross, v_dist, v_dot, v_norm, v_scale, v_sub, v_unit


def render(doc: Document, path: str, size: tuple[int, int] = (1200, 900), view: Vec3 = (-1.0, -1.4, 0.9), ids: Optional[Sequence[str]] = None, section: Optional[Plane] = None, tolerance: float = 0.15, highlight: Optional[Sequence[str]] = None, title: str = "", mode: str = "shaded", edges: bool = True, labels: bool = False, focus_ids: Optional[Sequence[str]] = None) -> str:
    """`mode`: shaded | xray (translucent faces, all edges) | wireframe.
    `edges` draws the B-rep edges; `labels` writes node names at their
    centroids; `focus_ids` frames those nodes instead of the whole scene."""
    w, h = size
    img = Image.new("RGB", size, (30, 32, 36))
    draw = ImageDraw.Draw(img)
    tris = []  # (depth, [(x,y),...], color)
    lo, hi = [math.inf] * 3, [-math.inf] * 3
    flo, fhi = [math.inf] * 3, [-math.inf] * 3
    meshes = []
    for n in doc.walk():
        if ids is not None and n.id not in ids:
            continue
        if n.kind not in ("body", "sheet", "instance", "mesh") or not doc.is_visible(n.id):
            continue
        m = doc.mesh_of(n.id, tolerance)
        if m is None:
            continue
        mat = doc.materials.get(n.material or "")
        color = n.color or (mat.color if mat else (0.7, 0.7, 0.72))
        if highlight and n.id in highlight:
            color = (1.0, 0.65, 0.25)
        meshes.append((n, m, color))
        bl, bh = m.bounds()
        lo = [min(lo[i], bl[i]) for i in range(3)]
        hi = [max(hi[i], bh[i]) for i in range(3)]
        if focus_ids and n.id in focus_ids:
            flo = [min(flo[i], bl[i]) for i in range(3)]
            fhi = [max(fhi[i], bh[i]) for i in range(3)]
    if lo[0] is math.inf:
        lo, hi = [-50, -50, 0], [50, 50, 50]
    if focus_ids and flo[0] is not math.inf:
        lo, hi = flo, fhi
    center = v_scale(v_add(tuple(lo), tuple(hi)), 0.5)
    radius = max(v_dist(tuple(lo), tuple(hi)) / 2, 1.0)
    back = v_unit(view)
    up = (0.0, 0.0, 1.0)
    right = v_unit(v_cross(up, back))
    up2 = v_unit(v_cross(back, right))
    scale = min(w, h) * 0.42 / radius
    light = v_unit((0.3, -0.5, 0.8))

    def project(p: Vec3):
        d = v_sub(p, center)
        return (w / 2 + v_dot(d, right) * scale, h / 2 - v_dot(d, up2) * scale, v_dot(d, back))

    # grid on z = lo[2]
    step = 10.0 if radius < 150 else 50.0
    z0 = lo[2]
    n = 12
    for i in range(-n, n + 1):
        a = project((center[0] + i * step, center[1] - n * step, z0))
        b = project((center[0] + i * step, center[1] + n * step, z0))
        c = project((center[0] - n * step, center[1] + i * step, z0))
        d = project((center[0] + n * step, center[1] + i * step, z0))
        col = (60, 62, 68) if i % 5 else (85, 88, 95)
        draw.line([a[:2], b[:2]], fill=col)
        draw.line([c[:2], d[:2]], fill=col)
    for n, m, color in meshes:
        if mode == "wireframe":
            continue
        for a, b, c in m.triangles:
            pa, pb, pc = m.vertices[a], m.vertices[b], m.vertices[c]
            if section is not None:
                nrm = v_unit(section.normal)
                if all(v_dot(v_sub(p, section.origin), nrm) > 0 for p in (pa, pb, pc)):
                    continue
            fn = v_cross(v_sub(pb, pa), v_sub(pc, pa))
            L = v_norm(fn)
            if L < 1e-12:
                continue
            fn = v_scale(fn, 1 / L)
            shade = 0.35 + 0.65 * max(0.0, v_dot(fn, light))
            rim = 0.15 * max(0.0, 1 - abs(v_dot(fn, back)))
            col = tuple(min(255, int(255 * (c * shade + rim))) for c in color)
            qa, qb, qc = project(pa), project(pb), project(pc)
            tris.append(((qa[2] + qb[2] + qc[2]) / 3, [qa[:2], qb[:2], qc[:2]], col))
    tris.sort(key=lambda t: t[0])
    if mode == "xray":
        overlay = Image.new("RGBA", size, (0, 0, 0, 0))
        odraw = ImageDraw.Draw(overlay)
        for _, poly, col in tris:
            odraw.polygon(poly, fill=(*col, 70))
        img = Image.alpha_composite(img.convert("RGBA"), overlay).convert("RGB")
        draw = ImageDraw.Draw(img)
    else:
        for _, poly, col in tris:
            draw.polygon(poly, fill=col)
    if edges or mode == "wireframe":
        k = doc.kernel
        for n, m, color in meshes:
            body = doc.resolved_body(n.id)
            if body is None:
                continue
            ecol = (20, 20, 24) if mode == "shaded" else tuple(int(255 * c) for c in color)
            try:
                for e in k.edges(body):
                    pts = k.sample_edge(e, body, 2 if e.kind.value == "line" else 24)
                    if section is not None:
                        nrm = v_unit(section.normal)
                        pts = [p for p in pts if v_dot(v_sub(p, section.origin), nrm) <= 0]
                        if len(pts) < 2:
                            continue
                    draw.line([project(p)[:2] for p in pts], fill=ecol, width=1)
            except Exception:
                pass
    if labels:
        for n, m, color in meshes:
            b = m.bounds()
            c = v_scale(v_add(b[0], b[1]), 0.5)
            x, y, _ = project(c)
            draw.rectangle([x - 3, y - 8, x + 6 * len(n.name) + 6, y + 8], fill=(20, 22, 26))
            draw.text((x, y - 6), n.name, fill=(255, 220, 120))
    if section is not None:
        from ..analysis import section_outline

        for loop in section_outline(doc, section, ids):
            pts = [project(p)[:2] for p in loop]
            if len(pts) >= 2:
                draw.line(pts, fill=(255, 100, 80), width=2)
    if title:
        draw.text((12, 10), title, fill=(230, 230, 235))
    img.save(path)
    return path
