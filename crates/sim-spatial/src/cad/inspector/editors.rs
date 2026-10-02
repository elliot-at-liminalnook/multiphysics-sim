//! The inspector's pivot and transform editors (cad-modify; RoboCAD's
//! `PATCH /nodes/{id}`, api.py:573-611): the inspected node's pivot
//! (`{"pivot": [x, y, z]}`, or `null` from "Clear pivot") and, for the
//! nodes RoboCAD keeps a placement for (instances and reference meshes and
//! images: `Transform`, document.py:142-144; bodies are baked in world),
//! its transform (`{"transform": {"translation", "axis", "angle_deg",
//! "scale"}}`).
//!
//! - **Typing** is the kit's one text field ([`EDITOR`], `ui_kit::text`;
//!   `ToolState::inspector_edit` is the value it edits, its text mirrored
//!   from the kit's draft): a press on a value gives it the keyboard with a
//!   draft of it (its text selected, so typing replaces it), Enter evaluates it with RoboCAD's unit expressions
//!   (`sim_runtime::units::evaluate`: lengths default to mm, the angle to
//!   degrees, the axis and scale are plain numbers; a vector is three
//!   expressions separated by commas) and writes one `CadPatch`, the
//!   existing one-edit path (one RoboCAD undo step); an error keeps the
//!   draft open with the evaluator's message naming the component, the
//!   token and its position. Escape, a press elsewhere, another node
//!   selected or another field taking the keyboard drop it.
//! - **A transform edit sends the whole transform**: RoboCAD's
//!   `Transform.from_json` fills absent keys with its defaults, so one
//!   changed component is sent with the other three as RoboCAD last
//!   reported them (`NodeSummary::transform`); a transform RoboCAD sent
//!   without one of them is not edited (nothing is filled in).
//! - **Refused as RoboCAD refuses them** (api.py:575-578), with nothing
//!   sent and the reason shown instead of the field: a component member's
//!   pivot and transform ("Edit component parameters or detach the
//!   occurrence first"), a component occurrence's transform ("Use
//!   set_component_overrides with placement for a component occurrence").
//!   A draft whose edit cannot be sent now (an edit in flight, not
//!   connected) stays open with the reason. A transform draft is also
//!   refused once the shown document is stale or RoboCAD's revision has
//!   moved since it opened (`CadDocument::commit_refusal`): the components
//!   it sends untouched were read then.
//! - **Untouched components keep their full precision**: the field shows
//!   values rounded to 1e-6, so a vector component whose typed text is
//!   still its shown text is sent as the value the draft opened with
//!   (an axis of 0.7071067811865476 is not re-sent as 0.707107).
//! - **Tessellation tolerance** (cad-views-export; RoboCAD's inspector
//!   spin box "Tessellation tolerance (mm)", ui/widgets.py:469-476: 0.005–2
//!   mm, three decimals, default 0.05): one `CadPatch
//!   {"tessellation_tolerance": mm}` (api.py:645, one undo step "Set
//!   attributes") on the inspected body, sheet or instance. RoboCAD's
//!   spin box writes every selected node directly, without undo
//!   (`_tol_changed`, widgets.py:729-735); here it is the inspected node,
//!   undoable, as every other inspector edit. RoboCAD reports no node's
//!   current value (`node_summary` has no `tessellation_tolerance`), so
//!   the field opens empty; the patch bumps RoboCAD's revision, so meshes
//!   are refetched, at the node's own tolerance (`mesh.rs` asks with
//!   `cad_client::NODE_TOLERANCE`, which RoboCAD's `mesh_of` reads as "the
//!   node's"): the change shows here as in RoboCAD's own viewport.
use super::{field, node};
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::{CadButton, edit_blocked, patch};
use crate::cad::selection::{CadItems, CadSelection};
use crate::cad::transform::num;
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFocus};
use crate::ui_kit::{DANGER, Kit, Look, SUBTLE, size, wrap};
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_runtime::cad_client::NodeSummary;

/// The inspector's value editors' field: one kit field for every editor
/// (the value is `ToolState::inspector_edit`).
pub(in crate::cad) const EDITOR: FieldId = FieldId("cad.inspector.edit");

