"""Atomic CAD edit batches and persistent candidates sharing the normal Ops.

All operations run against an isolated snapshot. A failed batch cannot mutate
the live document or its undo stack. Publishing swaps document state in one
undo step after checking the base revision again.
"""
from copy import deepcopy
import io
import json
from pathlib import Path
import time
import uuid

from .commands import Command, Ops
from .document import Document
from .experiments import RevisionConflict, write_json
from .kernel import KernelError
from .snapshots import capture, digest


# Deliberate document-edit surface. Export, script callbacks and other external
# effects cannot be staged and are not allowed inside an atomic transaction.
EDIT_OPS = frozenset('''delete rename set_visible set_locked set_disabled set_material
set_color set_pivot group move_node isolate show_all box box_center box_three_point
cylinder sphere new_sketch extrude revolve sweep pipe loft fill bridge push_pull
offset_faces offset_face_to move_faces rotate_faces set_radius set_diameter
set_distance set_angle draft delete_faces untrim imprint split_face boolean region
cut shell thicken fillet fillet_chordal fillet_all full_round remove_fillets chamfer
transform mirror instance make_unique array_rect array_radial array_curve join
unjoin extract_components dissolve project_curve silhouette set_control_points raise_degree rebuild_face
plane_from_face plane_three_points plane_two_points_camera plane_midplane add_measurement
clearance fastener_hole add_joint set_joint connect_fixed add_motor mount_motor
attach_motor set_ground infer_joints add_sensor add_cable set_robot_setting set_battery
set_control set_uncertainty set_material_props set_joint_physics create_thread update_thread
delete_thread add_comment update_comment delete_comment set_component_graph'''.split())
STATE_FIELDS = ('nodes', 'roots', 'materials', 'robot_settings', 'component_graph', 'annotations', 'active_group', '_snapshot_body_cache')


def check_revision(doc, expected):
    if type(expected) is not int or expected != doc.revision:
        raise RevisionConflict(f'Expected document revision {expected}; current revision is {doc.revision}. '
                               'Fetch the current document and rebuild the candidate before applying it.')


def state(doc):
    # Kernel handles are immutable values under Ops. Deep-copy mutable node,
    # material and annotation data, without trying to pickle OCCT objects.
    memo = {id(n.body): n.body for n in doc.nodes.values() if n.body is not None}
    return deepcopy({key: getattr(doc, key) for key in STATE_FIELDS}, memo)


class PublishState(Command):
    def __init__(self, doc, candidate, label):
        self.label = label
        self.before, self.after = state(doc), state(candidate)

    def apply(self, doc, value):
        memo = {id(n.body): n.body for n in value['nodes'].values() if n.body is not None}
        for key, item in deepcopy(value, memo).items(): setattr(doc, key, item)
        doc.mesh_cache.clear()
        doc.touch()

    def do(self, doc): self.apply(doc, self.after)
    def undo(self, doc): self.apply(doc, self.before)


def stage(snapshot, operations):
    from .api import ArgConverter
    if not isinstance(operations, list) or not operations:
        raise KernelError('An edit batch requires a nonempty operations array')
    doc = Document.load(io.BytesIO(snapshot.data)); doc.path = None
    ops = Ops(doc); converter = ArgConverter(doc); outputs = {}

    def resolve(value):
        if isinstance(value, dict):
            if set(value) == {'$ref'}:
                if value['$ref'] not in outputs: raise KernelError(f"Unknown prior operation reference {value['$ref']}")
                return deepcopy(outputs[value['$ref']])
            return {k: resolve(v) for k, v in value.items()}
        if isinstance(value, list): return [resolve(v) for v in value]
        return value

    for index, operation in enumerate(operations):
        if not isinstance(operation, dict) or set(operation) - {'op', 'args', 'kwargs', 'as'}:
            raise KernelError(f'Operation {index}: expected op, args, kwargs and optional as')
        name = operation.get('op')
        if name not in EDIT_OPS: raise KernelError(f'Operation {index}: {name} is not an atomic document edit')
        alias = operation.get('as', str(index))
        if not isinstance(alias, str) or alias in outputs: raise KernelError(f'Operation {index}: duplicate or invalid result alias')
        try:
            fn = getattr(ops, name)
            args, kwargs = converter.convert(fn, resolve(operation.get('args', [])), resolve(operation.get('kwargs', {})))
            outputs[alias] = fn(*args, **kwargs)
        except Exception as error:
            raise KernelError(f'Operation {index} ({name}): {error}') from error
    return doc, outputs


