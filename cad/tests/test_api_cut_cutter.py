"""POST /ops/cut with a sheet or curve node as the cutter (the GUI's "Cut
with selection": `ops.cut(body, cutter_id)`). `cut(cutter: str | Body |
Plane)` once sent every string through the plane converter, which refused
a sheet's id; a plane name or plane node id still converts to a Plane.
Headless, over HTTP."""

import pytest

from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.document import Document
from robocad.kernel import Plane
from robocad.kernel.sketch import Sketch


@pytest.fixture()
def served():
    doc = Document()
    server = ApiServer(doc, port=0).start()
    client = RoboClient(server.url)
    yield doc, client
    server.stop()


def _box(client):
    return client.create(kind="box", corner=[0, 0, 0], size=[20, 10, 5], name="Box")["id"]


def test_cut_with_a_sheet_node(served):
    doc, client = served
    box = _box(client)
    sk = Sketch(Plane.xz(5.0))
    outer = sk.rectangle((-50.0, -50.0), (100.0, 100.0))
    with doc._lock:
        sheet = doc.add_body(sk.to_face(outer), "Sheet").id
    assert doc.nodes[sheet].kind == "sheet"
    answer = client.post("/ops/cut", {"args": [box, sheet]})
    parts = answer["result"]
    assert parts[0] == box and len(parts) == 2
    assert answer["history"]["undo"][-1] == "Cut"
    volumes = sorted(client.get(f"/nodes/{p}")["mass"]["volume_mm3"] for p in parts)
    assert volumes == pytest.approx([500.0, 500.0])


def test_plane_cutters_still_convert(served):
    _, client = served
    box = _box(client)
    plane = client.create(kind="plane", plane={"axis": "x", "offset": 5}, name="Cutter plane")["id"]
    parts = client.post("/ops/cut", {"args": [box, plane]})["result"]
    assert len(parts) == 2
    volumes = sorted(client.get(f"/nodes/{p}")["mass"]["volume_mm3"] for p in parts)
    assert volumes == pytest.approx([250.0, 750.0])
    other = _box(client)
    assert len(client.post("/ops/cut", {"args": [other, {"axis": "z", "offset": 1}]})["result"]) == 2


def test_unknown_cutter_is_still_refused(served):
    _, client = served
    box = _box(client)
    with pytest.raises(RuntimeError) as e:
        client.post("/ops/cut", {"args": [box, "nope"]})
    assert "→ 400: unknown plane 'nope'" in str(e.value)
