"""Reusable CAD assemblies, explicit parameters, and independent occurrences.

Definitions own geometry. Occurrence nodes are derived ordinary CAD bodies and
joints, so every existing physics/export path sees the same materialized model.
"""
from __future__ import annotations

from copy import copy, deepcopy
from dataclasses import dataclass, field
import hashlib
import json
import math
import uuid
from typing import Optional

import numpy as np

from .component_parameters import FEATURES, expression, feature_arguments, finite, validate_parameters
from .document import Node, Material, Transform
from .kernel import KernelError
from .robotics import Joint

SCHEMA_VERSION = 1
SUPPORTED_NODES = {'body', 'sheet', 'curve', 'group', 'joint', 'sensor', 'cable'}
POINT_FIELDS = {'mount_point', 'shaft_tip', 'point', 'point_mm', 'com_mm', 'pivot', 'origin_mm', 'axis_point'}
DIRECTION_FIELDS = {'shaft_axis', 'axis', 'axis_dir', 'normal'}


def clone_node(node):
    result = copy(node)
    for field in ('children', 'transform', 'joint', 'robot', 'results', 'component_instance', 'component_member'):
        setattr(result, field, deepcopy(getattr(node, field)))
    return result


def placement_matrix(placement):
    if not isinstance(placement, Transform):
        placement = Transform.from_json(placement or {})
    for vector in (placement.translation, placement.axis):
        if len(vector) != 3: raise KernelError('Component placement requires three coordinates')
        for value in vector: finite(value)
    finite(placement.angle_deg)
    if finite(placement.scale) != 1:
        raise KernelError('Component placement is rigid; expose dimensions as parameters instead of scaling physics')
    if np.linalg.norm(placement.axis) < 1e-12:
        raise KernelError('Component rotation axis must be nonzero')
    return placement, np.asarray(placement.matrix())


def matrix_placement(matrix):
    rotation = matrix[:3, :3]
    angle = math.acos(float(np.clip((np.trace(rotation)-1)/2, -1, 1)))
    if abs(angle) < 1e-10: axis = np.array([0.,0.,1.])
    elif abs(math.pi-angle) < 1e-7:
        values, vectors = np.linalg.eigh((rotation+rotation.T)/2); axis = vectors[:, np.argmax(values)]
    else: axis = np.array([rotation[2,1]-rotation[1,2],rotation[0,2]-rotation[2,0],rotation[1,0]-rotation[0,1]])/(2*math.sin(angle))
    return Transform(tuple(map(float, matrix[:3,3])), tuple(map(float, axis)), math.degrees(angle))


def dependencies(definition):
    return {n.component_instance['definition_id'] for n in definition.nodes.values() if n.component_instance} | {v['definition_id'] for v in definition.variants.values()}


def validate_library(definitions):
    visited = set()
    def visit(key, ancestors):
        if key in ancestors: raise KernelError('Circular component dependency')
        if key not in definitions: raise KernelError(f'Missing nested component definition: {key}')
        if key in visited: return
        validate_definition(definitions[key])
        for child in dependencies(definitions[key]): visit(child, ancestors | {key})
        family = definitions[key]
        for variant in family.variants.values():
            target = definitions[variant['definition_id']]
            if target.variants: raise KernelError('A family variant must be an assembly definition, which may contain nested families')
            if set(target.ports) != set(family.ports) or any(target.ports[p]['kind'] != family.ports[p]['kind'] for p in family.ports):
                raise KernelError('Family variants must expose the same typed connection ports')
            _, quantities = validate_parameters(family.parameters)
            mapped = {}
            for name, value in variant.get('parameter_bindings', {}).items():
                if name not in target.parameters: raise KernelError('Family mapping targets a missing variant parameter')
                mapped[name] = expression(value, target.parameters[name]['unit'], quantities)
            validate_parameters(target.parameters, mapped)
        visited.add(key)
    for key in definitions: visit(key, set())


def remap(value, identities):
    if isinstance(value, str): return identities.get(value, value)
    if isinstance(value, dict): return {identities.get(k, k): remap(v, identities) for k, v in value.items()}
    if isinstance(value, list): return [remap(v, identities) for v in value]
    if isinstance(value, tuple): return tuple(remap(v, identities) for v in value)
    return deepcopy(value)


def physical_transform(value, matrix):
    if not isinstance(value, dict): return deepcopy(value)
    result = {}
    rotation = matrix[:3, :3]
    for key, item in value.items():
        if key in POINT_FIELDS | DIRECTION_FIELDS and isinstance(item, (list, tuple)) and len(item) == 3:
            vector = rotation @ np.asarray(item, dtype=float)
            if key in POINT_FIELDS: vector += matrix[:3, 3]
            result[key] = vector.tolist()
        elif key == 'inertia_kg_m2':
            inertia = np.asarray(item, dtype=float)
            if inertia.shape != (3, 3): raise KernelError('Inertia must be a 3 by 3 tensor')
            result[key] = (rotation @ inertia @ rotation.T).tolist()
        elif key == 'axes' and isinstance(item, list):
            result[key] = (np.asarray(item, dtype=float) @ rotation.T).tolist()
        elif isinstance(item, dict): result[key] = physical_transform(item, matrix)
        elif isinstance(item, list): result[key] = [physical_transform(v, matrix) if isinstance(v, dict) else deepcopy(v) for v in item]
        else: result[key] = deepcopy(item)
    return result


