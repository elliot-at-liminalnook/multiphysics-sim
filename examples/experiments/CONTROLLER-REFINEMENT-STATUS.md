# Controller/model refinement status

September 14, 2026. Implementation and bench evidence are in progress. The complete
accuracy acceptance goal is **not yet achieved**.




## Measured-actuator implementation: voltage-conditioned calibration

Shared FPGA prediction now supports each motor's recorded voltage history with
source-bound review validation. Viewer comparisons and fitting expose this mode;
legacy predictions/dataset identities remain compatible. The selected CAD baseline
and 34 captures are frozen. Twenty Rust regression tests and the viewer render
test pass; saved physical-capture predictions validate through the shared parser.

Three refits completed with unchanged bounds and retained validation influence.
Own-feedback validation RMS is 1.916/1.005/1.058° for IDs 10/11/12: all exceed
0.264°, and ID10 regresses. No fit is promoted. CAD profile authoring, shared
incremental motor/power integration and the unchanged-gait comparison remain.
See [implementation status](../full-robot/measured-actuator-integration/IMPLEMENTATION-STATUS.json)
and [retained calibration evidence](../full-robot/measured-actuator-integration/voltage-conditioning/README.md).

## September 15 pivot: measured actuators in the CAD quadruped

Priority shifts from more bench stress to integrating measured actuator behavior
through CAD into the shared Rust simulation. Source/CAD audit confirms the historical
0.5736 m/s gait uses a simplified effective servo that bypasses the detailed motor
and electrical dynamics; changing CAD electrical constants alone cannot fix it.
The preserved robot has twelve motors, no battery and no authored sensors.

[Integration plan and verified baseline inventory](../full-robot/measured-actuator-integration/README.md)
cover per-unit profiles, shared power, the FPGA controller in simulation, bench
validation and the unchanged-gait comparison. This is an audit/plan milestone;
CAD/runtime integration and accepted calibration remain unfinished.

## September 15: high-speed 100 Hz stress recorded

Nine two-second physical bursts on IDs 10–12 completed at 20–55% PWM: 1,800
frames and 5,400 audited motor updates, all stops verified. At 45% the three
repeats reached sampled peaks of 106–123°/s with up to 8.5% voltage sag. One
55% trial reached about 123°/s but 12.6% sag (10.4 V minimum), ending escalation.
Temperature remained at or below 45°C. These are separate bursts, not continuous
endurance or settled maximum-speed qualification. All motors finished zero/off.

Fast-reversal tracking remains poor: 55% RMS errors are 3.90/3.83/3.59° for
IDs 10/11/12, well above the unchanged 0.264° goal. No model was fitted/promoted.
Raw commands, feedback, current registers, voltage, temperature, device events,
shared Rust estimates and plots are saved in the [stress report](../actuators/hx30hm/hardware/2026-09-15-high-speed-stress/README.md).

## September 15: physical 100 Hz loop qualification passes at low drive

The FPGA and shared Rust validators now allow 10 ms for at most three motors;
larger groups retain 40 ms. The new image passes 50 MHz timing (59.62 MHz maximum).
Seven actual captures completed on IDs10–12, including 100 Hz zero drive, 5% and
10% motion, and matched 40 ms baselines: 1,100 frames / 3,300 audited motor updates,
all stops verified. Minimum motor voltage 11.7 V, maximum temperature 44 °C.
First-axis request intervals are essentially 10.000 ms and complete transaction
windows stay below 3.646 ms. All three physical watchdog probes pass.

Pooled discrete-setpoint tracking RMS changes from 0.656/0.683/0.560° at 40 ms
to 0.469/0.365/0.320° at 10 ms for IDs10/11/12. All remain above the unchanged
0.264° target. This compares configurations, not isolated sample rate: setpoint
sampling and per-tick feedforward also change. No new fit/model promotion.

The faster mock uncovered count-limited stop history dropping the required time
span; retention now covers time while preserving 150 ms stationary verification.
Six full-UART normal/fault scenarios and Rust integration checks pass. The initial
FPGA reply failure, slow-link test setup timeout, and mock failure remain retained.

[Fast-loop source, tests, image, physical captures and next steps](../actuators/hx30hm/hardware/2026-09-15-fast-loop/README.md).

## Current three-motor chain: measured speed, sag and tracking limits

After the supply adjustment, actual FPGA-driven tests on IDs 10–12 retained
1,402 complete frames / 4,206 audited motor updates across 18 captures. Sixteen
completed (including zero drive); two interrupted, and every final stop was
verified. Three 10-second 35% reversal blocks completed with 90–97°/s sampled
peaks, minimum 11.2 V (worst sag 5.9%), and maximum 50 °C. These blocks have
stop/review gaps and do not prove precise tracking or indefinite reliability.

At 45%, individual-moving tests showed about 3% sag versus 8–9% with all three
moving. Two longer 45% attempts tripped broad reason 11 with ID11 target errors
of 102 and 101 counts, consistent with the unchanged 100-count tracking guard.
The failed evidence is retained. Short 75% reversals reached about 134°/s but
17.6% sag; 100% was not run. Full settled speed and exact reversal time remain
unresolved with the current travel/cadence and overshooting controller.

Individual bounded motor fits improve recorded-PWM replay but regress own-feedback
controller prediction for IDs10 and11. On the separate validation pattern,
candidate RMS errors are 1.914°, 1.762° and 1.085° versus the 0.264° target.
All fail; no CAD/model promotion, controller gain, firmware or protection-limit
change was made. Final inspection confirms PWM zero, torque off and speed zero.

