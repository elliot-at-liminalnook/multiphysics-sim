"""Typed, bounded component parameters and a non-executable expression grammar."""
from __future__ import annotations

import ast
from copy import deepcopy
from dataclasses import dataclass
import math
import re

from .kernel import KernelError

# CAD geometry uses millimetres and degrees. Other quantities retain SI units.
UNITS = {
    '1': (1., (0, 0, 0, 0)),
    'mm': (1., (1, 0, 0, 0)), 'cm': (10., (1, 0, 0, 0)), 'm': (1000., (1, 0, 0, 0)),
    'deg': (1., (0, 1, 0, 0)), 'rad': (180/math.pi, (0, 1, 0, 0)),
    'kg': (1., (0, 0, 1, 0)), 'g': (.001, (0, 0, 1, 0)),
    's': (1., (0, 0, 0, 1)),
}
ZERO = (0, 0, 0, 0)
NAME = re.compile(r'^[A-Za-z_][A-Za-z_0-9]*$')


@dataclass(frozen=True)
class Quantity:
    value: float
    dimension: tuple


def finite(value):
    if type(value) not in (int, float) or not math.isfinite(value):
        raise KernelError('Component values must be finite numbers')
    return float(value)


def expression(value, unit, parameters=None):
    """Return a value in the requested unit, checking dimensional arithmetic.

    A bare numeric expression takes the field's unit. Expressions referencing
    parameters retain their dimensions; e.g. length/width cannot be a length.
    """
    if unit not in UNITS:
        raise KernelError(f'Unknown component unit: {unit}')
    scale, dimension = UNITS[unit]
    if type(value) in (int, float):
        return finite(value)
    if not isinstance(value, str) or len(value) > 1024:
        raise KernelError('Expected a number or arithmetic expression')
    text = re.sub(r'(?<=[0-9.)])\s*(mm|cm|deg|rad|kg|m|g|s)\b', r'*\1', value.strip())
    try:
        tree = ast.parse(text, mode='eval')
    except (SyntaxError, ValueError) as error:
        raise KernelError(f'Invalid component expression: {value}') from error
    if len(list(ast.walk(tree))) > 100:
        raise KernelError('Component expression is too complex')
    parameters = parameters or {}
    typed = False
    def visit(node):
        nonlocal typed
        if isinstance(node, ast.Constant):
            return Quantity(finite(node.value), ZERO)
        if isinstance(node, ast.Name):
            if node.id in parameters:
                typed = True
                return parameters[node.id]
            if node.id in UNITS and node.id != '1':
                typed = True
                return Quantity(*UNITS[node.id])
            if node.id in ('pi', 'tau'):
                return Quantity(getattr(math, node.id), ZERO)
            raise KernelError(f'Unknown parameter: {node.id}')
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, (ast.UAdd, ast.USub)):
            q = visit(node.operand)
            return Quantity(q.value if isinstance(node.op, ast.UAdd) else -q.value, q.dimension)
        if isinstance(node, ast.BinOp):
            a, b = visit(node.left), visit(node.right)
            if isinstance(node.op, (ast.Add, ast.Sub)):
                if a.dimension != b.dimension:
                    # Zero is safe in any dimension (useful for coordinates).
                    if a.value == 0 and a.dimension == ZERO: a = Quantity(0., b.dimension)
                    elif b.value == 0 and b.dimension == ZERO: b = Quantity(0., a.dimension)
                    else: raise KernelError('Cannot add component values with different units')
                return Quantity(a.value + (b.value if isinstance(node.op, ast.Add) else -b.value), a.dimension)
            if isinstance(node.op, ast.Mult):
                return Quantity(a.value*b.value, tuple(x+y for x, y in zip(a.dimension, b.dimension)))
            if isinstance(node.op, ast.Div):
                if b.value == 0: raise KernelError('Division by zero in component expression')
                return Quantity(a.value/b.value, tuple(x-y for x, y in zip(a.dimension, b.dimension)))
            if isinstance(node.op, ast.Pow):
                if b.dimension != ZERO or b.value != int(b.value) or abs(b.value) > 4:
                    raise KernelError('Exponents must be integers between -4 and 4')
                try:
                    return Quantity(a.value**int(b.value), tuple(x*int(b.value) for x in a.dimension))
                except (ZeroDivisionError, OverflowError) as error:
                    raise KernelError('Invalid exponent in component expression') from error
        raise KernelError('Only numbers, parameters, units and arithmetic are allowed')
    result = visit(tree.body)
    if not typed and result.dimension == ZERO:
        return finite(result.value)
    if result.dimension != dimension:
        if result.value == 0 and result.dimension == ZERO: return 0.
        raise KernelError(f'Expression does not have units of {unit}')
    return finite(result.value/scale)


