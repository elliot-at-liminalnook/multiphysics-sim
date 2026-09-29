"""Timing-belt drives derived from CAD geometry (explicit geometry → physics).

A belt drive in a document is a set of pulley bodies carrying
``robot['belt_pulley']`` metadata::

    {"drive": "turntable", "order": 0, "teeth": 280, "side": "teeth"}   # toothed, inside the loop
    {"drive": "turntable", "order": 2, "contact_diameter": 12.0, "side": "back"}  # smooth idler on the belt's back

``order`` is the pulley's place along the belt, travelling counter-clockwise
seen from +Z. Pulley centres are measured from the bodies (the centre of the
body's bounding box in XY), never taken from the script that built them; the
tooth count comes from the metadata and is cross-checked against the body's
measured tip radius.

:func:`belt_path` does the geometry: tangent spans and wrap angles for circles
with signed pitch radii (+ inside the loop, − for a back-side idler), so the
belt's pitch length, its free spans and the teeth in mesh follow exactly.
:func:`derive_belt_drive` turns a drive into the parameters of the
``part.belt_drive`` simulation part, each with how it was obtained, and
:func:`export_physics` writes the ``sim.cad-physics/1`` record that
``sim-system cad-params`` applies.

Belt data (GT2, 2 mm pitch): pitch-line differential 0.254 mm and belt
thickness 1.38 mm are the Gates 2GT/LL-2GT nominal values. The tensile
stiffness EA is an estimate, not a measurement; see ``GT2['ea_per_mm_width']``.
"""
from __future__ import annotations

import hashlib
import json
import math
import os
from typing import Optional

from .document import Document

SCHEMA = 'sim.cad-physics/1'
GT2 = {
    'pitch': 2.0,             # mm
    'pld': 0.254,             # mm, pitch line below the tooth tips (Gates nominal)
    'belt_thickness': 1.38,   # mm, over the teeth (Gates LL-2GT nominal)
    # Tensile stiffness EA per mm of belt width (N per unit strain per mm).
    # Estimated: glass-cord GT2 belts are hyper-elastic (stiffness rises with
    # load; Robotics 2018, 7(4), 75), so a single value holds only near one
    # tension. Replace with a measured value for the belt in use.
    'ea_per_mm_width': 2500.0,
    'ea_uncertainty': 0.5,    # relative
}


def pitch_radius(teeth: int, pitch: float = GT2['pitch']) -> float:
    """Pitch radius (mm) of a toothed pulley: circumference = teeth × pitch."""
    return teeth * pitch / (2.0 * math.pi)


def tip_radius(teeth: int, pitch: float = GT2['pitch'], pld: float = GT2['pld']) -> float:
    return pitch_radius(teeth, pitch) - pld


def toothed_outline(teeth: int, pitch: float = GT2['pitch'], pld: float = GT2['pld'], depth: float = 0.75,
                    groove: float = 1.3, samples: int = 7) -> list[tuple[float, float]]:
    """Closed outline (mm, centred at the origin) of a GT2 pulley.

    Grooves are rounded, ``depth`` deep and ``groove`` wide at the tip circle:
    an approximation of the 2GT profile that prints well at 0.4 mm. Print a
    fit coupon before trusting it with load.
    """
    ro = tip_radius(teeth, pitch, pld)
    half = (groove / 2.0) / ro
    pts = []
    for k in range(teeth):
        c = 2.0 * math.pi * k / teeth
        for i in range(samples):
            s = -1.0 + 2.0 * i / (samples - 1)
            r = ro - depth * math.sqrt(max(0.0, 1.0 - s * s))
            a = c + s * half
            pts.append((r * math.cos(a), r * math.sin(a)))
    return pts


def _rot(v, a):
    c, s = math.cos(a), math.sin(a)
    return (c * v[0] - s * v[1], s * v[0] + c * v[1])