[Three-motor measurements, plots, failure analysis and model results](../actuators/hx30hm/hardware/2026-09-14-three-motor-characterization/README.md).

## Full-drive request: implementation ahead of hardware validation

The user authorized 100% PWM and fast direction-switching tests on all nine
motors. The integrated device-clock controller/recorder and Rust acquisition
adapter are implemented. Full UART simulation checks +1000/-1000 drive and
fault stopping, and recorded bytes reproduce the shared Rust controller.
The full image is now packed and passes 50 MHz timing (60.93 MHz reported maximum).
The user confirmed power off; the exact image is loaded into SRAM and its live
1 Mbaud profile is verified. After confirmed power-on, all nine motors pass
inspection, three real 5% ID12 watchdog trials pass, and 12 zero-drive onboard
frames complete with 108 audited motor updates at 40 ms cadence. The physical
S2 motion-stop test and explicit rearm now pass. All-nine 25% and 50% rapid
reversal trials also pass with verified stopping; 50% motor voltage falls to 9.1 V.
The user reports a 12.6 V / 3 A supply setting. Higher-drive stages are held pending
supply/distribution review; no new model fit has been made. The existing
10% physical dataset and models remain unchanged.

[Current full-drive status, synthetic verification and next steps](../actuators/hx30hm/hardware/2026-09-14-full-drive/README.md).

The first 3.5 A comparison attempt failed before ARM when the FPGA stopped
replying. The user then explicitly permitted powered image reloads. Reloading the
same frozen SRAM image restored communication; fresh watchdog and zero-drive
checks passed. The unchanged all-nine 25% trial completed with all 45 PWM audits
matching the 3 A baseline and verified all-nine stopping. Minimum motor voltage
was 11.0 V versus 10.9 V at 3 A, only one telemetry increment; this does not
establish a clear improvement or identify the sag's cause. No higher-drive
escalation or new model fit was performed. All failed evidence is retained.

The user then requested higher stress. A matched all-nine five-frame ±50% trial
at the reported 3.5 A setting completes with all 45 PWM audits matching the earlier
3 A trial. Minimum motor voltage is unchanged at 9.1 V; maximum temperature is
48 °C and all-nine stopping is verified. Further escalation remains held near
the undervoltage threshold. Shared Rust motion analysis reports 74.7–90.1 degrees/s
per-motor peak interval-average speeds in this short trial, not rated maximum
speeds. No new model fit or promotion has been made.

An extended all-nine 25% trial now completes 30 seconds of commanded time in
three 10-second blocks, with stopping verified between blocks: 750 frames and
6,750 audited motor updates. Minimum voltage is 10.8 V and maximum temperature
49 °C. The user observed about 0.86 A and CV throughout, arguing against sustained
current limiting at this drive level while leaving transient/distribution and
telemetry causes unresolved. All motors finished PWM zero and torque off. These
are timing diagnostics; no new model fit or promotion has been performed.

## Nine-motor low-duty refinement and shared-supply modeling

Thirteen new physical captures retain 6,282 audited motor updates: twelve complete
and one guard-fault interruption, all with verified stopping. Nine completed
captures moved all nine motors simultaneously. The FPGA drove mixed-frequency,
synchronized reversal and matched single-moving/group profiles at 7.5–10% PWM.
Sampled peaks reached 22–24°/s at 10%; this is **not full-speed stress testing**.
Final post-confirmation inspection: all nine PWM zero, torque off and speed zero,
45–53°C. The reason-11 FPGA guard fault remains undiagnosed and retained.

Separate bounded models were fitted for IDs4–12 and frozen before three fresh
all-nine confirmations. Own-feedback prediction RMS improved 21.6% overall,
0.788° → 0.618°. Eight motors improved in pooled error, but ID6 regressed 20.4%;
ID11's improvement was negligible. Every fresh motor/trial still fails the
unchanged 0.2637° RMS prediction gate. No model/CAD promotion or hardware gain
change. Raw PWM replay improves 11.3%, also without acceptance.

Shared Rust bench components now support independent shafts coupled through one
explicit source and wiring network, with branch and total voltage/current/power.
The same FPGA controller adapter drives this runtime. Full nine-axis source
sensitivity runs are saved, but supply parameters, amps, watts and battery accuracy
remain unvalidated with servo telemetry alone. Thirty-four relevant Rust tests
pass, including circuit laws, timestep refinement and single/group parity.

**Next hardware milestone:** commission a higher-drive acquisition envelope with
faster onboard feedback, sufficient measured travel for braking/reversal, staged
drive increases and retained watchdogs. The current commissioned image caps drive
at 10%; the incomplete autonomous image is not ready for use. Resolve the rare
guard fault and motor6 model regression; freeze new candidates before new tests.

[Physical measurements, fitted models, plots, shared scenarios and viewer study](../actuators/hx30hm/hardware/2026-09-14-unloaded-refinement/README.md).

## Morning physical speed-ramp and reversal trials

At the user's request, restored the prior commissioned FPGA controller image and
ran actual motors: one ID4 ramp/reversal trial and two synchronized nine-motor
trials, after live inspection, three watchdog checks and a zero-command trial.
All three motion trials completed, retaining 1,052 motor updates with exact
shared-controller PWM audits and verified all-nine stopping. Final independent
readback: PWM zero, torque disabled, reported speed zero, temperatures 41–44 °C.

