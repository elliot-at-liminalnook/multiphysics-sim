//! The materials panel's two dialogs as kit forms (RoboCAD's modal
//! `QDialog`s): "New material" (ui/widgets.py:795-815: "Name" default
//! "Custom", "Density (g/cm³)" 0.01–25, three decimals, default 1.2) and
//! "{name}: engineering properties" (widgets.py:681-713).
//!
//! **Where the engineering values come from.** RoboCAD's dialog shows
//! `Material.props()`: the document's overrides (`engineering`) over
//! RoboCAD's defaults (`default_engineering`, with the print registry's
//! values for filaments). `/doc` carries only the overrides, and RoboCAD
//! reports the filled-in values only in its physical model (`GET
//! /physical`'s `materials`, `physical.material_block`, for the materials
//! the model uses). So each field shows the override when the document
//! sets one ("set in this document"), else the physical model's value at
//! the shown revision ("RoboCAD's default"), else nothing ("not
//! reported"); no default is copied into this window. Each field says
//! which. The model is fetched for the shown revision while the dialog is
//! open (`inspector::refresh`), and the dialog takes its defaults when it
//! lands ([`refill`]), keeping what was typed.
//!
//! **What OK sends.** One `set_material_props(id, …)` with only the
//! fields whose text changed, in RoboCAD's SI units (the dialog's GPa and
//! MPa divided back). RoboCAD's dialog re-sends every value; sending the
//! changed ones is the same edit on the overrides, except that untouched
//! defaults stay defaults instead of becoming overrides. Friction is sent
//! as RoboCAD builds it (self and world from "vs itself", steel from "vs
//! steel", static = 1.2 × kinetic) when either friction field changed; the
//! print anisotropy only for a printed material, merged into its print
//! block, as RoboCAD's `{**props["print"], "anisotropy_z": …}`.
use crate::cad::document::CadDocument;
use crate::cad::inspector::py_g;
use crate::ui_kit::form::{FieldKind, FieldValue, Unit, evaluate};
use serde_json::{Map, Value, json};
use sim_runtime::cad_client::{Material, NewMaterial};

/// RoboCAD's engineering rows (widgets.py:687): key, label, the dialog's
/// scale from SI.
pub(crate) const PROPS: [(&str, &str, f64); 9] = [
    ("youngs_modulus", "Young's modulus (GPa)", 1e-9),
    ("poisson", "Poisson ratio", 1.0),
    ("yield_strength", "Yield strength (MPa)", 1e-6),
    ("ultimate_strength", "Ultimate strength (MPa)", 1e-6),
    ("glass_transition_c", "Glass transition (°C)", 1.0),
    ("thermal_conductivity", "Thermal conductivity (W/m·K)", 1.0),
    ("specific_heat", "Specific heat (J/kg·K)", 1.0),
    ("thermal_expansion", "Thermal expansion (1/K)", 1.0),
    ("bearing_pressure", "Allowable bearing pressure (MPa)", 1e-6),
];
/// The friction and print rows' keys (this form's names for them).
pub(crate) const FRICTION_SELF: &str = "friction_self";
pub(crate) const FRICTION_STEEL: &str = "friction_steel";
pub(crate) const ANISOTROPY: &str = "anisotropy_z";

/// A number as RoboCAD's dialog reads it (`float(evaluate(text))`; no unit).
const NUMBER: FieldKind = FieldKind::Number { unit: Unit::Plain, min: None, max: None, decimals: 4 };
/// RoboCAD's density spin box: 0.01–25, three decimals.
const DENSITY: FieldKind = FieldKind::Number { unit: Unit::Plain, min: Some(0.01), max: Some(25.0), decimals: 3 };

/// Where a field's opening value came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// A New… field (RoboCAD's dialog default).
    Dialog,
    /// The document's override (`Material.engineering`).
    Set,
    /// RoboCAD's default, as its physical model reports it.
    Default,
    /// Neither: RoboCAD has not reported it here.
    Unreported,
    /// The material has no print block (RoboCAD sends no anisotropy for it).
    NotPrinted,
}

impl Origin {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Origin::Dialog => "dialog default",
            Origin::Set => "set in this document",
            Origin::Default => "RoboCAD's default",
            Origin::Unreported => "not reported",
            Origin::NotPrinted => "not a printed material",
        }
    }
}

/// One field: its key, label (with where its value came from), kind and origin.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FormField {
    pub key: &'static str,
    pub label: String,
    pub kind: FieldKind,
    pub origin: Origin,
}

impl FormField {
    /// May be left empty (nothing to show, nothing sent).
    pub(crate) fn optional(&self) -> bool {
        matches!(self.origin, Origin::Unreported | Origin::NotPrinted)
    }
}

/// Which dialog.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FormKind {
    New,
    Properties { id: String, name: String },
}

/// What RoboCAD's `props()["print"]` is for the material being edited.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Print {
    /// The document overrides it: the anisotropy merges into this block.
    Override(Map<String, Value>),
    /// A default print block (the physical model reports one).
    Default,
    /// None: not a printed material.
    NotPrinted,
    /// Not reported here.
    Unknown,
}

