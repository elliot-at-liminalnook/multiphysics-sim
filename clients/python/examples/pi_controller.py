#!/usr/bin/env python3
"""A PI controller on one sensor driving one actuator, over stdio.

    python3 pi_controller.py --kp 4 --ki 20 --setpoint 1.0 --sensor speed --actuator voltage --limit 12

The integral step is the simulator's declared period, and integration is
conditional (anti-windup): while the output is saturated, the integral only
moves if the error would pull it back out of saturation.
"""

import argparse
import math
import time
import sys

from simloop import Loop


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--kp", type=float, default=1.0, help="proportional gain (default 1)")
    p.add_argument("--ki", type=float, default=0.0, help="integral gain, per second (default 0)")
    p.add_argument("--setpoint", type=float, default=0.0, help="target sensor value (default 0)")
    p.add_argument("--sensor", help="sensor name (default: the first declared)")
    p.add_argument("--actuator", help="actuator name (default: the first declared)")
    p.add_argument("--limit", type=float, default=math.inf, help="clamp |output| to this (default: none)")
    p.add_argument("--busy", type=float, default=0.0, help="wall-clock seconds to spend computing each sample (for real-time tests)")
    args = p.parse_args()

    loop = Loop.stdio()
    c = loop.contract
    sensor = args.sensor or c.sensors[0].name
    actuator = args.actuator or c.actuators[0].name
    print(f"pi_controller: {c.element} period={c.period} {sensor} -> {actuator} kp={args.kp} ki={args.ki} setpoint={args.setpoint}", file=sys.stderr)

    integral = 0.0
    for frame in loop:
        error = args.setpoint - frame[sensor]
        raw = args.kp * error + args.ki * integral
        out = max(-args.limit, min(args.limit, raw))
        if out == raw or (raw > 0) != (error > 0):
            integral += error * c.period
        if args.busy > 0:
            time.sleep(args.busy)
        loop.send(**{actuator: out})


if __name__ == "__main__":
    main()