def transform_node(node, kernel, placement):
    placement, matrix = placement_matrix(placement)
    n = clone_node(node)
    if n.body is not None and (any(placement.translation) or placement.angle_deg):
        n.body = placement.apply(kernel, n.body)
    if n.joint:
        n.joint.pivot = tuple((matrix[:3, :3] @ n.joint.pivot + matrix[:3, 3]).tolist())
        n.joint.axis = tuple((matrix[:3, :3] @ n.joint.axis).tolist())
    if n.pivot is not None:
        n.pivot = tuple((matrix[:3, :3] @ n.pivot + matrix[:3, 3]).tolist())
    if n.robot is not None: n.robot = physical_transform(n.robot, matrix)
    if n.component_instance:
        _, child = placement_matrix(n.component_instance['placement'])
        n.component_instance['placement'] = matrix_placement(matrix @ child).to_json()
    return n


def referenced_nodes(value, known):
    if isinstance(value, str): return {value} if value in known else set()
    if isinstance(value, dict):
        return set().union(*(referenced_nodes(v, known) for v in list(value.values()) + list(value.keys()))) if value else set()
    if isinstance(value, (list, tuple)):
        return set().union(*(referenced_nodes(v, known) for v in value)) if value else set()
    return set()


@dataclass
class ComponentDefinition:
    id: str
    name: str
    nodes: dict[str, Node]
    roots: list[str]
    materials: dict[str, Material]
    revision: int = 1
    description: str = ''
    parameters: dict = field(default_factory=dict)
    features: list = field(default_factory=list)
    ports: dict = field(default_factory=dict)
    provenance: dict = field(default_factory=dict)
    source_bytes: dict = field(default_factory=dict, repr=False)
    variants: dict = field(default_factory=dict)
    default_variant: Optional[str] = None

    def descriptor(self):
        return {'id': self.id, 'name': self.name, 'revision': self.revision,
                'description': self.description, 'parameters': deepcopy(self.parameters),
                'variants': deepcopy(self.variants), 'default_variant': self.default_variant,
                'features': deepcopy(self.features), 'ports': deepcopy(self.ports),
                'nested': {nid: {'definition_id': n.component_instance['definition_id'],
                    'parameter_bindings': deepcopy(n.component_instance.get('parameter_bindings', {})),
                    'overrides': deepcopy(n.component_instance.get('overrides', {}))}
                    for nid,n in self.nodes.items() if n.component_instance and not n.component_member},
                'provenance': deepcopy(self.provenance), 'frame': {'length_unit': 'mm', 'angle_unit': 'deg', 'axes': 'right-handed XYZ'},
                'node_count': len(self.nodes), 'dependencies': sorted(dependencies(self))}

    def manifest(self):
        return {**self.descriptor(), 'version': SCHEMA_VERSION,
                'nodes': [{k:v for k,v in n.to_json().items() if not (n.component_member and k == 'body_kind')} for n in self.nodes.values()], 'roots': list(self.roots),
                'materials': [m.to_json() for m in self.materials.values()]}

    def archive_entries(self, kernel, prefix):
        out = []
        for nid, node in self.nodes.items():
            if node.body is None or node.component_member: continue
            cached = self.source_bytes.get(nid)
            if cached is None or cached[0] is not node.body:
                cached = (node.body, kernel.serialize(node.body))
                self.source_bytes[nid] = cached
            out.append((f'{prefix}/{nid}.brep', cached[1]))
        return out

    @classmethod
    def from_archive(cls, manifest, archive, kernel, prefix, progress=None):
        if manifest.get('version') != SCHEMA_VERSION: raise KernelError('Unsupported component definition version')
        nodes, captured = {}, {}
        for index, data in enumerate(manifest['nodes']):
            if progress: progress(index, len(manifest['nodes']), data['name'])
            n = Node(data['id'], data['kind'], data['name'], parent=data.get('parent'), children=list(data.get('children', [])),
                     visible=data.get('visible', True), locked=data.get('locked', False), material=data.get('material'),
                     color=tuple(data['color']) if data.get('color') else None, disabled=data.get('disabled', False))
            n.component_instance = deepcopy(data.get('component_instance'))
            n.component_member = deepcopy(data.get('component_member'))
            if data.get('body_kind') and not n.component_member:
                payload = archive.read(f'{prefix}/{n.id}.brep')
                n.body = kernel.deserialize(payload, data['body_kind']); captured[n.id] = (n.body, payload)
            n.joint = Joint.from_json(data['joint']) if data.get('joint') else None
            n.robot = deepcopy(data.get('robot'))
            n.pivot = tuple(data['pivot']) if data.get('pivot') else None
            n.tessellation_tolerance = data.get('tessellation_tolerance', .05)
            nodes[n.id] = n
        definition = cls(manifest['id'], manifest['name'], nodes, list(manifest['roots']),
                         {m['id']: Material.from_json(m) for m in manifest['materials']},
                         manifest['revision'], manifest.get('description', ''), deepcopy(manifest.get('parameters', {})),
                         deepcopy(manifest.get('features', [])), deepcopy(manifest.get('ports', {})),
                         deepcopy(manifest.get('provenance', {})), captured, deepcopy(manifest.get('variants', {})), manifest.get('default_variant'))
        validate_definition(definition)
        if progress: progress(len(manifest['nodes']), len(manifest['nodes']), '')
        return definition


