"""Worm-gear physics derived from CAD geometry (explicit geometry → physics).

A worm set in a document is a group of bodies named by role:

* ``worm root``   – the worm's core cylinder (axis = the group's Z axis)
* ``worm thread`` – thread bodies (any number; a stepped helix is fine)
* ``wheel rim``   – the wheel's core disc
* ``wheel tooth`` – one body per tooth

:func:`derive_worm_gear` measures the geometry and returns the parameters of
``rotational.worm_gear`` with, for each one, how it was obtained (rule and the
measured inputs). Nothing is taken from the script that built the geometry:
edit the bodies and the physics follows. Standard proportions are used to go
from radii to module (addendum 1·m, dedendum 1.25·m) and are recorded as such.

:func:`export_physics` writes a ``*.physics.json`` with the CAD file's SHA-256,
which ``sim-system datasheet rotational.worm_gear --parameters FILE`` benches.
"""
from __future__ import annotations

import hashlib
import json
import math
import os
from typing import Optional

from .document import Document

SCHEMA = 'sim.cad-physics/1'
# Sliding friction for lubricated worm pairs (Shigley Fig. 15-11 range at low
# sliding speed). An estimate until the pair is measured.
PAIR_FRICTION = {('steel', 'brass'): 0.07, ('steel', 'steel'): 0.10, ('steel', 'nylon'): 0.12}


def _bodies(doc: Document, role: str, group: Optional[str]):
    out = []
    for n in doc.nodes.values():
        if n.body is None or not n.name.lower().startswith(role):
            continue
        if group is not None and n.parent != group:
            continue
        out.append(n)
    return out


def _radial(doc: Document, nodes, axis_xy=(0.0, 0.0)):
    """(min, max) radial extent of the bodies about the Z axis, mm.

    A tooth or thread body is measured along its own radial direction (from
    the axis through its centroid), so the corners of a flat-topped tooth
    do not count as extra height. Bodies centred on the axis (a core
    cylinder) use plain vertex distance.
    """
    rs = []
    for n in nodes:
        c = doc.kernel.mass_properties(n.body).centroid
        ux, uy = c[0] - axis_xy[0], c[1] - axis_xy[1]
        norm = math.hypot(ux, uy)
        for v in doc.kernel.vertices(n.body):
            x, y, _ = v.point
            dx, dy = x - axis_xy[0], y - axis_xy[1]
            rs.append((dx * ux + dy * uy) / norm if norm > 1e-9 else math.hypot(dx, dy))
    return min(rs), max(rs)


