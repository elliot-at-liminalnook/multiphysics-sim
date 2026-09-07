# Heading-aware walking task

The paced student can complete supported steps yet accumulate a small heading
error. The preceding reward scores body position, upright orientation and foot
support, but upright orientation does not specify which way the body faces.
This experiment adds an optional heading penalty to the shared Rust walking
task, without changing dynamics, actor observations or reference generation.

`task.json` measures the world-Z bearing of the reference body's +X axis against
the held planner yaw. The frame offset is explicitly zero for this robot.
Signed angle error wraps at ±pi. The penalty is the squared error normalized by
0.005 rad, capped at one, multiplied by 0.5 per simulated second. These are
declared task choices, not measured hardware accuracy. Independent acceptance
still checks stepping, geometry, stopping and a 0.005 rad final heading budget.

The actor still has 45 ideal input features: joint tracking/reference/velocity,
body gravity direction and angular velocity, and requested motion. It does not
receive task heading. Its current policy has no accumulated attitude estimate;
gyro rate and gravity alone at one instant do not identify absolute heading.
Reward improvement therefore cannot establish general heading recovery or
deployable sensing. Actual CAD sensor definitions and upstream planner state
estimation remain unresolved.

`search.recipe.json` evaluates each candidate on a short pushed walk, a minute
walk with 20 ms physics, and a minute with 5 ms physics. Controller sampling
remains 50 Hz. A deterministic paired weight search selects the best worst-case
reward per simulated second. This is a small policy-search experiment, not a
PPO implementation. Both timestep cases are development data.

`validation-plan.json` reserves two yaw-moment pulses from weight updates and
checkpoint selection. They are hypothetical bounded probes, not calibrated
hardware disturbances. Preserve their results even when they fail. Any further
tuning against them makes them development data and requires fresh evaluation
conditions.

Reproduce from the repository root, using a new search output directory:

```sh
cargo test --locked -p sim-runtime --test walking_task --test environment --test policy_evaluation
cargo build --locked --release -p sim-runtime --example run_environment --example train_policy_suite
target/release/examples/train_policy_suite examples/full-robot/heading-task/search.recipe.json runs/full-robot/learning/heading-task/search-001
```

`check_scoring.mjs OLD_CAPTURE NEW_CAPTURE REPORT` verifies identical physical
frames and controller telemetry, identical preexisting task terms, and exactly
the additional heading reward (apart from floating-point summation). Host wall
timing is excluded. `scoring-check.json` records the 24-second check.

The nine evaluations select the final candidate: worst development reward per
simulated second increases from 0.801969 to 0.867407 (8.16%). Every candidate and
its case scores are retained in `search-status.json`; `search-result.json`
contains the selected network and complete optimizer history. Seven candidates
did not improve the objective. These results do not establish global optimality.

Independent re-execution passes all five cases in `validation-status.json`:

| Case | Supported swings | Final heading error | Final body-position error |
| --- | --- | --- | --- |
| Short pushed walk | 8 | 0.000872 rad | 0.619 mm |
| Ordinary minute | 26 | 0.003536 rad | 0.556 mm |
| 5 ms minute | 26 | 0.004109 rad | 0.595 mm |
| Reserved positive twist | 8 | 0.000871 rad | 0.619 mm |
| Reserved negative twist | 8 | 0.000757 rad | 0.685 mm |

The original weights fail the 5 ms minute's 0.005 rad heading gate at
0.005314 rad. The selected weights pass it without changing the physical model
or acceptance budgets. Both original and selected weights pass the two gentle
reserved twists; these tests do not establish a large recovery envelope. The
ordinary and refined minutes are training cases, not held-out evidence.

After training, preserve and independently validate the selected weights with:

```sh
node examples/full-robot/heading-task/collect_search.mjs runs/full-robot/learning/heading-task/search-001
node examples/full-robot/heading-task/validate_selected.mjs runs/full-robot/learning/heading-task/selected-001
```

The browser preset `robot-heading-student` (**Heading student**) displays heading
error and its reward alongside supported steps. Native/WASM agreement, replay,
UI checks and rendered performance are recorded separately from development
scores. The detailed CAD model is unchanged. Requested walking speed remains
only 1.25 mm/s; general commands, heading estimation, terrain robustness and
hardware transfer are unfinished.

The rendered minute maintains 1.00033 simulated seconds per wall second during
active walking, completing 26 supported swings. Active transition p95 is
**23.6 ms**, above the **20 ms** target and the preceding measured 22.5 ms.
The largest measured WASM call is 153 ms; rendering schedule p95 is 16.67 ms.
The reference host is an Intel i9-9980HK Mac, Chrome 152, UHD 630 via ANGLE Metal.
This is one WebGL-rendered episode in headless Chrome, not display-presentation
timing, general command acceptance or a broad performance guarantee.

All 32 browser UI checks pass. The 24-second native/WASM comparison has maximum
numerical difference 1.45e-10, with exact same-host replay/reset. Rust tests pass
(five walking-task, eight environment, three policy-evaluation tests), as do
workspace/all-target compilation and the WASM release build. Representative
scoring-isolation, walking and browser parity cases are added to CI; remote CI
success is not claimed by these local results.

Reproduce browser verification after building the WASM target:

```sh
node web/build-viewer.mjs runs/interactive/heading-student/viewer --environment-only
node web/tests/environment.mjs runs/interactive/heading-student/viewer robot-heading-student runs/full-robot/learning/heading-task/selected-001/short.native.json runs/interactive/heading-student/parity.json examples/full-robot/heading-task/short.config.json
node web/tests/viewer.mjs runs/interactive/heading-student/viewer runs/interactive/heading-student/viewer-report.json
node web/tests/live_performance.mjs runs/interactive/heading-student/viewer robot-heading-student runs/interactive/heading-student/live-performance.json sustained-forward
node examples/full-robot/heading-task/collect_delivery.mjs
node web/serve-viewer.mjs runs/interactive/heading-student/viewer 4189
```

Set `WASM_BINDGEN` and `CHROME_EXECUTABLE` if needed. Run timing checks with
training and other browser tests stopped. `browser-status.json` records timings,
validation results, source hashes and equality of the native/browser keyboard
recipes. The shareable bundle is `~/robot-heading-student-2026-09-07.zip`; its
archive and build-manifest hashes are recorded in `share-status.json`.
