# An agent in the CAD editor

`agent_loop.py` shows an agent doing a design review over the viewer's REST
API. It opens a model, models a part, pins a comment, saves a sliced named
view, saves the file, then answers the person's reply. Everything runs in
the viewer's own process (Rust + OCCT 7.7.2). There is no CAD server.

```sh
cp examples/wheeled-robot/baseline/robot.rcad /tmp/rover.rcad
target/debug/sim-spatial /tmp/rover.rcad          # REST on http://127.0.0.1:8421
python3 examples/cad-agent-loop/agent_loop.py /tmp/rover.rcad
```

Then reply in the window's Comments dock. Agent comments carry an **AI** tag.
Open the Saved Views panel and press **Restore view** on "Battery tray: cable
hole section".

## A whole robot over REST

`quadruped.py` models a 12-servo quadruped from an empty file in about a
minute: a shelled PETG chassis with electronics, four legs (one batch, one
undo step each) with servo-driven joints, encoders, foot contact sensors and
harness cables, battery and control settings, saved views and a pinned
design-notes comment. `quadruped.rcad` is its output (robot valid: 37 bodies,
24 joints, 12 DoF; masses provisional).

```sh
python3 examples/cad-agent-loop/quadruped.py /tmp/quadruped.rcad   # with the viewer running
```

## Start here

`GET /v1/cad_guide` (or the `cad_guide` command) explains the editor for an
agent starting cold: the concepts (archive, revisions, undo, units, face and
edge indices), the workflows in order, every command with a working example,
and the rules. `GET /v1/capabilities` lists every command.

## The commands an agent uses

| Command | What for |
| --- | --- |
| `cad_state` | Parts (`nodes`), selection, exact mass (`local_mass`), undo history, `unsaved` |
| `cad_model` | `box`, `cylinder`, `sphere`, `cone`, `extrude`, `group`, `fuse`/`cut`/`common`, `fillet`/`chamfer`, `move`, the read-only `topology` (face and edge indices with centres, normals, lengths), and `batch` (several operations as one all-or-nothing undo step, `{"$ref": alias}` passing results along) |
| `cad_op` | Any RoboCAD operation by name, about 90 of them: push/pull, offset faces, live dimensions (`set_diameter`, `set_distance`, `set_angle`), shell, draft, sweep, loft, revolve, mirror, arrays, planes, motors, joints, sensors, cables, fastener holes |
| `cad_sketch` | Sketch calls (lines, arcs, circles, slots, splines, text, trim, extend, fillet, offset…) on a plane or an existing sketch |
| `cad_patch`, `cad_delete` | Rename, show/hide, lock, material, colour, parent; delete |
| `cad_threads` | `create`/`reply` with `author_kind: "agent"`, `get` (a thread with its part, mass, pin camera and views), `watch` (wait for new comments, by `author_kind`), `seen`, `resolve`, `link`; `ask` and `ai` drive the in-window assistant |
| `cad_views` | `save`/`update` a named view: `fit` parts, a `direction` (front…iso) or `yaw`/`pitch`, a `section` slice (`{axis, offset}` or `{origin, normal}`), `parts` shown alone, `description`; `restore`, `rename`, `delete` |
| `cad_export`, `cad_render` | STL, 3MF, OBJ, STEP, IGES, sketch SVG, drawing SVG (hidden lines, section A-A); an off-screen PNG of any view or section |
| `cad_file` | `import` STEP/IGES (bodies), SVG (a sketch), PNG/JPEG (reference images); `new`, `save_as`, `open` |
| `cad_undo`, `cad_redo`, `cad_save` | One undo step per edit; atomic save (or save as with `path`) |

`GET /v1/cad_threads` is the comment feed: threads, last comments, and each
reader's seen mark and unread count. `GET /v1/events/cad_threads` streams it
as server-sent events. Use it to wait without holding the REST queue (a
`watch` holds the queue while it waits).

## The in-window assistant

When a person comments in the Comments dock, the built-in assistant (Codex,
through `sim_agent`) answers in the thread. It reads `/v1/cad_guide` and
`/v1/cad_state`, can edit the model through the same REST commands (each edit
one undo step), and links the parts it talks about. The dock's **Ask AI**
button asks it about the open thread; the **AI answers new comments** chip
turns automatic answers off or on. Comments already in a file when it opens
are not answered. Its state is in `cad_state.threads.ai`; set
`SIM_CODEX_MODEL` / `SIM_CODEX_EFFORT` to choose the model.

## What the file keeps

Comments and saved views are stored in the `.rcad` in RoboCAD's schema, so
RoboCAD reads files written here. A pin stores RoboCAD's own geometry stamp,
reproduced exactly (`sim_cad::stamp`), so pins read "attached" in both
editors until the part's geometry changes. The additions sit beside
RoboCAD's fields and RoboCAD carries them along. On comments: `author_kind`,
and `agent-` ids for agent comments. On views: `description`, `parts`,
`author`, `author_kind`.

Everything an agent does over REST can also be done in the window. For saved
views that means **Edit…** in the Saved Views panel: name, description, use
the current camera, slice Off/X/Y/Z, show only the selected parts.

## Not in the in-process editor yet

These refuse by name (the guide's rules list them): print studies, split,
coupons, print jobs and the wall/thin checks; the simulator export
(RoboCAD's physical model with collision meshes, joint physics and flex)
and its live link; components and composition; experiments and
captured-run review; results load; mesh import.
