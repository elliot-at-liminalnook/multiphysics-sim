"""Authoritative display-only reference pose sampling, independent of Qt."""
import math
from .pose import PoseModel, joint_range
from .motion import validate_program, sample_program, sweep_program
from .kernel import KernelError
from .candidates import check_revision


def guard(doc, body):
    from .experiments import RevisionConflict
    if body.get('document_id') != doc.document_id:
        raise RevisionConflict('Document changed; motion draft preserved')
    check_revision(doc, body.get('expected_revision'))


def identity(doc):
    return {'document_id': doc.document_id, 'revision': doc.revision,
            'source_kind': 'live_kinematic', 'source_id': doc.document_id,
            'physical_hash': None, 'archive_hash': None}


def metadata(doc):
    model = PoseModel(doc)
    return {'identity': identity(doc), 'focus_ids': {jid: focus_nodes(model, jid) for jid in model.drivers}, 'assumptions': ['Kinematic preview; no physics, loads or collision checks',
        'Ideal transmissions; fallback display ranges are not physical travel stops'],
        'joints': [{'id': jid, 'name': model.names[jid], 'unit': 'mm' if j.type == 'prismatic' else 'rad',
            'home': j.home, 'lower': j.lower, 'upper': j.upper,
            'display_lower': joint_range(j)[0], 'display_upper': joint_range(j)[1],
            'driver': jid in model.drivers, 'pivot': j.pivot, 'axis': j.axis,
            'child': j.child, 'parent': j.parent} for jid, j in model.joints.items() if jid in model.home]}


def resolve_program(doc, program):
    if isinstance(program, str):
        try: program = doc.robot_settings.get('motion_programs', {})[program]
        except KeyError: raise KernelError('Motion pattern not found')
    return validate_program(doc, program)


def sample_model(model, positions):
    """Shared Qt/headless primitive: reference solver updates resolved positions."""
    matrices = model.matrices(positions)
    return {'matrices': matrices, 'positions': {jid: float(v) for jid, v in model.last_positions.items()},
            'closure_error_mm': model.last_error_mm}


def sample(doc, body):
    guard(doc, body)
    model = PoseModel(doc)
    from .motion_continuation import restore_prior
    restore_prior(model, body.get('prior'), identity(doc))
    seconds = body.get('time', 0.)
    if type(seconds) not in (int,float) or not math.isfinite(seconds): raise KernelError('Time must be finite')
    program = body.get('program')
    positions = body.get('positions', {})
    if program is not None:
        program = resolve_program(doc, program)
        positions = sample_program(program, seconds)
    if not isinstance(positions, dict): raise KernelError('positions must be an object in radians/mm')
    if any(jid not in model.drivers for jid in positions): raise KernelError('Only driver joints can be requested')
    if any(type(v) not in (int,float) or not math.isfinite(v) for v in positions.values()): raise KernelError('Positions must be finite radians/mm')
    resolved = sample_model(model, {**model.home, **positions})
    resolved['matrices'] = {nid: m.tolist() for nid, m in resolved['matrices'].items()}
    return {'identity': identity(doc), 'time': seconds, 'program': program,
            'prior_applied': body.get('prior') is not None, **resolved}


def sweep(doc, body):
    guard(doc, body)
    model = PoseModel(doc)
    if body.get('joint') not in model.drivers: raise KernelError('Choose a driver joint')
    return validate_program(doc, sweep_program(model, body['joint']))


def capture_definition(doc):
    """Copy only kinematic inputs on owner thread; no OCCT or solver work."""
    from copy import deepcopy
    from types import SimpleNamespace
    with doc._lock:
        nodes = {nid: SimpleNamespace(id=nid, name=n.name, joint=deepcopy(n.joint),
                    disabled=n.disabled, robot=deepcopy(n.robot)) for nid, n in doc.nodes.items()}
        return SimpleNamespace(document_id=doc.document_id, revision=doc.revision,
                    nodes=nodes, robot_settings=deepcopy(doc.robot_settings))


def focus_nodes(model, jid):
    """Reference mechanism focus: driven/loop members and rigid attachments."""
    ids = {model.joints[jid].child}
    for driven, (driver, _) in model.transmissions.items():
        if driver == jid: ids.add(model.joints[driven].child)
    for loop in model.loops.values():
        path = set(model._ancestors(loop.parent)) ^ set(model._ancestors(loop.child))
        if jid in path: ids.update(n for n in (loop.parent, loop.child) if n is not None)
    while True:
        children = {child for child, (parent, joint) in model.parents.items()
                    if parent in ids and (joint is None or model.joints[joint].type == 'fixed')}
        if children <= ids: return sorted(ids)
        ids.update(children)
