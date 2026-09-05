"""Acceptance scenario, end to end and scripted through the same `Ops`
the buttons use:

  a two-part robot torso — an outer shell hollowed to 2 mm walls with four
  M3 heat-set insert bosses, a snap-fit lid with 0.2 mm clearance produced
  via dependent offset, a rectangular array of ventilation slots created
  with the slot tool and cut, filleted external edges (1 mm) and internal
  boss roots (0.5 mm), a mirrored motor mount created as a live instance,
  verified with section analysis and the wall-thickness check, and
  exported as a validated multi-body 3MF plus a hidden-line SVG drawing.

Run:  cd cad && .venv/bin/python scripts/acceptance.py [out_dir]
Writes torso.rcad, torso.3mf, torso.svg, stage-*.png and report.json.
"""

from __future__ import annotations

import json
import math
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from robocad.analysis import section_outline  # noqa: E402
from robocad.commands import Ops  # noqa: E402
from robocad.document import Document  # noqa: E402
from robocad.io.drawing import STANDARD_VIEWS, View, export_drawing_svg  # noqa: E402
from robocad.io.exporters import ThreeMfSettings, export_3mf  # noqa: E402
from robocad.io.snapshot import render  # noqa: E402
from robocad.kernel import BooleanOp, Plane, Sketch, SurfaceKind  # noqa: E402
from robocad.kernel.base import v_dist  # noqa: E402
from robocad.printing import FastenerSpec, insert_boss, mesh_open_edges, validate_for_export, wall_thickness  # noqa: E402


