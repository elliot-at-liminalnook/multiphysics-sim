"""The physical assembly description (simrobot v3): schema completeness,
inertia, signed distance grids, joint inference, fastened flanges, the
flexible-link reduction against beam theory, results/identification round
trips and the API routes."""

import json
import math
import os

import numpy as np
import pytest

from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import Plane
from robocad.physical import apply_identification, collision_block, export_physical_model, load_results
from robocad.printing import FastenerSpec
from robocad.robotics import MOTOR_LIBRARY, motor_physics


def _leg():
    """Ground block, thigh and shank with two servos, sensors, a cable and a battery."""
    doc = Document()
    ops = Ops(doc)
    hip = ops.box((-25.0, -20.0, 200.0), (50.0, 40.0, 40.0), name="ground")
    ops.set_material([hip], "al")
    thigh = ops.box((-8.0, -6.0, 90.0), (16.0, 12.0, 120.0), name="thigh")
    shank = ops.box((-6.0, -5.0, -15.0), (12.0, 10.0, 110.0), name="shank")
    ops.set_material([thigh, shank], "petg")
    ops.set_ground(hip)
    hm = ops.add_motor("mg996r", (0.0, -20.0, 200.0), (0.0, 1.0, 0.0), mount_on=hip, cut_mount=True, name="hip motor")
    km = ops.add_motor("mg90s", (0.0, -6.0, 90.0), (0.0, 1.0, 0.0), mount_on=thigh, cut_mount=True, name="knee motor")
    hj = ops.add_joint("revolute", hip, thigh, (0.0, 0.0, 200.0), (0.0, 1.0, 0.0), lower=-1.5, upper=1.5, name="hip")
    kj = ops.add_joint("revolute", thigh, shank, (0.0, 0.0, 90.0), (0.0, 1.0, 0.0), lower=-2.0, upper=0.1, name="knee")
    ops.attach_motor(hj, hm)
    ops.attach_motor(kj, km)
    ops.add_sensor("imu", shank, (0.0, 0.0, 0.0), name="imu")
    ops.add_sensor("encoder", shank, (0.0, 0.0, 90.0), joint=kj, name="knee encoder")
    ops.add_cable(hip, (0.0, -20.0, 190.0), shank, (0.0, -5.0, 60.0), name="lead")
    ops.set_battery(cells=2)
    ops.set_control(targets={"hip": 0.2})
    return doc, ops


def _check_schema(model):
    for key in ("version", "source", "gravity", "world", "materials", "links", "joints", "motors", "battery", "sensors", "cables", "control", "uncertainty", "identification", "planar"):
        assert key in model, key
    assert model["version"] == 3
    for l in model["links"]:
        for key in ("name", "id", "members", "material", "ground", "mass", "com", "inertia", "bbox", "collision", "flex", "print"):
            assert key in l, key
        assert len(l["com"]) == 3 and np.array(l["inertia"]).shape == (3, 3)
        c = l["collision"]
        assert len(c["vertices"]) <= 3000 and c["sdf"] and len(c["sdf"]["values"]) == int(np.prod(c["sdf"]["dims"]))
        if l["flex"]:
            f = l["flex"]
            m, nb = f["modes"], len(f["boundary_frames"])
            assert len(f["frequencies_hz"]) == m and len(f["modal_stiffness"]) == m and len(f["modal_mass"]) == m
            assert np.array(f["boundary_shapes"]).shape == (m, nb, 6) and np.array(f["participation"]).shape == (m, 6)
            assert np.array(f["stress_per_mode"]).shape == (m, len(f["stress_cells"]), 6)
            assert set(f["softening"]) == {"tg_c", "width_c", "ratio_above"}
    for j in model["joints"]:
        for key in ("name", "type", "parent", "child", "origin", "axis", "limits", "home", "physics", "fastened", "motor"):
            assert key in j, key
        p = j["physics"]
        for key in ("source", "clearance", "backlash", "wobble", "friction", "stiffness", "damping_ratio", "bearing"):
            assert key in p, key
        assert set(p["friction"]) >= {"coulomb", "viscous", "stribeck", "stribeck_speed", "static_ratio"}
        assert set(p["stiffness"]) >= {"radial", "axial", "bending"}
    for m in model["motors"]:
        for key in ("electrical", "gearbox", "thermal", "firmware", "driver", "joint", "mounted_on", "mount_point", "shaft_axis"):
            assert key in m, key
        assert set(m["electrical"]) >= {"resistance", "inductance", "torque_constant", "back_emf_constant", "no_load_current", "rotor_inertia", "supply_voltage", "current_limit"}
        assert set(m["gearbox"]) >= {"ratio", "efficiency", "backlash_rad", "inertia", "stiffness", "max_output_torque", "max_output_speed"}
        assert set(m["thermal"]) >= {"winding_heat_capacity", "case_heat_capacity", "r_winding_case", "r_case_mount", "r_case_ambient", "resistance_temp_coeff", "torque_derating_per_c", "max_winding_c"}
        assert set(m["firmware"]) >= {"kind", "loop_rate_hz", "latency_s", "deadband_rad", "sensor_resolution_rad", "kp", "ki", "kd", "output"}
    for s in model["sensors"]:
        assert set(s) >= {"name", "kind", "link", "point", "axes", "rate_hz", "noise", "quantization"}
    for c in model["cables"]:
        assert set(c) >= {"name", "from", "to", "length", "mass", "stiffness", "damping", "segments"}
    for mat in model["materials"].values():
        assert set(mat) >= {"density", "youngs_modulus", "poisson", "yield_strength", "glass_transition_c", "thermal_conductivity", "specific_heat", "friction"}
        assert "world" in mat["friction"] and "steel" in mat["friction"]