/// The editors' field as the kit spawns it (`inspector::build`).
pub(in crate::cad) fn editor_field() -> TextField {
    TextField::new("Inspector value").select_on_focus()
}

/// The node kinds whose transform RoboCAD applies (instances, reference meshes and images).
pub const PLACED_KINDS: [&str; 3] = ["instance", "mesh", "image"];

/// The node kinds RoboCAD tessellates at their own tolerance (`Document.mesh_of`:
/// a mesh node keeps its triangles).
pub const TESSELLATED_KINDS: [&str; 3] = ["body", "sheet", "instance"];
/// RoboCAD's tolerance spin box: range, decimals and default (ui/widgets.py:469-473).
pub const TOLERANCE_RANGE: (f64, f64) = (0.005, 2.0);
pub const TOLERANCE_DEFAULT: f64 = 0.05;

/// Which value an editor types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKey {
    Pivot,
    Translation,
    Axis,
    Angle,
    Scale,
    /// RoboCAD's per-node tessellation tolerance, mm.
    Tessellation,
}
impl EditKey {
    pub fn label(self) -> &'static str {
        match self {
            EditKey::Pivot => "Pivot",
            EditKey::Translation => "Translation",
            EditKey::Axis => "Axis",
            EditKey::Angle => "Angle",
            EditKey::Scale => "Scale",
            EditKey::Tessellation => "Tessellation tolerance",
        }
    }
    fn unit(self) -> &'static str {
        match self {
            EditKey::Pivot | EditKey::Translation | EditKey::Tessellation => "mm",
            EditKey::Angle => "°",
            EditKey::Axis | EditKey::Scale => "",
        }
    }
}

/// An inspector value being typed (`ToolState::inspector_edit`).
#[derive(Clone, Debug, PartialEq)]
pub struct EditDraft {
    pub node: String,
    pub key: EditKey,
    pub text: String,
    /// The text is selected: the next character replaces it.
    pub select_all: bool,
    /// Why the last Enter sent nothing (the evaluator's error, a refusal).
    pub error: Option<String>,
    /// RoboCAD's revision shown when the draft opened (`shown_revision`).
    pub began: u64,
    /// The vector being typed (pivot, translation, axis) as RoboCAD sent
    /// it, unrounded: a component still showing its rounded text sends this.
    pub original: Option<[f64; 3]>,
}

/// An editor's value: a press opens its draft.
#[derive(Component, Clone, Debug)]
pub(in crate::cad) struct EditField {
    node: String,
    key: EditKey,
}

/// RoboCAD's `Transform` as `node_summary` reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub translation: [f64; 3],
    pub axis: [f64; 3],
    pub angle_deg: f64,
    pub scale: f64,
}
impl Placement {
    pub fn json(&self) -> Value {
        json!({"translation": self.translation, "axis": self.axis, "angle_deg": self.angle_deg, "scale": self.scale})
    }
}

fn vec3(v: Option<&Value>) -> Option<[f64; 3]> {
    match v?.as_array()?.as_slice() {
        [x, y, z] => Some([x.as_f64()?, y.as_f64()?, z.as_f64()?]),
        _ => None,
    }
}

/// The node's transform as RoboCAD sent it; an error names what is missing.
pub fn placement(n: &NodeSummary) -> Result<Placement, String> {
    let t = &n.transform;
    let missing = |key: &str| format!("RoboCAD's transform of {} has no readable {key} ({t}); not edited", n.name);
    Ok(Placement {
        translation: vec3(t.get("translation")).ok_or_else(|| missing("translation"))?,
        axis: vec3(t.get("axis")).ok_or_else(|| missing("axis"))?,
        angle_deg: t.get("angle_deg").and_then(Value::as_f64).ok_or_else(|| missing("angle_deg"))?,
        scale: t.get("scale").and_then(Value::as_f64).ok_or_else(|| missing("scale"))?,
    })
}

