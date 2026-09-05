"""Document, commands, undo/redo, instances, persistence, clipboard."""

import math
import os

import pytest

from robocad.commands import Ops
from robocad.document import Document, Transform
from robocad.kernel import BooleanOp, ChamferSpec, Plane, Sketch, SurfaceKind
from robocad.printing import FastenerSpec


@pytest.fixture
def doc():
    return Document()


def vol(doc, nid):
    return doc.kernel.mass_properties(doc.resolved_body(nid)).volume


def test_create_edit_undo_redo(doc):
    ops = Ops(doc)
    b = ops.box((0, 0, 0), (10, 10, 10))
    assert vol(doc, b) == pytest.approx(1000)
    top = next(f for f in doc.kernel.faces(doc.nodes[b].body) if f.normal[2] > 0.9)
    ops.push_pull(b, top, 5)
    assert vol(doc, b) == pytest.approx(1500)
    assert ops.undo() == "Push/Pull"
    assert vol(doc, b) == pytest.approx(1000)
    assert ops.redo() == "Push/Pull"
    assert vol(doc, b) == pytest.approx(1500)
    ops.undo()
    ops.undo()  # the box itself
    assert b not in doc.nodes
    ops.redo()
    assert b in doc.nodes


def test_boolean_removes_tools_and_undo_restores(doc):
    ops = Ops(doc)
    a = ops.box((0, 0, 0), (10, 10, 10))
    c = ops.cylinder((5, 5, -1), (0, 0, 1), 2, 12)
    ops.boolean(a, [c], BooleanOp.SUBTRACT)
    assert c not in doc.nodes
    assert vol(doc, a) == pytest.approx(1000 - math.pi * 4 * 10, rel=1e-6)
    ops.undo()
    assert c in doc.nodes and vol(doc, a) == pytest.approx(1000)


def test_live_dimensions(doc):
    ops = Ops(doc)
    b = ops.box((0, 0, 0), (10, 20, 30))
    faces = doc.kernel.faces(doc.nodes[b].body)
    lo = next(f for f in faces if f.normal[0] < -0.9)
    hi = next(f for f in faces if f.normal[0] > 0.9)
    ops.set_distance(b, lo, hi, 14)
    assert doc.kernel.mass_properties(doc.nodes[b].body).size[0] == pytest.approx(14)
    c = ops.cylinder((0, 0, 0), (0, 0, 1), 4, 10)
    cf = next(f for f in doc.kernel.faces(doc.nodes[c].body) if f.kind == SurfaceKind.CYLINDER)
    ops.set_diameter(c, cf, 8.4)
    assert vol(doc, c) == pytest.approx(math.pi * 4.2**2 * 10, rel=1e-6)


def test_instances_follow_source_and_mirror(doc):
    ops = Ops(doc)
    b = ops.box((0, 0, 0), (10, 10, 10))
    inst = ops.instance(b, Transform((50, 0, 0)))
    assert doc.kernel.mass_properties(doc.resolved_body(inst)).centroid[0] == pytest.approx(55)
    top = next(f for f in doc.kernel.faces(doc.nodes[b].body) if f.normal[2] > 0.9)
    ops.push_pull(b, top, 10)
    assert vol(doc, inst) == pytest.approx(2000)  # live
    m = ops.mirror([b], Plane.yz(0), live=True)[0]
    assert doc.kernel.mass_properties(doc.resolved_body(m)).centroid[0] == pytest.approx(-5)
    baked = ops.make_unique(m)
    assert doc.nodes[baked].kind == "body" and m not in doc.nodes


def test_arrays(doc):
    ops = Ops(doc)
    b = ops.box((0, 0, 0), (5, 5, 5))
    made = ops.array_rect([b], (3, 2, 1), spacing=(10, 10, 0))
    assert len(made) == 5
    r = ops.array_radial([b], 6, (0, 0, 0), (0, 0, 1))
    assert len(r) == 5
    merged = ops.array_rect([b], (2, 1, 1), extent=(4, 0, 0), merge=True)  # overlapping copies fuse
    assert merged == [b]
    assert vol(doc, b) == pytest.approx(5 * 5 * 9)


def test_outliner_ops_undo(doc):
    ops = Ops(doc)
    a = ops.box((0, 0, 0), (1, 1, 1))
    b = ops.box((5, 0, 0), (1, 1, 1))
    g = ops.group([a, b], "Legs")
    assert doc.nodes[a].parent == g
    ops.rename(a, "Left leg")
    ops.set_material([a], "steel")
    ops.set_visible([b], False)
    assert not doc.is_visible(b)
    ops.isolate([a])
    assert doc.is_visible(a) and not doc.is_visible(b)
    ops.undo()
    ops.undo()
    assert doc.is_visible(b)
    ops.undo()
    assert doc.nodes[a].material == "pla"
    ops.undo()
    assert doc.nodes[a].name == "Box"
    ops.undo()
    assert doc.nodes[a].parent is None and g not in doc.nodes