def test_export_schema_and_units(tmp_path):
    doc, ops = _leg()
    path = str(tmp_path / "leg.simrobot.json")
    model = export_physical_model(doc, path, planar=Plane.xz(), flex=False)
    with open(path) as f:
        back = json.load(f)
    _check_schema(back)
    names = {l["name"] for l in back["links"]}
    assert names == {"ground", "thigh", "shank"}  # motors merged into their mounts
    thigh = next(l for l in back["links"] if l["name"] == "thigh")
    assert "knee motor" in thigh["member_names"]
    # SI: the shank alone is 12×10×110 mm PETG = 16.8 g; the thigh link carries the 13.4 g servo.
    shank = next(l for l in back["links"] if l["name"] == "shank")
    assert 0.014 < shank["mass"] < 0.018
    assert abs(shank["com"][2] - 0.040) < 1e-3
    hip = next(j for j in back["joints"] if j["name"] == "hip")
    assert hip["origin"] == pytest.approx([0.0, 0.0, 0.2]) and hip["parent"] == "ground" and hip["child"] == "thigh"
    assert hip["motor"] == "hip motor" and hip["limits"] == [-1.5, 1.5]
    assert back["control"]["targets"]["hip"] == 0.2 and back["control"]["targets"]["knee"] == 0.0
    assert back["battery"]["nominal_voltage"] == pytest.approx(7.4)
    assert next(s for s in back["sensors"] if s["kind"] == "encoder")["joint"] == "knee"
    assert back["cables"][0]["from"]["link"] == "ground" and back["cables"][0]["to"]["link"] == "shank"
    assert back["planar"]["normal"] == pytest.approx([0.0, -1.0, 0.0], abs=1e-9) or back["planar"]["normal"] == pytest.approx([0.0, 1.0, 0.0], abs=1e-9)


def test_inertia_matches_analytic_box(tmp_path):
    doc = Document()
    ops = Ops(doc)
    b = ops.box((30.0, -20.0, 5.0), (16.0, 12.0, 120.0), name="bar")
    ops.set_material([b], "petg")
    model = export_physical_model(doc, None, flex=False)
    l = model["links"][0]
    m = l["mass"]
    w, d, h = 0.016, 0.012, 0.120
    assert m == pytest.approx(0.016 * 0.012 * 0.120 * 1270.0, rel=1e-6)
    I = np.array(l["inertia"])
    assert I[0, 0] == pytest.approx(m * (d * d + h * h) / 12, rel=1e-6)
    assert I[1, 1] == pytest.approx(m * (w * w + h * h) / 12, rel=1e-6)
    assert I[2, 2] == pytest.approx(m * (w * w + d * d) / 12, rel=1e-6)
    assert abs(I[0, 1]) < 1e-12 and l["com"] == pytest.approx([0.038, -0.014, 0.065])


