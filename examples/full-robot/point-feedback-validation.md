# Swing-foot world-position feedback

This teacher-controller experiment adds direct foot-position feedback to the
existing joint and body corrections. The body-feedback experiment improved final
placement but retained about 4.16 mm maximum sampled world foot-path error. The
new question is whether correcting the swinging foot itself reduces that error
without losing supported lift or worsening timestep sensitivity.

## Mechanism and scope

`PointFeedback` is a shared Rust helper, not robot-specific dynamics. Configuration
names CAD markers, the CAD hash, world frame, absolute XYZ reference trajectories,
a dimensionless activation trajectory, damping in m/rad and an angular cap.
The preparation script takes marker positions and displacements from the existing
geometric plan. It does not infer geometry or replace the commanded motor path.

At the policy clock, the helper measures current world marker positions and asks
the existing closed-mechanism point Jacobian for a local angular correction. Each
point's desired displacement is its position error times activation. Activation
also weights that point in a damped least-squares solve; zero activation removes
its objective. This combination fades isolated-point corrections as well as
reducing their influence when points share joints. A common scale caps the largest
suggestion without changing the joint-correction direction.

This experiment controls only the -Y foot marker. Activation rises from zero to
one during reference time 0.75–0.9 s, remains one through 1.5 s, then falls to zero
at 1.65 s. It follows reference time, including support-clock pauses. These are
provisional policy phase choices, not inferred contact events. The helper itself
does not enforce contact safety. World positions are privileged simulated state;
CAD does not yet provide a deployable sensing implementation.

Damping is 0.002 m/rad and the suggestion cap is 0.04 rad. Rhai applies gain 0.25,
so this term alone adds at most 0.01 rad (0.573 degrees). Joint gain 0.5 and body
gain 0.25 remain unchanged. The combined command must still satisfy the existing
software/CAD bounds. No poses, contact forces, servo equations, robot properties,
world conditions or motor reference knots are edited.

## Validation

Three focused point-feedback tests cover conflicting multi-point objectives,
inactive points, known local kinematics, gradual activation, caps, malformed
configuration, and actual Rhai command execution with exact session replay. The
existing body feedback, observations, session and motion-tracking suites also pass
(20 tests total). These establish software behavior, not robot task accuracy.

Both complete 2.8 s robot runs pass the same sampled supported-lift test:
1 mm clearance, at most 0.1 N on the swing foot, at least 1 N on each supporting
foot, simultaneously for at least 50 ms of consecutive reporting samples.

| Measurement | Body only, 0.25 ms | Point feedback, 0.25 ms | Body only, 0.125 ms | Point feedback, 0.125 ms |
| --- | ---: | ---: | ---: | ---: |
| Maximum world foot-path error | 4.156 mm | 3.764 mm | 4.142 mm | 3.754 mm |
| RMS world foot-path error | 1.605 mm | 1.579 mm | 1.557 mm | 1.717 mm |
| Final foot horizontal error | 0.782 mm | 1.027 mm | 0.673 mm | 1.250 mm |
| Supported lift duration | 170 ms | 200 ms | 170 ms | 210 ms |
| Peak clearance | 2.886 mm | 3.211 mm | 2.908 mm | 3.203 mm |
| Body +Y undershoot at peak | 1.549 mm | 1.537 mm | 1.555 mm | 1.528 mm |
| Accepted internal contacts | 0 | 0 | 0 | 0 |

The controller improves peak swing-path error and clearance, but worsens final
placement at both timesteps and RMS path error at the finer timestep. The largest foot-path difference under timestep
refinement also grows from 2.064 to **2.177 mm**. It is retained as a labeled
experiment, not promoted as a uniformly better controller or accepted walking.
The inactive prefix through 0.75 s matches the previous foot trajectory; physical
robot/world parameters, numerical settings, clocks and motor reference remain
unchanged. A 10 mm intended step is not a declaration of allowable tracking error.

The point suggestion reaches its 0.04 rad cap in 41 of 90 active reporting
samples at both timesteps. Increasing that cap has not been validated; the
current results already show a swing-versus-landing tradeoff. Body orientation
remains uncontrolled, and the foot correction deliberately fades out before the
support-qualified return. These are concrete limitations of the controller.

Native stepping costs 200.9 / 270.7 wall seconds for 2.8 simulated seconds in
concurrent development runs. This is an accuracy/control experiment; no runtime
speedup, timestep convergence, deployed sensing or sim-to-real result is claimed.

