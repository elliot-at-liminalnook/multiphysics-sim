use super::*;
use crate::jobs::Pool;
use bevy_settings::SettingsGroup;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
pub(super) const SCHEMA: u32 = 1;
#[derive(Default)]
pub(super) struct Publication {
    pub revision: u64,
    pub expected: Option<Value>,
}
#[derive(Clone)]
pub(super) struct Paths {
    pub unified: Option<PathBuf>,
    pub recent: Option<PathBuf>,
    pub hardware: PathBuf,
}
impl Paths {
    pub fn environment() -> Self {
        let dir = super::super::recent::config_dir();
        Self {
            unified: dir.as_ref().map(|d| {
                d.join(format!(
                    "{}.json",
                    PreferenceGroup::settings_source().unwrap()
                ))
            }),
            recent: dir.map(|d| d.join(super::super::recent::FILE_NAME)),
            hardware: crate::robot::hardware::settings::path(),
        }
    }
}
pub(super) struct Loaded {
    pub raw: Value,
    pub recents: Recents,
    pub hardware: Settings,
    pub cad: CadDefaults,
    pub migrated: bool,
    pub previous: Option<Value>,
}
fn read(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("{}: {e}; preserved, saves blocked", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}; preserved, saves blocked", path.display())),
    }
}
fn version(v: &Value, key: &str, maximum: u32) -> Result<(), String> {
    if let Some(n) = v.get(key) {
        let n = n
            .as_u64()
            .ok_or_else(|| format!("{key}: invalid schema version"))?;
        if n > maximum as u64 {
            return Err(format!(
                "{key}: newer schema {n}, supported {maximum}; saves blocked"
            ));
        }
    }
    if !v.is_object() {
        return Err("preferences must be an object; saves blocked".into());
    }
    Ok(())
}
fn known_document(v: &Value) -> Result<Option<super::super::recent::Recent>, String> {
    let object = v
        .as_object()
        .ok_or("recents.document: expected variant object")?;
    let known: Vec<_> = object
        .iter()
        .filter(|(key, _)| matches!(key.as_str(), "path" | "preset" | "url"))
        .collect();
    if known.len() > 1 {
        return Err("recents.document: multiple known variants".into());
    }
    let Some((key, value)) = known.first() else {
        return Ok(None);
    };
    let mut variant = serde_json::Map::new();
    variant.insert((*key).clone(), (*value).clone());
    serde_json::from_value(Value::Object(variant))
        .map(Some)
        .map_err(|e| format!("recents.document: {e}"))
}
fn recents(v: &Value) -> Result<Recents, String> {
    version(v, "version", super::super::recent::VERSION)?;
    let mut result = Recents::default();
    if let Some(modes) = v.get("modes") {
        let modes = modes.as_object().ok_or("recents.modes: expected object")?;
        for (mode, entries) in modes {
            let entries = entries
                .as_array()
                .ok_or_else(|| format!("recents.modes.{mode}: expected array"))?;
            let mut known = Vec::new();
            for entry in entries {
                // Unknown document variants are retained raw, not presented as
                // an openable path. Malformed known variants refuse migration.
                if let Some(document) = known_document(&entry["document"])? {
                    let opened = entry
                        .get("opened")
                        .and_then(Value::as_u64)
                        .ok_or("recents.opened: expected Unix seconds")?;
                    known.push(super::super::recent::Entry { document, opened });
                }
            }
            known.truncate(super::super::recent::KEEP);
            result.modes.insert(mode.clone(), known);
        }
    }
    Ok(result)
}
pub(super) fn load(paths: &Paths) -> Result<Loaded, String> {
    let existing = match &paths.unified {
        Some(path) => read(path)?,
        None => None,
    };
    let migrated = existing.is_none();
    let previous = existing.clone();
    let raw = if let Some(raw) = existing {
        version(&raw, "schema", SCHEMA)?;
        let source = raw
            .get("hardware_source")
            .and_then(Value::as_str)
            .ok_or("hardware_source missing; saves blocked")?;
        if source != paths.hardware.to_string_lossy() {
            return Err("hardware preference path changed: use a separate SIM_SPATIAL_CONFIG_DIR or restore the original override; existing snapshot preserved".into());
        }
        raw
    } else {
        let r = match &paths.recent {
            Some(path) => read(path)?.unwrap_or_else(|| serde_json::json!({})),
            None => serde_json::json!({}),
        };
        let h = read(&paths.hardware)?.unwrap_or_else(|| serde_json::json!({}));
        serde_json::json!({"schema":SCHEMA,"hardware_source":paths.hardware.to_string_lossy(), PreferenceGroup::settings_group_name(): {"recents":r,"hardware":h,"cad":{}}})
    };
    let group = raw
        .get(PreferenceGroup::settings_group_name())
        .ok_or("preferences group missing")?;
    let r = recents(group.get("recents").ok_or("recents missing")?)?;
    let hraw = group.get("hardware").ok_or("hardware missing")?;
    version(hraw, "version", crate::robot::hardware::settings::VERSION)?;
    let mut h: Settings = serde_json::from_value(hraw.clone())
        .map_err(|e| format!("hardware: {e}; saves blocked"))?;
    h.validate()?;
    let cad: CadDefaults = serde_json::from_value(
        group
            .get("cad")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .map_err(|e| format!("cad: {e}; saves blocked"))?;
    cad.validate()?;
    Ok(Loaded {
        raw,
        recents: r,
        hardware: h,
        cad,
        migrated,
        previous,
    })
}
pub(super) fn load_job(paths: Paths) -> Job<Loaded> {
    Job::spawn(Pool::Io, 0, "viewer preferences load", move |_| {
        load(&paths)
    })
}
/// Overlay known values while preserving arbitrary nested unknown fields.
fn overlay(old: &mut Value, new: Value) {
    match (old, new) {
        (Value::Object(to), Value::Object(from)) => {
            for (k, v) in from {
                overlay(to.entry(k).or_insert(Value::Null), v);
            }
        }
        (Value::Array(to), Value::Array(from)) => {
            let prior = std::mem::take(to);
            *to = from
                .into_iter()
                .enumerate()
                .map(|(i, v)| {
                    let mut old = if v
                        .get("coordinate")
                        .and_then(Value::as_str)
                        .is_some_and(|c| !c.is_empty())
                    {
                        prior
                            .iter()
                            .find(|p| p.get("coordinate") == v.get("coordinate"))
                            .cloned()
                    } else {
                        prior.get(i).cloned()
                    }
                    .unwrap_or(Value::Null);
                    overlay(&mut old, v);
                    old
                })
                .collect();
        }
        (to, from) => *to = from,
    }
}
pub(super) fn snapshot(
    raw: &Value,
    r: &Recents,
    h: &Settings,
    c: &CadDefaults,
) -> Result<Value, String> {
    c.validate()?;
    let mut result = raw.clone();
    if result.get("preserved_source").is_none() {
        result["preserved_source"] = raw.clone();
    }
    let group = &mut result[PreferenceGroup::settings_group_name()];
    let old_r = group["recents"].clone();
    let mut next_r = serde_json::to_value(r).map_err(|e| e.to_string())?;
    if let Some(modes) = next_r["modes"].as_object_mut() {
        for (name, entries) in modes {
            if let (Some(next), Some(old)) =
                (entries.as_array_mut(), old_r["modes"][name].as_array())
            {
                for entry in next.iter_mut() {
                    if let Some(prior) = old.iter().find(|v| {
                        let identity = known_document(&entry["document"]).ok().flatten();
                        identity.is_some()
                            && known_document(&v["document"]).ok().flatten() == identity
                    }) {
                        let mut merged = prior.clone();
                        overlay(&mut merged, entry.clone());
                        *entry = merged;
                    }
                }
                next.extend(
                    old.iter()
                        .filter(|v| known_document(&v["document"]).ok().flatten().is_none())
                        .cloned(),
                );
            }
        }
    }
    // Preserve unknown modes and recents metadata, but don't index-overlay
    // reordered known entries (that could move unknown metadata to another doc).
    if let Some(obj) = next_r.as_object() {
        for (k, v) in obj {
            if k == "modes" {
                if let Some(modes) = v.as_object() {
                    for (mode, entries) in modes {
                        group["recents"]["modes"][mode] = entries.clone();
                    }
                }
            } else {
                group["recents"][k] = v.clone();
            }
        }
    }
    let mut hs = h.clone();
    hs.version = crate::robot::hardware::settings::VERSION;
    overlay(
        &mut group["hardware"],
        serde_json::to_value(hs).map_err(|e| e.to_string())?,
    );
    overlay(
        &mut group["cad"],
        serde_json::to_value(c).map_err(|e| e.to_string())?,
    );
    result["schema"] = SCHEMA.into();
    Ok(result)
}
pub(super) fn shutdown_snapshot(
    raw: &Value,
    mut recents: Recents,
    hardware: &Settings,
    cad: &CadDefaults,
    records: VecDeque<(ViewerMode, Document, u64)>,
) -> Result<Value, String> {
    for (mode, document, now) in records {
        recents.record(mode, &super::super::recent::absolute(&document), now);
    }
    snapshot(raw, &recents, hardware, cad)
}
pub(super) fn save_job(
    paths: Paths,
    snapshot: Value,
    revision: u64,
    gate: Arc<Mutex<Publication>>,
) -> Job<u64> {
    Job::spawn(Pool::Io, revision, "viewer preferences save", move |_| {
        publish_ordered(&paths, &snapshot, revision, &gate)
    })
    .complete_on_drop()
}
pub(super) fn publish_ordered(
    paths: &Paths,
    snapshot: &Value,
    revision: u64,
    gate: &Mutex<Publication>,
) -> Result<u64, String> {
    let mut published = gate
        .lock()
        .map_err(|_| "preference publication gate poisoned")?;
    if published.revision > revision {
        return Ok(published.revision);
    }
    let path = paths
        .unified
        .as_ref()
        .ok_or("No viewer config directory; preferences session-only")?;
    let current = read(path)?;
    if let Some(current) = &current {
        version(current, "schema", SCHEMA)?;
    }
    if current != published.expected {
        return Err(format!(
            "{}: changed since load/publication; preserving file, restart after reviewing it",
            path.display()
        ));
    }
    let result = publish(path, snapshot, revision);
    // A directory-sync error can follow a successful rename. Retain dirty
    // diagnostics, but recognize our bytes so the same revision can retry.
    if result.is_ok() || read(path).ok().flatten().as_ref() == Some(snapshot) {
        published.expected = Some(snapshot.clone());
    }
    result?;
    published.revision = revision;
    Ok(revision)
}
pub(super) fn publish(path: &Path, value: &Value, revision: u64) -> Result<(), String> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = dir.join(format!(
        ".viewer-preferences.{}.{}.tmp",
        std::process::id(),
        revision
    ));
    let mut created = false;
    let mut write = || -> Result<(), String> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        created = true;
        f.write_all(&serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        std::fs::File::open(dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| format!("snapshot renamed but directory durability unconfirmed: {e}"))?;
        Ok(())
    };
    let answer = write();
    if answer.is_err() && created {
        let _ = std::fs::remove_file(&tmp);
    }
    answer.map_err(|e| format!("{}: {e}", path.display()))
}
