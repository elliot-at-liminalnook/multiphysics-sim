# Teacher motor-action interface

The `robot-residual-policy` browser preset adds twelve angle corrections to the
accepted slow crawl. It is an interface for learning and manual investigation;
there is no trained neural policy yet. WASD still requests motion through the
baseline planner and controller. Expand **Motor corrections** to adjust individual
motor targets, and use **Clear motor corrections** to restore zero corrections
at the next control sample. Record/replay includes these actions.

## What the learner can change

`browser-residual-policy/learning.json` declares twelve named action bindings.
Each correction is added after the baseline tracking/body/foot feedback. The
declared range is ±0.01 rad; the commissioning probe exercises only ±0.001 rad
for 40 ms per motor. The full action range and arbitrary combinations are not
validated walking conditions. Existing motor-command and geometry guards still
apply; an accepted input range is not a certification of physical feasibility.

The full environment has eighteen input channels: twelve motor corrections,
three externally supplied motion commands, and three fixed feedback gains.
The learner must select only the twelve declared policy bindings. It must not
choose its own commanded task or alter the fixed gains to improve its reward.
The lateral command remains fixed at zero in this commissioned crawl.

No robot physical property, numerical tolerance, or actuator limit is changed.
The scene retains the terrain-contact profile's explicit omission of inter-link
impact forces, with sampled geometry guards. The detailed model remains separate.
Generate these versioned recipes with:

```sh
node examples/full-robot/prepare_residual_policy.mjs
```

## What the teacher observes and scores

The shared Rust environment now exposes typed body-frame linear velocity,
world angular velocity, orientation components, held controller inputs, motor
shaft torque, and per-link terrain-contact force. Forces and motion are read
from the current physics endpoint, rather than stale policy telemetry.

The robot task has 90 observations. Its `body.up` vector is world vertical
expressed in the body's coordinates: `Rᵀ [0, 0, 1]`. These are ideal simulation
observations, including privileged contact information; they are not a claim
that the real robot has equivalent sensors. Student observations still require
a separate deployable sensor definition, with delay and noise.

The provisional reward combines reference tracking, uprightness, small motor
corrections and low effort. A survival rate of 1 per simulated second avoids
scoring every nominal continuing step negatively. Sampled task-bound failure
subtracts 10 once. Reset and time-limit truncation do not receive that penalty.
Numerical failures remain errors, not valid scored transitions to feed into
learning. This is an initial reference-following objective, not a validated
terrain or disturbance-recovery reward. Survival reward alone does not establish
that all reward exploitation has been prevented.

## Evidence and reproduction

`residual-policy-status.json` records source and evidence hashes, physical
acceptance summaries, native/WASM parity, UI checks and rendered timing. Neutral
corrections reproduce all 3,001 original physical frames over 60 seconds exactly.
The separate 24-second probe reaches every motor and passes nine supported
swings. The neutral minute passes 28. These are sampled flat-floor checks;
they do not establish hardware transfer, between-sample clearance or general
terrain walking.

The rendered minute on the documented Intel Mac/Chrome host kept pace with
realtime (1.0006× during active walking), with 22.1 ms active p95 transition
latency. It still misses the 20 ms target. Rendering's p95 scheduling interval
was 16.67 ms; command-to-visible-response latency remains unmeasured. The actual
recorded keyboard inputs match the accepted native sustained schedule exactly.

```sh
cargo test --locked -p sim-runtime --test environment --test residual_policy
cargo build --locked --release -p sim-runtime --example run_environment --example evaluate_lift
cargo build --locked --release -p sim-web --target wasm32-unknown-unknown
node web/build-viewer.mjs runs/interactive/residual-policy/viewer --environment-only
mkdir -p runs/full-robot/learning/residual-policy
target/release/examples/run_environment examples/full-robot/browser-terrain-contact/scene.json examples/full-robot/browser-reversal/config.json examples/full-robot/browser-reversal/task.json examples/full-robot/browser-reversal/sustained.actions.json > runs/full-robot/learning/residual-policy/baseline.native.json
target/release/examples/run_environment examples/full-robot/browser-residual-policy/scene.json examples/full-robot/browser-residual-policy/config.json examples/full-robot/browser-residual-policy/task.json examples/full-robot/browser-residual-policy/sustained.actions.json > runs/full-robot/learning/residual-policy/sustained.native.json
target/release/examples/run_environment examples/full-robot/browser-residual-policy/scene.json examples/full-robot/browser-residual-policy/short.config.json examples/full-robot/browser-residual-policy/task.json examples/full-robot/browser-residual-policy/probe.actions.json > runs/full-robot/learning/residual-policy/probe.native.json
node examples/full-robot/check_residual_policy.mjs runs/full-robot/learning/residual-policy/baseline.native.json runs/full-robot/learning/residual-policy/sustained.native.json runs/full-robot/learning/residual-policy/probe.native.json runs/full-robot/learning/residual-policy/interface-report.json
node examples/full-robot/check_online_steps.mjs runs/full-robot/learning/residual-policy/sustained.native.json runs/full-robot/learning/residual-policy/sustained-acceptance
node examples/full-robot/check_online_steps.mjs runs/full-robot/learning/residual-policy/probe.native.json runs/full-robot/learning/residual-policy/probe-acceptance
node web/tests/environment.mjs runs/interactive/residual-policy/viewer robot-residual-policy runs/full-robot/learning/residual-policy/probe.native.json runs/interactive/residual-policy/probe-parity.json examples/full-robot/browser-residual-policy/short.config.json
node web/tests/viewer.mjs runs/interactive/residual-policy/viewer runs/interactive/residual-policy/viewer-report.json
node web/tests/live_performance.mjs runs/interactive/residual-policy/viewer robot-residual-policy runs/interactive/residual-policy/live-performance.json sustained-forward
node examples/full-robot/summarize_residual_policy.mjs
```

Run timing checks without concurrent build or simulation jobs. The parity check
records the explicit 24-second configuration override; the packaged preset has
a 60-second horizon. CI repeats the neutral/probe comparisons over 24 seconds,
physical acceptance, twelve-target routing, bounds, replay and browser checks.
These additions do not imply that unrelated existing CI failures are resolved.

The next step is a reusable Rust policy/learning implementation consuming these
bindings, with held-out evaluation and browser inference using the same policy
artifact. Teacher learning, distillation, bounded disturbance training, useful
walking speed and hardware calibration remain open requirements.
