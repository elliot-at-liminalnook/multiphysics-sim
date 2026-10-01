"""POST /ops/fill with a node id (the viewer's tool.fill, the GUI's
`ops.fill(ids[0])`). `fill(edges: str | Body)` has a parameter named
`edges`, and the converter once sent every `edges` argument through the
edge-reference converter, which refused a node id ("an edge is {node,
edge}"). Edge references for `fillet(node_id, edges: Sequence[EdgeRef])`
still convert. Headless, through the service the REST route calls."""

import pytest

from robocad.api import ApiError, Service
from robocad.document import Document


def test_fill_takes_a_closed_curve_node_id():
    service = Service(Document())
    sketch = service.create({"kind": "sketch", "plane": "xy", "calls": [["circle", [[0, 0], 10]]]})["id"]
    answer = service.op("fill", [sketch], {})
    patch = answer["result"]
    assert isinstance(patch, str) and patch != sketch
    assert patch in service.doc.nodes
    assert answer["history"]["undo"][-1] == "Fill"


def test_fillet_edges_still_convert_from_node_edge_refs():
    service = Service(Document())
    box = service.create({"kind": "box", "corner": [0, 0, 0], "size": [20, 10, 5]})["id"]
    answer = service.op("fillet", [box, [{"node": box, "edge": 0}], 1.0], {})
    assert answer["result"] == box
    assert answer["history"]["undo"][-1] == "Fillet"


def test_fill_of_an_unknown_node_id_is_404():
    service = Service(Document())
    with pytest.raises(ApiError) as refused:
        service.op("fill", ["no-such-node"], {})
    assert refused.value.status == 404
    assert "no-such-node" in str(refused.value)
