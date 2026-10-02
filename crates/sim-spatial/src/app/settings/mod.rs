//! One preference owner. Disk work and publication live in this feature,
//! never in a picker, hardware form or CAD document.
pub mod actions;
mod jobs;
mod plugin;
#[cfg(test)]
mod tests;
use super::{ViewerMode, recent::Recents, switch::Document};
use crate::{jobs::Job, robot::hardware::settings::Settings};
use bevy::prelude::*;
use bevy_settings::{ReflectSettingsGroup, SettingsGroup};
pub use plugin::{SettingsPlugin, SettingsSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Instant,
};

/// Registered Bevy settings contract. JSON payloads retain unknown fields;
/// validated typed projections below are the only consumer-facing values.
#[derive(Resource, Reflect, SettingsGroup, Default, Clone)]
#[reflect(Resource, Default, SettingsGroup)]
#[settings_group(group = "preferences", file = "viewer-preferences")]
pub struct PreferenceGroup {
    pub recents: String,
    pub hardware: String,
    pub cad: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FastenerDefaults {
    pub size: String,
    pub kind: String,
    pub extra: f64,
    pub depth: f64,
}
impl Default for FastenerDefaults {
    fn default() -> Self {
        Self {
            size: "M3".into(),
            kind: "clearance".into(),
            extra: 0.,
            depth: 0.,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CadDefaults {
    pub wall_threshold: Option<f64>,
    pub fastener: FastenerDefaults,
    pub clearance: f64,
}
impl Default for CadDefaults {
    fn default() -> Self {
        Self {
            wall_threshold: None,
            fastener: FastenerDefaults::default(),
            clearance: 0.2,
        }
    }
}
impl CadDefaults {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .wall_threshold
            .is_some_and(|v| !v.is_finite() || !(0.1..=20.).contains(&v))
        {
            return Err("cad.wall_threshold: expected 0.1..=20 millimetres".into());
        }
        if !self.clearance.is_finite()
            || !(-5.0..=5.0).contains(&self.clearance)
            || !self.fastener.extra.is_finite()
            || !(0.0..=1.0).contains(&self.fastener.extra)
            || !self.fastener.depth.is_finite()
            || !(0.0..=500.0).contains(&self.fastener.depth)
        {
            return Err("cad: invalid clearance/fastener dimensions".into());
        }
        if !["M2", "M2.5", "M3", "M4", "M5", "M6", "M8"].contains(&self.fastener.size.as_str())
            || !["clearance", "tap", "counterbore", "countersink", "insert"]
                .contains(&self.fastener.kind.as_str())
        {
            return Err("cad.fastener: unsupported size or kind".into());
        }
        Ok(())
    }
}

#[derive(Resource)]
pub struct SettingsOwner {
    pub recents: Recents,
    pub hardware: Settings,
    pub cad: CadDefaults,
    pub ready: bool,
    pub revision: u64,
    pub saved_revision: u64,
    pub diagnostic: Option<String>,
    touched: [bool; 3],
    hardware_claims: Vec<String>,
    cad_claims: [bool; 3],
    paths: Option<jobs::Paths>,
    raw: Value,
    blocked: bool,
    load: Option<Job<jobs::Loaded>>,
    save: Option<Job<u64>>,
    records: VecDeque<(ViewerMode, Document, u64)>,
    canonical: Option<Job<(ViewerMode, Document, u64)>>,
    gate: Arc<Mutex<jobs::Publication>>,
    retry_at: Instant,
}
impl Default for SettingsOwner {
    fn default() -> Self {
        Self {
            recents: Recents::default(),
            hardware: Settings::default(),
            cad: CadDefaults::default(),
            ready: false,
            revision: 0,
            saved_revision: 0,
            diagnostic: None,
            touched: [false; 3],
            hardware_claims: Vec::new(),
            cad_claims: [false; 3],
            paths: None,
            raw: serde_json::json!({}),
            blocked: false,
            load: None,
            save: None,
            records: VecDeque::new(),
            canonical: None,
            gate: Arc::new(Mutex::new(jobs::Publication::default())),
            retry_at: Instant::now(),
        }
    }
}
impl SettingsOwner {
    pub fn record(&mut self, mode: ViewerMode, document: Document) {
        self.touched[0] = true;
        self.revision += 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |t| t.as_secs());
        self.records.push_back((mode, document, now));
    }
    pub fn set_hardware_claimed(&mut self, mut value: Settings, paths: Vec<String>) {
        if let Err(e) = value.validate() {
            self.diagnostic = Some(e);
            return;
        }
        self.touched[1] = true;
        for path in paths {
            self.claim_hardware(path);
        }
        if self.hardware != value {
            self.hardware = value;
            self.revision += 1;
        }
    }
    pub fn set_cad(&mut self, value: CadDefaults) -> Result<(), String> {
        value.validate()?;
        self.touched[2] = true;
        if !self.ready {
            self.cad_claims[0] |= self.cad.wall_threshold != value.wall_threshold;
            self.cad_claims[1] |= self.cad.fastener != value.fastener;
            self.cad_claims[2] |= self.cad.clearance != value.clearance;
        }
        if self.cad != value {
            self.cad = value;
            self.revision += 1;
        }
        Ok(())
    }
    /// Explicit equal-value choices own only their form field or binding group.
    pub fn claim_hardware(&mut self, path: String) {
        if !self.ready && !self.hardware_claims.contains(&path) {
            self.hardware_claims.push(path);
        }
    }
    pub fn claim_cad(&mut self, field: &str) {
        if !self.ready {
            match field {
                "wall_threshold" => self.cad_claims[0] = true,
                "fastener" => self.cad_claims[1] = true,
                "clearance" => self.cad_claims[2] = true,
                _ => {}
            }
        }
    }
    /// Retry is independent of edits. Blocked loads are retried, never replaced.
    pub fn retry(&mut self) {
        self.retry_at = Instant::now();
        if self.blocked && !self.ready && self.load.is_none() {
            if let Some(paths) = self.paths.clone() {
                self.load = Some(jobs::load_job(paths));
            }
        }
    }
    pub fn dirty(&self) -> bool {
        self.revision > self.saved_revision
    }
    pub fn status(&self) -> Value {
        serde_json::json!({"ready":self.ready,"revision":self.revision,"saved_revision":self.saved_revision,"dirty":self.dirty(),"blocked":self.blocked,"diagnostic":self.diagnostic,"saving":self.save.is_some(),"pending_records":self.records.len(),"normalizing":self.canonical.is_some()})
    }
    fn snapshot(&self) -> Result<Value, String> {
        jobs::snapshot(&self.raw, &self.recents, &self.hardware, &self.cad)
    }
}
impl Drop for SettingsOwner {
    fn drop(&mut self) {
        // A best-effort final snapshot uses the same publication gate. No UI
        // blocking, and no guarantee that the process survives to finish it.
        if self.ready && !self.blocked && (self.dirty() || !self.records.is_empty()) {
            if let Some(paths) = self.paths.clone() {
                let raw = self.raw.clone();
                let recents = self.recents.clone();
                let hardware = self.hardware.clone();
                let cad = self.cad.clone();
                let records = self.records.clone();
                let revision = self.revision + 1;
                let gate = self.gate.clone();
                drop(
                    Job::spawn(
                        crate::jobs::Pool::Io,
                        revision,
                        "viewer preferences shutdown",
                        move |_| {
                            let snapshot =
                                jobs::shutdown_snapshot(&raw, recents, &hardware, &cad, records)?;
                            jobs::publish_ordered(&paths, &snapshot, revision, &gate).inspect_err(
                                |e| bevy::log::warn!("preference shutdown publication failed: {e}"),
                            )
                        },
                    )
                    .complete_on_drop(),
                );
            }
        }
    }
}

fn apply_claim(base: &mut Value, path: &str, value: Value) {
    if path.is_empty() {
        *base = value;
        return;
    }
    let mut cursor = base;
    let keys: Vec<_> = path.trim_start_matches('/').split('/').collect();
    for key in &keys[..keys.len() - 1] {
        if !cursor.is_object() {
            *cursor = serde_json::json!({});
        }
        cursor = &mut cursor[*key];
    }
    if !cursor.is_object() {
        *cursor = serde_json::json!({});
    }
    cursor[keys[keys.len() - 1]] = value;
}