def test_signed_distance_grid():
    doc = Document()
    ops = Ops(doc)
    b = ops.box((-10.0, -10.0, -10.0), (20.0, 20.0, 20.0))
    n = doc.nodes[b]
    block, _ = collision_block(doc, [n], np.zeros(3))
    sdf = block["sdf"]
    dims, cell, origin = sdf["dims"], sdf["cell"], np.array(sdf["origin"])
    values = np.array(sdf["values"]).reshape(dims)

    def at(p):
        i = np.round((np.array(p) - origin) / cell).astype(int)
        return values[tuple(i)], origin + i * cell

    v, q = at((0.0, 0.0, 0.0))
    assert v < 0 and abs(v - (-(0.010 - np.abs(q).max()))) < cell
    v, q = at((0.0112, 0.0, 0.0))
    assert v > 0 and abs(v - (abs(q[0]) - 0.010)) < cell
    assert values.min() < -0.006 and values.max() > 0.0
    assert len(block["vertices"]) >= 8 and len(block["hull"]) == 8


def test_joint_inference_from_pin_and_hole(monkeypatch):
    doc = Document()
    ops = Ops(doc)
    bracket = ops.box((-15.0, -15.0, 0.0), (30.0, 30.0, 10.0), name="ground")
    ops.set_material([bracket], "pla")
    # A 4 mm hole in the bracket and a 3.8 mm pin on the arm through it (0.1 mm radial clearance).
    from robocad.kernel import BooleanOp

    ops.boolean(bracket, [ops.cylinder((0.0, 0.0, -1.0), (0.0, 0.0, 1.0), 2.0, 12.0)], BooleanOp.SUBTRACT)
    arm = ops.box((-4.0, -4.0, 10.0), (8.0, 8.0, 60.0), name="arm")
    ops.set_material([arm], "petg")
    ops.boolean(arm, [ops.cylinder((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), 1.9, 11.0)], BooleanOp.UNION)
    made = ops.infer_joints()
    assert len(made) == 1
    model = export_physical_model(doc, None, flex=False)
    j = model["joints"][0]
    p = j["physics"]
    assert p["source"] == "inferred"
    assert p["pin_radius"] == pytest.approx(0.0019, abs=1e-6) and p["hole_radius"] == pytest.approx(0.002, abs=1e-6)
    assert p["clearance"] == pytest.approx(0.0001, abs=1e-6)
    assert p["contact_length"] == pytest.approx(0.010, abs=2e-4)
    # Backlash = clearance / lever (arm COM 30 mm above the pivot at z=5 → 35 mm).
    assert p["backlash"] == pytest.approx(0.0001 / p["lever"], rel=1e-6) and 0.02 < p["lever"] < 0.05
    assert p["wobble"] == pytest.approx(math.atan2(2e-4, p["contact_length"]), rel=1e-6)
    # Coulomb torque = µ_k · m g · r with the PLA/PETG pair.
    mass = next(l for l in model["links"] if l["name"] == "arm")["mass"]
    assert p["friction"]["coulomb"] == pytest.approx(mass * 9.81 * 0.0019 * (p["friction"]["static_ratio"] and 1.0) * 0.375, rel=0.2)
    assert p["stiffness"]["radial"] > 1e5 and p["bearing"]["kind"] == "printed_pin"
    from robocad import physical
    def forbid_collision(*args, **kwargs):
        pytest.fail('Joint inspection must not build collision geometry')
    monkeypatch.setattr(physical, 'collision_block', forbid_collision)
    assert physical.inspect_joint_physics(doc, j['id']) == p


