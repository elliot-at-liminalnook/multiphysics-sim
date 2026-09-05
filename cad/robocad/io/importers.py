"""Importers: STEP/STP (colours, names, assembly hierarchy), IGES, meshes
(OBJ/STL/3MF/FBX via trimesh, STL via OCCT as well), SVG curves, and
reference images. Mesh formats carry no unit: `mesh_units_guess` and the
`unit` argument let the UI ask before placing them."""

from __future__ import annotations

import math
import os
import re
from dataclasses import dataclass
from typing import Optional

from ..document import Document, Node, Transform
from ..kernel import Body, Plane, Sketch
from ..kernel.base import Mesh, Vec3, v_cross, v_sub, v_unit
from ..units import LENGTH_UNITS


class ImportError_(RuntimeError):
    pass


# ------------------------------------------------------------------ STEP


def import_step(doc: Document, path: str, parent: Optional[str] = None) -> list[str]:
    """STEP with XDE: names, colours and the assembly tree become groups."""
    from OCP.IFSelect import IFSelect_RetDone
    from OCP.Quantity import Quantity_Color
    from OCP.STEPCAFControl import STEPCAFControl_Reader
    from OCP.TCollection import TCollection_ExtendedString
    from OCP.TDataStd import TDataStd_Name
    from OCP.TDF import TDF_Label, TDF_LabelSequence
    from OCP.TDocStd import TDocStd_Document
    from OCP.TopLoc import TopLoc_Location
    from OCP.XCAFDoc import XCAFDoc_ColorSurf, XCAFDoc_DocumentTool, XCAFDoc_ShapeTool

    reader = STEPCAFControl_Reader()
    reader.SetColorMode(True)
    reader.SetNameMode(True)
    if reader.ReadFile(path) != IFSelect_RetDone:
        raise ImportError_(f"could not read {path}")
    tdoc = TDocStd_Document(TCollection_ExtendedString("import"))
    reader.Transfer(tdoc)
    shapes = XCAFDoc_DocumentTool.ShapeTool_s(tdoc.Main())
    colors = XCAFDoc_DocumentTool.ColorTool_s(tdoc.Main())
    roots = TDF_LabelSequence()
    shapes.GetFreeShapes(roots)
    created: list[str] = []

    def label_name(label: TDF_Label) -> str:
        n = TDataStd_Name()
        if label.FindAttribute(TDataStd_Name.GetID_s(), n):
            return n.Get().ToExtString() if hasattr(n.Get(), "ToExtString") else str(n.Get())
        return "Part"

    def label_color(label: TDF_Label):
        from OCP.XCAFDoc import XCAFDoc_ColorTool

        c = Quantity_Color()
        for getter in (lambda: XCAFDoc_ColorTool.GetColor_s(label, XCAFDoc_ColorSurf, c), lambda: colors.GetColor(label, c)):
            try:
                if getter():
                    return (c.Red(), c.Green(), c.Blue())
            except TypeError:
                continue
        return None

    def visit(label: TDF_Label, parent_id: Optional[str], loc: TopLoc_Location):
        name = label_name(label)
        if XCAFDoc_ShapeTool.IsAssembly_s(label):
            g = doc.add_group(doc.unique_name(name), parent_id)
            created.append(g.id)
            comps = TDF_LabelSequence()
            XCAFDoc_ShapeTool.GetComponents_s(label, comps)
            for i in range(1, comps.Length() + 1):
                comp = comps.Value(i)
                ref = TDF_Label()
                XCAFDoc_ShapeTool.GetReferredShape_s(comp, ref)
                cloc = loc.Multiplied(XCAFDoc_ShapeTool.GetLocation_s(comp))
                visit(ref if not ref.IsNull() else comp, g.id, cloc)
            return
        shape = XCAFDoc_ShapeTool.GetShape_s(label)
        if shape.IsNull():
            return
        shape = shape.Moved(loc)
        from ..kernel.occt import _solid_kind

        body = Body(shape, _solid_kind(shape))
        node = doc.add_body(body, doc.unique_name(name), parent=parent_id)
        col = label_color(label)
        if col:
            node.color = col
        created.append(node.id)

    for i in range(1, roots.Length() + 1):
        visit(roots.Value(i), parent, TopLoc_Location())
    if not created:
        # No XDE structure: plain transfer.
        from OCP.STEPControl import STEPControl_Reader

        r = STEPControl_Reader()
        r.ReadFile(path)
        r.TransferRoots()
        from ..kernel.occt import _solid_kind, explore
        from OCP.TopAbs import TopAbs_SOLID

        shape = r.OneShape()
        solids = explore(shape, TopAbs_SOLID) or [shape]
        for s in solids:
            created.append(doc.add_body(Body(s, _solid_kind(s)), os.path.splitext(os.path.basename(path))[0], parent=parent).id)
    return created


