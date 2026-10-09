//! The experiment service (RoboCAD's `Experiments` and its worker process,
//! in process). [`Lab::create`] captures a run's inputs and holds its owner
//! lock; the host runs [`Lab::execute`] on one of its jobs; reads observe
//! the records on disk, so runs another editor owns show too.
use super::capture::{self, Snapshot};
use super::config::{self, located, object_fields};
use super::records::{self, Lease, fields};
use super::results;
use super::{Simulator, canonical, digest, now, read_json, write_bytes, write_json};
use crate::archive::ArchiveDocument;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The system a run captures when none is given: the document's assembly.
pub const DEFAULT_SYSTEM: &str = "let assembly = cad(\"assembly\");\n";
/// The controller a run captures when none is given.
pub const DEFAULT_CONTROLLER: &str = "fn control(t, sensors, commands, state) {\n    let p = parameters();\n    let target = if t < 0.2 { 0.0 } else { p.target };\n    for name in commands.keys() { commands[name] = target; }\n    #{ commands: commands, state: state }\n}\n";

/// The limits every result states.
pub const LIMITATIONS: [&str; 5] = [
    "Accuracy bounds apply only to the documented geometry, material and boundary conditions.",
    "Every link is rigid: flexible links are not derived in process (settings.flex is refused).",
    "A seeded run does not sweep the captured material/geometry Monte Carlo uncertainty distributions.",
    "CAD sensor noise flag controls stochastic noise and bias walk; fixed bias and quantization remain captured CAD inputs.",
    "Scripted stochastic components use their declared parameters; use seed() to select reproducible streams.",
];

/// `{entry, files}` sources: text alone becomes one file; names must be relative.
pub fn sources(value: Option<&Value>, default_name: &str, default_text: &str) -> Result<Value, String> {
    let v = match value {
        None | Some(Value::Null) => json!({"entry": default_name, "files": {default_name: default_text}}),
        Some(Value::String(s)) => json!({"entry": default_name, "files": {default_name: s}}),
        Some(v) => v.clone(),
    };
    let m = v.as_object().filter(|m| m.len() == 2 && m.contains_key("entry") && m["files"].is_object()).ok_or("Sources require entry and files fields")?;
    for (name, text) in m["files"].as_object().expect("checked") {
        let p = Path::new(name);
        if !text.is_string() || name.contains('\\') || p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err("Script files must have relative names and text contents".into());
        }
    }
    let entry = m["entry"].as_str().ok_or("Entry script is missing from captured sources")?;
    if !m["files"].as_object().expect("checked").contains_key(entry) {
        return Err("Entry script is missing from captured sources".into());
    }
    Ok(v)
}

pub fn empty_graph() -> Value {
    json!({"version": 1, "components": {}, "connections": {}})
}

