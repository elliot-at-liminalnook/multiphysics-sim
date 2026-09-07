# Sampled joint feedback experiment

This experiment adds a captured Rhai policy to the same `EmbeddedSession` used
by the native motor runner and WASM viewer. It uses ideal joint angles, not a
new hardware sensor model. The reusable script is
`examples/interactive/controllers/joint-reference-feedback.rhai`; robot-specific
configuration is in `joint-feedback-experiment.json`.

For every independent motor joint, the requested motor angle is:

`reference + gain * (reference - actual angle)`

Gain 0 is the sampled-reference baseline. Gain 0.5 adds half the current error
to the requested angle. Registered servo firmware and winding/driver/gearbox
physics still determine the resulting motion. The script does not apply forces,
move geometry directly, or bypass motor limits. This outer correction can interact
with the servo's internal feedback, so improvement is an experimental question.

## Native results at 0.25 ms nominal physics steps

Both variants complete 1.6 simulated seconds with 161 reporting frames. Worst
sampled foot-marker error relative to the body falls from 4.770 to 3.142 mm.
For the moving −Y foot, RMS error falls from 2.160 to 1.444 mm. These measurements
compare actual mechanism motion with the same geometric reference; they are not
world foothold errors or hardware accuracy estimates.

The existing sampled lift diagnostic passes in both variants. Peak floor
clearance rises from 1.680 to 2.685 mm. The consecutive reporting span with at
least 1 mm clearance, no more than 0.1 N swing-foot load, and at least 1 N on each
support grows from 130 to 170 ms. This check does not accept landing, balance,
slip, between-sample collisions or walking.

The old continuously evaluated reference path had 4.632 mm maximum body-relative
foot error. The zero-gain sampled path has 4.770 mm: controller/firmware staging
is a separate change from feedback gain. We compare gain 0.5 against gain 0,
not attribute their combined differences entirely to feedback.

The two initial native runs take about 98 wall seconds each on this machine,
while other work was running. These are observed execution costs, not isolated
performance benchmarks. The controller improvement is not a realtime speedup.

## Refinement and browser evidence

At 0.125 ms, gain 0.5 again passes the same 170 ms sampled supported-lift span.
Peak clearance is 2.686 mm versus 2.685 mm, and maximum body-relative tracking
error is 3.176 mm versus 3.142 mm. However, sampled world marker positions differ
by up to 0.344 mm; the contact-pair list agrees at reporting samples, but some
backlash guard counts and occurrence times differ. Sampled current differs by
up to 0.085 A and shaft torque by 0.1255 N m. Stable task results do not establish
converged detailed forces or event history. The refined run takes 139.34 wall
seconds under concurrent work.

The full gain-0.5 browser run compares all 161 native frames. Maximum absolute
entry difference is 3.208e-9 rad/s in an observed worm-joint velocity, below the
1e-7 portability diagnostic threshold. Browser physical replay and reset are
exact, excluding wall-time instrumentation. Invalid work requests and changed
replay recipes preserve state. The run takes 106.85 wall seconds with a maximum
10 ms-simulation work chunk of 2.323 wall seconds; the main thread remains live.
This is neither realtime nor suitable training throughput yet.

Rendered UI checks pass for the full robot's live gain input, camera interaction,
plan/target/actual readings, and input-event replay with restored slider values.
The small sampled-Rhai fixture also passes 41-frame native/WASM comparison,
input recording/replay and desktop/mobile UI checks. Six native session tests
and three comparison-tool tests pass. CI includes the small fixture's policy
portability and UI gates; full robot runs were checked locally.

Exact artifact and source hashes are recorded in `joint-feedback-status.json`.

## Reproduce

First reproduce the corrected CAD export and geometric reference as described
in `lift-and-viewer-validation.md`. The physical source is the same revision
1357 CAD baseline; no mass, geometry, contact or servo constant is changed here.

```sh
node examples/full-robot/prepare_joint_feedback.mjs
cargo build --locked --release -p sim-runtime --example integrate_embedding \
  --example compare_motion --example compare_embedding --example evaluate_lift
```

The preparation tool captures exact script contents in each scene, copies the
explicit planner search envelope into the software command bounds, and hashes
its inputs and outputs in `runs/full-robot/learning/joint-feedback/manifest.json`.
The robot and scene physics options are unchanged. The envelope is provisional;
its bounds are not measured physical stops or a collision-free certificate.

For `GAIN=0` and `GAIN=0.5`, run:

```sh
target/release/examples/integrate_embedding \
  runs/full-robot/learning/joint-feedback/gain-${GAIN}.scene.json \
  runs/full-robot/learning/joint-feedback/gain-${GAIN}.config.json \
  > runs/full-robot/learning/joint-feedback/gain-${GAIN}.execution.json

target/release/examples/compare_motion \
  runs/full-robot/learning/joint-feedback/gain-${GAIN}.execution.json \
  runs/full-robot/learning/hip-grid-plan-16mm-lift-5mm.json \
  examples/full-robot/foot-markers.json 'Robot | Chassis and hip mounts'

target/release/examples/evaluate_lift \
  runs/full-robot/learning/joint-feedback/gain-${GAIN}.scene.json \
  runs/full-robot/learning/joint-feedback/gain-${GAIN}.execution.json \
  examples/full-robot/single-foot-lift-requirements.json
```

`gain-0.5.refined.config.json` halves the physics timestep and doubles the step
and reporting counts while preserving policy sampling and physical duration.
Use it with the same scene, then compare complete captures through
`compare_embedding` and repeat the lift check. The comparison rejects changed
policy metadata; it does not treat a changed controller as solver error.

Build the WASM workspace following `web/README.md`. The catalog declares live
zero-gain and gain-0.5 robot presets, plus a small sampled-Rhai fixture for CI.
Run `web/tests/embedded.mjs` with `robot-feedback-0.5` and its native capture for
all-frame portability and replay checks; `web/tests/viewer.mjs` exercises the
real UI, including gain input, camera, plan/target/actual readings, recording,
and restoring command values after replay.

## Limits and next work

This policy has ideal-state observations and an explicit nondeployable contract.
It neither estimates body state nor regulates balance or landing. Hardware sensor
mapping, delay/noise, calibration, robust disturbance handling, teacher/student
learning and WASD locomotion remain required. Higher gain is not automatically
better; it can amplify noise, exceed the command envelope or destabilize the
interaction with servo firmware. Task comparisons must retain the physical
load/contact behavior and include refinement and complete trajectories.

The feedback presets are delivered live. Packaging an additional recorded view
correctly rejected a raw/canonical world mismatch: the input export includes
`floor_friction_static`, which the Rust `World` does not retain, while the
canonical report includes its default `ambient_c`. The recorded-view provenance
check was kept strict. This experiment uses the existing regularized kinetic
friction model and imposed winding/supply boundaries; it does not establish
static-friction behavior. A future recorded view needs an explicit canonical
scene artifact rather than silently weakening that check.
