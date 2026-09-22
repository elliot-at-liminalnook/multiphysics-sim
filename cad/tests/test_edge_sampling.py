"""Batch display sampling preserves topology and avoids quadratic rematching."""
import numpy as np
import pytest

from robocad.kernel import default_kernel
from robocad.kernel.base import CurveKind
from robocad.kernel import occt


@pytest.mark.parametrize('shape', ['box', 'cylinder', 'sphere'])
def test_batch_matches_individual_samples(shape):
    k = default_kernel()
    body = {'box': lambda: k.box((3, 4, 5), (10, 20, 30)),
            'cylinder': lambda: k.cylinder((3, 4, 5), (1, 1, 0), 5, 12),
            'sphere': lambda: k.sphere((3, 4, 5), 7)}[shape]()
    before = k.serialize(body)
    refs = k.edges(body)
    batch = k.sample_edges(body)
    assert [edge for edge, _ in batch] == refs
    for edge, points in batch:
        expected = k.sample_edge(edge, body, 2 if edge.kind == CurveKind.LINE else 24)
        np.testing.assert_allclose(points, expected, atol=1e-12)
    assert k.serialize(body) == before


def test_batch_enumerates_topology_once(monkeypatch):
    k = default_kernel()
    body = k.box((0, 0, 0), (10, 20, 30))
    original = occt.occ_edges
    calls = []
    def counted(shape):
        calls.append(shape)
        return original(shape)
    monkeypatch.setattr(occt, 'occ_edges', counted)
    assert len(k.sample_edges(body)) == 12
    assert len(calls) == 1


def test_viewport_uses_batch_for_picking(monkeypatch):
    from robocad.document import Document
    from robocad.commands import Ops
    from robocad.ui.viewport import _display_edges
    doc = Document()
    node = doc.nodes[Ops(doc).box((0, 0, 0), (10, 20, 30))]
    mesh = doc.mesh_of(node.id)
    def no_rematching(*args):
        raise AssertionError('Display sampling must not rematch individual edges')
    monkeypatch.setattr(doc.kernel, 'sample_edge', no_rematching)
    _, _, samples, vertices = _display_edges(doc, node, mesh,
        np.asarray(mesh.vertices, dtype=np.float32),
        np.asarray(mesh.triangles, dtype=np.uint32),
        np.asarray(mesh.triangle_face, dtype=np.int32))
    assert len(samples) == 12
    assert vertices.shape == (8, 3)
