# Typed controller action forecasts

`ForecastRecipe` now declares one action layer:

- `actuator_targets`: the existing motor-angle interface, unchanged for old models.
- `controller_inputs`: the complete runtime `InputChannel` contract, including
  each name, quantity, software bounds and initial value. The recipe can reorder
  channels; named binding resolves runtime values into that order.

Controller inputs are requests to the existing controller. An angular velocity
input remains rad/s; a gain remains dimensionless. This does not turn a request
into a direct torque or voltage actuator. New physical actuator modes still need
their own supported runtime execution contract.

Controller-conditioned models also require `controller_context`, built by
`forecast_actions::ControllerContext::from_runtime`. It retains the existing
typed program, policy, clock, motion gate, trajectory and initial target definitions.
Changed interpretation is rejected even if the inputs have the same names and
units. Named input reordering is allowed. Version 2 models additionally require a
separate `physics_context`, covering physics settings and runtime library sources;
see `physics-bound-forecasts.md` for binding and migration rules.

Training reconstructs held inputs from the actual replay recording, checks any
recorded endpoint input values and rejects changes hidden inside a forecast
interval. Use a finer observation period when commands change more frequently.
Future commands belong to their own intervals; future states and sensor samples
do not enter earlier inputs. Speed/PPO recordings use the same reconstruction.

`EmbeddedEnvironment::predict_controller_trajectory` queries a trained model at
the current committed state. Supply the previous committed `MotionSnapshot` from
the same episode and future controls in runtime input order. The environment
provides the actual previously held controls. The result carries a typed action
sequence, checked physics context, features, kinematic reference and prediction in recipe order. Invalid
queries leave the environment and recording unchanged. History ownership remains
with the caller; clocks and topology are checked, but arbitrary caller-provided
history is not authenticated by replaying the episode.

The browser worker exposes the same Rust query with
`type: "predict_controller_trajectory"`, `model`, `previous` and `actions`.
Forecast queries do not advance physics or apply future commands. The existing
internal servo-target planner keeps its actuator-angle contract; it rejects
controller-layer models rather than labeling velocity requests as radians.

## Acceptance workflow

`prepare_controller_forecast` packages a recorded prefix as an explicit short
episode and derives its typed recipe. It records the horizon override and retains
the controller and robot. `check_controller_forecast` extracts actual trajectory
labels, performs a small fitting exercise, replays the episode and checks live
query inputs/references against the recorded training data. Its model and queries
can also be checked by `web/tests/environment.mjs` using `FORECAST_CASE_PATH`.
All fit samples are training data; these cases do not measure held-out accuracy.

The wheeled example integrates requested shaft velocities into position references
through Rhai while the original CAD motor/gearbox/firmware execute those references.
The quadruped case retains the archived fastest controller's velocity, yaw, gain,
residual and command-sequence inputs. Both use the same Rust preparation, training,
prediction and browser APIs. These short cases do not requalify sustained speed,
contact accuracy, realtime walking or learned transfer between morphologies.

Remaining work includes non-angular physical actuator execution, generalized
gravity/terrain features, full resolved robot/action identities, passive-aware
geometric feedback, prediction quality and shared optimization. Shared fidelity
comparison is available; physical convergence and realtime qualification remain open.
Forecast features also do not yet include controller memory or all internal
actuator states; binding the program does not make the observation history complete.

`controller-forecast-evidence-v1.json` retains the recipes, fitted models, native
queries, browser captures and builds from before the physics-binding requirement.
Use their archived executables or replay the recordings and rebuild version 2
models with the current workflow. The original evidence below remains historical.

The original checkpoint passed 43 targeted tests; four binding
tests were repeated after adding program-context and runtime-input reorder checks.
Native/WASM checks pass for eight wheeled queries over 30 ms and three quadruped
queries during a 100 ms episode, including rejected-query preservation and exact
reset/replay. Maximum checked differences are 8.9e-16 and 3.4e-11 respectively.
The quadruped's physical trajectory and held policy telemetry remain identical to
the previous fastest-controller prefix. Browser transition timing excludes the
forecast queries; this evidence does not qualify an inference performance budget.
