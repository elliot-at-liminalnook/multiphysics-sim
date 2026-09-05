"""Document annotations shared by the editor and REST API.

Geometry edits never silently move a pin to another face: changed geometry is
marked for review. Known body transforms move the anchor in the same undo step.
"""
from copy import deepcopy
from datetime import datetime, timezone
import hashlib
import json
import math
import re

from .kernel import KernelError

PART_LINK = re.compile(r'\[([^\]\n]+)\]\(part:([A-Za-z0-9_-]+)\)')


def now():
    return datetime.now(timezone.utc).isoformat()


def text(value, name):
    if not isinstance(value, str) or not value.strip():
        raise KernelError(f"{name} must not be empty")
    if len(value) > 20000:
        raise KernelError(f"{name} is too long")
    return value.strip()


def stamp(doc, node_id, body=None):
    node = doc.nodes.get(node_id)
    if node is None:
        return None
    body = body or doc.resolved_body(node_id)
    if body is None:
        if node.mesh is not None:
            data = [node.mesh.vertices, node.mesh.triangles, node.transform.to_json()]
            return hashlib.sha256(json.dumps(data, sort_keys=True).encode()).hexdigest()
        return None
    # Geometry commands replace Body objects. Selection and comment edits do
    # not: retain their fingerprint instead of integrating every face again.
    cache = getattr(doc, '_annotation_stamp_cache', {})
    doc._annotation_stamp_cache = cache
    cached = cache.get(node_id)
    if cached is not None and cached[0] is body:
        return cached[1]
    # B-rep serialization includes OCCT's mutable cache/flag state, so hashing
    # its bytes falsely detaches pins after a query or an undo. Fingerprint
    # geometric properties instead; exclude transient face indices.
    props = doc.kernel.mass_properties(body)
    def rounded(value):
        if isinstance(value, float): return round(value, 9)
        if isinstance(value, dict): return {k: rounded(v) for k,v in value.items()}
        if isinstance(value, (list, tuple)): return [rounded(v) for v in value]
        return value
    faces = sorted(json.dumps(rounded(f.to_json()), sort_keys=True) for f in doc.kernel.faces(body))
    data = [rounded([props.volume, props.area, props.centroid, props.bbox_min, props.bbox_max]), faces]
    result = hashlib.sha256(json.dumps(data, sort_keys=True).encode()).hexdigest()
    cache[node_id] = (body, result)
    return result


def camera_view(view):
    if view is None: return {}
    if not isinstance(view, dict): raise KernelError("view must be an object")
    allowed = {"target", "distance", "yaw", "pitch", "fov", "orthographic", "mode", "rot"}
    if set(view) - allowed: raise KernelError("unsupported annotation camera field")
    for key, value in view.items():
        if key == "mode":
            if value not in ("turntable", "trackball"): raise KernelError("invalid camera mode")
        elif key == "rot":
            import numpy as np
            try:
                matrix = np.asarray(value, dtype=float)
                valid = matrix.shape == (3, 3) and np.isfinite(matrix).all() and np.allclose(matrix @ matrix.T, np.eye(3), atol=1e-5) and np.isclose(np.linalg.det(matrix), 1)
            except (ValueError, TypeError):
                valid = False
            if not valid: raise KernelError("camera rotation must be an orthonormal 3 by 3 matrix")
        elif key == "orthographic":
            if not isinstance(value, bool): raise KernelError("orthographic must be boolean")
        elif key == "target":
            if not isinstance(value, (list, tuple)) or len(value) != 3 or not all(isinstance(v,(float,int)) and math.isfinite(v) for v in value):
                raise KernelError("camera target needs three finite coordinates")
        elif not isinstance(value, (int,float)) or not math.isfinite(value):
            raise KernelError("camera values must be finite numbers")
        elif key == "distance" and value <= 0 or key == "fov" and not 1 <= value <= 170:
            raise KernelError("camera distance or field of view is out of range")
    return deepcopy(view)


def anchor(doc, node_id, point, face=None):
    if node_id not in doc.nodes:
        raise KernelError("annotation part does not exist")
    if not isinstance(point, (list, tuple)) or len(point) != 3 or not all(isinstance(x, (int, float)) and math.isfinite(x) for x in point):
        raise KernelError("anchor point must contain three finite millimetre coordinates")
    out = {"node_id": node_id, "point": list(point), "geometry": stamp(doc, node_id)}
    if face is not None:
        body = doc.resolved_body(node_id)
        faces = doc.kernel.faces(body) if body else []
        if not isinstance(face, int) or not 0 <= face < len(faces):
            raise KernelError("annotation face does not exist")
        out["face"] = faces[face].to_json()
    return out