def belt_path(pulleys: list[dict]) -> dict:
    """Belt geometry around circles in travel order (counter-clockwise from +Z).

    Each pulley is ``{"center": (x, y), "radius": r}`` in mm with the pitch
    radius signed: positive when the pulley is inside the loop (the belt turns
    left around it), negative for a back-side idler outside the loop.
    Returns spans (tangent points and free lengths), wrap angles and the total
    pitch length.
    """
    n = len(pulleys)
    if n < 2:
        raise ValueError('a belt needs at least two pulleys')
    spans = []
    for i in range(n):
        a, b = pulleys[i], pulleys[(i + 1) % n]
        dx, dy = b['center'][0] - a['center'][0], b['center'][1] - a['center'][1]
        d = math.hypot(dx, dy)
        s = b['radius'] - a['radius']
        if d <= abs(s) + 1e-9:
            raise ValueError(f'pulleys {i} and {(i + 1) % n} overlap: no belt span between them')
        length = math.sqrt(d * d - s * s)
        u = _rot((dx / d, dy / d), -math.atan2(s, length))
        nrm = (-u[1], u[0])
        p = (a['center'][0] - a['radius'] * nrm[0], a['center'][1] - a['radius'] * nrm[1])
        q = (b['center'][0] - b['radius'] * nrm[0], b['center'][1] - b['radius'] * nrm[1])
        spans.append({'from': i, 'to': (i + 1) % n, 'start': p, 'end': q, 'direction': u, 'length': length})
    wraps = []
    for i in range(n):
        u_in, u_out = spans[i - 1]['direction'], spans[i]['direction']
        turn = math.atan2(u_in[0] * u_out[1] - u_in[1] * u_out[0], u_in[0] * u_out[0] + u_in[1] * u_out[1])
        r = pulleys[i]['radius']
        wrap = (turn if r > 0 else -turn) % (2.0 * math.pi)
        wraps.append({'pulley': i, 'angle': wrap, 'arc': abs(r) * wrap})
    total = sum(s['length'] for s in spans) + sum(w['arc'] for w in wraps)
    return {'spans': spans, 'wraps': wraps, 'length': total}


def solve_slot(pulleys: list[dict], index: int, origin, direction, length: float, travel=(0.0, 60.0)) -> float:
    """Distance along a slot (from ``origin`` in ``direction``) that puts
    pulley ``index`` where the belt path has the given pitch length."""
    def path_at(t):
        moved = [dict(p) for p in pulleys]
        moved[index]['center'] = (origin[0] + t * direction[0], origin[1] + t * direction[1])
        return belt_path(moved)['length'] - length
    lo, hi = travel
    flo, fhi = path_at(lo), path_at(hi)
    if flo * fhi > 0:
        raise ValueError(f'belt length {length:.1f} mm is outside the slot range '
                         f'({flo + length:.1f}–{fhi + length:.1f} mm)')
    for _ in range(80):
        mid = 0.5 * (lo + hi)
        fm = path_at(mid)
        if (fm > 0) == (fhi > 0):
            hi, fhi = mid, fm
        else:
            lo, flo = mid, fm
    return 0.5 * (lo + hi)


def _pulleys(doc: Document, drive: str):
    found = []
    for n in doc.nodes.values():
        meta = (n.robot or {}).get('belt_pulley')
        if n.body is None or not meta or meta.get('drive') != drive:
            continue
        lo, hi = doc.kernel.bounding_box(n.body) if hasattr(doc.kernel, 'bounding_box') else doc.mesh_of(n.id).bounds()
        centre = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0)
        found.append((meta['order'], n, meta, centre, (hi[0] - lo[0]) / 2.0))
    if len(found) < 2:
        raise ValueError(f'belt drive {drive!r} needs at least two pulley bodies with belt_pulley metadata')
    found.sort(key=lambda f: f[0])
    return found


