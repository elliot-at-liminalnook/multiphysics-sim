# CAD mode: parity ledger against RoboCAD

**Purpose.** This ledger has one row per user-facing feature of RoboCAD
(`cad/robocad`, Python/OCCT/Qt), so the native viewer's CAD mode can reach
exact parity in phases and each phase's claim can be checked row by row
(docs/architecture/native-viewer.md §9 "CAD in Rust"). It follows the
pattern of [hardware-parity.md](hardware-parity.md). Written 2026-09-30,
with the **cad-mode** epic, by reading the whole RoboCAD UI and service:

- `cad/robocad/ui/app.py` (1982 lines): the window, the command registry
  (`_build_commands`, app.py:270-431, 183 commands), menus, docks,
  autosave, close prompt
- `cad/robocad/ui/widgets.py` (1523): palette, numeric bar, outliner,
  properties, materials, radial menu and dialogs, robot panel
- `cad/robocad/ui/tools.py` (1361), `viewport.py` (1722), `comments.py`,
  `components.py`, `experiments.py`, `load_process.py`, `model_loading.py`,
  `motion_video.py`, `pose.py`, `references.py`, `saved_views.py`,
  `section_preview.py`, `system_graph.py`, `strings.py`, `keymap.json`
- `cad/robocad/commands.py` (`Ops`: every public method is reachable as
  `POST /ops/{name}`) and its mixins (`annotations.py`, `references.py`,
  `saved_views.py`, `components.py`)
- `cad/robocad/api.py`: the route table (`_route`, api.py:1098-1278) and
  the `Service` it calls
- `cad/README.md`, `cad/USER_GUIDE.md`, `cad/ARCHITECTURE.md`

**RoboCAD stays the reference.** It remains available, and every CAD edit
still runs through its command layer, until the §9 parity harness passes on
the user's real models and the user agrees to retire the Python path.

**Columns.** *RoboCAD source* is `file:line`, relative to
`cad/robocad/` (`ui/…` for the UI, `api.py`, `commands.py`, …), or
`keymap.json:line`. *REST route* is the route in `api.py` that backs the
feature. *Native target* is the symbol that implements it in this epic
(`cad::…` is `crates/sim-spatial/src/cad/…`; `CadClient` is
`sim_runtime::cad_client::CadClient`), or the later epic that owns it.
The cad-views-export and cad-physical-inspect rows name a `file:function`
instead: `camera/…` and `cad/…` are relative to `crates/sim-spatial/src/`, `main.rs` is
`crates/sim-spatial/src/main.rs`, and `crates/sim-runtime/src/cad_client/…`
is written in full; a backticked symbol after a path is in that file.

**REST route values.** Three forms are used:

- a route, e.g. `PATCH /nodes/{id}`, `POST /ops/fillet`, or
  `POST /commands/{id}`. `/commands` works only when RoboCAD's own window
  serves the API (a headless service answers `{}` and 409 "no GUI").
- **none: needs a Python route**: nothing in `api.py` reaches the feature
  (or reaches it only through a GUI command, which opens RoboCAD's dialogs
  or uses its clipboard or viewport). These rows are flagged and listed at
  the end: 10 rows, 10 distinct gaps (15 and 15 until the
  cad-physical-inspect epic added `GET /results/nodes` and
  `GET /physical?planar=1`; 17 and 17 until the
  cad-views-export epic added `POST /save/thumbnail` and
  `GET /import/units`; 24 rows and 22 gaps until the cad-modify epic added
  routes for copy and paste with placement, control points, the curvature
  comb and the continuity check).
- `n/a (display)`: purely the viewer's own presentation (camera, cursor,
  theme). No RoboCAD state is involved and no route is needed.

**Status legend.**

- `done-by-reading`: the cad-mode, cad-select-transform, cad-modify,
  cad-sketch, cad-views-export or cad-physical-inspect epic implements it
  (native-viewer.md, CAD mode section; "CAD selection and transform"; "CAD
  modify"; "CAD sketch"; "Shared camera and CAD views"; "CAD physical
  properties"). It is built and tested in that epic's
  verification pass (still to come for cad-views-export and
  cad-physical-inspect) and moves to `done` only when the user's checklist
  ([cad-checklist.md](cad-checklist.md)) passes. Nothing is `done` yet: no
  epic's checklist has been signed off (cad-sketch was verified at
  cc7ac194), and neither cad-views-export nor cad-physical-inspect has
  been through its verification pass.
- `later-epic: <name>`: owned by a later CAD epic (see "Epics" below).
  No row of an epic that has been worked is left open: every
  `later-epic` row names an epic not yet started.
- `deliberately different: <reason>`: the native viewer differs on purpose,
  for the reason given.

