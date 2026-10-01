# robocad user guide

Launch: `cad/run.sh` (macOS/Linux) or `cad\run.ps1` (Windows). Open a
`.rcad` by passing its path; other files on the command line are imported.

## Opening large models

File → Open shows a nonmodal loading window. **Reading CAD** counts document
items; **Preparing display** counts parts whose display geometry is ready.
Each stage has its own progress bar, current item name, and elapsed time.
The percentages describe completed items, not an estimated fraction of time:
a complex part can take longer than a simple one. A separate process prepares
each new model, so existing documents stay available while loading. **Cancel**,
Escape, or closing the loading window stops preparation. Once preparation is
complete, cancellation is disabled briefly while the new editor window opens.
That window then runs independently and keeps its own unsaved edits even if
the original editor is closed.

Unchanged models reuse cached display meshes, edges and picking data on later
opens. Exact CAD geometry still comes from the source archive. Cache entries
are keyed by the full archive contents and renderer/kernel version; editing
and saving a model invalidates them. A missing or unreadable cache is rebuilt.
On macOS the disposable cache is in `~/Library/Caches/robocad/display`.
On other platforms it is under `$XDG_CACHE_HOME/robocad/display`, or
`~/.cache/robocad/display` when that variable is unset.

The REST `POST /open` uses this same loader and immediately returns a `load_id`.
Poll `GET /loads/{load_id}` for `loading`, `ready`, `failed`, or `cancelled`;
`DELETE /loads/{load_id}` requests cancellation (`cancelling` until it finishes).
A ready job's `stats.api_url` identifies the newly opened window's API.
Opening or cancelling never saves or replaces an existing document.

## Finding and organizing components

Right-click a part in the outliner and choose **Fit in view** to center
and zoom the viewport on it. Select several parts to frame them together,
or fit a group to include its nested geometry. The viewport's focus command
also works on selected groups.

Use **New group** or right-click **Group selection…** to create named
folders. Drag items into a group, or use **Move to group** (including
**Top level**) to reorganize them. Grouping and moving support undo/redo;
the hierarchy is saved in the `.rcad` file. These folders organize the
outliner independently of the joints that define physical connections.

**Expand all** and **Collapse all** help navigate larger assemblies.
Collapsed folders stay collapsed through edits and selection changes.
Search finds nested parts and opens their ancestor folders temporarily;
clearing the search restores your collapse state. Double-click a name to
rename it.

## Comments on the model

Choose **Annotate** in the toolbar (or press **N**), click a surface, then
write and post your comment in the **Comments** panel. Numbered pins open
their discussions. You can reply, edit or delete messages, resolve/reopen
threads, filter by selected parts, and use **Show on model** to recall the
saved view. **View → Comments** opens the panel.

Select a discussion in the Comments sidebar and click **Fit in view** to
center and zoom to its attached part at your current viewing angle.
**Show on model** recalls the view saved with the annotation instead.

Comments are stored inside the `.rcad` document: save with Ctrl+S. Comment
changes share the model's undo/redo history. Moving, rotating, or scaling a
solid moves its pins too. Other geometry changes mark pins for review;
**Reattach** lets you choose the intended surface. Deleting a part retains
its discussion and marks the attachment as missing.

The viewport shows the active tool, selection mode, and next action.
Crosshair cursors indicate placement tools; navigation uses a closed hand,
and comment pins use a pointing hand. Model hotkeys pause while you type
in a text or numeric field. Hover picking is coalesced so it cannot replace
a deliberate click.

The same annotations are available through the local REST API:

| Method | Route | Action |
| --- | --- | --- |
| GET / POST | `/threads` | List / create threads |
| GET / PATCH / DELETE | `/threads/{id}` | Read / update / delete a thread |
| POST | `/threads/{id}/comments` | Reply |
| GET / PATCH / DELETE | `/comments/{id}` | Read / edit / delete a message |

List filters are `node_id` and `status` (`open` or `resolved`). Create a
thread with `node_id`, `point` (three world coordinates in millimetres),
`body`, and optional `author`, `face` index, and `view`. Patch a thread with
`status`, or `node_id` and `point` to reattach it; patch a message with
`body`. Authors are display labels. Delete the thread to remove its final
message. Responses report attachment state as `attached`, `needs_review`,
or `missing`.

