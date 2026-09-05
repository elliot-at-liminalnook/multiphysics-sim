"""A parallel gripper: two fingers on a printed base, each finger carried
by a four-bar (finger + coupler link) so the jaws stay parallel, one MG90S
driving the left crank and a coupler bar closing the loop to the right
crank. Exported as a v3 physical model with two `loop_revolute` joints.

    cd cad && .venv/bin/python scripts/gripper_demo.py out_dir
    SIMROBOT_EXCHANGE=dir also copies gripper.simrobot.json there.
"""
import json
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from robocad.commands import Ops  # noqa: E402
from robocad.document import Document  # noqa: E402
from robocad.kernel import BooleanOp, Plane  # noqa: E402
from robocad.simbridge import export_sim_model  # noqa: E402


def main(out_dir="demo_out"):
    os.makedirs(out_dir, exist_ok=True)
    doc = Document()
    ops = Ops(doc)
    # Base plate (ground), y is the jaw opening direction, z up; pivots along x.
    base = ops.box((-30.0, -30.0, 0.0), (60.0, 60.0, 8.0), name="ground")
    ops.set_material([base], "pla")
    ops.set_ground(base)
    # Two cranks per side (a four-bar): crank A at y=±12, crank B at y=±24, both 40 mm long, hinged on the base at z=8.
    links = {}
    for side, sgn in (("left", -1.0), ("right", 1.0)):
        for tag, y in (("a", 12.0), ("b", 24.0)):
            bar = ops.box((-4.0, sgn * y - 3.0, 6.0), (8.0, 6.0, 42.0), name=f"{side} crank {tag}")
            ops.set_material([bar], "petg")
            # Pin bosses at both ends (5 mm pins in 5.2 mm holes on the mating parts).
            for z in (8.0, 48.0):
                ops.boolean(bar, [ops.cylinder((-6.0, sgn * y, z), (1.0, 0.0, 0.0), 2.5, 12.0)], BooleanOp.UNION)
            links[(side, tag)] = bar
        jaw = ops.box((-6.0, min(sgn * 9.0, sgn * 27.0), 46.0), (12.0, 18.0, 30.0), name=f"{side} jaw")
        ops.set_material([jaw], "petg")
        for y in (12.0, 24.0):
            ops.boolean(jaw, [ops.cylinder((-7.0, sgn * y, 48.0), (1.0, 0.0, 0.0), 2.6, 14.0)], BooleanOp.SUBTRACT)
        links[(side, "jaw")] = jaw
    # Base holes for the crank pins.
    for sgn in (-1.0, 1.0):
        for y in (12.0, 24.0):
            ops.boolean(base, [ops.cylinder((-7.0, sgn * y, 8.0), (1.0, 0.0, 0.0), 2.6, 14.0)], BooleanOp.SUBTRACT)
    # Tree joints: base → crank a → jaw ; loop joints: crank b closes each four-bar.
    x_axis = (1.0, 0.0, 0.0)
    joints = {}
    for side, sgn in (("left", -1.0), ("right", 1.0)):
        ja = ops.add_joint("revolute", base, links[(side, "a")], (0.0, sgn * 12.0, 8.0), x_axis, lower=math.radians(-40), upper=math.radians(40), name=f"{side} crank")
        jj = ops.add_joint("revolute", links[(side, "a")], links[(side, "jaw")], (0.0, sgn * 12.0, 48.0), x_axis, name=f"{side} jaw pivot")
        jb = ops.add_joint("revolute", base, links[(side, "b")], (0.0, sgn * 24.0, 8.0), x_axis, name=f"{side} crank b")
        loop = ops.add_joint("loop_revolute", links[(side, "b")], links[(side, "jaw")], (0.0, sgn * 24.0, 48.0), x_axis, name=f"{side} four-bar close")
        joints[side] = (ja, jj, jb, loop)
    # The MG90S drives the left crank from the base; a coupler bar ties the two cranks (loop joint on the right).
    motor = ops.add_motor("mg90s", (-8.0, -12.0, 8.0), (1.0, 0.0, 0.0), mount_on=base, cut_mount=False, name="gripper servo")
    ops.attach_motor(joints["left"][0], motor)
    coupler = ops.box((6.0, -12.0, 18.0), (4.0, 24.0, 6.0), name="coupler")
    ops.set_material([coupler], "petg")
    ops.add_joint("revolute", links[("left", "a")], coupler, (8.0, -12.0, 21.0), x_axis, name="coupler left")
    ops.add_joint("loop_revolute", links[("right", "a")], coupler, (8.0, 12.0, 21.0), x_axis, name="coupler right")
    ops.add_sensor("current", base, (-8.0, -12.0, 8.0), joint=joints["left"][0], name="servo current")
    ops.add_sensor("force", links[("left", "jaw")], (0.0, -9.0, 70.0), name="jaw force")
    ops.set_battery(cells=1, chemistry="lipo", capacity_ah=0.5)
    ops.set_control(period_s=0.02, latency_s=0.01, targets={"left crank": math.radians(-25.0)})
    for issue in ops.robot()["issues"]:
        print(f"  [{issue['severity']}] {issue['message']}")
    path = os.path.join(out_dir, "gripper.rcad")
    doc.save(path)
    flex = "--no-flex" not in sys.argv
    model = export_sim_model(doc, os.path.join(out_dir, "gripper.simrobot.json"), plane=Plane.yz() if hasattr(Plane, "yz") else Plane.xz(), flex=flex)
    exchange = os.environ.get("SIMROBOT_EXCHANGE")
    if exchange:
        os.makedirs(exchange, exist_ok=True)
        with open(os.path.join(exchange, "gripper.simrobot.json"), "w") as f:
            json.dump(model, f)
    loops = [j["name"] for j in model["joints"] if j["type"].startswith("loop_")]
    print(f"saved {path}: {len(model['links'])} links, {len(model['joints'])} joints ({len(loops)} loop closures: {', '.join(loops)}), {len(model['motors'])} motors, {sum(1 for l in model['links'] if l['flex'])} flexible links")
    for j in model["joints"]:
        p = j["physics"]
        print(f"  {j['name']:<22} {j['type']:<14} {str(j['parent']):<14} → {j['child']:<14} clearance {p.get('clearance', 0) * 1e3:.2f} mm backlash {math.degrees(p.get('backlash', 0)):.2f}° [{p['source']}]")


if __name__ == "__main__":
    main(next((a for a in sys.argv[1:] if not a.startswith("--")), "demo_out"))
