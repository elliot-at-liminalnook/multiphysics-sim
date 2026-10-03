//! Catalogue-backed typed forms. Arithmetic expressions are kept verbatim:
//! only RoboCAD's dimensional parameter grammar evaluates them. No geometry
//! or physical default is inferred here.
use super::{ComponentsState, jobs::Identity};
use crate::cad::CadDocument;
use crate::ui_kit::form::{FieldKind, FieldValue, Unit, evaluate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_runtime::cad_client::{
    ComponentDefinition, ComponentNested, ComponentOperation, ComponentParameter, ComponentVariant,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComponentsFormKind {
    Make,
    Create,
    Parametric,
    Place,
    Defaults,
    Overrides,
    Reset,
    Detach,
    Transform,
    Import,
    Export,
    Family,
    LinkFamily,
}
impl ComponentsFormKind {
    pub const ALL: &'static [Self] = &[
        Self::Make,
        Self::Create,
        Self::Parametric,
        Self::Place,
        Self::Defaults,
        Self::Overrides,
        Self::Reset,
        Self::Detach,
        Self::Transform,
        Self::Import,
        Self::Export,
        Self::Family,
        Self::LinkFamily,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Make => "make",
            Self::Create => "create",
            Self::Parametric => "parametric",
            Self::Place => "place",
            Self::Defaults => "defaults",
            Self::Overrides => "overrides",
            Self::Reset => "reset",
            Self::Detach => "detach",
            Self::Transform => "transform",
            Self::Import => "import",
            Self::Export => "export",
            Self::Family => "family",
            Self::LinkFamily => "link_family",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Make => "Make from selection",
            Self::Create => "Capture definition",
            Self::Parametric => "New parametric…",
            Self::Place => "Place…",
            Self::Defaults => "Edit defaults…",
            Self::Overrides => "Apply occurrence…",
            Self::Reset => "Reset to inherited",
            Self::Detach => "Detach outer occurrence",
            Self::Transform => "Transform occurrences…",
            Self::Import => "Import…",
            Self::Export => "Save to library…",
            Self::Family => "Create component family…",
            Self::LinkFamily => "Link component family…",
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Draft {
    pub kind: ComponentsFormKind,
    pub identity: Identity,
    pub began: u64,
    pub definition: Option<String>,
    pub occurrence: Option<String>,
    pub ids: Vec<String>,
    pub fields: BTreeMap<String, String>,
    pub error: Option<String>,
    pub nested_member: bool,
    /// RoboCAD applied this draft: its form closed and it is no longer
    /// offered to resume (it is kept, like every draft).
    pub applied: bool,
}
fn text(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string())
}
pub(super) fn definition<'a>(
    st: &'a ComponentsState,
    id: &str,
) -> Result<&'a ComponentDefinition, String> {
    st.catalogue
        .as_ref()
        .and_then(|(_, c)| c.definitions.iter().find(|d| d.id == id))
        .ok_or_else(|| {
            format!("components.definition.{id}: no such definition in current catalogue")
        })
}

