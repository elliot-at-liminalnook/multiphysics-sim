"""GET /nodes/{id}/image: the read-only gap route the native viewer's CAD
mode textures reference image planes from (docs/cad-parity.md, cad-organize).
Headless, over HTTP."""

import base64
import io

import pytest
from PIL import Image

from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import Plane


@pytest.fixture
def served(tmp_path):
    doc = Document()
    ops = Ops(doc)
    path = tmp_path / "front.png"
    Image.new("RGB", (40, 20), "red").save(path)
    (ref,) = ops.import_references([path], Plane.xz())
    box = ops.box((0, 0, 0), (10, 10, 10), name="box")
    server = ApiServer(doc, port=0).start()
    try:
        yield doc, ref, box, path, RoboClient(server.url)
    finally:
        server.stop()


def test_image_route_returns_the_stored_bytes(served):
    doc, ref, _, path, client = served
    before = doc.revision
    got = client.get(f"/nodes/{ref}/image")
    assert got["id"] == ref
    assert got["revision"] == before
    assert (got["format"], got["width_px"], got["height_px"]) == ("png", 40, 20)
    data = base64.b64decode(got["data"])
    assert data == path.read_bytes() == doc.nodes[ref].image["data"]
    assert got["bytes"] == len(data)
    with Image.open(io.BytesIO(data)) as image:
        assert image.getpixel((0, 0)) == (255, 0, 0)
    assert doc.revision == before, "the route is read-only"


def test_image_route_refuses_other_nodes(served):
    _, _, box, _, client = served
    with pytest.raises(RuntimeError, match="→ 404: box is not a reference image"):
        client.get(f"/nodes/{box}/image")
    with pytest.raises(RuntimeError, match="→ 404: no node nope"):
        client.get("/nodes/nope/image")


def test_image_route_refuses_missing_or_unreadable_bytes(served):
    doc, ref, _, _, client = served
    name = doc.nodes[ref].name
    doc.nodes[ref].image["data"] = None  # an archive without image/<id> (Document.load)
    with pytest.raises(RuntimeError, match=f"→ 404: {name} has no stored image bytes"):
        client.get(f"/nodes/{ref}/image")
    doc.nodes[ref].image["data"] = b"not an image"
    with pytest.raises(RuntimeError, match="→ 422: .*the stored image cannot be read"):
        client.get(f"/nodes/{ref}/image")