def validate_parameters(specs, overrides=None):
    if not isinstance(specs, dict) or not isinstance(overrides or {}, dict):
        raise KernelError('Component parameters and overrides must be objects')
    overrides = overrides or {}
    if set(overrides) - set(specs):
        raise KernelError(f'Unknown parameter overrides: {sorted(set(overrides)-set(specs))}')
    values, quantities = {}, {}
    for name, spec in specs.items():
        if not NAME.fullmatch(name) or name in UNITS or name in ('pi', 'tau'):
            raise KernelError(f'Invalid or reserved parameter name: {name}')
        if not isinstance(spec, dict) or set(spec) - {'value', 'unit', 'min', 'max', 'description', 'provenance', 'uncertainty'}:
            raise KernelError(f'Invalid parameter specification: {name}')
        if 'value' not in spec or spec.get('unit') not in UNITS:
            raise KernelError(f'{name} requires a value and supported unit')
        if spec.get('provenance') not in ('measured', 'derived', 'estimated'):
            raise KernelError(f'{name} requires measured, derived or estimated provenance')
        unit = spec['unit']
        default = expression(spec['value'], unit)
        value = expression(overrides.get(name, spec['value']), unit)
        lo = expression(spec['min'], unit) if spec.get('min') is not None else -math.inf
        hi = expression(spec['max'], unit) if spec.get('max') is not None else math.inf
        if lo > hi or not lo <= default <= hi or not lo <= value <= hi:
            raise KernelError(f'{name} must be between {lo:g} and {hi:g} {unit}')
        if spec.get('uncertainty') is not None:
            uncertainty = spec['uncertainty']
            if not isinstance(uncertainty, dict) or set(uncertainty) != {'sigma'} or finite(uncertainty['sigma']) < 0:
                raise KernelError(f'{name}: uncertainty requires a nonnegative sigma in {unit}')
        values[name] = value
        quantities[name] = Quantity(value*UNITS[unit][0], UNITS[unit][1])
    return values, quantities


# One catalogue for the component inspector, REST and regeneration validation.
FEATURES = {
    'assembly_placement': {'label': 'Assembly placement', 'arguments': {'translation': ('mm', 3), 'axis': ('1', 3), 'angle_deg': ('deg', 1)}},
    'joint_ratio': {'label': 'Actuator-to-joint ratio', 'arguments': {'ratio': ('1', 1)}},
    'joint_home': {'label': 'Joint home angle', 'arguments': {'angle_deg': ('deg', 1)}},
    'box': {'label': 'Box', 'arguments': {'corner': ('mm', 3), 'size': ('mm', 3)}},
    'cylinder': {'label': 'Cylinder', 'arguments': {'base': ('mm', 3), 'axis': ('1', 3), 'radius': ('mm', 1), 'height': ('mm', 1)}},
    'placement': {'label': 'Part placement', 'arguments': {'translation': ('mm', 3), 'axis': ('1', 3), 'angle_deg': ('deg', 1)}},
    'joint_frame': {'label': 'Joint frame', 'arguments': {'pivot': ('mm', 3), 'axis': ('1', 3)}},
}


def feature_arguments(feature, parameters):
    kind = feature.get('kind')
    if kind not in FEATURES or set(feature) != {'node', 'kind', 'arguments'}:
        raise KernelError('A component feature requires node, kind and arguments from the catalogue')
    specs = FEATURES[kind]['arguments']
    arguments = feature['arguments']
    if not isinstance(arguments, dict) or set(arguments) != set(specs):
        raise KernelError(f'{kind} requires arguments {sorted(specs)}')
    out = {}
    for name, (unit, count) in specs.items():
        value = arguments[name]
        if count == 1:
            out[name] = expression(value, unit, parameters)
        else:
            if not isinstance(value, (list, tuple)) or len(value) != count:
                raise KernelError(f'{name} requires {count} values')
            out[name] = tuple(expression(v, unit, parameters) for v in value)
    return out
