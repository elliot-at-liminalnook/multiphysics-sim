# robocad architecture

A direct-modeling CAD tool for 3D-printable mechanical parts, written in
Python on Open CASCADE (OCCT 7.7 via the OCP binding) with a PySide6 UI and
an OpenGL viewport, and linked to the physics simulator in this repository.

```
robocad/
  units.py          expressions with units (mm internal), no eval
  kernel/base.py    GeometryKernel interface, Body, FaceRef/EdgeRef, Mesh, Plane
  kernel/occt.py    the OCCT implementation
  kernel/sketch.py  2D curves on planes → wires/faces; trim/split/extend/fillet/offset/join
  document.py       scene graph, materials, instances, persistence (.rcad zip), autosave, clipboard
  commands.py       Command/CommandStack (undo/redo) and Ops, the scripting façade
  analysis.py       measure, section, mass, curvature, continuity, draft/normal colours
  printing.py       fasteners, clearance, wall thickness, manifold validation, overhangs
  io/exporters.py   STEP/IGES/STL/3MF/OBJ(+MTL)/sketch SVG, parallel tessellation, validation gate
  io/drawing.py     technical drawing SVG (HLR visible/hidden lines, section hatching, sheet grid)
  io/importers.py   STEP (XDE names/colours/assemblies), IGES, meshes (trimesh), SVG, images
  io/snapshot.py    software renderer for headless screenshots
  simbridge.py      *.simrobot.json export and the save→simulate watcher
  bridge/server.py  websocket live link (Blender add-on in blender_addon/), bridge/webshare.py
  ui/viewport.py    QOpenGLWidget: camera (turntable/trackball), display modes, ID picking, snapping, view cube
  ui/tools.py       interaction state machines with Tab-to-type fields
  ui/widgets.py     palette, numeric bar, outliner, properties (live dimensions), materials, radial menus, dialogs
  ui/app.py         MainWindow: command registry + JSON keymap, menus, docks, import/export, bridges
```

## Kernel abstraction

`GeometryKernel` (kernel/base.py) is the only surface the rest of the
program uses: primitives, sketch-to-solid (extrude with taper/symmetric/
up-to, revolve, sweep, pipe, loft, fill, bridge), direct edits (booleans,
split, plane cuts, push/pull, offset faces, dependent offset to a body,
move/rotate faces, cylinder radius edits, draft, delete faces with
healing, imprint, shell, thicken, fillets — constant, variable, chordal,
full round, all-edges with tension — remove fillets, chamfers, transform,
mirror, join/unjoin, dissolve, projection, silhouette), queries (faces,
edges, vertices, mass properties, tessellation with per-face IDs,
validation, distances, sections, ray hits, normals, curvature,
continuity, control points) and serialisation.

Topology references are geometric: a `FaceRef` records surface kind,
centroid, normal, area and (for cylinders etc.) axis and radius. After
any edit the kernel re-finds the closest matching face (`match_face`).
That is what makes editing without a feature tree stable: "the face at
that place" survives the boolean that rebuilt the solid.

Hidden OCCT specifics worth knowing: booleans are fuzzy (1e-5) and unified
afterwards so seams disappear; cylinder radius edits are implemented as
fill-and-recut (holes) or add/subtract (bosses) with exact spans so no
caps are added; shell uses `MakeThickSolidByJoin` and reports the
actionable failure ("walls would meet") a 2 mm wall inside a 1 mm fillet
produces — hollow first, fillet after; fillets report "too large" when the
radius would consume a neighbour.

## Document model

`Document` holds `Node`s in a tree (groups) with kinds body, sheet, curve,
sketch, mesh, image, plane, measure, group and instance. A body owns a
kernel `Body`; an instance references a source node with a `Transform`
and optional mirror plane and is resolved on demand, so it follows edits
of its source (live). Materials carry density and colour; mass comes from
volume × density. Persistence is a zip: `manifest.json`, `brep/<id>.brep`,
`mesh/<id>.npz`, `image/<id>`, `thumbnail.png`. Autosave runs on a thread
at a configurable interval to `<name>.autosave.rcad`. The clipboard is a
JSON payload with B-rep hex and world placement so parts kit-bash between
windows and documents.

## Commands and undo

