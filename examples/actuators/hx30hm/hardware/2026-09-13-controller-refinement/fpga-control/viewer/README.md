# FPGA experiments in the Rust viewer

Open the retained study from the repository root:

```sh
cargo run -p sim-viewer -- --experiments examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/viewer/review.json
```

The viewer opens **FPGA experiments** automatically. This study contains all 15
retained hardware acquisitions, the final nine-motor replay and own-feedback
predictions, and the rejected ID4 fit. Original recordings, roles, controller
settings, per-motor model snapshots and prediction source identities are embedded.
The underlying PWM archive remains available in the existing study views.

- Import `fpga-recording.json`. Interrupted acquisitions remain visible and unscored.
- Select a run and motor. Compare targets, measured position, PWM, voltage and
  uncalibrated current. The tracking table accounts for every motor.
- Predict recorded PWM response or the controller's own simulated feedback using
  the current model draft. Jobs run off the UI thread and can be cancelled.
  Per-motor failures remain visible; successful motors cannot hide them.
- Set parameter bounds under **Sensitivity & fitting**, then fit complete FPGA
  training runs. Original validation roles are immutable; timing commissioning
  and incomplete acquisitions are excluded. These inspected validation runs are
  not fresh confirmation data.
- Each fit retains its request, candidate, objective history and failures. Compare
  the fitted family on the selected recording, preserving per-device deviations.
  This does not adopt the candidate into CAD or change the draft.
- Save a new study revision or export HTML using the normal study controls.
  Both retain FPGA evidence; exported plots separate tracking and prediction error.

The current physical accuracy failures remain failures. These UI features do not
establish calibrated amps, watts, battery sag, loaded-joint behavior or robot-level
accuracy. The FPGA calculates duty; the recorded host still schedules observations
and commands at 100/150 ms. Faster autonomous scheduling remains outstanding.

## Headless counterparts

`review_controller` uses the same shared validation and simulation:

```sh
cargo run -p sim-runtime --example review_controller -- import-fpga REVIEW RECORDING NEW_REVIEW
cargo run -p sim-runtime --example review_controller -- predict-fpga REVIEW INDEX replay NEW_REVIEW
cargo run -p sim-runtime --example review_controller -- predict-fpga REVIEW INDEX closed-loop NEW_REVIEW
cargo run -p sim-runtime --example review_controller -- import-fpga-results REVIEW PREDICTIONS NEW_REVIEW
cargo run -p sim-runtime --example review_controller -- export REVIEW NEW_HTML
```

Indices start at zero. All output paths must be new. `import-fpga-results` accepts
retained `review_fpga_controller` prediction files after the corresponding source
recording has been imported. It validates every motor's frozen model, role-bound
recording identity, sample timings and score before retaining the result.

## Verified in this increment

- 25 existing controller-refinement tests and 5 FPGA evidence tests passed.
- The viewer's eight-section render test passed, including real FPGA import,
  duplicate rejection and a cancelled nine-motor comparison.
- The retained 15-acquisition study was validated by the shared Rust loader and
  exported to HTML. Earlier per-device model snapshots remain intact.
- A fresh native viewer instance opened the study automatically in FPGA experiments;
  [its screenshot](native-panel.png) was visually inspected. The verification
  instance closed itself after capture; existing windows were not modified.

Editable FPGA gains and motion profiles are now available in the
[controller design workflow](../design/README.md), with saved drafts and plan export.