```python
from robocad.client import RoboClient

cad = RoboClient()
thread = cad.create_thread(part_id, [10, 5, 5], "Check clearance here")
cad.reply(thread["id"], "Try a 0.2 mm allowance")
```

The Python client's default author is `Codex`; pass `author` to override it.

## Reference images and tracing

Open **References** from the toolbar, then **Add reference images…**, or
drop several local image files onto the panel or viewport. Images are
embedded in the `.rcad` file and start locked. The list checkboxes toggle
visibility; **Remove reference** and all placement changes support undo.

Choose front, side, top, or the active construction plane. Set the width,
origin, rotation and opacity, then **Apply placement**. **Align view** looks
squarely at the image in orthographic projection. **Calibrate scale** lets
you pick two landmarks and enter their known distance; the first landmark
stays fixed while the image scales. **Sketch over this** aligns the camera,
sets the image's plane as the sketch plane and starts the line tool. Other
sketch tools use that same plane.

Calibration is reliable for flat drawings and square-on views. A single
scale does not correct perspective distortion in a photograph. This first
reference workspace supports still images; video scrubbing is not yet
implemented.

## Preview joint motion

Open **Pose** in the toolbar and choose **Enter pose mode**. Select a joint,
drag the slider or type its position (degrees or millimetres). Its range
appears on the model. Positions of the other joints are retained, so you
can pose a complete chain. **Play sweep** moves the selected joint back
and forth through its range; set the cycle time beside the button.

Fixed attachments, mounted motors, sensor glyphs, cable endpoints and
comment pins follow their connected parts. Parts without a joint or motor
mount connection remain stationary. **Return to CAD pose** or **Esc**
restores the original display. Choosing an editing tool or changing the
document also ends preview. Preview does not change solids, save a pose,
or add slider movements to undo history.

This is kinematic preview: it does not evaluate loads, collisions or motor
performance. Fixed, revolute, continuous and prismatic joint trees are
supported; ball joints and closed linkages report that a constraint solver
is needed. The panel labels fallback preview bounds when joint limits are
unset. Saved poses, video synchronization and collision highlighting are
future additions.

## The Tab-to-type workflow

Every creation and transform tool shows a live readout in the status bar
while you drag, and a numeric bar at the bottom with one field per
dimension. Press **Tab** at any moment to jump into the first field, type
an exact value — `20`, `20mm + 0.3`, `1in`, `50/2`, `pi*10`, `45deg` —
**Tab** cycles fields, **Enter** commits, **Esc** cancels. Bare numbers are
millimetres (degrees in angle fields). A red border means the expression
does not parse yet.

Examples: `Shift+A, B` starts a box; drag its base on the active plane,
drag the height, or press Tab and type `40, 30, 12`. `D` starts push/pull;
drag a face or Tab and type `-2` to sink it 2 mm. `G/R/S` move, rotate and
scale with the gizmo; hold **Ctrl** for grid/15° snapping, drag the centre
handle for screen-space movement, Tab for an exact amount.

## Live dimensions

Select a face (or double-click one in the select tool): its dimension
appears in the numeric bar and the Selection panel — a cylinder's
diameter, the distance between two parallel selected faces, or the angle
between two selected faces. Edit the number; the body
updates in place. With Measure (`M`), click two things: the value is
copied to the clipboard; Shift+click keeps it as an annotation that stays
attached to the geometry.

## Selection

`B/Shift+B/E/V/P` switch between bodies, faces, edges, vertices and points;
`Q` opens the selection radial menu. Shift adds, Ctrl toggles, drag box-
selects, Ctrl+A / Ctrl+Shift+I select all / invert, Ctrl+Shift+M selects
everything with the same material, "Selection: edges → bounding faces"
converts. Alt+click on overlapping picks opens a disambiguation menu.

## Sketching and construction planes

Sketch tools (`L` line, `Shift+L` rectangle, `C` circle, `A` arc, `Shift+P`
polygon — it remembers the last vertex count — `Shift+S` slot, `Shift+C`
spline, `T` text, ellipse, spiral) draw on the **active plane**: XY by
default, or one you set from a face (`Ctrl+P`), three points, two points
facing the camera, or as a midplane between two faces. "Toggle 2D
snapping" projects every pick onto the active plane. Snapping finds
endpoints, midpoints, circle centres, grid points and mesh vertices; hold
**Alt** to suppress it. Trim, split, extend, corner fillet, offset, join
and rebuild live in the Sketch menu. `X` extrudes the selected sketch
(hold Shift/Ctrl/Alt while releasing to subtract/union/intersect with the
body), `Shift+R` revolves; sweep, pipe, loft and fill take their curves
from the selection.

