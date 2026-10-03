# CAD mode: side-by-side checklist against RoboCAD

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
cad-components were written and checked only by reading. **Parts G to J
are traced by reading, unexecuted** (cad-checklist-traces, 2026-10-02):
every step CAD-131 to CAD-175 and CAD-187 to CAD-213 has a path:line trace
from the control to RoboCAD's route and back to the display (the "Reading
traces" sections at the end; CAD-176 to CAD-186 were traced in cb54e964,
"Reading traces — annotations"), and the gaps those traces found were
fixed in code or recorded as differences. None of it
has been compiled, run or compared side by side, so the steps below are
still to be done once by a person. Each
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
| CAD-01 Open | `cargo run -p sim-spatial -- /tmp/cad-check/turntable.rcad`. The top bar shows Connecting…; the left dock shows "Connecting: starting RoboCAD's headless service on … (N s)", then "Connected · revision N", and names the self-started service (pid, URL, headless). Also: switcher **CAD**, `system_ui` `mode:cad`, or `viewer_mode {"mode":"cad","path":"/tmp/cad-check/turntable.rcad"}` from another mode | The "Opening turntable.rcad" window, then the document | Both open the same document; the window never froze while the service started |
| CAD-02 Tree | The left dock lists every node in RoboCAD's order, indented by parent, with kind, name, "Shown"/"Hidden"/"Hidden by parent"/"Disabled" and "locked" | The outliner | Same nodes, order, nesting and visibility |
| CAD-03 Bodies | Every visible body is drawn (Z up, mm shown in metres); Home or **Fit** frames them; right-drag orbits, middle or Shift+right-drag pans, the wheel zooms | The viewport | Same shapes; nothing is drawn for hidden or disabled nodes |
| CAD-04 Select | Click a tree row, or a body in the 3D view: the row and the body highlight; `cad_select {"ids":["…"]}` does the same | Click the same part | The inspector follows the selection |
| CAD-05 Inspect | The right dock shows the node as RoboCAD returns it: kind, id, parent, material, colour, "Instance of" for instances, transform, body kind, volume mm³, area mm², mass g, centroid, bounding box, size, face and edge counts, then joint/robot/sketch/plane/… fields. A mass value RoboCAD sent as null (or as NaN) reads "null in RoboCAD's answer"; other nulls read "null" | The properties panel | Same numbers and units; nothing in the viewer is filled in that RoboCAD did not send |
| CAD-06 Physical labels | Press **Physical** ("Fetching RoboCAD's physical model…", then the link holding the body: mass kg, centre of mass m, inertia kg·m², bounding box, and `mass_sources` for this body as a chip, e.g. a declared measurement's `source`). `cad_physical` | `GET http://127.0.0.1:<port>/physical?flex=0` on the service (port in the left dock) | The chip text is RoboCAD's label exactly; a body without a label shows no chip |
| CAD-07 Patch | In the inspector: **Visible**, **Locked**, **Disabled**, a material chip; click the name, type, Enter. The status bar shows "Sending: …", then the tree and inspector update. `cad_patch {"id":"…","attrs":{"visible":false}}` | Same edits in the properties panel | Same result; while one edit is in flight a second is refused naming the first |
| CAD-08 Undo / redo | **Undo {label}** / **Redo {label}**, or Cmd/Ctrl+Z and Cmd/Ctrl+Shift+Z (`cad_undo`, `cad_redo`). History in the inspector lists RoboCAD's labels | Edit ▸ Undo / Redo | Same labels; each patch from CAD-07 is one undo step |
| CAD-09 Delete | Select a node, **Delete** or Delete/Backspace (not while typing a name). `cad_delete` | Delete | The node goes; undo brings it back |
| CAD-10 Unsaved edits | After an edit, the top bar says "Unsaved edits". Press the switcher's **Build** | (n/a) | Refused: "… has unsaved edits in the RoboCAD service this window started …: save first"; CAD mode stays |
| CAD-11 Save | **Save** or Cmd/Ctrl+S (`cad_save`; `{"path":"/abs/file.rcad"}` saves as: an absolute path or `~/…`, a relative one is refused naming why). The top bar returns to "Saved"; the status line says "Saved … with its thumbnail". Then `unzip -l` the file | File ▸ Save, then the same `unzip -l` | The file on disk changed (RoboCAD wrote it, through `POST /save/thumbnail`); both archives hold `thumbnail.png`; reopening it in RoboCAD shows the edits; after `cad_save {"path"}` the left dock names the new file (a self-started document follows it) |
| CAD-12 Leave | Switcher **Build** now succeeds; the RoboCAD service process is gone (`ps` with the pid from the left dock). Switch back to **CAD**: it starts a new service on the same file | (n/a) | No `robocad.api` process is left behind |

## Part B: attached to RoboCAD's window

Open RoboCAD on the copy (`cad/run.sh /tmp/cad-check/turntable.rcad`; its
status bar names its REST port, 8420 unless that was taken), then
`cargo run -p sim-spatial -- --cad-url http://127.0.0.1:8420` (or
`viewer_mode {"mode":"cad","url":"http://127.0.0.1:8420"}`).

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-13 Shared selection | Click a row or body | Click a different part in RoboCAD | Each window shows the other's selection within about a second |
| CAD-14 Edits both ways | Hide a node, rename one | Move a part, change a material | Both windows show all four changes; RoboCAD's undo history lists all of them in order |
| CAD-15 Commands | The inspector's Commands section lists RoboCAD's registry by category with its keys; run one, e.g. **Fit view** (`cad_command {"id":"view.fit"}`) | The same command from its menu or palette | The command runs in RoboCAD's window |
| CAD-16 Autosave | The left dock shows RoboCAD's autosave (running, saved revision, path) | RoboCAD autosaves as usual | The same state |
| CAD-17 Leaving keeps edits | With unsaved edits, switch to **Build** | (n/a) | The switch succeeds and its message says RoboCAD keeps the unsaved edits; RoboCAD's window still has them; RoboCAD is not stopped |
| CAD-18 Service loss | Back in CAD mode, quit RoboCAD | Quit (save or discard as you like) | The top bar says Lost and the left dock "Not connected: …" with the error; the tree stays on screen; edits are refused naming the lost connection; after reopening RoboCAD on the same port the viewer reconnects within a second (**Refresh** forces it) |

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
| CAD-19 Selection modes | The strip at the top left of the 3D view: **Bodies**, **Faces**, **Edges**, **Vertices**, **Points**, or the keys B, Shift+B, E, V, P (`cad_select_mode {"mode":"face"}`). The status reads "Selection mode: face" and the tool bar "Select  ·  Face"; edge mode draws every edge faintly, vertex mode marks every vertex | Select ▸ the same modes, same keys | Same modes and keys; a switch clears the selection in both; B, E, V and P do nothing while a text field has the keyboard |
| CAD-20 Click, Shift, Ctrl | In each mode click a face, an edge, a vertex, a point; Shift+click adds, Ctrl/Cmd+click toggles, a click on empty space clears (not with Shift or Ctrl). The status reads "n selected", or "Ready" when empty. `cad_select {"items":[["<id>","face",3]],"toggle":true}` | The same clicks | The same items `[node, kind, index]` in both; locked and hidden nodes are never picked and don't hide what is behind them |
| CAD-21 Hover | Move the pointer over the model in each mode: the face outline, edge, vertex mark (or, in body mode, the body's box) under it lights in the accent colour | The same | The same item lights; the selection, inspector and status never change from a hover; the view never stutters while hovering edges on a large model |
| CAD-22 Box select | Drag more than 6 px: a rubber band; on release, in body (and face, point) mode the bodies whose bounding box lies inside, in edge mode the edges wholly inside, in vertex mode the vertices inside; Shift or Ctrl extends. `cad_box_select {"rect":[x0,y0,x1,y1]}` | The same drag | The same items; locked nodes are taken by the box in both |
| CAD-23 Alt menu | In face mode, Alt+click where two faces meet: a list "name: face #i" (nearest first; `system_ui` `cad:candidate:<n>`). Choose one; Escape or a click elsewhere closes it. `cad_candidates` | Alt+click at the same spot | The same entries; the chosen one is selected with the Shift/Ctrl rule of the click |
| CAD-24 Select All, Invert, Same Material | Ctrl/Cmd+A, Ctrl/Cmd+Shift+I, Ctrl/Cmd+Shift+M, or the strip's **Select All**, **Invert**, **Same Material** (`cad_select_all`, `cad_invert_selection`, `cad_select_same_material`) | Edit ▸ Select All, Invert Selection, Select Same Material | The same body sets (visible bodies, sheets, curves, instances, meshes). Same Material with nothing selected, or with a body that has no material, is refused naming why (RoboCAD does nothing, or selects every body without a material) |
| CAD-25 Edges → faces | In edge mode select two edges, then **Edges → Faces** (`cad_edges_to_faces`) | Edit ▸ "Selection: edges → bounding faces" | The same face indices; the mode becomes Face in both. Before the topology has loaded it is refused naming the node, never guessed |
| CAD-26 Inspector: sub-body | Select a face, an edge, a vertex, a point: the right dock opens with "Face 3" / "Edge 7" / "Vertex 2" / "Point on face 4" (kind, area mm², centroid, normal, radius, as RoboCAD returns them; "fetching…" while loading), then the node | `GET /nodes/<id>/faces`, `/edges`, `/vertices` on RoboCAD's port | Every value is RoboCAD's exactly; nothing is filled in |
| CAD-27 Move | Select a body, G (`cad_tool {"tool":"move"}`): the gizmo at its pivot. Drag an axis: the body follows, the readout shows "Δ = (dx, dy, dz)  \|d mm\|"; hold Ctrl/Cmd: 10 mm steps; drag the centre handle: moves in the screen plane. Release: one commit. Press Escape during another drag: the body returns and the tool goes back to Select. `cad_transform {"translation":[10,0,0]}` | G, the same drags | The same final placement (`GET /nodes/<id>` transform); the body never jumps back while RoboCAD answers; the cancelled drag sent nothing |
| CAD-28 Rotate and scale | R: drag a ring (Ctrl/Cmd: 15° steps). S: drag an axis handle (uniform; Ctrl/Cmd: steps of 0.1). `cad_transform {"axis":[0,0,1],"angle_deg":30}`, `{"scale":1.5}` | R and S, the same drags | The same result for one selected node (pivot: the node's pivot, else its mass centroid). With several nodes selected the pivot is the centre of their drawn bounds, not RoboCAD's mass-weighted centroid (recorded difference) |
| CAD-29 Push/pull and offset | D (the mode becomes Face): press a planar face and drag along its normal (Ctrl/Cmd: 10 mm steps); the outline previews it. Release: `push_pull`. Hold Shift at release, or use Shift+D, or pick a cylindrical face: `offset_faces`. `cad_push_pull {"node":"<id>","face":3,"distance":5}`, `cad_offset_faces` | D and Shift+D, the same drags | The same solid (volume and face count in the inspector); a curved face is offset, never pushed |
| CAD-30 Numeric bar | During Move, press Tab: fields dx, dy, dz open on the first. Type `20mm + 0.3`: it reads "= 20.3 mm". Type `20mx`: a red border and the error naming the token and its position; Enter is refused. Tab cycles fields; Enter commits once; Escape cancels. Also for Rotate (angle, `45deg`), Scale (factor) and Push/Pull (distance). `cad_numeric {"values":["20mm + 0.3","0","0"]}` | Tab in the same tool, the same text | The same evaluations (also `1in`, `pi*10`) and the same result; RoboCAD also refuses the bad text |
| CAD-31 Live dimensions | Select tool, face mode: select a cylindrical face: "Ø name" in the bar; two parallel planar faces: "Distance" (the second moves); two other planar faces: "Angle"; a circular edge: "Ø edge i"; a sphere: "R …" read-only with "use Scale about the centre". Double-click a face (face mode): its diameter, or its distance to the opposite face, focused. Enter commits. `cad_set_dimension {"node":"<id>","dimension":"diameter","faces":[4],"value":8}` | The same selections; double-click the face | The same fields, values and result. RoboCAD also takes the double-click in body mode; the viewer takes it in face mode only (reported, not yet changed) |
| CAD-32 Snapping and measure | M: hovering shows the snap marker and "vertex  (x, y, z)" (also midpoint, center, grid, free); hold Alt: always free. Click two points: the tool bar and status show "12.000 mm" (or "R 3.000 mm  (Ø 6.000)", "90.00°"). Shift on the second click keeps it: a measure node appears in the tree. `cad_measure {"a":…,"b":…,"keep":true}` | M, the same clicks; Shift+click | The same snaps and value. RoboCAD also copies the value to the clipboard; the viewer does not (recorded difference). The same circular edge twice gives its radius in the viewer, 0 mm in RoboCAD (a RoboCAD bug) |
| CAD-33 One undo step per commit | After CAD-27 to CAD-32, read the inspector's History; Cmd/Ctrl+Z through them | Edit ▸ Undo through the same edits | Each commit (drag release, numeric Enter, dimension Enter, kept measurement) is exactly one step in RoboCAD's history, and each undo reverses exactly one; no preview stays on screen after an undo |
| CAD-34 Refused while an edit is in flight | Release a drag, then at once release another (or send `cad_transform` while the first is pending). In Part B, edit in RoboCAD's window during a drag, then release | (n/a) | The second is refused "another CAD edit is in flight: …" and the stale one "the document changed since the preview began (revision …); nothing was sent"; RoboCAD's history shows only the edits that were sent |

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
| CAD-35 Box (corner) | Toolbar **Box**, Create ▸ Box (corner), or Shift+A then B. Press on the ground, drag the base, release, move to drag the height, click. The preview shows the base and top in light blue and the readout "20 mm × 20 mm × 10 mm". Again, but press Tab during the drag: the form's corner takes the press point; type width 30, depth 15, height 5 and press Enter. `cad_run {"id":"tool.box","params":{"corner":[0,0,0],"width":30,"depth":15,"height":5}}` | The toolbar's Box, the same drag; Tab, the same sizes, Enter | The same box (corner, size). The undo label is "Box" in the viewer and "Extrude" in RoboCAD (recorded difference: the viewer sends one `Ops.box`); the node is named "Box" in both. Snapping to a vertex starts the base there in both |
| CAD-36 Box (centre) | Create ▸ Box (centre) (palette "centre"): the same drag from the centre; Tab sizes | Create ▸ Box (centre) | The base is centred on the press point and sits on the plane (not centred in height) in both |
| CAD-37 Cylinder | Toolbar **Cylinder** or Shift+A, C: drag the radius, then the height (a downward drag builds down); Tab: diameter, height | The same | The same base, axis direction, diameter and height; the readout "Ø 10 mm × 10 mm" |
| CAD-38 Sphere | Toolbar **Sphere** or Shift+A, S: press the centre, drag the radius, release (it finishes on release); Tab: diameter | The same | The same centre and radius; S as the chord's second key does not also pick the Scale tool |
| CAD-39 Fillet | Ctrl/Cmd+F (toolbar **Fillet**): the mode becomes Edge, the form opens at the 3D view's top right with "radius 1.0" and the hint "fillet: select edges (click adds) then type the size • Enter applies". Click two edges of one body and one of another (a second click on an edge removes it); Tab, type `2`, Enter. `cad_run {"id":"tool.fillet","params":{"radius":"2 mm"},"items":[["<id>","edge",3]]}` | Ctrl+F, the same edges, Tab, 2, Enter | The same fillets. History shows one "Fillet" step per body in both; the selection clears and the tool stays active in both. With no edge picked, Enter is refused "Select one or more edges first" in both |
| CAD-40 Variable and chordal fillet | Modify ▸ Variable fillet (start radius, end radius) and Modify ▸ Chordal fillet (chord) on the same edges | The same menu entries | The same results and one step per body |
| CAD-41 Chamfer | Ctrl/Cmd+Shift+F: pick edges; distance 1.5, angle 45 → Enter; again with angle 30 | The same | The same chamfers. At 45° only the distance is sent (`cad_state` edit label "Chamfer …: distance 1.5 mm"), at 30° the angle too, as RoboCAD |
| CAD-42 Fillet all | Select a body, Modify ▸ Fillet all edges…: a modal form "Radius (mm):" 1.0 (0.01 to 100); type 0.5, OK. With nothing selected the entry is disabled and says "Select the bodies to fillet" | Modify ▸ Fillet all edges…, 0.5 | The same result, one "Fillet all" per selected body. RoboCAD opens its dialog even with nothing selected and then does nothing (recorded difference) |
| CAD-43 Full round | Edge mode: select two opposite edges of one face, Modify ▸ Full round (two edges); then try one edge, and edges of two bodies | The same | The same full round; the bad selections are refused "Select two edges of the same body" in both |
| CAD-44 Remove fillets | Face mode: select fillet faces (on two bodies), Modify ▸ Remove fillets (selected faces) | The same | The same faces removed, one step per body; with no face selected the viewer refuses "Select the fillet faces to remove" (RoboCAD silent) |
| CAD-45 Shell | Ctrl/Cmd+Shift+H (toolbar **Shell**): the mode becomes Face; click the top face (it toggles), type wall 2, Enter. Also with no face (a closed shell) | Ctrl+Shift+H, then select the face with the Select tool first, wall 2, Enter | The same hollow body. In RoboCAD a face click while the shell tool is active selects nothing (its `ShellTool.press` only toggles edges), so pre-select the face there; the viewer toggles faces (recorded difference) |
| CAD-46 Thicken | Select a sheet (and a body: it is ignored), Modify ▸ Thicken sheet…: "Thickness (mm):" 2.0 | The same | The same solid; with no sheet selected both refuse "Select a sheet" |
| CAD-47 Draft | Face mode: select side faces, Modify ▸ Draft faces…: "Angle (degrees):" 2.0 (−45 to 45) and a neutral plane ("active": XY with no plane active) | The same, no active plane | The same draft, pull +Z about XY. The viewer's form also offers XY, XZ and YZ by name; with a plane active both use it (CAD-95) |
| CAD-48 Delete faces | Face mode: select faces, Modify ▸ Delete faces (heal) | The same | The same healed body, one step per body; the selection clears in both |
| CAD-49 Mirror and live mirror | Select bodies, Ctrl/Cmd+M; then Modify ▸ Mirror as live instance. REST `cad_run {"id":"tool.mirror","params":{"plane":"xz"}}` mirrors about XZ | Ctrl+M with no active plane; Mirror as live instance | The same mirrored copies about YZ; the live one follows its source in both (move the source to check) |
| CAD-50 Array | Ctrl/Cmd+Shift+A: the modal Array form (Kind rectangular: Count X/Y/Z, Mode "count + spacing" or "count + total extent", Spacing or extent X / Y / Z; As live instances; Merge into one body). 3 × 2 × 1 at 10, 10, 10, OK; again with extent; again Kind radial (count 6, total 360, axis plane XY) | Ctrl+Shift+A, the same dialog values | The same copies and positions; the rows switch with the kind in both. The viewer's radial axis is the chosen plane's normal through the origin (RoboCAD: the active plane's) |
| CAD-51 Instance | Select two bodies, Modify ▸ Instance selected | The same | One instance per body, each offset +20 mm in X, one step each; nothing selected: the viewer refuses "Select the bodies to instance" (RoboCAD silent) |
| CAD-52 Make unique | Select an instance: Modify ▸ Make instance unique, or right-click in the 3D view ▸ Make unique (bake instance) | The outliner's right-click ▸ Make unique (bake instance), or Modify ▸ Make instance unique | The instance becomes a body in both; a selection without an instance is refused "Select an instance to make unique" in the viewer (RoboCAD skips it silently); the viewer offers the entry in the 3D view's menu, RoboCAD in the outliner's |
| CAD-53 Set pivot at cursor snap | Select a body, point at a vertex of another body (the snap marker shows it), then the palette ▸ "Set pivot at cursor snap" (Help menu, as RoboCAD's "Tools" category). `cad_run {"id":"tool.set_pivot","params":{"point":[0,0,10]}}` | Help ▸ Set pivot at cursor snap with the cursor on the same vertex (use its palette with the pointer there) | The inspector's pivot reads the vertex in both. Over a face with no snap point the viewer takes the point on the face (RoboCAD takes the grid or plane point; recorded difference) |
| CAD-54 Inspector pivot and transform | Select a node: the inspector's pivot field; press it, type `10, 0, 5mm + 1`, Enter; **Clear pivot**. Select an instance: its translation, axis, angle and scale fields; change the angle to `30deg`. A bad value (`10, x, 0`) keeps the field open with the error naming the token | `PATCH /nodes/<id> {"pivot": [10,0,6]}` and `{"transform": …}` on RoboCAD's port, or its properties panel | One undo step each with RoboCAD's result; a component member's pivot and an occurrence's transform show RoboCAD's refusal instead of the field |
| CAD-55 Delete as one step | Select three bodies, Delete (or Backspace, the **Delete** button, Edit ▸ Delete) | Delete with the same selection | All three go in one undo step "Delete" in both; Undo brings all three back |
| CAD-56 Union, subtract, intersect | Select the target, then Shift-select the tools; Ctrl/Cmd+U, Ctrl/Cmd+Shift+U, Ctrl/Cmd+Alt+U (toolbar **Union**, **Subtract**). With one body selected: the status reads "Union: Select the target body first, then the tools" | The same selections and keys | The same result on the first selected body; the tools are removed and the selection clears in both; RoboCAD's message is the same |
| CAD-57 Region | Two overlapping bodies, Modify ▸ Region (overlap as new body); then with three | The same | A new "Region" body in both; three are refused "Select exactly two bodies" in both |
| CAD-58 Join and unjoin | Two bodies, J; then select the result, Shift+J | The same | The same joined body and the same parts after unjoin; J with one body is refused in the viewer ("Select two or more bodies to join"), RoboCAD calls join anyway |
| CAD-59 Dissolve | A body with redundant edges (after a union), Modify ▸ Dissolve redundant topology | The same | The same face count after, one step per body |
| CAD-60 Cut | Select a body, Modify ▸ Cut with active plane (with no plane active both cut with XY; `cad_run {"id":"tool.cut_plane","params":{"plane":"yz"}}` names another); then select a body and a sheet, Modify ▸ Cut with selected sheet/curve | The same, no active plane; the same body and sheet | The same pieces. The sheet cut works headless only since this epic's `ArgConverter` fix (`cad/tests/test_api_cut_cutter.py`) |
| CAD-61 Split faces | Select a body crossing z = 0, Modify ▸ Split faces with active plane | The same | The same faces split along XY |
| CAD-62 Imprint | A body, then a curve or body touching it, Modify ▸ Imprint selected curve/body | The same | The same imprinted edges; one node refused "Select the body, then the tool" in both |
| CAD-63 Project curve | A curve or sketch, then a body; orbit to look along −Z; Modify ▸ Project curve onto body | The same, from the same direction | The same projected curve; the direction is the view's in both (`cad_state` edit label), or REST's `direction` |
| CAD-64 Silhouette | Select a body, Modify ▸ Silhouette onto active plane | The same | The same silhouette curve on XY |
| CAD-65 Control points | Face mode: a curved face, Advanced ▸ Show/edit control points (advanced) | The same | The same points and rows in RoboCAD's pink, and the status "N control points (edit via Ops.set_control_points; …)"; nothing is written. Changing the document clears the overlay in both |
| CAD-66 Raise degree | The same face, Advanced ▸ Raise face degree | The same | One "Raise degree" step in both; the face is 4 × 4 after (`tool.control_points` again) |
| CAD-67 Rebuild face | Advanced ▸ Rebuild face…: "Spans per direction:" 4 (1 to 64) | The same | The same rebuilt face |
| CAD-68 Dependent offset | Select a face, then Shift-select another body; Modify ▸ Dependent offset (face to body)…: "Clearance (mm):" 0.2 (−10 to 10) | The same | The same offset face; a face alone is refused "Select a face, then the body to offset it to" in both |
| CAD-69 Copy and paste with placement | Select two bodies, Ctrl/Cmd+C: "Copied 2 item(s) with placement"; Ctrl/Cmd+V: two new bodies in place, one undo step "Paste". `cad_state.ops.clipboard` shows the copy. Copy in the viewer, then paste in RoboCAD's window | Ctrl+C, Ctrl+V | The same new nodes at the same placement, one step each time. The viewer's clip stays in the viewer: it does not reach RoboCAD's window or the OS clipboard (recorded difference) |
| CAD-70 Curvature comb | Select a curve, Inspect ▸ Curvature comb on selected curve | The same | The same comb lines (scale 5, 48 samples) in RoboCAD's violet; a sketch shows none in both; with a curve then a sketch selected the viewer shows none, RoboCAD the curve's (recorded difference) |
| CAD-71 Continuity | Select a filleted body, Inspect ▸ Continuity check (G0/G1/G2) | The same | The same edge colours (G0 red, G1 amber, G2 green, boundary grey) and the status "Continuity: {'G0': …, 'G1': …, 'G2': …, 'boundary': …}" |
| CAD-72 Toolbar | Under CAD mode's header: the menu tabs, then RoboCAD's 25 tools in order. Hover each: the hint names its keys and, when disabled, why ("Annotate belongs to the cad-organize epic; …"). Activate Move, then Fillet: their buttons light. Scroll the row with the wheel on a narrow window | RoboCAD's toolbar | The same buttons in the same order; the native ones run as in RoboCAD; later-epic buttons are disabled naming their epic; Qt folds the overflow behind "»" where the viewer scrolls (recorded difference) |
| CAD-73 Right-click menu | Right-click (without dragging) on the 3D view: Annotate, Comments panel, Push/Pull face, Fillet, Chamfer, Hollow / shell, Union, Subtract, Mirror, Array…, Measure, Isolate, Hide, Delete; with an instance selected also Make unique (bake instance). `cad_surface {"surface":{"kind":"context"}}` | Right-click in RoboCAD's viewport | The same entries in order; each enabled by the selection; Annotate, Comments panel, Isolate and Hide disabled naming their epic; a right drag still orbits |
| CAD-74 Radial menus | Space: the view pie at the pointer (Front, Top, Right, Iso, Ortho, Grid, Mode, Fit): hover highlights, release on Fit frames the model, the others say "… belongs to the cad-views-export epic". Q: Body, Face, Edge, Vertex, Point; release on Edge switches the mode. Escape or the dead centre closes; a press outside closes | Space and Q in RoboCAD's viewport | The same entries and layout (first at the top, then clockwise); the viewer's pills are rounded rectangles where RoboCAD's are ellipses (recorded difference); Space types a space while a text field has the keyboard |
| CAD-75 Palette | Control+Space (Command+Space is Spotlight on macOS) or Shift+F: "Type a command… (Ctrl+Space)". Type `fil`: "Fill / patch selected curve" (runnable since cad-sketch), then Fillet and Fillet all edges… (sorted by score, then label, as RoboCAD); Up/Down; Enter runs the highlighted row. Type `same`: "Edit: Select Same Material    [Ctrl+Shift+M]  ⚠ conflicts with Robot: add motor from library…". Rows of later epics read "(cad-views-export)" etc. and are disabled; GUI-only rows read "(GUI-only)" | Ctrl+Space, the same queries | The same ranking and the same conflict warning; the viewer also lists commands it cannot run yet, marked and disabled |
| CAD-76 Menus by category | Click each tab: File, Edit, View, Select, Create, Sketch, Modify, Planes, Inspect, Print, Advanced, Outliner, Robot, Bridge, Simulation, Help; each lists its commands with keys; "General", "Window" and "Tools" commands (Command palette, Numeric entry, Select tool, Move, Set pivot at cursor snap, …) are in Help. `cad_surface {"surface":{"kind":"menu","category":"Modify"}}` | RoboCAD's menu bar | The same menus, entries, order and keys; a click runs the command and closes the menu |

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
| CAD-77 Active plane XY / XZ / YZ | Planes ▸ Active plane: XZ (or the palette, or `cad_invoke {"id":"tool.plane_xz"}`): the status reads "Active plane set", a translucent ±60 mm square is drawn in XZ, `cad_state.plane` reads XZ. Then YZ, then XY | Planes ▸ Active plane: XZ, YZ, XY | The same status line; a later sketch or primitive lands on the same plane in both. RoboCAD draws no square for a named plane (the viewer does; recorded) |
| CAD-78 2D snapping | Planes ▸ Toggle 2D snapping to the active plane: "2D snapping on". With XZ active, M (measure) and hover a vertex off the plane: the marker sits projected onto XZ; away from geometry the readout says "grid" or "plane". Toggle again: "2D snapping off" | Planes ▸ Toggle 2D snapping to the active plane; M, the same hover | The same status lines and the same snapped points |
| CAD-79 Plane from face | Ctrl/Cmd+P (Planes ▸ Plane from face): the mode becomes Face and the hint "Click a face". Click a body's top face: a plane node appears in the tree, "Active plane set", its square is brighter than the other planes'; the tool stays active, Escape ends it. `cad_run {"id":"tool.plane","items":[["<id>","face",3]],"revision":N}` | Ctrl+P, click the same face | The same plane origin and normal, one undo step each, the new plane active in both |
| CAD-80 Plane from three points, two points and midplane | Planes ▸ Plane from three points: the mode becomes Vertex; click three vertices (the status counts "(1 of 3)", "(2 of 3)"). Planes ▸ Plane from two points (camera): two vertices, the view's direction. Planes ▸ Midplane between two faces: two parallel faces | The same menu entries and clicks, the same camera direction | The same planes, each active after it is made |
| CAD-81 Selecting a plane node | Click a plane node in the tree (one node selected): it becomes the active plane ("Active plane set") and its square brightens; a sketch drawn next goes on it | RoboCAD has no such gesture: make the same plane active by re-running its plane tool on the same picks | The same active plane (the viewer's gesture is a recorded native addition) |
| CAD-82 Line, chaining | L (Sketch ▸ Sketch: Line): click three points on the plane; the preview follows the cursor in light blue and the readout reads "length L  angle A"; two connected lines. Escape ends the chain. Then L, one click, Tab: length 20, angle 30, Enter | L, the same clicks; Tab 20, 30, Enter | The same curves in the same sketch. The first shape created the sketch in the viewer (undo steps "Sketch", then the shape); RoboCAD created it when the tool started |
| CAD-83 Rectangle, centre rectangle, circles, arc | Shift+L (two corners), Sketch: Rectangle (centre), C (centre, rim; Tab diameter), Sketch: Circle (two points), Sketch: Circle (three points), A (Sketch: Arc (three points)). Three collinear points for the arc are refused "the three points are collinear" | The same tools and clicks (RoboCAD has no key for the arc: use Sketch ▸ Sketch: Arc (three points); its A does nothing) | The same curves; the collinear arc is refused in both (RoboCAD after its kernel call); OK on a tool with no Tab values is refused by name in the viewer (RoboCAD records an empty step) |
| CAD-84 Polygon sides memory | Shift+P: Tab, radius 10, sides 8, Enter (an octagon at the plane origin). Shift+P again: the sides field opens at 8; click centre and corner: an octagon turned toward the second click | Shift+P, Tab 10, 8, Enter; Shift+P again | The same octagons; both remember 8 sides |
| CAD-85 Slot, ellipse, spiral | Shift+S: two clicks for the axis, a third for the width; Sketch: Ellipse: centre, x radius, y radius; Sketch: Spiral: centre and radius (3 turns); each again with Tab values | The same tools and clicks | The same curves from `GET /nodes/<id>/sketch`. The slot's caps are drawn bulging outward in the viewer and inward in RoboCAD's viewport; extrude both (CAD-91) and compare the solids, which match |
| CAD-86 Spline | Shift+C: click four points, Enter: one spline. Again, finishing with a double-click (within 400 ms and 5 px). Enter with one point does nothing; the form's OK is refused by name | Shift+C, the same clicks, Enter; then a double-click | The same splines in both |
| CAD-87 Text | T (Sketch ▸ Sketch: Text): the form's "Text to sketch:" field has the keyboard; type `RC`, Enter, click on the plane: outlines 10 mm high. Again with Tab height 5, Enter. With the field empty a click is refused "type the text to sketch first …" | T: the "Text to sketch:" dialog, `RC`, OK, click; then Tab height 5 | The same outlines. The viewer's preview is a placeholder box (recorded) |
| CAD-88 Tab, Enter and Escape in a sketch tool | During a rectangle, after the first click: Tab focuses the first field, Enter commits the typed values at the first click; Enter outside the fields does nothing; Escape drops the shape (nothing sent) and ends the tool | The same keys | The same results; no edit after Escape in either |
| CAD-89 Offset, fillet corners, join | Select the sketch with the rectangle: Sketch ▸ Sketch: offset selected curve… "Distance (mm):" 1.0; Sketch ▸ Sketch: fillet corner… "Radius (mm):" 2.0 (four corners rounded); again with 50 (refused: no corner takes it); two lines end to end, Sketch ▸ Sketch: join curves; join on a sketch of one curve is refused | The same Sketch menu entries and values | The same curves (labels "Offset curves", "Fillet corners", "Join curves" in RoboCAD's history for its own edits). RoboCAD records an empty step for the radius 50 and the one-curve join where the viewer refuses by name |
| CAD-90 `cad_sketch` (REST) | `cad_sketch {"node":"<sketch>","calls":[["join",[[0,1]]]],"revision":N}`; `[["trim",[0,[1],[5.0,2.5]]]]`; a call naming curve 9 of 3 is refused naming the call and argument; without `node`, `{"plane":"xz","calls":[["circle",[[0,0],5]]]}` goes to the XZ sketch RoboCAD's tools would pick, or a new one | `POST http://127.0.0.1:<RoboCAD port>/nodes/<id>/sketch {"calls": …}` with the same calls | The same curves and one "Sketch (API)" step per call list in both; a join of two curves and a two-curve trim work through REST in both (since the api.py fix; `cd cad && .venv/bin/pytest -q tests/test_api_sketch_calls.py`) |
| CAD-91 Extrude, taper, Shift/Ctrl/Alt | A sketch on a body's top face, selected; X: the hint names the modifiers. Drag up: the profile's outline at the base and the top, the readout "extrude h"; release: a new body. Again holding Ctrl at the release: unites with the body under the selection; Shift: subtracts; Alt: intersects. Tab: distance 5, taper 10, Enter. `cad_run {"id":"tool.extrude","params":{"distance":"5","taper":"0","boolean":"union"}}` | X, the same drags and modifiers; Tab 5, 10, Enter | The same volumes and face counts. Both drags send taper 0 (only Tab sends the taper). RoboCAD previews a shaded body (the viewer draws outlines without the taper) |
| CAD-92 Revolve | A sketch off the axis, Shift+R: Tab, angle 180, Enter: a half revolution about the sketch plane's x axis. Press and release in the view: a full 360° revolution (the readout says so) | Shift+R, Tab 180, Enter; then press and release in the view | The same solids: both revolve 360° on a click whatever the angle field says |
| CAD-93 Sweep, pipe, loft, fill | Create ▸ Sweep (profile + path from selection): select the profile, then the path; "Twist (degrees):" 0. Create ▸ Pipe along selected curve… "Diameter (mm):" 4.0. Create ▸ Loft selected sketches with two sketches on parallel planes. Create ▸ Fill / patch selected curve on a closed curve. Each with too few selected | The same Create entries, selections and values | The same solids; the same refusals ("Select the profile sketch, then the path sketch", "Select a curve or sketch", "Select two or more sketches to loft", "Select a closed curve") |
| CAD-94 Primitives on the active plane | Active plane XZ: Box (corner), Box (centre), Cylinder, Sphere drags (CAD-35 to CAD-38) | The same with XZ active | The same solids on XZ (box volumes and positions match); the box's undo label is "Box" in the viewer and "Extrude" in RoboCAD (recorded) |
| CAD-95 Plane-dependent operations | With the CAD-79 plane active: Ctrl/Cmd+M (mirror), Modify ▸ Cut with active plane, Split faces with active plane, Silhouette onto active plane, Draft faces… (neutral "active"), Array… radial | The same with the same plane active | The same results about that plane in both |
| CAD-96 Toolbar and right-click menu | The toolbar's Rectangle, Circle, Slot and Extrude are enabled; Rectangle lights while its tool is active. Right-click the 3D view: RoboCAD's 14 entries, then a "Sketch" section with the 13 sketch tools; each starts its tool | RoboCAD's toolbar and right-click menu | The same tools start from the same buttons; RoboCAD's sketch buttons never light and its menu has no Sketch section (both recorded native additions) |
| CAD-97 A shape in progress blocks leaving | Click two points of a slot, then the switcher's **Build**: refused "a sketch slot is in progress (2 point(s) clicked): finish it or press Escape". After a line the chained point counts too. Escape, then **Build** succeeds (with no unsaved edits, or after saving) | (n/a: RoboCAD has one window; changing tool drops the points) | Refused while points are clicked; nothing sent |
| CAD-98 Undo through the epic | Ctrl/Cmd+Z through CAD-79 to CAD-93 | Edit ▸ Undo through the same edits | Each plane, shape, sketch edit and solid is one step in RoboCAD's history (a new sketch adds its "Sketch" step), and each undo reverses exactly one |

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
| CAD-99 Orbit, pan, zoom to the cursor | Right-drag orbits (turntable; the view stops short of straight down, 89.5°); middle-drag or Shift+right-drag pans; the wheel zooms toward the point under the cursor: put the cursor on a corner of a part and roll in. REST `camera_orbit {"dx":100,"dy":0}`, `camera_pan`, `camera_zoom {"factor":0.8,"at":[x,y]}` | The same drags and wheel over the same corner | The model turns, slides and zooms the same way and at a similar rate; the corner under the cursor stays under it while zooming in both. RoboCAD's other gestures are CAD-129 |
| CAD-100 Named views | 1 front, 3 right, 7 top, 0 iso; Ctrl/Cmd+1 back, Ctrl/Cmd+3 left, Ctrl/Cmd+7 bottom (the keypad's digits too); also View ▸ View front … View iso, and Space's view radial (Front, Top, Right, Iso). `camera_view {"view":"front"}` | The same keys and View menu entries | Each key shows the same side of the model in both, at the same distance and focus (RoboCAD's yaw and pitch table); the Ctrl views are the opposite sides |
| CAD-101 Focus selection | Select one part, press F (View ▸ Focus Selection); select a group: F frames it and its children; select nothing: F frames everything (as Home) | The same selections and F | The same part (or group) fills the view in both |
| CAD-102 Orthographic and field of view | 5 (View ▸ Orthographic, the radial's Ortho) toggles orthographic; View ▸ Set field of view… opens "Field of view" at the lower right: type 30, Enter (5–120, one decimal; out-of-range is refused under the field). `camera_projection`, `camera_fov {"degrees":30}` | 5; View ▸ Set field of view… (Degrees: 30) | Orthographic has no perspective in both and keeps the model's size on screen; at 30° both show the same narrower perspective |
| CAD-103 Trackball | View ▸ Toggle orbit: turntable / trackball, then right-drag across the top of the model; toggle back. `camera_orbit_mode`, `camera_state` (`mode`) | The same command (status "Orbit: trackball") and drag | The model tumbles freely (it can roll past upside down) in both; back in turntable the view returns to the nearest upright heading |
| CAD-104 View cube | The cube net at the top right of the 3D view (Top; Left, Front, Right; Iso, Bottom, Back; the face you look at is lit): click Front, then Front again; click Iso. The **Cube** chip hides and shows it | Click the cube's front face, then again; click a corner | The first click shows the front, the second the back (RoboCAD's opposite), in both. The native cube is a net of buttons, not a shaded 3D cube (recorded) |
| CAD-105 Display modes | Z cycles Shaded → Shaded + edges → Wireframe → X-ray → Matcap → Render → Shaded; View ▸ Display: shaded … Display: render and the display panel's six buttons choose one. `cad_display {"mode":"wireframe"}`, `{"next":true}` | Z; View ▸ Display: … | Same order and the same look for shaded, shaded with edges (dark B-rep edges), wireframe and xray (translucent with edges). Matcap is approximated (clay tint, no sphere image) and Render has no ground shadow (both recorded); Inspect ▸ Normal-direction shading switches both to xray |
| CAD-106 Grid | Ctrl/Cmd+G (View ▸ Grid, the **Grid** chip, the radial's Grid). `cad_display {"toggle":"grid"}` | Ctrl+G | The same 10 mm grid on the model's XY plane, ±200 mm, every 5th line darker, red X, green Y and blue Z axes, shown and hidden together |
| CAD-107 Build plate and overhangs | Ctrl/Cmd+Shift+B (Print ▸ Build Plate Preview, the **Plate** chip) | Ctrl+Shift+B | The same 220 × 220 mm plate; the same downward faces are shaded red as overhangs (45°) in both; off again in both |
| CAD-108 High contrast | View ▸ High-Contrast Theme (the **Contrast** chip) | View ▸ High-Contrast Theme | The 3D view turns light with a lighter grid and black edges in both. Only the viewer's 3D view changes (its panels keep their colours) and the setting is not kept after a restart (both recorded) |
| CAD-109 Section preview | Ctrl/Cmd+Shift+X (Inspect ▸ Section Analysis, the **Section** chip) | Ctrl+Shift+X | Both start on XZ through the middle of the model (in Y); the same side of the plane is cut away in both and the cut is outlined in red; hovering and picking never select the removed part; the same key turns it off |
| CAD-110 Section plane | With the section on: the panel's **X**, **Y** and **Z** chips (planes through the model's centre), then **Rotate**; click the toolbar's offset field ("offset, e.g. 5 or 2 cm"), type 5, Enter: the plane moves 5 mm along its normal; then `0.5 cm`, Enter; `abc` is refused under the field. `cad_section {"offset":5}`; `cad_section {"axis":"z","offset":10}` | Tab, type 5, Enter; drag the plane; R | The same cuts for the same planes and offsets in both; Rotate and R turn the plane 90° about Z the same way. In the viewer R stays the Rotate tool, Tab the numeric bar, and a left drag on the plane box-selects (or Alt-orbits) rather than moving it (recorded) |
| CAD-111 Exact section | Section on Z at offset 0 (`cad_section {"axis":"z","offset":0}`), select a body, run `system_ui` `cad:section:exact` ("Exact section of …") or `cad_section {"exact":"<id>"}` | `curl 'http://127.0.0.1:<RoboCAD port>/nodes/<id>/section?plane=xy'` | A yellow exact outline appears over the red preview outline and follows the B-rep; on any plane other than xy, xz, yz through the origin or a plane node it is refused, naming why (RoboCAD's route takes only those); after an edit to the body it is re-read |
| CAD-112 Isolate | Select one part, press `/` (View ▸ Isolate, right-click ▸ Isolate); then Ctrl/Cmd+Z | `/`; Edit ▸ Undo | Everything but the part, its children and parents is hidden in both; one undo step "Isolate" brings it back. With nothing selected the viewer refuses "Select the nodes to isolate" (RoboCAD hides everything; recorded) |
| CAD-113 Hide and Show All | Select two parts, press H (View ▸ Hide, right-click ▸ Hide); then Alt/Option+H (View ▸ Show All); undo both | H; Alt+H; Edit ▸ Undo twice | H hides the selection as one step ("Hide"); Show All shows every node ("Show all"); the tree shows the same visibility; each undo reverses one. With nothing selected the viewer refuses "Select the nodes to hide" (recorded) |
| CAD-114 Save a view | View ▸ Saved Views opens the panel at the lower right: set a view (front, ortho, section on, grid off, wireframe), type "Front cutaway" in "View name, e.g. Worm drive cutaway", **Save current view**. `cad_views {"op":"save","name":"Front cutaway"}` | View ▸ Saved Views: the same view, the same name, Save current view | Both list "Front cutaway / Orthographic · Cutaway"; "Saved inside this CAD file · edits support Undo"; a blank or 121-character name is refused before anything is sent; one undo step in RoboCAD's history ("Save view") |
| CAD-115 Rename, replace, delete | On the row: **Rename…** (type "Front A-A", Enter), turn the camera, **Replace with current**, then **Delete**; Ctrl/Cmd+Z after each. `cad_views {"op":"rename","id":"…","name":"…"}`, `"replace"`, `"delete"` | Rename…, Replace with current, Delete; Edit ▸ Undo | Each is one undo step with RoboCAD's label ("Update saved view", "Delete saved view") and each undo restores the list as it was, in both |
| CAD-116 Restore, across both | Save a view in the viewer and Save the file; open that copy in RoboCAD and Restore it there. Save a view in RoboCAD, save, open that copy in the viewer: **Restore view**. Compare `unzip -p <file>.rcad manifest.json` (`saved_views`) | Restore view (or double-click the row) | A view saved in either one restores in the other with the same direction, distance, orthographic or perspective, field of view, display mode, grid and section; the `saved_views` entries have the same keys. The viewer restores with a button, not a double-click (recorded) |
| CAD-117 Tessellation tolerance | Select a curved body; in the inspector type 0.5 in "Tessellation tolerance (mm)", Enter; then 0.01; then Ctrl/Cmd+Z | The properties panel's "Tessellation tolerance (mm)" spin box, 0.5, then 0.01 | At 0.5 both draw the same visible facets; at 0.01 both are smooth; the viewer's change is one undo step ("Set attributes"), RoboCAD's panel records none, and the viewer's field opens empty (RoboCAD reports no current value; recorded) |
| CAD-118 File ▸ New | File ▸ New (Ctrl/Cmd+N): the path form proposes `untitled.rcad` in the document's folder; type `/tmp/cad-check/new.rcad`, OK. Try again with the same path | File ▸ New | The viewer writes the empty file, then opens it (the progress strip and status line say so); an existing path is refused ("exists: choose a new file name") and left untouched. With unsaved edits in a service this window started, New is refused before any file is written (CAD-127). REST `cad_file {"op":"new","path":"/tmp/cad-check/new2.rcad"}` answers only once the new file is open (its answer names `created` and `opened`). RoboCAD opens an untitled window instead (recorded) |
| CAD-119 File ▸ Open | File ▸ Open… (Ctrl/Cmd+O): the form lists the folder's `.rcad` files; click `turntable.rcad`, OK | File ▸ Open… | The same document opens in both; `..` and folder rows move through folders; a relative path is refused, naming why |
| CAD-120 File ▸ Save As | File ▸ Save As… (Ctrl/Cmd+Shift+S): `/tmp/cad-check/copy` (no extension), OK. Then `unzip -l /tmp/cad-check/copy.rcad` | File ▸ Save As… `/tmp/cad-check/copy-rc`, then the same `unzip -l` | Both append `.rcad`; both archives hold `thumbnail.png` (a headless service draws the viewer's with RoboCAD's snapshot renderer); the left dock names the new file, and leaving and re-entering CAD mode reopens `copy.rcad`. Plain Save writes the thumbnail too (CAD-11) |
| CAD-121 Import STEP | File ▸ Import… (Ctrl/Cmd+I): `/tmp/cad-check/print-kit.step`, OK; then undo | File ▸ Import… the same file; Edit ▸ Undo | The same new nodes, volumes and face counts; one undo step removes them in both. (An SVG or an image lands on XY in the viewer, on the active plane in RoboCAD: recorded) |
| CAD-122 Import a mesh with units | File ▸ Import… and type `/tmp/cad-check/cap.stl`: as soon as the path names the mesh, the form adds "Units of the mesh file" (empty), says "Asking RoboCAD for its guess…", then "RoboCAD's guess: … (largest extent …)" and fills the unit; OK is disabled, saying why, until then. OK. Then repeat, choosing `in` before the guess lands (the guess no longer replaces it); **Guess unit** asks again | File ▸ Import…: "Units of the file" / "This format carries no unit. What are the numbers in?" with its guess preselected | The guess is the same unit in both; the imported mesh has the same size for the same unit in both (inches 25.4× millimetres); no mesh is imported in a unit nobody picked or RoboCAD guessed. The viewer's unit is a row of its path form, not a second dialog (recorded) |
| CAD-123 Export STL, 3MF, OBJ with settings | File ▸ Export… (Ctrl/Cmd+E): Format `stl`, File `/tmp/cad-check/v.stl`, Format binary, Unit mm, Chord tolerance (mm) 0.05, Angular tolerance (°) 20; OK. Then `3mf` (Write colours, Write names) and `obj` (Scale, Up axis, Quads, N-gons, Write MTL, Write UVs) | File ▸ Export… the same files (`r.stl`, …) with the same values in its dialog | Each file is written (status "Exported … to …"); the viewer's and RoboCAD's files of each format import back into RoboCAD with the same triangles and size; a value outside a field's range is refused naming the setting; reopening the form starts from the last values sent for that format during the session (RoboCAD remembers them across launches; recorded) |
| CAD-124 Export STEP, IGES, sketch SVG | Format `step` (Schema AP214, Write names, Write colours), `iges`, and `svg` with a sketch selected (Sketch (node id) is filled; draw one as in Part E if the model has none) | The same three exports | The STEP and IGES files import back into RoboCAD with the same bodies and volumes; the SVGs show the same curves; an export RoboCAD blocks names its reason |
| CAD-125 Export drawing | File ▸ Export drawing (SVG)… (Ctrl/Cmd+Shift+D) with the section on: Front, Top, Right and Isometric view checked, Title, "Section A-A (the section tool's plane)" checked; OK | File ▸ Export drawing (SVG)… with the section on | The same four views and a "Section A-A" view, titled with the document's file name by default, in both SVGs |
| CAD-126 Render | `system_ui` `cad:file:render` ("Render (PNG)…") or `cad_render {"path":"/tmp/cad-check/v.png","view":"iso","w":1200,"h":900}` | `curl -o /tmp/cad-check/r.png 'http://127.0.0.1:<RoboCAD port>/render?view=iso&w=1200&h=900'` | Both PNGs show the same view of the model; a size outside 16–8192 px or a path without `.png` is refused before anything is sent; the window never stalls while it renders |
| CAD-127 Unsaved edits | In Part A (a service this window started) make an edit, then File ▸ Open… or New: the form says in red "Not now: … has unsaved edits …: save first", and OK is refused naming the reason; New writes no file. There is no discard button. Save (Ctrl/Cmd+S), then OK opens. Attached to RoboCAD's window (Part B), with an edit unsaved there: the form notes in amber that RoboCAD keeps its edits and this window then shows the other file; OK opens it. `cad_open` and `cad_file {"op":"open"}` follow the same rule | File ▸ Open… or New with unsaved edits: another window opens and the edits stay; close the window: "Unsaved changes" / "Save before closing?" with Save, Discard, Cancel | Nothing is lost in either: the viewer refuses rather than replace a self-started service's unsaved edits, and an attached RoboCAD keeps its own; the refusals name the document and what to do |
| CAD-128 The camera in the other modes | Switch to **Robot**, **Phenomena**, **Build** and a lesson with a 3D card (`--lessons DIR`). In each: right-drag orbits, middle or Shift+right-drag pans, the wheel zooms toward the focus (in a lesson card only with Ctrl/Cmd held, so the page scrolls); the numpad's 1/3/7 (Ctrl: opposite), 9, 5, 0 and `.`; Home returns to the mode's home view; a lesson's spinning card keeps spinning; `camera_spin {"rate":0.5}` then `{"rate":0}` | (n/a: compare with a build of 1b00d789, before the shared camera) | The orbit rate, pitch limits, zoom limits, home view, glide (or cut) to home and spin of each mode feel as they did before; a drag that starts on a side dock never moves the camera; RoboCAD's gestures (CAD-129) are CAD's only: Shift+middle still pans and the arrow keys are not camera keys in these modes |
| CAD-129 RoboCAD's camera gestures | In CAD mode with the Select tool: Shift+middle-drag orbits (a plain middle-drag still pans); Alt+right-drag orbits and snaps to the nearest axis view at every step (pitch level or ±89.5°, yaw a multiple of 90°); Alt/Option+left-drag orbits once it moves more than about 6 px, while an Alt+click on overlapping parts still opens the candidates menu and an Alt drag never box-selects; the arrow keys orbit 10° (Ctrl/Cmd: 90°), Shift+arrows pan; with a text field focused (the numeric bar, the inspector, the offset field) the arrows leave the camera alone. `camera_orbit {"degrees":[10,0]}` | The same drags and keys in RoboCAD's viewport | The same turns, snaps and pans in both, at the same steps. RoboCAD's own Alt+left-drag does not orbit (its tool takes the press, ui/app.py:542; recorded in the ledger's notes), so compare the viewer's with right-drag |
| CAD-130 Curve nodes | Select a body and run Modify ▸ Silhouette onto active plane (CAD-95): a "Silhouette" curve node appears in the tree. Toggle its visibility; select it in the tree; switch display modes (Z); turn the section on across it | The same silhouette in RoboCAD's window | The curve is drawn in both, 2 px, light blue (or the node's colour), orange while selected, in every display mode, cut by the section plane, gone while hidden. Clicking the curve in the viewer's 3D view does not select it (RoboCAD's 8 px curve pick is not ported; select it in the tree; recorded) |

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
  the toolbar, menus, palette and radials list later epics' and unported
  commands disabled, naming why; Command+Space is Spotlight's on macOS, so
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
   `cad/threads/isolation.rs:part_link` (308).
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
   (without parts deleted since) set on the one `Selection` (285) and
   published to RoboCAD (286); status "Returned to the assembly view"
   (288).
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

1. Robot mode's Open in CAD: `robot/threads/act.rs:open_in_cad` (127)
   sets `RevealThread` (139) and writes `Act<WindowAction>::Switch {Cad,
   document}` (140), both in Robot's one apply system (`RobotSet::Actions`
   within `ViewerSet::Actions`, `robot/mod.rs:196`). The reveal is state
   set before the message, so the switch handler sees it whichever of the
   two `ViewerSet::Actions` systems runs first (this frame or the next).
2. `app/switch/mod.rs:handle` (367) → `start` (458; 514-573): any refusal
   (another switch loading or being entered, the leave blockers, a
   document `prepare` refuses) returns `Err`.
3. **The line that proves no reveal stays pending:**
   `app/switch/mod.rs:460` — `drop_reveal` on every refused switch whose
   target is CAD, which clears `RevealThread` (481) unless CAD mode is
   already active or another switch to it is loading or being entered
   (476: a second Open in CAD refused as "still being applied" keeps
   the first one's reveal).
4. CAD switches never load (`prepare.rs:273-292`, `Prepared::Now`), so
   `finish_load`'s refusals cannot apply; leaving CAD mode with a reveal
   that never landed also drops it (`app/switch/leave.rs:104-106`).
5. Gap found and fixed: a refused Open in CAD left the reveal pending, so a
   later, unrelated visit to the same CAD document opened the Comments
   dock and that thread (`app/switch/mod.rs:457-462`, 468-484;
   `app/switch/leave.rs:104-106`).
6. Recorded, not fixed (low): a second Open in CAD while the first switch
   is still being entered overwrites `RevealThread` (`robot/threads/act.rs:139`)
   before its switch is refused as "still being applied", and `drop_reveal`
   keeps the reveal for the first switch (476), so the first switch lands
   on the second thread. Fixing it needs `open_in_cad` to carry the reveal
   with the switch request (on the `Switch` action or keyed by it), not as
   separate state.

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
   - the command surfaces and catalogue form (`cad/surfaces/mod.rs:384-397`,
     not consumed): their chain ends in `keys::gate` (322), so it runs
     before `Gate`, and before `CadKeySet::NumericEntry` (326); it is not
     ordered against the file form, so it stands aside by state while a
     file form is open (`CadFiles::form`, 342, 390), as calibrate and the
     threads do;
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
   threads), and the reader that does not (the surfaces) is stood aside
   for by state; the surfaces in turn stand aside for an open file form.
   Gap found and fixed (b75846ff): the results form's Escape closed it
   without clearing the key, and `transform::keys` stands aside only for
   `doc.ops`, so the same press also fired the Select tool's `cad:cancel`
   (`cad/results/forms.rs:309-314`).
5. Gap found and fixed (review follow-up): with no surface open the
   surfaces' Escape wrote `CadFormCancel` for an open operation form or
   active interaction even while a file form was open, and, being
   unordered against `files::form::input`, could see the same press before
   the file form consumed it: one Escape closed the file form and
   cancelled the tool (`cad/surfaces/mod.rs:390-391`).

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