At a requested 3.52°/s and 7.5% drive cap, physical tracking RMS was 0.292° for ID4,
0.315–0.639° for the first group trial and 0.382–0.577° for its repeat. The same
controller with simulated feedback predicted the repeat with 0.487–0.705° RMS
error. Every motor still fails the unchanged 0.2637° RMS tracking/prediction gate.
These are different patterns from yesterday, not a proven general improvement.

Speed varied within the ramps. None of 57 declared transitions established the
required final steady-speed tail, so full speed-up/reversal times remain unresolved.
The new shared analyzer and native panel compare sampled speed/acceleration with
the same estimator in hardware and simulation; preserve uncertainty, conditions
and missing plateaus. Nine new tests and 20 retained FPGA tests pass. No model or
CAD parameters were promoted. Servo telemetry alone still cannot validate amps,
power, battery behavior, full-drive or loaded performance.

See [physical evidence, plots, viewer study and reproduction](../actuators/hx30hm/hardware/2026-09-14-speed-response/README.md).
The incomplete autonomous bridge source remains untested and was not programmed;
its arbitration/START/output work and faster acquisition are still outstanding.

## Night handoff — stopped at the user's request

Work resumed at the user's request on September 14. Bridge integration began,
then the user prioritized physical speed/reversal experiments; see the fresh
results above. The following paragraphs preserve the previous night’s handoff.

At the previous night's handoff, process inspection found no running
FPGA builds, Rust builds/tests, RTL simulations, firmware loaders or motor-control
acquisition processes; no process held the bench serial port. No firmware was
loaded and no motion was commanded during this wrap-up. The physical power supply
was not switched off by software. The last retained inspection reported all nine
motors at PWM zero, torque disabled and speed zero; that is historical evidence,
not a new live readback.

Current result: the FPGA controller has driven all nine physical motors, but the
harder coordinated trials still miss the accuracy target. Best simple single-motor
tracking was 0.197 degrees RMS; faster nine-motor tracking was 0.911–1.361 degrees
RMS, with a 3.604-degree worst peak, against 0.264 RMS / 0.879 peak limits.
Closed-loop simulation prediction error was 0.405–0.711 degrees RMS. Recent work
improves timing/recording infrastructure; it has not demonstrated better physical
accuracy. No fitted model or CAD parameters were promoted.

Code and reproducible test evidence are saved locally in the simulator and Sipeed
working trees. The upload-only image passes routed timing and is archived. The
scheduler, recorder and Rust review checks pass their documented component and
integration tests. The complete autonomous image has not been wired or commissioned.

Resume, when requested, in this order:

1. Wire the tested transaction core and recorder into bridge arbitration, sealed
   row access and accepted START handling; preserve raw traffic, independent STOP,
   controller-history initialization and whole-packet host output.
2. Connect serial acquisition and the viewer/simulation recording adapter, with
   explicit device timing and independent physical post-stop evidence.
3. Run full UART/watchdog checks and route the combined image. Commission it on
   hardware, establish zero-command cadence, then collect bounded one/all-motor
   trajectories with held-out repetitions.
4. Refine per-motor dynamics/gains and evaluate unchanged accuracy gates. Finish
   FPGA-specific sensitivity/uncertainty views and remaining feature coverage.
   Later loaded-joint/robot validation requires those physical setups. With servo
   telemetry only, calibrated current, power, energy and battery validation remain
   unproven.

## Device-clock recording and Rust evidence checks

The transaction core now runs with a recorder that retains START, motor events
and terminal evidence through host backpressure. Six serializer/lifecycle groups
and nine combined transaction/recording groups pass; serializer/recorder synthesis
passes. The original nine transaction groups still pass. The serializer uses one
packet bank, with byte-identical output after its resource reduction.

Six Rust event tests decode 59 actual RTL-encoded synthetic fixture packets, check
source identity, frame timing, all-motor coverage, sample-sequence wrap, shared
controller arithmetic and exact PWM audits. Incomplete captures remain unscored;
corrupt/reordered records and false success are rejected. Seven existing upload
tests also pass. The batch reviewer now supports `review-device-capture` with
source/capture hashes and an explicit statement that protocol completion does
not prove mechanical stopping or model accuracy.

Bridge UART arbitration, the accepted START path, physical post-stop checks and
the serial/viewer recording adapter remain. These changes contain no new motor
motion or accuracy measurements. See [source, protocol and reproduction](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/device-events/README.md).

## Autonomous transaction integration and available electrical evidence

The scheduler, packet builder and raw-reply adapter now run together in an FPGA
transaction core. It freezes gains at start, pairs raw telemetry with independent
supervisor acceptance, and checks torque/PWM readbacks against the exact transmitted
batch. Backpressure and cancellation retain completed event data through STOP.
Nine integrated groups, seven reply-parser groups and sixteen scheduler groups
pass; integrated Gowin synthesis passes. These are modeled bus transactions,
not new physical trials or complete bridge UART verification.

This core still needs bridge arbitration, START/terminal identity, timestamp
transport and Rust acquisition integration, followed by complete-image timing and
hardware commissioning. See the [frozen source and verification](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/autonomous-transactions/README.md).

The user confirmed **servo telemetry only** for electrical measurements. Continue
motion refinement and retain reported voltage/raw current. Calibrated amps, power,
energy and battery behavior remain unvalidated; missing external sensing does not
prevent the remaining software or unloaded motion work.

## Upload-enabled bridge verification

The sealed trajectory store is now connected to an optional buffered bridge with
CONFIG/ROW/SEAL/STATUS receipts. Eight UART upload groups, retained controller tests,
seven Rust upload/transfer tests, and both sets of 4,096 arithmetic vectors pass.
Compact supervisor logic passed all 1,157 formal comparison points against the
preceding implementation. No safety threshold or controller arithmetic changed.

