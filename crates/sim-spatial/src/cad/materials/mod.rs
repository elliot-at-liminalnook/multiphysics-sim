//! RoboCAD's Materials panel (ui/widgets.py:741-815) as a section of CAD
//! mode's right dock (cad-physical-inspect):
//!
//! - **List**: one row per document material, "■ name   density g/cm³"
//!   with ■ in the material's colour (`/doc`'s materials, read with
//!   `Material::of`), filtered by "Search materials…" (a name containing
//!   the text, case-insensitive, or a tag containing it, as RoboCAD's
//!   `refresh`). A click makes a row current; a double-click applies it.
//! - **Apply to selection** (and the double-click): one `POST
//!   /ops/set_material(ids, material_id)` over the selected nodes (the
//!   shared selection's CAD items), one undo step "Material"; refused by
//!   name when nothing is selected (RoboCAD silently does nothing).
//! - **New…** and **Material properties…**: RoboCAD's modal dialogs as kit
//!   forms ([`form`]): one `POST /materials`, or one
//!   `set_material_props(id, changed keys)`.
//! - **Drag onto a body** (ui/app.py:1801-1816) is deliberately not
//!   offered: it would need a second pointer path (a drag held across the
//!   dock into the 3D view and its own ray cast at release) beside the one
//!   pick path (`pick`); Apply to selection and the double-click make the
//!   same single `set_material` edit.
//!
//! Every edit goes through `actions::edit_at` (refused by name while one
//! is in flight; the properties form with the revision its values were
//! read at). Typing and the forms' clicks are [`panel`]'s.
mod form;
pub(in crate::cad) mod panel;
#[cfg(test)]
mod tests;

use crate::app::actions::{Call, Spec, spec};
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::{CAD, CadAction, Cx, edit_at};
use crate::cad::document::{CadDocument, EditDone};
use crate::cad::inspector::py_g;
use crate::cad::selection::CadItems;
use crate::cad::sync::value;
use bevy::prelude::*;
use form::{FormKind, MaterialForm, Submit};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use crate::cad::types::{Material, SelectionItem};

pub(in crate::cad) use panel::{draw, key};

/// Which of the panel's fields has the keyboard (mirrored from the kit's
/// focus on `panel::SEARCH` and `panel::FORM`; a dialog row is the one the
/// form field edits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Focus {
    /// "Search materials…".
    Search,
    /// The open form's field `i`.
    Field(usize),
}

/// The panel's state on the document (reset with it): the search text, the
/// current row, the open dialog and the field typed into.
#[derive(Default)]
pub struct MaterialsState {
    /// Monotonic local modal lifetime; drafts and focus do not change it.
    pub(crate) form_sequence: u64,
    pub(crate) search: String,
    /// The current material (RoboCAD's list `currentItem`).
    pub(crate) current: Option<String>,
    pub(crate) form: Option<MaterialForm>,
    /// The field with the keyboard, as [`panel`]'s input mirrors the kit's
    /// focus (`cad_state.materials.typing` and the drawing read it); the
    /// handler clears it when it closes the dialog, and the input then
    /// takes the kit's focus away.
    pub(crate) focus: Option<Focus>,
    /// The focused field's text is selected (the kit's draft, mirrored).
    pub(crate) select_all: bool,
    /// A dialog opened since [`panel`]'s input last ran: the input gives
    /// its first field the keyboard (the handler cannot reach the kit's focus).
    pub(crate) claimed: bool,
}

impl MaterialsState {
    /// The dialog closed: its field no longer has the keyboard.
    fn closed(&mut self) {
        self.focus = None;
        self.select_all = false;
    }
}

/// What `cad_materials` does.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MaterialsOp {
    /// Filter the list (`text`; empty shows every material).
    #[default]
    Search,
    /// Make `material` the current row.
    Select,
    /// `set_material(ids, material)`: the selected nodes (or `ids`), the
    /// current material (or `material`).
    Apply,
    /// Open RoboCAD's "New material" dialog.
    New,
    /// Open "{name}: engineering properties" for `material` (or the current one).
    Properties,
    /// Set the open dialog's field `field` to `text`.
    FormSet,
    /// The open dialog's OK.
    FormSubmit,
    /// The open dialog's Cancel.
    FormCancel,
}

/// `cad_materials`' arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct MaterialsArgs {
    #[serde(default)]
    pub op: MaterialsOp,
    /// The search text (search) or the field's text (form_set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A material id (select, apply, properties).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// The dialog's field (form_set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// The nodes (apply; the selected nodes when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
    /// RoboCAD's revision the nodes were read at (apply).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

