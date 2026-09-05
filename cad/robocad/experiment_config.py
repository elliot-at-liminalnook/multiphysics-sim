"""Version-one experiment inputs, shared by UI/API capture and the worker."""
import math
from copy import deepcopy
from contextlib import contextmanager
from .kernel import KernelError


PROFILES = {
    'quick_check': {'seconds': 3.2, 'step': .0005, 'sample': .01,
                    'contact': False, 'flex': False, 'planar': False, 'noise': False},
    'validation': {'seconds': 3.2, 'step': .00025, 'sample': .005,
                   'contact': True, 'flex': True, 'planar': False, 'noise': True},
}
MAX_SEED = 2**53 - 1  # Exact in both Rhai integers and native numeric parameters.


@contextmanager
def source_context(location):
    """Keep a captured configure declaration attached to validation failures."""
    try:
        yield
    except KernelError as error:
        if not location: raise
        raise KernelError(f"{location['source']}:{location.get('line') or 0}:"
                          f"{location.get('column') or 0}: {error}") from error


def object_fields(value, allowed, label):
    if not isinstance(value, dict): raise KernelError(f'{label} must be an object')
    unknown = set(value) - set(allowed)
    if unknown: raise KernelError(f'Unknown {label} fields: {sorted(unknown)}')
    return value


def settings(overrides, profile='quick_check'):
    if profile not in PROFILES: raise KernelError(f'Unknown profile {profile}; choose quick_check or validation')
    object_fields(overrides, PROFILES[profile], 'settings')
    value = {**PROFILES[profile], **overrides}
    for name in ('seconds', 'step', 'sample'):
        v = value[name]
        if type(v) not in (int, float) or not math.isfinite(v) or v <= 0:
            raise KernelError(f'settings.{name} must be positive and finite')
    if value['sample'] < value['step']: raise KernelError('settings.sample must be at least settings.step')
    for name in ('contact', 'flex', 'planar', 'noise'):
        if type(value[name]) is not bool: raise KernelError(f'settings.{name} must be a boolean')
    return value


def request(value):
    object_fields(value, ('expected_revision', 'system', 'parameters', 'controller', 'settings',
                         'label', 'parent_run', 'candidate_id', 'profile', 'seed', 'preflight'), 'experiment')
    value = deepcopy(value)
    if type(value.setdefault('preflight', False)) is not bool: raise KernelError('preflight must be a boolean')
    profile = value.setdefault('profile', 'quick_check')
    seed = value.setdefault('seed', 0)
    if type(seed) is not int or not 0 <= seed <= MAX_SEED:
        raise KernelError(f'seed must be an integer in [0, {MAX_SEED}]')
    value['settings'] = settings(value.get('settings', {}), profile)
    if not isinstance(value.get('parameters', {}), dict): raise KernelError('parameters must be an object')
    controller = value.get('controller')
    if controller is not None:
        object_fields(controller, ('language', 'sources', 'parameters', 'command', 'process', 'seam', 'interface'), 'controller')
        language = controller.get('language')
        if language not in ('rhai', 'process'): raise KernelError('controller.language must be rhai or process')
        if not isinstance(controller.get('parameters', {}), dict): raise KernelError('controller.parameters must be an object')
        if controller.get('interface', 'position_target') not in ('position_target', 'driver_duty'):
            raise KernelError('controller.interface must be position_target or driver_duty')
        if language == 'rhai' and 'command' in controller: raise KernelError('Rhai controllers use sources, not a process command')
        if language == 'rhai' and 'process' in controller: raise KernelError('Rhai controllers use sources, not a process bundle')
        if language == 'process':
            if 'sources' in controller: raise KernelError('Process controller sources must be captured as artifacts')
            if 'command' in controller:
                raise KernelError('Experiment process controllers require a captured process bundle with runtime, entry, files and arguments; raw commands remain available in sim-cad')
            from .experiment_process import capture
            controller['process'] = capture(controller.get('process'))
    return value


def cad_overrides(model, overrides, location=None):
    if not isinstance(overrides, list): raise KernelError('cad_overrides must be an array')
    evidence, seen = [], set()
    for override in overrides:
        object_fields(override, ('section', 'id', 'field', 'value'), 'CAD override')
        if set(override) != {'section', 'id', 'field', 'value'}:
            raise KernelError('CAD overrides require section, id, field and value')
        section, identity, path, value = (override[k] for k in ('section', 'id', 'field', 'value'))
        if section not in ('links', 'joints', 'motors'): raise KernelError(f'Invalid CAD override section {section}')
        matches = [v for v in model[section] if v.get('id') == identity or v.get('name') == identity]
        if len(matches) != 1: raise KernelError(f'CAD override {identity} must identify one {section} entry')
        if not isinstance(path, str) or not path.startswith('/'): raise KernelError('CAD override field must be a JSON pointer')
        if (section == 'joints' and path == '/physics/flex_patch_radius') or (
                section == 'links' and path.startswith('/flex/boundary_frames/') and '/patch/' in path):
            raise KernelError('Flex patches must be set before CAD derivation: use set_joint_physics(flex_patch_radius=...) or the joint Properties editor, then capture a new run')
        item = matches[0]
        key = (section, item.get('id', item['name']), path)
        if key in seen: raise KernelError(f'Conflicting repeated CAD override for {identity}{path}')
        seen.add(key)
        target, parts = item, path[1:].split('/')
        try:
            for index, part in enumerate(parts):
                if '~' in part.replace('~0', '').replace('~1', ''): raise ValueError('invalid JSON pointer escape')
                part = part.replace('~1', '/').replace('~0', '~')
                if isinstance(target, list):
                    if not part.isdecimal() or str(int(part)) != part: raise ValueError('invalid array index')
                    part = int(part)
                if index == len(parts)-1: break
                target = target[part]
            old = target[part]
        except (KeyError, IndexError, TypeError, ValueError) as error:
            raise KernelError(f'CAD override {identity}{path}: field does not exist ({error})') from error
        if type(old) not in (int, float) or type(value) not in (int, float) or not math.isfinite(value):
            raise KernelError(f'CAD override {identity}{path} must change an existing numeric field to a finite number')
        evidence.append({**override, 'id': item.get('id'), 'name': item['name'], 'before': old, 'source': location})
        target[part] = value
    return evidence
