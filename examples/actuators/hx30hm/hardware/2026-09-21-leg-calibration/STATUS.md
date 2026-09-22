# Calibration handoff — 2026-09-21

Implemented: reusable Rust encoder teaching policy and serialized HX transport;
loopback HTTP adapter beside the preserved Rust/WASM robot viewer; finite jogs,
Stop, operator boundary/reference capture, immutable history and JSON export.
Motor mapping supplied by operator: 1 knee, 2 worm, 3 belt/hip.

FPGA calibration profile loaded into SRAM. IDs 1–3 only, one armed axis,
25/1000 PWM ceiling, <=32-count per-jog window, unchanged 200/300 ms watchdogs.
Routed timing passes 50 MHz. Existing supervisor/UART regressions and the new
calibration logic test pass. Five Rust policy/serial regressions pass. Browser
contract test with synthetic telemetry passes. Preserved source hashes and
bitstream are in source-manifest.json and source/.

Physical: ID 3 survived a motor power cycle. All three IDs read correctly after
connection. The latest zero-drive-verification.json records independent missing
feedback/command watchdog checks and verified PWM-zero, torque-off, stationary
encoder readback for every motor. Final service state is stopped, no enabled
axis. No nonzero drive was sent by those verification checks.

The operator exercised the belt-axis jog controls during development. A receive
fault occurred; matched commands are preserved in measurements/serial.jsonl.
The old implementation did not log the leftover bytes, so root cause remains
unresolved. The new implementation logs raw chunks and pending bytes, always
attempts STOP on parser faults, and supports explicit stopped-state reconnect.
Fresh physical zero-drive checks passed after operator power-cycling.

Not established: calibrated joint angles/torque, mechanical stopping distance,
repeatable safe endpoints, full travel, coupled-joint clearances or automatic
homing. No travel limits or reference have been taught yet. The four-count
inward margin is provisional and cannot substitute for physical clearance.

Browser: the extension-controlled Chrome tab hit ERR_BLOCKED_BY_CLIENT with
CDP blockedReason=inspector. A separate local browser test rendered the complete
robot + calibration panel, and the operator accessed and used the panel.
The user should reload the working page after this server restart to obtain its
new control token. No browser security settings were changed.

## 10% operator override — latest handoff

Operator explicitly requested 10% PWM and confirmed the worm is clear of its
stops and obvious binding in the commanded direction. Version 3 of the
calibration SRAM image is loaded: PWM ceiling 100/1000, selectable drive
25/50/75/100 in the UI; above 25 the host attempt duration is capped at 80 ms.
Per-jog encoder window, independent watchdogs, one-axis gating and physical
stop verification remain unchanged. Default step is one count. The result now
reports requested versus actual counts and target-reached/timeout/no-net-motion.
No automatic force increase is implemented.

Seven Rust tests pass, including 10% target tracking and a simulated stall that
returns a timeout with verified torque-off and no drive escalation. Browser
contract and FPGA calibration tests pass; routed clock is 81.81 MHz against
50 MHz required. The version-3 image passes all three motors' zero-drive
watchdog and stop-readback checks in zero-drive-verification-10pct.json.
Physical nonzero-drive performance at 10% remains for the operator to observe.
The previous 2.5% source and receipts are retained separately. Latest source
and bitstream identities are in source-manifest-10pct.json.


## Version 4 — current operator controls and receive fix

The user requested the full PWM range, arbitrary step counts, hold-to-repeat,
part-relative endpoint names, and a visual commanded/measured direction check.
Implemented PWM 0–100% (0.1% resolution), count input 1–4095 with target bounds,
serial repeat after verified stops, release cancellation, reversed pose teaching,
normalized part travel, and reusable measured-motion SVG. The 3D robot's CAD
joint mapping/angle calibration remains unbound; the UI states this explicitly.

Investigated the original timeout: a 21-byte stop-verification reply was
truncated to 14 bytes, after verified torque-off/PWM-zero reads. Fixed a reply
arbitration bug reproduced in a wire-level regression: a repeating STOP could
interrupt a gapped reply when its initial protection budget was shortened.
This demonstrated defect is consistent with the trace; electrical contributors
have not been excluded. Bounded stopped-state retries recover transient reads,
with no nonzero-write retry or rearm. Active communication failure still cuts
drive, but returns its measured jog receipt when stopping verifies, so the UI
does not display the preceding successful jog as though it were current.

