# Native viewer architecture

**Status:** target shape, adopted 2026-09-30. This is the standard new work is
held to. Agents keep it current: when a change alters the shape, update this
document in the same commit and record the decision.

`sim-spatial` is the one native viewer. Every user workflow (building systems,
robots, lessons, scanned places, inspection) lives in it, on Bevy 0.19.1. The
project rules in `AGENTS.md` still govern everything here. In particular, CAD
owns physical definitions, physics lives in shared crates, and the viewer never
duplicates physics.

## Where it is today (measured 2026-09-30, after the jobs module)

- **Bevy 0.19.1**, pinned in the workspace `Cargo.toml` and in
  `crates/sim-spatial/Cargo.toml` (hand-picked features, see
  [Bevy 0.19.1 migration](#bevy-0191-migration-2026-09-30)). Only
  `sim-spatial` and `sim-app` depend on Bevy.
- **Separate apps, not one viewer.** `sim-spatial/src/lib.rs` builds a separate
  `App` per launch mode (`run_builder`, `run_lessons`, `run_with_api`, …), and
  `main.rs` picks one from command-line flags. There is no switching modes in a
  window. Place mode (`place_view.rs`) has no REST API.
- **Background work goes through one `jobs` module** (`src/jobs/`, see
  [§4](#4-one-background-work-abstraction) and
  [the jobs module](#jobs-module-2026-09-30)): 0 `thread::spawn` /
  `thread::Builder` sites outside `src/jobs/` (there were 35 in 21 files),
  enforced by the lib test `jobs::tests::threads_are_started_only_in_jobs`.
  Pending verification (see the parity checklist).
- **Little Bevy structure:** 3 plugins, no `States`, no system sets. Buffered
  input uses the 0.17+ names (16 `MessageReader`, 6 `MessageWriter` sites);
  the 10 observers take `On<…>` (pointer picks, drags, screenshots).
- **UI is hand-built** `Node` trees (18 files name `Node`). Headers,
  inspectors, tabs, docks and charts are rebuilt per feature.
- **Large files:** `builder.rs` (3,000 lines), `robot.rs` (2,750), `robot_run.rs`
  (2,610) and `lesson/mod.rs` (2,340).
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
  `tonemapping_luts`, `smaa_luts`, `hdr`, `bevy_state`, scenes, glTF, audio or
  gamepad. `sim-app` keeps Bevy's default features.
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
- **Parity checklist for the verification pass** (build, lib tests, then
  `ui_capture` against the upgraded-019 captures with
  `$PAIR_CAPTURES/bevy-019/capture_modes.py`):
  - every mode's REST state and capabilities (`system_state`, `robot_state`,
    lesson state, `GET /v1/capabilities`, `system_ui` ids) identical;
  - build mode: the schematic re-layout after an edit and a level change
    (stale label, then the new layout), Open system (the previous system
    dropped off the UI thread), a live run start/pause/step/reset and a run
    replay, a study's progress, the Actuators, Measured evidence and Gait lab
    tabs loading;
  - placement drag: preview while dragging and the commit on release
    (a REST-activated control is not a drag gesture; say which was used);
  - lessons: scene recording progress and install, a comparison's
    "Running n/m…", figures, model values; narration progress **without**
    triggering paid generation (read `lesson` state `narration.job` only if a
    job is already running, or verify by reading);
  - robot: preset open, recording save, list and replay, reset and reload of
    a `--robot FILE` (the old run thread stops, no stale frame), preset
    switch, gait preview open/play/stop, recorded preset seek/play;
  - no warning "did not stop within" in the log during these.

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
  are retired once parity is shown with captures.

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
  feature's private systems.
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
- **Show parity with `ui_capture` before removing a legacy path.**
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
2. **Jobs abstraction.** *Done 2026-09-30, pending verification (batch
   jobs-module; see [the jobs module](#jobs-module-2026-09-30)).* Build
   `jobs`, then move all 35 thread sites onto it.
3. **One app.** Merge the separate `App` setups into `ViewerMode` states, with
   switching modes in the window.
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
