"""Author and export the benchmark through the public CAD scripting API."""
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "cad"))
from robocad.commands import Ops
from robocad.document import Document
from robocad.physical import export_physical_model


def build(output):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    doc = Document()
    ops = Ops(doc)
    ground = ops.box((-25, -20, 200), (50, 40, 40), name="ground")
    ops.set_material([ground], "al")
    ops.set_ground(ground)
    # A uniform PETG bar, 120 mm long, hinged at the centre of its top face.
    bar = ops.box((-8, -6, 80), (16, 12, 120), name="pendulum")
    ops.set_material([bar], "petg")
    motor = ops.add_motor("mg90s", (0, -20, 200), (0, 1, 0),
                          mount_on=ground, cut_mount=True, name="servo")
    joint = ops.add_joint("revolute", ground, bar, (0, 0, 200), (0, 1, 0),
                          lower=-1.0, upper=1.0, name="pivot")
    ops.attach_motor(joint, motor)
    # A declared ideal bearing isolates the geometry and motor/control chain.
    # Library motor backlash, firmware quantisation and thermal effects remain.
    ops.set_joint_physics(joint, source="declared", backlash=0.0,
                          friction={"coulomb": 0.0, "viscous": 0.0,
                                    "stribeck": 0.0, "stribeck_speed": 0.1,
                                    "static_ratio": 1.0})
    ops.set_control(period_s=0.02, latency_s=0.0, targets={"pivot": 0.0})
    doc.save(str(output / "pendulum.rcad"))
    model = export_physical_model(doc, str(output / "pendulum.simrobot.json"), flex=False)
    # Independent closed-form dimensions/density; never copy exported mass/inertia.
    width, depth, length, density = 0.016, 0.012, 0.120, 1270.0
    mass = width * depth * length * density
    reference = {
        "mass_kg": mass, "com_m": [0.0, 0.0, 0.140],
        "inertia_kg_m2": [mass * (depth**2 + length**2) / 12,
                           mass * (width**2 + length**2) / 12,
                           mass * (width**2 + depth**2) / 12],
        "pivot_m": [0.0, 0.0, 0.200], "com_radius_m": length / 2,
        "gravity_m_s2": 9.81,
    }
    (output / "reference.json").write_text(json.dumps(reference, indent=2) + "\n")
    return model, reference


if __name__ == "__main__":
    build(sys.argv[1] if len(sys.argv) > 1 else ROOT / "runs/motorized-pendulum")
