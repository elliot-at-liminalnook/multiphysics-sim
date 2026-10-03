# Hardware front end: the operator's checklist

This is the pending operator checklist for the native hardware front end in
[architecture/native-viewer.md](architecture/native-viewer.md) §8. The
LC1–LC3 batch covers **HW-01–HW-09**. The native-leg-clock-alignment batch
(`29aa81ac` server, `313964e3` viewer, `80e4de27` driver) adds a written virtual
acceptance path and reading traces for **HW-10 and HW-11**
([Reading traces — HW-10/HW-11](#reading-traces--hw-10hw-11-by-reading-unexecuted)).
Both are still **unexecuted**: neither the driver nor a physical pass has run
them. HW-12–HW-16 remain reference and separate operator checks. The accepted verification repairs `ee17ef00` through
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

**HW-10/HW-11, 2026-10-02 (after `29aa81ac`, `313964e3`, `80e4de27`).** There
is no executed evidence for HW-10 or HW-11 in the native viewer either. The
virtual steps HW-10a-sim, HW-10b-leg, HW-10b-both and HW-11 are written in the
driver and have never run; the new Rust and Python tests in those commits are
written and have never run. The physical Leg only and Both runs still need the
gait-lab requalification first (HW-10). Every HW-10/HW-11 trace was made by
reading the source at `80e4de27`.

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
generation; physical/unknown endpoints still refuse automation. Since `313964e3`
gait playback (select, mode, speed, effort, confirmation, Play) is in the virtual
scope; on a physical or unknown endpoint it is refused remotely like everything
else. No raw-step, flip, live-sync, mirror-binding or general remote-motion
override is included. STOP remains available
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
"After any library source change"). **Do not run Leg only or Both on the
physical leg until it is requalified.** The viewer and server do not check the
requalification; this gate is procedural. Sim only is safe at any time, and so
is Leg/Both on a verified virtual bench (its results are labelled simulated and
are not leg results).

- **Native:** open Gait playback (the list loads). Pick a gait, keep **Sim
  only**, press **Play**, **Pause**, **Resume**, move Playback speed, press the
  gait's **Stop**. After requalifying: tick the suspended-leg confirmation,
  choose **Leg only** at 50 % effort and a low playback speed, Play, then
  Stop. Then **Both**.
- **Expect:** Sim only animates the suspended robot: "Sim only · gait time t s
  of P s period · n% speed". Leg only drives the aligned, taught, enabled
  motors ("Not driven: …" lists the rest, with reasons), shows "Limits: …",
  "Leg: {phase} · error … counts", and the statistics table after the run.
  Recent leg runs gains a row. Both ("Sim + leg · …") shows the real leg
  (blue) on the encoders while the simulated legs follow the leg's own gait
  clock: the server's gait time, interpolated at most 150 ms ahead between
  reads and frozen while the data is not live. The radios and Leg effort are
  locked while it plays. Stop stops drive. A Leg/Both start the server
  refuses releases the motor Play selected (STOP is sent; the gait line reads
  "gait_start refused: …; motor N released (STOP sent: …)"; since 2026-10-03,
  by reading, unexecuted). The browser leaves that motor held: mark it as a
  deliberate difference (ledger CAL-157), not a failure.
- **On a verified virtual bench** (since `313964e3`) Leg only and Both run too.
  The Leg line reads "VIRTUAL (simulated) · Leg: {phase} · error … counts", and
  the new Recent leg runs row is headed "VIRTUAL (simulated) · …". A physical
  run is not labelled.
- **Stale or disconnected data is never shown as live.** When the status goes
  stale the gait line reads "… gait time t s (clock frozen) …" with "Leg data
  stale — last read N s ago; not live" under it, and the mirror says the same
  before its last reading. A lost link reads "Leg disconnected — {why}; not
  live", and the status line starts "DISCONNECTED — …".
- **Browser:** the same gait, mode, effort and speed.
- **Result:** [ ] same  [ ] differs: ______

### HW-11 Mirror alignment

- **Native:** in Simulated leg mirror, tick "Show the real leg on the
  suspended simulated robot", choose the leg (+X by default), and check each
  motor's row (joint, sign, alignment pose: Mid-travel for the foot, CAD home
  for the others). Move each motor until the real leg matches the simulated
  leg's alignment pose and press **Save sim alignment here** for it. Then move
  each with Q/A. Try **Run**, **Step** and **Reset** in the robot header.
