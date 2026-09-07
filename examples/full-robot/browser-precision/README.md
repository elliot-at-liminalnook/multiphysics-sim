# Browser numerical work

This experiment keeps the heading-aware student and all physical force laws.
It tests explicit Newton precision and the library's existing independent-block
constraint factorization. Neither changes the CAD model or the independent
walking acceptance thresholds. Every original closure equation and global rank
check remains active.

The baseline minute needs 39,405 Newton iterations, 4,306 derivative rebuilds,
and 13 fresh restarts. Profile buckets show 9.88 s in derivative assembly and
6.50 s in other residual calls; factorization of the outer Newton system takes
only 0.053 s. These buckets include nested work and must not all be added.
The earlier rendered run's lowering phase has p95 35.9 ms, versus 18.7 ms during
weight shifting. This motivates reducing residual/derivative work rather than
replacing the outer linear solver.

| Numerical recipe | Newton iterations | Derivative rebuilds | Native minute walking gate |
| --- | --- | --- | --- |
| Original 1e-8 precision | 39,405 | 4,306 | Pass |
| 1e-7 precision | 35,093 | 4,196 | Pass |
| 1e-6 precision | 31,080 | 4,146 | Pass |
| 1e-5 precision | 26,863 | 4,120 | Pass |
| Block factorization plus 1e-5 | 26,851 | 4,118 | Pass |

The precision-only 1e-5 rendered minute keeps up with realtime but has active
p95 22.86 ms and maximum WASM call 164.7 ms. It therefore still fails the 20 ms
target. Its original bundle and reports are under `runs/interactive/browser-precision`;
it is not promoted as a realtime solution.

Block factorization alone matches the original sampled foot motion within
1.01e-15 m in this minute. The combined recipe changes it by at most 1.30e-9 m.
Those are numerical agreements, not physical accuracy estimates. The library's
earlier block experiment on a different controller missed a strict roundoff
screen; this experiment does not erase that historical result.

`plan.json` records numerical screening bounds and the sequence of candidate
additions. `*.comparison.json` retains whole-trajectory kinematics and contact
force comparisons. `*.screen.json` applies the declared position, angle and
integrated-force-discrepancy limits. Contact-pair identities and gait phases
must match at every reporting sample. Force discrepancy integrates the magnitude
of force differences, so opposite errors cannot cancel. Sampling is 50 Hz;
these checks do not prove resolved impact accuracy.

Saved environment captures now expose the full frame-coordinate layout with
names and position/velocity units, including dependent prismatic coordinates.
The layout comes from Rust's constructed model, not an inferred CAD-list order.
The independent motor-coordinate list alone is insufficient to label every
entry of `joint_positions` and `joint_velocities`.

Reproduce a profiled minute from the repository root:

```sh
cargo build --locked --release -p sim-runtime --example run_environment --example evaluate_lift
mkdir -p runs/full-robot/learning/heading-performance
target/release/examples/run_environment examples/full-robot/student-distillation/scene.json examples/full-robot/browser-precision/block-1e-5.config.json examples/full-robot/heading-task/task.json examples/full-robot/browser-residual-policy/sustained.actions.json --profile runs/full-robot/learning/heading-performance/block-1e-5.profile.json > runs/full-robot/learning/heading-performance/block-1e-5.native.json
node examples/full-robot/check_online_steps.mjs runs/full-robot/learning/heading-performance/block-1e-5.native.json runs/full-robot/learning/heading-performance/block-1e-5-acceptance
```

Compare against an original `heading-task/sustained.config.json` capture with
`compare_mechanical_reuse.mjs --numerical-precision --block-factorization`, then
apply `check_precision.mjs`. Its layout-capture argument must contain the new
runtime metadata and the identical scene. A short run is sufficient for layout
metadata; all trajectory comparisons still cover the complete minute.

Broader controller behavior, refined-step and reserved-disturbance checks,
native/WASM parity and rendered browser performance are separate promotion
requirements. Requested speed is still 1.25 mm/s, with ideal observations and
a privileged planner. Hardware transfer and general terrain remain unfinished.

## Selected browser experiment