## Direct editing

Push/pull (`D`), offset face (`Shift+D`), move and rotate faces with the
gizmo, draft, delete face (the kernel heals the gap), imprint, split face,
booleans (`Ctrl+U` union, `Ctrl+Shift+U` subtract, `Ctrl+Alt+U` intersect,
region), cut with the active plane or a selected sheet/curve, shell
(`Ctrl+Shift+H`: pick the faces to open, type the wall), thicken, fillets
(`Ctrl+F` constant, variable, chordal, full round, all edges, remove
fillets), chamfer (`Ctrl+Shift+F`, distance–distance or distance–angle),
mirror (`Ctrl+M`, or as a live instance), instances, rectangular/radial
arrays (`Ctrl+Shift+A`, count + spacing or count + extent, live instances
or merged), join/unjoin/dissolve. Order matters for offsets: hollow first,
fillet the outside after — a 2 mm wall cannot be offset inside a 1 mm
fillet, and the tool tells you.

## Print helpers

*Fastener hole* (`Ctrl+H`): pick a size (M2–M8) and kind (clearance, tap,
counterbore, countersink, heat-set insert), then click a face; it remembers
the last settings. *Clearance* (`Ctrl+Shift+C`) grows selected holes and
shrinks bosses by a value (0.2 mm default, remembered). *Wall thickness
check* (`Ctrl+W`) marks regions thinner than a threshold. *Validate*
(`Ctrl+Shift+V`) reports invalid or open bodies with what to do; every mesh
export runs it first. *Build plate preview* (`Ctrl+Shift+B`) shows a
220 × 220 plate and shades overhangs over 45° red. Materials carry density;
the Selection panel shows approximate display bounds immediately. Use
**Calculate exact measurements** for volume, area and mass. This runs in
a separate process so you can keep navigating; changing the selection or
editing the document cancels obsolete results. Whole-body selection does
not inspect every face for inferred dimensions; select faces to edit them.

## Section analysis and inspection

`Ctrl+Shift+X` opens the section tool: drag the plane along its normal
(Tab for an exact offset, `R` rotates it) and the interior shows live with
the outline drawn; it works on solids, sheets and reference meshes.
Interactive outlines use cached display triangles, so their accuracy follows
the display tessellation. Exact B-rep sections remain available through the
explicit section analysis API; they are not computed on viewport redraws.
Curvature combs, continuity checks (G0/G1/G2 coloured edges), draft-angle
and normal shading are in Inspect.

## Viewport

Annotation comments support clickable part links: `[screw-shaped worm](part:PART_ID)`.
Use **Insert part link from selection** while writing a comment, or **Link
selected parts** to attach the current selection to an existing discussion.
The parts list supports plain-language labels and shows the actual CAD names.
Click a list entry to highlight it; double-click it or click an inline link to
inspect it alone. **Show only linked parts** shows the whole referenced mechanism.
**Return to assembly** or **Esc** restores the previous camera, cutaway and selection.
This temporary inspection never changes document visibility or geometry.

Thread REST requests accept `part_refs` entries with `node_id`, optional `label`,
`description`, and `view` (a Saved Views state), plus an optional `inspection_view`
for the discussion's contextual view. GET returns `linked_parts` with current
names and availability. `POST /threads/{id}/show` accepts `mode` (`context`,
`parts`, `highlight`, `back`) and optional `node_id` for a particular part.

**Saved Views** in the toolbar or View menu opens a panel for labeled views.
Position the model, type a name, and choose **Save current view**. Double-click
a saved view (or choose **Restore view**) to return to it. **Rename**, **Replace
with current**, and **Delete** edit the collection and support Undo/Redo.
Views are stored inside the `.rcad` file, including camera orientation, zoom,
orthographic/perspective projection, section plane, display mode, grid and
comment-pin visibility. Restoring a view leaves geometry and selection intact.

REST clients use `GET/POST /views`, `GET/PATCH/DELETE /views/{id}`, and
`POST /views/{id}/restore`. To bookmark the live desktop camera, POST
`{"name":"Worm drive — cutaway"}` to `/views`. Headless clients supply a
`state` object as well; GET a saved view to see its complete state schema.

