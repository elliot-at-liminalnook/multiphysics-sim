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
  models (`models.rs`, `library/models`, a system's own `models/`), and since
  batch simrobot-inspect it has a **read-only robot mode**:
  `sim-spatial --robot FILE.simrobot.json` (`crates/sim-spatial/src/robot.rs`,
  commits 1562f60c, 07b94d6a, 5bad56fb, 255b2d15).
  - *Loading:* a `std::thread` worker reads the file once, parses it with the
    shared `sim_domain_robot::PhysicalModel::parse`, triangulates each link's
    collision hull with the shared `Collision::display_triangles` (sim-app's
    cad view calls the same function), and computes the CAD link status. The
    window shows "Loading …" meanwhile; a bad or unparseable file shows an
    error naming the path and the window stays up. `--validate-only --robot
    FILE` prints the link count or exits 1 with the path-naming error.
  - *View:* one mesh per link at the stored `com` with identity rotation
    (model Z-up turned to Bevy Y-up). One selection is shared by the link
    list, 3D picking and `system_ui` (`select_link` controls); the selected
    link is highlighted.
  - *Inspector* (tabs Link / Joints / Drives / Source, all `system_ui`
    controls, plus `inspector:scroll_*`): link mass, com, full inertia,
    material and density from the file's materials map; the joints touching
    the link with type, parent/child, axis, origin, limits, friction,
    clearance, backlash, damping and `drive_backlash` with its typed
    provenance; motors, transmissions, battery, actuator profiles with family
    content hashes; `uncertainty` as stored; identification entries; the
    `source` block and `benchmark_assumptions` verbatim. REST `robot_state`
    carries the same values at full precision.
  - *Provenance rule:* a measured/derived/estimated label is shown only where
    the file carries a typed one (joint `physics.drive_backlash.provenance`,
    actuator-profile parameters). Free text the file carries (link
    `mass_sources`/`member_names`, joint `physics.source`, motor `notes`) is
    shown verbatim as the file's text, never mapped to a label. Every other
    value is marked "no per-value provenance in export" and points at the
    source notes.
  - *CAD link status* (`sim_domain_robot::cad_link`, hashed on the worker):
    `current`, `stale` (both hashes), `missing` (every path tried),
    `no_recorded_hash` (on-disk hash shown), `no_source_file`, `unreadable`.
    Resolution rule: an absolute `source.file` is used as is; a relative one is
    tried against the simrobot file's directory and then each ancestor, first
    existing file wins. On the tracked examples the true states are: wheeled
    baseline `missing` (`runs/shared-wheeled-cad-v1/robot.rcad` is not on
    disk); full robot `no_recorded_hash` (the export has no `cad_sha256`).
    `current` and `stale` are covered only by the lib test with temp files,
    not by a native capture.
  - *Verified evidence (T9.3):* `.claude-pair/captures/T9-simrobot/`
    (`drive_t93.py`, `capture.json` ok=true, 31 assertions): robot-wheeled.png,
    robot-link-selected.png, robot-full.png (29 links), cad-link-wheeled.png,
    cad-link-full.png, bad-path.png. capture.json asserts against values the
    driver parses from each file: link count and names, the selected link's
    mass/com/inertia exactly, density from the materials map, the source block
    and notes verbatim, the CAD link status against an independent hashlib and
    path check, link provenance null versus the full robot's typed backlash
    label (`unmeasured`), the bad-path error naming the path, and no file under
    `examples/` changed (size and mtime of every file, sha256 of the opened
    files). Activations are REST `system_ui` (the click handler), not pointer
    gestures.
  - *Live run (batch simrobot-live-view; commits 37a48029, 12c8614e,
    39a07162; `crates/sim-spatial/src/robot_run.rs`):* a `robot-run` thread
    owns the shared `sim_runtime::physical::PhysicalRobot`, built from the
    already-loaded model with `sim_runtime::registry()` and
    `BuildOptions::default()` (the options `sim-app --scene cad` uses,
    `cad_app.rs:104`). The UI never builds or advances it; it only applies
    frames. Run / Pause / Step / Reset are header buttons, `system_ui`
    `run:start|pause|step|reset` and REST `robot_run {action}`, all through one
    handler. The robot advances in fixed 0.02 s sim-time chunks, paced at most
    to real time (lag beyond one chunk is dropped, not made up). Step advances
    exactly one chunk and is refused while running; Reset rebuilds, bumps the
    generation and leaves it paused at t = 0. Frames carry (generation, time,
    steps, poses, joint angles, targets); the UI drops frames from an older
    generation and shows the assembly pose until the rebuild publishes t = 0.
    `robot_state.run` reports phase (idle | building | running | paused |
    failed), time, steps, chunk_s, measured RTF (sim s over wall s, last ~1 s
    of running), generation, frame_generation and error. A build or advance
    error sets `failed` with the message (full text in the inspector, clipped
    in the header), keeps the last good frame, refuses Run/Step naming Reset,
    and does not panic. Nothing is written to the file.
  - *Verified evidence (T10.3):* `.claude-pair/captures/T10-robot-live/`
    (`drive.py`, `capture.json` ok=true, 21 named assertions; debug build).
    Wheeled baseline: time increasing while running (0.12 → 0.48 → 1.34 s),
    **measured RTF 0.70** in the recorded capture (0.72–0.73 in two earlier runs of the same driver; debug build, paced; release not measured),
    pause freezing time and steps across two polls, one step = +0.02 s (within
    1e-9) and +1 step, reset to t = 0, steps 0, generation 0 → 1 with
    frame_generation 1 and every link pose equal to the pre-run assembly pose
    (com, identity rotation; max error 0.0), and `robot_run {action:"jump"}`
    erroring with the name. Screenshots robot-running.png, robot-jog.png,
    robot-reset.png. 29-link full robot
    (`cad-profiles/robot-provisional.simrobot.json`): **it does not run.** The
    build fails in ~0.02 s with "CAD actuator profiles require explicit PWM
    control; catalog servo firmware cannot substitute for the declared
    fixed-PD controller" (`physical.rs:200`, because the default options have
    `driver_control` false). The capture asserts phase failed with that text,
    no panic, a responsive window (robot_state and system_ui answered in
    0.04 s), the static assembly pose still shown, and Run refused naming
    Reset (robot-full-running.png shows the failed state). `sim-app --scene
    cad` loads with the same default options, so by code reading it fails the
    same way; that was not captured.
  - *Limits:* the wheeled robot runs; 29-link full-robot exports need a
    driver_control / PWM build path, which neither viewer provides yet. There
    is no recording, saving or replay of robot-mode runs, no graphs (since added:
    T12 recordings, T13 graphs, §2g/§2h), and no
    file watching or auto-rebuild on CAD save (sim-app watches the file;
    reopen the viewer here). Controls were activated through REST
    `system_ui`, not pointer clicks. The wheeled robot's motion is small on
    screen (its wheels are axisymmetric), so the numbers in capture.json, not
    the images, prove the motion. RoboCAD still owns
    authoring and export; the shell cannot trigger CAD edits. The pose is the
    stored com with identity rotation; that it equals sim-app's t=0 pose is
    established by reading the code (`articulated.rs`, `physical.rs`), not by
    a side-by-side capture. REST motors, joints and transmissions are
    loader-parsed structs, so fields absent in the file show loader defaults
    (only `uncertainty`, `mass_sources`, `member_names` and motor notes are
    raw). Mouse-wheel inspector scrolling and pointer picking were not
    exercised natively. The link list clips rather than scrolls beyond about
    29 rows. Small selected links (e.g. a 5.7 g pulley) are hard to see
    highlighted in the full-robot view.
  - *Robot presets (batch native-preset-run, T11.1–T11.3):* robot mode also
    opens a preset declared in `web/viewer/presets.json`, either with
    `--robot-preset ID` or with REST `robot_presets` / `robot_preset {id}`.
    Only mode `embedded` is accepted. The run thread builds
    `EmbeddedEnvironment::new(scene, config, task, seed 0)` from the declared
    files unchanged, or `EmbeddedSession` when there is no task. Link meshes
    and the inspector come from `scene.robot` through the T9 loader; poses come
    from the session frame, mapped to links by name. Run/Pause/Step/Reset use the
    `--robot FILE` dispatch. A chunk is one action interval for an environment
    (0.02 s = 128 nominal steps on 400hz). `robot_state.preset` carries id,
    label, paths, readiness and evidence verbatim, seed, step_s, the chunk
    and completed/requested steps. The phase is `ended` at the horizon or at
    episode termination, with the message, and Run/Step/motion are then
    refused naming Reset. Nothing is written.
    - *Which presets run:* `robot_presets` lists 44 entries with `openable`
      (embedded, and every declared input exists). A build is attempted only on
      Run/Step. Verified natively to build and advance: `robot-measured-400hz`
      (29 links), `robot-crawl-startup` (T11.1), `pendulum-embedded`, and
      `robot-heading-student`, which builds and resolves its motion channels
      from `policy_contract.step_reference.config`. The other openable presets
      were not each built natively. Presets whose inputs live only under
      ignored `runs/` are listed with `missing` and are not openable.
    - *Verified evidence (T11.3):* `.claude-pair/captures/T11-preset-run/`
      (`drive.py`, `capture.json` ok=true, 36 of 36 assertions). **Release
      build** of af2d291d, launched as `--robot-preset robot-measured-400hz`.
      - Opens with 29 links, readiness, evidence and paths equal to presets.json,
        EmbeddedEnvironment, seed 0, step_s 0.00015625 and phase idle.
      - The first frame arrives 1.5 s after Run. Time then increases 0.02 →
        0.32 → 0.96 s, with 48 chunks = 6144 nominal steps.
      - **Measured RTF:** 0.11–0.12 over the first second. While walking under
        the forward request it falls from 0.071 to 0.033 (12 samples, 5 s
        apart; mostly 0.04–0.06). This matches the readiness text's "about
        0.05× real time on this Mac". Wall time is ~20× sim time, so a
        1-minute window covers about 3 s of sim time.
      - Pause freezes time, chunks and heartbeat across two polls. Step is
        exactly +0.02 s (within 1e-9), +1 chunk, +128 nominal steps, and
        heartbeat +1.
      - Reset returns to t = 0 with 0 chunks, generation 0 → 1, the heartbeat
        back to its initial 0 and the requests cleared.
      - `pendulum-embedded` (EmbeddedSession, 80 × 0.25 ms) reaches phase
        `ended`: "horizon reached at t = 0.020 s: 80 of 80 steps". Run and
        motion requests are then refused naming it and Reset.
      - The wheeled `--robot FILE` is unchanged: no preset block, time
        advancing at RTF 0.75 (release), a jog accepted, and `robot_input`
        refused ("`--robot FILE` has servo-target jog").
      - Images: `preset-open.png` (29-link assembly pose; label; readiness
        verbatim in the inspector; the header subtitle is no longer clipped
        under the run status), `preset-running.png`, `preset-forward.png` and
        `preset-ended.png`.
    - *Limits:*
      - The header truncates the readiness text with "…"; the inspector shows
        it in full.
      - The camera does not follow the robot, which walks toward the edge of
        the view.
      - `preset-ended.png` shows RTF 1.06 for a 0.02 s run. That value comes
        from a very short wall window, not from sustained faster-than-real-time
        stepping.
      - Before the first request, the one-line motion summary shows 0 while
        robot_state `requested` is null.
      - There is no recording or input replay of preset runs (the browser saves
        overrides and input replay) and no graphs (both since added: T12
        recordings, T13 graphs).
