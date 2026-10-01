//! Robot FILE mode's read-only stress overlay (`--robot FILE`): the model's
//! `.simresult.json` (`sim_runtime::physical::results_path`) is read on a
//! worker together with every source check (open, watch, manual reload;
//! `robot::source::check`) and again when the overlay is switched on. The UI
//! thread only compares hashes and colours link meshes from the parsed
//! hotspots through the shared rule (`sim_domain_robot::stress_results`).
//! Nothing is written; nothing is computed from physics here.
use serde_json::{Value, json};
use sim_domain_robot::PhysicalModel;
use sim_domain_robot::stress_results::{self, Hotspot};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const RULE: &str = "read-only: <stem>.simresult.json beside the opened .simrobot.json (sim_runtime::physical::results_path), read on a worker with every open, watched or manual reload check of the model file and when the stress overlay is switched on (a results file written later is picked up by toggling stress or Reload). Status: current when the results' provenance.physical_hash equals the loaded model's source.physical_hash, stale when both exist and differ, no recorded hash when either is absent. Each vertex of a link with hotspot cells takes the nearest cell's peak stress normalised by the link material's yield (sim_domain_robot::stress_results::stress_colour, the shared rule); links without cells keep the normal colour. A missing or invalid file colours nothing.";

/// What the worker found at the results path.
#[derive(Clone, Debug)]
pub enum Contents {
    /// No file at the path.
    Missing,
    /// Unreadable or not JSON; the message names the path.
    Invalid(String),
    Parsed(Value),
}

/// One read of the results file.
#[derive(Clone, Debug)]
pub struct StressResults {
    pub path: PathBuf,
    /// The file's modification time, unix seconds (None when missing or not reported).
    pub mtime_unix_s: Option<f64>,
    pub contents: Contents,
}

/// Reads `results_path(model_path)` (called on a worker).
pub fn read(model_path: &Path) -> StressResults {
    let path = PathBuf::from(sim_runtime::physical::results_path(&model_path.to_string_lossy()));
    let mtime_unix_s = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs_f64());
    let contents = match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Contents::Missing,
        Err(e) => Contents::Invalid(format!("{}: {e}", path.display())),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(v) if v.is_object() => Contents::Parsed(v),
            Ok(_) => Contents::Invalid(format!("{}: not a results document (expected a JSON object)", path.display())),
            Err(e) => Contents::Invalid(format!("{}: {e}", path.display())),
        },
    };
    StressResults { path, mtime_unix_s, contents }
}

impl StressResults {
    pub fn parsed(&self) -> Option<&Value> {
        match &self.contents {
            Contents::Parsed(v) => Some(v),
            _ => None,
        }
    }
    /// current | stale | no recorded hash | no results file | invalid results file.
    pub fn status(&self, model: &PhysicalModel) -> &'static str {
        match &self.contents {
            Contents::Missing => "no results file",
            Contents::Invalid(_) => "invalid results file",
            Contents::Parsed(v) => stress_results::staleness(stress_results::recorded_physical_hash(v), model_hash(model)),
        }
    }
    /// Per vertex colours of link `index` (vertices in the link frame, as
    /// `Collision::display_triangles`); None when the link has no hotspot cells.
    pub fn colours(&self, model: &PhysicalModel, index: usize, positions: &[[f32; 3]]) -> Option<Vec<[f32; 4]>> {
        let link = model.links.get(index)?;
        let hotspot = Hotspot::from_results(self.parsed()?, &link.name)?;
        let yield_strength = model.material_of(link).yield_strength;
        Some(positions.iter().map(|p| stress_results::stress_colour(&hotspot, yield_strength, [p[0] as f64, p[1] as f64, p[2] as f64])).collect())
    }
    /// `robot_state.overlays.stress` (without `enabled`, added by the caller).
    pub fn json(&self, model: &PhysicalModel) -> Value {
        let (error, peaks) = match &self.contents {
            Contents::Missing => (None, Value::Null),
            Contents::Invalid(e) => (Some(e.clone()), Value::Null),
            Contents::Parsed(v) => (None, stress_results::peaks(v).into_iter().map(|(k, p)| (k, json!(p))).collect::<serde_json::Map<_, _>>().into()),
        };
        let hotspot_links: Vec<&str> = model.links.iter().filter(|l| self.parsed().and_then(|v| Hotspot::from_results(v, &l.name)).is_some()).map(|l| l.name.as_str()).collect();
        json!({"path": self.path, "mtime_unix_s": self.mtime_unix_s, "mtime_utc": self.mtime_unix_s.map(|t| crate::robot::recording::iso((t * 1e3) as u128)), "status": self.status(model), "absent": matches!(self.contents, Contents::Missing), "error": error,
            "recorded_physical_hash": self.parsed().and_then(stress_results::recorded_physical_hash), "model_physical_hash": model_hash(model),
            "peak_stress_pa": peaks, "hotspot_links": hotspot_links, "scale": stress_results::SCALE, "rule": RULE})
    }
}

pub fn model_hash(model: &PhysicalModel) -> Option<&str> {
    model.source.get("physical_hash").and_then(Value::as_str)
}

