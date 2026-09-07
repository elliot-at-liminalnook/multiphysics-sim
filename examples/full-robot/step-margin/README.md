# Student stepping margin and bounded mechanical recovery

This experiment keeps the improved student's neural weights, CAD robot, task
thresholds, and effective-servo force laws unchanged. It adjusts planner weight
shifts and pacing, then adds opt-in failure recovery to the shared Rust implicit
mechanics adapter. It is an experimental controller preset, not a calibrated
hardware controller or a general walking/terrain certificate.

The selected browser recipe is `retry-sustained.config.json` (60 seconds,
20 ms nominal physics, 50 Hz control). The short reversal/push recipe is
`retry-short.config.json`. Foot raise and lower phases each last 0.5 seconds
instead of 0.4; a transfer takes 2.2 rather than 2 seconds. Body support offsets
are explicit in the recipe. Requested forward speed remains only 1.25 mm/s.
Useful walking speed and general turning remain outstanding.

## Why change the planner and solver?

Earlier failures had different causes. A foot could lift far enough but fail
the three-support-foot force requirement: weight distribution, rather than lift
height, was the immediate problem. Simply increasing lift to 6 mm did not fix
this. Increasing all body shifts caused sampled internal geometry overlaps or
inverse-kinematics failure. Some reported overlaps were below a micrometre;
they indicate failure of the current geometric screen, not proven hardware
collision. The screen was not relaxed.

Moderate body shifts plus longer raise/lower phases improved supported-swing
duration. However, otherwise promising runs stopped when a single implicit
step failed to converge. Raising the iteration limit from 80 to 160 recovered
some cases but still failed at 0.985 seconds with 5 ms physics.

`RigidEmbedding::advance_implicit_mechanics` now uses the existing hybrid
interval scheduler to retry a failed step at smaller sizes. Every accepted
segment uses the existing backward-Euler solver and unchanged tolerances.
Controller samples and known force-schedule boundaries remain outside retries.
The adapter is restricted to pure force laws/effective servos; coupled motor
states and firmware events retain their existing adapters. Failure is atomic
with respect to the supplied mechanical seed.

The recipes allow four halvings per remaining interval and at most 32 accepted
segments per outer interval. This is not a global minimum-timestep guarantee,
local truncation-error control, or impact localization. Actual minimum accepted
steps and rejected-attempt reasons are recorded in `recovery-status.json`.

Cross-step Jacobian reuse flags do not affect the current effective-servo path:
it starts a fresh workspace each step. Within-solve modified Newton still
reuses derivatives. `workspace-check.json` records identical trajectories for
the cache-flag comparison. Flags-off is not a full-Newton reference.

## Measured native results

All cases use the existing independent clearance, unloading, support, stopping,
heading, tilt, and sampled internal-overlap gates in `check_online_steps.mjs`.

| Recovery recipe | Result | Evidence |
| --- | --- | --- |
| `retry-short` | Pass, 8 swings | 20 ms physics; direction changes and lateral push |
| `retry-5ms` | Pass, 8 swings | Minimum qualifying window 0.36 s; 0.646 mm final body error |
| `retry-diagonal` | Pass, 8 swings | Two diagonal pushes; 0.657 mm final body error |
| `retry-reverse` | Pass, 8 swings | Reverse-first and opposite lateral push; 0.684 mm final body error |
| `retry-x` | Pass, 8 swings | Reverse-first and X push at 10 ms physics |
| `retry-sustained` | Pass, 26 swings | 60 s unforced; 0.518 mm body error; heading 0.004981 rad, very close to 0.005 limit |
| `retry-minute` | **Fail** despite 26 supported swings | 60 s with three pushes; heading 0.005313 rad exceeds 0.005 limit |

The retained 5 ms diagnostic has 4,800 nominal intervals and two rejected
trials, each recovered with two 2.5 ms segments. The reverse-first 20 ms case
has one rejected trial, recovered with two 10 ms segments. These are sparse
recoveries, not evidence that arbitrary difficult motion will stay realtime.
Profiling was concurrent and includes overhead; its wall times are not browser
performance acceptance.