impl MaterialsArgs {
    pub(crate) fn of(op: MaterialsOp, material: Option<&str>) -> CadAction {
        CadAction::CadMaterials(MaterialsArgs { op, material: material.map(str::to_string), ..MaterialsArgs::default() })
    }
}

/// The document's materials as `/doc` lists them (a malformed one dropped).
pub(crate) fn list(doc: &CadDocument) -> Vec<Material> {
    doc.doc.as_ref().map(|d| d.materials.iter().filter_map(Material::of).collect()).unwrap_or_default()
}

/// RoboCAD's filter (`refresh`): the name contains the text (lower-cased)
/// or a tag does.
pub(crate) fn matches(m: &Material, search: &str) -> bool {
    let t = search.to_lowercase();
    t.is_empty() || m.name.to_lowercase().contains(&t) || m.tags.iter().any(|tag| tag.contains(&t))
}

/// A row's text, RoboCAD's "■ name   density g/cm³".
pub(crate) fn row_label(m: &Material) -> String {
    format!("■ {}   {} g/cm³", m.name, py_g(m.density, 6))
}

fn find(doc: &CadDocument, id: &str) -> Option<Material> {
    list(doc).into_iter().find(|m| m.id == id)
}

/// The material an op names (`material`, else the current row).
fn named(doc: &CadDocument, args: &MaterialsArgs) -> Result<Material, String> {
    let id = args.material.clone().or_else(|| doc.materials.current.clone()).ok_or("choose a material in the list first")?;
    find(doc, &id).ok_or_else(|| format!("no material {id} in RoboCAD's list"))
}

/// `CadMaterials`, from any entry point.
pub(in crate::cad) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let CadAction::CadMaterials(args) = action else { return Outcome::Done(Err("not a materials action".into())) };
    let selection = cx.shared.items();
    let doc = &mut *cx.doc;
    let done = Outcome::Done;
    match args.op {
        MaterialsOp::Search => {
            let text = args.text.clone().unwrap_or_default();
            if doc.materials.search != text {
                doc.materials.search = text;
                doc.touch();
            }
            let shown: Vec<String> = list(doc).into_iter().filter(|m| matches(m, &doc.materials.search)).map(|m| m.id).collect();
            done(Ok(json!({"search": doc.materials.search, "shown": shown})))
        }
        MaterialsOp::Select => match named(doc, args) {
            Ok(m) => {
                doc.materials.current = Some(m.id.clone());
                doc.touch();
                done(Ok(json!({"current": m.id})))
            }
            Err(e) => done(Err(e)),
        },
        MaterialsOp::Apply => apply(doc, call, args, &selection),
        MaterialsOp::New => {
            let form = form::new_form(doc.shown_revision());
            done(Ok(open(doc, form)))
        }
        MaterialsOp::Properties => match named(doc, args) {
            Ok(m) => {
                doc.materials.current = Some(m.id.clone());
                let form = form::properties_form(doc, &m);
                done(Ok(open(doc, form)))
            }
            Err(e) => done(Err(e)),
        },
        MaterialsOp::FormSet => {
            let Some(f) = doc.materials.form.as_mut() else { return done(Err("no material dialog is open".into())) };
            let (Some(field), Some(text)) = (args.field.as_deref(), args.text.as_deref()) else { return done(Err("form_set needs field and text".into())) };
            let r = form::set(f, field, text);
            doc.touch();
            done(r.map(|()| form_json(doc)))
        }
        MaterialsOp::FormSubmit => submit(doc, call),
        MaterialsOp::FormCancel => {
            if doc.materials.form.take().is_none() {
                return done(Err("no material dialog is open".into()));
            }
            doc.materials.closed();
            doc.touch();
            done(Ok(json!({"message": "Closed the dialog; nothing was sent."})))
        }
    }
}

/// Open a dialog with its first field focused (RoboCAD's dialog focus):
/// [`panel`]'s input gives it the kit's keyboard (`claimed`), which takes
/// it from whichever field had it.
fn open(doc: &mut CadDocument, form: MaterialForm) -> Value {
    // A numeric entry requested this frame would take the dialog's keyboard.
    doc.tool_state.numeric.focus_request = false;
    doc.materials.form_sequence = doc.materials.form_sequence.wrapping_add(1);
    doc.materials.form = Some(form);
    doc.materials.focus = Some(Focus::Field(0));
    doc.materials.select_all = true;
    doc.materials.claimed = true;
    doc.touch();
    form_json(doc)
}