The compact build passed final routed timing at **53.71 MHz** against **50 MHz**;
the earlier 43.18 MHz placement estimate was preliminary. The
earlier larger build failed placement. The upload profile has not been loaded and
does not implement START. A separate read-only hardware refresh found all nine
motors reporting torque off, PWM zero and speed zero, 12.1–12.4 V and 48–54 C.
Those are inspection readings, not new motion accuracy evidence.
See the [upload bridge source and verification](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/bridge-trajectory/README.md).

## Sealed FPGA trajectory storage and upload

The Rust compiler now produces provenance-preserving CONFIG/ROW/SEAL artifacts
for complete nine-motor trajectories. FPGA RAM verifies every row, motion bound,
delta and CRC before making it available to the scheduler. Malformed, incomplete
or changed uploads invalidate the plan; writes during execution cannot alter RAM.
Nineteen storage groups and eleven packet-decoder groups pass, including frozen
Rust packets through the real host byte queue. Decoder/store Gowin synthesis
passes with eight block RAM primitives. Five upload tests, seven existing FPGA
recording/design tests and the standard CRC unit check pass.

The initial isolated milestone below is superseded by the upload-enabled bridge
and transaction integration above; no new image has been loaded. These components
do not start or arm motors. Timestamp transport, Rust capture and integrated
hardware commissioning remain. The Rust
Plan retains its 50 ms minimum; 40 ms in the RTL is a provisional target.
See [source, protocol and reproducible vectors](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/trajectory-store/README.md).

## Buffered FPGA bridge

The optional buffered host path now accepts only complete checksum-valid packets,
waits for safe bus ownership, and gives STOP/DISARM a separate priority path into
the existing supervisor. STOP during an active servo reply passes wire-level
checks without corrupting telemetry/status or causing bus contention. Eight queue
cases, both host-mode safety regressions and 4,096 arithmetic parity vectors pass.
The separate image passes final routed timing at 60.68 MHz against 50 MHz.

The image has not been loaded. This is the host-input side of arbitration; the
scheduler request source, now-tested trajectory memory, timestamp stream and Rust
capture adapter remain to connect before autonomous hardware commissioning.
See [frozen image, source and evidence](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/buffered-bridge/README.md).

## Device-clock scheduler core

The FPGA sequencer core now schedules selected-motor telemetry, the existing
compiled feedback controller and torque/PWM audits within fixed device-clock
frames. Fifteen cycle-level cases and Gowin synthesis pass. Lost supervision,
missed deadlines, audit errors and evidence backpressure stop the sequence;
interrupted transactions and stop transmission retain separate timestamps.

This core is **not yet connected to bridge_control or loaded onto the board**.
The 40 ms lower period is a provisional target, not measured performance.
Packet arbitration, trajectory storage, timestamp transport/Rust capture,
integrated UART/watchdog testing, place-and-route and hardware commissioning
remain. See [frozen source and evidence](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/autonomous-scheduler/README.md).

## FPGA controller design

The native FPGA view can now fork a captured controller, edit gains and duty,
generate phased mixed-frequency motion with smooth starts/stops, preserve editable
drafts in study saves, simulate and export a plan. New designs retain the measured
timing schedule, source firmware and commissioned duty ceiling. Simulation uses
the same integer control law and motor runtime as measurement prediction.

Design runs preserve per-motor models, failures, tracking and available electrical
outputs; they are explicitly separate from measured-model accuracy. A nine-motor
edited design completed, but all nine still failed simulated tracking gates.
See the [saved design study and native editor](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/design/README.md).

Autonomous timing, per-axis gains and FPGA-specific uncertainty/sensitivity work
remain outstanding, alongside physical and calibrated electrical acceptance.

## Native FPGA review integration

The Rust viewer now imports the new FPGA acquisition format, keeps interrupted
runs unscored, and shows per-motor tracking, target/encoder/PWM plots and measured
voltage/raw-current traces. Replay and own-feedback comparisons use shared Rust
simulation with progress and cancellation; individual motor failures are retained.
The saved study preserves the separate model snapshot for every motor.

Bounded fitting uses original training/validation roles, excludes timing and
incomplete acquisitions, retains rejected attempts, and can evaluate a fitted
family with its per-device deviations on a selected run. Native save/reload and
HTML export include the FPGA evidence. The UI does not claim model acceptance. Verification: 25 existing refinement tests,
5 FPGA evidence tests and the eight-section viewer render/import/cancellation test
passed. The full retained study loaded and exported successfully; a native screenshot
was visually checked.
See the [ready-to-open study and workflow](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/viewer/README.md).

Still outstanding: faster autonomous FPGA scheduling, improved physical accuracy,
per-axis FPGA gains, FPGA-specific sensitivity/robustness
views, calibrated current/power evidence and later loaded-joint/assembly validation.

## Latest: actual FPGA controller and coordinated nine-motor trials

The feedback-to-PWM controller now runs as synthesized FPGA logic, and the same
integer expression graph runs against the shared Rust motor plant. The host still
schedules polling and targets. Both final 7.5% coordinated trials moved all nine
motors, with torque and PWM readbacks audited every frame.

Best small ID4 pilot: **0.197 degrees RMS / 0.615 peak**. Faster nine-motor test:
**0.911–1.361 degrees RMS**, **3.604 worst peak**. These are different trajectories.
The faster pattern fails the original tracking gates on every axis. Its model
prediction RMS is 0.405–0.711 degrees with simulated feedback, and 0.371–1.666
degrees on recorded-drive replay; both fail the 3-count model gate on every motor.
No fitted model or CAD parameters were promoted.