/// Lower a component graph into a generated Rhai module beside the entry
/// (RoboCAD's `compose_sources`): the sources with it imported, and each
/// component's mapping to its declaration.
pub fn compose_sources(sources: &Value, graph: &Value) -> Result<(Value, Vec<Value>), String> {
    let components = graph["components"].as_object().cloned().unwrap_or_default();
    if components.is_empty() {
        return Ok((sources.clone(), Vec::new()));
    }
    let entry = sources["entry"].as_str().unwrap_or("system.rhai");
    let parent = Path::new(entry).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let module = if parent.is_empty() { "__robocad_graph.rhai".to_string() } else { format!("{parent}/__robocad_graph.rhai") };
    if sources["files"].get(&module).is_some() {
        return Err(format!("Reserved generated module already exists: {module}"));
    }
    let quote = |v: &Value| serde_json::to_string(v).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut variables = BTreeMap::new();
    let mut mapping = Vec::new();
    for (index, (identity, component)) in components.iter().enumerate() {
        let variable = format!("component_{index}");
        variables.insert(identity.clone(), variable.clone());
        let name = format!("graph/{identity}");
        let parameters: Vec<String> = component["parameters"].as_object().into_iter().flatten().map(|(k, v)| format!("{}: {}", quote(&json!(k)), quote(v))).collect();
        let call = match component["binding"].as_str() {
            Some(b) => format!("bind_component({}, {},", quote(&json!(name)), quote(&json!(b))),
            None => format!("part({},", quote(&json!(name))),
        };
        lines.push(format!("let {variable} = {call} {}, #{{{}}});", quote(&component["type"]), parameters.join(", ")));
        mapping.push(json!({"id": identity, "name": component["name"], "native_name": name, "body_id": component["body_id"], "source": module, "line": lines.len()}));
    }
    for (_, connection) in graph["connections"].as_object().into_iter().flatten() {
        let ports: Vec<String> = connection["ports"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| format!("{}.port({})", variables.get(p["component_id"].as_str().unwrap_or("")).cloned().unwrap_or_default(), quote(&p["port"])))
            .collect();
        lines.push(format!("connect([{}]);", ports.join(", ")));
    }
    let mut result = sources.clone();
    result["files"][module.as_str()] = json!(lines.join("\n") + "\n");
    let main = result["files"][entry].as_str().unwrap_or("").to_string();
    result["files"][entry] = json!(main + "\nimport \"__robocad_graph\" as robocad_graph;\n");
    Ok((result, mapping))
}

/// The experiment service over one runs folder.
pub struct Lab {
    pub root: PathBuf,
    /// Owner leases of the runs this process created and has not finished.
    leases: Mutex<HashMap<String, Lease>>,
    cancelled: Mutex<HashSet<String>>,
}

/// `runs/experiments` in the repository (beside `library/`).
pub fn default_root() -> PathBuf {
    let registry = sim_print::registry::default_path();
    registry.parent().and_then(|p| p.parent()).and_then(|p| p.parent()).map_or_else(|| PathBuf::from("runs/experiments"), |root| root.join("runs").join("experiments"))
}

fn checked_id(id: &str) -> Result<&str, String> {
    if id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit()) { Ok(id) } else { Err(format!("no experiment {id}")) }
}

fn run_id() -> String {
    // 32 hex digits, as RoboCAD's uuid4().hex.
    format!("{}{}{}", crate::edit::new_id(), crate::edit::new_id(), &crate::edit::new_id()[..8])
}

impl Lab {
    pub fn new(root: PathBuf) -> Lab {
        Lab { root, leases: Mutex::default(), cancelled: Mutex::default() }
    }

    fn folder(&self, id: &str) -> Result<PathBuf, String> {
        Ok(self.root.join(checked_id(id)?))
    }

    /// Every run of `document_id` (all when None), newest first.
    pub fn list(&self, document_id: Option<&str>) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        let Ok(dir) = std::fs::read_dir(&self.root) else { return Ok(out) };
        for e in dir.flatten() {
            let folder = e.path();
            if !folder.join("run.json").is_file() {
                continue;
            }
            let Ok(record) = records::observe(&folder) else { continue };
            if document_id.is_none_or(|d| record["document_id"].as_str() == Some(d)) {
                out.push(record);
            }
        }
        out.sort_by(|a, b| b["created_at"].as_f64().unwrap_or(0.0).total_cmp(&a["created_at"].as_f64().unwrap_or(0.0)));
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Result<Value, String> {
        let folder = self.folder(id)?;
        if !folder.join("run.json").is_file() {
            return Err(format!("no experiment {id}"));
        }
        records::observe(&folder)
    }

    /// The registered components (the simulator's catalogue).
    pub fn catalogue(&self, sim: &dyn Simulator) -> Value {
        sim.catalogue()
    }

