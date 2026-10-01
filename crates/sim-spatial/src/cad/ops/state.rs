//! The catalogue as `cad_state.ops` shows it. Split from `ops` to keep it
//! under the size guard.
use super::*;
use super::form::form_json;

/// Every catalogue entry as `cad_state.ops.catalogue` lists it (built once).
pub(super) fn catalogue_json() -> &'static Value {
    static JSON: OnceLock<Value> = OnceLock::new();
    JSON.get_or_init(|| {
        Value::Array(
            CATALOGUE
                .iter()
                .map(|e| {
                    let params: Vec<Value> = e.params.iter().map(|p| json!({"name": p.name, "label": p.label, "default": p.default, "when": p.when})).collect();
                    let mut m = Map::new();
                    m.insert("id".into(), json!(e.id));
                    m.insert("label".into(), json!(e.label));
                    m.insert("category".into(), json!(e.category));
                    m.insert("keys".into(), json!(e.keys));
                    m.insert("flow".into(), json!(format!("{:?}", e.flow)));
                    m.insert("route".into(), json!(e.route));
                    m.insert("params".into(), Value::Array(params));
                    Value::Object(m)
                })
                .collect(),
        )
    })
}

/// `cad_state.ops`: the open form with its fields and evaluations, the
/// active operation, the primitive being placed, the open command surface,
/// the cursor snap, the clipboard and the catalogue.
pub(in crate::cad) fn state_json(doc: &CadDocument) -> Value {
    let ops = &doc.ops;
    let mut out = Map::new();
    out.insert("form".into(), form_json(doc));
    out.insert("active".into(), json!(ops.active));
    out.insert("place".into(), ops.place.as_ref().map_or(Value::Null, |p| json!(format!("{p:?}"))));
    out.insert("surface".into(), ops.surface.as_ref().map_or(Value::Null, |o| json!({"surface": o.surface, "highlight": o.highlight})));
    out.insert("cursor_snap".into(), json!(ops.cursor_snap.filter(|(revision, _)| *revision == doc.shown_revision()).map(|(_, p)| p)));
    out.insert(
        "clipboard".into(),
        ops.clipboard.as_ref().map_or(Value::Null, |(revision, clip)| json!({"revision": revision, "items": clip.get("items").and_then(Value::as_array).map_or(0, Vec::len)})),
    );
    out.insert("analysis".into(), crate::cad::analysis_overlay::state_json(&ops.analysis));
    out.insert("catalogue".into(), catalogue_json().clone());
    Value::Object(out)
}