The first 100 ms group loop failed timing; 150 ms passed after adding batch lease
renewal. Low-drive no-motion/one-way responses and rejected gains are retained.
Moving scheduling onto the FPGA, per-axis tuning, stronger motor dynamics,
calibrated electrical measurements and per-axis FPGA controller design remain required.
All motors were left stopped with torque disabled; a separate 161-sample observation
confirmed constant encoder positions on all nine.

See [full FPGA status](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/STATUS.md),
[measured plots](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/report.html),
and [source/evidence reproduction](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/README.md).

## Available now

- The Rust experiments panel has separate controller design, accuracy/coverage,
  sensitivity/fitting, robustness, hardware validation, CAD proposal and electrical/battery views.
- Captured Rust PID and Rhai controllers use the same encoder estimator and
  bounded command calculations in simulation and supervised bench acquisition.
  The controller receives quantized encoder feedback, not exact simulated speed.
- Captured hardware runs can be checked for reproducible controller calculations,
  simulated with their recorded PWM commands, or simulated as a separate closed
  loop using its own feedback. The latter two use the recorded host event times.
- Whole-trial bounded fitting, family parameters and additive device deviations
  use the shared physical plant. A shared calibration-data trait now supports both
  pulse archives and full recorded PWM histories. The latter freeze written duty
  and host timing; reserved runs are evaluated only after fitting. The UI records
  immutable run roles/limits and retains each full dataset with its fitting attempt. Original training and validation roles remain
  separate. Local sensitivity reports expose overlapping parameter effects.
- Controller robustness compares baseline, candidate and editable named model/
  timing scenarios with captured evidence. Failed or cancelled scenarios remain
  visible and cannot produce an optimistically narrowed prediction envelope.
  These are scenario tests, not confidence intervals.
- Rise, onset, sampled overshoot and settling reports use observed time brackets.
  Insufficient travel, dwell or sampling leaves the metric unresolved.
- Fixture snapshots record hardware/CAD associations, artifact hashes, properties,
  coordinate frames, provenance and uncertainty bounds. Unknown properties remain
  null; prior snapshots are retained. Controller coverage includes measured duty,
  displacement, sampled velocity, supply, temperature and cadence, and flags draft
  conditions/trajectories that differ from recorded evidence.
- Saved review files retain recordings, controller runs, predictions, fits,
  sensitivity/robustness results, failures and CAD proposals. Failed fitting
  attempts retain frozen requests and every objective evaluation. Saved controller
  calculations, source recordings, predictions, scores, fitted parameter values
  and scenario envelopes are cross-checked on load. Legacy fit summaries without
  traces are explicitly unverified. HTML exports show controller comparison plots,
  scores and bracketed transient results as well as the captured evidence.
- CAD proposals require explicit motor identity, a source revision, units,
  provenance, uncertainty and scope. Acceptance produces a new physical CAD
  artifact consumed by the common robot model mapping. The original CAD file is
  preserved. Currently mapped properties are resistance, inductance, reciprocal
  motor constants, rotor inertia and no-load current; unsupported/derived properties are listed
  and prevent acceptance.

## Electrical and battery increment

The Rust panel now has an **Electrical & battery** view. Source parameters and
auxiliary load use the shared circuit and battery components; controller-visible
voltage/current channels use declared sampling, quantization and delay. Rust PID
and Rhai share the electrical observation contract, voltage compensation and
sampled protection. Supply current, winding current, draw/return power, energy and
state of charge remain distinct. Physical electrical limits are evaluated
separately from motion tracking and controller protection activity.

Calibrated electrical sidecars retain raw values, conversion, sensor/circuit
location, source hashes, clock evidence, uncertainty and comparison thresholds.
Only synchronized same-location V/A channels produce watts and energy. Comparisons
reject extrapolation and stale/altered evidence. Existing recordings can compare
servo voltage without inventing amps from the uncalibrated current register. The
live bridge explicitly rejects controller plans requiring current channels it
cannot provide. No hardware commands or new physical measurements were made for
this electrical increment.

Two four-second hypothetical battery cases and an actual recorded-voltage
comparison are retained in [battery-scenarios](battery-scenarios/README.md):

| Case | Simulated electrical result | Controller/measurement outcome |
| --- | --- | --- |
| Charged example pack, 0.25 A auxiliary load | 11.949–11.950 V; peak supply 0.2554 A; 12.034 J drawn; declared limits pass | Position tracking fails; no electrical protection activations |
| Low-charge example pack, 0.75 A auxiliary load | 8.537–8.546 V; 25.625 J drawn; voltage and current limits fail | Protection requests zero PWM on all 81 ticks; auxiliary consumption persists |
| Original ID 4 voltage against a constant source | All 60 captured samples report 12.1 V; voltage RMS difference 0 V | Unscored; 0.1 V quantization and unknown absolute error/sample age prevent a claim of precise electrical validation |

The example sensor/current limits, battery values and electronics consumption
are explicit hypotheses. The controller can pass electrical limits while failing
tracking; zero PWM does not disconnect a battery or remove other loads. No model
has been adopted, no acceptance thresholds relaxed and no battery accuracy claimed.

