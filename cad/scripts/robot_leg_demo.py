"""A two-link robot leg built through Ops with library motors, declared
joints (limits, motors) and a ground body, exported for the simulator.

    cd cad && .venv/bin/python scripts/robot_leg_demo.py out_dir
"""
import json
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from robocad.commands import Ops  # noqa: E402
from robocad.document import Document  # noqa: E402
from robocad.kernel import Plane  # noqa: E402
from robocad.printing import FastenerSpec  # noqa: E402
from robocad.simbridge import export_sim_model  # noqa: E402


def main(out_dir="demo_out"):
    os.makedirs(out_dir, exist_ok=True)
    doc = Document()
    ops = Ops(doc)
    # The working plane for simulation is XZ (x forward, z up). A hip block fixed to the world:
    hip = ops.box((-25.0, -20.0, 200.0), (50.0, 40.0, 40.0), name="ground")
    ops.set_material([hip], "al")
    # Thigh: 120 mm long bar hanging from a pivot at the hip's bottom centre.
    thigh = ops.box((-8.0, -6.0, 90.0), (16.0, 12.0, 120.0), name="thigh")
    ops.set_material([thigh], "petg")
    # Shank: 110 mm, from the knee at z=90 down to the foot.
    shank = ops.box((-6.0, -5.0, -15.0), (12.0, 10.0, 110.0), name="shank")
    ops.set_material([shank], "petg")
    ops.set_ground(hip)
    # Library motors: the shaft face sits on the mounting body's -Y face, the
    # housing outside it, the shaft pointing +Y into the body (the joint axis).
    hip_motor = ops.add_motor("mg996r", (0.0, -20.0, 200.0), (0.0, 1.0, 0.0), mount_on=hip, cut_mount=True, name="hip motor")
    knee_motor = ops.add_motor("mg90s", (0.0, -6.0, 90.0), (0.0, 1.0, 0.0), mount_on=thigh, cut_mount=True, name="knee motor")
    # A foot pad rounded with a fillet, and an M3 clearance hole through the shank end.
    top = next(f for f in doc.kernel.faces(doc.nodes[shank].body) if f.normal[2] > 0.9)
    ops.fastener_hole(shank, top, (0.0, 0.0, 95.0), FastenerSpec("M3", "clearance"))
    # Joints with limits, each driven by its motor.
    hip_j = ops.add_joint("revolute", hip, thigh, (0.0, 0.0, 200.0), (0.0, 1.0, 0.0), lower=math.radians(-90), upper=math.radians(90), name="hip")
    knee_j = ops.add_joint("revolute", thigh, shank, (0.0, 0.0, 90.0), (0.0, 1.0, 0.0), lower=math.radians(-120), upper=math.radians(5), name="knee")
    ops.attach_motor(hip_j, hip_motor)
    ops.attach_motor(knee_j, knee_motor)
    for issue in ops.robot()["issues"]:
        print(f"  [{issue['severity']}] {issue['message']}")
    # Sensing, power and control: an IMU near the foot, an encoder per joint, a 2S LiPo, hold targets.
    ops.add_sensor("imu", shank, (0.0, 0.0, 0.0), name="foot imu")
    ops.add_sensor("encoder", thigh, (0.0, 0.0, 200.0), joint=hip_j, name="hip encoder")
    ops.add_sensor("encoder", shank, (0.0, 0.0, 90.0), joint=knee_j, name="knee encoder")
    ops.add_cable(hip, (0.0, -20.0, 190.0), shank, (0.0, -5.0, 60.0), name="servo lead")
    ops.set_battery(cells=2, chemistry="lipo", capacity_ah=1.0)
    ops.set_control(period_s=0.02, latency_s=0.004, targets={"hip": math.radians(20.0), "knee": math.radians(-30.0)})
    path = os.path.join(out_dir, "leg.rcad")
    doc.save(path)
    flex = "--no-flex" not in sys.argv
    model = export_sim_model(doc, os.path.join(out_dir, "leg.simrobot.json"), plane=Plane.xz(), flex=flex)
    exchange = os.environ.get("SIMROBOT_EXCHANGE")
    if exchange:
        os.makedirs(exchange, exist_ok=True)
        with open(os.path.join(exchange, "leg.simrobot.json"), "w") as f:
            json.dump(model, f)
    print(f"saved {path} and leg.simrobot.json (v{model['version']}): {len(model['links'])} links, {len(model['joints'])} joints, {len(model['motors'])} motors, {len(model['sensors'])} sensors, {len(model['cables'])} cables")
    for l in model["links"]:
        I = l["inertia"]
        fx = l["flex"]
        print(f"  {l['name']:<8} mass {l['mass']*1000:7.1f} g  com {tuple(round(c*1000,1) for c in l['com'])} mm  Ixx/Iyy/Izz {I[0][0]:.2e}/{I[1][1]:.2e}/{I[2][2]:.2e}  collision {len(l['collision']['vertices'])} v  flex {fx['modes'] if fx else 0} modes{' f1=' + format(fx['frequencies_hz'][0], '.0f') + ' Hz' if fx else ''}  ground={l['ground']}")
    for j in model["joints"]:
        p = j["physics"]
        print(f"  {j['name']}: {j['type']} {j['parent']} → {j['child']} origin {tuple(round(c*1000,1) for c in j['origin'])} mm limits {j.get('limits')} clearance {p.get('clearance',0)*1e3:.2f} mm backlash {math.degrees(p.get('backlash',0)):.2f}° coulomb {p['friction']['coulomb']*1e3:.2f} mN·m radial k {p['stiffness']['radial']:.2e} N/m motor {j.get('motor')} [{p['source']}]")
    for m in model["motors"]:
        e, g = m["electrical"], m["gearbox"]
        print(f"  motor {m['name']}: R {e['resistance']:.2f} Ω kt {e['torque_constant']*1e3:.2f} mN·m/A ratio {g['ratio']:.0f} backlash {math.degrees(g['backlash_rad']):.1f}° firmware {m['firmware']['kind']}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "demo_out")
