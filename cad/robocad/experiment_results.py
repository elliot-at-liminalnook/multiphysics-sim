"""Units, measured expectations, comparisons and replay for captured runs.

This module has no Qt dependency: the desktop and agent API review the same
signals, sample alignment and pass/fail decisions.
"""
from bisect import bisect_left, bisect_right
import math

from .kernel import KernelError


def signals(result):
    """Return named series with their own time base, unit and stable CAD IDs."""
    out = {}
    trace = result.get('trace', {})
    times = trace.get('t', [])
    mapping = {(m.get('section', 'links'), m['name']): m for m in result.get('cad_mapping', [])}

    def add(key, values, unit, section=None, name=None, t=None, interpolation='linear'):
        m = mapping.get((section, name), {})
        identity = key
        if m.get('id'):
            role = key.split('/')[1] if key.startswith('controller/') else 'plant'
            quantity = key.rsplit('/', 1)[-1].rsplit('.', 1)[-1]
            identity = f"{role}/{section}/{m['id']}/{quantity}"
        out[key] = {'t': times if t is None else t, 'values': values, 'unit': unit,
                    'identity': identity,
                    'node_ids': list(dict.fromkeys([x for x in [m.get('id'), *m.get('members', []), *m.get('related_ids', [])] if x])),
                    'interpolation': interpolation}

    graph_components = sorted(result.get('component_graph_mapping', []), key=lambda m: len(m['native_name']), reverse=True)
    script_components = sorted(result.get('script_component_mapping', []), key=lambda m: len(m['native_name']), reverse=True)
    for name, values in trace.get('signals', {}).items():
        add(name, values, result.get('signal_units', {}).get(name, 'unknown'))
        component = next((m for m in graph_components if name.startswith(m['native_name']+'.')), None)
        if component:
            out[name]['component_id'] = component['id']
            out[name]['component_name'] = component['name']
            out[name]['identity'] = 'component/'+component['id']+'/'+name[len(component['native_name'])+1:]
            out[name]['node_ids'] = [component['body_id']] if component.get('body_id') else []
        declaration = component or next((m for m in script_components if name.startswith(m['native_name']+'.')), None)
        if declaration and declaration.get('source'):
            out[name]['source'] = {'path': declaration['source'], 'line': declaration.get('line') or 1,
                'column': declaration.get('column') or 1}
    for name, values in trace.get('joints', {}).items():
        add(f'joints/{name}/angle', values, 'rad', 'joints', name)
    for name, block in trace.get('motors', {}).items():
        for field, values in block.items():
            add(f'motors/{name}/{field}', values,
                {'current': 'A', 'winding_c': '°C', 'torque_nm': 'N·m'}.get(field, 'unknown'), 'motors', name)
    for name, boundaries in trace.get('flex', {}).items():
        for bi, boundary in enumerate(boundaries):
            points, displacements = boundary['point_m'], boundary['displacement_m']
            if len(points) != len(times) or len(displacements) != len(times):
                raise KernelError(f'Missing synchronized flex samples for {name}')
            for vector in [*points, *displacements]:
                _flex_vector(vector, name)
            prefix = f'flex/{name}/{bi}:{boundary["name"]}'
            for axis, field in enumerate(('dx', 'dy', 'dz', 'magnitude')):
                values = [v[axis] if axis < 3 else math.hypot(*v) for v in displacements]
                key = f'{prefix}/{field}'
                add(key, values, 'm', 'links', name)
                # Preserve boundary identity as well as the link's stable CAD ID.
                boundary_identity = boundary.get('id') or f'{bi}:{boundary["name"]}'
                out[key]['identity'] += f'/flex/{boundary_identity}'
    frames = result.get('controller_frames', [])
    contract = result.get('controller_contract', {})
    for section, channels in [('sensors', contract.get('sensors', [])), ('commands', contract.get('actuators', []))]:
        for channel in channels:
            name = channel['name']
            component = name.rsplit('.', 1)[0]
            cad_section = 'motors' if ('motors', component) in mapping else 'joints'
            add(f'controller/{section}/{name}', [f[section][name] for f in frames], channel['unit'],
                cad_section, component, t=[f['t'] for f in frames], interpolation='hold')
    for name, series in out.items():
        t, y = series['t'], series['values']
        if len(t) != len(y) or not all(math.isfinite(v) for v in [*t, *y]):
            raise KernelError(f'Invalid or non-finite samples in {name}')
        if any(b <= a for a, b in zip(t, t[1:])):
            raise KernelError(f'Non-increasing sample times in {name}')
    return out


