//! The Leg calibration panel's persisted display preferences: one JSON file
//! (`path()`: `$SIM_SPATIAL_PREFERENCES`, else
//! `$HOME/.config/sim-spatial/hardware-preferences.json`). It replaces the
//! browser pages' `localStorage` keys:
//!
//! - `calibration-drive-mode` and `calibration-hold-others`
//!   (web/viewer/calibration-ui.mjs:199-202) → [`CalibrationPrefs`];
//! - `calibration-mirror-v1` (calibration-mirror.mjs:8,18,46) → [`MirrorSettings`];
//! - `walking-hardware-map-v1` (hardware-sync.mjs:19,30) → [`SyncSettings`].
//!
//! Display and form preferences only: never calibration data, taught poses,
//! alignments or limits (those live on the calibration server and in CAD).
//!
//! - **Tolerant reading.** Every field defaults and unknown fields are
//!   ignored. A missing or unreadable file gives the defaults; a file that is
//!   not valid JSON for this shape gives the defaults and is left as it is
//!   (its bytes are copied aside, once, to `<file>.corrupt.json` before the
//!   first save replaces it).
//! - **Writing off the UI thread.** [`Settings::save`] hands a snapshot to a
//!   `jobs::Pool::Io` job (`complete_on_drop`), which writes a temporary file
//!   beside the target and renames it over (atomic on one file system). Saves
//!   are numbered; an older save that runs after a newer one is skipped, so
//!   the file always ends with the newest preferences.
use super::actions::{Align, DriveMode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// The file's format version (written on every save).
pub const VERSION: u32 = 1;
/// Overrides the preferences file (tests, several viewers side by side).
pub const PATH_VARIABLE: &str = "SIM_SPATIAL_PREFERENCES";

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub calibration: CalibrationPrefs,
    pub mirror: MirrorSettings,
    pub sync: SyncSettings,
}

/// `calibration-drive-mode`, `calibration-hold-others` (None: the page's initial value).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct CalibrationPrefs {
    pub drive_mode: Option<DriveMode>,
    pub hold_others: Option<bool>,
}

/// `calibration-mirror-v1`: the mirror's `settings` (calibration-mirror.mjs:19).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct MirrorSettings {
    /// "Show the real leg on the suspended simulated robot" (default on).
    pub enabled: bool,
    /// "+X", "-X", "+Y" or "-Y" (default "+X").
    pub leg: String,
    /// Per motor ID: its CAD joint, sign and alignment pose.
    pub bindings: BTreeMap<u8, MirrorBinding>,
}
impl Default for MirrorSettings {
    fn default() -> Self {
        Self { enabled: true, leg: "+X".into(), bindings: BTreeMap::new() }
    }
}

/// One motor's display binding (`{joint, polarity, align}`). Read through
/// [`StoredMirrorBinding`]: a member it lacks takes its default (joint
/// "Hip servo output", sign +1, the joint's `default_align`), so one
/// incomplete binding never resets the whole file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(from = "StoredMirrorBinding")]
pub struct MirrorBinding {
    /// "Hip servo output", "Worm servo output" or "Foot servo output".
    pub joint: String,
    /// +1 or −1.
    pub polarity: i8,
    pub align: Align,
}

/// A [`MirrorBinding`] as stored, every member optional.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct StoredMirrorBinding {
    joint: Option<String>,
    polarity: Option<i8>,
    align: Option<Align>,
}
impl From<StoredMirrorBinding> for MirrorBinding {
    fn from(s: StoredMirrorBinding) -> Self {
        let joint = s.joint.unwrap_or_else(|| super::mirror::default_joint("").to_string());
        let align = s.align.unwrap_or_else(|| super::mirror::default_align(&joint));
        Self { polarity: sign(s.polarity.unwrap_or(1)), align, joint }
    }
}

/// A stored sign as ±1 (negative: −1; anything else: +1).
pub fn sign(polarity: i8) -> i8 {
    if polarity < 0 { -1 } else { 1 }
}

/// `walking-hardware-map-v1`: `{leg, amplitude, bindings}` (hardware-sync.mjs:19).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct SyncSettings {
    pub leg: Option<String>,
    /// Bench motion scale (0.03, 0.05 or 0.09).
    pub amplitude: Option<f64>,
    /// The mapping rows in the leg's coordinate order.
    pub bindings: Vec<SyncBinding>,
}

/// One mapping row. A member the file lacks takes its default (no
/// coordinate, motor 0 — not a bench ID, so `sync::mapping` falls back to the
/// row's default motor — and sign +1).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SyncBinding {
    /// `joint.+X | Hip servo output`, as the bench's `/config` names it.
    #[serde(default)]
    pub coordinate: String,
    #[serde(default)]
    pub motor_id: u8,
    /// +1 or −1.
    #[serde(default = "positive")]
    pub polarity: i8,
}
fn positive() -> i8 {
    1
}

/// `$SIM_SPATIAL_PREFERENCES` if set (and not empty), else
/// `$HOME/.config/sim-spatial/hardware-preferences.json` (relative to the
/// working directory when `HOME` is unset).
pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os(PATH_VARIABLE).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config").join("sim-spatial").join("hardware-preferences.json")
}

/// Reads [`path()`] (see [`load_from`]).
pub fn load() -> Settings {
    load_from(&path())
}

/// Missing or unreadable: the defaults. Not this shape: the defaults, with a
/// warning; the file is not changed.
pub fn load_from(path: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(path) else { return Settings::default() };
    match serde_json::from_str::<Settings>(&text) {
        Ok(s) => s,
        Err(e) => {
            bevy::log::warn!("hardware preferences {}: {e}; using the defaults (the file is left as it is)", path.display());
            Settings::default()
        }
    }
}

