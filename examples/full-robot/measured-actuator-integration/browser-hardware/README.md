# Browser gait playback on a mapped bench leg

Working and hardware-tested: a loopback Rust web adapter sends finite recorded
Rust-controller references through the existing supervised FPGA acquisition path.
The browser never calculates PWM. This is not yet a live quadruped simulation or
continuous gait steering.

Two browser-started full clips completed on IDs 10–12, each retaining 200 frames
and 600 telemetry samples. Separate early-stop tests passed for the Stop button
and closing the controlling tab while a second observing tab remained open.
Every physical run verified PWM zero, torque off and stationary motors afterward.
See [verification](verification.json) and [the browser capture](final-hardware-ui-test.png).

## Use

From the repository root:

```sh
cargo build --locked -p sim-runtime --example serve_motor_bench --example characterize_hx_bridge
target/debug/examples/serve_motor_bench examples/full-robot/measured-actuator-integration/browser-hardware/server.json 4180
```

Open `http://127.0.0.1:4180`. Select a CAD leg, assign its hip, worm, and foot
coordinates to distinct physical IDs, and choose each encoder polarity. Inspect
hardware, preview, then Run. The three connected IDs are configured explicitly in
`server.json`; an absent device is an error, not permission to shrink the stop scope.
The initial bench mapping is -Y hip=10, worm=11, foot=12. It is not a measured
installed-leg assignment. The UI can move this three-motor group to any CAD leg.

Motion scale now defaults to the hardware-tested 3%. Settings are validated before
Run becomes available. A rejected reference reports the coordinate, physical motor,
time, requested excursion or step, and applicable limit. In particular, the -Y
clip at 1x speed and 10% scale has a 33-count thigh-target change at 1.52 seconds;
the existing limit is 32. That combination remains rejected, while 9% passes.
The original generic error was a UI/default regression, not an FPGA connection
failure. The regression test uses the saved real gait trace and also checks the
3% default for all four leg selections. Limits and target samples are not relaxed
or silently clipped to make a request pass.

Each run lasts two seconds at 100 Hz, with a 10% PWM ceiling for UI bring-up.
Motion amplitude (1–25%) and playback speed (0.25–2x) transform the saved reference;
requests exceeding 80 encoder counts or the existing per-tick slew bound are
rejected. The final 200 ms transitions the target back to the measured start.
A target return does not prove the real motor returned exactly: the actual encoder
trace and subsequent stationary stop verification remain separate evidence.

The source is the existing full-authority ten-second Rust gait recording. The
prepared `reference-trace.json` retains its SHA-256 and named coordinates.
`trajectory_binding` is shared Rust code for coordinate mapping, interpolation,
units, polarity, bounded amplitude and time scaling. No robot plant is implemented
in the server or browser. Changing amplitude/speed does not generate a newly
simulated response, so the UI compares transmitted targets with actual encoders
and does not label a rescaled recording as a physical prediction.

## Transport and evidence

- One acquisition process owns the serial port for the entire preflight/upload/run/
  recovery sequence. Starts are mutually exclusive; mappings are immutable per run.
- Local requests require an unpredictable session token, exact loopback host, and
  same-origin checks. There is no remote binding or arbitrary executable/path API.
- The controlling browser tab polls every 100 ms. If its requests stop for 900 ms,
  the adapter requests
  STOP through the existing acquisition path. FPGA command/feedback watchdogs and
  the finite trajectory remain independent. A Stop button requests stopping;
  completion is displayed only after the acquisition verifies it.
  Other observing tabs cannot renew the controlling tab's lease.
- Each `sessions/session-*/` preserves the binding, source hash, compiled plan,
  upload, bitstream reference, raw serial/events, measured homes, live telemetry,
  controller audits and physical stop recovery. Voltage and temperature are
  displayed. Current is uncalibrated, so measured amps/watts are not claimed.