All 17 browser UI checks pass, including gain restoration, phase activation,
world foot targets/actual/errors and deterministic visible replay. All 281 browser frames pass the 1e-7 absolute native/WASM entry tolerance;
the largest difference is 3.22e-8 N in a floor-force observation. Replay and
reset are exact excluding measured wall time. The browser computes 2.8 simulated
seconds in 182.7 wall seconds. Its largest 10 ms physics request takes 2.29 s;
the main thread remains responsive, but outstanding worker work can delay pause.

The shared bundle retains all 15 presets. Earlier controllers remain available
and the new preset explicitly states the swing/landing tradeoff. The shareable
archive is `runs/interactive/robot-lab-point-feedback-2026-09-07.zip`; serve it over
HTTP/HTTPS as described in its OPEN.txt. It is not a public deployment.

## Reproduction

First reproduce the inputs in `body-feedback-validation.md`, then:

```sh
cargo test --locked -p sim-runtime --test point_feedback --test body_feedback --test embedded_session --test task_observation --test motion_tracking
cargo build --locked --release -p sim-runtime --example integrate_embedding --example evaluate_lift --example compare_motion --example compare_embedding
node examples/full-robot/prepare_point_feedback.mjs
target/release/examples/integrate_embedding runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/point-feedback/config.json > runs/full-robot/learning/point-feedback/execution.json
target/release/examples/integrate_embedding runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/point-feedback/refined.config.json > runs/full-robot/learning/point-feedback/refined.execution.json
```

For each capture run `evaluate_lift` with the prepared scene,
`forward-slow/lift-requirements.json`, and `--simulation-time`; run `compare_motion`
against `forward-slow/chassis-solid-sign.plan.json`, `foot-markers.json`, and
`Robot | Chassis and hip mounts`. Run `compare_embedding` on the two point-feedback
captures. Compare controller variants through their separate tracking reports;
do not strip policy/source metadata to pass a timestep-comparison check.

Build WASM and package with `web/build-viewer.mjs` as documented in `web/README.md`.
Run `web/tests/embedded.mjs` for `robot-point-feedback` against its native capture,
and `web/tests/viewer.mjs` for the packaged browser. Retain the preparation manifest,
source identities and original measurements with the shareable build.

## Updated complete-run profile

The same frozen native runner with `profile_solver: true` produces exactly the
same 281 sampled physical/controller frames. Its 173.6 s stepping wall time is a
separate concurrent development measurement, not a speedup over the unprofiled
200.9 s run. Profile overhead and competing processes prevent that comparison.

| Profile bucket | Seconds | Calls |
| --- | ---: | ---: |
| Jacobian assembly | 103.52 | 9,948 |
| Ordinary residual evaluations | 63.32 | 118,981 |
| Embedded closure mapping | 85.39 | 321,990 |
| Embedded contact history | 36.16 | 307,990 |
| Embedded dynamics preparation | 38.64 | 319,190 |
| Embedded applied-force solve | 2.49 | 798,502 |
| Embedded component equations | 4.94 | 787,302 |
| Newton matrix factorization | 0.64 | 9,948 |

Buckets overlap: closure/contact/dynamics work occurs inside residual and
Jacobian work. Closure Jacobian/factorization diagnostic sub-buckets are nested
within mapping; their 854,149 SVD calls cost 18.03 s, already included above.
These figures do not justify replacing the inexpensive final linear solver.

The run requested 11,200 nominal steps, accepted 11,280 segments, and made 11,753
continuous attempts. Successful trials alone consumed 785,939 endpoint evaluations
and 106,757 Newton iterations; 5,520 successful trials reused a Jacobian. Those
successful-only counts omit rejected solver internals. The minimum accepted
segment was 0.734 microseconds; an event-boundary segment is not automatically a
convergence failure. The present throughput problem is chiefly repeated work
inside solves, rather than pervasive timestep subdivision.

Next performance investigation: dependency-safe reuse or explicit reduction of
closure mapping and its derivative work, including whether rigid-base changes
or repeated joint configurations trigger unnecessary recomputation. Any fast-model
reduction must retain the original equations as validation diagnostics and pass
full-trajectory task checks. Do not assume this alone will deliver realtime.

```sh
# Copy config.json to profile.config.json and change only profile_solver to true.
runs/interactive/point-feedback/native-runner runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/point-feedback/profile.config.json > runs/full-robot/learning/point-feedback/profile.execution.json
node examples/interactive/summarize_embedded_profile.mjs runs/full-robot/learning/point-feedback/profile.execution.json runs/full-robot/learning/point-feedback/execution.json runs/full-robot/learning/point-feedback/profile.json
```
