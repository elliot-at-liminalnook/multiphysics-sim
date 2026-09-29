"""Split a part so each piece can print in its strong orientation.

A printed part is weakest across its layers. A part with members in several
directions (a post on a base, an arm on a hub) has no orientation where every
member is loaded along its layers; cutting it at the junction lets each piece
lie its own best way.

``compare`` answers "whole or split?" with the same checks as everything else:

1. the whole part: its best orientation and settings (``sim-print plan``),
   with the loads through each candidate cut;
2. each candidate cut (where the cross-section changes sharply): split with
   joints, and plan each piece on its own. The piece holding the fixtures
   carries the seam's force and moment from the whole-part solution; the
   other piece is held at its seam face (substructuring);
3. the seam's joints checked against that seam load;
4. a verdict: split when it reaches the safety target where the whole part
   does not, or when it is as strong and faster.
"""

from __future__ import annotations

import json
import math
import os
from typing import Callable, Optional

import numpy as np

from . import print_study as ps
from .print_split import AXES, SplitOptions, body_size, section, split_for_printing
from .kernel.base import v_dot, v_sub


def junction_planes(k, body, samples: int = 120, ratio: float = 3.0, inset: float = 2.0, limit: int = 3) -> list[dict]:
    """Planes just inside the smaller member where the cross-section jumps by `ratio` or more."""
    lo, hi = body_size(k, body)
    found = []
    for axis in range(3):
        n = AXES[axis]
        pos = np.linspace(lo[axis] + 0.5, hi[axis] - 0.5, samples)
        centre = [(lo[i] + hi[i]) / 2 for i in range(3)]
        areas = []
        for s in pos:
            p = list(centre)
            p[axis] = float(s)
            sec = section(k, body, tuple(p), n)
            areas.append(sec.area if sec else 0.0)
        for i in range(len(pos) - 1):
            a, b = areas[i], areas[i + 1]
            if min(a, b) <= 0:
                continue
            r = max(a, b) / min(a, b)
            # A real member: the smaller side runs on for at least 3× its width.
            small_idx = range(i + 1, len(pos)) if b < a else range(i, -1, -1)
            run = 0.0
            for j in small_idx:
                if areas[j] <= 0 or areas[j] > 1.5 * min(a, b):
                    break
                run = abs(pos[j] - pos[i + 1 if b < a else i])
            if r >= ratio and run >= 3 * math.sqrt(min(a, b)):
                # Into the smaller member by `inset`; the normal points into it.
                sign = 1.0 if b < a else -1.0
                at = float(pos[i + 1] if b < a else pos[i]) + sign * inset
                p = list(centre)
                p[axis] = at
                normal = [0.0, 0.0, 0.0]
                normal[axis] = sign
                found.append({'point': p, 'normal': normal, 'ratio': r, 'area_mm2': min(a, b),
                              'why': f'cross-section changes {r:.1f}× at {"xyz"[axis]} = {pos[i + 1]:.1f} mm'})
    found.sort(key=lambda f: -f['ratio'])
    kept = []
    for f in found:
        if all(abs(v_dot(v_sub(f['point'], g['point']), g['normal'])) > 5.0 or abs(v_dot(f['normal'], g['normal'])) < 0.9 for g in kept):
            kept.append(f)
    return kept[:limit]


def _ref_point(region: dict):
    kind, v = next(iter(region.items()))
    if kind == 'sphere':
        return v['center']
    if kind == 'box':
        return [(a + b) / 2 for a, b in zip(v['min'], v['max'])]
    if kind == 'cylinder':
        return v['base']
    if kind == 'points':
        return list(np.mean(np.asarray(v['points']), axis=0))
    if kind == 'slab':
        return v['point']
    return None  # 'below': a half-space, decided by the caller


def _side(region: dict, plane: dict) -> float:
    kind, v = next(iter(region.items()))
    if kind == 'below':
        # Everything below `height` along `axis`: which side holds most of it.
        a = np.asarray(v['axis'], float)
        return -float(np.dot(a, plane['normal'])) or 1.0
    p = _ref_point(region)
    return float(np.dot(np.subtract(p, plane['point']), plane['normal']))


def _seam_load_on(side: str, load: dict, plane: dict) -> tuple[list, float, list, list]:
    """The seam's force and moment on the `side` piece, from sim-print's section load
    (which is on the plus side from the minus side)."""
    n = np.asarray(load['normal'], float)
    f_plus = -load['tension_n'] * n + np.asarray(load['shear'], float)
    m_plus = np.asarray(load['bending'], float) + load['torsion_nm'] * n
    f, m = (f_plus, m_plus) if side == 'plus' else (-f_plus, -m_plus)
    mag = float(np.linalg.norm(f))
    direction = list(f / mag) if mag > 1e-12 else [0.0, 0.0, 1.0]
    return direction, mag, list(m), list(load['centroid'])