def _helix(samples) -> tuple[float, int]:
    """(axial pitch mm, number of starts) of thread bodies given as (angle, z).

    The axial pitch is the z spacing of thread bodies at the same angle. With
    ``z₁`` starts the lead is ``z₁·p``, so every body's helical phase
    ``z − hand·z₁·p·angle/2π`` agrees modulo ``p`` only for the true start
    count and hand; they are picked by that phase coherence. Start counts are
    searched below half the number of distinct body angles, where no other
    count or hand can alias the phase.
    """
    by_angle: dict = {}
    for a, z in samples:
        by_angle.setdefault((round(math.cos(a), 3), round(math.sin(a), 3)), []).append(z)
    gaps = []
    for zs in by_angle.values():
        zs.sort()
        gaps += [b - a for a, b in zip(zs, zs[1:]) if b - a > 1e-6]
    if not gaps or len(by_angle) < 3:
        raise ValueError('worm thread bodies must repeat along the worm at several angles to measure its pitch')
    gaps.sort()
    pitch = gaps[len(gaps) // 2]
    best = None
    for z1 in range(1, max(2, (len(by_angle) + 1) // 2)):
        for hand in (1.0, -1.0):
            phases = [2 * math.pi * (z - hand * z1 * pitch * a / (2 * math.pi)) / pitch for a, z in samples]
            coherence = math.hypot(sum(map(math.cos, phases)), sum(map(math.sin, phases))) / len(phases)
            if best is None or coherence > best[0] + 1e-9:
                best = (coherence, z1)
    if best[0] < 0.9:
        raise ValueError(f'worm thread bodies do not lie on a regular helix (phase coherence {best[0]:.2f})')
    return pitch, best[1]


def _material(doc: Document, nodes) -> str:
    ids = {getattr(n, 'material', None) or 'pla' for n in nodes}
    return sorted(ids)[0]


def derive_worm_gear(doc: Document, group: Optional[str] = None) -> dict:
    """Measure a worm set and return {parameter: {value, unit, provenance}}."""
    root = _bodies(doc, 'worm root', group)
    thread = _bodies(doc, 'worm thread', group)
    rim = _bodies(doc, 'wheel rim', group)
    teeth = _bodies(doc, 'wheel tooth', group)
    if not (root and thread and rim and teeth):
        raise ValueError('a worm set needs bodies named worm root, worm thread, wheel rim and wheel tooth')
    # Worm radii about the core's own axis (the worm runs along Z, anywhere in
    # XY): root from the core cylinder, tip from the thread's outermost vertices.
    worm_axis = doc.kernel.mass_properties(root[0].body).centroid
    wx, wy = worm_axis[0], worm_axis[1]
    _, r_root = _radial(doc, root, axis_xy=(wx, wy))
    _, r_tip = _radial(doc, thread, axis_xy=(wx, wy))
    module = (r_tip - r_root) / 2.25
    d1 = 2.0 * (r_tip - module)
    # Lead: axial advance per turn of one thread start.
    samples = []
    for n in thread:
        c = doc.kernel.mass_properties(n.body).centroid
        samples.append((math.atan2(c[1] - wy, c[0] - wx), c[2]))
    axial_pitch, thread_count = _helix(samples)
    lead = axial_pitch * thread_count
    starts = lead / (math.pi * module)
    # Wheel: tooth count, and its tip radius as a cross-check of the module.
    z2 = len(teeth)
    wheel_axis = doc.kernel.mass_properties(rim[0].body).centroid
    _, r_wheel_tip = _radial(doc, teeth, axis_xy=(wheel_axis[0], wheel_axis[1]))
    module_wheel = r_wheel_tip / (z2 / 2.0 + 1.0)
    pair = (_material(doc, root + thread), _material(doc, rim + teeth))
    mu = PAIR_FRICTION.get(pair, PAIR_FRICTION.get((pair[1], pair[0])))
    derived = lambda value, unit, rule, **inputs: {'value': value, 'unit': unit, 'provenance': {'kind': 'derived', 'rule': rule, 'inputs': inputs}}
    out = {
        'module': derived(module * 1e-3, 'm', 'm = (r_tip − r_root) / 2.25 (addendum m, dedendum 1.25 m)', r_tip_mm=r_tip, r_root_mm=r_root),
        'worm_pitch_diameter': derived(d1 * 1e-3, 'm', 'd₁ = 2·(r_tip − m)', r_tip_mm=r_tip, module_mm=module),
        'worm_starts': derived(round(starts), '1', 'z₁ = lead / (π·m), lead from the thread helix', lead_mm=lead, module_mm=module, raw=starts),
        'wheel_teeth': derived(z2, '1', 'z₂ = number of wheel tooth bodies', teeth=z2),
        'pressure_angle': {'value': math.radians(20.0), 'unit': 'rad', 'provenance': {'kind': 'declared', 'rule': 'standard 20° normal pressure angle (not measurable from the simplified tooth bodies)'}},
    }
    if mu is not None:
        out['friction'] = {'value': mu, 'unit': '1', 'provenance': {'kind': 'estimated', 'rule': f'lubricated {pair[0]} on {pair[1]} (material pair table)', 'inputs': {'pair': list(pair)}}}
    checks = {'wheel_module_mm': module_wheel, 'worm_module_mm': module, 'agree': abs(module_wheel - module) <= 0.05 * module, 'starts_raw': starts}
    return {'component_type': 'rotational.worm_gear', 'parameters': out, 'checks': checks}


def export_physics(doc: Document, rcad_path: str, out_path: str, group: Optional[str] = None) -> dict:
    """Derive and write ``*.physics.json`` referencing the saved CAD file."""
    derived = derive_worm_gear(doc, group)
    if not derived['checks']['agree']:
        raise ValueError(f"worm and wheel disagree on the module: {derived['checks']}")
    with open(rcad_path, 'rb') as f:
        sha = hashlib.sha256(f.read()).hexdigest()
    # Paths are recorded relative to the repository root, like every other artifact reference.
    repo = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
    full = os.path.abspath(rcad_path)
    shown = os.path.relpath(full, repo) if full.startswith(repo + os.sep) else full
    record = {'schema': SCHEMA, 'cad': {'path': shown, 'sha256': sha}, **derived}
    with open(out_path, 'w') as f:
        json.dump(record, f, indent=1)
        f.write('\n')
    return record


def build_worm_set(doc: Document, module: float, starts: int, teeth: int, worm_pitch_diameter: float, centre=(0.0, 0.0, 0.0)):
    """Model a worm set (mm) with the body roles derive_worm_gear reads.

    The worm lies along Z at `centre`; the wheel sits beside it (its own
    Z axis offset by the centre distance). Thread and teeth are simplified
    prismatic bodies with correct radii, lead and count.
    """
    from .commands import Ops
    ops = Ops(doc)
    m, d1 = module, worm_pitch_diameter
    r_root, r_tip = d1 / 2 - 1.25 * m, d1 / 2 + m
    length = max(6 * math.pi * m * starts, 12 * m)
    ids = []
    core = ops.cylinder((0, 0, -length / 2), (0, 0, 1), r_root, length, name='worm root')
    ids.append(core)
    per_turn = 16
    lead = math.pi * m * starts
    steps = int(length / lead * per_turn)
    # Start s is start 0 shifted one axial pitch (lead / starts) per start at
    # the same angles; also rotating it would land it back on start 0's helix.
    for s in range(starts):
        for k in range(steps):
            z = -length / 2 + k * lead / per_turn + s * lead / starts
            if z > length / 2 - 0.5 * m:
                break
            b = ops.box((r_root - 0.05 * m, -0.9 * m, z - 0.4 * m), (r_tip - r_root + 0.05 * m, 1.8 * m, 0.8 * m), name='worm thread')
            ops.transform([b], axis=(0, 0, 1), angle_deg=360.0 * k / per_turn, center=(0, 0, 0))
            ids.append(b)
    ops.set_material([i for i in ids], 'steel')
    r2 = teeth * m / 2
    a = r2 + d1 / 2
    cx = a
    wheel_ids = [ops.cylinder((cx, 0, -1.5 * m), (0, 0, 1), r2 - 1.25 * m, 3 * m, name='wheel rim')]
    for k in range(teeth):
        b = ops.box((cx + r2 - 1.3 * m, -0.6 * m, -1.4 * m), (2.3 * m, 1.2 * m, 2.8 * m), name='wheel tooth')
        ops.transform([b], axis=(0, 0, 1), angle_deg=360.0 * k / teeth, center=(cx, 0, 0))
        wheel_ids.append(b)
    ops.set_material(wheel_ids, 'brass')
    if any(centre):
        ops.transform(ids + wheel_ids, translation=tuple(centre))
    return ids + wheel_ids
