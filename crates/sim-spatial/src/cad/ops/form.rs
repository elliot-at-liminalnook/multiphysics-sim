//! The catalogue's parameter form (`CadDocument::ops.form`): opening it
//! with RoboCAD's defaults, a draft set, OK (`CadFormSubmit`: the drafts as
//! a `CadRun`), Cancel and Escape (`form_cancel`, which also ends a pick,
//! place, sketch, extrude or plane interaction without sending anything),
//! and the form as `cad_state.ops.form` shows it. Split from `ops` to keep
//! it under the size guard.
use super::*;

/// `CadFormSubmit`: the open form's drafts as `CadRun` parameters. A form
/// flow's run is refused when RoboCAD's revision changed since the form
/// opened; a pick or place tool's form stays open across its runs, so its
/// picks carry their own guard (the selection's revision) instead, and a
/// sketch tool's Tab values anchored at a clicked point carry that click's
/// (`SketchState::began`).
pub(super) fn submit(call: &mut Call, cx: &mut Cx) -> Outcome {
    let Some(form) = cx.doc.ops.form.clone() else { return Outcome::Done(Err("no form is open".into())) };
    let Some(entry) = entry(form.op) else {
        cx.doc.ops.form = None;
        return Outcome::Done(Err(unknown(form.op)));
    };
    // Only the shown fields are sent: a hidden one (its `when` does not
    // hold) would be refused as not applying.
    let text_of = |q: &Param| Value::String(entry.params.iter().position(|x| x.name == q.name).and_then(|i| form.texts.get(i)).cloned().unwrap_or_default());
    let mut params: Map<String, Value> = entry.params.iter().zip(&form.texts).filter(|(p, _)| gate(entry, p, text_of).unwrap_or(true)).map(|(p, t)| (p.name.to_string(), Value::String(t.clone()))).collect();
    // A sketch tool's Tab values are anchored at its first clicked point
    // (RoboCAD's `SketchTool.commit`: `self.points[0]`, else the plane
    // origin), and refused by name when RoboCAD's document changed since
    // that click (`began`). A chained point is the end of the line just
    // sent (whose own edit moved the revision): it is a point on the plane,
    // not a pick of geometry that may be gone, so it carries no guard.
    let sketching = matches!(entry.flow, Flow::Sketch(_));
    let mut clicked: Option<u64> = None;
    if sketching
        && params.get("anchor").is_none_or(|a| a.as_str().is_some_and(str::is_empty))
        && let Some(s) = cx.doc.ops.sketch.as_ref()
        && let Some(p) = s.points.first()
    {
        params.insert("anchor".into(), Value::String(p.map(crate::cad::transform::num).join(", ")));
        clicked = (!s.chained).then_some(s.began);
    }
    let revision = if entry.flow == Flow::Form { Some(form.began) } else { clicked };
    let outcome = run(entry, &params, None, revision, call, cx);
    // RoboCAD's `commit` clears the clicked points after its edit.
    if sketching
        && !matches!(outcome, Outcome::Done(Err(_)))
        && let Some(s) = cx.doc.ops.sketch.as_mut()
    {
        s.points.clear();
        s.chained = false;
        s.cursor = None;
    }
    if let Outcome::Done(Err(e)) = &outcome {
        if let Some(f) = cx.doc.ops.form.as_mut().filter(|f| f.op == entry.id) {
            f.error = Some(e.clone());
        }
        cx.doc.touch();
    }
    outcome
}

/// Open `entry`'s form with RoboCAD's defaults (or the drafts of the same
/// form, when it is open already); a number field first takes the keyboard
/// with its text selected (RoboCAD's dialog).
pub(super) fn open_form(doc: &mut CadDocument, entry: &'static OpEntry) -> Value {
    let texts = match &doc.ops.form {
        Some(f) if f.op == entry.id && f.texts.len() == entry.params.len() => f.texts.clone(),
        _ => entry.params.iter().map(|p| p.default.to_string()).collect(),
    };
    // A dialog's first number field takes the keyboard; a pick or place
    // tool's fields wait for Tab, as RoboCAD's numeric bar does.
    let focus = match entry.params.first() {
        Some(p) if entry.flow == Flow::Form && matches!(p.kind, FieldKind::Number { .. }) => Some(0),
        _ => None,
    };
    let began = doc.shown_revision();
    doc.ops.form = Some(FormState { op: entry.id, texts, focus, select_all: true, began, error: None });
    doc.touch();
    json!({"opened": entry.id, "form": form_json(doc)})
}

