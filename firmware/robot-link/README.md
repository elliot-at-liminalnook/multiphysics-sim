# Robot link: the bench over Wi-Fi from an ESP32-P4

A prototype that puts the bench's web control on the ESP32-P4-WIFI6-M board,
which sends the FPGA the same host protocol the simulator sends it over USB.
Nothing about how the servos are supervised changes: the FPGA stays the only
part that can promise timing or refuse a dangerous command.

```
  browser ──Wi-Fi 6 (2.4 GHz, ESP32-C6)──► ESP32-P4 ──UART 115200──► Tang Primer 25K ──1 Mbaud bus──► HX-30HM ×3
  (this page, or                           (host of the                (bridge_safety:
   the simulator through                    supervisor; hosts           leases, trips,
   the raw tunnel)                          the page)                   S2, stop pair)
```

Status: **built and tested in simulation, not yet on hardware.** The link
engine passes its tests against a behavioural bridge and against the real
supervisor Verilog under Verilator; the ESP32 firmware builds for the P4;
the FPGA bitstream with the moved host pins builds. Nothing here has been
flashed or wired, and the pin numbers on the P4 header need checking
against the board's silkscreen first (see "Before it meets hardware").

## Why this shape

- **The FPGA already has the safety layer and a host protocol.** The
  supervised bridge (`sipeed-tang-primer-25k/servo/safety.md`) arms per
  servo on fresh telemetry, trips on temperature, voltage, current, device
  error, a 200 ms telemetry lease and a 300 ms command lease, latches on the
  dock's S2 button, and stops with a PWM-zero/torque-off pair. Its host is
  a UART speaking HX bus packets plus local `FE A0` commands. The ESP32 is
  that host. Its two pins replace the dock's USB bridge; the RTL is
  unchanged (`fpga/`).
- **The ESP32 drives nothing that is not being held.** The page renews a
  session every 300 ms and a jog every 80 ms; the firmware zeroes a drive
  250 ms after the last hold and sends STOP a second after the last sign of
  the page. Heartbeats to the FPGA go only while a session is alive, so a
  closed tab, a lost radio link, or a dead ESP32 all end in the FPGA's own
  lease expiring. The co-simulation shows each of these ending in a latched
  supervisor with the drive at zero and torque off.
- **One owner at a time, STOP from anyone.** A page takes the link
  explicitly; the raw tunnel takes it only when no page has it; `STOP`
  works from any socket and from a plain HTTP POST.
- **The simulator keeps its own path.** The project rule is that hardware
  driving lives in the viewer's Rust process (`AGENTS.md`). The ESP32 adds a
  transport, not a second authority: its TCP tunnel (port 4196) is the
  FPGA's UART as a byte stream, so the existing Rust hardware layer can run
  unchanged through `socat pty,link=/tmp/fpga,raw tcp:192.168.4.1:4196` with
  `server.json` pointed at `/tmp/fpga`. The page on the ESP32 is for
  operating the bench without a laptop in the loop. Wi-Fi adds jitter of
  milliseconds either way; the FPGA's 10 ms control profile is unaffected
  because the control loop runs there, not on the host.
- **Not a transparent relay for a policy.** `servo/nn-control.md` in the
  Tang repo sketches a 2 Mbaud framed link with interpolation for a neural
  network in the loop. That is a different FPGA image (`link_core.v`) and a
  later step; this prototype keeps today's bitstream family so the bench
  keeps working as it is.

## Layout

