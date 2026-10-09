//! Captured experiments on the CAD model, in process (RoboCAD's
//! `experiments.py`, `experiment_worker.py`, `experiment_config.py`,
//! `experiment_records.py`, `experiment_results.py`, `candidates.py`,
//! `captured_review.py` and the results/identification half of
//! `physical.py`, ported).
//!
//! A run captures the document (a stable archive snapshot, [`capture`]) and
//! its inputs into `runs/experiments/<id>/`, then the host executes it on
//! one of its jobs ([`service::Lab::execute`]): component graph lowered
//! into the Rhai system, the system resolved, the captured archive derived
//! into the physical model (`physical::export`), the run stepped, and the
//! result checked against the script's expectations. The simulator is the
//! host's ([`Simulator`]: `sim_script` resolution and
//! `sim_phenomena::experiment::run`), so this crate stays a CAD library.
//!
//! Candidates ([`candidates`]) are staged edit batches on a snapshot: kept
//! on disk for review, run as experiments, and accepted as one undo step
//! (the host applies the staged archive when the document has not moved).
//!
//! Records are JSON files written atomically; a run's `owner.lock` is held
//! (an OS file lock) while this process executes it, so another editor
//! reading the folder can tell a live run from an interrupted one.
pub mod candidates;
pub mod capture;
pub mod config;
pub mod identification;
pub mod records;
pub mod results;
pub mod review;
pub mod service;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Python's `json.dumps(value, sort_keys=True, separators=(',', ':'))`:
/// compact, keys sorted (serde_json's maps are ordered).
pub fn canonical(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

/// SHA-256 hex of bytes.
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Write JSON atomically (a temporary file renamed over the target).
pub fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    write_bytes(path, &canonical(value))
}

/// Write bytes atomically.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_file_name(format!("{}.{}.tmp", path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(), crate::edit::new_id()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Read a JSON file.
pub fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Seconds since the epoch.
pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// What runs a captured system: the host supplies it (the native viewer:
/// `sim_script` and `sim_phenomena::experiment`).
pub trait Simulator: Send + Sync {
    /// The resolved plan of a composition (`sim-experiment resolve`):
    /// `{cad, components, configuration, configuration_location, …}`.
    fn resolve(&self, composition: &Value) -> Result<Value, String>;
    /// Run a specification into `output` (`sim-experiment run`): the
    /// result. `progress` gets the runner's events.
    fn run(&self, specification: &Value, output: &Path, progress: &dyn Fn(&Value), cancelled: &dyn Fn() -> bool) -> Result<Value, String>;
    /// The registered components (`sim-experiment catalogue`).
    fn catalogue(&self) -> Value;
    /// The runner's identity for provenance (build, version).
    fn identity(&self) -> Value;
}
