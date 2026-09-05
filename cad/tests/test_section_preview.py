from types import SimpleNamespace

import numpy as np
import pytest

from robocad.kernel import Plane
from robocad.ui import section_preview as preview


def test_section_interpolates_endpoints_and_skips_coplanar_and_tangent():
    vertices = np.array([[-1, 0, 0], [1, 0, 0], [1, 2, 0]], dtype=np.float32)
    indices = np.array([[0, 1, 2]])
    cut = preview.mesh_segments(vertices, indices, Plane.yz())
    assert cut.shape == (2, 3)
    assert np.allclose(cut, [[0, 0, 0], [0, 1, 0]])
    assert not len(preview.mesh_segments(vertices, indices, Plane.xy()))
    assert not len(preview.mesh_segments(vertices, indices, Plane.from_normal((-1, 0, 0), (1, 0, 0))))
    assert not len(preview.mesh_segments(vertices, indices, Plane.from_normal((2, 0, 0), (1, 0, 0))))


def test_section_cache_survives_redraw_and_invalidates_geometry_plane_visibility(monkeypatch):
    item = SimpleNamespace(vertices=np.array([[-1, 0, 0], [1, 0, 0], [1, 2, 0]], dtype=np.float32), indices=np.array([[0, 1, 2]]))
    cache = preview.SectionPreview()
    original = preview.mesh_segments
    calls = []
    def count(*args):
        calls.append(1)
        return original(*args)
    monkeypatch.setattr(preview, 'mesh_segments', count)
    first = cache.segments({'body': item}, Plane.yz())['body']
    assert cache.segments({'body': item}, Plane.yz())['body'] is first
    assert len(calls) == 1
    item.vertices = item.vertices + [0, 0, 1]
    assert np.allclose(cache.segments({'body': item}, Plane.yz())['body'][:, 2], 1)
    cache.segments({'body': item}, Plane.xy())
    assert len(calls) == 3
    assert cache.segments({}, Plane.xy()) == {}
    assert cache.entries == {}


def test_preview_chunk_boundary():
    vertices = np.array([[-1, 0, 0], [1, 0, 0], [1, 2, 0]], dtype=np.float32)
    indices = np.tile([0, 1, 2], (65537, 1))
    points = preview.mesh_segments(vertices, indices, Plane.yz())
    assert points.shape == (2 * 65537, 3)
    assert np.isfinite(points).all()


def test_boundary_edges_weld_duplicate_vertices_and_omit_same_face_diagonals():
    from robocad.ui.viewport import _face_boundary_edges
    v = np.array([[0,0,0],[1,0,0],[0,1,0],[1,0,0],[0,1,0],[1,1,0]],dtype=np.float32)
    idx = np.array([[0,1,2],[3,5,4]],dtype=np.uint32)
    assert len(_face_boundary_edges(v,idx,np.array([0,0]))) == 0
    edges = _face_boundary_edges(v,idx,np.array([0,1]))
    assert len(edges) == 1
    assert {tuple(p) for p in v[edges[0]]} == {(1,0,0),(0,1,0)}
