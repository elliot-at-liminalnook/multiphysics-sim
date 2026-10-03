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
   into wheel position targets.

The proof robot is `examples/wheeled-robot/baseline/robot.simrobot.json`,
with `robot.controller.json` and `robot.drive.json` beside it.

**Status legend.**

- **Implemented by reading (batch rover-drive-layers)**: written in that
  batch and traced below from the control to the effect.
- **Implemented by reading (unexecuted)** (batch rover-rest-flow, RV-01 to
  RV-05, RV-07 and RV-40): written in that batch and traced below from the
  REST command to RoboCAD or the runtime.
- **Next epic (rover-browser-drive)**: not written yet. One line says what
  is missing and where it will go.
- **Hardware (run sheet, never driven by the agent)**: needs the physical
  robot. The agent writes a run sheet and never drives hardware.

**Everything here is by reading, unexecuted.** Nothing was compiled or run:
no cargo build, check or test, no Python test, no viewer, no screenshot. The
Rust tests (`crates/sim-domain-control/tests/drive.rs`,
`crates/sim-spatial/src/robot/run/tests.rs:593-715`,
`crates/sim-spatial/src/robot/drive_input/tests.rs`,
`crates/sim-runtime/tests/controller_binding.rs`, and from rover-rest-flow
`crates/sim-runtime/src/drive_host/tests.rs`,
`crates/sim-runtime/tests/system_robot.rs`,
`crates/sim-runtime/src/cad_client/physical_tests.rs:155` and
`crates/sim-spatial/src/cad/results/tests.rs:386`) and the Python test
(`clients/python/tests/test_drive.py`) are written and have never run. The
REST script `clients/python/examples/build_rover_over_rest.py` has never
run either. Line numbers are from the working tree on 2026-10-02
(rover-drive-layers) and 2026-10-03 (rover-rest-flow, and the RV-18 to
RV-37 lines that moved when `TwistState` moved to
`crates/sim-runtime/src/drive_host.rs`). Other batches are editing some of
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
| RV-38 | Browser driving | Next epic (rover-browser-drive) | [RV-38](#rv-38-browser-driving-next-epic) |
| RV-39 | Hardware twist path | Hardware (documentation only) | [RV-39](#rv-39-hardware-twist-path-run-sheet) |
| RV-40 | The committed example script that runs the flow over REST | Implemented by reading (unexecuted) | [RV-40](#rv-40-the-committed-example-script) |

## Before you start (for the person who runs it later)

- Open the rover in Robot mode:
  `cargo run -p sim-spatial -- --robot examples/wheeled-robot/baseline/robot.simrobot.json`
  (`examples/wheeled-robot/README.md`, "Teleoperation drive").
- For the REST flow (RV-01 to RV-07, RV-40), start the viewer in CAD mode
  on the seed instead:
  `cargo run -p sim-spatial -- examples/wheeled-robot/baseline/robot.rcad`,
  then run `clients/python/examples/build_rover_over_rest.py --out DIR`
  ([RV-40](#rv-40-the-committed-example-script)).
- `python3` must be on `PATH`: `sim_couple::python` runs `python3 -u script
  args…` (`crates/sim-couple/src/native.rs:168-178`).
- The physics is uncalibrated, and the limits are estimates (see
  [Known limits](#known-limits)).

## Reading traces (by reading, unexecuted)

### RV-06 Controller program on the seam

1. **The binding file** `examples/wheeled-robot/baseline/robot.controller.json`
   names the language `python`, the script
   `../../../clients/python/examples/diff_drive_rover.py`, empty `args`, and
   the drive profile `robot.drive.json`. It carries no physical value.
2. **Parsing**: `ControllerBinding::from_json`
   (`crates/sim-runtime/src/controller_binding.rs:59-82`). The schema comes
   first; a newer `sim.controller-binding/N` is named as newer (67-77). Then
   serde, with unknown fields refused (`deny_unknown_fields`, 33). Then
   `validate` (84-103): only `python`, a non-empty script, no
   `--drive-json` in `args` (the host adds it, 96-98), a non-empty
   `drive_profile`.
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
   (`crates/sim-spatial/src/robot/drive_input/bindings.rs:121-156`).
3. Input system: `drive_input::devices`
   (`crates/sim-spatial/src/robot/drive_input/input.rs:135-267`),
   registered in robot mode's input chain
   (`crates/sim-spatial/src/robot/mod.rs:232`, `InputSet::Window`). It runs
   only for a controlled run (160-166). Physical keys come from
   `ButtonInput<KeyCode>`, not text; Cmd/Ctrl/Alt chords are skipped (167,
   208).
4. `DriveBindings::keyboard_axes` (`bindings.rs:377-385`) sums the held keys
   per axis and clamps each axis to −1..1 (input.rs:209).
5. `supported_only` (input.rs:49-61, used at 245-247) zeroes the axes the
   profile lacks and names them in `DriveInput.ignored`. On the rover, Q
   held with W drives forward and lists `lateral`.
6. Nonzero axes are sent every frame as quiet `Act<RobotAction::Drive {
   Axes }>`, with a redraw request (255-259). Release:
   [RV-20](#rv-20-stop-on-release). From here the path is RV-19 step 3.

By reading, unexecuted.

### RV-09 Device bindings: gamepad

1. **Control**: left stick Y forward, right stick X yaw (inverted, so stick
   left is +yaw), left stick X lateral (inverted), South `stop`, East `halt`,
   deadzone 0.15 (`bindings.rs:142-153`; sign rule in the module notes,
   14-20).
2. `devices` reads every `Gamepad` component (input.rs:214-217), but only
   while a window of this app has focus (211, 218-223).
3. `DriveBindings::gamepad_axes` (`bindings.rs:388-400`) passes each stick
   through `shape` (421-427): zero inside the deadzone, the rest of the
   travel rescaled to 0..1. NaN reads as zero.
4. After a stop, the pad stays blocked until it is focused, neutral and has
   no bound button held (input.rs:224-232).
5. Keyboard and pad are added and clamped per axis (`add`, 88-91, at 248),
   and the source is named (`keyboard`, `gamepad` or `keyboard+gamepad`,
   249-254). From here it is the same as RV-08 step 6.

By reading, unexecuted.

### RV-10 Bindings settings and defaults

1. **Control**: REST `drive_bindings` with no arguments (read),
   `{"bindings": {…}}` (set) or `{"reset": true}`. Spec:
   `crates/sim-spatial/src/app/settings/actions.rs:34-44`.
2. `apply` → `drive_bindings` (actions.rs:59, 65-75). Setting a value runs
   `BindingsFile::from_value` (`bindings.rs:254-262`): schema first, then
   serde (unknown fields refused), then `validate`/`compile` (264-323).
   Every refusal names its field, for example
   `drive_bindings.keyboard.axes[2].key`. Reserved keys G/C/J/F/H, Space,
   Enter, Tab and Escape are refused with the reason (37-47). A key or button
   bound twice is refused (272-279).
3. `SettingsOwner::set_drive_bindings`
   (`crates/sim-spatial/src/app/settings/mod.rs:238-252`) validates again and
   raises the revision. The snapshot written through the preferences
   publication includes the group (`jobs::with_drive_bindings`,
   `app/settings/jobs.rs:394`; also on shutdown, `mod.rs:336-351`). On load,
   the group is read and refused by name if invalid (`jobs.rs:159-168`), and
   it reaches the owner at `app/settings/plugin.rs:202`.
4. `drive_input::sync_bindings`
   (`crates/sim-spatial/src/robot/drive_input/plugin.rs:23-37`, JobResults
   after `SettingsSet::Publish`) rebuilds `DriveBindings` with
   `set_if_neq`.
5. The defaults are data in code and are stored only once the user sets
   bindings (`bindings.rs:8-11`). `reset` stores None, so a later change to
   the defaults reaches the user.
6. Shown: the answer `{schema, stored, bindings, describe}`
   (`bindings.rs:431-436`) plus `settings` status (actions.rs:73-74). The
   inspector's DEVICE BINDINGS block (RV-31) and `robot_state.bindings`
   (RV-32) show the same table.

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
   (`controller_binding.rs:271`).

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
4. The element itself: `from_parameters` (747-805) refuses a
   `deadman_timeout` shorter than `period` (801-803). `sample` (813…)
   applies the same rule as `kinematics::step`.

By reading, unexecuted.

### RV-13 Geometry derivation with provenance

1. `sim_domain_robot::drive_geometry::resolve`
   (`crates/sim-domain-robot/src/drive_geometry.rs:229-244`), called by the
   binding loader (`controller_binding.rs:272-273`). Model geometry goes to
   `derive` (173-227).
2. `derive` refuses a mecanum drive from the model (175-182) and requires
   gravity along −z (184-187). Per wheel (`wheel`, 63-117): the sign comes
   from the joint axis's +y component (94), and the radius from the wheel
   link's collision vertices about the axis (95-113). The wheels must share
   a parent (`same_parent`, 119), have equal radii (`equal_radii`, 132-150)
   and lie on one axle line (197-203). Left must be at +y (204-210).
3. Each value is `Provenance::Derived { from }` naming the joints, links
   and method: track width (212-216), wheel radius (in `equal_radii`), and
   each wheel's sign (`wheel_joint`, 151-164). Missing data is an error that
   names it. Nothing defaults.
4. For the rover (design numbers, not run): track 0.12 m from the axle
   origins y = ±0.06 m, radius 0.03 m, both signs +1 (axis [0,1,0]).
5. Shown with provenance in the inspector's Drive profile block
   (`crates/sim-spatial/src/robot/controls.rs:743-772`) and in
   `robot_state.drive.geometry` (`run/controlled.rs:202`).

By reading, unexecuted.

### RV-14 Limits against the motors; deadman against the period

1. `resolve` then calls `check_deadman` and `check_speeds`
   (`drive_geometry.rs:241-242`).
2. `check_deadman` (249-261) requires `deadman.timeout_s` to be strictly
   longer than the model's `control.period_s`. The rover has 0.5 s against
   0.02 s.
3. `check_speeds` (319-357) takes the slowest wheel's speed limit
   (`joint_speed_limit`, 270-292: the motor's gearbox `max_output_speed` /
   `gear_ratio`, or through a transmission) times the wheel radius as
   `v_free`. It refuses a forward or lateral `max_speed` above `v_free`, and
   a yaw `max_speed` above `v_free / lever` (track/2 for a differential),
   naming the axis, the ceiling and the derivation (348-353). A wheel joint
   without a motor is refused (289-291).
4. Rover: 14.66 rad/s × 0.03 m = 0.44 m/s, so 0.26 m/s passes; the yaw
   ceiling 2 × 0.44 / 0.12 = 7.3 rad/s, so 4.3 rad/s passes. These are the
   profile's own provenance numbers; the computation was not run.

By reading, unexecuted.

### RV-15 Rust reference mixers and the golden file

1. `kinematics.rs` (`crates/sim-domain-control/src/drive/kinematics.rs`,
   standard library only): `scale` (161-179), `check_twist` (183-196),
   `limit` (200-216), `deadman_expired` (220-222) and `step` (237-250).
2. `DifferentialDrive::mix` (281-292): lateral ≠ 0 is refused as
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
4. `controller_binding::load` (`controller_binding.rs:240-304`): parse
   (243-244), canonical script and its sha256 (247-254), `clients_root` = the
   nearest `clients` ancestor (256-264), the simloop library hash (265-266),
   the profile (268-271), geometry and checks (272-273, RV-13/RV-14), the
   resolved drive (274), `--drive-json` appended (278-280), the
   `ExternalProgram` with `profile_sha256` and `library_sha256` (285-293),
   and the `ControllerIdentity` (295-302).
5. `ControlledRun::new` (`robot/run/controlled.rs:52-56`) builds the scene
   (`controller_binding::scene`, 308-319: period = the model's
   `control.period_s`, duration `DRIVE_DURATION_S` 600 s, 23).
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
   → worker (`run/worker.rs:330-340`) → `build` (112-137) →
   `Sim::build` (`run/sim.rs:58-61`).
2. `Session::new(scene, seed 0)` (`crates/sim-runtime/src/session.rs:326`).
   `program.validate` and `check_external` (352-357; 586-607) check the
   script's sha256 and the simloop library's sha256 on disk against the
   recorded ones. A change is refused naming both hashes.
3. The plant is built, the seam is taken and its `<joint>.target` bounds come
   from joint limits (359-381). The inputs are the binding's four command
   channels (`drive_inputs`, `controller_binding.rs:120-138`), checked at
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
   `crates/sim-runtime/src/drive_host.rs:235-239`), which builds the
   session (step 2) and runs `check_inputs` (`drive_host.rs:86-92`) to
   confirm the four channels in order, with a default `TwistState`. The run
   is `Sim::Controlled { host, run }` (`run/sim.rs:60`).

By reading, unexecuted.

### RV-19 Twist path: held W to the motor targets

1. **Control**: hold W → `devices` writes `Act::quiet(Drive { Axes {
   forward: 1, lateral: 0, yaw: 0 } })` every frame
   (`drive_input/input.rs:255-259`).
2. `actions::apply` (`robot/actions/mod.rs:615`, `RobotSet::Actions`,
   `robot/mod.rs:235`) → `handle` (551-597, catch-all at 596) → `dispatch`
   (294). Drive takes its own branch (306-316).
3. `drive_request` (`actions/mod.rs:110-116`): with no binding, refused
   naming the reason (111-114). Then the shared interpretation
   `DriveRequest::interpret` (`crates/sim-runtime/src/drive_host.rs:68-81`,
   the one Build mode uses too): `Axes` → `kinematics::scale(axes,
   &resolved.limits())` (71-72): 1.0 × 0.26 = 0.26 m/s forward. Actions:
   `profile.action(name)` → Stop/Halt (75-78).
4. `RunController::drive` (`run/controlled.rs:171-181`) →
   `check_drive_request` (157-167): `check_drive` (134-151: recorded
   preset, no binding, replay, failed or ended), `check_twist` (160), and a
   nonzero request only while running (161-165). Then
   `thread.send(Command::Twist { request, halt })` (177;
   `run/protocol.rs:126`).
5. Run thread: `drain` (`run/worker.rs:39-51`) applies every queued command
   in order (140). `Command::Twist` (318-328) → `Sim::twist`
   (`run/sim.rs:97-105`) → `DriveHost::request` (`drive_host.rs:255-258`,
   at the host's sim time) → `TwistState::request` (121-139):
   `check_twist`, heartbeat + 1 (129), and `last_request_s` = sim now (130).
   The status is published at once (worker.rs:325).
6. Each pass while running: `s.advance()` (worker.rs:385) → `Sim::advance`
   for Controlled (`sim.rs:93`) → `DriveHost::step` (`drive_host.rs:267-274`)
   → `TwistState::advance` (145-155) → `kinematics::step(commanded,
   request, period, now − last_request_s, …)` (147), clamped to
   ±max_speed (149). The action is `[f, l, y, heartbeat]` (152) →
   `Session::step` (`drive_host.rs:271`; the twist state is committed only
   when the step ran, 272; `session.rs:489-518`): bounds checked
   (503-510), the values stored (511), the action recorded (512), and `robot.advance(period_s)` (513).
7. In `PhysicalRobot::advance` (`crates/sim-runtime/src/physical.rs:654-690`),
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
   firmware `target` port (`physical.rs:448-449`). The CAD motor firmware
   tracks the position reference through the H-bridge and motor.
10. Shown: link poses through `RunController::poll`
    (`run/controller.rs:324`, drive status at 362-363; called from
    `scene.rs:377`), the Drive block's requested/commanded lines (RV-31)
    and `robot_state.drive.status` (RV-32).

By reading, unexecuted.

### RV-20 Stop on release

1. **Control**: release every driving key and centre the stick.
2. `devices`: axes are zero and `latch.sending` was true, so it writes one
   quiet `Drive { Axes 0,0,0 }` (`input.rs:260-264`).
3. RV-19 steps 2-5 with a zero twist. Zero is accepted in any phase that
   can take a twist (`controlled.rs:161`). On the run thread the request
   is ZERO with a fresh heartbeat, and `kinematics::step` ramps the
   commanded twist down at `max_accel` (0.5 m/s², so about 0.52 s from
   0.26 m/s; `kinematics.rs:249`).
4. Python passes the limited twist through; the wheel targets stop
   advancing as the twist reaches 0.

By reading, unexecuted.

### RV-21 Stop on focus loss

1. **Control**: click another application while W is held.
2. `devices` reads `WindowFocused { focused: false }` (`input.rs:139,
   149`) → stop reason "the window lost focus" (180-181). It is sent only
   if the devices were driving (`latch.sending`), so a REST client's
   requests are left to the deadman (177-179).
3. It writes `Act::ui(Drive { Stop })` (196-200) → `drive_request` →
   `DriveRequest::interpret`: `Stop` → `(ZERO, false)` (`drive_host.rs:79`)
   → RV-19 steps 4-5. Then
   `disarm` (189-195): held keys are blocked until released and the pad is
   blocked until neutral (224-232).
4. Bevy also releases every key on focus loss, and the pad is not read
   without focus (211, 218).

By reading, unexecuted.

### RV-22 Stop on Escape

1. **Control**: Escape (not bindable: `bindings.rs:46`).
2. `devices` (`input.rs:182-183`): Escape sends Stop whenever no text field
   has the keyboard and no chord is held. It is shown in the header only
   when the devices were driving; otherwise it goes quietly (199). The Leg
   calibration panel's own Escape STOP still runs (doc at 119-122).
3. As RV-21 step 3: `Stop` → zero twist, approached under `max_accel`,
   then disarm.

By reading, unexecuted.

### RV-23 Stop button and the stop action

1. **Controls**: the Drive block's **Stop** button
   (`robot/controls.rs:672`, a kit button carrying `RobotAction::Drive {
   Stop }`); **X** (key action `stop`, `bindings.rs:140`); gamepad
   **South** (150); REST `robot_drive {"stop": true}` or
   `{"action":"stop"}`; `system_ui drive:stop` / `drive:action:stop`.
2. Button: `actions::buttons` (`actions/keys.rs:8-12`) writes `Act::ui`.
   Key or button action: `devices` sends a zero request first if driving,
   then `Drive { Action { name } }` (`input.rs:233-242`), then disarms.
3. `drive_request` → `DriveRequest::interpret`: `Stop` → `(ZERO, false)`
   (`drive_host.rs:79`); `Action "stop"` → `profile.action` →
   `ActionRequest::Stop` → `(ZERO, false)` (75-76). An unknown name is refused listing the
   profile's actions (`profile.rs:494`).
4. RV-19 steps 4-5. The run thread ramps to zero under `max_accel`.

By reading, unexecuted.

### RV-24 Halt

1. **Controls**: **B** (`bindings.rs:140`), gamepad **East** (151), the Drive
   block's **halt** button (`controls.rs:673-675`), REST
   `{"action":"halt"}`, `system_ui drive:action:halt`.
2. `DriveRequest::interpret`: `ActionRequest::Halt` → `(ZERO, true)`
   (`drive_host.rs:77`). A halt passes `check_drive_request` in any phase
   (`controlled.rs:161`).
3. Run thread: `TwistState::request` with `halt` sets both the request and
   the commanded twist to ZERO at once (`drive_host.rs:132-134`). The next
   period sends 0. The Python `DriveState` passes it through unlimited
   (`drive.py:506-508`), so the targets stop advancing in that period and
   the firmware holds them.

By reading, unexecuted.

### RV-25 Deadman

1. **Cause**: no fresh request for `timeout_s` (0.5 s) of simulation time,
   for example a REST client that stops sending, or a single `system_ui`
   `drive:forward`.
2. Run thread: `TwistState::advance` computes age = now −
   `last_request_s` (`drive_host.rs:146`) → `kinematics::step` →
   `deadman_expired` (`kinematics.rs:220-222, 241-246`) → ramp at
   `stop_decel` (rover `ramp`) or zero. `expired` is recorded (151).
3. Controller: the heartbeat stops rising, and `DriveState.update`
   (`drive.py:503-505`) applies the stop rule from its last output once its
   own age passes `timeout_s`. It is the guard if the run thread's
   channels stop changing.
4. Shown: "deadman EXPIRED (on loss: ramp)" in the Drive block
   (`controls.rs:693`) and `robot_state.drive.deadman.expired`
   (`controlled.rs:204-205`). The rule text is `DEADMAN_RULE`
   (`controlled.rs:31`).

By reading, unexecuted.

### RV-26 Stop when a text field takes the keyboard

1. **Control**: hold W, then click a kit text field (for example the gait
   path field or the comment composer).
2. `Typing::get` (`crates/sim-spatial/src/ui_kit/text/mod.rs:161-167`) turns
   true. `devices` sees `typing_started` while the keyboard was driving
   (`input.rs:150-151, 184-185`) and sends one Stop, then disarms. While
   typing, no key is readable (208).

By reading, unexecuted.

### RV-27 Pause

1. **Control**: Pause (Space while running, `run:pause`, REST).
2. `RunController::act` sets `running = false` and sends `Command::Pause`
   (`run/controller.rs:289-292`). The worker sets `running = false`
   (`run/worker.rs:342-345`) and blocks in `drain` for the next command
   (111). No simulation time passes, so the deadman cannot expire.
3. While paused, a nonzero request is refused on the UI thread
   (`controlled.rs:161-165`, `NOT_RUNNING`, 34) and on the run thread if it
   raced the Pause (`worker.rs:317`). Stop and halt are accepted.
4. On Run, a request still live keeps the age it had (see
   [Known limits](#known-limits)). Held inputs re-send each frame, so
   driving resumes at once.

By reading, unexecuted.

### RV-28 Leaving Robot mode

1. **Control**: switch to another mode, or close the window.
2. `leave_robot` (`crates/sim-spatial/src/app/switch/leave.rs:70-78`)
   removes `RobotView` and drops it off the UI thread
   (`jobs::drop_off_thread`). `RunThread::drop`
   (`crates/sim-spatial/src/jobs/run_thread.rs:109-125`) closes the command
   channel.
3. The worker's `drain` reports the channel closed. It applies what is
   queued, then returns (`worker.rs:111, 366-368`). Dropping the `Session`
   drops the `FrameCoupler`, whose `close` sends `close`, closes stdin and
   reaps or kills `python3` within 250 ms (`native.rs:132-160`).
4. Robot mode's input chain stops running outside Robot mode
   (`robot/mod.rs:243`), so no drive request outlives the mode. A carried
   REST call is abandoned (`robot/mod.rs:217-224`).

By reading, unexecuted.

### RV-29 REST `robot_drive`

1. **Control**: `POST robot_drive {"forward":0.5,"lateral":0,"yaw":0}`,
   `{"action":"halt"}` or `{"stop":true}`. Spec:
   `robot/actions/commands.rs:22`.
2. `wire::Command::RobotDrive` (`actions/mod.rs:840`) →
   `TryFrom` (`commands.rs:85`) → `DriveRequest::from_fields`
   (`crates/sim-runtime/src/drive_host.rs:47-60`, shared with Build's
   `system_drive`). It needs exactly one of axes, action or `stop: true`,
   and names the fields given when they are mixed (54-57). Absent axes
   are 0.
3. RV-19 steps 2-6. REST is strict: a nonzero lateral on the rover is
   refused by `scale` naming `lateral` (`kinematics.rs:170-173`), where the
   device layer would zero it.
4. While live motor sync streams, axes requests from REST are refused
   (`actions/mod.rs:646, 773, 783`); stop and actions still pass.
5. A refusal is kept in `robot_state.drive.last_refusal`
   (`controlled.rs:174, 210`) and in `drive_input.last_error`
   (`actions/mod.rs:699-712`). The answer is `robot_state` with `bindings`
   and `drive_input` (713-733).
6. A REST client must repeat its request faster than `timeout_s`; the
   deadman stops it otherwise (RV-25).

By reading, unexecuted.

### RV-30 `system_ui` drive controls

1. **Control**: `system_ui {"action":{"operation":"controls"}}` then
   `activate` with the id and `ui_revision`.
2. `controls` (`actions/mod.rs:387-459`) adds `drive_controls` for a
   controlled run (431-434). `drive_controls` (463-477) provides
   `drive:forward|back|left|right`, each one full-axis request labelled
   momentary with the deadman, plus `drive:stop` and
   `drive:action:<name>` per profile action. Ids are listed at
   `commands.rs:41-42`.
3. `Activate` (`actions/mod.rs:577`) → `dispatch` → RV-19 steps 3-6. A
   `drive:*` refusal updates `drive_input.last_error` (699-712).

By reading, unexecuted.

### RV-31 Inspector Drive panel

1. `controls::drive_panel` (`robot/controls.rs:630-739`), registered in
   Present (`robot/mod.rs:241`). It is built only for a controlled run
   (641-648) and rebuilt when the run, roots or actions change (656-685).
2. Buttons: Stop and each profile action (666-675), enabled by `check`
   (669, 735-737).
3. Live lines (686-733): requested/commanded twist with fixed decimals, the
   deadman, age, heartbeat and sim time (689-702); device input axes,
   source, ignored axes and last refusal (704-718); RUN FAILED (719); the
   DEVICE BINDINGS table (720-730). A text is rewritten only when its value
   changes (731-733).
4. Static detail (`drive_detail`, 743-772): script and sha256, binding,
   profile and sha256, description, kinematics, geometry with provenance,
   resolved limits per axis with units, and the deadman.

By reading, unexecuted.

### RV-32 `robot_state` drive fields

1. `state_json` sets `drive` = `RunController::drive_json` for a controlled
   run (`robot/state.rs:223-227`).
2. `drive_json` (`run/controlled.rs:187-214`) holds: label, fidelity,
   availability, binding, model, profile path and sha256, kinematics,
   geometry, per-axis limits with units (196-198), identity, rule, deadman
   (timeout, on_loss, clock rule, expired, age), session (seed, period,
   duration, channels), requested, status, `accepts_motion`,
   `last_refusal`, `last_apply_error` and error. An unbound run reports
   `bound: false` with the binding error (191-193).
3. `with_drive_input` (`state.rs:137-149`) adds `bindings` and
   `drive_input` (null unless controlled). Every answer and the 100 ms
   publication pass through it (`actions/mod.rs:718, 812`).

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
   (`run/sim.rs:216-221`, the host's session): `Session::recording()`
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
4. Each chunk: `Sim::advance_replay` (`run/sim.rs:189-195`) steps one
   recorded action through `DriveHost::step_recorded`
   (`drive_host.rs:277-282`: `Session::step`, then `TwistState::replayed`,
   161-171). Live twists are refused during it (`worker.rs:263`;
   `controlled.rs:143-145`).
5. End: `finish_replay` → `end_drive_replay` (`replay.rs:213-214`;
   `sim.rs:108-112` → `DriveHost::replay_ended`, `drive_host.rs:284-287` →
   `TwistState::replay_ended`, 180-185), so nothing keeps driving. Verdict
   done/failed at `replay.rs:223-225` (`DRIVE_VERDICT_RULE`, `recording.rs:347`). States
   are not compared.

By reading, unexecuted.

### RV-35 Replay refused by identity

1. Edit `diff_drive_rover.py`, `simloop/*.py` or `robot.drive.json`, or the
   model, then Replay an older recording.
2. `differences` (`run/controlled.rs:85-122`) compares through
   `ControllerIdentity::differences` (`controller_binding.rs:166-181`:
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
   phase to Failed with "advance failed at t = … s: …" (`worker.rs:433-448`).
4. Shown: RUN FAILED in the Drive block (`controls.rs:719`),
   `robot_state.drive.error` (`controlled.rs:212`). Further drive requests
   are refused (`controlled.rs:147`). Reset rebuilds.

By reading, unexecuted.

### RV-37 Preset coexistence

1. Preset motion keys act only while `motion_keys_active()`
   (`run/preset_ops.rs:28-30`, checked at `actions/keys.rs:50`). Drive input
   acts only while `controlled()` is Some (`input.rs:160`). A run is one or
   the other (`controlled.rs:126-128`; `check_drive` refuses a preset by
   name, 139).
2. On a controlled run, motion requests and held inputs are refused naming
   the controller (`run/sim.rs:126, 156`; `preset_ops.rs:44`), and so are
   jogs (`run/controller.rs:228-229`).
3. Both systems sit in the same chain (`robot/mod.rs:232`).

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
   (`crates/sim-domain-robot/src/drive_geometry.rs:229`) → `derive` (173).
   Track = left anchor y − right anchor y (204): 0.06 − (−0.06) = 0.12 m.
   Wheel radius = the largest distance of the wheel's collision vertices
   from the joint axis (102-111): about 0.03 m. Sign +1 from the +y axis
   (94). Only the profile's `left` and `right` joints are read, so the
   passive axle is not in the mixer. The baseline test asserts these
   numbers on the fixture's export of the same recipe
   (`the_wheeled_robot_derives_its_drive_geometry`, 371-376). The script
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
   (`crates/sim-web/src/lib.rs:351`) and lessons
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
   `builder/robot_run.rs:52-60`), a "builder-run" `RunThread` with join
   bound zero (58), so dropping the run never waits on a controller that is
   still starting.
2. **Build on the run thread**: `robot_thread` (`robot_run.rs:297-399`) →
   `Worker::build` (203-232) → `sim_system::flatten` →
   `system_robot::resolve` (215; `crates/sim-runtime/src/system_robot.rs:66-148`):
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

   Then `RobotSystem::host` (`robot_run.rs:221`; `system_robot.rs:179-181`)
   → `DriveHost::new` (`crates/sim-runtime/src/drive_host.rs:235-239`) →
   `Session::new` (`crates/sim-runtime/src/session.rs:326`) →
   `spawn_external` (431-432 → 682-688; `sim_couple::python` at 684) and
   the seam attach with the command channels (446-450). `check_inputs`
   (`drive_host.rs:86-92`) confirms the four channels.
3. **Drive**: `system_drive {"forward":0.5}` (script 557, re-sent at 10 Hz
   by `drive_steadily`, 539-547) → `SystemAction::SystemDrive`
   (`builder/system_actions.rs:85-96`) → `DriveRequest::from_fields`
   (360-361; `drive_host.rs:47-60`) → `Builder::drive`
   (`robot_run.rs:69-86`). The run panel's Forward, Back, Left, Right and
   Stop buttons (`builder/ui.rs:207-214`) write `BuildAction::Drive` →
   `dispatch` (`builder/actions.rs:199-202`) → the same `Builder::drive`.
   `check_drive` (`robot_run.rs:89-125`) refuses a run that is not a robot
   system (91-93), anything but a stop or halt while a reset is in
   progress (98-107), and a failed or ended run (110-114); it interprets
   the request against the loaded profile (`DriveRequest::interpret`, 116;
   `drive_host.rs:68-81`), accepts only stop before the system has loaded
   (117-119), and a nonzero request only after Run (121-123). Accepted → `RunControl::Twist` (79).
4. **Run thread**: every queued command, in order, before the next period
   (`robot_run.rs:325-377`). `Twist` (375) → `Worker::twist` (235-249) →
   `DriveHost::request` (248; `drive_host.rs:255-258`) →
   `TwistState::request` (121-139). Each period while running, never
   faster than real time (`robot_run.rs:381-388`) → `Worker::period`
   (252-270) → `DriveHost::step` (255; `drive_host.rs:267-274`):
   `TwistState::advance` (270; 145-155), then `Session::step` (271;
   `session.rs:489`, values stored 511, `robot.advance(period_s)` 513).
   This is the same host Robot mode steps (RV-19 step 6).
5. **Shown**: `system_state.live_run.drive` (`builder/live_run.rs:263,
   276`; `builder.rs:514`) = `Builder::drive_json` (`robot_run.rs:131-152`):
   the phase, the system (instances, files, limits with units, deadman,
   period, channels, wiring: `RobotSystem::json`,
   `system_robot.rs:205-228`), the status, the last request sent and
   refused, the last apply error and the run's error. The script reads it
   after its stop (559) and pauses (560).
6. **Failure**: a step error ends the run, named by the system's instances
   (`robot_run.rs:260-264`; `RobotSystem::name_error`,
   `system_robot.rs:187-200`: "`controller` (external controller (python)
   <script> on <element>): …") → `publish` (`robot_run.rs:272-294`; drive
   at 292) → `live_run.error` (`builder/live_run.rs:273`). The script fails
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
   - No keyboard or gamepad in Build mode yet: the device layer writes
     `RobotAction::Drive` for `RobotView` only (RV-19 step 1). REST
     `system_drive` and the run panel's buttons are the inputs.
   - A Build robot run keeps no run record (`robot_run.rs:26`;
     `Save run` hidden, `builder/ui.rs:198-201`) and draws no robot or
     graphs (`RunControl::Observe` is ignored, `robot_run.rs:373-374`;
     the snapshot has no frame, 287).

By reading, unexecuted.

### RV-38 Browser driving (next epic)

Missing; it is the next epic (rover-browser-drive). The browser cannot host
Python: `EmbeddedSession` refuses an external controller by name
(`crates/sim-runtime/src/embedded.rs:239-241`), and wasm32 refuses it
(`session.rs:663-666, 689-692`). Browser driving needs a Rhai (or
wasm-ported) adapter on the same twist channels, plus browser bindings.

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
- **No keyboard or gamepad in Build mode** (RV-07 step 7): REST
  `system_drive` and the run panel's buttons only.
- **A Build robot run keeps no run record and draws no robot or graphs**
  (RV-07 step 7). Robot mode records the drive session.
- **A `.rcad` changed on disk outside RoboCAD is not detected.** RoboCAD's
  `dirty` and `revision` track only its in-memory document, so such a file
  would be hashed and stamped (`cad/results/export.rs:24-29`).

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
  (`drive_geometry.rs:313-318`).
- **Rhai controllers share the slice-retry re-sample exposure.**
  `PhysicalRobot::advance` restores a snapshot and re-runs a failed slice
  (`physical.rs:663-681`), so the seam can sample the same time again. The
  Python rover rolls back its state (`diff_drive_rover.py:99-106`); Rhai
  controllers such as `velocity-controller.rhai` don't, and can integrate
  twice.
- **A paused live request keeps its age.** No sim time passes while paused,
  so on resume a request that was live keeps driving for up to `timeout_s`
  (0.5 s) of simulated motion with no new input (`DEADMAN_RULE`,
  `controlled.rs:31`).
- **The deadman can expire between frames.** Below fps = scale / timeout_s
  (2 fps at ×1, 16 fps at ×8 with 0.5 s), a held input is refreshed less
  often than the deadman, and the robot stutters (`controlled.rs:31`).
- **The browser can't host Python** (RV-38).
- **Mecanum geometry must be declared**: `derive` refuses it from the model
  (`drive_geometry.rs:175-182`), because the model does not describe the
  rollers.
- **The deadman bound is not stated the same way in two places.** The
  model check needs `timeout_s` strictly longer than `control.period_s`
  (`drive_geometry.rs:255`). The registry element accepts a timeout equal
  to its `period` (`profile.rs:801`). A profile for this viewer always goes
  through the stricter check.
- **Nothing here has run.** Every trace is by reading. The written tests
  have never run.