/// Python's truth of a JSON value (`if n.component_member`).
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Why RoboCAD refuses this edit of node `n` (api.py:575-578), if it does.
pub fn refusal(n: &NodeSummary, key: EditKey) -> Option<&'static str> {
    if truthy(n.component_member.as_ref()) {
        return Some("Edit component parameters or detach the occurrence first");
    }
    let transform = matches!(key, EditKey::Translation | EditKey::Axis | EditKey::Angle | EditKey::Scale);
    if transform && truthy(n.component_instance.as_ref()) {
        return Some("Use set_component_overrides with placement for a component occurrence");
    }
    None
}

/// Three numbers as typed: "10, 0, 2.5".
fn vector_text(v: [f64; 3]) -> String {
    v.map(num).join(", ")
}

/// The node's pivot when RoboCAD sent three finite numbers.
fn pivot_of(n: &NodeSummary) -> Option<[f64; 3]> {
    match n.pivot.as_deref() {
        Some([x, y, z, ..]) if [x, y, z].iter().all(|v| v.is_finite()) => Some([*x, *y, *z]),
        _ => None,
    }
}

/// The unrounded vector an editor types (None for the angle, the scale,
/// an unset pivot or an unreadable transform).
pub fn current_vector(n: &NodeSummary, key: EditKey) -> Option<[f64; 3]> {
    match key {
        EditKey::Pivot => pivot_of(n),
        EditKey::Translation => placement(n).ok().map(|p| p.translation),
        EditKey::Axis => placement(n).ok().map(|p| p.axis),
        EditKey::Angle | EditKey::Scale | EditKey::Tessellation => None,
    }
}

/// The text an editor opens with (empty for an unset pivot, and for the
/// tessellation tolerance, which RoboCAD does not report).
pub fn current_text(n: &NodeSummary, key: EditKey) -> String {
    match key {
        EditKey::Pivot => pivot_of(n).map(vector_text).unwrap_or_default(),
        EditKey::Tessellation => String::new(),
        other => match placement(n) {
            Ok(p) => match other {
                EditKey::Translation => vector_text(p.translation),
                EditKey::Axis => vector_text(p.axis),
                EditKey::Angle => num(p.angle_deg),
                _ => num(p.scale),
            },
            Err(_) => String::new(),
        },
    }
}

/// Three comma-separated expressions, each read by `eval`; errors name the
/// component. A component typed as `original`'s shown (rounded) text is
/// `original`'s unrounded value.
fn vector(text: &str, original: Option<[f64; 3]>, eval: impl Fn(&str) -> Result<f64, String>) -> Result<[f64; 3], String> {
    let parts: Vec<&str> = text.split(',').collect();
    let [x, y, z] = parts.as_slice() else { return Err(format!("type three values x, y, z separated by commas (got {})", parts.len())) };
    let one = |i: usize, name: &str, part: &str| match original {
        Some(o) if part == num(o[i]) => Ok(o[i]),
        _ => eval(part).map_err(|e| format!("{name}: {e}")),
    };
    Ok([one(0, "x", x.trim())?, one(1, "y", y.trim())?, one(2, "z", z.trim())?])
}

/// The typed text as the value sent (see the module doc); `original` is
/// the draft's unrounded vector (`EditDraft::original`).
pub fn evaluate(key: EditKey, text: &str, original: Option<[f64; 3]>) -> Result<Value, String> {
    use sim_runtime::units::evaluate as units;
    let length = |t: &str| units(t, false, Some("mm")).map_err(|e| e.to_string());
    let plain = |t: &str| units(t, false, None).map_err(|e| e.to_string());
    match key {
        EditKey::Pivot | EditKey::Translation => {
            if key == EditKey::Pivot && text.trim().is_empty() {
                return Err("type the pivot as x, y, z (mm), or press Clear pivot".into());
            }
            Ok(json!(vector(text, original, length)?))
        }
        EditKey::Axis => {
            let a = vector(text, original, plain)?;
            if a.iter().all(|c| c.abs() < 1e-12) {
                return Err("the axis must not be the zero vector".into());
            }
            Ok(json!(a))
        }
        EditKey::Angle => Ok(json!(units(text, true, None).map_err(|e| e.to_string())?)),
        EditKey::Scale => {
            let s = plain(text)?;
            if s <= 0.0 {
                return Err(format!("the scale must be a positive uniform factor (got {s})"));
            }
            Ok(json!(s))
        }
        EditKey::Tessellation => {
            if text.trim().is_empty() {
                return Err(format!("type a tolerance in mm ({}–{}; RoboCAD's default is {})", TOLERANCE_RANGE.0, TOLERANCE_RANGE.1, TOLERANCE_DEFAULT));
            }
            // RoboCAD's spin box keeps three decimals.
            let t = (length(text)? * 1000.0).round() / 1000.0;
            if !(TOLERANCE_RANGE.0..=TOLERANCE_RANGE.1).contains(&t) {
                return Err(format!("the tolerance must be {}–{} mm, as RoboCAD's inspector allows (got {t})", TOLERANCE_RANGE.0, TOLERANCE_RANGE.1));
            }
            Ok(json!(t))
        }
    }
}

