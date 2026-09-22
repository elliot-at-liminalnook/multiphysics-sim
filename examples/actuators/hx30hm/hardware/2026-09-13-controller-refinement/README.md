# September 13 supervised bench measurements

User-authorized acquisition on nine unloaded HX-30HM motors, IDs 4–12, through
the Sipeed Tang Primer 25K supervised bridge. Rust owns both controller execution
and acquisition. No intentional external load; attached output hardware and
inertia remain unspecified.

## Evidence

- `initial-inspection/` and `read-only-baseline/`: retained no-response attempts
  before reloading the volatile FPGA image. No motion commands were sent.
- `fpga-image.json`: image identity and powered SRAM reload authorization.
- `post-reload-inspection/`: supervisor profile, register snapshots and all-nine
  zero-PWM/torque-off/stationary confirmation.
- `stationary-baseline/`, `baseline-timing.json`: 800 valid telemetry samples.
  Median read transaction approximately 5.84 ms; observed maximum 12.49 ms.
  These are host windows, not a sensor clock or a latency guarantee.
- `watchdog-refresh/`: command-loss, telemetry-loss and no-host-traffic tests at
  5% PWM on ID12. Each verified an independent trip, zero PWM, torque off and
  stationary feedback before host recovery commands. No physical cable removal,
  heat fault or power-isolation test is claimed.
- `small-pilot-id4/`: four 100 ms pulses at signed 2.5% and 5% PWM.
- `nine-motor-onset/`: 108 completed individual 100 ms onset pulses at signed
  2.5%/5% PWM, three repetitions, with at least 600 ms zero-PWM rest. All raw
  samples, transactions, commands, checkpoints and final stops are retained.
  `split-policy.json` records the predeclared 72 training / 36 held-out split.
- `zero-controller-cadence/`: 60 zero-action controller frames. Intended period
  50 ms; largest observed tick interval approximately 54.6 ms.
- `controller-id4/` through `controller-id12/`: one three-second return trajectory
  per motor, 60 frames each. Shared Rust position PID, 5% duty cap, quantized
  encoder feedback and estimated velocity. Requested excursion 0.08 rad. The
  recording captures the source/settings, start angle, measured initial conditions,
  actual tick times, feedback/command windows, written PWM and verified cleanup.
- `measurement-summary.json`: per-device tracking and condition summary.
- `final-inspection/`: all nine motors responding, zero PWM, torque off and zero
  reported speed. No actuator remains armed by this acquisition workflow.

`onset-review.json` is the portable fresh-pulse study. `all-motors-review.json`
and its HTML companion combine the pulse study and all nine physical controller
runs with their command-replay and own-feedback closed-loop predictions.

## Open the review

From the repository root:

```sh
cargo run --locked -p sim-viewer -- --experiments examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/reserved-id4-fitted-review.json
```

The hardware validation view keeps three conclusions distinct: controller
calculations reproduce, recorded commands predict measured motion, and a
simulated closed loop predicts its physical counterpart. A pass in one is not a
pass in the others.

## Limits

The motors were already warm (initially approximately 45–46 °C) and reported
approximately 12.1–12.4 V. This is neither cold-start characterization nor a
temperature sweep. Internal current counts are uncalibrated and are not supply
current. Encoder resolution is not calibrated angle accuracy. Host transaction
midpoints approximate observation/command application times; device sample age
is unknown. Simulation currently holds initial supply and temperature constant.

The initial model fails the provisional prediction threshold on all nine motors
(closed-loop RMS approximately 0.0072–0.0323 rad). These
measurements are for identifying and reducing the gap; they do not establish
accurate loaded torque, joint behavior or quadruped motion. No accepted physical
property revision has been written into the robot's CAD definition.

Native display evidence: [hardware review screenshot](viewer-reserved-validation-v2.png).
Verification logs are retained in `verification/`; the initial UI test failure
and its passing rerun remain distinguishable.

## Frozen model and unseen reversal validation

`fit-id4-loss-request.json` and `fit-id4-loss-review.json` retain a bounded,
validation-influenced fit of one effective motor-loss parameter. The offset is
−0.02237 A from the 0.1 A baseline. Its optimizer history and every trial prediction
are preserved; this is not independent measurement of no-load current.

