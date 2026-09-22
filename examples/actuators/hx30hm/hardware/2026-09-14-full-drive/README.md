# Full-drive nine-motor experiments — 3 A / 3.5 A comparison completed

The user authorized raising the drive ceiling to **1000/1000 (100%)** and
stress-testing all nine powered bench motors with fast direction switching.
**No full-drive hardware capture has been made in this work yet.** The connected
FPGA now has the frozen 100% experiment image loaded into volatile SRAM.
The user confirmed motor power off before loading and then confirmed power on.
All-nine inspection, low-drive watchdog tests and zero-drive onboard recording pass.
The physical S2 test remains pending before increasing drive.

## Live load and connection verification

Following the user's power-off confirmation, `openFPGALoader` loaded the exact
SHA-256-verified image into SRAM and exited successfully. No persistent flash was
written. At 1 Mbaud the FPGA returned valid checksummed supervisor and experiment
replies: 50 MHz, autonomous fixed-gain capability 7, no armed motors, and the
expected thermal/voltage/raw-current thresholds. Initial reason 9 (S2) changed to
reason 10 after explicit STOP and remained there on subsequent reads. This shows
the S2 input was not continuously asserted during those checks; it does not prove
the cause of the initial event or constitute a physical S2 motion-stop test.
After power-on confirmation, the live checks below were completed.

## Extended display-observation test: 30 seconds completed

The user requested enough duration to read the supply display. A separate 2-second
25% pilot passed, followed by three complete 10-second all-nine 25% reversal
blocks (`extended-250-30s-block-01/` through `03/`). Each block stops, verifies
stationary shafts/PWM zero/torque off, and rearms before the next. Total commanded
time is 30 seconds, not one uninterrupted 30-second run. The original FPGA
finite-run and upload bounds remain intact. All 750 frames and 6,750 motor audits
complete, with no faults, minimum voltage 10.8 V and maximum temperature 49 °C.
The shared Rust motion reviews and `extended-30s-summary.json` retain details.

During these 25% blocks the user reported approximately **0.86 A** on the supply
and that it **stays in CV**. This argues against sustained supply current limiting
at this test level. It does not rule out brief transients, voltage telemetry
calibration/sample-age errors, or power-distribution loss; it does not establish
the cause of the earlier 50% dip. The observation is saved separately as a user
report, not synchronized logged current. No model fit or current-rating acceptance
was made. The 8-second plan was superseded before execution by the user's request.

## Latest higher-stress trial: 50% at 3.5 A

`stage-500-3p5a-01/` completes the short 200 ms, five-frame all-nine ±50% reversal
sequence at the user-reported 12.6 V / 3.5 A setting. All 45 motor PWM audits match
the earlier 3 A trial exactly. Minimum reported motor voltage remains **9.1 V at
both settings**, only 0.1 V above the 9.0 V trip. Maximum sampled temperature is
48 °C. Every motor is verified stationary with PWM zero and torque off afterward.
Higher-drive escalation remains held; this does not identify the cause of sag.
The comparison is retained in `current-limit-comparison-50pct.json`.

The shared Rust motion analyzer reviews both 25% and both 50% trials using the
same encoder estimator. On the latest 50% run, per-motor peak interval-average
speeds range from 74.7 to 90.1 degrees/s across roughly 40 ms observation intervals.
These are neither continuous maximum speeds nor exact braking/reversal times.
Each capture's `motion-review.json` retains the estimator windows and uncertainty.
No motor model fit or promotion was performed; these are diagnostic timing trials.

## Latest: powered reload and completed 3.5 A comparison

The user removed the motor-power-off prerequisite and authorized FPGA reloads as
needed. `servo/safety.md` and `COMMISSIONING.md` now reflect that instruction.
The exact frozen image was loaded into SRAM with motor supply power reported on;
no persistent flash was written. Live profile verification passed. All three
small-drive watchdog tests and all-nine zero-drive recording then passed again.
The prior successful S2 test applies to the same unchanged image and wiring.