Right-drag orbits (a two-finger drag on a trackpad; turntable by default,
trackball in View), Shift+right-drag or middle-drag pans, the arrow keys
orbit by 10° (Ctrl: 90°, Shift: pan), wheel zooms toward the cursor, `F` focuses the selection,
`Home` fits all, `1/3/7/0` front/right/top/iso, `5` orthographic, hold Alt
while orbiting with the right button to snap to an axis view, and the
view cube in the corner is clickable (click the same face again for the
opposite). `Z` cycles shaded, shaded with edges, wireframe, X-ray, matcap
and render (three lights, ground shadow). Space opens the view radial
menu. Materials drag from their panel onto bodies. A 3Dconnexion SpaceMouse
works when `pyspacemouse` is installed; buttons map in
`~/.robocad/spacemouse.json`.

## Import and export

Import STEP (names, colours, assemblies as groups), IGES, STL/OBJ/3MF/PLY/
glTF meshes (you are asked for their unit), SVG curves onto the active
plane, and PNG/JPG reference images (then click two points and type their
distance to calibrate). Export STEP (schema AP203/214/242), IGES, STL
(binary/ASCII, unit, chord tolerance), 3MF (multi-body, colours, names),
OBJ (+MTL, scale, up axis, quads/n-gons), a sketch as SVG, and a
technical drawing (Ctrl+Shift+D: front/top/right/iso plus a section view
when the section tool is on, hidden lines dashed, hatching). Dialogs
remember their last values.

## Live link and web share

*Bridge → Live link: start* serves the model on `ws://127.0.0.1:8765`;
install `blender_addon/robocad_link.py` in Blender and connect from the
robocad sidebar tab, choosing the tessellation tolerance there. Groups
become collections, sharp edges and seams are marked, and materials
assigned per face survive refreshes. *Web share* writes a single HTML
viewer with the mesh embedded.

## The simulation loop

Name a body `ground` to fix it, and add construction planes named
`joint:<child>` (or `joint:<child>:<parent>`) whose origin is the pivot and
whose normal is the revolute axis. *Simulation → export* writes
`<name>.simrobot.json`; *Simulation → live link* (Simulate) keeps it in
sync with every Ctrl+S and opens the native viewer's robot mode,
`sim-spatial --robot <file>` (`simbridge.viewer_command` picks the release
build, else the debug build). The viewer watches the file and reloads the
model in place on every export, so it is started once and never relaunched
per save. In the window: the header's Run / Pause / Step / Reset run the
model on a worker thread; − / ×scale / + (keys `-` and `=`/`+`) set the run
speed from ×0.125 to ×8, and the header reports the achieved real-time
factor, adding "(compute-limited)" when the machine cannot keep up; keys
C, J, F and H (or the overlay buttons) toggle contacts, joint frames,
flexible-link deflections and the stress colouring from
`<name>.simresult.json`; G toggles the graphs; the inspector's jog buttons
move a joint's servo target. The mouse orbits, pans and zooms. The same
controls are available over the viewer's REST API (`GET /v1/capabilities`
on port 8421).

If sim-spatial is not built, the live link launches nothing and the
status line names the build command (`cargo build --release -p
sim-spatial`); there is no fallback viewer (sim-app was retired on
2026-09-30).

**Planar v2** files (`export_sim_model(..., version=2)`) open in the same
robot mode, run through sim-phenomena's shared planar build and labelled
"planar v2 summary · uncalibrated · not the v3 physical model": it draws
the bodies' section outlines in their simulated poses, holds every joint
with a PD servo, and the keys are Space run/pause, R reset (rebuild from
the loaded file), ←/→ select a joint, ↑/↓ move its target 0.01 rad per
frame while held (Shift 0.05), C chain-tip contacts and `=`/`-` speed.
Stress, gait preview, recordings, graphs, the joint-frame and deflection
overlays and the leg mirror need a v3 export and are refused by name.
`sim-cad model.simrobot.json 2` runs a model headless and prints the
trajectory.

## The REST API (working alongside a script or an agent)

