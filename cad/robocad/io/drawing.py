"""Technical drawing SVG: a view of bodies with visible and hidden edges,
line weights and colours, hatching for section cuts, and a grid layout
template for multi-view sheets (front / top / right / iso)."""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Optional, Sequence

from OCP.BRepAdaptor import BRepAdaptor_Curve
from OCP.GeomAbs import GeomAbs_Line
from OCP.gp import gp_Ax2
from OCP.HLRAlgo import HLRAlgo_Projector
from OCP.HLRBRep import HLRBRep_Algo, HLRBRep_HLRToShape

from ..document import Document
from ..kernel import Body, Plane, Vec3
from ..kernel.base import v_add, v_cross, v_dot, v_scale, v_sub, v_unit
from ..kernel.occt import D, P, occ_edges, pt


@dataclass
class DrawingStyle:
    visible_width: float = 0.5
    hidden_width: float = 0.25
    visible_color: str = "#111111"
    hidden_color: str = "#666666"
    hidden_dash: str = "1.5,1"
    section_color: str = "#8b2323"
    hatch_spacing: float = 2.0
    hatch_angle_deg: float = 45.0
    font_size: float = 3.0


@dataclass
class View:
    name: str
    direction: Vec3  # camera looks along this vector
    up: Vec3 = (0.0, 0.0, 1.0)
    section: Optional[Plane] = None  # cut the bodies with this plane first
    scale: float = 1.0


STANDARD_VIEWS = {
    "front": View("Front", (0.0, 1.0, 0.0)),
    "top": View("Top", (0.0, 0.0, -1.0), (0.0, 1.0, 0.0)),
    "right": View("Right", (-1.0, 0.0, 0.0)),
    "iso": View("Isometric", v_unit((-1.0, 1.0, -1.0))),
}


def project_view(doc: Document, view: View, ids: Optional[Sequence[str]] = None):
    """Visible and hidden polylines in 2D (mm), plus section hatch polygons."""
    bodies = []
    for n in doc.walk():
        if ids is not None and n.id not in ids:
            continue
        if n.kind in ("body", "sheet", "instance") and doc.is_visible(n.id):
            b = doc.resolved_body(n.id)
            if b is not None:
                bodies.append(b)
    if not bodies:
        return [], [], []
    k = doc.kernel
    section_polys: list[list[tuple[float, float]]] = []
    if view.section is not None:
        cut = []
        for b in bodies:
            parts = k.cut_with_plane(b, view.section, keep="negative")
            cut.extend(parts)
            for loop in k.section(b, view.section):
                section_polys.append(loop)
        bodies = cut or bodies
    d = v_unit(view.direction)
    up = view.up if abs(v_dot(v_unit(view.up), d)) < 0.99 else (0.0, 1.0, 0.0)
    x = v_unit(v_cross(up, d))
    y = v_unit(v_cross(d, x))
    # HLR projector looks along -Z of its frame: the frame's Z is -d.
    proj = HLRAlgo_Projector(gp_Ax2(P((0.0, 0.0, 0.0)), D(v_scale(d, -1.0)), D(x)))
    algo = HLRBRep_Algo()
    for b in bodies:
        algo.Add(b.shape)
    algo.Projector(proj)
    algo.Update()
    algo.Hide()
    to = HLRBRep_HLRToShape(algo)

    def polylines(shape):
        out = []
        if shape.IsNull():
            return out
        for e in occ_edges(shape):
            ad = BRepAdaptor_Curve(e)
            f, l = ad.FirstParameter(), ad.LastParameter()
            n = 2 if ad.GetType() == GeomAbs_Line else 24
            pts = []
            for i in range(n):
                p = pt(ad.Value(f + (l - f) * i / (n - 1)))
                pts.append((p[0], p[1]))  # already in the projector's 2D frame
            out.append(pts)
        return out

    visible = polylines(to.VCompound()) + polylines(to.OutLineVCompound()) + polylines(to.Rg1LineVCompound())
    hidden = polylines(to.HCompound()) + polylines(to.OutLineHCompound())
    hatch = []
    for loop in chain_loops(section_polys):
        hatch.append([(v_dot(p, x), v_dot(p, y)) for p in loop])
    return visible, hidden, hatch


