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

## Deployment update

FPGA v5 loaded into SRAM after operator confirmed motor supplies off. Serial
readback verified version 5 and zero armed mask. Initial S2 latch was replaced
by explicit STOP reason 10 and remained there, so S2 was not continuously
asserted at this check. The matching host and learning UI are running on
http://127.0.0.1:4194 and visible in Chrome Incognito. Saved calibration hash
is unchanged. Powered motor readback, zero-drive watchdog checks and physical
learning validation remain pending. See deployment-adaptive-rollover.json.
