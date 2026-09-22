# FPGA controller development status

September 13, 2026. Actual FPGA controller execution and nine-motor experiments
are implemented and verified. **Accurate sim-to-real model acceptance remains
unfinished.** [View the measured plots and comparison report](report.html).

September 14 continuation: [fresh physical speed-ramp and reversal trials](../../2026-09-14-speed-response/README.md)
now include one motor and two all-nine runs, plus the same controller in simulation.
All runs completed and stopped correctly; accuracy and steady-speed settling still
fail their declared criteria. The report below preserves the September 13 results.

## Timing integration update

The [device event recorder](device-events/README.md) now runs with the transaction
core in nine integration groups. It preserves start/motor/terminal evidence under
backpressure. Six Rust groups validate real RTL-encoded synthetic packets,
including source identity, device timing, shared-law arithmetic and PWM audits;
seven existing upload groups pass. Serializer and stream synthesis pass. The
new batch review command retains incomplete captures as unscored. Bridge/serial/
viewer integration and physical stopping checks remain; no new motion was taken.

The [autonomous transaction core](autonomous-transactions/README.md) now joins
the scheduler, packet builder and raw-reply checks. Nine integrated groups,
seven parser groups and sixteen scheduler groups pass, including all nine motors,
missing evidence, signed PWM/torque audits and event retention through STOP.
Gowin synthesis and reproduction from the frozen source pass. The core is not
connected to bridge UART arbitration or programmed; these are software results.

The upload-enabled bridge passed final routed timing at **53.71 MHz** against
**50 MHz**. Its earlier 43.18 MHz placement estimate was preliminary. No new
image has been loaded. START/terminal identity, timestamp
transport, Rust acquisition, complete-image validation and new motor trials remain.

The user confirmed servo telemetry is the only electrical measurement available.
Keep reported voltage and raw current; calibrated amps, power, energy and battery
behavior remain unvalidated. This does not prevent the remaining motion work.

## Measured outcome

| Experiment | Physical tracking RMS | Physical peak | Outcome |
| --- | --- | --- | --- |
| ID4 small pilot, improved gains | 0.1973 degrees | 0.6152 degrees | Passes this pilot's limits |
| ID4 mixed-frequency trajectory | 0.4753 degrees | 1.2305 degrees | Fails accuracy limits |
| Nine motors, 7.5%, mixed-frequency | 0.871–1.141 degrees | 2.461 degrees worst | All move; accuracy fails |
| Nine motors, 7.5%, faster reserved pattern | 0.911–1.361 degrees | 3.604 degrees worst | All move; accuracy fails |

These are different trajectories, not directly comparable controller rankings.
The frozen tracking gates are 3 encoder counts RMS and 10 counts peak
(0.2637 / 0.8789 degrees). Feedback is each motor's internal encoder, not an
independent joint/output-shaft instrument. Velocity/acceleration plots are sampled
finite differences, with explicitly limited bandwidth and quantization.

Faster nine-axis prediction error using the same controller with its own simulated
feedback is 0.405–0.711 degrees RMS. Recorded-PWM replay error is 0.371–1.666 degrees
RMS. **Every axis fails the original 3-count prediction gate in both modes.**
A separate 60-evaluation fit of the existing symmetric loss/inertia model failed
all four ID4 training cases and its reserved validation case; inertia reached its
lower bound. Its candidate and all objective evaluations are retained as rejected.
No model parameters were promoted to CAD or accepted as identified constants.

## What is implemented

- One integer expression graph in `sim-domain-control::fixed_pd`, executed by Rust
  and compiled to synthesizable Verilog. The FPGA computes feedback-to-PWM and
  transmits synchronized motor commands. It does not receive host-computed PWM
  during these controller trials.
- The host still schedules polls and desired positions/displacements. Single-axis
  runs use 100 ms; the verified nine-axis schedule is 150 ms. The 20-FPGA-clock
  arithmetic settling contract is covered by RTL parity tests. This is not an
  autonomous FPGA trajectory or polling scheduler.
- Immutable target arrays, run roles, controller configuration, raw transaction
  windows, per-axis observations, PWM audits and source identities. Later runs
  additionally verify torque enable before and after every command. Prior recordings
  without that verification remain explicitly distinguishable.
- Both recorded-drive replay and independently closed-loop prediction use the
  shared MotorUnit/HBridge plant. FPGA recordings implement the existing calibration
  dataset trait; fitting preserves whole-run roles and original accuracy gates.
- Fifteen retained controller runs contain 3,102 motor updates, including the
  rejected zero-command timing attempt. Every captured PWM arithmetic audit matched.
  Exact host source revisions for all fifteen are archived and hash-checked under
  `verification/source-revisions/coverage.json`.