- **Expect:** the robot is held 0.25 m up with its body still. The chosen leg
  is tinted blue and follows the encoders: "{role}: x.x° from its alignment
  pose". An unaligned motor reads "not aligned — shown at {pose}". Save sim
  alignment answers only after the server saved the pose (since `29aa81ac`);
  right after it the mirror reads "{role}: 0.0° from its alignment pose" (a
  later poll may differ by the hold's jitter, a few tenths of a degree).
  Moving past a CAD limit adds "· Beyond CAD limit: …" (only the mirrored
  leg's joints). Flipping a sign reverses the simulated joint. Run, Step
  and Reset are refused while mirroring, by name (Step and Reset since
  2026-10-03, by reading, unexecuted; the browser refuses only Play, so mark
  Step and Reset as a deliberate difference, ledger MIR-38). Unticking ends the display. Stale data reads "Leg data
  stale — last read N s ago; not live · last reading: …"; a lost link reads
  "Leg disconnected — {why}; not live".
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
connected and no motor chosen. The virtual allowlist is a deliberate
exception for verified virtual HW-01–HW-09 and HW-10 gait playback only; the future driver exercises that
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
  gait intents and `gait_play` are refused here like any motion; mirror
  binding changes, flip, raw step and live sync are outside the remote
  allowlist on every link. `gait_stop` is never refused. `loss:leaving` is refused; use `hardware_stop` instead.
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

## Virtual acceptance map — HW-01–HW-11 (written, not executed)

This table is the contract the future driver checks against
[tools/native-calibration/acceptance.py](../tools/native-calibration/acceptance.py).
Everything in it is written: the driver and `fixtures.py` exist, but none of it
has run. The HW-10/HW-11 rows (`80e4de27`) run after HW-09 and before LC1
(`acceptance.py:1321-1322`). They use the virtual bench only: they are not the
physical HW-10, which still needs the gait-lab requalification and an operator.
They take no screenshots. Control ids are `system_ui` ids (`hardware:<name>`, or `mode:<mode>`).
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
| HW-10a-sim (`acceptance.py:1416-1446`) | `hardware:stop`, `hardware:gait_select_0` (activates `gait_select`), `hardware:gait_mode_sim`, `hardware:gait_play` (label Play, then Pause, then Resume), `hardware:gait_stop` | `hardware_gaits`; `gait_speed {percent:100}`, then `{percent:50}`; `gait_effort {percent:50}` | The gait list is non-empty and the panel lists at least one option. With gait 0 chosen and nothing playing, Play gives `session.gait` mode `sim`, `playing` true and `leg` false (30 s). `t` advances by 0.5 s within 5 s. Pause gives `playing` false, and `t` is exactly unchanged 0.6 s later. Resume gives `playing` and a larger `t`. `gait_speed` 50 gives `session.gait.scale` 0.5. The gait's Stop leaves `session.gait` null. | `hw10-gaits.json`; `sim_times` in `results.json` | — |
| HW-10b-leg (`acceptance.py:1452-1466`, `leg_gait` `:981-1025`) | `hardware:select_3/2`, `hardware:reset_poses`, `hardware:jog_upper/lower` toggles, `hardware:capture_lower/upper/reference`, `hardware:mirror_on`, `hardware:gait_mode_leg`, `hardware:gait_confirm`, `hardware:gait_play`, `hardware:gait_stop` | `speed {percent:80}`, `target {percent:50}`, `target_commit`, `gait_speed {percent:25}`, `gait_effort {percent:50}` | Preconditions (`wide_teaching` `:925`, `mirror_loaded` `:942`, `align_mid` `:953`). Motors 3 then 2 are reset and re-taught wide: lower after a 1600-count jog down, upper after a 3200-count jog up, at least 2800 counts apart. The mirror is enabled and shown with its model loaded (60 s). Each motor is aligned at its 50 % target with a finite `reference_joint_rad`. Motor 2 is selected with `hold_others` true and no "watchdog check failed". Then Leg only, 25 % speed, 50 % effort, confirmed. Play's own answer must be OK (50 s); a refusal is retained, marked misfit or not, and fails the step. Within 60 s: `server.gait.running` with phase `playing`, `session.gait` mode `leg` with `leg` true, and the panel's Leg line labelled "VIRTUAL (simulated)" with phase `playing` and "error … counts". The driven roles are not in `skipped`. The gait's Stop gives, within 30 s, no server gait running, `session.gait` null, and changed Recent leg runs headings with a "VIRTUAL (simulated) · " row. Then drive is stopped. | `hw10-leg.json` (phases seen, skipped, Leg line, server gait, run headings), `hw10-leg-refused.json` on refusal, `captures.jsonl` | — |
| HW-10b-both (`acceptance.py:1468-1477`) | `hardware:select_2`, `hardware:gait_mode_both`, `hardware:gait_confirm`, `hardware:gait_play`, `hardware:gait_stop`, `hardware:gait_mode_sim`, `hardware:mirror_off` | `gait_speed`, `gait_effort` (25 % / 50 %, then 100 % / 50 %) | As HW-10b-leg with mode `both`, plus `mirror.gait` true and `link_state` `live` while playing. `session.gait.t` (the one leg clock) increases between two reads while live (10 s). Afterwards the form is left Sim only at 100 %, the confirmation unticked, and the mirror not enabled and not shown (15 s). | `hw10-both.json` (as Leg, plus the two `t` values) | — |
| HW-11 (`acceptance.py:1479-1510`) | `hardware:mirror_on`, `hardware:select_2`, `hardware:capture_reference`, `hardware:jog_upper` toggle, `hardware:mirror_off`, `hardware:stop` | `target {percent:50}`, `target_commit`; `robot_run {action:"start"}`, `{action:"step"}`, `{action:"reset"}` (each must be refused) | The mirror is shown with its model loaded. Motor 2 reaches its 50 % target within 32 counts and holds. Save sim alignment is judged by its own answer (`capture` `:873-909`): OK, with the reference in the answered status and a new `records/calibration-*.json`. Within 10 s `mirror.leg_data` is `live` and the worm's "x.x° from its alignment pose" is within 0.3° of zero. After a 100-count jog up, within 10 s, it is at least 1.0° from zero. `robot_run` start, step and reset are each refused with "Run refused", "Step refused" or "Reset refused" and the `MIRRORING` text (step and reset added 2026-10-03, written, never executed). Mirror off hides it. STOP leaves no enabled motor. | `mirror` in `results.json` (role, line after save, line after jog, Run refusal, and `run_refusals` for start, step and reset), `captures.jsonl` | — |
| LC1 | `hardware:connect`, second viewer `hardware:select_2` | `raw_step`, `flip`, `sync_start` (since `80e4de27`; `gait_play` is now in scope, `acceptance.py:1327-1330`), `select` | Out-of-scope actions get the native remote-refusal text (`remote_refusal`). The server's own 400 for out-of-scope commands with a valid pin is covered only by written unit tests, not by this driver. Direct requests with no identity or a foreign identity get HTTP 409 "Calibration execution binding refused". STOP with the same headers returns `stop_latched: true`, `enabled_id: null`, `busy: false`. After a real server restart, the new UUID is seen, old headers get 409, and native refuses ("authorization expired") until reconnect. FIXTURE phase only: a second viewer behind the proxy presented as physical, then unknown, gets "Remote calibration requires a verified virtual" with zero crossings. | `replacement-execution.json`, `identity-fixture.jsonl`, `viewer-fixture-*.json` | — |

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
reference until a pass runs. The traces were written against `f5b48842`
(`server.rs` against `ef504937`); every `file:line` citation in them, and in
the HW-10/HW-11 traces, the virtual acceptance map and the run sheets, was
re-checked against the working tree on 2026-10-03 (see the citation note
below), so the lines now point at the current code. The
[HW-10/HW-11 traces](#reading-traces--hw-10hw-11-by-reading-unexecuted) use
the current lines throughout.

**Citation note (2026-10-03, by script and reading, nothing executed).** A
script extracted every `file:line` and `file:a-b` citation in this file (a
bare `:N` resolved to the last file named in the same bullet), read the
cited lines at the commit each section was written against (`ef504937` for
the HW-01–HW-09 traces, `6d449bda` for the rest and the "current lines"
bullets) and moved each to where the same lines are in the working tree by
a line diff; changed or ambiguous ones were fixed by hand. 711 citations
checked, 369 fixed (343 by the diff, 26 by hand), 342 already current. Lines
in `robot/actions/mod.rs` may move again with concurrent Robot-mode work.

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
  `actions.rs:619` `apply`, then `handlers.rs:165` `dispatch`, then
  `handlers.rs:385` `handle_inner`.
- **`system_ui` listing.** `robot_actions.rs:692-702` lists every
  `panel.rs:339` `controls` entry. Its `enabled` and `disabled_reason` combine
  `HardwareAction::authorize` (`actions.rs:312`) with the control's `ready`
  (`panel.rs:240` `control_list`).
- **`system_ui` activation.** `robot_actions.rs:653-691` runs `eligible`: the
  control exists and is ready, `authorize` passes, and no close is pending for
  motion, where JogRelease is exempt (`:660`). Over REST it goes through
  `Replies::submit` (`:680`) and is answered by the hardware reply. A one-way
  activation is written with `Origin::SystemUi` (`:688`).
- **Remote motion.** `dispatch` runs `authorize` (`actions.rs:312`, then
  `calibration.rs:43` `authorize_virtual`) and `remote_check`
  (`handlers.rs:28`). It assigns a ticket (`handlers.rs:204-210`). `send_with`
  then queues `LinkCommand::Checked` (`handlers.rs:75-90`). The link thread
  re-authorizes and validates it (`session.rs:623-674`, `validate_command`
  `:886`) and judges it by what it achieved (`run_checked` `:795`). REST waits
  on the recorded verdict (`handlers.rs:167-202`). On the virtual bench, every
  command except STOP carries `X-Calibration-Server/Bench/Generation`
  (`http.rs:110-120`).
- **Server.** `server.rs:1381` `handle` checks the token, then the identity
  and generation (`:1521-1540`; a newer generation latches STOP at
  `:1535-1538`), then `check_execution` (`:392`). STOP latches at once
  (`:1600-1611`). `capture_hold` and `motion_update` act on the live session
  (`:1543-1596`). Everything else is queued to `worker` (`:564`), which runs
  `observe_stop` first (`:617`, `:621`) and re-checks binding, expiry and STOP
  epoch (`:626-632`). Errors map to 409 only for the `BINDING_REFUSED` prefix,
  and everything else is 400 (`:1637-1647`).

### HW-01 Connect, identity, staleness, reconnect

- **Launch or Connect.** `--hardware` reaches `enter` (`actions.rs:490`) and
  then `connect` (`actions.rs:569`). The `hardware:connect` control
  (`panel.rs:255`) reaches `H::Connect` (`handlers.rs:400`) and then
  `connect`. `connect` first stops and drops any old link (`actions.rs:577-583`) and
  takes a new generation (`:584`). The connect job (`:588-607`, `Pool::Dedicated`)
  then runs:
  - `GET /calibration/status` (`server.rs:1479`).
  - For a virtual identity: the client is pinned with
    `with_calibration_execution` (`http.rs:36`) and posts `inspect`
    (`calibration.rs:588`). On the server, `handle` records the generation
    (`server.rs:1532-1538`). The worker's inspect arm (`server.rs:686-711`)
    resyncs through `reconnect_stopped` on the first enabled motor (ID 3 if
    all are disabled; `:697-698`), reads the enabled motors
    (`:701-703`) and reports "Readback received…".
  - The job refuses if the identity or connection changed (`actions.rs:600-602`).
  - Otherwise the client stays unpinned (physical/unknown, `:605`).
- **Link.** `poll_jobs` (`actions.rs:728-753`) starts `Link::spawn`
  (`link.rs:460`), which runs `session::run` (`session.rs:93`). Status is
  polled by `periodic.rs:12` every 600 ms when idle and 150 ms in a session
  (`link.rs:53-55`).
- **Panel.**
  - Connection line (`panel.rs:446-454`): "url · link n · VIRTUAL simulated
    bench" or "physical or unknown execution", plus "· stale/reconnect
    required" when blocked.
  - Status "Choose the motor you want to calibrate." (`view.rs:326-327`).
    It is prefixed "VIRTUAL · simulated…" on a virtual link (`panel.rs:222-227`).
  - `hardware_status`: `connected`, `generation`, `stale`,
    `authorization_revoked` and `age_ms` (`status.rs:32-44`), plus
    `server.fidelity` (`status.rs:97`).
- **Stale status.** After 2.4 s without a read (`link.rs:65`) the view is
  blocked with "Status stale — last read n s ago" (`view.rs:511-517`).
  `poll_jobs` then revokes the generation permanently. This happens when the
  status is stale and no request is awaited, when the connection is invalid,
  or when authorization is already revoked. It sets `link.authorization` and
  STOPs (`actions.rs:800-806`). The panel then blocks with "Connection or
  execution identity lost; reconnect required…" (`panel.rs:213-221`). A
  remote `select` fails with "Virtual calibration authorization expired;
  reconnect explicitly" (`calibration.rs:51-52`). Reconnect brings a new
  generation, and nothing from the old one is reused (`actions.rs:584`).
- **409 vs 400.** The server answers 409 `BINDING_REFUSED` only for these
  cases:
  - an identity mismatch, or a missing or stale generation (`server.rs:1525-1534`,
    `:399-403`);
  - a lost virtual bench (`:394-396`);
  - no identity on a virtual server (`:410`).

  An out-of-scope command with a valid pin is an ordinary 400 that keeps the
  binding (`server.rs:406-408`, `out_of_scope` `:360`, allowlist
  `calibration.rs:79-85`). On the client, `binding_lost` (`calibration.rs:66-72`)
  is true only for transport, decode or 409 errors. `Session::send`
  (`session.rs:405-410`) and the poll (`periodic.rs:40-42`) then call
  `lose_binding` (`session.rs:359-366`). A 400 is only returned and shown.

  Separately, an adopted status whose execution changed or vanished revokes
  and STOPs (`session.rs:434-465`). It reads "the virtual calibration bench
  was lost (disconnected); reconnect required" when the execution vanished,
  and "virtual execution identity changed; reconnect required" when it was
  replaced. A virtual status with `connected: false` also revokes
  (`session.rs:468-472`).
- **Out of virtual scope** (the step's refusal rule; current lines at
  `80e4de27`). On a virtual link, `in_scope` (`panel.rs:251`) disables these
  with `OUT_OF_VIRTUAL_SCOPE` (`panel.rs:185`), for the operator too:
  - Flip (`panel.rs:310`);
  - Send raw step (`:317`).

  Since `313964e3` the Leg and Both gait modes and their Play are in scope
  (`panel.rs:298-305`; the server's allowlist adds `gait_start` and
  `gait_update`, `calibration.rs:79-84`). Remotely, `authorize_with` refuses
  Flip, RawStep, live sync and the mirror's binding changes with
  `remote_refusal` (`actions.rs:341-349`). A request that still reached the
  server gets the 400 above, and the session is kept.

### HW-02 Select, disable, enable

- **Select.** The chip `hardware:select_<id>` (`panel.rs:256-258`, spawned at
  `panel_sections.rs:252`) sends `H::Select` (`handlers.rs:433`). From there
  `LinkCommand::Select` (`session.rs:677`) reaches `select_motor`
  (`session.rs:939-995`). For a disabled axis it STOPs and sends no select
  (`:941-949`). Otherwise it marks the session busy and `drove`, and posts
  `calibration::select` (`calibration.rs:596`).
  - **Server:** `handle` latches STOP for select (`server.rs:1597-1599`). The
    worker runs `observe_stop` and then the select arm (`server.rs:736-778`):
    `reconnect_stopped`, `prove_watchdogs`, hold-others proofs (`:750-759`)
    and `arm` (`:765`). It replies "Ready. Hold Q toward upper…" (`:772-776`).
  - **Session:** ready when `enabled_id` matches (`session.rs:971`). A remote
    select is achieved when that motor is ready, or chosen and disabled
    (`session.rs:813-816`). It is refused for an unknown motor
    (`session.rs:897-905`).
  - **Panel:** "Connecting and checking this motor at zero drive…" while busy,
    then the server message (`view.rs:324-330`).
- **Hold others / drive mode.**
  - `hardware:hold_others` (`panel.rs:261`) or REST `hold_others` reaches
    `handlers.rs:436-448`. A remote one is sent as `Inputs` and adopted on its
    verdict (`apply_resolved`, `handlers.rs:259-265`). It is sent with the
    next select (`session.rs:966`).
  - REST `drive_mode` goes through the same path (`handlers.rs:614-626`,
    `:266-271`).
- **Disable/Enable.** `hardware:set_disabled` (`panel.rs:259`) sends
  `H::SetDisabled` (`handlers.rs:434`). From there `session.rs:683` reaches
  `set_disabled` (`session.rs:1102-1116`), which STOPs first when disabling,
  and posts `calibration::set_disabled` (`calibration.rs:600`).
  - **Server:** the set_disabled arm (`server.rs:715-732`) refuses while the
    motor is enabled ("Stop this motor before disabling it"). It saves
    `calibration-<ms>.json` and replies "Motor disabled…" or "Motor enabled.
    Select it to reconnect." A disabled axis refuses motion (`server.rs:733-735`).
    Idle polling and inspect skip it (`:642`, `:701`).
  - **Panel:** the chip reads "⊘ Worm 2" (`view.rs:406`), and the status
    reads "{role} is disabled. Enable it to move it." (`view.rs:331-334`).

### HW-03 Hold-to-move (Q/A, buttons, `system_ui` toggles)

- **Press.** There are three ways in, all ending in `H::JogPress`:
  - Q/A keys: `input.rs:51-64` (panel open, no modifier, not typing).
  - Upper/Lower buttons: `input.rs:24-35`, with `JogButton` spawned at
    `panel_sections.rs:122`.
  - `system_ui`: `hardware:jog_upper`/`jog_lower` while not held, which list
    `JogPress` gated on `jog_enabled` (`panel.rs:267-276`).

  The handler (`handlers.rs:449-477`) requires a ready, non-busy motor. It
  sets the held flag and keeps the old value as `before`. It sends
  `LinkCommand::Press`, or `BothKeys` when the other direction is held. A
  press that was never queued restores the flag (`:463-467`). A remote press
  is recorded in `pending_presses` with `before` and `one_way`
  (`SystemUi`, `:471-475`). The remote gate is `remote_check`
  (`handlers.rs:40`, `:46-52`).
  - **Session:** `move_` (`session.rs:1080`) calls `begin` (`:1041-1078`), which
    posts `calibration::motion_start` (`calibration.rs:604`). The server's
    `motion_start` arm (`server.rs:995-1162`) runs
    `controlled_motion_multi` (`bus.rs:643`). The heartbeat `motion_update`
    (`beat.rs:239-249`, `calibration.rs:608`) reaches the server's
    `BrowserSweep::update` (`server.rs:1588-1595`, `:222-245`) with motion
    "upper"/"lower" (`server.rs:363-373`). A remote press is achieved when a
    run exists with that intent (`session.rs:817-823`).
- **Release.** The key or button release (`input.rs:65-69`, `:30-34`) sends
  `H::JogRelease`. While a direction is held, the `system_ui` toggle lists
  `JogRelease` with `Ok(())` (`panel.rs:271-272`).
  - **Never refused:** `authorize` passes with only a link present
    (`actions.rs:338-340`). `remote_check` lets it through (`handlers.rs:37`).
    It is exempt from the close-pending refusal (`actions.rs:640`,
    `robot_actions.rs:660`).
  - **Handler** (`handlers.rs:478-494`): it clears the `before` of pending
    presses in that direction, clears the flag, and sends `LinkCommand::Release`.
    If the release cannot be queued, it STOPs on the immediate path instead.
  - **Session:** `checked_release` (`session.rs:766-776`, chosen at `:632`)
    skips the freshness and generation check. If the session may no longer
    be driven, it STOPs. Otherwise `release` (`session.rs:1094-1100`) sets
    intent hold, and `update` sends `motion_update` "hold". The server
    switches to `MotionCommand::Hold` (`server.rs:367`), which is an active
    hold, not torque-off.
- **Refused remote press.** A refusal is the verdict in `command_results`.
  `settle_presses` (`handlers.rs:295-343`) restores `before` for the latest
  press, so a newer operator press keeps its flag. A one-way refusal becomes
  the panel notice "jog upper press refused: …" (`:327-333`). An evicted
  unread verdict while still held triggers STOP (`:336`, `:340-342`). It runs
  for REST verdicts in `dispatch` (`handlers.rs:177`, `:199`) and each frame
  for one-way presses (`actions.rs:783-790`). STOP and loss clear what a
  pending refusal would restore (`release_holds`, `handlers.rs:349-355`).
- **Speed.** The slider (`input.rs:115`) or REST `speed` reaches
  `handlers.rs:495-512`, where `stepped` (`:100-105`) refuses values outside
  0–100 with "must be a number from". It sends `SpeedChanged`, and the session
  calls `update` (`session.rs:696-699`).
- **Panel and status:**
  - held highlight: `view.rs:476-477`;
  - speed text "x.xx°/s motor · limited to …": `view.rs:423-430`;
  - dial needles: `view.rs:480`;
  - chart: `view.rs:508`;
  - `hardware_status.form.held_upper/held_lower`: `status.rs:61`.

### HW-04 STOP from every section

- **Controls.**
  - Top bar "Z  Stop": `panel_sections.rs:92`, outside the scroll area.
  - Each section header's compact Stop: `panel_sections.rs:82`.
  - `hardware:stop` is always `Ok(())` (`panel.rs:254`).
  - Z/Escape: `input.rs:56-58`, not gated by a focused field; it needs the
    panel open and no modifier key held (`input.rs:55`).
  - REST `hardware_stop` maps to `H::Stop` (`actions.rs:402`).
  - Sections: `hardware:section_*` (`panel.rs:285-288`) reach
    `H::ToggleSection` (`handlers.rs:407-416`).
- **Never gated.** `Stop` is not motion (`actions.rs:300`). `remote_check`
  skips it (`handlers.rs:29`).
- **Handler.** `H::Stop` (`handlers.rs:418-423`) calls `operator_stop`
  (`actions.rs:532`), then `stop_with` (`:536-547`). That clears holds and
  both confirmations, then calls `link::stop_now` (`link.rs:565-577`): it
  bumps the shared epoch and posts `calibration::stop` (`calibration.rs:592`)
  on its own connection, id-less if no motor is known. STOP never carries
  identity headers (`http.rs:110-113`). The handler then sends
  `LinkCommand::Stopped { epoch }`. Live sync is stopped too (`handlers.rs:421`).
- **Server.**
  - `handle` takes no identity for stop (`server.rs:1521-1522`).
    `check_execution` returns `Ok` for stop first (`:393`), even on a lost
    bench or a mismatched pin.
  - `latch_stop` (`:430-436`) answers a copy with `stop_latched: true`,
    `enabled_id: null` and `busy: false` (`:1600-1611`).
  - The worker's `observe_stop` (`server.rs:495-563`) torques off every
    configured axis: `bus.rs:344` `stop` (3 attempts), or
    `stop_single_attempt` (`bus.rs:379`) for disabled axes. It then clears
    the selection and owner.
  - Running sweeps, tunes and campaigns see `app.cancel`/`app.stop`
    (`server.rs:1072`, `:867`, `:2289`). A job captured before the STOP is
    refused with "STOP interrupted this command" (`:628`).
- **Session.** `Stopped` triggers `stopped_locally` (`session.rs:726-731`,
  `:911-922`): not ready, no run. `StopAnswered` (`actions.rs:765-769`) is
  adopted (`session.rs:732-738`). Requests queued before the STOP are not
  sent while it is pending (`session.rs:397-404`). A remote command queued
  before it is refused with `STOP_PENDING` (`:634-635`).
- **Readback loss on STOP** (`observe_stop`, `stop_outcome` `server.rs:469-479`):
  - **Lost readback on an enabled axis.** The bus forgets that axis's turns
    (`reset_turn_tracking_for`, `bus.rs:288`). If its poses or alignment are
    bound to the live session, `coordinate_session` is re-stamped
    (`server.rs:517-520`, `:546-548`). The bench is marked
    `connected: false` (`:549-552`). On a virtual bench a transport loss
    also drops the bus with `lose_bus` (`:531-541`, `:309-319`). That nulls
    `execution`, so every later command gets 409 "Virtual bench
    disconnected; restart and reconnect explicitly" (`:394-396`).
  - **Disabled motor.** It gets one attempt. Only its own zero-byte reply
    timeout counts as a note: " Note: disabled {role} (ID n): no readback
    expected (…)." (`:477`, `:525`, `:555`). Any other error is a failure.
    Its lost readback still resets its turns (`reset_turns`, `:478`), so
    if its poses are bound to the live session the shared session is
    re-stamped and every axis's multi-turn poses need re-teaching
    (protective; `:490-493`).
  - **Failure message.** "STOP latched; torque-off readback unverified (…).
    Cut motor power. Records retained." (`:558-560`).
  - **Success message.** "STOP latched; all configured axes torque off.
    Records retained." (`server.rs:25`, `:557`).
- **Panel.** The status line shows the server message (`view.rs:329`). After
  a re-stamp, a motor whose poses were multi-turn reads, once it is chosen
  again and ready (`view.rs:335-338`), "Saved poses are from an earlier
  session and are ignored until re-taught" and "Re-teach both
  poses" (`view.rs:299-303`, `:339-340`, `:488`). Range controls are off
  (`view.rs:434`). A virtual link revokes on `connected: false`, or on the
  vanished execution (`session.rs:434-472`). A physical link shows only the
  message: `hardware_status.connected` turns false (`status.rs:20`). Since
  `313964e3` the status line also starts "DISCONNECTED — …" and
  `link_state` is `disconnected`, without blocking selection (current lines
  in the HW-11 trace, "Disconnected after a physical STOP readback loss").
  JogPress needs the motor chosen again (`handlers.rs:453`).

### HW-05 Focus loss, panel close, leaving Robot mode

- **Focus loss.** `WindowFocused` (`input.rs:77-88`) writes
  `Loss::FocusLost` (Quiet). The REST `loss {focus_lost}` is allowed because
  it is not motion. Both reach `H::Loss` (`handlers.rs:427-430`) and then
  `loss` (`handlers.rs:130-152`). If `drive_active` (`link.rs:430-432`), it
  calls `stop_immediate` (`actions.rs:526`), which is the STOP path of
  HW-04 without the operator flag. Otherwise it sends `LinkCommand::Loss`,
  and the session re-checks `drive_active` and runs `stop()`
  (`session.rs:740-744`, `:924-937`).
- **Close and toggle.** `hardware:close` / × (`panel_sections.rs:93`) sends
  `H::ClosePanel` (`handlers.rs:396-399`). `hardware:toggle_panel` sends
  `H::TogglePanel` (`handlers.rs:388-395`). Both call `close`
  (`handlers.rs:155-158`), which is `loss(PanelClosed)` plus `open = false`.
  `hardware_status.open` follows it (`status.rs:33`).
- **Leaving Robot mode.** `OnExit(Robot)` runs `leave` (`actions.rs:504-517`):
  it answers pending REST calls, calls `stop_immediate`, stops our live sync,
  and drops the state off the UI thread. Without the resource, `apply`
  refuses with "not open in this mode" (`actions.rs:629-631`). Re-entry runs
  `enter` again (`actions.rs:490-498`).
- **Window close.** `WindowCloseRequested` sends `Loss::Leaving` (Quiet,
  `input.rs:83-84`). That stops immediately and also calls
  `post_stop_sync` (`handlers.rs:138-146`, `link.rs:501-518`). AppExit runs
  `stop_on_exit` (`actions.rs:474-484`), and `Link::drop` also stops
  (`link.rs:528-534`). A remote `loss {leaving}` is refused
  (`handlers.rs:424-426`).
- **Server.** The same STOP effect as HW-04.

### HW-06 Teach lower, upper, alignment; targets; reset

- **Capture.** `hardware:capture_lower/upper/reference` (`panel.rs:278-281`)
  is enabled when `pose_enabled`: ready, hold intent, and the latest sample
  holding (`view.rs:431`). It sends `H::Capture` (`handlers.rs:521-533`),
  with the mirror alignment angle for reference. `LinkCommand::Capture`
  (`session.rs:708`) reaches `buttons.rs:68-93`.
  - **In a held session:** `capture_hold` (`calibration.rs:674`) is posted
    through the beat (`send_in_session`, `session.rs:264-286`). On the
    server, `handle` (`server.rs:1543-1572`) requires hold and no STOP,
    stores the `PendingCapture` and waits in `await_capture` for the hold
    session's outcome (current behaviour since `29aa81ac`; before it the
    server answered `{"ok":true}` before anything was saved, see the
    superseded caveat below). The sample callback saves only when the sample
    is holding, under 2 counts/s, within 3 counts of target, and stable over
    6 samples (`server.rs:1096-1097`). It then writes `calibration-<ms>.json`
    and reports `capture_message` "Saved {boundary} pose" (`:1107-1109`).
    Otherwise it reports "Still settling. Release Q/A, wait for Holding,
    then save the pose." (`:1116-1119`).
  - **Without a run:** `calibration::capture` (`calibration.rs:664`) reaches
    the server's capture arm (`server.rs:1251`).
  - **Panel:** the capture line (`view.rs:500`, `panel.rs:423`) and the pose
    captions "x.x° motor" (`view.rs:484`).
  - **Superseded caveat (current lines).** Before `29aa81ac` a remote
    capture's verdict was Ok once `capture_hold` was accepted. Now the server
    answers `capture_hold` only after the hold session saved the pose (200
    with the full status) or refused it (400), and withdraws a capture it
    never took within 1 s (`server.rs:1543-1572`, `await_capture`
    `:173-220`, the save `:1094-1120`). The viewer adopts that status, and
    an answer that is not a status is an error (`CAPTURE_UNCONFIRMED`,
    `buttons.rs:13-26`, `:68-93`). The driver now judges the click's own
    answer (`acceptance.py:873-909`). The HW-11 trace follows the whole path.
- **Target.** The target slider (`input.rs:116`, `:128-130`) or REST `target`
  reaches `handlers.rs:513-519` (`stepped`, 0–100). It sends
  `LinkCommand::Target` to `buttons.rs:30-52`, which applies a 4-count inset
  inside the taught poses (`:46`), and then `begin`/`update` with motion
  "target" (`server.rs:370`). `target_commit` (`handlers.rs:520`) calls
  `update`. The remote gate is `target_enabled` (`handlers.rs:34`,
  `view.rs:434`). `validate_command` requires two poses more than 8 counts
  apart in the current session (`session.rs:890-896`).
- **Reset.** `hardware:reset_poses` (`panel.rs:283`) reaches
  `handlers.rs:534`, `session.rs:710-713` and `buttons.rs:95-114`: STOP, then
  `calibration::clear` (`calibration.rs:683`). The server latches STOP for
  clear (`server.rs:1597-1599`). The clear arm (`server.rs:805-835`) saves
  `…-before-clear.json` and the new calibration, then the session reselects.
- **Re-stamp invalidates poses.** `usable` (`server.rs:249-256`) drops the
  lower/upper of an axis whose `coordinate_session` is not the live one.
  Re-stamps come from STOP readback loss (`:546-548`), idle readback loss
  (`:658-660`), motion readback loss (`:1146-1152`) and `lose_bus` (`:315`).
  Only poses outside one revolution (0–4095) are bound to a session
  (`:1103-1104`). The panel shows "Saved poses are from an earlier session…"
  (`view.rs:299-303`, `:339-340`).

### HW-07 Try saved range, Learn, Sweep all

- **Sweep.** `hardware:sweep` (`panel.rs:282`, gated by `range_enabled`,
  `view.rs:434`) sends `H::Sweep` (`handlers.rs:537`) to `buttons.rs:131-154`.
  That is pause/hold when sweeping. Otherwise it begins a session and sets
  intent Sweep. It resets the speed to 0 (`:148-150`); the form follows in
  `poll_jobs` (`actions.rs:815-826`). On the server, `motion_update` "sweep"
  drives `controlled_motion_multi` inside the poses
  (`server.rs:1071-1131`).
- **Learn.** `hardware:learn` (`panel.rs:284`) sends `H::Learn`
  (`handlers.rs:538`) to `buttons.rs:156-176`. The terminal adaptation is
  kept at `session.rs:590`. The panel shows "{status}. Stops learned: d / i.
  Allowed now: …" (`view.rs:461-464`), and `hardware_status` exposes
  `session.learning_terminal` (`status.rs:50-51`).
- **Sweep all.** `hardware:sweep_all` (`panel.rs:260`) sends `H::SweepAll`
  (`handlers.rs:435`) to `sequences.rs:18`, which posts
  `calibration::sweep_all` (`calibration.rs:614`). The server's
  `sweep_all` arm (`server.rs:995-1162`, `all`) holds each axis after 2 half
  cycles (`:1070`, `:1078-1081`). Panel sequence texts: "Sweep-all: checking
  every enabled motor at zero drive…", "Swept … through their saved
  ranges." and "Sweep-all stopped: …" (`sequences.rs:39`, `:128`, `:86`,
  `:124`).
- **STOP and focus loss.** Same as HW-04 and HW-05. The server ends the
  sweep and records "Torque off and stationary encoder verified."
  (`server.rs:1132-1160`).

### HW-08 Tune

- **Confirm and tune.** `hardware:tune_confirm` (`panel.rs:289`) sets
  `H::TuneConfirm` (`handlers.rs:539-542`), a form flag only. `hardware:tune`
  (`panel.rs:290`) is enabled when ready and confirmed (`view.rs:495`). It
  sends `H::Tune` (`handlers.rs:543-548`) to `sequences.rs:137-172`: STOP,
  `select_motor(solo)`, `tune_stages` cleared, then `calibration::tune`
  (`calibration.rs:618`).
- **Server.** The tune arm (`server.rs:844-910`) refuses when the nearest
  saved pose is less than 100 counts away ("Only n counts … needs 100",
  `:853-857`). It arms and replies running (`:860-866`).
  `identify` follows `app.cancel` (`:867`). On success it writes
  `tune-<id>-<ms>.json` (`:877-883`), saves the gains, and reports "Tuned …
  Select it to use them." (`:891-901`). STOP cancels it; `tuning.error`
  becomes "Tuning stopped: {e}. Torque off; previous gains kept." (`:903-907`).
- **Session.** `tune_tick` (`sequences.rs:174-186`) ends it and bumps
  `tune_done`, and `poll_jobs` unticks the confirmation (`actions.rs:809-811`).
  STOP also clears it (`actions.rs:538`). The stages are collected in
  `adopt` (`session.rs:490-494`).
- **Panel.** "Tuning: {stage}", "Last tuning stopped: …" and "Tuned gains in
  use: kp …, ki …, kd …, friction …% · {record}" (`view.rs:436-448`). On a
  virtual link it is prefixed "VIRTUAL ·" with "Captured stages: …"
  (`panel.rs:224-225`).

### HW-09 Campaign and resume

- **Confirm and run.** `hardware:campaign_confirm` (`panel.rs:291`) sets
  `H::CampaignConfirm` (`handlers.rs:549-552`). `hardware:campaign` and
  `hardware:campaign_resume` (`panel.rs:293-294`) send
  `H::Campaign { resume }` (`handlers.rs:553-558`) to
  `sequences.rs:188-221`: STOP, `select_motor(hold_all)`, then
  `calibration::campaign` (`calibration.rs:622`).
- **Server.** The campaign arm (`server.rs:971-994`) calls `run_campaign`
  (`:2218`) and `campaign_directory` (`:2185-2216`). Resume reuses the
  receipts of the newest interrupted campaign and copies the old reports to
  `attempt-before-resume-<ms>`. Receipts are written as stages finish
  (`:2309`), and `completed` is counted (`:2323-2324`). STOP gives "Campaign
  stopped: {e}. Torque off; completed stages are kept as receipts (Resume
  continues)."
  (`:988-990`); finishing gives "Campaign finished: {…}. Results in {…}. Nothing
  was promoted to CAD." (`:983-985`).
- **Session.** `campaign_tick` (`sequences.rs:223-235`) ends it, and
  `poll_jobs` unticks the confirmation (`actions.rs:812-814`).
- **Panel.** "{stage} · n stage results saved · last stopped by {gate}",
  "Last campaign stopped: …" and "Finished: {headline}. {directory}"
  (`view.rs:449-460`), prefixed "VIRTUAL ·" on a virtual link
  (`panel.rs:226`).

### Export (HW-15; also keeps HW-06–HW-09 records)

- **Request.** `hardware:export` (`panel.rs:318-325`) or REST
  `hardware_export` sends `H::Export` (`handlers.rs:431`) to `export`
  (`handlers.rs:696-724`) and `start_export` (`:739-763`). The job runs
  `GET /calibration/export`, which the server answers with
  `export_document` (`server.rs:1494-1496`, `:324-357`). A virtual server
  adds `execution` and `"simulated": true`.
- **File.** `write_export` (`handlers.rs:780-811`) writes
  `viewer-exports/leg-calibration-<ms>[-virtual].json` with
  `create_new`, so a file is never overwritten.
- **Panel.** `poll_jobs` (`actions.rs:770-780`) prefixes the line with
  "VIRTUAL (simulated)" (`handlers.rs:727`), shown at `panel.rs:441`. REST
  answers `{path, simulated}` (`handlers.rs:699`).

**Hops not traced to a line.**
- The bench side of `hx_virtual_bench`: what the socket bench does on each
  frame.
- The inside of `controlled_motion_multi`, `identify` and `BusRig`
  (`bus.rs:643`, `:785`; `server.rs:2289`).
- How the UI kit turns a pointer press into `Activated`.

These traces stop at the call.

## Reading traces — HW-10/HW-11 (by reading, unexecuted)

**Status.** No executed evidence exists for HW-10 or HW-11 in the native
viewer. Every trace below was made by reading the source at `80e4de27` (the
server at `29aa81ac`, the viewer at `313964e3`, the driver at `80e4de27`;
2026-10-02), and every line number was then checked again by reading it at
`ecebd48c`, after `afe4a486` (`mirror.rs` `Mirror::update`: later `mirror.rs`
lines moved by 6) and `ecebd48c` (`session.rs:631`, no shift); all were
re-checked against the working tree on 2026-10-03 (citation note above). Nothing here was built, run or tested, and the new tests in those commits
have never run. The browser calibration page stays the reference until a pass
runs.

The short names are those of the HW-01–HW-09 traces, plus:

| Short name | File |
|---|---|
| `mirror.rs`, `apply.rs`, `thread.rs` | `crates/sim-spatial/src/robot/hardware/mirror.rs`, `…/mirror/apply.rs`, `…/mirror/thread.rs` |
| `mirror_panel.rs` | `crates/sim-spatial/src/robot/hardware/mirror_panel.rs` |
| `acceptance.py` | `tools/native-calibration/acceptance.py` |

### HW-10 Sim only (no server motion)

- **Play.** The `hardware:gait_play` control (`panel.rs:305`, label Play,
  Pause or Resume from `view.rs:685-691`) sends `H::GaitPlay`
  (`handlers.rs:601`) to `gait_play` (`handlers.rs:673-690`). With no gait
  playing it needs a gait (`:678-680`); Sim needs no confirmation (`:682`);
  no tune or campaign may run (`:685`). Sim sends no bindings (`:688`) and
  queues `LinkCommand::GaitPlay` (`:689`).
- **Link thread.** `session.rs:722` calls `gait_play` (`sequences.rs:252-268`),
  then `start_gait` (`:270-322`). It fetches the compiled gait
  (`GET /calibration/gait?path=…`, `:271`; server `server.rs:1488-1493`), reads it
  with the shared sampler, and publishes it for the mirror as
  `compiled_gait` (`sequences.rs:275`). With `leg` false (`:277`) the server is not
  asked to move anything. The run starts at `t` 0, playing, at the form's
  scale; `gait_last` and `next_frame` are set (`:258-261`).
- **Clock.** Sim time is wall time × scale on the link thread:
  `periodic.rs:98-102` runs `sim_frame` (`sequences.rs:389-395`) every
  `GAIT_FRAME` (20 ms, `session.rs:68`) and publishes. Nothing else writes
  a Sim run's `t`.
- **Mirror.** `mirror_sync` (`mirror_panel.rs:54`) passes the run to
  `Mirror::follow_gait` (`mirror_panel.rs:100-103`, `mirror.rs:632-683`). A
  new play loads the gait on the worker (`mirror.rs:647-656`, `thread.rs:45-53`);
  once it is ready (`mirror.rs:710`), each frame with no sample in flight
  sends `Sample` at `(run.t, run.scale)` (`mirror.rs:673`, `:682`). The worker
  steps `GovernedGait::step` when the gait has a governor, else samples it
  (`thread.rs:54-66`, `:62`). The pose comes back in `poll` (`mirror.rs:719`)
  and `set_gait` re-poses every leg; the line reads "Simulated gait"
  (`mirror.rs:541-547`).
- **Pause/Resume.** Play again sends `GaitToggle` (`handlers.rs:675-677`).
  `gait_toggle` (`sequences.rs:353-359`) counts the time played so far
  (`sim_frame`), then flips `playing`.
- **Speed.** REST `gait_speed` or the slider reaches `handlers.rs:573-583`
  (5–100 %), updates the inputs and sends `GaitScale`; `gait_scale`
  (`sequences.rs:370-378`) counts the time so far at the old speed, then sets
  the new scale.
- **Stop.** `hardware:gait_stop` (`panel.rs:306`) sends `H::GaitStop`
  (`handlers.rs:602-613`); for Sim it only queues `LinkCommand::GaitStop`.
  `gait_stop` (`sequences.rs:361-368`) runs `end_gait` (`:381-384`); with no
  run, `follow_gait` clears the gait pose (`mirror.rs:633-641`).
- **Panel.** "Sim only · gait time t s of P s period · n% speed"
  (`view.rs:595-613`). `hardware_status.session.gait` carries mode, `t`,
  `playing`, `scale` and `leg` (`status.rs:26-30`).

### HW-10 Leg only on the virtual bench

- **Control.** `hardware:gait_play` (`panel.rs:305`) is ready when there are
  gaits, the suspended-leg confirmation is ticked for Leg/Both, and no tune or
  campaign runs (`view.rs:685-691`); it is no longer disabled on a virtual
  link (`panel.rs:298-305`, compare `in_scope` `:251`). The mode radios are
  `hardware:gait_mode_*` (`:300-302`), the confirmation `hardware:gait_confirm`
  (`:303`).
- **Apply and authorize.** `actions.rs:619` `apply` calls `handle`
  (`dispatch`, `handlers.rs:165`, re-exported at `:382`). For a remote call,
  `dispatch` runs `HardwareAction::authorize` (`actions.rs:312-316`), which
  for a motion start (`starts_motion` lists the gait intents and Play,
  `actions.rs:275-280`) calls `authorize_with` (`:331-356`). The gait intents
  and `GaitPlay` are on its list (`:346-347`). It then calls
  `authorize_virtual` (`calibration.rs:43-54`) with
  `connection_valid && state.connected && !authorization_revoked &&
  disconnected.is_none() && link_generation == generation` and freshness
  (`actions.rs:351-355`). Then `remote_check` (`handlers.rs:28-59`) refuses
  the control's own disabled reason. A ticket is assigned (`:204-209`).
- **Handler.** `gait_play` (`handlers.rs:673-690`) needs the confirmation
  (`:682`) and takes the bindings and skipped motors from
  `Mirror::gait_bindings` on this frame's status (`:688`, `mirror.rs:746-771`:
  disabled, poses not taught, not aligned, no CAD joint are skipped; the
  home angle is the saved alignment angle). `send` queues
  `LinkCommand::Checked` with the epoch and generation (`handlers.rs:75-90`,
  `:81`). REST gets a 45 s continuation (`:214-221`).