/// The `CadPatch` an Enter writes: the pivot, or the whole transform with
/// the typed component (refused by name as RoboCAD refuses it). `original`
/// is the draft's unrounded vector (`EditDraft::original`).
pub fn patch_for(n: &NodeSummary, key: EditKey, text: &str, original: Option<[f64; 3]>) -> Result<CadAction, String> {
    if let Some(why) = refusal(n, key) {
        return Err(why.to_string());
    }
    let value = evaluate(key, text, original)?;
    if key == EditKey::Pivot {
        return Ok(patch(&n.id, "pivot", value));
    }
    if key == EditKey::Tessellation {
        return Ok(patch(&n.id, "tessellation_tolerance", value));
    }
    let mut p = placement(n)?;
    let number = value.as_f64();
    match key {
        EditKey::Translation => p.translation = vec3(Some(&value)).ok_or("translation: not three numbers")?,
        EditKey::Axis => p.axis = vec3(Some(&value)).ok_or("axis: not three numbers")?,
        EditKey::Angle => p.angle_deg = number.ok_or("angle: not a number")?,
        EditKey::Scale => p.scale = number.ok_or("scale: not a number")?,
        EditKey::Pivot | EditKey::Tessellation => {}
    }
    Ok(patch(&n.id, "transform", p.json()))
}

/// What the editors show, for the inspector's part key.
pub(super) fn key(doc: &CadDocument) -> String {
    format!("{:?}", (&doc.tool_state.inspector_edit, edit_blocked(doc)))
}

/// One editable value: its label, its field (the draft while typed), its
/// unit, and under it the draft's error or how to commit.
fn row(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, n: &NodeSummary, key: EditKey) {
    if let Some(why) = refusal(n, key) {
        let shown = current_text(n, key);
        field(p, k, key.label(), if shown.is_empty() { "not set" } else { shown.as_str() }, if shown.is_empty() { "" } else { key.unit() });
        p.spawn(k.caption(format!("RoboCAD refuses this edit: {why}.")));
        return;
    }
    let draft = doc.tool_state.inspector_edit.as_ref().filter(|d| d.node == n.id && d.key == key);
    let shown = draft.map_or_else(|| current_text(n, key), |d| d.text.clone());
    let placeholder = match key {
        EditKey::Pivot => "not set: x, y, z",
        EditKey::Tessellation => "not reported: type mm",
        _ => key.label(),
    };
    p.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|line| {
        line.spawn(k.text(key.label(), size::BODY, SUBTLE, 0));
        line.spawn(k.input(&shown, placeholder, EditField { node: n.id.clone(), key }, draft.is_some()));
        if !key.unit().is_empty() {
            line.spawn(k.text(key.unit(), size::SMALL, SUBTLE, 0));
        }
    });
    match draft {
        Some(EditDraft { error: Some(e), .. }) => {
            p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
        }
        Some(_) => {
            p.spawn(k.note("Enter sets it in RoboCAD (one undo step); Escape cancels."));
        }
        None => {}
    }
}

