# Wheeled driving and predictive API baseline

The unchanged CAD robot completes a 10 s episode with one second of settling,
seven seconds requesting 5 rad/s on both axle joints, and two seconds braking.
Net displacement is 0.969619 m, or 0.096962 m/s over the full episode, without
a sampled fall. This includes settling/braking and is not maximum driving speed.
Both CAD joint axes are +Y; the motor housings' shaft directions must not be used
to infer command signs. The opposite-sign case is retained as `turn.capture.json`.

The original endpoint-state auxiliary solver failed at 5.9275 s on a tiny hybrid
interval. The existing `auxiliary_rate_unknowns` formulation completes the same
turning command sequence without changing component equations or tolerances.
Its first 0.1 s differs by at most 1.22e-11 across task observations. This startup
comparison does not establish full-duration timestep convergence.

`forward.capture.json` retains all joint/link motion, held controller commands,
actual actuator targets, authored IMU readings and original physics provenance.
The same Rust experiment API completes all 500 actions and reproduces all 501
frames exactly across checkpoint replay/resumption. The learned prediction API
passes 490 live queries: online inputs and kinematic references equal those
extracted from the recording, without changing the episode recording.
Compressed full check reports and `evidence.json` preserve those results.

`trajectory-model.json` predicts position, velocity and finite-interval
acceleration at 20, 100 and 200 ms. It was trained on 0–7 s of the turning episode,
with a 7.02–10 s chronological development window. Its constant-velocity reference
plus learned residual reduces normalized error from 0.529258 to 0.296620 there.
On the separate forward episode, learned error is 1.022461 versus 0.650392 for
constant velocity: broader training is needed. These are prediction experiments,
not learned actuator selection or closed-loop control improvement.

Normalization comes from each model's training residuals; scores from different
reference/model normalizations cannot be directly compared. The reported model
was selected after inspecting turning-window results. The forward episode was
evaluated separately and was not used for its training or selection.

From the repository root, build the examples and use fresh outputs:

```sh
cargo build --locked --release -p sim-runtime --example benchmark_environment --example train_motion_forecast --example evaluate_motion_forecast
node examples/wheeled-robot/prepare_drive_benchmark.mjs runs/new-wheel.input.json rates forward
target/release/examples/benchmark_environment runs/new-wheel.input.json runs/new-wheel 10 runs/new-wheel.cancel --motion
target/release/examples/train_motion_forecast examples/wheeled-robot/predictive-baseline/training-experiment.json runs/new-wheel-model
target/release/examples/evaluate_motion_forecast runs/new-wheel-model/model.json examples/wheeled-robot/predictive-baseline/forward.capture.json 0 10 runs/new-wheel-validation.json
```

CAD actuator/contact estimates and sensor calibration remain unverified on
hardware. Source-bound models retain their capture's runtime identity; a new
physics implementation requires explicit compatible data and validation.