pub(crate) fn open(
    st: &mut ComponentsState,
    doc: &CadDocument,
    kind: ComponentsFormKind,
    id: Option<&str>,
    selected: &[String],
) -> Result<Value, String> {
    let identity = Identity::of(doc)?;
    let mut d = Draft {
        kind,
        identity,
        began: doc.shown_revision(),
        definition: if matches!(
            kind,
            ComponentsFormKind::Place
                | ComponentsFormKind::Defaults
                | ComponentsFormKind::Export
        ) {
            id.map(str::to_owned).or_else(|| st.selected.clone())
        } else if kind == ComponentsFormKind::LinkFamily {
            // LinkFamily's optional id addresses only the occurrence. The
            // independent library selection supplies the family definition.
            st.selected.clone()
        } else {
            None
        },
        occurrence: None,
        ids: selected.to_vec(),
        fields: BTreeMap::new(),
        error: None,
        nested_member: false,
        applied: false,
    };
    let nodes = doc
        .doc
        .as_ref()
        .ok_or("components.document: not yet read")?
        .nodes
        .as_slice();
    let occurrence = matches!(
        kind,
        ComponentsFormKind::Overrides
            | ComponentsFormKind::Reset
            | ComponentsFormKind::Detach
            | ComponentsFormKind::LinkFamily
    );
    if occurrence {
        let selected = id
            .or_else(|| selected.first().map(String::as_str))
            .ok_or("Select a linked assembly or one of its parts")?;
        let mut node = nodes
            .iter()
            .find(|n| n.id == selected)
            .ok_or_else(|| format!("components.occurrence.{selected}: missing node"))?;
        // A selected materialized part maps to its nearest linked occurrence;
        // nested occurrences keep their branch identity for overrides.
        if node.component_instance.is_none()
            && let Some(member) = &node.component_member
        {
            let owner = member["instance_id"]
                .as_str()
                .ok_or("components.member.instance_id: missing")?;
            node = nodes
                .iter()
                .find(|n| n.id == owner)
                .ok_or("components.member.instance_id: missing owner")?;
        }
        if matches!(kind, ComponentsFormKind::Detach) {
            for _ in 0..nodes.len() {
                let Some(owner) = node
                    .component_member
                    .as_ref()
                    .and_then(|m| m["instance_id"].as_str())
                else {
                    break;
                };
                node = nodes
                    .iter()
                    .find(|n| n.id == owner)
                    .ok_or("components.member.instance_id: missing outer occurrence")?;
            }
            if node.component_member.is_some() {
                return Err("components.member: cyclic ownership".into());
            }
        }
        let instance = node
            .component_instance
            .as_ref()
            .ok_or("Select a component occurrence")?;
        d.occurrence = Some(node.id.clone());
        d.nested_member = node.component_member.is_some();
        if kind != ComponentsFormKind::LinkFamily {
            d.definition = instance["definition_id"].as_str().map(str::to_owned);
        }
        let mut local = instance.get("overrides").cloned().unwrap_or(json!({}));
        if d.nested_member {
            let target = node.id.as_str();
            let mut outer = node;
            for _ in 0..nodes.len() {
                let Some(owner) = outer
                    .component_member
                    .as_ref()
                    .and_then(|m| m["instance_id"].as_str())
                else {
                    break;
                };
                outer = nodes
                    .iter()
                    .find(|n| n.id == owner)
                    .ok_or("components.member: missing owner")?;
            }
            if outer.component_member.is_some() {
                return Err("components.member: cyclic ownership".into());
            }
            let spec = outer
                .component_instance
                .as_ref()
                .ok_or("components.member: missing outer instance")?;
            let source = spec["node_map"]
                .as_object()
                .and_then(|map| {
                    map.iter()
                        .find(|(_, v)| v.as_str() == Some(target))
                        .map(|(k, _)| k)
                })
                .ok_or("components.member.node_map: missing nested source identity")?;
            local = spec
                .get("nested_overrides")
                .and_then(|b| b.get(source))
                .cloned()
                .unwrap_or(json!({}));
        }
        d.fields.insert("overrides".into(), local.to_string());
        d.fields
            .insert("placement".into(), instance["placement"].to_string());
        if !d.nested_member {
            d.fields.insert(
                "origin".into(),
                instance["placement"]["translation"]
                    .as_array()
                    .map(|v| v.iter().map(text).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default(),
            );
        }
    }
    for (key, val) in [
        ("name", ""),
        ("origin", "0, 0, 0"),
        ("angle_deg", "0"),
        ("shape", "box"),
        ("overrides", "{}"),
        ("path", ""),
        ("parameters", "{}"),
        ("variants", "{}"),
        ("variant", ""),
        ("axis", "0, 0, 1"),
        ("translation", "0, 0, 0"),
        ("scale", "1"),
    ] {
        d.fields.entry(key.into()).or_insert(val.into());
    }
    if kind == ComponentsFormKind::Parametric {
        d.fields.insert("name".into(), "Parametric box".into());
    }
    if let Some(id) = &d.definition {
        let definition = definition(st, id)?;
        d.fields.insert("name".into(), definition.name.clone());
        d.fields.insert(
            "variant".into(),
            definition.default_variant.clone().unwrap_or_default(),
        );
        d.fields
            .insert("features".into(), json!(definition.features).to_string());
        d.fields.insert(
            "parameters".into(),
            json!(definition.parameters).to_string(),
        );
        d.fields
            .insert("nested".into(), json!(definition.nested).to_string());
        d.fields
            .insert("variants".into(), json!(definition.variants).to_string());
        for (id, nested) in &definition.nested {
            d.fields.insert(
                format!("nested.{id}.parameter_bindings"),
                json!(nested.parameter_bindings).to_string(),
            );
            d.fields.insert(
                format!("nested.{id}.overrides"),
                json!(nested.overrides).to_string(),
            );
        }
        for (name, variant) in &definition.variants {
            d.fields.insert(
                format!("variant.{name}.parameter_bindings"),
                json!(variant.parameter_bindings).to_string(),
            );
        }
        for (name, p) in &definition.parameters {
            for (key, value) in [
                ("value", text(&p.value)),
                ("unit", p.unit.clone()),
                ("min", p.min.as_ref().map(text).unwrap_or_default()),
                ("max", p.max.as_ref().map(text).unwrap_or_default()),
                ("provenance", p.provenance.clone()),
                ("description", p.description.clone()),
            ] {
                d.fields.insert(format!("parameter.{name}.{key}"), value);
            }
            let local: Value = serde_json::from_str(
                d.fields
                    .get("overrides")
                    .map(String::as_str)
                    .unwrap_or("{}"),
            )
            .unwrap_or(json!({}));
            d.fields.insert(
                format!("override.{name}.enabled"),
                if local.get(name).is_some() {
                    "true"
                } else {
                    "false"
                }
                .into(),
            );
            d.fields.insert(
                format!("override.{name}.value"),
                text(local.get(name).unwrap_or(&p.value)),
            );
        }
        for (key, port) in &definition.ports {
            // RoboCAD's combo's first candidate, never fabricate a binding.
            d.fields.insert(
                format!("binding.{key}"),
                nodes
                    .iter()
                    .find(|n| n.kind == port.kind)
                    .map(|n| n.id.clone())
                    .unwrap_or_default(),
            );
        }
        if kind == ComponentsFormKind::Export {
            d.fields.insert(
                "path".into(),
                format!("{}/{}.rcomp", st.folder, definition.name),
            );
        }
    } else if matches!(
        kind,
        ComponentsFormKind::Place
            | ComponentsFormKind::Defaults
            | ComponentsFormKind::Export
            | ComponentsFormKind::LinkFamily
    ) {
        return Err("Select a component in the library".into());
    }
    if d.nested_member {
        d.fields.remove("origin");
    }
    let out = json!({"form":d});
    st.drafts.push(d);
    st.current = Some(st.drafts.len() - 1);
    st.focus = None;
    st.open = true;
    st.touch();
    Ok(out)
}
pub(crate) fn open_import(
    st: &mut ComponentsState,
    doc: &CadDocument,
    path: &str,
) -> Result<(), String> {
    open(st, doc, ComponentsFormKind::Import, None, &[])?;
    if let Some(d) = st.draft_mut() {
        d.fields.insert("path".into(), path.into());
    }
    Ok(())
}
/// Queued text edits belong to the draft which spawned their field. Selecting
/// another form before Actions must not redirect or lose those unsaved edits.
pub(crate) fn set_at(
    st: &mut ComponentsState,
    index: Option<usize>,
    name: &str,
    value: &str,
) -> Result<Value, String> {
    let selected = st.current;
    if let Some(index) = index {
        if index >= st.drafts.len() {
            return Err("components.draft: unknown retained draft".into());
        }
        st.current = Some(index);
    }
    let result = set(st, name, value);
    st.current = selected;
    result
}
pub(crate) fn set(st: &mut ComponentsState, name: &str, value: &str) -> Result<Value, String> {
    let d = st.draft_mut().ok_or("No component form is open")?;
    if !d.fields.contains_key(name) {
        return Err(format!("components.form.{name}: unknown field"));
    }
    if name == "shape"
        && d.kind == ComponentsFormKind::Parametric
        && ["Parametric box", "Parametric cylinder"].contains(&s(d, "name").as_str())
    {
        d.fields
            .insert("name".into(), format!("Parametric {value}"));
    }
    d.fields.insert(name.into(), value.into());
    if name == "parameters" {
        // Invalid raw drafts remain visible; a valid whole-spec edit rebuilds
        // the row projection so untouched old rows cannot shadow it.
        if let Ok(params) = serde_json::from_str::<BTreeMap<String, ComponentParameter>>(value) {
            d.fields.retain(|k, _| !k.starts_with("parameter."));
            for (name, p) in params {
                for (key, value) in [
                    ("value", text(&p.value)),
                    ("unit", p.unit),
                    ("min", p.min.as_ref().map(text).unwrap_or_default()),
                    ("max", p.max.as_ref().map(text).unwrap_or_default()),
                    ("provenance", p.provenance),
                    ("description", p.description),
                ] {
                    d.fields.insert(format!("parameter.{name}.{key}"), value);
                }
            }
        }
    }
    if name == "overrides"
        && let Ok(local) = serde_json::from_str::<BTreeMap<String, Value>>(value)
    {
        for (key, val) in &mut d.fields {
            if let Some(name) = key
                .strip_prefix("override.")
                .and_then(|s| s.strip_suffix(".enabled"))
            {
                *val = local.contains_key(name).to_string();
            } else if let Some(name) = key
                .strip_prefix("override.")
                .and_then(|s| s.strip_suffix(".value"))
                && let Some(v) = local.get(name)
            {
                *val = text(v);
            }
        }
    }
    if name == "nested"
        && let Ok(nested) = serde_json::from_str::<BTreeMap<String, ComponentNested>>(value)
    {
        d.fields.retain(|k, _| !k.starts_with("nested."));
        for (id, n) in nested {
            d.fields.insert(
                format!("nested.{id}.parameter_bindings"),
                json!(n.parameter_bindings).to_string(),
            );
            d.fields.insert(
                format!("nested.{id}.overrides"),
                json!(n.overrides).to_string(),
            );
        }
    }
    if name == "variants"
        && let Ok(variants) = serde_json::from_str::<BTreeMap<String, ComponentVariant>>(value)
    {
        d.fields.retain(|k, _| !k.starts_with("variant."));
        for (name, v) in variants {
            d.fields.insert(
                format!("variant.{name}.parameter_bindings"),
                json!(v.parameter_bindings).to_string(),
            );
        }
    }
    d.error = None;
    st.touch();
    Ok(json!({"field":name,"value":value}))
}
fn s(d: &Draft, key: &str) -> String {
    d.fields.get(key).cloned().unwrap_or_default()
}
fn json_field<T: serde::de::DeserializeOwned>(d: &Draft, key: &str) -> Result<T, String> {
    serde_json::from_str(&s(d, key)).map_err(|e| format!("components.form.{key}: {e}"))
}
fn vector(d: &Draft, key: &str, unit: Unit) -> Result<[f64; 3], String> {
    match evaluate(&FieldKind::Vector { unit }, &s(d, key))
        .map_err(|e| format!("components.form.{key}: {e}"))?
    {
        FieldValue::Vector(v) => Ok(v),
        _ => Err(format!("components.form.{key}: expected vector")),
    }
}
fn number(d: &Draft, key: &str, unit: Unit) -> Result<f64, String> {
    match evaluate(
        &FieldKind::Number {
            unit,
            min: None,
            max: None,
            decimals: 6,
        },
        &s(d, key),
    )
    .map_err(|e| format!("components.form.{key}: {e}"))?
    {
        FieldValue::Number(v) => Ok(v),
        _ => Err(format!("components.form.{key}: expected number")),
    }
}
fn parameters(d: &Draft) -> Result<BTreeMap<String, ComponentParameter>, String> {
    let mut params: BTreeMap<String, ComponentParameter> = json_field(d, "parameters")?;
    for (name, p) in &mut params {
        let prefix = format!("parameter.{name}");
        if d.fields.contains_key(&format!("{prefix}.value")) {
            p.value = Value::String(s(d, &format!("{prefix}.value")));
            p.unit = s(d, &format!("{prefix}.unit"));
            p.provenance = s(d, &format!("{prefix}.provenance"));
            p.description = s(d, &format!("{prefix}.description"));
            p.min = (!s(d, &format!("{prefix}.min")).is_empty())
                .then(|| Value::String(s(d, &format!("{prefix}.min"))));
            p.max = (!s(d, &format!("{prefix}.max")).is_empty())
                .then(|| Value::String(s(d, &format!("{prefix}.max"))));
        }
    }
    Ok(params)
}
pub(crate) fn operation(
    d: &Draft,
    st: &ComponentsState,
    _doc: &CadDocument,
) -> Result<ComponentOperation, String> {
    let selected_definition = || {
        d.definition
            .clone()
            .ok_or("Select a component in the library".to_string())
    };
    let occurrence = || {
        d.occurrence
            .clone()
            .ok_or("Select an occurrence".to_string())
    };
    let placement = || -> Result<Value, String> {
        Ok(
            json!({"translation":vector(d,"origin",Unit::Length)?,"axis":[0,0,1],"angle_deg":number(d,"angle_deg",Unit::Angle)?,"scale":1}),
        )
    };
    let overrides = || -> Result<BTreeMap<String, Value>, String> {
        let mut values: BTreeMap<String, Value> = json_field(d, "overrides")?;
        for (key, val) in &d.fields {
            if let Some(name) = key
                .strip_prefix("override.")
                .and_then(|s| s.strip_suffix(".enabled"))
            {
                match val.as_str() {
                    "true" => {
                        values.insert(
                            name.into(),
                            Value::String(s(d, &format!("override.{name}.value"))),
                        );
                    }
                    "false" => {
                        values.remove(name);
                    }
                    _ => return Err(format!("components.form.{key}: choose true or false")),
                }
            }
        }
        Ok(values)
    };
    use ComponentsFormKind as K;
    Ok(match d.kind {
        K::Make => ComponentOperation::Make {
            ids: d.ids.clone(),
            name: s(d, "name"),
            origin: vector(d, "origin", Unit::Length)?,
        },
        K::Create => ComponentOperation::Create {
            ids: d.ids.clone(),
            name: s(d, "name"),
            origin: vector(d, "origin", Unit::Length)?,
        },
        K::Parametric => ComponentOperation::Parametric {
            name: s(d, "name"),
            shape: s(d, "shape"),
        },
        K::Place => {
            let id = selected_definition()?;
            let def = definition(st, &id)?;
            ComponentOperation::Place {
                definition_id: id,
                placement: placement()?,
                name: s(d, "name"),
                overrides: overrides()?,
                variant: (!s(d, "variant").is_empty()).then(|| s(d, "variant")),
                bindings: def
                    .ports
                    .keys()
                    .map(|key| {
                        let value = s(d, &format!("binding.{key}"));
                        (key.clone(), (!value.is_empty()).then_some(value))
                    })
                    .collect(),
            }
        }
        K::Defaults => {
            let mut nested = json_field::<BTreeMap<String, ComponentNested>>(d, "nested")?;
            for (id, n) in &mut nested {
                n.parameter_bindings = json_field(d, &format!("nested.{id}.parameter_bindings"))?;
                n.overrides = json_field(d, &format!("nested.{id}.overrides"))?;
            }
            let mut variants = json_field::<BTreeMap<String, ComponentVariant>>(d, "variants")?;
            for (name, v) in &mut variants {
                v.parameter_bindings =
                    json_field(d, &format!("variant.{name}.parameter_bindings"))?;
            }
            ComponentOperation::Defaults {
                definition_id: selected_definition()?,
                parameters: parameters(d)?,
                features: json_field(d, "features")?,
                nested,
                family_variants: Some(variants),
            }
        }
        K::Overrides => {
            let mut placed: Value = json_field(d, "placement")?;
            if !d.nested_member {
                placed["translation"] = json!(vector(d, "origin", Unit::Length)?);
            }
            ComponentOperation::Overrides {
                instance_id: occurrence()?,
                overrides: overrides()?,
                placement: (!d.nested_member).then_some(placed),
            }
        }
        K::Reset => ComponentOperation::Overrides {
            instance_id: occurrence()?,
            overrides: BTreeMap::new(),
            placement: None,
        },
        K::Detach => ComponentOperation::Detach {
            instance_id: occurrence()?,
        },
        K::Import => ComponentOperation::Import { path: s(d, "path") },
        K::Export => {
            let path = s(d, "path");
            ComponentOperation::Export {
                definition_id: selected_definition()?,
                path: if path.ends_with(".rcomp") {
                    path
                } else {
                    format!("{path}.rcomp")
                },
            }
        }
        K::Family => ComponentOperation::Family {
            name: s(d, "name"),
            parameters: parameters(d)?,
            variants: json_field::<BTreeMap<String, ComponentVariant>>(d, "variants")?,
            default_variant: (!s(d, "variant").is_empty()).then(|| s(d, "variant")),
        },
        K::LinkFamily => ComponentOperation::LinkFamily {
            instance_id: occurrence()?,
            definition_id: selected_definition()?,
            variant: s(d, "variant"),
            overrides: overrides()?,
        },
        K::Transform => ComponentOperation::Transform {
            ids: d.ids.clone(),
            translation: vector(d, "translation", Unit::Length)?,
            axis: vector(d, "axis", Unit::Plain)?,
            angle_deg: number(d, "angle_deg", Unit::Angle)?,
            center: None,
            scale: number(d, "scale", Unit::Plain)?,
        },
    })
}