- **Link thread.** `session.rs:623-674` re-authorizes against its own
  snapshot (`:630-631`; it also refuses a disconnected link since `ecebd48c`), refuses after a newer STOP (`:634`), validates
  (`validate_command`, `:886-907`; a new Play waits for a tune or campaign,
  `:903`) and runs it (`run_checked`, `:795-883`). The play is judged
  started when `snap.gait` has that mode and nothing failed (`:863-872`). The
  verdict goes to `command_results` (`:668`) and REST is answered from it
  with `hardware_status` (`handlers.rs:175-181`).
- **Start.** `start_gait` (`sequences.rs:270-322`) with `leg` true: no
  binding refuses with "No motor is aligned, taught and enabled: …"
  (`:281-283`). It STOPs, selects the first bound motor holding all the
  others (`:284-285`, which proves their watchdogs on the server), needs it
  ready (`:286-295`), and posts `calibration::gait_start` (`:298-302`; body
  `calibration.rs:636-655`: `supported`, gait path, bindings, speed scale,
  effort, PWM ceiling, drive mode). A STOP pressed meanwhile drops the answer
  and stops (`:303-308`). The answered status is adopted (`:313`).
- **Refused start releases the motor (2026-10-03, by reading, unexecuted).**
  A `gait_start` the server refuses (an error answer, `:299-303`; or a
  status whose gait is not running and carries its error, a refusal inside
  `run_gait`, `:311-318`), and a select that left the session not ready
  (`:286-294`; sent whether or not the select's answer was adopted, since a
  lost answer can still leave motors held), go through
  `release_after_refused_start` (`:337-357`). A UI STOP pending since the
  select returns its own reason with no prefix and no extra request
  (`:302`, and `:338-340`: that STOP releases every motor itself); otherwise
  it sends the
  existing STOP request (`Session::stop`, `session.rs:924-937`;
  `calibration.rs:592-594`), whose server answer latches STOP and torques
  off every configured axis (`server.rs:1600-1611`). It returns the play's
  error with the release: "gait_start refused: {server's reason}; motor N
  released (STOP sent: STOP latched; all configured axes torque off. Records
  retained.)" ("…; the motors released (…)" after a select that was not
  ready, since STOP latches every axis), or "…; releasing motor N failed:
  {why}. Press STOP." when the STOP request itself failed (`:346-354`). `gait_play` puts it in the gait
  notice (`sequences.rs:263-266`): the panel's gait line shows it
  (`view.rs:651-653`), REST shows it as `session.gait_notice`
  (`status.rs:54`), and a remote Play is answered with it (`session.rs:863-872`).
  The browser page leaves the motor held here; the difference is deliberate
  (AGENTS.md hardware safety; ledger CAL-157). Test
  `a_checked_leg_gait_play_answers_ok_only_after_the_gait_started`
  (`session/tests.rs`): written, never executed.
