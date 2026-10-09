//! Run records (RoboCAD's `experiment_records.py`): `run.json` updated
//! atomically under `record.lock`, and `owner.lock` held by whoever executes
//! the run. A record that is not terminal while nobody holds its owner lock
//! was interrupted (its editor exited): reading it marks it failed, with
//! its captured inputs and partial outputs kept.
use super::{now, read_json, write_json};
use serde_json::{Map, Value, json};
use std::fs::File;
use std::path::Path;

pub const TERMINAL: [&str; 3] = ["completed", "failed", "cancelled"];

pub fn terminal(record: &Value) -> bool {
    record["state"].as_str().is_some_and(|s| TERMINAL.contains(&s))
}

/// An exclusive OS lock on a file, released when dropped.
pub struct Lease {
    _file: File,
}

impl Lease {
    /// The lock at `path`: waiting for it (`blocking`), else None when held elsewhere.
    pub fn acquire(path: &Path, blocking: bool) -> Result<Option<Lease>, String> {
        let file = std::fs::OpenOptions::new().create(true).append(true).read(true).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if blocking {
            file.lock().map_err(|e| format!("{}: {e}", path.display()))?;
        } else {
            match file.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
                Err(std::fs::TryLockError::Error(e)) => return Err(format!("{}: {e}", path.display())),
            }
        }
        Ok(Some(Lease { _file: file }))
    }
}

/// Merge `fields` into the record (a terminal record is final; a
/// cancelling one only moves to cancelled).
pub fn update(folder: &Path, fields: Map<String, Value>) -> Result<Value, String> {
    let _lease = Lease::acquire(&folder.join("record.lock"), true)?;
    let mut record = read_json(&folder.join("run.json"))?;
    if terminal(&record) {
        return Ok(record);
    }
    let next = fields.get("state").and_then(Value::as_str);
    if record["state"] == "cancelling" && !matches!(next, None | Some("cancelling") | Some("cancelled")) {
        return Ok(record);
    }
    for (k, v) in fields {
        record[k.as_str()] = v;
    }
    record["updated_at"] = json!(now());
    write_json(&folder.join("run.json"), &record)?;
    Ok(record)
}

/// The record as it stands; an orphaned live record is marked interrupted.
pub fn observe(folder: &Path) -> Result<Value, String> {
    let record = read_json(&folder.join("run.json"))?;
    if terminal(&record) {
        return Ok(record);
    }
    match Lease::acquire(&folder.join("owner.lock"), false)? {
        None => Ok(record),
        Some(_lease) => {
            let mut f = Map::new();
            f.insert("state".into(), json!("failed"));
            f.insert("stage".into(), json!("interrupted"));
            f.insert("error".into(), json!("The editor and worker exited before finalizing this run. Captured inputs and partial outputs are retained; restore inputs to start a new run."));
            update(folder, f)
        }
    }
}

/// `fields` from pairs.
pub fn fields<const N: usize>(pairs: [(&str, Value); N]) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}