def import_iges(doc: Document, path: str, parent: Optional[str] = None) -> list[str]:
    from OCP.IFSelect import IFSelect_RetDone
    from OCP.IGESControl import IGESControl_Reader

    r = IGESControl_Reader()
    if r.ReadFile(path) != IFSelect_RetDone:
        raise ImportError_(f"could not read {path}")
    r.TransferRoots()
    from ..kernel.occt import _solid_kind, explore
    from OCP.TopAbs import TopAbs_SOLID

    shape = r.OneShape()
    solids = explore(shape, TopAbs_SOLID) or [shape]
    return [doc.add_body(Body(s, _solid_kind(s)), os.path.splitext(os.path.basename(path))[0], parent=parent).id for s in solids]


# ----------------------------------------------------------------- meshes


def mesh_units_guess(extent: float) -> str:
    """A hint for the unit prompt from the model's largest extent."""
    if extent < 1.0:
        return "m"
    if extent < 20.0:
        return "in"
    return "mm"


def load_mesh_file(path: str, unit: str = "mm") -> tuple[Mesh, float]:
    """A mesh in millimetres and its largest raw extent (for the unit prompt)."""
    import numpy as np
    import trimesh

    loaded = trimesh.load(path, force="mesh", process=False)
    if isinstance(loaded, trimesh.Scene):
        loaded = trimesh.util.concatenate([g for g in loaded.geometry.values()])
    verts = np.asarray(loaded.vertices, dtype=float)
    faces = np.asarray(loaded.faces, dtype=int)
    extent = float(np.max(verts.max(axis=0) - verts.min(axis=0))) if len(verts) else 0.0
    scale = LENGTH_UNITS[unit]
    verts = verts * scale
    normals = np.asarray(loaded.vertex_normals, dtype=float) if hasattr(loaded, "vertex_normals") else np.zeros_like(verts)
    mesh = Mesh([tuple(map(float, v)) for v in verts], [tuple(map(float, n)) for n in normals], [tuple(map(int, f)) for f in faces], list(range(len(faces))), len(faces))
    return mesh, extent


def import_mesh(doc: Document, path: str, unit: str = "mm", parent: Optional[str] = None) -> str:
    mesh, _ = load_mesh_file(path, unit)
    node = doc.add_mesh(mesh, os.path.basename(path), parent=parent)
    return node.id


# --------------------------------------------------------------------- SVG


_NUM = r"[-+]?(?:\d+\.\d*|\.\d+|\d+)(?:[eE][-+]?\d+)?"


def import_svg(doc: Document, path: str, plane: Plane = Plane.xy(), scale: float = 1.0, parent: Optional[str] = None) -> str:
    """Paths, lines, polylines, polygons, rects and circles as sketch curves
    (curves are flattened; SVG y is flipped so drawings read upright)."""
    import xml.etree.ElementTree as ET

    tree = ET.parse(path)
    root = tree.getroot()
    sk = Sketch(plane, [], os.path.basename(path))

    def strip(tag: str) -> str:
        return tag.split("}")[-1]

    def flip(p):
        return (p[0] * scale, -p[1] * scale)

    for el in root.iter():
        tag = strip(el.tag)
        if tag == "line":
            sk.line(flip((float(el.get("x1", 0)), float(el.get("y1", 0)))), flip((float(el.get("x2", 0)), float(el.get("y2", 0)))))
        elif tag in ("polyline", "polygon"):
            nums = [float(v) for v in re.findall(_NUM, el.get("points", ""))]
            pts = [flip((nums[i], nums[i + 1])) for i in range(0, len(nums) - 1, 2)]
            if len(pts) >= 2:
                sk.polyline(pts, closed=tag == "polygon")
        elif tag == "rect":
            x, y, w, h = (float(el.get(a, 0)) for a in ("x", "y", "width", "height"))
            sk.polyline([flip((x, y)), flip((x + w, y)), flip((x + w, y + h)), flip((x, y + h))], closed=True)
        elif tag == "circle":
            sk.circle(flip((float(el.get("cx", 0)), float(el.get("cy", 0)))), float(el.get("r", 0)) * scale)
        elif tag == "ellipse":
            sk.ellipse(flip((float(el.get("cx", 0)), float(el.get("cy", 0)))), float(el.get("rx", 0)) * scale, float(el.get("ry", 0)) * scale)
        elif tag == "path":
            for sub, closed in _svg_path_polylines(el.get("d", "")):
                if len(sub) >= 2:
                    sk.polyline([flip(p) for p in sub], closed=closed)
    node = doc.add_sketch(sk, os.path.basename(path), parent)
    return node.id


