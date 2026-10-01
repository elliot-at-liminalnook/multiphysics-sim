"""GET /nodes/{id}/curvature_comb and /continuity: the GUI's curvature comb
and continuity check overlays as data (`analysis.curvature_comb`,
`analysis.continuity_report`). Read-only. Headless, over HTTP."""

import math

import pytest

from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.document import Document
from robocad.kernel import Plane
from robocad.kernel.sketch import Sketch


@pytest.fixture(scope="module")
def served():
    doc = Document()
    server = ApiServer(doc, port=0).start()
    client = RoboClient(server.url)
    box = client.create(kind="box", corner=[0, 0, 0], size=[20, 10, 5], name="Box")["id"]
    pin = client.create(kind="cylinder", base=[40, 0, 0], axis=[0, 0, 1], radius=4, height=6, name="Pin")["id"]
    sketch = client.create(kind="sketch", plane="xy", name="Sketch", calls=[["circle", [[0, 0], 3]]])["id"]
    ring = Sketch(Plane.xy())
    ring.circle((0.0, 0.0), 5.0)
    with doc._lock:
        curve = doc.add_body(ring.to_body(), "Ring").id
    assert doc.nodes[curve].kind == "curve"
    yield doc, client, box, pin, sketch, curve
    server.stop()


def _error(call, status):
    with pytest.raises(RuntimeError) as e:
        call()
    assert f"→ {status}: " in str(e.value), str(e.value)
    return str(e.value).split(f"→ {status}: ", 1)[1]


def test_comb_of_a_circle(served):
    _, client, _, _, _, curve = served
    before = client.get("/")
    got = client.get(f"/nodes/{curve}/curvature_comb")
    assert list(got) == ["node", "lines"] and got["node"] == curve
    # RoboCAD's defaults: 48 samples, scale 5: κ = 1/5 → every tooth is 1 mm, towards the centre.
    assert len(got["lines"]) == 48
    for a, b in got["lines"]:
        assert len(a) == len(b) == 3
        assert math.hypot(a[0], a[1]) == pytest.approx(5.0, abs=1e-6)
        assert math.dist(a, b) == pytest.approx(1.0, abs=1e-6)
        assert math.hypot(b[0], b[1]) == pytest.approx(4.0, abs=1e-6)
    scaled = client.get(f"/nodes/{curve}/curvature_comb?scale=10&samples=12")["lines"]
    assert len(scaled) == 12 and all(math.dist(a, b) == pytest.approx(2.0, abs=1e-6) for a, b in scaled)
    after = client.get("/")
    assert (after["revision"], after["dirty"]) == (before["revision"], before["dirty"])


def test_comb_only_on_curves_and_sketches(served):
    _, client, box, _, sketch, _ = served
    assert _error(lambda: client.get(f"/nodes/{box}/curvature_comb"), 400) == "Box is a body: the curvature comb is drawn on curves and sketches"
    # A sketch node holds no body: the GUI draws no comb for it, and neither does this.
    assert client.get(f"/nodes/{sketch}/curvature_comb") == {"node": sketch, "lines": []}
    assert _error(lambda: client.get("/nodes/nope/curvature_comb"), 404) == "no node nope"


@pytest.mark.parametrize("query,message", [
    ("samples=1", "samples must be an integer from 2 to 512, got '1'"),
    ("samples=513", "samples must be an integer from 2 to 512, got '513'"),
    ("samples=x", "samples must be an integer from 2 to 512, got 'x'"),
    ("scale=nan", "scale must be a finite number, got 'nan'"),
    ("scale=abc", "scale must be a finite number, got 'abc'"),
])
def test_comb_bad_parameters(served, query, message):
    _, client, _, _, _, curve = served
    assert _error(lambda: client.get(f"/nodes/{curve}/curvature_comb?{query}"), 400) == message


def test_comb_parameter_bounds(served):
    _, client, _, _, _, curve = served
    for n in (2, 512):
        assert len(client.get(f"/nodes/{curve}/curvature_comb?samples={n}")["lines"]) == n


def test_continuity_of_a_box(served):
    _, client, box, _, _, _ = served
    before = client.get("/")
    got = client.get(f"/nodes/{box}/continuity")
    assert list(got) == ["node", "edges", "counts"] and got["node"] == box
    edges = client.get(f"/nodes/{box}/edges")
    assert [e["index"] for e in got["edges"]] == [e["index"] for e in edges]
    for e, ref in zip(got["edges"], edges):
        assert list(e) == ["index", "continuity", "points"]
        # Two planes at 90°: G0 (a crease).
        assert e["continuity"] == "G0"
        # `kernel.sample_edge(e, body, 16)`: a line is its two ends.
        assert len(e["points"]) == 2 and all(len(p) == 3 for p in e["points"])
        assert e["points"][0] == pytest.approx(ref["start"], abs=1e-6) or e["points"][0] == pytest.approx(ref["end"], abs=1e-6)
    assert list(got["counts"].items()) == [("G0", 12), ("G1", 0), ("G2", 0), ("boundary", 0)]
    after = client.get("/")
    assert (after["revision"], after["dirty"]) == (before["revision"], before["dirty"])


def test_continuity_counts_every_edge(served):
    _, client, _, pin, _, curve = served
    for nid in (pin, curve):
        got = client.get(f"/nodes/{nid}/continuity")
        assert set(got["counts"]) == {"G0", "G1", "G2", "boundary"}
        assert sum(got["counts"].values()) == len(got["edges"]) == len(client.get(f"/nodes/{nid}/edges"))
        assert all(e["continuity"] in got["counts"] for e in got["edges"])
        # Curves are sampled at 16 points.
        assert all(len(e["points"]) == 16 for e, ref in zip(got["edges"], client.get(f"/nodes/{nid}/edges")) if ref["kind"] == "circle")
    # A wire's edges border no faces.
    assert client.get(f"/nodes/{curve}/continuity")["counts"]["boundary"] == 1


def test_continuity_needs_geometry(served):
    _, client, _, _, sketch, _ = served
    assert _error(lambda: client.get(f"/nodes/{sketch}/continuity"), 404) == "Sketch has no geometry"
    assert _error(lambda: client.get("/nodes/nope/continuity"), 404) == "no node nope"


def test_comb_scale_is_bounded_so_the_json_stays_finite(served):
    _, client, _, _, _, curve = served
    for big in ("1e7", "-1e300", "1e308"):
        assert _error(lambda: client.get(f"/nodes/{curve}/curvature_comb?scale={big}"), 400) == (
            f"scale must be at most 1e6 in magnitude, got '{big}'")
    for edge in ("1e6", "-1e6"):
        lines = client.get(f"/nodes/{curve}/curvature_comb?scale={edge}&samples=4")["lines"]
        assert len(lines) == 4 and all(math.isfinite(c) for line in lines for p in line for c in p)