`stage-250-3p5a-02/` completes the unchanged five-frame all-nine 25% reversal plan.
All 45 motor PWM audits match the original 3 A trial exactly. Minimum reported
voltage is **11.0 V at the reported 3.5 A setting versus 10.9 V at 3 A**. This is
one telemetry increment, with one trial at each setting and different motor
temperatures (47 °C versus 49 °C maximum), so it does not establish a clear sag
improvement or isolate supply limiting from power-distribution loss. All nine
motors are verified PWM zero, torque off and speed zero after the run.
`current-limit-comparison.json` contains the comparison and limitations. No higher
PWM trial or model fit was performed in this comparison. The earlier failed
connection attempt remains retained below; its requested reload is now complete.

## 3.5 A comparison attempt — connection failure before motion

The user increased the supply limit after the proposed 3.0→3.5 A diagnostic.
`stage-250-3p5a-01/` retains the unchanged 25% trial attempt. It stopped at the
initial supervisor STOP transaction because no FPGA reply arrived. No plan was
uploaded, no motor was armed, and no torque-enable or motion command was issued.
Cleanup could not verify hardware state either; the attempt is incomplete and
must not be scored as a voltage/current comparison. Read-only STATUS probes at
both 1 Mbaud and 115200 also received no reply. Read-only JTAG detection is saved.
A USB reconnection and loss of the volatile image are suspected; power-off
confirmation is required before reloading the frozen image. No reload occurred.

## Latest physical motion and supply limitation

The actual S2 motion-stop test (`physical-s2-01/`) passed after observed encoder
motion, with reason 9, autonomous zero/off and stationary tail before cleanup.
Explicit all-nine rearm and a zero-drive device capture subsequently passed.
All-nine short reversals then completed at 25% and 50% with exact positive and
negative PWM readbacks, five complete frames each, and all-nine stop verification.
Minimum motor voltage fell to 10.9 V at 25% and 9.1 V at 50%; maximum observed
motor temperature was 47 °C. Higher-drive stages are held because the latter is
only 0.1 V above the configured undervoltage trip.

The user confirms a WANPTEK DPS3010U set to 12.6 V / 3 A and all nine motors
powered through one daisy chain. The manufacturer rates the supply at 10 A
maximum; 3 A is the reported setting, not its hardware maximum. The current
ratings of the complete daisy-chain power path remain unverified.
Actual current and supply CV/CC state
remain unmeasured: the voltage drop does not isolate current limiting from wiring
loss. Historical September 11 notes report a WANPTEK DPS3010U, a daisy chain, and
CV indication during motor-voltage sag; the user has now confirmed that model and distribution. No limit or wiring
change has been made. The FPGA 100% image remains loaded. No new model has yet been fitted.
Two 25% training/validation plans are prepared but have not been executed.

## Powered hardware checks

- `powered-inspection-01/` retains an acquisition setup failure: macOS `stty`
  rejected 1 Mbaud before any motor transaction. The Rust adapter now uses the
  documented macOS IOSSIOSPEED_32 ioctl after configuring raw mode. The subsequent
  `powered-inspection-02/` passes for all nine motors: PWM zero, torque off, speed
  zero, 41–45 °C and 12.1–12.4 V. No firmware change/reload was needed.
- `watchdog-commissioning-01/` passes all three actual 5% ID12 trials. Missing
  command heartbeat caused reason 8; missing feedback and absence of all host
  traffic caused reason 7. Each verifies automatic zero PWM, torque off, at least
  150 ms of stationary tail, persistent latch, and explicit rearm between tests.
  All-nine final cleanup passes. These tests withhold traffic; they do not
  physically disconnect a cable or verify S2.
- `device-zero-01/` completes 12 onboard frames and 108 motor telemetry/PWM audits.
  Motor4 request intervals are 39.9981 ms for the first interval and 40.0000 ms for
  the remaining ten. Final all-nine physical stop checks pass. This proves only
  the observed zero-drive cadence, not full-drive timing or motor model accuracy.