- **Remaining gaps:**
  1. no way to trigger CAD edits or exports from the shell;
  2. no RoboCAD REST reload round trip (edit → export → shell reload);
  3. `--robot FILE` still has no build path for actuator-profile (full-robot)
     exports, which need explicit PWM / driver control. The same robot runs
     natively through a preset whose scene declares that control
     (`robot-measured-400hz`). There is no recording, replay or file
     watching in robot mode.
- **Source owner:** CAD (`.rcad` → `simrobot` v3 export). Display models are
  presentation only.
- **Dependencies:** a CAD service client (§3). Simrobot stepping now runs on
  a worker in sim-spatial; sim-app's cad scene still steps on its UI thread
  (§4) and is unchanged.
- **Acceptance evidence:** the read-only half is met (above). Correction to the
  earlier expectation: current exports carry no measured/derived/estimated
  label on most values (mass, inertia, com, friction), so the inspector shows
  "no per-value provenance in export" plus the file's own notes rather than a
  label. Still open: after a RoboCAD REST `PATCH /nodes/{id}` material change
  and export, the shell reloads and the values change, with before and after
  screenshots.

### c. Schematic and spatial views
- **Entry today:** spatial is sim-spatial (all modes). The schematic is a
  separate `sim-viewer` process, spawned by `sim-spatial --schematic`
  (`main.rs:build_mode`, where `current_exe().with_file_name("sim-viewer")`)
  and linked through a file session (`sim_inspect::selection::native`,
  `linked.rs`); also `examples/systems-viewer/run-linked.sh` and `run-live.sh`.
- **Reusable layer:** `sim_inspect::{model, selection, spatial}`,
  `sim_diagram::{layout, projection}` (egui-bound drawing in `sim_diagram::lib`).
- **Shell status:** *partial: a read-only schematic pane is in the shell and
  verified natively; sim-viewer is still needed for layout editing and plots.*
  Build mode has a **Schematic** toolbar toggle (`system_ui` action
  `toggle_schematic`) that opens a pane beside the 3D view in the same window
  (`builder/schematic.rs`, commit 35e4bedd). The pane draws the current level
  with the shared `sim_diagram` layer: `projection::project` (child subsystems
  collapsed), then `layout::initial_state_cancellable` and `route_cancellable`,
  with `style::port_domain`/`net_domain` colours. There is no new layout
  algorithm. Layout runs on a worker thread, keyed by description id,
  revision and level. A newer key drops, and so cancels, the old job. A layout
  whose key is not current is dimmed under "Stale layout (not the current
  system)", with its boxes disabled. There is one selection,
  `Builder.selected`: each box is a `system_ui` control
  (`{"schematic_select": "<instance>"}`) sharing the `Select` dispatch arm,
  and the highlight comes from `Projection::selection_highlights`.
  `system_state.schematic` reports visible, laid_out/current keys, pending,
  stale, node/net/unrouted counts, layout_ms, ui_build_ms and per-node
  highlight. The layout is never written to the system file. `--schematic`
  still spawns sim-viewer unchanged.
- **Verified (T8.2, `$PAIR_CAPTURES/T8-schematic/`, `capture.json` ok, 22
  assertions):** driven through REST `system_ui` activations (the same
  handlers as a click, not pointer gestures) on copies of motor-driver-board
  and worm-drive:
  - Node count equals the top-level instance count from the copied file:
    board 8/8 (6 nets, 0 unrouted), winch 11/11 (0 unrouted).
  - The Outline's `select` control (regulator) gives `selected == ["regulator"]`
    and exactly one highlighted node (`select-3d.png`: the Outline row, 3D
    part, schematic box and inspector all show the regulator).
  - `schematic_select` battery gives `selected == ["battery"]`, and only that
    node is highlighted. The inspector shows the battery's parameters
    (`select-schematic.png`).
  - A `set_parameter` edit (revision 1→2) was observed as pending and stale
    (laid out rev 1, current rev 2, new description id) at 85 ms, then as
    current at rev 2 at 108 ms. `system_open` of the winch gave a new
    description id and the winch's instances (`schematic-winch.png`). The
    intermediate state of the open was not caught by polling (it completes
    within one poll). The recorded invariant is that no poll ever showed the
    board layout as current for the winch.
  - A scene-neutral edit (the same value again, revision 2→3) leaves
    `display::scene_hash` unchanged, so it triggers no compile. Before commit 8ce86fdb, the pane
    stayed "Stale… waiting for the compile" indefinitely
    (`noop-edit-stale.png`, kept as history). `Builder::reload` now carries the
    schematic's compiled source to the new revision when no compile is
    queued or running (`Schematic::advance_revision`), and the keyed worker
    lays it out again. The capture asserts stale=false and pending=false at
    revision 3 within 3 s (it was current at the first poll, 77 ms), shown in
    `noop-edit-current.png`. The stale→current step itself is covered by the
    lib test `schematic_lays_out_levels_shares_selection_and_goes_stale_on_edit`.
  - The edited copy differs from the example only in the edited value,
    `revision` and the store's existing `display_id` stamps: no layout keys.
  - Timing: board layout 14.6 ms on the worker, pane UI build 0.1 ms; winch
    10.4 ms / 0.08 ms.
- **Remaining limits:**
  - The pane is read-only: there is no node dragging, pinning or saved layout
    (`DiagramState` is not persisted).
  - There are no plots, connection graphs or analysis overlays in the
    schematic, and the pane has no pan or zoom (it fits the level).
  - At nested levels, nets that cross the subsystem boundary are dropped.
    Subsystem and long labels clip inside fixed boxes.
  - sim-viewer (`sim-spatial --schematic`) is still needed for layout
    editing, plots and the `--experiments` review.
- **Source owner:** Rust runtime (description and identities); display layout
  is presentation.
- **Dependencies (remaining):** layout editing and persistence of
  `DiagramState`, and plots in the pane, before sim-viewer becomes optional.
- **Acceptance evidence:** one window. `select` a component and take a
  screenshot showing it highlighted in both the 3D view and the schematic pane.
  `state` returns one selection. This is met for read-only viewing by T8.2
  (above).

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
- **Entry today:** sim-spatial build-mode toolbar Run/Resume, Pause, Step
  (enabled only while the run is not running) and Reset; REST
  `system_run {"action":"start"|"pause"|"step"|"reset"}`. Both call the same
  Builder methods (`run_start`, `run_pause`, `run_step`, `run_reset`). An
  unknown action is an error that names it. Also: the graph dock
  (`builder/graphs.rs`, up to 4 charts, REST `system_plot`), overlays
  (`display` `set_overlay`: power, forces, current, heat, trails), and the
  Detailed/Realtime toggle. `system_state` reports `realtime` and `live_run`
  (null without a run; otherwise time, phase, step, generation, interval,
  reset_pending, error, fidelity, edited). Legacy surfaces remain:
  sim-viewer `live_ui.rs` (worker-backed graphs) and browser presets with
  sliders.
