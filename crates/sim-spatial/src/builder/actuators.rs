//! Read-only inspector for the accepted actuator registry (build mode).
//!
//! `Builder::actuators_request` is the one entry point: the Actuators tab
//! (registry path field, Reload, consumer path field, Check again),
//! `system_ui` activations of those controls and REST `system_actuators` all
//! call it. It loads the registry and checks consumer files on a worker
//! thread with `sim_runtime::actuator_registry` (discovery, hashing and the
//! staleness rule live there, not here); `finish_actuators` installs the
//! result on the UI thread. Nothing is written: no family is applied,
//! promoted or accepted from here.
use super::*;
use sim_domain_robot::actuator_profile::Parameter;
use sim_runtime::actuator_registry::{ConsumerCheck, Registry};
use std::path::Path;

/// The accepted registry, relative to the workspace root.
pub const DEFAULT_REGISTRY: &str = "examples/actuators/hx30hm/accepted/registry.json";

/// One parameter of a family, as the tab and `system_state` show it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ParameterRow {
    /// motor, driver, controller or envelope.
    pub group: &'static str,
    pub name: String,
    pub value: f64,
    pub unit: String,
    /// measured, derived or estimated (the family file's own label).
    pub provenance: String,
    /// Standard uncertainty; null means unknown, not zero.
    pub uncertainty: Option<f64>,
    /// Evidence key in the family's `evidence` map.
    pub evidence: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct FamilyView {
    pub name: String,
    /// The family JSON the registry names.
    pub file: PathBuf,
    /// Content hash (verified equal to the registry's at load).
    pub content_hash: String,
    /// The registry's acceptance note.
    pub accepted: String,
    pub description: String,
    pub limitations: Vec<String>,
    pub has_envelope: bool,
    pub parameters: Vec<ParameterRow>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RegistryView {
    pub path: PathBuf,
    pub registry_hash: String,
    pub description: String,
    /// Joint-name suffix → accepted family.
    pub roles: BTreeMap<String, String>,
    pub families: Vec<FamilyView>,
}

/// One finished load and check.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Inspection {
    pub registry: RegistryView,
    pub checks: Vec<ConsumerCheck>,
    pub seconds: f64,
}

fn rows(group: &'static str, parameters: impl IntoIterator<Item = (String, Parameter)>) -> impl Iterator<Item = ParameterRow> {
    parameters.into_iter().map(move |(name, p)| ParameterRow {
        group,
        name,
        value: p.value,
        unit: p.unit,
        provenance: serde_json::to_value(&p.provenance).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
        uncertainty: p.uncertainty,
        evidence: p.evidence,
    })
}

/// Load `registry` and check each consumer file against it (the shared
/// library calls; run off the UI thread). Load errors name the registry
/// path; consumer problems are per-file data in the checks.
pub fn inspect(registry: &Path, check: &[PathBuf]) -> Result<Inspection, String> {
    let started = std::time::Instant::now();
    let shown = registry.display().to_string();
    let loaded = Registry::load(registry).map_err(|e| if e.contains(&shown) { format!("Could not load actuator registry: {e}") } else { format!("Could not load actuator registry {shown}: {e}") })?;
    let dir = registry.parent().unwrap_or(Path::new("."));
    let families = loaded.families.iter().map(|(name, family)| {
        let entry = &loaded.file.families[name];
        let family = family.clone();
        let c = family.controller;
        let mut parameters: Vec<ParameterRow> = rows("motor", family.motor).chain(rows("driver", family.driver)).collect();
        parameters.extend(rows("controller", [("period".to_string(), c.period), ("latency".to_string(), c.latency), ("encoder_quantum".to_string(), c.encoder_quantum)]));
        let has_envelope = family.envelope.is_some();
        if let Some(e) = family.envelope {
            parameters.extend(rows("envelope", [("supply_voltage", e.supply_voltage), ("full_drive_speed", e.full_drive_speed), ("acceleration", e.acceleration), ("coast_deceleration", e.coast_deceleration), ("low_speed_friction", e.low_speed_friction), ("breakaway", e.breakaway)].map(|(n, p)| (n.to_string(), p))));
        }
        FamilyView { name: name.clone(), file: dir.join(&entry.path), content_hash: entry.content_hash.clone(), accepted: entry.accepted.clone(), description: family.description, limitations: family.limitations, has_envelope, parameters }
    }).collect();
    let checks = check.iter().map(|p| loaded.check_consumer(p)).collect();
    Ok(Inspection {
        registry: RegistryView { path: loaded.path.clone(), registry_hash: loaded.registry_hash.clone(), description: loaded.file.description.clone(), roles: loaded.file.roles.clone(), families },
        checks,
        seconds: started.elapsed().as_secs_f64(),
    })
}

