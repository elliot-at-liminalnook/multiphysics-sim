"""Kernel operation tests on a suite of mechanical fixtures."""

import math

import pytest

from robocad.kernel import BooleanOp, ChamferSpec, KernelError, Plane, Sketch, SurfaceKind, SweepOptions, default_kernel


@pytest.fixture(scope="module")
def k():
    return default_kernel()


def volume(k, b):
    return k.mass_properties(b).volume


def test_primitives_and_mass(k):
    box = k.box((0, 0, 0), (10, 20, 30))
    assert volume(k, box) == pytest.approx(6000)
    cyl = k.cylinder((0, 0, 0), (0, 0, 1), 5, 10)
    assert volume(k, cyl) == pytest.approx(math.pi * 25 * 10, rel=1e-6)
    sph = k.sphere((1, 2, 3), 4)
    assert volume(k, sph) == pytest.approx(4 / 3 * math.pi * 64, rel=1e-6)
    p = k.mass_properties(box)
    assert p.centroid == pytest.approx((5, 10, 15))
    assert p.mass(1.0) == pytest.approx(6.0)  # 6 cm³ of water = 6 g


def test_booleans(k):
    a = k.box((0, 0, 0), (10, 10, 10))
    b = k.box((5, 5, 5), (10, 10, 10))
    assert volume(k, k.boolean(a, b, BooleanOp.UNION)) == pytest.approx(2000 - 125)
    assert volume(k, k.boolean(a, b, BooleanOp.SUBTRACT)) == pytest.approx(1000 - 125)
    assert volume(k, k.boolean(a, b, BooleanOp.INTERSECT)) == pytest.approx(125)
    with pytest.raises(KernelError):
        k.boolean(a, k.box((50, 50, 50), (1, 1, 1)), BooleanOp.INTERSECT)


def test_push_pull_and_face_matching(k):
    box = k.box((0, 0, 0), (10, 10, 10))
    top = next(f for f in k.faces(box) if f.normal[2] > 0.9)
    taller = k.push_pull(box, top, 5)
    assert volume(k, taller) == pytest.approx(1500)
    shorter = k.push_pull(box, top, -3)
    assert volume(k, shorter) == pytest.approx(700)
    # The face reference still finds the top after the edit.
    found = k.find_face(taller, top)
    assert found.centroid[2] == pytest.approx(15)


def test_cylinder_radius_edit_hole_and_boss(k):
    plate = k.box((0, 0, 0), (40, 40, 5))
    hole = k.cylinder((20, 20, -1), (0, 0, 1), 4, 7)
    plate = k.boolean(plate, hole, BooleanOp.SUBTRACT)
    cyl_face = next(f for f in k.faces(plate) if f.kind == SurfaceKind.CYLINDER)
    assert cyl_face.radius == pytest.approx(4)
    bigger = k.set_cylinder_radius(plate, cyl_face, 4.2)  # +0.2 mm clearance
    assert volume(k, bigger) == pytest.approx(40 * 40 * 5 - math.pi * 4.2**2 * 5, rel=1e-6)
    smaller = k.set_cylinder_radius(plate, cyl_face, 3.5)
    assert volume(k, smaller) == pytest.approx(40 * 40 * 5 - math.pi * 3.5**2 * 5, rel=1e-6)
    boss = k.boolean(k.box((0, 0, 0), (40, 40, 5)), k.cylinder((20, 20, 5), (0, 0, 1), 4, 10), BooleanOp.UNION)
    bf = next(f for f in k.faces(boss) if f.kind == SurfaceKind.CYLINDER)
    grown = k.set_cylinder_radius(boss, bf, 6)
    assert volume(k, grown) == pytest.approx(8000 + math.pi * 36 * 10, rel=1e-6)


def test_fillet_chamfer_and_failure_report(k):
    box = k.box((0, 0, 0), (20, 20, 20))
    edges = k.edges(box)
    rounded = k.fillet(box, edges[:1], 2.0)
    assert volume(k, rounded) < 8000
    assert k.validate(rounded).watertight
    chamfered = k.chamfer(box, edges[:1], ChamferSpec(2.0))
    assert volume(k, chamfered) == pytest.approx(8000 - 0.5 * 2 * 2 * 20)
    allround = k.fillet_all(box, 1.0)
    assert k.validate(allround).watertight
    with pytest.raises(KernelError) as e:
        k.fillet(box, edges[:1], 25.0)
    assert "too large" in str(e.value) or "failed" in str(e.value)


def test_boss_meeting_wall_fillets(k):
    plate = k.box((0, 0, 0), (40, 40, 3))
    boss = k.cylinder((20, 20, 3), (0, 0, 1), 4, 8)
    body = k.boolean(plate, boss, BooleanOp.UNION)
    root = [e for e in k.edges(body) if e.kind.value == "circle" and abs(e.midpoint[2] - 3) < 1e-6 and e.radius and abs(e.radius - 4) < 1e-6]
    assert root
    filleted = k.fillet(body, root, 0.5)
    assert k.validate(filleted).watertight
    assert volume(k, filleted) > volume(k, body)


def test_shell_and_thicken(k):
    box = k.box((0, 0, 0), (30, 30, 30))
    top = next(f for f in k.faces(box) if f.normal[2] > 0.9)
    hollow = k.shell(box, 2.0, [top])
    assert volume(k, hollow) == pytest.approx(30**3 - 26 * 26 * 28)
    assert k.validate(hollow).watertight
    sk = Sketch(Plane.xy())
    sk.rectangle((0, 0), (10, 10))
    sheet = k.face_from_wire(sk.to_body())
    solid = k.thicken(sheet, 2.0)
    assert volume(k, solid) == pytest.approx(200)