Verification: 33 runtime tests and nine viewer tests passed across the relevant
suites and focused follow-up checks. The electrical tests cover load sag, charge
balance, distinct PWM supply/winding currents, signed energy, declared sensor
availability, voltage compensation above/below nominal, sampled protection,
calibrated synchronized comparisons and immutable saved evidence. Native and
headless builds pass; current-code reload/export of the battery review passes.
The native electrical panel was captured and visually inspected; voltage axes
show the plotted range so small droop remains visible.
Existing motor/Jacobian/loss-model tests remain recorded in the preceding increment.

Remaining electrical work: a calibrated current-sensor/logging adapter and real
V/A/W data; battery discharge and relaxation calibration; winding/pack thermal and
BMS behavior; explicit wiring/distribution and multiple actuators sharing a source;
and fitting electrical traces with properly separated training/validation data.
Current-source auxiliary load is a scenario approximation, not a validated
nine-motor load. Power scenario changes are explicitly unmapped in CAD proposals
until battery/wiring/fixture ownership is resolved; they cannot silently become
actuator parameters. Voltage is predicted by the selected source, not replayed as
a measured waveform. The CAD robot battery-reporting error identified during the
first electrical increment is corrected below. See the scenario README for
commands and remaining limits.

### Shared CAD robot source accounting

`PhysicalRobot` now captures signed V/A/W at the actual shared battery terminals,
including initial and subsequent sample times. Battery `min_voltage` is the
minimum retained value; `energy_j` is net battery energy drawn, with separate
consumed/returned energy and charge. Reports declare `accounting_version: 2`,
measurement location, the maximum sample interval, state-of-charge validity and
whether the CAD cutoff was crossed. No unmodeled BMS disconnect is implied.
Out-of-range state of charge stops the physical host with the invalid sample
retained. Rewinding only the underlying runtime invalidates battery reporting
instead of combining histories. The signed integration primitive is shared with
bench and calibrated-measurement comparisons; motor winding energy stays separate.
Older report files are not rewritten and must not be treated as terminal-energy
measurements. The changed contract is documented in `cad/PHYSICAL_MODEL.md`.

A synthetic pair of motor/driver branches sharing a source and shaft passes
current-balance, source IR drop, charge balance, voltage recovery and distinct
source/winding-energy checks. Opposed 30% PWM for 10 ms, then zero PWM, produces
minimum source voltage 5.1840 V, final 5.3000 V and 6.9408 mJ net energy drawn.
The captured controller command stops at 10 ms; the first committed zero-current
sample follows at 10.1 ms, within the declared 0.1 ms sampling interval. An initial
11 ms pulse caused by a floating-point boundary check is retained as rejected
timing evidence, and explicit command-edge/sample-bracket checks prevent that
error from recurring. Source accounting also converges when the step is halved and is unchanged by
host-call chunking. These are numerical/analytic checks, not physical calibration
of the bench or the quadruped. Four new source tests, 27 controller/electrical
regressions and five episode/replay tests pass. Native viewer and headless builds
pass. See [the source report](battery-scenarios/two-motor-source-report.json) and
[plot](battery-scenarios/two-motor-source.png).

The inspected current quadruped capture contains 12 motor definitions and no CAD
battery definition. That capture is not the nine-motor bench topology. A battery
and wiring hypothesis must be explicitly declared before testing its shared-load
behavior. The optimized embedded motor path also retains imposed supply
boundaries; this report correction does not silently add a battery to that path.
Real multi-actuator loading, pack calibration, sensor availability and assembly
accuracy remain unvalidated.

## Fresh physical evidence

See [the bench record](../actuators/hx30hm/hardware/2026-09-13-controller-refinement/README.md).

- Reloaded the supervised Tang Primer 25K SRAM image with explicit user
  authorization. Read back the supervisor profile and all nine motor states.
- Refreshed low-drive command-loss, telemetry-loss and host-traffic-loss watchdog
  tests. Each verified autonomous zero PWM, torque off and a stationary tail.
- Collected 108 individual onset pulses: nine motors, both directions, 2.5%/5%
  PWM and three repetitions. Repetitions 0–1 were assigned training and repetition
  2 validation before acquisition. All samples and failed initial connection
  attempts are retained.
- Collected a zero-action cadence run and one three-second position-controller
  run per motor at a nominal 50 ms period, limited to 5% PWM. Every run captured
  the actual observation, controller and command timing. All nine runs completed
  and all motors were verified stopped with torque disabled afterward.
- Imported the fresh onset data and controller recordings into a portable study.
  Motor 4's initial closed-loop prediction error was approximately 0.0077 rad RMS,
  exceeding the provisional 3-count threshold. All nine baseline closed-loop
  predictions fail that threshold (approximately 0.0072–0.0323 rad RMS). This is evidence of a model gap,
  not an accepted calibration. The measured controller also has substantial
  residual tracking error near low-duty breakaway.


## Frozen candidate and fresh validation

A bounded one-parameter fit used eight original motor 4 training pulses and four
reserved repetitions. It changed the effective no-load-current loss parameter
from 0.1000 to 0.07763 A. That parameter is a model hypothesis, not independently
measured winding current or uniquely identified friction. The original held-out
results had already been viewed, so that fit is marked validation-influenced.
It improved the 5% held-out pulse predictions and slightly worsened 2.5% predictions.

Before collecting additional measurements, the candidate, three repetitions,
negative/positive reversal trajectory and unchanged comparison limits were frozen
in `reserved-id4-reversal-protocol.json`. Each four-second run acquired 80 frames
with 5% maximum PWM. All three completed, and all nine motors were verified at
zero PWM, torque off and stationary afterward.

