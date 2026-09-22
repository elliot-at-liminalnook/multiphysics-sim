//! Shared observation-only live presentation contracts; no solver or renderer.
use crate::*;
#[cfg(unix)]
pub mod native;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Paused,
    Running,
    RecordingFull,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionStatus {
    pub phase: Phase,
    pub run_id: String,
    pub generation: u64,
    pub step: u64,
    /// Last fully completed interval, independently of render time.
    pub time: f64,
    pub sequence: u64,
    pub step_wall_seconds: f64,
    pub events: u64,
    pub message: Option<String>,
}

/// Compilation adds declared states and resolves availability. Everything
/// authored, including quantities, frames, signs and physical definitions, stays fixed.
pub fn validate_runtime_description(
    source: &SystemDescription,
    runtime: &SystemDescription,
) -> Result<(), InspectionError> {
    source.validate()?;
    runtime.validate()?;
    ensure(
        source.source_hash == runtime.source_hash
            && source.model_revision == runtime.model_revision
            && source.components == runtime.components
            && source.ports == runtime.ports
            && source.nets == runtime.nets
            && source.groups == runtime.groups
            && serde_json::to_value(&source.definitions)
                .map_err(|e| InspectionError(e.to_string()))?
                == serde_json::to_value(&runtime.definitions)
                    .map_err(|e| InspectionError(e.to_string()))?,
        "worker returned a different physical system",
    )?;
    for (id, authored) in &source.observables {
        let mut resolved = runtime
            .observables
            .get(id)
            .ok_or_else(|| InspectionError(format!("runtime omitted observable {id}")))?
            .clone();
        resolved.availability = authored.availability.clone();
        ensure(
            &resolved == authored,
            "runtime changed an authored observable",
        )?;
    }
    for (id, o) in &runtime.observables {
        ensure(
            source.observables.contains_key(id)
                || matches!(o.location, ObservationLocation::State { .. }),
            "runtime added a non-state observable",
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveSnapshot {
    pub version: u32,
    pub source_description_id: String,
    pub description: Option<SystemDescription>,
    pub status: Option<SessionStatus>,
    pub frame: Option<SampleFrame>,
    pub error: Option<String>,
}
impl LiveSnapshot {
    pub fn validate(&self, source: &SystemDescription) -> Result<(), InspectionError> {
        ensure(
            self.version == 1 && self.source_description_id == source.id,
            "foreign live snapshot",
        )?;
        if let Some(d) = &self.description {
            validate_runtime_description(source, d)?;
        }
        if let Some(s) = &self.status {
            ensure(
                !s.run_id.is_empty()
                    && s.time.is_finite()
                    && s.time >= 0.
                    && s.step_wall_seconds.is_finite()
                    && s.step_wall_seconds >= 0.,
                "invalid live status",
            )?;
        }
        if let Some(f) = &self.frame {
            let d = self
                .description
                .as_ref()
                .ok_or_else(|| InspectionError("frame without description".into()))?;
            let s = self
                .status
                .as_ref()
                .ok_or_else(|| InspectionError("frame without status".into()))?;
            f.validate(d)?;
            ensure(
                f.run_id == s.run_id
                    && f.generation == s.generation
                    && f.sequence == s.sequence
                    && f.step == s.step
                    && f.time == s.time,
                "frame/status disagree",
            )?;
        }
        Ok(())
    }
}

/// Protect a display against old generations, regressing data and mutated samples.
#[derive(Default)]
pub struct LiveGate {
    previous: Option<LiveSnapshot>,
    retired: BTreeSet<String>,
}
impl LiveGate {
    pub fn accept(
        &mut self,
        source: &SystemDescription,
        next: &LiveSnapshot,
    ) -> Result<(), InspectionError> {
        next.validate(source)?;
        if let Some(s) = &next.status {
            ensure(!self.retired.contains(&s.run_id), "retired live run")?;
            if let Some(old) = self.previous.as_ref().and_then(|p| p.status.as_ref()) {
                if old.run_id == s.run_id {
                    ensure(s.generation >= old.generation, "stale live generation")?;
                    if s.generation == old.generation {
                        ensure(
                            s.sequence >= old.sequence && s.step >= old.step && s.time >= old.time,
                            "regressing live status",
                        )?;
                        if let (Some(a), Some(b)) = (
                            self.previous.as_ref().and_then(|p| p.frame.as_ref()),
                            next.frame.as_ref(),
                        ) {
                            ensure(
                                a.sequence != b.sequence || a == b,
                                "changed immutable live frame",
                            )?;
                        }
                    }
                } else {
                    ensure(
                        self.retired.len() < 128,
                        "live run history full; open a new session",
                    )?;
                    self.retired.insert(old.run_id.clone());
                }
            }
        }
        // A building/restart notification does not discard the gate's prior identity.
        if next.status.is_some() {
            self.previous = Some(next.clone());
        }
        Ok(())
    }
}
