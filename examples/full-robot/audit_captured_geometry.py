"""CLI for shared CAD-side captured-pose geometry inspection; run off the UI thread."""
import argparse
import json
from pathlib import Path
from robocad.capture_geometry import audit_captured_pair

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("cad", type=Path)
p.add_argument("scene", type=Path)
p.add_argument("capture", type=Path)
p.add_argument("left")
p.add_argument("right")
p.add_argument("times", nargs="+", type=float)
p.add_argument("--probe-frame", type=float, help="Track contact samples from this frame through every inspected pose")
p.add_argument("--probe-sdf-cells", action="store_true", help="Compare the eight surrounding distance-grid nodes with exact CAD solids")
p.add_argument("--points-only", action="store_true", help="Inspect contact points without whole-part distance queries; does not establish pair clearance")
a = p.parse_args()
print(json.dumps(audit_captured_pair(a.cad, json.loads(a.scene.read_text()),
    json.loads(a.capture.read_text()), a.left, a.right, a.times, a.probe_frame, a.probe_sdf_cells,
    measure_pair_distances=not a.points_only), indent=2))