- `physical-s2.json` is ready but has not been executed. The operator must be
  ready to press S2 after ID12 starts a small 5% oscillation. It is bounded to
  eight seconds and 60 encoder counts of travel, then stops on timeout/fault.
  Acceptance requires observed encoder motion before S2, reason 9, autonomous
  zero/off and stationary tail before cleanup. A premature press is not accepted
  as a motion-stop test. Host source changes are frozen under `commissioning-01/`.

## Implemented, pending physical commissioning

- Explicit `fpga_device_pd` plans allow 0–1000 PWM at a 40 ms device-clock period.
  The slower host-scheduled controller retains its 100/1000 bound.
- The integrated FPGA owns sealed trajectory playback, all-nine telemetry,
  compiled shared Rust controller arithmetic, PWM transmission, torque/PWM
  readback and event timestamps. The host records data and renews a separate lease.
- The full-drive build specializes the shared controller to Kp=4096, Kd=0,
  Kv=4096 in Q8 notation. Upload/START checks reject different gains. This is
  an experimental controller configuration, not an accepted tuned controller.
- The Rust transport retains fragmented/coalesced UART frames and raw USB chunks.
  Device events convert to the existing simulation/fit recording format, but
  conversion cannot certify physical stopping or make a synthetic run scoreable.
- Raw current remains uncalibrated. Servo voltage does not identify total supply
  current, power, or battery/source impedance. Supply rating/current limit has
  been requested and remains unknown in this session.

## Verification evidence

`verification/case0..5-capture.json` and matching UART hex/log files contain
**synthetic stationary motor replies generated by the RTL testbench**. They are
not physical measurements and must never be fitted as motor characterization.
The full UART tests exercise successful all-nine +1000 and -1000 drive, missing
feedback, expired independent host lease, operator STOP, incorrect PWM audit,
and host backpressure. Each ends with zero PWM and torque off in the emulator.
A watchdog diagnostic overwrite was found and corrected: an already latched
supervisor no longer receives a redundant experiment STOP that replaces its
original reason.

The complete image initially exceeded FPGA capacity. The final source uses the
same generated controller with fixed gains, compact run-relative timestamps,
byte-addressed packet storage, serialized controller packet writes, and validated
nine-entry SYNC batches. Full-ID validation and supervisory limits remain intact.
The no-ALU mapping uses 17,308 LUTs (75% of the device) and passes final routing/timing at 50 MHz (reported maximum 60.93 MHz).
`make bridge-experiment-lut` completed successfully, including bitstream packing.
The exact image is `build/bridge_experiment_100pct.fs`; source, mapped/routed
netlists, logs, image hashes and tool versions are retained. Failed build attempts
are under `diagnostics/`. The frozen image has now been loaded successfully; see `commissioning-01/`.

Final software checks pass: six complete UART scenarios, 24 Rust controller/event/
upload tests, the acquisition mock with both valid and corrupt streams, controller
arithmetic parity, invalid-ID isolation, SYNC bounds, safety and buffered-controller
regressions. The acquisition mock verifies cleanup and result labeling, not real
motor behavior. Exact final source files are archived in `sources.tar.gz` with
per-file SHA-256 hashes in `source-files.sha256.json`.

The read-only live inspection in `read-only-inspection-01/` found all nine motors
reporting zero PWM, torque off and zero speed, with 44–50 °C temperature and
12.1–12.4 V telemetry. It did not issue motion commands.

## Remaining work

1. Coordinate and run the physical S2 motion-stop test; confirm button release.
2. Advance all-nine short reversal plans through 25%, 50%, 75% and 100% based on
   each preceding capture and observed stopping. Retain failures and limits.
3. Measure individual and simultaneous speed, acceleration, braking/reversal,
   saturation, voltage, temperature and raw current. Report timing resolution and
   unresolved steady-speed tails honestly.
4. Fit separate per-motor models from training data; freeze candidates before
   fresh all-nine validation with unchanged accuracy gates. No full-drive
   measurements or new model improvement have been obtained yet.

The previous 7.5–10% physical dataset and fitted candidate remain frozen in
[unloaded refinement](../2026-09-14-unloaded-refinement/README.md).
