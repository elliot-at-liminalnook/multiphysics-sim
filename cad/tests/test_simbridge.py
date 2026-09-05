"""The CAD → simulation export: masses, inertias, outlines, joints, ground."""

import json

from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import Plane
from robocad.simbridge import export_sim_model, joints_of


def test_export_model(tmp_path):
    doc = Document()
    ops = Ops(doc)
    hip = ops.box((-25, -20, 200), (50, 40, 40), name="ground")
    thigh = ops.box((-8, -6, 90), (16, 12, 120), name="thigh")
    ops.set_material([thigh], "petg")
    ops.plane_three_points((0, 0, 200), (1, 0, 200), (0, 0, 201), name="joint:thigh:ground")
    p = str(tmp_path / "m.simrobot.json")
    model = export_sim_model(doc, p, plane=Plane.xz(), version=2)
    with open(p) as f:
        back = json.load(f)
    assert back["format"] == "simrobot" and len(back["bodies"]) == 2 and len(back["joints"]) == 1
    g = next(b for b in back["bodies"] if b["name"] == "ground")
    t = next(b for b in back["bodies"] if b["name"] == "thigh")
    assert g["ground"] and not t["ground"]
    # 16×12×120 mm PETG bar: 23.04 cm³ × 1.27 → 29.3 g; rod inertia m(L²+w²)/12 about the plane normal (y).
    assert abs(t["mass_kg"] * 1000 - 29.26) < 0.1
    m, L, w = t["mass_kg"], 0.120, 0.016
    assert abs(t["inertia_zz"] - m * (L * L + w * w) / 12) / (m * (L * L + w * w) / 12) < 0.02
    assert t["outline"] and all(len(loop) >= 2 for loop in t["outline"])
    j = back["joints"][0]
    assert j["child"] == "thigh" and j["parent"] == "ground"
    assert j["pivot2"] == [0.0, 200.0]


def test_joint_parent_inferred():
    doc = Document()
    ops = Ops(doc)
    ops.box((0, 0, 0), (10, 10, 10), name="base")
    ops.box((0, 0, 10), (10, 10, 30), name="arm")
    ops.plane_three_points((5, 5, 10), (6, 5, 10), (5, 5, 11), name="joint:arm")
    js = joints_of(doc)
    assert js[0]["parent"] == "base"