/// An open dialog: its fields, their texts (typed) and as opened, the
/// opening values unrounded (the dialog's units), the last OK's refusal and
/// RoboCAD's revision the values were read at.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MaterialForm {
    pub kind: FormKind,
    pub fields: Vec<FormField>,
    pub texts: Vec<String>,
    pub opened: Vec<String>,
    pub values: Vec<Option<f64>>,
    pub error: Option<String>,
    pub began: u64,
    pub print: Print,
}

/// RoboCAD's "New material" dialog.
pub(crate) fn new_form(began: u64) -> MaterialForm {
    let fields = vec![FormField { key: "name", label: "Name".into(), kind: FieldKind::Text, origin: Origin::Dialog }, FormField { key: "density", label: "Density (g/cm³)".into(), kind: DENSITY, origin: Origin::Dialog }];
    let texts = vec!["Custom".to_string(), "1.2".to_string()];
    MaterialForm { kind: FormKind::New, fields, opened: texts.clone(), texts, values: vec![None, Some(1.2)], error: None, began, print: Print::Unknown }
}

/// Material `id`'s block in RoboCAD's physical model at the shown revision.
fn block<'a>(doc: &'a CadDocument, id: &str) -> Option<&'a Map<String, Value>> {
    let (revision, result) = doc.physical.as_ref()?;
    if Some(*revision) != doc.doc_key.as_ref().map(|k| k.1) {
        return None;
    }
    result.as_ref().ok()?.get("materials")?.get(id)?.as_object()
}

fn at<'a>(v: Option<&'a Value>, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(v?, |v, k| v.get(*k))
}

/// RoboCAD's "{name}: engineering properties" dialog for material `m`.
pub(crate) fn properties_form(doc: &CadDocument, m: &Material) -> MaterialForm {
    let eng = Value::Object(m.engineering.clone());
    let physical = block(doc, &m.id).map(|b| Value::Object(b.clone()));
    let mut fields = Vec::new();
    let mut values = Vec::new();
    let mut texts = Vec::new();
    let mut push = |key: &'static str, label: &str, value: Option<f64>, origin: Origin, digits: usize| {
        fields.push(FormField { key, label: format!("{label} · {}", origin.name()), kind: NUMBER, origin });
        texts.push(value.map(|v| py_g(v, digits)).unwrap_or_default());
        values.push(value);
    };
    let pick = |own: Option<&Value>, default: Option<&Value>| match (own.and_then(Value::as_f64), default.and_then(Value::as_f64)) {
        (Some(v), _) => (Some(v), Origin::Set),
        (None, Some(v)) => (Some(v), Origin::Default),
        (None, None) => (None, Origin::Unreported),
    };
    for (key, label, scale) in PROPS {
        let (v, origin) = pick(at(Some(&eng), &[key]), at(physical.as_ref(), &[key]));
        push(key, label, v.map(|v| v * scale), origin, 4);
    }
    let (v, origin) = pick(at(Some(&eng), &["friction", "self", "kinetic"]), at(physical.as_ref(), &["friction", m.id.as_str(), "kinetic"]));
    push(FRICTION_SELF, "Kinetic friction vs itself", v, origin, 3);
    let (v, origin) = pick(at(Some(&eng), &["friction", "steel", "kinetic"]), at(physical.as_ref(), &["friction", "steel", "kinetic"]));
    push(FRICTION_STEEL, "Kinetic friction vs steel", v, origin, 3);
    let print = match (eng.get("print").and_then(Value::as_object), physical.as_ref().map(|p| p.get("print"))) {
        (Some(own), _) => Print::Override(own.clone()),
        (None, Some(Some(Value::Object(_)))) => Print::Default,
        (None, Some(None | Some(Value::Null))) => Print::NotPrinted,
        _ => Print::Unknown,
    };
    let (v, origin) = match pick(at(Some(&eng), &["print", "anisotropy_z"]), at(physical.as_ref(), &["print", "anisotropy_z"])) {
        (None, _) if print == Print::NotPrinted => (None, Origin::NotPrinted),
        other => other,
    };
    push(ANISOTROPY, "Print anisotropy across layers (E ratio)", v, origin, 3);
    MaterialForm { kind: FormKind::Properties { id: m.id.clone(), name: m.name.clone() }, fields, opened: texts.clone(), texts, values, error: None, began: doc.shown_revision(), print }
}

/// `old` reopened as `fresh` (the same dialog read again at the same
/// revision, once RoboCAD's physical model arrived): fresh fields, origins
/// and opening values, with every field typed into keeping its text, and
/// the last refusal. None when nothing the dialog opened with changed.
pub(crate) fn refill(old: &MaterialForm, mut fresh: MaterialForm) -> Option<MaterialForm> {
    if fresh.kind != old.kind || fresh.fields.len() != old.fields.len() || fresh.texts.len() != old.texts.len() {
        return None;
    }
    if fresh.fields == old.fields && fresh.opened == old.opened && fresh.values == old.values && fresh.print == old.print {
        return None;
    }
    for (i, (text, opened)) in old.texts.iter().zip(&old.opened).enumerate() {
        if text.trim() != opened.trim() {
            fresh.texts[i] = text.clone();
        }
    }
    fresh.error = old.error.clone();
    fresh.began = old.began;
    Some(fresh)
}

