"""Approximate interactive cuts of display meshes; never call the CAD kernel."""

import numpy as np


def mesh_segments(vertices, indices, plane):
    """Return independent line endpoints. Coplanar triangles are omitted.

    Work in chunks to bound temporary memory even for large imported meshes.
    The result has the accuracy of the viewport tessellation, not the B-rep.
    """
    origin = np.asarray(plane.origin, dtype=np.float64)
    normal = np.asarray(plane.normal, dtype=np.float64)
    normal /= np.linalg.norm(normal)
    pieces = []
    for start in range(0, len(indices), 65536):
        triangles = vertices[indices[start:start + 65536]]
        distances = np.sum((triangles - origin) * normal, axis=2)
        crosses = (distances.min(axis=1) < 0) & (distances.max(axis=1) >= 0)
        triangles, distances = triangles[crosses], distances[crosses]
        if not len(triangles):
            continue
        endpoints = np.empty((len(triangles), 2, 3), dtype=np.float32)
        counts = np.zeros(len(triangles), dtype=np.int8)
        for a, b in ((0, 1), (1, 2), (2, 0)):
            rows = np.flatnonzero((distances[:, a] < 0) != (distances[:, b] < 0))
            t = distances[rows, a] / (distances[rows, a] - distances[rows, b])
            endpoints[rows, counts[rows]] = triangles[rows, a] + t[:, None] * (triangles[rows, b] - triangles[rows, a])
            counts[rows] += 1
        valid = (counts == 2) & (np.sum((endpoints[:, 0] - endpoints[:, 1]) ** 2, axis=1) > 1e-16)
        pieces.append(endpoints[valid].reshape(-1, 3))
    return np.ascontiguousarray(np.concatenate(pieces) if pieces else np.empty((0, 3)), dtype=np.float32)


class SectionPreview:
    """Cache by actual display arrays and plane, independent of camera/selection."""

    def __init__(self):
        self.key = None
        self.entries = {}

    def segments(self, items, plane):
        key = (tuple(plane.origin), tuple(plane.normal))
        if self.key != key:
            self.entries.clear()
            self.key = key
        current = {}
        for nid, item in items.items():
            old = self.entries.get(nid)
            if old is not None and old[0] is item.vertices and old[1] is item.indices:
                current[nid] = old
            else:
                current[nid] = (item.vertices, item.indices, mesh_segments(item.vertices, item.indices, plane))
        self.entries = current
        return {nid: entry[2] for nid, entry in current.items()}
