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

Locomotion is **not yet qualified**. The shared runtime now supports the passive
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
