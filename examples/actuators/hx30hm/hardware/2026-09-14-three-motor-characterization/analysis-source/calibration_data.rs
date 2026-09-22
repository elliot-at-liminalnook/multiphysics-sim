//! Calibration datasets supply captured inputs to one shared physical runner.
//! The optimizer cannot substitute measured feedback for an independent closed loop.
use super::recording::{self, Purpose, Recording};
use crate::{
    experiment_comparison::{Limits, Trace, hx_archive::Archive},
    experiment_study::ModelSettings,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug)]
pub struct Case {
    pub id: String,
    pub device: u8,
    pub split: String,
    pub measured: Trace,
    pub limits: Limits,
    pub resolution: f64,
}
/// Original trial roles and observations are supplied by the immutable dataset.
pub trait CalibrationData {
    fn cases(&self) -> Result<Vec<Case>, String>;
    fn predict(
        &self,
        id: &str,
        model: &ModelSettings,
        cancel: &AtomicBool,
    ) -> Result<Trace, String>;
    fn fingerprint(&self) -> String;
}
impl CalibrationData for Archive {
    fn cases(&self) -> Result<Vec<Case>, String> {
        Ok(self
            .trials
            .iter()
            .map(|t| Case {
                id: t.id.clone(),
                device: t.device,
                split: t.split.clone(),
                measured: t.measured.clone(),
                limits: t.limits.clone(),
                resolution: crate::experiment_comparison::hx_archive::ENCODER_QUANTUM_RAD,
            })
            .collect())
    }
    fn predict(
        &self,
        id: &str,
        model: &ModelSettings,
        cancel: &AtomicBool,
    ) -> Result<Trace, String> {
        crate::experiment_study::simulate(
            self.trials
                .iter()
                .find(|t| t.id == id)
                .ok_or("Missing pulse trial")?,
            model,
            cancel,
        )
    }
    fn fingerprint(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).expect("serializable archive"))
            .to_hex()
            .to_string()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Train,
    HeldOut,
}
impl Role {
    pub fn split(&self) -> &str {
        match self {
            Self::Train => "train",
            Self::HeldOut => "held_out_recording",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub recording_hash: String,
    pub role: Role,
    pub limits: Limits,
    pub rationale: String,
}
/// Snapshot retained with each attempt; future imports cannot change its split.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordingDataset {
    pub assignments: Vec<Assignment>,
    pub recordings: Vec<Recording>,
}
impl RecordingDataset {
    pub fn capture(recordings: &[Recording], assignments: &[Assignment]) -> Result<Self, String> {
        let mut selected = vec![];
        for a in assignments {
            selected.push(
                recordings
                    .iter()
                    .find(|r| r.fingerprint() == a.recording_hash)
                    .ok_or("Assigned recording is missing")?
                    .clone(),
            );
        }
        let dataset = Self {
            assignments: assignments.to_vec(),
            recordings: selected,
        };
        dataset.cases()?;
        Ok(dataset)
    }
}
impl CalibrationData for RecordingDataset {
    fn cases(&self) -> Result<Vec<Case>, String> {
        if self.assignments.is_empty() || self.assignments.len() != self.recordings.len() {
            return Err("Recording dataset needs explicit whole-run assignments".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut cases = vec![];
        for (a, r) in self.assignments.iter().zip(&self.recordings) {
            r.validate()?;
            if !r.completed
                || !seen.insert(&a.recording_hash)
                || a.recording_hash != r.fingerprint()
                || a.rationale.trim().is_empty()
            {
                return Err("Dataset requires distinct complete recordings, source identities and split rationale".into());
            }
            let measured = r.measured_trace();
            crate::experiment_comparison::compare(&measured, &measured, &a.limits)?;
            cases.push(Case {
                id: a.recording_hash.clone(),
                device: r.experiment.device,
                split: a.role.split().into(),
                measured,
                limits: a.limits.clone(),
                resolution: r.experiment.timing.encoder_quantum_rad,
            });
        }
        Ok(cases)
    }
    fn predict(
        &self,
        id: &str,
        model: &ModelSettings,
        cancel: &AtomicBool,
    ) -> Result<Trace, String> {
        let i = self
            .assignments
            .iter()
            .position(|a| a.recording_hash == id)
            .ok_or("Missing captured-command trial")?;
        let r = self.recordings.get(i).ok_or("Missing recording snapshot")?;
        if r.fingerprint() != id {
            return Err("Recording identity changed".into());
        }
        // Freeze written duty counts and actual host application/observation windows.
        recording::predict(
            r,
            model,
            Purpose::RecordedCommandReplay,
            &self.assignments[i].limits,
            cancel,
            |_, _| {},
        )
        .map(|p| p.predicted)
    }
    fn fingerprint(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).expect("serializable recording dataset"))
            .to_hex()
            .to_string()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordingFitAttempt {
    pub dataset: RecordingDataset,
    pub attempt: super::calibration::FitAttempt,
}

/// Join independently implemented datasets without changing their original roles,
/// limits or per-trial residual weighting. IDs must remain globally unambiguous.
pub struct JoinedData<'a> {
    sources: Vec<&'a dyn CalibrationData>,
    cases: Vec<Case>,
    routes: std::collections::BTreeMap<String, usize>,
}
impl<'a> JoinedData<'a> {
    pub fn new(sources: Vec<&'a dyn CalibrationData>) -> Result<Self, String> {
        if sources.is_empty() {
            return Err("Combined fitting needs at least one dataset".into());
        }
        let mut cases = vec![];
        let mut routes = std::collections::BTreeMap::new();
        for (i, source) in sources.iter().enumerate() {
            for case in source.cases()? {
                if routes.insert(case.id.clone(), i).is_some() {
                    return Err(format!("Duplicate trial across datasets: {}", case.id));
                }
                cases.push(case);
            }
        }
        Ok(Self {
            sources,
            cases,
            routes,
        })
    }
}
impl CalibrationData for JoinedData<'_> {
    fn cases(&self) -> Result<Vec<Case>, String> {
        Ok(self.cases.clone())
    }
    fn predict(
        &self,
        id: &str,
        model: &ModelSettings,
        cancel: &AtomicBool,
    ) -> Result<Trace, String> {
        self.sources[*self.routes.get(id).ok_or("Missing combined trial")?]
            .predict(id, model, cancel)
    }
    fn fingerprint(&self) -> String {
        blake3::hash(
            &serde_json::to_vec(
                &self
                    .sources
                    .iter()
                    .map(|s| s.fingerprint())
                    .collect::<Vec<_>>(),
            )
            .expect("source hashes"),
        )
        .to_hex()
        .to_string()
    }
}
/// Portable source snapshots for mixed pulse/release/controller-command fitting.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CombinedDataset {
    pub archives: Vec<Archive>,
    pub recordings: Option<RecordingDataset>,
}
impl CombinedDataset {
    pub fn joined(&self) -> Result<JoinedData<'_>, String> {
        let mut sources = self
            .archives
            .iter()
            .map(|a| a as &dyn CalibrationData)
            .collect::<Vec<_>>();
        if let Some(r) = &self.recordings {
            sources.push(r);
        }
        JoinedData::new(sources)
    }
    pub fn validate(&self) -> Result<(), String> {
        for a in &self.archives {
            for t in &a.trials {
                if let Some(release) = &t.release {
                    release.validate()?;
                }
                if crate::experiment_comparison::compare(&t.measured, &t.predicted, &t.limits)?
                    != t.comparison
                {
                    return Err(
                        "Combined archive reference differs from its captured observations".into(),
                    );
                }
            }
        }
        self.joined()?;
        Ok(())
    }
}
impl CalibrationData for CombinedDataset {
    fn cases(&self) -> Result<Vec<Case>, String> {
        self.validate()?;
        self.joined()?.cases()
    }
    fn predict(
        &self,
        id: &str,
        model: &ModelSettings,
        cancel: &AtomicBool,
    ) -> Result<Trace, String> {
        self.joined()?.predict(id, model, cancel)
    }
    fn fingerprint(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).expect("serializable combined dataset"))
            .to_hex()
            .to_string()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CombinedFitAttempt {
    pub dataset: CombinedDataset,
    pub attempt: super::calibration::FitAttempt,
}