/// Whether OK can be pressed: every filled field reads as its kind.
pub(crate) fn ok_ready(form: &MaterialForm) -> Result<(), String> {
    for (f, text) in form.fields.iter().zip(&form.texts) {
        if f.optional() && text.trim().is_empty() {
            continue;
        }
        evaluate(&f.kind, text).map_err(|e| format!("{}: {e}", f.label))?;
    }
    Ok(())
}

/// Set field `key`'s text (`cad_materials {op: form_set}`).
pub(crate) fn set(form: &mut MaterialForm, key: &str, text: &str) -> Result<(), String> {
    let keys: Vec<&str> = form.fields.iter().map(|f| f.key).collect();
    let i = keys.iter().position(|k| *k == key).ok_or_else(|| format!("no field {key} in this form; its fields are {}", keys.join(", ")))?;
    form.texts[i] = text.to_string();
    form.error = None;
    Ok(())
}

/// What OK sends.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Submit {
    /// `POST /materials`.
    New(NewMaterial),
    /// `set_material_props(id, props)`.
    Props { id: String, name: String, props: Map<String, Value> },
    /// Nothing changed.
    Nothing,
}

fn number(f: &FormField, text: &str) -> Result<f64, String> {
    match evaluate(&f.kind, text) {
        Ok(FieldValue::Number(v)) => Ok(v),
        Ok(_) => Err(format!("{}: not a number", f.label)),
        Err(e) => Err(format!("{}: {e}", f.label)),
    }
}

/// OK's edit, or why it sends nothing (shown in the form).
pub(crate) fn submit(form: &MaterialForm) -> Result<Submit, String> {
    let FormKind::Properties { id, name } = &form.kind else {
        let name = form.texts.first().cloned().unwrap_or_default();
        let density = number(&form.fields[1], form.texts.get(1).map_or("", String::as_str))?;
        // RoboCAD's spin box keeps three decimals; its id is the name lower-cased, spaces as "_" (else "custom").
        let density = (density * 1000.0).round() / 1000.0;
        let id = Some(name.to_lowercase().replace(' ', "_")).filter(|s| !s.is_empty()).unwrap_or_else(|| "custom".into());
        return Ok(Submit::New(NewMaterial { id: Some(id), name, density, color: None, roughness: None, metallic: None, tags: None }));
    };
    let index = |key: &str| form.fields.iter().position(|f| f.key == key);
    let changed = |i: usize| form.texts[i].trim() != form.opened[i].trim();
    // A changed field's typed value, else the value it opened with.
    let value = |i: usize| -> Result<Option<f64>, String> {
        if !changed(i) {
            return Ok(form.values[i]);
        }
        if form.texts[i].trim().is_empty() {
            return Err(format!("{}: type a value (an emptied field is not sent)", form.fields[i].label));
        }
        number(&form.fields[i], &form.texts[i]).map(Some)
    };
    let mut props = Map::new();
    for (key, _, scale) in PROPS {
        if let Some(i) = index(key)
            && changed(i)
            && let Some(v) = value(i)?
        {
            props.insert(key.into(), json!(v / scale));
        }
    }
    if let (Some(s), Some(t)) = (index(FRICTION_SELF), index(FRICTION_STEEL))
        && (changed(s) || changed(t))
    {
        let need = |i: usize| value(i).and_then(|v| v.ok_or_else(|| format!("{}: RoboCAD has not reported it here; type it too (friction is sent as one table)", form.fields[i].label)));
        let (mu_self, mu_steel) = (need(s)?, need(t)?);
        props.insert("friction".into(), json!({"self": {"static": mu_self * 1.2, "kinetic": mu_self}, "world": {"static": mu_self * 1.2, "kinetic": mu_self}, "steel": {"static": mu_steel * 1.2, "kinetic": mu_steel}}));
    }
    if let Some(a) = index(ANISOTROPY)
        && changed(a)
        && let Some(v) = value(a)?
    {
        let block = match &form.print {
            Print::Override(own) => {
                let mut b = own.clone();
                b.insert("anisotropy_z".into(), json!(v));
                b
            }
            Print::Default => Map::from_iter([("anisotropy_z".to_string(), json!(v))]),
            Print::NotPrinted => return Err(format!("{name} is not a printed material: RoboCAD keeps no print anisotropy for it")),
            Print::Unknown => return Err(format!("RoboCAD has not reported whether {name} is printed: its physical model (fetched while this dialog is open) does not list it yet, so its anisotropy cannot be edited here")),
        };
        props.insert("print".into(), Value::Object(block));
    }
    if props.is_empty() {
        return Ok(Submit::Nothing);
    }
    Ok(Submit::Props { id: id.clone(), name: name.clone(), props })
}