| Path | What |
|---|---|
| `core/hx.{c,h}` | HX packet build, stream framer, supervisor status and telemetry decode. The same bytes as `servo_bus.rs` / `servo_safety.rs`; the tests check the documented vectors. |
| `core/robot_link.{c,h}` | The host engine: one transaction at a time, telemetry polling that keeps armed servos fresh, lease heartbeats only for a live session, hold-to-move with a duty cap, STOP, refusals named. No ESP-IDF, no malloc, no clock of its own. |
| `test/` | `make` — the engine against a behavioural model of the supervisor (`fake_bridge.c`), in milliseconds. |
| `cosim/` | `make` — the engine against the real `bridge.v` + `hx_safety.v` under Verilator, with UART bits at the real baud rates and a servo answering on the bus. Boot, arm, clamped drive, reverse, release, 1.5 s of lease keeping, session loss, host loss, S2. About 90 s. |
| `esp32/` | ESP-IDF 5.5 project for the ESP32-P4 (Wi-Fi through the C6 with `esp_hosted` + `esp_wifi_remote`, as the camera firmware does): UART link task, HTTP server with the page, WebSocket, raw TCP tunnel. `idf.py menuconfig` → "Robot link" for pins, baud, servo IDs, duty cap, Wi-Fi. |
| `esp32/main/index.html` | The page: STOP, take the link, arm/disarm, hold-to-jog, live telemetry and supervisor state. |
| `fpga/` | `bridge_safety_esp32.cst` and a Makefile that builds the Tang repo's bridge-safety image with the host UART on header pins G7/G8. |
| `core/gait.{c,h}` | A gait plan from the pack, the reference governor (ported, and checked against the Rust output each plan carries), and servo position goal bytes. |
| `esp32/main/gait_store.c` | The gait pack in its own flash partition: upload, the page's JSON, one plan at a time into RAM. |
| `docs/wiring.html` | Animated bench guide, opened straight from disk. **Build** wires the P4, dock, supply and servos in 14 steps. **Bus check** finds where the servo signal stops, using meter probes and readings. `#check-3` links to a check, and `?at=4000` opens a step at that moment. |

## The page's protocol

WebSocket at `/ws`, JSON text frames. From the page: `claim`, `ping`,
`stop`, `arm {id}`, `disarm {id}`, `hold {id, duty}` (repeat ≤ 100 ms),
`release`. From the firmware: `state` ten times a second (owner, link
counters, FPGA status with latch reason, every servo's telemetry and
supervision, the current jog, the last refusal), and `refused {why}`.
`GET /api/state` gives the same object; `POST /api/stop` stops.

What the firmware refuses, by name: a hold without a live session, while
the FPGA is latched, on a servo the FPGA has not armed, or on a servo whose
control-mode register is not 2 (open-loop PWM); arming a servo that has
never answered a telemetry read; a second owner. What the FPGA refuses it
drops silently, which the engine counts as a timeout and reports as
"refused by the FPGA, or not answering".

## Build and test

```sh
# Engine tests (any machine):
make -C test
# Against the real RTL (oss-cad-suite's verilator on PATH, Tang repo at ~/projects):
make -C cosim
# Firmware:
cd esp32 && . ~/esp/esp-idf/export.sh && idf.py set-target esp32p4 && idf.py build
# FPGA image with the ESP32 host pins (does not program the board):
make -C fpga impl/bridge_safety_esp32.fs
```

Flashing the P4 (`idf.py -p PORT flash monitor`) and loading the dock
(`openFPGALoader`, see the Tang repo) are operator actions.

## Before it meets hardware

1. **P4 pins.** Defaults are GPIO 20 (TX) and 21 (RX). Confirm both are on
   the header and free: 14–19 and 54 are the C6's SDIO and reset, 24/25 USB,
   37/38 the console UART, 7/8 the camera I2C. Change them in menuconfig.
2. **FPGA pins.** G7/G8 are chosen for being next to the servo pins on the
   same header; the Tang repo settled where F5/G5 are by reading the
   underside silkscreen, so do the same for G7/G8. The dock's USB UART must
   not be connected to those pins at the same time.
3. **Common ground** between the P4 and the dock; both are 3.3 V I/O.
4. **Baud.** 115200 for `bridge_safety`; the device-clock experiment image
   runs its host link at 1 Mbaud (`menuconfig` → baud).
5. **Commission like any new host** (safety.md): small drive, then S2 stop,
   host-disconnect stop (close the tab), telemetry-loss stop, rearm. Watch
   the encoder until stationary; an acknowledgement is not a stop.
6. **Travel windows.** `bridge_safety` has none; only the calibration
   profile enforces taught poses. The jog is open-loop PWM with a 100/1000
   cap by default: watch the mechanism.

## Calibrating the printed leg over Wi-Fi

The leg (IDs 1 knee, 2 worm, 3 belt/hip) uses the Tang repo's calibration
profile: taught travel windows, bounded PWM, 400/600 ms leases. `make -C fpga
calibration` builds it with the ESP32 host pins. The robot-link page shows the
leg's telemetry. The Rust calibration panel does the teaching and motion,
unchanged, through the P4's raw tunnel:

```sh
# The P4 in station mode prints "station address A.B.C.D" on its USB console.
python3 firmware/robot-link/tools/tunnel_pty.py A.B.C.D --link /tmp/fpga &
# The leg's server.json with "serial" set to /tmp/fpga, nothing else changed:
python3 -c "import json;d=json.load(open('examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json'));d['serial']='/tmp/fpga';json.dump(d,open('/tmp/leg-wifi.json','w'))"
target/debug/examples/serve_actuator_calibration /tmp/leg-wifi.json 4194
```