`reserved-id4-reversal-protocol.json` freezes that candidate and the comparison
limits before collecting three repetitions of a new negative/positive trajectory.
`reserved-id4-reversal-0/` through `-2/` contain 80 controller frames each, raw
transactions and verified cleanup. The motors remained in the previously checked
PWM mode; no NVS changes or additional bridge reload were performed for these runs.
`pre-validation-inspection/` and `post-validation-inspection/` bracket acquisition.
The final inspection confirms all nine at PWM zero, torque off, speed zero, and
supervisor latched with no armed motors. Supply power itself remains on.

`reserved-id4-fitted-review.json` and its HTML companion retain the complete
combined study. `reserved-id4-results-summary.json` records the comparison:
closed-loop RMS improves from 0.0094–0.0117 rad to 0.0061–0.0081 rad, but every
repetition still fails the unchanged 0.004602 rad RMS limit. Recorded-command
replay also fails. Motor 4 reported 50–51 °C and approximately 12.1 V. No physical
model was adopted into CAD. Further fitting must retain these as already-inspected
validation results and reserve new confirmation data.

## Full-command fitting and paired release data

- `recorded-command-fit-request.json`, `recorded-command-fit-review.json`: one
  original return trajectory for tuning and all three reserved reversals for
  comparison, with immutable role/limit assignments and 18 objective evaluations.
  The effective loss plus small constant load-offset candidate improves training
  RMS to 0.005084 rad but worsens every reserved command replay. It is unsuccessful
  and remains unadopted. These inspected validation results are not called fresh.
- `recorded-command-controller-review.json`: separate command-replay and independent
  closed-loop predictions from that candidate; the three reserved closed loops
  still fail the original limits.
- `release-id4-plan.json`, `release-id4-split-policy.json`: original pre-acquisition
  definitions for twelve signed 5% release trials (200 ms drive, then zero PWM or
  torque off, three repetitions). The acquired segment windows preserve the
  actual commands. No PWM command is sent during torque-off observation.
- `release-id4/`: all samples, transaction log, outcomes and verified cleanup.
  `release-id4-measurement-summary.json` shows net offsets from the last sample
  before release: 3 counts for zero PWM, 4–5 counts for torque off. This does not
  interpolate the actual release angle or establish exact stopping time.
- `release-driver-hypothesis.json`: explicit uncalibrated passive diode/leakage
  model parameters for disabled drive. `release-id4-import-policy.json` normalizes
  the original roles after collection and conservatively marks the comparison as
  validation-influenced. Original source files are preserved.
- `pre-release-inspection/`, `post-release-inspection/`: all-nine state checks.
  Every motor is stationary with PWM zero and torque disabled in the final check.

`release-id4-review.json` and its HTML report compare all twelve releases against
the original physical baseline. `release-loss-inertia-fit-review.json` retains
an 8/4 tuning/reserved fit: three reserved releases pass, one fails. The fitted
rotor-inertia deviation reaches its declared upper bound. The transferred model
still fails the four captured controller comparisons in
`release-fit-controller-review.json`. These are preserved limitations, not an
accepted calibration. No comparison limits were relaxed.

To reproduce a release import into a new review (the output must not exist):

```sh
cargo run -p sim-runtime --example review_controller -- release-review \
  examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/release-id4 \
  examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/release-id4-import-policy.json \
  examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/release-driver-hypothesis.json \
  /tmp/new-release-review.json
```

This exercises the shared `robot.switchable_h_bridge` component through the
same physical runtime as fitting and interactive comparisons. It does not access
hardware. [Release viewer screenshot](viewer-release-review.png).


## Combined pulse, release and controller objective

`combined-loss-inertia-fit-request.json` freezes 17 training and 11 already
inspected validation trials: ID4 onset pulses, release trials and four complete
controller recordings. The same loss/inertia parameter bounds and original
comparison limits are retained. The portable dataset contains both full source
archives plus the frozen recording assignments; unselected trials are preserved.

After 30 objective evaluations, loss current is 0.068597 A and effective rotor
inertia is 2.6714e-9 kg·m², close to the declared lower search boundary. These are
fitted hypotheses, not independent measurements. The solver stopped at its
evaluation budget. The opposite inertia preference from the release-only fit is
further reason not to interpret this value as identified physical inertia.

