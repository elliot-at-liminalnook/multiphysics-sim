# Nine-motor low-duty measurements and model refinement

September 14, 2026. **Actual powered motors, IDs 4–12, controlled by the FPGA.**
These are bounded unloaded response tests at 7.5–10% PWM, not full-speed stress,
endurance, rated-speed, or loaded-joint validation.

[Fresh measured/model overlay](fresh-prediction.png) · [Measured speed and acceleration](measured-speed-acceleration.png) · [Shared-load comparison](shared-load-comparison.png) · [Native viewer study](study.json) · [Portable study](study.html)

## Physical work completed

13 finite motion captures: 12 completed and one tripped a controller/bridge guard.
They retain **6,282 audited motor-frame observations** including the incomplete
capture and the axes holding position. Nine completed captures moved all nine
motors together; three moved one axis while all nine were still polled, controlled
and torque-enabled. Every capture, including the fault, verified all-nine stopping.

The trajectories include slow and fast mixed frequencies, simultaneous reversals,
phase offsets, and matched single-moving-axis/group comparisons. Each complete
capture has 54 frames at nominal 150 ms all-nine cadence, about 8.1 seconds.
The FPGA computes the integer PD/PWM command; the host schedules targets and
observations. Gains stayed Kp=3, Kd=0, velocity feedforward=4. Every frame retains
PWM and torque readbacks, raw bus transactions and request/reply timing.

At 10% drive, all-nine sampled peak speeds were approximately **22–24°/s**, versus
approximately 15–18°/s at 7.5%. These are encoder secants at the captured cadence,
not instantaneous or maximum achievable speeds. Acceleration plots retain the
same sampling limitation. Complete maximum-speed acceleration/reversal times
remain unresolved. Internal encoder feedback is not an independent shaft sensor.

Voltage reached 11.7 V. Maximum recorded temperature across these motion trials
was 53°C. The post-confirmation independent inspection found all nine at PWM zero,
torque off and reported speed zero, with temperatures 45–53°C. Power remains on.
Some devices report zero raw current even while moving: do not convert the existing
uncalibrated current field into a claimed measured supply current or power.

The failed `train-sync` capture completed 50/54 frames before reason 11, ID254
(controller/bridge guard). Cleanup succeeded. Telemetry and the commanded target
steps immediately preceding the fault do not establish a motor overload. The same
bounded synchronized plan and later tests completed. The specific FPGA guard path
remains undiagnosed; this failure is retained in both raw evidence and the study.

## Fresh model confirmation

Each motor received its own bounded fit of the shared MotorUnit's effective loss
current, loss-speed scale and rotor inertia. Four complete whole recordings trained
each fit. The earlier inspected ramp-repeat recording is a separate diagnostic
validation case, explicitly marked validation-influenced. 680 objective evaluations
are retained, including rejected steps. Six motors reached at least one parameter
bound; the constants are provisional and may absorb timing, sensing or drive error.
No claim of uniquely identified physical inertia/friction is made.

All nine models were frozen together at **2026-09-14 12:24:10 UTC**, before capturing
three reserved all-nine profiles: mixed, synchronized, and mixed at 10% drive.
The [confirmation protocol](confirmation-protocol.json), [candidate freeze](candidate-freeze.json),
[unchanged baseline](baseline-family.json), and [candidate family](candidate-family.json)
are retained. No candidate was selected or retuned using those new outcomes.

Own-feedback simulation uses the same Rust integer controller, captured command
and observation timing, initial temperature, and mean measured voltage per motor.
It does not feed measured positions to the simulated controller. Pooled position
prediction RMS decreased **21.6%, from 0.788° to 0.618°** across the three fresh
profiles. Recorded-PWM replay RMS decreased 11.3%, from 1.322° to 1.173°.

| Motor ID | Baseline prediction RMS | Candidate prediction RMS | Reduction |
| --- | --- | --- | --- |
| 4 | 0.994° | 0.567° | +42.9% |
| 5 | 0.849° | 0.471° | +44.5% |
| 6 | 0.761° | 0.917° | -20.4% |
| 7 | 0.678° | 0.447° | +34.0% |
| 8 | 0.805° | 0.647° | +19.7% |
| 9 | 0.730° | 0.677° | +7.3% |
| 10 | 0.709° | 0.366° | +48.5% |
| 11 | 0.777° | 0.774° | +0.3% |
| 12 | 0.740° | 0.488° | +34.0% |

