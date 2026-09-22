# Printed leg: supervised travel teaching

Operator mapping: ID 1 knee, ID 2 worm, ID 3 belt/hip. All three are attached
to the printed leg, clamped and supported so torque-off cannot drop it (operator
confirmation, 2026-09-21). ID 3 was changed from factory ID 1; a read-only scan
verified ID 3 after the operator cycled the servo supply.

Start the existing simulator plus calibration panel:

```sh
cargo build --locked -p sim-runtime --example serve_actuator_calibration
target/debug/examples/serve_actuator_calibration \
  examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json 4194
```

Open http://127.0.0.1:4194. The viewer uses the preserved detailed Rust/WASM
bundle; the panel commands real hardware separately. Simulation Play/WASD do
not command the physical leg. No serial port opens and no motor command is
issued merely by loading this page. One service serializes hardware access;
macOS exclusive-open and an advisory descriptor lock protect ownership.

Read connected motors explicitly re-establishes the stopped serial session.
Select one motor, confirm physical support, and enable teaching. Enable runs
telemetry-loss and command-loss watchdog checks at zero drive. PWM is an editable
0–100% field in 0.1% increments, matching raw values 0–1000. Step is an editable
integer count (1–4095); the resulting target must remain within non-wrapping
encoder and taught working bounds. Neither field issues motion by itself.

Click Toward upper/lower for one step. Hold for 300 ms to begin repeating;
each repeat waits for the preceding jog and verified stop, with no queued target
backlog. Releasing a hold cancels its current jog and requests verified torque-off
without discarding teaching enable. Stop, focus loss, hidden tab, panel close,
and Escape disable teaching. The Rust worker owns serial I/O and cancellation;
the FPGA independently enforces feedback and command deadlines.

Each jog terminates at target, an 80 ms host attempt cap above 2.5% PWM (300 ms
otherwise), a fault, or release/Stop. PWM is operator-selected and never escalates
automatically. It is not a calibrated torque limit. Actual movement can stop
short or coast beyond the requested position. If the final position exceeds the
commanded envelope, teaching is disabled until the operator inspects and enables
again. Stop is not an active position brake; higher PWM can increase coasting.

Lower/upper/reference buttons read the stationary physical encoder and save
versioned JSON under `measurements/`. They never infer an endpoint from stall.
Upper/lower name part poses, not increasing/decreasing encoder values. Choose
mounting direction before teaching the first limit; two taught poses establish
the encoder polarity automatically. The working envelope is four counts inside
the sorted encoder endpoints, including reversed mounting.
Recovery inward from a newly captured edge is allowed. Historical captures
are retained. Download exports the current record. The calibration remains
provisional: it is not automatic homing, a joint-angle calibration, a structural
qualification, or a coupled-leg collision envelope. A power-cycle, assembly
change, belt slip or ambiguous encoder wrap requires renewed physical reference
verification. Values are not silently promoted into CAD.

## Continuous traversal of taught poses

Open **Saved poses / edit/reset** to capture replacements, clear either endpoint,
or reset both bounds for the selected axis. Clearing stops the hardware first,
disables teaching, preserves the mounting direction/reference, and saves the
previous calibration in immutable history. No calibration is deleted on reload.

Enable an axis, then **Start slow sweep**. Every start requests **5 motor encoder
counts/s** (about 0.44 motor degrees/s; joint gearing is not assumed). The worker
runs one continuous PWM-feedback session between inset poses; it does not call
or queue the single-step jog operation. Speed can be changed live with half/double
buttons or a numeric field (0.1–500 counts/s). Speed reductions decelerate through
the shared reference governor. The independent PWM ceiling is 0–100%; zero ends
the sweep. Each sweep starts explicitly and never resumes after Stop or a fault.

Default turnaround clearance is 64 motor counts inside each taught pose. It is
an explicit commissioning margin, not a measured stopping-distance guarantee.
The range must be wider than twice that clearance plus eight counts. The
controller follows the shared Rust reference governor and bounded PID, with
provisional gains and tuning declared in `server.json`. It checks measured
position, following error, time between feedbacks, device health, and prolonged
lack of progress at the selected PWM ceiling. Real friction can prevent a smooth
crawl; a requested rate is not a guarantee of measured speed. The live plot and
readout show actual velocity, tracking error, effort, and reversals.

The browser sends a run-specific heartbeat/update every 100 ms. An expired
750 ms lease cannot be revived; ownership, axis, run ID, and sequence are checked.
Focus loss, hidden tab, close, Reset bounds, and Stop terminate motion. The
independent FPGA command/feedback deadlines remain active. Continuous serial
errors issue STOP; no active-drive command is retried. All serial activity stays
on the existing worker and transport. Only the stopped-state verifier retries.