/// The default registry: `DEFAULT_REGISTRY` under the nearest ancestor of
/// the open system file, else of the working directory, that has it.
pub fn default_registry(system: &Path) -> Result<PathBuf, String> {
    let system = std::path::absolute(system).unwrap_or_else(|_| system.to_path_buf());
    let cwd = std::env::current_dir().unwrap_or_default();
    system.ancestors().skip(1).chain(cwd.ancestors()).map(|d| d.join(DEFAULT_REGISTRY)).find(|p| p.is_file()).ok_or_else(|| {
        format!("No actuator registry found: {DEFAULT_REGISTRY} is not under any folder above {} or {}. Type a registry path.", system.display(), cwd.display())
    })
}

struct Job {
    seq: u64,
    registry: PathBuf,
    receiver: Mutex<mpsc::Receiver<Result<Inspection, String>>>,
}

#[derive(Default)]
pub(super) struct ActuatorState {
    job: Option<Job>,
    seq: u64,
    /// Registry of the latest request (the path field's value).
    pub registry: Option<PathBuf>,
    /// Consumer files of the latest request (rechecked by Reload / Check again).
    pub check: Vec<PathBuf>,
    /// The latest good load, labelled with its own path.
    pub shown: Option<Inspection>,
    /// The latest failure (names the path); cleared by a good load.
    pub error: Option<String>,
    last: Option<(u64, Result<serde_json::Value, String>)>,
}

impl ActuatorState {
    pub fn pending(&self) -> Option<&Path> {
        self.job.as_ref().map(|j| j.registry.as_path())
    }
}

impl Builder {
    /// Load a registry and check consumer files off the UI thread (the
    /// shared path for the Actuators tab, `system_ui` and REST
    /// `system_actuators`). `registry: None` keeps the current one (the
    /// default at first); `check: None` rechecks the previous files.
    /// Refuses while a load is pending. Read-only.
    pub fn actuators_request(&mut self, registry: Option<PathBuf>, check: Option<Vec<PathBuf>>) -> Result<u64, String> {
        let result = self.start_actuators(registry, check);
        if let Err(e) = &result {
            self.actuators.error = Some(e.clone());
            self.action_error = Some(e.clone());
            self.status = e.clone();
        }
        self.panel_dirty = true;
        result
    }

    fn start_actuators(&mut self, registry: Option<PathBuf>, check: Option<Vec<PathBuf>>) -> Result<u64, String> {
        if let Some(pending) = self.actuators.pending() {
            return Err(format!("Still loading actuator registry {}; wait for it before another request.", pending.display()));
        }
        let absolute = |p: PathBuf| std::path::absolute(&p).unwrap_or(p);
        let registry = match registry.or_else(|| self.actuators.registry.clone()) {
            Some(p) => absolute(p),
            None => default_registry(&self.store.path)?,
        };
        let check: Vec<PathBuf> = check.map(|c| c.into_iter().map(absolute).collect()).unwrap_or_else(|| self.actuators.check.clone());
        self.actuators.registry = Some(registry.clone());
        self.actuators.check = check.clone();
        let (send, receive) = mpsc::channel();
        let worker = registry.clone();
        std::thread::spawn(move || {
            let _ = send.send(inspect(&worker, &check));
        });
        self.actuators.seq += 1;
        self.actuators.job = Some(Job { seq: self.actuators.seq, registry: registry.clone(), receiver: Mutex::new(receive) });
        self.status = format!("Loading actuator registry {} in the background…", registry.display());
        Ok(self.actuators.seq)
    }

