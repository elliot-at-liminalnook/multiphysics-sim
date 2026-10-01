# CAD mode: side-by-side checklist against RoboCAD

This checklist closes the first CAD epic (**cad-mode**, §9 phase 1 of
[docs/architecture/native-viewer.md](architecture/native-viewer.md), section
"CAD mode (2026-09-30)"), the second (**cad-select-transform**: sub-body
selection and the direct tools, Part C), the third (**cad-modify**: the
operation catalogue and the command surfaces, Part D, section "CAD modify
(2026-10-01)") and the fourth (**cad-sketch**: the active plane, the plane
tools, the sketch tools and the solids made from sketches, Part E, section
"CAD sketch (2026-10-01)"). cad-mode, cad-select-transform and cad-modify
were built and tested in their verification passes; cad-sketch was written
and checked only by reading: **Part E has not been compiled or run yet**
(its verification pass builds and tests it). Each step is done once in the native viewer and once in
RoboCAD's own window, so you can compare them. The feature-by-feature ledger
is [cad-parity.md](cad-parity.md); its `done-by-reading` rows are the ones
these steps show. Everything else in RoboCAD (views and export,
physical inspection, printing, experiments, motion, comments, …) is a later
CAD epic and stays in RoboCAD's window.

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
| CAD-11 Save | **Save** or Cmd/Ctrl+S (`cad_save`; `{"path":"…"}` saves as). The top bar returns to "Saved" | File ▸ Save | The file on disk changed (RoboCAD wrote it); reopening it in RoboCAD shows the edits |
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

## Known differences (deliberate)

- RoboCAD asks Save/Discard/Cancel when closing; the viewer never saves for
  you. It refuses to leave CAD mode while a service it started has unsaved
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

## Sign-off

When every step passes, record it in the coordination journal; the ledger's
`done-by-reading` rows then become `done`.
