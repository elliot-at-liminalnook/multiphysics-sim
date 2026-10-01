"""GET /nodes/{id}/control_points?face=i: a face's B-spline poles (the
GUI's "Control points" overlay, `kernel.control_points`). Read-only.
Headless, over HTTP."""

import pytest

from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.document import Document


@pytest.fixture(scope="module")
def served():
    doc = Document()
    server = ApiServer(doc, port=0).start()
    client = RoboClient(server.url)
    box = client.create(kind="box", corner=[0, 0, 0], size=[20, 10, 5], name="Box")["id"]
    pin = client.create(kind="cylinder", base=[40, 0, 0], axis=[0, 0, 1], radius=4, height=6, name="Pin")["id"]
    sketch = client.create(kind="sketch", plane="xy", name="Sketch", calls=[["circle", [[0, 0], 3]]])["id"]
    yield doc, client, box, pin, sketch
    server.stop()


def _error(call, status):
    with pytest.raises(RuntimeError) as e:
        call()
    assert f"→ {status}: " in str(e.value), str(e.value)
    return str(e.value).split(f"→ {status}: ", 1)[1]


def test_shape_and_read_only(served):
    doc, client, box, pin, _ = served
    before = client.get("/")
    for nid in (box, pin):
        faces = client.get(f"/nodes/{nid}/faces")
        for f in faces:
            got = client.get(f"/nodes/{nid}/control_points?face={f['index']}")
            assert list(got) == ["node", "face", "rows"]
            assert (got["node"], got["face"]) == (nid, f["index"])
            rows = got["rows"]
            assert rows and all(row and all(len(p) == 3 for p in row) for row in rows)
            assert len({len(row) for row in rows}) == 1
            if f["kind"] == "plane":
                # The poles of a planar face lie on its plane.
                for row in rows:
                    for p in row:
                        d = sum((p[k] - f["centroid"][k]) * f["normal"][k] for k in range(3))
                        assert d == pytest.approx(0.0, abs=1e-6)
    after = client.get("/")
    assert (after["revision"], after["dirty"]) == (before["revision"], before["dirty"])


def test_matches_the_kernel(served):
    doc, client, box, _, _ = served
    body = doc.resolved_body(box)
    face = doc.kernel.faces(body)[2]
    want = doc.kernel.control_points(body, face)
    got = client.get(f"/nodes/{box}/control_points?face=2")["rows"]
    assert [[pytest.approx(list(p)) for p in row] for row in want] == got


@pytest.mark.parametrize("bad", ["6", "-1", "99"])
def test_face_out_of_range(served, bad):
    _, client, box, _, _ = served
    assert _error(lambda: client.get(f"/nodes/{box}/control_points?face={bad}"), 400) == f"face index {int(bad)} out of range (0..5)"


@pytest.mark.parametrize("bad", ["abc", "1.5", "1e3"])
def test_face_not_an_index(served, bad):
    _, client, box, _, _ = served
    assert _error(lambda: client.get(f"/nodes/{box}/control_points?face={bad}"), 400) == f"face must be a face index, got {bad!r}"


def test_missing_face_and_nodes_without_geometry(served):
    _, client, box, _, sketch = served
    assert _error(lambda: client.get(f"/nodes/{box}/control_points"), 400) == "face must be a face index, got None"
    assert _error(lambda: client.get(f"/nodes/{sketch}/control_points?face=0"), 404) == "Sketch has no geometry"
    assert _error(lambda: client.get("/nodes/nope/control_points?face=0"), 404) == "no node nope"
