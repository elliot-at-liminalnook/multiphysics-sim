#!/usr/bin/env python3
"""Joint-space PD with gravity feed-forward and an inner current loop for a
planar serial leg driven through the seam.

Sensors expected per joint J (in any order): ``J.angle`` (rad), ``J.speed``
(rad/s), ``J.current`` (A). Actuators: ``J.duty`` (−1..1).

    python3 leg_controller.py --joints hip,knee,ankle \
        --target -1.72,0.49,1.23 --kp 75,65,32 --kd 8,7,3.5 \
        --reduction 9,12,6 --links 0.40:2.5:0.20,0.40:1.8:0.20,0.22:0.7:0.044

The law is the one the hand-written leg used: requested joint torque =
kp·(target − q) − kd·q̇ + gravity(q); desired current = torque / (reduction ·
kt) clamped to the current limit; voltage = R·i_des + kb·ω_motor +
current_kp·(i_des − i); duty = voltage / supply, clamped. Gravity torques
come from the link table the controller is given — a controller knows its
own robot — with the base horizontal at the origin and joint angles
relative, link 0 hanging from the base.
"""
from __future__ import annotations

import argparse
import math
import sys

from simloop import Loop


def floats(text: str) -> list[float]:
    return [float(x) for x in text.split(",")] if text else []


def gravity_torques(angles: list[float], links: list[tuple[float, float, float]], g: float) -> list[float]:
    """Torque each joint must supply to hold the chain still under gravity."""
    phi = 0.0
    joints = [(0.0, 0.0)]
    coms = []
    x = y = 0.0
    for (length, _mass, com), theta in zip(links, angles):
        phi += theta
        coms.append((x + com * math.cos(phi), y + com * math.sin(phi)))
        x += length * math.cos(phi)
        y += length * math.sin(phi)
        joints.append((x, y))
    out = []
    for j in range(len(links)):
        xj = joints[j][0]
        out.append(sum(links[i][1] * g * (coms[i][0] - xj) for i in range(j, len(links))))
    return out


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--joints", required=True, help="comma-separated joint names in chain order")
    p.add_argument("--target", required=True, help="target joint angles (rad), comma-separated")
    p.add_argument("--kp", required=True)
    p.add_argument("--kd", required=True)
    p.add_argument("--reduction", required=True, help="gear reduction per joint")
    p.add_argument("--links", required=True, help="length:mass:com per link, comma-separated")
    p.add_argument("--kt", type=float, default=0.075, help="motor torque constant (N·m/A)")
    p.add_argument("--resistance", type=float, default=0.35)
    p.add_argument("--current-kp", type=float, default=0.35)
    p.add_argument("--current-limit", type=float, default=18.0)
    p.add_argument("--supply", type=float, default=48.0)
    p.add_argument("--duty-limit", type=float, default=1.0)
    p.add_argument("--gravity", type=float, default=9.80665)
    p.add_argument("--step-at", type=float, default=math.inf, help="time at which the target replaces the initial pose (before it, hold --hold)")
    p.add_argument("--hold", default="", help="pose to hold before --step-at (defaults to the target)")
    p.add_argument("--off", action="store_true", help="send zero duty (a passive leg)")
    args = p.parse_args()

    joints = args.joints.split(",")
    target = floats(args.target)
    hold = floats(args.hold) or target
    kp, kd, reduction = floats(args.kp), floats(args.kd), floats(args.reduction)
    links = [tuple(float(v) for v in item.split(":")) for item in args.links.split(",")]
    n = len(joints)
    if not all(len(v) == n for v in (target, hold, kp, kd, reduction, links)):
        print("leg_controller: every per-joint list needs one entry per joint", file=sys.stderr)
        return 2

    loop = Loop.stdio()
    c = loop.contract
    print(f"leg_controller: {c.element} period={c.period} joints={joints}", file=sys.stderr)
    for frame in loop:
        if args.off:
            loop.send(**{f"{j}.duty": 0.0 for j in joints})
            continue
        q = [frame[f"{j}.angle"] for j in joints]
        qd = [frame[f"{j}.speed"] for j in joints]
        i_meas = [frame[f"{j}.current"] for j in joints]
        goal = target if frame.t >= args.step_at else hold
        gravity = gravity_torques(q, links, args.gravity)
        duty = {}
        for k, j in enumerate(joints):
            torque = kp[k] * (goal[k] - q[k]) - kd[k] * qd[k] + gravity[k]
            i_des = max(-args.current_limit, min(args.current_limit, torque / (reduction[k] * args.kt)))
            motor_speed = reduction[k] * qd[k]
            voltage = args.resistance * i_des + args.kt * motor_speed + args.current_kp * (i_des - i_meas[k])
            duty[f"{j}.duty"] = max(-args.duty_limit, min(args.duty_limit, voltage / args.supply))
        loop.send(**duty)
    return 0


if __name__ == "__main__":
    sys.exit(main())