| New reserved repetition | Baseline closed-loop RMS | Frozen candidate RMS | Outcome |
| --- | --- | --- | --- |
| 0 | 0.009433 rad | 0.006255 rad | Fail |
| 1 | 0.011681 rad | 0.008066 rad | Fail |
| 2 | 0.009744 rad | 0.006143 rad | Fail |

The RMS limit remains 0.004602 rad (three encoder counts), with a separate final
absolute error limit of five counts. Recorded-command replay also fails all three.
The motors were warmer than during initial acquisition (motor 4 approximately
50–51 °C, supply approximately 12.1 V). This evidence does not isolate temperature,
starting-angle dependence or repeatability. The candidate is not accepted into CAD.

Open `reserved-id4-fitted-review.json` for the combined 108 pulse trials, twelve
physical controller recordings and baseline/candidate comparisons. Its HTML report
and `reserved-id4-results-summary.json` retain the numerical results.


## Full-command fitting and release discrimination

The `CalibrationData` trait supplies immutable observations, roles, resolution
and shared-runtime predictions. Pulse and full-controller command datasets use
the same bounded fitter; no second plant or optimizer was added. Recording fits
retain the complete source dataset and reject changed roles, limits or sources.
A test traces every prediction request and verifies that reserved recordings
enter only the final baseline/candidate comparison.

`recorded-command-fit-review.json` fits motor 4's original return trajectory with
an effective loss and constant load-offset hypothesis (18 objective evaluations).
Training command-replay RMS decreased from 0.005657 to 0.005084 rad, while all three
reserved reversal command-replay errors increased. Separate own-feedback
predictions still fail those reserved trajectories. This candidate is not adopted.
All those measurements had already been inspected, so the fit is explicitly
validation-influenced.

Twelve additional release trials now compare signed 5% PWM for 200 ms followed by
zero PWM or torque off, three repetitions of each direction/mode. The original
plan reserved repetition 2. The importer policy was normalized afterward and is
conservatively labelled validation-influenced while preserving those roles.
All acquired samples, actual release windows and verified stops are retained.
Zero PWM produced 3 counts of net travel from the last pre-release sample;
torque-off produced 4–5 counts. These are observed offsets, not exact command-time
stopping distances. All nine motors were verified stationary, PWM zero, torque off
and supervisor disarmed after acquisition; supply power remains on.

The reusable `robot.switchable_h_bridge` shares the existing driven equations and
adds explicit enable, passive leakage and bidirectional diode return paths.
Tests check terminal currents, passive energy balance, identical driven response,
continued coasting versus braking, and timestep convergence. Imported release
trials preserve their typed driver hypothesis in the portable archive and use
the same physical replay/fitting path. Plateau metrics are unresolved when prior
motion, sampling or stationary dwell is insufficient.


A follow-up release fit changed both loss and rotor inertia, using eight training
release trials and four reserved repetitions with the original limits. Three of
four reserved release trials pass; the remaining RMS is 0.004863 rad versus a
0.004602 rad limit. The inertia deviation reached its upper bound (+2e-7 kg·m²),
so it does not identify the true inertia. The same candidate still fails all four
captured controller comparisons. `release-fit-controller-review.json` retains
that transfer failure alongside the improved release fit. No candidate is adopted.

The release study and both candidate/controller studies have portable JSON and
HTML reports. The native release viewer was rendered and checked; it visibly
labels zero-PWM braking as a hypothesis and shows the measured/model gap.

## Combined fitting across behaviors

The Rust fitter, viewer and batch workflow now accept pulse, release and full
recorded-command datasets together. Each fit freezes the complete source archives,
recordings, original roles, limits and request. The shared `CalibrationData` trait
routes predictions through their original runners. Duplicate trial identities are
rejected. Reserved trials enter only the final baseline/candidate comparison;
there is no second physics implementation or optimizer.

The Fitting view includes a combined-evidence action, an optional additional saved
study, per-trial outcomes, retained failures and explicit candidate selection.
Each whole trial is scaled by encoder resolution and sample count; more repeated
trials still contribute more total weight. CLI equivalents are `fit-combined` and
`evaluate-combined-fit`; HTML exports retain mixed-source fitting results.

`combined-loss-inertia-fit-review.json` retains a 30-evaluation loss/inertia fit
with 17 tuning and 11 already inspected validation trials. It passes all eight
reserved onset/release trials and fails all three reserved recorded-command runs.
Independent closed-loop comparisons also fail all four captured controller runs;
reserved RMS errors are 0.006285, 0.007679 and 0.005931 rad. The original run is
below the RMS limit but fails the separate final-error limit (six counts versus
five). Effective inertia is 2.6714e-9 kg·m², near the lower search boundary, and is
not an identified physical value. The solver reached its evaluation budget.

`combined-fit-controller-review.json` and its HTML report preserve these results.
No candidate was adopted and no limits were relaxed. The next physics investigation
should explain the sustained low-duty/reversal mismatch across all these behaviors;
passing the short pulses alone is now explicitly insufficient.

A fresh read-only supervisor check is saved in `current-supervisor-inspection/`.
The supervised image is already active. All nine IDs report PWM zero, torque off
and speed zero, with no armed motors. Reported supply is still on; this is not a
physical power-isolation claim. No reload or motor motion was needed for this
software increment.

## Low-speed loss refinement

The shared motor registry now exposes `loss_speed_scale` (rotor rad/s), retaining
5 rad/s when omitted. Both residuals and derivatives use this explicit smooth-loss
width. It reduces modeled creep when made narrower; it is not true static friction
or a measured breakaway threshold. The viewer can expose optional registry values
without changing an older baseline, and the fitter resolves those same defaults.