/// Whether an open dialog reads RoboCAD's physical model now: a material
/// properties dialog whose values were read at the shown revision (its
/// defaults come from that model; `inspector::refresh` fetches it).
pub(in crate::cad) fn wants_physical(doc: &CadDocument) -> bool {
    doc.materials.form.as_ref().is_some_and(|f| matches!(f.kind, FormKind::Properties { .. }) && f.began == doc.shown_revision())
}

/// The open properties dialog with the defaults RoboCAD's physical model
/// now reports at the revision it was opened at (typed fields kept), or
/// None when that changes nothing (`inspector::refresh` stores it).
pub(in crate::cad) fn refilled_form(doc: &CadDocument) -> Option<MaterialForm> {
    let f = doc.materials.form.as_ref()?;
    let FormKind::Properties { id, .. } = &f.kind else { return None };
    let landed = doc.physical.as_ref().is_some_and(|(r, res)| *r == f.began && res.is_ok());
    if f.began != doc.shown_revision() || !landed {
        return None;
    }
    let m = find(doc, id)?;
    form::refill(f, form::properties_form(doc, &m))
}

/// Apply to selection: one `set_material` over the nodes.
fn apply(doc: &mut CadDocument, call: &mut Call, args: &MaterialsArgs, selection: &[SelectionItem]) -> Outcome {
    let m = match named(doc, args) {
        Ok(m) => m,
        Err(e) => return Outcome::Done(Err(e)),
    };
    let ids = args.ids.clone().unwrap_or_else(|| selection.nodes());
    if ids.is_empty() {
        return Outcome::Done(Err(format!("Nothing selected: select the bodies to give {}, then apply it", m.name)));
    }
    if let Some(missing) = ids.iter().find(|id| !doc.has_node(id)) {
        return Outcome::Done(Err(format!("no node {missing} in the shown tree")));
    }
    let began = Some(args.revision.unwrap_or_else(|| doc.shown_revision()));
    if doc.materials.current.as_deref() != Some(m.id.as_str()) {
        doc.materials.current = Some(m.id.clone());
        doc.touch();
    }
    let (name, id, n) = (m.name, m.id, ids.len());
    edit_at(doc, call, began, format!("Material {name}"), move |c| c.set_material(&ids, &id).map(|r| EditDone { message: format!("Set material {name} on {n} node(s)"), result: value(&r) }))
}

/// The open dialog's OK: its edit, or why nothing is sent (kept in the form).
fn submit(doc: &mut CadDocument, call: &mut Call) -> Outcome {
    let Some(form) = doc.materials.form.clone() else { return Outcome::Done(Err("no material dialog is open".into())) };
    let outcome = match form::submit(&form) {
        Err(e) => Outcome::Done(Err(e)),
        Ok(Submit::Nothing) => {
            doc.show(Ok("No property was changed; nothing was sent.".into()));
            Outcome::Done(Ok(json!({"message": "No property was changed; nothing was sent."})))
        }
        // A new material reads nothing from the document: only an edit in flight or no connection refuses it.
        Ok(Submit::New(m)) => {
            let name = m.name.clone();
            edit_at(doc, call, None, format!("New material {name}"), move |c| c.add_material(&m).map(|r| EditDone { message: format!("Added material {} ({})", r.name, r.id), result: value(&r) }))
        }
        Ok(Submit::Props { id, name, props }) => {
            let keys = props.keys().cloned().collect::<Vec<_>>().join(", ");
            let message = format!("Set {name}'s engineering properties: {keys}");
            edit_at(doc, call, Some(form.began), format!("Material properties of {name}"), move |c| c.set_material_props(&id, &props).map(|r| EditDone { message, result: value(&r) }))
        }
    };
    match &outcome {
        Outcome::Done(Err(e)) => {
            if let Some(f) = doc.materials.form.as_mut() {
                f.error = Some(e.clone());
            }
        }
        _ => {
            doc.materials.form = None;
            doc.materials.closed();
        }
    }
    doc.touch();
    outcome
}

/// The open dialog as `cad_state.materials.form` shows it.
fn form_json(doc: &CadDocument) -> Value {
    let Some(f) = &doc.materials.form else { return Value::Null };
    let (kind, material) = match &f.kind {
        FormKind::New => ("new", None),
        FormKind::Properties { id, .. } => ("properties", Some(id)),
    };
    let fields: Vec<Value> = f.fields.iter().zip(&f.texts).map(|(field, text)| json!({"key": field.key, "label": field.label, "text": text, "origin": field.origin.name(), "optional": field.optional()})).collect();
    json!({"kind": kind, "material": material, "fields": fields, "error": f.error, "began": f.began, "ok_ready": form::ok_ready(f).err()})
}

