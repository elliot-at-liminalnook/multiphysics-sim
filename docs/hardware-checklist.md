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
| HW-03 | `hardware:jog_upper`, `hardware:jog_lower` | `speed {percent:25}`, `jog_release` | Encoder moves at least 60 counts each way. Hold after release. Opposing inputs hold within 32 counts for 1 s. | per-step state | `HW-03-held` |
| HW-04 | `hardware:section_tune/campaign/mirror/advanced`, `hardware:stop` | — | `hardware:stop` listed and enabled after each section toggle. STOP ends the jog: no ready session, `enabled_id: null`, not busy. | per-step state | `HW-04-stop-tune`, `HW-04-stop-campaign`, `HW-04-stop-advanced` (STOP placement is source review until a pass captures these) |
| HW-05 | `hardware:close`, `hardware:toggle_panel`, `mode:phenomena`, `mode:robot` | `loss {reason:"focus_lost"}` | Each interrupt stops drive. After close, `open: false`. After mode exit, direct server status has `enabled_id: null` and is not busy, and `hardware_status` is refused with "the active mode is phenomena". Robot re-entry reconnects idle and the form is set again. (Build needs a document, so it is not used.) | direct status polls in `http.jsonl` | — |
| HW-06 | `hardware:capture_lower/upper/reference`, `hardware:reset_poses`, `hardware:select_3` | `speed`, `target`, `target_commit` | After teaching, `speed`/`target` of −1 and 101 fail with "must be a number from". Targets of 25 % and 75 % are reached within 32 counts, and the commanded target lies inside the taught range minus 4 counts. Reference is non-null. Reset clears lower/upper. Motor 3 is taught. | per-step state | `HW-06-taught`, `HW-06-target`, `HW-06-reset` |
| HW-08 | `hardware:tune_confirm`, `hardware:tune`, `hardware:stop` | `target`, `target_commit` | STOP interrupts a tune. A fresh tune shows at least 2 observed stages, finite kp/ki/kd/friction, and a record file inside the output, and the confirmation clears. Repeated on motor 3. | `tune-stages.json`, tune records | `HW-08-stages`, `HW-08-terminal` |
| HW-07 | `hardware:sweep`, `hardware:learn`, `hardware:sweep_all`, `hardware:stop` | `speed`, `loss` | Saved-range travel stays inside the poses. Pause holds. Focus loss keeps the poses. `session.learning_terminal` shows completion with at least 3 stops each way, ending in hold. Sweep-all shows 2 half cycles on motors 2 and 3, then ends. A second sweep-all is stopped. | `learning-terminal.json`, `sweep-all-ends.json` | `HW-07-learned`, `HW-07-sweep-all` |
| HW-09 | `hardware:campaign_confirm`, `hardware:campaign`, `hardware:campaign_resume`, `hardware:stop` | — | A receipt is saved for each completed stage (`*.execution.json` provenance not counted). The count survives STOP. Receipt hashes are unchanged after resume. Terminal `report.json` is inside the output. Confirmation clears. | `records/` and `record_sha256` | `HW-09-saved`, `HW-09-stopped`, `HW-09-terminal` |
| LC1 | `hardware:connect`, second viewer `hardware:select_2` | `raw_step`, `gait_play`, `sync_start`, `select` | Out-of-scope actions get the remote-refusal text. Direct requests with no identity or a foreign identity get HTTP 409 "Calibration execution binding refused". STOP with the same headers returns `stop_latched: true`, `enabled_id: null`, `busy: false`. After a real server restart, the new UUID is seen, old headers get 409, and native refuses ("authorization expired") until reconnect. FIXTURE phase only: a second viewer behind the proxy presented as physical, then unknown, gets "Remote calibration requires a verified virtual" with zero crossings. | `replacement-execution.json`, `identity-fixture.jsonl`, `viewer-fixture-*.json` | — |

Cleanup runs even after a failure:

- The direct server STOP is decided by ownership of the server process and
  port, sends no identity headers, and is asserted latched.
- Native `hardware_stop` goes to each owned viewer.
- Only the driver's own children are terminated.

A timed-out job is cancelled with `DELETE /v1/jobs/<id>` before the direct STOP.

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

| Step | What to do | What to expect | How to stop / when to stop |
|---|---|---|---|
| HW-01 | Connect with motors supported. Stop the server for 3 s while idle, restart it, press Connect. | Nothing is energized. Status goes stale, then fresh with a new link generation. No motor is ready after reconnect. | Z/Escape/Stop if anything energizes; power switch if it does not stop. |
| HW-02 | Choose Knee, Worm, Belt at zero drive. Disable Worm, choose it, then enable it. | The correct motor and readout. A disabled motor cannot move. Enabling restores it. | Stop after any wrong motor or readout. |
| HW-03 | At slow speed, hold/release Q and A and each jog button. Hold both directions. Release the pointer outside the button. | Correct direction. Encoder follows target. Holds after release and with opposing inputs. | Stop if direction or hold differs; power switch if drive continues. |
| HW-04 | While jogging, and during a bounded sweep/tune/campaign, use Stop, Z and Escape. Scroll every section. | Stop never scrolls away. Torque off. Session ends and needs a reselect. Partial records stay. | This step *is* the stop check; power switch if any stop fails. |
| HW-05 | During a jog and a saved-range sweep: switch focus to another app, close the panel, toggle the header button, leave Robot mode. Once, close the viewer. | Each one stops drive. Records stay. Reopening is idle. | Z/Escape/Stop, or the power switch, if drive continues after any interrupt. |
| HW-06 | Teach safe lower, upper and alignment while holding. Request targets inside the range. Reset and teach again. | Poses saved in this encoder session. Targets stay 4 counts inside the range. Capture is refused while moving. Reset clears the poses. | Stop on any travel toward a hard end. |
| HW-07 | Run saved range, then Pause. Learn to completion or pause. Teach two motors, Sweep all, then STOP a second run. | Travel stays inside the poses. Learned stop counts. Both ends reached per motor. STOP interrupts with records kept. | Stop on limits or feedback disagreement; power switch if a sweep keeps running. |
| HW-08 | Mid-travel with clearance, confirm and Tune. STOP one attempt, then confirm and Tune again. | The interrupted record is kept. Terminal gains link to a record. Confirmation clears. Limits are unchanged. | Stop on unexpected travel. Tune holds no lease, so use STOP or the power switch. |
| HW-09 | Leg suspended. Confirm Campaign. STOP after a saved stage, confirm again, Resume. | Saved receipts are unchanged. Resume continues from them. A terminal report or an honest failure. No promotion. | STOP ends drive and keeps progress. A campaign holds no lease, so use the power switch if STOP fails. |

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
