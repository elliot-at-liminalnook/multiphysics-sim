"""Exporters: STEP, IGES, STL, 3MF, OBJ (+MTL). Every mesh export runs
manifold validation first. Settings dataclasses remember the last values
through `ExportSettings` in the UI's preferences."""

from __future__ import annotations

import io
import math
import os
import struct
import zipfile
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from typing import Optional, Sequence
from xml.sax.saxutils import escape

from ..document import Document, Node
from ..kernel import Body, GeometryKernel, KernelError
from ..kernel.base import Mesh, Vec3, v_cross, v_sub, v_unit
from ..printing import validate_for_export, weld
from ..units import LENGTH_UNITS


class ExportError(RuntimeError):
    pass


def _bodies(doc: Document, ids: Optional[Sequence[str]]) -> list[tuple[Node, Body]]:
    out = []
    for n in doc.walk():
        if ids is not None and n.id not in ids:
            continue
        if n.kind in ("body", "sheet", "instance") and doc.is_visible(n.id):
            b = doc.resolved_body(n.id)
            if b is not None:
                out.append((n, b))
    if not out:
        raise ExportError("nothing to export: no visible bodies")
    return out


def _validated(doc: Document, items: list[tuple[Node, Body]], require: bool = True):
    ok, messages = validate_for_export(doc.kernel, [(n.name, b) for n, b in items if n.kind != "sheet"])
    if not ok and require:
        raise ExportError("export blocked by validation:\n" + "\n".join(messages))
    return messages


def tessellate_all(doc: Document, items: list[tuple[Node, Body]], tolerance: float) -> list[Mesh]:
    """Parallel tessellation across bodies."""
    with ThreadPoolExecutor() as pool:
        return list(pool.map(lambda nb: weld(doc.kernel.tessellate(nb[1], tolerance)), items))


# ---------------------------------------------------------------- STEP/IGES


@dataclass
class StepSettings:
    schema: str = "AP214"  # AP203 | AP214 | AP242
    names: bool = True
    colors: bool = True


def export_step(doc: Document, path: str, ids: Optional[Sequence[str]] = None, settings: StepSettings = StepSettings()):
    from OCP.Interface import Interface_Static
    from OCP.STEPControl import STEPControl_AsIs, STEPControl_Writer
    from OCP.TCollection import TCollection_ExtendedString, TCollection_HAsciiString
    from OCP.XSControl import XSControl_WorkSession

    items = _bodies(doc, ids)
    if settings.names or settings.colors:
        try:
            _export_step_xde(doc, path, items, settings)
            return
        except Exception:
            pass  # fall back to the plain writer
    Interface_Static.SetCVal_s("write.step.schema", {"AP203": "AP203", "AP214": "AP214CD", "AP242": "AP242DIS"}.get(settings.schema, "AP214CD"))
    Interface_Static.SetCVal_s("write.step.unit", "MM")
    writer = STEPControl_Writer()
    for n, b in items:
        writer.Transfer(b.shape, STEPControl_AsIs)
    if writer.Write(path) != 1:
        raise ExportError("STEP write failed")


def _export_step_xde(doc: Document, path: str, items, settings: StepSettings):
    from OCP.Interface import Interface_Static
    from OCP.Quantity import Quantity_Color, Quantity_TOC_RGB
    from OCP.STEPCAFControl import STEPCAFControl_Writer
    from OCP.STEPControl import STEPControl_AsIs
    from OCP.TCollection import TCollection_ExtendedString
    from OCP.TDataStd import TDataStd_Name
    from OCP.TDocStd import TDocStd_Document
    from OCP.XCAFDoc import XCAFDoc_ColorGen, XCAFDoc_DocumentTool

    Interface_Static.SetCVal_s("write.step.schema", {"AP203": "AP203", "AP214": "AP214CD", "AP242": "AP242DIS"}.get(settings.schema, "AP214CD"))
    tdoc = TDocStd_Document(TCollection_ExtendedString("robocad"))
    shapes = XCAFDoc_DocumentTool.ShapeTool_s(tdoc.Main())
    colors = XCAFDoc_DocumentTool.ColorTool_s(tdoc.Main())
    for n, b in items:
        label = shapes.AddShape(b.shape, False)
        if settings.names:
            TDataStd_Name.Set_s(label, TCollection_ExtendedString(n.name))
        if settings.colors:
            mat = doc.materials.get(n.material or "")
            c = n.color or (mat.color if mat else (0.7, 0.7, 0.72))
            colors.SetColor(label, Quantity_Color(c[0], c[1], c[2], Quantity_TOC_RGB), XCAFDoc_ColorGen)
    writer = STEPCAFControl_Writer()
    writer.SetColorMode(settings.colors)
    writer.SetNameMode(settings.names)
    writer.Transfer(tdoc, STEPControl_AsIs)
    if writer.Write(path) != 1:
        raise ExportError("STEP write failed")