The tunnel holds the link while it is open. STOP on the robot-link page still
reaches the FPGA then, written straight onto the UART. Closing the tunnel
sends STOP. esp_hosted is pinned to the C6's 2.12 line: with 3.0.9, station
mode stopped carrying traffic within a minute (2026-10-08).

## Gaits on the page

The page lists the leg's gaits and previews each one, and plays a playable
gait on the leg. Nothing about a gait is defined on the ESP32: the Mac builds
a gait pack from the same sources the calibration panel plays from, and
uploads it.

```sh
cargo run -p sim-runtime --example leg_gait_pack -- \
    examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json \
    --out /tmp/leg.rlgp --push 192.168.1.39        # --effort 0.5 --hz 25 --supply 11.1
```

The pack is built by `sim_runtime::hardware::calibration::gait_pack`, and its
format is documented there. It contains:

- **Gaits:** the gait catalog and `compiled_with_governor`.
- **Bindings:** each motor's CAD joint and polarity, taken from the newest
  panel gait run. Alignment and windows come from the leg's
  `calibration.json`.
- **Limits:** the per-motor governor limits from the accepted actuator
  registry (`govern_leg`, the panel's own code).
- **The leg figure:** every link of the leg as its CAD collision mesh
  (simplified on a 3 mm grid), with each link's transform relative to its
  parent sampled over the one motor that moves it (hip output on the hip
  servo, thigh and worm on the worm servo, crank, curved link and crosshead on
  the foot servo). The page composes them for any motor angles: each gait
  frame (stored as motor angles), the real leg as read, and each place a worm
  reading allows. The pack refuses to build if the composed figure differs
  from the kinematic mirror solved directly by more than 2 mm at any mesh
  vertex (the stored pack: 0.08 mm). `leg_gait_pack … --svg FILE [--live IP]`
  draws the same composition, side and top, for checking by eye.

Playing on the leg follows the panel's `prepare_drive` path, step for step
(`core/robot_link.c`, "gait playback"):

1. For each motor: STOP, the taught window anchored to the live encoder, arm,
   torque off, PWM zero, re-arm, the plan's control mode, park (the goal on
   the current pose, or speed zero), heartbeat, torque on. Knee and hip run
   in servo position mode (`servo_command(ServoPosition)`); the worm runs in
   servo speed mode with a position trim (`servo_command(ServoSpeed)`).
2. The gait's governor runs live on the ESP32 and approaches the first pose
   with the gait clock held.
3. Once every motor has arrived, it streams goals every 30 ms.

The governor is `reference_governor::update` ported to `core/gait.c`. Every
plan carries the Rust governor's output for 3 s from rest. Selecting a gait
replays it and refuses a plan that differs by a count or more; the fixture
differs by 0.05 counts.

**The worm's turn.** The worm's taught travel (−1407..3116 counts) is
longer than the encoder's one turn, so a reading alone does not say where it
is. Its poses are encoder counts plus whole turns, as the panel counted them
while teaching, so they stay valid once the worm's current turn is known:

- The page draws the leg at each place the reading allows inside the travel
  (±256 counts): one place for most readings, two a full turn apart for
  readings 2689..3116. The operator picks the drawing that matches the leg
  (`turn_confirm`), or "None of these", which moves nothing.
- From then on the ESP32 counts turns on every reading, as the panel's
  `EncoderTurns` and the FPGA do. It forgets the turn, and stops a gait that
  drives the worm, on a missed or corrupt reading, a half-turn jump, more
  than 600 ms between readings, the tunnel or bus scan taking the link, a
  stop that was not verified, or a new pack. A verified stop keeps it.
- The window sent to the FPGA is anchored at the counted turns, and the
  FPGA counts from there. The FPGA still refuses speed past the window; what
  it cannot know is whether the window is on the right turn. That is the
  operator's confirmation.
- Position mode cannot drive the worm: a goal across encoder zero passes the
  FPGA's nearest-turn check but the servo goes the long way round, past its
  taught end. A plan with a multi-turn motor in position mode does not parse.

A motor is left out of the pack only if its poses and alignment do not share
one turn count (alignment outside the travel, or saved in another session),
or its travel is two turns or more.