- **Server handle.** The pinned client adds the identity headers
  (`http.rs:112-120`). `handle` (`server.rs:1381`) checks identity and
  generation (`:1521-1540`; a newer generation latches STOP, `:1535-1538`),
  then `check_execution` (`:392-414`): `virtual_command_allowed` now admits
  `gait_start` and `gait_update` (`calibration.rs:79-84`, used at
  `server.rs:406`). The command is queued to the worker (`:1622-1634`).
- **Worker.** The job is re-checked: binding, scope, expiry, STOP epoch
  (`server.rs:626-628`). The common motion checks need this client's
  selected, verified motor (`:836-838`) and a newer sequence (`:839-842`).
  The `gait_start` arm (`:911-946`) calls `run_gait` (`:1831-2104`) on the
  bus that `select` opened (`App::open_bus` `:437-443`, `CalibrationBus::open_virtual`
  `calibration_serial.rs:93`).
- **`run_gait` preconditions.** Confirmation (`server.rs:1845-1847`); every binding's
  motor enabled, taught (`:1863`), watchdogs proven (`:1866`), aligned
  (`:1868`), a multi-turn alignment from this encoder session (`:1869-1871`);
  the saved alignment angle wins over the client's (`:1874`). The gait must
  fit 95 % inside each taught window less 6 counts, else the misfit refusal
  names the likely reversed sign (`:1885-1914`). Limits come from the
  accepted registry at the measured supply; a 0 V or non-finite reading
  counts as no supply and 12 V is assumed (`:1919-1923`).
