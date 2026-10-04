//! Local edits of an owned archive. The archive's bytes stay the one source of
//! truth: an [`Edit`] is a changed manifest plus replaced (or removed) entries,
//! and [`ArchiveDocument::apply`] turns the current archive and an edit into the
//! next archive, re-read exactly as an opened file is. Unknown manifest keys and
//! opaque entries are carried through untouched, so files stay RoboCAD
//! compatible.
use crate::ArchiveDocument;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The working copy of one change: the manifest to write and the entries to
/// replace (`Some(bytes)`) or remove (`None`). Everything else is kept.
#[derive(Clone, Debug)]
pub struct Edit {
    pub manifest: Value,
    pub entries: BTreeMap<String, Option<Vec<u8>>>,
}

impl Edit {
    /// A copy of the document's manifest, nothing replaced yet.
    pub fn of(doc: &ArchiveDocument) -> Self {
        Self { manifest: doc.manifest.clone(), entries: BTreeMap::new() }
    }

    /// Whether any body geometry changes (a new tessellation is needed).
    pub fn changes_geometry(&self) -> bool {
        self.entries.keys().any(|name| name != "manifest.json")
    }

    /// The manifest node `id`, for changing.
    pub fn node_mut(&mut self, id: &str) -> Result<&mut Value, String> {
        self.manifest["nodes"]
            .as_array_mut()
            .ok_or("manifest nodes must be an array")?
            .iter_mut()
            .find(|n| n["id"] == id)
            .ok_or_else(|| format!("part {id} does not exist"))
    }

    /// The manifest node `id`.
    pub fn node(&self, id: &str) -> Option<&Value> {
        self.manifest["nodes"].as_array()?.iter().find(|n| n["id"] == id)
    }

    /// Material `id` of the manifest's material list (RoboCAD stores a list).
    pub fn material_mut(&mut self, id: &str) -> Option<&mut Value> {
        self.manifest["materials"].as_array_mut()?.iter_mut().find(|m| m["id"] == id)
    }

    /// A `{}` object member of the manifest (annotations, saved_views),
    /// created when absent or not an object.
    pub fn object_mut(&mut self, key: &str) -> &mut serde_json::Map<String, Value> {
        if !self.manifest[key].is_object() {
            self.manifest[key] = Value::Object(Default::default());
        }
        self.manifest[key].as_object_mut().expect("just made an object")
    }
}

/// Material `id` of a manifest's material list.
pub fn material<'m>(manifest: &'m Value, id: &str) -> Option<&'m Value> {
    manifest["materials"].as_array()?.iter().find(|m| m["id"] == id)
}

/// The bytes of a new, empty archive (RoboCAD's `Document.save` of a new
/// document): the manifest only, revision 0, the stock materials of `like`
/// (another document's) when given.
pub fn empty_archive(like: Option<&Value>) -> Result<Vec<u8>, String> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0., |d| d.as_secs_f64());
    let mut h = Sha256::new();
    h.update(secs.to_le_bytes());
    h.update(new_id().as_bytes());
    let document_id = format!("{:x}", h.finalize())[..32].to_string();
    let materials = like.map(|m| m["materials"].clone()).filter(Value::is_array).unwrap_or_else(|| serde_json::json!([]));
    let manifest = serde_json::json!({
        "format": "robocad", "version": 1, "saved": secs, "document_id": document_id, "revision": 0,
        "roots": [], "active_group": null, "view": {}, "saved_views": {}, "materials": materials, "nodes": [],
        "annotations": {}, "robot_settings": {}, "component_graph": {"version": 1, "components": {}, "connections": {}}, "results": null,
    });
    let mut written = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut written);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("manifest.json", options).map_err(|e| e.to_string())?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(written.into_inner())
}

/// A new 12-hex-digit id, the form RoboCAD's `Document.new_id` uses.
pub fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut h = Sha256::new();
    h.update(nanos.to_le_bytes());
    h.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    format!("{:x}", h.finalize())[..12].to_string()
}

/// Now in UTC as RoboCAD's `datetime.now(timezone.utc).isoformat()` writes it:
/// `2026-10-03T17:04:05.123456+00:00`.
pub fn now_iso() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    iso_utc(d.as_secs() as i64, d.subsec_micros())
}

/// Seconds since the epoch and microseconds as an ISO 8601 UTC time.
pub fn iso_utc(secs: i64, micros: u32) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{micros:06}+00:00",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

impl ArchiveDocument {
    /// The next archive: this one with `edit` applied, its manifest revision
    /// one higher, re-read (and its embedded components resolved) exactly as
    /// an opened file is. The path stays the same until it is saved elsewhere.
    pub fn apply(&self, mut edit: Edit) -> Result<ArchiveDocument, String> {
        let revision = edit.manifest["revision"].as_u64().unwrap_or(0) + 1;
        edit.manifest["revision"] = Value::from(revision);
        let manifest = serde_json::to_vec_pretty(&edit.manifest).map_err(|e| format!("manifest.json: {e}"))?;
        let mut written = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut written);
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            let mut seen = std::collections::HashSet::new();
            for (name, data) in &self.entries {
                seen.insert(name.clone());
                let data = if name == "manifest.json" {
                    Some(manifest.as_slice())
                } else {
                    match edit.entries.get(name) {
                        Some(Some(new)) => Some(new.as_slice()),
                        Some(None) => None,
                        None => Some(data.as_slice()),
                    }
                };
                if let Some(data) = data {
                    zip.start_file(name.as_str(), options).map_err(|e| format!("{name}: {e}"))?;
                    zip.write_all(data).map_err(|e| format!("{name}: {e}"))?;
                }
            }
            for (name, data) in &edit.entries {
                if let (false, Some(data)) = (seen.contains(name), data) {
                    zip.start_file(name.as_str(), options).map_err(|e| format!("{name}: {e}"))?;
                    zip.write_all(data).map_err(|e| format!("{name}: {e}"))?;
                }
            }
            zip.finish().map_err(|e| format!("archive: {e}"))?;
        }
        ArchiveDocument::from_bytes(&self.path, written.into_inner(), &|| false, &|_| {})
    }

    /// Write this archive's bytes to `path` atomically (a temporary file in
    /// the same folder, then a rename), so a failed save never leaves a
    /// half-written document.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let folder = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let name = path.file_name().ok_or_else(|| format!("{}: not a file path", path.display()))?;
        let temp = folder.join(format!(".{}.saving-{}", name.to_string_lossy(), new_id()));
        std::fs::write(&temp, &self.original_bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
        std::fs::rename(&temp, path).map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            format!("{}: {e}", path.display())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_times_match_python_isoformat() {
        assert_eq!(iso_utc(0, 0), "1970-01-01T00:00:00.000000+00:00");
        assert_eq!(iso_utc(1_790_896_451, 123_456), "2026-10-01T23:14:11.123456+00:00");
        assert_eq!(iso_utc(951_782_400, 0), "2000-02-29T00:00:00.000000+00:00");
    }

    #[test]
    fn ids_are_twelve_hex_digits_and_distinct() {
        let a = new_id();
        let b = new_id();
        assert_eq!(a.len(), 12);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
