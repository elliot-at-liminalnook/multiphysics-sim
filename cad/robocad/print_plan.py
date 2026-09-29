"""Build plates from a print plan (``sim-print plan``): each piece turned so its
chosen build direction points up, turned about z to its smallest footprint,
set on the bed and packed onto plates (grouped by layer height), then written
as one 3MF per plate with each object's settings.

The 3MF carries the geometry (core 3MF) plus Bambu Studio's per-object
settings file (``Metadata/model_settings.config``: wall loops, sparse infill
density and pattern, top/bottom shell layers, layer height). Those keys are
Bambu Studio's names; check the object settings after opening, and pick the
printer, nozzle and filament profiles there. A manifest lists every object's
settings, estimated time and filament, so nothing depends on the 3MF extras.
"""

from __future__ import annotations

import json
import math
import os
import zipfile
from dataclasses import dataclass, field
from typing import Optional, Sequence
from xml.sax.saxutils import escape

import numpy as np

from . import print_registry as reg
from .kernel.base import Body, v_cross, v_dot, v_norm, v_unit

SPACING = 6.0  # mm between parts on a plate


def orient(k, body: Body, build_direction) -> tuple[Body, float, float]:
    """The body turned so `build_direction` points up, turned about z to the
    smallest footprint and set on the bed at the origin. Returns (body, width, depth)."""
    d = v_unit(tuple(build_direction))
    up = (0.0, 0.0, 1.0)
    c = v_dot(d, up)
    b = body
    if c < 1 - 1e-9:
        axis = v_cross(d, up) if c > -1 + 1e-9 else (1.0, 0.0, 0.0)
        angle = math.degrees(math.acos(max(-1.0, min(1.0, c))))
        b = k.transform(b, rotation_axis=v_unit(axis), rotation_deg=angle, rotation_center=(0, 0, 0))
    pts = np.asarray(k.tessellate(b, 0.5).vertices)[:, :2]
    best = (math.inf, 0.0)
    for deg in range(0, 90, 3):
        t = math.radians(deg)
        rot = np.array([[math.cos(t), -math.sin(t)], [math.sin(t), math.cos(t)]])
        q = pts @ rot.T
        area = np.ptp(q[:, 0]) * np.ptp(q[:, 1])
        if area < best[0] - 1e-6:
            best = (area, deg)
    if best[1]:
        b = k.transform(b, rotation_axis=(0, 0, 1), rotation_deg=best[1], rotation_center=(0, 0, 0))
    lo, hi = k.bounding_box(b)
    b = k.transform(b, translation=(-lo[0], -lo[1], -lo[2]))
    return b, hi[0] - lo[0], hi[1] - lo[1]


@dataclass
class Placed:
    name: str
    body: Body
    x: float
    y: float
    w: float
    d: float
    settings: dict
    estimate: dict
    safety_factor: Optional[float]
    source: Optional[str] = None


@dataclass
class Plate:
    index: int
    layer_height: float
    items: list = field(default_factory=list)

    def hours(self) -> float:
        return sum(p.estimate.get('print_hours', 0.0) for p in self.items)

    def grams(self) -> float:
        return sum(p.estimate.get('filament_g', 0.0) + p.estimate.get('support_g', 0.0) for p in self.items)


def pack(k, pieces: Sequence[dict], printer: str) -> list[Plate]:
    """Shelf-pack oriented pieces onto plates. Each piece: {name, body, build_direction, settings, estimate, safety_factor, source}."""
    ux, uy, uz = reg.usable_mm(printer)
    oriented = []
    for p in pieces:
        b, w, d = orient(k, p['body'], p['build_direction'])
        lo, hi = k.bounding_box(b)
        if hi[2] - lo[2] > uz + 1e-6 or not ((w <= ux and d <= uy) or (w <= uy and d <= ux)):
            raise ValueError(f"{p['name']}: {w:.0f}×{d:.0f}×{hi[2] - lo[2]:.0f} mm does not fit the {printer} standing this way: split it first")
        if w > ux or d > uy:  # turn a quarter turn to fit
            b = k.transform(b, rotation_axis=(0, 0, 1), rotation_deg=90, rotation_center=(0, 0, 0))
            lo, hi = k.bounding_box(b)
            b = k.transform(b, translation=(-lo[0], -lo[1], -lo[2]))
            w, d = d, w
        oriented.append((p, b, w, d))
    plates: list[Plate] = []
    by_height: dict[float, list] = {}
    for item in oriented:
        by_height.setdefault(float(item[0]['settings'].get('layer_height', 0.2)), []).append(item)
    for lh, items in sorted(by_height.items()):
        items.sort(key=lambda it: -(it[2] * it[3]))
        open_plates: list[tuple[Plate, list]] = []   # (plate, shelves [(y, height, x_used)])
        for p, b, w, d in items:
            placed = False
            for plate, shelves in open_plates:
                for s in shelves:
                    y, height, used = s
                    if d <= height and used + w <= ux:
                        plate.items.append(Placed(p['name'], b, used, y, w, d, p['settings'], p['estimate'], p.get('safety_factor'), p.get('source')))
                        s[2] = used + w + SPACING
                        placed = True
                        break
                if placed:
                    break
                top = max((s[0] + s[1] + SPACING for s in shelves), default=0.0)
                if top + d <= uy and w <= ux:
                    plate.items.append(Placed(p['name'], b, 0.0, top, w, d, p['settings'], p['estimate'], p.get('safety_factor'), p.get('source')))
                    shelves.append([top, d, w + SPACING])
                    placed = True
                    break
            if not placed:
                plate = Plate(len(plates) + len(open_plates) + 1, lh)
                plate.items.append(Placed(p['name'], b, 0.0, 0.0, w, d, p['settings'], p['estimate'], p.get('safety_factor'), p.get('source')))
                open_plates.append((plate, [[0.0, d, w + SPACING]]))
        plates.extend(pl for pl, _ in open_plates)
    for i, pl in enumerate(plates):
        pl.index = i + 1
    return plates


