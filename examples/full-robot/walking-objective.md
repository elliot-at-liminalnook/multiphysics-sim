# A task score that measures executed walking

The `robot-walking-objective` browser preset keeps the existing student network,
5 mm reference lift, actuator limits and physical push. It adds body-reference
tracking and executed swing qualification to the task score. The viewport and
sidebar show qualified/failed counts and body error. No neural weights are
trained or promoted by this change.

Previously the run that stopped 41 mm off reference scored slightly higher than
a successful small-push run. The new shared Rust `WalkingMonitor` corrects that
specific ranking while retaining the original reward terms:

| Saved development run | Original score | New score | Qualified swings |
|---|---:|---:|---:|
| 0.1 N lateral push | 23.77514 | 24.11729 | 9 / 9 |
| 0.5 N lateral push | 23.77503 | 24.11633 | 9 / 9 |
| 5 N lateral push | 23.77552 | 23.00347 | 8 / 9 |
| 15 N lateral push | 23.77786 | 14.91099 | 8 / 9 |
| 0.5 N push, 10 ms physics | 23.78228 | 23.02080 | 8 / 9 |
| Unforced, 10 ms physics | 23.78227 | 23.02117 | 8 / 9 |
| 0.5 N push, 5 ms physics | 23.77912 | 23.00883 | 8 / 9 |

These are observed development cases, not an independent final robustness test
or proof that the objective cannot be exploited. Task weights are explicit
engineering choices; they are not calibrated physical parameters.

## Definition and evidence

At each 50 Hz endpoint, body position is compared with the walking reference
held over the just-executed controller interval. The record includes that
reference's sample index. The new error cost is the squared world-space position
error divided by (5 mm)², capped at 1, with weight 0.5 per second. It therefore
saturates outside 5 mm at a maximum penalty of 0.5 per simulated second. This
bounds one source of incentive to terminate a bad run early; it does not prove
the complete reward is immune to early-termination or other policy exploits.
The independent 1 mm stopping acceptance gate remains unchanged.

During raise/lower phases, the monitor records current compiled-surface floor
clearance and current contact forces. When that swing ends, it calls the same
`evaluate_lift` function used by offline commissioning. A qualified swing needs
at least 1 mm clearance, at most 0.1 N absolute swing-foot floor load, and at least
1 N on each other support foot simultaneously for 200 ms. Qualification awards
0.1 once; failure subtracts 1 once. A horizon or task termination interrupting
the swing cannot earn a completion bonus. Qualification is a sampled lift check,
not a separate certification of landing, slip, balance, complete walking, or
between-sample contact accuracy. Those remain independent acceptance checks.

For both the small push and the 15 N failure, live runs reproduce **every one of
1,201 original physical frames**, actor observations/actions, and original reward
terms exactly. Online outcomes match the offline phase-window checks; total
scores match re-evaluation of the original saved physics. Task information is
exposed in `Transition.walking` and the task contract; it does not silently add
privileged values to the actor's observation vector. CAD remains unchanged.

Four focused tests check simultaneous qualification, once-only rewards,
interrupted swings, invalid definitions, and the bounded body cost. The existing
environment, session and lift tests also pass. Native/WASM parity covers the full
24-second push sequence and the new task transitions, with exact same-host replay
and reset. Browser tests verify live counts, force onset/release and recording.

On the documented Intel Mac/Chrome host, the rendered 24-second task episode
reaches **1.003×** during active motion. Active transition p95 is **22.65 ms**,
still above the **20 ms** target; render scheduling p95 is **16.67 ms**. These
measurements preserve the realtime experience but do not establish an optimization
speedup, display-presentation timing, or command-to-visible-response latency.

## Reference-margin experiment — not promoted

A separate candidate changes only the reference lift from 5 to 6 mm. Neural
weights, physical properties, motor limits and acceptance gates stay fixed.
The small-push coarse run and the 0.5 N refined/reverse-first runs qualify all
nine swings. However, the unforced minute still stops **1.061 mm** off reference,
outside the 1 mm gate, and the 5 ms/0.5 N run encounters a Newton convergence
failure at step 3401, near 17.005 seconds. Its last committed environment frame
is at 17.00 seconds; the partial run is not a valid completed learning episode.
This candidate is preserved for investigation, not substituted for the browser's
existing controller. Better short-run clearance alone does not establish a robust
policy or timestep-independent simulation.

Next work is closed-loop improvement using the corrected task, wider validation
of the support margin, and diagnosis of the refined candidate's numerical failure.
The student and upstream planner still use provisional ideal observations;
sensor integration, hardware calibration, stronger recovery and the browser's
20 ms p95 target remain open.

## Reproduce

```sh
cargo test --locked -p sim-runtime --test walking_task --test environment --test embedded_session --test lift
cargo build --locked --release -p sim-runtime --example run_environment --example rescore_walking
mkdir -p runs/full-robot/learning/walking-objective
target/release/examples/rescore_walking runs/full-robot/learning/student-disturbances/lateral.native.json examples/full-robot/walking-objective/walking.json > runs/full-robot/learning/walking-objective/lateral.rescore.json
target/release/examples/run_environment examples/full-robot/student-distillation/scene.json examples/full-robot/student-disturbances/lateral.config.json examples/full-robot/walking-objective/task.json examples/full-robot/neural-teacher/train.actions.json > runs/full-robot/learning/walking-objective/lateral.native.json
node examples/full-robot/check_walking_objective.mjs runs/full-robot/learning/student-disturbances/lateral.native.json runs/full-robot/learning/walking-objective/lateral.native.json runs/full-robot/learning/walking-objective/lateral.rescore.json runs/full-robot/learning/student-disturbances/lateral-acceptance/summary.json runs/full-robot/learning/walking-objective/lateral.check.json
cargo build --locked --release -p sim-web --target wasm32-unknown-unknown
node web/build-viewer.mjs runs/interactive/walking-objective/viewer --environment-only
node web/tests/environment.mjs runs/interactive/walking-objective/viewer robot-walking-objective runs/full-robot/learning/walking-objective/lateral.native.json runs/interactive/walking-objective/parity.json
node web/tests/viewer.mjs runs/interactive/walking-objective/viewer runs/interactive/walking-objective/viewer-report.json
node web/tests/live_performance.mjs runs/interactive/walking-objective/viewer robot-walking-objective runs/interactive/walking-objective/live-performance.json forward-reverse
node web/serve-viewer.mjs runs/interactive/walking-objective/viewer 4185
```

Recreate the original captures using `student-disturbances.md` if needed. Repeat
rescoring for the other named cases. For reference-margin candidates, use the
versioned `walking-objective/lift-6mm*.config.json` files; the sustained case uses
`browser-residual-policy/sustained.actions.json`, and the reverse-first case uses
`neural-teacher/heldout.actions.json` with `lift-6mm-refined.config.json`.
The failed 5 ms simulation must remain a reported failure, not a partial success.
Run timing without competing jobs. The 24-second timing probe does not replace
sustained walking/turning/terrain or command-to-visible-response acceptance.
