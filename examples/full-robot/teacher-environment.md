# Sampled teacher environment

The reusable `sim_runtime::environment::EmbeddedEnvironment` holds declared
controller inputs for a task interval, advances the existing embedded simulation,
and returns endpoint observations, a reward and separate termination/truncation
flags. It uses the same Rhai controller, servo firmware, motor dynamics, contacts
and mechanism integration as the interactive runtime. No learning algorithm or
walking controller is introduced by this adapter.

The robot task runs at 50 Hz (20 ms). Its existing Rhai feedback continues at
500 Hz and its motor firmware at 1 kHz. The current actions are three feedback
gains. They are not twelve independent learned motor commands. The 54 teacher
observations are twelve motor angles, speeds, reference angles and currents,
plus world-frame chassis position and velocity. Units are inferred from named
physical sources, not arbitrary JSON pointers or caller-supplied unit labels.
Observations use the current physical endpoint, including at reset time zero;
they do not silently reuse the previous sampled policy observation.

The provisional reward is the negative mean squared motor reference error,
scaled by 0.05 rad and multiplied by the elapsed seconds. It is an endpoint
quadrature diagnostic, not a validated stepping or balance objective. The task
currently declares no termination bounds: reaching 2.8 seconds is truncation,
not evidence of success. Generic endpoint bounds are available. Bounds are
task conditions sampled every action interval, not physical collision stops.
An invalid action preserves state; integration or observation/reward failures
require reset and do not yield a usable training transition.

Recordings preserve the task and the runtime recipe, seed, committed steps and
input changes. Unchanged intervals use declared initial values or the preceding
change. Replay rebuilds and re-executes the episode; it is not constant-time
state restoration. Browser replay yields progress between action intervals and
can be cancelled by terminating its worker. Failed environment attempts are
retained as diagnostics but environment replay currently accepts only successful
transition prefixes. The lower-level runtime's failure diagnostics remain intact.

## Reproduce from versioned files

The frozen `teacher-baseline/scene.json` and `config.json` contain the explicit,
uncalibrated drive-connection experiment. Their manifest links the source CAD
hash and records the recipes. Additional drive-connection backlash is estimated
as zero; this is not a measured robot property. The source CAD is unchanged.

```sh
cargo test --locked -p sim-runtime --test environment --test embedded_session --test drive_backlash
cargo run --locked --release -p sim-runtime --example run_environment -- examples/full-robot/teacher-baseline/scene.json examples/full-robot/teacher-baseline/config.json examples/full-robot/teacher-environment.json > robot-environment.json
cargo build --locked --release -p sim-web --target wasm32-unknown-unknown
npm ci --prefix web
node web/build-viewer.mjs runs/interactive/teacher-viewer --environment-only
node web/tests/environment.mjs runs/interactive/teacher-viewer robot-teacher-environment robot-environment.json robot-environment-browser.json
node web/serve-viewer.mjs runs/interactive/teacher-viewer 4175
```

The build requires `wasm-bindgen` matching the Cargo lockfile; set `WASM_BINDGEN`
if it is not on PATH. Browser tests use Playwright Chromium, or
`CHROME_EXECUTABLE` for an installed Chrome. The environment-only bundle requires
no ignored prior simulation captures. Select **Quadruped · teacher environment**.
The inspector shows the interval score and ideal-observation status. Step,
play/pause, save/replay, reset, camera controls and component focus remain usable.

## Direction

`active-goal.md` now requires realtime browser walking and WASD, allowing a
substantially simplified browser physics profile. This environment provides the
shared action/observation/task boundary for that work. The current detailed motor
recipe remains slower than realtime. It does not yet supply a walking policy,
teacher RL, student distillation, a deployable sensor contract or hardware transfer.
Next introduce and validate a reusable effective-actuator/browser profile, then
a baseline gait with controller-routed motion commands. Retain the detailed
profile for comparisons rather than waiting for it to become realtime.

## Validation at this checkpoint

All 141 common robot physical frames match the prior 1 ms production capture
exactly across 140 task intervals. Native/WASM parity passes all physical fields
and task transitions (maximum difference 3.305e-10 in a force channel); reset and
replay are exact. Nineteen focused Rust tests, all-workspace/all-target compilation,
26 full-viewer checks and six standalone-viewer checks pass. The standalone ZIP
is about 4.3 MB; delivery and evidence hashes are recorded in the accompanying
`teacher-environment-delivery.json` and `teacher-environment-status.json`.
