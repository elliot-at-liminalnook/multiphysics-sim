import pytest

from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import Body, KernelError
from robocad.kernel.occt import _compound, occ_faces


def imported_compound():
    doc = Document(); k = doc.kernel
    solids = [k.box((i * 20, 0, 0), (10, 10, 10)) for i in range(4)]
    free_face = occ_faces(k.box((100, 0, 0), (5, 5, 5)).shape)[0]
    body = Body(_compound([s.shape for s in solids] + [free_face]))
    node = doc.add_body(body, 'Import', material='steel')
    node.color = (.2, .3, .4)
    return doc, node


def test_extract_preserves_geometry_and_undo():
    doc, original = imported_compound(); ops = Ops(doc)
    before = doc.kernel.mass_properties(original.body)
    result = ops.extract_components(original.id, {'Crank': [1], 'Coupler': [2, 3]}, doc.revision)
    ids = [original.id] + list(result['components'].values())
    properties = [doc.kernel.mass_properties(doc.nodes[i].body) for i in ids]
    assert sum(p.volume for p in properties) == pytest.approx(before.volume)
    assert sum(p.area for p in properties) == pytest.approx(before.area)
    assert [len(doc.kernel.solid_inventory(doc.nodes[i].body)) for i in ids] == [1, 1, 2]
    assert all(doc.nodes[i].color == original.color and doc.nodes[i].material == 'steel' for i in ids)
    assert ops.undo() == 'Extract components'
    assert len(doc.nodes) == 1
    assert len(doc.kernel.solid_inventory(original.body)) == 4
    ops.redo()
    assert set(ids) == set(doc.nodes)


@pytest.mark.parametrize('components', [{'A': [1, 1]}, {'A': [1], 'B': [1]}, {'A': [-1]}, {'A': [4]}, {'A': [True]}, {'A': []}, {'A': [0, 1, 2, 3]}])
def test_invalid_extraction_does_not_change_document(components):
    doc, node = imported_compound(); revision = doc.revision
    with pytest.raises(KernelError):
        Ops(doc).extract_components(node.id, components, revision)
    assert doc.revision == revision and len(doc.nodes) == 1


def test_rest_inventory_is_fast_and_rejects_stale_indices(monkeypatch):
    doc, node = imported_compound()
    def forbid(*args, **kwargs):
        pytest.fail('Inventory must not integrate geometry or tessellate')
    for method in ('mass_properties', 'faces', 'edges', 'tessellate'):
        monkeypatch.setattr(doc.kernel, method, forbid)
    server = ApiServer(doc, port=0).start()
    try:
        client = RoboClient(server.url)
        inventory = client.get(f'/nodes/{node.id}/solids')
        assert inventory['units'] == 'mm' and len(inventory['solids']) == 4
        assert inventory['solids'][1]['bbox_min'][0] == pytest.approx(20, abs=1e-5)
        Ops(doc).rename(node.id, 'Renamed')
        with pytest.raises(RuntimeError, match='409'):
            client.op('extract_components', node.id, {'Crank': [1]}, inventory['revision'])
        result = client.op('extract_components', node.id, {'Crank': [1]}, doc.revision)
        assert doc.nodes[result['components']['Crank']].name == 'Crank'
    finally:
        server.stop()