Every window serves a local REST API (ports 8420, 8421, … per window;
*Bridge → REST API: show address*). Requests run on the GUI thread between
frames, so a script and the person at the keyboard edit the same document,
share the undo history, and see each other's changes immediately.

    GET  /doc                      tree, materials, selection, view, history
    POST /nodes                    {"kind":"box","corner":[0,0,0],"size":[20,10,5],"material":"petg"}
    PATCH /nodes/{id}              {"name":"Lid","visible":false,"material":"steel"}
    GET  /nodes/{id}/faces         face references to pass to ops as {"node": id, "face": i}
    POST /ops/push_pull            {"args": [id, {"node": id, "face": 3}, 5]}     any Ops method
    GET  /nodes/{id}/solids        solid indices, conservative bounds (mm), document revision
    POST /ops/extract_components  {"args": [id, {"Crank": [1], "Rigid rod": [2, 3]}, revision]}
    POST /nodes/{id}/sketch        {"calls": [["rectangle", [[0,0],[20,10]]], ["circle", [[10,5],2]]]}
    POST /undo | /redo             PUT /selection  PUT /view {"preset":"front","display_mode":"xray"}
    GET  /render?view=iso&mode=xray&section=y:30&labels=1&focus=id    a PNG from any direction
    GET  /screenshot               the live viewport
    POST /autosave                 start a background recovery save (desktop)
    GET  /autosave                 running, captured revision, saved revision and path
    POST /capture                  temporary camera/section PNG; restores the live view

`python -m robocad.client render out.png --view top --mode wireframe` and
`python -m robocad.api model.rcad --port 8420` (headless, no GUI) use the
same routes; `robocad.client.RoboClient` wraps them for scripts.

For repeatable desktop inspection, POST `/capture` with, for example,
`{"view":{"target":[130,0,0],"distance":440,"yaw":90,"pitch":0,"orthographic":true,"section":{"enabled":true,"plane":{"origin":[0,0,0],"normal":[0,1,0],"x_axis":[1,0,0]}}}}`.
It returns a PNG at the current viewport size, then restores the camera,
grid and section state even if capture fails. Optional `focus_ids` frames
parts or groups before applying the explicit camera fields. The section
removes the positive-normal side of the plane. Camera captures use display
meshes; they do not perform exact section analysis or alter the document.


## Robot parts: motors, joints, sensors, cables

- **Motors** (`Ctrl+Shift+M`, Robot panel → *Add motor…*): pick a library
  entry (SG90/MG90S/MG996R/DS3218 servos, N20/GA25/GB37 gearmotors, gimbal
  and ODrive-class BLDCs, a cycloidal leg actuator, NEMA steppers, an L12
  linear actuator), then click a face. The housing lands outside the face,
  the shaft points into the body, and the body gets the mount holes and
  pilot cut when *Cut mounting holes* is on. A motor rides with the body it
  is mounted on and drives the joint you assign it to (*Assign motor…*).
- **Joints** (`Ctrl+Shift+J`): click the parent body (Ctrl-click for the
  world), the child, then a cylindrical face (its axis) or a flat face (its
  normal); the dialog takes type (revolute, continuous, prismatic, fixed,
  ball, and the loop-closing `loop_revolute` / `loop_spherical` for four-bars
  and parallel grippers), limits, motor and gear ratio. *Infer joints* finds
  every pin-in-hole pair between bodies and declares it. A body named
  `ground` or toggled with *Toggle ground* is fixed to the world.
- **Sensors** (*Add sensor…*): IMU, encoder (reads a joint), current sense,
  force/load cell, on a body at a point with a sample rate. **Cables**
  (*Add cable…*): a wire loom between two bodies with length, mass and
  stiffness; it pulls on the joints in the simulation.
- **Battery / control…**: series cells and chemistry (voltage sag under
  load is simulated), the control loop period and latency, joint targets,
  and the Monte Carlo uncertainty (print tolerance, friction spread).

## The physical model (simrobot v4)

*Export sim…* and the live link write a **physical assembly description**
with geometric derivations, authored estimates and measurements:

- **Links**: bodies merged with their mounted motors and fixed children;
  mass, centre of mass and the full inertia tensor from the B-rep; a
  decimated collision mesh, convex hull and signed distance grid, so
  contact happens between the real shapes (feet, jaws, link-on-link and
  end stops need no hand-placed contact points).
- **Joints as the printer made them**: from the coaxial pin/hole pair the
  export reads pin and hole radius, contact length, radial clearance, the
  bearing wobble estimate, friction from the material pair
  under the outboard weight (Coulomb, Stribeck, viscous), the compliance of
  the printed wall around the hole, and the bearing pressure against the
  material's allowable. Screws recorded by the fastener tool that pass
  through both bodies of a fixed joint become a bolted flange with preload,
  stiffness and shear capacity. Select a joint to see the inferred numbers
  in the Properties panel and override any of them (overridden fields are
  marked `*`).
