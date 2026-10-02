//! Hardware form preference data and the exact legacy migration path.
//! Disk loading, migration and saving belong exclusively to `app::settings`.
//! These choices contain no measured calibration, travel limits, operator
//! confirmations or active drive intent. `align` names a CAD pose reference;
//! it is not a measured alignment angle.
use super::actions::{Align, DriveMode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

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
    path_from(std::env::var_os(PATH_VARIABLE).filter(|p|!p.is_empty()).map(PathBuf::from), std::env::var_os("HOME").map(PathBuf::from))
}
pub(crate) fn path_from(override_path:Option<PathBuf>,home:Option<PathBuf>)->PathBuf {
    override_path.unwrap_or_else(||home.unwrap_or_default().join(".config").join("sim-spatial").join("hardware-preferences.json"))
}

impl Settings {
    /// Validate choices before the owner publishes them; retain incomplete
    /// bindings and normalize polarity exactly as the existing form adapters do.
    pub fn validate(&mut self) -> Result<(), String> {
        if !super::mirror::LEGS.contains(&self.mirror.leg.as_str()) {
            return Err(format!("hardware.mirror.leg: unsupported leg `{}`", self.mirror.leg));
        }
        for (id, binding) in &mut self.mirror.bindings {
            if !super::mirror::JOINTS.iter().any(|(joint, _)| *joint == binding.joint) {
                return Err(format!("hardware.mirror.bindings.{id}.joint: unsupported joint `{}`", binding.joint));
            }
            binding.polarity = sign(binding.polarity);
        }
        if let Some(amplitude) = self.sync.amplitude {
            if !super::sync::SCALES.iter().any(|(scale, _)| *scale == amplitude) {
                return Err("hardware.sync.amplitude: expected 0.03, 0.05 or 0.09".into());
            }
        }
        for binding in &mut self.sync.bindings {
            binding.polarity = sign(binding.polarity);
        }
        self.version = VERSION;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn round_trip_preserves_form_and_pose_references() {
        let mut s = Settings::default();
        s.calibration.drive_mode = Some(DriveMode::ServoSpeed);
        s.calibration.hold_others = Some(false);
        s.mirror.bindings.insert(3, MirrorBinding { joint: "Foot servo output".into(), polarity: -1, align: Align::Mid });
        s.sync = SyncSettings { leg: Some("+X".into()), amplitude: Some(0.05), bindings: vec![SyncBinding { coordinate: "joint.+X | Hip servo output".into(), motor_id: 2, polarity: 1 }] };
        let text = serde_json::to_string_pretty(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&text).unwrap(), s);
        let raw: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(raw["mirror"]["bindings"]["3"]["align"], "mid");
    }

    #[test]
    fn validation_rejects_invalid_choices_and_normalizes_signs() {
        let mut s = Settings::default();
        s.sync.bindings.push(SyncBinding { coordinate: String::new(), motor_id: 0, polarity: -7 });
        s.validate().unwrap();
        assert_eq!(s.sync.bindings[0].polarity, -1);
        s.sync.amplitude = Some(0.5);
        assert!(s.validate().unwrap_err().contains("hardware.sync.amplitude"));
        s.sync.amplitude = Some(0.05);
        s.mirror.leg = "wrong".into();
        assert!(s.validate().unwrap_err().contains("hardware.mirror.leg"));
    }
}