/// Save numbers: each save takes the next; the writer skips any older than the last written.
static SAVES: AtomicU64 = AtomicU64::new(0);
static WRITTEN: Mutex<u64> = Mutex::new(0);

impl Settings {
    /// Writes these preferences to [`path()`] off the UI thread (returns at once).
    pub fn save(&self) {
        let mut snapshot = self.clone();
        snapshot.version = VERSION;
        let target = path();
        let number = SAVES.fetch_add(1, Ordering::SeqCst) + 1;
        let job = crate::jobs::Job::spawn(crate::jobs::Pool::Io, number, "hardware-preferences save", move |_| {
            let mut written = WRITTEN.lock().unwrap_or_else(|p| p.into_inner());
            if *written > number {
                return Ok(());
            }
            write_to(&target, &snapshot)?;
            *written = number;
            Ok(())
        })
        .complete_on_drop();
        drop(job);
    }
}

/// Writes `settings` to `path` atomically: parent directories created, a
/// temporary file beside it, renamed over. An existing file that does not
/// parse as preferences is copied to `<file>.corrupt.json` first (once).
pub fn write_to(path: &Path, settings: &Settings) -> Result<(), String> {
    let name = |p: &Path| p.display().to_string();
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if let Ok(text) = std::fs::read_to_string(path) {
        if serde_json::from_str::<Settings>(&text).is_err() {
            let aside = PathBuf::from(format!("{}.corrupt.json", name(path)));
            if !aside.exists() {
                std::fs::copy(path, &aside).map_err(|e| format!("keeping the unreadable {} as {}: {e}", name(path), aside.display()))?;
            }
        }
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| format!("hardware preferences: {e}"))?;
    let tmp = PathBuf::from(format!("{}.tmp-{}", name(path), std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", name(path))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sim-spatial-prefs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("nested").join("hardware-preferences.json")
    }

    #[test]
    fn defaults_are_the_pages_initial_values() {
        let s = Settings::default();
        assert!(s.mirror.enabled);
        assert_eq!(s.mirror.leg, "+X");
        assert!(s.mirror.bindings.is_empty());
        assert_eq!(s.calibration, CalibrationPrefs::default());
        assert_eq!(s.sync, SyncSettings::default());
        // A partial file keeps the defaults of what it does not name, and ignores unknown fields.
        let s: Settings = serde_json::from_str(r#"{"mirror":{"leg":"-Y"},"unknown":1}"#).unwrap();
        assert!(s.mirror.enabled);
        assert_eq!(s.mirror.leg, "-Y");
    }

    #[test]
    fn an_incomplete_binding_takes_defaults_and_keeps_the_file() {
        let s: Settings = serde_json::from_str(
            r#"{"mirror":{"leg":"-Y","bindings":{"1":{},"3":{"joint":"Foot servo output"},"4":{"polarity":-1,"align":"home"},"5":{"polarity":7}}},
                "sync":{"leg":"-X","amplitude":0.05,"bindings":[{"motor_id":4},{"polarity":-1}]}}"#,
        )
        .expect("incomplete bindings still parse");
        assert_eq!(s.mirror.leg, "-Y", "the rest of the file is kept");
        let b = &s.mirror.bindings;
        assert_eq!(b[&1], MirrorBinding { joint: "Hip servo output".into(), polarity: 1, align: Align::Home });
        assert_eq!((b[&3].joint.as_str(), b[&3].polarity, b[&3].align), ("Foot servo output", 1, Align::Mid), "align follows the joint's default");
        assert_eq!((b[&4].joint.as_str(), b[&4].polarity, b[&4].align), ("Hip servo output", -1, Align::Home));
        assert_eq!(b[&5].polarity, 1, "a sign is clamped to ±1");
        assert_eq!((s.sync.leg.as_deref(), s.sync.amplitude), (Some("-X"), Some(0.05)));
        assert_eq!(s.sync.bindings, vec![SyncBinding { coordinate: String::new(), motor_id: 4, polarity: 1 }, SyncBinding { coordinate: String::new(), motor_id: 0, polarity: -1 }]);
        assert_eq!((sign(-3), sign(0), sign(1)), (-1, 1, 1));
    }

    #[test]
    fn round_trip_through_the_file() {
        let path = scratch("round-trip");
        let mut s = Settings::default();
        s.calibration.drive_mode = Some(DriveMode::ServoSpeed);
        s.calibration.hold_others = Some(false);
        s.mirror.bindings.insert(3, MirrorBinding { joint: "Foot servo output".into(), polarity: -1, align: Align::Mid });
        s.sync = SyncSettings { leg: Some("+X".into()), amplitude: Some(0.05), bindings: vec![SyncBinding { coordinate: "joint.+X | Hip servo output".into(), motor_id: 2, polarity: 1 }] };
        write_to(&path, &s).unwrap();
        assert_eq!(load_from(&path), s);
        // Integer map keys are written as strings, as the page's JSON has them.
        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["mirror"]["bindings"]["3"]["align"], "mid");
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn missing_and_corrupt_files_give_defaults_and_the_bad_file_is_kept() {
        let path = scratch("corrupt");
        assert_eq!(load_from(&path), Settings::default());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load_from(&path), Settings::default());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not json", "loading leaves the file alone");
        write_to(&path, &Settings::default()).unwrap();
        let aside = PathBuf::from(format!("{}.corrupt.json", path.display()));
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "{not json", "the bad bytes are kept aside before the first save");
        assert_eq!(load_from(&path), Settings::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