- **Loop.** It arms (`server.rs:1954`), publishes the labelled gait state
  (`labelled` `:341-345`; phase `approach`, `:1956-1962`) and replies, then
  takes the lease (`:1965`) and logs `gait_start` (`:1967`). The governed loop
  (`controlled_motion_multi`, `:1978`) ends on STOP or cancel, and on a lease
  older than 1.5 s ("Browser heartbeat lost; gait stopped", `:1980-1988`). It
  writes `gait.t`, phase (`approach`, `playing` or `paused`), scale, targets
  and clamped count each period (`:2048-2053`), and per-motor tracking errors
  (`:2069`).
- **Lease.** With a leg gait published, `beat_plan` (`session.rs:229`) gives
  the beat a gait plan; the beat posts `gait_update` every 300 ms and at once
  on a scale or pause change (`beat.rs:183-188`, `:215-219`, `:252-258`,
  body `calibration.rs:659-661`). The server renews the lease
  (`server.rs:1574-1587`). A revoked pin empties the plan
  (`session.rs:222-224`).
- **Status.** The link polls every 150 ms while a leg gait runs
  (`periodic.rs:50-51`) and adopts each status (`periodic.rs:20`,
  `session.rs:434-500`); `adopt` ends with `leg_frame` (`:497-499`,
  `sequences.rs:406-414`), which copies `gait.t` into `GaitRun::t` and marks
  the run started once the server reports it running.