def value_at(series, time):
    """Align inside the recorded interval; never extrapolate across a gap."""
    t, y = series['t'], series['values']
    if not t or time < t[0] or time > t[-1]:
        raise KernelError('Time is outside the recorded signal interval')
    i = min(len(t) - 1, bisect_right(t, time) - 1)
    if i == len(t) - 1 or series.get('interpolation') == 'hold':
        return y[i]
    f = (time - t[i]) / (t[i + 1] - t[i])
    return y[i] + f * (y[i + 1] - y[i])


def sample_index(times, time):
    if not times:
        raise KernelError('Run contains no replay samples')
    i = min(bisect_left(times, time), len(times) - 1)
    return i - 1 if i and abs(times[i - 1] - time) <= abs(times[i] - time) else i


def replay_matrices(result, index):
    """World-mm delta matrices mapped to every member of a merged link."""
    import numpy as np
    out = {}
    times = result.get('trace', {}).get('t', [])
    if not 0 <= index < len(times):
        raise KernelError('Replay sample index is out of range')
    for link in result.get('cad_mapping', []):
        if link.get('section', 'links') != 'links':
            continue
        frames = result.get('trace', {}).get('poses', {}).get(link['name'], [])
        if len(frames) != len(times):
            raise KernelError(f"Missing synchronized poses for {link['name']}")
        matrix = np.asarray(frames[index], dtype=float)
        if matrix.shape != (4, 4) or not np.isfinite(matrix).all():
            raise KernelError(f"Invalid pose for {link['name']}")
        for nid in [link.get('id'), *link.get('members', [])]:
            if nid:
                out[nid] = matrix
    return out


def _flex_vector(vector, name):
    if not isinstance(vector, (list, tuple)) or len(vector) != 3 or not all(
            isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v) for v in vector):
        raise KernelError(f'Invalid flex vector for {name}')


def replay_flex(result, index, scale=1.):
    """World-mm boundary arrows; scale changes the display, never physical signals.

    The captured BREP remains rigid. These samples describe modal motion at
    attachment frames, not a reconstructed full-field surface deformation.
    """
    if not isinstance(scale, (int, float)) or isinstance(scale, bool) or not math.isfinite(scale) or scale <= 0:
        raise KernelError('Flex display scale must be finite and positive')
    times = result.get('trace', {}).get('t', [])
    if not 0 <= index < len(times):
        raise KernelError('Replay sample index is out of range')
    mapping = {m['name']: m for m in result.get('cad_mapping', []) if m.get('section', 'links') == 'links'}
    out = []
    for name, boundaries in result.get('trace', {}).get('flex', {}).items():
        link = mapping.get(name, {})
        for bi, boundary in enumerate(boundaries):
            if any(len(boundary[field]) != len(times) for field in ('point_m', 'displacement_m')):
                raise KernelError(f'Missing synchronized flex samples for {name}')
            point, displacement = boundary['point_m'][index], boundary['displacement_m'][index]
            _flex_vector(point, name); _flex_vector(displacement, name)
            out.append({'name': f'{name}/{boundary["name"]}', 'boundary': bi,
                'node_ids': list(dict.fromkeys(x for x in [link.get('id'), *link.get('members', [])] if x)),
                'point_mm': [v*1000 for v in point],
                'tip_mm': [(p+scale*d)*1000 for p, d in zip(point, displacement)],
                'displacement_m': list(displacement)})
    return out


