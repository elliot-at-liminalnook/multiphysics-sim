# CAD mode: side-by-side checklist against RoboCAD

**Current native migration, 2026-10-03:** CD1–CD4 replaces selected opening,
tree, B-rep display, body selection and mass-family inspection paths with
shared Rust and directly called OCCT. The [current source ledger](cad-rust-physical-derivations.md)
supersedes their server-backed traces below and records archive limitations.
All current evidence is source-reviewed and unexecuted. Service startup,
attachment, edits, save, annotations and print traces below are historical;
they do not authorize an active native server path. Unmigrated native controls
refuse with a named Rust migration gap. No checklist or GUI parity is signed off.

**Current repair scope (source-reviewed, unexecuted):** empty component material
overrides follow reference truthiness without bypassing declaration validation.
Local body/face/point picking remains available; edge/vertex modes and exact
topology inspection expose shared Rust migration refusals. Local command/history
sections report migration status rather than waiting for RoboCAD. See the current
source ledger for repair traces and proposed acceptance cases.


This checklist closes the first CAD epic (**cad-mode**, §9 phase 1 of
[docs/architecture/native-viewer.md](architecture/native-viewer.md), section
"CAD mode (2026-09-30)"), the second (**cad-select-transform**: sub-body
selection and the direct tools, Part C), the third (**cad-modify**: the
operation catalogue and the command surfaces, Part D, section "CAD modify
(2026-10-01)"), the fourth (**cad-sketch**: the active plane, the plane
tools, the sketch tools and the solids made from sketches, Part E, section
"CAD sketch (2026-10-01)") and the fifth (**cad-views-export**: the shared
camera, display modes, grid, build plate, view cube, section, isolate and
hide, saved views, the tessellation tolerance, and new, open, save as,
import, export and render, Part F) and the sixth (**cad-physical-inspect**:
materials, the inspector's physical rows, the Robot panel and its tools,
results, the stress overlay, physical export and the live link, Part G,
section "CAD physical properties (2026-10-01)") and the seventh
(**cad-print**: the wall check, validation, overhang shading, the fastener
and clearance tools, split, strength, plan, whole or split, the assembly
guide, coupons, the Print jobs section and the print overlay, Part H,
section "CAD print (2026-10-01)") and the eighth (**cad-organize**: the
outliner's organization, comment threads, reference images and the
linked system file, Part I, section "CAD organize (2026-10-01)"). cad-mode,
cad-select-transform, cad-modify and cad-sketch (at cc7ac194) were built
and tested in their verification passes; cad-views-export was verified at
bcf0c56c; cad-physical-inspect, cad-print, cad-organize and
cad-components were written and checked only by reading. **Parts A to J
are all traced by reading, unexecuted**: every step CAD-01 to CAD-213 has
one path:line trace from the control to RoboCAD's route and back to the
display, written against the current code (the "Reading traces" sections
at the end: Parts A to F in cad-parts-a-f-retrace, Parts G to J in
cad-checklist-traces, CAD-176 to CAD-186 in "Reading traces —
annotations", all 2026-10-02), and the gaps those traces found were fixed
in code or recorded as differences. Each table row links to its trace.
With these traces goal 3 of the user's current focus (the CAD editor, with
annotations held to a high standard) is **complete by reading**. The
earlier executed evidence (cc7ac194, bcf0c56c) predates the code traced
here; none of the current code has been compiled, run or compared side by
side, so the steps below are still to be done once by a person. Each
step is done once in the native viewer and once in RoboCAD's own window,
so you can compare them. The feature-by-feature ledger is
[cad-parity.md](cad-parity.md); its `done-by-reading` rows are the ones
these steps show. Everything else in RoboCAD (the components library and
the system graph, experiments, motion, …) is a later CAD epic and stays in RoboCAD's window.

**RoboCAD stays the reference.** Nothing here changes it, its command layer
or the `.rcad` format. The viewer never writes a `.rcad` file itself.

## Before you start

- **Work on a copy.** These steps edit and save. Copy a model first, for
  example:

  ```sh
  mkdir -p /tmp/cad-check
  cp examples/camera-turntable/cad/turntable.rcad /tmp/cad-check/
  ```

- **The venv.** CAD mode starts RoboCAD's service with
  `cad/.venv/bin/python`. If it is missing, run `cad/run.sh` once (it creates
  the venv and opens RoboCAD); the viewer never creates it.
- **Two ways in.** Part A opens the file directly (the viewer starts a
  headless RoboCAD service for it). Part B attaches the viewer to RoboCAD's
  own window, so both show the same document and selection. RoboCAD's
  registry commands and autosave exist only in Part B (a headless service
  has neither).
- **REST.** Each step names its REST command too (`POST
  http://127.0.0.1:8421/v1/batch`, see `GET /v1/capabilities`); the same
  action runs whichever way you trigger it.

## Part A: a file opened by the viewer

| Step | Native viewer | RoboCAD (`cad/run.sh /tmp/cad-check/turntable.rcad`) | Pass when |
|---|---|---|---|
| CAD-01 Open | `cargo run -p sim-spatial -- /tmp/cad-check/turntable.rcad`. The top bar shows Connecting…; the left dock shows "Connecting: starting RoboCAD's headless service on … (N s)", then "Connected · revision N", and names the self-started service (pid, URL, headless). Also: switcher **CAD**, `system_ui` `mode:cad`, or `viewer_mode {"mode":"cad","path":"/tmp/cad-check/turntable.rcad"}` from another mode | The "Opening turntable.rcad" window, then the document | Both open the same document; the window never froze while the service started Trace: "Reading traces — Parts A and B › CAD-01" (by reading, unexecuted) |
| CAD-02 Tree | The left dock lists every node in RoboCAD's order, indented by parent, with kind, name, "Shown"/"Hidden"/"Hidden by parent"/"Disabled" and "locked" | The outliner | Same nodes, order, nesting and visibility Trace: "Reading traces — Parts A and B › CAD-02" (by reading, unexecuted) |
| CAD-03 Bodies | Every visible body is drawn (Z up, mm shown in metres); Home or **Fit** frames them; right-drag orbits, middle or Shift+right-drag pans, the wheel zooms | The viewport | Same shapes; nothing is drawn for hidden or disabled nodes Trace: "Reading traces — Parts A and B › CAD-03" (by reading, unexecuted) |
| CAD-04 Select | Click a tree row, or a body in the 3D view: the row and the body highlight; `cad_select {"ids":["…"]}` does the same | Click the same part | The inspector follows the selection Trace: "Reading traces — Parts A and B › CAD-04" (by reading, unexecuted) |
| CAD-05 Inspect | The right dock shows the node as RoboCAD returns it: kind, id, parent, material, colour, "Instance of" for instances, transform, body kind, volume mm³, area mm², mass g, centroid, bounding box, size, face and edge counts, then joint/robot/sketch/plane/… fields. A mass value RoboCAD sent as null (or as NaN) reads "null in RoboCAD's answer"; other nulls read "null" | The properties panel | Same numbers and units; nothing in the viewer is filled in that RoboCAD did not send Trace: "Reading traces — Parts A and B › CAD-05" (by reading, unexecuted) |
| CAD-06 Physical labels | Press **Physical** ("Fetching RoboCAD's physical model…", then the link holding the body: mass kg, centre of mass m, inertia kg·m², bounding box, and `mass_sources` for this body as a chip, e.g. a declared measurement's `source`). `cad_physical` | `GET http://127.0.0.1:<port>/physical?flex=0` on the service (port in the left dock) | The chip text is RoboCAD's label exactly; a body without a label shows no chip Trace: "Reading traces — Parts A and B › CAD-06" (by reading, unexecuted) |
| CAD-07 Patch | In the inspector: **Visible**, **Locked**, **Disabled**, a material chip; click the name, type, Enter. The status bar shows "Sending: …", then the tree and inspector update. `cad_patch {"id":"…","attrs":{"visible":false}}` | Same edits in the properties panel | Same result; while one edit is in flight a second is refused naming the first Trace: "Reading traces — Parts A and B › CAD-07" (by reading, unexecuted) |
| CAD-08 Undo / redo | **Undo {label}** / **Redo {label}**, or Cmd/Ctrl+Z and Cmd/Ctrl+Shift+Z (`cad_undo`, `cad_redo`). History in the inspector lists RoboCAD's labels | Edit ▸ Undo / Redo | Same labels; each patch from CAD-07 is one undo step Trace: "Reading traces — Parts A and B › CAD-08" (by reading, unexecuted) |
| CAD-09 Delete | Select a node, **Delete** or Delete/Backspace (not while typing a name). `cad_delete` | Delete | The node goes; undo brings it back Trace: "Reading traces — Parts A and B › CAD-09" (by reading, unexecuted) |
| CAD-10 Unsaved edits | After an edit, the top bar says "Unsaved edits". Press the switcher's **Build** | (n/a) | Refused: "… has unsaved edits in the RoboCAD service this window started …: save first"; CAD mode stays Trace: "Reading traces — Parts A and B › CAD-10" (by reading, unexecuted) |
| CAD-11 Save | **Save** or Cmd/Ctrl+S (`cad_save`; `{"path":"/abs/file.rcad"}` saves as: an absolute path or `~/…`, a relative one is refused naming why). The top bar returns to "Saved"; the status line says "Saved … with its thumbnail". Then `unzip -l` the file | File ▸ Save, then the same `unzip -l` | The file on disk changed (RoboCAD wrote it, through `POST /save/thumbnail`); both archives hold `thumbnail.png`; reopening it in RoboCAD shows the edits; after `cad_save {"path"}` the left dock names the new file (a self-started document follows it) Trace: "Reading traces — Parts A and B › CAD-11" (by reading, unexecuted) |
| CAD-12 Leave | Switcher **Build** now succeeds; the RoboCAD service process is gone (`ps` with the pid from the left dock). Switch back to **CAD**: it starts a new service on the same file | (n/a) | No `robocad.api` process is left behind Trace: "Reading traces — Parts A and B › CAD-12" (by reading, unexecuted) |

## Part B: attached to RoboCAD's window

Open RoboCAD on the copy (`cad/run.sh /tmp/cad-check/turntable.rcad`; its
status bar names its REST port, 8420 unless that was taken), then
`cargo run -p sim-spatial -- --cad-url http://127.0.0.1:8420` (or
`viewer_mode {"mode":"cad","url":"http://127.0.0.1:8420"}`).

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-13 Shared selection | Click a row or body | Click a different part in RoboCAD | Each window shows the other's selection within about a second Trace: "Reading traces — Parts A and B › CAD-13" (by reading, unexecuted) |
| CAD-14 Edits both ways | Hide a node, rename one | Move a part, change a material | Both windows show all four changes; RoboCAD's undo history lists all of them in order Trace: "Reading traces — Parts A and B › CAD-14" (by reading, unexecuted) |
| CAD-15 Commands | The inspector's Commands section lists RoboCAD's registry by category with its keys; run one, e.g. **Fit view** (`cad_command {"id":"view.fit"}`) | The same command from its menu or palette | The command runs in RoboCAD's window Trace: "Reading traces — Parts A and B › CAD-15" (by reading, unexecuted) |
| CAD-16 Autosave | The left dock shows RoboCAD's autosave (running, saved revision, path) | RoboCAD autosaves as usual | The same state Trace: "Reading traces — Parts A and B › CAD-16" (by reading, unexecuted) |
| CAD-17 Leaving keeps edits | With unsaved edits, switch to **Build** | (n/a) | The switch succeeds and its message says RoboCAD keeps the unsaved edits; RoboCAD's window still has them; RoboCAD is not stopped Trace: "Reading traces — Parts A and B › CAD-17" (by reading, unexecuted) |
| CAD-18 Service loss | Back in CAD mode, quit RoboCAD | Quit (save or discard as you like) | The top bar says Lost and the left dock "Not connected: …" with the error; the tree stays on screen; edits are refused naming the lost connection; after reopening RoboCAD on the same port the viewer reconnects within a second (**Refresh** forces it) Trace: "Reading traces — Parts A and B › CAD-18" (by reading, unexecuted) |

## Part C: sub-body selection and the direct tools (cad-select-transform)

Open the viewer on the copy as in Part A, and RoboCAD's own window on a
second copy so its edits don't touch the viewer's:

```sh
cp examples/camera-turntable/cad/turntable.rcad /tmp/cad-check/turntable-ref.rcad
cad/run.sh /tmp/cad-check/turntable-ref.rcad
```

Do each step on the same part in both. Compare items with `cad_state`
(`selection`, `select_mode`, `tool_state`) against `GET /selection` on
RoboCAD's port. A headless service stores the selection items but not the
mode, so the viewer holds its own mode. In Part B's setup (attached) the
selection steps also show up live in RoboCAD's window.

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-19 Selection modes | The strip at the top left of the 3D view: **Bodies**, **Faces**, **Edges**, **Vertices**, **Points**, or the keys B, Shift+B, E, V, P (`cad_select_mode {"mode":"face"}`). The status reads "Selection mode: face" and the tool bar "Select  ·  Face"; edge mode draws every edge faintly, vertex mode marks every vertex | Select ▸ the same modes, same keys | Same modes and keys; a switch clears the selection in both; B, E, V and P do nothing while a text field has the keyboard Trace: "Reading traces — Part C › CAD-19" (by reading, unexecuted) |
| CAD-20 Click, Shift, Ctrl | In each mode click a face, an edge, a vertex, a point; Shift+click adds, Ctrl/Cmd+click toggles, a click on empty space clears (not with Shift or Ctrl). The status reads "n selected", or "Ready" when empty. `cad_select {"items":[["<id>","face",3]],"toggle":true}` | The same clicks | The same items `[node, kind, index]` in both; locked and hidden nodes are never picked and don't hide what is behind them Trace: "Reading traces — Part C › CAD-20" (by reading, unexecuted) |
| CAD-21 Hover | Move the pointer over the model in each mode: the face outline, edge, vertex mark (or, in body mode, the body's box) under it lights in the accent colour | The same | The same item lights; the selection, inspector and status never change from a hover; the view never stutters while hovering edges on a large model Trace: "Reading traces — Part C › CAD-21" (by reading, unexecuted) |
| CAD-22 Box select | Drag more than 6 px: a rubber band; on release, in body (and face, point) mode the bodies whose bounding box lies inside, in edge mode the edges wholly inside, in vertex mode the vertices inside; Shift or Ctrl extends. `cad_box_select {"rect":[x0,y0,x1,y1]}` | The same drag | The same items; locked nodes are taken by the box in both Trace: "Reading traces — Part C › CAD-22" (by reading, unexecuted) |
| CAD-23 Alt menu | In face mode, Alt+click where two faces meet: a list "name: face #i" (nearest first; `system_ui` `cad:candidate:<n>`). Choose one; Escape or a click elsewhere closes it. `cad_candidates` | Alt+click at the same spot | The same entries; the chosen one is selected with the Shift/Ctrl rule of the click Trace: "Reading traces — Part C › CAD-23" (by reading, unexecuted) |
| CAD-24 Select All, Invert, Same Material | Ctrl/Cmd+A, Ctrl/Cmd+Shift+I, Ctrl/Cmd+Shift+M, or the strip's **Select All**, **Invert**, **Same Material** (`cad_select_all`, `cad_invert_selection`, `cad_select_same_material`) | Edit ▸ Select All, Invert Selection, Select Same Material | The same body sets (visible bodies, sheets, curves, instances, meshes). Same Material with nothing selected, or with a body that has no material, is refused naming why (RoboCAD does nothing, or selects every body without a material) Trace: "Reading traces — Part C › CAD-24" (by reading, unexecuted) |
| CAD-25 Edges → faces | In edge mode select two edges, then **Edges → Faces** (`cad_edges_to_faces`) | Edit ▸ "Selection: edges → bounding faces" | The same face indices; the mode becomes Face in both. Before the topology has loaded it is refused naming the node, never guessed Trace: "Reading traces — Part C › CAD-25" (by reading, unexecuted) |
| CAD-26 Inspector: sub-body | Select a face, an edge, a vertex, a point: the right dock opens with "Face 3" / "Edge 7" / "Vertex 2" / "Point on face 4" (kind, area mm², centroid, normal, radius, as RoboCAD returns them; "fetching…" while loading), then the node | `GET /nodes/<id>/faces`, `/edges`, `/vertices` on RoboCAD's port | Every value is RoboCAD's exactly; nothing is filled in Trace: "Reading traces — Part C › CAD-26" (by reading, unexecuted) |
| CAD-27 Move | Select a body, G (`cad_tool {"tool":"move"}`): the gizmo at its pivot. Drag an axis: the body follows, the readout shows "Δ = (dx, dy, dz)  \|d mm\|"; hold Ctrl/Cmd: 10 mm steps; drag the centre handle: moves in the screen plane. Release: one commit. Press Escape during another drag: the body returns and the tool goes back to Select. `cad_transform {"translation":[10,0,0]}` | G, the same drags | The same final placement (`GET /nodes/<id>` transform); the body never jumps back while RoboCAD answers; the cancelled drag sent nothing Trace: "Reading traces — Part C › CAD-27" (by reading, unexecuted) |
| CAD-28 Rotate and scale | R: drag a ring (Ctrl/Cmd: 15° steps). S: drag an axis handle (uniform; Ctrl/Cmd: steps of 0.1). `cad_transform {"axis":[0,0,1],"angle_deg":30}`, `{"scale":1.5}` | R and S, the same drags | The same result for one selected node (pivot: the node's pivot, else its mass centroid). With several nodes selected the pivot is the centre of their drawn bounds, not RoboCAD's mass-weighted centroid (recorded difference) Trace: "Reading traces — Part C › CAD-28" (by reading, unexecuted) |
| CAD-29 Push/pull and offset | D (the mode becomes Face): press a planar face and drag along its normal (Ctrl/Cmd: 10 mm steps); the outline previews it. Release: `push_pull`. Hold Shift at release, or use Shift+D, or pick a cylindrical face: `offset_faces`. `cad_push_pull {"node":"<id>","face":3,"distance":5}`, `cad_offset_faces` | D and Shift+D, the same drags | The same solid (volume and face count in the inspector); a curved face is offset, never pushed Trace: "Reading traces — Part C › CAD-29" (by reading, unexecuted) |
| CAD-30 Numeric bar | During Move, press Tab: fields dx, dy, dz open on the first. Type `20mm + 0.3`: it reads "= 20.3 mm". Type `20mx`: a red border and the error naming the token and its position; Enter is refused. Tab cycles fields; Enter commits once; Escape cancels. Also for Rotate (angle, `45deg`), Scale (factor) and Push/Pull (distance). `cad_numeric {"values":["20mm + 0.3","0","0"]}` | Tab in the same tool, the same text | The same evaluations (also `1in`, `pi*10`) and the same result; RoboCAD also refuses the bad text Trace: "Reading traces — Part C › CAD-30" (by reading, unexecuted) |
| CAD-31 Live dimensions | Select tool, face mode: select a cylindrical face: "Ø name" in the bar; two parallel planar faces: "Distance" (the second moves); two other planar faces: "Angle"; a circular edge: "Ø edge i"; a sphere: "R …" read-only with "use Scale about the centre". Double-click a face (face mode): its diameter, or its distance to the opposite face, focused. Enter commits. `cad_set_dimension {"node":"<id>","dimension":"diameter","faces":[4],"value":8}` | The same selections; double-click the face | The same fields, values and result. RoboCAD also takes the double-click in body mode; the viewer takes it in face mode only (reported, not yet changed) Trace: "Reading traces — Part C › CAD-31" (by reading, unexecuted) |
| CAD-32 Snapping and measure | M: hovering shows the snap marker and "vertex  (x, y, z)" (also midpoint, center, grid, free); hold Alt: always free. Click two points: the tool bar and status show "12.000 mm" (or "R 3.000 mm  (Ø 6.000)", "90.00°"). Shift on the second click keeps it: a measure node appears in the tree. `cad_measure {"a":…,"b":…,"keep":true}` | M, the same clicks; Shift+click | The same snaps and value. RoboCAD also copies the value to the clipboard; the viewer does not (recorded difference). The same circular edge twice gives its radius in the viewer, 0 mm in RoboCAD (a RoboCAD bug) Trace: "Reading traces — Part C › CAD-32" (by reading, unexecuted) |
| CAD-33 One undo step per commit | After CAD-27 to CAD-32, read the inspector's History; Cmd/Ctrl+Z through them | Edit ▸ Undo through the same edits | Each commit (drag release, numeric Enter, dimension Enter, kept measurement) is exactly one step in RoboCAD's history, and each undo reverses exactly one; no preview stays on screen after an undo Trace: "Reading traces — Part C › CAD-33" (by reading, unexecuted) |
| CAD-34 Refused while an edit is in flight | Release a drag, then at once release another (or send `cad_transform` while the first is pending). In Part B, edit in RoboCAD's window during a drag, then release | (n/a) | The second is refused "another CAD edit is in flight: …" and the stale one "the document changed since these values were taken (revision r, now n); nothing was sent: redo the drag, the entry or the form" (a drag the document changed during: "The document changed during the drag …; nothing was sent: drag again"); RoboCAD's history shows only the edits that were sent Trace: "Reading traces — Part C › CAD-34" (by reading, unexecuted) |

## Part D: the modify tools and command surfaces (cad-modify)

Set up as in Part C: the viewer on one copy, RoboCAD's own window on a
second copy, the same part selected in both. After each step compare the
tree, the inspector's History (RoboCAD's undo labels) and the result's
volume and face count in the inspector with RoboCAD's (`GET
/nodes/<id>` on RoboCAD's port), then undo in both before the next step.
Each operation is also in the native menu bar (the tabs under CAD mode's
header, RoboCAD's categories), the palette and REST (`cad_invoke
{"id":"<command id>"}` as the menu does, or `cad_run {"id", "params"}`);
`cad_state.ops` shows the open form, the active operation and the
catalogue. Leave the active plane unset in both windows for these steps
(RoboCAD's default; the viewer's operations then use RoboCAD's fallback
planes, YZ for mirror and XY for the rest); Part E repeats the
plane-dependent ones with a plane active.

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-35 Box (corner) | Toolbar **Box**, Create ▸ Box (corner), or Shift+A then B. Press on the ground, drag the base, release, move to drag the height, click. The preview shows the base and top in light blue and the readout "20 mm × 20 mm × 10 mm". Again, but press Tab during the drag: the form's corner takes the press point; type width 30, depth 15, height 5 and press Enter. `cad_run {"id":"tool.box","params":{"corner":[0,0,0],"width":30,"depth":15,"height":5}}` | The toolbar's Box, the same drag; Tab, the same sizes, Enter | The same box (corner, size). The undo label is "Box" in the viewer and "Extrude" in RoboCAD (recorded difference: the viewer sends one `Ops.box`); the node is named "Box" in both. Snapping to a vertex starts the base there in both Trace: "Reading traces — Part D › CAD-35" (by reading, unexecuted) |
| CAD-36 Box (centre) | Create ▸ Box (centre) (palette "centre"): the same drag from the centre; Tab sizes | Create ▸ Box (centre) | The base is centred on the press point and sits on the plane (not centred in height) in both Trace: "Reading traces — Part D › CAD-36" (by reading, unexecuted) |
| CAD-37 Cylinder | Toolbar **Cylinder** or Shift+A, C: drag the radius, then the height (a downward drag builds down); Tab: diameter, height | The same | The same base, axis direction, diameter and height; the readout "Ø 10 mm × 10 mm" Trace: "Reading traces — Part D › CAD-37" (by reading, unexecuted) |
| CAD-38 Sphere | Toolbar **Sphere** or Shift+A, S: press the centre, drag the radius, release (it finishes on release); Tab: diameter | The same | The same centre and radius; S as the chord's second key does not also pick the Scale tool Trace: "Reading traces — Part D › CAD-38" (by reading, unexecuted) |
| CAD-39 Fillet | Ctrl/Cmd+F (toolbar **Fillet**): the mode becomes Edge, the form opens at the 3D view's top right with "radius 1.0" and the hint "fillet: select edges (click adds) then type the size • Enter applies". Click two edges of one body and one of another (a second click on an edge removes it); Tab, type `2`, Enter. `cad_run {"id":"tool.fillet","params":{"radius":"2 mm"},"items":[["<id>","edge",3]],"revision":<the revision read>}` (items naming faces or edges need `revision`) | Ctrl+F, the same edges, Tab, 2, Enter | The same fillets. History shows one "Fillet" step per body in both; the selection clears and the tool stays active in both. With no edge picked, Enter is refused "Select one or more edges first" in both Trace: "Reading traces — Part D › CAD-39" (by reading, unexecuted) |
| CAD-40 Variable and chordal fillet | Modify ▸ Variable fillet (start radius, end radius) and Modify ▸ Chordal fillet (chord) on the same edges | The same menu entries | The same results and one step per body Trace: "Reading traces — Part D › CAD-40" (by reading, unexecuted) |
| CAD-41 Chamfer | Ctrl/Cmd+Shift+F: pick edges; distance 1.5, angle 45 → Enter; again with angle 30 | The same | The same chamfers. At 45° only the distance is sent (`cad_state` edit label "Chamfer …: distance 1.5 mm"), at 30° the angle too, as RoboCAD Trace: "Reading traces — Part D › CAD-41" (by reading, unexecuted) |
| CAD-42 Fillet all | Select a body, Modify ▸ Fillet all edges…: a modal form "Radius (mm):" 1.0 (0.01 to 100); type 0.5, OK. With nothing selected the entry is disabled and says "Select the bodies to fillet" | Modify ▸ Fillet all edges…, 0.5 | The same result, one "Fillet all" per selected body. RoboCAD opens its dialog even with nothing selected and then does nothing (recorded difference) Trace: "Reading traces — Part D › CAD-42" (by reading, unexecuted) |
| CAD-43 Full round | Edge mode: select two opposite edges of one face, Modify ▸ Full round (two edges); then try one edge, and edges of two bodies | The same | The same full round; the bad selections are refused "Select two edges of the same body" in both Trace: "Reading traces — Part D › CAD-43" (by reading, unexecuted) |
| CAD-44 Remove fillets | Face mode: select fillet faces (on two bodies), Modify ▸ Remove fillets (selected faces) | The same | The same faces removed, one step per body; with no face selected the viewer refuses "Select the fillet faces to remove" (RoboCAD silent) Trace: "Reading traces — Part D › CAD-44" (by reading, unexecuted) |
| CAD-45 Shell | Ctrl/Cmd+Shift+H (toolbar **Shell**): the mode becomes Face; click the top face (it toggles), type wall 2, Enter. Also with no face (a closed shell) | Ctrl+Shift+H, then select the face with the Select tool first, wall 2, Enter | The same hollow body. In RoboCAD a face click while the shell tool is active selects nothing (its `ShellTool.press` only toggles edges), so pre-select the face there; the viewer toggles faces (recorded difference) Trace: "Reading traces — Part D › CAD-45" (by reading, unexecuted) |
| CAD-46 Thicken | Select a sheet (and a body: it is ignored), Modify ▸ Thicken sheet…: "Thickness (mm):" 2.0 | The same | The same solid; with no sheet selected both refuse "Select a sheet" Trace: "Reading traces — Part D › CAD-46" (by reading, unexecuted) |
| CAD-47 Draft | Face mode: select side faces, Modify ▸ Draft faces…: "Angle (degrees):" 2.0 (−45 to 45) and a neutral plane ("active": XY with no plane active) | The same, no active plane | The same draft, pull +Z about XY. The viewer's form also offers XY, XZ and YZ by name; with a plane active both use it (CAD-95) Trace: "Reading traces — Part D › CAD-47" (by reading, unexecuted) |
| CAD-48 Delete faces | Face mode: select faces, Modify ▸ Delete faces (heal) | The same | The same healed body, one step per body; the selection clears in both Trace: "Reading traces — Part D › CAD-48" (by reading, unexecuted) |
| CAD-49 Mirror and live mirror | Select bodies, Ctrl/Cmd+M; then Modify ▸ Mirror as live instance. REST `cad_run {"id":"tool.mirror","params":{"plane":"xz"}}` mirrors about XZ | Ctrl+M with no active plane; Mirror as live instance | The same mirrored copies about YZ; the live one follows its source in both (move the source to check) Trace: "Reading traces — Part D › CAD-49" (by reading, unexecuted) |
| CAD-50 Array | Ctrl/Cmd+Shift+A: the modal Array form (Kind rectangular: Count X/Y/Z, Mode "count + spacing" or "count + total extent", Spacing or extent X / Y / Z; As live instances; Merge into one body). 3 × 2 × 1 at 10, 10, 10, OK; again with extent; again Kind radial (count 6, total 360, axis plane XY) | Ctrl+Shift+A, the same dialog values | The same copies and positions; the rows switch with the kind in both. The viewer's radial axis is the chosen plane's normal through the origin (RoboCAD: the active plane's) Trace: "Reading traces — Part D › CAD-50" (by reading, unexecuted) |
| CAD-51 Instance | Select two bodies, Modify ▸ Instance selected | The same | One instance per body, each offset +20 mm in X, one step each; nothing selected: the viewer refuses "Select the bodies to instance" (RoboCAD silent) Trace: "Reading traces — Part D › CAD-51" (by reading, unexecuted) |
| CAD-52 Make unique | Select an instance: Modify ▸ Make instance unique, or right-click in the 3D view ▸ Make unique (bake instance) | The outliner's right-click ▸ Make unique (bake instance), or Modify ▸ Make instance unique | The instance becomes a body in both; a selection without an instance is refused "Select an instance to make unique" in the viewer (RoboCAD skips it silently); the viewer offers the entry in the 3D view's menu, RoboCAD in the outliner's Trace: "Reading traces — Part D › CAD-52" (by reading, unexecuted) |
| CAD-53 Set pivot at cursor snap | Select a body, point at a vertex of another body (the snap marker shows it), then the palette ▸ "Set pivot at cursor snap" (Help menu, as RoboCAD's "Tools" category). `cad_run {"id":"tool.set_pivot","params":{"point":[0,0,10]}}` | Help ▸ Set pivot at cursor snap with the cursor on the same vertex (use its palette with the pointer there) | The inspector's pivot reads the vertex in both. Over a face with no snap point the viewer takes the point on the face (RoboCAD takes the grid or plane point; recorded difference) Trace: "Reading traces — Part D › CAD-53" (by reading, unexecuted) |
| CAD-54 Inspector pivot and transform | Select a node: the inspector's pivot field; press it, type `10, 0, 5mm + 1`, Enter; **Clear pivot**. Select an instance: its translation, axis, angle and scale fields; change the angle to `30deg`. A bad value (`10, x, 0`) keeps the field open with the error naming the token | `PATCH /nodes/<id> {"pivot": [10,0,6]}` and `{"transform": …}` on RoboCAD's port, or its properties panel | One undo step each with RoboCAD's result; a component member's pivot and an occurrence's transform show RoboCAD's refusal instead of the field Trace: "Reading traces — Part D › CAD-54" (by reading, unexecuted) |
| CAD-55 Delete as one step | Select three bodies, Delete (or Backspace, the **Delete** button, Edit ▸ Delete) | Delete with the same selection | All three go in one undo step "Delete" in both; Undo brings all three back Trace: "Reading traces — Part D › CAD-55" (by reading, unexecuted) |
| CAD-56 Union, subtract, intersect | Select the target, then Shift-select the tools; Ctrl/Cmd+U, Ctrl/Cmd+Shift+U, Ctrl/Cmd+Alt+U (toolbar **Union**, **Subtract**). With one body selected: the status reads "Union: Select the target body first, then the tools" | The same selections and keys | The same result on the first selected body; the tools are removed and the selection clears in both; RoboCAD's message is the same Trace: "Reading traces — Part D › CAD-56" (by reading, unexecuted) |
| CAD-57 Region | Two overlapping bodies, Modify ▸ Region (overlap as new body); then with three | The same | A new "Region" body in both; three are refused "Select exactly two bodies" in both Trace: "Reading traces — Part D › CAD-57" (by reading, unexecuted) |
| CAD-58 Join and unjoin | Two bodies, J; then select the result, Shift+J | The same | The same joined body and the same parts after unjoin; J with one body is refused in the viewer ("Select two or more bodies to join"), RoboCAD calls join anyway Trace: "Reading traces — Part D › CAD-58" (by reading, unexecuted) |
| CAD-59 Dissolve | A body with redundant edges (after a union), Modify ▸ Dissolve redundant topology | The same | The same face count after, one step per body Trace: "Reading traces — Part D › CAD-59" (by reading, unexecuted) |
| CAD-60 Cut | Select a body, Modify ▸ Cut with active plane (with no plane active both cut with XY; `cad_run {"id":"tool.cut_plane","params":{"plane":"yz"}}` names another); then select a body and a sheet, Modify ▸ Cut with selected sheet/curve | The same, no active plane; the same body and sheet | The same pieces. The sheet cut works headless only since this epic's `ArgConverter` fix (`cad/tests/test_api_cut_cutter.py`) Trace: "Reading traces — Part D › CAD-60" (by reading, unexecuted) |
| CAD-61 Split faces | Select a body crossing z = 0, Modify ▸ Split faces with active plane | The same | The same faces split along XY Trace: "Reading traces — Part D › CAD-61" (by reading, unexecuted) |
| CAD-62 Imprint | A body, then a curve or body touching it, Modify ▸ Imprint selected curve/body | The same | The same imprinted edges; one node refused "Select the body, then the tool" in both Trace: "Reading traces — Part D › CAD-62" (by reading, unexecuted) |
| CAD-63 Project curve | A curve or sketch, then a body; orbit to look along −Z; Modify ▸ Project curve onto body | The same, from the same direction | The same projected curve; the direction is the view's in both (`cad_state` edit label), or REST's `direction` Trace: "Reading traces — Part D › CAD-63" (by reading, unexecuted) |
| CAD-64 Silhouette | Select a body, Modify ▸ Silhouette onto active plane | The same | The same silhouette curve on XY Trace: "Reading traces — Part D › CAD-64" (by reading, unexecuted) |
| CAD-65 Control points | Face mode: a curved face, Advanced ▸ Show/edit control points (advanced) | The same | The same points and rows in RoboCAD's pink, and the status "N control points (edit via Ops.set_control_points; …)"; nothing is written. Changing the document clears the overlay in both Trace: "Reading traces — Part D › CAD-65" (by reading, unexecuted) |
| CAD-66 Raise degree | The same face, Advanced ▸ Raise face degree | The same | One "Raise degree" step in both; the face is 4 × 4 after (`tool.control_points` again) Trace: "Reading traces — Part D › CAD-66" (by reading, unexecuted) |
| CAD-67 Rebuild face | Advanced ▸ Rebuild face…: "Spans per direction:" 4 (1 to 64) | The same | The same rebuilt face Trace: "Reading traces — Part D › CAD-67" (by reading, unexecuted) |
| CAD-68 Dependent offset | Select a face, then Shift-select another body; Modify ▸ Dependent offset (face to body)…: "Clearance (mm):" 0.2 (−10 to 10) | The same | The same offset face; a face alone is refused "Select a face, then the body to offset it to" in both Trace: "Reading traces — Part D › CAD-68" (by reading, unexecuted) |
| CAD-69 Copy and paste with placement | Select two bodies, Ctrl/Cmd+C: "Copied 2 item(s) with placement"; Ctrl/Cmd+V: two new bodies in place, one undo step "Paste". `cad_state.ops.clipboard` shows the copy. Copy in the viewer, then paste in RoboCAD's window | Ctrl+C, Ctrl+V | The same new nodes at the same placement, one step each time. The viewer's clip stays in the viewer: it does not reach RoboCAD's window or the OS clipboard (recorded difference) Trace: "Reading traces — Part D › CAD-69" (by reading, unexecuted) |
| CAD-70 Curvature comb | Select a curve, Inspect ▸ Curvature comb on selected curve | The same | The same comb lines (scale 5, 48 samples) in RoboCAD's violet; a sketch shows none in both; with a curve then a sketch selected both draw the curve's Trace: "Reading traces — Part D › CAD-70" (by reading, unexecuted) |
| CAD-71 Continuity | Select a filleted body, Inspect ▸ Continuity check (G0/G1/G2) | The same | The same edge colours (G0 red, G1 amber, G2 green, boundary grey) and the status "Continuity: {'G0': …, 'G1': …, 'G2': …, 'boundary': …}" Trace: "Reading traces — Part D › CAD-71" (by reading, unexecuted) |
| CAD-72 Toolbar | Under CAD mode's header: the menu tabs, then RoboCAD's 25 tools in order. Hover each: the hint names its keys and, when disabled, why. Activate Move, then Fillet: their buttons light. Scroll the row with the wheel on a narrow window | RoboCAD's toolbar | The same buttons in the same order; all 25 run natively as in RoboCAD (none is disabled for a later epic); Qt folds the overflow behind "»" where the viewer scrolls (recorded difference) Trace: "Reading traces — Part D › CAD-72" (by reading, unexecuted) |
| CAD-73 Right-click menu | Right-click (without dragging) on the 3D view: Annotate, Comments panel, Push/Pull face, Fillet, Chamfer, Hollow / shell, Union, Subtract, Mirror, Array…, Measure, Isolate, Hide, Delete; with an instance selected also Make unique (bake instance). `cad_surface {"surface":{"kind":"context"}}` | Right-click in RoboCAD's viewport | The same entries in order; each enabled by the selection (Annotate, Comments panel, Isolate and Hide run natively); a right drag still orbits Trace: "Reading traces — Part D › CAD-73" (by reading, unexecuted) |
| CAD-74 Radial menus | Space: the view pie at the pointer (Front, Top, Right, Iso, Ortho, Grid, Mode, Fit): hover highlights, release on Fit frames the model; every view entry runs. Q: Body, Face, Edge, Vertex, Point; release on Edge switches the mode. Escape or the dead centre closes; a press outside closes | Space and Q in RoboCAD's viewport | The same entries and layout (first at the top, then clockwise); the viewer's pills are rounded rectangles where RoboCAD's are ellipses (recorded difference); Space types a space while a text field has the keyboard Trace: "Reading traces — Part D › CAD-74" (by reading, unexecuted) |
| CAD-75 Palette | Control+Space (Command+Space is Spotlight on macOS) or Shift+F: "Type a command… (Ctrl+Space)". Type `fil`: "Fill / patch selected curve" (runnable since cad-sketch), then Fillet and Fillet all edges… (sorted by score, then label, as RoboCAD); Up/Down; Enter runs the highlighted row. Type `same`: "Edit: Select Same Material    [Ctrl+Shift+M]  ⚠ conflicts with Robot: add motor from library…". Rows RoboCAD's viewer does not port are marked and disabled; GUI-only rows read "(GUI-only)" | Ctrl+Space, the same queries | The same ranking and the same conflict warning; the viewer also lists the commands it does not port, marked and disabled Trace: "Reading traces — Part D › CAD-75" (by reading, unexecuted) |
| CAD-76 Menus by category | Click each tab: File, Edit, View, Select, Create, Sketch, Modify, Planes, Inspect, Print, Advanced, Outliner, Robot, Bridge, Simulation, Help; each lists its commands with keys; "General", "Window" and "Tools" commands (Command palette, Numeric entry, Select tool, Move, Set pivot at cursor snap, …) are in Help. `cad_surface {"surface":{"kind":"menu","category":"Modify"}}` | RoboCAD's menu bar | The same menus, entries, order and keys; a click runs the command and closes the menu Trace: "Reading traces — Part D › CAD-76" (by reading, unexecuted) |

## Part E: the active plane, sketches and solids from sketches (cad-sketch)

Set up as in Part C and D: the viewer on one copy, RoboCAD's own window on
a second copy. Before each step set the same active plane in both
(Planes ▸ Active plane: XY unless the step says otherwise). After each
step compare the tree, the inspector's History and the result: a sketch's
curves with `GET /nodes/<id>/sketch` on each service (the viewer's port is
in the left dock), a solid's volume and face count in the inspector, a
plane's origin and normal with `GET /nodes/<id>`. Undo in both before the
next step. `cad_state.plane` shows the active plane (label, argument,
frame, 2D snapping) and `cad_state.ops.sketch` the shape in progress.
RoboCAD's undo history labels the viewer's sketch edits "Sketch (API)"
where its own GUI writes "Sketch rectangle" (its `SketchTool` label); compare the curves, not that
label.

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-77 Active plane XY / XZ / YZ | Planes ▸ Active plane: XZ (or the palette, or `cad_invoke {"id":"tool.plane_xz"}`): the status reads "Active plane set", a translucent ±60 mm square is drawn in XZ, `cad_state.plane` reads XZ. Then YZ, then XY | Planes ▸ Active plane: XZ, YZ, XY | The same status line; a later sketch or primitive lands on the same plane in both. RoboCAD draws no square for a named plane (the viewer does; recorded) Trace: "Reading traces — Part E › CAD-77" (by reading, unexecuted) |
| CAD-78 2D snapping | Planes ▸ Toggle 2D snapping to the active plane: "2D snapping on". With XZ active, M (measure) and hover a vertex off the plane: the marker sits projected onto XZ; away from geometry the readout says "grid" or "plane". Toggle again: "2D snapping off" | Planes ▸ Toggle 2D snapping to the active plane; M, the same hover | The same status lines and the same snapped points Trace: "Reading traces — Part E › CAD-78" (by reading, unexecuted) |
| CAD-79 Plane from face | Ctrl/Cmd+P (Planes ▸ Plane from face): the mode becomes Face and the hint "Click a face". Click a body's top face: a plane node appears in the tree, "Active plane set", its square is brighter than the other planes'; the tool stays active, Escape ends it. `cad_run {"id":"tool.plane","items":[["<id>","face",3]],"revision":N}` | Ctrl+P, click the same face | The same plane origin and normal, one undo step each, the new plane active in both Trace: "Reading traces — Part E › CAD-79" (by reading, unexecuted) |
| CAD-80 Plane from three points, two points and midplane | Planes ▸ Plane from three points: the mode becomes Vertex; click three vertices (the status counts "(1 of 3)", "(2 of 3)"). Planes ▸ Plane from two points (camera): two vertices, the view's direction. Planes ▸ Midplane between two faces: two parallel faces | The same menu entries and clicks, the same camera direction | The same planes, each active after it is made Trace: "Reading traces — Part E › CAD-80" (by reading, unexecuted) |
| CAD-81 Selecting a plane node | Click a plane node in the tree (one node selected): it becomes the active plane ("Active plane set") and its square brightens; a sketch drawn next goes on it | RoboCAD has no such gesture: make the same plane active by re-running its plane tool on the same picks | The same active plane (the viewer's gesture is a recorded native addition) Trace: "Reading traces — Part E › CAD-81" (by reading, unexecuted) |
| CAD-82 Line, chaining | L (Sketch ▸ Sketch: Line): click three points on the plane; the preview follows the cursor in light blue and the readout reads "length L  angle A"; two connected lines. Escape ends the chain. Then L, one click, Tab: length 20, angle 30, Enter | L, the same clicks; Tab 20, 30, Enter | The same curves in the same sketch. The first shape created the sketch in the viewer (undo steps "Sketch", then the shape); RoboCAD created it when the tool started Trace: "Reading traces — Part E › CAD-82" (by reading, unexecuted) |
| CAD-83 Rectangle, centre rectangle, circles, arc | Shift+L (two corners), Sketch: Rectangle (centre), C (centre, rim; Tab diameter), Sketch: Circle (two points), Sketch: Circle (three points), A (Sketch: Arc (three points)). Three collinear points for the arc are refused "the three points are collinear" | The same tools and clicks (RoboCAD has no key for the arc: use Sketch ▸ Sketch: Arc (three points); its A does nothing) | The same curves; the collinear arc is refused in both (RoboCAD after its kernel call); OK on a tool with no Tab values is refused by name in the viewer (RoboCAD records an empty step) Trace: "Reading traces — Part E › CAD-83" (by reading, unexecuted) |
| CAD-84 Polygon sides memory | Shift+P: Tab, radius 10, sides 8, Enter (an octagon at the plane origin). Shift+P again: the sides field opens at 8; click centre and corner: an octagon turned toward the second click | Shift+P, Tab 10, 8, Enter; Shift+P again | The same octagons; both remember 8 sides Trace: "Reading traces — Part E › CAD-84" (by reading, unexecuted) |
| CAD-85 Slot, ellipse, spiral | Shift+S: two clicks for the axis, a third for the width; Sketch: Ellipse: centre, x radius, y radius; Sketch: Spiral: centre and radius (3 turns); each again with Tab values | The same tools and clicks | The same curves from `GET /nodes/<id>/sketch`. The slot's caps are drawn bulging outward in the viewer and inward in RoboCAD's viewport; extrude both (CAD-91) and compare the solids, which match Trace: "Reading traces — Part E › CAD-85" (by reading, unexecuted) |
| CAD-86 Spline | Shift+C: click four points, Enter: one spline. Again, finishing with a double-click (within 400 ms and 5 px). Enter with one point does nothing; the form's OK is refused by name | Shift+C, the same clicks, Enter; then a double-click | The same splines in both Trace: "Reading traces — Part E › CAD-86" (by reading, unexecuted) |
| CAD-87 Text | T (Sketch ▸ Sketch: Text): the form's "Text to sketch:" field has the keyboard; type `RC`, Enter, click on the plane: outlines 10 mm high. Again with Tab height 5, Enter. With the field empty a click is refused "type the text to sketch first …" | T: the "Text to sketch:" dialog, `RC`, OK, click; then Tab height 5 | The same outlines. The viewer's preview is a placeholder box (recorded) Trace: "Reading traces — Part E › CAD-87" (by reading, unexecuted) |
| CAD-88 Tab, Enter and Escape in a sketch tool | During a rectangle, after the first click: Tab focuses the first field, Enter commits the typed values at the first click; Enter outside the fields does nothing; Escape drops the shape (nothing sent) and ends the tool | The same keys | The same results; no edit after Escape in either Trace: "Reading traces — Part E › CAD-88" (by reading, unexecuted) |
| CAD-89 Offset, fillet corners, join | Select the sketch with the rectangle: Sketch ▸ Sketch: offset selected curve… "Distance (mm):" 1.0; Sketch ▸ Sketch: fillet corner… "Radius (mm):" 2.0 (four corners rounded); again with 50 (refused: no corner takes it); two lines end to end, Sketch ▸ Sketch: join curves; join on a sketch of one curve is refused | The same Sketch menu entries and values | The same curves (labels "Offset curves", "Fillet corners", "Join curves" in RoboCAD's history for its own edits). RoboCAD records an empty step for the radius 50 and the one-curve join where the viewer refuses by name Trace: "Reading traces — Part E › CAD-89" (by reading, unexecuted) |
| CAD-90 `cad_sketch` (REST) | `cad_sketch {"node":"<sketch>","calls":[["join",[[0,1]]]],"revision":N}`; `[["trim",[0,[1],[5.0,2.5]]]]`; a call naming curve 9 of 3 is refused naming the call and argument; without `node`, `{"plane":"xz","calls":[["circle",[[0,0],5]]]}` goes to the XZ sketch RoboCAD's tools would pick, or a new one | `POST http://127.0.0.1:<RoboCAD port>/nodes/<id>/sketch {"calls": …}` with the same calls | The same curves and one "Sketch (API)" step per call list in both; a join of two curves and a two-curve trim work through REST in both (since the api.py fix; `cd cad && .venv/bin/pytest -q tests/test_api_sketch_calls.py`) Trace: "Reading traces — Part E › CAD-90" (by reading, unexecuted) |
| CAD-91 Extrude, taper, Shift/Ctrl/Alt | A sketch on a body's top face, selected; X: the hint names the modifiers. Drag up: the profile's outline at the base and the top, the readout "extrude h"; release: a new body. Again holding Ctrl at the release: unites with the body under the selection; Shift: subtracts; Alt: intersects. Tab: distance 5, taper 10, Enter. `cad_run {"id":"tool.extrude","params":{"distance":"5","taper":"0","boolean":"union"}}` | X, the same drags and modifiers; Tab 5, 10, Enter | The same volumes and face counts. Both drags send taper 0 (only Tab sends the taper). RoboCAD previews a shaded body (the viewer draws outlines without the taper) Trace: "Reading traces — Part E › CAD-91" (by reading, unexecuted) |
| CAD-92 Revolve | A sketch off the axis, Shift+R: Tab, angle 180, Enter: a half revolution about the sketch plane's x axis. Press and release in the view: a full 360° revolution (the readout says so) | Shift+R, Tab 180, Enter; then press and release in the view | The same solids: both revolve 360° on a click whatever the angle field says Trace: "Reading traces — Part E › CAD-92" (by reading, unexecuted) |
| CAD-93 Sweep, pipe, loft, fill | Create ▸ Sweep (profile + path from selection): select the profile, then the path; "Twist (degrees):" 0. Create ▸ Pipe along selected curve… "Diameter (mm):" 4.0. Create ▸ Loft selected sketches with two sketches on parallel planes. Create ▸ Fill / patch selected curve on a closed curve. Each with too few selected | The same Create entries, selections and values | The same solids; the same refusals ("Select the profile sketch, then the path sketch", "Select a curve or sketch", "Select two or more sketches to loft", "Select a closed curve") Trace: "Reading traces — Part E › CAD-93" (by reading, unexecuted) |
| CAD-94 Primitives on the active plane | Active plane XZ: Box (corner), Box (centre), Cylinder, Sphere drags (CAD-35 to CAD-38) | The same with XZ active | The same solids on XZ (box volumes and positions match); the box's undo label is "Box" in the viewer and "Extrude" in RoboCAD (recorded) Trace: "Reading traces — Part E › CAD-94" (by reading, unexecuted) |
| CAD-95 Plane-dependent operations | With the CAD-79 plane active: Ctrl/Cmd+M (mirror), Modify ▸ Cut with active plane, Split faces with active plane, Silhouette onto active plane, Draft faces… (neutral "active"), Array… radial | The same with the same plane active | The same results about that plane in both Trace: "Reading traces — Part E › CAD-95" (by reading, unexecuted) |
| CAD-96 Toolbar and right-click menu | The toolbar's Rectangle, Circle, Slot and Extrude are enabled; Rectangle lights while its tool is active. Right-click the 3D view: RoboCAD's 14 entries, then a "Sketch" section with the 13 sketch tools; each starts its tool | RoboCAD's toolbar and right-click menu | The same tools start from the same buttons; RoboCAD's sketch buttons never light and its menu has no Sketch section (both recorded native additions) Trace: "Reading traces — Part E › CAD-96" (by reading, unexecuted) |
| CAD-97 A shape in progress blocks leaving | Click two points of a slot, then the switcher's **Build**: refused "a sketch slot is in progress (2 point(s) clicked): finish it or press Escape". After a line the chained point counts too. Escape, then **Build** succeeds (with no unsaved edits, or after saving) | (n/a: RoboCAD has one window; changing tool drops the points) | Refused while points are clicked; nothing sent Trace: "Reading traces — Part E › CAD-97" (by reading, unexecuted) |
| CAD-98 Undo through the epic | Ctrl/Cmd+Z through CAD-79 to CAD-93 | Edit ▸ Undo through the same edits | Each plane, shape, sketch edit and solid is one step in RoboCAD's history (a new sketch adds its "Sketch" step), and each undo reverses exactly one Trace: "Reading traces — Part E › CAD-98" (by reading, unexecuted) |

## Part F: views, display, saved views and files (cad-views-export)

Set up as in Part C and D: the viewer on one copy, RoboCAD's own window on
a second copy, the same node selected in both; also copy two files to
import:

```sh
cp examples/actuators/hx30hm/fixture-draft/print-kit.step examples/actuators/hx30hm/fixture-draft/cap.stl /tmp/cad-check/
```

Menus are named as both menu bars show them (View, Inspect, Print, File).
The camera and display are each window's own: nothing in CAD-99 to
CAD-111 or CAD-129 changes the document or the other window (CAD-130
adds one curve node, undone before the next step). `cad_state.display`,
`cad_state.views` and `cad_state.files` show the viewer's display state,
saved views and file jobs; `camera_state` shows its camera. Steps that
edit (CAD-112 to CAD-117) are undone in both before the next step. The
files written in CAD-118 to CAD-126 go to `/tmp/cad-check/`; the viewer's
file commands take an absolute path (or `~/…`) typed into its path form.

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-99 Orbit, pan, zoom to the cursor | Right-drag orbits (turntable; the view stops short of straight down, 89.5°); middle-drag or Shift+right-drag pans; the wheel zooms toward the point under the cursor: put the cursor on a corner of a part and roll in. REST `camera_orbit {"dx":100,"dy":0}`, `camera_pan`, `camera_zoom {"factor":0.8,"at":[x,y]}` | The same drags and wheel over the same corner | The model turns, slides and zooms the same way and at a similar rate; the corner under the cursor stays under it while zooming in both. RoboCAD's other gestures are CAD-129 Trace: "Reading traces — Part F › CAD-99" (by reading, unexecuted) |
| CAD-100 Named views | 1 front, 3 right, 7 top, 0 iso; Ctrl/Cmd+1 back, Ctrl/Cmd+3 left, Ctrl/Cmd+7 bottom (the keypad's digits too); also View ▸ View front … View iso, and Space's view radial (Front, Top, Right, Iso). `camera_view {"view":"front"}` | The same keys and View menu entries | Each key shows the same side of the model in both, at the same distance and focus (RoboCAD's yaw and pitch table); the Ctrl views are the opposite sides Trace: "Reading traces — Part F › CAD-100" (by reading, unexecuted) |
| CAD-101 Focus selection | Select one part, press F (View ▸ Focus Selection); select a group: F frames it and its children; select nothing: F frames everything (as Home) | The same selections and F | The same part (or group) fills the view in both Trace: "Reading traces — Part F › CAD-101" (by reading, unexecuted) |
| CAD-102 Orthographic and field of view | 5 (View ▸ Orthographic, the radial's Ortho) toggles orthographic; View ▸ Set field of view… opens "Field of view" at the lower right: type 30, Enter (5–120, one decimal; out-of-range is refused under the field). `camera_projection`, `camera_fov {"degrees":30}` | 5; View ▸ Set field of view… (Degrees: 30) | Orthographic has no perspective in both and keeps the model's size on screen; at 30° both show the same narrower perspective Trace: "Reading traces — Part F › CAD-102" (by reading, unexecuted) |
| CAD-103 Trackball | View ▸ Toggle orbit: turntable / trackball, then right-drag across the top of the model; toggle back. `camera_orbit_mode`, `camera_state` (`mode`) | The same command (status "Orbit: trackball") and drag | The model tumbles freely (it can roll past upside down) in both; back in turntable the view returns to the nearest upright heading Trace: "Reading traces — Part F › CAD-103" (by reading, unexecuted) |
| CAD-104 View cube | The cube net at the top right of the 3D view (Top; Left, Front, Right; Iso, Bottom, Back; the face you look at is lit): click Front, then Front again; click Iso. The **Cube** chip hides and shows it | Click the cube's front face, then again; click a corner | The first click shows the front, the second the back (RoboCAD's opposite), in both. The native cube is a net of buttons, not a shaded 3D cube (recorded) Trace: "Reading traces — Part F › CAD-104" (by reading, unexecuted) |
| CAD-105 Display modes | Z cycles Shaded → Shaded + edges → Wireframe → X-ray → Matcap → Render → Shaded; View ▸ Display: shaded … Display: render and the display panel's six buttons choose one. `cad_display {"mode":"wireframe"}`, `{"next":true}` | Z; View ▸ Display: … | Same order and the same look for shaded, shaded with edges (dark B-rep edges), wireframe and xray (translucent with edges). Matcap is approximated (clay tint, no sphere image) and Render has no ground shadow (both recorded); Inspect ▸ Normal-direction shading switches both to xray Trace: "Reading traces — Part F › CAD-105" (by reading, unexecuted) |
| CAD-106 Grid | Ctrl/Cmd+G (View ▸ Grid, the **Grid** chip, the radial's Grid). `cad_display {"toggle":"grid"}` | Ctrl+G | The same 10 mm grid on the model's XY plane, ±200 mm, every 5th line darker, red X, green Y and blue Z axes, shown and hidden together Trace: "Reading traces — Part F › CAD-106" (by reading, unexecuted) |
| CAD-107 Build plate and overhangs | Ctrl/Cmd+Shift+B (Print ▸ Build Plate Preview, the **Plate** chip) | Ctrl+Shift+B | The same 220 × 220 mm plate; the same downward faces are shaded red as overhangs (45°) in both; off again in both Trace: "Reading traces — Part F › CAD-107" (by reading, unexecuted) |
| CAD-108 High contrast | View ▸ High-Contrast Theme (the **Contrast** chip) | View ▸ High-Contrast Theme | The 3D view turns light with a lighter grid and black edges in both. Only the viewer's 3D view changes (its panels keep their colours) and the setting is not kept after a restart (both recorded) Trace: "Reading traces — Part F › CAD-108" (by reading, unexecuted) |
| CAD-109 Section preview | Ctrl/Cmd+Shift+X (Inspect ▸ Section Analysis, the **Section** chip) | Ctrl+Shift+X | Both start on XZ through the middle of the model (in Y); the same side of the plane is cut away in both and the cut is outlined in red; hovering and picking never select the removed part; the same key turns it off Trace: "Reading traces — Part F › CAD-109" (by reading, unexecuted) |
| CAD-110 Section plane | With the section on: the panel's **X**, **Y** and **Z** chips (planes through the model's centre), then **Rotate**; click the toolbar's offset field ("offset, e.g. 5 or 2 cm"), type 5, Enter: the plane moves 5 mm along its normal; then `0.5 cm`, Enter; `abc` is refused under the field. `cad_section {"offset":5}`; `cad_section {"axis":"z","offset":10}` | Tab, type 5, Enter; drag the plane; R | The same cuts for the same planes and offsets in both; Rotate and R turn the plane 90° about Z the same way. In the viewer R stays the Rotate tool, Tab the numeric bar, and a left drag on the plane box-selects (or Alt-orbits) rather than moving it (recorded) Trace: "Reading traces — Part F › CAD-110" (by reading, unexecuted) |
| CAD-111 Exact section | Section on Z at offset 0 (`cad_section {"axis":"z","offset":0}`), select a body, run `system_ui` `cad:section:exact` ("Exact section of …") or `cad_section {"exact":"<id>"}` | `curl 'http://127.0.0.1:<RoboCAD port>/nodes/<id>/section?plane=xy'` | A yellow exact outline appears over the red preview outline and follows the B-rep; on any plane other than xy, xz, yz through the origin or a plane node it is refused, naming why (RoboCAD's route takes only those); after an edit to the body it is re-read Trace: "Reading traces — Part F › CAD-111" (by reading, unexecuted) |
| CAD-112 Isolate | Select one part, press `/` (View ▸ Isolate, right-click ▸ Isolate); then Ctrl/Cmd+Z | `/`; Edit ▸ Undo | Everything but the part, its children and parents is hidden in both; one undo step "Isolate" brings it back. With nothing selected the viewer refuses "Select the nodes to isolate" (RoboCAD hides everything; recorded) Trace: "Reading traces — Part F › CAD-112" (by reading, unexecuted) |
| CAD-113 Hide and Show All | Select two parts, press H (View ▸ Hide, right-click ▸ Hide); then Alt/Option+H (View ▸ Show All); undo both | H; Alt+H; Edit ▸ Undo twice | H hides the selection as one step ("Hide"); Show All shows every node ("Show all"); the tree shows the same visibility; each undo reverses one. With nothing selected the viewer refuses "Select the nodes to hide" (recorded) Trace: "Reading traces — Part F › CAD-113" (by reading, unexecuted) |
| CAD-114 Save a view | View ▸ Saved Views opens the panel at the lower right: set a view (front, ortho, section on, grid off, wireframe), type "Front cutaway" in "View name, e.g. Worm drive cutaway", **Save current view**. `cad_views {"op":"save","name":"Front cutaway"}` | View ▸ Saved Views: the same view, the same name, Save current view | Both list "Front cutaway / Orthographic · Cutaway"; "Saved inside this CAD file · edits support Undo"; a blank or 121-character name is refused before anything is sent; one undo step in RoboCAD's history ("Save view") Trace: "Reading traces — Part F › CAD-114" (by reading, unexecuted) |
| CAD-115 Rename, replace, delete | On the row: **Rename…** (type "Front A-A", Enter), turn the camera, **Replace with current**, then **Delete**; Ctrl/Cmd+Z after each. `cad_views {"op":"rename","id":"…","name":"…"}`, `"replace"`, `"delete"` | Rename…, Replace with current, Delete; Edit ▸ Undo | Each is one undo step with RoboCAD's label ("Update saved view", "Delete saved view") and each undo restores the list as it was, in both Trace: "Reading traces — Part F › CAD-115" (by reading, unexecuted) |
| CAD-116 Restore, across both | Save a view in the viewer and Save the file; open that copy in RoboCAD and Restore it there. Save a view in RoboCAD, save, open that copy in the viewer: **Restore view**. Compare `unzip -p <file>.rcad manifest.json` (`saved_views`) | Restore view (or double-click the row) | A view saved in either one restores in the other with the same direction, distance, orthographic or perspective, field of view, display mode, grid and section; the `saved_views` entries have the same keys. The viewer restores with a button, not a double-click (recorded) Trace: "Reading traces — Part F › CAD-116" (by reading, unexecuted) |
| CAD-117 Tessellation tolerance | Select a curved body; in the inspector type 0.5 in "Tessellation tolerance (mm)", Enter; then 0.01; then Ctrl/Cmd+Z | The properties panel's "Tessellation tolerance (mm)" spin box, 0.5, then 0.01 | At 0.5 both draw the same visible facets; at 0.01 both are smooth; the viewer's change is one undo step ("Set attributes"), RoboCAD's panel records none, and the viewer's field opens empty (RoboCAD reports no current value; recorded) Trace: "Reading traces — Part F › CAD-117" (by reading, unexecuted) |
| CAD-118 File ▸ New | File ▸ New (Ctrl/Cmd+N): the path form proposes `untitled.rcad` in the document's folder; type `/tmp/cad-check/new.rcad`, OK. Try again with the same path | File ▸ New | The viewer writes the empty file, then opens it (the progress strip and status line say so); an existing path is refused ("exists: choose a new file name") and left untouched. With unsaved edits in a service this window started, New is refused before any file is written (CAD-127). REST `cad_file {"op":"new","path":"/tmp/cad-check/new2.rcad"}` answers only once the new file is open (its answer names `created` and `opened`). RoboCAD opens an untitled window instead (recorded) Trace: "Reading traces — Part F › CAD-118" (by reading, unexecuted) |
| CAD-119 File ▸ Open | File ▸ Open… (Ctrl/Cmd+O): the form lists the folder's `.rcad` files; click `turntable.rcad`, OK | File ▸ Open… | The same document opens in both; `..` and folder rows move through folders; a relative path is refused, naming why Trace: "Reading traces — Part F › CAD-119" (by reading, unexecuted) |
| CAD-120 File ▸ Save As | File ▸ Save As… (Ctrl/Cmd+Shift+S): `/tmp/cad-check/copy` (no extension), OK. Then `unzip -l /tmp/cad-check/copy.rcad` | File ▸ Save As… `/tmp/cad-check/copy-rc`, then the same `unzip -l` | Both append `.rcad`; both archives hold `thumbnail.png` (a headless service draws the viewer's with RoboCAD's snapshot renderer); the left dock names the new file, and leaving and re-entering CAD mode reopens `copy.rcad`. Plain Save writes the thumbnail too (CAD-11) Trace: "Reading traces — Part F › CAD-120" (by reading, unexecuted) |
| CAD-121 Import STEP | File ▸ Import… (Ctrl/Cmd+I): `/tmp/cad-check/print-kit.step`, OK; then undo | File ▸ Import… the same file; Edit ▸ Undo | The same new nodes, volumes and face counts; one undo step removes them in both. (An SVG or an image lands on XY in the viewer, on the active plane in RoboCAD: recorded) Trace: "Reading traces — Part F › CAD-121" (by reading, unexecuted) |
| CAD-122 Import a mesh with units | File ▸ Import… and type `/tmp/cad-check/cap.stl`: as soon as the path names the mesh, the form adds "Units of the mesh file" (empty), says "Asking RoboCAD for its guess…", then "RoboCAD's guess: … (largest extent …)" and fills the unit; OK is disabled, saying why, until then. OK. Then repeat, choosing `in` before the guess lands (the guess no longer replaces it); **Guess unit** asks again | File ▸ Import…: "Units of the file" / "This format carries no unit. What are the numbers in?" with its guess preselected | The guess is the same unit in both; the imported mesh has the same size for the same unit in both (inches 25.4× millimetres); no mesh is imported in a unit nobody picked or RoboCAD guessed. The viewer's unit is a row of its path form, not a second dialog (recorded) Trace: "Reading traces — Part F › CAD-122" (by reading, unexecuted) |
| CAD-123 Export STL, 3MF, OBJ with settings | File ▸ Export… (Ctrl/Cmd+E): Format `stl`, File `/tmp/cad-check/v.stl`, Format binary, Unit mm, Chord tolerance (mm) 0.05, Angular tolerance (°) 20; OK. Then `3mf` (Write colours, Write names) and `obj` (Scale, Up axis, Quads, N-gons, Write MTL, Write UVs) | File ▸ Export… the same files (`r.stl`, …) with the same values in its dialog | Each file is written (status "Exported … to …"); the viewer's and RoboCAD's files of each format import back into RoboCAD with the same triangles and size; a value outside a field's range is refused naming the setting; reopening the form starts from the last values sent for that format during the session (RoboCAD remembers them across launches; recorded) Trace: "Reading traces — Part F › CAD-123" (by reading, unexecuted) |
| CAD-124 Export STEP, IGES, sketch SVG | Format `step` (Schema AP214, Write names, Write colours), `iges`, and `svg` with a sketch selected (Sketch (node id) is filled; draw one as in Part E if the model has none) | The same three exports | The STEP and IGES files import back into RoboCAD with the same bodies and volumes; the SVGs show the same curves; an export RoboCAD blocks names its reason Trace: "Reading traces — Part F › CAD-124" (by reading, unexecuted) |
| CAD-125 Export drawing | File ▸ Export drawing (SVG)… (Ctrl/Cmd+Shift+D) with the section on: Front, Top, Right and Isometric view checked, Title, "Section A-A (the section tool's plane)" checked; OK | File ▸ Export drawing (SVG)… with the section on | The same four views and a "Section A-A" view, titled with the document's file name by default, in both SVGs Trace: "Reading traces — Part F › CAD-125" (by reading, unexecuted) |
| CAD-126 Render | `system_ui` `cad:file:render` ("Render (PNG)…") or `cad_render {"path":"/tmp/cad-check/v.png","view":"iso","w":1200,"h":900}` | `curl -o /tmp/cad-check/r.png 'http://127.0.0.1:<RoboCAD port>/render?view=iso&w=1200&h=900'` | Both PNGs show the same view of the model; a size outside 16–8192 px or a path without `.png` is refused before anything is sent; the window never stalls while it renders Trace: "Reading traces — Part F › CAD-126" (by reading, unexecuted) |
| CAD-127 Unsaved edits | In Part A (a service this window started) make an edit, then File ▸ Open… or New: the form says in red "Not now: … has unsaved edits …: save first", and OK is refused naming the reason; New writes no file. There is no discard button. Save (Ctrl/Cmd+S), then OK opens. Attached to RoboCAD's window (Part B), with an edit unsaved there: the form notes in amber that RoboCAD keeps its edits and this window then shows the other file; OK opens it. `cad_open` and `cad_file {"op":"open"}` follow the same rule | File ▸ Open… or New with unsaved edits: another window opens and the edits stay; close the window: "Unsaved changes" / "Save before closing?" with Save, Discard, Cancel | Nothing is lost in either: the viewer refuses rather than replace a self-started service's unsaved edits, and an attached RoboCAD keeps its own; the refusals name the document and what to do Trace: "Reading traces — Part F › CAD-127" (by reading, unexecuted) |
| CAD-128 The camera in the other modes | Switch to **Robot**, **Phenomena**, **Build** and a lesson with a 3D card (`--lessons DIR`). In each: right-drag orbits, middle or Shift+right-drag pans, the wheel zooms toward the focus (in a lesson card only with Ctrl/Cmd held, so the page scrolls); the numpad's 1/3/7 (Ctrl: opposite), 9, 5, 0 and `.`; Home returns to the mode's home view; a lesson's spinning card keeps spinning; `camera_spin {"rate":0.5}` then `{"rate":0}` | (n/a: compare with a build of 1b00d789, before the shared camera) | The orbit rate, pitch limits, zoom limits, home view, glide (or cut) to home and spin of each mode feel as they did before; a drag that starts on a side dock never moves the camera; RoboCAD's gestures (CAD-129) are CAD's only: Shift+middle still pans and the arrow keys are not camera keys in these modes Trace: "Reading traces — Part F › CAD-128" (by reading, unexecuted) |
| CAD-129 RoboCAD's camera gestures | In CAD mode with the Select tool: Shift+middle-drag orbits (a plain middle-drag still pans); Alt+right-drag orbits and snaps to the nearest axis view at every step (pitch level or ±89.5°, yaw a multiple of 90°); Alt/Option+left-drag orbits once it moves more than about 6 px, while an Alt+click on overlapping parts still opens the candidates menu and an Alt drag never box-selects; the arrow keys orbit 10° (Ctrl/Cmd: 90°), Shift+arrows pan; with a text field focused (the numeric bar, the inspector, the offset field) the arrows leave the camera alone. `camera_orbit {"degrees":[10,0]}` | The same drags and keys in RoboCAD's viewport | The same turns, snaps and pans in both, at the same steps. RoboCAD's own Alt+left-drag does not orbit (its tool takes the press, ui/app.py:542; recorded in the ledger's notes), so compare the viewer's with right-drag Trace: "Reading traces — Part F › CAD-129" (by reading, unexecuted) |
| CAD-130 Curve nodes | Select a body and run Modify ▸ Silhouette onto active plane (CAD-95): a "Silhouette" curve node appears in the tree. Toggle its visibility; select it in the tree; switch display modes (Z); turn the section on across it | The same silhouette in RoboCAD's window | The curve is drawn in both, 2 px, light blue (or the node's colour), orange while selected, in every display mode, cut by the section plane, gone while hidden. Clicking the curve in the viewer's 3D view does not select it (RoboCAD's 8 px curve pick is not ported; select it in the tree; recorded) Trace: "Reading traces — Part F › CAD-130" (by reading, unexecuted) |

## Part G: materials, physical inspection, the Robot panel and simulation (cad-physical-inspect)

Set up as in Part C and D, on a model with joints and motors: the viewer on
one copy of `turntable.rcad`, RoboCAD's own window on a second copy, the
same node selected in both. CAD-138 and CAD-149 read loaded results: do
CAD-147 and CAD-150 before them. For the results steps, make a results file
from RoboCAD's export first:

```sh
cp examples/camera-turntable/cad/turntable.rcad /tmp/cad-check/turntable-rc.rcad
# after CAD-147 has written /tmp/cad-check/turntable.simrobot.json:
cargo run --release -p sim-phenomena --bin sim-cad -- run /tmp/cad-check/turntable.simrobot.json
```

Menus are named as both menu bars show them (Robot, Simulation, Inspect,
Print). In the viewer the Materials and Robot panels are sections of the
right dock (RoboCAD: the Materials and Robot docks, tabbed on the right).
`cad_state.materials`, `cad_state.robot`, `cad_state.inspector_physical` and
`cad_state.results` show the viewer's state. Steps that edit are undone in
both before the next step, unless the step says otherwise. Nothing in
this part has been compiled or run: **Part G is traced by reading,
unexecuted** ("Reading traces — Part G").

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-131 Materials list and search | The Materials section lists "■ name   density g/cm³", ■ in each material's colour; type `pla` into "Search materials…", then a tag (e.g. `metal`) | The Materials dock, the same search | The same rows, colours, densities and filtered rows in both (name or tag, any case). Trace: "Reading traces — Part G › CAD-131" (by reading, unexecuted) |
| CAD-132 Apply a material | Select two bodies, click a material row, **Apply to selection**; undo; then double-click the row; with nothing selected, **Apply to selection** | The same: Apply to selection and double-click; then with nothing selected | One undo step "Material" sets both bodies in both windows (the inspector's material and mass follow); the viewer refuses an empty selection by name, where RoboCAD silently does nothing (recorded). Dragging a material onto a body is RoboCAD's only (recorded). Trace: "Reading traces — Part G › CAD-132" (by reading, unexecuted) |
| CAD-133 New material | **New…**: Name `Check PLA`, Density (g/cm³) `1.25`, OK; then New… again with Density `abc` | **New…** with the same values | The new material appears in both lists with the same density; the density that is not a number is refused naming the field (OK is disabled). Undo removes it in both (the step is "Material" through RoboCAD's `POST /materials` route, "New material" from RoboCAD's own dialog: recorded). Trace: "Reading traces — Part G › CAD-133" (by reading, unexecuted) |
| CAD-134 Engineering properties | Select a material row, **Material properties…** (or the inspector's **Material properties…** on a body): the dialog shows each property with its origin; change Yield strength only, OK | Properties panel ▸ *Material properties…* for the same material, the same change | Both store the same value (re-open the dialog in both); the viewer sends only the changed key (`cad_state` history shows one "Material properties of …" step); a default RoboCAD's `_ENG` table supplies shows "not reported" in the viewer (recorded). Trace: "Reading traces — Part G › CAD-134" (by reading, unexecuted) |
| CAD-135 Colour | Inspector: type `0.9, 0.2, 0.2` in the colour field, Enter; then **Use material colour** | `curl -X PATCH http://127.0.0.1:<RoboCAD port>/nodes/<id> -d '{"color":[0.9,0.2,0.2]}'` (RoboCAD's window has no colour editor; the route is the reference), then `-d '{"color":null}'` | The body turns the same red in both windows and back to its material's colour; each is one undo step. Trace: "Reading traces — Part G › CAD-135" (by reading, unexecuted) |
| CAD-136 Joint editor | Select a joint; inspector **Edit joint…** (or double-click its row in the Robot panel, CAD-141): change the upper limit and the name, OK | Double-click the joint in the Robot panel ("Edit joint" dialog), the same change | The same limit and name in both (`GET /robot`); the rename is a second call only when the name changed, as RoboCAD's handler. Trace: "Reading traces — Part G › CAD-136" (by reading, unexecuted) |
| CAD-137 Joint physics overrides | Select a joint: the rows "Radial clearance (mm)", "Wobble (°)", "Drive backlash (°; …)", "Coulomb friction (mN·m)", "Viscous (mN·m·s)", "Radial stiffness (N/m)", "Flex patch radius (mm)" and the source line; type a Coulomb friction, Enter | Properties panel on the same joint, the same value | The same values and provenance in both, "*" on the overridden row in both after the edit; a value the physical model lacks is empty in the viewer, 0.0 in RoboCAD (recorded). Trace: "Reading traces — Part G › CAD-137" (by reading, unexecuted) |
| CAD-138 Results line | After CAD-150 (results loaded), select a link body | The same body in RoboCAD's Properties panel | The same "Results: key value, …" line (same keys, 3 significant figures) in both. Trace: "Reading traces — Part G › CAD-138" (by reading, unexecuted) |
| CAD-139 Exact measurement | Select two bodies, **Calculate exact measurements**; then start it again and change the selection before it finishes; start it again and make an edit | Properties panel ▸ *Calculate exact measurements*, the same selection | The same size, volume, area, mass and centroid in both; the viewer's measurement is cancelled by the selection change and by the edit, each saying why, and the window never stalls; its preview has no "Display size ≈" line (recorded). Trace: "Reading traces — Part G › CAD-139" (by reading, unexecuted) |
| CAD-140 Robot panel summary and tree | The Robot section: the summary line, the "Links", "Joints", "Motors", "Sensors & cables" headings with their rows, each with its detail line | The Robot dock | The same counts ("n bodies, n joints, n DoF, n motors, n sensors, n cables. Ground: …. Power: ….") and rows in both; the viewer omits "(n s run)" and adds "(stale: …)" when RoboCAD flags the results stale; no branch glyphs, and Detail and Margin are lines under the name (recorded). Trace: "Reading traces — Part G › CAD-140" (by reading, unexecuted) |
| CAD-141 Robot panel click and double-click | Click a joint row, then a link row; double-click a joint row | The same clicks in RoboCAD's Robot dock | A click selects the node in both (the viewer's tree and 3D view follow it); the double-click opens "Edit joint" preset from that joint in both. Trace: "Reading traces — Part G › CAD-141" (by reading, unexecuted) |
| CAD-142 Validate and issues | Robot ▸ Robot: validate; read the Robot panel's issue list | Robot ▸ Robot: validate | The same verdict: "robot valid: …" in the status line, or the same issues; RoboCAD's warning box is the viewer's status line and issue list, "Error:"/"Warning:" in place of ⛔/⚠ (recorded). Trace: "Reading traces — Part G › CAD-142" (by reading, unexecuted) |
| CAD-143 Motor library | Robot ▸ Robot: motor library… | Robot ▸ Robot: motor library… | The same motors and specs; the viewer shows a floating panel, RoboCAD a message box; the viewer lists them by id (recorded). Trace: "Reading traces — Part G › CAD-143" (by reading, unexecuted) |
| CAD-144 Add motor from library | Robot ▸ Robot: add motor from library… (or the Robot panel's **Add motor…**): Motor, Rotation about shaft, Mount on, "Cut mounting holes and pilot into the mounted body", Name; click a body's face; Escape ends the tool and the dialog | Robot ▸ Robot: add motor from library…, the same values, click the same face | The same motor node at the same place (housing outside, shaft into the body) in both; the new motor becomes the selection in both; one undo step. The viewer's fields stay beside the view while you click (recorded). Trace: "Reading traces — Part G › CAD-144" (by reading, unexecuted) |
| CAD-145 Add joint tool and joint from selection | Robot ▸ Robot: add joint (Ctrl+Shift+J): click the parent (Ctrl-click for the world), the child, an axis face; then select two bodies and Robot ▸ Robot: joint from the two selected bodies… (Type, Parent "(world)", Child, Pivot (mm), Axis, limits, Motor, Extra gear ratio, Damping, Name), OK | Robot ▸ Robot: add joint (the menu entry; Ctrl+Shift+J is not bound in RoboCAD), the same clicks; then the same dialog | The same joints (type, pivot, axis, limits) in both; the new joint becomes the selection; Ctrl+Shift+M still runs Select Same Material in both (recorded). Trace: "Reading traces — Part G › CAD-145" (by reading, unexecuted) |
| CAD-146 The other robot tools and dialogs | In turn, undoing each: Robot ▸ infer joints from coaxial holes and pins; assign selected motor to a joint… (Motor, Joint, Extra gear ratio); fix selected bodies together; toggle ground on selected bodies; add sensor… (Kind, On body, Point (mm), Reads joint, Rate (Hz), Name); add cable between bodies… (From/To body and point, Length, Mass, Name); battery, control loop and uncertainty… (Battery cells, Chemistry, Capacity (Ah), Control period (s), Control latency (s), Target per joint (°) as `{"joint": deg}`, Dimension σ (mm), Friction σ) | The same Robot menu entries and dialogs with the same values | The same result in `GET /robot`, `/sensors`, `/cables`, `/battery`, `/control`, `/uncertainty` after each; Assign motor without motors or joints is refused before its dialog in both; a joint left out of the targets keeps its target in the viewer (recorded). Trace: "Reading traces — Part G › CAD-146" (by reading, unexecuted) |
| CAD-147 Export physical model | Simulation ▸ Simulation: export physical model (simrobot v4, with flexible links)…: `/tmp/cad-check/turntable.simrobot.json`, OK; start it again and **Cancel export** while it runs | Simulation ▸ Simulation: export physical model… to `/tmp/cad-check/turntable-rc.simrobot.json` | Both files hold the same model (the same links, masses, joints and motors; simrobot version 4); the status line shows "exporting … n s"; cancellation before publication writes nothing, while a late cancellation retains and reports an already written file; queued exports wait for the terminal outcome; the window never stalls. Trace: "Reading traces — Part G › CAD-147" (by reading, unexecuted) |
| CAD-148 Export robot model (planar) | Simulation ▸ Simulation: export robot model…: `/tmp/cad-check/turntable-planar.simrobot.json` | Simulation ▸ Simulation: export robot model… | Both files carry the same x–z planar hint. Trace: "Reading traces — Part G › CAD-148" (by reading, unexecuted) |
| CAD-149 Stress overlay | Inspect ▸ Toggle stress overlay (or Print ▸ Strength overlay on/off, or the Robot panel's **Stress overlay**) after CAD-150 | Inspect ▸ Toggle stress overlay | The same links are coloured, hot where RoboCAD's are; the viewer uses Robot mode's log scale (blue at 0.1 % of yield → red at yield), RoboCAD a linear one (recorded), so mid-range colours differ; the staleness label matches RoboCAD's stale flag. Trace: "Reading traces — Part G › CAD-149" (by reading, unexecuted) |
| CAD-150 Load results | Robot ▸ Robot: load simulation results…: the form starts at `<stem>.simresult.json` (`/tmp/cad-check/turntable.simresult.json`, written by `sim-cad run` above); OK | Robot ▸ Robot: load simulation results…, the same file | The Robot panel's margins (yield, bearing, screw, stall, Tg, mount Tg) match in both, and the stress overlay turns on in both. Trace: "Reading traces — Part G › CAD-150" (by reading, unexecuted) |
| CAD-151 Apply identification | Robot ▸ Robot: apply identified joint parameters…: a `sim-cad fit` output file if you have one, else a missing path | The same entry and file | The same stored joint parameters (the next export's joint physics) in both, or the same RoboCAD error verbatim. Trace: "Reading traces — Part G › CAD-151" (by reading, unexecuted) |
| CAD-152 Live link into Robot mode | Simulation ▸ Simulation: live link (watch + run viewer) on the saved document: the window switches to Robot mode on `turntable.simrobot.json`; go back to CAD, change a joint limit, Save (Ctrl/Cmd+S), then switch to Robot mode | Simulation ▸ live link in RoboCAD's window: it exports and starts `sim-spatial --robot` | The viewer opens Robot mode in the same window (no new process) on the exported model, and after the save Robot mode shows the new limit; with an export running, leaving CAD is refused naming it; an unsaved document refuses the link ("Save the document first: …"). Trace: "Reading traces — Part G › CAD-152" (by reading, unexecuted) |

## Part H: printing (cad-print)

Set up as in Part G: the viewer on one copy of `turntable.rcad`, RoboCAD's
own window on a second copy (`turntable-rc.rcad`), the same node selected
in both. Print studies run as RoboCAD jobs, which only its REST service
runs, so RoboCAD's window must serve its API (it does from 8420 up; Part
B). CAD-159 to CAD-161 and CAD-165 need a print study on the document: in
both, set one on a body `<id>` (one of the model's printed parts):

- the viewer: `cad_op {"name":"set_robot_setting","args":["print_study",{"printer":"bambu-p1s","material":"petg-hf","parts":[{"node":"<id>","fixtures":[{"region":{"bottom":true}}],"loads":[{"region":{"faces":[3]},"direction":[0,0,-1],"magnitude":20.0}]}]}]}`
- RoboCAD: `curl -X POST http://127.0.0.1:<RoboCAD port>/ops/set_robot_setting -d '{"args":["print_study",{…the same study…}]}'`

The Print menu is named as both menu bars show it. In the viewer, "Print
jobs" is a section of the right dock (RoboCAD: a message box).
`cad_state.print` shows the viewer's state (checks, remembered values,
registry and study reads, jobs, the overlay). Steps that edit are undone
in both before the next step, unless the step says otherwise. Nothing in
this part has been compiled or run: **Part H is traced by reading,
unexecuted** ("Reading traces — Part H").

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-153 Wall thickness check | Print ▸ Wall thickness check… (Ctrl+W) with nothing selected: "Flag walls thinner than (mm):" 1.2, OK; then select one body and run it again at `2`; then open the form once more; finally `cad_print {"op":"clear"}` | Print ▸ Wall thickness check… (Ctrl+W), the same thresholds and selection | The same red points (RoboCAD's 1.0, 0.2, 0.2, 9 px) on the same places and the same status ("N thin region(s) under 1.2 mm" or "No walls thinner than 1.2 mm"); the window never stalls while it reads; the third form opens at `2` in the viewer (it remembers the last threshold; RoboCAD reopens at 1.2: recorded); running the same check again on unchanged nodes sends nothing (`cad_state.print.checks.cached_reads`); after any edit the points are no longer drawn; clear removes them. Trace: "Reading traces — Part H › CAD-153" (by reading, unexecuted) |
| CAD-154 Validate for printing | Print ▸ Validate for printing (Ctrl+Shift+V); then hide a body and run it again | Print ▸ Validate for printing | The same verdict: "n body(ies): valid and watertight." (the same n: every visible body), or the same messages ("name: message near (x, y, z) — fix"). The viewer's answer is the status line, not a box, and it has no open-edge lines: RoboCAD's tessellation open-edge check is its desktop's only (`cad_state.print.checks.open_edge_check` says so; recorded). Trace: "Reading traces — Part H › CAD-154" (by reading, unexecuted) |
| CAD-155 Overhang shading | Print ▸ Toggle overhang shading (or the toolbar's **Overhangs** chip); toggle it off; then View ▸ Build Plate Preview (Ctrl+Shift+B) on and off; then the plate on and overhang shading off | The same entries | The same faces facing down past 45° are tinted 0.9, 0.35, 0.3 in both; the build plate turns overhang shading on with it and off with it in both, and the shading's own toggle flips only the shading; nothing is written to the document (no undo step). Trace: "Reading traces — Part H › CAD-155" (by reading, unexecuted) |
| CAD-156 Fastener hole | Print ▸ Fastener hole… (Ctrl+H): Size `M4`, Kind `counterbore`, Extra clearance (mm) `0.1`, Depth (mm; 0: through) `0`; click a flat face, then another; Escape; open the tool again | Print ▸ Fastener hole… (Ctrl+H): the same values in the dialog, OK, then click the same faces; Escape; open it again | Each click is one hole and one undo step at the clicked point in both (the viewer's status line names it "M4 counterbore hole in NAME"), through the part (depth 0 is "through"); the viewer's fields stay beside the view while you click (recorded); both reopen with M4, counterbore, 0.1, through (remembered); a click off any face does nothing; the selection is not changed by the clicks. Trace: "Reading traces — Part H › CAD-156" (by reading, unexecuted) |
| CAD-157 Clearance offset | Select two faces of one hole and one face of another body, Print ▸ Clearance offset… (Ctrl+Shift+C): "Grow holes / shrink bosses by (mm):" `0.3`, OK; open it again | The same faces, Print ▸ Clearance offset…, `0.3` | The same faces move by 0.3 mm in both; one RoboCAD undo step "Clearance" per body (two here) in both; the form reopens at 0.3 in both (0.2 the first time); with no face selected both refuse "Select holes, bosses or faces to offset". Trace: "Reading traces — Part H › CAD-157" (by reading, unexecuted) |
| CAD-158 Split for printing (dovetail) | Select one body, Print ▸ Split selected for printing…: Printer the first entry ("id (x × y × z mm)") and another printer, Joints `dovetail`, OK; watch the status line until it ends | Print ▸ Split selected for printing…, the same printer and joint | The same printer list in the same order with the same usable sizes; while it runs "split: … (n %)"; the same pieces under a new split group and the same done text ("split into N pieces; hardware: …"); with two bodies selected both refuse "Select one body to split.". Trace: "Reading traces — Part H › CAD-158" (by reading, unexecuted) |
| CAD-159 Check strength | Print ▸ Check strength (document's print study); first on a copy with no study, then with the study set above | The same entry, without and with the study | Without a study both explain "This document has no print study yet. …" word for word; with it, both run a job and end "strength: least safety factor F on NAME (MODE); Print ▸ Strength overlay shows where" with the same F and part. Trace: "Reading traces — Part H › CAD-159" (by reading, unexecuted) |
| CAD-160 Plan with job progress | Print ▸ Plan print settings and plates (document's print study); watch the status line | The same entry | While it runs the status line reads "plan: message (n %)" in both (RoboCAD adds " — Print ▸ Print jobs… to cancel"; recorded), changing as it progresses; both end "plan: N plate(s), about H h and G g (estimates); 3MF files in DIR" with the same plates; the window never stalls. Trace: "Reading traces — Part H › CAD-160" (by reading, unexecuted) |
| CAD-161 Whole or split | Select the study's part, Print ▸ Whole or split for strength?; then select a body that is not in the study | The same | The same "RECOMMENDATION: WHY" in both; a body outside the study is refused by both ("Select a part that is in the document's print study …"). Trace: "Reading traces — Part H › CAD-161" (by reading, unexecuted) |
| CAD-162 Assembly guide | Select the split group from CAD-158 (then one of its pieces), Print ▸ Assembly guide for the selected split… | The same | Both run a job, end "assembly: N steps; guide PATH" and open the same guide with the system's opener; selecting a piece finds its split group in both; with nothing split selected both refuse by name. Trace: "Reading traces — Part H › CAD-162" (by reading, unexecuted) |
| CAD-163 Test coupons | With the split group selected, Print ▸ Test coupons…: Printer, Filament from the lists; then with nothing selected | The same, Printer and Filament | The same printer and filament ids in the same order; both end "coupons: N on P plate(s); break them, fill results.json, then `sim-print promote results.json`" and open the protocol's folder; with nothing selected the coupons are for the material only in both. Trace: "Reading traces — Part H › CAD-163" (by reading, unexecuted) |
| CAD-164 Print jobs and cancel | Start a plan (CAD-160), then Print ▸ Print jobs…: the section lists the last eight jobs ("kind id: state n % message"); **Cancel running jobs…** → "Cancel the running jobs?" → **No**; again → **Yes**; **Close** | Start a plan, Print ▸ Print jobs…: the list and "Cancel the running jobs?" → No; again → Yes | The same jobs and lines in both; No leaves the job running in both; Yes cancels it in both (status "plan cancelled"); the viewer asks in an inline row of the section, not a box (recorded), and sends exactly one cancel per running job (`cad_state.print.jobs`); with no job running there is no Cancel button and the list says "No print jobs yet." on a fresh service. Trace: "Reading traces — Part H › CAD-164" (by reading, unexecuted) |
| CAD-165 Print overlay and staleness | After CAD-159, Print ▸ Strength overlay on/off; read the results line in the Results section; then make any edit | Print ▸ Strength overlay on/off | The same parts are coloured, and the part with the least safety factor is the reddest in both; the viewer colours each part in one colour by its governing failure index (1 / safety factor) on Robot mode's stress scale, where RoboCAD colours each voxel's failure index (recorded), so colours inside a part differ; the viewer's line "Print strength: N part(s); least safety factor F on NAME (current)" turns "stale (computed at revision R, now M)" after the edit. Trace: "Reading traces — Part H › CAD-165" (by reading, unexecuted) |
| CAD-166 Leaving CAD mode while a job runs | Start a plan, then switch to Robot mode before it ends; then wait for it to end and switch again | (n/a: RoboCAD has no other modes) | The switch is refused while the job runs, naming it ("a print job is running in RoboCAD: plan (n %); wait for it, or cancel it in the Print jobs section"); once it ends the switch goes through after a save (the plan adds a RoboCAD undo step, so a self-started service's unsaved-edits rule refuses until you save); with the service lost (not connected) the print job no longer holds the switch (the unsaved-edits rule still does for a self-started service). Trace: "Reading traces — Part H › CAD-166" (by reading, unexecuted) |
| CAD-167 Robot panel row press | Click a joint row in the Robot section, then the same row through `system_ui` `cad:robot:row:<id>` | Click the same row in RoboCAD's Robot dock | The node is selected in both, as in CAD-141; the viewer's press is stamped with the revision the robot description was read at, on the action only (a body item drops it, `cad/selection/mod.rs:223`; `cad_state` doesn't show it), and a row of an older read is not refused; a row of a node that has left the tree is refused by name. Trace: "Reading traces — Part H › CAD-167" (by reading, unexecuted) |
| CAD-168 Partial REST Edit joint | With joint `<j>` selected, `cad_run {"id":"ops.set_joint","params":{"lower":-30}}` | `curl -X POST http://127.0.0.1:<RoboCAD port>/ops/set_joint -d '{"args":["<j>"],"kwargs":{…every current value, lower -0.5236…}}'` (RoboCAD's `set_joint` updates only the fields passed, `commands.py:955-956`; its Edit joint dialog sends them all, and so does the viewer, filled from the joint) | Only the lower limit changes (−30°), the joint's type, parent, child, pivot, axis, upper limit, motor, gear ratio, damping and name stay as they were in both (`GET /robot`); a type change between prismatic and revolute without both limits is refused by name; with the robot description not yet read at the shown revision it is refused by name and nothing is sent. Trace: "Reading traces — Part H › CAD-168" (by reading, unexecuted) |

## Part I: outliner organization, comments, references and the system link (cad-organize)

Set up as in Part G: the viewer on one copy of `turntable.rcad`, RoboCAD's
own window on a second copy (`turntable-rc.rcad`). Use a model with at
least two groups and a few bodies in each. For the references steps, have
a PNG or JPEG image on disk (`<img>`, e.g. a photo of the part with a
ruler) and, for the system steps, a `.system.json` (`<system>`, e.g. one
from `library/systems/`). In the viewer the outliner's search, New group,
Expand all and Collapse all sit above the model tree, and Comments and
References are sections at the top of the right dock while shown
(RoboCAD: the Outliner, Comments and References docks). `cad_state.tree`,
`cad_state.threads` and `cad_state.references` show the viewer's state
(REST: `cad_tree`, `cad_threads`, `cad_references`). Steps that edit are undone in both before the next
step, unless the step says otherwise. Nothing in this part has been
compiled or run: **Part I is traced by reading, unexecuted** ("Reading
traces — annotations" for CAD-176 to CAD-186, "Reading traces — Part I
(outliner, references, system link)" for the rest).

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-169 Search | Type part of a body's name into "Search (Ctrl+F)…" above the tree; clear it; then with the pointer over the tree press Ctrl+F, and with the pointer over the 3D view and a body selected press Ctrl+F | Type the same text into the Outliner's search; Ctrl+F | The same rows in both: every node whose name contains the text (any case), its ancestors and its descendants, all expanded while searching; clearing restores the collapse state from before. Ctrl+F over the tree focuses the search in the viewer; elsewhere it starts Fillet in both (RoboCAD's keymap gives Ctrl+F to Fillet, so its placeholder's key never reaches the field: recorded). Trace: "Reading traces — Part I (outliner, references, system link) › CAD-169" (by reading, unexecuted) |
| CAD-170 Expand and collapse | **Collapse all**, then expand one group by its "+" chip; make an edit (rename a body); **Expand all**; then type a search and press **Collapse all** | The same buttons and arrow | The same rows shown in both after each; the collapsed groups stay collapsed after the edit's refresh in both; while a search is typed the viewer refuses Expand all, Collapse all and the chips by name (RoboCAD changes the searched view without keeping it: recorded). Trace: "Reading traces — Part I (outliner, references, system link) › CAD-170" (by reading, unexecuted) |
| CAD-171 Shift and Ctrl select | Click a body row, Shift+click a row three below, then Ctrl/Cmd+click one of them | The same clicks in the Outliner | The same nodes selected in both (the range, then without the Ctrl-clicked one); the 3D view lights the same bodies; the viewer's selection is pushed to RoboCAD (`GET /selection`) as body items. Trace: "Reading traces — Part I (outliner, references, system link) › CAD-171" (by reading, unexecuted) |
| CAD-172 Rename in place | Double-click a body's name, type `Check name`, Enter; then double-click again and press Escape; again, type a name and click elsewhere; again, clear the name and press Enter | Double-click the same name, type, Enter; the same click elsewhere | One undo step renames it in both (the inspector and the tree follow); Escape leaves the name; an unchanged name sends nothing; a click elsewhere renames in RoboCAD (Qt commits on focus loss) and ends without renaming in the viewer, and an empty name is refused by name in the viewer (both recorded). Trace: "Reading traces — Part I (outliner, references, system link) › CAD-172" (by reading, unexecuted) |
| CAD-173 Drag into a group, before a sibling | Drag two selected bodies onto the middle of a group row; undo; drag one body onto a body row in another group; then drag a group onto the top quarter of another group's row; finally drag a group onto one of its own children | The same drags in the Outliner | The same tree in both after the first two: on a group the rows move into it (at its end); on a body they move before it under its parent; each drag is one `move_nodes` undo step. The top quarter of a group row moves the group in front of it in the viewer, where RoboCAD drops into the group (recorded). A group onto its own child is refused in both ("Cannot move a group into itself or its descendants"; the viewer checks before sending). Trace: "Reading traces — Part I (outliner, references, system link) › CAD-173" (by reading, unexecuted) |
| CAD-174 Context menu | Right-click an unselected body row (it becomes the selection), then a selected one: read the entries; **Hide**, **Show**, **Isolate**, **Show all**; **Lock**, then **Unlock**; **Group selection…** (`Organize components` / `Group name:` `Pair`, OK); under **Move to group**, **Top level**, then a group by its path ("A / B"); right-click a group: **Set as active group**; then **Clear active group**; registry `group.set_active` with no group selected | The same entries in RoboCAD's Outliner menu; the registry command "Set selected group as active" with no group selected | The same entries in the same order in both (Fit in view, Isolate, Hide, Show, Lock, Unlock, Group selection…, Move to group, Make unique (bake instance), Set as active group for one group, Delete, Clear active group, Show all); the same visibility, lock state, groups, moves and active group (its row in blue in both) after each, Hide and Show one undo step each; Move to group lists the same group paths in the same order, without the selection and its descendants, as a heading over indented entries in the viewer, a submenu in RoboCAD (recorded); with no group selected `group.set_active` is refused by name in the viewer, where RoboCAD silently clears the active group (recorded). Trace: "Reading traces — Part I (outliner, references, system link) › CAD-174" (by reading, unexecuted) |
| CAD-175 New group | **New group** above the tree: `Group name:` `Empty`, OK; then New group with an empty name | Outliner ▸ **New group**, the same names | An empty group "Empty" appears in the same place in both, one undo step; an empty name creates nothing in both, and the viewer says why in the dialog (RoboCAD's dialog just closes: recorded). Trace: "Reading traces — Part I (outliner, references, system link) › CAD-175" (by reading, unexecuted) |
| CAD-176 Comments section and Annotate | View ▸ Comments panel; **＋ Annotate model** (or N with the pointer over the 3D view): click a face of a body; type `Check this face`, **Post annotation**; then N and click empty space | View ▸ Comments panel, Annotate (N), click the same face, the same text | The same thread "1 · part" in both lists with the same preview and "Attached to surface"; the pin "1" at the clicked point in both; clicking empty space says "Click a visible surface to place the annotation" in both; with the composer focused, N is typed, not a new annotation. The viewer picks on the press and leaves the selection mode as it is (RoboCAD picks on the release and shows face mode while the tool is on: recorded). A pin placed at a revision the shown document has since moved past is refused by name with the text kept, and Annotate again places it on the current model (trace "Remote refresh keeps the draft"). Trace: "Reading traces — annotations › CAD-176" (by reading, unexecuted) |
| CAD-177 Reply with a part link | Select the thread; select another body in the tree; **Insert part link from selection**; type ` needs a fillet`; Shift+Enter, a second line; **Reply** (then, for a second reply, Enter) | The same in RoboCAD's Comments dock (Enter types a newline there; Reply posts) | The same reply in both, the link shown as the part's label; with nothing selected both say "Select a part in the outliner or viewport first"; in the viewer Enter posts and Shift+Enter is a newline (recorded). A draft whose thread is gone is kept and refused by name (trace "Remote refresh keeps the draft"). Trace: "Reading traces — annotations › CAD-177" (by reading, unexecuted) |
| CAD-178 Click the part link | Click the link in the reply (use a link to a group) | Click the same link | Both show only that part and its descendants, framed, with "Part view: name" in the status line; the viewer selects exactly the linked node (as `cad_select`), RoboCAD the node with its descendants (recorded). Trace: "Reading traces — annotations › CAD-178" (by reading, unexecuted) |
| CAD-179 Show on model | **Return to assembly**, then **Show on model** | **Show on model** | The anchor body is selected and the camera returns to the view saved with the thread in both; the viewer restores it from the thread list, not through RoboCAD's GUI-only `/threads/{id}/show` (recorded); the selection is pushed to RoboCAD (`GET /selection` shows the body), its camera is not moved. On a thread of experiment evidence both open the captured run: the viewer in its captured-run review, RoboCAD in its experiments panel; without a run_id or a connection the viewer refuses by name. Trace: "Reading traces — annotations › CAD-179" (by reading, unexecuted) |
| CAD-180 Fit in view | **Fit in view** | **Fit in view** | Both frame the thread's linked parts at the current angle, select them and show the pins, "Fit annotation in view: names" in both. Trace: "Reading traces — annotations › CAD-180" (by reading, unexecuted) |
| CAD-181 Show only linked parts and Return | **Link selected parts** with two bodies selected; **Show only linked parts**; **Return to assembly**; again Show only linked parts, then Escape | The same | Only the linked parts are drawn in both, the tool hint "Showing linked parts only · Esc or Return to assembly restores your view"; Return and Escape restore the camera and the selection from before in both; no node's visibility changes in either (`GET /doc`), and RoboCAD's window is not isolated by the viewer's (recorded). Trace: "Reading traces — annotations › CAD-181" (by reading, unexecuted) |
| CAD-182 Resolve and Reopen | **Resolve**; filter **Resolved**, then **Open**, then **All**; **Reopen** | The same | The thread moves between the filters the same way in both, "✓" when resolved; its pin is hidden while resolved in both. Trace: "Reading traces — annotations › CAD-182" (by reading, unexecuted) |
| CAD-183 Edit and delete a message | Select the reply, **Edit message**, change the text, **Save edit**; then **Delete message** | The same | The same text, then the message gone, in both; each one RoboCAD undo step. Trace: "Reading traces — annotations › CAD-183" (by reading, unexecuted) |
| CAD-184 Delete thread | **Delete thread** | **Delete thread** | The thread and its pin are gone in both; undo brings both back. Trace: "Reading traces — annotations › CAD-184" (by reading, unexecuted) |
| CAD-185 Pins | View ▸ Toggle comment pins off and on; click pin "1" | The same | The pins hide and show in both; a click on a pin opens its thread in the Comments section in both (amber for "needs review", blue otherwise). Trace: "Reading traces — annotations › CAD-185" (by reading, unexecuted) |
| CAD-186 Reattach | Delete the anchored body (then undo after the step), so the thread says "Part deleted — reattach this annotation"; **Reattach…**, click another face | The same | The same attachment texts in both; after the click the thread is attached to the new face in both (one `PATCH`, one undo step) and its pin moves; the viewer picks on the press (recorded). Trace: "Reading traces — annotations › CAD-186" (by reading, unexecuted) |
| CAD-187 Add reference images | View ▸ References; **＋ Add reference images…**: the absolute path of `<img>` in the path field, Enter; then drop `<img>` on the 3D view; then drop a `.step` file, and type a relative path in the path field | References ▸ **＋ Add reference images…**, the same file in the file dialog; then drop it on the viewport; then drop the `.step` file | One locked image node per file in both, on the active plane (else XY), 100 mm wide, the view aligned on it; the image textured on its plane at 60 % opacity in both; "n reference image(s) added • Calibrate scale before tracing"; the viewer takes one typed image per submit, not the system's file dialog (recorded); the `.step` drop and the relative path are refused by name in the viewer with nothing sent, where RoboCAD's import fails in Pillow (recorded). Trace: "Reading traces — Part I (outliner, references, system link) › CAD-187" (by reading, unexecuted) |
| CAD-188 Visibility | Uncheck the image in the References list (its chip), then check it; read the list | The same | The image hides and shows in both, one undo step each; RoboCAD shows a preview thumbnail under the list, the viewer none (the image is drawn on its plane; recorded); a WebP or BMP image is listed in the viewer with a note that it is not drawn (recorded). Trace: "Reading traces — Part I (outliner, references, system link) › CAD-188" (by reading, unexecuted) |
| CAD-189 Placement | Plane `Front (XZ)`, Width `200`, Origin `10, 0, 5`, Rotation `15`, Opacity `40`, **Apply placement** | The same fields, **Apply placement** | The same plane, size, rotation and opacity in both; "Reference placement updated • Ctrl+Z undoes"; one undo step. Trace: "Reading traces — Part I (outliner, references, system link) › CAD-189" (by reading, unexecuted) |
| CAD-190 Align view | **Align view**; then make a construction plane from a tilted face, make it the active plane, add `<img>` (it lands on that plane), delete the plane node, and Align view again, then **Sketch over this** | **Align view**, the same, **Sketch over this** | Both look straight at the image, orthographic, centred, the image's plane the active plane; on the tilted plane (no longer a plane node, nor XY, XZ or YZ) the viewer aligns the camera but leaves the active plane and says so, and refuses Sketch over this by name, where RoboCAD sketches on the image's plane (recorded). Trace: "Reading traces — Part I (outliner, references, system link) › CAD-190" (by reading, unexecuted) |
| CAD-191 Calibrate scale | **Calibrate scale**: click two points on the ruler, type the real distance (e.g. `100`) in the References section's "Real distance" field, Enter; again, clicking the same point twice | The same clicks and distance (RoboCAD's numeric bar) | The same new width in both ("Reference calibrated • Ctrl+Z undoes"); Escape before Enter changes nothing; the same point twice is refused with RoboCAD's text (the viewer on the second click, RoboCAD at Enter: recorded); the viewer's distance field is in the References section, not the numeric bar (recorded). Trace: "Reading traces — Part I (outliner, references, system link) › CAD-191" (by reading, unexecuted) |
| CAD-192 Sketch over this | **Sketch over this**, draw a line over the image | The same | The view is aligned and the line tool starts on the image's plane in both. Trace: "Reading traces — Part I (outliner, references, system link) › CAD-192" (by reading, unexecuted) |
| CAD-193 Remove reference | **Remove reference** | **Remove reference** | The image node is gone in both; undo restores it. Trace: "Reading traces — Part I (outliner, references, system link) › CAD-193" (by reading, unexecuted) |
| CAD-194 System status line | Read the line at the top of the References section with no system linked | The same line in RoboCAD's References dock | "System file: none linked. Link a .system.json to build circuits and subsystems for this model." in both. Trace: "Reading traces — Part I (outliner, references, system link) › CAD-194" (by reading, unexecuted) |
| CAD-195 Link, Accept, Unlink | **Link system file…**: `<system>` in the path form, OK; edit `<system>` on disk (e.g. change its title); **Accept changes**; **Unlink** | **Link system file…** (file dialog), the same edit, **Accept changes**, **Unlink** | The same status line in both after each: "System: title · revision n · n definitions", "· CHANGED since linked (was revision r)" after the edit, back without it after Accept, "none linked" after Unlink; each one RoboCAD undo step. With RoboCAD's desktop window, CHANGED after the on-disk edit shows in the viewer after **Refresh** (`cad_refresh`) or reopening the References section, as RoboCAD's own dock refreshes only on its document events; a headless service's status is re-read every 2 s while the section is open. Trace: "Reading traces — Part I (outliner, references, system link) › CAD-195" (by reading, unexecuted) |
| CAD-196 Open in builder | Link `<system>` again; **Open in builder**; then unlink and press it again | **Open in builder** | The viewer switches this window to Build mode on `<system>` (no new process; RoboCAD starts a second `sim-spatial --system … --schematic`: recorded); leaving CAD follows the usual rule (refused over unsaved edits of a self-started service); with no system linked both refuse "Link an existing system file first". Trace: "Reading traces — Part I (outliner, references, system link) › CAD-196" (by reading, unexecuted) |

## Part J: reusable components and composition (cad-components, T42)

Written acceptance sequence, **not executed or signed off**; traced by
reading, unexecuted ("Reading traces — Part J", CAD-197 to CAD-213). Use the native
CAD window on an editable copy of a real `.rcad`; Python/OCCT remains the kernel.
These checks do not substitute for the parity harness.

| Step | Native workflow | Reference/observable acceptance |
|---|---|---|
| CAD-197 Library and find | Components; find a definition | Names, definition revision and occurrence count match GET /components and RoboCAD. Trace: "Reading traces — Part J › CAD-197" (by reading, unexecuted) |
| CAD-198 Capture/parametric | Select bodies; Make from selection; separately New parametric box/cylinder | Matching identity relationships and recipe defaults; preserved IDs across native/service serialization (independent captures may generate different fresh IDs); one ComponentChange undo; headless service needs no Qt. Trace: "Reading traces — Part J › CAD-198" (by reading, unexecuted) |
| CAD-199 Place twice | Place the captured definition twice with name, origin, Z angle, variant and typed bindings | Distinct occurrence IDs; definition identity shared; binding kinds match source ports. Trace: "Reading traces — Part J › CAD-199" (by reading, unexecuted) |
| CAD-200 Defaults/nesting | Edit defaults including units, bounds, provenance, recipes, nested maps and family variants | Rebuild updates inherited occurrences; local branch overrides win; rejected draft remains resumable. Trace: "Reading traces — Part J › CAD-200" (by reading, unexecuted) |
| CAD-201 Overrides/reset | Select a linked part/nested occurrence; edit override and origin; disable/reset override | Correct branch node_map target; nested movement refused; reset inherits; native expression text labelled unevaluated. Trace: "Reading traces — Part J › CAD-201" (by reading, unexecuted) |
| CAD-202 Detach/transform | Detach outer occurrence; rigidly transform top-level occurrence | Undo restores links; member transform restrictions match source. Trace: "Reading traces — Part J › CAD-202" (by reading, unexecuted) |
| CAD-203 Progress/cancel | Start rebuild then Cancel, including while POST is pending | Durable cancellation; queued ready cannot commit after accepted cancel; already-applied commit is reported truthfully. Trace: "Reading traces — Part J › CAD-203" (by reading, unexecuted) |
| CAD-204 Stale/network | Mutate source externally during preparation; interrupt start response | Revision refusal preserves edit; status-list recovery adopts only captured operation/document/revision; POST never resent. Trace: "Reading traces — Part J › CAD-204" (by reading, unexecuted) |
| CAD-205 Graph forms | System composition; choose type, body, parameters and geometry rule | Authoritative metadata/units/input bounds; derived output cannot also be explicit; layout never edits physical geometry. Trace: "Reading traces — Part J › CAD-205" (by reading, unexecuted) |
| CAD-206 Imported bindings | Read an existing completed check ID; bind existing imported component | Imported port names and type preserved; graph-only edits may stale results while structural metadata remains current; physical edits refuse stale metadata. Trace: "Reading traces — Part J › CAD-206" (by reading, unexecuted) |
| CAD-207 Typed nets | Connect/open/remove ports; add third physical terminal; focus/overview/zoom | One shared net with all terminals and preserved ID; Rust and source reject incompatible schemas; presentation is display-only. Trace: "Reading traces — Part J › CAD-207" (by reading, unexecuted) |
| CAD-208 Save/library | Save document; choose folder; export/import .rcomp | Preserved source identities/provenance; cancelled/stale export leaves existing destination intact. Trace: "Reading traces — Part J › CAD-208" (by reading, unexecuted) |
| CAD-209 Draft/mode | Close/resume rejected draft; switch/reopen while rebuild active | Draft retained; active work blocks document replacement/mode exit until terminal; stale retained draft refused. Trace: "Reading traces — Part J › CAD-209" (by reading, unexecuted) |

Creating/running a check remains cad-experiments-motion and requires the external
runner/reference workflow. Native expression fields do not provide RoboCAD's
computed Current-value table. Path fields, dock sections and graph port controls
are deliberate presentation differences, so this sequence cannot claim exact parity.

## Known differences (deliberate)

- RoboCAD asks Save/Discard/Cancel when closing; the viewer never saves for
  you and has no discard. It refuses to leave CAD mode, or to open or
  create another file, while a service it started has unsaved
  edits, and if the window closes it leaves that service running and logs
  its URL (attach with `--cad-url` and save).
- The viewer's camera and fit are its own; they never move RoboCAD's view or
  change geometry.
- A headless service has no command registry, autosave or load progress
  (RoboCAD's window provides those).
- cad-select-transform (each recorded in its module doc and in
  cad-parity.md): Measure does not copy to the clipboard; edges → faces is
  computed from RoboCAD's drawn tessellation, because its kernel's
  `faces_of_edge` has no REST route; the gizmo is the viewer's own (RoboCAD's
  handles), not Bevy's transform gizmo; a headless service doesn't store the
  selection mode, so the viewer holds it; the rotate/scale pivot for several
  nodes is the centre of their mesh bounds; a typed rotation after the X
  ring turns about X (RoboCAD turns about Z); the viewport footer (orbit
  hints, ms/frame) is not drawn.
- cad-modify (each recorded in cad-parity.md with its reason): mirror,
  cut, split, silhouette, draft and the radial array take a plane
  parameter whose default is the active plane (RoboCAD's fallback when none
  is active), and REST may name another; both box tools send `Ops.box` on
  XY and `Ops.box_three_point` on another plane (undo label "Box",
  RoboCAD's "Extrude"); the shell tool toggles faces on click (RoboCAD's toggles
  nothing); the viewer refuses by name where RoboCAD silently does nothing
  (empty selections for delete, copy, instance, unjoin, dissolve, cut,
  split, silhouette, set pivot, remove fillets, delete faces, the face
  edits, the analyses); copy keeps the clip in the viewer, not on the OS
  clipboard; per-node operations run one RoboCAD call per node inside one
  edit; the curvature comb reads the last selected curve and continuity
  the last selected node with a body (as RoboCAD leaves them drawn), and
  refuse by name when there is none; the selection is cleared only when
  the edit succeeds (as RoboCAD); the pie entries are
  rounded rectangles; Make unique is in the 3D view's right-click menu;
  the toolbar, menus, palette and radials list the unported commands
  disabled, naming why (no command belongs to a later epic any more); Command+Space is Spotlight's on macOS, so
  the palette opens with Control+Space or Shift+F.
- cad-sketch (each recorded in cad-parity.md with its reason): every
  sketch shape and edit is one REST sketch edit, which RoboCAD's history
  labels "Sketch (API)" (its GUI: "Sketch line", "Offset curves", …); a
  new sketch is created with the first finished shape, not when the tool
  starts; selecting one plane node makes it the active plane, and the
  active plane is drawn even when it is XY/XZ/YZ or hidden; the Rectangle,
  Circle and Slot toolbar buttons light while active; the right-click menu
  has a Sketch section; A runs the three-point arc; a slot's caps are
  drawn outward as the solid is (RoboCAD's viewport draws them inward);
  the text preview is a placeholder box; Enter outside a sketch tool's
  form fields does nothing (but finishes a spline); join with one curve,
  an offset of an empty sketch and a fillet with no corner to round are
  refused by name (RoboCAD records an empty step); the extrude preview is
  outlines without the taper; the extrude source follows the
  selection while the tool is active; a sketch shape in progress refuses
  leaving CAD mode.
- cad-views-export (each recorded in cad-parity.md with its reason): file
  commands use the viewer's path form, not the system's file dialog; New
  names its file first and opens it in this window, under the open rule
  (refused over a self-started service's unsaved edits); the mesh unit is
  a row of the import form, filled by RoboCAD's guess; an imported SVG or image lands on XY; export
  settings are remembered for the session and the form starts in the
  document's folder; the view cube is a net of buttons; matcap is
  approximated and render has no ground shadow; high contrast changes only
  the 3D view and is not kept; the tessellation field patches the
  inspected node with undo and opens empty; Isolate and Hide refuse an
  empty selection; Saved Views is a floating panel with a Restore button
  per row; the Blender live link, web share, draft-angle shading, the
  Preferences dialog, recovery saves and the SpaceMouse are not ported;
  curve nodes are drawn but not picked in the 3D view; the section offset
  is the toolbar's field (no plane drag, R or Tab in the 3D view). No
  cad-views-export row is open.
- cad-physical-inspect (each recorded in cad-parity.md with its reason):
  the exact measurement runs one read per node on a job, and a cancel only
  stops waiting; joint physics values the model lacks are empty, not 0.0;
  the colour is an "r, g, b" field with "Use material colour"; no
  drag-and-drop of a material onto a body; the properties dialog sends only
  changed keys and shows "not reported" where RoboCAD would use its
  built-in table; glyphs are sized at their own depth, dots are small
  crosses; the stress overlay uses Robot mode's log colour scale; the motor
  tool's dialog stays beside the view; the Robot panel has no branch
  glyphs, puts Detail and Margin under the name, omits "(n s run)" and
  writes "Error:"/"Warning:"; motors are listed by id; validation goes to
  the status line and issue list; the motor library is a panel; the power
  dialog's targets are one JSON field; exports are written by the viewer
  on a job; the live link opens Robot mode in this window; Ctrl+Shift+J is
  bound to the joint tool and Ctrl+Shift+M stays Select Same Material. No
  cad-physical-inspect row is open.
- cad-print (each recorded in cad-parity.md with its reason): the wall
  check remembers its last threshold (RoboCAD reopens at 1.2); validation
  has no open-edge lines (RoboCAD's desktop runs that tessellation check;
  its REST route serves only the kernel's report) and answers in the
  status line; the Fastener hole fields stay beside the view while you
  click, with a Point field for REST; split adds `expected_revision` to
  RoboCAD's background split; job progress drops " — Print ▸ Print jobs…
  to cancel", one list poll every 0.5 s replaces RoboCAD's per-job timer
  and a failure is the status line's error, not a box; Print jobs is a
  section of the right dock and "Cancel the running jobs?" an inline Yes/No
  row; the print overlay colours each part uniformly by its governing
  failure index on Robot mode's scale, not RoboCAD's per-voxel field;
  `GET /print/jobs/{id}`'s `wait` is never used (RoboCAD never honours
  it). No cad-print row is open.
- cad-organize (each recorded in cad-parity.md with its reason): Ctrl+F
  focuses the outliner's search only with the pointer over the model tree
  dock (elsewhere it is Fillet, RoboCAD's keymap binding); expand and
  collapse are refused while searching; a rename ends without renaming on
  focus loss, and an empty name (rename, New group, Group selection…) is
  refused by name; a drop on a group row's top quarter lands in front of
  the group; "Move to group" is a heading over its entries, not a
  submenu; `group.set_active` refuses by name with no group selected
  (RoboCAD silently clears the active group); Annotate and Reattach pick
  on the press and leave the selection mode alone; Enter posts in the
  composer; a part row has a single press and the label dialog is
  inline; a part link selects exactly the linked node; an unknown
  attachment state reads "Attachment unknown" with a grey pin; Show on
  model and Show only linked parts are reproduced from the thread list
  (RoboCAD's `/threads/{id}/show` is GUI-only), and a thread of experiment
  evidence opens its captured run in the viewer's captured-run review
  (RoboCAD: its experiments panel); a pin placed before RoboCAD's document
  changed is not posted (Annotate again places it; RoboCAD posts it as
  picked); reference images and the
  system file are typed in the viewer's path field, one image per submit,
  and non-image paths and drops are refused by name; a file dropped
  anywhere on the CAD window is imported; only PNG and JPEG (and MPO)
  references are textured, there is no preview thumbnail, and pixels are
  cached per connection and node; the calibrate distance is the
  References section's field; an image off XY, XZ, YZ and the plane
  nodes leaves the active plane alone and refuses Sketch over this; Open
  in builder switches this window to Build mode instead of starting a
  process. The components library and the system graph are
  cad-components'. No cad-organize row is open.
- cad-checklist-traces (2026-10-02; found while tracing Parts G to J by
  reading, each in its trace and in cad-parity.md): the material swatch
  colours only the ■, not the row text; a new material's undo step is
  "Material" through RoboCAD's `POST /materials` route (its dialog says
  "New material"; the route fixes the label, so matching needs a RoboCAD
  change); the load-results form always starts at `<stem>.simresult.json`;
  the split catalogue form needs exactly one body or sheet node, so a body
  plus a group is refused where RoboCAD splits the body (unverified
  against RoboCAD's window); the calibrate tool refuses a coincident
  second point on the click, RoboCAD at Enter, with the same text; an
  active group set in RoboCAD's own window shows only after the next
  revision change or `cad_refresh` (`set_active_group` does not move the
  revision: needs a RoboCAD change); the components rows are
  single-spaced and the filter matches the name only; the native Cancel
  rebuild cancels the one job it started (RoboCAD's cancels all);
  component expression values are unevaluated text (no computed Current
  column); a pending port pick is kept across a refresh in both, but the
  viewer refuses a stale one before sending (comparing generation,
  document and revision) while RoboCAD sends it at the first pick's
  revision and its service refuses it as a revision conflict (revision
  only); an invalid composition adaptation draws no graph in CAD's
  composition section (only the error), where Build's schematic keeps the
  last good layout; a start whose answer was lost waits at most
  30 s for RoboCAD to list it, then ends without retrying; a component
  draft that RoboCAD's revision has moved past can be copied onto the
  current revision with consent (Copy draft).
- cad-parts-a-f-retrace (2026-10-02; found while tracing Parts A to F by
  reading, unexecuted; each also named in its trace and in cad-parity.md):
  - **Server-side revision check (needs RoboCAD).** A stale drag, numeric
    entry, form, explicit-item run, kept measurement or window patch is
    refused by the viewer against RoboCAD's revision as last polled
    (`CadDocument::commit_refusal`), but `Service.op` (api.py:1011-1028)
    honours `expected_revision` only for component jobs, so an edit made in
    RoboCAD's own window between the poll and the send is not caught.
    Closing it needs `Service.op` to check `expected_revision` for every op.
  - **Port race with another headless RoboCAD (needs RoboCAD).** A headless
    RoboCAD on the same file, started by another process, that takes the
    port the viewer just chose is accepted as this window's service,
    because `GET /` names no process id; the viewer's child then exits on
    the taken port and the connection shows Lost. The other service keeps
    its edits and is never stopped here. A desktop RoboCAD there is refused
    (`cad/sync/launch.rs:accept_served`). Refusing the headless case needs
    a `pid` in RoboCAD's health answer.
  - Undo and redo say "Undid {label}" / "Redid {label}" on the status line,
    as every viewer mode's undo does (RoboCAD: "Undo {label}" /
    "Redo {label}"); the buttons and history are RoboCAD's labels.
  - The sub-body inspector shows the face, edge or vertex at the item's
    index at the shown revision even when it was picked at an older one,
    and its "Ø" rows are 2 × RoboCAD's radius, derived here; topology
    reads carry no revision (`GET /nodes/{id}/faces|edges|vertices`), so a
    late answer is labelled with the revision it was asked at until the
    next revision clears it. Commits stay guarded.
  - The Array form shows only the chosen kind's rows and adds an axis-plane
    choice (RoboCAD's `ArrayDialog` shows every row and uses the active
    plane). A typed negative cylinder height builds down along −normal, as
    the drag does (RoboCAD passes the signed height unchanged).
  - `cad_sketch` refusals for what RoboCAD would answer with a Python
    exception (a missing, non-finite or mistyped argument, a polygon of
    fewer than three sides, a join naming a curve twice, a negative curve
    index) name the call and the argument; where RoboCAD has its own words
    (collinear points, a curve index out of range, an unknown method, an
    unknown plane) the refusal uses them verbatim. A REST `cad_sketch`
    naming curves by index must pass `revision`. RoboCAD's
    `Service.edit_sketch` answers 500 for an `IndexError` (not changed;
    the viewer never sends one).
  - Views: the wheel zoom anchors on the plane through the focus with an
    exp(−0.12 × lines) step (RoboCAD: the active plane, 0.9 or 1.1 per
    event); leaving the trackball takes the nearest upright heading; an import leaves the camera where
    it is (RoboCAD frames everything); saving a view does not mark its row
    current.
  - A sent export runs to its end in RoboCAD (no cancel route): the job
    strip's Cancel says so and the outcome says the file was written
    anyway; a cancelled render is drawn by RoboCAD but its PNG is not
    written. `cad_open` (and File ▸ Open, New's open) stats the `.rcad` on
    the UI thread, a documented known cost shared with the switch's
    `prepare`; RoboCAD's service reads the file.

## Sign-off

When every step passes, record it in the coordination journal; the ledger's
`done-by-reading` rows then become `done`.


## T42 batch evidence IDs (source review; execution unverified)

| Required ID | Reading evidence |
| --- | --- |
| cad-components:outcome-1 | `cad/components/{form,validate,ui}.rs`, typed `cad_client/components.rs`, authoritative `components.py` |
| cad-components:outcome-2 | `component_service.py`, `component_jobs.py`, API routes and Qt delegation; `cad/components/jobs.rs` |
| cad-components:outcome-3 | `sim-system/composition*`, Resolver, `sim-diagram/composition.rs`, CAD/Build shared route presentation |
| cad-components:outcome-4 | All 38 component ledger rows mapped; current inventory and architecture T42 trace |
| cad-components:task-T42.1 | Shared typed payloads; headless service owner; component/import/recipe metadata routes and fixtures |
| cad-components:task-T42.2 | Native library/occurrence dock, kit forms, selection and durable jobs/recovery/refusal paths |
| cad-components:task-T42.3 | Explicit source envelope adaptation, shared validation/layout and guarded graph commands |
| cad-components:task-T42.4 | Independent reading reviews, written windowless/fake-service fixtures, Part J and architecture trace |

These IDs describe implementation and reading evidence. Part J, compilation and
exact reference parity remain unexecuted; no historical receipt is replaced.


### T42 repair acceptance additions (written, unexecuted)

| Step | Native workflow | Reference/observable acceptance |
|---|---|---|
| CAD-210 Explicit family target | Select library family; open LinkFamily with a distinct occurrence ID | Definition comes from library selection, ID names occurrence; same typed operation as no-ID shared-selection path. Trace: "Reading traces — Part J › CAD-210" (by reading, unexecuted) |
| CAD-211 Stale first pick | Pick a port; edit source/reload same-revision document; pick another or Leave open | First generation/document/revision stamp refused; pending intent and diagnostic retained. Trace: "Reading traces — Part J › CAD-211" (by reading, unexecuted) |
| CAD-212 Open/cancel/remove | Pick unused physical or signal-output port; Leave port open; separately Cancel connection or Remove connection | Leave creates singleton ID and undo; connected port refuses; Cancel performs no source edit; Remove deletes whole net. Trace: "Reading traces — Part J › CAD-212" (by reading, unexecuted) |
| CAD-213 Build failure | Present invalid shared graph twice; then change source key | Path-named diagnostic retained, unchanged failed key schedules no retry; prior layout labelled stale; changed key retries. Trace: "Reading traces — Part J › CAD-213" (by reading, unexecuted) |

## T43 source-reading handoff (not an executed checklist signoff)

All eight cad-experiments-motion batch IDs and source traces are in
[cad-experiments-motion-evidence.md](cad-experiments-motion-evidence.md).
Unexecuted acceptance sequence: rendered editor → Check → ImportComposition →
Run → Cancel/status receipt → history captured review → baseline compare →
sample annotation/show evidence → candidate read/geometry/accept or refuse →
reference pose/program edit/sweep/play/seek/return → native export/cancel.
Exercise missing/stale/replaced documents, closed docks, switched focused drafts,
Running cancel acknowledgments, ambiguous POST discovery/inspection, guarded undo
and export publication failure. Written fixtures inspect actual
Button/CadButton/Enabled entities rather than catalogue registration alone.
Python/OCCT, registry/experiment executables and local ffmpeg remain required;
no assigned step requires a Qt window. Exact parity and physical qualification
remain open. No fixture, build, launch, screenshot, capture or export ran.

## Reading traces — Parts A and B

Each trace follows one Part A or Part B step (CAD-01 to CAD-18) from the
native control to RoboCAD and back to what the top bar, the left dock (tree,
service and connection lines, autosave), the inspector, the status line, the
3D view or the file shows, and compares it with RoboCAD's own window.
Everything here is **by reading, unexecuted**: nothing was built, run or
captured, and no step was compared side by side. Native paths are under
`crates/sim-spatial/src/` unless they start with `crates/`; RoboCAD paths are
under `cad/robocad/`. Gaps found while tracing were fixed in this batch
(each named in its entry) unless an entry says it is recorded. The common
legs are written out once:

- **Click leg** (every toolbar button, chip and the inspector's buttons): a
  kit button carries `CadButton(action)` (`cad/panel.rs:55`), which
  `cad/panel/name.rs:buttons` (71) writes as `Act::ui(action)`; the one
  apply system `cad/actions.rs:apply` (354) drains `Act<CadAction>` into
  `handle` (459); a refusal from a click is written to the status line there
  (408). `system_ui` lists the same controls (`cad/panel.rs:own_controls`,
  191-245: `cad:undo`, `cad:redo`, `cad:save`, `cad:refresh`, `cad:fit`,
  `cad:physical`, `cad:delete`, `cad:locked:<id>`, `cad:disabled:<id>`,
  `cad:material:<id>:<m>`, `cad:node:<id>`, `cad:visible:<id>`).
- **Key leg** (a RoboCAD shortcut): `cad/keys.rs:keys` (352) in
  `CadKeySet::Keys`, after `keys::gate` (293, `CadKeySet::Gate`, registered
  `cad/surfaces/mod.rs:327`) by `cad/mod.rs:configure_sets` (177-184:
  `Gate.before(Keys)`, `Focus.before(Keys)`, `EscapeTool.before(Keys)`);
  it returns while a kit text field has the keyboard (`typing.get()`, 366),
  reads Control or Super as Ctrl (`combo_now`, 233-240), and writes
  `CadInvoke { id }` for a bound registry command when
  `surfaces/registry.rs:ready` (573) allows it (else the refusal on the
  status line, 413). `CadInvoke` → `cad/ops/mod.rs:handle` (491, 493) →
  `cad/ops/invoke.rs:invoke` (12): a catalogue entry runs there, any other
  registry command goes to `cad/surfaces/registry.rs:invoke` (619), whose
  `Resolved::Action` re-enters `actions::handle` (624).
- **Edit leg** (patch, delete, undo, redo, save, command):
  `cad/edit.rs:edit` (12) → `cad/sync/mod.rs:start_edit` (686): refused
  with nothing sent when `CadDocument::edit_refusal_for`
  (`cad/document/state.rs:182`) names an unknown outcome, a preview, a
  component rebuild, "another CAD edit is in flight: {label}" (193) or "not
  connected to RoboCAD: …" (196); else one `Job::spawn(Pool::Dedicated, …,
  "RoboCAD edit: …")` (693) off the UI thread with `EDIT_TIMEOUT`. The
  answer lands in `finish_edit` (565; an older generation's is dropped,
  573), which writes the outcome to the status line (601) and asks the poll
  for `/doc` (`refresh`, 641, called at 630; `dirty_known_at` makes the
  saved state "being refetched" until a `GET /` sent after the answer is
  read, 442-445). The status bar shows "Sending: {label}…" while it runs
  (`cad/panel.rs:status`, 635-638). REST callers wait on
  `cad/actions.rs:wait_edit` (602).
- **Poll leg** (what keeps the window current): `cad/sync/mod.rs:spawn_poll`
  (140) starts a `RunThread` "cad-poll" (jobs module) running `poll_loop`
  (163): every `POLL_PERIOD` 0.5 s (56) `GET /` (189), `GET /selection`
  (194), from a desktop window `GET /autosave` (205), and `GET /doc` and
  `GET /commands` when (document id, revision) moved or on Refresh
  (219-234). `receive` (285, `CadSet::Results` in `ViewerSet::JobResults`,
  `cad/mod.rs:214`) takes each snapshot once (`take_snapshot`, 413).
- **RoboCAD's route table**: `api.py:_route` (1540) → `run_on_main` → the
  `Service` method; `GET /` (1551) → `health` (459-460), `GET /doc` (1614) →
  `doc_state` (473: `doc.walk()` order, `history`, `selection`).

### CAD-01 Open

1. **Launch**: `main.rs:459-461` → `cad_mode` (250): the target is
   `CadTarget::File(path)` (`--cad-url` is the Part B path, 462-463); the
   document registry gets CAD's entry (`documents.open(Cad,
   cad_source(&target))`, 271) and the window opens with `CadDocument::new`
   (273; no I/O, `cad/document/mod.rs:179`). **Switch**: the switcher's
   **CAD**, `system_ui mode:cad` or `viewer_mode {"mode":"cad","path"}`
   (`app/switch/mod.rs:126-147`, `from_args`) → `handle` (373) → `start`
   (502): `leaving_blockers` (517) of the current mode, then `prepare`
   (`app/switch/prepare.rs:273-293`: a `.rcad` path, one stat named as a
   known cost at 283-287) → `Prepared::Now` with `CadDocument::new(target)`.
   The reveal hunk of `start` (`app/switch/mod.rs:553-561`) runs only for an
   accepted switch to CAD; a switcher, `system_ui` or `viewer_mode` request
   carries `reveal: None` (147, 201), so it sets `RevealThread(None)` with
   `set_if_neq` (no change when already None) and touches nothing Part A or
   B reads. `enter` (`app/switch/arrival.rs:22-36`) sets the state;
   `install` (155-165) opens CAD's registry entry (`sources::open`,
   `app/switch/sources.rs:106`) and inserts the document.
2. OnEnter(ModeScope::Cad): `cad/sync/mod.rs:enter` (264, registered
   `cad/mod.rs:201-209`) → `start` (76): a new generation (84), the
   connection "Connecting: starting RoboCAD's headless service on {path}"
   (127, 133; the line `connection_line`, `cad/document/state.rs:81`, adds
   "({N} s)"), and one `Job::spawn(Pool::Dedicated, …, "cad-start")` (127)
   running `cad/sync/launch.rs:self_start` (47). The top bar's state word is
   "Connecting…" (`cad/panel.rs:517`).
3. **The job** (off the UI thread): the interpreter
   (`crates/sim-runtime/src/cad_client/service.rs:interpreter`, 33: never
   creates the venv), the absolute path and its `is_file` (launch.rs:50-53),
   a free port (`service::free_port`, service.rs:51: bind 127.0.0.1:0, read,
   release), the command (`service_command`, 69: `python -m robocad.api
   DOC --port N --host 127.0.0.1`, stderr to `log_path`, 59), spawned as a
   `jobs::ChildProcess` (launch.rs:58; `jobs/child.rs:41`) and put in the
   document's `ChildSlot` at once (60; a slot closed meanwhile hands it back
   to be stopped, 60-63). Progress "started RoboCAD's headless service (pid
   P) at URL; waiting for it to load …" (64), which `finish_connect`
   (`cad/sync/mod.rs:331`) copies into the Connecting line (335-345).
   `service::wait_until_live` (service.rs:128) polls `GET /` every 150 ms
   up to `START_TIMEOUT` 120 s, ending early on the child's exit
   (`slot.exited`) or a cancel/closed slot (launch.rs:66-75).
4. **Port race** (the port was free when chosen but released before the
   child bound it): `accept_served` (launch.rs:24) accepts the first answer
   only from a headless RoboCAD serving this file (`serves`, 13: `app ==
   "robocad"` and the canonical path, run on the job thread); anything else
   is refused naming what answered ("… another service took the port"), the
   child is taken out of the slot and stopped (84-87), and the error carries
   the child's log tail (91). If instead the other process holds the port
   and our child fails to bind, the child exits, `wait_until_live`'s
   `alive` sees it and the job errs with the exit and log tail. Either way
   the connection is Lost with the error (`finish_connect`, 367), nothing
   but `GET /` was sent to the other process, and no edit exists yet in our
   child (it never answered). Gap found and fixed: a **desktop** RoboCAD
   window on the same file that took the port was accepted as this window's
   self-started service (`serves` checks only app and path); now `gui:
   true` is refused too (`cad/sync/launch.rs:24-37`, test
   `cad/lifecycle_tests.rs:302-305`), since the child `robocad.api` has no
   window (`api.py:1934-1945`: `ApiServer(doc)`, `app` None, so `health`
   answers `gui: false`, api.py:460). Recorded (needs RoboCAD): a
   **headless** RoboCAD on the same file started by someone else cannot be
   told apart, because `GET /` names no process id; it is accepted, our
   child then exits on the taken port, `watch_child` (377-391) marks the
   connection Lost naming that exit, and the other service, never stopped
   by this window, keeps its edits.
5. **Connected**: `finish_connect` (356-366): the URL, the log path, the
   poll worker (`spawn_poll`, 362), the client and health;
   `Connection::Connected`. The registry's selection entry for the target
   is made on the first `receive` (`ensure_registered`, 288-290).
6. Shown: the left dock's head (`cad/panel.rs:document`, 613-632): the
   path (`path_line`, 578), the service line "Self-started RoboCAD (pid P)
   at URL · headless · RoboCAD {version}" (`CadDocument::service_line`,
   `cad/document/state.rs:60-76`; "Starting RoboCAD's headless service (pid
   P) on {path}" while connecting, 63), the connection line "Connected ·
   revision N" (82-86), the autosave line "Autosave: not applicable
   (headless service; …)" (`cad/panel.rs:autosave_line`, 590-592). The UI
   thread never waits: the switch returns at once (`prepare.rs:269-272`)
   and every request is a job. RoboCAD's counterpart: its nonmodal
   "Opening name" window (`ui/model_loading.py:279-345`) then the document.

Deliberate difference (recorded, `cad-parity.md` "Load progress and
cancel"): the headless service loads before it binds, so the viewer shows
elapsed seconds, not RoboCAD's stage counts. By reading, unexecuted.

### CAD-02 Tree

1. No control: the left dock's tree part (`cad/panel.rs:450`,
   `Part::Tree`) → `cad/tree/rows.rs:draw` (148).
2. No action: the rows are `cad/tree/state.rs:shown` (181) over
   `CadDocument::rows` (`cad/document/state.rs:12-41`): `/doc`'s nodes in
   RoboCAD's walk order, each with its depth from the parent chain (bounded
   by the node count, 22), kind, name, `visible`, `effective_visible`,
   `locked`, `disabled`.
3. No RoboCAD call of its own: the poll leg's `GET /doc` (`cad/sync/mod.rs:222`,
   `crates/sim-runtime/src/cad_client/mod.rs:doc`, 271) → `api.py:1614` →
   `doc_state` (473: `nodes` = `node_summary` of `doc.walk()`, `document.py:420`;
   `effective_visible` = `doc.is_visible`, api.py:111, document.py:433).
4. Shown: each row indented `depth × INDENT` (rows.rs:186), the kind tag
   (214), the name grey when not effectively visible (`name_colour`, 137-145),
   "locked" (228-230), and the visibility chip `eye` (126-133): "Disabled"
   for a disabled node, "Shown" when effectively visible, "Hidden by parent"
   when the node's own flag is on but an ancestor hides or disables it,
   "Hidden" when its own flag is off. RoboCAD's counterpart `Outliner.refresh`
   (`ui/widgets.py:271-314`): the same walk from `doc.roots` (309-311) and
   children (304-305), the 👁/◌ column for its own `visible`, 🔒, ⏸, grey
   for `not doc.is_visible` (301-302).

Deliberate difference (cosmetic): words and an indent instead of Qt's icon
columns and disclosure tree; the chip names the effective state where
RoboCAD's 👁/◌ shows the node's own flag. By reading, unexecuted.

### CAD-03 Bodies

1. Display only (no control): `cad/mesh.rs:sync` (351, `CadSet::Mesh`,
   `cad/mod.rs:260`). **Home** (`view.fit`, `surfaces/registry.rs:317`, key
   leg → `Do::Fit` → `CadAction::CadFit`) or **Fit** (`cad:fit`,
   `cad/panel.rs:210`) → `cad/actions.rs:578` → `fit` (691) →
   `CadMeshes::frame` (`cad/mesh.rs:176`). Right-drag orbit, middle or
   Shift+right-drag pan and the wheel are the shared camera's
   (`camera/input.rs:78-117`).
2. Which bodies: `wanted` (mesh.rs:383) is every `effective_visible` node of
   a `BODY_KINDS` kind (40); hidden, disabled or deleted ones are despawned
   (385-394). Each missing mesh is fetched on one `Pool::Dedicated` job
   (468, at most `MAX_FETCHES`) and built on `Pool::Compute` (409).
3. `crates/sim-runtime/src/cad_client/mod.rs:mesh` (304) with
   `NODE_TOLERANCE` (the node's own tolerance) → `api.py:1648` → `mesh`
   (819: `doc.mesh_of`).
4. Frame: the root's transform is −90° about X and ×0.001
   (`root_transform`, mesh.rs:258-261; `display`, 253-255: (x, y, z) mm →
   (x, z, −y) m), so RoboCAD's Z-up millimetres show Z up in metres. RoboCAD's
   counterpart `Viewport.rebuild_item` (`ui/viewport.py:397-402`, the same
   `mesh_of`) and its Z-up world.

Matches RoboCAD. By reading, unexecuted.

### CAD-04 Select

1. **Tree row**: a press → `cad/tree/input.rs:rows` (221) writes
   `CadTree {op: select}` (262, `select`, 209) → `cad/tree/handle.rs:handle`
   (214-224) → `select_action` (174: Shift range, Ctrl/Cmd toggle) →
   `CadSelect`. **3D click**: `cad/pick.rs` writes `CadSelect { items,
   extend: shift, toggle: ctrl, picked_at }` (467; empty space clears, 469).
   REST `cad_select {"ids":[…]}`; `system_ui cad:node:<id>`
   (`cad/panel.rs:231`).
2. `cad/actions.rs:489` → `cad/selection/mod.rs:handle` (104-108) →
   `select` (216): validated, applied to the **one shared Selection**
   (`shared.apply`, 225), status "n selected" (`selection_status`, 196-199),
   then `publish` (132).
3. `publish` → `cad/sync/selection.rs:push_selection` (108): one push at a
   time (a newer one waits, `selection_again`), one `Pool::Dedicated` job.
4. `crates/sim-runtime/src/cad_client/mod.rs:set_selection` (403) →
   `api.py:1681-1684` → `set_selection` (1062: headless, the items only).
5. Shown: the row highlight (`cad/tree/rows.rs:highlight`, 106), the body in
   the selection material (`cad/mesh.rs:highlight`, 551), and the inspector
   follows the first selected node (CAD-05: `sync/selection.rs:detail`,
   123-155). RoboCAD's counterparts: `Outliner._select` (`ui/widgets.py:331-339`,
   body items for every row) and `SelectTool` (`ui/tools.py:111`).

Matches RoboCAD. By reading, unexecuted.

### CAD-05 Inspect

1. No control: selecting (CAD-04) → the right dock's `Part::Inspector`
   (`cad/panel.rs:460`) → `cad/inspector/node.rs:inspector` (202).
2. No action: `cad/sync/selection.rs:detail` (123), run by `receive`
   (`cad/sync/mod.rs:318`): one `GET /nodes/{id}` job per (first selected
   node, shown revision) on `Pool::Dedicated` (150-152); a result for
   another node or revision is dropped (131).
3. `crates/sim-runtime/src/cad_client/mod.rs:node` (288); bare
   `NaN`/`Infinity` tokens are read as null before decoding
   (`null_non_finite`, 476).
4. `api.py:1628-1629` (GET `/nodes/{id}`) → `node_detail` (114-135:
   `node_summary` plus `body_kind`, `mass` = `kernel.mass_properties`,
   `face_count`, `edge_count`, then `sketch`, `plane`, `measure`,
   `mirror_plane`, `joint`, `robot`).
5. Shown, from the shown tree's summary: Kind, Id, Parent ("null (a root)"),
   Children, Effective visibility, Material, Colour, "Instance of"
   (node.rs:207-218); Pivot and Transform (`cad/inspector/editors.rs:editors`,
   360-385: read-only `lines` of the transform for kinds RoboCAD does not
   place); then the detail (226-264): Body kind, Volume mm³, Area mm², Mass
   g, Centroid, Bounding box min/max, Size (mm), Faces, Edges, then each
   present block flattened. A mass value RoboCAD sent as null (or NaN) reads
   "null in RoboCAD's answer" without a unit (`mass_field`, `mass_vector`,
   `cad/inspector/mod.rs:195-212`); other JSON nulls read "null" (`scalar`,
   94). Nothing is computed or filled in (module doc, mod.rs:1-11). RoboCAD's
   counterpart `PropertiesPanel.refresh` (`ui/widgets.py:480-500`).

Deliberate difference (recorded in cad-parity.md "Inspector"): the native
inspector lists `node_detail` as returned where RoboCAD's panel shows a
display-size preview and exact measurements on request. By reading,
unexecuted.

### CAD-06 Physical labels

1. **Physical** (`cad:physical`, `cad/panel.rs:211-212`, enabled only while
   connected) → click leg → `cad/actions.rs:579` →
   `cad/sync/mod.rs:fetch_physical` (661): one `Pool::Dedicated` job stamped
   with the shown revision (663-664). REST `cad_physical`.
2. `crates/sim-runtime/src/cad_client/mod.rs:physical` (430, `flex=0`).
3. `api.py:1765-1778`: headless, `Ops.physical` under the lock; desktop, a
   snapshot derived by `export_worker.export_snapshot`. Each link carries
   `mass_sources` = {body id: `mass_source`} (`physical.py:766`), the
   `source` of a declared `mass_properties` block or RoboCAD's default label
   (`body_mass_properties`, `physical.py:134-154`).
4. `finish_physical` (669) stores (revision, result).
5. Shown: `cad/inspector/sections.rs:link` (33-79): "Fetching RoboCAD's
   physical model…" (37) while the job runs; for another revision the
   refetch hint (45-47); the link holding the body: Mass kg, Centre of mass
   m, Inertia kg·m², Bounding box m (62-69), then `mass_sources[id]` as a
   kit chip with RoboCAD's text verbatim (`provenance`,
   `cad/inspector/mod.rs:158-168`; sections.rs:73-78). A link without the
   key shows no chip; RoboCAD sends a label for every body
   (physical.py:139, 151, 154), so a member body always shows one.
   RoboCAD's counterpart: the same `GET /physical?flex=0` answer.

Matches RoboCAD. By reading, unexecuted.

### CAD-07 Patch

1. **Visible**, **Locked**, **Disabled**, a material chip
   (`cad/inspector/sections.rs:attributes`, 96-130; their actions are
   `cad/panel.rs:own_controls`' `patch(id, key, value)`, 140-145, 219-233),
   the tree row's visibility chip (`cad/tree/rows.rs:231`), or the name
   field (`cad/inspector/node.rs:name`, 20-45, the kit field; Enter →
   `cad/panel/name.rs:119-135` writes `patch(id, "name", text)`). REST
   `cad_patch`.
2. `cad/actions.rs:506-521`: refused for a node not in the shown tree;
   label "Patch {name}: {keys}". A window control (a click, or its
   `system_ui` activation: not `call.rest()`) passes the shown revision
   (519) to `cad/edit.rs:edit_at` (48), so `commit_refusal`
   (`cad/document/state.rs:138-153`) refuses it with nothing sent while
   the shown tree is behind RoboCAD's ("the shown document is behind
   RoboCAD's (…); nothing was sent") or RoboCAD's revision moved since; a
   REST `cad_patch` names its own values and goes straight to the edit
   leg. A second edit while one is in
   flight is refused "another CAD edit is in flight: Patch {name}: {keys}"
   (`cad/document/state.rs:192-193`), and the chips are disabled meanwhile
   (`edit_blocked`, `cad/panel.rs:131`).
3. `crates/sim-runtime/src/cad_client/mod.rs:patch` (294) → `api.py:1630`
   → `patch` (748-786): `name` → `ops.rename` (`commands.py:333`),
   `visible`/`locked`/`disabled`/`material` → `Ops.set_visible`,
   `set_locked`, `set_disabled`, `set_material` (336-347), one undo step
   each.
4. Shown: "Sending: Patch …" then "Patched {name}: {keys}"; the refetched
   `/doc` redraws the tree and inspector. RoboCAD's counterpart: its
   properties panel and outliner columns call the same `Ops` methods
   (`ui/widgets.py:359-364`).

Matches RoboCAD. Gap found and fixed (this batch): a chip's value is the
opposite of the shown tree's flag, and it used to be sent while the shown
tree was behind RoboCAD's; now a window patch is sent only at the shown
revision (`cad/actions.rs:513-520`), where RoboCAD's panel always reads its
live document. By reading, unexecuted.

### CAD-08 Undo / redo

1. **Undo {label}** / **Redo {label}** (`cad:undo`, `cad:redo`,
   `cad/panel.rs:197-201`: the label is the last entry of `/doc`'s
   `history.undo`/`redo`; disabled when empty or edits are blocked), or
   Cmd/Ctrl+Z and Cmd/Ctrl+Shift+Z: key leg (`edit.undo`/`edit.redo`,
   `cad/surfaces/registry.rs:307-308`, `Native::Action(Do::Undo/Redo)`,
   `Do::action`, 111-112) in `CadKeySet::Keys` after the Gate, never while
   a text field types (`cad/keys.rs:366`); `ready` takes the button's
   readiness (`registry.rs:579`). REST `cad_undo`, `cad_redo`.
2. `cad/actions.rs:531-536` → edit leg ("Undo", "Redo").
3. `crates/sim-runtime/src/cad_client/mod.rs:undo` (390), `redo` (394) →
   `api.py:1677-1680` → `undo`/`redo` (1046-1054) → `Ops.undo`/`redo`
   (`commands.py:305-309`) on the one command stack.
4. Shown: status "Undid {label}" / "Redid {label}" ("Nothing to undo"); the
   inspector's History (`cad/inspector/sections.rs:history`, 131-146) lists
   RoboCAD's labels most recent first. RoboCAD's counterpart:
   `ui/app.py:291-292` (status "Undo {label}").

Matches RoboCAD: the labels are RoboCAD's and each CAD-07 patch is one step.
Deliberate difference (cosmetic): the status line says "Undid"/"Redid", as
every viewer mode's undo does, where RoboCAD's says "Undo"/"Redo". By
reading, unexecuted.

### CAD-09 Delete

1. **Delete** in the inspector (`cad/inspector/sections.rs:123-127`, the
   control `cad:delete` = `CadInvoke { edit.delete }`, `cad/panel.rs:214-216`)
   or Delete/Backspace: key leg (`edit.delete`, `registry.rs:309`), not
   while a text field (the name) types (`keys.rs:366`), silent on an empty
   selection (411-412). REST `cad_delete {id}` (one node).
2. `CadInvoke` → `cad/ops/invoke.rs:invoke` (15, `Flow::Immediate`) →
   `ops::run` (`cad/ops/mod.rs:528`) → `prepare` (543: `commit_refusal`, so
   also refused while the shown tree is stale) → `start` → edit leg; the
   entry (`cad/ops/catalogue/edit_create.rs:8-20`) clears the selection on
   success. `cad_delete` → `cad/actions.rs:522-529` → edit leg.
3. `crates/sim-runtime/src/cad_client/mod.rs:op` (374, `POST /ops/delete`,
   every selected node) or `delete` (298, `DELETE /nodes/{id}`) →
   `api.py:1668`/`1632` → `Ops.delete` (`commands.py:312`, one
   `RemoveNodes("Delete")` step) / `delete` (api.py:789-793).
4. Shown: the node leaves the refetched tree and the 3D view (mesh.rs:385-394);
   Undo (CAD-08) restores it. RoboCAD's counterpart `delete_selection`
   (`ui/app.py:1458-1463`).

Deliberate difference (recorded, cad-parity.md "Delete"): a menu or REST
delete with nothing selected is refused by name. By reading, unexecuted.

### CAD-10 Unsaved edits

1. After an edit, the top bar shows "Checking saved state…" until the poll
   reads a `GET /` sent after the answer, then "Unsaved edits"
   (`cad/panel.rs:top`, 545-552, from `CadDocument::unsaved`,
   `cad/document/state.rs:165-170`: `health.dirty`, `api.py:460`).
2. **Build** in the switcher → `app/switch/mod.rs:handle` (373) → `start`
   (502) → `leaving_blockers` (517; `app/switch/prepare.rs:30-86`) → for CAD
   `CadDocument::switch_blockers` (prepare.rs:79-80;
   `cad/document/state.rs:206-238`).
3. **Unsaved edits, mode leave refused** (the table's text): with the
   self-started child running and answered (`child_may_hold_edits`, 174-176)
   and `unsaved() == Some(true)`, the blocker is "{name} has unsaved edits in
   the RoboCAD service this window started (pid P), which stops when CAD
   mode closes: save first (the Save button)" (229); not connected, or an
   edit just finished, gives the "may have unsaved edits …" variants
   (230-234); an edit in flight is its own blocker (212-214). `start`
   returns `refusal` (`app/switch/mod.rs:483-485`): "Not switching to Build
   mode: … save first (the Save button). CAD mode stays." Nothing is
   stopped and CAD mode stays.
4. **Race between the check and OnExit** (edits begun after
   `leaving_blockers` ran, e.g. an `Act<CadAction>` applied later in the
   same `ViewerSet::Actions` frame): OnExit(Cad) → `app/switch/leave.rs:leave_cad`
   (93) → `release_child("leaving CAD mode")` (`cad/document/state.rs:276-294`):
   an answered, running child whose `unsaved()` is not `Some(false)` (an
   edit in flight makes it None, 166) is detached (`ChildProcess::detach`,
   `jobs/child.rs:94`), left running, its URL logged, and the URL returned;
   `leave_cad` records `CadTarget::Service(url)` as CAD's source
   (leave.rs:104, 109; `sources::left`, `app/switch/sources.rs:133`), so the
   next visit attaches to it (`prepare.rs:290`) instead of starting a new
   service. Gap found and fixed: the switch's message said only "Switched to
   Build mode." while the service was left running (the URL was only in the
   log); `leave_cad` now appends "the RoboCAD service this window started
   for {name} may hold unsaved edits and is left running at {url}; CAD mode
   reattaches to it (save there, or stop it)" to the switch's `entering`
   summary, which `handle` shows once the mode is entered
   (`app/switch/leave.rs:96-105`).

Deliberate difference (recorded, cad-parity.md "Closing with unsaved
changes"): RoboCAD's window prompts Save/Discard/Cancel
(`ui/app.py:1920-1934`); the viewer never saves for the user, so it refuses
to leave or keeps the service running. By reading, unexecuted.

### CAD-11 Save

1. **Save** (`cad:save`, `cad/panel.rs:202`) or Cmd/Ctrl+S (key leg,
   `file.save`, `registry.rs:301`, `Do::Save` → `CadSave { path: None }`,
   113); REST `cad_save` or `cad_save {"path"}`.
2. `cad/actions.rs:540-543`: a path goes through `cad/files/mod.rs:absolute`
   (247-263: `~/` expanded; a relative path is refused "cad_save: {p} is not
   an absolute path (RoboCAD would resolve it against its own working
   directory)", 256-258; a trailing `/` is refused, 259-261) → `files::save`
   (406): `.rcad` appended (409), label "Save" / "Save as {p}", edit leg with
   `FILE_TIMEOUT`; a save to a path marks its own edit `retarget` (419-425).
3. `crates/sim-runtime/src/cad_client/files.rs:save_with_thumbnail` (152,
   `POST /save/thumbnail {path?}`) → `api.py:1736-1737` →
   `save_with_thumbnail` (1296-1319): headless, a 256×192 snapshot render
   as the thumbnail, then `Document.save(p, thumbnail=…)` (RoboCAD writes
   the archive; the viewer never writes the `.rcad`).
4. Shown: "Saved {p} with its thumbnail" (or "without a thumbnail: RoboCAD
   could not draw one", files/mod.rs:415); the refetched `GET /` clears
   `dirty`, so the top bar says "Saved" (`cad/panel.rs:550`). After a save
   to a path, `finish_edit` retargets a self-started document to the saved
   file (`cad/sync/mod.rs:605-610`) and `receive` points CAD's registry
   entry at it (312-314); the left dock's path line is RoboCAD's new path
   (`path_line`, `cad/panel.rs:578-585`). RoboCAD's counterparts
   `MainWindow.save`/`save_as` (`ui/app.py:1337-1350`: `doc.save(…,
   thumbnail=self.thumbnail())`).
5. **Save-then-leave**: once the refetch reads `dirty: false`, CAD-10's
   blocker is gone and CAD-12 stops the service (`release_child` keeps it
   only when `unsaved() != Some(false)`, `cad/document/state.rs:277`).

Matches RoboCAD. By reading, unexecuted.

### CAD-12 Leave

1. **Build** in the switcher (as CAD-10, now with no blocker) → `start` →
   `prepare` → `enter` (`app/switch/arrival.rs:22-36`; `leaving_note` is
   None for a self-started document, `cad/document/state.rs:243-245`).
2. OnExit(Cad) → `leave_cad` (`app/switch/leave.rs:93`) →
   `release_child` (`cad/document/state.rs:276`): the slot is closed and,
   the saved state being `Some(false)`, the child is stopped
   (`ChildProcess::stop`, `jobs/child.rs:88-115`: kill, reaped on a reaper
   thread, never blocking); the document (its poll worker) is dropped off
   the UI thread (leave.rs:108); CAD's registry entry keeps the file
   (`sources::left`, 109) and `cad::clear` drops the caches
   (`cad/mod.rs:317`).
3. No RoboCAD call: the process ends. Switching back to **CAD** → `prepare`
   finds the remembered `CadTarget::File` (`app/switch/prepare.rs:290`,
   `sources::cad_target`, `app/switch/sources.rs:63`) → a new
   `CadDocument` → `sync::enter` → CAD-01's start, a new service on the same
   file (a new pid in the left dock).
4. RoboCAD's counterpart: closing its window stops its API
   (`ui/app.py:1920-1959`).

Matches RoboCAD. By reading, unexecuted.

### CAD-13 Shared selection

1. **Attach** (Part B): `main.rs:462-463` (`--cad-url`, refused unless a
   loopback URL, 266-269) or `viewer_mode {"mode":"cad","url"}`
   (`app/switch/mod.rs:126-147`, `Document::Url`) → `prepare.rs:275-278` →
   `CadTarget::Service(url)`; or the left dock's attach field while not
   connected (`cad/attach.rs:74`, `CadOpen {url}`). `sync::start`'s Service
   branch (`cad/sync/mod.rs:114-124`): one `GET /`, refused unless `ok:
   true`; `self_started: false`, the slot stays empty.
2. Viewer → RoboCAD: CAD-04's `CadSelect` → `publish` →
   `push_selection` (`cad/sync/selection.rs:108`) → `set_selection` (403)
   with the mode (`selection_body`, 100-102) → `api.py:1684` →
   `set_selection` (1062-1072): a desktop window sets its viewport's items
   and mode and calls `selection_changed(None)` (`ui/app.py:587-595`:
   properties, outliner `sync_selection`, viewport).
3. RoboCAD → viewer: the poll's `GET /selection` every 0.5 s
   (`cad/sync/mod.rs:194`; `api.py:1057-1060`, items and mode) →
   `take_snapshot` (547-552) → `adopt_selection` (`sync/selection.rs:25`):
   adopted unless our push is in flight or the read predates its answer
   (26-29), the desktop window's mode adopted when it changed (32-40),
   items of nodes absent from a current tree dropped (46-49), set in the
   shared selection and recorded as published so it is never pushed back
   (51-60).
4. Shown: each window highlights the other's pick within one poll period
   (0.5 s plus the request).

Matches RoboCAD. By reading, unexecuted.

### CAD-14 Edits both ways

1. Viewer: hide (the eye chip or **Visible**, CAD-07) and rename (CAD-07's
   name field or the tree's double-click, `cad/tree/handle.rs`); RoboCAD:
   move a part and change a material in its window.
2. Viewer edits: `CadPatch` / `CadTree {rename}` → edit leg → `PATCH
   /nodes/{id}` (`api.py:748-786`) on the same `Ops` stack RoboCAD's
   window uses (`commands.py:333-340`).
3. RoboCAD's edits move `doc.revision`; the poll's `GET /` sees the new
   (document id, revision) and fetches `/doc` (`cad/sync/mod.rs:219-234`),
   `take_snapshot` adopts the tree (488-492) and the meshes refetch for the
   new revision (`cad/mesh.rs:383-394`, 468).
4. Shown: both windows show all four changes; the inspector's History
   (CAD-08) lists RoboCAD's one stack in order; RoboCAD's Edit menu shows
   the same.

Matches RoboCAD. By reading, unexecuted.

### CAD-15 Commands

1. The inspector's commands section (`cad/inspector/sections.rs:commands`,
   153-199): `/commands` grouped by category (187-190), each a button
   labelled with its keys (`command_label`, `cad/panel.rs:156`) carrying
   `CadCommand { id }` (197); a headless service shows
   `HEADLESS_COMMANDS` (170, 174; `cad/panel.rs:162`). `system_ui cad:command:<id>`
   (`cad/panel.rs:270`); REST `cad_command {"id":"view.fit"}`.
2. `cad/actions.rs:544-550`: an id the outliner, threads, references,
   components, experiments or motion own natively runs here
   (`registry::organize_action`, 105-107); any other → edit leg "Command
   {id}".
3. `crates/sim-runtime/src/cad_client/mod.rs:run_command` (382) →
   `api.py:1818-1820` → `run_command` (1497-1503): the window's registry
   entry's `run()` (409 "no GUI" headless).
4. Shown: "Ran RoboCAD command view.fit"; the command runs in RoboCAD's
   window. RoboCAD's counterpart: its menus and palette run the same
   registry (`ui/widgets.py:52-136`).

Matches RoboCAD. By reading, unexecuted.

### CAD-16 Autosave

1. No control: the poll reads `GET /autosave` every tick from a desktop
   window (`cad/sync/mod.rs:198-214`; one failed read after an Ok keeps the
   shown state, 206-210); `take_snapshot` adopts a changed value (497-500).
2. `crates/sim-runtime/src/cad_client/mod.rs:autosave` (425) →
   `api.py:1618-1619` → `autosave` (462-471): `running`, `revision` being
   written, `saved_revision`, `path` (`doc.autosave_path()`).
3. Shown: the left dock's line (`cad/panel.rs:autosave_line`, 588-610):
   "Autosave: running / not running · saved revision N · writing revision M
   · path"; an error verbatim in red. RoboCAD's counterpart: its timer
   (`ui/app.py:103-112`, `_autosave` 135-147, `_finish_autosave` 149-160).

Matches RoboCAD. By reading, unexecuted.

### CAD-17 Leaving keeps edits

1. **Build** with unsaved edits in the attached RoboCAD → `start` →
   `leaving_blockers` → `switch_blockers`: no blocker, the slot being empty
   (`child_may_hold_edits` false, `cad/document/state.rs:174-176, 224`).
2. `enter` (`app/switch/arrival.rs:23-26`) adds `leaving_note`
   (`app/switch/prepare.rs:90-92` → `cad/document/state.rs:242-253`):
   "Switched to Build mode; RoboCAD at {url} keeps the unsaved edits to
   {name}." (or "had unsaved edits … when last read" when unconfirmed).
3. OnExit: `release_child` finds no child (`self.child.close()?`, 278) and
   returns None; the target stays the URL (`leave.rs:106`); RoboCAD is
   never stopped.

Matches RoboCAD (its window keeps the edits). By reading, unexecuted.

### CAD-18 Service loss

1. Quit RoboCAD: the poll's `GET /` fails (`cad/sync/mod.rs:189`,
   `POLL_TIMEOUT` 5 s, connection refused at once) → `take_snapshot`
   (464-470): `Connection::Lost { error }` verbatim; the tree, meshes and
   inspector stay (only the connection changes).
2. Shown: the top bar's "Lost" (`cad/panel.rs:519`), the left dock "Not
   connected: {error}" (`cad/document/state.rs:100`) and the attach field
   (`cad/panel.rs:629-631`). Every edit is refused "not connected to
   RoboCAD: Not connected: {error}" (`edit_refusal_for`, 195-196); the
   inspector's detail says so (`cad/inspector/node.rs:waiting`, 189-199).
3. Reconnect: the poll keeps asking; the first `GET /` that answers turns
   Lost into Connected (`take_snapshot`, 452-461; meshes and a failed detail
   retried), and the new (document id, revision) refetches `/doc`.
   **Refresh** (`cad:refresh`, always enabled, `cad/panel.rs:203-205`) →
   `cad/actions.rs:refresh` (675-688): an attached client is kept and the
   poll asked to fetch now (`sync::refresh`, 641).
4. RoboCAD's counterpart: its own quit prompt (`ui/app.py:1920-1934`).

Matches RoboCAD. By reading, unexecuted.

### Unsaved edits: window close, mode leave, port race, document close

1. **Window close with a self-started service holding unsaved edits.**
   `AppExit` → `cad/sync/mod.rs:on_exit` (707-714; `Last`, after
   `bevy::window::ExitSystems`, `cad/mod.rs:236`, so the message is read in
   the frame it is written) → `release_child("the window closed")`
   (`cad/document/state.rs:276-294`): the child answered, runs and is dirty
   or unconfirmable, so it is detached (`jobs/child.rs:94-100`), left
   running and logged: "the window closed: the RoboCAD service this window
   started (pid P) holds unsaved edits to {name}; it is left running at URL
   so they are not lost: open it there (sim-spatial --cad-url URL) and save,
   or stop it" (281-287). The slot is then empty, so dropping the World
   cannot kill it (`ChildProcess`'s `Drop`, `jobs/child.rs:120-124`, acts
   only on a held child). A clean one is stopped.
2. **Mode leave refused**: CAD-10 point 3 (`leaving_blockers` →
   `CadDocument::switch_blockers`, `cad/document/state.rs:224-236`).
   **Edits between the check and OnExit**: CAD-10 point 4 (`leave_cad`
   detaches, records the URL, reattaches next visit, and now says so in the
   switch's message, `app/switch/leave.rs:96-105`).
3. **Port race at service start**: CAD-01 point 4 (`accept_served`,
   `cad/sync/launch.rs:24-37`; nothing but `GET /` reaches another process,
   and our child holds no edits before it answers).
4. **Closing or replacing a document** (`cad_open`, File > Open…, File >
   New, the attach field): `cad/actions.rs:open` (624) checks
   `switch_blockers` (644-647) and refuses "Not opening {target}: {name}
   has unsaved edits in the RoboCAD service this window started …: save
   first"; File > New checks the same rule before RoboCAD writes the file
   (`cad/files/mod.rs:341-344`) and opens the created file through the same
   `CadOpen` (`cad/files/jobs.rs:188, 204-205, 250-251`), so the rule is
   checked again when it lands. Accepted, the old document's child is
   released (`release_child("opening another CAD document")`, actions.rs:651:
   left running if edits appeared since the check, and the message names
   its URL, 666-668), the new document starts (`sync::start`, 654) and the
   old one is dropped off the UI thread. The guard in
   `app/switch/arrival.rs:159-163` releases a replaced document the same
   way.

Deliberate difference (recorded in Known differences): RoboCAD asks
Save/Discard/Cancel on close; the viewer never saves for you and never
discards, it refuses or leaves the service running, so no edit is lost in
any of the four cases. By reading, unexecuted.

## Reading traces — Part C

Each trace follows one Part C step (CAD-19 to CAD-34: sub-body selection
and the direct tools) from the native control to RoboCAD and back to what
the 3D view, the selection strip, the tool bar and numeric bar, the
inspector and the status line show. Everything here is by reading,
unexecuted: nothing was built, run or captured, and no step was compared
side by side. Native paths are under `crates/sim-spatial/src/` unless they
start with `crates/`; RoboCAD paths are under `cad/robocad/`. Gaps found
while tracing were fixed in this batch (each named in its entry), unless an
entry says it is recorded.
The common legs are written out once:

- **Key leg** (B, Shift+B, E, V, P, Ctrl/Cmd+A, Ctrl/Cmd+Shift+I,
  Ctrl/Cmd+Shift+M): `cad/keys.rs:keys` (344; `CadKeySet::Keys`) returns
  while a kit text field has the keyboard (358) and matches RoboCAD's
  keymap (`keymap.json:5,7` → `cad/surfaces/registry.rs:312-314,344-348`),
  then writes `CadInvoke { id }`; `cad/actions.rs:handle` (561) →
  `cad/ops/mod.rs:493` → `cad/ops/invoke.rs:invoke` (12; not a catalogue
  entry) → `cad/surfaces/mod.rs:invoke_command` (246) →
  `cad/surfaces/registry.rs:invoke` (619) → `Resolved::Action` (624) →
  the native action (`Do::action`, 109-120) back into `actions::handle`.
  The tool keys G, R, S, D, Shift+D, M and Escape are not in that map
  (`cad/keys.rs:25-27`): they are `cad/transform/input.rs:keys` (67;
  `CadKeySet::ToolKeys`, `keys::free`, registered `transform/mod.rs:271`),
  which writes the matching `panel::controls` action (93-99).
- **Click leg** (the strip's mode segments and buttons, the Alt menu's
  entries, the tool strip): a kit button carries `CadButton(action)`
  (`cad/panel.rs:55`), written as `Act::ui` by `cad/panel/name.rs:buttons`
  (71); the controls are `cad/panel.rs:240-265` (`cad:mode:*`,
  `cad:select_all`, `cad:invert_selection`, `cad:select_same_material`,
  `cad:edges_to_faces`, `cad:candidate:<n>`, `cad:tool:*`, `cad:cancel`),
  also listed by `system_ui`.
- **Apply leg**: `cad/actions.rs:apply` (354) drains `Act<CadAction>` into
  `handle` (459): the selection arms (489-497) go to
  `cad/selection/mod.rs:handle` (104), the tool arms (498-505) to
  `cad/transform/mod.rs:handle` (363). A refusal of a UI action is written
  to the status line (`actions.rs:407-409`); a REST caller gets it.
- **Selection leg**: every selection arm changes the one shared selection
  through `Shared::apply` (`cad/selection/shared.rs:165` →
  `selection/mod.rs:Selection::apply`, 257, which refuses an item picked
  at another revision by name, 264), sets RoboCAD's status
  ("n selected" or empty → "Ready", `cad/selection/mod.rs:170-173`,
  `cad/panel.rs:647`) and `publish`es (132): the panels are touched and,
  when connected and different, the items and mode go to `PUT /selection`
  (`cad/sync/selection.rs:push_selection`, 108; one `Pool::Dedicated` job,
  115) → `crates/sim-runtime/src/cad_client/mod.rs:set_selection` (403) →
  `api.py:_route` (1540) `/selection` (1681-1684) → `api.py:set_selection`
  (1062: a desktop window's `viewport.selection` and mode, then
  `selection_changed`; headless, `_headless_selection` without the mode).
- **Edit leg** (every commit): `cad/transform/commit.rs:commit` (264)
  refuses with nothing sent when `CadDocument::commit_refusal`
  (`cad/document/state.rs:138`) names a reason (an edit in flight, 193:
  "another CAD edit is in flight: …"; not connected; the shown document
  behind RoboCAD's, 146; RoboCAD's revision moved since `began`, 149-151)
  → `op_for` (202) → `send` (246) → `cad/edit.rs:edit` (12) →
  `cad/sync/mod.rs:start_edit` (686): one `Job::spawn(Pool::Dedicated, …,
  "RoboCAD edit: …")` (693) → `crates/sim-runtime/src/cad_client/mod.rs:op`
  (374, `POST /ops/{name}`) → `api.py:1668-1671` → `api.py:op` (1011) →
  the `Ops` method pushing one command on RoboCAD's stack
  (`commands.py:212`) → `cad/sync/mod.rs:finish_edit` (565: status 601,
  refetch `refresh` 630/641).
- **Topology leg** (sub-body indices, polylines, inspector values):
  `cad/topology.rs:sync` (139) fetches, per wanted node (`wanted`, 113:
  the selected nodes, and every drawn body in a sub-body mode or a tool),
  `GET /nodes/{id}/faces`, `/edges?samples=24`, `/vertices` on one
  `Pool::Dedicated` job each (183) → `cad_client/mod.rs:faces` (316),
  `edges` (323), `vertices` (332) → `api.py:1642-1647` →
  `api.py:faces` / `edges` / `vertices` (796-817: `face_json`,
  `edge_json` plus `kernel.sample_edges` points). `CadTopology::get` (69)
  answers only at the shown revision.

### CAD-19 Selection modes
1. The strip's segments (`cad/overlay.rs:strip`, 266; `cad:mode:<m>`,
   `cad/panel.rs:240`, click leg), the keys B, Shift+B, E, V, P (key leg,
   `registry.rs:344-348`), the selection radial (Q, `registry.rs:284,349`)
   or REST `cad_select_mode`.
2. `CadSelectMode { mode }` → `cad/selection/mod.rs:set_mode` (239): the
   mode set, the shared selection cleared (242), hover and Alt menu
   dropped, status "Selection mode: face" (245), `publish` (selection leg;
   the mode travels with the items).
3. RoboCAD: `PUT /selection {"items": [], "mode"}` → `api.py:set_selection`
   (1062-1072). Its own window: `ui/app.py:set_selection_mode` (597-602:
   mode, `selection.clear()`, the same status text).
4. Shown: the tool bar head "Select  ·  Face" (`cad/transform/mod.rs:
   mode_label`, 352, drawn by `cad/numeric.rs:head`, 335); edge mode
   draws every drawn body's sampled edges faintly (`cad/overlay.rs:
   edge_lines`, 89-134, one retained gizmo), vertex mode marks every vertex
   (`highlights`, 240-253); the topology leg starts for every drawn body
   (`topology.rs:116`). B, E, V, P do nothing while a field types
   (`cad/keys.rs:358`; the kit also consumes the key,
   `ui_kit/text/input.rs:248`).

Matches RoboCAD. By reading, unexecuted.

### CAD-20 Click, Shift, Ctrl
1. A left press and release within 6 px (Manhattan, `cad/pick.rs:431`) in
   the Select tool, not over UI, no field typing (`usable`, 404) →
   `candidates_at` (343): body mode or a mesh node `[id, "body", 0]`;
   face and point modes the nearest unlocked ray hit mapped by
   `CadMeshes::face_at` at the shown revision (`surface_item`, 208-215);
   edge and vertex modes `search` (264: within 6 px, not behind the first
   surface). REST `cad_select {items, toggle}`.
2. `CadSelect { items, extend: Shift, toggle: Ctrl/Cmd, picked_at: shown
   revision }` (467); empty space without Shift or Ctrl writes an empty
   `CadSelect` (469) → `cad/selection/mod.rs:select` (216): `validate`
   (177), the op (`op`, 196: Ctrl toggles, Shift adds, else set), items
   stamped with `picked_at` (222-225), selection leg.
3. RoboCAD `PUT /selection` (selection leg). Its window:
   `ui/tools.py:SelectTool.release` (131-149) → `_apply` (151-169), the same
   rule; `selection_changed` status `ui/app.py:594-595`.
4. Shown: the status "n selected" / "Ready"; the selected faces, edges,
   vertices and points outlined (`cad/overlay.rs:highlights`, 254-256),
   bodies by material; locked nodes are left out of the ray cast
   (`pick.rs:189,193`) and the edge/vertex search (`search_for`, 240) and
   so do not hide what is behind them; hidden bodies are not drawn and the
   cast takes only visible ones (`RayCastVisibility::Visible`, 188), as
   RoboCAD's pick pass (`ui/viewport.py:1262`).

Matches RoboCAD. By reading, unexecuted.

### CAD-21 Hover
1. Pointer motion over the 3D view in the Select tool (`cad/pick.rs:
   pointer`, 476-533), searched at most once per 33 ms (`HOVER_PERIOD`,
   489-494).
2. Body, face and point modes: the cursor ray's nearest hit, inline
   (496-502). Edge and vertex modes: `search` on a `Pool::Compute` job
   (`start_hover`, 544-548), one in flight, a newer search replaces a
   waiting one (503-513, 517-533). Only a changed item writes
   `Act::quiet(CadHover)` (`set_hover`, 537) →
   `cad/selection/mod.rs:hover` (251): `doc.hover` set, no `touch`, no
   push.
3. No RoboCAD call. RoboCAD's counterpart: `ui/tools.py:SelectTool.hover`
   (171-183) over `request_hover` (`ui/viewport.py:1226-1247`, a 33 ms
   timer).
4. Shown: `cad/overlay.rs:highlights` (257-259) draws the hovered face
   outline, edge polyline, vertex mark or, for a body, its bounding box
   (`draw`, 181-226) in the accent colour; the selection, inspector and
   status are unchanged (no `touch`, no `publish`); the edge search never
   runs on the UI thread.

Matches RoboCAD. Deliberate difference (already recorded in `pick.rs`
module doc): a body hover is drawn as its box, not RoboCAD's tint. By
reading, unexecuted.

### CAD-22 Box select
1. A left drag past 6 px (`cad/pick.rs:431-436`) draws the rubber band
   (`cad/overlay.rs:band`, 377); the release writes `CadBoxSelect { rect,
   extend: Shift || Ctrl }` (446). REST `cad_box_select {rect}`.
2. `cad/selection/mod.rs:handle` (111-114) → `box_select` (555) →
   `box_items` (513): visible nodes (520); body, face and point modes the
   nodes whose 8 bounding-box corners project inside (542-546); edge mode
   the edges whose every sampled point is inside (535-539); vertex mode
   the vertices inside (528-533); edges and vertices stamped with their
   topology's revision; `Op::Add` with extend, else set (564); a node whose
   topology is still loading is named (`not_loaded`, 572-576).
3. RoboCAD `PUT /selection` (selection leg). Its window:
   `ui/tools.py:SelectTool._box_select` (198-232), the same three rules,
   no lock test (locked nodes are taken in both).
4. Shown: the status "n selected", the outlines; the answer's `found`.

Matches RoboCAD. By reading, unexecuted.

### CAD-23 Alt menu
1. Alt+click with more than one candidate (`cad/pick.rs:463-466`):
   `candidates_at` (343) collects the nearest visible hit of nine rays
   (the cursor and 3 px around it, `ring`, 219), nearest ray first, or the
   edges/vertices nearest first; `CadCandidates { items, extend: Shift,
   toggle: Ctrl }`. REST `cad_candidates`.
2. `cad/selection/mod.rs:candidates` (259): the menu stored with the shown
   revision (268-269). Its entries are `cad:candidate:<n>` (`cad/panel.rs:
   249-257`, label "name: face #i", no "#i" for a body), drawn by
   `cad/overlay.rs:menu` (323-374) at the click. A choice writes
   `CadSelect` (picked_at None) → `select` (216) takes the menu's revision
   (`menu_revision`, 277-281) and its Shift/Ctrl rule; a stale menu closes
   with the refusal (226-230).
3. Escape → `cad/transform/input.rs:keys` (76-77) → `CadCancel` →
   `cad/transform/mod.rs:cancel` (438-443) closes the menu first; a press
   elsewhere in the view writes `CadCandidates {items: []}`
   (`pick.rs:419-421`) → closed (`selection/mod.rs:260-265`).
4. RoboCAD: `ui/tools.py:SelectTool.release` (143-147) →
   `ui/app.py:disambiguate` (583) → `ui/widgets.py:disambiguation_menu`
   (888-894), the same label, `_apply` with the click's modifiers; then
   `PUT /selection` (selection leg).

Matches RoboCAD. By reading, unexecuted.

### CAD-24 Select All, Invert, Same Material
1. Ctrl/Cmd+A, Ctrl/Cmd+Shift+I, Ctrl/Cmd+Shift+M (key leg,
   `registry.rs:312-314`), the strip's buttons (`cad/overlay.rs:263`,
   click leg), REST `cad_select_all`, `cad_invert_selection`,
   `cad_select_same_material`.
2. `cad/selection/mod.rs:select_all` (296) and `invert` (305) over
   `visible_bodies` (285: kinds body, sheet, curve, instance, mesh
   (`SELECTABLE_KINDS`, 86), `effective_visible`, walk order);
   `same_material` (315): refused with nothing selected (317) or a first
   node without material (323-325), else every body or instance with that
   material (`MATERIAL_KINDS`, 88; 326-331). Selection leg.
3. RoboCAD `PUT /selection`. Its window: `ui/app.py:select_all`
   (604-610), `invert_selection` (612-619), `select_same_material`
   (621-629) over `document.py:same_material` (448-450).
4. Shown: the status "n selected", the bodies highlighted.

Matches RoboCAD for the sets. Deliberate difference (recorded):
Same Material refuses by name where RoboCAD does nothing or selects every
body without a material. By reading, unexecuted.

### CAD-25 Edges → faces
1. The strip's **Edges → Faces** (`cad:edges_to_faces`, `cad/panel.rs:
   246-247`, ready only with an edge selected), Edit ▸ "Selection: edges →
   bounding faces" (`registry.rs:315`), REST `cad_edges_to_faces`.
2. `cad/selection/mod.rs:edges_to_faces` (344): refused without edges
   (346-348), without a 3D view (349-351), while the node's topology is
   loading or failed, naming the node (357-361), while its mesh is not
   drawn (363-365) or drawn at another revision than its topology
   (366-369), or when an edge was picked at another revision (370-377);
   the faces come from `faces_of_edge` (404) → `faces_along` (464: the
   drawn tessellation's triangle sides along the sampled polyline), stamped
   with the topology's revision, `Op::Set` (390), the mode becomes Face
   (391), status "Selection: n edges → m faces" (394). Selection leg.
3. Topology leg for the polylines; the mesh from `GET /nodes/{id}/mesh`
   (`triangle_face`). RoboCAD's window: `ui/app.py:convert_edges_to_faces`
   (631-645) through its kernel's `faces_of_edge`, which has no REST route.
4. Shown: the faces outlined, Face mode in the strip and tool bar.

Deliberate difference (recorded): the faces come from RoboCAD's drawn
tessellation, not its kernel; refused by name, never guessed, until both
are loaded at one revision. By reading, unexecuted.

### CAD-26 Inspector: sub-body
1. Selecting a face, edge, vertex or point (CAD-20) makes it the first
   item; the right dock's inspector draws `cad/inspector/node.rs:sub_body`
   (109) for it (`sub_item`, 55), keyed by `sub_key` (61-75).
2. No action: the inspector reads `CadTopology` (topology leg) at the
   shown revision.
3. RoboCAD `GET /nodes/<id>/faces`, `/edges`, `/vertices` → `api.py:
   faces`, `edges`, `vertices` (796-817; `face_json`, 147-150;
   `edge_json`, 153-154).
4. Shown: the title "Face 3" / "Edge 7" / "Vertex 2" / "Point on face 4"
   (`node.rs:111-117`), "Of" the node; while loading "fetching…" (134),
   a failed fetch's error verbatim (131); then the note "RoboCAD's
   topology of name at revision r (mm)" (139) and the rows as returned
   (face: `face_rows`, 93-106; edge 143-155; vertex 161-162), "null in
   RoboCAD's answer" for a missing value, "RoboCAD listed no face i …"
   for an index RoboCAD does not have (140, 158, 164, 175).

Deliberate difference: RoboCAD's window has no per-item section (its
Properties panel shows only facts and live dimensions,
`ui/widgets.py:443-509`); the viewer shows the routes' values. Recorded,
not fixed (the inspector is outside this part's files): the "Ø" rows
(`node.rs:100,148`) are 2 × RoboCAD's radius, derived in the viewer; and
an item picked at an older revision (its stamp kept by `follow_tree`,
`cad/selection/shared.rs:190-194`) is shown with the face of that index at
the new revision, not refused. By reading, unexecuted.

### CAD-27 Move
1. G (tool keys, `cad/transform/input.rs:80-81`), the tool strip
   (`cad/numeric.rs:339-344`) or REST `cad_tool {"tool":"move"}` →
   `CadTool` → `cad/transform/mod.rs:activate` (388): the pivot
   (`geometry.rs:pivot`, 34: the node's pivot, else the one node's mass
   centroid at the shown revision, else the drawn bounds' centre); with
   nothing selected "Select something to transform" (412-414).
2. `cad/transform/gizmo.rs:drag` (211; SimSync, `transform/mod.rs:278`):
   a press on a handle (`hit_test`, 80: centre within 10 px, axes within
   14 px) starts a `Drag` at the shown revision when it is RoboCAD's
   (285-289) over the unlocked selected nodes (292-298); each frame
   `drag_point` (124) on the drag plane, `drag_delta` (170: Ctrl 10 mm
   grid steps, `move_delta`, 131; the centre handle on the screen plane)
   and the readout "Δ = (dx, dy, dz)  \|d mm\|" (175); the preview is
   display only (`Preview`, 248; `preview.rs:previews`, 50). Release:
   `drag_action` (192-207) writes one `CadTransform { ids, translation,
   revision: Some(began) }` (258-262). A revision change during the drag
   cancels it, nothing sent (235-239).
3. Edit leg: `commit.rs:commit` (264) → `commit_refusal(Some(began))`
   (270; the revision guard on release) → `op_for` (202) →
   `transform_call` (64) → `POST /ops/transform {"args": [ids],
   "kwargs": {"translation"}}` → `commands.py:transform` (648-692: one
   `Composite("Transform")`).
4. Shown: the body keeps the preview until its new mesh (newer than
   `began`) is drawn (`preview.rs:settle`, 20-45: dropped at once on a
   refused commit, `commit.rs:284-285`, or a failed edit), so it never
   jumps back; the gizmo hides while the commit waits (`gizmo.rs:314-316`).
   Escape during a drag: `CadCancel` → `cancel` (`transform/mod.rs:447-451`)
   → `activate(Select)` → `end_live` (375-384) drops the drag and its
   live preview; nothing is written (the release finds no transform tool,
   `gizmo.rs:224`). RoboCAD's window: `ui/tools.py:TransformTool`
   (234-385; `_apply`, 352-372), `ui/app.py:487-497`.

Matches RoboCAD. Gap found and fixed: a REST `cad_transform` or a typed
move over nodes that are all locked was sent and reported "Move … by …"
though RoboCAD's `Ops.transform` skips locked nodes (`commands.py:
661-662`) and pushes no step; it is now refused by name with nothing sent,
and locked nodes are left out of a partly locked transform so its label
names only what moves (`cad/transform/commit.rs:209-218`). With a
component member or occurrence among the ids nothing is left out
(`component`, 212): RoboCAD's component checks (`commands.py:650-655`)
see every id and give their own refusal. By reading, unexecuted.

### CAD-28 Rotate and scale
1. R or S (`cad/transform/input.rs:82-85`, not with Shift or Ctrl/Cmd),
   REST `cad_transform {"axis","angle_deg"}`, `{"scale"}`.
2. `cad/transform/gizmo.rs:drag` (211): a ring (`ring_point`, 68, 48
   samples) or axis handle; `rotate_angle` (143: Ctrl 15° steps,
   `round_ties_even` as Python's `round`), `scale_factor` (159: uniform,
   Ctrl 0.1 steps, at least 0.01); readouts "angle = …" / "scale = ×…"
   (180, 184); release → `drag_action` (196-204: axis, angle_deg and
   center, or scale and center, rounded to 1e-6).
3. Edit leg → `transform_call` (`commit.rs:86-105`) → `POST
   /ops/transform` → `commands.py:transform` (648: `center` given, so the
   same pivot in both).
4. Shown as CAD-27. RoboCAD's window: `ui/tools.py:311-330, 360-364`; its
   pivot `_place_gizmo` (253-270: the node's pivot, else
   `selection_properties` centroid).

Deliberate difference (recorded): with several nodes selected the pivot is
the centre of their drawn bounds, not RoboCAD's mass-weighted centroid
(`cad/transform/mod.rs:64-72`); a typed angle after dragging the X ring
turns about X (`mod.rs:75-79`). By reading, unexecuted.

### CAD-29 Push/pull and offset
1. D or Shift+D (`cad/transform/input.rs:86-87`) → `activate`
   (`transform/mod.rs:417-425`: Face mode, published; the first selected
   face as target). REST `cad_push_pull {"node","face","distance"}`,
   `cad_offset_faces`.
2. `cad/transform/push_pull.rs:tool` (141): a press on a face (the first
   unlocked ray hit, `geometry.rs:ray_hit`, 118; refused while the mesh
   lags, 215-219) selects it (`CadSelect`, 223, as RoboCAD) and starts a
   `PushDrag` at the shown revision when it is RoboCAD's (234-238); the
   drag `push_distance` (108: Ctrl 10 mm steps), readout "push/pull …"
   (178), the outline shifted along the normal (`draw`, 246-258); a
   revision change cancels it, nothing sent (168-171); release
   `release_action` (122-133): Shift, the offset tool or a non-planar face
   → `CadOffsetFaces`, else `CadPushPull`, with `revision: Some(began)`.
3. Edit leg → `push_pull_call` / `offset_call` (`commit.rs:111-139`) →
   `POST /ops/push_pull` / `offset_faces` → `commands.py:push_pull`,
   `offset_faces` (515-519, `_edit`, 281-291: one `EditBodies`).
4. Shown: the refetched mesh and topology (volume and face count in the
   inspector). RoboCAD's window: `ui/tools.py:PushPullTool` (547-632;
   `_apply`, 618-627: the same offset rule).

Matches RoboCAD. Gap found and fixed: a push/pull, offset or dimension of
a locked node (a face selected by the tree or the box, which take locked
nodes) was sent for RoboCAD to refuse; it is now refused by name with
nothing sent, in RoboCAD's words "… is locked" (`cad/transform/commit.rs:
184-188`, used at 224, 230, 236). By reading, unexecuted.

### CAD-30 Numeric bar
1. Tab (`cad/numeric.rs:entry`, 187, in `CadKeySet::NumericEntry` inside
   `Focus`, `transform/mod.rs:269`; `cad/mod.rs:184`) opens the entry on
   the first field when no field types and no two-step key owns the frame
   (271-278), the field being the one kit text field (`NUMERIC`, 47,
   `TextField::select_on_focus`, 51-53); the shown revision is recorded
   (`began`, 276). Each keystroke (`FieldEvent::Changed`, 208-213) is
   evaluated (`numeric_fields.rs:FieldKind::evaluate`, 24-31, over
   `sim_runtime::units::evaluate`): "= 20.3 mm" (`numeric.rs:375-378`), or
   the evaluator's error naming the token in the danger colour with a red
   border (369-372, 379-381). Tab cycles (236-242). REST `cad_numeric
   {"values"}`.
2. Enter (`Submit`, 214-228): only when every field evaluates, one
   `CadNumeric { values }` (222), the entry closed; else it stays open
   (RoboCAD's `values()`). `cad/transform/commit.rs:numeric` (301):
   `evaluate_fields` (292), the revision the entry gained focus (321;
   REST: the shown one), then one commit per tool (Move, Rotate about
   `numeric_axis`, Scale, Push/Pull with RoboCAD's offset rule).
3. Escape: the kit field's Escape is `Outcome::Cancel`
   (`ui_kit/text/input.rs:93`), which takes the keyboard away and consumes
   the key (228-232, 248), so the later CAD readers (`CadKeySet::Gate` →
   `EscapeTool` → `Escape` → `ToolKeys`, `cad/mod.rs:182`) never see it:
   `numeric.rs:229-234` ends the entry (and its `began`) and nothing else
   happens. A second Escape, the field no longer typing, reaches
   `transform::keys` (ToolKeys) → `CadCancel` (a drag or the tool ends).
   One Escape cancels one thing.
4. Edit leg as CAD-27/29. RoboCAD's window: `ui/widgets.py:NumericBar`
   (142-216: Enter commits only when `values()` evaluates, Escape
   `cancelled`, Tab cycles), Tab routed in `ui/app.py:483-486`, cancel
   `_numeric_cancel` (572-574).

Matches RoboCAD. By reading, unexecuted.

### CAD-31 Live dimensions
1. Select tool, face mode: the numeric bar's fields are
   `numeric_fields.rs:fields` (89) → `dimensions.rs:live` (55, only when
   the selection's indices are of the shown revision, 58): "Ø name"
   (66-69), "R name" read-only with RoboCAD's message (70-74,
   `ROUND_READ_ONLY`, 36), "Distance" for two parallel planar faces of one
   node (the second moves, 82-85), else "Angle" (86-88), "Ø edge i"
   (90-99, the cylinder from `selection::faces_of_edge`). A double-click
   on a face (`double_click`, 142: within 400 ms and 5 px, face mode
   only) selects the face and puts `edit_at` (105) in the bar, focused
   (`focus_request`, 190 → `numeric.rs:sync`, 167-175, `began` 172).
   REST `cad_set_dimension`.
2. Enter → `commit.rs:numeric` (351-363: one changed field per Enter) →
   `CadSetDimension { revision }` → `dimension_call` (142-173).
3. Edit leg → `POST /ops/set_diameter` / `set_distance` / `set_angle` →
   `commands.py:534-552`.
4. Shown: the refetched geometry and the new field values. RoboCAD's
   window: `ui/app.py:live_dimensions` (654-691), `edit_dimension_at`
   (693-713), `ui/tools.py:SelectTool.double` (185-196).

Matches RoboCAD. Deliberate difference (recorded): the double-click is
taken in face mode only (RoboCAD switches to face picks for it in body
mode). By reading, unexecuted.

### CAD-32 Snapping and measure
1. M (`cad/transform/input.rs:88-89`) → `CadTool { Measure }`.
   `cad/measure.rs:tool` (187): every frame over the view the snap
   (`snap::snap_on`, `cad/snap.rs:221`; Alt suppresses, `snap.rs:223`, from `measure.rs:218`) with the
   readout "vertex  (x, y, z)" (`Snap::readout`, 99) or, with a first
   pick, "12.000 mm  (vertex)" (`measure.rs:223-226`). A press picks the item in the
   mode (face by ray cast at the shown revision; edge and vertex by
   `pick::candidates_at`, 256-261) and the point (the snap point, or the
   surface hit when the snap is free or on the plane, 265-268); the second
   press writes `CadMeasure { a, b, keep: Shift }` (274). REST
   `cad_measure`.
2. `cad/transform/commit.rs:measure` (372) → `measure::between` (149:
   RoboCAD's rules and labels "12.000 mm", "R 3.000 mm  (Ø 6.000)",
   "90.00°"); not kept: the status line and the tool bar (383-386). Kept:
   one `add_measurement` call (393-394).
3. Edit leg → `POST /ops/add_measurement` → `commands.py:add_measurement`
   (897-900: one `AddNodes("Measurement")`).
4. Shown: the measure node in the tree after the refetch. RoboCAD's
   window: `ui/tools.py:MeasureTool` (1027-1062), `ui/app.py:
   measure_between` (715-740), `ui/viewport.py:snap` (1369).

Deliberate differences (recorded): no clipboard copy (the value shows in
the status line and the tool bar); the same circular edge twice gives its
radius (RoboCAD's branch at `ui/app.py:730` is unreachable after 726, so
it reports 0 mm). Gaps found and fixed: (a) the edge and vertex picks took
the nearest polyline or vertex within 12 px of any drawn body, locked
ones and ones behind a surface included, though RoboCAD's
`MeasureTool.press` reads its pick pass (`ui/viewport.py:1249-1345`: no
locked nodes, 7 px lines, occluded, clipped by the section); they now use
the select click's search (`cad/measure.rs:256-261` →
`cad/pick.rs:candidates_at`, 343), and the old 12 px helpers are gone;
(b) a kept measurement checked only for an edit in flight and the
connection, so one measured on a shown tree RoboCAD had moved past (or a
stale tree) was recorded with points of gone geometry; it is now refused
by `commit_refusal(Some(shown revision))` (`cad/transform/commit.rs:
387-392`). By reading, unexecuted.

### CAD-33 One undo step per commit
1. Cmd/Ctrl+Z (key leg, `edit.undo`) → `CadUndo`.
2. `cad/actions.rs:531-533` → edit leg (`edit`, refused while another
   edit is in flight) → `cad_client/mod.rs:undo` (`POST /undo`) →
   `api.py:undo` → `commands.py:undo` (221-228: one command popped).
3. Each commit traced above is exactly one Ops call through one
   `start_edit` (`commit.rs:send`, 246-258): a drag release (CAD-27/28/29),
   numeric Enter (CAD-30), dimension Enter (CAD-31), a kept measurement
   (CAD-32); each pushes exactly one command (`Composite("Transform")`,
   `EditBodies`, `AddNodes`; `commands.py:212-219`). A refused commit sends
   nothing (`commit.rs:270-276`).
4. Shown: the History list (`/doc` `history`, refetched by `refresh`); a
   waiting preview is dropped once the bodies are drawn at a newer revision
   (`preview.rs:settle`, 30-39), which an undo also produces, and a live
   drag is cancelled by the revision change (`gizmo.rs:235-239`), so no
   preview outlives an undo. RoboCAD: Edit ▸ Undo (`ui/app.py:291`).

Matches RoboCAD. By reading, unexecuted.

### CAD-34 Refused while an edit is in flight
1. A second release while the first edit is pending: the gizmo cannot
   start a second drag while a preview waits (`gizmo.rs:270,278`, the
   handles hidden, 314-316), so the second commit comes from push/pull
   (whose press does not wait) or REST `cad_transform`.
2. `commit.rs:commit` (264) → `commit_refusal` (`state.rs:138`) →
   `edit_refusal_for` (182) → "another CAD edit is in flight: {label}"
   (193); nothing is sent; a released drag's preview is dropped
   (`commit.rs:284-285`). An edit in RoboCAD's window during a drag: the
   poll moves `health.revision`; the live drag is cancelled with "The
   document changed during the drag (another edit, or RoboCAD's own
   window); nothing was sent: drag again" (`gizmo.rs:235-239`,
   `push_pull.rs:168-171`); a release already written is refused by
   `commit_refusal(Some(began))` with "the document changed since these
   values were taken (revision {began}, now {now}); nothing was sent: redo
   the drag, the entry or the form" (`state.rs:149-151`). The numeric
   entry carries the revision it gained focus at (`numeric.rs:276`,
   `commit.rs:321`).
3. No RoboCAD call for a refused commit; RoboCAD's history holds only the
   edits sent.
4. Shown: the refusal on the status line (`actions.rs:407-409`); the body
   back where it was.

Matches RoboCAD for edits sent in turn (RoboCAD's own window applies one
at a time). The table's refusal text is updated to the code's (since
db272924). Deliberate difference (recorded, needs a RoboCAD change): the
revision guard is the viewer's (`health.revision` as last polled);
`POST /ops/{name}` checks `expected_revision` only for component-job
operations (`api.py:1011-1023`), so an edit in RoboCAD's window between
the viewer's last poll and the POST is not caught. By reading, unexecuted.

## Reading traces — Part D

Each trace follows one Part D step (CAD-35 to CAD-76: the modify tools and
the command surfaces) from the native control to RoboCAD and back to what
the tree, the inspector's History, the status line, the 3D view, the
overlay or the form shows, with RoboCAD's own window code beside it.
Everything here is by reading, unexecuted: nothing was built, run or
captured, and no step was compared side by side. Native paths are under
`crates/sim-spatial/src/` unless they start with `crates/`; RoboCAD paths
are under `cad/robocad/` (`api.py`, `commands.py`, `ui/app.py`,
`ui/tools.py`, `ui/widgets.py`). Gaps found while tracing were fixed in
this batch's working tree (named in the entry), unless an entry says it is
recorded. The common legs are written out once:

- **Surface leg** (how a person reaches a command). Every surface lists
  RoboCAD's command table `cad/surfaces/registry.rs:COMMANDS` (287) and
  writes `CadAction::CadInvoke { id }`: the toolbar chip carries
  `CadButton(CadInvoke)` (`cad/surfaces/toolbar.rs:refresh`, 159-196),
  written by `cad/panel/name.rs:buttons` (71) as a guarded `Act::ui`; a
  menu, context-menu row is a `SurfaceEntry` whose click
  `cad/surfaces/mod.rs:input` (364-376) writes the action and then
  `CadSurface { closed }`, both stamped by `cad/activation.rs:guard` (98;
  refused at apply when the document was replaced, `cad/actions.rs:474-476`);
  a disabled row's press shows its refusal (366-370). The palette's Enter
  is `cad/surfaces/palette.rs:input` (167-184); the radials'
  press/release `cad/surfaces/radial.rs:input` (56-101); a key
  `cad/keys.rs:keys` (352: exact key and modifier match against the
  table's keymap keys, two-step "Shift+A, B" chords 379-392, standing
  aside while a kit text field types (366), while a surface is open or a
  modal form is open (374); ready → `CadInvoke`, or `CadSurface` for the
  palette and radials at the pointer (404-409); not ready → the status
  line `registry::status_line` (413)). Readiness is
  `registry.rs:ready` (573: a catalogue op first needs
  `CadDocument::edit_refusal`, then `readiness` 540, the op's `Needs`; a
  pick, place, sketch or plane tool is always ready). REST sends the same
  `cad_invoke {id}` / `cad_run {id, params, items, revision}` /
  `cad_surface {surface}`, and `system_ui` lists `cad:op:<id>`
  (`cad/surfaces/mod.rs:controls`, 256).
- **Invoke leg**: `cad/actions.rs:apply` (354) → `handle` (459; arm 561)
  → `cad/ops/mod.rs:handle` (491) → `cad/ops/invoke.rs:invoke` (12) by the
  entry's `Flow`: `Immediate`/`AtCursorSnap` run at once (15);
  `Form` checks the selection first (`resolve::resolve`, 62) then opens the
  modal form (77); `PickThenForm` sets the selection mode and keeps the
  selection (84-87), opens the form beside the view (89) with the hint on
  the status line (90); `Place` makes the placement active with its form
  (93-99). A command that is not in the catalogue goes to
  `cad/surfaces/registry.rs:invoke` (619; via `cad/surfaces/mod.rs:invoke_command`, 246).
  A refusal from a click is written to the status line at
  `cad/actions.rs:408`.
- **Run leg** (every catalogue run): `cad/ops/mod.rs:run` (528) →
  `prepare` (543): `CadDocument::commit_refusal(revision)`
  (`cad/document/state.rs:138`: an edit in flight, not connected, the shown
  document stale, RoboCAD's revision not the given one; 549); explicit
  REST items naming faces or edges without `revision` are refused "pass
  revision: …" (554-560); `resolve::resolve` (569; `cad/ops/resolve.rs:51`:
  nodes in the shown tree, a selection first seen at an older revision
  refused "reselect" (74-79), indices checked against the topology
  (81-96), the entry's `Needs` else its refusal (98-143)); `values` (366;
  unknown or gated-off parameters refused by name); `args::build`
  (`cad/ops/args.rs:562`) → `start` (584) → `actions::edit` (589) →
  `cad/edit.rs:edit` (12) → `cad/sync/mod.rs:start_edit` (686): one
  `Job::spawn(Pool::Dedicated, …, "RoboCAD edit: …")` (693) runs each call
  `c.op` (`cad/ops/mod.rs:592`) →
  `crates/sim-runtime/src/cad_client/mod.rs:op` (374, `POST /ops/{name}`)
  → RoboCAD `api.py:_route` (1540; ops 1668-1671) → `Service.op` (1011:
  `ArgConverter.convert`, 173-287, turns `{node, edge}`, `{node, face}`,
  plane names into kernel refs; the `Ops` method pushes one command on
  RoboCAD's stack; `_refresh`) → the answer lands in
  `cad/sync/mod.rs:finish_edit` (565; status line 601; the selection is
  cleared where RoboCAD's handler clears it, noted by
  `cad/ops/mod.rs:started` 663-667 and applied at 615 only on success) →
  `refresh` (641, called at 630) refetches `/doc`, so the tree, inspector
  and History redraw. A `Fan::PerNode` entry sends one call per node in
  RoboCAD's order inside that one job, each its own RoboCAD undo step,
  the first error naming how many ran (`cad/ops/mod.rs:590-601`). The
  edit's label (and status message) is RoboCAD's history label
  (`cad/ops/args.rs:history`, 53) with the subject and parameters
  (`plain`, 348-434).
- **Form leg** (`Flow::Form` dialogs and the tools' numeric fields): 
  `cad/ops/form.rs:open_form_with` (75; RoboCAD's defaults, the first
  number field focused for a modal form 89-92, `began` = the shown
  revision 93) → drawn by `cad/surfaces/form.rs:draw` (367; modal and
  dimmed when no interaction is active 402-404, else beside the view) on
  the kit form (`ui_kit/form.rs:Kit::form`, 213), whose fields are the one
  kit text field (`ui_kit/form.rs:284`, 291 → `ui_kit/widgets.rs:input_selectable`,
  244, tagged `KitInput` 259; the field `FORM` registered at
  `cad/surfaces/mod.rs:321`, `ui_kit/text/mod.rs` "One text entry") →
  `cad/surfaces/form.rs:input` (167; Enter in the field → `CadFormSubmit`
  241, Escape → `CadFormCancel` 245, Tab cycles 249, Tab with no field
  focused takes the first 314, Enter with none focused submits 322) →
  `cad/ops/form.rs:submit` (15): a modal form runs with
  `revision = Some(began)` (41), so a form whose document moved is refused
  by name with nothing sent; a pick tool's form passes none and its picks
  carry the selection's revision (resolve); a refusal stays in the form
  (52-56).
- **Escape leg**: a kit field that types consumes its own Escape (its
  owner gets `FieldEvent::Cancel`: the palette closes, the form
  cancels). Otherwise `cad/surfaces/mod.rs:input` (396-421, before the key
  gate in the surfaces' chain, 327-332): it stands aside while a file
  form, a results path form or the outliner menu is open (396); an open
  surface closes, else an open form or active interaction is cancelled
  (`CadFormCancel` → `cad/ops/form.rs:form_cancel`, 128: "Cancelled
  {label}"), and the key is consumed when it acted (415), a pending
  two-step key dropped (`cad/keys.rs:Chord::abandon`, 283). After the gate,
  calibrate's (`CadKeySet::EscapeTool`), the threads' (`CadKeySet::Escape`)
  and the Select tool's Escape (`cad/transform/input.rs:keys`, 67, which
  also skips while a form, surface or interaction exists, 70) see only a
  press nobody before them used. So a form opened over an active tool
  (a `Flow::Form` dialog leaves a transform tool active, as RoboCAD's
  modal dialog leaves its tool) ends first, the tool on the next press.
  **Gap found and fixed (whole leg)**: `surfaces::input` did not consume
  the Escape it acted on, and stood aside only for the file form. The
  threads' Escape checks `ops.form` but not a form-less `ops.active` (a
  plane tool, the joint tool), so one press could cancel the op and
  Annotate together; and `results::forms::input` and `tree::popup::input`
  run in the same pre-gate band unordered against it, so with a results
  form or the outliner menu open one press could also cancel the op. Now
  `cad/surfaces/mod.rs:396` stands aside for those, and 415-419 consume
  the key and drop the chord when it acts (Bevy 0.19.1
  `ButtonInput::clear_just_pressed`, bevy_input `button_input.rs:200`).
- **Read leg** (copy and the analysis overlays): `Built::Read` →
  `cad/ops/mod.rs:start` (614-621) → `cad/analysis_overlay.rs:start`
  (142: not connected, a read pending, a stale document or a revision
  other than the picks' refused by name; one `Job::spawn(Pool::Dedicated,
  …)` 162) → `crates/sim-runtime/src/cad_client/mod.rs` reads (344-369) →
  `analysis_overlay.rs:receive` (254; an answer for another generation is
  dropped 275, a result read at a revision no longer shown is dropped with
  a status 291-293; an overlay of an older revision is cleared 256-261).

### CAD-35 Box (corner)
1. Toolbar **Box** (`cad/surfaces/registry.rs:TOOLBAR`, 258; row 356
   "Shift+A, B"), Create ▸ Box (corner), or Shift+A then B (the chord,
   `cad/keys.rs:379-392`); REST `cad_run {"id":"tool.box", …}`.
2. `CadInvoke {tool.box}` → invoke leg, `Flow::Place(BoxCorner)`
   (`cad/ops/catalogue/edit_create.rs:46-57`) → `cad/ops/invoke.rs:93-99`:
   the active op, its form (width 20, depth 20, height 10, corner;
   `cad/ops/kinds.rs:75-78`), the hint `HINT_CORNER` (`kinds.rs:72`).
3. `cad/ops/interact.rs:pointer` (305; SimSync): the press snaps on the
   active plane or XY (408-418, Alt suppresses), the drag moves the second
   corner (420-426), the release starts stage 2 (430-432), the height
   follows `height_at` (439-442, Ctrl snaps to 10 mm half-to-even), the
   next press finishes (436-438) with `CadRun {params:
   finish_params, revision: Some(press revision)}` (455; `finish_params`
   182-216: the lower corner, absolute sizes, signed height). The preview
   (`draw`, 478) is overlay lines in (0.4, 0.9, 1.0) and the readout
   "20 mm × 20 mm × 10 mm" (`readout`, 219-232). Tab during the drag writes
   the press point into the form's `corner` (445-453) and the form's Tab
   takes width (`cad/surfaces/form.rs:314`); Enter → submit → run.
4. Run leg → `args.rs:place` (499): on XY one `POST /ops/box {"args":
   [corner, size]}` (514-522; label "Box 30 mm × 15 mm × 5 mm at (…)"),
   else `box_three_point` (523-531) → `client.op`.
5. `api.py:1011` → `commands.py:box` (419-420: `_new("Box", …)`).
6. The new "Box" node in the tree; History "Box". RoboCAD's window:
   `ui/tools.py:PrimitiveTool` (391-536: the same stages, readout, colour;
   `_make_box` 517-520 extrudes a sketch rectangle, so its History reads
   "Extrude" via `commands.py:487`, the node named "Box"). Snapping to a
   vertex: `snap::snap_on` here, `ctx.snap` there.

Deliberate difference (recorded in cad-parity.md): the undo label is
"Box" (one `Ops.box`), RoboCAD's "Extrude". Gap found and fixed: the doc
comment on `args.rs:place` said `_make_box`'s extrude is labelled "Box";
it now says RoboCAD records "Extrude" (`cad/ops/args.rs:495-498`). Note:
the table's REST example passes no `revision`; that is accepted here (a
placement names no geometry indices). By reading, unexecuted.

### CAD-36 Box (centre)
1. Create ▸ Box (centre) (`registry.rs:357`, no key, as RoboCAD), or the
   palette row "Create: Box (centre)".
2. `CadInvoke {tool.box_center}` → `Flow::Place(BoxCentre)`
   (`edit_create.rs:59-69`) → `invoke.rs:93-99`.
3. `interact.rs:pointer` as CAD-35; `base_rect` (153-160) draws twice each
   half-size about the press; `finish_params` (198-204) sends `center` and
   2× sizes.
4. `args.rs:place` (514: `x0 = u − w/2, y0 = v − d/2`; 518: `z0 = 0` for
   h ≥ 0) → `POST /ops/box`.
5. `commands.py:box` (419).
6. A box centred on the press in the plane, its base on the plane.
   RoboCAD: `PrimitiveTool(center_mode=True)._finish` (`ui/tools.py:491-500`)
   and `_make_box`: the same footprint, base on the plane.

Deliberate difference (recorded): `Ops.box` (label "Box") where RoboCAD
extrudes (label "Extrude"). By reading, unexecuted.

### CAD-37 Cylinder
1. Toolbar **Cylinder** (`registry.rs:358`, "Shift+A, C") or Shift+A, C.
2. `Flow::Place(Cylinder)` (`edit_create.rs:71-82`; form diameter 10,
   height 10, base).
3. `interact.rs:pointer`: the radius drag, then the height (a downward
   drag gives a negative height, `height_at` 272-284); readout
   "Ø 10 mm × 10 mm" (226-230); `finish_params` (205-209).
4. `args.rs:place` (533-542): base on the plane, axis the normal or its
   reverse by the height's sign (538), radius ≥ 1e-3 → `POST /ops/cylinder`.
5. `commands.py:cylinder` (437-438, label "Cylinder").
6. A "Cylinder" node; RoboCAD `_finish` (`ui/tools.py:501-505`): the same
   base, axis flip and radius floor.

Matches RoboCAD for the drag. Recorded difference (KNOWN): for Tab values
RoboCAD's `commit` (`ui/tools.py:532-534`) sends the typed height signed
along the plane normal with no radius floor, where the viewer applies the
drag's rule (`args.rs:538`, |h| along ± the normal) to both. By reading,
unexecuted.

### CAD-38 Sphere
1. Toolbar **Sphere** (`registry.rs:359`, "Shift+A, S") or Shift+A, S.
2. `Flow::Place(Sphere)` (`edit_create.rs:84-95`; diameter 10, centre).
3. `interact.rs:pointer`: the press is the centre, the release finishes
   at once (427-429); `finish_params` sends the unprojected `p0` and the
   diameter (210-213).
4. `args.rs:place` (543-548) → `POST /ops/sphere {center, radius}`.
5. `commands.py:sphere` (440-441).
6. A "Sphere" node. RoboCAD: `release` finishes a sphere (`ui/tools.py:435-442`).
   S as the chord's second key: `keys::gate` marks the frame the chord's
   (`cad/keys.rs:293-312`), and the Select tool's keys run under
   `keys::free` (`cad/transform/mod.rs:271`), so S completes the sphere and
   does not pick Scale.

Matches RoboCAD. By reading, unexecuted.

### CAD-39 Fillet
1. Ctrl/Cmd+F (`registry.rs:366`; keymap.json `tool.fillet`), toolbar
   **Fillet**; REST `cad_invoke {tool.fillet}`, `cad_run`.
2. `Flow::PickThenForm(Edge)` (`cad/ops/catalogue/modify.rs:10-25`) →
   `invoke.rs:79-92`: mode Edge (published to the one Selection), the form
   "radius 1.0" at the view's top right, the hint "fillet: select edges
   (click adds) then type the size • Enter applies" on the status line.
3. A click on an edge toggles it: `cad/pick.rs:450-459` (`CadSelect
   {toggle: true, picked_at: shown revision}`); Tab, `2`, Enter → form
   leg → `submit` with no revision (the picks carry theirs) → run leg:
   `resolve` refuses picks made at an older revision; `groups` one call per
   owning node (`args.rs:139-148`) → `POST /ops/fillet {"args": [node,
   [{node, edge}…], 2.0]}` per body.
4. `client.op` (`crates/sim-runtime/src/cad_client/mod.rs:374`).
5. `commands.py:fillet` (630-631, `_edit("Fillet", …)`) per node; edges
   resolved by `ArgConverter.edge` (`api.py:193-203`).
6. One "Fillet" History step per body; the selection clears on success
   (`started` 663-667 → `finish_edit` 615); the form stays and the tool
   stays active (`started` 680-686). No edge: `resolve` refuses with
   `NO_EDGES` "Select one or more edges first" (`kinds.rs:80`), shown in
   the form and on the status line. RoboCAD: `EdgeTool.commit`
   (`ui/tools.py:967-989`), the same message and clear.

Matches RoboCAD. Note: the table's REST example now passes `revision`; without it a run
is refused by name, "pass revision: the RoboCAD revision
the face and edge indices in items were read at", with nothing sent
(`cad/ops/mod.rs:554-560`). By
reading, unexecuted.

### CAD-40 Variable and chordal fillet
1. Modify ▸ Variable fillet / Chordal fillet (`registry.rs:367`, 368).
2. `modify.rs:26-40` (start radius 1.0, end radius 2.0, both sent:
   `Arg::Param("radius"), Arg::Param("radius_end")`) and 41-55 (chord 1.0,
   route `fillet_chordal`), both `PickThenForm(Edge)`, `Fan::PerNode`.
3. As CAD-39.
4. `client.op` per node.
5. `commands.py:fillet` (630, `radius_end`) and `fillet_chordal` (633).
6. One step per body ("Fillet" / "Chordal fillet"). RoboCAD `EdgeTool`
   kinds "variable" and "chordal" (`ui/tools.py:948`, 980-983).

Matches RoboCAD. By reading, unexecuted.

### CAD-41 Chamfer
1. Ctrl/Cmd+Shift+F (`registry.rs:372`).
2. `modify.rs:90-106`: `Shape::Chamfer`, distance 1.0, angle 45.0.
3. As CAD-39; `args.rs:plain` builds the spec (354-366): the angle only
   when it is not 45° (360); the label adds "distance 1.5 mm" and the angle
   only then (368-373).
4. `POST /ops/chamfer {"args": [node, edges, {"distance"[, "angle_deg"]}]}`.
5. `commands.py:chamfer` (645).
6. "Chamfer …: distance 1.5 mm" in `cad_state` (the edit label), the angle
   added at 30°. RoboCAD `EdgeTool.commit` (`ui/tools.py:984-985`):
   `ChamferSpec(d, angle_deg=None at 45°)`.

Matches RoboCAD. By reading, unexecuted.

### CAD-42 Fillet all
1. Modify ▸ Fillet all edges… (`registry.rs:369`).
2. `modify.rs:56-68` `Flow::Form`: "Radius (mm):" 1.0, 0.01–100, 3
   decimals → `invoke.rs:59-78`: the selection checked first (62), so with
   nothing selected the menu row is disabled with "Select the bodies to
   fillet" (`registry.rs:readiness`, 549-565) and a key or REST call is
   refused with it.
3. Form leg (modal, the field focused) → OK → run with `Some(began)`.
4. `POST /ops/fillet_all` per node.
5. `commands.py:fillet_all` (636-637, "Fillet all").
6. One "Fillet all" per body; the form closes (`started` 681-682).
   RoboCAD `fillet_all` (`ui/app.py:832-837`) opens its dialog even with
   nothing selected and then does nothing.

Deliberate difference (recorded): the empty selection is refused before
the form. By reading, unexecuted.

### CAD-43 Full round
1. Modify ▸ Full round (two edges) (`registry.rs:370`).
2. `modify.rs:69-78`: `Needs::Edges {2, 2, same_node}`, `Flow::Immediate`.
3. Run leg; `resolve.rs:114-119` refuses one edge or edges of two bodies
   with "Select two edges of the same body".
4. `POST /ops/full_round {"args": [node, {node, edge}, {node, edge}]}`
   (`Arg::EdgeA`, `EdgeB`, `args.rs:308-309`).
5. `commands.py:full_round` (639).
6. One "Full round" step. RoboCAD `full_round` (`ui/app.py:839-846`):
   the same check and message.

Matches RoboCAD. By reading, unexecuted.

### CAD-44 Remove fillets
1. Face mode, Modify ▸ Remove fillets (selected faces) (`registry.rs:371`).
2. `modify.rs:79-89`: `FACES`, `Fan::PerNode`.
3. Run leg; faces grouped by owning node (`args.rs:owners`, 111, `groups` 144).
4. `POST /ops/remove_fillets {"args": [node, [{node, face}…]]}` per body.
5. `commands.py:remove_fillets` (642).
6. One "Remove fillets" step per body. No face: refused "Select the
   fillet faces to remove". RoboCAD `remove_fillets` (`ui/app.py:848-855`)
   is silent.

Deliberate difference (recorded): the refusal is ours. By reading,
unexecuted.

### CAD-45 Shell
1. Ctrl/Cmd+Shift+H (`registry.rs:373`), toolbar **Shell**.
2. `modify.rs:107-122`: `PickThenForm(Face)`, wall 2.0,
   `Needs::NodesWithFaces` → `invoke.rs:79-92` (mode Face).
3. A face click toggles the face (`cad/pick.rs:450-459`); Enter → run leg;
   one call per selected node with its faces (`args.rs:groups`, 145).
4. `POST /ops/shell {"args": [node, 2.0, [{node, face}…]]}` (no faces: a
   closed shell).
5. `commands.py:shell` (624).
6. "Shell" per body; the selection clears. RoboCAD `ShellTool`
   (`ui/tools.py:992-1017`): `press` reuses `EdgeTool.press`, which toggles
   only edges, so a face click there selects nothing.

Deliberate difference (recorded): the viewer toggles faces. By reading,
unexecuted.

### CAD-46 Thicken
1. Modify ▸ Thicken sheet… (`registry.rs:374`).
2. `modify.rs:123-135`: `nodes(1, None, ["sheet"])` (bodies ignored,
   `resolve.rs:102-104`), `Flow::Form` "Thickness (mm):" 2.0.
3. `invoke.rs:62` refuses "Select a sheet" before the form; form leg.
4. `POST /ops/thicken` per sheet.
5. `commands.py:thicken` (627).
6. A solid per sheet ("Thicken"). RoboCAD `thicken` (`ui/app.py:857-864`):
   the same filter, message and dialog.

Matches RoboCAD. By reading, unexecuted.

### CAD-47 Draft
1. Face mode, Modify ▸ Draft faces… (`registry.rs:375`).
2. `modify.rs:136-148`: "Angle (degrees):" 2.0 (−45 to 45, 2 decimals),
   `neutral` ("active", xy, xz, yz; `kinds.rs:17`); pull `[0, 0, 1]`.
3. Form leg; `Arg::Plane("neutral", Xy)` → `args.rs:plane_arg` (263-269):
   the active plane, else XY.
4. `POST /ops/draft {"args": [node, faces, [0,0,1], 2.0, "xy"]}` per node.
5. `commands.py:draft` (553); `ArgConverter.plane` (`api.py:205`).
6. "Draft" per body. RoboCAD `draft_faces` (`ui/app.py:866-878`): pull
   (0, 0, 1), `active_plane or Plane.xy()`.

Matches RoboCAD (the plane names are a native addition, recorded). By
reading, unexecuted.

### CAD-48 Delete faces
1. Face mode, Modify ▸ Delete faces (heal) (`registry.rs:376`).
2. `modify.rs:149-160`: `FACES`, `PerNode`, `clears_selection`.
3. Run leg.
4. `POST /ops/delete_faces` per body.
5. `commands.py:delete_faces` (556-557, "Delete face").
6. One step per body; the selection clears on success. RoboCAD
   `delete_faces` (`ui/app.py:880-887`) clears it too (silent on none;
   the viewer refuses "Select the faces to delete", recorded).

Matches RoboCAD. By reading, unexecuted.

### CAD-49 Mirror and live mirror
1. Ctrl/Cmd+M (`registry.rs:388`), Modify ▸ Mirror as live instance (389);
   REST `cad_run {"id":"tool.mirror","params":{"plane":"xz"}}`.
2. `cad/ops/catalogue/arrange.rs:10-34`: `Arg::Plane("plane", Yz)`,
   `live` false / true; `Flow::Immediate`.
3. Run leg; `plane_arg` sends the active plane, else "yz", or the named one.
4. One `POST /ops/mirror {"args": [ids, plane], "kwargs": {"live": …}}`.
5. `commands.py:mirror` (694).
6. Mirrored copies about YZ; the live one is an instance that follows its
   source. RoboCAD `mirror` (`ui/app.py:910-914`): `active_plane or
   Plane.yz()`, "Select bodies to mirror".

Matches RoboCAD. By reading, unexecuted.

### CAD-50 Array
1. Ctrl/Cmd+Shift+A (`registry.rs:393`).
2. `arrange.rs:46-70`: `Flow::Form`, `Shape::Array`; Kind, Count X/Y/Z,
   Mode, Spacing or extent X / Y / Z (rectangular), Radial count, total
   angle and axis plane (radial), As live instances, Merge.
3. `invoke.rs:62` ("Select bodies to array") → form leg; the rows whose
   `when` holds are shown (`cad/surfaces/form.rs:shown`, 78) → submit →
   `args.rs:array` (441-477): `array_rect` with `spacing` or `extent` by the
   mode (466), or `array_radial` about the plane's normal through its
   origin (450-460).
4. One `POST /ops/array_rect` / `array_radial`.
5. `commands.py:array_rect` (728), `array_radial` (738).
6. The copies; History "Rectangular array" / "Radial array". RoboCAD's
   `array` (`ui/app.py:920-941`) and `ArrayDialog` (`ui/widgets.py:1024-1053`).

Deliberate difference (recorded below in KNOWN): RoboCAD's dialog shows
every row at once (Count X / Y / Z on one row, the radial rows always
visible) and takes the radial axis from the active plane only; the
viewer shows only the chosen kind's rows (the table's "the rows switch
with the kind in both" holds only here) and adds an axis-plane choice.
By reading, unexecuted.

### CAD-51 Instance
1. Modify ▸ Instance selected (`registry.rs:392`).
2. `arrange.rs:35-45`: `Arg::Const({"translation": [20, 0, 0]})`, `PerNode`.
3. Run leg; nothing selected → "Select the bodies to instance".
4. `POST /ops/instance` per node.
5. `commands.py:instance` (712); `ArgConverter` makes the `Transform`.
6. One instance per body, +20 mm X, one step each. RoboCAD
   `instance_selection` (`ui/app.py:916-918`) is silent on none.

Deliberate difference (recorded): the refusal is ours. By reading,
unexecuted.

### CAD-52 Make unique
1. Modify ▸ Make instance unique (`registry.rs:431`), or the 3D view's
   right-click ▸ Make unique (bake instance) (`registry.rs:MAKE_UNIQUE`,
   278, added while an instance is selected:
   `cad/surfaces/context_menu.rs:instance_selected`, 41;
   `cad/surfaces/mod.rs:192-195`).
2. `cad/ops/catalogue/boolean.rs:88-98`: `nodes(1, None, ["instance"])`.
3. Run leg; no instance → "Select an instance to make unique".
4. `POST /ops/make_unique` per instance.
5. `commands.py:make_unique` (719).
6. The instance becomes a body ("Make unique"). RoboCAD
   (`ui/app.py:389`) skips non-instances silently; its entry is in the
   outliner's menu (`ui/widgets.py:431`).

Deliberate difference (recorded). By reading, unexecuted.

### CAD-53 Set pivot at cursor snap
1. The palette row "Tools: Set pivot at cursor snap", Help menu
   (`registry.rs:404`, category Tools → Help, `menu_of` 252); REST
   `cad_run {"id":"tool.set_pivot","params":{"point":[0,0,10]}}`.
2. `arrange.rs:187-199`: `Flow::AtCursorSnap` → `invoke.rs:15` runs at once.
3. `cad/ops/interact.rs:pointer` keeps `ops.cursor_snap` with its shown
   revision (343-370; cleared off-window or when the revision moves);
   `resolve.rs:56` uses it only at the shown revision; `Arg::CursorSnap`
   (`args.rs:338-342`): the `point` parameter, else the snap, else refused
   by name.
4. `POST /ops/set_pivot {"args": [first node, point]}`.
5. `commands.py:set_pivot` (351-352, "Pivot").
6. The inspector's pivot reads the point. RoboCAD `set_pivot`
   (`ui/app.py:1015-1020`): `viewport.snap` at the cursor.

Deliberate difference (recorded): over a face with no snap point the
viewer's snap takes the grid or plane point as RoboCAD's does
(`interact.rs:48-54`), but `snap_on` falls back to the first surface under
the pointer where RoboCAD has none (cad-parity.md row). By reading,
unexecuted.

### CAD-54 Inspector pivot and transform
1. The inspector's pivot field and **Clear pivot**
   (`cad/inspector/editors.rs:368`), the instance's translation, axis,
   angle and scale fields; the one kit text field (`editors.rs:9-19`).
2. Enter → `editors.rs:enter` (395-408) → `patch_for` (296-302: a vector
   of unit expressions, an error naming the component and token keeps the
   field open) → `CadAction::CadPatch` (`cad/actions.rs:506-521`); a
   transform draft is refused once RoboCAD's revision moved since it
   opened (`commit_refusal(Some(began))`, 402).
3. `actions::edit` → `start_edit` (Dedicated job).
4. `crates/sim-runtime/src/cad_client/mod.rs:patch` (294, `PATCH /nodes/{id}`).
5. `api.py:1631` → `Service.patch` (748-775: a component member's pivot
   and an occurrence's transform refused 750-753; `set_pivot`, 768).
6. One undo step each with RoboCAD's result; the refusal text is shown
   instead of the field (`editors.rs:refusal`, 185). RoboCAD's properties
   panel writes the same `PATCH`.

Matches RoboCAD. Stale citations fixed (this batch): `cad/inspector/editors.rs:2`
and 25 now cite `api.py:748-787` (`Service.patch`) and `750-753` (its
component refusals). By reading, unexecuted.

### CAD-55 Delete as one step
1. Delete or Backspace (`registry.rs:309`; silent on an empty selection,
   `cad/keys.rs:412`), the **Delete** button (`cad/panel.rs:216`,
   `CadInvoke {edit.delete}`), Edit ▸ Delete.
2. `edit_create.rs:8-20`: `Arg::Nodes`, `Fan::Once`, `clears_selection`.
3. Run leg: one job, one call.
4. `POST /ops/delete {"args": [[ids]]}`.
5. `commands.py:delete` (312, `RemoveNodes("Delete")` 331).
6. All three go in one "Delete" step; Undo (`CadUndo`,
   `cad/actions.rs:531`) brings them back. RoboCAD `delete_selection`
   (`ui/app.py:1458-1463`).

Matches RoboCAD. By reading, unexecuted.

### CAD-56 Union, subtract, intersect
1. Ctrl/Cmd+U, Ctrl/Cmd+Shift+U, Ctrl/Cmd+Alt+U (`registry.rs:424-426`),
   toolbar **Union**, **Subtract**.
2. `boolean.rs:8-43`: `Needs::TargetThenTools`, `Arg::Target, Tools,
   Const("union"|…)`, `clears_selection`.
3. With one body the key is not ready: the status line reads "Union:
   Select the target body first, then the tools"
   (`registry.rs:status_line`, 591; `cad/keys.rs:413`). Else run leg; the
   history label is the op capitalised (`args.rs:387-393`).
4. `POST /ops/boolean {"args": [target, [tools], "union"]}`.
5. `commands.py:boolean` (573-585).
6. The result on the first body; tools removed; selection cleared on
   success. RoboCAD `boolean` (`ui/app.py:787-793`): the same message;
   it clears the selection after the call.

Matches RoboCAD. By reading, unexecuted.

### CAD-57 Region
1. Modify ▸ Region (overlap as new body) (`registry.rs:427`).
2. `boolean.rs:44-53`: `nodes(2, Some(2))`.
3. Run leg; three → "Select exactly two bodies".
4. `POST /ops/region {"args": [a, b]}`.
5. `commands.py:region` (588-589).
6. A new "Region" node. RoboCAD `region` (`ui/app.py:795-799`).

Matches RoboCAD. By reading, unexecuted.

### CAD-58 Join and unjoin
1. J, Shift+J (`registry.rs:428-429`).
2. `boolean.rs:54-76`: join `nodes(2, None)` once; unjoin `PerNode`.
3. Run leg; J with one body is not ready ("Select two or more bodies to
   join").
4. `POST /ops/join {"args": [[ids]]}`; `POST /ops/unjoin` per node.
5. `commands.py:join` (806), `unjoin` (814).
6. The same joined body and parts. RoboCAD (`ui/app.py:386-387`) calls
   join with any selection.

Deliberate difference (recorded). By reading, unexecuted.

### CAD-59 Dissolve
1. Modify ▸ Dissolve redundant topology (`registry.rs:430`).
2. `boolean.rs:77-87`: `PerNode`.
3. Run leg.
4. `POST /ops/dissolve` per node.
5. `commands.py:dissolve` (823).
6. One "Dissolve" step per body. RoboCAD `ui/app.py:388`.

Matches RoboCAD (an empty selection refused by name, recorded). By
reading, unexecuted.

### CAD-60 Cut
1. Modify ▸ Cut with active plane (`registry.rs:394`), Cut with selected
   sheet/curve (395); REST `cad_run {"id":"tool.cut_plane","params":{"plane":"yz"}}`.
2. `arrange.rs:72-93`: `Arg::Plane("plane", Xy)` per node; `Target, Second`.
3. Run leg.
4. `POST /ops/cut {"args": [node, "xy"]}` per node; `{"args": [body, cutter]}`.
5. `commands.py:cut` (591); `ArgConverter` turns a cutter id or plane
   name into the kernel argument (the `ArgConverter` fix the table names).
6. The pieces. RoboCAD `cut_with_plane` / `cut_with_selection`
   (`ui/app.py:943-951`).

Matches RoboCAD. By reading, unexecuted.

### CAD-61 Split faces
1. Modify ▸ Split faces with active plane (`registry.rs:396`).
2. `arrange.rs:94-105`: plane fallback XY, `PerNode`.
3. Run leg.
4. `POST /ops/split_face` per node.
5. `commands.py:split_face` (566).
6. The faces split along XY. RoboCAD `split_face` (`ui/app.py:953-955`).

Matches RoboCAD. By reading, unexecuted.

### CAD-62 Imprint
1. Modify ▸ Imprint selected curve/body (`registry.rs:397`).
2. `arrange.rs:106-115`: `nodes(2, None)`.
3. Run leg; one node → "Select the body, then the tool".
4. `POST /ops/imprint {"args": [body, tool]}`.
5. `commands.py:imprint` (562).
6. Imprinted edges. RoboCAD `imprint` (`ui/app.py:957-961`): the same
   message.

Matches RoboCAD. By reading, unexecuted.

### CAD-63 Project curve
1. Modify ▸ Project curve onto body (`registry.rs:398`).
2. `arrange.rs:116-126`: `Arg::ViewDir`.
3. Run leg; `resolve.rs:56`: `view_dir = −view_back(view)` in RoboCAD's
   frame; `args.rs:333-337`: REST `direction`, else the view's, else
   refused by name.
4. `POST /ops/project_curve {"args": [curve, body, dir]}`.
5. `commands.py:project_curve` (856).
6. The projected curve; the direction is in the edit label. RoboCAD
   `project_curve` (`ui/app.py:963-968`): `-camera.basis()[2]`.

Matches RoboCAD. By reading, unexecuted.

### CAD-64 Silhouette
1. Modify ▸ Silhouette onto active plane (`registry.rs:399`).
2. `arrange.rs:127-138`: plane fallback XY, `PerNode`.
3. Run leg.
4. `POST /ops/silhouette` per node.
5. `commands.py:silhouette` (860).
6. The silhouette curve on XY. RoboCAD `silhouette` (`ui/app.py:970-972`).

Matches RoboCAD. By reading, unexecuted.

### CAD-65 Control points
1. Face mode, Advanced ▸ Show/edit control points (advanced) (`registry.rs:400`).
2. `arrange.rs:140-150`: `Shape::ControlPoints` → `args.rs:584-587`
   (`Read::ControlPoints {node, face}`).
3. Read leg (`cad/analysis_overlay.rs:start`, 142).
4. `crates/sim-runtime/src/cad_client/mod.rs:control_points` (356, `GET
   /nodes/{id}/control_points?face=i`).
5. `api.py:1656` → `Service.control_points` (895).
6. Points and rows in RoboCAD's pink; the status "N control points (edit
   via Ops.set_control_points; proportional falloff in scripting)"
   (`analysis_overlay.rs:235`); nothing written; cleared when the shown
   revision changes (256-261). RoboCAD `control_points`
   (`ui/app.py:974-984`), `temp_shapes` cleared on refresh.

Matches RoboCAD. By reading, unexecuted.

### CAD-66 Raise degree
1. Advanced ▸ Raise face degree (`registry.rs:401`).
2. `arrange.rs:151-161`: `Arg::Face, Const(4), Const(4)`.
3. Run leg.
4. `POST /ops/raise_degree`.
5. `commands.py:raise_degree` (867-868, "Raise degree").
6. One step. RoboCAD `raise_degree` (`ui/app.py:986-991`; silent without
   a face, refused by name here).

Matches RoboCAD. By reading, unexecuted.

### CAD-67 Rebuild face
1. Advanced ▸ Rebuild face… (`registry.rs:402`).
2. `arrange.rs:162-174`: `Flow::Form` "Spans per direction:" 4 (1–64, a
   count).
3. `invoke.rs:62` (no face → "Select a face to rebuild") → form leg.
4. `POST /ops/rebuild_face {"args": [node, face, n, n]}`.
5. `commands.py:rebuild_face` (870).
6. The rebuilt face. RoboCAD `rebuild_face` (`ui/app.py:993-1001`):
   `getInt(4, 1, 64)`.

Matches RoboCAD. By reading, unexecuted.

### CAD-68 Dependent offset
1. Modify ▸ Dependent offset (face to body)… (`registry.rs:403`).
2. `arrange.rs:175-186`: `Needs::FaceThenNode`, "Clearance (mm):" 0.2
   (−10 to 10).
3. `resolve.rs:130-135` (the first node owning no selected face);
   a face alone → "Select a face, then the body to offset it to"; form leg.
4. `POST /ops/offset_face_to {"args": [node, face, target, 0.2]}`.
5. `commands.py:offset_face_to` (521).
6. The offset face ("Dependent offset"). RoboCAD `dependent_offset`
   (`ui/app.py:1003-1013`): the same rule and message.

Matches RoboCAD. By reading, unexecuted.

### CAD-69 Copy and paste with placement
1. Ctrl/Cmd+C, Ctrl/Cmd+V (`registry.rs:310-311`).
2. `edit_create.rs:21-43`: `Shape::Copy` → `args.rs:573-578`
   (`Read::Copy`); `Shape::Paste` → 579-583 (`Built::Paste`).
3. Copy: read leg; `receive` keeps `OpsState::clipboard` with the revision
   (`analysis_overlay.rs:285-289`, "Copied 2 item(s) with placement").
   Paste: `cad/ops/mod.rs:606-608` → `actions::edit` (Dedicated job).
4. `crates/sim-runtime/src/cad_client/mod.rs:copy_nodes` (344),
   `paste` (351).
5. `api.py:1672-1676` → `Service.copy` (848, `Document.copy_nodes`) and
   `Service.paste` (856-891: one `AddNodes("Paste", …)`).
6. "Pasted 2 item(s)"; one "Paste" step; `cad_state.ops.clipboard`
   (`cad/ops/state.rs:41-44`). RoboCAD `copy_with_placement` /
   `paste_with_placement` (`ui/app.py:1465-1484`) use the OS clipboard.

Deliberate difference (recorded): the clip stays in the viewer. By
reading, unexecuted.

### CAD-70 Curvature comb
1. Inspect ▸ Curvature comb on selected curve (`registry.rs:443`).
2. `boolean.rs:100-110` → `args.rs:590-593`: the last selected curve
   (`last_of`, 556).
3. Read leg.
4. `crates/sim-runtime/src/cad_client/mod.rs:curvature_comb` (361).
5. `api.py:1658` → `Service.curvature_comb` (912; scale 5, 48 samples).
6. The comb in RoboCAD's violet. RoboCAD `curvature_comb`
   (`ui/app.py:1277-1284`): each curve or sketch with a body; a sketch has
   none.

Matches RoboCAD, including a curve then a sketch: both draw the curve's
comb (RoboCAD's loop skips the bodiless sketch, leaving the curve's drawn;
`last_of` picks the last curve). The table's "the viewer shows none"
recorded difference no longer holds (the table row is updated in this batch); the refusal for no curve stays ours.
By reading, unexecuted.

### CAD-71 Continuity
1. Inspect ▸ Continuity check (G0/G1/G2) (`registry.rs:444`).
2. `boolean.rs:111-121` → `args.rs:596-599`: the last selected node with
   a body.
3. Read leg.
4. `crates/sim-runtime/src/cad_client/mod.rs:continuity` (366).
5. `api.py:1660` → `Service.continuity` (945).
6. Edges in G0 red, G1 amber, G2 green, boundary grey
   (`analysis_overlay.rs:grade_colour`, 203) and "Continuity: {'G0': …,
   'G1': …, 'G2': …, 'boundary': …}" (`counts_text`, 214, 243). RoboCAD
   `continuity` (`ui/app.py:1286-1302`).

Matches RoboCAD. By reading, unexecuted.

### CAD-72 Toolbar
1. The command bar under the header (`cad/surfaces/toolbar.rs:spawn`,
   69): the menu tabs, then `TOOLBAR` (`registry.rs:258-261`, RoboCAD's 25
   in order) as kit chips (`refresh`, 159-196).
2. Each chip → `CadInvoke {id}` (surface leg); `Enabled` from
   `registry::ready`; lit while that tool or op is active (`lit`, 131-137,
   `checkable` 126).
3. The hint (`describe`, 140-148; `hint`, 201-218) names the keys and, when
   disabled, why. The wheel scrolls the row (`scroll`, 223-242).
4. –5. No RoboCAD call of its own (each command's own route).
6. RoboCAD's `QToolBar` (`ui/app.py:440-446`): the same ids, `tool.*`
   checkable, tooltips the labels; overflow behind "»".

Deliberate difference (recorded): the row scrolls; the sketch buttons
light. The table's "later-epic buttons are disabled naming their epic"
no longer applies (the table row is updated in this batch): all 25 resolve natively (Annotate, References, Pose
and Experiments to `Do::Organize`, `registry.rs:288-294`; Fastener and
Validate are catalogue ops, 386, 433), so none is disabled for an epic.
Gap found and fixed: the module doc still said later-epic entries are
disabled (`cad/surfaces/toolbar.rs:11-18` now says why a button can be
disabled and that none belongs to a later epic). By reading, unexecuted.

### CAD-73 Right-click menu
1. A right press and release within 4 px over the 3D view
   (`cad/surfaces/context_menu.rs:input`, 48-72; a drag stays the orbit) →
   `CadSurface {context {at}}`; REST `cad_surface {"surface":{"kind":"context"}}`.
2. `cad/surfaces/mod.rs:handle` (215) → `entries` (189-197): `CONTEXT`
   (`registry.rs:264`, RoboCAD's 14 in order), the Sketch section
   (`SKETCH_CONTEXT`, 271), Make unique when an instance is selected.
3. Drawn by `draw` (494-498, `popup_list`); a click → surface leg.
4. –5. Each entry's own route.
6. RoboCAD `_context_menu` (`ui/app.py:1103-1107`).

Deliberate difference (recorded): the Sketch section and Make unique.
The table's "Annotate, Comments panel, Isolate and Hide disabled naming
their epic" no longer applies (the table row is updated in this batch): Annotate and Comments run through
`Do::Organize` (`registry.rs:294-295`), Isolate and Hide are catalogue
ops (`cad/ops/catalogue/view.rs`). By reading, unexecuted.

### CAD-74 Radial menus
1. Space, Q (`registry.rs:343`, 349) → `cad/keys.rs:404-409`:
   `CadSurface {view_radial|select_radial {at: pointer}}`; Space is typed
   while a kit field has the keyboard (`keys.rs:366`).
2. `cad/surfaces/mod.rs:handle`; `entries` (198-199): `VIEW_RADIAL`
   (Front, Top, Right, Iso, Ortho, Grid, Mode, Fit) and `SELECT_RADIAL`
   (`registry.rs:281`, 284).
3. `radial::spawn` (45) → `ui_kit/pie.rs:Kit::pie` (60; first entry up,
   clockwise, `slot` 51); `radial::input` (56-101): hover by
   `ui_kit/pie.rs:index_at` (38, 18 px dead centre), a press inside or a
   release runs the entry under the pointer then closes; a press outside
   closes (89-91); Escape closes (Escape leg).
4. –5. Fit → `Do::Fit` → `CadFit`; the views and Ortho → camera intents
   (`registry.rs:camera`, 635); Edge → `CadSelectMode`. No RoboCAD call.
6. RoboCAD `RadialMenu` (`ui/widgets.py:821-882`) opened by `view_radial`
   / `selection_radial` (`ui/app.py:1095-1101`).

Deliberate difference (recorded): rounded-rectangle pills. The table's
"the others say … belongs to the cad-views-export epic" no longer
applies: every view entry runs (`radial.rs:3-8`). By reading, unexecuted.

### CAD-75 Palette
1. Control+Space or Shift+F (`registry.rs:298`) → `CadSurface {palette}`.
2. `cad/surfaces/palette.rs:spawn` (105) → `ui_kit/palette.rs:Kit::palette`
   (138): the search field is the one kit text field (`PALETTE`,
   `cad/surfaces/mod.rs:322`; `ui_kit/palette.rs:167` → `Kit::input`,
   `ui_kit/widgets.rs:237`), focused when the palette opens
   (`palette.rs:150-156`); placeholder "Type a command… (Ctrl+Space)"
   (`placeholder`, 95).
3. Typing re-ranks (`ui_kit/palette.rs:rank`, 96-128: score 0, 1 + the
   label position, or 50; sorted by (score, label); 60 shown); Up/Down
   (`palette.rs:199-200`); Enter runs the highlighted row (167-184);
   Escape closes (188). Conflicts (`conflicts`, 60) give "Edit: Select Same
   Material    [Ctrl+Shift+M]  ⚠ conflicts with Robot: add motor from
   library…".
4. –5. The row's command (surface leg).
6. RoboCAD `CommandPalette.refresh` (`ui/widgets.py:79-108`): the same
   score, sort and text.

Deliberate difference (recorded): rows that cannot run are noted and
disabled; today only the "not ported" ones (`registry.rs:306-473`,
`Native::Different`), as no command maps to a later epic. Gap found and
fixed: the palette's Enter wrote its action unstamped while a row's click
is stamped with its source; it now goes through `cad::activation::guard`
(`cad/surfaces/palette.rs:171-179`). By reading, unexecuted.

### CAD-76 Menus by category
1. The tabs (`cad/surfaces/menus.rs:tabs`, 44-50; lit in place, `light`
   55-67) → `CadSurface {menu {category}}`; REST
   `cad_surface {"surface":{"kind":"menu","category":"Modify"}}` (an
   unknown category is refused naming the 16, `cad/surfaces/mod.rs:220-224`).
2. `entries` (188): the category's commands in registry order, "General",
   "Window" and "Tools" in Help (`registry.rs:menu_of`, 252), each with its
   keymap keys (`shortcut_keys`, 176).
3. `draw` (484-492) places it under its tab; a click runs the command and
   closes the menu (`cad/surfaces/mod.rs:364-376`); the wheel scrolls it
   (`popup_scroll`, 426).
4. –5. Each command's own route.
6. RoboCAD `_build_menus` (`ui/app.py:433-438`): the same 16 menus, the
   same fallback to Help.

Matches RoboCAD. By reading, unexecuted.

## Reading traces — Part E

Each trace follows one Part E step (CAD-77 to CAD-98: the active plane,
the plane tools, the sketch tools, the sketch edits and `cad_sketch`, the
solids made from sketches, the primitives and plane-dependent operations
on the active plane, the surfaces, the leaving guard and undo) from the
native control to RoboCAD and back to what the tree, the status line, the
plane quads, the sketch curves, the readout and `cad_state` show.
Everything here is by reading, unexecuted: nothing was built, run or
captured, and no step was compared side by side. Native paths are under
`crates/sim-spatial/src/` unless they start with `crates/`; RoboCAD paths
are under `cad/robocad/`. The gaps found while tracing were fixed in this
batch (each named in its entry) unless an entry says it is recorded.
The common legs are written out once:

- **Invoke leg** (a menu entry, palette row, toolbar button, context-menu
  row or key): a registry command with `Native::Op`
  (`cad/surfaces/registry.rs:378-385` the Planes, :405-423 the Sketch
  menu, :360-365 Create) is written as `CadInvoke { id }` (keys:
  `cad/keys.rs:keys`, 352, `registry::ready` then `CadInvoke` at 403-407;
  menus, palette, toolbar and context menu write the same action from
  their entry) → `cad/actions.rs:apply` (354) → `handle` (459) → the
  catalogue arm (561) → `cad/ops/mod.rs:handle` (491) →
  `cad/ops/invoke.rs:invoke` (12): `Flow::View` applies viewer state at
  once (17); `Flow::Sketch`, `Flow::Extrude` and `Flow::PlanePick` (18-58)
  end a transform tool (`end_tool`, 150), clear every other interaction
  (`clear_interactions`, 138), open the tool's form beside the view (30;
  none for a plane tool, 26-28), start the interaction
  (`sketch::interact::begin`, `sketch::extrude::begin`,
  `sketch::plane::begin`, 36-38) and show the tool's hint (49). A
  refusal from a click is written to the status line in `apply`
  (`actions.rs:408`). REST `cad_invoke {"id"}` sends the same action.
- **Run leg** (`CadRun`: a plane tool's picks, an extrude release, a
  form's OK, REST `cad_run`): `ops/mod.rs:handle` → `run` (528) →
  `prepare` (543): viewer state returns before any refusal (545); else
  `CadDocument::commit_refusal(revision)` (549;
  `cad/document/state.rs:138-154`: an edit in flight, not connected, the
  shown document stale, or RoboCAD's revision moved since `revision`),
  `resolve::resolve` (569; `cad/ops/resolve.rs:51`), `values`, then
  `args::build` (`cad/ops/args.rs:562`, keyed by shape: `Plain` 564,
  `Place` 566, `Sketch` 567, `SketchEdit` 568, `Extrude` 569,
  `View` 570) → `start` (584): `Built::Edit` → `actions::edit` →
  `cad/edit.rs:edit` (12) → `cad/sync/mod.rs:start_edit` (686: one
  `Job::spawn(Pool::Dedicated, …, "RoboCAD edit: …")`, 693, off the UI
  thread) → `CadClient::op` (`crates/sim-runtime/src/cad_client/mod.rs:374`,
  `POST /ops/{name}`) → `api.py:1668-1671` → `Service.op` (1011; a
  `KernelError` answers 422 with its text, 1025-1026) → the `Ops` method,
  one command on RoboCAD's stack → `sync::finish_edit` (565): the edit's
  message or RoboCAD's error on the status line (601), a plane tool's new
  node noted (595-600), `/doc` refetched (`refresh`, 630).
- **Sketch leg** (`CadSketch`: every finished shape, REST `cad_sketch`)
  and the **sketch-edit leg** (`Built::Sketch`: a form's OK or the Tab
  values, the offset, fillet and join edits): `ops/mod.rs:handle` arm
  (499) → `cad/sketch/edits.rs:sketch_action` (322: `commit_refusal`,
  then `prepare`, 276: each call read by `SketchCall::from_json` and
  checked by `check_calls`, 303; the target by `shape_target`, 257), or
  `start` (`ops/mod.rs:609`) → `ops/mod.rs:send_sketch` (634, the one
  path) → `actions::edit` → `start_edit` (as above) →
  `CadClient::edit_sketch`
  (`crates/sim-runtime/src/cad_client/sketch/mod.rs:430`, `POST
  /nodes/{id}/sketch {"calls"}`) → `api.py:1664-1666` →
  `Service.edit_sketch` (962: an index past the curves is RoboCAD's 400
  "curve index {i} out of range ({n} curves)", 970; an unknown name "no
  sketch method {m}", 977; the calls run on a copy inside
  `Ops.edit_sketch(…, label="Sketch (API)")`, 996, a `KernelError`,
  `ValueError` or `TypeError` answers 422 with its text, 997-998) →
  `commands.py:edit_sketch` (449-454: `fn` runs on a copy and ONE
  `SetAttributes` is pushed only after it returned, so a refused call
  leaves no partial geometry); or, for a new sketch,
  `CadClient::create_sketch` (sketch/mod.rs:438, `POST /nodes {"kind":
  "sketch", "plane", "calls"}`) → `api.py:1625` → `Service.create` (702;
  sketch 715-731: `Ops.new_sketch`, `commands.py:444-447`, one "Sketch"
  step, then `edit_sketch`; a refused call undoes that "Sketch" step and
  drops it from the redo stack, 721-728, then raises its error: no empty
  sketch is left behind). The answer lands in `finish_edit` as above
  (the polygon's side count follows a successful polygon,
  `sketch::specs::polygon_edit_done`, sync/mod.rs:592).
- **Error text**: a RoboCAD refusal reaches the status line as
  `CadError`'s Display (`crates/sim-runtime/src/cad_client/mod.rs:191-201`):
  "RoboCAD POST /nodes/…/sketch: " then RoboCAD's `error` verbatim, then
  " (HTTP n)". The viewer's own refusals before sending use RoboCAD's
  words where RoboCAD has them (the collinear points, the curve index,
  the unknown method and the unknown plane; see CAD-83 and CAD-90).
- **Plane reads**: `cad/sketch/cache.rs:sync` (195; SimSync) reads every
  sketch and plane node of the shown tree with `GET /nodes/{id}` on
  `Pool::Dedicated` jobs (246; at most two at once), keyed by (node,
  revision); `CadSketches::plane` and `::sketch` answer only at the shown
  revision. `cad/sketch/plane.rs:sync` (96, `CadSet::Plane`, chained
  after the cache, `cad/mod.rs:219`) → `follow` (104) keeps
  `CadActivePlane` (`cad/sketch/mod.rs:88`) to the document generation
  (105-108), adopts a plane tool's new node (110-117), follows a selected
  plane node (118-130), drops a node gone from the shown tree with a
  status naming it (142-151), and refreshes the node's frame to the shown
  revision's only (154-157). While a plane node's frame is not read,
  `CadActivePlane::frame` (mod.rs:107) refuses naming it ("the active
  plane (node …) is still being read from RoboCAD; try again in a
  moment", 114), and every press that needs the plane shows that refusal
  with nothing picked or sent.

### CAD-77 Active plane XY / XZ / YZ
1. Planes ▸ Active plane: XZ (registry `tool.plane_xz`,
   `cad/surfaces/registry.rs:383`), the palette row, or REST `cad_invoke
   {"id":"tool.plane_xz"}`; invoke leg → `ops/invoke.rs:17`.
2. `CadInvoke` → catalogue entry `tool.plane_xz`
   (`cad/ops/catalogue/plane.rs:82-91`, `Flow::View(ViewAct::Plane(Xz))`)
   → `cad/sketch/plane.rs:view_act` (172): the active plane becomes
   `ActivePlane::Base(Xz)` (179) and the status is RoboCAD's "Active plane
   set" (`SET`, 64; 180). REST `cad_run` of the same id returns before
   any refusal (`ops/mod.rs:545`, `run` 533): viewer state, never an edit.
3. No job, no RoboCAD call: RoboCAD's `viewport.active_plane` is its GUI
   state too.
4. No `cad_client` function.
5. RoboCAD's counterpart: `ui/app.py:355` → `set_active_plane(None,
   Plane.xz())` (`ui/app.py:1022-1027`, status "Active plane set");
   `Plane.xz()` is `PlaneFrame::XZ`
   (`crates/sim-runtime/src/cad_client/sketch/mod.rs:70`).
6. Shown: the ±60 mm square (`cad/sketch/plane_draw.rs:36`) filled at
   alpha 0.18 and outlined at 0.8 (38-39): `wanted` (77) adds the active
   plane when no visible plane node is it (89-93), `quads` (118) rebuilds
   the fill under the CAD root, `outlines` (164) draws the edges;
   `cad_state.plane` reads "XZ" (`plane.rs:state_json`, 209). A later
   sketch (`specs::calls`, `specs.rs:259`; `interact::pointer`) or
   primitive (`ops/args.rs:place`) uses the frame (`frame_or_xy`,
   mod.rs:123). RoboCAD draws quads for plane nodes only
   (`ui/viewport.py:635-657`).

Deliberate difference (recorded): the viewer draws the square for a named
plane. By reading, unexecuted.

### CAD-78 2D snapping
1. Planes ▸ Toggle 2D snapping to the active plane (`tool.plane_2d_snap`,
   registry.rs:385; `cad_invoke`).
2. `CadInvoke` → `plane.rs:view_act` (172), `ViewAct::Snap2d` (183-186):
   `snap_2d` flips, status "2D snapping on" / "2D snapping off".
3. No job, no RoboCAD call.
4. No `cad_client` function.
5. RoboCAD: `ui/app.py:357` → `toggle_plane_snapping`
   (`ui/app.py:1029-1031`, the same status lines); its snap projects onto
   the active plane when `plane_snapping` is on (`ui/viewport.py:1369-1374`,
   `want_plane or (self.active_plane if self.plane_snapping else None)`).
6. Shown: the measure tool's hover projects onto the plane
   (`cad/measure.rs:221`, `snap::snap_plane`, `cad/snap.rs:214`) and a
   press refuses by name while the active plane node is being read
   (`measure.rs:238`, `snap::press_snap_plane`, `snap.rs:205-209`: "2D
   snapping is on and the active plane (node …) is still being read …;
   nothing was picked"); `cad_state.plane.snap_2d`.

Matches RoboCAD. By reading, unexecuted.

### CAD-79 Plane from face
1. Ctrl/Cmd+P or Planes ▸ Plane from face (`tool.plane`, registry.rs:378);
   invoke leg → `ops/invoke.rs:18-58`; `cad/sketch/plane.rs:begin` (193):
   picks cleared, selection mode Face (195-202), pushed with the items
   (`invoke.rs:54-56`); status "Click a face" (catalogue `plane.rs:27`).
2. A left press over the view: `cad/sketch/plane.rs:picks` (279; SimSync
   after `CadSet::View`, `CadSet::Mesh` and `sync`, 72): the face under the
   cursor (`ray_hit`, then `CadMeshes::face_at` at the shown revision,
   306-315; a body being redrawn refuses by name, 309-312). With its one
   pick `run_for` (243) builds `CadRun {tool.plane, items [[node, "face",
   i]], revision: the pick's}`; `commit_refusal` is checked first and a
   refused run keeps the picks (348-353); else the picks clear (the tool
   stays active, 355) and the run is written (356). REST `cad_run
   {"id":"tool.plane","items":[…],"revision":N}` sends the same.
3. Run leg: `Arg::Node`, `Arg::Face` (`ops/args.rs:314`) →
   `start` → `started` (`ops/mod.rs:662`) marks the edit `activates_plane`
   (668-672).
4. `CadClient::op("plane_from_face", [node, {node, face}])`
   (`cad_client/mod.rs:374`).
5. `api.py:1668-1671` → `Service.op` (1011) → `commands.py:plane_from_face`
   (874-876) → `_add_plane` (892-895): one `AddNodes("Plane")` step.
6. `finish_edit` notes the answer's `result` id as `ops.plane_created`
   (`sync/mod.rs:595-600`); `plane::follow` makes it the active plane and
   says "Active plane set" (`plane.rs:110-117`), keeps it while the
   refetched tree has not shown it yet (`seen.unseen`, 137-151); its frame
   arrives through the plane reads; `plane_draw::wanted` draws the active
   node's square brighter (`plane_draw.rs:84-86`). Escape ends the tool
   (CAD-88's Escape leg; "Cancelled Plane from face", `ops/form.rs:145`).
   RoboCAD: `PlaneTool.press` (`ui/tools.py:1081-1095`): the clicked face,
   `plane_from_face`, `set_active_plane(pid)`, `self.picks = []`.

Matches RoboCAD. By reading, unexecuted.

### CAD-80 Plane from three points, two points and midplane
1. Planes ▸ Plane from three points / two points (camera) / Midplane
   between two faces (registry.rs:379-381); invoke leg; `plane::begin`
   (`plane.rs:193`): Vertex mode for three and camera, Face for midplane
   (195-198).
2. `plane.rs:picks` (279): point modes take the snap (`snap::snap_on`
   over the drawn candidates, on the active plane only with 2D snapping
   on; refused by name while that plane node is being read, 326-334) as
   `PlanePick::Point(s.exact)` (337); face modes as in CAD-79. Short of
   the count the picks are kept and the status counts them ("Click three
   points (1 of 3)", 358-362; `needed`, 229-235); complete, `run_for`
   (243) writes one `CadRun`: points as the "x, y, z" parameters a, b, c
   (260-263), faces as items with their revision.
3. Run leg; `plane_camera`'s direction is `Arg::ViewDir`
   (`ops/args.rs:333`: the view's direction at the run, or the
   `direction` parameter); `activates_plane` as CAD-79.
4. `CadClient::op` with `plane_three_points`, `plane_two_points_camera` or
   `plane_midplane`.
5. `Service.op` → `commands.py:878-890` (`Plane.from_three_points`; the
   camera plane's normal facing the camera, 881-885; the midplane of the
   two face planes, 887-890) → `_add_plane` (892).
6. The new plane becomes active as in CAD-79. RoboCAD: `PlaneTool.press`
   (`ui/tools.py:1096-1111`), `set_active_plane(pid)` after each.

Matches RoboCAD. By reading, unexecuted.

### CAD-81 Selecting a plane node
1. A click on a plane node in the tree: the one Selection changes
   (`cad/selection`, the tree's click).
2. No action: `cad/sketch/plane.rs:follow` (104) sees the selection change
   (119-120) to exactly one node of kind "plane" (121-124) and makes it
   the active plane with "Active plane set" (126-128).
3. No job; its frame comes from the plane reads (`cache.rs:sync`, 195).
4. `CadClient::node` (`cad_client/mod.rs:288`) on the cache's job, read
   by `plane_of` (`cad_client/sketch/mod.rs:401`).
5. RoboCAD has no such gesture; `set_active_plane(node_id)`
   (`ui/app.py:1022-1025`) is reached only from a plane tool.
6. Shown: the node's square brightens (`plane_draw.rs:84-86`), the next
   sketch goes on it (`specs::target`, `specs.rs:198`, with the node's
   frame); while its frame is being read the header says "(reading)"
   (`mod.rs:146`) and presses are refused by name (`mod.rs:114`).

Deliberate difference (recorded native addition). By reading, unexecuted.

### CAD-82 Line, chaining
1. L or Sketch ▸ Sketch: Line (registry.rs:405); invoke leg;
   `cad/sketch/interact.rs:begin` (118) sets `ops.sketch`.
2. `interact.rs:pointer` (280; SimSync, after `CadSet::View`,
   `CadSet::Mesh` and `CadSet::Plane`): the cursor snapped on the active
   plane, else XY (`frame_or_xy`, 314; `snap_on` with the plane, 328);
   a left press → `press` (241): the first records `began` (243-244) and
   a completing press is checked first (`refusal`, 218: `commit_refusal`,
   then `edits::shape_target`) so a shape that would be refused is not
   taken (the points kept, "Sketch line not sent: …; click again when
   RoboCAD has caught up"); complete → `finish_action` (172) builds
   `CadSketch { node: None, plane, calls, revision: Some(began) }` (181)
   from `specs::from_points` (`specs.rs:104`), written at 380; the line
   keeps its last point (`reset_after_finish`, 186; `chained`). Tab,
   length 20, angle 30, Enter → `CadFormSubmit` → `ops/form.rs:submit`
   (15): the first clicked point as `anchor` with its click's revision
   (31-41) → run leg → `specs::calls` (`specs.rs:259`) →
   `from_values` (156: `Line` from length and angle).
3. Sketch leg; `specs::target` (`specs.rs:198`) picks RoboCAD's
   `_ensure_sketch` target: a selected sketch on the plane, else the first
   visible one, else `SketchTarget::New` on the plane argument.
4. `CadClient::create_sketch` for the first shape, then
   `CadClient::edit_sketch`.
5. `Service.create` (two steps "Sketch" then "Sketch (API)") and then
   `Service.edit_sketch` per line.
6. The readout "length L  angle A" (`interact.rs:readout`, 153),
   `cad_state.ops.sketch`; the preview in (0.4, 0.9, 1.0)
   (`cad/sketch/preview.rs:26`, `PREVIEW`, drawn by `draw` 158); the curves drawn from the cache
   (`cad/sketch/display.rs:74`). Escape ends the chain and the tool
   (CAD-88). RoboCAD: `SketchTool` (`ui/tools.py:638-817`): `activate`
   creates the sketch (`_ensure_sketch`, 675-686), `press` (692-697),
   `_finish` (762-770, lines chain at 768), `commit` (784-794).

Deliberate differences (recorded): the sketch is created with the first
shape; the history label of a REST edit is "Sketch (API)". By reading,
unexecuted.

### CAD-83 Rectangle, centre rectangle, circles, arc
1. Shift+L, Sketch: Rectangle (centre), C, Sketch: Circle (two points),
   Sketch: Circle (three points), A (`sketch.arc_3pt`, registry.rs:406-414);
   invoke leg.
2. One interaction for all (`interact.rs:pointer`, 280), each shape's
   row in `specs.rs:55-69` (`needed` 2 or 3); `from_points`
   (`specs.rs:104`) builds RoboCAD's `_build` calls; three collinear
   points are refused by the kernel's words "the three points are
   collinear" (`collinear`, specs.rs:93-96; shown at `interact.rs:395`)
   with nothing sent. Tab values: `from_values` (156); OK on circle_2pt,
   circle_3pt or arc_3pt is refused "sketch.circle_2pt has no exact
   values: click its points on the plane …" (183).
3. Sketch leg.
4. `CadClient::edit_sketch` / `create_sketch`; over REST a collinear
   `circle_three_point` or `arc_three_point` is refused before sending by
   `SketchCall::check` (`crates/sim-runtime/src/cad_client/sketch/calls.rs:497`).
5. `Service.edit_sketch` → `kernel/sketch.py:circumcircle` (613-616:
   `KernelError("the three points are collinear")`, answered 422).
6. The curves in the tree's sketch and `GET /nodes/<id>/sketch`; the
   refusal on the status line. RoboCAD: `_build` (`ui/tools.py:728-744`),
   `_finish`'s `self.ctx.error(str(e))` (762-767).
   Gap found and fixed: `cad_sketch`'s pre-send refusal said "arguments
   a, b and c are collinear (no circle passes through …)" where RoboCAD's
   answer is "the three points are collinear"; it now reads
   "circle_three_point: the three points are collinear (arguments a […],
   b […] and c […])", RoboCAD's words then the arguments
   (`crates/sim-runtime/src/cad_client/sketch/calls.rs:503`).

Deliberate differences (recorded): the collinear points are refused before
the kernel call; OK on a tool with no Tab values is refused by name.
By reading, unexecuted.

### CAD-84 Polygon sides memory
1. Shift+P (`sketch.polygon`, registry.rs:415); invoke leg;
   `interact.rs:begin` (118) opens the `sides` draft at the remembered
   count (`ops.polygon_sides`, 6 at first, 120-125).
2. Tab, radius 10, sides 8, Enter → `ops/form.rs:submit` (15) → run leg
   → `specs::calls` (`specs.rs:259`) → `from_values` (156: sides truncated
   and at least 3, 167-176) at the plane origin; a clicked polygon is
   `from_points` (104) with the remembered count and the rotation toward
   the second click (119).
3. Sketch leg; `send_sketch` notes the side count
   (`specs::note_polygon_sides`, `specs.rs:236`, from `ops/mod.rs:647-650`)
   and `finish_edit` makes it the remembered one when the edit succeeded
   (`polygon_edit_done`, `specs.rs:244`; `sync/mod.rs:592`).
4. `CadClient::edit_sketch` / `create_sketch` (`polygon` with sides).
5. `Service.edit_sketch` → `kernel/sketch.py:polygon` (261-263: `sides or
   last_polygon_sides`, then `last_polygon_sides = sides`).
6. The octagon's curve; the next Shift+P opens with 8. RoboCAD: `_fields`
   (`ui/tools.py:668`, `Sketch.last_polygon_sides`), `commit` (799-800).

Matches RoboCAD; deliberate difference (recorded): a clicked polygon
sends its side count. By reading, unexecuted.

### CAD-85 Slot, ellipse, spiral
1. Shift+S, Sketch: Ellipse, Sketch: Spiral (registry.rs:416-419).
2. `interact.rs:pointer`/`press` with the rows `specs.rs:64`, :66, :67;
   `from_points` (`specs.rs:120-139`: the slot's width from the third
   click, at least 0.5; the ellipse's radii and rotation; the spiral 0.15 r
   to r, 3 turns); Tab values `from_values` (178-180).
3. Sketch leg.
4. `CadClient::edit_sketch` / `create_sketch` (`slot`, `ellipse`,
   `spiral`).
5. `Service.edit_sketch` → `kernel/sketch.py` constructors.
6. The curves from the cache, sampled by `SketchCurve::sample`
   (`crates/sim-runtime/src/cad_client/sketch/mod.rs:229`; the slot's caps
   bulge outward, `slot_points`, 282). RoboCAD: `_build`
   (`ui/tools.py:747-758`), `commit` (801-806); its viewport draws the
   slot through `_slot_points` with the caps inward.

Deliberate difference (recorded): the slot's drawn caps. By reading,
unexecuted.

### CAD-86 Spline
1. Shift+C (`sketch.spline`, registry.rs:417).
2. `interact.rs:pointer`: each press adds a point (`Finish::EnterOrDouble`,
   `specs.rs:65`); Enter with no field typing and no surface open (339) or
   a second press within 400 ms and 5 px (`DOUBLE_CLICK`,
   `DOUBLE_DISTANCE`; 341) finishes when there are two or more points
   (366: `finish_check`, 258, the completing checks). Enter with one point
   does nothing (366). The form's OK refuses "sketch.spline has no exact
   values …" (`specs.rs:183`). The form's own Enter stands aside for a
   sketch tool (`cad/surfaces/form.rs:322`).
3. Sketch leg.
4. `CadClient::edit_sketch` / `create_sketch` (`spline`).
5. `Service.edit_sketch` → `kernel/sketch.py:spline`.
6. One spline curve. RoboCAD: `double` and `key` (`ui/tools.py:772-781`).

Matches RoboCAD; the OK refusal is recorded. By reading, unexecuted.

### CAD-87 Text
1. T (`sketch.text`, registry.rs:420); invoke leg; `interact.rs:begin`
   (118) focuses the form's "Text to sketch:" field (126-131).
2. Typing edits the draft; Enter in that field blurs it instead of OK
   (`cad/surfaces/form.rs:235-238`: RoboCAD's `getText` dialog's OK starts
   the clicks); one click finishes (`Finish::Points(1)`, `specs.rs:68`) →
   `finish_action` (`interact.rs:172`): empty text is refused "type the
   text to sketch first (the form's "Text to sketch:" field), then click
   where it starts" (174-175); else `Text { height 10 }`
   (`CLICKED_TEXT_HEIGHT`, `specs.rs:79`). Tab height 5, Enter →
   `specs::calls` (259; empty text refused, 273-275) → `from_values` (181).
3. Sketch leg.
4. `CadClient::edit_sketch` / `create_sketch` (`text`).
5. `Service.edit_sketch` → `kernel/sketch.py:text` (font outlines).
6. The outlines; the preview is a placeholder box (`preview.rs` module
   doc). RoboCAD: `start_sketch` (`ui/app.py:743-750`), `_build`
   (`ui/tools.py:759-760`: `text_height` never set, so 10), `commit`
   (807-808).

Deliberate differences (recorded): the dialog is the form's first field;
the empty text is refused; the preview box. By reading, unexecuted.

### CAD-88 Tab, Enter and Escape in a sketch tool
1. During a rectangle after one click: Tab → the form's first field takes
   the keyboard (`cad/surfaces/form.rs`, the module doc's keys; RoboCAD's
   "Numeric entry (Tab)"); Enter in the field → `CadFormSubmit`; Enter
   with no field focused does nothing for a sketch tool (322); Escape:
   a typing field's Escape is the kit's `Cancel` → `CadFormCancel`
   (form.rs:245-248); else `cad/surfaces/mod.rs:input` (InputSet::Window,
   chained before `keys::gate` in `CadKeySet::Gate`) writes
   `CadFormCancel` while a form or interaction is active and consumes the
   key (`surfaces/mod.rs:396-415`), so calibrate's
   (`CadKeySet::EscapeTool`), the threads' (`CadKeySet::Escape`) and the
   Select tool's (`CadKeySet::ToolKeys`, `transform/input.rs`, which also
   stands aside while `ops.form` or `ops.active` is set) Escape never act
   on the same press (`cad/mod.rs:182`: Gate → EscapeTool → Escape →
   ToolKeys).
2. `CadFormSubmit` → `ops/form.rs:submit` (15): the anchor is the first
   clicked point and the run carries that click's revision (33-41); the
   points clear after a sent run (44-51). `CadFormCancel` →
   `ops::form_cancel` (`ops/form.rs:128`): the form, the active op and
   the shape go with nothing sent (`unsent`, 134; `SketchState::unsent`,
   `cad/sketch/mod.rs:304`), "Cancelled Sketch: Rectangle" (145).
   `CadCancel` (REST `cad_cancel`) does the same while an op is active
   (`transform/mod.rs:366`, `cancel` 438).
3. Submit: run leg then sketch leg; Escape: no job.
4. `CadClient::edit_sketch` / `create_sketch` for the submit only.
5. RoboCAD: `commit` (`ui/tools.py:784-818`, the anchor `points[0]`);
   Escape → `self.tool.cancel()` then Select (`ui/app.py:487-497`).
6. The readout clears when the tool ends (`interact.rs:pointer`'s first
   branch); `cad_state.ops.sketch` null.

Matches RoboCAD. By reading, unexecuted.

### CAD-89 Offset, fillet corners, join
1. Sketch ▸ Sketch: offset selected curve… / fillet corner… / join curves
   (registry.rs:421-423; catalogue `ops/catalogue/sketch.rs:173-196`:
   forms "Distance (mm):" 1.0 and "Radius (mm):" 2.0, join immediate).
2. Run leg (`Shape::SketchEdit`, `ops/args.rs:568`) →
   `cad/sketch/edits.rs:calls` (185): the sketch by `selected_sketch`
   (51; none: "Select a sketch"), read at the shown revision, curves the
   read dropped refused naming them (`dropped_refusal`, 59); offset: one
   `offset` per curve; fillet: `fillet_plan` (167) runs the kernel's
   `fillet_corner` in plane space and sends only the corners RoboCAD would
   round, refusing "No corner of … takes a 50 fillet: …" (210) when there
   are none; join: refused "Nothing to join: … has 1 curve(s)" (218).
3. Sketch-edit leg (one `POST /nodes/{id}/sketch`).
4. `CadClient::edit_sketch`.
5. `Service.edit_sketch` → `kernel/sketch.py:fillet_corner` (389-420),
   `offset` (422), `join` (430).
6. The curves; one "Sketch (API)" step. RoboCAD: `ui/app.py:758-785`
   (`_selected_sketch` 752-756; the fillet swallows each corner's
   `KernelError`; join only with two or more curves).
   Gap found and fixed: a sketch read with curves dropped (no kind) was
   refused with a count only; the refusal now names RoboCAD's curve
   indices ("… could not be read (no kind: RoboCAD's curve 2) …";
   `SketchGeometry::from_value` keeps the indices,
   `crates/sim-runtime/src/cad_client/sketch/mod.rs:377-389`;
   `CadSketches::dropped`, `cad/sketch/cache.rs:113`;
   `edits.rs:59-70`).

Deliberate differences (recorded): the empty-step cases are refused by
name. By reading, unexecuted.

### CAD-90 `cad_sketch` (REST)
1. REST `cad_sketch {"node", "calls", "revision"}` (`cad/rest_form.rs:60`,
   `cad/specs.rs:86`) → `CadAction::CadSketch` (`cad/actions.rs:251`).
2. `ops/mod.rs:handle` (499) → `edits.rs:sketch_action` (322):
   `commit_refusal(revision)`; `prepare` (276): each call read by
   `SketchCall::from_json`
   (`crates/sim-runtime/src/cad_client/sketch/calls.rs:352`, a refusal
   names the call and the argument; a non-finite number "must be a finite
   number (got null)"), the node must be a sketch of the shown tree, the
   target without `node` by `shape_target` (257: the plane given, else
   the active plane, else XY; RoboCAD's sketch tools' rule), dropped
   curves refused by name, `check_calls` (`calls.rs:560`) against the
   curve count. A call naming curve 9 of 3 is refused "cad_sketch: call 1
   (join): argument curves[2]: curve index 9 out of range (3 curves)".
3. Sketch leg.
4. `CadClient::edit_sketch` (sketch/mod.rs:430) or `create_sketch` (438).
5. `api.py:1664-1666` → `Service.edit_sketch` (962-999: curve indices
   become curves before two-number lists become points, 979-991).
6. The `cad_sketch` answer is the edit's (`edit.rs:34-37`, pending until
   `finish_edit`).
   Gaps found and fixed: (a) the pre-send refusals' words now are
   RoboCAD's where it has its own: "curve index {i} out of range ({n}
   curves)" (`calls.rs:521`, api.py:970), "no sketch method {m}"
   (`calls.rs:355`, api.py:977) and "unknown plane '{v}' (xy/xz/yz or a
   plane node id)" (`edits.rs:246-248`, api.py:213); (b) a stale revision
   was accepted: a REST `cad_sketch` naming curves by index without
   `revision` could apply indices read before another edit to the curves
   after it; it is now refused "cad_sketch call n (name): pass revision:
   the RoboCAD revision the curve indices in calls were read at"
   (`edits.rs:332-342`), as `cad_run` items naming faces need theirs.

Matches RoboCAD (one "Sketch (API)" step per call list). By reading,
unexecuted.

### CAD-91 Extrude, taper, Shift/Ctrl/Alt
1. X (`tool.extrude`, registry.rs:360; catalogue
   `ops/catalogue/solid.rs:21-33`, hint `HINT_EXTRUDE`, 8); invoke leg;
   `cad/sketch/extrude.rs:begin` (240): the source by `source` (161:
   the last selected sketch, curve or sheet, else the first visible sketch
   with curves; an unread sketch is `Reading` and refused by name).
2. `extrude.rs:pointer` (334; after `CadSet::Plane`, 136): a press starts
   the drag on the source's plane (`source_plane`, 229; a plane not read
   is shown as the refusal, 415) and records its boolean in the form;
   moving sets the height ("extrude h"); the release writes one `CadRun
   {tool.extrude, {distance, taper 0, boolean}, revision: the press's}`
   (456) with the release's modifiers (`boolean_for`, 207: Shift
   subtract, Ctrl/Command union, Alt intersect). Tab 5, 10, Enter →
   `ops/form.rs:submit`. REST `cad_run {"id":"tool.extrude",…}`.
3. Run leg → `extrude::calls` (263): `extrude(source, distance, None,
   taper, false, op, target)`, the target `body_under_selection` (192),
   "new" without one.
4. `CadClient::op("extrude", …)`.
5. `Service.op` → `commands.py:extrude` (480-487: `_profile`, 457-476,
   the outer loop and holes; `_apply_boolean`).
6. A new body or the united/subtracted/intersected target; the preview
   lines (`preview_lines`, 483) without the taper. RoboCAD: `ExtrudeTool`
   (`ui/tools.py:822-931`: press 855-857, drag 859-875, release 877-880
   with `self.taper` 0.0, `_boolean_for` 882-890, `_apply` 910-924,
   commit 926-931).

Deliberate differences (recorded): the source follows the selection; the
preview is outlines. By reading, unexecuted.

### CAD-92 Revolve
1. Shift+R (`tool.revolve`, registry.rs:361; `solid.rs:34-46`); invoke
   leg; `extrude::begin` (240, `revolve: true`).
2. Tab, angle 180, Enter → submit → run leg. A press and release in the
   view: `extrude.rs:pointer` writes `CadRun {tool.revolve, {angle "360",
   boolean}}` (447-456) with the readout `REVOLVE_READOUT` (320).
3. Run leg → `extrude::calls` (263): `revolve(source, plane.origin,
   plane.x_axis, angle or 360, op, target)` (283: RoboCAD's `angle or
   360.0`), the plane the source sketch's own.
4. `CadClient::op("revolve", …)`.
5. `Service.op` → `commands.py:revolve` (489-491).
6. A solid of revolution. RoboCAD: `_apply` (`ui/tools.py:916-918`),
   release with `angle=None` (877-880), commit (926-929).

Matches RoboCAD (both revolve 360° on a click). By reading, unexecuted.

### CAD-93 Sweep, pipe, loft, fill
1. Create ▸ Sweep / Pipe along selected curve… / Loft selected sketches /
   Fill / patch selected curve (registry.rs:362-365; catalogue
   `solid.rs:47-97`).
2. `Flow::Form` (sweep "Twist (degrees):", pipe "Diameter (mm):") or
   immediate (loft, fill): `invoke.rs:59-78` checks the selection first
   (`resolve::resolve`, `Needs::Nodes` of kinds sketch and curve,
   `resolve.rs:101`), refusing with RoboCAD's messages (`solid.rs:58`,
   :72, :83, :94).
3. Run leg; pipe fans out one call per node (`Fan::PerNode`, `solid.rs:71`).
4. `CadClient::op` (`sweep` with `{"twist_deg"}`, `pipe`, `loft`, `fill`).
5. `Service.op` → `commands.py:sweep` (493), `pipe` (498), `loft` (502),
   `fill` (507).
6. The solids. RoboCAD: `ui/app.py:801-830` (the same refusals and
   dialogs).

Matches RoboCAD. By reading, unexecuted.

### CAD-94 Primitives on the active plane
1. With XZ active, Box (corner), Box (centre), Cylinder, Sphere
   (registry.rs:356-359); invoke leg (`Flow::Place`, `invoke.rs:93-100`).
2. `cad/ops/interact.rs:pointer` (305): the plane is
   `CadActivePlane::frame_or_xy` (384; a node not read refuses the press by
   name); a change of the active plane ends the placement with nothing sent
   (397-402); the release writes `CadRun` with the press's revision (455).
3. Run leg → `ops/args.rs:place` (499): on XY `Ops.box`, else
   `Ops.box_three_point` with a, b, c so z is the plane's normal; the
   cylinder on the plane with its normal; without an anchor the plane's
   origin.
4. `CadClient::op` (`box`, `box_three_point`, `cylinder`, `sphere`).
5. `Service.op` → `commands.py:box` (419), `box_three_point` (426),
   `cylinder` (437), `sphere` (440).
6. The solids on XZ. RoboCAD: `PrimitiveTool` (`ui/tools.py:391-532`,
   `_make_box` 517: a sketch rectangle extruded, labelled "Extrude").

Deliberate difference (recorded): the box's undo label. By reading,
unexecuted.

### CAD-95 Plane-dependent operations
1. With the CAD-79 plane active: Ctrl/Cmd+M, Modify ▸ Cut with active
   plane, Split faces with active plane, Silhouette onto active plane,
   Draft faces…, Array… radial (registry.rs:388, :394, :396, :399).
2. Run leg; each entry's plane parameter defaults to "active"
   (`ops/catalogue/arrange.rs:15`, :60, :76, :98, :131;
   `modify.rs:140`), sent by `Arg::Plane` (`ops/args.rs:329`) as the
   active plane's argument (its node id, `CadActivePlane::arg_or`,
   `sketch/mod.rs:129`), else RoboCAD's fallback (YZ for mirror, XY for
   the rest); the radial array reads the frame (`plane_frame`, args.rs:275,
   refusing by name while the node is being read).
3. Run leg (one job).
4. `CadClient::op` (`mirror`, `cut`, `split_face`, `silhouette`, `draft`,
   `array_radial`).
5. `api.py:205-212` (`ArgConverter.plane`: a plane node id is the node's
   plane) → `commands.py:mirror` (694), `cut` (591), `split_face` (566),
   `silhouette` (860), `draft` (553), `array_radial` (738).
6. The results about that plane. RoboCAD: `ui/app.py:866-878`, 910-914,
   920-945, 953-955, 970-972 (`active_plane or Plane.xy()` / `.yz()`).

Matches RoboCAD. By reading, unexecuted.

### CAD-96 Toolbar and right-click menu
1. The toolbar (`registry::TOOLBAR`, registry.rs:258-261: Rectangle,
   Circle, Slot, Extrude among RoboCAD's) and the right-click menu
   (`CONTEXT` then `SKETCH_CONTEXT`, 264, 271-273; listed by
   `cad/surfaces/mod.rs:191`, the "Sketch" heading 496).
2. A click writes `CadInvoke` (invoke leg); the toolbar lights a sketch
   button while its tool is the active op or open form
   (`cad/surfaces/toolbar.rs:126-136`, `checkable` includes `sketch.`).
3. No job until a shape is finished.
4. —
5. RoboCAD: `_build_menus` toolbar (`ui/app.py:442`), `_context_menu`
   (`ui/app.py:1103-1107`): no sketch section, sketch buttons never lit.
6. The tool's hint on the status line (`invoke.rs:49`).

Deliberate differences (recorded native additions). By reading,
unexecuted.

### CAD-97 A shape in progress blocks leaving
1. The switcher's **Build** → the mode switch (`app/switch/mod.rs:517`).
2. `app/switch/prepare.rs:leaving_blockers` (30) adds
   `cad::sketch_blocker` (82; `cad/mod.rs:111`) →
   `cad/sketch/mod.rs:blocker` (331): clicked points not sent (`unsent`,
   304: a lone chained point was sent with its line) refuse "a sketch slot
   is in progress (2 point(s) clicked): finish it or press Escape"; the
   switch joins the blockers (`app/switch/mod.rs:518-520`; also at
   arrival, `app/switch/arrival.rs:64-66`).
3. No job; nothing sent.
4. —
5. RoboCAD has one window: changing tool drops the points.
6. The refusal on the switcher's status. Escape (`form_cancel`, CAD-88)
   drops the shape; Build then proceeds (or is refused by the unsaved-edit
   guard, `CadDocument::switch_blockers`, `cad/document/state.rs:206`).

Deliberate difference (native addition). By reading, unexecuted.

### CAD-98 Undo through the epic
1. Ctrl/Cmd+Z (`edit.undo`, registry.rs:307, `Do::Undo`) or REST
   `cad_undo`.
2. `CadAction::CadUndo` → `actions.rs:handle` (531) → `edit`.
3. `start_edit` (one job) → `finish_edit` ("Undid <label>").
4. `CadClient::undo` (`cad_client/mod.rs:390`, `POST /undo`).
5. `api.py:1677` → `Service.undo` (1046) → `Ops.undo`
   (`commands.py:305`): one command off the stack.
6. Each plane (`AddNodes("Plane")`, `commands.py:892-895`), shape
   ("Sketch (API)", one `SetAttributes` per call list, `commands.py:449-454`;
   a new sketch adds its "Sketch" step, `api.py:715-719`), sketch edit and
   solid (one `Ops` call, `extrude`/`revolve`/…) is one step, each undo
   reverses exactly one; a refused call left no step (`Service.create`
   undoes its "Sketch", api.py:721-728). RoboCAD: Edit ▸ Undo
   (`ui/app.py:291`).

Matches RoboCAD. By reading, unexecuted.

## Reading traces — Part F

Each trace follows one Part F step (CAD-99 to CAD-130: the shared camera,
display modes, grid, build plate, section, isolate and hide, saved views,
the tessellation tolerance, and new, open, save as, import, export and
render) from the native control to RoboCAD and back to what the 3D view,
the display panel, the Saved Views panel, the path form, the job strip,
the status line or the written file shows. Everything here is by reading,
unexecuted: nothing was built, run or captured, and no step was compared
side by side. Native paths are under `crates/sim-spatial/src/` unless they
start with `crates/`; RoboCAD paths are under `cad/robocad/`
(`ui/viewport.py`, `ui/app.py`, `ui/saved_views.py`, `api.py`, …). Steps
that only move the camera or change the display make no RoboCAD call and
are compared with RoboCAD's window code instead. Gaps found while tracing
were fixed in this batch (each named in its entry), unless an entry says it
is recorded. The common legs are written out once:

- **Key and menu leg** (every bound key, menu entry, palette row and
  radial slice): `cad/keys.rs:keys` (352) matches the press exactly against
  RoboCAD's command table (`cad/surfaces/registry.rs:COMMANDS`; the keypad's
  digits and `/` are the digits, `normalise` 210), refuses by name on the
  status line when `registry::ready` (573) says why, else writes
  `Act::ui(CadAction::CadInvoke { id })` (409); a menu entry, palette row or
  radial slice writes the same `CadInvoke`. The one apply system
  `cad/actions.rs:apply` (354) drains it into `handle` (459) → arm 561 →
  `cad/ops/mod.rs:handle` (491) → `ops/invoke.rs:invoke` (12): a catalogue
  entry runs (`ops/mod.rs:run` 528), anything else goes to
  `cad/surfaces/mod.rs:invoke_command` (246) → `registry::invoke` (619) →
  `registry::resolve` (495): `Resolved::Action` re-enters
  `actions::handle` with `Do::action` (109: `Do::Fit` 114, `Do::Display`
  121 → `DisplayCmd::action` 150, `Do::SavedViews` 123, `Do::File` 125 →
  `files::command_action`), `Resolved::Camera` → `registry::camera` (635),
  which pushes the camera intent (`CameraCmd::action`, 179) on
  `Cx::camera`; after the handler, `actions::apply` writes those as
  `Act<CameraAction>` (415-418). A click's refusal is the status line
  (actions.rs:408).
- **Camera leg** (every camera intent): `camera/apply.rs:apply` (175,
  `ViewerSet::Actions`) takes the active orbit camera (186) and runs
  `handle` (59); `camera/orbit.rs:place` (411, `CameraSet::Place` in
  SimSync) writes the camera's `Transform` and `Projection` only on a
  change; `cad/view.rs:update` (134, after Place) refreshes the CAD view
  snapshot picks and overlays project through; `cad/views/mod.rs:snapshot`
  (466) copies the `Orbit` for saved views. REST `camera_*` and
  `system_ui` `camera:*` parse to the same `CameraAction`
  (`camera/mod.rs:334`, `Action::parse` 417). No RoboCAD call: the native
  camera is this window's, RoboCAD's is its own (`api.py` `/view` is not
  used).
- **Display leg** (`cad_display`, `cad_section`): `actions.rs:handle` arm
  580 → `cad/display/mod.rs:handle` (493) → `apply_display` (338) or
  `apply_section` (384) on `CadDisplay`; display only, never an edit.
  Drawing: `display/draw.rs:materials` (138) and `section::preview` (367)
  in SimSync after `CadSet::Highlight`, `draw::lines` (333), `quads` (572),
  `lights` (632) and `ui::toolbar` (128) in Present
  (`display/mod.rs:build` 526-556).
- **Edit leg** (every document change, as Part G's): `cad/edit.rs:edit`
  (12) → `cad/sync/mod.rs:start_edit` (686): refused by name by
  `CadDocument::edit_refusal`, else one `Job::spawn(Pool::Dedicated, …,
  "RoboCAD edit: …")` (693) → the `sim_runtime::cad_client` call → RoboCAD
  `api.py:_route` (1540; `run = s.run_on_main` 1546) → `Ops` / `commands.py`
  (one undo step) → `sync::finish_edit` (565): an answer of an older
  generation is dropped, the edit's message or RoboCAD's error is the
  status line (601), a Save As retargets the window's file (609), then
  `refresh` (641, called at 630) refetches `/doc`; the tree, inspector and
  3D view follow the new revision. `edit_at` (48) adds
  `CadDocument::commit_refusal` (`cad/document/state.rs:138`) for values
  read at a revision.
- **File job leg** (new, export, render, the unit guess):
  `cad/files/jobs.rs:start` (143) spawns one `Pool::Dedicated` job, marked
  `complete_on_drop` for the writes (148: leaving CAD mode does not stop a
  sent request; its outcome is logged, `logged` 126), shows "label…" on the
  status line (155) and in the progress strip (`strip` 277, bottom left of
  the 3D view, each export and render with its **Cancel**), and makes a REST
  caller wait (`file_job` in its continuation; `wait` 181).
  `jobs::receive` (228; JobResults, before `CadSet::Results`,
  `files/mod.rs:build` 622, registered at 632) polls each job: its answer to a waiting REST
  caller, `cad_state.files.last`, the status line (262: an error stays an
  error, a job is never shown as done unless it succeeded), and what follows
  it (a new file's open, a unit guess into the form). RoboCAD's 4xx text is
  named through `jobs::named` (118).
- **Path form** (every file command without a path): `files/mod.rs:open_form`
  (459) → `files/form.rs:FileForm::new` (117), drawn modal over a kit
  backdrop that also covers the switcher strip (`form::draw` 613), typed in
  the one kit text field `form::FILES` (54; `form::input` 446 in
  `CadKeySet::Focus`, before `EscapeTool`, so its Escape closes the form
  and is consumed, 547), listing the path's directory through the kit path
  field's job (`ui_kit/path_field.rs:request` 130 on `Pool::Io`,
  `receive` 136, drawn by `Kit::path_listing` 227); OK writes the one
  action REST takes (`FileForm::action` 342, captured with the form's
  sequence by `activation::guard_files`), and a refusal comes back into the
  form (`files/mod.rs:handle` 286-291).

### CAD-99 Orbit, pan, zoom to the cursor
1. Right-drag, middle-drag (Shift+right-drag), the wheel over the 3D view:
   `camera/input.rs:navigate` (123, `CameraSet::Navigate` in SimSync); a
   drag latches its camera when a button goes down inside the view area and
   not over a panel (`accepts` 56: CAD's rules `yield_to_ui`,
   `cad/scene.rs:rules` 40-54) and keeps going until both buttons are up
   (149-154). REST `camera_orbit {"dx":100,"dy":0}`, `camera_pan`,
   `camera_zoom {"factor":0.8,"at":[x,y]}` → `camera/apply.rs:handle` arms
   93, 88, 110.
2. `drag_kind` (82) with CAD's `robocad_gestures`: right without Shift
   orbits → `Orbit::rotate` (`orbit.rs:245`: 0.007 rad/px, a drag right
   lowers the yaw, a drag down raises the pitch, clamped to CAD's 89.5°,
   `scene.rs:44`); middle or Shift+right pans → `Orbit::pan` (238:
   0.0015 × radius per pixel along the view's right and up). The wheel
   (`wheel_lines` 45) zooms by `exp(−0.12 × lines)` toward
   `camera/input.rs:cursor_anchor` (217: the point under the cursor on the plane through
   the focus facing the view, `focus_plane_hit` 323) → `Orbit::zoom` (287:
   the focus moves toward the anchor by the applied ratio, so the anchor
   keeps its pixel; the radius clamps to 0.05–40 × the drawn bodies'
   extent, `scene.rs:45`). Any gesture stops a glide or spin (`interrupt`
   129).
3. No job, no document leg: display only. `orbit::place` (411) places the
   camera; `cad/view.rs:update` (134) keeps picks on what is drawn.
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD's counterpart: `ui/viewport.py:mouseMoveEvent`
   (1480-1512: right-drag orbits at 0.4°/px, `Camera.orbit` 79-86, pitch
   within ±89.5°; middle or Shift+right pans by its world-per-pixel,
   `Camera.pan` 88-91) and `wheelEvent` (1553-1566: one step of 0.9 or
   1.1 per event toward `snap(…, suppress=True)`'s point, `Camera.zoom`
   97-103).
6. Shown: the 3D view turns, slides and zooms; `camera_state` (apply.rs:62,
   `camera/mod.rs:state_json` 437) reports the RoboCAD yaw and pitch too.
   Both keep the point under the cursor on its pixel while zooming (any
   anchor on the cursor's ray stays put when the eye scales about it). RoboCAD
   anchors on the active plane or the model's XY plane under the cursor
   (`viewport.py:1415-1435`), the viewer on the plane through the focus.

Deliberate difference (recorded): the zoom anchor's depth (the focus plane,
not RoboCAD's ground or active plane) and the wheel step
(`exp(−0.12 × lines)`, about 0.89 a line, where RoboCAD takes 0.9 per event
whatever its size), so the focus drifts differently while the point under
the cursor stays put in both; 0.007 rad/px is RoboCAD's 0.4°/px. By
reading, unexecuted.

### CAD-100 Named views
1. 1, 3, 7, 0 and Ctrl/Cmd+1, 3, 7 (the keypad's digits too, `normalise`
   210-221): key leg → `registry.rs` rows 319-325
   (`Native::Camera(CameraCmd::Preset(…))`); View ▸ View front … View iso
   (the same rows), and Space's view radial (`VIEW_RADIAL` 281: Front,
   Top, Right, Iso). REST `camera_view {"view":"front"}`. The shared
   numpad keys are off in CAD (`scene.rs:50`, `keys: false`), so a digit is
   read once.
2. `registry::camera` (635) pushes `CameraAction::View` → camera leg →
   `apply.rs:63-67` → `Orbit::preset` (`orbit.rs:169`: RoboCAD's yaw and
   pitch from `ViewPreset::robocad_degrees`, `camera/mod.rs:260-270`,
   through `robocad_to_display` 283, pitch within the rules' limit; focus
   and distance kept; a cut, and back to the turntable, `glide_to` 61-63).
3. No job: display only.
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/app.py:303-304` (`camera.set_view(n)`),
   `ui/viewport.py:Camera.set_view` (110-114: the same table, turntable).
6. Shown: the same side at the same distance and focus; Ctrl's views are
   the opposite sides. Gap found and fixed: the CAD camera opened with
   Bevy's 45° field of view and an arbitrary heading, where RoboCAD's
   camera starts at 40° and its iso heading (`ui/viewport.py:44-46`), so
   the same distance drew the model about 12 % smaller than RoboCAD's
   window; `cad/scene.rs:setup` now starts at RoboCAD's iso view and 40°
   (`ROBOCAD_FOV_DEG`, `scene.rs:56-57`, 62-77; the projection and the
   orbit agree from the first frame).

Matches RoboCAD. By reading, unexecuted.

### CAD-101 Focus selection
1. F (View ▸ Focus Selection, `registry.rs:318`,
   `Native::Camera(CameraCmd::Focus)`) → key leg → `registry::camera` (635)
   → `focus` (656).
2. No `CameraAction`: `focus` takes the shared selection's nodes
   (`cx.shared.items().nodes()`), adds every node under them (walk order
   lists parents first), and asks `CadMeshes::bounds` (`cad/mesh.rs:149`)
   of them; `CadMeshes::frame` (176) sets the framing request. With nothing
   selected it is `CadFit { id: None }` (Fit All, `actions.rs:578` →
   `fit` 691). Nothing drawn: refused by name ("nothing to frame: no body
   of … is drawn").
3. `cad/scene.rs:fit` (125, SimSync before Place) takes the request and
   `Orbit::frame_bounds` (`orbit.rs:231`): the bounds' centre at 3.2 × their
   half diagonal from the current heading, keeping a trackball.
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/viewport.py:focus_selection` (449-453) →
   `focus_nodes` (455-474: the nodes and every descendant's bbox) →
   `Camera.focus` (105-108: distance = half diagonal / sin(fov/2) × 1.1,
   which at RoboCAD's 40° is 3.22 × the half diagonal).
6. Shown: the part, or the group with its children, fills the view;
   everything with nothing selected.

Matches RoboCAD (3.2 × against 3.22 × at RoboCAD's default 40°). By
reading, unexecuted.

### CAD-102 Orthographic and field of view
1. 5 (View ▸ Orthographic, the radial's Ortho; `registry.rs:326`) → key
   leg → `CameraAction::Projection { orthographic: None }`. View ▸ Set field
   of view… (`registry.rs:336`, `CameraCmd::Fov`) → `registry::camera` →
   `cad/views/mod.rs:open_fov` (392): the Saved Views panel's "Field of
   view" entry opens filled with the camera's degrees (`panel::fov_text`),
   on the one kit field `views/panel.rs:VIEWS` (39). REST
   `camera_projection`, `camera_fov {"degrees":30}`.
2. Projection: `apply.rs:69-72` toggles `Orbit::orthographic`. The field's
   Enter: `views/panel.rs:input` (113) → `submit` (74): `evaluate(&FOV, …)`
   (94; `FOV` 48: 5–120, one decimal), refused under the field naming the
   range; else `Act<CameraAction>::Fov` → `apply.rs:73-77` (`fov` 23 refuses
   outside 5–120 by name).
3. `Orbit::projection` (`orbit.rs:336`): perspective keeps the mode's near
   plane; orthographic is `2 × radius × tan(fov/2)` high, so the model keeps
   its size on screen; `place` writes it (433-435).
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/app.py:toggle_ortho` (1034-1036),
   `set_fov` (1059-1063: `QInputDialog.getDouble(…, 5, 120, 1)`),
   `ui/viewport.py:Camera.projection` (128-131: half height
   `distance × tan(fov/2)`).
6. Shown: no perspective in orthographic, the same size; 30° narrows it.
   The entry sits in the floating panel at the lower right instead of a
   dialog (recorded with the Saved Views dock).

Matches RoboCAD. By reading, unexecuted.

### CAD-103 Trackball
1. View ▸ Toggle orbit: turntable / trackball (`registry.rs:335`,
   `CameraCmd::OrbitMode`) → key leg (menu) → `CameraAction::OrbitMode {
   mode: None }`; REST `camera_orbit_mode`, `camera_state` (`mode` is
   `trackball` non-null).
2. `apply.rs:133-141` → `Orbit::set_trackball` (`orbit.rs:302`): on, the
   trackball starts from the current rotation; off, `sync_turntable` (310)
   takes the turntable heading nearest the current view.
3. A right-drag then rotates the trackball (`rotate_by` 251-262: about the
   view's own up and right axes), so the model can roll past upside down.
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/app.py:toggle_orbit_mode` (1050-1057:
   `sync_trackball`, mode "trackball"; back, mode "turntable"; either
   way status "Orbit: {mode}"), `Camera.orbit`'s trackball branch
   (`ui/viewport.py:80-84`).
6. Shown: the model tumbles freely in both. Back in turntable RoboCAD
   returns to the yaw and pitch it had before the trackball (they were
   never changed), the viewer to the nearest upright heading of the view as
   it is. The status line says "Orbit: trackball" / "Orbit: turntable", the
   mode switched to (`cad/surfaces/registry.rs:camera`, 638-645 → 644: the
   opposite of this frame's camera snapshot, `CadViews::camera`), as
   RoboCAD's `self.status(f"Orbit: {cam.mode}")` (`ui/app.py:1057`).

Deliberate difference (recorded): leaving the trackball keeps the view's
direction (nearest upright heading) rather than jumping back to the
heading from before it. Gap found and fixed (this batch): the viewer wrote
no "Orbit: …" status line. By reading, unexecuted.

### CAD-104 View cube
1. The cube net at the top right of the 3D view (`cad/display/ui.rs:cube`
   224-242: Top; Left, Front, Right; Iso, Bottom, Back as kit segment
   buttons carrying `CubeButton`), lit on the face the camera looks at
   (`facing` 77 = RoboCAD's `view_cube_hit` at the cube's centre, through
   `cube_face` 63). The **Cube** chip (`toolbar` 200,
   `DisplaySetting::ViewCube`) hides and shows it.
2. `ui.rs:cube_press` (99, `ViewerSet::Input`) → `cube_action` (91):
   `CameraAction::View`, or `CameraAction::Opposite` when the turntable
   heading already is that face's (within 1e-3 rad, pitch clamped as the
   camera clamps it; never for Iso) → camera leg → `apply.rs:63-68`
   (`Orbit::opposite`, `orbit.rs:177`: yaw + 180°, pitch negated).
3. No job: display only.
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/viewport.py:mousePressEvent` (1458-1468:
   a cube hit sets the view, or `camera.opposite()` when the camera is
   exactly at that face), `view_cube_hit` (1183-1196), `_draw_view_cube`
   (1134).
6. Shown: Front then the back (RoboCAD's opposite); Iso; the lit face
   follows the view; `cad_state.display.view_cube`.

Deliberate difference (recorded): the cube is a net of buttons, not a
shaded 3D cube; a click picks a face by its button, not by the pixel's
direction. By reading, unexecuted.

### CAD-105 Display modes
1. Z (`registry.rs:328`, `DisplayCmd::Next`), View ▸ Display: shaded …
   Display: render (329-334), the display panel's six segment buttons
   (`ui.rs:toolbar` 187-192, `CadButton(CadDisplay { mode })`), Inspect ▸
   Normal-direction shading (446: xray). REST `cad_display
   {"mode":"wireframe"}`, `{"next":true}`; `system_ui`
   `cad:display:mode_*`, `cad:display:next` (`display/mod.rs:controls_of`
   579-582).
2. Display leg: `apply_display` (338): `DisplayMode::next` (94, RoboCAD's
   `MODES` order, wrapping) or the mode given.
3. `draw::materials` (138): each body's material swapped for the mode's
   derived copy (`derive_material` 101: xray 0.35 alpha blended, wireframe
   fully transparent and still pickable, matcap the clay tint), back to the
   body's own in shaded, shaded_edges and render; `edges_sync` (215) and
   `lines` (333, `Layer::Edges`) draw RoboCAD's sampled B-rep edges in
   0.08, 0.08, 0.1 while `DisplayMode::edges` (99); `lights` (632) adds
   render's fill and back lights.
4. Edges not held by `CadTopology`: `GET /nodes/{id}/edges?samples=24`
   (`crates/sim-runtime/src/cad_client/mod.rs:edges` 323) on
   `Pool::Dedicated` jobs (`draw.rs:fetch_edges` 199), at most 2 at once,
   dropped on a new revision.
5. RoboCAD: `api.py:1644-1645` (edges). Its window: `ui/app.py:1042-1048`
   (`next_display_mode`, `set_display_mode`), `ui/viewport.py:MODES` (260)
   and its draw passes (761-786 edges, 879-904 render).
6. Shown: the same order and look for shaded, shaded with edges, wireframe
   and xray; `cad_state.display.mode`.

Deliberate difference (recorded): matcap is approximated (clay tint, no
sphere image) and render has no ground shadow. By reading, unexecuted.

### CAD-106 Grid
1. Ctrl/Cmd+G (`registry.rs:327`, `DisplayCmd::Grid`), View ▸ Grid, the
   **Grid** chip (`ui.rs:195`), the radial's Grid. REST `cad_display
   {"toggle":"grid"}`.
2. Display leg: `apply_display` (338-378) flips `CadDisplay::grid`.
3. `draw::lines` (333) rebuilds `Layer::Grid` and `Layer::Axes` (retained
   gizmos under the Z-up root, only when their inputs change): `draw_grid`
   (471-486: ±20 steps of `GRID_STEP_MM` 10 mm on z = 0, every 5th line
   major, minor ones 0.7 ×, high contrast's colours), the axes (red X and
   green Y 200 mm, blue Z 100 mm), all cut by the section plane when it is
   on (`strips`).
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/app.py:toggle_grid` (1038-1040),
   `ui/viewport.py:_draw_grid` (590-618: the same step, count, major rule,
   colours and axes).
6. Shown and hidden together; `cad_state.display.grid`.

Matches RoboCAD. By reading, unexecuted.

### CAD-107 Build plate and overhangs
1. Ctrl/Cmd+Shift+B (Print ▸ Build Plate Preview, `registry.rs:341`), the
   **Plate** chip (`ui.rs:196`). REST `cad_display {"toggle":"build_plate"}`.
2. Display leg: `apply_display` (372-377): toggling the plate sets
   `overhangs` to the plate's new state, as RoboCAD's.
3. `draw::quads` (572): the 220 × 220 mm plate (`BUILD_PLATE_MM`,
   `display/mod.rs:181`) at z = −0.05; `section::preview` (367) builds the
   overhang tint on `Pool::Compute` (`derive_preview` 283 → `overhangs`
   225: triangles facing down more than 45° from horizontal, RoboCAD's
   0.9, 0.35, 0.3), never changing the body's mesh.
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/app.py:toggle_build_plate` (1072-1077:
   `show_overhangs = build_plate is not None`), `ui/viewport.py:_draw_build_plate`
   (622), `overhang_threshold` 45 (293).
6. Shown: the plate and the same downward faces in red; off again.

Matches RoboCAD. By reading, unexecuted.

### CAD-108 High contrast
1. View ▸ High-Contrast Theme (`registry.rs:342`), the **Contrast** chip
   (`ui.rs:199`). REST `cad_display {"toggle":"high_contrast"}`.
2. Display leg flips `CadDisplay::high_contrast`.
3. `draw::lights` (632): the 3D view's clear colour becomes RoboCAD's
   0.98, 0.98, 0.99; `lines` redraws the grid in high contrast's colours
   and the edges black (`EDGE_CONTRAST`).
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/app.py:toggle_high_contrast` (1085-1093:
   kept in QSettings, its stylesheet swapped too), `ui/viewport.py:533`,
   599-600, 764.
6. Shown: the 3D view turns light with a lighter grid and black edges.

Deliberate difference (recorded): only the 3D view changes (the kit's
panels keep their colours) and the setting is not kept after a restart. By
reading, unexecuted.

### CAD-109 Section preview
1. Ctrl/Cmd+Shift+X (Inspect ▸ Section Analysis, `registry.rs:340`,
   `DisplayCmd::Section`), the **Section** chip (`ui.rs:198`). REST
   `cad_section {}`; `system_ui` `cad:section:toggle`.
2. Display leg: `apply_section` (384) with nothing given toggles (392-400);
   turned on without a plane it starts on `default_plane` (463: XZ, normal
   −Y, through the drawn bodies' bounds centre in Y, 0 with nothing drawn).
3. `section::preview` (367, SimSync): each drawn body shows a clipped copy
   of RoboCAD's tessellation built on `Pool::Compute` (`clip` 146: kept
   where `distance ≤ 0`, the side the normal points to removed, as
   RoboCAD's clip plane), with its cut outline (`segments` 199) drawn in
   RoboCAD's red by `lines` (`Layer::Outline`); picks go through the shown
   copy and skip the removed side (`cad/pick.rs:375`, `candidates_at` 343,
   `search_for` 266).
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/app.py:toggle_section` (1065-1070) →
   `ui/tools.py:SectionTool.activate` (1162-1170: `Plane.xz` through the
   bounds centre in Y), `ui/viewport.py:538-541` (`glClipPlane` with the
   negated normal), `_draw_section_outline` (941).
6. Shown: the same side cut away and outlined in red; hovering and picking
   never select the removed part; the same key turns it off.

Matches RoboCAD. By reading, unexecuted.

### CAD-110 Section plane
1. With the section on, the toolbar's **X**, **Y**, **Z** chips
   (`ui.rs:202-209`: `CadSection { axis, offset }` through the drawn bodies'
   centre) and **Rotate** (210); the offset field ("offset, e.g. 5 or 2
   cm", `ui.rs:215`) on the one kit field `display/entry.rs:SECTION` (28):
   a press focuses it at "0" selected (`entry::input` 70, `CadKeySet::Focus`),
   Enter → `offset_action` (59: a unit expression in mm; `abc` refused under
   the field "Offset: …"; 0 sends nothing). REST `cad_section {"offset":5}`,
   `{"axis":"z","offset":10}`.
2. Display leg: `apply_section` (401-419): `SectionPlane::on_axis`
   (`section.rs:89`), `moved` (105: along the unit normal), `rotated` (110:
   90° about Z, `Plane.from_normal`'s x axis).
3. The preview rebuilds for the new plane (`preview`'s `Key` includes the
   plane); the plane quad (`quads`, `plane_transform` 561).
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/tools.py:SectionTool` (1158-1200: Tab's
   `NumericField("offset")` commit, the drag along the normal 1177-1187, R
   1195-1198).
6. Shown: the same cuts for the same planes and offsets; Rotate turns the
   plane as R does.

Deliberate difference (recorded): R stays the Rotate tool, Tab the numeric
bar, and a left drag on the plane box-selects (or Alt-orbits) rather than
moving it (`entry.rs` module doc 11-18). By reading, unexecuted.

### CAD-111 Exact section
1. `system_ui` `cad:section:exact` ("Exact section of …",
   `display/mod.rs:controls_of` 599-618; ready only with the section on, a
   node selected, connected and a plane RoboCAD's route takes) or REST
   `cad_section {"exact":"<id>"}`.
2. Display leg: `apply_section` (420-443): the node must be in the shown
   tree; `exact_query` (449): a named plane through the origin
   (`SectionQuery::named`) or the active plane node when it is the same
   set, else refused naming why; the request is keyed by (node, RoboCAD's
   revision, plane, query).
3. `section::exact_jobs` (510, JobResults): one `jobs::Latest` read on
   `Pool::Dedicated` (549); an answer for a superseded request is dropped
   (`accept` 503); a document change cancels it (512-521); while the
   section stays on the request's plane a new revision re-reads it
   (530-536).
4. `crates/sim-runtime/src/cad_client/section.rs:CadClient::section` (103:
   `GET /nodes/{id}/section?plane=xy|xz|yz|<plane id>`, read tolerantly,
   `section_from_value` 78).
5. RoboCAD: `api.py:1652-1653` → `Service.section` (830) →
   `analysis.section_outline` (95: OCCT's section, each edge sampled).
6. Shown: the yellow exact outline over the red preview outline
   (`draw.rs:lines`, `Layer::Exact`, only `ExactSection::drawn`:
   `display/mod.rs:144-148`, for the request at the shown revision on the
   current plane); `cad_state.display.section.exact` (pending, then the
   result or RoboCAD's error).

Matches RoboCAD's route (its window has no exact section to compare). By
reading, unexecuted.

### CAD-112 Isolate
1. `/` (View ▸ Isolate, `registry.rs:337`; the viewport's right-click menu,
   `CONTEXT` 264; the outliner's context menu, `cad/tree/controls.rs:123`)
   → key leg → the catalogue entry `view.isolate`
   (`cad/ops/catalogue/view.rs:10-21`: `ANY_NODES`, refusal "Select the
   nodes to isolate").
2. `ops/mod.rs:run` (528) → `prepare` (543: `commit_refusal`, the
   selection against the entry's needs) → `start` (584) → edit leg with
   one call.
3. Edit leg: one `RoboCAD edit: Isolate` job.
4. `crates/sim-runtime/src/cad_client/mod.rs:op` (374: `POST /ops/isolate
   {"args": [ids]}`).
5. RoboCAD: `api.py:1668-1671` → `Service.op` (1011) →
   `commands.py:isolate` (402-412: every node but the selected, their
   descendants and ancestors hidden, one `SetAttributes("Isolate")`).
   Its window: `ui/app.py:312` (`ops.isolate(selection.nodes())`).
6. Shown: the refetched tree's visibility, the hidden bodies gone from the
   3D view; Ctrl/Cmd+Z → `CadUndo` (`actions.rs`) undoes the one step.

Deliberate difference (recorded): with nothing selected the viewer refuses
"Select the nodes to isolate" where RoboCAD hides everything. By reading,
unexecuted.

### CAD-113 Hide and Show All
1. H (View ▸ Hide, `registry.rs:339`, right-click ▸ Hide) and Alt/Option+H
   (View ▸ Show All, 338: the physical key with Alt held, `keys.rs`
   module doc) → key leg → catalogue entries `view.hide`
   (`catalogue/view.rs:31-41`: `set_visible(ids, false)`) and
   `view.show_all` (22-29).
2. `ops::run` → `prepare` → `start` → edit leg, one call each.
3. Edit leg: one job each.
4. `CadClient::op` (`crates/sim-runtime/src/cad_client/mod.rs:374`):
   `POST /ops/set_visible`, `POST /ops/show_all`.
5. RoboCAD: `api.py:1668-1671` → `Service.op` (1011) →
   `commands.py:set_visible` (336-337: one `SetAttributes("Hide")`) and
   `show_all` (415-416: `SetAttributes("Show all")` over every node). Its
   window: `ui/app.py:313-314`.
6. Shown: the tree's eye column and the 3D view follow the refetched
   revision; each undo reverses one step.

Deliberate difference (recorded): Hide with nothing selected is refused
"Select the nodes to hide" (RoboCAD pushes an empty step). By reading,
unexecuted.

### CAD-114 Save a view
1. View ▸ Saved Views (`registry.rs:296`, `Do::SavedViews` 123:
   `CadViews { op: panel, open: true }`) opens the floating panel
   (`cad/views/panel.rs:draw` 229, `body` 294). The name field ("View name,
   e.g. Worm drive cutaway", 310) is the one kit field `VIEWS` (39);
   **Save current view** (311) is the `cad:view:save` control's
   `CadButton`, enabled only for a 1–120 character name, an edit that can
   be sent and a placed camera (`views/mod.rs:controls_of` 511); Enter in
   the field submits through `panel::submit` (74-83). REST `cad_views
   {"op":"save","name":"Front cutaway"}`.
2. `views/mod.rs:handle` (`ViewsOp::Save` 301): `check_view_name` (302;
   `crates/sim-runtime/src/cad_client/views.rs:222`, RoboCAD's
   `_view_name`), `CadViews::capture` (171) of the native camera
   (`snapshot` 466) and display through `convert::capture`
   (`convert.rs:115`), checked as RoboCAD checks it (`ViewState::check`,
   `cad_client/views.rs:159`), all before anything is sent; the typed name
   stays until RoboCAD answers (`saving` 314, `settle_save` 406).
3. Edit leg: one job.
4. `CadClient::save_view` (`cad_client/views.rs:267`: `POST /views {name,
   state}` with exactly the schema's twelve keys).
5. RoboCAD: `api.py:1727-1729` → `saved_view_request` (1126-1133) →
   `saved_views.py:SavedViewOps.save_view` (107-114: `validate_state`, one
   `ChangeSavedViews('Save view')`). Its window: `ui/saved_views.py:save_current`
   (85-90).
6. Shown: the list refetched at the new revision (`views::sync` 437, one
   `GET /views` job per (generation, revision)): "Front cutaway /
   Orthographic · Cutaway" (`convert::details` 190); the feedback line
   "Saved: Front cutaway" (`settle_save`), else the panel's
   "Saved inside this CAD file · edits support Undo" (339).

Matches RoboCAD. Deliberate difference (cosmetic, recorded): RoboCAD
selects the saved row; the viewer marks a row current only on restore. By
reading, unexecuted.

### CAD-115 Rename, replace, delete
1. On a row (`panel.rs:322-337`): **Rename…** (334) opens the row's own
   field on the kit field `VIEWS` (`ViewField::Rename`), Enter → `submit`
   (84-92: unchanged names send nothing); **Replace with current** (333,
   `cad:view:replace-<id>`) and **Delete** (335, `cad:view:delete-<id>`)
   are `CadButton`s. REST `cad_views {"op":"rename"|"replace"|"delete",
   "id":…}`.
2. `views/mod.rs:handle` arms `Rename` (319), `Replace` (332: the camera
   and display captured as for a save), `Delete` (344); edit leg.
3. Edit leg: one job each.
4. `CadClient::update_view` (`cad_client/views.rs:273`: `PATCH /views/{id}`
   with only `name` or only `state`), `delete_view` (282: `DELETE
   /views/{id}`).
5. RoboCAD: `api.py:1727-1729` → `saved_view_request` (1144-1152) →
   `SavedViewOps.update_saved_view` (`saved_views.py:116-122`, "Update
   saved view") and `delete_saved_view` (124-127, "Delete saved view").
   Its window: `ui/saved_views.py:replace_current` (101-105), `rename`
   (107-112), `delete` (114-118).
6. Shown: the list at the new revision; the status line "Renamed saved
   view …", "Updated: … · Undo to revert", "Deleted saved view … · Undo to
   restore"; Ctrl/Cmd+Z undoes each.

Matches RoboCAD. By reading, unexecuted.

### CAD-116 Restore, across both
1. **Restore view** on a row (`panel.rs:332`, `cad:view:<id>`); REST
   `cad_views {"op":"restore","id":…}` (waits for the list at the current
   revision, `views/mod.rs:wait` 202).
2. `handle` (`ViewsOp::Restore` 282) → `restore` (357): the state checked
   whole first (`convert::camera_of` 142, `apply_display` 168), then
   `CameraAction::Set` (a cut, 362) and the display's grid, mode, comment
   pins and section; `views.selected` and the feedback "Showing: …", status
   "Restored view: …".
3. Camera leg (`apply.rs:155-165`: pitch clamped to the rules, the
   trackball's rotation from `rot` when the view's mode is trackball).
4. The list is `CadClient::views` (`cad_client/views.rs:254`: `GET /views`),
   read by `views::sync` (434); no call at restore (RoboCAD's
   `POST /views/{id}/restore` is GUI-only and moves its own camera).
5. RoboCAD: `api.py:1137-1143` (restore, GUI only),
   `ui/saved_views.py:restore` (92-99: `comments.end_inspection()` first),
   `saved_views.py:restore_view` (65-80: clears `inspection_ids`).
6. Shown: the same direction, distance, projection, field of view, display
   mode, grid and section; a view saved in either restores in the other
   (the same twelve keys, `VIEW_STATE_KEYS`, `cad_client/views.rs:28`).
   Gap found and fixed: RoboCAD's restore first ends a comment thread's part
   view (the parts shown alone come back, the selection from before
   returns); the viewer's restore left cad-organize's "Show only linked
   parts" isolation on, so the other parts stayed hidden after the saved
   view was applied. `views/mod.rs:end_part_view` (375-388, called from the
   restore arm at 295-298) now ends it after a successful restore (the
   selection from before, without parts deleted since, through the one
   Selection and `selection::publish`), leaving the camera and display to
   the saved view as RoboCAD's `restore_view` after `end_inspection` does;
   the answer says `returned_to_assembly`.

Deliberate difference (recorded): restore is a button per row, not a
double-click. By reading, unexecuted.

### CAD-117 Tessellation tolerance
1. Select a curved body; the inspector's "Tessellation tolerance (mm)"
   row (`cad/inspector/editors.rs:373`, `EditKey::Tessellation`), typed on
   the inspector's kit field; Enter → `enter` (395-398) → `patch_for` (296,
   304-306) → `evaluate` (279-289: 0.005–2 mm, three decimals, refused by
   name outside) → `CadAction::CadPatch { "tessellation_tolerance": mm }`.
2. `actions.rs:handle` arm 506 → edit leg (`Patch …: tessellation_tolerance`).
3. Edit leg: one job.
4. `CadClient::patch` (`crates/sim-runtime/src/cad_client/mod.rs:294`:
   `PATCH /nodes/{id}`).
5. RoboCAD: `api.py:1630-1631` → `Service.patch` (748: 774-775 sets it,
   785 pushes one `SetAttributes("Set attributes")`). The new revision
   refetches the body's mesh at the node's own tolerance
   (`cad/mesh.rs:468`, `NODE_TOLERANCE` 0 =
   `crates/sim-runtime/src/cad_client/mod.rs:130`; `api.py:1648-1649` →
   `document.py:mesh_of` 471-475). RoboCAD's panel: `ui/widgets.py:469-476`
   (the spin box, 0.005–2, three decimals), `_tol_changed` (730-735: sets
   every selected node directly, no undo).
6. Shown: coarse facets at 0.5, smooth at 0.01; one undo step "Set
   attributes"; the field opens empty (RoboCAD's `node_summary`,
   `api.py:110-111`, has no `tessellation_tolerance`).

Deliberate difference (recorded): the viewer's change is one undo step on
the inspected node, RoboCAD's panel records none; the field opens empty. A
stale citation was fixed (this batch): `cad/inspector/editors.rs:42` now
cites `api.py:774-775 and 782-785` for the patch. By reading, unexecuted.

### CAD-118 File ▸ New
1. File ▸ New (Ctrl/Cmd+N, `registry.rs:299`, `Do::File("file.new")` →
   `files::command_action` 605) → `files/mod.rs:handle` (275) → `file`
   (300) without a path → path form proposing `{dir}untitled.rcad`
   (`form.rs:123`, `start_dir` 237: the document's folder). The form shows
   `cad_open`'s rule as it stands (`open_rule` 649: `switch_blockers` in red,
   an attached RoboCAD's `leaving_note` in amber) and "… exists: RoboCAD
   refuses to replace it" when the listing has that name (`footer` 693-694).
   REST `cad_file {"op":"new","path":…}`; `system_ui` `cad:file:new`.
2. `file` arm `FileOp::New` (335): an absolute `.rcad`; the rule checked
   before RoboCAD writes anything (341-344: "Not creating …: …"); file job
   leg (`jobs::start` with `Then::Open`).
3. File job leg; on success `receive` writes `CadFile { op: open }` for a
   click (`jobs.rs:250-252`), or a REST caller's `wait` opens it itself
   (`open_created` 204), so its answer names `created` and `opened`.
4. `crates/sim-runtime/src/cad_client/files.rs:CadClient::new_file` (158:
   `POST /new {path}`).
5. RoboCAD: `api.py:1738-1740` → `Service.new_file` (1321-1348: creates the
   file exclusively, 409 "… exists: choose a new file name"). Its window:
   `ui/app.py:283` (`MainWindow().show()`: an untitled window).
6. Shown: "Create …" in the strip and status line, then "Created …;
   opening it" and the open's "Opening …" (CAD-119); an existing path is
   RoboCAD's 409 named ("Create …: RoboCAD answered 409: … exists: choose a
   new file name"), the file untouched.

Deliberate difference (recorded): New names its file first and opens it in
this window under the open rule; RoboCAD opens an untitled window. A gap is
recorded for the lead: the open that follows stats the file on the UI
thread (`actions.rs:633`, `p.is_file()`, see CAD-119). By reading,
unexecuted.

### CAD-119 File ▸ Open
1. File ▸ Open… (Ctrl/Cmd+O, `registry.rs:300`) → `file` without a path →
   the path form on the document's folder; its listing shows the folder's
   `.rcad` files and subfolders (`form.rs:extensions` 306,
   `path_field::listing_key` 119 → `request` 130 on `Pool::Io`, `list`
   94); a folder row descends, `..` goes up (`FileForm::pick` 329, `up`
   335, `path_field::pick` 147, `up` 160), a file row fills the path. OK →
   `CadFile { op: open, path }`. REST `cad_open`, `cad_file {"op":"open"}`.
2. `file` arm `FileOp::Open` (325-333): `absolute` (247: `~/` expanded; a
   relative path refused "… is not an absolute path (RoboCAD would resolve
   it against its own working directory)") → `CadOpen` →
   `actions.rs:handle` arm 481 → `open` (624): a `.rcad`, the file exists
   (633), `switch_blockers` (644), the old document's service released, a
   new `CadDocument` started (`sync::start`), the registry's CAD entry
   follows.
3. The new document's connect job (`sync/mod.rs:116-127`: a self-started
   headless service on the file).
4. `CadClient` health and `/doc` polls of the new service.
5. RoboCAD: the headless service started on the file (no `POST /open`,
   which opens another window, `api.py:1746`, `Service.open`). Its window:
   `ui/app.py:open_file` (1332-1335) → `open_path` (1326) → the loader.
6. Shown: the left dock names the new document; the form closes
   (`close_unless_refused` 431); a refusal stays in the form.

Matches RoboCAD (the same document opens). Gap recorded, not fixed (a
shared file): `cad/actions.rs:open` stats the path on the UI thread
(`p.is_file()`, 633; its comment calls it a known cost); the existence check
belongs in the connect job (`sync::start`'s self-start already fails on a
missing file) or a `Pool::Io` read before the switch. By reading,
unexecuted.

### CAD-120 File ▸ Save As
1. File ▸ Save As… (Ctrl/Cmd+Shift+S, `registry.rs:302`) → the path form
   on `{dir}{stem}.rcad` (`form.rs:124`); OK → `CadFile { op: save_as }`.
   REST `cad_file {"op":"save_as"}` (and `cad_save {path}`, `actions.rs:540`).
2. `file` arm `FileOp::SaveAs` (356-364): absolute, `.rcad` appended →
   `files::save` (406): edit leg, the edit marked to retarget the window
   (423).
3. Edit leg: one job with `FILE_TIMEOUT` (414); `finish_edit` retargets a
   self-started document's file on success (`sync/mod.rs:604-610`).
4. `CadClient::save_with_thumbnail` (`cad_client/files.rs:152`: `POST
   /save/thumbnail {path}`).
5. RoboCAD: `api.py:1736-1737` → `Service.save_with_thumbnail` (1296-1319:
   headless, the snapshot renderer's 256 × 192 PNG; with a window,
   `MainWindow.thumbnail`). Its window: `ui/app.py:save_as` (1344-1350:
   `.rcad` appended, `doc.save(p, thumbnail=…)`), `thumbnail` (1352-1362).
6. Shown: "Saved … with its thumbnail" (or "without a thumbnail: RoboCAD
   could not draw one"); the left dock names the new file; leaving CAD mode
   reopens it (`app/switch/leave.rs:leave_cad` 93-109 uses `doc.target`).

Matches RoboCAD. By reading, unexecuted.

### CAD-121 Import STEP
1. File ▸ Import… (Ctrl/Cmd+I, `registry.rs:303`) → the path form (the
   folder listing filtered to `IMPORT_EXTENSIONS`, `form.rs:308`); OK →
   `CadFile { op: import, path }`. REST `cad_file {"op":"import"}`.
2. `file` arm `FileOp::Import` (366-379): `import_args` (442: an extension
   RoboCAD imports; no unit for STEP) → edit leg.
3. Edit leg: one job with `FILE_TIMEOUT` (377).
4. `CadClient::import` (`cad_client/files.rs:147`: `POST /import {path}`).
5. RoboCAD: `api.py:1752-1753` → `Service.import_file` (1424-1438:
   `importers.import_step`, `io/importers.py:27`). Its window:
   `ui/app.py:import_path` (1369-1390, the same importers, then
   `viewport.focus_all()`).
6. Shown: "Imported print-kit.step: n new node(s)"; the new nodes in the
   refetched tree; one undo removes them.

Deliberate differences (recorded): an SVG or image lands on XY (RoboCAD's
window uses the active plane, images as references); RoboCAD frames
everything after an import (`focus_all`, `ui/app.py:1390`), the viewer
leaves the camera where it is (Home or F frames it). By reading,
unexecuted.

### CAD-122 Import a mesh with units
1. The import form, path `…/cap.stl`: the "Units of the mesh file" row
   appears (`form.rs:rows` 201-203, empty) and `form::input` asks RoboCAD's
   guess once per path (`ask_guess` 275, 590-594: `CadFile { op:
   guess_unit }`); OK is disabled saying why until a unit is guessed or
   chosen (`unit_missing` 287, `ok_ready` 395); **Guess unit** (685) asks
   again; a unit chosen by hand is kept over a later guess (`set` 256-258,
   `guessed` 296).
2. `file` arm `FileOp::GuessUnit` (381-394: a mesh path) → file job leg
   (a read, not `complete_on_drop`, `Then::Guess`); `receive` fills the form
   (`jobs.rs` `Then::Guess` arm). OK → `FileOp::Import` with the unit →
   edit leg.
3. Guess: file job leg; import: edit leg.
4. `CadClient::mesh_units` (`cad_client/files.rs:164`: `GET
   /import/units?path=`), then `CadClient::import` (147: `{path, unit}`).
5. RoboCAD: `api.py:1741-1743` → `Service.mesh_units` (1350-1366:
   `importers.load_mesh_file`, `mesh_units_guess`, `io/importers.py:135`),
   then `api.py:1752-1753` → `import_file` → `importers.import_mesh` (162).
   Its window: `ui/app.py:import_path` (1377-1384) → `UnitsDialog`
   (`ui/widgets.py:897`) with the guess preselected.
6. Shown: "Asking RoboCAD for its guess…", then "RoboCAD's guess: …
   (largest extent …)" (`footer` 674-686); the mesh imported at the unit
   picked; never in a unit nobody chose or RoboCAD guessed.

Deliberate difference (recorded): the unit is a row of the path form, not
a second dialog. By reading, unexecuted.

### CAD-123 Export STL, 3MF, OBJ with settings
1. File ▸ Export… (Ctrl/Cmd+E, `registry.rs:304`) → `export` (473) without
   format or path → `open_form` (459) → the export form
   (`FileForm::new` 131-157: format STL first, every format's settings from
   the values last sent this session, else RoboCAD's defaults); OK →
   `CadExport { format, path, settings }` (`action` 361-378). REST
   `cad_export`.
2. `export` (473-503): the format, the path's extension
   (`formats::extension_fits` 234), `formats::settings` (212: each value
   checked with the desktop dialog's ranges, refused naming the setting,
   `check` 151); remembered per format (`export_settings`); file job leg
   (`complete_on_drop`).
3. File job leg; the strip's **Cancel** (or `cad_file {"op":"cancel",
   "job":n}`) → `jobs::cancel` (90).
4. `CadClient::export` (`crates/sim-runtime/src/cad_client/mod.rs:435`:
   `POST /export {format, path, settings, ids}` with `FILE_TIMEOUT`).
5. RoboCAD: `api.py:1750-1751` → `Service.export` (1390-1422:
   `exporters.export_stl`/`export_3mf`/`export_obj` with their settings
   dataclasses; 422 for an `ExportError`). Its window:
   `ui/app.py:export_path` (1399-1444, `ExportDialog` `ui/widgets.py:917`).
6. Shown: "Exported STL to … (n warning(s): …)"; `cad_state.files.last`;
   the reopened form starts from the last values sent.
   Gap found and fixed: an export could not be cancelled and the strip said
   only "runs to the end once sent". A sent export still runs to its end
   (api.py has no cancel route), so the strip now has a Cancel per export
   and render (`files/jobs.rs:strip` 283-347, its seconds updated in place
   on `StripLine` 272-276 so a press is never lost to a rebuild,
   `cad:file:cancel-<job>`,
   `files/mod.rs:control_list` 683-687, `FileOp::Cancel` 126-128 (`FileArgs::job` 167-169),
   `file` 306-317), `jobs::cancel` (90-107) says at once that RoboCAD
   writes the file anyway, and the outcome says so too ("…; the cancel did
   not stop it", `files/mod.rs:511-514`, `cancel_asked` in the answer); a
   job cancelled before its thread started sends nothing (`jobs/mod.rs`
   `Job::streaming`'s start check).

Deliberate difference (recorded): export settings are remembered for the
session, not across launches. By reading, unexecuted.

### CAD-124 Export STEP, IGES, sketch SVG
1. The export form with format `step` (Schema, Write names, Write
   colours), `iges` (no settings) or `svg` (Sketch (node id), filled with
   the first selected sketch, `export_context` 266-271); REST
   `cad_export`.
2. `export` (473): as CAD-123; the sketch SVG without a sketch is refused
   ("the sketch SVG needs settings.sketch …", `formats.rs:204`).
3. File job leg.
4. `CadClient::export` (`cad_client/mod.rs:435`).
5. RoboCAD: `api.py:1750-1751` → `Service.export` (1400-1410: STEP, IGES,
   sketch SVG); an `ExportError` is RoboCAD's 422, named
   ("Export STEP to …: RoboCAD answered 422: …", `jobs::named` 118). Its
   window: `ui/app.py:export_path` (1417-1438).
6. Shown: the status line and `cad_state.files.last`; the files RoboCAD
   wrote.

Matches RoboCAD. By reading, unexecuted.

### CAD-125 Export drawing
1. File ▸ Export drawing (SVG)… (Ctrl/Cmd+Shift+D, `registry.rs:305`,
   `files::command_action` 605: the export form on `drawing`): the four
   views checked, Title the document's file name, "Section A-A (the section
   tool's plane)" checked while the section is on (`form.rs:141-153`,
   `section_available`).
2. `export` (473) → `formats::settings` (212-231: views as a list, the
   section as the section tool's plane, refused when it is off).
3. File job leg.
4. `CadClient::export` (`cad_client/mod.rs:435`, format `drawing`).
5. RoboCAD: `api.py:1750-1751` → `Service.export` (1411-1416:
   `STANDARD_VIEWS`, `View("Section A-A", pl.normal, section=pl)`,
   `export_drawing_svg`). Its window: `ui/app.py:export_drawing`
   (1446-1456: the same four views, Section A-A while the section is on,
   the title `basename(doc.path or "untitled")`).
6. Shown: "Exported Drawing (SVG) to …".

Matches RoboCAD. By reading, unexecuted.

### CAD-126 Render
1. `system_ui` `cad:file:render` ("Render (PNG)…",
   `files/mod.rs:control_list` 678) → the render form (`form.rs:159-164`:
   view iso, 1200 × 900, shaded, edges on); REST `cad_render
   {"path":…,"view":"iso","w":1200,"h":900}`.
2. `render` (567-602): an absolute `.png` path, `render_request` (521: the
   view, mode, 16–8192 px, section and tolerance checked before anything is
   sent) → file job leg (`complete_on_drop`).
3. File job leg: the job asks RoboCAD, then writes the PNG itself (591), so
   the window never stalls; the strip's **Cancel** (`jobs::cancel` 90).
4. `CadClient::render` (`cad_client/files.rs:169`: `GET /render?…`,
   `RenderRequest::route` 106; an answer that is not a PNG is an error).
5. RoboCAD: `api.py:1730-1731` → `Service.render_request` (1157-1172:
   headless the snapshot renderer `render` 1189-1221; with a window the GPU
   viewport for a plain shaded view).
6. Shown: "Rendered … (n KB)"; a size outside 16–8192 px or a path without
   `.png` is refused first. Gap found and fixed: a render could not be
   cancelled. The PNG is this window's to write, so the job now checks the
   cancel before writing (`files/mod.rs:587-590`: "…: cancelled; RoboCAD
   drew the image … but nothing was written to …", an error, never shown as
   done) and says when the cancel came too late to stop the write (591-595); the
   strip's Cancel and `cad:file:cancel-<job>` ask it (CAD-123).

Matches RoboCAD's route (its window has no render command). By reading,
unexecuted.

### CAD-127 Unsaved edits
1. File ▸ Open… or New in a self-started service with an unsaved edit: the
   form's red line "Not now: … has unsaved edits in the RoboCAD service this
   window started …: save first" (`form.rs:open_rule` 649-658, drawn by
   `footer` 667-673); OK writes the action and is refused by name in the
   form; New is refused before RoboCAD writes anything
   (`files/mod.rs:341-344`). Attached to RoboCAD's window: the amber note
   "RoboCAD at … keeps the unsaved edits to …; this window then shows the
   other file" (`leaving_note`). REST `cad_open`, `cad_file {op: open|new}`
   follow the same rule.
2. Open → `actions.rs:open` (624: `switch_blockers` 644, "Not opening …");
   New → `file` 341.
3. `CadDocument::switch_blockers` (`cad/document/state.rs:206`): an edit in
   flight, a component rebuild, model exports and print jobs, and a
   self-started service's unsaved or unconfirmable edits (`unsaved`, from
   `health.dirty` after a fresh `GET /`); `leaving_note` (242).
4. `GET /` (`health.dirty`) through the poll worker.
5. RoboCAD: `ui/app.py:283-284` (New and Open open another window);
   closing: `closeEvent` (1920-1934: "Unsaved changes" / "Save before
   closing?").
6. Shown: the refusal names the document and what to do (save first); no
   discard button; after Save (Ctrl/Cmd+S) OK opens.

Deliberate difference (recorded): the viewer refuses rather than replace a
self-started service's unsaved edits; there is no discard answer. By
reading, unexecuted.

### CAD-128 The camera in the other modes
1. Robot (`robot/ui.rs:126-136`), Phenomena (`phenomena/scene.rs:59-69`),
   Build, Inspect and Lessons (`inspect_view/camera.rs:spatial_rules`
   16-29) spawn the shared camera with their own `OrbitRules`; right-drag,
   middle or Shift+right-drag and the wheel go through the same
   `camera/input.rs:navigate` (123); the numpad's 1/3/7 (Ctrl opposite), 9,
   5, 0, `.` and Home are `camera/input.rs:keys` (245-275, `keys: true` in
   these modes, never while a kit field types: `camera/mod.rs:485`). REST
   `camera_spin {"rate":0.5}`.
2. `drag_kind` (82-95) without `robocad_gestures`: middle or Shift pans,
   right orbits (Shift+middle still pans); arrows are not camera keys
   (`keys` 249: only with `robocad_gestures`). The wheel zooms toward the
   focus (`zoom_to_cursor` false), in a lesson card only with Ctrl/Cmd
   (`inspect_view/camera.rs:sync_camera` 46-66, `zoom_modifier`).
3. `orbit::place` (411): a home request frames with the mode's framing
   (the spatial view's overview glides, `glide_home`; Phenomena's fixed
   pose; Robot frames from the heading); spin (`step` 118-120) keeps going
   until a gesture (`interrupt`).
4. No `cad_client` call.
5. No RoboCAD call (compare with a build before the shared camera).
6. Shown: each mode's rate, pitch limit, zoom limits, home and spin as its
   rules state; a drag that starts on a side dock never moves the camera
   (`accepts` 56 with the mode's `ViewArea::Docks`).

No RoboCAD counterpart (the shared camera's other modes); matches the
modes' former feel by reading of the rules (not compared with a 1b00d789
build). By reading, unexecuted.

### CAD-129 RoboCAD's camera gestures
1. CAD with the Select tool: Shift+middle-drag, Alt+right-drag,
   Alt/Option+left-drag past 6 px, the arrow keys (Ctrl/Cmd: 90°,
   Shift: pan). REST `camera_orbit {"degrees":[10,0]}`.
2. `camera/input.rs:drag_kind` (82-95 with `robocad_gestures`):
   Shift+middle orbits, plain middle pans, Alt with a right orbit snaps
   (`Orbit::snap_to_axis`, `orbit.rs:268-282`, after every step as RoboCAD
   does, 181-185). Alt+left: noted at the press when CAD's gate allows it
   (`alt_left`, written by `cad/scene.rs:fit` 133-136 from `gate` 110-112:
   the Select tool, no catalogue interaction or command surface), an orbit
   once past `ALT_DRAG_SLOP` (65, 156-169, 187-195); `cad/pick.rs` (418-431)
   makes the same press never a box select (`alt_drag`) and keeps a shorter
   Alt+click the candidates menu (462-465). Arrows: `keys` (245) →
   `arrow_action` (226-236) → `apply.rs:93-109` (`rotate_by`) or `Pan`;
   not while a kit field has the keyboard (`camera/mod.rs:485`).
3. No job: display only.
4. No `cad_client` call.
5. No RoboCAD call. RoboCAD: `ui/viewport.py:mouseMoveEvent` (1480-1512:
   orbit on right without Shift, Alt+left, or Shift+middle; Alt with right
   snaps, `snap_orthographic` 116-120), `keyPressEvent` (1535-1551: 10°,
   Ctrl 90°, Shift pans `pan(-dx × 4, dy × 4)`).
6. Shown: the same turns, snaps and pans at the same steps
   (`orbit(-dx/0.4, dy/0.4)` is dx degrees of yaw, as `rotate_by`'s).

Deliberate difference (recorded in the ledger): RoboCAD's own Alt+left-drag
does not orbit (its tool takes the press, `ui/app.py:542`); the viewer's is
compared with right-drag. By reading, unexecuted.

### CAD-130 Curve nodes
1. Select a body, Modify ▸ Silhouette onto active plane (catalogue
   `tool.silhouette`, `cad/ops/catalogue/arrange.rs:128-136`) → key leg →
   `ops::run` → edit leg; the "Silhouette" curve node appears in the
   refetched tree. Visibility, selection in the tree, Z and the section as
   in CAD-105, CAD-109 and Part I.
2. `CadClient::op` (`POST /ops/silhouette`), edit leg.
3. `cad/display/draw.rs:edges_sync` (215-265): every visible `curve` node
   (`curve_nodes` 209: `effective_visible`) has its sampled edges fetched
   once per revision on `Pool::Dedicated` in every display mode
   (`fetch_edges` 199, 32 samples, `CURVE_SAMPLES` 79).
4. `CadClient::edges` (`crates/sim-runtime/src/cad_client/mod.rs:323`:
   `GET /nodes/{id}/edges?samples=32`).
5. RoboCAD: `api.py:1668-1671` → `commands.py:silhouette` (860); edges
   `api.py:1644-1645`. Its window: `ui/viewport.py:_curve_item` (1660-1672:
   colour `node.color or (0.35, 0.8, 1.0)`), `_draw_curve_item` (906-911: 2
   px, 1.0, 0.65, 0.2 while selected), its 8 px curve pick pass
   (1269-1278).
6. Shown: `draw::lines` `Layer::Curves` (440-449): 2 px, the node's colour
   or RoboCAD's light blue, orange while any item of the node is selected
   (`curve_color` 317), cut by the section plane (`strips`), gone while
   hidden; in every display mode.

Deliberate difference (recorded): clicking the curve in the 3D view does
not select it (RoboCAD's 8 px curve pick is not ported; select it in the
tree). By reading, unexecuted.

## Reading traces — annotations

Each trace follows one Part I comment step (CAD-176 to CAD-186) from the
native control to RoboCAD and back to what the Comments section, the pins
and the status line show, plus four cross-cutting traces. Everything here
is **by reading, unexecuted**: nothing was built, run or captured, and no
step was compared side by side. Native paths are under
`crates/sim-spatial/src/` unless they start with `crates/`; RoboCAD paths
are under `cad/robocad/`. The common legs are written out once:

- **Commit leg** (every thread edit): `cad/threads/ops.rs:handle` (167) →
  `ops.rs:commit` (68) → `annotations/mod.rs:apply` (219; `ThreadOp` →
  `ThreadCommand`, 175-193) → `cad/threads/source.rs:CadThreadSource::commit`
  (423; a known thread through `request_on`, 475) → `source.rs:send` (321)
  → `cad/edit.rs:edit_auxiliary_at` (65; refused by name with nothing sent
  by `cad/document/state.rs:commit_refusal_for`, 141, when another edit is
  in flight, the connection is down, or RoboCAD's revision moved since the
  list was read) → `InFlight::submitted` (`source.rs:333`,
  `annotations/mod.rs:107`) → the edit job runs `Request::send`
  (`source.rs:283`) → `crates/sim-runtime/src/cad_client/threads.rs`
  (`create_thread` 341, `update_thread` 351, `delete_thread` 355,
  `add_comment` 359, `update_comment` 367, `delete_comment` 371) → RoboCAD
  `api.py:1611-1613` → `api.py:annotation_request` (396-449) →
  `annotations.py:AnnotationOps` (create 233, update 251, delete 271, reply
  276, edit/delete message 284-305), each pushing exactly one
  `ChangeThreads` (190-212) on the command stack (`commands.py:212-219`),
  whose `apply` sets `dirty` and calls `notify("annotations")`, which moves
  `doc.revision` (`document.py:287-289`).
- **Answer leg**: `cad/sync/mod.rs:finish_edit` (531) →
  `cad/threads/mod.rs:edit_answered` (451; only this source's sequences,
  `InFlight::waits`, `annotations/mod.rs:115`) → `st.read.again()` (458;
  the read epoch moves) and the draft ends only if it is still the text
  sent (467) → the status line is the edit's message (`source.rs:324`:
  RoboCAD's "Annotation saved in document • Ctrl+S writes the file •
  Ctrl+Z undoes" after a post, reply or edit; "<label> · Ctrl+Z undoes"
  otherwise) → `sync/mod.rs:refresh` (607) asks the poll for `/doc`.
- **Re-read leg**: `cad/threads/read.rs:key` (59; generation, shown
  revision, epoch) → `read.rs:sync` (281; JobResults, after
  `CadSet::Results`) → `needs_work` (115) → `tick` (129) spawns one
  `Pool::Dedicated` job (155) calling `CadClient::threads` (160;
  `crates/sim-runtime/src/cad_client/threads.rs:328`, `GET /threads`,
  `annotations.py:216` with `thread_detail`, 106-121) → `listed` (145) →
  the dock (`cad/threads/dock.rs:draw`, 141; redrawn when `dock.rs:key`,
  124, changes) and the pins (`cad/threads/pins.rs:draw`, 132; rebuilt on
  `activation::render_key`, 150). While a newer key is read the old list
  stays shown with "Comments as read at revision R; reading revision N…"
  (`read.rs:line`, 86-101), and commits are refused by name until it lands
  (`ops.rs:began`, 41-64).

### CAD-176 Comments section and Annotate

1. View ▸ Comments panel: registry `view.comments`
   (`cad/surfaces/registry.rs:295`) → `Do::Organize` →
   `cad/threads/mod.rs:command_action` (421) → `ThreadsOp::Dock {open:
   true}` → `ops.rs:handle` (167) → the section is drawn (`dock.rs:draw`,
   141) and the re-read leg starts (`read.rs:wanted`, 105).
2. ＋ Annotate model (`dock.rs:155`, control `cad:threads:annotate`,
   `controls.rs:70`) or N (`registry.rs:294`, `cad/keys.rs` →
   `CadInvoke { tool.annotate }` → `command_action`, `mod.rs:424`) →
   `ops.rs:181` → `cad/threads/annotate.rs:start` (51): refused while a
   draft is open ("Post or cancel your current draft before placing
   another pin", RoboCAD's `begin`, ui/comments.py:326-331), unless only
   a stale pin is drafted (see "Remote refresh keeps the draft"); the
   Select tool replaces another tool; the status line shows RoboCAD's hint
   "Click a surface to place a comment • click a pin to read it • Esc
   cancels" (`mod.rs:77`, ui/comments.py:100), also drawn in the section.
3. The click: `annotate.rs:click` (160; Input) casts the cursor ray
   (`transform::ray_hit`) on a left **press** over the 3D view and reads
   the face only through `CadMeshes::face_at` at the shown revision (201);
   nothing hit: the status line says "Click a visible surface to place the
   annotation" (197, `MISSED`, 48; RoboCAD's ui/comments.py:121), the tool
   stays. A hit writes `cad_threads {op: place, node, point, face, view,
   revision}` (208).
4. `ops.rs:182` → `annotate.rs:place` (83): refused when the click's
   revision is not the shown one (90); the node must be in the shown tree;
   the pending pin is `Pending {node, point, face, view, revision}` (118),
   the composer gets the keyboard, the status line "Pin placed • write your
   annotation, then Post annotation" (122; RoboCAD's ui/comments.py:346).
   The "+" pin is drawn at the point (`pins.rs:106`).
5. Typing `Check this face`: the kit field `COMPOSE`
   (`cad/threads/input.rs:232`) mirrors into `ThreadsState::compose`
   (`input.rs:108-114`). **N while the composer is focused is typed**: the
   kit's `Typing` holds the keyboard, `cad/keys.rs:358` returns before any
   binding, and `input.rs:input` runs in `CadKeySet::Focus` (239), before
   CAD's keys.
6. Post annotation (or Enter, `EnterKey::ShiftNewline`, 232) →
   `input.rs:post` (76) → `controls.rs:submit_action` (21, `Create`) →
   `ops.rs:create` (384; a stale pin is refused here, 387) → the commit leg
   with `NewThread {node_id, point, face, view}` (`source.rs:444-453`) at
   the pin's revision → RoboCAD `api.py:418-424` →
   `annotations.py:create_thread` (233; `anchor`, 91-103, stores the
   point, the geometry stamp and the face's description), undo step "Add
   annotation".
7. Answer leg: `mod.rs:edit_answered` opens the new thread (473-480:
   filter All, current = RoboCAD's id) and ends the draft (467).
8. Shown: the list row "1 · <part>" with the first message as preview
   (`dock.rs:148`, `CadHost::heading` 88, `previewed` 93); the location
   line "<part> · Attached to surface" (`dock.rs:179-191`,
   `controls.rs:attachment` 40-48 = ui/comments.py:283); the pin "1" at the
   point, blue (`pins.rs:80-101`, `ATTACHED` 38).

### CAD-177 Reply with a part link

1. The thread is current (`ops.rs:open`, 363). A body selected in the
   tree is in the one `Selection` (through the CAD selection adapter,
   `cx.shared`).
2. Insert part link from selection (`dock.rs:250`, `controls.rs:100`) →
   `ops.rs:346` → `ops.rs:insert_link` (502): with nothing selected,
   "Select a part in the outliner or viewport first" (505; RoboCAD's
   ui/comments.py:463) in the status line (`cad/actions.rs:407-408`);
   otherwise `[label](part:ID)` per selected node, labelled as the thread
   names it or by the part's name, `[`/`]`/newline escaped
   (`crates/sim-runtime/src/cad_client/threads.rs:part_link`, 315; as
   ui/comments.py:460-473), appended to the draft; the composer takes the
   keyboard (`input.rs:179-187`).
3. ` needs a fillet`, Shift+Enter (a newline in the kit field), a second
   line; Reply or Enter (`input.rs:115-122`, `post`) →
   `controls.rs:submit_action` (26, `Reply`) → `ops.rs:211` → the commit
   leg: `ThreadCommand::AddComment` (`source.rs:485-489`) →
   `add_comment` → RoboCAD `api.py:434-436` →
   `annotations.py:add_comment` (276), undo step "Reply to annotation".
4. Answer and re-read legs; the reply is drawn with the link as the
   part's label (`ui_kit::threads::messages`, `dock.rs:241`; a deleted
   part's link reads "label (part deleted)", `dock.rs:237`, as
   ui/comments.py:57-68).

### CAD-178 Click the part link

1. The link in the message → `dock.rs:CadHost::link` (70; only for a node
   in the shown tree) → `ThreadsOp::PartLink` → `ops.rs:341` →
   `cad/threads/isolation.rs:part_link` (316).
2. `isolation.rs:view_parts` (215; called with highlight off, 316) captures
   the camera, the selection and the display once (229-232), frames the
   part and its descendants, shows them alone (display only:
   `cad/mesh.rs` hides the others; `isolation.rs:shown`, 66), and the
   status line says "Part view: <name>" (265; RoboCAD's
   ui/comments.py:423).
3. Then exactly the linked node is selected (317) through
   `isolation.rs:select` (132) → `cad/selection/mod.rs:handle` (104), the
   one path `cad_select {ids: [ID]}` takes. RoboCAD selects the node with
   its descendants (`highlight_parts`, ui/comments.py:396-400): recorded.

### CAD-179 Show on model

1. Return to assembly first (`ops.rs:340` → `isolation.rs:end`, 270),
   then Show on model (`dock.rs:199`, `controls.rs:89`) → `ops.rs:325`
   (refused by name only for another thread than an open draft's,
   RoboCAD's `select` rule as its `/threads/{id}/show` applies it,
   api.py:408-409; `ops.rs:not_drafting_other`, 141) →
   `isolation.rs:show` (146).
2. Surface thread: any isolation ends; the pin's part is selected through
   `cad/selection/mod.rs:handle`; the thread's saved camera (`thread.view`,
   from the list, not `GET /threads/{id}`) is merged over the current
   camera with mode "turntable" when absent (`camera_of_dict`, 106-118, as
   ui/comments.py:383-388) and pushed as `CameraAction::Set` (176-182);
   with no saved camera only a trackball camera is set, back to turntable
   (178); then the `inspection_view`; status "Showing the annotation on
   <part>" (186; RoboCAD shows none). The selection is pushed to RoboCAD
   as every `cad_select` is (`cad/selection/mod.rs:publish`, 132; `GET
   /selection` shows it); the view is not: RoboCAD's camera stays where
   it is (its `POST /threads/{id}/show`, api.py:399-414, is GUI-only).
3. Experiment-evidence thread: the code now opens the captured run in
   experiment review (148-163, `experiment_review::handle`), as RoboCAD
   opens its experiments panel (ui/comments.py:374-376); without a
   `run_id` or a connection the control is disabled and the op refused by
   name ("Captured evidence requires a run_id and a connected service",
   `controls.rs:80-86`; `isolation.rs:149-150`).

### CAD-180 Fit in view

1. Fit in view (`dock.rs:199`, `controls.rs:90`, disabled without linked
   parts that exist) → `ops.rs:329` (refused only for another thread than
   an open draft's) → `isolation.rs:fit` (191).
2. The linked parts still in the tree (`fit_nodes`, 83) and their
   descendants are framed at the current angle (`CadMeshes::frame`, 200;
   no geometry: "This annotation has no geometry to frame", 199, RoboCAD's
   text), selected (`select`, through the one selection), and the pins
   turned on (205, as `vp.show_comment_pins = True`).
3. Status line "Fit annotation in view: <names>" (208; ui/comments.py:484).

### CAD-181 Show only linked parts and Return

1. Link selected parts (`dock.rs:215`, `controls.rs:95`) → `ops.rs:link`
   → the commit leg, `PATCH {part_refs}` (`source.rs:patch`, 352), undo
   step "Update annotation".
2. Show only linked parts (`dock.rs:224`, `controls.rs:98`) → `ops.rs:333`
   (refused only for another thread than an open draft's)
   → `isolation.rs:view_parts` (215): the camera, the selection
   (`cx.shared.items()`, 231) and the whole `CadDisplay` captured the first
   time; a single part with a saved view restores it, else the section off,
   orthographic (247), framed; the parts and descendants shown alone (257)
   and selected; status "Part view: <names>".
3. The hint "Showing linked parts only · Esc or Return to assembly
   restores your view" is drawn in the section (`dock.rs:229`; RoboCAD's
   viewport tool hint, ui/comments.py:421).
4. Return to assembly (`controls.rs:99`) or Escape (`input.rs:escape`,
   212, `ThreadsOp::Return`, 221) → `isolation.rs:end` (270): the camera
   pushed back (273), the display restored, the selection from before
   (without parts deleted since) set on the one `Selection` and published
   to RoboCAD by `restore_selection` (282 → 290-298: `set` 294, `publish`
   295; shared with a saved view's restore, `views/mod.rs:end_part_view`);
   status "Returned to the assembly view" (283).
5. No `PATCH visible` and no `Ops.isolate` anywhere in `isolation.rs`:
   `GET /doc` visibility is unchanged; RoboCAD's window is not isolated.

### CAD-182 Resolve and Reopen

1. Resolve (`dock.rs:199`, `controls.rs:93`, "Reopen" when resolved) →
   `ops.rs:288` (a window press is refused by name while a draft is open,
   `ops.rs:ui_not_drafting` 152, as RoboCAD's dock disables the button; a
   REST `resolve` is accepted, as RoboCAD's `PATCH` route) → the commit leg:
   `ThreadCommand::Resolve` → `PATCH {status}` (`source.rs:510-513`) →
   `annotations.py:update_thread` (251-269), undo step "Update
   annotation".
2. Filters: `cad:threads:filter-*` (`controls.rs:72`, chips `dock.rs:156`)
   → `ops.rs:190` (refused while drafting) → `Filter::keeps`
   (`mod.rs:134-140`) in `controls.rs:shown_threads` (31-36).
3. Shown: the row's number becomes "✓" (`dock.rs:148`, `controls.rs:77`;
   ui/comments.py:257); the pin is not drawn while resolved
   (`pins.rs:88`; RoboCAD draws open threads only, ui/comments.py:520).

### CAD-183 Edit and delete a message

1. "···" → Edit message (`ThreadsOp::EditMessage`, `ops.rs:254` →
   `edit_message`, 415): the body into the composer, `editing` set; Save
   edit (`controls.rs:24`) → `ops.rs:234` (a message gone since is refused
   under the composer, 237) → commit leg: `PATCH /comments/{id}`
   (`source.rs:491-497`) → `annotations.py:update_comment` →
   `_change_comment` (290-305), undo step "Edit comment".
2. Delete message (`controls.rs:110`; the last message is refused before
   sending, "delete the thread to remove its last comment",
   `source.rs:506`, RoboCAD's annotations.py:298) → `ops.rs:262` →
   `DELETE /comments/{id}`, undo step "Delete comment".
3. Each is one RoboCAD undo step; see "One undo step per thread edit".

### CAD-184 Delete thread

1. Delete thread (`dock.rs:279`, `controls.rs:113`) → `ops.rs:277`
   (refused while drafting) → commit leg: `DELETE /threads/{id}`
   (`source.rs:515-517`) → `annotations.py:delete_thread` (271-274), undo
   step "Delete annotation".
2. Answer: `mod.rs:482-485` clears the current thread; the re-read drops
   the row and its pin.
3. Undo: `cad_undo` / Ctrl+Z (`registry.rs:307`) brings the thread and its
   pin back (see "One undo step per thread edit").

### CAD-185 Pins

1. View ▸ Toggle comment pins (`registry.rs:297`, `cad:threads:pins`
   `controls.rs:114`) → `mod.rs:427` → `CadDisplay::comment_pins`
   (display only) → `pins.rs:draw` (149-157) draws none with it off.
2. Pins drawn: each open thread with a surface anchor on a visible part,
   numbered by its place in RoboCAD's list (`pins.rs:80-101`); amber
   `REVIEW` (40) for "needs_review", blue `ATTACHED` (38) otherwise
   (ui/comments.py:529); none for evidence or a deleted part (92).
3. Click pin "1": a UI button (`pins.rs:169`) → `press` (201) →
   `ThreadsOp::Open` (204) → `ops.rs:open` (363): the section shown, the
   filter All, the thread current (refused while a draft is open on
   another thread, as RoboCAD's `select`, ui/comments.py:310-314).

### CAD-186 Reattach

1. Delete the anchored body: RoboCAD's `thread_detail` answers
   `anchor_status: "missing"`, `node_name: "Deleted part"`
   (annotations.py:110-114); the location line reads "Deleted part · Part
   deleted — reattach this annotation" (`controls.rs:44`), in WARN
   (`dock.rs:192-195`); the pin is not drawn (`pins.rs:92`).
2. Reattach… (`controls.rs:91`) → `ops.rs:305` → `annotate.rs:start` (51)
   with the thread → the click as in CAD-176 (press, shown revision) →
   `place` (83) → `moved` (140): the thread's first target replaced with
   the new surface pin → `ops.rs:put` (94) → `source.rs:patch` (352: only
   `node_id`, `point`, `face`, `view`, 366-369) → **one** `PATCH
   /threads/{id}` → `annotations.py:update_thread` (257-258: `anchor` at
   the new node, a fresh geometry stamp), one undo step "Update
   annotation"; the thread is opened (`annotate.rs:111`).
3. Re-read: "attached", "<new part> · Attached to surface", the pin moves
   to the new point.
4. Every anchor text matches RoboCAD's dict (ui/comments.py:283):
   "Captured experiment", "Attached to surface", "Part deleted — reattach
   this annotation", "Geometry changed — check and reattach this pin"
   (`controls.rs:42-45`); "Attachment unknown" is the viewer's own for a
   state it does not know (recorded). The evidence run line now prints
   the time range as Python's `str()` does, `[0.5, 2.0]` (`dock.rs:time_range`,
   289).

### Persistence: save, close, reopen

1. After creating, replying to and resolving a thread (CAD-176, 177,
   182), `doc.annotations[tid]` holds the anchor (node id, point, geometry
   stamp, face description), the saved camera `view`, `status`, the
   comments and `part_refs` (annotations.py:241-247, 256-268, 279).
2. Save (`cad_save` / `cad_file save_as`) → `cad/files/mod.rs:save` (378)
   → `POST /save/thumbnail` (api.py:1736) → `save_with_thumbnail`
   (api.py:1296) → `Document.save` (document.py:514) →
   `archive_snapshot` (525) → `to_manifest` writes `"annotations"` and
   `"revision"` (499-512).
3. Close and reopen in a new service and window: `cad_open` → a new
   headless service loads the file, `Document.load` restores
   `annotations` (document.py:569) and `revision` (562); a pin stays
   "attached" because the stamp fingerprints mass properties and face
   geometry, not B-rep bytes or face indices (annotations.py:47-58).
4. The new window's document has a new generation
   (`cad/sync/mod.rs:start`, `threads::restarted` at 85 drops whatever was
   read before); `read.rs:listed` keeps only lists of this generation
   (65-67), so the first read is `GET /threads` at (generation, revision,
   epoch). The anchor, the pin point, the saved view (Show on model,
   CAD-179) and the resolved state ("✓", no pin) come back as read.

### Autosave while attached

1. RoboCAD's desktop window autosaves on a timer
   (`ui/app.py:_start_autosave`, 129-133): `_autosave` (135-146) runs
   only while `doc.dirty` and the revision differs from the last autosave,
   captures `archive_snapshot` (the manifest with `annotations`) on the Qt
   thread and writes `<file>.autosave.rcad` on a worker; `_finish_autosave`
   (148-160) records the revision. `notify("autosaved")` does not move the
   revision (document.py:288).
2. A thread edit from the viewer sets `doc.dirty` and moves the revision
   (annotations.py:200-201), so the next autosave includes it.
3. The viewer reads `GET /autosave` (api.py:462-471) on every poll tick
   from a desktop window, apart from the `/doc` refetch
   (`cad/sync/mod.rs:205`, applied only when it differs, 497-500; one
   failed `GET /autosave` after a good read keeps the last line, 206-214),
   and shows it in the left dock's Autosave line
   (`cad/panel.rs:autosave_line`, 588). A headless service has no
   autosave (409). An autosave that completes without a revision change
   now shows within a poll tick (commit 83ebb812; trace "Reading traces —
   Part I › Autosave line follows autosave state").

### One undo step per thread edit

1. Every thread commit is one RoboCAD call (`source.rs:request_on`, 475;
   `Request`, 271-279) and one `ChangeThreads` push (annotations.py:248,
   268, 274, 281, 303).
2. The annotations service's own undo is refused: `ThreadCommand::Undo` /
   `Redo` → `UNDO_IS_ROBOCADS` (`source.rs:455`, 465, 519), which names
   the document's Undo and Redo.
3. Undo: `cad_undo` / Ctrl+Z (`registry.rs:307`) →
   `cad/actions.rs:525` → `POST /undo` (api.py:1677-1678) →
   `Api.undo` (api.py:1046) → `CommandStack.undo` (commands.py:221-228)
   → `ChangeThreads.undo` restores the previous threads and
   `notify("annotations")` moves the revision (document.py:287-289).
4. The answer is not a thread commit (`edit_answered` returns at
   `mod.rs:453-455`), but the revision moved: `sync/mod.rs:refresh` →
   `GET /doc` → `doc.doc_key` (`sync/mod.rs:456`) → a new read key
   (`read.rs:59-60`) → the re-read leg. Redo is the same through
   `POST /redo`.

### Remote refresh keeps the draft

1. A reply typed in RoboCAD's own window → `annotations.py:add_comment`
   → `notify("annotations")` → revision + 1.
2. The viewer's poll (`cad/sync/mod.rs:poll_loop`, 160) reads `GET /`
   every 500 ms (181); a new (document id, revision) fetches `GET /doc`
   (193) → `receive` sets `doc.doc_key` (456) → `shown_revision`
   (`cad/document/state.rs:128`) → the read key moves (`read.rs:59-60`) →
   the re-read leg, with no manual refresh (while the section is open or
   the pins are on, `read.rs:105-107`).
3. The draft survives: `read.rs:tick` (129-166) writes only the read
   state; nothing in the re-read touches `compose`, `current`, `editing`
   or `pending`; the composer field is redrawn from `ThreadsState`
   (`input.rs:192-199`).
4. A pending pin whose revision the shown document has moved past is
   stale (`mod.rs:stale_pin`, 266): its face index is RoboCAD's numbering
   at that revision, so Post annotation is refused under the composer
   with "RoboCAD's document changed since the pin was placed: Annotate
   again to place it on the current model" (`mod.rs:STALE_PIN`, 260),
   nothing sent and the text kept (`ops.rs:387`, `refuse_draft` 159,
   `mod.rs:draft_gone` 282); ＋ Annotate model and N are allowed then
   (`controls.rs:70`, `annotate.rs:53`) and the click replaces the pin
   (`annotate.rs:97`), keeping the text.
5. A reply whose thread, or a Save edit whose message, is gone from a list
   read at RoboCAD's current revision (deleted there, or undone) is kept,
   its composer and Cancel still drawn (`dock.rs:247`), the reason shown
   under it (`dock.rs:271`) and the post refused by name, nothing sent
   (`ops.rs:218-223`, 237-241). While the list is being read again nothing
   is claimed gone.
6. Another mode's request to show a thread (`mod.rs:RevealThread`,
   `Reveal::new` 104) opens the Comments dock once (`read.rs:reveal_step`
   219, `reveal` 243-249; closing it drops the request), then opens the
   thread as RoboCAD's `select` does (`read.rs:261`, `ops.rs:open`): an
   open draft on another thread keeps its thread and the status line says
   why. A read that failed at the current key, or a lost connection, ends
   it with "Asked to show comment thread X: RoboCAD's comments could not
   be read: …" (`read.rs:223-228`, 255-258).

## Reading traces — Part G

Each trace follows one Part G step (CAD-131 to CAD-152) from the native
control to RoboCAD and back to what the Materials and Robot sections, the
inspector rows, the status line, the stress overlay, the exported file or
the mode switch show. Everything here is **by reading, unexecuted**:
nothing was built, run or captured, and no step was compared side by side.
Native paths are under `crates/sim-spatial/src/` unless they start with
`crates/`; RoboCAD paths are under `cad/robocad/`. Gaps found while tracing were fixed in commits 49bc553e and e6c48005 (each named in its message), unless an entry says it is recorded.
The common legs are written out once:

- **Click leg** (every button and menu entry): a kit button carries
  `CadButton(action)` (`cad/panel.rs:55`), which
  `cad/panel/name.rs:buttons` (71) writes as `Act::ui(action)`; a menu,
  command bar or keyboard entry is a registry command
  (`cad/surfaces/registry.rs:450-467`): `Native::Action(Do::Physical(id))`
  → `Do::action` (127) → `cad/robot/mod.rs:command_action` (106; validate,
  the motor library) → `cad/results/mod.rs:command_action` (455; results,
  identification, overlay, exports, the link), or `Native::Op` →
  `CadInvoke` → `cad/ops/invoke.rs:invoke` (12; the op-catalogue form or
  click tool, `cad/ops/catalogue/robot.rs:37-226`). The one apply system
  `cad/actions.rs:apply` (354) drains `Act<CadAction>` into `handle` (459;
  arms `CadRobot` 577, `CadMaterials` 578, `CadInspector` 579,
  `CadResults` 580, `CadInvoke` 555); a refusal from a click is written to
  the status line there (408). REST sends the same actions (`cad_materials`,
  `cad_inspector`, `cad_robot`, `cad_results`, `cad_invoke`, `cad_run`);
  `system_ui` lists the same controls (`materials::controls_of`,
  `inspector::physical_edit::controls_of`, `robot::controls`,
  `results::controls_of`).
- **Edit leg** (every document change): `cad/edit.rs:edit_at` (48) refuses
  by name with nothing sent when `CadDocument::commit_refusal`
  (`cad/document/state.rs:138`) names a reason (an edit in flight, not
  connected, the shown document behind RoboCAD's, or RoboCAD's revision
  moved since the form/row was read) → `edit` (12) →
  `cad/sync/mod.rs:start_edit` (686): one `Job::spawn(Pool::Dedicated, …,
  "RoboCAD edit: …")` (693) off the UI thread → the `sim_runtime::cad_client`
  call → RoboCAD `api.py:_route` (1540) → `run_on_main` → the `Ops` method
  pushing one command on RoboCAD's stack (`commands.py`) → the answer lands
  in `cad/sync/mod.rs:finish_edit` (565; an answer of an older generation is
  dropped), which writes the edit's message (or RoboCAD's error) to the
  status line (601), selects a created node when the op asked for it
  (`cad/ops/mod.rs:started`, 662, `selects_created`) and refetches `/doc`
  (`refresh`, 641, called from `finish_edit` at 630); the tree, inspector
  and sections redraw on the new revision.
- **Robot reads** (what the Robot section, the joint rows and the results
  line show): `cad/robot/data.rs:sync` (160; JobResults after
  `CadSet::Results`) starts one `Pool::Dedicated` job per (generation,
  shown revision) (192) reading `GET /robot`, `/results/nodes`,
  `/sensors`, `/cables`, `/battery`, `/control`, `/uncertainty`,
  `/actuator-profiles` and once per generation `/motors`
  (`crates/sim-runtime/src/cad_client/robot.rs:409-480`,
  `physical.rs:178`); a job for an older key is dropped (cancelled, 165)
  so a stale read never lands; RoboCAD `api.py:1754-1813` →
  `commands.py:robot` (1036) → `robotics.py:robot_summary` (322) and
  `api.py:results_nodes` (1465) over `physical.py:results_margins` (1030).
  A results load or identification does not move RoboCAD's revision, so its
  settle calls `RobotData::invalidate` (`data.rs:62`).
- **Physical model reads** (joint physics rows, material defaults):
  `cad/inspector/refresh.rs:sync` (45) fetches `GET /physical?flex=0`
  (`cad/sync/mod.rs:fetch_physical`, 661, one `Pool::Dedicated` job;
  `crates/sim-runtime/src/cad_client/mod.rs:physical`, 430) once per
  (generation, revision) while a joint is inspected or a material
  properties dialog is open, after the revision settled for 0.5 s; RoboCAD
  `api.py:1765-1778` derives it in `export_worker.export_snapshot` off its
  GUI thread.

### CAD-131 Materials list and search

1. The Materials section of the right dock: `cad/panel.rs:464` →
   `cad/materials/panel.rs:draw` (98): "Search materials…" (`SEARCH`, 42,
   the kit text field), one row per `/doc` material (`materials/mod.rs:list`,
   141, `Material::of`), "■" in the material's colour (`Color::srgb` of
   `m.color`, panel.rs:114) then `row_label` (`mod.rs:153`: "■ name   {density
   as py_g(·, 6)} g/cm³", Python's `:g`).
2. Typing: `panel.rs:input` (169; `CadKeySet::Focus`) mirrors each
   `FieldEvent::Changed` of `SEARCH` into `MaterialsState::search` (202-208);
   REST `cad_materials {op: search, text}` → `mod.rs:handle` (174). The
   filter is `mod.rs:matches` (147): the name lower-cased contains the
   lower-cased text, or a tag contains it.
3. No RoboCAD call: the list is `/doc`'s `materials` (`api.py:doc_state`).
   RoboCAD's counterpart is `ui/widgets.py:MaterialsPanel.refresh` (765):
   the same row text `f"■ {m.name}   {m.density:g} g/cm³"`, the same filter
   (`t not in m.name.lower() and not any(t in tag …)`: tags compared as
   stored, the text lower-cased, as here).
4. Shown: the filtered rows, "No material matches the search." when none
   (panel.rs:110); `cad_state.materials.materials[].shown`
   (`mod.rs:state_json`, 328).

Matches RoboCAD. Deliberate difference (cosmetic): RoboCAD colours the
whole row text, the viewer only the ■.

### CAD-132 Apply a material

1. Click a row: `panel.rs:input` (287-307) writes `cad_materials {op:
   select}` (a press) or, for a second press on the same row within 400 ms
   (Qt's double-click), `{op: apply, material}` (301). **Apply to
   selection**: control `cad:materials:apply` (`mod.rs:controls_of`, 358),
   drawn by `panel.rs:132`.
2. `mod.rs:handle` (191) → `apply` (260): with nothing selected, refused
   "Nothing selected: select the bodies to give {name}, then apply it"
   (267; shown on the status line, `actions.rs:408`, and as a note under
   the buttons, panel.rs:136-138); a node not in the shown tree is refused
   (269) → edit leg with `began` the shown revision (272, 278).
3. `crates/sim-runtime/src/cad_client/physical.rs:set_material` (196) →
   `mod.rs:op` (374) `POST /ops/set_material` → `api.py:1668-1671` →
   `api.py:op` (1011) → `commands.py:set_material` (345): one
   `SetAttributes("Material", …)` over every id.
4. Shown: status "Set material {name} on {n} node(s)"; the inspector's
   material and mass follow the refetched `/doc` and node detail. Undo
   (Ctrl+Z → `CadUndo`, `actions.rs`) undoes the one "Material" step.

Matches RoboCAD (`ui/widgets.py:_apply`, 776: one `ops.set_material(ids,
id)`). Deliberate differences (recorded): an empty selection is refused by
name where RoboCAD silently does nothing; no drag of a material onto a body
(`materials/mod.rs:16-20`).

### CAD-133 New material

1. **New…** (`cad:materials:new`, `mod.rs:359`) → `handle` (192) →
   `form::new_form` (`materials/form.rs:137`: "Name" "Custom", "Density
   (g/cm³)" "1.2", 0.01–25, three decimals, RoboCAD's `_new`,
   `ui/widgets.py:795`) → `open` (`mod.rs:226`): the modal kit form
   (`panel.rs:draw_form`, 365) takes the keyboard.
2. Typing `abc` in Density: `form::ok_ready` (217) answers "Density
   (g/cm³): …" (the kit evaluator's error naming the field); OK is
   disabled (`panel.rs:395`, control `cad:materials:form-ok` not ready,
   `mod.rs:366`) and the reason is drawn under the buttons (`footer`, 404).
3. OK: `mod.rs:submit` (282) → `form::submit` (256): id = name lower-cased,
   spaces "_" (else "custom"), density rounded to three decimals →
   `edit_at(…, None, …)` (293; a new material reads nothing from the
   document, so only an edit in flight or no connection refuses it).
4. `cad_client/physical.rs:add_material` (163) `POST /materials` →
   `api.py:1814-1816` → `api.py:add_material` (1483): `Material.from_json`
   with RoboCAD's defaults, one `SetMaterialDef("Material", m)`.
5. Shown: "Added material Check PLA (check_pla)"; the new row with density
   "1.25 g/cm³"; undo removes it.

Matches RoboCAD for the material and its undo. Recorded, not fixed: needs
RoboCAD change — the undo step is named "Material" through `POST
/materials` (`api.py:1487`) and "New material" in RoboCAD's own dialog
(`ui/widgets.py:813`); the route fixes the label, and no `Ops` method adds a
material with another one, so the viewer cannot match it without a RoboCAD
change.

### CAD-134 Engineering properties

1. **Material properties…** (`cad:materials:properties`, `mod.rs:360`) or
   the inspector's (`cad:inspect:material-props`,
   `inspector/physical_edit.rs:291`, drawn `inspector/rows.rs:237`) →
   `cad_materials {op: properties}` → `mod.rs:handle` (196) →
   `form::properties_form` (`form.rs:157`): RoboCAD's rows and labels
   (`PROPS`, 37 = `ui/widgets.py:_edit_material`, def 681, rows 690), each labelled with
   its origin (`Origin::name`, 74): the document's override ("set in this
   document"), else the physical model's value ("RoboCAD's default", `block`,
   144, read only at the shown revision), else "not reported".
2. While it is open the physical model reads leg fetches `GET /physical`
   (`refresh.rs:wanted`, 24 → `materials::wants_physical`, `mod.rs:241`);
   when it lands `refilled_form` (248) / `form::refill` (199) fills the
   defaults, keeping typed fields.
3. Change Yield strength, OK: `form::submit` (256) sends only the changed
   keys in SI (278-285; friction as RoboCAD builds it, 286-292) →
   `mod.rs:submit` (295-298) → edit leg with `began` the form's revision →
   `cad_client/physical.rs:set_material_props` (208) `POST
   /ops/set_material_props` → `api.py:op` (1011) →
   `commands.py:set_material_props` (1171): one `SetMaterialDef("Material
   properties", …)`.
4. Shown: "Set {name}'s engineering properties: yield_strength"; reopening
   shows the value "set in this document".

Matches RoboCAD for the stored value. Deliberate difference (recorded):
only changed keys are sent (RoboCAD re-sends all), and a default RoboCAD
takes from its `_ENG` table but does not report (the material is not in the
physical model) shows "not reported".

### CAD-135 Colour

1. Inspector, Colour row (`inspector/rows.rs:222-227`, `input_row` 188):
   typing `0.9, 0.2, 0.2`, Enter → `inspector/entry.rs:entry` (56) →
   `submit` (33): `parse_color` (`physical_edit.rs:169`, three numbers
   0–1) → `cad_inspector {op: color, id, color, revision}` (entry.rs:42).
   **Use material colour**: control `cad:inspect:material-colour`
   (`physical_edit.rs:283`; disabled "… already uses its material's colour"
   when it has none).
2. `physical_edit.rs:handle_physical` (216) → 224-245 → edit leg with the
   row's revision.
3. `crates/sim-runtime/src/cad_client/mod.rs:patch` (294) `PATCH
   /nodes/{id} {"color": [r,g,b] | null}` → `api.py:1630-1631` →
   `api.py:patch` (748) → `commands.py:set_color` (348): one
   `SetAttributes("Color", …)`.
4. Shown: "Set {name}'s colour to 0.9, 0.2, 0.2" / "{name} uses its
   material's colour"; the body redraws from `/doc`'s `color`.

Matches RoboCAD (its route is the reference). Deliberate difference
(recorded): an "r, g, b" field instead of a colour dialog.

### CAD-136 Joint editor

1. Select a joint; inspector **Edit joint…** (`cad:inspect:edit-joint`,
   `physical_edit.rs:286` → `CadInvoke ops.set_joint`), or the Robot
   panel's double-click (CAD-141) → `cad/ops/invoke.rs:invoke` (12) → the
   catalogue form `ops.set_joint` (`cad/ops/catalogue/robot.rs:183`,
   precheck `cad/ops/robot_form.rs:166`), preset from the joint
   (`robot_form.rs:fill_from_joint`, 315, from `GET /robot`).
2. OK → `cad/ops/form.rs:submit` (15) → `cad/ops/robot_args.rs:build`
   (160), `RobotCall::EditJoint` (193-205): `set_joint(jid, fields)` and,
   only when a name is given and differs, `rename(jid, name)` (199-202) →
   `robot_args.rs:send` (344) → edit leg (one job, the calls in order).
3. `crates/sim-runtime/src/cad_client/mod.rs:op` (374) → `api.py:op` (1011)
   → `commands.py:set_joint` (949), then `commands.py:rename` (333).
4. Shown: "joint {name} updated"; the inspector's Joint rows
   (`rows.rs:joint_fields`, 303) and the Robot panel row read the new
   limits after the robot reads leg.

Matches RoboCAD (`ui/app.py:robot_edit_joint`, 1565-1575: `set_joint`, then
`rename` only when the name changed).

### CAD-137 Joint physics overrides

1. Select a joint: `rows.rs:joint` (241) draws RoboCAD's rows
   (`JointField::label`, `physical_edit.rs:50`) from the physical model
   (`rows.rs:physics`, 60, only at the shown revision) and the declared
   overrides (`overrides`, 71, from `GET /nodes/{id}` at the shown
   revision): `row` (100) puts " *" on an overridden row, "Drive backlash
   (°; provenance)" with its reference, and `source_line` (140) is
   RoboCAD's "source: …, pin Ø… mm in Ø… mm over … mm; * = overridden".
2. Type a Coulomb friction, Enter → `entry.rs:entry` (56) → `submit` (46):
   `cad_inspector {op: joint_physics, field: coulomb, value, revision}` →
   `handle_physical` (246-263) → `joint_override` (183: mN·m → N·m,
   `{"friction": {"coulomb": …}}`, the payload RoboCAD's panel builds) →
   edit leg with the row's revision.
3. `cad_client/physical.rs:set_joint_physics` (216) `POST
   /ops/set_joint_physics` → `api.py:op` (1011) →
   `commands.py:set_joint_physics` (1184).
4. Shown: "Set {name}'s Coulomb friction (mN·m)"; after the revision moves
   the physical model and the node detail are read again (physical model
   reads leg) and the row shows the value with " *".

Matches RoboCAD (`ui/widgets.py:refresh`, 509-544, `_joint_override`, 659).
Deliberate difference (recorded): a value the physical model lacks is
empty, not RoboCAD's 0.0 (or its 4 mm flex patch fallback).

### CAD-138 Results line

1. After CAD-150, select a link body: the inspector rows
   (`rows.rs:draw`, 207) read `doc.robot.data.node_results(id)`
   (`robot/data.rs:96`, `GET /results/nodes`' `nodes[id].results`, the
   node's `Node.results` block as `api.py:results_nodes`, 1465-1481,
   returns it) through the robot reads leg.
2. `rows.rs:results_line` (54): the keys of `RESULT_KEYS` (50:
   `peak_stress_pa, yield_margin, max_deflection_m, peak_temperature_c,
   peak_reaction_force_n, bearing_margin, peak_current_a, stall_margin,
   peak_winding_c`, RoboCAD's order) that are numbers, each
   `format!("{k} {}", py_g(v, 3))`, joined ", " after "Results: ".
   `py_g` (`inspector/physical_edit.rs:140`) is Python's `format(v,
   ".3g")`: three significant digits, trailing zeros dropped, exponent form
   below 1e-4 or from 1e3 ("1.5e+08").
3. RoboCAD's counterpart: `ui/widgets.py:refresh` (545-549): the same key
   list filtered `r.get(k) is not None`, `f"{k} {r[k]:.3g}"`, joined ", "
   after "Results: ".
4. Shown under the Colour rows (`rows.rs:231-236`), with "These results
   were computed for another state of the document (stale)." when RoboCAD's
   flag says so.

Matches RoboCAD (same keys, same order, 3 significant figures).

### CAD-139 Exact measurement

1. **Calculate exact measurements** (`cad:inspect:exact`,
   `physical_edit.rs:277`, drawn `rows.rs:217`; enabled by
   `inspector/exact.rs:ready`, 183: something measurable selected, no run,
   connected) → `cad_inspector {op: exact}` → `handle_physical` (222) →
   `exact::start` (202).
2. `start` stamps the run (`Stamp::of`, 57: generation, shown revision,
   `edit_seq`, the selected nodes) and spawns one `Job::spawn(Pool::Dedicated,
   …, "cad-exact-measurement")` (210) running `measure` (229): one `GET
   /nodes/{id}` per node (`crates/sim-runtime/src/cad_client/mod.rs:node`,
   288) within RoboCAD's 60 s limit (`LIMIT`, 40), each request's timeout
   clamped to what remains, `ctx.cancelled()` checked between requests.
3. RoboCAD: `api.py:1626-1627` → `node_detail` (114-120: `mass` =
   `kernel.mass_properties` of the resolved body with the node's density);
   `combine` (250) adds them as `analysis.py:selection_properties` (66-92)
   does (bbox union, summed volume, area, mass, centroid weighted by
   `max(m, 1e-9)`).
4. **Selection/revision guard**: `exact::sync` (295; JobResults after
   `CadSet::Results`, `physical_edit.rs:337`) runs `settle` (156) each
   frame against the stamp now: `cancel_reason` (140) drops the run when
   the generation changed ("the CAD document was replaced or reconnected"),
   when `edit_seq` or the revision moved ("the document was edited": an
   edit started here moves `edit_seq` before RoboCAD's revision does) or
   when the selected nodes changed ("the selection changed"); dropping the
   run drops its `Job` (cancel flag set, `jobs/mod.rs:260`), so its answer
   is never applied, and a result is shown only for the stamp it was
   measured at (`ExactState::facts`, 123; `settle` 163-166 drops one for
   another stamp). **Cancel**: `cad:inspect:exact-cancel`
   (`physical_edit.rs:279`) → `exact::cancel` (219).
5. Shown: the facts line (`rows.rs:214`): "Calculating exact
   measurements… You can keep working.", then RoboCAD's text
   (`Measured::text`, 75 = `ui/widgets.py:_measure_selection`, def 572,
   text from 620:
   "{n} measured item(s)\nsize … mm\nvolume … cm³\narea … cm²\nmass … g\ncentroid
   (…)"), or "Exact measurements cancelled: {why}. This window stopped
   waiting; …" on the facts line. The status line shows it too only for a
   selection change (`sync` 307-310: the run's stamp and the stamp now have
   the same generation, revision and `edit_seq` and different nodes; shown
   at 312-314) and for Cancel (`exact::cancel` 224); a run dropped by an
   edit or a document change leaves the status line to the edit's send line
   or the reconnect's "connecting…" written the same frame (`sync` 315,
   `touch` only).

Matches RoboCAD for the values and the rule (`ui/widgets.py:refresh`,
480-486: a key `(id(doc), revision, ids)` change cancels). Deliberate
difference (recorded): a cancel only stops waiting (RoboCAD kills its
measuring process); the preview has no "Display size ≈" line. Gap found and
fixed: a cancel by a selection change or the Cancel button was only written
on the facts line, never on the status line; `exact::sync` now shows the
reason there when `settle` dropped a run for a selection change only
(`inspector/exact.rs:302-317`), and `exact::cancel` too
(`inspector/exact.rs:223-224`). A first fix showed every dropped run's
reason on the status line, which could overwrite another feature's line
written the same frame (an edit's send line, a reconnect's "connecting…");
an edit or a document change now leaves the reason on the facts line only.

### CAD-140 Robot panel summary and tree

1. The Robot section (`cad/panel.rs:463`) → `cad/robot/panel.rs:draw`
   (396) over `view` (280), from the robot reads leg.
2. `summary_line` (216): "{links} bodies, {joints} joints, {n} DoF (or
   "closed-loop mobility requires constraint analysis"), {motors} motors,
   {sensors} sensors, {cables} cables. Ground: {names or "none (heaviest root
   body is used)"}. Power: {`g(V)` V chemistry or "no battery (motor supply
   voltage)"}." plus "  Results: {file}" and " (stale: the document
   changed since they were loaded)" from RoboCAD's `stale` flag.
3. Branches: "Links" (`links`, 254: bodies and sheets, motors excepted,
   material name, ", peak … MPa", ", … °C"), "Joints" (292-312: "{type}:
   {parent or world} → {child}  [{lower}°, {upper}°]  motor {name}"),
   "Motors" (313-323: "{spec_name}: on {body or loose}, drives {joint or no
   joint}"), "Sensors & cables" (324-336) only when there are any; each row's
   margins `margin_text` (168: "+.2f" ratios, "+.0f°C" temperatures, two
   spaces apart); each row a button with name, Detail and Margin lines
   (`row`, 436).
4. RoboCAD's counterpart: `ui/widgets.py:RobotPanel.refresh` (1271-1355),
   the same texts from `ops.robot()`, `results_margins`, `doc.walk()`.

Matches RoboCAD. Deliberate differences (recorded): no "(n s run)", "(stale:
…)" added, no branch glyphs (⚙ ⚡ ◎ 〜 ▣), the joint's motor reads "motor
{name}" for "⚡{name}", Detail and Margin are lines under the name.

### CAD-141 Robot panel click and double-click

1. A row press: its `CadButton(select_action)` (`robot/panel.rs:450`,
   `select_action` 197: `CadSelect {ids: [id], picked_at: the revision the
   description was read at}`) → `actions.rs:handle` →
   `cad/selection/mod.rs:108` → `select`: the one shared selection, so the
   tree, the 3D highlight and the inspector follow; RoboCAD is told through
   the selection push (`selection::publish`).
2. A second press on the same joint row within 400 ms:
   `panel.rs:double_click` (532; `InputSet::Window`) → `press` (523) writes
   `CadInvoke ops.set_joint` → the Edit joint form preset from that joint
   (CAD-136, step 1).
3. No RoboCAD edit; RoboCAD's counterpart is `ui/widgets.py:_select`
   (1357: `selection.set_nodes(ids)`) and `_edit` (1363:
   `robot_edit_joint(nid)` for a joint).

Matches RoboCAD.

### CAD-142 Validate and issues

1. Robot ▸ Robot: validate: registry `robot.validate`
   (`surfaces/registry.rs:457`) → `robot/mod.rs:command_action` (108) →
   `cad_robot {op: validate}` → `robot/mod.rs:handle` (116; arm 125) →
   `robot/tools.rs:handle` (120) → `validate` (180); control
   `cad:robot:validate` (`tools.rs:456`).
2. The verdict needs the description at the current revision
   (`settle_now`, 149, `RobotData::current`): a click before it is read
   shows "Robot: validate: reading RoboCAD's robot description…" (202) and
   `settle` (208; JobResults after `data::sync`) shows the verdict when it
   lands; REST waits (198).
3. `verdict` (137) over `GET /robot` (`cad_client/robot.rs:robot`, 409 →
   `api.py:1754` → `commands.py:robot` 1036, `exact=False` →
   `robotics.py:robot_summary` 322 → `validate_robot` 340): no issues:
   `Ok("robot valid: {links} bodies, {n} joints, {mobility}")`; else
   `Err("Robot validation: [severity] message; …")` (142-143).
4. Shown: the status line (`settle_now`, 165); the Robot panel's issue list
   (`robot/panel.rs:337-343`): "Error: …" for `error`, "Warning: …"
   otherwise (`warning`, `info`), "✓ robot is valid" when there are joints
   and no issues.

Matches RoboCAD: `ui/app.py:robot_validate` (1635-1642) writes `f"robot
valid: {info['links']} bodies, {len(info['joints'])} joints, {mobility}"`
on the status line, else a "Robot validation" box of `f"[{severity}]
{message}"` lines; its panel prefixes "⛔ " for errors and "⚠ " for
everything else (`ui/widgets.py:1347-1352`), which "Error:"/"Warning:"
mirror. Deliberate difference (recorded): the box is the status line and
issue list, the glyphs are words.

### CAD-143 Motor library

1. Robot ▸ Robot: motor library…: registry `robot.motors` (458) →
   `command_action` (110: shown, never toggled off) → `tools.rs:handle`
   (121-129) sets `library_open`.
2. The library is the robot reads leg's `GET /motors` (once per
   generation; `cad_client/robot.rs:motors`, 423 → `api.py:1756` →
   `commands.py:motor_library` 1041, `MOTOR_LIBRARY`).
3. `robot/tools_library.rs:draw` (48; Present) draws a floating kit panel
   with `rows` (28): `"{name:<28} {kind:<13} {stall:>7} N·m {speed:>6}
   rad/s {mass:>6} g   {notes}"`, values through `ops::g` (Python's `:g`),
   in the library's id order; Close writes `{op: library, open: false}`.

Matches RoboCAD's text rows (`ui/app.py:robot_motor_library`, 1644-1647).
Deliberate difference (recorded): a panel, not a message box; rows in id
order.

### CAD-144 Add motor from library

1. Robot ▸ Robot: add motor from library… (registry 450, `Native::Op`) or
   the Robot panel's **Add motor…** (`robot/panel.rs:61`, ready by
   `registry::ready`) → `CadInvoke robot.add_motor` → `ops/invoke.rs` opens
   the catalogue form (`ops/catalogue/robot.rs:38-58`: Motor, Rotation
   about shaft, Mount on, the cut checkbox, Name) and the click tool
   `Flow::RobotPick(Motor)`; the form stays beside the view.
2. A face click: `robot/tools_click.rs:click` (48; `InputSet::Window`):
   the cursor ray's first unlocked body and its face at the shown revision
   (`CadMeshes::face_at`), RoboCAD's snap there → `cad_robot {op: pick,
   item, picked_at}` → `tools.rs:pick` (312; a face item must carry the
   shown revision, `check_pick` 299) → `motor_pick` (326): the snap or hit,
   `motor_mount` (262: the shaft into the body, radial on a cylinder) →
   `ops::run_entry` (`cad/ops/mod.rs:510`) with `picked_at` as the revision
   → `ops/robot_args.rs:build` `AddMotor` (163-179) → edit leg.
3. `cad_client/mod.rs:op` `add_motor` → `api.py:op` (1011) →
   `commands.py:add_motor` (966): one undo step.
4. Shown: "motor placed on {body}; Assign motor… links it to a joint" (as
   `ui/tools.py` 1284); `ops::started` (`ops/mod.rs:662`) asks
   `finish_edit` to select the created node. Escape ends the tool and the
   dialog (`ops/invoke.rs:end_tool`, 150).

Matches RoboCAD (`ui/app.py:robot_add_motor`, 1526-1531; `ui/tools.py:
MotorTool.press`, 1261). Deliberate difference (recorded): the fields stay
beside the view while clicking.

### CAD-145 Add joint tool and joint from selection

1. Robot ▸ Robot: add joint (registry 451; Ctrl+Shift+J bound here) →
   `CadInvoke robot.add_joint` → `Flow::RobotPick(Joint)`.
2. Clicks: `tools_click.rs:click` (Ctrl-click at stage 0: the world) →
   `tools.rs:joint_pick` (371): stage 0 the parent, 1 the child (face
   selection mode set), 2 the axis face → `joint_axis` (277: a cylinder's
   axis with the click projected onto it, else the face normal through the
   click) → `ops::open_preset` (`ops/robot_form.rs:445`) opens
   `robot.joint_dialog` preset with parent, child, pivot and axis; status
   "Joint: check the joint dialog and press OK to add it" (430).
3. Joint from the two selected bodies…: `CadInvoke robot.joint_dialog`
   (`catalogue/robot.rs:71-80`; preset from the selection). OK →
   `robot_args.rs:build` `AddJoint` (181-192; "a joint needs a child body"
   refused before anything is sent) → edit leg: `add_joint` and, when
   damping ≠ 0, `set_joint(id, damping)` (`robot_args.rs:367-378`).
4. `api.py:op` → `commands.py:add_joint` (935), `set_joint` (949).
5. Shown: "joint {name} added"; the created joint becomes the selection
   (`ops::started`, `finish_edit`).

Matches RoboCAD (`ui/app.py:robot_add_joint` 1533, `robot_joint_dialog`
1540, `_robot_joint_from` 1552; `ui/tools.py:JointTool.press` 1315).
Deliberate difference (recorded): Ctrl+Shift+J is bound; Ctrl+Shift+M stays
Select Same Material.

### CAD-146 The other robot tools and dialogs

1. Each Robot menu entry (registry 453-461) or Robot panel button
   (`robot/panel.rs:59-70`) → `CadInvoke` → the catalogue entry
   (`ops/catalogue/robot.rs`: `robot.infer` 82, `robot.assign_motor` 91,
   `robot.fixed` 102, `robot.ground` 113, `robot.add_sensor` 124,
   `robot.add_cable` 142, `robot.power` 161) → its form, or run at once.
2. Assign motor without motors or joints is refused before its dialog:
   `ops/robot_form.rs:precheck` (176: "add a motor and a joint first", as
   `ui/app.py:robot_assign_motor` 1583-1615).
3. `ops/robot_args.rs:build` → one plan per entry (`Infer` 206,
   `AssignMotor` 207, `Fixed` 216: one `connect_fixed` per child, `Ground`
   223: per body, read then toggled, `Sensor` 230, `Cable` 239: mass g →
   kg, `power` 282: `set_battery` or `set_robot_setting("battery", None)`,
   then `set_control` with every motion joint's target (one left out keeps
   its current target, 303-312) and `set_uncertainty`) → `send` (344) →
   edit leg → `commands.py` `infer_joints` 1023, `attach_motor` 992,
   `connect_fixed` 960, `set_ground` 1016, `add_sensor` 1047, `add_cable`
   1062, `set_battery` 1145, `set_robot_setting` 1090, `set_control` 1151,
   `set_uncertainty` 1162.
4. Shown: RoboCAD's status texts (`robot_args.rs:361-396`: "{n} joint(s)
   inferred …", "{motor} now drives {joint}", "{n} bodies fixed to …",
   "ground toggled on …", "sensor … added", "cable … added"); the robot
   reads leg refreshes `/robot`, `/sensors`, `/cables`, `/battery`,
   `/control`, `/uncertainty`.

Matches RoboCAD (`ui/app.py` 1577-1674). Deliberate difference (recorded):
the power dialog's targets are one JSON field, and a joint left out keeps
its target.

### CAD-147 Export physical model

1. Simulation ▸ Simulation: export physical model (simrobot v4, with
   flexible links)… (registry 465) → `results::command_action` (462) →
   `cad_results {op: export, kind: physical}` → `results/mod.rs:handle`
   (344-356): without a path the path form opens (`results/forms.rs:open`,
   155; `default_path` 106: `<stem>.simrobot.json`); OK →
   `export::model_file` (`results/export.rs:105`: `.simrobot.json` appended
   unless it ends in `.json`, never the `.rcad`) → `export::request` (124).
2. `admit` (112): a second export while one runs is refused with RoboCAD's
   "a model export is already running" (35); the live link's queues (latest
   wins). `start` (141): one `Job::spawn(Pool::Dedicated, …, "RoboCAD
   export: …")` (147), the shown revision recorded (156) → status
   "exporting physical model in the background…".
3. The job: `crates/sim-runtime/src/cad_client/physical.rs:physical_model`
   (190, `GET /physical?flex=1`) → `api.py:1765-1778`: a snapshot on
   RoboCAD's GUI thread, derived in a child process
   (`export_worker.py:export_snapshot`, 33 → `main`, 15 →
   `physical.py:export_physical_model`, 859, simrobot v4) → back in the job,
   `ctx.cancelled()` (150) and `write_model` (177): a per-job temporary file
   beside the target, the cancel checked again just before the rename
   (188-191), then `rename` (as `export_worker`'s `os.replace`).
4. Progress: `export::poll` (231; from `results/link.rs:receive`,
   JobResults) writes "exporting physical model in the background… n s" once
   a second (235-242), and the results panel shows it
   (`results/overlay.rs:340-341`).
5. **Cancel export** (`cad:results:export_cancel`, `results/mod.rs:487`;
   panel `overlay.rs:346`) → `export::cancel` (200): sets the job's cancel
   flag, keeps the job until its terminal outcome, status "physical model
   cancellation requested; waiting for the export outcome". Outcomes in
   `poll`: any failure after a cancel request (276-282) → "physical model
   export cancelled: nothing was written to {path}", followed by the job's
   error in parentheses unless it is plain "cancelled" (e.g. `crate::jobs`'
   "RoboCAD export: physical model was cancelled before it started.",
   `jobs/mod.rs:206`, when the cancel came before the closure ran). Every
   `Err` of the job comes before or instead of the rename (`physical_model`,
   the check at 150, `write_model`'s returns at 178-195), so nothing was
   renamed and the previous file and `written` are untouched; cancelled too late → "physical model
   written: {path} (n links, m flexible); cancellation arrived after the
   final publication check and could not revoke the write"; queued exports
   start only after the terminal outcome (`start_queued`, 211; 290).
6. **Generation/revision guard**: `poll` compares the job's generation
   with the document's (`same_document`, 249): an export of an older
   connection or document is reported truthfully (256) but neither returned
   as `Landed` nor recorded as `written` (264-269), so it never drives the
   live link, Robot mode or "Show in Robot mode" (`link.rs:shown_model`:
   with the link off it reads `exports.written`, 113, which only this
   document's export sets; with the link on it opens the link's
   `.simrobot.json`, 108-112); an export whose shown revision changed while it
   ran says "; the export started at revision R and the document changed
   while it ran (now revision N), so later edits may not be in it" (258).
   R is the revision the viewer showed when the export was asked for
   (`Running.revision`, 64-68); RoboCAD builds the model when the request
   arrives, which the viewer does not observe, so the text claims no more
   than that.
7. Leaving CAD mode, or opening another document, is refused while an
   export runs or is queued: `CadDocument::switch_blockers`
   (`cad/document/state.rs:206`, "a model export is running: {label} to
   {path}; wait or cancel it"), checked by the mode switch
   (`app/switch/prepare.rs:leaving_blockers`, 79-82).

Matches RoboCAD (`ui/app.py:sim_export_physical` 1700 and
`_export_in_background` 1711-1767: the same "exporting {label} in the
background… n s", "{label} written: {path} ({n} links, {m} flexible)",
"{label} export cancelled", "a model export is already running").
Deliberate difference (recorded): the viewer writes the file on a job from
RoboCAD's `GET /physical` answer, and a cancel cannot stop RoboCAD's
derivation. Gaps found and fixed: a cancel before publication was reported
as "physical model export failed: cancelled" (an error, not RoboCAD's
cancelled outcome, and not saying nothing was written)
(`results/export.rs:270-282`, which also covers `crate::jobs`' "cancelled
before it started" text, earlier reported as a failure); the progress line
overwrote the cancel request each second without saying a cancel was
pending (`results/export.rs:238-240`, `results/overlay.rs:248,340-341`); an
export that landed after a reconnect or another document's open drove the
live link and Robot mode from the old document and became the model "Show
in Robot mode" opens, and an edit during the export was not mentioned
(`results/export.rs:64-68,156-157,245-269`).

### CAD-148 Export robot model (planar)

1. Simulation ▸ Simulation: export robot model… (registry 466) →
   `command_action` (463) → `{op: export, kind: simulation}` →
   `ExportKind::shape` (`results/mod.rs:129`: flex true, planar true,
   "simulation model") → as CAD-147.
2. `physical_model(true, true)` → `GET /physical?flex=1&planar=1` →
   `api.py:1766-1778` (`planar` → `export_snapshot(…, planar=True)` →
   `export_worker.main`, `Plane.xz()`).
3. Shown: "simulation model written: …".

Matches RoboCAD (`ui/app.py:sim_export`, 1705-1709: `planar=True`, flexible
links).

### CAD-149 Stress overlay

1. Inspect ▸ Toggle stress overlay (registry 464 `view.stress`), Print ▸
   Strength overlay on/off (441 `print.overlay`), or the Robot panel's
   **Stress overlay** (`robot/panel.rs:73`) → `{op: overlay}` /
   `{op: print_overlay}` → `results/mod.rs:overlay` (366): toggles
   `doc.results.overlay`; status "stress overlay on: {SCALE}, from the
   loaded results ({staleness})" (370) or "stress overlay off".
2. No RoboCAD edit: the colours are display only. `results/overlay.rs:
   paint` (127; SimSync) colours each drawn body whose results block has a
   hotspot (`inputs_of`, 59) through `sim_domain_robot::stress_results::
   link_colours` (`cad_colours`, 75; the node's mass centroid read on a job
   when the block has no com, `centroid_m` 231).
3. Staleness: `results/mod.rs:staleness` (245) from RoboCAD's `stale`
   flag (`physical.py:load_results`, 999).
4. Shown: the coloured bodies and the results panel (`overlay.rs:panel`,
   260) with the legend and the staleness line.

Matches RoboCAD's coloured links and stale flag. Deliberate difference
(recorded): Robot mode's log scale (blue at 0.1 % of yield → red at
yield), RoboCAD linear (`ui/viewport.py:_stress_colors`, 826), so
mid-range colours differ.

### CAD-150 Load results

1. Robot ▸ Robot: load simulation results… (registry 462) →
   `command_action` (457) → `{op: load}` → `results/mod.rs:handle` (319):
   the path form at `<stem>.simresult.json` (`forms.rs:default_path`, 106,
   from `doc_path`); OK → `edit_noted` (267; `Waiting::Load`) → edit leg.
2. `cad_client/physical.rs:load_results` (173) `POST /results/load` →
   `api.py:1784-1786` → `commands.py:load_results` (1239) →
   `physical.py:load_results` (999): each block hung on its node, `stale`
   from the physical hash.
3. `results/mod.rs:settle` (281; from `link.rs:receive`): the edit's own
   success (same generation and `edit_seq`, status Ok) turns the overlay on
   and invalidates the robot reads (292-295), which read `/results/nodes`
   again.
4. Shown: "results loaded: {path} (stress overlay on; margins in the Robot
   panel)" (323); the Robot panel's margins (`panel.rs:margin_text`, 168);
   the overlay on.

Matches RoboCAD (`ui/app.py:robot_load_results`, 1676-1687: the same status
text, the overlay on). Deliberate difference: the form always starts at
`<stem>.simresult.json` (RoboCAD falls back to the folder when it is
missing; the form's listing shows whether it exists).

### CAD-151 Apply identification

1. Robot ▸ Robot: apply identified joint parameters… (registry 463) →
   `{op: identify}` → `results/mod.rs:handle` (328-339) → `edit_noted`
   (`Waiting::Identify`) → edit leg.
2. `cad_client/physical.rs:apply_identification` (184) `POST
   /identification/apply` → `api.py:1792-1793` →
   `commands.py:apply_identification` (1244) →
   `physical.py:apply_identification` (1046): stored in
   `robot_settings.identification`; a missing file raises in RoboCAD and
   `api.py:_handle` (1824-1830) answers 500 "FileNotFoundError: …".
3. Shown: "identified parameters stored for {joints}; they ride along with
   the next export" (335), or RoboCAD's error verbatim inside "RoboCAD POST
   /identification/apply: …" (`cad_client/mod.rs:191-200`); `settle`
   invalidates the robot reads (296).

Matches RoboCAD (`ui/app.py:robot_apply_identification`, 1689-1693).

### CAD-152 Live link into Robot mode

1. Simulation ▸ Simulation: live link (registry 467) → `command_action`
   (464) → `{op: link}` → `results/link.rs:toggle` (67): an unsaved
   document (no path from RoboCAD's health or the opened file,
   `results/mod.rs:doc_path` 225) is refused with RoboCAD's text "Save the
   document first: the link watches the saved file" (`UNSAVED`, 52; the
   control `cad:results:link` is disabled with it, `results/mod.rs:490`).
2. On: `export::request(link_request)` (`link.rs:62`: `<stem>.simrobot.json`,
   flex false, planar true, "live simulation model", queued behind a running
   export) → CAD-147's job; `link_switch` set (83); status "Simulation link:
   the model is re-exported on every save and Robot mode shows it".
3. Written: `link.rs:receive` (209) → `export::poll` → `after_write` (146):
   the first written link model, when `switch_refusal` (128 =
   `switch_blockers` + the sketch blocker) names nothing, sets
   `switch_to`; `receive` (270-274) writes `switch_action` (200):
   `Act<WindowAction::Switch(ModeSwitch { mode: Robot, document:
   Path(model) })>` → `app/switch/mod.rs:handle` (367; `Switch` arm 411)
   → `start` (514;
   `leaving_blockers` refuse naming an export running or queued, an edit in
   flight, …) → `app/switch/prepare.rs` (241-249: the `.simrobot.json` file
   opened by `RobotView::open`) in this window, no process started. When
   the switch would be refused, the request is kept and the status line
   says why (`after_write`, 157-161).
4. Back in CAD, change a joint limit (CAD-136), Save (Ctrl/Cmd+S: registry
   `file.save`, 301 → `CadSave` → `cad/files/mod.rs:save`, 378 →
   `results::note_save`, 393, recording the link watched as the save
   started) → `POST /save/thumbnail` (`cad_client/files.rs:152` →
   `api.py:save_with_thumbnail` 1296) → `results/mod.rs:settle` →
   `link::saved` (94) → the link's model is exported again → `after_write`
   → `follow` (170): the registry's Robot entry is bumped, so the switch to
   Robot mode reads the new file (`prepare.rs:241`) and shows the new
   limit.
5. While that export runs, leaving CAD is refused naming it
   (`switch_blockers`, `cad/document/state.rs:206`; "Show in Robot mode" too,
   `link.rs:show_target` 119).

Deliberate difference (recorded): RoboCAD's live link exports and starts
`sim-spatial --robot` as a separate process (`ui/app.py:sim_link_toggle`
1769, `simbridge.py:SimLink` 213-262); the viewer opens Robot mode in the
same window. The refusal text and the exported model (flex off, planar
hint, "live simulation model", queued: `simbridge.py:243`) match RoboCAD.
With the CAD-147 guard (`results/export.rs:249,264-269`), a link export
from an older connection or document no longer switches this window, moves
the Robot entry or becomes the model "Show in Robot mode" opens
(`link.rs:shown_model`: with the link off it reads `exports.written`, 113,
which only this document's export sets; with the link on it opens the
link's `.simrobot.json`, 108-112).

## Reading traces — Part H

Each trace follows one Part H step (CAD-153 to CAD-168) from the native
control to RoboCAD and back to what the status line, the Print jobs
section, the results panel and the 3D view show. Everything here is **by
reading, unexecuted**: nothing was built, run or captured, and no step was
compared side by side. Native paths are under `crates/sim-spatial/src/`
unless they start with `crates/`; RoboCAD paths are under `cad/robocad/`.
Gaps found while tracing were fixed in commit 0caaf92e (each named in its message), unless an entry says it is recorded.
The common legs are written out once:

- **Menu leg** (every Print entry): the registry row
  (`cad/surfaces/registry.rs:386-387`, `:432-442`; View ▸ Build Plate
  Preview `:341`) → its key (`cad/keys.rs:392-401`: `registry::ready`, then
  `CadInvoke {id}` or the row's `Native::Action`) or the menu / palette
  row → `Act<CadAction>` drained by CAD's one apply system
  (`cad/actions.rs:apply`, 354) → `handle` (459): `CadInvoke`/`CadRun`/
  forms to `cad/ops/mod.rs:handle` (491-500), `CadPrint` to
  `print::handle` (`cad/actions.rs:581` → `print/mod.rs:235`). A catalogue
  form (`Flow::Form`) checks the selection first, then
  `print::precheck` (`cad/ops/invoke.rs:58-76`), opens with the remembered
  values (`print::seed`, `print/mod.rs:154`); its OK → `ops::form::submit`
  → `ops/mod.rs:run` (528) → `prepare` (543; `commit_refusal` first) →
  `args::build` (`cad/ops/args.rs:560`; `Shape::Print` arm 570) →
  `print::build_plan` (`print/mod.rs:125`) → `start` (`ops/mod.rs:584`;
  `Built::Print` arm 611) → `print::send`
  (`print/mod.rs:136`). A `Flow::Immediate` entry (Validate, Check
  strength, Plan, Whole or split, Assembly) runs at once
  (`cad/ops/invoke.rs:15`).
- **Edit leg** (every RoboCAD write: fastener, clearance, every study
  start): `cad/edit.rs:edit_at` (48; refused by name with nothing sent by
  `cad/document/state.rs:commit_refusal` (138) when an edit is in flight,
  the window is not connected, the shown document is behind RoboCAD's, or
  RoboCAD's revision moved since the form, pick or selection was read) →
  `cad/sync/mod.rs:start_edit` (686; one `Pool::Dedicated` job,
  `Job::spawn` 693) → `finish_edit` (565) → `print::edit_answered`
  (called at `cad/sync/mod.rs:586` → `print/jobs_tracker.rs:edit_answered`,
  215) → `sync::refresh` (641)
  reads `/doc` back. The status line is `CadDocument::show`
  (`cad/document/state.rs:261`).
- **Read leg** (wall check, validation, registry, study): one
  `crate::jobs::Job` on `Pool::Dedicated` (`crates/sim-spatial/src/jobs/mod.rs:60-63`,
  `spawn` 183, `poll` 239; dropping a job cancels it, 260) polled by a
  JobResults system that runs after `CadSet::Results`
  (`print/checks.rs:receive` 446 and `build_core` 440;
  `print/studies.rs:tick` 341, `build_core` 410). Nothing is read on the
  UI thread.
- **Print job leg** (split, strength, plan, whole-or-split, assembly,
  coupons): start → poll → terminal state.
  1. Start: `print/studies.rs:send` (269) makes ONE call through the edit
     leg (`CadClient::print_split_job` or `print_start`,
     `crates/sim-runtime/src/cad_client/print.rs:415`, `:421`; RoboCAD
     `api.py:1588-1591` → `api.py:print_request` (307-341) →
     `print_jobs.py:PrintJobs._start` (108-126) on its own thread); the
     edit's sequence is noted (`studies.rs:277`).
  2. Adopt: the edit's answer is the job (`Job.public`,
     `print_jobs.py:42-45`); `jobs_tracker.rs:edit_answered` (215) watches
     it, stamped with the poll counter, and asks for a poll.
  3. Poll: `jobs_tracker.rs:tick` (425; JobResults, after
     `CadSet::Results`, `build_core` 506) starts at most one `GET
     /print/jobs` on a `Pool::Dedicated` job (470;
     `cad_client/print.rs:print_jobs` 428 → `api.py:330-331` →
     `print_jobs.py:list` 133) every `POLL_INTERVAL` 0.5 s (57) while a
     watched job runs or the section is open (468). Never a thread per
     job, never `wait` (RoboCAD ignores it on a GET).
  4. Progress: `land` (317): a watched job still `queued`/`running`
     (`cad_client/print.rs:running` 239) writes `progress` (155) "kind:
     message (n %)" only when it changed (341, 352).
  5. Terminal state: `finish` (366) maps RoboCAD's states
     (`print_jobs.py:29`, set at 113-119) one to one: `"done"` (376) →
     the kind's done text (`done_text`, 167, RoboCAD's
     `ui/app.py:1183-1262`), the guide or folder opened through
     `jobs::open_local` on a `Pool::Io` job (380), and for a publishing
     kind (`publishes`, 208) `sync::refresh` plus a robot re-read;
     `"failed"` (395) → the status line's **error** "Kind: error"
     (RoboCAD's warning box title and text, `ui/app.py:1159-1160`);
     `"cancelled"` (396) → `published_revision` (416): when the work had
     already published before the cancel landed, "{kind} cancelled after
     RoboCAD had already published its result as one undo step (revision
     R); Ctrl+Z undoes it" (401) and `/doc` and the robot are read again
     (400-404); otherwise plain "{kind} cancelled" (405, RoboCAD's
     `ui/app.py:1161-1162`); any other state (409) → an error naming it;
     a watched job RoboCAD stopped listing (370-371) → an error. Only the
     `"done"` arm shows the done text or opens a file, so a cancelled or
     failed job never reads as done.

### CAD-153 Wall thickness check

1. Print ▸ Wall thickness check… or Ctrl+W: registry `print.wall_check`
   (`cad/surfaces/registry.rs:432`) → menu leg → catalogue entry
   (`cad/ops/catalogue/print.rs:59-70`: "Flag walls thinner than (mm):",
   1.2, 0.1–20, 2 decimals) → the form, preset by `checks.rs:seed` (376)
   with the last threshold a check ran (`CadDefaults::wall_threshold`).
2. OK → `checks.rs:build` (198): the selected nodes, else every visible
   body (`visible_bodies`, 193: kind body/sheet and `effective_visible`,
   RoboCAD's `doc.bodies(True)`, `document.py:430-438`, served as
   `api.py:111`) → `checks.rs:send` (213) → `start_wall` (245).
3. `split_cached` (231) answers nodes read at (generation, shown
   revision, threshold) from the cache; the rest go on one
   `Pool::Dedicated` job (259) calling `CadClient::thin_walls`
   (`crates/sim-runtime/src/cad_client/print.rs:446`; a 404 is RoboCAD's
   `if b is None: continue`) → `GET /nodes/{id}/thin?threshold=`
   (`api.py:1654-1655` → `api.py:thin` 835-837 →
   `printing.py:wall_thickness`). All cached: it lands at once with
   nothing sent (`cad_state.print.checks.cached_reads`, 427).
4. `receive` (446; a redraw is requested while a read is out) →
   `land_wall` (309): status `wall_status` (147) "N thin region(s) under
   1.2 mm" / "No walls thinner than 1.2 mm" with Python's float text
   (`py_float`, 126; RoboCAD's `ui/app.py:1126`).
5. Shown: `thin_overlay.rs:draw` (25; Present) draws each point at
   RoboCAD's (1.0, 0.2, 0.2) and 9 px (14-17; `ui/app.py:1123`), only
   while `checks.rs:drawn` (368) sees the shown revision: after any edit
   they are no longer drawn. `cad_print {"op":"clear"}` → `checks.rs:clear`
   (385) empties them (the counts stay in the state).

Deliberate difference: the form reopens at the last threshold (RoboCAD:
1.2 each time). Gap found and fixed: a whole remembered threshold reopened
as "2.0" (Python's float text) instead of the spin box's "2"; the seed now
shows it with the field's 2 decimals as the Fastener and Clearance forms do
(`crates/sim-spatial/src/cad/print/checks.rs:376-381`).

### CAD-154 Validate for printing

1. Print ▸ Validate for printing or Ctrl+Shift+V: registry
   `print.validate` (`registry.rs:433`), entry `catalogue/print.rs:71-80`,
   `Flow::Immediate` → `checks.rs:build` (208): every visible body
   regardless of selection (RoboCAD's `validate`, `ui/app.py:1129-1135`).
2. `start_validate` (280): one `Pool::Dedicated` job (286) calling
   `CadClient::validate_node` (`cad_client/print.rs:457`) → `GET
   /nodes/{id}/validate` (`api.py:1650-1651` → `api.py:validate`,
   825-828: the kernel's report only).
3. `land_validation` (356) → `validation_lines` (168) and
   `validation_status` (187): "n body(ies): valid and watertight." or the
   lines "name: message near (x, y, z) — fix" (`issue_line`, 153; as
   `printing.py:validate_for_export` 138-148), joined by "; " on the
   status line as an error. Hiding a body drops it from
   `visible_bodies`, so n follows.

Deliberate difference: the answer is the status line, not a box, and
there are no open-edge lines (`printing.py:149-153` runs the tessellation
check in RoboCAD's desktop only; `cad_state.print.checks.open_edge_check`,
`checks.rs:44`). A failing body RoboCAD gives no issue for gets a line
saying so (`checks.rs:176-179`; RoboCAD would list nothing). No gap found.

### CAD-155 Overhang shading

1. Print ▸ Toggle overhang shading (`registry.rs:434`, `Do::Print`) →
   `print::command_action` (`print/mod.rs:225-228`) →
   `CadDisplay {toggle: Overhangs}`; the toolbar chip **Overhangs**
   (`cad/display/ui.rs:197`) writes the same. View ▸ Build Plate Preview
   / Ctrl+Shift+B (`registry.rs:341`) → `DisplaySetting::BuildPlate`.
2. `cad/display/mod.rs:apply_display` (338): a plate toggle sets
   `overhangs` to the plate's new state (372-377; RoboCAD's
   `toggle_build_plate`, `ui/app.py:1072-1077`); the shading toggle flips
   only itself (RoboCAD's `toggle_overhangs`, 1079-1083).
3. `cad/display/section.rs:overhangs` (225; `printing.py:overhangs`
   206-220, 45°) builds a tint mesh in 0.9, 0.35, 0.3 (48, 296, 382;
   `ui/viewport.py:818`). Display state only: no RoboCAD call, no undo step.

Matches RoboCAD. No gap found.

### CAD-156 Fastener hole

1. Print ▸ Fastener hole… / Ctrl+H: `registry.rs:386` → entry
   `catalogue/print.rs:23-43` (`Flow::PrintPick`) →
   `cad/ops/invoke.rs:101`: the tool goes active in face mode with its
   form beside the view, preset by `edits.rs:seed` (198; the remembered
   `FastenerDefaults`, RoboCAD's `last_fastener`, `ui/app.py:889-894`).
2. A click: `fastener_tool.rs:click` (151; `InputSet::Window`): a left
   press over the 3D view, not Alt, not a surface's closing press;
   `ray_hit` (179) and the face only through `CadMeshes::face_at` at the
   shown revision (182); a miss returns with nothing written; the snap
   (vertex, midpoint, centre, endpoint) as `ui/tools.py:1120-1155` →
   `Act::ui(CadPrint {op: pick, item, picked_at})` (192).
3. `fastener_tool.rs:pick` (104): `check_pick` (86; the tool active, a
   face of the shown tree, `picked_at` the shown revision, each refused by
   name) → the form's texts or the remembered ones → `point_for` (58) →
   `ops::run_entry_on` (131; explicit items, so `started` leaves the
   selection alone, `ops/mod.rs:662-667`) → `edits.rs:build` (99; depth 0
   → `None`, "through") → `edits.rs:send` (131) → edit leg (140) →
   `CadClient::fastener_hole` (`cad_client/print.rs:469`) →
   `commands.py:fastener_hole` (922-931): one `Composite(spec.label)` undo
   step per click.
4. Status line "M4 counterbore hole in NAME" (`edits.rs:135`;
   `FastenerSpec::label`, `cad_client/print.rs:383`); the values are
   remembered once the run starts (141-144), so the reopened form shows M4,
   counterbore, 0.1, 0.

Deliberate difference: the fields stay beside the view while you click,
with a Point field for REST. Matches RoboCAD otherwise. No gap found.

### CAD-157 Clearance offset

1. Print ▸ Clearance offset… / Ctrl+Shift+C: `registry.rs:387` → entry
   `catalogue/print.rs:44-58` (`needs: FACES`, refusal "Select holes,
   bosses or faces to offset", RoboCAD's `ui/app.py:897-899`) → the Form
   flow resolves the selection first (`cad/ops/invoke.rs:58-63`), so
   with no face selected nothing opens; the form is preset by
   `edits.rs:seed` (209; 0.2 at first, `commands.py:271`).
2. OK → `edits.rs:build` (112-125): faces grouped by node in first-seen
   order (RoboCAD's `by.setdefault`) → `send` (147) → one edit job (158)
   calling `CadClient::clearance` (`cad_client/print.rs:463`) once per
   node → `commands.py:clearance` (903-920): one "Clearance" undo step
   each (two here).
3. Status "Clearance 0.3 mm on N face(s) of A, B (2 calls, each its own
   RoboCAD undo step)" (154-157); the amount is remembered (174-177) and
   the form reopens at 0.3.

Matches RoboCAD. No gap found.

### CAD-158 Split for printing (dovetail)

1. Print ▸ Split selected for printing… (`registry.rs:435`) → entry
   `catalogue/print.rs:81-93` (`nodes(1, Some(1), body|sheet)`, refusal
   "Select one body to split.") → the Form flow checks the selection
   before the registry (`invoke.rs:61-69`), as RoboCAD checks it before
   loading the registry (`ui/app.py:1170-1174`); `studies.rs:precheck`
   (305) then refuses by name while the registry is unread.
2. Printer list: `studies.rs:picks` (286) "printers" from the registry
   read once per generation on a `Pool::Dedicated` job (383;
   `CadClient::print_registry`, `cad_client/print.rs:395`, order kept by
   `get_ordered`; `api.py:313-318`), labelled `PrinterInfo::label`
   (`cad_client/print.rs:158`: "id (x × y × z mm)" with `:g`, RoboCAD's
   `ui/app.py:1175`). Joints from `SPLIT_JOINTS` (auto, pins+screws,
   dovetail, pins).
3. OK → `studies.rs:build` (193-201): `SplitRequest {background: true,
   expected_revision}` → `send` (269) → print job leg (start:
   `print_split_job`, `api.py:326-327` → `print_jobs.py:split_job`
   158-160 → `split` 162-184).
4. Progress "split: cutting NAME for the PRINTER (0 %)"; done →
   `SplitDone::status` (`cad_client/print.rs:324`) "split into N pieces;
   hardware: …" (RoboCAD's `ui/app.py:1183`), the document read back.

Deliberate difference: the start carries `expected_revision` (RoboCAD's
menu sends none), so a moved document ends the job failed with RoboCAD's
revision-conflict text instead of splitting a different body. Unverified:
RoboCAD counts selected nodes that have a body of any kind; the catalogue
needs exactly one body or sheet node, so a body plus a group is refused
here where RoboCAD splits the body.

### CAD-159 Check strength

1. Print ▸ Check strength (`registry.rs:436`, `Flow::Immediate`) →
   `studies.rs:build` (202-216): the print study as read at the run's
   revision (`study_at`, 123; the study read on its own Dedicated job per
   (generation, shown revision), 388, `CadClient::print_study`
   `cad_client/print.rs:399` → `api.py:319-325`).
2. No study (`PrintStudy::has_study`, `cad_client/print.rs:202`, Python's
   `if not study`) → `NO_STUDY` (`studies.rs:43`), RoboCAD's text
   word for word (`ui/app.py:1190-1193`), nothing sent.
3. With it: the study as the body plus `expected_revision` → print job leg
   with kind `analyze` (`print_jobs.py:analyze` 218-247; it publishes the
   "print" blocks, "Strength check").
4. Done → `done_text` (`jobs_tracker.rs:174-185`): "strength: least
   safety factor F on NAME (MODE); Print ▸ Strength overlay shows where",
   the first part with the least safety factor (Python's `min`); the robot
   reads are taken again so the overlay sees the new blocks.

Matches RoboCAD. No gap found.

### CAD-160 Plan with job progress

1. Print ▸ Plan print settings and plates (`registry.rs:437`) →
   `studies.rs:build` (202-216, kind `plan`; no study → `NO_STUDY`, as
   RoboCAD's `print_plan` falls back to `print_strength`'s explanation,
   `ui/app.py:1206-1208`) → print job leg (`print_jobs.py:plan` 250-294).
2. While it runs the status line is `progress`
   (`jobs_tracker.rs:155`) "plan: finding where parts are held and loaded
   (n %)", then "laying out plates", "publishing", each written once
   (352); polls on `Pool::Dedicated` (470), so the window never waits.
3. Done → `done_text` (186-190): "plan: N plate(s), about H h and G g
   (estimates); 3MF files in DIR" (`ui/app.py:1211`).

Deliberate difference: no " — Print ▸ Print jobs… to cancel" suffix (the
section has the Cancel button), and one list poll every 0.5 s replaces
RoboCAD's 250 ms per-job timer. No gap found.

### CAD-161 Whole or split

1. Print ▸ Whole or split for strength? (`registry.rs:438`) →
   `studies.rs:build` (218-241): nothing selected → the entry's refusal
   (`catalogue/print.rs:118`, RoboCAD's `ui/app.py:1220`); otherwise the
   first study part whose node is selected, else the same refusal; its
   printer, material, simulation, safety_target and space with node and
   part (`STRENGTH_SPLIT_KEYS`, 47; `ui/app.py:1222-1223`).
2. Print job leg, kind `strength_split` (`print_jobs.py:297-317`).
3. Done → "RECOMMENDATION: WHY" (`jobs_tracker.rs:191`;
   `ui/app.py:1225`).

Matches RoboCAD. No gap found.

### CAD-162 Assembly guide

1. Print ▸ Assembly guide for the selected split… (`registry.rs:439`) →
   `studies.rs:build` (242-250): `split_groups` (169; selected split
   groups, else the split groups of the selected pieces' parents, from
   `GET /print/study`'s `splits`), none → the refusal "Select a split (the
   group Split for printing made) or one of its pieces."
   (`catalogue/print.rs:128`, `ui/app.py:1235`).
2. Print job leg, kind `assembly` (`print_jobs.py:320-342`; with the
   exploded view it publishes).
3. Done → "assembly: N steps; guide PATH" (`jobs_tracker.rs:192-195`)
   and the guide opened with the system's opener on a `Pool::Io` job
   (380; RoboCAD's `QDesktopServices.openUrl`, `ui/app.py:1240`).

Matches RoboCAD. No gap found.

### CAD-163 Test coupons

1. Print ▸ Test coupons… (`registry.rs:440`) → entry
   `catalogue/print.rs:132-142`: "Printer:" over `printer_ids` and
   "Filament:" over `filaments` in the registry's order (`studies.rs:picks`
   290-291; `ui/app.py:1252-1256`); refused by name until the registry
   is read (`precheck`, 305-314).
2. OK → `studies.rs:build` (251-261): with a selection, its split group
   (the study read is required so a selected split is never silently
   dropped); with nothing selected `group: null`, coupons for the material
   only (`ui/app.py:1259`).
3. Print job leg, kind `coupons` (`print_jobs.py:345-363`).
4. Done → "coupons: N on P plate(s); break them, fill results.json, then
   `sim-print promote results.json`" (`jobs_tracker.rs:196-199`) and the
   protocol's folder opened on a `Pool::Io` job (380).

Matches RoboCAD. No gap found.

### CAD-164 Print jobs and cancel

1. Print ▸ Print jobs… (`registry.rs:442`, `Do::Print`) →
   `print::command_action` (`print/mod.rs:229`) → `CadPrint {op: jobs,
   open: true}` → `jobs_panel.rs:show` (30): the section opens and a poll
   is asked for. The right dock draws it (`cad/panel.rs:465`, rebuilt when
   `jobs_panel.rs:key`, 115, changes, `panel.rs:491`).
2. `jobs_panel.rs:draw` (77): the last eight jobs (`lines`,
   `jobs_tracker.rs:138`, `SHOWN` 59) as RoboCAD's "kind id: state n %
   message" (`line`, 150; `ui/app.py:1270`); none on a listed fresh
   service → "No print jobs yet." (`NO_JOBS`, 20; drawn at 86).
3. **Cancel running jobs…** only while a job runs and no question is open
   (106-108) → `CadButton` → `cad/panel/name.rs:buttons` (71) →
   `Act::ui(CadPrint {op: cancel})` → `jobs_tracker.rs:cancel` (251):
   with `confirm` absent it only opens the question (288-294; nothing
   sent). The section shows "Cancel the running jobs?" (`QUESTION`, 22)
   with **Yes** and **No** (98-104; `system_ui` `cad:print:cancel_yes` /
   `cad:print:cancel_no`, 62-65).
4. **No** → `confirm: false` (253-262): the question closes, "The running
   print jobs were left running.", nothing sent.
5. **Yes** → `confirm: true`: refused while a cancel is out (296); the
   ids of the running jobs, each once (`running`, 128-136: the last list's
   then watched ones it lacks), on ONE `Pool::Dedicated` job (303) sending
   exactly one `CadClient::cancel_print_job` per id
   (`cad_client/print.rs:440` → `DELETE /print/jobs/{id}`, `api.py:337-338`
   → `print_jobs.py:cancel` 136-143); "Cancelling n print job(s)…"; when
   it lands a poll follows (`tick`, 445-454) and the
   job's terminal state comes through the print job leg: "plan cancelled".
   `cad_state.print.jobs.cancelling` shows the cancel in flight. Before
   any list was read in this generation it is refused with
   `UNREAD` and a poll is started (271-280).
6. **Close** (109) → `jobs: open false` (the question closes with it, 37).

Deliberate difference: the question is an inline row of the section, not
a box. Gap found and fixed: a cancel that reached RoboCAD after the job's
work had already published (RoboCAD's `_start` marks it cancelled once the
work returned, `print_jobs.py:116`) read "plan cancelled" while the
document had changed by one undo step, and was not read back; it now says
so, names the revision, refetches the document and the robot reads, and a
state RoboCAD never writes is an error instead of "cancelled"
(`crates/sim-spatial/src/cad/print/jobs_tracker.rs:396-419`).

### CAD-165 Print overlay and staleness

1. Print ▸ Strength overlay on/off (`registry.rs:441`, `Do::Physical`) →
   `cad/results/mod.rs:command_action` (461) → `ResultsOp::PrintOverlay`
   → `results/mod.rs:overlay` (366-380): the same toggle as
   `view.stress`.
2. Colours: `cad/results/overlay.rs:58-64` sends each node whose block
   has section "print" (`print_jobs.py:233-237`, `:281-285`) through
   `print/overlay.rs:inputs` (50): one cell whose stress is the governing
   failure index 1 / safety factor with yield 1, so the shared rule paints
   each part in one colour on Robot mode's scale and the least safety
   factor is reddest.
3. The results line: `print/overlay.rs:panel_line` (77), drawn in the
   results panel under "Stress overlay" (`cad/results/overlay.rs:316-318`)
   and part of its rebuild key (253): "Print strength: N part(s); least
   safety factor F on NAME (current)". The revision comparison is
   `staleness` (60-66): current while the shown revision is at most
   `cad_revision + 1` (63; the publish moves RoboCAD's revision by exactly
   one: `candidates.py:PublishState.apply` 62-66 → `document.py:touch`
   298-306 → `notify("changed")` 287-289, and `revision` is not one of
   `STATE_FIELDS`, `candidates.py:36`); after any edit the shown revision
   passes it and the line ends "; stale (computed at revision R, now M)"
   (`tag`, 96-102).

Deliberate difference: RoboCAD colours each voxel's failure index from
its run folder (`ui/viewport.py:831-840`); the viewer colours parts
uniformly. Gap found and fixed: the stale line read "… on NAME (stale
(computed at revision R, now M))", nested parentheses, and the doc comment
promised "; current"; the line now ends " (current)" or "; stale
(computed at revision R, now M)"
(`crates/sim-spatial/src/cad/print/overlay.rs:73-102`).

### CAD-166 Leaving CAD mode while a job runs

1. The switch (mode switcher, `system_ui` `mode:robot` or REST
   `viewer_mode`) → `app/switch/mod.rs:start` (514; calls
   `leaving_blockers` at 529) → `prepare.rs:leaving_blockers`
   (30) → for CAD `CadDocument::switch_blockers` (`prepare.rs:79-80` →
   `cad/document/state.rs:206`) → `self.print.jobs.blockers()` (223 →
   `print/jobs_tracker.rs:blockers`, 111-116).
2. One line per watched job still `queued`/`running`: "a print job is
   running in RoboCAD: plan (n %); wait for it, or cancel it in the Print
   jobs section" (115), wrapped by `app/switch/mod.rs:refusal` (495-497):
   "Not switching to Robot mode: …. CAD mode stays." Between the start's
   send and its answer the edit in flight holds the switch
   (`state.rs:212-214`), so there is no unguarded frame.
3. Once the poll sees it end (`land`, 336-349) it leaves `watched` and the
   blocker goes; a publishing job's refetch is asked for first
   (`finish`, 381-391) so a stale "saved" is never read in that frame.
4. Not connected: `blockers` returns nothing (112-114; `connected` kept
   by `tick`, 430-431), so a lost service never holds the switch.

Matches the brief (RoboCAD has no other modes). Recorded, not fixed (the
checklist's wording, not code): the plan publishes one undo step, so on
the self-started service of Part H's setup the switch after the plan ends
is still refused, by the unsaved-edits blocker (`state.rs:224-236`), until
the document is saved; and with the service lost that blocker (not the
print one) holds while RoboCAD's saved state can't be confirmed. The step
says so now ("after a save").

### CAD-167 Robot panel row press

1. Window: the row is a kit button carrying `CadButton(select_action)`
   (`cad/robot/panel.rs:450-451`) → `cad/panel/name.rs:buttons` (71) →
   `Act::ui(CadSelect)`. REST: `system_ui {"operation":"activate","id":
   "cad:robot:row:<id>"}` → `cad/ui_api.rs:system_ui` (40; activate 49-56) → the
   control list (`ui_api.rs:19` → `cad/robot/mod.rs:controls` 139 → `robot/panel.rs:controls`, 495-512;
   rows only while the panel is open, 506) → `handle` with the same
   action.
2. The action: `select_action` (`robot/panel.rs:197-199`) =
   `CadSelect {ids: [id], picked_at: doc.robot.data.read_at()}` (the
   revision the description was read at, `cad/robot/data.rs:75-77`).
3. `cad/selection/mod.rs:handle` (108) → `select` (216): `validate`
   (176-190) refuses a node no longer in the shown tree by name
   ("[id, body, 0]: no node id in the shown tree", 183); body items are
   stamped with the current revision (223), so a row of an older read is
   not refused for its revision; the selection is published to RoboCAD's
   `/selection` (234). RoboCAD: `RobotPanel._select`
   (`ui/widgets.py:1357-1361`) → `selection.set_nodes(ids)`.
4. Once the description is re-read without the node its row and control
   are gone, and `system_ui` answers "unknown control cad:robot:row:<id>;
   request controls" (`ui_api.rs:54`).

Matches RoboCAD. Recorded, not fixed (Part G's and the selection's
files): the `picked_at` stamp exists only on the action; `select` drops it
for body items (`cad/selection/mod.rs:223`) and `cad/rest_form.rs:30`
omits it, so nothing observable records where the row came from. The
checklist row now says the stamp is on the action only and never refuses
a row of an older read.

### CAD-168 Partial REST Edit joint

1. `cad_run {"id":"ops.set_joint","params":{"lower":-30}}` →
   `cad/ops/mod.rs:handle` (494) → `run` (528) → `prepare` (543):
   `commit_refusal` (550-552) → `resolve` (the selected joint; entry
   `catalogue/robot.rs:182-194`, `needs nodes(1, Some(1), joint)`) →
   `robot_form::fill_from_joint` (574 → `cad/ops/robot_form.rs:315`).
2. `fill_from_joint`: the parameters not given are filled from
   `description` (126-136), which refuses unless the robot description
   was read at the shown revision ("Edit joint: NAME's current values fill
   the parameters not given (type, parent, …), and the robot description
   is still being read (revision N); try again in a moment", 322-323),
   so nothing is sent; a joint missing from that read is refused by name
   (324). The joint's values are `joint_values` (354-370: type, parent,
   child, pivot, axis, limits in degrees unless prismatic, motor, gear
   ratio, damping, name).
3. A type given that crosses prismatic↔revolute with a set limit not
   given is refused naming the limits to pass and both units (325-331).
4. `values` → `robot_args::build` EditJoint (`cad/ops/robot_args.rs:194-206`;
   `joint_fields` 142-157 converts the degrees) → one `set_joint` call
   with every field, and no `rename` because the name is unchanged (199)
   → edit leg → `POST /ops/set_joint` → `commands.py:set_joint` (949-958).
5. Status "joint NAME updated" (`ui/app.py:1575`); `GET /robot` shows only
   the lower limit changed.

Matches RoboCAD. Recorded, not fixed (the checklist's wording):
RoboCAD's `set_joint` does not need every field; it updates only the ones
passed (`commands.py:955-956`, `d.update({k: v … if k in d})`), so the
curl with only `lower` would do the same. The viewer still sends every
field, read at the shown revision, which is equivalent; the checklist row
now says so.

### Cross-cutting: cad-print reads and jobs never run on the UI thread

Every RoboCAD read or write of this part goes through `crate::jobs::Job`:
thin and validate reads (`checks.rs:259`, `:286`), the registry and study
(`studies.rs:383`, `:388`), the job list and cancels
(`jobs_tracker.rs:470`, `:303`) on `Pool::Dedicated`, opens on `Pool::Io`
(`jobs_tracker.rs:380`), and every edit through `sync::start_edit`
(`cad/sync/mod.rs:693`). No `thread::spawn` and no raw pool spawn in
`cad/print/`.

## Reading traces — Part I (outliner, references, system link)

Each trace follows one Part I step (CAD-169 to CAD-175, CAD-187 to
CAD-196) from the native control to RoboCAD and back to what the model
tree, the References section, the status line or the mode switch shows,
plus four cross-cutting traces. Everything here is **by reading,
unexecuted**: nothing was built, run or captured, and no step was compared
side by side. Native paths are under `crates/sim-spatial/src/` unless they
start with `crates/`; RoboCAD paths are under `cad/robocad/`. The common
legs are written out once (gaps found while tracing were fixed in
commits e7b8393f, 83ebb812, e6c48005 and b75846ff, each named in its message, and in the
review follow-up that each entry names, unless an entry says it is
recorded):

- **Apply leg** (every outliner and references action): a press, key,
  drop or REST `cad_tree` / `cad_references` writes `Act<CadAction>`
  (`CadTree`, `CadReferences`; `cad/actions.rs:312`, 320) → CAD's one apply
  system `cad/actions.rs:apply` (354) → `tree::handle` (582,
  `cad/tree/handle.rs:195`) or `references::handle` (584,
  `cad/references/mod.rs:342`). A window press's refusal is the status
  line (`cad/actions.rs:407-408`); a REST caller gets it as the answer.
- **Edit leg** (every edit): `cad/edit.rs:edit_at` (48; refused by name
  with nothing sent by `CadDocument::commit_refusal` while another edit is
  in flight, when not connected, when the shown tree is behind RoboCAD's,
  or when RoboCAD's revision moved since the row, menu, drag, dialog or
  form was read) → `cad/sync/mod.rs:start_edit` (686): one `Job` on
  `Pool::Dedicated` holding the client, off the UI thread → a
  `sim_runtime::cad_client` call (`crates/sim-runtime/src/cad_client/`
  `organize.rs`, `references.rs`, `system_link.rs`, or `mod.rs`'s `patch`
  294, `delete` 298, `op` 374) → RoboCAD `api.py` (`POST /ops/{name}`
  1668-1670 → `Service.op` 1011-1029, a `KernelError` answered 422;
  `PATCH /nodes/{id}` 1630-1631 → `Service.patch` 748; `DELETE` 1632-1633
  → `Service.delete` 789) → one `commands.py` / `references.py` op pushing
  one command on the stack.
- **Answer leg**: `cad/sync/mod.rs:finish_edit` (565) → the status line is
  the edit's message or RoboCAD's refusal verbatim (601) →
  `references::edit_answered` (590, `cad/references/mod.rs:464`) for the
  references' own follow-ups → `refresh(doc, true)` (630) asks the poll for
  `/doc` → `take_snapshot` (413) installs the new tree → the panels rebuild
  when their keys change (`cad/panel.rs:480-483`: `tree::key`,
  `tree::tools_key`, `references::dock::key`).

### CAD-169 Search

1. The field "Search (Ctrl+F)…" above the tree (`cad/tree/rows.rs:tools`,
   264; the kit field `SEARCH`, `cad/tree/input.rs:26-34`, RoboCAD's
   `outliner.search`, ui/strings.py:20). Typing: `(SEARCH,
   FieldEvent::Changed)` (`cad/tree/input.rs:402-408`) writes `cad_tree {op:
   search, text}` → `cad/tree/handle.rs:203-211` stores it.
2. Shown: `cad/tree/state.rs:shown` (181): with a query
   (`TreeState::query`, 127: trimmed, lowercased as RoboCAD's
   `text().lower().strip()`), `matches` (168-176): every node whose name
   contains it, its descendants and its ancestors, every row open (199) and
   no disclosure chips (`rows.rs:207`) — RoboCAD's `refresh`
   (ui/widgets.py:271-312, `setExpanded(bool(text) or …)`). Clearing the
   field: an empty query, the rows from `tree.collapsed`, which a search
   never changes (`handle.rs:309-311`, 329-331), so the collapse state from
   before returns. "No row matches the search." when none (`rows.rs:156`).
3. Ctrl+F (Cmd+F) with the pointer over the tree dock:
   `cad/tree/input.rs:search` (61; `over_dock`, 54, the dock between the
   top bar and the status bar) clears the key (72) and claims the field
   (73); it runs in `CadKeySet::Focus` after the gate (`cad/tree.rs:177`),
   so `keys::keys` (`CadKeySet::Keys`, `cad/mod.rs:256`) never sees the F.
   Next frame `input::fields` gives the field the keyboard (488-493).
4. Ctrl+F elsewhere (the 3D view, a body selected): `search` returns at
   69, the key reaches `keys::keys` → `tool.fillet` "Ctrl+F"
   (`cad/surfaces/registry.rs:366`; RoboCAD's ui/keymap.json:8) → Fillet
   starts as in RoboCAD. RoboCAD's application shortcut takes Ctrl+F from
   its own field too, so its placeholder's key never reaches the search.
5. Deliberate difference: Ctrl+F over the tree focuses the search (recorded).

### CAD-170 Expand and collapse

1. **Collapse all** / **Expand all** (`cad/tree/rows.rs:tools`, 265-272;
   `cad/tree/controls.rs:44-45`) → `cad/tree/handle.rs:all` (328):
   collapse records every node with children (`state.rs:parents_of_children`,
   205), expand clears the record — RoboCAD's `_expansion`
   (ui/widgets.py:267-269, a node expanded unless recorded collapsed).
2. A group's "+" chip (`rows.rs:207-210`, `TreeOp::Toggle`) →
   `handle.rs:disclose` (307).
3. A rename's refetch: `tree.collapsed` lives on `CadDocument::tree`
   (`state.rs:27`), untouched by `take_snapshot`; only `sync::start`'s
   restart clears the open rename/menu/dialog, keeping the search and
   collapse (`cad/tree.rs:restarted`, 136-147). The collapsed groups stay
   collapsed, as RoboCAD's `_expansion` survives `refresh`.
4. With a search typed: only Expand all and Collapse all are disabled
   ("the rows are all expanded while a search is typed; clear the search
   first", `controls.rs:26-28`, applied at 44-45); New group stays enabled;
   the +/− chips are not drawn (`rows.rs:207`); and the ops are refused by
   name (`handle.rs:WHILE_SEARCHING`, 168; 309-311, 329-331).
5. Deliberate difference: RoboCAD changes the searched view without
   recording it (`_expanded` ignores it while searching); the viewer refuses
   (recorded).

### CAD-171 Shift and Ctrl select

1. A left press on a row: `cad/tree/input.rs:rows` (221) reads Bevy
   picking's `Pointer<Press>` messages (242): Shift/Ctrl/Cmd read at 238;
   `select(&row.id, shift, ctrl)` (262) → `TreeOp::Select` →
   `cad/tree/handle.rs:214-226` → `select_action` (174): Shift is the shown
   rows from the anchor (`state.rs:range`, 213), Ctrl/Cmd toggles the row
   (`toggle`), otherwise the row alone; the anchor moves on a non-Shift
   press (222-224). A plain press on an already selected row is held as
   pending (`input.rs:259-260`) and selects it alone only on its
   `Pointer<Click>` (278-284), so a drag keeps the selection, as Qt's
   ExtendedSelection (ui/widgets.py:250).
2. `CadAction::CadSelect` through `cad/selection/mod.rs:handle` (104, 108),
   the one selection a 3D pick writes; the rows restyle in place
   (`rows.rs:highlight`, 106) and the 3D view lights the same bodies.
3. Pushed to RoboCAD: `selection::publish` (132-144) →
   `sync::push_selection` (`cad/sync/selection.rs`) → `PUT /selection`
   (`crates/sim-runtime/src/cad_client/mod.rs:404`)
   with `(id, "body", 0)` items, as RoboCAD's `_select` sets body items
   (ui/widgets.py:331-339).
4. Matches RoboCAD.

### CAD-172 Rename in place

1. A double-click (400 ms, `cad/tree/input.rs:250-256`) →
   `TreeOp::BeginRename` → `cad/tree/handle.rs:227-239`: `RenameState
   {draft: the name, selected, began: shown revision}`, claim `Rename`; the
   row draws the kit field with the name selected (`rows.rs:215-223`);
   next frame `input::fields` focuses it (494-498).
2. Enter: `(RENAME, FieldEvent::Submit)` (`input.rs:420-426`; the kit
   keeps the keyboard on Submit, `ui_kit/text/input.rs:224-227`) →
   `TreeOp::Rename {text, revision: began}` → `handle.rs:rename` (342):
   stripped (350); unchanged sends nothing and closes the field
   ("… keeps its name", 352-358); empty is refused by name ("A name cannot
   be empty: type one, or Escape to keep the current name", 359-361), the
   field stays open and the status line says why; otherwise the edit leg
   (367): `CadClient::patch(id, {"name"})` → `PATCH /nodes/{id}` →
   `Service.patch` (api.py:756-757) → `Ops.rename` (commands.py:333-334),
   one `SetAttributes("Rename")`; the field closes once sent (368-371).
   Answer leg: "Renamed OLD to NEW"; the tree and the inspector follow the
   new `/doc`.
3. Escape: the kit's Cancel (`ui_kit/text/input.rs:228-232`) →
   `(RENAME, FieldEvent::Cancel)` (`input.rs:427-432`): the rename ends,
   nothing sent.
4. A click elsewhere: the kit blurs the field
   (`ui_kit/text/input.rs:166-178`) → `input.rs:509-512` ends the rename
   without renaming.
5. Deliberate difference: Qt's delegate commits on focus loss and RoboCAD
   renames to an empty name (`_renamed`, ui/widgets.py:341-348); the viewer
   ends without renaming on blur and refuses an empty name (recorded).

### CAD-173 Drag into a group, before a sibling

1. The drag: `cad/tree/input.rs:rows` reads `Pointer<DragStart>` (285-292,
   290: `began` = shown revision), arms past 4 px (`DRAG_SLOP`, 47;
   295-306: the selection when the pressed row is selected, else that
   row), and keeps the target from `DragEnter/DragOver/DragLeave`
   (307-326) through `RowLookup::target` (140), drawn as an outline (into) or a line (before)
   (`rows.rs:marker`, 74; 198-206).
2. The target (`RowLookup::plan`, 149-179): a group row below its top
   quarter (`BEFORE_BAND`, 50) is `Into(group)`; any other row, and a
   group's top quarter, is `Before(row)`; the room under the rows is
   `TopLevel`. `state.rs:move_plan` (238) builds RoboCAD's `_drop`
   (ui/widgets.py:366-381): into a group at its end (index None), before a
   sibling at its parent and `index_of` (223), and refuses "Cannot move a
   group into itself or its descendants (NAME)" (266-272) when the new
   parent is one of the moving nodes or under them (RoboCAD's
   commands.py:384-389).
3. The drop (`input.rs:327-355`): one `TreeOp::Move {ids, parent |
   before, revision: began}` → `handle.rs:247-263` (`move_plan` again) →
   the edit leg: `CadClient::move_nodes` (`crates/sim-runtime/src/cad_client/
   organize.rs:32`) → `POST /ops/move_nodes` → `Ops.move_nodes`
   (commands.py:377-396), one `Composite("Move in outliner")`. "Moved X into
   G" / "to the top level".
4. A group onto itself or under its own child: refused before sending —
   no marker while hovering (`target` is None), and the drop says why in
   the status line with nothing sent (`input.rs:337-344`). `plan` passes
   only that refusal on (165-178: the drop's new parent — the group, or
   the `before` row's parent — is a moving node or under one,
   `state.rs:with_descendants`, 155); `move_plan`'s other refusals
   ("cannot be moved in front of itself", "not listed under its parent"),
   which a short drag ending on the dragged row itself gives, are dropped
   silently as before (RoboCAD's `_drop` would send such a move and RoboCAD
   would take or ignore it, not refuse it by name).
5. Deliberate difference: a group row's top quarter drops in front of the
   group (RoboCAD always drops into a group); recorded.
6. Gap found and fixed: a drop RoboCAD would refuse sent nothing and said
   nothing (`target` dropped `move_plan`'s error); it now shows the
   refusal (`cad/tree/input.rs:149-179`, 337-344). The text now reads as
   RoboCAD's, capitalized "Cannot move a group into itself or its
   descendants" (`cad/tree/input.rs:176`, `cad/tree/state.rs:271`,
   `cad/tree/controls.rs:56`). Review follow-up: the first fix passed every
   `move_plan` error to the status line, so a short drag ending on the
   dragged row said "cannot be moved in front of itself"; only the
   descendant refusal is shown now (`input.rs:165-178`).

### CAD-174 Context menu

1. A right press on a row (`cad/tree/input.rs:265-274`) → `TreeOp::Menu
   {id, open, at}` → `cad/tree/handle.rs:menu` (435): an unselected row is
   selected first through the one selection (449-458), as RoboCAD's
   `_menu` (ui/widgets.py:383-390); the menu's nodes are frozen (460-462);
   a selection change while it is open closes it (`popup.rs:73-76`).
2. The entries, `cad/tree/controls.rs:menu_rows` (105-157), in RoboCAD's
   `_context_menu` order (ui/widgets.py:403-437): Fit in view
   (`view.focus`), Isolate (`view.isolate`), Hide, Show, Lock, Unlock,
   Group selection…, "Move to group" heading over Top level and each group
   path "A / B" without the selection and its descendants
   (`state.rs:group_paths`, 279-294, walk order as RoboCAD's
   `doc.walk()`), Make unique (bake instance) (`modify.make_unique`), Set
   as active group (one group selected, 142-148), Delete (`edit.delete`);
   always Clear active group and Show all (`view.show_all`). The viewer adds
   a rule after the Move to group entries and before Clear active group
   (display only).
3. A press on an entry (`popup.rs:input`, 78-88) writes its action and
   closes the menu; a disabled entry says why. Escape closes the menu and
   is consumed (96-101).
4. Hide / Show / Lock / Unlock → `handle.rs:flags` (376): one
   `set_visible` / `set_locked` op (391, 396; commands.py:336-341), one
   undo step each.
5. Group selection… → `TreeOp::GroupDialog` (`handle.rs:284-302`, the
   "Organize components" dialog with "Group name:", `popup.rs:150-166`) →
   OK or Enter (`input.rs:475-479`, 443-447) → `handle.rs:group` (400) →
   `CadClient::group` (`organize.rs:28`) → `Ops.group`
   (commands.py:354-364), one "Group" step.
6. Move to group ▸ Top level / a path → `TreeOp::Move {ids, parent?}` →
   as CAD-173.
7. Set as active group / Clear active group → `handle.rs:266-282` →
   `CadClient::set_active_group` (`organize.rs:36`) →
   `Ops.set_active_group` (commands.py:398-400), which only notifies
   `"active_group"`: no revision move (document.py:287-289). The row is
   drawn in RoboCAD's blue (`rows.rs:ACTIVE_GROUP`, 25; `name_colour`,
   137-145) and "Active group: NAME" under the tools (273-275).
8. Registry `group.set_active` with no group selected: the catalogue entry
   needs one group (`cad/ops/catalogue/organize.rs:16-19`) and is refused
   "Select a group to make it the active group"; RoboCAD's registry command
   sets `None`, clearing the active group (ui/app.py:427).
9. Deliberate differences: "Move to group" is a heading over indented
   entries, not a submenu; `group.set_active` refuses by name (both
   recorded).
10. Gap found and fixed: because `set_active_group` does not move RoboCAD's
    revision, the refetch after Set as active group / Clear active group
    came back at the same (document id, revision), and `take_snapshot`
    dropped it as "not newer": the blue row and the "Active group:" line
    never changed. The tree is now taken when its active group differs at
    the same key (`cad/sync/mod.rs:425`).
11. Recorded, not fixed: an active group set in RoboCAD's own window shows
    in the viewer only after the next revision change or `cad_refresh`
    (the poll refetches `/doc` on a new revision only); needs a RoboCAD
    change (`Ops.set_active_group` notifying an event that moves the
    revision).

### CAD-175 New group

1. **New group** above the tree (`cad/tree/controls.rs:43`, `TreeOp::GroupDialog
   {ids: []}`) → `cad/tree/handle.rs:284-302`: the dialog for an empty
   group ("OK adds an empty group.", `popup.rs:154`), its field claimed
   (`input.rs:499-503`).
2. `Empty`, OK → `handle.rs:group` (400): ids `[]` → `CadClient::group(&[],
   "Empty")` → `Ops.group` (commands.py:354-364): a group under no parent
   (the document's `add` puts it in the active group when there is one,
   document.py:323, in both), one "Group" step; the dialog closes once sent
   (419-429); "Added the empty group Empty".
3. An empty name: OK is disabled while the draft is blank (`popup.rs:152`,
   `controls.rs:68`); Enter is refused in the dialog "A group name is needed:
   type one, or Cancel" (`handle.rs:404-405`, drawn by `popup.rs:157-159`),
   nothing sent.
4. Deliberate difference: RoboCAD's `_group` just closes on an empty name
   (ui/widgets.py:392-395); the viewer says why (recorded).

### CAD-187 Add reference images

1. View ▸ References: registry `view.references`
   (`cad/surfaces/registry.rs:288`) → `cad/references/mod.rs:command_action`
   (535-541) → `ReferencesOp::Dock {open}` → `dock_shown` (373-384): the
   section (`cad/references/dock.rs:draw`, 85) and a fresh status read.
2. **＋ Add reference images…** (`mod.rs:558`, `BrowseKind::Images`) →
   `browse` (387-405): the path field (`dock.rs:106-125`, kit
   `path_field`), its listing on `Pool::Io` (`input.rs:281-289`). Enter
   or Add (`input.rs:167-172`, 241-243) → `ReferencesOp::Add {paths}` →
   `cad/references/edits.rs:67-83`: `image_path` (51-61) refuses a relative
   path ("type an absolute path (~/ works)", 38-44), a folder or a name
   without png/jpg/jpeg/webp/bmp, nothing sent, the error under the field
   (`settle_browse`, 134-148); otherwise the edit leg with the active plane,
   else XY (73) → `CadClient::import_references`
   (`crates/sim-runtime/src/cad_client/references.rs:181`) → `POST
   /ops/import_references` → `ReferenceOps.import_references`
   (references.py:11-29): locked image nodes 100 mm wide, opacity 0.6, one
   "Import references" step. Status "n reference image(s) added •
   Calibrate scale before tracing" (`edits.rs:78`, references.py:183).
3. Answer: `edit_answered` (`mod.rs:477-485`) makes the last image current
   and the dock shown; `reads::receive` aligns on it once its placement is
   read at the new revision (`cad/references/reads.rs:251-260`, CAD-190's
   align) — RoboCAD's `add_paths` then `align` (references.py:174-184).
4. A file dropped on the 3D view: `cad/references/drop.rs:drops` (47-56;
   `FileDragAndDrop::DroppedFile`, CAD mode only) → the same `Add`; a
   `.step` drop is refused by name by `image_path` with nothing sent.
5. Drawn: `cad/references/planes.rs` (header 1-28): the placements read on
   a `Pool::Dedicated` job (`reads.rs:180-199`, `Job::spawn` 186), the pixels from `GET
   /nodes/{id}/image` decoded on a job (`planes.rs:192`), a quad at the
   placement's opacity (236), as RoboCAD's `_draw_images`.
6. Deliberate differences: one typed path per submit instead of the file
   dialog; non-image paths and drops refused before sending (RoboCAD's
   import fails in Pillow); a drop anywhere on the CAD window imports
   (recorded).

### CAD-188 Visibility

1. The list row's chip (`dock.rs:153-155`; `mod.rs:569-572`:
   `ReferencesOp::Visible {visible: !visible, revision: shown}`) →
   `edits.rs:84-94` → `CadClient::update_reference(id, {visible})`
   (`references.rs:185`) → `ReferenceOps.update_reference`
   (references.py:31-63), one `SetAttributes("Edit reference")`; status
   "NAME hidden" / "NAME shown".
2. The tree's `visible` changes; `planes.rs` despawns or draws the quad
   (only `effective_visible` images are drawn).
3. Deliberate differences: no preview thumbnail (`dock.rs:15-18`); WebP and
   BMP images are listed with the note `dock.rs:FORMATS` (35) and not drawn
   (recorded). RoboCAD's checkbox shows no status text.

### CAD-189 Placement

1. The form (`cad/references/form.rs`): loaded from the placement read at
   the shown revision (`PlacementForm::load`, 94-98; `reads::follow_form`,
   `reads.rs:106`), rows Width, Origin X/Y/Z, Rotation, Opacity with
   RoboCAD's ranges and decimals (`ROWS`, 38-45; references.py:49-57), the
   plane chips (`dock.rs:160-167`) and the lock chip.
2. Typing: `input.rs:138-147` (one kit field for the six rows; Tab moves).
   **Apply placement** (`mod.rs:580-585`, `PlacementForm::apply`, 110-122,
   with `revision: began`) → `edits.rs:placement` (152-181) →
   `form::update` (156-168: every value, opacity / 100, the plane from the
   choice, `plane_value` 133-141) → the edit leg →
   `update_reference(id, width, opacity, origin, plane, rotation_deg,
   locked)` → references.py:31-63, one "Edit reference" step. Status
   "Reference placement updated • Ctrl+Z undoes" (176; references.py:193).
3. A refusal shows under the button (`edits.rs:refused`, 184-190;
   `mod.rs:488-492`).
4. Matches RoboCAD.

### CAD-190 Align view

1. **Align view** (`mod.rs:587`, ready once the placement is read at the
   shown revision) → `mod.rs:360` → `cad/references/align.rs:align` (90):
   `camera` (46-69) is RoboCAD's `align` (references.py:195-209):
   trackball rows [x, y, normal], orthographic, target the image's centre,
   distance `max(h, w / aspect) · 0.6 / tan(fov / 2)`; one
   `CameraAction::Set` (95). Display only.
2. The active plane: `plane_for` (73-87) names the image's plane as XY,
   XZ or YZ, or a plane node it lies in; then it becomes the active plane
   (100, `set_active` 114-121).
3. The tilted plane, its node deleted: `plane_for` finds none → the camera
   is aligned, the active plane left as it is, and the status says so
   ("NAME is not on XY, XZ, YZ or a plane node, so the active plane is
   unchanged", 103-109). **Sketch over this** is refused by name
   (`align.rs:sketch`, 134-136) with nothing started.
4. Deliberate difference: RoboCAD sets the image's own plane as the active
   plane and sketches on it; the viewer's sketch tools take only named
   planes and plane nodes (recorded).

### CAD-191 Calibrate scale

1. **Calibrate scale** (`mod.rs:588`) → `mod.rs:362` →
   `cad/references/calibrate.rs:start` (78-98): the Select tool replaces
   another tool and a form is cancelled (81-85), `align` (86), the tool
   with RoboCAD's hint "Click two points on the image, then type their
   real distance" (67; ui/tools.py:1212).
2. Clicks: `calibrate.rs:click` (Input; 246-276): a left press over the 3D
   view (not over UI, not Alt), the cursor ray met with the image's plane
   (`on_image_plane`, 232-242) → `CalibratePick {point, picked_at}` →
   `pick` (120-151) / `check_pick` (155-178). Picks while the tool takes
   clicks are not selections (`mod.rs:takes_clicks`, 528-530).
3. The same point twice: the second pick is refused with RoboCAD's text
   "Pick two distinct points and enter a positive distance"
   (`DISTINCT`, 46; 171-176), shown in the section and the status line,
   the first point kept.
4. After two points the "Real distance" field opens with the picked
   distance, keyboard claimed (141-145; `dock.rs:198-205`). `100`, Enter
   (`input.rs:185-196`) → `CalibrateDistance` → `distance` (199-229):
   refused before sending for a non-positive distance, coincident points
   or stale picks (`check_distance`, 181-196) → the edit leg →
   `CadClient::calibrate_reference` (`references.rs:193`) →
   `ReferenceOps.calibrate_reference` (references.py:65-74, keeping the
   first point still) → `update_reference`, one step. Answer:
   `edit_answered` ends the tool (`mod.rs:487`); status "Reference
   calibrated • Ctrl+Z undoes" (`calibrate.rs:218`; ui/tools.py:1241).
5. Escape before Enter: in the distance field, the kit's Cancel →
   `input.rs:197-200` → `ReferencesOp::Cancel` (`calibrate.rs:cancel`,
   101-110); with no field typing, `calibrate.rs:escape` (288-298,
   `CadKeySet::EscapeTool`, consuming the key). Nothing is sent; "Calibration
   cancelled".
6. Deliberate differences: RoboCAD accepts the second click and refuses
   at Enter (`_safe` around `calibrate_reference`); the viewer refuses the
   coincident click itself, same text. The distance field is in the
   References section, not the numeric bar (recorded).
7. Gap found and fixed: Escape with a command surface, catalogue form or
   results form open over the calibrate tool closed that and also ended
   the tool (none of those consumes the key); the tool's Escape now stands
   aside for them as the threads' Escape does
   (`cad/references/calibrate.rs:290`; threads also for the results form,
   `cad/threads/input.rs:216`).

### CAD-192 Sketch over this

1. **Sketch over this** (`mod.rs:589`) → `mod.rs:361` →
   `cad/references/align.rs:sketch` (125-141): the placement at the shown
   revision; refused by name when the image's plane cannot be the active
   plane (134-136); `align` (137), then `CadInvoke {sketch.line}` (140) —
   RoboCAD's `sketch` (references.py:216-219: `align`, `SketchTool('line')`).
2. The Line tool draws on the active plane, now the image's.
3. Matches RoboCAD (on XY, XZ, YZ or a plane node; see CAD-190).

### CAD-193 Remove reference

1. **Remove reference** (`mod.rs:590`) → `edits.rs:96-103` →
   `CadAction::CadDelete` (`cad/actions.rs:516-524`) → `CadClient::delete`
   → `DELETE /nodes/{id}` → `Service.delete` (api.py:789-793) →
   `Ops.delete([id])` — RoboCAD's `delete` (references.py:221-222). "Deleted
   NAME".
2. The new tree has no image: `follow_form` moves the current image to
   the first left (`reads.rs:106-152`); the quad is despawned
   (`planes.rs`).
3. Undo (`cad_undo`, `cad/actions.rs:525-527`) restores the node.
4. Matches RoboCAD.

### CAD-194 System status line

1. The line at the top of the section (`dock.rs:95`) =
   `system_link::line` (`cad/references/system_link.rs:64-70`) →
   `SystemStatus::line` (`crates/sim-runtime/src/cad_client/system_link.rs:74-91`),
   RoboCAD's `show_system` texts (references.py:90-103).
2. Read: `reads::tick` (`reads.rs:201-230`) → one `Pool::Dedicated` job
   `CadClient::system_status` (`system_link.rs:107`) → `POST
   /ops/system_status` → `Ops.system_status` (commands.py:1134-1136) →
   `system_link.status` (system_link.py:66-76: `unlinked`). "Reading the
   linked system file…" until it lands (`system_link.rs:30`).
3. Unlinked: "System file: none linked. Link a .system.json to build
   circuits and subsystems for this model."
4. Matches RoboCAD.

### CAD-195 Link, Accept, Unlink

1. **Link system file…** (`mod.rs:559`, `BrowseKind::System`) → the path
   field; Enter → `ReferencesOp::Link {path}` (`input.rs:57`) →
   `edits.rs:104-113` (absolute paths only, 38-44) → `CadClient::link_system`
   (`system_link.rs:95`) → `Ops.link_system` (commands.py:1112-1119:
   `set_robot_setting`, one undo step). Answer: the status is read again
   (`mod.rs:486`) → "System: TITLE · revision n · n definitions".
2. The file edited on disk: no RoboCAD revision moves. RoboCAD's own
   dock reads the status only in `show_system` (references.py:90-103),
   called from its `refresh` (133, on each panel refresh after a document
   change: ui/app.py:1825-1832, 1853-1854) and after Link, Accept and
   Unlink (105-118); there is no timer or file watcher. The native section
   matches that cadence for a desktop window: it reads the status again on
   opening the section (`mod.rs:376`, 397), a link edit's answer
   (`mod.rs:486`), a revision change (the key, `reads.rs:216`) and Refresh
   (`cad_refresh` moves `CadDocument::mesh_retry`, `cad/actions.rs:679`;
   `reads.rs:218`, 226) → "· CHANGED since linked (was revision r)";
   Accept changes becomes ready only then (`system_link.rs:accept_ready`,
   73-81). It is not read on a timer from a desktop window: `system_status`
   is an op, and `Service.op` calls `_refresh` after every op (api.py:1027,
   1505-1509), which rebuilds RoboCAD's outliner and properties and
   repaints its viewport, disturbing a rename there. A headless service
   (`health.gui` false, `_refresh` has no window) is also read every
   `STATUS_PERIOD` (2 s, `cad/references/reads.rs:49`; 219-221) while the
   section is open. An unchanged answer marks nothing changed (210, 214).
3. **Accept changes** → `edits.rs:114-120` → `refresh_system_link`
   (commands.py:1126-1132), one step → the status line without CHANGED.
4. **Unlink** → `edits.rs:121-127` → `unlink_system` (commands.py:1121-1124),
   one step → "none linked".
5. Deliberate differences: the system file is typed in the path field, not
   the file dialog; Accept changes is enabled only when RoboCAD's status
   says "changed" (RoboCAD's button always runs `refresh_system_link`).
6. Gap found and fixed: the status was read only per (generation, shown
   revision) or on reopening the section, so an edit on disk never showed
   CHANGED and Accept changes stayed disabled ("has not changed since it
   was linked") until an unrelated edit; Refresh now reads it again, and
   a headless service's open section re-reads it on a job every 2 s; an
   unchanged answer marks nothing changed
   (`cad/references/reads.rs:201-230`). Review follow-up: the first fix
   re-read every 2 s from a desktop window too, whose `POST
   /ops/system_status` refreshed RoboCAD's window each time; a desktop
   window is now read at RoboCAD's own dock's cadence (step 2).

### CAD-196 Open in builder

1. **Open in builder** (`mod.rs:561`, ready per `system_link::open_ready`,
   114-117) → `mod.rs:367` → `system_link.rs:open_builder` (122-136):
   `builder_target` (99-111) needs a current status; unlinked or missing
   is refused with RoboCAD's "Link an existing system file first"
   (`LINK_FIRST`, 28; 102-104; references.py:124-125), the control disabled
   with that reason; leaving CAD mode's own refusals are checked first
   (`results::switch_refusal`, `cad/results/link.rs:128-132`:
   `CadDocument::switch_blockers` and the sketch blocker only). The mode
   switch's `leaving_blockers` (`app/switch/prepare.rs:68-78`) also refuses
   while a source operation awaits acknowledgement in Experiments
   ("A source operation is awaiting acknowledgement in Experiments",
   `cad/experiments/mod.rs:171-177`) or a motion export runs ("Cancel
   motion export and wait for its terminal receipt before changing
   modes/documents", `cad/motion/mod.rs:124-130`); those two are not
   checked here, so Open in builder accepts and the refusal shows on the
   mode switcher's line (step 2).
2. Accepted: `switch_to` set (130) and the status "Switching to Build mode
   on PATH (the mode switcher's line says if the switch is refused)" (133);
   `reads::receive` (`reads.rs:261-266`) writes
   `system_link::switch_action` (139-141): `Act<WindowAction>::Switch
   {Build, Document::Path}`.
3. `app/switch/mod.rs:handle` (367) → `start` (514-573) →
   `leaving_blockers(Cad, Build)` (`app/switch/prepare.rs:67-84`: an
   Experiments source operation awaiting acknowledgement (68-71), a motion
   export (75-78), then `CadDocument::switch_blockers` (80: an edit in
   flight, a self-started service's unsaved edits, a model export or
   print job running, …) and the sketch blocker (82)) → `prepare` Build
   (190-217: the build loader on a `Pool::Compute` job, or the builder in
   the window) → `finish_load` (`app/switch/arrival.rs:41-72`) → `enter`
   (22-36) → `leave_cad` (`app/switch/leave.rs:91`) → THIS window is in
   Build mode on the file. No process is started.
4. Deliberate difference: RoboCAD starts `sim-spatial --system …
   --schematic` (references.py:126-130); the viewer switches this window
   (recorded).
5. Gap found and fixed: the status line claimed "Switching to Build mode"
   even when the switch handler then refused (e.g. the builder holds
   another file, `prepare.rs:193-199`), whose reason is the mode switcher's
   line; the status now says where the outcome shows
   (`cad/references/system_link.rs:133`).

### Open in CAD refused: no pending reveal

1. Robot mode's Open in CAD: `robot/threads/act.rs:open_in_cad` (130)
   builds the reveal (144) and writes `Act<WindowAction>::Switch` with
   `ModeSwitch {mode: Cad, document, reveal}` (145) in Robot's one apply
   system (`RobotSet::Actions` within `ViewerSet::Actions`,
   `robot/mod.rs:196`). It no longer writes `RevealThread`: the robot apply
   only reads whether the resource exists (`robot/actions/mod.rs:550`,
   `reveal.is_some()` at 565; "CAD mode is not part of this window",
   `act.rs:140-142`).
2. The reveal travels with its own request: `app/switch/mod.rs:122`
   (`ModeSwitch::reveal`). `handle` (373) passes the request to `start`
   (466, 502). Every refusal returns `Err` before the install: another
   switch still loading (505) or being entered (508), the target already
   active with a document, the leave blockers, the picker branch, and a
   document `prepare` refuses (552). A refused request and its reveal are
   dropped together (464-468).
3. **The line that proves the right thread is revealed:**
   `app/switch/mod.rs:557-561`. Only once `prepare` has accepted a switch
   to CAD mode does `start` install that request's own reveal into
   `RevealThread` with `set_if_neq`. None clears a reveal an earlier visit
   left. `RevealThread`'s writers are this install, CAD mode's read that
   takes it (`cad/threads/read.rs:243-277`, called from `sync` at 281) and
   `leave_cad` (`app/switch/leave.rs:113-117`).
4. Two Open in CAD requests during entry: the first is accepted, installs
   reveal A (560) and `enter` sets `switch.entering` and
   `NextState(Cad)` (`app/switch/arrival.rs:33-35`). The second, in the
   same frame or a later one before the entry finishes (388), is refused
   at 508 ("the switch to CAD mode is still being applied"), and its
   reveal B goes with the request. CAD mode lands and `read.rs:sync`
   (281-289) opens A once its document's threads are read. Nothing stale
   is left: the landed reveal is taken (`pending.0 = None`, `read.rs:276`),
   and leaving CAD mode drops one that never landed. Unit test
   (unexecuted): `app/tests.rs:443`
   `a_refused_switch_to_cad_cannot_change_the_reveal_of_the_one_that_lands`.
5. CAD switches never load (`prepare.rs:292`, `Prepared::Now`), so
   `finish_load`'s refusals cannot apply. If one ever did, the next
   accepted switch to CAD mode would replace the reveal (557-561).
6. Gap closed (this batch): a second Open in CAD while the first switch was
   entering used to overwrite `RevealThread` before its own refusal, so
   the first switch landed on the second thread. The old `drop_reveal` is
   deleted: no refused request ever touches `RevealThread` now.
   By reading, unexecuted.

### Escape order

1. The sets: `cad/mod.rs:148-172` (`CadKeySet::{Gate, Focus, Keys,
   ToolKeys, NumericEntry, EscapeTool, Escape}`), configured in
   `configure_sets` (176-184): all in `InputSet::Window` (179); `Gate`
   before `Keys` and `ToolKeys` (180); `Focus` before `Keys` (181);
   `Gate → EscapeTool → Escape → ToolKeys` chained (182); `EscapeTool`
   before `Keys` (183); `NumericEntry` inside `Focus` (184). The ordering
   test materializes every set (`app/ordering_tests.rs:21-27`).
2. The consumers, each ordered only against public sets:
   - the outliner's menu (`cad/tree/popup.rs:input`, Escape 96-101,
     consumed) and its rename and dialog fields (the kit's Cancel):
     `cad/tree.rs:169-173`, `before(CadKeySet::Gate)`;
   - the results forms: `cad/results/forms.rs:422`, `before(Gate)` in
     `Focus` (Escape 309, consumed: the key is cleared at 313 since
     b75846ff, so the Select tool's `cad:cancel` no longer fires on the
     same press; calibrate and threads also stand aside for it by state,
     `calibrate.rs:290`, `threads/input.rs:216`);
   - the references' fields (distance Escape is the kit's Cancel):
     `cad/references/input.rs:303`, `before(Gate)` in `Focus`;
   - the command surfaces and catalogue form (`cad/surfaces/mod.rs:389-421`,
     consumed since cad-parts-a-f-retrace: the key is cleared at 415 and a
     pending two-step key dropped, `keys.rs:Chord::abandon`, 416-418): their
     chain ends in `keys::gate` (327), so it runs before `Gate`, and before
     `CadKeySet::NumericEntry` (331); it is not ordered against the file
     form, the results form or the outliner menu, so it stands aside by
     state while one is open (396), as calibrate and the threads do;
   - the file form: `cad/files/mod.rs:591`, in `Focus` and
     `before(CadKeySet::EscapeTool)`; its Escape closes the form and is
     consumed (`cad/files/form.rs:544-548`), so the Select tool's
     `cad:cancel` no longer fires on the same press;
   - the calibrate tool: `cad/references/calibrate.rs:316`,
     `CadKeySet::EscapeTool`, `run_if(keys::free)`;
   - Annotate, Reattach and the linked parts shown alone:
     `cad/threads/input.rs:244`, `CadKeySet::Escape`, `run_if(keys::free)`;
   - the Select tool's Escape (`cad:cancel`, `cad/transform/input.rs:76`):
     `cad/transform/mod.rs:271`, `CadKeySet::ToolKeys` after `Gate` and
     after `CadKeySet::NumericEntry` (the numeric bar, 269);
   - RoboCAD's shortcuts: `cad/mod.rs:256`, `CadKeySet::Keys`; Escape is
     not bound there (`cad/keys.rs:25`, only a pending chord's Escape,
     373-374).
3. No CAD system orders against another feature's private system for
   Escape or for the numeric bar's Tab: the former private edges
   (`threads/input.rs` after `references::calibrate::escape`;
   `surfaces/mod.rs`, `transform/mod.rs` and `pick.rs:169` against
   `numeric::entry`) are the public sets above (commits e7b8393f,
   83ebb812).
4. One press ends at most one thing: each reader clears the key when it
   acts (tree menu, file form, results form since b75846ff, calibrate,
   threads, and the surfaces since cad-parts-a-f-retrace); the surfaces
   also stand aside by state for an open file form, results form or
   outliner menu. Gap found and fixed (cad-parts-a-f-retrace, Part D): the
   surfaces' Escape cancelled a form-less active tool (a plane tool, the
   joint tool) without consuming the key, and the threads' Escape checks
   the form but not a form-less tool, so one press could end the tool and
   Annotate together; it is consumed now.
   Gap found and fixed (b75846ff): the results form's Escape closed it
   without clearing the key, and `transform::keys` stands aside only for
   `doc.ops`, so the same press also fired the Select tool's `cad:cancel`
   (`cad/results/forms.rs:309-314`).
5. Gap found and fixed (review follow-up): with no surface open the
   surfaces' Escape wrote `CadFormCancel` for an open operation form or
   active interaction even while a file form was open, and, being
   unordered against `files::form::input`, could see the same press before
   the file form consumed it: one Escape closed the file form and
   cancelled the tool (`cad/surfaces/mod.rs:396` now).

### Autosave line follows autosave state

1. RoboCAD's `GET /autosave` (api.py:1618-1619 → `Service.autosave`,
   462-471) reads three attributes on the Qt thread (`run_on_main`, as
   `GET /` and `GET /selection`, 1546-1552, 1681-1683); 409 headless.
2. The poll worker (`cad/sync/mod.rs:poll_loop`, 163, a `RunThread`, off
   the UI thread) now reads it every tick from a desktop window, with the
   poll's short timeout (205), separately from the `/doc` refetch
   (218-235); a headless service gives `None`, a failed `GET /` keeps the
   last one (243-246). A single failed `GET /autosave` after an Ok keeps
   the shown state too; the second failure in a row, or a failure before
   any Ok, is published verbatim (worker locals `autosave_read`,
   `autosave_failed`, 173; 206-214), so one slow tick does not flash the
   line red.
3. `take_snapshot` (413) copies it and applies it only when it differs
   (`doc.autosave != snapshot.autosave`, 497-500), so the document is touched only then.
4. The Autosave line (`cad/panel.rs:autosave_line`, 588-610; a pending
   write's revision reads " · writing revision r", 602, RoboCAD's
   `pending[2]`, not the document's revision; drawn
   621-623) is part of the left dock's key (`cad/panel.rs:478`), so the
   dock is rebuilt only when the line's text changes.
5. Gap found and fixed: the line refreshed only when (document id,
   revision) changed, so an autosave write finishing (`notify("autosaved")`
   does not move the revision, document.py:287-289) or a pending write
   showed only after the next edit or Refresh (`cad/sync/mod.rs:205`,
   243-246). This closes the open item of "Autosave while attached".
   Review follow-up: the every-tick read published each transient failure,
   turning the line red until the next tick; one failure after an Ok is
   now held back (206-214).

## Reading traces — Part J

Each trace follows one Part J step (CAD-197 to CAD-209) or T42 repair
step (CAD-210 to CAD-213) from the native control to RoboCAD and back to
what the Components or System composition section and the status line
show. Everything here is **by reading, unexecuted**: nothing was built,
run or captured, and no step was compared side by side. Native paths are
under `crates/sim-spatial/src/` unless they start with `crates/`; RoboCAD
paths are under `cad/robocad/`. Gaps found while tracing were fixed in commits 38cf7745 and ac973444 (each named in its message), unless an entry says it is recorded; gaps a later review found were fixed after it (named "Review gap fixed").
The common legs are written out once:

- **Surfaces leg** (every component and composition control): the window's
  kit buttons carry the same typed action as system_ui and REST
  (`CadButton(CadAction::CadComponents | CadComposition)`;
  `cad/components/ui.rs:draw` 167, `cad/composition/ui.rs:draw` 290;
  system_ui lists `cad/components/mod.rs:controls_of` 446 and
  `cad/composition/ui.rs:controls` 74 through `cad/ui_api.rs:29-30`; REST
  `cad_components` / `cad_composition` specs, `cad/specs.rs:17-18`) →
  `Act<CadAction>` drained by the one apply system
  (`cad/actions.rs:apply` 354; dispatch 585-586) →
  `cad/components/mod.rs:handle` 153 or `cad/composition/mod.rs:handle`
  153. Field typing goes through the kit's one text entry
  (`cad/components/ui.rs:input` 28, `cad/composition/ui.rs:input` 23, both
  in `CadKeySet::Focus`), which writes the same `FormSet`/`SetField`
  action with the draft index the field belongs to (the draft a running
  rebuild was built from takes no typing: `ui.rs:118-152`). The raw `cad_op`
  route refuses every component op and `set_component_graph` by name
  (`cad/actions.rs:546-551`), so nothing bypasses the tracked job or the
  typed graph commands. The sections redraw only when their keys change
  (`cad/panel.rs:430-431`: `components::key` / `composition::key` plus the
  document's render stamp).
- **Component job leg** (every reusable-component mutation):
  `cad/components/mod.rs:submit` 300 refuses by name with nothing sent
  while a rebuild runs (`edit_refusal` 125), until the catalogue was read
  at this document and shown revision (309-314), when the draft belongs
  to another document or connection (325), when
  `CadDocument::commit_refusal(began)` names a reason
  (`cad/document/state.rs:138-155`: another edit in flight, not
  connected, the shown document behind, RoboCAD's revision moved since
  the form opened) and when native structural validation fails with a
  path-named error (`cad/components/validate.rs:validate_operation` 8).
  Then one `Pool::Dedicated` job (`mod.rs:344-363`, the `jobs` module,
  off the UI thread) on a client with `EDIT_TIMEOUT` (343;
  `crates/sim-runtime/src/cad_client/mod.rs:123`) first lists the jobs
  RoboCAD already has (`CadClient::component_jobs`,
  `crates/sim-runtime/src/cad_client/components.rs:289`; a failed listing
  sends nothing and answers `Started::NotSent`, `jobs.rs:17-26`, reported
  "nothing was sent", 205-210) and then sends **one** POST
  (`start_component`, `components.rs:279-285`: `/ops/{op}` with
  `kwargs`, `expected_revision`, `document_id`) → RoboCAD `api.py:1668-1671`
  → `Api.op` (api.py:1011-1028; component ops in
  `component_jobs.OPERATIONS`, component_jobs.py:22-24) →
  `component_service.ComponentJobService.start` (component_service.py:22-43:
  under the document lock, refuses a moved revision or document with
  RevisionConflict → 409 "Component start revision changed; your draft is
  preserved. Refresh and retry.", refuses a second active job) →
  `ComponentJob` snapshot and isolated worker
  (component_jobs.py:99-181; `component_worker.py:main` 8 in a child
  process; progress messages) → `{"job": status}`. The busy guard
  `doc.component_busy` is set to the operation's name (`mod.rs:366`) and
  `jobs::Active` records the identity, expected revision, captured
  client, operation, the job IDs listed before the POST and the draft it
  was built from (`cad/components/jobs.rs:59-117`). A panic in the start
  job (`crate::jobs` turns it into the job's error, perhaps after the
  POST) is not "nothing was sent": "the start job failed before its
  answer: …; outcome uncertain" and read-only recovery (211-221).
  Polling: `cad/components/jobs.rs:tick` 123 (JobResults, after
  `CadSet::Results`, `mod.rs:144-149`) adopts the start answer only for
  the captured document and revision (222-233), then GETs
  `/component-jobs/{id}` every 250 ms on a job (`component_job`,
  `components.rs:286`; spawn 508-525) → `api.py:1707-1708` →
  `ComponentJobService.status` (component_service.py:69-76) → `poll`
  (45-59), which commits a ready result exactly once:
  `ComponentJob.commit` (component_jobs.py:201-227) re-checks document and
  revision ("The document changed during preparation. Your edits are
  preserved; retry the component operation.", 205-208) and pushes one
  `components.ComponentChange` undo step (components.py:467-486;
  component_jobs.py:214-220). Terminal (`ComponentJobState::terminal`,
  `components.rs:122-126`) → `jobs.rs:378-490`: Applied refreshes the
  document (`cad::sync::refresh`, 434), drops the catalogue so it is read
  again, selects the definition and the new occurrence
  (`selection_after`, 128-147 → `CadAction::CadSelect`, the one selection
  path, once the node is in the shown tree), closes and marks the applied
  draft (`close_applied` 471, 547-557) and shows RoboCAD's text "Component
  updated. Undo restores the previous version." or "Saved to the
  component library." (472-477; ui/components.py:175-176); Failed names
  RoboCAD's error on the submitted draft, in the section and on the
  status line (479-482, `failure` 561-569, `report` 534); Cancelled shows
  "Rebuild cancelled. Model unchanged." (483-485). A job whose identity
  was displaced (a reconnect: same document, new generation) is reported
  without touching the open document (397-431): Applied marks and closes
  its draft and says "RoboCAD applied it on the previous connection;
  refresh before reusing this draft" in the section, on the draft and on
  the status line of the same document; Failed is reported with no
  document; Cancelled says "RoboCAD cancelled it on the previous
  connection; the model is unchanged". Every end clears `component_busy`
  (492-503). The section's progress line is RoboCAD's "stage ·
  done/total" (`cad/components/ui.rs:354-365`; ui/components.py:poll
  169-193).
- **Composition graph leg** (every source graph edit):
  `cad/composition/mod.rs:mutation` 533 refuses by name with nothing sent
  when `commit_refusal(revision)` names a reason (540), when the shown
  graph snapshot is not of this generation and revision (543-548), or
  belongs to another document (553-562); applies the command to a copy of
  the source graph (563-613) and validates it with the shared adapter
  (`cad/composition/adapter.rs` → `sim_system::composition::adapter::adapt`,
  `crates/sim-system/src/composition_adapter.rs:132`; port schemas
  `crates/sim-system/src/composition.rs:validate_port_schemas` 276);
  refuses a bound graph whose imported metadata is not current (634-641,
  `jobs.rs:imports_current` 3) → the shared CAD edit
  (`cad/edit.rs:edit_at` 48, the one in-flight edit, `cad/sync/mod.rs:686`
  `start_edit`, `Pool::Dedicated`, `EDIT_TIMEOUT`) → 
  `crates/sim-runtime/src/cad_client/composition.rs:composition_edit_guarded`
  87-111 (`expected_revision`, `document_id`, `check_id`) → RoboCAD
  `api.py:1582-1585` (`checked_graph_imports`, api.py:485-497, when a check
  is named) → `Service.system_request` (api.py:500-549: document identity
  and revision guards) → `component_graph.edit_graph`
  (component_graph.py:221-272: validation, `RegistryView`
  121-219, `ops.set_component_graph`, one undo step, commands.py:262).
  The answer: `cad/sync/mod.rs:finish_edit` 565 (and
  `composition::port_edit_answered`, called at 574/584, defined at
  `cad/composition/mod.rs:687`, for a sent port pick) →
  status line "System composition updated" → the revision moves → the
  graph read leg.
- **Graph read leg**: `cad/composition/jobs.rs:tick` 9 (JobResults, after
  `CadSet::Results`) keys every read by (generation, shown revision)
  (11); a new key spawns one `Pool::Dedicated` job (107-185) reading
  `CadClient::composition` (`GET /system`), `system_types`
  (`GET /experiments/catalogue`), `geometry_recipes`
  (`GET /component-recipes`) and, with a check chosen, `system_imports`
  (`composition.rs:52-71`), then adapts and lays out off the UI thread
  (`sim_diagram::composition::present`, `crates/sim-diagram/src/composition.rs:15`,
  with the job's cancel flag). A result for another revision, check or
  document is dropped (51-57); a read error marks the key failed and is
  not retried until the key changes (98-101, 107-112); an answer at this
  revision and check naming no document, or another document than the
  open one (once its ID is read), is refused the same way (77-96). A graph read at an earlier
  revision stays drawn, labelled "Stale graph: as read at revision R; the
  document is at revision N; …" (`cad/composition/ui.rs:367-388`), and
  its source controls are disabled by `commit_refusal` at its revision
  (`ui.rs:188`, 225, 237).
- **Mode and document guard leg**: `ComponentsState` and
  `CadCompositionState` are app resources initialised by their plugins
  (`cad/components/mod.rs:144`, `cad/composition/mod.rs:131`) and are not
  among the resources `cad::clear` removes on leaving CAD
  (`cad/mod.rs:317-329`), so every draft, the pending port intent and the
  check choice survive a mode switch. While a rebuild runs,
  `doc.component_busy` refuses every other source edit
  (`cad/document/state.rs:189-191`, "a component rebuild is in progress:
  place component; wait or cancel it in Components"), and is a switch
  blocker (`state.rs:209-211`) read by leaving CAD mode
  (`app/switch/prepare.rs:78-82`), by `cad_open` and new documents
  (`cad/actions.rs:638-641`) and by Open in builder
  (`cad/references/system_link.rs:94`, `cad/results/link.rs:128`).

### CAD-197 Library and find

1. View: `components.show` "Components library" (Window category,
   `cad/surfaces/registry.rs:390`) → `Do::Organize` →
   `cad/components/mod.rs:command_action` 600 → `ComponentsOp::Dock
   {open: true}` (162-169), or the `cad:components:dock` control.
2. Read: `cad/components/jobs.rs:read` 598 (no frame I/O) spawns one
   `Pool::Dedicated` job per (identity, shown revision) at most every
   500 ms (617-648): `CadClient::components` and `component_recipes`
   (`components.rs:263`, 269) → RoboCAD `api.py:1703-1704` →
   `Api.component_catalogue` (api.py:1030-1038: `ComponentOps.component_catalogue`,
   components.py:648, plus each definition's `targets`) and
   `api.py:1699-1702` (RECIPES, FEATURES, UNITS). The result is adopted
   only for the identity and revision it was asked at (jobs.rs:172-192);
   until then "Reading authoritative component catalogue…"
   (`ui.rs:193`).
3. Rows: `controls_of` (`mod.rs:465-500`) "name · rN · n placed", the
   count being the nodes whose `component_instance.definition_id` is the
   definition (`mod.rs:470-480`), as RoboCAD's `refresh`
   (ui/components.py:198-203, which counts the same nodes).
4. Find: the "Find a component…" field (`ui.rs:174-179`) →
   `ComponentsOp::Find` (`mod.rs:170-174`) → rows filtered by a
   case-insensitive name substring (`mod.rs:467`, `ui.rs:182`), as
   `filter_definitions` (ui/components.py:134-137, casefold over the row
   text). Selecting a row → `ComponentsOp::Select` (`mod.rs:175-190`), refused
   by name for a definition not in the current catalogue; the row says
   " · selected".
5. Deliberate difference: rows are single-spaced "name · rN · n placed"
   (RoboCAD pads the separators with two spaces), the filter matches the
   name only (RoboCAD's matches the whole row text, so "r2" finds
   revision 2), and there is no Library/Occurrence tab pair (one dock
   section). Names, revisions and counts match GET /components.

### CAD-198 Capture and parametric

1. Select bodies (the one `Selection`), then Make from selection
   (`cad:components:make`, or `components.make` "Make linked component…",
   `registry.rs:391` → `command_action` 609) → `ComponentsOp::Open {kind:
   make}` → `cad/components/form.rs:open` 114: a draft stamped with the
   identity and shown revision, `ids` = the selected nodes (`form.rs:141`,
   passed from `mod.rs:191-197`),
   fields name and origin (Make leaves the selected library definition
   alone, 123-139). "Capture definition" is the same with `create`.
2. Name typed; Apply component (`cad:components:submit`, `mod.rs:532`,
   disabled with the refusal while busy or stale) →
   `ComponentsOp::Submit` → `form::operation` (`K::Make` 582,
   `K::Create` 587) → `validate.rs:72-109` (a name, finite origin, a
   non-empty selection, and the whole linked occurrence rather than
   individual members, "capture the whole linked occurrence, not
   individual linked members") → component job leg →
   `ComponentOps.make_component` (components.py:515-539: preserves the
   captured IDs, `node_map` identity) or `create_component` (652-656);
   the committed undo step is `ComponentChange(doc,
   operation.replace('_',' ').title())` (component_jobs.py:214), so it is
   named "Make Component" or "Create Component", RoboCAD's own behaviour
   for both windows (they share the job service).
3. New parametric… → `kind: parametric` (name "Parametric box";
   choosing cylinder renames it "Parametric cylinder" while untouched,
   `form.rs:415-421`) → `validate.rs:110-115` →
   `new_parametric_component` (components.py:566) in the worker.
4. Answer: Applied selects the new definition (`jobs.rs:435-451`) and,
   for Make, the new occurrence (`instance_id`, 452-470), as RoboCAD's
   `poll` (ui/components.py:178-184). The headless service owns the jobs
   (`api.py:298-303`, `component_service.owner` 84-92): no Qt window is
   needed.
5. Deliberate difference: Make and New parametric are dock forms instead
   of RoboCAD's name and shape input dialogs; RoboCAD's parametric
   dialog sends "Parametric box"/"Parametric cylinder" fixed, the native
   name stays editable. Matches RoboCAD otherwise (same ops, one undo
   step, preserved IDs).

### CAD-199 Place twice

1. Select the captured definition (CAD-197 step 4); Place… →
   `form::open` with `kind: place`: the definition from the explicit id
   or the library selection (`form.rs:123-133`), name = the definition's,
   variant = its default, one `binding.KEY` per typed port preset to the
   first node of the port's kind (343-352, RoboCAD's combo's first
   entry, never invented); name, origin, Z angle, variant, bindings and
   per-parameter overrides are the visible fields (`ui.rs:visible_fields`
   392; the Place filter 402-406), with a button per matching node for each binding
   ("<label> · type <kind>", `ui.rs:320-346`).
2. Apply → `K::Place` (`form.rs:596-614`: placement `{translation,
   axis [0,0,1], angle_deg, scale 1}`) → `validate.rs:116-157` (known
   overrides, every port bound to a node of the port's kind, an existing
   variant) → component job leg → `place_component`
   (components.py:658-675: a fresh root ID and a fresh `node_map` ID per
   template node, one undo step named "Place Component" by
   component_jobs.py:214, the same for both windows).
3. Answer: the new occurrence is selected (`jobs.rs:452-470`); the
   definition row's count rises. Place again: a second, distinct
   occurrence ID; both reference the same `definition_id`.
4. Deliberate difference: a dock form instead of RoboCAD's Place dialog;
   it also offers per-parameter overrides at placement (RoboCAD's
   dialog places with none). Matches RoboCAD (distinct IDs, shared
   definition, binding kinds from the source ports).

### CAD-200 Defaults, units and nesting

1. Edit defaults… → `kind: defaults`: per-parameter rows value, unit,
   min, max, provenance, description (`form.rs:311-342`), unit chips
   from RoboCAD's UNITS and provenance chips measured/derived/estimated
   (`ui.rs:272-278`), the "Geometry and joints" recipe JSON, nested
   `parameter_bindings`/`overrides` per child occurrence and family
   variant bindings (`form.rs:295-310`). A whole `parameters` JSON edit
   re-projects the rows (`form.rs:423-441`) so old rows cannot shadow it;
   the note says values are unevaluated expressions (`ui.rs:244`).
2. Apply → `K::Defaults` (`form.rs:615-633`) → `validate.rs:158-225`
   (parameter names, units from RoboCAD's list, provenance, recipe kinds
   from the catalogue, recipe targets not derived members, nested
   mappings keep their definition, variant names and definitions
   read-only) → component job leg → `set_component_parameters`
   (components.py:677-713: RoboCAD's dimensional grammar,
   `component_parameters.validate_parameters` 106, revises the
   definition and its dependants and rematerialises every inherited
   occurrence, one undo step named "Set Component Parameters" by
   component_jobs.py:214, the same for both windows).
3. A refused or failed edit: the error is named on the draft that was
   sent (`jobs.rs:report` 534), the form stays open with its text, and a
   closed form is offered again ("Resume … draft · revision R",
   `ui.rs:211-231`; `mod.rs:252-269`). Local branch overrides win
   because RoboCAD materialises them over the mapping.
4. Gap found and fixed: a failure was written onto whichever form was
   open when it ended, not the draft that was sent; the request now
   records its draft (`cad/components/jobs.rs:83-85`, `mod.rs:315-317`).
   Gap found and fixed: a draft refused as stale (the document moved)
   could only be retyped; Copy … draft to revision N re-stamps a copy by
   consent (`cad/components/mod.rs:376-398`, control 564-589, button
   `ui.rs:233-235`), as the composition's Copy draft. Review gap fixed:
   Resume and Copy draft accepted an applied draft over REST while the
   window hid it, so the same change could be applied twice; both refuse
   it, "RoboCAD already applied this draft; open a new form to change
   the model again" (`mod.rs:259`, 385-387, `applied_refusal` 406-410).
5. Deliberate difference: JSON and text rows replace RoboCAD's tabbed
   RecipeDialog (ui/components.py:22-90), and no current value is
   computed natively (RoboCAD alone evaluates expressions).

### CAD-201 Overrides and reset

1. Select a linked part → `kind: overrides` → `form.rs:open` 153-258:
   a member maps to its nearest occurrence (168-180); a nested
   occurrence keeps its branch identity: the outer occurrence's
   `node_map` source key and its `nested_overrides[source]` are the
   local overrides (209-245); origin is offered only for a top-level
   occurrence (249-257, 369-371); the section says "Nested occurrence:
   branch-local overrides only; move through the parent definition"
   (`ui.rs:245-247`). Each parameter has `override.NAME.enabled`
   (true/false chips) and `.value`.
2. Apply occurrence → `K::Overrides` (`form.rs:634-644`; unchecked rows
   leave the map, 557-578) → `validate.rs:226-252` (nested movement
   refused: "move nested components through the parent definition") →
   `set_component_overrides` (components.py:715-733: the branch map on
   the outer occurrence for a nested one; RoboCAD refuses nested
   placement the same way).
3. Reset to inherited → `K::Reset` = overrides `{}` without placement
   (`form.rs:645-649`), as `reset_override` (ui/components.py:281-282).
4. Deliberate difference: the native values are unevaluated expression
   text, labelled so (`ui.rs:244`); RoboCAD's Current column is
   computed by its evaluator and has no native counterpart.

### CAD-202 Detach and transform

1. Detach outer occurrence → `kind: detach`: from any member the walk
   climbs to the outermost occurrence (`form.rs:181-198`, cycle refused),
   as RoboCAD's `detach` (ui/components.py:284-290) →
   `validate.rs:253-261` → `detach_component` (components.py:735-744;
   one undo step named "Detach Component" by component_jobs.py:214, the
   same for both windows). Undo (`cad_undo`) restores the links
   through ComponentChange.undo (components.py:485).
2. Transform occurrences… → `kind: transform` on the selected
   occurrences: translation, axis, angle, scale
   (`form.rs:677-684`) → `validate.rs:319-349` (top-level unlocked
   occurrences only, scale 1, finite) → `transform_components`
   (components.py:490-513: a rigid placement change, locked refused).
3. Matches RoboCAD (same member restrictions). RoboCAD has no
   Transform form in its panel; the op is reached through the same
   service.

### CAD-203 Progress and cancel

1. Apply starts the job (component job leg). Before any status:
   "Preparing component… You can keep viewing the model."
   (`cad/components/ui.rs:364`, RoboCAD's `watch`,
   ui/components.py:159-162); while a lost start answer is being
   recovered instead "Outcome uncertain; reading RoboCAD's jobs (no POST
   is retried)" (360-363); then "stage · done/total", or the stage
   alone without a total (`ui.rs:357-359`, RoboCAD's `poll`
   ui/components.py:191, from ComponentJob progress,
   component_jobs.py:130-132, 183-194).
2. Cancel rebuild (`cad:components:cancel`, `mod.rs:590-596`) →
   `ComponentsOp::Cancel` (`mod.rs:271-279`) sets the durable
   `cancel_requested` in state, not a message; the section says "Cancel
   requested · waiting for RoboCAD to confirm; a change it already
   applied is reported as applied" (`ui.rs:366-371`).
3. Cancel while the POST is pending: nothing is sent until the job ID
   is known; then DELETE before any GET (`jobs.rs:306-331`: no status
   GET is spawned while a cancel is requested, 508-525, because a GET
   can commit) → `cancel_component_job` (`components.rs:292-298`) →
   `api.py:1707-1708` → `ComponentJobService.status(cancel=True)`
   (component_service.py:69-76: `job.cancel()` sets the stop flag and
   terminates the worker before `poll`, component_jobs.py:119-122) → a
   queued ready result becomes cancelled instead of committing
   (component_jobs.py:190, 204).
4. A DELETE that answers still-running is repeated until terminal
   (`accept_cancel`, `jobs.rs:673-679`), and an older GET cannot revive a
   terminal state (`accept_status` 658-669).
5. A commit RoboCAD already made (another poller, e.g. RoboCAD's own
   window timer, got there first): DELETE answers `applied`; the
   status line says "Cancel arrived after RoboCAD had applied it.
   Component updated. Undo restores the previous version." When the
   applied state came from a GET instead, it says "Cancel was sent, but
   RoboCAD applied it before the cancel took effect." or, with no DELETE
   sent yet, "Cancel was requested, but RoboCAD applied it before the
   cancel was sent." (`jobs.rs:383-392`, 477; `by_cancel` 333-347).
6. Cancelled: "Rebuild cancelled. Model unchanged." (483-485, RoboCAD's
   text); the busy guard clears (492-503).
7. Gap found and fixed: an applied-after-cancel commit was reported as
   a plain success (`cad/components/jobs.rs:383-392`). Gap found and
   fixed: the progress line printed `{stage} · {done} / {total} ·
   Running` (Debug state) instead of RoboCAD's text
   (`cad/components/ui.rs:354-372`). Gap found and fixed: the busy label
   read "a component rebuild is in progress: A component rebuild is in
   progress; wait or cancel in Components; wait or cancel it in
   Components"; it is now the operation's name (`mod.rs:366`). Review gap
   fixed: every cancelled-then-applied commit said the cancel "arrived";
   only a DELETE that itself answered `applied` says so (step 5). Review
   gap fixed: while a lost start answer was being recovered the line
   said "Preparing component…" (`ui.rs:360-363`). Review gap fixed:
   typing into the draft being applied was accepted, and the applied
   draft then closed and hid it, losing the typing; `FormSet` on that
   draft is refused "this draft is being applied; wait for the rebuild
   or cancel it" (`mod.rs:198-202`, `jobs.rs:applying` 573-575), its
   fields lose and refuse the keyboard and its chips are disabled
   (`ui.rs:118-152`, 237-243, 316, 341), so the kit field shows the
   stored value; the refusal is not stored on the draft and leaves the
   section when the job ends (`mod.rs:287`, `jobs.rs:493-496`).
8. Deliberate difference: RoboCAD's Cancel rebuild cancels every job of
   its service at once (`cancel_jobs` → `cancel_all`, ui/components.py:195-196,
   component_service.py:78-81); the
   viewer cancels the one job it started.

### CAD-204 Stale source and lost answers

1. Mutate the source in RoboCAD while preparing: the worker's result is
   refused at commit ("The document changed during preparation. Your
   edits are preserved; retry the component operation.",
   component_jobs.py:205-208) → Failed → named on the submitted draft,
   the section and the status line (`jobs.rs:479-482`); the draft keeps
   every field. A revision refused at start (409, component_service.py:27-32)
   is not ambiguous (`ambiguous_start` 653), so it ends the same way
   (`jobs.rs:234-247`). The draft is then stale (`commit_refusal`), and
   Copy … draft to revision N re-stamps it by consent (CAD-200 step 4).
2. Interrupt the start response: a timeout after sending, a closed
   connection, a 5xx or an unreadable 2xx is marked "RoboCAD may still
   apply it" / "applied it, but its answer could not be read"
   (`crates/sim-runtime/src/cad_client/mod.rs:133-170`) → `uncertain`
   (`jobs.rs:237-240`). The POST is never sent again: there is no other
   start path (`submit` only). A panic in the start job is treated the
   same way (211-221), since it may have come after the POST.
3. Recovery by read only: every 500 ms `GET /component-jobs`
   (`jobs.rs:291-305`, `components.rs:289`) → `api.py:1705-1706` →
   `ComponentJobService.discover` (component_service.py:61-67: drains
   preparation, never publishes) → `recover` (`jobs.rs:684-707`) adopts
   exactly one job with the captured operation, document and revision
   **that was not listed before the POST**; two candidates are named
   "unresolved request (n matching jobs); no POST is retried". A cancel
   requested meanwhile stays requested and is sent once the job is
   adopted.
4. Gap found and fixed: recovery could adopt an earlier failed or
   cancelled job of the same operation at the same revision (a failure
   or cancel does not move the revision) and report its outcome as this
   request's; the start job now lists RoboCAD's jobs first and recovery
   excludes them (`cad/components/mod.rs:344-363`, `jobs.rs:691`).
   Gap found and fixed: a request RoboCAD never registered (e.g. its
   desktop window answered 504 "the GUI did not answer in time; the
   request was cancelled", which the client marks "may still apply")
   kept the rebuild guard forever, blocking every edit and leaving CAD;
   after 30 s with no matching new job the wait ends with "RoboCAD
   still lists no job for this request 30 s after its answer was lost;
   nothing was retried. Check the model before applying the retained
   draft again" (`jobs.rs:31`, 259-267). Gap found and fixed: the start
   POST used the 30 s read timeout instead of `EDIT_TIMEOUT`, so a busy
   desktop RoboCAD produced needless ambiguity (`mod.rs:343`). Gap found
   and fixed: after a reconnect (same document resource, new
   generation) a finished job never cleared `component_busy`, and a job
   the service no longer had (404) or a service that could no longer be
   reached was polled or cancelled forever; both now end, saying the
   outcome is not known here (`jobs.rs:332-377`, 492-503, `job_error`
   582-590, `lost` 592). Review gap fixed: after a reconnect, a job that
   had already committed answered `applied` to the cancel but was not
   reported, and its draft stayed copyable, so it could be applied
   twice; the displaced branch (`jobs.rs:397-431`) marks the draft
   applied and says "RoboCAD applied it on the previous connection;
   refresh before reusing this draft", and reports a failure with no
   document. Review gap fixed: a cancel to the old service failing
   otherwise (e.g. a read timeout) was retried every 250 ms forever,
   keeping the busy guard; cancels sent while displaced end after 30 s
   with "whether it changed the model is not known here"
   (`displaced_cancel_at`, `jobs.rs:72-75`, 328-330, 336-358). Review
   gap fixed: a panic in the start job was reported "nothing was sent"
   and released the guard, though it may have come after the POST; only
   a failed listing (`Started::NotSent`) says so now, and a panic is an
   uncertain outcome recovered by read (`jobs.rs:17-26`, 205-221,
   `mod.rs:348-361`).
5. Matches RoboCAD's guard (document ID and revision at start and at
   commit). Deliberate difference: RoboCAD's own panel calls the
   service in-process and has no lost-answer case.

### CAD-205 Graph forms

1. System composition (`cad:composition:dock`, `cad/composition/ui.rs:89-94`
   → `CompositionOp::Dock`, `mod.rs:158-161`) → graph read leg.
2. Type chooser: "Add <type name>" per authoritative type
   (`ui.rs:164-176`, disabled "Parameter metadata unavailable" while the
   type's parameters are incomplete) → `CompositionOp::New`
   (`mod.rs:162-216`: a draft stamped with document ID, generation and
   revision). Selecting a graph component → `Select`: its fields, its
   recipe and parameters, and its CAD body selected through the one
   selection (`cx.shared.set`, 173-178).
3. Form: Name, Imported binding, CAD body ID with "Attach <body>"
   buttons (`ui.rs:460-490`); typed parameters "name [unit] · blank =
   imported/default …" (556-567), wildcard families with "Concrete
   parameter name" and Add named parameter (512-554, `AddParameter`
   `mod.rs:319-340`); "Geometry rule KIND → outputs" chips
   (568-592, `SetField recipe.kind` forks a copy with the old recipe
   fields dropped, `mod.rs:294-308`), inputs "label [unit] · default"
   (610-628), "Derived by KIND; geometry/provenance owned by RoboCAD".
4. Apply component → `Submit` → `draft_command_at` (`mod.rs:451-532`:
   draft generation and document refused when changed; numbers parsed
   with path-named errors) → graph leg; the shared adapter refuses a
   derived output given explicitly ("source-owned derived parameter
   cannot be overridden", `crates/sim-system/src/composition_adapter.rs:99-101`)
   and RoboCAD's `RegistryView.validate_component`
   (component_graph.py:177-199) checks declared bounds and units.
5. Layout never edits the source: Overview, Focus, Zoom, Pan and
   Arrange display (`mod.rs:261-287`, 364-385) only change presentation
   state and drop the snapshot to re-lay it out; none calls `mutation`.
6. Gap found and fixed: a refused port pick, pan or check read was also
   written onto the open component form as its error; only a draft's
   own edit or submission is named on the draft it addressed
   (`cad/composition/mod.rs:419-431`).
7. Deliberate difference: kit fields and buttons instead of RoboCAD's
   graph scene and form widgets (ui/system_graph.py:143-175, 377-429).

### CAD-206 Imported bindings

1. "Completed check ID (read only)" field (`ui.rs:334-342`) → Read
   completed check bindings → `CompositionOp::ImportCheck`
   (`mod.rs:341-358`) clears the old snapshot and pending port intent
   (`set_check` 139-152, with a named note) and spawns one
   `Pool::Dedicated` job: `CadClient::system_imports`
   (`composition.rs:66-71`) → `GET /experiments/{id}/components`
   (api.py:595, `Experiments.components`, experiments.py:336;
   `component_imports.metadata_guard`, component_imports.py:10).
2. The result is adopted only when its guard document and revision are
   the shown ones and it is the check asked for
   (`cad/composition/jobs.rs:24-44`); otherwise "document or revision
   changed during read; read the completed check again".
3. Shown: "Check ID · state · measured results stale · binding metadata
   stale" (`ui.rs:344-365`); "Bind <name>" buttons for imported
   components of the draft's type, disabled while the metadata is stale
   (491-506), keep the imported port names and type.
4. Graph-only edits are allowed while measured results are stale; a
   bound graph with metadata of another revision is refused before
   sending ("metadata belongs to an older revision; refresh or remove
   bindings", `mod.rs:634-641`), and RoboCAD refuses stale metadata
   itself (api.py:485-497, 409 "imported structural metadata is stale").
5. Deliberate difference: an existing completed check ID is typed;
   creating and running the check is cad-experiments-motion's.

### CAD-207 Typed nets

1. Connect A.p (`port:ID` control "Connect component.port" stamped with
   the snapshot revision, `ui.rs:200-228`) → `CompositionOp::Port` →
   `ports::pick` (`cad/composition/ports.rs:140-184`): the endpoint must
   exist in the adapted graph (`endpoint_exists` 50-75); the first pick
   is kept as `PendingPort {endpoint, generation, document_id,
   revision}` (177-182).
2. Second pick → `validate_pending` (30-49) → `mutation` with
   `GraphCommand::Connect [first, second]` at the first pick's revision
   (157-170); locally joined nets merge into one (`mod.rs:581-605`) and
   the shared adapter checks port schemas → `POST /system/connections`
   (`composition.rs:102`) → `edit_graph` connect
   (component_graph.py:251-263: a port already in a net extends that
   net and keeps its ID) → `validate_connections` (201-219: matching
   physical connector types, no physical-to-signal, exactly one signal
   output, compatible units).
3. Third physical terminal: picking a port of the existing net and a new
   one extends the same net (same ID, all three terminals).
4. Remove connection (`open:ID`, `ui.rs:229-240`) → `CompositionOp::Open`
   → `DELETE /system/connections/{id}?expected_revision&document_id`
   (`composition.rs:104-109`) → `edit_graph` delete_connection
   (component_graph.py:264-265): the whole net goes.
5. Focus/Overview/Zoom are display only (CAD-205 step 5). Matches
   RoboCAD's rules (the same server validation; the Rust adapter rejects
   the same incompatible schemas first). Deliberate difference: typed
   port buttons accompany the shared routed drawing; RoboCAD picks ports
   by clicking them in its scene (`ConnectionsView.mousePressEvent`,
   ui/system_graph.py:98 → `choose_port` 498-508).

### CAD-208 Save and library

1. Save document: `cad_save` (cad-files; not this part).
2. Choose folder: the "Component library folder" field
   (`cad/components/ui.rs:199-204`) → Enter or Choose / refresh folder →
   `ComponentsOp::Folder` (`mod.rs:215`, `folder` 412-436: an absolute
   path on the service host, else refused by name) → one
   `Pool::Dedicated` job `CadClient::component_library`
   (`components.rs:272-278`) → `api.py:1691-1698` (read-only listing on
   the HTTP worker; 422 for a file). The listing is adopted only for the
   identity it was asked for (`jobs.rs:148-171`); a failure stays named
   until refreshed.
3. Import selected (a listed row carrying the shown revision,
   `mod.rs:517-528`) → `ImportSelected` (216-244: a path not listed is
   refused; a request without the row's revision is refused, as a direct
   submit is; a row read at another revision is refused "nothing was
   sent: choose the file again") → an Import draft → component job leg → `import_component`
   (components.py:612-646: library validated, identities remapped; one
   undo step named "Import Component" by component_jobs.py:214, the same
   for both windows).
4. Save to library… → `kind: export` (path preset to
   `folder/name.rcomp`, `form.rs:353-358`) → `validate.rs:263-269`
   (absolute `.rcomp`) → `export_component` job: the archive is written
   to a temporary file beside the target (component_jobs.py:139-148) and
   published with `os.replace` only at commit after the revision guard
   (222-224); cancelled or stale work deletes the temporary file
   (`release`, 229-235), so the existing destination stays intact. The
   status line says "Saved to the component library."
5. Gap found and fixed: Import selected ignored the revision its row
   carried (`cad/components/mod.rs:225-230`). Gap found and fixed: an
   import refused after its form opened was not named on that form
   (`mod.rs:232-242`). Review gap fixed: an import without `revision`
   skipped the row-revision check; it is refused "components.revision:
   required for import_selected …; nothing was sent" (`mod.rs:222-224`).
6. Deliberate difference: service-host path fields and a listed-file
   row per file replace RoboCAD's file dialogs (ui/components.py:292-310).

### CAD-209 Draft and mode

1. Close (keep draft) → `ComponentsOp::CloseForm` (`mod.rs:209-214`,
   "draft_retained"); every draft not current and not applied is offered:
   the window's button reads "Resume <kind> draft · revision R"
   (`ui.rs:211-231`, text 216), the REST/system_ui control's label
   "Resume <kind> draft" (`mod.rs:550-562`, label 554)
   → `Resume` (252-269; an applied draft is refused by name, 259). Composition drafts the same with Resume draft /
   Copy draft to current revision (`cad/composition/ui.rs:132-149`,
   `mod.rs:236-260`).
2. Where drafts live: `ComponentsState.drafts` ("Drafts are never
   dropped on refusal or mode exit", `cad/components/mod.rs:98-99`) and
   `CadCompositionState.drafts` (`cad/composition/mod.rs:106-127`), both
   app resources kept across leaving and re-entering CAD (mode and
   document guard leg).
3. Switch mode or open another document while a rebuild is active:
   refused by `switch_blockers` naming the rebuild
   (`cad/document/state.rs:209-211` via `app/switch/prepare.rs:78-82`
   and `cad/actions.rs:638-641`) until the job is terminal; then
   `component_busy` clears (`cad/components/jobs.rs:492-503`). While it
   runs, the draft it was built from refuses edits (CAD-203 step 7).
4. Stale retained draft: after re-entering CAD the document has a new
   generation; a component draft is refused "document or connection
   changed; reopen against this document (the old draft is retained)"
   (`mod.rs:325-327`), a composition draft "document changed; old draft
   retained" (`cad/composition/mod.rs:459-466`); a draft whose revision
   moved is refused by `commit_refusal`. Copy draft re-stamps either by
   consent.
5. Gap found and fixed: an applied component draft stayed open and
   current with Apply disabled by "redo the drag, the entry or the
   form"; it now closes and is no longer offered to resume, as
   RoboCAD's dialog closes (`cad/components/form.rs:93-95`,
   `jobs.rs:close_applied` 547-557, `mod.rs:551`, `ui.rs:212`). Gap found
   and fixed: `component_busy` survived a reconnect forever (CAD-204 step
   4). Review gap fixed: Resume and Copy draft accepted an applied draft
   over REST (CAD-200 step 4); an applied draft reported after a
   reconnect is marked applied too (CAD-204 step 4).

### CAD-210 Explicit family target

1. Select the family in the library (`ComponentsOp::Select`), then
   `open {kind: link_family, id: OCCURRENCE}` (REST/system_ui) →
   `form.rs:open`: the definition is the library selection only
   (134-137), the id names the occurrence (159-163; a member maps to its
   occurrence) and is never read as a definition (`if kind !=
   LinkFamily`, 205-207).
2. Apply → `K::LinkFamily` (`form.rs:671-676`) → `validate.rs:294-318`
   (a top-level occurrence already using the chosen variant's
   definition) → `link_component_family` (components.py:554-565).
3. The window's no-id path (selection) builds the same typed operation
   (`cad/components/tests.rs:link_family_explicit_id_is_occurrence_not_selected_family`,
   written, unexecuted). Matches RoboCAD's op; RoboCAD's panel has no
   LinkFamily form (it is reached through the service).

### CAD-211 Stale first pick

1. Pick a port: `PendingPort` keeps the generation, document ID and
   revision of the displayed snapshot (`ports.rs:177-182`).
2. Edit the source in RoboCAD, or reload a document with the same
   revision: the shown revision or generation moves; the graph read
   leg reads the new key; the pending intent is kept
   (`cad/composition/jobs.rs:12-23`, "Retain stamped intent").
3. Pick another port: `snapshot_at` (`ports.rs:10-29`) refuses a
   revision that is not the shown one, and `validate_pending` (30-49)
   compares the supplied revision with the first pick's and its
   generation and document with the document's: "composition.port.stamp:
   first pick belongs to another document or revision; cancel
   connection to discard retained intent". Leave open uses the same
   check (`open_command` 185-211), and its control is disabled with it
   (`ui.rs:242-251`).
4. On refusal `pending_port` is untouched and the diagnostic is in
   `st.error`, drawn at the top of the section (`ui.rs:314-316`); the
   Leave port open and Cancel connection buttons are drawn even with
   the section closed or no snapshot (`ui.rs:305-310`).
5. Gap found and fixed: the refusal was also written onto the open
   component form (CAD-205 step 6). RoboCAD's counterpart: its `refresh`
   (ui/system_graph.py:321-338) does not drop `pending_port`, which only
   `choose_port` (a pick or cancel, 500, 508) and `open_port` (515)
   clear; ports are picked by click (`ConnectionsView.mousePressEvent`,
   98). Its second pick publishes at the first pick's revision
   (`connection_revision`, 502, 507), and `edit_graph`'s `check_revision`
   (component_graph.py:225, candidates.py:39-42) refuses a moved revision
   as a RevisionConflict ("Expected document revision R; current revision
   is N. …"), raised before `pending_port` is cleared, so the pick is kept
   there too. Deliberate difference (recorded): the viewer refuses before
   sending, with its own text, and also compares the generation and
   document ID (a reload at the same revision); RoboCAD compares the
   revision only, at its service.

### CAD-212 Open, cancel and remove

1. Pick an unused physical or signal-output port; Leave port open
   (`ports::leave_open` 212-226 → `open_command`: refused "already
   connected; remove its connection before declaring it open" for a
   connected port, 193-204) → `GraphCommand::Connect [port]` at the
   pick's revision → graph leg → `edit_graph` connect with one port
   (component_graph.py:251-263, a fresh ID; a lone signal input is
   refused by `validate_connections`, 216-218), one undo step.
2. The pick becomes `submitted_port` once the edit is dispatched
   (`dispatched` 88-98); `answered` (99-126) clears the intent only on
   success, keeps it with "source edit failed; retained intent if not
   cancelled" otherwise.
3. Cancel connection → `ports::cancel` (127-139): no source edit;
   "connection cancelled; source unchanged", or "local connection
   cancelled; submitted source edit continues" while one is in flight.
4. Remove connection deletes the whole net (CAD-207 step 4).
5. RoboCAD's "Leave port open" is a `QPushButton`
   (ui/system_graph.py:198) calling `open_port` (510-515), which refuses a
   connected port ("This port is already connected. Remove its connection
   first to declare it open.", 512-513) and publishes `connect` with the
   one port at the pick's revision. Matches RoboCAD (a button in both;
   the refusal texts differ in wording).

### CAD-213 Build failure

1. Build mode's schematic (the shared graph): `builder/schematic.rs:tick`
   168-191 lays out only a description compiled at the current
   revision, keyed by (description, revision, level); `lay_out` 106
   validates through `sim_system::composition::Composition::new`
   (109; `crates/sim-system/src/composition.rs:106`) on a
   `Pool::Compute` job (`start` 206-210).
2. Invalid graph: the worker's path-named error (e.g.
   `nets.broken.ports[0]`) → `finish` 194-200 keeps the last good
   layout, stores the error and marks the key failed; `needs_layout`
   (202-204) admits no new job for the failed key, so presenting it
   again schedules nothing.
3. Shown: "Stale layout · layout failed: <error>" (318) over the prior
   layout, with the "Stale layout (not the current system)" overlay
   (354-359).
4. Change the source: a new key passes `needs_layout`, `start` clears
   the error while it lays out, and a success replaces the layout.
   Written fixture: `schematic.rs:440-479`, unexecuted.
5. CAD's composition section: an invalid adaptation (`adapter::adapt`'s
   error, `cad/composition/jobs.rs:168`) or a failed types/recipes read
   (170-173) is read once per (generation, revision) and replaces the
   snapshot with one that has no presentation (69-71), its path-named
   `presentation_error` kept as the section's error (72-75): no graph is
   drawn and only the error shows. Only a failed read keeps the old
   snapshot, drawn and labelled stale (98-101; `ui.rs:367-388`), and is
   not retried for the same key (107-112); a changed key reads again.
   Recorded difference: Build's schematic keeps the last good layout over
   an invalid graph (steps 2-3); CAD's composition section draws none.
6. Gap found and fixed: the CAD composition section drew a graph read at
   an earlier revision with no stale label
   (`cad/composition/ui.rs:367-388`). Gap found and fixed: a `/system`
   answer naming no document was re-read on every completion, without
   end (`cad/composition/jobs.rs:77-96`). Review gap fixed: an answer at
   the current revision and check naming a different document than the
   open one was re-read every frame too; it is refused the same way,
   naming both documents (`cad/composition/jobs.rs:77-96`). Matches the T42 repair
   acceptance; no RoboCAD counterpart (Build is native).