These are pooled own-feedback errors across three fresh trials, not controller
tracking errors or statistical confidence intervals. Motor 6 regressed by 20.4%.
Motor 11's 0.3% change is negligible. Some other motors also regress on individual
profiles; all individual results remain in [confirmation-summary.json](confirmation-summary.json).
**Every fresh motor/trial still exceeds the original three-count (0.2637°) RMS
prediction limit. No candidate or CAD parameter was promoted.** Physical controller
tracking also remains outside its original limits; fitting the model did not change
the hardware controller or improve its tracking.

## What we learned about shared performance

ID4's matched trajectory yielded 1.251° tracking RMS with only ID4 moving,
1.200° with all nine moving, and 1.177° in the ID4-only repeat. All three used
all-nine polling and command cadence. Thus the group tracking change falls inside
this small repeatability comparison. Mean ID4 voltage was 11.885, 11.815 and
11.893 V respectively: a roughly 0.07–0.08 V difference, with 0.1 V telemetry
resolution. ID6-only tracking was 1.041° versus 1.058° in the group.
This limited comparison does not prove an absence of coupling or identify resistance.

The library now composes independent mechanical motor axes on **one shared
physical electrical circuit**, using existing registry MotorUnit, H-bridge,
source, resistance and sensor components. The captured FPGA adapter reuses the
same controller logic in single-axis and shared-supply simulations. Source and
branch resistance, electronics load, voltage/current/power and optional battery
SOC are explicit. Electrical scenario limits are retained as metadata; the group
report does not issue an electrical acceptance verdict.

Frozen 12 V ideal and 0.8-ohm shared-feed scenarios were run through the full fresh
10% capture with baseline and candidate models. They demonstrate executable
coupling and retain predicted branch and common-bus voltage/current/power. They
are **sensitivity hypotheses**, not calibrated bench or battery models. No external
current measurement is available to validate amps, watts, electronics draw,
regeneration or supply impedance. Do not treat the 21.6% conditional mechanical
prediction improvement as validation of the shared electrical source.

## Why the ceiling is 10%, and what remains

The commissioned FPGA controller image enforces 100/1000 maximum PWM, with
100-count tracking, 180-count relative-travel, 32-count target-step, temperature,
voltage, feedback-age and command-lease guards. Today's runs did not raise these
limits or load the unfinished autonomous firmware.

This ceiling is an acquisition restriction, not a motor capability rating.
Reaching the user's full-speed stress-testing goal requires a separately
commissioned higher-drive envelope: faster onboard acquisition, enough verified
travel for acceleration and braking, staged one/all-nine drive increases, and
fresh maximum-speed/reversal holds with adequate dwell. Keep the independent
stop path and diagnose the intermittent guard fault before expanding that envelope.
The live Sipeed autonomous-bridge integration remains incomplete and untested.

Improve motor 6 and low-speed/reversal model mismatch with experiments that can
distinguish drive/sensor latency, static friction and direction-dependent behavior.
Preserve the baseline and fit bounds; do not explain unknown timing by silently
inflating inertia. New model revisions need new reserved confirmation trials.
Loaded-joint/leg/robot acceptance still requires those physical setups, but further
unloaded characterization can continue without a leg. Calibrated electrical
measurements remain necessary for battery accuracy.

## Reproduction and software checks

34 relevant Rust tests pass: four group-circuit tests (KCL/Ohm/power, nine-axis
coupling and timestep refinement, ideal-source independence, single-axis parity,
and invalid setups), ten motion-analysis tests, seven FPGA-controller tests,
six device-event tests and seven upload tests. Example builds passed. Initial
failed software checks and corrected passing logs are retained. New plots were
visually inspected. The study export validates retained captures, fits and model
predictions through the shared library; native UI interaction was not retested.

Offline from the repository root:

```sh
cargo run -p sim-runtime --example review_fpga_controller -- RECORDING FAMILY closed-loop NEW_JSON
cargo run -p sim-runtime --example review_fpga_group -- RECORDING FAMILY SUPPLY_SCENARIO closed-loop NEW_JSON
cargo run -p sim-runtime --example review_controller -- measure-fpga-motion RECORDING ESTIMATOR NEW_JSON
cargo run -p sim-viewer -- --experiments examples/actuators/hx30hm/hardware/2026-09-14-unloaded-refinement/study.json
```

`fit-id*-request.json` reproduces each bounded fit using
`fit_fpga_controller fit REQUEST NEW_ATTEMPT`. `prediction-batch.json` records
exact offline prediction commands. `render_results.py` plots saved Rust outputs
only. Raw captures include acquisition sources and FPGA image identity. The final
manifest hashes artifacts and source snapshots; these files live outside ignored
`runs/`. The baseline remains the study draft; per-device candidates are retained
as separate fit/prediction evidence.