def evaluate_expectations(result, expectations):
    """Evaluate explicit units and sample-window reductions.

    RMS/mean are sample statistics. The result names that convention explicitly;
    they are not presented as continuous-time integrals.
    """
    catalogue = signals(result)
    if not isinstance(expectations, list):
        raise KernelError('expectations must be an array')
    metrics = []
    for spec in expectations:
        allowed = {'name', 'signal', 'unit', 'reduction', 'start', 'end', 'target', 'min', 'max'}
        if not isinstance(spec, dict) or set(spec) - allowed:
            raise KernelError('Unknown expectation field')
        name = spec.get('name', spec.get('signal', 'expectation'))
        series = catalogue.get(spec.get('signal'))
        if series is None:
            raise KernelError(f"{name}: unknown signal {spec.get('signal')}")
        if spec.get('unit') != series['unit'] or series['unit'] == 'unknown':
            raise KernelError(f"{name}: expected explicit unit {series['unit']}")
        for key in ('start', 'end', 'target', 'min', 'max'):
            if key in spec and (isinstance(spec[key], bool) or not isinstance(spec[key], (int, float)) or not math.isfinite(spec[key])):
                raise KernelError(f'{name}: {key} must be finite')
        if not {'min', 'max'} & spec.keys() or spec.get('min', -math.inf) > spec.get('max', math.inf):
            raise KernelError(f'{name}: provide valid min/max bounds')
        lo, hi = spec.get('start', -math.inf), spec.get('end', math.inf)
        y = [v for t, v in zip(series['t'], series['values']) if lo <= t <= hi]
        if not y:
            raise KernelError(f'{name}: expectation window contains no samples')
        reduction = spec.get('reduction', 'max_abs')
        if reduction == 'rmse':
            if 'target' not in spec:
                raise KernelError(f'{name}: rmse requires a target')
            value = math.sqrt(sum((v - spec['target']) ** 2 for v in y) / len(y))
        elif reduction == 'max_abs': value = max(abs(v) for v in y)
        elif reduction == 'max': value = max(y)
        elif reduction == 'min': value = min(y)
        elif reduction == 'final': value = y[-1]
        elif reduction == 'mean': value = sum(y) / len(y)
        elif reduction == 'rms': value = math.sqrt(sum(v * v for v in y) / len(y))
        else: raise KernelError(f'{name}: unknown reduction {reduction}')
        metrics.append({**spec, 'name': name, 'reduction': reduction, 'value': value, 'samples': len(y),
                        'passed': spec.get('min', -math.inf) <= value <= spec.get('max', math.inf)})
    return {'status': ('passed' if all(m['passed'] for m in metrics) else 'failed') if metrics else 'unchecked',
            'convention': 'sample statistics; time in seconds', 'metrics': metrics}


def compare(baseline, candidate):
    """Align common signals to candidate samples over their shared interval."""
    if baseline.get('preflight') or candidate.get('preflight'):
        raise KernelError('Build-only checks have no simulation samples to compare')
    a, b = signals(baseline), signals(candidate)
    by_identity = {series['identity']: (key, series) for key, series in a.items()}
    differences = {}
    for key in sorted(b):
        right = b[key]
        if right['identity'] not in by_identity: continue
        baseline_key, left = by_identity[right['identity']]
        if left['unit'] != right['unit'] or left['unit'] == 'unknown':
            differences[key] = {'comparable': False, 'reason': 'units differ or are unknown'}
            continue
        if not left['t'] or not right['t']:
            continue
        t = [v for v in right['t'] if left['t'][0] <= v <= left['t'][-1]]
        delta = [value_at(right, v) - value_at(left, v) for v in t]
        differences[key] = {'comparable': bool(t), 'unit': right['unit'], 't': t, 'delta': delta,
                            'baseline_signal': baseline_key, 'identity': right['identity'],
                            'rms_delta': math.sqrt(sum(v*v for v in delta) / len(delta)) if delta else None,
                            'max_abs_delta': max(map(abs, delta)) if delta else None}
    changed = {}
    for field in ('source_hash', 'parameters_hash', 'controller_hash', 'physical_hash',
                  'component_graph_hash', 'cad_derivation_hash', 'binary_hash', 'seed'):
        x, y = baseline.get('provenance', {}).get(field), candidate.get('provenance', {}).get(field)
        if x != y: changed[field] = {'baseline': x, 'candidate': y}
    same_scenario = all(baseline.get('provenance', {}).get(k) == candidate.get('provenance', {}).get(k)
                        for k in ('source_hash', 'parameters_hash', 'seed'))
    same_fidelity = baseline.get('settings') == candidate.get('settings')
    same_interface = baseline.get('controller_interface') == candidate.get('controller_interface')
    objectives = {}
    for name, right in candidate.get('objectives', {}).items():
        left = baseline.get('objectives', {}).get(name)
        if left is None: continue
        comparable = left['unit'] == right['unit'] and left.get('definition') == right.get('definition')
        objectives[name] = {'comparable': comparable, 'unit': right['unit'],
            'baseline': left['value'], 'candidate': right['value'],
            'delta': right['value']-left['value'] if comparable else None,
            'reason': None if comparable else 'Units or objective definitions differ'}
    return {'baseline': baseline['run_id'], 'candidate': candidate['run_id'],
            'same_scenario': same_scenario, 'same_settings': same_fidelity, 'same_controller_interface': same_interface,
            'caveats': ([] if same_scenario else ['System source, scenario parameters or seed differ']) +
                       ([] if same_fidelity else ['Simulation settings differ']) +
                       ([] if same_interface else ['Controller interfaces differ']),
            'alignment': 'candidate sample times over shared interval; linear plant traces, held controller samples',
            'changed_inputs': changed, 'signals': differences,
            'objectives': objectives,
            'baseline_evaluation': baseline.get('evaluation'), 'candidate_evaluation': candidate.get('evaluation')}