    /// Capture a run: its inputs, the snapshot archive, the record (queued)
    /// and its owner lease. `graph` is the document's component graph.
    pub fn create(&self, request: &Value, snapshot: Option<&Snapshot>, graph: &Value, sim: &dyn Simulator) -> Result<Value, String> {
        let request = config::request(request)?;
        if let Some(s) = snapshot {
            let expected = request["expected_revision"].as_u64();
            if expected != Some(s.revision) {
                return Err(format!("Expected document revision {}; current revision is {}", request["expected_revision"], s.revision));
            }
        }
        let system = sources(request.get("system"), "system.rhai", if snapshot.is_some() { DEFAULT_SYSTEM } else { "" })?;
        let mut controller = match request.get("controller").filter(|c| !c.is_null()) {
            Some(c) => c.clone(),
            None if snapshot.is_some() => json!({"language": "rhai", "parameters": {"target": 0.3}}),
            None => Value::Null,
        };
        if controller["language"] == "rhai" {
            controller["sources"] = sources(controller.get("sources"), "controller.rhai", DEFAULT_CONTROLLER)?;
        }
        let settings = request["settings"].clone();
        let id = run_id();
        let folder = self.root.join(&id);
        std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let parameters = request.get("parameters").filter(|p| !p.is_null()).cloned().unwrap_or_else(|| json!({}));
        let seed = request["seed"].clone();
        let mut provenance = json!({
            "document_id": snapshot.map(|s| s.document_id.clone()), "revision": snapshot.map(|s| s.revision),
            "physical_hash": snapshot.map(|s| s.physical_hash.clone()), "cad_archive_hash": snapshot.map(|s| s.archive_hash.clone()),
            "cad_derivation_hash": snapshot.map(|s| s.cad_derivation_hash.clone()),
            "source_hash": digest(&canonical(&system)), "controller_hash": digest(&canonical(&controller)),
            "parameters_hash": digest(&canonical(&parameters)), "candidate_id": request["candidate_id"],
            "seed": seed, "component_graph_hash": digest(&canonical(graph)),
            "derivation": "sim-cad physical export (in process)",
        });
        for (k, v) in sim.identity().as_object().into_iter().flatten() {
            provenance[k.as_str()] = v.clone();
        }
        let spec = json!({
            "version": 1, "run_id": id, "system": system, "parameters": parameters, "preflight": request["preflight"],
            "controller": controller, "settings": settings, "provenance": provenance, "cad": {},
            "seed": seed, "profile": request["profile"], "component_graph": graph,
        });
        write_json(&folder.join("input.json"), &spec)?;
        if let Some(s) = snapshot {
            write_bytes(&folder.join("model.rcad"), &s.data)?;
        }
        let label = request["label"].as_str().map(str::to_string).unwrap_or_else(|| format!("Experiment {}", &id[..8]));
        let record = json!({
            "id": id, "created_at": now(), "updated_at": now(), "state": "queued", "fraction": 0,
            "document_id": spec["provenance"]["document_id"], "revision": spec["provenance"]["revision"],
            "label": label, "provenance": spec["provenance"], "directory": folder.display().to_string(),
            "settings": settings, "parent_run": request.get("parent_run").cloned().unwrap_or(Value::Null),
            "profile": request["profile"], "seed": seed, "preflight": request["preflight"],
        });
        let lease = Lease::acquire(&folder.join("owner.lock"), false)?.ok_or("the new run's owner lock is held elsewhere")?;
        self.leases.lock().map_err(|_| "experiment leases poisoned")?.insert(id.clone(), lease);
        write_json(&folder.join("run.json"), &record)?;
        Ok(record)
    }

    fn update(&self, id: &str, f: Map<String, Value>) -> Result<Value, String> {
        let cancelled = self.cancelled.lock().map_or(false, |c| c.contains(id));
        if cancelled && !matches!(f.get("state").and_then(Value::as_str), Some("cancelling" | "cancelled")) {
            return self.get(id);
        }
        records::update(&self.root.join(id), f)
    }

