"""Test coupons: small prints you break to measure what the registry estimates.

Material coupons are printed solid (the registry's strengths are for solid
material): a tensile bar lying flat (pulled along its layers) and one
standing on end (pulled across them). Joint coupons copy a split's real joints
where given: a dowel pin in a lap joint (pin bearing), a heat-set insert in
a boss pulled along the layers (the insert lesson's test), and a dovetail
tab in its slot. Every coupon has a loading hole at each end for a bolt or
S-hook.

``make_coupons`` returns coupon bodies with their print direction and
settings; ``write_coupon_kit`` lays them on plates (3MF), writes the results
template for ``sim-print promote`` and a short test protocol.
"""

from __future__ import annotations

import json
import math
import os
from dataclasses import dataclass, field
from typing import Optional

from . import print_registry as reg
from .kernel.base import Body, BooleanOp, Plane
from .kernel.sketch import Sketch

SOLID = {'walls': 3, 'infill': 1.0, 'pattern': 'gyroid', 'layer_height': 0.2, 'top_bottom_layers': 5}


@dataclass
class Coupon:
    kind: str
    name: str
    body: Body
    build_direction: tuple
    settings: dict
    geometry: dict
    how: str
    notes: list = field(default_factory=list)


def _plate_xy(k, outline, thickness: float) -> Body:
    sk = Sketch(Plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0)))
    sk.polyline(outline, closed=True)
    return k.extrude(sk.to_body(), (0.0, 0.0, 1.0), thickness)


def _holes(k, body: Body, xs, y: float, d: float, height: float) -> Body:
    for x in xs:
        body = k.boolean(body, k.cylinder((x, y, -1.0), (0.0, 0.0, 1.0), d / 2, height + 2.0), BooleanOp.SUBTRACT)
    return body


def dogbone(k, gauge_len=60.0, gauge_w=10.0, thick=4.0, grip_w=20.0, grip_len=30.0, taper=12.0, hole=6.5) -> tuple[Body, float]:
    """A flat tensile bar along x; returns (body, gauge area mm²)."""
    L = 2 * grip_len + 2 * taper + gauge_len
    g0, g1 = grip_len + taper, grip_len + taper + gauge_len
    hw, gw = grip_w / 2, gauge_w / 2
    outline = [(0, -hw), (grip_len, -hw), (g0, -gw), (g1, -gw), (L - grip_len, -hw), (L, -hw),
               (L, hw), (L - grip_len, hw), (g1, gw), (g0, gw), (grip_len, hw), (0, hw)]
    b = _plate_xy(k, outline, thick)
    b = _holes(k, b, (grip_len / 2, L - grip_len / 2), 0.0, hole, thick)
    return b, gauge_w * thick


