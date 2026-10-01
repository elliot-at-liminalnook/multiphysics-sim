# sim-app parity ledger (fold-sim-app, 2026-09-30)

Every user-facing behaviour of `crates/sim-app` (read line by line at
a4fe42d3: `main.rs` 44 lines, `phenomena_app.rs` 403, `cad_app.rs` 395) and
where it lives in the one native viewer, `sim-spatial`. sim-app is deleted
only because every row below is **done** or **deliberately different**.

Status:
- **done-by-reading**: the native path does the same thing, traced by reading
  the code (this run builds nothing; the verification pass builds and runs the
  named tests).
- **deliberately different**: the native viewer does it another way, for the
  reason given.

No row is todo. Paths are under `crates/`; sim-app line numbers are as of
a4fe42d3 (`git show a4fe42d3:crates/sim-app/src/<file>`).

## Launch and CLI (`sim-app/src/main.rs`)

| # | sim-app behaviour | sim-app file:line | Native file:function | Status |
|---|---|---|---|---|
| L1 | `sim-app` with no flags opens the phenomena gallery (default scene) | main.rs:6-11, :18 | `sim-spatial/src/main.rs` `phenomena_mode` via `--phenomena`; `app::run` with `ViewerMode::Phenomena` | deliberately different: sim-spatial's no-flag launch stays Inspect (its documented default); the gallery is `--phenomena`, the mode switcher's Phenomena button, `system_ui` `mode:phenomena` or REST `viewer_mode {"mode":"phenomena"}` |
| L2 | `--scene phenomena\|cad` chooses the window | main.rs:17-18, :28-43 | `main.rs` `--phenomena` / `--robot FILE` (and a positional `*.simrobot.json`, `launch::classify`) | deliberately different: modes are chosen by flag or file type, and switched in the running window (native-viewer.md §1) |
| L3 | `--model FILE` (required for the CAD scene, exit 2 without it) | main.rs:19-21, :29-34 | `main.rs` `robot_mode` (`--robot FILE`); `app/switch.rs` `prepare` (robot arm: "robot mode needs a robot …" refusal) | done-by-reading |
| L4 | `--exhibit N\|title` | main.rs:22-24, :36-41 | `main.rs` `--exhibit` (requires `--phenomena`) → `Documents::exhibit` → `phenomena::enter` → `phenomena/run.rs` `run` (`ExhibitRef::parse(..).resolve`) | done-by-reading |
| L5 | `PHENOMENA_EXHIBIT` environment variable | phenomena_app.rs:55-63 | `main.rs` `phenomena_mode` (used when `--exhibit` is absent) | done-by-reading |
| L6 | Exhibit selection rule: a 1-based number in range, else the first title containing the text (case-insensitive), else exhibit 1 | phenomena_app.rs:56-63 | `phenomena/mod.rs` `ExhibitRef::resolve`; `phenomena/run.rs` `run` (an unmatched selector opens exhibit 1 and says so in the frame's `notice`) | done-by-reading (sim-app opened exhibit 1 silently; the native mode also says why) |
| L7 | Window: title, 1380×840, light clear colour, ambient light | phenomena_app.rs:77-90 | `app/mod.rs` `look` (`ViewerMode::Phenomena`: title "Systems — Phenomena", sim-app's clear colour and ambient, Continuous while focused) | done-by-reading (size is the one window's) |
| L8 | `--validate-only` (none in sim-app) | — | `main.rs` `phenomena_mode` lists the exhibits and resolves `--exhibit`; `robot_mode` reports a v2 file through `build_planar` | done-by-reading (new) |

## Phenomena gallery (`sim-app/src/phenomena_app.rs`)

| # | sim-app behaviour | sim-app file:line | Native file:function | Status |
|---|---|---|---|---|
| P1 | Key `]` / `N` / `Tab`: next exhibit (wraps) | :137-140 | `phenomena/keys.rs` `keys` → `PhenomenaAction::PhenomenaNext` → `phenomena/actions.rs` `apply` → `run.rs` `Run::apply` (`Op::Step(1)`) | done-by-reading |
| P2 | Key `[` / `P` / `Shift+Tab`: previous exhibit (wraps) | :141-144 | `keys.rs` `keys` → `PhenomenaPrevious` → `Op::Step(-1)` | done-by-reading |
| P3 | Digits `1`…`9`, `0`: exhibits 1–10 (only those that exist) | :145-154 | `keys.rs` `keys` → `PhenomenaSelect {exhibit: n}` | done-by-reading |
| P4 | `←`/`→`: knob nudge by one step; `Shift` five | :156-166 | `keys.rs` `keys` → `PhenomenaKnob {steps: ±1 \| ±5}` → `run.rs` `knob_target` (clamp, round to step) → `Exhibit::set_knob` | done-by-reading |
| P5 | `R`: reset the exhibit | :167-170 | `keys.rs` `keys` → `PhenomenaReset` → `Exhibit::reset` on the run thread | done-by-reading |
| P6 | `Space`: pause toggle | :171 | `keys.rs` `keys` → `PhenomenaPause {}` → `Op::Pause(None)` | done-by-reading |
| P7 | `↑`/`↓`: speed ×2 / ÷2, clamped to [1/64, 64] | :172-173 | `keys.rs` `keys` → `PhenomenaSpeed {steps: ±1}` → `run.rs` `speed_target` | done-by-reading |
| P8 | A switch (select, knob, reset) clears the chart, chart clock, grid accumulator and error | :174-179 | `run.rs` `Op::switches`, `Pacing::switched`; the generation is bumped (`gallery.rs` `Gallery::send`) so older frames are never shown | done-by-reading |
| P9 | Advance: nothing while paused or after an error; `real = frame dt.min(0.05)`; `dt = real × time_scale × speed`; skip `dt <= 0` | :182-188 | `run.rs` `Pacing::step` (real = wall time between the run thread's ~60 Hz ticks) | done-by-reading (sim-app advanced in `Update` on the UI thread; now on the `phenomena-run` `RunThread`) |
| P10 | Grid stepping: accumulate, take whole grid steps, carry the remainder | :189-197 | `run.rs` `Pacing::step` | done-by-reading (`phenomena/tests.rs` covers the carry) |
| P11 | An `advance` error is kept and stops the run until a switch | :198-201 | `run.rs` `Pacing::step` (also a panic, kept as the error) | done-by-reading |
| P12 | `SIM_VIEWER_STATS=1` prints simulated vs wall seconds once a second | :202-212 | `run.rs` `Pacing::step` (same text, measured on the run thread) | done-by-reading |
| P13 | Chart: a sample of `signal()` every 1/30 s of real time after a successful advance, keeping the last 1800 (one minute) | :49-51, :213-222 | `run.rs` `CHART_INTERVAL`, `CHART_POINTS`, `Pacing::step` | done-by-reading |
| P14 | Chart y range: data range padded 10 % | :223-227 | `panel.rs` `chart` → `chart::rasterize_span` (pads 8 % of the span) | deliberately different: the one chart rasterizer every mode uses (native-viewer.md §6) |
| P15 | Chart drawn as a gizmo board behind the scene | :248-266 | `panel.rs` `chart`: a kit `chart_image` strip with `chart_label`s (signal label, y range) | deliberately different: the UI kit's chart instead of a 3D board the orbit could hide |
| P16 | Shapes: spheres, rods, blocks through one entity pool; materials per entity | :28-37, :116-123, :239-312 | `phenomena/scene.rs` `setup` (pool meshes), `render` (Present; same transforms, colour set only when changed) | done-by-reading |
| P17 | Lines, arrows, polylines as gizmos | :268-276 | `scene.rs` `render` | done-by-reading |
| P18 | Orbit camera: left-drag orbit, wheel zoom, focus (0, 0.3, 0) from (3, 2.6, 9) | :41-47, :103-111, :377-403 | `scene.rs` `setup` (same pose), `orbit` (right-drag rotates, middle or Shift+right-drag pans, wheel zooms, only over the 3D area), `viewport` | deliberately different: the viewer's orbit convention (left click belongs to the panels); there is no camera shared by every mode, so the mode has its own orbit in that convention |
| P19 | Shadowed directional light | :112-115 | `scene.rs` `setup` | done-by-reading |
| P20 | Status text: number / count, title, summary | :348-354 | `panel.rs` `rebuild` / `refresh` (exhibit list on the left, title and summary on the right) | done-by-reading |
| P21 | Verdict | :354 | `panel.rs` `refresh` | done-by-reading |
| P22 | Knob line: label, value, unit, [min .. max], hint | :354 | `panel.rs` kit `slider` with label, value, unit and range; −/+ buttons; release commits `PhenomenaKnob {value}` (`panel.rs` `slider`) | done-by-reading (a slider is new) |
| P23 | Readouts: label, value (4 decimals), unit | :349-353 | `panel.rs` `refresh` (kit property rows, steady rounding) | done-by-reading |
| P24 | Time with the exhibit's unit, speed, chart signal label | :354 | `panel.rs` `refresh` | done-by-reading |
| P25 | "Simulation error: …" shown | :358 | `panel.rs` `refresh` (in the kit's danger colour; also `phenomena_state.error`) | done-by-reading |
| P26 | Key-help line | :354 | `panel.rs` header hint | done-by-reading |
| P27 | `ascii()`: Greek, arrows, sub/superscripts spelled out (Bevy's default font) | :327-346 | `panel.rs` `glyphs` (only the glyphs IBM Plex Sans lacks: ⁻ ⇒ ∝ ▸ ∓ ⟨ ⟩ ᵀ and dotted letters) | deliberately different: real glyphs from the kit fonts |
| P28 | No REST, no `system_ui` | — | `phenomena/actions.rs` `PhenomenaAction` (`state`, `phenomena_state`, `phenomena_select`, `phenomena_next`, `phenomena_previous`, `phenomena_knob`, `phenomena_reset`, `phenomena_pause`, `phenomena_speed`, `system_ui`) | done-by-reading (new) |
| P29 | Exhibits built on the UI thread at startup | :54 | `run.rs` `run` builds `exhibits::all()` on the run thread; "Building the exhibits…" until the first frame | deliberately different: no UI-thread stall |

## CAD scene (`sim-app/src/cad_app.rs`) → Robot mode

| # | sim-app behaviour | sim-app file:line | Native file:function | Status |
|---|---|---|---|---|
| C1 | Opens v2 (planar) and v3 (physical) files through `AnyRobot::load` | :101-119 | `robot.rs` `load_file_bytes` (version by `simrobot_version`): v3 → `load_bytes` (`PhysicalModel`), v2 → `robot_planar::load_bytes` (`CadModel`) | done-by-reading |
| C2 | v2 build: `CadRobot::build(model, registry, 6.0, 1.0)` | cad_robot.rs (via :105) | `robot_planar.rs` `Worker::build` → `sim_phenomena::scenarios::cad_robot::build_planar` (the one planar build, also used by `AnyRobot::load` and `run_file`) | done-by-reading |
| C3 | File watch: stat every 1 s, rebuild on a changed mtime | :121-135 | `robot.rs` `watch` → `robot_source::SourceWatch::poll` (0.5 s stat, sha256 unchanged rule, failed keeps the last good model) → `receive` → `install_planar` (v2) or the v3 install | done-by-reading (stricter: identical bytes are not rebuilt) |
| C4 | `R`: rebuild from the file | :141-143 | v2: `robot/actions.rs` `planar_keys` (R) → `RobotAction::Run {Reset}` → `PlanarRun` rebuild from the loaded model; re-reading the file is Reload (`robot:reload`, `robot_reload`, the watch) | deliberately different: Reset rebuilds the loaded model, Reload re-reads the file (two actions robot mode already has) |
| C5 | `Space`: pause | :138-140 | v2: `planar_keys` (Space) → `RobotAction::Run {Start\|Pause}`; v3: the header's Run/Pause buttons, `system_ui` run:*, REST `robot_run` | done-by-reading (v2); deliberately different (v3): robot mode's run controls are buttons and REST, Space is not bound there |
| C6 | Pacing: real `dt.min(0.05) × speed`, 0.02 s grid, at most one grid step per frame, accumulator capped | :178-202 | `robot_planar.rs` `Worker::tick` (on the "robot-run (planar v2)" `RunThread`, ~60 Hz) | done-by-reading |
| C7 | Advance error stops and pauses, shown | :195-198 | `robot_planar.rs` `Worker::tick`/`fail` → frame `error`; header and `robot_state` | done-by-reading |
| C8 | `=` / `-`: speed ×2 / ÷2 within [0.125, 8] | :152-157 | `robot/actions.rs` `speed_keys` → `RobotAction::Speed` → `robot_run::speed_target` (v2: `PlanarRun::speed`) | deliberately different: a request past ×8 / ×0.125 is refused with the reason shown, not silently clamped |
| C9 | `←`/`→`: select a joint | :160-165 | v2: `planar_keys` → `RobotAction::SelectJoint`; v3: the inspector's joint list | done-by-reading (v2); deliberately different (v3): robot mode selects links, and jogs joints by name |
| C10 | `↑`/`↓` held: move the selected joint's target ±0.01 rad (Shift ±0.05) | :166-174 | v2: `planar_keys` → `RobotAction::Jog` → `PlanarRun::nudge` → `CadRobot::set_target` on the run thread; v3: jog buttons, `system_ui` jog:*, REST `robot_jog` (`RunController`) | done-by-reading (v2); deliberately different (v3): servo-target jog through the run controller's buttons and REST |
| C11 | `C`: contacts on/off | :149-151, :326-334 | `robot/actions.rs` `overlay_keys` (C) → `RobotAction::Overlay {contacts}`; v2 draws chain-tip contact points (`robot_planar.rs` `draw`), v3 the run thread's contacts | done-by-reading |
| C12 | `S`: stress colouring from the results file (v3) | :144-148, :221-232, :361-366 | v3: `overlay_keys` (H) → `RobotAction::Overlay {stress}` → `robot_stress` (read on the source worker, `StressOverlay`); v2: `planar_keys` (S) and H refused with `robot_planar::STRESS` | deliberately different: robot mode's stress key is H (S is not bound for v3); v2 has no results file, so it is refused by name |
| C13 | v3: link collision meshes posed from `poses()` | :204-274 | `robot.rs` `receive` (link meshes) and `apply_frames` | done-by-reading (pre-existing robot mode) |
| C14 | v2: section outlines in their simulated poses (bbox when no outline), COM dots, chain-tip dots | :287-309 | `robot_planar.rs` `Worker::publish` (`outlines()`, `poses()`, tips) → `robot.rs` `draw` → `robot_planar::draw` | done-by-reading |
| C15 | v3: joint axes, deflections drawn | :317-325, :335-339 | `robot.rs` `draw` (overlays J, F) | done-by-reading (pre-existing robot mode); refused by name for v2 (`JOINT_FRAMES`, `DEFLECTIONS`) |
| C16 | Ground grid | :277-292, :311-316 | `robot_planar.rs` `draw` (v2); robot mode's floor (v3) | done-by-reading |
| C17 | Status: path, paused, error, time, speed, real-time ratio, joint angles and targets, warnings | :344-371 | `robot.rs` header and inspector (`robot_planar::run_line`, `inspector_text`); `robot_state.planar` | done-by-reading; the ratio is reported as `achieved_rate` (sim s per wall s) and `compute_s_per_sim_s`, since sim-app's "× real time" was the inverse |
| C18 | Fidelity of a v2 file (sim-app showed none) | — | `robot_planar.rs` `FIDELITY`, `HEADER_LABEL`; `robot_state.format {version, name, fidelity}` (v3 reports `physical v3`) | done-by-reading (new) |
| C19 | Orbit camera (left-drag orbit, right-drag pan, wheel) | :373-395 | `robot.rs` `orbit` (right-drag, middle/Shift pan, wheel) | deliberately different: the viewer's orbit convention |
| C20 | Features without a v2 meaning | — | `robot/actions.rs` `check_planar` with `robot_planar::{MOTION, SAVE_RECORDING, REPLAY, GAIT, RECORDED, JOINT_FRAMES, DEFLECTIONS, GRAPHS, STRESS, NO_MIRROR}` | done-by-reading (refused by name) |

## RoboCAD's launcher

| # | Behaviour | File | Native | Status |
|---|---|---|---|---|
| R1 | simbridge fell back to `sim-app --scene cad --model` when sim-spatial was not built | `cad/robocad/simbridge.py` `viewer_command` (at a4fe42d3) | `viewer_command`: sim-spatial release, then debug, else no launch and "no simulator viewer built: cargo build --release -p sim-spatial"; `cad/tests/test_simbridge.py` | done-by-reading (the fallback is gone) |

## Totals

58 rows: 44 done-by-reading, 11 deliberately different (L1, L2, P14,
P15, P18, P27, P29, C4, C8, C12, C19) and 3 split rows, done-by-reading
for v2 and deliberately different for v3 (C5, C9, C10). None todo. The verification pass builds
and runs the tests named in
[native-viewer.md "Fold in sim-app"](architecture/native-viewer.md#fold-in-sim-app-2026-09-30).