def export_iges(doc: Document, path: str, ids: Optional[Sequence[str]] = None):
    from OCP.IGESControl import IGESControl_Controller, IGESControl_Writer

    IGESControl_Controller.Init_s()
    writer = IGESControl_Writer("MM", 0)
    for n, b in _bodies(doc, ids):
        writer.AddShape(b.shape)
    writer.ComputeModel()
    if not writer.Write(path):
        raise ExportError("IGES write failed")


# ---------------------------------------------------------------------- STL


@dataclass
class StlSettings:
    binary: bool = True
    unit: str = "mm"  # output unit: mm | cm | m | in | ft
    tolerance: float = 0.05  # chord tolerance in mm
    angular_deg: float = 20.0


def export_stl(doc: Document, path: str, ids: Optional[Sequence[str]] = None, settings: StlSettings = StlSettings()) -> list[str]:
    items = _bodies(doc, ids)
    warnings = _validated(doc, items)
    meshes = tessellate_all(doc, items, settings.tolerance)
    scale = 1.0 / LENGTH_UNITS[settings.unit]
    tris = []
    for m in meshes:
        for a, b, c in m.triangles:
            pa, pb, pc = m.vertices[a], m.vertices[b], m.vertices[c]
            n = v_unit(v_cross(v_sub(pb, pa), v_sub(pc, pa)))
            tris.append((n, tuple(x * scale for x in pa), tuple(x * scale for x in pb), tuple(x * scale for x in pc)))
    if settings.binary:
        with open(path, "wb") as f:
            f.write(b"robocad binary STL".ljust(80, b"\0"))
            f.write(struct.pack("<I", len(tris)))
            for n, a, b, c in tris:
                f.write(struct.pack("<12fH", *n, *a, *b, *c, 0))
    else:
        with open(path, "w") as f:
            f.write("solid robocad\n")
            for n, a, b, c in tris:
                f.write(f"  facet normal {n[0]:.6e} {n[1]:.6e} {n[2]:.6e}\n    outer loop\n")
                for p in (a, b, c):
                    f.write(f"      vertex {p[0]:.6e} {p[1]:.6e} {p[2]:.6e}\n")
                f.write("    endloop\n  endfacet\n")
            f.write("endsolid robocad\n")
    return warnings


# ---------------------------------------------------------------------- 3MF


@dataclass
class ThreeMfSettings:
    tolerance: float = 0.05
    colors: bool = True
    names: bool = True