def make_coupons(k, material: str = 'pla-basic', split: Optional[dict] = None, part_settings: Optional[dict] = None, copies: Optional[int] = None) -> list[Coupon]:
    registry, _ = reg.load()
    copies = copies or int(registry['test_plan'].get('min_samples', 3))
    joint_settings = part_settings or {'walls': 3, 'infill': 0.15, 'pattern': 'gyroid', 'layer_height': 0.2, 'top_bottom_layers': 5}
    out: list[Coupon] = []
    # Material: in-layer (flat) and across-layer (standing) tensile bars.
    bar, area = dogbone(k)
    for i in range(copies):
        out.append(Coupon('tensile_in_layer', f'tensile flat {i + 1}', bar, (0, 0, 1), SOLID, {'gauge_area_mm2': area},
                          'Hang it from one hole and load the other until it breaks; the break should be in the narrow middle.'))
    tall, area_t = dogbone(k, gauge_len=50.0, gauge_w=10.0, thick=6.0, grip_w=22.0, grip_len=25.0, taper=10.0)
    for i in range(copies):
        out.append(Coupon('tensile_across_layers', f'tensile standing {i + 1}', tall, (1, 0, 0), SOLID, {'gauge_area_mm2': area_t},
                          'Printed standing on one end (use a brim). Load it like the flat bar; it breaks between two layers.',
                          ['stands 120 mm tall on a 22 × 6 mm end: add a brim in the slicer']))
    # Joints: from the split's joints where given, else registry defaults.
    joints = [j for s in (split or {}).get('seams', []) for j in s['joints']]
    dowel = next((j for j in joints if j['kind'] == 'dowel'), None)
    d = float(dowel['spec']['diameter_mm']) if dowel else 4.0
    engaged = float(min(dowel['spec']['depth_minus_mm'], dowel['spec']['depth_plus_mm'])) if dowel else 6.0
    pin = reg.joint('dowel_pin')
    slip, press = reg.value(pin['slip_clearance_mm']), reg.value(pin['press_clearance_mm'])
    for i in range(copies):
        # Two lapped straps: the pin crosses the lap; pulling the straps apart loads it in bearing.
        a = k.box((0, -10, 0), (50, 20, engaged))
        a = k.boolean(a, k.cylinder((40, 0, -1), (0, 0, 1), (d + press) / 2, engaged + 2), BooleanOp.SUBTRACT)
        a = _holes(k, a, (8.0,), 0.0, 6.5, engaged)
        b = k.box((30, -10, engaged + 0.4), (50, 20, engaged))
        b = k.boolean(b, k.cylinder((40, 0, engaged - 1), (0, 0, 1), (d + slip) / 2, engaged + 2), BooleanOp.SUBTRACT)
        b = _holes(k, k.transform(b, translation=(0, 0, -(engaged + 0.4))), (72.0,), 0.0, 6.5, engaged)
        b = k.transform(b, translation=(0, 30, 0))
        out.append(Coupon('pin_shear', f'pin lap {i + 1} (two straps)', k.join([a, b]), (0, 0, 1), joint_settings,
                          {'pin_diameter_mm': d, 'engaged_mm': engaged},
                          f'Press a Ø{d:g} pin into the tight hole of the short strap, lay the long strap over it, and pull the straps apart along their length.'))
    size = 'M3'
    ins = next((j for j in joints if j['kind'] == 'insert_screw'), None)
    if ins:
        size = ins['spec']['size']
    geo = reg.insert(size)
    r_boss = geo['knurl_mm']
    for i in range(copies):
        base = k.box((-20, -20, 0), (40, 40, 5))
        base = _holes(k, base, (-13.0, 13.0), 0.0, 3.4, 5)
        boss = k.cylinder((0, 0, 5), (0, 0, 1), r_boss, 15)
        b = k.boolean(base, boss, BooleanOp.UNION)
        b = k.boolean(b, k.cylinder((0, 0, 20.1), (0, 0, -1), geo['hole_mm'] / 2, geo['depth_mm'] + 0.1), BooleanOp.SUBTRACT)
        out.append(Coupon('insert_pullout', f'insert boss {size} {i + 1}', b, (0, 0, 1), joint_settings,
                          {'knurl_mm': geo['knurl_mm'], 'insert_length_mm': geo['length_mm'], 'size': size},
                          f'Heat-set a {size} insert, screw the base down through its two holes, and pull a screw in the insert straight up (along the layers, as in the insert lesson).'))
    tab = next((j for j in joints if j['kind'] == 'dovetail'), None)
    neck = float(tab['spec']['neck_mm']) if tab else 10.0
    depth = float(tab['spec']['depth_mm']) if tab else 9.0
    thick = float(min(tab['spec']['rail_length_mm'], 8.0)) if tab else 5.0
    angle = math.radians(reg.joint('dovetail.angle_deg'))
    clearance = reg.joint('dovetail.sliding_clearance_mm')
    for i in range(copies):
        w0, w1 = neck / 2, neck / 2 + depth * math.tan(angle)
        # Tab strap along +x ending in the tail; slot strap receives it.
        tab_outline = [(0, -12), (40, -12), (40, -w0), (40 + depth, -w1), (40 + depth, w1), (40, w0), (40, 12), (0, 12)]
        t = _holes(k, _plate_xy(k, tab_outline, thick), (8.0,), 0.0, 6.5, thick)
        c = clearance
        slot_outline = [(40 - 0.01, -12), (90, -12), (90, 12), (40 - 0.01, 12), (40 - 0.01, w0 + c), (40 + depth + c, w1 + c), (40 + depth + c, -w1 - c), (40 - 0.01, -w0 - c)]
        sl = _holes(k, _plate_xy(k, slot_outline, thick), (82.0,), 0.0, 6.5, thick)
        sl = k.transform(sl, translation=(0, 30, 0))
        out.append(Coupon('dovetail_pull', f'dovetail tab {i + 1} (two straps)', k.join([t, sl]), (0, 0, 1), joint_settings,
                          {'neck_mm': neck, 'depth_mm': depth, 'thickness_mm': thick, 'angle_deg': math.degrees(angle)},
                          'Drop the tab into the slot and pull the straps apart along their length until the tab or slot breaks.'))
    return out


