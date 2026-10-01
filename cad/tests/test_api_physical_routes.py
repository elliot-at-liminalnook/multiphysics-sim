"""GET /results/nodes and GET /physical?…&planar=1: the routes the native
viewer's CAD mode reads results and the planar physical model through.
Headless, over HTTP."""

import json

import pytest

from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.commands import Ops
from robocad.document import Document
from robocad.physical import results_margins


def _robot():
    """A grounded block, a thigh on a revolute hip and the hip's servo."""
    doc = Document()
    ops = Ops(doc)
    base = ops.box((-25.0, -20.0, 200.0), (50.0, 40.0, 40.0), name="ground")
    ops.set_material([base], "al")
    thigh = ops.box((-8.0, -6.0, 90.0), (16.0, 12.0, 120.0), name="thigh")
    ops.set_material([thigh], "petg")
    ops.set_ground(base)
    motor = ops.add_motor("mg996r", (0.0, -20.0, 200.0), (0.0, 1.0, 0.0), mount_on=base, name="hip motor")
    hip = ops.add_joint("revolute", base, thigh, (0.0, 0.0, 200.0), (0.0, 1.0, 0.0), lower=-1.5, upper=1.5, name="hip")
    ops.attach_motor(hip, motor)
    return doc, {"ground": base, "thigh": thigh, "hip motor": motor, "hip": hip}


@pytest.fixture
def served():
    doc, ids = _robot()
    server = ApiServer(doc, port=0).start()
    try:
        yield doc, ids, RoboClient(server.url)
    finally:
        server.stop()


def test_results_nodes_without_results_is_empty(served):
    doc, _, client = served
    got = client.get("/results/nodes")
    assert got == {"revision": doc.revision, "path": None, "loaded": None, "stale": None, "provenance": None, "margins": {}, "nodes": {}}


def test_results_nodes_lists_margins_blocks_and_yield_strength(served, tmp_path):
    doc, ids, client = served
    res = {
        "version": 1,
        "links": {"thigh": {"peak_stress_pa": 1.2e7, "yield_margin": 2.75, "peak_temperature_c": 31.0, "tg_margin_c": 49.0, "hotspot": {"cells": [[0, 0, 0]], "stress_pa": [1.2e7]}}},
        "joints": {"hip": {"peak_reaction_force_n": 4.2, "bearing_margin": 3.0, "screw_shear_margin": 6.5}},
        "motors": {"hip motor": {"stall_margin": 0.4, "peak_current_a": 1.1, "peak_winding_c": 45.0}},
        "provenance": {"physical_hash": "not-this-document"},
    }
    path = tmp_path / "leg.simresult.json"
    path.write_text(json.dumps(res))
    before = client.get("/")
    client.post("/results/load", {"path": str(path)})
    got = client.get("/results/nodes")
    # Loading results and reading them do not move the revision.
    assert got["revision"] == doc.revision == before["revision"]
    assert (got["path"], got["stale"], got["provenance"]) == (str(path), True, {"physical_hash": "not-this-document"})
    assert got["loaded"] == doc.results["loaded"]
    assert got["margins"] == json.loads(json.dumps(results_margins(doc)))
    assert set(got["margins"]) == {ids["thigh"], ids["hip"], ids["hip motor"]}
    assert got["margins"][ids["thigh"]] == {"yield_margin": 2.75, "peak_stress_pa": 1.2e7, "peak_temperature_c": 31.0, "tg_margin_c": 49.0}
    assert got["margins"][ids["hip motor"]]["mount_tg_margin_c"] is None
    thigh = got["nodes"][ids["thigh"]]
    assert thigh["results"] == {"section": "links", **res["links"]["thigh"]}
    assert thigh["yield_strength_pa"] == doc.materials["petg"].props()["yield_strength"]
    assert got["nodes"][ids["hip"]]["results"]["section"] == "joints"
    assert got["nodes"][ids["hip"]]["yield_strength_pa"] is None, "a joint node has no material"
    motor = got["nodes"][ids["hip motor"]]
    assert motor["results"] == {"section": "motors", **res["motors"]["hip motor"]}
    assert motor["yield_strength_pa"] == doc.materials["abs"].props()["yield_strength"], "a servo body is ABS"
    assert ids["ground"] not in got["nodes"] and ids["ground"] not in got["margins"]
    # The whole file stays at GET /results; POST /results/load is unchanged.
    assert client.get("/results")["links"]["thigh"]["yield_margin"] == 2.75


def test_physical_planar_hint_only_when_asked(served):
    _, _, client = served
    plain = client.get("/physical?flex=0")
    assert plain["version"] == 4 and plain["planar"] is None
    planar = client.get("/physical?flex=0&planar=1")
    # Plane.xz(): origin 0, normal −Y.
    assert planar["planar"] == {"normal": [0.0, -1.0, 0.0], "origin": [0.0, 0.0, 0.0]}
    assert client.get("/physical?flex=0&planar=0")["planar"] is None
    assert len(planar["links"]) == len(plain["links"])