    /// Stop waiting for a pending load (its result is dropped).
    pub fn cancel_actuators(&mut self) -> bool {
        let Some(job) = self.actuators.job.take() else { return false };
        self.status = format!("Loading actuator registry {} cancelled.", job.registry.display());
        self.actuators.last = Some((job.seq, Err(self.status.clone())));
        self.panel_dirty = true;
        true
    }

    /// Install a finished load. Returns true when one finished.
    pub(crate) fn finish_actuators(&mut self) -> bool {
        let Some(job) = &self.actuators.job else { return false };
        let polled = job.receiver.lock().map(|r| r.try_recv()).unwrap_or(Err(mpsc::TryRecvError::Disconnected));
        let result = match polled {
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err(format!("Could not load actuator registry {}: the loader ended without a result.", job.registry.display())),
            Ok(r) => r,
        };
        let job = self.actuators.job.take().expect("polled above");
        self.panel_dirty = true;
        match result {
            Ok(inspection) => {
                let stale = inspection.checks.iter().filter(|c| !c.is_current()).count();
                self.status = format!(
                    "Actuator registry {}: {} families{}",
                    inspection.registry.path.display(),
                    inspection.registry.families.len(),
                    if inspection.checks.is_empty() { String::new() } else { format!("; {} of {} consumer files current", inspection.checks.len() - stale, inspection.checks.len()) }
                );
                let value = serde_json::to_value(&inspection).unwrap_or_default();
                self.actuators.shown = Some(inspection);
                self.actuators.error = None;
                self.actuators.last = Some((job.seq, Ok(value)));
            }
            Err(e) => {
                self.action_error = Some(e.clone());
                self.status = e.clone();
                self.actuators.error = Some(e.clone());
                self.actuators.last = Some((job.seq, Err(e)));
            }
        }
        true
    }

