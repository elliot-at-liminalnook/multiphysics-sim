"""Read-only canonical parity observations; no operation dispatch or GUI imports.

Units follow kernel/base.py and physical.py, rather than caller metadata. Missing
provenance is retained, never filled from a corpus declaration. Geometry is in
resolved CAD world coordinates; inertia is the volume moment about the centroid.
"""
import dataclasses
import hashlib
import math
from pathlib import Path
from .parity_paths import read_regular


MAX_POINTS = 4096


def plain(value):
    if dataclasses.is_dataclass(value):
        return plain(dataclasses.asdict(value))
    if isinstance(value, dict):
        return {str(k): plain(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [plain(v) for v in value]
    if hasattr(value, 'tolist'):
        return plain(value.tolist())
    return value


def finite(value):
    if isinstance(value, float):
        return math.isfinite(value)
    if isinstance(value, dict):
        return all(finite(v) for v in value.values())
    if isinstance(value, (list, tuple)):
        return all(finite(v) for v in value)
    return True


def observation(value=None, *, owner='robocad.document', unit=None, frame=None,
                provenance=None, uncertainty=None, state='present', reason=None):
    value = plain(value)
    if state == 'present' and not finite(value):
        state, reason = 'invalid', 'Non-finite authoritative observation'
    return {'owner': owner, 'unit': unit, 'frame': frame,
            'provenance': provenance, 'uncertainty': plain(uncertainty),
            'value': {'state': state, 'value': value if state == 'present' else reason}}


def source_identity(doc, expected_sha256=None):
    if not doc.path:
        raise ValueError('identity.source_sha256: loaded file is required')
    path = Path(doc.path)
    if path.is_symlink() or not path.is_file():
        raise ValueError('identity.source_sha256: require a regular non-symlink source')
    digest = hashlib.sha256(read_regular(path)).hexdigest()
    if expected_sha256 is not None and digest != expected_sha256:
        raise ValueError('identity.source_sha256: isolated source hash mismatch')
    return digest


def collect(doc, expected_sha256=None):
    """Collect independently owned raw observations; errors are explicit fields.

    Tessellation is bounded without truncating: larger meshes are unsupported.
    Topology diagnostics remain separate from any geometric agreement.
    """
    out = {}
    def put(path, value=None, **kwargs):
        if path not in ('provenance', 'uncertainty'):
            kwargs.setdefault('provenance', 'derived: authoritative CAD document observation')
        out[path] = observation(value, **kwargs)
    try:
        import sys
        import OCP
        import numpy
        import scipy
        from . import __version__
        put('adapter.environment', {'python_version': sys.version,
            'python_executable': sys.executable, 'robocad_version': __version__,
            'ocp_version': OCP.__version__, 'numpy_version': numpy.__version__,
            'scipy_version': scipy.__version__,
            'kernel_class': type(doc.kernel).__module__ + '.' + type(doc.kernel).__name__},
            owner='robocad.kernel', provenance='observed: executing Python/kernel dependencies')
    except Exception as error:
        put('adapter.environment', state='unsupported',
            reason='Executing dependency identity unavailable: ' + str(error), owner='robocad.kernel')
    put('identity.source_sha256', source_identity(doc, expected_sha256))
    put('identity.document_id', doc.document_id)
    from .motion_service import identity
    put('live.identity', identity(doc), owner='robocad.motion_service',
        provenance='derived: live CAD identity')
    put('model.units', 'mm', unit='mm')
    put('model.frame', 'cad-world', frame='cad-world')
    put('lifecycle.revision', doc.revision)
    put('lifecycle.nodes', {n.id: {'name': n.name, 'kind': n.kind,
        'parent': n.parent, 'disabled': n.disabled} for n in doc.walk()})
    put('components', {'definitions': {key: definition.manifest()
        for key, definition in getattr(doc, 'component_definitions', {}).items()},
        'occurrences': {n.id: {'instance': n.component_instance, 'member': n.component_member}
            for n in doc.walk() if n.component_instance or n.component_member}},
        owner='robocad.components', provenance='derived: authoritative component definitions and occurrences')
    put('joints', {n.id: n.joint.to_json() for n in doc.walk() if n.joint is not None},
        owner='robocad.robotics', frame='cad-world', unit='mixed:mm,rad')
    for key in ('provenance', 'uncertainty'):
        value = doc.robot_settings.get(key)
        put(key, value, state='missing' if value is None else 'present',
            reason='No explicit source robot_settings.' + key)
    for node in doc.walk():
        if node.disabled:
            continue
        try:
            body = doc.resolved_body(node.id)
        except Exception as error:
            put('geometry.' + node.id, state='invalid', reason=str(error), owner='robocad.kernel')
            continue
        if body is None:
            continue
        prefix = 'geometry.' + node.id
        try:
            props = doc.kernel.mass_properties(body)
            has_volume = body.kind == 'solid' and bool(doc.kernel.solid_components(body))
            for name, value, unit in (
                ('volume', props.volume, 'mm^3'), ('area', props.area, 'mm^2'),
                ('center', props.centroid, 'mm'), ('inertia', props.inertia, 'mm^5' if has_volume else 'mm^4'),
                ('bbox_min', props.bbox_min, 'mm'), ('bbox_max', props.bbox_max, 'mm')):
                if name == 'volume' and not has_volume:
                    put(prefix + '.' + name, state='unsupported', unit=unit, frame='cad-world',
                        reason='Volume observation requires volumetric solid topology')
                    continue
                put(prefix + '.' + name, value, owner='robocad.kernel', unit=unit,
                    frame='cad-world', provenance='derived: OCCT volume properties' if has_volume else 'derived: OCCT surface properties')
            if node.kind == 'body' and has_volume and props.volume > 0:
                from .physical import body_mass_properties
                mass, com, inertia, source = body_mass_properties(doc, node, props)
                for name, value, unit in (('mass_kg', mass, 'kg'),
                    ('com_m', com, 'm'), ('inertia_kg_m2', inertia, 'kg*m^2')):
                    put(prefix + '.' + name, value, owner='robocad.physical', unit=unit,
                        frame='cad-world', provenance=source)
            else:
                for name in ('mass_kg', 'com_m', 'inertia_kg_m2'):
                    put(prefix + '.' + name, state='unsupported',
                        reason='Physical mass owner requires positive-volume body; no surface/instance approximation')
        except NotImplementedError as error:
            put(prefix, state='unsupported', reason=str(error), owner='robocad.kernel')
        except Exception as error:
            put(prefix, state='invalid', reason=str(error), owner='robocad.kernel')
        try:
            put('topology.' + node.id, {'faces': len(doc.kernel.faces(body)),
                'edges': len(doc.kernel.edges(body)), 'vertices': len(doc.kernel.vertices(body)),
                'validation': plain(doc.kernel.validate(body)),
                'solids': doc.kernel.solid_inventory(body)}, owner='robocad.kernel')
        except NotImplementedError as error:
            put('topology.' + node.id, state='unsupported', reason=str(error))
        except Exception as error:
            put('topology.' + node.id, state='invalid', reason=str(error))
        try:
            mesh = doc.mesh_of(node.id, .1)
            path = 'tessellation.' + node.id + '.points'
            if mesh is None:
                put(path, state='missing', reason='No authoritative mesh')
            elif len(mesh.vertices) > MAX_POINTS:
                put(path, state='unsupported', reason='Mesh exceeds 4096-point comparison bound')
            else:
                put(path, mesh.vertices, owner='robocad.kernel', unit='mm', frame='cad-world',
                    provenance='derived: OCCT tessellation deflection 0.1 mm')
            if mesh is not None:
                put('tessellation.' + node.id + '.topology', {
                    'triangles': len(mesh.triangles), 'face_count': mesh.face_count,
                    'triangle_face': mesh.triangle_face}, owner='robocad.kernel')
        except Exception as error:
            put('tessellation.' + node.id + '.points', state='invalid', reason=str(error))
    return out


def result_observation(result, owner='robocad.command'):
    """Mixed-unit command result retains its entire authoritative structure."""
    return observation(result, owner=owner)


def required_field(result, key, **metadata):
    """Malformed replies retain absence and null-invalid separately."""
    if not isinstance(result, dict):
        return observation(state='invalid', reason='Reply must be an object', **metadata)
    if key not in result:
        return observation(state='missing', reason='Authoritative reply omitted ' + key, **metadata)
    if result[key] is None:
        return observation(state='invalid', reason='Required reply field is null: ' + key, **metadata)
    return observation(result[key], **metadata)


def captured_observation(doc, records, name, action):
    """Read-only archive owner shared by adapters, independent of dispatch.

    The ephemeral cache pins the observed source; live edits never replace it.
    This produces no files, Qt display, simulation, rendering or export.
    """
    import io
    from .document import Document
    from .snapshots import capture
    from . import captured_review
    from .kernel import KernelError
    if not isinstance(name, str) or not name or len(name) > 128:
        raise KernelError('capture: require a nonempty bounded observation label')
    if action == 'snapshot':
        if name in records:
            raise KernelError('Captured observation already exists; overwrite refused')
        if len(records) >= 4:
            raise KernelError('Captured observation cache bound reached (four sources)')
        captured = capture(doc)
        record = {'id': name, 'document_id': captured.document_id,
            'revision': captured.revision, 'physical_hash': captured.physical_hash,
            'cad_archive_hash': captured.archive_hash,
            'provenance': {'physical_hash': captured.physical_hash,
                'cad_archive_hash': captured.archive_hash}}
        records[name] = (Document.load(io.BytesIO(captured.data)), record)
    if name not in records:
        raise KernelError('Captured source not found: ' + name)
    captured_doc, record = records[name]
    if action in ('snapshot', 'geometry'):
        return captured_review.geometry(captured_doc, record, 'parity_capture')
    if action == 'identity':
        return captured_review.identity(record, 'parity_capture')
    raise NotImplementedError('Captured observation action unsupported: ' + action)