def results_template(coupons: list[Coupon], material: str, printer: str, settings_note: dict) -> dict:
    _, sha = reg.load()
    tests = {}
    for c in coupons:
        t = tests.setdefault(c.kind, {'coupon': c.kind, 'failure_n': [], 'geometry': {k_: v for k_, v in c.geometry.items() if k_ != 'size'}, 'how_it_broke': [], 'notes': ''})
        t['failure_n'].append(None)
        t['how_it_broke'].append('')
    return {'schema': 'sim.print-test/1', 'material': material, 'printer': printer, 'registry_sha256': sha, 'settings': settings_note,
            'printed': '', 'tested': '', 'operator': '', 'tests': list(tests.values())}


PROTOCOL = """# Coupon tests

Print the plates in this folder, then break each coupon and write its
breaking load (N) into `results.json` (one number per coupon, in order).

## What you need
- A luggage scale (to 50 kg / 500 N), or a bucket you fill with water and then weigh.
- An M6 bolt or S-hooks through the loading holes, and something solid to hang from.
- The hardware the joint coupons use (a dowel pin, a heat-set insert and screw).

## How
1. Hang the coupon from one loading hole. Hook the scale (or bucket) to the other.
2. Load slowly, over 10–30 s, until it breaks. Read the peak (the scale's max-hold, or weigh the bucket: 1 kg ≈ 9.81 N).
3. Write the load in `failure_n` and a word in `how_it_broke` (for example "middle", "at a grip", "insert pulled out", "tab snapped").
   Leave out breaks at a grip or hole: they measure the grip, not the material.
4. Break at least three of each kind. More gives a tighter design value (mean − 2·std).

## Recording
    target/release/sim-print promote results.json            # dry run: what would change
    target/release/sim-print promote results.json --write    # a new registry revision

The registry then says `measured`, with your breaks, their spread and the value it
replaced; every later strength check and plan uses the measured value.

## Safety
Breaking plastic can fling pieces: wear glasses, keep hands clear of the load path,
and keep the load low over the floor.
"""


def write_coupon_kit(k, out_dir: str, material: str = 'pla-basic', printer: str = 'bambu-h2c', split: Optional[dict] = None,
                     part_settings: Optional[dict] = None, copies: Optional[int] = None) -> dict:
    from .print_plan import write_plates
    coupons = make_coupons(k, material, split, part_settings, copies)
    os.makedirs(out_dir, exist_ok=True)
    pieces = [{'name': c.name, 'body': c.body, 'build_direction': c.build_direction, 'settings': c.settings,
               'estimate': {}, 'safety_factor': None, 'source': c.kind} for c in coupons]
    plates = write_plates(k, pieces, out_dir, printer, material)
    template = results_template(coupons, material, printer, {'material_coupons': SOLID, 'joint_coupons': part_settings or 'registry defaults'})
    with open(os.path.join(out_dir, 'results.json'), 'w') as f:
        json.dump(template, f, indent=1)
    with open(os.path.join(out_dir, 'PROTOCOL.md'), 'w') as f:
        f.write(PROTOCOL)
        f.write('\n## Coupons\n' + '\n'.join(f'- **{c.name}** ({c.kind}): {c.how}' + (f' _{"; ".join(c.notes)}_' if c.notes else '') for c in coupons) + '\n')
    return {'coupons': [{'kind': c.kind, 'name': c.name, 'geometry': c.geometry, 'build_direction': list(c.build_direction), 'settings': c.settings} for c in coupons],
            'plates': [p['file'] for p in plates['plates']], 'results_template': os.path.join(out_dir, 'results.json'),
            'protocol': os.path.join(out_dir, 'PROTOCOL.md')}