/// The stress overlay's UI-side state (RobotView).
#[derive(Default)]
pub struct StressOverlay {
    pub enabled: bool,
    pub results: Option<StressResults>,
    /// A results-only read started by switching the overlay on.
    pending: Option<crate::jobs::Job<StressResults>>,
    /// Bumped whenever the mesh colours must be recomputed (toggle, new results, new meshes).
    pub revision: u64,
    /// The revision the link meshes were last painted for, and the seconds that took.
    pub painted: Option<(u64, f64)>,
}
impl StressOverlay {
    /// Starts a results-only read of the file beside `model_path` on a worker.
    pub fn refresh(&mut self, model_path: &Path) {
        let path = model_path.to_path_buf();
        self.pending = Some(crate::jobs::Job::spawn(crate::jobs::Pool::Io, 0, "the stress results reader", move |_| Ok(read(&path))));
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    /// Takes a finished results-only read (never blocks); true when applied.
    pub fn take(&mut self) -> bool {
        let Some(got) = self.pending.as_ref().and_then(crate::jobs::Job::poll) else { return false };
        self.pending = None;
        match got {
            Ok(r) => {
                self.set(r);
                true
            }
            Err(e) => {
                bevy::log::warn!("{e}");
                false
            }
        }
    }
    pub fn set(&mut self, results: StressResults) {
        self.results = Some(results);
        self.revision += 1;
    }
    /// Whether link meshes show stress colours (on, and a parsed results file).
    pub fn painting(&self) -> bool {
        self.enabled && self.results.as_ref().is_some_and(|r| r.parsed().is_some())
    }
    /// `robot_state.overlays.stress`.
    pub fn json(&self, model: Option<&PhysicalModel>) -> Value {
        let mut out = match (self.results.as_ref(), model) {
            (Some(r), Some(m)) => r.json(m),
            _ => json!({"status": "not read yet", "rule": RULE}),
        };
        out["available"] = json!(true);
        out["enabled"] = json!(self.enabled);
        out["painting"] = json!(self.painting());
        out["reading"] = json!(self.busy());
        out["paint_seconds"] = json!(self.painted.map(|p| p.1));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_domain_robot::stress_results::stress_colour;

    /// Current (hash equal), stale (hash differs) and missing (no file, no
    /// colours) results beside a temp copy of the wheeled robot, and a known
    /// vertex coloured exactly by the shared rule.
    #[test]
    fn current_stale_and_missing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let original = std::fs::read_to_string(root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap();
        let dir = std::env::temp_dir().join(format!("sim-spatial-robot-stress-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("robot.simrobot.json");
        let mut raw: Value = serde_json::from_str(&original).unwrap();
        raw["source"]["physical_hash"] = json!("hash-a");
        std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        let loaded = crate::robot::load(&path).unwrap();
        let model = &loaded.model;
        let (li, link) = model.links.iter().enumerate().find(|(i, _)| loaded.geometry[*i].is_some()).unwrap();
        let results_file = dir.join("robot.simresult.json");

        // Missing: says so with the path, colours nothing.
        let missing = read(&path);
        assert!(matches!(missing.contents, Contents::Missing));
        assert_eq!(missing.path, results_file);
        assert_eq!(missing.status(model), "no results file");
        let positions = &loaded.geometry[li].as_ref().unwrap().positions;
        assert!(missing.colours(model, li, positions).is_none());
        let j = missing.json(model);
        assert_eq!((j["absent"].as_bool(), j["path"].as_str()), (Some(true), Some(results_file.to_str().unwrap())));

        // Current: the recorded hash equals the model's.
        let p = positions[0];
        let cells = json!([[p[0] as f64 + 0.001, p[1] as f64, p[2] as f64], [1.0, 1.0, 1.0]]);
        let doc = |hash: Value| json!({"version": 1, "provenance": if hash.is_null() { Value::Null } else { json!({"physical_hash": hash}) },
            "links": {link.name.clone(): {"peak_stress_pa": 2.5e6, "hotspot": {"cells": cells, "stress_pa": [2.5e6, 1.0e3]}}}});
        std::fs::write(&results_file, serde_json::to_vec(&doc(json!("hash-a"))).unwrap()).unwrap();
        let current = read(&path);
        assert_eq!(current.status(model), "current");
        let j = current.json(model);
        assert_eq!(j["peak_stress_pa"][&link.name].as_f64(), Some(2.5e6));
        assert_eq!((j["recorded_physical_hash"].as_str(), j["model_physical_hash"].as_str()), (Some("hash-a"), Some("hash-a")));
        assert!(j["mtime_unix_s"].as_f64().is_some_and(|t| t > 0.0));
        let colours = current.colours(model, li, positions).expect("the link has hotspot cells");
        assert_eq!(colours.len(), positions.len());
        let hotspot = Hotspot { cells: vec![[p[0] as f64 + 0.001, p[1] as f64, p[2] as f64], [1.0, 1.0, 1.0]], stress_pa: vec![2.5e6, 1.0e3] };
        let expected = stress_colour(&hotspot, model.material_of(link).yield_strength, [p[0] as f64, p[1] as f64, p[2] as f64]);
        assert_eq!(colours[0], expected);
        // The nearest cell (2.5 MPa) decides the colour, not the far one.
        let ratio = (2.5e6 / model.material_of(link).yield_strength.max(1.0)).max(1e-6);
        assert_eq!(expected, stress_results::colormap(((ratio.log10() + 3.0) / 3.0) as f32));

        // Stale: both hashes present and different.
        std::fs::write(&results_file, serde_json::to_vec(&doc(json!("hash-b"))).unwrap()).unwrap();
        assert_eq!(read(&path).status(model), "stale");
        // No recorded hash: provenance null.
        std::fs::write(&results_file, serde_json::to_vec(&doc(Value::Null)).unwrap()).unwrap();
        assert_eq!(read(&path).status(model), "no recorded hash");
        // Invalid: an error naming the path, no colours.
        std::fs::write(&results_file, b"{\"links\": [").unwrap();
        let invalid = read(&path);
        assert_eq!(invalid.status(model), "invalid results file");
        assert!(invalid.json(model)["error"].as_str().unwrap().contains(results_file.to_str().unwrap()));
        assert!(invalid.colours(model, li, positions).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