- **Panel.** `render_gait` (`view.rs:589-700`) writes "Leg only · gait time …",
  "Limits: …" (`:622-625`), then "VIRTUAL (simulated) · Leg: {phase}" when the
  server's gait says `simulated` or the link is pinned to a virtual execution
  (`:626-630`), " · error {role} n, … counts" (`:631-637`), clamped targets
  (`:638-640`) and "Not driven: …" (`:641-643`). Effort and the radios are
  locked while it plays (`:684`, `:694`).
- **Stop.** The gait's Stop (`handlers.rs:602-613`) first STOPs on the
  immediate path for a leg gait (`:609-611`, `link::stop_now` `link.rs:565`),
  then queues `GaitStop`; `gait_stop` (`sequences.rs:361-368`) ends the run and
  sends its own STOP. On the server, STOP latches (`server.rs:1600-1611`); the
  loop sees `app.stop` and ends (`:1979-1981`). `run_gait` writes the labelled
  record `gait-runs/run-<ms>.json` (`:2083-2093`; `virtual_limits` on a
  virtual bench, `:2090-2092`), logs `gait_end` (`:2094`) and refreshes
  `gait_runs` (`:2098`). The arm then clears the lease, labels the final gait
  state and reports "… Torque off and stationary encoder verified." or "Gait
  stopped: {e}. Torque off." (`:916-943`). A readback loss resets the bound
  axes' turns and re-stamps a multi-turn session (`:931-939`).
- **Recent leg runs.** `gait_run_history` (`server.rs:1806-1817`) lists the
  newest 12 records with `simulated` (false when absent) and `execution`. The
  panel heads a simulated row "VIRTUAL (simulated) · {gait} · effort …"
  (`view.rs:669-676`). The link thread ends its run when the server reports
  the started gait no longer running (`session.rs:585-587`).

### HW-10 refusals (remote `gait_play` and the gait intents)

| Link | Where it is refused | Text or status |
|---|---|---|
| Physical (unpinned) | `authorize_virtual` identity check (`calibration.rs:47-49`), via `authorize_with` (`actions.rs:351-355`) before any ticket | "Remote calibration requires a verified virtual calibration server". The operator's own clicks are not remote and are not authorized this way; the unpinned physical server accepts them (`server.rs:411`). |
| Unknown (no identity) | Same as physical in the viewer. A direct request without identity to a virtual server: `check_execution` (`server.rs:410`) | 409 "… Virtual execution identity required" (`server.rs:1642-1647`) |
| Stale status | `authorize_with` freshness (`actions.rs:354`); a pending ticket whose status went stale STOPs and fails (`handlers.rs:191-194`); the link thread's own check (`session.rs:630-631`) | "Virtual calibration authorization expired; reconnect explicitly" (`calibration.rs:50-52`) |
| Replaced generation | `link_generation == generation` (`actions.rs:353`) and `authorize_virtual` (`calibration.rs:50`); a pending ticket from another generation (`handlers.rs:168-170`); the link thread (`session.rs:630`); the server (`server.rs:1530-1534`, `:399-403`) | "authorization expired", "hardware connection replaced while the command was pending", or 409 "stale connection generation" / "identity/generation mismatch" |
| Revoked | `!authorization_revoked` (`actions.rs:353`); the link thread refuses sends while revoked (`session.rs:401-404`, `REVOKED` `:81`) | "authorization expired"; "virtual calibration authorization revoked; reconnect required" |
| Disconnected | `disconnected.is_none()` (`actions.rs:353`); the link thread (`session.rs:631`, since `ecebd48c`); a lost virtual bench on the server (`server.rs:394-396`) | "authorization expired"; 409 "Virtual bench disconnected; restart and reconnect explicitly" |
| Out of scope on a valid pin (not the gait, which is in scope; e.g. `jog`, `flip`) | `check_execution` (`server.rs:406-408`) | 400 `out_of_scope` (`:360-362`); the binding is kept |

**Never gated.** `Stop` and `GaitStop` are not motion starts
(`actions.rs:300`, `:304`), so `authorize` returns Ok (`:313`) and
`remote_check` skips them (`handlers.rs:29`). `GaitStop` without a link is a
no-op answer (`handlers.rs:604-606`). The server takes STOP without identity
(`server.rs:1521-1522`) and before any check (`:393`).

### HW-10 Both: one leg clock

- **Base.** The server writes `gait.t` (`server.rs:2048`). `adopt` stamps
  `read_at` (`session.rs:496`) and runs `leg_frame` (`:499`), the only writer
  of a leg run's `GaitRun::t` (`sequences.rs:406-414`, field `link.rs:208`).