**Epics.** This epic is `cad-mode`. It covers: opening a `.rcad` (a
self-started headless service) or attaching to a running RoboCAD; the model
tree; tessellated bodies with picking; body selection synced with
`/selection`; the node inspector; editing visible, locked, disabled,
material and name; delete; undo and redo with history labels; save; GUI
registry commands; Ops through REST; refresh; fit view; the keys
Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z, Delete/Backspace, Home and Ctrl/Cmd+S; and
service status, dirty state, the autosave indicator and stale/lost states.
The later epics are `cad-select-transform` and
`cad-modify` (the planned `cad-tools`, split 2026-10-01), `cad-sketch`, `cad-views-export`,
`cad-physical-inspect`, `cad-print`, `cad-experiments-motion` and
`cad-organize`. `cad-organize` was planned as `cad-annotations`;
it is renamed because it also takes the outliner's organization features, which no other epic
fits. The `cad-select-transform` epic (2026-10-01) has mapped its rows to
native code (sub-body selection, the transform tools, push/pull, the
numeric bar, live dimensions, snapping and measure); none stays open
(see "Counts"). The `cad-modify` epic (2026-10-01) is done by reading: its
116 rows map to native code (the op catalogue `cad::ops`, the command
surfaces `cad::surfaces`, the keys `cad::keys`, the analysis overlays
`cad::analysis_overlay` and the inspector's pivot and transform editors
`cad::inspector::editors`), 77 `done-by-reading` and 39 `deliberately
different`, each with its reason; none stays open. Its five new
routes in RoboCAD's `api.py` (`POST /clipboard/copy`, `POST
/clipboard/paste`, `GET /nodes/{id}/control_points|curvature_comb|continuity`)
closed five flagged gaps (seven rows). The `cad-sketch` epic (2026-10-01)
is done by reading: its 60 rows map to native code (the active plane and
the plane tools, the 13 sketch tools, the sketch edits and `cad_sketch`,
extrude, revolve, sweep, pipe, loft and fill: `cad::sketch` and the
catalogue's `cad::ops::catalogue::{plane, sketch, solid}`), 35
`done-by-reading` and 25 `deliberately different`, each with its reason;
none stays open. It also re-assessed the 15 cad-modify and
cad-select-transform rows that waited for cad-sketch (the active plane,
sketch endpoints, sketch and plane nodes): 6 became
`done-by-reading` (cylinder, sphere, mirror, mirror as live instance,
array, draft) and 9 stay `deliberately different` for reasons that remain
(the box tools' `Ops.box`/`Ops.box_three_point`, refusals by name, centre
snaps, the toolbar's and context menu's additions, `POST /nodes` for
solids). Its one Python change fixes `Service.edit_sketch`'s curve
indices (see "Edit a sketch with a call list"); it closed no flagged gap.
The `cad-views-export` epic (2026-10-01) is done by reading: its 112
rows map to native code: the shared camera (`camera/`: orbit, pan, zoom
to the cursor, RoboCAD's Shift+middle and Alt+left orbit, Alt snap and
arrow keys, presets, ortho and field of view, trackball, focus), the display state
(`cad/display/`: display modes and edges, grid, build plate and
overhangs, view cube, high contrast, curve nodes, the section preview,
its offset field and the exact section), saved views (`cad/views/`),
isolate, hide and show all (`cad/ops/catalogue/view.rs`), the
tessellation tolerance and file workflows (`cad/files/`: new, open, save
and save as with the thumbnail, import with RoboCAD's unit guess, every
export format, the drawing, render), 79 `done-by-reading` and 33
`deliberately different`, each with its reason; none stays open (the six
rows left open at 9b1e5eec were closed by f15766ea: 4 became
`done-by-reading`, the save thumbnail, Shift+middle and Alt+left orbit,
the Alt snap and the arrow keys; 2 `deliberately different`, curve nodes
drawn but not picked, and the section offset typed in the toolbar with
no plane drag, R or Tab). It re-assessed the rows of earlier epics that waited for it: the
per-node tessellation tolerance (cad-mode) and the view radial (cad-modify)
became `done-by-reading`; the right-click menu, the toolbar, the Save As
row and the navigation footer keep their status with updated text. Its
three new routes in `api.py` (`POST /new`, `POST /save/thumbnail`, `GET
/import/units`, pytests `cad/tests/test_api_files.py`) closed two flagged
gaps (the save thumbnail and the mesh-unit guess); New needed no gap.
The `cad-physical-inspect` epic (2026-10-01) is done by reading: its 82
rows map to native code: the materials panel and its dialogs
(`cad/materials/`), the inspector's physical rows, colour, joint editor,
joint physics overrides, results line and exact measurement
(`cad/inspector/`), the Robot panel, its tools, dialogs, motor library and
glyphs (`cad/robot/`, the catalogue's `cad/ops/catalogue/robot.rs`), and
results, the stress overlay, physical export and the live link
(`cad/results/`), with the client's `robot.rs`, `robot_ops.rs` and
`physical.rs`; 64 `done-by-reading` and 18 `deliberately different`, each
with its reason; none stays open. Its two new routes in `api.py`
(`GET /results/nodes`, `Service.results_nodes`, and `GET
/physical?planar=1`; pytests `cad/tests/test_api_physical_routes.py`)
closed five flagged rows and five gaps (the inspector's "Results" line,
the stress overlay, the print overlay's data, the Robot panel's margins
and the planar export); the print overlay row stays cad-print's. It also
stamped box-select, Alt+click menu and edges → faces items with their
topology revision (rows "Box select", "Alt+click" and "Selection: edges →
bounding faces", statuses unchanged).

**Every Ops method is already reachable natively, but only by REST.** The
`cad_op` REST command (`CadAction::CadOp` → `CadClient::op`) can call any
`POST /ops/{name}` in this epic. "Later epic" rows mean the *viewer UI* for
the feature is later, not that it cannot be reached at all.

## File and document

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Open a `.rcad` by path ("Open…", `file.open`; RoboCAD opens it through the nonmodal loader in a new window) | ui/app.py:284, ui/app.py:1326-1335, ui/model_loading.py:279 | none for a new service: the viewer starts `python -m robocad.api PATH --port N` (api.py:1390-1405). `POST /open` (api.py:966) is GUI-only and opens another RoboCAD window | `cad::actions::CadAction::CadOpen { path }` → `cad::sync` connect job (`sim_runtime::cad_client::service::service_command`, `wait_until_live`, `jobs::ChildProcess`) | done-by-reading |
| The open-file dialog "Open" filtered to "robocad (*.rcad)" | ui/app.py:1332-1335 | n/a (display) | File > Open… and Ctrl+O → `cad/files/mod.rs:open_form` → `cad/files/form.rs:FileForm` (a modal kit path form pre-filled with the document's directory, listing its `.rcad` files from a `Pool::Io` job, `cad/files/jobs.rs:list`) | deliberately different: a kit path form instead of the system's file dialog: rfd is not in the workspace, and a native dialog's macOS main-thread requirement cannot be verified without building and running (`cad/files/mod.rs` module doc) |
| Open from the command line (`.rcad` argument; other files are imported after load) | ui/app.py:1962-1978, ui/load_process.py:44-46 | `POST /import` for the extra files | `main.rs:take_file` → `CadTarget::File` (one `.rcad` FILE argument; or `--cad-url`) | deliberately different: the viewer takes one file argument; other files are imported after it opens, with File > Import… (`cad/files/mod.rs:file`, `FileOp::Import`) |
| Attach to a running RoboCAD (each window serves REST from 8420 up; `ROBOCAD_API_PORT`) | ui/app.py:1783-1795 | `GET /` (api.py:387) | `CadAction::CadOpen { url }` (loopback only; never stopped) | done-by-reading |
| New ("New", `file.new`: an empty document in a new window) | ui/app.py:283 | `POST /new {"path"}` (api.py `Service.new_file`, added with cad-views-export: creates the `.rcad` exclusively, `open(path, "xb")`, then saves an empty document over it, so an existing file is never replaced (409)) | File > New and Ctrl+N → `cad/files/mod.rs:file` (`FileOp::New`: the path form; `cad_open`'s rule, `CadDocument::switch_blockers`, checked before RoboCAD writes the file; then `crates/sim-runtime/src/cad_client/files.rs:CadClient::new_file` on a `Pool::Dedicated` job, then `CadOpen` of the new file, `jobs::Then::Open`: `cad/files/jobs.rs:receive` for a click, `cad/files/jobs.rs:wait` → `open_created` for a REST caller, whose answer is the open's outcome) | deliberately different: New names its file first and opens it in this window under `cad_open`'s rule (a self-started service with unsaved edits is refused; an attached RoboCAD keeps its edits): CAD mode shows one document, served by a headless service from its file; RoboCAD opens an untitled window, so its New never loses edits |
| Save ("Save", `file.save`; with no path it falls through to Save As) | ui/app.py:285, ui/app.py:1337-1342 | `POST /save` (api.py `Service.save`; answers 400 "no path" for a never-saved document); since f15766ea the viewer sends `POST /save/thumbnail` instead, as RoboCAD's desktop Save writes the thumbnail too | `CadAction::CadSave { path: None }` (Ctrl/Cmd+S in `cad::keys`, `cad:save`, REST `cad_save`) → `cad/actions.rs` CadSave arm → `cad/files/mod.rs:save` → `crates/sim-runtime/src/cad_client/files.rs:CadClient::save_with_thumbnail` | done-by-reading |
| Save writes `thumbnail.png` from the viewport into the `.rcad` | ui/app.py:1340, ui/app.py:1352-1362 | `POST /save/thumbnail {"path"?}` (api.py `Service.save_with_thumbnail`, added with cad-views-export: the window's thumbnail, or headless the snapshot renderer at 256 × 192) | every save: Save (Ctrl/Cmd+S, `cad:save`, REST `cad_save`: `CadAction::CadSave`, `cad/actions.rs` CadSave arm) and Save As… (`cad/files/mod.rs:file`, `FileOp::SaveAs`) both go through `cad/files/mod.rs:save` → `crates/sim-runtime/src/cad_client/files.rs:CadClient::save_with_thumbnail`; the status line says whether RoboCAD could draw the thumbnail | done-by-reading |
| Save As… ("Save As…", `file.save_as`; appends `.rcad`) | ui/app.py:286, ui/app.py:1344-1350 | `POST /save {"path"}`; the viewer sends `POST /save/thumbnail {"path"}` (since cad-views-export; REST `cad_save {path}` too since f15766ea) | File > Save As… and Ctrl+Shift+S open the path form (`cad/files/mod.rs:file`, `FileOp::SaveAs`: absolute, `~/` expanded, appends `.rcad`) → `cad/files/mod.rs:save` → `CadClient::save_with_thumbnail`; REST `cad_save {path}` is `CadAction::CadSave { path: Some }` → `cad/files/mod.rs:absolute` (absolute, `~/` expanded; no `.rcad` appended) → the same `save`; a self-started document follows the saved file (`Edit::retarget`, set on the edit that save started) | done-by-reading |
| Quit ("Quit", `file.quit` → window close) | ui/app.py:290 | n/a (display) | the viewer's own window | deliberately different: one native app with modes; leaving CAD mode is the app's mode switch, not a RoboCAD window close |
| Window title "robocad — name *" (dirty marker) | ui/app.py:126-127, ui/app.py:1820 | `GET /` (`path`, `dirty`) | `cad::panel` header (path and dirty from `Health`) | done-by-reading |
| Several documents at once (`WINDOWS`; New and Open add windows) | ui/app.py:65, ui/app.py:121 | n/a (display) | one RoboCAD document per CAD mode (`CadDocument`) | deliberately different: CAD mode shows one document; another RoboCAD window can still be attached by URL |
| "REST API: show address" (`api.address`, message box "REST API") | ui/app.py:426, ui/app.py:1797-1798 | n/a (display) | `cad::panel` header shows the service URL (`CadClient::url`) | deliberately different: the header always shows the service URL, so no dialog is needed |
| Status bar messages; errors as "⚠ text" for 8 s with a beep | ui/app.py:1863-1873 | the error text of each route | `cad::panel` status line (`CadDocument.status`; RoboCAD's error verbatim via `CadError` Display) | done-by-reading |
| A busy command shows "label…" and the wait cursor | ui/app.py:253-268 | n/a (display) | `cad::panel` shows the edit in flight (`Edit.label`) | done-by-reading |
| Permanent readout label (snap kind and coordinates) | ui/app.py:176-177, ui/app.py:557-560, ui/app.py:1867-1868 | n/a (display) | `cad::numeric` body: the tool's live readout (`ToolState.readout`) and the snap readout (`cad::snap::Snap::readout`, "vertex  (x, y, z)") | done-by-reading |
| Mode label "Tool · Selection mode" | ui/app.py:448-450, ui/app.py:525-526 | n/a (display) | `cad::transform::mode_label` ("Move  ·  Body") in the `cad::numeric` head | done-by-reading |
| "User guide" (`help.guide`: a message box with the path of USER_GUIDE.md) | ui/app.py:430, ui/app.py:1875-1877 | n/a (display) | docs/architecture/native-viewer.md and this ledger | deliberately different: RoboCAD shows only a path; the viewer's docs live in the repository |
| "Open diagnostics folder" (`help.logs`) | ui/app.py:431, ui/app.py:1879-1883 | none needed | the self-started service's stderr log (`service::log_path`), whose tail is quoted in connection errors | deliberately different: the viewer reports the log of the service it started; RoboCAD's own session logs stay in RoboCAD |
| Dark stylesheet | ui/app.py:44-57 | n/a (display) | the UI kit's theme | deliberately different: the native UI kit owns the look (native-viewer.md "UI kit") |
| High-Contrast Theme (`view.high_contrast`, kept in QSettings) | ui/app.py:317, ui/app.py:1085-1093, ui/viewport.py:287 | n/a (display) | `view.high_contrast` (View menu), the toolbar's Contrast chip (`cad/display/ui.rs:toolbar`) → `cad/display/mod.rs:apply_display` (toggle); drawn by `cad/display/draw.rs:lights` (background 0.98, 0.98, 0.99), `draw_grid` and `draw_edges` (black edges) | deliberately different: only the 3D view changes (background, grid, edges): the UI kit's colour tokens are constants, so RoboCAD's stylesheet swap has no counterpart; not kept between launches (RoboCAD keeps it in QSettings) |
| 3Dconnexion SpaceMouse (buttons mapped in `~/.robocad/spacemouse.json`) | ui/app.py:1885-1918 | n/a (display) | none | deliberately different: RoboCAD polls `pyspacemouse` when it is installed (ui/app.py:1885-1918); Bevy 0.19.1 has no 6-DoF input (bevy_input reads keyboards, mice, touch and gamepads) and no HID crate is in the workspace, so it needs a new dependency; the same motions are the shared camera's actions (`camera_orbit`, `camera_pan`, `camera_zoom`) |
| Drop image files on the viewport: they become references | ui/app.py:243-245, ui/app.py:1801-1803 | `POST /ops/import_references` | cad-organize epic | later-epic: cad-organize |
| "Preferences…" (`edit.preferences`): grid step | ui/app.py:300, ui/app.py:1491-1494 | n/a (display) | none: the grid keeps RoboCAD's default step (`cad/display/mod.rs:GRID_STEP_MM`, 10 mm) | deliberately different: RoboCAD's Preferences dialog sets its desktop autosave timer, which a headless service does not have, and its viewport grid step; the native grid keeps the 10 mm default (`cad/surfaces/registry.rs` `PREFERENCES`) |
| Shutdown on close: cancel component jobs, picks and measurements; stop autosave, the export child, pose, bridge, API and sim link | ui/app.py:1935-1959 | n/a | the self-started service is stopped when the document closes (`jobs::ChildProcess::stop`; an attached RoboCAD is never stopped) | done-by-reading |

## Autosave and unsaved-edit rules

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Background autosave every N s (default 120 s, from QSettings) while dirty; the archive is captured on the Qt thread and written by a worker | ui/app.py:103-112, ui/app.py:129-161 | `GET /autosave` (api.py:390; GUI only, 409 headless) | `cad::panel` autosave indicator from `CadClient::autosave` (GUI only) | done-by-reading |
| The autosave path `<name>.autosave.rcad` | document.py:640-643 | `GET /autosave` (`path`) | `cad::panel` (GUI only) | done-by-reading |
| "Autosaved to …" status | ui/app.py:1858-1859 | `GET /autosave` (`saved_revision`) | `cad::panel` autosave indicator | done-by-reading |
| "Autosave failed: …" status | ui/app.py:145-146, ui/app.py:160-161 | none: needs a Python route (`Service.autosave`, api.py:390-399, does not report a failed write) | attached to RoboCAD's window: the autosave indicator from `GET /autosave` (`cad::panel`) | deliberately different: a headless service never autosaves (only RoboCAD's window starts autosave, ui/app.py:112), so there is no failure to report; attached to a window, RoboCAD shows its own failure status |
| Start a recovery save now | api.py:390-393 | `POST /autosave` (GUI only) | none | deliberately different: `POST /autosave` is GUI-only (409 headless) and the viewer never saves for the user; Save and Save As write the file |
| Preferences: autosave interval ("Autosave interval (seconds):") | ui/app.py:300, ui/app.py:1486-1490 | none: needs a Python route (GUI: `POST /commands/edit.preferences` opens the Qt dialogs) | none | deliberately different: the interval sets RoboCAD's desktop autosave timer, which a headless service does not have (`cad/surfaces/registry.rs` `PREFERENCES`) |
| A headless service never autosaves: `Document.start_autosave` is called only by the window (ui/app.py:112) | document.py:645-660, api.py:1390-1405 | none | CAD mode must not lose a self-started document's edits; see the next three rows | done-by-reading |
| Replacing a self-started document that has unsaved edits | n/a (RoboCAD opens new windows instead) | `GET /` (`dirty`) | `CadAction::CadOpen` is refused, naming the reason ("…has unsaved edits in the RoboCAD service this window started…: save first") | done-by-reading |
| Leaving CAD mode while an edit is in flight | n/a | n/a | refused, naming the edit (`cad::document::Edit`, `app::switch::leaving_blockers`) | done-by-reading |
| Closing with unsaved changes: "Unsaved changes" / "Save before closing?" (Save, Discard, Cancel); a failed or cancelled save keeps the window | ui/app.py:1920-1934 | `GET /` (`dirty`), `POST /save` (the viewer: `POST /save/thumbnail`) | `CadDocument::switch_blockers` (leaving CAD mode or `cad_open` is refused while a self-started service has unsaved edits, or while its saved state can't be confirmed); `CadDocument::release_child` and `sync::on_exit` (closing the window detaches a dirty self-started service, leaves it running and logs its URL; a clean one is stopped); an attached RoboCAD is never stopped and keeps its edits | deliberately different: the viewer never saves for the user, so there is no Save/Discard prompt; it refuses to leave, or keeps the service running, instead of losing edits |
| The viewer never writes the `.rcad` itself | n/a | `POST /save/thumbnail` (and `POST /new`, which RoboCAD writes) only | `cad::document` (module doc), `CadAction::CadSave`, `cad/files/mod.rs:save` | done-by-reading |

## Load progress and cancel

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Nonmodal "Opening name" window: stages "Checking model", "1 / 2 · Reading CAD", "2 / 2 · Preparing display", counts "n / N parts (%)", the current part name and "Ns elapsed" | ui/model_loading.py:279-345 | `GET /loads/{id}` (GUI only) | `cad::panel` shows "connecting" with elapsed time (`Connection::Connecting { since }`; `service::START_TIMEOUT` is 120 s) | deliberately different: the headless service loads the whole document before it binds its port (api.py:1398-1399), so it has no stage counts to report |
| Cancel (button, Escape, or closing the loading window) | ui/model_loading.py:375-408, ui/model_loading.py:421-427 | `DELETE /loads/{id}` (GUI only) | dropping the connect job (generation bump in `CadDocument`; `wait_until_live` cancelled) stops the self-started child (`ChildProcess::stop`) | done-by-reading |
| Cancel is disabled once the prepared model hands off to its window ("Opening prepared model…") | ui/model_loading.py:234-244, ui/model_loading.py:410-415 | n/a | n/a: there is no hand-off; the service owns the document from the start | deliberately different: there is no separate preparation process to hand off from |
| "Could not open model" with the error and the diagnostics path | ui/model_loading.py:360-369 | n/a | `Connection::Lost` with the error verbatim and the tail of the service's stderr log | done-by-reading |
| "The CAD file changed while reading/loading" | ui/model_loading.py:115-117, ui/model_loading.py:147-149 | n/a | n/a | deliberately different: the headless service uses `Document.load` (api.py:1398), which has no such check |
| Disposable display cache keyed by archive hash (`~/Library/Caches/robocad/display`) | ui/model_loading.py:30-99, ui/model_loading.py:102-151 | n/a | `cad::mesh` caches meshes in memory by (node id, revision) | deliberately different: the native cache lives in memory only; RoboCAD's cache files are left alone |
| `POST /open` returns a `load_id`; poll and cancel through `/loads/{id}` | api.py:966-985, ui/model_loading.py:297-307 | `POST /open`, `GET/DELETE /loads/{id}` (GUI only) | File > Open… → `cad/files/mod.rs:file` (`FileOp::Open` → `CadAction::CadOpen`: a self-started headless service on the file); `crates/sim-runtime/src/cad_client/mod.rs:CadClient::open`, `load_status` and `cancel_load` stay client calls, unused | deliberately different: `POST /open` opens another RoboCAD window (409 headless) and never replaces the document; CAD mode starts its own headless service on the file instead (see "Headless versus GUI-only routes") |

## Edit and history

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Undo (`edit.undo`, status "Undo label") | ui/app.py:291, commands.py:305-306, commands.py:221-228 | `POST /undo` | `CadAction::CadUndo` → `CadClient::undo`; key Ctrl/Cmd+Z | done-by-reading |
| Redo (`edit.redo`) | ui/app.py:292, commands.py:308-309 | `POST /redo` | `CadAction::CadRedo` → `CadClient::redo`; key Ctrl/Cmd+Shift+Z | done-by-reading |
| History labels (undo and redo stacks) | commands.py:204-243, api.py:472-473 | `GET /history`, and `history` in `/doc` | `cad::panel` history list | done-by-reading |
| RoboCAD's command stack is the only undo stack (EditBodies, AddNodes, RemoveNodes, SetAttributes, MoveNode, SetMaterialDef, Composite) | commands.py:31-202 | every mutating route | every CAD edit goes to RoboCAD (`cad::actions` module doc) | done-by-reading |
| Delete (`edit.delete`; deletes the whole selection as one undo step) | ui/app.py:293, ui/app.py:1458-1463, commands.py:312 | `DELETE /nodes/{id}` (one node); `POST /ops/delete {"args": [[ids]]}` (one step for many) | `cad::ops` catalogue `edit.delete` (`Flow::Immediate`, `Arg::Nodes`; keys Delete/Backspace in `cad::keys`, the panel's Delete button `cad:delete`, the Edit menu, palette and right-click menu) → `ops::handle` → `ops::run` → `ops::prepare` (`CadDocument::commit_refusal`, `resolve::resolve`, `args::build`) → `ops::start` → `actions::edit` (one Dedicated edit job): one `POST /ops/delete {"args": [[ids]]}` (one undo step), then the selection is cleared. REST `cad_delete {id}` still deletes one node | deliberately different: refused by name ("Select the nodes to delete") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason (the Delete and Backspace keys stay silent on an empty selection, as RoboCAD's) |
| "Copy with Placement" (`edit.copy`: JSON with B-rep hex and world placement on the clipboard) | ui/app.py:294, ui/app.py:1465-1468, document.py:672 | `POST /clipboard/copy {"ids"}` (api.py `Service.copy`, 2026-10-01; read-only); GUI: `POST /commands/edit.copy` uses RoboCAD's clipboard | `cad::ops` catalogue `edit.copy` (`Shape::Copy`; Ctrl+C) → `args::build` (`Read::Copy`) → `analysis_overlay::start` (a Dedicated job, `CadClient::copy_nodes`) → `analysis_overlay::receive` keeps the clip in `OpsState::clipboard` with the revision it was read at ("Copied N item(s) with placement") | deliberately different: the clip stays in the viewer (`OpsState::clipboard`), not on the OS clipboard: the viewer reads it over REST and pastes it into the same service; an empty selection is refused by name ("Select the nodes to copy") where RoboCAD copies an empty clip |
| "Paste with Placement" (`edit.paste`, one undo step "Paste") | ui/app.py:295, ui/app.py:1470-1484, document.py:683 | `POST /clipboard/paste {"clip"}` (api.py `Service.paste`, 2026-10-01; one undo step "Paste"); GUI: `POST /commands/edit.paste` | `cad::ops` catalogue `edit.paste` (`Shape::Paste`; Ctrl+V) → `args::build` (`Built::Paste` of `OpsState::clipboard`) → `ops::start` → `actions::edit` → `CadClient::paste` (one undo step "Paste") | deliberately different: pastes the viewer's last copy, not the OS clipboard (refused "Nothing to paste: copy with placement first (Ctrl+C)" when there is none); a clip RoboCAD cannot read comes back as its "Clipboard has no robocad content" |
| Model hotkeys pause while a text or number field has focus | ui/app.py:470-477 | n/a | `cad::keys` ignores every key while the name field has focus (`CadInputFocus`; `keys` runs after `panel::name_entry`) | done-by-reading |
| A failed command reports RoboCAD's message and the app stays alive | ui/app.py:253-268 | the error JSON of each route (api.py:1280-1286) | `CadError` shown verbatim in `cad::panel` | done-by-reading |
| Rename (outliner text edit or joint dialog) | ui/widgets.py:341-348, commands.py:333 | `PATCH /nodes/{id} {"name"}` | `CadAction::CadPatch`: the inspector's name field (Enter sends `CadPatch {"name"}`) and REST `cad_patch` | done-by-reading |

## Selection and selection modes

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Click a body to select it (Select tool; "Click to select") | ui/tools.py:111-169 | `PUT /selection {"items": [[id, "body", 0]]}` | `cad::pick` click → `CadAction::CadSelect { items }` (items `[node, kind, index]` in the selection mode; body mode `[id, "body", 0]`) → `cad::selection::select` | done-by-reading |
| Shift adds to the selection | ui/tools.py:162-164 | `PUT /selection` | `cad::pick` → `CadAction::CadSelect { extend: true }` → `cad::selection::combine` | done-by-reading |
| Ctrl toggles an item | ui/tools.py:160-161, ui/viewport.py:237 | `PUT /selection` | `cad::pick` (Control or Command) → `CadAction::CadSelect { toggle: true }` → `cad::selection::combine` | done-by-reading |
| Clicking empty space clears (unless Shift/Ctrl) | ui/tools.py:154-156 | `PUT /selection {"items": []}` | `cad::pick` click on nothing → `CadSelect` with no items (kept with Shift or Ctrl) → `cad::selection::select`; REST `cad_select {"ids":[]}` | done-by-reading |
| The selection is synced with RoboCAD (its window, scripts and this viewer share it) | api.py:741-756, ui/widgets.py:316-329 | `GET/PUT /selection` | `cad::sync::selection::adopt_selection` (the poll adopts RoboCAD's items of every kind, and a desktop window's mode); `cad::selection::publish` → `cad::sync::selection::push_selection` (`PUT /selection` with items `[node, kind, index]` and the mode, one at a time, newest wins) | deliberately different: a headless RoboCAD stores only the items, not the mode (api.py:741-756), so the viewer holds its own selection mode and adopts only a desktop window's |
| Locked and hidden nodes are not pickable | ui/viewport.py:1262 | `/doc` (`locked`, `effective_visible`) | `cad::pick`: hidden and disabled bodies are not drawn; locked nodes are left out of the click, Alt and hover picks and do not hide what is behind them; box select takes them (as RoboCAD's `_box_select`, tools.py:203-228) | done-by-reading |
| Box select (drag more than 6 px; bodies whose bounding box lies inside; vertex and edge modes too) | ui/tools.py:124-137, ui/tools.py:198-228 | `PUT /selection` | `cad::pick` drag past 6 px (rubber band `cad::overlay::band`) → `CadAction::CadBoxSelect` → `cad::selection::box_select` (`box_items`: bounding-box corners in body, face and point modes; every sampled edge point; vertices; since cad-physical-inspect edge and vertex items carry their topology's revision) | done-by-reading |
| Hover highlight (coalesced to 33 ms; never replaces a click pick) | ui/tools.py:171-183, ui/viewport.py:1226-1247 | n/a (display) | `cad::pick` hover (one search per 33 ms; edge and vertex search on a `Pool::Compute` job) → `CadAction::CadHover` → `cad::selection::hover`; drawn by `cad::overlay::highlights` | done-by-reading |
| Alt+click on overlapping picks opens a disambiguation menu ("name: kind #i") | ui/tools.py:140-147, ui/widgets.py:888-894 | n/a (display) | `cad::pick` Alt+click → `CadAction::CadCandidates` → `cad::selection::candidates`, `select`, `menu_revision` (since cad-physical-inspect a choice carries the menu's revision; a stale choice is refused by name and closes the menu); the list `cad::overlay::menu` (`cad:candidate:<n>`, "name: kind #i") | done-by-reading |
| Selection mode bodies (`select.body`, "Select bodys") | ui/app.py:319-320, ui/app.py:597-602 | `PUT /selection {"mode": "body"}` | `CadAction::CadSelectMode { mode: Body }` → `cad::selection::set_mode`; `cad::pick` picks `[id, "body", 0]` | done-by-reading |
| Selection mode faces (`select.face`) | ui/app.py:319-320, ui/viewport.py:1281-1293 | `PUT /selection {"mode": "face"}`; `GET /nodes/{id}/mesh` (`triangle_face`) | `CadSelectMode { mode: Face }` → `cad::selection::set_mode`; `cad::pick` maps the hit triangle to `[id, "face", f]` (`CadMeshes::face_of`); outlines `cad::overlay` | done-by-reading |
| Selection mode edges (`select.edge`) | ui/app.py:319-320, ui/viewport.py:1301-1309 | `GET /nodes/{id}/edges?samples=N` | `CadSelectMode { mode: Edge }`; `cad::pick::search`: the nearest sampled edge polyline within 6 px not behind the first surface → `[id, "edge", i]` (polylines from `cad::topology`); `cad::overlay` draws the edges | done-by-reading |
| Selection mode vertices (`select.vertex`) | ui/app.py:319-320, ui/viewport.py:1310-1316 | `GET /nodes/{id}/vertices` | `CadSelectMode { mode: Vertex }`; `cad::pick::search`: the nearest vertex within 6 px → `[id, "vertex", i]`; marks `cad::overlay` | done-by-reading |
| Selection mode points (`select.point`: a surface point) | ui/app.py:319-320, ui/viewport.py:1281, ui/viewport.py:1337 | `PUT /selection {"mode": "point"}` | `CadSelectMode { mode: Point }`; `cad::pick` ray cast → `[id, "point", f]` (the hit face, as RoboCAD's pick pass) | done-by-reading |
| Changing the selection mode clears the selection ("Selection mode: mode") | ui/app.py:597-602 | `PUT /selection` | `cad::selection::set_mode` (clears; status "Selection mode: m") | done-by-reading |
| "Select All" (`edit.select_all`: visible bodies, sheets, curves, instances and meshes) | ui/app.py:296, ui/app.py:604-610 | `PUT /selection` (computed from `/doc`) | `CadAction::CadSelectAll` → `cad::selection::select_all` (`SELECTABLE_KINDS`, visible) | done-by-reading |
| "Invert Selection" (`edit.invert`) | ui/app.py:297, ui/app.py:612-619 | `PUT /selection` | `CadAction::CadInvertSelection` → `cad::selection::invert` | done-by-reading |
| "Select Same Material" (`edit.select_same_material`) | ui/app.py:298, ui/app.py:621-629, document.py:448 | `PUT /selection` (from `/doc` materials) | `CadAction::CadSelectSameMaterial` → `cad::selection::same_material` | deliberately different: refuses by name when nothing is selected or the first selected node has no material; RoboCAD does nothing for an empty selection and selects every body without a material for one with none |
| "Selection: edges → bounding faces" (`edit.convert_faces`) | ui/app.py:299, ui/app.py:631-645 | `GET /nodes/{id}/edges`, `/faces`; `PUT /selection` | `CadAction::CadEdgesToFaces` → `cad::selection::edges_to_faces` (`faces_along`; since cad-physical-inspect the faces are stamped with the topology revision, and an edge picked at an older revision is refused by name) | deliberately different: RoboCAD asks its kernel (`faces_of_edge`), which has no REST route; the faces come from RoboCAD's drawn tessellation along the sampled edge polyline, and the command refuses by name until both are loaded |
| Status "n selected" / "Ready" | ui/app.py:594-595 | n/a (display) | `cad::selection` ("n selected"); the `cad::panel` status line shows "Ready" when empty | done-by-reading |
| Escape clears the selection in the Select tool, or returns to the Select tool | ui/app.py:487-497 | n/a | Escape (`cad::transform::keys`) → `CadAction::CadCancel` → `cad::transform::cancel` (closes the Alt menu first, then leaves a tool, then clears the selection) | done-by-reading |

## Tree (outliner)

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Hierarchy from roots and children, indented | ui/widgets.py:271-314 | `GET /doc` (`roots`, `nodes[].parent`, `children`) | `cad::tree` (indent by depth) | done-by-reading |
| Name column | ui/widgets.py:293 | `GET /doc` | `cad::tree` | done-by-reading |
| Kind: folder or file icon, tooltip "Kind · Right-click for Fit in view and organization" | ui/widgets.py:294-295 | `GET /doc` (`kind`) | `cad::tree` shows the kind | done-by-reading |
| Visibility column 👁/◌; clicking toggles | ui/widgets.py:293, ui/widgets.py:359-360 | `PATCH /nodes/{id} {"visible"}` | `cad::tree` toggle → `CadAction::CadPatch` (system_ui `cad:visible:<id>`) | done-by-reading |
| A row is greyed when not effectively visible (a hidden ancestor) | ui/widgets.py:301-302 | `GET /doc` (`effective_visible`) | `cad::tree` | done-by-reading |
| Lock column 🔒; clicking toggles | ui/widgets.py:293, ui/widgets.py:361-362 | `PATCH /nodes/{id} {"locked"}` | shown in `cad::tree`; toggled in `cad::inspector` (`CadPatch`) | done-by-reading |
| Disabled column ⏸; clicking toggles | ui/widgets.py:293, ui/widgets.py:363-364 | `PATCH /nodes/{id} {"disabled"}` | shown and toggled in `cad::inspector` | done-by-reading |
| Selected rows highlighted, and kept in sync without rebuilding the tree | ui/widgets.py:307-308, ui/widgets.py:316-329 | `GET /selection` | `cad::tree` | done-by-reading |
| Click a row to select it (always a body item `(id, "body", 0)`, even for groups and joints) | ui/widgets.py:331-339 | `PUT /selection` | `cad::tree` row → `CadAction::CadSelect` (system_ui `cad:node:<id>`) | done-by-reading |
| Shift/Ctrl multi-select in the tree | ui/widgets.py:250 | `PUT /selection` | cad-organize epic | later-epic: cad-organize |
| Active group shown in blue | ui/widgets.py:299-300 | `GET /doc` (`active_group`) | cad-organize epic | later-epic: cad-organize |
| Search "Search (Ctrl+F)…" (matches names; shows ancestors and descendants; expands them temporarily) | ui/widgets.py:231-233, ui/widgets.py:276-291, ui/strings.py:20 | n/a (display) | cad-organize epic | later-epic: cad-organize |
| "New group" button ("Organize components" / "Group name:") | ui/widgets.py:236, ui/widgets.py:392-395 | `POST /ops/group` | cad-organize epic | later-epic: cad-organize |
| "Expand all" / "Collapse all"; collapse state survives edits and search | ui/widgets.py:237-238, ui/widgets.py:261-269, ui/widgets.py:306 | n/a (display) | cad-organize epic | later-epic: cad-organize |
| Double-click a name to rename it in place | ui/widgets.py:256, ui/widgets.py:341-352 | `PATCH /nodes/{id} {"name"}` | cad-organize epic (the REST rename is done: see "Edit and history") | later-epic: cad-organize |
| Drag rows into a group, or before a sibling | ui/widgets.py:251-252, ui/widgets.py:366-381 | `POST /ops/move_nodes`; `PATCH /nodes/{id} {"parent", "index"}` | cad-organize epic | later-epic: cad-organize |
| Context menu "Fit in view" ("Fit in view: names" / "No geometry to frame in this selection") | ui/widgets.py:397-401, ui/widgets.py:408 | n/a (display) | `CadAction::CadFit { id }` (native camera) | done-by-reading |
| Context menu "Isolate" | ui/widgets.py:410 | `POST /ops/isolate` | View > Isolate, `/` and the 3D view's right-click "Isolate" → `cad/ops/catalogue/view.rs:ENTRIES` (`view.isolate`: `POST /ops/isolate` on the selected nodes) | deliberately different: offered in the View menu, on `/` and in the 3D view's right-click menu: the native tree has no context menu (the outliner's menu belongs to cad-organize) |
| Context menu "Hide" / "Show" (whole selection, one undo step) | ui/widgets.py:411-412 | `POST /ops/set_visible` | Hide: View > Hide, H and the 3D view's right-click "Hide" → `cad/ops/catalogue/view.rs:ENTRIES` (`view.hide`: `POST /ops/set_visible` with false, one undo step); Show: the tree's per-node visibility toggle, or Show All | deliberately different: the native tree has no context menu (the outliner's menu belongs to cad-organize), so there is no "Show" of a whole selection; Hide is in the View menu, on H and in the 3D view's right-click menu |
| Context menu "Lock" / "Unlock" (whole selection) | ui/widgets.py:413-414 | `POST /ops/set_locked` | cad-organize epic | later-epic: cad-organize |
| Context menu "Group selection…" | ui/widgets.py:415 | `POST /ops/group` | cad-organize epic | later-epic: cad-organize |
| Context menu "Move to group" ▸ "Top level" and every group path ("A / B") | ui/widgets.py:416-430 | `POST /ops/move_nodes` | cad-organize epic | later-epic: cad-organize |
| Context menu "Make unique (bake instance)" | ui/widgets.py:431 | `POST /ops/make_unique` | `cad::surfaces::context_menu`: `registry::MAKE_UNIQUE` ("Make unique (bake instance)") is added to the 3D view's right-click menu while an instance is selected (`context_menu::instance_selected`) → `CadInvoke { modify.make_unique }` → `cad::ops` catalogue `modify.make_unique` (instances only, one call per node) | deliberately different: offered in the 3D view's right-click menu: the native tree has no context menu (the outliner's menu belongs to cad-organize) |
| Context menu "Set as active group"; "Clear active group"; registry "Set selected group as active" (`group.set_active`) | ui/widgets.py:432-435, ui/app.py:427 | `POST /ops/set_active_group` | cad-organize epic | later-epic: cad-organize |
| Context menu "Delete" | ui/widgets.py:434 | `DELETE /nodes/{id}`; `POST /ops/delete` | the panel's Delete (`cad:delete` → `CadInvoke { edit.delete }`: every selected node in one `POST /ops/delete` since cad-modify); REST `cad_delete` (`CadAction::CadDelete`, one node) | done-by-reading |
| Context menu "Show all" | ui/widgets.py:436 | `POST /ops/show_all` | View > Show All and Alt+H → `cad/ops/catalogue/view.rs:ENTRIES` (`view.show_all`: `POST /ops/show_all`) | deliberately different: offered in the View menu and on Alt+H: the native tree has no context menu (the outliner's menu belongs to cad-organize) |
| "Group selection" (`group.group`) | ui/app.py:428 | `POST /ops/group` | cad-organize epic | later-epic: cad-organize |

## Inspector (properties)

RoboCAD's "Selection" panel (`PropertiesPanel`) first, then one row per
field group of `node_detail` (api.py:98-133), the record the native
inspector shows as returned.

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Facts: "Nothing selected." / "n item(s)" / "Display size ≈ x × y × z mm" / "Exact measurements available on request." (display bounds only; a click never integrates a B-rep) | ui/widgets.py:452-455, ui/widgets.py:494-500, ui/widgets.py:555-564 | `GET /nodes/{id}` | `cad::inspector` shows `GET /nodes/{id}` (with its exact mass block) for the first selected node | deliberately different: the native inspector shows RoboCAD's node detail, whose mass block RoboCAD computes for each request (api.py:105-110). On a large imported body that request is slow, and in the GUI it runs on RoboCAD's Qt thread (api.py:1143-1144); RoboCAD's own panel avoids it |
| "Calculate exact measurements" (a separate process over the whole selection: size, volume, area, mass, centroid; 60 s limit; cancelled by any edit or selection change) | ui/widgets.py:456-462, ui/widgets.py:566-633 | `GET /nodes/{id}` per node (combined natively) | `cad/inspector/exact.rs:start`, `settle`, `sync` (the inspector's "Calculate exact measurements" button: one `GET /nodes/{id}` per selected node on a `Pool::Dedicated` job, combined as RoboCAD's `analysis.selection_properties`) | deliberately different: one `GET /nodes/{id}` per node on a job instead of RoboCAD's child process over the whole selection; the 60 s limit and the cancel on any edit or selection change are RoboCAD's, but a cancel only stops waiting (the request already sent finishes in RoboCAD); the preview drops "Display size ≈" |
| Live dimensions of the selected faces and edges, editable | ui/widgets.py:505-509, ui/app.py:654-691 | `GET /nodes/{id}/faces`, `/edges`; `POST /ops/set_diameter`, `set_distance`, `set_angle` | `cad::transform::dimensions::live` as `cad::numeric` fields; Enter → `CadAction::CadSetDimension` → `cad::transform::commit::dimension_call` | done-by-reading |
| "Material" dropdown "name (density g/cm³)", applied to the selection | ui/widgets.py:465-468, ui/widgets.py:491-503, ui/widgets.py:723-728 | `PATCH /nodes/{id} {"material"}`; `GET /doc` (`materials`) | `cad::inspector` material choice → `CadPatch` | done-by-reading |
| "Tessellation tolerance (mm)" (0.005–2.0; RoboCAD's panel sets it without undo) | ui/widgets.py:469-476, ui/widgets.py:730-735 | `PATCH /nodes/{id} {"tessellation_tolerance"}` (undoable) | `cad/inspector/editors.rs:patch_for` (the inspector's "Tessellation tolerance (mm)" field: 0.005–2 mm, three decimals; one `PATCH /nodes/{id} {"tessellation_tolerance"}`); meshes refetched at the node's own tolerance (`cad/mesh.rs:sync`) | deliberately different: it patches the inspected node as one undo step, like every inspector edit (RoboCAD's spin box writes every selected node directly, without undo), and the field opens empty because RoboCAD reports no node's current value |
| Joint physics overrides: "Radial clearance (mm)", "Wobble (°)", "Drive backlash (°; provenance)" ("Unmeasured"), "Coulomb friction (mN·m)", "Viscous (mN·m·s)", "Radial stiffness (N/m)", "Flex patch radius (mm)", the source line, and "*" for overridden values | ui/widgets.py:510-544, ui/widgets.py:635-679 | `GET /physical?flex=0` (joint `physics`, physical.py:552); `POST /ops/set_joint_physics` | `cad/inspector/rows.rs:row` (the joint physics rows) and `cad/inspector/physical_edit.rs:joint_override` → `handle_physical` (`POST /ops/set_joint_physics` through `cad/actions.rs:edit_at`); values and provenance from `GET /physical?flex=0`, read again on each revision by `cad/inspector/refresh.rs:sync` | deliberately different: a value the physical model lacks is left empty instead of RoboCAD's 0.0 |
| "Results: key value, …" for the selected node | ui/widgets.py:545-549 | `GET /results/nodes` (api.py `Service.results_nodes`, added with cad-physical-inspect: each node's `results` block and its material's yield strength; pytests `cad/tests/test_api_physical_routes.py`) | `cad/inspector/rows.rs:results_line` over the robot reads' `GET /results/nodes` (`cad/robot/data.rs:sync`) | done-by-reading |
| "Material properties…" ("name: engineering properties" dialog) | ui/widgets.py:550-552, ui/widgets.py:681-713 | `POST /ops/set_material_props` | `cad/inspector/physical_edit.rs:controls_of` ("Material properties…") → `cad/materials/form.rs:properties_form` | done-by-reading |
| The panel is disabled during pose preview | ui/pose.py:127, ui/pose.py:350 | n/a | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| `id`, `kind`, `name`, `parent`, `children`, `source` | api.py:98-99 | `GET /nodes/{id}` | `cad::inspector` (as returned) | done-by-reading |
| `visible` (editable) | api.py:99, api.py:584-585 | `PATCH /nodes/{id} {"visible"}` | `cad::inspector` → `CadPatch` | done-by-reading |
| `locked` (editable) | api.py:99, api.py:586-587 | `PATCH /nodes/{id} {"locked"}` | `cad::inspector` → `CadPatch` | done-by-reading |
| `disabled` (editable) | api.py:99, api.py:588-589 | `PATCH /nodes/{id} {"disabled"}` | `cad::inspector` → `CadPatch` | done-by-reading |
| `material` (editable) | api.py:99, api.py:590-591 | `PATCH /nodes/{id} {"material"}` | `cad::inspector` → `CadPatch` | done-by-reading |
| `name` (editable) | api.py:99, api.py:582-583 | `PATCH /nodes/{id} {"name"}` | `CadPatch`: the inspector's name field and REST `cad_patch` | done-by-reading |
| `effective_visible` | api.py:99 | `GET /nodes/{id}` | `cad::inspector`, `cad::tree` | done-by-reading |
| `color` shown | api.py:99 | `GET /nodes/{id}` | `cad::inspector` (as returned) | done-by-reading |
| `color` editor | api.py:592-593, commands.py:348 | `PATCH /nodes/{id} {"color"}` (REST `cad_patch` works now) | `cad/inspector/physical_edit.rs:handle_physical` (`Color`, `MaterialColor`: `PATCH /nodes/{id} {"color"}` through `cad/actions.rs:edit_at`) | deliberately different: an "r, g, b" field plus "Use material colour" instead of a colour dialog |
| `pivot` shown | api.py:99 | `GET /nodes/{id}` | `cad::inspector` | done-by-reading |
| `pivot` editor | api.py:594-595, commands.py:351 | `PATCH /nodes/{id} {"pivot"}` | `cad::inspector::editors` (`EditKey::Pivot`: `editors::entry` evaluates "x, y, z" with `sim_runtime::units` → `patch_for` → `CadAction::CadPatch {"pivot"}`, one undo step; "Clear pivot" sends `null`; refused as RoboCAD refuses a component member's, `editors::refusal`) | done-by-reading |
| `transform` shown (`Transform.to_json`) | api.py:99 | `GET /nodes/{id}` | `cad::inspector` | done-by-reading |
| `transform` editor (refused for component occurrences, api.py:577-578) | api.py:596-597 | `PATCH /nodes/{id} {"transform"}` | `cad::inspector::editors` (`EditKey::Translation`, `Axis`, `Angle`, `Scale`: one changed component is sent with the other three as RoboCAD reported them, `placement` → `patch_for` → `CadAction::CadPatch {"transform"}`; refused for component occurrences and members as RoboCAD refuses them, `editors::refusal`) | deliberately different: the editor is shown only for the nodes RoboCAD keeps a placement for (instances, reference meshes and images; bodies are baked in world, document.py:142-144), and a transform RoboCAD sent without one of its keys is not edited (nothing is filled in) |
| Mass block: `volume_mm3`, `area_mm2`, `mass_g`, `centroid`, `bbox_min`, `bbox_max`, `size` | api.py:104-107 | `GET /nodes/{id}` | `cad::inspector` (`MassBlock`) | done-by-reading |
| `body_kind`, `face_count`, `edge_count` | api.py:106-109 | `GET /nodes/{id}` | `cad::inspector` | done-by-reading |
| `sketch` (curves on a plane) shown | api.py:110-111 | `GET /nodes/{id}`, `GET /nodes/{id}/sketch` | `cad::inspector` (as returned) | done-by-reading |
| `plane` shown | api.py:112-113 | `GET /nodes/{id}` | `cad::inspector` | done-by-reading |
| `measure` shown | api.py:114-115 | `GET /nodes/{id}` | `cad::inspector` | done-by-reading |
| `mirror_plane` (live mirror instance) shown | api.py:116-117 | `GET /nodes/{id}` | `cad::inspector` | done-by-reading |
| `joint` shown | api.py:118-119 | `GET /nodes/{id}` | `cad::inspector` | done-by-reading |
| `joint` editor ("Edit joint" dialog) | ui/app.py:1565-1575 | `POST /ops/set_joint`, `POST /ops/rename` | `cad/inspector/rows.rs:joint` ("Edit joint") → the catalogue's `ops.set_joint` (`cad/ops/catalogue/robot.rs`; `cad/ops/robot_args.rs` `EditJoint`: `set_joint`, then `rename` only when the name changed) | done-by-reading |
| `robot` (motor, ground, print-split and other metadata) shown | api.py:120-121 | `GET /nodes/{id}` | `cad::inspector` | done-by-reading |
| Sensors (`kind: sensor`, `robot` block: kind, body, point, axes, rate, joint) shown | api.py:120-121, api.py:1250-1253 | `GET /nodes/{id}`, `GET /sensors` | `cad::inspector` | done-by-reading |
| Cables (`kind: cable`, `robot` block: from/to body and point, length, mass) shown | api.py:120-121, api.py:1254-1257 | `GET /nodes/{id}`, `GET /cables` | `cad::inspector` | done-by-reading |
| `component_instance` / `component_member` shown (PATCH of a member is limited to visible, color and name, api.py:575-576) | api.py:99, api.py:122-125 | `GET /nodes/{id}` | `cad::inspector`; a refused PATCH is shown verbatim | done-by-reading |
| `mesh` (vertex and triangle counts of an imported mesh) | api.py:126-127 | `GET /nodes/{id}` | `cad::inspector` | done-by-reading |
| `image` (width, height, opacity, rotation, plane; pixel data stripped) | api.py:128-130 | `GET /nodes/{id}` | `cad::inspector` | done-by-reading |
| The physical link's mass and `mass_sources` labels for the selected body | physical.py:766 | `GET /physical?flex=0` (never `path`) | `CadAction::CadPhysical` → `CadClient::physical(false)`; `cad::inspector` physical section | done-by-reading |

## Materials

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Materials panel: "■ name density g/cm³" in the material's colour | ui/widgets.py:741-774, ui/app.py:186-189 | `GET /materials` | `cad/materials/panel.rs:draw` (a section of the right dock) | done-by-reading |
| "Search materials…" (name or tag) | ui/widgets.py:747-750, ui/widgets.py:768-770 | n/a (display) | `cad/materials/mod.rs:matches` | done-by-reading |
| "Apply to selection" button and double-click | ui/widgets.py:753, ui/widgets.py:756-758, ui/widgets.py:776-781 | `POST /ops/set_material` | `cad/materials/mod.rs:apply`, `cad/materials/panel.rs:input` (Apply and double-click: one `POST /ops/set_material` over the shared selection's nodes through `cad/actions.rs:edit_at`) | done-by-reading |
| Drag a material onto a body in the viewport | ui/widgets.py:752, ui/widgets.py:783-793, ui/app.py:1801-1816 | `POST /ops/set_material` | none (Apply to selection and double-click, `cad/materials/mod.rs:apply`) | deliberately different: a drop onto a body would need a second pointer path (a drag across the dock into the view with its own ray cast) beside `cad/pick.rs`; Apply and double-click make the same single edit |
| "New…" material dialog ("Name", "Density (g/cm³)") | ui/widgets.py:759-761, ui/widgets.py:795-815 | `POST /materials` | `cad/materials/form.rs:new_form`, `submit` → `cad/materials/mod.rs:submit` (`POST /materials`) | done-by-reading |
| Engineering properties dialog (Young's modulus, Poisson ratio, yield, ultimate, glass transition, conductivity, specific heat, expansion, bearing pressure, friction against itself and steel, print anisotropy) | ui/widgets.py:681-713 | `POST /ops/set_material_props` | `cad/materials/form.rs:properties_form`, `submit` → `cad/materials/mod.rs:submit` (one `POST /ops/set_material_props`) | deliberately different: only the changed keys are sent; the defaults shown come from the physical model's materials, else "not reported" (RoboCAD's `_ENG` table is not copied into Rust) |

## Viewport: display, camera, views, grid, build plate, section, isolate

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Tessellated bodies, sheets, instances and meshes in their node or material colour | ui/viewport.py:397-430, ui/viewport.py:693-812, ui/viewport.py:1617-1633 | `GET /nodes/{id}/mesh?tolerance=` | `cad::mesh` (fetch, Compute build, `CadBody` entities, display only) | done-by-reading |
| Tessellation at the node's `tessellation_tolerance` | ui/viewport.py:265-268, ui/widgets.py:469-476 | `GET /nodes/{id}/mesh?tolerance=` | `cad/mesh.rs:sync` asks `GET /nodes/{id}/mesh` with `crates/sim-runtime/src/cad_client/mod.rs:NODE_TOLERANCE` (0), which RoboCAD's `mesh_of` reads as the node's own tolerance (since cad-views-export; before, a fixed 0.1 mm) | done-by-reading |
| Z-up world, lights | ui/viewport.py:41-75, ui/viewport.py:568-588 | n/a (display) | `cad::scene` (Z-up root, light) | done-by-reading |
| Display "shaded" | ui/viewport.py:260, ui/viewport.py:693-760 | n/a (display) | `cad::mesh` shaded bodies | done-by-reading |
| Display "shaded with edges" (RoboCAD's default) | ui/viewport.py:282, ui/viewport.py:761-786 | B-rep edge polylines: see the row below | `cad/display/draw.rs:edges_sync` (each drawn body's `GET /nodes/{id}/edges?samples=24` on `Pool::Dedicated`, or `CadTopology`'s) and `lines` → `draw_edges` (RoboCAD's 0.08, 0.08, 0.1, cut by the section plane); the default mode (`cad/display/mod.rs:CadDisplay`) | done-by-reading |
| Display "wireframe" | ui/viewport.py:260, ui/viewport.py:765-775 | n/a (display) | `cad/display/draw.rs:derive_material` (`MaterialLook::Wireframe`: a transparent copy that stays pickable) and `draw_edges` (0.6 × colour + 0.3; a mesh node shows its triangle sides) | done-by-reading |
| Display "xray" | ui/viewport.py:260, ui/viewport.py:732-760 | n/a (display) | `cad/display/draw.rs:derive_material` (`MaterialLook::Xray`: alpha 0.35, blended) and `draw_edges` | done-by-reading |
| Display "matcap" (procedural clay) | ui/viewport.py:260, ui/viewport.py:700-707, ui/viewport.py:1682-1703 | n/a (display) | `cad/display/draw.rs:derive_material` (`MaterialLook::Matcap`) | deliberately different: approximated without a custom shader: the colour times RoboCAD's clay tint (235/225/210), fully rough, lit by the view-following headlight; RoboCAD's sampled sphere image and its rim term are not reproduced |
| Display "render" (three lights, ground shadow) | ui/viewport.py:260, ui/viewport.py:336-337, ui/viewport.py:708, ui/viewport.py:879-904 | n/a (display) | `cad/display/draw.rs:lights` (the headlight casts shadows; RoboCAD's fill and back lights) | deliberately different: no ground shadow (Bevy has no shadow-catcher material) and the ambient is not lowered; bodies shadow each other |
| "Next display mode" (`view.mode_next`) and "Display: …" (`view.mode.shaded`, `view.mode.shaded_edges`, `view.mode.wireframe`, `view.mode.xray`, `view.mode.matcap`, `view.mode.render`) | ui/app.py:307-309, ui/app.py:1042-1048 | n/a (display) | `cad/surfaces/registry.rs:DisplayCmd::action` → `cad/display/mod.rs:apply_display` (`next` in RoboCAD's order, or `mode`): Z, the View menu, the view radial's Mode, the toolbar's six modes (`cad/display/ui.rs:toolbar`), REST `cad_display`, `system_ui` `cad:display:*` | done-by-reading |
| B-rep edge polylines (display edges, edge picking, curve nodes) | ui/viewport.py:1634-1672, ui/viewport.py:906-920 | `GET /nodes/{id}/edges?samples=N` (api.py:625-638: each edge's `points`, as `kernel.sample_edges`); `/mesh` has no curves | display edges: `cad/display/draw.rs:edges_sync` and `draw_edges` (bodies, sheets, instances; a mesh node shows its triangle sides); edge picking: `cad::pick` and `cad::topology` (cad-select-transform); curve nodes: `cad/display/draw.rs:edges_sync` fetches every visible `curve` node's `GET /nodes/{id}/edges?samples=32` (`curve_nodes`, `fetch_edges` with `CURVE_SAMPLES`, in every display mode) and `lines` draws them as 2 px gizmo polylines in RoboCAD's colours (`curve_color`: 1.0, 0.65, 0.2 selected, else the node's colour, else 0.35, 0.8, 1.0), cut by the section plane | deliberately different: curve nodes are drawn but not pickable in the 3D view (RoboCAD's 8 px curve pick pass is not ported); pick them in the tree |
| Sketch curves drawn on their planes | ui/viewport.py:922-939 | `GET /nodes/{id}` (its `sketch` field, so dropped curves are counted) | `cad::sketch::display` (`shown`: every `effective_visible` sketch node; `lines`: each curve's `SketchCurve::sample(48)` through the sketch's own plane, from `CadSketches::sketch_last`; `draw`, Present: orange (1.0, 0.65, 0.2) when selected, else blue (0.35, 0.8, 1.0), 2 px over the bodies); the geometry from `cad::sketch::cache::sync` (`CadClient::sketch`, one Dedicated job per node, by (node, revision)) | deliberately different: a slot's caps bulge outward, as the kernel's solid does (`sim_runtime::cad_client::SketchCurve::sample`); RoboCAD's viewport turns them inward (`io/exporters.py` `_slot_points` sweeps the other way: a RoboCAD display bug, see the notes below) |
| Construction planes (translucent quads; the active plane is brighter) | ui/viewport.py:635-657 | `GET /doc`, `GET /nodes/{id}` (`plane`) | `cad::sketch::plane_draw` (`wanted`, `quads`: every visible plane node with its frame read (`CadSketches::plane_last`), a ±60 mm square filled (0.3, 0.6, 0.9) at alpha 0.18 when it is the active plane, else 0.08; `outlines`: alpha 0.8, else 0.4); frames from `cad::sketch::cache` | deliberately different: the active plane is drawn even when no visible plane node is it (XY, XZ, YZ or a hidden node), so the plane the tools work on is always shown; RoboCAD draws plane nodes only |
| Reference images textured on their planes | ui/viewport.py:659-691 | none: needs a Python route (`node_detail` strips the image bytes, api.py:128-130) | cad-organize epic | later-epic: cad-organize |
| Joint glyphs, motor shaft axes, sensor triads and sagging cable arcs | ui/viewport.py:971-1021 | `GET /nodes/{id}` (`joint`, `robot`) | `cad/robot/glyphs.rs:draw` (shapes from `glyph_lines`; cached by generation and read revision) | deliberately different: screen sizes are measured at each glyph's own depth; dots are small three-axis crosses (gizmos draw no points); nothing is hidden for a pose preview (CAD mode has none) |
| Orbit (right-drag, Alt+left-drag, Shift+middle-drag; turntable) | ui/viewport.py:1480-1512, ui/viewport.py:79-86 | n/a (display) | `camera/input.rs:navigate` → `camera/orbit.rs:Orbit::rotate` (turntable, 89.5° pitch limit, `cad/scene.rs:rules`): right-drag, and with `OrbitRules::robocad_gestures` (true only in CAD, `cad/scene.rs:rules`) Shift+middle-drag (`camera/input.rs:drag_kind`) and Alt+left-drag once it moves past `camera/input.rs:ALT_DRAG_SLOP` (6 px), gated by `cad/scene.rs:gate` to the Select tool with no catalogue interaction or command surface; `cad::pick` no longer box-selects an Alt drag, and an Alt click still opens the candidates menu (RoboCAD's own Alt+left orbit never fires: see the notes at the end) | done-by-reading |
| Pan (Shift+right-drag, middle-drag) | ui/viewport.py:1489-1497, ui/viewport.py:88-91 | n/a (display) | `camera/input.rs:navigate` → `camera/orbit.rs:Orbit::pan` on middle-drag or Shift+right-drag | done-by-reading |
| Wheel zoom | ui/viewport.py:1553-1569 | n/a (display) | `cad::scene` zoom | done-by-reading |
| Wheel zoom toward the point under the cursor | ui/viewport.py:97-103, ui/viewport.py:1559-1563 | n/a (display) | `camera/input.rs:navigate` → `cursor_anchor` (the point under the cursor on the plane through the focus, facing the view) → `camera/orbit.rs:Orbit::zoom` (the eye scales about it, as RoboCAD's `Camera.zoom`); on in CAD (`cad/scene.rs:rules`, `zoom_to_cursor`) | done-by-reading |
| "Toggle orbit: turntable / trackball" (`view.orbit_mode`) | ui/app.py:310, ui/app.py:1050-1057 | n/a (display) | `view.orbit_mode` (View menu) → `CameraAction::OrbitMode` → `camera/orbit.rs:Orbit::set_trackball` (starts from the current view; back to the nearest turntable heading) | done-by-reading |
| Hold Alt while right-orbiting to snap to an axis view | ui/viewport.py:1494-1495, ui/viewport.py:116-120 | n/a (display) | `camera/input.rs:navigate`: Alt with a right-drag orbit (`drag_kind` → `DragKind::OrbitSnap`) rotates, then `camera/orbit.rs:Orbit::snap_to_axis` (yaw to a multiple of 90°, pitch to ±89.5° beyond ±45°, else level), after every orbit step while Alt is held, as RoboCAD's `snap_orthographic` | done-by-reading |
| Arrow keys orbit 10° (Ctrl: 90°, Shift: pan) | ui/viewport.py:1535-1551 | n/a (display) | `camera/input.rs:keys` → `arrow_action` while `OrbitRules::robocad_gestures` (CAD) and no text field has the keyboard (`typing`): `CameraAction::Orbit { degrees }` by 10° (Ctrl/Cmd 90°), or with Shift `CameraAction::Pan` by 4 px per degree (RoboCAD's `pan(-dx × 4, dy × 4)`), applied by `camera/apply.rs:handle` | done-by-reading |
| "Fit All" (`view.fit`, Home) | ui/app.py:301, ui/viewport.py:443-447 | n/a (display) | `CadAction::CadFit { id: None }` → `cad::scene`; key Home | done-by-reading |
| "Focus Selection" (`view.focus`, F; includes a group's descendants) | ui/app.py:302, ui/viewport.py:449-474 | n/a (display) | F, View > Focus Selection → `cad/surfaces/registry.rs:focus` (frames the selected nodes and their descendants; Fit All with nothing selected) | done-by-reading |
| "View front" (`view.front`) | ui/app.py:303-304, ui/viewport.py:110-114 | n/a (display) | 1 (`cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`); the View menu; the view cube; the view radial for front, top, right and iso) → `CameraAction::View { front }` → `camera/orbit.rs:Orbit::preset` (RoboCAD's yaw and pitch table, `camera/mod.rs:ViewPreset::robocad_degrees`; a cut, as `Camera.set_view`) | done-by-reading |
| "View back" (`view.back`) | ui/app.py:303-304 | n/a (display) | Ctrl+1 (`cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`); the View menu; the view cube; the view radial for front, top, right and iso) → `CameraAction::View { back }` → `camera/orbit.rs:Orbit::preset` (RoboCAD's yaw and pitch table, `camera/mod.rs:ViewPreset::robocad_degrees`; a cut, as `Camera.set_view`) | done-by-reading |
| "View top" (`view.top`) | ui/app.py:303-304 | n/a (display) | 7 (`cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`); the View menu; the view cube; the view radial for front, top, right and iso) → `CameraAction::View { top }` → `camera/orbit.rs:Orbit::preset` (RoboCAD's yaw and pitch table, `camera/mod.rs:ViewPreset::robocad_degrees`; a cut, as `Camera.set_view`) | done-by-reading |
| "View bottom" (`view.bottom`) | ui/app.py:303-304 | n/a (display) | Ctrl+7 (`cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`); the View menu; the view cube; the view radial for front, top, right and iso) → `CameraAction::View { bottom }` → `camera/orbit.rs:Orbit::preset` (RoboCAD's yaw and pitch table, `camera/mod.rs:ViewPreset::robocad_degrees`; a cut, as `Camera.set_view`) | done-by-reading |
| "View right" (`view.right`) | ui/app.py:303-304 | n/a (display) | 3 (`cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`); the View menu; the view cube; the view radial for front, top, right and iso) → `CameraAction::View { right }` → `camera/orbit.rs:Orbit::preset` (RoboCAD's yaw and pitch table, `camera/mod.rs:ViewPreset::robocad_degrees`; a cut, as `Camera.set_view`) | done-by-reading |
| "View left" (`view.left`) | ui/app.py:303-304 | n/a (display) | Ctrl+3 (`cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`); the View menu; the view cube; the view radial for front, top, right and iso) → `CameraAction::View { left }` → `camera/orbit.rs:Orbit::preset` (RoboCAD's yaw and pitch table, `camera/mod.rs:ViewPreset::robocad_degrees`; a cut, as `Camera.set_view`) | done-by-reading |
| "View iso" (`view.iso`) | ui/app.py:303-304 | n/a (display) | 0 (`cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`); the View menu; the view cube; the view radial for front, top, right and iso) → `CameraAction::View { iso }` → `camera/orbit.rs:Orbit::preset` (RoboCAD's yaw and pitch table, `camera/mod.rs:ViewPreset::robocad_degrees`; a cut, as `Camera.set_view`) | done-by-reading |
| "Orthographic" (`view.ortho`) | ui/app.py:305, ui/app.py:1034-1036 | n/a (display) | 5, View > Orthographic, the view radial's Ortho → `CameraAction::Projection` (toggle) → `camera/orbit.rs:Orbit::projection` (ortho height 2 × distance × tan(fov / 2), as RoboCAD's) | done-by-reading |
| "Set field of view…" (`view.fov`, 5–120°) | ui/app.py:311, ui/app.py:1059-1063 | n/a (display) | View > Set field of view… → `cad/views/mod.rs:open_fov` (the "Field of view" entry, degrees 5–120, one decimal, `cad/views/panel.rs:input`) → `CameraAction::Fov` (`camera/apply.rs:handle`) | done-by-reading |
| View cube in the corner; a click sets that view; a second click shows the opposite | ui/viewport.py:1134-1196, ui/viewport.py:1458-1468 | n/a (display) | `cad/display/ui.rs:toolbar` (the cube net) and `cube_press` → `cube_action` (`CameraAction::View`, or `Opposite` when the camera is already at that face); the lit face is `facing` | deliberately different: a 3 × 3 net of kit buttons (Top; Left, Front, Right; Iso, Bottom, Back) in the display panel instead of a shaded 3D cube; the clicks behave as RoboCAD's |
| "Grid" (`view.grid`; 10 mm step; drawn on XY) | ui/app.py:306, ui/app.py:1038-1040, ui/viewport.py:590-620 | n/a (display) | Ctrl+G, View > Grid, the view radial's Grid, the toolbar's Grid chip → `cad/display/mod.rs:apply_display` (toggle) → `cad/display/draw.rs:draw_grid` (10 mm, ±20 steps on the model's XY, every 5th line major, red X, green Y and blue Z axes) | done-by-reading |
| "Build Plate Preview" (`view.build_plate`: 220 × 220 mm plate; turns overhang shading on) | ui/app.py:316, ui/app.py:1072-1077, ui/viewport.py:622-633 | n/a (display) | Ctrl+Shift+B, Print > Build Plate Preview, the Plate chip → `cad/display/draw.rs:quads` (220 × 220 mm) and `cad/display/section.rs:overhangs` (45°, built by `preview` on `Pool::Compute`) | done-by-reading |
| "Section Analysis" (`view.section`; Section tool: clip plane, drag along its normal, Tab offset, R rotates 90° about Z) | ui/app.py:315, ui/app.py:1065-1070, ui/tools.py:1158-1207, ui/viewport.py:538-542 | n/a (display) | Ctrl+Shift+X, Inspect > Section Analysis, the Section chip → `cad/display/mod.rs:apply_section` (toggle; starts on XZ through the bodies' centre, `default_plane`); the toolbar's X, Y, Z chips (through the bodies' centre) and Rotate (`SectionPlane::rotated`); the offset along the normal: the display toolbar's offset field (`cad/display/entry.rs:input`, `offset_action`: a length expression such as "5" or "2 cm"; Enter writes `CadSection {offset}`) and REST `cad_section {"offset"}` | deliberately different: the offset is typed in the toolbar's field and the plane turned by the Rotate chip; R and Tab are not bound (R is the Rotate tool's key, Tab the numeric bar's), and dragging the plane is not bound (a left drag in the 3D view is the Select tool's box selection or the Alt orbit, and the plane quad covers the model) |
| Section outline from display triangles (never the kernel); picks respect the clip | ui/section_preview.py:6-55, ui/viewport.py:941-969, ui/viewport.py:1260-1261 | n/a (display); exact B-rep sections: `GET /nodes/{id}/section` | `cad/display/section.rs:preview` (clipped copies of the display triangles on `Pool::Compute`, in RoboCAD's triangle order, so a pick on the kept part names the right face and the removed part is never hit) and `segments` (the outline); exact: `exact_jobs` → `crates/sim-runtime/src/cad_client/section.rs:CadClient::section` (`system_ui` `cad:section:exact`, REST `cad_section {"exact"}`) | done-by-reading |
| "Isolate" (`view.isolate`) | ui/app.py:312, commands.py:402 | `POST /ops/isolate` | /, View > Isolate, the 3D view's right-click menu → `cad/ops/catalogue/view.rs:ENTRIES` (`view.isolate`: `POST /ops/isolate` on the selected nodes, one undo step) | deliberately different: refused by name with nothing selected ("Select the nodes to isolate"); RoboCAD's runs and hides everything |
| "Show All" (`view.show_all`) | ui/app.py:313, commands.py:415 | `POST /ops/show_all` | Alt+H, View > Show All → `cad/ops/catalogue/view.rs:ENTRIES` (`view.show_all`: `POST /ops/show_all`, one undo step) | done-by-reading |
| "Hide" (`view.hide`, the selection) | ui/app.py:314 | `POST /ops/set_visible` | H, View > Hide, the 3D view's right-click menu → `cad/ops/catalogue/view.rs:ENTRIES` (`view.hide`: `POST /ops/set_visible` with false on the selected nodes, one undo step) | deliberately different: refused by name with nothing selected ("Select the nodes to hide"); RoboCAD's pushes an empty undo step |
| Stress overlay (`view.stress` "Toggle stress overlay (from loaded results)" and `print.overlay` "Strength overlay on/off"; blue 0 → red at yield) | ui/app.py:399, ui/app.py:422, ui/app.py:1695-1698, ui/viewport.py:826-877 | `GET /results/nodes` (per-node `results.hotspot` and yield strength; added with cad-physical-inspect) | `cad/results/overlay.rs:paint`; `cad/results/mod.rs:handle` (`Overlay`, `PrintOverlay`); colours from `sim_domain_robot::stress_results::link_colours` | deliberately different: one colour rule shared with Robot mode (`link_colours`: log scale over 3 decades, blue at 0.1 % of yield → red at yield), where RoboCAD's window is linear from blue 0 to red at yield; `print.overlay` only toggles this overlay (the print study's failure-index colouring is cad-print's); the staleness label follows RoboCAD's stale flag |
| "Draft-angle shading" (`inspect.draft`, pull +Z) | ui/app.py:403, ui/app.py:1304-1317 | `GET /nodes/{id}/mesh` (derived natively) | none (`cad/surfaces/registry.rs` `DRAFT_SHADING`) | deliberately different: RoboCAD colours each selected body's triangles by draft against +Z (`analysis.draft_angle_colors`); no route serves those colours and the native meshes carry no per-triangle colours to draw them |
| "Normal-direction shading" (`inspect.normals`, which switches to xray) | ui/app.py:404, ui/app.py:1319-1322 | n/a (display) | `inspect.normals` (Inspect menu) → `DisplayCmd::Mode(Xray)` (`cad/surfaces/registry.rs`), as RoboCAD's `normal_shading` switches to xray | done-by-reading |
| Overlay: "tool · mode" and the tool hint; footer "Right-drag orbit · Shift+right-drag pan · Wheel zoom · F focus \| n ms/frame" | ui/viewport.py:1198-1223 | n/a (display) | "tool · mode" and the hint: `cad::numeric` head (`cad::transform::mode_label`, `cad::transform::hint`), at the bottom of the 3D view, with the navigation line (`cad::numeric::NAVIGATION`: this view's own orbit, pan, zoom and Home fit) | deliberately different: no ms/frame readout (frame timing belongs to the diagnostics overlay, see native-viewer.md "Bevy features to use"), and the navigation names this view's keys ("Right-drag orbit · Shift+right-drag or middle-drag pan · Wheel zoom · Home fit"; F focuses the selection since cad-views-export, `cad/surfaces/registry.rs:focus`, but the line does not list it) |
| Frame time, display triangle counts | ui/viewport.py:561-566, api.py:1222-1228 | `GET /performance` | the viewer's own frame statistics | deliberately different: the viewer measures its own frames; RoboCAD's numbers describe RoboCAD's window |

## Tools

One row per tool in `ui/tools.py`, plus the tool-launching registry
commands. Direct-edit commands driven by dialogs are under "Modify".

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Select tool (`tool.select`, "Select tool"; the click part is in "Selection") | ui/app.py:322, ui/tools.py:111-228 | `PUT /selection` | `cad::pick` (click, Shift, Ctrl, box, Alt menu, hover; see "Selection") and `cad::transform::dimensions::double_click`; `CadAction::CadTool { tool: Select }` | done-by-reading |
| Annotate (`tool.annotate`, N: click a surface, then write in Comments) | ui/app.py:278, ui/comments.py:98-125 | `POST /threads` | cad-organize epic | later-epic: cad-organize |
| Move (`tool.move`, G: gizmo axes, centre handle for screen-space, Ctrl snaps to the grid, Tab for dx dy dz) | ui/app.py:323, ui/tools.py:234-385, ui/viewport.py:1060-1132 | `POST /ops/transform {"translation"}` | `cad::transform::gizmo::drag` (axis handles; the centre handle on the screen plane; Ctrl 10 mm grid, `move_delta`) → one `CadAction::CadTransform` → `cad::transform::commit::transform_call`; Tab: dx dy dz in `cad::numeric` | done-by-reading |
| Rotate (`tool.rotate`, R: rings, Ctrl snaps 15°, Tab for the angle) | ui/app.py:324, ui/tools.py:311-322, ui/tools.py:360-362 | `POST /ops/transform {"axis", "angle_deg", "center"}` | `cad::transform::gizmo` rings (`rotate_angle`, Ctrl 15°) → `CadTransform { axis, angle_deg, center }`; Tab: the angle | deliberately different: the pivot for several selected nodes is the centre of their drawn-mesh bounds, not RoboCAD's mass-weighted centroid (`cad::transform::pivot`); a typed angle turns about the last dragged ring's axis, also after the X ring (RoboCAD's `axis_index or 2` turns about Z then) |
| Scale (`tool.scale`, S: Ctrl snaps 0.1, Tab for the factor) | ui/app.py:325, ui/tools.py:323-330, ui/tools.py:363-364 | `POST /ops/transform {"scale", "center"}` | `cad::transform::gizmo` (`scale_factor`, uniform, Ctrl ×0.1) → `CadTransform { scale, center }`; Tab: the factor | deliberately different: the pivot for several selected nodes is the centre of their drawn-mesh bounds, not RoboCAD's mass-weighted centroid (`cad::transform::pivot`) |
| Push/Pull face (`tool.push_pull`, D: drag along the normal; Shift offsets; Ctrl snaps to the grid; a non-planar face is offset) | ui/app.py:326, ui/tools.py:547-632 | `POST /ops/push_pull`, `POST /ops/offset_faces` | `cad::transform::push_pull::tool` (the face by ray cast, along its normal, Ctrl 10 mm steps; `release_action`: Shift or a non-planar face → `CadOffsetFaces`, else `CadPushPull`) → `cad::transform::commit::push_pull_call` / `offset_call` | done-by-reading |
| Offset face (`tool.offset_face`, Shift+D) | ui/app.py:327, ui/tools.py:551-553 | `POST /ops/offset_faces` | `CadAction::CadTool { tool: OffsetFace }` → `cad::transform::push_pull::release_action` → `CadOffsetFaces` → `cad::transform::commit::offset_call` | done-by-reading |
| Box (corner) (`tool.box`: drag the base, then the height; Tab width depth height; built as a sketch rectangle plus an extrude named "Box") | ui/app.py:328, ui/tools.py:391-541 | `POST /nodes {"kind": "box"}` or `POST /ops/box` | `cad::ops` catalogue `tool.box` (`Flow::Place`: `ops::invoke` makes it the active op with its form for exact sizes; `cad::ops::interact::pointer` snaps the press on the active plane (else XY), drags the base, then the height along the plane's normal; `interact::finish_params` writes `CadRun {id, params, revision}`; Tab writes the drag's first point into the form's anchor) → `args::build` (`place`: on XY one `POST /ops/box {"args": [corner, size]}`; on another plane one `POST /ops/box_three_point {"args": [a, b, c, height]}` spanning the same solid) | deliberately different: sent as one `Ops.box` on XY, else one `Ops.box_three_point` (history label "Box"; RoboCAD extrudes a sketch rectangle, label "Extrude", node "Box"; no route takes a client-built sketch body) |
| Box (centre) (`tool.box_center`) | ui/app.py:329, ui/tools.py:471-473 | `POST /ops/box_center` | `cad::ops` catalogue `tool.box_center` (`Flow::Place`, as `tool.box`, on the active plane, else XY) → `args::build` (`place`: the corner is the centre less half the width and depth in plane coordinates, the base on the plane, as RoboCAD's tool) → one `POST /ops/box` on XY, else `POST /ops/box_three_point` | deliberately different: sent as `Ops.box` or `Ops.box_three_point` like the corner box (RoboCAD's tool extrudes; `Ops.box_center`, which also centres the height, is the REST-only `ops.box_center`) |
| Cylinder (`tool.cylinder`; Tab diameter height) | ui/app.py:330, ui/tools.py:503-507 | `POST /ops/cylinder` | `cad::ops` catalogue `tool.cylinder` (`Flow::Place`: the press snapped on the active plane, else XY; the radius, then the height along the plane's normal; `interact::finish_params` → `CadRun`; Tab writes the drag's first point into the form's anchor) → `args::build` (`place`: the base projected onto the plane, axis ± its normal by the height's sign, radius at least 1e-3, as `_finish`) → one `POST /ops/cylinder` | done-by-reading |
| Sphere (`tool.sphere`; Tab diameter) | ui/app.py:331, ui/tools.py:508-510 | `POST /ops/sphere` | `cad::ops` catalogue `tool.sphere` (`Flow::Place`: the centre is the press snapped on the active plane, else XY; the release finishes it, as RoboCAD's `release`) → `args::build` (`place`: the centre as given, radius at least 1e-3) → one `POST /ops/sphere` | done-by-reading |
| Extrude (`tool.extrude`, X: the selected sketch or curve; drag the height, taper; Shift subtracts, Ctrl unites, Alt intersects with the body under the selection; preview mesh) | ui/app.py:332, ui/tools.py:822-931 | `POST /ops/extrude {"op", "target"}` | `cad::ops` catalogue `tool.extrude` (`Flow::Extrude`, X; form distance 10, taper 0, boolean) → `ops::invoke` → `sketch::extrude::begin` (`extrude::source`: RoboCAD's `activate` rule) → `extrude::pointer` (the press on the source's plane, `source_plane`; the drag through `transform::push_distance`, Ctrl 10 mm steps; the release's Shift/Ctrl/Alt → `boolean_for`) → one `CadRun {tool.extrude, {distance, taper, boolean}, revision}` → `ops::run` → `args::build` → `extrude::calls` (`body_under_selection`; "new" without a target) → `POST /ops/extrude {"args": [source, h, null, taper, false, op, target]}`; preview `extrude::draw` | deliberately different: the source follows the selection while the tool is active; the preview is outlines only and does not draw the taper (RoboCAD tessellates a preview body with its kernel, which no route serves). As RoboCAD's, the drag sends taper 0 (only the form's OK sends the typed taper) |
| Revolve (`tool.revolve`, Shift+R: about the sketch plane's x axis; Tab angle) | ui/app.py:333, ui/tools.py:916-918 | `POST /ops/revolve` | `cad::ops` catalogue `tool.revolve` (`Flow::Extrude { revolve: true }`, Shift+R; form angle 360, boolean) → `extrude::begin` → the form's OK → `extrude::calls` (`[source, plane.origin, plane.x_axis, angle or 360, op, target]`, the source sketch's plane, else the active plane) → `POST /ops/revolve` | done-by-reading: as RoboCAD's, a press and release in the view revolves 360° with the release's boolean (`extrude::pointer` writes one `tool.revolve` with angle 360; RoboCAD's `_apply` gets `angle=None`), and the form's OK revolves by the typed angle |
| Fillet (`tool.fillet`, Ctrl+F: click edges, type the radius) | ui/app.py:338, ui/tools.py:937-989 | `POST /ops/fillet` | `cad::ops` catalogue `tool.fillet` (`Flow::PickThenForm`: `ops::invoke` sets the selection mode, keeps the selection and opens the form beside the view, `cad::surfaces::form`; clicks toggle picks in `cad::pick::pointer`; Enter or OK → `CadFormSubmit` → `ops::submit`) → `ops::handle` → `ops::run` → `ops::prepare` (`CadDocument::commit_refusal`, `resolve::resolve`, `args::build`) → `ops::start` → `actions::edit` (one Dedicated edit job): `POST /ops/fillet {"args": [node, [{"node", "edge"}], radius]}`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD; the selection is cleared and the tool stays active | done-by-reading |
| Variable fillet (`tool.fillet_variable`: start and end radius) | ui/app.py:339, ui/tools.py:981-982 | `POST /ops/fillet {"radius_end"}` | `cad::ops` catalogue `tool.fillet_variable` (`Flow::PickThenForm`: `ops::invoke` sets the selection mode, keeps the selection and opens the form beside the view, `cad::surfaces::form`; clicks toggle picks in `cad::pick::pointer`; Enter or OK → `CadFormSubmit` → `ops::submit`) → `POST /ops/fillet` with `radius` and `radius_end`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | done-by-reading |
| Chordal fillet (`tool.fillet_chordal`) | ui/app.py:340, ui/tools.py:983-984 | `POST /ops/fillet_chordal` | `cad::ops` catalogue `tool.fillet_chordal` (`Flow::PickThenForm`: `ops::invoke` sets the selection mode, keeps the selection and opens the form beside the view, `cad::surfaces::form`; clicks toggle picks in `cad::pick::pointer`; Enter or OK → `CadFormSubmit` → `ops::submit`) → `POST /ops/fillet_chordal`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | done-by-reading |
| Chamfer (`tool.chamfer`, Ctrl+Shift+F: distance, and an angle unless it is 45°) | ui/app.py:344, ui/tools.py:985-986 | `POST /ops/chamfer` | `cad::ops` catalogue `tool.chamfer` (`Flow::PickThenForm`: `ops::invoke` sets the selection mode, keeps the selection and opens the form beside the view, `cad::surfaces::form`; clicks toggle picks in `cad::pick::pointer`; Enter or OK → `CadFormSubmit` → `ops::submit`) → `args::build` (`Shape::Chamfer`: `{"distance"}`, plus `angle_deg` only when it is not 45°) → `POST /ops/chamfer`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | done-by-reading |
| Hollow / shell (`tool.shell`, Ctrl+Shift+H: pick the faces to open, type the wall) | ui/app.py:345, ui/tools.py:992-1021 | `POST /ops/shell` | `cad::ops` catalogue `tool.shell` (`Flow::PickThenForm`: `ops::invoke` sets the selection mode, keeps the selection and opens the form beside the view, `cad::surfaces::form`; clicks toggle picks in `cad::pick::pointer`; Enter or OK → `CadFormSubmit` → `ops::submit`) in face mode → `POST /ops/shell {"args": [node, wall, [faces]]}`, one call per selected node with its selected faces (`Needs::NodesWithFaces`) | deliberately different: a face click toggles the face: RoboCAD's `ShellTool.press` reuses `EdgeTool.press`, whose `hit[0] == "edge"` test makes a face click toggle nothing there, while the tool's own hint says "click adds" (`cad::pick` module doc) |
| Measure (`tool.measure`, M: two picks; distance, angle or radius; the value is copied to the clipboard; Shift+click keeps it as a measure node) | ui/app.py:349, ui/tools.py:1027-1062, ui/app.py:715-740 | `POST /ops/add_measurement` (kept measurements); `GET /nodes/{id}/faces`, `/edges` | `cad::measure::tool` (two picks, `cad::measure::between`) → `CadAction::CadMeasure { keep: Shift }` → `cad::transform::commit::measure` (kept: one `POST /ops/add_measurement`) | deliberately different: the value is not copied to the clipboard (it shows in the status line and the tool bar, and REST `cad_measure` answers it); the same circular edge picked twice gives its radius (RoboCAD's branch is unreachable); the label is in the tool bar, not 3D text |
| Plane from face (`tool.plane`, Ctrl+P) | ui/app.py:350, ui/tools.py:1065-1099 | `POST /ops/plane_from_face` | `cad::ops` catalogue `tool.plane` (`Flow::PlanePick(Face)`, Ctrl+P) → `sketch::plane::begin` (face mode) → `plane::picks` (`ray_hit`, then `CadMeshes::face_at` at the shown revision) → `plane::run_for` → `CadRun {items, revision}` → `ops::run` → `args::build` → `POST /ops/plane_from_face {"args": [node, face]}`; `activates_plane`: `sync::finish_edit` sets `ops.plane_created` and `plane::sync` makes the new node the active plane ("Active plane set"); the tool stays active | done-by-reading |
| Plane from three points (`tool.plane_three`) | ui/app.py:351, ui/tools.py:1100-1106 | `POST /ops/plane_three_points` | `cad::ops` catalogue `tool.plane_three` (`Flow::PlanePick(Three)`: vertex mode; `plane::picks` takes three snaps, `snap::snap_on` over `snap::drawn_candidates`, on the active plane while 2D snapping is on) → `plane::run_for` (params a, b, c) → `POST /ops/plane_three_points`; the new plane becomes active as `tool.plane` | done-by-reading |
| Plane from two points (camera) (`tool.plane_camera`) | ui/app.py:352, ui/tools.py:1107-1111 | `POST /ops/plane_two_points_camera` | `cad::ops` catalogue `tool.plane_camera` (`Flow::PlanePick(Camera)`: two snaps as `tool.plane_three`; `Arg::ViewDir`, the view's direction at the run) → `POST /ops/plane_two_points_camera`; the new plane becomes active | done-by-reading |
| Midplane between two faces (`tool.plane_mid`) | ui/app.py:353, ui/tools.py:1096-1099 | `POST /ops/plane_midplane` | `cad::ops` catalogue `tool.plane_mid` (`Flow::PlanePick(Mid)`: two face picks as `tool.plane`; `Arg::Node`, `Arg::Face`, `Arg::FaceB`) → `POST /ops/plane_midplane`; the new plane becomes active | done-by-reading |
| Active plane XY / XZ / YZ (`tool.plane_xy`, `tool.plane_xz`, `tool.plane_yz`; "Active plane set") | ui/app.py:354-356, ui/app.py:1022-1027 | n/a (viewer state; RoboCAD's `PUT /view {"active_plane"}` is GUI-only) | `cad::ops` catalogue `tool.plane_xy`, `tool.plane_xz`, `tool.plane_yz` (`Flow::View(ViewAct::Plane)`) → `sketch::plane::view_act` sets `CadActivePlane` (status "Active plane set"); `cad_state.plane` (`plane::state_json`); every plane parameter's "active" (`Arg::Plane`) and the tools read it | deliberately different: also, selecting exactly one plane node makes it the active plane (`plane::sync`), a native gesture (RoboCAD's plane nodes become active only when a plane tool creates them); the active plane is the viewer's display state, never `PUT /view` |
| "Toggle 2D snapping to the active plane" (`tool.plane_2d_snap`) | ui/app.py:357, ui/app.py:1029-1031 | n/a (viewer state) | `cad::ops` catalogue `tool.plane_2d_snap` (`Flow::View(ViewAct::Snap2d)`) → `sketch::plane::view_act` toggles `CadActivePlane::snap_2d` ("2D snapping on\|off"); read through `snap::snap_plane` by measure, the cursor snap and the plane tools' points | done-by-reading |
| Fastener hole… (`tool.fastener`, Ctrl+H: dialog "Size" M2–M8, "Kind" clearance/tap/counterbore/countersink/insert, "Extra clearance (mm)", "Depth (mm)" or "through"; remembers the last; then click faces) | ui/app.py:358, ui/app.py:889-894, ui/widgets.py:988-1021, ui/tools.py:1120-1155 | `POST /ops/fastener_hole` | cad-print epic | later-epic: cad-print |
| Mirror (about active plane) (`tool.mirror`, Ctrl+M; YZ when no plane is active) | ui/app.py:360, ui/app.py:910-914 | `POST /ops/mirror` | `cad::ops` catalogue `tool.mirror` (`Flow::Immediate`; Ctrl+M) → `ops::handle` → `ops::run` → `ops::prepare` (`CadDocument::commit_refusal`, `resolve::resolve`, `args::build`) → `ops::start` → `actions::edit` (one Dedicated edit job): one `POST /ops/mirror {"args": [ids, plane], "kwargs": {"live": false}}`; `plane` defaults to "active" (`Arg::Plane("plane", Yz)`: the active plane by name or plane node id, else YZ as RoboCAD); REST `cad_run` may name xy, xz or yz | done-by-reading |
| Mirror as live instance (`tool.mirror_live`) | ui/app.py:361 | `POST /ops/mirror {"live": true}` | `cad::ops` catalogue `tool.mirror_live` → one `POST /ops/mirror {"args": [ids, plane], "kwargs": {"live": true}}`; `plane` as `tool.mirror` (the active plane, else YZ) | done-by-reading |
| Array… (`tool.array`, Ctrl+Shift+A: rectangular, count X/Y/Z, "count + spacing" or "count + total extent", or radial about the active plane; "As live instances"; "Merge into one body") | ui/app.py:365, ui/app.py:920-941, ui/widgets.py:1024-1063 | `POST /ops/array_rect`, `POST /ops/array_radial` | `cad::ops` catalogue `tool.array` (`Flow::Form`, `Shape::Array`: the ArrayDialog's fields in `cad::surfaces::form`, the rectangular or radial rows shown by `Param::when`) → `args::build` (`array`) → one `POST /ops/array_rect` (count X/Y/Z with `spacing` or `extent` by the mode) or `POST /ops/array_radial` (count, total angle, about the plane's normal through its origin: `plane_frame`, "active" by default: the active plane, else XY as RoboCAD), with `as_instances` and `merge` | done-by-reading |
| Instance selected (`tool.instance`: offset +20 mm in X) | ui/app.py:364, ui/app.py:916-918 | `POST /ops/instance` | `cad::ops` catalogue `tool.instance` (`Flow::Immediate`) → `POST /ops/instance {"args": [node, {"translation": [20, 0, 0]}]}`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | deliberately different: refused by name ("Select the bodies to instance") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Set pivot at cursor snap" (`tool.set_pivot`) | ui/app.py:376, ui/app.py:1015-1020 | `POST /ops/set_pivot`; `PATCH /nodes/{id} {"pivot"}` | `cad::ops` catalogue `tool.set_pivot` (`Flow::AtCursorSnap`) → `ops::handle` → `ops::run` → `ops::prepare` (`CadDocument::commit_refusal`, `resolve::resolve`, `args::build`) → `ops::start` → `actions::edit` (one Dedicated edit job): `POST /ops/set_pivot {"args": [first node, point]}` with `Arg::CursorSnap`, the snap `cad::ops::interact::pointer` keeps in `OpsState::cursor_snap` while the pointer is over the 3D view, with the shown revision it was snapped at (used only at that revision; cleared when the pointer leaves the window), or REST's `point` | deliberately different: the snap falls back to the first surface under the pointer (RoboCAD's `viewport.snap` has none), so a pivot can be set on a face; an empty selection is refused by name ("Select the node whose pivot to set") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| Image calibrate tool (two clicks on the image, type the real distance) | ui/tools.py:1210-1241 | `POST /ops/calibrate_reference` | cad-organize epic | later-epic: cad-organize |
| Motor tool (click a face: housing outside, shaft into the body) | ui/tools.py:1244-1291 | `POST /ops/add_motor` | `cad/robot/tools.rs:motor_pick` (the face from `cad/robot/tools_click.rs:click` → `CadMeshes::face_at` at the shown revision) → `cad/ops/mod.rs:run_entry` | deliberately different: the "Add motor" dialog's fields stay beside the view while you click (one step; Escape ends both) |
| Joint tool (parent, Ctrl-click for the world; child; an axis face) | ui/tools.py:1294-1361 | `POST /ops/add_joint` | `cad/robot/tools.rs:joint_pick` → `cad/ops/robot_form.rs:open_preset` ("Add joint" with the picks) | done-by-reading |
| Snapping: vertices, edge midpoints, centres, sketch endpoints, grid, plane, free; Alt suppresses; readout "kind (x, y, z)" | ui/viewport.py:1369-1435, ui/app.py:557-560 | `GET /nodes/{id}/vertices`, `GET /nodes/{id}/edges`, `GET /nodes/{id}` (sketch field) | `cad::snap::snap_on` (`candidates`: vertices, edge midpoints, edge centres; `sketch_candidates`: the visible sketches' endpoints; then the 10 mm grid in plane coordinates; then the plane or free; projected onto the active plane while 2D snapping is on or a sketch or placement tool passes its plane; Alt suppresses), readout `Snap::readout`; used by `cad::measure::tool`, the cursor snap, placement, the plane tools and the sketch tools | deliberately different: centre snaps work here (RoboCAD reads a `centers` attribute its items lack, so its never fire) |
| Gizmo drawing and hit testing | ui/viewport.py:1060-1132 | n/a (display) | `cad::transform::gizmo::draw` and `hit_test` (RoboCAD's 90 px handles; centre within 10 px, axes and rings within 14 px), CAD mode's own rather than Bevy's `TransformGizmoPlugin` (reasons in cad/transform/mod.rs) | done-by-reading |
| Tool cursors (arrow, size-all, crosshair) | ui/app.py:519-523 | n/a (display) | `cad::transform::tool_cursor` (arrow, move, crosshair over the 3D view; `restore_cursor` on leaving CAD mode) | done-by-reading |
| Tools toolbar (Select, Annotate, Saved Views, References, Pose, Experiments, Move, Rotate, Scale, Box, Cylinder, Sphere, Rectangle, Circle, Slot, Extrude, Push/Pull, Fillet, Shell, Union, Subtract, Fastener, Measure, Section, Validate; tools checkable) | ui/app.py:440-447, ui/app.py:527-529 | n/a (display) | `cad::surfaces::toolbar` (`registry::TOOLBAR`, RoboCAD's 25 entries in order, under the menu bar's tabs; each a kit chip with `CadButton(CadInvoke { id })` and `Enabled` from `registry::ready`; the `tool.*` and `sketch.*` entries lit while that tool or op is active, `toolbar::checkable` and `lit`; a hint under the hovered button names its keys and why it cannot run); Rectangle, Circle, Slot and Extrude are catalogue ops (cad-sketch) | deliberately different: the entries owned by later epics (Annotate, References, Pose, Experiments, Fastener, Validate) are shown disabled with the hint naming the epic (Saved Views and Section run since cad-views-export: `Do::SavedViews`, `DisplayCmd::Section`); Rectangle, Circle and Slot light while their sketch tool is active (RoboCAD checks only `tool.*`, app.py:445-446); the row scrolls sideways where Qt folds its overflow behind "»"; the hint stands in for Qt's tooltips (the kit has no tooltip widget) |
| Viewport right-click menu (Annotate, Comments panel, Push/Pull, Fillet, Chamfer, Shell, Union, Subtract, Mirror, Array, Measure, Isolate, Hide, Delete) | ui/app.py:1103-1107, ui/viewport.py:1526-1528 | the commands' routes | `cad::surfaces::context_menu` (a right press and release without a drag over the 3D view → `CadSurface { context { at } }`; `registry::CONTEXT`, RoboCAD's 14 entries in order, then a "Sketch" section of the 13 sketch tools in registry order, `registry::SKETCH_CONTEXT`; each enabled by `registry::ready`; a click → `CadInvoke`, then `CadSurface { closed }`) | deliberately different: Annotate and Comments panel (cad-organize) are shown disabled, naming their epic (Isolate and Hide run since cad-views-export: `cad/ops/catalogue/view.rs:ENTRIES`); the "Sketch" section is a native addition after RoboCAD's 14 (RoboCAD's menu has no sketch tools); the outliner's "Make unique (bake instance)" is added while an instance is selected |
| Double-click a face: its dimension goes into the numeric bar | ui/tools.py:185-196, ui/app.py:693-713 | `GET /nodes/{id}/faces`; `POST /ops/set_diameter`, `set_distance` | `cad::transform::dimensions::double_click` (`edit_at`: a cylinder's diameter, or a planar face's distance to the opposite face) → the focused `cad::numeric` field; face mode only | deliberately different: face mode only. In body mode the second click's pick (on release) re-selects the body, so a face entry would be overwritten; switch to face mode (Shift+B) first. RoboCAD picks a face temporarily in body mode (tools.py:185-196) |
| Escape cancels the tool and returns to Select | ui/app.py:487-497, ui/tools.py:103-105 | n/a | Escape → `CadAction::CadCancel` → `cad::transform::cancel` (`activate(Select)`) | done-by-reading |

## Sketch

RoboCAD has no sketch constraint solver (kernel/sketch.py has no
constraints), so no constraint rows exist. Sketch tools draw on the
**active plane**, into the selected sketch on that plane, else the first
visible one, else a new sketch (ui/tools.py:675-686). Natively every
finished shape and sketch edit is one `POST /nodes/{id}/sketch` (or one
`POST /nodes {"kind": "sketch"}` for a new sketch) through
`cad::ops::send_sketch`, which RoboCAD's undo history labels
"Sketch (API)" (api.py `Service.edit_sketch`) where its GUI labels the
shape ("Sketch rectangle", "Offset curves"): the tool rows below record
that difference.

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "Sketch: Line" (`sketch.line`, L; lines chain; Tab length and angle) | ui/app.py:377-378, ui/tools.py:638-817 | `POST /nodes/{id}/sketch {"calls": [["line", …]]}` | `cad::ops` catalogue `sketch.line` (`Flow::Sketch(Line)`, L) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at two points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor); lines chain: `interact::reset_after_finish` keeps the last point (`SketchSpec::chains`) | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", … |
| "Sketch: Rectangle" (`sketch.rectangle`, Shift+L; Tab width height) | ui/app.py:377-378, ui/tools.py:733-734 | `POST /nodes/{id}/sketch` (`rectangle`) | `cad::ops` catalogue `sketch.rectangle` (`Flow::Sketch(Rectangle)`, Shift+L) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at two points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor) | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", … |
| "Sketch: Rectangle (centre)" (`sketch.rectangle_center`) | ui/app.py:377-378, ui/tools.py:735-736 | `POST /nodes/{id}/sketch` (`rectangle_center`) | `cad::ops` catalogue `sketch.rectangle_center` (`Flow::Sketch(RectangleCenter)`) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at two points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor) | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", … |
| "Sketch: Circle" (`sketch.circle`, C; Tab diameter) | ui/app.py:377-378, ui/tools.py:737-738 | `POST /nodes/{id}/sketch` (`circle`) | `cad::ops` catalogue `sketch.circle` (`Flow::Sketch(Circle)`, C) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at two points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor); the diameter halved, as `commit` | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", … |
| "Sketch: Circle (two points)" (`sketch.circle_2pt`) | ui/app.py:377-378, ui/tools.py:739-740 | `POST /nodes/{id}/sketch` (`circle_two_point`) | `cad::ops` catalogue `sketch.circle_2pt` (`Flow::Sketch(Circle2pt)`) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at two points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor) | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", …; it has no Tab values, so the form's OK is refused by name (RoboCAD's `commit` records an empty undo step) |
| "Sketch: Circle (three points)" (`sketch.circle_3pt`) | ui/app.py:377-378, ui/tools.py:741-742 | `POST /nodes/{id}/sketch` (`circle_three_point`) | `cad::ops` catalogue `sketch.circle_3pt` (`Flow::Sketch(Circle3pt)`) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at three points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor) | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", …; no Tab values (OK refused by name); three collinear points are refused with the kernel's message before anything is sent (RoboCAD's kernel call fails with it) |
| "Sketch: Arc (three points)" (`sketch.arc_3pt`) | ui/app.py:377-378, ui/tools.py:743-744 | `POST /nodes/{id}/sketch` (`arc_three_point`) | `cad::ops` catalogue `sketch.arc_3pt` (`Flow::Sketch(Arc3pt)`, A (keymap.json's dead `sketch.arc`, bound here)) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at three points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor) | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", …; no Tab values (OK refused by name); collinear points refused before sending; A runs it (RoboCAD binds A to a command that does not exist) |
| "Sketch: Polygon" (`sketch.polygon`, Shift+P; remembers the side count; Tab radius and sides) | ui/app.py:377-378, ui/tools.py:745-746, ui/tools.py:668 | `POST /nodes/{id}/sketch` (`polygon`) | `cad::ops` catalogue `sketch.polygon` (`Flow::Sketch(Polygon)`, Shift+P) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at two points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor); the side count: `OpsState::polygon_sides` (6 at first; RoboCAD's `Sketch.last_polygon_sides`), set when a polygon sent with sides succeeds (`ops::send_sketch` → `specs::note_polygon_sides`; `sync::finish_edit` → `specs::polygon_edit_done`), the form's `sides` opens with it (`interact::begin`), and the preview and the clicked polygon use it | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", …; the sides field takes a whole count of at least 3 (RoboCAD `int()`s any number); a clicked polygon is sent with the viewer's remembered count (`sides`, what its preview draws) where RoboCAD's `_build` passes none and uses its process-wide `last_polygon_sides`, which other REST clients also move (kernel/sketch.py:259-263 records the sent count either way) |
| "Sketch: Slot" (`sketch.slot`, Shift+S; Tab length and width) | ui/app.py:377-378, ui/tools.py:747-749 | `POST /nodes/{id}/sketch` (`slot`) | `cad::ops` catalogue `sketch.slot` (`Flow::Sketch(Slot)`, Shift+S) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at three points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor); the width from the third click | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", … |
| "Sketch: Spline" (`sketch.spline`, Shift+C; Enter or double-click finishes) | ui/app.py:377-378, ui/tools.py:754-756, ui/tools.py:772-782 | `POST /nodes/{id}/sketch` (`spline`) | `cad::ops` catalogue `sketch.spline` (`Flow::Sketch(Spline)`, Shift+C) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at Enter (no field focused) or a double-click (400 ms and 5 px, Qt's defaults) with two or more points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor) | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", …; no Tab values (OK refused by name) |
| "Sketch: Ellipse" (`sketch.ellipse`; Tab radius x and radius y) | ui/app.py:377-378, ui/tools.py:750-753 | `POST /nodes/{id}/sketch` (`ellipse`) | `cad::ops` catalogue `sketch.ellipse` (`Flow::Sketch(Ellipse)`) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at three points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor) | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", … |
| "Sketch: Spiral" (`sketch.spiral`; Tab start radius, end radius, turns) | ui/app.py:377-378, ui/tools.py:757-758 | `POST /nodes/{id}/sketch` (`spiral`) | `cad::ops` catalogue `sketch.spiral` (`Flow::Sketch(Spiral)`) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at two points `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor); turns a plain number (`Unit::Plain`) | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", … |
| "Sketch: Text" (`sketch.text`, T; dialog "Text to sketch:"; Tab height) | ui/app.py:377-378, ui/app.py:743-750, ui/tools.py:759-760 | `POST /nodes/{id}/sketch` (`text`) | `cad::ops` catalogue `sketch.text` (`Flow::Sketch(Text)`, T) → `ops::invoke` (its Tab fields and `anchor` as the tool's form) → `sketch::interact::begin` → `sketch::interact::pointer` (each press `snap::snap_on` on the active plane, else XY, projected onto it; Alt suppresses) → at one point (height 10 mm, as RoboCAD's click) `interact::finish_action` (`sketch::specs::from_points`, RoboCAD's `_build`) → one `CadSketch {node: None, plane, calls, revision}` → `sketch::edits::sketch_action` → `specs::target` (RoboCAD's `_ensure_sketch`) → `ops::send_sketch` → `POST /nodes/{id}/sketch` or `POST /nodes {"kind": "sketch", "plane", "calls"}`; Tab values: the form's OK → `CadRun` → `args::build` → `specs::calls` (`specs::from_values`, RoboCAD's `commit`, at the anchor); "Text to sketch:" is the form's first field (`FieldKind::Text`), focused by `interact::begin`; Enter in it starts the clicks (`surfaces::form::input`), as RoboCAD's dialog OK | deliberately different: RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Sketch line", "Sketch rectangle", …; RoboCAD's `getText` dialog is the form's first field; empty text is refused by name (RoboCAD records an empty undo step) |
| Live preview of the shape and readout (length, angle, radius, size) | ui/tools.py:699-726 | n/a (display) | `cad::sketch::preview` (`preview_lines`: `specs::from_points` with the cursor appended → `preview_curves`, the kernel constructors' geometry, `SketchCurve::sample(48)`; `draw`, Present, `ToolGizmos` in (0.4, 0.9, 1.0)); readout `interact::readout` into `tool_state.readout` ("length L  angle A", "radius R", "W × H") | deliberately different: a text call previews as a placeholder box on its baseline (RoboCAD's outlines need fontTools; its text tool finishes on its one click, so its hover never shows text either) |
| Picks are projected onto the active plane while a sketch tool is active | ui/tools.py:651-660 | n/a (viewer state) | `cad::sketch::interact::pointer` (`snap::snap_on` with the active plane's frame, else XY, always: RoboCAD's `activate` turns `plane_snapping` on; the point projected with `PlaneFrame::project`, kept as `SketchState::cursor`) | deliberately different: RoboCAD's `deactivate` (ui/tools.py:658-660) sets `plane_snapping` False when a sketch tool ends, losing a 2D snap the user had turned on; the native tool never touches the user's 2D snap toggle (`CadActivePlane::snap_2d`), so it is as the user left it after the tool (`sketch::interact` module doc) |
| New sketch on a plane | commands.py:444 | `POST /nodes {"kind": "sketch", "plane", "calls"}`; `POST /ops/new_sketch` | `cad::sketch::specs::target` (RoboCAD's `_ensure_sketch`: the selected sketch on the plane, else the first visible one, else new) → `ops::send_sketch` → `CadClient::create_sketch` → one `POST /nodes {"kind": "sketch", "plane", "calls"}` (api.py `Service.create`: `Ops.new_sketch`, then `edit_sketch`) | deliberately different: the sketch is created with the first finished shape, not when the tool is activated (RoboCAD's `activate` → `new_sketch`, an undo step), so activating a tool never edits the document; the undo steps are the same pair, "Sketch" then the shape's |
| "Sketch: offset selected curve…" (`sketch.offset`; "Distance (mm):") | ui/app.py:379, ui/app.py:758-764 | `POST /nodes/{id}/sketch` (`offset`) | `cad::ops` catalogue `sketch.offset` (`Flow::Form`: "Distance (mm):" 1.0, −1000 to 1000, 3 decimals) → `args::build` → `sketch::edits::calls` (`selected_sketch`, RoboCAD's `_selected_sketch`; `offset(c, d)` for every curve, "Offset curves") → `ops::send_sketch` → one `POST /nodes/{id}/sketch` | deliberately different: the form opens before the sketch is checked ("Select a sketch" on OK; RoboCAD checks before its dialog); a sketch with no curves is refused by name (RoboCAD records an empty step); RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Offset curves" |
| "Sketch: fillet corner…" (`sketch.fillet`; "Radius (mm):"; every corner of closed polylines) | ui/app.py:380, ui/app.py:766-780 | `POST /nodes/{id}/sketch` (`fillet_corner`) | `cad::ops` catalogue `sketch.fillet` (`Flow::Form`: "Radius (mm):" 2.0, 0.01 to 1000) → `sketch::edits::calls` → `edits::fillet_plan` (every corner of every closed polyline from the last vertex down, through `edits::fillet_corner`, an exact simulation of kernel/sketch.py's `fillet_corner`, so the corners RoboCAD's GUI skips on `KernelError` are not sent) → one `POST /nodes/{id}/sketch` (`fillet_corner` calls) | deliberately different: no corner to round is refused by name (RoboCAD records an empty step); the form opens before the sketch is checked; RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Fillet corners" |
| "Sketch: join curves" (`sketch.join`) | ui/app.py:381, ui/app.py:782-785 | `POST /nodes/{id}/sketch` (`join`) | `cad::ops` catalogue `sketch.join` (`Flow::Immediate`) → `sketch::edits::calls` (`join` of every curve) → one `POST /nodes/{id}/sketch` | deliberately different: a sketch of one curve or none is refused by name, and no sketch at all says "Select a sketch" (RoboCAD records an empty undo step, and is silent without a sketch); RoboCAD's undo history labels the edit "Sketch (API)" (api.py `Service.edit_sketch`), where its GUI's is "Join curves" |
| Trim, split, extend, rebuild, unjoin, reverse, remove, insert and remove vertex (kernel only: USER_GUIDE.md:188-189 says they are in the Sketch menu; no menu item exists) | kernel/sketch.py:301-480, api.py:661-685 | `POST /nodes/{id}/sketch` (`trim`, `split_at`, `extend`, `rebuild`, `unjoin`, `reverse`, `remove`, `insert_vertex`, `remove_vertex`) | REST `cad_sketch {"node", "calls", "revision"}` (`CadAction::CadSketch` → `sketch::edits::sketch_action`: each call read by `SketchCall::from_json`, refused naming the call and argument, curve indices checked by `check_calls`) → `ops::send_sketch` → `CadClient::edit_sketch` | done-by-reading (REST only, as RoboCAD has no menu items for them; trim and extend with two curve indices work through REST since the api.py `Service.edit_sketch` fix, `cad/tests/test_api_sketch_calls.py`) |
| Constructors reachable only by REST: `polyline`, `control_curve`, `arc`, `arc_tangent`, `circle_tangent`, `rectangle_three_point` | kernel/sketch.py:162-259 | `POST /nodes/{id}/sketch` | REST `cad_sketch` (as the row above; `SketchCall::{Polyline, ControlCurve, Arc, ArcTangent, CircleTangent, RectangleThreePoint}`) → `ops::send_sketch` → `CadClient::edit_sketch` (or `create_sketch` without `node`) | done-by-reading (REST only, as RoboCAD; `circle_tangent` and `arc_tangent` work through REST since the api.py `Service.edit_sketch` fix) |
| "Sweep (profile + path from selection)" (`tool.sweep`; "Twist (degrees):") | ui/app.py:334, ui/app.py:801-809 | `POST /ops/sweep {"options": {"twist_deg"}}` | `cad::ops` catalogue `tool.sweep` (`Flow::Form`: "Twist (degrees):" 0.0, −3600 to 3600; the selected sketches and curves, at least two) → `args::build` → one `POST /ops/sweep {"args": [profile, path, {"twist_deg"}]}` (`Arg::Keyed`) | done-by-reading |
| "Pipe along selected curve…" (`tool.pipe`; "Diameter (mm):") | ui/app.py:335, ui/app.py:811-818 | `POST /ops/pipe` | `cad::ops` catalogue `tool.pipe` (`Flow::Form`: "Diameter (mm):" 4.0, 0.01 to 1000) → one `POST /ops/pipe {"args": [node, diameter]}` per selected sketch or curve (`Fan::PerNode`) | done-by-reading |
| "Loft selected sketches" (`tool.loft`) | ui/app.py:336, ui/app.py:820-824 | `POST /ops/loft` | `cad::ops` catalogue `tool.loft` (two or more sketches or curves, else "Select two or more sketches to loft") → one `POST /ops/loft {"args": [ids]}` | done-by-reading |
| "Fill / patch selected curve" (`tool.fill`) | ui/app.py:337, ui/app.py:826-830 | `POST /ops/fill` | `cad::ops` catalogue `tool.fill` (the first selected sketch or curve, else "Select a closed curve") → one `POST /ops/fill {"args": [node]}`; needs api.py's 2026-10-01 fix: `ArgConverter` read the node id given to `Ops.fill(edges: str \| Body)` as an edge reference (any parameter named `edges`), a 400 for every fill; a string now passes through when the annotation has no `EdgeRef` (cad/tests/test_api_fill_node.py) | done-by-reading |

## Modify and booleans

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "Union" (`modify.union`, Ctrl+U; the first selected is the target: "Select the target body first, then the tools") | ui/app.py:382, ui/app.py:787-793 | `POST /ops/boolean {"op": "union"}` | `cad::ops` catalogue `modify.union` (`Flow::Immediate`, `Needs::TargetThenTools`: refused "Select the target body first, then the tools" with fewer than two nodes) → `ops::handle` → `ops::run` → `ops::prepare` (`CadDocument::commit_refusal`, `resolve::resolve`, `args::build`) → `ops::start` → `actions::edit` (one Dedicated edit job): one `POST /ops/boolean {"args": [target, [tools], "union"]}` (RoboCAD's Composite "Union"), then the selection is cleared | done-by-reading |
| "Subtract" (`modify.subtract`, Ctrl+Shift+U) | ui/app.py:383 | `POST /ops/boolean {"op": "subtract"}` | `cad::ops` catalogue `modify.subtract` (`Flow::Immediate`, `Needs::TargetThenTools`: refused "Select the target body first, then the tools" with fewer than two nodes) → `ops::handle` → `ops::run` → `ops::prepare` (`CadDocument::commit_refusal`, `resolve::resolve`, `args::build`) → `ops::start` → `actions::edit` (one Dedicated edit job): one `POST /ops/boolean {"args": [target, [tools], "subtract"]}` (RoboCAD's Composite "Subtract"), then the selection is cleared | done-by-reading |
| "Intersect" (`modify.intersect`, Ctrl+Alt+U) | ui/app.py:384 | `POST /ops/boolean {"op": "intersect"}` | `cad::ops` catalogue `modify.intersect` (`Flow::Immediate`, `Needs::TargetThenTools`: refused "Select the target body first, then the tools" with fewer than two nodes) → `ops::handle` → `ops::run` → `ops::prepare` (`CadDocument::commit_refusal`, `resolve::resolve`, `args::build`) → `ops::start` → `actions::edit` (one Dedicated edit job): one `POST /ops/boolean {"args": [target, [tools], "intersect"]}` (RoboCAD's Composite "Intersect"), then the selection is cleared | done-by-reading |
| "Region (overlap as new body)" (`modify.region`) | ui/app.py:385, ui/app.py:795-799 | `POST /ops/region` | `cad::ops` catalogue `modify.region` (exactly two nodes, "Select exactly two bodies") → one `POST /ops/region {"args": [a, b]}` | done-by-reading |
| "Join" (`modify.join`, J) | ui/app.py:386 | `POST /ops/join` | `cad::ops` catalogue `modify.join` (key J) → one `POST /ops/join {"args": [ids]}` | deliberately different: fewer than two nodes are refused by name ("Select two or more bodies to join") where RoboCAD calls `Ops.join` unchecked |
| "Unjoin" (`modify.unjoin`, Shift+J) | ui/app.py:387 | `POST /ops/unjoin` | `cad::ops` catalogue `modify.unjoin` (Shift+J) → `POST /ops/unjoin`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | deliberately different: refused by name ("Select the bodies to unjoin") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Dissolve redundant topology" (`modify.dissolve`) | ui/app.py:388 | `POST /ops/dissolve` | `cad::ops` catalogue `modify.dissolve` → `POST /ops/dissolve`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | deliberately different: refused by name ("Select the bodies to dissolve") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Make instance unique" (`modify.make_unique`) | ui/app.py:389 | `POST /ops/make_unique` | `cad::ops` catalogue `modify.make_unique` (instances only, `Needs::Nodes` with kinds `instance`) → `POST /ops/make_unique`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | deliberately different: a selection with no instance is refused by name ("Select an instance to make unique") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Fillet all edges…" (`tool.fillet_all`; "Radius (mm):") | ui/app.py:341, ui/app.py:832-837 | `POST /ops/fillet_all` | `cad::ops` catalogue `tool.fillet_all` (`Flow::Form`: "Radius (mm):", 1.0, 0.01 to 100, as RoboCAD's dialog) → `POST /ops/fillet_all`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | deliberately different: the selection is checked before the form opens and an empty one is refused by name ("Select the bodies to fillet") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason (RoboCAD opens its dialog and then does nothing) |
| "Full round (two edges)" (`tool.full_round`) | ui/app.py:342, ui/app.py:839-846 | `POST /ops/full_round` | `cad::ops` catalogue `tool.full_round` (`Needs::Edges` two of one body: "Select two edges of the same body") → one `POST /ops/full_round {"args": [node, edge_a, edge_b]}` | done-by-reading |
| "Remove fillets (selected faces)" (`tool.remove_fillets`) | ui/app.py:343, ui/app.py:848-855 | `POST /ops/remove_fillets` | `cad::ops` catalogue `tool.remove_fillets` (the selected faces grouped by node) → `POST /ops/remove_fillets`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | deliberately different: refused by name ("Select the fillet faces to remove") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Thicken sheet…" (`tool.thicken`; "Thickness (mm):") | ui/app.py:346, ui/app.py:857-864 | `POST /ops/thicken` | `cad::ops` catalogue `tool.thicken` (`Flow::Form`: "Thickness (mm):", 2.0, 0.01 to 100; sheets only) → `POST /ops/thicken`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | done-by-reading |
| "Draft faces…" (`tool.draft`; "Angle (degrees):"; pull +Z; the active plane is neutral) | ui/app.py:347, ui/app.py:866-878 | `POST /ops/draft` | `cad::ops` catalogue `tool.draft` (`Flow::Form`: "Angle (degrees):", 2.0, -45 to 45; pull `[0, 0, 1]`; `neutral` defaults to "active": `Arg::Plane("neutral", Xy)`, the active plane, else XY as RoboCAD) → `POST /ops/draft {"args": [node, [faces], [0, 0, 1], angle, neutral]}`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD | done-by-reading |
| "Delete faces (heal)" (`tool.delete_face`) | ui/app.py:348, ui/app.py:880-887 | `POST /ops/delete_faces` | `cad::ops` catalogue `tool.delete_face` (faces grouped by node) → `POST /ops/delete_faces`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD; the selection is cleared | deliberately different: refused by name ("Select the faces to delete") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Cut with active plane" (`tool.cut_plane`) | ui/app.py:366, ui/app.py:943-945 | `POST /ops/cut` (plane) | `cad::ops` catalogue `tool.cut_plane` → `POST /ops/cut {"args": [node, plane]}`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD; `plane` defaults to "active" (`Arg::Plane("plane", Xy)`: the active plane by name or plane node id, else XY as RoboCAD) | deliberately different: an empty selection is refused by name ("Select the bodies to cut") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Cut with selected sheet/curve" (`tool.cut_sheet`) | ui/app.py:367, ui/app.py:947-951 | `POST /ops/cut` (cutter id) | `cad::ops` catalogue `tool.cut_sheet` (two nodes: "Select the body, then the cutter") → one `POST /ops/cut {"args": [body, cutter id]}`; RoboCAD's `ArgConverter` now passes a non-plane node id through as the cutter (api.py, 2026-10-01; `cad/tests/test_api_cut_cutter.py`) where it read every `plane`-named argument as a plane name | done-by-reading |
| "Split faces with active plane" (`tool.split_face`) | ui/app.py:368, ui/app.py:953-955 | `POST /ops/split_face` | `cad::ops` catalogue `tool.split_face` → `POST /ops/split_face {"args": [node, plane]}`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD; `plane` defaults to "active" (`Arg::Plane("plane", Xy)`: the active plane by name or plane node id, else XY as RoboCAD) | deliberately different: an empty selection is refused by name ("Select the bodies whose faces to split") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Imprint selected curve/body" (`tool.imprint`) | ui/app.py:369, ui/app.py:957-961 | `POST /ops/imprint` | `cad::ops` catalogue `tool.imprint` (two nodes: "Select the body, then the tool") → one `POST /ops/imprint {"args": [body, tool]}` | done-by-reading |
| "Project curve onto body" (`tool.project_curve`, along the view direction) | ui/app.py:370, ui/app.py:963-968 | `POST /ops/project_curve` | `cad::ops` catalogue `tool.project_curve` (two nodes: "Select the curve/sketch, then the body") → one `POST /ops/project_curve {"args": [curve, body, direction]}` with `Arg::ViewDir`, the native camera's view direction (`-view_back`, RoboCAD's `-camera.basis()[2]`), or REST's `direction` | done-by-reading |
| "Silhouette onto active plane" (`tool.silhouette`) | ui/app.py:371, ui/app.py:970-972 | `POST /ops/silhouette` | `cad::ops` catalogue `tool.silhouette` → `POST /ops/silhouette {"args": [node, plane]}`, one call per node in RoboCAD's order inside that one job (`Fan::PerNode`), each its own RoboCAD undo step as in RoboCAD; `plane` defaults to "active" (`Arg::Plane("plane", Xy)`: the active plane by name or plane node id, else XY as RoboCAD) | deliberately different: an empty selection is refused by name ("Select the bodies to project") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Show/edit control points (advanced)" (`tool.control_points`: shows the poles; editing is script-only) | ui/app.py:372, ui/app.py:974-984 | `GET /nodes/{id}/control_points?face=i` (api.py `Service.control_points`, 2026-10-01; read-only); writing: `POST /ops/set_control_points` | `cad::ops` catalogue `tool.control_points` (`Shape::ControlPoints`, the first selected face) → `args::build` (`Read::ControlPoints`) → `analysis_overlay::start` (a Dedicated job, `CadClient::control_points`) → `analysis_overlay::receive` → `draw` (points and rows in RoboCAD's colours, display only; status "N control points (edit via Ops.set_control_points; …)"); editing: the REST-only `ops.set_control_points` (a JSON grid) | done-by-reading |
| "Raise face degree" (`tool.raise_degree`, to 4 × 4) | ui/app.py:373, ui/app.py:986-991 | `POST /ops/raise_degree` | `cad::ops` catalogue `tool.raise_degree` (the first selected face, du = dv = 4) → one `POST /ops/raise_degree {"args": [node, {"node", "face"}, 4, 4]}` | deliberately different: a selection without a face is refused by name ("Select a face to raise its degree") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Rebuild face…" (`tool.rebuild_face`; "Spans per direction:") | ui/app.py:374, ui/app.py:993-1001 | `POST /ops/rebuild_face` | `cad::ops` catalogue `tool.rebuild_face` (`Flow::Form`: "Spans per direction:", 4, 1 to 64) → one `POST /ops/rebuild_face {"args": [node, face, n, n]}` | deliberately different: the face is checked before the form opens and its absence is refused by name ("Select a face to rebuild") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| "Dependent offset (face to body)…" (`tool.dependent_offset`; "Clearance (mm):") | ui/app.py:375, ui/app.py:1003-1013 | `POST /ops/offset_face_to` | `cad::ops` catalogue `tool.dependent_offset` (`Flow::Form`: "Clearance (mm):", 0.2, -10 to 10; `Needs::FaceThenNode`: "Select a face, then the body to offset it to") → one `POST /ops/offset_face_to {"args": [node, face, target, clearance]}` | done-by-reading |
| "Curvature comb on selected curve" (`inspect.curvature`) | ui/app.py:401, ui/app.py:1277-1284 | `GET /nodes/{id}/curvature_comb` (api.py `Service.curvature_comb`, 2026-10-01; scale 5 and 48 samples as the GUI; read-only) | `cad::ops` catalogue `inspect.curvature` (`Shape::CurvatureComb`, the last selected node of kind curve, as RoboCAD's loop leaves the last curve's comb drawn: a sketch has no body and draws nothing there) → `args::build` (`Read::CurvatureComb`) → `analysis_overlay::start` (a Dedicated job, `CadClient::curvature_comb`) → `draw` (RoboCAD's lines and colour, display only; status "Curvature comb: N line(s)", a viewer addition) | deliberately different: a selection with no curve is refused by name ("Select a curve: a sketch has no body to comb…") where RoboCAD silently draws nothing, so a menu entry, key or REST call is never ignored without a reason; the status line is the viewer's |
| "Continuity check (G0/G1/G2)" (`inspect.continuity`; coloured edges; "Continuity: counts") | ui/app.py:402, ui/app.py:1286-1302 | `GET /nodes/{id}/continuity` (api.py `Service.continuity`, 2026-10-01; read-only) | `cad::ops` catalogue `inspect.continuity` (`Shape::Continuity`, the last selected node with a body (a body kind or a curve), as RoboCAD's loop leaves the last one drawn) → `args::build` (`Read::Continuity`) → `analysis_overlay::start` (`CadClient::continuity`) → `draw` (each edge's 16-point polyline in RoboCAD's G0/G1/G2/boundary colours; status "Continuity: {…}" as `analysis_overlay::counts_text`) | deliberately different: a selection with no body is refused by name ("Select a body: …") where RoboCAD silently does nothing, so a menu entry, key or REST call is never ignored without a reason |
| REST-only direct edits with no GUI: `move_faces`, `rotate_faces`, `set_radius`, `untrim`, `array_curve`, `box_three_point`, `bridge`, `extract_components` | commands.py:525, 528, 531, 559, 743, 426, 511, 826 | `POST /ops/{name}` | `cad::ops` catalogue `ops.move_faces`, `ops.rotate_faces`, `ops.set_radius`, `ops.untrim`, `ops.array_curve`, `ops.box_three_point`, `ops.bridge`, `ops.extract_components` (`expected_revision` is the shown revision) through REST `cad_invoke` (their parameter form) and `cad_run` → `ops::handle` → `ops::run` → `ops::prepare` (`CadDocument::commit_refusal`, `resolve::resolve`, `args::build`) → `ops::start` → `actions::edit` (one Dedicated edit job) | done-by-reading (REST only, with the form; not in the menus or palette, as RoboCAD has no GUI for them) |

## Numeric entry

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| The numeric bar: one field per dimension of the active tool, with the hint "Tab: type an exact value • Enter: confirm • Esc: cancel" | ui/widgets.py:142-176, ui/app.py:169-175, ui/strings.py:18 | n/a (display) | `cad::numeric` (fields from `cad::transform::fields`; hint `cad::numeric::ENTRY_HINT`) | done-by-reading |
| Tab focuses the first field ("Numeric entry (Tab)", `numeric.entry`); Tab is routed by hand because Qt's focus chain takes it | ui/app.py:429, ui/app.py:468-469, ui/app.py:483-486 | n/a | `cad::numeric::entry` (Tab opens the entry on the first field; `CadInputFocus` keeps the CAD keys out) | done-by-reading |
| Enter commits, Escape cancels, Tab cycles fields | ui/widgets.py:200-216, ui/app.py:566-574 | the active tool's route | `cad::numeric::entry` (Enter → `CadAction::CadNumeric { values }` → `cad::transform::commit::numeric`; Escape cancels; Tab cycles) | done-by-reading |
| Unit-aware expressions (`20mm + 0.3`, `1in`, `pi*10`, `45deg`); bare numbers are mm or degrees; a red border marks a parse error | ui/widgets.py:183-198, units.py | n/a (the Rust port `sim_runtime::units::evaluate`; RoboCAD's `units.py` stays the reference) | `sim_runtime::units::evaluate` on each keystroke in `cad::numeric` ("= 20.3 mm", or the error naming the token with a red border) | done-by-reading |
| Live dimension edits: "Ø name", "Distance", "Angle", "Ø edge i" (a sphere or torus "R" is shown but says "use Scale about the centre") | ui/app.py:654-691, ui/widgets.py:715-721 | `POST /ops/set_diameter`, `set_distance`, `set_angle` | `cad::transform::dimensions::live` (Ø name, Distance, Angle, Ø edge i; R read-only with `ROUND_READ_ONLY`) → `CadSetDimension` → `cad::transform::commit::dimension_call` | done-by-reading |

## Radial menus

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "View radial menu" (`view.radial`, Space: Front, Top, Right, Iso, Ortho, Grid, Mode, Fit) | ui/app.py:318, ui/app.py:1095-1097 | n/a (display) | `cad::surfaces::radial` on `ui_kit::pie` (`registry::VIEW_RADIAL`: Front, Top, Right, Iso, Ortho, Grid, Mode, Fit), opened at the pointer by Space (`cad::keys` → `CadSurface { view_radial }`) or REST `cad_surface`; since cad-views-export every entry runs through the registry (Front, Top, Right, Iso: `CameraAction::View`; Ortho: `CameraAction::Projection`; Grid and Mode: `cad/display/mod.rs:apply_display`; Fit: `CadFit`) | done-by-reading |
| "Selection-mode radial menu" (`select.mode_radial`, Q: Body, Face, Edge, Vertex, Point) | ui/app.py:321, ui/app.py:1099-1101 | `PUT /selection {"mode"}` | `cad::surfaces::radial` (`registry::SELECT_RADIAL`: Body, Face, Edge, Vertex, Point → `CadInvoke { select.<mode> }` → `CadSelectMode`, pushed with `PUT /selection {"mode"}`), opened at the pointer by Q (`cad::keys`) or REST `cad_surface` | done-by-reading |
| The pie widget (opens at the cursor; hover highlights; release or click runs; Escape closes) | ui/widgets.py:821-882 | n/a (display) | `ui_kit::pie` (`Kit::pie`, `index_at`: the entry under the pointer's angle, none in the 18 px dead centre; `slot`: the first straight up, then clockwise) and `cad::surfaces::radial::input` (a press inside or a release runs the entry and closes the pie; a press outside or Escape closes it) | deliberately different: the entries are kit buttons of RoboCAD's 92 × 44 size with the kit's 5 px corner radius (rounded rectangles, not RoboCAD's ellipses), so hover, disabled and label styling are the kit's |

## Command palette

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| List RoboCAD's registry commands with category and keys, and run one | ui/widgets.py:52-136, ui/app.py:247-251 | `GET /commands`, `POST /commands/{id}` (GUI only) | `cad::panel` commands list → `CadAction::CadCommand` → `CadClient::run_command`; a headless 409 "no GUI" is shown verbatim | done-by-reading |
| "Command palette" (`command_palette`, Ctrl+Space or Shift+F): search "Type a command… (Ctrl+Space)" over id, label and category; ranked; first 60 | ui/app.py:282, ui/app.py:1109-1110, ui/widgets.py:79-108 | `GET /commands` | `cad::surfaces::palette` on `ui_kit::palette` (`PLACEHOLDER` "Type a command… (Ctrl+Space)", `rank` as RoboCAD ranks, the first 60) over `registry::COMMANDS` (RoboCAD's 183 commands), opened by Ctrl+Space or Shift+F (`cad::keys` → `CadSurface { palette }`) or REST `cad_surface`; Up/Down, Enter or a click → `CadInvoke { id }` | deliberately different: it lists RoboCAD's whole command table: a command that cannot run here is noted (its owning epic, or "not ported" with the ledger's reason) and disabled; on macOS Command+Space is Spotlight's, so Control+Space or Shift+F opens it (`cad::keys` clash table) |
| Key-conflict warning "⚠ conflicts with labels" | ui/widgets.py:72-77, ui/widgets.py:99-103 | `GET /commands` (`keys`) | `ui_kit::palette::conflicts` over the registry's keys (`cad::surfaces::palette::keys_of`; RoboCAD's `/commands` keys on a desktop RoboCAD, a user keymap included); RoboCAD's quirk kept: only the last conflicting key's warning shows | done-by-reading |
| Menus by category (File, Edit, View, Select, Create, Sketch, Modify, Planes, Inspect, Print, Advanced, Outliner, Robot, Bridge, Simulation, Help; "General" and "Window" fall into Help) | ui/app.py:433-439 | `GET /commands` (`category`) | `cad::surfaces::menus` (`menus::tabs`: one tab per `registry::CATEGORIES` in RoboCAD's order → `CadSurface { menu { category } }`; `registry::menu_of` puts General, Window and Tools in Help; a menu lists its commands with keys and enabled state) | done-by-reading |

## Print

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "Wall thickness check…" (`print.wall_check`, Ctrl+W; "Flag walls thinner than (mm):" 1.2; red points) | ui/app.py:390, ui/app.py:1113-1127 | `GET /nodes/{id}/thin?threshold=` | cad-print epic | later-epic: cad-print |
| "Validate for printing" (`print.validate`, Ctrl+Shift+V; "n body(ies): valid and watertight.") | ui/app.py:391, ui/app.py:1129-1135 | `GET /nodes/{id}/validate` | cad-print epic | later-epic: cad-print |
| "Toggle overhang shading" (`print.overhangs`; faces past 45° shaded red) | ui/app.py:392, ui/app.py:1079-1083, ui/viewport.py:293-294, ui/viewport.py:1675-1679 | `GET /nodes/{id}/mesh` (derived natively) | cad-print epic | later-epic: cad-print |
| "Split selected for printing…" (`print.split`: "Printer:" list with usable size, "Joints:" auto / pins+screws / dovetail / pins) | ui/app.py:393, ui/app.py:1166-1183 | `POST /print/split`; `GET /print/registry` | cad-print epic | later-epic: cad-print |
| "Check strength (document's print study)" (`print.strength`) | ui/app.py:394, ui/app.py:1185-1200 | `POST /print/analyze` | cad-print epic | later-epic: cad-print |
| "Plan print settings and plates (document's print study)" (`print.plan`) | ui/app.py:395, ui/app.py:1202-1211 | `POST /print/plan` | cad-print epic | later-epic: cad-print |
| "Whole or split for strength? (selected part of the print study)" (`print.strength_split`) | ui/app.py:396, ui/app.py:1213-1225 | `POST /print/strength_split` | cad-print epic | later-epic: cad-print |
| "Assembly guide for the selected split…" (`print.assembly`; opens the guide) | ui/app.py:397, ui/app.py:1227-1242 | `POST /print/assembly` | cad-print epic | later-epic: cad-print |
| "Test coupons (for the selected split, or the material)…" (`print.coupons`; "Printer:", "Filament:") | ui/app.py:398, ui/app.py:1244-1263 | `POST /print/coupons` | cad-print epic | later-epic: cad-print |
| "Print jobs…" (`print.jobs`: the last eight; "Cancel the running jobs?") | ui/app.py:400, ui/app.py:1265-1275 | `GET /print/jobs`, `DELETE /print/jobs/{id}` | cad-print epic | later-epic: cad-print |
| Job progress in the status bar ("kind: message (n %) — Print ▸ Print jobs… to cancel") | ui/app.py:1144-1164 | `GET /print/jobs/{id}` | cad-print epic | later-epic: cad-print |
| The print overlay ("print" results section in the stress colours) | ui/viewport.py:831-840 | `GET /results/nodes` (per-node `results`, print-study blocks included; added with cad-physical-inspect) | cad-print epic | later-epic: cad-print |
| "Clearance offset…" (`tool.clearance`, Ctrl+Shift+C; "Grow holes / shrink bosses by (mm):", remembered) | ui/app.py:359, ui/app.py:896-908 | `POST /ops/clearance` | cad-print epic | later-epic: cad-print |

## Robot: joints, motors, sensors, cables, battery, control, uncertainty

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Robot panel summary ("n bodies, n joints, n DoF, n motors, n sensors, n cables. Ground: …. Power: ….") | ui/widgets.py:1234-1291 | `GET /robot`; `GET /results` | `cad/robot/panel.rs:summary_line` (Robot section of the right dock; reads from `cad/robot/data.rs:sync`) | deliberately different: "(n s run)" is omitted (`GET /results/nodes` has no `duration_s`); "(stale: …)" is added when RoboCAD's stale flag is set |
| Robot panel tree "Links", "Joints", "Motors", "Sensors & cables" with "Detail" and "Margin" columns; click selects; double-click a joint edits it | ui/widgets.py:1246-1252, ui/widgets.py:1304-1366 | `GET /robot`; `GET /results` | `cad/robot/panel.rs:view`, `row`, `double_click` | deliberately different: the branch glyphs ⚙ ⚡ ◎ 〜 ▣ are not in IBM Plex Sans, so rows sit under headings without them; Detail and Margin are lines under the name, not columns, to fit the dock width |
| Margins (yield, bearing, screw, stall, Tg, mount Tg) | ui/widgets.py:1293-1302, physical.py:1030 | `GET /results/nodes` (`margins`: `results_margins` per node, print-study blocks left out; added with cad-physical-inspect) | `cad/robot/panel.rs:margin_text` | done-by-reading |
| Issues list "⛔"/"⚠" and "✓ robot is valid" | ui/widgets.py:1348-1355 | `GET /robot` (`issues`) | `cad/robot/panel.rs:view` | deliberately different: ⛔ and ⚠ are not in the font, so "Error:" and "Warning:" in the danger and warn colours; ✓ kept |
| Robot panel buttons (Add joint…, Add motor…, Joint from selection…, Infer joints, Assign motor…, Fix together, Toggle ground, Add sensor…, Add cable…, Battery / control…, Export sim…, Stress overlay, Load results…, Apply identification…) | ui/widgets.py:1255-1269 | the commands' routes below | `cad/robot/panel.rs:buttons` | done-by-reading |
| "Robot: add motor from library…" (`robot.add_motor`; "Add motor" dialog: Motor, Rotation about shaft, Mount on, "Cut mounting holes and pilot into the mounted body", Name, notes) | ui/app.py:408, ui/app.py:1526-1531, ui/widgets.py:1076-1129 | `POST /ops/add_motor`; `GET /motors` | the catalogue's `robot.add_motor` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`, `cad/ops/robot_form.rs`; motors from `GET /motors` only) | deliberately different: motors are listed in id order (`GET /motors` read into a `BTreeMap`) |
| "Robot: add joint (click parent, child, axis face)" (`robot.add_joint`) | ui/app.py:409, ui/app.py:1533-1538 | `POST /ops/add_joint` | the catalogue's `robot.add_joint` → the joint tool `cad/robot/tools.rs:joint_pick` (Ctrl+Shift+J) | done-by-reading |
| "Robot: joint from the two selected bodies…" (`robot.joint_dialog`; "Add joint" dialog: Type with hints, Parent "(world)", Child, Pivot (mm), Axis, limits (° or mm), Motor, Extra gear ratio, Damping, Name) | ui/app.py:410, ui/app.py:1540-1563, ui/widgets.py:1132-1231 | `POST /ops/add_joint`, `POST /ops/set_joint` | the catalogue's `robot.joint_dialog` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_form.rs`, `cad/ops/robot_args.rs`) | done-by-reading |
| "Robot: infer joints from coaxial holes and pins" (`robot.infer`) | ui/app.py:411, ui/app.py:1577-1581 | `POST /ops/infer_joints` | the catalogue's `robot.infer` (`cad/ops/catalogue/robot.rs`) | done-by-reading |
| "Robot: assign selected motor to a joint…" (`robot.assign_motor`; "Motor", "Joint", "Extra gear ratio") | ui/app.py:412, ui/app.py:1583-1615 | `POST /ops/attach_motor` | the catalogue's `robot.assign_motor` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_form.rs`) | done-by-reading |
| "Robot: fix selected bodies together (first is the parent)" (`robot.fixed`) | ui/app.py:413, ui/app.py:1617-1624 | `POST /ops/connect_fixed` | the catalogue's `robot.fixed` (`cad/ops/catalogue/robot.rs`) | done-by-reading |
| "Robot: toggle ground on selected bodies" (`robot.ground`) | ui/app.py:414, ui/app.py:1626-1633 | `POST /ops/set_ground` | the catalogue's `robot.ground` (`cad/ops/catalogue/robot.rs`) | done-by-reading |
| "Robot: validate" (`robot.validate`; "robot valid: …" or the "Robot validation" warning) | ui/app.py:415, ui/app.py:1635-1642 | `GET /robot` | `cad/robot/tools.rs:validate`, `verdict` | deliberately different: RoboCAD's warning box becomes the status line and the Robot panel's issue list |
| "Robot: motor library…" (`robot.motors`) | ui/app.py:416, ui/app.py:1644-1647 | `GET /motors` | `cad/robot/tools_library.rs:draw` | deliberately different: a floating panel instead of a message box |
| "Robot: add sensor (IMU, encoder, current, force)…" (`robot.add_sensor`; Kind, On body, Point (mm), Reads joint, Rate (Hz), Name) | ui/app.py:417, ui/app.py:1649-1657, ui/widgets.py:1369-1407 | `POST /sensors`; `POST /ops/add_sensor` | the catalogue's `robot.add_sensor` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`) | done-by-reading |
| "Robot: add cable between bodies…" (`robot.add_cable`; From/To body and point, Length "auto: 10 % slack", Mass "auto: 4 g per 100 mm", Name) | ui/app.py:418, ui/app.py:1659-1667, ui/widgets.py:1410-1457 | `POST /cables`; `POST /ops/add_cable` | the catalogue's `robot.add_cable` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`) | done-by-reading |
| "Robot: battery, control loop and uncertainty…" (`robot.power`; Battery cells, Chemistry, Capacity (Ah), Control period (s), Control latency (s), Target per joint (°), Dimension σ (mm), Friction σ (fraction)) | ui/app.py:419, ui/app.py:1669-1674, ui/widgets.py:1460-1523 | `PUT /battery`, `PUT /control`, `PUT /uncertainty`; `POST /ops/set_robot_setting` (no battery) | the catalogue's `robot.power` (`cad/ops/robot_form.rs:precheck`, `cad/ops/robot_args.rs` `power`) | deliberately different: the per-joint targets are one JSON field {name: deg}; joints left out keep their target and non-motion joints are refused; the dialog is refused until the description, battery, control and uncertainty reads are current (`cad/ops/robot_form.rs:precheck`) |
| "Robot: load simulation results…" (`robot.load_results`; defaults to `<name>.simresult.json`; turns the stress overlay on) | ui/app.py:420, ui/app.py:1676-1687 | `POST /results/load` | `cad/results/mod.rs:handle` (`Load`) and `cad/results/forms.rs` (path field, starting at `<stem>.simresult.json` even when it is missing, where RoboCAD falls back to the folder) | done-by-reading |
| "Robot: apply identified joint parameters…" (`robot.apply_identification`) | ui/app.py:421, ui/app.py:1689-1693 | `POST /identification/apply` | `cad/results/mod.rs:handle` (`Identify`) | done-by-reading |
| Actuator profiles (REST only) | api.py:1239-1243, commands.py:1138 | `GET/POST /actuator-profiles` | `cad/results/mod.rs:profiles` (`POST /actuator-profiles`; in the window through a JSON file field); the current profiles in `cad_state.results` | done-by-reading |

## Physical export and simulation link

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| The physical description for the inspector (nothing written) | api.py:1229-1238 | `GET /physical?flex=0` | `CadAction::CadPhysical` → `CadClient::physical(false)` | done-by-reading |
| "Simulation: export physical model (simrobot v4, with flexible links)…" (`sim.export_physical`; a background child process, "exporting … n s", queued while one runs, terminated on close) | ui/app.py:423, ui/app.py:1700-1703, ui/app.py:1711-1767 | `GET /physical?path=P&flex=1` (writes the file; in the GUI a child process derives it, api.py:1533-1546); the viewer sends `GET /physical?flex=1` without `path` and writes the file itself | `cad/results/export.rs:request`, `start`, `poll`, `cancel` (`crates/sim-runtime/src/cad_client/physical.rs:CadClient::physical_model` on a `Pool::Dedicated` job; written atomically by `write_model`) | deliberately different: a job runs `GET /physical?flex=1` and writes the file here instead of a RoboCAD child process; cancelling, or leaving CAD, drops it before the write (RoboCAD's request itself cannot be aborted); leaving CAD is refused while an export runs or is queued |
| "Simulation: export robot model…" (`sim.export`: the same model with the x–z planar hint) | ui/app.py:424, ui/app.py:1705-1709 | `GET /physical?flex=1&planar=1` (added with cad-physical-inspect: headless `export_physical_model(planar=Plane.xz())`, desktop `export_snapshot(planar=True)`; pytests `cad/tests/test_api_physical_routes.py`) | the same as the row above (`cad/results/export.rs:request`, `ExportKind::Simulation`) with `planar=1` | done-by-reading |
| "Simulation: live link (watch + run viewer)" (`sim.link`: re-exports `<name>.simrobot.json` on every save and runs `sim-spatial --robot`) | ui/app.py:425, ui/app.py:1769-1781, simbridge.py:215-265 | `POST /save` then `GET /physical?path=…&flex=0` | `cad/results/link.rs:toggle`, `saved`, `after_write`, `receive` | deliberately different: no process is started; on the first written export Robot mode opens in this window through `WindowAction::Switch`, and later saves re-export and bump the registry's Robot entry (same id); simbridge's watch-and-run stays for RoboCAD's own window |

## Experiments

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "Experiments" dock (`view.experiments`); intro "Edit CAD, system or controller, then run a captured experiment." | ui/app.py:275, ui/experiments.py:373-395 | n/a (display) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Editors "System · Rhai", "Controller · Rhai", "Parameters" (JSON: system, controller, settings, seed) | ui/experiments.py:396-406, ui/experiments.py:531-546 | `POST /experiments` (request body) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Run experiment" (and "Run captured experiment", `simulation.experiment`) | ui/app.py:276, ui/experiments.py:418-419, ui/experiments.py:587-594 | `POST /experiments` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Check system" (compile the system and open the controller contract; no samples) | ui/experiments.py:420-422, ui/experiments.py:596-602 | `POST /experiments {"preflight": true}` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Cancel run" | ui/experiments.py:423, ui/experiments.py:609-610 | `POST /experiments/{id}/cancel` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Runs" list ("★" for the baseline; label, state, evaluation, id, revision), status, progress, diagnostics | ui/experiments.py:407-416, ui/experiments.py:483-518 | `GET /experiments`, `GET /experiments/{id}` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Inspect / compare", "Set baseline" | ui/experiments.py:426-428, ui/experiments.py:612-625 | `GET /experiments/{id}/result`, `POST /experiments/{id}/compare` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Link Rhai file…" (watched every 500 ms; imports captured on each run) | ui/experiments.py:429-432, ui/experiments.py:663-684 | `POST /experiments` (sources) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Restore run inputs" | ui/experiments.py:433, ui/experiments.py:639-661 | `GET /experiments/{id}/inputs`; `PUT /system` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Rerun after edits (750 ms debounce)" | ui/experiments.py:434-435, ui/experiments.py:473-474, ui/experiments.py:686-693 | `POST /experiments` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Use sampled controller"; controller language "Rhai controller source" / "Captured process bundle · JSON"; interface "Position targets · rad · servo firmware" / "Driver duty · −1 to 1 · external feedback"; profile "Quick check · rigid · contact/noise off" / "Validation · contact/flex/noise · finer step" | ui/experiments.py:436-455 | `POST /experiments` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Components" tab: "Load registered Rust components"; ports, units and parameters of each | ui/experiments.py:456-463, ui/experiments.py:557-585 | `GET /experiments/catalogue` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Run review: summary, part and signal filters, trace plot (blue run, gold baseline), Play and slider, readout, metrics, flex arrows ×1/10/100/1000 | ui/experiments.py:23-98, ui/experiments.py:100-345 | `GET /experiments/{id}/result`, `/partial`, `/diagnostics` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Run review's captured-CAD replay viewport (`replay_matrices`, `replay_flex`, the captured document) | ui/experiments.py:109, ui/experiments.py:155-163, ui/experiments.py:290-301 | none: needs a Python route (`captured_document`, `replay_matrices` and `signals` run in Python; no route returns the captured geometry or the poses) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Inspect live component", "View captured source", "Frame replay" | ui/experiments.py:144-153, ui/experiments.py:251-277, ui/experiments.py:326-332 | `GET /experiments/{id}/sources` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Annotate sample" (a thread with run evidence) | ui/experiments.py:203-207, ui/experiments.py:346-356 | `POST /threads {"evidence"}` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Review design candidates…": list, change summary, "Run candidate", "Accept into CAD", "Discard candidate" | ui/experiments.py:412-414, ui/experiments.py:704-770 | `GET /candidates`, `GET /candidates/{id}`, `POST /candidates/{id}/experiments`, `POST /candidates/{id}/accept`, `DELETE /candidates/{id}` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Candidate review's viewport of the proposed design | ui/experiments.py:732-738 | none: needs a Python route (`candidates.document(id)` geometry is not served) | cad-experiments-motion epic | later-epic: cad-experiments-motion |

## Motion, pose and video

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "Pose" dock (`view.pose`); "Preview joint motion" (`robot.pose`) and "Enter pose mode" | ui/app.py:274, ui/app.py:277, ui/pose.py:15-133 | `GET /motion` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Pose kinematics (driver joints, transmissions, loop closure, "Closure residual") | ui/pose.py:111-133, ui/pose.py:205-218 | none: needs a Python route (`PoseModel` runs in the window; `/motion` answers 409 headless, api.py:300) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Joint chooser, limits text ("preview bounds; joint limits unset"), value and slider | ui/pose.py:33-46, ui/pose.py:135-203 | `POST /motion/seek` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Range arc and current-value marker on the model | ui/pose.py:153-180 | n/a (display) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Play pattern" / "Pause"; "s / cycle" | ui/pose.py:47-53, ui/pose.py:317-336 | `POST /motion/play`, `POST /motion/pause` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Focus mechanism"; "Show markers" | ui/pose.py:54-59, ui/pose.py:230-253 | `POST /motion/focus` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Patterns: chooser ("Unsaved pattern"), "Edit pattern JSON", "Make joint sweep", "Save pattern", "Delete pattern" | ui/pose.py:60-78, ui/pose.py:255-292 | `GET/POST/DELETE /motion/programs` (headless too) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Timeline scrub and clock "t / duration s" | ui/pose.py:79-82, ui/pose.py:294-315 | `POST /motion/seek` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| "Return to CAD pose" (and Escape) | ui/pose.py:98-100, ui/pose.py:343-354, ui/app.py:488-490 | `POST /motion/stop` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Video: 720p / 1080p, 24 / 30 / 60 fps, "Export MP4…", "Cancel export", progress | ui/pose.py:86-97, ui/pose.py:220-228, ui/motion_video.py:30-139 | `GET/POST/DELETE /motion/export` (GUI only) | cad-experiments-motion epic (the viewer records its own frames) | later-epic: cad-experiments-motion |

## Comments and threads

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "Comments" dock ("Comments panel", `view.comments`) and "＋ Annotate model" | ui/app.py:279, ui/comments.py:128-144 | n/a (display) | cad-organize epic | later-epic: cad-organize |
| Filter "Open" / "All" / "Resolved"; "Selected parts only" | ui/comments.py:145-152, ui/comments.py:249-263 | `GET /threads?node_id&status` | cad-organize epic | later-epic: cad-organize |
| Thread list "n · part" with a preview; "✓" when resolved | ui/comments.py:153-159, ui/comments.py:254-260 | `GET /threads` | cad-organize epic | later-epic: cad-organize |
| Attachment state ("Attached to surface", "Part deleted — reattach this annotation", "Geometry changed — check and reattach this pin", "Captured experiment") | ui/comments.py:283-287 | `GET /threads/{id}` (`anchor_status`) | cad-organize epic | later-epic: cad-organize |
| "Show on model" (restore the saved view, or open the run evidence) | ui/comments.py:164, ui/comments.py:370-390 | `POST /threads/{id}/show {"mode": "context"}` (GUI only); `GET /threads/{id}` (`view`) | cad-organize epic | later-epic: cad-organize |
| "Fit in view" (the thread's parts, at the current angle) | ui/comments.py:164-171, ui/comments.py:475-486 | `GET /threads/{id}` | cad-organize epic | later-epic: cad-organize |
| "Reattach…" | ui/comments.py:164, ui/comments.py:488-489 | `PATCH /threads/{id} {"node_id", "point", "face"}` | cad-organize epic | later-epic: cad-organize |
| "Resolve" / "Reopen" | ui/comments.py:172, ui/comments.py:491-495 | `PATCH /threads/{id} {"status"}` | cad-organize epic | later-epic: cad-organize |
| Linked parts list with labels; "Link selected parts", "Rename part label…" | ui/comments.py:174-190, ui/comments.py:442-458 | `PATCH /threads/{id} {"part_refs"}` | cad-organize epic | later-epic: cad-organize |
| "Show only linked parts" / "Return to assembly" (and Escape; temporary isolation that never changes visibility) | ui/comments.py:182-191, ui/comments.py:402-436, ui/app.py:480-482 | `POST /threads/{id}/show {"mode": "parts" or "highlight" or "back"}` (GUI only) | cad-organize epic | later-epic: cad-organize |
| Messages with clickable part links `[label](part:ID)`; "Insert part link from selection" | ui/comments.py:57-68, ui/comments.py:201-204, ui/comments.py:438-473 | `GET /threads/{id}` | cad-organize epic | later-epic: cad-organize |
| Author field ("You"); editor "Write a reply…"; "Reply" / "Post annotation" / "Save edit"; "Cancel" | ui/comments.py:197-219, ui/comments.py:326-368 | `POST /threads`, `POST /threads/{id}/comments`, `PATCH /comments/{id}` | cad-organize epic | later-epic: cad-organize |
| "Edit message", "Delete message", "Delete thread" | ui/comments.py:221-227, ui/comments.py:497-514 | `PATCH /comments/{id}`, `DELETE /comments/{id}`, `DELETE /threads/{id}` | cad-organize epic | later-epic: cad-organize |
| Numbered pins in the viewport (amber for review); click a pin to open its thread | ui/comments.py:516-540, ui/viewport.py:1453-1457 | `GET /threads` (`anchor.point`) | cad-organize epic | later-epic: cad-organize |
| "Toggle comment pins" (`view.comment_pins`) | ui/app.py:281, ui/app.py:531-533 | n/a (display) | cad-organize epic | later-epic: cad-organize |

## Components and system graph

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "Components library" dock (`components.show`; category "Window", which has no menu, so it lands in Help) | ui/app.py:362, ui/components.py:93-131 | `GET /components` | cad-organize epic | later-epic: cad-organize |
| Definitions "name · rN · n placed"; "Find a component…" | ui/components.py:102-105, ui/components.py:133-136, ui/components.py:184-197 | `GET /components` | cad-organize epic | later-epic: cad-organize |
| "Make from selection" (and "Make linked component…", `components.make`) | ui/app.py:363, ui/components.py:107, ui/components.py:224-226 | `POST /ops/make_component` (GUI: a job, `{"job"}`) | cad-organize epic | later-epic: cad-organize |
| "New parametric…" (box or cylinder) | ui/components.py:107, ui/components.py:228-230 | `POST /ops/new_parametric_component` | cad-organize epic | later-epic: cad-organize |
| "Place…" (Name, Origin, Rotation around Z, Variant, port bindings) | ui/components.py:109, ui/components.py:232-249 | `POST /ops/place_component` | cad-organize epic | later-epic: cad-organize |
| "Edit defaults…" (tabs Parameters with Name/Default/Unit/Min/Max/Provenance/Description; "Geometry and joints" JSON; "Nested parameters"; "Family variants") | ui/components.py:22-90, ui/components.py:251-255 | `POST /ops/set_component_parameters` | cad-organize epic | later-epic: cad-organize |
| "Import…", "Save to library…", saved library list, "Choose folder…", "Import selected" (`~/Documents/RoboCAD/Components`) | ui/components.py:111-115, ui/components.py:278-295 | `POST /ops/import_component`, `POST /ops/export_component` | cad-organize epic | later-epic: cad-organize |
| "Occurrence" tab: overrides table (Parameter, Current, Override, Value), "Occurrence origin (mm)", "Apply occurrence", "Reset to inherited", "Detach outer occurrence" | ui/components.py:117-124, ui/components.py:199-276 | `POST /ops/set_component_overrides`, `POST /ops/detach_component` | cad-organize epic | later-epic: cad-organize |
| Rebuild progress and "Cancel rebuild" | ui/components.py:125-182 | `GET/DELETE /component-jobs/{id}` (GUI only) | cad-organize epic | later-epic: cad-organize |
| System graph: type chooser and "New"; imports from a check ("Use existing", "Check") | ui/system_graph.py:114-141, ui/system_graph.py:207-258, ui/system_graph.py:352-361 | `GET /experiments/catalogue`, `GET /experiments/{id}/components` | cad-organize epic | later-epic: cad-organize |
| Component form: Name, CAD body, "Bind existing", "Specific heat · J/(kg·K)", "Fluid direction", "Attach to selected CAD body", parameters table, "+ Parameter", "Apply", "Remove" | ui/system_graph.py:143-175, ui/system_graph.py:377-412 | `GET /system`, `POST /system/components`, `PATCH /system/components/{id}`, `DELETE /system/components/{id}` | cad-organize epic | later-epic: cad-organize |
| "Geometry rule" choices and their derived outputs ("Derived from CAD") | ui/system_graph.py:154-155, ui/system_graph.py:414-429 | none: needs a Python route (`component_derivation.RECIPES` is not served) | cad-organize epic | later-epic: cad-organize |
| Connection graph: "Overview", "Focus selected", "−"/"+", click ports to connect, "Leave port open", "Remove connection" | ui/system_graph.py:12-112, ui/system_graph.py:179-196, ui/system_graph.py:431-459 | `POST /system/connections`, `DELETE /system/connections/{id}` | cad-organize epic | later-epic: cad-organize |

## References

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "References" dock (`view.references`) | ui/app.py:272, ui/references.py:12-78 | n/a (display) | cad-organize epic | later-epic: cad-organize |
| Linked system file line ("System file: none linked…", "System file missing", "System: title · revision n · n definitions", "CHANGED since linked") | ui/references.py:90-103 | `POST /ops/system_status` | cad-organize epic | later-epic: cad-organize |
| "Link system file…", "Accept changes", "Unlink" | ui/references.py:24-28, ui/references.py:105-117 | `POST /ops/link_system`, `refresh_system_link`, `unlink_system` | cad-organize epic | later-epic: cad-organize |
| "Open in builder" (starts `sim-spatial --system … --schematic`) | ui/references.py:119-130 | `POST /ops/system_status` (the path) | cad-organize epic (switch to the builder mode in this app) | later-epic: cad-organize |
| "＋ Add reference images…" (and `reference.import` "Add reference images…"); drop images on the panel | ui/app.py:273, ui/references.py:32-34, ui/references.py:170-184, ui/references.py:224-230 | `POST /ops/import_references` | cad-organize epic | later-epic: cad-organize |
| Image list with visibility checkboxes and a preview | ui/references.py:35-44, ui/references.py:132-168 | `PATCH /nodes/{id}`; `POST /ops/update_reference {"visible"}`; preview: none: needs a Python route (no image bytes; see "Viewport") | cad-organize epic | later-epic: cad-organize |
| Placement: Plane ("Keep current plane", "Front (XZ)", "Side (YZ)", "Top (XY)", "Active construction plane"), Width, Origin X/Y/Z, Rotation, Opacity, "Lock reference against selection", "Apply placement" | ui/references.py:45-63, ui/references.py:186-193 | `POST /ops/update_reference` | cad-organize epic | later-epic: cad-organize |
| "Align view", "Calibrate scale", "Sketch over this" | ui/references.py:64-69, ui/references.py:195-219 | `POST /ops/calibrate_reference` | cad-organize epic | later-epic: cad-organize |
| "Remove reference" | ui/references.py:70-72, ui/references.py:221-222 | `DELETE /nodes/{id}` | `CadAction::CadDelete` (the node); the panel is cad-organize | later-epic: cad-organize |

## Saved views

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "Saved Views" dock (`view.saved_views`) and its hint | ui/app.py:280, ui/saved_views.py:9-17 | n/a (display) | `view.saved_views` (View menu) → `CadViews` panel toggle → `cad/views/panel.rs:draw` (a floating kit panel over the 3D view's lower right) | deliberately different: a floating panel with a Restore view button on every row instead of a dock whose list restores on double-click; its hint says "Restore returns to one" |
| View name "View name, e.g. Worm drive cutaway" and "Save current view" | ui/saved_views.py:18-28, ui/saved_views.py:82-87 | `POST /views {"name", "state"}` (headless needs `state`) | `cad/views/panel.rs:body` (the name field, "View name, e.g. Worm drive cutaway", and "Save current view") → `cad/views/mod.rs:handle` (`ViewsOp::Save`: `cad/views/convert.rs:capture` of the camera and display, the name checked as RoboCAD's) → `crates/sim-runtime/src/cad_client/views.rs:CadClient::save_view` | done-by-reading |
| List "name / Orthographic or Perspective · Cutaway"; empty text | ui/saved_views.py:29-37, ui/saved_views.py:64-76 | `GET /views` | `cad/views/panel.rs:body` (name, then `cad/views/convert.rs:details`: "Orthographic" or "Perspective", " · Cutaway"; RoboCAD's empty text) from `cad/views/mod.rs:sync` (`GET /views` on a job at each revision) | done-by-reading |
| "Restore view" and double-click (camera, section, grid, pins, display mode) | ui/saved_views.py:33, ui/saved_views.py:89-96, saved_views.py:65-80 | `GET /views/{id}` (state); `POST /views/{id}/restore` moves only RoboCAD's own window | "Restore view" on each row → `cad/views/mod.rs:restore` (`CameraAction::Set`, a cut; `cad/views/convert.rs:apply_display`: grid, display mode, comment pins, section) from the listed state; no double-click (see the dock row) | done-by-reading |
| "Replace with current" | ui/saved_views.py:40, ui/saved_views.py:98-102 | `PATCH /views/{id} {"state"}` | `cad/views/mod.rs:handle` (`ViewsOp::Replace`: `cad/views/convert.rs:capture`) → `crates/sim-runtime/src/cad_client/views.rs:CadClient::update_view` (state only) | done-by-reading |
| "Rename…" ("Rename saved view" / "View name:") | ui/saved_views.py:41, ui/saved_views.py:104-109 | `PATCH /views/{id} {"name"}` | "Rename…" types into the row's own field (`cad/views/panel.rs:input`) → `ViewsOp::Rename` → `crates/sim-runtime/src/cad_client/views.rs:CadClient::update_view` (name only) | done-by-reading |
| "Delete" | ui/saved_views.py:41, ui/saved_views.py:111-115 | `DELETE /views/{id}` | `ViewsOp::Delete` → `crates/sim-runtime/src/cad_client/views.rs:CadClient::delete_view` | done-by-reading |
| Feedback "Saved inside this CAD file · edits support Undo" | ui/saved_views.py:50-52 | n/a (display) | `cad/views/panel.rs:body` (the feedback line; RoboCAD's text until an action reports) | done-by-reading |

## Export, import and drawings

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| "Import…" (`file.import`; STEP, IGES, STL, OBJ, 3MF, FBX, PLY, glTF, SVG onto the active plane, PNG/JPG as references) | ui/app.py:287, ui/app.py:1364-1390 | `POST /import {"path", "unit"}` | File > Import… and Ctrl+I → `cad/files/mod.rs:file` (`FileOp::Import`: `import_args` checks the extension against `IMPORT_EXTENSIONS`; one edit → `crates/sim-runtime/src/cad_client/files.rs:CadClient::import`) | deliberately different: everything goes through `POST /import`, which takes no plane, so an SVG and a PNG/JPG land on XY (`importers.import_svg`, `import_image`); RoboCAD's desktop puts them on the active plane, images through `Ops.import_references` |
| Mesh units dialog ("Units of the file" / "This format carries no unit. What are the numbers in?") with a guessed default | ui/widgets.py:897-914, ui/app.py:1378-1384 | `POST /import {"unit"}`; the guess: `GET /import/units?path=` (api.py `Service.mesh_units`, added with cad-views-export) | the import form's "Units of the mesh file" choice (mm, cm, m, in, ft): as soon as the path names a mesh, `cad/files/form.rs:input` asks RoboCAD's guess once per path (`FileForm::ask_guess`, which empties the unit) → `FileOp::GuessUnit` → `crates/sim-runtime/src/cad_client/files.rs:CadClient::mesh_units` on a job; `cad/files/jobs.rs:receive` → `FileForm::guessed` fills the unit unless it was chosen by hand; OK stays disabled, saying why (`FileForm::unit_missing`), until a guess landed or a unit was chosen; `cad/files/form.rs:footer` shows the guess and a "Guess unit" button that asks again | deliberately different: the unit is a row of the import path form, guessed once the path names a mesh, rather than a second dialog opened after RoboCAD has read the file (the path form opens before a file is named) |
| "Export…" (`file.export`; the last path is remembered) | ui/app.py:288, ui/app.py:1392-1397 | `POST /export {"format", "path", "settings", "ids"}` | File > Export… and Ctrl+E → `cad/files/mod.rs:export` (the export form when the format or path is missing, `open_form`) | deliberately different: the form starts in the document's directory with the document's name (`start_dir`), not at the last export path |
| STL: Format binary/ascii, Unit, Chord tolerance (mm), Angular tolerance (°) | ui/widgets.py:927-931, ui/app.py:1405-1410 | `POST /export {"format": "stl"}` | `cad/files/formats.rs:FORMATS` (`stl`: Format, Unit, Chord tolerance (mm), Angular tolerance (°), the dialog's ranges and defaults, checked by `settings`) → `cad/files/mod.rs:export` | done-by-reading |
| 3MF: Chord tolerance, "Write colours", "Write names" | ui/widgets.py:932-935, ui/app.py:1411-1416 | `POST /export {"format": "3mf"}` | `cad/files/formats.rs:FORMATS` (`3mf`: Chord tolerance, Write colours, Write names) → `cad/files/mod.rs:export` | done-by-reading |
| STEP: Schema AP203/AP214/AP242, "Write names", "Write colours" | ui/widgets.py:944-947, ui/app.py:1417-1423 | `POST /export {"format": "step"}` | `cad/files/formats.rs:FORMATS` (`step`: Schema AP203/AP214/AP242, Write names, Write colours) → `cad/files/mod.rs:export` | done-by-reading |
| IGES | ui/app.py:1424-1426 | `POST /export {"format": "iges"}` | `cad/files/formats.rs:FORMATS` (`iges`, no settings) → `cad/files/mod.rs:export` | done-by-reading |
| OBJ: Chord tolerance, Scale, Up axis Z/Y, "Quads where possible", "N-gons where possible", "Write MTL", "Write UVs" | ui/widgets.py:936-943, ui/app.py:1427-1432 | `POST /export {"format": "obj"}` | `cad/files/formats.rs:FORMATS` (`obj`: Chord tolerance, Scale, Up axis, Quads, N-gons, Write MTL, Write UVs) → `cad/files/mod.rs:export` | done-by-reading |
| Sketch SVG ("Select a sketch to export as SVG") | ui/app.py:1433-1438 | `POST /export {"format": "svg", "settings": {"sketch"}}` | `cad/files/formats.rs:FORMATS` (`svg`: the sketch node id, the selected sketch by default, `cad/files/mod.rs:export_context`) → `cad/files/mod.rs:export` | done-by-reading |
| Export options remembered per format (QSettings `export_settings`) | ui/app.py:78, ui/app.py:1403, ui/app.py:1441, ui/widgets.py:917-985 | n/a (viewer preferences) | `CadFiles::export_settings` (`cad/files/mod.rs:export` stores each format's settings; the form starts from them) | deliberately different: remembered per format for the CAD mode session, not kept between launches (RoboCAD keeps them in QSettings) |
| Export blocked by validation ("Export blocked") and "Exported: path (n warning(s))" | ui/app.py:1442-1444, ui/strings.py:21 | `POST /export` (422 with the reason; `warnings`) | `cad/files/mod.rs:export` (RoboCAD's 422 reason named through `cad/files/jobs.rs:named`; "Exported FORMAT to PATH (n warning(s): …)") | done-by-reading |
| "Export drawing (SVG)…" (`file.export_drawing`, Ctrl+Shift+D: front, top, right, iso, and "Section A-A" while the section is on) | ui/app.py:289, ui/app.py:1446-1456 | `POST /export {"format": "drawing", "settings": {"views", "section", "title"}}` | File > Export drawing (SVG)… and Ctrl+Shift+D → `cad/files/mod.rs:export` (`drawing`: views front, top, right, iso; the title; the section tool's plane while it is on, `export_context`) | done-by-reading |
| "Live link: start (Blender)" / "Live link: stop" (`bridge.start`, `bridge.stop`; websocket) | ui/app.py:405-406, ui/app.py:1497-1509 | none: needs a Python route (GUI: `POST /commands/bridge.start`) | none (`cad/surfaces/registry.rs` `BLENDER_LINK`) | deliberately different: RoboCAD-GUI-only: the Blender live link is a websocket server inside RoboCAD's desktop window; a headless service has no route for it and the native viewer has no Blender bridge |
| "Web share: publish viewer…" (`bridge.share`; one HTML file) | ui/app.py:407, ui/app.py:1511-1517 | none: needs a Python route (GUI: `POST /commands/bridge.share` opens a save dialog) | none (`cad/surfaces/registry.rs` `WEB_SHARE`) | deliberately different: RoboCAD-GUI-only: web share writes one HTML viewer through a desktop save dialog; no headless route serves it (export a mesh with File > Export… instead) |
| Software render of any view to PNG | api.py:830-894 | `GET /render?view&w&h&mode&section&ids&highlight&labels&edges&focus` | `cad/files/mod.rs:render` (REST `cad_render`, `system_ui` `cad:file:render` "Render (PNG)…": the render form, `render_request` checks the query, `crates/sim-runtime/src/cad_client/files.rs:CadClient::render` on a `Pool::Dedicated` job writes the PNG) | done-by-reading |

## REST routes

One row per route in `_route` (api.py:1098-1278) and the sub-routers it
calls. Who uses each route, by reading:

- **RoboCAD's UI** hosts every route but calls none of them. Its panels call
  `Ops` directly. Only its print commands share the service's
  `print_jobs` object (ui/app.py:1138-1142).
- **`robocad/client.py`** (`RoboClient`, used by scripts and agents) is
  marked "client".
- **`simbridge.py`**, **sim-cad** (`crates/sim-phenomena/src/bin/sim_cad.rs`,
  which works on files: simrobot.json, results, identification) and
  **`web/`** call no RoboCAD route.
- **cad-mode** (this epic) is marked where `CadClient` uses the route.

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| Health: ok, app, version, path, dirty, gui, nodes, document_id, revision (client: base URL) | api.py:1109-1110, api.py:387-388 | `GET /` | `CadClient::health` (`cad::sync` poll; `service::wait_until_live`) | done-by-reading |
| System graph and revision (client) | api.py:1112-1113, api.py:413-448 | `GET /system` | cad-organize epic | later-epic: cad-organize |
| Replace the system graph (expected_revision; client) | api.py:442-445 | `PUT /system` | cad-organize epic | later-epic: cad-organize |
| Read components or connections | api.py:423-425 | `GET /system/{components\|connections}[/{id}]` | cad-organize epic | later-epic: cad-organize |
| Add a system component (client) | api.py:432-433 | `POST /system/components` | cad-organize epic | later-epic: cad-organize |
| Update a system component (client) | api.py:434-435 | `PATCH /system/components/{id}` | cad-organize epic | later-epic: cad-organize |
| Delete a system component (`?expected_revision=`; client) | api.py:426-430, api.py:436-437 | `DELETE /system/components/{id}` | cad-organize epic | later-epic: cad-organize |
| Connect ports (client) | api.py:432-433 | `POST /system/connections` | cad-organize epic | later-epic: cad-organize |
| Remove a connection (client) | api.py:436-437 | `DELETE /system/connections/{id}` | cad-organize epic | later-epic: cad-organize |
| Run a repository model script as one undo step (client) | api.py:1114-1115, api.py:483-510 | `POST /doc/script` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Print registry: printers and materials | api.py:1116-1119, api.py:269-274 | `GET /print/registry` | cad-print epic | later-epic: cad-print |
| Split for printing (synchronous; `background` makes it a job) | api.py:275-276 | `POST /print/split` | cad-print epic | later-epic: cad-print |
| Strength analysis job | api.py:277-278 | `POST /print/analyze` | cad-print epic | later-epic: cad-print |
| Print plan job | api.py:277-278 | `POST /print/plan` | cad-print epic | later-epic: cad-print |
| Assembly guide job | api.py:277-278 | `POST /print/assembly` | cad-print epic | later-epic: cad-print |
| Test coupons job | api.py:277-278 | `POST /print/coupons` | cad-print epic | later-epic: cad-print |
| Whole or split for strength job | api.py:277-278 | `POST /print/strength_split` | cad-print epic | later-epic: cad-print |
| List print jobs | api.py:279-280 | `GET /print/jobs` | cad-print epic | later-epic: cad-print |
| A print job's state (its `wait` is read from the body, which RoboCAD never parses for GET, api.py:1102, so it is never honoured) | api.py:281-283 | `GET /print/jobs/{id}` | cad-print epic | later-epic: cad-print |
| Cancel a print job | api.py:284-285 | `DELETE /print/jobs/{id}` | cad-print epic | later-epic: cad-print |
| List candidates | api.py:1120-1122, api.py:516-517 | `GET /candidates` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Create a candidate | api.py:518 | `POST /candidates` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Read a candidate | api.py:520 | `GET /candidates/{id}` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Discard a candidate | api.py:521 | `DELETE /candidates/{id}` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Accept a candidate into CAD | api.py:523 | `POST /candidates/{id}/accept` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Run an experiment on a candidate | api.py:524-526 | `POST /candidates/{id}/experiments` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Agent edit batch as a candidate | api.py:515 | `POST /doc/batch` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| List experiments | api.py:1123-1125, api.py:454 | `GET /experiments` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Create an experiment, or a preflight check (client) | api.py:455 | `POST /experiments` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Rust component catalogue | api.py:456-457 | `GET /experiments/catalogue` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| An experiment's status | api.py:456-457 | `GET /experiments/{id}` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Cancel an experiment | api.py:459 | `POST /experiments/{id}/cancel` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Result | api.py:460 | `GET /experiments/{id}/result` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Inputs | api.py:461 | `GET /experiments/{id}/inputs` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Diagnostics | api.py:462 | `GET /experiments/{id}/diagnostics` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Imported CAD components (client) | api.py:463 | `GET /experiments/{id}/components` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Captured sources (client) | api.py:464 | `GET /experiments/{id}/sources` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Partial trace | api.py:465 | `GET /experiments/{id}/partial` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Compare with a baseline | api.py:466 | `POST /experiments/{id}/compare` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| List threads (`node_id`, `status`, `run_id`; client) | api.py:1126-1128, api.py:349-351 | `GET /threads` | cad-organize epic | later-epic: cad-organize |
| Create a thread (client) | api.py:352-354 | `POST /threads` | cad-organize epic | later-epic: cad-organize |
| Read a thread | api.py:357 | `GET /threads/{id}` | cad-organize epic | later-epic: cad-organize |
| Resolve, reopen, reattach, relink (client) | api.py:358-360 | `PATCH /threads/{id}` | cad-organize epic | later-epic: cad-organize |
| Delete a thread (client) | api.py:361-363 | `DELETE /threads/{id}` | cad-organize epic | later-epic: cad-organize |
| Reply (client) | api.py:364-366 | `POST /threads/{id}/comments` | cad-organize epic | later-epic: cad-organize |
| Show a thread in RoboCAD's window (context, parts, highlight, back) | api.py:333-348 | `POST /threads/{id}/show` (GUI only) | cad-organize epic | later-epic: cad-organize |
| Read a message | api.py:367-373 | `GET /comments/{id}` | cad-organize epic | later-epic: cad-organize |
| Edit a message (client) | api.py:374-376 | `PATCH /comments/{id}` | cad-organize epic | later-epic: cad-organize |
| Delete a message (client) | api.py:377-379 | `DELETE /comments/{id}` | cad-organize epic | later-epic: cad-organize |
| Document state: path, dirty, roots, active group, nodes, materials, selection, view, history, document_id, revision | api.py:1129-1130, api.py:401-402 | `GET /doc` | `CadClient::doc` (`cad::sync` poll) | done-by-reading |
| Undo and redo labels | api.py:1131-1132, api.py:472-473 | `GET /history` | `CadClient::history` | done-by-reading |
| Autosave status | api.py:1133-1134, api.py:390-399 | `GET /autosave` (GUI only) | `CadClient::autosave` → `cad::panel` | done-by-reading |
| Start a recovery save | api.py:1133-1134, api.py:392-393 | `POST /autosave` (GUI only) | none | deliberately different: `POST /autosave` is GUI-only (409 headless) and the viewer never saves for the user; Save and Save As write the file |
| Node summaries (`?kind=`) | api.py:1137-1138, api.py:532-533 | `GET /nodes` | `CadClient::nodes` | done-by-reading |
| Create box, cylinder, sphere, sketch, plane, group, instance or measure (client) | api.py:1139-1140, api.py:541-571 | `POST /nodes` | the catalogue creates boxes, cylinders, spheres and instances through `POST /ops/box`, `/ops/box_three_point`, `/ops/cylinder`, `/ops/sphere` and `/ops/instance` (`tool.box`, `tool.box_center`, `tool.cylinder`, `tool.sphere`, `tool.instance`, `ops.box`), the Ops methods `POST /nodes` itself calls (api.py `Service.create`); sketches through `POST /nodes {"kind": "sketch", "plane", "calls"}` (`CadClient::create_sketch` from `ops::send_sketch`); planes through the plane tools' `POST /ops/plane_*` | deliberately different: the viewer calls the Ops methods directly for solids and planes (one route family, `POST /ops/*`); groups belong to cad-organize, measure nodes to cad-select-transform's `POST /ops/add_measurement` |
| Node detail | api.py:1143-1144, api.py:102-133 | `GET /nodes/{id}` | `CadClient::node` → `cad::inspector` | done-by-reading |
| Set attributes (name, visible, locked, disabled, material, color, pivot, transform, parent and index, tessellation_tolerance, plane, sketch) | api.py:1145-1146, api.py:573-612 | `PATCH /nodes/{id}` | `CadClient::patch` (`CadAction::CadPatch`) | done-by-reading |
| Delete a node | api.py:1147-1148, api.py:614-618 | `DELETE /nodes/{id}` | `CadClient::delete` (`CadAction::CadDelete`) | done-by-reading |
| Solid inventory | api.py:1150-1155 | `GET /nodes/{id}/solids` | `CadClient::solids` | done-by-reading |
| Face references | api.py:1157-1158, api.py:621-623 | `GET /nodes/{id}/faces` | `CadClient::faces` → `cad::topology` | done-by-reading |
| Edge references | api.py:1159-1160, api.py:625-627 | `GET /nodes/{id}/edges`, `GET /nodes/{id}/edges?samples=N` | `CadClient::edges` (`?samples=N`) → `cad::topology` | done-by-reading |
| Vertices | api.py:1161-1162, api.py:629-631 | `GET /nodes/{id}/vertices` | `CadClient::vertices` → `cad::topology` | done-by-reading |
| Display mesh (vertices, triangles, triangle_face, face_count; 404 "no mesh") | api.py:1163-1164, api.py:633-637 | `GET /nodes/{id}/mesh?tolerance=` | `CadClient::mesh` → `cad::mesh` | done-by-reading |
| Validation report | api.py:1165-1166, api.py:639-642 | `GET /nodes/{id}/validate` | cad-print epic | later-epic: cad-print |
| Exact B-rep section outline | api.py:1167-1168, api.py:644-647 | `GET /nodes/{id}/section?plane=` | `crates/sim-runtime/src/cad_client/section.rs:CadClient::section` from `cad/display/section.rs:exact_jobs` (planes the route takes: xy, xz, yz through the origin, or a plane node, `cad/display/mod.rs:exact_query`) | done-by-reading |
| Thin walls | api.py:1169-1170, api.py:649-651 | `GET /nodes/{id}/thin?threshold=` | cad-print epic | later-epic: cad-print |
| A sketch's curves | api.py:1171-1174, api.py:117-118 | `GET /nodes/{id}/sketch`; `GET /nodes/{id}` (its `sketch`, the same `Sketch.to_json`) | `CadClient::sketch` (`SketchGeometry::from_value`, tolerant: unknown or malformed curves are counted, not fatal); `cad::sketch::cache` (`CadSketches`, by (node, revision), Dedicated jobs) reads `CadClient::node` and `SketchGeometry::from_value` on its `sketch`, keeping the dropped count (`CadSketches::dropped`: the sketch edits and a `cad_sketch` call naming curve indices refuse by name when it is non-zero); plane nodes' frames through `CadClient::node` and `plane_of` (`CadSketches::plane_state`: a plane read without a valid frame is "node X has no valid plane frame", not "still being read") | done-by-reading |
| Edit a sketch with a call list | api.py:1172-1173, api.py:661-685 | `POST /nodes/{id}/sketch` | `CadClient::edit_sketch` (`SketchCall::to_json`, `calls_body`) from `ops::send_sketch`, the one path every sketch tool, edit and REST `cad_sketch` (`CadAction::CadSketch` → `sketch::edits::sketch_action`) takes | done-by-reading (api.py `Service.edit_sketch` now maps curve indices before turning two-number lists into points: a join of two curves, trim or extend with two, `circle_tangent` and `arc_tangent` failed through REST before; `cad/tests/test_api_sketch_calls.py`) |
| Ops names and signatures | api.py:1175-1176, api.py:687-693 | `GET /ops` | `CadClient::ops` | done-by-reading |
| Call an Ops method (component operations return `{"job"}` in the GUI; client) | api.py:1177, api.py:695-711 | `POST /ops/{name}` | `CadClient::op` (`CadAction::CadOp`) | done-by-reading |
| Undo (client). Any method undoes, GET included | api.py:1179-1180, api.py:719-722 | `POST /undo` | `CadClient::undo` | done-by-reading |
| Redo (client). Any method redoes | api.py:1181-1182, api.py:724-727 | `POST /redo` | `CadClient::redo` | done-by-reading |
| Read the selection (headless: the last PUT; GUI: with `mode`) | api.py:1183-1184, api.py:730-733 | `GET /selection` | `CadClient::selection` | done-by-reading |
| Set the selection (`items`, `mode`; any non-GET method) | api.py:1185-1186, api.py:735-745 | `PUT /selection` | `CadClient::set_selection` | done-by-reading |
| RoboCAD's camera and display state (headless `{}`) | api.py:1190-1191, api.py:747-752 | `GET /view` | the viewer's own camera (`cad::scene`) | deliberately different: the native view is the viewer's own; RoboCAD's `/view` describes only RoboCAD's window |
| Set RoboCAD's camera and display (409 "no GUI: /view needs the app" headless) | api.py:1192, api.py:754-793 | `PUT /view` | the viewer's own camera | deliberately different: the native camera is not RoboCAD's |
| Fit RoboCAD's camera | api.py:1188-1189 | `POST /view/fit` | `CadAction::CadFit` (native camera) | deliberately different: `cad_fit` frames the viewer's camera and leaves RoboCAD's alone |
| Component catalogue | api.py:1193-1194, components.py:648-650 | `GET /components` | cad-organize epic | later-epic: cad-organize |
| Component job status | api.py:1195-1196, api.py:713-717 | `GET /component-jobs/{id}` (GUI only) | cad-organize epic | later-epic: cad-organize |
| Cancel a component job | api.py:1195-1196, api.py:713-717 | `DELETE /component-jobs/{id}` (GUI only) | cad-organize epic | later-epic: cad-organize |
| Pose panel state | api.py:1197-1198, api.py:315 | `GET /motion` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Motion programs | api.py:296-297 | `GET /motion/programs` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Save a motion program | api.py:298 | `POST /motion/programs` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Delete a motion program (`{"name"}`) | api.py:299 | `DELETE /motion/programs` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Play | api.py:318-326 | `POST /motion/play` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Seek | api.py:318-325 | `POST /motion/seek` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Pause | api.py:328 | `POST /motion/pause` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Stop (return to the CAD pose) | api.py:329 | `POST /motion/stop` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Focus the mechanism | api.py:327 | `POST /motion/focus` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Video export state | api.py:303-304 | `GET /motion/export` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Start a video export | api.py:306-312 | `POST /motion/export` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| Cancel a video export | api.py:305 | `DELETE /motion/export` (GUI only) | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| List saved views | api.py:1199-1201, api.py:799 | `GET /views` | `crates/sim-runtime/src/cad_client/views.rs:CadClient::views` (`cad/views/mod.rs:sync`) | done-by-reading |
| Save a view (headless needs `state`) | api.py:800-806 | `POST /views` | `crates/sim-runtime/src/cad_client/views.rs:CadClient::save_view` (`cad/views/mod.rs:handle`) | done-by-reading |
| Read a saved view | api.py:817-818 | `GET /views/{id}` | `crates/sim-runtime/src/cad_client/views.rs:CadClient::view` (restore applies the listed state, `cad/views/mod.rs:restore`) | done-by-reading |
| Rename or replace a saved view | api.py:819-822 | `PATCH /views/{id}` | `crates/sim-runtime/src/cad_client/views.rs:CadClient::update_view` (`cad/views/mod.rs:handle`) | done-by-reading |
| Delete a saved view | api.py:823-825 | `DELETE /views/{id}` | `crates/sim-runtime/src/cad_client/views.rs:CadClient::delete_view` (`cad/views/mod.rs:handle`) | done-by-reading |
| Restore a saved view in RoboCAD's window | api.py:810-816 | `POST /views/{id}/restore` (GUI only) | `cad/views/mod.rs:restore` (this window's camera and display) | deliberately different: the route is GUI-only (409 headless) and moves RoboCAD's own camera, not this one |
| Software render PNG (client) | api.py:1202-1203, api.py:830-894 | `GET /render` | `crates/sim-runtime/src/cad_client/files.rs:CadClient::render` (`cad/files/mod.rs:render`) | done-by-reading |
| RoboCAD viewport screenshot (client) | api.py:1204-1205, api.py:945-954 | `GET /screenshot` (GUI only) | the viewer's own capture | deliberately different: the native viewport is captured by the viewer, not by RoboCAD |
| Temporary-camera capture PNG | api.py:1206-1207, api.py:896-943 | `POST /capture` (GUI only) | the viewer's own capture | deliberately different: same reason as `/screenshot` |
| Save (`{"path"}` saves as) | api.py `Service.save` and its `save` route | `POST /save` | `CadClient::save` remains a client call, unused by CAD mode since f15766ea: `CadAction::CadSave` (Save, and `cad_save {path}` to save as) sends the same save with RoboCAD's thumbnail, `POST /save/thumbnail` (`crates/sim-runtime/src/cad_client/files.rs:CadClient::save_with_thumbnail`, `cad/files/mod.rs:save`) | done-by-reading |
| Open a file in a new RoboCAD window (headless 409) | api.py:1210-1211, api.py:966-972 | `POST /open` (GUI only) | `CadClient::open` exists; CAD mode opens files by starting its own service | deliberately different: CAD mode starts a headless service on the file rather than asking RoboCAD for another window |
| Load status | api.py:1212-1213, api.py:974-985 | `GET /loads/{id}` (GUI only) | `crates/sim-runtime/src/cad_client/mod.rs:CadClient::load_status` (unused) | deliberately different: `POST /open` opens another RoboCAD window (409 headless) and never replaces the document; CAD mode starts its own headless service on the file instead (see "Headless versus GUI-only routes") |
| Cancel a load | api.py:1212-1213, api.py:978-983 | `DELETE /loads/{id}` (GUI only) | `crates/sim-runtime/src/cad_client/mod.rs:CadClient::cancel_load` (unused) | deliberately different: `POST /open` opens another RoboCAD window (409 headless) and never replaces the document; CAD mode starts its own headless service on the file instead (see "Headless versus GUI-only routes") |
| Export STL, 3MF, STEP, IGES, OBJ, sketch SVG or drawing | api.py:1214-1215, api.py:987-1019 | `POST /export` | `crates/sim-runtime/src/cad_client/mod.rs:CadClient::export` (`cad/files/mod.rs:export`) | done-by-reading |
| Import a file | api.py:1216-1217, api.py:1021-1036 | `POST /import` | `crates/sim-runtime/src/cad_client/files.rs:CadClient::import` (`cad/files/mod.rs:file`) | done-by-reading |
| Robot summary: joints, motors, DoF, ground, issues | api.py:1218-1219 | `GET /robot` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::robot`; read in `cad/robot/data.rs:sync` | done-by-reading |
| Motor library | api.py:1220-1221 | `GET /motors` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::motors`; read once per generation in `cad/robot/data.rs:sync` | done-by-reading |
| Frame time and display triangles (GUI); revision and node count | api.py:1222-1228 | `GET /performance` | the viewer's own frame statistics | deliberately different: the viewer measures its own frames |
| Physical description (`flex`; `path` writes the file; client) | api.py:1229-1238 | `GET /physical` | `CadClient::physical` (never passes `path`) | done-by-reading |
| Actuator profiles | api.py:1239-1241 | `GET /actuator-profiles` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::actuator_profiles`; read in `cad/robot/data.rs:sync` | done-by-reading |
| Set actuator profiles | api.py:1242-1243 | `POST /actuator-profiles` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::set_actuator_profiles`; `cad/results/mod.rs:profiles` | done-by-reading |
| Simulation results file (client) | api.py:1247 | `GET /results` | `crates/sim-runtime/src/cad_client/physical.rs:CadClient::results`; the viewer reads `GET /results/nodes` (`CadClient::results_nodes` in `cad/robot/data.rs:sync`) | done-by-reading |
| Load results (client) | api.py:1245-1246 | `POST /results/load` | `crates/sim-runtime/src/cad_client/physical.rs:CadClient::load_results`; `cad/results/mod.rs:handle` | done-by-reading |
| Apply identification (any method or sub-path under `/identification`; client) | api.py:1248-1249 | `POST /identification/apply` | `crates/sim-runtime/src/cad_client/physical.rs:CadClient::apply_identification`; `cad/results/mod.rs:handle` | done-by-reading |
| List sensors | api.py:1250-1253 | `GET /sensors` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::sensors`; read in `cad/robot/data.rs:sync` | done-by-reading |
| Add a sensor (client) | api.py:1251-1252 | `POST /sensors` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::add_sensor`; the catalogue's robot entries (`cad/ops/robot_args.rs`) | done-by-reading |
| List cables | api.py:1254-1257 | `GET /cables` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::cables`; read in `cad/robot/data.rs:sync` | done-by-reading |
| Add a cable (client) | api.py:1255-1256 | `POST /cables` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::add_cable`; the catalogue's robot entries (`cad/ops/robot_args.rs`) | done-by-reading |
| Battery | api.py:1258-1261 | `GET /battery` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::battery`; read in `cad/robot/data.rs:sync` | done-by-reading |
| Set the battery (client) | api.py:1259-1260 | `PUT /battery` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::set_battery`; the catalogue's robot entries (`cad/ops/robot_args.rs`) | done-by-reading |
| Control loop | api.py:1262-1265 | `GET /control` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::control`; read in `cad/robot/data.rs:sync` | done-by-reading |
| Set the control loop (client) | api.py:1263-1264 | `PUT /control` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::set_control`; the catalogue's robot entries (`cad/ops/robot_args.rs`) | done-by-reading |
| Uncertainty | api.py:1266-1269 | `GET /uncertainty` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::uncertainty`; read in `cad/robot/data.rs:sync` | done-by-reading |
| Set the uncertainty (client) | api.py:1267-1268 | `PUT /uncertainty` | `crates/sim-runtime/src/cad_client/robot.rs:CadClient::set_uncertainty`; the catalogue's robot entries (`cad/ops/robot_args.rs`) | done-by-reading |
| Materials | api.py:1270-1273, api.py:1038-1039 | `GET /materials` | read from `/doc` (`cad/materials/mod.rs:list`); `crates/sim-runtime/src/cad_client/physical.rs:CadClient::materials` | done-by-reading |
| Add a material | api.py:1271-1272, api.py:1041-1048 | `POST /materials` | `cad/materials/mod.rs:submit` → `crates/sim-runtime/src/cad_client/physical.rs:CadClient::add_material` | done-by-reading |
| The GUI command registry (`{}` headless) | api.py:1274-1278, api.py:1050-1053 | `GET /commands` | `CadClient::commands` → `cad::panel` | done-by-reading |
| Run a GUI command (409 "no GUI" headless; 404 unknown) | api.py:1275-1276, api.py:1055-1061 | `POST /commands/{id}` | `CadClient::run_command` (`CadAction::CadCommand`) | done-by-reading |
| CORS preflight (`Access-Control-Allow-Methods`) | api.py:1303-1308 | `OPTIONS *` | n/a | deliberately different: CORS is for browser pages; the native client is not a browser and sends no preflight |

## Ops command layer

One row per public `Ops` method (commands.py:253 and its mixins). All are
reachable now by REST `cad_op` (`CadAction::CadOp`, `POST /ops/{name}`).
*Status* is that of the viewer UI for the method. `body_of` is excluded:
`GET /ops` hides it (api.py:690).

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| `configure_robot` (assembly metadata and connectors as one edit) | commands.py:257 | `POST /ops/configure_robot` | the catalogue's `ops.configure_robot` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot_ops.rs:CadClient::configure_robot` | done-by-reading |
| `set_component_graph` | commands.py:262 | `POST /ops/set_component_graph` | cad-organize epic | later-epic: cad-organize |
| `print_split` | commands.py:299 | `POST /ops/print_split` | cad-print epic | later-epic: cad-print |
| `undo` | commands.py:305 | `POST /undo` | `CadAction::CadUndo` | done-by-reading |
| `redo` | commands.py:308 | `POST /redo` | `CadAction::CadRedo` | done-by-reading |
| `delete` | commands.py:312 | `DELETE /nodes/{id}`; `POST /ops/delete` | `cad::ops` catalogue `edit.delete` (`POST /ops/delete` of every selected node); `CadAction::CadDelete` (`DELETE /nodes/{id}`, one node) | done-by-reading |
| `rename` | commands.py:333 | `PATCH /nodes/{id} {"name"}` | `CadAction::CadPatch` | done-by-reading |
| `set_visible` | commands.py:336 | `PATCH /nodes/{id} {"visible"}` | `CadAction::CadPatch` | done-by-reading |
| `set_locked` | commands.py:339 | `PATCH /nodes/{id} {"locked"}` | `CadAction::CadPatch` | done-by-reading |
| `set_disabled` | commands.py:342 | `PATCH /nodes/{id} {"disabled"}` | `CadAction::CadPatch` | done-by-reading |
| `set_material` | commands.py:345 | `PATCH /nodes/{id} {"material"}` | `CadAction::CadPatch` | done-by-reading |
| `set_color` | commands.py:348 | `PATCH /nodes/{id} {"color"}` | `cad/inspector/physical_edit.rs:handle_physical` (`PATCH /nodes/{id} {"color"}` through `cad/actions.rs:edit_at`); `crates/sim-runtime/src/cad_client/physical.rs:CadClient::set_color` also exists | done-by-reading |
| `set_pivot` | commands.py:351 | `PATCH /nodes/{id} {"pivot"}` | `cad::ops` catalogue `tool.set_pivot`; the inspector's pivot editor (`cad::inspector::editors`) sends `PATCH /nodes/{id} {"pivot"}` via `ops::handle`/`args::build` | done-by-reading |
| `group` | commands.py:354 | `POST /ops/group` | cad-organize epic | later-epic: cad-organize |
| `move_nodes` | commands.py:377 | `POST /ops/move_nodes` | cad-organize epic | later-epic: cad-organize |
| `move_node` | commands.py:395 | `PATCH /nodes/{id} {"parent", "index"}` | cad-organize epic | later-epic: cad-organize |
| `set_active_group` | commands.py:398 | `POST /ops/set_active_group` | cad-organize epic | later-epic: cad-organize |
| `isolate` | commands.py:402 | `POST /ops/isolate` | `cad/ops/catalogue/view.rs:ENTRIES` (`view.isolate`, route `isolate`) | done-by-reading |
| `show_all` | commands.py:415 | `POST /ops/show_all` | `cad/ops/catalogue/view.rs:ENTRIES` (`view.show_all`, route `show_all`) | done-by-reading |
| `box` | commands.py:419 | `POST /ops/box`; `POST /nodes` | `cad::ops` catalogue `tool.box` and `tool.box_center` (both send `Ops.box`), and the REST-only `ops.box` via `ops::handle`/`args::build` | done-by-reading |
| `box_center` | commands.py:422 | `POST /ops/box_center` | `cad::ops` catalogue `ops.box_center` (REST `cad_invoke`/`cad_run`, with its form) via `ops::handle`/`args::build` | done-by-reading |
| `box_three_point` | commands.py:426 | `POST /ops/box_three_point` | `cad::ops` catalogue `ops.box_three_point` (REST, with its form; every parameter required) via `ops::handle`/`args::build` | done-by-reading |
| `cylinder` | commands.py:437 | `POST /ops/cylinder` | `cad::ops` catalogue `tool.cylinder` via `ops::handle`/`args::build` | done-by-reading |
| `sphere` | commands.py:440 | `POST /ops/sphere` | `cad::ops` catalogue `tool.sphere` via `ops::handle`/`args::build` | done-by-reading |
| `new_sketch` | commands.py:444 | `POST /ops/new_sketch` | never called by the viewer: a new sketch comes with its first shape, `CadClient::create_sketch` → `POST /nodes {"kind": "sketch", "plane", "calls"}` (`Service.create` calls `Ops.new_sketch`); `cad_op` still reaches `POST /ops/new_sketch` | deliberately different: no empty sketch is created when a tool starts (see "New sketch on a plane") |
| `edit_sketch` (takes a Python callable, so `/ops` cannot pass it) | commands.py:449 | `POST /nodes/{id}/sketch` | `CadClient::edit_sketch` → `POST /nodes/{id}/sketch` (api.py `Service.edit_sketch` calls `Ops.edit_sketch`) from `ops::send_sketch` | done-by-reading |
| `extrude` | commands.py:480 | `POST /ops/extrude` | `cad::ops` catalogue `tool.extrude` via `ops::handle`/`args::build` (`sketch::extrude::calls`) | done-by-reading |
| `revolve` | commands.py:489 | `POST /ops/revolve` | `cad::ops` catalogue `tool.revolve` via `ops::handle`/`args::build` (`sketch::extrude::calls`) | done-by-reading |
| `sweep` | commands.py:493 | `POST /ops/sweep` | `cad::ops` catalogue `tool.sweep` via `ops::handle`/`args::build` | done-by-reading |
| `pipe` | commands.py:498 | `POST /ops/pipe` | `cad::ops` catalogue `tool.pipe` via `ops::handle`/`args::build` | done-by-reading |
| `loft` | commands.py:502 | `POST /ops/loft` | `cad::ops` catalogue `tool.loft` via `ops::handle`/`args::build` | done-by-reading |
| `fill` | commands.py:507 | `POST /ops/fill` | `cad::ops` catalogue `tool.fill` via `ops::handle`/`args::build`; needs api.py's 2026-10-01 fix: `ArgConverter` read the node id given to `Ops.fill(edges: str \| Body)` as an edge reference (any parameter named `edges`), a 400 for every fill; a string now passes through when the annotation has no `EdgeRef` (cad/tests/test_api_fill_node.py) | done-by-reading |
| `bridge` | commands.py:511 | `POST /ops/bridge` | `cad::ops` catalogue `ops.bridge` (two curves or sketches; REST) via `ops::handle`/`args::build` | done-by-reading |
| `push_pull` | commands.py:515 | `POST /ops/push_pull` | `cad::transform::commit::push_pull_call` (`CadAction::CadPushPull`, REST `cad_push_pull`) | done-by-reading |
| `offset_faces` | commands.py:518 | `POST /ops/offset_faces` | `cad::transform::commit::offset_call` (`CadAction::CadOffsetFaces`, REST `cad_offset_faces`) | done-by-reading |
| `offset_face_to` | commands.py:521 | `POST /ops/offset_face_to` | `cad::ops` catalogue `tool.dependent_offset` via `ops::handle`/`args::build` | done-by-reading |
| `move_faces` | commands.py:525 | `POST /ops/move_faces` | `cad::ops` catalogue `ops.move_faces` (REST, with its form; per node) via `ops::handle`/`args::build` | done-by-reading |
| `rotate_faces` | commands.py:528 | `POST /ops/rotate_faces` | `cad::ops` catalogue `ops.rotate_faces` (REST, with its form; per node) via `ops::handle`/`args::build` | done-by-reading |
| `set_radius` | commands.py:531 | `POST /ops/set_radius` | `cad::ops` catalogue `ops.set_radius` (REST, with its form) via `ops::handle`/`args::build` | done-by-reading |
| `set_diameter` | commands.py:534 | `POST /ops/set_diameter` | `cad::transform::commit::dimension_call` (`CadAction::CadSetDimension`, REST `cad_set_dimension`) | done-by-reading |
| `set_distance` | commands.py:537 | `POST /ops/set_distance` | `cad::transform::commit::dimension_call` (`CadAction::CadSetDimension`, REST `cad_set_dimension`) | done-by-reading |
| `set_angle` | commands.py:544 | `POST /ops/set_angle` | `cad::transform::commit::dimension_call` (`CadAction::CadSetDimension`, REST `cad_set_dimension`) | done-by-reading |
| `draft` | commands.py:553 | `POST /ops/draft` | `cad::ops` catalogue `tool.draft` via `ops::handle`/`args::build` | done-by-reading |
| `delete_faces` | commands.py:556 | `POST /ops/delete_faces` | `cad::ops` catalogue `tool.delete_face` via `ops::handle`/`args::build` | done-by-reading |
| `untrim` | commands.py:559 | `POST /ops/untrim` | `cad::ops` catalogue `ops.untrim` (REST; per node) via `ops::handle`/`args::build` | done-by-reading |
| `imprint` | commands.py:562 | `POST /ops/imprint` | `cad::ops` catalogue `tool.imprint` via `ops::handle`/`args::build` | done-by-reading |
| `split_face` | commands.py:566 | `POST /ops/split_face` | `cad::ops` catalogue `tool.split_face` via `ops::handle`/`args::build` | done-by-reading |
| `boolean` | commands.py:573 | `POST /ops/boolean` | `cad::ops` catalogue `modify.union`, `modify.subtract`, `modify.intersect` via `ops::handle`/`args::build` | done-by-reading |
| `region` | commands.py:588 | `POST /ops/region` | `cad::ops` catalogue `modify.region` via `ops::handle`/`args::build` | done-by-reading |
| `cut` | commands.py:591 | `POST /ops/cut` | `cad::ops` catalogue `tool.cut_plane` (a plane name) and `tool.cut_sheet` (a cutter node id, which api.py's `ArgConverter` passes through since 2026-10-01) via `ops::handle`/`args::build` | done-by-reading |
| `shell` | commands.py:624 | `POST /ops/shell` | `cad::ops` catalogue `tool.shell` via `ops::handle`/`args::build` | done-by-reading |
| `thicken` | commands.py:627 | `POST /ops/thicken` | `cad::ops` catalogue `tool.thicken` via `ops::handle`/`args::build` | done-by-reading |
| `fillet` | commands.py:630 | `POST /ops/fillet` | `cad::ops` catalogue `tool.fillet` and `tool.fillet_variable` (`radius_end`) via `ops::handle`/`args::build` | done-by-reading |
| `fillet_chordal` | commands.py:633 | `POST /ops/fillet_chordal` | `cad::ops` catalogue `tool.fillet_chordal` via `ops::handle`/`args::build` | done-by-reading |
| `fillet_all` | commands.py:636 | `POST /ops/fillet_all` | `cad::ops` catalogue `tool.fillet_all` via `ops::handle`/`args::build` | done-by-reading |
| `full_round` | commands.py:639 | `POST /ops/full_round` | `cad::ops` catalogue `tool.full_round` via `ops::handle`/`args::build` | done-by-reading |
| `remove_fillets` | commands.py:642 | `POST /ops/remove_fillets` | `cad::ops` catalogue `tool.remove_fillets` via `ops::handle`/`args::build` | done-by-reading |
| `chamfer` | commands.py:645 | `POST /ops/chamfer` | `cad::ops` catalogue `tool.chamfer` (`Shape::Chamfer`) via `ops::handle`/`args::build` | done-by-reading |
| `transform` | commands.py:648 | `POST /ops/transform` | `cad::transform::commit::transform_call` (`CadAction::CadTransform`, REST `cad_transform`) | done-by-reading |
| `mirror` | commands.py:694 | `POST /ops/mirror` | `cad::ops` catalogue `tool.mirror` and `tool.mirror_live` via `ops::handle`/`args::build` | done-by-reading |
| `instance` | commands.py:712 | `POST /ops/instance`; `POST /nodes {"kind": "instance"}` | `cad::ops` catalogue `tool.instance` (`POST /ops/instance`) via `ops::handle`/`args::build` | done-by-reading |
| `make_unique` | commands.py:719 | `POST /ops/make_unique` | `cad::ops` catalogue `modify.make_unique` via `ops::handle`/`args::build` | done-by-reading |
| `array_rect` | commands.py:728 | `POST /ops/array_rect` | `cad::ops` catalogue `tool.array` (rectangular) via `ops::handle`/`args::build` | done-by-reading |
| `array_radial` | commands.py:738 | `POST /ops/array_radial` | `cad::ops` catalogue `tool.array` (radial) via `ops::handle`/`args::build` | done-by-reading |
| `array_curve` | commands.py:743 | `POST /ops/array_curve` | `cad::ops` catalogue `ops.array_curve` (REST, with its form; the bodies, then the path) via `ops::handle`/`args::build` | done-by-reading |
| `join` | commands.py:806 | `POST /ops/join` | `cad::ops` catalogue `modify.join` via `ops::handle`/`args::build` | done-by-reading |
| `unjoin` | commands.py:814 | `POST /ops/unjoin` | `cad::ops` catalogue `modify.unjoin` via `ops::handle`/`args::build` | done-by-reading |
| `dissolve` | commands.py:823 | `POST /ops/dissolve` | `cad::ops` catalogue `modify.dissolve` via `ops::handle`/`args::build` | done-by-reading |
| `extract_components` | commands.py:826 | `POST /ops/extract_components` | `cad::ops` catalogue `ops.extract_components` (REST, with its form; `expected_revision` is the shown revision) via `ops::handle`/`args::build` | done-by-reading |
| `project_curve` | commands.py:856 | `POST /ops/project_curve` | `cad::ops` catalogue `tool.project_curve` via `ops::handle`/`args::build` | done-by-reading |
| `silhouette` | commands.py:860 | `POST /ops/silhouette` | `cad::ops` catalogue `tool.silhouette` via `ops::handle`/`args::build` | done-by-reading |
| `set_control_points` | commands.py:864 | `POST /ops/set_control_points` | `cad::ops` catalogue `ops.set_control_points` (REST, with its form; the grid `tool.control_points` shows) via `ops::handle`/`args::build`; the route takes the grid since `ArgConverter`'s points fix (api.py, 2026-10-01, `test_api_control_points_set.py`) | done-by-reading |
| `raise_degree` | commands.py:867 | `POST /ops/raise_degree` | `cad::ops` catalogue `tool.raise_degree` via `ops::handle`/`args::build` | done-by-reading |
| `rebuild_face` | commands.py:870 | `POST /ops/rebuild_face` | `cad::ops` catalogue `tool.rebuild_face` via `ops::handle`/`args::build` | done-by-reading |
| `plane_from_face` | commands.py:874 | `POST /ops/plane_from_face` | `cad::ops` catalogue `tool.plane` via `ops::handle`/`args::build` | done-by-reading |
| `plane_three_points` | commands.py:878 | `POST /ops/plane_three_points` | `cad::ops` catalogue `tool.plane_three` via `ops::handle`/`args::build` | done-by-reading |
| `plane_two_points_camera` | commands.py:881 | `POST /ops/plane_two_points_camera` | `cad::ops` catalogue `tool.plane_camera` via `ops::handle`/`args::build` | done-by-reading |
| `plane_midplane` | commands.py:887 | `POST /ops/plane_midplane` | `cad::ops` catalogue `tool.plane_mid` via `ops::handle`/`args::build` | done-by-reading |
| `add_measurement` | commands.py:897 | `POST /ops/add_measurement`; `POST /nodes {"kind": "measure"}` | `cad::transform::commit::measure` (`CadAction::CadMeasure { keep: true }`, REST `cad_measure`) | done-by-reading |
| `clearance` | commands.py:903 | `POST /ops/clearance` | cad-print epic | later-epic: cad-print |
| `fastener_hole` | commands.py:922 | `POST /ops/fastener_hole` | cad-print epic | later-epic: cad-print |
| `add_joint` | commands.py:935 | `POST /ops/add_joint` | the catalogue's `robot.add_joint`, `robot.joint_dialog` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot_ops.rs:CadClient::add_joint` | done-by-reading |
| `set_joint` | commands.py:949 | `POST /ops/set_joint` | the catalogue's `ops.set_joint (Edit joint)`, `robot.joint_dialog` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot_ops.rs:CadClient::set_joint` | done-by-reading |
| `connect_fixed` | commands.py:960 | `POST /ops/connect_fixed` | the catalogue's `robot.fixed` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot_ops.rs:CadClient::connect_fixed` | done-by-reading |
| `add_motor` | commands.py:966 | `POST /ops/add_motor` | the catalogue's `robot.add_motor` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot_ops.rs:CadClient::add_motor` | done-by-reading |
| `mount_motor` | commands.py:983 | `POST /ops/mount_motor` | the catalogue's `robot.add_motor (Mount on)` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot_ops.rs:CadClient::mount_motor` | done-by-reading |
| `attach_motor` | commands.py:992 | `POST /ops/attach_motor` | the catalogue's `robot.assign_motor` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot_ops.rs:CadClient::attach_motor` | done-by-reading |
| `set_ground` | commands.py:1016 | `POST /ops/set_ground` | the catalogue's `robot.ground` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot_ops.rs:CadClient::set_ground` | done-by-reading |
| `infer_joints` | commands.py:1023 | `POST /ops/infer_joints` | the catalogue's `robot.infer` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot_ops.rs:CadClient::infer_joints` | done-by-reading |
| `robot` | commands.py:1036 | `GET /robot`; `POST /ops/robot` | `cad/robot/tools.rs:validate` (`robot.validate`) and the Robot panel (`cad/robot/data.rs:sync`); `crates/sim-runtime/src/cad_client/robot.rs:CadClient::robot` | done-by-reading |
| `motor_library` | commands.py:1041 | `GET /motors` | `cad/robot/tools_library.rs:draw` (`robot.motors`); `crates/sim-runtime/src/cad_client/robot.rs:CadClient::motors` | done-by-reading |
| `add_sensor` | commands.py:1047 | `POST /sensors`; `POST /ops/add_sensor` | the catalogue's `robot.add_sensor` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot.rs:CadClient::add_sensor` | done-by-reading |
| `add_cable` | commands.py:1062 | `POST /cables`; `POST /ops/add_cable` | the catalogue's `robot.add_cable` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot.rs:CadClient::add_cable` | done-by-reading |
| `save_motion` | commands.py:1073 | `POST /motion/programs` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| `delete_motion` | commands.py:1083 | `DELETE /motion/programs` | cad-experiments-motion epic | later-epic: cad-experiments-motion |
| `set_robot_setting` | commands.py:1090 | `POST /ops/set_robot_setting` | the catalogue's `robot.power` (`cad/ops/catalogue/robot.rs`, `cad/ops/robot_args.rs`); `crates/sim-runtime/src/cad_client/robot_ops.rs:CadClient::set_robot_setting` | done-by-reading |
| `link_system` | commands.py:1112 | `POST /ops/link_system` | cad-organize epic | later-epic: cad-organize |
| `unlink_system` | commands.py:1121 | `POST /ops/unlink_system` | cad-organize epic | later-epic: cad-organize |
| `refresh_system_link` | commands.py:1126 | `POST /ops/refresh_system_link` | cad-organize epic | later-epic: cad-organize |
| `system_status` | commands.py:1134 | `POST /ops/system_status` | cad-organize epic | later-epic: cad-organize |
| `set_actuator_profiles` | commands.py:1138 | `POST /actuator-profiles` | `cad/results/mod.rs:profiles`; `crates/sim-runtime/src/cad_client/robot.rs:CadClient::set_actuator_profiles` | done-by-reading |
| `set_battery` | commands.py:1145 | `PUT /battery` | the catalogue's `robot.power` (`cad/ops/robot_args.rs` `power`); `crates/sim-runtime/src/cad_client/robot.rs:CadClient::set_battery` | done-by-reading |
| `set_control` | commands.py:1151 | `PUT /control` | the catalogue's `robot.power` (`cad/ops/robot_args.rs` `power`); `crates/sim-runtime/src/cad_client/robot.rs:CadClient::set_control` | done-by-reading |
| `set_uncertainty` | commands.py:1162 | `PUT /uncertainty` | the catalogue's `robot.power` (`cad/ops/robot_args.rs` `power`); `crates/sim-runtime/src/cad_client/robot.rs:CadClient::set_uncertainty` | done-by-reading |
| `set_material_props` | commands.py:1171 | `POST /ops/set_material_props` | `cad/materials/mod.rs:submit`; `crates/sim-runtime/src/cad_client/physical.rs:CadClient::set_material_props` | done-by-reading |
| `set_joint_physics` | commands.py:1184 | `POST /ops/set_joint_physics` | `cad/inspector/physical_edit.rs:handle_physical`; `crates/sim-runtime/src/cad_client/physical.rs:CadClient::set_joint_physics` | done-by-reading |
| `physical` (`path` writes a file) | commands.py:1233 | `GET /physical` | `CadAction::CadPhysical` (never `path`) | done-by-reading |
| `load_results` | commands.py:1239 | `POST /results/load` | `cad/results/mod.rs:handle` (`Load`); `crates/sim-runtime/src/cad_client/physical.rs:CadClient::load_results` | done-by-reading |
| `apply_identification` | commands.py:1244 | `POST /identification/apply` | `cad/results/mod.rs:handle` (`Identify`); `crates/sim-runtime/src/cad_client/physical.rs:CadClient::apply_identification` | done-by-reading |
| `import_references` | references.py:11 | `POST /ops/import_references` | cad-organize epic | later-epic: cad-organize |
| `update_reference` | references.py:31 | `POST /ops/update_reference` | cad-organize epic | later-epic: cad-organize |
| `calibrate_reference` | references.py:65 | `POST /ops/calibrate_reference` | cad-organize epic | later-epic: cad-organize |
| `saved_views` | saved_views.py:104 | `GET /views` | `crates/sim-runtime/src/cad_client/views.rs:CadClient::views` | done-by-reading |
| `save_view` | saved_views.py:107 | `POST /views` | `crates/sim-runtime/src/cad_client/views.rs:CadClient::save_view` | done-by-reading |
| `update_saved_view` | saved_views.py:116 | `PATCH /views/{id}` | `crates/sim-runtime/src/cad_client/views.rs:CadClient::update_view` | done-by-reading |
| `delete_saved_view` | saved_views.py:124 | `DELETE /views/{id}` | `crates/sim-runtime/src/cad_client/views.rs:CadClient::delete_view` | done-by-reading |
| `threads` | annotations.py:216 | `GET /threads` | cad-organize epic | later-epic: cad-organize |
| `thread` | annotations.py:228 | `GET /threads/{id}` | cad-organize epic | later-epic: cad-organize |
| `create_thread` | annotations.py:233 | `POST /threads` | cad-organize epic | later-epic: cad-organize |
| `update_thread` | annotations.py:251 | `PATCH /threads/{id}` | cad-organize epic | later-epic: cad-organize |
| `delete_thread` | annotations.py:271 | `DELETE /threads/{id}` | cad-organize epic | later-epic: cad-organize |
| `add_comment` | annotations.py:276 | `POST /threads/{id}/comments` | cad-organize epic | later-epic: cad-organize |
| `update_comment` | annotations.py:284 | `PATCH /comments/{id}` | cad-organize epic | later-epic: cad-organize |
| `delete_comment` | annotations.py:287 | `DELETE /comments/{id}` | cad-organize epic | later-epic: cad-organize |
| `transform_components` | components.py:490 | `POST /ops/transform_components` | cad-organize epic | later-epic: cad-organize |
| `make_component` | components.py:515 | `POST /ops/make_component` | cad-organize epic | later-epic: cad-organize |
| `create_component_family` | components.py:541 | `POST /ops/create_component_family` | cad-organize epic | later-epic: cad-organize |
| `link_component_family` | components.py:554 | `POST /ops/link_component_family` | cad-organize epic | later-epic: cad-organize |
| `new_parametric_component` | components.py:566 | `POST /ops/new_parametric_component` | cad-organize epic | later-epic: cad-organize |
| `export_component` | components.py:593 | `POST /ops/export_component` | cad-organize epic | later-epic: cad-organize |
| `import_component` | components.py:612 | `POST /ops/import_component` | cad-organize epic | later-epic: cad-organize |
| `component_catalogue` | components.py:648 | `GET /components` | cad-organize epic | later-epic: cad-organize |
| `create_component` | components.py:652 | `POST /ops/create_component` | cad-organize epic | later-epic: cad-organize |
| `place_component` | components.py:658 | `POST /ops/place_component` | cad-organize epic | later-epic: cad-organize |
| `set_component_parameters` | components.py:677 | `POST /ops/set_component_parameters` | cad-organize epic | later-epic: cad-organize |
| `set_component_overrides` | components.py:715 | `POST /ops/set_component_overrides` | cad-organize epic | later-epic: cad-organize |
| `detach_component` | components.py:735 | `POST /ops/detach_component` | cad-organize epic | later-epic: cad-organize |

## Keymap

One row per id in `ui/keymap.json`. Shortcuts are bound only from
keymap.json (and `~/.robocad/keymap.json`), at ui/app.py:453-467; an id
with no registry command is skipped (ui/app.py:462-463). Qt's "Ctrl" is
Cmd on macOS, which matches the native Ctrl/Cmd.

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| `command_palette`: Ctrl+Space, Shift+F | keymap.json:3 | `GET /commands` | `cad::keys` → `CadSurface { palette }` at the pointer (`registry::Opens::Palette`) | deliberately different: on macOS Command+Space is Spotlight's, so Control+Space or Shift+F opens it (`cad::keys` clash table) |
| `file.new`: Ctrl+N | keymap.json:4 | n/a | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `Do::File` → `cad/files/mod.rs:command_action` (`CadFile { new }`: the path form) | done-by-reading |
| `file.open`: Ctrl+O | keymap.json:4 | n/a | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `cad/files/mod.rs:command_action` (`CadFile { open }`: the path form) | done-by-reading |
| `file.save`: Ctrl+S | keymap.json:4 | `POST /save` (the viewer: `POST /save/thumbnail`) | `cad::keys` → `CadAction::CadSave` → `cad/files/mod.rs:save` | done-by-reading |
| `file.save_as`: Ctrl+Shift+S | keymap.json:4 | `POST /save {"path"}` (the viewer: `POST /save/thumbnail {"path"}`) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `cad/files/mod.rs:command_action` (`CadFile { save_as }`: the path form) | done-by-reading |
| `file.import`: Ctrl+I | keymap.json:4 | `POST /import` | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `cad/files/mod.rs:command_action` (`CadFile { import }`: the path form) | done-by-reading |
| `file.export`: Ctrl+E | keymap.json:4 | `POST /export` | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `cad/files/mod.rs:command_action` (`CadExport`: the export form) | done-by-reading |
| `file.export_drawing`: Ctrl+Shift+D | keymap.json:4 | `POST /export` | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `cad/files/mod.rs:command_action` (`CadExport { drawing }`: the export form) | done-by-reading |
| `edit.undo`: Ctrl+Z | keymap.json:5 | `POST /undo` | `cad::keys` → `CadAction::CadUndo` | done-by-reading |
| `edit.redo`: Ctrl+Shift+Z | keymap.json:5 | `POST /redo` | `cad::keys` → `CadAction::CadRedo` | done-by-reading |
| `edit.delete`: Delete, Backspace | keymap.json:5 | `POST /ops/delete {"args": [[ids]]}` | `cad::keys` → `CadInvoke { edit.delete }` → `cad::ops` catalogue `edit.delete` (every selected node in one step since cad-modify; silent on an empty selection, as RoboCAD) | done-by-reading |
| `edit.copy`: Ctrl+C | keymap.json:5 | `POST /clipboard/copy` (as "Copy with Placement") | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { edit.copy }` when ready, else the status line says why | deliberately different: the clip stays in the viewer (`OpsState::clipboard`), not on the OS clipboard (see "Copy with Placement") |
| `edit.paste`: Ctrl+V | keymap.json:5 | `POST /clipboard/paste` (as "Paste with Placement") | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { edit.paste }` when ready, else the status line says why | deliberately different: pastes the viewer's last copy, not the OS clipboard (see "Paste with Placement") |
| `edit.select_all`: Ctrl+A | keymap.json:5 | `PUT /selection` | `cad::keys` → `CadAction::CadSelectAll` | done-by-reading |
| `edit.invert`: Ctrl+Shift+I | keymap.json:5 | `PUT /selection` | `cad::keys` → `CadAction::CadInvertSelection` | done-by-reading |
| `edit.select_same_material`: Ctrl+Shift+M (`robot.add_motor` lists the same key, unbound; see below) | keymap.json:5 | `PUT /selection` | `cad::keys` → `CadAction::CadSelectSameMaterial` | done-by-reading |
| `view.fit`: Home | keymap.json:6 | n/a (display) | `cad::keys` → `CadAction::CadFit` | done-by-reading |
| `view.focus`: F | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `cad/surfaces/registry.rs:focus` | done-by-reading |
| `view.front`: 1 | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CameraAction::View { front }` (`camera/orbit.rs:Orbit::preset`) | done-by-reading |
| `view.back`: Ctrl+1 | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CameraAction::View { back }` (`camera/orbit.rs:Orbit::preset`) | done-by-reading |
| `view.top`: 7 | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CameraAction::View { top }` (`camera/orbit.rs:Orbit::preset`) | done-by-reading |
| `view.bottom`: Ctrl+7 | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CameraAction::View { bottom }` (`camera/orbit.rs:Orbit::preset`) | done-by-reading |
| `view.right`: 3 | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CameraAction::View { right }` (`camera/orbit.rs:Orbit::preset`) | done-by-reading |
| `view.left`: Ctrl+3 | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CameraAction::View { left }` (`camera/orbit.rs:Orbit::preset`) | done-by-reading |
| `view.iso`: 0 | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CameraAction::View { iso }` (`camera/orbit.rs:Orbit::preset`) | done-by-reading |
| `view.ortho`: 5 | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CameraAction::Projection` (toggle) | done-by-reading |
| `view.grid`: Ctrl+G | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `DisplayCmd::Grid` → `cad/display/mod.rs:apply_display` | done-by-reading |
| `view.mode_next`: Z | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `DisplayCmd::Next` → `cad/display/mod.rs:apply_display` | done-by-reading |
| `view.isolate`: / | keymap.json:6 | `POST /ops/isolate` | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CadInvoke { view.isolate }` → `cad/ops/catalogue/view.rs:ENTRIES` (the keypad's divide is `/`) | done-by-reading |
| `view.show_all`: Alt+H | keymap.json:6 | `POST /ops/show_all` | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CadInvoke { view.show_all }` → `cad/ops/catalogue/view.rs:ENTRIES` (the physical H with Alt, so macOS's Option+H reaches it) | done-by-reading |
| `view.hide`: H | keymap.json:6 | `POST /ops/set_visible` | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `CadInvoke { view.hide }` → `cad/ops/catalogue/view.rs:ENTRIES` | done-by-reading |
| `view.section`: Ctrl+Shift+X | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `DisplayCmd::Section` → `cad/display/mod.rs:apply_section` (toggle) | done-by-reading |
| `view.build_plate`: Ctrl+Shift+B | keymap.json:6 | n/a (display) | `cad/keys.rs:keys` (matched against `cad/surfaces/registry.rs:COMMANDS`) → `DisplayCmd::BuildPlate` → `cad/display/mod.rs:apply_display` | done-by-reading |
| `view.radial`: Space | keymap.json:6 | n/a (display) | `cad::keys` → `CadSurface { view_radial }` at the pointer (typed as a space while a text field has the keyboard) | done-by-reading |
| `select.body`: B | keymap.json:7 | `PUT /selection {"mode"}` | `cad::keys` → `CadAction::CadSelectMode { mode: Body }` | done-by-reading |
| `select.face`: Shift+B | keymap.json:7 | `PUT /selection {"mode"}` | `cad::keys` → `CadAction::CadSelectMode { mode: Face }` | done-by-reading |
| `select.edge`: E | keymap.json:7 | `PUT /selection {"mode"}` | `cad::keys` → `CadAction::CadSelectMode { mode: Edge }` | done-by-reading |
| `select.vertex`: V | keymap.json:7 | `PUT /selection {"mode"}` | `cad::keys` → `CadAction::CadSelectMode { mode: Vertex }` | done-by-reading |
| `select.point`: P | keymap.json:7 | `PUT /selection {"mode"}` | `cad::keys` → `CadAction::CadSelectMode { mode: Point }` | done-by-reading |
| `select.mode_radial`: Q | keymap.json:7 | n/a (display) | `cad::keys` → `CadSurface { select_radial }` at the pointer | done-by-reading |
| `tool.select`: Escape | keymap.json:8 | n/a | `cad::transform::keys` → `CadAction::CadCancel` | done-by-reading |
| `tool.annotate`: N | keymap.json:8 | `POST /threads` | cad-organize epic | later-epic: cad-organize |
| `tool.move`: G | keymap.json:8 | `POST /ops/transform` | `cad::transform::keys` → `CadAction::CadTool { tool: Move }` | done-by-reading |
| `tool.rotate`: R | keymap.json:8 | `POST /ops/transform` | `cad::transform::keys` → `CadAction::CadTool { tool: Rotate }` | done-by-reading |
| `tool.scale`: S | keymap.json:8 | `POST /ops/transform` | `cad::transform::keys` → `CadAction::CadTool { tool: Scale }` (not with Ctrl: Ctrl+S saves) | done-by-reading |
| `tool.push_pull`: D | keymap.json:8 | `POST /ops/push_pull` | `cad::transform::keys` → `CadAction::CadTool { tool: PushPull }` | done-by-reading |
| `tool.offset_face`: Shift+D | keymap.json:8 | `POST /ops/offset_faces` | `cad::transform::keys` → `CadAction::CadTool { tool: OffsetFace }` | done-by-reading |
| `tool.box`: Shift+A, B | keymap.json:8 | `POST /ops/box` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.box }` when ready, else the status line says why (a two-step `cad::keys::Chord`: Shift+A, then the key within 1.5 s; `keys::gate` holds the second key for the chord so it does not also run B, select bodies) | done-by-reading |
| `tool.cylinder`: Shift+A, C | keymap.json:8 | `POST /ops/cylinder` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.cylinder }` when ready, else the status line says why (a two-step `cad::keys::Chord`: Shift+A, then the key within 1.5 s; `keys::gate` holds the second key for the chord so it does not also run C, sketch circle) | done-by-reading |
| `tool.sphere`: Shift+A, S | keymap.json:8 | `POST /ops/sphere` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.sphere }` when ready, else the status line says why (a two-step `cad::keys::Chord`: Shift+A, then the key within 1.5 s; `keys::gate` holds the second key for the chord so it does not also run S, the Scale tool) | done-by-reading |
| `tool.extrude`: X | keymap.json:8 | `POST /ops/extrude` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.extrude }` when ready, else the status line says why | done-by-reading |
| `tool.revolve`: Shift+R | keymap.json:8 | `POST /ops/revolve` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.revolve }` when ready, else the status line says why (transform's R refuses Shift) | done-by-reading |
| `tool.fillet`: Ctrl+F (the outliner's search placeholder also names Ctrl+F, which no command binds) | keymap.json:8 | `POST /ops/fillet` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.fillet }` when ready, else the status line says why | done-by-reading |
| `tool.chamfer`: Ctrl+Shift+F | keymap.json:8 | `POST /ops/chamfer` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.chamfer }` when ready, else the status line says why | done-by-reading |
| `tool.shell`: Ctrl+Shift+H | keymap.json:8 | `POST /ops/shell` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.shell }` when ready, else the status line says why | done-by-reading |
| `tool.measure`: M | keymap.json:8 | `POST /ops/add_measurement` | `cad::transform::keys` → `CadAction::CadTool { tool: Measure }` (not with Ctrl: Ctrl+Shift+M is Same Material) | done-by-reading |
| `tool.plane`: Ctrl+P | keymap.json:8 | `POST /ops/plane_from_face` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.plane }` when ready, else the status line says why | done-by-reading |
| `tool.fastener`: Ctrl+H | keymap.json:8 | `POST /ops/fastener_hole` | cad-print epic | later-epic: cad-print |
| `tool.clearance`: Ctrl+Shift+C | keymap.json:8 | `POST /ops/clearance` | cad-print epic | later-epic: cad-print |
| `tool.mirror`: Ctrl+M | keymap.json:8 | `POST /ops/mirror` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.mirror }` when ready, else the status line says why (Control+M always works; a macOS app menu binding Command+M to minimise would take it, and winit's default menu has none) | done-by-reading |
| `tool.array`: Ctrl+Shift+A | keymap.json:8 | `POST /ops/array_rect` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { tool.array }` when ready, else the status line says why | done-by-reading |
| `sketch.line`: L | keymap.json:9 | `POST /nodes/{id}/sketch` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { sketch.line }` when ready, else the status line says why | done-by-reading |
| `sketch.rectangle`: Shift+L | keymap.json:9 | `POST /nodes/{id}/sketch` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { sketch.rectangle }` when ready, else the status line says why | done-by-reading |
| `sketch.circle`: C | keymap.json:9 | `POST /nodes/{id}/sketch` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { sketch.circle }` when ready, else the status line says why (C after Shift+A is the cylinder chord's, `keys::gate`) | done-by-reading |
| `sketch.arc`: A. **Dead**: no command `sketch.arc` exists (the registry has `sketch.arc_3pt`), so ui/app.py:462 skips it; USER_GUIDE.md:181 still says "`A` arc" | keymap.json:9 | n/a | `surfaces::registry` binds A to `sketch.arc_3pt` → `cad::keys` → `CadInvoke { sketch.arc_3pt }` when ready | deliberately different: A runs the three-point arc, the only arc tool; RoboCAD binds A to a command it never registers, so its A does nothing |
| `sketch.polygon`: Shift+P | keymap.json:9 | `POST /nodes/{id}/sketch` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { sketch.polygon }` when ready, else the status line says why | done-by-reading |
| `sketch.slot`: Shift+S | keymap.json:9 | `POST /nodes/{id}/sketch` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { sketch.slot }` when ready, else the status line says why (transform's S refuses Shift) | done-by-reading |
| `sketch.spline`: Shift+C | keymap.json:9 | `POST /nodes/{id}/sketch` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { sketch.spline }` when ready, else the status line says why | done-by-reading |
| `sketch.text`: T | keymap.json:9 | `POST /nodes/{id}/sketch` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { sketch.text }` when ready, else the status line says why (typed while the text field has the keyboard) | done-by-reading |
| `modify.union`: Ctrl+U | keymap.json:10 | `POST /ops/boolean` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { modify.union }` when ready, else the status line says why | done-by-reading |
| `modify.subtract`: Ctrl+Shift+U | keymap.json:10 | `POST /ops/boolean` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { modify.subtract }` when ready, else the status line says why | done-by-reading |
| `modify.intersect`: Ctrl+Alt+U | keymap.json:10 | `POST /ops/boolean` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { modify.intersect }` when ready, else the status line says why | done-by-reading |
| `modify.join`: J | keymap.json:10 | `POST /ops/join` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { modify.join }` when ready, else the status line says why | done-by-reading |
| `modify.unjoin`: Shift+J | keymap.json:10 | `POST /ops/unjoin` | `cad::keys` (matched against `surfaces::registry`) → `CadInvoke { modify.unjoin }` when ready, else the status line says why | done-by-reading |
| `print.wall_check`: Ctrl+W | keymap.json:11 | `GET /nodes/{id}/thin` | cad-print epic | later-epic: cad-print |
| `print.validate`: Ctrl+Shift+V | keymap.json:11 | `GET /nodes/{id}/validate` | cad-print epic | later-epic: cad-print |
| `numeric.entry`: Tab (cleared at ui/app.py:469 and routed by keyPressEvent) | keymap.json:12 | n/a | `cad::numeric::entry` | done-by-reading |
| Keys listed in the registry but never bound, because they are not in keymap.json: `simulation.experiment` Ctrl+Return, `robot.add_motor` Ctrl+Shift+M (pressing it runs Select Same Material), `robot.add_joint` Ctrl+Shift+J (USER_GUIDE.md:368 and USER_GUIDE.md:375 document both) | ui/app.py:276, ui/app.py:408-409, ui/app.py:247-251 | n/a | `cad/surfaces/registry.rs` (`bound`), `cad/keys.rs` (module doc table) | deliberately different: Ctrl+Shift+J is bound to `robot.add_joint` (nothing else uses it; USER_GUIDE.md:375 documents it); Ctrl+Shift+M stays Select Same Material, RoboCAD's live behaviour, and `robot.add_motor` is unbound (menus, palette, the Robot panel); `simulation.experiment` stays cad-experiments-motion's |
| Native addition: the same five shortcuts on Cmd and Ctrl in `cad::keys` (Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z, Delete/Backspace, Home, Ctrl/Cmd+S) | n/a | as above | `cad::keys` | done-by-reading |

## Native surface (no RoboCAD counterpart)

| Feature | RoboCAD source (file:line) | REST route | Native target | Status |
|---|---|---|---|---|
| The viewer's REST commands `state`, `cad_state`, `cad_open`, `cad_select`, `cad_patch`, `cad_delete`, `cad_undo`, `cad_redo`, `cad_save`, `cad_command`, `cad_op`, `cad_refresh`, `cad_fit`, `cad_physical`, `system_ui`; and, with cad-select-transform, `cad_select_mode`, `cad_hover`, `cad_box_select`, `cad_candidates`, `cad_select_all`, `cad_invert_selection`, `cad_select_same_material`, `cad_edges_to_faces`, `cad_tool`, `cad_cancel`, `cad_transform`, `cad_push_pull`, `cad_offset_faces`, `cad_set_dimension`, `cad_numeric`, `cad_measure`; and, with cad-modify, `cad_invoke`, `cad_run`, `cad_form_set`, `cad_form_submit`, `cad_form_cancel`, `cad_surface` (and `cad_state.ops`) | none | the viewer's own REST | `cad::actions::CadAction` and `apply`; the specs in `cad::specs` | done-by-reading |
| `system_ui` controls `cad:undo`, `cad:redo`, `cad:save`, `cad:refresh`, `cad:fit`, `cad:physical`, `cad:delete`, `cad:node:<id>`, `cad:visible:<id>`, `cad:locked:<id>`, `cad:disabled:<id>`, `cad:material:<id>:<mat>`, `cad:command:<id>`, and, with cad-select-transform, `cad:mode:<mode>`, `cad:select_all`, `cad:invert_selection`, `cad:select_same_material`, `cad:edges_to_faces`, `cad:candidate:<n>`, `cad:tool:<tool>`, `cad:cancel`; and, with cad-modify, `cad:op:<id>` (every RoboCAD command), `cad:surface:<kind>`, `cad:menu:<category>`, `cad:form:ok`, `cad:form:cancel`, `cad:form:set:<name>:<value>`; each with enabled and disabled_reason | none | the viewer's own REST | `cad::actions`, `cad::panel::controls` (`own_controls`, then `cad::surfaces::controls`) | done-by-reading |
| "Refresh": refetch `/doc`, `/commands` and `/autosave` now | none | `GET /doc`, `GET /commands`, `GET /autosave` | `CadAction::CadRefresh` → `cad::sync` (`PollCommand::Refresh`) | done-by-reading |
| Stale and lost states: a failed request keeps the last snapshot on screen, marked stale, with the error verbatim | none | `GET /`, `GET /doc` | `cad::document::Connection::Lost`, `CadDocument.stale` | done-by-reading |
| One edit in flight at a time; others are refused, naming it | none | the mutating routes | `cad::document::Edit` | done-by-reading |
| Loopback-only endpoints (`http://127.0.0.1:PORT`, `http://localhost:PORT`) | none | n/a | `sim_runtime::loopback_http::Endpoint::parse` | done-by-reading |
| The self-started service is a child of the viewer: stopped with the document unless it may hold unsaved edits (then detached and left running), reaped by the jobs module; an attached RoboCAD is never stopped | none | n/a | `jobs::ChildProcess`, `CadDocument.child` | done-by-reading |

## Counts

Recounted from the tables above (2026-10-01, after the
cad-physical-inspect epic, commits 3fb34225 and f26842fa and their review
fixes: of its 82 rows, 64 became `done-by-reading` and 18 `deliberately
different`; none stays open; no other row changed status) with
this script, run from the repository
root; it splits each row on its unescaped `|`, takes the status cell and
counts it by its leading status (up to its reason), skipping header rows
and the count tables themselves:

```sh
python3 - docs/cad-parity.md <<'PY'
import re, sys, collections
rows, epics, section = collections.Counter(), collections.Counter(), ""
for line in open(sys.argv[1]):
    if line.startswith("## "): section = line.strip()
    if not line.startswith("|") or line.startswith("|---") or section == "## Counts": continue
    s = re.split(r"(?<!\\)\|", line.rstrip("\n"))[-2].strip()
    m = re.match(r"(done-by-reading|later-epic|deliberately different|done)\b", s)
    if not m: continue
    rows[m.group(1)] += 1
    if m.group(1) == "later-epic": epics[s.split(":", 1)[1].split()[0]] += 1
print(dict(rows), sum(rows.values())); print(dict(epics))
PY
```

There are 773 rows. (After cad-views-export, at f15766ea and still at
f26842fa, the ledger had 367 done by reading, 286 later (82 of them
cad-physical-inspect's) and 120 deliberately different. At 9b1e5eec and f15766ea the ledger was unchanged
from cad-sketch's: an uncommitted draft before the review fixes had 363
done by reading, 292 later (6 of them cad-views-export's own open rows)
and 118 deliberately different; after cad-sketch 286, 398
and 89; after cad-modify 245, 458 and 70; after cad-select-transform
168, 574 and 31; cad-select-transform's 63 rows were 55 and 8.) The
epic's own split was counted by pairing each row of the ledger at
f15766ea (`git show f15766ea:docs/cad-parity.md`, the same ledger as at
9b1e5eec: neither commit changed it) with the same row now, in order and
checked to be in the same section, through the same row parser, and
counting the new status of every row that was `later-epic:
cad-views-export`: 79 and 33 of 112 (an uncommitted draft before the
review fixes had 75, 31 and 6 open); no other row changed status except
2 earlier `deliberately different` rows that waited for it, the per-node
tessellation tolerance and the view radial, which became `done-by-reading`.
cad-physical-inspect's split was counted the same way, pairing each row of
`git show f26842fa:docs/cad-parity.md` with the same row now: of the 82
rows that were cad-physical-inspect's `later-epic` rows, 64 and 18; no other row
changed status (the print overlay row keeps `later-epic: cad-print`
with its new route).

| Status | Rows |
|---|---|
| done | 0 |
| done-by-reading | 431 |
| later-epic | 204 |
| deliberately different | 138 |
| **total** | **773** |

| Later epic | Rows |
|---|---|
| cad-organize | 108 |
| cad-experiments-motion | 63 |
| cad-print | 33 |
| **total** | **204** |

Some rows repeat a feature from another angle: as a UI feature, as a REST
route, as an Ops method and as a key. The ledger checks each of those
surfaces separately. By script, every one of the 183 registry command ids,
the 77 keymap ids and the 134 public Ops methods appears in a row. Other
counts: 81 rows are `n/a (display)` and 10 are flagged (a REST route cell
holding "none: needs a Python route").

No row is blank or `todo`, and no cad-select-transform, cad-modify,
cad-sketch, cad-views-export or cad-physical-inspect row is open. Nothing
is `done`, because no epic's rows have been moved to `done` yet:
cad-mode, cad-select-transform, cad-modify and cad-sketch were built and
tested in their verification passes and await the user's checklist;
cad-views-export and cad-physical-inspect are written and reviewed by
reading, pending their verification passes. The verification passes and the checklist move rows to
`done`.

## Rows flagged "none: needs a Python route"

There are 10 flagged rows covering 10 distinct gaps. Each gap needs one of
two things before the native viewer can reach the feature headless:

- a new route in `cad/robocad/api.py`, a RoboCAD change outside this epic;
- a Rust port, gated by the parity harness.

No route at all (8):

1. Report a failed autosave. `/autosave` does not report one.
   `deliberately different` since cad-views-export: a headless service
   never autosaves.
2. Set the autosave interval preference. `deliberately different` since
   cad-views-export: it is RoboCAD's desktop timer.
3. Reference image pixels in the viewport.
4. The reference list's preview (same data as 3).
5. Run review's captured CAD replay (captured document and poses).
6. Candidate review's proposed geometry.
7. Pose kinematics without a desktop window.
8. Geometry-rule recipes for system components
   (`component_derivation.RECIPES`).

No longer gaps since cad-physical-inspect (2026-10-01; routes added to
`api.py` `Service`, pytests `cad/tests/test_api_physical_routes.py`):
per-node simulation results (`GET /results/nodes`,
`Service.results_nodes`, read-only: the results file's path, loaded,
stale and provenance, each node's `results` block with its material's
yield strength, and `results_margins` per node with print-study blocks
left out), which closed four gaps: the inspector's "Results" line, the
stress overlay's per-node hotspot colours, the print overlay's per-node
results (row kept by cad-print, which reuses the route) and the Robot
panel's margins; and the planar ("x–z") simulation export (`GET
/physical?…&planar=1`: headless `export_physical_model(planar=Plane.xz())`,
in the desktop window `export_snapshot(planar=True)`; without it the
route is unchanged). The same epic corrected `api.py`'s module docstring,
which called the export simrobot v3: `physical.py` `SCHEMA_VERSION` is 4.

No longer a gap: B-rep edge polylines (edge display, edge picking and
curve nodes). RoboCAD now serves them as `GET /nodes/{id}/edges?samples=N`
(api.py:625-638, 2026-10-01); edge picking uses it (cad-select-transform),
and edge display, curve nodes included (drawn, not picked), is
`cad/display/draw.rs:edges_sync` since cad-views-export.

No longer gaps since cad-views-export (2026-10-01; routes added to
`api.py` `Service`, pytests `cad/tests/test_api_files.py`): the save
thumbnail (`POST /save/thumbnail`, `Service.save_with_thumbnail`: the
window's thumbnail, or headless the snapshot renderer at 256 × 192;
every native save uses it, Save and Save As alike) and the
mesh-unit guess (`GET /import/units?path=`, `Service.mesh_units`,
read-only). The same epic added `POST /new` (`Service.new_file`: an empty
`.rcad`, created exclusively so an existing file is never replaced),
which no flagged row needed.

No longer gaps since cad-modify (2026-10-01; routes added to `api.py`
`Service`, pytests `cad/tests/test_api_clipboard.py`,
`test_api_control_points.py`, `test_api_analysis.py`): Copy with Placement
(`POST /clipboard/copy`, `Service.copy`, read-only), Paste with Placement
(`POST /clipboard/paste`, `Service.paste`, one undo step "Paste"), reading
face control points (`GET /nodes/{id}/control_points?face=i`), the
curvature comb (`GET /nodes/{id}/curvature_comb`) and the continuity check
(`GET /nodes/{id}/continuity`). The same epic fixed `ArgConverter` twice,
found by reading: `POST /ops/cut` takes a cutter node id
(`test_api_cut_cutter.py`), and `POST /ops/set_control_points` takes its
grid of points (`test_api_control_points_set.py`); both were refused
before.

GUI-only through `POST /commands/{id}`, with no headless route (2;
both `deliberately different` since cad-views-export):

9. Blender live link start and stop.
10. Web share.

## Headless versus GUI-only routes

CAD mode's own document is a **headless** service (`python -m robocad.api
PATH`, api.py:1390-1405), so it matters which routes need RoboCAD's window:

- `GET /commands` returns `{}` headless (api.py:1050-1052), and
  `POST /commands/{id}` answers 409 "no GUI" (api.py:1055-1057). The native
  panel shows the empty list and the 409 verbatim. Registry commands work
  only when CAD mode is attached to RoboCAD's desktop window.
- `POST /open` opens a **new RoboCAD window**, which serves its own API on
  another port (ui/app.py:1783-1795), and answers 409 headless
  (api.py:966-972). It never replaces the document the request was sent to.
  Its `load_id` is polled at `GET/DELETE /loads/{id}` (GUI only, 409
  headless), and `stats.api_url` names the new window's API. CAD mode does
  not use it: it starts its own service on the file.
- `/autosave` (GET and POST) is GUI-only (409 headless; api.py:390-392). A
  headless service never autosaves at all (only ui/app.py:112 starts
  autosave), so a self-started document's unsaved edits exist only in that
  process until a save (`POST /save`; the viewer sends `POST /save/thumbnail`).
- `/view`, `/view/fit`, `/screenshot`, `/capture`,
  `/views/{id}/restore`, `/threads/{id}/show`, `/motion` playback and
  export, and `/component-jobs/{id}` need the window. `GET /view` answers
  `{}` headless. The rest answer 409.
- In the GUI, `/physical` derives in a child process on a snapshot
  (api.py:1233-1238). Headless, it runs `Ops.physical` under the document
  lock (api.py:1231-1232).
- In the GUI, component operations through `/ops` return `{"job": …}` to
  poll at `/component-jobs/{id}`. Headless, they run synchronously
  (api.py:702-704).
- Every GUI request is marshalled onto RoboCAD's Qt thread and waits up to
  120 s, then answers 504 if it never started (api.py:1333-1348).
  Reads and selection pushes use `REQUEST_TIMEOUT` (30 s) and can time out
  before a slow GUI answers; edits use `EDIT_TIMEOUT` (130 s), longer than
  RoboCAD's 120 s wait, and a timed-out (or otherwise interrupted) edit says
  RoboCAD may still apply it.

## Unsaved edits in a self-started service (decided 2026-09-30)

A headless service never autosaves, and stopping it would lose unsaved
edits. The viewer never saves for the user, so: leaving CAD mode or opening
another document (`cad_open`, File > Open… and File > New, one rule:
`CadDocument::switch_blockers`; there is no discard) is refused while a service this window started reports
`dirty`, or its saved state can't be confirmed (not connected while the
process still runs, or an edit in flight or just finished); closing the window detaches such a
service (`ChildProcess::detach`) instead of stopping it and logs its URL,
so the user can attach to it (`--cad-url`) and save. A clean self-started
service is stopped and reaped. An attached RoboCAD is never stopped.

## Notes found while reading (RoboCAD, not changed)

- `keymap.json:9` binds `sketch.arc` to A, but no such command exists, so A
  does nothing, although USER_GUIDE.md:181 documents it. (The native viewer
  binds A to `sketch.arc_3pt`.)
- The registry's inline keys (`simulation.experiment` Ctrl+Return,
  `robot.add_motor` Ctrl+Shift+M, `robot.add_joint` Ctrl+Shift+J) are never
  bound: `_cmd` sets no shortcut (ui/app.py:247-251), and only keymap.json
  entries are bound. Ctrl+Shift+M runs Select Same Material, though
  USER_GUIDE.md:368 says it adds a motor.
- `POST /undo` and `POST /redo` respond to any method, GET included
  (api.py:1179-1182). A crawler or a prefetching client that GETs `/undo`
  undoes an edit.
- `GET /print/jobs/{id}` reads `wait` from the request body (api.py:282),
  which is never parsed for GET (api.py:1102), so `wait` is never honoured.
- USER_GUIDE.md:188-189 says trim, split, extend and rebuild "live in the
  Sketch menu", but the menu has only offset, fillet and join
  (ui/app.py:379-381). The others are reachable only through
  `POST /nodes/{id}/sketch`.
- USER_GUIDE.md:549 says "Window → Components library". There is no Window
  menu: the "Window" and "General" categories fall into Help
  (ui/app.py:436-439).
- ARCHITECTURE.md:84 says "~130 commands". The registry has 183.
- The properties panel's tessellation tolerance mutates the node directly,
  with no undo step (ui/widgets.py:730-735). `PATCH tessellation_tolerance`
  is undoable (api.py:599-600, api.py:608-610).
- The GUI's Box tool builds a sketch rectangle and extrudes it
  (ui/tools.py:517-520), so its undo label is "Extrude" and it is not
  `Ops.box` (commands.py:419). A parity harness comparing the GUI and
  REST paths should expect that difference.

- Found with cad-select-transform (2026-10-01):
  - A typed rotation uses `axes[self.axis_index or 2]` (ui/tools.py:379),
    so after dragging the X ring (index 0) it turns about Z.
  - The centre snap reads `getattr(it, "centers", [])`
    (ui/viewport.py:1388), but the edge centres are stored only on the
    function (`_display_edges.last_centers`, ui/viewport.py:1654-1656), so
    no centre snap ever fires.
  - `measure_between` tests "two edges" before "the same edge twice"
    (ui/app.py:726-731), so its radius branch is unreachable and one
    circular edge picked twice measures 0 mm between its midpoints.

- Found with cad-sketch (2026-10-01):
  - `io/exporters.py` `_slot_points` sweeps its cap angles the other way
    from the kernel's slot (kernel/sketch.py `_wire`), so RoboCAD's
    viewport and SVG export draw a slot's caps turned inward: `(8.5, 0)`
    for a slot from (0, 0) to (10, 0) of radius 1.5, where the solid has
    `(11.5, 0)`. Not changed; the viewer draws the solid's outline.
  - `ExtrudeTool.release` revolves a full 360° whatever the revolve
    tool's angle field says (`_apply(h, …)` with `angle=None`), and its
    drag always sends `self.taper`, which stays 0.0 (only the Tab commit
    reads the taper field).
  - `Service.edit_sketch` (api.py) turned every list of two numbers into
    a point before mapping curve indices, so a join of two curves, trim
    or extend with two cutting curves, `circle_tangent` and `arc_tangent`
    failed through REST. **Changed**: indices are mapped first
    (`cad/tests/test_api_sketch_calls.py`); route, body and undo label
    unchanged.
  - `edit_sketch` through REST records the label "Sketch (API)" for every
    call list, so RoboCAD's history cannot tell a REST rectangle from a
    REST offset.

- Found with cad-views-export (2026-10-01):
  - RoboCAD's Alt+left-drag orbit never fires: `mouseMoveEvent` orbits on
    Alt+left only while `_tool_dragging` is false (ui/viewport.py:1488),
    but every left press is emitted as a tool "press" whose handler sets
    `_tool_dragging = True` (ui/app.py:540-542) until the release, so the
    drag goes to the tool instead (unless the pose panel is active, where
    `_on_drag` returns first). The native viewer orbits on Alt+left-drag
    past 6 px, as viewport.py's own comment (ui/viewport.py:1485-1487)
    describes it (USER_GUIDE.md:266-270 lists only right-drag orbit).

- Found with cad-physical-inspect (2026-10-01):
  - `api.py`'s module docstring called `GET /physical` "simrobot v3", but
    `physical.py` writes `SCHEMA_VERSION = 4`. **Changed**: the docstring
    says v4.
  - RoboCAD's stress overlay colours linearly from blue at 0 to red at
    yield (ui/viewport.py:826-877), while Robot mode colours the same
    results on a log scale over 3 decades. The native CAD overlay uses
    Robot mode's rule (`sim_domain_robot::stress_results::link_colours`,
    now the one rule for both), so RoboCAD's window and the native viewer
    colour the same results differently (row "Stress overlay").
  - `sim_domain_robot::stress_results` paired a hotspot's cells and
    stresses out of step when a stress was null (Rust; fixed in the same
    epic).