def test_extrude_revolve_sweep_loft(k):
    sk = Sketch(Plane.xy())
    sk.circle((0, 0), 5)
    disc = k.extrude(sk.to_body(), (0, 0, 1), 10)
    assert volume(k, disc) == pytest.approx(math.pi * 25 * 10, rel=1e-6)
    tapered = k.extrude(sk.to_body(), (0, 0, 1), 10, taper_deg=10)
    assert volume(k, tapered) < volume(k, disc)
    sym = k.extrude(sk.to_body(), (0, 0, 1), 10, symmetric=True)
    assert k.mass_properties(sym).centroid[2] == pytest.approx(0.0, abs=1e-6)
    sk2 = Sketch(Plane.xy())
    sk2.rectangle((10, 0), (5, 5))
    ring = k.revolve(sk2.to_body(), (0, 0, 0), (0, 1, 0), 360)
    assert volume(k, ring) == pytest.approx(2 * math.pi * 12.5 * 25, rel=1e-3)
    path = Sketch(Plane.xz())
    path.line((0, 0), (0, 30))
    tube = k.pipe(path.to_body(), 6)
    assert volume(k, tube) == pytest.approx(math.pi * 9 * 30, rel=1e-3)
    p1 = Sketch(Plane.xy(0))
    p1.circle((0, 0), 5)
    p2 = Sketch(Plane.xy(20))
    p2.rectangle_center((0, 0), (8, 8))
    loft = k.loft([p1.to_body(), p2.to_body()])
    assert k.validate(loft).watertight
    prof = Sketch(Plane.xy())
    prof.circle((0, 20), 2)
    sweep_path = Sketch(Plane.yz())
    sweep_path.arc((0, 0), 20, 0, 90)
    swept = k.sweep(prof.to_body(), sweep_path.to_body(), SweepOptions())
    assert volume(k, swept) == pytest.approx(math.pi * 4 * (math.pi * 20 / 2), rel=0.05)


def test_slot_cut_and_arrays_by_transform(k):
    body = k.box((0, 0, 0), (60, 30, 3))
    sk = Sketch(Plane.xy(-1))
    sk.slot((4, 15), (14, 15), 3)
    cutter = k.extrude(sk.to_body(), (0, 0, 1), 5)
    for i in range(4):
        tool = k.transform(cutter, translation=(i * 14, 0, 0))
        body = k.boolean(body, tool, BooleanOp.SUBTRACT)
    slot_area = 10 * 3 + math.pi * 1.5**2
    assert volume(k, body) == pytest.approx(60 * 30 * 3 - 4 * slot_area * 3, rel=1e-4)


def test_mirror_delete_face_split_section(k):
    box = k.box((0, 0, 0), (10, 10, 10))
    m = k.mirror(box, Plane.yz(0))
    assert k.mass_properties(m).centroid[0] == pytest.approx(-5)
    edges = k.edges(box)
    ch = k.chamfer(box, edges[:1], ChamferSpec(2.0))
    chamfer_face = next(f for f in k.faces(ch) if abs(abs(f.normal[0]) - abs(f.normal[1])) < 1e-6 and abs(f.normal[2]) < 1e-6 or abs(abs(f.normal[1]) - abs(f.normal[2])) < 1e-6 and abs(f.normal[0]) < 1e-6 and f.area < 100)
    healed = k.delete_faces(ch, [chamfer_face])
    assert volume(k, healed) == pytest.approx(1000)
    parts = k.cut_with_plane(box, Plane.xy(4))
    assert len(parts) == 2
    assert sorted(volume(k, p) for p in parts) == pytest.approx([400, 600])
    loops = k.section(box, Plane.xy(5))
    assert loops


def test_dependent_offset_for_mating_parts(k):
    base = k.box((0, 0, 0), (20, 20, 5))
    lid = k.box((0, 0, 20), (20, 20, 5))
    bottom = next(f for f in k.faces(lid) if f.normal[2] < -0.9)
    mated = k.offset_face_to_body(lid, bottom, base, clearance=0.2)
    lo = k.mass_properties(mated).bbox_min[2]
    assert lo == pytest.approx(5.2)


def test_validation_and_persistence(k):
    box = k.box((0, 0, 0), (10, 10, 10))
    rep = k.validate(box)
    assert rep.valid and rep.watertight
    sk = Sketch(Plane.xy())
    sk.rectangle((0, 0), (10, 10))
    sheet = k.face_from_wire(sk.to_body())
    rep2 = k.validate(sheet)
    assert rep2.valid
    data = k.serialize(box)
    back = k.deserialize(data)
    assert volume(k, back) == pytest.approx(1000)


def test_tessellation_has_face_ids(k):
    box = k.box((0, 0, 0), (10, 10, 10))
    mesh = k.tessellate(box)
    assert mesh.face_count == 6
    assert len(mesh.triangles) == 12
    assert sorted(set(mesh.triangle_face)) == list(range(6))


def test_sketch_editing():
    sk = Sketch(Plane.xy())
    a = sk.line((0, 0), (10, 0))
    b = sk.line((5, -5), (5, 5))
    sk.trim(a, [b], click=(8, 0))
    assert len(sk.curves) == 2
    kept = [c for c in sk.curves if c.kind == "line" and c.points[0][1] == 0][0]
    assert max(p[0] for p in kept.points) == pytest.approx(5.0)
    r = sk.rectangle((0, 0), (10, 10))
    sk.fillet_corner(r, 1, 2.0)
    assert len(r.points) > 4
    off = sk.offset(r, 1.0)
    assert off.closed
    c = sk.circle_three_point((0, 0), (10, 0), (5, 5))
    assert c.radius == pytest.approx(5)
    txt = sk.text((0, 0), "A", 10)
    assert txt and all(len(t.points) > 2 for t in txt)