def thread_detail(doc, thread, stamps=None):
    out = deepcopy(thread)
    a = thread["anchor"]
    node = doc.nodes.get(a["node_id"])
    out["node_name"] = node.name if node else ("Experiment evidence" if a['node_id'] is None else "Deleted part")
    if a['node_id'] is None:
        out['anchor_status'] = 'evidence'
    elif node is None:
        out["anchor_status"] = "missing"
    else:
        current = stamps.get(node.id) if stamps is not None else stamp(doc, node.id)
        out["anchor_status"] = "attached" if current == a["geometry"] else "needs_review"
    out['linked_parts'] = [dict(ref,
        name=doc.nodes[ref['node_id']].name if ref['node_id'] in doc.nodes else 'Deleted part',
        available=ref['node_id'] in doc.nodes) for ref in thread_parts(thread)]
    return out


def thread_parts(thread):
    """Older annotations implicitly refer to their anchor part."""
    if 'part_refs' in thread:
        refs = deepcopy(thread['part_refs'])
    else:
        ids = thread.get('evidence', {}).get('node_ids', [])
        nid = thread['anchor']['node_id']
        refs = [{'node_id': i} for i in ([nid] if nid else ids)]
    seen = {r['node_id'] for r in refs}
    for comment in thread['comments']:
        for label, nid in PART_LINK.findall(comment['body']):
            if nid not in seen:
                refs.append({'node_id': nid, 'label': label})
                seen.add(nid)
    return refs


def validate_part_refs(doc, refs, previous=()):
    if not isinstance(refs, list) or len(refs) > 200:
        raise KernelError('Linked parts must be a list of at most 200 parts')
    from .saved_views import validate_state
    retained = {r['node_id'] for r in previous}
    out, seen = [], set()
    for value in refs:
        ref = {'node_id': value} if isinstance(value, str) else deepcopy(value)
        if not isinstance(ref, dict) or set(ref) - {'node_id', 'label', 'description', 'view'}:
            raise KernelError('A linked part needs node_id, with optional label, description and view')
        nid = ref.get('node_id')
        if not isinstance(nid, str) or nid in seen:
            raise KernelError('Linked part IDs must be unique strings')
        if nid not in doc.nodes and nid not in retained:
            raise KernelError('Linked part does not exist: ' + nid)
        seen.add(nid)
        for key, limit in (('label', 120), ('description', 1000)):
            if key in ref and (not isinstance(ref[key], str) or len(ref[key]) > limit):
                raise KernelError(f'Part {key} must be text of at most {limit} characters')
        if 'view' in ref: ref['view'] = validate_state(ref['view'])
        out.append(ref)
    return out


def evidence_reference(value):
    if value is None: return None
    if not isinstance(value, dict) or set(value) - {'run_id', 'signal', 'time_range', 'source', 'physical_hash', 'node_ids'}:
        raise KernelError('Invalid experiment evidence reference')
    text(value.get('run_id'), 'Run ID')
    for key in ('signal', 'physical_hash'):
        if key in value: text(value[key], key)
    if 'node_ids' in value:
        if not isinstance(value['node_ids'], list): raise KernelError('Evidence node_ids must be an array')
        for node_id in value['node_ids']: text(node_id, 'CAD part ID')
    if 'time_range' in value:
        t = value['time_range']
        if not isinstance(t, (list, tuple)) or len(t) != 2 or not all(isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v) for v in t) or not 0 <= t[0] <= t[1]:
            raise KernelError('Evidence time range requires ordered nonnegative seconds')
    if 'source' in value:
        s = value['source']
        if not isinstance(s, dict) or set(s) - {'path', 'line', 'column'}:
            raise KernelError('Invalid script location')
        text(s.get('path'), 'Script path')
        for key in ('line', 'column'):
            if key in s and (type(s[key]) is not int or s[key] < 1):
                raise KernelError('Script line and column must be positive integers')
    return deepcopy(value)


class ChangeThreads:
    def __init__(self, label, changes):
        self.label, self.changes, self.previous = label, deepcopy(changes), None

    def apply(self, doc, changes):
        for tid, value in changes.items():
            if value is None:
                doc.annotations.pop(tid, None)
            else:
                doc.annotations[tid] = deepcopy(value)
        doc.dirty = True
        doc.notify("annotations")  # No geometry invalidation or retessellation.

    def do(self, doc):
        if self.previous is None:
            self.previous = {tid: deepcopy(doc.annotations.get(tid)) for tid in self.changes}
        self.apply(doc, self.changes)

    def undo(self, doc):
        self.apply(doc, self.previous)

    def redo(self, doc):
        self.apply(doc, self.changes)


