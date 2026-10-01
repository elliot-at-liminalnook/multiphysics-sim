"""GET /print/study and GET /print/registry: the reads the native viewer's
Print menu starts from (the document's print study and split groups, and
the printers in registry order). Headless, over HTTP; no OCCT split."""

import pytest

from robocad import print_registry as pr
from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.commands import Ops
from robocad.document import Document


@pytest.fixture
def served():
    doc = Document()
    ops = Ops(doc)
    server = ApiServer(doc, port=0).start()
    try:
        yield doc, ops, RoboClient(server.url)
    finally:
        server.stop()


def test_print_study_on_a_fresh_document_is_empty(served):
    doc, _, client = served
    assert client.get("/print/study") == {"revision": doc.revision, "study": None, "splits": []}


def test_print_study_answers_the_study_and_split_groups_in_tree_order(served):
    doc, ops, client = served
    a = ops.box((0.0, 0.0, 0.0), (10.0, 10.0, 10.0), name="a")
    b = ops.box((20.0, 0.0, 0.0), (10.0, 10.0, 10.0), name="b")
    c = ops.box((40.0, 0.0, 0.0), (10.0, 10.0, 10.0), name="c")
    group = ops.group([b], name="b split")
    study = {"printer": "bambu-p1s", "material": "petg-hf",
             "parts": [{"node": a, "fixtures": [{"region": {"bottom": True}}], "loads": [{"region": {"faces": [3]}, "direction": [0, 0, -1], "magnitude": 20.0}]}]}
    ops.set_robot_setting("print_study", study)
    # As apply_split leaves them: the group and nothing else carries print_split.
    doc.nodes[c].robot = {"print_split": {"source": c, "printer": "bambu-h2c"}}
    doc.nodes[group].robot = {"print_split": {"source": b, "printer": "bambu-h2c"}}
    doc.nodes[a].robot = {"print_split": None, "fasteners": []}
    walk = [n.id for n in doc.walk()]
    got = client.get("/print/study")
    assert got["revision"] == doc.revision
    assert got["study"] == study
    assert got["splits"] == [i for i in walk if i in (group, c)]
    assert set(got["splits"]) == {group, c}


def test_print_study_is_read_only(served):
    _, _, client = served
    with pytest.raises(RuntimeError, match="405"):
        client.post("/print/study", {"study": {}})


def test_print_registry_lists_printers_in_registry_order(served):
    _, _, client = served
    got = client.get("/print/registry")
    data, sha = pr.load()
    assert got["sha256"] == sha
    assert list(got["printers"]) == list(data["printers"])
    assert list(got["printers"])[0] == "bambu-h2c"
    for k, v in got["printers"].items():
        assert v["usable_mm"] == list(pr.usable_mm(k))
        assert v["name"] == data["printers"][k]["name"]
    assert list(got["materials"]) == list(data["materials"])
    assert got["materials"]["pla-basic"]["cad_material"] == "pla"
