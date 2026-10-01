"""GET /nodes/{id}/edges?samples=N: the viewport's sampled edge polylines,
opt-in; without it the answer is unchanged. Headless, over HTTP."""

import math

import pytest

from robocad.api import ApiServer, edge_json
from robocad.client import RoboClient
from robocad.document import Document


@pytest.fixture(scope="module")
def served():
    doc = Document()
    server = ApiServer(doc, port=0).start()
    client = RoboClient(server.url)
    box = client.create(kind="box", corner=[0, 0, 0], size=[20, 10, 5], name="Box")["id"]
    cyl = client.create(kind="cylinder", base=[40, 0, 0], axis=[0, 0, 1], radius=4, height=6, name="Pin")["id"]
    yield doc, client, box, cyl
    server.stop()


def _close(a, b, tol=1e-6):
    return all(math.isclose(x, y, abs_tol=tol) for x, y in zip(a, b)) and len(a) == len(b) == 3


def test_without_samples_the_answer_is_unchanged(served):
    doc, client, box, cyl = served
    for nid in (box, cyl):
        got = client.get(f"/nodes/{nid}/edges")
        # The old answer exactly: edge_json of every kernel edge, JSON round-tripped.
        old = [edge_json(e) for e in doc.kernel.edges(doc.resolved_body(nid))]
        assert len(got) == len(old) > 0
        for g, o in zip(got, old):
            assert list(g) == ["index", "kind", "midpoint", "length", "start", "end", "center", "radius"]
            assert "points" not in g
            assert g["index"] == o["index"] and g["kind"] == o["kind"]
            assert g["length"] == pytest.approx(o["length"])
            assert _close(g["start"], o["start"]) and _close(g["end"], o["end"])


def test_samples_add_points_matching_each_edge(served):
    doc, client, box, cyl = served
    plain = client.get(f"/nodes/{box}/edges")
    sampled = client.get(f"/nodes/{box}/edges?samples=8")
    assert len(sampled) == len(plain)
    for p, s in zip(plain, sampled):
        assert list(s) == list(p) + ["points"]
        assert {k: v for k, v in s.items() if k != "points"} == p
        assert s["kind"] == "line" and len(s["points"]) == 2
        assert _close(s["points"][0], s["start"]) and _close(s["points"][-1], s["end"])
    edges = client.get(f"/nodes/{cyl}/edges?samples=8")
    circles = [e for e in edges if e["kind"] == "circle"]
    assert len(circles) == 2
    for e in circles:
        assert len(e["points"]) == 8
        assert _close(e["points"][0], e["start"]) and _close(e["points"][-1], e["end"])
        for q in e["points"]:
            assert len(q) == 3
            assert math.dist(q[:2], e["center"][:2]) == pytest.approx(4.0, abs=1e-6)
            assert q[2] == pytest.approx(e["center"][2], abs=1e-6)
    for e in edges:
        if e["kind"] == "line":
            assert len(e["points"]) == 2
    # Indices line up with the unsampled list.
    assert [e["index"] for e in edges] == [e["index"] for e in client.get(f"/nodes/{cyl}/edges")]


@pytest.mark.parametrize("bad", ["1", "257", "abc", "-3", "2.5", "9" * 5000])
def test_bad_samples_are_refused(served, bad):
    _, client, box, _ = served
    with pytest.raises(RuntimeError) as e:
        client.get(f"/nodes/{box}/edges?samples={bad}")
    assert "→ 400: " in str(e.value)
    # The message alone (the path in the error text already holds the value).
    msg = str(e.value).split("→ 400: ", 1)[1]
    assert msg.startswith("samples must be an integer from 2 to 256") and repr(bad) in msg


def test_bounds_are_accepted(served):
    _, client, _, cyl = served
    for n in (2, 256):
        circles = [e for e in client.get(f"/nodes/{cyl}/edges?samples={n}") if e["kind"] == "circle"]
        assert circles and all(len(e["points"]) == n for e in circles)