**Arming and teaching on the page (calibration profile).** The leg's FPGA
image arms a motor only inside a window it was sent, anchored to the live
reading. The pack carries every motor's taught travel (an axes table at the
start of its plans section), so the page needs no gait to arm:

- **Arm** sends the motor's taught window, arms it, zeroes PWM and puts it in
  open-loop PWM mode (unlocking the mode register only to change it). Hold
  ◄ ► to move. Leaving the window does not trip the FPGA; it only drops
  drive further outward. So the ESP32 zeroes a held jog itself when the motor
  reaches the window's edge.
- **Recovery.** A motor found outside its travel (a knee that drooped with
  torque off) is armed with the window stretched only to where it already
  is: it can come back in, never further out.
- **Teaching.** *Open* an end, jog to the new end while watching, then *Set*
  it there; *Revert* goes back to the pack's travel. Poses set on the page are
  used at once, kept in NVS against the pack they were taught on, and marked
  as not promoted. A gait refuses a motor whose poses changed until the Mac
  promotes them:
  `leg_gait_pack SERVER.json --pull-calibration 192.168.1.39 [--reason "…"]`
  writes them into `calibration.json` (the previous file kept beside it, the
  change recorded in `esp32-poses-<ms>.json`; a widened travel raises a limit
  and needs `--reason`). Then build and push the pack again. Storing a
  pack keeps a motor's taught poses when the pack leaves that motor's travel
  unchanged; `--push` refuses a pack that would replace poses taught on the
  page without carrying them. Poses are named as in the calibration panel:
  a reversed motor (the worm) has its lower pose at its high-count end.

**What stops a gait:** STOP, a lost page session, focus loss or a hidden tab,
Escape, an FPGA latch, a refused goal, or telemetry outside the panel's
commissioning limits. Every one ends with a verified stop: torque off, PWM
zero, and the shaft still on two readings.

**Recording runs:** `leg_gait_pack ... --pull 192.168.1.39` saves the last
run (`GET /api/gait_log`) as `gait-runs/esp32-run-<ms>.json` beside the
panel's runs.

HTTP endpoints:

| Endpoint | Purpose |
|---|---|
| `GET /api/gaits` | The stored pack's JSON. |
| `POST /api/gaits` | Upload a pack: digest-checked, header written last. |
| `POST /api/gait {"action":"select","offset":…,"length":…}` | Choose a gait. |
| `POST /api/gait {"action":"stop"}` | Stop the gait. |
| `GET /api/calibration` | Each motor's travel: the pack's, as taught on the page, open ends, position, turn places. |
| `POST /api/calibration {"action":"set_lo"\|"set_hi"\|"open_lo"\|"open_hi"\|"revert","id":1,"operator_at_leg":true}` | Teach a motor's travel at its reading; only for someone at the leg. |
| `POST /api/gait {"action":"confirm_turn","id":2,"counts":-300,"operator_sees_leg":true}` | Confirm the worm's turn (one of `gait.axes[k].candidates` in `/api/state`); only for someone looking at the leg. |
| `GET /api/gait_log` | The last run's samples. |

Playing requires the page's live session (the WebSocket `gait_play`), because
one front end owns a motion session.

Tests:

- **`make -C test`:** the plan fixture (knee, worm, hip), the governor
  check, the position and speed bytes against the Rust test vectors, the
  worm's places and confirmation, turns counted across encoder zero and
  forgotten on a missed reading or a tunnel, and preparation, approach,
  play, pause, speed and verified stop against the calibration-profile model
  (multi-turn servo, speed mode, the FPGA's own turn count). A missed worm
  reading mid-gait stops it.
- **`make -C cosim cal`:** the same run against the real RTL built with the
  calibration image's parameters. The worm starts at −300 (reading 3796) and
  crosses encoder zero into its gait. It covers every goal and speed
  accepted, a replay after a verified stop, STOP and a lost session ending in
  a verified stop, and the RTL's own leases stopping a silent host.

## Not in this prototype

- The simulator's Rust hardware layer talking TCP itself (today: a pty via
  socat). A `tcp://` port in `server.json` is the natural next step.
- The camera. The same board's camera firmware (the stereo-camera project)
  serves MJPEG on port 81; merging the two is a later step and the P4's
  ISP/JPEG work would run beside the link task.
- The calibration page's taught-pose and tuning flows, which live in the
  Rust `calibration` service; they reach the robot through the tunnel.
- The 2 Mbaud framed link and FPGA-side interpolation of `nn-control.md`.
