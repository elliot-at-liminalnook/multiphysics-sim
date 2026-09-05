"""The REST API, headless: CRUD, ops, sketches, undo, render, export."""

import json
import threading
import urllib.request

import pytest

from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.document import Document


@pytest.fixture(scope="module")
def client():
    server = ApiServer(Document(), port=0).start()
    yield RoboClient(server.url)
    server.stop()


def test_health_and_crud(client):
    h = client.get("/")
    assert h["ok"] and h["version"]
    box = client.create(kind="box", corner=[0, 0, 0], size=[20, 10, 5], name="Base", material="steel")
    assert box["name"] == "Base" and box["mass"]["volume_mm3"] == pytest.approx(1000)
    assert box["mass"]["mass_g"] == pytest.approx(7.85)
    got = client.get(f"/nodes/{box['id']}")
    assert got["material"] == "steel"
    client.patch(f"/nodes/{box['id']}", {"name": "Plate", "visible": False})
    assert client.get(f"/nodes/{box['id']}")["name"] == "Plate"
    assert not client.get(f"/nodes/{box['id']}")["visible"]
    client.patch(f"/nodes/{box['id']}", {"visible": True})
    faces = client.get(f"/nodes/{box['id']}/faces")
    assert len(faces) == 6
    top = next(i for i, f in enumerate(faces) if f["normal"][2] > 0.9)
    r = client.op("push_pull", box["id"], {"node": box["id"], "face": top}, 5)
    assert client.get(f"/nodes/{box['id']}")["mass"]["volume_mm3"] == pytest.approx(2000)
    assert client.undo()["undone"] == "Push/Pull"
    assert client.get(f"/nodes/{box['id']}")["mass"]["volume_mm3"] == pytest.approx(1000)
    client.delete(f"/nodes/{box['id']}")
    assert all(n["id"] != box["id"] for n in client.get("/nodes"))


def test_sketch_extrude_and_boolean(client):
    sk = client.create(kind="sketch", plane="xy", calls=[["rectangle", [[0, 0], [30, 20]]], ["circle", [[15, 10], 3]]])
    assert len(sk["sketch"]["curves"]) == 2
    body = client.op("extrude", sk["id"], 6)
    v = client.get(f"/nodes/{body}")["mass"]["volume_mm3"]
    assert v == pytest.approx(30 * 20 * 6 - 3.141592653589793 * 9 * 6, rel=1e-4)
    cyl = client.create(kind="cylinder", base=[5, 5, -1], axis=[0, 0, 1], radius=2, height=10)
    client.op("boolean", body, [cyl["id"]], "subtract")
    assert client.get(f"/nodes/{body}")["mass"]["volume_mm3"] < v
    holes = [f for f in client.get(f"/nodes/{body}/faces") if f["kind"] == "cylinder"]
    assert len(holes) == 2
    client.op("set_diameter", body, {"node": body, "face": holes[0]["index"]}, 8.4)
    assert any(abs(f["radius"] - 4.2) < 1e-6 for f in client.get(f"/nodes/{body}/faces") if f["kind"] == "cylinder")
    rep = client.get(f"/nodes/{body}/validate")
    assert rep["valid"] and rep["watertight"]
    sec = client.get(f"/nodes/{body}/section?plane=xz")
    assert sec


def test_ops_listing_and_errors(client):
    ops = client.get("/ops")
    assert "fillet" in ops and "shell" in ops
    with pytest.raises(RuntimeError) as e:
        client.op("nonexistent")
    assert "404" in str(e.value)
    with pytest.raises(RuntimeError) as e:
        client.op("box", [0, 0, 0], [0, 1, 1])
    assert "422" in str(e.value)
    nid = client.op('box', [0, 0, 0], [2, 3, 4], name='Named through client')
    assert client.get(f'/nodes/{nid}')['name'] == 'Named through client'
    client.delete(f'/nodes/{nid}')


def test_render_and_export(client, tmp_path):
    p = client.render(str(tmp_path / "a.png"), view="iso", w=400, h=300, labels=1)
    with open(p, "rb") as f:
        assert f.read(8) == b"\x89PNG\r\n\x1a\n"
    client.render(str(tmp_path / "b.png"), view="front", mode="xray", section="y:10")
    out = client.post("/export", {"format": "stl", "path": str(tmp_path / "a.stl"), "settings": {"unit": "mm"}})
    assert out["exported"].endswith("a.stl")
    saved = client.post("/save", {"path": str(tmp_path / "doc.rcad")})
    assert saved["saved"].endswith(".rcad")
    doc = client.get("/doc")
    assert doc["path"].endswith("doc.rcad") and doc["history"]["undo"]


def test_materials_and_selection(client):
    m = client.post("/materials", {"name": "Carbon PA", "density": 1.3})
    assert any(x["id"] == m["id"] for x in client.get("/materials"))
    nodes = client.get("/nodes?kind=body")
    sel = client.put("/selection", {"items": [[nodes[0]["id"], "body", 0]]})
    assert sel["items"][0][0] == nodes[0]["id"]