def chain_loops(polylines: list[list[Vec3]], tol: float = 1e-4) -> list[list[Vec3]]:
    """Join polyline pieces end to end into closed loops (a section comes
    back one edge at a time)."""
    pieces = [list(p) for p in polylines if len(p) >= 2]
    loops = []
    while pieces:
        cur = pieces.pop(0)
        changed = True
        while changed:
            changed = False
            for i, q in enumerate(pieces):
                if _close(cur[-1], q[0], tol):
                    cur += q[1:]
                elif _close(cur[-1], q[-1], tol):
                    cur += list(reversed(q))[1:]
                elif _close(cur[0], q[-1], tol):
                    cur = q[:-1] + cur
                elif _close(cur[0], q[0], tol):
                    cur = list(reversed(q))[:-1] + cur
                else:
                    continue
                pieces.pop(i)
                changed = True
                break
        loops.append(cur)
    return loops


def _close(a: Vec3, b: Vec3, tol: float) -> bool:
    return abs(a[0] - b[0]) < tol and abs(a[1] - b[1]) < tol and abs(a[2] - b[2]) < tol


def export_drawing_svg(doc: Document, path: str, views: Sequence[View] = (), ids: Optional[Sequence[str]] = None, style: DrawingStyle = DrawingStyle(), sheet: tuple[float, float] = (297.0, 210.0), title: str = "") -> str:
    """Lay the views out on a grid (2 columns) on an A4-landscape sheet by
    default; returns the SVG text as well as writing it."""
    views = list(views) or [STANDARD_VIEWS["front"], STANDARD_VIEWS["top"], STANDARD_VIEWS["right"], STANDARD_VIEWS["iso"]]
    cols = 2 if len(views) > 1 else 1
    rows = math.ceil(len(views) / cols)
    margin = 12.0
    cell_w = (sheet[0] - 2 * margin) / cols
    cell_h = (sheet[1] - 2 * margin - 10) / rows
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{sheet[0]}mm" height="{sheet[1]}mm" viewBox="0 0 {sheet[0]} {sheet[1]}">']
    out.append(f'<rect x="0" y="0" width="{sheet[0]}" height="{sheet[1]}" fill="white"/>')
    out.append(f'<rect x="{margin/2}" y="{margin/2}" width="{sheet[0]-margin}" height="{sheet[1]-margin}" fill="none" stroke="#333" stroke-width="0.4"/>')
    if title:
        out.append(f'<text x="{sheet[0]-margin}" y="{sheet[1]-margin/2-2}" text-anchor="end" font-family="sans-serif" font-size="{style.font_size+1}">{title}</text>')
    out.append(f'<defs><pattern id="hatch" patternUnits="userSpaceOnUse" width="{style.hatch_spacing}" height="{style.hatch_spacing}" patternTransform="rotate({style.hatch_angle_deg})"><line x1="0" y1="0" x2="0" y2="{style.hatch_spacing}" stroke="{style.section_color}" stroke-width="0.25"/></pattern></defs>')
    for i, view in enumerate(views):
        visible, hidden, hatch = project_view(doc, view, ids)
        pts = [p for poly in visible + hidden + hatch for p in poly]
        if not pts:
            continue
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        w, h = max(xs) - min(xs), max(ys) - min(ys)
        fit = min((cell_w - 10) / max(w, 1e-6), (cell_h - 12) / max(h, 1e-6))
        s = view.scale if view.scale != 1.0 else fit
        s = min(s, fit)
        cx = margin + (i % cols) * cell_w + cell_w / 2
        cy = margin + (i // cols) * cell_h + cell_h / 2 + 4
        ox = cx - s * (min(xs) + max(xs)) / 2
        oy = cy + s * (min(ys) + max(ys)) / 2

        def tr(p):
            return f"{ox + s*p[0]:.3f} {oy - s*p[1]:.3f}"

        out.append(f'<g id="view-{i}">')
        out.append(f'<text x="{cx:.2f}" y="{margin + (i // cols) * cell_h + 4:.2f}" text-anchor="middle" font-family="sans-serif" font-size="{style.font_size}">{view.name}  1:{1/s:.2g}</text>')
        for poly in hatch:
            if len(poly) >= 3:
                out.append(f'<path d="M {" L ".join(tr(p) for p in poly)} Z" fill="url(#hatch)" stroke="{style.section_color}" stroke-width="{style.visible_width}"/>')
        for poly in hidden:
            out.append(f'<path d="M {" L ".join(tr(p) for p in poly)}" fill="none" stroke="{style.hidden_color}" stroke-width="{style.hidden_width}" stroke-dasharray="{style.hidden_dash}"/>')
        for poly in visible:
            out.append(f'<path d="M {" L ".join(tr(p) for p in poly)}" fill="none" stroke="{style.visible_color}" stroke-width="{style.visible_width}" stroke-linecap="round"/>')
        out.append("</g>")
    out.append("</svg>")
    text = "\n".join(out)
    with open(path, "w") as f:
        f.write(text)
    return text