def test_fastened_fixed_joint():
    doc = Document()
    ops = Ops(doc)
    base = ops.box((0.0, 0.0, 0.0), (40.0, 40.0, 6.0), name="base")
    bracket = ops.box((10.0, 10.0, 6.0), (20.0, 20.0, 6.0), name="bracket")
    ops.set_material([base, bracket], "pla")
    top = next(f for f in doc.kernel.faces(doc.nodes[bracket].body) if f.normal[2] > 0.9)
    ops.fastener_hole(bracket, top, (15.0, 15.0, 12.0), FastenerSpec("M3", "clearance"))
    ops.fastener_hole(bracket, top, (25.0, 25.0, 12.0), FastenerSpec("M3", "clearance"))
    assert len(doc.nodes[bracket].robot["fasteners"]) == 2
    ops.connect_fixed(base, bracket, name="mount")
    model = export_physical_model(doc, None, flex=False)
    assert [l["name"] for l in model["links"]] == ["base"]  # merged
    j = next(j for j in model["joints"] if j["type"] == "fixed")
    f = j["fastened"]
    assert f["screw"] == "M3" and f["count"] == 2
    assert f["preload"] == pytest.approx(0.6 / (0.2 * 0.003)) and f["shear_capacity"] > 3000 and f["stiffness"] > 1e6
    assert f["pattern_radius"] == pytest.approx(math.hypot(5.0, 5.0) * 1e-3, rel=1e-3)
    from robocad.physical import inspect_joint_physics
    assert inspect_joint_physics(doc, j['id']) == j['physics']


def test_flexible_link_matches_beam_theory():
    from robocad.flex import flexible_link

    doc = Document()
    ops = Ops(doc)
    L, b = 100.0, 10.0
    bid = ops.box((0.0, -5.0, -5.0), (L, b, b))
    ops.set_material([bid], "pla")
    n = doc.nodes[bid]
    p = doc.kernel.mass_properties(n.body)
    com = np.array(p.centroid) * 1e-3
    block, _ = collision_block(doc, [n], com)
    mat = doc.materials["pla"]
    mat.engineering = {"print": {"anisotropy_z": 1.0}}
    link = {"name": "beam", "mass": p.mass(1.24) * 1e-3, "com": com.tolist(), "bbox": [list(np.array(p.bbox_min) * 1e-3 - com), list(np.array(p.bbox_max) * 1e-3 - com)], "print": {"orientation": [0, 0, 1], "infill": 1.0, "walls": 3}}
    frames = [{"name": "root", "point": (np.array([0.0, 0.0, 0.0]) * 1e-3 - com).tolist(), "role": "root", "radius": 8e-3}, {"name": "tip", "point": (np.array([L, 0.0, 0.0]) * 1e-3 - com).tolist(), "role": "outboard", "radius": 8e-3}]
    fx = flexible_link(block["sdf"], link, mat, frames)
    assert fx["normalization"] == "mass_normalized"
    E = fx["fe"]["modulus"]
    rho, I, A, Lm = 1240.0, b ** 4 / 12 * 1e-12, b * b * 1e-6, L * 1e-3
    f1 = 1.875 ** 2 / (2 * math.pi) * math.sqrt(E * I / (rho * A * Lm ** 4))
    assert abs(fx["frequencies_hz"][0] - f1) / f1 < 0.15
    sag = rho * A * 9.81 * Lm ** 4 / (8 * E * I)
    assert abs(fx["gravity_sag_m"] - sag) / sag < 0.15
    assert fx["modes"] == 6 and all(v == 0.0 for v in fx["boundary_shapes"][0][0])  # the root does not move
    assert abs(fx["boundary_shapes"][0][1][1]) > 1.0 or abs(fx["boundary_shapes"][0][1][2]) > 1.0  # the tip does
    assert np.array(fx["stress_per_mode"]).shape == (6, len(fx["stress_cells"]), 6) and np.abs(fx["stress_per_mode"]).max() > 0


def test_motor_physics_blocks():
    for mid, spec in MOTOR_LIBRARY.items():
        phys = motor_physics(spec)
        e, g = phys["electrical"], phys["gearbox"]
        assert e["resistance"] > 0 and e["torque_constant"] > 0 and e["back_emf_constant"] > 0 and e["inductance"] > 0, mid
        assert g["ratio"] >= 1.0 and 0 < g["efficiency"] <= 1.0, mid
        # The electromechanical chain reproduces the library stall torque at the output within 10 %.
        if spec.kind != "linear":
            stall = e["torque_constant"] * e["stall_current"] * g["ratio"] * g["efficiency"]
            assert abs(stall - spec.stall_torque) / spec.stall_torque < 0.1, (mid, stall, spec.stall_torque)
    assert motor_physics(MOTOR_LIBRARY["sg90"])["firmware"]["kind"] == "servo"
    assert motor_physics(MOTOR_LIBRARY["nema17"])["firmware"]["kind"] == "stepper"
    assert motor_physics(MOTOR_LIBRARY["mg996r"], 2.0)["gearbox"]["max_output_torque"] == pytest.approx(2.0)