def validate_definition(definition):
    if not isinstance(definition.id, str) or not definition.id or not isinstance(definition.name, str) or not definition.name.strip():
        raise KernelError('A component requires an ID and name')
    _, quantities = validate_parameters(definition.parameters)
    if not isinstance(definition.variants, dict): raise KernelError('Component variants must be an object')
    if definition.variants:
        if definition.nodes or definition.roots or definition.features: raise KernelError('A component family owns parameter mappings, not duplicate geometry')
        if definition.default_variant not in definition.variants: raise KernelError('Family default variant is missing')
        for name, variant in definition.variants.items():
            if not isinstance(name, str) or not name or not isinstance(variant, dict) or set(variant) - {'definition_id','parameter_bindings'} or not isinstance(variant.get('definition_id'), str):
                raise KernelError('Invalid family variant definition')
    elif definition.default_variant is not None: raise KernelError('Default variant requires a component family')
    known = set(definition.nodes)
    if len(definition.roots) != len(set(definition.roots)) or set(definition.roots) != {n.id for n in definition.nodes.values() if n.parent is None}:
        raise KernelError('Component roots do not match its hierarchy')
    visited = set()
    def walk(nid, ancestors):
        if nid not in known or nid in ancestors or nid in visited:
            raise KernelError('Component hierarchy has a cycle or repeated/missing node')
        visited.add(nid)
        node = definition.nodes[nid]
        if node.kind not in SUPPORTED_NODES:
            raise KernelError(f'Unsupported component node: {node.name}')
        for child in node.children:
            if child not in known or definition.nodes[child].parent != nid:
                raise KernelError('Component parent/child identity mismatch')
            walk(child, ancestors | {nid})
    for root in definition.roots: walk(root, set())
    if visited != known: raise KernelError('Component contains unreachable nodes')
    if not isinstance(definition.ports, dict): raise KernelError('Component ports must be an object')
    for name, port in definition.ports.items():
        if not isinstance(name, str) or not name or not isinstance(port, dict) or set(port) != {'source_id', 'kind', 'label'}:
            raise KernelError('Invalid component external port')
        if port['source_id'] in known or not isinstance(port['source_id'], str):
            raise KernelError('External ports must refer outside the component')
    external = {spec['source_id'] for spec in definition.ports.values()}
    if len(external) != len(definition.ports): raise KernelError('Duplicate component external port')
    for node in definition.nodes.values():
        if node.material is not None and node.material not in definition.materials:
            raise KernelError(f'{node.name}: missing material')
        if node.joint:
            for ref in (node.joint.parent, node.joint.child, node.joint.motor):
                if ref is not None and ref not in known | external:
                    raise KernelError(f'{node.name}: missing joint reference {ref}')
    for feature in definition.features:
        if feature.get('node') in known and definition.nodes[feature['node']].component_member:
            raise KernelError('Map parameters to the nested component instead of editing its derived members')
        if feature.get('node') not in known and not (feature.get('kind') == 'assembly_placement' and feature.get('node') == '*'): raise KernelError('Component feature targets a missing node')
        feature_arguments(feature, quantities)


def capture_definition(doc, ids, name, origin=(0., 0., 0.)):
    selected = set()
    def include(nid):
        if nid not in doc.nodes: raise KernelError(f'Missing component selection: {nid}')
        selected.add(nid)
        for child in doc.nodes[nid].children: include(child)
    if not ids: raise KernelError('Select parts or a group to create a component')
    for nid in ids:
        n = doc.nodes[nid]
        if len(ids) == 1 and n.kind == 'group' and not n.component_instance:
            for child in n.children: include(child)
        else: include(nid)
    for nid in selected:
        member = doc.nodes[nid].component_member
        if member and member['instance_id'] not in selected:
            raise KernelError('Capture the whole linked occurrence, not individual linked members')
    bodies = {nid for nid in selected if doc.nodes[nid].body is not None}
    for n in doc.nodes.values():
        if n.joint and n.joint.child in bodies: selected.add(n.id)
    if any(c.get('body_id') in selected for c in doc.component_graph.get('components', {}).values()):
        raise KernelError('This selection has native system graph bindings; component capture must include those bindings before it can be reused')
    if not bodies: raise KernelError('A component requires at least one geometric body')
    nodes, captured, external = {}, {}, set()
    placement = Transform(tuple(-finite(v) for v in origin))
    for nid in selected:
        n = transform_node(doc.nodes[nid], doc.kernel, placement)
        n.parent = n.parent if n.parent in selected else None
        n.children = [i for i in n.children if i in selected]
        n.results = None
        nodes[nid] = n
        if n.body is doc.nodes[nid].body and nid in doc._snapshot_body_cache:
            captured[nid] = doc._snapshot_body_cache[nid]
        external |= referenced_nodes(n.robot, doc.nodes) - selected
        if n.joint: external |= {r for r in (n.joint.parent, n.joint.child, n.joint.motor) if r is not None and r not in selected}
    ports = {}
    for index, nid in enumerate(sorted(external)):
        if nid not in doc.nodes: raise KernelError(f'Component has a broken external reference: {nid}')
        ports[f'connection_{index+1}'] = {'source_id': nid, 'kind': doc.nodes[nid].kind, 'label': doc.nodes[nid].name}
    definition = ComponentDefinition(uuid.uuid4().hex, name, nodes, [n.id for n in nodes.values() if n.parent is None],
                                     deepcopy(doc.materials), ports=ports,
                                     provenance={'document_id': doc.document_id, 'revision': doc.revision,
                                                 'path': doc.path, 'origin_mm': list(origin), 'source': 'captured CAD'},
                                     source_bytes=captured)
    validate_definition(definition)
    return definition


