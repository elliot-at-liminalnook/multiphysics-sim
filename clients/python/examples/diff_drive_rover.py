#!/usr/bin/env python3
"""The wheeled rover's controller: a teleoperation twist to wheel position targets.

    python3 diff_drive_rover.py --drive-json '<sim.drive.resolved/1 JSON>'
    python3 diff_drive_rover.py --drive resolved.json

The viewer finds ``robot.controller.json`` beside the model, starts this
script and appends ``--drive-json`` with the resolved drive profile: limits,
deadman and the drive geometry derived from the CAD model.

Sensors read (from the hello; refused by name before ``ready`` when missing,
so the host's handshake fails with this reason):
  ``command.forward`` (m/s), ``command.lateral`` (m/s), ``command.yaw``
  (rad/s): the twist the run thread already limited with the shared rule;
  ``command.heartbeat``: raised by one for every fresh request.
Actuators written: ``<wheel joint>.target`` (rad) for each wheel of the
geometry, an integrated position reference the CAD motor firmware follows.

Per sample: the deadman on heartbeat staleness (``DriveState``, sim time,
dt = the declared period).  A live twist is checked against the profile and
used as sent: the run thread's limited twist is authoritative, so a halt
stops at once and its deadman ramp keeps the profile's stop deceleration.
When the heartbeat stops rising for the deadman timeout, the stop rule
applies from the last output.  Then the profile's mixer (twist to joint rad/s),
then ``target += period * joint_rate`` exactly as
examples/wheeled-robot/velocity-controller.rhai does.  Targets start at 0.0,
the seam's initial held value.  Logs go to stderr only.

The output is a function of the sample sequence, not of how often it is
asked: when the runtime retries a slice that failed to converge
(``PhysicalRobot::advance`` restores a snapshot and re-runs it), the seam
samples the same simulation time again (a retry with halved sub-steps may
land a few ulps later).  A sample at a time not later than the last one plus
half a period rolls the controller back to its state before that time, so a
retry never integrates the wheel targets twice.
"""

from __future__ import annotations

import argparse
import copy
import sys

from simloop import Loop, ProtocolError
from simloop.drive import KinematicsError, ResolvedDrive, ResolvedDriveError, DriveState

COMMANDS = ("command.forward", "command.lateral", "command.yaw", "command.heartbeat")
REPORT_EVERY_S = 1.0
#: Samples kept for a retried slice's rollback (a retry re-samples at most one slice).
HISTORY = 16


def log(message: str) -> None:
    print(f"diff_drive_rover: {message}", file=sys.stderr)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--drive-json", help="the resolved drive (sim.drive.resolved/1) as JSON text; the host appends this")
    p.add_argument("--drive", help="a file holding the resolved drive JSON, for standalone use")
    args = p.parse_args()
    try:
        if args.drive_json is not None:
            resolved = ResolvedDrive.from_json(args.drive_json)
        elif args.drive is not None:
            with open(args.drive, encoding="utf-8") as f:
                resolved = ResolvedDrive.from_json(f.read())
        else:
            log("needs --drive-json JSON or --drive FILE (the resolved drive profile); refusing to start")
            return 2
    except (OSError, ResolvedDriveError) as e:
        log(f"{args.drive or '--drive-json'}: {e}")
        return 2
    mixer = resolved.mixer()
    state = DriveState(resolved.limits(), resolved.deadman())
    targets = {f"{joint}.target": 0.0 for joint in resolved.joints}

    def check(contract) -> None:
        # Runs before `ready`: a missing channel fails the host's handshake with this reason.
        have_sensors = {ch.name for ch in contract.sensors}
        have_actuators = {ch.name for ch in contract.actuators}
        missing = [n for n in COMMANDS if n not in have_sensors] + [n for n in targets if n not in have_actuators]
        if missing:
            raise ProtocolError(f"{contract.element}: the hello lacks {missing}; this controller needs sensors {list(COMMANDS)} and actuators {list(targets)}")

    loop = Loop.stdio(check=check)
    c = loop.contract
    sensors = {ch.name for ch in c.sensors}
    # Optional diagnostic: the seam's tachometers (`<joint>.speed`, rad/s, from
    # crates/sim-runtime/src/physical.rs) unmixed into the body twist they produce.
    speeds = [f"{joint}.speed" for joint in resolved.joints]
    measured = all(n in sensors for n in speeds)
    log(f"{c.element} period={c.period} {resolved!r} limits={resolved.limits()} deadman={resolved.deadman()} measured={'yes' if measured else 'no'}")

    next_report = 0.0
    # (sample time, wheel targets and drive state before that sample), oldest first.
    history = []
    for frame in loop:
        # A retried slice samples a time again: restore the state from before it.
        # Half a period of tolerance: a retried slice's sample times can differ by ulps.
        if history and frame.t <= history[-1][0] + 0.5 * c.period:
            while history and frame.t <= history[-1][0] + 0.5 * c.period:
                _, targets, state = history.pop()
            log(f"t={frame.t:.4f} s sampled again (a retried slice): rolled back")
        history.append((frame.t, dict(targets), copy.copy(state)))
        del history[:-HISTORY]
        request = (frame["command.forward"], frame["command.lateral"], frame["command.yaw"])
        try:
            twist, expired = state.update(frame.t, request, frame["command.heartbeat"], c.period)
            rates = mixer.mix(twist)
        except KinematicsError as e:
            # The run thread only sends twists inside the profile; anything else is a protocol violation.
            log(f"t={frame.t:.3f} s: refused twist {request} (heartbeat {frame['command.heartbeat']:.0f}): {e}")
            raise
        for name, rate in zip(targets, rates):
            targets[name] += c.period * rate
        loop.send(**targets)
        if frame.t >= next_report:
            next_report = frame.t + REPORT_EVERY_S
            seen = f" measured={tuple(round(v, 3) for v in mixer.unmix([frame[n] for n in speeds]))}" if measured else ""
            log(f"t={frame.t:.1f} s twist={tuple(round(v, 3) for v in twist)} expired={expired}{seen}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