def test_nested_organization_preserves_connections_and_rejects_cycles(doc, tmp_path):
    from robocad.kernel import KernelError
    ops = Ops(doc)
    a = ops.box((0, 0, 0), (1, 1, 1))
    b = ops.box((5, 0, 0), (1, 1, 1))
    joint = ops.connect_fixed(a, b)
    inner = ops.group([b], 'Drive')
    outer = ops.group([b, inner], 'Robot')
    assert doc.nodes[b].parent == inner and doc.nodes[inner].parent == outer
    before = len(ops.stack.undo_stack)
    with pytest.raises(KernelError, match='descendants'):
        ops.move_nodes([a, outer], inner)
    assert len(ops.stack.undo_stack) == before and doc.nodes[a].parent is None
    ops.move_nodes([inner, b], None)
    assert doc.nodes[inner].parent is None and doc.nodes[b].parent == inner
    ops.undo()
    assert doc.nodes[inner].parent == outer
    ops.isolate([outer])
    assert doc.is_visible(b) and not doc.is_visible(a)
    ops.undo()
    path = str(tmp_path/'organized.rcad')
    doc.save(path)
    loaded = Document.load(path)
    assert loaded.nodes[b].parent == inner and loaded.nodes[inner].parent == outer
    assert loaded.nodes[joint].joint.parent == a and loaded.nodes[joint].joint.child == b
    assert vol(loaded, b) == pytest.approx(1)


def test_sketch_extrude_cut(doc):
    ops = Ops(doc)
    s = ops.new_sketch(Plane.xy())
    ops.edit_sketch(s, lambda sk: (sk.rectangle((0, 0), (20, 10)), sk.circle((10, 5), 2)))
    body = ops.extrude(s, 5)
    assert vol(doc, body) == pytest.approx(20 * 10 * 5 - math.pi * 4 * 5, rel=1e-6)
    parts = ops.cut(body, Plane.yz(10))
    assert len(parts) == 2
    ops.undo()
    assert len(parts) == 2 and parts[1] not in doc.nodes


def test_fastener_and_clearance(doc):
    ops = Ops(doc)
    b = ops.box((0, 0, 0), (30, 30, 10))
    top = next(f for f in doc.kernel.faces(doc.nodes[b].body) if f.normal[2] > 0.9)
    ops.fastener_hole(b, top, (15, 15, 10), FastenerSpec("M3", "insert"))
    assert vol(doc, b) < 9000
    hole = next(f for f in doc.kernel.faces(doc.nodes[b].body) if f.kind == SurfaceKind.CYLINDER and f.radius and abs(f.radius - 2.0) < 1e-6)
    ops.clearance(b, [hole], 0.2)
    assert any(abs(f.radius - 2.2) < 1e-6 for f in doc.kernel.faces(doc.nodes[b].body) if f.kind == SurfaceKind.CYLINDER)
    assert ops.last_clearance == 0.2


def test_save_load_roundtrip(doc, tmp_path):
    ops = Ops(doc)
    b = ops.box((0, 0, 0), (10, 10, 10))
    s = ops.new_sketch(Plane.xz())
    ops.edit_sketch(s, lambda sk: sk.circle((0, 0), 3))
    ops.plane_three_points((0, 0, 0), (1, 0, 0), (0, 1, 1))
    ops.instance(b, Transform((20, 0, 0)))
    ops.group([b], "Body group")
    path = str(tmp_path / "t.rcad")
    doc.save(path, thumbnail=b"\x89PNG")
    back = Document.load(path)
    assert len(back.nodes) == len(doc.nodes)
    assert vol(back, b) == pytest.approx(1000)
    assert back.nodes[s].sketch.curves[0].kind == "circle"
    assert Document.read_thumbnail(path) == b"\x89PNG"
    inst = next(n for n in back.nodes.values() if n.kind == "instance")
    assert back.kernel.mass_properties(back.resolved_body(inst.id)).centroid[0] == pytest.approx(25)


def test_clipboard_with_placement(doc):
    ops = Ops(doc)
    b = ops.box((100, 0, 0), (10, 10, 10))
    clip = doc.copy_nodes([b])
    other = Document()
    pasted = other.paste_nodes(clip, keep_placement=True)
    assert other.kernel.mass_properties(pasted[0].body).centroid[0] == pytest.approx(105)


def test_autosave(doc, tmp_path):
    ops = Ops(doc)
    ops.box((0, 0, 0), (1, 1, 1))
    doc.path = str(tmp_path / "work.rcad")
    doc.save_autosave()
    assert os.path.exists(str(tmp_path / "work.autosave.rcad"))
