# CAD mode: side-by-side checklist against RoboCAD

This checklist closes the first CAD epic (**cad-mode**, §9 phase 1 of
[docs/architecture/native-viewer.md](architecture/native-viewer.md), section
"CAD mode (2026-09-30)") and the second (**cad-select-transform**: sub-body
selection and the direct tools, Part C). Agents built them and checked them
only by reading: **nothing here has been compiled or run yet** (the
verification pass builds and tests it). Each step is done once in the native viewer and once in
RoboCAD's own window, so you can compare them. The feature-by-feature ledger
is [cad-parity.md](cad-parity.md); its `done-by-reading` rows are the ones
these steps show. Everything else in RoboCAD (sketching, the dialog-driven
modify tools, printing, experiments, motion, comments, …) is a later CAD epic and stays in
RoboCAD's window.

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
  ring turns about X (RoboCAD turns about Z); sketch endpoints and the active
  plane are not snap targets yet (cad-sketch); the viewport footer (orbit
  hints, ms/frame) is not drawn.

## Sign-off

When every step passes, record it in the coordination journal; the ledger's
`done-by-reading` rows then become `done`.