- **Reusable layer:** `sim_runtime::system_session::SystemSession`,
  `sim_inspect::{live, plot, animation}`. Step and Reset are sent to the run
  thread and carried out only by `session.execute(Command::Step | Command::Reset)`.
  No physics is stepped in the UI.
- **Reset policy:** a run that reached at least 0.1 s is saved first, under the
  same rule as stopping a run. The status then reads "Reset to t = 0 (paused);
  kept the previous run as <id>." Reset clears the graph history, and
  `reset_pending` drops pre-reset samples until the thread has reset, so a
  later save holds only post-reset samples. After a reset, `edited` is false
  and the fidelity is kept.
- **Shell status:** *verified natively* (T6.2, commit bd0b7be4 binary) on a
  copy of the worm-drive winch. Evidence is in
  `.claude-pair/captures/T6-live-controls/` (`drive_live.py`, `capture.json`
  ok with 23 of 23 assertions, `running.png`, `stepped.png`, `reset.png`,
  `replayed.png`). The capture shows:
  - Running: `live_run.time` rose from 0.633 to 1.071 s between polls, and the
    pinned `drum.shaft.speed` chart window grew from 0.554 to 1.108 s.
  - Step while running: the Step control reports disabled, activating it is
    refused, and REST step returns "pause the run before stepping".
  - Pause: time held at 2.215 s (step 4430) across polls 0.5 s apart.
  - Step: step 4430 to 4431, and time rose by 0.0005 s, the configured
    interval, within 1e-9.
  - Reset: time 0, step 0, paused, generation 0 to 1, and the chart emptied.
    The 2.2155 s run was auto-saved and named in the status.
  - The post-reset 0.4305 s run was saved and replayed in the Studies tab:
    "Reproduced exactly · max rel diff 0 · 1696 samples · detailed". All of
    its sample times were ≤ its duration.
  - The Realtime toggle turned `realtime` from false to true, and the next
    run reported fidelity "realtime".
- **Pause status:** *verified natively* (T7.2, commit a7e743c6 binary).
  Pause sets the status bar to "Paused (Step advances one timestep; Run
  resumes)." and Resume sets "Running on the shared runtime (background
  thread, paced to real time at most)." again. Evidence is in
  `.claude-pair/captures/T7-live-truthfulness/` (`drive_pause.py`,
  `capture.json` ok with 8 of 8 assertions, `paused.png`, `resumed.png`).
  After Pause, the status bar, `live_run.phase` "paused" and the toolbar
  (Resume, Step enabled) agree. After Resume they read running, phase
  "running", and Pause with Step disabled. After a Step the status stays
  "Paused…".
- **Live-edit and grab swaps:** live edits and the grab/push load swap go
  through one fidelity-aware `Builder::hot_swap`. It applies the run's
  fidelity profile (a realtime run never receives the detailed document),
  sets `run.document` to exactly the document sent, and marks the run
  edited. If the profile fails, the run is stopped and kept with an
  explanation. A grab changes only the running model, never the file;
  release swaps back to the file value and the run stays edited. Saved runs
  record `run.document`, the model last swapped in, so a save or a Reset
  after a grab describes the model that was simulated.
- **Remaining limits:**
  - There is no shared time cursor or scrub on the graphs, and no replay
    cursor.
  - Every control was activated through REST `system_ui`, which uses the same
    handlers as a click. Pointer gestures (clicks, drags) were not exercised.
  - The grab/push load swap (Alt+left-drag) is a pointer gesture with no
    REST route, so it was not exercised natively. It is verified only by the
    unit test
    `builder::replay_tests::grab_swap_keeps_the_run_fidelity_and_marks_it_edited`.
  - The part registry, library and `runs/` come from the one resolved
    workspace root (§6), not the working directory, so the viewer can be
    launched from anywhere with an absolute FILE. Verified in T14.3 from a
    temporary directory outside the repository: the same palette (215
    distinct rows, identical per-category counts), the authored parts and
    the 18 library subsystems as a launch from the repository root
    (`.claude-pair/captures/T14-launch/capture.json`).
- **Source owner:** Rust runtime.
- **Acceptance evidence:** reproduce by running
  `python3 .claude-pair/captures/T6-live-controls/drive_live.py` and
  `python3 .claude-pair/captures/T7-live-truthfulness/drive_pause.py`. They
  need a current debug `sim-spatial` build. (Those drivers pass relative
  example paths, so they still run from the repository root; the viewer
  itself no longer needs to.)

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
- **Shell status:** *partial: native motion requests for embedded presets.*
  Robot mode on a preset (`--robot-preset ID`, §2b) sends motion requests
  through the preset's own Rust controller (T11.2, af2d291d; verified
  natively in T11.3). These requests are labelled "motion request through the
  preset's Rust controller (not joint control)".
  - *One handler:* physical W/A/S/D (press/release) and X (Stop), the inspector
    W/A/S/D/Stop buttons, `system_ui` `motion:w|a|s|d|stop` and REST
    `robot_input {channels:{name: value}}` or `{key}` all go through
    `RobotAction::Motion` to `Command::Motion` on the run thread. There the
    request is written into the held action for the next
    `EmbeddedEnvironment::step`, or passed through `set_inputs` for a session.
  - *Where the motion channels come from:* the session's
    `policy_contract.step_reference.config` first, then presets.json
    `motion_commands`. Key vectors come from `motion_key_vectors`, else from
    the channel bounds. This UI mapping was ported from `motion-commands.mjs`;
    no controller logic was ported.
  - *Semantics:*
    - Physical keys follow press/release, as in the browser.
    - A `system_ui` or REST key latches until Stop, another key or a channel
      request.
    - A value, key vector or summed key vector outside the session's typed
      bounds is **refused, naming the channel and bounds**. The browser clamps
      combined keys; the native viewer refuses them instead.
    - The declared heartbeat (`command.packet_sequence`) is incremented once
      per action packet (one chunk), as `nextMotionAction` does.
  - *Verified (T11.3, release build, `.claude-pair/captures/T11-preset-run/`)
    on `robot-measured-400hz`:*
    - Channels and bounds come from the session inputs: forward_speed
      [-0.8, 0.8] m/s, lateral_speed [0, 0] m/s, yaw_rate [-1, 1] rad/s.
    - `motion:w` requests [0.1, 0, 0], and after one packet the session holds
      forward_speed 0.1.
    - `motion:stop` requests [0, 0, 0], and all three held values are 0 after
      one packet.
    - REST `{"command.forward_speed": 5}` is refused with "requested value 5
      is outside its bounds [-0.8, 0.8] m/s; not clamped". The refusal is
      recorded in `motion.last_refusal`, and the held and requested values are
      unchanged.
    - An unknown channel is refused, listing the motion channels and their
      bounds.
    - REST `{key:"w"}` is accepted and requests [0.1, 0, 0].
    - The heartbeat equals the chunk count while running and increases by 1
      per Step.
  - **Measured body motion (from chassis poses in frames):**
    - *Under forward_speed 0.1:* over 3.32 s of sim time (61.6 s wall time),
      `Robot | Chassis and hip mounts` moved 0.504 m along the controller's
      travel heading (chassis yaw + `travel_heading_offset_rad` 0.785, a
      parameter of the scene's Rhai controller). That is an average of
      **0.152 m/s against the 0.1 m/s request**. It moved 0.018 m across that
      heading, with a yaw change of 0.023 rad and a height change of 4.5 mm.
    - *Controls:* at zero request before W, the chassis moved under 1 mm over
      1.16 s. In the 1.8 s after Stop it moved 0.028 m, which includes the
      controller's deceleration.
    - *What this does and does not establish:* the robot walks diagonally
      (+X+Y in world), as the heading offset declares, and faster than
      requested. This viewer does not establish why the achieved speed
      exceeds the request, nor whether it would converge over a longer
      window. A single ~3 s span is not a walking-speed qualification.
    - *Visibility:* `preset-forward.png` shows the robot displaced toward the
      edge of the view and the MOTION section with requested 0.1 / held 0.1
      and the label.
  - *Not exercised:*
    - Physical key presses and pointer clicks. All activations were REST
      `system_ui`, `robot_run` or `robot_input`; keys share the handler by
      code (`robot.rs motion_keys`).
    - A/D turning.
    - Heartbeat exhaustion.
  - *What still needs the browser:*
    - ~~Recording and input replay of a preset run~~: native since T12
      (§2h). Saved overrides of a teleop run, and scrubbing/timeline playback
      of recorded frames (`viewer.js` `timeline` → `replayAt`), are still
      browser only.
    - ~~Live graphs~~ of motion request vs measured chassis motion and of
      servo target vs measured joint angle: native since T13 (below).
      Observation panels beyond these two charts, arbitrary channel picking or
      pinning, and a time cursor/scrub on the charts are still browser only.
    - Hardware sync and mirroring (`hardware-sync.mjs`), which are never
      driven from here.
    - Calibration UI.
    - Gait playback.
    - Presets in `sim-web` modes other than `embedded`, and presets whose
      inputs live only under ignored `runs/`.
    - Realtime walking. Native detailed physics runs at about 0.04–0.07×
      real time, the same order as the readiness text. The browser's
      realtime presets use their own declared fidelity profiles, which were
      not compared here.
- **Robot-mode graphs (batch robot-mode-graphs, T13.1 fc1d9aa8; verified
  natively in T13.2).**
  - *What exists:* a graph dock under the robot viewport, toggled by the
    **Graphs (G)** header button, key G, or `system_ui` `graphs:toggle`. It
    has a fixed set of two charts, drawn with the same CPU raster as build
    mode's graph dock (`crate::chart`; build mode keeps its own observable
    selection, `builder/graphs.rs`):
    - `motion` (only when the built preset has motion channels): each motion
      channel's *request*, meaning the session input held in the frame
      (`Frame.inputs` at the channel's index; a replay holds the recorded
      action), against the chassis `|v_xy|` (m/s) and `ω_z` (rad/s). Both are
      taken from the session frame's published world-frame `velocity_m_s` /
      `angular_velocity_rad_s` and labelled "world frame, measured from frame
      poses" with the link named. Nothing is differentiated or integrated in
      the UI. Before a build it says "no built session yet".
    - `joints`: servo target vs measured angle for the selected link's servo
      joints (`--robot FILE`). It says "no servo joint on selected link" or
      "select a link". On presets it says "not in frame": session frames
      carry no named joint targets, so no trace is invented.
    - *Chassis rule:* the loaded model's root link as the shared articulation
      builds it. A ground root means pinned, so there are no measured traces.
      When several links qualify, the viewer refuses to plot rather than
      repeat the runtime's heaviest-link choice. On the full-robot presets
      this is `Robot | Chassis and hip mounts`.
    - *Sampling:* one sample per frame applied by `RunController::poll`
      whose generation equals the controller's. A same-time frame replaces
      the last sample, so a paused jog or request updates it. History is
      bounded to 20 s of sim time and 2000 points per trace. It clears on
      Reset, on replay start and on any generation change. The dock
      re-rasters at most 10 Hz while running. Each chart shows a LIVE or
      REPLAY badge with the generation.
  - *Verified in T13.2* (`.claude-pair/captures/T13-robot-graphs/`, `capture.json` ok=true, 26 assertions
    of which 23 gating, merged from per-phase receipts `capture-J/M/C.json`):
    - `joint-chart.png` (debug, wheeled `--robot FILE`, `left wheel`
      selected): 20 chunks at the file target 0, then `robot_jog left axle
      0.3` and 30 more. The chart shows the target step mid-window and the
      measured angle converging (0.2927 against 0.3, LIVE gen 0). Asserted:
      the traces' latest values equal `robot_state.jog` target/measured and
      `run.targets`/`joint_angles` at `latest_time == run.time`.
    - `motion-chart.png` (release, `robot-measured-400hz`): 3 steps at
      request 0, then `motion:w` and Run to t = 1.22 s. The
      `command.forward_speed` request steps 0 → 0.1, plotted against the
      measured traces. Asserted: chassis `|v_xy|` latest 0.18703 equals
      `hypot(vx, vy)` of the chassis pose `velocity_m_s` in the same paused
      response (≤ 1e-9, at the same frame time), `ω_z` latest 0.48727 equals
      `angular_velocity_rad_s[2]`, and the sources say "world frame". The
      joints chart says "not in frame". This is world-frame chassis speed,
      not a walking-speed qualification.
    - `replay-chart.png`: see §2h.
    - `reset-chart.png`: after Reset, LIVE gen 2. The history holds only the
      t = 0 rebuild frame (one sample per trace, 5 in total), not 0.
    - `build-graphs.png` (release, motor-driver-board, Graphs + Run through
      `system_ui`): build-mode charts still draw rising temperature traces
      through the shared raster. This is a visual check, not a pixel diff.
  - *Limits:* activations were REST/`system_ui` only; the G key and button
    were not pressed physically. There is no time cursor, scrub or seek, and
    no channel picker. Only the fixed charts exist. Controller-specific
    quantities (e.g. travel heading) are not plotted.
