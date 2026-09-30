//! Whether an exported `.simrobot.json` still matches the CAD file it names.
//! RoboCAD records `source.file` and `source.cad_sha256` (sha256 of the raw
//! `.rcad` bytes) at export; this compares them with the file on disk. Read
//! only: nothing is written and no CAD process is contacted.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// How `source.file` is resolved, stated wherever the status is shown.
pub const RESOLUTION_RULE: &str = "an absolute source.file is used as is; a relative one is tried against the simrobot file's directory and then each ancestor directory, and the first existing file wins";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CadLinkStatus {
    /// The on-disk sha256 equals the recorded one.
    Current { path: PathBuf, sha256: String, tried: Vec<PathBuf> },
    /// The CAD file changed since the export.
    Stale { path: PathBuf, recorded_sha256: String, on_disk_sha256: String, tried: Vec<PathBuf> },
    /// No candidate path exists.
    Missing { file: String, tried: Vec<PathBuf> },
    /// The file exists but the export recorded no hash to compare with.
    NoRecordedHash { path: PathBuf, on_disk_sha256: String, tried: Vec<PathBuf> },
    /// The export names no source file.
    NoSourceFile,
    /// A candidate exists but could not be read.
    Unreadable { path: PathBuf, error: String, tried: Vec<PathBuf> },
}

/// Lowercase hex sha256 of a file's raw bytes.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// The paths [`RESOLUTION_RULE`] tries, in order.
pub fn candidates(simrobot: &Path, file: &str) -> Vec<PathBuf> {
    let file = Path::new(file);
    if file.is_absolute() {
        return vec![file.to_path_buf()];
    }
    let simrobot = std::path::absolute(simrobot).unwrap_or_else(|_| simrobot.to_path_buf());
    simrobot.parent().map(|d| d.ancestors().map(|a| a.join(file)).collect()).unwrap_or_default()
}

/// Status of the CAD link recorded in an export's `source` block. Hashes the
/// file, so call it off the UI thread.
pub fn status(simrobot: &Path, source: &serde_json::Value) -> CadLinkStatus {
    let Some(file) = source.get("file").and_then(|f| f.as_str()).filter(|f| !f.is_empty()) else {
        return CadLinkStatus::NoSourceFile;
    };
    let tried = candidates(simrobot, file);
    let Some(path) = tried.iter().find(|p| p.is_file()).cloned() else {
        return CadLinkStatus::Missing { file: file.to_string(), tried };
    };
    let on_disk = match sha256_file(&path) {
        Ok(h) => h,
        Err(e) => return CadLinkStatus::Unreadable { path, error: e.to_string(), tried },
    };
    match source.get("cad_sha256").and_then(|h| h.as_str()) {
        None => CadLinkStatus::NoRecordedHash { path, on_disk_sha256: on_disk, tried },
        Some(recorded) if recorded.eq_ignore_ascii_case(&on_disk) => CadLinkStatus::Current { path, sha256: on_disk, tried },
        Some(recorded) => CadLinkStatus::Stale { path, recorded_sha256: recorded.to_string(), on_disk_sha256: on_disk, tried },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cad_link_status_current_stale_missing_and_unrecorded() {
        let root = std::env::temp_dir().join(format!("cad-link-{}", std::process::id()));
        let export_dir = root.join("exports/robot");
        std::fs::create_dir_all(&export_dir).unwrap();
        std::fs::write(root.join("robot.rcad"), b"abc").unwrap();
        let simrobot = export_dir.join("robot.simrobot.json");
        // sha256("abc")
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let relative = |hash: Option<&str>| json!({"file": "robot.rcad", "cad_sha256": hash});

        // A relative path resolves through the ancestors; the first existing wins.
        match status(&simrobot, &relative(Some(abc))) {
            CadLinkStatus::Current { path, sha256, tried } => {
                assert_eq!(path, root.join("robot.rcad"));
                assert_eq!(sha256, abc);
                assert_eq!(&tried[..3], &[export_dir.join("robot.rcad"), root.join("exports/robot.rcad"), root.join("robot.rcad")]);
            }
            other => panic!("{other:?}"),
        }
        let recorded = "0".repeat(64);
        assert!(matches!(status(&simrobot, &relative(Some(&recorded))),
            CadLinkStatus::Stale { recorded_sha256, on_disk_sha256, .. } if recorded_sha256 == recorded && on_disk_sha256 == abc));
        let absolute = json!({"file": root.join("robot.rcad")});
        assert!(matches!(status(&simrobot, &absolute), CadLinkStatus::NoRecordedHash { on_disk_sha256, tried, .. } if on_disk_sha256 == abc && tried.len() == 1));
        let gone = json!({"file": root.join("gone.rcad"), "cad_sha256": abc});
        assert_eq!(status(&simrobot, &gone), CadLinkStatus::Missing { file: root.join("gone.rcad").to_string_lossy().into(), tried: vec![root.join("gone.rcad")] });
        assert_eq!(status(&simrobot, &json!({"exported": "x"})), CadLinkStatus::NoSourceFile);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
