# Native viewer architecture

**Status:** target shape, adopted 2026-09-30. This is the standard new work is
held to. Agents keep it current: when a change alters the shape, update this
document in the same commit and record the decision.

`sim-spatial` is the one native viewer. Every user workflow (building systems,
robots, lessons, scanned places, inspection) lives in it, on Bevy 0.19.1. The
project rules in `AGENTS.md` still govern everything here. In particular, CAD
owns physical definitions, physics lives in shared crates, and the viewer never
duplicates physics.

## Current source-review status — 2026-10-02

T45 settings was accepted by source review at `6f3b701e`/`33d9da46`.
T46 retained offline identification was accepted in call-0360 across
`1b0ca533`, `d238d6b6`, `c380210b`; T47 ordinary close preservation was accepted
in call-0364 at `a0671a6b`. These are source acceptances, not compilation or
execution receipts. T48 controller refinement was accepted by source review in
call-0368 across `7c4fbd09`/`247ccfa6`; it extends those owners. Its
[source evidence](../native-refinement-authoring.md) and
[field inventory](../native-refinement-inventory.md) record that accepted bounded batch.
T49 ordinary activation and focus was accepted in call-0378 across
`11e898f7`, `6b61a495`, `090d608d`, and `72478587`. Its public modal-containment
ordering and actual-consumer focus repairs are accepted by source review only;
compilation, fixtures, rendering and hit testing remain unexecuted.

T50 recording authoring was accepted by source review in call-0386 across
`5dd26014`, `11b9ce82` and `3a7d321a`. The accepted repairs include captured
fit traces, rejected additional-study input retention, shared two-source quarantine,
and bounded receipt references backed by Study-owned immutable `.study-inputs/`
companions. Save-new/reopen validates those companions; legacy inline evidence
stays readable and is never silently stripped. See [T50 evidence](../native-recording-fit-authoring.md).
This acceptance establishes no compilation, fixture execution, GUI parity,
executed durability or reference retirement.

T51 shares filesystem publication mechanics beneath the existing Study and
SettingsOwner contracts. The [publication source map](../shared-evidence-publication.md)
records immutable-new versus replacement policy, typed visibility/durability
outcomes, synchronization/retry limits and retained recovery. Existing actions,
public scheduling, jobs and document/lifecycle owners remain authoritative.

T52 native offline electrical and power authoring was accepted by source review
in call-0396 across `d9bc1455`/`929f6833`. Cancelled and incomplete terminal
content reopens as unapplied UNSCORED diagnostics; native and report completeness
gates agree. [T52 evidence](../native-power-authoring.md) records the bounded batch.
T53 portable retained-study artifacts extends the same Study and publication owners;
[format, focused inventory and source traces](../portable-study-artifacts.md) record
its implementation and written unexecuted fixtures. No execution or parity follows.

The accepted [2026-10-02 verification receipts](../verification-20261002.md)
supersede the historical blanket “uncompiled” statements below for their exact
recorded commands and exercised paths, through repairs `ee17ef00`–`f971bedb`.
They establish workspace compilation, repaired fixture reruns, bounded CAD
operations and virtual HW-01 connection/idle STOP. They do not establish
HW-02–HW-09 calibration completion, full CAD parity or an independent Rust CAD
replacement. T53 at `f541b05f` remains set aside and unaccepted. Later changes
remain source-reviewed only unless a new receipt explicitly executes them.
Historical Python results remain historical; no reference retirement or
physical-source acceptance follows.

## Where it is today

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
- **Documents, selection, annotations** (unified-selection-document,
  2026-10-01, done pending verification): one document registry
  (`document/`), one selection (`selection/`) that every mode reads, and one
  annotations service (`annotations/`) with one thread panel
  (`ui_kit/threads.rs`); see
  [Documents, selection and annotations](#documents-selection-and-annotations-2026-10-01).
  Since cad-annotation-parity (2026-10-02, by reading, unexecuted), threads
  behave the same in CAD, Build, Inspect, Lessons and Robot (the
  [cross-mode matrix](#cross-mode-annotation-behaviour-cad-annotation-parity-2026-10-02)).
  Inspect notes are full threads in a v1/v2 sidecar, Robot mode shows its
  CAD source's RoboCAD threads on its links (new REST `robot_threads`), and
  CAD-176 to CAD-186 have reading traces in `docs/cad-checklist.md`.
  In the 2026-10-01 batch no REST command was added or renamed (392 `spec(`/`c("` entries before
  and after); sim-spatial has 399 `#[test]` functions (374 at aa34ef48,
  counted with `grep -rh '#\[test\]' crates/sim-spatial/src | wc -l`).
- **Leg hardware, HW-10 and HW-11** (native-leg-clock-alignment,
  2026-10-02): complete by reading. Gait playback Sim, Leg and Both, one
  leg clock and the mirror's alignment are traced in
  `docs/hardware-checklist.md` (traces around line 1052, run sheets around
  1495). Nothing was executed: no test, driver run or build.
- **Window-first** (window-first-usability, 2026-10-01, verified at
  aa34ef48: lib 369 passed, 1 ignored; bins 4/4; workspace check clean): every mode's document can be opened in the window. A mode
  chosen in the switcher with no document opens the document picker
  (`app/picker/`); the switcher sits in a reserved strip every dock ends
  above (`ui_kit::SWITCHER_STRIP`); no window text tells a person to use
  REST (`copy_guard_tests.rs`).
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
  `cad/actions.rs`, re-counted 2026-10-01): 138 in all. cad-modify adds
  6 (`cad_invoke`, `cad_run`, `cad_form_set`, `cad_form_submit`,
  `cad_form_cancel`, `cad_surface`; the CAD specs moved to `cad/specs.rs`,
  37 `spec(` entries there, re-counted 2026-10-01 against the 31 before):
  144 in all. cad-sketch adds 1 (`cad_sketch`; 38 `spec(` entries,
  re-counted 2026-10-01 with `grep -c 'spec(' crates/sim-spatial/src/cad/specs.rs`
  against 37 at e0996878): 145 in all. cad-views-export adds 19: the
  shared camera's 13 `camera_*` commands (`camera/mod.rs`, one entry each,
  tagged with every orbit mode) and 6 CAD commands (`cad_display`,
  `cad_section`, `cad_views`, `cad_file`, `cad_export`, `cad_render`, from
  the `specs()` of `cad/display`, `cad/views` and `cad/files`; `cad/specs.rs`
  still has 38), 164 in all. Re-counted 2026-10-01 with `grep -c 'spec('`
  per file and `git grep -E '(^|[^a-z_])spec\('` over the crate's non-test
  sources (89 at cc7ac194, 108 at f15766ea). Place mode answers `state`, `camera` and `screenshot`.
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
  systems; SimSync each mode's continuous work (jobs, drags, text entry,
  scene sync, and the shared camera's `CameraSet` Viewport → Navigate →
  Place, which holds every orbit camera and Place's fly camera since
  cad-views-export); Present drawing and REST snapshots.
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
  two computed states, one set enum (`CameraSet` joined it in cad-views-export: 2), 9 action Message types (10 with `CameraAction`) (`Act<A>` for
  `WindowAction`, `InspectAction`, `SystemAction`, `LessonCommand`,
  `RobotAction`, `HardwareAction`, `PlaceAction`, `CadAction`,
  `PhenomenaAction`). Same greps after fold-sim-app: 27
  `MessageReader<` lines, 57 `MessageWriter<` lines, 11 `On<` lines,
  `KeyCode` 223 times in 17 files, `Interaction` 94 times in 21 files.
  After cad-views-export (re-measured 2026-10-01 at f15766ea, same greps):
  13 plugins (`CameraPlugin` added: the shared camera, `src/camera/`), 10
  action Message types (`Act<CameraAction>` added; counted from the
  `actions::register::<…>` sites, `InspectAction` registered in
  `inspect_view` and once more in a `notes.rs` test), 27 `MessageReader<`
  lines, 88 `MessageWriter<` lines, 10 `On<` lines, `KeyCode` 474 times
  in 36 files (`grep -row`), `Interaction` 121 times in 31 files, and 4
  `MessageCursor<` lines (the camera's `navigate` and `fly` drain the
  pointer messages through `Option<Res<Messages<…>>>` with a local
  cursor).
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
  verified at c0ed9b29 (sim-spatial lib tests 209 passed, 1 ignored; bins
  4 passed; sim-runtime `cad_client` and `units` 59 passed; api pytests 14
  passed; sim-web wasm check clean): face,
  edge, vertex and point selection with hover, box select, the Alt menu
  and the selection commands; the move/rotate/scale gizmo, push/pull and
  offset, measure, live dimensions, snapping and the numeric bar with
  unit expressions (`sim_runtime::units`); each commit one RoboCAD Ops
  call. `cad/` is 10,397 lines in 30 files (wc -l, 2026-10-01; largest
  `document.rs` 734, `transform/mod.rs` 737, `panel.rs` 687, `actions.rs`
  678), `sim-runtime/src/units.rs` 901 (tests included).
  **cad-modify** (2026-10-01, see [CAD modify](#cad-modify-2026-10-01)),
  verified at e0996878 (sim-spatial lib 252 passed, 1 ignored; bins 4; `cad_client` 33; `units` 29; api pytests 48; sim-web wasm check clean): the op
  catalogue as data (`cad/ops/`, 54 entries: 43 RoboCAD commands and 11
  REST-only Ops methods) with one apply
  path into the existing edit job, primitive placement, pick-then-form
  tools and the cursor snap; the command surfaces built from RoboCAD's
  command table (`cad/surfaces/`: menu bar, tools toolbar, right-click menu,
  Space and Q radials, command palette with key conflicts, parameter form);
  data-driven keys with two-step chords; read-only analysis overlays; the
  inspector's pivot and transform editors; five RoboCAD routes (copy and
  paste with placement, control points, curvature comb, continuity). `cad/`
  is now 17,063 lines in 54 files (wc -l after the review fixes, tests
  included; was 10,445 in 30 at fe6995d4; `ops/` and `surfaces/` 5,193;
  largest `panel.rs` 700, `ops/catalogue.rs` 698, `ops/mod.rs` 682,
  `actions.rs` 676, `inspector/mod.rs` 663, `sync/mod.rs` 582;
  `document.rs` became `document/` 774 in three files and
  `transform/mod.rs` 460).
  **cad-sketch** (2026-10-01, see [CAD sketch](#cad-sketch-2026-10-01)),
  verified at cc7ac194 (sim-spatial lib 293 passed, 1 ignored; bins 4; `cad_client` 47; `units` 29; api pytests 61; sim-web wasm check clean): the
  active plane (XY/XZ/YZ or a plane node, display state) and 2D snapping,
  the four plane tools, the 13 sketch tools on one data-driven
  interaction, sketch offset/fillet corners/join and the `cad_sketch` REST
  command, extrude and revolve with Shift/Ctrl/Alt booleans, sweep, pipe,
  loft and fill; the plane-dependent cad-modify operations and the
  primitives now follow the active plane. Recorded Python fixes:
  `Service.edit_sketch`'s curve indices, `ArgConverter` passing a node id
  through for `fill`, the kernel's `fill_hole` (`Shape()`, found by test in
  the verification pass), and named 4xx refusals with a rolled-back failed
  sketch create (verification pass). Re-measured 2026-10-01: `cad/`
  is 21,893 lines in 75 files (`find crates/sim-spatial/src/cad -name '*.rs' | xargs wc -l`,
  tests included; `sketch/` 3,148 in 11; `ops/` 4,032 in 17); the
  catalogue has 84 entries (`grep -c 'id: "' crates/sim-spatial/src/cad/ops/catalogue/*.rs`
  gives 68, plus the 16 `tool(`/`edit(` rows of `catalogue/sketch.rs`,
  `grep -cE '^\s+(tool|edit)\($'`; 54 before: 6 solids, 8 plane entries
  and 16 sketch entries added); `sim-runtime`'s `cad_client/sketch/` is
  930 lines in 2 files plus `sketch_tests.rs` 356.
  **cad-views-export** (2026-10-01, see
  [Shared camera and CAD views](#shared-camera-and-cad-views-2026-10-01)),
  written and reviewed by reading in 78553886, 9b1e5eec and f15766ea,
  verified at bcf0c56c (see [Verification result](#verification-result-bcf0c56c)): one
  shared camera for every orbit mode (`src/camera/`, `CameraPlugin`,
  `CameraAction` with 13 `camera_*` commands; CAD gets RoboCAD's presets,
  89.5° pitch, zoom to the cursor, trackball, ortho/FOV and its extra
  gestures), and in CAD mode the six display modes, grid, build plate,
  view cube, high contrast, the section preview and exact section, saved
  views in RoboCAD's view-state schema, isolate/hide/show all, per-node
  tessellation tolerance, and new, open, save as, import with units,
  export, drawing and render on jobs; three api.py gap routes (`POST /new`,
  `POST /save/thumbnail`, `GET /import/units`). Re-measured 2026-10-01 at
  f15766ea: `cad/` is 29,375 lines in 93 files (same `find … | xargs wc -l`;
  `display/` 2,569 in 6, `views/` 1,279 in 4, `files/` 2,224 in 5);
  `camera/` 2,458 in 7 (881 of them `tests.rs`); the catalogue has 87
  entries (the same two greps give 71 and 16; `view.isolate`,
  `view.show_all` and `view.hide` added); `cad_client` gains `views.rs`
  285, `section.rs` 108 and `files.rs` 178 lines, plus their tests.
  **cad-physical-inspect** (2026-10-01, see
  [CAD physical properties](#cad-physical-properties-2026-10-01)), done
  pending verification (written and reviewed by reading in 3fb34225,
  f26842fa and the review fixes committed in 697a15c1; nothing compiled
  or run): the
  materials panel and dialogs, the inspector's physical rows and exact
  measurement, the Robot panel with its tools, dialogs, motor library and
  glyphs, results and identification, the stress overlay (one colour rule
  with Robot mode), physical export on a job and the live link into Robot
  mode in this window; two api.py gap routes (`GET /results/nodes`,
  `GET /physical?planar=1`). Measured 2026-10-01 in the working tree:
  `cad/` is 39,198 lines in 128 files (same `find … | xargs wc -l`;
  `robot/` 2,326 in 10, `materials/` 1,283 in 4, `results/` 2,065 in 6);
  the catalogue has 101 entries (the same two greps give 85 and 16);
  `cad_client` gains `robot.rs` 491, `robot_ops.rs` 132 and `physical.rs`
  219 lines, plus their tests.
  **cad-checklist-traces** (2026-10-02): Parts G to J of the CAD
  checklist (CAD-131 to CAD-175, CAD-187 to CAD-213) are **traced by
  reading, unexecuted**: each step has a path:line trace from the control
  through `CadAction`, the feature's job, `cad_client` and RoboCAD's
  `api.py` route back to the display ("Reading traces — Part G/H/I/J" in
  [cad-checklist.md](../cad-checklist.md)), and the gaps found were fixed
  in e7b8393f, 83ebb812, 49bc553e, 0caaf92e, 38cf7745, e6c48005,
  ac973444 and b75846ff (export and exact-measurement stale guards and cancel texts,
  print late-cancel and stale-line texts, component job recovery and
  draft locking, the outliner and system-link reads, a refused Open in CAD
  dropping its pending reveal, the Autosave line read every poll tick,
  the results form consuming its Escape).
  CAD Escape now orders through public sets (`CadKeySet::EscapeTool`,
  `Escape`; the numeric bar in `CadKeySet::NumericEntry`). Nothing was
  compiled, run or compared side by side; the ledger records the
  differences found.
  **cad-parts-a-f-retrace** (2026-10-02): Parts A to F (CAD-01 to
  CAD-130) are traced by reading, unexecuted, against the current code
  ("Reading traces — Parts A and B" to "— Part F"), so **Parts A to J of
  the CAD checklist are all traced by reading, unexecuted, and goal 3 of
  the current focus (the CAD editor, with annotations) is complete by
  reading**. Gaps fixed: Open in CAD's reveal travels with its own switch
  request (`ModeSwitch::reveal`, installed only when that switch is
  accepted), the port race refuses a desktop RoboCAD, leaving CAD names a
  service left running, window patches are revision-checked, locked nodes
  are left out of or refuse a transform and face edit as RoboCAD's do,
  Measure picks through the select click's search, the surfaces' Escape is
  consumed (one press ends one thing), the palette's Enter is stamped,
  sketch refusals use RoboCAD's words and name dropped curves, a REST
  sketch naming curve indices needs `revision`, the sketch pointer runs
  after `CadSet::Plane`, export and render have a Cancel that says what was
  written, a saved view's restore ends a part view, the CAD camera starts
  at RoboCAD's iso view and 40° field of view, and the orbit toggle writes
  RoboCAD's "Orbit: …" line. No executed evidence is claimed; the ledger's
  Counts were recounted by script.
- **Phenomena mode and planar v2 robot files** (see
  [Fold in sim-app](#fold-in-sim-app-2026-09-30)), written 2026-09-30 and
  verified at 80b5997e (sim-spatial lib tests 172 passed, 1 ignored;
  `cargo check --workspace --all-targets --locked` clean): `ViewerMode::Phenomena`
  (`src/phenomena/`) runs `sim_phenomena::exhibits` on the "phenomena-run"
  `RunThread` with sim-app's pacing, `PhenomenaAction` and kit panels; robot
  mode opens planar v2 `*.simrobot.json` files through
  `cad_robot::build_planar` on the "robot-run (planar v2)" `RunThread`
  (`src/robot/planar/`). `crates/sim-app` is deleted; its ledger is
  [docs/sim-app-parity.md](../sim-app-parity.md) (58 rows, none open). Every
  child process starts in `jobs` (`spawn_detached`, `open_in_browser`,
  `ChildProcess`), enforced by `jobs::tests::processes_are_started_only_in_jobs`.
- **Hardware in process** (§8, leg-in-process LIP1–LIP3, 2026-10-03;
  implemented and source-reviewed, unexecuted): Robot mode's Leg panel,
  calibration/gait/mirror and Sync motors use shared Rust application sessions
  and caller-owned `jobs::RunThread` workers. Native Lessons lab steps use the
  same calibration layer. `--hardware-config FILE` and `--motor-bench-config FILE`
  replace URL/token arguments, which refuse by name. STOP latches independently
  of queued acquisition and reports release/readback separately. The browser
  examples are compatibility adapters; no native hardware server, virtual socket
  server or acquisition child is required. [Source parity and run sheets](../leg-in-process.md)
  distinguish host virtual simulation from physical FPGA acquisition.
  **CAD still depends on RoboCAD's service**; §9 is not implemented by this batch.
- **File size: closed and guarded** (split-large-files, 2026-10-01; see
  [Split large files](#split-large-files-2026-10-01)). No non-test source
  file in `crates/sim-spatial/src` is over 750 lines, and the lib test
  `app::tests::source_files_stay_small` keeps it so (cap 750 non-test
  lines, empty allowlist). Re-measured 2026-10-01 after cad-sketch
  (non-test lines, the guard's rule, by a script repeating its count): the
  largest are `cad/panel.rs` 716, `cad/actions.rs` 705,
  `robot/hardware/session.rs` 698, `cad/ops/mod.rs` 686,
  `robot/hardware/sync.rs` 679, `physics_view.rs` 672,
  `builder/actions.rs` 661, `lesson/mod.rs` 657; every other file is under
  655 (`cad/ops/catalogue.rs`, 698 at split-large-files, became
  `cad/ops/catalogue/`, nine files). The former giants are now short roots: `robot/mod.rs` 201
  (was `robot.rs` 2,532), `robot/run/mod.rs` 30 (was `robot_run.rs`
  2,647), `builder.rs` 590 (was 2,453), `lesson/mod.rs` 657 (was 2,371),
  `lib.rs` 64 (was 1,297), `app/switch/mod.rs` 536 (was `app/switch.rs`
  959). Trees (wc -l, tests included): `robot/` 18,902 lines in 67 files
  (`run/` 2,736 in 11, `hardware/` 9,139 in 30), `builder*` 11,364 in 33,
  `lesson/` 7,193 in 23, `app/` 2,646 in 9, `inspect_view/` 1,280 in 5;
  `src/` 68,672 lines in 228 files at split-large-files, 73,269 in 249
  after cad-sketch. Robot mode is one module tree under
  `robot/`, as CAD mode is under `cad/`. Re-measured 2026-10-01 after
  cad-views-export at f15766ea (the same script): the largest are
  `cad/files/form.rs` 734, `cad/panel.rs` 716, `robot/hardware/session.rs`
  698, `cad/ops/mod.rs` 698, `robot/hardware/sync.rs` 679,
  `physics_view.rs` 672, `cad/display/draw.rs` 671, `builder/actions.rs`
  661, `lesson/mod.rs` 659; every other file is at most 654
  (`cad/actions.rs` fell from 705 to under 640 when `snapshot.rs` and
  `rest_form.rs` were split out). `src/` is 83,045 lines in 274 files
  (wc -l, tests included; `camera/` 2,458 in 7).
- **Bevy-practice gaps** (audited 2026-10-01 against
  `tools/claude-pair/prompts/bevy.md`; counts by grep over non-test
  sources, nothing compiled). No pre-0.19 APIs remain: 0 `EventReader`,
  `Trigger<`, `despawn_recursive` or `StateScoped`, and the simulation
  boundary holds, because `Res<Time>` drives only animation and playback.
  Open gaps, each with an epic ID in [Default epic order](#default-epic-order):
  - **Private ordering edges closed by reading** (`public-system-sets`,
    uncompiled): public Input, robot, CAD, inspect and text sets replace
    cross-feature private ordering. The baseline had 82 matching grep lines,
    107 qualified function-path occurrences and five bare occurrences,
    including 34 serve edges; previous 94/66/24 counts are stale. See
    [Public system sets](#public-system-sets-2026-10-01) for every relation,
    remaining local edges, source guard and windowless schedule tests.
  - **Hand-computed 3D viewports** (`viewport-nodes`): `camera/viewport.rs`,
    `view.rs:158` and `:296` (the split schematic) and `place_view.rs:294`
    set `Camera.viewport` from dock sizes every frame. 0.19.1's
    `ViewportNode` (`bevy_ui-0.19.1/src/widget/viewport.rs`, with
    `viewport_picking` under the `bevy_picking` feature) renders a camera
    into a UI node, so the dock layout owns where a view goes. 0 uses.
  - **Persisted settings gap closed by reading** (T45, uncompiled):
    `app/settings::SettingsOwner` is the one jobs-owned load, migration,
    validation, revision, save and publication owner for recents, inactive
    hardware forms and remembered CAD print defaults. The pinned
    `bevy-settings =0.19.1` contracts are registered through a narrow custom
    seam; stock SettingsPlugin is not installed. See [viewer preferences](../viewer-preferences.md).
  - **Small gaps closed by reading** (public-system-sets, uncompiled):
    `physics_view::labels` retains keyed entities and changes Node/Text with
    `set_if_neq`; rounded values, refresh pacing and anchor rules stay intact.
    `selection::apply_actions` uses five resource system parameters instead
    of exclusive World access. `FieldMsg` derives Message.
  - **Deliberate, not gaps:** held/continuous controls retain paired
    Interaction handling; ordinary activation is migrated in T49 below. CAD
    face picks by `MeshRayCast` (see CAD selection and transform), and the
    shared camera and grid instead of Bevy's controllers and `InfiniteGrid`
    (see Shared camera and CAD views).
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
  - **Later addition (rover-drive-layers):** `bevy_gilrs` (it enables
    `gamepad`), for the drive bindings' gamepad sticks and buttons;
    `DefaultPlugins` then adds `GilrsPlugin`
    (`bevy_internal-0.19.1/src/default_plugins.rs:82-83`). `Cargo.lock`
    resolved offline (`cargo update --offline -p sim-spatial`, about 3 s):
    `bevy_gilrs` 0.19.1, `gilrs` 0.11.2, `gilrs-core` 0.6.8 and their
    platform crates (on Linux `libudev-sys`, which CI already installs). See
    [Teleoperation](#teleoperation-rover-drive-layers-2026-10-02).
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
    - Compile (feeds the schematic and the scene): start `builder/rebuild.rs:7`
      (Compute); apply `rebuild_scene` `builder/rebuild.rs:89`. Changed only on a panic, which now
      sets `compile_error` and the status (it used to wait forever).
    - Open system: start `builder/open.rs:192` (Compute, generation = open
      seq); apply `finish_open` `open.rs:209` from the `open_system` system
      `builder/background.rs:6`. The previous builder is dropped by
      `jobs::drop_off_thread` at `open.rs:292`, so its run thread and agent
      are joined off the UI thread. Feeds `system_state.open.last` and the
      status line. Unchanged.
    - Live run start/pause/step/reset: `LiveRun::spawn` `builder/live_run.rs:11`
      (RunThread "builder-run"), started from `start_run` `builder/live_run.rs:55`.
      Commands `builder/live_run.rs:161` (Start), 165 (Pause), 172 (Step) and 196
      (Reset); the loop at `builder/live_run.rs:248` is unchanged. Read by
      `live_run_json` `builder/live_run.rs:226` and `running()` from the shared
      snapshot. A replaced run drops its RunThread (bounded 200 ms join).
    - Run replay: start `builder/studies.rs:44` (Dedicated, `replay_with_cancel`
      given the job's cancel flag); apply `poll_replay` `builder/studies.rs:69`
      (called at `builder/live_run.rs:428`); cancel `builder/studies.rs:57`. Feeds
      `system_state.replay` via `replay_json` `builder/studies.rs:93`. Unchanged.
    - Study progress and cancel: start `builder/studies.rs:117` (Dedicated, steps
      progress); apply `poll_study` `builder/studies.rs:146` (at `builder/live_run.rs:427`);
      progress `study_progress` `builder/studies.rs:142` into
      `system_study_result.running` and `study_json`; Cancel
      `builder/actions.rs:299` calls `Job::cancel` (fixed in 4b74edc6; it used the
      removed flag and did not compile).
    - Actuators tab: start `builder/actuators.rs:178` (Io, generation = seq);
      apply `finish_actuators` `actuators.rs:194` (system at
      `builder/background.rs:13`). Measured evidence: start `builder/calibration.rs:351`
      (Io); apply `finish_calibration` `calibration.rs:367`
      (`builder/background.rs:20`). Gait lab: start `builder/gait_lab.rs:147` (Io);
      apply `finish_gait_reports` `gait_lab.rs:166` (`builder/background.rs:27`).
      They feed `system_state.actuators`, `calibration_review` and
      `gait_reports` (`last` = `(seq, result)`). Unchanged.
    - Source preview and agent context: `builder/reference.rs:31` (Io; a
      web link's `open` process is reaped with `jobs::reap_child`,
      `reference.rs:24`; since fold-sim-app `jobs::open_in_browser`) and `builder/agent.rs:159` (Compute, polled at
      `agent.rs:172`). Unchanged apart from the reaping.
  - *Placement*
    - Drag validator: `builder/placement_worker.rs:15` (RunThread
      "placement-validator", join bound 0). Pointer moves `submit` at
      `builder/placement.rs:535`; the result is taken at `placement.rs:538`.
      The worker drains the channel and validates only the newest position
      (the condvar became channel commands; latest-wins is kept, and a
      closed channel starts nothing). Changed in mechanism only.
    - Commit on release: `placement.rs:595` (Io, `complete_on_drop`); apply
      `poll_drop` `placement.rs:602` (at `placement.rs:505`). Unchanged.
  - *Robot*
    - Preset open and preset switch: `RobotView::open_preset` `robot/state.rs:16`
      starts the loader at `robot/state.rs:22` (Compute); apply `receive`
      `robot/scene.rs:23` (`load.poll()` at `robot/scene.rs:41`). A switch
      (`Request::RobotPreset`, now `RobotAction::OpenPreset` at `robot/actions/mod.rs:458`) replaces the whole view, so
      the old run, gait and playback RunThreads drop with a bounded join.
      Feeds `robot_state.status`, `preset` and `load_seconds`. Unchanged.
    - Recording save: command `save_recording` `robot/run/preset_ops.rs:142`; the run thread snapshots
      and hands the write to `robot/run/worker.rs:143` (Io, `complete_on_drop`,
      publishes `Published.save`); apply `RunController::poll`
      `robot/run/controller.rs:268`. Feeds `robot_state.recording.pending/last_saved`.
      Changed: a writer panic is now published as the save error (before
      and after the migration it was lost and `pending` never cleared).
    - Recording list: `Latest` at `robot/run/preset_ops.rs:184` (Io; a newer list
      supersedes); apply `robot/run/controller.rs:287`. Feeds `robot_state.recordings`.
      Unchanged.
    - Replay: runs on the robot-run RunThread (`Command::Replay`, loop from
      `robot/run/worker.rs:155`), published as `Published.replay` and accepted in
      `poll` only for the current generation and seq. Unchanged.
    - `--robot FILE` reload: `robot/source.rs:163` (Compute, sha256 and
      parse); taken by `receive` via `SourceWatch::take` (`robot/scene.rs:49`); a
      loaded reload replaces the run with generation + 1. Unchanged.
    - Gait preview: RunThread "robot-gait" `robot/gait.rs:171`; apply `poll`
      `robot/gait.rs:269` (the listing is `Shared.listing` with its seq).
      Recorded seek and play: RunThread "robot-recorded"
      `robot/playback.rs:166`; apply `poll` `robot/playback.rs:182` via
      `RunThread::latest(generation)`. Feed `robot_state.gait_preview` and
      `recorded`. Unchanged loops.
    - Stress results reader: `robot/stress.rs:108` (Io), polled at
      `robot/stress.rs:115`. Unchanged.
    - `sim-viewer` children: `main.rs:241` and `main.rs:392` use
      `jobs::reap_child` (since fold-sim-app `jobs::spawn_detached`).
  - *Lessons*
    - Figures: `lesson/practice/mod.rs:34` (Compute); apply `poll_figures`
      `practice/mod.rs:64` (at `lesson/watch.rs:93`). Unchanged.
    - Model: `lesson/extras.rs:68` (Dedicated, streamed values); apply
      `poll_model` `extras.rs:98` (at `lesson/watch.rs:110`). Feeds
      `lesson_state.model`. A panic now shows under `model.errors`.
    - Comparisons: `lesson/handler.rs:385` (Dedicated, steps progress); apply
      `lesson/watch.rs:99`; "Running n/m…" from `lesson/ui/cards.rs:54`. Feeds
      `lesson_state.compares`. Unchanged.
    - Scene recordings: `lesson/opening.rs:140` (first) and `opening.rs:197`
      (re-record), both Dedicated and streaming `Stage`s; apply from
      `lesson/watch.rs:131`; progress `ActiveScene::progress` `lesson/mod.rs:334`
      into `lesson_state.scene.recording` and the "Recording on the shared
      runtime… n %" line (`lesson/ui/scene_card.rs:304`). Changed: a panic in the job
      now ends the recording with its error instead of leaving the scene
      "recording".
    - Narration progress (never run generation here): `lesson/narrate/mod.rs:268`
      (Dedicated; paid work is not cancelled mid-request); apply
      `narrate/mod.rs:386`; the line is `GenJob::progress` `narrate/mod.rs:55`
      ("Starting…" until the first message) and `lesson_state.narration.job`
      (`lesson/actions.rs` `narration_state`). Unchanged text.
    - Practice bench request: `lesson/extras.rs:208` (Dedicated), polled at
      `extras.rs:142`. Unchanged.
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
    `robot::run::tests`.
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
  - `app/switch/`: `ModeSwitch`, `Switcher`, `Documents`, the handler
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
  [Action layer](#action-layer-2026-09-30). Since cad-views-export
  `camera_viewport` and the per-mode orbit systems are gone: the spatial
  view's first chain ends in `sync_camera` `.before(CameraSet::Viewport)`
  and its second runs `.after(CameraSet::Place)`, the builder and lesson
  chains run `.before(inspect_view::sync_camera)`, place's `fly` is
  `camera::fly` in `CameraSet::Place`; see
  [Shared camera and CAD views](#shared-camera-and-cad-views-2026-10-01).)
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
    are off for this run). *Superseded by window-first-usability:* the
    switcher is now the reserved strip `Dock::Strip` along the whole bottom
    edge (`ui_kit::SWITCHER_STRIP`, 40 px), the segments on the right and
    the last outcome on the left, wrapped and clipped to two lines; every
    dock ends above it. A mode with no document opens the document picker.
    See [Window-first usability](#window-first-usability-2026-10-01).
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
    click on a link in the 3D view could not select it (robot/actions/keys.rs
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
    OnEnter(Inspect scope) `setup_scene` + `setup_ui` (inspect_view/mod.rs:303).
  - Build: `build_mode` (main.rs:235: `Builder::open`,
    `builder::compiled_scene`, annotations, models, `enable_open`) →
    OnEnter(Builder scope) `setup_scene` (inspect_view/mod.rs:304); the builder's chrome
    is built by `ui::rebuild_panel` in Build.
  - Lessons: `lessons_mode` (main.rs:173: `lesson::open_lessons`) →
    OnEnter(Lessons) `arrive` then `show_lessons` (a no-op: a new `Learn`
    is already shown) → OnEnter(Builder scope) `setup_scene`.
  - Robot: `robot_mode` / `robot_preset_mode` (main.rs:195/208:
    `RobotView::open` / `open_preset`, loading on their jobs as before) →
    OnEnter(Robot scope) `robot::ui::setup` (robot/ui.rs:112).
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
    `builder::ui_api::tests`, `builder::replay_tests`, `robot::run::tests`,
    `rest::tests` and the `inspect_view/tests.rs` `keyboard`/`pick_part` test;
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
    | Robot | `robot::RobotAction` (`robot/actions/mod.rs`) | `robot::actions::apply` | `RobotPlugin` |
    | Place | `place_view::PlaceAction` | `place_view::apply` | `PlacePlugin` |

  - Input mappings (Input): `app::switcher::switcher_clicks`,
    `app::switch::lesson_screen_requests`, `inspect::input`,
    `notes::clicks`, `physics_view::overlay_clicks`,
    `builder::actions::{buttons, keys}`, `lesson::actions::{buttons,
    keys}`, the lesson timebar, slider and narration bar (`lesson::seek`,
    `lesson::sliders`, `narrate::seek`), `robot::actions::{buttons,
    motion_keys, graph_key, overlay_keys, speed_keys}`, `place_view::keys`,
    and the pick observers (`inspect_view/scene.rs` `pick_part`, `linked::pick_net`,
    `builder::pick_reference`, `robot::actions::pick_link`). The builder's
    `text_input` (SimSync; since one-text-entry `drafts::sync_field`, over
    the kit fields `builder.draft`/`builder.note`) edits the draft and writes its Enter/Escape as
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
    Codex answer, pending open; `switch/prepare.rs` `leaving_blockers`), or
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
    `notes::update`. Builder and lessons: before `text_input` (now `drafts::sync_field`) and the lesson
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
    `robot::run::tests`, the `inspect_view/tests.rs` pick/button test);
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

- **Ordinary buttons: UI Button plus pinned widget Button/ActivateOnPress.**
  T49 supersedes the pointer-only decision: UI Button/action/Enabled/label remain
  for system_ui discovery, while pinned Activate preserves primary pointer-down
  timing and adds focused Enter/Space with repeat suppression. Kit capture records
  original entity intent; InputSet::Window converters feed existing typed owners.
  Eligibility synchronizes InteractionDisabled and removes hidden/disabled indexes.
  InputFocus remains the single owner; pinned TabNavigation, modal groups and kit
  outlines supply navigation and visible focus. Durable editor anchors retain
  source identity/drafts through presentation rebuilds. HeldControl explicitly
  excludes paired motion; KeyboardOnly permits compound tree keyboard selection.
  Rejected alternative: fabricating Interaction presses or feature keyboard loops.
  Revisit after authorized execution exposes an input/lifetime defect. See the
  [T49 inventory and source evidence](../native-keyboard-activation.md); all new
  fixtures, compilation and GUI interaction remain unexecuted.
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
- **Text input: one kit field on `InputFocus`, not Bevy's `EditableText`**
  (one-text-entry, 2026-10-01; first decided here as "not adopted"). Every
  text site is a `ui_kit::text` field: a `TextDraft` edited by one input
  system on Bevy's `InputFocus`, with submit, cancel, tab, arrows and blur
  delivered as `FieldMsg` data and one `typing` run condition for every key
  map (the `typing` run condition or the `Typing` parameter). `EditableText` lacks a placeholder, submit handling and AccessKit
  support and needs persistent editor nodes the kit's rebuilt panels do
  not keep; the API facts and reasons are in
  [One text entry](#one-text-entry-2026-10-01). The builder's draft
  semantics (the "/" filter key is not typed, Escape leaves Connect, a
  draft survives a mode switch) are kept and traced there.
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
    the inspector scroll (`InspectorScroll` + `robot::scroll`); no slider
    (since window-first-usability: the recorded timeline's seek slider,
    `Kit::slider(Timebar)` → `SliderValue` + `slider_held` in
    `robot::panel_ui::recorded_seek` → `RobotAction::Recorded { Seek }`).
  - Inspect: a toolbar chip (`InspectAction` → `inspect::input`); the
    inspector scroll (`scroll_inspector`); no slider.
  - Switcher: a mode segment (`ModeButton` → `switcher_clicks` →
    `WindowAction::Switch`).
  - Place: no buttons or sliders (keys and fly camera only).
- **Verified at 4bc03789**, after the fixes in 9b6aa068, 801f12c5 and
  4bc03789.

## Hardware front end (2026-09-30)

Historical HTTP-era implementation. The current in-process owners and launch
path are in §8 and [leg-in-process.md](../leg-in-process.md); the dated receipts
here establish only their stated historical code, not the new migration.

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
    shared-owner inactive preference seeding (T45), the lifecycle `enter` (OnEnter
    Robot: connects with `--hardware`) and `leave` (OnExit Robot: calls
    `stop_immediate` and `LiveSync::stop_ours` directly, keeps the
    preferences, then drops the state off the UI thread), `stop_immediate`,
    `connect`, the input systems (`actions/input.rs`) `buttons`, `jog_buttons`, `keys` (Q/A
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
    `release`, `set_disabled`, `stop_after_dropped`, `interrupted`,
    `time_of_day`.
    - `session/periodic.rs`: `poll`, `next_deadline`, `run_due`,
      `shutdown`.
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
    `PanelView::block`, and `status_json` (`view/status.rs`; REST `hardware_status`).
  - `dial.rs` (`needle_end`, `rasterize`) and `motion_view.rs`
    (`comparison`, `update`): the dial and the command-vs-motion chart.
  - `panel.rs`: `spawn` (the dock: `FocusPolicy::Block`, the accessible
    label), `panel_view`, `control_list`, `controls` (the `hardware:<name>`
    list for `system_ui`), `connection_line`, `scroll`, `refresh`;
    `panel_sections.rs`: `top_bar`, `body`, `section`, `chips`, `gaits`,
    `stats`, `runs`, `chart_labels`.
  - `mirror.rs` (`Mirror::prepare`, `update`, `follow_gait`, `poll`,
    `alignment_angle`, `gait_bindings`, `record`, `worker_failed`,
    `SceneId`; `apply` in `mirror/apply.rs`, `worker` in `mirror/thread.rs`) and `mirror_panel.rs` (`mirror_sync`, which drives
    begin/prepare, updates and gait sampling every frame; `mirror_panel`,
    `fill`): the suspended robot posed from the encoders through
    `sim_runtime::kinematic_mirror::KinematicMirror` on a jobs worker,
    written into `RobotView::mirror`.
  - `sync.rs` (`LiveSync::connect`, `start`, `stop`, `stop_ours`, `poll`,
    `poll_status`, `on_frame`, `watch_run`; in `sync/page.rs` `live_input`,
    `sample_from`, `source_text`, `legs`, `mapping`, `distinct`,
    `banner_text`, `reading_lines`; `apply` in `sync/apply.rs`; `drain` and
    `worker` in `sync/thread.rs`) and `sync_panel.rs`
    (`sync_frames`, `sync_panel`, `row_title`, `rms_and_saturation`,
    `charts`, `chart_note_of`, `sync_overlay`, `sync_texts`): Real motor
    sync against `serve_motor_bench`.
  - `settings.rs`: typed validated hardware preferences and exact legacy `path`;
    `app/settings` owns all loading, migration and publication (T45).
- Outside the folder: `app/actions.rs` (`Origin::SystemUi`,
  `Action::accepts`, `Call::remote`, the registry entry "hardware"),
  `robot/actions/mod.rs` and `keys.rs` (`apply`: `system_ui` passes `hardware:<name>` on,
  refuses motion by name and a disabled control with "{id} is disabled:
  {why}", and lists the controls; `motion_keys`: A is given to the panel
  while it is open; `check`: Run refused while mirroring), `robot/ui.rs` and `robot/scene.rs`
  (`setup`: the header button; `highlight`: `RobotView::mirror` drawn
  instead of the run's frame, the blue `Materials::mirrored`; `scroll`: the
  inspector ignores the wheel while the panel covers it), `robot/run/frames.rs`
  (`MotorTargets.done`, the frame's episode end), `chart.rs`
  (`rasterize_fixed`, fixed axes for the sync charts), `main.rs` (the four
  flags), `app/switch/mod.rs` (`Documents::hardware`).

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
- **Refusal rule (superseded by LC1/LC2, 2026-10-02).** The original blanket
  rule refused every `HardwareAction::starts_motion` action from
  `Origin::Rest` or `Origin::SystemUi`. The rule now:
  - *Remote motion* goes through one policy, `HardwareAction::authorize`
    (`crates/sim-spatial/src/robot/hardware/actions.rs`, refusing with
    `remote_refusal`): allowed only on a verified virtual calibration link
    (server and bench identity plus the current connection generation,
    fresh) and only for the in-scope calibration actions, which include the
    gait settings and Play since HW-10 (2026-10-02: `GaitSelect`, `GaitMode`,
    `GaitSpeed`, `GaitEffort`, `GaitConfirm`, `GaitPlay`; a remote Play goes
    to the link as a checked command and answers once the gait started).
    Physical, unknown, mismatched or replaced endpoints are refused, and
    flip, raw step, live sync and the mirror's bindings (leg, joint,
    polarity, alignment pose) are refused remotely on every link. The gait's
    Stop is never refused, like STOP. Robot mode refuses a disabled `hardware:<name>`
    control itself, and the hardware handler checks again
    (`handlers::remote_check` in `crates/sim-spatial/src/robot/hardware/handlers.rs`).
  - *Only a lost binding revokes.* An identity mismatch, a stale or replaced
    connection generation, or a lost virtual bench is answered HTTP 409 with
    `calibration::BINDING_REFUSED` (`crates/sim-runtime/src/hardware_client/calibration.rs`);
    the native link then revokes its pinned authorization until an explicit
    reconnect (`calibration::binding_lost`, which also counts transport and
    decode failures; `crates/sim-spatial/src/robot/hardware/session.rs`).
    A status whose execution changes revokes the same way: to another
    identity ("virtual execution identity changed") or to none, the server
    having lost its virtual bench ("the virtual calibration bench was lost
    (disconnected)"), both "reconnect required".
  - *Everything else is an ordinary refusal.* Any other refusal, including
    an out-of-scope command on a virtual bench (flip, raw step, lesson
    motion: anything `calibration::virtual_command_allowed` rejects), is HTTP
    400: it never runs, keeps the binding, and is shown as an ordinary
    refusal (the REST answer, or the panel's notice for a one-way
    `system_ui` activation; a one-way jog press the link refuses after
    answering also reaches the notice, through `handlers::settle_presses`,
    a late refusal of any other one-way command is not re-reported there).
  - *STOP is never authorization-gated* (status, connect, sections, export
    and loss other than `leaving` are always allowed too). *JogRelease is
    never refused remotely while a link exists* (`authorize` passes it like
    STOP, as refusing it would leave the motor moving), and the
    `hardware:jog_upper` / `hardware:jog_lower` controls list `JogRelease`
    while their direction is held (`panel::control_list` in
    `crates/sim-spatial/src/robot/hardware/panel.rs`). A refused remote press
    puts its direction's held flag back to what it was before the press, so
    an operator already holding that direction keeps the hold and their
    release still reaches the link.
  - *On a virtual link* flip and raw step are listed disabled with
    `panel::OUT_OF_VIRTUAL_SCOPE`, for the operator too. Leg and Both gait
    playback is in scope (HW-10, 2026-10-02): `gait_start` and `gait_update`
    are in `calibration::virtual_command_allowed`, and the server's one
    `run_gait` loop drives the virtual bench exactly as it drives the leg,
    with every precondition (suspended confirmation, bindings, taught poses,
    proven watchdogs, alignment session, the 95 % fit) unchanged.
  - *Virtual gait runs are labelled simulated* the way exports are: the run
    record under `gait-runs/`, the `gait_start`/`gait_end` events in
    `gait.jsonl`, the live `gait` status and each `gait_runs` history row
    carry `execution` and `simulated` (a row from an older record without
    the field reads as not simulated). The panel's Leg line and each
    simulated Recent leg runs heading start `VIRTUAL (simulated) · `
    (`view::render_gait`). Every record also names the supply its limits
    were computed at and whether it was assumed (`supply_v`,
    `supply_assumed`), and a virtual one lists what the bench does not
    emulate (`virtual_limits`).
  - *The suspended-leg confirmation does not survive a new link*
    (`actions::poll_jobs`): it can now be ticked remotely on a virtual
    bench, and must never carry to a physical leg after a reconnect.
  - *A virtual export is labelled simulated* (`handlers::write_export`): the
    file carries `execution` and `"simulated": true` and is named
    `leg-calibration-<unix_ms>-virtual.json`, also when the link has no
    virtual pin but the server labelled the document itself; the panel's
    export line starts `VIRTUAL (simulated)` and REST `hardware_export`
    answers `{"path", "simulated"}`.

  See "Virtual transport isolation (LC1)" and "Remote acknowledgement (LC2)"
  below.
  *Why:* AGENTS.md: drive motors only with the operator present; a simulated
  bench has no motor to protect, and the binding stops a virtual
  authorization from reaching a physical server. *Revisit if* an
  operator-presence signal other than a local pointer or key exists.
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
- **Run, Step and Reset refused while mirroring.** `robot::actions::check`
  (and `check_planar`) refuse `Run { Start | Step | Reset }` by name while
  `RobotView::mirror` is set (`mirror::refuse_run`: "Run refused: …",
  "Step refused: …", "Reset refused: …", each with `mirror::MIRRORING`);
  Pause stays allowed. The run buttons are dimmed through the same `check`
  (`robot::scene::highlight`), and a click, `system_ui` and REST
  `robot_run` all reach it. *Why:* the page's `setPlaying` refuses to play
  while mirroring, and the mirror's poses replace the run's frame; Step
  and Reset would advance or rebuild that run too (focus-safety-closure,
  2026-10-03; by reading, unexecuted). *Revisit if* the mirror is drawn as
  a second robot instead.
- **A refused `gait_start` releases the motor** (focus-safety-closure,
  2026-10-03; a deliberate safety difference from the browser page, which
  leaves the selected motor held). When the server refuses a Leg or Both
  gait start after the stop and select, `Session::start_gait` releases the
  selected motor through the existing STOP request
  (`session/sequences.rs` `release_after_refused_start`, `Session::stop`)
  and reports "gait_start refused: …; motor N released" in the panel's gait
  line and REST `session.gait_notice`. A 200 answer whose gait is not
  running and carries an error is now treated as a refusal too (it used to
  read as a started gait). Ledger: `docs/hardware-parity.md` CAL-157,
  MIR-38. *Why:* AGENTS.md hardware safety: a motor nobody is driving must
  not stay held. *Revisit if* the user prefers page parity for that case.
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
- **Preferences, one asynchronous owner (T45).** `app/settings` reads and
  publishes validated inactive forms; `hardware::actions` is a consumer
  adapter, not a disk backend. The exact legacy `$SIM_SPATIAL_PREFERENCES` or
  `$HOME/.config/sim-spatial/hardware-preferences.json` import identity is
  retained in the unified file; no alternate platform search is added.
  Missing fields retain their existing defaults, polarity normalizes to ±1,
  and alignment remains a CAD pose reference. Loaded choices never arm sync,
  jog, drive or controllers, restore confirmations or send hardware commands.
  Calibration, taught travel windows and measured models remain source-owned.
  See [compatibility and retry](../viewer-preferences.md).
- **Warnings stamped in UTC.** *Why:* the viewer carries no timezone
  database; a local offset guessed without one could be wrong. *Revisit if*
  a timezone crate is added for another reason.
- **Stale status shown as stale** (a native addition). A status older than
  2.4 s (four idle polls) is marked stale, never shown as live, and the
  motion controls are blocked. *Why:* the page silently keeps the last
  answer; the native link publishes a timestamp, so it can say so.
  *Revisit if* the poll periods change.
- **One leg gait clock (HW-10 Both, 2026-10-02).** The server's
  `state.gait.t` is the clock. Its one writer on the native side is the
  session's `leg_frame`, which copies it into `GaitRun::t` at the end of
  every `adopt` (so the base and `read_at` change together); readers derive
  the time now with `LinkSnapshot::leg_clock` → `link::leg_gait_time`:
  advanced by the time since the read × the server's `speed_scale` only while
  the data is live, the run has started and is not paused here, and the
  server reports the `playing` phase, and never by more than `POLL_ACTIVE`
  (150 ms) past the read. The mirror's Both sample uses it every frame
  (`Mirror::follow_gait`); a new read that puts it behind the last sample by
  at most one capped interval holds the sample there instead of stepping
  back. The gait line shows the read time (steady between reads) and
  `hardware_status` the clock's. Sim-only playback keeps `sim_frame`'s wall
  time × scale. *Why:* the leg is the source of truth for where the real
  motion is; the cap keeps the display from claiming motion the server has
  not reported. *Revisit if* measured status latency shows the cap too
  tight or too loose (the cap is per read, so each request's own latency
  shows as a short hold).
- **Link health: stale and disconnected are never shown as live
  (2026-10-02).** `LinkSnapshot::health` (Waiting, Live, Stale with its
  age, Disconnected with why) is the one judgement. `mirror_sync` checks it
  every frame, before anything can pose (a stale link brings no new
  revision): while not live the mirror solves no pose from the encoders,
  holds the last one, and its line leads with "Leg data stale — last read
  N s ago; not live" or "Leg disconnected — why; not live" (whole seconds);
  live again, it is updated by force. The gait line freezes its clock with
  the same note. `LinkSnapshot::disconnected` is set by the session when a
  status reports the bus disconnected after reporting it connected (a STOP
  that lost readback, a lost bench), when a pinned execution is gone or
  replaced, or when a request finds the binding lost; it clears on a
  connected status, except for a revoked virtual pin. The panel's status
  line then starts "DISCONNECTED — why." without blocking motor selection
  (selecting a motor is how the server reopens its bus), and
  `hardware_status` reports `link_state` (none, waiting, live, stale,
  disconnected) and `link_note` next to the unchanged `connected`/`stale`.
  *Why:* AGENTS.md: never show old encoders as live. *Revisit if* the link
  gains a push channel that makes the poll age meaningless.
- **Captures answered after the save (2026-10-02).** `capture_hold` waits
  (≤ 1 s for the hold session to take it, ≤ 2 s more for the outcome) and
  answers 200 with the full state once the pose is saved, or 400 with the
  reason ("Still settling…"); a capture never taken is withdrawn, and taking
  or saving one renews the motion lease. The native session adopts the
  answered state (`session/buttons.rs`), so the reply to REST/`system_ui`
  and the panel change only on a confirmed save, and the mirror at once
  shows "{role}: 0.0° from its alignment pose". An answer that is not a
  full status (an older server's `{"ok":true}`) is an error, never taken as
  saved. Save sim alignment without a mirror angle (no model loaded) still
  saves the reference without one, as the page does; the mirror and the
  gait then take CAD home. *Why:* an Ok before the save let a caller act on
  a pose that was never stored. *Revisit if* the server gains a capture
  receipt id.
- **Reconnect after a lost virtual bench (2026-10-02).** A lost bench
  cannot come back on the same server (`lose_bus` clears its execution
  until restart). Reconnect pins through the existing verification whenever
  the server reports a virtual execution (a restarted bench: new identity,
  new generation); otherwise the link stays unpinned ("physical or
  unknown"), remote motion is refused, and the panel's notice says why
  (`actions::BENCH_GONE`). *Rejected:* re-pinning to a remembered identity
  (the bench is gone; nothing could verify it). *Revisit if* the server can
  reopen a virtual bench without a restart.
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
    ignored while the panel covers it (`robot/scene.rs` `scroll`).
  - `toFixed` ties rounded to even: fixed, `view.rs` `fixed`.
  - The gait's Stop for a leg gait waited behind the link: fixed, it goes
    through `stop_immediate` first (`handlers.rs` `handle`, `GaitStop`).
  - A `system_ui` activation of a disabled control read as success (its
    outcome is dropped): fixed, robot mode refuses it with "{id} is
    disabled: {why}" (`robot/actions/mod.rs` `apply`).
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
  - Settings file work: the historical adapter read at app build; T45
    replaces both that read and `Settings::save` with asynchronous jobs-owned
    loading/publication through `SettingsOwner`.
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
   (`robot/actions/keys.rs` `motion_keys`).
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
(`robot/actions/mod.rs` `moves_synced_motors`, `SYNC_REMOTE_REFUSAL`); Pause,
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
    `CadInputFocus` (deleted by one-text-entry: `ui_kit::text`), `switch_blockers`, `leaving_note`, `release_child`.
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
    fit (since cad-views-export: the shared camera's spawn, `rules()`,
    `fit` on `CadMeshes::epoch` and `gate`; no orbit of its own). `keys.rs`: RoboCAD's keymap (below).
  - `actions.rs`: `CadAction` and its one handler `apply` (Actions),
    `system_ui` (its controls are `panel::controls`), `cad_state`,
    `publish` (`/v1/state`, `/v1/cad_state`).
  - `panel.rs`, `tree.rs`, `inspector.rs`: the top bar, left dock (service,
    connection, autosave, stale; the tree), right dock (name field,
    summary, transform, detail, physical link, attributes, history,
    commands) and status bar, on the UI kit; refreshed only when
    `CadDocument.revision` changes.
- Outside `cad/`: `app/mod.rs` (`ViewerMode::Cad`, `ModeScope::Cad`, the
  Look, `Launch.cad`), `app/switch/` (`Document::Url`, `viewer_mode`'s
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
  Save/Discard prompt (a modal flow the viewer does not have). Since
  cad-views-export the file form's and `cad_file`'s new and open follow
  the same rule (`CadDocument::switch_blockers`), and every save is
  `POST /save/thumbnail` (see
  [Shared camera and CAD views](#shared-camera-and-cad-views-2026-10-01)).
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
  name field has focus (then `CadInputFocus`; since one-text-entry while
  any kit text field has the keyboard, `ui_kit::text::Typing`, see
  [One text entry](#one-text-entry-2026-10-01)), and a key whose button is disabled shows the
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
  `switch::handle` → `prepare`'s Cad arm (app/switch/prepare.rs:191) →
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
  /save` (RoboCAD writes its file) → as Patch (since cad-views-export:
  `CadAction::CadSave` → `files::save` → `CadClient::save_with_thumbnail`
  → `POST /save/thumbnail`, so the desktop's thumbnail is kept; the line
  numbers here are the cad-mode record); the poll sees `dirty`
  false and the header says Saved. Leaving: `leaving_blockers`
  (app/switch/prepare.rs:26; CAD clause :52) → `CadDocument::switch_blockers`; `leave_cad`
  (app/switch/leave.rs:71) removes the document (the slot's child is stopped,
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
- **Planar v2 files in Robot mode** (`robot/planar/`): a v2 file
  (`simrobot_version` < 3, the shared rule) is read as sim-phenomena's
  `CadModel` from the bytes the source worker read (`robot/loader.rs`
  `load_file_bytes`; `robot::source::FileModel::{Physical, Planar}`) and run
  by the "robot-run (planar v2)" `RunThread`, which builds through
  `sim_phenomena::scenarios::cad_robot::build_planar` (the one planar build;
  `AnyRobot::load` and `run_file` call it too) and paces with the CAD
  scene's rule. The header shows `HEADER_LABEL`; `robot_state.format`
  carries `{version, name, fidelity}` (`FIDELITY`, stated from
  `CadRobot::build`), and v3+ files report `physical v<N>`. Run, pause,
  step, reset, speed, joint select and target, tip contacts, outlines and
  COM dots, the watch and reload; everything without a v2 meaning is
  refused by name (`robot::planar::UNAVAILABLE`, live motor sync included).
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
  deliberately dropped (left click belongs to the panels). Since
  cad-views-export every orbit mode uses the shared camera
  (`crate::camera`), and Phenomena writes its feel as data
  (`phenomena/scene.rs`).
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
- `robot_planar.rs` (826 lines with tests) and `robot/actions.rs` (898) were
  over the 800-line smell; `robot.rs` grew to 2,514. Split 2026-10-01 by the
  split-large-files epic (see [Split large files](#split-large-files-2026-10-01)).
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
- `lesson/handler.rs` `open_url` left a zombie per opened link.

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
  `robot::planar::tests::*`, `robot::source` tests, `robot::actions` tests,
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
  (`robot/source.rs:138`) → `check` (:85, a Compute job) →
  `load_file_bytes` (`robot/loader.rs:61`; v2 → `robot::planar::load_bytes`,
  `robot/planar/mod.rs:104`) → `watch` (`robot/scene.rs:8`) / `receive` (:23) →
  `FileModel::Planar` (:81) → `install_planar` (:219) → `PlanarView::new`
  (:283) → `PlanarRun::spawn` (`robot/planar/thread.rs:242`) → `Worker::run`
  (:50) → `build` (:76) → `cad_robot::build_planar`
  (`sim-phenomena/src/scenarios/cad_robot.rs:506`), paused at t = 0.
- **v2 run.** Run button, Space (`robot/actions/keys.rs` `planar_keys`) or REST
  `robot_run` → `robot/actions/mod.rs:497` `apply` → `check_planar` (:109) →
  `dispatch_planar` (:191) → `PlanarRun::act` (`robot/planar/thread.rs:318`) →
  `Worker::command` (:151, Start) → `tick` (:127: 0.05 s × speed, 0.02 s
  grid, one step per tick) → `publish` (:196) → SimSync `planar_sync`
  (`robot/scene.rs:319`) → `PlanarRun::poll` (`robot/planar/thread.rs:269`, current
  generation only) → Present `robot/overlay_view.rs:202` `draw` →
  `robot::planar::draw` (`robot/planar/view.rs:20`).

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
Written and reviewed by reading, then built and tested in its
verification pass: verified at c0ed9b29 (sim-spatial lib 209 passed, 1
ignored; bins 4; sim-runtime `cad_client` and `units` 59; api pytests 14;
sim-web wasm check clean). Paths are `crates/sim-spatial/src/cad/` unless
they name another crate.

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
- **Push/pull commit.** D (`transform/input.rs` `keys`, `transform/mod.rs` `keys` before cad-modify's split) → `CadTool
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

## CAD modify (2026-10-01)

Batch cad-modify (default order item 7, §9 phase 1, the second half of the
planned cad-tools epic; see §9 "Later CAD epics" 2) brought RoboCAD's
operation catalogue into CAD mode: primitives, fillets, chamfer, shell,
thicken, draft, mirror, array, instance, make unique, booleans, region,
join, dissolve, cut, split, imprint, project, silhouette, the advanced face
edits, set pivot, copy and paste with placement, the read-only analyses,
the REST-only Ops methods, and the command surfaces RoboCAD offers them
through (menus, tools toolbar, right-click menu, Space and Q radials,
command palette, parameter dialogs) with RoboCAD's keys. RoboCAD's command
layer still does every edit: a run is one edit job sending the Ops calls
RoboCAD's own handler makes, so undo and provenance stay RoboCAD's. The
ledger rows are in [docs/cad-parity.md](../cad-parity.md) (116 rows: 77
done by reading, 39 deliberately different, each with its reason; none
open; after the epic the ledger has 245 rows done by reading, 458 later and
70 deliberately different), the side-by-side steps in
[docs/cad-checklist.md](../cad-checklist.md) (CAD-35 to CAD-76). Verified
at e0996878 (sim-spatial lib 252 passed, 1 ignored; bins 4; `cad_client` 33; `units` 29; api pytests 48; sim-web wasm check clean). Paths are
`crates/sim-spatial/src/cad/` unless they name another crate.

### Shape

- **The catalogue as data** (`ops/`): `catalogue.rs` `CATALOGUE`, 54
  `OpEntry`s (43 RoboCAD commands in its registry order, then 11 REST-only
  Ops methods as `ops.<name>`), each with RoboCAD's id, label, category,
  keys, what must be selected (`Needs`), typed parameters (`Param`, with
  RoboCAD's prompts, defaults and dialog ranges as `ui_kit::form::FieldKind`),
  how it starts (`Flow`: `Immediate`, `Form`, `PickThenForm(mode)`,
  `Place(primitive)`, `AtCursorSnap`), its route, its argument list
  (`Shape`, `Arg`, kwargs), whether RoboCAD's handler calls once or per
  node (`Fan`), RoboCAD's refusal and hint, whether it clears the
  selection, and the RoboCAD source it was read from ("(ours)" where
  RoboCAD says nothing). `kinds.rs` holds the shared field kinds, needs,
  hints and `BASE`. Nothing is code per operation: `resolve.rs` `resolve`
  is keyed by `Needs`, `args.rs` `build` by `Shape` (`plain`, `array`,
  `place`, copy, paste and the three reads), with `history` naming each
  route's RoboCAD history label.
- **One action family** (`actions.rs` `CadAction`): `CadInvoke { id }`
  (what a menu entry, toolbar button, palette row, radial entry, key,
  `system_ui` `cad:op:<id>` or REST `cad_invoke` does), `CadRun { id,
  params, items, revision }` (REST `cad_run`, the form's OK, a finished
  placement), `CadFormSet`, `CadFormSubmit`, `CadFormCancel` and
  `CadSurface { surface }`. `actions::handle` passes the first five to
  `ops::handle` and the last to `surfaces::handle`; their REST specs are in
  `specs.rs` (split from `actions.rs`), generated from the catalogue where
  they list it.
- **One apply path** (`ops/mod.rs`): `invoke` starts an entry by its flow
  (an id not in the catalogue goes to `surfaces::invoke_command`, i.e.
  `registry::invoke`); `run` → `prepare` (`CadDocument::commit_refusal`,
  `resolve::resolve`, `values`, `args::build`) → `start`: the calls in
  order inside one `actions::edit` job (`sync::start_edit`, a Dedicated
  job), a paste through the same edit path, or a read through
  `analysis_overlay::start`; `started` notes the selection on the edit
  where RoboCAD clears it (`sync::finish_edit` clears it once the edit
  succeeds and only if unchanged, as RoboCAD clears after its Ops call
  returns) and closes a form-flow form. REST `items` naming faces or edges
  need the `revision` their indices were read at; a parameter whose `when`
  does not hold is refused by name. `submit`, `open_form`, `form_set`,
  `form_cancel` own the form state (`OpsState::form`, `FormState`);
  `state_json` is `cad_state.ops`.
- **Interactions** (`ops/interact.rs`): primitive placement (RoboCAD's
  `PrimitiveTool`: snap the press, drag the base, then the height on the
  plane facing the camera, Ctrl for 10 mm steps, Tab for exact sizes;
  `finish_params` writes one `CadRun` with the revision at the press; the
  preview is overlay lines only); pick-then-form clicks are `pick.rs`'s
  (`pick::pointer` toggles an item of the tool's kind from
  `candidates_at`, i.e. `CadMeshes::face_at` and the topology at the shown
  revision; no box select while the tool is active); the cursor snap
  (`OpsState::cursor_snap`: `snap::snap` only, RoboCAD's `viewport.snap`
  rule, every 33 ms at most, candidates cached by epochs; it carries the
  shown revision it was snapped at and is cleared when that revision moves
  on, the pointer leaves the window or the search finds nothing, so a stale
  snap is refused by name, never sent) for "Set pivot at cursor snap". A placement's base and Tab anchor lie on z = 0 (on the
  active plane since cad-sketch; a sphere keeps its centre); the anchor is the form's last field, so Tab
  reaches width or diameter first as in RoboCAD. `topology::wanted` loads every
  drawn body's topology while an op is active.
- **Surfaces** (`surfaces/`): `registry.rs` is RoboCAD's whole command
  table (183 commands, `COMMANDS`, keys and whether they are bound, and a
  `Native` mapping: `Op`, `Action`, `Surface`, `NumericEntry`,
  `Later(epic)`, `Different(reason)`; the ledger leaves no command
  GUI-only: Blender link and web share belonged to cad-views-export, which
  made them `Different` with their reasons, `BLENDER_LINK` and `WEB_SHARE`), plus `TOOLBAR`,
  `CONTEXT`, `VIEW_RADIAL`, `SELECT_RADIAL`, `CATEGORIES` and `ready`, the
  readiness every surface, key and `system_ui` control shares. `mod.rs`
  `handle` opens and closes a surface (`OpsState::surface`) and answers its
  entries; `controls` lists `cad:op:<id>`, `cad:surface:<kind>`,
  `cad:menu:<category>` and the form's `cad:form:*`. `toolbar.rs` (the
  menu bar's tabs and the tools row under CAD mode's header, `COMMAND_BAR`
  high), `menus.rs`, `context_menu.rs`, `radial.rs`, `palette.rs` and
  `form.rs` draw them on the kit and turn clicks and keys into the same
  actions.
- **Keys** (`keys.rs`): RoboCAD's bindings read from the registry, not
  written per command (`parse`, exact modifiers; Control or Super for
  Ctrl); a matched command acts only when `registry::ready`, else the
  status line says why; the two-step "Shift+A, B/C/S" `Chord` with
  `gate`; ignored while a text field, a command surface or a modal form
  has the keyboard. The clash table is below.
- **Kit widgets** (`src/ui_kit/`): `pie.rs` (`Kit::pie`, `index_at`,
  `slot`), `palette.rs` (`rank`, `score`, `conflicts`, `Kit::palette`) and
  `form.rs` (`evaluate`, `TextDraft`, `Kit::form`), with no intent logic.
- **Inspector editors** (`inspector/editors.rs`, `inspector.rs` became
  `inspector/`): the pivot and transform editors, one `CadPatch` per Enter.
- **Overlays** (`analysis_overlay.rs`): copy, control points, curvature
  comb and continuity read on one Dedicated job at a time, results landed
  in JobResults (a copy into `OpsState::clipboard`), drawn on the tools'
  gizmo group in RoboCAD's colours and cleared when the shown revision
  changes.
- **Python additions** (`cad/robocad/api.py`, read-only except paste): `POST
  /clipboard/copy` (`Service.copy`), `POST /clipboard/paste`
  (`Service.paste`, one undo step "Paste"; on an error the half-pasted
  nodes are removed and the revision, dirty flag and results' stale flag
  restored), `GET /nodes/{id}/control_points?face=i`,
  `GET /nodes/{id}/curvature_comb[?scale&samples]`, `GET
  /nodes/{id}/continuity`; and two `ArgConverter._one` fixes found by
  reading: a non-plane node id passes through to `cut`'s `cutter`, and
  `set_control_points`' grid of points converts. Pytests
  `cad/tests/test_api_clipboard.py`, `test_api_control_points.py`,
  `test_api_analysis.py`, `test_api_cut_cutter.py`,
  `test_api_control_points_set.py`. sim-runtime
  `cad_client` gains `copy_nodes`, `paste` (`Pasted`), `control_points`
  (`ControlPoints`), `curvature_comb` (`CurvatureComb`) and `continuity`
  (`Continuity`, `EdgeContinuity`).
- **Splits** (each file under 700 lines): `document.rs` → `document/`
  (`mod.rs` the resource, `state.rs` queries and the shared refusals,
  `types.rs` value types); `transform/mod.rs` → `geometry.rs`, `input.rs`
  and `numeric_fields.rs` beside it; `inspector.rs` → `inspector/`
  (`mod.rs`, `editors.rs`); `actions.rs`' specs → `specs.rs`;
  `ops/catalogue.rs`' building blocks → `ops/kinds.rs`.

### Decisions

- **The catalogue is data in `cad/ops`, not a match per operation.** Every
  surface, the keys, REST and `system_ui` read one table, so a label, a key
  or a refusal is written once and the tests can cross-check it against
  RoboCAD's registry (`surfaces::tests`). *Rejected:* one `CadAction`
  variant per operation (116 rows of hand-written arms and specs); calling
  RoboCAD's `POST /commands/{id}` (GUI-only, opens Qt dialogs). *Revisit
  if* an operation needs logic no `Shape` or `Needs` expresses; add a shape,
  not a per-id branch.
- **`CadOp` stays as the raw REST escape hatch.** `cad_op` still calls any
  `POST /ops/{name}` with raw args; the catalogue is the checked path (its
  selection, revision and parameter refusals). *Rejected:* removing it
  (scripts and agents use it for methods no epic surfaces yet). *Revisit
  if* every Ops method has a catalogue entry and nothing calls `cad_op`.
- **Per-node calls run inside one edit job.** Where RoboCAD's handler loops
  over the selected nodes, the viewer sends the same calls in the same
  order on one job, each its own RoboCAD undo step as in RoboCAD; the first
  error stops the rest and the message says how many ran. *Rejected:* one
  job per call (the one-edit-in-flight rule would refuse the second);
  folding them into one `Composite` (RoboCAD's undo would differ, and it
  needs a new route). *Revisit if* RoboCAD's handlers become one Ops call.
- **Two revision guards.** A form-flow run is refused when RoboCAD's
  revision changed since its form opened (`FormState::began`); a pick or
  place tool's form stays open across runs, so its picks carry the guard
  instead (`resolve` refuses a selection first seen at an older revision,
  `selection_revision`; a placement sends the revision at its press).
  *Rejected:* one guard at the form's opening for every flow (a fillet
  tool left open across an undo would be refused forever, or would send
  stale indices). *Revisit if* forms become non-persistent.
- **No active plane until cad-sketch.** *Retired by cad-sketch
  (2026-10-01; see [CAD sketch](#cad-sketch-2026-10-01)).* Mirror (YZ),
  cut, split, silhouette, draft's neutral plane and the radial array's axis
  (XY) took a plane choice parameter with the default RoboCAD uses when no
  plane is active, and primitives were placed on XY through the origin.
  Now the parameter is "active" | xy | xz | yz with `Arg::Plane(name,
  fallback)`: "active" sends the native active plane (`CadActivePlane`, by
  name or plane node id), else that handler's RoboCAD fallback (YZ for
  mirror, XY for the rest); the radial array reads the plane's frame; the
  primitives are placed on the active plane (a box off XY is sent as
  `Ops.box_three_point`).
- **Both box tools send `Ops.box`.** RoboCAD's `PrimitiveTool` extrudes a
  sketch rectangle (history label "Extrude", node "Box"); the viewer sends
  one `POST /ops/box` (label "Box") with the corner the tool computes. The
  centre box keeps the tool's rule (centred in the plane, base on it), not
  `Ops.box_center` (centred in height too), which stays as the REST-only
  `ops.box_center`. *Rejected:* building a sketch body client-side (no
  route takes one). *Revisit if* the undo label must match exactly.
- **RoboCAD's command table is static data** (`surfaces/registry.rs`):
  menus, the palette, the toolbar, the context menu and the keys list all
  183 commands: later epics' disabled, naming the epic, and the few the
  ledger leaves unported disabled with its reason (no command maps to
  RoboCAD's `POST /commands/{id}`). *Rejected:*
  reading `GET /commands` (empty headless, the common case); listing only
  what runs (a user could not see where a RoboCAD command went).
  *Revisit if* RoboCAD's registry changes (the tests pin the count and
  the catalogue's agreement).
- **Copy keeps the clip in the viewer.** `POST /clipboard/copy` answers the
  clip and `OpsState::clipboard` keeps it with its revision; paste sends it
  back. *Rejected:* the OS clipboard (another surface to own, and RoboCAD's
  headless service could not read it anyway). *Revisit if* pasting between
  RoboCAD windows and the viewer is wanted.
- **The `ArgConverter` fixes.** `cut(node, cutter: str | Body | Plane)`
  had every `plane`-annotated argument parsed as a plane name, so a cutter
  node id failed headless (the GUI calls `Ops.cut` directly). A string that
  names a node with no plane now passes through (only `cut` mixes `str`
  and `Plane`). `set_control_points(…, points: list[list[Vec3]])` had its
  grid sent through the `Vec3` branch, which raised; a grid named `points`
  now converts first (only `set_control_points` has a `points`
  parameter). *Rejected:* new routes for the same Ops methods. *Revisit
  if* another Ops signature mixes ids and planes or takes nested points.
- **The selection is cleared when the edit succeeds.** RoboCAD's handlers
  clear it after the Ops call returns, so a failed fillet keeps the picks
  for a retry; `sync::finish_edit` clears it only on success and only if
  it is still the selection the run used. *Rejected:* clearing when the
  edit starts (the first version; it lost the picks on a refused radius).
- **`cad_state.ops`** reports the open form (each field's text and
  evaluation), the active op, the placement, the open surface, the cursor
  snap, the clipboard and the catalogue, so REST and an agent see what the
  window shows. It is set outside the `json!` macro (recursion limit).
- **The kit widgets carry no intent logic.** `ui_kit::{pie, palette, form}`
  take entries and one action component per clickable part; ranking,
  evaluation and the pie's hit test are pure functions tested in
  `ui_kit::tests`. *Rejected:* CAD-only widgets (other modes need a
  palette and forms). *Revisit if* a second caller needs a different
  interaction model.

### Key clashes

From `keys.rs`' module doc (grep of `KeyCode::` over
`crates/sim-spatial/src`, 2026-10-01; `app/`, `ui_kit/`, the switcher and
REST read no keys, so no key is read in every mode):

| Key | Commands | Resolution |
|---|---|---|
| Ctrl+Shift+M | `edit.select_same_material` (keymap), `robot.add_motor` (inline) | RoboCAD binds only the keymap's: Same Material runs; the palette shows RoboCAD's own conflict warning |
| Ctrl+Space | `command_palette` | macOS takes Command+Space (Spotlight); Control+Space or Shift+F opens it |
| Ctrl+H | `tool.fastener` (cad-print: starts the fastener tool, its form beside the view, then face clicks) | macOS's app menu takes Command+H (hide); Control+H starts the tool |
| Ctrl+W | `print.wall_check` (cad-print: the "Flag walls thinner than (mm):" form) | winit's default macOS menu has no Close item, so Command+W reaches the check; no other CAD reader of W (the shared camera's fly keys are not used in CAD mode) |
| V, Ctrl+V, Ctrl+Shift+V | select vertices, paste with placement, validate for printing (cad-print) | exact modifiers |
| Ctrl+M | `tool.mirror` | a macOS app menu binding Command+M (minimise) would take it; winit's default menu has none; Control+M always works |
| Shift+A, B / C / S | `tool.box` / `tool.cylinder` / `tool.sphere` | the second key is the chord's (see above), not B (select bodies), C (sketch circle) or S (scale) |
| S, G, R, D, Shift+D, M, Escape | tools (transform) | read by transform's keys only; Shift+S (sketch slot), Shift+R (revolve), Shift+J etc. differ by Shift, which transform's S/G/R/M refuse |
| Ctrl+S, Ctrl+Shift+S, Ctrl+Shift+D | save, save as, export drawing | transform's S and D act only without Ctrl |
| Ctrl+A, Ctrl+Shift+A, Shift+A | select all, array, chord start | exact modifiers keep them apart |
| Ctrl+Z, Z | undo, next display mode (cad-views-export; native since) | exact modifiers |
| B, Shift+B, Ctrl+Shift+B | select bodies, select faces, build plate (cad-views-export; native since) | exact modifiers |
| F, Shift+F, Ctrl+F, Ctrl+Shift+F | focus (cad-views-export; native since), palette, fillet, chamfer | exact modifiers |
| H, Alt+H, Ctrl+H, Ctrl+Shift+H | hide, show all (cad-views-export; native since), fastener, shell | exact modifiers |
| C, Shift+C, Ctrl+C, Ctrl+Shift+C, Shift+A then C | sketch circle, sketch spline, copy, clearance offset (cad-print: needs selected faces, then its form), cylinder | exact modifiers; C after Shift+A is the chord's (see above), not the circle's |
| P, Shift+P, Ctrl+P | select points, sketch polygon (cad-sketch), plane from face (cad-sketch) | exact modifiers |
| Delete, Backspace | `edit.delete` | ignored while a text field (name, numeric bar, palette, form) has the keyboard |
| Space | `view.radial` | typed as a space while a text field has the keyboard |
| Tab | `numeric.entry` | an open form with a text field takes it (`surfaces::form::input`); during a placement drag `ops::interact` also copies the base point into the form's anchor field; else the numeric bar's |
| digits, J, Q, X, T, L, C, N, /, Home | views (cad-views-export; native since), join, selection radial, extrude, sketch text/line/circle (cad-sketch), annotate (cad-organize), isolate, fit | no other reader in CAD mode |

Commands of later epics keep their keys, so a press says which epic owns
them (status line), as their menu entries do.

(This table is the cad-modify record. Since cad-sketch the keys marked
"(cad-sketch)" run natively, A binds the three-point arc, and the table in
`cad/keys.rs`'s module doc is the current one: S/Shift+S, R/Shift+R, L,
Shift+L, C, Shift+C, T, X, Ctrl+P and Enter rows added. Since
cad-views-export the keys marked "(cad-views-export; native since)" run
natively, `/` isolates, and the numpad and arrow keys are the shared
camera's; see [Shared camera and CAD views](#shared-camera-and-cad-views-2026-10-01),
Key clashes.)

### Review findings (five pair-reviewers by area, then fixes)

No reviewer found a compile error. Fixed:

- **Kit:** `Unit::Factor` was never constructed (a dead-code warning;
  removed); `TextDraft::key` reported `Ignored` when only the selection
  changed, so typing the same character over a selected field left it
  selected; the checkbox chip lit only for "true"; palette rows wrapped
  inside their fixed height (now no-wrap); a hovered disabled pie entry
  looked lit; two clippy lints.
- **Catalogue and apply path:** the selection was cleared when an edit
  started, losing a failed fillet's picks (now on success, in
  `sync::finish_edit`); REST `items` with face or edge indices were sent
  without a revision check (now `revision` is required); `when` gates
  compared raw text, so "Radial" dropped every Array row, and a parameter
  of the other kind was silently ignored (now the canonical option, and
  refused by name); a read's revision guard could never fire; a create
  with nothing selected named the selected node; a typed box or cylinder
  kept its anchor's z where RoboCAD projects it onto the plane; the comb
  and continuity read the last selected node even without a body; arrays
  of strings kept their JSON quotes; the stale-revision refusal spoke only
  of drags; `ops.set_control_points` could never succeed (an api.py
  converter bug, fixed with a pytest).
- **Interactions and pickers:** `cad_cancel` cleared a pick tool's picks
  instead of ending the tool (now it ends it as the form's Cancel does);
  the press that closes a popup also started a placement; Tab wrote a
  snapped anchor off the plane and reached the anchor field before width;
  the cursor snap ran a mesh ray cast over every body every 33 ms (now
  `snap::snap` only, RoboCAD's rule); `pick::pointer` was unordered
  against the editors' and the numeric bar's focus; push/pull read a face
  with `face_of` before its revision check (older code; now `face_at`).
- **Python and the client:** a failed paste left the document dirty with
  a raised revision and no undo step (now restored), and its test never
  reached the cleanup (now a valid item then a bad one).
- **Surfaces and keys:** `RadialSlot`'s index was never read (a warning);
  long menus (Modify 32 rows, View 31) were clipped with no scrolling
  (now a wheel-scrolled area); Escape in the name field or an inspector
  editor also cancelled the open op or popup; catalogue entries showed
  enabled while an edit was in flight; three commands the ledger marks
  "not ported" ran as GUI-only; open popups did not refresh on a
  connection or edit change; the Tab row of the clash table.

Rejected, with reasons: `cad_state.ops` clones the catalogue's JSON on
each 100 ms snapshot (about 50 small objects; kept, it is what lets a REST
client discover the operations); an empty paste still pushes an empty
"Paste" step (RoboCAD's own paste does, app.py:1478-1483: parity); a
timed-out copy carries the client's "may still apply" wording (copy is a
POST that changes nothing; harmless); the palette shows keys from a
desktop RoboCAD's `/commands`, which may include the user's keymap while
the viewer binds RoboCAD's defaults (the palette shows what RoboCAD
binds); a count typed as `0.1*30` is refused as not whole (RoboCAD's spin
box takes integers only); the context menu's extra "Make unique (bake
instance)" (it is the outliner context row of this epic, recorded in the
ledger; the native outliner has no context menu yet); `ArgConverter`'s
unreachable `Sequence[Vec3]` branch (no Ops method takes a flat point
list; out of scope).

### Disk

Removed 2026-10-01 before the epic (regenerable build output only; no
`cargo clean`; `runs/`, `.claude-pair` and captures untouched):

- 275 superseded incremental session directories under
  `target/debug/incremental/*/` (every crate directory holding two or more
  finalized `s-*` sessions kept only its newest): about 3 GiB.
- 7,237 superseded artifacts in `target/debug/deps/` (an older hash of the
  same crate and file kind, more than two days older than the newest of
  that kind, which was kept): 14.22 GiB.

Free space was 21.43 GiB before and 38 GiB after (`df -g`).

### Verification checklist

- `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- `cargo test -p sim-spatial --lib --bins`, in particular `cad::ops::tests::*`
  (`catalogue_data_is_well_formed`, `every_entry_builds_calls_to_its_route`,
  `fillet_and_chamfer_send_one_call_per_node_with_edge_refs`,
  `shell_draft_and_faces_use_face_refs`,
  `booleans_mirror_and_instance_match_robocad_handlers`,
  `array_dialog_builds_rect_or_radial`, `primitives_follow_the_primitive_tool`,
  `resolve_refuses_with_robocad_messages`,
  `a_selection_seen_at_an_older_revision_is_refused`,
  `runs_are_refused_in_flight_stale_or_with_unknown_parameters`,
  `explicit_face_and_edge_items_need_their_revision`,
  `when_gates_read_the_canonical_option_and_refuse_what_does_not_apply`,
  `form_set_joins_string_arrays_without_quotes`,
  `an_operation_that_needs_nothing_names_no_selected_node`,
  `primitive_anchors_are_projected_onto_the_plane_and_come_last`,
  `comb_and_continuity_read_the_last_node_with_a_body`),
  `cad::surfaces::tests::*` (`the_table_is_robocads_registry`,
  `every_surface_names_registry_commands`,
  `every_catalogue_command_agrees_with_the_registry`,
  `menus_put_general_window_and_tools_in_help`,
  `readiness_refuses_with_the_entrys_refusal`, `keys_parse`,
  `the_palette_shows_robocads_key_conflict`,
  `the_context_menu_offers_make_unique_for_instances`,
  `every_surfaces_control_round_trips_through_rest`), `ui_kit::tests::*`
  (`pie_picks_the_entry_under_the_pointer`, `palette_ranks_as_robocad`,
  `palette_warns_of_key_conflicts`, `form_fields_evaluate`,
  `text_drafts_edit_as_the_numeric_bar`, `modify_widgets_spawn`, and the
  colour rule), `cad::mesh::tests::face_at_reads_only_a_tessellation_at_the_shown_revision`,
  `cad::inspector::editors::tests` (`a_typed_component_sends_the_whole_transform_and_the_pivot_alone`,
  `component_occurrences_are_refused_as_robocad_refuses_them`),
  `cad::ops::interact::tests` (`finishing_sends_the_sizes_robocad_finishes_with`,
  `the_preview_outlines_the_base_and_the_top`,
  `the_height_follows_the_cursor_along_the_normal`),
  `cad::analysis_overlay::tests` (`counts_print_as_robocads_status`,
  `overlays_use_robocads_shapes_and_colours`,
  `state_json_names_the_overlay_and_the_read`), `app::tests::*` (the
  registry cross-check
  `every_capability_parses_into_its_action_and_every_parsed_command_is_registered`),
  `cad::tests::every_cad_control_fits_a_pattern_and_round_trips_through_rest`,
  and the earlier epics' CAD, jobs and transform tests.
- `cargo test -p sim-runtime --lib -- cad_client units` (new:
  `clipboard_copy_and_paste`, `analysis_reads_as_api_writes_them`,
  `analysis_errors_name_the_route`).
- `cd cad && .venv/bin/pytest -q tests/test_api.py
  tests/test_api_edge_samples.py tests/test_api_clipboard.py
  tests/test_api_control_points.py tests/test_api_analysis.py
  tests/test_api_cut_cutter.py tests/test_api_control_points_set.py`
  (run 2026-10-01, the only checks run in this epic: the four new files
  27 passed before the review fixes; after them
  `test_api_clipboard.py` 5 passed and `test_api_control_points_set.py`
  2 passed).
- `cargo check -p sim-web --target wasm32-unknown-unknown`.
- Then the user's [docs/cad-checklist.md](../cad-checklist.md) CAD-35 to
  CAD-76.

### Reading traces

Function names, not line numbers (fixes may follow the review).

- **Edge pick → fillet form → commit → undo.** Ctrl+F (`keys::keys`
  matches `tool.fillet`'s "Ctrl+F" from `surfaces::registry`, and it is
  ready: `registry::readiness` accepts every pick-then-form op) →
  `CadInvoke { tool.fillet }` → `actions::handle` → `ops::handle` →
  `ops::invoke`: `Flow::PickThenForm(Edge)` → `end_tool` (Select), the
  selection mode becomes Edge with the selection kept
  (`selection::publish`, `PUT /selection`), `OpsState::active =
  tool.fillet`, `open_form` (radius "1.0", unfocused, `began` = the shown
  revision), the hint on the status line. A click on an edge:
  `pick::pointer` (the tool's kind, also while its form has the keyboard)
  → `candidates_at` (faces from `surface_item` via `CadMeshes::face_at`
  at the shown revision; edges from the topology search at the shown
  revision) → the first candidate of kind edge → `CadSelect { items,
  toggle: true }` → `actions::handle` → `selection::handle` →
  `selection::select` → `publish`. Tab focuses the form's radius field and
  typing edits its draft (`surfaces/form.rs` `input`, `TextDraft` on
  `FormState::texts`, display state as the numeric bar's; REST sets it with
  `CadFormSet`), then Enter or OK → `CadFormSubmit` → `ops::submit` (the drafts as params; no
  form revision for a pick tool) → `run` → `prepare`:
  `commit_refusal(None)` (an edit in flight, not connected, stale),
  `resolve::resolve` (the selection's revision must be the shown one; each
  edge must exist in the shown topology; `Needs::Edges`, else "Select one
  or more edges first"), `values` (radius evaluated by
  `ui_kit::form::evaluate`), `args::build` → `plain` with `Fan::PerNode`:
  one `OpCall` per node owning a selected edge → `start` →
  `actions::edit` → `sync::start_edit` (a Dedicated job, "RoboCAD edit:
  Fillet …") → `CadClient::op` → `POST /ops/fillet {"args": [node,
  [{"node", "edge"}, …], radius]}` → RoboCAD's `Ops.fillet` (`_edit`, undo
  step "Fillet") → the job lands: `finish_edit` → `refresh` (the poll
  refetches `/doc`, meshes and topology at the new revision); `started`
  noted the selection, which `finish_edit` clears now that the edit
  succeeded, and kept the form and the tool active. Undo:
  Ctrl+Z → `keys::keys` → `CadInvoke { edit.undo }` → `ops::invoke` (not
  in the catalogue) → `surfaces::invoke_command` → `registry::invoke`
  (`Native::Action(Do::Undo)`) → `actions::handle` `CadUndo` →
  `actions::edit` → `POST /undo` (RoboCAD undoes "Fillet").
- **Palette search → union.** Ctrl+Space or Shift+F (`keys::keys`:
  `command_palette` resolves to `Native::Surface(Opens::Palette)`) →
  `CadSurface { palette }` at the pointer → `actions::handle` →
  `surfaces::handle` (`OpsState::surface`, highlight 0) → `surfaces/palette.rs`
  `input` (it holds the keyboard) → typing "uni" edits the query
  (`TextDraft`) → `palette::ranked` → `ui_kit::palette::rank` ("Union"
  has "uni" at position 0 of its label: score 1, first) → Enter →
  `row_entry` (ready) → `CadInvoke { modify.union }` and `CadSurface {
  closed }` → `ops::invoke` (`Flow::Immediate`) → `run` → `prepare` →
  `resolve::resolve` (`Needs::TargetThenTools`; fewer than two nodes:
  RoboCAD's "Select the target body first, then the tools") →
  `args::build` → `plain`: `[target, [tools], "union"]`, label "Union …"
  → `start` → `actions::edit` → `POST /ops/boolean {"args": [target,
  [tools], "union"]}` → RoboCAD's `Ops.boolean` (`Composite` "Union",
  the tools removed) → `finish_edit` clears the selection
  (`clears_selection`, noted by `started`) once the edit succeeded and
  publishes it.

## Split large files (2026-10-01)

Batch split-large-files (structural; tasks T32.1–T32.4). Pure
restructuring of `crates/sim-spatial/src`: code moved verbatim along its
seams (only `use` lines, `mod` declarations, visibility qualifiers and path
prefixes changed), whole files moved with `git mv`, and no REST name,
capability text, action spec, key binding, system set, system ordering,
run condition or UI changed. Every plugin's `build()` (`RobotPlugin`,
`hardware::build`, `BuilderPlugin`, `LearnPlugin`, `SpatialViewerPlugin`,
`app::switch::build`) is textually unchanged: moved systems are imported
back into the module root under their old names. Written and reviewed by
reading (four parallel reviews: run thread, robot tree and hardware,
builder and lessons, lib/switch/guard/transport; no compile or ordering
defect found). Verified at 57f7d447 (2026-10-01 pass): `cargo check
--workspace --all-targets` and `cargo build -p sim-spatial --lib --tests
--bins` with no sim-spatial warnings; `cargo test -p sim-spatial --lib
--bins` 253 passed (252 plus `source_files_stay_small`), 1 ignored, bins 4;
`cargo test -p sim-runtime --lib hardware_client` 18 passed (the refusal
test included), `cad_client` 33; `cargo check -p sim-web --target
wasm32-unknown-unknown` clean. The build found one slip, fixed in 0170f176:
`HeldSlider` private under the now-`pub(super)` `sliders` system (E0446),
and an unused `LiveInput`/`live_run` re-export. Commits 33ebd373 (doc
truth), d39bd96b (robot moves), 19b4b86b (run thread), 1f6c5311 (robot
view, actions, planar), c021ce87 (hardware), b2b5397e (builder), 2a883b20
(lessons), eff1de77 (lib.rs, switch, guard), a62ece7b (transport), plus
two doc-only commits.

### Module map

Paths are `crates/sim-spatial/src/`; non-test lines after the split.

| Was | Now |
|---|---|
| `robot.rs` (2,532) | `robot/mod.rs` 201 (doc, imports, `RobotView`, `Status`, `Section`, mod list, re-exports, `RobotPlugin`); `loader.rs` 131 (`Loaded`, `load*`, `Opened`); `state.rs` 245 (`impl RobotView`: open, switch, `robot_state` JSON and its planar twin); `ui.rs` 235 (marker components, `Materials`, `setup`); `scene.rs` 499 (`watch`, `receive`, `install_planar`, `planar_sync`, `apply_frames`, orbit, viewport, highlight, scroll); `inspector.rs` 286 (`panels`, speed and overlay panels); `controls.rs` 475 (jog, motion, recorded and gait panels); `sections.rs` 232 (link, joints, drives, source texts); `overlay_view.rs` 250 (stress paint, graph dock, overlay gizmos); `tests.rs` |
| `robot_run.rs` (2,647) | `robot/run/mod.rs` 30 (re-exports); `pacing.rs` 63; `jog.rs` 75; `protocol.rs` 155 (`Phase`, `Source`, `Status`, `Command`, `RunAction`); `frames.rs` 222 (`Frame`, overlays, `map_poses`, frame JSON); `replay.rs` 187; `sim.rs` 184 (`Sim`); `worker.rs` 387 (the run-thread loop); `controller.rs` 521 and `preset_ops.rs` 320 (`RunController`, two impl blocks); `tests.rs` |
| `robot_{gait,graphs,motion,playback,preset,recording,source,stress}.rs` | `robot/<name>.rs`, unchanged |
| `robot_planar.rs` (862) | `robot/planar/mod.rs` 272 (refusals, `PlanarView`); `thread.rs` 383 (the planar run thread); `view.rs` 129 (drawing, inspector text); `tests.rs` |
| `robot/actions.rs` (911) | `robot/actions/mod.rs` 616 (`RobotAction`, check, dispatch, controls, apply, publish); `keys.rs` 140 (input systems); `commands.rs` 127 (the spec table and wire conversion, verbatim); `tests.rs` |
| `robot/hardware/sync.rs` (905) | `sync.rs` 679; `sync/page.rs` 116 (the page's pure rules); `sync/apply.rs` 65; `sync/thread.rs` 72 (outbox, worker) |
| `robot/hardware/session.rs` (821) | `session.rs` 698; `session/periodic.rs` 132 (a second `impl Session`: poll, deadlines, shutdown) |
| `robot/hardware/mirror.rs` (766) | `mirror.rs` 654; `mirror/apply.rs` 53; `mirror/thread.rs` 71 |
| `robot/hardware/actions.rs` (742), `view.rs` (711) | `actions.rs` 626 + `actions/input.rs` 126 (input systems); `view.rs` 640 + `view/status.rs` 82 (`hardware_status` JSON) |
| `builder.rs` (2,453) | `builder.rs` 590 (types, `Builder::open`, accessors, `state_json`, `BuilderPlugin`); `builder/editing.rs` 395; `drafts.rs` 163; `live_run.rs` 531; `studies.rs` 210; `rebuild.rs` 249; `background.rs` 69; `test_support.rs` (`#[cfg(test)]` hooks); `replay_tests.rs` |
| `builder/ui.rs` (1,395) | `ui.rs` 377; `ui/tabs.rs` 378; `ui/inspector_panel.rs` 510; `ui/notes_tab.rs` 148 |
| `builder/discussion.rs` (897), `calibration.rs` (826), `placement.rs` (1,212) | `discussion.rs` 498 + `discussion/handlers.rs` 289 + `discussion/tests.rs`; `calibration.rs` 552 + `calibration/panel.rs` 170 + `calibration/tests.rs`; `placement.rs` 649 + `placement/tests.rs` |
| `lesson/mod.rs` (2,371) | `lesson/mod.rs` 657 (types, `Learn::new`, catalogue, `LearnPlugin`); `opening.rs` 258; `editing.rs` 125; `handler.rs` 397; `watch.rs` 300; `scene_view.rs` 225; `controls.rs` 156; `tests.rs` |
| `lesson/ui.rs` (1,158), `narrate.rs` (880), `practice.rs` (790) | `lesson/ui/{mod 426, outline 154, scene_card 345, cards 108, margin 145}.rs`; `lesson/narrate/{mod 528, bar 195, overlay 169}.rs`; `lesson/practice/{mod 362, cards 338, sketch 104}.rs` |
| `lib.rs` (1,297) | `lib.rs` 64 (doc, mod list, re-exports, the imports glob users share, the `fixture` test helper); `inspect_view/mod.rs` 397 (`SpatialScene`, `SpatialViewerPlugin`, components); `scene.rs` 320; `camera.rs` 125; `ui.rs` 284; `tests.rs` |
| `app/switch.rs` (959) | `app/switch/mod.rs` 536 (`handle`, `Switcher`, `build`); `prepare.rs` 220 (leaving blockers, `prepare`); `arrival.rs` 139 (`enter`, `finish_load`, `arrive`); `leave.rs` 109 (`leave_*` per mode) |

### Decisions

- **Cap 750 non-test lines, guarded.** `app::tests::source_files_stay_small`
  walks `src/` and fails naming each file over the cap and its count. A
  file's non-test lines are those before its first column-0 `#[cfg(test)]`
  immediately followed by a column-0 `mod … {` line (an inline test
  module); otherwise every line counts, so a `#[cfg(test)]` helper or
  re-export mid-file does not end the count. Files named `tests.rs` or
  `*_tests.rs` and everything under a `tests/` directory are skipped. The
  aim when splitting was 700; 750 leaves room for cohesive files such as
  `cad/ops/mod.rs` (708) and `cad/panel.rs` (700) without churn.
  *Rejected:* a 700 cap (would force artificial splits of just-reviewed CAD
  files); counting to the first `#[cfg(test)]` of any kind
  (`cad/sync/mod.rs` has a test-only re-export at line 28). *Revisit if*
  files settle at 740–750 and keep creeping.
- **The allowlist is empty.** Every file the batch named is under 700; an
  `ALLOWED` entry needs a reason, and an entry whose file is gone or back
  under the cap fails the test, so the list cannot go stale.
- **Re-exports keep every public path.** Each split root re-exports what
  callers use with the item's own visibility (`pub use` for `pub`,
  `pub(crate) use`, or a private `use` that children reach through
  `use super::*`). `crate::robot::run::X`, `crate::robot::planar::X`,
  `crate::SpatialScene`, `sim_spatial::load_inspect`,
  `crate::builder::compiled_scene`, `lesson::open_lessons`,
  `app::switch::{handle, arrive, finish_load}` and the rest resolve as
  before. Only the robot module names changed (`crate::robot_run::X` →
  `crate::robot::run::X`, `sim_spatial::robot_preset` →
  `sim_spatial::robot::preset`), with every use site updated; no temporary
  `robot_*` aliases remain in `lib.rs`.
- **Visibility is the narrowest that keeps the old reach.** A private item
  moved into a child became `pub(super)` (or `pub(in crate::<mode>)` one
  level deeper), which is exactly the reach it had in the parent; nothing
  was widened past its callers. Struct definitions stayed with the code
  that touches their private fields where possible (`RobotView`,
  `Builder`, `Learn` stay in their roots); `SpatialScene`'s fields became
  `pub(crate)` because they were private at the crate root, which was
  already crate-wide.
- **Robot mode is one tree** (`robot/`, as CAD mode is `cad/`): the ten
  `robot_*.rs` files moved under it first as pure `git mv` (d39bd96b), so
  the parallel splits could not collide on use sites.
- **Behaviour bugs noticed while splitting are left for a feature batch**
  (this batch changes no behaviour): `gait_panel` and `motion_panel`
  (`robot/controls.rs`) never clear a block once shown, so a REST
  `robot_preset` reopen into a view without gait preview or motion can
  leave stale buttons (`recorded_panel` despawns its children); a failed
  preset load in `receive` (`robot/scene.rs`) despawns the old meshes before
  it checks the result. Misplaced doc comments found the same way were
  fixed (comments only).

### The refusal test (hardware_client, T32.1)

`hardware_client::tests::a_refusal_before_the_body_surfaces_the_servers_error`
failed deterministically on macOS. Investigation (python3 reproduction on
127.0.0.1 with a 3000-byte body and a server that reads the head in
1024-byte pieces, answers 403 and closes with the body unread): an answer
written in **one** write always reached the client whole before the reset
(40 of 40 runs, whether the client read at once, after 0.2 s, or peeked
first), so `exchange`'s read-ahead and `salvage` already work. An answer
written **piecewise** (`write!` on a `TcpStream`, as the test server and
both real servers do) lost every piece after the first: Nagle holds them
until the first is acknowledged, and the reset sent by closing with unread
data discards the server's unsent bytes (the client received 72 of 131
bytes, the status line and part of the head, then ECONNRESET). No client
change can read bytes that were never sent.

*Decision:* the transport cannot recover them, so the client's error on
that path names it truthfully. `loopback_http::exchange`'s closed-connection
error now gives the status code when the status line arrived ("the server
closed the connection after sending only part of an HTTP 403 answer") and
appends `REFUSAL_LOST` ("a server that refuses a request without reading
its body resets the connection, which can discard its answer") before the
caller's hint; it never invents the server's reason. Requests, the
loopback-only refusal and every timeout are unchanged. The test keeps its
assertion off macOS; on macOS it accepts the server's error or `HTTP 403`
plus `REFUSAL_LOST`; a new one-write case asserts the server's error on
every platform. It is not ignored. *Rejected:* polling or peeking between
body writes (the bytes are lost on the server side, not the client's);
loosening the assertion everywhere. *Follow-up (server-side, out of this
batch's scope):* `serve_actuator_calibration` (`reply`) and
`serve_motor_bench` (`response`) in `crates/sim-runtime/examples/` should
write each answer in one write and, before closing, shut down the write
half and drain the unread body for a bounded time; then the macOS user sees
"Session token required" instead of the partial-answer error. *Revisit
when* those servers change.

### Verification checklist

- [ ] `cargo build -p sim-spatial --lib --tests --bins` with no warnings
  (watch for unused imports in split roots whose names only children use
  through `use super::*`, and for re-exports wider than their items).
- [ ] `cargo test -p sim-spatial --lib --bins`: 253 passed (the 252 of
  cad-modify plus `app::tests::source_files_stay_small`), 1 ignored; bins 4.
- [ ] `cargo test -p sim-runtime --lib hardware_client` (the refusal test
  and its one-write case pass on macOS).
- [ ] `cargo check -p sim-web --target wasm32-unknown-unknown`.

## CAD sketch (2026-10-01)

Batch cad-sketch (default order item 7, §9 phase 1; §9 "Later CAD epics"
3) brought RoboCAD's sketching into CAD mode: the active plane (XY, XZ, YZ
or a plane node) and 2D snapping, the four plane tools, the 13 sketch
tools, sketch offset, fillet corners and join, the REST-only sketch calls,
extrude and revolve with RoboCAD's Shift/Ctrl/Alt booleans, and sweep,
pipe, loft and fill. RoboCAD's command layer still does every edit: each
finished shape, plane or solid is one RoboCAD call on one edit job, so
undo and provenance stay RoboCAD's. The ledger rows are in
[docs/cad-parity.md](../cad-parity.md) (60 rows: 35 done by reading, 25
deliberately different, each with its reason; none open; 6 earlier rows
that waited for the active plane became done by reading; after the epic
the ledger has 286 rows done by reading, 398 later and 89 deliberately
different), the side-by-side steps in
[docs/cad-checklist.md](../cad-checklist.md) (CAD-77 to CAD-98). Written
and reviewed by reading in commits 34fa7901, f0c23f87 and 87287d70; verified
at cc7ac194 (sim-spatial lib 293 passed, 1 ignored; bins 4; `cad_client` 47;
`units` 29; api pytests 61; sim-web wasm check clean). Paths are
`crates/sim-spatial/src/cad/` unless they name another crate.

### Shape

- **Client types** (`sim-runtime/src/cad_client/sketch/`): `PlaneFrame`
  (origin, x axis, normal; `to_local`, `to_world`, `project`, `same`),
  `SketchGeometry` and `SketchCurve` read tolerantly (an unknown or
  malformed curve is counted, not fatal; `sample(n)` as RoboCAD's viewport
  samples, a slot's caps outward as the kernel builds them), `SketchCall`
  (`calls.rs`: the 31 kernel/sketch.py calls api.py accepts, `to_json`,
  `from_json` with refusals naming the call and the argument, `check`,
  `curves_after`), `check_calls` (curve indices as each call leaves the
  list), `plane_of` (a plane node's frame), and
  `CadClient::sketch`, `edit_sketch` (`POST /nodes/{id}/sketch`) and
  `create_sketch` (`POST /nodes {"kind": "sketch", "plane", "calls"}`).
  Tests in `sketch_tests.rs`.
- **The active plane** (`sketch/mod.rs` `CadActivePlane`: `Base(BasePlane)`
  or `Node {id, frame}`, `snap_2d`, the document generation; `frame`,
  `frame_or_xy`, `arg_or`, `label`). `sketch/plane.rs`: `sync` (a plane
  tool's new node becomes active, `ops.plane_created`; selecting exactly
  one plane node makes it active; a node's frame from the cache; a node
  gone from the tree is dropped by name), `view_act` (Active plane:
  XY/XZ/YZ, Toggle 2D snapping, RoboCAD's status lines), `begin`,
  `picks` and `run_for` (the plane tools' face picks and snapped points, one
  `CadRun` per complete set), `state_json` (`cad_state.plane`).
  `sketch/plane_draw.rs`: the translucent quads (`wanted`, `quads`,
  `outlines`).
- **The cache** (`sketch/cache.rs` `CadSketches`): sketch geometry and
  plane frames of the shown tree by (node, revision), one `Pool::Dedicated`
  job per node; `sketch`/`plane` answer only at the shown revision (the
  tools decide on current geometry), `sketch_last`/`plane_last` also
  answer the last read (the display does not blink).
- **The sketch tools as data** (`ops/catalogue/sketch.rs`, 13 `tool(…)`
  entries with `Flow::Sketch(shape)`; `sketch/specs.rs` `SPECS`, one
  `SketchSpec` per shape: points `needed`, `Finish::Points(n)` or
  `EnterOrDouble`, `chains`, `Readout`, `text_form`). `specs::from_points`
  is RoboCAD's `_build`, `from_values` its `commit`, `target` its
  `_ensure_sketch`, `calls` the Tab path; `note_polygon_sides` (in `ops::send_sketch`) and
  `polygon_edit_done` (in `sync::finish_edit`, on success only) are
  `Sketch.last_polygon_sides`. `sketch/interact.rs`: one pointer system
  for all 13 (`begin`, `pointer`, `finish_action`, `reset_after_finish`,
  `readout`; double-click `DOUBLE_CLICK` 400 ms and `DOUBLE_DISTANCE`
  5 px). `sketch/preview.rs` (`preview_curves`, `preview_lines`, `draw`)
  and `sketch/display.rs` (`shown`, `lines`, `draw`) are display only.
- **One sketch edit path** (`ops/mod.rs` `send_sketch`): every finished
  shape, Tab commit, sketch edit and REST `cad_sketch` ends in one
  `actions::edit` job calling `CadClient::edit_sketch` or `create_sketch`.
  `sketch/edits.rs`: `selected_sketch` (RoboCAD's `_selected_sketch`),
  `calls` (offset, `fillet_plan` over `fillet_corner`, join),
  `sketch_action` (`CadAction::CadSketch`: `commit_refusal`, the calls
  read and checked, `specs::target` without a node).
- **Extrude and revolve** (`sketch/extrude.rs`, entries in
  `ops/catalogue/solid.rs` with sweep, pipe, loft and fill): `begin`,
  `source` (RoboCAD's `activate` rule), `body_under_selection`,
  `boolean_for`, `pointer` (the drag through `transform::push_distance`),
  `calls` (`Shape::Extrude`), `draw` (outlines).
- **Planes in the catalogue** (`ops/catalogue/plane.rs`): the four plane
  tools (`Flow::PlanePick(mode)`, `activates_plane`) and the four viewer
  state entries (`Flow::View(ViewAct)`, `Shape::View`, `Built::View`).
- **Catalogue plumbing** (`ops/`): `Flow::{Sketch, Extrude, PlanePick,
  View}`, `Shape::{Sketch, SketchEdit, Extrude, View}`, `Arg::{FaceB,
  Keyed, Plane}`, `Env` (the active plane and the cache for builders),
  `OpEntry::activates_plane`, `Built::{Sketch, View}`; `ops/mod.rs` split
  into `form.rs` (the form's state and `form_cancel`) and `state.rs`;
  `catalogue.rs` split into `catalogue/{edit_create, solid, modify, plane,
  arrange, sketch, boolean, rest_only}.rs`, joined at compile time in
  RoboCAD's registry order (`catalogue/mod.rs` `join`), 84 entries. Plane
  parameters are "active" | xy | xz | yz (`kinds::PLANES`,
  `Arg::Plane(name, fallback)`); `args::place` puts primitives on the
  active plane.
- **Snapping** (`snap.rs`): `SnapKind::{Endpoint, Plane}`,
  `sketch_candidates`, `drawn_candidates`, `snap_plane`, `snap_on` (the
  best candidate projected onto the plane, the grid in plane coordinates,
  else the plane hit); `measure.rs` and the cursor snap honour 2D
  snapping. `pick.rs` leaves the left button to the plane, sketch and
  extrude interactions.
- **Surfaces and keys**: every cad-sketch command is `Native::Op` in
  `surfaces/registry.rs`; the toolbar's Rectangle, Circle, Slot and
  Extrude run and light (`toolbar::checkable`); the right-click menu's
  "Sketch" section (`registry::SKETCH_CONTEXT`); `keys.rs`' clash table
  covers the new keys; `surfaces/form.rs` keeps Enter for the spline and
  makes Enter in "Text to sketch:" start the clicks. `ui_kit/form.rs`:
  `FieldKind::Text`, `Unit::Plain`.
- **REST and state**: `CadAction::CadSketch` (`cad_sketch`, `specs.rs`),
  `Cx` carries the plane and the cache, `cad_state.plane`;
  `app/switch/prepare.rs` refuses leaving CAD mode while a shape is in
  progress (`cad::sketch_blocker`, `sketch::blocker`).
- **Python** (`cad/robocad/api.py` `Service.edit_sketch`): curve indices
  are mapped before two-number lists become points; pytest
  `cad/tests/test_api_sketch_calls.py`.

### Decisions

- **The active plane is a native resource, display state.** `CadActivePlane`
  holds it; it is sent to RoboCAD only as the plane argument of an
  operation that uses it. *Why:* RoboCAD's `viewport.active_plane` is its
  GUI's state, and `PUT /view {"active_plane"}` is GUI-only (a headless
  service answers 409), so it could not be shared anyway. *Rejected:*
  `PUT /view` (fails headless; would move RoboCAD's window when attached).
  *Revisit if* RoboCAD stores the active plane in the document.
- **Sketch tools are catalogue entries driven by one interaction.** 13
  `OpEntry`s with `Flow::Sketch(shape)` and one `SketchSpec` row each;
  `interact::pointer`, `specs::from_points` and `from_values` have no
  per-tool branch beyond the shape's geometry. *Why:* surfaces, keys, REST
  and `system_ui` already read the catalogue, and RoboCAD's `SketchTool`
  is itself one class. *Rejected:* one tool type per shape; a separate
  sketch-mode state machine. *Revisit if* a constraint solver arrives
  (RoboCAD has none).
- **A finished shape is one `cad_sketch`.** The pointer writes
  `CadSketch {node: None, plane, calls, revision}`, and the action picks
  the sketch by RoboCAD's rule (tools.py:675-686: the selected sketch on the
  plane, else the first visible one, else new). A new sketch is created
  with the first shape (`POST /nodes {"kind": "sketch", "plane", "calls"}`),
  not when the tool starts. *Why:* activating a tool never edits the
  document, so Escape leaves nothing behind, and REST and the pointer share
  one path. *Rejected:* `POST /ops/new_sketch` on activation (an empty
  undo step and a node for every tool started). *Revisit if* a user wants
  the empty sketch to exist before drawing.
- **The `Service.edit_sketch` fix.** api.py turned every two-number list
  into a point before mapping curve indices, so `join([0, 1])`, trim or
  extend with two curves, `circle_tangent` and `arc_tangent` failed over
  REST. Indices are now mapped first; route, body and undo label are
  unchanged. *Rejected:* refusing those calls natively (the REST surface
  would stay broken for every client). *Revisit if* api.py's call format
  changes.
- **History label "Sketch (API)".** Every native sketch edit goes through
  `POST /nodes/{id}/sketch`, which RoboCAD's history labels "Sketch (API)";
  its GUI labels "Sketch rectangle" (its `SketchTool` label), "Offset curves". The native status line
  shows the GUI's wording. *Rejected:* a new route taking a label (a
  Python change for a label only). *Revisit if* the parity harness
  compares history labels.
- **A runs the three-point arc.** keymap.json binds `sketch.arc` to A, a
  command RoboCAD never registers; the native registry binds A to
  `sketch.arc_3pt`, the only arc tool. *Rejected:* leaving A dead as
  RoboCAD does. *Revisit if* RoboCAD fixes its keymap differently.
- **The right-click menu gains a "Sketch" section.** The 13 tools follow
  RoboCAD's 14 entries, which stay first and unchanged. *Why:* the batch
  asked for every sketch tool in the 3D view's own menu. *Rejected:* a
  separate sketch pie (more surface to keep). *Revisit if* it crowds the
  menu.
- **Selecting a plane node activates it.** Exactly one selected plane node
  becomes the active plane (`plane::sync`). *Why:* otherwise an existing
  plane node could be made active only by rebuilding it with a plane tool,
  as in RoboCAD. *Rejected:* a separate "make active" command (one more
  step for the common case). *Revisit if* selecting planes for other
  operations changes the active plane unexpectedly.
- **The named active plane is drawn.** `plane_draw::wanted` adds a quad for
  the active plane when no visible plane node is it (XY, XZ, YZ, a hidden
  node). *Why:* the plane the tools draw on is always visible. RoboCAD
  draws plane nodes only. *Revisit if* the quad hides geometry users need.
- **A box off XY is sent as `Ops.box_three_point`.** RoboCAD's tool
  extrudes a sketch rectangle; no route takes a client-built sketch body,
  and `Ops.box` is axis-aligned. `args::place` picks a, b, c so the
  three-point box spans the same solid (label "Box"). On XY it stays
  `Ops.box`. *Rejected:* sketch + extrude as two edits (two undo steps
  where RoboCAD's tool records one composite). *Revisit if* the undo
  label must read "Extrude".
- **Fillet corners simulates the kernel.** RoboCAD's GUI loops over every
  corner and swallows each `KernelError`; over REST one failing call fails
  the whole list. `edits::fillet_plan` runs kernel/sketch.py's
  `fillet_corner` exactly (`edits::fillet_corner`, its straight-corner
  and too-large checks, each fillet's 9 arc points changing the next
  corner's neighbours) and sends only the corners RoboCAD would round.
  *Rejected:* one REST call per corner (an undo step each). *Revisit if*
  the kernel's fillet changes (the simulation must follow it).
- **Join with one curve or none is refused.** RoboCAD records an empty undo
  step (and is silent with no sketch); the viewer refuses by name and
  sends nothing, as it does for an empty offset or a fillet with no
  corner. *Revisit if* the harness needs identical history.
- **A revolve press and release revolves 360°, as RoboCAD's.** RoboCAD's
  release calls `_apply(h, …)` with `angle=None`, so a click revolves a
  full turn whatever the angle field says; the viewer keeps that gesture
  for parity (one `tool.revolve` with angle 360 and the release's
  boolean; the readout says so) and the form's OK revolves by the typed
  angle. *Rejected:* no click commit (a silent gesture), or the click
  reading the angle field (not RoboCAD). *Revisit if* RoboCAD's release
  is fixed to read the angle.
- **The drag sends taper 0, as RoboCAD's.** RoboCAD's drag sends
  `self.taper`, which stays 0.0 (only its Tab commit reads the field); the
  viewer's drag does the same, so the preview (which draws no taper) shows
  what is sent, and only the form's OK sends the typed taper. *Rejected:*
  sending the form's taper draft (a taper typed once would apply to every
  later drag unseen, and a half-typed draft would fail every drag).
  *Revisit if* the preview draws the taper.
- **The extrude preview is outlines only.** RoboCAD tessellates a preview
  body with its kernel (`_preview`), which no REST route serves; the
  viewer draws the profile at the base and the top with connecting lines,
  without the taper. *Rejected:* a preview route (a Python addition for
  display only). *Revisit with* a Rust kernel (§9 phase 4).
- **Slot caps are drawn outward.** `io/exporters.py` `_slot_points`
  sweeps the other way from the kernel's slot, so RoboCAD's viewport and
  SVG draw the caps turned inward; `SketchCurve::sample` draws the solid's
  outline. *Revisit if* RoboCAD fixes `_slot_points` (then both agree).
- **The text preview is a placeholder box.** RoboCAD's outlines need
  fontTools (`text_outlines`), and its text tool finishes on its one
  click, so its hover never shows text either; the box shows where and how
  large. *Revisit if* text outlines get a route.
- **Enter outside a sketch form's fields does nothing.** RoboCAD's
  `SketchTool.key` takes Enter only to finish a spline, and its Tab values
  commit from the numeric bar; so the form's Enter-submit stands aside for
  `Flow::Sketch`, and Enter in "Text to sketch:" starts the clicks, as
  RoboCAD's `getText` OK does. *Rejected:* Enter submitting the Tab values
  (would place a shape where RoboCAD finishes nothing).
- **Double-click is 400 ms and 5 px.** Qt delivers a double-click instead
  of the second press (`QStyleHints` defaults); within those limits the
  second press is not a point for any tool and only finishes a spline.
  *Revisit if* the platform's own double-click settings should apply.
- **A press that cannot be sent is not taken.** A press that would start
  a shape (its revision is stamped then) or complete one, and the
  spline's finish, are checked first: `commit_refusal` (an edit in
  flight, the shown document stale or behind RoboCAD's) and where the
  shape would go (`edits::shape_target`: the plane frame and the sketches
  read at the shown revision). Refused, the point is not added, the
  clicked points stay, nothing is sent, and the status line names why
  ("Sketch line not sent: …; click again when RoboCAD has caught up"), so
  fast chained lines are never lost between RoboCAD's answer and the
  refetch. Points clicked before RoboCAD's revision moved are dropped with
  a message (they were made against geometry that is gone). *Rejected:*
  sending and letting the action refuse it next frame (the points were
  already cleared: shapes were silently lost); queueing the shape until
  RoboCAD catches up (sends geometry the user can no longer see refused).
  *Revisit if* RoboCAD answers edits synchronously with the new tree.
- **A lone chained point does not block leaving CAD mode.** After a line
  its end point starts the next one (RoboCAD's chaining); nothing unsent
  is lost by leaving, so `sketch::blocker` counts only points beyond it
  (`SketchState::unsent`).
- **Snaps carry their f64 source point** (`Snap::exact`): the sketch,
  placement, plane-tool and measure picks send the topology's or
  sketch's own coordinates (projected in f64 onto the plane), not the
  f32 point drawn, so a point snapped onto an endpoint coincides with it
  within RoboCAD's 1e-6 joins.
- **Fill takes a node id through RoboCAD's REST.** `ArgConverter._one`
  treated any parameter named `edges` as edge references, so `Ops.fill(edges:
  str | Body)` got a 400 for every node id; api.py now passes a string
  through when the annotation has no `EdgeRef` (cad/tests/test_api_fill_node.py).
  The second minimal, recorded Python change of this epic, with the
  `edit_sketch` index mapping.

### Verification checklist

- `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- `cargo test -p sim-spatial --lib --bins`, in particular
  `cad::sketch::tests::*` (`a_shape_in_progress_blocks_leaving_cad_mode`,
  `escape_drops_the_shape_and_sends_nothing`,
  `the_rows_follow_robocads_needed_counts_in_shape_order`,
  `clicks_build_robocads_calls_for_every_shape`,
  `tab_values_build_robocads_commit_at_the_anchor`,
  `a_finished_line_is_one_cad_sketch_and_the_next_line_starts_at_its_end`,
  `the_polygon_remembers_its_sides`,
  `a_shape_goes_to_the_selected_then_the_first_visible_sketch_on_its_plane`,
  `fillet_corners_sends_only_the_corners_robocad_rounds`,
  `sketch_edits_work_on_the_selected_or_first_visible_sketch`,
  `cad_sketch_refusals_name_the_call_and_the_argument_and_send_nothing`,
  `a_press_that_cannot_be_sent_is_not_taken_and_keeps_the_points`),
  `app::tests::leaving_cad_mode_is_refused_only_on_unsent_sketch_points`
  (through the real mode switch, `switch::leaving_blockers`),
  `cad::ops::tests::a_created_plane_waits_for_the_tree_that_shows_it`,
  `cad::snap::tests::the_exact_point_is_the_source_points_f64`,
  `cad::sketch::extrude::tests::*`,
  `cad::sketch::plane::tests::complete_picks_make_one_run`,
  `cad::ops::tests` (`plane_entries_follow_robocads_registry`,
  `active_plane_entries_are_viewer_state`,
  `plane_parameters_default_to_the_active_plane`,
  `primitives_are_placed_on_the_active_plane`),
  `cad::snap::tests::sketch_endpoints_and_the_plane_step`,
  `cad::surfaces::tests` (`the_context_menus_sketch_section_is_robocads_sketch_tools`,
  `every_cad_sketch_command_runs_through_the_catalogue`,
  `the_toolbars_sketch_and_extrude_buttons_are_enabled_operations`,
  `menus_and_palette_list_the_cad_sketch_commands_in_robocads_order`,
  `system_ui_reaches_every_cad_sketch_command_as_a_click_does`,
  `cad_sketch_is_a_capability_with_a_valid_example`),
  `app::tests::source_files_stay_small`, and the earlier epics' tests.
- `cargo test -p sim-runtime --lib cad_client units` (new in
  `cad_client::sketch_tests`: `sketch_decodes_as_api_writes_it`,
  `sketch_reads_missing_and_malformed_fields_tolerantly`,
  `edit_sketch_request_and_answer`, `create_sketch_request_and_answer`,
  `plane_of_a_plane_node`, `every_call_to_json_and_back`,
  `from_json_refusals_name_the_call_and_the_argument`,
  `check_calls_tracks_curve_indices`, `sample_counts_as_the_viewport_draws`, …).
- `cd cad && .venv/bin/pytest -q tests/test_api*.py`, including the new
  `tests/test_api_sketch_calls.py` (join of two curves, trim with two
  cutters, extend to two targets, `circle_tangent` to three curves,
  `arc_tangent` after a line, points still become points) and
  `tests/test_api_fill_node.py` (fill takes a closed curve's node id;
  fillet's edge references still convert).
- `cargo check -p sim-web --target wasm32-unknown-unknown`.
- Then the user's [docs/cad-checklist.md](../cad-checklist.md) CAD-77 to
  CAD-98, each step against RoboCAD's own window.

### Reading trace (plane from face → rectangle → extrude with Ctrl → undo)

Function names, not line numbers (fixes may follow the verification pass).

- **Plane from face.** Ctrl/Cmd+P → `keys::keys` (matches `tool.plane`'s
  "Ctrl+P" from `surfaces::registry`; ready) → `CadInvoke { tool.plane }`
  → `actions::handle` → `ops::handle` → `ops::invoke`:
  `Flow::PlanePick(Face)` → `end_tool`, `OpsState::active = tool.plane`, no
  form → `sketch::plane::begin` (picks cleared, selection mode Face,
  `selection::publish`) → the hint. A left press on a face:
  `sketch::plane::picks` (SimSync; `pick::pointer` leaves the left button
  to the plane tool) → `transform::ray_hit` → `CadMeshes::face_at` at the
  shown revision → `PlanePick::Face` → `plane::run_for` → `CadRun
  {id: tool.plane, items: [[node, "face", i]], revision}` →
  `ops::handle` → `ops::run` → `ops::prepare`
  (`CadDocument::commit_refusal`, the revision check, `resolve::resolve`,
  `values`, `args::build` → `plain`: `[node, face]`) → `ops::start` →
  `actions::edit` → `sync::start_edit` (a Dedicated job) →
  `CadClient::op` → `POST /ops/plane_from_face` (RoboCAD's
  `Ops.plane_from_face`, one undo step) → `ops::started` marks the edit
  `activates_plane` → the job lands: `sync::finish_edit` writes the new
  node's id to `ops.plane_created` → `sketch::cache::sync` reads its frame
  (`CadClient::node`, `plane_of`) → `sketch::plane::sync` sets
  `CadActivePlane` to `Node {id, frame}`, status "Active plane set";
  `plane_draw::quads` brightens its square.
- **Rectangle.** Shift+L → `keys::keys` → `CadInvoke { sketch.rectangle }`
  → `ops::invoke`: `Flow::Sketch(Rectangle)` → `open_form` (width, height,
  anchor; unfocused) → `sketch::interact::begin` (`SketchState`). Each left
  press: `interact::pointer` → `snap::snap_on` (the drawn bodies' and
  sketches' candidates, on the active plane's frame) → `PlaneFrame::project`
  → the point appended (the first records `began`); the preview
  (`preview::draw`) and readout ("W × H") follow the cursor. At the second
  point (`SketchSpec::finish` `Points(2)`): `interact::finish_action` →
  `specs::local` → `specs::from_points` (`rectangle(corner, size)`) →
  `CadSketch {node: None, plane: <plane node id>, calls, revision: began}`
  → `ops::handle` → `sketch::edits::sketch_action` (`commit_refusal(began)`)
  → `edits::prepare` (`SketchCall::from_json`; `specs::target`: the
  selected sketch on the plane, else the first visible one, else
  `SketchTarget::New`; `check_calls`) → `ops::send_sketch`
  (`specs::note_polygon_sides`) → `actions::edit` →
  `CadClient::create_sketch` → `POST /nodes {"kind": "sketch", "plane":
  "<plane node id>", "calls": [["rectangle", …]]}` (RoboCAD's
  `Service.create`: `Ops.new_sketch`, then `edit_sketch`, label "Sketch
  (API)"), or `CadClient::edit_sketch` → `POST /nodes/{id}/sketch` when a
  sketch is on that plane. `interact::reset_after_finish` clears the
  points; the cache refetches the sketch at the new revision and
  `display::draw` draws it.
- **Extrude with Ctrl (union).** Select the sketch, X → `CadInvoke {
  tool.extrude }` → `ops::invoke`: `Flow::Extrude` → `open_form`
  (distance 10, taper 0, boolean new) → `sketch::extrude::begin`
  (`extrude::source`). A left press: `extrude::pointer` → `source_plane`
  (the sketch's plane from the cache) → `CadView::ray` meets it
  (`view::ray_plane`) → `ExtrudeDrag`; moving: `transform::push_distance`
  → the height, readout "extrude h", `extrude::draw` outlines. Release
  holding Ctrl: `boolean_for(false, true, false)` = "union" → `CadRun
  {tool.extrude, {distance, taper, boolean: union}, revision at the press}`
  → `ops::run` → `ops::prepare` → `args::build` → `extrude::calls`
  (`body_under_selection`: the selected body, else the only visible body)
  → `ops::start` → `actions::edit` → `POST /ops/extrude {"args": [source,
  h, null, taper, false, "union", target]}` (RoboCAD's `Ops.extrude`).
- **Undo.** Ctrl/Cmd+Z → `keys::keys` → `CadInvoke { edit.undo }` →
  `ops::invoke` (not in the catalogue) → `surfaces::invoke_command` →
  `registry::invoke` (`Native::Action(Do::Undo)`) → `actions::handle`
  `CadUndo` → `actions::edit` → `POST /undo` (RoboCAD's stack undoes the
  extrude; again: the sketch edit, the new sketch, the plane).

## Shared camera and CAD views (2026-10-01)

Batch cad-views-export (default order item 7, §9 phase 1; §9 "Later CAD
epics" 4) did two things. First it gave every mode one camera: `camera/`
replaces CAD's `CadOrbit`, Robot's `RobotOrbit`, Phenomena's
`PhenomenaOrbit` and the spatial view's `Orbit`, together with each mode's
own orbit input and viewport code. Then it brought RoboCAD's view, display
and file workflows into CAD mode: the six display modes, the grid, the
build plate, the view cube, high contrast, the section tool (a preview
and the exact section), saved views, isolate, hide and show all, per-node
tessellation tolerance, new, open, save as, import with units, export in
every `exporters.py` format and the drawing, and render. RoboCAD's command
layer still does every edit, and the display is display only. The ledger
rows are in [docs/cad-parity.md](../cad-parity.md) (112 rows; their split
into done by reading and deliberately different, and the ledger's totals
after the epic, are in its Counts), and the side-by-side steps are in
[docs/cad-checklist.md](../cad-checklist.md) Part F. The work was written
and reviewed by reading in commits 78553886, 9b1e5eec and f15766ea and is
verified at bcf0c56c ([Verification result](#verification-result-bcf0c56c)).
Paths are relative to `crates/sim-spatial/src/` unless they name
another crate.

### Shape

- **The camera** (`camera/`, 2,458 lines in 7 files including the 881-line
  `tests.rs`):
  - `mod.rs` holds the types. `Orbit` is the crate's one orbit-state
    component: focus, radius, yaw/pitch or a `trackball` quaternion,
    `orthographic`, `fov`, the framed `centre`/`extent`, a `home` request, a
    `Glide` and `spin`. `OrbitRules` is the mode's feel (`rate`,
    `pan_rate`, `pitch_limit`, `RadiusLimits::{Extent, Absolute}`,
    `Framing::{Bounds, Fixed}`, `glide_home`, `zoom_to_cursor`,
    `yield_to_ui`, `robocad_gestures`) plus a per-frame input gate the mode
    writes as data (`enabled`, `zoom_modifier`, `reduced_motion`, `keys`,
    `alt_left`; `typing` until one-text-entry, which gates the camera's keys
    on `ui_kit::text::typing` instead). `ViewArea::{Window, Docks, Card}` says where the
    view draws. `ViewPreset` is RoboCAD's `set_view` table
    (`robocad_degrees`, `robocad_to_display`/`display_to_robocad`).
    `OrbitMode` and `CameraState` round out the types.
  - `CameraAction` has 13 REST commands: `camera_view`, `camera_opposite`,
    `camera_projection`, `camera_fov`, `camera_fit`, `camera_home`,
    `camera_pan`, `camera_zoom`, `camera_orbit` (pixels or `degrees`),
    `camera_orbit_mode`, `camera_spin`, `camera_set` and `camera_state`.
    All are tagged with `ORBIT_MODES` (every mode but Place), and the
    `system_ui` controls are `camera:*`.
  - `CameraSet` orders Viewport → Navigate → Place inside SimSync.
    `CameraPlugin` registers the action once, puts `input::keys` in Input
    and `apply::apply` in Actions, and runs `viewport::viewport`,
    `input::navigate` and `orbit::place` with `fly::fly` in that order.
  - `orbit.rs` holds `Orbit`'s methods (`rotate`, `rotate_by`, `pan`,
    `zoom` with an anchor, `snap_to_axis`, `set_trackball`, `preset`,
    `opposite`, `framing`/`frame`/`frame_bounds`, `glide_to`, `step`,
    `interrupt`, `projection`), `view_aspect` and the `place` system.
  - `input.rs` has `navigate`: a drag latched where it starts, `drag_kind`,
    the Alt+left press past `ALT_DRAG_SLOP` (6 px) and the wheel with
    `cursor_anchor`. It also has `keys` (the numpad and `arrow_action`).
  - `apply.rs` has `apply`, `handle` (with `checked` and `fov` refusals)
    and `controls`/`control_action`. `viewport.rs` has `viewport`, `wanted`
    and `area`. `fly.rs` is Place's `Fly`, `orientation` and `fly`, moved
    unchanged from `place_view.rs`.
- **Each mode's spawn**:
  - CAD: `cad/scene.rs` `rules()` sets 89.5° pitch, zoom 0.05–40 × extent,
    zoom to the cursor, `yield_to_ui`, numpad `keys` off and
    `robocad_gestures` on. `setup` spawns `Orbit`, `rules()` and
    `ViewArea::Docks`. `fit` writes every drawn body's bounds on
    `CadMeshes::epoch`, runs `cad_fit` through `frame_bounds`, and sets the
    gate from `gate`.
  - Robot: `robot/ui.rs` spawns the camera; `robot/scene.rs` `view_area`
    keeps the docks, which move with the graph dock.
  - Phenomena: `phenomena/scene.rs` uses sim-app's rates,
    `RadiusLimits::Absolute` 3–30 m and `Framing::Fixed(home)`.
  - Inspect, Build and Lessons: `inspect_view/camera.rs` `spatial_rules`
    (the overview from yaw 0.35, pitch 0.60, gliding), `sync_camera` and
    `view_area` (a lesson card is `ViewArea::Card`).
- **CAD display** (`cad/display/`, 2,569 lines in 6 files):
  - `mod.rs` holds `CadDisplay` (mode, grid, build plate, high contrast,
    view cube, comment pins, `Section`, `ExactSection`), `DisplayMode`
    (RoboCAD's `MODES`), `SectionPlane`, `apply_display`, `apply_section`,
    `exact_query`, `default_plane`, `state_json`, `handle`, and the specs
    for `cad_display` and `cad_section`.
  - `draw.rs` holds `materials`, `edges_sync`/`lines` (display edges and
    curve nodes), `draw_grid`, `quads` (the plate and the section plane)
    and `lights`.
  - `section.rs` holds `clip`, `segments`, `overhangs`, `clip_polyline`,
    `derive_preview` and `preview` (a `Pool::Compute` job), plus
    `exact_jobs` on a `jobs::Latest` (`Pool::Dedicated`).
  - `ui.rs` holds the view cube (`cube`, `cube_face`, `facing`,
    `cube_action`, `cube_press`) and the display `toolbar`.
  - `entry.rs` holds the section offset field (`offset_action`, `input`).
- **Saved views** (`cad/views/`, 1,279 lines in 4 files): `mod.rs` holds
  `CadViews`, `handle` (list, save, rename, replace, delete, restore),
  `restore`, `settle_save`, `sync` (the list on a job) and `snapshot`
  (after `CameraSet::Place`). `convert.rs` has `capture`, `camera_of`,
  `apply_display`, `rot_of`/`rotation_of` and the model ↔ display maps.
  `panel.rs` has the Saved Views panel and the field-of-view entry.
- **Catalogue** (`cad/ops/catalogue/view.rs`): `view.isolate` (/),
  `view.show_all` (Alt+H) and `view.hide` (H), one Ops call each.
- **Files** (`cad/files/`, 2,224 lines in 5 files): `mod.rs` holds
  `CadFiles`, `handle`, `file` (new, open, save_as, import, guess_unit,
  close), `save` (also `cad_save`), `export`, `render`/`render_request`,
  `command_action` and the specs for `cad_file`, `cad_export` and
  `cad_render`. `form.rs` is the kit path form (rows, unit guess, listing
  picks, `open_rule`). `formats.rs` is every `exporters.py` format with its
  settings (`format`, `settings`, `extension_fits`). `jobs.rs` has
  `start`, `wait`, `receive`, `keep_result`, `list`/`request_listing` and
  the status strip.
- **Split from `actions`**: `cad/snapshot.rs` (`cad_state` with `display`,
  `views` and `files`, and the REST snapshot) and `cad/rest_form.rs` (a
  `CadAction` as its REST command).
- **Client** (`sim-runtime/src/cad_client/`):
  - `views.rs` has `SavedView`, `ViewState` (exactly the 12
    `VIEW_STATE_KEYS`; `check` mirrors `validate_state`),
    `check_view_name`, and `views`, `view`, `save_view`, `update_view` and
    `delete_view`.
  - `section.rs` has `SectionQuery::{named, …}`, `SectionCurves` and
    `section`.
  - `files.rs` has `import`, `save_with_thumbnail`, `new_file`,
    `mesh_units`, `render` and `RenderRequest`.
  - `mod.rs` adds `NODE_TOLERANCE` and `export`. `loopback_http.rs` gains
    `exchange_bytes`, which render uses.
  - Tests are in `views_tests.rs`, `section_tests.rs` and `files_tests.rs`.
- **Python gap routes** (`cad/robocad/api.py`, 87 lines added; each calls
  existing functions only):
  - `POST /new` is `Service.new_file`: an exclusive create, then
    `Document().save`, removed again on failure.
  - `POST /save/thumbnail` is `save_with_thumbnail`: the desktop's
    `thumbnail()`, or headless a 256×192 `render` at tolerance 0.
  - `GET /import/units` is `mesh_units`: `importers.load_mesh_file` and
    `mesh_units_guess`.
  - The pytests are in `cad/tests/test_api_files.py`.

### Decisions

- **One `Orbit` component with per-mode data.** Each mode spawns `Orbit`,
  `OrbitRules` and `ViewArea` and writes only data into them; the module
  holds no mode code. *Why:* five copies of orbit, pan, zoom, viewport and
  glide code had drifted (pitch limits, wheel scaling, viewport fallback),
  and REST camera commands needed one target. *Rejected:* per-mode wrapper
  components over a shared core, which would keep five input systems.
  *Revisit if* a mode needs a gesture that cannot be written as rules
  data.
- **Bevy 0.19.1's first-party controllers are not adopted; the orbit core
  is ours** (`camera/fly.rs` module doc). `FreeCamera` lives in the
  `bevy_camera_controller` crate, which is in neither the lockfile nor the
  local registry. It grabs the cursor on a right-click, and its state
  (velocity, speed multipliers) is not REST-addressable the way `Fly`'s
  yaw, pitch and speed are for Place's `camera` and `state`. `PanCamera`
  is 2D, and 0.19.1 has no orbit controller. Place's `Fly` moved to
  `camera/fly.rs` unchanged: it is first-person, with no focus to orbit,
  so it is not orbit state. *Revisit if* Bevy ships an orbit controller
  whose state can be set and read as data.
- **Grid: gizmo lines as RoboCAD's `_draw_grid`, not `bevy_dev_tools`'
  `InfiniteGrid`** (`cad/display/mod.rs` module doc, `draw.rs`
  `draw_grid`). The grid uses a 10 mm step (`GRID_STEP_MM`) and ±20 steps
  (±200 mm, `GRID_STEPS`) on the model's XY. Every 5th line is major
  (minor lines are 0.7 × the major colour 0.36, 0.38, 0.42), and the axes
  are red X, green Y and blue Z. *Rejected:* `InfiniteGrid`. It is
  infinite and fades with distance, marks every 10th line, and colours
  only the X and Z axes (`InfiniteGridSettings`). It would need the
  `bevy_dev_tools` feature (in the registry, not in the lockfile), and the
  section plane could not cut it. *Revisit if* users want an unbounded
  grid.
- **Presets come from RoboCAD's `set_view` table** through `yaw =
  robocad_yaw + 90°`, because CAD and Robot hang their Z-up models from a
  root rotated −90° about X (`camera/mod.rs` `robocad_to_display`). Top
  and bottom are ±89.5°, as RoboCAD's.
- **CAD feel changes, on purpose** (`cad/scene.rs` module doc). The pitch
  limit is RoboCAD's 89.5° (it was 1.5 rad ≈ 85.9°), and the wheel zooms
  toward the point under the cursor, as RoboCAD's `Camera.zoom(factor,
  anchor)` (it zoomed toward the focus).
- **RoboCAD's gestures only in CAD** (`OrbitRules::robocad_gestures`).
  These are Shift+middle orbit, Alt+right snap to the nearest axis view
  after every orbit step (as `mouseMoveEvent` calls `snap_orthographic`),
  Alt+left-drag orbit once past the 6 px slop, and the arrow keys (10°, or
  90° with Ctrl/Cmd, and Shift pans 4 px per degree). Other modes keep
  their feel. RoboCAD's own Alt+left orbit never fires: the press reaches
  `_on_drag`, which sets `_tool_dragging` (ui/app.py:542), and that flag
  blocks `alt_left` (ui/viewport.py:1488). Natively a short Alt+click
  stays CAD's candidates menu and a drag orbits. *Revisit if* the
  checklist shows users relying on RoboCAD's behaviour.
- **Robot: intended differences.** A drag latches to the camera where it
  starts, and continues over a panel until both buttons are up. Robot's
  Fit (`robot/actions/mod.rs` `RobotAction::Fit`, `orbit.home`) now
  re-centres the focus on the bounds, as RoboCAD's `focus_all`; Robot's
  former Fit kept the focus.
- **Viewport fallback.** When the docks leave no room, a view draws over
  the whole window rather than outside it (`viewport.rs` `docks`; CAD's
  former rule, now every mode's).
- **The headless inspect server refuses `camera_*`** and `system_ui`
  `camera:*` by name: it has no window and no camera to move
  (`app/route.rs` `route` with `window` false). Its controls list no
  camera control.
- **Meshes at each node's own tolerance.** `mesh.rs` asks with
  `NODE_TOLERANCE` (0), which RoboCAD's `Document.mesh_of` reads as
  `tolerance or n.tessellation_tolerance` (document.py:474), so a body is
  drawn as RoboCAD's viewport draws it. The default 0.05 mm is finer than
  the former fixed 0.1 mm. That means more triangles on curved faces (up
  to about twice as many for doubly curved ones by chord-tolerance
  scaling; not measured). *Revisit if* frame time or fetch time suffers
  on large assemblies.
- **Display modes are approximations without custom shaders**
  (`draw.rs` module doc):
  - Matcap is the colour times RoboCAD's clay tint, fully rough, under
    the view-following headlight, with no rim term.
  - Render is the headlight casting shadows plus RoboCAD's fill and back
    lights. There is no ground shadow (Bevy has no shadow-catcher
    material) and the ambient is not lowered.
  - Wireframe is a fully transparent, still pickable copy plus edges.
  - X-ray is 0.35 alpha with no depth writes, plus edges.
  - *Revisit with* a matcap shader if the look matters.
- **High contrast covers the 3D view only** (background 0.98, 0.98, 0.99,
  grid and edges). RoboCAD also swaps its Qt stylesheet; the kit's tokens
  are constants, so a UI theme switch needs a runtime `ui_kit` theme.
  *Revisit with* that theme.
- **The view cube is a 3 × 3 net of kit buttons** (Top; Left, Front,
  Right; Iso, Bottom, Back), not a shaded 3D cube. The face the camera
  faces is lit (`facing`, RoboCAD's `view_cube_hit` at the centre). A
  second click on the current face sends `Opposite`, as RoboCAD's does.
  *Revisit if* the checklist finds the net hard to read.
- **Section preview by clipping display triangles** on a `Pool::Compute`
  job (`section.rs` `clip`, `derive_preview`). The side the normal points
  to is removed, as RoboCAD's `glClipPlane`. The default plane is XZ
  through the bounds' centre in Y (`SectionTool.activate`).
  - **The exact section** reads `GET /nodes/{id}/section` on a `Latest`
    job, cached by (node, revision, plane), and is never drawn stale.
    It works only for planes RoboCAD's route can name, xy, xz or yz
    through the origin or a plane node (api.py passes `plane` as a
    string to `ArgConverter.plane`; `exact_query` refuses anything else
    by name).
  - **An offset field replaces R, Tab and the plane drag**
    (`entry.rs`): R is the Rotate tool's key, Tab the numeric bar's, and
    a left drag on the plane quad is box select or the camera's.
    *Revisit if* the section becomes a tool that owns keys.
- **Saved views in RoboCAD's exact 12-key view-state schema, restored
  natively.** Save and replace capture the native camera (`snapshot`,
  after `CameraSet::Place`) and `CadDisplay` through `convert::capture`.
  Restore reads the listed view and sends `CameraAction::Set` (a cut) and
  the display state. *Rejected:* `POST /views/{id}/restore`, which is
  GUI-only (409 headless) and moves RoboCAD's own camera.
- **Isolate, hide and show all are catalogue operations**, one RoboCAD
  undo step each. Isolate and Hide refuse an empty selection by name;
  RoboCAD hides everything, or pushes an empty step. *Revisit if* the
  parity harness compares history.
- **The tessellation tolerance lives in the inspector**, where RoboCAD
  keeps it (0.005–2 mm, one PATCH). `node_summary` does not report the
  current value, so the field opens empty.
- **File dialogs are a kit path form, not `rfd`** (`files/mod.rs` module
  doc). `rfd` is not in the workspace and would add a new dependency
  tree, and its macOS dialogs must run on the main thread's event loop,
  which cannot be checked without running. *Revisit in* a verification
  pass that can run the viewer.
- **Unsaved edits: new and open follow `cad_open`'s rule.**
  `CadDocument::switch_blockers` refuses by name while an edit is in
  flight or a self-started service has unsaved or unconfirmable edits; an
  attached RoboCAD keeps its edits (`leaving_note`). This matches RoboCAD,
  whose New and Open open another window and never lose edits. *Rejected:*
  a Save / Discard / Cancel prompt (see CAD mode's decision).
- **Every save goes through `POST /save/thumbnail`**, as RoboCAD's
  desktop `save`/`save_as` always write the thumbnail; a plain `/save`
  would erase it. Save As (and `cad_save {path}`) retargets a
  self-started document's file once the save succeeds (`Edit::retarget`).
- **Gap routes added versus deliberately different.** Added: `POST /new`
  (exclusive create), `POST /save/thumbnail` and `GET /import/units`.
  Deliberately different:
  - the autosave interval and failure report: RoboCAD's desktop timer and
    status-bar message, with no stored state;
  - `/capture` and `/screenshot`, the Blender live link and web share,
    which are GUI-only;
  - `edit.preferences` and `inspect.draft`;
  - SpaceMouse: Bevy 0.19.1 has no 6-DoF input and the lockfile has no
    HID crate.
- **A cancel of a sent export or render says what was written**
  (cad-parts-a-f-retrace, 2026-10-02). api.py has no cancel route, so
  RoboCAD runs a sent request to its end. The job strip's Cancel (and
  `cad_file {op: cancel, job?}`, `cad:file:cancel-<job>`) asks the job to
  stop (`cad/files/jobs.rs:cancel`): a render's PNG is the viewer's to
  write and is not written once the cancel is seen; an export's file is
  RoboCAD's, so its outcome says the cancel did not stop it. A REST
  caller's cancel still stops waiting only, and the outcome lands in
  `cad_state.files.last`.

### Key clashes

The cad-views-export rows of `cad/keys.rs`' module doc table (grep of
`KeyCode::` over `crates/sim-spatial/src`, 2026-10-01; that table is the
full, current one), with the cad-print keys that share their letters
(Ctrl+H, Ctrl+W, Ctrl+Shift+V, Ctrl+Shift+C), then the shared camera's keys:

| Key | Commands | Resolution |
|---|---|---|
| Ctrl+S, Ctrl+Shift+S, Ctrl+Shift+D | save, save as, export drawing | transform's S and D act only without Ctrl |
| S, Shift+S, Ctrl+S, Ctrl+Shift+S | scale (transform), sketch slot, save, save as (the files part's row) | transform's S refuses Shift and Ctrl; the rest are exact modifiers here |
| X, Ctrl+Shift+X | extrude, section analysis (`cad_section` toggle) | exact modifiers |
| Ctrl+Z, Ctrl+Shift+Z, Z | undo, redo, next display mode (`cad_display` next) | exact modifiers |
| B, Shift+B, Ctrl+Shift+B | select bodies, select faces, build plate preview (`cad_display` toggle) | exact modifiers |
| F, Shift+F, Ctrl+F, Ctrl+Shift+F | focus selection (frames the selected nodes; Fit All with none), palette, fillet, chamfer | exact modifiers |
| H, Alt+H, Ctrl+H, Ctrl+Shift+H | hide, show all (catalogue: `Ops.set_visible`, `Ops.show_all`), fastener (cad-print), shell | exact modifiers; macOS's Option+H types "˙", but keys match the physical key (`KeyCode::KeyH`) with Alt held, so Alt+H reaches Show All |
| Ctrl+H | `tool.fastener` (cad-print: starts the fastener tool, its form beside the view, then face clicks) | macOS's app menu takes Command+H (hide); Control+H starts the tool |
| Ctrl+W | `print.wall_check` (cad-print: the "Flag walls thinner than (mm):" form) | winit's default macOS menu has no Close item, so Command+W reaches the check; no other CAD reader of W (the shared camera's fly keys are not used in CAD mode) |
| V, Ctrl+V, Ctrl+Shift+V | select vertices, paste with placement, validate for printing (cad-print) | exact modifiers |
| C, Shift+C, Ctrl+C, Ctrl+Shift+C, Shift+A then C | sketch circle, sketch spline, copy, clearance offset (cad-print: needs selected faces, then its form), cylinder | exact modifiers; C after Shift+A is the chord's (its second step), not the circle's |
| Ctrl+G | grid (`cad_display` toggle) | no other reader in CAD mode |
| 1, 3, 7, 0, Ctrl+1, Ctrl+3, Ctrl+7 | view front, right, top, iso; back, left, bottom (`camera_view`, RoboCAD's yaw/pitch table) | exact modifiers; the keypad's digits are the digits (`normalise`); the shared camera's numpad keys are off in CAD (`OrbitRules::keys` false, `scene`), so a digit is read once; macOS's Mission Control may take Control+digit ("Switch to Desktop n") when enabled, Command+digit still works |
| 5 | orthographic toggle (`camera_projection`) | as the digits above |
| / | isolate (catalogue: `Ops.isolate`) | the keypad's divide is `/` (`normalise`) |
| J, Q, X, T, L, C, A, N, Home | join, selection radial, extrude, sketch text/line/circle/arc, annotate (cad-organize), fit | no other reader in CAD mode |
| Numpad1, Numpad3, Numpad7 (Ctrl: the opposite side), Numpad9, Numpad5, Numpad0, NumpadDecimal, Home | shared camera (`camera/input.rs` `keys`): front/back, right/left, top/bottom, opposite, orthographic toggle, iso, fit, home | read only where an enabled camera's rules set `keys` and no text field is `typing`: Inspect, Build, Lessons, Robot and Phenomena; off in CAD, whose keymap reads the digits and Home itself; no other reader of these numpad keys or Home in those modes (Robot's NumpadAdd/NumpadSubtract set speed; grep of `Numpad` and `KeyCode::Home`) |
| Arrow keys (Ctrl/Cmd: 90°; Shift: pan) | RoboCAD's orbit 10° and pan steps (`camera/input.rs` `arrow_action`, `camera_orbit {degrees}`, `camera_pan`) | CAD only (`robocad_gestures`), not while a text field has the keyboard (the shared `ui_kit::text::typing` condition since one-text-entry; the palette's field takes Up/Down as `FieldEvent::Arrow`); Robot, Phenomena, Lessons and Build read their own arrows, and the camera's arrows are off there |

### Review findings (three reviewers by area, then fixes in f15766ea)

- **Camera.** Fixed:
  - CAD's `camera_fit`/`camera_home` framed stale bounds (only a `cad_fit`
    wrote them; now the bounds follow every mesh change, and a node fit no
    longer overwrites them);
  - fits dropped the trackball;
  - the headless server parsed `camera_*` with a parser that could not
    take them (now a named refusal);
  - a headless fit jumped to the overview heading;
  - the first home framing used aspect 1 before the viewport existed
    (`view_aspect`);
  - CAD's view snapshot paired this frame's transform with last frame's
    projection;
  - the split camera ignored ortho;
  - saved views and notes stored stale yaw/pitch under the trackball
    (`Orbit::turntable`).

  Kept and recorded: Robot's Fit re-centres. Left, pre-existing: the
  builder's placement and markers read `GlobalTransform`, which is a frame
  late.
- **Display.** Fixed:
  - picks on the section preview's appended triangle halves named no face
    (the shown copy's `triangle_face` is kept and read);
  - `system_ui` text;
  - the saved-view name was cleared before RoboCAD answered
    (`settle_save`);
  - a controls test was added
    (`display_controls_fit_a_pattern_and_round_trip_through_rest`).

  Rejected: that edges → faces breaks with coarse node tolerances.
  `selection::faces_along` tests the mesh *vertices* against the sampled
  edge polyline, and tessellation vertices on an edge lie on the edge
  curve whatever the chord tolerance, so the bound is the polyline's own
  sag. The docs drift is this section. Left (display only): the overlay
  outline is not clipped while sectioned.
- **Files.** Fixed:
  - `Edit` literals lacked `retarget` in two test files (a compile error);
  - the Discard path discarded nothing and disagreed with `cad_open`
    (replaced by `cad_open`'s rule);
  - Save As retargeted by label (now tied to its edit, and
    `cad_save {path}` retargets too);
  - a plain Save erased the thumbnail;
  - a mesh import defaulted to mm (now RoboCAD's guess, and OK waits for
    it);
  - New answered before the open;
  - waited answers were pruned by distance (`keep_result` keeps them by
    count);
  - a listing click could pick from a stale listing;
  - api.py's `new_file` raced between its check and its save (now an
    exclusive create).

  Reduced: the headless thumbnail's cost (it reuses the node-tolerance
  tessellation the meshes already cached).
- **Second pass** (two reviewers over f15766ea and the docs; fixed in the
  final commit): `CadMeshes::face_at` was deleted while pick, measure,
  push/pull, dimensions and the plane tools still called it (a compile
  error; restored over `face_of`); the modal file form left the section
  offset and saved-view fields typing, so its keys reached them too; a
  view save's answer could be read from a file job's status line written
  the same frame (`files` results now run before `sync::receive`); docs:
  the CAD sketch intro still said "not compiled", stale notes on
  Phenomena's own camera, the set-enum count, CAD's `scene.rs` and the
  ledger's pre-fix counts. Left: many api.py line citations in
  docs/cad-parity.md predate this epic and are stale as api.py grew
  (cited by name where this epic rewrote the row).

### Found by reading and fixed (across the epic)

- REST `camera`, note restore and discussion restore wrote the pose
  without ending a glide, so the next step overwrote it (78553886).
- `CadView` read the `GlobalTransform` before propagation, a frame behind
  the drawn view (78553886).
- `mesh.rs` fetched every body at 0.1 mm, overriding each node's own
  tessellation tolerance (9b1e5eec; now `NODE_TOLERANCE`).
- A Save As left the window's target on the old file, so leaving and
  re-entering CAD mode reopened it (9b1e5eec; `Edit::retarget`).

### Reading trace (orbit in Robot → saved view in CAD → section → export STEP)

Function names, not line numbers (fixes may follow the verification
pass).

- **Orbit in Robot.**
  1. A right-drag inside the view reaches `camera/input.rs` `navigate`
     (SimSync, `CameraSet::Navigate`). The press latches the camera whose
     `ViewArea` contains the cursor (`accepts`, `viewport::area`; Robot's
     docks are written by `robot/scene.rs` `view_area`, before
     `CameraSet::Viewport`).
  2. `drag_kind` gives `Orbit`, then `Orbit::interrupt` and
     `Orbit::rotate` (yaw and pitch within `pitch_limit` 1.4).
  3. `orbit.rs` `place` (`CameraSet::Place`) writes the `Transform` from
     `Orbit::transform`.
- **Switch to CAD.** `viewer_mode {"mode": "cad"}` goes to
  `app::switch::handle`. On entering, `cad/scene.rs` `setup` spawns the
  camera with `Orbit`, `rules()` and `ViewArea::Docks`, and `fit` writes
  the bounds when the meshes arrive.
- **Save the view.**
  1. `cad_views {"op": "save", "name": …}` (or the panel's Save) reaches
     `cad/actions.rs` `apply`, then `views::handle` `ViewsOp::Save`.
  2. `check_view_name`, then `CadViews::capture`: the camera copied by
     `views::snapshot` after `CameraSet::Place`, then
     `convert::capture` (`ViewCamera` and `CadDisplay` → `ViewState`).
  3. `actions::edit` runs `CadClient::save_view`, which is `POST /views`
     (RoboCAD's `SavedViewOps.save_view`, one undo step "Save view").
  4. `settle_save` clears the typed name on success.
- **Restore it.** `cad_views {"op": "restore", "id"}` goes to
  `ViewsOp::Restore` and then `restore`: `convert::camera_of` and
  `convert::apply_display`, then a `CameraAction::Set` pushed on
  `Cx::camera`. `cad/actions.rs` `apply` writes it as
  `Act<CameraAction>`, and `camera/apply.rs` `apply` → `handle` → `Set`
  (`checked`) applies it.
- **Section.**
  1. `cad_section {}` (or the toolbar chip, or Ctrl+Shift+X) goes to
     `display::handle`, then `apply_section`, which turns the section on
     at `default_plane`.
  2. `section.rs` `preview` (SimSync) starts a `Pool::Compute` job
     (`derive_preview` → `clip`) per body. The drawn mesh is swapped for
     the clipped copy, and `draw.rs` `lines` draws the cut outline.
  3. Exact: `cad_section {"exact": id}` → `apply_section` →
     `exact_query` (xy/xz/yz or the plane node) → sets `ExactSection.request`.
     `exact_jobs` (JobResults) starts `CadClient::section` (`GET
     /nodes/{id}/section?plane=…`) on its `Latest`, `accept` keeps the
     answer, and `ExactSection::drawn` draws it only while it is current.
- **Export STEP.**
  1. `cad_export {"format": "step", "path", "settings"}` goes to
     `files::handle` and then `export`.
  2. The checks are `formats::format`, `absolute`, `extension_fits` and
     `formats::settings` (defaults filled, unknown settings refused by
     name).
  3. `jobs::start` (`Pool::Dedicated`, `complete_on_drop`) runs
     `CadClient::export`, which is `POST /export`; that is `Service.export`
     → `exporters.export_step`.
  4. `jobs::receive` writes the outcome to `cad_state.files.last` and the
     status line; a REST caller waits through `jobs::wait`.

### Verification checklist

- `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- `cargo test -p sim-spatial --lib --bins`, in particular:
  - `camera::tests::*` (27 tests, among them
    `presets_look_as_robocads_views`,
    `drags_latch_where_they_start_and_the_wheel_zooms_inside_the_view`,
    `camera_commands_route_in_every_orbit_mode`,
    `headless_controls_list_no_camera_controls`,
    `fits_and_heading_keeping_homes_keep_the_trackball`,
    `the_first_home_framing_uses_the_view_areas_aspect`,
    `robocad_drags_shift_middle_orbits_alt_right_snaps_and_alt_left_drags_past_the_slop`
    and `arrow_keys_only_where_the_rules_ask_and_no_text_field_types`);
  - `cad::scene::tests` (`the_orbit_bounds_follow_every_drawn_body_and_a_node_fit_keeps_them`,
    `the_gate_follows_the_text_focus`);
  - `cad::display::tests::*` (among them
    `clipping_a_unit_cube_keeps_robocads_side_and_triangle_order`,
    `a_stale_exact_section_is_never_drawn`,
    `picks_on_a_clipped_copy_name_the_copys_faces`,
    `the_view_cube_writes_robocads_views`);
  - `cad::views::tests::*`, `cad::files::tests::*`
    (`new_and_open_use_cad_opens_rule_and_never_discard`, …);
  - `app::tests::source_files_stay_small` and the earlier epics' tests,
    which now run on the shared camera.
- `cargo test -p sim-runtime --lib -- cad_client units loopback_http
  hardware_client` (`loopback_http` and `hardware_client` because
  `loopback_http` gained `exchange_bytes`). New in `cad_client`:
  `views_tests`, `section_tests` and `files_tests`.
- `cd cad && .venv/bin/pytest -q tests/test_api*.py`, including the new
  `tests/test_api_files.py` (eight tests: new, its refusals and its
  cleanup; the headless thumbnail and its reuse of the node tessellation;
  an untitled save; the unit guess and its refusals).
- `cargo check -p sim-web --target wasm32-unknown-unknown`.
- If a build fails, look first at:
  - serde's internally tagged enums (`CadAction`'s new newtype variants
    `CadDisplay`, `CadSection`, `CadViews`, `CadFile`, `CadExport` and
    `CadRender` need struct payloads; `CameraAction`'s
    `#[serde(tag = "command", deny_unknown_fields)]`);
  - Bevy 0.19.1 names in `camera/` and `cad/display/` (`SubCameraView`,
    `Viewport`, `Camera::viewport_to_world`, `GizmoAsset`,
    `bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster}`,
    `Projection`, `HoverMap`);
  - the `Option<Res<Messages<..>>>` + `Local<MessageCursor<..>>` readers
    in `camera/input.rs` `navigate`.
- Then the user's [docs/cad-checklist.md](../cad-checklist.md) Part F,
  each step against RoboCAD's own window.

### Verification result (bcf0c56c)

The verification pass (2026-10-01) over 1b00d789..813f0a86 and its fixes:

- `cargo check --workspace --all-targets` found 3 errors and 2 new
  warnings in sim-spatial. All are fixed in a33d0a2c:
  - tracing's `info!` shadowed serde_json's `Value` (E0782);
  - an `Outcome::Image` arm was missing (E0004);
  - a private `Shown` type was in a `pub(super)` signature;
  - an unused import and an unused re-export.
- Five reviewers read the diff by area. Fixes are in 4883aef2 (files,
  views, section, picking, surfaces) and 8ce1ed43 (camera).
- The camera fixes:
  - split view and close-up work when the view has no viewport;
  - restores stop a spin;
  - leaving orthographic restores the mode's own near plane;
  - Home and Fit frame with the view area's aspect;
  - Robot's zoom is clamped when the model's extent changes.
- The linker failed once on missing `sim_runtime` LLVM-local symbols: a
  corrupt incremental cache left by killed builds. Removing
  `target/debug/incremental/sim_runtime-*` (about 4 GB, regenerable) fixed
  it.
- `cargo build -p sim-spatial --lib --tests --bins`: clean, with no
  sim-spatial warnings (4 min).
- `cargo test -p sim-spatial --lib --bins`: lib 356 passed, 1 ignored;
  bins 4 passed. bcf0c56c fixes two tests, and neither change touches
  behaviour:
  - the node-fit test compared rotations with f32 `angle_between`, which
    is acos noise of about 1e-3 rad; it now compares the quaternion dot
    product;
  - the clip test checked the collapsed point of removed triangles.
- `cargo test -p sim-runtime --lib cad_client`: 65 passed.
- `cargo test -p sim-runtime --lib units`: 29 passed.
- `loopback_http` has no unit tests of its own (0 matched). Its
  `exchange_bytes` is covered by the `cad_client` fake-server tests.
- `cd cad && .venv/bin/pytest -q tests`: 396 passed, before 4883aef2,
  which changes no Python.
- `cargo check -p sim-web --target wasm32-unknown-unknown`: no errors.
  Four warnings remain, all in files this epic did not touch: sim-agent
  lib.rs:451 and :467, and sim-runtime `decision` and `rotor_speed`.

## Window-first usability (2026-10-01)

Batch window-first-usability fixes three bugs the user reported on
2026-10-01 from using the window by hand, in all seven modes:

1. Pressing Robot in a Build window with no robot refused with a REST
   payload ("give path … or preset …, e.g. viewer_mode {…}").
2. The switcher floated bottom right over panel content: the Robot
   inspector's gait list, the Build status line, and its outcome message
   over both.
3. Some window text told people to use REST.

Paths are `crates/sim-spatial/src/`. Written and checked by reading; not yet
built or tested (the verification pass does that).

### Shape

- **Kit widgets** (§6, no intent logic):
  - `ui_kit/path_field.rs` is the one path entry: an input with a
    submit button, and the typed directory's matching entries.
    - The listing runs on `Pool::Io` through a `jobs::Latest<Listing>`
      (`request`/`receive`).
    - `~` and `~/` are expanded, ".." goes up, and the typed file name
      narrows the entries.
    - The pure helpers are tested without a window.
    - CAD's path form (`cad/files/form.rs`) now uses it: its own `dir_of`,
      `file_of`, listing and footer listing are deleted, and so are
      `cad/files/jobs.rs`'s `Listing`/`list`/`request_listing`.
  - `ui_kit/picker.rs` holds `Kit::backdrop(label, cover_strip)`, at
    `MODAL_Z` 50, above the pie (45), CAD popups (44) and the switcher (40),
    and `Kit::document_picker`: titled sections of entries, the path field
    and Close.
- **The strip:**
  - `ui_kit::SWITCHER_STRIP` (40 px, `theme.rs`) is reserved once.
  - `Kit::dock` adds it to every bottom edge (`Dock::Bottom`, `Left`,
    `Right`, and `Under`, which gained a `bottom`). `dock_rect` is the pure
    layout.
  - `Dock::Strip` is the strip itself (`app/switcher.rs`). Other
    bottom-anchored nodes use `above_strip(px)`.
  - Modes add no switcher room of their own: phenomena's `SWITCHER_ROOM`
    and CAD's 580 px status-bar padding are gone.
  - Viewports and wheel hit-tests end above the strip:
    - `inspect_view::bottom()` (Inspect and Build);
    - `cad/scene.rs`, `phenomena/scene.rs` and `robot/scene.rs`
      `wanted_area`;
    - Place's new viewport and its UI camera;
    - the fly camera, which ignores a drag that starts in the strip and the
      wheel over it;
    - CAD popups (`popup_place`).
- **The picker** (`app/picker/mod.rs`, discovery in `app/picker/discover.rs`):
  - `Picker` is a resource. Discovery runs on `Pool::Io` when it opens, and
    is cancelled when it closes.
  - Its sources per mode:
    - the recent documents;
    - Robot: the presets (`robot::preset::list`);
    - the examples under `<workspace>/examples` (and `lessons/`, and `cad/`
      for `.rcad`), skipping `runs`, `target`, `node_modules`, `.venv` and
      dot-dirs, at most 40 per section, with a truncation note;
    - CAD: the RoboCAD service at the default URL;
    - every mode: "Open file…" (suffixes per mode; directories only for
      Lessons and Place; an http(s) URL for CAD).
  - Choices are switches: `Picker::choice`/`typed` build
    `ModeSwitch { mode, document: Some(..) }`, equal to
    `ModeSwitch::from_args` for `viewer_mode {mode, path|preset|url}`. They
    are written as `Act::ui(WindowAction::Switch(..))` and handled by
    `app::switch::handle`. There is no second switch path.
- **Recent documents** (`app/recent.rs`):
  - Historical migration input `recent.json` in the config directory:
    `$SIM_SPATIAL_CONFIG_DIR`, `$XDG_CONFIG_HOME/sim-spatial`,
    `~/Library/Application Support/sim-spatial` or `~/.config/sim-spatial`.
    Never in the repository; `None` under `cfg(test)`.
  - Versioned (`VERSION` 1). A newer version's file, or an unreadable
    file, is never overwritten.
  - Current recents publish atomically in the unified settings envelope;
    the legacy input remains untouched.
  - Accepted switches record through `SettingsOwner`; jobs canonicalizes paths.
    Picker reads the same owner, including its explicit startup readiness.
- **`system_ui`:**
  - While the picker is open, every mode's controls list ends with
    `picker:<mode>:<n>` (the flat index), `picker:path` (activate with an
    optional `text`) and `picker:close`.
  - `route::mode_control` routes `picker:` ids to the window in every mode,
    and `actions::serve` passes the controls to `route::annotate`.
  - A `system_ui` `mode:<mode>` activation is interactive, like a click.

### Decisions

- **Only window intents open the picker.** A click (`Origin::Ui`, which
  includes the builder's Lessons button) and a `system_ui` `mode:<mode>`
  activation open it. REST `viewer_mode` without a document keeps its
  behaviour: a structured Err, or for CAD the default-URL fallback. Its
  refusal text names the in-window way ("robot mode needs a robot: choose
  one in the picker …") with no payloads. Rejected: opening the picker for
  REST too, which is a side effect a script didn't ask for.
- **The picker opens after the leaving checks.** A person is never asked to
  choose a document for a switch that would be refused.
- **CAD with no document.** CAD opens the picker in the window when this
  window has never had a CAD document (`Documents::cad` is None). Its
  entries include the RoboCAD service at the default URL, which REST still
  gets without asking.
- **No keyboard mode switch exists.** grep finds none, so the keyboard
  part of the brief has nothing to change.
- **The picker is modal and safe for STOP.**
  - `picker::keys` runs in PreUpdate after Bevy's input systems. It reads
    keys and the wheel through its own cursors, then clears the keyboard and
    wheel messages.
  - Held keys are released (`clear` + `release_all`), not erased, so a
    walking robot or a jog sees the release and stops.
  - The picker doesn't open in Robot mode while the Leg calibration panel
    is open, and closes if the panel opens, so its STOP stays reachable.
  - Modifiers are tracked from the key messages, so Cmd+V is not typed.
- **The picker's backdrop ends above the strip.** The switcher stays
  usable, since the picker holds no work. CAD's modal forms cover the strip
  (`cover_strip` true), because a switch would discard their typed values
  and `leaving_blockers` doesn't check open forms.
- **The outcome message** wraps within the strip and is clipped to two
  lines (CAPTION, 28 px). The full text is its accessible label, and the
  picker shows later outcomes, such as a refused choice, as its status line.
  Rejected: a toast over the 3D view, which would overlap docks in some
  modes.
- **CAD's form keeps its path input as a kit form row.** Only the listing,
  navigation and expansion moved to the path field: a form row is how the
  kit form draws text inputs.

### No REST in window text

Each instruction is replaced by an in-window control emitting the existing
typed action:

- **Robot, older recordings:** "More recordings…" lists every recording's
  Replay (`RobotAction::Replay`).
- **Robot, the recorded timeline:** a seek slider
  (`RobotAction::Recorded { Seek }`, checked first).
- **Robot, the gait report list:** a compiled-gait path field
  (`RobotAction::Gait { Open { Path } }`). While it has the keyboard, robot
  and camera keys are ignored and held motion keys request zero once. It
  refuses focus while the Leg calibration panel is open, whose Q/A and STOP
  keys read any focus.
- **CAD, unconnected:** an attach-URL field (`cad/attach.rs`,
  `CadAction::CadOpen { url }`; since one-text-entry the kit field `cad.attach`).
- **Wording:**
  - HEADLESS_COMMANDS names the menus, toolbar, right-click menu and
    palette;
  - the empty state points to the picker and Open;
  - the field-of-view prompt;
  - the robot key, recording, mode-switch and planar-graph texts;
  - CAD export warnings;
  - CAD Preferences;
  - the switch refusals and `open_hint`.

Unchanged: capability descriptions, refusals only REST or `system_ui`
callers receive, and the hardware rule texts (an operator at the window for
motion; `robot/hardware/actions.rs`, `handlers.rs`, `SYNC_REMOTE_REFUSAL`).

**Guard.** `copy_guard_tests.rs` fails on any string literal outside tests
naming "REST" (as a word), `cad_state.`, `robot_state.` or `system_ui `.
- Exempt: `spec(`/`c(` capability calls, where `c(` is exempt only in the
  files whose `c` builds a `Spec`, and `specs.rs`/`commands.rs`.
- Allowlisted: a reasoned list of REST-only answers, terminal and OS
  strings, and origin labels.
- Stale allowlist entries fail too.

### Reading trace (Build window → Robot button → picker → preset → Robot mode)

1. A click on "Robot" in the strip: `switcher_clicks`
   (`app/switcher.rs:63`, Input) writes
   `Act::ui(WindowAction::Switch(ModeSwitch { mode: Robot, document: None }))`.
2. `switch::handle` (`app/switch/mod.rs:390`, Actions) marks it interactive
   (`:432`, `origin == Origin::Ui`) and calls `start` (`:514`). After the
   blockers, `missing_document` (`prepare.rs:113`) finds `docs.robot` None
   (`:118`). `start` (`:537`) sets the switcher line "Choose a robot for
   Robot mode in the picker." and calls `Picker::open_for` (`:552`;
   `picker/mod.rs:156`). Discovery starts on `Pool::Io` (`discover`,
   `discover.rs:207`), and Build mode stays.
3. `picker::receive` (`picker/mod.rs:339`, JobResults) takes the sources.
   `picker::draw` (`:599`, Present, registered in `app/mod.rs:353`) spawns
   `Kit::backdrop` + `Kit::document_picker` with the presets, recents,
   examples and "Open file…".
4. A click on a preset: `picker::clicks` (`:378`, `PickHit::Entry` at
   `:388`) gets `Picker::choice` (`:185`), which is
   `ModeSwitch { Robot, Some(Preset(id)) }`, and writes it as
   `Act::ui(WindowAction::Switch(..))`: the same request `viewer_mode
   {"mode":"robot","preset":id}` parses to (`picker_tests.rs:49`).
5. `handle` → `start` → `prepare`'s Robot arm (`prepare.rs:211`): the
   preset loads through `RobotView::open_preset` → `finish_load`
   (`arrival.rs:38`) → `enter` (`arrival.rs:19`) → `NextState(Robot)` →
   OnEnter `arrive` (`arrival.rs:75`).
6. On the next frame `handle` confirms the switch and records the preset in
   the recent documents through `SettingsOwner::record` (T45). The dated
   original trace used `recent::record_job`; that writer is superseded.
   `picker::receive` sees the mode changed and closes the
   picker.

### Tests (windowless)

- `app/picker_tests.rs`:
  - `a_picker_choice_is_the_switch_viewer_mode_builds`;
  - `a_click_to_a_mode_with_no_document_opens_the_picker_and_rest_is_refused`;
  - `refusals_name_no_rest_payload`;
  - `recents_round_trip_dedupe_and_save_atomically`;
  - `a_record_leaves_a_newer_versions_file_alone`;
  - `discovery_finds_examples_and_skips_runs`.
- `app/tests.rs`: the controls cross-check covers `picker:*`.
- `ui_kit/tests.rs`:
  - `path_field_paths`;
  - `path_field_lists_a_directory`;
  - `docks_leave_the_switcher_strip` (the strip layout);
  - `document_picker_spawns_labelled_buttons`.
- `cad/files/tests.rs`: `the_forms_listing_is_the_kit_path_fields`.
- `cad/surfaces/tests.rs`: `popups_are_kept_inside_the_window`, with the
  strip.
- `copy_guard_tests.rs`: `window_text_does_not_send_people_to_rest`,
  `the_lexer_finds_literals_and_spec_calls`.

### Verification checklist

- `cargo build -p sim-spatial --lib --tests --bins` with no sim-spatial
  warnings.
- `cargo test -p sim-spatial --lib --bins`, in particular the tests above
  and the existing `app::tests` switch tests, `jobs::tests` (the thread
  guard) and the 750-line guard.
- By reading, against the built code: the trace above; each mode's docks
  ending above the strip; no UI-thread file I/O in `picker::draw`,
  `robot::panel_ui::gait_path_draw` or `cad::attach`.
- Unverified until screenshots are on: how the strip, the picker and the
  two-line message look in each mode.

**Verified at cd30ef6d** (verification pass, 2026-10-01): the build of
lib, tests and bins finishes with no sim-spatial warnings (other crates'
existing warnings: sim-print `promote.rs:92`, sim-runtime
`joint_steps.rs:44`); `cargo test -p sim-spatial --lib --bins` passes,
lib 368 passed and 1 ignored, bins 4, including every test listed above,
`app::tests::source_files_stay_small` and `jobs::tests`. The one fix:
`SliderValue` is an immutable component in Bevy 0.19, so the recorded
seek slider's sync (`robot/controls.rs` `recorded_panel`) replaces it
through `Commands` instead of mutating it. Recents: `record_in` holds a
process-wide `Mutex` around load, record and save, so two quick
`Pool::Io` record jobs in one window cannot lose an entry; two separate
viewer processes can still race (last rename wins, losing at most the
other's newest entry, never corrupting the file), which is accepted for
a convenience list.

## Documents, selection and annotations (2026-10-01)

*Batch unified-selection-document, delivering §7. Done pending
verification: written and checked by reading; nothing was built or run in
the batch.* Every mode now reads one document registry, one selection and
one annotations service, in place of per-mode fields. No REST command was
added or renamed, and no argument shape or on-disk format changed.

### Module map

| Module | What it owns |
|---|---|
| `document/` | `DocumentRegistry`: one entry per mode's document. Each entry has an id (`DocumentId`, never reused), `DocumentKind`, `Source` (a path, an assembly, a lessons folder and lesson, a preset, a URL or an exhibit), a revision, the owning mode, a `Presence` (open, parked or remembered) and the parked scene. `open` of the same source is a reload: the same id and revision + 1. Another source gets a new id and reports the id it replaced. |
| `app/switch/sources.rs` | The registry as the switch uses it: `Document`/`CadTarget` ↔ `Source` conversions, `open` (which forgets a replaced document's selection items), `ensure_open`, `left`, and `Documents::json`. That JSON keeps `viewer_mode {}`'s old `documents` keys and shapes, plus `registry`. |
| `app::switch::Documents` | Launch configuration only: `library`, `models`, `presets` and `hardware`. The launch's documents are registry entries (`Launch::registry`). |
| `selection/` | `Selection`: typed items (`Item::Component`, `Port`, `Net`, `Link {index, name}`, `Cad([node, kind, index])`), each with its document and the revision it was picked at, plus a change counter and the `dropped` labels. `SelectionAction {op: Set/Add/Toggle/Remove/Clear, document, items}` is validated by `Selection::apply`, which refuses by name an item picked at another revision. One system, `apply_actions`, applies it in `ViewerSet::Actions`; it is registered as feature `selection`, with no REST command of its own. `revalidate` / `revalidate_all` re-check a document's items after it advances. |
| `builder/picked.rs` | Build's view of the selection (`Picked`): builder instance names at the builder's level under the Build entry. `sync` copies `document.revision` to the registry and drops removed instances by name. `track` (SimSync) re-projects the scene highlight in Build. |
| `robot/picked.rs` | Robot's link (`Item::Link`) under the Robot entry. It is re-found by name on a reload, which bumps the revision. |
| `cad/selection/shared.rs` | CAD's items under the CAD entry (`Shared`, `View`, the `CadSelection` system param, `follow_tree`). The RoboCAD echo state (`remote_selection`, `remote_mode`, `selection_pushed_at`, `selection_again`, `published_selection`, `selection_read`) stays in `CadDocument`. |
| `inspect_view/projection.rs` | Inspect's items shown on the scene (`SpatialScene::shown`, a display projection only) and re-checked after a reload. `inspect::select` is Inspect's one adapter, used by REST `select`/`display`, clicks, Escape, the link and notes. |
| `lesson/selection.rs` | A lesson pick shared as the Build document's instance (`share`), and dropped when the Build selection moves elsewhere (`follow`). |
| `annotations/` | The one thread service: `ThreadSource` (an anchor adapter over `sim_annotate::Anchor`), `ThreadOp` (create, reply, post, edit, delete comment, resolve, delete, retitle, link, pin, undo, redo), `apply`, which lowers an op to one `ThreadCommand`, validates and commits it, and the shared helpers. |
| `ui_kit/threads.rs` | The one thread panel (replaces `annotate.rs`): `Host` (with `warning`, a thread's truthfulness line), `anchors`, `list`, `messages`, `card`, `card_with` (an open card with a comment menu and the host's rows), `composer`, and the Open / All / Resolved filter (`Shown`, `filter_row`). |
| Adapters | `notes.rs` + `notes/panel.rs` + `notes/compose.rs` (Inspect notes: `NoteAnchor`, `InspectNotes`, format `sim_inspect::annotations` v1/v2, the panel's `NotesUi` drafts), `builder/discussion.rs` (`SystemThreads`, `sim_system::display`), `lesson/threads.rs` (`LessonThreads`, lesson sidecars), `cad/threads/source.rs` (`CadThreadSource`, RoboCAD's threads over its REST routes; `request_on` is the one RoboCAD call for a command on an existing thread), `robot/threads/` (`RobotCadThreads`: the robot's CAD source's RoboCAD threads, mapped to links by CAD node id; never written by Rust, changes go through `request_on`). Each keeps its own file format and I/O. |

### Decisions

- **Selection items carry `(DocumentId, revision)`; items of several
  documents coexist.** Each mode's selection survives a switch, as before.
  A replaced document's items are forgotten. *Rejected:* one document's
  items at a time, which would lose Build's selection on a visit to
  Inspect. *Revisit if* cross-document selection is needed.
- **No REST command of its own for `SelectionAction`.** Each mode's
  existing command (`select`, `display`, `system_select`, the builder
  `system_ui` rows and picks, robot `system_ui` `link:<i>` /
  `clear_selection`, `cad_select` and its siblings) is the adapter. It
  checks the mode's own rules, then calls `Selection::apply` in its
  handler, so the answer reports the new state in the same frame. The
  registry cross-check test accepts an action type with no commands.
  *Rejected:* a new generic `selection` REST command, which would be a
  new user feature.
- **`system_select` refuses unknown names**, naming them, as its help text
  already documented (found by reading).
- **Builder items are instance names at the builder's level**, as
  `Builder.selected` held them. Lesson picks are stored the same way
  (`builder.instance_for_component`), while `learn.picked` keeps the full
  path for the lesson page.
- **CAD sub-body items keep the revision they were picked at.** Body items
  are restamped on a new tree, and items naming a node absent from a
  current tree are dropped and named in the status line (kept while the
  tree is behind RoboCAD's revision). 3D picks send `picked_at`, the shown
  revision; REST cannot set it. CAD's existing stale-revision and
  in-flight refusals are unchanged.
- **The RoboCAD echo stays in `CadDocument`.** An adopted RoboCAD
  selection is recorded as published, so it is never pushed back, unless
  a change was already pending from the same snapshot (a prune), which is
  then pushed once. `publish_changes` runs in JobResults, after Actions,
  so a change from any writer is pushed once, one push at a time.
- **The picker's index check uses `picker_revision`**, a key of its own on
  each `picker:<mode>:<n>` control: the controls answer's `ui_revision` is
  the mode's own, which generic clients send back. *Rejected:* checking
  `ui_revision`, which refused every valid entry (found in review).
- **Not document items:** the selected discussion thread, gait-lab entry,
  calibration trial and planar joint are list choices and stay where they
  were.
- **Annotation file I/O stays where it was off the UI thread.** The two
  sidecars (Inspect notes, lesson notes) keep `sim_annotate::store::Store`,
  the shared crate's own background worker with locked multi-process
  transactions and idle re-reads. Its stores are created in the loaders'
  `jobs` (inspect loader, lessons loader). System discussions are saved
  with the system document by the builder's save path. *Rejected:* moving
  `Store`'s edits onto `Pool::Io` jobs, which would need a second edit path
  for the shared crate's transactions. *Revisit if* `Store` moves into
  `jobs`.
- **Inspect notes are full threads** (cad-annotation-parity, 2026-10-02,
  replacing "Inspect notes are shown as one-message threads"). A note's
  text is the thread's first comment (no author or time; it is edited,
  never deleted on its own: "delete the note to remove its text"), its
  replies are `sim_annotate::Comment<Link>` and it can be resolved. The
  sidecar (`sim_inspect::annotations`) reads versions 1 and 2 and writes
  version 2 only while a note, or a command on the undo/redo stacks,
  carries replies or a resolved flag, so an untouched version-1 file is
  written back unchanged and older viewers still read it. A newer version
  is refused before the typed parse, naming the path and the version
  (`Revisioned::check_raw`, called by `sim_annotate::store::read`); every
  struct keeps `deny_unknown_fields`. Replies, reply edits and deletes and
  resolve are `notes::Command`s applied by the Store's worker, each with a
  whole-note undo step, submitted without a revision guard (they commute);
  edits of the note itself keep the guard. Saved views and navigation stay
  `notes::Command`. `as_thread`/`as_note` are lossless both ways. *Rejected:*
  replacing the sidecar with `sim_annotate`'s thread document (a migration
  of every existing file). *Revisit if* keeping the two formats in step
  costs more than a one-time migration.
- **Small visual changes:** Inspect notes render Markdown; the builder
  composer hint gains "· Esc cancels"; a missing anchor chip shows its
  label.
- **The builder highlight is re-projected without recompiling the
  scene.** It is re-projected on a selection change, on entering Build,
  and after a compile, but not in Lessons, where the lesson page draws its
  own. A discussion's exact-part highlight is kept (its `seen_selection`
  is recorded).

### Cross-mode annotation behaviour (cad-annotation-parity, 2026-10-02)

Every mode that shows a model takes comment threads through the one
service (`annotations::apply`) and draws them with `ui_kit/threads.rs`.
Phenomena and Place have no model nodes and show no threads (decision of
this batch; revisit if comments on exhibits or scanned places are wanted).
All of it is **by reading, unexecuted**: nothing was built, tested or run.
Paths are under `crates/sim-spatial/src/` unless named.

| Operation | CAD (RoboCAD's threads) | Build (system discussions) | Inspect (notes sidecar) | Lessons (lesson sidecar) | Robot (the CAD source's threads) |
|---|---|---|---|---|---|
| Create | Annotate (N), a face click, Post: `cad/threads/ops.rs:210` → `create` `:384` → `CadThreadSource::commit` `cad/threads/source.rs:423` → `POST /threads` | `builder/discussion.rs:320` (`ThreadOp::Create`) | `notes/panel.rs` "Annotate selected parts" → `notes::Command::PutNote` (a note, not a `ThreadOp`) | `lesson/threads.rs:70`, `lesson/actions.rs:560` | Refused: `request_on` (`cad/threads/source.rs:475`) says a new thread is placed in CAD mode. *Why:* a pin needs RoboCAD's faces at the shown revision, which only CAD mode reads. |
| Reply | `ops.rs:211` → `request_on` → `POST /threads/{id}/comments` | `builder/discussion.rs:330` | `notes.rs:310-312` → `InspectNotes::commit` `notes.rs:169` → `AddReply` | `lesson/threads.rs:66` | `robot/threads/act.rs:254` → `RobotCadThreads::commit` `robot/threads/mod.rs:336` → `request_on` → a job re-checking path, document id and revision (`mod.rs:362`) → `Request::send` |
| Edit a comment | `ops.rs:234` → `PATCH /comments/{id}` | `builder/discussion.rs:335` | `notes.rs:316` (the note's text → `PutNote`, a reply → `EditReply`) | `lesson/threads.rs:62` | `robot/threads/act.rs:259` (as Reply) |
| Delete a comment | `ops.rs:262` → `DELETE /comments/{id}`; the last one is refused as RoboCAD refuses it | `builder/discussion.rs:337` | `notes.rs:317` → `DeleteReply`; the note's own text is refused by name | `lesson/actions.rs:564`, `lesson/handler.rs:243` | `robot/threads/act.rs:263` (as Reply, same last-comment refusal) |
| Resolve / reopen | `ops.rs:288` → `PATCH status`; the filter `cad/threads/dock.rs:156`, the pin hidden `cad/threads/pins.rs:88` | `builder/discussion.rs:338`; "Open notes / All notes" toggle | `notes.rs:318` → `Resolve`; `notes/panel.rs:162` and the filter `notes/panel.rs:285` | `lesson/handler.rs:249`; open-only toggle `lesson/ui/margin.rs:86` | `robot/threads/act.rs:270`; the kit filter |
| Delete a thread | `ops.rs:277` → `DELETE /threads/{id}` | `builder/discussion.rs:332` | `notes::Command::DeleteNote` (`notes.rs:169` maps `DeleteThread`) | `lesson/handler.rs:253` | Not offered. *Why:* deleting a part's comment thread is a CAD edit with RoboCAD's undo; Robot mode has no undo for it. Open in CAD. |
| Part link | `[label](part:ID)` in a body: `ops.rs:341` (`PartLink`) selects the node | Inline links `builder/discussion.rs:330` (`inline_links`) | Note links (`notes::Link`), followed by `FollowLink` (now also a reply's) | Scene anchors `lesson/ui/margin.rs:25` | A part link or chip to a member selects the containing link: `robot/threads/panel.rs:90-95` |
| Select from a thread | Open selects its part; Show on model `cad/threads/isolation.rs:146` | `builder/discussion.rs:260` (`Picked::set`) | `notes.rs` `SelectNote` → `inspect::select` | `lesson/selection.rs` (share) | `robot/threads/panel.rs:77` → `picked::select` (`Item::Link`); an unmapped thread selects nothing |
| Pin / place on the model | Numbered pins, amber current / blue others: `cad/threads/pins.rs:88-99` | Markers `builder/markers.rs` | Guide boxes `notes/panel.rs:319` | None (text and scene anchors). *Why:* a lesson note is on text or a scene time. | None; the selected link is highlighted. *Why:* the pin is in RoboCAD's frame (mm), and the export carries no CAD-to-model transform for it. *Revisit* with the export's frame record. |
| Truthful anchor | RoboCAD's `anchor_status` texts (`cad/threads/controls.rs`, `attachment`) | Missing targets marked (`Target::missing`) | `Host::warning` on a note whose parts are gone | Missing anchors marked | `Host::warning`: "Not on any link of this export: …", and on every thread when `cad_link` is not Current (`robot/threads/mod.rs:61`, `:116-127`) |
| Persistence | RoboCAD's document (`.rcad` manifest), saved by `cad_file` save or autosave; the viewer never writes it | The system file, with the builder's save | The `*.annotations.json` sidecar through `sim_annotate::store::Store`'s worker | The lesson sidecar through the Store | RoboCAD's document only; Rust never writes a `.rcad` |
| Undo | RoboCAD's undo (`cad_undo`, one step per thread edit); `ThreadOp::Undo` is refused with `UNDO_IS_ROBOCADS` | The system's undo | Sidecar inverse commands (`sim-inspect/src/annotations.rs:309`) | Sidecar inverse commands | Refused with `UNDO_IS_ROBOCADS`: undo in CAD mode |
| Remote refresh | `cad/threads/read.rs:129` re-reads at each new RoboCAD revision; the draft is kept | The builder's file watch `builder/background.rs:42` | The Store's idle re-read; a draft settles only on its own result (`notes/compose.rs`) | The Store's idle re-read | A 2 s probe while shown (`robot/threads/read.rs:22`, `:174`); the draft is kept |
| When it can't act | Refused by name when stale, in flight or disconnected | — | Refused by name (unknown note or reply, the note's text) | — | "<file> is not open in CAD mode: open it there to reply", with **Open in CAD** (`robot/threads/act.rs:130`), whose switch request carries the thread to show (`ModeSwitch::reveal`); the switch installs it in `cad::threads::RevealThread` (state, not a message) only when accepted, and it stays until CAD mode has read it (`cad/threads/read.rs:243-277`) |

Remaining differences, each justified in its cell: Robot creates no
threads, deletes none, has no pins and no undo of its own. CAD's undo is
RoboCAD's. Lessons have no pins. Build and Lessons keep their own open-only
toggles rather than the three-way kit filter (not migrated in this batch).

### Found by reading and fixed

- `notes::update` marked `SpatialScene` changed every frame, so the
  systems that skip an unchanged scene ran every frame.
- Builder Resolve indexed `threads[&id]` and could panic on a thread
  deleted elsewhere.
- In review (five reviewers by area):
  - **CAD:**
    - a struct literal in an `if let` chain would not compile;
    - dropped items were never named;
    - an adopted read hid a same-snapshot prune from RoboCAD.
  - **Picker:** the `ui_revision` collision.
  - **Inspect:** the link exchange ran before the launch's `--select` was
    adopted.
  - **Build:**
    - entering Build did not re-project;
    - a rebuild in Lessons projected over the lesson highlight;
    - a discussion's part highlight was overwritten;
    - a new sandbox builder kept the old names;
    - dropped names were not shown.
  - **Lessons:** a `lesson_notes` `submitted` could become null.
  - **Notes:** a duplicate Inspect selection adapter.

### Reading trace (Build click → Inspect → REST select → note → reload)

1. **A click on a component in Build.** `inspect_view/scene.rs`
   `pick_part` writes `BuildAction::PickPart`. The builder's apply system
   calls `click_part` with a `Picked`, which writes
   `SelectionAction::set(build_doc, [Item::Component{name}])` through
   `Selection::apply`. `picked::track` (SimSync) sees `Selection.changed`
   move and projects the instance's parts onto `scene.shown`, so
   `update_parts` paints them.
2. **Switch to Inspect.** `app::switch::handle` → `prepare` → `enter`.
   `leave_builder` (OnExit Builder scope) parks the builder scene on the
   Build entry (`DocumentRegistry::park`). `arrive` (OnEnter Inspect)
   opens or unparks the Inspect entry through `sources::open`. The Build
   item stays in `Selection` under the Build id.
3. **REST `select {"target":{"kind":"components","ids":["x"]}}`.**
   `InspectAction::Select` → `inspect::handle` → `inspect::select`. It
   checks with `target.resolve` (the same refusal text as before), applies
   `SelectionAction::set(inspect_doc, target_items(..))`, projects to
   `shown`, and `state.selection` reports `selection.target(inspect_doc)`.
4. **A note on the selection.** The notes panel's New
   (`notes/panel.rs` `clicks`) reads `selection.target(inspect_doc)` as
   the note's targets and writes `InspectAction::Annotations`.
   `notes::api` → the `InspectNotes` adapter → `annotations::apply` →
   `Store::submit`, written by the store's worker to
   `<description>.annotations.json` in the unchanged format.
5. **Reload keeps the note.** `viewer_mode {mode: inspect, path}` with the
   same assembly loads on a Compute job (`prepare`, which connects the same
   sidecar path). `arrive` → `sources::open` sees the same `Source::Assembly`:
   a reload, same id, revision + 1. `projection::project` re-checks the
   items (restamped if the id still exists, otherwise dropped and named),
   and the new store reads the sidecar, so the note is listed again.

### Tests (windowless)

- **Selection core** (`selection/tests.rs`): the ops, a stale-revision
  refusal by name, re-checks that restamp, keep or drop (links re-found
  by name), the apply system answering REST, and the inspection-target
  round trip.
- **Same Selection from the UI path and from REST:**
  - Build: `builder/picked/tests.rs`;
  - Robot: `robot/actions/tests.rs`;
  - Inspect: `inspect_view/tests.rs` `click_parts_row_and_rest_select_make_the_same_shared_selection`;
  - CAD: `cad/selection/tests.rs` `cad_select_and_a_selection_action_give_the_same_selection`.
- **Stale and dropped items:**
  - CAD `picks_carry_their_revision_and_a_new_tree_rechecks_them`;
  - Inspect `a_reload_drops_items_the_assembly_no_longer_has`;
  - builder: a removed instance is dropped by name.
- **CAD echo:** `cad/selection/tests.rs` `the_selection_echo_does_not_loop`.
- **Annotation formats** (`annotations/tests.rs`): the committed
  `examples/systems-viewer/evidence/rest-api/*/discussion.annotations.json`
  go note → thread → note; `lessons/motor-torque-speed/lesson.md.annotations.json`
  round-trips as JSON; system discussions round-trip in a system document;
  add, reply and resolve through the service match the hand-built commands.
- **Registry:**
  - `document/tests.rs`: reload ids and revisions;
  - `app/tests.rs` `a_reopened_document_keeps_its_id_and_a_new_one_gets_a_new_id`
    and `a_parked_inspect_scene_comes_back_through_the_registry`.
- **Picker:** `app/picker_tests.rs` `a_stale_picker_index_is_refused_naming_both_revisions`.

The Lessons share/follow and the link exchange have no windowless test:
the lessons need a full `Builder`, and `SelectionClient::connect` starts
a worker on a session directory.

### Verification checklist

- [ ] `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- [ ] `cargo test -p sim-spatial --lib --bins` (including
  `app::tests::source_files_stay_small`, the registry cross-check and
  the copy guard).
- [ ] `cargo test -p sim-inspect`.
- [ ] `cargo test -p sim-system`.
- [ ] `grep -rn "selected: BTreeSet<String>\|pub selected: Option<usize>\|pub selection: Vec<SelectionItem>\|pub selection: SelectionTarget" crates/sim-spatial/src`
  finds nothing.

## CAD physical properties (2026-10-01)

*Batch cad-physical-inspect (default order item 7, §9 phase 1; §9 "Later
CAD epics" 5). Done pending verification: written and reviewed by reading
in commits 3fb34225 (client and `api.py` routes) and f26842fa (the native
viewer), plus review fixes committed in 697a15c1; nothing was compiled,
tested or run in the batch.* It brings RoboCAD's physical side into CAD mode: the
materials panel and its dialogs; the inspector's physical rows (colour,
joint editor, joint physics overrides, results line, exact measurement);
the Robot panel with its tools, dialogs, motor library and glyphs;
results, identification, actuator profiles and the stress overlay; and
physical export with the live link into Robot mode in the same window.
RoboCAD's command layer still does every edit. The ledger rows are in
[docs/cad-parity.md](../cad-parity.md) (82 rows: 64 `done-by-reading`, 18
`deliberately different`; none open; the totals are in its Counts), and
the side-by-side steps are in [docs/cad-checklist.md](../cad-checklist.md)
Part G. Paths are relative to `crates/sim-spatial/src/` unless they name
another crate.

### Shape

| Module | What it owns |
|---|---|
| `crates/sim-runtime/src/cad_client/robot.rs` | Reads and REST-shaped writes: `CadClient::robot`, `robot_exact`, `motors`, `actuator_profiles`/`set_actuator_profiles`, `sensors`/`add_sensor`, `cables`/`add_cable`, `battery`/`set_battery`, `control`/`set_control`, `uncertainty`/`set_uncertainty`, with tolerant types (`RobotSummary`, `MotorSpec`, …). |
| `crates/sim-runtime/src/cad_client/robot_ops.rs` | The `POST /ops/…` wrappers with RoboCAD's signatures: `add_joint`, `set_joint`, `rename`, `connect_fixed`, `add_motor`, `mount_motor`, `attach_motor`, `set_ground`, `infer_joints`, `set_robot_setting`, `configure_robot`. |
| `crates/sim-runtime/src/cad_client/physical.rs` | `materials`, `add_material`, `results`, `load_results`, `results_nodes`, `apply_identification`, `physical_model(flex, planar)` (never `path=`), `set_material`, `set_color`, `set_material_props`, `set_joint_physics`. |
| `cad/robocad/api.py` (gap routes) | `GET /results/nodes` (`Service.results_nodes`: path, loaded, stale, provenance, `results_margins` per node with print-study blocks left out, and each node's results block with its material's yield strength; read-only, 405 otherwise) and `GET /physical?…&planar=1` (headless `export_physical_model(planar=Plane.xz())`, desktop `export_snapshot(planar=True)`; unchanged without it). The module docstring said simrobot v3; `physical.py` `SCHEMA_VERSION` is 4, and it now says v4. |
| `cad/robot/mod.rs`, `data.rs` | `CadAction::CadRobot` (`cad_robot`) and the robot reads: `data.rs:sync` runs one `Pool::Dedicated` job per (generation, shown revision) over `/robot`, `/results/nodes`, `/sensors`, `/cables`, `/battery`, `/control`, `/uncertainty`, `/actuator-profiles` (and `/motors` once per generation), kept on `CadDocument.robot.data`. |
| `cad/robot/panel.rs` | The Robot section of the right dock (`Part::Robot`): `summary_line`, `view` (tree and issues), `row`, `double_click`, `margin_text`, `buttons`. |
| `cad/robot/tools.rs`, `tools_click.rs`, `tools_library.rs` | The motor and joint tools (`motor_pick`, `joint_pick`; the face from `tools_click.rs:click` through `CadMeshes::face_at` at the shown revision), `validate`/`verdict`, and the motor library panel (`tools_library.rs:draw`). |
| `cad/robot/glyphs.rs` | Joint glyphs, motor shaft axes, sensor triads and cable arcs (`glyph_lines`, `draw`), cached by (generation, read revision). |
| `cad/materials/` | The Materials section of the right dock (`Part::Materials`): `mod.rs` (`list`, `matches`, `apply`, `submit`, `CadAction::CadMaterials`, `cad_materials`), `panel.rs` (`draw`, `input`), `form.rs` (`new_form`, `properties_form`, `submit`). |
| `cad/inspector/node.rs`, `sections.rs`, `rows.rs`, `entry.rs`, `exact.rs`, `refresh.rs`, `physical_edit.rs` | The inspector's physical rows: `rows.rs` (`row`, `joint`, `results_line`), `physical_edit.rs` (`controls_of`, `joint_override`, `handle_physical`; `cad_inspector`), `exact.rs` (`start`, `settle`, `sync`: the exact measurement), `refresh.rs:sync` (`GET /physical?flex=0` again per revision), with `node.rs`, `sections.rs` and `entry.rs` laying the rows out. |
| `cad/ops/catalogue/robot.rs`, `robot_args.rs`, `robot_form.rs`, `invoke.rs` | The robot entries of the op catalogue (`robot.add_motor`, `add_joint`, `joint_dialog`, `infer`, `assign_motor`, `fixed`, `ground`, `add_sensor`, `add_cable`, `power`, `ops.set_joint`, `ops.configure_robot`); `robot_args.rs:build` turns a form into RoboCAD calls (`EditJoint`: `set_joint` then `rename`; `power`); `robot_form.rs` (`picks`, `seed`, `precheck`, `note`, `open_preset`); `invoke.rs` starts and ends the tools. |
| `cad/results/` | `mod.rs` (`handle`: load, identify, overlay, print overlay, export, link; `profiles`; `CadAction::CadResults`, `cad_results`), `export.rs` (`request`, `start`, `poll`, `cancel`, `write_model`), `link.rs` (`toggle`, `saved`, `after_write`, `follow`, `receive`), `forms.rs` (the path forms), `overlay.rs` (`paint`). |
| `crates/sim-domain-robot/src/stress_results.rs` | `link_colours`: the one stress colouring rule, used by Robot mode (`robot/stress.rs`) and the CAD overlay. |

### Decisions

- **Robot reads are one job per shown revision.** `cad/robot/data.rs:sync`
  reads every robot route together on `Pool::Dedicated` and keeps the
  answers on `CadDocument.robot.data`; `load_results` and
  `apply_identification` invalidate it because they do not move RoboCAD's
  revision. *Why:* the panel, the glyphs, the dialogs' picks and the
  inspector's results line need a consistent snapshot, and none of them
  may block the UI thread. *Rejected:* a read per panel, which could show
  a summary and margins from different revisions. *Revisit if* RoboCAD
  bumps its revision on results loads, or one read becomes slow enough to
  split.
- **One edit helper.** `cad/actions.rs:edit_at(doc, call, began, label,
  work)` is `CadDocument::commit_refusal(began)` then `actions::edit`;
  every new edit (materials, inspector rows, results) goes through it,
  and catalogue runs use `prepare`'s `commit_refusal` then `edit`. *Why:*
  one place refuses by name an edit in flight, a lost connection, a shown
  tree behind RoboCAD's or a revision that moved since the form or row was
  read. *Rejected:* a check in each panel, which could differ.
- **Runtime choices in forms.** `FieldKind::Pick { source }` with
  `FormRow.picks` fills the robot dialogs' combo boxes at open time
  (`cad/ops/robot_form.rs:picks`: motors from `GET /motors` only, bodies,
  joints). Selection-seeded forms always reseed when opened again; Edit
  joint and Battery/control refuse until the description is current at the
  shown revision (`precheck`). *Why:* RoboCAD's dialogs fill their
  `QComboBox`es from the document; nothing is computed or filled in.
  *Revisit if* the kit gains a searchable picker.
- **Robot and Materials are sections of the right dock** (`Part::Robot`,
  `Part::Materials` in `cad/panel.rs`), beside the inspector's sections.
  *Why:* RoboCAD docks both in its right dock area under the properties
  panel, tabbed together (ui/app.py:182-195), and the native right dock
  already holds the inspector's sections. *Rejected:* separate floating panels for each.
  *Revisit if* the dock gets too long to use.
- **One stress rule.** `sim_domain_robot::stress_results::link_colours`
  (log scale over 3 decades, blue at 0.1 % of yield → red at yield) is
  used by Robot mode (`robot/stress.rs`) and by the CAD overlay
  (`cad/results/overlay.rs:paint`, on display-mesh vertex colours only).
  *Why:* the same results must look the same in both modes of one window.
  *Rejected:* copying RoboCAD's linear rule into CAD mode, which would
  make CAD and Robot disagree. *Revisit if* the user prefers RoboCAD's
  linear scale; change it in `link_colours` for both.
- **Exports are written natively.** `cad/results/export.rs` runs
  `GET /physical` (never `path=`) on a `Pool::Dedicated` job and writes the
  file atomically here; one at a time, and the live link's request queues
  latest-wins. Leaving CAD is refused while an export runs or is queued.
  *Why:* a job can be cancelled before the write and reports progress;
  RoboCAD's `path=` would write from inside RoboCAD with no cancel.
  *Revisit if* RoboCAD's export becomes cancellable. Since rover-rest-flow
  (by reading, unexecuted): a third kind, `rigid` (`GET /physical?flex=0`,
  the fixture scripts' model; `ExportKind::Rigid`); the job stamps
  `source.cad_sha256` only when RoboCAD's document is the saved, absolute,
  unchanged file for the whole export (`cad_client::stamp_saved_source`),
  else it records why not; each export has a `seq`, and outcomes land in
  `cad_state.results.exports.last` and the bounded `recent`
  (`docs/rover-checklist.md` RV-03).
- **Live link in the app.** `cad/results/link.rs`: no process is started;
  the first written export switches this window to Robot mode
  (`WindowAction::Switch`), later saves re-export and bump the registry's
  Robot entry (same id), which Robot mode reads on its next entry.
  *Rejected:* starting `sim-spatial --robot` as simbridge does, which
  would be a second window. simbridge's watch-and-run stays for RoboCAD's
  own window.
- **Exact measurement** is one `GET /nodes/{id}` per selected node on a
  `Pool::Dedicated` job, combined as RoboCAD's `analysis.selection_properties`,
  60 s limit, cancelled by any edit or selection change
  (`cad/inspector/exact.rs`). A cancel stops waiting only: the request
  already sent finishes in RoboCAD. *Rejected:* a RoboCAD child process,
  which only RoboCAD's window can start.
- **Keys.** Ctrl+Shift+J is bound to `robot.add_joint`; Ctrl+Shift+M stays
  Select Same Material and `robot.add_motor` is unbound (see Key clashes).
- **Known residual (fixed by cad-print).** A REST `cad_run ops.set_joint`
  with only some params sent RoboCAD's dialog defaults for the rest (the
  window's form always seeds every field from the joint). Since cad-print
  the missing params are the joint's current values
  (`cad/ops/robot_form.rs:fill_from_joint`, from `cad/ops/mod.rs:prepare`),
  refused by name when they are not known at the shown revision.

### Key clashes

| Key | RoboCAD | Native | Why |
|---|---|---|---|
| Ctrl+Shift+M | listed on `robot.add_motor`, but pressing it runs Select Same Material | Select Same Material; `robot.add_motor` unbound (menus, palette, the Robot panel's button, `system_ui`) | keeps RoboCAD's live behaviour |
| Ctrl+Shift+J | listed on `robot.add_joint`, never bound | starts the joint tool | nothing else in CAD mode reads it; USER_GUIDE.md:375 documents it |

The Space radial is unchanged: RoboCAD's view and selection radials
(ui/app.py:1095-1101) have no robot tools. The full table is
`cad/keys.rs`' module doc (`cad/surfaces/registry.rs` marks which keys
are `bound`).

### Reading trace (materials → Apply → joint from selection → add motor → export → Robot mode)

1. **Materials panel.** `cad/materials/panel.rs:draw` lists
   `cad/materials/mod.rs:list` (the `/doc` materials), filtered by
   `matches`. A click or double-click is read by `panel.rs:input`, which
   writes `CadAction::CadMaterials` (`cad_materials`).
2. **Apply.** `cad/materials/mod.rs:handle` → `apply`: the shared
   selection's CAD nodes, refused by name when empty, then
   `cad/actions.rs:edit_at` (`commit_refusal`, then `edit`) →
   `CadClient::set_material` (`POST /ops/set_material`), one undo step.
3. **Joint from selection.** The Robot panel's "Joint from selection…"
   (`cad/robot/panel.rs:buttons`) invokes `robot.joint_dialog`
   (`cad/ops/invoke.rs:invoke`); `cad/ops/robot_form.rs:seed` presets
   it from the selection and the active plane, and `picks` fills the
   combo boxes. OK → `cad/ops/form.rs:submit` → `cad/ops/mod.rs:run` →
   `prepare` (`commit_refusal`; `cad/ops/args.rs:build` →
   `cad/ops/robot_args.rs:build`) → `start` → `robot_args.rs:send`
   (`add_joint`), and `started` records `selects_created` so the new
   joint becomes the selection (`cad/sync/mod.rs:finish_edit`,
   `receive`).
4. **Add motor from library.** `robot.add_motor` opens "Add motor" with the
   motors from `GET /motors` (`cad/robot/data.rs:sync`); with the motor
   tool, `cad/robot/tools.rs:motor_pick` takes the face clicked
   (`tools_click.rs:click` → `CadMeshes::face_at`) and runs the entry
   (`cad/ops/mod.rs:run_entry`) → `add_motor`.
5. **Export physical.** `sim.export_physical` → `cad/results/mod.rs:handle`
   → `cad/results/export.rs:request` (refused while one runs) → `start`: a
   `Pool::Dedicated` job calls `CadClient::physical_model(flex, planar)`
   and `write_model` writes it atomically; `poll` lands it.
6. **Robot mode reloads.** With the live link on (`cad/results/link.rs:toggle`),
   each settled save (`saved`) requests the link's export; when it lands,
   `link.rs:receive` → `after_write`: the first time it sets `switch_to`,
   written as `WindowAction::Switch` (`switch_action`) for
   `app::switch::handle`; later it `follow`s: the registry's Robot entry is
   bumped (same id, revision + 1), and Robot mode reads the file when next
   entered.

### Tests (windowless)

- `cad/robot/panel_tests.rs`: `a_row_press_and_rest_cad_select_give_the_same_selection`,
  the summary/tree/margins text, `a_joint_rows_double_click_opens_edit_joint`,
  the controls round-tripping through REST.
- `cad/robot/tools_tests.rs`, `cad/robot/glyphs_tests.rs`: motor shaft
  direction, joint axis, a face pick at another revision refused,
  validation, the library rows, glyph shapes and cache key.
- `cad/inspector/physical_tests.rs`: the exact measurement cancelled by a
  selection change and by an edit (each saying why), its combined result,
  joint physics payloads, override marks, the results line.
- `cad/materials/tests.rs`: rows and search, controls through REST, the
  New dialog, properties sending only changes.
- `cad/results/tests.rs`: `the_live_link_requests_the_robot_reload_through_the_switch_and_registry`,
  `the_live_links_first_switch_is_kept_when_the_switch_would_be_refused`
  (the refusal path), the export queue, the atomic write,
  `the_cad_overlay_and_robot_mode_share_one_colour_rule`.
- `cad/surfaces/tests.rs`: `the_physical_rows_run_native_actions`,
  `every_robot_tool_row_runs_in_the_catalogue`.
- `cad/ops/robot_tests.rs`: every robot entry, picks, presets, Edit joint's
  rename, the power dialog's order, prechecks, reseeding, the created
  node becoming the selection.
- `crates/sim-runtime/src/cad_client/robot_tests.rs` and
  `physical_tests.rs`: tolerant reads and RoboCAD's signatures, including
  `physical_model_asks_for_flex_and_planar_and_never_a_path` and
  `results_nodes_reads_margins_and_blocks_per_node`.
- `cad/tests/test_api_physical_routes.py`: `/results/nodes` empty, with
  margins, blocks and yield strength, print-study blocks left out of the
  margins; the planar hint only when asked.

### Found by reading and fixed

- `sim_domain_robot::stress_results`: a hotspot's cells and stresses
  fell out of step when a stress was null.
- Edges → faces converted an edge picked at an older revision against the
  new topology; it is now refused by name, and box-select and Alt+click
  menu items carry their topology revision too
  (`cad/selection/mod.rs:edges_to_faces`, `box_items`, `menu_revision`).
- `switch_blockers` and the sketch refusals told people to use REST
  commands (`cad_save`, `cad_refresh`) in window text.
- The power dialog could reset joint targets when the control read
  failed (it is now refused until the reads are current).
- `api.py`'s docstring called the physical export simrobot v3; it is v4
  (`physical.py` `SCHEMA_VERSION = 4`).

### Deliberately different (summary)

18 rows, each with its reason in the ledger: the exact measurement (a job,
not a child process; cancel stops waiting); joint physics values the model
lacks shown empty, not 0.0; the colour as an "r, g, b" field; no
drag-and-drop of a material; only changed engineering properties sent,
with no copy of RoboCAD's `_ENG` table; glyph sizing, dots and no pose
hiding; the stress overlay's shared log rule; the motor tool's dialog
staying open beside the view; the Robot panel's summary (no "(n s run)"),
tree (no glyphs; Detail and Margin as lines) and issues ("Error:" and
"Warning:"); motors in id order; validation in the status line and issue
list; the motor library as a panel; the power dialog's JSON targets and
precheck; export as a native job with atomic write; the live link inside
this window; and the key bindings.

### Verification checklist

Nothing above was compiled or run. To do in the verification pass:

- [ ] `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- [ ] `cargo test -p sim-spatial --lib` (including
  `app::tests::source_files_stay_small` and the registry cross-check).
- [ ] `cargo test -p sim-runtime cad_client`.
- [ ] `cargo test -p sim-domain-robot`.
- [ ] `cd cad && .venv/bin/pytest -q tests/test_api_physical_routes.py`
  (then the whole RoboCAD suite).
- [ ] In the window, [docs/cad-checklist.md](../cad-checklist.md) Part G
  side by side with RoboCAD: the materials panel and dialogs, the
  inspector's physical rows and exact measurement, the Robot panel's row
  click and double-click, each robot tool and dialog, the motor library,
  load results with the stress overlay, apply identification, both
  exports, and the live link opening Robot mode and reloading it after a
  save.

## CAD print (2026-10-01)

*Batch cad-print (default order item 7, §9 phase 1; §9 "Later CAD
epics" 6). Done pending verification: written and reviewed by reading in
commits 35ea6de0 (the skeleton: client, catalogue entries, the
`cad/print/` module and its wiring) and 17f90d08 (the parts, the
`api.py` gap route and the tests); nothing was compiled, tested or run in
the batch.* It brings RoboCAD's Print menu into CAD mode: the wall
thickness check and its red points, validation for printing, overhang
shading, the fastener hole and clearance offset tools, split for
printing, check strength, plan, whole or split, the assembly guide, test
coupons, RoboCAD's print jobs with progress and cancel, and the print
study's results in the stress overlay. RoboCAD stays the kernel: its
sampling, validation, split, strength, plan, assembly and coupon jobs run
there, and its command layer does every edit. The ledger rows are in
[docs/cad-parity.md](../cad-parity.md) (33 rows: 25 `done-by-reading`, 8
`deliberately different`; none open; one REST row added for the gap
route; the totals are in its Counts), and the side-by-side steps are in
[docs/cad-checklist.md](../cad-checklist.md) Part H. Paths are relative
to `crates/sim-spatial/src/` unless they name another crate.

### Shape

| Module | What it owns |
|---|---|
| `crates/sim-runtime/src/cad_client/print.rs` | The typed print client: `CadClient::print_registry` (`PrintRegistry`, printers and filaments in the registry's order through `Ordered`), `print_study`, `print_split_now`, `print_split_job` (`SplitRequest`, `background`), `print_start(kind, body)` for analyze, plan, assembly, coupons and strength_split (any other kind refused unsent), `print_jobs`, `print_job`, `cancel_print_job`, `thin_walls`, `validate_node`, and the Ops `clearance`, `fastener_hole` (`FastenerSpec`) and `print_split_op`; RoboCAD's choice lists `SPLIT_JOINTS`, `FASTENER_SIZES`, `FASTENER_KINDS`. Tests in `print_tests.rs`. |
| `cad/robocad/api.py` (gap route) | `GET /print/study` in `Service.print_request`: `revision`, `robot_settings["print_study"]` or null, and the split group ids in tree order; read-only (405 otherwise). Pytests `cad/tests/test_api_print_routes.py` (with the registry's order). |
| `cad/ops/catalogue/print.rs` | The Print menu's catalogue entries with RoboCAD's labels, prompts, defaults and refusals: `tool.fastener` (`Flow::PrintPick`), `tool.clearance`, `print.wall_check`, `print.validate`, `print.split`, `print.strength`, `print.plan`, `print.strength_split`, `print.assembly`, `print.coupons` (`Shape::Print(PrintCall)`), and the REST-only `ops.print_split`. |
| `cad/print/mod.rs` | `PrintState` on `CadDocument.print` (reset with the document), `PrintCall`, `Plan`, `build_plan` (from `ops::args::build`), `send` (from `ops::start`), `picks`, `seed`, `precheck` (the robot forms' fallbacks), `edit_answered` (from `sync::finish_edit`), `command_action` (`print.overhangs`, `print.jobs`), `CadAction::CadPrint(PrintArgs)` (`cad_print`: state, jobs, cancel, pick, clear) and `handle`. |
| `cad/print/checks.rs`, `thin_overlay.rs` | The wall check (`build`, `start_wall`, `land_wall`, `seed`, `clear`) and validation (`start_validate`, `land_validation`, `validation_lines`, `validation_status`) on `Pool::Dedicated` jobs landed by `receive`; the thin points drawn by `thin_overlay.rs:draw`, display only. |
| `cad/print/edits.rs`, `fastener_tool.rs` | Fastener hole and Clearance offset (`build`, `send` through `actions::edit_at`, `seed` with the remembered values); the fastener tool's face clicks (`fastener_tool.rs:click` through `CadMeshes::face_at`, `pick`). |
| `cad/print/studies.rs` | The registry read (once per generation) and the print study read (per shown revision) in `tick`; the six studies' `build` (RoboCAD's selection rules and texts, `expected_revision`) and `send` (one `edit_at` call each); `picks`, `precheck`. |
| `cad/print/jobs_tracker.rs` | `PrintJobTracker` on `CadDocument.print.jobs`: `edit_answered` adopts a started job, `tick` polls `GET /print/jobs`, `land` writes the progress line, `finish` the done texts and refresh, `cancel`, `blockers`. |
| `cad/print/jobs_panel.rs` | The Print jobs section of the right dock (`panel::Part::Print`): `show`, `draw`, `controls` (`cad:print:*`), `key`. |
| `cad/print/overlay.rs` | The print study's blocks in the stress overlay: `inputs` (called from `cad/results/overlay.rs:inputs_of`), `staleness`, `panel_line`, `RULE`. |
| `cad/display/mod.rs`, `section.rs`, `ui.rs` | `CadDisplay::overhangs` and `DisplaySetting::Overhangs` (`apply_display`: the build plate sets it, as RoboCAD); the tint `section.rs:overhangs` at `OVERHANG_DEG`; the toolbar chip. |
| `jobs/child.rs` | `jobs::open_local(path)`: the assembly guide and the coupon protocol's folder opened with the system opener. |
| `cad/robot/panel.rs`, `cad/ops/robot_form.rs` | Two cad-physical-inspect follow-ups: a Robot panel row press stamped with its description's revision (`select_action`), and a partial REST Edit joint filled from the joint (`fill_from_joint`, called from `cad/ops/mod.rs:prepare`). |

### Decisions

- **One poller on the document, per generation.** `PrintJobTracker`
  lives on `CadDocument.print.jobs` and starts over when the connection
  generation changes (`jobs_tracker.rs:reset`). *Why:* RoboCAD's jobs
  belong to one service; a restarted service has none of the old ones, and
  the document is already the thing reset per generation. *Rejected:* a
  separate resource, which would need its own reset rule.
- **`GET /print/jobs`, not `GET /print/jobs/{id}`.** One list poll is in
  flight at most, on `Pool::Dedicated`, every 0.5 s while a watched job
  runs or the Print jobs section is open (`jobs_tracker.rs:tick`). *Why:*
  one request serves every watched job and the section alike, and the
  per-job route's `wait` is useless: RoboCAD reads it from the body of a
  GET, which it never parses (api.py `print_request`), so it never
  waits. *Rejected:* RoboCAD's per-job 250 ms timer, which would be a
  request per job per tick. *Revisit if* RoboCAD reads `wait` from the
  query string.
- **Jobs start through `edit_at` and are adopted in `finish_edit`.**
  `studies.rs:send` makes each start one `actions::edit_at` call with
  `expected_revision` set to the revision the selection, the form and the
  study were read at, and notes the edit's sequence; when the edit
  answers, `sync::finish_edit` calls `print::edit_answered`, which watches
  the job RoboCAD returned. *Why:* the edit path already refuses by name an
  edit in flight, a lost connection or a moved document, and RoboCAD
  refuses a moved document too; a start is a RoboCAD call like any edit.
  *Rejected:* a separate start job, which would need its own refusals.
- **Split runs in the background, as RoboCAD's menu.** `print.split`
  sends `background: true` (`print_split_job`), so it is a job with
  progress and cancel like the others; the synchronous split is the
  REST-only `ops.print_split` (one undo step, no job). *Why:* RoboCAD's
  "Split selected for printing…" calls `split_job`.
- **Leaving CAD mode is refused only while connected.**
  `PrintJobTracker::blockers` names each running job this window started
  (`CadDocument::switch_blockers`). *Why:* a self-started RoboCAD stops
  when CAD mode closes, which would kill the job, and an attached one
  would finish unseen. Not connected, nothing can be confirmed or
  cancelled, so nothing is held.
- **The print overlay is uniform per part, through the shared rule.**
  `print/overlay.rs:inputs` turns a "print" block into one cell whose
  stress is the governing failure index (1 / safety factor) with yield 1,
  so `link_colours` colours the whole body, red at failure.
  *Why:* one colour rule for every result in this window (see "One stress
  rule" in CAD physical properties); RoboCAD's per-voxel field is a file in
  its run folder, and porting its sampling is out of scope. Staleness
  comes from the block's `cad_revision` (current until the document moves
  past the publishing step). *Revisit if* a route serves the voxel field.
- **Local files open through `jobs::open_local`** on a `Pool::Io` job
  (the assembly guide, the coupons' protocol folder). *Why:* nothing
  outside `jobs` starts a process; the opener is detached and reaped like
  `open_in_browser`, and a relative or missing path is refused by name
  before any process starts.
- **The cancel confirmation is an inline row.** "Cancel the running
  jobs?" with Yes and No is a row of the Print jobs section
  (`jobs_panel.rs:draw`), and `cad_print {op: cancel}` without `confirm`
  only asks. *Why:* the UI kit has no modal confirm, and an inline row
  keeps REST and the window on one action. Yes sends exactly one `DELETE`
  per running job.
- **The registry keeps its order.** `Ordered` (client) keeps RoboCAD's
  printers and filaments in the registry's order, and serialises back as
  an object. *Why:* RoboCAD's dialogs list them in that order and preselect
  the first; a map would reorder them.
- **One gap route, `GET /print/study`.** RoboCAD's handlers read
  `robot_settings["print_study"]` and find split groups by walking the
  tree; no route served either headless. The route (`api.py`
  `Service.print_request`) answers `revision`, the print study or null
  and the split group ids in tree order; it is read-only (any other
  method is 405) and is read per (generation, shown revision) in
  `studies.rs:tick`. Pytests: `cad/tests/test_api_print_routes.py`.
- **The wall check remembers its threshold.** The last threshold a check
  ran with presets the next form (`checks.rs:seed`); RoboCAD reopens at
  1.2 each time. *Why:* the other print dialogs (Fastener hole,
  Clearance) remember their values; this one is used the same way.
  Recorded as deliberately different.
- **The fastener form has a Point field.** "Point (mm; empty: the clicked
  point)" lets REST place a hole without a click; with a click, the point
  is the click's snap, else its hit (`fastener_tool.rs:pick`). *Why:* every
  window action must be reachable by REST with the same validation.
- **Validation has no open-edge check.** RoboCAD's desktop adds a
  tessellation open-edge count in `validate_for_export`; `GET
  /nodes/{id}/validate` answers only the kernel's report, so the viewer
  shows that and says so (`checks.rs:OPEN_EDGE_NOTE` in
  `cad_state.print.checks`). *Revisit if* the route serves the count.

### Reading trace (wall check → split with dovetail → plan progress → cancel → print overlay)

1. **Wall check.** Ctrl+W: `cad/keys.rs:keys` matches the registry row
   `print.wall_check` (`cad/surfaces/registry.rs`, `Native::Op`) and writes
   `CadAction::CadInvoke` (`keys.rs:358`) → `cad/ops/mod.rs:handle` (488)
   → `cad/ops/invoke.rs:invoke` (12, `Flow::Form` at 59): the form
   "Flag walls thinner than (mm):" opens, preset by
   `cad/ops/robot_form.rs:seed` (266) → `cad/print/mod.rs:seed` →
   `cad/print/checks.rs:seed` (355; the last threshold, else 1.2). OK →
   `CadAction::CadFormSubmit` → `cad/ops/form.rs:submit` (15) →
   `cad/ops/mod.rs:run` (525) → `prepare` (540) → `cad/ops/args.rs:build`
   (559; `Shape::Print` at 569) → `cad/print/mod.rs:build_plan` (125) →
   `cad/print/checks.rs:build` (200; the selected nodes, else
   `visible_bodies`) → `cad/ops/mod.rs:start` (581; `Built::Print` at 608)
   → `cad/print/mod.rs:send` (136) → `checks.rs:start_wall` (237): cached
   nodes answered at once, the rest read on one `Pool::Dedicated` job
   (`CadClient::thin_walls`). The landing: `checks.rs:receive` (425, after
   `sync::receive` in JobResults) → `land_wall` (302): cached, the points
   kept, RoboCAD's status line ("N thin region(s) under T mm"); the points
   are drawn by `cad/print/thin_overlay.rs:draw` (25) while their revision
   is shown (`checks.rs:drawn`).
2. **Split with dovetail.** Print ▸ Split selected for printing…
   (`print.split`): the form's "Printer:" lists the registry's printers
   (`cad/ops/robot_form.rs:picks` (84) → `cad/print/studies.rs:picks`
   (277), from the registry read in `studies.rs:tick` (329)), refused by
   `studies.rs:precheck` (293) until it is read; "Joints:" `dovetail`. OK
   → as in 1 to `cad/print/studies.rs:build` (188): `SplitRequest { node,
   printer, joint: "dovetail", expected_revision, background: true }` →
   `studies.rs:send` (260) → `cad/actions.rs:edit_at` (523) →
   `CadClient::print_split_job` on the edit's job; `studies.started` notes
   the edit. When it answers, `cad/sync/mod.rs:finish_edit` (525) calls
   `cad/print/mod.rs:edit_answered` (171, at sync 539) →
   `cad/print/jobs_tracker.rs:edit_answered` (188): the job is watched and
   polled at the next tick.
3. **Plan job progress.** Print ▸ Plan print settings and plates
   (`print.plan`) → `studies.rs:build` (the study read at the shown
   revision, with `expected_revision`) → `send` →
   `CadClient::print_start("plan", …)` → adopted as in 2. Then
   `jobs_tracker.rs:tick` (336) starts one `GET /print/jobs` poll every
   0.5 s → `land` (262): while the job runs the status line is
   `progress` (128), "plan: message (n %)", written only when it changes;
   when it ends `finish` (307): `done_text` ("plan: N plate(s), …"),
   `cad/sync/mod.rs:refresh` and the robot reads invalidated (the plan
   published a step).
4. **Cancel.** The Print jobs section's "Cancel running jobs…"
   (`cad/print/jobs_panel.rs:draw` (76), `controls` (53)) writes
   `CadAction::CadPrint { op: cancel }` → `cad/print/mod.rs:handle` (235)
   → `jobs_tracker.rs:cancel` (220): without `confirm` it opens "Cancel the
   running jobs?"; Yes (`confirm: true`) sends one
   `CadClient::cancel_print_job` per running job on one `Pool::Dedicated`
   job, then polls; the job lands "plan cancelled" (`finish`).
5. **Print overlay.** After Check strength publishes, the robot reads
   (`cad/robot/data.rs:sync` (160), `CadClient::results_nodes`) carry each
   part's "print" block from `GET /results/nodes`. With the overlay on
   (`print.overlay` → `cad/results/mod.rs:handle`, `ResultsOp::PrintOverlay`
   at 334), `cad/results/overlay.rs:paint` (127) asks
   `inputs_of` (59), whose print branch is
   `cad/print/overlay.rs:inputs` (46) → `cad/results/overlay.rs:cad_colours`
   (75) → `sim_domain_robot::stress_results::link_colours` → the body's
   vertex colours. The results panel adds `cad/print/overlay.rs:panel_line`
   (71) with `staleness` (56): "current", or "stale (computed at revision
   R, now M)".

### Tests (windowless)

- `cad/print/checks_tests.rs`: `floats_print_as_python_does`,
  `status_texts_are_robocads`,
  `build_takes_the_selection_else_the_visible_bodies_and_refuses_nothing`,
  `a_repeat_check_of_unchanged_nodes_sends_nothing`,
  `points_show_at_their_revision_and_clear`,
  `results_land_for_this_generation_only`,
  `overhang_shading_follows_the_build_plate`.
- `cad/print/edits_tests.rs`: `the_fastener_spec_is_robocads_dialog`,
  `a_hole_needs_a_point_and_a_face`,
  `clearance_calls_go_per_node_in_selection_order`,
  `the_forms_open_with_the_remembered_values`,
  `a_refused_edit_sends_nothing_and_remembers_nothing`,
  `picks_are_refused_by_name`,
  `the_point_is_the_click_then_the_typed_point_then_the_face`,
  `a_click_pick_is_one_fastener_run_refused_with_nothing_sent`.
- `cad/print/studies_tests.rs`: `the_registry_picks_keep_its_order_and_robocads_labels`,
  `split_sends_the_chosen_printer_and_joint_as_a_background_job`,
  `strength_and_plan_send_the_study_with_the_revision_or_robocads_explanation`,
  `strength_split_takes_the_first_study_part_selected_with_the_studys_settings`,
  `assembly_takes_a_selected_split_or_a_selected_pieces_split`,
  `coupons_send_the_split_or_none_with_the_chosen_printer_and_filament`,
  `the_split_and_coupon_dialogs_wait_for_the_registry`,
  `a_refused_start_sends_nothing_and_notes_nothing`,
  `a_print_block_colours_its_body_through_the_shared_rule`.
- `cad/print/tracker_tests.rs` (a fake RoboCAD on a loopback socket):
  `a_watched_job_shows_its_progress_then_robocads_done_text_and_refreshes`,
  `cancel_asks_first_then_sends_exactly_one_delete_per_running_job`,
  `robocads_texts_for_each_kind`.
- `cad/display/tests.rs`: `build_plate_sets_overhang_shading_and_its_toggle_flips_only_it`.
- `cad/surfaces/tests.rs`: `the_print_rows_run_native_actions`.
- `jobs/tests.rs`: `open_local_refuses_relative_and_missing_paths_by_name`.
- `cad/robot/panel_tests.rs`: `a_row_press_carries_the_revision_the_description_was_read_at`.
- `cad/ops/robot_tests.rs`: `a_partial_rest_edit_joint_keeps_the_joints_other_values`,
  `a_partial_rest_edit_joint_refuses_without_the_current_description`.
- `crates/sim-runtime/src/cad_client/print_tests.rs`:
  `registry_keeps_the_registry_order_and_drops_a_malformed_printer`,
  `study_reads_the_study_parts_and_split_groups`,
  `split_now_answers_the_summary_and_split_job_starts_a_job`,
  `print_start_sends_each_job_kind_and_refuses_others_unsent`,
  `jobs_list_oldest_first_and_drop_a_malformed_job`,
  `job_reads_and_cancel_sends_a_bare_delete`,
  `thin_walls_and_validation_read_tolerantly`,
  `print_ops_send_the_python_signature`,
  `split_errors_carry_robocad_text_and_status`,
  `choice_lists_match_robocad_dialogs`.
- `cad/tests/test_api_print_routes.py`: `/print/study` empty, with the
  study and split groups in tree order, read-only; `/print/registry` in
  registry order.

### Found by reading and fixed

- `cad_client/print.rs`: `Ordered` serialised as a list of pairs, so a
  registry round-tripped through serde came back empty; it now
  serialises as an object (17f90d08).
- Overhang shading was tied to the build plate; it is now its own setting,
  which the build plate sets as RoboCAD's `toggle_build_plate` does, with
  its own toggle (`print.overhangs`).
- cad-physical-inspect's known residual: a REST `cad_run ops.set_joint`
  with only some params sent the dialog's defaults for the rest; it now
  fills them from the joint (`cad/ops/robot_form.rs:fill_from_joint`), or
  is refused by name when the joint's values are not known.

### Deliberately different (summary)

8 rows, each with its reason in the ledger: the fastener dialog's fields
beside the view with a Point field; the wall check's remembered
threshold; validation without the open-edge check, in the status line;
split's added `expected_revision`; the Print jobs section with an inline
confirmation; the progress line without " — Print ▸ Print jobs… to
cancel", with failures on the status line and one list poll; the print
overlay's uniform per-part colour on the shared scale; and
`GET /print/jobs/{id}`'s `wait`, never honoured, replaced by the list
poll.

### Verification checklist

Nothing above was compiled or run. To do in the verification pass:

- [ ] `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- [ ] `cargo test -p sim-spatial --lib --bins` (including
  `app::tests::source_files_stay_small`, the registry cross-check and
  `jobs::tests::processes_are_started_only_in_jobs`), and by name
  `surfaces::tests::the_print_rows_run_native_actions`, the
  `cad::print` checks, edits, studies and tracker tests, the `jobs`
  `open_local` refusals, the Robot panel's row stamp and the partial
  `set_joint` tests listed above.
- [ ] `cargo test -p sim-runtime --lib cad_client`.
- [ ] `cd cad && .venv/bin/pytest -q tests/test_api_print_routes.py tests/test_api_physical_routes.py`
  (then the whole RoboCAD suite).
- [ ] In the window, [docs/cad-checklist.md](../cad-checklist.md) Part H
  side by side with RoboCAD: the wall check and validation, overhang
  shading, the fastener and clearance tools, a dovetail split, strength,
  plan with its progress, whole or split, the assembly guide, coupons, the
  Print jobs section and cancel, the print overlay and its staleness,
  leaving CAD mode while a job runs, the Robot panel row press and a
  partial REST Edit joint.

## One text entry (2026-10-01)

Batch one-text-entry (structural; T39.1–T39.3). Text entry in
`sim-spatial` goes through one keyboard focus and one kit text field.
Before it, 16 files read `KeyboardInput` messages with their own focus
logic (15 by `MessageReader`, the document picker by `MessageCursor`), and
four flags said "a field is typing" (`CadInputFocus`, `Builder::typing`,
robot `panel_ui::typing`, `materials::typing`). All of them are gone.
**Written and reviewed by reading only: nothing was compiled, tested or
run.** Commits: d385e7eb (the kit skeleton and guard), 8627fd51
(`Typing`), 91c44f38 (every site moved, the flags deleted, the key maps
gated, and both review passes' fixes), and the docs commit after it.

### Decision: TextDraft on InputFocus, not Bevy's `EditableText`

API facts read from the 0.19.1 registry sources:

- `bevy_ui_widgets-0.19.1/src/lib.rs:63-76`: `UiWidgetsPlugins` adds
  `EditableTextInputPlugin`, so it is already active. Its observer
  `on_focused_keyboard_input` (`text_input.rs`) handles
  `On<FocusedInput<KeyboardInput>>` and returns at once unless the focused
  entity has `EditableText` (`query.get_mut(focused_entity)`). It maps keys
  to `TextEdit`s (Copy/Cut/Paste, Cmd+A/C/X/V, word and line motion,
  Backspace, Delete, Escape as `CollapseSelection`, characters, Enter as a
  newline only when `allow_newlines`) and lets Enter and Tab propagate
  ("for tab navigation and submit actions"). `SelectAllOnFocus` selects on
  `FocusGained` (deferred to pointer release for a press). `ImeSystems`
  (HandleEvents, ToggleWindowIMEInput in PreUpdate; UpdatePosition in
  PostUpdate) drive the IME for a focused `EditableText`.
- `bevy_text-0.19.1/src/editing.rs`: `EditableText` holds a parley
  `PlainEditor`, `pending_edits: Vec<TextEdit>`, `max_characters`,
  `allow_newlines`; edits apply in PostUpdate (`apply_text_edits`, which
  needs `FontCx`, `LayoutCx` and `bevy_clipboard::Clipboard`) and trigger
  the `TextEditChange` entity event. `EditableTextFilter(Option<Arc<dyn
  Fn(char) -> bool>>)` refuses inserts. Its module doc lists as **not
  implemented**: placeholder text, text validation, AccessKit integration
  and "text form submission handling". It is "headless": no box, border or
  focus styling. `text_edit.rs`: `TextEdit` is the edit enum (Insert,
  Backspace, motions, selections, IME compose/commit, Paste).
- `bevy_input_focus-0.19.1/src/lib.rs`: `InputFocus` (`get`, `set(entity,
  FocusCause)`, `clear`, `from_entity`), set to the primary window by
  `set_initial_focus` (PostStartup). `InputDispatchPlugin` triggers
  `FocusedInput<KeyboardInput>` on the focused entity (bubbling to the
  window) in `InputFocusSystems::Dispatch`; it clears a despawned focus.
  `gained_and_lost.rs`: `FocusGained`/`FocusLost` entity events in
  PostUpdate (`InputFocusSystems::FocusChangeEvents`). `tab_navigation.rs`:
  `TabNavigationPlugin` is not added by `DefaultPlugins`.
- `bevy_winit-0.19.1/src/accessibility.rs:216-221`: AccessKit updates are
  skipped while the focused entity has no `AccessibilityNode`.
- sim-spatial's `bevy` features include `bevy_text`, `bevy_ui`,
  `bevy_ui_widgets` (which enables `bevy_input_focus`) and `keyboard`, but
  not `system_clipboard`, so `EditableText`'s copy and paste would be
  in-app only.

Why the kit keeps `TextDraft` driven by one system on `InputFocus`:

1. Every panel is rebuilt from its owner's state (despawn and respawn on a
   change key). An `EditableText` holds the editor state and must live
   across rebuilds; focusing a rebuilt node would lose focus (dispatch
   clears a despawned focus). Moving 19 fields to persistent editor nodes
   would rewrite every panel's drawing.
2. The batch needs a placeholder, submit and cancel as data, and an
   accessible label: `EditableText` has none of them.
3. Windowless tests: `apply_text_edits` needs the font, layout and
   clipboard resources; the kit's tests run on `InputPlugin` alone.
4. Copy, cut and paste would not reach the OS clipboard without a new
   feature. **Intended gap:** kit fields have no clipboard, cursor
   motion or selection beyond select-all (as every site before).

Bevy's observer cannot double-type into a kit field: no kit field entity
has `EditableText` (grep finds none in `src/`), so
`on_focused_keyboard_input` returns before queuing an edit. Revisit when
Bevy's text input gains placeholder, submit and AccessKit support and the
panels keep their nodes (or a field adopts a persistent node).

### Module map

- `ui_kit/text/mod.rs`: `FieldId` (a field's identity), `TextField` (the
  working `TextDraft`, `filter: Option<fn(char) -> bool>`, `placeholder`,
  `label`, `select_on_focus`, `EnterKey::{Submit, ShiftNewline,
  CommandSubmits}`, `TabKey::{Emit, Indent}`, `sticky`), `KitInput` (tags
  `Kit::input` nodes), `FieldMsg { field, event: FieldEvent::{Changed,
  Submit, Cancel, Tab { back }, Arrow { up }, Blur} }`, `Typing` (read-only
  check) and the run condition `typing`, `TextFocus` (`focused`, `typing`,
  `draft`, `focus`, `focus_draft`, `set`, `release`, `blur`; writes `Blur`
  through `Commands`), `TextFieldApp::add_text_field` (spawns the field
  entity once, with an `AccessibilityNode` carrying the label, placeholder
  and the draft as its value), `TextEntryPlugin` (added by `UiKitPlugin`;
  windowless).
- `ui_kit/text/input.rs`: `keys`, the one reader of `KeyboardInput` for
  text (PreUpdate, after `InputSystems` and `UiSystems::Focus`), and
  `release_held` (moved unchanged from `app/picker`).
- `ui_kit/text/draft.rs`: `TextDraft` and `DraftKey` (moved from
  `form.rs`), `TextDraft::key(key, chord, filter)`.
- `ui_kit/text/tests.rs`: the windowless tests and the source guard.

The input system, each frame: (1) a field that lost the keyboard to a
non-field entity gets `Blur`; (2) a left press not on a `KitInput` takes
the keyboard from a non-sticky field, and a mode switch from the field
that had it before the switch (`Blur`); (3) a field that gained the
keyboard since the last run releases every held key (`release_held`,
same-frame releases kept); (4) the focused field's keys: typing edits
the draft (one `Changed` per frame), Enter is `Submit` as `EnterKey`
reads it (the field keeps the keyboard: the owner blurs it on
acceptance), Escape is `Cancel` (the kit blurs), Tab is `Tab` or a
two-space indent, ↑/↓ are `Arrow`; keys after a Submit, Cancel or Tab in
the same frame are dropped; every key the field used and every key
pressed without Cmd/Ctrl is consumed (`clear_just_pressed` on both
`ButtonInput`s), so no key map sees it; Cmd/Ctrl chords the field
ignores pass through. Modifiers are followed from the keyboard messages
(not `ButtonInput`, which the release empties).

Owners: one `FieldId` per site (a form or a row set shares one, with the
row as owner data); a press on the field (its existing action component,
which `system_ui` also activates, so ids and handlers are unchanged) calls
`focus`; `FieldMsg` is read as data. Mode key maps run under
`not(ui_kit::text::typing)` or test `Typing::get()`.

### Sites

| Site | Old loop | Field | Submit / Cancel / Tab / Blur |
|---|---|---|---|
| Document picker "Open file…" | `app/picker/mod.rs` `keys` (own cursors, modifiers) | `picker.path` (select-all, sticky; `app/picker/modal.rs`) | open / close / give up the keyboard / redraw; unfocused Enter, Escape, Tab from `ButtonInput<Key>`; still releases held keys every frame while open |
| CAD name | `cad/panel.rs` `name_entry` | `cad.name` (sticky; `cad/panel/name.rs`) | checks, refusal keeps it / end / – / end |
| Numeric bar | `cad/numeric.rs` `entry` | `cad.numeric` (select-all) | `CadNumeric {values}` if every row evaluates / end, key cleared / next row selected / end |
| Inspector editors | `cad/inspector/editors.rs` | `cad.inspector.edit` (select-all) | as before |
| Inspector physical rows | `cad/inspector/entry.rs` | `cad.inspector.physical` (select-all) | as before |
| Materials search, dialog | `cad/materials/panel.rs` (two loops) | `cad.materials.search`, `cad.materials.form` (select-all, sticky) | as before; `typing()`/`end_typing()` deleted |
| Saved views | `cad/views/panel.rs` | `cad.views` | as before |
| Section offset | `cad/display/entry.rs` | `cad.section.offset` (select-all; opens "0") | `offset_action` / end / – / end |
| Attach URL | `cad/attach.rs` | `cad.attach` | attach / – / – / – |
| Files form | `cad/files/form.rs` | `cad.files` (sticky) | OK / Close / next text row / row cleared, retaken while open |
| Results forms | `cad/results/forms.rs` | `cad.results` (sticky) | OK / FormCancel / – / retaken while open |
| Parameter form (incl. "Text to sketch:", print and robot forms) | `cad/surfaces/form.rs` | `cad.form` (sticky) | `CadFormSubmit` (Text row: starts the clicks) / `CadFormCancel` / next row / row none |
| Command palette | `cad/surfaces/palette.rs` | `cad.palette` (sticky) | run highlighted row / close / – / close; `Arrow` moves the highlight; `Changed` re-ranks |
| Builder drafts | `builder/drafts.rs` `text_input` | `builder.draft`, `builder.note` (the note: Shift+Enter newline; both sticky) | `SubmitDraft` / `DropDraft` (+ `SetMode(Select)` with no drag in Build) / – / – |
| Lesson drafts and inputs | `lesson/actions.rs` `keys` | `lesson.note` (Shift+Enter newline), `lesson.block` (Cmd/Ctrl+Enter submits); Tab indents; sticky | `Submit` / `CancelDraft` |
| Robot gait path | `robot/panel_ui.rs` | `robot.gait_path` | check, accepted path blurs / – / – / – |

The flags: `CadInputFocus` (type, resource, every reader) deleted;
`Builder::typing`, `RobotPanelUi::typing`, `GaitPathDraft.focused`,
`Picker.focused`, `AttachDraft.focused`, `materials::typing`/`end_typing`
and `OrbitRules::typing` deleted. What remains is owner data reconciled
from `InputFocus` every frame (never consulted for key routing):
`Numeric::focus` (the row the open entry edits; read by transform, sketch
and REST state), `SectionEntry::typing` and `CadViews::typing` (draft and
error), `MaterialsState::focus` and `select_all` (REST `"typing"`),
`NameDraft.node`, the results form's `focused` and the files form's
`focus` row.
`cad/keys.rs` `Held` is `Typing` plus the pending chord (`keys::free`).

Gated on the shared condition: CAD keys (`keys::keys`, transform keys,
picks, sketch tools via `Held`; the numeric bar's Tab-to-open also not in
a chord's frame), the camera's numpad/arrow keys (`camera/input.rs` under
`not(typing)`) and fly's W/A/S/D/Q/E (`Typing`), builder keys, placement
X/Y/Z/Escape, `inspect::input`, lesson page keys, robot keys, hardware
Q/A `JogPress`, phenomena and Place keys. Ungated on purpose: hardware
`JogRelease` (a release always stops a jog) and keyboard STOP (no field can
hold the keyboard while the Leg panel is shown: the gait field refuses and
blurs, the picker closes), and gesture modifiers (Shift/Alt/Ctrl variants
of pointer gestures).

### Intended behaviour differences

- The picker's path field takes the keyboard when the picker opens; Tab
  gives it up. Unfocused, only one of Escape, Enter and Tab applies per
  frame, Escape first, then Enter, then Tab.
- Characters typed with Cmd/Ctrl held are not typed anywhere (the builder
  typed "v" for Cmd+V). Shift+Tab in the numeric bar still moves forward.
- A press elsewhere ends a non-sticky field's entry in PreUpdate; that
  press then also acts (a pick, a sketch point) where the old code blocked
  the frame.
- A menu, context menu or radial opening ends a non-sticky field's entry
  (`TextFocus::release`, `surfaces::input`): the numeric bar's, as before;
  the inspector's, section offset's and saved views' entries end instead of
  pausing. The name field and the forms are sticky and keep theirs.
- During a placement drag, Tab while the form types no longer also copies
  the base point. The camera's arrows are not stopped by a pending
  Shift+A chord.
- Robot and inspect keys are silent while any field types (not only their
  own); an open picker also sends robot `motion_keys`' one zero request.
- Keys typed in the frame after Enter (before the builder's or lessons'
  handler applied it) are not mirrored into the next draft.

### Reading trace (Build draft → numeric '10 mm + 2' → Enter → gait path)

1. **Build draft.** "/" in Build: `builder::actions::keys` (under
   `not(typing)`) writes the filter action; the handler opens
   `Builder.input`; `drafts::sync_field` (SimSync, Build) focuses
   `builder.draft` with the buffer. The "/" was read by
   `ui_kit::text::input::keys` in that frame's PreUpdate, before the field
   had focus, so it is not typed. Typed keys: `Changed` → the buffer.
2. **CAD numeric bar.** Switching to CAD: the kit blurs `builder.draft`
   (it had the keyboard before the switch); the draft stays open in
   `Builder.input`. In CAD with the Move tool, Tab: `numeric::entry`
   (`!typing`, no chord) focuses `cad.numeric` with row 0 selected and
   `began` set. Next PreUpdate the kit releases held keys. Typing
   "10 mm + 2": each frame's `Changed` sets `texts[0]` and re-evaluates
   (`fields[0].kind.evaluate` → `sim_runtime::units::evaluate(text,
   false, Some("mm"))` = 12 mm), so the bar shows "= 12 mm" live.
3. **Enter commits.** The kit writes `Submit` (a `Changed` comes first
   only if keys typed in the Enter frame edited the draft) and consumes
   Enter (CAD's keys never see it). `numeric::entry` evaluates every row;
   all evaluate, so it writes `CadNumeric { values: ["10 mm + 2", …] }`,
   clears `focus` and `key` and blurs the field.
   `transform::commit::numeric` evaluates again and commits with the
   `began` revision (refused by name if RoboCAD's document changed).
4. **Robot gait path.** In Robot mode a press on the gait path field
   (`robot/panel_ui.rs`, refused while the picker or Leg panel is open)
   focuses `robot.gait_path`. Next PreUpdate the kit releases held keys,
   and `motion_keys` (which reads no key while typing) sends
   `HeldKeys([])` once as typing starts, so a robot walking on a held W
   stops. Enter: `Submit` → the existing path check; an accepted path
   writes its action and blurs.

### Review findings (four pair-reviewers by area, then fixes)

Fixed: a `field` name clash in `cad/inspector/editors.rs` (E0255; renamed
`editor_field`); `TextFocus` held a `MessageWriter<FieldMsg>`, which
conflicts with a `MessageReader<FieldMsg>` in one system (Blur now goes
through `Commands`); the palette's ↑/↓ (new `FieldEvent::Arrow`); a mode
switch blurring a field focused after the transition (`StateTransition`
runs after PreUpdate: only the field that had the keyboard before is
blurred); the windowless tests asserting consumption in the focus frame,
where the release alone hid the key (an update between focus and the
key); dead `TextField::filter` builder, `Typing::draft` and
`TextDraft::key` wrapper; field entities without an `AccessibilityNode`
(AccessKit updates paused while typing); `cad_cancel` and an opening menu
no longer ending the numeric entry; the materials dialog not clearing a
pending numeric focus request; a material row press ending the search
(rows are `KitInput`); a saved-view focus request taking the previous
draft's typing; the files and results forms losing modality after a
`Blur` (retaken while open and no field types); the numeric Tab opening
in a chord's frame; sketch clicks blocked while renaming (only the
form's, numeric bar's and inspector editor's fields block them, as
before); a new parameter form taking the previous form's typed text; the
attach field's ordering before CAD's keys; keys typed between Enter and
its handler landing in the next builder or lesson draft; stale comments.

Rejected, with reasons: an accessibility *crash* (Bevy skips the update
when the focus has no node; the pause was real and is fixed); Bevy's
checkbox taking the keyboard from a sticky field (sim-spatial uses no
`bevy_ui_widgets` checkbox or menu); the camera's arrows during a pending
chord (accepted, listed above).

Unverified, to watch on the first build: every `ParamSet<(MessageReader<
FieldMsg>, TextFocus)>` borrow, the `Option<fn(char) -> bool>` field in
struct-update literals, `AccessibilityNode` deref to `set_label`,
`set_placeholder`, `set_value`, `value`, and the run-condition
`not(typing)` on systems in chains.

### Tests (windowless, `ui_kit/text/tests.rs`)

`only_the_focused_field_types`, `mode_keys_are_silent_while_typing`,
`held_keys_are_released_on_focus`, `release_keeps_same_frame_releases`
(moved from `app/picker_tests.rs`, where it was
`picker_release_keeps_same_frame_releases`), `enter_submits_and_escape_cancels`,
`arrows_reach_the_owner`, `a_press_elsewhere_blurs`,
`kit_inputs_and_sticky_fields_keep_the_keyboard`, `a_mode_switch_blurs`,
`tab_filter_and_chords`, and the guard
`keyboard_text_is_read_only_in_the_kit` (no `KeyboardInput>` outside
`ui_kit/text/`, which covers `MessageReader`, `MessageCursor`,
`Messages` and `FocusedInput`; allowlist empty). Also updated:
`camera/tests.rs` (keys silent while a field types),
`cad/materials/tests.rs` (the dialog's field takes the keyboard),
`cad/scene.rs` (`the_gate_follows_the_text_focus` replaced by
`the_gate_keeps_alt_left_drag_off_without_a_document`), `app/tests.rs`
and `builder/test_support.rs` (the draft is kept, without
`Builder::typing`), `ui_kit/tests.rs` (`TextDraft::key` with a filter; the
picker's path-field parts are `KitInput`).

### Verification checklist

- [ ] `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- [ ] `cargo test -p sim-spatial --lib --bins`, including
  `ui_kit::text::tests::*`, `app::tests::source_files_stay_small`,
  `jobs::tests::*`, the copy guard and `camera::tests`.
- [ ] In the window: Build "/" filter typed without "/"; a builder draft
  across Build → Lessons → Build; the CAD numeric bar "10 mm + 2" + Enter;
  the name field; the palette's ↑/↓; a held W in Robot then a press on
  the gait path field (the robot stops); the picker's path field.

## CAD organize (2026-10-01)

*Batch cad-organize (default order item 7, §9 phase 1; §9 "Later CAD
epics" 7). Done pending verification: written and reviewed by reading
only, in b476b28e (the client's `threads.rs`, `organize.rs`,
`references.rs` and `system_link.rs` with fake-server tests, the `api.py`
gap route with its pytest, and `annotations::InFlight`) and the commits
after it (the `cad/tree/`, `cad/threads/` and `cad/references/` modules,
their wiring and the review fixes). Nothing was compiled; only the gap
route's pytest ran (2 passed at b476b28e; the third test, added after
review, is unverified). It is the fifth epic stacked uncompiled since
aa34ef48.* It brings RoboCAD's organization features into CAD mode: the
outliner's search, expand and collapse, Shift/Ctrl multi-select, inline
rename, drag-and-drop, its context menu (Fit in view, Isolate, Hide,
Show, Lock, Unlock, Group selection…, Move to group, Make unique, Set as
active group, Delete, Clear active group, Show all) and New group;
RoboCAD's comment threads as the fourth `annotations::ThreadSource`, drawn
by the one `ui_kit::threads` panel, with Annotate (N), pins, part links,
Show on model, Fit in view and the temporary "Show only linked parts";
the References section with textured reference image planes, placement,
Align view, Sketch over this and the calibrate tool; and the linked
system file with Open in builder, an in-window switch to Build mode.
RoboCAD stays the kernel and the reference: its command layer does every
edit, and undo and provenance stay RoboCAD's. The components library,
component jobs and the system graph moved to the new epic cad-components
(see §9 "Later CAD epics"). The ledger rows are in
[docs/cad-parity.md](../cad-parity.md) (108 rows: 38 moved to `later-epic:
cad-components`; of the other 70, 44 `done-by-reading` and 26
`deliberately different`; none open; one REST row added for the gap
route; four earlier outliner context-menu rows became `done-by-reading`;
the totals are in its Counts), and the side-by-side steps are in
[docs/cad-checklist.md](../cad-checklist.md) Part I. Paths are relative
to `crates/sim-spatial/src/` unless they name another crate. Each module
root (`cad/tree.rs`, `cad/threads/mod.rs`, `cad/references/mod.rs`) has a
doc comment listing what it does and its deliberate differences.

### Shape

| Module | What it owns |
|---|---|
| `crates/sim-runtime/src/cad_client/threads.rs` | The typed thread client: `CadClient::threads` (`threads_route`: `node_id`, `status`, `run_id`), `thread`, `create_thread` (`NewThread`), `update_thread` (`ThreadPatch`), `delete_thread`, `add_comment`, `comment`, `update_comment`, `delete_comment`; `CadThread`, `ThreadAnchor`, `AnchorStatus` (with `Unknown` for a state it does not know), `PartRef`, `LinkedPart`; `part_links` and `part_link` (RoboCAD's `[label](part:ID)`). Tests in `threads_tests.rs`. |
| `crates/sim-runtime/src/cad_client/organize.rs` | `CadClient::group`, `move_nodes`, `set_active_group`, `set_locked`, `move_node` (the `PATCH /nodes/{id} {"parent", "index"}` form, client only). Tests in `organize_tests.rs`. |
| `crates/sim-runtime/src/cad_client/references.rs` | `CadClient::import_references`, `update_reference` (`ReferenceUpdate`, only the given keys), `calibrate_reference`, `reference_image` (`ReferenceImage::decode`, `base64_decode`); `ImagePlacement::of` (a node's `image` without the bytes) and `corners`; `PlaneJson`. Tests in `references_tests.rs`. |
| `crates/sim-runtime/src/cad_client/system_link.rs` | `CadClient::link_system`, `unlink_system`, `refresh_system_link`, `system_status` (`SystemStatus`, `LinkState`, `SystemStatus::line`: RoboCAD's four texts). Tests in `system_link_tests.rs`. |
| `cad/robocad/api.py` (gap route) | `GET /nodes/{id}/image` in `Service.reference_image` (api.py:1317-1336, routed at 1492-1493): a reference image node's stored bytes in base64 with `format`, `width_px`, `height_px`, `bytes` and `revision`; 404 for a node that is not an image or has no stored bytes, 422 for bytes Pillow cannot read; read-only. Pytests `cad/tests/test_api_reference_image.py`. The only Python change. |
| `annotations/mod.rs` | `InFlight` and `InFlight::land`: the seam a remote `ThreadSource` keeps its pending requests in and lands each job's answer through, as the sidecar sources land `sim_annotate::store::Store`'s. |
| `cad/ops/catalogue/organize.rs` | The registry's `group.group` ("Group selection", `Ops.group(selection)` with the default name "Group") and `group.set_active` ("Set selected group as active", one group node; refused by name with none) as catalogue entries. |
| `cad/tree.rs` | The outliner's module root: its doc, `command_action`, `menu_open` (read by `pick.rs` so a press that dismisses the menu does not select), `restarted` (on reconnect, from `sync`), and the plugin `build` (the kit fields, `input::search` after `keys::gate` and before `keys::keys`, the popup's input before `keys::gate`). |
| `cad/tree/state.rs` | `TreeState` (on `CadDocument::tree`: search, `collapsed`, the anchor, the rename row, the menu, the dialog, the drag) and the pure readings: `matches`, `shown`, `range`, `index_of`, `move_plan` (a drop's parent and index with `Ops.move_nodes`' refusals), `group_paths`. |
| `cad/tree/handle.rs` | `TreeOp`, `TreeArgs` (`CadAction::CadTree`, REST `cad_tree`) and the one `handle`: display ops on `TreeState`; `select_action` → `CadSelect`; `rename`, `flags` (Lock/Unlock, Hide/Show), `group`, move and set-active, each one RoboCAD call through `actions::edit_at`; `state_json` (`cad_state.tree`), `specs`. |
| `cad/tree/controls.rs` | `controls_of`, `controls` (`cad:tree:*`) and `menu_rows`: the context menu's entries in RoboCAD's order, each with its action and readiness. |
| `cad/tree/rows.rs` | `draw` and `highlight` (the rows, `Part::Tree`; `ACTIVE_GROUP`, `name_colour`), `tools` and `tools_key` (`Part::TreeTools`: the search field, New group, Expand all, Collapse all). |
| `cad/tree/input.rs` | `search` (Ctrl/Cmd+F over the tree dock), `rows` (presses, the 400 ms double-click, Shift and Ctrl, and the drag from Bevy picking's pointer messages), `fields` (the search, rename and group-name kit fields). |
| `cad/tree/popup.rs` | The context menu (a kit popup at the pointer) and the "Organize components" dialog: `draw` in Present, `input` in Input (its Escape before `keys::gate`). |
| `cad/threads/mod.rs` | `ThreadsState`, `ThreadsArgs`/`ThreadsOp` (`CadAction::CadThreads`, REST `cad_threads`), `Filter` (`keeps`), `command_action`, `takes_clicks`, `shown`, `edit_answered` (lands commits through `InFlight::land`), `restarted`, `state_json`, `specs`. |
| `cad/threads/source.rs` | `CadAnchor`, `thread_of` and `CadThreadSource` (the fourth `annotations::ThreadSource`): `validate` (RoboCAD's limits) and `commit`, one RoboCAD call per command (`Request::send`) through `edit_at`, reported `Committed::Pending`. |
| `cad/threads/read.rs` | `tick`: `GET /threads` (all threads, `CadClient::threads(None, None, None)`) on a `Pool::Dedicated` job per (generation, revision, epoch); `thread` (a thread from the last list), `line` (a stale list's label), `wait` (a REST caller waits for the list at RoboCAD's revision). |
| `cad/threads/ops.rs`, `controls.rs` | `handle` and its ops (`create`, `edit_message`, `link`, `label`, `insert_link`, …); `controls_of`, `controls` (`cad:threads:*`), `shown_threads` (the filter, applied locally), `attachment`, `submit_action`. |
| `cad/threads/dock.rs`, `input.rs` | The Comments section (`Part::Comments`: `draw`, `key`, `label_dialog`, the panel's `CadHost`); the three kit fields (composer, author, part label) and `escape` (after `references::calibrate::escape`, before `transform::keys`). |
| `cad/threads/annotate.rs`, `pins.rs`, `isolation.rs` | Annotate and Reattach… (`start`, `click`, `place`, `reattach`); the numbered pins (`entries`, `draw`, `press`); Show on model, Fit in view, Show only linked parts, Return to assembly and part links (`show`, `fit`, `view_parts`, `end`, `highlight`, `part_link`), display only. |
| `cad/references/mod.rs` | `ReferencesState`, `ReferencesArgs`/`ReferencesOp` (`CadAction::CadReferences`, REST `cad_references`), `handle`, `command_action`, `controls`, `edit_answered`, `restarted`, `takes_clicks`, `state_json`, `specs`. |
| `cad/references/dock.rs`, `form.rs`, `input.rs` | The References section (`Part::References`: `draw`, `key`); the placement form (`ROWS`, `PlacementForm::load`, `apply`, `update`); the kit fields (the placement rows, the path field of Add and Link, the calibrate tool's "Real distance"). |
| `cad/references/edits.rs`, `reads.rs` | The edits (add with `image_path`, visible, `placement`, link, accept, unlink, remove), each one RoboCAD call through `edit_at`; the windowless reads (`tick`: placements per (generation, shown revision), the system status) and `receive` (the align after an import, Open in builder's window action, the path listing). |
| `cad/references/align.rs`, `calibrate.rs`, `planes.rs`, `drop.rs`, `system_link.rs` | Align view and Sketch over this (`align`, `sketch`); Calibrate scale (`start`, `click`, `pick`, `distance`, `escape`, `markers`); the textured planes (`sync`, `decodable`, `decode`); dropped files (`drops`); the status line and Open in builder (`line`, `builder_target`, `open_builder`, `switch_action`). |
| `cad/surfaces/registry.rs` | `Do::Organize(id)` → `organize_action` (`tree::command_action`, else `threads::command_action`, else `references::command_action`) for `view.references`, `reference.import`, `tool.annotate` (N), `view.comments` and `view.comment_pins`; At the cad-organize checkpoint, `components.show` and `components.make` were `Native::Later("cad-components")`; T42 now resolves both through `components::command_action` to typed native actions. |
| `cad/keys.rs`, `cad/panel.rs`, `cad/pick.rs`, `builder/drafts.rs` | The clash table's Ctrl+F, N and Escape rows; `Part::TreeTools` above the tree and `Part::Comments`, `Part::References` at the top of the right dock (the panel's wheel stands aside over the outliner's popups); a press that dismisses the outliner menu or began with a click tool does not select (`tool_press`, `tree::menu_open`); Build mode's `drops` skips the frame Build mode is entered. |

### Decisions

- **RoboCAD's threads are a remote `ThreadSource` on edit jobs.**
  `cad/threads/source.rs:CadThreadSource` implements
  `annotations::ThreadSource`; each commit is one RoboCAD call through
  `actions::edit_at` (refused by name when stale, in flight or
  disconnected) and reports `Committed::Pending` with the edit's sequence;
  its pending requests sit in `annotations::InFlight` and
  `cad/threads/mod.rs:edit_answered` (called from
  `cad/sync/mod.rs:finish_edit`) lands each answer through
  `InFlight::land`. *Why:* §7's one annotations service and one thread
  panel; the edit path already refuses what an edit must not do. Undo
  stays RoboCAD's (the source refuses undo and redo by name).
  *Rejected:* a CAD-only comments panel, a second thread UI.
- **One list read, filtered locally.** `read.rs:tick` reads every thread
  and the dock filters (`Filter::keeps`, `controls::shown_threads`), as
  RoboCAD's `refresh` does; Show on model, Fit in view and Show only
  linked parts read the thread from that list (`read::thread`), so no
  viewer code calls `CadClient::thread`.
- **One gap route, `GET /nodes/{id}/image`.** `node_detail` strips the
  image bytes (api.py:141-142), so no route served a reference's pixels.
  The route answers the stored bytes in base64 with format, pixel size and
  revision; it is read-only. *Rejected:* reading the file at the image's
  stored `path`, which may be gone or changed; the document holds the
  bytes.
- **`POST /threads/{id}/show` is reproduced natively.** It is GUI-only
  (409 headless) and moves RoboCAD's own camera. Show on model restores
  the thread's saved camera on the shared camera through `views::convert`;
  Show only linked parts isolates the linked parts at display time only
  (`cad/mesh.rs`), never calling `set_visible`, and Return restores the
  camera, the selection and the whole `CadDisplay`. A thread of
  experiment evidence opens isolated captured review through threads/isolation.rs and the typed review action (T43).
- **Ctrl+F stays Fillet.** RoboCAD's keymap binds Ctrl+F to Fillet while
  its outliner placeholder reads "Search (Ctrl+F)…". Here Ctrl+F focuses
  the search only with the pointer over the model tree dock
  (`tree/input.rs:search`, after `keys::gate` and before `keys::keys`,
  consuming the key); elsewhere it is Fillet.
- **`group.set_active` refuses with no group selected.** RoboCAD's
  handler calls `set_active_group(None)` then, silently clearing the
  active group under a "Set" label. The command refuses by name ("Select a
  group to make it the active group"); the outliner menu's "Clear active
  group" clears it.
- **Comments and References are sections at the top of the right dock
  while shown,** above the inspector (`panel::Part::Comments`,
  `Part::References`), as Materials, Robot and Print jobs are sections;
  the outliner's tools are `Part::TreeTools` above the tree.
- **Open in builder switches this window.** RoboCAD starts a second
  `sim-spatial --system … --schematic` process. Here
  `references/system_link.rs:open_builder` sets `switch_to`, and
  `references/reads.rs:receive` writes `system_link::switch_action`
  (`WindowAction::Switch(ModeSwitch { mode: Build, document:
  Document::Path })`) to `app/switch/mod.rs:handle`, which opens the file
  through the document registry (`app/switch/prepare.rs`). Refused by name
  with nothing written: without a window; unlinked or missing ("Link an
  existing system file first"); while the status is not read for the
  shown revision; and when leaving CAD mode is blocked
  (`results::switch_refusal`, the live link's check). Nothing outside
  `jobs` starts a process.
- **Escape has one consumer per press, in a fixed order:** the outliner
  popup's (`tree::popup::input`, before `keys::gate`), the results
  form's (`results::forms::input`, before `CadKeySet::Gate`; consumed
  since b75846ff), the file form's
  (`files::form::input`, before `CadKeySet::EscapeTool`), then the
  calibrate tool's (`references::calibrate::escape`, in the public
  `CadKeySet::EscapeTool`), then the threads' (`threads::input::escape`:
  Annotate, else Return to assembly; in `CadKeySet::Escape`), then the
  Select tool's (`transform::keys`, `CadKeySet::ToolKeys`). The sets are
  chained Gate → EscapeTool → Escape → ToolKeys in `cad::configure_sets`
  (cad-checklist-traces, 2026-10-02), so no feature orders against
  another's Escape system. Each consumes the key only when it acted (the
  `cad/keys.rs` Escape row); the surfaces' Escape, which doesn't consume
  it, is stood aside for by state. Before b75846ff the results form's
  Escape closed it without consuming the key, so the same press also
  fired the Select tool's `cad:cancel` (`transform::keys` stands aside
  only for `doc.ops`); it now clears the key
  (`cad/results/forms.rs:309-314`).
- **Drag-and-drop reads pointer messages, a recorded departure from §3.**
  §3 keeps observer triggers for pointer events on entities. The
  outliner's gesture (press, double-click timing, a pending plain press
  on a selected row, the drag's slop, target and end) is one `Local`
  spanning several events, and the rows are despawned and rebuilt during
  a drag, so one Input system (`tree/input.rs:rows`) reads Bevy picking's
  `Pointer<DragStart/Drag/DragEnter/DragOver/DragLeave/DragDrop/DragEnd>`
  messages in order. Every `Pointer<E>` is both an `EntityEvent` and a
  `Message` (`bevy_picking-0.19.1/src/events.rs:71-84`), written by
  `pointer_events` beside each trigger (events.rs:597-614; 824-835
  `DragEnter`; 1007-1031 `DragDrop` then `DragEnd` on release) in
  PreUpdate's `PickingSystems::Hover` (lib.rs:428-454). Messages do not
  bubble (the triggers propagate through `PointerTraversal`,
  events.rs:95-125), so a hit on a row's chip resolves to the row by
  walking `ChildOf`, and the row's labels are `Pickable::IGNORE`. The drop
  is `tree/state.rs:move_plan`: on a group, into it (its top quarter, in
  front of it: a recorded difference, so a group can be reordered); on
  another row, in front of it under its parent; below the rows, the top
  level; RoboCAD's refusals checked first; then one `Ops.move_nodes`.
- **Dropped files: `FileDragAndDrop` in CAD mode only.**
  `bevy_window-0.19.1/src/event.rs:372-406`: `FileDragAndDrop` is a
  `Message` (`DroppedFile { window, path_buf }`, `HoveredFile`,
  `HoveredFileCanceled`), registered by `WindowPlugin`; winit writes one
  `DroppedFile` per file. The window has one drop target, so the modes
  share it by state: `references/drop.rs:drops` runs only in CAD mode and
  `builder/drafts.rs:drops` only in Build mode, and each skips the frame
  its mode is entered (a reader gated off keeps its cursor, and messages
  live two updates, so drops from the mode before would otherwise land).
  A folder or a file without an image suffix is refused by name before
  anything is sent (`references/edits.rs:image_path`; RoboCAD sends every
  dropped file to `import_references`).
- **Image formats: PNG and JPEG are textured.** sim-spatial enables
  Bevy's `png` and `jpeg` features (`crates/sim-spatial/Cargo.toml`);
  `references/planes.rs:decodable` takes png, jpeg, jpg and mpo (an MPO
  decodes as its first JPEG frame) and `decode` builds the texture with
  `Image::from_buffer` (`bevy_image-0.19.1/src/image.rs:1557`) on the read
  job. WebP and BMP, which RoboCAD's file filter accepts, are imported,
  placed and listed but not drawn; the References section says so for
  each. Pixels are cached per (connection generation, node): RoboCAD never
  changes an image node's bytes. *Revisit if* another image feature is
  enabled.
- **File paths are typed in the viewer's path field,** for Add reference
  images… (one image per submit; absolute image paths only) and Link
  system file…, as cad-views-export's file commands (no system file
  dialog).

### Reading trace (search → rename → drag into a group → Annotate a face → reply with a part link → Show only linked parts → Return → Open in builder)

1. **Search.** Pointer over the tree, Ctrl+F → `cad/tree/input.rs:search`
   (61; after `keys::gate`, before `keys::keys`) gives the kit's search
   field the keyboard (`InputFocus`); typing → `TreeOp::Search` →
   `cad/tree/handle.rs:handle` (195) → `TreeState` → `cad/tree/state.rs:matches`
   (166) and `shown` (179) → `cad/tree/rows.rs:draw` (143) with every shown
   row expanded and `TreeState::collapsed` unchanged.
2. **Rename.** A second press on a row within 400 ms
   (`cad/tree/input.rs:rows` (199), `DOUBLE_CLICK` (45)) →
   `TreeOp::BeginRename` → the row's kit field with the name selected;
   Enter → `TreeOp::Rename` → `cad/tree/handle.rs:handle` → `rename` (341;
   stripped; unchanged: nothing sent; empty: refused by name) → one
   `CadClient::patch {"name"}` through `cad/actions.rs:edit_at` → when it
   answers, `cad/sync/mod.rs:finish_edit` (531) → refresh.
3. **Drag into a group.** Press, drag past the slop, release on a group
   row: `cad/tree/input.rs:rows` reads the `Pointer<DragStart … DragDrop,
   DragEnd>` messages, `cad/tree/state.rs:move_plan` (236) gives (parent =
   the group, index None) → `TreeOp::Move` → `handle.rs:handle` (Move at
   247) → one `crates/sim-runtime/src/cad_client/organize.rs:CadClient::move_nodes`
   through `edit_at` (262) → `finish_edit` → refresh.
4. **Annotate a face.** N → `cad/keys.rs` → `CadInvoke { tool.annotate }`
   → `cad/surfaces/registry.rs` `Do::Organize` → `organize_action` →
   `cad/threads/mod.rs:command_action` (348) → `cad/threads/annotate.rs:start`
   (48) → a left press over the 3D view → `click` (153: its own ray,
   `transform::ray_hit`, the face through `CadMeshes::face_at` at the shown
   revision) → `cad_threads {op: place}` → `place` (78: the Comments
   section, the "+" pin, the composer); Post annotation →
   `cad/threads/ops.rs:create` (315) → `annotations::apply`
   (`annotations/mod.rs`, 219) → `cad/threads/source.rs:CadThreadSource::commit`
   (426) → `Request::send` (282) → `CadClient::create_thread` on the edit
   job → `cad/sync/mod.rs:finish_edit` calls
   `cad/threads/mod.rs:edit_answered` (378, at sync 548) →
   `InFlight::land` (384).
5. **Reply with a part link.** Select a body, Insert part link from
   selection → `cad/threads/ops.rs:insert_link` (426): `part_link(label,
   id)` into the composer; Reply → `annotations::apply` →
   `CadThreadSource::commit` → `CadClient::add_comment`.
6. **Show only linked parts.** A press on the link → `cad/threads/dock.rs`
   `CadHost::link` (70) → `ThreadsOp::PartLink` → `cad/threads/ops.rs:handle`
   (272) → `cad/threads/isolation.rs:part_link` (286): `view_parts` (193;
   the camera, selection and whole `CadDisplay` captured, the part and its
   descendants shown alone, framed), then the selection exactly `[ID]`,
   as `cad_select` selects it.
7. **Return.** Escape → `cad/threads/input.rs:escape` (214; in
   `CadKeySet::Escape`, after `CadKeySet::EscapeTool`, before `transform::keys`) →
   `cad/threads/isolation.rs:end` (270): the camera, the selection
   (without parts deleted since) and the whole `CadDisplay` restored.
8. **Open in builder.** References ▸ Open in builder →
   `CadAction::CadReferences` → `cad/references/system_link.rs:open_builder`
   (122; `builder_target` (99), `switch_refusal` (94)) sets `switch_to` →
   `cad/references/reads.rs:receive` (209) writes
   `system_link::switch_action` (137; at reads 230) →
   `app/switch/mod.rs:handle` (365) → Build mode on the linked file.

### Tests (windowless)

- `cad/tree/tests.rs`: `the_search_keeps_ancestors_and_descendants`,
  `the_collapse_state_survives_an_edit_and_a_search`,
  `shift_ranges_and_ctrl_toggles_write_the_expected_select`,
  `a_drop_builds_robocads_parent_and_index`,
  `every_tree_control_fits_its_pattern_and_round_trips_through_rest`,
  `an_edit_while_another_is_in_flight_is_refused_with_nothing_sent`,
  `renames_and_groups_are_checked_before_sending`.
- `cad/threads/tests.rs`: `a_commit_is_refused_by_name_when_stale_and_pending_otherwise`,
  `a_posted_pin_opens_the_new_thread_when_robocad_answers`,
  `a_part_link_selects_as_cad_select_does`,
  `showing_linked_parts_alone_never_writes_visibility`,
  `every_threads_control_fits_its_pattern_and_round_trips`,
  `thread_detail_maps_onto_the_annotations_thread`,
  `a_whole_thread_change_is_one_patch_of_what_changed`.
- `cad/references/tests.rs`: `controls_fit_the_pattern_and_round_trip_through_rest`,
  `apply_placement_builds_robocads_one_update`,
  `calibrate_refuses_equal_points_and_stale_picks`,
  `the_align_camera_is_robocads_formula`,
  `open_in_builder_switches_this_window_to_build_on_the_linked_file`,
  `the_system_status_line_is_robocads`, `drops_are_taken_in_cad_mode_only`.
- `annotations/tests.rs`: `a_remote_commit_lands_through_in_flight`.
- `cad/surfaces/tests.rs`: `the_organize_rows_run_native_actions`.
- `crates/sim-runtime/src/cad_client/threads_tests.rs` (a fake RoboCAD on
  a loopback socket): `threads_read_tolerantly_with_filters_encoded`,
  `an_unknown_anchor_state_and_malformed_fields_read_as_defaults`,
  `create_update_and_delete_send_annotations_shapes`,
  `comments_reply_read_edit_and_delete`,
  `part_links_match_annotations_part_link`.
- `crates/sim-runtime/src/cad_client/organize_tests.rs`:
  `organize_ops_send_positional_args`,
  `a_one_node_move_is_a_parent_and_index_patch`.
- `crates/sim-runtime/src/cad_client/references_tests.rs`:
  `reference_ops_send_only_the_given_keys`,
  `an_import_without_a_plane_sends_null`,
  `placement_reads_from_the_node_and_places_the_corners`,
  `the_image_route_decodes_base64`, `base64_matches_pythons_encoder`.
- `crates/sim-runtime/src/cad_client/system_link_tests.rs`:
  `link_accept_and_unlink_are_one_op_each`,
  `status_reads_every_state_and_writes_robocads_line`.
- `cad/tests/test_api_reference_image.py`:
  `test_image_route_returns_the_stored_bytes`,
  `test_image_route_refuses_other_nodes` (2 passed in 4.5 s at b476b28e),
  `test_image_route_refuses_missing_or_unreadable_bytes` (added after
  review; its run timed out at 10 s, so unverified).

### Review findings (six pair-reviewer passes, then fixes)

Six passes by area: the client and `api.py`; the outliner; the threads
and the annotations seam; the references and the system link; the
shared wiring; the docs. By reading, they found no compile errors (not
a substitute for a build). Fixed:

- `api.py`: the image route refuses missing bytes (404) and bytes Pillow
  cannot read (422); a pytest was added, unverified (its rerun timed out).
- Client: `AnchorStatus` defaults to `Unknown` ("Attachment unknown", a
  grey pin); a `part_refs` list keeps its good items when one is malformed.
- Keys: Escape ordering made deterministic (tree popup → calibrate →
  threads → Select tool); the Ctrl+F search ordered after `keys::gate`.
- Outliner: a press that dismisses the outliner menu, or that started
  with a click tool, no longer selects (`pick.rs` `tool_press`,
  `tree::menu_open`); Shift+Ctrl range; frozen menu ids (the menu acts on
  the nodes it opened for); the panel's wheel stands aside over the
  outliner's popups.
- Reconnect: `restarted()` for the tree, the threads and the references.
- Threads: the draft is not lost when a post lands; isolation frames with
  `CadMeshes::bounds_of`; bodies respawned during an isolation stay
  hidden, and the overhang overlay hides with its body; no per-frame
  document change while a thread read runs; no REST routes in window copy;
  a REST revision waits for the read (`read::wait`); the isolation
  restores the whole display.
- References: the status read is requested when needed (Accept, Unlink
  and Open in builder with the dock closed); failed placement reads are
  shown; refusals of placement, calibrate, import and link are shown
  (`edit_answered` gets the `Result`); MPO decoded; a Close button; the
  builder's drop skips the frame Build is entered; non-image paths
  refused; Align and Sketch over this use the placement read at the shown
  revision; RoboCAD's calibrate hint; Open in builder refused without a
  window; one shared `switch_refusal`.
- A final pass over the fix round found one compile error, fixed: the
  Comments dock's key moved the non-`Copy` `sending` out of a borrow
  (`threads/dock.rs:key` now keys the sequence only). The calibrate and
  threads Escape readers now stand aside while a file form is open
  (`files::form` then closed on Escape without consuming it; it consumes
  it since 83ebb812, and the results form since b75846ff).

Low-severity leftovers, not fixed: the builder's existing drop and
library import read files on the UI thread (pre-existing); one
`GET /nodes/{id}` per image node per revision; WebP and BMP untextured;
a reconnect during Show only linked parts drops the isolation without
restoring the camera, selection and display it saved
(`threads::restarted`).

### Deliberately different (summary)

26 rows, each with its reason in the ledger. The outliner: Ctrl+F only
over the tree dock; expand and collapse refused while searching; a
rename ends without renaming on focus loss, and an empty name (rename,
New group, Group selection…) is refused by name; a drop on a group row's
top quarter lands in front of it; "Move to group" is a heading, not a
submenu; `group.set_active` refuses with no group selected. The threads:
Annotate and Reattach pick on the press and leave the selection mode
alone; "Attachment unknown" and a grey pin for an unknown state; Show on
model, Show only linked parts and `POST /threads/{id}/show` reproduced
from the thread list, evidence threads refused; a part row has a single
press and the label dialog is inline; a part link selects exactly the
linked node; Enter posts in the composer. The references: drops and
paths that are not images refused by name, one image per submit in the
path field, a drop anywhere on the CAD window; WebP and BMP not textured,
MPO as JPEG, pixels cached per (connection, node); no preview thumbnail;
the calibrate distance in the References section; an image off the
nameable planes leaves the active plane and refuses Sketch over this;
Link system file… in the path field; Open in builder switches this
window to Build mode.

### Verification checklist

Nothing above was compiled or run, except the gap route's pytest (2
passed at b476b28e; the third test, added after review, is unverified).
To do in the verification pass:

- [ ] `cargo build -p sim-spatial --lib --tests --bins` with no warnings
  (the four stacked epics before this one build in the same pass).
- [ ] `cargo test -p sim-spatial --lib --bins` (including
  `app::tests::source_files_stay_small`, the registry cross-check,
  `surfaces::tests::the_organize_rows_run_native_actions`, the keyboard
  guard's allowlist and `jobs::tests::processes_are_started_only_in_jobs`),
  and by name the `cad::tree`, `cad::threads` and `cad::references` tests
  and `annotations::tests::a_remote_commit_lands_through_in_flight`.
- [ ] `cargo test -p sim-runtime --lib cad_client` (the threads, organize,
  references and system_link fake-server tests).
- [ ] `cd cad && .venv/bin/pytest -q tests/test_api_reference_image.py`
  (all three; then the whole RoboCAD suite).
- [ ] In the window, [docs/cad-checklist.md](../cad-checklist.md) Part I
  side by side with RoboCAD: search, expand and collapse, Shift/Ctrl
  select, rename, drag into a group and before a sibling, the context
  menu, New group; the Comments section, Annotate (N), a reply with a part
  link, the link click, Show on model, Fit in view, Show only linked parts
  and Return (and Escape), Resolve/Reopen, edit and delete a message,
  delete a thread, pins, Reattach…; the References section, add and drop
  images, visibility, placement, Align view, Calibrate scale, Sketch over
  this, Remove reference; the system status line, Link, Accept changes,
  Unlink and Open in builder.

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
  workspace root. Since unified-selection-document (2026-10-01) the document
  registry, the one selection and the annotations service also survive a
  switch (§7).
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
  Actions. The builder and lesson SimSync chains now order against public
  `InspectViewSet::{Camera, Parts, Notes}`; their former private camera
  and part edges are closed by public-system-sets (uncompiled). Since cad-views-export the cameras
  are one feature with a public set: `camera::CameraSet` (Viewport →
  Navigate → Place, in SimSync), which the spatial view, Robot and CAD
  order their camera data before and their camera readers after
  (Phenomena writes none per frame), in place of the spatial view's
  private `camera_viewport`.
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
  since cad-print `jobs::open_local(path)` opens an existing local file or
  folder the same way (an absolute path only: "not an absolute path:
  {path}", "no such file or folder: {path}", refused before any process
  starts; cad-print runs it on a `Pool::Io` job for the assembly guide and
  the coupons' protocol folder); all three are `ChildProcess::detach`ed, so a reaper thread waits for the
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
  pinned widget Button/ActivateOnPress/Activate for ordinary activation
  alongside UI Button for discovery; Interaction remains styling and held gestures,
  and `bevy_ui_widgets::Slider` for sliders,
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
- *Status:* done 2026-10-01 pending verification (batch
  unified-selection-document; see
  [Documents, selection and annotations](#documents-selection-and-annotations-2026-10-01)):
  `document::DocumentRegistry`, `selection::Selection` with
  `SelectionAction`, and `annotations` with the `ui_kit::threads` panel.

### 8. Hardware in process in the native viewer

*Current implementation: leg-in-process, LIP1–LIP3, 2026-10-03, source review
only, unexecuted.* This supersedes the historical HTTP front-end description
below the dated 2026-09-30 heading. All five batch outcomes and the action-level
reference/replacement map are in [leg-in-process.md](../leg-in-process.md).

- **Placement and ownership.** The Leg calibration dock remains in Robot mode
  with its mirror and live Sync motors. `HardwareAction` retains its one apply
  and public ViewerSet pipeline; the kit entities keep their mode lifetimes.
  Shared `hardware::protocol` types carry request bodies, tolerant statuses,
  strict execution identity and virtual allowlist. `hardware::calibration`
  owns teaching, sweeps, tuning, campaigns, gait and lab actions;
  `hardware::bench` owns finite/live acquisition and records. The single
  process-wide `DeviceLease` excludes concurrent calibration/bench ownership;
  the physical serial descriptor also retains OS exclusivity. Disconnect
  calibration before acquiring that same device for Sync, and retry only after
  final release; a pending release still refuses acquisition.
- **Execution.** Local sessions return caller-owned workers; native starts them
  through `jobs::RunThread`, with discrete jobs through `Job`. No library starts
  a viewer thread. Physical transport calls `CalibrationBus` directly; serial
  configuration calls libc, without stty. Virtual calibration calls existing
  `Bench` through in-process packet I/O. Virtual Sync uses that Bench's host
  loop and explicitly reports `virtual_host_bench`; it does not emulate the
  other installed FPGA profiles or establish device-clock/physical parity.
  Physics and measured policies remain in shared Rust libraries.
- **Configuration.** Launch with `--hardware-config FILE` and optionally
  `--motor-bench-config FILE`. Configurations declare physical serial paths or
  explicit virtual identity/model/taught-window inputs. URL/token arguments
  `--hardware`, `--hardware-token-file`, `--motor-bench` and
  `--motor-bench-token-file` refuse by name. Older inactive preferences remain
  readable. Lessons use `SIM_BENCH_CONFIG`; obsolete `SIM_BENCH_URL` refuses.
  No native path starts, discovers, polls or waits for a hardware server.
- **Safety.** Immediate STOP/cancel and motion/gait heartbeats remain separate
  from ordinary serialized jobs. Acceptance-time epochs bind queued acquisition;
  STOP and generation replacement refuse old work. Queue expiry, sequence and
  owner checks, watchdog proofs, taught windows, bounded targets, measured limits
  and record publication stay authoritative. Late heartbeats cannot revive an
  expired lease. Focus loss, panel close, mode exit, disconnect and dropped or
  replaced owned handles latch STOP even if snapshots are idle and work is queued.
  An unwind guard attempts all-axis release before calibration bus close.
  STOP acceptance is a latch, **not stationary readback**; authoritative status
  separately reports release proof or uncertainty. Abrupt process termination
  cannot execute destructors or publish release. FPGA supervision remains
  independent; cut motor power when release is unverified.
- **Authorization and truth.** Physical motion needs the operator at the window;
  REST/system_ui cannot authorize it. Virtual automation requires fresh connection
  generation and strict explicit identity, remains allowlisted, and records are
  simulated. Unknown/stale/replaced frames refuse. Mirror layout and display
  bindings never mutate CAD geometry or physical definition. Lessons lab run is
  operator-only with focused window, supported-fixture checklist, shared watchdog
  proofs/limits and independent STOP; virtual lab_step stays outside the allowlist.
- **Compatibility and evidence.** Browser server examples are thin HTTP/static
  asset/startup adapters over these same applications. Browser paths and historical
  executed receipts remain; their old acceptance drivers are not in-process
  receipts. Current evidence is reading only; no builds, tests, launches,
  screenshots or hardware operation. See [hardware parity](../hardware-parity.md)
  and [operator checklist](../hardware-checklist.md). The selected CAD
  opening/display/body-selection/mass paths now use Rust and directly called
  OCCT (§9); other native CAD controls name their remaining migration gap.

### 9. CAD in Rust

*Decided 2026-09-30.* RoboCAD (Python/OCCT/Qt, about 27,700 lines) moves to
Rust with exact feature parity, in phases. RoboCAD is the reference throughout.

**Current implementation, 2026-10-03: cad-rust-physical-derivations (CD1–CD4).**
The selected opening/display/body-selection/mass-inspection paths use the shared
`sim-cad` archive and mass modules and a narrow C ABI bridge into OCCT, all in
the viewer process. Native ownership is the existing CAD document resource;
the document registry and shared Selection remain authoritative. Typed actions
start jobs; accepted job results precede mesh synchronization through CadSet and
ViewerSet. Numeric snapshots cross the job boundary; OCCT handles never do.
Local body/face/point picking uses displayed triangles; edge/vertex modes require
unmigrated exact topology. One shared availability check drives controls and typed
action validation. Exact topology, modelling history and command registry rows
show migration status rather than fictitious fetch/service progress. Empty
component overrides follow reference truthiness at regeneration while preserved
declarations retain their full downstream validation.
Current source evidence, inventory, limitations and unexecuted acceptance cases
are in [the bounded migration ledger](../cad-rust-physical-derivations.md).

The binding decision is a focused C++ bridge rather than a process/server adapter
or a general Rust CAD wrapper: the exact archive B-rep stream reader, tessellation,
per-solid enumeration, placement and full volume tensor queries are required
together. [BRepTools](https://dev.opencascade.org/doc/refman/html/class_b_rep_tools.html),
[BRepGProp](https://dev.opencascade.org/doc/refman/html/class_b_rep_g_prop.html)
and [BRepMesh_IncrementalMesh](https://dev.opencascade.org/doc/refman/html/class_b_rep_mesh___incremental_mesh.html)
document these kernel operations. Pinned native source inspection and linking
requirements are recorded in [sim-cad](../../crates/sim-cad/README.md).
Archive format version 1 and OCCT topology stream version are separate contracts;
existing archived bytes, unknown JSON and opaque entries remain owned unchanged.
Triangle meshes serve display only. Exact B-rep integration supplies mass,
centroid and full inertia, with declared measurements taking precedence.

OCCT access is serialized conservatively on a jobs worker. Native shapes are
constructed, queried and destroyed there before returning owned numeric buffers.
Cancellation is cooperative between kernel operations; an active OCCT operation
is not forcibly interrupted. Failed/cancelled replacement preserves the current
document, and captured generation/revision stamps reject stale completion.
No unmigrated native control may silently start or contact RoboCAD. Legacy
sources remain behaviour references. Full modelling/sketch edits, booleans,
fillets, print/flex derivations and physical simrobot export remain migration
gaps; this batch does not claim complete archive corpus or GUI parity.

**Historical migration plan and receipts below.** The user decision of
2026-10-03 supersedes its active server ownership and server-parity-gate policy.
It is retained to explain earlier implementations and evidence.

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
client. It was built and tested in its verification pass, verified at
a4fe42d3 (sim-spatial lib tests 154 passed, 1 ignored), and the user's
[docs/cad-checklist.md](../cad-checklist.md) compares it with RoboCAD step
by step. The ledger
[docs/cad-parity.md](../cad-parity.md) assigns every other RoboCAD feature
to one of the later epics below. The second, **cad-select-transform**
(2026-10-01, see [CAD selection and transform](#cad-selection-and-transform-2026-10-01)),
added sub-body selection, the transform gizmo, push/pull and offset,
measure, live dimensions, snapping and the numeric bar over a Rust port
of `units.evaluate`, with one read-only Python addition (sampled edge
polylines); verified at c0ed9b29 (sim-spatial lib 209 passed, 1 ignored;
bins 4; `cad_client` and `units` 59; api pytests 14). The third,
**cad-modify** (2026-10-01, see [CAD modify](#cad-modify-2026-10-01)),
added the op catalogue (54 operations as data, one apply path, primitive
placement and pick-then-form tools), the command surfaces built from
RoboCAD's command table (menus, toolbar, right-click menu, radials,
palette, parameter form), data-driven keys, read-only analysis overlays and
the inspector's pivot and transform editors, with five RoboCAD routes
(copy and paste with placement, control points, curvature comb,
continuity) and an `ArgConverter` fix for cutting with a node; verified at
e0996878 (sim-spatial lib 252 passed, 1 ignored; bins 4; `cad_client` 33; `units` 29; api pytests 48; sim-web wasm check clean). The fourth,
**cad-sketch** (2026-10-01, see [CAD sketch](#cad-sketch-2026-10-01)),
added the active plane and 2D snapping, the four plane tools, the 13
sketch tools on one data-driven interaction, the sketch edits and the
`cad_sketch` REST command, extrude, revolve, sweep, pipe, loft and fill,
with recorded Python fixes (`Service.edit_sketch` maps curve indices
first; `ArgConverter` passes `fill`'s node id; the verification pass fixed
`fill_hole` and made sketch refusals 4xx with a rolled-back failed create); it is verified at cc7ac194
(sim-spatial lib 293 passed, 1 ignored; bins 4; `cad_client` 47; `units` 29; api pytests 61; sim-web wasm check clean). The fifth,
**cad-views-export** (2026-10-01, see
[Shared camera and CAD views](#shared-camera-and-cad-views-2026-10-01)),
replaced every mode's own orbit camera with one shared camera
(`src/camera/`, 13 `camera_*` commands) and added RoboCAD's display
modes, grid, build plate, view cube, high contrast, section preview and
exact section, saved views, isolate/hide/show all, per-node tessellation
tolerance and the file workflows (new, open, save as, import with units,
export, drawing, render) with three api.py gap routes; it is written and
reviewed by reading (78553886, 9b1e5eec, f15766ea), and verified at
bcf0c56c. The sixth, **cad-physical-inspect** (2026-10-01, see
[CAD physical properties](#cad-physical-properties-2026-10-01)), added
the materials panel, the inspector's physical rows, the Robot panel with
its tools and dialogs, results and the stress overlay, physical export and
the live link into Robot mode, with two api.py gap routes
(`GET /results/nodes`, `GET /physical?planar=1`); it is written and
reviewed by reading (3fb34225, f26842fa and the review fixes in 697a15c1)
and pending verification. The seventh, **cad-print** (2026-10-01, see
[CAD print](#cad-print-2026-10-01)), added RoboCAD's Print menu: the wall
check and validation, overhang shading, the fastener and clearance tools,
split, strength, plan, whole or split, the assembly guide and coupons as
RoboCAD jobs started through the edit path, one poller of RoboCAD's print
jobs with progress, cancel and the Print jobs section, and the print
study's results in the stress overlay, with one api.py gap route
(`GET /print/study`); it is written and reviewed by reading (35ea6de0,
17f90d08, review fixes 6c6b1a5a, docs 9afd63f5, fixes 0a1147b3) and
pending verification. After it, the structural one-text-entry (see
[One text entry](#one-text-entry-2026-10-01)) moved every CAD text field
onto the kit field. The eighth, **cad-organize** (2026-10-01, see
[CAD organize](#cad-organize-2026-10-01)), added the outliner's
organization (search, expand and collapse, multi-select, inline rename,
drag-and-drop, the context menu, the active group, New group), RoboCAD's
comment threads as the fourth `annotations::ThreadSource` in the one
`ui_kit::threads` panel (Annotate, pins, part links, Show on model, Fit
in view, Show only linked parts), the References section with textured
image planes and the calibrate tool, and the linked system file with Open
in builder as an in-window switch to Build mode, with one api.py gap
route (`GET /nodes/{id}/image`); it is written and reviewed by reading
and pending verification: nothing compiled; only the gap route's pytest ran (2 passed at b476b28e; the third test added after review is unverified). Its components and system-graph rows
moved to cad-components. cad-experiments-motion is implemented by T43 (reading only); next structural batch is selected by the Director; public-system-sets
is done by reading and remains uncompiled pending verification.

#### Later CAD epics (planned 2026-09-30)

These follow cad-mode, which covers opening and attaching, the tree, the
bodies, picking, body selection, the inspector, the basic attribute edits,
delete, undo/redo, save, commands and keys. Each later epic is a client of
RoboCAD's REST service, like cad-mode. Every edit still goes through
RoboCAD's command layer. Every Ops method is already callable through the
`cad_op` REST command, so these epics build the *viewer UI*. Row-level scope
is in [docs/cad-parity.md](../cad-parity.md) (773 rows: 113 cad-mode, 637
later, 23 deliberately different when planned; the planned cad-tools' 179
rows were split 2026-10-01 into cad-select-transform, 63, and cad-modify,
116; after cad-modify 245 are done by reading, 458 later and 70
deliberately different; after cad-sketch 286, 398 and 89; after
cad-views-export see the ledger's Counts). Eight gaps there have no
headless route (10 before cad-organize added `GET /nodes/{id}/image`; 15
before cad-physical-inspect added `GET /results/nodes`
and `GET /physical?planar=1`; 17 before cad-views-export added `POST
/save/thumbnail` and `GET /import/units`; 22 before cad-modify added five
routes). Each needs a new route in `cad/robocad/api.py`, or a Rust port gated
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
2. **cad-modify** (116 rows; the second half of the planned cad-tools).
   *Done 2026-10-01, verified at e0996878 (sim-spatial lib 252 passed, 1 ignored; bins 4; `cad_client` 33; `units` 29; api pytests 48; sim-web wasm check clean) (see
   [CAD modify](#cad-modify-2026-10-01)): 77 rows done by reading, 39
   deliberately different; the five gaps below now have routes.* The
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
   category. Routes: `POST /ops/*`, `POST /nodes`, `GET /commands`. Gaps
   when planned: copy and paste with placement, reading control points,
   curvature comb and continuity check (each needed a Python route; the
   epic added `POST /clipboard/copy`, `POST /clipboard/paste` and `GET
   /nodes/{id}/control_points|curvature_comb|continuity`).
3. **cad-sketch** (60 rows). *Written 2026-10-01 and reviewed by reading,
   verified at cc7ac194 (sim-spatial lib 293 passed, 1 ignored; bins 4; `cad_client` 47; `units` 29; api pytests 61; sim-web wasm check clean) (see [CAD sketch](#cad-sketch-2026-10-01)):
   35 rows done by reading, 25 deliberately different, none open; 6
   earlier rows that waited for the active plane became done by reading.*
   The active plane, construction planes (from a
   face, three points, two points and the camera, midplane), the 13 sketch
   tools, sketch offset/fillet/join and the REST-only edits (trim, split,
   extend, rebuild, vertices), extrude and revolve with boolean modifiers,
   sweep, pipe, loft and fill. Routes: `GET/POST /nodes/{id}/sketch`,
   `POST /nodes {"kind": "sketch"}`, `POST /ops/plane_*`,
   `POST /ops/extrude|revolve|sweep|pipe|loft|fill`. Sketch curves are
   drawn from the sketch geometry (`GET /nodes/{id}/sketch`, sampled as
   RoboCAD's viewport samples them); curve nodes keep the sampled edge
   polylines cad-select-transform added (`GET /nodes/{id}/edges?samples=N`). RoboCAD has no sketch constraints, so
   there is nothing to port there. The dead `sketch.arc` key (A) is bound
   to the three-point arc.
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
   *Done by reading 2026-10-01* (78553886, 9b1e5eec, f15766ea; see
   [Shared camera and CAD views](#shared-camera-and-cad-views-2026-10-01)),
   verified at bcf0c56c. Every row is now done by reading or
   deliberately different; the split and the ledger's totals are in
   [docs/cad-parity.md](../cad-parity.md)'s Counts. Gaps closed by new
   routes: the save thumbnail (`POST /save/thumbnail`) and the mesh-unit
   guess (`GET /import/units`), plus `POST /new`. Deliberately different:
   the autosave interval and failure report, `/capture` and `/screenshot`,
   the Blender link, web share and SpaceMouse. The camera work also
   replaced every other mode's orbit camera with the shared one.
5. **cad-physical-inspect** (82 rows). *Done 2026-10-01 pending
   verification; see [CAD physical properties](#cad-physical-properties-2026-10-01).
   64 `done-by-reading`, 18 `deliberately different`; both gaps closed by
   new routes: `GET /results/nodes` (per-node results, margins and yield
   strength; cad-print's print overlay reuses it for its per-node
   results) and `GET /physical?planar=1`.* As planned: the materials panel and engineering
   properties, colour, the Robot panel (summary, tree, margins, issues),
   motor, joint, sensor and cable tools and dialogs, joint editing and
   joint-physics overrides, battery/control/uncertainty, the exact
   multi-selection measurement, physical export, results and
   identification, the stress overlay, and the live simulation link (on
   save, export `simrobot.json` and reload robot mode in this app; as
   built, the viewer reads `GET /physical` without `path=` and writes the
   file itself on a job). Routes: `GET /robot`, `GET /motors`, `/sensors`,
   `/cables`, `PUT /battery|control|uncertainty`, `GET/POST /materials`,
   `GET /physical`, `GET /results`, `POST /results/load`,
   `POST /identification/apply`, `/actuator-profiles`,
   `POST /ops/add_joint|set_joint|add_motor|attach_motor|set_joint_physics|set_material_props`.
   Gaps (closed, see above): per-node results (inspector line, stress
   overlay, margins) and the planar export variant.
6. **cad-print** (33 rows). *Done 2026-10-01 pending verification; see
   [CAD print](#cad-print-2026-10-01). 25 `done-by-reading`, 8
   `deliberately different`; one gap route added, `GET /print/study` (the
   print study and split groups), which no flagged row needed.* As
   planned: wall check, validate, overhang shading,
   fastener and clearance tools, split for printing, strength, plan,
   strength-or-split, assembly guide, coupons, and the job list with
   progress and cancel. Routes: `GET /nodes/{id}/thin|validate`,
   `/print/*`, `POST /ops/fastener_hole|clearance|print_split`. Gaps,
   both closed: the print overlay's per-node results (shared with 5), by
   cad-physical-inspect's `GET /results/nodes`, whose `nodes` keep the
   print-study blocks (only its `margins` leave them out); and the print
   study with the split groups, which RoboCAD's handlers read in process,
   by the new `GET /print/study` (read-only, other methods 405; pytests
   `cad/tests/test_api_print_routes.py`).
7. **cad-organize** (108 rows when planned; planned as cad-annotations).
   *Done 2026-10-01 pending verification, written and reviewed by
   reading (nothing compiled; only the gap route's pytest ran (2 passed at b476b28e; the third test added after review is unverified)); see [CAD organize](#cad-organize-2026-10-01).
   The Director moved 38 rows (components and the system graph) to
   cad-components; of the other 70, 44 `done-by-reading` and 26
   `deliberately different`; four earlier outliner context-menu rows
   became `done-by-reading`; the reference-pixel gap closed by the new
   `GET /nodes/{id}/image`.* Outliner organization (search, groups,
   drag-and-drop, move to group, active group, inline rename,
   multi-select), comments and threads with pins and part links (the
   fourth `annotations::ThreadSource`, drawn by `ui_kit::threads`),
   references (images, placement, calibration, the linked system file and
   Open in builder, an in-window switch to Build mode). Routes:
   `/threads*`, `/comments/{id}`, `GET /nodes/{id}/image`,
   `POST /ops/group|move_nodes|set_active_group|set_locked|import_references|update_reference|calibrate_reference|link_system|unlink_system|refresh_system_link|system_status`.
   Gaps, both closed: reference image pixels in the viewport and in the
   list's preview (the same data).
8. **cad-components** (38 rows; split out of cad-organize by the Director
   2026-10-01). The components library dock (definitions, find, make from
   selection and make linked, new parametric, place, edit defaults with
   variants and nested parameters, import and export, the saved library),
   occurrences and their overrides, detach, component jobs with rebuild
   progress and cancel, the system graph (type chooser, the component
   form, the geometry rule, the connection graph), `set_component_graph`
   and `transform_components`. *Why separate:* it shares `sim_system`'s
   graph types with Build mode and needs its own design (one graph model
   for the builder and CAD, not a second editor). Routes:
   `GET /components`, `/component-jobs/{id}` (headless and Qt; shared service owner), `GET/PUT /system`,
   `/system/components|connections`,
   `POST /ops/make_component|place_component|set_component_parameters|set_component_overrides|detach_component|import_component|export_component|…`.
   Closed in T42: `/component-recipes` serves authoritative geometry-rule recipes and inputs; `/component-library` discovers archives off the UI thread. See the T42 section below.
9. **cad-experiments-motion** (63 rows; implemented by T43, reading only).
   Native experiment editors, checks/runs/cancel, profiles/catalogue, baseline
   comparison, linked sources, restored inputs, captured/candidate review,
   model scripts/batches and kinematic pose/program/export controls share typed
   handlers. Isolated geometry routes use captured_document/Candidates.document;
   `/motion/pose` and `/motion/sample` use the reference solver without Qt.
   Each caller carries validated prior resolved state to preserve linkage branches;
   preview/export continuations are independent. Native playback/export uses
   native actions and viewport frames, not GUI-only `/motion` controls. Python/OCCT,
   the shared registry/experiment executables and local ffmpeg remain dependencies.
   No derivation port, executed parity or legacy retirement is claimed.

## Bevy features to use

These are verified in the official 0.17, 0.18 and 0.19 release notes. Before
using any of them, read the 0.19.1 API docs and the migration guides; don't rely
on memory.

| Feature | Release | Use it for |
|---|---|---|
| Event / observer overhaul | 0.17 | the action layer (§3) |
| Headless standard widgets (`bevy_ui_widgets`); Feathers | 0.17–0.19 | the UI kit (§6): the slider is headless; Feathers is not used (its look is not the builder's) |
| Text input (`EditableText`, `EditableTextInputPlugin`); input focus (`bevy_input_focus`) | 0.19 | `InputFocus` adopted as the one keyboard focus; `EditableText` not adopted (no placeholder, submit or AccessKit support; editor nodes must persist across the kit's panel rebuilds; no OS clipboard without the `system_clipboard` feature): every field is the kit's `ui_kit::text` field, a `TextDraft` driven by one system on `InputFocus` (see [One text entry](#one-text-entry-2026-10-01)) |
| `ViewportNode` | 0.17 | 3D views inside panels (schematic beside spatial, inspector previews); not adopted yet: viewports are computed from dock sizes (epic viewport-nodes) |
| First-party camera controllers | 0.18 | not adopted (cad-views-export): 0.19.1 has no orbit controller, `FreeCamera` (`bevy_camera_controller`, in neither the lockfile nor the registry) grabs the cursor on a right-click and keeps state REST cannot set as Place's `Fly` yaw/pitch/speed, and `PanCamera` is 2D; the hand-rolled orbit cameras became one shared module instead (`src/camera/`, see [Shared camera and CAD views](#shared-camera-and-cad-views-2026-10-01)) |
| Easy screenshot and video recording | 0.18 | `ui_capture` and run recordings |
| App settings | 0.19 | SettingsGroup reflection/registration adopted through `app/settings` (T45); jobs owns all disk work; stock SettingsPlugin not installed (see [preferences](../viewer-preferences.md)) |
| Interactive transform gizmo, infinite grid | 0.19 | build-mode placement (display-only); the infinite grid is not adopted for CAD (cad-views-export): CAD draws RoboCAD's `_draw_grid` as gizmo lines (10 mm step, ±200 mm, every 5th line major, X/Y/Z axis colours) because `InfiniteGrid` is infinite and fading, marks every 10th line, colours only X and Z, needs the `bevy_dev_tools` feature and cannot be cut by the section plane |
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

**Current focus (the user, 2026-10-03), ahead of everything below:**
everything in Rust, in one process. The viewer must not rely on the RoboCAD
server or on a robot driver server. First the leg driven in process (the
calibration and motor-bench server logic moves into shared libraries, with
every safety rule unchanged), then CAD in Rust (§9: derivations, then the
geometry kernel from Rust, `.rcad` compatible). This supersedes keeping
RoboCAD available until a parity harness passes; its source stays the
reference for behaviour. The earlier focus (calibration, sim and leg, CAD
editor and annotations, the REST-built rover) is complete by reading.

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
   is verified at c0ed9b29 (sim-spatial lib 209 passed, 1 ignored; bins 4;
   `cad_client` and `units` 59; api pytests 14). **cad-modify** (2026-10-01;
   see [CAD modify](#cad-modify-2026-10-01)) is verified at e0996878
   (sim-spatial lib 252 passed, 1 ignored; bins 4; `cad_client` 33; `units` 29; api pytests 48; sim-web wasm check clean).
   **cad-sketch** (2026-10-01; see [CAD sketch](#cad-sketch-2026-10-01))
   is verified at cc7ac194 (sim-spatial lib 293 passed, 1 ignored; bins 4; `cad_client` 47; `units` 29; api pytests 61; sim-web wasm check clean).
   **cad-views-export** (2026-10-01; see
   [Shared camera and CAD views](#shared-camera-and-cad-views-2026-10-01))
   is verified at bcf0c56c (sim-spatial lib 356 passed, 1 ignored; bins 4;
   `cad_client` 65; `units` 29; RoboCAD pytests 396; sim-web wasm check
   without errors; see [Verification result](#verification-result-bcf0c56c)).
   Before it, the structural **unified-selection-document** (§7,
   2026-10-01, done pending verification; see
   [Documents, selection and annotations](#documents-selection-and-annotations-2026-10-01))
   gave every mode one document registry, one selection and one
   annotations service, which cad-physical-inspect's inspector builds on.
   **cad-physical-inspect** (2026-10-01; see
   [CAD physical properties](#cad-physical-properties-2026-10-01)) is done
   pending verification (written and reviewed by reading in 3fb34225,
   f26842fa and the review fixes committed in 697a15c1; nothing compiled
   or run).
   **cad-print** (2026-10-01; see [CAD print](#cad-print-2026-10-01)) is
   done pending verification (written and reviewed by reading in 35ea6de0,
   17f90d08, 6c6b1a5a, 9afd63f5 and 0a1147b3; nothing compiled or run).
   Then the structural **one-text-entry** (2026-10-01; see
   [One text entry](#one-text-entry-2026-10-01)), done pending
   verification (by reading; nothing compiled or run).
   **cad-organize** (2026-10-01; see [CAD organize](#cad-organize-2026-10-01))
   is done pending verification (written and reviewed by reading;
   nothing compiled; only the gap route's pytest ran (2 passed at b476b28e; the third test added after review is unverified)); its components and system-graph rows became the new epic
   cad-components.
   **cad-components**, **public-system-sets** and **cad-experiments-motion**
   are implemented by reading, pending verification and uncompiled.
   T43 source review was accepted at 4d2725f2/e6f6ed10; T44 was accepted by source review at 8a7c0cd7/fe2a6eb1; the phase-2 harness is unexecuted; no phase-1
   CAD family remains deferred in the source ledger. Exact parity remains unproven.
8. **Parity harness** (§9 phase 2). T44 accepted by source review at 8a7c0cd7/fe2a6eb1; compilation, fixtures and parity remain unexecuted.
9. **Derivations in Rust** (§9 phase 3). Several epics, one derivation family
   each.
10. **OCCT from Rust** (§9 phase 4). Several epics, one kernel area each.
11. **Fold in `sim-app`.** Bring its scenes in as modes, or retire them.
    *Done 2026-09-30, verified at 80b5997e (batch fold-sim-app; sim-spatial
    lib tests 172 passed, 1 ignored; workspace `--locked` check clean; see
    [Fold in sim-app](#fold-in-sim-app-2026-09-30)).*

**Bevy-practice epics** (added 2026-10-01; structural; the gaps are listed under
"Bevy-practice gaps" in "Where it is today"). one-text-entry and
cad-organize and **public-system-sets** are done (pending verification,
uncompiled). T42 implements cad-components and T43 implements cad-experiments-motion
by accepted source review at 4d2725f2/e6f6ed10; T44 was accepted by source review at 8a7c0cd7/fe2a6eb1; T45 realizes persisted settings by reading.
Interleave structural work with later CAD migration phases, keeping at least
one epic in three structural:

- **public-system-sets.** Done by reading, pending verification and uncompiled;
  see [Public system sets](#public-system-sets-2026-10-01). No system orders itself against a function in
  another feature's folder.
  - Input gets public sub-steps: the REST poll first, then buttons, keys and
    picks remain unordered in Window. This replaces 34 serve ordering occurrences.
  - A feature that others order against exposes a public set, as
    `camera::CameraSet` does: Robot frames (`robot::apply_frames`), the
    robot's action apply, CAD sync results (`cad::sync::receive`), and the
    spatial view's camera sync (closing §2's open builder and lesson
    item).
  - A lib test scans `src/` and names any cross-folder `.after` / `.before`
    on a function path, like `jobs::tests::threads_are_started_only_in_jobs`.
- **viewport-nodes.** 3D views inside docks are `ViewportNode`s, so the
  layout owns placement and no system computes `Camera.viewport` from dock
  sizes.
  - Start with a spike on one view (the split schematic in `view.rs`).
    Check gizmos, anti-aliasing and HDR on an image target, picking through
    the node (Bevy's `viewport_picking`, and the cursor ray CAD's
    `MeshRayCast` builds, which must become node-local), and the REST
    `screenshot`.
  - Then move the rest, or record per view why not, and update §6 and the
    "Bevy features to use" table.
- **persisted-settings.** T45.1–T45.3 implemented by reading, uncompiled.
  One settings owner replaces independent recents/hardware persistence and
  document-local print defaults. Original migration files and unknown data
  remain preserved; protected inputs block publication. Source CAD and the
  actuator registry remain authoritative. See [viewer preferences](../viewer-preferences.md).

After that, feature work resumes on the target shape. The large-file debt
was paid off by split-large-files (2026-10-01, see
[Split large files](#split-large-files-2026-10-01)), verified at 9765dcb6
(code as of 57f7d447; sim-spatial lib 253 passed, 1 ignored; bins 4;
`hardware_client` 18; `cad_client` 33); the source-size guard
(`app::tests::source_files_stay_small`, cap 750 non-test lines) keeps it
paid, so split a file along a seam when a change would take it past the cap.

## Open questions

- Whether Bevy Remote Protocol should back the REST surface. Adopt it only if it
  removes code and keeps `sim_api`'s guarantees.
- ~~The CAD (Python/OCCT) boundary~~: resolved 2026-09-30. CAD moves to Rust
  (§9).
- Which Rust OCCT bindings to use or extend (for example `opencascade-sys`
  through `cxx`), and how to keep the C++ build out of the fast path.

## Public system sets (2026-10-01)

T41.1, T41.2 and T41.3 implement the public ordering contract by reading;
**uncompiled and pending verification**. No viewer, test or build was run.
REST names/shapes, key predicates and mode conditions are unchanged.
No effective ordering relation or ViewerSet phase was intentionally changed.

| Public set | Owner/configuration home | Members and relation |
|---|---|---|
| `InputSet::{Rest, Window}` | `app::ModesPlugin` | Chained inside Input; only serve is Rest. Former serve-dependent inputs are Window. |
| `RobotSet::{Actions, Frames}` | `robot::configure_sets`, RobotPlugin | One robot action handler in Actions; only apply_frames in SimSync Frames. |
| `CadSet::{Results, Mesh, Highlight, Plane, View}` | `cad::configure_sets`, CadCorePlugin | Singleton service receive, mesh sync, highlight, plane sync and view snapshot; existing producer chains preserved. |
| `CadKeySet::{Gate, Focus, Keys, ToolKeys, NumericEntry, EscapeTool, Escape}` | same CAD configuration | Gate precedes Keys and ToolKeys; Focus precedes Keys; Gate → EscapeTool → Escape → ToolKeys chained, EscapeTool before Keys; NumericEntry (the numeric bar) inside Focus (cad-checklist-traces, 2026-10-02). Other relations remain explicit at readers. |
| `InspectViewSet::{Notes, Link, Camera, Parts}` | `inspect_view::configure_sets`, SpatialViewerPlugin | Singleton notes navigation, linked exchange, camera synchronization and part update in SimSync. |
| `TextInputSet` | `ui_kit::text::configure_sets`, TextEntryPlugin | PreUpdate input after Bevy input and UI focus; picker sync before and picker modal keys after. |
| `CameraSet` | CameraPlugin (existing) | Viewport → Navigate → Place in SimSync, unchanged. |

Decisions: Rest → Window deliberately retains the previous partial order
between buttons, keys and picks; splitting those into a total chain would add
relations. Inputs previously unordered against serve stay directly in Input
(e.g. picker clicks and display cube presses). `lesson_screen_requests` stays
after serve and unordered against Window. CAD Focus is not chained against
Gate, and Keys is not chained against ToolKeys: only existing relations are
expressed. Tree search's new Window ancestry was already implied by
serve → tree readers → gate → search. Top-level files and same-named folders
are one feature (`builder.rs` and `builder/**`); notes, linked, animation and
physics_view are separate features. Local function edges remain when they
express unique intra-feature relations. No allowlist exceptions are needed.

The baseline at `525d5276` has **82 grep matching lines**, **107 qualified
lowercase function-path occurrences** (including multiline calls), and five
bare-function occurrences. Thus the old 94/66/24 and later 25 figures are
historical/stale; the actual serve occurrence count is **34**. The table below
records all 112 baseline occurrences, including retained edges, using baseline
file:line under `crates/sim-spatial/src`. No tuple-target function edges were
found in this baseline; the guard also handles multiline and tuple arguments.
All rows use Update unless explicitly marked PreUpdate. “Cross” resolves the
target to its top-level feature, including imported bare aliases.

| Baseline file:line / source system or chain | Old edge | Schedule / phase | Cross | Replacement / reason |
|---|---|---|---|---|
| `app/switch/mod.rs:339` / `picker::sync` | `before(crate::ui_kit::text::input::keys)` | PreUpdate | yes | same before TextInputSet; Draft sync precedes typing; modal picker keys read after typing. |
| `app/switch/mod.rs:340` / `picker::keys` | `after(crate::ui_kit::text::input::keys)` | PreUpdate | yes | same after TextInputSet; Draft sync precedes typing; modal picker keys read after typing. |
| `builder.rs:532` / `buttons → keys` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `builder.rs:539` / `frame_timing … clear_for_learn chain` | `before(crate::inspect_view::sync_camera)` | Update / SimSync | yes | same before InspectViewSet::Camera; Builder docks and lesson viewport/cues reach camera before synchronization. |
| `builder.rs:540` / `frame_timing … clear_for_learn chain` | `before(update_parts)` | Update / SimSync | yes | same before InspectViewSet::Parts; Scene rebuild/selection precedes parts; placement reads posed parts after. |
| `builder.rs:545` / `placement::update` | `after(update_parts)` | Update / SimSync | yes | same after InspectViewSet::Parts; Scene rebuild/selection precedes parts; placement reads posed parts after. |
| `builder.rs:546` / `apply_preview → draw_handles` | `after(placement::update)` | Update / SimSync | no | retained intra-feature function edge; Local producer data reaches its consumer before reading/drawing. |
| `builder.rs:547` / `discussion::hover` | `after(notes::update)` | Update / SimSync | yes | same after InspectViewSet::Notes; Notes navigation updates selection before projection and hover. |
| `builder.rs:548` / `markers::sync` | `after(placement::apply_preview)` | Update / SimSync | no | retained intra-feature function edge; Local producer data reaches its consumer before reading/drawing. |
| `builder.rs:548` / `markers::sync` | `after(discussion::hover)` | Update / SimSync | no | retained intra-feature function edge; Local producer data reaches its consumer before reading/drawing. |
| `builder.rs:549` / `ui_api::collect` | `after(markers::sync)` | Update / SimSync | no | retained intra-feature function edge; Local producer data reaches its consumer before reading/drawing. |
| `builder.rs:549` / `ui_api::collect` | `after(ui::rebuild_panel)` | Update / SimSync | no | retained intra-feature function edge; Local producer data reaches its consumer before reading/drawing. |
| `cad/attach.rs:66` / `input` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/attach.rs:66` / `input` | `before(super::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/display/mod.rs:549` / `entry::input` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/display/mod.rs:550` / `entry::input` | `before(crate::cad::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/display/mod.rs:554` / `preview → materials → edges_sync` | `after(crate::cad::mesh::highlight)` | Update / SimSync | no | same after CadSet::Highlight; Material overrides land before highlight; display reads after highlight. |
| `cad/files/mod.rs:587` / `form::input` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/files/mod.rs:587` / `form::input` | `before(crate::cad::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/files/mod.rs:590` / `jobs::receive` | `before(crate::cad::sync::receive)` | Update / JobResults | no | same before CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `cad/inspector/mod.rs:65` / `editor_entry` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/inspector/mod.rs:66` / `editor_entry` | `before(super::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/inspector/physical_edit.rs:324` / `entry::entry` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/inspector/physical_edit.rs:325` / `entry::entry` | `before(crate::cad::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/inspector/physical_edit.rs:339` / `exact::sync, refresh::sync` | `after(crate::cad::sync::receive)` | Update / JobResults | no | same after CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `cad/materials/mod.rs:387` / `panel::input` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/materials/mod.rs:388` / `panel::input` | `before(crate::cad::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/mod.rs:138` / `analysis_overlay::receive` | `after(sync::receive)` | Update / JobResults | no | same after CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `cad/mod.rs:173` / `keys::keys` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/mod.rs:173` / `keys::keys` | `after(keys::gate)` | Update / Input | no | CadKeySet::Gate → Keys; Pending chord/key ownership arbitrates readers. |
| `cad/ops/interact.rs:111` / `pointer` | `after(crate::cad::view::update)` | Update / SimSync | no | same after CadSet::View; Current camera snapshot before interaction. |
| `cad/ops/interact.rs:111` / `pointer` | `after(crate::cad::mesh::sync)` | Update / SimSync | no | same after CadSet::Mesh; Current geometry before ray casts, previews and material overrides. |
| `cad/panel.rs:271` / `name::buttons → name_entry` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/panel.rs:271` / `name::buttons → name_entry` | `before(super::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/pick.rs:167` / `pointer` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/pick.rs:168` / `pointer` | `after(super::keys::gate)` | Update / Input | no | same after CadKeySet::Gate; Pending chord/key ownership arbitrates readers. |
| `cad/pick.rs:169` / `pointer` | `after(super::numeric::entry)` | Update / Input | no | retained intra-CAD function edge; Numeric focus, Escape priority or local dataflow remains exactly ordered. |
| `cad/print/checks.rs:431` / `receive` | `after(crate::cad::sync::receive)` | Update / JobResults | no | same after CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `cad/print/fastener_tool.rs:197` / `click` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/print/jobs_tracker.rs:481` / `poll` | `after(crate::cad::sync::receive)` | Update / JobResults | no | same after CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `cad/print/studies.rs:411` / `sync` | `after(crate::cad::sync::receive)` | Update / JobResults | no | same after CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `cad/references/calibrate.rs:309` / `click` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/references/calibrate.rs:310` / `escape` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/references/calibrate.rs:310` / `escape` | `after(crate::cad::keys::gate)` | Update / Input | no | same after CadKeySet::Gate; Pending chord/key ownership arbitrates readers. |
| `cad/references/calibrate.rs:310` / `escape` | `before(crate::cad::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/references/calibrate.rs:310` / `escape` | `before(crate::cad::transform::keys)` | Update / Input | no | same before CadKeySet::ToolKeys; Calibration/annotation Escape precedes Select cancellation. |
| `cad/references/drop.rs:60` / `drops` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/references/input.rs:306` / `input` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/references/input.rs:306` / `input` | `before(crate::cad::keys::gate)` | Update / Input | no | same before CadKeySet::Gate; Pending chord/key ownership arbitrates readers. |
| `cad/references/input.rs:306` / `input` | `before(crate::cad::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/references/reads.rs:242` / `receive` | `after(crate::cad::sync::receive)` | Update / JobResults | no | same after CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `cad/results/forms.rs:411` / `input` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/results/forms.rs:411` / `input` | `before(crate::cad::keys::gate)` | Update / Input | no | same before CadKeySet::Gate; Pending chord/key ownership arbitrates readers. |
| `cad/results/forms.rs:411` / `input` | `before(crate::cad::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/results/mod.rs:496` / `link::receive` | `after(super::sync::receive)` | Update / JobResults | no | same after CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `cad/results/overlay.rs:364` / `paint` | `after(crate::cad::mesh::sync)` | Update / SimSync | no | same after CadSet::Mesh; Current geometry before ray casts, previews and material overrides. |
| `cad/results/overlay.rs:364` / `paint` | `before(crate::cad::mesh::highlight)` | Update / SimSync | no | same before CadSet::Highlight; Material overrides land before highlight; display reads after highlight. |
| `cad/robot/mod.rs:160` / `data::sync` | `after(super::sync::receive)` | Update / JobResults | no | same after CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `cad/robot/panel.rs:516` / `double_click` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/robot/tools.rs:463` / `click::click` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/robot/tools.rs:464` / `settle` | `after(super::data::sync)` | Update / JobResults | no | retained intra-CAD function edge; Numeric focus, Escape priority or local dataflow remains exactly ordered. |
| `cad/sketch/extrude.rs:136` / `pointer` | `after(crate::cad::view::update)` | Update / SimSync | no | same after CadSet::View; Current camera snapshot before interaction. |
| `cad/sketch/extrude.rs:136` / `pointer` | `after(crate::cad::mesh::sync)` | Update / SimSync | no | same after CadSet::Mesh; Current geometry before ray casts, previews and material overrides. |
| `cad/sketch/extrude.rs:136` / `pointer` | `after(crate::cad::sketch::plane::sync)` | Update / SimSync | no | same after CadSet::Plane; Current plane frames before plane drawing/extrusion. |
| `cad/sketch/interact.rs:105` / `pointer` | `after(crate::cad::view::update)` | Update / SimSync | no | same after CadSet::View; Current camera snapshot before interaction. |
| `cad/sketch/interact.rs:105` / `pointer` | `after(crate::cad::mesh::sync)` | Update / SimSync | no | same after CadSet::Mesh; Current geometry before ray casts, previews and material overrides. |
| `cad/sketch/plane.rs:72` / `picks` | `after(crate::cad::view::update)` | Update / SimSync | no | same after CadSet::View; Current camera snapshot before interaction. |
| `cad/sketch/plane.rs:72` / `picks` | `after(crate::cad::mesh::sync)` | Update / SimSync | no | same after CadSet::Mesh; Current geometry before ray casts, previews and material overrides. |
| `cad/sketch/plane.rs:72` / `picks` | `after(sync)` | Update / SimSync | no | retained intra-CAD function edge; Numeric focus, Escape priority or local dataflow remains exactly ordered. |
| `cad/sketch/plane_draw.rs:60` / `quads` | `after(super::plane::sync)` | Update / SimSync | no | same after CadSet::Plane; Current plane frames before plane drawing/extrusion. |
| `cad/surfaces/mod.rs:323` / `form::input … keys::gate chain` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/surfaces/mod.rs:326` / `form::input … keys::gate chain` | `before(super::numeric::entry)` | Update / Input | no | retained intra-CAD function edge; Numeric focus, Escape priority or local dataflow remains exactly ordered. |
| `cad/threads/annotate.rs:206` / `click` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/threads/input.rs:236` / `input` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/threads/input.rs:236` / `input` | `before(crate::cad::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/threads/input.rs:241` / `escape` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/threads/input.rs:242` / `escape` | `after(crate::cad::keys::gate)` | Update / Input | no | same after CadKeySet::Gate; Pending chord/key ownership arbitrates readers. |
| `cad/threads/input.rs:243` / `escape` | `after(crate::cad::references::calibrate::escape)` | Update / Input | no | retained intra-CAD function edge; Numeric focus, Escape priority or local dataflow remains exactly ordered. |
| `cad/threads/input.rs:244` / `escape` | `before(crate::cad::transform::keys)` | Update / Input | no | same before CadKeySet::ToolKeys; Calibration/annotation Escape precedes Select cancellation. |
| `cad/threads/pins.rs:211` / `press` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/threads/read.rs:222` / `sync` | `after(crate::cad::sync::receive)` | Update / JobResults | no | same after CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `cad/transform/mod.rs:269` / `numeric::entry` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/transform/mod.rs:269` / `numeric::entry` | `before(super::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/transform/mod.rs:271` / `transform::keys` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/transform/mod.rs:271` / `transform::keys` | `after(super::keys::gate)` | Update / Input | no | same after CadKeySet::Gate; Pending chord/key ownership arbitrates readers. |
| `cad/transform/mod.rs:271` / `transform::keys` | `after(super::numeric::entry)` | Update / Input | no | retained intra-CAD function edge; Numeric focus, Escape priority or local dataflow remains exactly ordered. |
| `cad/transform/mod.rs:281` / `track_selection … previews chain` | `after(super::view::update)` | Update / SimSync | no | same after CadSet::View; Current camera snapshot before interaction. |
| `cad/transform/mod.rs:282` / `track_selection … previews chain` | `after(super::mesh::sync)` | Update / SimSync | no | same after CadSet::Mesh; Current geometry before ray casts, previews and material overrides. |
| `cad/tree.rs:170` / `popup::input → rows → fields` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/tree.rs:171` / `popup::input → rows → fields` | `before(super::keys::gate)` | Update / Input | no | same before CadKeySet::Gate; Pending chord/key ownership arbitrates readers. |
| `cad/tree.rs:177` / `search` | `after(super::keys::gate)` | Update / Input | no | same after CadKeySet::Gate; Pending chord/key ownership arbitrates readers. |
| `cad/tree.rs:177` / `search` | `before(super::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/views/mod.rs:464` / `panel::input` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `cad/views/mod.rs:465` / `panel::input` | `before(crate::cad::keys::keys)` | Update / Input | no | CadKeySet::Focus → Keys (source membership); Field focus/consumption precedes global shortcuts. |
| `cad/views/mod.rs:467` / `sync` | `after(crate::cad::sync::receive)` | Update / JobResults | no | same after CadSet::Results; Service answers land before revision-dependent readers; file jobs land first. |
| `camera/mod.rs:479` / `input::keys` | `after(actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `inspect_view/mod.rs:319` / `inspect::input → notes::clicks → overlay_clicks` | `after(app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `inspect_view/mod.rs:327` / `projection::project_selection` | `after(notes::update)` | Update / SimSync | yes | same after InspectViewSet::Notes; Notes navigation updates selection before projection and hover. |
| `inspect_view/mod.rs:327` / `projection::project_selection` | `before(linked::sync_link)` | Update / SimSync | yes | same before InspectViewSet::Link; Projection updates shared selection before exchange with peer. |
| `inspect_view/mod.rs:327` / `projection::project_selection` | `before(update_parts)` | Update / SimSync | no | same before InspectViewSet::Parts; Scene rebuild/selection precedes parts; placement reads posed parts after. |
| `inspect_view/mod.rs:351` / `view::animate … inspect::publish chain` | `after(animation::draw_markers)` | Update / Present | yes | ViewerSet::SimSync → Present (redundant edge removed); Markers are SimSync; presentation already follows via pipeline. |
| `lesson/mod.rs:644` / `keys → buttons → seek → narrate::seek → sliders` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `lesson/mod.rs:652` / `poll … sketch_dots chain` | `before(crate::inspect_view::sync_camera)` | Update / SimSync | yes | same before InspectViewSet::Camera; Builder docks and lesson viewport/cues reach camera before synchronization. |
| `lesson/mod.rs:656` / `live_equations, track_blocks, apply_settings, frames::step` | `after(ui::rebuild)` | Update / SimSync | no | retained intra-feature function edge; Local producer data reaches its consumer before reading/drawing. |
| `lesson/mod.rs:661` / `selection::follow` | `before(playback)` | Update / SimSync | no | retained intra-feature function edge; Local producer data reaches its consumer before reading/drawing. |
| `phenomena/mod.rs:114` / `keys → buttons → slider` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `place_view.rs:138` / `keys` | `after(actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `robot/hardware/actions.rs:410` / `buttons → jog_buttons → keys → window_loss → sliders` | `after(actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |
| `robot/hardware/actions.rs:411` / `hardware::actions::apply` | `after(super::super::actions::apply)` | Update / Actions | no | same after RobotSet::Actions; Robot system_ui passes hardware action before hardware drains it. |
| `robot/hardware/mirror_panel.rs:35` / `mirror_sync` | `before(crate::robot::apply_frames)` | Update / SimSync | no | same before RobotSet::Frames; Mirror changes before poses; synchronization reads posed links after. |
| `robot/hardware/sync_panel.rs:58` / `sync_frames` | `after(crate::robot::apply_frames)` | Update / SimSync | no | same after RobotSet::Frames; Mirror changes before poses; synchronization reads posed links after. |
| `robot/mod.rs:202` / `gait_path_input … actions::buttons chain` | `after(crate::app::actions::serve)` | Update / Input | yes | InputSet::Rest → Window (membership); REST writes actions before window input; original chain retained. |


**Remaining edges (by reading):** 13 function targets, 11 qualified and two
bare, on 11 source lines (the original qualified-path grep matches nine
lines). All are intra-feature: builder placement::update,
placement::apply_preview, discussion::hover, markers::sync, ui::rebuild_panel
(five); CAD numeric::entry at pick and transform, calibrate::escape at
threads, data::sync at robot/tools, numeric::entry at surfaces and local
sketch-plane sync (six); lesson ui::rebuild and playback (two). Thus 99 of
the 112 original occurrences are replaced or redundant, and zero serve
function ordering edges remain. Public set ordering calls are excluded
from these function counts. (cad-checklist-traces, 2026-10-02: the four
CAD edges against `numeric::entry` and `calibrate::escape` in the table
above now order against `CadKeySet::NumericEntry` and
`CadKeySet::EscapeTool`/`Escape`; their rows are kept as history. The
headline counts above are as of that batch; the CAD function edges left
are `data::sync` at robot/tools and the local sketch-plane sync.)

**Small gaps:** PhysicsLabel stores a stable key and occurrence number.
The existing visible-label pass reconciles Node/Text in place, using
set_if_neq; only absent/new visible labels despawn/spawn. The existing
rounded text, 0.25-second live refresh, easing, collision slots, fixed-width
value allowance and viewport refusal rules remain. Selection's shared
actions::apply receives the same messages, in-flight state, replies,
selection and registry as five system parameters; outcomes/refusals are
unchanged. FieldMsg derives Message using the pinned macro.

**Review and verification:** Five reviewer areas cover app/Input,
robot/hardware, CAD, inspect/builder/lesson/kit/small gaps, and guard/docs.
All checks are by reading, not executed workflow parity or compile evidence.
Review findings fixed: the guard resolves glob reexports and lexical import
scopes, a stale next-epic paragraph now names cad-components, and label
children use explicit `for &child in children` (IntoIterator for &Children)
to avoid confusing its borrowed iterator with RelationshipTarget::iter.
All five worker reviewer areas reported no remaining findings after these fixes.
The subsequent orchestrator review found a transitive-resolution bypass:
chained module aliases and qualified local reexports could conceal a
foreign function. The focused T41.2/T41.3 repair follows every module prefix
through symbol tables, including aliases used as glob targets. An active
binding recursion stack rejects import/reexport cycles (including paths
that grow on each expansion) with a file:line ownership diagnostic; repeated
use of a completed alias expansion is allowed. Viewer ordering is unchanged.
The repair reviewer also caught a use-site overlay that let block aliases
change module-level imports. Binding selection is now lexical, while
import targets and qualified self/super paths resolve in module tables.
Declaration metadata distinguishes a local identity from a self-import cycle.
Regression fixtures cover both cross-feature bypasses, same-feature chains
and reexports, lexical shadowing of both functions and module aliases,
self/ordinary/growing cycles, aliased glob targets and valid alias
reuse. The independent repair reviewer reported no remaining findings after the
provenance fix. The repair is checked by reading only; fixtures remain
unexecuted.
The windowless app/ordering_tests graph calls the production configure
helpers and materializes the public sets with no-op producers/consumers,
including the spatial/camera, CAD mesh, robot frame and PreUpdate picker
chains. It checks schedule initialization when run; it has not run here.
The source guard ignores comments/literals/test items, resolves use trees,
aliases and glob reexports transitively, with explicit cycle diagnostics,
and scans tuple/multiline targets; its allowlist
is empty. Its own fixture includes the old bare update_parts glob-import
regression. Existing camera and app tests were split without changing
test bodies so every Rust source file is at most 750 physical lines.

Required verification, deliberately pending under this run's reading-only
rule (these are future checks, not receipts):

- [ ] `cargo build -p sim-spatial --lib --tests --bins` with no warnings.
- [ ] `cargo test -p sim-spatial --lib --bins` including the source guard
      and windowless schedule tests.
- [x] Read each removed edge against its singleton set or Input chain;
      no effective relation changes identified.
- [x] Read hardware STOP and jog-release predicates: unchanged and
      ungated by text focus; existing chain and REST/action order retained.
- [x] Read pinned Bevy 0.19.1 derives, configure_sets, in_set, chain,
      schedule initialization, Message, Children and set_if_neq signatures.
- [x] Source size and whitespace inspection; no build, test, screenshot
      or hardware operation performed.

Current order: cad-components (T42) and cad-experiments-motion (T43) are implemented by reading. T43 source review was accepted at 4d2725f2/e6f6ed10; T44 was accepted by source review at 8a7c0cd7/fe2a6eb1; T45 realizes persisted settings by reading. Execution and exact parity remain unverified.


## CAD components ownership and integration (T42, 2026-10-01)

Implemented with source-only verification; compilation, fixtures and exact parity are unexecuted. The seven stacked Rust
epics include this batch; historical Python receipts above remain historical.

Before implementation, the ownership contract is: Python/OCCT owns durable CAD
definitions, occurrences, bodies, graph bindings, derivation provenance and undo.
`ComponentJobService` owns preparation snapshots and the one guarded commit; Qt
and REST advance that owner, with Qt retaining only widgets and selection. Rust
`cad_client::components` carries explicit operation, document identity and revision
payloads; no geometry is calculated by these forms.

The persistent native `ComponentsState` owns drafts, catalogue snapshots and
pending start/poll/cancel jobs. Source mutations have one writer, the authoritative
service. `CadDocument.component_busy` blocks source edits and replacement until
completion is acknowledged. Cancellation is durable state, not an expiring
mode-gated message. UI fields/buttons write `Act<CadAction>` in Input; the existing
apply system consumes them in Actions; job results land after public
`CadSet::Results` in JobResults; presentation is Present. The CAD panel spawns
transient descendants of its mode-scoped roots, which linked despawn and
`DespawnOnExit` remove. Kit text entities retain their drafts through the existing
InputFocus/TextDraft service. Network, geometry and directory work belongs on jobs
or the Python worker; no simulation is added to frame systems.

The explicit version-1 CAD graph envelope and the hierarchical Build document
remain separate persisted schemas. Both adapt to a shared composition description
and presentation; CAD identity, imported bindings, multi-terminal connections and
geometry recipes are retained in the source envelope. Display position and zoom
are presentation state and never physical transforms. Unsupported metadata is
refused by path, rather than filled with invented physical parameters.

### T42 source trace and review boundary

Launch remains `cargo run -p sim-spatial -- path/to/model.rcad` or
`--cad-url http://127.0.0.1:8420` to attach to an existing RoboCAD service. These
are launch instructions, not commands executed in this batch. Python/OCCT and
the existing registry executable remain required. Component authoring and jobs
need no Qt panel. Creating a new experiment/check still requires the external
reference workflow; the native graph imports metadata from an existing completed
check ID and never runs experiments in this batch.

The window Components section, `system_ui` controls and REST `cad_components`
all reach `cad/components/mod.rs::handle`. `form::open/set/operation` retains
selection-backed drafts, definition metadata, variants, nested mappings and branch
overrides; `validate::validate_operation` applies member restrictions and typed bindings.
`jobs::Active` captures generation, document ID and revision before POST. The
service `component_service::start` checks the source stamp, `ComponentJob` prepares
snapshots in its worker, and `commit` checks the stamp again before ComponentChange.
Qt's panel starts/polls this same owner.

End-to-end reading trace: make from shared Selection → guarded make operation →
ready commit → refreshed definition/selection; Place twice submits two separate
occurrence requests; Edit defaults sends definitions/nested parameter mappings;
Apply occurrence sends the selected branch's overrides; Reset removes overrides;
Cancel repeats DELETE until terminal (an acknowledgment can still say Running).
Lost POST recovery uses nonpublishing discovery, never a second POST, so pending
cancel can precede commit. Export prepares a temporary archive and publishes it
only after the same revision guard. Save uses the existing `cad/files` guarded
path after tracked rebuild completion. Drafts remain available after refusal or
mode teardown. Active work blocks document/mode replacement; unexpected identity
displacement cancels through the captured client. An unresolved network outcome
keeps a named blocker rather than assuming the service did nothing.

CAD graph intents similarly reach `cad/composition::handle` and typed
`cad_client::composition` commands, then revision-guarded `Service.system_request`
and `component_graph.edit_graph` undo. `sim_system::composition` preserves the
CAD source envelope and adapts catalogue/import metadata into shared inspection
topology. Build's Resolver uses shared port validation; Build's schematic and CAD
use `sim_diagram::composition::present` and `graph_presentation::route`. Focus,
zoom and layout are disposable display state. Body links, imported bindings, IDs,
multi-terminal connections and derivation/provenance remain source-owned.

Decisions: keep both persisted formats through explicit adapters; refuse unknown
metadata instead of inventing physical values; keep dimensional Current expressions
labelled unevaluated; use kit path fields instead of Qt file dialogs; import only
completed check metadata. Structural metadata freshness uses the existing
`cad_derivation_hash` (excluding graph edits) while measured result freshness
continues using full physical_hash; legacy records without that stamp use the
stricter physical hash. Revisit these choices when independent execution or parity
evidence warrants a change. No schemas, measured values or simulation paths change.

Independent reading reviews covered service/client commit races, native actions/
forms/jobs and shared graph consumers. Written regression fixtures cover headless
undo, start/commit revision refusal, queued-ready cancellation, nonpublishing
recovery, staged export, retained drafts, metadata freshness, CAD envelope round
trips and connection/geometry validation. No fixture was executed. The ledger
contains 775 rows: 540 done-by-reading, 172 deliberately different and 63 deferred
to cad-experiments-motion; all 38 cad-components rows have current owners.

Graph review repairs retain source/catalogue snapshots even when adaptation fails,
so users can repair/remove invalid components or replace the complete graph with
a validated retained JSON draft. Retained graph drafts expose Resume and explicit
Copy to current revision; recipe changes preserve the prior draft and clear only
obsolete recipe fields. Display pan/arrange offsets are in-memory and are not
written into either persisted schema. Shared Build validation runs before
subsystem scoping, so external signal drivers do not invalidate focused drawings.
Imported-check changes invalidate prior reads and completions check the run ID.
`/system` responses now carry document identity; native graph mutations send it
alongside the revision guard, including DELETE queries. Legacy clients may omit
the new wire guard; persisted .rcad and sim.system schemas remain unchanged.
Body links and derivation provenance survive in the CAD envelope. The disposable
inspection adapter leaves CadReference absent without an actual CAD artifact
digest, rather than labelling a graph hash as a geometry artifact hash. Custom
connector definitions absent from the authoritative built-in metadata are refused
by path; they are not silently treated as compatible.

Queued component and graph text actions carry the draft index that spawned their
field. Selecting another form does not redirect the old field's unsaved text.
Component Apply refuses if its displayed draft is no longer selected; graph Copy
explicitly captures a new document stamp. This chooses preserved drafts and named
refusal over silently applying inputs to a different form/document.

### T42 repair review: family identity, pending ports and failed layout

The four findings from call-0332 are repaired by source review; compilation,
fixtures and exact parity remain unexecuted. Existing T42.1–T42.4 and all eight
batch evidence IDs remain in force. No persisted schema or geometry owner changes.

`cad_components {op:"open",kind:"link_family",id:"occurrence-id"}` now treats
`id` only as an occurrence (or a materialized member resolved to its occurrence).
Select the family definition with `op:"select",id:"family-definition-id"` first.
Omitting the form ID uses shared source Selection for the occurrence and library
selection for the family. This is the same form path for window, system_ui and
REST; one ID no longer ambiguously names both sources. Separate new fields were
rejected because existing independent selections already express the two values.

Pending graph picks are durable presentation intent owned by composition state,
not source geometry or an expiring message. Each first pick captures endpoint,
generation, document ID and source revision after validating the displayed
snapshot. Connecting a second pick or Leave port open validates that original
stamp; an intervening edit or same-revision document replacement refuses with a
named diagnostic, preserving pending intent until explicit cancellation. Snapshot
reads do not silently rebase the first pick. Window controls, system_ui and REST
share `Port`, `LeaveOpen` and `CancelPort` typed intents in the existing action
pipeline. Leave open creates an explicit singleton connection through guarded
source commands and undo, refusing already-connected ports. Remove connection
continues to delete a complete connection; Cancel connection changes only pending
intent. No complete-graph JSON workaround is required for these operations.

Build layout validation now returns path-named errors through the jobs Result.
Cancellation alone returns no layout. Failed source keys and diagnostics remain
recorded, so an unchanged invalid description does not launch a new job each
frame. A changed source key may retry. Any prior usable layout remains visibly
stale with the diagnostic and disabled source interaction. Validation/layout stays
on the existing jobs module; source truth stays in the shared runtime/model.

Written fixtures cover explicit family/occurrence identities, stamped first picks,
intervening revisions, document replacement, singleton physical and signal-output
nets, connected-port refusal, cancellation without mutation, and layout failure
retention/retry suppression. These choices prefer named refusal and preserved
intent over silent rebasing/retry. Revisit only if independent execution or source
review demonstrates a missing lifecycle case. Transient panel descendants, public
system-set ordering and the existing kit text service remain unchanged.

Repair source owners: `components/form.rs::open` plus `components/tests.rs`
prove explicit family occurrence semantics; `composition/ports.rs::{pick,
validate_pending,leave_open,cancel}` owns stamped intent and the singleton command,
with `composition/ui.rs::{controls,draw}` exposing and rendering Leave open/Cancel.
The renderer uses the existing shared kit button helper before both the dock-open
and snapshot branches: pending intent remains cancellable with a missing, stale or
replaced snapshot, even when the dock is closed. Leave open keeps stamped readiness;
Cancel remains enabled.
`composition/tests.rs::drawn_pending_buttons_keep_shared_actions_without_a_usable_snapshot` inspects actual
spawned Button/CadButton/Enabled entities for these cases, rather than merely the
control catalogue. Rendering does not change pending or submitted source intent;
publication and acknowledgment ownership remain unchanged. This focused renderer
repair and its windowless fixture were reviewed by reading only, not executed.
`builder/schematic.rs::{lay_out,finish,needs_layout,tick}` owns path-error delivery
and failed-key suppression. Independent reading reviewers found no remaining
correctness findings in these repairs. All new fixtures remain unexecuted.

Final lifecycle reading found one additional race at accepted dispatch: a later
source revision refusal must retain connection intent, as Qt publication does.
`SubmittedPort` now records the submitted generation/edit sequence and pending
stamp. `sync::finish_edit` resolves it from the authoritative answer before the
independent REST result is stored. Success clears matching intent; failure retains
it with a diagnostic; repeated port submissions wait for acknowledgment. Cancel
may discard local intent but keeps the marker and explicitly states that the sent
source edit continues. This reuses the existing CAD job/result owner, with an
optional composition resource parameter checked against pinned Bevy 0.19.1, and
adds no competing job lifecycle or private ordering edge. Fixtures cover refusal,
success, mismatched answers and cancellation without resurrecting intent.

## CAD experiments and motion (T43, 2026-10-01)

Implemented and independently cross-reviewed by reading; uncompiled and unexecuted. The 63-row family now has individual owners in cad-parity.md; [T43 evidence](../cad-experiments-motion-evidence.md) records all eight batch IDs, actual rendered controls, source/service traces, ownership, public scheduling and lifecycle contracts. This realizes §§1–7 and §9 phase 1 only. Headless captured geometry and reference PoseModel sampling retain Python/OCCT authority; Qt reuses the shared service. Native experiment preflight feeds composition metadata. Resources retain drafts, rejected requests and durable cancellation/unknown receipts while transient kit widgets may close. Preview blocks source geometry edits centrally, with guarded annotation/program metadata as the only auxiliary exception. Native export uses bounded capture delivery and a serialized cancellation/publication gate. No physics, derivation port, executed parity or legacy retirement is claimed. T43 was accepted by the orchestrator in call-0342 after source review of 4d2725f2 and e6f6ed10; compilation and execution remain unverified.

### T43 repair ownership and continuation contract

Closed experiment docks retain drafts and cancellation receipts but suspend automatic
rebasing/reruns. Queued automatic Run actions carry a closure epoch distinct from
explicit UI/REST Run. Close invalidates that epoch; deliberate reopen resumes a
fresh debounce. Linked-source HTTP requests use Dedicated, alongside all other
network jobs; Io remains for local file work and Compute for chart rasterization.

Pose continuation is caller-owned state (`cad_client::motion::PoseContinuation`),
not a mutable remote session. Python validates exact document/revision/source
identity, full finite resolved positions, bounds, transmissions and loop closure
before seeding a request-local PoseModel. The existing matrices implementation
then follows intermediate steps from those resolved positions. Preview accepts
only current ordered/stamped samples. Seek and program changes continue from
the last published pose; Enter/replaced identity resets to the source home branch.
Export has its own cloned continuation and advances it sequentially, leaving the
saved preview continuation untouched for restoration. Cancelled/stale responses
advance neither consumer. Solver and validation remain on the reference HTTP
worker through native Dedicated jobs. Qt retains its model and uses the same
shared sample_model primitive.

Decision: prefer explicitly validated prior state over server sessions to avoid
remote lifetime/cleanup and cross-consumer branch state. Revisit only if executed
performance evidence warrants a bounded identity-scoped session service. No
solver or physical definitions change. Written race and closed-loop fixtures
remain unexecuted; no compilation, launch, capture/export or parity execution.


### CAD parity harness — T44 (2026-10-01)

§9 phase 2 now has a graphics-independent shared contract and paired headless
runner in `sim_runtime::cad_parity`, with direct RoboCAD reference dispatch and
a separate typed Rust CAD-client adapter. Both still use Python/OCCT; service
agreement cannot qualify an independent derivation or kernel replacement.
The [harness guide](../cad-parity-harness.md) records the module map, versioned
wire contract, isolated hash-backed corpus, future launch path, source trace and
fail-closed migration gates. This changes no CAD authority, UI, physics or
measured values. Python/browser compatibility and unsigned checklists remain.

T44 contracts, adapters, fixtures and reports are written and reviewed by
reading only. No scenario, build, test, export, window or capture was executed.
There is no executed passing migration evidence and no legacy retirement.
Historical T43 accepted source review is distinct from compilation or parity.

T44 shutdown repairs preserve partial reference receipts and record run-level
execution issues separately; those issues block gates. Process ownership retains
the unreaped leader through final descendant signalling before releasing identity.
The harness guide records the bounded recovery path and exclusive-wait/process-group
limitations. Repair fixtures remain uncompiled and unexecuted.


## Native offline identification authoring — T46 (2026-10-02)

Accepted by source review in call-0360 across 1b0ca533, d238d6b6 and c380210b.
This acceptance resolves the three repair findings; compilation, fixtures,
interactive usability and executed export/parity remain unverified. Historical
stacked-uncompiled counts elsewhere are dated snapshots, not execution evidence.

The existing Build → Actuators → Measured evidence dock presents retained studies
through `builder/calibration/study`. `StudyOwner` is global durable window state;
transient widgets use the existing dock and UI kit. Typed StudyAction occurrences
are applied by one owner in Actions. Shared `experiment_study::commands` owns
transactional configuration/view/exposure/decision semantics, also consumed by
legacy experiments_ui. Registry descriptions drive motor/bridge parameter fields.
The existing runtime `evaluate`/`simulate`/`actuator_bench` executes captured inputs
inside adopted jobs. Results poll globally in JobResults and cannot attach to a
different retained study at the same revision. Text intent uses the kit service,
with identity/revision guards against late submissions. A global status/cancellation
surface remains reachable when the evidence dock closes.

T46 repair contract: one runtime complete-pair outcome governs summary, filters,
native/legacy labels and HTML. Missing baseline/candidate predictions or either
error means UNSCORED; available metrics and traces remain inspectable. Evaluation
capture records terminal native job identity and cancellation requested separately
from the runtime's actual execution-cancelled flag. This additive saved metadata
survives reopening without rewriting scores or making save acknowledgments dirty.

Decision: polling a handle is not a semantic resource publication. Jobs poll
through pinned Bevy change-detection bypass and mark the owner changed only for
terminal publication or displacement. Form/chart presentation likewise invalidates
on real input or publication, rather than idle mutable polling. Progress remains
observable independently through the global status projection. Rejected alternative:
using every ResMut dereference as a reason to reconstruct the whole Build panel.
Revisit if a shared presentation revision contract replaces these explicit semantic
publication boundaries. All repair fixtures and execution remain unverified.

Immutable publication captures a Study revision and uses runtime save_new or
export_html_new in jobs. A successful save marks only the captured revision saved;
later drafts remain dirty. Failed/cancelled work retains named receipts. Normal
window closure refuses dirty/pending study evidence; abrupt process termination
cannot guarantee in-memory draft durability. Mode/document replacement guards
preserve authored work. Existing physical document identity and shared Selection
remain the spatial owners; persisted trial/evaluation fields are review view state.

Decision: guard normal closure instead of silently writing an unspecified recovery
file. This keeps evidence destinations explicit and immutable. Rejected alternatives
are discard-on-close and automatic overwrite. Revisit if a separately reviewed
recovery-store contract supplies user-visible durable destinations. Saved unknown
supported payloads remain round-trippable without migrating refinement controls.
Controller refinement now follows T48 below; power and FPGA refinement, raw sweep
UI and accepted registry promotion still use existing external paths. Compilation, all new fixtures, export
execution and exact parity remain unverified; legacy sim-viewer stays available.

## Persisted viewer settings — T45 (2026-10-02)

Global durable preference projections have one writer, `app::settings::SettingsOwner`.
`PreferenceGroup` uses pinned `SettingsGroup`, `ReflectSettingsGroup` and registered
reflection. Its named file/group contract identifies the custom JSON envelope;
registered resources hold validated JSON projections. The stock plugin's immediate
file load and task-pool-scoped save work do not satisfy §4: only `jobs` may
perform disk work. The pinned scope waits for completion, while save ticks advance
after logged store errors; neither behavior provides the required publication Result.
The [compatibility guide](../viewer-preferences.md) records authoritative pinned
contracts, field/subgroup mutation ownership, readiness, paths, migration and recovery.

Public ordering remains `ViewerSet` Input → Actions → JobResults → SimSync → Present.
Settings actions own retry occurrences; pending loads, recents records and dirty
revisions are durable resource state rather than expiring messages. Settings results
publish in JobResults through the public settings set; picker readiness follows that
publication. Consumers never use incidental registration order or change ticks to
claim durability. Reflection publication follows validation. No settings UI entities
or second selection/action path are added. Frame systems validate small projections;
file reads, canonicalization, file-byte serialization and atomic publication run
through jobs. Small projection/envelope encoding is in-memory frame work.

CAD print forms, closed fastener picks and state reporting read the same global
CadDefaults through `Cx`, `Env` and snapshot `Parts`. A validated wall-check launch
or accepted revision-guarded edit dispatch mutates the owner; refused commands do
not. Picks, cached reads, results, source stamps and jobs remain on CadDocument.
Geometry and undo continue through RoboCAD's guarded command path (§9). Preferences
are tool inputs, never physical truth. Hardware choices remain inactive intent (§8):
connection/config validation, STOP, release/loss handling and explicit activation
retain their existing owners. Late publication uses the existing host-only Inputs
assignment through `inputs_changed` to synchronize an already connected session
with the displayed drive-mode/hold-others choices. That assignment makes no physical
request and restores no activation; subsequent explicit Select/jog consumes it.

Active mirror maps are rebuilt from current motor keys, preserving nested metadata
only for retained identities. Removed mirror/sync rows stay outside active projections.
Initial `preserved_source` remains immutable; subsequent removals append deduplicated
raw-row records to flat version-1 `retained_rows`, never enclosing archive snapshots.
A 256-record/1-MiB cap fails closed without eviction or publication; dirty diagnostics
remain retryable. Later-startup unknown metadata is recoverable without reactivating
removed rows. The preferences guide specifies identities and recovery. T45 repair
fixtures are written and source-reviewed only; compilation and execution remain
unverified. The SettingsGroup seam, jobs owner and public schedule ordering remain.

Abrupt/uninterceptable shutdown submits best-effort jobs for dirty latest snapshots, serialized with any
in-flight save by the publication gate; the UI never blocks on disk. Dropping a job
handle alone does not guarantee durability; an ordinary immediate process exit
can also end before final work finishes. Abrupt process exit can lose unsaved
preferences, including termination during loading. CAD unsaved-edit and hardware
safety guards are unchanged. Publication errors remain diagnostic and dirty; retry
requires no unrelated edit. Fixtures are isolated and written only, never executed.
Python/OCCT and external reference dependencies remain; no parity or legacy
retirement is established. T44 accepted source review covers 8a7c0cd7/fe2a6eb1 only.

## One ordinary native close lifecycle — T47 (2026-10-02)

[T47 source map and checklist](../graceful-preference-exit.md) records T47.1–T47.3
and all eight graceful-preference-exit IDs. `app::close::CloseOwner` is the sole
ordinary close decision owner, retaining pending intent and revision-scoped
preference-loss acknowledgment. Window, actual global kit controls, system_ui
and REST feed its typed CloseAction. The former guarded_close orchestration is
removed. StudyOwner and StudyUi remain authoritative preservation-fact owners;
CAD release_child remains authoritative for preserving potentially unsaved
self-started services. No authored-work discard or automatic save is added.

Public CloseSet::Apply in Actions follows hardware action handling; STOP is
requested immediately, independently of preference jobs. Pending closure refuses
new hardware motion; loss, AppExit and bounded Drop fallbacks remain independent.
SettingsSet::Publish lands existing jobs in JobResults. CloseSet::Publish in
Present exposes semantic status. The final Last authorization before ExitSystems
rechecks current settings and preservation, arms ClosingWindow, and rechecks the
same stamp on the next frame before despawning. Expiring messages never carry
pending close work. Cancel-close retains jobs and drafts. UI polling avoids idle
semantic changes or panel reconstruction.

Ordinary success requires acknowledged latest preference publication and an
empty required recent queue. Loading, unavailable destinations, protected sources,
normalization/snapshot/publication failures remain named states. Retry and an
explicit preference-only exit are visible globally; a later edit or recent record
invalidates the bypass acknowledgment. Drop is only best effort. Arbitrary
AppExit and Cocoa termination cannot be delayed by an observer; abrupt exits
cannot guarantee draft recovery or CAD release. The source map states those
limits honestly. Written lifecycle/window/control/settings/CAD/safety fixtures
and implementation were source-reviewed; no compilation or execution is claimed.

## Native offline controller refinement — T48 (2026-10-02)

The [T48 evidence](../native-refinement-authoring.md) and
[field inventory](../native-refinement-inventory.md) extend §§1–7 through the
existing StudyPlugin, StudyOwner, StudyUi and global jobs, with no new mode or
persistence owner. Shared `experiment_study::refinement` owns transactional
commands, immutable preparation/dispatch and result application, consumed by
native typed actions and migrated legacy operations. Individual kit fields and
structured rows author experiments, coordinates and scenarios; archive selectors
keep frozen splits. Captured review and chart jobs preserve sample times and model,
runtime, timing and task-limit identity. Explicit candidate use changes only an
exploratory draft and records its source fit/exposure; review decisions never
accept physical properties. Failed/incomplete/cancelled analyses remain unscored.

Decision: additive bounded shared execution evidence and native receipt/publication
captures survive saved Study reopening. Existing opaque payloads remain intact;
receipts do not recursively nest previous Studies. Revision-scoped save
acknowledgments and existing T47 preservation facts remain the only lifecycle
contract. This is source-reading verification with unexecuted fixtures, no
hardware/CAD/registry promotion, legacy retirement or parity execution. Deferred
power, FPGA, recording/combined fitting and hardware paths stay available.

T48 publication includes retained malformed/unsubmitted form text as raw unapplied
evidence from StudyUi, through the existing action owner and publication snapshot.
It does not apply or clear that text, alter acknowledgment revisions or remove
close blockers. Reopened artifacts show the captured raw intent for inspection;
physical and exploratory settings are still only shared validated commands.

## One ordinary native activation and focus contract — T49 (2026-10-02)

[Bounded inventory, ownership, pinned signatures and source evidence](../native-keyboard-activation.md)
cover T49.1–T49.3 and all five native-keyboard-activation outcomes. This is the
§§1–7 shell/kit/action consolidation; no physics, CAD numerical replacement or
hardware keyboard hold is introduced. Ordinary per-feature press adapters are
removed. Pointer primary cause authorization precedes deferred Activate; secondary
presses are refused. Enter/Space repeat is suppressed by pinned Button; consumed
presses never also trigger mode shortcuts, and releases/STOP remain independent.

Public ActivationSet orders eligibility before picking/focused dispatch, modal
containment before TextInputSet, capture before Validate before InputSet::Window,
and occurrence cleanup before Actions. Rendered source/document/form stamps
refuse stale controls; CAD Captured and Build RenderedUi revalidate before the
same existing authoritative action owners. Expiring occurrences never own edits.

Transient ModalFocus roots contain pinned tab navigation and return the one focus.
Stable typed input identities and captured source anchors rebind durable fields
without replacing their drafts/caret or initializing the first row on every
rebuild. Missing/ambiguous/hidden/replaced anchors deliberately release focus.
Modal owners respect navigation focus instead of stealing it back to a field.
Held jog, sliders, compound pointer tree gestures, double-click, radials and
viewport surfaces are named exceptions; tree keyboard selection still uses the
ordinary contract. Implementation and written fixtures are source-reviewed only;
compilation, execution and GUI parity remain unverified. External Python/OCCT and
browser reference workflows stay available; no migration retirement is claimed.


### T49 correction: retained lifetimes and modal priority

Call-0372's five findings in `11e898f7` require this correction within T49.1–T49.3,
retaining every native-keyboard-activation outcome ID. Focus-only picker changes
are presentation state and cannot invalidate a source choice stamp. CAD retained
presentation keys include their source lifetime: a new source revision rebuilds
controls rather than refreshing immutable occurrence stamps. Old captured actions
still fail the existing source validation before authoritative application.

The one editor may transfer navigation to a unique eligible current control of the
same durable document and stable intent when its old anchor disappears, including
while suspended under another modal. This preserves draft/caret only; action stamps,
physical values and draft acknowledgment never transfer. CAD identities retain
source generation and form/numeric/rename lifetimes while excluding source revision.

ModalFocus priority is explicit through ModalPriority (default zero; pending close
100), then ancestry depth, then generational entity bits as a deterministic sibling
tie-break. The highest eligible scope owns containment; cancellation restores valid
previous focus or initializes the remaining scope. Logical lower scopes retain
their original return target even when they rebuild beneath a higher close scope. Pending close uses this same
InputFocus, pinned TabNavigation and typed CloseOwner lifecycle above existing forms.
Fixtures feed KeyboardInput through pinned public dispatch rather than constructing
private FocusedInput fields, and inspect results only after deferred delivery.
All repair evidence remains source-only, with fixtures and compilation unexecuted.


### T49 correction: higher-modal editor suspension

Restoring InputFocus alone is insufficient when a real form owner has discarded
its active-property mapping on Blur. The shared kit now marks a valid durable
TextField as suspended before a higher ModalFocus takes the keyboard. Suspension
is transient per-editor navigation state, owned by modal containment; it is not
another focus resource, an activation occurrence or an applied document edit.
The text input writer suppresses only that temporary Blur. Actual field owners
query TextFocus::suspended, drain occurrences, retain their property mapping and
refrain from refocusing, resetting the draft or applying text during suspension.

Cancellation restores only eligible current document/form/field identities.
The kit clears suspension before the existing consumer resumes editing. A missing,
hidden, ambiguous or replaced identity emits real Blur to the owner even while
the higher modal has the keyboard, invalidates the return anchor and leaves the
draft unapplied. Explicit owner blur also invalidates the anchor. Ordinary Tab or
pointer departure remains real Blur; suspension never makes an editor sticky.

Catalogue and file fixtures use their actual renderers and input consumers with
WindowCloseRequested, keyboard cancellation, continued editing and submission.
Other form owners and reconciliation loops follow the same contract. Editor state
means the existing TextDraft text/selection and its existing implicit end insertion
position; this repair introduces no alternate text editor or caret model. All
T49 task/outcome IDs remain in scope. Fixtures, compilation and GUI behavior are
unexecuted; STOP, unconditional releases and close preservation remain unchanged.


### T49 correction: suspension before pointer focus

Call-0376 identifies an ordering race in `090d608d`: if picking assigns focus to
a close button before modal containment, the departing editor is no longer visible
to suspension capture. PreUpdate now explicitly orders the public kit set
ActivationSet::Eligibility → ActivationSet::Containment → PickingSystems::Hover.
Containment holds modal_focus; it remains before TextInputSet and focused Dispatch.
Pinned picking chains ProcessInput → Backend → Hover → PostHover → Last, with
pointer_events in Hover. TextInputSet follows Last and UI Focus, then Dispatch and
shortcut consumption follow text input. No edge requires containment after picking.

Eligibility uses Commands to publish TabIndex/InteractionDisabled. Its normal
`.after` edge to Containment uses pinned Bevy's default automatic ApplyDeferred
insertion; no ignore_deferred edge or disabled synchronization is introduced.
Suspension and InputFocus updates inside containment are direct resource/component
writes. They are visible before Hover emits any press that can replace focus.
Pointer-down activation, real Tab-away Blur, source/form refusal and unconditional
release/STOP behavior retain their existing owners and contracts.

The actual catalogue/file pointer-first fixtures queue the first close interaction
for the scheduled picking stage after one OS-request frame renders pending controls.
They inspect suspension before press emission, then use the existing observers,
close cancellation owner, form input and continued editing/submission. All prior
T49.1–T49.3 and outcome traces remain; fixtures and compilation are unexecuted.

## Native offline controller recording authoring — T50 (2026-10-02)

[T50 bounded inventory](../native-refinement-inventory.md) and
[source evidence](../native-recording-fit-authoring.md) extend the existing
StudyPlugin/StudyOwner/StudyUi, typed actions and adopted jobs in §§1–7. Recording
classification/import, immutable setup revision authoring, frozen whole-run
assignments, distinct command-replay/own-feedback prediction, recording fitting and
optional saved-study combined fitting reuse shared runtime contracts and optimizer.
The native path retains incomplete/deferred/rejected input and terminal evidence.
No mode, persistence owner, physics path, feature thread or optimizer is added.

Decision: retain content fingerprints as controller recording identity; shared
code/cadence source hashes do not identify unique acquisitions. Exact duplicate
imports preserve assignments/exposure; conflicting declarations for the same
recording hash and dataset trial collisions refuse retargeting. Roles/limits/rationale freeze by
recording identity and exposure/influence remain monotonic. Setup revisions append;
CAD remains physical source of truth. Predictions require explicit comparison
limits. Candidate use remains explicit exploratory source-linked draft authoring,
separate from review decisions and physical-source acceptance.

T50 was accepted in call-0386 by source review across `5dd26014`, `11b9ce82`
and `3a7d321a`, with written unexecuted fixtures; no
compilation, execution, publication or GUI parity receipt is claimed. §§8–9 remain
unchanged: Python/OCCT, browser, FPGA/electrical/power, raw sweep and hardware
acquisition reference requirements remain. No legacy retirement is authorized.


## Shared evidence publication — T51 (2026-10-02)

The [focused contract and source map](../shared-evidence-publication.md) realizes
§§2–4 and §7 beneath the T45/T47/T50 owners. Shared filesystem stages do not own
serialization, revision gates, cancellation permission, document state or close
intent. Study retains immutable companion-first publication; SettingsOwner retains
ordered replacement and exact revision acknowledgment; CloseOwner rechecks current
settings and authored-work blockers before ordinary closure. A visible destination
whose required synchronization fails is a recoverable failure, never a saved receipt.

Decision: share publication mechanics, retaining consumer policy and ownership.
This removes duplicate temporary/sync/link/rename implementations without creating
a persistence service or simulation path. Specialized undo journals, annotations,
recording streams and isolated CAD parity outputs retain their existing writers;
their transaction, streaming or isolation contracts are outside this bounded migration.
Revisit those writers only under a separately reviewed migration. Compilation,
fixtures, filesystem behavior and GUI parity remain unverified; no execution is
part of T51 source review.


## Native offline electrical and power authoring — T52 (2026-10-02)

T51 was accepted by source review in call-0390 at `49f47d9b`; that acceptance
covers its shared publication batch, with no compilation, fixture execution or
platform durability receipt. [T52 navigation, ownership and source evidence](../native-power-authoring.md)
and [bounded field inventory](../native-power-legacy-inventory.md) extend the existing
Build → Actuators Study owner in §§1–7. Structured electrical source and controller
feedback authoring, both recording prediction purposes, captured servo voltage and
calibrated sidecar comparison use shared validated commands and retained jobs.
Exact comparison inputs use existing immutable companions; publication/reopen keep
T51's existing revision and durability gates. Numeric trace previews and calibration
review remain separate from motion tracking, with truthful unscored limitations.

Explicit source installation clears the incompatible fixed-voltage override as one
transaction. Unknown authoring input is refused and retained; failed/cancelled raw
sidecars retain exact bytes. Bounded comparison receipt identities never nest prior
Studies. This is source-only implementation evidence, not executed parity. §§8–9,
CAD/registry authority and independent hardware safety remain unchanged. Python/OCCT,
FPGA refinement, raw sweeps and hardware acquisition/driving remain external; reference
implementations remain available until separately authorized parity proves migration.

T52 was accepted in call-0396 by source review across `d9bc1455` and `929f6833`,
including the cancellation repair requested in call-0394. In that repair, terminal ResultData uses the existing Study
content companion owner and bounded references; serialization and reopen decoding
remain in existing jobs. Partial, failed and late-cancelled electrical terminals
are inspectable after reopen as unapplied UNSCORED diagnostics, without restoring
comparison attachment capabilities. Reports and native review share completeness
gating even when sampled summaries pass declared limits. This decision prevents
cancellation from erasing evidence or presenting incomplete sampled results as
accepted. Source-review fixtures are written but unexecuted; all seven T52 checklist
IDs and the §§8–9 boundaries remain unchanged. See the linked T52 source map.


## Portable retained-study artifacts — T53 (2026-10-02)

**Set aside and unaccepted at `f541b05f`.** The accepted 2026-10-02
verification repairs do not accept this batch. Its sources and retained evidence
stay available; calibration work does not expand its scope or qualification.

[The focused format, inventory and checklist](../portable-study-artifacts.md)
realizes §§2–4 and §7 through existing Study, Store, native StudyOwner/StudyUi,
typed actions, adopted jobs and immutable publication. SIMSTUDY v1 adds one
self-contained file beside existing JSON/companion and HTML workflows. Shared
loading detects it independent of extension, recovers exact objects into Store,
restores diagnostic caches and validates Study. Portable publication never extracts
paths, fetches inputs, nests receipt snapshots or rewrites sources. Resource limits
and duplicate refusal precede hydration. The same captured revision/destination,
publication gate, cancellation, displaced receipt and close-preservation owners apply.
Reports remain inspection outputs and never acknowledge editable-study saving.

T53 call-0400 repairs retain legacy publication captures in the existing panel
pending/result owner, including exact content, destination, representation and
revision. Failed, cancelled or stale captures can become separate retained reviews;
they never replace later edits or overwrite visible-unconfirmed destinations.
Recovery remains in-memory, not recursively embedded receipt history. Legacy atomic
cancellation cannot revoke a write already started; native PublicationGate remains
unchanged. Ordinary JSON preserves historical loading acceptance and companion
behavior; portable byte/count/depth limits apply only to SIMSTUDY. JSON publication
round-trips its representation and exact known content before writing so a newly
acknowledged file can reopen through the same shared decoder. Historical legacy
inputs retain their memory-use limitations. Repair fixtures remain unexecuted.

Decision: package the complete existing manifest and Store membership under the
shared owner, preserving identities and opaque compatibility fields. JSON keeps its
sibling companion contract; no automatic migration or reference retirement occurs.
§§8–9, CAD physical authority and hardware safety remain unchanged. Source reading
and written unexecuted fixtures establish implementation evidence only; compilation,
GUI parity, execution and platform durability remain unverified.

## Teleoperation (rover-drive-layers, 2026-10-02)

Goal 4's substrate: a keyboard, gamepad, `system_ui` or REST request in Robot
mode becomes one body twist, the twist reaches the robot's own external
controller on the `control.external` seam, the controller mixes it into
wheel commands, and the twist is recorded with the run for replay. Proven by
reading on the CAD-built two-wheel robot
`examples/wheeled-robot/baseline/robot.simrobot.json`. The step list and its
path:line traces are in [docs/rover-checklist.md](../rover-checklist.md).
**Everything here is by reading, unexecuted**: nothing was compiled, tested
or run; the only executed step was the golden-vector generator (below).

### Shape: three layers that don't know about each other

| Layer | Owner | Format | Code |
|---|---|---|---|
| Device bindings: keys, sticks, buttons → normalized axes (forward, lateral, yaw in -1..1) and named actions | The viewer, per device, shared across robots: the persisted preferences' `drive_bindings` group (the `app/settings` owner) | `sim.drive-bindings/1`; defaults in code (`BindingsFile::default`), stored only once a user sets them; REST `drive_bindings` reads, sets, resets | Format, defaults, validation and the W3C browser mapping: `sim_runtime::drive_bindings` (no Bevy; shared with the browser). Native polling: `sim-spatial/src/drive_input/` (`DriveInputPlugin`; `bindings.rs` maps names to Bevy, `input.rs` is the one poller for every mode) |
| Drive profile: supported axes, max speed / accel / stop decel with units and provenance, named actions (`stop`, `halt`), deadman (timeout, ramp or immediate) | The robot, beside its model: `<stem>.drive.json`, named by `<stem>.controller.json` | `sim.drive/1`, `deny_unknown_fields`; errors name the file and field | `sim_domain_control::drive::profile`; registry description `control.drive_limiter` (a real sampled element, so CAD inspectors, exports, Rhai and the systems editor share it) |
| Kinematic adapter: twist → wheel joint rates → integrated position targets | The robot's controller: `clients/python/examples/diff_drive_rover.py` on the seam | `--drive-json <sim.drive.resolved/1>` appended by the host | Rust reference `sim_domain_control::drive::kinematics` (`DifferentialDrive`, `Mecanum`, `HeartbeatDeadman`); stdlib Python port `clients/python/simloop/drive.py`, checked against one golden file. Browser: the binding's `embedded` Rhai adapter `examples/wheeled-robot/drive-adapter.rhai`, mixing only through the Rust functions `sim-script` registers (`sim-script/src/drive.rs`) |

The body twist is `[forward m/s, lateral m/s, yaw rad/s]`, the order
`SteeredGait::command` takes (`drive::steered::command_steered` shows the
same `BodyTwist` steering a gait), lateral positive left, yaw positive
counter-clockwise.

### The path (one execution path)

1. **Open.** `--robot FILE` finds `<stem>.controller.json` beside the model
   (`controller_binding::binding_path_for`), on the reload job, never the UI
   thread. `controller_binding::load` resolves the script (sha256), the
   simloop library (sha256 over its `.py` files), the profile (sha256),
   derives the geometry from the model (`sim_domain_robot::drive_geometry`:
   track width from the wheel joint anchors, wheel radius from the wheel
   collision geometry about the joint axis, joint signs from the axes, each
   `Provenance::Derived { from }`), checks the profile's speeds against the
   motors' free-running wheel speed and the deadman against the control
   period, and builds the controller program: `ControllerProgram.external`
   (Python) with the four command inputs. A binding that fails to load fails
   the run naming it; there is no silent fallback to the hold controller.
2. **Run.** The run thread (the jobs-owned `RunThread`) builds
   `sim_runtime::session::Session` from `controller_binding::scene`: the
   shared `PhysicalRobot` with the Python program started by
   `sim_couple::python` and attached through `Runtime::attach` on the
   model's `control.external` seam, wrapped in the session's
   `EpisodeCoupler`, which appends `command.forward`, `command.lateral`,
   `command.yaw` and `command.heartbeat` to the controller's sensors. No
   thread is spawned in a feature; no physics or mixing runs in the viewer.
3. **Drive.** Every input writes one action, `RobotAction::Drive { request:
   DriveRequest::{Axes, Action, Stop} }`, applied by robot mode's one apply
   system: axes are scaled by the profile (`kinematics::scale`, so a twist
   outside the profile cannot be made), sent as `Command::Twist`; the run
   thread applies the shared limiter and deadman (`kinematics::step`) on
   simulation time once per seam period and steps the session with
   `[forward, lateral, yaw, heartbeat]`. The heartbeat rises by one for every
   fresh request.
4. **Adapter.** The controller passes the live twist through (the run
   thread's limited twist is authoritative), applies its own deadman on
   heartbeat staleness (the stop rule from its last output), mixes with the
   resolved geometry and integrates `left axle.target`, `right axle.target`
   (`target += period × joint_rate`, as `velocity-controller.rhai` does),
   which the CAD motor firmware tracks.
5. **Record.** Save writes `Session::recording()` (the scene with the
   controller identity, the seed, one `[f, l, y, heartbeat]` action per
   period) under `runs/robot-drive/<stem>/`. Replay compares the recorded
   identity (script, script sha256, simloop library sha256, args, profile
   and its sha256, resolved drive, robot, period) with the loaded binding's
   and refuses by name on any difference; otherwise it rebuilds the session
   with the recorded seed and steps the recorded actions.

### Decisions

- *Three layers, three owners.* Bindings are the viewer's (a person's
  devices, every robot); the profile is the robot's, beside its CAD export
  (limits belong with the model, and the browser and hardware hosts read the
  same file); the adapter is the controller's (teleoperation requests motion
  through the controller, so sim, browser and hardware get one twist).
  Rejected: mixing in the viewer (duplicates the controller, breaks one
  execution path); limits in the bindings (per device, not per robot).
- *The session's command inputs carry the twist.* `EpisodeCoupler` already
  appended named command channels for Rhai programs; `ControllerProgram`
  gained `external` (an external Python simloop program) beside Rhai
  `sources`, so recording and replay come from the shared `Session` path.
  Rejected: a second command-input mechanism on the physical runtime.
- *Geometry goes to the controller as `--drive-json`.* The hello is fixed in
  Rust and the derived geometry is not in the profile file (it is derived
  from the model at load, never hand-copied). The resolved JSON is part of
  the recorded args. Rejected: a profile path argument (the controller would
  need the model too) or extending the hello (a protocol change for every
  client).
- *The binding is a file beside the model.* `<stem>.controller.json` (script
  path, args, the drive profile), found by convention. Rejected: a field in
  the CAD export (the controller is not a physical property).
- *Limit and deadman on the run thread, on simulation time.* Replay is then
  deterministic (the recorded actions are the limited ones). The deadman
  cannot expire while paused (no simulation time passes); a nonzero request
  is accepted only while Running, so nothing waits to move the robot at the
  next Run. Every queued command is applied before each period, so a
  release, stop or halt is never stuck behind stale requests.
- *Stops.* Release (every input neutral) sends one zero request; window
  focus loss stops what the devices were driving; Escape, the Stop button,
  the profile's `stop` (ramp at `stop_decel`) and `halt` (zero at once), a
  text field taking the keyboard and the deadman all stop. Leaving Robot
  mode drops the view and with it the run (`app/switch/leave.rs`).
- *Coexistence with presets.* The preset motion-channel path
  (`robot/motion.rs`, `Command::Motion`, `actions::motion_keys`) acts only
  for a preset with a motion config; drive input acts only for a run with a
  controller binding. A run is one or the other, so one key press never
  drives both.
- *Native gamepad: enabled* (`bevy_gilrs`; the lockfile resolved offline).
  Default bindings: keyboard W/S forward, A/D yaw, Q/E lateral, X `stop`,
  B `halt` (Space, Enter, Tab and Escape are the kit's activation and focus
  keys and are refused as bindings); gamepad left stick Y forward, right
  stick X yaw, left stick X lateral, South `stop`, East `halt`, deadzone
  0.15. Axes a robot's profile does not support are zeroed and named by the
  device layer (REST stays strict).
- *Golden vectors from the Rust reference.* `kinematics.rs` is standard
  library only, so `tests/fixtures/gen_drive_golden.rs` compiles it alone
  with `rustc` (about 2 s, no cargo) and prints
  `tests/fixtures/drive_golden.json`; the Rust test recomputes it and the
  Python unittest reads it. This was the one executed step of the batch.
- *Hardware.* Not driven. The same resolved drive and controller can run on
  a hardware host later; the twist path there is documented only (RV-39).
- *One stepped drive session, shared (rover-rest-flow).* `TwistState`,
  `DriveStatus`, `twist_json`, `check_inputs`, `HEARTBEAT_MAX` and the
  request vocabulary `DriveRequest` (with `from_fields` for REST and
  `interpret` against a profile) live in `sim_runtime::drive_host`, beside
  `DriveHost` (the `Session` plus its `TwistState`): the one place a driven
  session is stepped. Robot mode's run thread (`Sim::Controlled { host }`)
  and Build mode's robot-system run thread both own a `DriveHost`.
- *Build mode hosts the robot on `DriveHost`, not `SystemSession`.*
  `SystemSession` runs `Runtime::advance`, which would skip
  `PhysicalRobot::advance`'s slice retry and battery sampling: a second
  stepping path for the same robot. A system file instead hosts root
  instances from files (`SystemDocument::links`, `Command::LinkFile`): a
  `robot.articulated` linked to the `.simrobot.json`, a `control.external`
  linked to the robot's own `<stem>.controller.json` and optionally a
  `control.drive_limiter` linked to the `.drive.json` that binding names.
  Their parameters come from those files only; the document may give only
  the controller's `sense.command.<axis>` port members, and the only
  wiring is `limiter.twist.<axis> → controller.sense.command.<axis>`
  (`sim_runtime::system_robot::resolve`). Hosted instances are left out of
  the flattened model, and every `SystemSession` host refuses a document
  that has them (`system_builder::check_hosted`), so nothing runs a partial
  model. Rejected: a parallel binding format inside the system file (the
  binding file beside the model stays the one source, as Robot mode finds
  it), and attaching the external controller to `SystemSession`.

### Browser and Build mode (rover-browser-drive, 2026-10-03)

The same three layers drive the rover in the browser and in Build mode's
live robot-system run. **By reading, unexecuted**: no build, wasm build,
test, browser or viewer was run. Traces: `docs/rover-checklist.md` RV-38
and RV-41 to RV-43.

- **Browser adapter: a Rhai program on the Rust mixers.** The browser cannot
  start a Python process, so `robot.controller.json` gained an optional
  `embedded` program (`controller_binding::EmbeddedController`: `language:
  "rhai"`, `entry`, `files`, `config`; serde default, so older bindings read
  unchanged). `examples/wheeled-robot/drive-adapter.rhai` reads the four
  command channels, applies the controller-side deadman through
  `drive_update` (`kinematics::HeartbeatDeadman`, the Rust counterpart of
  simloop's `DriveState`), mixes through `drive_differential_mix` /
  `drive_mecanum_mix` with the geometry passed in its parameters (the
  resolved drive, derived from CAD with provenance) and integrates the wheel
  targets as `diff_drive_rover.py` does, with the same retry rollback keyed
  by sample time. No mixing arithmetic lives in Rhai, `sim-script` or JS.
  Rejected: compiling the mixers into a JS-callable controller (a second
  controller host), mixing in JS. The Python program stays the reference on
  the native seam; the browser path is a compatibility surface.
- **One scene builder.** `sim_runtime::embedded_drive::build` makes the
  embedded scene from the simrobot, the binding and the files it names
  (passed as text: the browser fetches, Rust parses): the profile resolved
  against the model (`controller_binding::resolve_drive`, shared with
  `load`), the captured adapter sources, the typed command inputs
  (`drive_inputs`: bounds from the profile, the heartbeat) and an
  `EmbeddedIdentity` (script, config, profile, model and CAD hashes). The
  embedded config (`drive-adapter.config.json`) is checked against the model:
  the servo boundaries' supply voltage and temperature, and a wheel target
  envelope wide enough for the horizon at the profile's top wheel rate.
- **`TwistState` on wasm32: no split.** `drive_host` imports only
  `controller_binding` and `session` items that are not target-gated (only
  `spawn_external`, `library_sha256` and `controller_binding::load` are
  native-only). `embedded_drive::DriveSession` wraps `EmbeddedSession` with
  a `TwistState`: each request is interpreted against the profile
  (`DriveRequest::interpret_with`) and raises the heartbeat; once per control
  period `TwistState::advance` (`kinematics::step`) runs on simulation time
  and `EmbeddedSession::set_inputs` sends `[f, l, y, heartbeat]` before the
  period is stepped, so the twists are the recording's input events. Replay
  is the embedded session's own `prepare_replay`, refused when the recorded
  identity's hashes differ (paths are recorded, not compared). Rejected:
  methods on `EmbeddedSimulation` (it has no profile or identity), limiting
  in JS.
- **sim-web exports** `default_drive_bindings`, `validate_drive_bindings`,
  `drive_device_axes`, `drive_binding_files`, `build_drive_scene`,
  `drive_request` and the `DriveSimulation` class. The page
  (`web/viewer`, preset `rover-drive`) reads keys by `event.code` and the
  Standard Gamepad per animation frame and only sends requests; stops are
  release, blur, hidden, Escape, the stop/halt actions and a text field
  taking focus.
- **Shared bindings module.** `sim.drive-bindings/1` (types, defaults,
  validation, deadzone, `supported_only`, describe) moved to
  `sim_runtime::drive_bindings`; `sim-spatial` keeps only the Bevy name
  mapping. W3C sign rule: the file's stick convention is gilrs's (up +); the
  Standard Gamepad's stick Y is +down, so `STICKS` carries sign −1 for Y and
  `browser_axes` applies it in Rust.
- **Mode-neutral device input.** `sim-spatial/src/drive_input/` is its own
  plugin: one poller (`input::devices`, `InputSet::Window`, every mode)
  reads the devices only for the current mode's live `DriveTarget` (written
  by Robot's `controls::drive_target` and Build's `robot_run::drive_target`
  before `InputSet::Window`) and writes `Act<DriveDevice>` stamped with that
  mode. Robot forwards it as `RobotAction::Drive` before its apply; Build
  drains it into `Builder::drive_request` → `RunControl::Twist` →
  `DriveHost`. Leaving Build while driving sends the owed stop; a target
  that disappears or changes while driving gets one stop. Robot-only
  inputs (the Leg calibration panel's Q/A/Z, Build's editing keys) are the
  target's `owned_keys`. Build's run panel shows the bound keys and the
  requested twist in a strip under the viewport. Rejected: a second key loop
  in Build mode; the poller writing each mode's action type (it would import
  both modes).

### Where it is today

Goal 4's software steps are complete by reading (unexecuted): RV-01 to
RV-38 and RV-40 to RV-43. RV-39 (hardware) is a run sheet; the agent never
drives hardware. Known gaps: the browser path's realtime performance is not
measured; Build robot runs keep no run record and draw no robot.

focus-safety-closure (2026-10-03; by reading, unexecuted; traces in
`docs/rover-checklist.md` "Safety closure"):
- **One disarm path.** `drive_input::Disarm` (a Message, `DISARM_RULE`) is
  written by each mode's one apply when it accepts a stop, halt, named
  action, Pause or Reset from any origin (Robot: `robot::actions::apply`;
  Build: `builder::system_actions::apply`), in `ViewerSet::Actions`, and
  read every frame by the one poller (`drive_input::input::devices`,
  `InputSet::Window`, no `run_if`). A key, stick or button held when the
  stop was applied is ignored until released, while one first pressed after
  it drives (focus-final-leftovers: Robot's apply writes none for the
  devices' own requests, which it reads itself, so no echo swallows a fresh
  key); if the devices were driving they send one Stop
  first. Build writes it for a Pause only when that Pause paused a running
  run. `LiveTarget::run` carries the run's identity (Robot: file, run
  generation and replay in progress; Build: file, run id, run start number
  and generation), so a reload, a replay's start or end and a new run
  disarm held inputs through the target change.
- **Pause invalidates a live request.** `sim_runtime::drive_host::PAUSE_RULE`:
  `TwistState::pause` (shared with a replay's end) zeroes the request and
  expires the deadman, so on resume the profile's on-loss rule runs until a
  fresh request. Called from Robot's run thread (`Command::Pause` →
  `Sim::pause_drive`), Build's robot-system run (`RunControl::Pause` →
  `DriveHost::pause`) and the browser (`setPlaying(false)` → `drive_pause`
  → `DriveSimulation::pause` → `DriveSession::pause`; a no-op while
  replaying).
- **One deadman bound.** `sim_domain_control::drive::kinematics::deadman_bound`
  (timeout strictly longer than one finite, positive period), called by
  `drive_geometry::check_deadman` and the `control.drive_limiter` element.
- **One stated motor ambient.** The parser keeps `motors[i].thermal.ambient_c`,
  the motor's datasheet rating ambient; `PhysicalModel::motor_ambient(i)`
  returns it with provenance (else `world.ambient_c`, labelled stated or the
  parser's 20 °C default). It is the motor unit's resistance and derating
  reference in the embedded session and the native `PhysicalRobot`, and the
  value the embedded drive's servo-temperature check uses (its JSON re-read
  is gone). The thermal network's environment stays `world.ambient_c`.
  Motor resistance and torque change for models that state a per-motor
  ambient: gait qualifications need a rerun.
- **Build `system_state`** carries `bindings` and `drive_input` from the one
  serializer Robot's `robot_state` uses (`drive_input::insert_state`).
- **Replay cancel.** Robot's drive replay cancels through the run thread
  (`jobs::RunThread`, `Command::CancelReplay`), ends Cancelled with
  "cancelled at n/N seam periods, sim time t s of T s" and leaves no live
  request. Build mode has no drive replay; its run-history replay cancels
  through `jobs::Job`.
- `web/tests/drive_input.mjs` is listed in `.github/workflows/browser.yml`
  (not yet run).

Earlier history. Written and traced by reading on the wheeled robot: the drive library and
registry description, the geometry derivation, the golden file (generated),
the Python port and the rover controller, the runtime's external program and
binding loader, the Robot-mode run thread, recording and replay, the bindings
and their settings group, the one drive action with keyboard, gamepad,
`system_ui` and REST `robot_drive`, and the inspector's Drive block. Added by
rover-rest-flow: the shared `sim_runtime::drive_host`; Build mode's robot
system (linked files, `link_file`, a live run on `DriveHost`, REST
`system_drive` and the run panel's Forward, Back, Left, Right and Stop
buttons; no run record and no robot drawing for that run yet); the CAD export's `rigid` kind, `cad_sha256`
stamping and `seq`/`recent`; and the committed script
`clients/python/examples/build_rover_over_rest.py`, which builds the rover
over CAD REST, rigs and annotates it, wires and drives it in Build mode and
drives it in Robot mode. None of it compiled or run: all by reading,
unexecuted. Traces: `docs/rover-checklist.md` RV-01 to RV-07 and RV-40.
Browser and Build-mode device driving followed in rover-browser-drive
(above).