Version 4 FPGA SRAM image loaded with motor supplies confirmed OFF by operator.
Route timing 105.69 MHz, passing 50 MHz. Thirteen focused Rust tests, existing
FPGA regressions, new calibration/gapped-reply checks and browser contract tests
pass. Build has an existing unrelated dead-code warning. Native Chrome
Incognito window was reloaded and visibly rendered the robot plus new controls.

Afterward the operator powered the motors and used the controls. Fresh physical
replies identify FPGA profile 4, and stop readback is recorded in
physical-receipts-v4.json. No agent-initiated nonzero jog was used. The operator's
worm step at 40% PWM requested -20 counts (845 -> 825) but stopped at 784:
-61 actual counts, demonstrating substantial overshoot of the short pulse
window. Torque-off and stationary readback verified. Do not weaken the boundary
checks or treat the four-count margin as a qualified stopping distance.

Current server uses the preserved simulator bundle at 127.0.0.1:4194. Each
restart requires browser reload for a fresh token. Calibrated physical limits,
repeatability, joint gearing/zero/polarity binding and loaded tracking remain
operator measurements, not established sim-to-real validation.


Final version-4 hardware check: all three motors passed independent zero-drive
telemetry/command watchdog tests and fresh PWM-zero, torque-off and stationary
readback. See zero-drive-verification-v4.json. Final service state is connected,
no enabled axis; user must explicitly enable before a jog. The agent sent only
inspect/enable-zero-test/stop commands. The latest display build is running and
the existing Incognito browser was refreshed.


## Continuous sweep revision

Added operator-started, feedback-regulated traversal between taught poses,
starting at 5 counts/s, with live speed changes, separate PWM ceiling, explicit
turnaround clearance and Stop. Reuses shared reference governor, encoder
estimate, and PID; the governor now handles a reduced speed request without
violating its acceleration bound. Provisional tuning/provenance is explicit in
server.json; no loaded-leg fit or precise crawling performance is claimed.
The version-4 FPGA profile and bitstream are unchanged.

Clear lower, Clear upper, and Reset both bounds stop first, save previous
calibration history, retain mounting direction/reference, and require explicit
re-enable. The saved lower/upper values for IDs 1–3 were preserved during this
update. Continuous traffic owns one worker session; live control messages carry
an expiring run-specific lease and cannot resume after stopping.

Checks: 19 calibration/transport/controller tests, two reference-governor tests,
one browser-lease server test and both browser suites pass. The serial tests
include continuous speed changes and operator cancellation plus a lost active
ACK that stops without retry/rearm. The synthetic plant is explicitly not a
physical validation model. No agent-initiated nonzero sweep was performed.

Deployment verified: restarted the calibration service, performed stopped-state
inspection of IDs 1–3, and compared the saved calibration SHA-256 before and
after restart (unchanged). See continuous-deployment-readback.json. The native
browser was refreshed to load the new controls. The first loaded-leg continuous
sweep remains an operator-started trial; no nonzero sweep was sent by the agent.

## Keyboard teaching and active hold revision

The main UI now starts with one motor selection (1 knee, 2 worm, 3 belt).
Selection connects, stops and proves watchdogs at zero drive automatically,
using the operator's already-confirmed supported fixture. No drive starts on
page load or selection. Hold Q/A or the large pointer buttons for part-relative
upper/lower velocity commands. Release discards queued tracking error, brakes
the reference from the measured pose and keeps the same PID session energized
for position hold. Z, Escape, focus loss, selection change, or lease expiry
ends drive with verified STOP. After Z the motor must be deliberately selected
again. Browser renewal remains owner/run/sequence bound with a 750 ms deadline;
FPGA version 4 and its independent watchdogs are unchanged.

The main panel exposes a crawl-to-fast slider, Save lower/upper, Try saved
range / Pause and hold, and Reset poses. A position slider and requested vs
measured dial work between taught poses. Angles are motor encoder angles;
CAD joint gearing/reference are not inferred. PWM 0–100%, raw signed steps,
single-bound reset, direction swap and detailed traces remain in Advanced.
A saved pose outside the current encoder position is explained before movement
and the reset button targets just that pose. Existing measurements are preserved.

Saving during active hold requires six stable observations, low measured
velocity and small tracking error; it does not disable torque. Newly saved
bounds immediately tighten the host controller; the FPGA retains the enclosing
initial arm envelope until the next session. Clearing poses first stops drive,
retains history/reference/direction, and automatically prepares the same motor.
Untaught sides use encoder wrap guards, not invented mechanical limits.

