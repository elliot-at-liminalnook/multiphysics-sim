"""Robotic parts: motors from the library, joints, inference, validation,
and the simulation export with merged links, limits and motors."""

import json
import math

import pytest

from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import BooleanOp, Plane, SurfaceKind
from robocad.robotics import MOTOR_LIBRARY, gravity_torque, infer_joints, motor_body, validate_robot
from robocad.simbridge import export_sim_model


def test_closed_loop_mobility_is_not_a_negative_dof_count():
    doc = Document(); ops = Ops(doc)
    a = ops.box((0, 0, 0), (5, 5, 5))
    b = ops.box((0, 0, 5), (5, 5, 5))
    ops.add_joint('loop_revolute', a, b, (2, 2, 5))
    summary = ops.robot()
    assert summary['has_closed_loops'] and summary['dof'] is None


def test_motor_library_geometry():
    doc = Document()
    k = doc.kernel
    for mid in ("nema17", "sg90", "n20_100", "gb37_100", "gm2804"):
        spec = MOTOR_LIBRARY[mid]
        body, meta = motor_body(k, spec, (0, 0, 0), (0, 0, 1))
        p = k.mass_properties(body)
        assert p.volume > 0 and k.validate(body).watertight, mid
        assert meta["shaft_axis"] == [0.0, 0.0, 1.0] and meta["shaft_tip"][2] > 0
        # The shaft is a cylinder of the spec's diameter.
        assert any(f.kind == SurfaceKind.CYLINDER and f.radius and abs(2 * f.radius - spec.shaft_diameter) < 1e-6 for f in k.faces(body))
    nema = MOTOR_LIBRARY["nema17"]
    body, _ = motor_body(k, nema, (0, 0, 0), (0, 0, 1))
    size = k.mass_properties(body).size
    assert abs(size[0] - 42.3) < 1e-6 and abs(size[2] - (40.0 + 22.0 + 2.0)) < 1e-6


def test_ops_motor_joint_and_export(tmp_path):
    doc = Document()
    ops = Ops(doc)
    base = ops.box((-30, -30, 0), (60, 60, 10), name="ground")
    arm = ops.box((-6, -6, 30), (12, 12, 80), name="arm")
    # A NEMA 17 on the base's top face, shaft up through the arm's pivot; the bracket gets its holes cut.
    m = ops.add_motor("nema17", (0, 0, 10), (0, 0, 1), mount_on=base, cut_mount=True)
    assert doc.nodes[m].robot["kind"] == "motor"
    assert any(f.kind == SurfaceKind.CYLINDER for f in doc.kernel.faces(doc.nodes[base].body))  # mounting holes cut
    j = ops.add_joint("revolute", base, arm, (0, 0, 30), (0, 0, 1), lower=-math.pi / 2, upper=math.pi / 2)
    ops.attach_motor(j, m)
    assert doc.nodes[j].joint.motor == m and doc.nodes[m].robot["drives"] == j
    summary = ops.robot()
    assert summary["dof"] == 1 and summary["joints"][0]["motor_name"] == doc.nodes[m].name
    assert not [i for i in summary["issues"] if i["severity"] == "error"]
    # Undo/redo through the joint edits.
    ops.set_joint(j, upper=1.0)
    assert doc.nodes[j].joint.upper == 1.0
    ops.undo()
    assert doc.nodes[j].joint.upper == pytest.approx(math.pi / 2)
    # Export: the motor merges into ground (mounted on it); the joint carries limits and motor numbers.
    path = str(tmp_path / "m.simrobot.json")
    model = export_sim_model(doc, path, plane=Plane.xz(), version=2)
    names = {b["name"]: b for b in model["bodies"]}
    assert set(names) == {"ground", "arm"}
    assert doc.nodes[m].name in names["ground"]["members"] and names["ground"]["ground"]
    assert names["ground"]["mass_kg"] > 0.28  # base + 280 g motor
    jj = model["joints"][0]
    assert jj["type"] == "revolute" and jj["limits"] == pytest.approx([-math.pi / 2, math.pi / 2])
    assert jj["motor"]["stall_torque"] == pytest.approx(0.40) and jj["motor"]["spec"] == "nema17"
    # Save/load keeps joints and motor metadata.
    doc.save(str(tmp_path / "r.rcad"))
    back = Document.load(str(tmp_path / "r.rcad"))
    assert back.nodes[j].joint.type == "revolute" and back.nodes[m].robot["spec"] == "nema17"


def test_infer_joints_from_pin_in_hole():
    doc = Document()
    ops = Ops(doc)
    bracket = ops.box((0, 0, 0), (40, 20, 10), name="bracket")
    hole = ops.cylinder((20, 10, -1), (0, 0, 1), 3.0, 12, name="h")
    ops.boolean(bracket, [hole], BooleanOp.SUBTRACT)
    lever = ops.box((15, 8, 10), (60, 4, 4), name="lever")
    pin = ops.cylinder((20, 10, 0), (0, 0, 1), 3.0, 14, name="pin")
    ops.boolean(lever, [pin], BooleanOp.UNION)
    found = infer_joints(doc)
    assert len(found) == 1
    j = found[0]
    assert j.type == "revolute" and j.parent == bracket and j.child == lever
    assert abs(j.pivot[0] - 20) < 1e-6 and abs(j.pivot[1] - 10) < 1e-6
    ids = ops.infer_joints()
    assert len(ids) == 1 and doc.nodes[ids[0]].kind == "joint"
    assert ops.infer_joints() == []  # not twice


def test_validation_catches_loops_and_double_parents():
    doc = Document()
    ops = Ops(doc)
    a = ops.box((0, 0, 0), (10, 10, 10), name="a")
    b = ops.box((10, 0, 0), (10, 10, 10), name="b")
    c = ops.box((20, 0, 0), (10, 10, 10), name="c")
    ops.add_joint("revolute", a, b, (10, 5, 5), (0, 0, 1))
    ops.add_joint("revolute", b, c, (20, 5, 5), (0, 0, 1))
    ops.add_joint("revolute", c, a, (0, 5, 5), (0, 0, 1))  # loop
    issues = validate_robot(doc)
    assert any("loop" in i.message for i in issues)
    ops.add_joint("fixed", a, c, (25, 5, 5))  # c now has two parents
    assert any("already has a parent" in i.message for i in validate_robot(doc))


def test_gravity_torque_and_motor_warning():
    doc = Document()
    ops = Ops(doc)
    base = ops.box((-20, -20, 0), (40, 40, 10), name="ground")
    arm = ops.box((0, -5, 10), (200, 10, 10), name="arm")  # a 200 mm PLA arm, 24.8 g
    ops.set_material([arm], "pla")
    j = ops.add_joint("revolute", base, arm, (0, 0, 15), (0, 1, 0))
    load = gravity_torque(doc, doc.nodes[j].joint)
    assert load == pytest.approx(0.0248 * 9.81 * 0.1, rel=0.05)
    m = ops.add_motor("sg90", (0, -6, 15), (0, -1, 0), mount_on=base)
    ops.attach_motor(j, m)
    assert not any("stalls" in i.message for i in validate_robot(doc))
    heavy = ops.box((0, -5, 10), (900, 10, 10), name="long arm")
    ops.set_material([heavy], "steel")
    j2 = ops.add_joint("revolute", base, heavy, (0, 0, 15), (0, 1, 0))
    ops.attach_motor(j2, m)
    assert any("stalls" in i.message for i in validate_robot(doc))
