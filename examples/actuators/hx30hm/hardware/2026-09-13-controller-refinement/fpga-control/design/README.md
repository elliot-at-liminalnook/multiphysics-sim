# Designing FPGA controller experiments

In **FPGA experiments**, select a completed acquisition and expand **Design a new
FPGA controller experiment**. Create a design, edit position gain, velocity damping,
velocity feedforward and duty limit, then optionally generate a new motion profile.
The profile combines two frequencies, a smooth start/finish envelope and a phase
spread across the selected motors. The shared Rust generator rejects unresolved
frequencies, invalid ramps and targets outside the FPGA's position/increment bounds.

Drafts are retained in study saves, including incomplete drafts. Simulation and
plan export require full validation. Editing a saved run creates a new draft; the
run and its source measurements stay unchanged.

**Simulate design against current model draft** uses the same integer control law,
quantized own-feedback observations and shared physical runtime as the FPGA
prediction workflow. It retains every motor's result or failure, captured model,
controller and runtime identity, simulated electrical channels and tracking score.
The source acquisition supplies actual observation/command timing, home position,
mean voltage and initial temperature. An explicit source/battery model in the
selected model draft supersedes the constant-voltage boundary.

These design results are simulation only. A changed controller or trajectory has
no matching physical validation until a new acquisition is made. Simulated target
tracking must not be reported as improved motor-model prediction accuracy.

**Export experiment plan** creates a new plan file for the supervised acquisition
tool. It neither loads firmware nor commands motors. The source firmware identity,
measured schedule, motor set and commissioned duty ceiling remain fixed in this
version. Gains are shared across the group; source timing is currently host-driven.

The headless equivalents use the same shared APIs:

```sh
cargo run -p sim-runtime --example review_controller -- prepare-fpga-design REVIEW INDEX PROFILE_JSON NEW_EXPERIMENT
cargo run -p sim-runtime --example review_controller -- design-fpga REVIEW EXPERIMENT_JSON NEW_REVIEW
```

`prepare-fpga-design` preserves the captured gains initially. Edit the generated
experiment's gains if desired; the simulator validates bounds before running.
`design-fpga` saves a new portable study and HTML report. No hardware is accessed.

Still needed: faster autonomous FPGA scheduling, per-axis gain design, robustness
and sensitivity workflows for new FPGA designs, accepted physical accuracy,
calibrated electrical measurements and later loaded-joint/robot validation.

## Retained demonstration

Open `design/review.json` with `sim-viewer --experiments` to inspect the saved
editable draft and all nine simulated results. The [native editor screenshot](native-editor.png)
shows this saved configuration. The profile uses phased 0.45 Hz / 1.035 Hz motion,
30-count amplitude, Kp 6, Kd 2, velocity feedforward 6 and a 7.5% duty ceiling.
All nine simulations completed; tracking RMS spans 0.476–0.652 degrees and all
nine still fail the captured tracking gates. This is neither a new motor
measurement nor evidence that physical model accuracy improved.

The [HTML study](review.html) and [machine-readable summary](results-summary.json)
retain that distinction. No motors were commanded during this design increment.

Verification: 25 existing refinement tests, 7 FPGA tests and the native eight-section
render test passed. A fresh native screenshot was visually inspected. Unchanged
designs reproduce existing prediction samples exactly; edited designs retain the
original recording fingerprint and have independent simulation tracking scores.
