"""A sampled position supervisor; the exported servo firmware closes feedback.

Track two position steps, or hold zero as the falsifier. All timing uses
simulation frames. The log records what the external controller actually saw.
"""
import argparse
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "clients/python"))
from simloop import Loop


def reference(t):
    # Event location can return a sample a few ulps before its exact time.
    return 0.0 if t < 0.2 - 1e-9 else (0.3 if t < 1.6 - 1e-9 else -0.2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=["track", "hold"], default="track")
    parser.add_argument("--log", required=True)
    args = parser.parse_args()
    with Loop.stdio() as loop, open(args.log, "w") as log:
        if [c.name for c in loop.contract.actuators] != ["pivot.target"]:
            raise ValueError("expected exactly one pivot.target actuator")
        if {c.name for c in loop.contract.sensors} != {"pivot.angle", "pivot.speed"}:
            raise ValueError("expected pivot angle and speed sensors")
        for frame in loop:
            target = reference(frame.t) if args.mode == "track" else 0.0
            loop.send(**{"pivot.target": target})
            log.write(json.dumps({"seq": frame.seq, "t": frame.t,
                                  "angle_rad": frame["pivot.angle"],
                                  "speed_rad_s": frame["pivot.speed"],
                                  "target_rad": target}, allow_nan=False) + "\n")
            log.flush()


if __name__ == "__main__":
    main()
