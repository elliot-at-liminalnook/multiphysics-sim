# Native viewer consolidation: workflow inventory and shell decision

Status: inventory and decision, 2026-09-29 (task B0/T0.1). Documentation only;
nothing here has been migrated yet. Pointers were checked against source at
commit `6368c7c4`. Items marked *unverified* were not confirmed in code or on
screen.

## 1. Summary and decision

**The shell is `sim-spatial` (Bevy 0.16, `crates/sim-spatial`).** Every other
workflow moves into it as a mode, tab or dock. No new viewer is created.

Reasons, from source:

- **Automation surface.** Each window serves loopback REST (`--api-port`,
  default 8421, `crates/sim-spatial/src/main.rs:Args`). `rest.rs:capabilities`
  lists display, camera, render, `screenshot`, selection and annotation
  commands. Build mode adds about 30 `system_*` commands (`rest.rs:system_execute`,
  line 225) and lesson mode adds 25 `lesson_*` commands (`lesson/rest.rs`).
  `system_ui` (`builder/ui_api.rs:Request`) discovers live controls and runs
  them through the same `BuildAction` handlers as a click. This makes it the
  only native app that `ui_capture.py` can drive end to end today.
- **It opens and edits systems.** `--system FILE` → `main.rs:build_mode` →
  `Builder::open` (`builder.rs:265`). Every edit goes through
  `sim_system::Command` and the shared on-disk undo store
  (`Builder::apply`, `builder.rs:431`).
- **It already runs the shared runtime off the UI thread.**
  `builder.rs:run_thread` (line 1147) owns a `sim_runtime::system_session::SystemSession`
  on a background thread. The module header says the window "never integrates
  anything itself" (`builder.rs:7`). Studies run on their own thread
  (`run_study`, `builder.rs:904–914`).
- **It has the most panels.** Library, Outline, Studies, References and Notes tabs
  (`builder/ui.rs:430`), an inspector (`ui.rs:572`), a graph dock
  (`builder/graphs.rs`), physics overlays (`physics_view.rs`), discussions
  and Codex (`builder/discussion.rs`, `builder/agent.rs`), drag placement on a
  worker (`builder/placement_worker.rs`), a Detailed/Realtime fidelity toggle
  (`ui.rs:391`), and lessons (`lesson/`).
- **It has a 3D stack with CAD display models.** OBJ catalog from CAD
  (`models.rs`) and scanned places (`place_view.rs`).

Rejected candidates and their disposition:

| App | Why not the shell | Disposition |
|---|---|---|
| `sim-viewer` (eframe/egui schematic, `crates/sim-viewer/src/main.rs`) | Good 2D schematic and experiment review. Its REST covers only a subset of system edits (`system`, `system_state`, `system_level`, `system_select`, `system_undo`, `system_redo`, `system_grid`, `system_move`; no `system_run`, studies or `system_ui`). It has no 3D view. A second GUI toolkit in the same window as Bevy would need a bridge. | Keep as legacy until parity. Its reusable parts are already libraries: `sim-diagram` (schematic layout/projection, `TimeGraph` plot) and `sim_runtime::{experiment_study, controller_refinement::*}`. Port views into Bevy rather than embedding egui. |
| `sim-app` (Bevy, `crates/sim-app/src/main.rs`) | No REST API and no system files. Both scenes step physics inside a Bevy `Update` system on the UI thread (`cad_app.rs:advance`, line 176; `phenomena_app.rs:advance`, line 180). This conflicts with the off-UI-thread rule. | Keep as legacy. Migrate the `--scene cad` workflow (simrobot live view) and the phenomena gallery into sim-spatial on a worker thread, then retire it. |
| Browser (`web/viewer`, `web/system-builder`, `sim-web` WASM) | The native-first direction supersedes it as the primary surface. | Preserved: it is the AGENTS.md realtime browser-walking surface. It is kept until native parity is shown and stays as a compatibility target afterwards. |
| RoboCAD (Python/OCCT/Qt, `cad/robocad/ui/app.py`) | CAD kernel and authoring. Rewriting OCCT is out of scope. | Remains the CAD service behind REST (§3). |