def export_3mf(doc: Document, path: str, ids: Optional[Sequence[str]] = None, settings: ThreeMfSettings = ThreeMfSettings()) -> list[str]:
    """Multi-body 3MF with per-object names and base-material colours."""
    items = _bodies(doc, ids)
    warnings = _validated(doc, items)
    meshes = tessellate_all(doc, items, settings.tolerance)
    ns = 'xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:m="http://schemas.microsoft.com/3dmanufacturing/material/2015/02"'
    parts = [f'<?xml version="1.0" encoding="UTF-8"?>\n<model unit="millimeter" xml:lang="en-US" {ns}>\n<resources>\n']
    # Base materials: one per distinct colour.
    colors = []
    color_index = {}
    for n, _ in items:
        mat = doc.materials.get(n.material or "")
        c = n.color or (mat.color if mat else (0.7, 0.7, 0.72))
        key = tuple(round(x, 3) for x in c)
        if key not in color_index:
            color_index[key] = len(colors)
            colors.append((key, mat.name if mat else "default"))
    if settings.colors and colors:
        parts.append('<m:basematerials id="1">\n')
        for (r, g, b), name in colors:
            parts.append(f'<m:base name="{escape(name)}" displaycolor="#{int(r*255):02X}{int(g*255):02X}{int(b*255):02X}FF"/>\n')
        parts.append("</m:basematerials>\n")
    object_ids = []
    for k, ((n, b), m) in enumerate(zip(items, meshes)):
        oid = k + 2
        object_ids.append(oid)
        mat = doc.materials.get(n.material or "")
        c = n.color or (mat.color if mat else (0.7, 0.7, 0.72))
        pid = f' pid="1" pindex="{color_index[tuple(round(x, 3) for x in c)]}"' if settings.colors else ""
        name = f' name="{escape(n.name)}"' if settings.names else ""
        parts.append(f'<object id="{oid}" type="model"{name}{pid}>\n<mesh>\n<vertices>\n')
        for v in m.vertices:
            parts.append(f'<vertex x="{v[0]:.5f}" y="{v[1]:.5f}" z="{v[2]:.5f}"/>\n')
        parts.append("</vertices>\n<triangles>\n")
        for a, bb, cc in m.triangles:
            parts.append(f'<triangle v1="{a}" v2="{bb}" v3="{cc}"/>\n')
        parts.append("</triangles>\n</mesh>\n</object>\n")
    parts.append("</resources>\n<build>\n")
    for oid in object_ids:
        parts.append(f'<item objectid="{oid}"/>\n')
    parts.append("</build>\n</model>\n")
    model = "".join(parts)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", '<?xml version="1.0" encoding="UTF-8"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>')
        z.writestr("_rels/.rels", '<?xml version="1.0" encoding="UTF-8"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>')
        z.writestr("3D/3dmodel.model", model)
    return warnings


# ---------------------------------------------------------------------- OBJ


@dataclass
class ObjSettings:
    tolerance: float = 0.05
    scale: float = 1.0
    up_axis: str = "Z"  # Z | Y
    quads: bool = False  # merge coplanar triangle pairs into quads
    ngons: bool = False  # planar faces as n-gons where the tessellation allows
    mtl: bool = True
    uvs: bool = True


def export_obj(doc: Document, path: str, ids: Optional[Sequence[str]] = None, settings: ObjSettings = ObjSettings()) -> list[str]:
    items = _bodies(doc, ids)
    warnings = _validated(doc, items, require=False)
    meshes = tessellate_all(doc, items, settings.tolerance)
    base = os.path.splitext(path)[0]
    mtl_path = base + ".mtl"
    lines = ["# robocad OBJ", f"mtllib {os.path.basename(mtl_path)}" if settings.mtl else "#"]
    mtl = []
    offset = 1
    uv_offset = 1
    for (n, b), m in zip(items, meshes):
        mat = doc.materials.get(n.material or "")
        c = n.color or (mat.color if mat else (0.7, 0.7, 0.72))
        mname = (mat.name if mat else "default").replace(" ", "_")
        if settings.mtl:
            mtl.append(f"newmtl {mname}\nKd {c[0]:.3f} {c[1]:.3f} {c[2]:.3f}\nKs 0.1 0.1 0.1\nNs {int(10 + 200*(1-(mat.roughness if mat else 0.5)))}\nd 1.0\n")
        lines.append(f"o {n.name.replace(' ', '_')}")
        if settings.mtl:
            lines.append(f"usemtl {mname}")
        for v in m.vertices:
            x, y, z = (x * settings.scale for x in v)
            if settings.up_axis == "Y":
                x, y, z = x, z, -y
            lines.append(f"v {x:.5f} {y:.5f} {z:.5f}")
        for nn in m.normals:
            x, y, z = nn
            if settings.up_axis == "Y":
                x, y, z = x, z, -y
            lines.append(f"vn {x:.4f} {y:.4f} {z:.4f}")
        if settings.uvs:
            # Planar box-projection UVs per face group: cheap and stable.
            for v in m.vertices:
                lines.append(f"vt {(v[0] % 100) / 100:.4f} {(v[1] % 100) / 100:.4f}")
        faces = _group_faces(m, settings)
        for poly in faces:
            if settings.uvs:
                lines.append("f " + " ".join(f"{i + offset}/{i + uv_offset}/{i + offset}" for i in poly))
            else:
                lines.append("f " + " ".join(f"{i + offset}//{i + offset}" for i in poly))
        offset += len(m.vertices)
        uv_offset += len(m.vertices)
    with open(path, "w") as f:
        f.write("\n".join(lines) + "\n")
    if settings.mtl:
        with open(mtl_path, "w") as f:
            f.write("\n".join(mtl))
    return warnings