- Device frame numbers align targets and telemetry. Browser repaint timing is not
  FPGA timing, and transaction timestamps are not internal sensor sample times.
- `SERVER.lock` prevents another instance using the same output directory. After
  an abnormal exit, confirm the recorded PID and serial owner are gone before
  removing a stale lock. Launch from the repository root.

## Remaining

Three shared binding tests, two acquisition fixture tests, browser mapping/rejection
checks and HTTP access checks pass. Failed attempts remain in `verification/`:
an initially oversized synthetic test reference correctly hit the travel limit;
a concurrent acquisition fixture run hit the existing heartbeat timeout, verified
stopping, then passed with serial test execution. No production limit was widened.
The browser test also needed to await the new preview rather than assert against
the previous capture's cards. Actual hardware recordings remain separate from all
software fixtures.

Live simulator connection, calibrated installed-leg zeros/transmissions, persistent
whole-robot assignments, continuous interactive updates, and a validated realtime
physics profile. An unloaded motor clip cannot establish loaded walking accuracy.

## Live WASD prototype — September 15

Open http://127.0.0.1:4180/walking/ for the existing 3D Rust/WASM walking controller, leg/motor mapping, and target-versus-encoder graphs. Start simulation with Play; use WASD. “Sync motors” starts a bounded 12-second physical session. Pause, reset, stop, stale references, or loss of the owning tab stop the hardware.

The current live adapter is explicitly provisional: 10 Hz host-scheduled FPGA control, 10% PWM ceiling, and 3% default reference scale. The requested buffered 100 Hz live implementation is still pending. Do not interpret this mode as validation of full gait tracking.

A headless check using the same frozen Rust/WASM drove IDs 10/11/12 for 68 control frames and retained 204 real readings, then explicitly requested STOP and verified the physical stop. Evidence: `live-validation/wasd-status.json` and `sessions/session-1789509811685237000`. Tracking criteria did not pass. Chrome returned ERR_BLOCKED_BY_CLIENT for the viewer; rendered UI verification remains pending.

### Browser connection confirmed; tracking rejected

The rendered Chrome viewer subsequently completed a full 12-second live session, `sessions/session-1789510466300153000`, with 120 control frames / 360 physical motor samples and a verified final stop. All three encoders moved. The user confirmed physical motion. Tracking failed: hip/thigh/foot RMS errors were 0.855 / 0.518 / 0.431 degrees; the hip repeatedly oscillated around a nearly stationary target and hit the 10% PWM limit in 61% of frames. This is transport functionality, not gait-fidelity validation.

Paired read-only packets reduce USB request/reply overhead without exceeding the installed FPGA two-packet queue. Every motor still has position/voltage/temperature/current gates, per-frame torque/PWM audits, arithmetic checks, fixed schedule deadlines, stale-reference checks, and a verified stop. One earlier paired-read browser run still missed a host deadline (`session-1789510438053722000`); retain it as evidence that macOS host timing is provisional. A subsequent Darwin interactive-QoS request is a scheduling hint only and was added after the successful capture.

Fixed an early-STOP/startup directory race; cancellation now leaves a marker in the session root instead of creating the child's exclusive capture directory. The viewer shows a prominent SIMULATION ONLY / MOTOR SYNC status and reports acquisition errors rather than claiming completion. Ended episodes require Reset before reconnecting.

Next acceptance milestone: independently clocked FPGA feedback at 100 Hz with live buffered references, followed by gain tuning and tracking validation. Do not promote the current 10 Hz adapter or its gains as an accurate robot controller.

Validation of this patch: paired-read protocol and early-STOP race tests pass. Acquisition example suite: 13 passed, 2 existing autonomous UART capture fixtures failed heartbeat acknowledgement deadlines; both also failed an isolated repeat. Those fixtures use the unchanged autonomous path, but the failures remain unresolved and are retained in `live-validation/wasd-isolated-capture-tests.log`. No broad timing acceptance is claimed.
