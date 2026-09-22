//! Portable refinement evidence, shared by the native viewer and batch clients.
use super::{
    calibration::{Coordinate, Fit, Robustness, Sensitivity},
    control::{Experiment, Run},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub experiment: Experiment,
    pub controller_runs: Vec<Run>,
    pub sensitivities: Vec<Sensitivity>,
    pub fits: Vec<Fit>,
    pub robustness: Vec<Robustness>,
    pub coordinates: Vec<Coordinate>,
    pub failures: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fpga_design_drafts: Vec<super::fpga_design::Experiment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fpga_design_runs: Vec<super::fpga_design::Run>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fpga_recordings: Vec<super::fpga::Recording>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fpga_reviews: Vec<super::fpga_review::Review>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fpga_fits: Vec<super::fpga_review::FitAttempt>,
    pub recordings: Vec<super::recording::Recording>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub electrical_comparisons: Vec<super::electrical_measurements::Evaluation>,
    pub predictions: Vec<super::recording::Prediction>,
    pub cad_proposals: Vec<super::cad::Proposal>,
    pub cad_acceptances: Vec<super::cad::Acceptance>,
    pub scenarios: Vec<super::calibration::Variant>,
    pub fit_attempts: Vec<super::calibration::FitAttempt>,
    pub capture_contexts: Vec<super::context::CaptureContext>,
    pub recording_assignments: Vec<super::calibration_data::Assignment>,
    pub recording_fits: Vec<super::calibration_data::RecordingFitAttempt>,
    pub combined_fits: Vec<super::calibration_data::CombinedFitAttempt>,
}
impl Workspace {
    pub fn validate_archive(
        &self,
        archive: &crate::experiment_comparison::hx_archive::Archive,
    ) -> Result<(), String> {
        for fit in &self.fits {
            fit.validate(archive)?;
        }
        for attempt in &self.fit_attempts {
            attempt.validate(archive)?;
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        self.experiment.validate()?;
        // Drafts may be incomplete; full plan validation is required before execution/export.
        for draft in &self.fpga_design_drafts {
            let r=self.fpga_recordings.iter().find(|r|r.fingerprint()==draft.timing_recording_hash).ok_or("Controller draft refers to a missing timing source")?;
            if draft.plan.ids!=r.plan.ids || draft.plan.targets.len()!=r.frames.len() || draft.plan.period_s!=r.plan.period_s {
                return Err("Controller draft changed the source motor set or schedule".into());
            }
        }
        for run in &self.fpga_design_runs {
            let r=self.fpga_recordings.iter().find(|r|r.fingerprint()==run.experiment.timing_recording_hash).ok_or("Design refers to missing timing recording")?;
            run.validate(r)?;
        }
        for r in &self.fpga_recordings { r.validate_capture()?; }
        for review in &self.fpga_reviews {
            let r = self.fpga_recordings.iter().find(|r|r.fingerprint()==review.recording_hash).ok_or("FPGA review references missing recording")?;
            review.validate(r)?;
        }
        for fit in &self.fpga_fits {
            fit.attempt.validate(&fit.dataset)?;
            for r in &fit.dataset.recordings {
                if !self.fpga_recordings.iter().any(|source|source.fingerprint()==r.fingerprint()) {
                    return Err("FPGA fit changed a source recording or its frozen role".into());
                }
            }
        }
        for fit in &self.combined_fits {
            fit.dataset.validate()?;
            fit.attempt.validate(&fit.dataset)?;
            if let Some(data) = &fit.dataset.recordings {
                for a in &data.assignments {
                    if !self.recording_assignments.contains(a) {
                        return Err("Combined fit changed a frozen recording assignment".into());
                    }
                }
            }
        }
        if !self.recording_assignments.is_empty() {
            super::calibration_data::RecordingDataset::capture(
                &self.recordings,
                &self.recording_assignments,
            )?;
        }
        for fit in &self.recording_fits {
            fit.attempt.validate(&fit.dataset)?;
            for (a, r) in fit.dataset.assignments.iter().zip(&fit.dataset.recordings) {
                if !self.recording_assignments.contains(a)
                    || !self
                        .recordings
                        .iter()
                        .any(|source| source.fingerprint() == r.fingerprint())
                {
                    return Err(
                        "Recording fit changed an original role, limit or source recording".into(),
                    );
                }
            }
        }

        for context in &self.capture_contexts {
            context.validate()?;
            let r = self
                .recordings
                .iter()
                .find(|r| r.fingerprint() == context.recording_hash)
                .ok_or("Setup snapshot refers to missing recording")?;
            if let Some(id) = &r.experiment.component_id {
                if context
                    .bindings
                    .iter()
                    .any(|b| b.hardware_id == r.experiment.device && &b.cad_component_id != id)
                {
                    return Err("Setup binding conflicts with captured CAD identity".into());
                }
            }
        }
        for run in &self.controller_runs {
            run.validate()?;
        }
        for fit in &self.fits {
            fit.baseline.shared.validate()?;
            fit.candidate.shared.validate()?;
            fit.runtime.validate()?;
            if fit
                .training_ids
                .iter()
                .any(|id| fit.validation_ids.contains(id))
            {
                return Err("Saved fit leaks validation into training".into());
            }
        }
        for r in &self.robustness {
            r.validate()?;
        }
        for r in &self.recordings {
            r.validate()?;
        }
        for e in &self.electrical_comparisons {
            let p = self
                .predictions
                .iter()
                .find(|p| super::electrical_measurements::prediction_hash(p) == e.prediction_hash)
                .ok_or("Electrical comparison references missing prediction")?;
            let r = self
                .recordings
                .iter()
                .find(|r| r.fingerprint() == e.measurements.recording_hash)
                .ok_or("Electrical measurement references missing recording")?;
            e.measurements.validate_recording(r)?;
            e.validate(p)?;
        }
        for p in &self.predictions {
            let recording = self
                .recordings
                .iter()
                .find(|r| r.fingerprint() == p.recording_hash)
                .ok_or("Prediction references a missing recording")?;
            p.validate(recording)?;
        }
        Ok(())
    }
}