Validation: 22 controller/transport tests and one server ownership/lease test
pass. Tests cover synthetic constant-load holding, both orientations, release
without target backlog, one torque session through release, and active ACK loss
without retry. Browser contract tests cover one-selection startup, Q/A release
during startup, hold capture, PWM, live speed, sweep/pause, Z, focus loss, reset,
out-of-range explanation, and stale startup replies. Build passes with only the
pre-existing unused joint_steps::decision warning. Native Chrome was refreshed
and the controls visually inspected alongside the robot viewer.

All three physical motors passed the new selection/zero-drive checks. Final
STOP confirmed torque off and stationary readback; calibration SHA-256 was
unchanged. See keyboard-deployment-readback.json. Belt encoder 2183 is beyond its
saved upper 2176, so the UI offers Reset upper pose. No nonzero motion was sent
by the agent. Loaded-leg hold/crawl gains and stopping distance remain provisional;
the synthetic plant tests are not physical validation. Source/config snapshots
and hashes are in source-keyboard/ and manifest-keyboard.json.

## Online response learning and continuous encoder revision (staged)

The previous keyboard controller treated raw 0/4095 as end stops. The live worm
readback was 4 counts (~0.35 degrees) with no taught worm limits. This was an
encoder rollover limitation, not a measured mechanical stop. The new host and
FPGA profile 5 track signed continuous counts across raw rollover in either
direction and through multiple revolutions. Protocol operation 7 carries an
explicit signed 32-bit anchor/lower/upper envelope, accepted only disarmed with
fresh raw feedback matching the anchor modulo 4096. FPGA signed mechanical
limits, exclusive axis ownership, addressed PWM bounds, independent watchdogs,
S2, and STOP verification remain active. Ambiguous half-turn jumps are faults.
Operation 6 remains the legacy raw single-turn window for compatibility.

The reusable control.adaptive_braking_envelope primitive is backed by
sim-domain-control::adaptive_braking. It records direction-specific acceleration
and conservative stopping excursion observations; three fresh interior stops
are needed to promote a demonstrated speed. Speed promotion never exceeds the
commanded trial ceiling. A one-second settled encoder-position window handles
quantization instead of treating a single low velocity sample as a complete
stop. Weaker-than-predicted braking immediately reduces trust. The planner uses
measured speed/position, response delay, uncertainty, and stopping clearance;
a predictive brake holds before a boundary. Hard bounds are never widened.

Learn motion in the middle starts at crawl, returns to center, performs short
alternating move/hold trials, and increases trial speed by at most 25 percent
within the operator-selected speed/PWM ceilings. It automatically holds after
both directions support the requested speed. Q/A, pause and Stop still take
precedence. Ordinary moves also contribute interior stop evidence. Faster
travel is confined to the central region; the outer quarters remain at crawl
because center trials do not identify changing gravity/leverage near the ends.
Evidence expires and is reset on pose/PWM changes, significant supply or
thermal changes, and a new control session. These are conservative engineering
assumptions, not a certified stopping guarantee. PID gains remain provisional.

Response evidence/configuration is saved as response-ID-RUN.json and serial
samples retain the online decision history. Learned models are not blindly
reused on a new session or promoted to CAD. The offline shared-library example
is crates/sim-domain-control/examples/adaptive_braking.rs.

Telemetry now distinguishes raw 12-bit position from position_continuous.
The host reads all three encoders while connected, including while another
axis moves. Single-turn encoders cannot recover revolutions missed during a
power/communication gap. Poses requiring a multi-turn coordinate are tagged
with the tracking session; after reference loss the UI requires re-teaching
both poses instead of silently reusing an ambiguous turn count. The fixture's
existing knee/belt measurements are preserved; angles are motor, not joint,
angles. Untaught sides have no artificial stop at zero; mechanical clearance
still has to be taught by the operator.

Validation: shared control/primitive tests, calibration/serial tests, browser
contracts and FPGA tests cover asymmetric synthetic gravity/load, altered
response, quantization, stale evidence, effort changes, predictive braking,
zero crossings, multiple turns, anchored limits, ambiguous jumps, and ACK
loss without re-driving. The FPGA build meets 50 MHz (final report 81.29 MHz).
No agent-initiated nonzero physical identification or rollover trial has been
performed. Physical performance and loaded stopping distance are unverified.

Deployment is pending confirmation that motor supplies are off for replacing
the independent FPGA safety logic. The image is built and tested; do not claim
it is loaded until a separate deployment receipt confirms that fact.