`server.py` only serves the repository's root `index.html` planning page on
port 8000 (`server.py:main`). It is not the WASM workspace, which is served by
`web/serve-viewer.mjs`. It needs no migration.

## 2. Workflow inventory

Status key: **present**: usable in sim-spatial today; **partial**: some of it
is there, with the gap named; **absent**: not reachable in sim-spatial.

### a. Opening and editing systems
- **Entry today:** `sim-spatial --system FILE` (`main.rs:build_mode`). Also
  `sim-viewer --system FILE`, the `sim-system` CLI (`crates/sim-runtime/src/bin/sim-system.rs`)
  and RoboCAD "Open in builder" (`cad/robocad/ui/references.py:open_builder`,
  which spawns `sim-spatial --system … --schematic --api-port 0`).
- **Reusable layer:** `sim_system::{document, commands, store, resolve, flatten, library, snap}`;
  `sim_runtime::system_builder::compile`.
- **Shell status:** *partial*. Editing, levels, group/swap, undo/redo and
  snapping are present. **Opening another system in the same window is present**
  (commit 6dab907a, verified natively in T3.2). New-system creation and recent
  files are absent.
- **Open path (one function):** `Builder::open_system(path)`
  (`crates/sim-spatial/src/builder/open.rs`) is called by three surfaces:
  - the build-mode **Systems** sidebar tab. It has a path field (Enter opens)
    and a list of `*.system.json` files found under `examples/systems-builder`,
    the library, and the current file's folder;
  - `system_ui`: the tab `{"tab":"systems"}`, the rows `{"open_system": "<abs path>"}`,
    the path field `"open_system_path"` (then `input … submit: true`) and
    `"cancel_open"`;
  - REST `system_open {"path"}`, listed in `/v1/capabilities` with an example.
    It returns a pending job; poll it.

  The file is loaded, validated and compiled on a worker thread. The measured
  load plus compile time was 0.043 s for the winch and 0.074 s for the board
  (debug build). `Builder::finish_open` is the only place the builder is
  replaced. The document, path, runs (`<name>.runs/`), replay, study, graphs,
  selection and level all come from the new file. It also swaps the 3D scene,
  retargets annotations to `<new>.annotations.json` and rebuilds the model
  catalog. `system_state.open` reports `pending`, `last` (path, title, revision,
  runs, annotations, notes, load_seconds, or the error), the discovered
  `systems`, the `annotations` sidecar and the `schematic` link.
- **Guards** are checked when the open is requested and again just before the
  new system is installed:

  | In progress | Handling |
  |---|---|
  | Text-field or discussion draft, comment edit | Refused: "Not opening X: a text field draft is open ("open_system"): submit or cancel it. Y stays open." |
  | Placement drag | Refused, naming the drag |
  | Running study | Refused, naming the study and its progress |
  | Running replay | Refused, naming the run id |
  | Codex answering a discussion | Refused |
  | Live run | Stopped and saved to the **old** file's `.runs/` with the note "stopped to open another system". The status line and `open.last.notes` name the saved file. A run under 0.1 s is not kept, and the note says so. If the save fails, the open is refused. |
  | `--schematic` window | Detached, not retargeted. The note says it still shows the old file. |

  A missing file is refused at once ("Could not open P: no such file."). A file
  that fails to load, validate or compile is an error on the worker that names
  the path, and the current system stays open and unchanged. The open itself
  writes no runs or annotations. An explicit `--annotations FILE` stays with
  the launch file; other systems use their own sidecar, and reopening the
  launch file restores it.