def structure(definition, definitions, variant=None):
    if not definition.variants:
        if variant is not None: raise KernelError('This component has no named variants')
        return definition
    choice = variant or definition.default_variant
    if choice not in definition.variants: raise KernelError(f'Unknown component variant: {choice}')
    return definitions[definition.variants[choice]['definition_id']]


def variant_nodes(definition, kernel, overrides=None, definitions=None, nested_overrides=None, variants=None, variant=None):
    values, quantities = validate_parameters(definition.parameters, overrides)
    if definition.variants:
        selected = definition.variants.get(variant or definition.default_variant)
        if selected is None: raise KernelError('Unknown component variant')
        target = structure(definition, definitions, variant)
        mapped = {key: expression(value, target.parameters[key]['unit'], quantities) for key,value in selected.get('parameter_bindings', {}).items()}
        return values, variant_nodes(target, kernel, mapped, definitions, nested_overrides, variants)[1]
    nodes = {nid: clone_node(n) for nid, n in definition.nodes.items()}
    for feature in definition.features:
        args = feature_arguments(feature, quantities)
        kind = feature['kind']
        if kind == 'assembly_placement':
            for nid, original in list(nodes.items()):
                if not original.component_member: nodes[nid] = transform_node(original, kernel, Transform(**args))
            continue
        node = nodes[feature['node']]
        if kind in ('box', 'cylinder'):
            if node.kind not in ('body', 'sheet'):
                raise KernelError('Geometry generation must target a body')
            if (node.robot or {}).get('mass_properties') or (node.robot or {}).get('solid_materials'):
                raise KernelError(f'{node.name}: redefine its mass/material overrides before regenerating geometry')
            node.body = getattr(kernel, kind)(**args)
        elif kind == 'placement':
            if node.kind == 'group' and not node.component_instance:
                raise KernelError('A placement feature must target a part, joint, sensor, cable or nested occurrence')
            nodes[node.id] = transform_node(node, kernel, Transform(**args))
        elif kind == 'joint_ratio':
            if node.joint is None or args['ratio'] <= 0: raise KernelError('Joint ratio requires a joint and a positive ratio')
            node.joint.gear_ratio = args['ratio']
        elif kind == 'joint_home':
            if node.joint is None: raise KernelError('Joint home requires a joint')
            node.joint.home = math.radians(args['angle_deg'])
        elif kind == 'joint_frame':
            if node.joint is None: raise KernelError('Joint-frame feature requires a joint')
            if np.linalg.norm(args['axis']) < 1e-12: raise KernelError('Joint axis must be nonzero')
            node.joint.pivot, node.joint.axis = args['pivot'], args['axis']
    # Regenerate children after evaluating parent features and mapped parameters.
    from .document import Document
    definitions = definitions or {definition.id: definition}
    nested_overrides = nested_overrides or {}
    local = Document(kernel); local.nodes = dict(nodes); local.component_definitions = definitions
    for port in definition.ports.values():
        local.nodes[port['source_id']] = Node(port['source_id'], port['kind'], port['label'])
    for original in list(nodes.values()):
        if not original.component_instance or original.component_member: continue
        child = clone_node(original); spec = child.component_instance
        target = definitions[spec['definition_id']]
        child_values = deepcopy(spec.get('overrides', {}))
        for name, value in spec.get('parameter_bindings', {}).items():
            if name not in target.parameters: raise KernelError(f'Unknown child parameter: {name}')
            child_values[name] = expression(value, target.parameters[name]['unit'], quantities)
        spec['inherited_parameters'] = validate_parameters(target.parameters, child_values)[0]
        child_values.update(nested_overrides.get(child.id, {}))
        spec['overrides'] = child_values; spec['revision'] = target.revision
        spec['nested_overrides'] = {**spec.get('nested_overrides', {}), **{source: nested_overrides[member] for source, member in spec['node_map'].items() if member in nested_overrides}}
        nodes[child.id] = child
        nodes.update(materialize(local, child, target, variants))
        local.nodes.update(nodes)
    return values, nodes


