"""Author a three-wheel CAD fixture for the shared robot-learning APIs.

Two N20 drives and one passive wheel; dimensions in mm at authoring, SI export.
The library motor estimates and authored joint assumptions are uncalibrated.
Run from the repository root with the CAD virtualenv Python and an output path.
"""
import hashlib
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from robocad.commands import Ops
from robocad.document import Document
from robocad.physical import export_physical_model


def build(output):
    os.makedirs(output, exist_ok=True)
    if os.path.exists(os.path.join(output, "robot.rcad")):
        raise FileExistsError("refusing to replace an existing CAD fixture")
    doc = Document()
    ops = Ops(doc)
    chassis = ops.box((-70., -40., 35.), (120., 80., 10.), name="chassis")
    ops.set_material([chassis], "petg")
    for name, y in [("left", 60.), ("right", -60.)]:
        wheel = ops.cylinder((25., y-6., 30.), (0., 1., 0.), 30., 12., name=name+" wheel")
        ops.set_material([wheel], "petg")
        sign = 1. if y > 0. else -1.
        motor = ops.add_motor("n20_100", (25., sign*45., 30.), (0., sign, 0.),
                              mount_on=chassis, name=name+" drive")
        joint = ops.add_joint("continuous", chassis, wheel, (25., y, 30.),
                              (0., 1., 0.), name=name+" axle")
        ops.attach_motor(joint, motor)
        ops.set_joint_physics(joint,
            drive_backlash={"width_rad": 0., "provenance": "estimated",
                            "reference": "fixture assumes a rigid shaft-wheel coupling; motor gearbox backlash remains separate",
                            "uncertainty_rad": None},
            friction={"coulomb": 0.0001, "viscous": 0.00001, "stribeck": 0.,
                      "stribeck_speed": 0.1, "static_ratio": 1.})
        ops.add_sensor("encoder", wheel, (25., y, 30.), joint=joint, name=name+" encoder")
    rear = ops.cylinder((-55., -5., 20.), (0., 1., 0.), 20., 10., name="passive wheel")
    ops.set_material([rear], "petg")
    rear_joint = ops.add_joint("continuous", chassis, rear, (-55., 0., 20.),
                               (0., 1., 0.), name="passive axle")
    ops.set_joint_physics(rear_joint,
        friction={"coulomb": 0.0001, "viscous": 0.00001, "stribeck": 0.,
                  "stribeck_speed": 0.1, "static_ratio": 1.})
    ops.add_sensor("imu", chassis, (0., 0., 40.), name="body imu")
    ops.set_battery(cells=5, chemistry="nimh", capacity_ah=0.5)
    ops.set_control(period_s=0.02, latency_s=0.001,
                    targets={"left axle": 0., "right axle": 0.})
    ops.set_robot_setting("world", {"floor_z": 0., "floor_material": "world",
        "floor_stiffness": 2e5, "floor_damping": 2e3, "terrain": None})
    assumptions = {
        "status": "uncalibrated synthetic CAD benchmark",
        "geometry": "three solid PETG wheels, rectangular PETG chassis, library N20 motor bodies",
        "mass": "CAD volume and material density, plus library motor mass; no separate battery body modeled",
        "joint_friction": "estimated 0.0001 N m Coulomb and 0.00001 N m s/rad viscous per axle",
        "actuators": "library N20 equivalent electrical and gearbox estimates; position firmware is a simulated external controller",
        "topology": "two powered continuous axles and a passive continuous rear axle; no steering caster",
        "purpose": "exercise shared contracts and locomotion APIs on different morphology; not a manufactured robot specification",
    }
    ops.set_robot_setting("benchmark_assumptions", assumptions)
    cad_path = os.path.join(output, "robot.rcad")
    doc.save(cad_path)
    model = export_physical_model(doc, flex=False, verbose=True)
    model["source"]["cad_sha256"] = hashlib.sha256(open(cad_path, "rb").read()).hexdigest()
    model["source"]["benchmark_assumptions"] = assumptions
    with open(os.path.join(output, "robot.simrobot.json"), "x") as stream:
        json.dump(model, stream)
    print(json.dumps({"links": len(model["links"]), "joints": len(model["joints"]),
                      "motors": len(model["motors"]), "sensors": len(model["sensors"]),
                      "cad_sha256": model["source"]["cad_sha256"]}), flush=True)


if __name__ == "__main__":
    build(sys.argv[1])
