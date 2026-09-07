# Motor-driven knee retraction and return

The registered motors, drivers and 1 kHz servo firmware now accept a time-varying
reference through `EmbeddedServoBank::connect_target_law`. The law is pure and
sampled by the original firmware clocks; no controller update occurs inside a
Newton residual. Voltage and temperature boundaries remain explicitly imposed.
No motor, material, joint, friction or firmware parameter was retuned for this
experiment.

The shared `sim_domain_control::trajectory` sampler provides validated linear
and quintic rest-to-rest keyframes, values and time derivatives. The existing
detailed-runtime linear trajectory path uses the same implementation. The
reduced diagnostic requires exact motor names/order, the matching CAD hash and
initial targets, and rejects independent target values outside authored limits
before stepping. Bounds on commanded values do not bound tracking overshoot or
dependent-joint motion. Missing CAD limits are not invented.

## Motion and geometric preflight

All four foot crank targets move from 0 to -0.2 rad (-11.46 degrees), then back.
The first 0.2 s is a hold, followed by a 0.2 s smooth retraction, 0.2 s hold,
0.2 s return and 0.2 s final hold. Other initial motor targets remain fixed.
The ideal motion retracts the feet approximately 2.6 mm relative to the body.

A 201-pose audit preserves original closure, checks direct derivatives against
independent velocity probes, retains positive reduced inertia and checks authored
limits. No internal contact is reported on this sampled path; its minimum scaled
singular value is about 0.922. That number depends on the declared coordinate
scales. The audit is not a proof of continuous clearance or a complete operating
envelope.

Two exploratory candidates identified boundaries worth keeping visible:

- +0.1 rad exceeds the +X foot motor's authored +4.5-degree upper limit. The
  audit rejects it; it was not run dynamically. The final integration CLI also
  rejects its keyframe before any physics step.
- -0.4 rad gives about 10.3 mm ideal retraction, but the imported collision model
  reports -Y crosshead/sector-gear contact beginning near -0.273 rad. Maximum
  reported penetration is 0.677 mm. A real interference versus conservative
  collision geometry remains unresolved. This is not a certified hardware stop.

Only the +X foot motor currently has authored angular limits. The other three
foot-motor limits remain missing. The chosen test path does not fill that gap.

## Loaded results and what they mean

All three runs complete, retain 1,000 ticks per servo and report no internal
contact in accepted steps. Body lowering reaches about 3.1 mm and sampled tilt
stays below 0.030 degrees. The applied command is a small retraction/return under
load, not a walking or recovery policy.

| Nominal physics step | Wall time for 1 simulated second | Maximum foot tracking error relative to the chassis |
| --- | ---: | ---: |
| 0.125 ms | 58.88 s | 1.805 mm |
| 0.25 ms | 41.76 s | 1.766 mm |
| 1 ms | 20.45 s | 1.707 mm |

Tracking is measured against the prescribed geometric reference, in the moving
chassis frame. It includes actuator lag, compliance and loaded offsets. It is
different from world-space foothold error: the body can lower while supported
feet stay near the floor. Reports sample every 10 ms and establish no continuous
tracking or physical acceptance gate. Timings are single native diagnostic runs,
possibly overlapping small builds; they are not controlled speedup measurements.

The tracking error is substantial relative to the 2.6 mm motion. In the finest
run at 0.69 s, the -Y foot target is -0.1186 rad while the actual joint remains
at -0.1833 rad during return. Its worm coordinate also has a load-dependent
offset despite a constant target. Maximum sampled duty magnitude is about 0.526
and current 0.432 A; these samples do not certify absence of brief saturation.
Changing the numerical timestep does not remove this control/plant response.

Relative to 0.125 ms, maximum sampled world-foot differences are 0.228 mm at
0.25 ms and 0.497 mm at 1 ms. Duty differences reach 0.091 and 0.157 respectively,
including the initial settling transient. Neither timestep is promoted on marker
agreement alone. Hardware accuracy, loaded tracking margins and walking
feasibility remain unvalidated.

Next separate dynamic lag from loaded offsets with controlled reference tests,
resolve the reported collision geometry before enlarging the envelope, and
account for measured actuator response in planner timing and controller actions.
The full planner/teacher/student/robust-learning/viewable-policy goal remains
unfinished.

## Initial tracking-lag follow-up (historical failed diagnostics)

Both failures below have since been addressed through shared firmware timing and
an opt-in auxiliary-rate formulation. Completed replays and remaining numerical
differences are documented in `servo-timing-and-rate-validation.md` and
`servo-timing-and-rate-status.json`. The original failed captures are retained.

At 45% of the return stroke, the -Y crank target is -6.7967 degrees.
The original 0.25/0.125 ms runs give -10.5002/-10.5033 degrees actual angle.
This particular gap is insensitive to that refinement; it does not establish
hardware accuracy or rule out other numerical errors.

Two additional diagnostics used the final source-snapshot executable:

- `mechanical-servo-knee-motion-slow.json` doubles each transition to 0.4 s,
  retaining the 0.2 s holds, amplitude, actuator parameters and 0.25 ms step.
  At the corresponding return phase (0.98 s), actual angle is -9.6886 degrees,
  reducing the gap from 3.7035 to 2.8919 degrees. However, the requested 1.4 s
  run fails after 1.35475 s, during the final hold. The failure involves a
  roughly 6e-16 s hybrid remainder and Newton nonconvergence. Its cause has
  not been diagnosed. This partial comparison is not a completed validation.
- A separate scene sets `robot.gravity` to `[0,0,0]` and `options.contact` to
  `false`, preserving inertia, internal losses and motor/firmware parameters.
  It fails after 0.24025 s during hybrid guard location, before return begins.
  It therefore cannot establish the effect of removing external loading on
  the observed return-stroke lag.

The shared motion comparator correctly rejects the incomplete slow capture.
The slower prescribed geometric sweep completes, which does not establish
dynamic feasibility. Neither diagnostic is promoted. Raw errors, scene overrides,
source reference and hashes are recorded in `knee-motion-lag-diagnosis-status.json`.
Resolve these numerical failures before drawing a full speed-versus-load
conclusion. No CAD or actuator calibration values were changed.

## Reproduction

Use the scene derivation and source CAD recorded in `embedded-integration.md`.

```sh
cargo run --locked --release -p sim-runtime --example audit_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json 201 examples/full-robot/knee-motion-sweep.json > runs/full-robot/learning/knee-motion-sweep.json
cargo run --locked --release -p sim-runtime --example integrate_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/mechanical-servo-knee-motion-coarse.json > runs/full-robot/learning/knee-motion-coarse.json
cargo run --locked --release -p sim-runtime --example compare_motion -- runs/full-robot/learning/knee-motion-coarse.json runs/full-robot/learning/knee-motion-sweep.json examples/full-robot/foot-markers.json 'Robot | Chassis and hip mounts' > runs/full-robot/learning/knee-motion-coarse-tracking.json
```

Repeat with refined/1ms recipes and use `compare_embedding` for matched simulation
samples, events and accepted contact impulses. `summarize_hold` can report body
movement from the same capture without interpreting that movement as a hold pass.
The relative-marker transform and pose validation are shared Rust functions,
tested under known translated/rotated frames. CI covers the sampler, original
sampled motor/circuit reference, rollback, runtime/replay and marker validation;
WASM compilation passes. `knee-motion-status.json` records input/capture hashes,
validation and exact integration plus final reporting/input-guard source snapshots.
