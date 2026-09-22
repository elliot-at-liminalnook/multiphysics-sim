# Contact timing, foot-path, and stance co-design

Completed milestone: substantially faster sustained walking with the detailed
CAD motor dynamics active during every performance evaluation, while retaining
tracking and balance. `upright-fast` passes the recorded flat-ground simulation
suite. Hardware is disconnected; browser and hardware deployment are separate.

## Execution path

The existing shared Rust `ContactPhaseMotion` and `ContactPlanner` generate new
body/foot paths and solve the CAD mechanism. `compile_contact_reference` creates
ordinary periodic joint references, with recorded interpolation and inverse-load
audits. The inverse planner's effective-servo approximation is **only a proposal
diagnostic**. It neither substitutes for the detailed evaluation motors nor
establishes walking feasibility. Failed inverse-load audits remain failures.

`prepare_contact_shape_experiment.mjs` binds each compiled reference into the
same `EmbeddedEnvironment` experiment, with all twelve CAD fixed-PD motor profiles,
11.1 V, 2 ms estimated delay, encoder/controller cadence, electrical dynamics,
mechanics, contacts, and the 80 degrees/s / 400 degrees/s² reference governor.
The physical robot JSON is asserted unchanged. Each candidate has an explicit
closed at-rest starting stance from shared Rust IK; the pose is experimental
controller/reset configuration, not a geometry or mass change.

Search coordinates now include stride displacement, period, independent foot
phase, stance fraction, swing height/shape, foothold center, and body stance pose.
The first deterministic designs jointly vary these quantities. The old gait's
amplitude and velocity-lead adjustments are disabled for generated paths.
Unused body/point-feedback references from the historical gait are removed;
physical task observations remain recorded.

## Comparison and gates

`validation-protocol.json` retains the prior balance, tracking, reference-bound,
foot-lift, collision, stopping, and numerical thresholds. Five-second screens do
not qualify a gait. Initial objective: at least 0.10 m/s and at least twice the
matched baseline speed, followed by ten- and thirty-second validation. A fully
accepted candidate still needs exact replay, stop/reversal checks, timestep
sensitivity, and longer-run tracking/balance evidence. Browser promotion and
hardware qualification remain separate deliverables and are not claimed here.

The historical command schedule included yaw. `matched-baseline-5s` therefore
reruns the old slow full-stride gait with zero yaw, matching the new straight-line
experiments. The requested nominal speed varies between candidates; the objective
is actual measured travel, not requested speed or inverse-plan speed.

## Current evidence

- The matched old gait achieves 0.03230 m/s over five seconds with zero yaw.
- `trot-upright/10s` passes all ten-second physical gates: 0.969 m traveled,
  0.09693 m/s, worst motor tracking 1.8753 degrees RMS.
- `upright-fast/10s` improves this to **1.071 m forward in ten seconds**, or
  **0.10713 m/s signed forward speed** (3.316 times the matched baseline).
  Worst motor tracking is **1.6044 degrees RMS**, with a 3.2509 degree maximum
  error. All four feet clear the floor repeatedly (10 excursions each; peak
  clearances 5.7–10.6 mm). Balance, command bounds and sampled collision pass.
  This result is from detailed physical dynamics, not the planned stride speed.
- The improved recipe uses a 0.9 s cycle, 0.15 m planned stride, 62% stance,
  alternating diagonal phases, a taller stance and narrower foothold centers.
  Thirty-second, half-timestep, stop, reverse and exact-replay qualification
  also pass; see `upright-fast/qualification.json` and the receipts below.
- Rejected designs and interpolation failures remain saved. Finer compilation
  sampling resolved interpolation limits without relaxing their thresholds.
- No browser or hardware candidate is promoted. Motor physics remain provisional; no new
  measured calibration, power/battery model, or hardware test has occurred.

## Qualification results

All requirements in the frozen `qualification-protocol.json` pass.

| Check | Observed result |
| --- | --- |
| Forward, 10 s | 1.07127 m signed forward travel; 1.6044° worst RMS tracking |
| Forward, 30 s | 3.26848 m signed forward travel; 0.10895 m/s; 1.6097° worst RMS tracking |
| Improvement | 3.37 times the matched five-second baseline speed over the sustained run |
| Feet and balance | All four feet repeatedly lift; no fall; sampled collision checks pass |
| Stop after 7 s | 39.49 mm maximum subsequent travel; 0.193 mm final-second drift |
| Reverse after 4 s | 345.16 mm signed reverse travel from 6–10 s; transient tracking passes |
| Halved physics step | 1.265 mm travel difference; 1.270 mm body-position difference; 0.0856° maximum joint difference |
| Replay | All frames and task transitions match exactly, excluding wall-clock diagnostics |

The reverse run ends near its start. Its generic forward-displacement check
therefore remains false in the saved report; the predeclared signed reverse
travel criterion passes. The qualification receipt explicitly selects the
direction-appropriate checks, without changing a threshold.

`long-run-protocol.json` retains the three-meter requirement and twenty separate
clearance excursions per foot. No tracking, balance or command limit was relaxed.
The planner's `required_load_audits_passed` remains false: the historical inverse
approximation does not certify the nominal unfiltered path. Acceptance here is
the actual governed motion in detailed dynamics, not that nominal path.

## Remaining work

This is a saved flat-ground simulation baseline. Browser integration needs a
measured realtime profile and comparison against this detailed model. Hardware
needs unit mapping, loaded-leg measurement and model calibration. Turning,
disturbance/terrain robustness, power sag and thermal scenarios remain unqualified.
The current detailed model takes about 27 wall seconds per simulated second on
this host; this is not yet a realtime browser profile.

`source-provenance.json` identifies the retained Rust source archive, source delta,
and executable hashes. Every experiment stores its full input specification,
seed, generated reference, captured observations, physical robot and runtime
identity. The preparation/statistics helpers only manipulate configuration and
measure saved output; the physics, controller and inverse kinematics remain Rust.

## Reproduce a saved episode

From the repository root (write output to a fresh file):

```sh
cargo build --release --locked -p sim-runtime --features bayesian \
  --example run_environment --example compile_contact_reference --example audit_capture_geometry
gait_case=examples/full-robot/measured-actuator-integration/gait-generation/contact-shape-2026-09-19/upright-fast
RAYON_NUM_THREADS=1 target/release/examples/run_environment \
  --experiment "$gait_case/10s/spec.json" > NEW_CAPTURE.json
RAYON_NUM_THREADS=1 target/release/examples/audit_capture_geometry \
  NEW_CAPTURE.json --pairs > NEW_GEOMETRY.json
```

Use the saved source identity for exact replay. The qualification preparation
helper produces separate long, half-step, stop and reverse experiment inputs.
The saved replay recording can be passed to `run_environment --replay`.
The generated reference itself can be rebuilt with `compile_contact_reference`
using `planning.scene.json`, `markers.json` and `upright-fast/fine/recipe.json`.

The tested forward input is a nominal 0.16667 m/s phase-rate request, producing
about 0.107 m/s actual motion. This controller does not close the loop on body
speed. Turns, arbitrary joystick schedules, rough terrain, disturbances, supply
sag, thermal drift and unit-specific motor calibration are not qualified here.