def _svg_path_polylines(d: str) -> list[tuple[list[tuple[float, float]], bool]]:
    tokens = re.findall(r"[MmLlHhVvCcSsQqTtAaZz]|" + _NUM, d)
    out = []
    cur: list[tuple[float, float]] = []
    pos = (0.0, 0.0)
    start = (0.0, 0.0)
    i = 0
    cmd = None
    last_ctrl = None

    def nums(n):
        nonlocal i
        vals = [float(tokens[i + k]) for k in range(n)]
        i += n
        return vals

    while i < len(tokens):
        t = tokens[i]
        if re.match(r"[A-Za-z]", t):
            cmd = t
            i += 1
            if cmd in "Zz":
                if cur:
                    out.append((cur, True))
                cur = []
                pos = start
                continue
        if cmd is None:
            i += 1
            continue
        rel = cmd.islower()
        c = cmd.upper()
        if c == "M":
            x, y = nums(2)
            pos = (pos[0] + x, pos[1] + y) if rel else (x, y)
            if cur:
                out.append((cur, False))
            cur = [pos]
            start = pos
            cmd = "l" if rel else "L"
        elif c == "L":
            x, y = nums(2)
            pos = (pos[0] + x, pos[1] + y) if rel else (x, y)
            cur.append(pos)
        elif c == "H":
            (x,) = nums(1)
            pos = (pos[0] + x if rel else x, pos[1])
            cur.append(pos)
        elif c == "V":
            (y,) = nums(1)
            pos = (pos[0], pos[1] + y if rel else y)
            cur.append(pos)
        elif c in ("C", "S", "Q", "T"):
            n = {"C": 6, "S": 4, "Q": 4, "T": 2}[c]
            vals = nums(n)
            pts = [(vals[k], vals[k + 1]) for k in range(0, n, 2)]
            if rel:
                pts = [(pos[0] + p[0], pos[1] + p[1]) for p in pts]
            if c == "S":
                refl = (2 * pos[0] - last_ctrl[0], 2 * pos[1] - last_ctrl[1]) if last_ctrl else pos
                pts = [refl] + pts
            if c == "T":
                refl = (2 * pos[0] - last_ctrl[0], 2 * pos[1] - last_ctrl[1]) if last_ctrl else pos
                pts = [refl] + pts
            ctrl = [pos] + pts
            for k in range(1, 9):
                s = k / 8
                tmp = list(ctrl)
                while len(tmp) > 1:
                    tmp = [((1 - s) * tmp[j][0] + s * tmp[j + 1][0], (1 - s) * tmp[j][1] + s * tmp[j + 1][1]) for j in range(len(tmp) - 1)]
                cur.append(tmp[0])
            last_ctrl = ctrl[-2]
            pos = ctrl[-1]
        elif c == "A":
            vals = nums(7)
            pos = (pos[0] + vals[5], pos[1] + vals[6]) if rel else (vals[5], vals[6])
            cur.append(pos)  # arcs flattened to their chord
        else:
            i += 1
    if cur:
        out.append((cur, False))
    return out


# ------------------------------------------------------------------ images


def image_size(path: str) -> tuple[int, int]:
    from PIL import Image

    with Image.open(path) as im:
        return im.size


def import_image(doc: Document, path: str, plane: Plane = Plane.xy(), width_mm: Optional[float] = None, opacity: float = 0.6, parent: Optional[str] = None) -> str:
    w, h = image_size(path)
    width_mm = width_mm or 100.0
    node = doc.add_image(path, plane, width_mm, width_mm * h / w, opacity, parent=parent)
    return node.id


def calibrate_image(doc: Document, node_id: str, pixel_a: tuple[float, float], pixel_b: tuple[float, float], real_distance_mm: float):
    """Scale the image so two clicked points are `real_distance_mm` apart."""
    n = doc.nodes[node_id]
    w, h = image_size(n.image["path"]) if os.path.exists(n.image["path"]) else (1000, 1000)
    px = math.hypot(pixel_a[0] - pixel_b[0], pixel_a[1] - pixel_b[1])
    if px <= 0:
        return
    n.image["width"] = real_distance_mm * w / px
    n.image["height"] = n.image["width"] * h / w
    doc.touch(node_id)