/// `cad_state.materials`.
pub(in crate::cad) fn state_json(doc: &CadDocument, selection: &[SelectionItem]) -> Value {
    let st = &doc.materials;
    let materials: Vec<Value> = list(doc).iter().map(|m| json!({"id": m.id, "name": m.name, "density_g_cm3": m.density, "color": m.color, "tags": m.tags, "label": row_label(m), "shown": matches(m, &st.search), "engineering": m.engineering})).collect();
    json!({
        "search": st.search,
        "current": st.current,
        "materials": materials,
        "selected_nodes": selection.nodes(),
        "form": form_json(doc),
        "typing": st.focus.map(|f| format!("{f:?}")),
    })
}

/// `system_ui`'s materials controls; the panel's buttons write the same actions.
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    controls_of(cx.doc, &cx.shared.items())
}

/// The controls for `doc` and the selection: (id, label, action, ready).
pub(crate) fn controls_of(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let edit = doc.edit_refusal().map_or(Ok(()), Err);
    let current = doc.materials.current.as_deref().and_then(|id| find(doc, id));
    let choose = || Err::<(), String>("choose a material in the list first".into());
    let apply = match &current {
        None => choose(),
        Some(m) if selection.is_empty() => Err(format!("Nothing selected: select the bodies to give {}", m.name)),
        Some(_) => edit.clone(),
    };
    let id = current.as_ref().map(|m| m.id.as_str());
    let mut out = vec![
        ("cad:materials:apply".to_string(), "Apply to selection".to_string(), MaterialsArgs::of(MaterialsOp::Apply, id), apply),
        ("cad:materials:new".to_string(), "New…".to_string(), MaterialsArgs::of(MaterialsOp::New, None), Ok(())),
        ("cad:materials:properties".to_string(), "Material properties…".to_string(), MaterialsArgs::of(MaterialsOp::Properties, id), current.as_ref().map_or_else(choose, |_| Ok(()))),
    ];
    for m in list(doc).iter().filter(|m| matches(m, &doc.materials.search)) {
        out.push((format!("cad:materials:row-{}", m.id.replace(':', "_")), row_label(m), MaterialsArgs::of(MaterialsOp::Select, Some(m.id.as_str())), Ok(())));
    }
    if let Some(f) = &doc.materials.form {
        let ok = form::ok_ready(f).and_then(|()| edit.clone());
        out.push(("cad:materials:form-ok".into(), "OK".into(), MaterialsArgs::of(MaterialsOp::FormSubmit, None), ok));
        out.push(("cad:materials:form-cancel".into(), "Cancel".into(), MaterialsArgs::of(MaterialsOp::FormCancel, None), Ok(())));
    }
    out
}

/// This part's REST command.
pub(in crate::cad) fn specs() -> Vec<Spec> {
    vec![spec(
        "cad_materials",
        CAD,
        json!({"op": "apply", "material": "petg"}),
        "CAD mode: RoboCAD's Materials panel (cad_state.materials: each material with its \"■ name density g/cm³\" label, colour, tags and engineering overrides). op: search (text: a name or tag fragment; empty shows all), select (material: the current row), apply (material? default the current row, ids? default the selected nodes, revision? the revision they were read at: one POST /ops/set_material, undo step \"Material\"; refused when nothing is selected), new (opens RoboCAD's \"New material\" dialog: fields name, density g/cm³ 0.01–25), properties (material? default the current row: \"{name}: engineering properties\", fields youngs_modulus GPa, poisson, yield_strength MPa, ultimate_strength MPa, glass_transition_c °C, thermal_conductivity W/m·K, specific_heat J/kg·K, thermal_expansion 1/K, bearing_pressure MPa, friction_self, friction_steel, anisotropy_z; each shows the document's override, else RoboCAD's default as its physical model reports it), form_set (field, text), form_submit (new: one POST /materials; properties: one POST /ops/set_material_props with only the changed keys, in SI), form_cancel. system_ui lists cad:materials:<id>.",
    )]
}

/// CadPlugin: the panel's fields, its typing and clicks (Input), and the
/// dialog (Present).
pub(in crate::cad) fn build(app: &mut App) {
    use crate::ui_kit::text::TextFieldApp;
    app.add_text_field(panel::SEARCH, panel::search_field()).add_text_field(panel::FORM, panel::form_field()).add_systems(
        Update,
        panel::input
            .in_set(crate::cad::CadKeySet::Focus)
            .run_if(in_state(ViewerMode::Cad)),
    )
    .add_systems(Update, (panel::scroll_form, panel::draw_form).chain().in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}
