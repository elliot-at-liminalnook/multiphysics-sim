# Student improvement across multiple episodes

The selected network improves all three development cases, but **does not pass
the reserved push case or the additional 5 ms refinement**. It is available as an experimental browser preset,
not a promoted robust controller. Both the original and selected student miss
step 4 in the reverse-first validation: simultaneous clearance, unloading and
support lasts 100 ms rather than the required 200 ms. Do not weaken that gate.

The additional 5 ms simulation completes but step 0 qualifies for only 180 ms,
before the scheduled push; 8 / 9 swings pass and final body error is 0.576 mm.
`refined-5ms-status.json` preserves this failure. Passing 20 and 10 ms does not
establish timestep-independent stepping success. The next improvement needs a
larger support/clearance duration margin, with phase-varied disturbances and
fresh validation cases, rather than a relaxed 200 ms requirement.

| Case | Original qualified swings | Selected qualified swings | Original → selected final body error |
|---|---:|---:|---:|
| 0.5 N push, 20 ms physics | 9 / 9 | 9 / 9 | 0.670 → 0.559 mm |
| Same push, 10 ms physics | 8 / 9 | 9 / 9 | 0.674 → 0.565 mm |
| Unforced 60 seconds | 28 / 28 | 28 / 28 | 1.127 → 0.685 mm |
| Reserved reverse-first / −0.35 N push | 8 / 9 | 8 / 9 | 0.807 → 0.739 mm |

The selected full captures pass the independent stepping, body/tilt/yaw,
stopping and sampled internal-geometry checks in the three development cases.
They reproduce their training scores and final walking diagnostics. The original
minute failed the 1 mm stopping gate; the selected minute passes it. The reserved
case fails despite meeting the stopping gate, demonstrating why reward and final
position alone are insufficient.

The weakest-case reward rate improves from 0.9592000480 to 1.0067881594.
Two networks are rejected for numerical failures. The last evaluated network
improves the minute's score further but misses a refined-timestep swing and is
not selected. `search-status.json` preserves all nine evaluations; selection is
evaluation 6. `validation-status.json` includes the failed reserved case as well
as successful development checks. These are simulation development results with
provisional physics, not hardware transfer evidence.

This experiment starts from the distilled student and changes only its neural
weights. CAD properties, the 5 mm reference lift, motor limits, the 45 actor
features, and the walking task remain unchanged. It uses the existing shared
Rust paired parameter search, not PPO or a reproduction of a published RL method.

The fixed development suite contains a 24-second forward/reverse walk with a
0.5 N sideways push at both 20 and 10 ms physics timesteps, plus the unforced
60-second sequence. Control remains at 50 Hz. Each candidate is scored by its
worst episode's reward per simulated second. This makes the weak case matter and
avoids giving the longer episode more weight simply for surviving longer.
Successful optimization still does not replace independent walking acceptance.

`search.recipe.json` fixes the three cases, seed, four paired search iterations,
and 0.001 parameter perturbation before evaluation. There are nine evaluated
networks including the original. The perturbation is in network parameter units,
not a commanded joint angle; the existing ±0.05 rad network-output bound remains.
All three cases run for every candidate. A failed or truncated-by-error episode
has diagnostic accrued reward but no selectable score. Per-case outcomes are
written immediately; the output directory must be new to protect prior evidence.

`validation.json` declares a separate reverse-first sequence with a −0.35 N
sideways push from 9.00 to 9.24 seconds. It is excluded from optimization. These
small forces are hypothetical development probes, not a hardware-calibrated
distribution or a broad robustness certificate. Once inspected, this validation
case must not be described as an untouched final test for future tuning.

The library's episode evaluator is used by both the original single-episode
trainer and this suite runner. It executes the production environment and retains
the last successful transition, walking outcomes, and error diagnostics. Focused
tests compare direct execution, duration normalization, incomplete episodes,
task termination, and invalid actions after a successful interval. CI also builds
the suite example. The browser runtime and actor contract remain shared.

Run from the repository root:

```sh
cargo test --locked -p sim-runtime --test policy_evaluation --test environment --test walking_task
cargo test --locked -p sim-domain-control --test neural
cargo build --locked --release -p sim-runtime --example train_policy_suite
mkdir -p runs/full-robot/learning/student-robustness
target/release/examples/train_policy_suite examples/full-robot/student-robustness/search.recipe.json runs/full-robot/learning/student-robustness/search-001
node examples/full-robot/summarize_policy_suite.mjs runs/full-robot/learning/student-robustness/search-001 examples/full-robot/student-robustness/search-status.json
node examples/full-robot/validate_policy_suite.mjs runs/full-robot/learning/student-robustness/search-001 examples/full-robot/student-robustness/validation.json runs/full-robot/learning/student-robustness/validation-001
```

The run archives resolved inputs, every evaluated policy, every case result, and
the selected policy. The versioned summary preserves rejected outcomes and hashes
the versioned source recipes. Full captures and independent geometry/stepping
checks are still required before promoting a controller. Ideal observations,
privileged upstream planning, hardware calibration, wider operating conditions,
and the browser's p95 processing target remain open work.

## Browser delivery

Choose **Quadruped · Improved student**, press Play, and use WASD to request
motion. The previous controllers remain selectable. The sidebar explicitly
states the failed push and 5 ms refinement; the viewport shows executed swing
counts and body error. The full minute is live Rust/WASM physics, not playback.

On the documented Intel i9-9980HK Mac / Chrome 152 host, the rendered minute
keeps up at 1.00035 simulated seconds per wall second during 56 seconds of active
motion. Active transition p95 is **22.56 ms**, above the **20 ms** target; render
scheduling p95 is 16.67 ms. This is a successful single-host realtime minute,
not latency headroom, display-presentation timing, command-to-visible-response
acceptance, turning/terrain certification, or broad hardware performance.

The browser records exactly the accepted native minute's inputs, controller
configuration and task. A separate complete 24-second native/WASM parity check
has maximum numerical difference 1.555e-10 with exact same-host replay/reset.
The viewer passes 26 checks covering all packaged controller presets, live
corrections, recording/replay, keyboard handling and narrow layout. The final
metadata update preserves the tested WASM, UI and model/controller data bytes;
the rendered minute verifies the final visible limitation text. Nineteen focused
Rust tests, the workspace/all-targets check, and WASM release build pass. Whole
GitHub CI status is not claimed.

`browser-status.json` records parity, usability, performance, source hashes and
remaining limitations. Reproduce after building the Rust WASM target:

```sh
node web/build-viewer.mjs runs/interactive/student-robustness/viewer --environment-only
node web/tests/environment.mjs runs/interactive/student-robustness/viewer robot-improved-student runs/full-robot/learning/student-robustness/validation-001/case-0.native.json runs/interactive/student-robustness/parity.json examples/full-robot/student-robustness/short.config.json
node web/tests/viewer.mjs runs/interactive/student-robustness/viewer runs/interactive/student-robustness/viewer-report.json
node web/tests/live_performance.mjs runs/interactive/student-robustness/viewer robot-improved-student runs/interactive/student-robustness/live-performance.json sustained-forward
node examples/full-robot/summarize_student_delivery.mjs
node web/serve-viewer.mjs runs/interactive/student-robustness/viewer 4186
```
