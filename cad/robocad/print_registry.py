"""The print registry (``library/printing/registry.json``), read by CAD.

One source for printer build volumes, filament properties and printed-joint
geometry. CAD reads the same file as the Rust stress check and planner and
records its SHA-256, so a result can always be traced to the values it used.
Values change only by editing the registry (estimates) or by promoting coupon
measurements with ``sim-print promote``; never copy numbers from here.
"""

from __future__ import annotations

import hashlib
import json
import os
from typing import Optional

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
SCHEMA = 'sim.print-registry/1'
_cache: dict[str, tuple[float, dict, str]] = {}


def default_path() -> str:
    return os.environ.get('SIM_PRINT_REGISTRY') or os.path.join(REPO, 'library', 'printing', 'registry.json')


def load(path: Optional[str] = None) -> tuple[dict, str]:
    """(registry, sha256 of the file bytes); cached until the file changes."""
    path = path or default_path()
    mtime = os.path.getmtime(path)
    hit = _cache.get(path)
    if hit and hit[0] == mtime:
        return hit[1], hit[2]
    with open(path, 'rb') as f:
        raw = f.read()
    data = json.loads(raw)
    if data.get('schema') != SCHEMA:
        raise ValueError(f'{path}: schema must be {SCHEMA!r}')
    sha = hashlib.sha256(raw).hexdigest()
    _cache[path] = (mtime, data, sha)
    return data, sha


def value(q) -> float:
    return q['value'] if isinstance(q, dict) and 'value' in q else q


def printer(printer_id: str, path: Optional[str] = None) -> dict:
    reg, _ = load(path)
    if printer_id not in reg['printers']:
        raise KeyError(f"printer {printer_id!r} is not in the print registry (have: {', '.join(reg['printers'])})")
    return reg['printers'][printer_id]


def usable_mm(printer_id: str, path: Optional[str] = None) -> tuple[float, float, float]:
    """The box one piece must fit in: build volume less the edge margin in x and y."""
    p = printer(printer_id, path)
    b, m = value(p['build_mm']), value(p['margin_mm'])
    return (b[0] - 2 * m, b[1] - 2 * m, b[2])


def material(material_id: str, path: Optional[str] = None) -> dict:
    reg, _ = load(path)
    if material_id not in reg['materials']:
        raise KeyError(f"material {material_id!r} is not in the print registry (have: {', '.join(reg['materials'])})")
    return reg['materials'][material_id]


def for_cad_material(cad_id: str, path: Optional[str] = None) -> Optional[tuple[str, dict]]:
    """The registry filament backing a CAD material id (``pla`` → ``pla-basic``), if any."""
    try:
        reg, _ = load(path)
    except (OSError, ValueError):
        return None
    for k, m in reg['materials'].items():
        if m.get('cad_material') == cad_id:
            return k, m
    return None


def joint(path_: str, path: Optional[str] = None):
    """A joint value by dotted path (``dowel_pin.slip_clearance_mm``), numbers unwrapped."""
    reg, _ = load(path)
    v = reg['joints']
    for part in path_.split('.'):
        if part not in v:
            raise KeyError(f'joints.{path_}: {part!r} is missing from the print registry')
        v = v[part]
    return value(v)


def insert(size: str, path: Optional[str] = None) -> dict:
    """Heat-set insert geometry (hole, depth, knurl, length; mm)."""
    inserts = joint('heat_set_insert', path)
    if size not in inserts:
        raise KeyError(f'joints.heat_set_insert has no {size!r} in the print registry')
    return dict(inserts[size])


def screw(size: str, path: Optional[str] = None) -> dict:
    s = joint('screw', path)
    return {'clearance_mm': s['clearance_mm'][size], 'head_mm': s['head_mm'][size], 'counterbore_mm': tuple(s['counterbore_mm'][size]),
            'lengths_mm': list(s['lengths_mm']), 'proof_load_n': s['proof_load_n'][size]}


def cad_engineering(cad_id: str, path: Optional[str] = None) -> Optional[dict]:
    """Engineering properties for a CAD material from its registry filament:
    stiffness and strength along the layers, and the print anisotropy as the
    registry's across/along ratios. None when no filament backs the id."""
    found = for_cad_material(cad_id, path)
    if not found:
        return None
    key, m = found
    _, sha = load(path)
    e_in, e_across = value(m['modulus_in_layer']), value(m['modulus_across_layers'])
    s_in, s_across = value(m['tensile_in_layer']), value(m['tensile_across_layers'])
    return {
        'youngs_modulus': e_in,
        'poisson': value(m['poisson']),
        'yield_strength': s_in,
        'ultimate_strength': s_in,
        'glass_transition_c': value(m['glass_transition_c']),
        # CAD's bearing value is an allowable: the registry's failure pressure ÷ 3.
        'bearing_pressure': value(m['bearing']) / 3.0,
        'print': {'anisotropy_z': e_across / e_in, 'layer_adhesion_factor': s_across / s_in,
                  'registry_material': key, 'registry_sha256': sha},
    }