- *Before T11:* robot mode on `--robot FILE` offered **servo-target jogging
  only**, labelled "servo target (PD hold from the export), not
  walking-controller teleop". That jogging is unchanged:
  - *Controls:* −/+ buttons and `system_ui` `jog:<joint>:+|-` (0.05 rad,
    0.005 m prismatic) for the non-fixed joints touching the selected link;
    REST `robot_jog {joint, target|delta}` for any joint by its file name. One
    handler; the run thread calls `PhysicalRobot::set_target`.
  - *Validation:* a target outside the file's joint limits is refused with a
    message naming the joint and the limit, "not clamped". Unknown joints,
    passive joints and non-finite targets are also refused by name.
    Continuous joints show "no limit in file". A jog while paused sets the
    target, which takes effect when running or stepping; Reset restores the
    file's targets. `robot_state.jog` and the Joints section show the
    requested target and the target and measured angle from the latest frame.
  - *Verified (T10.3, `.claude-pair/captures/T10-robot-live/`):* on the
    wheeled baseline, six `jog:left axle:+` activations set the target from
    0.0 to 0.3 rad while paused. After ~1.5 s of running, the measured angle
    went from 0.0006 to 0.297 rad (|target − measured| 0.299 → 0.003;
    robot-jog.png). Both drive axles report "no limit in file". On the full
    robot, `robot_jog {"+X | Foot servo output", 1.0}` is refused naming the
    joint, `[-2.5743606466916362, 0.07853981633974483]` and "not clamped",
    and a within-limit −0.5 is accepted (queued; that robot cannot build, see
    §2b). A refusal in trajectory mode is covered only by code reading
    (`physical.rs:560`); no example uses that mode.
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
  after. Releasing produces a zero request. *Met natively in T11.3* for a
  system_ui/REST latched W on `robot-measured-400hz` (above). Physical
  press/release was not exercised.
- **Fixed-PD implementation identity (T11.0, 2026-09-30, user decision).**
  The only presets that declare motion commands, WASD key vectors and a
  heartbeat (`robot-measured-400hz`, `-reuse`) failed to build with "CAD
  fixed-PD implementation identity differs from the shared FPGA controller".
  The guard hashed all of `fixed_pd.rs`, and commit 7ff9b794 added the unrelated
  `step_differences` to that file.
  - *Rule:* the identity is the blake3 of `crates/sim-domain-control/src/fixed_pd/law.rs`,
    defined once as `sim_domain_control::fixed_pd::implementation_identity()`.
    That file holds `Gains` and its validation, the integer expression graph
    (`law()`, evaluation, RTL lowering), `step` (input bounds, `/256`
    rounding, saturation) and `verilog()`. The law depends only on std integer
    arithmetic; `Gains` parsing also depends on serde, which is not hashed.
    `step_differences` stays outside the law file. It is used only by
    `sampled_fixed_pd` with `multi_turn = 1`, which is labelled as not the
    deployed single-turn RTL; the embedded CAD fixed-PD path runs `step`.
  - *Users:* the `embedded.rs` guard (same error text), the
    `actuator_profiles` test, and the `controller_ir(_blake3)` fields of the
    records in `controller_refinement/{fpga,fpga_group,fpga_design,tracking,fpga_events}.rs`.
    Those fields name the same controller IR and are only length-checked on read.
    New records therefore carry the narrowed identity, and old records keep the
    whole-file hash they were produced with.
  - *Evidence:* the whole-file blake3 of `fixed_pd.rs` at ae6b0dd1 equals the
    stored `bc7964115b…633e`. Its controller region (the file before `#[cfg(test)]`)
    equals today's once `step_differences` is removed, and `law.rs` is that
    region verbatim except for three `pub(super)` qualifiers. The new
    identity is `f50894e2a6d8c86b64e08f65ae8739284ef486fc3c8a89eb6492e4b66e1eb26d`,
    pinned in `fixed_pd::tests::implementation_identity_covers_the_law_only`.
  - *Migrated:* only `examples/full-robot/measured-actuator-integration/browser-control-400hz/scene.json`,
    the run input of both 400hz presets. The file is byte-identical except for
    the three `families/hx30hm-fit-{10,11,12}-400hz/controller/implementation_blake3`
    values. `robot-measured-400hz` now builds in 2.4 s and advances at a
    debug RTF of 0.023–0.032 per 0.02 s chunk.
  - *Not migrated:*
    - historical records (capture, spec, qualification, comparison and search outputs);
    - the browser-control-400hz `controller-identity.json`/`overrides.json` receipts;
    - `realtime-control-2026-09-20/protocol.json`, and the build manifests that record
      the scene's old sha256 `225ad4f0…`;
    - the accepted actuator registry files `examples/actuators/hx30hm/accepted/hx30hm-{hip-measured,knee-measured,provisional}.json`,
      because changing them changes the accepted family content hashes;
    - `gait-generation/…/current-controller/profiles.json`, and gait-lab/gait-search study configs.

    Scenes produced from the registry, and `browser-control-400hz/prepare.mjs`, still
    use the old whole-file rule. prepare.mjs asserts the whole-file sha256
    and already fails. Promoting the new identity into the registry is a
    separate registry decision.

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
  **replay/verify** of a saved build-mode run (`run_history::replay` exists
  but sim-spatial never calls it) and no scrub cursor over a saved run.
  Robot-preset recording and replay are native since T12 (below).
