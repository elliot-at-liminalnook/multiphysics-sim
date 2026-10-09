//! The in-process editor behind every CAD edit: the methods each panel,
//! tool and form calls (`edit(doc, call, label, |c| c.patch(…))`), with the
//! same names and answers RoboCAD's client had, applied to the archive being
//! edited (`local::Workspace`) through the shared library (`sim_cad::ops`,
//! `sim_cad::sketch`, `sim_cad::references`, `sim_cad::robotics`,
//! `sim_cad::component_edit`, `sim_cad::scripts`, `sim_cad::experiments`).
use super::local::Workspace;
use serde_json::{Map, Value, json};
use crate::cad::types::{CadError, FastenerSpec, History, Material, NewMaterial, NodeDetail, OpResult, Pasted, Ran, ReferenceUpdate, SketchCall};

type R<T> = Result<T, CadError>;

fn err(e: String) -> CadError {
    CadError::local(e)
}
fn op_result(result: Value) -> OpResult {
    OpResult { result, history: History::default(), job: None }
}

impl Workspace<'_> {
    /// `Ops.<name>(*args, **kwargs)` (`sim_cad::ops::run`).
    pub fn op(&mut self, name: &str, args: &[Value], kwargs: &Map<String, Value>) -> R<OpResult> {
        let geometry = self.geometry;
        let centroid = |id: &str| geometry.iter().find(|b| b.node_id == id).map(|b| b.properties.centroid_mm);
        let mut cx = sim_cad::ops::Ctx { doc: self.archive, stamps: self.stamps, edit: &mut self.edit, centroid: &centroid, cancelled: self.cancelled };
        sim_cad::ops::run(&mut cx, name, args, kwargs).map(op_result).map_err(err)
    }
    fn op_kw(&mut self, name: &str, args: Vec<Value>, kwargs: Value) -> R<OpResult> {
        self.op(name, &args, kwargs.as_object().unwrap_or(&Map::new()))
    }

    /// `GET /nodes/{id}` as this edit has the node.
    pub fn node(&mut self, id: &str) -> R<NodeDetail> {
        let n = self.edit.node(id).cloned().ok_or_else(|| err(format!("no node {id}")))?;
        let mut v = n.clone();
        v["body_kind"] = n["body_kind"].clone();
        serde_json::from_value(v).map_err(|e| err(format!("node {id}: {e}")))
    }
    /// `PATCH /nodes/{id}`: name, visible, locked, disabled, material, color,
    /// pivot, tessellation tolerance, parent and index; a sketch or plane value too.
    pub fn patch(&mut self, id: &str, attrs: &Map<String, Value>) -> R<NodeDetail> {
        let mut plain = attrs.clone();
        for key in ["sketch", "plane", "transform"] {
            if let Some(v) = plain.remove(key) {
                let node = self.edit.node_mut(id).map_err(err)?;
                match key {
                    "sketch" => node["sketch"] = sim_cad::sketch::Sketch::from_json(&v).map_err(err)?.json(),
                    "plane" => node["plane"] = sim_cad::sketch::Plane::parse(&v).map_err(err)?.json(),
                    _ => node["transform"] = v,
                }
            }
        }
        if !plain.is_empty() {
            sim_cad::nodes::patch(&mut self.edit, id, &plain).map_err(err)?;
        }
        self.node(id)
    }
    pub fn group(&mut self, ids: &[String], name: &str) -> R<OpResult> {
        self.op_kw("group", vec![json!(ids), json!(name)], json!({}))
    }
    pub fn move_nodes(&mut self, ids: &[String], new_parent: Option<&str>, index: Option<i64>) -> R<OpResult> {
        self.op_kw("move_nodes", vec![json!(ids), json!(new_parent), json!(index)], json!({}))
    }
    pub fn set_active_group(&mut self, group_id: Option<&str>) -> R<OpResult> {
        self.op_kw("set_active_group", vec![json!(group_id)], json!({}))
    }
    pub fn set_locked(&mut self, ids: &[String], locked: bool) -> R<OpResult> {
        self.op_kw("set_locked", vec![json!(ids), json!(locked)], json!({}))
    }
    pub fn set_material(&mut self, ids: &[String], material_id: &str) -> R<OpResult> {
        self.op_kw("set_material", vec![json!(ids), json!(material_id)], json!({}))
    }
    /// A document material (`add_material`): its id is the given one, else from its name.
    pub fn add_material(&mut self, m: &NewMaterial) -> R<Material> {
        if !(m.density.is_finite() && m.density > 0.) {
            return Err(err("density must be a positive number of g/cm³".into()));
        }
        let id = m.id.clone().unwrap_or_else(|| m.name.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect());
        if id.is_empty() {
            return Err(err("a material needs a name".into()));
        }
        if self.edit.material_mut(&id).is_some() {
            return Err(err(format!("a material with id {id} already exists")));
        }
        let material = json!({"id": id, "name": m.name, "density": m.density, "color": m.color.unwrap_or([0.7, 0.7, 0.7]), "roughness": m.roughness.unwrap_or(0.5), "metallic": m.metallic.unwrap_or(0.0), "tags": m.tags.clone().unwrap_or_default(), "engineering": {}});
        if !self.edit.manifest["materials"].is_array() {
            self.edit.manifest["materials"] = json!([]);
        }
        self.edit.manifest["materials"].as_array_mut().expect("array").push(material.clone());
        serde_json::from_value(material).map_err(|e| err(e.to_string()))
    }
    pub fn set_material_props(&mut self, material_id: &str, props: &Map<String, Value>) -> R<OpResult> {
        self.op("set_material_props", &[json!(material_id)], props)
    }
    pub fn set_joint_physics(&mut self, joint_id: &str, overrides: &Map<String, Value>) -> R<OpResult> {
        self.op("set_joint_physics", &[json!(joint_id)], overrides)
    }
    /// RoboCAD's GUI registry commands lived in its desktop window.
    pub fn run_command(&mut self, id: &str) -> R<Ran> {
        Err(err(format!("command {id}: RoboCAD's desktop commands are not part of the in-process editor; use the matching menu entry or cad_run")))
    }

    // ---- sketches ----
    fn sketch_of(&self, id: &str) -> R<sim_cad::sketch::Sketch> {
        let n = self.edit.node(id).ok_or_else(|| err(format!("no node {id}")))?;
        sim_cad::sketch::Sketch::from_json(n.get("sketch").filter(|s| !s.is_null()).ok_or_else(|| err(format!("{} is not a sketch", n["name"].as_str().unwrap_or(id))))?).map_err(err)
    }
    /// `POST /nodes/{id}/sketch`: the calls applied in order to a copy of the sketch.
    pub fn edit_sketch(&mut self, id: &str, calls: &[SketchCall]) -> R<NodeDetail> {
        let mut s = self.sketch_of(id)?;
        for (i, call) in calls.iter().enumerate() {
            s.call(&call.to_json()).map_err(|e| err(if calls.len() > 1 { format!("{e} (call {} of {})", i + 1, calls.len()) } else { e }))?;
        }
        self.edit.node_mut(id).map_err(err)?["sketch"] = s.json();
        self.node(id)
    }
    /// `POST /nodes {"kind": "sketch", "plane", "calls"}`: a new sketch carrying the calls.
    pub fn create_sketch(&mut self, plane: &Value, calls: &[SketchCall], name: Option<&str>) -> R<NodeDetail> {
        let id = self.op_kw("new_sketch", vec![plane.clone()], json!({"name": name.unwrap_or("Sketch")}))?.result.as_str().map(str::to_string).ok_or_else(|| err("no sketch was made".into()))?;
        self.edit_sketch(&id, calls)
    }

    /// `POST /clipboard/paste`: RoboCAD's clipboard (`robocad_clipboard`, items
    /// with node, B-rep hex and sketch) pasted in place.
    pub fn paste(&mut self, clip: &Value) -> R<Pasted> {
        let items = clip["items"].as_array().ok_or_else(|| err("the clipboard has no items".into()))?;
        let mut pasted = Vec::new();
        for item in items {
            let d = &item["node"];
            let name = d["name"].as_str().unwrap_or("Pasted");
            if let Some(hex) = item["brep"].as_str() {
                let bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("zz"), 16)).collect::<Result<Vec<u8>, _>>().map_err(|_| err("the clipboard's B-rep is not hex".into()))?;
                let kind = match d["body_kind"].as_str() { Some("sheet") => sim_cad::kernel::Kind::Sheet, Some("wire") => sim_cad::kernel::Kind::Wire, _ => sim_cad::kernel::Kind::Solid };
                let geometry = self.geometry;
                let centroid = |id: &str| geometry.iter().find(|b| b.node_id == id).map(|b| b.properties.centroid_mm);
                let mut cx = sim_cad::ops::Ctx { doc: self.archive, stamps: self.stamps, edit: &mut self.edit, centroid: &centroid, cancelled: self.cancelled };
                let id = cx.add_built(sim_cad::kernel::Built { kind, brep: bytes }, name, d["material"].as_str(), None).map_err(err)?;
                if !d["color"].is_null() {
                    self.edit.node_mut(&id).map_err(err)?["color"] = d["color"].clone();
                }
                pasted.push(id);
            } else if let Some(s) = item.get("sketch").filter(|s| !s.is_null()) {
                let sk = sim_cad::sketch::Sketch::from_json(s).map_err(err)?;
                let id = self.op_kw("new_sketch", vec![sk.plane.json()], json!({"name": name}))?.result.as_str().map(str::to_string).unwrap_or_default();
                self.edit.node_mut(&id).map_err(err)?["sketch"] = sk.json();
                pasted.push(id);
            } else if d["kind"] == "mesh" {
                return Err(err(format!("{name}: a reference mesh is not pasted; import its file again")));
            }
        }
        Ok(Pasted { pasted, revision: None, history: History::default() })
    }

    // ---- print helpers ----
    pub fn clearance(&mut self, node: &str, faces: &[i64], amount: f64) -> R<OpResult> {
        let faces: Vec<Value> = faces.iter().map(|f| json!({"node": node, "face": f})).collect();
        self.op_kw("clearance", vec![json!(node), json!(faces), json!(amount)], json!({}))
    }
    pub fn fastener_hole(&mut self, node: &str, face: i64, point: [f64; 3], spec: &FastenerSpec) -> R<OpResult> {
        let s = json!({"size": spec.size, "kind": spec.kind, "extra_clearance": spec.extra_clearance, "depth": spec.depth});
        self.op_kw("fastener_hole", vec![json!(node), json!({"node": node, "face": face}), json!(point), s], json!({}))
    }

    // ---- components and the system composition ----
    /// One system-graph edit (RoboCAD's `/system` routes), validated against the document.
    pub fn composition_edit(&mut self, revision: u64, command: &crate::cad::types::composition::GraphCommand, _check: Option<&str>, document_id: Option<&str>) -> R<crate::cad::types::composition::GraphEdit> {
        if document_id.is_some_and(|d| Some(d) != self.archive.manifest["document_id"].as_str()) {
            return Err(err("system.document_id: document changed; draft preserved".into()));
        }
        let command = serde_json::to_value(command).map_err(|e| err(e.to_string()))?;
        let graph = self.edit.manifest["component_graph"].clone();
        let (graph, id) = sim_cad::component_graph::edit(&graph, &self.edit.manifest, &command).map_err(err)?;
        self.edit.manifest["component_graph"] = graph.clone();
        serde_json::from_value(json!({"revision": revision + 1, "document_id": self.archive.manifest["document_id"], "id": id, "graph": graph})).map_err(|e| err(e.to_string()))
    }

    // ---- references and the linked system file ----
    pub fn import_references(&mut self, paths: &[String], plane: Option<&Value>) -> R<OpResult> {
        let plane = plane.map(sim_cad::sketch::Plane::parse).transpose().map_err(err)?;
        sim_cad::references::import(&mut self.edit, paths, plane).map(|ids| op_result(json!(ids))).map_err(err)
    }
    pub fn update_reference(&mut self, node_id: &str, u: &ReferenceUpdate) -> R<OpResult> {
        let up = sim_cad::references::Update {
            width: u.width,
            opacity: u.opacity,
            origin: u.origin,
            plane: u.plane.as_ref().map(sim_cad::sketch::Plane::parse).transpose().map_err(err)?,
            rotation_deg: u.rotation_deg,
            visible: u.visible,
            locked: u.locked,
            name: u.name.clone(),
        };
        sim_cad::references::update(&mut self.edit, node_id, up).map(|()| op_result(json!(node_id))).map_err(err)
    }
    pub fn calibrate_reference(&mut self, node_id: &str, first: [f64; 3], second: [f64; 3], distance: f64) -> R<OpResult> {
        sim_cad::references::calibrate(&mut self.edit, node_id, first, second, distance).map(|()| op_result(json!(node_id))).map_err(err)
    }
    pub fn link_system(&mut self, path: &str) -> R<OpResult> {
        let doc_path = self.archive.path.clone();
        sim_cad::references::link(&mut self.edit, &doc_path, std::path::Path::new(path)).map(op_result).map_err(err)
    }
    pub fn unlink_system(&mut self) -> R<OpResult> {
        let s = self.edit.object_mut("robot_settings");
        if s.get("system").is_none_or(Value::is_null) {
            return Err(err("No system file is linked".into()));
        }
        s.remove("system");
        Ok(op_result(Value::Null))
    }
    pub fn refresh_system_link(&mut self) -> R<OpResult> {
        let link = self.edit.manifest["robot_settings"]["system"].clone();
        let stored = link["path"].as_str().ok_or_else(|| err("No system file is linked".into()))?;
        let path = sim_cad::references::resolve(&self.archive.path, stored);
        let doc_path = self.archive.path.clone();
        sim_cad::references::link(&mut self.edit, &doc_path, &path).map(op_result).map_err(err)
    }

    // ---- motion programs ----
    pub fn save_motion(&mut self, program: &Value, _stamp: &crate::cad::types::ComponentStamp) -> R<Value> {
        self.op_kw("save_motion", vec![program.clone()], json!({})).map(|r| r.result)
    }
    pub fn delete_motion(&mut self, name: &str, _stamp: &crate::cad::types::ComponentStamp) -> R<Value> {
        self.op_kw("delete_motion", vec![json!(name)], json!({})).map(|r| r.result)
    }

    /// File > Import: not yet in process (STEP, IGES and meshes need the exchange toolkits).
    /// `POST /import` (`sim_cad::import`): STEP/IGES bodies, an SVG sketch or
    /// a reference image; meshes are refused by name.
    pub fn import(&mut self, path: &str, unit: Option<&str>) -> R<crate::cad::types::Imported> {
        let geometry = self.geometry;
        let centroid = |id: &str| geometry.iter().find(|b| b.node_id == id).map(|b| b.properties.centroid_mm);
        let mut cx = sim_cad::ops::Ctx { doc: self.archive, stamps: self.stamps, edit: &mut self.edit, centroid: &centroid, cancelled: self.cancelled };
        let ids = sim_cad::import::import_file_in(&mut cx, path, unit).map_err(err)?;
        Ok(crate::cad::types::Imported { imported: ids.into_iter().map(Value::from).collect() })
    }

    // ---- experiments (RoboCAD's experiment service) ----
    /// Accept a draft candidate: the edit becomes the candidate's staged
    /// state (one undo step), refused when the document moved since its base.
    pub fn accept_candidate(&mut self, id: &str, document_id: &str, revision: u64) -> R<crate::cad::types::candidates::CandidateRecord> {
        if self.archive.manifest["document_id"].as_str().is_some_and(|d| d != document_id) {
            return Err(err("Document replaced before acceptance".into()));
        }
        let (_, edit) = super::lab::accept(self.archive, revision, id, revision).map_err(err)?;
        self.edit = edit;
        let record = super::lab::mark_accepted(id, document_id, revision + 1).map_err(err)?;
        serde_json::from_value(record).map_err(|e| err(e.to_string()))
    }
    /// `POST /doc/batch`: operations `{op, args, kwargs, as}` applied in order as
    /// one edit (all or nothing); `{"$ref": alias}` names an earlier result.
    pub fn batch(&mut self, r: &crate::cad::types::candidates::CandidateRequest) -> R<Value> {
        if self.archive.manifest["document_id"].as_str().is_some_and(|d| d != r.document_id) {
            return Err(err("Document replaced before the batch".into()));
        }
        if r.expected_revision != self.archive.manifest["revision"].as_u64().unwrap_or(0) {
            return Err(err(format!("the document is at revision {}, not {}: read it again", self.archive.manifest["revision"], r.expected_revision)));
        }
        if r.operations.is_empty() {
            return Err(err("An edit batch requires a nonempty operations array".into()));
        }
        fn resolve(v: &Value, outputs: &Map<String, Value>) -> Result<Value, String> {
            Ok(match v {
                Value::Object(o) if o.len() == 1 && o.contains_key("$ref") => {
                    let k = o["$ref"].as_str().unwrap_or("");
                    outputs.get(k).cloned().ok_or_else(|| format!("Unknown prior operation reference {k}"))?
                }
                Value::Object(o) => Value::Object(o.iter().map(|(k, v)| resolve(v, outputs).map(|v| (k.clone(), v))).collect::<Result<_, _>>()?),
                Value::Array(a) => Value::Array(a.iter().map(|v| resolve(v, outputs)).collect::<Result<_, _>>()?),
                other => other.clone(),
            })
        }
        let mut outputs = Map::new();
        for (i, operation) in r.operations.iter().enumerate() {
            let o = operation.as_object().filter(|o| o.keys().all(|k| ["op", "args", "kwargs", "as"].contains(&k.as_str()))).ok_or_else(|| err(format!("Operation {i}: expected op, args, kwargs and optional as")))?;
            let name = o.get("op").and_then(Value::as_str).ok_or_else(|| err(format!("Operation {i}: missing op")))?;
            let alias = o.get("as").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| i.to_string());
            if outputs.contains_key(&alias) {
                return Err(err(format!("Operation {i}: duplicate or invalid result alias")));
            }
            let args = resolve(o.get("args").unwrap_or(&json!([])), &outputs).map_err(err)?;
            let kwargs = resolve(o.get("kwargs").unwrap_or(&json!({})), &outputs).map_err(err)?;
            let out = self.op(name, args.as_array().map(Vec::as_slice).unwrap_or(&[]), kwargs.as_object().unwrap_or(&Map::new())).map_err(|e| err(format!("Operation {i} ({name}): {}", e.message)))?;
            outputs.insert(alias, out.result);
        }
        Ok(json!({"results": outputs, "label": r.label}))
    }
    /// The accepted actuator profiles (`robot_settings.actuator_profiles`).
    pub fn set_actuator_profiles(&mut self, profiles: &Value) -> R<Value> {
        if !(profiles.is_object() || profiles.is_null()) {
            return Err(err("actuator profiles must be an object of profiles by name".into()));
        }
        self.op_kw("set_robot_setting", vec![json!("actuator_profiles"), profiles.clone()], json!({}))?;
        Ok(profiles.clone())
    }
    /// `POST /results/load`: a results file hung on its nodes (stale when
    /// it describes another physical state).
    pub fn load_results(&mut self, path: &str) -> R<Value> {
        let hash = sim_cad::experiments::capture::capture(self.archive, 0).map_err(err)?.physical_hash;
        sim_cad::experiments::identification::load_results(&mut self.edit, std::path::Path::new(path), &hash).map_err(err)
    }
    /// `POST /identification/apply`: fitted joint parameters stored; a joint
    /// the accepted actuator registry covers points at its family instead
    /// of taking copied motor numbers.
    pub fn apply_identification(&mut self, path: &str) -> R<Value> {
        let registry = sim_runtime::actuator_registry::Registry::load(&sim_runtime::actuator_registry::default_path()).ok();
        let covered = |joint: &str| -> Option<(String, Value)> {
            let r = registry.as_ref()?;
            r.role_family(joint).ok().map(|f| (f.to_string(), r.identity()))
        };
        sim_cad::experiments::identification::apply_identification(&mut self.edit, std::path::Path::new(path), &covered).map_err(err)
    }
    /// `POST /doc/script`: a repository `.rhai` model script run on this
    /// edit (`sim_cad::scripts`): one undo step; with `changes`, what changed.
    pub fn model_script(&mut self, r: &Value) -> R<Value> {
        if r["document_id"].as_str().is_some_and(|d| Some(d) != self.archive.manifest["document_id"].as_str()) {
            return Err(err("Document replaced while the model script was prepared".into()));
        }
        let path = r["path"].as_str().ok_or_else(|| err("model script: give path (a .rhai file in the repository)".into()))?;
        let doc = std::sync::Arc::new(self.archive.clone());
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new((self.cancelled)()));
        let (edit, summary) = sim_cad::scripts::stage(doc, path, &r["params"], r["replace"].as_bool().unwrap_or(true), stop).map_err(err)?;
        let mut out = summary.json();
        if r["changes"] == true {
            let before = sim_cad::experiments::capture::capture(self.archive, 0).map_err(err)?;
            let after = sim_cad::experiments::capture::capture(&self.archive.apply(edit.clone()).map_err(err)?, 1).map_err(err)?;
            out["changes"] = sim_cad::experiments::candidates::change_set(&before, &after).map_err(err)?;
        }
        self.edit = edit;
        Ok(out)
    }
    /// Restore a run's captured component graph (one undo step); its
    /// source bundles are answered for the editor, never written to files.
    pub fn restore_experiment_inputs(&mut self, id: &str, document_id: &str, _revision: u64) -> R<Value> {
        if self.archive.manifest["document_id"].as_str().is_some_and(|d| d != document_id) {
            return Err(err("Document changed; retained draft cannot publish here".into()));
        }
        let spec = super::lab::lab().inputs(id).map_err(err)?;
        if let Some(graph) = spec.get("component_graph").filter(|g| !g.is_null()) {
            self.edit.manifest["component_graph"] = sim_cad::component_graph::validate(graph, &self.edit.manifest).map_err(err)?;
        }
        Ok(json!({"inputs": spec}))
    }
}
