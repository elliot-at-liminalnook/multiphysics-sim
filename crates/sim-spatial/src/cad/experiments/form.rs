//! Authoring values are preserved verbatim on refusal. Validation names field
//! paths; runner-owned defaults and fidelity profiles remain authoritative.
use super::*;
use crate::cad::types::{candidates::CandidateRequest, experiments::ExperimentRequest};
use std::time::Instant;
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Draft {
    pub stamp: Stamp,
    pub fields: BTreeMap<String, String>,
    pub linked: BTreeMap<String, String>,
    pub restored: BTreeMap<String, Value>,
    pub auto: bool,
    pub error: Option<String>,
    #[serde(skip)]
    pub edited: Instant,
    #[serde(skip)]
    pub submitted_sequence: Option<u64>,
}
impl Draft {
    pub(crate) fn new(stamp: Stamp) -> Self {
        Self {
            stamp,
            fields: BTreeMap::from([
                ("label".into(), "Captured experiment".into()),
                ("system".into(), String::new()),
                ("controller".into(), String::new()),
                ("controller_enabled".into(), "false".into()),
                ("language".into(), "rhai".into()),
                ("interface".into(), "position_target".into()),
                ("profile".into(), "quick_check".into()),
                (
                    "parameters".into(),
                    "{\"system\":{},\"controller\":{},\"settings\":{},\"seed\":0}".into(),
                ),
                ("operations".into(), "[]".into()),
                ("script_path".into(), String::new()),
                ("script_params".into(), "{}".into()),
                ("system_path".into(), String::new()),
                ("controller_path".into(), String::new()),
            ]),
            linked: BTreeMap::new(),
            restored: BTreeMap::new(),
            auto: false,
            error: None,
            edited: Instant::now(),
            submitted_sequence: None,
        }
    }
    pub(crate) fn get(&self, key: &str) -> &str {
        self.fields.get(key).map(String::as_str).unwrap_or("")
    }
}
pub(crate) fn set(st: &mut ExperimentsState, a: &ExperimentsArgs) -> Result<(), String> {
    let index = st.current.ok_or("experiments.draft: open a draft first")?;
    if a.draft_index.is_some_and(|i| i != index) {
        return Err("experiments.draft: selected draft changed; original retained".into());
    }
    let d = &mut st.drafts[index];
    if a.draft_sequence.is_some_and(|seq| seq != d.stamp.sequence) {
        return Err("experiments.draft_sequence: editor changed; original retained".into());
    }
    let name = a.name.as_deref().ok_or("experiments.field: required")?;
    if d.linked.contains_key(name) {
        return Err(format!(
            "experiments.{name}: linked source is read-only; unlink before editing"
        ));
    }
    if !d.fields.contains_key(name) {
        return Err(format!("experiments.{name}: no such field"));
    }
    d.fields
        .insert(name.into(), a.value.clone().unwrap_or_default());
    d.stamp.sequence += 1;
    d.edited = Instant::now();
    d.error = None;
    Ok(())
}
pub(crate) fn link(st: &mut ExperimentsState, a: &ExperimentsArgs) -> Result<(), String> {
    let index = st.current.ok_or("experiments.draft: open a draft first")?;
    if a.draft_index.is_some_and(|i| i != index) {
        return Err("experiments.draft: editor changed".into());
    }
    let key = a.name.as_deref().unwrap_or("system");
    if !["system", "controller"].contains(&key) {
        return Err("experiments.link: choose system or controller".into());
    }
    let path = a.value.clone().unwrap_or_default();
    if !path.is_empty() && !std::path::Path::new(&path).is_absolute() {
        return Err(
            "experiments.link.path: requires an absolute path".into(),
        );
    }
    let d = &mut st.drafts[index];
    if a.draft_sequence
        .is_some_and(|sequence| sequence != d.stamp.sequence)
    {
        return Err("experiments.link.draft_sequence: editor changed; original retained".into());
    }
    if path.is_empty() {
        d.linked.remove(key);
    } else {
        d.linked.insert(key.into(), path);
    }
    d.restored.remove(key);
    d.stamp.sequence += 1;
    d.edited = Instant::now();
    Ok(())
}
pub(crate) fn guard<'a>(doc: &CadDocument, st: &'a ExperimentsState) -> Result<&'a Draft, String> {
    guard_for(doc, st, false)
}
pub(crate) fn guard_for<'a>(
    doc: &CadDocument,
    st: &'a ExperimentsState,
    auxiliary: bool,
) -> Result<&'a Draft, String> {
    let d = st
        .draft()
        .ok_or("experiments.draft: open or resume a draft first")?;
    if !d.stamp.document_matches(doc) {
        return Err("experiments.draft: document generation or identity changed; draft retained, create a new draft".into());
    }
    if let Some(e) = doc.commit_refusal_for(Some(d.stamp.revision), auxiliary) {
        return Err(e);
    }
    Ok(d)
}
fn object(value: &Value, key: &str) -> Result<BTreeMap<String, Value>, String> {
    let v = value.get(key).cloned().unwrap_or_else(|| json!({}));
    serde_json::from_value(v)
        .map_err(|e| format!("experiments.parameters.{key}: expected object: {e}"))
}
fn bundle(d: &Draft, key: &str) -> Value {
    if d.get(key).is_empty() && !d.restored.contains_key(key) {
        return Value::Null;
    }
    let entry = if key == "system" {
        "system.rhai"
    } else {
        "controller.rhai"
    };
    let mut value = d
        .restored
        .get(key)
        .cloned()
        .unwrap_or_else(|| json!({"entry":entry,"files":{}}));
    let owned = value["entry"].as_str().unwrap_or(entry).to_string();
    value["files"][&owned] = json!(d.get(key));
    value
}
pub(crate) fn request(
    doc: &CadDocument,
    st: &ExperimentsState,
    preflight: bool,
) -> Result<ExperimentRequest, String> {
    let d = guard_for(doc, st, true)?;
    if !["quick_check", "validation"].contains(&d.get("profile")) {
        return Err("experiments.profile: choose quick_check or validation".into());
    }
    let p: Value = serde_json::from_str(d.get("parameters"))
        .map_err(|e| format!("experiments.parameters: {e}"))?;
    let map = p
        .as_object()
        .ok_or("experiments.parameters: expected JSON object")?;
    for key in map.keys() {
        if !["system", "controller", "settings", "seed"].contains(&key.as_str()) {
            return Err(format!("experiments.parameters.{key}: unknown field"));
        }
    }
    let seed = p
        .get("seed")
        .map(|v| {
            v.as_u64()
                .ok_or("experiments.parameters.seed: expected nonnegative integer")
        })
        .transpose()?
        .unwrap_or(0);
    if seed > 9_007_199_254_740_991 {
        return Err("experiments.parameters.seed: exceeds exact runner integer range".into());
    }
    let controller = match d.get("controller_enabled") {
        "false" => None,
        "true" => {
            let interface = d.get("interface");
            if !["position_target", "driver_duty"].contains(&interface) {
                return Err("experiments.interface: choose position_target or driver_duty".into());
            }
            let mut c = json!({"language":d.get("language"),"interface":interface,"parameters":object(&p,"controller")?});
            match d.get("language") {
                "rhai" => {
                    c["sources"] = bundle(d, "controller");
                }
                "process" => {
                    c["process"] = serde_json::from_str(d.get("controller"))
                        .map_err(|e| format!("experiments.controller.process: {e}"))?;
                }
                _ => return Err("experiments.controller.language: choose rhai or process".into()),
            }
            Some(c)
        }
        _ => return Err("experiments.controller_enabled: expected true or false".into()),
    };
    Ok(ExperimentRequest {
        document_id: d.stamp.document_id.clone(),
        expected_revision: d.stamp.revision,
        system: bundle(d, "system"),
        controller,
        parameters: object(&p, "system")?,
        settings: object(&p, "settings")?,
        profile: d.get("profile").into(),
        seed,
        preflight,
        label: d.get("label").into(),
        parent_run: st.selected.clone(),
    })
}
pub(crate) fn candidate_request(
    doc: &CadDocument,
    st: &ExperimentsState,
) -> Result<CandidateRequest, String> {
    let d = guard(doc, st)?;
    let operations: Vec<Value> = serde_json::from_str(d.get("operations"))
        .map_err(|e| format!("candidate.operations: expected array: {e}"))?;
    if operations.is_empty() {
        return Err("candidate.operations: at least one authoritative operation required".into());
    }
    Ok(CandidateRequest {
        document_id: d.stamp.document_id.clone(),
        expected_revision: d.stamp.revision,
        label: d.get("label").into(),
        operations,
    })
}
pub(crate) fn restored(st: &mut ExperimentsState, stamp: Stamp, spec: Value) -> Result<(), String> {
    let mut d = Draft::new(Stamp {
        draft_index: st.drafts.len(),
        sequence: 0,
        ..stamp
    });
    for key in ["system", "controller"] {
        let b = if key == "system" {
            &spec["system"]
        } else {
            &spec["controller"]["sources"]
        };
        if let Some(entry) = b["entry"].as_str() {
            d.fields
                .insert(key.into(), b["files"][entry].as_str().unwrap_or("").into());
            d.restored.insert(key.into(), b.clone());
        }
    }
    d.fields.insert("parameters".into(),serde_json::to_string_pretty(&json!({"system":spec["parameters"],"controller":spec["controller"]["parameters"].as_object().cloned().unwrap_or_default(),"settings":spec["settings"],"seed":spec["seed"]})).map_err(|e|e.to_string())?);
    d.fields.insert(
        "controller_enabled".into(),
        (!spec["controller"].is_null()).to_string(),
    );
    for (key, value) in [
        ("profile", &spec["profile"]),
        ("language", &spec["controller"]["language"]),
        ("interface", &spec["controller"]["interface"]),
    ] {
        if let Some(v) = value.as_str() {
            d.fields.insert(key.into(), v.into());
        }
    }
    if spec["controller"]["language"] == "process" {
        d.fields.insert(
            "controller".into(),
            serde_json::to_string_pretty(&spec["controller"]["process"])
                .map_err(|e| e.to_string())?,
        );
    }
    st.current = Some(st.drafts.len());
    st.drafts.push(d);
    st.inputs = Some(spec);
    st.open = true;
    st.focus = None;
    Ok(())
}
