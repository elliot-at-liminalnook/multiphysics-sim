//! `cad_guide` (and `GET /v1/cad_guide`): how the in-process CAD editor
//! works, for an agent starting cold. The concepts (the archive, revisions,
//! undo, units and frames, face/edge indices), the workflows in order, every
//! command an agent uses with a working example, and the rules. Written from
//! the code it describes; the command list mirrors `GET /v1/capabilities`.
use serde_json::{Value, json};

/// The guide (`topic` narrows it to one section: concepts, workflows,
/// commands, chat, views, rules).
pub(crate) fn guide(topic: Option<&str>) -> Result<Value, String> {
    let all = json!({
        "about": "The CAD editor runs inside the viewer process (Rust + OCCT 7.7.2). The open document is a .rcad archive (RoboCAD's format: manifest.json + brep/<id>.brep per body). Every change is an in-process edit: applied on a job, one undo step, the archive rewritten in memory; nothing is written to disk until cad_save. RoboCAD's Python service is not involved.",
        "how_to_call": {
            "batch": "POST /v1/batch {\"commands\":[{\"command\":NAME,\"args\":{...}}]} → {\"job_id\"}; GET /v1/jobs/{job_id} until status is succeeded | failed | cancelled; results[i].ok, results[i].value or results[i].error. Commands in one batch run in order.",
            "resources": "GET /v1/cad_state (the document now), /v1/cad_threads (comment feed), /v1/cad_guide (this), /v1/capabilities (every command with its description).",
            "projects": "Building a robot end to end (design → model → test → learn → make)? Start with GET /v1/project_guide: a robot project points every mode at one robot and makes its simulation model and tests from this editor's saved design.",
            "one_at_a_time": "The REST queue runs one job at a time; a cad_threads watch holds it while it waits (stream GET /v1/events/cad_threads instead to wait without holding it).",
        },
        "concepts": {
            "units_and_frame": "Millimetres, degrees, the archive's frame with Z up. Masses are SI (kg, m, kg·m²).",
            "nodes": "cad_state.nodes: id, kind (body | sheet | curve | sketch | plane | group | instance | joint | sensor | cable | image | measure), name, parent, visibility. Bodies are world placed: a move rewrites the B-rep (RoboCAD's convention); instances keep source + transform.",
            "revision": "cad_state.document_key.revision moves forward on every edit and undo. Face and edge indices belong to one revision: read cad_model topology after the last edit before naming faces or edges.",
            "faces_and_edges": "cad_model {op: topology, node} lists faces (index, kind, center, normal, area_mm2) and edges (index, kind, midpoint, length_mm, start, end). Operations take faces as {\"node\": ID, \"face\": i} and edges as {\"node\": ID, \"edge\": i} (RoboCAD's ArgConverter form); cad_model's own fillet/chamfer take edges: [i, ...].",
            "undo": "Each command that changes the document is one undo step (cad_undo / cad_redo; history in cad_state.history). A refused or failed operation changes nothing.",
            "physics": "cad_state.local_mass: exact B-rep volume × material density per body and for the assembly, with provenance. Materials are the document's list (pla, petg, abs, steel, al …); cad_patch {attrs: {material}} changes one.",
            "saving": "cad_save writes the archive atomically (cad_state.unsaved says whether there are unsaved edits). RoboCAD reads files written here.",
        },
        "workflows": [
            {"goal": "Look before changing", "steps": ["GET /v1/cad_state: nodes, selection, local_mass, history", "cad_model {op: topology, node} for the faces/edges you will name", "cad_render {path: /abs/file.png, view: iso, edges: true} to see the model (or cad_views save + the person restores it)"]},
            {"goal": "Make a part", "steps": ["cad_model {op: box | cylinder | sphere | cone, ...} or {op: extrude, loops, direction}", "or a sketch: cad_sketch {plane: xy, calls: [[\"rectangle\", [[x, y], [w, h]]], [\"circle\", [[cx, cy], r]]]} then cad_op {name: extrude, args: [SKETCH_ID, distance], kwargs: {op: new | union | subtract | intersect, target}}"]},
            {"goal": "Change a part", "steps": ["fillet/chamfer: cad_model {op: fillet, node, radius, edges}", "resize: cad_op {name: push_pull, args: [NODE, {node, face}, mm]}; cad_op {name: set_distance | set_diameter | set_angle, ...}", "hollow: cad_op {name: shell, args: [NODE, wall_mm, [open faces]]}", "combine: cad_model {op: fuse | cut | common, target, tools}", "move: cad_model {op: move, ids, translation, axis, angle_deg}"]},
            {"goal": "Robot parts", "steps": ["cad_op {name: add_motor, args: [spec_id, mount_point, shaft_dir], kwargs: {mount_on: BODY, cut_mount: true}} (spec ids: GET cad_state.robot or the motor library: sg90, mg996r, nema17, n20_100, …)", "cad_op {name: add_joint, args: [type, parent, child, pivot, axis]}; cad_op {name: attach_motor, args: [JOINT, MOTOR]}", "cad_op {name: infer_joints} finds shaft-in-bore revolute joints"]},
            {"goal": "Discuss with the person", "steps": ["cad_threads {op: create, node, point, body, author, author_kind: agent} pins a comment on a part", "cad_threads {op: watch, reader, author_kind: person, timeout_s} waits for their reply", "cad_threads {op: reply, thread, body, author_kind: agent}", "inside the window the built-in AI answers automatically (cad_threads {op: ai, on}); ask it with {op: ask, thread}"]},
            {"goal": "Show the person something", "steps": ["cad_views {op: save, name, description, fit: [ids], direction: front | top | iso…, section: {axis: x | y | z, offset}, parts: [ids]} — they click it in the Saved Views panel", "link parts in comments with [label](part:ID)"]},
            {"goal": "Several edits that must all succeed", "steps": ["cad_model {op: batch, operations: [{op: NAME, args, kwargs, as: alias}, ...]}: RoboCAD operations (cad_op names) in order as ONE undo step; {\"$ref\": alias} passes an earlier result (a new node's id) into a later call; any failure changes nothing"]},
        ],
        "commands": {
            "cad_state": {"example": {}, "does": "The document: nodes, selection, local_mass, history, threads, views, unsaved."},
            "cad_model": {"example": {"op": "cylinder", "name": "Axle", "base": [0, -40, 30], "axis": [0, 1, 0], "radius": 4, "height": 80, "material": "steel"}, "ops": "box, cylinder, sphere, cone, extrude, group, fuse, cut, common, fillet, chamfer, move, topology, batch"},
            "cad_op": {"example": {"name": "push_pull", "args": ["NODE", {"node": "NODE", "face": 5}, 3.0]}, "does": "Any RoboCAD operation by name (Python argument binding).", "names": sim_cad::ops::METHODS},
            "cad_sketch": {"example": {"plane": "xy", "calls": [["rectangle", [[0, 0], [40, 20]]], ["circle", [[20, 10], 4]]]}, "does": "Sketch calls (RoboCAD kernel/sketch.py names; curves by index) on a new sketch (plane) or an existing one (node)."},
            "cad_patch": {"example": {"id": "NODE", "attrs": {"name": "Bracket", "material": "petg", "color": [0.2, 0.5, 0.9], "visible": true}}},
            "cad_delete": {"example": {"id": "NODE"}},
            "cad_undo": {"example": {}}, "cad_redo": {"example": {}},
            "cad_save": {"example": {}, "does": "Write the file (or {path: /abs/new.rcad} to save as)."},
            "cad_file": {"example": {"op": "import", "path": "/abs/bracket.step"}, "does": "File workflows: import (STEP/IGES solids become bodies named after the file, an SVG drawing one sketch on XY, PNG/JPEG reference images; meshes are refused), new {path} (an empty .rcad, then opened), save_as {path}, open {path}."},
            "cad_export": {"example": {"format": "step", "path": "/abs/out.step"},"formats": "stl, 3mf, step, iges, obj, svg (a sketch), drawing (front/top/right/iso SVG with hidden lines and section A-A)"},
            "cad_render": {"example": {"path": "/abs/view.png", "view": "iso", "mode": "shaded", "section": "z:30", "highlight": ["NODE"], "edges": true, "title": "Bracket"}},
            "cad_threads": {"example": {"op": "reply", "thread": "THREAD", "body": "Done: …", "author": "Claude", "author_kind": "agent"}, "ops": "list, get, create, reply, edit, delete, delete_thread, resolve, link, watch, seen, ask, ai, show, fit"},
            "cad_views": {"example": {"op": "save", "name": "Bore section", "fit": ["NODE"], "section": {"axis": "y", "offset": 0}, "parts": ["NODE"], "description": "Why it matters"}, "ops": "list, save, update, rename, replace, delete, restore, panel"},
            "cad_select": {"example": {"items": [["NODE", "face", 3]]}, "does": "Set the shared selection (modes body, face, edge, vertex, point)."},
        },
        "rules": [
            "Read cad_state (and topology) before editing; check the result after.",
            "Prefer small, explained changes: each is one undo step the person can undo.",
            "Do not cad_save unless asked; never edit repository files to change the model.",
            "Face/edge indices change after geometry edits: read topology again.",
            "Refusals name their reason (a locked part, a too-large fillet, a stale revision): fix the cause, do not retry blindly.",
            "Masses are exact geometry × catalogue density, provisional until measured; say so when it matters.",
            "Not in the in-process editor yet (refused by name): print studies/split/jobs/wall checks, flexible links and link-to-link contact in the simulator export (it writes rigid links), components and composition, experiments and captured-run review, results load, mesh import.",
        ],
    });
    match topic {
        None => Ok(all),
        Some(t) => all.get(t).cloned().map(|v| json!({t: v})).ok_or_else(|| format!("no guide topic {t} (concepts, workflows, commands, rules, how_to_call)")),
    }
}