class AnnotationOps:
    def threads(self, node_id=None, status=None, run_id=None):
        if status not in (None, "open", "resolved"):
            raise KernelError("status must be open or resolved")
        items = [t for t in self.doc.annotations.values()
                 if (node_id is None or t["anchor"]["node_id"] == node_id or node_id in t.get('evidence', {}).get('node_ids', []) or any(r['node_id'] == node_id for r in thread_parts(t)))
                 and (status is None or t["status"] == status)
                 and (run_id is None or t.get('evidence', {}).get('run_id') == run_id)]
        stamps = {t["anchor"]["node_id"]: None for t in items}
        for nid in stamps:
            stamps[nid] = stamp(self.doc, nid)
        return [thread_detail(self.doc, t, stamps) for t in items]

    def thread(self, thread_id):
        if thread_id not in self.doc.annotations:
            raise KeyError(thread_id)
        return thread_detail(self.doc, self.doc.annotations[thread_id])

    def create_thread(self, node_id=None, point=None, body='', author="You", face=None, view=None, evidence=None, part_refs=None, inspection_view=None):
        evidence = evidence_reference(evidence)
        if node_id is None and evidence is None:
            raise KernelError('An annotation requires a part or experiment evidence')
        a = anchor(self.doc, node_id, point, face) if node_id is not None else {'node_id': None, 'point': None, 'geometry': None}
        content, author = text(body, "Comment"), text(author, "Author")
        view = camera_view(view)
        tid, cid, ts = self.doc.new_id(), self.doc.new_id(), now()
        thread = {"id": tid, "anchor": a, "view": deepcopy(view or {}), "status": "open", "created_at": ts, "updated_at": ts,
                  "comments": [{"id": cid, "author": author, "body": content, "created_at": ts, "updated_at": ts}]}
        if evidence is not None: thread['evidence'] = evidence
        if part_refs is not None: thread['part_refs'] = validate_part_refs(self.doc, part_refs)
        if inspection_view is not None:
            from .saved_views import validate_state
            thread['inspection_view'] = validate_state(inspection_view)
        self.stack.push(ChangeThreads("Add annotation", {tid: thread}))
        return tid

    def update_thread(self, thread_id, status=None, node_id=None, point=None, face=None, view=None, evidence=None, part_refs=None, inspection_view=None):
        t = deepcopy(self.doc.annotations[thread_id])
        if status is not None:
            if status not in ("open", "resolved"):
                raise KernelError("status must be open or resolved")
            t["status"] = status
        if node_id is not None or point is not None:
            t["anchor"] = anchor(self.doc, node_id or t["anchor"]["node_id"], point if point is not None else t["anchor"]["point"], face)
        if view is not None:
            t["view"] = camera_view(view)
        if evidence is not None:
            t['evidence'] = evidence_reference(evidence)
        if part_refs is not None: t['part_refs'] = validate_part_refs(self.doc, part_refs, thread_parts(t))
        if inspection_view is not None:
            from .saved_views import validate_state
            t['inspection_view'] = validate_state(inspection_view)
        t["updated_at"] = now()
        self.stack.push(ChangeThreads("Update annotation", {thread_id: t}))
        return thread_id

    def delete_thread(self, thread_id):
        if thread_id not in self.doc.annotations:
            raise KeyError(thread_id)
        self.stack.push(ChangeThreads("Delete annotation", {thread_id: None}))

    def add_comment(self, thread_id, body, author="You"):
        t = deepcopy(self.doc.annotations[thread_id])
        ts, cid = now(), self.doc.new_id()
        t["comments"].append({"id": cid, "body": text(body, "Comment"), "author": text(author, "Author"), "created_at": ts, "updated_at": ts})
        t["updated_at"] = ts
        self.stack.push(ChangeThreads("Reply to annotation", {thread_id: t}))
        return cid

    def update_comment(self, comment_id, body):
        return self._change_comment(comment_id, text(body, "Comment"))

    def delete_comment(self, comment_id):
        return self._change_comment(comment_id, None)

    def _change_comment(self, comment_id, body):
        for tid, source in self.doc.annotations.items():
            if not any(c["id"] == comment_id for c in source["comments"]):
                continue
            t = deepcopy(source)
            c = next(c for c in t["comments"] if c["id"] == comment_id)
            if body is None:
                if len(t["comments"]) == 1:
                    raise KernelError("delete the thread to remove its last comment")
                t["comments"].remove(c)
            else:
                c["body"], c["updated_at"] = body, now()
            t["updated_at"] = now()
            self.stack.push(ChangeThreads("Delete comment" if body is None else "Edit comment", {tid: t}))
            return tid
        raise KeyError(comment_id)
