"""Read-only captured geometry and replay contracts, shared by UI and REST.

The caller supplies an isolated document from Experiments.captured_document or
Candidates.document. Missing captured CAD is explicit, never filled from live CAD.
"""
import math
from .kernel import KernelError


def identity(record, kind):
    provenance = record.get('provenance', {})
    return {'document_id': record.get('document_id'), 'revision': record.get('revision'),
            'source_kind': kind, 'source_id': record['id'],
            'physical_hash': record.get('physical_hash', provenance.get('physical_hash')),
            'archive_hash': record.get('cad_archive_hash', provenance.get('cad_archive_hash'))}


def geometry(doc, record, kind):
    nodes = []
    if doc is not None:
        for node in doc.walk():
            if not doc.is_visible(node.id) or node.disabled: continue
            mesh = doc.mesh_of(node.id, .1)
            nodes.append({'id': node.id, 'name': node.name, 'source': node.source,
                'mesh': None if mesh is None else {'vertices': [[float(x) for x in v] for v in mesh.vertices],
                    'triangles': [[int(x) for x in t] for t in mesh.triangles], 'triangle_face': [int(x) for x in mesh.triangle_face],
                    'face_count': mesh.face_count}})
    return {'identity': identity(record, kind), 'units': 'mm', 'nodes': nodes,
            'missing_reason': 'Captured run has no CAD geometry' if doc is None else None,
            'provenance': record.get('provenance', {})}


def sample(result, record, seconds, scale=1.):
    from .experiment_results import sample_index, replay_matrices, replay_flex, signals, value_at
    if type(seconds) not in (int, float) or not math.isfinite(seconds): raise KernelError('Replay time must be finite')
    if type(scale) not in (int, float) or not math.isfinite(scale) or not 0 < scale <= 1000: raise KernelError('Flex scale must be finite in (0,1000]')
    times = result.get('trace', {}).get('t', [])
    if not times: raise KernelError('Captured result has no replay samples')
    index = sample_index(times, seconds)
    catalogue = signals(result)
    values = {key: value_at(series, times[index]) if series['t'] and series['t'][0] <= times[index] <= series['t'][-1] else None for key, series in catalogue.items()}
    return {'identity': identity(record, 'experiment'), 'index': index, 'time': times[index],
            'matrices': {nid: m.tolist() for nid, m in replay_matrices(result, index).items()}, 'flex': replay_flex(result, index, scale),
            'signals': catalogue, 'values': values}
