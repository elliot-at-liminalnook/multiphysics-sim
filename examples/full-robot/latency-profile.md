# Browser latency investigation

The delivered reversal crawl sustains realtime average speed, but its slowest
updates miss the 20 ms target. This investigation separates Rust work from
transport and UI updates, then tests numerical precision as an explicit browser
tradeoff. No precision change is promoted. The existing viewer remains available.

## What the measurements show

On the documented i9-9980HK Mac/Chrome 152 reference, a WebGL-enabled one-minute
baseline has active-motion p95 round trips of 29.03 ms. Across the episode:

| Region | Mean | p95 |
|---|---:|---:|
| Rust/WASM call, including frame serialization | 14.87 ms | 27.89 ms |
| Worker JSON parsing | 0.151 ms | 0.215 ms |
| Transport and main-thread dispatch | 2.09 ms | 7.27 ms |
| Frame UI update to DOM observer | 0.414 ms | 0.530 ms |

These p95 values are **not additive**. Frame UI timing does not include display
presentation. Raising/lowering phases have approximately 34 ms p95 WASM calls;
idle phases are much cheaper. Standing must not dilute the walking measurement.

The standalone native profile takes 31.93 s for the same 60 simulated seconds:

| Work | Time | Fraction of native wall time |
|---|---:|---:|
| Numerical Jacobian assembly | 19.15 s | 60.0% |
| Residual evaluations outside assembly | 7.50 s | 23.5% |
| Inter-part collision queries | 8.47 s | 26.5% |
| Online reference planning | 1.47 s | 4.6% |
| Body and foot feedback | 1.07 s | 3.4% |
| Linear factorization | 0.070 s | 0.2% |

Collision work is nested within other rows; the table must not be summed. There
are 5,537 Jacobian assemblies and 138,426 mechanical dynamics preparations.
All 3,001 native frames, transitions and the recording match the unprofiled
capture exactly after excluding wall timing. Profiling adds overhead, so its
wall time is diagnostic rather than an acceptance benchmark.

## Precision experiment

The retained profile uses Newton absolute/relative tolerances of 1e-8. Generated
`precision-experiment/` recipes change only these two settings; physical
properties, controller, timestep and task acceptance limits remain unchanged.

- **1e-6:** the 24 s reversal passes all nine swings. Maximum foot difference
  from the retained profile is 2.94e-11 m.
- **1e-5:** the reversal passes all nine swings; the sustained minute passes all
  28 and stops with approximately 0.965 mm body error. Maximum foot difference
  over the minute is 7.60e-10 m. These tiny differences are numerical comparisons,
  not physical accuracy claims.
- **1e-4:** fails at 0.92 s during the first lift. The solver reports a mechanical
  velocity residual of 0.1104 after seven iterations. The incomplete capture is
  retained as failed evidence; it cannot enter a trajectory acceptance comparison.

The rendered 1e-5 candidate still misses the target: active p95 is 27.47 ms,
versus 29.03 ms for the baseline. This single pair suggests only a modest gain,
not a robust speedup claim. Both sustain realtime average speed. The candidate's
keyboard recording exactly matches its native recipe, seed, actions and task;
full numerical native/WASM trajectory parity for this candidate is not promoted.

Precision alone is insufficient. The next experiment should address repeated
whole-robot derivative work or explicitly simplify inter-part collision physics
for the browser. If collision forces are omitted, retain independent geometry
checks: an empty force list must not be accepted as evidence of no collision.
Ground/terrain contact, motor-to-foot motion, support and stopping remain required.
The detailed model and current browser profile must remain available.

## Reproduce

```sh
node examples/full-robot/prepare_precision.mjs
cargo build --release -p sim-runtime --example run_environment --example evaluate_lift
target/release/examples/run_environment \
  examples/full-robot/browser-reversal/scene.json \
  examples/full-robot/browser-reversal/config.json \
  examples/full-robot/browser-reversal/task.json \
  examples/full-robot/browser-reversal/sustained.actions.json \
  --profile runs/profile.json > runs/profiled-capture.json
node examples/interactive/check_profile_capture.mjs runs/unprofiled-capture.json runs/profiled-capture.json
```

Use `precision-experiment/1e-5-long.config.json` with the same scene/task/actions
for the candidate, and `check_online_steps.mjs` plus `compare_effective_servo.mjs`
for the unchanged task gates and trajectory comparison. Short recipes use the
24 s `forward-reverse.actions.json` schedule.

`web/tests/live_performance.mjs` requests opt-in worker timing outside physical
frames and recordings. It saves per-transition `.timing.json` data, phase-level
breakdowns, screenshots and keyboard recordings. `latency-profile-status.json`
preserves measured results, input/capture hashes and failure details. No hardware
accuracy, uneven-terrain, learned-controller or full realtime acceptance is claimed.
