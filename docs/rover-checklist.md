# Rover drive: the goal-4 checklist

This checklist covers goal 4 of the user's current focus
(`tools/claude-pair/prompts/mission.md`, "Current focus", item 4): an agent
drives the whole flow through the REST API. It creates a robot in CAD, rigs
its joints and annotates it. It wires the robot in the systems editor, writes
its controller as an external program on the controller seam
(`clients/python` simloop) and hooks that into the systems editor. Then it
drives the robot from a keyboard or gamepad in the viewer and the browser.
The acceptance case is a small two-wheel (differential-drive) rover built by
a committed example script.

Controls go through three layers that don't know about each other:

1. **Device bindings**, per device and shared across robots
   (`sim.drive-bindings/1`, the viewer preferences' `drive_bindings` group).
   Keys, sticks and buttons become normalized axes and named actions.
2. **A drive profile**, per robot and owned with its model (`sim.drive/1`,
   `<stem>.drive.json` named by `<stem>.controller.json`). It is validated
   through the registry's `control.drive_limiter` description.
3. **A kinematic adapter in the robot's controller**: the external Python
   program `clients/python/examples/diff_drive_rover.py` turns the body twist
   into wheel position targets. In the browser, which cannot start a
   process, the binding's `embedded` Rhai adapter
   `examples/wheeled-robot/drive-adapter.rhai` does the same through the
   shared Rust drive functions, in the shared embedded session
   (`robot.controller.json:6`; [RV-38](#rv-38-browser-driving)). It is a
   compatibility surface; the Python program on the native seam stays the
   reference.

The proof robot is `examples/wheeled-robot/baseline/robot.simrobot.json`,
with `robot.controller.json` and `robot.drive.json` beside it.

**Status legend.**

- **Implemented by reading (batch rover-drive-layers)**: written in that
  batch and traced below from the control to the effect.
- **Implemented by reading (unexecuted)** (batch rover-rest-flow, RV-01 to
  RV-05, RV-07 and RV-40): written in that batch and traced below from the
  REST command to RoboCAD or the runtime.
- **Implemented by reading (unexecuted)** (batch rover-browser-drive, RV-38
  and RV-41 to RV-43): written in that batch and traced below from the
  device (a browser key or pad; a native key or pad in Build mode) to the
  wheel targets.
- **Hardware (run sheet, never driven by the agent)**: needs the physical
  robot. The agent writes a run sheet and never drives hardware.

**Everything here is by reading, unexecuted.** Nothing was compiled or run:
no cargo build, check or test, no Python or Node test, no viewer, no
browser, no screenshot. The Rust tests (`crates/sim-domain-control/tests/drive.rs`,
`crates/sim-spatial/src/robot/run/tests.rs:596-717`,
`crates/sim-runtime/tests/controller_binding.rs`, and from rover-rest-flow
`crates/sim-runtime/src/drive_host/tests.rs`,
`crates/sim-runtime/tests/system_robot.rs`,
`crates/sim-runtime/src/cad_client/physical_tests.rs:155` and
`crates/sim-spatial/src/cad/results/tests.rs:386`) and the Python test
(`clients/python/tests/test_drive.py`) are written and have never run. The
REST script `clients/python/examples/build_rover_over_rest.py` has never
run either. From rover-browser-drive, also written and never run:
`crates/sim-spatial/src/drive_input/tests.rs` (it replaces the deleted
`robot/drive_input/tests.rs`), `crates/sim-runtime/src/drive_bindings/tests.rs`,
`crates/sim-runtime/src/embedded_drive/tests.rs`,
`crates/sim-script/src/drive/tests.rs`, the `HeartbeatDeadman` case added to
`crates/sim-domain-control/tests/drive.rs` (471-509), the inline tests in
`crates/sim-runtime/src/controller_binding.rs` (377 onward),
`device_tests` in `crates/sim-spatial/src/builder/robot_run.rs` (481-525),
the device case in `crates/sim-spatial/src/robot/actions/tests.rs` (169)
and the Node test `web/tests/drive_input.mjs`. Line numbers are from the
working tree on 2026-10-02 (rover-drive-layers) and 2026-10-03
(rover-rest-flow; then rover-browser-drive, which moved the device input to
`crates/sim-spatial/src/drive_input/`, the bindings format to
`crates/sim-runtime/src/drive_bindings.rs`, added
`DriveRequest::interpret_with` to `crates/sim-runtime/src/drive_host.rs` and
the `embedded` field to `controller_binding.rs`; the RV-06 to RV-37 citations
into those files were refreshed then; the rover-browser-drive review fixes
moved lines again, and the citations into the files they touched were
re-checked by reading on 2026-10-03). The citations into files that
focus-safety-closure changed were then refreshed against its working
tree (2026-10-03). Other batches are editing some of
these files at the same time, so a cited line can drift by a few lines.

## Steps

| ID | Step | Status | Trace |
|---|---|---|---|
| RV-01 | Create the rover in CAD through REST | Implemented by reading (unexecuted) | [RV-01](#rv-01-create-the-rover-in-cad-through-rest) |
| RV-02 | Rig its joints (two driven continuous axles, one passive) through REST | Implemented by reading (unexecuted) | [RV-02](#rv-02-rig-its-joints) |
| RV-03 | Wheel, motor and encoder parts through REST; save, export and the `.rcad` hash | Implemented by reading (unexecuted) | [RV-03](#rv-03-parts-save-export-and-the-hash) |
| RV-04 | Annotate it: threads pinned to its nodes | Implemented by reading (unexecuted) | [RV-04](#rv-04-annotate-it-threads-pinned-to-its-nodes) |
| RV-05 | Wire it in the systems editor | Implemented by reading (unexecuted) | [RV-05](#rv-05-wire-it-in-the-systems-editor) |
| RV-06 | Controller program on the seam (binding file + `diff_drive_rover.py`) | Implemented by reading | [RV-06](#rv-06-controller-program-on-the-seam) |
| RV-07 | Hook the controller into the systems editor (Build-mode live run on the shared drive host) | Implemented by reading (unexecuted) | [RV-07](#rv-07-build-live-run-and-system_drive) |
| RV-08 | Device bindings: keyboard | Implemented by reading | [RV-08](#rv-08-device-bindings-keyboard) |
| RV-09 | Device bindings: gamepad | Implemented by reading | [RV-09](#rv-09-device-bindings-gamepad) |
| RV-10 | Bindings settings (REST `drive_bindings`, preferences) and defaults | Implemented by reading | [RV-10](#rv-10-bindings-settings-and-defaults) |
| RV-11 | Drive profile file and validation | Implemented by reading | [RV-11](#rv-11-drive-profile-file-and-validation) |
| RV-12 | Registry description `control.drive_limiter` | Implemented by reading | [RV-12](#rv-12-registry-description) |
| RV-13 | Geometry derived from the model, with provenance | Implemented by reading | [RV-13](#rv-13-geometry-derivation-with-provenance) |
| RV-14 | Limits against the motors; deadman against the control period | Implemented by reading | [RV-14](#rv-14-limits-against-the-motors-deadman-against-the-period) |
| RV-15 | Kinematic adapter: Rust reference mixers and the golden file | Implemented by reading | [RV-15](#rv-15-rust-reference-mixers-and-the-golden-file) |
| RV-16 | Kinematic adapter: Python port checked against the golden file | Implemented by reading | [RV-16](#rv-16-python-port) |
| RV-17 | Robot mode opens a model with its binding | Implemented by reading | [RV-17](#rv-17-robot-mode-opens-a-model-with-its-binding) |
| RV-18 | Run: the run thread builds the session and starts Python | Implemented by reading | [RV-18](#rv-18-run-builds-the-session-and-starts-python) |
| RV-19 | Twist path: a held W to the wheel motor targets | Implemented by reading | [RV-19](#rv-19-twist-path-held-w-to-the-motor-targets) |
| RV-20 | Stop: release | Implemented by reading | [RV-20](#rv-20-stop-on-release) |
| RV-21 | Stop: window focus loss | Implemented by reading | [RV-21](#rv-21-stop-on-focus-loss) |
| RV-22 | Stop: Escape | Implemented by reading | [RV-22](#rv-22-stop-on-escape) |
| RV-23 | Stop: the Stop button, X, South and the `stop` action | Implemented by reading | [RV-23](#rv-23-stop-button-and-the-stop-action) |
| RV-24 | Halt: B, East and the `halt` action | Implemented by reading | [RV-24](#rv-24-halt) |
| RV-25 | Deadman | Implemented by reading | [RV-25](#rv-25-deadman) |
| RV-26 | Stop: a text field takes the keyboard | Implemented by reading | [RV-26](#rv-26-stop-when-a-text-field-takes-the-keyboard) |
| RV-27 | Pause | Implemented by reading | [RV-27](#rv-27-pause) |
| RV-28 | Leaving Robot mode | Implemented by reading | [RV-28](#rv-28-leaving-robot-mode) |
| RV-29 | REST `robot_drive` | Implemented by reading | [RV-29](#rv-29-rest-robot_drive) |
| RV-30 | `system_ui` drive:* controls | Implemented by reading | [RV-30](#rv-30-system_ui-drive-controls) |
| RV-31 | Inspector Drive panel | Implemented by reading | [RV-31](#rv-31-inspector-drive-panel) |
| RV-32 | `robot_state` drive fields | Implemented by reading | [RV-32](#rv-32-robot_state-drive-fields) |
| RV-33 | Save a drive recording | Implemented by reading | [RV-33](#rv-33-save-a-drive-recording) |
| RV-34 | Replay reproduces a drive recording | Implemented by reading | [RV-34](#rv-34-replay-reproduces-a-recording) |
| RV-35 | Replay refused by identity | Implemented by reading | [RV-35](#rv-35-replay-refused-by-identity) |
| RV-36 | A controller crash is an error naming it | Implemented by reading | [RV-36](#rv-36-controller-crash) |
| RV-37 | Preset motion keys and drive input coexist | Implemented by reading | [RV-37](#rv-37-preset-coexistence) |
| RV-38 | Browser driving (keyboard and gamepad, embedded Rhai adapter) | Implemented by reading (unexecuted) | [RV-38](#rv-38-browser-driving) |
| RV-39 | Hardware twist path | Hardware (documentation only) | [RV-39](#rv-39-hardware-twist-path-run-sheet) |
| RV-40 | The committed example script that runs the flow over REST | Implemented by reading (unexecuted) | [RV-40](#rv-40-the-committed-example-script) |
| RV-41 | Build mode: the keyboard drives the live robot-system run | Implemented by reading (unexecuted) | [RV-41](#rv-41-build-mode-keyboard) |
| RV-42 | Build mode: a gamepad drives the live robot-system run | Implemented by reading (unexecuted) | [RV-42](#rv-42-build-mode-gamepad) |
| RV-43 | Build mode: stops, and the drive strip's bound keys and twist | Implemented by reading (unexecuted) | [RV-43](#rv-43-build-mode-stops-and-the-drive-strip) |

## Before you start (for the person who runs it later)

- Open the rover in Robot mode:
  `cargo run -p sim-spatial -- --robot examples/wheeled-robot/baseline/robot.simrobot.json`
  (`examples/wheeled-robot/README.md`, "Teleoperation drive").
- For the REST flow (RV-01 to RV-07, RV-40), start the viewer in CAD mode
  on the seed instead:
  `cargo run -p sim-spatial -- examples/wheeled-robot/baseline/robot.rcad`,
  then run `clients/python/examples/build_rover_over_rest.py --out DIR`
  ([RV-40](#rv-40-the-committed-example-script)).
- For the browser (RV-38), package the `rover-drive` preset
  (`web/viewer/presets.json:527-541`) with
  `node web/build-viewer.mjs runs/interactive/viewer --preset=rover-drive`
  (the output directory must come first: it is `process.argv[2]`,
  `web/build-viewer.mjs:9`; `packageDrive`, 44-67, copies the source files
  only) over a current
  sim-web WASM build, then open it in the viewer page and press Play.
- For Build mode (RV-41 to RV-43), wire and run the rover system as in RV-05
  and RV-07, then press Run.
- `python3` must be on `PATH`: `sim_couple::python` runs `python3 -u script
  args…` (`crates/sim-couple/src/native.rs:168-178`).
- The physics is uncalibrated, and the limits are estimates (see
  [Known limits](#known-limits)).

## Reading traces (by reading, unexecuted)

### RV-06 Controller program on the seam

1. **The binding file** `examples/wheeled-robot/baseline/robot.controller.json`
   names the language `python`, the script
   `../../../clients/python/examples/diff_drive_rover.py`, empty `args`, and
   the drive profile `robot.drive.json`. It carries no physical value. Its
   optional `embedded` program (`drive-adapter.rhai` with
   `drive-adapter.config.json`, `robot.controller.json:6`) is the browser's
   adapter (RV-38); the native viewer never runs it.
2. **Parsing**: `ControllerBinding::from_json`
   (`crates/sim-runtime/src/controller_binding.rs:87-110`). The schema comes
   first; a newer `sim.controller-binding/N` is named as newer (95-105). Then
   serde, with unknown fields refused (`deny_unknown_fields`, 33). Then
   `validate` (112-149): only `python` (114-120), a non-empty script, no
   `--drive-json` in `args` (the host adds it, 124-126), a non-empty
   `drive_profile`, and for an `embedded` program only `rhai` with a
   non-empty entry, files and config (130-147). A binding without
   `embedded` reads unchanged (`EmbeddedController`, 41-69).
3. **The program**: `diff_drive_rover.py` parses `--drive-json` into
   `ResolvedDrive` (`clients/python/examples/diff_drive_rover.py:62-73`,
   `clients/python/simloop/drive.py:349`). It builds the mixer (74,
   `drive.py:376`) and `DriveState(limits, deadman)` (75; `limit_live`
   defaults to False, `drive.py:484`). The targets
   `{"left axle.target", "right axle.target"}` start at 0.0 (76).
4. **Handshake check**: `check` (78-84) refuses a hello that lacks the four
   command sensors or the wheel target actuators, naming them. It runs inside
   `Loop.__init__` before `ready` (`clients/python/simloop/__init__.py:128-134`),
   so the host's handshake fails with that reason.
5. **Per frame** (98-121): rollback on a re-sampled time (101-106), then
   `DriveState.update` (109; `drive.py:494-510`), `mixer.mix` (110),
   `target += period × rate` (115-116) and `loop.send(**targets)` (117;
   `__init__.py:206-231`, which keeps any unmentioned actuator at its
   previous value). Logs go to stderr only (53-54).

By reading, unexecuted.

### RV-08 Device bindings: keyboard

1. **Control**: hold W (forward +1), S (−1), A (yaw +1, turn left), D
   (yaw −1), Q/E (lateral; ignored on the rover).
2. Defaults: `BindingsFile::default`
   (`crates/sim-runtime/src/drive_bindings.rs:190-224`), the one parser and
   set of defaults shared with the browser. The native viewer maps the names
   to Bevy keys (`crates/sim-spatial/src/drive_input/bindings.rs:20-32`).
3. Input system: the one device poller `drive_input::input::devices`
   (`crates/sim-spatial/src/drive_input/input.rs:177-351`), registered by
   `DriveInputPlugin` in `InputSet::Window` for every mode
   (`crates/sim-spatial/src/drive_input/plugin.rs:24`). It reads nothing
   unless the current mode's `DriveTarget` is live (`live_target`, 95-97,
   checked at 214-221). Robot mode writes that target before the poller
   (`robot::controls::drive_target`, `crates/sim-spatial/src/robot/controls.rs:639-659`,
   registered at `robot/mod.rs:236`): live only for a controlled run
   (`robot_target`, 625-632). Physical keys come from
   `ButtonInput<KeyCode>` (input.rs:178), not text; Cmd/Ctrl/Alt chords are
   skipped (`chord`, 227, read at 256 and 292).
4. `DriveBindings::keyboard_axes` (`drive_input/bindings.rs:132-134`, called
   at input.rs:293) asks the shared `Resolved::keyboard_axes`
   (`drive_bindings.rs:396-405`), which sums the held keys per axis and
   clamps each axis to −1..1 (403).
5. `supported_only` (`drive_bindings.rs:444-456`, used at input.rs:329-330)
   zeroes the axes the profile lacks and names them in `DriveInput.ignored`.
   On the rover, Q held with W drives forward and lists `lateral`.
6. Nonzero axes are sent every frame as a quiet `Act<DriveDevice { mode:
   Robot, Axes }>`, with a redraw request (input.rs:339-343; `send`,
   223-226). Robot mode forwards it as `RobotAction::Drive` (RV-19 step 2).
   Release: [RV-20](#rv-20-stop-on-release). From here the path is RV-19
   step 3.

By reading, unexecuted.

### RV-09 Device bindings: gamepad

1. **Control**: left stick Y forward, right stick X yaw (inverted, so stick
   left is +yaw), left stick X lateral (inverted), South `stop`, East `halt`,
   deadzone 0.15 (`crates/sim-runtime/src/drive_bindings.rs:210-221`; sign
   rule in the module notes, 20-28).
2. `devices` reads every `Gamepad` component
   (`crates/sim-spatial/src/drive_input/input.rs:299-301`), but drives from
   them and reads their buttons only while a window of this app has focus
   (295, 302-307).
3. `DriveBindings::gamepad_axes` (`drive_input/bindings.rs:139-141`) asks
   the shared `Resolved::gamepad_axes` (`drive_bindings.rs:408-420`), which
   passes each stick through `shape` (433-438): zero inside the deadzone,
   the rest of the travel rescaled to 0..1. NaN reads as zero (434).
4. After a stop, the pad stays blocked until it is focused, neutral and has
   no bound button held (input.rs:308-316).
5. Keyboard and pad are added and clamped per axis (`add`, 83-86, at 332),
   and the source is named (`keyboard`, `gamepad` or `keyboard+gamepad`,
   333-338). From here it is the same as RV-08 step 6.

By reading, unexecuted.

### RV-10 Bindings settings and defaults

1. **Control**: REST `drive_bindings` with no arguments (read),
   `{"bindings": {…}}` (set) or `{"reset": true}`. Spec:
   `crates/sim-spatial/src/app/settings/actions.rs:34-44`.
2. `apply` → `drive_bindings` (actions.rs:59, 65-76). Setting a value runs
   the shared `BindingsFile::from_value`
   (`crates/sim-runtime/src/drive_bindings.rs:284-292`): schema first, then
   serde (unknown fields refused), then `validate` → `resolve` (294-362).
   Every refusal names its field, for example
   `drive_bindings.keyboard.axes[2].key`. Reserved keys G/C/J/F/H, Space,
   Enter, Tab and Escape are refused with the reason (`RESERVED`, 46-56;
   checked at 312-314). A key or button bound twice is refused (302-310).
   The browser validates a stored override with the same function
   (RV-38 (d)).
3. `SettingsOwner::set_drive_bindings`
   (`crates/sim-spatial/src/app/settings/mod.rs:238-252`) validates again and
   raises the revision. The snapshot written through the preferences
   publication includes the group (`jobs::with_drive_bindings`,
   `app/settings/jobs.rs:394`; also on shutdown, `mod.rs:336-351`). On load,
   the group is read and refused by name if invalid (`jobs.rs:159-168`), and
   it reaches the owner at `app/settings/plugin.rs:202`.
4. `drive_input::plugin::sync_bindings`
   (`crates/sim-spatial/src/drive_input/plugin.rs:36-50`, registered at 27
   in JobResults after `SettingsSet::Publish`, every mode) rebuilds
   `DriveBindings` (`DriveBindings::new`, `drive_input/bindings.rs:112-124`)
   with `set_if_neq`.
5. The defaults are data in code and are stored only once the user sets
   bindings (`drive_bindings.rs:14-16`). `reset` stores None, so a later
   change to the defaults reaches the user.
6. Shown: the answer `{schema, stored, bindings, describe,
   w3c_standard_gamepad}` (`drive_bindings::json`, `drive_bindings.rs:462-471`)
   plus `settings` status (actions.rs:73-74). The inspector's DEVICE
   BINDINGS block (RV-31), `robot_state.bindings` (RV-32) and Build mode's
   drive strip (RV-43) show the same bindings.

By reading, unexecuted.

### RV-11 Drive profile file and validation

1. **File**: `examples/wheeled-robot/baseline/robot.drive.json`. It is
   differential on `left axle`/`right axle`, has geometry
   `{"source":"model"}`, axes `forward` (0.26 m/s, 0.5 m/s², stop 1.0) and
   `yaw` (4.3 rad/s, 8.0, stop 16.0), actions `stop` and `halt`, and deadman
   0.5 s `ramp`. Each provenance is `estimated` with its derivation written
   out.
2. Loaded by `DriveProfile::load`
   (`crates/sim-domain-control/src/drive/profile.rs:331-337`), which returns
   the sha256 of the bytes, then `from_json` (311-326): the schema first, then
   serde (the failing section is named), then `validate` (341-433).
3. `validate` checks wheel joints named and distinct (345-354), at least
   one axis (358-360), known axis names (361-363), no lateral on a
   differential (364-366), units equal to `SPEED_UNITS`/`ACCEL_UNITS`
   (367-374), provenance present (376-378, 383-385), numeric ranges
   through the registry (387-395, see RV-12), unique action names
   (396-406) and complete declared geometry with sources and ±1 signs
   (407-431).
4. Every error is `DriveProfileError { file, field, message }`, shown as
   `{file}: {field}: {message}`. The binding loader passes it on verbatim
   (`controller_binding.rs:328`).

By reading, unexecuted.

### RV-12 Registry description

1. `limiter_descriptor` (`profile.rs:697-725`) declares `control.drive_limiter`
   (`LIMITER`, 32): inputs `request.forward|lateral|yaw` and `sequence`,
   outputs `twist.*`, parameters `max_speed.<axis>` (≥ 0),
   `max_accel.<axis>` and `stop_decel.<axis>` (> 0), `deadman_timeout`
   (> 0), `on_loss_immediate` (0 or 1) and `period` (> 0). It also carries
   notes (`LIMITER_NOTES`, 646, attached at 723).
2. It is registered in the shared registry
   (`crates/sim-domain-control/src/elements.rs:147`). So the systems editor,
   exports and Rhai see the same description.
3. A profile is validated against the same declarations without `period`
   (`profile_descriptor`, 728-734), mapped through `limiter_parameters`
   (439-450) and `validate_parameters` (389-395). A refusal maps back to the
   profile field (`parameter_field`, 296).
4. The element itself: `from_parameters` (747-806) refuses a
   `deadman_timeout` shorter than `period` (800-803; the shared bound,
   see "Safety closure" (c)). `sample` (813…)
   applies the same rule as `kinematics::step`.

By reading, unexecuted.

### RV-13 Geometry derivation with provenance

1. `sim_domain_robot::drive_geometry::resolve`
   (`crates/sim-domain-robot/src/drive_geometry.rs:230-245`), called through
   `controller_binding::resolve_drive` (`controller_binding.rs:273-277`) by
   the binding loader (329) and by the browser's scene builder
   (`embedded_drive.rs:239`, RV-38 (g)). Model geometry goes to `derive`
   (174-223).
2. `derive` refuses a mecanum drive from the model (176-183) and requires
   gravity along −z (185-188). Per wheel (`wheel`, 64-118): the sign comes
   from the joint axis's +y component (95), and the radius from the wheel
   link's collision vertices about the axis (96-114). The wheels must share
   a parent (`same_parent`, 120), have equal radii (`equal_radii`, 133-151)
   and lie on one axle line (198-204). Left must be at +y (205-211).
3. Each value is `Provenance::Derived { from }` naming the joints, links
   and method: track width (213-217), wheel radius (in `equal_radii`), and
   each wheel's sign (`wheel_joint`, 152-165). Missing data is an error that
   names it. Nothing defaults.
4. For the rover (design numbers, not run): track 0.12 m from the axle
   origins y = ±0.06 m, radius 0.03 m, both signs +1 (axis [0,1,0]).
5. Shown with provenance in the inspector's Drive profile block
   (`drive_detail`, `crates/sim-spatial/src/robot/controls.rs:786-816`) and in
   `robot_state.drive.geometry` (`run/controlled.rs:202`).

By reading, unexecuted.

### RV-14 Limits against the motors; deadman against the period

1. `resolve` then calls `check_deadman` and `check_speeds`
   (`drive_geometry.rs:242-243`).
2. `check_deadman` (252-259) requires `deadman.timeout_s` to be strictly
   longer than the model's `control.period_s`. The rover has 0.5 s against
   0.02 s.
3. `check_speeds` (317-354) takes the slowest wheel's speed limit
   (`joint_speed_limit`, 268-290: the motor's gearbox `max_output_speed` /
   `gear_ratio`, or through a transmission) times the wheel radius as
   `v_free`. It refuses a forward or lateral `max_speed` above `v_free`, and
   a yaw `max_speed` above `v_free / lever` (track/2 for a differential),
   naming the axis, the ceiling and the derivation (346-351). A wheel joint
   without a motor is refused (287-289).
4. Rover: 14.66 rad/s × 0.03 m = 0.44 m/s, so 0.26 m/s passes; the yaw
   ceiling 2 × 0.44 / 0.12 = 7.3 rad/s, so 4.3 rad/s passes. These are the
   profile's own provenance numbers; the computation was not run.

By reading, unexecuted.

### RV-15 Rust reference mixers and the golden file

1. `kinematics.rs` (`crates/sim-domain-control/src/drive/kinematics.rs`,
   standard library only): `scale` (197-215), `check_twist` (219-232),
   `limit` (236-252), `deadman_expired` (256-258) and `step` (273-286).
2. `DifferentialDrive::mix` (317-328): lateral ≠ 0 is refused as
   `Unsupported`, left/right = (forward ∓ track/2·yaw)/radius × sign.
   `Mecanum::mix` is at 327.
3. Golden vectors: `crates/sim-domain-control/tests/fixtures/gen_drive_golden.rs`
   compiles `kinematics.rs` standalone and prints
   `tests/fixtures/drive_golden.json` (schema `sim.drive.golden/1`,
   tolerance 1e-12, command recorded in `generated_by`). The Rust test
   `golden_vectors_match_the_shared_kinematics`
   (`crates/sim-domain-control/tests/drive.rs:54`) checks at least 30
   cases (89).
4. Legs: `steered::command_steered` (`drive/steered.rs:13`) feeds the same
   `BodyTwist` to `SteeredGait`.

By reading, unexecuted (the golden file was generated by the Part A worker;
not re-run here).

### RV-16 Python port

1. `clients/python/simloop/drive.py` ports `scale`, `check_twist`, `limit`
   (193-208), `deadman_expired` (211-213), `step` (216-230) and
   `DifferentialDrive` (253-279, `mix` 261-270) in the Rust evaluation order
   (module notes 15-21). `Mecanum` is at 282.
2. `DriveState` (460-510) is the controller-side deadman. A heartbeat is
   fresh only when it rises (497-500); heartbeat 0 means no request (501-502).
   When expired, or with `limit_live=True`, `step` applies (504-505). With
   `limit_live=False`, a live twist is checked and passed through (506-508).
3. `clients/python/tests/test_drive.py` reads the same golden file (20,
   47-48) and checks `scale`, `limit`, `step`, `differential` and `mecanum`
   (65-92), plus `DriveState` cases (154 onward).

By reading, unexecuted.

### RV-17 Robot mode opens a model with its binding

1. **Control**: `--robot FILE` (`crates/sim-spatial/src/main.rs:322-344`,
   `RobotView::open`, 340), or a switch to Robot mode with a path.
2. `RobotView::open` (`crates/sim-spatial/src/robot/state.rs:7-11`) →
   `SourceWatch::open` (`robot/source.rs:204-208`) → `spawn` (226-233), a
   `jobs::Job` on `Pool::Compute`. The UI thread never reads the files.
3. On the worker, `check_controlled` (131-160) calls `load_controller`
   (`robot/loader.rs:121-133`). No binding file means `None` and the hold
   run; any other error is `Some(Err)` naming the binding.
4. `controller_binding::load` (`controller_binding.rs:297-359`): parse
   (300-301), canonical script and its sha256 (304-311), `clients_root` = the
   nearest `clients` ancestor (313-321), the simloop library hash (322-323),
   the profile (325-328), geometry, checks and the resolved drive
   (`resolve_drive`, 329; 273-277; RV-13/RV-14), `--drive-json` appended
   (333-335), the `ExternalProgram` with `profile_sha256` and
   `library_sha256` (336-349), and the `ControllerIdentity` (350-357).
5. `ControlledRun::new` (`robot/run/controlled.rs:52-56`) builds the scene
   (`controller_binding::scene`, 363-365 → `scene_with`, 372-375: period =
   the model's `control.period_s`, duration `DRIVE_DURATION_S` 600 s, 23).
6. Back on the UI thread, `receive` (`robot/scene.rs:172-181`) calls
   `RunController::spawn_file` (`run/controller.rs:168-179`) or
   `replace_file` (451). A loaded binding becomes `Source::Controlled`; a
   failed one becomes `Source::Unbound`, a failed run naming it from the
   start (180-192). It never falls back to a hold run. The run thread
   starts (`RunThread::spawn`, 194).
7. Shown: the Drive block (RV-31), `robot_state.drive` (RV-32) and the
   header's reload note (`scene.rs:218`).

By reading, unexecuted.

### RV-18 Run builds the session and starts Python

1. **Control**: Run (Space, `run:start`, REST `robot_run {"action":"start"}`)
   → `RunController::act` (`run/controller.rs:282-288`) → `Command::Start`
   → worker (`run/worker.rs:339-349`) → `build` (112-137) →
   `Sim::build` (`run/sim.rs:58-61`).
2. `Session::new(scene, seed 0)` (`crates/sim-runtime/src/session.rs:326`).
   `program.validate` and `check_external` (352-357; 586-607) check the
   script's sha256 and the simloop library's sha256 on disk against the
   recorded ones. A change is refused naming both hashes.
3. The plant is built, the seam is taken and its `<joint>.target` bounds come
   from joint limits (359-381). The inputs are the binding's four command
   channels (`drive_inputs`, `controller_binding.rs:166-184`), checked at
   402-421.
4. `spawn_external` (431-433 → 682-688) → `sim_couple::python`
   (`crates/sim-couple/src/native.rs:168-178`) with a 3 s reply timeout
   (`EXTERNAL_REPLY_TIMEOUT`, session.rs:677).
5. `Runtime::attach` (`crates/sim-compile/src/runtime.rs:233-241`) →
   `External::couple` (`crates/sim-domain-control/src/external.rs:158-165`)
   → `EpisodeCoupler::open` (session.rs:197-205) appends the command
   channels to the contract's sensors → `FrameCoupler::open`
   (native.rs:107-116) sends hello and waits for ready. A failed handshake
   (RV-06 step 4) is reported right away (session.rs:464-471).
6. `Sim::build` calls `DriveHost::new` (`run/sim.rs:59`;
   `crates/sim-runtime/src/drive_host.rs:278-282`), which builds the
   session (step 2) and runs `check_inputs` (`drive_host.rs:103-109`) to
   confirm the four channels in order, with a default `TwistState`. The run
   is `Sim::Controlled { host, run }` (`run/sim.rs:60`).

By reading, unexecuted.

### RV-19 Twist path: held W to the motor targets

1. **Control**: hold W → the one poller `devices` writes
   `Act::quiet(DriveDevice { mode: Robot, request: Axes { forward: 1,
   lateral: 0, yaw: 0 } })` every frame
   (`crates/sim-spatial/src/drive_input/input.rs:339-343`), for the target
   Robot mode's writer offers (`robot/controls.rs:625-659`).
2. `actions::forward_devices` (`robot/actions/mod.rs:794-800`, registered
   in `ViewerSet::Actions` before `RobotSet::Actions` at `robot/mod.rs:238`)
   turns each Robot-mode request into `Act<RobotAction::Drive>` with the
   poller's origin (`device_action`, 784-786; a request stamped for another
   mode is skipped). `actions::apply` (619, `RobotSet::Actions`,
   `robot/mod.rs:239`) → `handle` (552-599, catch-all at 597) → `dispatch`
   (295). Drive takes its own branch (307-317).
3. `drive_request` (`actions/mod.rs:111-117`): with no binding, refused
   naming the reason (112-115). Then the shared interpretation
   `DriveRequest::interpret` (`crates/sim-runtime/src/drive_host.rs:79-81`)
   → `interpret_with` (85-98, the one Build mode and the browser use too):
   `Axes` → `kinematics::scale(axes, &resolved.limits())` (88-89): 1.0 ×
   0.26 = 0.26 m/s forward. Actions: `profile.action(name)` → Stop/Halt
   (92-95).
4. `RunController::drive` (`run/controlled.rs:171-181`) →
   `check_drive_request` (157-167): `check_drive` (134-151: recorded
   preset, no binding, replay, failed or ended), `check_twist` (160), and a
   nonzero request only while running (161-165). Then
   `thread.send(Command::Twist { request, halt })` (177;
   `run/protocol.rs:126`).
5. Run thread: `drain` (`run/worker.rs:39-51`) applies every queued command
   in order (140). `Command::Twist` (327-337) → `Sim::twist`
   (`run/sim.rs:97-105`) → `DriveHost::request` (`drive_host.rs:298-301`,
   at the host's sim time) → `TwistState::request` (141-159):
   `check_twist`, heartbeat + 1 (149), and `last_request_s` = sim now (150).
   The status is published at once (worker.rs:334).
6. Each pass while running: `s.advance()` (worker.rs:404) → `Sim::advance`
   for Controlled (`sim.rs:93`) → `DriveHost::step` (`drive_host.rs:310-317`)
   → `TwistState::advance` (165-175) → `kinematics::step(commanded,
   request, period, now − last_request_s, …)` (167), clamped to
   ±max_speed (169). The action is `[f, l, y, heartbeat]` (172) →
   `Session::step` (`drive_host.rs:314`; the twist state is committed only
   when the step ran, 315; `session.rs:489-518`): bounds checked
   (503-510), the values stored (511), the action recorded (512), and `robot.advance(period_s)` (513).
7. In `PhysicalRobot::advance` (`crates/sim-runtime/src/physical.rs:662-698`),
   the seam's scheduled event fires `External::jump` (external.rs:141-156)
   → `External::sample` (64-102) → `EpisodeCoupler::sample`
   (session.rs:206-238): sensors plus the command values (212-217), then
   the actuators are bounds-checked against CAD limits (220-229) →
   `FrameCoupler::sample` (native.rs:118-130) sends `sample` and expects
   `act` with the same `seq`.
8. Python: `Loop.__next__` (`__init__.py:184-204`) →
   `diff_drive_rover.py:107-117`: `DriveState.update` passes the live
   0.26 m/s through → `DifferentialDrive.mix` gives [8.67, 8.67] rad/s
   (0.26 / 0.03) → each target += 0.02 × 8.67 → `act`.
9. The actuators are held (`External::sample`, 99) on the seam's
   `act.<joint>.target`, which the build wires to each motor's servo
   firmware `target` port (`physical.rs:456-457`). The CAD motor firmware
   tracks the position reference through the H-bridge and motor.
10. Shown: link poses through `RunController::poll`
    (`run/controller.rs:324`, drive status at 362-363; called from
    `scene.rs:377`), the Drive block's requested/commanded lines (RV-31)
    and `robot_state.drive.status` (RV-32).

By reading, unexecuted.

### RV-20 Stop on release

1. **Control**: release every driving key and centre the stick.
2. `devices`: axes are zero and `latch.sending` was true, so it writes one
   quiet `DriveDevice { Axes 0,0,0 }`
   (`crates/sim-spatial/src/drive_input/input.rs:344-348`), forwarded as
   `RobotAction::Drive` (RV-19 step 2).
3. RV-19 steps 2-5 with a zero twist. Zero is accepted in any phase that
   can take a twist (`controlled.rs:161`). On the run thread the request
   is ZERO with a fresh heartbeat, and `kinematics::step` ramps the
   commanded twist down at `max_accel` (0.5 m/s², so about 0.52 s from
   0.26 m/s; `kinematics.rs:285`).
4. Python passes the limited twist through; the wheel targets stop
   advancing as the twist reaches 0.

By reading, unexecuted.

### RV-21 Stop on focus loss

1. **Control**: click another application while W is held.
2. `devices` reads `WindowFocused { focused: false }`
   (`crates/sim-spatial/src/drive_input/input.rs:192`) → stop reason "the
   window lost focus" (254-255). It is sent only if the devices were
   driving (`latch.sending`), so a REST client's requests are left to the
   deadman (comment 251-253).
3. It writes `Act::ui(DriveDevice { Stop })` (263-266; `send`, 223-226) →
   forwarded (RV-19 step 2) → `drive_request` → `interpret_with`: `Stop` →
   `(ZERO, false)` (`drive_host.rs:96`) → RV-19 steps 4-5. Then `disarm`
   (229-235, called at 270): held keys are blocked until released and the
   pad is blocked until neutral (308-316).
4. Bevy also releases every key on focus loss, and the pad is not read
   without focus (295, 302).

By reading, unexecuted.

### RV-22 Stop on Escape

1. **Control**: Escape (not bindable:
   `crates/sim-runtime/src/drive_bindings.rs:55`).
2. `devices` (`crates/sim-spatial/src/drive_input/input.rs:256-257`):
   Escape sends Stop whenever no text field has the keyboard and no chord is
   held. It is shown in the header only when the devices were driving;
   otherwise it goes quietly (`shown` = `latch.sending`, 257; sent at 265).
   The Leg calibration panel's own Escape STOP still runs (doc at 133-138).
3. As RV-21 step 3: `Stop` → zero twist, approached under `max_accel`,
   then disarm.

By reading, unexecuted.

### RV-23 Stop button and the stop action

1. **Controls**: the Drive block's **Stop** button
   (`robot/controls.rs:715`, a kit button carrying `RobotAction::Drive {
   Stop }`); **X** (key action `stop`,
   `crates/sim-runtime/src/drive_bindings.rs:209`); gamepad **South**
   (219); REST `robot_drive {"stop": true}` or `{"action":"stop"}`;
   `system_ui drive:stop` / `drive:action:stop`.
2. Button: `actions::buttons` (`actions/keys.rs:8-12`) writes `Act::ui`;
   an accepted Stop also disarms held inputs (see "Safety closure" (a)).
   Key or button action: `devices` sends a zero request first if driving,
   then `DriveDevice { Action { name } }`, then disarms
   (`crates/sim-spatial/src/drive_input/input.rs:317-326`; the action names
   are collected at 294 and 305); forwarded as
   `RobotAction::Drive` (RV-19 step 2).
3. `drive_request` → `interpret_with`: `Stop` → `(ZERO, false)`
   (`drive_host.rs:96`); `Action "stop"` → `profile.action` →
   `ActionRequest::Stop` → `(ZERO, false)` (92-93). An unknown name is
   refused listing the profile's actions (`profile.rs:494`).
4. RV-19 steps 4-5. The run thread ramps to zero under `max_accel`.

By reading, unexecuted.

### RV-24 Halt

1. **Controls**: **B** (`crates/sim-runtime/src/drive_bindings.rs:209`),
   gamepad **East** (220), the Drive block's **halt** button (one per
   profile action, `robot/controls.rs:716-718`; halt at 717), REST
   `{"action":"halt"}`, `system_ui drive:action:halt`.
2. `interpret_with`: `ActionRequest::Halt` → `(ZERO, true)`
   (`drive_host.rs:94`). A halt passes `check_drive_request` in any phase
   (`controlled.rs:161`).
3. Run thread: `TwistState::request` with `halt` sets both the request and
   the commanded twist to ZERO at once (`drive_host.rs:152-155`). The next
   period sends 0. The Python `DriveState` passes it through unlimited
   (`drive.py:506-508`), so the targets stop advancing in that period and
   the firmware holds them.

By reading, unexecuted.

### RV-25 Deadman

1. **Cause**: no fresh request for `timeout_s` (0.5 s) of simulation time,
   for example a REST client that stops sending, or a single `system_ui`
   `drive:forward`.
2. Run thread: `TwistState::advance` computes age = now −
   `last_request_s` (`drive_host.rs:166`) → `kinematics::step` (167) →
   `deadman_expired` (`kinematics.rs:256-258, 277-282`) → ramp at
   `stop_decel` (rover `ramp`) or zero. `expired` is recorded
   (`drive_host.rs:171`).
3. Controller: the heartbeat stops rising, and `DriveState.update`
   (`drive.py:503-505`) applies the stop rule from its last output once its
   own age passes `timeout_s`. It is the guard if the run thread's
   channels stop changing.
4. Shown: "deadman EXPIRED (on loss: ramp)" in the Drive block
   (`robot/controls.rs:736`) and `robot_state.drive.deadman.expired`
   (`controlled.rs:204-205`). The rule text is `DEADMAN_RULE`
   (`controlled.rs:31`).

By reading, unexecuted.

### RV-26 Stop when a text field takes the keyboard

1. **Control**: hold W, then click a kit text field (for example the gait
   path field or the comment composer).
2. `Typing::get` (`crates/sim-spatial/src/ui_kit/text/mod.rs:161-169`) turns
   true. `devices` sees `typing_started`
   (`crates/sim-spatial/src/drive_input/input.rs:195-196`) while the
   keyboard was driving (258-259) and sends one Stop, then disarms
   (263-270). While typing, no key is readable (292).

By reading, unexecuted.

### RV-27 Pause

1. **Control**: Pause (Space while running, `run:pause`, REST).
2. `RunController::act` sets `running = false` and sends `Command::Pause`
   (`run/controller.rs:289-292`). The worker sets `running = false` and,
   outside a replay, invalidates the live request (`run/worker.rs:351-363`
   → `Sim::pause_drive`, `run/sim.rs:119-123` → `DriveHost::pause`;
   `PAUSE_RULE`, see "Safety closure" (b)), then blocks in `drain` for the
   next command. No simulation time passes while paused.
3. While paused, a nonzero request is refused on the UI thread
   (`controlled.rs`, `NOT_RUNNING`) and on the run thread if it raced the
   Pause. Stop and halt are accepted.
4. Robot mode's apply writes `Disarm` for the accepted Pause (no Stop
   sent; "Safety closure" (a)), so a key or stick still held is ignored
   until released. On Run the profile's on-loss rule stops the robot
   from the commanded twist it had, until a fresh request arrives
   (focus-safety-closure, 2026-10-03; by reading, unexecuted).

By reading, unexecuted.

### RV-28 Leaving Robot mode

1. **Control**: switch to another mode, or close the window.
2. `leave_robot` (`crates/sim-spatial/src/app/switch/leave.rs:70-78`)
   removes `RobotView` and drops it off the UI thread
   (`jobs::drop_off_thread`). `RunThread::drop`
   (`crates/sim-spatial/src/jobs/run_thread.rs:109-125`) closes the command
   channel.
3. The worker's `drain` reports the channel closed. It applies what is
   queued, then returns (`worker.rs:111, 385-387`). Dropping the `Session`
   drops the `FrameCoupler`, whose `close` sends `close`, closes stdin and
   reaps or kills `python3` within 250 ms (`native.rs:132-160`).
4. On leaving, `drive_input::leave_mode` (`robot/mod.rs:216`;
   `crates/sim-spatial/src/drive_input/mod.rs:140-143`) clears the drive
   target and the device status, so the poller reads nothing more for
   Robot mode. Robot mode's systems, `forward_devices` among them, stop
   running outside Robot mode (`robot/mod.rs:247`), and a request still
   buffered for Robot mode is never applied by another mode, whose reader
   skips it (`DriveDevice`, `drive_input/mod.rs:66-75`). A carried REST
   call is abandoned (`robot/mod.rs:217-224`).

By reading, unexecuted.

### RV-29 REST `robot_drive`

1. **Control**: `POST robot_drive {"forward":0.5,"lateral":0,"yaw":0}`,
   `{"action":"halt"}` or `{"stop":true}`. Spec:
   `robot/actions/commands.rs:22`.
2. `wire::Command::RobotDrive` (`actions/mod.rs:899`) →
   `TryFrom` (`commands.rs:85`) → `DriveRequest::from_fields`
   (`crates/sim-runtime/src/drive_host.rs:58-71`, shared with Build's
   `system_drive`). It needs exactly one of axes, action or `stop: true`,
   and names the fields given when they are mixed (65-68). Absent axes
   are 0.
3. RV-19 steps 2-6. REST is strict: a nonzero lateral on the rover is
   refused by `scale` naming `lateral` (`kinematics.rs:206-209`), where the
   device layer would zero it.
4. While live motor sync streams, axes requests from REST are refused
   (`actions/mod.rs:652`; `moves_synced_motors`, 838-847, axes only at
   842); stop and actions still pass.
5. A refusal is kept in `robot_state.drive.last_refusal`
   (`controlled.rs:174, 210`) and in `drive_input.last_error`
   (`actions/mod.rs:710-723`). The answer is `robot_state` with `bindings`
   and `drive_input` (724-744).
6. A REST client must repeat its request faster than `timeout_s`; the
   deadman stops it otherwise (RV-25).

By reading, unexecuted.

### RV-30 `system_ui` drive controls

1. **Control**: `system_ui {"action":{"operation":"controls"}}` then
   `activate` with the id and `ui_revision`.
2. `controls` (`actions/mod.rs:388-460`) adds `drive_controls` for a
   controlled run (432-435). `drive_controls` (464-478) provides
   `drive:forward|back|left|right`, each one full-axis request labelled
   momentary with the deadman, plus `drive:stop` and
   `drive:action:<name>` per profile action. Ids are listed at
   `commands.rs:41-42`.
3. `Activate` (`actions/mod.rs:578`) → `dispatch` → RV-19 steps 3-6. A
   `drive:*` refusal updates `drive_input.last_error` (710-723).

By reading, unexecuted.

### RV-31 Inspector Drive panel

1. `controls::drive_panel` (`robot/controls.rs:673-782`), registered in
   Present (`robot/mod.rs:245`). It is built only for a controlled run
   (685-692) and rebuilt when the run, roots or actions change (696-728).
2. Buttons: Stop and each profile action (709-718), enabled by `check`
   (711, 779-781).
3. Live lines (729-778): requested/commanded twist with fixed decimals, the
   deadman, age, heartbeat and sim time (732-746); device input axes,
   source, ignored axes and last refusal from `DriveInput`
   (`crates/sim-spatial/src/drive_input/input.rs:31-49`; 747-761); RUN
   FAILED (762); the DEVICE BINDINGS table (763-773). A text is rewritten
   only when its value changes (775-777).
4. Static detail (`drive_detail`, 786-816): script and sha256, binding,
   profile and sha256, description, kinematics, geometry with provenance,
   resolved limits per axis with units, and the deadman.

By reading, unexecuted.

### RV-32 `robot_state` drive fields

1. `state_json` sets `drive` = `RunController::drive_json` for a controlled
   run (`robot/state.rs:216-220`).
2. `drive_json` (`run/controlled.rs:187-214`) holds: label, fidelity,
   availability, binding, model, profile path and sha256, kinematics,
   geometry, per-axis limits with units (196-198), identity, rule, deadman
   (timeout, on_loss, clock rule, expired, age), session (seed, period,
   duration, channels), requested, status, `accepts_motion`,
   `last_refusal`, `last_apply_error` and error. An unbound run reports
   `bound: false` with the binding error (191-193).
3. `with_drive_input` (`state.rs:139-143`) adds `bindings` and
   `drive_input` (null unless controlled), read from the resources
   `crate::drive_input::DriveBindings` and `DriveInput`. Every answer and
   the 100 ms publication pass through it (`actions/mod.rs:729, 871`).

By reading, unexecuted.

### RV-33 Save a drive recording

1. **Control**: Save recording, `system_ui recording:save`, or REST
   `robot_save_recording` (`commands.rs:23`).
2. `RunController::save_recording` (`run/preset_ops.rs:180-200`): for a
   controlled run the target is `recording::drive_target` →
   `runs/robot-drive/<stem>/<stamp>.recording.json`
   (`robot/recording.rs:165-167`, `DRIVE_LOCATION_RULE` 343). Then
   `Command::SaveRecording`.
3. Worker (`run/worker.rs:164-186`) → `Sim::save` for Controlled
   (`run/sim.rs:236-241`, the host's session): `Session::recording()`
   (`session.rs:561-568`: scene with `controller.external` including the script and library
   hashes and `--drive-json`, seed, one `[f,l,y,heartbeat]` per period) and
   `drive_meta` (`recording.rs:352`). Written on a `Pool::Io` job with
   `create_new`, never overwritten (`recording.rs:184-217`).
4. Shown: `recording.pending` and then `last_saved` in `robot_state`.

By reading, unexecuted.

### RV-34 Replay reproduces a recording

1. **Control**: a Replay button, `system_ui replay:<file>` or REST
   `robot_replay {"file": …}` (`commands.rs:25`).
2. `RunController::replay` (`run/preset_ops.rs:305-325`) →
   `drive_replay_source` (`recording.rs:296-298`) → `Command::Replay` →
   worker (`worker.rs:188-234`) → `prepare_replay` (`run/replay.rs:98`)
   → `prepare_drive_replay` (177-208).
3. The identity check is RV-35. `Session::new(recorded scene, recorded
   seed)` starts the recorded controller again, and `check_external`
   re-verifies the script and library hashes (`session.rs:586-607`).
4. Each chunk: `Sim::advance_replay` (`run/sim.rs:209-215`) steps one
   recorded action through `DriveHost::step_recorded`
   (`drive_host.rs:320-325`: `Session::step`, then `TwistState::replayed`,
   181-191). Live twists are refused during it (`worker.rs:272`;
   `controlled.rs:143-145`).
5. End: `finish_replay` → `end_drive_replay` (`replay.rs:213-214`;
   `sim.rs:108-112` → `DriveHost::replay_ended`, `drive_host.rs:327-330` →
   `TwistState::replay_ended`, 200-209), so nothing keeps driving. Verdict
   done/failed at `replay.rs:223-225` (`DRIVE_VERDICT_RULE`, `recording.rs:347`). States
   are not compared.

By reading, unexecuted.

### RV-35 Replay refused by identity

1. Edit `diff_drive_rover.py`, `simloop/*.py` or `robot.drive.json`, or the
   model, then Replay an older recording.
2. `differences` (`run/controlled.rs:85-122`) compares through
   `ControllerIdentity::differences` (`controller_binding.rs:212-227`:
   script, `script_sha256`, `library_sha256`, args, profile and
   `profile_sha256`). It also compares the language (99-101), the resolved
   drive members (102-111), the robot fingerprint (112-115) and the seam
   period (116-120). Each difference is named with both values.
3. Refused (`replay.rs:198-202`): "refused by the drive identity check: …".
   The worker republishes the current run unchanged
   (`worker.rs:225-234`).
4. The loader also notices on reload: a changed binding identity changes
   the fingerprint (`source.rs:59-63`).

By reading, unexecuted.

### RV-36 Controller crash

1. **Cause**: the Python process exits, raises (for example a refused twist,
   `diff_drive_rover.py:111-114`), sends a malformed `act`, or takes longer
   than 3 s.
2. `FrameCoupler::receive` (`native.rs:91-97`) returns `Timeout` or
   `Exited("stream closed (status)")`. `EpisodeCoupler` relabels it
   `external controller (python) <script> on <element>: …`
   (`session.rs:189-194, 217`; label 65-67). `External::jump` keeps it as
   the failure (`external.rs:142-146`). The runtime's commit returns
   `RuntimeError::Controller` (`crates/sim-compile/src/runtime.rs:280-287`).
3. `Session::step` stores the error (`session.rs:513-516`), and later steps
   are refused (490-492). `Sim::advance` errors, and the worker sets the
   phase to Failed with "advance failed at t = … s: …" (`worker.rs:452-467`).
4. Shown: RUN FAILED in the Drive block (`robot/controls.rs:762`),
   `robot_state.drive.error` (`controlled.rs:212`). Further drive requests
   are refused (`controlled.rs:147`). Reset rebuilds.

By reading, unexecuted.

### RV-37 Preset coexistence

1. Preset motion keys act only while `motion_keys_active()`
   (`run/preset_ops.rs:28-30`, checked at `actions/keys.rs:50`). Drive input
   acts only while Robot mode's drive target is live, which needs
   `controlled()` to be Some (`robot/controls.rs:627`; the poller's
   `live_target`, `crates/sim-spatial/src/drive_input/input.rs:95-97`,
   checked at 214). A run is one or the other (`controlled.rs:126-128`;
   `check_drive` refuses a preset by name, 139; the rule is the poller's
   doc, input.rs:110-114).
2. On a controlled run, motion requests and held inputs are refused naming
   the controller (`run/sim.rs:146, 176`; `preset_ops.rs:44`), and so are
   jogs (`run/controller.rs:228-229`).
3. Both run in `InputSet::Window`: the motion keys in Robot mode's chain
   (`robot/mod.rs:232`), the device poller from `DriveInputPlugin`
   (`drive_input/plugin.rs:24`), after Robot mode writes its target
   (`robot/mod.rs:236`).

By reading, unexecuted.

### RV-01 Create the rover in CAD through REST

Before the script runs, a windowed viewer must be in CAD mode on an
existing `.rcad`:
`cargo run -p sim-spatial -- examples/wheeled-robot/baseline/robot.rcad`
(`crates/sim-spatial/src/main.rs:246`). RoboCAD's service cannot start on a
path that does not exist yet (`Document.load(a.path)`,
`cad/robocad/api.py:1943`; `self_start` refuses a missing file,
`crates/sim-spatial/src/cad/sync/launch.rs:51-53`, and so does the mode
switch, `crates/sim-spatial/src/app/switch/prepare.rs:285-287`). So the
seed is only opened. It is never edited or saved. Native paths below are
under `crates/sim-spatial/src/` unless they start with `crates/`.

1. **Enter CAD mode** (`open_new_document`,
   `clients/python/examples/build_rover_over_rest.py:239-249`):
   `viewer_mode {"mode":"cad","path": seed}` when CAD mode is not active
   (243) → `app/switch/prepare.rs:273-292`: a `.rcad` that exists, else
   refused by name; `CadTarget::File` → CAD mode's `cad/sync/mod.rs:start`
   (76; `CadTarget::File` at 125-128 → a `Pool::Dedicated` job running
   `self_start`, `launch.rs:47`) → `service_command`
   (`crates/sim-runtime/src/cad_client/service.rs:69-80`: `python -m
   robocad.api <absolute path> --port N --host 127.0.0.1`). The script polls
   `cad_state.connection.state` until it is `connected` (245-246).
2. **New document**: `cad_file {"op":"new","path": DIR/robot.rcad}` (247)
   → `cad/actions.rs:582` → `cad/files/mod.rs:275` (`handle`) → `file`
   (300), `FileOp::New` (335-355): an absolute `.rcad` path (336), refused
   while a switch blocker such as a running export is live (341-344), then
   `jobs::start` (`cad/files/jobs.rs:143`, a `Pool::Dedicated` job) →
   `CadClient::new_file` (`crates/sim-runtime/src/cad_client/files.rs:158-160`,
   `POST /new`) → `api.py:1740` → `Service.new_file` (`api.py:1321`: the
   file is created exclusively; an existing one is never replaced). The REST
   call waits (`jobs.rs:181`), then `open_created` (`jobs.rs:204-214`) runs
   `cad_open` on the new file (`cad/actions.rs:624`) → `sync::start` again:
   the seed's service is stopped (`sync/mod.rs:81-83`) and a new
   self-started service opens DIR/robot.rcad. The script polls until
   `cad_state.health.path` is that file (248-249).
3. **Geometry and materials** (`build_fixture`, script 267-310, the values
   of `cad/scripts/wheeled_learning_fixture.py`): `cad_op box` for the
   chassis (271), `cylinder` for the left and right wheels (275) and the
   passive wheel (290), `set_material … "petg"` for each (272, 276, 291).
4. **One `cad_op`, end to end** (every RV-01 to RV-03 op takes this path):
   - REST args `{"name","args","kwargs"}` → `CadAction::CadOp`
     (`cad/actions.rs:205-211`), decoded by `sim_api::decode`
     (`crates/sim-api/src/lib.rs:564`).
   - Apply: `cad/actions.rs:551-560`. Component ops and
     `set_component_graph` are refused by name (552-557). Otherwise `edit`
     (`cad/edit.rs:12`) → `sync::start_edit` (`cad/sync/mod.rs:686-693`, a
     `Pool::Dedicated` job). A REST caller keeps `{"edit": seq,
     "generation"}` as its continuation and waits (`edit.rs:34-36`).
   - Client: `CadClient::op` (`crates/sim-runtime/src/cad_client/mod.rs:374-376`,
     `POST /ops/{name}`) → RoboCAD route `api.py:1668-1671` →
     `Service.op` (`api.py:1011-1028`) → `ArgConverter.convert`
     (`api.py:221-229`; `_one`, 231-284: a list of three numbers for a
     `Vec3` parameter becomes a tuple, 270-271, with a bare three-number
     fallback at 276-277; dicts and `null` pass
     through) → the `Ops` method → `{"result", "history"}`.
   - Ops (`cad/robocad/commands.py`): `set_material` 345, `box` 419,
     `cylinder` 437.
   - Answer: `finish_edit` (`cad/sync/mod.rs:565`) keeps `{"message",
     "result"}` for the waiting caller (624-628). The next pass of `handle`
     sees the continuation (`cad/actions.rs:461-462`) and `wait_edit`
     (602-619) answers. `result` is RoboCAD's `OpResult`
     (`crates/sim-runtime/src/cad_client/types.rs:171-175`), so the new
     node id is at the job value's `result.result` (`op_result_id`,
     script 252-258).

By reading, unexecuted.

### RV-02 Rig its joints

1. **Driven axles**, per side (`build_fixture`, script 273-289):
   `add_joint ["continuous", chassis, wheel, pivot, [0,1,0]] {name:
   "<side> axle"}` (279-280; `commands.py:935`), `attach_motor [joint,
   motor]` (281; `commands.py:992`) and `set_joint_physics [joint]
   {drive_backlash: {width_rad, provenance: "estimated", reference,
   uncertainty_rad}, friction: {…}}` (282-286; `commands.py:1184`). The
   dicts pass through `ArgConverter` unchanged (`api.py:231-284`).
2. **Passive axle** (292-294): a continuous joint with friction only and
   no motor.
3. Pivots in mm: left (25, 60, 30), right (25, -60, 30), passive
   (-55, 0, 20). Every axis is +y.
4. Every call takes the `cad_op` path of RV-01 step 4.

By reading, unexecuted.

### RV-03 Parts, save, export and the hash

1. **Parts** (`build_fixture`): `add_motor ["n20_100", mount point, shaft
   dir] {mount_on: chassis, name}` (277-278; `commands.py:966`),
   `add_sensor ["encoder", wheel, point] {joint, name}` (287-288;
   `commands.py:1047`), the body IMU (295), `set_battery` 5-cell NiMH (296;
   `commands.py:1145`), `set_control` period 0.02 s (297;
   `commands.py:1151`), and `set_robot_setting` for `world` (298-299) and
   `benchmark_assumptions` (300-309; `commands.py:1090`). Each takes the
   `cad_op` path of RV-01 step 4.
2. **Save** (`save_and_export`, script 370-391): once `cad_state` shows no
   edit in flight, no stale document and `health.path` = DIR/robot.rcad
   (`wait_cad_settled`, 313-323), `cad_save {}` (373) →
   `cad/actions.rs:540-543` → `cad/files/mod.rs:406` (`save`; no path, so
   the document's own) → `edit` with `save_with_thumbnail` (413-418;
   `cad_client/files.rs:152-154`, `POST /save/thumbnail`) → `api.py:1736`
   → `Service.save_with_thumbnail` (`api.py:1296`: `path or
   self.doc.path`) → `Document.save` (`cad/robocad/document.py:514-522`:
   writes the archive, then `dirty = revision != saved revision`, 522).
3. **Export**: `cad_results {"op":"export","kind":"rigid","path":
   DIR/robot.simrobot.json}` (375) → `cad/actions.rs:586` →
   `cad/results/mod.rs:317` (`handle`). `kind` is accepted only with `op:
   export` (323-325). The Export arm (353-365) takes
   `ExportKind::Rigid.shape()`: flex false, planar false, "rigid physical
   model" (128-141) → `export::request` (`cad/results/export.rs:221-246`).
   One export runs at a time (`admit`, 191-200); a second one from REST is
   refused with "a model export is already running" (only a live-link
   export queues and answers `{queued, seq, message}`). The answer is
   `{started, path, seq, message}` (243).
4. **The job** (`start`, `export.rs:252-277`, `Pool::Dedicated`): `GET /`
   (259), then `CadClient::physical_model(false, false)` (260;
   `crates/sim-runtime/src/cad_client/physical.rs:190-193`, `GET
   /physical?flex=0`, never `path=`) → `api.py:1765-1772` → `Ops.physical`
   (`commands.py:1233`). RoboCAD writes `source.file = doc.path`
   (`cad/robocad/physical.py:959`). A cancel before the write leaves no
   file (`export.rs:262-264`).
5. **The `.rcad` hash** (`stamp_saved_source`, `physical.rs:255-284`,
   called at `export.rs:265`). `saved_file` (`physical.rs:227-242`) needs
   a RoboCAD answer, a path, an absolute one, and `dirty == false`.
   `source.file` must equal that path exactly (266-270). The file is hashed
   (`sha256_file`, `crates/sim-domain-robot/src/cad_link.rs:30-32`). A
   second `GET /` must show the same path and document id (273-275), still
   no unsaved edits (276-278) and the same revision (279-281). Only then is
   `source.cad_sha256` written (282). Otherwise the model is written
   without it and the reason is kept (`export.rs:267-270`). `write_model`
   writes through a temporary file and a rename (295-315).
6. **Completion**: `poll` (`export.rs:354-420`) lands `Last { seq, …,
   cad_sha256, cad_sha256_reason }` (389) through `Exports::land`
   (163-169): `last` and the bounded `recent` (`RECENT` = 8, 63), both in
   `Exports::json` (142-161; `last_json`, 173-179). The script polls
   `cad_state.results.exports.recent` and `last` for its `seq`
   (`export_outcome`, script 229-236), requires `ok` and compares
   `cad_sha256` with its own sha256 of DIR/robot.rcad (383-390).
7. **CAD link in Robot mode**: `cad_link::status` (`cad_link.rs:51-68`)
   uses an absolute `source.file` as is (`candidates`, 40-44), hashes it
   and compares it with `cad_sha256` (63-67). A self-started service was
   given the absolute path (`service.rs:70`), so `doc.path` and with it
   `source.file` is absolute, and the stamped export reads Current.
8. **Drive geometry from the export** (the profile written for RV-06 says
   `geometry: {"source": "model"}`): `drive_geometry::resolve`
   (`crates/sim-domain-robot/src/drive_geometry.rs:230`) → `derive` (174).
   Track = left anchor y − right anchor y (205): 0.06 − (−0.06) = 0.12 m.
   Wheel radius = the largest distance of the wheel's collision vertices
   from the joint axis (103-112): about 0.03 m. Sign +1 from the +y axis
   (95). Only the profile's `left` and `right` joints are read, so the
   passive axle is not in the mixer. The baseline test asserts these
   numbers on the fixture's export of the same recipe
   (`the_wheeled_robot_derives_its_drive_geometry`, 369-374). The script
   mirrors the radius rule to choose its limits (`wheel_reading`, script
   400-424) and writes `robot.drive.json` and `robot.controller.json` with
   exclusive create (`write_profile_and_binding`, 427-492; 487-490).

Design numbers, not run.

By reading, unexecuted.

### RV-04 Annotate it: threads pinned to its nodes

1. **Calls** (`annotate`, script 342-367): four `cad_threads
   {"op":"create","node","point","body","author"}` (348-359), pinned to
   the chassis, the left axle joint, the right axle joint and the passive
   wheel, each saying what the part is for; one `reply` on the left axle
   thread whose body carries `[left wheel](part:<id>)` (362-364); one
   `link` adding the two drive motors and the IMU to the chassis thread
   (365-366). Each goes in an ordered batch `[{op: list}, {op: …}]` after
   `cad_state` shows no edit and no stale document (`threads_call`,
   334-339). `list` waits for the threads at the current revision
   (`cad/threads/ops.rs:171-175`), so `reply` and `link` find the thread
   (`known`, 123).
2. **Apply**: `CadAction::CadThreads` → `cad/actions.rs:589` →
   `cad/threads/ops.rs:167` (`handle`).
   - `Create` (210) → `create` (384-412): a given `node` needs a `point`
     (395) and must be in the shown tree (`has_node`, 396-398;
     `cad/document/state.rs:49-51`; a joint is a tree node). The anchor is
     a `CadAnchor::Surface` named after the node (409) → `ThreadOp::Create`
     (410).
   - `Reply` (211-232) → `ThreadOp::Reply` (232).
   - `Link` (313) → `link` (433-456): ids not yet linked become
     `CadAnchor::part` (451) → `ThreadOp::Link` (455).
3. **Commit**: `commit` (`ops.rs:68-74`) → `annotations::apply` →
   `CadThreadSource::commit` (`cad/threads/source.rs:423`) → `send` (321)
   → `edit_auxiliary_at` (328; `cad/edit.rs:65-78`, refused by name while
   another edit is in flight or RoboCAD's revision moved) →
   `Request::send` (`source.rs:283-293`): `create_thread` (285;
   `crates/sim-runtime/src/cad_client/threads.rs:341-343`, `POST
   /threads`), `add_comment` (289; `threads.rs:359-361`, `POST
   /threads/{id}/comments`), `update_thread` (287; `threads.rs:351-353`,
   `PATCH /threads/{id}`).
4. **RoboCAD**: `api.py:1611-1613` → `annotation_request` (`api.py:396`):
   `POST /threads` (415-424) → `create_thread`
   (`cad/robocad/annotations.py:233`); comments (434-436) → `add_comment`
   (`annotations.py:276-282`, one undo step "Reply to annotation"). The
   anchor (`anchor`, 91-103) keeps the node id, the point and a geometry
   stamp (`stamp`, 30-39). A joint has no body or mesh, so its stamp is
   None and `thread_detail` reports the pin "attached" while the node
   exists (106-117). A comment's `[label](part:ID)` links are added to the
   thread's linked parts (`thread_parts`, 124-138); the dock writes the
   same text form (`part_link`, `cad_client/threads.rs:315`).
5. **Answer**: the thread as RoboCAD answered it, its id at the job value's
   `result.id` (`thread_id`, script 326-331).
6. **Persistence**: the threads live in `doc.annotations`, written into the
   manifest on save (`document.py:507`) and restored on load (569). The
   save is RV-03 step 2, before the export. This is the path the CAD
   checklist traces in
   [Persistence: save, close, reopen](cad-checklist.md#persistence-save-close-reopen).

By reading, unexecuted.

### RV-05 Wire it in the systems editor

1. **Calls** (`wire_in_build`, script 523-536): the script writes an empty
   `sim.system/1` file DIR/rover.system.json with exclusive create
   (525-529), switches with `viewer_mode {"mode":"build","path"}` (530) and
   sends one `system` batch (534) from `system_wiring_commands` (497-515):
   `add_instance` rover (`robot.articulated`), controller
   (`control.external`) and limiter (`control.drive_limiter`); `link_file`
   each to `robot.simrobot.json`, `robot.controller.json` and
   `robot.drive.json`; `set_parameter controller sense.command.<axis>`
   (value 1) and `connect limiter twist.<axis> → controller
   sense.command.<axis>` for forward and yaw.
2. **The document**: `SystemDocument.links`
   (`crates/sim-system/src/document.rs:47`), `FileLink { path }` relative
   and `/`-separated (60), `HOSTED_TYPES` (68), `hosted` (518).
3. **`link_file`**: `Command::LinkFile`
   (`crates/sim-system/src/commands.rs:194-204`) → `apply_one` (604-625):
   `Resolver::check_link` (608; `crates/sim-system/src/resolve.rs:222-249`:
   a root instance, an element of a hosted type, a relative `/` path with
   that type's suffix). `null` unlinks, and is refused while the
   instance's ports are still connected (617-620). Removing a root
   instance drops its link (313); renaming it renames the link (333-337).
   Validating a document checks every link first (`resolve.rs:166-169`).
4. **`connect`** is checked as it applies (`commands.rs:393` →
   `Resolver::check_net`, `resolve.rs:340-347`): a net on a hosted
   instance joins only hosted instances. So `link_file` must come before
   `connect`.
5. **Flatten**: a linked root instance is not compiled. It is kept in
   `Flattened.hosted` with its parameters
   (`crates/sim-system/src/flatten.rs:169-176`), and the root nets that
   touch it in `hosted_nets` (338-340). Each is reported as a `hosted`
   finding (`resolve.rs:379-381`).
6. **No partial runs**: every host that would start a `SystemSession` from
   a compiled document calls `system_builder::check_hosted`
   (`crates/sim-runtime/src/system_builder.rs:85-93`) first:
   `simulate_cancellable` (207), the live bundle writer `write_bundle`
   (148-156: refused, and a stale `<stem>.live.json` removed), which the
   egui viewer's compile uses (`crates/sim-viewer/src/system_ui.rs:61-62`),
   Build's generic run thread (`builder/live_run.rs:299-301`), the browser
   (`crates/sim-web/src/lib.rs:493`) and lessons
   (`crates/sim-runtime/src/lesson.rs:308`).
7. **Tests** (written, never run): `crates/sim-runtime/tests/system_robot.rs`
   (links without parameters 49, rename and remove 73, refusals 91, the
   rover resolved 135, error naming 163, what the host does not run 178).

By reading, unexecuted.

### RV-07 Build live run and `system_drive`

1. **Run**: `system_run {"action":"start"}` (`live_run_in_build`, script
   552) → `Builder::start_run` (`builder/live_run.rs:64-81`). A document
   with links is a robot system (65). The same kind of run resumes
   (66-73); otherwise `LiveRun::spawn_robot` (78;
   `builder/robot_run.rs:56-64`), a "builder-run" `RunThread` with join
   bound zero (62), so dropping the run never waits on a controller that is
   still starting.
2. **Build on the run thread**: `robot_thread` (`robot_run.rs:369-479`) →
   `Worker::build` (275-304) → `sim_system::flatten` →
   `system_robot::resolve` (287; `crates/sim-runtime/src/system_robot.rs:66-148`):
   - nothing compiled beside the hosted instances (73-76);
   - exactly one robot and one controller, at most one limiter (86-96);
   - hosted parameters only the controller's `sense.command.<axis>`
     (97-110);
   - the hosted nets exactly `limiter.twist.<axis> →
     controller.sense.command.<axis>` (`check_wiring`, 153-172);
   - the model parsed (114-115) and the binding loaded
     (`controller_binding::load`, 116);
   - the controller's link must be the robot's own `<stem>.controller.json`
     (117-132), and a limiter's link the profile that binding names
     (133-145);
   - the scene (146).

   Then `RobotSystem::host` (`robot_run.rs:293`; `system_robot.rs:179-181`)
   → `DriveHost::new` (`crates/sim-runtime/src/drive_host.rs:278-282`) →
   `Session::new` (`crates/sim-runtime/src/session.rs:326`) →
   `spawn_external` (431-432 → 682-688; `sim_couple::python` at 684) and
   the seam attach with the command channels (446-450). `check_inputs`
   (`drive_host.rs:103-109`) confirms the four channels.
3. **Drive**: `system_drive {"forward":0.5}` (script 557, re-sent at 10 Hz
   by `drive_steadily`, 539-547) → `SystemAction::SystemDrive`
   (`builder/system_actions.rs:84-96`) → `DriveRequest::from_fields`
   (360-361; `drive_host.rs:58-71`) → `Builder::drive`
   (`robot_run.rs:71-74`) → the one drive apply `Builder::drive_request`
   (85-112). The run panel's Forward, Back, Left, Right and Stop buttons
   (`builder/ui.rs:211-217`) write `BuildAction::Drive` → `dispatch`
   (`builder/actions.rs:200-203`) → the same `Builder::drive`; the bound
   keys and gamepad reach `Builder::drive_request` directly (RV-41).
   `check_drive` (`robot_run.rs:142-178`) refuses a run that is not a robot
   system (144-146), anything but a stop or halt while a reset is in
   progress (151-160), and a failed or ended run (163-167); it interprets
   the request against the loaded profile (`DriveRequest::interpret`, 169;
   `drive_host.rs:79-98`), accepts only stop before the system has loaded
   (171-172), and a nonzero request only after Run (174-176). Accepted →
   `RunControl::Twist` (98); a refusal is kept as `drive_refusal` (88-96),
   and the panel is marked dirty only when the refusal text changes (89-92,
   104-106), so held keys and sticks rebuild nothing.
4. **Run thread**: every queued command, in order, before the next period
   (`robot_run.rs:396-457`). `Twist` (455) → `Worker::twist` (307-321) →
   `DriveHost::request` (320; `drive_host.rs:298-301`) →
   `TwistState::request` (141-159). Each period while running, never
   faster than real time (`robot_run.rs:461-468`) → `Worker::period`
   (324-342) → `DriveHost::step` (327; `drive_host.rs:310-317`):
   `TwistState::advance` (313; 165-175), then `Session::step` (314;
   `session.rs:489`, values stored 511, `robot.advance(period_s)` 513).
   This is the same host Robot mode steps (RV-19 step 6).
5. **Shown**: `system_state.live_run.drive` (`builder/live_run.rs:263,
   276`; `builder.rs:514`) = `Builder::drive_json` (`robot_run.rs:194-216`):
   the phase, the system (instances, files, limits with units, deadman,
   period, channels, wiring: `RobotSystem::json`,
   `system_robot.rs:205-228`), the status, the last request sent and
   refused, the last apply error and the run's error. The script reads it
   after its stop (559) and pauses (560).
6. **Failure**: a step error ends the run, named by the system's instances
   (`robot_run.rs:332-336`; `RobotSystem::name_error`,
   `system_robot.rs:187-200`: "`controller` (external controller (python)
   <script> on <element>): …") → `publish` (`robot_run.rs:344-366`; drive
   at 364) → `live_run.error` (`builder/live_run.rs:273`). The script fails
   the step on `live_run.error` (554-556).
7. **Decisions**:
   - Build's robot run hosts `DriveHost` (`Session` → `PhysicalRobot::advance`),
     not `SystemSession`. `SystemSession`'s `Runtime::advance` would skip
     `PhysicalRobot::advance`'s slice retry and battery sampling, a second
     stepping path for the same robot (module doc, `robot_run.rs:1-9`).
   - A hosted instance's parameters come from its linked file only
     (`system_robot.rs:97-110`).
   - A robot system holds only the linked robot, controller and limiter
     (73-76).
   - `link_file` must come before `connect` (step 4 of RV-05).
   - The keyboard and gamepad drive a Build robot run through the same
     one poller as Robot mode (rover-browser-drive): RV-41 to RV-43.
   - A Build robot run keeps no run record (`NO_RUN_RECORD`,
     `robot_run.rs:30`; `Save run` hidden, `builder/ui.rs:201-204`) and
     draws no robot or graphs (`RunControl::Observe` is ignored,
     `robot_run.rs:453-454`; the snapshot has no frame, 359).

By reading, unexecuted.

### RV-38 Browser driving

**Compatibility status.** The browser path is a compatibility surface. The
browser cannot start Python (`EmbeddedSession` refuses an external
controller by name, `crates/sim-runtime/src/embedded.rs:239-241`), so it runs
the binding's `embedded` Rhai adapter instead. The Python external
controller on the native seam stays the reference, and no parity run between
the two exists. Realtime is not measured, the physics is uncalibrated, and
nothing here has run in a browser. The page says so: the mode label
(`web/viewer/viewer.js:361`), the panel's scope line
(`web/viewer/drive-panel.mjs:29`), the fidelity text built in Rust
(`crates/sim-runtime/src/embedded_drive.rs:412-420`) and the preset's
readiness (`web/viewer/presets.json:539`).

The page reads devices and sends requests only. Rust maps the devices
through the shared bindings, interprets the request against the profile,
limits it, runs the deadman on simulation time and mixes it
(`web/viewer/drive-input.mjs:1-9`).

**(a) A held key to the wheel targets.**

1. **Control**: the `rover-drive` preset is loaded, Play is pressed, and W
   is held.
2. `keydown` (`web/viewer/viewer.js:538-542`, drive presets only): the
   default is prevented for a bound key outside a text field or chord
   (540), then `drive.input.keyDown` (`drive-input.mjs:136-143`) adds the
   physical `KeyboardEvent.code` to the `held` set (141). A fresh keydown
   re-arms a key a stop disarmed (140); an auto-repeat keydown of a key
   whose press was never seen (held before focus or load) is held but
   disarmed (139), so it never drives. A `keyup` removes it
   (`viewer.js:543`; `drive-input.mjs:144`).
3. Each animation frame (`renderer.setAnimationLoop`, `viewer.js:477-478`)
   → `driveTick` (387-398): while playing, not replaying and with no replay
   pending (391), it polls (394) → `poll` (`drive-input.mjs:159-190`) →
   `evaluate` (126) → the
   wasm export `drive_device_axes` (`crates/sim-web/src/lib.rs:240-245`) →
   `drive_bindings::browser_axes`
   (`crates/sim-runtime/src/drive_bindings.rs:543-605`) →
   `Resolved::keyboard_axes` (550; 396-405, the function the native viewer
   uses) and `supported_only` per device (591-592; 444-456), so Q on the
   rover is zeroed and listed in `ignored`.
4. Send cadence (`drive-input.mjs:178-188`): nonzero axes are sent when they
   change and at least every `AXES_RESEND_MS` = 50 ms of wall time (14,
   181), as `{axes}` (182). This is a resend cadence, not a timer that
   stops anything.
5. `sendDrive` (`viewer.js:369-375`; nothing is sent while replaying or
   while a replay is pending, 370) → the worker's `drive_request`
   (`web/worker.js:178-182`) → `DriveSimulation::request`
   (`crates/sim-web/src/lib.rs:307-311`) → `DriveSession::request`
   (`embedded_drive.rs:503-512`), which refuses while replaying (504-506)
   and, through `check_running` (507; 583-591), once the session has
   latched a failure or reached its horizon, naming which →
   `DriveRequest::interpret_with` (508; `crates/sim-runtime/src/drive_host.rs:85-98`,
   `kinematics::scale` at 88) → `TwistState::request` (510;
   `drive_host.rs:141-159`, heartbeat + 1 at 149, stamped at the session's
   simulation time).
6. Live work chunks: `advanceLive` (`viewer.js:457-470`) asks the worker for
   `drive_advance` (460; `worker.js:189-194`) with
   `drivePeriodsPerChunk` periods (`drive-input.mjs:81-84`, 0.05 s of
   simulated time) → `DriveSimulation::advance` (`lib.rs:322-333`) →
   `DriveSession::advance` (`embedded_drive.rs:531-581`). Each live period
   (550-558): `TwistState::advance` (554; `drive_host.rs:165-175`, which is
   `kinematics::step` on simulation time at 167) on a copy →
   `EmbeddedSession::set_inputs` (`embedded_drive.rs:555`;
   `embedded.rs:840-868`): the four values `[forward, lateral, yaw,
   heartbeat]` (`drive_host.rs:172`) are validated and, when they changed,
   kept as an input event at the committed step (848-860), so they are
   recorded → `EmbeddedSession::advance` for one period
   (`embedded_drive.rs:556`); the copy is committed after it (557).
7. In the session, the `SampledPolicy` is sampled each control period
   (`embedded.rs:1072-1073`): the held inputs are appended to the sensors
   (`embedded_policy.rs:433`) and the Rhai program is sampled (438-440).
8. The adapter (`examples/wheeled-robot/drive-adapter.rhai`) reads
   `command.forward`, `command.lateral`, `command.yaw` (76) and
   `command.heartbeat` (77) → `drive_update` (77; registered at
   `crates/sim-script/src/drive.rs:169-171` → `update`, 130-145 →
   `HeartbeatDeadman::update`,
   `crates/sim-domain-control/src/drive/kinematics.rs:424-450`) →
   `drive_differential_mix` (`drive-adapter.rhai:87`; registered at
   `drive.rs:149-153` → `DifferentialDrive::mix`, `kinematics.rs:317`) →
   `targets[i] += period * rates[i]` and `commands[<joint>.target]`
   (`drive-adapter.rhai:91-94`). The functions are on every controller
   engine (`crates/sim-script/src/lib.rs:249`). The script has no literal
   geometry: track, radius and signs come from `parameters().drive`
   (44-48).
9. The policy's targets (`embedded_policy.rs:520`) drive the CAD servo
   firmware (`embedded.rs:1167`; servos and target coordinates from
   `examples/wheeled-robot/drive-adapter.config.json:11-15`). The frame comes
   back with the drive status (`DriveSession::frame`,
   `embedded_drive.rs:633-639`) and is drawn (`viewer.js:462`).

**(b) A gamepad stick.** `driveTick` reads `navigator.getGamepads()` through
`gamepadSnapshot` (`viewer.js:393`; `drive-input.mjs:71-78`: mapping, id,
axes, button values and pressed; a non-finite axis or button value is sent
as 0, never NaN, 68-69) → `browser_axes` reads only `standard`
pads (`drive_bindings.rs:559-563`, others named in `ignored_pads`), takes
each bound stick from its W3C source and converts its sign (`STICKS`,
85-92, applied at 564-573: Standard Gamepad Y is +1 down), then the shared
`Resolved::gamepad_axes` (577; 408-420) with `shape`'s deadzone (433-438).
A pad starts disarmed and is ignored until Rust reports zero pad axes and
no held button action (`drive-input.mjs:123-124, 163-167`). From there it
is the same as (a) steps 4-9.

**(c) Stops.** Every stop is a request; the page decides no motion.

- Window blur (`viewer.js:544`) and the page hidden (`visibilitychange`,
  545) → `input.stop()` (`drive-input.mjs:145`): `"stop"`, then every held
  input is disarmed until released.
- Escape (`drive-input.mjs:137`): stop, ignored only while a text-entry
  element has focus (`isTextEntry`, 58-63: a text-like `<input>`, a
  textarea or contenteditable; passed as `textEntry`, `viewer.js:539,
  541`). With a select, checkbox, slider or button focused, Escape still
  stops.
- Release of every input: one zero request (`drive-input.mjs:185-188`).
- A bound `stop` or `halt` key or button (X, B, South, East): on its rising
  edge a zero request first if axes were being sent, then the action, then
  disarm (`drive-input.mjs:170-177`). The panel's buttons (one per profile
  action, `drive-panel.mjs:35-41`, `onAction` at 39) go through the same
  state machine (`viewer.js:355-357`): `stop` → `input.stop()`, any other
  action → `input.action(name)` (`drive-input.mjs:146-150`: a zero request
  first if axes were being sent, then the action, then disarm).
  `interpret_with` maps them as RV-23/RV-24 (`drive_host.rs:92-96`).
- A text field taking focus (`viewer.js:546`, and while the leaderboard
  dialog is open, 390 and 539) → `textFocus` (`drive-input.mjs:151-157`):
  held keys are disarmed, and one stop is sent if the keyboard was
  driving. A keydown in a text field or a chord is held and disarmed
  (139), and a chord also disarms the keys already held (138).
- Pause sends stop (`setPlaying`, `viewer.js:105`), then `drive_pause`
  (see "Safety closure" (b)).
- The deadman: when requests stop arriving, nothing in JavaScript times
  out. Simulation time advances only in `drive_advance`, and
  `TwistState::advance` computes the request's age on that time
  (`drive_host.rs:166`) → `kinematics::step` → `deadman_expired`
  (`kinematics.rs:256-258`) → the profile's stop rule. The adapter's own
  `HeartbeatDeadman` (`kinematics.rs:439-441`) stops it again if the
  heartbeat stops rising.

**(d) Bindings.** `loadDriveBindings` (`drive-input.mjs:35-46`, called at
`viewer.js:352-353`) uses the defaults from `default_drive_bindings`
(`crates/sim-web/src/lib.rs:216-218` → `drive_bindings::json(None)`) unless
`localStorage["sim.drive-bindings/1"]` holds an override. The override is
checked by `validate_drive_bindings` (224-227 → `BindingsFile::from_value`,
`drive_bindings.rs:284-292`, the parser the native viewer uses). A refusal
falls back to the defaults and is shown verbatim in a notice
(`drive-input.mjs:43-45`; `drive-panel.mjs:65`), with the bindings
table (66-68).

**(e) Recording and replay.** Save run (`viewer.js:503-510`) asks the
worker for `drive_recording` (`worker.js:202-206`) → `DriveSimulation::recording`
(`lib.rs:340-342`) → `DriveSession::recording` (`embedded_drive.rs:627-629`)
→ `EmbeddedSession::recording` (`embedded.rs:1460-1477`): scene (with the
identity parameter), config, seed, committed steps and the input events,
each `[f, l, y, heartbeat]`. It is saved as Rust's text unchanged
(`viewer.js:508`). Replay (`viewer.js:511`, file chooser 399-401) →
`replayDrive` (403-416), which sets `replayPending` before the identity
check and clears it only in its `finally` (407, 415; the flag's rule,
39-41), so `sendDrive` (370) and `driveTick` (391) send no device input
from the moment the replay is asked for, even if a live chunk resolves
meanwhile. The page then asks the worker for `drive_replay`
(`worker.js:207-223`) with `chunk_periods` = `DRIVE_REPLAY_CHUNK_PERIODS`
= 200 (`viewer.js:44`, passed at 411; checked 1..1000 at
`worker.js:210-211`). The worker calls `DriveSimulation::prepare_replay`
(212; `lib.rs:345-352`), then advances 200 periods per call with progress
after each (214-221). `prepare_replay` →
`DriveSession::prepare_replay` (`embedded_drive.rs:677-713`): the recorded
`EmbeddedIdentity` is compared (`EmbeddedIdentity::differences`, 78-93:
the entry and the script, config, profile, model and CAD hashes; the
binding and profile paths are recorded, not compared, because they differ
between hosts and URL layouts for the same bytes, 73-77) and a difference
is refused naming each field (685-688). Then the scene, with those paths
set aside (690-698), and the config must match (699-703), and
`EmbeddedSession::prepare_replay` (`embedded.rs:1481-1520`) re-checks the
events. A refusal is shown verbatim as "Replay refused: …"
(`viewer.js:414`). The replay steps the recorded inputs
(`embedded_drive.rs:559-577`, `TwistState::replayed`), live requests are
refused meanwhile (504-506), and the end leaves nothing driving
(`end_replay`, 595-599).

**(f) The preset WASD path is unchanged.** Presets with motion commands
keep their own WASD listeners (`viewer.js:522-535`) and
`motion-commands.mjs` (`motionCommandConfig` 8-19, `nextMotionAction`
31-35, `driveMotionValues` 39-52). A drive preset has no policy inputs on
the page (`makeInputs([])`, `viewer.js:351`) and no `motion_commands`
(`presets.json:527-541`), so `motionCommandConfig` returns null for it
(`motion-commands.mjs:11-12`). The drive listeners act only when a drive
preset is loaded (`viewer.js:537-546`).

**(g) The scene.** `loadDrive` (`viewer.js:333-366`) fetches the packaged
model and binding text (`packageDrive`, `web/build-viewer.mjs:44-67`), asks
Rust which files the binding names (`drive_files` →
`embedded_drive::files_to_read`, `embedded_drive.rs:143-149`: the profile,
entry, further files and config), fetches them and asks the worker to build
(`drive_build` → `build_drive_scene`, `crates/sim-web/src/lib.rs:262-274`)
→ `embedded_drive::build` (`embedded_drive.rs:222-425`):

- the profile resolved against the model by `resolve_drive` (239;
  `controller_binding.rs:273-277`), geometry derived from CAD with its
  provenance (RV-13) and limits checked against the motors (RV-14);
- the adapter's sources captured (242-267) and the config's step checked
  against the model's control period (270-284), the CAD hash checked
  (285-296), and the motor joints, servos and target bounds matched to the
  drive's wheels (300-331);
- the policy's target envelope checked against a whole session at full
  speed (332-349): each wheel's `target_bounds_rad` must cover the initial
  target ± `max_wheel_rate` × the session duration, where `max_wheel_rate`
  (172-195) is the largest wheel joint rate the drive's own mixer gives at
  every corner of the profile's speed envelope; a narrower envelope is
  refused naming the bound and the travel. For the rover (design numbers,
  not run): full forward plus full yaw gives (0.26 + 0.06 × 4.3) / 0.03 ≈
  17.3 rad/s on the outer wheel, × 600 s ≈ 10 400 rad from a 0 rad start,
  inside the config's ±12000 rad
  (`examples/wheeled-robot/drive-adapter.config.json:19`);
- each servo's `supply_voltage_v` and `winding_temperature_k` checked
  against the model (350-386): the motor's `electrical.supply_voltage`,
  and its `thermal.ambient_c` (else `world.ambient_c`) + 273.15; a
  disagreement is refused naming both values, and a value the model does
  not state is kept and named in the fidelity label as an imposed boundary
  (387-391). The rover's config states 6 V and 298.15 K (`drive-adapter.config.json:12-13`);
- the command inputs from `drive_inputs` (409; `controller_binding.rs:166-184`):
  twist bounds ± the profile's max speed and the heartbeat channel
  0..2^53 (176-182);
- the `EmbeddedIdentity` (394-403) and the resolved drive as scene
  parameters (404-408), the fidelity label (412-420), and the scene through
  `scene_with` (423; `controller_binding.rs:372-375`).

`drive_load` (`worker.js:169-177`) takes only the drive JSON text that
`build_drive_scene` returned and refuses a parsed object (171: re-serializing
it would turn 0.0 into 0, which `DriveSession::new` refuses) →
`DriveSimulation::new` (`lib.rs:298-304`) → `DriveSession::new`
(`embedded_drive.rs:482-500`), which checks the schema, that the scene's
parameters carry this drive and identity (486-493), and the four command
inputs (`check_inputs`, 497). The panel (`createDrivePanel`,
`drive-panel.mjs:25-94`) shows the identity, limits with units, deadman,
bindings and the live status from Rust.

By reading, unexecuted.

### RV-40 The committed example script

`clients/python/examples/build_rover_over_rest.py` (stdlib only; documented
in `clients/python/README.md`, "Example: build and drive a rover over the
viewer's REST API"). It has never run, and its log records that
(`SCRIPT_STATUS`, 69-70; written into `rest_log.json`, 671).

1. **Launch**: the viewer in CAD mode on the seed (RV-01), then
   `python3 clients/python/examples/build_rover_over_rest.py --out
   /tmp/rover-<date>` (docstring 11-15; `--port` default 8421, 617).
2. **Transport**: every command is `POST /v1/batch`, then its job is
   polled at `GET /v1/jobs/{id}` until it succeeds, fails or is cancelled
   (`Rest.batch`, 116-144; `crates/sim-api/src/lib.rs:431`, 414-430).
   Before anything else, `GET /v1/capabilities` must list every command the
   script uses (645-654).
3. **Steps** (`main`, 615-691), each request tagged with its RV id in
   `rest_log.json`:

   | Script step | REST commands | Trace |
   |---|---|---|
   | `open_new_document` (239-249) | `viewer_mode`, `cad_state`, `cad_file new` | [RV-01](#rv-01-create-the-rover-in-cad-through-rest) |
   | `build_fixture` (267-310) | `cad_op` ×25 | [RV-01](#rv-01-create-the-rover-in-cad-through-rest), [RV-02](#rv-02-rig-its-joints), [RV-03](#rv-03-parts-save-export-and-the-hash) |
   | `annotate` (342-367) | `cad_threads` list + create/reply/link | [RV-04](#rv-04-annotate-it-threads-pinned-to-its-nodes) |
   | `save_and_export` (370-391) | `cad_save`, `cad_results export`, `cad_state` | [RV-03](#rv-03-parts-save-export-and-the-hash) |
   | `write_profile_and_binding` (427-492) | none (local files) | [RV-06](#rv-06-controller-program-on-the-seam), [RV-03](#rv-03-parts-save-export-and-the-hash) step 8 |
   | `wire_in_build` (523-536) | `viewer_mode build`, `system`, `system_state` | [RV-05](#rv-05-wire-it-in-the-systems-editor) |
   | `live_run_in_build` (550-561) | `system_run`, `system_drive`, `system_state` | [RV-07](#rv-07-build-live-run-and-system_drive) |
   | `drive_in_robot_mode` (587-610) | `viewer_mode robot`, `robot_run`, `robot_drive`, `robot_state`, `robot_save_recording` | [RV-17](#rv-17-robot-mode-opens-a-model-with-its-binding), [RV-18](#rv-18-run-builds-the-session-and-starts-python), [RV-29](#rv-29-rest-robot_drive), [RV-32](#rv-32-robot_state-drive-fields), [RV-33](#rv-33-save-a-drive-recording) |

4. **Safety**:
   - `--out` must not exist (623-625) and must not lie under the
     repository's `examples/`, `cad/` or `web/` (626-631; the viewer's
     `PROTECTED`, `crates/sim-spatial/src/robot/recording.rs:32`). The
     seed must be a file (632-635). The directory is created with
     `exist_ok=False` (636), and every file the script writes is opened
     with exclusive create (`"x"`: 488, 527, 675).
   - Every call has a deadline (`command`, 156-158; `poll`, 160-180). A
     job that misses it is cancelled with `DELETE /v1/jobs/{id}`
     (133-135; `cancel_job`, 146-154; the server sets `cancel_requested`,
     `crates/sim-api/src/lib.rs:419-421`) and never resubmitted.
   - On any failure or exception (665-670) the script sends a best-effort
     stop in the active mode only (`best_effort_stop`, 182-197: Robot
     mode `robot_drive stop` then `robot_run pause`; Build mode
     `system_drive stop` then `system_run pause`), writes the log and
     exits 1 naming the failing step (677-681).
   - No hardware commands: the script talks only to the viewer on
     loopback, without a proxy (72, 85), and uses only the commands in its
     capability list (647-649): CAD edits, file export, Build and Robot
     simulation (docstring 7-9).
5. **Expected result** (design, not run): `robot.rcad`,
   `robot.simrobot.json` whose `source.cad_sha256` is the `.rcad`'s
   sha256, `robot.drive.json`, `robot.controller.json`,
   `rover.system.json`, a drive recording and `rest_log.json` in DIR
   (summary print, 682-690).

By reading, unexecuted.

### RV-41 Build mode: keyboard

1. **Control**: the rover system is wired and running in Build mode (RV-05,
   RV-07: Run pressed, the system loaded); hold W.
2. **Target**: `robot_run::drive_target`
   (`crates/sim-spatial/src/builder/robot_run.rs:222-225`, registered in
   `ViewerSet::Input` before `InputSet::Window` while in Build mode,
   `crates/sim-spatial/src/builder.rs:552`) writes `Builder::drive_target`
   (`robot_run.rs:125-139`) with `set_if_neq`: no target while a text draft
   is open or a placement drag runs (126-128; the drag's X/Y/Z axis keys
   would also be drive keys, X the default stop); otherwise live only for
   a robot system's run whose profile has loaded (129-131), with the linked
   binding's supported axes (135), a run identity from the system file and
   the run id (132, 136; a rebuilt run is a new target, which disarms held
   inputs, `crates/sim-spatial/src/drive_input/input.rs:236-245`), and
   Build's own keys as owned keys (138).
3. **Poller**: the same one poller as Robot mode
   (`drive_input::input::devices`, `input.rs:177-351`; RV-08 steps 3-6),
   reading W through the shared bindings. Build's own keys
   (`OWNED_KEYS`: arrows, N, G, U, R, `builder/actions.rs:619-623`) are
   never read for driving (`input.rs:246-250`). It writes
   `Act::quiet(DriveDevice { mode: Build, Axes })` every frame
   (339-343).
4. **Drain**: `actions::drive_devices` (`builder/actions.rs:659-672`,
   registered in `ViewerSet::Actions` after the builder's one apply,
   `builder.rs:552`) → `device_action` (628-630: only Build-mode
   requests, as `BuildAction::Drive` with the poller's origin) →
   `apply_device` (638-647) → `Builder::drive_request`
   (`robot_run.rs:85-112`), the one apply the run panel's buttons and REST
   `system_drive` use (RV-07 step 3). A quiet request's refusal is kept as
   the run's `drive_refusal` only; a shown one's is also the status line
   (`builder/actions.rs:643-645`). `DriveInput::last_error` mirrors the
   refusal (666-671).
5. `check_drive` (`robot_run.rs:142-178`) → `interpret_with`
   (`drive_host.rs:85-98`; `kinematics::scale` at 88) → `RunControl::Twist`
   (`robot_run.rs:98`) → run thread `Worker::twist` (dispatched at 455;
   307-321) →
   `DriveHost::request` (320; `drive_host.rs:298-301`, heartbeat + 1 at
   149) → each period `Worker::period` → `DriveHost::step` (327;
   `drive_host.rs:310-317`) → `TwistState::advance` (313) → `Session::step`
   with `[f, l, y, heartbeat]` on `COMMAND_CHANNELS`
   (`controller_binding.rs:27`) → the Python controller (RV-19 steps 7-9).
6. A nonzero request before Run, or while paused, is refused
   (`robot_run.rs:174-176`) and shown in the drive strip (RV-43). A held
   key repeats that refusal every frame, but the panel is marked dirty
   only when the refusal text changes (`robot_run.rs:88-94`; an accepted
   request after a refusal, 104-106), so held keys and sticks rebuild
   nothing; the strip's twist follows the 4 Hz live-run refresh.

By reading, unexecuted.

### RV-42 Build mode: gamepad

1. **Control**: the same running system; push the left stick forward, or
   press South (`stop`) or East (`halt`).
2. The poller reads every `Gamepad` (`input.rs:299-301`), drives from them
   and their buttons only while a window of this app has focus (295,
   302-307), through the shared `Resolved::gamepad_axes` and deadzone
   (RV-09 step 3). A pad held at a stop, or when the target became live,
   stays blocked until focused, neutral and released (308-316).
3. Axes: as RV-41 steps 3-5. Buttons: a zero request first while driving,
   then `DriveDevice { Action { name } }`, then disarm (`input.rs:317-326`)
   → `drive_devices` → `Builder::drive_request` → `interpret_with`: `stop`
   → `(ZERO, false)`, `halt` → `(ZERO, true)` (`drive_host.rs:92-95`). A
   stop or halt is accepted even during a reset
   (`robot_run.rs:151-160`), and a halt zeroes the request and the
   commanded twist at once (`drive_host.rs:152-155`).

By reading, unexecuted.

### RV-43 Build mode: stops and the drive strip

1. **Release**: one zero request (`input.rs:344-348`), then the run thread
   ramps to zero under `max_accel` (RV-20 step 3).
2. **Focus loss**: a stop when the devices were driving (`input.rs:192,
   254-255`, sent at 263-266), then disarm (270).
3. **Escape**: a stop (`input.rs:256-257`), not while a text field has the
   keyboard or in a chord. Build's own Escape (back to Select,
   `builder/actions.rs:721-722`) still runs.
4. **Text focus**: a kit field taking the keyboard while keys drive sends
   one stop (`input.rs:195-196, 258-259`); while typing no key is read
   (292), and Build's own keys do not run (`builder.rs:547`).
5. **Bound actions**: X/B and South/East (RV-42 step 3).
6. **Deadman**: if requests stop, the run thread's `TwistState::advance`
   expires the request on simulation time (`drive_host.rs:166-167`), and the
   Python controller's `DriveState` is the second guard (RV-25 steps 2-3).
7. **The target goes away or changes while the devices drive** (Reset or
   Stop of the run, a draft field or a placement drag opened, a run
   rebuilt from an edited file): the poller owes the zero it would have
   sent on release. `owed_stop` (`input.rs:199-213`) writes one quiet
   `Stop` stamped with the previous target's mode, only while that mode is
   still current; it runs when no target is live (214-221) and when the
   target's identity changed (236-245), and `DriveInput::last_action`
   names why.
8. **Leaving Build mode**: `actions::leave_drive`, registered on
   `OnExit(ViewerMode::Build)` (`builder.rs:553`;
   `builder/actions.rs:674-688`), first sends the owed stop through the one
   drive apply when the devices were driving (`DriveInput::axes` nonzero,
   682-686): the builder and its run are kept, paused, and a pause does not
   zero the requested twist, so the zero would otherwise never come (a
   pause now also invalidates a live request; see "Safety closure" (b)). It
   then calls `drive_input::leave_mode` as a function (687;
   `crates/sim-spatial/src/drive_input/mod.rs:140-143`), which clears the
   target and the status; the drain and the target writer run only in
   Build mode (`builder.rs:552`).
9. **Shown**: the drive strip under the viewport (`drive_strip`,
   `builder/ui.rs:401-414`, called at 141) while a robot system's run is
   live (`drive_strip_shown`, 349-351). Its height is part of
   `SpatialScene::builder_dock` (`builder/graphs.rs:128-132`), so the camera
   viewport and picks stop above it. `drive_lines` (359-396): the bound
   keys generated from the bindings (`DriveBindings::key_summary`,
   `crates/sim-spatial/src/drive_input/bindings.rs:162-187`: an axis the
   profile lacks marked "(not in profile)", Build's own keys marked "(mode
   key, ignored)", then the action keys and "Esc stop"; ui.rs:363-367),
   then the device input (normalized), the requested and commanded twist
   with units (368-385), the ignored axes and the last refusal (386-394),
   rounded to fixed decimals. The run panel's Forward, Back, Left, Right
   and Stop buttons stay in the toolbar (`builder/ui.rs:211-217`). REST
   `system_drive`'s description names the bound keys and stops
   (`builder/system_actions.rs:239`).

By reading, unexecuted.

### Recorded gaps (rover-rest-flow)

- **`benchmark_assumptions` is not in the REST-built export.** Robot mode
  shows `source.benchmark_assumptions` (`robot/sections.rs:178-183`), but
  RoboCAD's `physical.py` does not copy the `benchmark_assumptions`
  setting into the source block (`cad/robocad/physical.py:957-960`), and no
  GET route returns that setting (`GET /battery`, `/control` and
  `/uncertainty` return only their own settings, `api.py:1802-1813`).
  `POST /ops/set_robot_setting` does answer the whole `doc.robot_settings`
  (`commands.py:1110`), but only to the script; the export job never sees
  it. So the native export can't fill it in without a RoboCAD change. The
  fixture script copies it by hand
  (`cad/scripts/wheeled_learning_fixture.py:64, 69`), and the REST-built
  model reads "none recorded". This is a RoboCAD gap (`physical.py` should
  add it). It is not fixed here.
- **CAD mode needs an existing `.rcad` to start** (RV-01): the seed is a
  prerequisite of the script.
- **No keyboard or gamepad in Build mode**: closed by rover-browser-drive
  (RV-41 to RV-43).
- **A Build robot run keeps no run record and draws no robot or graphs**
  (RV-07 step 7). Robot mode records the drive session.
- **A `.rcad` changed on disk outside RoboCAD is not detected.** RoboCAD's
  `dirty` and `revision` track only its in-memory document, so such a file
  would be hashed and stamped (`cad/results/export.rs:24-29`).

### Safety closure (focus-safety-closure, 2026-10-03)

By reading, unexecuted: nothing below was compiled, run or tested. Line
numbers are from the working tree of this epic's commit.

**(a) A native Stop, halt, action, Pause or Reset disarms held inputs.**
One public path, the Message `drive_input::Disarm`
(`crates/sim-spatial/src/drive_input/mod.rs:81-107`, rule `DISARM_RULE` :79,
registered `plugin.rs:23`).
1. Robot: the Drive block's Stop button (`robot/controls.rs:715`; one button
   per profile action, 717) writes `Act<RobotAction>` in `InputSet::Window`;
   REST `robot_drive {"stop": true}` and `system_ui drive:*` build the same
   `RobotAction::Drive`. Robot mode's one apply
   (`robot/actions/mod.rs` `apply`, in `ViewerSet::Actions`) resolves the
   reason before the action runs (`disarm_reason`, 650 and 759) and, when
   the action is accepted, writes `Disarm { mode: Robot }` (705-709). Run
   Pause and Reset are covered by the same `disarm_reason`.
2. Build: the run panel's Stop (`builder/ui.rs:217`) and REST
   `system_drive {"stop": true}` (`builder/system_actions.rs:360-362`) both
   reach `Builder::drive` inside `builder::system_actions::apply`, which
   resolves `disarm_reason` (417, called at 506) and, when accepted, writes
   `Disarm { mode: Build }` (510-516). A dispatched click is accepted only
   when `action_error` stayed empty (513). Pause and Reset likewise.
3. The poller (`drive_input/input.rs` `devices`, `InputSet::Window`, no
   `run_if`) drains every `Disarm` each frame before any early return
   (192). For one stamped with the live target's mode (273-286): if the
   devices were sending, one quiet `Stop` is sent first (so a held key's
   axes applied after the click in the same frame do not leave the robot
   driving), then the existing `disarm` blocks every held bound key and the
   pad until released, and `DriveInput::last_action` reads
   "stop (<reason>)" or "disarmed (<reason>)". A message for another mode
   is ignored. Ordering: writers in `ViewerSet::Actions`, the reader in
   the next frame's `InputSet::Window`; no edge to another feature's
   systems. Tests (unexecuted): `drive_input/tests.rs`.
   Known: after a device stop Robot's apply echoes a `Disarm`, so a bound
   key first pressed in exactly the next frame is ignored until pressed
   again (harmless).

**(b) A request live at Pause does not drive after resume.**
`sim_runtime::drive_host::PAUSE_RULE` (`crates/sim-runtime/src/drive_host.rs:34`):
`TwistState::pause` (222) shares `invalidate` (204-208) with
`replay_ended` (200): the request becomes zero, the deadman counts as
expired (last request stamped 2 × timeout_s back), the commanded twist is
kept. The next `advance` therefore applies the profile's on-loss rule
(ramp at stop_decel, or zero at once) until a fresh `request` resets the
age. Callers:
- Robot: `Command::Pause` (`robot/run/worker.rs:351-363`) →
  `Sim::pause_drive` (`robot/run/sim.rs:119-123`) → `DriveHost::pause`
  (`drive_host.rs:329`); not during a replay, whose end invalidates.
- Build: every pause (`builder/live_run.rs:103` `pause_run`) →
  `RunControl::Pause` (`builder/robot_run.rs:419-427`) → `host.pause()`.
- Browser: `setPlaying(false)` (`web/viewer/viewer.js:105`) →
  `pauseDrive` (379) → worker `drive_pause` (`web/worker.js:184`) →
  `DriveSimulation::pause` (`crates/sim-web/src/lib.rs:315`) →
  `DriveSession::pause` (`embedded_drive.rs:518-524`; a no-op while
  replaying).
Pause also disarms held device inputs (a), so a still-held key does not
count as fresh input. Tests (unexecuted): `drive_host/tests.rs`
`pause_invalidates_a_live_request_and_resume_applies_the_on_loss_rule`,
`a_fresh_request_after_pause_drives_again`, `pause_with_no_request_is_harmless`;
`embedded_drive/tests.rs`
`pause_invalidates_a_live_request_and_is_ignored_while_replaying`.

**(c) One deadman bound.** `kinematics::deadman_bound`
(`crates/sim-domain-control/src/drive/kinematics.rs:124-131`; error type
`DeadmanBound` 100): a finite, positive period and a finite timeout strictly
longer than it. Called by `drive_geometry::check_deadman`
(`crates/sim-domain-robot/src/drive_geometry.rs:252-260`, call 255) and by
the `control.drive_limiter` element (`drive/profile.rs:800`), each naming
its own path (`deadman.timeout_s`/`control.period_s`; `period`/`deadman_timeout`).

**(d) One stated motor ambient.** The parser keeps
`motors[i].thermal.ambient_c` (`MotorThermal::ambient_c: Option<f64>`,
`crates/sim-domain-robot/src/model.rs:902`), the motor's datasheet rating
ambient (`cad/PHYSICAL_MODEL.md` "Motor": the temperature its resistance and
torque are stated at), and records whether `world.ambient_c` was stated
(`WorldDocument`, 97-126). `PhysicalModel::motor_ambient(i)` returns the
motor's value, else the world's, with provenance
(`MotorAmbient::provenance`, 206-214: `motors[i] (name).thermal.ambient_c`,
`world.ambient_c`, or `world.ambient_c (not stated in the model; the
parser's default 20 °C)`). It is the motor unit's resistance and derating
`reference` (`motor.rs:117`, used at 233-234) in the embedded session
(`crates/sim-runtime/src/embedded.rs:481-490`) and in the native
`PhysicalRobot` (`physical.rs:341-347`), and the value the embedded drive's
build checks each servo's imposed `winding_temperature_k` against
(`embedded_drive.rs:370-385`; the JSON re-read is gone, and a defaulted
world ambient is listed as imposed, with its provenance). The thermal
network's environment (winding and case start, the case-to-air path, the
environment node) stays `world.ambient_c` (`physical.rs:231-237`). An older
model without the per-motor field reads exactly as before. The rover states
25 °C on each motor and no world ambient, so its motors' reference moves
from the parser's 20 °C default to 25 °C, matching the browser config's
298.15 K servo windings; the same holds for the legged models that state
25 °C per motor. Resistance and torque change slightly: any gait or rover
qualification needs a rerun (`examples/wheeled-robot/README.md`, the
gait-lab README).

**(e) Build `system_state` reports the device layer.** One serializer,
`drive_input::insert_state` (`drive_input/mod.rs:117`): Robot's
`robot_state` (`robot/state.rs:139`) and Build's answers carrying
`live_run` (`builder/system_actions.rs:518-522` → `Builder::with_drive_input`,
`builder/robot_run.rs:185`) both add `bindings` and `drive_input`.

**(f) Drive replay cancel.** Cancel replay (`robot/controls.rs:217`),
`system_ui replay:cancel` (`robot/actions/mod.rs:450`) and REST
`robot_replay {"cancel": true}` (`robot/actions/commands.rs:89`) →
`RunController::cancel_replay` (`robot/run/preset_ops.rs:327`) → the run
thread, a `crate::jobs::RunThread` (`robot/run/controller.rs:21`), gets
`Command::CancelReplay` (`worker.rs:237-260`): phase `Cancelled`, verdict
"cancelled at n/N seam periods, sim time t s of T s: not a verdict",
then `end_drive_replay` invalidates the recorded request (no live
request). Build mode has no drive replay; its run-history replay cancels
through `jobs::Job` (`builder/studies.rs:44, 57`).

**(g) CI.** `node web/tests/drive_input.mjs` is listed in
`.github/workflows/browser.yml` (not yet run).

### RV-39 Hardware twist path (run sheet)

The agent never drives hardware. No rover hardware client exists. The
intended path is the same `DriveState` with `limit_live=True`
(`drive.py:484, 504-505`; module notes 11-13), so a host sending raw twists gets
the shared limiter and deadman.

**Run sheet (operator only, for when a rover hardware client exists):**

- **Do**: the rover sits on a stand with its wheels off the ground, the
  operator is present, and the supply has a physical cut-off within reach.
  Start the controller against the hardware host. Press W briefly, then
  release.
- **Expect**: both wheels turn forward at no more than 0.26 m/s equivalent
  and ramp at 0.5 m/s². On release they ramp to rest. With no input for
  0.5 s they stop by the deadman.
- **Stop**: release, Escape, X or B. If anything else happens, cut motor
  power at the supply first. The hardware safety layer (watchdog, STOP) must
  hold regardless of host code (AGENTS.md).

## Known limits

- **The physics is uncalibrated.** Friction, traction and the motor model
  are library estimates (`examples/wheeled-robot/README.md:9-14`;
  `DRIVE_FIDELITY`, `run/controlled.rs:30`).
- **The limits are estimated from library motor data.** The N20
  `max_output_speed` is a library estimate. `robot.drive.json` labels every
  axis `estimated`, and the motor check (RV-14) is only as good as that
  data. Full forward plus full yaw asks the outer wheel for more than
  `v_free`, because the limiter checks each axis alone
  (`drive_geometry.rs:311-316`).
- **Rhai controllers share the slice-retry re-sample exposure.**
  `PhysicalRobot::advance` restores a snapshot and re-runs a failed slice
  (`physical.rs:671-689`), so the seam can sample the same time again. The
  Python rover rolls back its state (`diff_drive_rover.py:99-106`), and so
  does the browser's `drive-adapter.rhai` (its notes 31-36, rollback 61-74);
  other Rhai controllers such as `velocity-controller.rhai` don't, and can
  integrate twice.
- **The deadman can expire between frames.** Below fps = scale / timeout_s
  (2 fps at ×1, 16 fps at ×8 with 0.5 s), a held input is refreshed less
  often than the deadman, and the robot stutters (`controlled.rs:31`).
- **The browser runs a different controller program** (RV-38). It cannot
  host Python, so it runs the binding's embedded Rhai adapter through the
  same Rust drive functions. No run compares it with the Python reference,
  and realtime in the browser is not measured. Its results are labelled
  as the compatibility path (`embedded_drive.rs:412-420`).
- **Mecanum geometry must be declared**: `derive` refuses it from the model
  (`drive_geometry.rs:176-183`), because the model does not describe the
  rollers.
- **Fixed by focus-safety-closure** (see "Safety closure" above): a paused
  live request keeping its age; native Stop/action buttons not disarming
  held inputs; the session's motor ambient disagreeing with the model; the
  deadman bound written in two places.
- **Nothing here has run.** Every trace is by reading. The written tests
  have never run.