/// The pivot editor, and the transform editor for instances, reference
/// meshes and images (any other node's transform is shown as RoboCAD sent it).
pub(super) fn editors(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, n: &NodeSummary) {
    p.spawn(k.section("Pivot"));
    row(p, k, doc, n, EditKey::Pivot);
    if pivot_of(n).is_none() {
        p.spawn(k.caption("Not set: the transform tools turn and scale about the mass centroid, else the bounds centre."));
    } else if refusal(n, EditKey::Pivot).is_none() {
        let ready = edit_blocked(doc).is_none();
        p.spawn(wrap()).with_children(|r| {
            r.spawn(k.button("Clear pivot", CadButton(patch(&n.id, "pivot", Value::Null)), Look::Secondary, ready));
        });
    }
    if TESSELLATED_KINDS.contains(&n.kind.as_str()) {
        p.spawn(k.section("Tessellation"));
        row(p, k, doc, n, EditKey::Tessellation);
        p.spawn(k.caption(format!(
            "RoboCAD's default {TOLERANCE_DEFAULT} mm ({}–{} mm). RoboCAD does not report the current value; bodies are drawn at each node's own tolerance, as in RoboCAD's viewport.",
            TOLERANCE_RANGE.0, TOLERANCE_RANGE.1
        )));
    }
    p.spawn(k.section("Transform"));
    if !PLACED_KINDS.contains(&n.kind.as_str()) {
        super::lines(p, k, "transform", &n.transform);
        return;
    }
    if let Err(e) = placement(n) {
        p.spawn(k.text(e, size::SMALL, DANGER, 0));
        return;
    }
    for key in [EditKey::Translation, EditKey::Axis, EditKey::Angle, EditKey::Scale] {
        row(p, k, doc, n, key);
    }
}

/// The `CadPatch` an Enter of draft `d` writes: None when the value is
/// unchanged, else the edit; Err why nothing is sent (shown under the field).
fn enter(doc: &CadDocument, n: &NodeSummary, d: &EditDraft) -> Result<Option<CadAction>, String> {
    // The tolerance has no shown value (`current_text` is ""): an empty Enter is evaluated, so it names what to type.
    let unchanged = d.key != EditKey::Tessellation && d.text.trim() == current_text(n, d.key);
    let result = if unchanged { Ok(None) } else { patch_for(n, d.key, &d.text, d.original).map(Some) };
    // The transform is sent whole, its untouched components as read when
    // the draft opened: refused once RoboCAD's document has moved on since
    // (or the shown one is stale).
    let blocked = if matches!(d.key, EditKey::Pivot | EditKey::Tessellation) { edit_blocked(doc) } else { doc.commit_refusal(Some(d.began)) };
    result.and_then(|action| match blocked {
        Some(why) if action.is_some() => Err(format!("Not sent: {why}. Enter again once it clears, or Escape and reopen the field for the current values.")),
        _ => Ok(action),
    })
}