/// `CadFormSet`: one draft (a string as typed; a bool or number as its
/// text; `[x, y, z]` as "x, y, z").
pub(super) fn form_set(doc: &mut CadDocument, name: &str, value: &Value) -> Result<Value, String> {
    {
        let Some(form) = doc.ops.form.as_mut() else { return Err("no form is open".into()) };
        let entry = entry(form.op).ok_or_else(|| unknown(form.op))?;
        let Some(i) = entry.params.iter().position(|p| p.name == name) else {
            let names: Vec<&str> = entry.params.iter().map(|p| p.name).collect();
            return Err(format!("{} has no parameter {name} (its parameters: {})", entry.id, if names.is_empty() { "none".to_string() } else { names.join(", ") }));
        };
        let text = match value {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.iter().map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string)).collect::<Vec<_>>().join(", "),
            other => other.to_string(),
        };
        form.texts.resize(entry.params.len(), String::new());
        form.texts[i] = text;
        if form.focus == Some(i) {
            form.select_all = false;
        }
        form.error = None;
    }
    doc.touch();
    Ok(form_json(doc))
}

/// `CadFormCancel` (Cancel, Escape): close the form and end its
/// interaction (also transform's `CadCancel` while a pick or place op is active).
pub(in crate::cad) fn form_cancel(doc: &mut CadDocument) -> Value {
    let form = doc.ops.form.take().map(|f| f.op);
    let active = doc.ops.active.take();
    let place = doc.ops.place.take().is_some();
    // A sketch shape, an extrude drag or plane picks end with nothing sent
    // (a lone chained point was sent with its line: no shape in progress).
    let sketch = doc.ops.sketch.take().is_some_and(|s| s.unsent());
    doc.ops.extrude = None;
    doc.ops.plane_picks.clear();
    if form.is_none() && active.is_none() && !place {
        return json!({"closed": null});
    }
    let what = active.or(form).and_then(entry).map_or("", |e| e.label);
    doc.show(Ok(format!("Cancelled {what}")));
    json!({"closed": {"form": form, "active": active, "place": place, "sketch_in_progress": sketch}})
}

/// The open form as `cad_state.ops.form` shows it: each field's draft,
/// whether it is shown (its `when`), and its evaluation or error.
pub(super) fn form_json(doc: &CadDocument) -> Value {
    let Some(form) = &doc.ops.form else { return Value::Null };
    let Some(entry) = entry(form.op) else { return Value::Null };
    let text_of = |q: &Param| Value::String(entry.params.iter().position(|x| x.name == q.name).and_then(|i| form.texts.get(i)).cloned().unwrap_or_default());
    let fields: Vec<Value> = entry
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let text = form.texts.get(i).map_or("", String::as_str);
            // Compared as the gating field reads (its canonical option).
            let shown = gate(entry, p, text_of).unwrap_or(false);
            let evaluation = if !shown {
                Value::Null
            } else if text.is_empty() && p.default.is_empty() {
                json!({"ok": true, "optional_or_required": "empty"})
            } else {
                match param_value(p, &Value::String(text.to_string())) {
                    Ok(v) => json!({"ok": true, "value": v}),
                    Err(e) => json!({"ok": false, "error": e}),
                }
            };
            let mut f = Map::new();
            f.insert("name".into(), json!(p.name));
            f.insert("label".into(), json!(p.label));
            f.insert("kind".into(), json!(format!("{:?}", p.kind)));
            f.insert("text".into(), json!(text));
            f.insert("default".into(), json!(p.default));
            f.insert("shown".into(), json!(shown));
            f.insert("evaluation".into(), evaluation);
            Value::Object(f)
        })
        .collect();
    let mut out = Map::new();
    out.insert("op".into(), json!(entry.id));
    out.insert("label".into(), json!(entry.label));
    out.insert("fields".into(), Value::Array(fields));
    out.insert("focus".into(), json!(form.focus));
    out.insert("began".into(), json!(form.began));
    out.insert("error".into(), json!(form.error));
    Value::Object(out)
}

