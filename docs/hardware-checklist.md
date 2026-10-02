# Hardware front end: the operator's checklist

This is the pending operator checklist for the native hardware front end in
[architecture/native-viewer.md](architecture/native-viewer.md) §8. The active
LC1–LC3 batch covers **HW-01–HW-09 only**; HW-10–HW-16 remain reference and
separate operator checks. The accepted verification repairs `ee17ef00` through
`f971bedb` built the bounded touched paths and exercised virtual connection and
idle STOP. [verification-20261002.md](verification-20261002.md) and retained
`runs/verification-20261002-f541b05f/` do **not** establish calibration completion,
independent CAD parity, physical behavior or acceptance of portable-study-artifacts.
Portable work at `f541b05f` remains set aside/unaccepted. No physical step below
was executed in the LC writing turn. The feature ledger is
[hardware-parity.md](hardware-parity.md).

**Execution status, 2026-10-02 (after `f734194d`, `27f5f1aa`, `f5b48842`).** There is
**no executed evidence for HW-01–HW-09 in the native viewer**. Neither the virtual
acceptance driver nor a physical pass has run since these changes. The earlier
observation of virtual connection and idle STOP predates them, so it is not
evidence for the current code. Every trace in
[Reading traces](#reading-traces--hw-01hw-09-by-reading-unexecuted) comes from
reading the source, and every step below is unexecuted. No test is claimed to
pass. The browser calibration page stays the reference until a pass runs.

The written future acceptance driver is
[tools/native-calibration/acceptance.py](../tools/native-calibration/acceptance.py),
with [launch/evidence instructions](../tools/native-calibration/README.md).
**Launch path.** Three freshly built binaries:

1. `hx_virtual_bench --capability-socket`.
2. `serve_actuator_calibration CONFIG PORT --virtual-bench SOCKET`. This is the
   calibration server, still a separate Rust process. It owns the bus and the
   records, and it still serves the browser calibration page, which stays the
   reference.
3. `sim-spatial --robot-preset … --hardware <server URL>`. The native panel is a
   client of the server; it never opens serial.

The positive HW-01–HW-09 path connects the viewer directly to the server. A
loopback identity proxy is used only in a separately labelled fixture phase with
a second viewer. Each viewer gets an isolated config directory, so the operator's
preferences and recent documents are never read. The driver uses an isolated
copied virtual-only configuration and a new output directory. Evidence classes
are kept apart: written fixtures and source review exist now; executed evidence
needs a future authorized run. Its virtual
results are labelled **SIMULATED**, never measured/calibrated hardware results.
Motion authorization requires current server/bench identity and connection
generation; physical/unknown endpoints still refuse automation. No gait, raw-step,
live-sync or general remote-motion override is included. STOP remains available
regardless of identity. Browser compatibility and CAD physical definitions remain.

All seven batch checklist IDs remain pending future executed acceptance:

| ID | Status |
|---|---|
| `native-leg-calibration-completion:outcome-1` | Pending HW-01–HW-09 fresh native/virtual evidence |
| `native-leg-calibration-completion:outcome-2` | Pending physical/unknown/replacement refusal and unconditional STOP evidence |
| `native-leg-calibration-completion:outcome-3` | Pending teaching/learning/tune/campaign limits and preserved-record evidence |
| `native-leg-calibration-completion:outcome-4` | Written driver/run sheets; pending verified receipt |
| `native-leg-calibration-completion:task-LC1` | Source implementation and written fixtures; execution pending |
| `native-leg-calibration-completion:task-LC2` | Source implementation and written fixtures; execution pending |
| `native-leg-calibration-completion:task-LC3` | Written driver/docs; fresh-binary verification pending |

**The browser pages stay** (`http://127.0.0.1:4194` for calibration,
`http://127.0.0.1:4180/walking/` for live sync) until you sign this off at the
end.

## Before you start

- **Fixture.** The printed leg is clamped and supported so that torque-off
  cannot drop it (examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/README.md:
  ID 1 knee, ID 2 worm, ID 3 belt/hip). The operator stays at the fixture
  with a hand free for STOP for the whole session. For the tune, campaign and
  gait steps the leg must be suspended with clear space around every joint.
- **FPGA.** Use the calibration image already deployed (profile v8, see
  `deployment-v8-reload-2026-09-25.json`). This checklist loads no image. If
  an image has to be loaded, do it only with motor power off and record a
  deployment receipt.
- **One front end drives at a time.** The server ties a motion session to
  the client that started it. Before you repeat a step in the other front
  end, press STOP and close the first front end's panel (× in either).
- **Record every limit you raise** (with the reason and the previous value),
  as AGENTS.md requires. No step here needs a limit raised.

The following is a **physical operator launch reference**, not an instruction to
execute during source work. The native viewer still requires the separate Rust
calibration server; it does not own serial I/O. Use the already reviewed fixture
configuration only when the operator has authorized a physical session:

```sh
cargo build --locked -p sim-runtime --example serve_actuator_calibration
target/debug/examples/serve_actuator_calibration \
  examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json 4194
```

Start the native viewer on the measured full robot, with the panel connected
at launch. A release build loads the robot faster. The panel itself needs no
release build: it neither simulates nor computes motion.

```sh
cargo run -p sim-spatial -- --robot-preset robot-measured-400hz --hardware http://127.0.0.1:4194
```

Without `--hardware`, open the panel with the header's **Leg calibration**
button and press **Connect** (it uses `http://127.0.0.1:4194`). The browser
comparison is the Leg calibration panel at `http://127.0.0.1:4194`. The
viewer's REST port is 8421 (`--api-port`).

In each step, tick the result box and write down anything that differs:
`[ ] same` / `[ ] differs: …`.

## Steps

### HW-01 Connect

- **Native:** launch as above, or press Leg calibration, then Connect.
- **Expect:** the panel opens with "Choose the motor you want to calibrate."
  Nothing moves and no motor is energized. A verified virtual connection is labelled
  SIMULATED; physical/unknown remains labelled honestly and motion automation is
  refused. Server status is read every
  600 ms: the position readout shows "—" until a motor is chosen (the dial
  keeps its neutral needles), and the status is never shown as stale while
  the server runs. The top bar's connection line reads
  "http://127.0.0.1:4194 · link n" (current connection generation). Stop the server for
  3 s: the panel marks its status stale (a native addition) and still
  nothing moves. Restart the server and press Connect. The generation changes, no earlier
  ready/motion permission is reused, and selection is required again.
- **Browser:** open `http://127.0.0.1:4194`, press Leg calibration.
- **Result:** [ ] same  [ ] differs: ______

### HW-02 Select, disable and enable a motor

- **Native:** press **Knee 1**, then **Worm 2**, then **Belt 3**. On Worm,
  press **Disable this motor**, then press the Worm chip again, then
  **Enable this motor**.
- **Expect:** each select shows "Connecting and checking this motor at zero
  drive…", then the server's ready message ("Ready. Hold Q toward upper or A
  toward lower; release to hold position."). With "Hold the other enabled
  motors in place" checked, the others hold. Disable stops first. The Worm
  chip reads "⊘ Worm 2" (the page strikes it through).
  Choosing it reads "worm is disabled. Enable it to move it." and sends no
  select. Enable restores it.
- **Browser:** the same presses.
- **Result:** [ ] same  [ ] differs: ______

### HW-03 Hold-to-move (Q/A and the Upper/Lower buttons)

- **Native:** choose Worm, keep Movement speed near the slow end. Hold **Q**
  for about 1 s and release. Hold **A** and release. Press and hold **Upper ↑**
  with the mouse and release; the same with **Lower ↓**. Hold Q and A
  together. Move the speed slider while holding Q.
- **Expect:** the motor moves toward upper while Q is held and holds its pose
  on release (active hold, not torque-off). A goes toward lower. The held
  button is highlighted. Q+A together holds. The speed label reads "x.xx°/s
  motor" (and "· limited to … here" when the server limits it). The dial's
  green needle follows the encoder, the blue one the request. The Command vs
  real motion chart draws blue target and green encoder traces. While the
  panel is open, A does not steer the robot (W, S and D still do).
- **Browser:** the same with Q/A and the buttons.
- **Result:** [ ] same  [ ] differs: ______

### HW-04 STOP from every section: Z, Escape, the Stop buttons

- **Native:** for each of these, hold Q on Worm (or start the named activity),
  then stop it:
  1. top bar **Z Stop** button with the panel scrolled to the top;
  2. with **Tune this motor** open and scrolled into view: the top bar Stop
     (it must still be on screen);
  3. with **Characterization campaign** open: the top bar Stop;
  4. with **Gait playback** open: the top bar Stop, then (sim-only gait) the
     gait's own **Stop**;
  5. with **Simulated leg mirror** open: the top bar Stop;
  6. with **Advanced settings & feedback** open and scrolled to its end: the
     top bar Stop;
  7. with **Real motor sync** in view: run during HW-13 (the top bar Stop,
     and **Stop motors**);
  8. key **Z**, then key **Escape** (also with the pointer over a slider).
- **Expect:** drive stops at once each time, and the status shows the
  server's stop message. Holding Q again after a stop does nothing until the
  motor is chosen again (the page's `ready` is cleared). The top bar never
  scrolls away.
- **Browser:** Z, Escape and the Stop button with each section open.
- **Result:** [ ] same  [ ] differs: ______

### HW-05 Focus loss, panel close and leaving Robot mode stop drive

- **Native:** hold Q, and while it is held:
  1. switch to another app (Cmd+Tab);
  2. again, then close the panel with ×;
  3. again, then click **Leg calibration** in the header (closes it);
  4. again, then switch mode with the mode switcher (bottom right) to Build.
  5. once more with a slow **Try saved range** running instead of Q (no key
     held), then switch to another app.
- **Expect:** each one stops drive at once (the STOP goes out on its own
  connection, not behind other requests). The mirror display ends with the
  panel. Back in Robot mode the panel reconnects only if the viewer was
  launched with `--hardware`; otherwise press **Connect**. Nothing moves
  either way. In 5 the native panel stops the sweep (any drive stops on a
  loss, a deliberate safety difference, ledger CAL-30); the page stops it
  too, because a sweep is a motion session.
- **Browser:** hold Q and switch tabs (visibilitychange), close the panel,
  press the header toggle, and close the tab (pagehide with the keepalive
  STOP).
- **Result:** [ ] same  [ ] differs: ______

### HW-06 Teach lower, upper and the sim alignment

- **Native:** on Worm, move with Q/A to a safe lower pose and release, wait
  for Holding, press **Save lower here**. The same for **Save upper here**.
  Move to the mirror's alignment pose (HW-11 explains it) and press **Save
  sim alignment here**. Drag **Move to a taught pose** to about 25 %, 75 %,
  then release. Press **Reset poses** and teach both again.
- **Expect:** each caption shows the saved angle ("x.x° motor"). The target
  slider becomes available with both poses ("lower → upper"), and the motor
  follows it inside a 4-count inset, then holds. Saving while moving reads
  the server's "Release…" message in the capture line. Reset stops, clears
  and re-selects. A motor beyond a taught pose reads "Beyond the saved …
  pose. Move back inward freely; driving further out is blocked."
- **Browser:** the same.
- **Result:** [ ] same  [ ] differs: ______

### HW-07 Try saved range, Sweep all and Learn

- **Native:** press **Try saved range**, watch one traverse, press **Pause &
  hold**. Press **Learn motion in the middle** and let it finish (or **Pause
  learning & hold**). With two or more motors taught, press **Sweep all
  enabled motors**, then let it finish; run it again and press **Stop
  sweeping all**.
- **Expect:** Try saved range sets the speed slider to its slow end and
  sweeps between the poses. The learning line reads "{status}. Stops learned:
  d / i. Allowed now: …". Sweep all shows "Sweep-all: checking every enabled
  motor at zero drive…", then "{role} n/2 ends · …" about 4 times a second,
  and ends with "Swept {roles} through their saved ranges." (or "Sweep-all
  stopped: …").
- **Browser:** the same.
- **Result:** [ ] same  [ ] differs: ______

### HW-08 Tune

- **Native:** with the motor mid-travel, tick "The motor is mid-travel with
  room to move both ways" and press **Tune this motor**.
- **Expect:** the tune status reads "Tuning: {stage}", then "Tuned gains in
  use: kp …, ki …, kd …, friction …% · {record}". About 20 s, at most
  ±200 counts of travel. The confirmation unticks at the end.
- **Browser:** the same, on a different motor or after a pause.
- **Result:** [ ] same  [ ] differs: ______

### HW-09 Campaign and resume

- **Native:** with the leg suspended and every tested motor tuned and taught,
  open Characterization campaign, tick the confirmation, press **Run
  campaign**. After a stage or two, press STOP. Tick again and press
  **Resume**.
- **Expect:** "{stage} · n stage results saved" while it runs (with "· last
  stopped by {gate}" after a gated stage). STOP ends it with "Last campaign
  stopped: …". Resume continues from the saved stages. On finishing: "Finished:
  {headline}. {directory}". Nothing is promoted to CAD.
- **Browser:** the same.
- **Result:** [ ] same  [ ] differs: ______

### HW-10 Gait playback: Sim, Leg and Both

**Requalify before Leg or Both.** Any edit to crate sources invalidates the
gait-lab runtime fingerprint, and this epic edits crate sources. Before gait
playback on the leg, requalify the reduced model as the gait-lab README says
(examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25/README.md,
"After any library source change"). **Do not run Leg only or Both until it is
requalified.** Sim only is safe at any time.

- **Native:** open Gait playback (the list loads). Pick a gait, keep **Sim
  only**, press **Play**, **Pause**, **Resume**, move Playback speed, press the
  gait's **Stop**. After requalifying: tick the suspended-leg confirmation,
  choose **Leg only** at 50 % effort and a low playback speed, Play, then
  Stop. Then **Both**.
- **Expect:** Sim only animates the suspended robot: "Sim only · gait time t s
  of P s period · n% speed". Leg only drives the aligned, taught, enabled
  motors ("Not driven: …" lists the rest, with reasons), shows "Limits: …",
  "Leg: {phase} · error … counts", and the statistics table after the run.
  Recent leg runs gains a row. Both shows the real leg (blue) on the gait's
  clock. The radios and Leg effort are locked while it plays. Stop stops
  drive.
- **Browser:** the same gait, mode, effort and speed.
- **Result:** [ ] same  [ ] differs: ______

### HW-11 Mirror alignment

- **Native:** in Simulated leg mirror, tick "Show the real leg on the
  suspended simulated robot", choose the leg (+X by default), and check each
  motor's row (joint, sign, alignment pose: Mid-travel for the foot, CAD home
  for the others). Move each motor until the real leg matches the simulated
  leg's alignment pose and press **Save sim alignment here** for it. Then move
  each with Q/A. Try **Run** in the robot header.
- **Expect:** the robot is held 0.25 m up with its body still. The chosen leg
  is tinted blue and follows the encoders: "{role}: x.x° from its alignment
  pose". An unaligned motor reads "not aligned — shown at {pose}". Moving past
  a CAD limit adds "· Beyond CAD limit: …". Flipping a sign reverses the
  simulated joint. Run is refused while mirroring. Unticking ends the display.
- **Browser:** the same (its mirror runs in a web worker).
- **Result:** [ ] same  [ ] differs: ______

### HW-12 Advanced settings

- **Native:** open Advanced settings & feedback. Set Control mode to each
  option and hold Q briefly each time (the servo modes are experimental and
  untested on hardware; skip them if in doubt). Lower the PWM ceiling while
  holding Q. Press **Swap upper / lower direction**, check the direction line
  in the chart, then swap back. Press **Reset lower only**, then **Reset upper
  only**, and re-teach. Set a raw step of 20 and press **Send raw step**, then
  −20.
- **Expect:** the mode applies from the next session. The PWM ceiling is a
  slider/stepper (the page's number field), and the effort stays under it.
  Swap stops, re-selects and flips "Toward upper ↑ = encoder ±". Each reset
  clears one pose. A raw step moves about that many counts. The telemetry
  line shows V, °C, encoder and effort. The control mode is still set after a
  viewer restart (the preferences file).
- **Browser:** the same.
- **Result:** [ ] same  [ ] differs: ______

### HW-13 Live sync (motor bench)

The bench server drives IDs 10–12 through the same serial port as the
calibration server (both `server.json` files name it). Stop the calibration
server first, since one process owns the port, and use the bench motors its
README describes.

```sh
cargo build --locked -p sim-runtime --example serve_motor_bench --example characterize_hx_bridge
target/debug/examples/serve_motor_bench examples/full-robot/measured-actuator-integration/browser-hardware/server.json 4180
cargo run -p sim-spatial -- --robot-preset robot-measured-400hz --motor-bench http://127.0.0.1:4180
```

- **Native:** open Leg calibration and find Real motor sync. It reads "Ready.
  Choose a leg, then start sync and steer with WASD." Choose a leg, give two
  joints the same motor ID and press **Sync motors · 12 seconds**. Then give
  distinct IDs and set the scale to 3 %. Press Run on a live walking
  controller, press Sync motors and steer with W/S/D. Then pause the run
  mid-session. Start again and press Reset. Start again and press **Stop
  motors**. Start again and switch to another app. Once, let it run the
  full 12 s. Then, while syncing, switch to Build with the mode switcher;
  return to Robot mode, start again, and close the window.
- **Expect:** duplicate IDs read "Assign a different motor ID to each
  joint." and nothing starts. Without a live run: "Reset the episode, then
  choose a live walking controller with named motor targets." The banner reads
  "CONNECTING MOTORS · Starting bounded motor session…", then "MOTOR SYNC ·
  Syncing live WASD targets…" (green). A chart per joint shows RMS error and
  "at drive limit", with readings "ID n: x.xx° · V · °C · input age ms".
  Pause, Reset and Stop motors each stop, showing "… — verifying physical
  stop…" and then "Motors stopped and verified. … Saved: {run}". Switching
  app stops it with "Window lost focus" (the page does not stop on a tab
  change: a deliberate difference, ledger SYNC-32). Switching to Build and
  closing the window each stop it at once (the page's `pagehide` stop,
  SYNC-29): read the bench's `/status` with `curl` or in the browser page
  to see the session end. The full run ends "12-second live session
  complete.". Without `--motor-bench`, the section explains how to start the
  bench (the page shows no section at all).
- **Browser:** `http://127.0.0.1:4180/walking/`, the same steps (close the
  tab for the window close).
- **Result:** [ ] same  [ ] differs: ______

### HW-14 Watchdog trip and lease loss

Run each at slow speed on one motor, the leg clamped.

**Do not move focus away from the viewer before a kill (steps 2–5).** The
window losing focus stops drive by itself (`WindowFocused` →
`Loss::FocusLost` → STOP), so the lease or watchdog would never be tested.
Start the kill first, delayed, then give the viewer focus and do not touch
another window: in a terminal run
`sleep 8; kill -9 $(pgrep -f 'target/.*/sim-spatial')` (or the command
named in the step), then click into the viewer within the 8 s. Or run the
kill over ssh from another machine.

1. **Window closed mid-jog.** Hold Q and close the viewer's window with its
   close button. *Expect:* drive stops at once (`Loss { Leaving }`: the
   immediate STOP, a STOP written synchronously as the window closes, and
   the link's STOP on drop). Repeat with Cmd+Q instead of the close button:
   *Expect:* drive stops at once too (the `AppExit` system and the link's
   drop write STOP synchronously; the server's log shows the `stop`).
   *Browser:* hold Q and close the tab. The keepalive STOP stops it at once.
   **Known limit:** after a crash or `kill -9` the viewer sends nothing. Gait
   and motion sessions stop when their leases expire and the FPGA watchdog
   stops motion independently, but a running tune, campaign or sweep-all
   keeps going on the server until it ends or STOP is pressed (on the
   server's page, or in a restarted viewer).
2. **Viewer killed mid-jog (no STOP sent).** Start the delayed kill, click
   into the viewer, choose the motor and hold Q until the viewer dies.
   *Expect:* the heartbeat stops. Within the server's 1.5 s motion lease the
   server stops drive and its status reads "Browser heartbeat lost; sweep
   stopped" (read it in the browser page or with `curl` on
   `/calibration/status`). If the status shows the server's ordinary stop
   message instead, the viewer sent STOP because focus was lost first:
   repeat. *Browser:* start a delayed kill of the browser process, click
   into the page and hold Q (or Upper ↑) until it dies. The result should be
   the same.
3. **Leg gait lease** (only after the HW-10 requalification). Play Leg only
   in the viewer, start the delayed kill, click into the viewer and wait.
   *Expect:* "Browser heartbeat lost; gait stopped" within 1.5 s (if the
   viewer's stop shows instead, focus was lost first: repeat). *Browser:*
   Play Leg only in the page and kill the browser the same way; the same
   message.
4. **Bench lease** (HW-13 set-up). While syncing, start the delayed kill,
   click into the viewer and keep steering with W. *Expect:* the bench stops
   within its 0.9 s lease ("Browser lease expired"); if it reads "Window
   lost focus — verifying physical stop…" first, focus was lost: repeat.
   *Browser:* sync on `/walking/` and kill the browser the same way; the
   same result.
5. **FPGA watchdog.** Start a delayed kill of the calibration server
   (`sleep 8; kill -9 $(pgrep -f serve_actuator_calibration)`), click into
   the viewer and hold Q until the server dies. *Expect:* the FPGA's
   command (300 ms) and telemetry (200 ms) deadlines trip and latch the bridge
   with drive off. The viewer's status turns stale and shows the connection
   error. Nothing moves when the server restarts until a motor is chosen
   again. Follow the server's message if it reports a stale port lock.
   *Browser:* the same with the page open.

- **Result:** [ ] same  [ ] differs: ______

### HW-15 Export

- **Native:** press **Download calibration** (Advanced), and send REST
  `hardware_export` once.
- **Expect:** a new file
  `examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/measurements/viewer-exports/leg-calibration-<unix_ms>.json`
  (under the server's `output`), with its path shown in the panel and
  returned by REST. Nothing is overwritten. It holds the server's export plus
  `display_mirror` (leg, bindings, `lift_m` 0.25, `counts_per_revolution`
  4096 and the display-only note).
- **Browser:** Download calibration saves `leg-calibration.json`. Compare the
  two files; only the time-dependent fields should differ.
- **Result:** [ ] same  [ ] differs: ______

### HW-16 REST and system_ui refusal

This reference check targets a **physical or unknown** endpoint with the panel
connected and no motor chosen. The LC1–LC3 virtual allowlist is a deliberate
exception for verified virtual HW-01–HW-09 only; the future driver exercises that
exception and harmless identity fixtures without opening physical serial.

Use the shared viewer batch API and poll the returned `/v1/jobs/<id>` URL:

```sh
curl -s -H 'Content-Type: application/json' \
  -d '{"commands":[{"command":"hardware","args":{"action":{"select":{"id":1}}}}],"stop_on_error":true}' \
  http://127.0.0.1:8421/v1/batch
```

- **Expect on physical/unknown:** `select`, tune/campaign confirmations, motion
  values and all other motion arming requests fail with a named refusal. No motor
  is energized and no confirmation changes. `mirror_enabled` is display-only;
  mirror binding changes, raw step, gait play and live sync remain outside the
  remote allowlist. `loss:leaving` is refused; use `hardware_stop` instead.
- **STOP:** request `hardware_stop` through the same batch API. Poll its terminal
  outcome and observe authoritative server torque-off/session termination. STOP
  must succeed independently of the identity mismatch/refusal. A queued HTTP
  response alone does not prove interruption.
- **system_ui:** list with `{"action":{"operation":"controls"}}`; motion controls
  are disabled with the same authorization reason. Activation with the current
  `ui_revision` must fail truthfully, including eventual authoritative consumer
  rejection; it must not acknowledge success merely because an action was queued.
- **Verified virtual:** only in-scope calibration actions become available for the
  current strict identity and connection generation. Disconnect, stale status,
  identity mismatch or replacement revoke permission; reconnect does not reuse it.
- **Browser:** the browser has no viewer automation surface. Keep its existing
  supervised physical workflow available; the virtual exception does not change
  the physical-browser execution policy.
- **Result:** [ ] as expected  [ ] differs: ______

## Virtual acceptance map — HW-01–HW-09 (written, not executed)

This table is the contract the future driver checks against
[tools/native-calibration/acceptance.py](../tools/native-calibration/acceptance.py).
Everything in it is written: the driver and `fixtures.py` exist, but none of it
has run. Control ids are `system_ui` ids (`hardware:<name>`, or `mode:<mode>`).
REST actions are `hardware {"action": …}` batch commands polled through
`/v1/jobs/<id>`. Every step passes only on the listed observation, never on a
202 or a job that succeeded. Every step also retains its `http.jsonl` entries and
its final `hardware_status` in `results.json`. A failed deadline writes
`timeout-state-<ms>.json`.

**Build receipt.** After building the three binaries and keeping the build logs,
the pass writes `fresh-build-receipt.json` with `acceptance.py receipt --bench …
--server … --viewer … --build-log LOG … --out FILE`. The helper builds nothing.
It records `source_hash()`, each binary's sha256, the git commit and the
build-log paths and hashes. It refuses, writing nothing, when a binary or build
log is missing or empty, or when FILE already exists. The hashes bind the
receipt to the tree and the binaries; they do not prove compilation, so the
verifier inspects the logs the receipt names.

**Screenshots.** With `--screenshots` (off by default; normal writing turns
never pass it) the driver captures each checkpoint below through the viewer's
own REST `screenshot` command. It polls the job and then the file (PNG
signature and trailer) within 20 s, and saves the image as
`<out>/screenshots/<step>-<checkpoint>.png`, never overwriting a file. The
outcome is recorded in that step's `screenshots` list. A failed or timed-out
capture is recorded as failed and never decides a step: only the observable
assertion does. No screenshot has been taken by this driver yet.

| Step | Native control ids | REST actions | Observable assertion | Extra evidence retained | Screenshot checkpoints (`--screenshots`) |
|---|---|---|---|---|---|
| HW-01 | `hardware:connect` | `hardware_status`, `select` (must be refused) | `url` is the owned server URL (direct, no proxy), `fidelity: virtual_simulated`, fresh age, `enabled_id: null`. The server is paused (SIGSTOP) until status is stale **and** `authorization_revoked` is true (25 s), then resumed. Revocation persists for the same generation, `select` fails with "authorization expired", and the server still has no enabled motor. Reconnect gives a new generation, no revocation, no ready session, and `hardware:select_2` is enabled again. | `execution.json`, `hw01-revoked.json`, server/viewer logs | `HW-01-identity`, `HW-01-fresh`, `HW-01-stale`, `HW-01-reconnect` |
| HW-02 | `hardware:select_1/2/3`, `hardware:set_disabled` | `hold_others {on:true}`, `drive_mode {mode:"pwm"}` | `form.hold_others == true` and `form.drive_mode == "pwm"` (preferences isolated per viewer). Each motor becomes ready. A disabled motor stays not ready when selected. After re-enabling, it is ready again. | `viewer-main-launch.json` (isolated config environment) | `HW-02-disabled` |
| HW-03 | `hardware:jog_upper`, `hardware:jog_lower` as toggles: activating one presses it (it lists `jog_press`), and activating it again while held releases it (it lists `jog_release`) | `speed {percent:25}` | Each press and release is confirmed through `form.held_upper`/`held_lower` (or the toggle's listed action). The encoder moves at least 60 counts each way, the session intent is not hold while moving, and the motor holds after release. With upper then lower pressed, both show held. After both are released through their toggles, the motor holds within 32 counts for 1 s. A failed jog is released best-effort (`jog-release-failures.jsonl`). | per-step state | `HW-03-held` |
| HW-04 | `hardware:section_tune/campaign/mirror/advanced`, `hardware:select_2`, `hardware:jog_upper`, `hardware:stop` | — | `hardware:stop` is listed and enabled after each section toggle. A jog started through its toggle is ended by STOP, with no release: no ready session, no run, `enabled_id: null`, not busy, and no sweep, tune or campaign running. | per-step state | `HW-04-stop-tune`, `HW-04-stop-campaign`, `HW-04-stop-advanced` (none for mirror; STOP placement is source review until a pass captures these) |
| HW-05 | `hardware:close`, `hardware:toggle_panel`, `mode:phenomena`, `mode:robot` (90 s) | `loss {reason:"focus_lost"}` | Each interrupt stops drive. After close or toggle, `open: false`, and the panel is reopened with `hardware:toggle_panel`. After mode exit, direct server status has `enabled_id: null` and is not busy, and `hardware_status` is refused with "the active mode is phenomena". Robot re-entry reconnects idle with a new generation, and the form is set again. (Build needs a document, so it is not used.) | direct status polls in `http.jsonl` | — |
| HW-06 | `hardware:capture_lower/upper/reference`, `hardware:reset_poses`, `hardware:select_3`, `hardware:stop` | `speed`, `target`, `target_commit` | Before each capture, the motor holds still inside the server's capture bounds for 0.5 s (holding, under 2 counts/s, target within 3 counts). A capture counts only when a new `records/calibration-*.json` appears, `capture_message` reads `Saved <pose> pose` and the value is set. It is retried up to 5 times while unsaved, for example while the server reports "Still settling". After teaching, `speed`/`target` of −1 and 101 fail with "must be a number from". Targets of 25 % and 75 % are reached within 32 counts, inside the taught range minus 4 counts. Reference is non-null. Reset clears lower/upper, and motor 2 is taught again. Motor 3 is taught. | `captures.jsonl`, per-step state | `HW-06-taught`, `HW-06-target`, `HW-06-reset` |
| HW-08 | `hardware:tune_confirm`, `hardware:tune`, `hardware:stop`, `hardware:select_3` | `target`, `target_commit` | Motor 2 starts untuned. STOP is pressed as soon as the tune is seen running, with no screenshot in between. Right after STOP the interruption is asserted: tuning not running, `tuning.error` set (stopped path), and still no gains on motor 2. A fresh tune then shows at least 2 distinct stages in `session.tune_stages`, finite kp/ki/kd/friction, a record file inside the output, and the confirmation cleared. Repeated on motor 3. | `tune-interrupted.json`, `tune-stages.json`, tune records | `HW-08-stopped` (after the interruption is asserted), `HW-08-terminal` |
| HW-07 | `hardware:sweep`, `hardware:learn`, `hardware:sweep_all`, `hardware:stop` | `speed`, `loss {reason:"focus_lost"}` | Starting Try saved range resets the speed to 0. Saved-range travel stays inside the poses. Pause holds. Focus loss stops a sweep and keeps the poses. `session.learning_terminal` shows completion with at least 3 stops each way, `learning: false`, intent hold. Sweep-all shows 2 half cycles on motors 2 and 3, then ends. A second sweep-all is stopped. | `learning-terminal.json`, `sweep-all-ends.json` | `HW-07-learned`, `HW-07-sweep-all` |
| HW-09 | `hardware:campaign_confirm`, `hardware:campaign`, `hardware:campaign_resume`, `hardware:stop` | — | STOP is pressed as soon as a saved stage is seen with the campaign running, with no screenshot or hashing in between. Right after STOP the interruption is asserted: not running, `campaign.error` set, no result. A receipt is saved for each completed stage (`*.execution.json` provenance not counted). The count survives STOP. Receipt hashes are unchanged after resume. Terminal `report.json` is inside the output. Confirmation clears. | `records/` and `record_sha256` | `HW-09-stopped` (after the interruption is asserted), `HW-09-terminal` |
| LC1 | `hardware:connect`, second viewer `hardware:select_2` | `raw_step`, `gait_play`, `sync_start`, `select` | Out-of-scope actions get the native remote-refusal text (`remote_refusal`). The server's own 400 for out-of-scope commands with a valid pin is covered only by written unit tests, not by this driver. Direct requests with no identity or a foreign identity get HTTP 409 "Calibration execution binding refused". STOP with the same headers returns `stop_latched: true`, `enabled_id: null`, `busy: false`. After a real server restart, the new UUID is seen, old headers get 409, and native refuses ("authorization expired") until reconnect. FIXTURE phase only: a second viewer behind the proxy presented as physical, then unknown, gets "Remote calibration requires a verified virtual" with zero crossings. | `replacement-execution.json`, `identity-fixture.jsonl`, `viewer-fixture-*.json` | — |

Cleanup runs even after a failure:

- The direct server STOP is decided by ownership of the server process and
  port, sends no identity headers, and is asserted latched.
- Native `hardware_stop` goes to each owned viewer.
- Only the driver's own children are terminated.

A timed-out job is cancelled with `DELETE /v1/jobs/<id>` before the direct STOP.

## Reading traces — HW-01–HW-09 (by reading, unexecuted)

**Status.** No executed evidence exists for HW-01–HW-09 in the native viewer.
Neither the virtual driver above nor a physical pass has run since `f734194d`,
`27f5f1aa` and `f5b48842`. Every trace below was made by reading the source at
`f5b48842` (2026-10-02); `server.rs` lines are those of the commit that adds
this section, which also makes inspect, the error-path stop and the link
probe use the first enabled motor. Nothing here was built, run or tested. The
browser calibration page served by `serve_actuator_calibration` stays the
reference until a pass runs. Later edits may move the line numbers.

Each trace follows one path: the control (panel button id, key, or `system_ui`
id `hardware:<name>`), then the `HardwareAction`, the handler, the
link/session/`hardware_client` call, the server/bench effect, and finally what
the panel or `hardware_status` shows.

| Short name | File |
|---|---|
| `actions.rs` | `crates/sim-spatial/src/robot/hardware/actions.rs` |
| `input.rs` | `crates/sim-spatial/src/robot/hardware/actions/input.rs` |
| `handlers.rs` | `crates/sim-spatial/src/robot/hardware/handlers.rs` |
| `panel.rs`, `panel_sections.rs` | `crates/sim-spatial/src/robot/hardware/panel.rs`, `…/panel_sections.rs` |
| `view.rs`, `status.rs` | `crates/sim-spatial/src/robot/hardware/view.rs`, `…/view/status.rs` |
| `link.rs`, `session.rs` | `crates/sim-spatial/src/robot/hardware/link.rs`, `…/session.rs` |
| `buttons.rs`, `sequences.rs`, `periodic.rs`, `beat.rs` | `crates/sim-spatial/src/robot/hardware/session/*.rs` |
| `robot_actions.rs` | `crates/sim-spatial/src/robot/actions/mod.rs` (robot mode's `system_ui`) |
| `calibration.rs`, `http.rs` | `crates/sim-runtime/src/hardware_client/calibration.rs`, `…/http.rs` |
| `server.rs` | `crates/sim-runtime/examples/serve_actuator_calibration.rs` |
| `bus.rs` | `crates/sim-runtime/src/acquisition/calibration_serial.rs` (`CalibrationBus`) |

**Shared paths used by every step.**

- **Panel click.** `input.rs:13` `buttons` turns an `Activated` button
  into `Act::ui`, but never for a disabled one (`Enabled`). Next comes
  `actions.rs:576` `apply`, then `handlers.rs:164` `dispatch`, then
  `handlers.rs:384` `handle_inner`.
- **`system_ui` listing.** `robot_actions.rs:596-606` lists every
  `panel.rs:331` `controls` entry. Its `enabled` and `disabled_reason` combine
  `HardwareAction::authorize` (`actions.rs:307`) with the control's `ready`
  (`panel.rs:232` `control_list`).
- **`system_ui` activation.** `robot_actions.rs:557-595` runs `eligible`: the
  control exists and is ready, `authorize` passes, and no close is pending for
  motion, where JogRelease is exempt (`:564`). Over REST it goes through
  `Replies::submit` (`:584`) and is answered by the hardware reply. A one-way
  activation is written with `Origin::SystemUi` (`:592`).
- **Remote motion.** `dispatch` runs `authorize` (`actions.rs:307`, then
  `calibration.rs:43` `authorize_virtual`) and `remote_check`
  (`handlers.rs:27`). It assigns a ticket (`handlers.rs:203-209`). `send_with`
  then queues `LinkCommand::Checked` (`handlers.rs:74-89`). The link thread
  re-authorizes and validates it (`session.rs:566-617`, `validate_command`
  `:805`) and judges it by what it achieved (`run_checked` `:738`). REST waits
  on the recorded verdict (`handlers.rs:166-201`). On the virtual bench, every
  command except STOP carries `X-Calibration-Server/Bench/Generation`
  (`http.rs:110-120`).
- **Server.** `server.rs:1260` `handle` checks the token, then the identity
  and generation (`:1400-1419`; a newer generation latches STOP at
  `:1414-1417`), then `check_execution` (`:300`). STOP latches at once
  (`:1469-1480`). `capture_hold` and `motion_update` act on the live session
  (`:1422-1465`). Everything else is queued to `worker` (`:472`), which runs
  `observe_stop` first (`:525`, `:529`) and re-checks binding, expiry and STOP
  epoch (`:534-540`). Errors map to 409 only for the `BINDING_REFUSED` prefix,
  and everything else is 400 (`:1506-1516`).

### HW-01 Connect, identity, staleness, reconnect

- **Launch or Connect.** `--hardware` reaches `enter` (`actions.rs:465`) and
  then `connect` (`actions.rs:534`). The `hardware:connect` control
  (`panel.rs:247`) reaches `H::Connect` (`handlers.rs:399`) and then
  `connect`. `connect` first stops and drops any old link (`:538-544`) and
  takes a new generation (`:545`). The connect job (`:549-568`, `Pool::Dedicated`)
  then runs:
  - `GET /calibration/status` (`server.rs:1358`).
  - For a virtual identity: the client is pinned with
    `with_calibration_execution` (`http.rs:36`) and posts `inspect`
    (`calibration.rs:571`). On the server, `handle` records the generation
    (`server.rs:1411-1417`). The worker's inspect arm (`server.rs:594-619`)
    resyncs through `reconnect_stopped` on the first enabled motor (ID 3 if
    all are disabled; `:605-606`), reads the enabled motors
    (`:609-611`) and reports "Readback received…".
  - The job refuses if the identity or connection changed (`actions.rs:561-563`).
  - Otherwise the client stays unpinned (physical/unknown, `:566`).
- **Link.** `poll_jobs` (`actions.rs:685-701`) starts `Link::spawn`
  (`link.rs:351`), which runs `session::run` (`session.rs:89`). Status is
  polled by `periodic.rs:12` every 600 ms when idle and 150 ms in a session
  (`link.rs:53-55`).
- **Panel.**
  - Connection line (`panel.rs:438-446`): "url · link n · VIRTUAL simulated
    bench" or "physical or unknown execution", plus "· stale/reconnect
    required" when blocked.
  - Status "Choose the motor you want to calibrate." (`view.rs:307-308`).
    It is prefixed "VIRTUAL · simulated…" on a virtual link (`panel.rs:214-219`).
  - `hardware_status`: `connected`, `generation`, `stale`,
    `authorization_revoked` and `age_ms` (`status.rs:20-30`), plus
    `server.fidelity` (`status.rs:83`).
- **Stale status.** After 2.4 s without a read (`link.rs:65`) the view is
  blocked with "Status stale — last read n s ago" (`view.rs:492-498`).
  `poll_jobs` then revokes the generation permanently. This happens when the
  status is stale and no request is awaited, when the connection is invalid,
  or when authorization is already revoked. It sets `link.authorization` and
  STOPs (`actions.rs:748-754`). The panel then blocks with "Connection or
  execution identity lost; reconnect required…" (`panel.rs:211-213`). A
  remote `select` fails with "Virtual calibration authorization expired;
  reconnect explicitly" (`calibration.rs:51-52`). Reconnect brings a new
  generation, and nothing from the old one is reused (`actions.rs:545`).
- **409 vs 400.** The server answers 409 `BINDING_REFUSED` only for these
  cases:
  - an identity mismatch, or a missing or stale generation (`server.rs:1404-1413`,
    `:307-311`);
  - a lost virtual bench (`:302-304`);
  - no identity on a virtual server (`:318`).

  An out-of-scope command with a valid pin is an ordinary 400 that keeps the
  binding (`server.rs:314-316`, `out_of_scope` `:268`, allowlist
  `calibration.rs:75-80`). On the client, `binding_lost` (`calibration.rs:66-72`)
  is true only for transport, decode or 409 errors. `Session::send`
  (`session.rs:391-396`) and the poll (`periodic.rs:40-42`) then call
  `lose_binding` (`session.rs:349-352`). A 400 is only returned and shown.

  Separately, an adopted status whose execution changed or vanished revokes
  and STOPs (`session.rs:407-427`). It reads "the virtual calibration bench
  was lost (disconnected); reconnect required" when the execution vanished,
  and "virtual execution identity changed; reconnect required" when it was
  replaced. A virtual status with `connected: false` also revokes
  (`session.rs:430-434`).
- **Out of virtual scope** (the step's refusal rule). On a virtual link,
  `in_scope` (`panel.rs:240-243`) disables these with `OUT_OF_VIRTUAL_SCOPE`
  (`panel.rs:183`), for the operator too:
  - Flip (`panel.rs:302`);
  - Send raw step (`:309`);
  - the Leg and Both gait modes (`:289-292`);
  - a Leg/Both Play (`:294-297`).

  Remotely, `authorize` refuses Flip, RawStep, gait, live sync and binding
  changes with `remote_refusal` (`actions.rs:317-323`, `:334-339`). A request
  that still reached the server gets the 400 above, and the session is kept.

### HW-02 Select, disable, enable

- **Select.** The chip `hardware:select_<id>` (`panel.rs:248-250`, spawned at
  `panel_sections.rs:252`) sends `H::Select` (`handlers.rs:432`). From there
  `LinkCommand::Select` (`session.rs:620`) reaches `select_motor`
  (`session.rs:854-910`). For a disabled axis it STOPs and sends no select
  (`:856-864`). Otherwise it marks the session busy and `drove`, and posts
  `calibration::select` (`calibration.rs:579`).
  - **Server:** `handle` latches STOP for select (`server.rs:1466-1468`). The
    worker runs `observe_stop` and then the select arm (`server.rs:644-686`):
    `reconnect_stopped`, `prove_watchdogs`, hold-others proofs (`:658-667`)
    and `arm` (`:673`). It replies "Ready. Hold Q toward upper…" (`:680-684`).
  - **Session:** ready when `enabled_id` matches (`session.rs:886`). A remote
    select is achieved when that motor is ready, or chosen and disabled
    (`session.rs:754-757`). It is refused for an unknown motor
    (`session.rs:816-820`).
  - **Panel:** "Connecting and checking this motor at zero drive…" while busy,
    then the server message (`view.rs:305-311`).
- **Hold others / drive mode.**
  - `hardware:hold_others` (`panel.rs:253`) or REST `hold_others` reaches
    `handlers.rs:435-447`. A remote one is sent as `Inputs` and adopted on its
    verdict (`apply_resolved`, `handlers.rs:258-264`). It is sent with the
    next select (`session.rs:881`).
  - REST `drive_mode` goes through the same path (`handlers.rs:603-615`,
    `:265-270`).
- **Disable/Enable.** `hardware:set_disabled` (`panel.rs:251`) sends
  `H::SetDisabled` (`handlers.rs:433`). From there `session.rs:626` reaches
  `set_disabled` (`session.rs:1017-1031`), which STOPs first when disabling,
  and posts `calibration::set_disabled` (`calibration.rs:583`).
  - **Server:** the set_disabled arm (`server.rs:623-640`) refuses while the
    motor is enabled ("Stop this motor before disabling it"). It saves
    `calibration-<ms>.json` and replies "Motor disabled…" or "Motor enabled.
    Select it to reconnect." A disabled axis refuses motion (`server.rs:641-643`).
    Idle polling and inspect skip it (`:550`, `:609`).
  - **Panel:** the chip reads "⊘ Worm 2" (`view.rs:387`), and the status
    reads "{role} is disabled. Enable it to move it." (`view.rs:312-315`).

### HW-03 Hold-to-move (Q/A, buttons, `system_ui` toggles)

- **Press.** There are three ways in, all ending in `H::JogPress`:
  - Q/A keys: `input.rs:51-64` (panel open, no modifier, not typing).
  - Upper/Lower buttons: `input.rs:24-35`, with `JogButton` spawned at
    `panel_sections.rs:122`.
  - `system_ui`: `hardware:jog_upper`/`jog_lower` while not held, which list
    `JogPress` gated on `jog_enabled` (`panel.rs:259-268`).

  The handler (`handlers.rs:448-476`) requires a ready, non-busy motor. It
  sets the held flag and keeps the old value as `before`. It sends
  `LinkCommand::Press`, or `BothKeys` when the other direction is held. A
  press that was never queued restores the flag (`:462-466`). A remote press
  is recorded in `pending_presses` with `before` and `one_way`
  (`SystemUi`, `:470-474`). The remote gate is `remote_check`
  (`handlers.rs:39`, `:45-51`).
  - **Session:** `move_` (`session.rs:995`) calls `begin` (`:956-993`), which
    posts `calibration::motion_start` (`calibration.rs:587`). The server's
    `motion_start` arm (`server.rs:890-1041`) runs
    `controlled_motion_multi` (`bus.rs:643`). The heartbeat `motion_update`
    (`beat.rs:239-249`, `calibration.rs:591`) reaches the server's
    `BrowserSweep::update` (`server.rs:1457-1464`, `:151-174`) with motion
    "upper"/"lower" (`server.rs:271-281`). A remote press is achieved when a
    run exists with that intent (`session.rs:758-764`).
- **Release.** The key or button release (`input.rs:65-69`, `:30-34`) sends
  `H::JogRelease`. While a direction is held, the `system_ui` toggle lists
  `JogRelease` with `Ok(())` (`panel.rs:263-264`).
  - **Never refused:** `authorize` passes with only a link present
    (`actions.rs:314-316`). `remote_check` lets it through (`handlers.rs:36`).
    It is exempt from the close-pending refusal (`actions.rs:597`,
    `robot_actions.rs:564`).
  - **Handler** (`handlers.rs:477-493`): it clears the `before` of pending
    presses in that direction, clears the flag, and sends `LinkCommand::Release`.
    If the release cannot be queued, it STOPs on the immediate path instead.
  - **Session:** `checked_release` (`session.rs:709-719`, chosen at `:575`)
    skips the freshness and generation check. If the session may no longer
    be driven, it STOPs. Otherwise `release` (`session.rs:1009-1015`) sets
    intent hold, and `update` sends `motion_update` "hold". The server
    switches to `MotionCommand::Hold` (`server.rs:275`), which is an active
    hold, not torque-off.
- **Refused remote press.** A refusal is the verdict in `command_results`.
  `settle_presses` (`handlers.rs:294-342`) restores `before` for the latest
  press, so a newer operator press keeps its flag. A one-way refusal becomes
  the panel notice "jog upper press refused: …" (`:326-332`). An evicted
  unread verdict while still held triggers STOP (`:335`, `:339-341`). It runs
  for REST verdicts in `dispatch` (`handlers.rs:176`, `:198`) and each frame
  for one-way presses (`actions.rs:731-738`). STOP and loss clear what a
  pending refusal would restore (`release_holds`, `handlers.rs:348-354`).
- **Speed.** The slider (`input.rs:115`) or REST `speed` reaches
  `handlers.rs:494-511`, where `stepped` (`:99-104`) refuses values outside
  0–100 with "must be a number from". It sends `SpeedChanged`, and the session
  calls `update` (`session.rs:639-642`).
- **Panel and status:**
  - held highlight: `view.rs:457-458`;
  - speed text "x.xx°/s motor · limited to …": `view.rs:404-411`;
  - dial needles: `view.rs:461`;
  - chart: `view.rs:489`;
  - `hardware_status.form.held_upper/held_lower`: `status.rs:47`.

### HW-04 STOP from every section

- **Controls.**
  - Top bar "Z  Stop": `panel_sections.rs:92`, outside the scroll area.
  - Each section header's compact Stop: `panel_sections.rs:82`.
  - `hardware:stop` is always `Ok(())` (`panel.rs:246`).
  - Z/Escape: `input.rs:56-58`, not gated by a focused field; it needs the
    panel open and no modifier key held (`input.rs:55`).
  - REST `hardware_stop` maps to `H::Stop` (`actions.rs:377`).
  - Sections: `hardware:section_*` (`panel.rs:276-279`) reach
    `H::ToggleSection` (`handlers.rs:406-415`).
- **Never gated.** `Stop` is not motion (`actions.rs:295`). `remote_check`
  skips it (`handlers.rs:28`).
- **Handler.** `H::Stop` (`handlers.rs:417-422`) calls `operator_stop`
  (`actions.rs:507`), then `stop_with` (`:511-522`). That clears holds and
  both confirmations, then calls `link::stop_now` (`link.rs:456-468`): it
  bumps the shared epoch and posts `calibration::stop` (`calibration.rs:575`)
  on its own connection, id-less if no motor is known. STOP never carries
  identity headers (`http.rs:110-113`). The handler then sends
  `LinkCommand::Stopped { epoch }`. Live sync is stopped too (`:420`).
- **Server.**
  - `handle` takes no identity for stop (`server.rs:1400-1401`).
    `check_execution` returns `Ok` for stop first (`:301`), even on a lost
    bench or a mismatched pin.
  - `latch_stop` (`:338-344`) answers a copy with `stop_latched: true`,
    `enabled_id: null` and `busy: false` (`:1469-1480`).
  - The worker's `observe_stop` (`server.rs:403-471`) torques off every
    configured axis: `bus.rs:344` `stop` (3 attempts), or
    `stop_single_attempt` (`bus.rs:379`) for disabled axes. It then clears
    the selection and owner.
  - Running sweeps, tunes and campaigns see `app.cancel`/`app.stop`
    (`server.rs:967`, `:775`, `:2142`). A job captured before the STOP is
    refused with "STOP interrupted this command" (`:536`).
- **Session.** `Stopped` triggers `stopped_locally` (`session.rs:669-674`,
  `:826-837`): not ready, no run. `StopAnswered` (`actions.rs:713-717`) is
  adopted (`session.rs:675-681`). Requests queued before the STOP are not
  sent while it is pending (`session.rs:383-390`). A remote command queued
  before it is refused with `STOP_PENDING` (`:577-578`).
- **Readback loss on STOP** (`observe_stop`, `stop_outcome` `server.rs:377-387`):
  - **Lost readback on an enabled axis.** The bus forgets that axis's turns
    (`reset_turn_tracking_for`, `bus.rs:288`). If its poses or alignment are
    bound to the live session, `coordinate_session` is re-stamped
    (`server.rs:425-428`, `:454-456`). The bench is marked
    `connected: false` (`:457-460`). On a virtual bench a transport loss
    also drops the bus with `lose_bus` (`:439-449`, `:238-248`). That nulls
    `execution`, so every later command gets 409 "Virtual bench
    disconnected; restart and reconnect explicitly" (`:302-304`).
  - **Disabled motor.** It gets one attempt. Only its own zero-byte reply
    timeout counts as a note: " Note: disabled {role} (ID n): no readback
    expected (…)." (`:385`, `:433`, `:463`). Any other error is a failure.
    Its lost readback still resets its turns (`reset_turns`, `:386`), so
    if its poses are bound to the live session the shared session is
    re-stamped and every axis's multi-turn poses need re-teaching
    (protective; `:398-401`).
  - **Failure message.** "STOP latched; torque-off readback unverified (…).
    Cut motor power. Records retained." (`:466-468`).
  - **Success message.** "STOP latched; all configured axes torque off.
    Records retained." (`server.rs:25`, `:465`).
- **Panel.** The status line shows the server message (`view.rs:310`). After
  a re-stamp, a motor whose poses were multi-turn reads, once it is chosen
  again and ready (`view.rs:316-319`), "Saved poses are from an earlier
  session and are ignored until re-taught" and "Re-teach both
  poses" (`view.rs:280-284`, `:320-321`, `:469`). Range controls are off
  (`view.rs:415`). A virtual link revokes on `connected: false`, or on the
  vanished execution (`session.rs:407-434`). A physical link shows only the
  message: `hardware_status.connected` turns false (`status.rs:16`), but the
  panel has no separate disconnected marker (see the defects in the report).
  JogPress needs the motor chosen again (`handlers.rs:452`).

### HW-05 Focus loss, panel close, leaving Robot mode

- **Focus loss.** `WindowFocused` (`input.rs:77-88`) writes
  `Loss::FocusLost` (Quiet). The REST `loss {focus_lost}` is allowed because
  it is not motion. Both reach `H::Loss` (`handlers.rs:426-429`) and then
  `loss` (`handlers.rs:129-151`). If `drive_active` (`link.rs:321-323`), it
  calls `stop_immediate` (`actions.rs:501`), which is the STOP path of
  HW-04 without the operator flag. Otherwise it sends `LinkCommand::Loss`,
  and the session re-checks `drive_active` and runs `stop()`
  (`session.rs:683-687`, `:839-852`).
- **Close and toggle.** `hardware:close` / × (`panel_sections.rs:93`) sends
  `H::ClosePanel` (`handlers.rs:395-398`). `hardware:toggle_panel` sends
  `H::TogglePanel` (`handlers.rs:387-394`). Both call `close`
  (`handlers.rs:154-157`), which is `loss(PanelClosed)` plus `open = false`.
  `hardware_status.open` follows it (`status.rs:21`).
- **Leaving Robot mode.** `OnExit(Robot)` runs `leave` (`actions.rs:479-492`):
  it answers pending REST calls, calls `stop_immediate`, stops our live sync,
  and drops the state off the UI thread. Without the resource, `apply`
  refuses with "not open in this mode" (`actions.rs:586-588`). Re-entry runs
  `enter` again (`actions.rs:465-473`).
- **Window close.** `WindowCloseRequested` sends `Loss::Leaving` (Quiet,
  `input.rs:83-84`). That stops immediately and also calls
  `post_stop_sync` (`handlers.rs:137-145`, `link.rs:392-409`). AppExit runs
  `stop_on_exit` (`actions.rs:449-459`), and `Link::drop` also stops
  (`link.rs:419-425`). A remote `loss {leaving}` is refused
  (`handlers.rs:423-425`).
- **Server.** The same STOP effect as HW-04.

### HW-06 Teach lower, upper, alignment; targets; reset

- **Capture.** `hardware:capture_lower/upper/reference` (`panel.rs:270-272`)
  is enabled when `pose_enabled`: ready, hold intent, and the latest sample
  holding (`view.rs:412`). It sends `H::Capture` (`handlers.rs:520-526`),
  with the mirror alignment angle for reference. `LinkCommand::Capture`
  (`session.rs:651`) reaches `buttons.rs:40-56`.
  - **In a held session:** `capture_hold` (`calibration.rs:657`) is posted
    through the beat (`send_in_session`, `session.rs:256-278`). On the
    server, `handle` (`server.rs:1422-1441`) requires hold and no STOP, sets
    `ctl.capture` and answers `{"ok":true}` before anything is saved. The
    sample callback saves only when the sample is holding, under 2 counts/s,
    within 3 counts of target, and stable over 6 samples
    (`server.rs:987-990`). It then writes `calibration-<ms>.json` and reports
    `capture_message` "Saved {boundary} pose" (`:997-998`). Otherwise it
    reports "Still settling. Release Q/A, wait for Holding, then save the
    pose." (`:999`).
  - **Without a run:** `calibration::capture` (`calibration.rs:647`) reaches
    the server's capture arm (`server.rs:1130`).
  - **Panel:** the capture line (`view.rs:481`, `panel.rs:415`) and the pose
    captions "x.x° motor" (`view.rs:465`).
  - **Caveat:** a remote capture's verdict is Ok once `capture_hold` is
    accepted (`session.rs:792`). It does not prove the save. That is why the
    driver waits for a new calibration file.
- **Target.** The target slider (`input.rs:116`, `:128-130`) or REST `target`
  reaches `handlers.rs:512-518` (`stepped`, 0–100). It sends
  `LinkCommand::Target` to `buttons.rs:11-33`, which applies a 4-count inset
  inside the taught poses (`:27`), and then `begin`/`update` with motion
  "target" (`server.rs:278`). `target_commit` (`handlers.rs:519`) calls
  `update`. The remote gate is `target_enabled` (`handlers.rs:33`,
  `view.rs:415`). `validate_command` requires two poses more than 8 counts
  apart in the current session (`session.rs:809-815`).
- **Reset.** `hardware:reset_poses` (`panel.rs:274`) reaches
  `handlers.rs:527`, `session.rs:653-656` and `buttons.rs:58-77`: STOP, then
  `calibration::clear` (`calibration.rs:666`). The server latches STOP for
  clear (`server.rs:1466-1468`). The clear arm (`server.rs:713-743`) saves
  `…-before-clear.json` and the new calibration, then the session reselects.
- **Re-stamp invalidates poses.** `usable` (`server.rs:178-185`) drops the
  lower/upper of an axis whose `coordinate_session` is not the live one.
  Re-stamps come from STOP readback loss (`:454-456`), idle readback loss
  (`:566-568`), motion readback loss (`:1025-1031`) and `lose_bus` (`:244`).
  Only poses outside one revolution (0–4095) are bound to a session
  (`:993-994`). The panel shows "Saved poses are from an earlier session…"
  (`view.rs:280-284`, `:320-321`).

### HW-07 Try saved range, Learn, Sweep all

- **Sweep.** `hardware:sweep` (`panel.rs:273`, gated by `range_enabled`,
  `view.rs:415`) sends `H::Sweep` (`handlers.rs:530`) to `buttons.rs:94-117`.
  That is pause/hold when sweeping. Otherwise it begins a session and sets
  intent Sweep. It resets the speed to 0 (`:111-113`); the form follows in
  `poll_jobs` (`actions.rs:763-774`). On the server, `motion_update` "sweep"
  drives `controlled_motion_multi` inside the poses
  (`server.rs:966-1010`).
- **Learn.** `hardware:learn` (`panel.rs:275`) sends `H::Learn`
  (`handlers.rs:531`) to `buttons.rs:119-139`. The terminal adaptation is
  kept at `session.rs:533`. The panel shows "{status}. Stops learned: d / i.
  Allowed now: …" (`view.rs:442-445`), and `hardware_status` exposes
  `session.learning_terminal` (`status.rs:36-37`).
- **Sweep all.** `hardware:sweep_all` (`panel.rs:252`) sends `H::SweepAll`
  (`handlers.rs:434`) to `sequences.rs:18`, which posts
  `calibration::sweep_all` (`calibration.rs:597`). The server's
  `sweep_all` arm (`server.rs:890-1041`, `all`) holds each axis after 2 half
  cycles (`:965`, `:973-976`). Panel sequence texts: "Sweep-all: checking
  every enabled motor at zero drive…", "Swept … through their saved
  ranges." and "Sweep-all stopped: …" (`sequences.rs:39`, `:128`, `:86`,
  `:124`).
- **STOP and focus loss.** Same as HW-04 and HW-05. The server ends the
  sweep and records "Torque off and stationary encoder verified."
  (`server.rs:1011-1039`).

### HW-08 Tune

- **Confirm and tune.** `hardware:tune_confirm` (`panel.rs:280`) sets
  `H::TuneConfirm` (`handlers.rs:532-535`), a form flag only. `hardware:tune`
  (`panel.rs:281`) is enabled when ready and confirmed (`view.rs:476`). It
  sends `H::Tune` (`handlers.rs:536-541`) to `sequences.rs:137-172`: STOP,
  `select_motor(solo)`, `tune_stages` cleared, then `calibration::tune`
  (`calibration.rs:601`).
- **Server.** The tune arm (`server.rs:752-818`) refuses when the nearest
  saved pose is less than 100 counts away ("Only n counts … needs 100",
  `:761-765`). It arms and replies running (`:768-774`).
  `identify` follows `app.cancel` (`:775`). On success it writes
  `tune-<id>-<ms>.json` (`:785-791`), saves the gains, and reports "Tuned …
  Select it to use them." (`:799-809`). STOP cancels it; `tuning.error`
  becomes "Tuning stopped: {e}. Torque off; previous gains kept." (`:811-815`).
- **Session.** `tune_tick` (`sequences.rs:174-186`) ends it and bumps
  `tune_done`, and `poll_jobs` unticks the confirmation (`actions.rs:757-759`).
  STOP also clears it (`actions.rs:513`). The stages are collected in
  `adopt` (`session.rs:436-440`).
- **Panel.** "Tuning: {stage}", "Last tuning stopped: …" and "Tuned gains in
  use: kp …, ki …, kd …, friction …% · {record}" (`view.rs:417-429`). On a
  virtual link it is prefixed "VIRTUAL ·" with "Captured stages: …"
  (`panel.rs:216-217`).

### HW-09 Campaign and resume

- **Confirm and run.** `hardware:campaign_confirm` (`panel.rs:282`) sets
  `H::CampaignConfirm` (`handlers.rs:542-545`). `hardware:campaign` and
  `hardware:campaign_resume` (`panel.rs:284-285`) send
  `H::Campaign { resume }` (`handlers.rs:546-551`) to
  `sequences.rs:188-221`: STOP, `select_motor(hold_all)`, then
  `calibration::campaign` (`calibration.rs:605`).
- **Server.** The campaign arm (`server.rs:866-889`) calls `run_campaign`
  (`:2071`) and `campaign_directory` (`:2038-2069`). Resume reuses the
  receipts of the newest interrupted campaign and copies the old reports to
  `attempt-before-resume-<ms>`. Receipts are written as stages finish
  (`:2162`), and `completed` is counted (`:2176-2177`). STOP gives "Campaign
  stopped: {e}. Torque off; completed stages are kept as receipts (Resume
  continues)."
  (`:883-885`); finishing gives "Campaign finished: {…}. Results in {…}. Nothing
  was promoted to CAD." (`:878-880`).
- **Session.** `campaign_tick` (`sequences.rs:223-235`) ends it, and
  `poll_jobs` unticks the confirmation (`actions.rs:760-762`).
- **Panel.** "{stage} · n stage results saved · last stopped by {gate}",
  "Last campaign stopped: …" and "Finished: {headline}. {directory}"
  (`view.rs:430-441`), prefixed "VIRTUAL ·" on a virtual link
  (`panel.rs:218`).

### Export (HW-15; also keeps HW-06–HW-09 records)

- **Request.** `hardware:export` (`panel.rs:310-317`) or REST
  `hardware_export` sends `H::Export` (`handlers.rs:430`) to `export`
  (`handlers.rs:681-709`) and `start_export` (`:724-748`). The job runs
  `GET /calibration/export`, which the server answers with
  `export_document` (`server.rs:1373-1375`, `:253-265`). A virtual server
  adds `execution` and `"simulated": true`.
- **File.** `write_export` (`handlers.rs:765-796`) writes
  `viewer-exports/leg-calibration-<ms>[-virtual].json` with
  `create_new`, so a file is never overwritten.
- **Panel.** `poll_jobs` (`actions.rs:718-728`) prefixes the line with
  "VIRTUAL (simulated)" (`handlers.rs:712`), shown at `panel.rs:433`. REST
  answers `{path, simulated}` (`handlers.rs:684`).

**Hops not traced to a line.**
- The bench side of `hx_virtual_bench`: what the socket bench does on each
  frame.
- The inside of `controlled_motion_multi`, `identify` and `BusRig`
  (`bus.rs:643`, `:785`; `server.rs:2142`).
- How the UI kit turns a pointer press into `Activated`.

These traces stop at the call.

## Physical operator run sheets — HW-01–HW-09

These sheets are **unexecuted physical checks**, not an automated verification
recipe. Use the fixture/FPGA/power prerequisites above, with one client at a
time. Record the actual firmware, configuration, limits and output directory.
These checks load no image, raise no limit and promote no CAD or registry value.
The operator stays at the supported fixture throughout. Launch the separate Rust
calibration server first, then the native viewer with `--hardware`. The browser
page at the same URL is the reference comparison.

**How to stop, in every step:**

1. Press **Z**, **Escape** or the panel's top **Stop** button.
2. Confirm torque-off at the fixture and in the server status (no enabled motor).
3. If host STOP does not answer or the motor keeps moving, switch **motor power
   off** at the supply/power switch. The FPGA supervisor STOP and watchdog stay
   independent of the host.

Never rely on a GUI acknowledgement alone. Keep all partial records.

**What the STOP message tells you** (server `observe_stop`; see the HW-04 trace).
These messages are read from the source and have not been observed.

- **"STOP latched; all configured axes torque off. Records retained."** Every
  axis read back stationary.
- **"… torque-off readback unverified (…). Cut motor power. Records
  retained."** At least one axis did not confirm torque-off. **Switch motor
  power off at the supply now.** Do not retry from the GUI first.
- **"Note: disabled {role} (ID n): no readback expected (…)"** at the end of
  the message. A disabled motor did not answer its single STOP attempt, which
  is expected when it is absent. This is a note, not a failure, but if that
  motor has multi-turn poses, every motor's multi-turn poses must be
  re-taught (its turn count was reset). If a disabled
  motor *is* fitted and the message instead says torque-off is unverified,
  cut power.
- **Lost readback on STOP.** The server forgets that motor's encoder turns and
  marks the bench disconnected (`connected: false`). Poses taught beyond one
  revolution (multi-turn) are no longer used. The panel reads "Saved poses are
  from an earlier session and are ignored until re-taught", and Reset reads
  "Re-teach both poses". Re-teach them before any range motion. Single-turn
  poses (0–4095 counts) are kept.
  - Virtual link: the panel blocks with "Connection or execution identity
    lost; reconnect required".
  - Physical link: the panel shows only the message, and
    `hardware_status.connected` is false. Choose the motor again to reconnect
    the bench.
- **A refused command keeps the session.** On a virtual bench, a command
  outside its scope (Swap direction, raw step, Leg/Both gait) is disabled in
  the panel and refused as an ordinary error. The binding is kept. Only an
  identity, generation or lost-bench refusal (HTTP 409) ends the session, and
  then the panel asks for a reconnect.
- **Releasing a jog always works** while a link exists, even with a stale
  status, so Q/A, button and `system_ui` releases reach the server as a hold, or as
  a STOP if the session may no longer be driven. If a release cannot be
  queued, the panel STOPs instead.

| Step | What to do | What to expect | How to stop / when to stop |
|---|---|---|---|
| HW-01 | Connect with motors supported. Stop the server for 3 s while idle, restart it, press Connect. | Nothing is energized. Status goes stale, then fresh with a new link generation ("… · link n · physical or unknown execution"). No motor is ready after reconnect. | Z/Escape/Stop if anything energizes; power switch if it does not stop. |
| HW-02 | Choose Knee, Worm, Belt at zero drive. Disable Worm, choose it, then enable it. | The correct motor and readout. "⊘ Worm 2" while disabled, and choosing it sends a STOP but no select. A disabled motor cannot move. Enabling reads "Motor enabled. Select it to reconnect." | Stop after any wrong motor or readout. |
| HW-03 | At slow speed, hold/release Q and A and each jog button. Hold both directions. Release the pointer outside the button. | Correct direction. Encoder follows target. Holds (active hold, not torque-off) after release and with opposing inputs. | Stop if direction or hold differs; power switch if drive continues. |
| HW-04 | While jogging, and during a bounded sweep/tune/campaign, use Stop, Z and Escape. Scroll every section. | Stop never scrolls away. "STOP latched; all configured axes torque off." Session ends and needs a reselect. Partial records stay. | This step *is* the stop check. If the message says "readback unverified … Cut motor power", or any stop fails, switch motor power off. |
| HW-05 | During a jog and a saved-range sweep: switch focus to another app, close the panel, toggle the header button, leave Robot mode. Once, close the viewer. | Each one stops drive. Records stay. Reopening is idle. | Z/Escape/Stop, or the power switch, if drive continues after any interrupt. |
| HW-06 | Teach safe lower, upper and alignment while holding. Request targets inside the range. Reset and teach again. | Poses saved in this encoder session. Each save reads "Saved {pose} pose". Saving before the motor settles reads "Still settling. Release Q/A, wait for Holding, then save the pose.": wait and save again. Targets stay 4 counts inside the range. Reset clears the poses. | Stop on any travel toward a hard end. |
| HW-07 | Run saved range, then Pause. Learn to completion or pause. Teach two motors, Sweep all, then STOP a second run. | Travel stays inside the poses. Learned stop counts. Both ends reached per motor. STOP interrupts with records kept. | Stop on limits or feedback disagreement; power switch if a sweep keeps running. |
| HW-08 | Mid-travel with clearance, confirm and Tune. STOP one attempt, then confirm and Tune again. | Less than 100 counts to the nearest saved pose refuses the tune ("move toward the middle first"). The interrupted tune reads "Last tuning stopped: …" and the previous gains are kept. Terminal gains link to a record. Confirmation clears. Limits are unchanged. | Stop on unexpected travel. Tune holds no lease, so use STOP or the power switch. |
| HW-09 | Leg suspended. Confirm Campaign. STOP after a saved stage, confirm again, Resume. | STOP reads "Last campaign stopped: …"; completed stages are kept as receipts. Saved receipts are unchanged. Resume continues from them. A terminal report or an honest failure. "Nothing was promoted to CAD." | STOP ends drive and keeps progress. A campaign holds no lease, so use the power switch if STOP fails. |

The fuller steps above provide the browser comparison. Sign physical results
only after observing the fixture; virtual receipts cannot satisfy physical
expectations.

## Sign-off

Once every step above reads "same" (or its difference is accepted and noted),
the browser pages `calibration-ui.mjs`, `calibration-mirror.mjs`,
`actuator-motion-view.mjs` and `hardware-sync.mjs` may be retired in a later
batch. Until then they stay.

| | |
|---|---|
| Operator | |
| Date | |
| Viewer commit | |
| Server commit / FPGA profile | |
| Steps passed | HW-01 … HW-16: |
| Differences accepted | |
| Gait-lab requalification (HW-10) | run id / date: |
| Browser pages may be retired | [ ] yes  [ ] no |
