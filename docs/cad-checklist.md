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
bcf0c56c; cad-physical-inspect, cad-print and cad-organize were written
and checked only by reading: **Parts G, H and I have not been through
their verification passes yet** (nothing in Part I has been compiled). Each
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
this part was compiled or run before it was written: **Part G has not been
through its verification pass yet**.

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-131 Materials list and search | The Materials section lists "■ name   density g/cm³", ■ in each material's colour; type `pla` into "Search materials…", then a tag (e.g. `metal`) | The Materials dock, the same search | The same rows, colours, densities and filtered rows in both (name or tag, any case) |
| CAD-132 Apply a material | Select two bodies, click a material row, **Apply to selection**; undo; then double-click the row; with nothing selected, **Apply to selection** | The same: Apply to selection and double-click; then with nothing selected | One undo step "Material" sets both bodies in both windows (the inspector's material and mass follow); the viewer refuses an empty selection by name, where RoboCAD silently does nothing (recorded). Dragging a material onto a body is RoboCAD's only (recorded) |
| CAD-133 New material | **New…**: Name `Check PLA`, Density (g/cm³) `1.25`, OK; then New… again with Density `abc` | **New…** with the same values | The new material appears in both lists with the same density; the density that is not a number is refused naming the field (OK is disabled). Undo removes it in both |
| CAD-134 Engineering properties | Select a material row, **Material properties…** (or the inspector's **Material properties…** on a body): the dialog shows each property with its origin; change Yield strength only, OK | Properties panel ▸ *Material properties…* for the same material, the same change | Both store the same value (re-open the dialog in both); the viewer sends only the changed key (`cad_state` history shows one "Material properties of …" step); a default RoboCAD's `_ENG` table supplies shows "not reported" in the viewer (recorded) |
| CAD-135 Colour | Inspector: type `0.9, 0.2, 0.2` in the colour field, Enter; then **Use material colour** | `curl -X PATCH http://127.0.0.1:<RoboCAD port>/nodes/<id> -d '{"color":[0.9,0.2,0.2]}'` (RoboCAD's window has no colour editor; the route is the reference), then `-d '{"color":null}'` | The body turns the same red in both windows and back to its material's colour; each is one undo step |
| CAD-136 Joint editor | Select a joint; inspector **Edit joint…** (or double-click its row in the Robot panel, CAD-141): change the upper limit and the name, OK | Double-click the joint in the Robot panel ("Edit joint" dialog), the same change | The same limit and name in both (`GET /robot`); the rename is a second call only when the name changed, as RoboCAD's handler |
| CAD-137 Joint physics overrides | Select a joint: the rows "Radial clearance (mm)", "Wobble (°)", "Drive backlash (°; …)", "Coulomb friction (mN·m)", "Viscous (mN·m·s)", "Radial stiffness (N/m)", "Flex patch radius (mm)" and the source line; type a Coulomb friction, Enter | Properties panel on the same joint, the same value | The same values and provenance in both, "*" on the overridden row in both after the edit; a value the physical model lacks is empty in the viewer, 0.0 in RoboCAD (recorded) |
| CAD-138 Results line | After CAD-150 (results loaded), select a link body | The same body in RoboCAD's Properties panel | The same "Results: key value, …" line (same keys, 3 significant figures) in both |
| CAD-139 Exact measurement | Select two bodies, **Calculate exact measurements**; then start it again and change the selection before it finishes; start it again and make an edit | Properties panel ▸ *Calculate exact measurements*, the same selection | The same size, volume, area, mass and centroid in both; the viewer's measurement is cancelled by the selection change and by the edit, each saying why, and the window never stalls; its preview has no "Display size ≈" line (recorded) |
| CAD-140 Robot panel summary and tree | The Robot section: the summary line, the "Links", "Joints", "Motors", "Sensors & cables" headings with their rows, each with its detail line | The Robot dock | The same counts ("n bodies, n joints, n DoF, n motors, n sensors, n cables. Ground: …. Power: ….") and rows in both; the viewer omits "(n s run)" and adds "(stale: …)" when RoboCAD flags the results stale; no branch glyphs, and Detail and Margin are lines under the name (recorded) |
| CAD-141 Robot panel click and double-click | Click a joint row, then a link row; double-click a joint row | The same clicks in RoboCAD's Robot dock | A click selects the node in both (the viewer's tree and 3D view follow it); the double-click opens "Edit joint" preset from that joint in both |
| CAD-142 Validate and issues | Robot ▸ Robot: validate; read the Robot panel's issue list | Robot ▸ Robot: validate | The same verdict: "robot valid: …" in the status line, or the same issues; RoboCAD's warning box is the viewer's status line and issue list, "Error:"/"Warning:" in place of ⛔/⚠ (recorded) |
| CAD-143 Motor library | Robot ▸ Robot: motor library… | Robot ▸ Robot: motor library… | The same motors and specs; the viewer shows a floating panel, RoboCAD a message box; the viewer lists them by id (recorded) |
| CAD-144 Add motor from library | Robot ▸ Robot: add motor from library… (or the Robot panel's **Add motor…**): Motor, Rotation about shaft, Mount on, "Cut mounting holes and pilot into the mounted body", Name; click a body's face; Escape ends the tool and the dialog | Robot ▸ Robot: add motor from library…, the same values, click the same face | The same motor node at the same place (housing outside, shaft into the body) in both; the new motor becomes the selection in both; one undo step. The viewer's fields stay beside the view while you click (recorded) |
| CAD-145 Add joint tool and joint from selection | Robot ▸ Robot: add joint (Ctrl+Shift+J): click the parent (Ctrl-click for the world), the child, an axis face; then select two bodies and Robot ▸ Robot: joint from the two selected bodies… (Type, Parent "(world)", Child, Pivot (mm), Axis, limits, Motor, Extra gear ratio, Damping, Name), OK | Robot ▸ Robot: add joint (the menu entry; Ctrl+Shift+J is not bound in RoboCAD), the same clicks; then the same dialog | The same joints (type, pivot, axis, limits) in both; the new joint becomes the selection; Ctrl+Shift+M still runs Select Same Material in both (recorded) |
| CAD-146 The other robot tools and dialogs | In turn, undoing each: Robot ▸ infer joints from coaxial holes and pins; assign selected motor to a joint… (Motor, Joint, Extra gear ratio); fix selected bodies together; toggle ground on selected bodies; add sensor… (Kind, On body, Point (mm), Reads joint, Rate (Hz), Name); add cable between bodies… (From/To body and point, Length, Mass, Name); battery, control loop and uncertainty… (Battery cells, Chemistry, Capacity (Ah), Control period (s), Control latency (s), Target per joint (°) as `{"joint": deg}`, Dimension σ (mm), Friction σ) | The same Robot menu entries and dialogs with the same values | The same result in `GET /robot`, `/sensors`, `/cables`, `/battery`, `/control`, `/uncertainty` after each; Assign motor without motors or joints is refused before its dialog in both; a joint left out of the targets keeps its target in the viewer (recorded) |
| CAD-147 Export physical model | Simulation ▸ Simulation: export physical model (simrobot v4, with flexible links)…: `/tmp/cad-check/turntable.simrobot.json`, OK; start it again and **Cancel export** while it runs | Simulation ▸ Simulation: export physical model… to `/tmp/cad-check/turntable-rc.simrobot.json` | Both files hold the same model (the same links, masses, joints and motors; simrobot version 4); the status line shows "exporting … n s"; cancellation before publication writes nothing, while a late cancellation retains and reports an already written file; queued exports wait for the terminal outcome; the window never stalls |
| CAD-148 Export robot model (planar) | Simulation ▸ Simulation: export robot model…: `/tmp/cad-check/turntable-planar.simrobot.json` | Simulation ▸ Simulation: export robot model… | Both files carry the same x–z planar hint |
| CAD-149 Stress overlay | Inspect ▸ Toggle stress overlay (or Print ▸ Strength overlay on/off, or the Robot panel's **Stress overlay**) after CAD-150 | Inspect ▸ Toggle stress overlay | The same links are coloured, hot where RoboCAD's are; the viewer uses Robot mode's log scale (blue at 0.1 % of yield → red at yield), RoboCAD a linear one (recorded), so mid-range colours differ; the staleness label matches RoboCAD's stale flag |
| CAD-150 Load results | Robot ▸ Robot: load simulation results…: the form starts at `<stem>.simresult.json` (`/tmp/cad-check/turntable.simresult.json`, written by `sim-cad run` above); OK | Robot ▸ Robot: load simulation results…, the same file | The Robot panel's margins (yield, bearing, screw, stall, Tg, mount Tg) match in both, and the stress overlay turns on in both |
| CAD-151 Apply identification | Robot ▸ Robot: apply identified joint parameters…: a `sim-cad fit` output file if you have one, else a missing path | The same entry and file | The same stored joint parameters (the next export's joint physics) in both, or the same RoboCAD error verbatim |
| CAD-152 Live link into Robot mode | Simulation ▸ Simulation: live link (watch + run viewer) on the saved document: the window switches to Robot mode on `turntable.simrobot.json`; go back to CAD, change a joint limit, Save (Ctrl/Cmd+S), then switch to Robot mode | Simulation ▸ live link in RoboCAD's window: it exports and starts `sim-spatial --robot` | The viewer opens Robot mode in the same window (no new process) on the exported model, and after the save Robot mode shows the new limit; with an export running, leaving CAD is refused naming it; an unsaved document refuses the link ("Save the document first: …") |

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
this part was compiled or run before it was written: **Part H has not
been through its verification pass yet**.

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-153 Wall thickness check | Print ▸ Wall thickness check… (Ctrl+W) with nothing selected: "Flag walls thinner than (mm):" 1.2, OK; then select one body and run it again at `2`; then open the form once more; finally `cad_print {"op":"clear"}` | Print ▸ Wall thickness check… (Ctrl+W), the same thresholds and selection | The same red points (RoboCAD's 1.0, 0.2, 0.2, 9 px) on the same places and the same status ("N thin region(s) under 1.2 mm" or "No walls thinner than 1.2 mm"); the window never stalls while it reads; the third form opens at `2` in the viewer (it remembers the last threshold; RoboCAD reopens at 1.2: recorded); running the same check again on unchanged nodes sends nothing (`cad_state.print.checks.cached_reads`); after any edit the points are no longer drawn; clear removes them |
| CAD-154 Validate for printing | Print ▸ Validate for printing (Ctrl+Shift+V); then hide a body and run it again | Print ▸ Validate for printing | The same verdict: "n body(ies): valid and watertight." (the same n: every visible body), or the same messages ("name: message near (x, y, z) — fix"). The viewer's answer is the status line, not a box, and it has no open-edge lines: RoboCAD's tessellation open-edge check is its desktop's only (`cad_state.print.checks.open_edge_check` says so; recorded) |
| CAD-155 Overhang shading | Print ▸ Toggle overhang shading (or the toolbar's **Overhangs** chip); toggle it off; then View ▸ Build Plate Preview (Ctrl+Shift+B) on and off; then the plate on and overhang shading off | The same entries | The same faces facing down past 45° are tinted 0.9, 0.35, 0.3 in both; the build plate turns overhang shading on with it and off with it in both, and the shading's own toggle flips only the shading; nothing is written to the document (no undo step) |
| CAD-156 Fastener hole | Print ▸ Fastener hole… (Ctrl+H): Size `M4`, Kind `counterbore`, Extra clearance (mm) `0.1`, Depth (mm; 0: through) `0`; click a flat face, then another; Escape; open the tool again | Print ▸ Fastener hole… (Ctrl+H): the same values in the dialog, OK, then click the same faces; Escape; open it again | Each click is one hole and one undo step at the clicked point in both (the viewer's status line names it "M4 counterbore hole in NAME"), through the part (depth 0 is "through"); the viewer's fields stay beside the view while you click (recorded); both reopen with M4, counterbore, 0.1, through (remembered); a click off any face does nothing; the selection is not changed by the clicks |
| CAD-157 Clearance offset | Select two faces of one hole and one face of another body, Print ▸ Clearance offset… (Ctrl+Shift+C): "Grow holes / shrink bosses by (mm):" `0.3`, OK; open it again | The same faces, Print ▸ Clearance offset…, `0.3` | The same faces move by 0.3 mm in both; one RoboCAD undo step "Clearance" per body (two here) in both; the form reopens at 0.3 in both (0.2 the first time); with no face selected both refuse "Select holes, bosses or faces to offset" |
| CAD-158 Split for printing (dovetail) | Select one body, Print ▸ Split selected for printing…: Printer the first entry ("id (x × y × z mm)") and another printer, Joints `dovetail`, OK; watch the status line until it ends | Print ▸ Split selected for printing…, the same printer and joint | The same printer list in the same order with the same usable sizes; while it runs "split: … (n %)"; the same pieces under a new split group and the same done text ("split into N pieces; hardware: …"); with two bodies selected both refuse "Select one body to split." |
| CAD-159 Check strength | Print ▸ Check strength (document's print study); first on a copy with no study, then with the study set above | The same entry, without and with the study | Without a study both explain "This document has no print study yet. …" word for word; with it, both run a job and end "strength: least safety factor F on NAME (MODE); Print ▸ Strength overlay shows where" with the same F and part |
| CAD-160 Plan with job progress | Print ▸ Plan print settings and plates (document's print study); watch the status line | The same entry | While it runs the status line reads "plan: message (n %)" in both (RoboCAD adds " — Print ▸ Print jobs… to cancel"; recorded), changing as it progresses; both end "plan: N plate(s), about H h and G g (estimates); 3MF files in DIR" with the same plates; the window never stalls |
| CAD-161 Whole or split | Select the study's part, Print ▸ Whole or split for strength?; then select a body that is not in the study | The same | The same "RECOMMENDATION: WHY" in both; a body outside the study is refused by both ("Select a part that is in the document's print study …") |
| CAD-162 Assembly guide | Select the split group from CAD-158 (then one of its pieces), Print ▸ Assembly guide for the selected split… | The same | Both run a job, end "assembly: N steps; guide PATH" and open the same guide with the system's opener; selecting a piece finds its split group in both; with nothing split selected both refuse by name |
| CAD-163 Test coupons | With the split group selected, Print ▸ Test coupons…: Printer, Filament from the lists; then with nothing selected | The same, Printer and Filament | The same printer and filament ids in the same order; both end "coupons: N on P plate(s); break them, fill results.json, then `sim-print promote results.json`" and open the protocol's folder; with nothing selected the coupons are for the material only in both |
| CAD-164 Print jobs and cancel | Start a plan (CAD-160), then Print ▸ Print jobs…: the section lists the last eight jobs ("kind id: state n % message"); **Cancel running jobs…** → "Cancel the running jobs?" → **No**; again → **Yes**; **Close** | Start a plan, Print ▸ Print jobs…: the list and "Cancel the running jobs?" → No; again → Yes | The same jobs and lines in both; No leaves the job running in both; Yes cancels it in both (status "plan cancelled"); the viewer asks in an inline row of the section, not a box (recorded), and sends exactly one cancel per running job (`cad_state.print.jobs`); with no job running there is no Cancel button and the list says "No print jobs yet." on a fresh service |
| CAD-165 Print overlay and staleness | After CAD-159, Print ▸ Strength overlay on/off; read the results line in the Results section; then make any edit | Print ▸ Strength overlay on/off | The same parts are coloured, and the part with the least safety factor is the reddest in both; the viewer colours each part in one colour by its governing failure index (1 / safety factor) on Robot mode's stress scale, where RoboCAD colours each voxel's failure index (recorded), so colours inside a part differ; the viewer's line "Print strength: N part(s); least safety factor F on NAME (current)" turns "stale (computed at revision R, now M)" after the edit |
| CAD-166 Leaving CAD mode while a job runs | Start a plan, then switch to Robot mode before it ends; then wait for it to end and switch again | (n/a: RoboCAD has no other modes) | The switch is refused while the job runs, naming it ("a print job is running in RoboCAD: plan (n %); wait for it, or cancel it in the Print jobs section"); once it ends the switch goes through; with the service lost (not connected) nothing holds the switch |
| CAD-167 Robot panel row press | Click a joint row in the Robot section, then the same row through `system_ui` `cad:robot:row:<id>` | Click the same row in RoboCAD's Robot dock | The node is selected in both, as in CAD-141; the viewer's press is stamped with the revision the robot description was read at, which records where the row came from without refusing a row of an older read; a row of a node that has left the tree is refused by name |
| CAD-168 Partial REST Edit joint | With joint `<j>` selected, `cad_run {"id":"ops.set_joint","params":{"lower":-30}}` | `curl -X POST http://127.0.0.1:<RoboCAD port>/ops/set_joint -d '{"args":["<j>"],"kwargs":{…every current value, lower -0.5236…}}'` (RoboCAD's `set_joint` takes every field; the Edit joint dialog fills them) | Only the lower limit changes (−30°), the joint's type, parent, child, pivot, axis, upper limit, motor, gear ratio, damping and name stay as they were in both (`GET /robot`); a type change between prismatic and revolute without both limits is refused by name; with the robot description not yet read at the shown revision it is refused by name and nothing is sent |

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
step, unless the step says otherwise. Nothing in this part was compiled
or run before it was written: **Part I has not been through its
verification pass yet**.

| Step | Native viewer | RoboCAD | Pass when |
|---|---|---|---|
| CAD-169 Search | Type part of a body's name into "Search (Ctrl+F)…" above the tree; clear it; then with the pointer over the tree press Ctrl+F, and with the pointer over the 3D view and a body selected press Ctrl+F | Type the same text into the Outliner's search; Ctrl+F | The same rows in both: every node whose name contains the text (any case), its ancestors and its descendants, all expanded while searching; clearing restores the collapse state from before. Ctrl+F over the tree focuses the search in the viewer; elsewhere it starts Fillet in both (RoboCAD's keymap gives Ctrl+F to Fillet, so its placeholder's key never reaches the field: recorded) |
| CAD-170 Expand and collapse | **Collapse all**, then expand one group by its "+" chip; make an edit (rename a body); **Expand all**; then type a search and press **Collapse all** | The same buttons and arrow | The same rows shown in both after each; the collapsed groups stay collapsed after the edit's refresh in both; while a search is typed the viewer refuses Expand all, Collapse all and the chips by name (RoboCAD changes the searched view without keeping it: recorded) |
| CAD-171 Shift and Ctrl select | Click a body row, Shift+click a row three below, then Ctrl/Cmd+click one of them | The same clicks in the Outliner | The same nodes selected in both (the range, then without the Ctrl-clicked one); the 3D view lights the same bodies; the viewer's selection is pushed to RoboCAD (`GET /selection`) as body items |
| CAD-172 Rename in place | Double-click a body's name, type `Check name`, Enter; then double-click again and press Escape; again, type a name and click elsewhere; again, clear the name and press Enter | Double-click the same name, type, Enter; the same click elsewhere | One undo step renames it in both (the inspector and the tree follow); Escape leaves the name; an unchanged name sends nothing; a click elsewhere renames in RoboCAD (Qt commits on focus loss) and ends without renaming in the viewer, and an empty name is refused by name in the viewer (both recorded) |
| CAD-173 Drag into a group, before a sibling | Drag two selected bodies onto the middle of a group row; undo; drag one body onto a body row in another group; then drag a group onto the top quarter of another group's row; finally drag a group onto one of its own children | The same drags in the Outliner | The same tree in both after the first two: on a group the rows move into it (at its end); on a body they move before it under its parent; each drag is one `move_nodes` undo step. The top quarter of a group row moves the group in front of it in the viewer, where RoboCAD drops into the group (recorded). A group onto its own child is refused in both ("Cannot move a group into itself or its descendants"; the viewer checks before sending) |
| CAD-174 Context menu | Right-click an unselected body row (it becomes the selection), then a selected one: read the entries; **Hide**, **Show**, **Isolate**, **Show all**; **Lock**, then **Unlock**; **Group selection…** (`Organize components` / `Group name:` `Pair`, OK); under **Move to group**, **Top level**, then a group by its path ("A / B"); right-click a group: **Set as active group**; then **Clear active group**; registry `group.set_active` with no group selected | The same entries in RoboCAD's Outliner menu; the registry command "Set selected group as active" with no group selected | The same entries in the same order in both (Fit in view, Isolate, Hide, Show, Lock, Unlock, Group selection…, Move to group, Make unique (bake instance), Set as active group for one group, Delete, Clear active group, Show all); the same visibility, lock state, groups, moves and active group (its row in blue in both) after each, Hide and Show one undo step each; Move to group lists the same group paths in the same order, without the selection and its descendants, as a heading over indented entries in the viewer, a submenu in RoboCAD (recorded); with no group selected `group.set_active` is refused by name in the viewer, where RoboCAD silently clears the active group (recorded) |
| CAD-175 New group | **New group** above the tree: `Group name:` `Empty`, OK; then New group with an empty name | Outliner ▸ **New group**, the same names | An empty group "Empty" appears in the same place in both, one undo step; an empty name creates nothing in both, and the viewer says why in the dialog (RoboCAD's dialog just closes: recorded) |
| CAD-176 Comments section and Annotate | View ▸ Comments panel; **＋ Annotate model** (or N with the pointer over the 3D view): click a face of a body; type `Check this face`, **Post annotation**; then N and click empty space | View ▸ Comments panel, Annotate (N), click the same face, the same text | The same thread "1 · part" in both lists with the same preview and "Attached to surface"; the pin "1" at the clicked point in both; clicking empty space says "Click a visible surface to place the annotation" in both; with the composer focused, N is typed, not a new annotation. The viewer picks on the press and leaves the selection mode as it is (RoboCAD picks on the release and shows face mode while the tool is on: recorded) |
| CAD-177 Reply with a part link | Select the thread; select another body in the tree; **Insert part link from selection**; type ` needs a fillet`; Shift+Enter, a second line; **Reply** (then, for a second reply, Enter) | The same in RoboCAD's Comments dock (Enter types a newline there; Reply posts) | The same reply in both, the link shown as the part's label; with nothing selected both say "Select a part in the outliner or viewport first"; in the viewer Enter posts and Shift+Enter is a newline (recorded) |
| CAD-178 Click the part link | Click the link in the reply (use a link to a group) | Click the same link | Both show only that part and its descendants, framed, with "Part view: name" in the status line; the viewer selects exactly the linked node (as `cad_select`), RoboCAD the node with its descendants (recorded) |
| CAD-179 Show on model | **Return to assembly**, then **Show on model** | **Show on model** | The anchor body is selected and the camera returns to the view saved with the thread in both; the viewer restores it from the thread list, not through RoboCAD's GUI-only `/threads/{id}/show` (recorded); on a thread of experiment evidence the viewer refuses by name (RoboCAD opens its experiments panel: cad-experiments-motion) |
| CAD-180 Fit in view | **Fit in view** | **Fit in view** | Both frame the thread's linked parts at the current angle, select them and show the pins, "Fit annotation in view: names" in both |
| CAD-181 Show only linked parts and Return | **Link selected parts** with two bodies selected; **Show only linked parts**; **Return to assembly**; again Show only linked parts, then Escape | The same | Only the linked parts are drawn in both, the tool hint "Showing linked parts only · Esc or Return to assembly restores your view"; Return and Escape restore the camera and the selection from before in both; no node's visibility changes in either (`GET /doc`), and RoboCAD's window is not isolated by the viewer's (recorded) |
| CAD-182 Resolve and Reopen | **Resolve**; filter **Resolved**, then **Open**, then **All**; **Reopen** | The same | The thread moves between the filters the same way in both, "✓" when resolved; its pin is hidden while resolved in both |
| CAD-183 Edit and delete a message | Select the reply, **Edit message**, change the text, **Save edit**; then **Delete message** | The same | The same text, then the message gone, in both; each one RoboCAD undo step |
| CAD-184 Delete thread | **Delete thread** | **Delete thread** | The thread and its pin are gone in both; undo brings both back |
| CAD-185 Pins | View ▸ Toggle comment pins off and on; click pin "1" | The same | The pins hide and show in both; a click on a pin opens its thread in the Comments section in both (amber for "needs review", blue otherwise) |
| CAD-186 Reattach | Delete the anchored body (then undo after the step), so the thread says "Part deleted — reattach this annotation"; **Reattach…**, click another face | The same | The same attachment texts in both; after the click the thread is attached to the new face in both (one `PATCH`, one undo step) and its pin moves; the viewer picks on the press (recorded) |
| CAD-187 Add reference images | View ▸ References; **＋ Add reference images…**: the absolute path of `<img>` in the path field, Enter; then drop `<img>` on the 3D view; then drop a `.step` file, and type a relative path in the path field | References ▸ **＋ Add reference images…**, the same file in the file dialog; then drop it on the viewport; then drop the `.step` file | One locked image node per file in both, on the active plane (else XY), 100 mm wide, the view aligned on it; the image textured on its plane at 60 % opacity in both; "n reference image(s) added • Calibrate scale before tracing"; the viewer takes one typed image per submit, not the system's file dialog (recorded); the `.step` drop and the relative path are refused by name in the viewer with nothing sent, where RoboCAD's import fails in Pillow (recorded) |
| CAD-188 Visibility | Uncheck the image in the References list (its chip), then check it; read the list | The same | The image hides and shows in both, one undo step each; RoboCAD shows a preview thumbnail under the list, the viewer none (the image is drawn on its plane; recorded); a WebP or BMP image is listed in the viewer with a note that it is not drawn (recorded) |
| CAD-189 Placement | Plane `Front (XZ)`, Width `200`, Origin `10, 0, 5`, Rotation `15`, Opacity `40`, **Apply placement** | The same fields, **Apply placement** | The same plane, size, rotation and opacity in both; "Reference placement updated • Ctrl+Z undoes"; one undo step |
| CAD-190 Align view | **Align view**; then make a construction plane from a tilted face, make it the active plane, add `<img>` (it lands on that plane), delete the plane node, and Align view again, then **Sketch over this** | **Align view**, the same, **Sketch over this** | Both look straight at the image, orthographic, centred, the image's plane the active plane; on the tilted plane (no longer a plane node, nor XY, XZ or YZ) the viewer aligns the camera but leaves the active plane and says so, and refuses Sketch over this by name, where RoboCAD sketches on the image's plane (recorded) |
| CAD-191 Calibrate scale | **Calibrate scale**: click two points on the ruler, type the real distance (e.g. `100`) in the References section's "Real distance" field, Enter; again, clicking the same point twice | The same clicks and distance (RoboCAD's numeric bar) | The same new width in both ("Reference calibrated • Ctrl+Z undoes"); Escape before Enter changes nothing; the same point twice is refused with RoboCAD's text; the viewer's distance field is in the References section, not the numeric bar (recorded) |
| CAD-192 Sketch over this | **Sketch over this**, draw a line over the image | The same | The view is aligned and the line tool starts on the image's plane in both |
| CAD-193 Remove reference | **Remove reference** | **Remove reference** | The image node is gone in both; undo restores it |
| CAD-194 System status line | Read the line at the top of the References section with no system linked | The same line in RoboCAD's References dock | "System file: none linked. Link a .system.json to build circuits and subsystems for this model." in both |
| CAD-195 Link, Accept, Unlink | **Link system file…**: `<system>` in the path form, OK; edit `<system>` on disk (e.g. change its title); **Accept changes**; **Unlink** | **Link system file…** (file dialog), the same edit, **Accept changes**, **Unlink** | The same status line in both after each: "System: title · revision n · n definitions", "· CHANGED since linked (was revision r)" after the edit, back without it after Accept, "none linked" after Unlink; each one RoboCAD undo step |
| CAD-196 Open in builder | Link `<system>` again; **Open in builder**; then unlink and press it again | **Open in builder** | The viewer switches this window to Build mode on `<system>` (no new process; RoboCAD starts a second `sim-spatial --system … --schematic`: recorded); leaving CAD follows the usual rule (refused over unsaved edits of a self-started service); with no system linked both refuse "Link an existing system file first" |

## Part J: reusable components and composition (cad-components, T42)

Written acceptance sequence, **not executed or signed off**. Use the native
CAD window on an editable copy of a real `.rcad`; Python/OCCT remains the kernel.
These checks do not substitute for the parity harness.

| Step | Native workflow | Reference/observable acceptance |
|---|---|---|
| CAD-197 Library and find | Components; find a definition | Names, definition revision and occurrence count match GET /components and RoboCAD |
| CAD-198 Capture/parametric | Select bodies; Make from selection; separately New parametric box/cylinder | Matching identity relationships and recipe defaults; preserved IDs across native/service serialization (independent captures may generate different fresh IDs); one ComponentChange undo; headless service needs no Qt |
| CAD-199 Place twice | Place the captured definition twice with name, origin, Z angle, variant and typed bindings | Distinct occurrence IDs; definition identity shared; binding kinds match source ports |
| CAD-200 Defaults/nesting | Edit defaults including units, bounds, provenance, recipes, nested maps and family variants | Rebuild updates inherited occurrences; local branch overrides win; rejected draft remains resumable |
| CAD-201 Overrides/reset | Select a linked part/nested occurrence; edit override and origin; disable/reset override | Correct branch node_map target; nested movement refused; reset inherits; native expression text labelled unevaluated |
| CAD-202 Detach/transform | Detach outer occurrence; rigidly transform top-level occurrence | Undo restores links; member transform restrictions match source |
| CAD-203 Progress/cancel | Start rebuild then Cancel, including while POST is pending | Durable cancellation; queued ready cannot commit after accepted cancel; already-applied commit is reported truthfully |
| CAD-204 Stale/network | Mutate source externally during preparation; interrupt start response | Revision refusal preserves edit; status-list recovery adopts only captured operation/document/revision; POST never resent |
| CAD-205 Graph forms | System composition; choose type, body, parameters and geometry rule | Authoritative metadata/units/input bounds; derived output cannot also be explicit; layout never edits physical geometry |
| CAD-206 Imported bindings | Read an existing completed check ID; bind existing imported component | Imported port names and type preserved; graph-only edits may stale results while structural metadata remains current; physical edits refuse stale metadata |
| CAD-207 Typed nets | Connect/open/remove ports; add third physical terminal; focus/overview/zoom | One shared net with all terminals and preserved ID; Rust and source reject incompatible schemas; presentation is display-only |
| CAD-208 Save/library | Save document; choose folder; export/import .rcomp | Preserved source identities/provenance; cancelled/stale export leaves existing destination intact |
| CAD-209 Draft/mode | Close/resume rejected draft; switch/reopen while rebuild active | Draft retained; active work blocks document replacement/mode exit until terminal; stale retained draft refused |

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
  evidence is refused (cad-experiments-motion); reference images and the
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
| CAD-210 Explicit family target | Select library family; open LinkFamily with a distinct occurrence ID | Definition comes from library selection, ID names occurrence; same typed operation as no-ID shared-selection path |
| CAD-211 Stale first pick | Pick a port; edit source/reload same-revision document; pick another or Leave open | First generation/document/revision stamp refused; pending intent and diagnostic retained |
| CAD-212 Open/cancel/remove | Pick unused physical or signal-output port; Leave port open; separately Cancel connection or Remove connection | Leave creates singleton ID and undo; connected port refuses; Cancel performs no source edit; Remove deletes whole net |
| CAD-213 Build failure | Present invalid shared graph twice; then change source key | Path-named diagnostic retained, unchanged failed key schedules no retry; prior layout labelled stale; changed key retries |

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
