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

- **Implemented by reading (batch rover-drive-layers)**: written in this
  batch and traced below from the control to the effect.
- **Next epic (rover-rest-flow)**: not written yet. One line says what is
  missing and where it will go.
- **Hardware (run sheet, never driven by the agent)**: needs the physical
  robot. The agent writes a run sheet and never drives hardware.

**Everything here is by reading, unexecuted.** Nothing was compiled or run:
no cargo build, check or test, no Python test, no viewer, no screenshot. The
Rust tests (`crates/sim-domain-control/tests/drive.rs`,
`crates/sim-spatial/src/robot/run/tests.rs:614-805`,
`crates/sim-spatial/src/robot/drive_input/tests.rs`,
`crates/sim-runtime/tests/controller_binding.rs`) and the Python test
(`clients/python/tests/test_drive.py`) are written and have never run. Line
numbers are from the working tree on 2026-10-02. Other batches are editing
some of these files at the same time, so a cited line can drift by a few
lines.

## Steps

| ID | Step | Status | Trace |
|---|---|---|---|
| RV-01 | Create the rover in CAD through REST | Next epic | [RV-01](#rv-01rv-05-rv-07-rv-38-rv-40-next-epic) |
| RV-02 | Rig its joints (two driven continuous axles, one passive) through REST | Next epic | [RV-01…](#rv-01rv-05-rv-07-rv-38-rv-40-next-epic) |
| RV-03 | Wheel, motor and encoder parts through REST | Next epic | [RV-01…](#rv-01rv-05-rv-07-rv-38-rv-40-next-epic) |
| RV-04 | Annotate it: threads pinned to its nodes | Next epic | [RV-01…](#rv-01rv-05-rv-07-rv-38-rv-40-next-epic) |
| RV-05 | Wire it in the systems editor | Next epic | [RV-01…](#rv-01rv-05-rv-07-rv-38-rv-40-next-epic) |
| RV-06 | Controller program on the seam (binding file + `diff_drive_rover.py`) | Implemented by reading | [RV-06](#rv-06-controller-program-on-the-seam) |
| RV-07 | Hook the controller into the systems editor (Build-mode `SystemSession`) | Next epic | [RV-01…](#rv-01rv-05-rv-07-rv-38-rv-40-next-epic) |
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
| RV-38 | Browser driving | Next epic | [RV-01…](#rv-01rv-05-rv-07-rv-38-rv-40-next-epic) |
| RV-39 | Hardware twist path | Hardware (documentation only) | [RV-39](#rv-39-hardware-twist-path-run-sheet) |
| RV-40 | The committed example script that runs RV-01 to RV-38 over REST | Next epic | [RV-01…](#rv-01rv-05-rv-07-rv-38-rv-40-next-epic) |

## Before you start (for the person who runs it later)

- Open the rover in Robot mode:
  `cargo run -p sim-spatial -- --robot examples/wheeled-robot/baseline/robot.simrobot.json`
  (`examples/wheeled-robot/README.md`, "Teleoperation drive").
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
   `robot_state.drive.geometry` (`run/controlled.rs:338`).

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
5. `ControlledRun::new` (`robot/run/controlled.rs:53-57`) builds the scene
   (`controller_binding::scene`, 308-319: period = the model's
   `control.period_s`, duration `DRIVE_DURATION_S` 600 s, 23).
6. Back on the UI thread, `receive` (`robot/scene.rs:172-181`) calls
   `RunController::spawn_file` (`run/controller.rs:167-178`) or
   `replace_file` (450). A loaded binding becomes `Source::Controlled`; a
   failed one becomes `Source::Unbound`, a failed run naming it from the
   start (179-191). It never falls back to a hold run. The run thread
   starts (`RunThread::spawn`, 193).
7. Shown: the Drive block (RV-31), `robot_state.drive` (RV-32) and the
   header's reload note (`scene.rs:218`).

By reading, unexecuted.

### RV-18 Run builds the session and starts Python

1. **Control**: Run (Space, `run:start`, REST `robot_run {"action":"start"}`)
   → `RunController::act` (`run/controller.rs:281-287`) → `Command::Start`
   → worker (`run/worker.rs:329-339`) → `build` (111-136) →
   `Sim::build` (`run/sim.rs:56-60`).
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
6. `check_inputs` (`run/controlled.rs:79-85`) confirms the four channels in
   order. `Sim::Controlled { session, drive: TwistState::default() }`
   (`run/sim.rs:59`).

By reading, unexecuted.

### RV-19 Twist path: held W to the motor targets

1. **Control**: hold W → `devices` writes `Act::quiet(Drive { Axes {
   forward: 1, lateral: 0, yaw: 0 } })` every frame
   (`drive_input/input.rs:255-259`).
2. `actions::apply` (`robot/actions/mod.rs:634-760`, `RobotSet::Actions`,
   `robot/mod.rs:235`) → `handle` (570-616, catch-all at 615) → `dispatch`
   (313-336). Drive takes its own branch (325-335).
3. `drive_request` (`actions/mod.rs:119-136`): with no profile, refused
   naming the reason (120-123). `Axes` → `kinematics::scale(axes,
   &resolved.limits())` (125-127): 1.0 × 0.26 = 0.26 m/s forward.
   Actions: `profile.action(name)` → Stop/Halt (129-132).
4. `RunController::drive` (`run/controlled.rs:307-317`) →
   `check_drive_request` (293-303): `check_drive` (270-287: recorded
   preset, no binding, replay, failed or ended), `check_twist` (296), and a
   nonzero request only while running (297-301). Then
   `thread.send(Command::Twist { request, halt })` (313;
   `run/protocol.rs:125`).
5. Run thread: `drain` (`run/worker.rs:38-50`) applies every queued command
   in order (139). `Command::Twist` (317-327) → `Sim::twist`
   (`run/sim.rs:104-113`) → `TwistState::request` (`controlled.rs:114-132`):
   `check_twist`, heartbeat + 1 (122), and `last_request_s` = sim now (123).
   The status is published at once (324).
6. Each pass while running: `s.advance()` (worker.rs:384) → `Sim::advance`
   for Controlled (`sim.rs:93-100`) → `TwistState::advance`
   (`controlled.rs:138-148`) → `kinematics::step(commanded, request,
   period, now − last_request_s, …)` (140), clamped to ±max_speed (142).
   The action is `[f, l, y, heartbeat]` (145) → `Session::step`
   (`session.rs:489-518`): bounds checked (502-509), the values stored
   (511), the action recorded (512), and `robot.advance(period_s)` (513).
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
    (`run/controller.rs:323`, drive status at 361-362; called from
    `scene.rs:377`), the Drive block's requested/commanded lines (RV-31)
    and `robot_state.drive.status` (RV-32).

By reading, unexecuted.

### RV-20 Stop on release

1. **Control**: release every driving key and centre the stick.
2. `devices`: axes are zero and `latch.sending` was true, so it writes one
   quiet `Drive { Axes 0,0,0 }` (`input.rs:260-264`).
3. RV-19 steps 2-5 with a zero twist. Zero is accepted in any phase that
   can take a twist (`controlled.rs:297`). On the run thread the request
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
3. It writes `Act::ui(Drive { Stop })` (196-200) → `drive_request`
   `Stop` → `(ZERO, false)` (`actions/mod.rs:133`) → RV-19 steps 4-5. Then
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
3. `drive_request`: `Stop` → `(ZERO, false)` (`actions/mod.rs:133`);
   `Action "stop"` → `profile.action` → `ActionRequest::Stop` →
   `(ZERO, false)` (129-130). An unknown name is refused listing the
   profile's actions (`profile.rs:494`).
4. RV-19 steps 4-5. The run thread ramps to zero under `max_accel`.

By reading, unexecuted.

### RV-24 Halt

1. **Controls**: **B** (`bindings.rs:140`), gamepad **East** (151), the Drive
   block's **halt** button (`controls.rs:673-675`), REST
   `{"action":"halt"}`, `system_ui drive:action:halt`.
2. `drive_request`: `ActionRequest::Halt` → `(ZERO, true)`
   (`actions/mod.rs:131`). A halt passes `check_drive_request` in any phase
   (`controlled.rs:297`).
3. Run thread: `TwistState::request` with `halt` sets both the request and
   the commanded twist to ZERO at once (`controlled.rs:124-127`). The next
   period sends 0. The Python `DriveState` passes it through unlimited
   (`drive.py:506-508`), so the targets stop advancing in that period and
   the firmware holds them.

By reading, unexecuted.

### RV-25 Deadman

1. **Cause**: no fresh request for `timeout_s` (0.5 s) of simulation time,
   for example a REST client that stops sending, or a single `system_ui`
   `drive:forward`.
2. Run thread: `TwistState::advance` computes age = now −
   `last_request_s` (`controlled.rs:139`) → `kinematics::step` →
   `deadman_expired` (`kinematics.rs:220-222, 241-246`) → ramp at
   `stop_decel` (rover `ramp`) or zero. `expired` is recorded (144).
3. Controller: the heartbeat stops rising, and `DriveState.update`
   (`drive.py:503-505`) applies the stop rule from its last output once its
   own age passes `timeout_s`. It is the guard if the run thread's
   channels stop changing.
4. Shown: "deadman EXPIRED (on loss: ramp)" in the Drive block
   (`controls.rs:693`) and `robot_state.drive.deadman.expired`
   (`controlled.rs:340-341`). The rule text is `DEADMAN_RULE`
   (`controlled.rs:29`).

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
   (`run/controller.rs:288-291`). The worker sets `running = false`
   (`run/worker.rs:341-344`) and blocks in `drain` for the next command
   (110). No simulation time passes, so the deadman cannot expire.
3. While paused, a nonzero request is refused on the UI thread
   (`controlled.rs:297-301`, `NOT_RUNNING`, 35) and on the run thread if it
   raced the Pause (`worker.rs:316`). Stop and halt are accepted.
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
   queued, then returns (`worker.rs:110, 365-367`). Dropping the `Session`
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
2. `wire::Command::RobotDrive` (`actions/mod.rs:859`) →
   `TryFrom` (`commands.rs:85-99`). It needs exactly one of axes, action or
   `stop: true`, and names the fields given when they are mixed (92-96).
   Absent axes are 0.
3. RV-19 steps 2-6. REST is strict: a nonzero lateral on the rover is
   refused by `scale` naming `lateral` (`kinematics.rs:170-173`), where the
   device layer would zero it.
4. While live motor sync streams, axes requests from REST are refused
   (`actions/mod.rs:665, 792, 802`); stop and actions still pass.
5. A refusal is kept in `robot_state.drive.last_refusal`
   (`controlled.rs:310, 346`) and in `drive_input.last_error`
   (`actions/mod.rs:718-731`). The answer is `robot_state` with `bindings`
   and `drive_input` (733-752).
6. A REST client must repeat its request faster than `timeout_s`; the
   deadman stops it otherwise (RV-25).

By reading, unexecuted.

### RV-30 `system_ui` drive controls

1. **Control**: `system_ui {"action":{"operation":"controls"}}` then
   `activate` with the id and `ui_revision`.
2. `controls` (`actions/mod.rs:406-477`) adds `drive_controls` for a
   controlled run (451-453). `drive_controls` (482-496) provides
   `drive:forward|back|left|right`, each one full-axis request labelled
   momentary with the deadman, plus `drive:stop` and
   `drive:action:<name>` per profile action. Ids are listed at
   `commands.rs:41-42`.
3. `Activate` (`actions/mod.rs:596-602`) → `dispatch` → RV-19 steps 3-6. A
   `drive:*` refusal updates `drive_input.last_error` (721-731).

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
   run (`robot/state.rs:223-226`).
2. `drive_json` (`run/controlled.rs:323-350`) holds: label, fidelity,
   availability, binding, model, profile path and sha256, kinematics,
   geometry, per-axis limits with units (332-334), identity, rule, deadman
   (timeout, on_loss, clock rule, expired, age), session (seed, period,
   duration, channels), requested, status, `accepts_motion`,
   `last_refusal`, `last_apply_error` and error. An unbound run reports
   `bound: false` with the binding error (327-329).
3. `with_drive_input` (`state.rs:137-149`) adds `bindings` and
   `drive_input` (null unless controlled). Every answer and the 100 ms
   publication pass through it (`actions/mod.rs:737, 818-832`).

By reading, unexecuted.

### RV-33 Save a drive recording

1. **Control**: Save recording, `system_ui recording:save`, or REST
   `robot_save_recording` (`commands.rs:23`).
2. `RunController::save_recording` (`run/preset_ops.rs:180-200`): for a
   controlled run the target is `recording::drive_target` →
   `runs/robot-drive/<stem>/<stamp>.recording.json`
   (`robot/recording.rs:165-167`, `DRIVE_LOCATION_RULE` 343). Then
   `Command::SaveRecording`.
3. Worker (`run/worker.rs:163-185`) → `Sim::save` for Controlled
   (`run/sim.rs:226-231`): `Session::recording()` (`session.rs:561-568`:
   scene with `controller.external` including the script and library
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
   worker (`worker.rs:187-233`) → `prepare_replay` (`run/replay.rs:97-102`)
   → `prepare_drive_replay` (174-205).
3. The identity check is RV-35. `Session::new(recorded scene, recorded
   seed)` starts the recorded controller again, and `check_external`
   re-verifies the script and library hashes (`session.rs:586-607`).
4. Each chunk: `Sim::advance_replay` (`run/sim.rs:197-205`) steps one
   recorded action through `Session::step` and shows it with
   `TwistState::replayed` (`controlled.rs:154-164`). Live twists are refused
   during it (`worker.rs:262`; `controlled.rs:279-281`).
5. End: `finish_replay` → `end_drive_replay` (`replay.rs:210-211`;
   `controlled.rs:173-178`), so nothing keeps driving. Verdict done/failed
   at `replay.rs:220-222` (`DRIVE_VERDICT_RULE`, `recording.rs:347`). States
   are not compared.

By reading, unexecuted.

### RV-35 Replay refused by identity

1. Edit `diff_drive_rover.py`, `simloop/*.py` or `robot.drive.json`, or the
   model, then Replay an older recording.
2. `differences` (`run/controlled.rs:221-258`) compares through
   `ControllerIdentity::differences` (`controller_binding.rs:166-181`:
   script, `script_sha256`, `library_sha256`, args, profile and
   `profile_sha256`). It also compares the language (235-237), the resolved
   drive members (238-247), the robot fingerprint (248-251) and the seam
   period (252-256). Each difference is named with both values.
3. Refused (`replay.rs:195-199`): "refused by the drive identity check: …".
   The worker republishes the current run unchanged
   (`worker.rs:224-233`).
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
   phase to Failed with "advance failed at t = … s: …" (`worker.rs:432-447`).
4. Shown: RUN FAILED in the Drive block (`controls.rs:719`),
   `robot_state.drive.error` (`controlled.rs:348`). Further drive requests
   are refused (`controlled.rs:283`). Reset rebuilds.

By reading, unexecuted.

### RV-37 Preset coexistence

1. Preset motion keys act only while `motion_keys_active()`
   (`run/preset_ops.rs:28-30`, checked at `actions/keys.rs:50`). Drive input
   acts only while `controlled()` is Some (`input.rs:160`). A run is one or
   the other (`controlled.rs:262-264`; `check_drive` refuses a preset by
   name, 275).
2. On a controlled run, motion requests and held inputs are refused naming
   the controller (`run/sim.rs:134, 164`; `preset_ops.rs:44`), and so are
   jogs (`run/controller.rs:227-228`).
3. Both systems sit in the same chain (`robot/mod.rs:232`).

By reading, unexecuted.

### RV-01–RV-05, RV-07, RV-38, RV-40 (next epic)

- **RV-01 to RV-03 (CAD rover through REST)**: missing. The rover is built
  today by RoboCAD's Python `cad/scripts/wheeled_learning_fixture.py`
  (`build`, 18; motors 30, wheel axles 32-34, encoders 41, passive axle 44-45),
  not over the viewer's REST. These will go in a committed example script
  that drives CAD mode's REST commands (rover-rest-flow).
- **RV-04 (annotation)**: missing for the rover. The thread machinery exists
  (CAD threads, REST `robot_threads`, `robot/actions/mod.rs:83-86`). The
  rover's threads will be written by the same script.
- **RV-05 and RV-07 (systems editor)**: missing. Nothing in
  `sim-runtime/src/system_session.rs` or the Build mode knows the controller
  binding or the drive profile. `control.drive_limiter` is registered
  (RV-12), so it can appear there. The wiring and the Build-mode
  `SystemSession` hook will go in rover-rest-flow.
- **RV-38 (browser)**: missing. The browser cannot host Python:
  `EmbeddedSession` refuses an external controller by name
  (`crates/sim-runtime/src/embedded.rs:239-241`), and wasm32 refuses it
  (`session.rs:663-666, 689-692`). Browser driving needs a Rhai (or
  wasm-ported) adapter on the same twist channels, plus browser bindings.
- **RV-40 (the committed example script)**: missing. It will run RV-01 to
  RV-38 over REST end to end and record the result.

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
  `DRIVE_FIDELITY`, `run/controlled.rs:28`).
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
  `controlled.rs:29`).
- **The deadman can expire between frames.** Below fps = scale / timeout_s
  (2 fps at ×1, 16 fps at ×8 with 0.5 s), a held input is refreshed less
  often than the deadman, and the robot stutters (`controlled.rs:29`).
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