Every action is a `Command` with `do`/`undo`. `EditBodies` swaps kernel
handles (OCCT shapes are never mutated, so keeping the previous handle is
free), `SetAttributes` restores attribute dictionaries (names, visibility,
lock, materials, colours, pivots, transforms, sketches), `AddNodes`/
`RemoveNodes` keep nodes and positions, `MoveNode` keeps parent and index,
`Composite` groups several. `CommandStack` is the undo/redo history.
`Ops` is the façade the UI and scripts share; the acceptance macro is a
plain `Ops` script.

## UI layering

`ui/app.py` owns the document, the `Ops`, the viewport and the panels; it
registers ~130 commands with categories and shortcuts loaded from
`ui/keymap.json` (overridable by `~/.robocad/keymap.json`). Tools
(`ui/tools.py`) are state machines given a narrow `ToolContext`; each
declares numeric fields the `NumericBar` shows and Tab focuses. The
viewport renders tessellations with fixed-function OpenGL, picks through
an ID pass into a plain FBO (bodies, faces, edges, vertices), snaps to
vertices/midpoints/centres/grid/plane, and draws gizmos, planes, images,
sketches, section outlines, the build plate and the view cube. Text
overlays use QPainter. Strings go through `ui/strings.py` (English
shipped; JSON tables for other languages) and the high-contrast theme is
a stylesheet plus viewport palette.

## Bridge protocols

*Blender live link*: `bridge/server.py` serves JSON text frames over a
websocket; `{"hello": {"tolerance"}}` sets the tessellation from the
target side, the server pushes `{"scene": {objects: [...]}}` on every
change with per-face IDs, sharp edges, materials and the owning group as
the collection; the add-on keeps material assignments by face ID across
refreshes. *Web share*: a single HTML file with the tessellation embedded
(three.js from a CDN); no B-rep leaves the machine. *Simulator*:
`simbridge.py` writes `*.simrobot.json` (bodies with mass, COM, planar
inertia, section outlines; joints from `joint:<child>[:<parent>]` planes;
`ground`); the Rust side (`sim-phenomena::scenarios::cad_robot`) builds a
planar multibody with PD-held servos on the seam, `sim-cad` runs it
headless and `sim-app --scene cad --model file` draws it and rebuilds on
every save.

## Performance notes

Tessellation and mesh export run in a thread pool per body. The viewport
caches tessellations per node and tolerance and invalidates only touched
nodes (and their instances). Kernel operations on ~500-face bodies are
sub-second on a laptop; previews use coarser tolerances (0.2 mm).


## Physical layer (robotics.py, physical.py, flex.py)

`robotics.py` holds what the user declares: the motor library with each
entry's geometry and datasheet blocks (`MOTOR_DATASHEETS`, `motor_physics`
derives ke/kt/R where the sheet is silent), the `Joint` dataclass (tree
types plus `loop_*` closures), joint inference from coaxial pin/hole pairs,
validation. `physical.py` derives the simrobot v3 description
(`PHYSICAL_MODEL.md`): links merged through fixed joints and motor mounts
with full inertia (OCCT's matrix of inertia is about the centroid, scaled
by density), collision meshes (vertex clustering that keeps real surface
vertices), convex hulls and signed distance grids (k-d tree to dense
surface samples, exact point–triangle refinement within two cells, sign by
ray parity), joint physics from the bearing pair and the material table,
fastened flanges from the screws recorded by `Ops.fastener_hole`, sensors,
cables, battery/control/uncertainty settings from `Document.robot_settings`,
and the identification and results round trips. `flex.py` builds the voxel
hex FE model from the SDF (identical cubic elements: one stiffness matrix,
scatter-assembled), rigid RBE2 patches per joint/attachment, clamps the
parent joint and takes the lowest fixed-root modes with `scipy.sparse.linalg
.eigsh` (shift-invert), then recovers per-mode frame motions, rigid-body
participation and centroid stress tensors.

Materials carry an `engineering` dict on top of the print/render fields;
`Material.props()` fills defaults from `document._ENG` so old files load.
Sensors and cables are nodes of kind `sensor`/`cable` with `robot`
metadata; results hang on `Node.results` and `Document.results`. The
viewport paints `Node.results.hotspot` through a nearest-cell lookup into
per-vertex colours (`RenderItem.stress_colors`, `GL_COLOR_ARRAY`).