def materialize(doc, root, definition=None, variants=None):
    spec = root.component_instance
    definition = definition or doc.component_definitions[spec['definition_id']]
    validate_definition(definition)
    values, _ = validate_parameters(definition.parameters, spec.get('overrides'))
    key = (definition.id, definition.revision, json.dumps(values, sort_keys=True), json.dumps(spec.get('nested_overrides', {}), sort_keys=True), spec.get('variant'))
    variants = variants if variants is not None else {}
    if key not in variants: variants[key] = variant_nodes(definition, doc.kernel, spec.get('overrides'), doc.component_definitions, spec.get('nested_overrides'), variants, spec.get('variant'))[1]
    source = variants[key]
    identities = dict(spec['node_map'])
    if set(identities) != set(source) or len(set(identities.values())) != len(identities):
        raise KernelError('Occurrence node identities do not match its component definition')
    bindings = spec.get('bindings', {})
    if set(bindings) != set(definition.ports):
        raise KernelError(f'Connect all component ports: {sorted(definition.ports)}')
    for port, value in bindings.items():
        declared = structure(definition, doc.component_definitions, spec.get('variant')).ports[port]
        if value not in doc.nodes or doc.nodes[value].kind != declared['kind']:
            raise KernelError(f'{port} requires an existing {declared["kind"]}')
        identities[declared['source_id']] = value
    placement, _ = placement_matrix(spec.get('placement'))
    members = {}
    for index, (nid, node) in enumerate(source.items()):
        getattr(doc, '_component_progress', lambda *args: None)('Rebuilding ' + root.name, index, len(source), node.name)
        n = transform_node(node, doc.kernel, placement)
        n.id = identities[nid]
        n.parent = identities[node.parent] if node.parent else root.id
        n.children = [identities[i] for i in node.children]
        n.name = f'{root.name} / {node.name}'
        n.robot = remap(n.robot, identities)
        if n.joint: n.joint = Joint.from_json(remap(n.joint.to_json(), identities))
        if node.component_member:
            n.component_member = {'instance_id': identities[node.component_member['instance_id']], 'source_node': node.component_member['source_node']}
        else: n.component_member = {'instance_id': root.id, 'source_node': nid}
        if n.component_instance:
            n.component_instance['node_map'] = {key: identities[value] for key, value in node.component_instance['node_map'].items()}
            n.component_instance['bindings'] = {key: identities.get(value, value) for key, value in node.component_instance.get('bindings', {}).items()}

        n.locked = True
        n.results = None
        previous = doc.nodes.get(n.id)
        if previous is not None and previous.component_member == n.component_member:
            n.name, n.visible, n.color = previous.name, previous.visible, previous.color
        members[n.id] = n
    return members


def restore_occurrences(doc):
    validate_library(doc.component_definitions)
    variants = {}; claimed = set()
    source_bytes = {id(body): payload for definition in doc.component_definitions.values() for body,payload in definition.source_bytes.values()}
    for root in list(doc.nodes.values()):
        if not root.component_instance or root.component_member: continue
        spec = root.component_instance
        if spec['definition_id'] not in doc.component_definitions:
            raise KernelError(f'Missing component definition for {root.name}')
        if spec.get('revision') != doc.component_definitions[spec['definition_id']].revision:
            raise KernelError('Occurrence revision does not match its embedded definition')
        members = materialize(doc, root, variants=variants)
        if claimed & set(members): raise KernelError('Occurrences share member identities')
        claimed.update(members)
        if root.children != [spec['node_map'][i] for i in structure(doc.component_definitions[spec['definition_id']], doc.component_definitions, spec.get('variant')).roots]:
            raise KernelError('Occurrence hierarchy differs from its definition')
        for nid, derived in members.items():
            saved = doc.nodes.get(nid)
            if saved is None or saved.component_member != derived.component_member:
                raise KernelError(f'Missing occurrence member: {nid}')
            # Retain visibility/selection organization; geometry and physics are
            # authoritative from the pinned embedded definition and overrides.
            saved.body = derived.body
            if saved.body is not None and id(saved.body) in source_bytes:
                doc._snapshot_body_cache[nid] = (saved.body, source_bytes[id(saved.body)])
            saved.joint = derived.joint
            saved.robot = derived.robot
            saved.component_instance = derived.component_instance
            if saved.parent != derived.parent or saved.children != derived.children:
                raise KernelError('Occurrence member hierarchy differs from its definition')
    if claimed != {n.id for n in doc.nodes.values() if n.component_member}:
        raise KernelError('Orphan component members in document')


class ComponentChange:
    """One atomic undoable swap, prepared before touching the live document."""
    def __init__(self, doc, label, definitions, nodes, roots, materials=None):
        self.label = label
        self.before_meshes = dict(doc.mesh_cache)
        self.before = (doc.component_definitions, doc.nodes, doc.roots, doc.materials)
        self.after = (definitions, nodes, roots, materials if materials is not None else doc.materials)

    def apply(self, doc, state):
        doc.component_definitions, doc.nodes, doc.roots, doc.materials = state
        doc.mesh_cache.clear()
        if state is self.before: doc.mesh_cache.update(self.before_meshes)
        elif hasattr(self, 'prepared_meshes'): doc.mesh_cache.update(self.prepared_meshes)
        doc.touch()
        display = getattr(self, 'before_display' if state is self.before else 'after_display', None)
        if display is not None: doc.notify('component_prepared', display)

    def do(self, doc): self.apply(doc, self.after)
    def undo(self, doc): self.apply(doc, self.before)
    def redo(self, doc): self.do(doc)


