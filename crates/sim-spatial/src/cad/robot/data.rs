//! RoboCAD's robot description and the reads beside it, as last read:
//! `GET /robot` (joints, motors, links, DoF, ground, issues), the per-node
//! results and margins (`GET /results/nodes`, over physical.py's
//! `results_margins`), `GET /sensors`, `GET /cables`, `GET /battery`,
//! `GET /control`, `GET /uncertainty`, `GET /actuator-profiles`, and the
//! motor library (`GET /motors`, once per document generation).
//!
//! One Dedicated job reads them all at each (document generation, shown
//! revision), the key the tree, meshes and selection share; a job for an
//! older key is dropped (cancelled) when a newer one starts, so a stale
//! read never lands. Each read keeps its own error: an older RoboCAD
//! without `/results/nodes` still shows the summary. RoboCAD's
//! `load_results` and `apply_identification` do not move the revision, so
//! their callers [`RobotData::invalidate`] the reads.
//!
//! Nothing here is derived: every value is RoboCAD's answer, shown with the
//! revision it was read at.
use crate::cad::document::CadDocument;
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use sim_runtime::cad_client::{Battery, Control, Margins, MotorSpec, NodeDetail, NodeResult, NodeResults, RobotSummary};
use std::collections::BTreeMap;

/// (document generation, RoboCAD's shown revision): what a read was made at.
pub(crate) type Key = (u64, u64);

/// One read of everything beside the summary, each with its own outcome.
#[derive(Clone, Debug)]
pub(crate) struct Bundle {
    pub summary: Result<RobotSummary, String>,
    pub results: Result<NodeResults, String>,
    pub sensors: Result<Vec<NodeDetail>, String>,
    pub cables: Result<Vec<NodeDetail>, String>,
    pub battery: Result<Option<Battery>, String>,
    pub control: Result<Option<Control>, String>,
    pub uncertainty: Result<Option<Map<String, Value>>, String>,
    pub profiles: Result<Value, String>,
    /// Read only when this generation's library was not read yet.
    pub motors: Option<Result<BTreeMap<String, MotorSpec>, String>>,
}

/// The reads as last landed, and the job in flight.
#[derive(Default)]
pub struct RobotData {
    /// The key the bundle was read at.
    pub(crate) key: Option<Key>,
    pub(crate) bundle: Option<Bundle>,
    /// RoboCAD's motor library (`GET /motors`) and the generation it was read for.
    pub(crate) motors: Option<(u64, Result<BTreeMap<String, MotorSpec>, String>)>,
    job: Option<(Key, Job<Bundle>)>,
}

/// The key the document shows now.
pub(crate) fn key(doc: &CadDocument) -> Key {
    (doc.generation, doc.shown_revision())
}

impl RobotData {
    /// Read everything again at the current key (after a results load or
    /// an identification, which RoboCAD does not count as a revision).
    pub(crate) fn invalidate(&mut self) {
        self.key = None;
        self.job = None;
    }
    /// Whether the reads describe the document as shown now.
    pub(crate) fn current(&self, doc: &CadDocument) -> bool {
        self.key == Some(key(doc))
    }
    /// A read is in flight.
    pub(crate) fn reading(&self) -> bool {
        self.job.is_some()
    }
    /// The revision the shown reads were made at.
    pub(crate) fn read_at(&self) -> Option<u64> {
        self.key.map(|k| k.1)
    }
    /// RoboCAD's summary, if the last read succeeded.
    pub(crate) fn summary(&self) -> Option<&RobotSummary> {
        self.bundle.as_ref().and_then(|b| b.summary.as_ref().ok())
    }
    /// Why the summary could not be read.
    pub(crate) fn summary_error(&self) -> Option<&str> {
        self.bundle.as_ref().and_then(|b| b.summary.as_ref().err()).map(String::as_str)
    }
    /// The per-node results and margins, if read.
    pub(crate) fn results(&self) -> Option<&NodeResults> {
        self.bundle.as_ref().and_then(|b| b.results.as_ref().ok())
    }
    /// Node `id`'s margins (`results_margins`), if results are loaded.
    pub(crate) fn margins(&self, id: &str) -> Option<&Margins> {
        self.results().and_then(|r| r.margins.get(id))
    }
    /// Node `id`'s results (`Node.results`: section, peaks, hotspot) and
    /// its material's yield strength as RoboCAD reads it.
    pub(crate) fn node_results(&self, id: &str) -> Option<&NodeResult> {
        self.results().and_then(|r| r.nodes.get(id))
    }
    pub(crate) fn sensors(&self) -> &[NodeDetail] {
        self.bundle.as_ref().and_then(|b| b.sensors.as_ref().ok()).map_or(&[], Vec::as_slice)
    }
    pub(crate) fn cables(&self) -> &[NodeDetail] {
        self.bundle.as_ref().and_then(|b| b.cables.as_ref().ok()).map_or(&[], Vec::as_slice)
    }
    pub(crate) fn battery(&self) -> Option<&Battery> {
        self.bundle.as_ref().and_then(|b| b.battery.as_ref().ok()).and_then(Option::as_ref)
    }
    pub(crate) fn control(&self) -> Option<&Control> {
        self.bundle.as_ref().and_then(|b| b.control.as_ref().ok()).and_then(Option::as_ref)
    }
    pub(crate) fn uncertainty(&self) -> Option<&Map<String, Value>> {
        self.bundle.as_ref().and_then(|b| b.uncertainty.as_ref().ok()).and_then(Option::as_ref)
    }
    /// The document's actuator profiles (`robot_settings.actuator_profiles`; null when none).
    pub(crate) fn profiles(&self) -> Option<&Value> {
        self.bundle.as_ref().and_then(|b| b.profiles.as_ref().ok())
    }
    /// RoboCAD's motor library for this generation (id → spec), in its id order.
    pub(crate) fn motor_library(&self, doc: &CadDocument) -> Option<&BTreeMap<String, MotorSpec>> {
        self.motors.as_ref().filter(|(g, _)| *g == doc.generation).and_then(|(_, r)| r.as_ref().ok())
    }
    /// Why the motor library could not be read.
    pub(crate) fn motor_library_error(&self) -> Option<&str> {
        self.motors.as_ref().and_then(|(_, r)| r.as_ref().err()).map(String::as_str)
    }
}