- **Remaining limits:**
  - There is no OS file picker (only the path field plus the discovered list),
    no recent files and no new-system action.
  - The `--schematic` sim-viewer window is not retargeted.
  - A file that does not compile cannot be opened in-app (as at launch).
  - Opening a file creates its hidden `.<file>.agents/` Codex lock dir, as a
    launch does.
  - Lesson mode keeps its own sandbox flow.
  - The native proof activates controls through REST `system_ui`, not pointer
    gestures.
- **Source owner:** system file `*.system.json` (`sim.system/1`); topology
  lives in examples/library.
- **Verified evidence (T3.2):** `.claude-pair/captures/T3-open-system/`
  (`drive_open.py`, `capture.json` with ok=true). The run launched on a board
  copy, opened the winch copy through the system_ui Systems row, and got the
  REST missing-path error with the winch kept. A path-field draft refused the
  switch, as the screen shows. A 0.57 s live winch run was saved to
  `work/winch.runs/` while the board reopened with its own empty runs list.

### b. CAD/geometry and physical properties
- **Entry today:** RoboCAD (`cad/run.sh`; REST `cad/robocad/api.py`, port 8420
  and up, one port per window, `ui/app.py:1784`). Also `sim-app --scene cad --model X.simrobot.json`
  (`cad_app.rs`, which watches the file and rebuilds on save) and the `sim-cad`
  CLI (cad/README).
- **Reusable layer:** `sim_phenomena::scenarios::cad_robot` / `cad_physical`
  (used by sim-app), `sim_domain_robot::PhysicalModel`,
  `sim_runtime::physical`, `sim_runtime::part_fit`, and the
  `sim-runtime cad-params` CLI (per builder-roadmap M7; *unverified* here).