def main(out_dir: str = "acceptance_out"):
    os.makedirs(out_dir, exist_ok=True)
    started = time.time()
    doc = Document()
    ops = Ops(doc)
    k = doc.kernel
    stages = []

    def stage(name, **extra):
        p = os.path.join(out_dir, f"stage-{len(stages) + 1:02d}-{name}.png")
        render(doc, p, title=name, **extra)
        stages.append({"name": name, "image": p})
        print(f"  stage {len(stages)}: {name}")

    # 1. The torso: an 80 × 60 × 50 box with 1 mm external fillets, hollowed to 2 mm.
    W, D, H, WALL = 80.0, 60.0, 50.0, 2.0
    shell = ops.box((0, 0, 0), (W, D, H), name="Torso shell")
    ops.set_material([shell], "petg")
    stage("box")
    bottom = next(f for f in k.faces(doc.nodes[shell].body) if f.kind == SurfaceKind.PLANE and f.normal[2] < -0.9)
    ops.shell(shell, WALL, [bottom])
    stage("hollowed", view=(-1.0, -1.4, -0.9))
    # External fillets after hollowing (a 2 mm wall cannot be offset inside a 1 mm fillet).
    def outer(e):
        m = e.midpoint
        on_x = abs(m[0]) < 1e-6 or abs(m[0] - W) < 1e-6
        on_y = abs(m[1]) < 1e-6 or abs(m[1] - D) < 1e-6
        on_top = abs(m[2] - H) < 1e-6
        return (on_x and on_y) or (on_top and (on_x or on_y))
    ops.fillet(shell, [e for e in k.edges(doc.nodes[shell].body) if outer(e) and e.kind.value == "line"], 1.0)
    stage("external-fillets")

    # 2. Four M3 heat-set insert bosses hanging from the ceiling down to the lid plane, roots filleted 0.5 mm.
    inset = 7.0
    plug_h = 6.0
    boss_h = H - WALL - plug_h
    boss_r = 2.0 + 1.8
    for x, y in ((inset, inset), (W - inset, inset), (inset, D - inset), (W - inset, D - inset)):
        boss = insert_boss(k, (x, y, H - WALL), (0, 0, -1), FastenerSpec("M3", "insert"), boss_h, wall=1.8)
        bid = doc.add_body(boss, "boss")
        ops.boolean(shell, [bid.id], BooleanOp.UNION)
    roots = [e for e in k.edges(doc.nodes[shell].body) if e.kind.value == "circle" and e.radius and abs(e.radius - boss_r) < 1e-3 and abs(e.midpoint[2] - (H - WALL)) < 1e-6]
    ops.fillet(shell, roots, 0.5)
    stage("insert-bosses", view=(-1.0, -1.4, -0.9))

    # 3. Ventilation: a rectangular array of slots cut through the side wall.
    vent = ops.new_sketch(Plane.xz(-1.0), "Vent slots")
    ops.edit_sketch(vent, lambda sk: sk.slot((14, 18), (14, 34), 3.0))
    cutter = ops.extrude(vent, WALL + 2.0, direction=(0, 1, 0), name="slot cutter")
    copies = ops.array_rect([cutter], (6, 1, 1), spacing=(10.0, 0, 0))
    ops.boolean(shell, [cutter] + copies, BooleanOp.SUBTRACT)
    doc.nodes[vent].visible = False
    stage("vent-slots", view=(0.2, -1.6, 0.6))

    # 4. The lid: a plate closing the open bottom, with a plug that mates the cavity
    #    through a dependent offset of each side face to the shell's inner wall at 0.2 mm.
    lid = ops.box((0, 0, -4.0), (W, D, 4.0), name="Lid")
    ops.set_material([lid], "petg")
    plug = ops.box((10.0, 10.0, 0.0), (W - 20.0, D - 20.0, plug_h), name="plug")
    for axis, sign in ((0, 1), (0, -1), (1, 1), (1, -1)):
        face = next(f for f in k.faces(doc.nodes[plug].body) if f.kind == SurfaceKind.PLANE and f.normal[axis] * sign > 0.9)
        ops.offset_face_to(plug, face, shell, clearance=0.2)
    plug_size = k.mass_properties(doc.nodes[plug].body).size
    ops.boolean(lid, [plug], BooleanOp.UNION)
    # Pockets for the bosses (0.2 mm around them) and M3 clearance holes through the plate.
    lid_bottom = next(f for f in k.faces(doc.nodes[lid].body) if f.normal[2] < -0.9 and f.area > 1000)
    for x, y in ((inset, inset), (W - inset, inset), (inset, D - inset), (W - inset, D - inset)):
        pocket = ops.cylinder((x, y, -0.001), (0, 0, 1), boss_r + 0.2, plug_h + 1.0, name="pocket")
        ops.boolean(lid, [pocket], BooleanOp.SUBTRACT)
        ops.fastener_hole(lid, lid_bottom, (x, y, -4.0), FastenerSpec("M3", "clearance", extra_clearance=0.2))
    stage("lid", view=(-1.0, -1.4, -0.7))

    # 5. Motor mount and its mirrored live instance.
    mount = ops.box((W, 10.0, 8.0), (14.0, 40.0, 30.0), name="Motor mount")
    ops.set_material([mount], "petg")
    mount_face = next(f for f in k.faces(doc.nodes[mount].body) if f.normal[0] > 0.9)
    ops.fastener_hole(mount, mount_face, (W + 14.0, 30.0, 23.0), FastenerSpec("M3", "clearance"), depth=14.0)
    ops.cylinder((W + 14.0 + 0.001, 30.0, 23.0), (-1, 0, 0), 6.0, 8.0, name="shaft bore")
    bore = doc.find("shaft bore").id
    ops.boolean(mount, [bore], BooleanOp.SUBTRACT)
    mirrored = ops.mirror([mount], Plane.yz(W / 2), live=True)[0]
    doc.nodes[mirrored].name = "Motor mount (mirror instance)"
    stage("motor-mounts", view=(-1.0, -1.6, 0.9))

    # 6. Verification: section analysis and the wall-thickness check.
    section = Plane.xz(D / 2)
    loops = section_outline(doc, section)
    thin = wall_thickness(k, doc.nodes[shell].body, 1.2)
    stage("section-analysis", section=section, view=(0.0, -1.0, 0.35))
    stage("wall-check", highlight=[shell] if thin else [])
    ok, messages = validate_for_export(k, [(n.name, n.body) for n in doc.bodies()])

    # 7. Exports: a validated multi-body 3MF and a hidden-line drawing.
    doc.save(os.path.join(out_dir, "torso.rcad"))
    warnings = export_3mf(doc, os.path.join(out_dir, "torso.3mf"), settings=ThreeMfSettings(0.05))
    views = [STANDARD_VIEWS["front"], STANDARD_VIEWS["top"], STANDARD_VIEWS["right"], View("Section A-A", (0.0, 1.0, 0.0), section=section)]
    export_drawing_svg(doc, os.path.join(out_dir, "torso.svg"), views, title="Robot torso — two-part shell")
    stage("exported", view=(-1.0, -1.4, 0.9))

    report = {
        "seconds": round(time.time() - started, 1),
        "bodies": [{"name": n.name, "volume_cm3": round(k.mass_properties(n.body).volume / 1000, 3), "mass_g": round(k.mass_properties(n.body).mass(doc.density_of(n.id)), 2)} for n in doc.bodies()],
        "instances": [n.name for n in doc.nodes.values() if n.kind == "instance"],
        "plug_clearance_mm": {"x": round((W - 2 * WALL - plug_size[0]) / 2, 3), "y": round((D - 2 * WALL - plug_size[1]) / 2, 3)},
        "section_loops": len(loops),
        "thin_regions_under_1.2mm": len(thin),
        "validation": {"ok": ok, "messages": messages, "export_warnings": warnings},
        "mesh_open_edges": {n.name: mesh_open_edges(doc.mesh_of(n.id)) for n in doc.bodies()},
        "stages": stages,
        "undo_depth": len(ops.stack.undo_stack),
    }
    with open(os.path.join(out_dir, "report.json"), "w") as f:
        json.dump(report, f, indent=1)
    print(json.dumps({k2: v for k2, v in report.items() if k2 != "stages"}, indent=1))
    assert ok, "validation failed"
    assert all(v == 0 for v in report["mesh_open_edges"].values()), "non-manifold export"
    assert report["section_loops"] > 0
    assert len(report["instances"]) == 1
    return report


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "acceptance_out")
