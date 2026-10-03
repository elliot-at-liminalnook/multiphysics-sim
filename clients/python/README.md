# simloop (Python)

A stdlib-only client (Python 3.9+) for the simulator's external-controller seam.

## Install

Add `clients/python` to `PYTHONPATH`; there are no dependencies.

    export PYTHONPATH=/path/to/physics-simulator/clients/python

## Use

```python
import sys
from simloop import Loop

loop = Loop.stdio()                           # the simulator spawned us and speaks over stdin/stdout
# loop = Loop.listen(("127.0.0.1", 9000))     # or: accept one simulator connection over TCP
# loop = Loop.listen_unix("/tmp/sim.sock")    # or: over a Unix socket
c = loop.contract                             # .element, .period, .sensors / .actuators as Channel(name, unit)
print(c, file=sys.stderr)                     # never print to stdout in stdio mode
for frame in loop:                            # frame.t (sim time), frame.seq, frame["angle"], frame.sensors, frame.values
    loop.send(voltage=-2.0 * frame["angle"])  # by name, or loop.send([v]) by position; unmentioned actuators hold
```

`Loop` raises `simloop.ProtocolError` on a malformed frame or seq mismatch and ends iteration on `close` or EOF.

## Example

    python3 examples/pi_controller.py --kp 4 --ki 20 --setpoint 1 --sensor speed --actuator voltage --limit 12

Point the simulator at that command line; `tests/fake_sim.py` plays the simulator for a smoke test.

## Tests

    python3 -m unittest discover -s clients/python/tests -t clients/python
    python3 clients/python/tests/fake_sim.py

## Protocol

Newline-delimited JSON, lockstep, simulator speaks first (see `crates/sim-couple`):

    -> {"type":"hello","element":"controller","period":0.001,"sensors":[{"name":"angle","unit":"rad"}],"actuators":[{"name":"voltage","unit":"V"}]}
    <- {"type":"ready"}
    -> {"type":"sample","seq":0,"t":0.0,"sensors":[0.1]}
    <- {"type":"act","seq":0,"actuators":[2.5]}
    -> {"type":"close"}

Each `act` echoes the sample's `seq` and carries exactly as many actuators as the hello declared.

## From the simulator side

`sim_couple::python(clients_root, script, args)` spawns a script with this
directory on `PYTHONPATH`, and `Runtime::attach_python(behavior,
clients_root, script, args)` attaches it to a `control.external` element in
one call. Give negative-valued flags as `--flag=value`.

## Drive kinematics (`simloop.drive`)

`simloop.drive` is the Python port of
`crates/sim-domain-control/src/drive/kinematics.rs`, stdlib-only: `BodyTwist`,
`scale`, `check_twist`, `limit`, `deadman_expired`, `step`,
`DifferentialDrive` / `Mecanum` (`mix`, `unmix`), `KinematicsError` (`.kind`
as in Rust), `ResolvedDrive.from_json` for the host's `sim.drive.resolved/1`
JSON and `DriveState`, the controller-side deadman. On the seam the run
thread's limited twist is authoritative: `DriveState` checks a live twist
against the profile and passes it through (so a halt stops at once), and
applies the stop rule from its last output when the heartbeat stops rising
(a heartbeat is fresh only when it is greater than the last, as in the Rust
limiter). `DriveState(limits, deadman, limit_live=True)` also limits live
twists, for hosts that send raw ones. Arithmetic
follows the Rust order, and `tests/test_drive.py` checks every case of the
golden file the Rust code generates
(`crates/sim-domain-control/tests/fixtures/drive_golden.json`) within its
tolerance (not run in the batch that added it):

    python3 -m unittest discover -s clients/python/tests -t clients/python -p test_drive.py

## Example: the wheeled rover's teleoperation controller

`examples/diff_drive_rover.py` drives `examples/wheeled-robot/baseline`.
Robot mode finds `robot.controller.json` beside the model, starts the script
and appends `--drive-json` with the resolved drive profile:

    cargo run -p sim-spatial -- --robot examples/wheeled-robot/baseline/robot.simrobot.json

To run it outside the viewer, write the resolved JSON to a file and pass
`--drive FILE`. Without either flag it refuses to start (exit 2). A hello
that lacks the command channels or wheel targets is refused before `ready`
(`Loop.stdio(check=...)`), so the viewer's handshake reports the reason. The README
in `examples/wheeled-robot` lists the channels and the deadman rule.

## Example: build and drive a rover over the viewer's REST API

`examples/build_rover_over_rest.py` (stdlib only) builds the rover of
`cad/scripts/wheeled_learning_fixture.py` through the native viewer's loopback
REST API (`http://127.0.0.1:8421`, `--port` to change). The rover has two driven
wheels (left and right, each turned by an N20 motor on a continuous axle read by
an encoder) and one unpowered passive wheel. The script works in this order:

1. Creates a new CAD document.
2. Adds four comment threads, pinned to the chassis, the left axle joint, the
   right axle joint and the passive wheel. It replies on the left axle thread
   with a `[left wheel](part:ID)` part link, and runs a link op that adds the
   drive motors and the IMU to the chassis thread.
3. Saves, exports `robot.simrobot.json` and checks the export's `cad_sha256`
   against the saved `.rcad`.
4. Derives `robot.drive.json` from the export and writes `robot.controller.json`.
5. Wires and runs the rover in Build mode.
6. Drives it in Robot mode and saves the drive recording (checklist RV-40; its
   Robot-mode requests are tagged RV-17, RV-18, RV-29, RV-32 and RV-33 from
   `docs/rover-checklist.md`).

Each request and response is in `<out>/rest_log.json`, tagged with its RV step
id. The script talks only to the viewer and never to hardware.

On a failure it cancels a timed-out job (`DELETE /v1/jobs/{id}`) and stops the
simulated drive. It then writes the log, names the failing step and exits 1. A
stop queued behind a long job may run late.

It refuses (exit 2) an output directory that already exists or lies under the
repository's `examples/`, `cad/` or `web/`. It also stops at RV-01 if the viewer
does not list a command it needs.

    cargo run -p sim-spatial -- examples/wheeled-robot/baseline/robot.rcad
    python3 clients/python/examples/build_rover_over_rest.py --out /tmp/rover-<date>

The seed `.rcad` is only used to bring CAD mode up (`--cad-seed` to change it).
It is never edited or saved.

Status: committed **unexecuted**. Its request shapes were checked by reading the
viewer's source, and no run evidence exists yet. Build-mode `link_file` and
`system_drive` and the export's `cad_sha256` fields come from work landing
alongside it.
