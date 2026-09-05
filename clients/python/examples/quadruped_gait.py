#!/usr/bin/env python3
"""A trot for a planar quadruped driven through the seam.

Legs are two-link chains hanging from the body at their hips; each joint
reports ``<leg>.<joint>.angle`` and ``<leg>.<joint>.speed`` and takes a torque
``<leg>.<joint>.torque``. Diagonal pairs (fl+rr, fr+rl) alternate: a foot in
stance sweeps backward under its hip at the walking speed; a foot in swing
returns forward along an arc. Foot targets become joint targets through the
two-link inverse kinematics, and joint PD produces the torques. Before the
trot starts the controller holds the standing pose so the body can settle.

    python3 quadruped_gait.py --stride 0.12 --period 0.6 --height 0.478 --start 0.5
"""
from __future__ import annotations

import argparse
import math
import sys

from simloop import Loop

LEGS = ["fl", "fr", "rl", "rr"]
PHASE = {"fl": 0.0, "rr": 0.0, "fr": 0.5, "rl": 0.5}


def inverse(x: float, y: float, l1: float, l2: float) -> tuple[float, float]:
    """Hip and knee angles (chain convention: link 0 along +x at zero, knee
    bending backward) placing the foot at (x, y) relative to the hip."""
    r2 = x * x + y * y
    c = max(-1.0, min(1.0, (r2 - l1 * l1 - l2 * l2) / (2.0 * l1 * l2)))
    knee = -math.acos(c)
    hip = math.atan2(y, x) - math.atan2(l2 * math.sin(knee), l1 + l2 * math.cos(knee))
    return hip, knee


def foot_target(phase: float, stride: float, lift: float, height: float, duty: float) -> tuple[float, float]:
    """Foot position relative to the hip at gait phase in [0, 1)."""
    if phase < duty:
        s = phase / duty
        return stride * (0.5 - s), -height
    s = (phase - duty) / (1.0 - duty)
    return stride * (s - 0.5), -height + lift * math.sin(math.pi * s)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--stride", type=float, default=0.12)
    p.add_argument("--period", type=float, default=0.6, help="gait period (s)")
    p.add_argument("--duty", type=float, default=0.5, help="stance fraction")
    p.add_argument("--lift", type=float, default=0.05, help="swing height (m)")
    p.add_argument("--height", type=float, default=0.478, help="hip height above the feet (m)")
    p.add_argument("--l1", type=float, default=0.25)
    p.add_argument("--l2", type=float, default=0.25)
    p.add_argument("--kp", type=float, default=60.0)
    p.add_argument("--kd", type=float, default=2.0)
    p.add_argument("--start", type=float, default=0.5, help="time the trot begins (s)")
    args = p.parse_args()

    loop = Loop.stdio()
    c = loop.contract
    print(f"quadruped_gait: {c.element} period={c.period} stride={args.stride}", file=sys.stderr)
    previous: dict[str, tuple[float, float]] = {}
    for frame in loop:
        t = frame.t
        torques = {}
        for leg in LEGS:
            if t < args.start:
                target = (0.0, -args.height)
            else:
                phase = ((t - args.start) / args.period + PHASE[leg]) % 1.0
                target = foot_target(phase, args.stride, args.lift, args.height, args.duty)
            hip, knee = inverse(target[0], target[1], args.l1, args.l2)
            # Target rates from the previous target, for the derivative term.
            last = previous.get(leg, (hip, knee))
            rates = ((hip - last[0]) / c.period, (knee - last[1]) / c.period)
            previous[leg] = (hip, knee)
            for joint, q_des, qd_des in (("hip", hip, rates[0]), ("knee", knee, rates[1])):
                q = frame[f"{leg}.{joint}.angle"]
                qd = frame[f"{leg}.{joint}.speed"]
                torques[f"{leg}.{joint}.torque"] = args.kp * (q_des - q) + args.kd * (qd_des - qd)
        loop.send(**torques)
    return 0


if __name__ == "__main__":
    sys.exit(main())
