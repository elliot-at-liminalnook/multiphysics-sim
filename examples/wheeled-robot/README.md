# Wheeled learning fixture

The versioned CAD model in `baseline/robot.rcad` has four physical links: a PETG
chassis, two N20-driven wheels and one passive rear wheel. It uses three continuous
axles, two encoders and a body IMU. `baseline/robot.simrobot.json` is the CAD export;
its source records the CAD SHA-256 and the benchmark assumptions saved in the CAD
document. Geometry is authored in millimetres and exported in SI units.

This is an uncalibrated benchmark. Wheel and chassis mass/inertia derive from CAD
geometry and material density. Motor mass and electrical/gearbox parameters come
from the library's estimates. Axle friction and rigid shaft-wheel coupling are
explicit estimates. The supply is modeled without a separate battery body, and
the rear wheel has no steering caster. These assumptions are not a manufactured
robot specification.

Recreate into a new directory using the CAD environment:

```sh
cad/.venv/bin/python cad/scripts/wheeled_learning_fixture.py output-directory
python3 cad/scripts/check_wheeled_fixture.py output-directory analytic-report.json
cargo run --release -p sim-runtime --example inspect_robot_contract -- output-directory/robot.simrobot.json
```

The independent analytic check verifies solid-cylinder wheel mass and all inertia
entries against CAD, plus the persisted source identity and topology. The shared
inspection API preserves all twelve entities and their relationships.

The [predictive baseline](predictive-baseline/README.md) now records a complete
10 s contact-enabled drive, exact experiment replay, and learned online forecast
queries. Prediction generalization and hardware calibration remain unresolved.
The shared runtime supports the passive
axle through explicit independent-coordinate selection, and advances the authored
IMU through its existing hybrid scheduler. The sensor and contact smoke recipes
exercise native/WASM parity and replay with the complete CAD robot. They command
zero winding voltage and are not walking or driving controllers.

The contact recipe uses a 0.125 ms timestep. It passes host parity over 20 ms;
the 0.25 ms case does not, and finer timestep trajectories remain unconverged.
See [the runtime evidence](../interactive/passive-coordinates-and-sensors.md).
A separate [IMU policy fixture](../interactive/imu-policy-observations.md) now
checks Rhai control and environment replay with the original CAD. Its shared
runtime tests also cover neural sensor inputs and causal trajectory prediction.
It is a short no-contact API case. Locomotion control, contact accuracy, learned
prediction quality and speed optimization remain required work.

The `velocity` profile of `prepare_imu_policy.mjs` supplies angular-velocity
requests to a reference-integrating Rhai controller. Shared
[controller forecast APIs](../interactive/controller-action-forecasts.md) train
on the actual held requests and expose read-only trajectory predictions in
native and browser environments. This remains a short no-contact acceptance case.

## Teleoperation drive

`baseline/robot.drive.json` (`sim.drive/1`) is the rover's drive profile: a
differential drive on `left axle` / `right axle`, with no lateral axis.
Geometry is `{"source": "model"}`. Track width, wheel radius and wheel signs
come from `robot.simrobot.json`, each with its provenance. The limits are
estimates. Max forward speed is 60% of the free-running wheel speed: the
N20 gearbox's 14.66 rad/s × 0.03 m = 0.44 m/s, giving 0.26 m/s. Max yaw is the
spin-in-place rate at the same wheel speed, 4.3 rad/s. Accelerations are
chosen, not motor-limited. Each provenance `source` states its derivation.
Full forward plus full yaw asks the outer wheel for more than its
free-running speed, because the limiter does not cap combined wheel speed.

`baseline/robot.controller.json` (`sim.controller-binding/1`) attaches
`clients/python/examples/diff_drive_rover.py` as the controller on the
`control.external` seam:

    cargo run -p sim-spatial -- --robot examples/wheeled-robot/baseline/robot.simrobot.json

Robot mode finds `<stem>.controller.json` beside the model, resolves the
profile against the model, starts the script with `clients/python` on
`PYTHONPATH` and appends `--drive-json '<sim.drive.resolved/1 JSON>'`.

Channels the controller sees, appended after the model's own seam sensors
(`<joint>.angle` rad and `<joint>.speed` rad/s per joint, `imu.*`):

- `command.forward` (m/s), `command.lateral` (m/s, always 0 here) and
  `command.yaw` (rad/s, positive counter-clockwise from above). This is the
  twist the viewer's run thread already limited with the shared rule
  (`kinematics::step`, on sim time).
- `command.heartbeat`: increases by one for every fresh request. The
  controller measures a request's age as the sim time since the heartbeat
  last changed. Heartbeat 0 means no request yet, and the twist is then
  treated as zero. At or beyond `timeout_s` (0.5 s) the deadman applies the
  profile's stop rule (ramp at `stop_decel`), whatever the twist channels
  say. A live twist outside the profile, such as nonzero lateral, is a
  protocol violation: the controller logs it to stderr and exits, and the
  run fails naming it.

Actuators it writes: `left axle.target` and `right axle.target` (rad). Each
frame it mixes the twist into joint rates (rad/s) and integrates them,
`target += period × rate`, exactly as `velocity-controller.rhai` does. The
targets start at 0.0, the seam's initial value. The CAD motor firmware tracks
these position references.

### The embedded adapter (browser) and Build mode

The binding also names an embedded program (`"embedded"`):
`drive-adapter.rhai` with its session recipe `drive-adapter.config.json`.
It is the same kinematic adapter for hosts that cannot start a process,
the browser above all. It reads the same four command channels, applies the
same controller-side deadman (`drive_update`, the Rust
`kinematics::HeartbeatDeadman`), mixes through the Rust functions
`sim-script` registers (`drive_differential_mix`; the geometry comes from
its `drive` parameter, the profile resolved against this model, never
literals) and integrates the same targets, rolling back a re-sampled time
as the Python program does. `sim_runtime::embedded_drive::build` builds the
embedded scene from `robot.simrobot.json`, the binding and these files. It
refuses a config whose servo supply voltage or temperature disagree with
the model, or whose wheel target envelope (±12000 rad) is too narrow for the
600 s session at the profile's top wheel rate (about 17.3 rad/s ×
600 s = 10 360 rad). The browser preset `rover-drive` (`web/README.md`)
runs it in the shared embedded session; the limiter and deadman are the
shared `kinematics::step` on simulation time, in Rust. This is a
compatibility path: the Python program stays the reference controller.

In Build mode, a system file that hosts this robot (`system_drive`) is now
also driven by the bound keys and gamepad, through the same one device
poller as Robot mode (`docs/rover-checklist.md` RV-41 to RV-43).

All of this was verified by reading the code only. Nothing here has been run
or calibrated against hardware. The Python kinematics are checked against
the Rust golden vectors by
`python3 -m unittest discover -s clients/python/tests -t clients/python -p test_drive.py`,
which was not run in the batch that added it.