- **Drive backlash** is separate from radial bearing clearance. The joint
  inspector shows it as **Unmeasured** until you enter a value; editing the
  field declares an estimate in degrees, and clearing it restores unknown.
  REST `set_joint_physics` accepts a `drive_backlash` record with `width_rad`,
  `provenance`, `reference`, and optional `uncertainty_rad`. This is additional
  lost rotation at the connection, beyond the motor's gearbox backlash. New
  motor simulations require an explicit value; entering zero is an idealization,
  not a hardware measurement. Older exported scenes preserve their old behavior.
- **Flexible links**: each printed link is meshed with voxel finite
  elements (orthotropic across layers by the material's print anisotropy,
  infill homogenised), clamped at its parent joint and reduced to its
  lowest modes; the export carries frequencies, how every other joint and
  attachment moves per mode, and the stress tensor per mode so the
  simulator can paint stress and report a yield margin. A 100×10×10 mm PLA
  cantilever reproduces the Euler–Bernoulli first bending frequency within
  2 %. *Export physical model…* includes this; the live link skips it for
  speed.
- **Motors as electromechanics**: winding resistance and inductance,
  torque and back-EMF constants (from the datasheet where known, otherwise
  derived from stall torque, no-load speed and voltage and said so in
  `notes`), gearbox ratio/efficiency/backlash/stiffness, winding and case
  thermal masses and resistances, firmware (servo position loop with
  deadband and potentiometer resolution, position/velocity/torque loops,
  stepper) and driver limits.
- **Materials**: the Materials panel's entries carry Young's modulus,
  Poisson ratio, yield and ultimate strength, glass transition, thermal
  conductivity, specific heat, expansion, friction against themselves,
  steel and the floor, allowable bearing pressure and print anisotropy
  (Properties → *Material properties…* edits them).

The file is `<name>.simrobot.json`, SI units, described key by key in
`PHYSICAL_MODEL.md`. `sim-cad run model.simrobot.json` simulates it, and
`sim-spatial --robot model.simrobot.json` shows it live (the Simulate
viewer; there is no fallback when sim-spatial is not built).

## Results back in CAD