- **Shell status:** *partial*. sim-spatial draws CAD-exported OBJ display
  models (`models.rs`, `library/models`, a system's own `models/`). Gaps:
  1. no view or inspection of a `.simrobot.json` physical model (mass,
     inertia, joints, provenance);
  2. no way to trigger CAD edits or exports from the shell;
  3. the CAD link status (path and SHA-256, `cad/robocad/system_link.py`) is
     not shown in the shell.
- **Source owner:** CAD (`.rcad` → `simrobot` v3 export). Display models are
  presentation only.
- **Dependencies:** a CAD service client (§3). Move simrobot live stepping
  from the sim-app UI thread onto a worker (§4).
- **Acceptance evidence:** open a simrobot file in the shell. The inspector
  shows mass and inertia with measured/derived/estimated labels. After a
  RoboCAD REST `PATCH /nodes/{id}` material change and export, the shell
  reloads and the values change. Screenshots are taken before and after, and
  the provenance label is visible.

### c. Schematic and spatial views
- **Entry today:** spatial is sim-spatial (all modes). The schematic is a
  separate `sim-viewer` process, spawned by `sim-spatial --schematic`
  (`main.rs:build_mode`, where `current_exe().with_file_name("sim-viewer")`)
  and linked through a file session (`sim_inspect::selection::native`,
  `linked.rs`); also `examples/systems-viewer/run-linked.sh` and `run-live.sh`.
- **Reusable layer:** `sim_inspect::{model, selection, spatial}`,
  `sim_diagram::{layout, projection}` (egui-bound drawing in `sim_diagram::lib`).
- **Shell status:** *partial*. 3D is present. The schematic is a second
  window and toolkit. systems-builder-progress "Remaining" says selection is
  not linked between the two windows in build mode, and the build-mode spawn
  passes no `--selection-link` (verified in `build_mode`).
- **Source owner:** Rust runtime (description and identities); display layout
  is presentation.
- **Dependencies:** split `sim_diagram::layout/projection` (graphics-free)
  from the egui painter, then draw the schematic as a Bevy view or tab over
  the same `SystemDescription`.
- **Acceptance evidence:** one window. `select` a component and take a
  screenshot showing it highlighted in both the 3D view and the schematic pane.
  `state` returns one selection.

### d. Library / parameters / typed connections
- **Entry today:** sim-spatial Library tab and cards (`ui.rs:library_tab`,
  457), inspector parameters, ports, Snap on, and REST `system_component`,
  `system_suggest`, `system_snap`, `system_publish`, `system_where_used`,
  `system_sync`, `system_expose`, `system_parts`. Also `sim-system catalog` and
  `library/CATALOG.md`.
- **Reusable layer:** `sim_core::ComponentNotes` and the registry
  (`sim_runtime::system_registry()`), `sim_system::{library, snap}`, `sim_parts`.
- **Shell status:** *present*. The progress notes say these panels were
  exercised over REST but not inspected on screen, so on-screen quality is
  *unverified*.
- **Source owner:** registry (element notes, typed ports and units) plus
  `library/systems`, `library/parts`.
- **Dependencies:** none for parity.
- **Acceptance evidence:** `system_ui` activates the Library tab and a
  motor card, with a screenshot of the card (ports, units, parameters).
  `system_snap` on a compatible port, then `system_state` shows the new
  instance and a net. `system_undo` restores the previous revision.

### e. Live controls and graphs
- **Entry today:** sim-spatial Run/Pause (`builder.rs:run_start`/`run_pause`,
  REST `system_run {"action":"start"|"pause"}`), graph dock
  (`builder/graphs.rs`, up to 4 charts, REST `system_plot`), overlays
  (`display` `set_overlay`: power, forces, current, heat, trails), and the
  Detailed/Realtime toggle. Also sim-viewer `live_ui.rs` (worker-backed
  graphs) and browser presets with sliders.
- **Reusable layer:** `sim_runtime::system_session::SystemSession`,
  `sim_inspect::{live, plot, animation}`.
- **Shell status:** *present* for system files. Gaps: no step or reset control
  in the builder run (only start and pause are handled in
  `rest.rs:system_execute`, lines 251–252). Graphs have no shared time cursor or
  scrub.
- **Source owner:** Rust runtime.
- **Dependencies:** `SystemSession` already supports reset through its factory
  (`system_session.rs` header). Wire it up.
- **Acceptance evidence:** `system_run start`, wait, then `system_plot` to pin
  an observable. A screenshot shows a moving trace. `system_state` sim time
  increases across two polls, stays unchanged after `pause`, and returns to 0
  after reset.

### f. Annotations and source links
- **Entry today:** sim-spatial Notes tab and pins (`builder/discussion.rs`,
  `builder/markers.rs`, REST `system_discussions`, `system_agent`,
  `system_context`). Sidecar notes: `annotations` command, `notes.rs`, and
  `<file>.annotations.json`. Markdown and source previews (`markdown.rs`,
  `sim-markdown`). sim-viewer `annotations_ui.rs`. RoboCAD threads
  (`/threads`, `cad/robocad/annotations.py`).
- **Reusable layer:** `sim_annotate`, `sim_inspect::annotations`,
  `sim_system::display` (threads bound by lineage), `sim_model_context`, `sim_agent`.
- **Shell status:** *partial*. System discussions are present. Gaps: CAD
  threads (RoboCAD `/threads`) are not visible in the shell, and there is no
  jump from a shell part to its CAD node or source line in CAD.
- **Source owner:** system document (discussions) and CAD document (CAD threads).
- **Dependencies:** the CAD service client (§3) and a mapping from system
  instance to CAD node id (the CAD link stores only path and hash today).
- **Acceptance evidence:** `system_ui` Annotate on a part with a typed comment
  (`Input` then submit). A screenshot shows the pin, and `system_discussions`
  lists it with its link. After `system_undo` the thread is gone.

### g. Robot teleoperation
- **Entry today:** browser only. `web/viewer/viewer.js:415` (keydown handling),
  `web/viewer/motion-commands.mjs` (maps keys to typed velocity channels; "UI
  command mapping only. Controllers and motion clocks remain in Rust/Rhai"),
  and `sim-web` `EnvironmentSimulation` / `EmbeddedSimulation`
  (`crates/sim-web/src/lib.rs:75,143`). Hardware mirroring:
  `web/viewer/hardware-sync.mjs` (FPGA bridge, token gated). `sim-app --scene cad`
  arrow keys set joint targets directly (`cad_app.rs:keyboard`). That is joint
  jogging, not motion requests through a walking controller.
- **Reusable layer:** `sim_runtime::{session, embedded, environment, walking_task, steered_reference}`,
  `sim_domain_control::motion_clock`.
- **Shell status:** *absent*. sim-spatial's WASD is camera fly-through in
  place mode only (`place_view.rs` header).
- **Source owner:** example config (preset scene and controller recipe,
  `web/viewer/presets.json`) and the Rust runtime.
- **Dependencies:** a native host for `EmbeddedSession`/environment on a
  worker thread, a robot scene renderer (links as CAD meshes), and a key →
  typed motion-channel mapping ported from `motion-commands.mjs`, including the
  heartbeat and zero-on-release. Hardware sync stays out of scope (never
  driven by this work).
- **Acceptance evidence:** a REST motion-command equivalent of holding W
  (a REST-issued command is not a key press). `state` shows the controller's
  commanded velocity and increasing body x. Screenshots are taken before and
  after. Releasing produces a zero request.

### h. Recordings / replay
- **Entry today:** sim-spatial "Save run" (`ui.rs:398`, `Builder::save_run`
  at `builder.rs:797`, REST `system_save_run`), the Studies tab "Runs" list and
  "Compare" (`ui.rs:1202–1219`, `Builder::compare_runs`, REST
  `system_compare_runs`). Lesson frame playback (`lesson/frames.rs`). Browser
  Save run / Replay inputs (`sim_runtime::session::Recording`,
  `session.rs:212,457`). sim-viewer experiment recordings
  (`controller_refinement::recording`).
- **Reusable layer:** `sim_runtime::run_history::{record, save, load, list, replay, compare}`
  (`run_history.rs:81–117`), `sim_runtime::session::Session::replay`,
  `sim_runtime::embedded::EmbeddedRecording`.
- **Shell status:** *partial*. Save and compare are present. Gaps: no
  **replay/verify** of a saved run (`run_history::replay` exists but sim-spatial
  never calls it), no scrub cursor over a saved run, and robot `Recording`
  replay is browser only.
- **Source owner:** Rust runtime. Records live in `<system>.runs/`, are
  outputs, and are never deleted.
- **Dependencies:** a background job with progress and cancel. The pattern
  already exists for studies.
- **Acceptance evidence:** see §5.

### i. Experiments / gait studies
- **Entry today:** gait lab CLI
  (`cargo run --release -p sim-runtime --example gait_lab -- export|validate|evaluate|poses|steer|tune`,
  `crates/sim-runtime/examples/gait_lab.rs`; library `sim_runtime::gait_lab`),
  `compare_gait_search`. Web leaderboard (`web/viewer/leaderboard.js`,
  `web/leaderboard/`). Experiment review in
  `sim-viewer --experiments DIR` (`experiments_ui.rs` over
  `sim_runtime::experiment_study`, `experiment_comparison`). System studies in
  the sim-spatial Studies tab (`ui.rs:studies_tab`, 1172; REST `system_study`,
  `system_study_result`). RoboCAD experiments (`cad/robocad/ui/experiments.py`,
  `experiment_worker.py`).
- **Reusable layer:** `sim_runtime::{gait_lab, gait_playback, experiment_study, experiment_search, system_study, fidelity}`.
- **Shell status:** *partial*. System compare/sweep studies are present. Gait
  lab reports, gait playback, the leaderboard and experiment review are absent
  from the shell.
- **Source owner:** example config (study configs, YAML gait files), with
  results in `runs/` (preserved, never deleted).
- **Dependencies:** a read-only report browser first (a gait report YAML with
  its fidelity label and gates), then gait playback on the robot scene (needs g's
  renderer), then launching evaluations as cancellable jobs (the gait lab
  already honours `OUT_DIR/STOP`). The runtime fingerprint means evaluations
  must be requalified after code edits (gait-lab README).
- **Acceptance evidence:** open a gait-lab output directory. A screenshot shows
  the report table with the fidelity (fast/detailed) and gate results exactly as
  in `report.yaml`. A REST query returns the same numbers as the file.

### j. Measured actuator models and calibration inspection
- **Entry today:** `actuator_registry` example CLI (`hash|limits|apply|check`,
  `crates/sim-runtime/examples/actuator_registry.rs`) over
  `examples/actuators/hx30hm/accepted/registry.json`. Calibration and FPGA
  review in `sim-viewer --experiments` (`experiments_ui/{refinement, fpga_ui, motor_response_ui, power_ui}.rs`).
  Browser calibration panel (`web/viewer/calibration-ui.mjs`, token gated,
  talks to `serve_actuator_calibration.rs`, which drives hardware and is out of
  scope). `web/motor-bench/`. The sim-spatial lesson bench page only asks a
  calibration server named by `SIM_BENCH_URL` (`lesson/extras.rs:193,520`).
- **Reusable layer:** `sim_runtime::{actuator_registry, part_fit, acquisition::calibration, controller_refinement::{calibration, calibration_data, evidence, fpga_review, motor_response}}`,
  `sim_domain_robot::actuator_profile`.
- **Shell status:** *absent* for the registry and calibration review.
- **Source owner:** actuator registry (single source of measured values),
  bench data in examples and `runs/`.
- **Dependencies:** read-only panels first. `Registry` load and `check`
  staleness (family hash vs consumer) run on a worker, and
  measured/derived/estimated labels come from the family file. No hardware I/O
  in the shell.
- **Acceptance evidence:** open the registry. A screenshot lists families with
  hash, role and joint limits at the stated supply voltage. A stale consumer
  file shows "stale" with both hashes, matching the `actuator_registry check`
  CLI output.

## 3. External UI that remains

- **RoboCAD (Python/OCCT/Qt) stays** for geometry authoring: sketching,
  booleans, direct edits, print splitting, and physical property editing on
  B-reps. Rewriting OCCT in Rust is not planned. For now, geometry edits
  require the RoboCAD window.
- **Proposed boundary:** RoboCAD REST (`cad/robocad/api.py`) as a CAD
  service. It runs headless (`python -m robocad.api model.rcad --port 8420`) or
  inside the GUI, where requests are marshalled onto the Qt thread, so undo
  stays single. The shell would use a small Rust client (a new module in a
  shared crate, not in the UI) for: `GET /robot`, `/physical`, `/nodes/{id}`
  (mass properties), `/threads`, `POST /export`, `/render` thumbnails, and
  `PATCH`/`/ops/*` edits. The shell never writes CAD files itself. Edits go
  through the CAD command layer, so CAD undo and provenance stay intact. Long
  calls run on a worker with progress and cancel.
- **Open questions:**
  1. Which RoboCAD endpoints are safe to call while a person is editing (unsaved
     edits must be preserved; AGENTS.md).
  2. How to map a system instance to a CAD node (the link is path + SHA-256 only,
     `system_link.py`).
- **Browser:** preserved. AGENTS.md requires realtime browser walking. The
  WASM workspace (`web/`, `sim-web`) stays the teleoperation and walking
  surface until workflow g is shown natively, and it remains a supported
  compatibility target afterwards. Hardware calibration and sync
  (`calibration-ui.mjs`, `hardware-sync.mjs`) stay in the browser. They are
  hardware paths and are not migrated in this effort.
- **sim-viewer:** stays for the schematic and `--experiments` review until c,
  i and j reach parity.

## 4. Constraints check (risks found in current code)

- **Physics on the UI thread:** `sim-app` advances physics in Bevy `Update`
  (`cad_app.rs:176`, `phenomena_app.rs:180`). It caps work at one grid step per
  frame but still blocks the frame. Do not port this pattern. Host these
  scenes on a worker, as sim-spatial's `run_thread` does.
- **Small synchronous I/O on the UI thread in sim-spatial:** `save_run`
  (JSON write), `compare_runs` (loads records), and `Builder::apply` (store
  write) run inline in action handlers. They are fine for small files. Replay
  (§5) must not be added inline, because `run_history::replay` resimulates.
- **Duplicated physics:** none found in sim-spatial. The graph and overlay
  modules state that they only read committed frames (`builder/graphs.rs`,
  `physics_view.rs` headers). The browser JS mapping is command-only
  (`motion-commands.mjs` header). `sim-app` scenes use shared
  `sim_phenomena` scenarios rather than their own integrators.
- **Fidelity labels:** the builder shows Detailed/Realtime and a staleness
  note for realtime measurements (`ui.rs:645`). Gait-lab fidelity
  (fast/detailed) and actuator measured/estimated status have no native
  display yet. Migrations must carry them through, not drop them.
- **Stale frames:** REST edits check `expected_revision` (`rest.rs:231,236`)
  and placement checks the scene hash. Saved runs note "edited while running"
  (`builder.rs:811`). Whether the graph dock marks samples from an
  earlier revision after a hot swap is *unverified*.
- **Build-mode schematic selection is not linked** (see c). Two windows can
  show different selections.
- **`sim-spatial --help` text** ("no physics stepping", `main.rs` `about`)
  is accurate for the window. Build mode does host a background
  `SystemSession` in-process, though, and the text should say so when docs are
  next touched.

## 5. First implementation slice: replay and verify a saved run

**Why this slice:** it closes a real gap (h), reuses one existing
runtime function, touches only sim-spatial's Studies tab and REST, and
demonstrates the one-execution-path rule. The saved run is resimulated by the
same `system_builder::simulate` that headless runs use, and the shell reports
the agreement. It has no physics, CAD or hardware risk.

**User-visible outcome:** in the Studies tab, each saved run gets a
**Replay** button. Clicking it starts a background job and shows "Replaying
…" with a Cancel button. When done, the row shows "Reproduced: max relative
difference 3e-12" or the error. Runs noted "edited while running" show a
clear refusal or warning, because their recorded document was edited
mid-run. The result appears in `system_state` and in a new REST command.

**Files likely touched:**
- `crates/sim-spatial/src/builder.rs`: `BuildAction::ReplayRun(id)` and
  `CancelReplay`, `Builder::replay_run` spawning a thread that loads the record
  and calls `sim_runtime::run_history::replay`, plus job state following the
  `run_study` pattern (`builder.rs:904–921`).
- `crates/sim-spatial/src/builder/ui.rs`: a button and result per row in
  `studies_tab` (runs list around line 1206).
- `crates/sim-spatial/src/rest.rs`: `system_replay_run {"id": …}`
  capability, dispatch, and result in `state_json`.
- Possibly `crates/sim-runtime/src/run_history.rs`: a cancellable variant
  (a flag checked between steps) if `system_builder::simulate` exposes no
  hook. Otherwise cancel means "discard the result". The report must say
  which one was implemented.

**Reuse points:** `run_history::{load, replay}`, the studies job pattern and
progress UI, `system_ui` controls (the new button is automatically discoverable).

**Acceptance evidence** (`ui_capture.py`, rebuilt `target/debug/sim-spatial`):

```
python3 tools/claude-pair/ui_capture.py --out $PAIR_CAPTURES/T1-replay --script replay.json \
  -- --system examples/systems-builder/worm-drive/winch.system.json
```
`replay.json` outline:
1. `{"command":"system_run","args":{"action":"start"}}`, `{"wait":2}`,
   `{"command":"system_run","args":{"action":"pause"}}`
2. `{"command":"system_save_run","args":{"note":"slice-1"}}`, then
   `{"command":"system_state"}` to read the run id
3. `{"command":"system_ui","args":{"action":{"operation":"tab","tab":"studies"}}}`
   (payload shape verified in `builder/ui_api.rs:Request`), `{"screenshot":"runs-list"}`
4. `system_ui` `controls`, then `{"operation":"activate","id":…,"ui_revision":…}` on the
   Replay control for that run id (same handler as a click; not a pointer gesture), `{"screenshot":"replaying"}`
5. Poll `system_state` until the replay reports done, then `{"screenshot":"replayed"}`
   showing the difference value.
6. Negative path: `system_replay_run` with an unknown id returns an error
   naming it, and one run is cancelled mid-way, with state showing "cancelled"
   and no result.

Honest limit: a run saved from the live window was paced in realtime.
Replay resimulates headless from sim time 0 with the recorded config, so a
non-zero difference is a finding to report, not something to hide by
loosening the comparison.

**Focused checks:** `cargo check -p sim-spatial`;
`cargo test -p sim-runtime --test live_iteration`
(`runs_are_kept_compared_and_replayed_exactly`);
`cargo test -p sim-spatial --lib` (add a unit test for the job state and the
unknown-id error).

**Next slices, in order:**
1. **Open another system in-app** (a): `system_open` REST plus an Open control,
   with draft preservation. This removes the relaunch.
2. **Actuator registry inspector** (j): a read-only panel over
   `actuator_registry::Registry` and `check`, with hashes, roles, limits and
   measured/estimated labels, loaded on a worker.
3. **Gait-lab report browser** (i): read-only `report.yaml` and journal view
   with fidelity and gate labels. Launching evaluations comes later.
4. **Schematic pane in the shell** (c): a graphics-free `sim_diagram` layout
   drawn in Bevy with shared selection. The sim-viewer window then becomes
   optional.
5. **simrobot live view on a worker** (b/g groundwork): port `sim-app --scene cad`
   into a sim-spatial mode through the `sim_phenomena::scenarios::cad_robot`
   worker. This is the prerequisite for native teleoperation and gait playback.

## 6. Launch path

Today (build mode, the shell):

```
cargo run -p sim-spatial -- --system examples/systems-builder/motor-driver-board/board.system.json
```
Once it is running, open another system from the **Systems** sidebar tab (a
discovered row, or a path in the field and Enter), or send REST
`system_open {"path": …}`. There is no relaunch. Other modes today are `--lessons lessons`, `--place DIR`, and `--description/--spatial`.
Separate apps are still needed for the schematic and experiments (`sim-viewer`),
simrobot and phenomena (`sim-app`), CAD (`cad/run.sh`), and walking and
calibration (browser, `web/README.md`).

After consolidation: a single `cargo run --release -p sim-spatial -- [FILE]`,
where FILE may be a system, simrobot, lesson directory or gait-lab output, opened
in one window with modes and tabs. RoboCAD runs as a CAD service (its window is
still used for geometry authoring), and the browser remains for realtime
walking and hardware calibration.

## Stale or unverified claims found in docs

- `systems-builder-progress.md` item 5 says the REST `system_run` and
  `system_import_image` commands exist "on both viewers". sim-viewer registers
  neither (its capability list has only `system`, `system_state`,
  `system_level`, `system_select`, `system_undo`, `system_redo`,
  `system_grid`, `system_move`).
- `systems-viewer-plan.md` checkpoint 4 (recording and linked replay) is
  unchecked. Save and compare now exist in sim-spatial (`run_history`), but
  replay and the shared cursor do not, so the plan is partly overtaken. Its
  "decide the final single-window host" is decided here.
- README quick start `sim-app --exhibit quadruped` is *unverified*: no exhibit
  named "quadruped" was confirmed in this pass.