def _group_faces(m: Mesh, settings: ObjSettings) -> list[list[int]]:
    tris = [list(t) for t in m.triangles]
    if not (settings.quads or settings.ngons):
        return tris
    # Merge coplanar adjacent triangles of the same B-rep face into quads.
    out = []
    used = [False] * len(tris)
    by_edge: dict[tuple[int, int], list[int]] = {}
    for i, t in enumerate(tris):
        for e in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
            by_edge.setdefault((min(e), max(e)), []).append(i)
    for i, t in enumerate(tris):
        if used[i]:
            continue
        merged = None
        for e in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
            key = (min(e), max(e))
            for j in by_edge.get(key, []):
                if j != i and not used[j] and m.triangle_face[j] == m.triangle_face[i]:
                    u = tris[j]
                    other = [v for v in u if v not in e][0]
                    a = t.index(e[0])
                    b = t.index(e[1])
                    # insert `other` between e[0] and e[1] going around t
                    if (a + 1) % 3 == b:
                        merged = t[: b] + [other] + t[b:] if b != 0 else t + [other]
                    else:
                        merged = t[: a] + [other] + t[a:] if a != 0 else t + [other]
                    if _is_convex_planar(m, merged):
                        used[j] = True
                        break
                    merged = None
            if merged:
                break
        used[i] = True
        out.append(merged or t)
    return out


def _is_convex_planar(m: Mesh, poly: list[int]) -> bool:
    if len(poly) < 4:
        return True
    pts = [m.vertices[i] for i in poly]
    n = v_unit(v_cross(v_sub(pts[1], pts[0]), v_sub(pts[2], pts[0])))
    for i in range(len(pts)):
        a, b, c = pts[i], pts[(i + 1) % len(pts)], pts[(i + 2) % len(pts)]
        cr = v_cross(v_sub(b, a), v_sub(c, b))
        if cr[0] * n[0] + cr[1] * n[1] + cr[2] * n[2] < -1e-9:
            return False
    return True


# ------------------------------------------------------------- sketch SVG


def export_sketch_svg(doc: Document, path: str, sketch_id: str, stroke: str = "#222", width: float = 0.35):
    node = doc.nodes[sketch_id]
    sk = node.sketch
    paths = []
    xs, ys = [], []
    for c in sk.curves:
        pts = c.sample(64)
        if c.kind == "slot":
            pts = _slot_points(c)
        if not pts:
            continue
        xs.extend(p[0] for p in pts)
        ys.extend(-p[1] for p in pts)
        d = "M " + " L ".join(f"{p[0]:.3f} {-p[1]:.3f}" for p in pts) + (" Z" if c.closed or c.kind in ("circle", "ellipse", "slot") else "")
        paths.append(f'<path d="{d}" fill="none" stroke="{stroke}" stroke-width="{width}"/>')
    if not xs:
        xs, ys = [0.0], [0.0]
    x0, y0, x1, y1 = min(xs) - 5, min(ys) - 5, max(xs) + 5, max(ys) + 5
    with open(path, "w") as f:
        f.write(f'<svg xmlns="http://www.w3.org/2000/svg" width="{x1-x0:.2f}mm" height="{y1-y0:.2f}mm" viewBox="{x0:.3f} {y0:.3f} {x1-x0:.3f} {y1-y0:.3f}">\n')
        f.write("\n".join(paths))
        f.write("\n</svg>\n")


def _slot_points(c) -> list[tuple[float, float]]:
    a, b = c.points
    d = (b[0] - a[0], b[1] - a[1])
    L = math.hypot(*d) or 1.0
    d = (d[0] / L, d[1] / L)
    n = (-d[1], d[0])
    r = c.radius
    out = []
    ang = math.atan2(n[1], n[0])
    for k in range(17):
        t = ang + math.pi * k / 16
        out.append((b[0] + r * math.cos(t), b[1] + r * math.sin(t)))
    for k in range(17):
        t = ang + math.pi + math.pi * k / 16
        out.append((a[0] + r * math.cos(t), a[1] + r * math.sin(t)))
    return out