`sim-cad` writes `<name>.simresult.json` beside the model. *Load results…*
(Robot panel) reads it: the stress hotspot paints the links (toggle with
*Stress overlay*: blue at zero, red at the material's yield), the Robot
panel's *Margin* column lists yield, bearing, screw shear, stall and
glass-transition margins per link, joint and motor, and the Properties
panel shows the peak numbers for the selected node. `sim-cad fit model
log.csv` fits friction, backlash and stiffness to a logged run; *Apply
identification…* stores the fitted values in the document so the next
export carries them (they show as `identified` on the joint).

Scripts: `scripts/robot_leg_demo.py` (two-link leg with servos, IMU,
encoders, a cable and a 2S battery) and `scripts/gripper_demo.py` (a
parallel gripper with two four-bars and a coupler: three loop closures).
Over the API: `GET /physical`, `GET /results`, `POST /results/load`,
`POST /identification/apply`, `POST /sensors`, `POST /cables`,
`PUT /battery`, `PUT /control`, `PUT /uncertainty`.

## CAD and Rhai experiments

Open **Experiments** to edit a system and controller, select a profile, run in the
background, and inspect synchronized plots and captured CAD replay. Candidate
review and run-linked comments support collaboration through the same REST API.
See [CAD + Rhai experiments](EXPERIMENTS.md) for setup, authoring, process
controllers, reproducible seeds, comparisons, caching and current limitations.

## Programmable motion in the Pose panel

Open **Pose**, then **Enter pose mode**. Select a driver joint to move it manually,
or choose a saved pattern and press **Play pattern**. **Pause** holds the current
position; the timeline scrubs in seconds; **Return to CAD pose** (or Esc) restores
the original display without editing geometry or the undo history.

**Make joint sweep** creates a modest sweep for the selected joint using the
cycle duration control. Edit its JSON to name it, add tracks, or change keyframes,
then **Save pattern**. Patterns are stored in the CAD file and support undo.
Angular tracks declare `deg` or `rad`; sliding tracks declare `mm`. Keys are
`[seconds, value]` pairs, ordered from zero through the pattern duration. Each
segment uses smooth cosine easing. A looping pattern must end where it starts.
Use driver joint IDs or unambiguous joint names; dependent joints are computed.

```json
{
  "name": "Inspect worm drive",
  "duration": 8,
  "loop": true,
  "tracks": [{
    "joint": "+X | Worm servo output",
    "unit": "deg",
    "keys": [[0, 0], [2, 150], [6, -150], [8, 0]]
  }]
}
```

Ideal transmission ratios are shared with physical export: a 5:1 drive turns the
thigh by 30 degrees for 150 degrees at the worm input. Motor-driven closed hinges
solve their passive rotation and sliding coordinates, so the curved knee link
stays connected to the foot. The readout shows joint positions and closure error.
An infeasible pose stops playback and keeps the last valid displayed pose.

This is geometric inspection, not dynamic simulation: collision, torque, contact,
backlash, belt motion/deformation and worm self-locking are not evaluated. Unset
limits use labeled preview bounds. Cut planes still clip moving geometry; exact
CAD section outlines are hidden during preview. Large assemblies reuse mesh
buffers for motion. Isolate a leg with the outliner or annotation part links to
see its enclosed mechanisms more clearly.

The local REST API supports `GET /motion`, `GET/POST /motion/programs`, and
`DELETE /motion/programs` with `{"name":"..."}`. `POST /motion/play` or
`POST /motion/seek` accepts `{"program": "saved name", "time": 2}` (or a program
object). `POST /motion/pause` and `POST /motion/stop` control the visible preview.
Program CRUD also works headlessly; playback requires the desktop window.

### Export a motion video

Choose a pattern and frame it using the current camera or **Focus mechanism**.
Select **720p** or **1080p**, choose **24, 30, or 60 fps**, then click **Export MP4…**.
The exporter records one cycle at fixed time steps, independent of playback speed.
It captures the 3D viewport without sidebars or comment widgets, preserving the
viewport aspect ratio with borders when needed. Progress and **Cancel export**
remain available while a worker encodes H.264 video. The previous pose is restored
when export finishes. Cancellation removes the partial export and preserves an
existing destination file. The camera is held fixed during recording.

`POST /motion/export` starts an export and returns immediately:

```json
{
  "program": "+X knee drives foot",
  "path": "/absolute/path/knee.mp4",
  "fps": 24,
  "width": 1280,
  "height": 720
}
```

Poll `GET /motion/export` for frame progress or use `DELETE /motion/export` to
cancel. `POST /motion/focus` frames the selected mechanism. Video encoding uses
the FFmpeg executable supplied by the `imageio-ffmpeg` CAD dependency.

## Component library and nested parametric assemblies

Open **Window → Components library** (also available in the command palette).

- **Make from selection** turns an assembly into a linked component. Existing
  body and joint IDs remain stable. Select a group, several parts, or a linked
  component to wrap it in a new parent component.
- **New parametric…** creates a box or cylinder with named dimensions. **Place…**
  inserts an occurrence with its own origin, rotation and required connections.
- **Edit defaults…** changes shared values. The Parameters tab includes units,
  bounds and measured/derived/estimated provenance. The geometry recipe explicitly
  binds those parameters to box/cylinder construction, rigid part placement, or
  joint frames. Imported geometry has no inferred design dimensions.
- Select an occurrence or one of its parts, then use the **Occurrence** tab. Check
  **Override** for the values that should differ. Unchecked values inherit shared
  defaults. **Reset to inherited** removes that occurrence's local overrides.
- To nest components, place the children, group them, and **Make from selection**.
  In the parent's **Nested parameters** tab, map child parameter names to parent
  expressions, for example `{"length": "leg_length / 2"}`. Child defaults are
  shared globally; parent mappings specialize the child; an explicit override on
  a particular nested occurrence takes precedence. Resetting it restores the
  parent mapping. Circular component dependencies are rejected.
- **Save to library…** writes a `.rcomp` file containing the selected definition
  and its nested dependencies. **Import…** embeds them into another document.
  Documents are self-contained; later edits to a library file do not silently
  change an already-open assembly. Conflicting definitions with the same ID are
  rejected. The saved library browser initially uses `~/Documents/RoboCAD/Components`.
- **Detach outer occurrence** makes its full assembly ordinary editable parts.
  Undo restores the link. Individual linked members cannot be reshaped, deleted,
  or rearranged independently; change the recipe/parameters or detach first.

The desktop runs component geometry and display preparation in a separate process.
The panel shows stage counts and offers cancellation. Viewing can continue during
preparation. A complete result applies as one undo step only if the captured CAD
revision is still current; intervening edits cause the prepared result to be
rejected, preserving your work.

Definitions store source geometry once, including across nested parents. Every
occurrence materializes ordinary CAD bodies/joints with independent stable IDs.
The existing physics exporter consumes those bodies; shared CAD definitions do
not share actuator state or couple the legs' motion. Rigid placements transform
joint frames, motor/sensor coordinates, declared centers of mass and inertia.
Nonrigid scale is rejected. Changing geometry with a declared mass override
requires an explicit updated mass/material definition rather than retaining a
stale measurement.

### Automation

`GET /components` returns the shared definition/parameter/feature catalogue.
The following methods are also available through `POST /ops/{method}` and Python
`Ops(doc)`:

- `make_component(ids, name, origin)` — capture and link the selected assembly.
- `create_component(ids, name, origin)` — capture a library definition only.
- `new_parametric_component(name, shape)` — box or cylinder recipe.
- `place_component(definition_id, placement, overrides, bindings, name)`.
- `set_component_parameters(definition_id, parameters, features, nested)`.
- `set_component_overrides(instance_id, overrides, placement)` — an empty override
  object resets inheritance; nested occurrence placement belongs to its parent.
- `import_component(path)`, `export_component(definition_id, path)` and
  `detach_component(instance_id)`.

Desktop component operations return `{"job": {"id": ...}}`. Poll
`GET /component-jobs/{id}` until `applied`, `failed`, or `cancelled`; use
`DELETE /component-jobs/{id}` to cancel. Headless `Ops` and REST execute the same
validated commands synchronously. Persist the resulting `.rcad` alongside
controller configurations and measurements.

Examples are in `examples/components/parametric-quadruped` (a four-occurrence,
three-definition nested assembly) and `examples/components/quadruped-linked-leg`
(the existing quadruped with its +X leg linked, preserving the original geometry).
The former is an estimated CAD acceptance example, not a calibrated walking robot.

Current scope: ordinary CAD bodies, sheets, curves, groups, joints, sensors and
cables can be captured. Legacy single-body instances, mesh/image/sketch nodes and
native system-graph bindings must be resolved explicitly before capture. Recipes
support existing topology with box/cylinder generation, placement and joint-frame
features; arbitrary feature-history recovery from imported B-reps is not provided.

### Component families and complete quadruped conversion

A **component family** shares parameter defaults across named assembly variants.
Variants can have different part counts and topology. They expose the same typed
external connections. **Place** lets you choose a variant; the Occurrence panel
identifies the placed variant. **Edit defaults → Family variants** shows how the
shared parameters drive each variant's parameters. Use the library search field
to find a family or nested component quickly.

Python/REST operations `create_component_family(name, variants, parameters,
default_variant)` and `link_component_family(instance_id, definition_id, variant,
overrides)` use the same validation and undo path. `place_component` accepts an
optional `variant`. `set_component_parameters` accepts `family_variants` to edit
validated parameter bindings. Linking an existing occurrence to its corresponding
family variant preserves its node IDs. Switching to a different topology requires
a new occurrence and explicit reconnection.

Additional shared recipe operations are `assembly_placement` (target `*`, rigid
translation/rotation of the complete nested assembly), `joint_ratio` (positive
actuator-to-joint ratio), and `joint_home` (angle in degrees, converted to radians
in the joint record). They run in the existing cancellable component worker.

The complete converted quadruped is in
`examples/components/quadruped-parametric/model/robot.rcad`. Its README documents
the verified backups, retained variants, exposed parameters, and geometry scope.


## Crash logs

Use **Help → Open diagnostics folder**. On macOS, logs are in
`~/Library/Logs/RoboCAD/`; `latest-editor.json` identifies the most recent editor
session. You can simply report that the editor crashed; the session log records
recent actions, errors and fatal-signal tracebacks. Full macOS crash reports are
collected on the next launch or with `python -m robocad.diagnostics`. See
[Crash diagnostics](README.md#crash-diagnostics) for commands and platform paths.
