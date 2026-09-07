"""Reproduce the recorded CAD-export parameter experiment without editing CAD.

Run from the repository root; optional argument selects an alternate output file.
All values come from the reviewed ledger, not a simulation-side motor default.
"""
import hashlib
import json
from pathlib import Path
import sys


def main():
    ledger = json.loads(Path("examples/full-robot/catalog-stall-correction-experiment.json").read_text())
    raw = Path(ledger["base_scene"]).read_bytes()
    if hashlib.sha256(raw).hexdigest() != ledger["base_scene_sha256"]:
        raise ValueError("base scene differs from recorded experiment")
    scene = json.loads(raw)
    if scene["robot"]["source"]["cad_sha256"] != ledger["source_cad_sha256"]:
        raise ValueError("CAD provenance mismatch")
    motors = scene["robot"]["motors"]
    names = [m["name"] for m in motors]
    changes = ledger["changes"]
    if len(set(names)) != len(names) or sorted(names) != sorted(c["motor"] for c in changes):
        raise ValueError("motor set differs from recorded experiment")
    for change in changes:
        motor = next(m for m in motors if m["name"] == change["motor"])
        for field, key in [("resistance", "resistance_ohm"), ("inductance", "inductance_h")]:
            if motor["electrical"][field] != change[key]["before"]:
                raise ValueError(f"original {field} differs for {motor['name']}")
            motor["electrical"][field] = change[key]["after"]
    output = (json.dumps(scene, indent=2) + "\n").encode()
    if hashlib.sha256(output).hexdigest() != ledger["output_scene_sha256"]:
        raise ValueError("reconstructed scene differs from recorded output")
    path = Path(sys.argv[1] if len(sys.argv) == 2 else ledger["output_scene"])
    if len(sys.argv) > 2:
        raise ValueError("usage: replay_catalog_stall_override.py [output.scene.json]")
    if path.exists() and path.read_bytes() != output:
        raise ValueError("refusing to replace a different artifact")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(output)
    print(f"Verified {len(changes)} recorded motor overrides: {path}")


if __name__ == "__main__":
    main()
