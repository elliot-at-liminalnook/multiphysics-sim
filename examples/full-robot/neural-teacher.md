# First learned residual teacher

`robot-neural-teacher` runs a small trained neural network inside the existing
Rust sampled controller. WASD requests the baseline crawl; the network adds small
motor-angle corrections after Rhai feedback and before command-limit validation.
The browser's **Learned motor corrections** panel shows the live outputs. The
same serialized network runs natively and in WASM, with no viewer-side inference
or physics. Earlier controllers remain available.

This is a first policy-learning experiment, not a replacement for the baseline
or a completed walking policy. The hand-authored gait still does almost all the
work. The task uses ideal teacher observations and provisional physics.

## Shared implementation

`sim-domain-control::neural` validates versioned dense tanh networks, named
features and outputs, units, normalization, clipping and finite values. The
controller binds the artifact against its existing sensor/actuator contract.
`PolicyConfig.neural_residual` stores the artifact and exposes it in runtime
metadata and recordings. The output is a bounded correction, not a direct state
change or bypass around the servo model. Zero output preserves the original
floating-point arithmetic and all 1,201 physical frames of the baseline case.

The initial network has 36 features, eight hidden units, twelve outputs and 404
trainable parameters. Features include reference-angle errors, motor velocities,
body gravity direction and velocity, commanded motion, and foot support forces.
Normalization scales are numerical choices, not measured physical parameters.
Outputs are bounded to ±0.001 rad; the selected policy reaches approximately
0.000098 rad (0.0056 degrees) in the short training run.

`sim-domain-control::policy_search` implements seeded paired random search over
episode rewards. Each iteration evaluates positive and negative parameter
perturbations and retains the best completed candidate. It is an elementary
policy-search method, not PPO or a reproduction of a published ES algorithm.
Evolution strategies provide a broader research precedent for optimizing
control policies through episode evaluations: [Salimans et al., 2017](https://arxiv.org/abs/1703.03864).

The Rust training example rebuilds the same environment for each candidate,
records the exact recipe and each trial's weights, score or error, and exports
the selected policy/configuration. Failed numerical solves and sampled task
terminations are rejected candidates, not invented successful transitions.
General policies should eventually learn recovery from valid failure transitions;
this initial search deliberately requires a complete commissioning episode.

## What the evidence says

Four paired iterations (nine evaluations including the baseline) produced a
training reward change from 23.78693223 to 23.78696734. One candidate failed the
numerical solve. The selected network also improved the untouched reverse-first
sequence's reward, from 23.78646245 to 23.78650769. A 10 ms timestep retains the
sign of that small held-out improvement. These tiny gains are not evidence of
a practically superior gait, robustness or hardware transfer.

The 24-second training and held-out cases pass nine supported swings each.
The initial one-minute run failed at 34.9 seconds with a 40-iteration solver
budget; an earlier candidate failed at the same time. The explicit browser
configuration permits 80 iterations, keeping equations, timestep and error
tolerances unchanged. It completes 28 supported swings with no sampled
inter-link overlap and 0.962 mm final body error. Native/WASM comparison and
same-host replay/reset pass over that complete minute.

The 10 ms minute also needs the larger iteration budget. Its maximum foot/body
differences from 20 ms are 0.479/0.283 mm, but its final body error is **1.013 mm**,
outside the existing **1 mm** acceptance gate. Preserve this failure. The profile
is experimental; nominal success does not establish timestep-independent task
acceptance. `neural-teacher-status.json` records these results, browser timing,
source/evidence hashes and remaining requirements.

The rendered nominal minute keeps pace with realtime on the documented Intel
Mac/Chrome host. Active p95 transition processing is **22.6 ms**, still above the
**20 ms** target; p95 rendering-scheduling interval is 16.67 ms. Visible response
latency is not yet measured. The archive includes ten presets, and all trial
policies—including the failed candidate—are versioned under `neural-teacher/trials`.

## Reproduce

Run commands from the repository root. The first two commands prepare and train
the exact initial experiment; materialization creates explicit evaluation and
60-second browser configurations. It does not claim those artifacts pass gates.

```sh
node examples/full-robot/prepare_neural_teacher.mjs
cargo run --locked --release -p sim-runtime --example train_residual_policy -- examples/full-robot/neural-teacher/experiment.json runs/full-robot/learning/neural-teacher/search
node examples/full-robot/materialize_neural_teacher.mjs
cargo test --locked -p sim-domain-control --test neural
cargo test --locked -p sim-runtime --test neural_policy --test environment
cargo build --locked --release -p sim-runtime --example run_environment --example evaluate_lift
target/release/examples/run_environment examples/full-robot/neural-teacher/scene.json examples/full-robot/neural-teacher/config.json examples/full-robot/neural-teacher/task.json examples/full-robot/browser-residual-policy/sustained.actions.json > runs/full-robot/learning/neural-teacher/sustained-80.native.json
node examples/full-robot/check_online_steps.mjs runs/full-robot/learning/neural-teacher/sustained-80.native.json runs/full-robot/learning/neural-teacher/sustained-80-acceptance
cargo build --locked --release -p sim-web --target wasm32-unknown-unknown
node web/build-viewer.mjs runs/interactive/neural-teacher/viewer --environment-only
node web/tests/environment.mjs runs/interactive/neural-teacher/viewer robot-neural-teacher runs/full-robot/learning/neural-teacher/sustained-80.native.json runs/interactive/neural-teacher/sustained-parity.json
node web/tests/viewer.mjs runs/interactive/neural-teacher/viewer runs/interactive/neural-teacher/viewer-report.json
node web/tests/live_performance.mjs runs/interactive/neural-teacher/viewer robot-neural-teacher runs/interactive/neural-teacher/live-performance.json sustained-forward
node web/serve-viewer.mjs runs/interactive/neural-teacher/viewer 4182
```

Use `short.config.json` and `train.actions.json`/`heldout.actions.json` for the
24-second cases. `initial.config.json` is the zero-network comparator;
`refined.config.json` and `initial.refined.config.json` use 10 ms. For the refined
minute, halve `config.json`'s `step_s` and double `steps` and `report_every` while
keeping the 80-iteration limit. Run timing without competing simulation jobs.
CI exercises the artifact, typed inference, bounds, learned held values,
sampled stepping, native/WASM parity and viewer replay. Existing unrelated CI
failures are not resolved by this work.

Next: improve policy quality across multiple command/condition cases, resolve
the refined sustained acceptance miss, define deployable student observations,
and implement distillation and bounded disturbance training. Maintain browser
execution and visible learned outputs at each stage.