| Comparison | Candidate result |
| --- | --- |
| Training pulses, releases and command history | 15/17 pass |
| Reserved onset pulses | 4/4 pass |
| Reserved release trials | 4/4 pass |
| Reserved controller-command replays | 0/3 pass |
| Independent closed-loop predictions, including original run | 0/4 pass |

Reserved closed-loop RMS errors are 0.006285, 0.007679 and 0.005931 rad against
0.004602 rad. The original controller run has 0.004468 rad RMS but fails the
separate final-error limit: six encoder counts versus five allowed. No limit is
changed to make this candidate pass. No physical parameter is promoted into CAD.

- `combined-loss-inertia-fit-review.json`: complete mixed-source attempt and traces.
- [Combined controller report](combined-fit-controller-review.html) and its JSON:
  the same candidate checked with recorded commands and independent feedback.
- `combined-results-summary.json`: extracted parameters and per-trial scores.
- `current-supervisor-inspection/`: current profile v1, latched, armed mask zero;
  all nine report PWM zero, torque disabled and speed zero. Supply remains on.
- `verification/combined-*`: compilation, 33 passing regression tests, fitting,
  controller comparison and validated report export logs.

No motor motion or FPGA reload was performed for this combined-evidence increment.

The rebuilt native viewer successfully loaded the combined review. Its
[checked screenshot](viewer-combined-controller-review.png) shows the candidate
closed-loop mismatch and keeps the unadopted draft distinction visible.

## Explicit low-speed loss transition

The shared motor component now exposes `loss_speed_scale` in rotor rad/s, with
the historical default of 5. It is the width of a smooth Coulomb approximation:
a smaller width reduces low-speed creep but does not create an exact static hold.
The residual, thermal loss and Jacobian use the same law. Tests preserve the old
omitted/default behavior, check derivatives and passive heat, and compare low-drive
steady motion against an independent torque/current balance at two timesteps.
The zero-power absolute-value heat cusp is checked against one-sided derivatives,
not incorrectly treated as differentiable; the initial test failure is retained.

`loss-transition-fit-request.json` starts from the preceding mixed-source candidate,
keeps all 17/11 source roles and limits, and adds a shared transition-speed bound of
0.02–5 rad/s. It also retains the previous loss/current and inertia bounds. The fit
stops at 50 evaluations, with the transition at its lower bound (0.02 rad/s),
effective loss current 0.060036 A and rotor inertia 1.94454e-7 kg·m². Neither the
bound nor the fitted inertia is an independently identified physical property.

| Comparison | Previous combined candidate | Loss-transition candidate |
| --- | --- | --- |
| Training pulse/release/command history | 15/17 pass | 17/17 pass |
| Reserved onset and release | 8/8 pass | 8/8 pass |
| Reserved recorded-command replay | 0/3 pass | 0/3 pass |
| Independent controller predictions | 0/4 pass | 2/4 pass |

Reserved independent-loop RMS errors are 0.004740, 0.005936 and 0.003974 rad.
Only the last passes both original limits. Original-controller RMS is 0.002765 rad
and passes. Reserved plant-replay RMS remains 0.012792, 0.024313 and 0.015815 rad;
one is worse than before. These failures prevent accepting the model as calibrated.
No comparison limits were relaxed and no additional measurements were acquired.

- `loss-transition-fit-review.json`: complete fit and frozen source datasets.
- `loss-transition-controller-review.json` and its HTML report: independent
  controller comparisons and separate recorded-command predictions.
- `loss-transition-results-summary.json`, `loss-transition-candidate.json`: concise
  reusable values and per-trial results, explicitly unaccepted.
- `verification/loss-*`: component/runtime/UI checks, the retained initial cusp
  test failure, corrected tests, fitting and controller comparison.

A new optional CAD model field, `electrical.loss_speed_scale`, reaches the common
robot mapping. Introducing it requires a version-2 proposal with an explicitly
undeclared previous value and recorded provenance/uncertainty. Existing numeric
version-1 changes remain supported. This was tested on a synthetic CAD artifact;
no real CAD source or accepted motor parameters were changed.


The later [electrical/battery review](../../../../experiments/battery-scenarios/README.md)
adds simulation sources, controller electrical feedback and calibrated-measurement
comparisons. It uses these unchanged recordings for a voltage-only comparison;
no new hardware capture, calibrated amps, measured watts or measured battery
behavior is claimed. Battery examples and protection thresholds are hypothetical.