The selected `guarded.config.json` combines block factorization and 1e-5 Newton
precision with guarded backtracking and a 12-correction cached first-attempt
cap. Fresh restarts retain the original 80-correction budget. The minute needs
26,196 Newton iterations and has no timestep subdivisions. Its maximum sampled
foot difference is 1.34e-9 m. The numerical screen and ordinary-minute walking
check pass, as do the short, refined-minute, two reserved-twist and new
forward/turn/reverse/stop cases. These remain slow, flat-floor commissioning
cases rather than general walking acceptance.

The first refined recipe was invalid: it inherited a profile with fresh restarts
disabled while setting a cached-attempt cap. It was rejected at step zero, before
motion. The invalid recipe and initial report are retained; the corrected recipe
explicitly enables the browser's restart policy. `merge_validation.mjs` checks
that this is the only correction before merging the successful rerun.

The viewer now redraws for new physics frames, camera changes, selection, resize
and contact-visibility changes. It stops issuing WebGL draws for unchanged
paused scenes. Camera damping and simulation scheduling remain independent.
All 50 Hz physics transitions are retained; multiple completed states can still
be combined into the next display frame when the solver catches up.

| Rendered case | Active simulation/wall ratio | Active transition p95 | Actual draws |
| --- | --- | --- | --- |
| 60-second straight walk | 1.00007× | **20.98 ms — target missed** | 2,578 |
| 24-second forward/turn/reverse/stop | 1.00004× | **18.97 ms — target passed** | 1,033 |

The maximum measured WASM call in the straight minute is 74.2 ms. Conditional
drawing reduced redundant work but did not itself produce a p95 improvement:
the preceding continuously drawn guarded trial measured 20.55 ms. These are
individual runs with scheduling variation, not a repeated statistical speedup
claim. rAF scheduling p95 remains about 16.67 ms; that is not display-presentation
or command-to-visible-motion latency. The documented host is an Intel i9-9980HK
Mac, Chrome 152, UHD 630 via ANGLE Metal, with WebGL drawing in headless Chrome.

All 35 viewer checks pass, including actual redraws after physics updates and
an unchanged paused scene that stops drawing. The 24-second native/WASM check
has maximum numerical difference below 1.0e-9, with exact replay/reset. Eight
Rust environment tests, the independent-block pose/toggle-rejection test,
workspace/all-target checking and the WASM release build pass. Representative
short-walk, turning and parity cases are added to CI; remote CI success is not
claimed here.

Reproduce the broader checks with new output directories:

```sh
node examples/full-robot/heading-task/validate_selected.mjs runs/full-robot/learning/heading-performance/guarded-validation-fresh examples/full-robot/browser-precision/validation.recipe.json examples/full-robot/browser-precision/validation-status.json
node web/build-viewer.mjs runs/interactive/demand-render/viewer --environment-only
node web/tests/viewer.mjs runs/interactive/demand-render/viewer runs/interactive/demand-render/viewer-report.json
node web/tests/environment.mjs runs/interactive/demand-render/viewer robot-browser-solver runs/full-robot/learning/heading-performance/guarded-validation-fresh/short.native.json runs/interactive/demand-render/parity.json examples/full-robot/browser-precision/guarded-short.config.json
node web/tests/live_performance.mjs runs/interactive/demand-render/viewer robot-browser-solver runs/interactive/demand-render/live-performance.json sustained-forward
node web/tests/live_performance.mjs runs/interactive/demand-render/viewer robot-browser-solver runs/interactive/demand-render/turn-performance.json turn-reverse examples/full-robot/browser-precision/guarded-short.config.json
node web/serve-viewer.mjs runs/interactive/demand-render/viewer 4190
```

Set `WASM_BINDGEN` and `CHROME_EXECUTABLE` if needed. The performance tool now
accepts an explicit configuration override, records its hash and uses the same
actual viewer/worker path. Run timing checks with training and other tests
stopped. `study-status.json` retains failed timing candidates;
`browser-status.json` binds the selected cases to recipes, source hashes and
keyboard recordings. The served preset is **Browser solver**. Earlier controllers
remain selectable. The archive and manifest hashes are in `share-status.json`.