def derive_belt_drive(doc: Document, drive: str, belt_width: float = 6.0) -> dict:
    """Measure a belt drive and return {parameter: {value, unit, provenance}} for ``part.belt_drive``."""
    found = _pulleys(doc, drive)
    circles, measured, checks = [], [], {}
    back_offset = GT2['belt_thickness'] - GT2['pld']
    for order, node, meta, centre, half_width in found:
        if meta.get('side', 'teeth') == 'teeth':
            r = pitch_radius(int(meta['teeth']))
            # The body's outer radius includes flanges; the tip circle must fit inside it.
            checks[node.name] = {'teeth': meta['teeth'], 'pitch_radius_mm': r, 'body_half_width_mm': half_width,
                                 'fits': half_width >= tip_radius(int(meta['teeth'])) - 0.05}
        else:
            r = -(meta['contact_diameter'] / 2.0 + back_offset)
            checks[node.name] = {'contact_diameter_mm': meta['contact_diameter'], 'belt_back_offset_mm': back_offset}
        circles.append({'center': centre, 'radius': r})
        measured.append({'name': node.name, 'id': node.id, 'center_mm': list(centre), 'signed_pitch_radius_mm': r,
                         'teeth': meta.get('teeth'), 'role': meta.get('role')})
    path = belt_path(circles)
    roles = {m['role']: i for i, m in enumerate(measured)}
    if 'driver' not in roles or 'driven' not in roles:
        raise ValueError(f"belt drive {drive!r}: mark one pulley role 'driver' and one 'driven'")
    i_drv, i_out = roles['driver'], roles['driven']
    r_drv, r_out = circles[i_drv]['radius'], circles[i_out]['radius']
    # The two belt runs between driver and driven act in parallel once the belt
    # is pretensioned; each run is the free spans along it in series.
    n = len(circles)
    run_a = run_b = 0.0
    i = i_drv
    while i != i_out:
        run_a += path['spans'][i]['length']; i = (i + 1) % n
    while i != i_drv:
        run_b += path['spans'][i]['length']; i = (i + 1) % n
    ea = GT2['ea_per_mm_width'] * belt_width
    k = ea / (run_a * 1e-3) + ea / (run_b * 1e-3)
    wrap_drv = path['wraps'][i_drv]['angle']
    teeth_drv = measured[i_drv]['teeth']
    in_mesh = teeth_drv * wrap_drv / (2 * math.pi)
    derived = lambda value, unit, rule, **inputs: {'value': value, 'unit': unit, 'provenance': {'kind': 'derived', 'rule': rule, 'inputs': inputs}}
    params = {
        'radius_driver': derived(r_drv * 1e-3, 'm', 'r = teeth × pitch / 2π', teeth=teeth_drv, pitch_mm=GT2['pitch']),
        'radius_driven': derived(r_out * 1e-3, 'm', 'r = teeth × pitch / 2π', teeth=measured[i_out]['teeth'], pitch_mm=GT2['pitch']),
        'stiffness': {'value': k, 'unit': 'N/m', 'uncertainty': GT2['ea_uncertainty'] * k, 'provenance': {
            'kind': 'estimated', 'rule': 'k = EA/L_run1 + EA/L_run2 (pretensioned runs in parallel; free spans in series; tooth compliance neglected)',
            'inputs': {'EA_N': ea, 'EA_source': 'estimated GT2 glass-cord value per mm width; measure the belt', 'run1_mm': run_a, 'run2_mm': run_b}}},
        'damping': {'value': 2e-4 * k, 'unit': 'N·s/m', 'provenance': {'kind': 'estimated', 'rule': 'c = 2e-4 s × k (light rubber damping); not measured'}},
    }
    # Pretension is set when the belt is tensioned: declared on a pulley in CAD, never assumed here.
    tension = [m for _, _, m, _, _ in found if m.get('pretension_n') is not None]
    if not tension:
        raise ValueError(f"belt drive {drive!r}: declare the belt's static tension per run as belt_pulley.pretension_n on one pulley (N)")
    t = tension[0]
    params['pretension'] = {'value': float(t['pretension_n']), 'unit': 'N', 'uncertainty': float(t.get('pretension_uncertainty', 0.5)) * float(t['pretension_n']),
                            'provenance': {'kind': t.get('pretension_provenance', 'estimated'), 'rule': t.get('pretension_source', 'declared on the pulley in CAD')}}
    summary = {'pitch_length_mm': path['length'], 'ratio': r_out / r_drv,
               'driver_wrap_deg': math.degrees(wrap_drv), 'driver_teeth_in_mesh': in_mesh,
               'spans_mm': [s['length'] for s in path['spans']],
               'wraps_deg': [math.degrees(w['angle']) for w in path['wraps']]}
    checks['driver_teeth_in_mesh_ok'] = in_mesh >= 6.0
    return {'component_type': 'part.belt_drive', 'parameters': params, 'pulleys': measured, 'belt': summary,
            'checks': checks, 'path': {'spans': [{k2: v for k2, v in s.items() if k2 != 'direction'} for s in path['spans']],
                                       'wraps_deg': summary['wraps_deg']}}


def export_physics(doc: Document, rcad_path: str, out_path: str, drive: str, belt_width: float = 6.0) -> dict:
    """Derive and write ``*.physics.json`` referencing the saved CAD file."""
    derived = derive_belt_drive(doc, drive, belt_width)
    with open(rcad_path, 'rb') as f:
        sha = hashlib.sha256(f.read()).hexdigest()
    repo = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
    full = os.path.abspath(rcad_path)
    shown = os.path.relpath(full, repo) if full.startswith(repo + os.sep) else full
    record = {'schema': SCHEMA, 'cad': {'path': shown, 'sha256': sha}, **derived}
    with open(out_path, 'w') as f:
        json.dump(record, f, indent=1)
        f.write('\n')
    return record
