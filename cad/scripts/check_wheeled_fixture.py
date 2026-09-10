"""Independent analytic mass/inertia and source-identity check of the CAD export."""
import hashlib
import json
import math
import os
import sys

root, report_path = sys.argv[1:]
with open(os.path.join(root, "robot.simrobot.json")) as stream:
    model = json.load(stream)
with open(os.path.join(root, "robot.rcad"), "rb") as stream:
    cad_hash = hashlib.sha256(stream.read()).hexdigest()
assert model["source"]["cad_sha256"] == cad_hash
assert len(model["links"]) == 4 and len(model["joints"]) == 3
assert len(model["motors"]) == 2 and len(model["sensors"]) == 3
assert all(j["type"] == "continuous" and j["limits"] is None for j in model["joints"])
assert sum(j["motor"] is None for j in model["joints"]) == 1
comparisons = []
for name, radius, width in [("left wheel", .03, .012), ("right wheel", .03, .012),
                            ("passive wheel", .02, .01)]:
    link = next(link for link in model["links"] if link["name"] == name)
    density = model["materials"][link["material"]]["density"]
    mass = density * math.pi * radius**2 * width
    inertia = [mass*(3*radius**2+width**2)/12, mass*radius**2/2,
               mass*(3*radius**2+width**2)/12]
    assert math.isclose(link["mass"], mass, rel_tol=1e-8)
    for i in range(3):
        for j in range(3):
            assert math.isclose(link["inertia"][i][j], inertia[i] if i == j else 0., rel_tol=1e-8, abs_tol=1e-14)
    comparisons.append({"link": name, "mass_kg": mass, "inertia_diagonal_kg_m2": inertia})
report = {"version": 1, "passed": True, "cad_sha256": cad_hash, "comparisons": comparisons,
          "scope": "CAD solid-cylinder mass/inertia in SI and persisted CAD identity. No dynamics, actuator calibration, collision accuracy or locomotion qualification."}
with open(report_path, "x") as stream:
    json.dump(report, stream, indent=2)
print(json.dumps(report))