/// An outcome as `cad_state.robot` shows it.
fn result<T: serde::Serialize>(r: &Result<T, String>) -> Value {
    match r {
        Ok(v) => json!({"ok": true, "value": v}),
        Err(e) => json!({"ok": false, "error": e}),
    }
}

/// `cad_state.robot` (the panel and tools add their parts).
pub(crate) fn state_json(doc: &CadDocument) -> Value {
    let data = &doc.robot.data;
    let mut out = json!({
        "read_at_revision": data.read_at(),
        "current": data.current(doc),
        "reading": data.reading(),
    });
    if let Some(b) = &data.bundle {
        out["summary"] = result(&b.summary);
        out["results"] = result(&b.results);
        out["sensors"] = result(&b.sensors);
        out["cables"] = result(&b.cables);
        out["battery"] = result(&b.battery);
        out["control"] = result(&b.control);
        out["uncertainty"] = result(&b.uncertainty);
        out["actuator_profiles"] = result(&b.profiles);
    }
    out["motors"] = data.motors.as_ref().map_or(Value::Null, |(_, r)| result(r));
    out
}

/// JobResults: the reads at the current key: started on a change (or after
/// [`RobotData::invalidate`]), a job for an older key dropped, a result landed.
pub(crate) fn sync(doc: Option<ResMut<CadDocument>>) {
    let Some(mut doc) = doc else { return };
    let now = key(&doc);
    // A job for another key is superseded (dropping it cancels it). Read
    // through `Ref` first: a `ResMut` deref would mark the document changed.
    if doc.robot.data.job.as_ref().is_some_and(|(k, _)| *k != now) {
        doc.robot.data.job = None;
    }
    let landed = doc.robot.data.job.as_ref().and_then(|(k, job)| job.poll().map(|r| (*k, r)));
    if let Some((k, result)) = landed {
        let generation = doc.generation;
        let data = &mut doc.robot.data;
        data.job = None;
        match result {
            Ok(mut bundle) => {
                if let Some(motors) = bundle.motors.take() {
                    data.motors = Some((generation, motors));
                }
                data.bundle = Some(bundle);
            }
            // The job itself failed (a panic): every read says so.
            Err(e) => data.bundle = Some(failed(&e)),
        }
        data.key = Some(k);
        doc.touch();
    }
    let data = &doc.robot.data;
    if data.key == Some(now) || data.job.is_some() || !doc.connected() || doc.doc_key.is_none() {
        return;
    }
    let Some(local) = doc.local.clone() else { return };
    let library = data.motors.as_ref().is_none_or(|(g, r)| *g != now.0 || r.is_err());
    let revision = doc.shown_revision();
    // Read from the open archive (`sim_cad::robotics`; no service).
    let job = Job::spawn(Pool::Compute, now.0, "cad-robot-description", move |_| {
        let archive = &local.archive;
        let m = &archive.manifest;
        fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, String> {
            serde_json::from_value(v).map_err(|e| e.to_string())
        }
        let summary = parse(sim_cad::robotics::summary(archive));
        let nodes_of = |kind: &str| -> Result<Vec<NodeDetail>, String> { m["nodes"].as_array().into_iter().flatten().filter(|n| n["kind"] == kind).map(|n| parse(n.clone())).collect() };
        let res = &m["results"];
        let node_results: serde_json::Map<String, Value> = m["nodes"].as_array().into_iter().flatten().filter(|n| n["results"].is_object()).map(|n| (n["id"].as_str().unwrap_or("").to_string(), json!({"results": n["results"], "yield_strength_pa": null}))).collect();
        let results = parse(json!({"revision": revision, "path": res["path"], "loaded": res["loaded"], "stale": if res.is_object() { json!(false) } else { Value::Null }, "provenance": res["provenance"], "margins": {}, "nodes": node_results}));
        let setting = |k: &str| m["robot_settings"].get(k).cloned().unwrap_or(Value::Null);
        let battery = parse(setting("battery"));
        let control = parse(setting("control"));
        let uncertainty = parse(setting("uncertainty"));
        let profiles = Ok(setting("actuator_profiles"));
        let motors = library.then(|| {
            let lib = sim_cad::robotics::library_json();
            Ok(lib.as_object().into_iter().flatten().filter_map(|(k, v)| serde_json::from_value(v.clone()).ok().map(|v| (k.clone(), v))).collect())
        });
        Ok(Bundle { summary, results, sensors: nodes_of("sensor"), cables: nodes_of("cable"), battery, control, uncertainty, profiles, motors })
    });
    doc.robot.data.job = Some((now, job));
}

/// A bundle whose every read failed with `e`.
fn failed(e: &str) -> Bundle {
    fn err<T>(e: &str) -> Result<T, String> {
        Err(e.to_string())
    }
    Bundle { summary: err(e), results: err(e), sensors: err(e), cables: err(e), battery: err(e), control: err(e), uncertainty: err(e), profiles: err(e), motors: None }
}
