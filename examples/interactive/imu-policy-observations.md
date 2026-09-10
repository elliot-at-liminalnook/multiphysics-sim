# Authored IMUs in policies, trajectories and environments

Set `policy.imu_observations` to exact CAD sensor names. The shared Rust adapter
adds the registered `robot.articulated` IMU channels `imu.<name>.ax/ay/az` (specific
force, m/s²) and `gx/gy/gz` (angular velocity, rad/s), plus `available` (0 or 1)
and `age_s` (seconds). Values use the authored sensor axes, mounting point, noise,
bias, quantization and range. No sensor physics is duplicated in the controller.
Metadata records the CAD ID, parsed definition and effective sampling schedule.
The parsed definition may include legacy schema defaults; the original
`RobotDocument` inspection preserves which properties were actually authored.

Rhai and neural controllers read the latest committed sample before the next
physics interval. Sensor sampling remains on the shared hybrid clock. Before the
first tick, `available=0`; values and age are zero placeholders, not measurements.
Neural policies selecting a physical sensor channel or age must also select that
sensor's availability channel. Missing, ambiguous and non-IMU names are rejected.
This requirement concerns observation validity, not gait or speed constraints.

Environment transitions and `MotionSnapshot` retain typed `imu_samples`, including
the optional sample timestamp. These are separate from ideal task observations,
rewards and failure bounds. They do not turn privileged state into hardware
observations. Old configurations and captures without IMUs omit the new fields.

`ForecastRecipe.imu_observations` selects the same eight channels, in recipe order,
after kinematic/terrain inputs and before action history. Only the current sensor
sample enters a prediction. Forecast training validates recorded sensor identity,
link and time; online prediction reads the same committed runtime sensor state.
Use `future_action_offset()` instead of hardcoded neural input positions.

Mechanical chart order, full joint-state order and CAD motor order are distinct:

- Motor initialization uses the actuator's resolved joint position, including
  transmission-dependent positions, so passive-first charts do not preload motors.
- Environment joint observations select any uniquely named articulated DOF,
  including passive or dependent joints, and derive angular/linear units from CAD.
- Actuator references use motor order; requesting a passive joint reference fails.

## Reproduce

```
cargo test -p sim-runtime --test imu_policy
node examples/wheeled-robot/prepare_imu_policy.mjs runs/imu-policy
cargo run --release -p sim-runtime --example run_environment -- runs/imu-policy/scene.json runs/imu-policy/config.json runs/imu-policy/task.json
```

The wheeled fixture retains its original CAD and runs a 30 ms sensor-conditioned
policy without contact. Tests also exercise neural inference, causal prediction,
reset/replay, reordered charts, transmission-dependent initialization and a
synthetic prismatic variation. The prismatic case is an explicit test mutation,
not a CAD baseline. The original quadruped has no authored IMUs; none are invented.
The packaging helper never writes CAD/export sources; JSON transport normalizes
eight negative-zero geometry entries to zero, recorded in the identity check.

This does not qualify locomotion, learned prediction accuracy, realtime performance
or sim-to-real transfer. Forecast actions still describe angle targets and existing
gravity/terrain restrictions remain. Body/point feedback and online step-reference
planning still need actuator-aware treatment of passive charts. Contact timestep
convergence on the wheeled model remains unresolved. Full robot-bound dynamics
contracts, generic actions, optimization and fidelity APIs remain in the delivery
ledger in `shared-robot-learning-plan.md`.

`imu-policy-evidence-v1.json` archives the builds, recipes and results: 46 runtime
tests pass; the native/WASM 30 ms episode agrees within 4.4e-19 with exact browser
reset/replay. The original quadruped's 0.1 s prefix retains identical physical
frames, transitions, contracts and recording, excluding frame wall time. These
short comparisons do not requalify sustained speed or timestep accuracy.