## Retained failures and diagnostic limits

The first nine-axis 100 ms zero-command run took about 174 ms and was stopped.
A batch command-lease renewal reduced traffic, and the 150 ms schedule then passed.
No watchdog timeout was extended to make it pass.

At 5%, some motors showed no motion or mainly one-way motion. Later isolated and
repeated group tests produced different responses. Explicit torque verification
confirmed enable state but did not uniquely identify the cause. Do not label all
of this variation as friction, oscillation, or packet loss. A bounded 7.5% stage
produced measurable motion on all nine in two complete coordinated runs. More
aggressive gains on the ID4 training pattern worsened tracking and were rejected.

The final loaded SRAM image is `firmware-bounded-drive/impl/bridge_control.fs`.
It has a 10% command ceiling; **actual trials used at most 7.5%**. Earlier images
and the failed DSP mapping / superseded combinational build are retained separately.
Final place-and-route timing passed 50 MHz (reported maximum 57.53 MHz). No full-load,
rated-current or thermal-endurance acceptance is claimed.

All nine were left at PWM zero, torque disabled and reported speed zero, with the
FPGA latched and no armed IDs. A separate 161-sample stationary observation had
zero encoder span on every motor and zero read failures. Final temperatures were
49–55 C. Command-loss, telemetry-loss and absent-host-traffic watchdog probes
passed on each of the three loaded images. Physical S2, broken bus and external
power isolation were not retested here.

## Validation

- 4,096 vectors: exact agreement between Rust, combinational RTL and pipelined RTL.
- Full UART tests: captured feedback, previous-position state, signed PWM,
  synchronized nine-axis output, repeated/stale feedback rejection, travel bounds,
  batch watchdog renewals, and the bounded 7.5% command stage.
- Independent supervisor regression: 21 injected cases, UART stop/tri-state checks,
  and retained release-bridge regression passed.
- Rust: 25 existing controller-refinement tests, 3 FPGA recording/simulation tests,
  and 3 electrical-measurement tests passed. The signed integer controller unit test
  also passed. The native viewer and all four FPGA experiment examples passed
  `cargo check`. Generated plots were visually inspected.
- Native FPGA integration: 25 refinement + 5 FPGA tests and the eight-section
  viewer render/import/cancellation test passed. The full 15-acquisition study
  loaded and exported; a fresh native screenshot was visually inspected.
- Earlier voltage-history work now has a tested guard preventing imposed measured
  voltage from being counted as independent voltage validation. UI wiring and
  broader varying-voltage coverage for that input mode remain separate work.

## What remains, in order

1. Integrate the tested [device-clock sequencer core](autonomous-scheduler/README.md)
   with its autonomous packet source, trajectory storage and timestamp transport.
   The [sealed trajectory store and Rust upload compiler](trajectory-store/README.md)
   now pass 19 storage and 11 packet-decoder groups, 13 Rust checks and Gowin
   synthesis. They are not connected to the bridge or loaded on hardware.
   The [buffered host bridge](buffered-bridge/README.md) now passes UART tests and
   routed timing; it has not been loaded.
   Measure the integrated hardware cadence and log device-clock event times. Host USB timing is a current accuracy
   limit, even though arithmetic already executes on the FPGA.
2. Add per-axis configuration and tune low-speed control on training profiles,
   retaining rejected gains. Quantify repeatability at different shaft positions
   and directions before adopting friction/deadband hypotheses.
3. Improve and validate the physical model beyond the current smooth symmetric
   loss approximation. Preserve older pulse/release regressions. Today's reserved
   data have now been inspected; the next candidate needs fresh confirmation.
4. Extend native FPGA design with per-axis gains and experiment-specific
   sensitivity and robustness. Shared gains, phased trajectories, saved drafts,
   design simulation and plan export are now available; see [design workflow](design/README.md). Import, per-motor plots,
   replay/own-feedback prediction, bounded fitting, fitted-family comparisons,
   cancellation and study save/export are now wired into the viewer. See
   [the retained native study](viewer/README.md). Hardware acquisition is still
   driven by the supervised headless tools.
5. Add calibrated supply/winding current measurements and synchronized voltage to
   validate amps, watts, energy and shared battery behavior. Register voltage is
   measured at 0.1 V resolution; internal current is still uncalibrated raw counts.
6. Progress from unloaded motors to measured joint loads, transmissions, backlash,
   thermal effects and the robot assembly. Nine independent simulated motors do
   not validate a shared electrical supply or full CAD robot.