A mixed-source fit adds the 0.02–5 rad/s transition coordinate to the original
loss/inertia bounds, retaining 17 tuning and 11 inspected validation trials. After
50 evaluations it reaches the 0.02 lower transition bound, with effective loss
current 0.060036 A and inertia 1.94454e-7 kg·m². All 17 training trials and eight
reserved pulses/releases pass. Two of four independent controller predictions pass,
compared with zero for the preceding candidate. The three reserved controller RMS
errors are 0.004740, 0.005936 and 0.003974 rad; only the last passes both limits.

All three reserved recorded-command replays still fail (RMS 0.012792, 0.024313 and
0.015815 rad). One worsens. This is still an unaccepted model hypothesis, with no
new hardware measurements or CAD adoption. `loss-transition-controller-review.json`
and its HTML report retain the full comparison; the earlier candidates remain.

A reviewed proposal can introduce `electrical.loss_speed_scale` into CAD and the
shared robot runtime. The new field uses an explicit "not declared" prior value
in proposal version 2; existing numeric version-1 changes remain supported. Only
this declared model-regularization field can be introduced this way. Missing
resistance, inertia or other physical properties are not silently supplied.
Synthetic acceptance tests verify source preservation and shared-model propagation.

## Verification performed

- Shared controller/estimator tests: cyclic encoder handling, held/stale feedback,
  bounded PID anti-windup, Rust/Rhai agreement, deterministic physical simulation,
  cancellation and distinct command-replay/closed-loop predictions.
- Synthetic fitting recovery, held-out leakage rejection and sensitivity checks.
- Saved-evidence tests reject modified controller commands, mismatched traces,
  altered scenario inputs and missing setup references. Tests distinguish unknown
  setup properties from measured values and flag unseen trajectories.
- The combined saved study successfully passes the strengthened load validation:
  twelve controller recordings, 34 predictions and one fit. The HTML export
  includes model-prediction plots, separate tracking scores and transient reports.
- CAD proposal source preservation, stale-source rejection and propagation into
  the common motor parameter mapping.
- Fresh sweep import preserves all samples, nine hardware identities and the
  72/36 training/validation split.
- Existing comparison and physical-study regression tests pass.
- All six new UI views render without mutating the saved review. The native viewer
  was rendered and visually checked against the combined nine-motor review; a
  repeated widget identifier was fixed and plot axes added. An initial
  headless-test texture cleanup error was corrected and the test rerun passed.
- The bench controller protocol double verifies bounded command execution and
  fault cleanup. The FPGA safety, UART supervision and released-TX simulations
  pass. Physical watchdog evidence is separately retained.

Previous increment verification: **38 tests pass** (17 controller/refinement, 2 comparison,
4 physical-study, 9 viewer, 5 existing motor tests and 1 passive-bridge test).
The supervised acquisition-protocol test also passed in the preceding turn. The native viewer
was rebuilt and visually checked using the combined review. The fixture-editor
compile error encountered during development is retained beside the corrected
build/check logs. Actual hardware measurements are separate from protocol tests.

## Remaining acceptance work

1. Refine the actuator/driver hypothesis beyond a single smooth Coulomb-loss law.
   Making its transition explicit improved training and some independent-controller
   predictions, but all reserved plant replays still fail. Investigate distinct
   breakaway/moving friction and reversal behavior against the shared friction
   components, while retaining command/sensor timing uncertainty. Do not attribute
   all remaining error to friction without distinguishing measurements.
2. Use the implemented combined objective to find a candidate that passes onset,
   release and controller comparisons together. Preserve single-behavior failures
   and compare the independently simulated feedback loop separately. Passive diode/
   leakage parameters remain explicit hypotheses and must not be presented as
   calibrated device electronics.
3. Reserve additional trajectories and repeated condition measurements before
   fitting the next candidate. Existing warm measurements do not establish thermal
   dependence, unique motor constants or generalization to other starting angles.
4. Extend CAD property mappings for declared driver and transmission parameters,
   and connect accepted proposals to the CAD application's own editing/undo flow.
   Exporting a reviewed physical-model artifact works; editing a live CAD document
   is not implemented here.
5. Populate fixture geometry, attached inertia and independent sensor/electrical
   uncertainty with actual measurements. The capture UI now supports these facts;
   it does not manufacture them. Device sample age, winding current accuracy and
   detailed proprietary PWM behavior remain unestablished.
6. Validate known-load joints, legs and the quadruped when those physical setups
   are available. Unloaded bench evidence cannot validate those conditions.

Combined-evidence increment verification: **33 tests pass** (18 controller/refinement,
2 comparison, 4 physical-study and 9 viewer). The new test checks routing across
pulse/release/controller sources, exclusion of every reserved behavior from
optimizer calls, duplicate IDs, snapshot round-tripping and tamper rejection.
The native viewer and batch CLI build successfully.

Low-speed-loss increment verification: **48 tests pass** (5 motor, 8 Jacobian,
20 controller/refinement, 2 comparison, 4 physical-study and 9 viewer). Tests cover
legacy/default parity, low-drive analytic equilibrium and timestep consistency,
smooth derivatives and the zero-power heat cusp, new CAD-field adoption and legacy
review rendering. The initial overly strict cusp derivative test is retained beside
the corrected one-sided check. The native viewer and batch CLI build successfully.

The goal stays active. Passing software checks and producing simulated traces
do not establish the requested sim-to-real accuracy.
