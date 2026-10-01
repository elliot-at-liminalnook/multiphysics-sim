"""POST /clipboard/copy and /clipboard/paste: the GUI's Copy and Paste with
Placement over REST. Copy changes nothing; paste is one undo step "Paste"
(as `MainWindow.paste_with_placement`). Headless, over HTTP."""

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
    pin = client.create(kind="cylinder", base=[40, 0, 0], axis=[0, 0, 1], radius=4, height=6, name="Pin")["id"]
    yield doc, client, box, pin
    server.stop()


def _error(call, status):
    with pytest.raises(RuntimeError) as e:
        call()
    assert f"→ {status}: " in str(e.value), str(e.value)
    return str(e.value).split(f"→ {status}: ", 1)[1]


def test_copy_changes_nothing(served):
    doc, client, box, pin = served
    before = client.get("/")
    history = client.get("/history")
    clip = client.post("/clipboard/copy", {"ids": [box, pin]})
    assert clip["robocad_clipboard"] is True
    assert [item["node"]["id"] for item in clip["items"]] == [box, pin]
    assert all(item["brep"] for item in clip["items"])
    after = client.get("/")
    assert (after["revision"], after["dirty"], after["nodes"]) == (before["revision"], before["dirty"], before["nodes"])
    assert client.get("/history") == history


def test_copy_refuses_unknown_ids_and_bad_bodies(served):
    _, client, box, _ = served
    assert _error(lambda: client.post("/clipboard/copy", {"ids": [box, "nope"]}), 404) == "no node nope"
    assert _error(lambda: client.post("/clipboard/copy", {"ids": box}), 400) == "ids must be a list of node ids"
    assert client.post("/clipboard/copy", {"ids": []}) == {"robocad_clipboard": True, "items": []}


def test_paste_is_one_undo_step(served):
    doc, client, box, pin = served
    clip = client.post("/clipboard/copy", {"ids": [box, pin]})
    before = client.get("/")
    undo_before = client.get("/history")["undo"]
    answer = client.post("/clipboard/paste", {"clip": clip})
    pasted = answer["pasted"]
    assert len(pasted) == 2 and not {box, pin} & set(pasted)
    assert answer["history"]["undo"] == undo_before + ["Paste"]
    after = client.get("/")
    assert after["revision"] > before["revision"] and answer["revision"] == after["revision"]
    assert after["nodes"] == before["nodes"] + 2
    # Same geometry, same world placement (keep_placement=True).
    for src, new in zip((box, pin), pasted):
        a, b = client.get(f"/nodes/{src}")["mass"], client.get(f"/nodes/{new}")["mass"]
        assert b["volume_mm3"] == pytest.approx(a["volume_mm3"])
        assert b["bbox_min"] == pytest.approx(a["bbox_min"]) and b["bbox_max"] == pytest.approx(a["bbox_max"])
    # One undo removes every pasted node.
    assert client.post("/undo")["undone"] == "Paste"
    assert all(nid not in doc.nodes for nid in pasted)
    assert client.get("/")["nodes"] == before["nodes"]
    assert client.get("/history")["undo"] == undo_before
    # Redo brings them back as one step.
    assert client.post("/redo")["redone"] == "Paste"
    assert all(nid in doc.nodes for nid in pasted)


def test_paste_refuses_content_that_is_not_robocads(served):
    doc, client, _, _ = served
    before = client.get("/")
    history = client.get("/history")
    for clip in ({"items": []}, {"robocad_clipboard": False, "items": []}, "text", None, {"robocad_clipboard": True, "items": "x"}):
        assert _error(lambda: client.post("/clipboard/paste", {"clip": clip}), 400) == "Clipboard has no robocad content"
    # A malformed item leaves nothing behind outside the undo stack.
    msg = _error(lambda: client.post("/clipboard/paste", {"clip": {"robocad_clipboard": True, "items": [{"node": {"name": "X"}, "brep": "zz"}]}}), 400)
    assert msg.startswith("Clipboard has no robocad content: ")
    after = client.get("/")
    assert after["nodes"] == before["nodes"]
    assert client.get("/history") == history


def test_failed_paste_after_a_valid_item_leaves_the_document_untouched(served):
    # The valid first item is added before the bad second one fails; the
    # cleanup removes it and the revision and dirty flag come back too.
    doc, client, box, _ = served
    good = client.post("/clipboard/copy", {"ids": [box]})["items"][0]
    clip = {"robocad_clipboard": True, "items": [good, {"node": {"name": "X"}, "brep": "zz"}]}
    doc.dirty = False  # as after a save, so a stray `touch` would show
    before = client.get("/")
    history = client.get("/history")
    with pytest.raises(RuntimeError) as e:
        client.post("/clipboard/paste", {"clip": clip})
    status = int(str(e.value).split("→ ", 1)[1].split(":", 1)[0])
    assert 400 <= status < 600, str(e.value)
    after = client.get("/")
    assert after["nodes"] == before["nodes"]
    assert (after["revision"], after["dirty"]) == (before["revision"], before["dirty"])
    assert client.get("/history") == history


def test_paste_passes_kernel_errors_through_unprefixed(served):
    # Well-formed clip, unreadable B-rep: the kernel's message, as a 422.
    _, client, _, _ = served
    before = client.get("/")
    history = client.get("/history")
    clip = {"robocad_clipboard": True, "items": [{"node": {"name": "X"}, "brep": "00"}]}
    assert _error(lambda: client.post("/clipboard/paste", {"clip": clip}), 422) == "could not read the B-rep data"
    assert client.get("/")["nodes"] == before["nodes"]
    assert client.get("/history") == history
