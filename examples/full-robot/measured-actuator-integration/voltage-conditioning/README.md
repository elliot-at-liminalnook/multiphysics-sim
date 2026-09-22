# FPGA motor identification with measured voltage

Current priority: gait generation with provisional motor dynamics. The historical
0.264° threshold and failures below are retained evidence, not a gate for gait
work. No replacement calibration threshold or newly accepted fit has been declared.

The new shared Rust path conditions motor prediction on each motor's captured
terminal-voltage history instead of reducing the capture to its mean. It reuses
`electrical.voltage_history`, the existing motor/bridge plant and the same integer
controller used by the FPGA. The legacy mean/scenario modes remain available.

## Implemented

- `fpga_voltage::condition_model` resolves one recording and one motor into an
  explicit voltage-source scenario with source identity and replaced-source receipt.
- `fpga_voltage::predict` supports recorded PWM replay and independently simulated
  encoder feedback. Voltage samples use request/reply midpoints, linear interpolation
  and held endpoints. Unknown internal sample age and calibration remain visible.
- Typed reviews verify source/motor identity, source parameters, mode, scores and
  the actual simulated voltage trace. Conditioning survives workspace save/reload.
- FPGA datasets opt into measured-voltage fitting explicitly and retain that choice
  in their fingerprint. Legacy datasets keep their serialized identity and behavior;
  timing captures remain ineligible for fitting.
- The viewer offers both new comparison modes. Its FPGA fitting action now declares
  measured-voltage conditioning. Existing worker progress/cancellation and immutable
  review storage are reused. This does not promote a fit into CAD.

## Headless usage

```sh
cargo build --locked -p sim-runtime --example review_fpga_controller --example fit_fpga_controller
# MODEL may be ModelSettings or a shared/per-device Family.
target/debug/examples/review_fpga_controller RECORDING MODEL replay-voltage NEW_RESULT
target/debug/examples/review_fpga_controller RECORDING MODEL closed-loop-voltage NEW_RESULT
```

The existing `review_controller predict-fpga` study operation accepts the same
`replay-voltage` and `closed-loop-voltage` names. For `fit_fpga_controller
prepare-device`, add `"measured_voltage": true` to its settings. The generated
request binds the original whole-trial roles to the selected input mode.

## Current evidence

The previous candidate family still fails under measured voltage. On the retained
100 Hz low-drive capture its own-feedback prediction RMS is 1.682/1.658/1.156°
for motors 10/11/12. On a 45% stress capture it is 1.322/1.356/1.421°. The frozen
criterion is 0.264°. These timing captures diagnose errors and are not relabeled
as clean held-out validation.

New per-motor fits use the original four training trials and separate validation
trial, unchanged three-parameter bounds and 80-evaluation budget. Their requests
explicitly retain prior validation influence. Fit attempts and subsequent
own-feedback checks are retained here; none is automatically accepted.

Voltage is an imposed test input, not a predicted battery response. Simulated
current/power are conditional, uncalibrated predictions. The single-axis replay
cannot validate shared battery/wiring behavior. The CAD/runtime integration and
full quadruped comparison remain separate unfinished plan milestones.

## Refit outcome

The three fits completed, reaching the rotor-inertia upper bound. Under the
same recorded voltage on the separate validation pattern, own-feedback RMS is:

| Motor | Baseline | Refit | 0.264° criterion |
| --- | --- | --- | --- |
| 10 | 1.773° | 1.916° | Fail |
| 11 | 1.709° | 1.005° | Fail |
| 12 | 1.654° | 1.058° | Fail |

The refit improves two motors and regresses one. At 100 Hz, its low-drive RMS
is 1.682/1.295/1.430° and stress RMS is 1.322/1.480/1.358°; all fail the historical
criterion. No candidate is promoted. Future calibration should investigate residual
structure and parameter identifiability and define task-relevant acceptance criteria.
The user's decision to proceed with gait search does not change these measurements.

Ten FPGA controller/review tests and ten motor-response tests pass, plus the
viewer refinement render test. These establish software behavior and evidence
handling; they do not establish calibrated motor or quadruped accuracy.
