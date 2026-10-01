# Native viewer architecture

**Status:** target shape, adopted 2026-09-30. This is the standard new work is
held to. Agents keep it current: when a change alters the shape, update this
document in the same commit and record the decision.

`sim-spatial` is the one native viewer. Every user workflow (building systems,
robots, lessons, scanned places, inspection) lives in it, on Bevy 0.19.1. The
project rules in `AGENTS.md` still govern everything here. In particular, CAD
owns physical definitions, physics lives in shared crates, and the viewer never
duplicates physics.

## Where it is today (re-measured 2026-10-01 after cad-select-transform; CAD mode verified at a4fe42d3; fold-sim-app verified at 80b5997e; cad-select-transform written and reviewed by reading, pending its verification pass)

- **Bevy 0.19.1**, pinned in the workspace `Cargo.toml` and in
  `crates/sim-spatial/Cargo.toml` (hand-picked features, see
  [Bevy 0.19.1 migration](#bevy-0191-migration-2026-09-30)). Only
  `sim-spatial` depends on Bevy (sim-app, the other Bevy app, was retired
  by fold-sim-app; the workspace's default-feature `bevy` entry is now
  unused).
- **One app, modes as states** (see [One app](#one-app-2026-09-30)):
  `app::run` is the only `App` builder (`App::new()` appears elsewhere in
  `src/` only in `#[cfg(test)]` code). `ViewerMode` (Inspect, Build,
  Lessons, Robot, Place, and since cad-mode Cad) is a Bevy `States`; the computed states
  `ModeScope` and `SpatialScreen` follow it. Since fold-sim-app there are
  seven modes: Phenomena joined (sim-app's gallery). Launch flags choose the initial
  mode and document; the user switches modes in the window (the mode
  switcher, `system_ui` `mode:*`, REST `viewer_mode`), through one handler
  (`app::switch::handle`). Verified at 7da1216e.
- **One REST server** (`rest::bind`, the only `sim_api::Server::bind` in
  sim-spatial, also used by `--headless`) and **one REST poll**
  (`app::actions::serve`, Input): every mode's commands, each tagged with
  its `modes`; one dispatch (`app::route::route`) refuses a command of
  another mode by name. The capability list (97 entries: the action
  layer's 92 plus the hardware front end's 5, `hardware_status`,
  `hardware_stop`, `hardware_export`, `hardware_gaits` and `hardware`) is
  generated from the action registry (`app::actions::capabilities`); no
  hand-written list is left. CAD mode adds 15 (`state`, `system_ui` and the
  13 `cad_*` commands), 112 in all (counted from the specs, not from a
  running server). Phenomena mode adds 10 (`state`, `system_ui` and the 8
  `phenomena_*` commands): 122 in all, re-counted 2026-09-30 from the
  `spec(`/`c(` entries: 121 in the nine action types' `commands()` plus
  the switcher's `system_ui` (`WindowAction::switcher_commands`).
  cad-select-transform adds 16 CAD commands (31 `spec(` entries in
  `cad/actions.rs`, re-counted 2026-10-01): 138 in all. Place mode
  answers `state`, `camera` and `screenshot`.
- **One action layer** (see [Action layer](#action-layer-2026-09-30)),
  verified at 90c65c86: every intent is a typed action
  (`WindowAction`, `InspectAction`, `SystemAction` carrying the builder's
  `BuildAction`, `LessonCommand` carrying `LessonAction`, `RobotAction`,
  `PlaceAction`, since the hardware front end `HardwareAction`, part of
  Robot mode, then `CadAction` and `PhenomenaAction`) written as a Bevy Message (`Act<A>`) by buttons, keys,
  `system_ui` and REST in Input and applied by one system per action type
  in Actions. REST waits on reply tokens (`app::actions::Replies`).
  `Origin` has four variants: `Rest`, `Ui`, `Quiet` and `SystemUi` (a
  `system_ui` activation one mode passes on to another action type).
- **The shared pipeline sets** `ViewerSet` Input → Actions → JobResults →
  SimSync → Present are configured once (`app::ModesPlugin`). Input holds
  the REST poll and every button and key mapping; Actions the apply
  systems; SimSync each mode's continuous work (jobs, orbit and fly
  cameras, drags, text entry, scene sync); Present drawing and REST
  snapshots.
- **Background work goes through one `jobs` module** (`src/jobs/`, see
  [§4](#4-one-background-work-abstraction) and
  [the jobs module](#jobs-module-2026-09-30)): 0 `thread::spawn` /
  `thread::Builder` sites outside `src/jobs/` (there were 35 in 21 files),
  enforced by the lib test `jobs::tests::threads_are_started_only_in_jobs`.
  Verified at ae80a137.
- **Bevy structure** (re-measured 2026-09-30 after fold-sim-app with
  `grep -rn "impl Plugin for"`): 12 plugins (`CorePlugin`, `ModesPlugin`,
  `UiKitPlugin`, `SpatialViewerPlugin`, `BuilderPlugin`, `LearnPlugin`,
  `RobotPlugin`, `PlacePlugin`, `CadPlugin` with its window-free
  `CadCorePlugin`, `PhenomenaPlugin` with `PhenomenaCorePlugin`; the
  hardware panel is part of `RobotPlugin`), one `States` enum (7 modes) and
  two computed states, one set enum, 9 action Message types (`Act<A>` for
  `WindowAction`, `InspectAction`, `SystemAction`, `LessonCommand`,
  `RobotAction`, `HardwareAction`, `PlaceAction`, `CadAction`,
  `PhenomenaAction`). Same greps after fold-sim-app: 27
  `MessageReader<` lines, 57 `MessageWriter<` lines, 11 `On<` lines,
  `KeyCode` 223 times in 17 files, `Interaction` 94 times in 21 files.
  The figures below are the hardware front end's. Site counts, re-measured 2026-09-30 after the hardware front
  end with `grep -rn` over `crates/sim-spatial/src` (lines, comments
  included): 18 `MessageReader<` lines (16 at 4bc03789 by the same grep; 2
  in `robot/hardware/`) and 46 `MessageWriter<` lines (37 at 4bc03789; 8
  in `robot/hardware/`, 1 in `robot/actions.rs`); 10 `On<` lines (pointer
  picks, drags, screenshots; unchanged). `KeyCode` appears 146 times in 11
  files (133 in 10 before, same `grep -ow` method) and `Interaction` as a
  word 83 times in 19 files (76 in 17 at 4bc03789 by that method). The
  earlier figures (14 `MessageReader` and 35 `MessageWriter` sites,
  `Interaction` 86 times in 16 files) were measured before the hardware
  code by a narrower method that could not be reproduced; they stand as
  measured before. The press, key and pick sites only map input to actions
  or style hover (swept 2026-09-30; the hardware panel's `buttons`,
  `jog_buttons`, `keys`, `window_loss` and `sliders` only write actions).
- **One UI kit** (`src/ui_kit/`, see [UI kit](#ui-kit-2026-09-30)), done
  2026-09-30, verified at 4bc03789: tokens defined once (`ui_kit/theme.rs`),
  one widget builder (`Kit`: text, header, button in every `Look`, tab strip,
  chip, segment, section, property row, list item, text-entry styling, dock,
  scroll area, slider on `bevy_ui_widgets::Slider`, pointer surface, chart
  image and labels), one repaint for hover and state (`Look`, `Tint`), and
  accessible labels on interactive widgets. Every mode and the switcher
  build their common UI from it; the lib test
  `ui_kit::tests::ui_colours_come_from_the_kit` forbids `Tint` literals and
  token-equal `Color::srgb` literals outside the kit. One chart rasterizer
  (`chart::rasterize_span`).
- **`Node {` sites per file**, before (90c65c86) → after: `builder/ui.rs`
  82 → 64, `lesson/ui.rs` 78 → 76, `robot.rs` 48 → 28, `lesson/practice.rs`
  27 → 27, `builder/calibration.rs` 16 → 13, `lib.rs` 14 → 13,
  `lesson/narrate.rs` 13 → 12, `lesson/extras.rs` 11 → 11, `markdown.rs`
  10 → 10, `builder/gait_lab.rs` 10 → 10, `builder/schematic.rs` 9 → 9,
  `notes.rs` 6 → 2, `annotate.rs` 5 → 5, `physics_view.rs` 3 → 2,
  `builder/markers.rs` 3 → 3, `app/switcher.rs` 3 → 1, `place_view.rs`
  1 → 1; `ui_kit/` 27 (its widgets). What remains outside the kit is
  layout-only containers (rows, columns, gaps, the `layout` a dock or scroll
  area is given) and feature drawings (the schematic canvas, lesson cards,
  playheads, masks, sketch dots, markers, 3D labels, cards the kit has no
  widget for); see the UI kit section.
- **CAD mode** (§9 phase 1, see [CAD mode](#cad-mode-2026-09-30)), built,
  tested and verified at a4fe42d3 (sim-spatial lib tests 154 passed, 1
  ignored), pending the user's CAD checklist: `ViewerMode::Cad`, `CadPlugin` (`src/cad/`), a client of
  RoboCAD's REST service through `sim_runtime::cad_client`, which shares one
  loopback HTTP/1.1 transport (`sim_runtime::loopback_http`) with the
  hardware client. A `.rcad` starts RoboCAD's headless service as a
  `jobs::ChildProcess`; `--cad-url` attaches to a running RoboCAD. Tree,
  tessellated bodies, selection shared with RoboCAD's `/selection`,
  inspector with RoboCAD's labels as returned, attribute edits, delete,
  undo/redo, save, registry commands and Ops, every intent a `CadAction`.
  The ledger is [docs/cad-parity.md](../cad-parity.md) (773 rows), the
  side-by-side steps [docs/cad-checklist.md](../cad-checklist.md).
  Since **cad-select-transform** (2026-10-01, see
  [CAD selection and transform](#cad-selection-and-transform-2026-10-01)),
  written and reviewed by reading, pending its verification pass: face,
  edge, vertex and point selection with hover, box select, the Alt menu
  and the selection commands; the move/rotate/scale gizmo, push/pull and
  offset, measure, live dimensions, snapping and the numeric bar with
  unit expressions (`sim_runtime::units`); each commit one RoboCAD Ops
  call. `cad/` is 10,397 lines in 30 files (wc -l, 2026-10-01; largest
  `document.rs` 734, `transform/mod.rs` 737, `panel.rs` 687, `actions.rs`
  678), `sim-runtime/src/units.rs` 901 (tests included).
- **Phenomena mode and planar v2 robot files** (see
  [Fold in sim-app](#fold-in-sim-app-2026-09-30)), written 2026-09-30 and
  verified at 80b5997e (sim-spatial lib tests 172 passed, 1 ignored;
  `cargo check --workspace --all-targets --locked` clean): `ViewerMode::Phenomena`
  (`src/phenomena/`) runs `sim_phenomena::exhibits` on the "phenomena-run"
  `RunThread` with sim-app's pacing, `PhenomenaAction` and kit panels; robot
  mode opens planar v2 `*.simrobot.json` files through
  `cad_robot::build_planar` on the "robot-run (planar v2)" `RunThread`
  (`src/robot_planar.rs`). `crates/sim-app` is deleted; its ledger is
  [docs/sim-app-parity.md](../sim-app-parity.md) (58 rows, none open). Every
  child process starts in `jobs` (`spawn_detached`, `open_in_browser`,
  `ChildProcess`), enforced by `jobs::tests::processes_are_started_only_in_jobs`.
- **Hardware front end** (§8, see
  [Hardware front end](#hardware-front-end-2026-09-30)), done 2026-09-30
  pending the user's hardware checklist: the Leg calibration panel is a
  dock in Robot mode (`robot/hardware/`, built by `RobotPlugin`), opened by
  the header's "Leg calibration" button or `--hardware URL`. It talks to
  the unchanged `serve_actuator_calibration` and `serve_motor_bench` through
  one typed loopback client, `sim_runtime::hardware_client`. Every intent is
  a `HardwareAction`, and anything that starts, changes or arms motion is
  refused by name from REST and `system_ui`. STOP goes out on its own
  connection from a Dedicated job, never behind the link thread. Focus
  loss, panel close, leaving Robot mode and closing the window stop any
  drive (wider than the page's rule), as does the link's drop; closing the
  window or quitting also writes STOP synchronously. The heartbeats run on
  their own worker beside the link. Built and tested in the 2026-09-30
  verification pass (see the section's Verification pass). The panel also holds the leg mirror
  (blue-tinted suspended robot) and live motor sync. The feature-by-feature
  ledger is [docs/hardware-parity.md](../hardware-parity.md), and the
  operator's steps are [docs/hardware-checklist.md](../hardware-checklist.md).
- **Large files:** `robot_run.rs` (2,647 lines), `robot.rs` (2,514 after
  fold-sim-app's v2 branches; was 2,232), `builder.rs` (2,453) and
  `lesson/mod.rs` (2,371); fold-sim-app also left `robot/actions.rs` at 898
  and `robot_planar.rs` at 826 (tests included), past the 800-line smell,
  and `app/switch.rs` at 959. Phenomena mode's files are under 600 each. UI files after ui-kit:
  `builder/ui.rs` 1,395 (was 1,684), `lesson/ui.rs` 1,158, `lib.rs` 1,294
  (was 1,411), `app/switcher.rs` 84; `ui_kit/` 903 lines (`widgets.rs` 346,
  `theme.rs` 199, `tests.rs` 157, `slider.rs` 85, `mod.rs` 71, `scroll.rs`
  45). The action modules are
  under 710 lines each (`hardware/actions.rs` 709, `robot/actions.rs` 689,
  `lesson/actions.rs` 670, `builder/actions.rs` 661, `app/actions.rs` 513,
  `inspect.rs` 412, `builder/system_actions.rs` 395); `app/switch.rs` is
  850, `chart.rs` 183, `app/tests.rs` 364, `app/route.rs` 75, `rest.rs` 140.
  The hardware front end (wc -l, 2026-09-30): `robot/hardware/` 8,049 lines
  in 21 files (`sync.rs` 765, `session.rs` 733 plus `session/sequences.rs`
  381 and `session/buttons.rs` 147, `mirror.rs` 741, `actions.rs` 709,
  `view.rs` 705, `panel.rs` 630, `handlers.rs` 524, `sync_panel.rs` 346,
  `link.rs` 344, `panel_sections.rs` 291, `settings.rs` 285, `mod.rs` 222,
  `motion_view.rs` 205, `mirror_panel.rs` 187, `dial.rs` 145; tests
  `session/tests.rs` 267, `view/tests.rs` 156, `sync/tests.rs` 150,
  `mirror/tests.rs` 116) and `sim-runtime/src/hardware_client/` 2,175 lines
  (`tests.rs` 653, `calibration.rs` 613, `http.rs` 349, `mod.rs` 268,
  `bench.rs` 186, `token.rs` 106).
- **What already works well, to keep:**
  - typed actions with one validated handler per action type
  - generation-stamped frames
  - shared undo history
  - one `jobs` module for background work (pool jobs and `RunThread`)
  - the `system_ui` control registry and REST adapter (`sim_api`)
  - worker-computed results kept off the UI thread

## Bevy 0.19.1 migration (2026-09-30)

Batch bevy-0-19-upgrade moved `sim-spatial` and `sim-app` from 0.16.1 to 0.19.1
through the official 0.16→0.17→0.18→0.19 guides, with no intended feature change.

- **Features.** `sim-spatial`'s `default-features = false` list was rebuilt from
  0.19.1's cargo features: `bevy_camera`, `bevy_light`, `bevy_mesh`,
  `bevy_shader`, `bevy_material`, `bevy_ui_render` and `bevy_gizmos_render` are
  now separate; `mesh_picking` and `ui_picking` replace
  `bevy_mesh_picking_backend` / `bevy_ui_picking_backend`; `keyboard` and
  `mouse` are listed explicitly. The effective set matches 0.16: still no
  `tonemapping_luts`, `smaa_luts`, `hdr`, scenes, glTF, audio or gamepad.
  `sim-app` keeps Bevy's default features.
  - **Later addition (one-app-modes):** `bevy_state`, for the `ViewerMode`
    states (`StatesPlugin` comes with `DefaultPlugins`; `bevy_state` was
    already in `Cargo.lock` through `sim-app`).
  - **Later addition (ui-kit):** `bevy_ui_widgets` (it enables
    `bevy_input_focus`), for the kit's headless slider; `DefaultPlugins`
    then adds `UiWidgetsPlugins` and the input-focus plugins. `bevy_feathers`
    is not enabled (see [UI kit](#ui-kit-2026-09-30)). Both crates were
    already in `Cargo.lock`.
- **Pins.** `bevy = "0.19.1"` (workspace) and `=0.19.1` (`sim-spatial`).
  `image = "=0.25.10"` is unchanged (0.19.1 resolves to it). `objc2` 0.6 and
  `raw-window-handle` 0.6 became direct macOS dependencies of `sim-spatial`
  (already in the tree through winit/wgpu). No non-Bevy crate changed.
- **Silent upstream changes kept at 0.16 behaviour** (the compiler does not
  catch these):
  - UI nodes use `UiTransform` / `UiGlobalTransform`, not `Transform`: lesson
    block tracking, scroll-to, card viewports and narration overlays read
    `UiGlobalTransform`; leader and overlay lines rotate through `UiTransform`.
  - `RelativeCursorPosition::normalized` is centre-origin since 0.17;
    `view::cursor_fraction` gives the old 0..1 value.
  - Layout no longer writes the clamped `ScrollPosition` back;
    `view::clamp_scroll_positions` (PostUpdate, after `UiSystems::Layout`) does.
  - `system_ui` merges buttons that share an action and keeps the shortest
    label; ties are now broken by text, so the label no longer depends on ECS
    iteration order (restores "Detailed" for the Detailed/Realtime segment).
- **Forced choice: hidden windows.** wgpu 29 (gfx-rs/wgpu#8309) skips drawing a
  macOS window that is not visible: screen locked, minimized, fully covered or
  on another Space. A user sees no difference, but the REST `screenshot` then
  wrote an all-black PNG (0.16 kept drawing). `rest::Occlusion` reads the same
  `NSWindow.occlusionState` (winit sends no event for a window created hidden)
  and `screenshot` refuses with an error naming the cause. Unattended captures
  need an unlocked, visible window. Alternatives rejected: patching wgpu, or an
  offscreen render path for screenshots (new-feature work; revisit with the
  0.18 screenshot/recording API).
- **Accepted visual differences** (viewed side by side, baseline-016 vs
  upgraded-019):
  - text layout moved from Cosmic Text to Parley: line metrics differ by under a
    pixel, so clipped schematic node labels show slightly more of their next
    line, and the robot inspector's scroll range changed by ≤ 1 px
    (1440.0→1439.0, 241.5→241.0). Glyphs, sizes and wrapping are unchanged.
  - nothing else: panels, overlays (robot contact gizmos with the a5543970
    depth bias), lighting, colours and fonts match in build+schematic, robot
    FILE, embedded preset, recorded preset, lesson, place and `sim-app`
    phenomena/cad (v4, v3, v2).
- **Evidence.** `$PAIR_CAPTURES/bevy-019/`: `capture_modes.py` (one script for
  both runs), `baseline-016/` (0.16.1, before the pin), `upgraded-019/`,
  `compare.py` and `comparison.json` (ok: identical capabilities, `system_ui`
  ids and labels, state structure, recorded seek pose, masses, run poses and
  lesson ids). The sim-spatial lib tests are the same 65 (+1 ignored), passing.
- **Gait-lab fingerprint.** This edit to crate sources invalidates the gait-lab
  runtime fingerprint; requalification is deferred to its own batch.

## Jobs module (2026-09-30)

Batch jobs-module moved all 35 hand-rolled thread sites onto `src/jobs/`, with
no intended user-visible change (same REST commands, `system_ui` ids, labels,
result formats and state fields).

- **Where each site went.**
  - `Pool::Io`: source previews (`builder/reference`), the actuator registry,
    the identification archive, gait-lab result scans, the recording lister
    (`Latest`), the stress results reader, the recording writer and the
    placement commit (both `complete_on_drop`).
  - `Pool::Compute`: the scene compile, Open system's load, the schematic
    layout (its cancel token is `lay_out`'s; a newer key drops the job), the
    agent's model context, lesson figures, the robot preset load and the
    `--robot FILE` reload check.
  - `Pool::Dedicated`: narration generation (progress lines as the job's
    message; paid work is never cancelled mid-request, as before), the lab
    bench request, the lesson model (streamed values), scene recordings
    (streamed stages, fraction progress), comparisons and studies (steps
    progress), run replays.
  - `RunThread`: the builder run session (`LiveRun::spawn`, one constructor
    for what were four spawns), `robot-run`, `robot-gait`, `robot-recorded`
    (its `PlaybackState` is `Stamped`), since fold-sim-app "phenomena-run"
    and "robot-run (planar v2)", and the placement validator (pointer
    moves are commands; the worker drains the channel and validates only the
    newest position; a closed channel never starts a queued one).
  - `ChildProcess` (`jobs/child.rs`, added by cad-mode): owns a child
    process the viewer started (RoboCAD's headless service): `spawn`,
    `id`, `name`, `exited` (non-blocking), `stop` (kill, then reaped on a
    reaper thread; never blocks the caller), `detach` (left running,
    reaped when it ends) and `Drop` = `stop`. Only processes the viewer
    spawned are ever owned, so only self-started services are stopped.
  - Helpers: `reap_child` for the linked `sim-viewer` (two sites in
    `main.rs`) and, found by reading, for the `open`/`xdg-open` process of a
    web source link, which was never waited for; `drop_off_thread` for the
    builder replaced by Open system. *Since fold-sim-app:* those sites use
    `jobs::spawn_detached` and `jobs::open_in_browser` (with the lesson's
    `open_url`), and `reap_child` is private to `jobs`.
- **Behaviour that changed** (all in failure paths):
  - A panic in a job used to leave its feature waiting forever (compile,
    figures, lesson model, studies) or report "… ended without a result". It
    now reports "{name} ended without a result ({panic})." through the
    feature's usual error field (a compile panic shows as the compile error).
  - Dropping a replaced `RunThread` now waits up to 200 ms for it to exit
    instead of not at all.
- **Tests** (`jobs/tests.rs`, no window): both pools run without an `App`; a
  stale generation is dropped (`Latest`); cancel is observed explicitly, on
  drop, and before start (the closure never runs); errors and panics are
  surfaced; progress and streamed updates are visible while running; a
  `complete_on_drop` write finishes after its handle is dropped; `RunThread`
  delivers stamped snapshots, joins on drop, and bounds the wait for a busy
  worker; the source guard. The placement validator tests now use
  `RunThread` (latest position wins; drop returns promptly and starts no
  queued work).
- **Parity, traced by reading** (screenshots are off for this run; the
  verification pass builds and tests). "Unchanged" means the poll/apply code
  and the state field are the same as before 44a08bd8; only the thread start,
  channel and cancel/generation plumbing moved into `jobs`. Paths are
  `crates/sim-spatial/src/`, lines as of the jobs-module follow-up.
  - *Build mode*
    - Schematic re-layout: start `builder/schematic.rs:204` (Compute, stamped
      with the key's revision, cancel token passed to `lay_out`); apply
      `SchematicState::tick` `schematic.rs:166`, called every frame from
      `schematic.rs:255`. A newer revision or level drops the job
      (cancel on drop, `superseded += 1`). Feeds `system_state.schematic`
      (`laid_out`, `pending`, `stale`, `error`) and the panel status.
      Unchanged.
    - Compile (feeds the schematic and the scene): start `builder.rs:341`
      (Compute); apply `builder.rs:2386`. Changed only on a panic, which now
      sets `compile_error` and the status (it used to wait forever).
    - Open system: start `builder/open.rs:164` (Compute, generation = open
      seq); apply `finish_open` `open.rs:181` from the `open_system` system
      `builder.rs:1779`. The previous builder is dropped by
      `jobs::drop_off_thread` at `open.rs:264`, so its run thread and agent
      are joined off the UI thread. Feeds `system_state.open.last` and the
      status line. Unchanged.
    - Live run start/pause/step/reset: `LiveRun::spawn` `builder.rs:156`
      (RunThread "builder-run"), started from `start_run` `builder.rs:1063`.
      Commands `builder.rs:1041` (Start), 1069 (Pause), 1334 (Step) and 1360
      (Reset); the loop at `builder.rs:1447` is unchanged. Read by
      `live_run_json` `builder.rs:1425` and `running()` from the shared
      snapshot. A replaced run drops its RunThread (bounded 200 ms join).
    - Run replay: start `builder.rs:977` (Dedicated, `replay_with_cancel`
      given the job's cancel flag); apply `poll_replay` `builder.rs:1002`
      (called at `builder.rs:2584`); cancel `builder.rs:990`. Feeds
      `system_state.replay` via `replay_json` `builder.rs:1026`. Unchanged.
    - Study progress and cancel: start `builder.rs:1132` (Dedicated, steps
      progress); apply `poll_study` `builder.rs:1161` (at `builder.rs:2583`);
      progress `study_progress` `builder.rs:1157` into
      `system_study_result.running` and `study_json`; Cancel
      `builder.rs:2066` calls `Job::cancel` (fixed in 4b74edc6; it used the
      removed flag and did not compile).
    - Actuators tab: start `builder/actuators.rs:178` (Io, generation = seq);
      apply `finish_actuators` `actuators.rs:194` (system at
      `builder.rs:1788`). Measured evidence: start `builder/calibration.rs:350`
      (Io); apply `finish_calibration` `calibration.rs:366`
      (`builder.rs:1795`). Gait lab: start `builder/gait_lab.rs:146` (Io);
      apply `finish_gait_reports` `gait_lab.rs:165` (`builder.rs:1802`).
      They feed `system_state.actuators`, `calibration_review` and
      `gait_reports` (`last` = `(seq, result)`). Unchanged.
    - Source preview and agent context: `builder/reference.rs:31` (Io; a
      web link's `open` process is reaped with `jobs::reap_child`,
      `reference.rs:24`; since fold-sim-app `jobs::open_in_browser`) and `builder/agent.rs:159` (Compute, polled at
      `agent.rs:172`). Unchanged apart from the reaping.
  - *Placement*
    - Drag validator: `builder/placement_worker.rs:15` (RunThread
      "placement-validator", join bound 0). Pointer moves `submit` at
      `builder/placement.rs:533`; the result is taken at `placement.rs:536`.
      The worker drains the channel and validates only the newest position
      (the condvar became channel commands; latest-wins is kept, and a
      closed channel starts nothing). Changed in mechanism only.
    - Commit on release: `placement.rs:593` (Io, `complete_on_drop`); apply
      `poll_drop` `placement.rs:600` (at `placement.rs:503`). Unchanged.
  - *Robot*
    - Preset open and preset switch: `RobotView::open_preset` `robot.rs:216`
      starts the loader at `robot.rs:222` (Compute); apply `receive`
      `robot.rs:1183` (`load.poll()` at `robot.rs:1201`). A switch
      (`Request::RobotPreset`, `robot.rs:736`) replaces the whole view, so
      the old run, gait and playback RunThreads drop with a bounded join.
      Feeds `robot_state.status`, `preset` and `load_seconds`. Unchanged.
    - Recording save: command `robot_run.rs:802`; the run thread snapshots
      and hands the write to `robot_run.rs:1777` (Io, `complete_on_drop`,
      publishes `Published.save`); apply `RunController::poll`
      `robot_run.rs:1035`. Feeds `robot_state.recording.pending/last_saved`.
      Changed: a writer panic is now published as the save error (before
      and after the migration it was lost and `pending` never cleared).
    - Recording list: `Latest` at `robot_run.rs:831` (Io; a newer list
      supersedes); apply `robot_run.rs:1054`. Feeds `robot_state.recordings`.
      Unchanged.
    - Replay: runs on the robot-run RunThread (`Command::Replay`, loop from
      `robot_run.rs:1789`), published as `Published.replay` and accepted in
      `poll` only for the current generation and seq. Unchanged.
    - `--robot FILE` reload: `robot_source.rs:155` (Compute, sha256 and
      parse); taken by `receive` via `SourceWatch::take` (`robot.rs:1209`); a
      loaded reload replaces the run with generation + 1. Unchanged.
    - Gait preview: RunThread "robot-gait" `robot_gait.rs:171`; apply `poll`
      `robot_gait.rs:269` (the listing is `Shared.listing` with its seq).
      Recorded seek and play: RunThread "robot-recorded"
      `robot_playback.rs:166`; apply `poll` `robot_playback.rs:182` via
      `RunThread::latest(generation)`. Feed `robot_state.gait_preview` and
      `recorded`. Unchanged loops.
    - Stress results reader: `robot_stress.rs:108` (Io), polled at
      `robot_stress.rs:115`. Unchanged.
    - `sim-viewer` children: `main.rs:241` and `main.rs:392` use
      `jobs::reap_child` (since fold-sim-app `jobs::spawn_detached`).
  - *Lessons*
    - Figures: `lesson/practice.rs:26` (Compute); apply `poll_figures`
      `practice.rs:56` (at `lesson/mod.rs:1325`). Unchanged.
    - Model: `lesson/extras.rs:67` (Dedicated, streamed values); apply
      `poll_model` `extras.rs:97` (at `lesson/mod.rs:1342`). Feeds
      `lesson_state.model`. A panic now shows under `model.errors`.
    - Comparisons: `lesson/mod.rs:1194` (Dedicated, steps progress); apply
      `lesson/mod.rs:1331`; "Running n/m…" from `lesson/ui.rs:773`. Feeds
      `lesson_state.compares`. Unchanged.
    - Scene recordings: `lesson/mod.rs:665` (first) and `mod.rs:722`
      (re-record), both Dedicated and streaming `Stage`s; apply from
      `lesson/mod.rs:1363`; progress `ActiveScene::progress` `mod.rs:303`
      into `lesson_state.scene.recording` and the "Recording on the shared
      runtime… n %" line (`lesson/ui.rs:1130`). Changed: a panic in the job
      now ends the recording with its error instead of leaving the scene
      "recording".
    - Narration progress (never run generation here): `lesson/narrate.rs:261`
      (Dedicated; paid work is not cancelled mid-request); apply
      `narrate.rs:379`; the line is `GenJob::progress` `narrate.rs:48`
      ("Starting…" until the first message) and `lesson_state.narration.job`
      (`lesson/actions.rs` `narration_state`). Unchanged text.
    - Practice bench request: `lesson/extras.rs:207` (Dedicated), polled at
      `extras.rs:141`. Unchanged.
- **What the build/test pass must run** (no screenshots):
  - `cargo build -p sim-spatial --lib --tests --bins` with no sim-spatial
    warnings (an unused `mpsc`/`Mutex`/`Arc`/`AtomicBool` import would show
    here);
  - `cargo check -p sim-app` (dropped since fold-sim-app: the crate is retired);
  - `cargo test -p sim-spatial --lib`, in particular `jobs::tests::*`
    (including `threads_are_started_only_in_jobs`),
    `builder::placement_worker::tests`, `builder::placement::tests`,
    `builder::replay_tests` (the run, step/reset, grab swap and realtime
    runs on `LiveRun::spawn`), `builder::open::tests`,
    `builder::calibration::tests`, `builder::schematic::tests` and
    `robot_run::tests`.
- **Verification.** Verified at ae80a137: `cargo check --workspace
  --all-targets` with no errors, the sim-spatial bins built with no
  sim-spatial warnings, and `cargo test -p sim-spatial --lib` gave 75 passed,
  1 ignored (the existing `measure_full_robot_preset` benchmark), including
  every jobs test and the source guard.

## One app (2026-09-30)

Batch one-app-modes replaced the five per-mode `App` builders (`run_builder`,
`run_lessons`, `run_with_api` with `run`, `robot::run_robot`,
`place_view::run_place`) and the two REST servers (`physical-assembly` and
`robot`) with one app, `ViewerMode` states, one mode switch and one server.
Paths below are `crates/sim-spatial/src/`.

- **Shape.**
  - `app/mod.rs`: `run(Launch)` (the one builder), `ViewerMode`, the
    computed states `ModeScope` (entity and resource lifetime: Inspect,
    Builder = Build + Lessons, Robot, Place; Cad since cad-mode) and `SpatialScreen` (the
    spatial view is drawn: Inspect, Build, Lessons), `ViewerSet`,
    `CorePlugin` (window, per-mode look, fonts, mesh picking, occlusion,
    scroll clamp, REST wake, the switcher) and `ModesPlugin` (states, sets,
    the switch; no window, so the test runs it).
  - `app/switch.rs`: `ModeSwitch`, `Switcher`, `Documents`, the handler
    (`handle`, Actions), document loads (`finish_load`, JobResults),
    `arrive` (every mode's OnEnter) and the scopes' OnExit teardown.
  - `app/route.rs`: the one REST dispatch (`route`, `annotate`); since the
    action layer the shared commands are `switch::WindowAction`'s.
  - `app/switcher.rs`: the mode switcher's buttons.
  - Mode plugins: `SpatialViewerPlugin` (setup on OnEnter of the Inspect
    and Builder scopes; chains under `SpatialScreen`), `BuilderPlugin`
    (Builder scope; its Build-only systems under `in_state(Build)`),
    `LearnPlugin` (Builder scope while a `Learn` exists), `RobotPlugin`,
    `PlacePlugin` (their scopes and modes).
- **Where each chain sits** (internal order unchanged): the spatial frame
  chain (`notes::update` … `animation::draw_markers`), the builder chain
  (still `.before(update_parts)`) and its placement/markers/`ui_api`
  systems, the lesson chains (still `.before(camera_viewport)` and
  `.after(ui::rebuild)`), robot's frame chain (`watch` … `highlight`) and
  place's (`poll_rest`, `fly`, `toggles`) are in **SimSync**; the spatial
  drawing chain (`view::animate` … `view::caption_fonts`) and robot's
  panels chain (`panels` … `draw`, still after its frame chain) are in
  **Present**. Shared: the switcher's clicks and the lesson screen's
  requests in **Input**, the switch handler in **Actions**, its document
  loads in **JobResults**, the switcher's highlight and `/v1/viewer_mode` in
  **Present**. (Superseded by the action layer, which moved every REST
  poll, button and key mapping to Input and their handlers to Actions; see
  [Action layer](#action-layer-2026-09-30).)
- **`building()` / `Learn.active`.** `building()` is gone: its uses are
  `in_state(ViewerMode::Build)`; `clear_for_learn` runs in Lessons; the
  lesson checks in `pick_part`, `keyboard`, `placement::start_part` and
  `rebuild_scene` read the state. `Learn.active` remains as the lesson
  page's own flag, read by the lesson systems (13 reads in `lesson/`),
  because those systems keep running while the builder is shown over a
  lesson (recordings, narration and the lesson model keep going, and the
  page hides itself). It is written only by `Learn::show`, which only
  OnEnter/OnExit(Lessons) call, so it always matches the state. The lesson
  and builder toggles ("Open in builder", "‹ lesson", `lesson_screen`,
  `lesson_open`) call `Learn::request_screen`, a switch request.
- **Decisions.**
  - *Winit per mode.* Reactive (1/60 s focused, low power 40 ms unfocused)
    stays the shared default; each mode's OnEnter sets its winit setting,
    so Place's continuous update while focused (100 ms unfocused) is scoped
    to Place and replaced on entering any other mode. The same OnEnter sets
    the title, clear colour, ambient light and minimum size. The window
    size is the initial mode's; a switch does not resize the window.
  - *The builder stays across modes.* Build ↔ Lessons keep it (and the
    lesson); leaving both for another mode keeps it too, paused (no physics
    runs unseen), its scene parked in `Documents`; on return its chrome is
    rebuilt and its scene recompiled and respawned (reference images are
    drawn only by `rebuild_scene`). Leaving Build/Lessons removes the lesson (its jobs cancel) and
    resets the lesson's text size (`UiScale`). A new lessons folder replaces
    the builder with the lesson's sandbox builder, as a lessons launch does
    (refused on the builder's blockers). `viewer_mode build {path}` while
    the builder has another file open is refused, pointing at `system_open`
    (the one open path, which keeps a live run); a lesson's sandbox builder
    (no Open) outside lessons is replaced by the file.
  - *Teardown.* Mode entities carry `DespawnOnExit<ModeScope>`, inserted by
    one sweep in `Last` (`app::scope_new_entities`: every root `Node` or
    `Transform` not marked `Persistent`) rather than at each of the modes'
    spawn sites; children go with their root. The robot view, a place and a
    lesson are removed on exit and dropped off the UI thread
    (`jobs::drop_off_thread`), so their jobs cancel and their `RunThread`s
    join within `JOIN_BOUND` there. `ModelLibrary`, `UiFonts`, `Documents`,
    `Rest` and the workspace root always survive. Inspect's scene and
    selection link are parked and come back as they were.
  - *Documents.* A switch reopens what the mode last showed (or the
    launch's); a `path` or robot `preset` names a new one. Loads use the
    launch's loaders off the UI thread: `builder::open::open_build`,
    `lesson::open_lessons`, `load_inspect`, `PlaceView::open` (Compute
    jobs), and the robot view's own loader, waited for through
    `RobotView::opened` so a robot that fails to load leaves the current
    mode. Refusals name the reason; the handler re-checks the blockers when
    a load finishes (as `finish_open` does).
  - *Blockers.* Leaving Build/Lessons (or replacing the builder) uses
    `Builder::switch_blockers`: open.rs's `system_open` blockers (drafts,
    drag, study, replay, Codex) plus a pending open; a lesson draft or
    contact sheet; leaving Robot, a recording being written or a replay. A
    new lesson that would replace a build-mode builder is also refused while
    that builder has a live run (`Builder::replace_blockers`: `system_open`
    saves a live run before replacing a builder, a mode switch cannot).
    Build → Lessons is refused on the builder's blockers and pauses a live
    run (see *Build → Lessons* below); Lessons → Build is never blocked.
  - *The switcher.* A row of the robot header's buttons in the bottom-right
    corner of every mode, with the last outcome above it: every mode's top
    edge is full (toolbars, run controls). Not visually checked (screenshots
    are off for this run).
  - *Capability mode tags.* Each capability gains a `modes` array (added in
    `rest::capabilities`; `sim_api` is unchanged). A name may appear more
    than once with different modes and arguments (`system_ui`: builder,
    robot, and the mode-only one for Inspect and Place; `camera`: the orbit
    for the spatial modes and robot, the fly camera for Place; `state`).
    The router's table is built from the same list, so a command's modes
    cannot drift from what capabilities says.
  - *Server kind.* `sim-spatial` (was `physical-assembly` or `robot`); no
    client reads it (ui_capture polls `/v1/capabilities`; simbridge only
    launches `--robot FILE`).
  - *Place REST surface.* `state` (dir, description, stations, views,
    station, camera pose and speed, help and marker visibility), `camera`
    (position, yaw, pitch, speed, station), `screenshot` (the shared
    Occlusion refusal), `viewer_mode` and `system_ui` `mode:*`.
  - *Headless.* `--headless` binds through the same `rest::bind` and serves
    inspect mode only; `viewer_mode` and `mode:*` answer that there is no
    window to switch.
  - *Found by reading.* Robot mode never added `MeshPickingPlugin`, so a
    click on a link in the 3D view could not select it (robot.rs
    `pick_link`); the core now adds mesh picking for every mode. Inspect's
    captions and physics labels now use the interface fonts (the fonts are
    loaded once for every mode).
- **Launch path, traced by reading** (main.rs → OnEnter). Every mode:
  `main` resolves the flags and calls `open_window` (main.rs:160), which
  binds `rest::bind` and calls `app::run` with a `Launch` (the initial
  mode, its documents, `Documents` with the launch facts, the shared
  `ModelLibrary`). `run` (app/mod.rs:158) adds `CorePlugin` (window and
  look of the initial mode; fonts loaded before the first OnEnter, which
  runs ahead of Startup), inserts the documents, then `ModesPlugin`
  (`insert_state(initial)`) and the mode plugins. At startup the
  `StateTransition` schedule runs OnEnter(`ViewerMode`) (`apply_look`,
  `arrive`: nothing to install at launch) and then OnEnter(`ModeScope`):
  - Inspect: main.rs tail (`load_inspect`, flags, `set_compact`, link) →
    OnEnter(Inspect scope) `setup_scene` + `setup_ui` (lib.rs:336).
  - Build: `build_mode` (main.rs:235: `Builder::open`,
    `builder::compiled_scene`, annotations, models, `enable_open`) →
    OnEnter(Builder scope) `setup_scene` (lib.rs:337); the builder's chrome
    is built by `ui::rebuild_panel` in Build.
  - Lessons: `lessons_mode` (main.rs:173: `lesson::open_lessons`) →
    OnEnter(Lessons) `arrive` then `show_lessons` (a no-op: a new `Learn`
    is already shown) → OnEnter(Builder scope) `setup_scene`.
  - Robot: `robot_mode` / `robot_preset_mode` (main.rs:195/208:
    `RobotView::open` / `open_preset`, loading on their jobs as before) →
    OnEnter(Robot scope) `robot::setup` (robot.rs:1021).
  - Place: main.rs:303 `PlaceView::open` (read before the window opens, so
    a bad directory still fails the launch) → OnEnter(Place scope)
    `place_view::setup` (place_view.rs:135).
- **A switch, traced by reading.** Switcher click (`switcher_clicks`,
  Input), `system_ui` `mode:*` or REST `viewer_mode` (`route::route` in the
  active mode's poll) → `Switcher::submit` → `switch::handle` (Actions):
  blockers, then `prepare` (the document now, or a load) → `finish_load`
  (JobResults) → `enter`: `NextState` → next frame's `StateTransition`:
  OnExit of the old scope (despawn, park or remove) → OnEnter(new mode)
  `apply_look`, `arrive` → OnEnter(new scope) setup → `handle` confirms the
  outcome, which a pending REST job then returns.
- **What the verification pass must run** (no screenshots):
  - `cargo build -p sim-spatial --lib --tests --bins`, with no sim-spatial
    warnings;
  - `cargo check -p sim-app` (dropped since fold-sim-app: the crate is retired);
  - `cargo test -p sim-spatial --lib`, in particular
    `app::tests::build_robot_build_tears_down_the_robot_and_keeps_shared_state`
    and `app::tests::rest_refuses_commands_of_another_mode_by_name` (new),
    the jobs tests (`jobs::tests::*`, including
    `threads_are_started_only_in_jobs`), `builder::open::tests`,
    `builder::placement::tests`, `builder::placement_worker::tests`,
    `builder::ui_api::tests`, `builder::replay_tests`, `robot_run::tests`,
    `rest::tests` and the lib.rs `keyboard`/`pick_part` test;
  - a reading check of the launch trace above against the built code.
- **Verified at 7da1216e** (see "Where it is today"): built and tested as
  listed above. The switcher's placement over each mode's panels, the robot
  link picking the core now enables, and a live switch between every pair
  of modes in a window stay unverified until screenshots are on.

## Action layer (2026-09-30)

Batch action-layer put every user intent in the five modes onto typed
actions with one handler each, generated the REST capabilities from one
registry, and made entering Lessons from Build go through the mode switch's
validation. No REST command name, argument shape, result, `system_ui` id,
label or keybinding changed, apart from the Build → Lessons fix below.
Paths are `crates/sim-spatial/src/`.

- **Shape.**
  - `app/actions.rs`: the `Action` trait (an action type's REST commands
    as `Spec`s: name, modes, example args, description, next to its
    variants; its `system_ui` control ids as patterns; its REST `parse`,
    serde by default), the transport (`Act<A>`, a Bevy Message, with an
    `Origin`: `Rest(Reply)`, `Ui` or `Quiet`), reply tokens (`Replies`),
    `InFlight<A>` and `apply` (the body of every apply system), the
    registry (`registry`, `capabilities`, `command_modes`, `feature_for`,
    `fallback`), the one REST poll `serve` and `variants` (the commands an
    action type's serde form accepts, read from serde itself).
  - Action types, their apply systems (all in `ViewerSet::Actions`) and
    where they are registered:

    | Mode | Action type | Apply system | Registered in |
    |---|---|---|---|
    | every mode | `app::switch::WindowAction` (`viewer_mode`, `screenshot`, switcher `system_ui`, `Switch`) | `app::switch::handle` | `app::switch::build` |
    | Inspect (and the spatial view of Build, Lessons) | `inspect::InspectAction` | `inspect::apply` | `SpatialViewerPlugin` |
    | Build (and Lessons) | `builder::system_actions::SystemAction` (the `system_*` commands; `Ui(BuildAction)` for the chrome) | `builder::system_actions::apply` | `BuilderPlugin` |
    | Lessons (and Build over a lesson) | `lesson::actions::LessonCommand` (the `lesson_*` commands; `Ui(LessonAction)` for the page) | `lesson::actions::apply` | `LearnPlugin` |
    | Robot | `robot::RobotAction` (`robot/actions.rs`) | `robot::actions::apply` | `RobotPlugin` |
    | Place | `place_view::PlaceAction` | `place_view::apply` | `PlacePlugin` |

  - Input mappings (Input): `app::switcher::switcher_clicks`,
    `app::switch::lesson_screen_requests`, `inspect::input`,
    `notes::clicks`, `physics_view::overlay_clicks`,
    `builder::actions::{buttons, keys}`, `lesson::actions::{buttons,
    keys}`, the lesson timebar, slider and narration bar (`lesson::seek`,
    `lesson::sliders`, `narrate::seek`), `robot::actions::{buttons,
    motion_keys, graph_key, overlay_keys, speed_keys}`, `place_view::keys`,
    and the pick observers (`lib.rs` `pick_part`, `linked::pick_net`,
    `builder::pick_reference`, `robot::actions::pick_link`). The builder's
    `text_input` (SimSync) edits the draft and writes its Enter/Escape as
    `BuildAction::SubmitDraft`/`DropDraft`; `robot::watch` (SimSync) writes
    the file watch's Reload. Continuous gestures stay where they were
    (orbit and fly cameras, wheel scroll, placement drags, sketch strokes)
    because they are navigation or input editing.
- **Deleted.** `rest::capabilities`, `rest.rs` `Command`, `SystemRequest`,
  `system_execute`, `execute`, `tick` and `poll`; `route::capabilities`,
  `mode_ui_capability`, `Modes`, `rest_switch` and the capability-JSON
  parse in `command_modes`; `robot::capabilities`, `Request`, `UiRequest`,
  `execute` and `poll_rest` (merged into `RobotAction` and its REST form
  `robot::actions::wire`); `place_view::capabilities`, `Request`,
  `poll_rest` and `toggles`; `lesson/rest.rs` (`capabilities`, `Request`,
  `NotesRequest`: now `LessonCommand` and `NoteOperation` in
  `lesson/actions.rs`); `lib.rs` `Action`, `dispatch` and `keyboard`;
  `builder/ui_api.rs` `Request` (now `system_actions::UiAction`);
  `builder_buttons`, `builder_keys`; `Switcher::submit`, `outcome`,
  `waiting`, `cancel` and its queue (generalized into `Replies`).
- **Decisions.**
  - *Messages, not observer triggers.* Each action type has exactly one
    consumer that must run once per frame after every input mapping and
    before job results and the scene sync; a Message drained by that
    system in `ViewerSet::Actions` gives exactly that, keeps the frame's
    order of actions and needs no entity. Rejected: `commands.trigger`
    with a global observer per action type (runs at the writer's command
    flush, so a REST action would apply in Input, outside the Actions
    set, and ordering against the frame chains is lost).
  - *Reply tokens generalize the switcher.* `Replies` is the switcher's
    submit/outcome/waiting/cancel with one slot per REST command: the poll
    (`Replies::submit`) parses, writes the action with `Origin::Rest`, keeps
    `{"reply": token}` in the `sim_api` continuation and answers Pending
    until the handler has answered. `sim_api` is unchanged. Rejected: owned
    requests with reply channels in `sim_api` (an API change).
  - *Asynchronous work.* A handler that answers Pending (system_open,
    studies, context builds, actuators, calibration and gait-lab scans,
    renders, annotation edits, lesson_frames, a lesson command waiting for
    the mode switch) is kept in `InFlight<A>` with its own continuation and
    applied again every frame until it answers; a REST cancel reaches it as
    `Call::cancelled`, which each handler already honoured. A mode switch
    that is still loading its document is dropped (its job cancels) when
    its REST caller cancels. A click's Pending work continues in the
    feature's own jobs, as before; only REST callers wait.
  - *REST forms stay serde.* Every REST variant keeps its tag,
    `rename_all`, `deny_unknown_fields` and defaults. Commands whose
    arguments were read loosely before (`system_open`, `system_context`,
    the tab loaders, `viewer_mode`, `screenshot`, `system_ui` in inspect
    and place, `lesson_frames`) carry their argument object and are read by
    the handler as before, so their error texts are unchanged. Robot's
    commands deserialize through `wire::Command` into `RobotAction`
    (`try_from`), keeping every argument error text.
  - *The builder keeps its handlers.* `SystemAction` wraps
    `Builder::apply` (validated commands, shared undo), the
    `expected_revision` checks and `dispatch` (the chrome's handler, moved
    to `builder/actions.rs`) exactly as they were; nothing is re-checked.
    The builder is two action modules (`actions.rs`: the chrome;
    `system_actions.rs`: the REST commands), not one per tab: the chrome's
    `BuildAction` serialization names every `system_ui` control id
    (`control-<hash>`), so splitting it would change ids.
  - *Build → Lessons (the one behaviour change).* Every entry to Lessons
    from Build is a `WindowAction::Switch` validated by `switch::handle`:
    the switcher, the builder's Lessons button (`builder/actions.rs`
    `buttons`), `system_ui` `mode:lessons`, `viewer_mode`, and
    `lesson_open` / `lesson_screen {learn: true}` in build mode, which wait
    for the switch's answer (`switch::ask_switch` / `awaited_switch`;
    `lesson_open` opens its lesson only once the switch is accepted). It is
    refused on `Builder::switch_blockers` (drafts, drag, study, replay,
    Codex answer, pending open; `switch.rs` `leaving_blockers`), or
    `replace_blockers` when a new lessons folder replaces the builder. A
    live run is paused and kept (`Builder::pause_for_learn`, called from
    `show_lessons`; `sync_run` leaves the scene to the lesson meanwhile);
    Run resumes it back in build mode. `stop_for_learn` is gone. Rejected:
    saving and dropping the run (loses the live session for no reason), or
    a second blocker list. Limitation, unchanged from before: activating
    another lesson scene installs that scene's sandbox builder, replacing
    the builder (and a paused run of the previous sandbox).
  - *Order within a frame.* Actions now apply before each mode's SimSync
    chain instead of inside it. Robot: REST, keys and buttons apply before
    `watch`/`receive` (they ran after them), so an action in the frame a
    load finishes sees the view before the load (the next frame sees it);
    the file watch's Reload is written in SimSync and applied next frame.
    Place: station keys apply before `fly`. Inspect: actions apply before
    `notes::update`. Builder and lessons: before `text_input` and the lesson
    poll. Nothing reads state that an action of the same frame changes
    later in the frame, except these one-frame shifts.
  - *Error texts changed only in edge cases:* a malformed `lesson_*`
    command with no lesson open now reports its parse error before "no
    lessons are open"; a command whose mode handler is gone reports that
    rather than hanging.
  - *Control ids.* Dynamic ids are registered as patterns
    (`link:<index>`, `control-<hash>`, …, matched by
    `actions::control_matches`); the tests check every listed id against
    them.
- **Completion pass (same day).** Found by reading and fixed:
  - the lesson timebar, sliders and narration bar mutated `Learn`
    directly; they now write `SeekTo`/`Slider`/`Narrate(Seek)` and REST
    `lesson_scene` seek/slider map to the same actions;
  - `pick_part` (Build, Inspect, Lessons), `pick_net`, `pick_reference`
    and the builder draft's Enter/Escape changed state in the observer or
    SimSync; they now write `BuildAction::PickPart`/`ReferencePoint`/
    `SubmitDraft`/`DropDraft`, `InspectAction::Select`/`Display`,
    `LessonAction::Pick`;
  - `system_ui` activate of the "‹ lesson" button answered success and did
    nothing (as before 152e02de); it now writes the button's
    `WindowAction::Switch` to Lessons (`switch::ask_switch`, shared with
    `lesson_open`/`lesson_screen`) and answers with the switch's result, a
    refusal naming the blocker; with no lesson open it is an error (the
    button does nothing then);
  - `system_open`, `system_gait_reports`, `system_calibration_review` and
    `system_actuators` with `args: null` were rejected ("args must be an
    object") where they used to read `{}`; restored
    (`SystemAction::parse`);
  - a `render` left Pending across a scope change captured the new scene;
    it now answers "render dropped";
  - a cancelled `lesson_open`/`lesson_screen` waiting for the switch leaked
    its nested reply slot;
  - `serve` could leave a command Pending forever if the mode changed and
    the same name belonged to another action type (latent); the
    continuation now records the owning feature;
  - `app/tests.rs` called `unwrap_err` on a `Result<&Feature, _>`
    (`Feature` is not `Debug`);
  - a lesson chart click kept its moment outside the action layer; it now
    writes `LessonAction::KeepMoment` once per press, with the moment held
    when the press ends (no pause and no page rebuild, as the click did
    before);
  - `lesson_open` in build mode switched to Lessons before checking the
    lesson loads; a lesson that fails to load is now refused first, with
    the loader's error, and the window stays in Build;
  - an unknown `system*`/`lesson_*` command in inspect mode (and headless)
    answered serde's unknown-variant list; it answers the old "start the
    viewer with --system FILE…" / "no lessons are open…" again.
  - Escape in a builder draft now goes through the Cancel button's handler
    (`discussion::Action::CancelDraft`, also REST `cancel_input`), which also
    ends a comment edit; before it only dropped the text, which could leave a
    stale edit that refused a later thread-title rename.
  - REST texts that still differ only in failures: a command's argument
    error now comes before "no builder"/"no lessons are open"; non-object
    args to `system_context`/`lesson_frames` read "args must be an object";
    a `"command"` key inside the args of `screenshot`, inspect/place
    `system_ui` or the loosely read `system_*` commands is refused ("args
    cannot override command"); the unknown-variant lists name the merged
    commands (`system_open`, …, `lesson_frames`); `lesson_open`/
    `lesson_screen {learn: true}` in build mode now wait for the switch and
    report its refusal.
  - Capability audit against 7da1216e: 92 entries before and after, same
    order, names, modes, examples and descriptions byte-identical except
    the intended `viewer_mode` text; every merged enum keeps its serde
    attributes.
  - Left as found: `notes::update` calls `notes::sync` every frame, which
    marks `SpatialScene` and `Orbit` changed every frame (pre-existing; some
    systems may rely on it).
- **Tests** (lib, no window): `app::tests::
  every_capability_parses_into_its_action_and_every_parsed_command_is_registered`,
  `every_mode_control_resolves_to_a_switch`,
  `rest_refuses_commands_of_another_mode_by_name` (now through the
  registry), `entering_lessons_from_build_refuses_on_a_draft_and_keeps_a_live_run`,
  `build_robot_build_tears_down_the_robot_and_keeps_shared_state` (on reply
  tokens), `pending_actions_are_carried_until_they_answer_and_a_cancel_reaches_them`; `robot::actions::tests::every_listed_control_fits_a_registered_pattern`
  and `rest_argument_errors_are_unchanged`; `builder::ui_api::tests` checks
  the collected id against the builder's pattern; `inspect::tests` (moved
  from `rest::tests`).
- **What the verification pass must run** (no screenshots):
  - `cargo build -p sim-spatial --lib --tests --bins` with no sim-spatial
    warnings;
  - `cargo test -p sim-spatial --lib` (the tests above, `jobs::tests::*`
    including `threads_are_started_only_in_jobs`, `builder::*::tests`,
    `robot_run::tests`, the lib.rs pick/button test);
  - `cargo check -p sim-app` (dropped since fold-sim-app: the crate is retired);
  - a reading trace, per mode, of one REST command, one button and one key
    to the same handler: Inspect `display` / Explode button / key E →
    `inspect::apply`; Build `system_undo` / an Undo button / Cmd+Z →
    `builder::system_actions::apply`; Lessons `lesson_scene {action:
    play}` / the Play button / Space → `lesson::actions::apply`; Robot
    `robot_speed {action: up}` / the + button / key = →
    `robot::actions::apply`; Place `camera {station: 0}` / key 1 →
    `place_view::apply` (Place has no buttons).
  - Bevy API spots to check first: `Option<MessageWriter<…>>` params in the
    `pick_part` observer; `MessageWriter` in the `pick_net` and
    `pick_reference` observers; `EntityWorldMut::observe` with
    `save_to_disk` in `switch::screenshot`; `Messages::drain` from
    `ResMut<Messages<Act<A>>>`; `#[derive(Resource)]` on the generic
    `InFlight<A>`; `MessageWriter` in the `pick_link` observer;
    `World::write_message` in `serve`.
- **Verified at 90c65c86** (see "Where it is today" and the epic order):
  built and tested as listed above.

## UI kit (2026-09-30)

Batch ui-kit (epic order item 5) put every mode's common UI on one kit,
`crates/sim-spatial/src/ui_kit/`. Commits: 66a79f33 (kit, tokens, chart),
60a9b9e3, d2584f09 and b5dfcb40 (kit follow-ups and tests), 1a9583da (robot),
6f8f4d78 (build panels), 864bdda6 (lessons), 106c4dfe (inspect, place,
switcher), 32e4cea1 and a56b4260 (review fixes).

- **Layout.** `theme.rs` (tokens, `UiFonts`, `Look` and its `Paint`, `Tint`
  and presets), `widgets.rs` (`Kit` and the repaint and label systems),
  `slider.rs` (slider, pointer surface), `scroll.rs` (scroll area, wheel
  step, scroll clamp), `mod.rs` (contract, `UiKitPlugin`), `tests.rs`.
  `UiKitPlugin` is added once by `app::CorePlugin`.
- **Tokens.** The builder's palette and metrics, defined once: layout
  (`TOPBAR`, `STATUSBAR`, `LEFT_WIDTH`, `RIGHT_WIDTH`), colours (`BAR`,
  `SURFACE`, `RAISED`, `HOVER_BG`, `BORDER`, `TEXT`, `SUBTLE`, `FAINT`,
  `VALUE`, `ACCENT` and its hover, `ACCENT_BG`, `ON_ACCENT`, `WARN`, `DANGER`
  and its hover and edge, `OK`), the type scale (`size::…`) and `UiFonts`.
  Deleted: `builder::ui::Kit`, `Look`, `Tint`, `UiFonts`, `hover`, the
  builder/ui.rs and lib.rs colour constants, lesson/ui.rs's `ACCENT` alias,
  robot.rs's `label`, `tab` and header builders, the switcher's own button
  colours, `builder/graphs.rs::rasterize` and `view::cursor_fraction`.
- **Temporary re-exports** (66a79f33) kept the tree whole while the four
  modes moved: `builder/ui.rs` re-exported the kit (tokens, `Kit`, `Look`,
  `Tint`, `UiFonts`, `wrap`, `divider`), `builder.rs` re-exported the layout
  constants and `lib.rs` aliased `INK`/`MUTED`/`PANEL`/`ACCENT` to kit tokens.
  All three are gone (6f8f4d78, 106c4dfe); every file imports `crate::ui_kit`.

### Decisions

- **Buttons: `bevy::ui::Button` + `Interaction`, not `bevy_ui_widgets::Button`.**
  The headless button activates on release (`Pointer<Click>`), or on a
  picking press with `ActivateOnPress`, through `Activate` observers. Every
  mode's input system, `system_ui` activation, the tests and the hover
  styling read press-time `Interaction`, so adopting it would have changed
  when clicks count and rewritten the action layer's input side. The kit
  button keeps the same `Button` + action component + `Enabled` + label text
  that `builder::ui_api::collect` discovers, so ids and labels are unchanged.
  Revisit if the action layer moves to observers.
- **Sliders: `bevy_ui_widgets::Slider`** (`TrackClick::Snap`, no thumb, range
  0..=1, `SliderStep(0.01)`), with Bevy's `slider_self_update` observer.
  Modes poll `SliderValue` (the pointer's fraction, as `cursor_fraction`
  gave) and `ui_kit::slider_held` (the widget's `Pressed` and the left
  button's `Interaction::Pressed`), so the timeline, parameter sliders and
  narration bar send the same actions at the same moments as before. The
  widget alone reacts to every mouse button and can keep `Pressed` after a
  still release off the bar; `slider_held` restores the old left-button,
  release-anywhere semantics. The chart hover and the sketch canvas are 2D
  pointer surfaces (`pointer_surface`, `surface_point`), which Bevy has no
  widget for.
- **Scroll areas: not `bevy_ui_widgets::ScrollArea`.** Its wheel step is
  fixed at 100 px a line and it scrolls the node under the pointer; the
  modes scroll their columns by pointer region at 28 px (24 px in robot and
  inspect) a line. The kit gives the node (`scroll_area`), the wheel step
  (`wheel_delta`) and the clamp (`clamp_scroll_positions`, moved from
  `view.rs`). No scrollbar widget was added (a visual addition).
- **Feathers: not used.** Its palette, fonts and control shapes are not the
  builder's, and the batch preserves the builder's look; its theme tokens
  would have been a second token set. Revisit with a visual refresh.
- **Text input: Bevy's text input not adopted.** The builder's draft
  semantics (the "/" filter key is not typed, Escape leaves Connect, a draft
  survives a mode switch) are recent reading-only fixes with no test; the
  kit only styles the entry (`Kit::input`). Revisit when those have tests.
- **State styling.** `Look` is a component: a mode that toggles a button
  (current tab, lit chip, chosen segment, enabled) writes a new `Look` or
  `Enabled` and `repaint_buttons` repaints it, hover included. `Tint`
  (with presets; the struct literal is forbidden outside the kit) does the
  same for rows and cards. This replaced robot's, inspect's and the
  switcher's per-frame `BackgroundColor` painting.
- **Accessibility.** Kit buttons, rows, inputs, sliders and pointer surfaces
  carry `AccessibleLabel`; `keep_labels` re-applies it after `bevy::ui`'s
  own button labelling (which clears labels of buttons without direct text
  children), and `follow_button_text` keeps a button's label in step with a
  rewritten label text. The remaining hand-built clickable rows got labels.
- **Colour guard allowlist** (`ui_kit/tests.rs`): `builder/schematic.rs`
  (the light schematic canvas), `builder/placement.rs`, `models.rs`,
  `linked.rs`, `animation.rs` (3D materials and gizmos). Tag colours
  (`builder::ui::tag_color`), chart trace colours and card drawings are
  their own colours and none equals a token today (the one that did, the
  Subsystems tag, now names `ACCENT`).

### Visual differences (intended, none designed)

- **Inspect:** text, secondary text and panels converge on `TEXT`, `SUBTLE`,
  `SURFACE` (columns) and `BAR` (header and footer, now with a 1 px edge);
  the old INK 0.87/0.90/0.94, MUTED 0.56/0.64/0.72 and PANEL 0.075/0.093/0.12
  are gone. Buttons (fill 0.115/0.15/0.19, border 0.21/0.27/0.33, hover
  0.20/0.26/0.32, active 0.12/0.32/0.31, 14 px) become Chip (toggles and
  selections) and Secondary looks at 11.5/12.5 px; headings become kit
  sections; the title is semibold IBM Plex at its old size.
- **Robot:** the header is BAR with an edge, side columns and graph dock get
  1 px edges; buttons (0.16/0.20/0.25, 45 % accent hover) become Secondary,
  overlay toggles Chips, the current section Tab(true), link selection
  `ACCENT_BG`; disabled buttons show FAINT text instead of a faded fill; the
  title is 16 px semibold (was 18 px regular); block headings are kit
  sections with a caption; the motion, recorded and gait rows wrap.
- **Switcher:** a kit segmented control (Segment palette, kit frame and
  padding) on its translucent backdrop.
- **Physics overlay bar:** opaque Chip looks with hover (were translucent
  without hover).
- **Build:** none intended; the graph dock sits in a layout-only positioning
  node. The inspect notes panel now uses IBM Plex, kit sections and buttons.
- **Lessons:** the notes column's titles are 16 px (were 18), the lab's
  expected values are property rows, equation and id text may break inside
  words (`Kit::mono`), the narration overlay label uses `ON_ACCENT`.

### Remaining raw `Node {` trees (reasons)

- Layout-only containers: rows, columns, gaps and margins, the `layout`
  argument of `dock`/`scroll_area`/`chart_image`, positioning slots
  (graph docks, overlay bar, switcher corner, split and inset captions over
  the 3D view, place help).
- Feature drawings: the schematic pane and canvas (boxes, nets, ports,
  overlay), viewport markers and leader lines, lesson cards (scene, quiz,
  compare, reflect, lab, locked, editor, agent), playheads, masks, fills,
  event marks, the concept meter, sketch canvas, grid and dots, phase dots,
  figures, markdown blocks, discussion and annotation cards, composers and
  3D labels.
- Rows the kit has no widget for yet (requested, not built: a card, a
  multi-line text area, a progress bar, an icon-less list row): builder
  reference/snap/study cards, port rows, calibration and gait trial rows,
  lesson outline rows, robot link rows.

### Found by reading and fixed

- The study sweep chart ("metric vs parameter") was drawn with the 20 s time
  window on the parameter axis (dropping points more than 20 units below the
  largest); it is an x–y plot and uses the whole range (66a79f33).
- Robot's run controls could draw under the header (ui roots of equal
  z-index); they get `ZIndex(1)` (1a9583da).
- `Kit::chart_label` would not compile (`use<>` with an `impl` argument);
  sliders answered every mouse button and could stay held; inspect's long
  connection names overflowed; see 32e4cea1 and a56b4260.

### Rejected or deferred review findings

- A disabled `Look::Primary` keeps its accent fill: that is today's builder
  look; not redesigned.
- Moving `Enabled` from `builder::ui_api` into the kit: it is `system_ui`'s
  discovery flag and stays with discovery; the kit depends on it on purpose.
- A kit card, text area, progress bar and icon-less row: new widgets, left
  for when a feature needs them.
- `list_item`'s selected edge is drawn at spawn only (documented; rebuild the
  row to change selection). Robot link rows are hand-built and only change
  their `Tint`.
- Lesson transcript wheel scrolling (the wheel scrolls by column): as before.

### Disk

Before the batch, 18.46 GiB were free. 573 older duplicate
`target/debug/incremental/<crate>-*` directories (keeping the newest per
crate, 377 kept) were removed, freeing about 31 GiB (49 GiB free after).

### Verification checklist

The verification pass ran the following (verified at 4bc03789, below):

- `cargo build -p sim-spatial --lib --tests --bins` with no sim-spatial
  warnings (first compile of the kit: check `use<>` bounds, `AccessibleLabel`
  and `AccessibilityNode` paths, `ChildOf::parent`, the `bevy_ui_widgets`
  feature in the lock file);
- `cargo test -p sim-spatial --lib`: the registry tests (app/tests.rs,
  `builder::ui_api::tests`) with the same ids, the chart test
  (`chart::tests::a_time_window_draws_the_same_pixels_as_the_points_inside_it`),
  the source guard (`ui_kit::tests::ui_colours_come_from_the_kit`),
  `kit_bundles_spawn`, `looks_paint_the_builder_palette`, the lesson card
  test and the notes panel test;
- `cargo check -p sim-app` (dropped since fold-sim-app: the crate is retired);
- a reading trace per mode of one button, one slider and one scroll area:
  - Build: Toolbar "Run" (`Kit::button` → `Button` + `BuildAction::Run` →
    `builder::actions::buttons` on `Changed<Interaction>` → `Act` →
    `system_actions::apply`); sidebar scroll (`Scroll::Left` +
    `scroll_panels` via `wheel_delta`); no slider.
  - Lessons: timeline (`Kit::slider(Timebar)` → `SliderValue` + `slider_held`
    in `lesson::seek` → `SeekTo`); a lesson button (`LessonAction` →
    `lesson::actions::buttons`); the page scroll (`LearnScroll::Page`).
  - Robot: a run button (`RobotAction::Run` → `robot::actions::buttons`);
    the inspector scroll (`InspectorScroll` + `robot::scroll`); no slider.
  - Inspect: a toolbar chip (`InspectAction` → `inspect::input`); the
    inspector scroll (`scroll_inspector`); no slider.
  - Switcher: a mode segment (`ModeButton` → `switcher_clicks` →
    `WindowAction::Switch`).
  - Place: no buttons or sliders (keys and fly camera only).
- **Verified at 4bc03789**, after the fixes in 9b6aa068, 801f12c5 and
  4bc03789.

## Hardware front end (2026-09-30)

Batch hardware-front-end (epic order item 6, §8) built the Leg calibration
panel: a native front end, feature for feature, over the browser's
`web/viewer/calibration-ui.mjs` (with `actuator-motion-view.mjs`),
`calibration-mirror.mjs` and `hardware-sync.mjs`. The hardware servers did
not change. The ledger is [docs/hardware-parity.md](../hardware-parity.md);
the operator's steps are [docs/hardware-checklist.md](../hardware-checklist.md).
Paths are `crates/sim-spatial/src/` unless they name another crate.

### Shape

Every name below was checked with `grep 'fn <name>'` (or the type's
definition) against the final code.

- `sim-runtime/src/hardware_client/`: the one typed loopback client.
  - `mod.rs`: the contract (loopback, headers, token hand-off, errors, body
    order), `ClientError`, `Json`/`Body` (`Body::text`), `js_number`,
    `encode_uri_component`, the tolerant deserializers `lenient` and
    `lenient_items`, and the constants `CONNECT_TIMEOUT` (500 ms),
    `REQUEST_TIMEOUT` (10 s), `STOP_TIMEOUT` (12 s), `CALIBRATION_MAX_BODY`
    (4096) and `MOTOR_BENCH_MAX_BODY` (8192).
  - `http.rs`: `Endpoint::parse` (127.0.0.1 or localhost only),
    `Client::new`, `for_kind` (the body limit), `with_timeout` (at least
    1 ms), `get`, `post`, `get_as`, `post_as`, `page`, `send_only` (written,
    answer never read), `exchange` (a non-2xx answer's `error`, else
    "Request failed (HTTP {status})"), `new_client_id`.
  - `token.rs`: `discover`, `from_page`, `read_file` and `connect`.
  - `calibration.rs`: the status types (tolerant: every field defaults, a
    malformed or null section reads as its default) and one builder per
    command body the page posts (`command`, `stop`, `select`, …, `jog`,
    `gait_path`; `Input::members`).
  - `bench.rs`: `serve_motor_bench`'s `/config`, `/status`, `/live/open`
    (`open`), `/live/sample` (`sample`) and `/stop` (`stop`);
    `SessionResult::summary`.
- `robot/hardware/`, built by `RobotPlugin` (`hardware::build`):
  - `mod.rs`: `Hardware` (the resource, present only in Robot mode;
    `Hardware::new`, `target`, `url`), `HardwareConfig`/`ServerTarget` (the
    launch flags, kept in `app::switch::Documents`), `Section`
    (`open_initially`), `MirrorDisplay`.
  - `actions.rs`: `HardwareAction` (every intent), `starts_motion` and
    `remote_refusal`, the REST form (`wire::Command`, `parse`),
    `Preferences` (read once in `build`), the lifecycle `enter` (OnEnter
    Robot: connects with `--hardware`) and `leave` (OnExit Robot: calls
    `stop_immediate` and `LiveSync::stop_ours` directly, keeps the
    preferences, then drops the state off the UI thread), `stop_immediate`,
    `connect`, the input systems `buttons`, `jog_buttons`, `keys` (Q/A
    hold-to-move, Z/Escape), `window_loss` (`WindowFocused` false →
    `Loss { FocusLost }`, `WindowCloseRequested` → `Loss { Leaving }`) and
    `sliders`, then `apply` in `ViewerSet::Actions` and `poll_jobs` in
    JobResults.
  - `handlers.rs`: one handler per intent, `handle`; `remote_check` (a
    remote action whose control is disabled now is refused with its
    reason; `loss` `leaving` is refused from REST and `system_ui`), `loss`
    (the synchronous STOP only for the window's own close request,
    `Origin::Quiet`), `close`, `gait_play`, `export`,
    `start_export`, `write_export`, `load_gaits`, `gaits_json`.
  - `link.rs`: `Link::spawn` (one `jobs::RunThread` "hardware-link" per
    connection, join bound zero), `Link::send`, `Link::snapshot`,
    `LinkCommand` (one per page handler; `Stopped { epoch }`), `Inputs`
    (with `speed_reset`), `LinkSnapshot` (generation-stamped, `read_at` for
    `stale`; `compiled_gait` is a field), `drive_active`, the page's
    periods, and `stop_now` (the immediate STOP job; returns the epoch it
    bumped to).
  - `session.rs`: the page's state machine on the link thread: `run`,
    `Session::handle`, `render` (the page's render-time rules:
    `merge_warnings`, `leg_frame`, the leg gait's end, learning complete),
    `stop`, `stopped_locally`, `select_motor`, `update`, `begin`, `move_`,
    `release`, `set_disabled`, `poll`, `next_deadline`, `run_due`,
    `shutdown`, `stop_after_dropped`, `interrupted`, `time_of_day`.
    - `session/buttons.rs`: `target`, `capture`, `reset`, `flip`, `sweep`,
      `learn`, `raw_step`.
    - `session/sequences.rs`: `sweep_all`/`sweep_all_tick`, `tune`/
      `tune_tick`, `campaign`/`campaign_tick`, `load_gaits`, `gait_play`/
      `start_gait`, `gait_lease` (its own Dedicated job), `gait_toggle`,
      `gait_scale`, `gait_stop`, `end_gait`, `sim_frame`/`leg_frame`.
  - `view.rs`: the page's `render()` rules as pure functions of the
    snapshot and the form: `render`, `render_gait`, `status_line`, `chips`,
    `angle`, `fraction`, `speed`, `outside_pose`, `stats_rows`,
    `gait_option_label`, `fixed` (JavaScript `toFixed`), `js_num`,
    `PanelView::block`, and `status_json` (REST `hardware_status`).
  - `dial.rs` (`needle_end`, `rasterize`) and `motion_view.rs`
    (`comparison`, `update`): the dial and the command-vs-motion chart.
  - `panel.rs`: `spawn` (the dock: `FocusPolicy::Block`, the accessible
    label), `panel_view`, `control_list`, `controls` (the `hardware:<name>`
    list for `system_ui`), `connection_line`, `scroll`, `refresh`;
    `panel_sections.rs`: `top_bar`, `body`, `section`, `chips`, `gaits`,
    `stats`, `runs`, `chart_labels`.
  - `mirror.rs` (`Mirror::prepare`, `update`, `follow_gait`, `poll`,
    `alignment_angle`, `gait_bindings`, `record`, `worker_failed`, `apply`,
    `SceneId`, `worker`) and `mirror_panel.rs` (`mirror_sync`, which drives
    begin/prepare, updates and gait sampling every frame; `mirror_panel`,
    `fill`): the suspended robot posed from the encoders through
    `sim_runtime::kinematic_mirror::KinematicMirror` on a jobs worker,
    written into `RobotView::mirror`.
  - `sync.rs` (`LiveSync::connect`, `start`, `stop`, `stop_ours`, `poll`,
    `poll_status`, `on_frame`, `watch_run`; `live_input`, `sample_from`,
    `source_text`, `legs`, `mapping`, `distinct`, `banner_text`,
    `reading_lines`, `apply`, `drain`, `worker`) and `sync_panel.rs`
    (`sync_frames`, `sync_panel`, `row_title`, `rms_and_saturation`,
    `charts`, `chart_note_of`, `sync_overlay`, `sync_texts`): Real motor
    sync against `serve_motor_bench`.
  - `settings.rs`: the free functions `load` and `path`, and the method
    `Settings::save` (an Io job), the preferences file.
- Outside the folder: `app/actions.rs` (`Origin::SystemUi`,
  `Action::accepts`, `Call::remote`, the registry entry "hardware"),
  `robot/actions.rs` (`apply`: `system_ui` passes `hardware:<name>` on,
  refuses motion by name and a disabled control with "{id} is disabled:
  {why}", and lists the controls; `motion_keys`: A is given to the panel
  while it is open; `check`: Run refused while mirroring), `robot.rs`
  (`setup`: the header button; `highlight`: `RobotView::mirror` drawn
  instead of the run's frame, the blue `Materials::mirrored`; `scroll`: the
  inspector ignores the wheel while the panel covers it), `robot_run.rs`
  (`MotorTargets.done`, the frame's episode end), `chart.rs`
  (`rasterize_fixed`, fixed axes for the sync charts), `main.rs` (the four
  flags), `app/switch.rs` (`Documents::hardware`).

### Decisions

- **Token hand-off: read it from the page the server serves.** The
  calibration server's `calibration-token` meta; the bench's
  `const token='…'` in `/` or the `motor-bridge-token` meta of `/walking/`;
  or `--hardware-token-file` / `--motor-bench-token-file`. *Why:* the least
  invasive choice, with no server change at all. Any local process that can
  reach the loopback port can already read the page, so it grants nothing
  new. *Rejected:* a 0600 token file written by the server (a server
  change, on hardware code the checklist would then have to requalify).
  *Revisit if* the servers stop injecting the token into their pages, or
  bind beyond loopback.
- **Loopback means 127.0.0.1.** `Endpoint::parse` accepts
  `http://127.0.0.1:PORT` and `http://localhost:PORT` (connected as
  127.0.0.1) and refuses `[::1]` and every other host. *Why:* both servers
  bind IPv4 127.0.0.1 only and require `Host: 127.0.0.1:PORT`; `[::1]`
  could never connect. *Revisit if* a server binds IPv6.
- **STOP on its own connection.** A `jobs::Pool::Dedicated` job with
  `complete_on_drop`, never queued behind the link thread, with
  `STOP_TIMEOUT` 12 s. The link thread also sends STOP when its channel
  closes. *Why:* the link thread can sit up to 8 s in one request (a select
  proving watchdogs, a hardware reply), and the server latches its stop
  flags when it parses the request, so a second connection lands at once;
  the server answers a STOP only after its worker finishes, up to 8 s, so a
  shorter timeout would report a STOP that worked as failed. *Rejected:*
  STOP as a `LinkCommand` (waits behind the request in flight), or a
  persistent second socket (both servers close after each answer).
  *Revisit if* the servers gain a streaming control channel.
- **STOP ordering on the link.** `stop_now` returns the epoch it bumped to
  and the UI sends `LinkCommand::Stopped { epoch }`; while the shared epoch
  is ahead of the last one the link applied, it sends nothing but `stop`
  (`STOP_PENDING`). A `select`, `motion_start`, `sweep_all`, `gait_start`,
  `tune` or `campaign` that succeeded but whose answer is dropped because
  STOP arrived while it was in flight is followed by a `stop` at once
  (`stop_after_dropped`). *Why:* the server may parse such a request after
  the UI's STOP (a `select` clears the stop latch), and nothing else would
  end what it energized; a second STOP pressed before the first was
  applied must stay pending. *Rejected:* the page's rule alone (drop the
  answer), which can leave the motor driving. *Revisit if* the server
  orders requests by sequence across connections.
- **Gait lease on its own job.** Each `gait_update` is posted from its own
  Dedicated job with a 1 s timeout, at most one in flight. *Why:* the
  server ends a leg gait after 1.5 s without an update, and a request on
  the link thread may wait up to 8 s. *Revisit if* the lease period
  changes.
- **Loss of control stops any drive.** `WindowFocused` false, panel close,
  leaving Robot mode and closing the window stand for the page's
  `visibilitychange`, close/toggle and `pagehide`. Unlike the page's
  `loss()` (ready, starting or a session), any drive stops:
  `link::drive_active` also counts busy, sweep-all, tuning, campaigning and
  a leg gait. `Loss::Leaving` always stops. Closing the window also writes
  STOP synchronously with `Client::send_only` (`Link::post_stop_sync` and
  `LiveSync::post_stop_on_leave`, the keepalive equivalent; at most the
  500 ms connect timeout and a loopback write, once, retried by a later
  exit path only if it failed), and so do `AppExit` (`stop_on_exit`) and
  the link's drop. Only the window's own close request (`Origin::Quiet`)
  writes it on the UI thread; REST and `system_ui` are refused `leaving`. Live sync stops on every loss, but only a session this
  viewer opened (`LiveSync::stop_ours`); the operator's Stop motors and
  STOP use `LiveSync::stop`, which also ends a session the bench reports
  from elsewhere. *Why:* the page leaves a leg gait, tune, campaign or
  sweep-all driving after the tab is hidden; that is unsafe with nobody
  watching (AGENTS.md). *Revisit if* the panel is ever shown in a second
  window.
- **`Origin::SystemUi`.** A `system_ui` activation that robot mode passes on
  to `HardwareAction`. It is remote like `Rest`, but its REST command was
  already answered by robot mode's handler, so its outcome is dropped; for
  that reason robot mode refuses a disabled control itself ("{id} is
  disabled: {why}"). *Why:* the refusal rule has to see that the intent
  came from automation; `Origin::Ui` would have let `system_ui` start
  motion. *Rejected:* `Origin::Rest` with a dummy reply token (a second
  answer to one command), or a separate `system_ui` feature for hardware (a
  second controls list beside robot mode's). *Revisit if* other modes start
  passing activations on.
- **Refusal rule.** `HardwareAction::starts_motion` actions from
  `Origin::Rest` or `Origin::SystemUi` are refused with "hardware `{name}`
  starts, changes or arms motion and needs an operator at the window: REST
  and system_ui may read status, list gaits, export, connect, turn the
  mirror on or off and STOP only". Refused: select, set disabled
  (enable/disable), sweep all, hold others, jog press/release, speed,
  target and its commit, capture, reset poses, clear lower/upper, sweep,
  learn, the tune/campaign/gait confirmations, tune, campaign, gait
  select/mode/speed/effort/play, drive mode, PWM ceiling, flip, raw step
  value, raw step, live sync's leg/motor/polarity/scale/start, and the
  mirror's leg, joint, polarity and alignment bindings (they become the
  Leg/Both `gait_start` bindings and the alignment reference that Save
  sim alignment sends). Allowed: toggle/close panel, connect, sections,
  status, STOP, loss (focus lost, panel closed; `leaving` is the window's
  own close request and is refused from REST), export, load gaits, gait
  stop, mirror on/off, sync connect and sync stop. Robot mode refuses them when a `hardware:<name>`
  control is activated, and the hardware handler refuses them again.
  *Why:* AGENTS.md: drive motors only with the operator present; the
  confirmations and drive settings arm or shape motion, so automation may
  not set them either. *Revisit if* an operator-presence signal other than
  a local pointer or key exists.
- **`Action::accepts`.** The registry reads an action type's accepted REST
  commands from `A::accepts()` (default: its own serde variants).
  `HardwareAction` names its REST form, `wire::Command`. *Why:* the
  hardware REST form (`hardware_status`, …, `hardware {action}`) is not the
  action enum's own serde form, and the registry test cross-checks
  `accepts` against `commands`. *Revisit if* more action types need a
  separate REST form (then make it the rule).
- **A is given to the panel while it is open.** `robot::actions::motion_keys`
  drops A from WASD while `Hardware::open`; W, S and D still steer. *Why:*
  the page's capture-phase handler takes Q/A from the robot viewer the same
  way, and A is the lower jog. *Revisit if* robot mode's steering keys
  change.
- **Run refused while mirroring.** `robot::actions::check` refuses
  `Run { Start }` while `RobotView::mirror` is set (`mirror::MIRRORING`).
  *Why:* the page's `setPlaying` refuses to play while mirroring, and the
  mirror's poses replace the run's frame. *Revisit if* the mirror is drawn
  as a second robot instead.
- **Placement: a dock in Robot mode, not a mode.** The dock blocks the
  pointer (`FocusPolicy::Block`) and takes the wheel while open, so nothing
  reaches the inspector under it. *Why:* the mirror and live sync need Robot
  mode's robot and run, and STOP-on-leave is then the mode's own exit.
  *Rejected:* a Hardware mode (it would duplicate robot loading and lose
  the mirror's robot). *Revisit if* the hardware UI grows beyond the leg
  fixture.
- **Export location.** Download calibration writes
  `<server output>/viewer-exports/leg-calibration-<unix_ms>.json` (create_new,
  never overwritten) and shows the path; REST `hardware_export` answers it.
  *Why:* the viewer has no downloads folder, and the server's output
  directory is where the calibration's own versions live. *Revisit if* a
  native save dialog is added to the kit.
- **Preferences file, read once.** `$SIM_SPATIAL_PREFERENCES`, else
  `~/.config/sim-spatial/hardware-preferences.json`, replaces the pages'
  localStorage (`calibration-drive-mode`, `calibration-hold-others`,
  `calibration-mirror-v1`, `walking-hardware-map-v1`). It holds operator
  preferences only, never calibration data. It is read once when the app
  is built (`actions::Preferences`), so entering Robot mode reads no file
  on the UI thread; saves go to an Io job. Unknown or missing fields take
  their defaults, a stored sign is clamped to ±1. *Why:* the nearest
  native equivalent, and testable through the env override. *Revisit if*
  Bevy's app settings (0.19) are adopted for the viewer's other
  preferences.
- **Warnings stamped in UTC.** *Why:* the viewer carries no timezone
  database; a local offset guessed without one could be wrong. *Revisit if*
  a timezone crate is added for another reason.
- **Stale status shown as stale** (a native addition). A status older than
  2.4 s (four idle polls) is marked stale, never shown as live, and the
  motion controls are blocked. *Why:* the page silently keeps the last
  answer; the native link publishes a timestamp, so it can say so.
  *Revisit if* the poll periods change.
- **No free-text number fields.** The PWM ceiling (0–100 in 0.1 steps) and
  the raw step (−4095..4095) are a slider/stepper, not the page's number
  fields. *Why:* the kit has no validated number entry, and a control that
  can only hold valid values makes the page's `reportValidity` checks hold
  by construction. *Revisit if* the kit gains a numeric field.
- **Honest labels where the page prints 0.** A missing gait statistic
  prints "— ms"/"—%", a null governor limit or tracking error "—", and an
  empty limits object no "Limits: " line, where the page's arithmetic on
  `null` prints 0 (`view::stats_rows`, `render_gait`, `rounded`). *Why:*
  AGENTS.md: label missing values honestly; a missing measurement is not a
  measured zero. *Revisit if* the server stops writing `null`.
- **`toFixed` exactly.** `view::fixed` rounds an exact binary tie away from
  zero, as JavaScript does, and leaves every other value to `format!`
  (which already rounds the exact binary value). *Why:* the texts must
  match the page digit for digit. *Revisit if* none.
- **Live sync off the UI thread, newest sample wins.** `/status` is polled
  on its own Dedicated job; samples go through the sync RunThread, which
  drains its queue and keeps only the newest (`drain`, `Outbox`); `/stop`
  is posted on its own job whenever a session may be open, and again when
  an open lands after a stop. The run's episode end is the frame's
  `MotorTargets.done`. *Why:* a slow poll must not delay a sample or a
  STOP; a STOP during `/live/open` must not let the open outlive it.
  *Revisit if* the bench gains a streaming channel.

### Deviations

Every `deliberately-different` row of the ledger, in short:

- the panel connects to a server instead of being served by it (CAL-01);
- a stale-status marker (CAL-11, addition);
- an answer without `error` reads "Request failed (HTTP {status})", for the
  calibration page's "Request failed" and the sync page's `r.statusText`
  (CAL-15, SYNC-31; `hc/http.rs` `exchange`);
- loss of control stops any drive, not only a ready or moving motor
  (CAL-30);
- a disabled motor's chip reads "⊘ Knee 1", not a strike-through (CAL-36,
  `view.rs` `chips`);
- warning times in UTC (CAL-52);
- no Space/Enter on a focused jog button: kit buttons take no keyboard
  focus, Q/A are the keyboard path (CAL-58);
- no gait tooltip: the summary is in REST `hardware_gaits` only (CAL-110,
  `handlers.rs` `gaits_json`);
- PWM ceiling and raw step as slider/stepper (CAL-134, CAL-138);
- preferences in a file (CAL-140, MIR-05, SYNC-09);
- export to `viewer-exports/` (CAL-141);
- the aria-labels of the × button, the chip group, the warnings, the radios,
  the dial and the control-mode select have no `AccessibleLabel` (CAL-146);
  checkboxes, the gait select, radios and the statistics table are kit
  chips, segments and lines (CAL-151);
- "—" where the page prints 0 for missing statistics, limits and gait
  errors; no bare "Limits: " line; the recent-runs heading falls back to the
  path, not "undefined" (CAL-148, CAL-149, CAL-150);
- without a bench configured, the sync section explains how to start it
  (SYNC-01);
- sync is a section of the Leg calibration panel, not of the walking page's
  inspector (SYNC-03);
- live sync also stops on focus loss, panel close and mode exit, only for a
  session this viewer opened (SYNC-32), and a STOP during `/live/open` is
  re-posted when the open lands (SYNC-33).

### Review findings

Five reviews read the batch (client, link, panel, mirror and sync, docs).
Every finding and its disposition; locations are files in this section's
Shape.

- **Client** (`hardware_client`):
  - STOP timeout too short (3 s) for a server that answers STOP only after
    its worker finishes (up to 8 s): fixed, `STOP_TIMEOUT` 12 s (`mod.rs`).
  - `[::1]` accepted though both servers bind IPv4 127.0.0.1 only: fixed,
    refused (`http.rs` `Endpoint::parse`); `localhost` connects as
    127.0.0.1.
  - No request body limit: fixed, 4096 bytes for the calibration server and
    8192 for the bench before connecting (`Client::for_kind`,
    `checked_body`; `token.rs` `connect` sets it).
  - Null gait values from the server: fixed, read as missing by tolerant
    fields (`lenient`, `lenient_items` in `mod.rs`; `calibration.rs`) and
    shown as "—" (`view.rs` `rounded`).
  - A zero timeout was an error from `std::net`: fixed, clamped to 1 ms
    (`Client::with_timeout`).
- **Link** (`link.rs`, `session.rs`):
  - Double-STOP race (a second STOP's pending state cleared by the first
    `Stopped`): fixed, `stop_now` returns the epoch and
    `LinkCommand::Stopped { epoch }` (`Session::stop_applied`).
  - An answer dropped because STOP arrived while it was in flight could
    leave a motor energized: fixed, `stop_after_dropped` after a dropped
    `select`, `motion_start`, `sweep_all`, `gait_start`, `tune` or
    `campaign`.
  - STOP timeout on the link's own STOPs: fixed, `STOP_TIMEOUT` (`stop`,
    `shutdown`, `stop_after_dropped`).
  - Loss checked only ready/starting/run: fixed, `link::drive_active` on
    both the UI (`handlers.rs` `loss`) and the link (`Session::handle`,
    `shutdown`).
  - Speed-reset race (an `Inputs` sent before the UI saw a sweep's speed
    reset put the old speed back): fixed, `Inputs::speed_reset`
    (`Session::handle`, `actions.rs` `poll_jobs`).
  - Gait lease could expire behind a slow request on the link thread:
    first fixed by posting each update from its own job, but its schedule
    still ran on the link thread; fixed for good in the verification pass
    (item 3 below), the "hardware-beat" worker.
  - Dropping the link joined a thread that may wait `STOP_TIMEOUT`: fixed,
    join bound zero (`Link::spawn`), dropped off the UI thread.
- **Panel** (`actions.rs`, `handlers.rs`, `panel.rs`, `view.rs`):
  - A leg gait was not stopped on loss: fixed (`drive_active` counts a leg
    gait).
  - Live sync kept running after the panel closed: fixed, every loss calls
    `LiveSync::stop_ours` (`handlers.rs` `loss`).
  - Clicks fell through the dock to the inspector: fixed,
    `FocusPolicy::Block` (`panel.rs` `spawn`); the inspector's wheel is
    ignored while the panel covers it (`robot.rs` `scroll`).
  - `toFixed` ties rounded to even: fixed, `view.rs` `fixed`.
  - The gait's Stop for a leg gait waited behind the link: fixed, it goes
    through `stop_immediate` first (`handlers.rs` `handle`, `GaitStop`).
  - A `system_ui` activation of a disabled control read as success (its
    outcome is dropped): fixed, robot mode refuses it with "{id} is
    disabled: {why}" (`robot/actions.rs` `apply`).
  - Window close relied on a detached job that may die with the process:
    fixed, STOP also written synchronously with `Client::send_only`
    (then `handlers.rs` `post_stop_on_leave`; since the verification pass
    `Link::post_stop_sync`), and `leave` calls
    `stop_immediate` and `stop_ours` directly.
  - Preferences were read from disk on entering Robot mode (UI thread):
    fixed, read once at app build (`actions::Preferences`).
  - Parity texts where the page prints 0 or "undefined" (missing
    statistics, limits and gait errors; the empty limits line; the
    recent-runs heading): kept as deliberate differences (honest labels;
    CAL-148, CAL-149, CAL-150).
- **Mirror and sync** (`mirror.rs`, `mirror_panel.rs`, `sync.rs`,
  `sync_panel.rs`, `settings.rs`):
  - STOP while `/live/open` was in flight could be overtaken by the open:
    fixed, an open that lands after a stop is deactivated and `/stop`
    posted again (`LiveSync::poll`); STOP is never gated on local state
    (`LiveSync::stop`, `may_be_open`).
  - `/status` polled on the sample thread: fixed, its own Dedicated job
    (`poll_status`).
  - Samples could queue behind a slow post: fixed, the newest wins
    (`drain`, `Outbox`).
  - Settings file work on the UI thread: fixed, read once at app build
    and saved by `Settings::save` on an Io job.
  - Scene identity (which loaded run the mirror poses): fixed, `SceneId`
    (`Weak::ptr_eq` on the preset or recording).
  - A dead mirror worker went unnoticed: fixed, reported ("Mirror
    unavailable: Mirror worker failed", `worker_failed`) and replaced on
    the next begin (`Mirror::prepare`).
  - A re-begin flashed the run's frame: fixed, the shown poses are kept and
    only the tint changes (`mirror_sync`).
  - `frame.done` read from the run's phase: fixed, `MotorTargets.done`
    (`live_input`).
  - Settings bindings with missing fields failed the whole file, and a
    stored sign could be any number: fixed, tolerant bindings and the
    polarity clamped to ±1 (`settings.rs` `sign`).
  - Sync charts on a moving scale dropped points: fixed,
    `chart::rasterize_fixed` with fixed axes.
  - Doc names: fixed in the module docs.
- **Docs** (this section, §8, the ledger, the checklist, README,
  consolidation §6): every native function name reconciled with the code;
  the refusal rule written out in full everywhere; the checklist's kill
  steps (HW-14, HW-16) no longer move focus away first; HW-05, HW-04, HW-01,
  HW-13 (SYNC-29) corrected; stale placeholders removed; the ledger
  recounted (275 rows). All fixed.
- **Rejected:** none of substance. Kept as deliberate differences: the
  panel parity texts above (CAL-148, CAL-149, CAL-150).

### Verification checklist

What the verification pass was asked to check (its results are in the
next section). No hardware has been driven:

- `cargo build -p sim-spatial --lib --tests --bins` with no sim-spatial
  warnings;
- `cargo test -p sim-runtime hardware_client` (body bytes against the pages'
  `JSON.stringify`, `js_number`, `encode_uri_component`, loopback refusal
  including `[::1]`, body limits, token discovery from saved pages,
  tolerant status parsing);
- `cargo test -p sim-spatial --lib`, in particular `app::tests` (97
  capabilities; every capability parses into its action; `accepts`
  cross-checked against each type's commands),
  `robot::actions::tests` (every listed control, `hardware:<name>`
  included, fits a registered pattern; argument errors unchanged),
  `jobs::tests::threads_are_started_only_in_jobs`,
  `ui_kit::tests::ui_colours_come_from_the_kit`, and the
  `robot::hardware` tests (`panel::tests`, `handlers::tests`,
  `settings` tests, `view::tests`, `session::tests` against a fake server,
  `sync::tests`, `mirror::tests`, `dial` and `motion_view` tests);
- `cargo check -p sim-app` (dropped since fold-sim-app: the crate is retired);
- reading traces of STOP, each to a `stop` request on its own connection:
  the Stop button (`hardware::actions::buttons` → `apply` →
  `handlers::handle` → `actions::stop_immediate` → `link::stop_now`), Z and
  Escape (`keys`), REST `hardware_stop`, `system_ui` `hardware:stop`
  (`robot::actions::apply` → `Origin::SystemUi` → `apply`), focus loss
  (`window_loss` → `Loss { FocusLost }` → `handlers::loss`), panel close
  (`handlers::close`), window close (`window_loss` → `Loss { Leaving }` →
  `loss` → `stop_immediate`, `Link::post_stop_sync` and
  `LiveSync::post_stop_on_leave`), quitting (`stop_on_exit` on `AppExit`,
  and `Drop for Link`), and leaving Robot
  mode (`actions::leave` → `stop_immediate`, `LiveSync::stop_ours`, then
  the link's drop → `Session::shutdown`);
- move the ledger's rows to `done` as they are built and tested, and
  recount them by status;
- **never drive hardware**: the hardware steps are the user's
  ([docs/hardware-checklist.md](../hardware-checklist.md)).

### Verification pass (2026-09-30)

The first build and test of the batch, and a repair turn for every review
finding. No hardware, server, serial port or motion was touched; tests use
in-process fake loopback servers only.

- **Compiler:** sim-spatial did not compile at bce6b21c (`panel::refresh`
  took `&mut` on Bevy's immutable `SliderValue`): fixed, the value is
  replaced with an insert (f7288f5d). `Mirror::state_json` and
  `LiveSync::state_json` were never called, so REST `hardware_status`
  lacked the mirror and sync state: wired in. sim-web's wasm32 build was
  broken before this run (sim-lesson called `write_atomic`, native only):
  fixed with a wasm32 `write_atomic` that returns an error (1d0f463a).
- **Test:** on macOS `set_read_timeout` fails with EINVAL on a socket the
  peer has reset, which hid a server's refusal: now best effort, the
  deadline is still checked (3a246be9).

Reading-review findings, numbered as the orchestrator listed them; every
one is fixed:

1. *Safety.* The mirror's leg, joint, polarity and alignment were
   automatable, but they become the Leg/Both `gait_start` bindings and the
   saved alignment reference: `starts_motion` now covers them; only mirror
   on/off stays remote (`hw/actions.rs`; §8, NAT-06, MIR-08, MIR-09,
   HW-16).
2. *Safety.* Live sync could open a bench session for a run that would
   never start, holding the first target for 12 s: it opens only when the
   run is running or accepts Start (`StartInput::of`,
   `LiveSync::start_with`); replays, recorded playback, replaced runs and
   gait previews are not live (`sync::live_run`, the page's `NOT_LIVE`);
   an engaged session also stops on a failed run, input that stops being
   live, or no Running within 2 s of its Start (`LiveSync::watch`;
   SYNC-16, SYNC-22, SYNC-30).
3. *Safety.* The heartbeats were scheduled on the link thread, which can
   wait 8 s: they run on their own `jobs::RunThread` "hardware-beat"
   (`session/beat.rs`), the single sender of the server's per-run sequence
   domain (`motion_update` and `capture_hold`, which the review of this
   item found shares that check), silent while a STOP is pending; the link
   thread waits for an intent change's beat at most `beat_wait` (two
   requests' connect and read timeouts, a lease, a margin) and acts on a
   beat's failure only when its run and epoch still match (CAL-23, CAL-24,
   CAL-128 to CAL-130).
4. The bench STOP on window close was only a job:
   `LiveSync::post_stop_on_leave` writes it synchronously, for this
   viewer's session only (SYNC-29).
5. The sync thread was not told when the bench ended a session: it is sent
   `Deactivate` (SYNC-25).
6. The synchronous STOP blocked the UI thread for remote `loss`: only the
   window's own close request (`Origin::Quiet`) writes it; REST and
   `system_ui` are refused `loss` `leaving`.
7. Mirror and sync workers were dropped on the UI thread (up to 200 ms):
   released with `jobs::drop_off_thread` (`release_worker`,
   `Drop for Mirror`, `Drop for LiveSync`; MIR-16, MIR-30).
8. Opening the panel while A was held left the simulated robot strafing:
   `HeldKeys` is re-sent without A when the panel opens
   (`robot/actions.rs` `motion_keys`).
9. Quitting without a close request (Cmd+Q, `AppExit`) could skip every
   STOP: `actions::stop_on_exit` in `Last` and `Drop for Link`
   (`Link::post_stop_sync`, at most once per link, retried by a later exit
   path if the write failed). Known limit: a crash or SIGKILL sends
   nothing; leases and the FPGA watchdog stop motion, but a tune,
   campaign or sweep-all runs on the server until it ends or STOP (HW-14).
10. Client numbers differed from `JSON.stringify` (1e-6 to 1e-5, 2^53 and
    above): ECMAScript `Number::toString` (`js_number_text`; CAL-13).
11. A refusal that reset the connection read as "connection reset": the
    answer that arrived is used (`http.rs` `salvage`).
12. Sync and mirror numbers rounded ties to even: `view::fixed`
    (`stats_text`, `reading_lines`, `degrees_text`; CAL-147, MIR-25).
13. `fixed` printed -0 as "-0.0": "0.0" (CAL-147).
14. Motor chips came from the calibration: always the page's Knee 1, Worm 2,
    Belt 3 (CAL-34).
15. A one-sample motion chart was blank: the page's fixed axes through
    `chart::rasterize_fixed`; the labels now show the data's hi and lo, not
    the padded edges, a bug found while fixing it (AMV-10, AMV-12).
16. The motion chart was stretched: it keeps the raster's aspect ratio.
17. Dial caps differed from the SVG: butt track ends with round joins,
    round measured needle, butt dashed requested needle (CAL-74).
18. The client id was new on every connect: one per process
    (`process_client_id`; CAL-12, SYNC-05).

Found by the combined review of the repair and fixed: the beat's wait
had no headroom for a request already in flight (a slow but working
server would read as a dead beat and stop the session); shutdown could let
one more heartbeat out before its STOP (the session now bumps the epoch
first); a failed synchronous exit STOP was not retried. Also found there,
outside this diff: while live sync is engaged, REST and `system_ui` could
still start, step, jog, drive or re-speed the run whose targets go to the
motors. They are now refused while `LiveSync::engaged`
(`robot/actions.rs` `moves_synced_motors`, `SYNC_REMOTE_REFUSAL`); Pause,
Reset and STOP stay available. Deliberate remaining difference: an intent
change while a heartbeat is in flight is sent right after it, where the
page's `heartbeatBusy` drops it until the next beat.

Commands and results are in the batch's report; the last run:
`cargo check -p sim-spatial --all-targets` clean (no warnings),
`cargo check -p sim-web --target wasm32-unknown-unknown` clean,
`cargo test -p sim-runtime --lib hardware_client` 18 passed,
`cargo test -p sim-spatial --lib` 143 passed, 1 ignored (the rerun after the review's last fixes first failed `sync::tests::leave_stop_only_for_our_session`, which still asserted the old never-retry rule; the test now checks the retry).

## CAD mode (2026-09-30)

Batch cad-mode (default order item 7, §9 phase 1, first of several CAD
epics) added a CAD mode to `sim-spatial` that works as a client of
RoboCAD's REST service. RoboCAD's Python kernel and command layer do all the
work, so its undo, provenance and `.rcad` format are unchanged; no Python,
`.rcad`, REST shape, Qt UI or browser page changed, and the hardware client
now uses the shared transport with unchanged requests. The
feature-by-feature ledger is [docs/cad-parity.md](../cad-parity.md) (773
rows: 113 this epic, 637 named later epics, 23 deliberately different, 25
rows flagged as needing a Python route); the side-by-side steps are
[docs/cad-checklist.md](../cad-checklist.md). **Written and reviewed by
reading only; the verification pass builds and tests it.** *Verified at
a4fe42d3 (see the journal's verification pass).* Paths are
`crates/sim-spatial/src/` unless they name another crate.

### Shape

- `sim-runtime/src/loopback_http.rs`: the one loopback HTTP/1.1 transport,
  moved from `hardware_client/http.rs` (unchanged except the refusal text
  and one check order, see Decisions): `Endpoint` (`parse`,
  `loopback`, `host`, `origin`), `Error` (`NotLoopback`, `Transport`,
  `Server {status, error}`, `Decode`), `CONNECT_TIMEOUT`, `Request {method,
  path, headers, body, closed_hint}`, `exchange`, `send_only`, `json`,
  `decode`, and the private request writer, `read_response`, `parse_head`,
  `salvage` and `is_closed` (the macOS reset handling). Per-client headers:
  the hardware client passes `X-Control-Token`, `X-Client-Id` and
  `Content-Type` on control requests (none on `page()`), the RoboCAD client
  only `Content-Type` on requests with a body. Body caps stay per client.
  `hardware_client` re-exports `Endpoint` and `CONNECT_TIMEOUT` and keeps
  `ClientError` as a type alias of `loopback_http::Error`, so its users and
  tests are unchanged.
- `sim-runtime/src/cad_client/`: `CadClient` (`new`, `url`, `with_timeout`,
  `health` (`GET /`), `doc`, `nodes`, `node`, `patch`, `delete`, `mesh`
  (`Ok(None)` only for RoboCAD's 404 "no mesh"), `ops`, `op`, `commands`,
  `run_command`, `history`, `undo`, `redo`, `selection`, `set_selection`,
  `save`, `open`, `load_status`, `cancel_load`, `autosave`, `physical`
  (never passes `path`, which would write a file), `export`); `CadError
  {method, route, status, message}` ("RoboCAD {method} {route}: {message}",
  RoboCAD's `error` verbatim); tolerant serde types in `types.rs` (every
  struct `#[serde(default)]`, unknown fields ignored, `Value` where
  RoboCAD's shape is open; `color`, `pivot`, the mass block and `/doc`'s
  node list are lenient so one malformed value cannot hide the document);
  bare `NaN`/`Infinity` tokens from Python's `json.dumps` read as null;
  `DEFAULT_URL` (`http://127.0.0.1:8420`), `REQUEST_TIMEOUT` (30 s),
  `EDIT_TIMEOUT` (130 s), `MESH_TOLERANCE` (0.1). `service.rs`:
  `interpreter` (`cad/.venv/bin/python`, as `cad/run.sh`; never creates the
  venv), `free_port`, `log_path`, `service_command` (`-m robocad.api <abs
  file> --port N --host 127.0.0.1` in `cad/`, stderr to the log),
  `log_tail`, `wait_until_live`, `START_TIMEOUT` (120 s).
- `jobs/child.rs`: `ChildProcess` (see the jobs module section).
- `cad/` (`CadCorePlugin`: the action, its handler, the connection and its
  results, the mesh cache's lifetime and the REST snapshot, window-free;
  `CadPlugin` = the core + scene, meshes, keys and panels):
  - `document.rs`: `CadDocument` (target, client, `ChildSlot`, connection
    `Connecting`/`Connected`/`Lost`, health, doc, doc key, stale,
    selection, detail, commands, autosave, physical, edit, status,
    revision), `CadTarget` (`File` or `Service`), `TreeRow`,
    `CadInputFocus`, `switch_blockers`, `leaving_note`, `release_child`.
  - `sync.rs` (`sync/` since cad-select-transform): the connect job (`Pool::Dedicated`; a file starts the
    service and waits for `GET /`), the `cad-poll` `RunThread` (every
    500 ms `GET /` and `GET /selection`; `/doc`, `/commands` and (GUI)
    `/autosave` when the document id or revision changes or on Refresh;
    its own 5 s client), and every result in JobResults (`receive`):
    connect, snapshot, staleness, selection adoption, selection pushes
    (one at a time, newest wins), node detail, physical, edits, the
    child's exit with its log tail; `on_exit` at window close.
  - `mesh.rs`: `CadMeshes`, fetch (`Pool::Dedicated`, at most 2 at once)
    then build (`Pool::Compute`), cached by (node id, revision), drawn
    under a Z-up mm→m root, picked (`CadSelect`, the tree's value), a
    failed fetch retried on Refresh and on reconnect.
  - `scene.rs`: camera (`MeshPickingCamera`), UI camera, light, orbit,
    fit. `keys.rs`: RoboCAD's keymap (below).
  - `actions.rs`: `CadAction` and its one handler `apply` (Actions),
    `system_ui` (its controls are `panel::controls`), `cad_state`,
    `publish` (`/v1/state`, `/v1/cad_state`).
  - `panel.rs`, `tree.rs`, `inspector.rs`: the top bar, left dock (service,
    connection, autosave, stale; the tree), right dock (name field,
    summary, transform, detail, physical link, attributes, history,
    commands) and status bar, on the UI kit; refreshed only when
    `CadDocument.revision` changes.
- Outside `cad/`: `app/mod.rs` (`ViewerMode::Cad`, `ModeScope::Cad`, the
  Look, `Launch.cad`), `app/switch.rs` (`Document::Url`, `viewer_mode`'s
  `url`, `Documents.cad`, the Cad arm of `prepare`, `leaving_blockers`,
  `leaving_note`, `leave_cad`, `mode:cad`), `app/actions.rs` (the "cad"
  feature, `fallback`), `launch.rs` (`LaunchKind::Cad` for `*.rcad`),
  `main.rs` (positional `.rcad`, `--cad-url`, `--validate-only`).

### CadAction

REST and `system_ui` names: `state`, `cad_state`, `cad_open`, `cad_select`,
`cad_patch`, `cad_delete`, `cad_undo`, `cad_redo`, `cad_save`,
`cad_command`, `cad_op`, `cad_refresh`, `cad_fit`, `cad_physical`,
`system_ui` (controls `cad:undo`, `cad:redo`, `cad:save`, `cad:refresh`,
`cad:fit`, `cad:physical`, `cad:delete`, `cad:node:<id>`,
`cad:visible:<id>`, `cad:locked:<id>`, `cad:disabled:<id>`,
`cad:material:<id>:<mat>`, `cad:command:<id>`). The panel's buttons,
`system_ui` and the keys read one list (`panel::controls`), tree rows and 3D
picks write the same `CadSelect`, and `cad::tests` is written to check that
every control's REST form parses back to the value a click writes. Mutations (patch,
delete, undo, redo, save, command, op) run one at a time on a Dedicated job
through RoboCAD's routes; a REST caller waits for RoboCAD's answer.

### Decisions

- **The switch never waits on RoboCAD.** Entering CAD mode is immediate;
  connecting (or starting a service, up to 120 s for a large file) is CAD
  mode's own job, shown in the top bar and left dock and in `cad_state.connection`. *Rejected:*
  a switch that loads first (as robot mode does): a slow or absent RoboCAD
  would hold the switch for minutes. *Revisit if* callers need
  `viewer_mode` to answer only once connected.
- **Opening a file starts a headless service; `/open` is not used.**
  RoboCAD's GUI `/open` opens a *new window on another port*, so the
  attached URL would not show the new document. `cad_open {path}` starts a
  new headless service; `cad_open {url}` attaches. `open`/`load_status`/
  `cancel_load` are in the client for a later epic.
- **Unsaved edits.** The viewer never saves for you. Leaving CAD mode or
  `cad_open` is refused while a self-started service reports `dirty` (or
  its state can't be confirmed while it still runs); closing the window
  detaches such a service (left running, URL logged) instead of stopping
  it. An attached RoboCAD keeps its edits, and the switch's message says
  so. *Rejected:* auto-save (writes the user's file unasked), RoboCAD's
  Save/Discard prompt (a modal flow the viewer does not have).
- **Network on `Pool::Dedicated`, not `Pool::Io`.** The assignment asked for
  mesh fetches on Io; the jobs module's pool rule puts network on
  Dedicated, because a request may wait up to its timeout and Io has at
  most 4 threads shared with the asset server. Fetches are capped at 2 in
  flight; building stays on Compute. *Revisit if* thread churn shows up.
- **Timeouts.** Reads 30 s, the poll 5 s (so it exits promptly), edits
  130 s (longer than RoboCAD's 120 s GUI wait, which cancels only a request
  not yet started), so an edit is not reported failed and then applied; a
  timed-out edit says RoboCAD may still apply it.
- **Every revision refetches every visible body's mesh** (RoboCAD exposes
  no per-node geometry stamp). Known cost on large assemblies; the old mesh
  stays drawn until the new one arrives.
- **Keys** (RoboCAD's keymap.json; Ctrl is Cmd on macOS, so Control or
  Super): Cmd/Ctrl+Z undo, Cmd/Ctrl+Shift+Z redo, Delete/Backspace delete
  the selected node, Home fit, Cmd/Ctrl+S save. No clash: no key is read in
  every mode (`CorePlugin`, the switcher and the kit read none), and the
  other modes' keys run only in their modes. Keys are ignored while the
  name field has focus (`CadInputFocus`; `keys` runs after
  `panel::name_entry`), and a key whose button is disabled shows the
  button's reason instead of sending.
- **The loopback refusal text** now reads "the local servers listen on
  127.0.0.1 only …" (was "the hardware servers …"); hardware tests check
  the unchanged substring. The hardware client's own control-character
  check now runs before the loopback check, so a client wrong in both ways
  reports the token first (nothing is sent either way).
- **`cad_fit` is the native camera only**: RoboCAD's view is never moved
  and geometry never changes for display.

### Review findings (combined pair-reviewer pass, five reviewers, then two more)

Fixed (by reading; unverified until the verification pass): two app tests that counted five modes; orphaned service when the
window closed while starting (the child now sits in a `ChildSlot` the
document owns from spawn); a lost connection could clear the unsaved-edit
guard; concurrent selection pushes could land out of order; a failed mesh
was never retried; the poll could linger 150 s after its document closed;
attach accepted a non-RoboCAD server; `/selection` errors were dropped;
edits timed out before RoboCAD's own wait; `NaN` from Python failed whole
answers; one malformed node hid the document; `mesh()` hid a missing route;
"Hidden by parent" shown for a node disabled itself; an instance's `source`
shown as a provenance chip; "Loading…" while lost or after a failed fetch;
no in-flight state for Physical; a GUI with no commands called headless;
two `system_ui` control lists (now one) missing lock/disabled/material;
a one-frame name-focus gap; empty names sent. Second pass: the poll's 5 s timeout
also cut `/doc` (now only `GET /` and `/selection`); a refresh without a
poll left the saved state looking clean; a restarted connection kept the old
service's selection and tree key; an edit whose connection closed after
sending did not say RoboCAD may still apply it. Found by reading during
integration: the physical fetch's end did not refresh the panel when its
generation was stale.

Rejected or recorded: `p.is_file()` on the UI thread in `prepare`/
`cad_open` (one `stat`, as every other mode's switch does); SIGKILL of a
self-started service can orphan RoboCAD experiment workers in their own
process groups (no CAD-mode route starts one; a later epic that does must
stop it gracefully); service logs (`robocad-api-<pid>-<port>.log` in the
temp dir) are kept for diagnosis. A self-started service is accepted
only when `GET /` reports `app: "robocad"` serving the opened file
(`sync::serves`), so another service that took the chosen port in the
`free_port` race is refused rather than edited.

### Verification checklist

- `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- `cargo test -p sim-spatial --lib`, in particular
  `app::tests::build_cad_build_tears_down_the_cad_document_and_keeps_shared_state`,
  `app::tests::rest_refuses_commands_of_another_mode_by_name`,
  `app::tests::every_mode_control_resolves_to_a_switch`,
  `app::tests::every_capability_parses_into_its_action_and_every_parsed_command_is_registered`,
  `cad::tests::*`, `cad::mesh::tests`, `cad::inspector` tests,
  `jobs::tests::*` (the `ChildProcess` tests and
  `threads_are_started_only_in_jobs`), `launch::tests`.
- `cargo test -p sim-runtime --lib hardware_client` (unchanged requests).
- `cargo test -p sim-runtime --lib cad_client`.
- `cargo check -p sim-app` (dropped since fold-sim-app: the crate is retired); `cargo check -p sim-web --target
  wasm32-unknown-unknown` (the new modules are `cfg(not(wasm32))`).
- Then the user's [docs/cad-checklist.md](../cad-checklist.md).

### Reading trace (open, select, patch, undo, save)

Line numbers are as of a4fe42d3. Since cad-select-transform, `cad/sync.rs`
is `cad/sync/` (`mod.rs`; `launch.rs` holds `self_start`; `selection.rs`
holds `push_selection` and `detail`), the 3D pick is `cad/pick.rs`
`pointer` and selection is `cad::selection::select` (see
[CAD selection and transform](#cad-selection-and-transform-2026-10-01)).

- **Open.** `sim-spatial FILE.rcad`: `launch::classify` → `LaunchKind::Cad`
  (main.rs:342) → `cad_mode` (main.rs:205) → `CadDocument::new` →
  `app::run`. Or the switcher / `mode:cad` / `viewer_mode` →
  `switch::handle` → `prepare`'s Cad arm (app/switch.rs:650) →
  `Prepared::Now` → `arrive` inserts the document. OnEnter(ModeScope::Cad)
  `sync::enter` (cad/sync.rs:258) → `start` (:56): a Dedicated job runs
  `self_start` (:108: interpreter, free port, `ChildProcess::spawn` into
  the slot, `wait_until_live`) or `CadClient::health` → `receive` (:275) →
  `finish_connect` (:296) → `spawn_poll` (:162) → `poll_loop` (:185) →
  `take_snapshot` (:373) sets doc, stale, selection → `mesh::sync`
  (cad/mesh.rs:242) fetches and builds meshes; the panels refresh on
  `revision`.
- **Select.** Tree row (cad/tree.rs:119) or 3D pick (`mesh::pick`,
  cad/mesh.rs:431) → `Act<CadAction::CadSelect>` → `panel::buttons` /
  observer → `actions::apply` (cad/actions.rs:125) → `select` (:293) sets
  the selection now → `sync::push_selection` (cad/sync.rs:550) `PUT
  /selection` on a job; `detail` (:572) fetches `GET /nodes/{id}`.
- **Patch.** Inspector chip / `cad:visible:<id>` / REST `cad_patch` →
  `handle` (cad/actions.rs:162-170) → `edit` (:211) → `sync::start_edit`
  (cad/sync.rs:632, `EDIT_TIMEOUT`) → `CadClient::patch` (`PATCH
  /nodes/{id}`, RoboCAD's command layer, one undo step) → `finish_edit`
  (:498) → status, REST answer, `refresh` (:518) → the poll refetches
  `/doc`.
- **Undo.** Button / Cmd+Z (`keys`, cad/keys.rs:28, only when `cad:undo`
  is ready) / `cad_undo` → cad/actions.rs:181 → `POST /undo` → as Patch.
- **Save.** Button / Cmd+S / `cad_save` → cad/actions.rs:187-193 → `POST
  /save` (RoboCAD writes its file) → as Patch; the poll sees `dirty`
  false and the header says Saved. Leaving: `leaving_blockers`
  (app/switch.rs:485; CAD clause :511) → `CadDocument::switch_blockers`; `leave_cad`
  (app/switch.rs:856) removes the document (the slot's child is stopped,
  or detached if dirty) and `cad::clear`.

## Fold in sim-app (2026-09-30)

Batch fold-sim-app (default order item 11) brought sim-app's last two
user-facing capabilities into the one app and deleted the crate. There is
now one viewer binary and one Bevy app. The ledger is
[docs/sim-app-parity.md](../sim-app-parity.md): 58 rows, 44 done by
reading, 11 deliberately different, 3 done for v2 and deliberately
different for v3, none open. Written and reviewed by reading, then
*verified at 80b5997e* (sim-spatial lib tests 172 passed, 1 ignored;
workspace `--locked` check clean). Paths are `crates/sim-spatial/src/`
unless they name another crate.

### Shape

- **Phenomena mode** (`phenomena/`): `ViewerMode::Phenomena` with
  `ModeScope::Phenomena`, `PhenomenaPlugin` (keys, panels, scene) over
  `PhenomenaCorePlugin` (the action, the gallery, its frames and the REST
  snapshot; no window needed, so the switch test runs it).
  - `run.rs`: the "phenomena-run" `RunThread` builds and owns every
    `Box<dyn Exhibit>` (`exhibits::all()` on the thread) and advances the
    shown one with phenomena_app.rs `advance()`'s rules, factored into the
    pure `Pacing::step`: real time per ~60 Hz tick clamped to 0.05 s, ×
    `time_scale()` × speed, whole grid steps with the remainder carried, an
    error (or panic) kept verbatim until a switch, one chart sample per
    1/30 s keeping 1800, `SIM_VIEWER_STATS` printed on the thread. Frames
    are `Stamped`; select, next, previous, knob and reset bump the
    generation (`Op::switches`) and the UI takes only `latest(requested)`.
  - `actions.rs`: `PhenomenaAction` (`state`, `phenomena_state`,
    `phenomena_select`, `phenomena_next`, `phenomena_previous`,
    `phenomena_knob`, `phenomena_reset`, `phenomena_pause`,
    `phenomena_speed`, `system_ui`) and its one `apply` in Actions. REST
    commands that change the exhibit answer Pending until the shown frame
    includes them, then answer `phenomena_state`.
  - `keys.rs`: sim-app's bindings (see the ledger's P1–P7).
  - `panel.rs`: kit docks: exhibit list, title, summary, verdict, the knob
    slider with range and unit and −/+ buttons, readouts as property rows,
    time, speed, pause/reset, the error in the danger colour, and the strip
    chart through `chart::rasterize_span` with `chart_label`s. Real glyphs
    (IBM Plex Sans); `glyphs` maps only what the font lacks.
  - `scene.rs`: one camera at sim-app's pose, a shadowed light, one entity
    pool (sphere, cylinder, cuboid) and gizmos for lines, arrows and
    polylines, drawn in Present.
  - `mod.rs` `leave` (OnExit, registered by `app::switch`): the shown
    exhibit's number goes to `Documents::exhibit`, the gallery is dropped
    off the UI thread (its run thread joins within `JOIN_BOUND`, or is
    detached and ends at its next check), the pool and panels go.
- **Planar v2 files in Robot mode** (`robot_planar.rs`): a v2 file
  (`simrobot_version` < 3, the shared rule) is read as sim-phenomena's
  `CadModel` from the bytes the source worker read (`robot.rs`
  `load_file_bytes`; `robot_source::FileModel::{Physical, Planar}`) and run
  by the "robot-run (planar v2)" `RunThread`, which builds through
  `sim_phenomena::scenarios::cad_robot::build_planar` (the one planar build;
  `AnyRobot::load` and `run_file` call it too) and paces with the CAD
  scene's rule. The header shows `HEADER_LABEL`; `robot_state.format`
  carries `{version, name, fidelity}` (`FIDELITY`, stated from
  `CadRobot::build`), and v3+ files report `physical v<N>`. Run, pause,
  step, reset, speed, joint select and target, tip contacts, outlines and
  COM dots, the watch and reload; everything without a v2 meaning is
  refused by name (`robot_planar::UNAVAILABLE`, live motor sync included).
  The reload worker tries the shared build once, so a file that cannot
  build (a closed joint loop, which the build now refuses by name instead
  of never finishing) keeps the last good model and its run; the selected
  joint is carried across a reload by name. `robot_state.run` also carries
  `robot_run`'s `chunk_s` and `rtf` keys (`compute_limited` is null: the
  planar pacing cap, not compute, limits it). Verification pass
  2026-10-01.
- **Child processes** (`jobs/child.rs`): `spawn_detached` and
  `open_in_browser`; `reap_child` is private. The two `sim-viewer` launches,
  the builder's source links and the lesson's `open_url` use them.
- **sim-app deleted**: the crate, its workspace member, its Cargo.lock
  entry and the 51 packages only its default-feature Bevy needed;
  `sim-runtime/build.rs` and CI no longer name it. RoboCAD's
  `simbridge.viewer_command` no longer falls back to it.

### Decisions

- **One run thread owns all exhibits** (built there), not one thread per
  exhibit or a rebuild per selection: switching is instant and the UI
  never builds an exhibit. Revisit if the exhibits' memory matters.
- **Light scene background** for Phenomena mode (sim-app's clear colour and
  ambient, in `app::look`): the exhibits paint with dark ink for a light
  board. The panels are kit docks with their own surfaces.
- **Orbit convention**: right-drag rotates, middle or Shift+right-drag pans,
  wheel zooms, only over the 3D area; sim-app's left-drag orbit is
  deliberately dropped (left click belongs to the panels). No camera is
  shared by every mode today, so the mode has its own in that convention.
- **Chart**: the kit's chart image through `rasterize_span` instead of a 3D
  gizmo board; `rasterize_span` pads 8 % (sim-app padded 10 %).
- **Keys**: sim-app's bindings, no clash: nothing outside a mode reads keys
  (`app/`, `ui_kit/`, the switcher), Bevy's `DefaultPlugins` here add no
  Tab navigation, and the kit slider handles arrows only when focused.
  Keys are ignored while Cmd, Ctrl or Alt is held (a deliberate difference).
- **`PHENOMENA_EXHIBIT`** seeds `Documents::exhibit` at every launch, after
  `--exhibit`.
- **REST waits for the frame**: a changing phenomena command answers once
  the run thread has applied it, with the resulting state.
- **SIM_VIEWER_STATS** is carried over, measured on the run thread with
  sim-app's text.
- **v2 run starts paused at t = 0** when a file opens (sim-app ran at
  once), but a reload of a running v2 file starts the new run once built,
  so the CAD edit-save-watch loop keeps moving.
- **v2 Reset rebuilds the loaded model; Reload re-reads the file** (sim-app's
  R did the latter); both are actions robot mode already had.
- **v2 speed** goes through robot mode's `speed_target`, which refuses past
  ×8 / ×0.125 instead of clamping silently.
- **v3 keys unchanged**: robot mode's v3 run controls, jog and stress stay
  buttons, `system_ui` and REST (stress is key H); the arrow keys and Space
  are bound for v2 files only. Recorded as deliberately different rows.
- **Function made public / added in shared crates**:
  `cad_robot::build_planar` with `PLANAR_BANDWIDTH_HZ`/`PLANAR_DAMPING_RATIO`
  (a refactor of `AnyRobot::load`'s v2 branch, no behaviour change). Message
  text only: `PhysicalModel::parse`'s v2 refusal names `sim-spatial --robot
  FILE`, and the planar build's warnings say a dropped joint's child is not
  attached through it (a fixed joint is now named too).
- **Spawns**: no raw spawn is left outside `jobs`. `main.rs` still builds
  the two `sim-viewer` `Command`s it hands to `spawn_detached` (allowlisted
  in `processes_are_started_only_in_jobs` with a count of 2). Their errors
  now read "Could not open the schematic: could not start sim-viewer:
  {os error}. …" (build mode) and "Could not open schematic: could not
  start sim-viewer: {os error}. …" (inspect mode): one more prefix than
  before; the OS error is kept.
- **Cargo.lock** was regenerated by cargo's resolver (`cargo metadata
  --offline` writes the lock before downloading sources): 51 packages
  dropped, no version changed.
- **Tonemapping**: CAD mode's camera now sets `Tonemapping::None` like every
  other mode (found by reading: without `tonemapping_luts` the default
  TonyMcMapface samples a placeholder LUT).

### Deviations

- Phenomena exhibits are rebuilt each time the mode is entered (the gallery
  is a mode resource); leaving during the build cannot cancel it, so it
  runs to the end on its detached thread.
- `robot_planar.rs` (826 lines with tests) and `robot/actions.rs` (898) are
  over the 800-line smell; `robot.rs` grew to 2,514. Splitting them is the
  split-large-files epic.
- The workspace `bevy = "0.19.1"` entry stays though nothing uses it.

### Review findings (four pair-reviewers, then fixes)

Fixed:
- A panic in an exhibit's `time_scale`/`grid`/`signal`/`time` ended the run
  thread silently with the last frame shown: the whole tick is guarded, and
  a stopped thread is reported (`phenomena_state.stopped`, the alert line).
- A click on the knob slider without moving it rebuilt the exhibit.
- `ExhibitRef::resolve` did not try an out-of-range number as a title
  fragment, as sim-app did.
- `PHENOMENA_EXHIBIT` was ignored on a later switch to phenomena mode.
- Texts that claimed one thread per exhibit, a hard 200 ms join and "the
  last minute of real time".
- v2 `FIDELITY` said dropped joints were "treated as fixed"; it now states
  what the build does (a dropped joint's child is not attached, or becomes
  the root; branch rerooting of every child; the nearly fixed ground root;
  the root contacts' placement; 2 ms sampled PD, joint damping, contact
  parameters, clamp order, ideal sensors, gravity axis), checked against
  `CadRobot::build` by a second reviewer.
- A save in CAD left a running v2 robot paused; a v2 → v3 reload restarted
  the run generation at 0; v4 files were labelled "physical v3"; robot
  mode's keys (v2's and C/J/F/H, =/−, G) fired with Cmd/Ctrl/Alt chords;
  `--validate-only` printed warnings twice; a knob-slider click compared
  unsnapped floats exactly.
- The hand-edited Cargo.lock would have failed every `--locked` job.
- With a v2 file open the leg mirror waited forever; it refuses by name.
- `lesson/mod.rs` `open_url` left a zombie per opened link.

Rejected or deferred, with reasons:
- The process scan does not look for `.status()`: it matches unrelated
  `status()` calls; any new `Command::new(` outside `main.rs` still fails
  the scan.
- The doubled error prefix of a failed `sim-viewer` launch is kept: it
  still names the OS error.
- `tools/claude-pair/checks.json` still runs `cargo check -p sim-app`: it is
  the coordinator's configuration, which this epic must not edit
  (reported to the orchestrator).
- `sim-runtime/build.rs` hashes `sim-spatial` into the runtime identity
  although its comment calls UI crates out of scope: unchanged here (it
  would change the gait-lab fingerprint rule).

### Verification checklist

- `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- `cargo test -p sim-spatial --lib`, in particular `phenomena::tests::*`,
  `app::tests::phenomena_commands_route_to_phenomena_mode_only`,
  `app::tests::build_phenomena_build_runs_the_gallery_and_remembers_the_exhibit`,
  `app::tests::every_capability_parses_into_its_action_and_every_parsed_command_is_registered`,
  `app::tests::every_mode_control_resolves_to_a_switch`,
  `app::tests::rest_refuses_commands_of_another_mode_by_name`,
  `robot_planar::tests::*`, `robot_source` tests, `robot::actions` tests,
  `jobs::tests::processes_are_started_only_in_jobs`,
  `jobs::tests::threads_are_started_only_in_jobs`,
  `jobs::tests::spawn_detached_keeps_running_reaps_and_names_a_failure`.
- `cargo test -p sim-domain-robot` (the v2 refusal message test) and
  `cargo test -p sim-phenomena` (the planar build refactor; CI skips
  `every_exhibit_runs_in_real_time`).
- `cargo check --workspace --all-targets --locked` (the regenerated lock).
- From `cad/`: `.venv/bin/pytest -q tests/test_simbridge.py`.
- `sim-spatial --phenomena --validate-only` and `sim-spatial --robot
  <a v2 file> --validate-only` (no window).
- The reading trace below.

### Reading trace (exhibit select, knob change, v2 open, v2 run)

- **Exhibit select.** Digit key (`phenomena/keys.rs:38`, `DIGITS` :35,
  writes `PhenomenaSelect` :58), a list row (`panel.rs:250` `buttons`) or
  REST `phenomena_select` → `phenomena/actions.rs:88` `apply` → `handle`
  (:111) → `op_for` (:137; resolved against the shown frame's titles at
  :140) → `gallery.rs:66` `send` (bumps the requested generation) → on the
  thread `run.rs:366` `run` → `Run::apply` (:274; `Op::Select` :283,
  `Pacing::switched`) → `Run::frame` (:309) published → JobResults
  `phenomena/mod.rs:127` → `gallery.rs:77` `receive` (`latest(requested)`)
  → REST answered by `actions.rs:162` `wait` once the frame includes the
  command → Present `scene.rs:164` `render`, `panel.rs:429` `refresh`,
  `panel.rs:556` `chart`.
- **Knob change.** ←/→ (`keys.rs`) as `PhenomenaKnob {steps}`, the slider's
  release (`panel.rs:261`, nothing if unchanged) or REST `phenomena_knob`
  → `apply` → `op_for` (:143) → `send` → `Run::apply` `Op::Knob` (:289) →
  `knob_target` (`run.rs:234`: clamp, round to the step) →
  `Exhibit::set_knob` on the thread (a panic is kept as the error) → frame
  → as above.
- **v2 open.** `sim-spatial --robot F` (main.rs:384 for a positional file)
  → `robot_mode` (main.rs:291) → `RobotView::open` → `SourceWatch::open`
  (`robot_source.rs:138`) → `check` (:85, a Compute job) →
  `load_file_bytes` (`robot.rs:119`; v2 → `robot_planar::load_bytes`,
  `robot_planar.rs:91`) → `watch` (`robot.rs:836`) / `receive` (:851) →
  `FileModel::Planar` (:909) → `install_planar` (:1047) → `PlanarView::new`
  (:1111) → `PlanarRun::spawn` (`robot_planar.rs:415`) → `Worker::run`
  (:223) → `build` (:249) → `cad_robot::build_planar`
  (`sim-phenomena/src/scenarios/cad_robot.rs:506`), paused at t = 0.
- **v2 run.** Run button, Space (`robot/actions.rs` `planar_keys`) or REST
  `robot_run` → `robot/actions.rs:495` `apply` → `check_planar` (:110) →
  `dispatch_planar` (:192) → `PlanarRun::act` (`robot_planar.rs:491`) →
  `Worker::command` (:322, Start) → `tick` (:300: 0.05 s × speed, 0.02 s
  grid, one step per tick) → `publish` (:369) → SimSync `planar_sync`
  (`robot.rs:1146`) → `PlanarRun::poll` (`robot_planar.rs:442`, current
  generation only) → Present `robot.rs:2439` `draw` →
  `robot_planar::draw` (`robot_planar.rs:627`).

## CAD selection and transform (2026-10-01)

Batch cad-select-transform (default order item 7, §9 phase 1, the first
half of the planned cad-tools epic; see §9 "Later CAD epics" 1) brought
RoboCAD's interactive selection and direct-transform workflows into CAD
mode. RoboCAD's command layer still does every edit: each drag release or
numeric Enter is exactly one `POST /ops/*` call, so undo and provenance
stay RoboCAD's; previews move display transforms and overlays only. The
ledger rows are in [docs/cad-parity.md](../cad-parity.md) (63 rows: 55
done by reading, 8 deliberately different; none open), the side-by-side
steps in [docs/cad-checklist.md](../cad-checklist.md) (CAD-19 to CAD-34).
**Written and reviewed by reading only; the verification pass builds and
tests it.** Paths are `crates/sim-spatial/src/cad/` unless they name
another crate.

### Shape

- **One Python addition** (`cad/robocad/api.py` `Service.edges`): `GET
  /nodes/{id}/edges?samples=N` (2..256) adds each edge's `points`, the
  polyline RoboCAD's viewport draws and picks (`kernel.sample_edges`);
  without the parameter the answer is unchanged. Pytest:
  `cad/tests/test_api_edge_samples.py`.
- **sim-runtime**: `cad_client` gains `FaceInfo`, `EdgeInfo` (with
  `points`), `VertexInfo`, `Solids` and `faces`, `edges(id, samples)`,
  `vertices`, `solids`; `units` is a port of RoboCAD's `units.evaluate`
  (`evaluate(text, angle, default_unit) -> Result<f64, UnitError>`,
  `try_evaluate`, `format_length`, `format_angle`; errors name the token
  and its character position).
- **Shared state** (`document.rs`): the selection is RoboCAD's items
  `[node, kind, index]` (`SelectionItem`), with `select_mode`
  (`SelectMode`), `hover`, `candidates` (the Alt menu), `tool` (`CadTool`)
  and `tool_state` (`transform::ToolState`); `commit_refusal(began)` is
  the one refusal for tool commits (an edit in flight, not connected, the
  shown document stale, or RoboCAD's revision changed since `began`).
- **The one handler** (`actions.rs`): `handle(action, call, cx: &mut
  Cx)` with `Cx { doc, meshes, topology, view, documents }`; the
  selection variants go to `selection::handle`, the tool variants to
  `transform::handle`. Every commit goes through `actions::edit` →
  `sync::start_edit` (a Dedicated job), the same path as Patch and Undo.
- `view.rs` `CadView`: the camera as matrices (model mm ↔ window pixels,
  cursor rays in RoboCAD's frame), refreshed in SimSync.
- `topology.rs` `CadTopology`: faces, edges (24 samples per curved edge,
  RoboCAD's default) and vertices per (node, revision) on Dedicated jobs
  (two at a time), for the selected nodes and, in a sub-body mode or a
  non-Select tool, every drawn body; another revision's data is dropped.
- `mesh.rs` keeps RoboCAD's tessellation per drawn body (`mesh_data`,
  `face_of`: Bevy's ray-cast triangle is RoboCAD's triangle, so
  `triangle_face` names the face), `drawn_revision`, `body_bounds`.
- **Selection** (`selection/`, `pick.rs`, `overlay.rs`, `keys.rs`,
  `sync/selection.rs`, `inspector.rs`): modes (B, Shift+B, E, V, P; the
  mode strip at the 3D view's top left), click (Shift extends, Ctrl
  toggles, empty space clears), hover (coalesced to 33 ms; edge and
  vertex searches on a Compute job, newest wins), box select (a kit
  rubber band past 6 px), the Alt menu (RoboCAD's 7×7 px neighbourhood:
  nine rays 3 px apart, nearest hit per ray), Select All, Invert, Same
  Material, Edges → Faces; Bevy gizmo overlays for edges, vertices,
  hovered and selected items; the inspector's face/edge/vertex/point
  section. Pushed with `PUT /selection {"items", "mode"}`, adopted from
  the poll's `GET /selection` (every kind; a GUI's mode).
- **Tools** (`transform/`, `numeric.rs`, `snap.rs`, `measure.rs`): G/R/S
  gizmo (RoboCAD's handles: centre within 10 px, axes and rings within
  14 px; Ctrl snaps 10 mm, 15°, 0.1), D push/pull and Shift+D offset (drag
  along the normal; non-planar faces offset), M measure (Shift keeps a
  measure node), Tab numeric entry, Escape cancel; the numeric bar at the
  3D view's bottom (tool · mode label, hint, navigation line, tool strip,
  readout, fields evaluated on every keystroke); live dimensions of the
  selected faces and edges and a double-clicked face's dimension
  (`set_diameter`, `set_distance`, `set_angle`); snapping (vertex,
  midpoint, centre within 12 px, then the 10 mm grid on z = 0, else free;
  Alt suppresses) with its marker and readout; tool cursors.
- **Preview life** (`transform/preview.rs`): the moved bodies' display
  transforms follow the drag; after a commit they stay until each body's
  mesh is drawn from a newer revision (that body then drops the delta),
  and are reset at once on a failed or refused edit. Escape ends a live
  drag (its preview is dropped); a released or committed preview waits
  for its edit, since the request was already sent. A drag leaves out
  locked nodes (RoboCAD's `Ops.transform` skips them) and is refused by
  name when every selected node is locked, so a body is never left offset.
- `sync.rs` became `sync/` (`mod.rs` connect, poll, edits; `launch.rs`
  `self_start`, `serves`, `accept_served`; `selection.rs` adoption,
  pushes, node detail). `lifecycle_tests.rs`: the service lifecycle
  without a window. `main.rs`: the CLI refusals: `--cad-url` with any of
  `NOT_IN_CAD_MODE` (`--select`, `--exploded`, `--connections`,
  `--compact`, `--annotations`) is a clap `conflicts_with_all`; a `.rcad`
  FILE is refused by `cad_mode_refusal` in `take_file`, before any
  window or service starts (clap cannot conflict on a positional's file
  extension).
- **Revision guards** (verification pass): a face index is read from the
  drawn tessellation only while it is at the shown revision
  (`CadMeshes::face_at`): picking, hover, measure and double-click give
  nothing, or say the body is being redrawn, while a mesh lags; live
  dimensions show only for a selection first seen at the shown revision;
  `selection::faces_of_edge` needs the mesh and topology at one revision.

### Decisions

- **The gizmo is the viewer's own, not Bevy's `TransformGizmoPlugin`.**
  Read in full (bevy_gizmos-0.19.1 `transform_gizmo.rs`, bevy_gizmos_render-0.19.1
  `transform_gizmo_render.rs`): its render plugin spawns the handle meshes
  and an overlay camera once at `Startup` when the plugin's settings exist
  (`transform_gizmo_render.rs:83-89`), which `app::scope_new_entities`
  would scope to the first mode and despawn on the first switch, and the
  overlay camera would draw in every mode; scale is per axis only (RoboCAD's
  `Ops.transform` scale is uniform); a drag starts on any raw left press
  (`transform_gizmo.rs:415`), also over a panel; it confines the cursor by
  default and runs in PostUpdate outside the action order. The native
  gizmo ports RoboCAD's (tools.py:234-385, viewport.py:1060-1132) on
  `Gizmos` and `CadView`. *Revisit if* Bevy's gizmo gains uniform scale and
  mode-scoped spawning.
- **Picking**: faces by Bevy's `MeshRayCast` on the UI thread (the cost
  Bevy's own picking backend pays every frame); edges and vertices by a
  screen-space search, hover's on a Compute job, a click's inline after
  culling bodies by their projected bounds. Edge/vertex occlusion compares
  with the first surface along the cursor ray (RoboCAD uses its depth
  buffer per pixel). Locked nodes are neither picked nor occluding, as in
  RoboCAD's pick pass. Box select runs inline on release (one projection pass).
- **Edges → faces** is computed from the drawn tessellation (a face whose
  triangle side lies along the edge's polyline, within one sagitta plus
  the mesh tolerance and parallel to it): RoboCAD's `kernel.faces_of_edge`
  has no route, and the one Python change was reserved for edge polylines.
- **The selection mode is the viewer's**: a headless RoboCAD stores only
  the items (api.py `set_selection`) and answers no mode; a desktop
  RoboCAD's mode is adopted when it changes there. Known limit; no second
  Python change.
- **Pivot**: the first selected node's `pivot`, else (one node) its mass
  centroid from the inspected detail at the shown revision, else the
  centre of the selected bodies' mesh bounds. RoboCAD uses the
  selection's mass centroid; multi-node selections differ.
- **Revisions**: a drag captures RoboCAD's revision at the press, a typed
  entry when the field takes focus; the commit is refused by name if it
  changed. A push/pull target's face is found again after an edit
  (RoboCAD's `match_face` rule) or refused by name.
- **Keys** (RoboCAD's keymap): B, Shift+B, E, V, P, Ctrl/Cmd+A,
  Ctrl/Cmd+Shift+I, Ctrl/Cmd+Shift+M, G, R, S, D, Shift+D, M, Tab, Escape.
  No clash: S and M act only without Ctrl/Cmd (Ctrl+S saves,
  Ctrl+Shift+M is Same Material); no key is read in every mode.
- **Deliberately different** (also in the ledger): measure does not copy
  to the clipboard (the value shows in the bar and REST answers it); Same
  Material refuses by name without a material; a typed rotation after
  dragging the X ring turns about X (RoboCAD's `axis_index or 2` turns
  about Z); the footer has no ms/frame; the double-click dimension needs
  face mode; the centre snap and the same-edge radius work (both
  unreachable in RoboCAD, noted in the ledger).

### Review findings (four pair-reviewers by area, then fixes)

Fixed: `topology::sync` wrote `RequestRedraw` unconditionally and would
panic in the windowless core plugin; preview functions re-exported more
widely than declared (E0364); test-only re-exports in `sync/mod.rs`
(unused-import warnings, now `#[cfg(test)]`); a multi-body preview
double-moved bodies whose new mesh had landed; a push/pull face index
went stale after an edit; live dimensions rescanned meshes every frame
(now cached by selection, tool and epochs); a typed entry took its
revision at Enter; grid snap rounded half away from zero (now half to
even, as Python); Alt candidates listed occluded bodies; edges → faces
took the far side of thin walls; adopted selections could name deleted
nodes and pruning was not pushed; `curve` items were refused from REST but
adopted from RoboCAD; a hovered body showed nothing; spec texts for
`cad_state` and `system_ui`; an oversized `samples` gave 500 (now 400); the
pytest could not fail on the message; tool cursors and the navigation line
were missing.

Rejected: a blank `?samples=` returns the unsampled answer (`parse_qs`
drops blank values; the Rust client always sends a number); the lifecycle
test's pid-reuse window (only on the clean-stop test's failure path,
after waiting up to 2 s, sequential pids on macOS; a `ps` check would need another process build site); selection
overlays do not follow a preview (they return when the new meshes land).

### Verification checklist

- `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- `cargo test -p sim-spatial --lib`, in particular `cad::tests::*`,
  `cad::selection::tests::*`, `cad::pick::tests`, `cad::overlay::tests`,
  `cad::transform::tests::*`, `cad::transform::dimensions::tests`,
  `cad::snap::tests`, `cad::measure::tests`, `cad::view::tests`,
  `cad::lifecycle_tests::*`, `app::tests::*` (the registry and dispatch
  cross-check and the CAD switch test), `ui_kit::tests::*`,
  `jobs::tests::threads_are_started_only_in_jobs` and
  `jobs::tests::processes_are_started_only_in_jobs`.
- `cargo test -p sim-spatial --bins` (main.rs: the CLI conflicts).
- `cargo test -p sim-runtime --lib cad_client` and `cargo test -p
  sim-runtime --lib units`.
- `cd cad && .venv/bin/pytest -q tests/test_api_edge_samples.py` (9 passed
  in 4.85 s on 2026-10-01, the one check run in this epic).
- `cargo check -p sim-web --target wasm32-unknown-unknown` (units is not
  cfg-gated; std only).
- Then the user's [docs/cad-checklist.md](../cad-checklist.md) CAD-19 to CAD-34.

### Reading trace (face pick → push/pull commit → undo)

- **Face pick.** Shift+B (`keys.rs:79` → `cad:mode:face`) →
  `CadSelectMode` → `selection::handle` (`selection/mod.rs:83`) clears and
  pushes (`publish` :108 → `sync::push_selection`, `sync/selection.rs:89`,
  `PUT /selection {"items": [], "mode": "face"}` on a Dedicated job). A
  click in the Select tool: `pick::pointer` (`pick.rs:308`) →
  `candidates_at` (:292) → `surface_items` (:182, `MeshRayCast` on
  `CadBody` → `CadMeshes::face_of`) → `Act::ui(CadSelect {items:
  [[id, "face", f]]})` (:373) → `actions::apply` → `selection::select`
  (`selection/mod.rs:182`) → pushed as above; `topology::sync` fetches
  the body's faces for the inspector and the tool.
- **Push/pull commit.** D (`transform/mod.rs` `keys`) → `CadTool
  {push_pull}` → `transform::handle` (`transform/mod.rs:578`) targets the
  selected face. A drag: `push_pull::tool` (`transform/push_pull.rs:140`)
  captures RoboCAD's revision at the press, previews a line and the
  shifted outline (display only); release → `release_action` (:121) →
  `CadPushPull {node, face, distance, revision}` → `transform::handle`
  (:582) → `commit::commit` (`transform/commit.rs:234`):
  `commit_refusal(revision)` (`document.rs:607`), `op_for` (:185) →
  `push_pull_call` (:106) → `send` (:216) → `actions::edit`
  (`actions.rs:402`) → `sync::start_edit` (`sync/mod.rs:543`): one `POST
  /ops/push_pull {"args": [node, {"node", "face"}, distance]}` →
  RoboCAD's `Ops.push_pull` (one undo step "Push/Pull") → `finish_edit`
  (`sync/mod.rs:472`) → `refresh` (:498) → the poll refetches `/doc`; the
  meshes and topology refetch at the new revision; the preview clears
  when the body's new mesh is drawn (`transform/preview.rs`).
- **Undo.** Cmd+Z (`keys.rs`) / the Undo button / `cad_undo` →
  `actions.rs:371` → `edit` → `POST /undo` (RoboCAD undoes "Push/Pull")
  → as above.


### Disk

Free space was 18.49 GiB before the epic, below the 20 GiB build baseline.
Removed 2026-10-01 (regenerable build output only; no `cargo clean`;
`runs/`, `.claude-pair` and captures untouched), measured 27.59 GiB free
afterwards:

- 602 superseded incremental sessions in `target/debug/incremental/*/`
  (every crate directory holding two finalized `s-*` sessions kept only
  its newest, as rustc's own collection would): 10.19 GiB.
- The deleted sim-app crate's build output: `target/debug/incremental/sim_app-*`
  (3 directories) and `target/debug/deps/sim_app-*` and `libsim_app-*`
  (320 files): 0.44 GiB.
- 9 superseded incremental sessions in
  `target/wasm32-unknown-unknown/debug/incremental/`: 0.18 GiB.
- 608 superseded workspace-crate artifacts in `target/release/deps/`
  (an older hash of the same crate and file kind, more than three days
  older than the newest, which was kept): 2.76 GiB.

## Target shape

### 1. One app, modes as states

- One `App`, built in one place.
- Modes are a Bevy `States` enum: Build, Robot, Lessons, Place and Inspect.
  - Each mode is a plugin whose systems run under `in_state(..)`.
  - `OnEnter` sets up the mode's scene and UI; `OnExit` tears them down.
- Launch flags only choose the initial mode and document. The user can switch
  modes in the running window, and shared state (models, library, selection,
  annotations) survives the switch.
- `sim-app`'s scenes (phenomena exhibits, CAD view) become modes of this app, or
  are retired once parity is shown by tracing their workflows in code.
  *Done by fold-sim-app (2026-09-30), verified at 80b5997e:* the
  gallery is Phenomena mode, the CAD view's planar v2 files open in Robot
  mode, and sim-app is deleted ([ledger](../sim-app-parity.md)).
- *Status:* in place since one-app-modes (see [One app](#one-app-2026-09-30)),
  verified at 7da1216e: `app::run`, `ViewerMode` with the `ModeScope` and
  `SpatialScreen` computed states, setup on each scope's `OnEnter`, teardown
  by `DespawnOnExit<ModeScope>` and the scopes' `OnExit`, and one switch
  handler. What survives a switch: the builder, the display-model library,
  the fonts, the REST server, the documents each mode reopens, and the
  workspace root. Selection and annotations are still per mode (§7).
  `sim-app` is folded in (epic order item 11, see
  [Fold in sim-app](#fold-in-sim-app-2026-09-30)): there is one viewer
  binary and one Bevy app.

### 2. Plugins and ordered system sets

- **One plugin per feature**, in its own folder:
  - `src/<feature>/mod.rs` and `plugin.rs`
  - `actions.rs`, `ui.rs` and `jobs.rs` as needed
- **One pipeline of named system sets**, shared by all features:
  1. Input
  2. Actions
  3. JobResults
  4. SimSync
  5. Present

  A feature adds systems to these sets; it never orders itself against another
  feature's private systems. *Status:* `ViewerSet` is declared and ordered
  once (`app::ModesPlugin`); since the action layer each mode's input
  mappings sit in Input (after the one REST poll) and its apply system in
  Actions. The builder and lesson SimSync chains still order themselves
  against the spatial view's `update_parts` and `camera_viewport` (display
  ordering, left for the UI kit epic).
- **Files over about 800 lines are a smell.** Split them by responsibility when
  you touch them.

### 3. One action layer

- Every user intent is a typed action (an enum per mode or feature), validated
  in one handler.
- Buttons, keyboard, `system_ui`, REST and scripts all produce the same actions.
  UI callbacks contain no logic.
- Actions travel as Bevy **Messages** (`app::actions::Act<A>`: the action
  and its `Origin`, REST reply token, UI or quiet), written in
  `ViewerSet::Input` and drained once per frame by the action type's one
  apply system in `ViewerSet::Actions`. Observer triggers are kept for
  pointer events on entities (picks, drags, screenshots); a pick observer
  only writes its mode's action. Undoable actions go through the shared
  undo history.
- **Gestures.** A discrete intent from a pointer gesture commits as the
  same action value REST produces: the lesson timebar writes
  `LessonAction::SeekTo` per press or drag position (the REST
  `lesson_scene` seek's action; `rewind` is true only for the press that
  starts a drag, so REST never counts rewinds), a slider writes
  `LessonAction::Slider` on release (the REST slider's), the narration bar
  `NarrateAction::Seek` (REST `lesson_narration`'s), a click on a lesson
  chart `KeepMoment` (its hover preview stays local and is undone on leaving),
  a part or net click the mode's selection or pick action. A drag may keep a local preview
  (`slider_drag`). Freehand sketch strokes and typing into an open draft
  are input editing (Enter and Escape are the draft's submit and drop
  actions); orbit, pan, fly, wheel scroll and placement drags are
  navigation and stay in SimSync.
- REST answers through **reply tokens** (`app::actions::Replies`): the poll
  writes the action once with a token kept in the `sim_api` continuation
  and answers Pending until the handler writes the outcome; a handler that
  answers Pending is re-applied each frame with its own continuation
  (`InFlight`) and sees a REST cancel as `Call::cancelled`.
- `sim_api` capabilities are generated from the action registry
  (`app::actions::registry`), and a lib test checks the registry against
  what each action type parses, so the REST surface can't drift from the
  UI. *Status:* done 2026-09-30, verified at 90c65c86.

### 4. One background-work abstraction

- A `jobs` module (`src/jobs/`) owns all off-thread work:
  - **One-shot jobs** (`jobs::Job`, `jobs::Latest`): load, scan, read, write,
    compute, lay out, generate. A closure runs once and returns
    `Result<T, String>`. It receives a `Ctx` with the cancel token
    (`cancelled()`, or `cancel_flag()` for shared-crate functions that take
    `&AtomicBool`), a progress sink (fraction, steps done/total, a message)
    and, for streamed work (`Job::streaming`), `emit` for partial results.
  - **Long-lived workers** (`jobs::RunThread<Cmd, S>`): simulation sessions,
    robot runs, gait and playback clocks, the placement validator. One named
    OS thread each, with a command channel in and a shared snapshot `S` out
    (`Stamped` snapshots: `latest(generation)` never returns an older one).
- Every job has progress, cancellation, a generation stamp (stale results are
  dropped) and a surfaced error. A panic in a job closure becomes an error
  naming the job ("… ended without a result (…)."); a job cancelled before it
  started reports that instead of never answering.
- One system (or one poll point) per feature applies finished results; a
  feature keeps its `Job` in its state and polls it there.
- `thread::spawn` and `thread::Builder` appear nowhere outside `jobs`; a lib
  test scans `src/` and names any offending file and line.

**Pools** (Bevy 0.19.1 `TaskPoolOptions::default()`: `IoTaskPool` and
`AsyncComputeTaskPool` each get 25% of the cores, at least 1 and at most 4
threads, so a 2-core machine has one of each; the asset server loads on
`IoTaskPool`):

| `Pool` | Runs on | For |
|---|---|---|
| `Io` | `IoTaskPool` | file reads, writes, directory scans (short; blocked on the local disk) |
| `Compute` | `AsyncComputeTaskPool` | CPU work of up to a few seconds: layout, compile, parse and load, rasterise, context builds |
| `Dedicated` | its own named thread | anything that may hold a thread for longer: network, bench and model API requests (narration, the lab bench), studies and replays that fan out their own workers, scene recordings, the lesson model |

The pools are created on first use with `TaskPoolOptions::default()
.create_default_pools()`, which calls each pool's `get_or_init`, so a job
started before the `App` (tests, `--validate-only`, headless) and
`TaskPoolPlugin` create the same pools and whichever runs second reuses them.

**Drop policy.**
- Dropping a `Job` cancels it: the token is set (a running closure sees it at
  its next check) and a pool task that has not started never runs (dropping a
  Bevy `Task` cancels it). Replacing a feature's job (a newer request) is how
  superseded work is cancelled; `Latest` does this and returns only the
  newest generation's result.
- **Saves and writes are `complete_on_drop`**: the pool task is detached and
  the token is never set by a drop, so the recording writer and the placement
  commit finish even if their owner is gone.
- **`RunThread` drop** closes the command channel (the stop signal every
  worker loop already handles) and waits for the thread to exit, at most
  `jobs::JOIN_BOUND` (200 ms; every worker loop checks its channel between
  ticks, and an idle one is blocked on it and wakes at once). The wait is a condvar with a timeout on an exit flag
  set by a drop guard in the thread (so a panic sets it too); then the thread
  is joined, or past the bound detached with a warning (it exits at its next
  check). The placement validator uses bound zero: a release or cancel never
  waits on a validation. A replaced worker's snapshots are never applied: the
  new worker's owner starts at a later generation or has its own snapshot.
- **Helpers**: `jobs::spawn_detached(name, command) -> Result<u32, String>`
  starts a process whose lifetime is not tied to ours (the linked
  `sim-viewer` window) and `jobs::open_in_browser(url)` opens an http(s)
  link with `open`/`xdg-open` (anything else is "not a web link: {url}");
  both are `ChildProcess::detach`ed, so a reaper thread waits for the
  process and it never lingers as a zombie. Nothing outside `jobs` starts a
  process (`jobs::tests::processes_are_started_only_in_jobs`; `main.rs` only
  builds the `sim-viewer` command it hands to `spawn_detached`).
  `jobs::drop_off_thread` drops a value whose drop is slow or joins threads
  (the builder replaced by Open system) away from the UI thread.

### 5. Simulation boundary

- The viewer consumes shared runtime APIs (`sim_runtime`, `sim_system`,
  `sim_domain_*`) and observes their frames. It never advances physics except
  through those APIs, on simulation time, independently of rendering
  (`AGENTS.md`).
- Display-only state (layout, camera, overlays, grid) is kept apart from model
  and CAD state and never mutates source geometry.

### 6. One UI kit

- The `ui_kit` module (`src/ui_kit/`) provides: text, header, tab strip,
  buttons in every look (primary, secondary, ghost, danger, tab, chip,
  segment), inspector sections, property rows, list items, text-entry
  styling, docks, scroll areas, sliders, pointer surfaces and chart images.
  Its contract is the comment at the top of `ui_kit/mod.rs`.
- Behaviour comes from Bevy's own facilities: `bevy::ui::Button` and
  `Interaction` for buttons, `bevy_ui_widgets::Slider` for sliders,
  `AccessibleLabel`/`AccessibilityNode` for accessibility. Toasts and dialogs
  are not built yet (no mode needs one).
- Theme tokens live in one place (`ui_kit/theme.rs`), from the builder's
  palette and metrics; a lib test forbids second definitions.
- Features compose these widgets instead of building their own `Node` trees for
  common UI. Widgets take the typed action as a component and hold no intent.
  Accessible labels go on interactive widgets.

### 7. Documents, selection, annotations

- One document resource per open file, with a revision number.
- One selection model, and one annotations and discussions service. Every mode
  uses them.

### 8. Hardware front end in the native viewer

*Decided 2026-09-30; built 2026-09-30 (batch hardware-front-end, see
[Hardware front end](#hardware-front-end-2026-09-30)), pending the user's
hardware checklist.* The browser's calibration and hardware pages
(`web/viewer/calibration-ui.mjs` with `actuator-motion-view.mjs`,
`calibration-mirror.mjs`, `hardware-sync.mjs`) have a native front end in
`sim-spatial` with exact feature parity, ledgered row by row in
[docs/hardware-parity.md](../hardware-parity.md).

- **Placement.** A dock inside Robot mode, not a mode of its own: the
  mirror poses Robot mode's own robot, live sync streams the targets of
  Robot mode's own run, and the page lived beside the robot viewer too. The
  header's "Leg calibration" button shows it; `--hardware URL` opens and
  connects it at launch. `robot/hardware/` is built by `RobotPlugin`; its
  `Hardware` resource exists only while Robot mode is entered.
- **One client** (`sim_runtime::hardware_client`, shared crate, not the
  viewer):
  - loopback only (`Endpoint::parse` accepts `http://127.0.0.1:PORT` and
    `http://localhost:PORT`, connected as 127.0.0.1; `[::1]` and anything
    else is refused before a byte is sent: both servers bind IPv4
    127.0.0.1). Plain HTTP/1.1, one request per connection, with a 500 ms
    connect timeout and read and write timeouts (10 s for ordinary requests,
    longer than the server's own 8 s hardware wait; 12 s for STOP, which the
    server answers only after its worker finishes, up to 8 s), and the
    servers' body limits (4096 bytes calibration, 8192 bench) checked before
    connecting;
  - the pages' headers: `Host` the server's own origin, `X-Control-Token`,
    `X-Client-Id` (one 36-character UUID per viewer process,
    `hardware_client::process_client_id`, reused on every connect to
    either server, as the page keeps one per page load),
    `Content-Type: application/json`; no `Origin` or `Sec-Fetch-Site`;
  - bodies with their members in the page's order and numbers in
    ECMAScript `Number::toString` form (`Body`, `js_number`,
    `js_number_text`: `0.0000032`, `1e-7`, `1e+21`, 2^60 as
    `1152921504606847000`, -0 as `0`, non-finite as `null`), so each
    request is the page's `JSON.stringify` byte for byte;
  - a non-2xx answer surfaces the server's `error` field verbatim
    (`ClientError::Server`), as the pages show `v.error`; without one it
    reads "Request failed (HTTP {status})". A server that refuses a request
    after reading only its head (a stale token after a restart) closes
    with the body unread, which may reset the connection; the answer that
    arrived is still used, and without one the error says the server
    closed the connection and its token may have changed.
- **Token hand-off.** The token is read from the page the server already
  serves, as the browser receives it: the calibration server's
  `<meta name="calibration-token">`, the bench's `const token='…'` in `/`
  or the `motor-bridge-token` meta of `/walking/`. Or it comes from a file
  the operator names (`--hardware-token-file`, `--motor-bench-token-file`).
  No server changed.
- **Refusal rule and origins.** Every intent is a `HardwareAction`.
  `HardwareAction::starts_motion` (actions.rs) is refused when it comes
  from `Origin::Rest` or `Origin::SystemUi`, with "hardware `{name}`
  starts, changes or arms motion and needs an operator at the window: REST
  and system_ui may read status, list gaits, export, connect, turn the
  mirror on or off and STOP only": select, set disabled (enable/disable),
  sweep all, hold others, jog press/release, speed, target and its commit,
  capture, reset poses, clear lower/upper, sweep, learn, the
  tune/campaign/gait confirmations, tune, campaign, gait
  select/mode/speed/effort/play, drive mode, PWM ceiling, flip, raw step
  value, raw step, live sync's leg/motor/polarity/scale/start, and the
  mirror's leg, joint, polarity and alignment bindings (the Leg/Both gait
  bindings and the saved alignment reference come from them). Motion
  needs a pointer or key in the window, with the operator there. Allowed
  from automation: toggle/close panel, connect, sections, status, STOP,
  loss (except `leaving`, which only the window's close request sends),
  export, load gaits, gait stop, turning the mirror on or off, sync
  connect and sync stop (REST `hardware_status`, `hardware_stop`,
  `hardware_export`, `hardware_gaits`, `hardware {action}`; `system_ui`
  `hardware:<name>`). A `system_ui` activation of a hardware control that
  is disabled now is refused with "{id} is disabled: {why}"
  (`robot/actions.rs` `apply`). While live motor sync is engaged, REST and
  `system_ui` may not start, step, jog, drive or re-speed the robot run
  either, since its targets go to the motors (`moves_synced_motors`);
  Pause, Reset and STOP stay available.
- **STOP paths.**
  - *Immediate.* The Stop button, Z, Escape, REST `hardware_stop` and
    `system_ui` `hardware:stop` post `stop` on a fresh connection from a
    `jobs::Pool::Dedicated` job with `complete_on_drop` (`link::stop_now`).
    It never queues behind the link thread, which may be waiting on a select
    that proves the watchdogs, or on a reply the server waits up to 8 s for.
    The server latches its stop flags when it parses the request. The job
    draws its sequence from the link's shared counter, and the UI bumps the
    link's epoch first, so answers already in flight are dropped as the page
    drops them.
  - *Loss of control* (the page's `loss()`, widened). Bevy
    `WindowFocused` false (the page's `visibilitychange`), panel close (×
    or the header toggle), leaving Robot mode (`actions::leave`) and
    closing the window (`WindowCloseRequested`, the page's `pagehide`)
    stop drive on the same immediate path whenever anything may drive
    (`link::drive_active`: ready, starting, a session, busy, sweep-all,
    tuning, campaigning, a leg gait), not only when a motor is ready,
    starting or in a session as the page checks; leaving always stops. Live
    sync stops on every loss, but only a session this viewer opened
    (`LiveSync::stop_ours`).
  - *Link drop.* When the link's channel closes (reconnect, mode exit,
    window close), the link thread sends STOP before it returns. Dropping
    the `Link` itself also writes a synchronous STOP when drive is active
    (`Link::post_stop_sync`, at most once per link), and an `AppExit`
    system in `Last` (`actions::stop_on_exit`) does what a window close
    does. bevy_winit clears the world when its event loop exits, so Cmd+Q
    and other exits without `WindowCloseRequested` still stop drive.
  - *Keepalive equivalence.* The page marks STOP `keepalive` so it outlives
    the tab. The native equivalent is `complete_on_drop` on its own thread
    (the request finishes even if the panel or the mode goes away first)
    and, when the window closes or the app exits, a STOP written
    synchronously with `Client::send_only` (`Link::post_stop_sync`; the
    bench's `/stop` likewise, `LiveSync::post_stop_on_leave`, only for a
    session this viewer opened), since the process may end before a job
    connects. Only the window's own close request (`Origin::Quiet`) writes
    it on the UI thread.
  - *Underneath.* The servers' leases (1.5 s motion and gait leases on the
    calibration server, 0.9 s on the bench) and the FPGA's command and
    telemetry watchdogs still stop the motors if the viewer dies without
    sending anything (SIGKILL, a crash). A tune, campaign or sweep-all has
    no lease and keeps running on the server until it ends or someone
    presses STOP (on the server's page, or in a restarted viewer).
  - *Heartbeats beside the link.* The `motion_update` heartbeat (100 ms
    after each answer) and the gait lease (`gait_update`, 300 ms, at once
    on pause or speed) run on their own `jobs::RunThread`
    ("hardware-beat", `session/beat.rs`), not on the link thread, so a
    request there that waits up to 8 s cannot let a 1.5 s lease lapse. The
    beat is the only sender of the server's per-run sequence domain
    (`motion_update`, `capture_hold`), draws sequences from the shared
    counter as it sends, and sends nothing while a STOP is pending.
- **What stays in the servers.** The serial bus, the one hardware worker,
  leases, sequence and owner checks, watchdog proofs, the FPGA supervisor
  and taught travel windows, the feedback controller, tuning, the campaign,
  gait playback on the leg and live streaming. Nothing in the viewer opens a
  serial port or computes a motor command. The mirror uses the shared
  `sim_runtime::kinematic_mirror`, and gait sampling uses the shared
  `sim_runtime::gait_playback`, as the page's worker does.
- **Safety stays underneath the UI.** The FPGA supervisor, taught travel
  windows, watchdogs and STOP are unchanged and hold whatever the UI does.
  STOP sits in the panel's top bar, which never scrolls, so it is reachable
  from every section.
- **Agents never drive hardware.** They build and verify by reading. This
  epic ends with [docs/hardware-checklist.md](../hardware-checklist.md),
  which the user runs with the operator present (connect, select, jog,
  teach, STOP from every section, focus loss and mode exit, sweeps, tune,
  campaign, gait, mirror, advanced settings, live sync, watchdog trip and
  lease loss, export, REST refusal). The browser pages stay until the user
  has signed that checklist off.

### 9. CAD in Rust

*Decided 2026-09-30.* RoboCAD (Python/OCCT/Qt, about 27,700 lines) moves to
Rust with exact feature parity, in phases. RoboCAD is the reference throughout.

1. **A CAD mode in `sim-spatial`.** It covers RoboCAD's workflows (sketch,
   features, direct edits, physical properties, joints, print splitting,
   export) as a client of RoboCAD's REST service, so the Python kernel still
   does the work. Every edit goes through RoboCAD's command layer, so undo and
   provenance stay intact.
2. **A parity harness.** It runs the same operations through both paths on a
   corpus of the user's real models and compares the results: mass properties,
   joints, exports, derived physics, and B-rep topology with tessellation
   within stated tolerances. Bit-identical output isn't the goal; tessellation
   and floating-point order differ.
3. **Derivations ported to Rust:** physical model, joint inference, flex
   analysis, printing, belts. One at a time, each gated by the harness.
4. **The OCCT kernel called from Rust** through bindings (not a pure-Rust
   kernel rewrite), module by module under the harness.
5. **The Python path is retired** only when the harness passes on the whole
   corpus and the user agrees.

The harness runs in verification passes. The `.rcad` format, undo, provenance
and the REST surface stay compatible throughout.

**Phase 1 progress.** *In progress.* The first CAD epic, **cad-mode**
(2026-09-30, see [CAD mode](#cad-mode-2026-09-30)), added `ViewerMode::Cad`
over RoboCAD's REST service: open a `.rcad` (self-started headless service)
or attach to a running RoboCAD, the model tree, tessellated bodies, picking
and selection shared with `/selection`, the inspector with RoboCAD's fields
and labels as returned, attribute edits, delete, undo/redo, save, GUI
registry commands and Ops, as typed `CadAction`s over a shared loopback
client. It is written and reviewed by reading; the verification pass builds
and tests it, and the user's [docs/cad-checklist.md](../cad-checklist.md)
compares it with RoboCAD step by step. The ledger
[docs/cad-parity.md](../cad-parity.md) assigns every other RoboCAD feature
to one of the later epics below. The second, **cad-select-transform**
(2026-10-01, see [CAD selection and transform](#cad-selection-and-transform-2026-10-01)),
added sub-body selection, the transform gizmo, push/pull and offset,
measure, live dimensions, snapping and the numeric bar over a Rust port
of `units.evaluate`, with one read-only Python addition (sampled edge
polylines); written and reviewed by reading, pending its verification
pass. Next: cad-modify.

#### Later CAD epics (planned 2026-09-30)

These follow cad-mode, which covers opening and attaching, the tree, the
bodies, picking, body selection, the inspector, the basic attribute edits,
delete, undo/redo, save, commands and keys. Each later epic is a client of
RoboCAD's REST service, like cad-mode. Every edit still goes through
RoboCAD's command layer. Every Ops method is already callable through the
`cad_op` REST command, so these epics build the *viewer UI*. Row-level scope
is in [docs/cad-parity.md](../cad-parity.md) (773 rows: 113 cad-mode, 637
later, 23 deliberately different; the planned cad-tools' 179 rows were split
2026-10-01 into cad-select-transform, 63, and cad-modify, 116). Twenty-three gaps there have no headless
route. Each needs a new route in `cad/robocad/api.py`, or a Rust port gated
by the parity harness. Planned order:

1. **cad-select-transform** (63 rows; the first half of the planned
   cad-tools, split 2026-10-01 because one turn could not deliver 179 rows
   well). Sub-body selection and direct transforms: face, edge, vertex and
   point selection modes (B, Shift+B, E, V, P); hover; box select; Alt
   disambiguation; select-all, invert, same-material and edges→faces;
   selection pushed to and adopted from `/selection` as `[node, kind,
   index]` with its mode; the inspector's face, edge and vertex details;
   the move, rotate and uniform-scale gizmo with RoboCAD's pivot rule;
   push/pull and offset of faces; measure (kept as a measure node on
   Shift); the numeric bar with unit expressions over a Rust port of
   `units.evaluate` (`sim_runtime::units`); live dimensions of the selected
   faces and edges (`set_diameter`, `set_distance`, `set_angle`);
   snapping (vertex, midpoint, centre, grid, plane; Alt suppresses) with
   its readout; tool cursors and the tool · mode hint. Routes:
   `/nodes/{id}/faces|edges|vertices|solids`, `GET /nodes/{id}/edges?samples=N`
   (sampled B-rep edge polylines, the one read-only Python addition this
   epic makes), `PUT /selection`, `POST /ops/transform|push_pull|offset_faces|set_diameter|set_distance|set_angle|add_measurement`.
   It comes first because sketching, printing and cad-modify reuse its
   picking, snapping and numeric entry.
2. **cad-modify** (116 rows; the second half of the planned cad-tools). The
   operation catalogue on top of cad-select-transform's picking and
   numeric entry: primitives (box, centre box, three-point box, cylinder,
   sphere), fillets (variable, chordal, all edges, full round, remove),
   chamfer, shell, thicken, draft, mirror (and live), array (rectangular,
   radial, curve), instance, make unique, pivot editing and "set pivot at
   cursor", the inspector's pivot and transform editors, multi-node delete
   as one step, every Modify command (booleans, region, join/unjoin,
   dissolve, delete faces, cut, split, imprint, project, silhouette,
   control points, raise degree, rebuild, dependent offset, the REST-only
   direct edits), the tools toolbar and right-click menu, both radial
   menus, and the command palette with key conflicts and menus by
   category. Routes: `POST /ops/*`, `POST /nodes`, `GET /commands`. Gaps:
   copy and paste with placement, reading control points, curvature comb
   and continuity check (each needs a Python route).
3. **cad-sketch** (60 rows). The active plane, construction planes (from a
   face, three points, two points and the camera, midplane), the 13 sketch
   tools, sketch offset/fillet/join and the REST-only edits (trim, split,
   extend, rebuild, vertices), extrude and revolve with boolean modifiers,
   sweep, pipe, loft and fill. Routes: `GET/POST /nodes/{id}/sketch`,
   `POST /nodes {"kind": "sketch"}`, `POST /ops/plane_*`,
   `POST /ops/extrude|revolve|sweep|pipe|loft|fill`. Curve-node display
   uses the sampled edge polylines cad-select-transform added
   (`GET /nodes/{id}/edges?samples=N`). RoboCAD has no sketch constraints, so
   there is nothing to port there. The dead `sketch.arc` key (A) should be
   bound deliberately.
4. **cad-views-export** (112 rows). Display modes (shaded with edges,
   wireframe, xray, matcap, render), pan, zoom to the cursor, trackball,
   view presets, the view cube, ortho and FOV, the grid, the build plate,
   the section tool (from display triangles) and exact sections, isolate,
   hide and show all, high contrast, SpaceMouse, saved views (the native
   camera written in RoboCAD's view-state schema), the per-node
   tessellation tolerance, file dialogs (new, open, save as, import with
   units), every export format and the drawing, and render/capture. Routes:
   `/views*`, `POST /ops/isolate|show_all|set_visible`,
   `GET /nodes/{id}/section`, `POST /export`, `POST /import`,
   `GET /render`, `/loads/{id}`, `POST /autosave`. Gaps: the save
   thumbnail, the autosave interval and failure report, the mesh-unit
   guess, and the GUI-only Blender link and web share (edge polylines are
   served since cad-select-transform; drawing them as display edges is here).
5. **cad-physical-inspect** (82 rows). The materials panel and engineering
   properties, colour, the Robot panel (summary, tree, margins, issues),
   motor, joint, sensor and cable tools and dialogs, joint editing and
   joint-physics overrides, battery/control/uncertainty, the exact
   multi-selection measurement, physical export, results and
   identification, the stress overlay, and the live simulation link (on
   save, export `simrobot.json` through `/physical?path=` and reload robot
   mode in this app). Routes: `GET /robot`, `GET /motors`, `/sensors`,
   `/cables`, `PUT /battery|control|uncertainty`, `GET/POST /materials`,
   `GET /physical`, `GET /results`, `POST /results/load`,
   `POST /identification/apply`, `/actuator-profiles`,
   `POST /ops/add_joint|set_joint|add_motor|attach_motor|set_joint_physics|set_material_props`.
   Gaps: per-node results (inspector line, stress overlay, margins) and
   the planar export variant. Port `results_margins` or add a route first.
6. **cad-print** (33 rows). Wall check, validate, overhang shading,
   fastener and clearance tools, split for printing, strength, plan,
   strength-or-split, assembly guide, coupons, and the job list with
   progress and cancel. Routes: `GET /nodes/{id}/thin|validate`,
   `/print/*`, `POST /ops/fastener_hole|clearance|print_split`. Gap: the
   print overlay's per-node results (shared with 5).
7. **cad-organize** (108 rows; planned as cad-annotations). Outliner
   organization (search, groups, drag-and-drop, move to group, active
   group, inline rename, multi-select), comments and threads with pins and
   part links, references (images, placement, calibration, the linked
   system file and opening it in builder mode), components (library,
   place, recipes, occurrences, jobs) and the system graph. Routes:
   `/threads*`, `/comments/{id}`, `GET /components`,
   `/component-jobs/{id}`, `/system*`,
   `POST /ops/group|move_nodes|set_active_group|import_references|update_reference|calibrate_reference|make_component|place_component|…`.
   Gaps: reference image pixels (viewport and list preview) and the
   geometry-rule recipes (`component_derivation.RECIPES`).
8. **cad-experiments-motion** (63 rows). The experiments panel (Rhai
   editors, profiles, runs, cancel, baseline and compare, linked files,
   restore inputs, auto-rerun, the catalogue), run review, candidate
   review, model scripts and batches, the pose panel, motion programs and
   video export (recorded by the viewer). Routes: `/experiments*`,
   `/candidates*`, `POST /doc/batch`, `POST /doc/script`,
   `/motion/programs` (headless). Gaps: captured-CAD replay, candidate
   geometry and headless pose kinematics. `/motion` playback and export
   need RoboCAD's window, so add routes for these or port `pose.py`'s
   kinematics before this epic.

## Bevy features to use

These are verified in the official 0.17, 0.18 and 0.19 release notes. Before
using any of them, read the 0.19.1 API docs and the migration guides; don't rely
on memory.

| Feature | Release | Use it for |
|---|---|---|
| Event / observer overhaul | 0.17 | the action layer (§3) |
| Headless standard widgets (`bevy_ui_widgets`); Feathers | 0.17–0.19 | the UI kit (§6): the slider is headless; Feathers is not used (its look is not the builder's) |
| Text input | 0.19 | not adopted yet: the builder's draft keeps its own entry (see the UI kit section) |
| `ViewportNode` | 0.17 | 3D views inside panels (schematic beside spatial, inspector previews) |
| First-party camera controllers | 0.18 | replace hand-rolled orbit cameras where equivalent |
| Easy screenshot and video recording | 0.18 | `ui_capture` and run recordings |
| App settings | 0.19 | persisted viewer preferences |
| Interactive transform gizmo, infinite grid | 0.19 | build-mode placement and grid (display-only) |
| Text gizmos | 0.19 | 3D labels (steady values, fixed anchors) |
| Diagnostics overlay, frame time graph | 0.17–0.19 | measuring realtime performance, which `AGENTS.md` requires |
| Observer run conditions, delayed commands | 0.19 | mode-scoped observers, timed UI |
| Next-generation scenes | 0.19 | evaluate for mode setup and teardown |

## How to change the code

- **New code is written in the target shape**, even while old code around it
  isn't.
- **Move existing code one subsystem or one mode at a time.** Delete what it
  supersedes in the same epic; don't leave two ways.
- **Show parity before removing a legacy path**, by tracing the workflow
  through both code paths (screenshots are off for now).
- **A refactor must remove a named, recurring cost**, such as duplicated
  handlers, hand-rolled threads or per-mode apps. Renaming for its own sake
  doesn't count.
- **When you can't follow this document,** record the decision (why, the
  alternatives, revisit-if) and, if the shape really should change, update this
  document.

## Default epic order

The Director re-ranks with evidence, but this is the default:

1. **Upgrade to Bevy 0.19.1.** *Done 2026-09-30 (batch bevy-0-19-upgrade; see
   the migration section).* One epic covering the whole workspace
   (`sim-spatial` and `sim-app`):
   - follow the migration guides 0.16→0.17→0.18→0.19
   - make no feature changes
   - every binary builds, and each mode opens and captures
2. **Jobs abstraction.** *Done 2026-09-30 (verified at ae80a137; batch
   jobs-module; see [the jobs module](#jobs-module-2026-09-30)).* Build
   `jobs`, then move all 35 thread sites onto it.
3. **One app.** *Done 2026-09-30 (verified at 7da1216e; batch
   one-app-modes; see [One app](#one-app-2026-09-30)).* Merge the separate
   `App` setups into `ViewerMode` states, with switching modes in the
   window.
4. **Action layer.** *Done 2026-09-30, verified at 90c65c86 (batch
   action-layer; see [Action layer](#action-layer-2026-09-30)).* Unify
   buttons, `system_ui` and REST onto typed actions.
5. **UI kit.** *Done 2026-09-30, verified at 4bc03789 (batch ui-kit; see
   [UI kit](#ui-kit-2026-09-30)).* Build it on Bevy's widgets, then move
   headers, inspectors, tabs, docks and charts onto it.
6. **Hardware front end** (§8). *Done 2026-09-30 pending the user's
   hardware checklist ([docs/hardware-checklist.md](../hardware-checklist.md);
   batch hardware-front-end; see
   [Hardware front end](#hardware-front-end-2026-09-30)).* The calibration
   and hardware panel in
   `sim-spatial`, over the existing Rust calibration layer, ending with the
   user's hardware checklist.
7. **CAD mode** (§9 phase 1). *In progress.* RoboCAD's workflows in
   `sim-spatial` over its REST service, in several epics. **cad-mode**
   (2026-09-30; see [CAD mode](#cad-mode-2026-09-30)) is verified at
   a4fe42d3 and awaits the user's [CAD checklist](../cad-checklist.md).
   **cad-select-transform** (2026-10-01; see
   [CAD selection and transform](#cad-selection-and-transform-2026-10-01))
   is done pending its verification pass. Next: **cad-modify**. Remaining,
   in order (§9 "Later CAD epics"): cad-modify, cad-sketch,
   cad-views-export, cad-physical-inspect, cad-print, cad-organize,
   cad-experiments-motion.
8. **Parity harness** (§9 phase 2).
9. **Derivations in Rust** (§9 phase 3). Several epics, one derivation family
   each.
10. **OCCT from Rust** (§9 phase 4). Several epics, one kernel area each.
11. **Fold in `sim-app`.** Bring its scenes in as modes, or retire them.
    *Done 2026-09-30, verified at 80b5997e (batch fold-sim-app; sim-spatial
    lib tests 172 passed, 1 ignored; workspace `--locked` check clean; see
    [Fold in sim-app](#fold-in-sim-app-2026-09-30)).*

After that, feature work resumes on the target shape. Split large files while
they're being touched.

## Open questions

- Whether Bevy Remote Protocol should back the REST surface. Adopt it only if it
  removes code and keeps `sim_api`'s guarantees.
- ~~The CAD (Python/OCCT) boundary~~: resolved 2026-09-30. CAD moves to Rust
  (§9).
- Which Rust OCCT bindings to use or extend (for example `opencascade-sys`
  through `cxx`), and how to keep the C++ build out of the fast path.