    /// REST `system_actuators`: start (or refuse) on the first call, then
    /// report the outcome once `finish_actuators` has run.
    pub(crate) fn actuators_rest(&mut self, args: &serde_json::Value, continuation: &mut serde_json::Value, cancelled: bool) -> sim_api::Outcome {
        let Some(seq) = continuation.get("actuators").and_then(|s| s.as_u64()) else {
            let registry = match args.get("registry") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(s)) => Some(PathBuf::from(s)),
                Some(_) => return sim_api::Outcome::Done(Err("system_actuators: registry must be a path string".into())),
            };
            let check = match args.get("check") {
                None | Some(serde_json::Value::Null) => None,
                Some(v) => match serde_json::from_value::<Vec<PathBuf>>(v.clone()) {
                    Ok(c) => Some(c),
                    Err(_) => return sim_api::Outcome::Done(Err("system_actuators: check must be a list of path strings".into())),
                },
            };
            return match self.actuators_request(registry, check) {
                Ok(seq) => {
                    *continuation = serde_json::json!({"actuators": seq});
                    sim_api::Outcome::Pending
                }
                Err(e) => sim_api::Outcome::Done(Err(e)),
            };
        };
        match &self.actuators.last {
            Some((s, result)) if *s == seq => sim_api::Outcome::Done(result.clone()),
            _ if cancelled && self.actuators.job.as_ref().is_some_and(|j| j.seq == seq) => {
                self.cancel_actuators();
                sim_api::Outcome::Done(Err("cancelled".into()))
            }
            _ if self.actuators.job.as_ref().is_none_or(|j| j.seq != seq) => sim_api::Outcome::Done(Err("the actuator request was superseded".into())),
            _ => sim_api::Outcome::Pending,
        }
    }

    pub(super) fn actuators_json(&self) -> serde_json::Value {
        let a = &self.actuators;
        let shown = a.shown.as_ref();
        serde_json::json!({
            "read_only": true,
            "pending": a.pending(),
            "requested": {"registry": a.registry, "check": a.check},
            "registry": shown.map(|s| &s.registry),
            "checks": shown.map(|s| &s.checks),
            "load_seconds": shown.map(|s| s.seconds),
            "error": a.error,
            "last": a.last.as_ref().map(|(seq, r)| serde_json::json!({"seq": seq, "ok": r.is_ok(), "error": r.as_ref().err()})),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(b: &mut Builder) {
        for _ in 0..1200 {
            if b.finish_actuators() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("actuator load did not finish");
    }

    #[test]
    fn actuators_load_check_and_name_bad_paths() {
        let root = std::path::absolute(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
        let system = root.join("examples/systems-builder/motor-driver-board/board.system.json");
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let mut b = Builder::open(system.clone(), root.join("library/systems"), registry).unwrap();
        // Tracked and stale on disk (a truthful historical record); read only.
        let stale = root.join("examples/full-robot/measured-actuator-integration/browser-control-400hz/scene.json");
        let before = std::fs::read(&stale).unwrap();

        // Default registry from the system file's ancestors, loaded off the UI thread.
        let seq = b.actuators_request(None, Some(vec![stale.clone()])).unwrap();
        assert!(b.actuators.pending().is_some());
        let e = b.actuators_request(None, None).unwrap_err();
        assert!(e.contains("Still loading"), "{e}");
        wait(&mut b);
        assert_eq!(b.actuators.last.as_ref().unwrap().0, seq);
        let shown = b.actuators.shown.clone().unwrap();
        assert_eq!(shown.registry.path, root.join(DEFAULT_REGISTRY));
        let knee = shown.registry.families.iter().find(|f| f.name == "hx30hm-knee-measured").unwrap();
        assert!(knee.has_envelope && !knee.limitations.is_empty() && knee.content_hash.len() == 64);
        assert!(knee.parameters.iter().any(|p| p.provenance == "measured"), "{:?}", knee.parameters);
        assert!(knee.parameters.iter().any(|p| p.uncertainty.is_none()));
        assert_eq!(shown.registry.roles["Foot servo output"], "hx30hm-knee-measured");
        let check = &shown.checks[0];
        assert!(!check.is_current() && check.issue.is_none());
        let m = check.models.iter().find_map(|m| m.mismatch.as_ref()).unwrap();
        assert!(m.have_hash.is_some() && m.accepted_hash.is_some() && m.have_hash != m.accepted_hash, "{m:?}");
        let state = b.state_json();
        assert_eq!(state["actuators"]["checks"][0]["models"][0]["status"], "stale");
        assert!(state["actuators"]["registry"]["families"].as_array().unwrap().iter().any(|f| f["parameters"].as_array().unwrap().iter().any(|p| p["uncertainty"].is_null())));

        // A missing registry fails on the worker, names the path, keeps the last good load.
        let missing = root.join("target/no-such-dir/registry.json");
        b.actuators_request(Some(missing.clone()), Some(vec![])).unwrap();
        wait(&mut b);
        let e = b.actuators.error.clone().unwrap();
        assert!(e.contains(&missing.display().to_string()), "{e}");
        assert!(b.status.contains(&missing.display().to_string()));
        assert_eq!(b.actuators.shown.as_ref().unwrap().registry.path, root.join(DEFAULT_REGISTRY));
        assert_eq!(std::fs::read(&stale).unwrap(), before, "checking writes nothing");
    }
}