- **Robot preset recordings (batch preset-run-recording, T12.1 bd5819d4,
  T12.2 9c5bfef3; verified natively in T12.3).**
  - *What exists:* in robot mode with a preset loaded, **Save recording**
    (inspector button, `system_ui` `recording:save`, REST
    `robot_save_recording {path?, note?}`; one `RobotAction::SaveRecording`)
    snapshots the shared recording on the run thread, and a writer thread
    writes it. The type is the one the browser worker's Download uses:
    `EmbeddedEnvironment::episode_recording()` (`sampled_environment_recording`)
    for a preset with a task, `EmbeddedSession::recording()` (`embedded_session`)
    without. **Replay** (inspector list of the 5 newest recordings,
    `system_ui` `replay:<file>`, REST `robot_replay {file}|{path}`; Cancel via
    `replay:cancel` / `{action:"cancel"}`; re-list via `replay:refresh` /
    `{action:"list"}`) parses the file as the loaded preset's kind, builds it
    only through the shared `EmbeddedEnvironment::prepare_replay` or
    `EmbeddedSession::prepare_replay(record, CaptureMode::Latest)` on the run
    thread, and advances it in paced chunks (one returned action per chunk
    for an environment) under a new generation, with completed/total and
    Cancel between chunks. `robot_state` carries `recording` (last_saved,
    pending, error), `recordings` (the listing) and `replay` (path, phase
    idle/replaying/cancelled/done/failed, completed/total, completed_steps,
    verdict, error, measured, replaced, wall_s).
  - *File rule:* `runs/robot-presets/<preset-id>/<UTC stamp>.json` under the
    resolved workspace root (§6), the same root the preset paths are resolved
    against, whatever the launch directory. Verified in T14.3: launched from
    a temporary directory with `--robot-presets <root>/web/viewer/presets.json`,
    Save wrote `<root>/runs/robot-presets/pendulum-environment/20260930T074259.860Z.json`
    and its sidecar, and nothing in the launch directory.
    `<stem>.json` is the shared recording exactly as `serde_json` writes it,
    with no wrapper, so it goes straight into `prepare_replay`. The viewer's
    metadata (preset id and paths, UTC time, seed, note, viewer version,
    runtime identity, replayability, final frame) goes into a
    `<stem>.meta.json` sidecar (schema `sim-spatial.robot-preset-recording-meta` v1).
    Both files are created with `create_new`: nothing is overwritten. An
    explicit path under `examples/`, `cad/` or `web/` is refused. A failed
    environment episode is saved as a diagnostic (`replayable=false`, with
    the runtime's reason), because the runtime's `prepare_replay` refuses it.
  - *What the runtime verifies:* environment: record version/kind, no
    recorded error, `completed_steps` within the horizon and a whole number
    of action intervals ("only valid completed-transition environment
    prefixes …"), task/scene/config fingerprints equal to the loaded
    environment ("replay must match loaded robot, controller and task"), and
    a valid action schedule. Then every returned action steps through
    `EmbeddedEnvironment::step`. Session: version, kind, step count and event
    order; re-applies each input event at its step and checks
    `replay_expected` (an unrecorded failure or a recorded failure that does
    not reproduce is an error). The viewer's `done` verdict says exactly
    that, plus "completed_steps N (recorded N); states are not compared by
    the runtime".
  - *What it does not verify:* neither runtime compares replayed states,
    observations, rewards or termination with the original run (they are
    not in the record). `EmbeddedSession::prepare_replay` rebuilds from the
    recording's own scene and does not compare it with the loaded preset,
    so for session presets the viewer applies its own labelled identity
    check (scene and config fingerprints, the same check sim-web makes). The
    per-link final-position difference against the sidecar's final frame is
    labelled "measured difference, not a pass criterion" and is never
    thresholded.
  - *State rules:* a replay needs a paused (not running) run. During it,
    Run, Pause, Step, motion, Save and a second replay are refused naming
    the replay; Cancel and Reset always work. After `done` the replayed
    simulation is the current paused run (as in the browser). After
    `cancelled` Run/Step/motion/Save are refused, naming the cancelled
    partial replay, until Reset or another replay. Save's "already exists"
    refusal is asynchronous (in `recording.error`); other refusals are
    returned by the REST call.
  - *Verified (T12.3, `.claude-pair/captures/T12-preset-recording/`,
    `drive.py`, `capture.json` ok=true, 43 assertions, 39 gating):*
    - pendulum-environment (debug build): Run to the horizon (phase ended, 1600 steps; an ended run is saveable), Save via
      `recording:save` → a file in `runs/robot-presets/pendulum-environment/`
      that parses as `sampled_environment_recording` v1, with
      completed_steps equal in the file, `last_saved` and `robot_state.run`,
      and a sidecar naming the preset. The file is listed in the recordings
      and appears as control `replay:<file>` (`saved.png`). Replay via
      `system_ui` → generation 0→1, frames stamped with it, phase done, 20/20
      actions, replayed completed_steps equal to the recording's, the runtime
      verdict above, and a measured difference of 0.0 m over 2 links
      (`replayed.png`). The replay takes about 0.4 s wall, too fast to
      screenshot or to refuse a request mid-replay, so `replaying.png` comes
      from 400hz. Save before any built session and Save to
      `examples/interactive/…` are refused by name; nothing is written there.
    - Mismatch: a one-action (80-step) pendulum-environment recording,
      opened by REST path while `robot-crawl-startup` is loaded (paused
      after one step), is refused verbatim with "EmbeddedEnvironment::prepare_replay
      refused it: replay must match loaded robot, controller and task". It
      leaves replaced=false, and time, chunks and completed_steps are
      unchanged (`mismatch.png`). The 1600-step recording on crawl is
      refused earlier by the runtime's prefix check (it exceeds crawl's
      830-step horizon). A pendulum-policy session recording opened in
      pendulum-embedded is refused by the viewer identity check (config
      differs), with replaced=false.
    - robot-measured-400hz (release build): three Steps, `motion:w`, then
      Run to 1.22 s sim (61 chunks, 7808 steps; 17.5 s wall), Pause, Save.
      The recording has 61 input events (the heartbeat changes every
      action). forward_speed is 0 for the first 3 and 0.1 from at_step 384.
      Replay: during it, motion, Save and Pause were refused naming the
      replay (`replaying.png`, 2/61). It finished done at 61/61, replayed
      completed_steps 7808 = original = recording, wall_s 21.7 (paced).
      Measured chassis position difference 0.0 m, max 0.0 m over 29 links (a
      measurement, not a pass) (`400hz-replayed.png`, robot moved from its
      start pose). A second replay cancelled via `replay:cancel` → cancelled
      at 4/61 and never done. Run was then refused ("the run is a cancelled
      partial replay …") (`cancelled.png`), and Reset returned the replay to idle.
    - A copy of the recording re-serialised as the browser's Download does
      (`JSON.stringify` of the parsed object: integral floats as integers),
      opened by REST path, replayed to done (non-gating; a simulation of a
      browser file, not an actual browser download).
  - *Limits:* activations were REST/`system_ui` only; pointer clicks and
    key presses were not exercised. The inspector lists only the 5 newest
    recordings (the rest are in `robot_state.recordings` and `system_ui`).
    There is no file-open dialog, so a file outside
    `runs/robot-presets/<id>/` is opened only by REST path. There is no scrubbing,
    timeline or frame playback of a recording. Cosmetic issues seen in the
    captures: after a replay ends, the inspector still shows the last
    "save refused … replay in progress" line (a stale `recording.error`
    until the next save), and the "Save recording" button label wraps.
  - *sim-runtime gaps (flagged, not changed):* `EmbeddedSession::prepare_replay`
    has no "matches the loaded session" check (the viewer and sim-web each
    do their own); neither runtime compares replayed states; a reproduced
    session failure returns `Err` with the recorded message, with no
    explicit "reproduced" signal; and there is no cheap replayability
    predicate (the viewer restates the rule in `REPLAYABLE_RULE`).
  - *Charts on replays (T13):* with the graph dock open, a replay's start
    clears the history, and the charts then plot the replayed frames under
    the replay's generation with an orange **REPLAY · gen N** badge
    (`robot_state.graphs.mode == "replay"`). The request trace shows the
    recorded action. Verified in T13.2 (`.claude-pair/captures/T13-robot-graphs/replay-chart.png`,
    400hz, taken at 20/61 actions). The first poll under the replay
    generation had 0 samples (the live history had 310) and the window was
    null. Samples grew from 115 at the screenshot to 310 at done. The graphs
    generation was 1, equal to the replay's and greater than the live 0.
    Reset returns to LIVE with a new generation. There is no scrub over the
    replayed history.
  - *Is the browser still needed for preset recordings?* Not to save or
    replay an embedded preset run. It is still needed for:
    1. Scrubbing or timeline playback of recorded frames and `recorded`-mode
       presets (`robot-lift-5mm`, `-3mm`).
    2. Presets that run only in the browser: sim-web modes other than
       `embedded` (`live`, `recorded`), and presets whose inputs live only
       under ignored `runs/` when those files are absent (§2g).
    3. Saved overrides.
    A browser-downloaded `<preset-id>.json` has the same format. It can be
    replayed natively by REST `robot_replay {path}`, or by copying it into
    `runs/robot-presets/<preset-id>/` and using Refresh list. The check
    above used a browser-style re-serialisation, not a file from a real
    browser session.
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
- **Shell status:** *partial*. System compare/sweep studies are present.
  Gait-lab reports are *present (read-only)* in the build-mode **Gait lab**
  tab. Gait playback, launching evaluations, the leaderboard and experiment
  review are absent from the shell.
- **Source owner:** example config (study configs, YAML gait files), with
  results in `runs/` or tracked results folders (preserved, never deleted).
- **One path:** `Builder::gait_reports_request(dir?)`
  (`crates/sim-spatial/src/builder/gait_lab.rs`, commit c4f25b0a) is called
  by the results path field (`gait_results_path`, Enter reads), Reload
  (`gait_reload`), Cancel (`cancel_gait_reports`, only while a read is
  pending), the first visit to the tab (which reads the tracked
  `examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25/results`,
  found above the system file or the working directory), and REST
  `system_gait_reports {"dir"?}` (listed in capabilities). A worker thread runs
  `sim_runtime::gait_lab::scan_results` (commit 8d0fc4cd), the typed reader
  that parses each `<root>/*/report.yaml` into `GaitReport`, `PoseReport` or
  `ManeuverReport` by its `kind` and attaches the latest `journal.jsonl` line
  whose `results` folder name matches. The UI thread only polls. A malformed
  report is a per-entry error naming its path. Results are in
  `system_state.gait_reports`, with full values. Row selection is
  `gait_report_select`. Nothing is written.
- **What it shows:** a fixed caveat; the root and whether `journal.jsonl`
  exists; a compact list (name, kind, status coloured, speed); and a detail for
  the selected entry. The detail holds the summary, the fidelity string
  verbatim, speed, distance and simulated time (null shown as `none` or `not
  simulated`, never 0), gates with value/limit/✓ ok, reasons, the top 5 joints
  as a percent of the motor limit, the source files, and the journal UTC time,
  `unix_s`, cached flag and line. Without a journal line it says "No journal
  entry (no timestamp recorded)". Maneuvers show checked time, travel, turn and
  overlaps. A bad path is an error naming it, and the last good listing stays
  visible, labelled "Still showing the last good read: <root>".
- **Evidence (T5.3):** `$PAIR_CAPTURES/T5-gait-reports/` (driver
  `drive_gait_reports.py`, receipt `capture.json` ok=true, 63 assertions):
  - `gait-reports.png`: the caveat, the root and 15 rows for `results/`.
  - `gait-detail.png`, `gait-detail-2.png`, `gait-detail-3.png`:
    6216-Bayesian-009, with the fast fidelity string, 0.199916 m/s, 3 gates ✓ ok,
    joints, and journal 2026-09-27 15:28:09 UTC, `unix_s` 1790522889, line 16.
  - `pose.png`, `pose-2.png`: stand-crouch, "No journal entry".
  - `screened-out.png`, `screened-out-2.png`: results-legscreen 5014-CmaEs-088,
    screened_out, speed and distance `none`, simulated `not simulated`,
    fidelity "detailed model", and its reason.
  - `maneuver-list.png`, `maneuver.png`: results-steer, the blocked
    forward-start-stop with checked 14 s, turned 0°, overlaps and reason.
  - `bad-path.png`: the error names the path, with results-steer still shown and
    labelled.

  In `capture.json`, the driver parses the files with PyYAML and asserts exact
  equality between them and `system_state.gait_reports`:
  - the entry counts per root (15, 2 and 7) equal the `*/report.yaml` on disk;
  - for the passed gait and the screened_out gait: status, summary, speed,
    distance and simulated time (null equals null), fidelity, reasons, every
    gate's value, limit and ok, and joint percentages;
  - for the maneuver: checked_s, turned_deg, travel, overlaps and reasons;
  - the journal `unix_s` and line equal the latest matching `journal.jsonl`
    line;
  - REST `system_gait_reports` returns the same entries as `system_state`.
- **Remaining limits:**
  - No gait playback on the robot scene, no launching or cancelling
    evaluations, and no leaderboard or experiment review.
  - Reports do not record the runtime fingerprint, so whether a report is still
    qualified against the current code is unknown. The tab says so. Follow-up:
    add `runtime_fingerprint` and `unix_s` to the gait, pose and maneuver
    reports.
  - Poses and maneuvers have no journal lines, so they have no timestamp.
  - The browser `lab_catalog`
    (`crates/sim-runtime/examples/serve_actuator_calibration.rs`) is still a
    separate untyped reader.
  - On-screen numbers are rounded for display, and exact equality is checked
    only in `system_state`.
  - Controls were activated with REST `system_ui`, which uses the same handlers
    as a click, and the sidebar was positioned with `system_ui scroll`. Pointer
    clicks and wheel gestures were not tested.
- **Dependencies (remaining):** gait playback on the robot scene (needs g's
  renderer), then launching evaluations as cancellable jobs (the gait lab
  already honours `OUT_DIR/STOP`). The runtime fingerprint means evaluations
  must be requalified after code edits (gait-lab README).

### j. Measured actuator models and calibration inspection
- **Entry today:** the build-mode **Actuators** sidebar tab in `sim-spatial`
  (read-only registry inspector, commit d4c14f9f, verified natively in T4.3),
  plus the `actuator_registry` example CLI (`hash|limits|apply|check`,
  `crates/sim-runtime/examples/actuator_registry.rs`). The same tab's
  **Measured evidence** view (commits 178c36f0, 4ee19e52; verified natively in
  T15.3) reviews an HX identification archive read-only. Evaluation,
  refinement, FPGA, power and motor-response review remain in
  `sim-viewer --experiments` (`experiments_ui/{refinement, fpga_ui, motor_response_ui, power_ui}.rs`).
  Browser calibration panel (`web/viewer/calibration-ui.mjs`, token gated,
  talks to `serve_actuator_calibration.rs`, which drives hardware and is out of
  scope). `web/motor-bench/`. The sim-spatial lesson bench page only asks a
  calibration server named by `SIM_BENCH_URL` (`lesson/extras.rs:193,520`).
- **Reusable layer:** `sim_runtime::{actuator_registry, part_fit, acquisition::calibration, controller_refinement::{calibration, calibration_data, evidence, fpga_review, motor_response}}`,
  `sim_domain_robot::actuator_profile`.
- **Shell status:** *present (read-only)* for the registry and consumer
  staleness; *present (read-only review only)* for measured identification
  archives (measured vs predicted, the archive's own pass/fail); *absent* for
  evaluation/re-simulation, candidate editing and refinement, FPGA, power and
  motor-response review, sweep.csv review, saving studies and HTML export.
- **One path:** `Builder::actuators_request(registry?, check?)`
  (`crates/sim-spatial/src/builder/actuators.rs`) is called by the registry
  path field (`actuator_registry_path`, Enter loads), the consumer path field
  (`actuator_consumer_path`, Enter checks that file), Reload/Check again
  (`actuator_reload`), Cancel (`cancel_actuators`), the first visit to the tab
  (which loads `examples/actuators/hx30hm/accepted/registry.json`), and REST
  `system_actuators {"registry"?, "check"?: [paths]}` (listed in
  capabilities). A worker thread runs `Registry::load` and
  `Registry::check_consumer`, the same library code the CLI `check` uses
  (`robot_pointers` discovery moved into the library, commit 2800bf6e); the UI
  thread only polls. Results are in `system_state.actuators`.
- **What it shows:** registry path and hash; per family the content hash
  (short on screen, full in state), acceptance text, description, limitations
  and every motor/driver/controller/envelope parameter with value, unit,
  provenance label (measured/derived/estimated, coloured), uncertainty
  (`± unknown` when null, never 0) and evidence key; the joint-role map; and
  per consumer file and model pointer: current/stale/invalid with motor, joint,
  family → accepted family and both hashes. Load errors (missing file, family
  content-hash mismatch) name the path and both hashes, and the last good load
  stays visible, labelled with its own path.
- **Evidence (T4.3):** `$PAIR_CAPTURES/T4-actuators/` — `registry.png` (knee
  family rows with provenance and `± unknown`), `registry-knee-header.png`,
  `registry-knee-more.png` (controller and envelope), `current-check.png`,
  `stale-check.png`, `bad-path.png`, `hash-mismatch.png`; `capture.json`
  (ok=true) records the CLI stdout/stderr and exit codes (current 0, stale 1)
  and asserts the CLI have/accepted hashes equal the native ones
  (d1bdbbb6…8f09 / 1bd83048…4f3b), and native family hashes, acceptance text
  and roles equal `registry.json`. Driver: `drive_actuators.py` there.
- **Remaining limits:** read-only — no apply, promote or re-accept in the
  shell (use the CLI and the promotion path); only the first mismatch per model
  is reported (the library returns the first); calibration review and FPGA
  review still live in `sim-viewer --experiments`; the hardware calibration
  server (`serve_actuator_calibration.rs`, browser calibration panel) stays
  external and is not migrated; joint limits for a checked consumer are not
  shown. Controls were activated with REST `system_ui` (the same handlers as a
  click) and the sidebar was positioned with `system_ui scroll` (commit
  12c744b3; OS-injected wheel events did not reach the window), so pointer
  clicks and wheel gestures are not captured. A long load error overflows the
  status bar line.

**Measured identification review (batch calibration-review, T15.1–T15.3).**
- **One path:** `Builder::calibration_request(path?)`
  (`crates/sim-spatial/src/builder/calibration.rs`) is called by the archive
  path field (Enter loads), the first visit to Actuators → **Measured
  evidence** (`system_ui` action `{"actuator_view":"evidence"}`, which loads
  `<workspace root>/examples/actuators/hx30hm/pwm-full-range-identification`),
  Reload and Cancel (`system_ui`), and REST `system_calibration_review
  {"path"?, "trial"?}` (listed in capabilities with an example). A worker
  thread runs the shared `sim_runtime::experiment_comparison::hx_archive::load(dir,
  workspace_root)`, the call sim-viewer makes; the UI thread only polls.
  Selection goes through `Builder::select_calibration_trial(id)` from a trial
  row button, `system_ui` `{"calibration_trial": id}` or REST `trial`.
  Filters are `system_ui` `{"calibration_split": all|train|held_out}`,
  `{"calibration_outcome": all|pass|fail}` and `{"calibration_page": N}`.
- **What it shows:** label, interpretation and split policy verbatim; the full
  observations.json/results.json blake3 hashes, verified inputs `N of M`
  against the workspace root and integrity issues; split × outcome counts
  taken only from each trial's `comparison.passes`; a paged trial list (20
  rows) with id, run, device, stage, kind, drive, duration, split (held-out
  rows purple and tagged "held-out (validation)"), voltage and temperature
  ranges, and RMSE/final error against their limits; for the selected trial,
  a chart on the shared `crate::chart` raster of `measured (hardware
  archive)` against `predicted (fitted model, archive)` in rad against time
  [s], with the split role (HELD-OUT (validation data) / TRAIN (fitting
  data)). `system_state.calibration_review` carries phase, path, requested,
  error, hashes, counts, filters, visible ids, the page, `selected` (metrics,
  limits, true sample counts, first/last samples) and the chart axes. Errors
  name the path and reason; the last good archive stays, labelled "Still
  showing the last good load: <path>".
- **Evidence (T15.3):** `.claude-pair/captures/T15-calibration-review/`
  (`drive.py`, `capture.json` ok=true, 53 assertions). Independent of REST:
  the hashes equal a pure-Python BLAKE3 of observations.json and results.json
  (0c60030e…d081, c6a9ac95…881c; the implementation is self-checked against
  the empty-input vector and results.json `input_blake3`); 216 trials equal
  observations.json; held-out 81/162 pass and train 38/54 equal a count from
  results.json `evaluations[].metrics` under the predeclared criteria (RMSE ≤ 3
  and |final| ≤ 5 counts). **81/162 agrees with the README claim.** The passing
  held-out trial `pwm-individual-full-range/4/1` (selected by `system_ui` row
  action) and failing `…/4/5` (REST `trial`) match results.json metrics
  (counts × 2π/4096 rad), the 3/5-count limits, 23 samples each, measured
  endpoints from observations.json and predicted endpoints from
  predictions.csv. A nonexistent path over REST failed naming the path, phase
  `failed`, `requested` = that path, while path, hashes and 216 trials stayed
  those of the tracked archive. PNGs: `review-loaded.png`,
  `heldout-filter.png`, `trial-pass.png`, `trial-fail.png`, `bad-path.png`.
- **Remaining limits:** read-only review of `hx_archive` directories only; a
  file (e.g. a study) or a `sweep.csv` directory is refused as not supported
  yet. Nothing is evaluated, re-simulated, refitted, saved or exported, and no
  value is promoted (use `sim-viewer --experiments` and the promotion path).
  The empty-trace "not in archive" rendering is verified by code reading only
  (no trial in the tracked archive has an empty trace). The load took ~14 ms,
  so the capture never observed the `loading` phase or exercised Cancel on a
  live load. Activations were REST `system_ui` and REST commands (the same
  handlers as a click) and the sidebar was positioned with `system_ui scroll`;
  no pointer gestures were captured. The chart's top value label can be
  overdrawn by a flat trace (seen on `trial-pass.png`).

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
- **sim-viewer:** stays for schematic layout editing, plots and the
  `--experiments` review until c, i and j reach parity. The shell now covers
  read-only review of measured identification archives (§2j), but
  `sim-viewer --experiments` is still needed for evaluation and
  re-simulation (`experiment_study::evaluate`), candidate model editing and
  refinement, FPGA, power and motor-response review, `sweep.csv` review,
  saving studies and HTML export. sim-viewer stays unchanged until those
  reach parity. Read-only schematic
  viewing with shared selection is now in the sim-spatial shell (§2c).

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
  note for realtime measurements (`ui.rs:645`). Gait-lab fidelity strings
  (Gait lab tab, §2i) and actuator measured/derived/estimated provenance
  (Actuators tab, §2j) are now shown natively, verbatim. Migrations must carry them through, not drop them.
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
1. **Done — open another system in-app** (a): Systems tab, `system_ui` and
   REST `system_open` share `Builder::open_system`, with draft preservation
   (commits 6dab907a, 2ceb4f92; verified natively in T3.2).
2. **Done — actuator registry inspector** (j): read-only Actuators tab and
   REST `system_actuators` over `Registry::load`/`check_consumer` on a worker
   (commits 2800bf6e, d4c14f9f, 12c744b3; verified natively in T4.3, see §2j).
3. **Done — gait-lab report browser** (i): read-only Gait lab tab and REST
   `system_gait_reports` over `sim_runtime::gait_lab::scan_results` on a
   worker (commits 8d0fc4cd, c4f25b0a; verified natively in T5.3, see §2i).
   Launching evaluations comes later.
4. **Partial — schematic pane in the shell** (c): a read-only pane over the
   shared `sim_diagram` layout, computed on a worker, sharing the one Builder
   selection (commit 35e4bedd; verified natively in T8.2, see §2c). The
   sim-viewer window is *not yet* optional: layout editing, saved layouts and
   plots still need it.
5. **Partial — simrobot live view on a worker** (b/g groundwork). Done: the
   read-only inspect slice, `sim-spatial --robot FILE` (worker load, posed
   collision meshes, one link selection, inspector, CAD link status; verified
   natively in T9.3). Also done: a worker-owned `PhysicalRobot` with Run /
   Pause / Step / Reset, generation-stamped frames and servo-target jogging
   with file-limit validation (commits 37a48029, 12c8614e, 39a07162; verified
   natively in T10.3 on the wheeled baseline, measured RTF ~0.7 in a debug
   build; see §2b and §2g). Not done: full-robot exports fail to build with
   the default options (they need a driver_control / PWM path, and sim-app
   has the same failure). Recording and replay, file watching, walking
   teleoperation and gait playback are also not done.
   **Update (batch native-preset-run, T11.1–T11.3):**
   - *Done:* robot presets from `web/viewer/presets.json` run natively on the
     shared `EmbeddedEnvironment`/`EmbeddedSession` (commits 4cb0e3e1,
     d3bb70a4, af2d291d). Typed motion requests go through the preset's Rust
     controller.
   - *Verified in T11.3 (release build):* the 29-link `robot-measured-400hz`
     walked 0.50 m in 3.3 s of sim time under a 0.1 m/s forward request, at a
     measured RTF of about 0.04–0.07. Also verified: pause, step, reset,
     Stop, the refusal, the `ended` phase and the unchanged `--robot FILE`
     (§2b, §2g).
   - *Still not done:* graphs, file watching, gait playback, and a PWM
     build path for `--robot FILE` full-robot exports.
   **Update (batch preset-run-recording, T12.1–T12.3):**
   - *Done:* Save and Replay of robot-preset runs through the shared
     recording types and `prepare_replay` on the run thread (commits
     bd5819d4, 9c5bfef3).
   - *Verified in T12.3:* save, list, replay, verdict, cancel and mismatch
     on pendulum-environment (debug) and a save/replay with `motion:w` on
     `robot-measured-400hz` (release), §2h.
   - *Still not done:* scrubbing/timeline playback of recordings, and
     replay of build-mode runs (`run_history::replay`, the original slice
     above).
   **Update (batch robot-mode-graphs, T13.1–T13.2):**
   - *Done:* a robot-mode graph dock (motion request vs world-frame chassis
     `|v_xy|`/`ω_z`, and servo target vs measured angle) drawn from
     generation-stamped frames with the raster shared with build mode
     (commit fc1d9aa8).
   - *Verified in T13.2:* live joint and motion charts, replay labelling and
     clearing, and Reset clearing (§2g, §2h).
   - *Still not done:* observation panels beyond the two charts, channel
     picking, a chart time cursor/scrub, file watching, gait playback, and a
     PWM build path for `--robot FILE` full-robot exports.
6. **Partial — L0: one launch path** (batch launch-cwd, T14.1–T14.3).
   - *Done:* `sim-spatial FILE` opens a `*.system.json`, a `*.simrobot.json`,
     a lessons directory or a place directory in the mode its type selects
     (`sim_spatial::launch::classify`, commit 3d092226). Repository data comes
     from one workspace root resolved by `sim_runtime::workspace` (commit
     ca305fbf) and reported over REST (§6).
   - *Verified in T14.3* (`.claude-pair/captures/T14-launch/`, capture.json
     ok=true): from a temporary directory outside the repository with
     absolute paths, the system opened with the same registry and library
     counts as a repository-root launch, the simrobot ran, a preset recording
     saved under `<root>/runs/robot-presets/`, capabilities, system_state and
     robot_state reported `found_by: opened_file`, and an unknown FILE failed
     naming the path and the accepted types. Nothing was written to the
     launch directory.
   - *Still not done:* switching modes inside one window (a FILE of another
     type needs a relaunch); presets are not positional (`--robot-preset ID`,
     which from outside a checkout needs `--robot-presets FILE`, `--workspace`
     or `SIM_WORKSPACE`); gait-lab output is not a FILE type; lessons and
     place launches from outside the repository were not captured (they use
     the same resolver and dispatch).
7. **Partial — measured identification review** (j; batch
   calibration-review, T15.1–T15.3).
   - *Done:* Actuators → Measured evidence loads an HX identification archive
     on a worker through the shared `hx_archive::load`, lists and filters its
     trials by split and outcome, and charts a selected trial's measured
     against predicted trace on the shared raster (commits 178c36f0,
     4ee19e52). REST `system_calibration_review`.
   - *Verified in T15.3* (`.claude-pair/captures/T15-calibration-review/`,
     capture.json ok=true): hashes, trial count, held-out 81/162 (agrees with
     the README) and pass/fail trial metrics and endpoints against independent
     reads of the archive files; bad path keeps the last good archive (§2j).
   - *Still not done:* everything else `sim-viewer --experiments` does
     (evaluation/re-simulation, candidate editing and refinement,
     FPGA/power/motor-response review, sweep.csv review, saving studies, HTML
     export).

## 6. Launch path

One command opens any supported file, from any directory:

```
sim-spatial FILE                  # e.g. target/debug/sim-spatial /abs/path/board.system.json
cargo run -p sim-spatial -- FILE
sim-spatial --validate-only FILE  # checks FILE without a window
```
FILE is dispatched by name or structure only (`sim_spatial::launch::classify`):
`*.system.json` → build mode, `*.simrobot.json` → robot mode, a directory
holding `place.json` → place mode, a directory with `<slug>/lesson.md`
entries → lessons mode. Anything else (another suffix, a missing path, a
directory with neither marker) exits nonzero naming the path and the four
accepted types. FILE conflicts with the mode flags (`--system`, `--robot`,
`--robot-preset`, `--lessons`, `--place`, `--description`, …), which keep
their meaning; `--lesson SLUG` works with a lessons FILE.

**Workspace root.** Repository data (the part registry `library/parts`, the
palette library `library/systems`, `library/models`, `web/viewer/presets.json`
and its preset inputs, `runs/` outputs such as `runs/robot-presets` and the
lesson sandbox) comes from one root resolved once per launch by
`sim_runtime::workspace` (rule in `workspace::RULE`), first match wins:
1. `--workspace DIR`, then `$SIM_WORKSPACE` (an override without the marker
   is an error naming it; it never falls through);
2. the nearest ancestor of the opened file (FILE, the mode flag's path, or
   an explicit `--robot-presets FILE`);
3. the nearest ancestor of the current directory.

The marker is a `Cargo.toml` with a `[workspace]` table next to a `library/`
directory. Narrower overrides still win over the root: `SIM_PARTS_DIR` for
authored parts, and explicit `--library`, `--models` and `--robot-presets`
paths, which keep normal cwd-relative meaning. `SIM_LESSON_SANDBOX` /
`SIM_LESSON_SETTINGS` still override the lesson sandbox.

**No root found.** Nothing silently falls back to the current directory. The
error lists every directory searched, the marker and how to override. Build
and lessons modes refuse to start unless `--library` is given; the registry
loads built-in components only, with a warning; `--robot FILE` still opens,
with `root: null`; a preset needs `--robot-presets FILE`, `--workspace` or
`SIM_WORKSPACE` (bare `--robot-preset ID` from outside a checkout fails with
the named error, captured in T14.3).

**Where REST reports it.** The `workspace` object `{root, found_by: override |
env | opened_file | cwd, from, error, rule}` is in `GET /v1/capabilities`,
build-mode `system_state` and `robot_state` (and `robot_presets`).
Verified in T14.3 from a temporary directory outside the repository
(`.claude-pair/captures/T14-launch/`: `system-outside.png`,
`robot-outside.png`, `preset-outside.png`, capture.json ok=true).

Build mode (the shell):

```
sim-spatial examples/systems-builder/motor-driver-board/board.system.json
```
Once it is running, open another system from the **Systems** sidebar tab (a
discovered row, or a path in the field and Enter), or send REST
`system_open {"path": …}`. There is no relaunch. Other modes today are `--lessons lessons`, `--place DIR`, and `--description/--spatial`.

Measured identification review (§2j), in build mode with any system: open the
**Actuators** sidebar tab and choose **Measured evidence** (or `system_ui`
`{"operation":"tab","tab":"actuators"}`, then activate the control with action
`{"actuator_view":"evidence"}`). The first visit loads
`examples/actuators/hx30hm/pwm-full-range-identification` from the workspace
root; type another archive directory in the path field and press Enter, or
send REST `system_calibration_review {"path": DIR, "trial": ID}` (both
optional). Read `system_state.calibration_review`.

Robot inspection and live run (a CAD-exported simrobot file; nothing is
written to it):

```
cargo run -p sim-spatial -- --robot examples/wheeled-robot/baseline/robot.simrobot.json
cargo run -p sim-spatial -- --validate-only --robot FILE.simrobot.json
```
REST `robot_state` returns the loaded values and the run and jog state.

Robot presets (the scene, controller config and task declared in
`web/viewer/presets.json`, run on the shared Rust environment/session). Inside
a checkout the root comes from the current directory; from elsewhere pass
`--robot-presets <root>/web/viewer/presets.json` (or `--workspace`). Use a
release build for the full robot, where debug reaches only about 0.03× real
time:

```
cargo run --release -p sim-spatial -- --robot-preset robot-measured-400hz
sim-spatial --robot-preset pendulum-environment --robot-presets /abs/repo/web/viewer/presets.json
```
Press Run (or Step) to build. Then use W/A/S/D and X (Stop), the inspector
buttons, `system_ui` `motion:*` or REST `robot_input` to send motion requests
through the preset's controller. REST `robot_presets` lists every preset and
whether it is openable. `robot_preset {id}` switches presets in the same
window.
`system_ui` lists the link, section, scroll, `run:*` and `jog:*` controls; REST
`robot_run` and `robot_jog` use the same handlers.

Preset recordings (§2h). After Run or Step, **Save recording** (or
`system_ui` `recording:save`, REST `robot_save_recording {"note": …}`)
writes `runs/robot-presets/<preset-id>/<UTC stamp>.json` (the shared
recording, as the browser's Download) plus `<stem>.meta.json` under the
resolved workspace root, whatever the launch directory. Nothing is
overwritten; `robot_state.recording.last_saved` gives the path and step
count. To list recordings, use the inspector's Replay section (the 5
newest), `robot_state.recordings`, or `robot_replay {"action":"list"}` /
`replay:refresh`. To replay, Pause, then press a **Replay** button,
`system_ui` `replay:<file>` or REST `robot_replay {"file": …}` (or
`{"path": …}` for any recording, such as a browser download). Poll
`robot_state.replay` for phase, completed/total and verdict. **Cancel
replay** (`replay:cancel`, `{"action":"cancel"}`) stops it between chunks;
Reset then starts a fresh run.

Robot-mode graphs (§2g). Press **Graphs (G)** in the header, press G, or send
`system_ui` `graphs:toggle`. The dock plots `motion` (preset request vs
world-frame chassis `|v_xy|`/`ω_z`, once a preset with motion channels is
built) and `joints` (target vs measured for the selected link's servo joints
on `--robot FILE`; "not in frame" on presets). REST `robot_state.graphs` gives
`visible`, `mode` (live|replay), `generation`, `frames_sampled`, `window`
[t0, t1] in sim s, `window_s`, `max_samples`, and
`charts[{id, title, absent_reason, traces[{name, source, unit, latest,
latest_time, samples, absent_reason}]}]`, plus the sampling and chassis
rules. For presets, `robot_state.run.poses[i]` also carries the frame's
`velocity_m_s` and `angular_velocity_rad_s`.

`sim-app --scene cad` is **no longer needed to run** a v3 simrobot file that
builds with the default options, such as the wheeled baseline (verified in
T10.3). It is not a way around the full-robot limitation: 29-link exports with
actuator profiles fail to build in sim-spatial, and sim-app uses the same
`BuildOptions::default()` (`cad_app.rs:104`), so it fails the same way
(established by code reading, not captured). Neither viewer can run them yet.
sim-app still offers what robot mode lacks: rebuilding on file save and
arrow-key jogging. `sim-app --scene cad` and `cad_app.rs` were not changed by
this batch; their only diff against the run baseline is the earlier accepted
1562f60c (shared collision triangulation).

Separate apps are still needed for the schematic and experiments
(`sim-viewer`; the shell only reviews identification archives, §3), phenomena and file-watching robot view (`sim-app`), CAD
(`cad/run.sh`), and calibration, hardware sync, scrubbing of recorded
frames, observation panels beyond the two robot-mode charts, and realtime walking
(browser, `web/README.md`; §2g lists what native preset runs lack).

After consolidation: a single `cargo run --release -p sim-spatial -- [FILE]`,
where FILE may be a system, simrobot, lesson directory or gait-lab output, opened
in one window with modes and tabs. The FILE launch exists now for systems,
simrobots, lessons and places (above); gait-lab output and switching modes
within one window do not. RoboCAD runs as a CAD service (its window is
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
