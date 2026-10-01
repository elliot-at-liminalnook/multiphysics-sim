//! `CadAction` as its REST command: what `system_ui`, the surfaces and the
//! palette list as a control's action (split from `actions` to keep it
//! under the size cap).
use super::actions::CadAction;
use serde_json::{Value, json};

/// A payload struct's fields with the command name added.
fn tagged<T: serde::Serialize>(command: &str, args: &T) -> Value {
    let mut value = serde_json::to_value(args).unwrap_or_else(|_| json!({}));
    if let Value::Object(map) = &mut value {
        map.insert("command".into(), json!(command));
        value
    } else {
        json!({"command": command})
    }
}

/// An action as its REST command (what `system_ui` lists as a control's action).
pub(in crate::cad) fn rest_form(action: &CadAction) -> Value {
    match action {
        CadAction::CadUndo => json!({"command": "cad_undo"}),
        CadAction::CadRedo => json!({"command": "cad_redo"}),
        CadAction::CadSave { path } => json!({"command": "cad_save", "path": path}),
        CadAction::CadRefresh => json!({"command": "cad_refresh"}),
        CadAction::CadFit { id } => json!({"command": "cad_fit", "id": id}),
        CadAction::CadPhysical => json!({"command": "cad_physical"}),
        CadAction::CadDelete { id } => json!({"command": "cad_delete", "id": id}),
        CadAction::CadSelect { ids, items, extend, toggle, .. } => json!({"command": "cad_select", "ids": ids, "items": items, "extend": extend, "toggle": toggle}),
        CadAction::CadSelectMode { mode } => json!({"command": "cad_select_mode", "mode": mode}),
        CadAction::CadHover { item } => json!({"command": "cad_hover", "item": item}),
        CadAction::CadBoxSelect { rect, extend } => json!({"command": "cad_box_select", "rect": rect, "extend": extend}),
        CadAction::CadCandidates { items, extend, toggle } => json!({"command": "cad_candidates", "items": items, "extend": extend, "toggle": toggle}),
        CadAction::CadSelectAll => json!({"command": "cad_select_all"}),
        CadAction::CadInvertSelection => json!({"command": "cad_invert_selection"}),
        CadAction::CadSelectSameMaterial => json!({"command": "cad_select_same_material"}),
        CadAction::CadEdgesToFaces => json!({"command": "cad_edges_to_faces"}),
        CadAction::CadTool { tool } => json!({"command": "cad_tool", "tool": tool}),
        CadAction::CadTransform { ids, translation, axis, angle_deg, center, scale, revision } => {
            json!({"command": "cad_transform", "ids": ids, "translation": translation, "axis": axis, "angle_deg": angle_deg, "center": center, "scale": scale, "revision": revision})
        }
        CadAction::CadPushPull { node, face, distance, revision } => json!({"command": "cad_push_pull", "node": node, "face": face, "distance": distance, "revision": revision}),
        CadAction::CadOffsetFaces { node, faces, distance, revision } => json!({"command": "cad_offset_faces", "node": node, "faces": faces, "distance": distance, "revision": revision}),
        CadAction::CadSetDimension { node, dimension, faces, value, revision } => {
            json!({"command": "cad_set_dimension", "node": node, "dimension": dimension, "faces": faces, "value": value, "revision": revision})
        }
        CadAction::CadNumeric { values } => json!({"command": "cad_numeric", "values": values}),
        CadAction::CadMeasure { a, b, keep } => json!({"command": "cad_measure", "a": a, "b": b, "keep": keep}),
        CadAction::CadCancel => json!({"command": "cad_cancel"}),
        CadAction::CadPatch { id, attrs } => json!({"command": "cad_patch", "id": id, "attrs": attrs}),
        CadAction::CadCommand { id } => json!({"command": "cad_command", "id": id}),
        CadAction::CadOp { name, args, kwargs } => json!({"command": "cad_op", "name": name, "args": args, "kwargs": kwargs}),
        CadAction::CadOpen { path, url } => json!({"command": "cad_open", "path": path, "url": url}),
        CadAction::CadInvoke { id } => json!({"command": "cad_invoke", "id": id}),
        CadAction::CadRun { id, params, items, revision } => json!({"command": "cad_run", "id": id, "params": params, "items": items, "revision": revision}),
        CadAction::CadFormSet { name, value } => json!({"command": "cad_form_set", "name": name, "value": value}),
        CadAction::CadFormSubmit => json!({"command": "cad_form_submit"}),
        CadAction::CadFormCancel => json!({"command": "cad_form_cancel"}),
        CadAction::CadSketch { node, plane, calls, revision } => json!({"command": "cad_sketch", "node": node, "plane": plane, "calls": calls, "revision": revision}),
        CadAction::CadSurface { surface } => json!({"command": "cad_surface", "surface": surface}),
        CadAction::State => json!({"command": "state"}),
        CadAction::CadState => json!({"command": "cad_state"}),
        CadAction::SystemUi(args) => json!({"command": "system_ui", "action": args.get("action")}),
        CadAction::CadDisplay(a) => tagged("cad_display", a),
        CadAction::CadSection(a) => tagged("cad_section", a),
        CadAction::CadViews(a) => tagged("cad_views", a),
        CadAction::CadFile(a) => tagged("cad_file", a),
        CadAction::CadExport(a) => tagged("cad_export", a),
        CadAction::CadRender(a) => tagged("cad_render", a),
        CadAction::CadRobot(a) => tagged("cad_robot", a),
        CadAction::CadMaterials(a) => tagged("cad_materials", a),
        CadAction::CadInspector(a) => tagged("cad_inspector", a),
        CadAction::CadResults(a) => tagged("cad_results", a),
    }
}

