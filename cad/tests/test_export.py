"""Export validation: STL/3MF manifold checks, STEP round-trip, OBJ, SVG."""

import math
import os
import struct
import zipfile

import pytest

from robocad.commands import Ops
from robocad.document import Document
from robocad.io.drawing import STANDARD_VIEWS, View, export_drawing_svg
from robocad.io.exporters import ExportError, StlSettings, export_3mf, export_obj, export_sketch_svg, export_step, export_stl
from robocad.io.importers import import_mesh, import_step, import_svg
from robocad.kernel import BooleanOp, Plane
from robocad.printing import mesh_open_edges, wall_thickness


@pytest.fixture
def doc():
    d = Document()
    ops = Ops(d)
    b = ops.box((0, 0, 0), (20, 20, 10))
    c = ops.cylinder((10, 10, -1), (0, 0, 1), 3, 12)
    ops.boolean(b, [c], BooleanOp.SUBTRACT)
    ops.cylinder((40, 0, 0), (0, 0, 1), 5, 8, name="Boss")
    return d


def test_stl_binary_and_ascii_manifold(doc, tmp_path):
    p = str(tmp_path / "a.stl")
    export_stl(doc, p, settings=StlSettings(binary=True, unit="mm"))
    with open(p, "rb") as f:
        f.read(80)
        n = struct.unpack("<I", f.read(4))[0]
    assert n > 12
    export_stl(doc, str(tmp_path / "b.stl"), settings=StlSettings(binary=False, unit="in"))
    text = open(str(tmp_path / "b.stl")).read()
    assert text.startswith("solid") and "endsolid" in text
    # Unit override: a 20 mm box in inches spans ~0.787
    xs = [float(line.split()[1]) for line in text.splitlines() if line.strip().startswith("vertex")]
    assert max(xs) == pytest.approx(45 / 25.4, rel=1e-3)


def test_export_blocked_by_open_sheet(tmp_path):
    d = Document()
    from robocad.kernel import Sketch

    sk = Sketch(Plane.xy())
    sk.rectangle((0, 0), (10, 10))
    sheet = d.kernel.face_from_wire(sk.to_body())
    n = d.add_body(sheet, "Open sheet")
    n.kind = "body"  # pretend it's a solid: validation must catch it
    n.body.kind = "solid"
    with pytest.raises(ExportError) as e:
        export_stl(d, str(tmp_path / "bad.stl"))
    assert "open" in str(e.value).lower() or "solid" in str(e.value).lower()


def test_3mf_structure_and_manifold(doc, tmp_path):
    p = str(tmp_path / "a.3mf")
    export_3mf(doc, p)
    with zipfile.ZipFile(p) as z:
        names = z.namelist()
        assert "3D/3dmodel.model" in names and "[Content_Types].xml" in names
        model = z.read("3D/3dmodel.model").decode()
    assert model.count("<object ") == 2
    assert 'name="Boss"' in model and "displaycolor" in model
    for n in doc.bodies():
        m = doc.mesh_of(n.id)
        assert mesh_open_edges(m) == 0


def test_step_roundtrip(doc, tmp_path):
    p = str(tmp_path / "a.step")
    export_step(doc, p)
    back = Document()
    ids = import_step(back, p)
    assert len(ids) >= 2
    vols = sorted(back.kernel.mass_properties(back.nodes[i].body).volume for i in ids if back.nodes[i].body is not None)
    assert vols[-1] == pytest.approx(20 * 20 * 10 - math.pi * 9 * 10, rel=1e-4)
    names = {back.nodes[i].name for i in ids}
    assert "Boss" in names


def test_obj_and_mesh_import(doc, tmp_path):
    p = str(tmp_path / "a.obj")
    export_obj(doc, p)
    assert os.path.exists(str(tmp_path / "a.mtl"))
    text = open(p).read()
    assert "o Boss" in text and "usemtl" in text
    d2 = Document()
    mid = import_mesh(d2, p, unit="mm")
    m = d2.mesh_of(mid)
    lo, hi = m.bounds()
    assert hi[0] - lo[0] == pytest.approx(45, abs=0.01)


def test_svg_sketch_roundtrip(tmp_path):
    d = Document()
    ops = Ops(d)
    s = ops.new_sketch(Plane.xy())
    ops.edit_sketch(s, lambda sk: (sk.rectangle((0, 0), (30, 10)), sk.circle((5, 5), 2)))
    p = str(tmp_path / "s.svg")
    export_sketch_svg(d, p, s)
    d2 = Document()
    s2 = import_svg(d2, p)
    assert len(d2.nodes[s2].sketch.curves) == 2


def test_drawing_svg_hidden_lines_and_section(doc, tmp_path):
    p = str(tmp_path / "drawing.svg")
    text = export_drawing_svg(doc, p, [STANDARD_VIEWS["front"], STANDARD_VIEWS["top"], View("Section A-A", (0.0, 1.0, 0.0), section=Plane.xz(10))], title="Test part")
    assert "stroke-dasharray" in text  # hidden lines (the hole)
    assert "url(#hatch)" in text  # section hatching
    assert text.count("<g id=") == 3


def test_wall_thickness(doc):
    ops = Ops(doc)
    thin = ops.box((100, 0, 0), (20, 20, 0.8))
    regions = wall_thickness(doc.kernel, doc.nodes[thin].body, 1.2)
    assert regions and all(r.thickness < 1.2 for r in regions)
    thick = ops.box((200, 0, 0), (20, 20, 5))
    assert not wall_thickness(doc.kernel, doc.nodes[thick].body, 1.2)