    /// Execute a run this process created (on the host's job): the record
    /// ends completed, failed or cancelled, and the owner lease is released.
    pub fn execute(&self, id: &str, sim: &dyn Simulator, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
        let stop = || cancelled() || self.cancelled.lock().map_or(false, |c| c.contains(id));
        let outcome = self.pipeline(id, sim, &stop);
        let record = match outcome {
            Ok(result) => self.update(id, fields([("state", json!("completed")), ("stage", json!(if result["preflight"] == true { "checked" } else { "finished" })), ("fraction", json!(1)), ("exit_code", json!(0)), ("timing", result["timing"].clone()), ("evaluation", result["evaluation"].clone())])),
            Err(_) if stop() => self.update(id, fields([("state", json!("cancelled")), ("stage", json!("cancelled"))])),
            Err(e) => self.update(id, fields([("state", json!("failed")), ("error", json!(e))])),
        };
        if let Ok(mut leases) = self.leases.lock() {
            leases.remove(id);
        }
        record
    }

    /// RoboCAD's worker: compose, resolve, derive, run, check.
    fn pipeline(&self, id: &str, sim: &dyn Simulator, stop: &dyn Fn() -> bool) -> Result<Value, String> {
        let folder = self.folder(id)?;
        let started = std::time::Instant::now();
        let mut spec = read_json(&folder.join("input.json"))?;
        let emit = |f: Map<String, Value>| {
            let _ = self.update(id, f);
        };
        let graph = spec.as_object_mut().and_then(|m| m.remove("component_graph")).unwrap_or_else(empty_graph);
        if graph["components"].as_object().into_iter().flatten().any(|(_, c)| c.get("derivation").is_some_and(|d| !d.is_null())) {
            return Err("component derivations (geometry-derived components such as fluid volumes) are not derived in process yet; remove the derivation or run the component by its parameters".into());
        }
        emit(fields([("state", json!("building")), ("stage", json!("composition"))]));
        write_json(&folder.join("component_derivations.json"), &json!([]))?;
        let (system, graph_mapping) = compose_sources(&spec["system"], &graph)?;
        spec["system"] = system;
        write_json(&folder.join("component_graph_mapping.json"), &json!(graph_mapping))?;
        write_json(&folder.join("composition.json"), &spec)?;
        emit(fields([("stage", json!("resolve"))]));
        let plan = sim.resolve(&spec)?;
        let cads: Vec<String> = plan["cad"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
        spec["provenance"]["uses_cad"] = json!(!cads.is_empty());
        write_json(&folder.join("system.json"), &plan)?;
        let script_mapping: Vec<Value> = plan["components"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| json!({"native_name": c["name"], "name": c["name"], "source": c["location"]["source"], "line": c["location"]["line"], "column": c["location"]["column"]}))
            .collect();
        write_json(&folder.join("script_component_mapping.json"), &json!(script_mapping))?;
        let configuration = plan.get("configuration").filter(|c| !c.is_null()).cloned().unwrap_or_else(|| json!({}));
        let location = plan.get("configuration_location").cloned().unwrap_or(Value::Null);
        let profile_name = spec["profile"].as_str().unwrap_or("quick_check").to_string();
        let defaults = config::profile(&profile_name)?;
        let keys: Vec<&str> = defaults.as_object().expect("object").keys().map(String::as_str).collect();
        object_fields(&configuration, &["settings", "expectations", "cad_overrides"], "configure").map_err(|e| located(&location, e))?;
        let script_settings = configuration.get("settings").cloned().unwrap_or_else(|| json!({}));
        object_fields(&script_settings, &keys, "configure.settings").map_err(|e| located(&location, e))?;
        let mut evidence = json!({
            "profile": profile_name, "profile_defaults": defaults, "captured_settings": spec["settings"],
            "script_settings": script_settings, "source": location, "seed": spec["seed"], "cad_overrides": [], "mechanical_exclusions": [],
        });
        let mut merged = spec["settings"].clone();
        for (k, v) in script_settings.as_object().into_iter().flatten() {
            merged[k.as_str()] = v.clone();
        }
        spec["settings"] = config::settings(&merged, &profile_name).map_err(|e| located(&location, e))?;
        evidence["effective_settings"] = spec["settings"].clone();
        let derive_started = std::time::Instant::now();
        let mut cache_hit = false;
        if !cads.is_empty() {
            if !folder.join("model.rcad").is_file() {
                return Err("System imports CAD, but this run has no captured document".into());
            }
            if cads.len() != 1 {
                return Err("Use one captured assembly per experiment".into());
            }
            if spec["settings"]["flex"] == true {
                return Err("settings.flex: flexible links are not derived in process (every link would be rigid, which a flex run must not substitute silently); set flex false, and the result says rigid".into());
            }
            emit(fields([("stage", json!("cad"))]));
            let cad_hash = spec["provenance"]["cad_derivation_hash"].as_str().or(spec["provenance"]["physical_hash"].as_str()).unwrap_or("").to_string();
            let key = digest(&canonical(&json!({"cad_derivation_hash": cad_hash, "mechanical_exclusions": [], "flex": false, "derivation": spec["provenance"]["derivation"]})));
            let cache = self.root.join("cache").join(&key);
            let mut model: Option<Value> = None;
            if let (Ok(bytes), Ok(meta)) = (std::fs::read(cache.join("model.json")), read_json(&cache.join("metadata.json")))
                && meta["content_hash"].as_str() == Some(digest(&bytes).as_str())
            {
                model = serde_json::from_slice(&bytes).ok();
                cache_hit = model.is_some();
            }
            let mut model = match model {
                Some(m) => m,
                None => {
                    let m = derive(&folder.join("model.rcad"), &cad_hash, stop)?;
                    write_json(&cache.join("model.json"), &m)?;
                    let bytes = std::fs::read(cache.join("model.json")).map_err(|e| e.to_string())?;
                    write_json(&cache.join("metadata.json"), &json!({"content_hash": digest(&bytes), "key": key}))?;
                    m
                }
            };
            let overrides = configuration.get("cad_overrides").cloned().unwrap_or_else(|| json!([]));
            evidence["cad_overrides"] = json!(config::cad_overrides(&mut model, &overrides, &location).map_err(|e| located(&location, e))?);
            // Seed zero keeps the CAD seed; every explicit seed selects an independent stream.
            let cad_seed = model["uncertainty"]["seed"].as_u64().unwrap_or(0);
            evidence["cad_seed"] = json!(cad_seed);
            if !model["uncertainty"].is_object() {
                model["uncertainty"] = json!({});
            }
            model["uncertainty"]["seed"] = json!(cad_seed ^ spec["seed"].as_u64().unwrap_or(0));
            evidence["effective_cad_seed"] = model["uncertainty"]["seed"].clone();
            if spec["settings"]["noise"] != true {
                for sensor in model["sensors"].as_array_mut().into_iter().flatten() {
                    let zeroed: Map<String, Value> = sensor["noise"].as_object().into_iter().flatten().map(|(k, _)| (k.clone(), json!(0.0))).collect();
                    sensor["noise"] = Value::Object(zeroed);
                    sensor["bias_walk"] = json!(0.0);
                }
            }
            write_json(&folder.join("physical.json"), &model)?;
            spec["cad"][cads[0].as_str()] = model;
        } else if configuration.get("cad_overrides").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) {
            return Err(located(&location, "cad_overrides requires an imported CAD assembly".into()));
        }
        write_json(&folder.join("configuration.json"), &evidence)?;
        spec["provenance"]["resolved_model_hash"] = json!(digest(&canonical(&spec["cad"])));
        write_json(&folder.join("specification.json"), &spec)?;
        let derive_seconds = derive_started.elapsed().as_secs_f64();
        emit(fields([("stage", json!("compile")), ("cache", json!({"cad_hit": cache_hit})), ("timing", json!({"derive_s": derive_seconds}))]));
        if stop() {
            return Err("cancelled".into());
        }
        let progress = |event: &Value| {
            let mut f: Map<String, Value> = event.as_object().into_iter().flatten().filter(|(k, _)| matches!(k.as_str(), "stage" | "fraction" | "error" | "cache" | "timing")).map(|(k, v)| (k.clone(), v.clone())).collect();
            if let Some(s) = event["state"].as_str().filter(|s| matches!(*s, "building" | "running")) {
                f.insert("state".into(), json!(s));
            }
            emit(f);
        };
        let mut result = sim.run(&spec, &folder, &progress, stop)?;
        result["timing"]["derive_s"] = json!(derive_seconds);
        result["timing"]["component_derive_s"] = json!(0.0);
        result["timing"]["worker_total_s"] = json!(started.elapsed().as_secs_f64());
        result["cache"] = json!({"cad_hit": cache_hit, "derived": {}, "component_derivations": {}});
        result["configuration"] = evidence;
        result["component_graph_mapping"] = json!(graph_mapping);
        result["component_derivations"] = json!([]);
        result["script_component_mapping"] = json!(script_mapping);
        result["limitations"] = json!(LIMITATIONS);
        result["expectations"] = configuration.get("expectations").cloned().unwrap_or_else(|| json!([]));
        // Ids carried through name-keyed summaries: replay maps link members;
        // channel filtering also needs joint and motor ids.
        let mut mapping: Vec<Value> = result["cad_mapping"].as_array().cloned().unwrap_or_default().into_iter().map(|mut m| {
            m["section"] = json!("links");
            m
        }).collect();
        for model in spec["cad"].as_object().into_iter().flatten().map(|(_, m)| m) {
            let link_ids: BTreeMap<String, Vec<String>> = model["links"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| (l["name"].as_str().unwrap_or("").to_string(), l["members"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_else(|| l["id"].as_str().map(|i| vec![i.to_string()]).unwrap_or_default())))
                .collect();
            for section in ["joints", "motors"] {
                for item in model[section].as_array().into_iter().flatten() {
                    let mut related: Vec<String> = Vec::new();
                    for key in ["parent", "child", "mounted_on"] {
                        for nid in item[key].as_str().and_then(|n| link_ids.get(n)).into_iter().flatten() {
                            if !related.contains(nid) {
                                related.push(nid.clone());
                            }
                        }
                    }
                    mapping.push(json!({"section": section, "name": item["name"], "id": item["id"], "related_ids": related}));
                }
            }
        }
        result["cad_mapping"] = json!(mapping);
        result["evaluation"] = if spec["preflight"] == true { json!({"status": "not_simulated", "metrics": []}) } else { results::evaluate_expectations(&result, &result["expectations"].clone())? };
        let mut objectives = Map::new();
        if spec["cad"].as_object().is_some_and(|m| !m.is_empty()) {
            let links: Vec<&Value> = spec["cad"].as_object().into_iter().flatten().flat_map(|(_, m)| m["links"].as_array().into_iter().flatten()).collect();
            let mass = |moving: bool| links.iter().filter(|l| !moving || l["ground"] != true).map(|l| l["mass"].as_f64().unwrap_or(0.0)).sum::<f64>();
            objectives.insert("mass/total".into(), json!({"value": mass(false), "unit": "kg"}));
            objectives.insert("mass/moving".into(), json!({"value": mass(true), "unit": "kg"}));
        }
        for metric in result["evaluation"]["metrics"].as_array().cloned().unwrap_or_default() {
            let definition: Map<String, Value> = metric.as_object().into_iter().flatten().filter(|(k, _)| !matches!(k.as_str(), "value" | "passed" | "samples" | "min" | "max")).map(|(k, v)| (k.clone(), v.clone())).collect();
            objectives.insert(format!("expectation/{}", metric["name"].as_str().unwrap_or("")), json!({"value": metric["value"], "unit": metric["unit"], "definition": definition}));
        }
        result["objectives"] = Value::Object(objectives);
        result["preflight"] = spec["preflight"].clone();
        write_json(&folder.join("result.json"), &result)?;
        Ok(result)
    }

    /// Cancel a run: one this process executes stops at its next check; a
    /// queued one ends now; another editor's is refused by name.
    pub fn cancel(&self, id: &str) -> Result<Value, String> {
        let record = self.get(id)?;
        if records::terminal(&record) {
            return Ok(record);
        }
        let owned = self.leases.lock().map_or(false, |l| l.contains_key(id));
        if !owned {
            return Err("This run is owned by another live editor or worker. Cancel it in the originating editor; history continues to refresh here.".into());
        }
        self.cancelled.lock().map_err(|_| "experiment cancels poisoned")?.insert(id.to_string());
        let running = record["state"] != "queued";
        self.update(id, fields([("state", json!(if running { "cancelling" } else { "cancelled" })), ("stage", json!(if running { "cancelling" } else { "cancelled" }))]))
    }

    /// Whether this process holds the run (it may execute and cancel it).
    pub fn owns(&self, id: &str) -> bool {
        self.leases.lock().map_or(false, |l| l.contains_key(id))
    }

    /// A completed run's result, marked stale when the document moved since
    /// (`current`: the document's snapshot now and its component graph).
    pub fn result(&self, id: &str, current: Option<(&Snapshot, &Value)>) -> Result<Value, String> {
        let record = self.get(id)?;
        let path = self.folder(id)?.join("result.json");
        if record["state"] != "completed" || !path.is_file() {
            return Err("This run has no completed result".into());
        }
        let mut result = read_json(&path)?;
        if let Some((snapshot, graph)) = current {
            let p = &result["provenance"];
            let graph_stale = p["component_graph_hash"].as_str() != Some(digest(&canonical(graph)).as_str());
            let uses_cad = p["uses_cad"].as_bool().unwrap_or(true);
            result["stale"] = json!(graph_stale || (uses_cad && p["physical_hash"].as_str() != Some(snapshot.physical_hash.as_str())));
        }
        Ok(result)
    }

    pub fn inputs(&self, id: &str) -> Result<Value, String> {
        self.get(id)?;
        read_json(&self.folder(id)?.join("input.json"))
    }

    pub fn diagnostics(&self, id: &str) -> Result<Value, String> {
        let record = self.get(id)?;
        let folder = self.folder(id)?;
        let partial = record["state"] != "completed" && (folder.join("result.json").is_file() || folder.join("partial.json").is_file());
        Ok(json!({"run_id": id, "state": record["state"], "error": record["error"], "stderr": "", "partial": partial}))
    }

    /// A failed or cancelled run's retained samples.
    pub fn partial(&self, id: &str) -> Result<Value, String> {
        let record = self.get(id)?;
        if !matches!(record["state"].as_str(), Some("failed" | "cancelled")) {
            return Err("Partial output is only available for failed or cancelled runs".into());
        }
        let folder = self.folder(id)?;
        let path = [folder.join("partial.json"), folder.join("result.json")].into_iter().find(|p| p.is_file()).ok_or("This run retained no partial samples")?;
        let mut result = read_json(&path)?;
        result["partial"] = json!(true);
        result["state"] = record["state"].clone();
        result["error"] = record["error"].clone();
        for name in ["component_graph_mapping", "component_derivations", "script_component_mapping"] {
            if let Ok(v) = read_json(&folder.join(format!("{name}.json"))) {
                result[name] = v;
            }
        }
        if let Some(m) = result.as_object_mut() {
            m.remove("evaluation");
        }
        Ok(result)
    }

    pub fn compare(&self, baseline: &str, candidate: &str) -> Result<Value, String> {
        results::compare(&self.result(baseline, None)?, &self.result(candidate, None)?)
    }

    /// The captured system and controller sources.
    pub fn source_bundles(&self, id: &str) -> Result<Value, String> {
        self.get(id)?;
        let folder = self.folder(id)?;
        let path = if folder.join("composition.json").is_file() { folder.join("composition.json") } else { folder.join("input.json") };
        let captured = read_json(&path)?;
        Ok(json!({"system": captured["system"], "controller": captured["controller"]["sources"]}))
    }

    /// The run's captured archive, when it captured CAD and used it.
    pub fn captured_document(&self, id: &str) -> Result<Option<ArchiveDocument>, String> {
        self.get(id)?;
        let folder = self.folder(id)?;
        if let Ok(plan) = read_json(&folder.join("system.json"))
            && plan["cad"].as_array().is_none_or(Vec::is_empty)
        {
            return Ok(None);
        }
        let path = folder.join("model.rcad");
        if !path.is_file() {
            return Ok(None);
        }
        Ok(Some(ArchiveDocument::open(&path)?))
    }
}

/// The physical model of a captured archive (rigid; the print registry's
/// filament values when it can be read).
pub fn derive(path: &Path, cad_hash: &str, stop: &dyn Fn() -> bool) -> Result<Value, String> {
    let doc = ArchiveDocument::open(path)?;
    let geometry = crate::geometry::load_geometry(&doc, stop, &|_| {})?;
    let masses = crate::mass::derive_document_with(&doc, &geometry, stop, &|_| {})?;
    let registry_path = sim_print::registry::default_path();
    let registry = std::fs::read(&registry_path).ok().and_then(|b| Some((serde_json::from_slice::<Value>(&b).ok()?, digest(&b))));
    let opts = crate::physical::Options {
        planar: None,
        registry: registry.as_ref().map(|(json, sha)| crate::materials::Registry { json, sha256: sha }),
        exported_at: crate::edit::now_iso(),
    };
    let mut model = crate::physical::export(&doc, &geometry, &masses, &opts)?;
    model["source"] = json!({"cad_derivation_hash": cad_hash});
    Ok(model)
}

/// Capture `doc` at `revision` with its component graph (`create`'s inputs).
pub fn snapshot_of(doc: &ArchiveDocument, revision: u64) -> Result<(Snapshot, Value), String> {
    let snapshot = capture::capture(doc, revision)?;
    let graph = doc.manifest.get("component_graph").filter(|g| !g.is_null()).cloned().unwrap_or_else(empty_graph);
    Ok((snapshot, graph))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_graph_lowers_into_a_generated_module() {
        let sources = json!({"entry": "system.rhai", "files": {"system.rhai": "let a = 1;\n"}});
        let graph = json!({"version": 1, "components": {"c1": {"id": "c1", "name": "Motor", "type": "dc_motor", "parameters": {"r": 1.5}}}, "connections": {}});
        let (out, mapping) = compose_sources(&sources, &graph).unwrap();
        assert!(out["files"]["__robocad_graph.rhai"].as_str().unwrap().contains("part(\"graph/c1\", \"dc_motor\", #{\"r\": 1.5});"));
        assert!(out["files"]["system.rhai"].as_str().unwrap().ends_with("import \"__robocad_graph\" as robocad_graph;\n"));
        assert_eq!(mapping[0]["native_name"], "graph/c1");
        assert_eq!(compose_sources(&sources, &empty_graph()).unwrap().0, sources);
    }

    #[test]
    fn sources_refuse_escaping_names() {
        assert!(sources(Some(&json!({"entry": "a.rhai", "files": {"../a.rhai": ""}})), "x", "").is_err());
        assert_eq!(sources(Some(&json!("let x = 1;")), "s.rhai", "").unwrap()["entry"], "s.rhai");
    }
}