`study-status.json` preserves all completed and failed trial summaries, source
hashes, and accepted-case geometry checks. Configurations remain versioned so
ignored `runs/` is not the only baseline. `validation-plan.json` and
`validation-wave2.json` record when challenges were introduced; once used for
tuning, they are development data, not untouched final holdouts. Push strengths
are hypothetical bounded probes, not measured hardware uncertainty.

## Reproduce

From the repository root, build `run_environment`, then run a selected recipe:

```sh
cargo test --locked -p sim-domain-robot --test embedded_step
cargo test --locked -p sim-runtime --test embedded_session --test environment --test step_reference
cargo build --locked --release -p sim-runtime --example run_environment
mkdir -p runs/full-robot/learning/step-margin
target/release/examples/run_environment examples/full-robot/student-distillation/scene.json examples/full-robot/step-margin/retry-5ms.config.json examples/full-robot/walking-objective/task.json examples/full-robot/neural-teacher/train.actions.json --profile runs/full-robot/learning/step-margin/retry-5ms.profile.json > runs/full-robot/learning/step-margin/retry-5ms.native.json
node examples/full-robot/check_online_steps.mjs runs/full-robot/learning/step-margin/retry-5ms.native.json runs/full-robot/learning/step-margin/retry-5ms-acceptance
```

Use `neural-teacher/heldout.actions.json` for reverse/X cases and
`browser-residual-policy/sustained.actions.json` for minute cases. Paths are
relative to `examples/full-robot/`. The summary script checks recorded action
events against the declared schedule. A failed run/check exits nonzero but its
partial capture/report must be retained.

Choose **Quadruped · Paced student** in the browser bundle. It uses live Rust
physics and the same student network as the prior preset. Browser checks and
performance results are recorded separately in `browser-status.json`. Both the
short walk and the recovery-triggering reversal match native execution to a
maximum difference below 1.67e-10, with exact same-host replay/reset. The viewer
checks include live neural outputs, keyboard input, recording, replay and reset.

The rendered minute on the Intel i9-9980HK Mac / Chrome 152 / UHD 630 keeps up
at 1.00035 simulated seconds per wall second during active walking. Active
transition p95 is **28.5 ms**, above the **20 ms** target; rendering schedule
p95 is 16.67 ms. This improves stepping margin, not measured latency. It is a
single-host flat-floor result, not realtime acceptance for general turning,
terrain or arbitrary commands. Command-to-visible latency remains unmeasured.

Neither native/WASM agreement nor exact replay establishes hardware
accuracy. Ideal sensors, privileged planning, heading robustness, full trajectory
refinement, wider commands, and real-world calibration remain open work.

Browser reproduction after building `sim-web` for `wasm32-unknown-unknown`:

```sh
node web/build-viewer.mjs runs/interactive/step-margin/viewer --environment-only
node web/tests/environment.mjs runs/interactive/step-margin/viewer robot-paced-student runs/full-robot/learning/step-margin/retry-short.native.json runs/interactive/step-margin/parity.json examples/full-robot/step-margin/retry-short.config.json
node web/tests/environment.mjs runs/interactive/step-margin/viewer robot-paced-student runs/full-robot/learning/step-margin/retry-reverse.native.json runs/interactive/step-margin/recovery-parity.json examples/full-robot/step-margin/retry-reverse.config.json
node web/tests/viewer.mjs runs/interactive/step-margin/viewer runs/interactive/step-margin/viewer-report.json
node web/tests/live_performance.mjs runs/interactive/step-margin/viewer robot-paced-student runs/interactive/step-margin/live-performance.json sustained-forward
node web/serve-viewer.mjs runs/interactive/step-margin/viewer 4187
```

Set `WASM_BINDGEN` and `CHROME_EXECUTABLE` if these tools are not on the default
paths. The shareable archive is `~/robot-paced-student-2026-09-07.zip`;
`share-status.json` records its hash and the build manifest hash. Its `OPEN.txt`
explains local serving or static HTTPS hosting without installing CAD.
