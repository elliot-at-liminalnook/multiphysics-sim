# Native viewer architecture

**Status:** target shape, adopted 2026-09-30. This is the standard new work is
held to. Agents keep it current: when a change alters the shape, update this
document in the same commit and record the decision.

`sim-spatial` is the one native viewer. Every user workflow (building systems,
robots, lessons, scanned places, inspection) lives in it, on Bevy 0.19.1. The
project rules in `AGENTS.md` still govern everything here. In particular, CAD
owns physical definitions, physics lives in shared crates, and the viewer never
duplicates physics.

## Where it is today (measured 2026-09-30)

- **Bevy 0.16.1**, pinned in the workspace `Cargo.toml`. Only `sim-spatial` and
  `sim-app` depend on Bevy.
- **Separate apps, not one viewer.** `sim-spatial/src/lib.rs` builds a separate
  `App` per launch mode (`run_builder`, `run_lessons`, `run_with_api`, …), and
  `main.rs` picks one from command-line flags. There is no switching modes in a
  window.
- **Background work is hand-rolled:** 35 `thread::spawn` / `thread::Builder`
  call sites across 21 files, each with its own progress, cancellation and
  stale-result handling.
- **Little Bevy structure:** 3 plugins, no `States`, no system sets.
- **UI is hand-built** `Node` trees in 21 files. Headers, inspectors, tabs,
  docks and charts are rebuilt per feature.
- **Large files:** `builder.rs` (3,000 lines), `robot.rs` (2,650), `robot_run.rs`
  (2,560) and `lesson/mod.rs` (2,340).
- **What already works well, to keep:**
  - typed, validated handlers ("one handler per action")
  - generation-stamped frames
  - shared undo history
  - the `system_ui` control registry and REST adapter (`sim_api`)
  - worker-computed results kept off the UI thread

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

- A `jobs` module owns all off-thread work:
  - **One-shot jobs** (load, export, compute, scan) run on Bevy's
    `AsyncComputeTaskPool` or `IoTaskPool`.
  - **Long-lived run threads** (simulation sessions, robot runs) use one
    `RunThread` type: a command channel in, generation-stamped snapshots out.
- Every job has progress, cancellation, a generation stamp (stale results are
  dropped) and a surfaced error.
- One system per feature applies finished results.
- `thread::spawn` appears nowhere outside `jobs`.

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

1. **Upgrade to Bevy 0.19.1.** One epic covering the whole workspace
   (`sim-spatial` and `sim-app`):
   - follow the migration guides 0.16→0.17→0.18→0.19
   - make no feature changes
   - every binary builds, and each mode opens and captures
2. **Jobs abstraction.** Build `jobs`, then move all 35 thread sites onto it.
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
