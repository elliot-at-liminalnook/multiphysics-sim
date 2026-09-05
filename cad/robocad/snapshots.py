"""Captured CAD inputs and stable content identities for experiments.

Creating a snapshot does not change the live path, dirty flag or undo history.
B-reps are serialized once per immutable body handle; physical derivation runs
later in a worker owning the captured document.
"""
import hashlib
import io
import json
import zipfile
from copy import deepcopy
from dataclasses import dataclass


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(value).hexdigest()


@dataclass(frozen=True)
class Snapshot:
    document_id: str
    revision: int
    physical_hash: str
    archive_hash: str
    data: bytes
    cad_derivation_hash: str


def capture(doc):
    with doc._lock:
        manifest = deepcopy(doc.to_manifest())
        manifest.pop('saved', None)
        # Results are independent artifacts; copying them can make snapshots
        # enormous and accidentally imply that they describe a new candidate.
        manifest['results'] = None
        for n in manifest['nodes']: n.pop('results', None)
        blobs = {}
        body_cache = {}
        for n in doc.nodes.values():
            if n.body is not None:
                cached = doc._snapshot_body_cache.get(n.id)
                data = cached[1] if cached and cached[0] is n.body else doc.kernel.serialize(n.body)
                body_cache[n.id] = (n.body, data)
                blobs[f'brep/{n.id}.brep'] = data
            if n.mesh is not None:
                import numpy as np
                buf = io.BytesIO()
                np.savez(buf, vertices=np.asarray(n.mesh.vertices,dtype=np.float32),
                    normals=np.asarray(n.mesh.normals,dtype=np.float32), triangles=np.asarray(n.mesh.triangles,dtype=np.int32),
                    triangle_face=np.asarray(n.mesh.triangle_face,dtype=np.int32))
                blobs[f'mesh/{n.id}.npz'] = buf.getvalue()
            if n.image and n.image.get('data'): blobs[f'image/{n.id}'] = n.image['data']
        doc._snapshot_body_cache = body_cache
        physical = physical_manifest(manifest)
        physical['geometry'] = {name:digest(data) for name,data in blobs.items() if name.startswith(('brep/','mesh/'))}
        physical_hash = digest(canonical(physical))
        cad_derivation_hash = digest(canonical({k: v for k, v in physical.items() if k != 'component_graph'}))
        blobs['manifest.json'] = canonical(manifest)
        buf = io.BytesIO()
        with zipfile.ZipFile(buf,'w',zipfile.ZIP_STORED) as archive:
            for name,data in sorted(blobs.items()):
                # Stable timestamps: recapturing the same state has the same hash.
                archive.writestr(zipfile.ZipInfo(name,date_time=(1980,1,1,0,0,0)),data)
        data = buf.getvalue()
        return Snapshot(doc.document_id,doc.revision,physical_hash,digest(data),data,cad_derivation_hash)


def physical_manifest(manifest):
    """Exclude review-only state; retain names because current export uses them."""
    physical_kinds = {'body','sheet','instance','mesh','joint','sensor','cable','plane'}
    nodes = []
    for n in manifest['nodes']:
        if n['kind'] not in physical_kinds: continue
        nodes.append({k:v for k,v in n.items() if k not in ('results','color','locked','visible','children','parent')})
    materials = [{k:v for k,v in m.items() if k not in ('color','roughness','metallic')} for m in manifest['materials']]
    return {'nodes':sorted(nodes,key=lambda n:n['id']), 'materials':sorted(materials,key=lambda m:m['id']), 'robot_settings':manifest['robot_settings'],
            'component_graph': manifest.get('component_graph', {'version': 1, 'components': {}, 'connections': {}})}