/// Input: the editors' drafts (see the module doc).
pub(in crate::cad) fn entry(
    doc: Option<ResMut<CadDocument>>,
    presses: Query<&EditField, With<crate::ui_kit::activation::Activated>>,
    mut msgs: MessageReader<FieldMsg>,
    mut text: TextFocus,
    mut out: MessageWriter<Act<CadAction>>,
    selection: CadSelection,
) {
    let Some(mut doc) = doc else {
        msgs.clear();
        return;
    };
    let before = doc.tool_state.inspector_edit.clone();
    let mut draft = before.clone();
    for m in msgs.read().filter(|m| m.field == EDITOR) {
        match &m.event {
            FieldEvent::Changed(t) => {
                if let Some(d) = draft.as_mut() {
                    d.text = t.text.clone();
                    d.select_all = t.select_all;
                    d.error = None;
                }
            }
            FieldEvent::Submit(typed) => {
                let Some(d) = draft.as_mut() else {
                    text.blur(EDITOR);
                    continue;
                };
                d.text = typed.clone();
                let Some(n) = node(&doc, &d.node) else {
                    draft = None;
                    text.blur(EDITOR);
                    continue;
                };
                match enter(&doc, n, d) {
                    Ok(action) => {
                        if let Some(action) = action {
                            out.write(Act::ui(action));
                        }
                        draft = None;
                        text.blur(EDITOR);
                    }
                    Err(why) => d.error = Some(why),
                }
            }
            FieldEvent::Cancel => draft = None,
            // A press elsewhere or another field's focus (unless this
            // system gave the field the keyboard again since).
            FieldEvent::Blur if !text.focused(EDITOR) => draft = None,
            FieldEvent::Blur | FieldEvent::Tab { .. } | FieldEvent::Arrow { .. } => {}
        }
    }
    let mut started = false;
    for f in &presses {
        // A press on the open editor keeps its draft.
        if draft.as_ref().is_some_and(|d| d.node == f.node && d.key == f.key) {
            continue;
        }
        let Some(n) = node(&doc, &f.node) else { continue };
        let opened = current_text(n, f.key);
        let original = current_vector(n, f.key);
        if text.focus_draft(EDITOR, TextDraft::new(opened.clone(), true)) {
            draft = Some(EditDraft { node: f.node.clone(), key: f.key, text: opened, select_all: true, error: None, began: doc.shown_revision(), original });
            started = true;
        }
    }
    // Another node selected drops the draft.
    if !started && let Some(d) = draft.as_ref() {
        let selected = selection.items().first_node().map(str::to_string);
        if selected.as_deref() != Some(d.node.as_str()) {
            draft = None;
            text.blur(EDITOR);
        }
    }
    // The kit's focus is the record: a draft whose field lost the keyboard
    // without a message read here (or one a handler cleared) ends.
    if draft.is_some() && !text.focused(EDITOR) {
        draft = None;
    }
    if draft.is_none() {
        text.blur(EDITOR);
    }
    if draft != before {
        doc.tool_state.inspector_edit = draft;
        // The panels refresh on the document's revision.
        doc.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance() -> NodeSummary {
        NodeSummary { id: "i1".into(), kind: "instance".into(), name: "Leg".into(), transform: json!({"translation": [1.0, 2.0, 3.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0}), ..Default::default() }
    }

    fn attrs(action: CadAction) -> (String, Value) {
        match action {
            CadAction::CadPatch { id, attrs } => (id, Value::Object(attrs)),
            other => panic!("not a patch: {other:?}"),
        }
    }

    /// One typed component sends the whole transform (RoboCAD's
    /// `Transform.from_json` defaults absent keys); units are RoboCAD's.
    #[test]
    fn a_typed_component_sends_the_whole_transform_and_the_pivot_alone() {
        let n = instance();
        let (id, a) = attrs(patch_for(&n, EditKey::Translation, "10, 1in, 0", None).unwrap());
        assert_eq!(id, "i1");
        assert_eq!(a, json!({"transform": {"translation": [10.0, 25.4, 0.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0}}));
        let (_, a) = attrs(patch_for(&n, EditKey::Angle, "0.5rad", None).unwrap());
        assert!((a["transform"]["angle_deg"].as_f64().unwrap() - 28.64788975654116).abs() < 1e-9, "{a}");
        assert_eq!(a["transform"]["translation"], json!([1.0, 2.0, 3.0]));
        let (_, a) = attrs(patch_for(&n, EditKey::Pivot, "0, 0, 5mm", None).unwrap());
        assert_eq!(a, json!({"pivot": [0.0, 0.0, 5.0]}));
        // Errors name the component; nothing is sent.
        assert!(patch_for(&n, EditKey::Translation, "1, 2", None).unwrap_err().contains("three values"));
        assert!(patch_for(&n, EditKey::Translation, "1, 2 qq, 3", None).unwrap_err().starts_with("y: "));
        assert!(patch_for(&n, EditKey::Axis, "0, 0, 0", None).unwrap_err().contains("zero vector"));
        assert!(patch_for(&n, EditKey::Scale, "0", None).unwrap_err().contains("positive"));
        assert!(patch_for(&n, EditKey::Pivot, " ", None).unwrap_err().contains("Clear pivot"));
        // A transform without one of its keys is not edited (nothing is filled in).
        let partial = NodeSummary { transform: json!({"translation": [0.0, 0.0, 0.0]}), ..instance() };
        assert!(patch_for(&partial, EditKey::Scale, "2", None).unwrap_err().contains("axis"));
    }

    /// RoboCAD's refusals (api.py:575-578): a component member's pivot and
    /// transform, a component occurrence's transform (its pivot is allowed).
    #[test]
    fn component_occurrences_are_refused_as_robocad_refuses_them() {
        let occurrence = NodeSummary { component_instance: Some(json!({"component": "c1"})), ..instance() };
        assert!(patch_for(&occurrence, EditKey::Translation, "0, 0, 0", None).unwrap_err().contains("set_component_overrides"));
        assert!(patch_for(&occurrence, EditKey::Pivot, "0, 0, 0", None).is_ok());
        let member = NodeSummary { component_member: Some(json!({"component": "c1"})), ..instance() };
        assert!(patch_for(&member, EditKey::Pivot, "0, 0, 0", None).unwrap_err().contains("detach the occurrence"));
        // Python's truth: an empty object is no flag.
        let empty = NodeSummary { component_member: Some(json!({})), ..instance() };
        assert!(refusal(&empty, EditKey::Scale).is_none());
        assert_eq!(current_text(&instance(), EditKey::Translation), "1, 2, 3");
    }

    /// The tessellation tolerance: RoboCAD's spin box range and decimals,
    /// one patch of the inspected node; refused for a component member only.
    #[test]
    fn the_tessellation_tolerance_is_one_patch_in_robocads_range() {
        let body = NodeSummary { id: "b1".into(), kind: "body".into(), name: "Bracket".into(), ..Default::default() };
        let (id, a) = attrs(patch_for(&body, EditKey::Tessellation, "0.02", None).unwrap());
        assert_eq!((id.as_str(), a), ("b1", json!({"tessellation_tolerance": 0.02})));
        let (_, a) = attrs(patch_for(&body, EditKey::Tessellation, "0.01234", None).unwrap());
        assert_eq!(a, json!({"tessellation_tolerance": 0.012}), "three decimals");
        let (_, a) = attrs(patch_for(&body, EditKey::Tessellation, "0.1cm", None).unwrap());
        assert_eq!(a, json!({"tessellation_tolerance": 1.0}), "units as RoboCAD's lengths");
        assert!(patch_for(&body, EditKey::Tessellation, "3", None).unwrap_err().contains("0.005–2"));
        assert!(patch_for(&body, EditKey::Tessellation, "0.001", None).unwrap_err().contains("0.005–2"));
        assert!(patch_for(&body, EditKey::Tessellation, " ", None).unwrap_err().contains("0.05"));
        assert_eq!(current_text(&body, EditKey::Tessellation), "", "RoboCAD does not report it");
        let occurrence = NodeSummary { component_instance: Some(json!({"component": "c1"})), ..instance() };
        assert!(patch_for(&occurrence, EditKey::Tessellation, "0.1", None).is_ok(), "only the transform is refused for an occurrence");
        let member = NodeSummary { component_member: Some(json!({"component": "c1"})), ..instance() };
        assert!(patch_for(&member, EditKey::Tessellation, "0.1", None).unwrap_err().contains("detach the occurrence"));
    }

    /// The field shows 1e-6-rounded values; a component left as shown is
    /// sent unrounded, a retyped one as typed.
    #[test]
    fn untouched_vector_components_keep_their_full_precision() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let n = NodeSummary { transform: json!({"translation": [1.0, 2.0, 3.0], "axis": [h, 0.0, h], "angle_deg": 0.0, "scale": 1.0}), ..instance() };
        let original = current_vector(&n, EditKey::Axis);
        assert_eq!(original, Some([h, 0.0, h]));
        assert_eq!(current_text(&n, EditKey::Axis), "0.707107, 0, 0.707107");
        let (_, a) = attrs(patch_for(&n, EditKey::Axis, "0.707107, 1, 0.707107", original).unwrap());
        assert_eq!(a["transform"]["axis"], json!([h, 1.0, h]));
        // A retyped component is the typed value, even when close.
        let (_, a) = attrs(patch_for(&n, EditKey::Axis, "0.70711, 0, 0.707107", original).unwrap());
        assert_eq!(a["transform"]["axis"], json!([0.70711, 0.0, h]));
        // Without the draft's original the shown text is all there is.
        let (_, a) = attrs(patch_for(&n, EditKey::Axis, "0.707107, 1, 0.707107", None).unwrap());
        assert_eq!(a["transform"]["axis"], json!([0.707107, 1.0, 0.707107]));
        assert_eq!(current_vector(&n, EditKey::Angle), None);
        assert_eq!(current_vector(&instance(), EditKey::Pivot), None);
    }
}
