# Bounded body-position feedback

The previous controller corrected individual joint-angle errors, but the body
still undershot its planned weight transfer. This experiment adds a shared Rust
`BodyFeedback` helper and a Rhai policy that explicitly consumes its suggestions.
It retains the corrected chassis collision grid, the slower 10 mm foot reference,
joint gain 0.5, 2 ms policy clock, servo firmware, electrical/mechanical equations,
software bounds, and support checkpoint.

## How it works

The body reference is the geometric planner's absolute world COM trajectory.
At each policy sample, the helper compares that reference with the current body
COM. For supported feet, it asks the shared mechanism Jacobian for joint changes
that move those feet oppositely relative to a fixed body. If the floor holds the
feet, the physical reaction can move the body toward its target. The solver
includes the existing linkage closure tangent and exact point kinematics.

A weighted, damped least-squares solve produces angular suggestions. Weights
come from ideal world-Z floor loads, excluding internal contacts, with full
weight at 5 N. No supported feet means zero suggestion. Damping is explicitly
0.002 m/rad; a common scale bounds every suggestion to 0.04 rad while preserving
its direction. Rhai applies body gain **0.25**, so this term alone adds at most
0.01 rad (0.573 degrees) to a target. The existing software/CAD target-bound
checks remain active; they reject an out-of-bounds total command.

This is a motor-target correction, not an applied force or a pose edit. The
actual motors, backlash, linkages, inertia and contacts execute it. Reference
time follows the existing support clock; paused references have zero desired
velocity. Velocity damping is supported but set to zero in this experiment.
Orientation feedback and direct swing-foot position feedback are not included.
The helper requires the sole floating root as its reference and angular command
coordinates; it is reusable for other compatible articulated systems.

## Results

All four complete runs below use the same corrected robot and world. Requirements
remain 1 mm foot clearance, at most 0.1 N swing-foot force, and at least 1 N on
all three support feet simultaneously for 50 ms of consecutive reporting samples.

| Measurement | Joint only, 0.25 ms | Body feedback, 0.25 ms | Joint only, 0.125 ms | Body feedback, 0.125 ms |
| --- | ---: | ---: | ---: | ---: |
| Body +Y undershoot at planned peak | 1.818 mm | 1.549 mm | 1.745 mm | 1.555 mm |
| Body height undershoot at peak | 1.315 mm | 1.121 mm | 1.315 mm | 1.123 mm |
| Final foot world XY error | 1.326 mm | 0.782 mm | 1.262 mm | 0.673 mm |
| Supported lift duration | 270 ms | 170 ms | 180 ms | 170 ms |
| Peak foot clearance | 2.667 mm | 2.886 mm | 2.707 mm | 2.908 mm |
| Accepted internal contacts | 0 | 0 | 0 | 0 |

Both feedback runs pass the sampled lift criterion. Body and endpoint errors
improve, while qualifying lift duration decreases. **The largest timestep
foot-path difference increases from 1.729 to 2.064 mm.** Final world-X error also
changes sign between feedback timesteps (-0.432 / +0.455 mm). These results do
not establish converged contact dynamics, successful landing over a specified
foothold margin, stable walking, or sim-to-real accuracy. Current observations
are privileged simulated body pose and loads; no hardware sensing is invented.

Native feedback runs cost 158.3 / 245.1 wall seconds for 2.8 simulated seconds.
These are concurrent development measurements, not an isolated performance
comparison. The feedback path adds kinematic/contact inspection work; no speedup
is claimed. Realtime and learning throughput remain unmet.

## Browser and regression checks

All 281 browser frames pass the 1e-7 absolute native/WASM entry tolerance, with
largest difference 3.22e-8 N in a floor-force observation. Browser replay and
reset are exact excluding wall time. The browser takes 186.9 wall seconds for
2.8 simulated seconds; its longest worker request is 2.42 seconds. The main
thread remains responsive, but pause can wait for an outstanding physics chunk.

The `Quadruped · body position feedback` preset offers joint and body gains.
The body/foot inspector shows world target versus actual body position, position
error, support weights and the largest suggestion before policy gain. All 16 UI
checks pass, including new gain restoration and diagnostic replay, earlier
controllers, selection/fit, failure recovery and narrow layout. Earlier failed
and uncorrected experiments remain labeled and selectable.

Three focused Rust tests cover analytic corrections, rotation invariance,
bounds, unloaded feet, reference-phase/paused-velocity behavior, and a floating
fixture whose ideal body error reaches Rhai without inventing support. The
floating fixture replays its poses and policy state exactly. Seven existing
session tests, two task-observation tests and five motion-tracking tests also
pass. The new feedback suite is included in browser CI. These tests establish
software behavior, not robot calibration.

## Reproduce

Prepare and validate the corrected slower-placement model as documented in
`forward-rate-validation.md`, then:

```sh
cargo test --locked -p sim-runtime --test body_feedback --test embedded_session --test task_observation --test motion_tracking
cargo build --locked --release -p sim-runtime --example integrate_embedding --example evaluate_lift --example compare_motion --example compare_embedding
node examples/full-robot/prepare_body_feedback.mjs
target/release/examples/integrate_embedding runs/full-robot/learning/body-feedback/scene.json runs/full-robot/learning/body-feedback/config.json > runs/full-robot/learning/body-feedback/execution.json
target/release/examples/integrate_embedding runs/full-robot/learning/body-feedback/scene.json runs/full-robot/learning/body-feedback/refined.config.json > runs/full-robot/learning/body-feedback/refined.execution.json
```

Apply `evaluate_lift` to each capture with the same scene, the slower experiment's
`lift-requirements.json`, and `--simulation-time`. Apply `compare_motion` against
`forward-slow/chassis-solid-sign.plan.json` with `foot-markers.json` and reference
link `Robot | Chassis and hip mounts`. Compare the two feedback timesteps with
`compare_embedding`; do not strip policy metadata to compare different controllers.

Build and package WASM as in `web/README.md`. Run `web/tests/embedded.mjs` for
`robot-body-feedback` against its native capture, plus `web/tests/viewer.mjs`.
The frozen feedback runner and source snapshot identify the physical execution
used here. Prepared configurations preserve the controller inputs and derivation
hashes. The original CAD is unchanged.
