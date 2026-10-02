"""Independent reference dispatch to Ops and shared headless service owners.

This module never calls Service.op, REST, or the Rust native dispatch. The small
allowlist prevents filesystem and rendering operations in scenario inputs.
"""
import time
from .kernel import KernelError
from .commands import Ops
from .component_service import owner as component_owner
from .component_jobs import OPERATIONS
from . import motion_service

SAFE_OPS = {'rename', 'set_visible', 'set_locked', 'set_disabled', 'set_material',
    'set_color', 'set_pivot', 'transform', 'set_joint', 'set_ground',
    'set_joint_physics', 'set_uncertainty', 'set_material_props', 'set_control',
    'set_robot_setting', 'save_motion', 'delete_motion'}
SAFE_COMPONENTS = {'set_component_parameters', 'set_component_overrides',
    'detach_component', 'transform_components'}


class Incomplete(RuntimeError):
    pass


class Dispatcher:
    def __init__(self, doc, cancelled=lambda: False):
        self.doc = doc
        self.ops = Ops(doc)
        self.cancelled = cancelled
        self.components = component_owner(self.ops)
        self.prior = None
        self.captured = {}

    def revision(self, offset):
        return self.doc.revision + offset

    def execute(self, operation):
        if self.cancelled():
            raise InterruptedError('Cancelled before command dispatch')
        kind = operation['kind']
        if kind == 'observe':
            return None
        if kind == 'rename':
            return self.ops.rename(operation['node'], operation['name'])
        if kind in ('undo', 'redo'):
            result = getattr(self.ops, kind)()
            if result is None:
                raise KernelError('No command available to ' + kind)
            return result
        if kind == 'op':
            name = operation['name']
            if name not in SAFE_OPS:
                raise NotImplementedError('Operation outside headless isolated allowlist: ' + name)
            return getattr(self.ops, name)(*operation.get('args', []), **operation.get('kwargs', {}))
        if kind == 'configure_robot':
            return self.ops.configure_robot(self.revision(operation['revision_offset']),
                updates=operation['updates'])
        if kind == 'component':
            return self.component(operation)
        if kind == 'pose':
            body = {'document_id': self.doc.document_id,
                'expected_revision': self.revision(operation['revision_offset']),
                'positions': operation['positions'], 'time': operation['time']}
            if operation['continuation'] and self.prior is not None:
                body['prior'] = self.prior
            result = motion_service.sample(self.doc, body)
            self.prior = {'identity': result['identity'], 'positions': result['positions']}
            return result
        if kind == 'physical':
            from .physical import export_physical_model
            from .kernel import Plane
            return export_physical_model(self.doc, planar=Plane.xz() if operation['planar'] else None,
                flex=operation['flex'])
        if kind == 'captured':
            return self.capture(operation)
        raise NotImplementedError('Unknown operation kind: ' + str(kind))

    def component(self, operation):
        request = operation['operation']
        tag = request.get('operation', request.get('kind'))
        names = {'overrides': 'set_component_overrides', 'defaults': 'set_component_parameters',
            'detach': 'detach_component', 'transform': 'transform_components'}
        name = names.get(tag)
        if name not in SAFE_COMPONENTS or name not in OPERATIONS:
            raise NotImplementedError('Component operation outside isolated allowlist: ' + str(name))
        kwargs = {key: value for key, value in request.items() if key not in ('operation', 'kind')}
        if tag == 'defaults' and 'nested' in kwargs:
            kwargs['nested'] = {key: {'parameter_bindings': value.get('parameter_bindings', {}),
                'overrides': value.get('overrides', {})} for key, value in kwargs['nested'].items()}
        if kwargs.get('placement') is not None:
            from .document import Transform
            kwargs['placement'] = Transform.from_json(kwargs['placement'])
        status = self.components.start(name, [], kwargs,
            self.revision(operation['revision_offset']), self.doc.document_id)
        identity = status['id']
        if operation.get('interfere'):
            node = operation['interfere']
            self.ops.rename(node, self.doc.nodes[node].name + ' parity-interference')
        deadline = time.monotonic() + 60
        while True:
            cancel = operation['cancel'] or self.cancelled() or time.monotonic() >= deadline
            status = self.components.status(identity, cancel=cancel)
            state = status['state']
            if state == 'applied':
                if cancel:
                    raise Incomplete('Component applied before cancellation acknowledgment')
                return status
            if state == 'failed':
                # Preparation exceptions and worker crashes are not validated
                # command refusals. Only the authoritative commit guard plus
                # its captured/live identity mismatch establishes rejection.
                guard = 'The document changed during preparation. Your edits are preserved; retry the component operation.'
                if status.get('error') == guard and (
                        status.get('document_id') != self.doc.document_id or
                        status.get('revision') != self.doc.revision):
                    from .experiments import RevisionConflict
                    raise RevisionConflict(guard)
                raise Incomplete(status.get('error') or 'Component preparation failed without validated refusal')
            if state == 'cancelled':
                if operation['cancel'] or self.cancelled():
                    raise InterruptedError('Component cancellation acknowledged')
                raise Incomplete('Component deadline exceeded')
            if cancel and time.monotonic() >= deadline + 2:
                raise Incomplete('Component cancellation acknowledgment deadline exceeded')
            time.sleep(.02)

    def capture(self, operation):
        """In-memory immutable archive capture, reviewed by captured_review itself.

        Archive snapshot is the same owner used by experiments; no experiment,
        renderer or exporter is launched. Explicit record identity is preserved.
        """
        from .parity_observations import captured_observation
        return captured_observation(self.doc, self.captured,
            operation['capture'], operation['action'])

    def close(self):
        """Cancel before bounded drain; process runner also owns the child group."""
        self.components.cancel_all()
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            statuses = self.components.discover()
            if all(s['state'] not in ('pending', 'running', 'ready') for s in statuses):
                return
            time.sleep(.02)
        # Do not imply child completion; runner must kill and reap our process group.
        raise Incomplete('Component children have not acknowledged shutdown')
