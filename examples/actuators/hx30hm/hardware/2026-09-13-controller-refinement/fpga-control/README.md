# FPGA controller tracking experiments

This work follows the request to execute the controller on the connected FPGA,
then execute the same controller against the shared Rust motor model.

The feedback-to-PWM expression is defined once in
`sim-domain-control::fixed_pd`. Rust executes its integer expression graph;
`compile_fixed_pd` emits synthesizable Verilog from that graph. A registered
implementation has a required 20-clock settling interval, exercised by the
bridge before using its result. The resulting PWM batch is transmitted directly
by the FPGA. The host sends desired encoder positions and per-tick displacement,
then reads back the motor PWM registers to audit arithmetic agreement. The host
still schedules polling and target updates. This is not yet an autonomous FPGA
trajectory/polling scheduler.

The controller is discrete PD with velocity feedforward, Q8 gains, division
truncated toward zero, and symmetric saturation. It uses internal encoder counts;
there is no claim of independent output-shaft accuracy. Polled register timestamps
are transaction windows, not exact sensor acquisition timestamps. Simulation uses
window midpoints and bridge receipts as explicit timing approximations.

## Frozen first acceptance criteria

- Tracking RMS <=3 encoder counts (0.263671875 degrees); peak <=10 counts
  (0.87890625 degrees), compared against the previously applied held setpoint.
- Shared-controller integer PWM agreement on every captured frame.
- Model prediction RMS <=3 encoder counts, separately for recorded-drive replay
  and own-feedback controller simulation. These are distinct tests.
- Complete finite trajectory and verified stationary PWM-zero/torque-off for all
  nine motors after each trial. Failures are retained and never promoted.

The initial live preflight found all nine stationary in PWM mode, with torque off,
12.1–12.4 V and temperatures 49–54 C. This is a warm, unloaded bench. Tests retain
<=5% PWM, <=180 counts observed travel, a 58 C host cutoff, and the independent
FPGA's existing 60 C/9–12.6 V/raw-current and command/feedback watchdogs. These are
bounded coordinated tracking tests, not rated-current, loaded or endurance tests.

Voltage is retained in 0.1 V register counts; current remains an uncalibrated raw
register. Measured amps, watts, supply droop transients, battery state and
regeneration cannot be validated from these channels alone. No measured current
or power is synthesized from the raw register.

## Build and test

From physics-simulator:

```
cargo run -p sim-domain-control --example compile_fixed_pd -- /path/to/sipeed/servo/impl/fixed-pd
cargo test -p sim-runtime --test fpga_controller
cargo build -p sim-runtime --example characterize_hx_bridge --example plan_fpga_controller --example review_fpga_controller
```

From the Sipeed servo directory: `make sim-control bridge-control`.
The `bridge-control` target only builds. SRAM loading is a separate action and
must have its result retained. The retained supervised bridge remains separate.

`plan_fpga_controller BITSTREAM STYLE ROLE NEW_PLAN` freezes target arrays before
acquisition; styles are zero, pilot, complex, nine and nine-fast. Each motor in a
nine-axis pattern has a different phase. Quintic envelopes smoothly start and stop
the mixed-frequency trajectory; actual targets are quantized to encoder counts.

`review_fpga_controller RECORDING MODEL_OR_CANDIDATE replay|closed-loop NEW_OUTPUT`
uses the shared MotorUnit/HBridge fixture and the same integer controller for
own-feedback predictions. Nine motor predictions currently use independent plants;
this must not be mistaken for a calibrated shared-battery robot simulation.

## Current evidence

[Status and remaining work](STATUS.md), [plots and report](report.html), and
[machine-readable results](results-summary.json) retain the final outcome.
Three SRAM images were commissioned, and the final two 7.5% trials moved all nine
motors with per-frame torque and PWM verification. No motor model was accepted.
The initial 5% conditions above describe the first phase, not the final firmware
ceiling. See STATUS for the exact final image and measured limits.

`fit_fpga_controller prepare MODEL NEW_REQUEST RECORDING...` captures a fitting
request; `fit_fpga_controller fit REQUEST NEW_ATTEMPT` uses the shared optimizer.
Calibration compares continuous predicted angle with quantized observations;
the closed-loop controller still receives quantized simulated feedback. Rejected
candidates and whole-run validation roles are retained.

`verification/render_report.py` is offline plotting only. It reads retained JSON
and CSV; it never controls hardware or simulates physics.

## Native review

The [Rust viewer study](viewer/README.md) includes all 15 acquisitions, final replay
and closed-loop comparisons, and the rejected fit. See the
[native panel screenshot](viewer/native-panel.png) and [exported study](viewer/review.html).
