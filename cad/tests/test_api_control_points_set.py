"""POST /ops/set_control_points: the GUI's "Move control points" over REST.
`points` is a grid of rows (`list[list[Vec3]]`, as GET
/nodes/{id}/control_points returns it); the edit is one undo step. Headless,
over HTTP."""

import pytest

from robocad.api import ApiServer, ArgConverter
from robocad.client import RoboClient
from robocad.document import Document


@pytest.fixture()
def served():
    doc = Document()
    server = ApiServer(doc, port=0).start()
    client = RoboClient(server.url)
    box = client.create(kind="box", corner=[0, 0, 0], size=[20, 10, 5], name="Box")["id"]
    yield doc, client, box
    server.stop()


def test_set_control_points_moves_the_poles_as_one_undo_step(served):
    doc, client, box = served
    top = next(f["index"] for f in client.get(f"/nodes/{box}/faces") if f["normal"][2] > 0.9)
    rows = client.get(f"/nodes/{box}/control_points?face={top}")["rows"]
    assert rows and all(len(row) >= 2 for row in rows)
    moved = [[[p[0], p[1], p[2] + 0.5] for p in row] for row in rows]
    undo_before = client.get("/history")["undo"]
    answer = client.post("/ops/set_control_points", {"args": [box, {"node": box, "face": top}, moved]})
    assert answer["result"] == box
    assert answer["history"]["undo"] == undo_before + ["Move control points"]
    got = client.get(f"/nodes/{box}/control_points?face={top}")["rows"]
    assert got != rows
    assert [[pytest.approx(p) for p in row] for row in moved] == got
    # One undo puts the poles back.
    assert client.post("/undo")["undone"] == "Move control points"
    back = client.get(f"/nodes/{box}/control_points?face={top}")["rows"]
    assert [[pytest.approx(p) for p in row] for row in rows] == back


def test_points_grid_and_flat_points_convert():
    # No `Ops` method takes a flat `points` list today (only
    # set_control_points has a `points` parameter); the converter still turns
    # a flat list of points into a list of tuples, as before.
    conv = ArgConverter(Document())
    assert conv._one("points", "list[list[Vec3]]", [[[0, 1, 2], [3, 4, 5]], [[6, 7, 8], [9, 10, 11]]]) == [
        [(0.0, 1.0, 2.0), (3.0, 4.0, 5.0)], [(6.0, 7.0, 8.0), (9.0, 10.0, 11.0)]]
    assert conv._one("points", None, [[0, 1, 2], [3, 4, 5]]) == [(0.0, 1.0, 2.0), (3.0, 4.0, 5.0)]
    assert conv._one("points", "list", [[0, 1, 2]]) == [(0.0, 1.0, 2.0)]


def test_second_edit_does_not_mutate_the_first_edits_undo_snapshot(served):
    # After one edit the face is a B-spline; editing it again must work on a
    # copy, or undo would bring back the second edit's poles.
    doc, client, box = served
    top = next(f["index"] for f in client.get(f"/nodes/{box}/faces") if f["normal"][2] > 0.9)
    rows = client.get(f"/nodes/{box}/control_points?face={top}")["rows"]
    first = [[[p[0], p[1], p[2] + 0.5] for p in row] for row in rows]
    client.post("/ops/set_control_points", {"args": [box, {"node": box, "face": top}, first]})
    top = next(f["index"] for f in client.get(f"/nodes/{box}/faces") if f["normal"][2] > 0.9)
    got_first = client.get(f"/nodes/{box}/control_points?face={top}")["rows"]
    second = [[[p[0], p[1], p[2] + 1.0] for p in row] for row in got_first]
    client.post("/ops/set_control_points", {"args": [box, {"node": box, "face": top}, second]})
    assert client.post("/undo")["undone"] == "Move control points"
    back = client.get(f"/nodes/{box}/control_points?face={top}")["rows"]
    assert [[pytest.approx(p) for p in row] for row in got_first] == back


def test_a_grid_of_the_wrong_shape_is_refused(served):
    doc, client, box = served
    top = next(f["index"] for f in client.get(f"/nodes/{box}/faces") if f["normal"][2] > 0.9)
    rows = client.get(f"/nodes/{box}/control_points?face={top}")["rows"]
    undo_before = client.get("/history")["undo"]
    for bad in (rows[:-1], [row[:-1] for row in rows], rows + [rows[0]]):
        with pytest.raises(RuntimeError) as e:
            client.post("/ops/set_control_points", {"args": [box, {"node": box, "face": top}, bad]})
        assert "control points must be a" in str(e.value)
    assert client.get("/history")["undo"] == undo_before
    assert client.get(f"/nodes/{box}/control_points?face={top}")["rows"] == rows