def _bambu_settings(s: dict) -> dict:
    return {
        'wall_loops': str(int(s['walls'])),
        'sparse_infill_density': f"{float(s['infill']) * 100:g}%",
        'sparse_infill_pattern': s.get('pattern', 'gyroid'),
        'top_shell_layers': str(int(s.get('top_bottom_layers', 5))),
        'bottom_shell_layers': str(int(s.get('top_bottom_layers', 5))),
        'layer_height': f"{float(s['layer_height']):g}",
    }


def write_plate(k, plate: Plate, path: str, printer: str, material: str) -> dict:
    """One 3MF: geometry, object names, placements on the bed, Bambu per-object settings."""
    from .printing import weld
    ux, uy, _ = reg.usable_mm(printer)
    margin = reg.value(reg.printer(printer)['margin_mm'])
    objects, items, configs = [], [], []
    for n, pl in enumerate(plate.items):
        oid = n + 1
        m = weld(k.tessellate(pl.body, 0.05))
        verts = ''.join(f'<vertex x="{v[0]:.4f}" y="{v[1]:.4f}" z="{v[2]:.4f}"/>' for v in m.vertices)
        tris = ''.join(f'<triangle v1="{a}" v2="{b}" v3="{c}"/>' for a, b, c in m.triangles)
        objects.append(f'<object id="{oid}" type="model" name="{escape(pl.name)}"><mesh><vertices>{verts}</vertices><triangles>{tris}</triangles></mesh></object>')
        tx, ty = pl.x + margin, pl.y + margin
        items.append(f'<item objectid="{oid}" transform="1 0 0 0 1 0 0 0 1 {tx:.3f} {ty:.3f} 0" printable="1"/>')
        meta = ''.join(f'<metadata key="{k_}" value="{escape(v)}"/>' for k_, v in {'name': pl.name, 'extruder': '1', **_bambu_settings(pl.settings)}.items())
        configs.append(f'<object id="{oid}">{meta}<part id="1" subtype="normal_part"><metadata key="name" value="{escape(pl.name)}"/></part></object>')
    model = ('<?xml version="1.0" encoding="UTF-8"?>\n<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">'
             f'<metadata name="Application">robocad print plan</metadata><resources>{"".join(objects)}</resources><build>{"".join(items)}</build></model>')
    plate_cfg = '<plate><metadata key="plater_id" value="1"/><metadata key="plater_name" value="Plate {}"/>{}</plate>'.format(
        plate.index, ''.join(f'<model_instance><metadata key="object_id" value="{n + 1}"/><metadata key="instance_id" value="0"/></model_instance>' for n in range(len(plate.items))))
    settings = f'<?xml version="1.0" encoding="UTF-8"?>\n<config>{"".join(configs)}{plate_cfg}</config>'
    with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as z:
        z.writestr('[Content_Types].xml', '<?xml version="1.0" encoding="UTF-8"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/><Default Extension="config" ContentType="text/xml"/></Types>')
        z.writestr('_rels/.rels', '<?xml version="1.0" encoding="UTF-8"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>')
        z.writestr('3D/3dmodel.model', model)
        z.writestr('Metadata/model_settings.config', settings)
    return {'file': os.path.basename(path), 'plate': plate.index, 'layer_height_mm': plate.layer_height,
            'estimated_hours': round(plate.hours(), 2), 'estimated_filament_g': round(plate.grams(), 1),
            'objects': [{'name': p.name, 'source': p.source, 'at_mm': [round(p.x + margin, 1), round(p.y + margin, 1)], 'footprint_mm': [round(p.w, 1), round(p.d, 1)],
                         'settings': p.settings, 'bambu': _bambu_settings(p.settings), 'estimate': p.estimate, 'safety_factor': p.safety_factor} for p in plate.items]}


def write_plates(k, pieces: Sequence[dict], out_dir: str, printer: str, material: str, plan_path: Optional[str] = None) -> dict:
    os.makedirs(out_dir, exist_ok=True)
    plates = pack(k, pieces, printer)
    files = [write_plate(k, pl, os.path.join(out_dir, f'plate-{pl.index}.3mf'), printer, material) for pl in plates]
    _, sha = reg.load()
    manifest = {'schema': 'sim.print-plates/1', 'printer': printer, 'material': material, 'registry_sha256': sha, 'plan': plan_path,
                'plates': files, 'total_hours': round(sum(f['estimated_hours'] for f in files), 2),
                'total_filament_g': round(sum(f['estimated_filament_g'] for f in files), 1),
                'note': 'Open each plate in Bambu Studio, choose the printer, nozzle and filament, and check the per-object settings. Times and filament are estimates from the plan; the slicer is authoritative.'}
    with open(os.path.join(out_dir, 'plates.json'), 'w') as f:
        json.dump(manifest, f, indent=1)
    return manifest
