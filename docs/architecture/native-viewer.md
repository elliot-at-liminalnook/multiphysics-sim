# Native viewer architecture

**Status:** target shape, adopted 2026-09-30. This is the standard new work is
held to. Agents keep it current: when a change alters the shape, update this
document in the same commit and record the decision.

`sim-spatial` is the one native viewer. Every user workflow (building systems,
robots, lessons, scanned places, inspection) lives in it, on Bevy 0.19.1. The
project rules in `AGENTS.md` still govern everything here. In particular, CAD
owns physical definitions, physics lives in shared crates, and the viewer never
duplicates physics.

## Where it is today (measured 2026-09-30, after one-app-modes)

- **Bevy 0.19.1**, pinned in the workspace `Cargo.toml` and in
  `crates/sim-spatial/Cargo.toml` (hand-picked features, see
  [Bevy 0.19.1 migration](#bevy-0191-migration-2026-09-30)). Only
  `sim-spatial` and `sim-app` depend on Bevy.
- **One app, modes as states** (see [One app](#one-app-2026-09-30)):
  `app::run` is the only `App` builder (`App::new()` appears elsewhere in
  `src/` only in `#[cfg(test)]` code). `ViewerMode` (Inspect, Build,
  Lessons, Robot, Place) is a Bevy `States`; the computed states
  `ModeScope` and `SpatialScreen` follow it. Launch flags choose the initial
  mode and document; the user switches modes in the window (the mode
  switcher, `system_ui` `mode:*`, REST `viewer_mode`), through one handler
  (`app::switch::handle`). Done, pending verification.
- **One REST server** (`rest::bind`, the only `sim_api::Server::bind` in
  sim-spatial, also used by `--headless`): every mode's commands, each
  tagged with its `modes`; one dispatch (`app::route::route`) refuses a
  command of another mode by name. Place mode answers `state`, `camera` and
  `screenshot`.
- **The shared pipeline sets** `ViewerSet` Input → Actions → JobResults →
  SimSync → Present are configured once (`app::ModesPlugin`). Each mode's
  existing frame chain sits whole in SimSync and its drawing-only chain in
  Present, under `in_state(..)`, in their original order.
- **Background work goes through one `jobs` module** (`src/jobs/`, see
  [§4](#4-one-background-work-abstraction) and
  [the jobs module](#jobs-module-2026-09-30)): 0 `thread::spawn` /
  `thread::Builder` sites outside `src/jobs/` (there were 35 in 21 files),
  enforced by the lib test `jobs::tests::threads_are_started_only_in_jobs`.
  Verified at ae80a137.
- **Bevy structure:** 7 plugins (`CorePlugin`, `ModesPlugin`,
  `SpatialViewerPlugin`, `BuilderPlugin`, `LearnPlugin`, `RobotPlugin`,
  `PlacePlugin`), one `States` enum and two computed states, one set
  enum. Buffered input uses the 0.17+ names (14 `MessageReader`, 7
  `MessageWriter` sites); the 10 observers take `On<…>` (pointer picks,
  drags, screenshots).
- **UI is hand-built** `Node` trees (17 files name `Node`). Headers,
  inspectors, tabs, docks and charts are rebuilt per feature.
- **Large files:** `builder.rs` (3,013 lines), `robot.rs` (2,794),
  `robot_run.rs` (2,597) and `lesson/mod.rs` (2,397). The new `app/` files
  are under 700 lines each (`switch.rs` 659, `mod.rs` 319, `route.rs` 201,
  `switcher.rs` 105, `tests.rs` 141).
- **What already works well, to keep:**
  - typed, validated handlers ("one handler per action")
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
    (its `PlaybackState` is `Stamped`) and the placement validator (pointer
    moves are commands; the worker drains the channel and validates only the
    newest position; a closed channel never starts a queued one).
  - Helpers: `reap_child` for the linked `sim-viewer` (two sites in
    `main.rs`) and, found by reading, for the `open`/`xdg-open` process of a
    web source link, which was never waited for; `drop_off_thread` for the
    builder replaced by Open system.
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
      `reference.rs:24`) and `builder/agent.rs:159` (Compute, polled at
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
      `jobs::reap_child`.
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
      (`lesson/rest.rs:438`). Unchanged text.
    - Practice bench request: `lesson/extras.rs:207` (Dedicated), polled at
      `extras.rs:141`. Unchanged.
- **What the build/test pass must run** (no screenshots):
  - `cargo build -p sim-spatial --lib --tests --bins` with no sim-spatial
    warnings (an unused `mpsc`/`Mutex`/`Arc`/`AtomicBool` import would show
    here);
  - `cargo check -p sim-app`;
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
    Builder = Build + Lessons, Robot, Place) and `SpatialScreen` (the
    spatial view is drawn: Inspect, Build, Lessons), `ViewerSet`,
    `CorePlugin` (window, per-mode look, fonts, mesh picking, occlusion,
    scroll clamp, REST wake, the switcher) and `ModesPlugin` (states, sets,
    the switch; no window, so the test runs it).
  - `app/switch.rs`: `ModeSwitch`, `Switcher`, `Documents`, the handler
    (`handle`, Actions), document loads (`finish_load`, JobResults),
    `arrive` (every mode's OnEnter) and the scopes' OnExit teardown.
  - `app/route.rs`: the one REST dispatch (`route`, `annotate`) and the
    capabilities every mode shares (`viewer_mode`, `screenshot`).
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
  **Present**. Splitting the mode chains across the sets is the action
  layer's work (epic 4).
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
    runs unseen), its scene parked in `Documents` and its chrome rebuilt on
    return. Leaving Build/Lessons removes the lesson (its jobs cancel) and
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
    Build ↔ Lessons over an open lesson is the lesson screen's toggle and
    is never blocked (as before).
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
  - `cargo check -p sim-app`;
  - `cargo test -p sim-spatial --lib`, in particular
    `app::tests::build_robot_build_tears_down_the_robot_and_keeps_shared_state`
    and `app::tests::rest_refuses_commands_of_another_mode_by_name` (new),
    the jobs tests (`jobs::tests::*`, including
    `threads_are_started_only_in_jobs`), `builder::open::tests`,
    `builder::placement::tests`, `builder::placement_worker::tests`,
    `builder::ui_api::tests`, `builder::replay_tests`, `robot_run::tests`,
    `rest::tests` and the lib.rs `keyboard`/`pick_part` test;
  - a reading check of the launch trace above against the built code.
- **Not yet verified.** Nothing here has been built or run yet. The
  switcher's placement over each mode's panels, the robot link picking the
  core now enables, and a live switch between every pair of modes in a
  window are unverified until screenshots are on.

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
- *Status:* in place since one-app-modes (see [One app](#one-app-2026-09-30)),
  pending verification: `app::run`, `ViewerMode` with the `ModeScope` and
  `SpatialScreen` computed states, setup on each scope's `OnEnter`, teardown
  by `DespawnOnExit<ModeScope>` and the scopes' `OnExit`, and one switch
  handler. What survives a switch: the builder, the display-model library,
  the fonts, the REST server, the documents each mode reopens, and the
  workspace root. Selection and annotations are still per mode (§7);
  `sim-app` is not folded in (epic 6).

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
  once (`app::ModesPlugin`); the modes' existing chains sit whole in SimSync
  and Present, and the builder and lesson chains still order themselves
  against the spatial view's `update_parts` and `camera_viewport` (to be
  untangled with the action layer).
- **Files over about 800 lines are a smell.** Split them by responsibility when
  you touch them.

### 3. One action layer

- Every user intent is a typed action (an enum per mode or feature), validated
  in one handler.
- Buttons, keyboard, `system_ui`, REST and scripts all produce the same actions.
  UI callbacks contain no logic.
- Actions travel as Bevy events or observer triggers (the 0.17 event/observer
  model). Undoable actions go through the shared undo history.
- `sim_api` capabilities are generated from, or checked against, the action
  registry, so the REST surface can't drift from the UI.

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
- **Helpers**: `jobs::reap_child` waits for a detached child process (the
  linked `sim-viewer` window, a browser opened for a link) on its own thread;
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

- A `ui_kit` module on Bevy Feathers and the standard headless widgets
  (0.17–0.19) provides: header, tabs, inspector sections, property rows, lists,
  text input, docks, charts, toasts and dialogs.
- Theme tokens live in one place.
- Features compose these widgets instead of building their own `Node` trees for
  common UI. Accessible labels (0.19) go on interactive widgets.

### 7. Documents, selection, annotations

- One document resource per open file, with a revision number.
- One selection model, and one annotations and discussions service. Every mode
  uses them.

## Bevy features to use

These are verified in the official 0.17, 0.18 and 0.19 release notes. Before
using any of them, read the 0.19.1 API docs and the migration guides; don't rely
on memory.

| Feature | Release | Use it for |
|---|---|---|
| Event / observer overhaul | 0.17 | the action layer (§3) |
| Feathers widgets, headless standard widgets | 0.17–0.19 | the UI kit (§6) |
| Text input | 0.19 | the UI kit (§6) |
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
3. **One app.** *Done 2026-09-30, pending verification (batch
   one-app-modes; see [One app](#one-app-2026-09-30)).* Merge the separate
   `App` setups into `ViewerMode` states, with switching modes in the
   window.
4. **Action layer.** Unify buttons, `system_ui` and REST onto typed actions.
5. **UI kit.** Build it on Feathers, then move headers, inspectors, tabs, docks
   and charts onto it.
6. **Fold in `sim-app`.** Bring its scenes in as modes, or retire them.

After that, feature work resumes on the target shape. Split large files while
they're being touched.

## Open questions

- Whether Bevy Remote Protocol should back the REST surface. Adopt it only if it
  removes code and keeps `sim_api`'s guarantees.
- The CAD (Python/OCCT) boundary: which CAD controls move into the native
  viewer, behind a clean service boundary.