- **Now.** `LinkSnapshot::leg_clock` (`link.rs:384-394`) takes the server's
  `speed_scale` (else the run's), the health (`:363-372`), and advances only
  when live, started, playing here and the server's phase is `playing`
  (`:390-391`). `leg_gait_time` (`link.rs:341-346`) adds at most one active
  poll (`POLL_ACTIVE`, 150 ms, `link.rs:55`) × scale since `read_at`; each
  new read resets the base.
- **Mirror.** `mirror_sync` computes the clock every frame
  (`mirror_panel.rs:102`) and passes it to `follow_gait` (`:103`). Leg only
  is not sampled (`mirror.rs:643`). For Both, `mirror.rs:660-671` uses the
  clock unless it is frozen; a new time that is behind the last sample by no
  more than one active poll × scale holds the last time instead of stepping
  back (`:666-668`). The worker steps the governed gait at that time
  (`thread.rs:62`). With `sample_both` (`mirror.rs:679`) the pose is applied
  with the real leg on the encoders (`:719`, `set_gait` `:606-611`; `update`
  overwrites the bound joints with the encoders, `:554-578`).
- **Panel and status.** The gait line shows `run.t` (the base, steady
  between reads) and the clock's scale (`view.rs:600-613`);
  `hardware_status.session.gait.t` is the interpolated clock, with
  `clock_frozen` (`status.rs:25-29`).
- **Stale freeze.** `mirror_sync` judges health every frame before anything
  can pose (`mirror_panel.rs:57-62`, `set_leg_health` `mirror.rs:376-382`).
  While not live: `update` poses nothing from the encoders (`mirror.rs:549-551`),
  `follow_gait` sends no sample and holds the gait pose (`:661-665`), and the
  clock does not advance (`link.rs:391`). The texts come from
  `LinkHealth::leg_note` (`link.rs:324-331`): "Leg data stale — last read N
  s ago; not live" (whole seconds) or "Leg disconnected — {why}; not live".
  The gait line adds " (clock frozen)" and the note (`view.rs:610`,
  `:614-616`); the mirror line is "{note} · last reading: {line}"
  (`mirror.rs:359-369`); `hardware_status` has `link_state` and `link_note`
  (`status.rs:22-24`, `:40-41`).
- **Recovery.** When the health becomes live again `set_leg_health` returns
  true (`mirror.rs:377`) and `mirror_sync` forces `update` with the current
  status (`mirror_panel.rs:94-99`). The first sample after a hold uses the
  shortest step (`mirror.rs:663`).

### Simulated labels (HW-10)

- **Server.** `labelled` (`server.rs:341-345`) adds `execution` and
  `simulated` (true only for a virtual execution) to the live gait state
  (`:1958`, and again after the run, `:922-924`), the `gait_start` and
  `gait_end` log lines (`:1967`, `:2094`, in `gait.jsonl`, `writeln_log`
  `:2105-2111`) and the run record (`:2083`). A virtual record adds
  `virtual_limits`, what the bench does not emulate and the supply assumed
  (`virtual_gait_limits` `:348-357`, `:2090-2092`); every record carries
  `supply_v` and `supply_assumed` (`:2086`). History rows carry `simulated`
  and `execution` (`:1813-1814`).
- **Client types.** `GaitState::simulated` and `GaitRun::simulated` default to
  false for an older server or record (`calibration.rs:411-414`,
  `:509-516`).
- **Panel and status.** The Leg line (`view.rs:626-630`), the run headings
  (`view.rs:669-676`, `VIRTUAL_SIMULATED` `:202`) and
  `hardware_status.server.gait.simulated` (`status.rs:105`).

### HW-11 Mirror alignment

- **Mirror on.** `hardware:mirror_on` (`panel.rs:327`) or the section's chip
  (`mirror_panel.rs:164`) sends `MirrorEnabled { on: true }`, which is not a
  motion start (`actions.rs:305`) and so is allowed remotely. `handlers.rs:662`
  calls `mirror::apply` (`apply.rs:11-14`): enabled, `begin`. The preference
  path is claimed (`actions.rs:707`). In `mirror_sync`, roles come from the
  first status that lists axes (`mirror_panel.rs:71-77`); `want` needs
  enabled, the panel open, a link and roles (`:78`). `prepare`
  (`mirror.rs:431-494`) loads the scene on the worker (`thread.rs:25-33`),
  checks the bindings, and returns the links whose names start with
  "{leg} |" to tint (`mirror.rs:488-490`). The first show pauses a running
  run, sets `RobotView::mirror`, and fits the view (`mirror_panel.rs:111-119`).
- **Save sim alignment.** `hardware:capture_reference` (`panel.rs:278-281`)
  sends `H::Capture { Reference }`; the handler adds the mirror's alignment
  angle, mid-travel or CAD home (`handlers.rs:521-533`,
  `Mirror::alignment_angle` `mirror.rs:342-349`). `LinkCommand::Capture`
  (`session.rs:708`) reaches `buttons.rs:68-93`. In a held session it posts
  `capture_hold` through the beat (`send_in_session` `session.rs:264-286`,
  `beat.rs:198-206`; body `calibration.rs:674-682`). The server's `handle`
  (`server.rs:1543-1572`) requires a held teaching session and no pending
  capture, renews the session's lease and sequence (`ctl.update`), stores the
  `PendingCapture` (`:1568`) and waits in `await_capture` (`:173-220`). The
  hold session takes it at the next sample and renews the lease
  (`:1094`). It saves only when holding, under 2 counts/s, within 3 counts of
  target and stable over 6 samples (`:1096-1097`): it writes
  `calibration-<ms>.json`, sets `reference`, `reference_session` (multi-turn
  only) and `reference_joint_rad`, sets "Saved reference pose" and the saved
  reading in `samples` (`:1099-1110`), renews the lease again and answers
  (`:1112-1114`). Otherwise it answers "Still settling…" (`:1116-1119`).
  `await_capture` withdraws a capture not taken within 1 s (`:174-185`) and
  returns the full state (`:192`), sent as 200 (`:1571-1572`, `:1638`); a
  refusal is a 400 (`:1642-1647`).
- **Viewer.** `saved_status` (`buttons.rs:20-26`) accepts only a status with
  `connected` and `calibration`, else `CAPTURE_UNCONFIRMED` (`:13`). The
  status is adopted unless a STOP came meanwhile (`:77-80`); an error sets the
  capture line and the verdict (`:88-91`). `run_checked` returns the recorded
  error or Ok (`session.rs:873`); the verdict goes to `command_results`
  (`:668`) and REST is answered with `hardware_status` (`handlers.rs:175-181`).
- **Mirror after the save.** `publish` bumps the revision (`session.rs:211-217`);
  `mirror_sync` sees the new revision and calls `update` (`mirror_panel.rs:92-99`).
  The saved reading equals `reference`, so `delta` is 0
  (`mirror.rs:575`) and the line reads "{role}: 0.0° from its alignment
  pose" (`:577`; `degrees_text` `:784-786`; `fixed` prints a negative zero as
  "0.0", `view.rs:228-231`). The line is replaced even when no joint value
  changed (since `afe4a486`, `mirror.rs:579-590`: `update` returns early only
  when both the values and the line are unchanged), so a first alignment
  saved exactly at the pose the unaligned motor was shown at also turns
  "not aligned — shown at {pose}" into "0.0° from its alignment pose". The
  test for that (`a_first_alignment_at_the_shown_pose_changes_the_line`,
  `mirror/tests.rs:60`) is written and has never run.
- **Sign flip.** `MirrorPolarity` is a motion start (`actions.rs:288`) that is
  not on the remote list, so it is refused remotely on every link. Locally
  `apply.rs:29-35` sets the sign and begins again; `prepare` re-tints and
  forces `update` (`mirror.rs:491-492`), where `delta` takes the new sign
  (`:575`). The same sign becomes `gait_start`'s polarity
  (`mirror.rs:768`).
- **Beyond CAD limit.** The worker returns the solve's authored limit
  violations (`thread.rs:35-43`). `poll` keeps only the mirrored leg's joints
  (prefix "{leg} |", as the tint) and appends " · Beyond CAD limit: …"
  (`mirror.rs:728-731`).
- **Run, Step and Reset refused while mirroring (Step and Reset since
  2026-10-03, by reading, unexecuted).** Robot mode's `check` and
  `check_planar` call `refuse_run` (`mirror.rs:65-71`) while
  `RobotView::mirror` is set (`robot_actions.rs:196-199`, planar
  `:152-155`): Start, Step and Reset are refused as "Run refused: …",
  "Step refused: …" and "Reset refused: …" with `MIRRORING`
  (`mirror.rs:58`); Pause is allowed. Every origin goes through `check`: a
  click and a `system_ui` activation reach `dispatch` (`robot_actions.rs:318`,
  planar `dispatch_planar` `:254`; `Activate` `:578-584`), REST `robot_run`
  is parsed to the same `RobotAction::Run` (`robot/actions/commands.rs:65`),
  and the `system_ui` listing reports `enabled` false with the refusal as
  `disabled_reason` (`robot_actions.rs:575`). A clicked refusal shows as the run message
  (`apply`, `Origin::Ui` → `view.run_message`, `robot_actions.rs:745-747`). The browser page
  guards only Play (viewer.js:103) and lets Step and Reset act behind the
  mirror (ledger MIR-38). The Run/Step/Reset buttons are dimmed through
  the same `check` (`robot/scene.rs:434-440` `highlight`, call at `:438`;
  focus-safety-closure, by reading, unexecuted), so they look disabled
  while mirroring.
- **Unticking.** `MirrorEnabled { on: false }` sets enabled false and calls
  `end` (`apply.rs:11-14`, `mirror.rs:413-418`). `mirror_sync` also ends it
  when `want` turns false (`mirror_panel.rs:86-91`), then clears
  `RobotView::mirror` once (`take_ended`, `mirror.rs:420-422`;
  `mirror_panel.rs:124-127`), so the run's untinted frame shows again.
- **Multi-turn alignment.** `update` shows a multi-turn alignment saved in an
  earlier encoder session at its alignment pose with "{role}: alignment is
  from an earlier session — re-align (shown at {pose})" (`mirror.rs:564-569`).
  `run_gait` refuses the same case (`server.rs:1869-1871`).
- **Disconnected after a physical STOP readback loss.** `observe_stop` marks
  `connected: false` when a STOP lost readback (`server.rs:549-552`). `adopt`
  on a physical link keeps `connection_valid` (`session.rs:467`) and, because
  the previous status was connected, sets `disconnected` to "the server
  reports its bus disconnected: {message}" (`:481-486`). `health` returns
  `Disconnected` first (`link.rs:363-366`). The status line starts
  "DISCONNECTED — …" (`mark_disconnected` `view.rs:526-538`, again after a
  block `panel.rs:216-221`) without blocking selection (only a revoked or
  invalid connection blocks, `panel.rs:213-215`); `hardware_status.link_state`
  is `disconnected` (`status.rs:22-23`); the mirror holds its pose and says
  "Leg disconnected — …; not live". Remote motion is refused
  (`actions.rs:353`). Selecting a motor reconnects the bus (`reconnect_stopped`, `server.rs:745`; `connected: true` at `:767`),
  and the next connected status clears the mark (`session.rs:477-480`).
- **Reconnect after a lost virtual bench.** `lose_bus` nulls `execution`
  (`server.rs:309-319`); the next adopted status revokes, marks
  `BENCH_LOST`, STOPs locally and, only if this link drove, posts one id-less
  STOP (`session.rs:437-465`); later polls only update the reason
  (`:448-454`). Connect (`actions.rs:569-608`) records that the old link was
  pinned (`:576`), stops and drops it (`:577-583`), takes a new generation
  (`:584`) and runs the connect job: a virtual server is pinned with the new
  generation and verified by `inspect` (`:592-604`); otherwise the link is
  unpinned (`:605`). `poll_jobs` spawns the link (`:733`), shows `BENCH_GONE`
  when a pinned link was replaced by an unpinned one (`:739-741`, text
  `:612`) and unticks the suspended-leg confirmation on every new link
  (`:746`).