def change_set(before, after):
    def contents(snapshot):
        import zipfile
        with zipfile.ZipFile(io.BytesIO(snapshot.data)) as z:
            manifest = json.loads(z.read('manifest.json'))
            geometry = {name: digest(z.read(name)) for name in z.namelist() if name.startswith(('brep/', 'mesh/'))}
        return manifest, geometry
    a, ga = contents(before); b, gb = contents(after)
    nodes_a = {n['id']: n for n in a['nodes']}; nodes_b = {n['id']: n for n in b['nodes']}
    changes = []
    for nid in sorted(nodes_a.keys() | nodes_b.keys()):
        x, y = nodes_a.get(nid), nodes_b.get(nid)
        geometry = any(ga.get(f'{prefix}/{nid}{ext}') != gb.get(f'{prefix}/{nid}{ext}') for prefix, ext in [('brep', '.brep'), ('mesh', '.npz')])
        if x != y or geometry:
            changes.append({'id': nid, 'name': (y or x)['name'], 'kind': 'added' if x is None else 'removed' if y is None else 'modified',
                            'geometry_changed': geometry, 'before': x, 'after': y})
    return {'nodes': changes, 'document': {key: {'before': a.get(key), 'after': b.get(key)}
            for key in ('materials', 'robot_settings', 'component_graph', 'annotations', 'roots', 'active_group') if a.get(key) != b.get(key)},
            'physical_changed': before.physical_hash != after.physical_hash}


class Candidates:
    def __init__(self, doc, ops, root):
        self.doc, self.ops, self.root = doc, ops, Path(root)

    def batch(self, request):
        with self.doc._lock:
            check_revision(self.doc, request.get('expected_revision'))
            before = capture(self.doc)
        staged, outputs = stage(before, request.get('operations'))
        after = capture(staged)
        changes = change_set(before, after)
        with self.doc._lock:
            check_revision(self.doc, before.revision)
            self.ops.stack.push(PublishState(self.doc, staged, request.get('label', 'Atomic edit batch')))
        return {'revision': self.doc.revision, 'results': outputs, 'changes': changes}

    def create(self, request):
        with self.doc._lock:
            check_revision(self.doc, request.get('expected_revision'))
            before = capture(self.doc)
        staged, outputs = stage(before, request.get('operations'))
        snapshot = capture(staged)
        candidate_id = uuid.uuid4().hex
        folder = self.root/candidate_id; folder.mkdir(parents=True)
        (folder/'base.rcad').write_bytes(before.data); (folder/'candidate.rcad').write_bytes(snapshot.data)
        record = {'id': candidate_id, 'document_id': self.doc.document_id, 'base_revision': before.revision,
                  'revision': snapshot.revision, 'label': request.get('label', f'Candidate {candidate_id[:8]}'),
                  'state': 'draft', 'created_at': time.time(), 'operations': request['operations'], 'results': outputs,
                  'physical_hash': snapshot.physical_hash, 'changes': change_set(before, snapshot)}
        write_json(folder/'candidate.json', record)
        return record

    def get(self, candidate_id):
        if not isinstance(candidate_id, str) or len(candidate_id) != 32 or any(c not in '0123456789abcdef' for c in candidate_id):
            raise KeyError(candidate_id)
        path = self.root/candidate_id/'candidate.json'
        if not path.exists(): raise KeyError(candidate_id)
        record = json.loads(path.read_text())
        if record['document_id'] != self.doc.document_id: raise KeyError(candidate_id)
        return record

    def list(self):
        records = []
        for path in self.root.glob('*/candidate.json'):
            try: records.append(self.get(path.parent.name))
            except (KeyError, ValueError, OSError): pass
        return sorted(records, key=lambda record: record['created_at'], reverse=True)

    def document(self, candidate_id):
        self.get(candidate_id)
        doc = Document.load(str(self.root/candidate_id/'candidate.rcad')); doc.path = None
        return doc

    def accept(self, candidate_id, expected_revision):
        record = self.get(candidate_id)
        if record['state'] != 'draft': raise KernelError('Only a draft candidate can be accepted')
        staged = self.document(candidate_id)
        with self.doc._lock:
            check_revision(self.doc, expected_revision)
            check_revision(self.doc, record['base_revision'])
            self.ops.stack.push(PublishState(self.doc, staged, f"Accept {record['label']}"))
            record.update(state='accepted', accepted_revision=self.doc.revision)
            write_json(self.root/candidate_id/'candidate.json', record)
        return record

    def discard(self, candidate_id):
        record = self.get(candidate_id)
        if record['state'] != 'draft': raise KernelError('Only a draft candidate can be discarded')
        record['state'] = 'discarded'
        write_json(self.root/candidate_id/'candidate.json', record)
        return record
