import copy
import hashlib
import json
import zipfile

import numpy as np
import pytest
from OCP.gp import gp_Pnt

from robocad.capture_geometry import audit_captured_pair, captured_cad_transform, sdf_cell_probes
from robocad.kernel import default_kernel


def test_captured_transform_converts_units_and_rotates_about_original_com():
    pose = {"rotation": [[0, -1, 0], [1, 0, 0], [0, 0, 1]], "position_m": [1, 2, 3]}
    p = gp_Pnt(1010, 2000, 3000).Transformed(captured_cad_transform([1, 2, 3], pose))
    assert [p.X(), p.Y(), p.Z()] == pytest.approx([1000, 2010, 3000])
    for rotation in [np.diag([1, 1, -1]), np.diag([2, 1, 1]), np.full((3, 3), np.nan)]:
        with pytest.raises(ValueError, match="rigid pose"):
            captured_cad_transform([1, 2, 3], dict(pose, rotation=rotation))


def test_grid_cell_probes_reconstruct_a_rotated_affine_distance_field():
    pose = {"rotation": [[0, -1, 0], [1, 0, 0], [0, 0, 1]], "position_m": [1, 2, 3]}
    # A plane field is reproduced exactly by trilinear interpolation.
    sdf = {"origin": [0, 0, 0], "cell": .01, "dims": [2, 2, 2],
           "values": [(.01*x + .02*y + .03*z - .015) for x, y, z in np.ndindex(2, 2, 2)]}
    rows = sdf_cell_probes(sdf, pose, [.996, 2.002, 3.007])
    assert len(rows) == 8
    assert sum(r["weight"] for r in rows) == pytest.approx(1)
    assert sum(r["weight"] * r["stored_distance_m"] for r in rows) == pytest.approx(.016)
    assert rows[-1]["point_world_m"] == pytest.approx([.99, 2.01, 3.01])
    for bad in [dict(sdf, cell=0), dict(sdf, values=[]), dict(sdf, dims=[2.5, 2, 2])]:
        with pytest.raises(ValueError):
            sdf_cell_probes(bad, pose, [.996, 2.002, 3.007])
    with pytest.raises(ValueError, match="interior"):
        sdf_cell_probes(sdf, pose, [1, 2.1, 3])


def test_brep_pair_and_transported_contact_probe_preserve_source(tmp_path):
    kernel = default_kernel()
    cad = tmp_path / "two-boxes.rcad"
    with zipfile.ZipFile(cad, "w") as archive:
        archive.writestr("manifest.json", json.dumps({"nodes": [{"id": x, "name": x} for x in ["a", "b"]]}))
        for member, corner in [("a", (0, 0, 0)), ("b", (20, 0, 0))]:
            archive.writestr(f"brep/{member}.brep", kernel.serialize(kernel.box(corner, (10, 10, 10))))
    raw = cad.read_bytes()
    source = {"cad_sha256": hashlib.sha256(raw).hexdigest()}
    scene = {"robot": {"source": source, "links": [
        {"name": "a", "com": [.005, .005, .005], "members": ["a"]},
        {"name": "b", "com": [.025, .005, .005], "members": ["b"]}]}}
    pose = lambda name, x: {"name": name, "rotation": np.eye(3).tolist(), "position_m": [x, .005, .005]}
    capture = {"source": source, "completed": True, "error": None, "frames": [
        {"time_s": 0, "poses": [pose("a", .005), pose("b", .025)], "contacts": []},
        {"time_s": 1, "poses": [pose("a", .025), pose("b", .025)],
         "contacts": [{"link": 0, "other": 1, "point_m": [.025, .005, .005]}]}]}
    before = copy.deepcopy((scene, capture))
    result = audit_captured_pair(cad, scene, capture, "a", "b", [0, 1], 1)
    start, end = result["frames"]
    assert start["member_distances"][0]["distance_mm"] == pytest.approx(10)
    assert end["member_distances"][0]["distance_mm"] == pytest.approx(0, abs=1e-9)
    probe = start["contact_point_probes"][0]
    assert probe["runtime_contact"] is None
    assert probe["point_world_m"] == pytest.approx([.005, .005, .005])
    assert probe["brep_members"][0]["solid_states"] == ["outside"]
    assert probe["brep_members"][0]["distance_mm"] == pytest.approx(15)
    assert all(p["brep_members"][0]["solid_states"] == ["inside"] for p in end["contact_point_probes"])
    assert all(p["brep_members"][0]["surface_distance_mm"] == pytest.approx(5) for p in end["contact_point_probes"])
    assert (scene, capture) == before and cad.read_bytes() == raw
    focused = audit_captured_pair(cad, scene, capture, "a", "b", [0, 1], 1,
                                 measure_pair_distances=False)
    assert focused["pair_distances_measured"] is False
    for full, points in zip(result["frames"], focused["frames"]):
        assert points["member_distances"] == []
        assert points["contact_point_probes"] == full["contact_point_probes"]
    geometric = copy.deepcopy(capture)
    for frame in geometric["frames"]:
        frame["internal_contacts"] = frame.pop("contacts")
    assert audit_captured_pair(cad, scene, geometric, "a", "b", [0, 1], 1) == result
    for args in [("a", "a", [0], None), ("a", "b", [.5], None), ("a", "b", [0], 0)]:
        with pytest.raises(ValueError):
            audit_captured_pair(cad, scene, capture, *args)
    bad = copy.deepcopy(capture)
    bad["source"] = {"cad_sha256": "different"}
    with pytest.raises(ValueError, match="provenance"):
        audit_captured_pair(cad, scene, bad, "a", "b", [0])