def test_results_and_identification_round_trip(tmp_path):
    doc, ops = _leg()
    res = {"version": 1, "links": {"thigh": {"peak_stress_pa": 1.2e7, "yield_margin": 2.75, "hotspot": {"cells": [[0, 0, 0]], "stress_pa": [1.2e7]}}}, "joints": {"knee": {"peak_reaction_force_n": 4.2, "bearing_margin": 3.0}}, "motors": {"hip motor": {"stall_margin": 0.4, "peak_current_a": 1.1}}, "trace": {"t": [0, 1]}}
    p = str(tmp_path / "leg.simresult.json")
    with open(p, "w") as f:
        json.dump(res, f)
    out = ops.load_results(p)
    assert out['trace'] == res['trace'] and doc.results["links"]["thigh"]["yield_margin"] == 2.75
    assert out['stale']  # Legacy results have no captured physical identity.
    thigh = next(n for n in doc.walk() if n.name == "thigh")
    assert thigh.results["section"] == "links" and thigh.results["peak_stress_pa"] == 1.2e7
    fit = {"identification": {"knee": {"friction": {"coulomb": 0.004, "viscous": 0.0005}, "backlash": 0.02, "stiffness_scale": 0.8, "rms_error_rad": 0.01}}, "source_log": "run1.csv"}
    fp = str(tmp_path / "fit.json")
    with open(fp, "w") as f:
        json.dump(fit, f)
    ops.apply_identification(fp)
    assert doc.robot_settings["identification"]["knee"]["backlash"] == 0.02
    model = export_physical_model(doc, None, flex=False)
    knee = next(j for j in model["joints"] if j["name"] == "knee")
    assert knee["physics"]["identified"]["friction"]["coulomb"] == 0.004 and model["identification"]["knee"]["source_log"] == "run1.csv"
    # Saved and reloaded documents keep settings, results and engineering materials.
    ops.set_material_props("petg", youngs_modulus=2.5e9)
    path = str(tmp_path / "leg.rcad")
    doc.save(path)
    back = Document.load(path)
    assert back.robot_settings["battery"]["cells"] == 2 and back.results["links"]["thigh"]["yield_margin"] == 2.75
    assert back.materials["petg"].props()["youngs_modulus"] == 2.5e9
    assert next(n for n in back.walk() if n.name == "thigh").results["peak_stress_pa"] == 1.2e7
    assert any(n.kind == "sensor" for n in back.walk()) and any(n.kind == "cable" for n in back.walk())


def test_api_physical_routes(tmp_path):
    from robocad.api import ApiServer
    from robocad.client import RoboClient

    doc, ops = _leg()
    server = ApiServer(doc, port=0).start()
    try:
        c = RoboClient(server.url)
        model = c.physical(flex=False)
        assert model["version"] == 3 and len(model["links"]) == 3
        res = {"version": 1, "links": {"thigh": {"peak_stress_pa": 5e6, "yield_margin": 8.0}}, "joints": {}, "motors": {}}
        p = str(tmp_path / "r.simresult.json")
        with open(p, "w") as f:
            json.dump(res, f)
        assert c.load_results(p)["links"]["thigh"]["yield_margin"] == 8.0
        assert c.get("/results")["links"]["thigh"]["peak_stress_pa"] == 5e6
        shank = next(n["id"] for n in c.get("/nodes?kind=body") if n["name"] == "shank")
        sid = c.add_sensor("force", shank, [0, 0, -15], name="foot force")
        assert c.get(f"/nodes/{sid}")["robot"]["kind"] == "force"
        cid = c.add_cable(shank, [0, 0, 0], shank, [0, 0, 50], name="loop")
        assert c.get(f"/nodes/{cid}")["kind"] == "cable"
        assert c.set_battery(cells=3, chemistry="liion")["battery"]["nominal_voltage"] == pytest.approx(10.8)
        assert c.set_control(period_s=0.01, targets={"hip": 0.1})["control"]["period_s"] == 0.01
        fp = str(tmp_path / "fit.json")
        with open(fp, "w") as f:
            json.dump({"identification": {"hip": {"backlash": 0.01}}}, f)
        assert c.apply_identification(fp)["hip"]["backlash"] == 0.01
    finally:
        server.stop()