Validation includes both mounting orientations, a synthetic first-order lag
plant (not a fitted physical leg), acceleration-limited speed changes, a fake
serial endpoint checking one torque-enable session, lost-ACK shutdown without
redrive, browser heartbeat/Stop/reset controls, and stale/foreign/expired leases.
The first nonzero continuous loaded-leg sweep remains an operator trial.

## FPGA profile

Source is in the hardware repository's `servo/src/hx_safety.v` and `bridge.v`.
`make -C servo bridge-calibration` produces a separate SRAM image, preserving
the existing 4–12 profiles. The calibration image covers only IDs 1–3 and
identifies itself with status version 4. It starts latched and broadcasts zero
PWM and torque-off. An explicit valid per-jog encoder envelope is required
before arming. Only one motor may arm at a time. PWM magnitude is constrained to the device range 0–1000;
position, arbitrary configuration, sync and unknown-ID writes are blocked.
Feedback outside the window trips; outward PWM at its boundary is rejected.
Telemetry and command watchdogs remain 200 ms and 300 ms, respectively; S2 and
host STOP latch the bridge. Serial-based stopping cannot cut motor supply power
or stop an actuator when its signal connection is broken.

The window is checked on solicited telemetry, not an independent joint sensor.
These intervals and margins do not establish a measured stopping distance.
Other axes are torque-off during teaching and must be mechanically supported.

## Receive-fault recovery and direction display

The 19:35:09 belt jog reached its target and reported torque-off and zero PWM;
a subsequent stop-verification telemetry reply was truncated to 14 of 21 bytes.
The code had shortened the FPGA reply protection from 2 ms to 20 bit periods
after the first received byte, allowing a repeated STOP broadcast to collide
with a gapped reply. `bridge_reply_gap_tb.v` reproduces this failure in the
previous bridge and passes with the fix. This is a demonstrated firmware defect
consistent with the trace, not proof excluding wiring or supply disturbances.
The original host timestamps do not resolve servo-bus inter-byte timing.

The serial reader records request and timeout details. Stop verification may
retry twice, each time issuing STOP, logging retained bytes, waiting for a quiet
receive stream, and obtaining fresh torque/PWM/stationary readbacks. This
recovery never repeats a nonzero drive command or rearms. Active-drive reply
loss still cuts drive and disables teaching; persistent readback loss remains
an unverified-stop error. A timeout never becomes a taught mechanical limit.

The reusable `ActuatorMotionView` plots requested versus measured encoder change
and labels commanded/measured part direction. Samples come from the Rust jog
receipt, including final stopped position. The plot updates after each step;
it does not interpolate missing measurements as physical observations. Once both
poses exist, the panel reports normalized lower-to-upper part travel. The 3D
robot remains a separate simulation until joint binding, gearing, reference
angle and mounting polarity have been physically established.

## CAD promotion

Retain `server.json`, calibration versions, serial trace, and physical setup
photos/measurements with this artifact. Bind the confirmed encoder zero,
polarity, transmission ratio and uncertainty to the specific CAD joint/actuator.
Check clearances at combinations of all three joint positions before promoting
accepted joint limits to CAD. This first UI intentionally records motor-space
measurements without inventing those missing joint properties.

## Checks

- Rust calibration policy and serial command-order/fault regression tests:
  `cargo test --locked -p sim-runtime --lib acquisition::calibration`
- Browser UI test with synthetic telemetry and no hardware:
  `node web/tests/calibration-ui.mjs`
- Existing FPGA supervisor and UART tests: `make -C servo sim-safety`.
- `make -C servo sim-calibration`: target envelopes, PWM device range, IDs, command
  gating, watchdog and S2. The routed image passes 50 MHz timing.
- Physical receipts and limitations are recorded in `STATUS.md`.

## Operator-requested 10% drive revision

The operator reported no visible movement, and traces showed no worm encoder
change at 25/1000 PWM. The operator requested 10% and confirmed the worm was
clear of stops and obvious binding in the commanded direction. Version 3 of
the separate FPGA profile allows at most 100/1000 PWM; UI selection is explicit
and there is no automatic drive increase. Above 25/1000, the host drive attempt
is limited to 80 ms, with unchanged FPGA watchdogs and narrow travel windows.
The UI reports requested/actual counts and target-reached versus timeout.
A no-motion timeout does not diagnose friction versus binding.
The earlier 2.5% source/bitstream and physical receipts remain preserved.
