"""The file-workflow gap routes the native viewer's cad-views-export uses:
POST /new (an empty .rcad written beside the open document, which is not
touched), POST /save/thumbnail (/save with the desktop's thumbnail, drawn
headless by the snapshot renderer) and GET /import/units (the mesh unit
prompt's guess). Headless, over HTTP."""

import zipfile

import pytest

from robocad.api import ApiServer
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


def _error(call, status):
    with pytest.raises(RuntimeError) as e:
        call()
    assert f"→ {status}: " in str(e.value), str(e.value)
    return str(e.value).split(f"→ {status}: ", 1)[1]


def _cube_obj(path, size):
    """An OBJ cube `size` units on a side (raw units: the unit prompt's input)."""
    s = size
    vertices = [(x, y, z) for x in (0, s) for y in (0, s) for z in (0, s)]
    faces = [(1, 2, 4, 3), (5, 7, 8, 6), (1, 5, 6, 2), (3, 4, 8, 7), (1, 3, 7, 5), (2, 6, 8, 4)]
    lines = [f"v {x} {y} {z}" for x, y, z in vertices]
    lines += [f"f {a} {b} {c}\nf {a} {c} {d}" for a, b, c, d in faces]
    path.write_text("\n".join(lines) + "\n")
    return str(path)


def test_new_writes_an_empty_document_and_leaves_the_open_one(served, tmp_path):
    _, client, box = served
    before = client.get("/")
    target = tmp_path / "fresh.rcad"
    assert client.post("/new", {"path": str(target)}) == {"created": str(target)}
    created = Document.load(str(target))
    assert len(created.nodes) == 0
    after = client.get("/")
    assert (after["revision"], after["dirty"], after["nodes"], after["path"]) == (before["revision"], before["dirty"], before["nodes"], before["path"])
    assert client.get(f"/nodes/{box}")["name"] == "Box"


def test_new_refuses_an_existing_file_and_other_names(served, tmp_path):
    _, client, _ = served
    target = tmp_path / "taken.rcad"
    target.write_bytes(b"keep me")
    assert "exists" in _error(lambda: client.post("/new", {"path": str(target)}), 409)
    assert target.read_bytes() == b"keep me"
    assert _error(lambda: client.post("/new", {"path": str(tmp_path / "model.step")}), 400) == "path must name a .rcad file"
    assert _error(lambda: client.post("/new", {}), 400) == "path must name a .rcad file"
    assert "could not write" in _error(lambda: client.post("/new", {"path": str(tmp_path / "missing" / "a.rcad")}), 422)


def test_save_thumbnail_headless_writes_a_png_thumbnail(served, tmp_path):
    doc, client, _ = served
    target = tmp_path / "with-thumb.rcad"
    answer = client.post("/save/thumbnail", {"path": str(target)})
    assert answer == {"saved": str(target), "thumbnail": True}
    thumb = Document.read_thumbnail(str(target))
    assert thumb is not None and thumb[:8] == b"\x89PNG\r\n\x1a\n"
    with zipfile.ZipFile(target) as z:
        assert "manifest.json" in z.namelist()
    # As /save: the document now lives at the new path and is clean.
    health = client.get("/")
    assert health["path"] == str(target) and health["dirty"] is False
    # Without a path, the document's own path (the plain /save rule).
    assert client.post("/save/thumbnail", {})["saved"] == str(target)


def test_save_thumbnail_needs_a_path_for_an_untitled_document(tmp_path):
    server = ApiServer(Document(), port=0).start()
    try:
        client = RoboClient(server.url)
        assert _error(lambda: client.post("/save/thumbnail", {}), 400) == "no path"
    finally:
        server.stop()


@pytest.mark.parametrize("size, guess", [(0.5, "m"), (10.0, "in"), (120.0, "mm")])
def test_import_units_guesses_from_the_largest_extent(served, tmp_path, size, guess):
    _, client, _ = served
    path = _cube_obj(tmp_path / f"cube-{size}.obj", size)
    before = client.get("/")
    answer = client.get(f"/import/units?path={path}")
    assert answer["guess"] == guess and answer["extent"] == pytest.approx(size)
    assert answer["path"] == path and answer["units"] == ["mm", "cm", "m", "in", "ft"]
    after = client.get("/")
    assert (after["revision"], after["nodes"]) == (before["revision"], before["nodes"])


def test_import_units_refuses_non_meshes_and_missing_files(served, tmp_path):
    _, client, _ = served
    assert "not a mesh file" in _error(lambda: client.get(f"/import/units?path={tmp_path / 'part.step'}"), 400)
    assert _error(lambda: client.get(f"/import/units?path={tmp_path / 'gone.stl'}"), 404) == f"no such file {tmp_path / 'gone.stl'}"
    assert _error(lambda: client.get("/import/units"), 400) == "path is required"
