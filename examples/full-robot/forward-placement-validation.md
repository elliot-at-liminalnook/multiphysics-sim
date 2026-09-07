# A forward foot placement that fails the supported-lift requirement

The new reference moves the -Y foot 10 mm in export-world +X and leaves it
there, with a requested 5 mm lift. It retains the 16 mm body weight shift,
gain-0.5 sampled Rhai joint feedback, mechanics/contact parameters and the
100 ms support checkpoint. Only the authored foot path changes. This is one
placement attempt, not a complete walking stride.

## What happened

The geometric planner passes its 321 knot/midpoint checks, with a maximum
marker error of 0.01284 mm. Both physical timestep runs complete their 2 s
horizons and have no reported internal contacts in accepted steps. However,
**both fail the existing supported-lift requirement**. The requirement remains
unchanged: at least 1 mm clearance, at most 0.1 N swing-foot load, and at least
1 N on each support foot, simultaneously for 50 ms of consecutive samples.

| Measurement | 0.25 ms physics step | 0.125 ms physics step |
| --- | ---: | ---: |
| Longest qualifying supported swing | 40 ms | 40 ms |
| Peak physical foot clearance | 2.942 mm | 2.957 mm |
| Support-checkpoint wait | 16 ms | 18 ms |
| Final world XY placement error | 1.155 mm | 1.331 mm |
| Native wall time for 2 s | 112.1 s | 171.3 s |

During otherwise unloaded, clear swing samples, the +Y support foot repeatedly
falls below 1 N and frequently reaches zero. At 0.8 s the geometric static
support calculation predicts only 1.579 N on that foot. The physical body is
2.534 mm short of the intended +Y shift and tilted approximately 0.33 degrees
in the corresponding gravity-direction component. These are observations that
motivate checking weight-transfer tracking and active balance; they do not
uniquely identify the causal contribution of actuator lag, compliance and
dynamic loads.

The two timesteps differ by up to 0.580 mm in a sampled world-foot position,
with one reporting sample having different contact pairs. The support decision
differs by one 2 ms controller period. Refinement preserves the failed-lift
conclusion, but does not establish a converged contact or motor reference.
Timings are development measurements with concurrent work, not isolated
throughput benchmarks. Realtime performance remains unmet.

## Exact reference inspection after pauses

A 16 or 18 ms wait moves subsequent reference times off the original 5 ms
geometric inspection grid. The planner now accepts extra inspection times via
`plan_marker_motion_with_inspections` and the CLI's optional fourth JSON input.
It reconstructs the mechanism at those phases with the same closure solver and
prescribed base translation, retaining authored-limit, collision and support
checks. It does not add or modify motor command knots. The new synthetic slide
test verifies unchanged command trajectories and exact point reconstruction;
the robot's original and expanded trajectories are also exactly equal.

The expanded robot plan contains 961 unique inspected poses. This lets the
shared motion-tracking tool compare both executions at their exact recorded
plan times. Lift checks remain in **simulation time**, including physical dwell;
`evaluate_lift --simulation-time` makes that choice explicit for gated runs.
Without that option the CLI continues rejecting an ambiguous gated check. The
new output records the time basis and the motion-gate configuration.

## Reproduce

Prepare the preceding support-checkpoint experiment, then:

```sh
cargo test --locked -p sim-runtime --test planning --test lift --test motion_tracking --test tracking
cargo build --locked --release -p sim-runtime --example plan_marker_motion --example integrate_embedding --example compare_motion --example evaluate_lift --example compare_embedding
mkdir -p runs/full-robot/learning/forward-10mm
target/release/examples/plan_marker_motion runs/full-robot/learning/landing-checkpoint/scene.json examples/full-robot/foot-markers.json examples/full-robot/single-foot-marker-motion-forward-10mm.json > runs/full-robot/learning/forward-10mm/plan.json
node examples/full-robot/prepare_forward_step.mjs
target/release/examples/integrate_embedding runs/full-robot/learning/forward-10mm/scene.json runs/full-robot/learning/forward-10mm/config.json > runs/full-robot/learning/forward-10mm/execution.json
target/release/examples/integrate_embedding runs/full-robot/learning/forward-10mm/scene.json runs/full-robot/learning/forward-10mm/refined.config.json > runs/full-robot/learning/forward-10mm/refined.execution.json
target/release/examples/plan_marker_motion runs/full-robot/learning/forward-10mm/scene.json examples/full-robot/foot-markers.json examples/full-robot/single-foot-marker-motion-forward-10mm.json runs/full-robot/learning/forward-10mm/inspection-times.json > runs/full-robot/learning/forward-10mm/inspected-plan.json
```

For each `execution.json` / `refined.execution.json`, run `compare_motion` against
`inspected-plan.json` with `foot-markers.json` and reference link
`Robot | Chassis and hip mounts`. Run `evaluate_lift` with the same scene,
`single-foot-lift-requirements.json` and `--simulation-time`. Compare both full
captures with `compare_embedding` and the same marker file. Prepared inputs and
exact native/browser artifacts are hashed in `forward-placement-status.json`.

The browser preset `robot-forward-10mm` clearly labels the failed lift criterion
and retains the same physical motion for inspection. Earlier presets remain
available. No limit, contact law or acceptance threshold was relaxed to make
this attempt pass. The next experiment should address weight transfer—first
separating motion-rate effects from persistent tracking offsets, then closing
the body/foot feedback loop with the shared observations as needed.

The full native/WASM comparison passes at all 201 reporting frames (largest
entry difference 1.12e-8 N in a sampled foot-force observation, below the 1e-7
portability tolerance). Browser replay and reset are exact excluding wall time.
Live browser stepping costs 131.4 s for 2 simulated seconds; the slowest 10 ms
worker request costs 3.10 s. The rendering/main thread stays available, but
pause can still wait for an outstanding worker chunk. This is not realtime
simulation. The sampled supported-lift failure is a task result, separate from
successful runtime completion and successful native/browser agreement.

All thirteen viewer checks pass, including the explicit failed-result state,
camera/selection, earlier controller presets, pause/reset/replay, timeout
recovery and narrow layout. The tested eleven-preset bundle is
`runs/interactive/robot-lab-forward-placement-2026-09-07.zip`; the same files are
installed in the local viewer. The native runs used the preserved
`runs/interactive/landing-checkpoint/native-runner` and its separately captured
source baseline. Current measurement/planner/viewer sources are snapshotted
alongside this experiment, with hashes in its status artifact.