def compare(doc, node_id: str, part: dict, out_dir: str, printer: str = 'bambu-h2c', material: str = 'pla-basic',
            simulation: Optional[dict] = None, safety_target: float = 2.0, space: Optional[dict] = None,
            planes: Optional[list] = None, voxels: int = 30000, run: Optional[Callable] = None,
            on_progress: Optional[Callable[[float, str], None]] = None) -> dict:
    """Whole versus split. `part` is a REST-style part (fixtures and loads with resolved regions)."""
    from .print_jobs import region_from
    say = on_progress or (lambda f, m: None)
    run = run or (lambda cmd, study: ps.Run(cmd, study).start().wait())
    k = doc.kernel
    body = doc.resolved_body(node_id)
    name = part.get('name') or doc.nodes[node_id].name
    fixtures = [{'name': f['name'], 'region': region_from(doc, node_id, f['region'])} for f in part['fixtures']]
    loads = [{**l, 'region': region_from(doc, node_id, l['region'])} for l in part['loads']]
    planes = planes if planes is not None else junction_planes(k, body)
    if not planes:
        return {'recommendation': 'whole', 'why': 'no junction where the cross-section changes sharply', 'planes': []}
    say(0.05, f'{name}: whole part, {len(planes)} candidate cut(s)')
    # 1. The whole part, with the loads through each candidate cut.
    whole_dir = os.path.join(out_dir, 'whole')
    spec = ps.PartSpec(node_id, name, fixtures=fixtures, loads=loads,
                       sections=[{'name': f'cut {i + 1}', 'point': p['point'], 'normal': p['normal']} for i, p in enumerate(planes)])
    study = ps.write_study(doc, [spec], whole_dir, printer, material, simulation, safety_target, voxels, plan=space)
    whole = run('plan', study)['parts'][0]
    options = []
    for i, plane in enumerate(planes):
        say(0.3 + 0.6 * i / len(planes), f'{name}: split at {plane["why"]}')
        section = next(s for s in whole['verified']['sections'] if s['name'] == f'cut {i + 1}')
        result = split_for_printing(doc, node_id, SplitOptions(printer=printer, extra_planes=[{**plane, 'why': plane['why']}]))
        if len(result.pieces) != 2:
            options.append({'plane': plane, 'error': f'the cut gave {len(result.pieces)} pieces'})
            continue
        # Which piece is on which side of the plane.
        sides = ['plus' if v_dot(v_sub(k.mass_properties(p).centroid, plane['point']), plane['normal']) > 0 else 'minus' for p in result.pieces]
        held_side = 'plus' if sum(_side(f['region'], plane) for f in fixtures) > 0 else 'minus'
        slab = {'slab': {'point': plane['point'], 'normal': plane['normal'], 'thickness': 1.0}}
        specs = []
        for piece, side in zip(result.pieces, sides):
            mine = [l for l in loads if (_side(l['region'], plane) > 0) == (side == 'plus')]
            if side == held_side:
                d, mag, m, about = _seam_load_on(side, section['load'], plane)
                piece_loads = mine + [{'name': 'seam: the other piece', 'region': slab, 'direction': d, 'magnitude': mag, 'moment': m, 'about': about}]
                piece_fixtures = fixtures
            else:
                piece_loads, piece_fixtures = mine, [{'name': 'held at the seam', 'region': slab}]
            specs.append(ps.PartSpec(node_id, f'{name} · {"held" if side == held_side else "free"} piece', fixtures=piece_fixtures, loads=piece_loads,
                                     mesh=ps.body_mesh_of(k, piece)))
        piece_dir = os.path.join(out_dir, f'split-{i + 1}')
        study = ps.write_study(doc, specs, piece_dir, printer, material, simulation, safety_target, voxels, plan=space)
        pieces = run('plan', study)['parts']
        # 3. The seam's joints against the seam load (on the whole part).
        seam_dir = os.path.join(out_dir, f'split-{i + 1}-seam')
        seam_spec = ps.PartSpec(node_id, name, tuple(whole['chosen']['build_direction']), whole['chosen']['settings'], fixtures, loads,
                                seams=ps.seams_from_split(result))
        seams = run('analyze', ps.write_study(doc, [seam_spec], seam_dir, printer, material, simulation, safety_target, voxels))['parts'][0]['seams']
        options.append({'plane': plane, 'pieces': [{'name': p['name'], 'chosen': p['chosen'], 'safety_factor': (p.get('verified') or {}).get('safety_factor'), 'notes': p['notes']} for p in pieces],
                        'seams': [{'name': s['name'], 'safety_factor': s['safety_factor'], 'notes': s['notes']} for s in seams],
                        'hardware': result.hardware(), 'split': result.summary(k)})
    # 4. Verdict.
    w_sf = (whole.get('verified') or {}).get('safety_factor') or 0.0
    w_hours = whole['chosen']['estimate']['print_hours'] if whole.get('chosen') else math.inf
    best = None
    for o in options:
        if 'error' in o:
            continue
        sf = min([p['safety_factor'] or 0.0 for p in o['pieces']] + [s['safety_factor'] for s in o['seams']])
        hours = sum(p['chosen']['estimate']['print_hours'] for p in o['pieces'] if p['chosen'])
        o['safety_factor'], o['hours'] = sf, hours
        if best is None or (sf >= safety_target, -hours) > (best['safety_factor'] >= safety_target, -best['hours']):
            best = o
    verdict = {'whole': {'chosen': whole.get('chosen'), 'safety_factor': w_sf, 'hours': w_hours, 'notes': whole.get('notes')}, 'options': options}
    if best is None:
        verdict.update(recommendation='whole', why='no cut gave two joinable pieces')
    elif best['safety_factor'] >= safety_target and (w_sf < safety_target or best['hours'] < 0.9 * w_hours):
        why = (f"split: every piece and seam reaches {best['safety_factor']:.2f} (target {safety_target}), where the whole part reaches {w_sf:.2f}"
               if w_sf < safety_target else f"split: {best['hours']:.2f} h instead of {w_hours:.2f} h at safety {best['safety_factor']:.2f}")
        verdict.update(recommendation='split', why=why, plane=best['plane'])
    else:
        verdict.update(recommendation='whole', why=f"whole: safety {w_sf:.2f} in {w_hours:.2f} h; the best split reaches {best['safety_factor']:.2f} in {best['hours']:.2f} h")
    with open(os.path.join(out_dir, 'strength-split.json'), 'w') as f:
        json.dump(verdict, f, indent=1, default=float)
    return verdict