class ComponentOps:
    def transform_components(self, ids, translation, axis, angle_deg, center=None, scale=1.):
        delta, matrix = placement_matrix(Transform(tuple(translation), tuple(axis), angle_deg, scale))
        rotation = matrix[:3, :3]
        nodes = dict(self.doc.nodes); variants = {}
        for nid in ids:
            root = clone_node(self.doc.nodes[nid])
            if root.locked: raise KernelError('Component occurrence is locked')
            old, old_matrix = placement_matrix(root.component_instance['placement'])
            pivot = np.asarray(center if center is not None else old.translation)
            position = rotation @ (np.asarray(old.translation)-pivot) + pivot + np.asarray(translation)
            combined = rotation @ old_matrix[:3, :3]
            angle = math.acos(float(np.clip((np.trace(combined)-1)/2, -1, 1)))
            if abs(angle) < 1e-10:
                direction = np.asarray([0., 0., 1.])
            elif abs(math.pi-angle) < 1e-7:
                eigenvalues, vectors = np.linalg.eigh((combined+combined.T)/2)
                direction = vectors[:, np.argmax(eigenvalues)]
            else:
                direction = np.asarray([combined[2,1]-combined[1,2], combined[0,2]-combined[2,0], combined[1,0]-combined[0,1]])/(2*math.sin(angle))
            root.component_instance['placement'] = Transform(tuple(map(float, position)), tuple(map(float, direction)), math.degrees(angle)).to_json()
            nodes[nid] = root
            nodes.update(materialize(self.doc, root, variants=variants))
        self.stack.push(ComponentChange(self.doc, 'Transform components', self.doc.component_definitions, nodes, self.doc.roots))
        return list(ids)

    def make_component(self, ids: list[str], name: str, origin: tuple = (0., 0., 0.)):
        """Capture and replace selection with a linked occurrence, preserving IDs."""
        definition = capture_definition(self.doc, ids, name, origin)
        nodes = dict(self.doc.nodes)
        original = self.doc.nodes[ids[0]] if len(ids) == 1 and self.doc.nodes[ids[0]].kind == 'group' and not self.doc.nodes[ids[0]].component_instance else None
        if original and original.component_member:
            raise KernelError('Selection is already linked; detach before capturing')
        root = clone_node(original) if original else Node(self.doc.new_id(), 'group', self.doc.unique_name(name))
        root.component_instance = {'definition_id': definition.id, 'revision': definition.revision,
            'placement': Transform(tuple(origin)).to_json(), 'overrides': {},
            'bindings': {key: port['source_id'] for key, port in definition.ports.items()},
            'node_map': {nid: nid for nid in definition.nodes}}
        captured = set(definition.nodes)
        for nid, node in list(nodes.items()):
            if nid not in captured and nid != root.id and captured & set(node.children):
                n = clone_node(node); n.children = [i for i in n.children if i not in captured]; nodes[nid] = n
        members = materialize(self.doc, root, definition)
        for nid, member in members.items(): member.name = self.doc.nodes[nid].name
        root.children = list(definition.roots)
        nodes.update(members); nodes[root.id] = root
        roots = [nid for nid in self.doc.roots if nid not in captured]
        if original is None: roots.append(root.id)
        definitions = {**self.doc.component_definitions, definition.id: definition}
        self.stack.push(ComponentChange(self.doc, 'Make linked component', definitions, nodes, roots))
        return {'definition_id': definition.id, 'instance_id': root.id}

    def create_component_family(self, name: str, variants: dict, parameters: dict, default_variant: Optional[str] = None):
        if not variants: raise KernelError('A family requires at least one variant')
        first = self.doc.component_definitions[next(iter(variants.values()))['definition_id']]
        definition = ComponentDefinition(uuid.uuid4().hex, name, {}, [], deepcopy(self.doc.materials),
            parameters=deepcopy(parameters), ports=deepcopy(first.ports), variants=deepcopy(variants),
            default_variant=default_variant or next(iter(variants)),
            provenance={'source':'explicit component family', 'document_id':self.doc.document_id, 'revision':self.doc.revision})
        definitions = {**self.doc.component_definitions, definition.id:definition}
        validate_library(definitions)
        for choice in variants: variant_nodes(definition, self.k, definitions=definitions, variant=choice)
        self.stack.push(ComponentChange(self.doc, 'Create component family', definitions, self.doc.nodes, self.doc.roots))
        return definition.id

    def link_component_family(self, instance_id: str, definition_id: str, variant: str, overrides: Optional[dict] = None):
        root = clone_node(self.doc.nodes[instance_id])
        if not root.component_instance or root.component_member: raise KernelError('Link a top-level occurrence before nesting it')
        family = self.doc.component_definitions[definition_id]
        if not family.variants: raise KernelError('Select a component family')
        target = structure(family, self.doc.component_definitions, variant)
        if root.component_instance['definition_id'] != target.id: raise KernelError('The occurrence must already use the selected variant definition')
        root.component_instance.update(definition_id=family.id, revision=family.revision, variant=variant, overrides=deepcopy(overrides or {}))
        nodes = {**self.doc.nodes, root.id:root, **materialize(self.doc, root)}
        self.stack.push(ComponentChange(self.doc, 'Link component family', self.doc.component_definitions, nodes, self.doc.roots))
        return instance_id

    def new_parametric_component(self, name: str = 'Parametric link', shape: str = 'box'):
        """A reusable primitive with explicit dimensions, units and feature bindings."""
        from .document import Document
        from .commands import Ops
        if shape not in ('box', 'cylinder'): raise KernelError('Choose box or cylinder')
        scratch = Document(self.k); local = Ops(scratch)
        if shape == 'box':
            nid = local.box((0,0,0), (20,10,60))
            dimensions = {'width': 20, 'depth': 10, 'length': 60}
            arguments = {'corner': [0,0,0], 'size': ['width','depth','length']}
        else:
            nid = local.cylinder((0,0,0), (0,0,1), 5, 60)
            dimensions = {'radius': 5, 'length': 60}
            arguments = {'base': [0,0,0], 'axis': [0,0,1], 'radius': 'radius', 'height': 'length'}
        definition = capture_definition(scratch, [nid], name)
        definition.parameters = {key: {'value': value, 'unit': 'mm', 'min': .01, 'max': 10000,
            'provenance': 'estimated', 'description': key.title()} for key, value in dimensions.items()}
        definition.features = [{'node': nid, 'kind': shape, 'arguments': arguments}]
        definition.provenance = {'source': 'explicit parametric primitive', 'geometry_units': 'mm'}
        validate_definition(definition)
        # Resolve the primitive's library materials against the host document.
        definition.materials = {mid: deepcopy(m) for mid, m in self.doc.materials.items()}
        for node in definition.nodes.values(): node.material = None
        definitions = {**self.doc.component_definitions, definition.id: definition}
        self.stack.push(ComponentChange(self.doc, 'New parametric component', definitions, self.doc.nodes, self.doc.roots))
        return definition.id

    def export_component(self, definition_id: str, path: str):
        from .document import write_archive
        validate_library(self.doc.component_definitions)
        included = set()
        def include(key):
            if key in included: return
            included.add(key)
            for child in dependencies(self.doc.component_definitions[key]): include(child)
        include(definition_id)
        definitions = {key: self.doc.component_definitions[key] for key in sorted(included)}
        manifest = {'version': SCHEMA_VERSION, 'root': definition_id,
                    'definitions': {key: d.manifest() for key,d in definitions.items()}}
        entries = [('component.json', json.dumps(manifest, indent=2).encode())]
        for key, definition in definitions.items():
            entries.extend(definition.archive_entries(self.k, 'geometry/' + key))
        write_archive(path, entries)
        return {'path': path, 'id': definition_id, 'revision': definitions[definition_id].revision,
                'definitions': len(definitions)}

    def import_component(self, path: str):
        import zipfile
        with zipfile.ZipFile(path) as archive:
            manifest = json.loads(archive.read('component.json'))
            if manifest.get('version') != SCHEMA_VERSION: raise KernelError('Unsupported component library version')
            root_id = manifest['root']
            incoming = {key: ComponentDefinition.from_archive(data, archive, self.k, 'geometry/' + key)
                        for key, data in manifest['definitions'].items()}
        if root_id not in incoming: raise KernelError('Library root definition is missing')
        if any(key != d.id for key,d in incoming.items()): raise KernelError('Component identity mismatch')
        validate_library(incoming)
        definitions = dict(self.doc.component_definitions); materials = dict(self.doc.materials)
        for key, definition in incoming.items():
            if key in definitions:
                existing = definitions[key]
                if existing.manifest() != definition.manifest() or existing.archive_entries(self.k, 'geometry') != definition.archive_entries(self.k, 'geometry'):
                    raise KernelError('An embedded component has the same ID but different contents')
                continue
            identities = {}
            for mid, material in definition.materials.items():
                target = mid
                if mid in materials and materials[mid].to_json() != material.to_json(): target = definition.id + ':' + mid
                identities[mid] = target
                renamed = deepcopy(material); renamed.id = target
                if target in materials and materials[target].to_json() != renamed.to_json(): raise KernelError('Conflicting component material')
                materials[target] = renamed
            definition.materials = {identities[mid]: materials[identities[mid]] for mid in definition.materials}
            for node in definition.nodes.values():
                node.material = identities.get(node.material, node.material)
                if node.robot and node.robot.get('solid_materials'):
                    node.robot['solid_materials'] = {key: identities[value] for key, value in node.robot['solid_materials'].items()}
            definitions[key] = definition
        validate_library(definitions)
        self.stack.push(ComponentChange(self.doc, 'Import component library', definitions, self.doc.nodes, self.doc.roots, materials))
        return root_id

    def component_catalogue(self):
        return {'version': SCHEMA_VERSION, 'features': deepcopy(FEATURES),
                'definitions': [d.descriptor() for d in self.doc.component_definitions.values()]}

    def create_component(self, ids: list[str], name: str, origin: tuple = (0., 0., 0.)) -> str:
        definition = capture_definition(self.doc, ids, name, origin)
        definitions = {**self.doc.component_definitions, definition.id: definition}
        self.stack.push(ComponentChange(self.doc, 'Create component', definitions, self.doc.nodes, self.doc.roots))
        return definition.id

    def place_component(self, definition_id: str, placement: Transform = Transform(), overrides: Optional[dict] = None,
                        bindings: Optional[dict] = None, name: Optional[str] = None, variant: Optional[str] = None) -> str:
        definition = self.doc.component_definitions[definition_id]
        template = structure(definition, self.doc.component_definitions, variant)
        root = Node(self.doc.new_id(), 'group', self.doc.unique_name(name or definition.name))
        root.component_instance = {'definition_id': definition.id, 'revision': definition.revision,
                                   'placement': placement.to_json() if isinstance(placement, Transform) else placement,
                                   'overrides': deepcopy(overrides or {}), 'bindings': deepcopy(bindings or {}),
                                   'node_map': {nid: self.doc.new_id() for nid in template.nodes}}
        if definition.variants: root.component_instance['variant'] = variant or definition.default_variant
        members = materialize(self.doc, root)
        root.children = [root.component_instance['node_map'][nid] for nid in template.roots]
        nodes = {**self.doc.nodes, root.id: root, **members}
        for key, material in definition.materials.items():
            if key not in self.doc.materials or self.doc.materials[key].to_json() != material.to_json():
                raise KernelError('Component material differs from this document; import its material explicitly first')
        self.stack.push(ComponentChange(self.doc, 'Place component', self.doc.component_definitions, nodes, [*self.doc.roots, root.id]))
        return root.id

    def set_component_parameters(self, definition_id: str, parameters: dict, features: Optional[list] = None, nested: Optional[dict] = None, family_variants: Optional[dict] = None):
        old = self.doc.component_definitions[definition_id]
        definition = copy(old)
        definition.parameters = deepcopy(parameters)
        definition.features = deepcopy(old.features if features is None else features)
        definition.revision += 1
        if family_variants is not None: definition.variants = deepcopy(family_variants)
        if nested is not None:
            definition.nodes = {nid: clone_node(n) for nid,n in old.nodes.items()}
            for nid, settings in nested.items():
                if nid not in definition.nodes or not definition.nodes[nid].component_instance or definition.nodes[nid].component_member:
                    raise KernelError('Nested parameter mapping must target an immediate child occurrence')
                if set(settings) - {'parameter_bindings', 'overrides'}: raise KernelError('Unsupported nested setting')
                definition.nodes[nid].component_instance.update(deepcopy(settings))
        definitions = {**self.doc.component_definitions, definition_id: definition}
        validate_library(definitions)
        affected = {definition_id}
        while True:
            parents = {key for key,d in definitions.items() if dependencies(d) & affected} - affected
            if not parents: break
            for key in parents:
                parent = copy(definitions[key]); parent.revision += 1; definitions[key] = parent
            affected.update(parents)
        # Validate defaults even when there are no placed occurrences.
        for key in affected:
            for choice in definitions[key].variants or [None]:
                variant_nodes(definitions[key], self.k, definitions=definitions, variant=choice)
        staging = copy(self.doc); staging.component_definitions = definitions
        nodes = dict(self.doc.nodes); variants = {}
        for original in self.doc.nodes.values():
            if original.component_instance and not original.component_member and original.component_instance['definition_id'] in affected:
                root = clone_node(original)
                root.component_instance['revision'] = definitions[root.component_instance['definition_id']].revision
                nodes[root.id] = root
                nodes.update(materialize(staging, root, variants=variants))
        self.stack.push(ComponentChange(self.doc, 'Edit component defaults', definitions, nodes, self.doc.roots))
        return definition.descriptor()

    def set_component_overrides(self, instance_id: str, overrides: dict, placement: Optional[Transform] = None):
        root = clone_node(self.doc.nodes[instance_id])
        if not root.component_instance: raise KernelError('Select a component occurrence')
        if root.component_member:
            if placement is not None: raise KernelError('Move nested components through the parent definition')
            target_id = root.id
            while root.component_member: root = clone_node(self.doc.nodes[root.component_member['instance_id']])
            if root.locked: raise KernelError('Component occurrence is locked')
            source = next(key for key, value in root.component_instance['node_map'].items() if value == target_id)
            branch = root.component_instance.setdefault('nested_overrides', {})
            if overrides: branch[source] = deepcopy(overrides)
            else: branch.pop(source, None)
        else:
            if root.locked: raise KernelError('Component occurrence is locked')
            root.component_instance['overrides'] = deepcopy(overrides)
        if placement is not None: root.component_instance['placement'] = placement.to_json() if isinstance(placement, Transform) else placement
        nodes = {**self.doc.nodes, root.id: root, **materialize(self.doc, root)}
        self.stack.push(ComponentChange(self.doc, 'Edit component occurrence', self.doc.component_definitions, nodes, self.doc.roots))
        return deepcopy(root.component_instance)

    def detach_component(self, instance_id: str):
        root = self.doc.nodes[instance_id]
        if not root.component_instance: raise KernelError('Select a component occurrence')
        if root.component_member: raise KernelError('Detach the outer occurrence first')
        nodes = dict(self.doc.nodes)
        for nid in [root.id, *root.component_instance['node_map'].values()]:
            node = clone_node(nodes[nid]); node.component_instance = node.component_member = None
            node.locked = False; nodes[nid] = node
        self.stack.push(ComponentChange(self.doc, 'Detach component', self.doc.component_definitions, nodes, self.doc.roots))
        return instance_id