**Gaps found while tracing (both fixed since; the fixes are unexecuted).**

- **Fixed in `afe4a486`: the alignment line could keep its old text.**
  Before it, `update` returned before it set `text` whenever the new joint
  values equalled the pending ones and `force` was false. A first alignment
  saved exactly at the shown alignment pose gives the same value for that
  joint (the unaligned motor was shown at `alignment_angle`, and the saved
  angle is that same value plus a zero delta), so the line kept
  "{role}: not aligned — shown at {pose}" until some joint value changed.
  Now `update` builds the line first and returns early only when the values
  and the line are both unchanged (`mirror.rs:584-587`). The test
  `a_first_alignment_at_the_shown_pose_changes_the_line`
  (`mirror/tests.rs:60`) is written and has never run. HW-11 in the driver
  re-aligns a motor that HW-10b already aligned, so a passing run does not
  exercise this case.
- **Fixed in `ecebd48c`: the link thread's re-authorization did not check
  `disconnected`.** It now passes `self.snap.disconnected.is_none()` to
  `authorize_virtual` (`session.rs:631`), as `authorize_with` does
  (`actions.rs:353`). By reading, every path that sets `disconnected` on a
  pinned link also revokes it or invalidates the connection
  (`session.rs:359-366`, `:437-472`), so this closed a gap for future paths;
  no command was let through before it.

**Hops not traced to a line.** The inside of `controlled_motion_multi`
(`calibration_serial.rs:643`) and of `GovernedGait`; what `hx_virtual_bench`
does with each frame; how the robot view draws the tinted links from
`RobotView::mirror`; the kinematic solve inside `KinematicMirror::pose`.

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
  - Physical link: the status line starts "DISCONNECTED — the server
    reports its bus disconnected: …" (since `313964e3`), the mirror says "Leg
    disconnected — …; not live", and `hardware_status.connected` is false.
    Choose the motor again to reconnect the bench; the marker clears with the
    next connected status.
- **A refused command keeps the session.** On a virtual bench, a command
  outside its scope (Swap direction, raw step; Leg/Both gait is in scope
  since `313964e3`) is disabled in
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

## Physical operator run sheets — HW-10/HW-11

These sheets are **unexecuted physical checks**, in the same terms as the
HW-01–HW-09 sheets above: one client at a time, the fixture/FPGA/power
prerequisites of [Before you start](#before-you-start), no image loaded, no limit
or PWM ceiling raised, nothing promoted to CAD or the registry. The texts below
were read from the source (see the
[HW-10/HW-11 traces](#reading-traces--hw-10hw-11-by-reading-unexecuted)) and
have not been observed. A virtual-bench run of HW-10b or HW-11 by the driver
cannot stand in for these sheets.

**Preconditions, in this order:**

1. **Gait-lab requalification first (Leg only and Both only).** Requalify the
   reduced model as the gait-lab README says
   (examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25/README.md,
   "After any library source change"), and record the run id in the sign-off.
   The viewer and server do not check this: it is a procedural gate. Do not
   play Leg only or Both on the leg until it is done. Sim only and HW-11 do not
   need it.
2. **The leg is suspended with clear space around every joint**, the fixture
   supported so that torque-off cannot drop it.
3. **The operator stays at the window and the fixture** for the whole run,
   with **STOP within reach** (Z, Escape, the panel's top Stop) and the motor
   power switch within reach.
4. Every motor to be driven is enabled, taught (lower and upper) in this
   encoder session, aligned with Save sim alignment, and its watchdogs are
   proven by selecting a motor with "Hold the other enabled motors in place
   while one moves" checked. A motor that is not is listed under "Not driven: …" and stays
   still.

**How to stop, in every step:**

1. Press **Z**, **Escape** or the panel's top **Stop**, or the gait's own
   **Stop** (it STOPs drive on the immediate path first for a leg gait).
2. Confirm torque-off at the fixture and in the server status (no enabled
   motor; the gait line no longer shows "Leg: …").
3. If STOP does not answer or a motor keeps moving, switch **motor power off**.
   The FPGA supervisor and watchdog stay independent of the host; the server
   also ends a leg gait 1.5 s after the viewer's lease stops (HW-14 item 3).

| Step | What to do | What to expect | How to stop / when to stop |
|---|---|---|---|
| HW-10 Sim only | Open Gait playback, pick a gait, keep **Sim only**. Play, Pause, Resume, move Playback speed, Stop. | Nothing on the leg moves; no motor is energized. "Sim only · gait time t s of P s period · n% speed"; the time holds on Pause and continues on Resume. | The gait's Stop. Z/Escape/Stop if anything on the leg moves. |
| HW-10 Leg only | After requalifying: select a motor with hold others checked, tick "The leg is suspended with clear space around every joint (needed for Leg and Both)", choose **Leg only**, 50 % effort or less, a low playback speed (25 % or less). Play. Watch one or two periods, then the gait's Stop. | Server message "Moving the leg to the gait's first pose through the gait's governor. Z stops drive." The Leg line goes "Leg: approach", then "Leg: playing · error {role} n, … counts", with "Limits: …" and, if any, "Not driven: …". There is **no** "VIRTUAL (simulated)" label on a physical leg. The radios and Leg effort are locked. Stop ends the run and latches STOP, so the status ends on the STOP message ("STOP latched; all configured axes torque off. Records retained.", or "… unverified … Cut motor power" — then cut power); the run's own "Gait stopped after x s of gait time; statistics saved. Torque off and stationary encoder verified." may show briefly first. A new Recent leg runs row appears without the VIRTUAL label. A refusal names why: a misfit ("only n% of the gait fits its taught poses …"), "save its sim alignment first", "watchdogs not proven …", "No motor is aligned, taught and enabled: …". **A refused start releases the motor** (since 2026-10-03, by reading, unexecuted; the browser page leaves it held): when the server refuses `gait_start` after Play selected a motor, the viewer sends STOP at once, and the gait line reads "gait_start refused: {reason}; motor N released (STOP sent: STOP latched; all configured axes torque off. Records retained.)". Expect the selected motor and the held others to go limp (torque off), the status to end on the STOP message and no motor to be ready; choose a motor again before the next attempt. If it reads "…; releasing motor N failed: … Press STOP.", press STOP at once. | Stop at once on travel toward a hard end, a joint moving the wrong way, a growing error, a stall or a noise. Do not retry a misfit by widening poses past a safe range; fix the alignment or sign instead. Power switch if STOP fails. After a refused start, support the leg before it goes limp: the release torques off every axis; if any motor still holds or moves after "motor N released", press STOP, then the power switch. |
| HW-10 Both | As Leg only with **Both**, with the mirror shown. | "Sim + leg · gait time …". The chosen leg (blue) follows the encoders; the simulated legs move with the leg's gait clock and hold while the real leg is in "approach" or paused. Should the status go stale during the run, the gait line reads "(clock frozen)" with "Leg data stale — last read N s ago; not live", and the mirror says the same before its last reading; nothing is shown as live. | As Leg only. **Do not induce staleness on a physical leg:** a stalled server is the FPGA-watchdog case of HW-14, and a failed status poll stops drive. The stale display is covered by reading only (no driver step induces it). |
| HW-11 Mirror alignment | Tick "Show the real leg on the suspended simulated robot", choose the leg and check each motor's joint, sign and alignment pose. For each motor: move it with Q/A until the real leg matches the simulated alignment pose, wait for Holding, press **Save sim alignment here**. Then jog each a little with Q/A. Flip one sign and flip it back. Try **Run**, **Step** and **Reset** (the run must not move or rebuild). Untick. | The robot is held 0.25 m up; the chosen leg is tinted blue. Save sim alignment answers only after the save; the mirror then reads "{role}: 0.0° from its alignment pose" (a later reading may differ by a few tenths). If it still reads "not aligned — shown at {pose}", record it as a difference (fixed by reading in `afe4a486`, see the gap in the HW-11 trace), then jog a count or two. A jog moves the reading; a flipped sign reverses the simulated joint. Past a CAD limit: "· Beyond CAD limit: …" for that leg's joints only. Run, Step and Reset are each refused while mirroring ("Run refused: the leg mirror is showing the real leg on the robot; turn the mirror off …", likewise "Step refused: …" and "Reset refused: …"; Step and Reset since 2026-10-03, by reading, unexecuted; the browser page refuses only Play). The Run, Step and Reset buttons are dimmed while mirroring (2026-10-03, by reading, unexecuted); through `system_ui` or REST `robot_run` each is refused with that text. Pause stays allowed. Unticking restores the untinted robot. | Z/Escape/Stop for any unexpected jog motion. The mirror commands no motor itself. If Run, Step or Reset is accepted while mirroring (the simulated run starts, steps or rebuilds), press Pause and record it as a failure of this step. |

**What to record (every step):** the gait-lab requalification run id and date
(Leg/Both); the gait path, mode, effort and speed; the server message and the
Leg line at start and end; the new run file under `<output>/gait-runs/` and its
`supply_v` and `supply_assumed`; any refusal text; for HW-11 each motor's
saved alignment (`reference`, `reference_joint_rad`) and the mirror line after
the save and after the jog; anything that differs from the browser page. Keep
all partial records.

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
| Gait-lab requalification (HW-10) | run id / date: (required before physical Leg only / Both; not checked in code) |
| HW-10 Sim only / Leg only / Both (physical) | **unexecuted** · result: |
| HW-10a-sim, HW-10b-leg, HW-10b-both (virtual driver) | **unexecuted** (written in `80e4de27`) · receipt: |
| HW-11 Mirror alignment (physical) | **unexecuted** · result: |
| HW-11 (virtual driver) | **unexecuted** (written in `80e4de27`) · receipt: |
| Browser pages may be retired | [ ] yes  [ ] no |

Until a pass of HW-10 and HW-11 has actually run and been signed here, the
browser calibration page (gait playback and `calibration-mirror.mjs`) stays the
reference for both.
