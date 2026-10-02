use eframe::egui::{self, Color32};
use sim_runtime::{
    controller_refinement::{
        cad, calibration as cal, calibration_data as data, control, evidence, recording,
    },
    experiment_study::Study,
};
use std::{collections::BTreeMap, sync::atomic::AtomicBool};

pub enum Outcome {
    Shared(sim_runtime::experiment_study::refinement::Outcome),
    SharedInputs(sim_runtime::experiment_study::refinement::Outcome, serde_json::Value),
    Recording(recording::Recording),
    FpgaRecording(sim_runtime::controller_refinement::fpga::Recording),
    FpgaReview(sim_runtime::controller_refinement::fpga_review::Review),
    FpgaDesign(sim_runtime::controller_refinement::fpga_design::Run),
    FpgaPlanSaved(String),
    FpgaFit(sim_runtime::controller_refinement::fpga_review::FitAttempt),
    Proposal(cad::Proposal),
    Accepted(cad::Acceptance),
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Simulate,
    Sensitivity(Vec<String>),
    Fit(Vec<String>, Vec<String>),
    FitRecordings,
    FitCombined {
        selected: Vec<String>,
        additional_study: String,
    },
    Robustness,
    Import(String),
    ReviewFpga(usize, sim_runtime::controller_refinement::fpga_review::Mode),
    FitFpga,
    DesignFpga(sim_runtime::controller_refinement::fpga_design::Experiment),
    ExportFpgaPlan(
        sim_runtime::controller_refinement::fpga_design::Experiment,
        String,
    ),
    ReviewFpgaFit(
        usize,
        usize,
        sim_runtime::controller_refinement::fpga_review::Mode,
    ),
    Predict(usize, recording::Purpose),
    CompareElectrical(usize, Option<String>),
    Propose(String),
    Accept(usize, String, String, String),
}
impl Action {
    pub fn operation(&self) -> Option<sim_runtime::experiment_study::refinement::Operation> {
        use sim_runtime::experiment_study::refinement::Operation;
        match self {
            Self::Simulate => Some(Operation::Simulate),
            Self::Sensitivity(selected) => Some(Operation::Sensitivity { selected:selected.clone() }),
            Self::Fit(train,validation) => Some(Operation::Fit {train:train.clone(),validation:validation.clone()}),
            Self::Robustness => Some(Operation::Robustness),
            _ => None,
        }
    }
    pub fn label(&self) -> &str {
        if let Some(operation)=self.operation() {return operation.label();}
        match self {
            Self::ReviewFpga(_, _) | Self::ReviewFpgaFit(_, _, _) => {
                "Comparing captured FPGA controller"
            }
            Self::FitFpga => "Fitting FPGA motor response",
            Self::DesignFpga(_) => "Simulating a new FPGA controller design",
            Self::ExportFpgaPlan(_, _) => "Exporting FPGA experiment plan",
            Self::Simulate | Self::Sensitivity(_) | Self::Fit(_,_) | Self::Robustness => unreachable!("shared label dispatched above"),
            Self::CompareElectrical(_, _) => "Comparing electrical measurement channels",
            Self::FitRecordings => "Fitting captured PWM command histories",
            Self::FitCombined { .. } => {
                "Fitting pulse, release and captured PWM histories together"
            }
            Self::Import(_) => "Importing captured hardware controller run",
            Self::Propose(_) => "Preparing CAD property proposal",
            Self::Accept(_, _, _, _) => "Saving accepted CAD artifact",
            Self::Predict(_, recording::Purpose::RecordedCommandReplay) => {
                "Replaying captured PWM commands"
            }
            Self::Predict(_, recording::Purpose::ClosedLoopPrediction) => {
                "Predicting physical closed-loop behavior"
            }
        }
    }
    pub fn total(&self, s: &Study) -> usize {
        if let Some(operation)=self.operation() {return operation.total(s);}
        match self {
            Self::ReviewFpga(i, _) | Self::ReviewFpgaFit(i, _, _) => {
                s.refinement.fpga_recordings[*i].plan.ids.len() * 1000
            }
            Self::FitFpga => 40,
            Self::DesignFpga(e) => e.plan.ids.len() * 1000,
            Self::ExportFpgaPlan(_, _) => 0,
            Self::Simulate | Self::Sensitivity(_) | Self::Fit(_,_) | Self::Robustness => unreachable!("shared total dispatched above"),
            Self::FitRecordings | Self::FitCombined { .. } => 40,
            Self::Import(_)
            | Self::CompareElectrical(_, _)
            | Self::Propose(_)
            | Self::Accept(_, _, _, _) => 0,
            Self::Predict(i, _) => s.refinement.recordings[*i].frames.len() * 3,
        }
    }
    pub fn run(
        self,
        s: Study,
        cancel: &AtomicBool,
        progress: impl FnMut(usize, usize),
    ) -> Result<Outcome, String> {
        if let Some(operation) = self.operation() {
            let mut captured = s;
            let capture = sim_runtime::experiment_study::refinement::prepare(&mut captured, operation)?;
            let outcome = sim_runtime::experiment_study::refinement::execute(capture, cancel, progress)?;
            let inputs = outcome.capture.inputs();
            return Ok(Outcome::SharedInputs(outcome, inputs));
        }
        let family = cal::Family {
            shared: s.draft.clone(),
            device_deltas: BTreeMap::new(),
        };
        let w = &s.refinement;
        match self {
            Self::DesignFpga(e) => {
                let r = w
                    .fpga_recordings
                    .iter()
                    .find(|r| r.fingerprint() == e.timing_recording_hash)
                    .ok_or("Missing design timing source")?;
                sim_runtime::controller_refinement::fpga_design::simulate(
                    &e, r, &family, cancel, progress,
                )
                .map(Outcome::FpgaDesign)
            }
            Self::ExportFpgaPlan(e, path) => {
                let r = w
                    .fpga_recordings
                    .iter()
                    .find(|r| r.fingerprint() == e.timing_recording_hash)
                    .ok_or("Missing design timing source")?;
                e.export_new(r, std::path::Path::new(&path))?;
                Ok(Outcome::FpgaPlanSaved(path))
            }
            Self::ReviewFpgaFit(i, fit, mode) => {
                sim_runtime::controller_refinement::fpga_review::review_family(
                    w.fpga_recordings.get(i).ok_or("Select an FPGA recording")?,
                    &w.fpga_fits
                        .get(fit)
                        .and_then(|f| f.attempt.outcome.as_ref())
                        .ok_or("Missing fitted family")?
                        .candidate,
                    mode,
                    cancel,
                    progress,
                )
                .map(Outcome::FpgaReview)
            }
            Self::ReviewFpga(i, mode) => sim_runtime::controller_refinement::fpga_review::review(
                w.fpga_recordings.get(i).ok_or("Select an FPGA recording")?,
                &s.draft,
                mode,
                cancel,
                progress,
            )
            .map(Outcome::FpgaReview),
            Self::FitFpga => sim_runtime::controller_refinement::fpga_review::fit_measured_voltage(
                w.fpga_recordings
                    .iter()
                    .filter(|r| r.completed && r.plan.role != "timing")
                    .cloned()
                    .collect(),
                family,
                w.coordinates.clone(),
                cancel,
                progress,
            )
            .map(Outcome::FpgaFit),
            Self::CompareElectrical(i, path) => {
                use sim_runtime::experiment_study::refinement::{self as shared, Operation};
                use shared::electrical::Operation as Electrical;
                let prediction = w.predictions.get(i).ok_or("refinement.electrical.prediction: missing prediction")?;
                let hash = prediction.recording_hash.clone();
                let mut snapshot = s;
                let mut input = serde_json::json!({"legacy_action":"compare_electrical","prediction":i,"recording_hash":hash,"path":path});
                let parsed = if let Some(path) = path {
                    std::fs::read(&path).map_err(|e|format!("refinement.electrical.measurement_path: {e}"))
                        .and_then(|bytes| {
                            input["content_ref"] = serde_json::json!(snapshot.input_contents.capture(bytes.clone()));
                            serde_json::from_slice(&bytes).map_err(|e|format!("refinement.electrical.measurements: {e}"))
                        }).map(|measurements| Electrical::CompareMeasurements{measurements,prediction:i})
                } else {
                    Ok(Electrical::CompareServoVoltage{recording_hash:hash.clone(),prediction:i})
                };
                let operation = parsed.as_ref().map(|op|Operation::Electrical(op.clone()))
                    .unwrap_or_else(|_|Operation::Electrical(Electrical::RejectedInput{prediction:i,source:input.to_string()}));
                let capture = parsed.and_then(|_|shared::prepare(&mut snapshot,operation.clone()));
                match capture {
                    Ok(capture)=>{
                        let outcome=shared::execute(capture,cancel,progress)?;
                        let mut inputs=outcome.capture.inputs();
                        inputs["additional_input"]=input;
                        Ok(Outcome::SharedInputs(outcome,inputs))
                    },
                    Err(error)=>{let outcome=shared::Outcome {
                        capture:shared::Capture{study:snapshot,operation,runtime:sim_runtime::experiment_study::execution_identity(),captured_unix_ns:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos().to_string()},
                        result:Err(format!("Rejected electrical comparison input {input}: {error}")),cancelled:false,
                    };
                        let mut inputs=outcome.capture.inputs();
                        inputs["additional_input"]=input;
                        Ok(Outcome::SharedInputs(outcome,inputs))
                    },
                }
            }
            Self::FitCombined { selected, additional_study } => {
                use sim_runtime::experiment_study::refinement::{self as shared, Command, Operation};
                let additional = if additional_study.trim().is_empty() { None } else {
                    Some(Box::new(Study::load(std::path::Path::new(additional_study.trim()))?))
                };
                let mut snapshot=s;
                shared::validate_archive_selection(&snapshot,&selected,"refinement.fit_combined.selected")?;
                let train=selected.iter().filter(|id|snapshot.archive.trials.iter().any(|t|&t.id==*id&&t.split=="train")).cloned().collect();
                let validation=selected.iter().filter(|id|snapshot.archive.trials.iter().any(|t|&t.id==*id&&t.split!="train")).cloned().collect();
                shared::apply(&mut snapshot,Command::SetSelection{kind:"train".into(),ids:train})?;
                shared::apply(&mut snapshot,Command::SetSelection{kind:"validation".into(),ids:validation})?;
                if let Some(extra)=additional.as_deref(){shared::recordings::reserve_additional(&mut snapshot,extra)?;}
                let operation=Operation::FitCombined{additional};
                let rejected=operation.clone();
                let capture=match shared::prepare(&mut snapshot,operation) {
                    Ok(capture)=>capture,
                    Err(error)=>return Ok(Outcome::Shared(shared::Outcome{capture:shared::Capture{study:snapshot,operation:rejected,runtime:sim_runtime::experiment_study::execution_identity(),captured_unix_ns:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos().to_string()},result:Err(error),cancelled:false})),
                };
                shared::execute(capture,cancel,progress).map(|outcome| {
                    let inputs=outcome.capture.inputs();
                    Outcome::SharedInputs(outcome,inputs)
                })
            }
            Self::FitRecordings => {
                use sim_runtime::experiment_study::refinement::{self as shared, Operation};
                let mut snapshot=s;
                let capture=shared::prepare(&mut snapshot,Operation::FitRecordings)?;
                shared::execute(capture,cancel,progress).map(|outcome| {
                    let inputs=outcome.capture.inputs();
                    Outcome::SharedInputs(outcome,inputs)
                })
            }
            Self::Propose(path) => {
                let source: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                cad::propose(
                    &source,
                    w.experiment
                        .component_id
                        .as_deref()
                        .ok_or("Enter an explicit CAD motor component ID")?,
                    w.experiment.device,
                    &s.baseline,
                    &s.draft,
                    vec![
                        format!("measurements blake3 {}", s.archive.observation_blake3),
                        format!("candidate blake3 {}", s.draft.fingerprint()),
                    ],
                    &format!("Unloaded bench only; {}. {}", w.experiment.fixture, s.notes),
                )
                .map(Outcome::Proposal)
            }
            Self::Accept(i, source, destination, decision) => cad::save_accepted_new(
                &w.cad_proposals[i],
                std::path::Path::new(&source),
                std::path::Path::new(&destination),
                &decision,
            )
            .map(Outcome::Accepted),
            Self::Import(path) => {
                use sim_runtime::experiment_study::refinement::recordings::{classify,ImportClassification};
                let bytes=std::fs::read(&path).map_err(|e|format!("refinement.import.path: {e}"))?;
                match classify(&bytes)? {
                    ImportClassification::Controller(r) => {
                        // Application uses the shared transactional owner in the result consumer.
                        let mut validated=s.clone();
                        sim_runtime::experiment_study::refinement::apply(&mut validated,
                            sim_runtime::experiment_study::refinement::Command::ImportRecording{recording:r.clone()})?;
                        Ok(Outcome::Recording(r))
                    }
                    ImportClassification::FpgaDeferred => {
                        let r:sim_runtime::controller_refinement::fpga::Recording=serde_json::from_slice(&bytes).map_err(|e|format!("refinement.import.fpga: {e}"))?;
                        r.validate_capture()?;
                        if w.fpga_recordings.iter().any(|source|source.fingerprint()==r.fingerprint()) {
                            return Err("This FPGA recording is already imported".into());
                        }
                        Ok(Outcome::FpgaRecording(r))
                    }
                }
            }
            Self::Predict(i, purpose) => {
                use sim_runtime::experiment_study::refinement::{self as shared,Operation};
                let hash=w.recordings.get(i).ok_or("refinement.predict.recording: missing recording")?.fingerprint();
                let mut snapshot=s;
                let capture=shared::prepare(&mut snapshot,Operation::PredictRecording{recording_hash:hash,purpose})?;
                shared::execute(capture,cancel,progress).map(|outcome| {
                    let inputs=outcome.capture.inputs();
                    Outcome::SharedInputs(outcome,inputs)
                })
            }
            Self::Simulate | Self::Sensitivity(_) | Self::Fit(_,_) | Self::Robustness => unreachable!("shared operation dispatched above"),
        }
    }
}
#[derive(Default)]
pub struct State {
    section: usize,
    selected_run: usize,
    json_editor: String,
    editor_open: bool,
    recording_path: String,
    selected_recording: usize,
    cad_path: String,
    cad_output: String,
    cad_decision: String,
    scenario_editor: String,
    scenario_error: Option<String>,
    context_editor: String,
    context_error: Option<String>,
    assignment_rationale: String,
    combined_study_path: String,
    power_ui: super::power_ui::State,
    fpga_ui: super::fpga_ui::State,
}
fn number(ui: &mut egui::Ui, label: &str, value: &mut f64, speed: f64) {
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(value).speed(speed).max_decimals(6));
        ui.label(label);
    });
}
impl State {
    pub fn select_fpga(&mut self) {
        self.section = 7;
    }
    pub fn select_power(&mut self) {
        self.section = 6;
    }
    pub fn select_hardware(&mut self) {
        self.section = 4;
        self.selected_recording = 0;
    }
    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Study, busy: bool, study_id:usize) -> (Option<Action>, bool) {
        if let Some(hash)=&s.refinement_evidence.selected_recording {
            if let Some(index)=s.refinement.recordings.iter().position(|r|r.fingerprint()==*hash) {self.selected_recording=index;}
        }
        let exposure_before=sim_runtime::experiment_study::refinement::ReviewExposure::capture(s);
        sim_runtime::experiment_study::refinement::expose_review(s);
        let original = s.clone();
        let before = serde_json::to_vec(&(
            &s.refinement.experiment,
            &s.refinement.coordinates,
            &s.refinement.scenarios,
            &s.refinement.capture_contexts,
            &s.refinement.recording_assignments,
            &s.refinement.fpga_design_drafts,
            &s.refinement_evidence.selected_recording,
        ))
        .unwrap();
        let mut changed = exposure_before!=sim_runtime::experiment_study::refinement::ReviewExposure::capture(s);
        let mut action = None;
        ui.horizontal(|ui| {
            for (i, label) in [
                "Controller & trajectory",
                "Accuracy & coverage",
                "Sensitivity & fitting",
                "Robustness",
                "Hardware validation",
                "CAD proposal",
                "Electrical & battery",
                "FPGA experiments",
            ]
            .iter()
            .enumerate()
            {
                ui.selectable_value(&mut self.section, i, *label);
            }
        });
        egui::ScrollArea::vertical().id_salt("refinement-body").max_height(590.).show(ui,|ui|{
            match self.section {
                0=>{
                    ui.strong("Design a signed-PWM feedback controller");
                    ui.small("Simulation only until matched physical controller recordings are imported. Controller feedback is quantized encoder position with shared velocity estimation.");
                    ui.horizontal_top(|ui|{
                        ui.vertical(|ui|{ui.set_width(360.);
                            let e=&mut s.refinement.experiment;
                            ui.text_edit_singleline(&mut e.name);
                            ui.horizontal(|ui|{ui.label("Motor ID");ui.add(egui::DragValue::new(&mut e.device).range(1..=253));});
                            ui.label("Fixture / attached output hardware");ui.text_edit_multiline(&mut e.fixture);
                            let mut link=e.component_id.clone().unwrap_or_default();ui.horizontal(|ui|{ui.label("CAD component");if ui.text_edit_singleline(&mut link).changed(){e.component_id=(!link.is_empty()).then_some(link);}});
                            match &mut e.controller {
                                control::Policy::RustPid{parameters:p}=>{ui.strong("Rust position PID");number(ui,"Kp (duty/rad)",&mut p.kp,0.01);number(ui,"Ki (duty/rad/s)",&mut p.ki,0.01);number(ui,"Kd (duty·s/rad)",&mut p.kd,0.001);number(ui,"Integral bound (rad·s)",&mut p.integral_limit,0.01);number(ui,"Maximum |PWM duty|",&mut p.duty_limit,0.01);},
                                control::Policy::Rhai{source,..}=>{ui.label("Rhai controller source");ui.add(egui::TextEdit::multiline(source).code_editor().desired_rows(10));}
                            }
                            ui.collapsing("Trajectory: time (s), relative angle (rad)",|ui|{
                                for (i,k) in e.trajectory.iter_mut().enumerate(){ui.push_id(i,|ui|{ui.horizontal(|ui|{ui.add(egui::DragValue::new(&mut k.time_s).speed(0.01));ui.add(egui::DragValue::new(&mut k.position_rad).speed(0.01));});});}
                                if ui.button("Add waypoint").clicked(){let last=e.trajectory.last().cloned().unwrap_or(control::Knot{time_s:0.,position_rad:0.});e.duration_s=e.duration_s.max(last.time_s+0.2);e.trajectory.push(control::Knot{time_s:last.time_s+0.2,position_rad:last.position_rad});}
                                if e.trajectory.len()>2&&ui.button("Remove last waypoint").clicked(){e.trajectory.pop();}
                            });
                            number(ui,"Duration (s)",&mut e.duration_s,0.1);
                            ui.collapsing("Feedback, timing & initial state",|ui|{
                                number(ui,"Controller period (s)",&mut e.timing.period_s,0.001);
                                number(ui,"Encoder quantum (rad)",&mut e.timing.encoder_quantum_rad,0.00001);
                                number(ui,"Velocity filter (s)",&mut e.timing.velocity_filter_s,0.001);
                                number(ui,"Maximum observation age (s)",&mut e.timing.maximum_observation_age_s,0.001);
                                ui.horizontal(|ui|{ui.add(egui::DragValue::new(&mut e.timing.observation_delay_ticks));ui.label("Observation delay (ticks)");});
                                ui.horizontal(|ui|{ui.add(egui::DragValue::new(&mut e.timing.command_delay_ticks));ui.label("Command delay (ticks)");});
                                number(ui,"Initial encoder angle (rad)",&mut e.initial_encoder_rad,0.01);
                                number(ui,"Supply (V)",&mut e.voltage_v,0.1);number(ui,"Temperature (°C)",&mut e.temperature_c,0.1);
                                ui.label("Timing evidence / assumptions");ui.text_edit_multiline(&mut e.timing.evidence);
                            });
                            ui.collapsing("Predeclared tracking limits",|ui|{number(ui,"RMS (rad)",&mut e.limits.rms_rad,0.001);number(ui,"Peak (rad)",&mut e.limits.peak_rad,0.001);number(ui,"Final tail error (rad)",&mut e.limits.settled_rad,0.001);number(ui,"Maximum saturation fraction",&mut e.limits.maximum_saturation_fraction,0.01);ui.small("Tail error is maximum error over the final 10% of samples; it is not a measured settling time.");ui.text_edit_multiline(&mut e.notes);});
                            if ui.add_enabled(!busy,egui::Button::new("Run controller on candidate model")).clicked(){action=Some(Action::Simulate);}
                            if let Err(error)=e.validate(){ui.colored_label(Color32::DARK_RED,error);}
                            if ui.button("Edit complete experiment / Rhai JSON").clicked(){self.json_editor=serde_json::to_string_pretty(e).unwrap();self.editor_open=true;}
                        });
                        ui.separator();ui.vertical(|ui|{
                            ui.set_min_width(480.);
                            ui.strong("Captured controller runs");
                            if s.refinement.controller_runs.is_empty(){ui.label("Run a controller to inspect target, feedback, physical angle and applied duty.");}
                            for (i,r) in s.refinement.controller_runs.iter().enumerate(){if ui.selectable_label(self.selected_run==i,format!("{} · {} · {}",i+1,r.experiment.name,if r.failure.is_some()||r.cancelled{"UNSCORED"}else if r.score.as_ref().is_some_and(|s|s.passes){"TRACKING PASS"}else{"TRACKING FAIL"})).clicked(){self.selected_run=i;}}
                            if let Some(r)=s.refinement.controller_runs.get(self.selected_run){
                                if r.experiment!=s.refinement.experiment||r.model!=s.draft{ui.colored_label(Color32::DARK_RED,"Captured result differs from current draft.");}
                                if let Some(score)=&r.score{ui.label(format!("RMS {:.5} rad · peak {:.5} rad · tail {:.5} rad · saturated {:.1}%",score.rms_rad,score.peak_rad,score.settled_error_rad,score.saturation_fraction*100.));}
                                if let Some(error)=&r.failure{ui.colored_label(Color32::DARK_RED,error);}
                                chart(ui,"Position (rad)",&[
                                    ("Target",Color32::GRAY,r.frames.iter().map(|f|[f.time_s,f.target_rad]).collect()),
                                    ("Encoder estimate",Color32::from_rgb(30,130,200),r.frames.iter().map(|f|[f.time_s,f.estimated_position_rad]).collect()),
                                    ("Physical angle",Color32::from_rgb(50,160,100),r.truth.clone()),
                                ]);
                                chart(ui,"Applied PWM duty",&[("Duty",Color32::from_rgb(160,100,200),r.frames.iter().map(|f|[f.time_s,f.applied_duty]).collect())]);
                                ui.small("A controller tracking pass evaluates this simulated plant. It does not validate model accuracy against hardware.");
                            }
                        });
                    });
                    if self.editor_open {ui.separator();ui.add(egui::TextEdit::multiline(&mut self.json_editor).code_editor().desired_rows(16).desired_width(f32::INFINITY));if ui.button("Apply validated experiment").clicked(){match serde_json::from_str::<control::Experiment>(&self.json_editor){Ok(e)=>match e.validate(){Ok(())=>{s.refinement.experiment=e;self.editor_open=false;},Err(e)=>{ui.colored_label(Color32::DARK_RED,e);}},Err(e)=>{ui.colored_label(Color32::DARK_RED,e.to_string());}}}}
                }
                1=>{
                    let report=evidence::coverage(&s.archive,s.view.evaluation.and_then(|i|s.evaluations.get(i)));
                    ui.strong("Measured model-prediction error and tested scope");
                    for text in &report.limitations{ui.small(text);}
                    ui.collapsing("Controller recordings and draft applicability",|ui|{
                        for (i,r) in evidence::controller_coverage(&s.refinement,&s.draft).iter().enumerate(){ui.push_id(i,|ui|{ui.collapsing(format!("ID {} · {} · {}",r.device,r.name,if r.completed{"complete"}else{"incomplete"}),|ui|{
                            ui.label(format!("Position {:?} rad · sampled speed {:?} rad/s · duty {:?}",r.displacement_range_rad,r.sampled_velocity_range_rad_s,r.duty_range));
                            ui.label(format!("Supply {:?} V · temperature {:?} °C · largest tick gap {:?} s",r.voltage_range_v,r.temperature_range_c,r.maximum_tick_gap_s));
                            for (purpose,rms,passes) in &r.prediction_results{ui.label(format!("Current model {:?}: RMS {:.5} rad · {}",purpose,rms,if *passes{"passes captured limits"}else{"fails captured limits"}));}
                            if r.draft_extrapolation_reasons.is_empty(){ui.label("Draft matches this recorded configuration; use its prediction errors to assess accuracy.");}else{ui.strong("Draft extrapolations / missing evidence");for reason in &r.draft_extrapolation_reasons{ui.label(reason);}}
                            ui.small(&r.uncertainty);
                        });});}
                    });
                    egui::Grid::new("coverage").striped(true).show(ui,|ui|{
                        for title in ["Motor / split","PWM / duration","Voltage / temperature","RMS / peak (rad)","Onset bracket (s)"]{ui.strong(title);}ui.end_row();
                        for r in &report.rows{ui.label(format!("{} / {}",r.device,r.role));ui.label(format!("{:+.0}% / {:.2}s",r.duty*100.,r.duration_s));ui.label(format!("{:?} V / {:?} °C",r.voltage_range_v,r.temperature_range_c));ui.label(r.model_rmse_rad.zip(r.model_peak_error_rad).map(|(a,b)|format!("{a:.5} / {b:.5}")).unwrap_or("Unscored".into()));ui.label(r.onset_window_s.map(|w|format!("{:.4}–{:.4}",w[0],w[1])).unwrap_or("Unresolved".into()));ui.end_row();}
                    });
                    if report.rows.iter().any(|r|r.release.is_some()){ui.collapsing("Zero-PWM and torque-off release evidence",|ui|{for r in &report.rows{if let Some(release)=&r.release{ui.label(format!("ID {} · {} · {:+.1}% · {}",r.device,r.kind,r.duty*100.,r.role));ui.label(format!("Offset from last pre-release sample: {:?} rad; stationary-tail start: {}",release.terminal_offset_from_reference_rad,release.stationary_tail_start_delay.interval_s.map(|[lo,hi]|format!("{lo:.5}–{hi:.5} s")).unwrap_or_else(||format!("Unresolved: {}",release.stationary_tail_start_delay.unresolved.clone().unwrap_or_default()))));ui.small(&release.interpretation);}}});}
                    ui.collapsing("Observed repeatability",|ui|{if report.repeatability.is_empty(){ui.label("No repeated matching device/duty/duration/mode groups in this archive.");}for r in report.repeatability{ui.label(format!("ID {} · {:+.1}% · {} trials · endpoint {:?} rad",r.device,r.duty*100.,r.trials.len(),r.endpoint_range_rad));ui.small(r.interpretation);}});
                }
                2=>{
                    ui.strong("Bounded physical-model refinement");
                    ui.label("The current measurement filters choose whole trials. Fitting uses only original training trials; original held-out trials are scored separately.");
                    if s.refinement.coordinates.is_empty()&&ui.button("Start with motor resistance").clicked(){if let Some(&r)=s.draft.motor.get("resistance"){s.refinement.coordinates.push(cal::Coordinate{path:"motor.resistance".into(),device:None,lower:r*0.5,upper:r*1.5});}}
                    let mut remove=None;
                    for (i,c) in s.refinement.coordinates.iter_mut().enumerate(){ui.push_id(i,|ui|{ui.horizontal(|ui|{ui.text_edit_singleline(&mut c.path);ui.add(egui::DragValue::new(&mut c.lower).speed(0.0001).max_decimals(8));ui.label("to");ui.add(egui::DragValue::new(&mut c.upper).speed(0.0001).max_decimals(8));let mut device=c.device.unwrap_or(0);if ui.add(egui::DragValue::new(&mut device).range(0..=253)).changed(){c.device=(device>0).then_some(device);}ui.label("ID 0 = shared");if ui.button("Remove").clicked(){remove=Some(i);}});});}
                    if let Some(i)=remove{s.refinement.coordinates.remove(i);}
                    if ui.button("Add parameter / device deviation").clicked(){s.refinement.coordinates.push(cal::Coordinate{path:"condition.delay".into(),device:None,lower:0.,upper:0.05});}
                    ui.small("Paths: motor.resistance, motor.constant (Kt=Ke), condition.load_inertia, condition.delay, or a registry motor/bridge parameter. Device bounds describe additive deviations.");
                    let ids=super::filtered(s);let training=ids.iter().filter(|id|s.archive.trials.iter().any(|t|&t.id==*id&&t.split=="train")).cloned().collect::<Vec<_>>();let held=ids.iter().filter(|id|s.archive.trials.iter().any(|t|&t.id==*id&&t.split!="train")).cloned().collect::<Vec<_>>();
                    ui.label(format!("{} training / {} held-out trials selected",training.len(),held.len()));
                    ui.horizontal(|ui|{if ui.add_enabled(!busy&&!training.is_empty(),egui::Button::new("Analyze training sensitivity")).clicked(){action=Some(Action::Sensitivity(training.clone()));}if ui.add_enabled(!busy&&!training.is_empty()&&!held.is_empty(),egui::Button::new("Fit bounded candidate · 40 evaluations")).clicked(){action=Some(Action::Fit(training,held));s.validation_seen=true;changed=true;}});
                    for (i,r) in s.refinement.sensitivities.iter().enumerate().rev(){ui.collapsing(format!("Sensitivity {} · local rank {}/{}",i+1,r.rank,r.coordinates.len()),|ui|{for (c,n) in r.coordinates.iter().zip(&r.column_norms){ui.label(format!("{} · ID {:?} · scaled sensitivity {:.4}",c.path,c.device,n));}for (a,b,cos) in &r.similar_parameters{ui.label(format!("Confounded: {} / {} (cosine {cos:.4})",r.coordinates[*a].path,r.coordinates[*b].path));}for w in &r.warnings{ui.small(w);}});}
                    for (i,attempt) in s.refinement.fit_attempts.iter().enumerate().rev(){ui.collapsing(format!("Attempt {} · {} evaluations · {}",i+1,attempt.evaluations.len(),if attempt.outcome.is_some(){"result retained"}else{"failed / cancelled"}),|ui|{if let Some(error)=&attempt.failure{ui.colored_label(Color32::DARK_RED,error);}ui.monospace(serde_json::to_string_pretty(attempt).unwrap());});}
                    let mut adopt=None;
                    for (i,f) in s.refinement.fits.iter().enumerate().rev(){ui.collapsing(format!("Fit {} · {} training / {} held",i+1,f.training_ids.len(),f.validation_ids.len()),|ui|{ui.label(&f.status);if !f.has_verified_traces(){ui.colored_label(Color32::DARK_RED,"Legacy fit summaries lack captured prediction traces; rerun before treating them as verified evidence.");}if f.validation_influenced{ui.colored_label(Color32::DARK_RED,"Validation-influenced candidate; needs fresh confirmation.");}for r in &f.scores{ui.label(format!("ID {} · {} · baseline {:?} → candidate {:?} rad {}",r.device,r.split,r.baseline.as_ref().map(|s|s.rmse),r.candidate.as_ref().map(|s|s.rmse),r.failure.clone().unwrap_or_default()));}ui.collapsing("Optimizer history",|ui|{ui.monospace(serde_json::to_string_pretty(&f.optimizer).unwrap());});if ui.button("Use fitted model for selected controller device").clicked(){adopt=Some(i);}});}
                    if let Some(i)=adopt{let device=s.refinement.experiment.device;match sim_runtime::experiment_study::refinement::apply(s,sim_runtime::experiment_study::refinement::Command::UseFit{fit:i,device:Some(device)}){Ok(())=>changed=true,Err(e)=>s.refinement.failures.push(e)}}
                    ui.separator();ui.strong("Fit full recorded PWM histories");
                    ui.label("Freeze whole-run roles and prediction limits, then fit the same parameters to written PWM and captured timing. The simulated feedback controller is not in this fitting objective; validate its own-feedback behavior separately.");
                    ui.small("These imported recordings have been available for inspection. Fits are conservatively labelled validation-influenced and need fresh confirmation. A reserved run cannot later be assigned to tuning in this review.");
                    ui.horizontal(|ui|{ui.label("Split rationale");ui.text_edit_singleline(&mut self.assignment_rationale);});
                    for (i,r) in s.refinement.recordings.iter().enumerate(){ui.push_id(("assignment",i),|ui|{ui.horizontal(|ui|{
                        let hash=r.fingerprint();ui.label(format!("#{} ID {} · {}",i+1,r.experiment.device,r.experiment.name));
                        if let Some(a)=s.refinement.recording_assignments.iter().find(|a|a.recording_hash==hash){ui.label(format!("Frozen: {} · RMS limit {:.6} rad",a.role.split(),a.limits.rmse));}
                        else {for (label,role) in [("Use for tuning",data::Role::Train),("Reserve validation",data::Role::HeldOut)] {if ui.add_enabled(!busy&&r.completed&&!self.assignment_rationale.trim().is_empty(),egui::Button::new(label)).clicked(){let limits=s.limits.clone().unwrap_or(sim_runtime::experiment_comparison::Limits{rmse:3.*r.experiment.timing.encoder_quantum_rad,final_abs_error:5.*r.experiment.timing.encoder_quantum_rad});s.refinement.recording_assignments.push(data::Assignment{recording_hash:hash.clone(),role,limits,rationale:self.assignment_rationale.clone()});}}}
                    });});}
                    let train=s.refinement.recording_assignments.iter().filter(|a|a.role==data::Role::Train).count();let held=s.refinement.recording_assignments.len()-train;
                    if ui.add_enabled(!busy&&train>0&&held>0&&!s.refinement.coordinates.is_empty(),egui::Button::new(format!("Fit command histories · {train} tuning / {held} validation · 40 evaluations"))).clicked(){action=Some(Action::FitRecordings);s.validation_seen=true;changed=true;}
                    ui.separator();ui.strong("Fit one model across experiment types");
                    ui.label("Combine the filtered pulse/release trials above with every assigned recording. Each whole trial has equal residual weight after scaling by encoder resolution; original roles and limits remain frozen.");
                    ui.horizontal(|ui|{ui.label("Optional additional saved study");ui.text_edit_singleline(&mut self.combined_study_path);});
                    ui.small(format!("The additional study contributes every pulse/release repetition for controller device ID {}. Sources are captured in the result; duplicate trial IDs are rejected.",s.refinement.experiment.device));
                    if ui.add_enabled(!busy&&!s.refinement.coordinates.is_empty(),egui::Button::new("Fit combined evidence · 40 evaluations")).clicked(){action=Some(Action::FitCombined{selected:super::filtered(s),additional_study:self.combined_study_path.clone()});s.validation_seen=true;changed=true;}
                    let mut adopted=None;
                    for (kind,i,attempt) in s.refinement.recording_fits.iter().enumerate().map(|(i,r)|("Command-history",i,&r.attempt)).chain(s.refinement.combined_fits.iter().enumerate().map(|(i,r)|("Combined",i,&r.attempt))){ui.push_id((kind,i),|ui|{ui.collapsing(format!("{kind} fit {} · {} evaluations",i+1,attempt.evaluations.len()),|ui|{
                        if let Some(error)=&attempt.failure{ui.colored_label(Color32::DARK_RED,error);}
                        if let Some(fit)=&attempt.outcome{ui.label(&fit.status);ui.label(format!("{} tuning / {} validation trials",fit.training_ids.len(),fit.validation_ids.len()));if fit.validation_influenced{ui.small("Inspected validation data: fresh confirmation still required.");}for score in &fit.scores{ui.label(format!("{} · ID {} · {} · {:.8} → {:.8} rad RMS",score.id,score.device,score.split,score.baseline.as_ref().map(|s|s.rmse).unwrap_or(f64::NAN),score.candidate.as_ref().map(|s|s.rmse).unwrap_or(f64::NAN)));if let Some(score)=&score.candidate{ui.colored_label(if score.passes{Color32::DARK_GREEN}else{Color32::DARK_RED},if score.passes{"PASS captured limits"}else{"FAIL captured limits"});}if let Some(error)=&score.failure{ui.colored_label(Color32::DARK_RED,error);}}
                            if ui.button("Use this candidate for selected controller device").clicked(){adopted=Some((if kind=="Combined" {"combined"} else {"recording"},i));}
                        }
                        ui.collapsing("Frozen dataset, request and objective history",|ui|{let json=if kind=="Combined"{serde_json::to_string_pretty(&s.refinement.combined_fits[i])}else{serde_json::to_string_pretty(&s.refinement.recording_fits[i])};ui.monospace(json.unwrap());});
                    });});}
                    if let Some((kind,index))=adopted{let device=s.refinement.experiment.device;match sim_runtime::experiment_study::refinement::apply(s,sim_runtime::experiment_study::refinement::Command::UseRecordingFit{kind:kind.into(),index,device:Some(device)}){Ok(())=>changed=true,Err(e)=>s.refinement.failures.push(e)}}

                }
                3=>{
                    ui.strong("Controller behavior across model and timing hypotheses");
                    ui.label("Run a captured controller against named model/timing scenarios. Each scenario requires evidence or an explicit hypothesis. Empty selection uses baseline, candidate, one extra command tick and 25% more fixture inertia.");
                    ui.collapsing("Edit captured scenarios",|ui|{
                        if ui.button("Add current candidate scenario").clicked(){s.refinement.scenarios.push(cal::Variant{label:format!("Candidate scenario {}",s.refinement.scenarios.len()+1),model:s.draft.clone(),timing:s.refinement.experiment.timing.clone(),evidence:"Exploratory scenario; enter evidence and uncertainty assumptions before interpretation".into()});self.scenario_editor=serde_json::to_string_pretty(&s.refinement.scenarios).unwrap();}
                        if ui.button("Load scenario JSON into editor").clicked(){self.scenario_editor=serde_json::to_string_pretty(&s.refinement.scenarios).unwrap();}
                        ui.add(egui::TextEdit::multiline(&mut self.scenario_editor).code_editor().desired_rows(14).desired_width(f32::INFINITY));
                        if ui.button("Apply validated scenarios").clicked(){let parsed=(||->Result<Vec<cal::Variant>,String>{let variants:Vec<cal::Variant>=serde_json::from_str(&self.scenario_editor).map_err(|e|e.to_string())?;if variants.len()>32{return Err("At most 32 scenarios".into());}let mut names=std::collections::BTreeSet::new();for v in &variants{if v.label.is_empty()||v.evidence.is_empty()||!names.insert(&v.label){return Err("Distinct names and evidence are required".into());}v.model.validate()?;let mut e=s.refinement.experiment.clone();e.timing=v.timing.clone();e.validate()?;}Ok(variants)})();match parsed{Ok(v)=>{s.refinement.scenarios=v;self.scenario_error=None;},Err(e)=>self.scenario_error=Some(e)}}
                        if let Some(e)=&self.scenario_error{ui.colored_label(Color32::DARK_RED,e);}
                        ui.small(format!("{} captured scenarios; leave [] to use four explicit presets",s.refinement.scenarios.len()));
                    });
                    if ui.add_enabled(!busy,egui::Button::new("Run robustness scenarios")).clicked(){action=Some(Action::Robustness);}
                    for (i,r) in s.refinement.robustness.iter().enumerate().rev(){ui.collapsing(format!("Robustness evaluation {}",i+1),|ui|{ui.small(&r.interpretation);for (label,evidence,run) in &r.runs{ui.label(format!("{label}: {}",run.score.as_ref().map(|s|format!("RMS {:.5} rad · saturated {:.1}% · {}",s.rms_rad,s.saturation_fraction*100.,if s.passes{"tracking pass"}else{"tracking fail"})).unwrap_or("unscored".into())));ui.small(evidence);if let Some(error)=&run.failure{ui.colored_label(Color32::DARK_RED,error);}}for e in &r.failures{ui.colored_label(Color32::DARK_RED,e);}});}
                }
                4=>{
                    ui.strong("Compare a captured controller with the real motor");
                    ui.label("Import recording.json or fpga-recording.json from a supervised bench controller run. FPGA runs appear in FPGA experiments. Import verifies that recorded feedback reproduces the controller's calculations and written PWM quantization.");
                    ui.horizontal(|ui|{ui.text_edit_singleline(&mut self.recording_path);if ui.add_enabled(!busy,egui::Button::new("Import recording")).clicked(){action=Some(Action::Import(self.recording_path.clone()));}});
                    let recording_label=|i:usize,r:&recording::Recording|format!("#{} · ID {} · {} · {} frames · {}",i+1,r.experiment.device,r.experiment.name,r.frames.len(),if r.completed{"complete"}else{"incomplete / unscored"});
                    egui::ComboBox::from_id_salt("selected-hardware-recording").width(720.).selected_text(s.refinement.recordings.get(self.selected_recording).map(|r|recording_label(self.selected_recording,r)).unwrap_or("No recordings imported".into())).show_ui(ui,|ui|{for (i,r) in s.refinement.recordings.iter().enumerate(){ui.selectable_value(&mut self.selected_recording,i,recording_label(i,r));}});
                    if let Some(r)=s.refinement.recordings.get(self.selected_recording){
                        ui.label(format!("Controller calculations reproduced · stop verified: {}",r.stop_verified));ui.small(&r.timing_evidence);ui.label(&r.experiment.fixture);
                        if let Some(error)=&r.failure{ui.colored_label(Color32::DARK_RED,error);}
                        ui.collapsing("Fixture, CAD association and measurement uncertainty",|ui|{
                            if ui.button("Prepare setup snapshot").clicked(){self.context_editor=serde_json::to_string_pretty(&sim_runtime::controller_refinement::context::CaptureContext::unknown(r)).unwrap();}
                            ui.small("Document attached hardware, geometry/mass artifact hashes, coordinate frames and initial pose. Unknown values remain null; append a revision to preserve earlier declarations.");
                            ui.add(egui::TextEdit::multiline(&mut self.context_editor).code_editor().desired_rows(12).desired_width(f32::INFINITY));
                            if ui.button("Capture setup revision").clicked(){let result=(||->Result<sim_runtime::controller_refinement::context::CaptureContext,String>{let c:sim_runtime::controller_refinement::context::CaptureContext=serde_json::from_str(&self.context_editor).map_err(|e|e.to_string())?;c.validate()?;if c.recording_hash!=r.fingerprint(){return Err("Snapshot must describe the selected recording".into());}if let Some(id)=&r.experiment.component_id{if c.bindings.iter().any(|b|b.hardware_id==r.experiment.device&&&b.cad_component_id!=id){return Err("Binding conflicts with captured CAD identity".into());}}Ok(c)})();match result{Ok(c)=>{s.refinement.capture_contexts.push(c);self.context_error=None;},Err(e)=>self.context_error=Some(e)}}
                            if let Some(e)=&self.context_error{ui.colored_label(Color32::DARK_RED,e);}
                            for (i,c) in s.refinement.capture_contexts.iter().enumerate().filter(|(_,c)|c.recording_hash==r.fingerprint()){ui.collapsing(format!("Setup revision {}",i+1),|ui|{ui.monospace(serde_json::to_string_pretty(c).unwrap());});}
                        });

                        ui.horizontal(|ui|{for (label,purpose) in [("Predict recorded PWM response",recording::Purpose::RecordedCommandReplay),("Predict own-feedback closed loop",recording::Purpose::ClosedLoopPrediction)]{if ui.add_enabled(!busy&&r.completed,egui::Button::new(label)).clicked(){action=Some(Action::Predict(self.selected_recording,purpose));}}});
                        ui.small("Both simulations use the captured host schedule. Command replay freezes measured PWM; closed-loop prediction computes new PWM from its own simulated feedback. Timing midpoints and constant initial supply/temperature remain assumptions.");
                        let recording_hash=r.fingerprint();
                        for (prediction_index,p) in s.refinement.predictions.iter().enumerate().rev().filter(|(_,p)|p.recording_hash==recording_hash){
                            ui.separator();ui.strong(format!("{} · model RMS {:.5} rad · peak {:.5} rad · {}",match p.purpose {recording::Purpose::ClosedLoopPrediction=>"Closed-loop prediction",recording::Purpose::RecordedCommandReplay=>"Recorded PWM replay"},p.model_error.rmse,p.model_error.maximum_abs_error,if p.model_error.passes{"prediction pass"}else{"prediction fail"}));
                            if p.model!=s.draft{ui.colored_label(Color32::DARK_RED,"Captured model differs from the current draft.");}
                            chart(ui,"Measured / predicted position (rad)",&[("Measured",Color32::from_rgb(30,130,200),p.measured.samples.iter().map(|s|[s.time_s,s.value]).collect()),("Predicted",Color32::from_rgb(50,160,100),p.predicted.samples.iter().map(|s|[s.time_s,s.value]).collect())]);
                            for (label,score) in [("Measured controller",&p.measured_tracking),("Simulated controller",&p.simulated_tracking)]{if let Some(score)=score{ui.label(format!("{label}: tracking RMS {:.5} rad · {:.1}% saturated",score.rms_rad,score.saturation_fraction*100.));}}
                            ui.push_id(prediction_index,|ui|{
                                ui.collapsing("Transient response",|ui|{for (label,trace) in [("Measured",&p.measured),("Predicted",&p.predicted)]{ui.strong(label);for result in sim_runtime::controller_refinement::transients::trajectory(&r.experiment,trace){match result{Ok(response)=>{ui.label(format!("{:.2}s: {:.3} → {:.3} rad; sampled overshoot {:.5} rad",response.transition_start_s,response.from_rad,response.target_rad,response.sampled_overshoot_rad));for (label,value) in [("Onset delay",response.onset_delay),("10–90% rise",response.rise_10_to_90),("Settling after hold",response.settling_after_hold)]{ui.label(format!("{label}: {}",value.interval_s.map(|v|format!("{:.4}–{:.4} s",v[0],v[1])).unwrap_or_else(||format!("Unresolved: {}",value.unresolved.unwrap_or_default()))));}ui.small(response.interpretation);},Err(e)=>{ui.label(e);}}}}});
                                ui.collapsing("Prediction assumptions",|ui|{ui.small(&p.assumptions);});
                            });
                        }
                    }
                }
                7=>{action=self.fpga_ui.show(ui,s,busy);}
                6=>{let (edited,power_action)=self.power_ui.show(ui,s,busy,study_id);changed|=edited;action=power_action;}
                _=>{
                    ui.strong("Review refined physical properties before carrying them into CAD");
                    ui.label("The proposal maps the explicitly selected hardware ID to a stable motor ID in a physical CAD export. It records estimated values, unknown uncertainty and unloaded test scope.");
                    ui.horizontal(|ui|{ui.label("Source physical CAD JSON");ui.text_edit_singleline(&mut self.cad_path);});
                    let mut component=s.refinement.experiment.component_id.clone().unwrap_or_default();
                    ui.horizontal(|ui|{ui.label("Stable CAD motor ID");if ui.text_edit_singleline(&mut component).changed(){s.refinement.experiment.component_id=(!component.is_empty()).then_some(component);}});
                    if ui.add_enabled(!busy,egui::Button::new("Prepare candidate property diff")).clicked(){action=Some(Action::Propose(self.cad_path.clone()));}
                    for (i,p) in s.refinement.cad_proposals.iter().enumerate().rev(){ui.collapsing(format!("Proposal {} · motor {} → CAD {}",i+1,p.hardware_id,p.component_id),|ui|{
                        ui.small(&p.tested_scope);
                        for c in &p.changes{ui.label(format!("{}: {} → {} {}",c.parameter,c.previous.map(|v|v.to_string()).unwrap_or("not declared".into()),c.proposed,c.unit));ui.small(format!("{}; {}",c.provenance,c.uncertainty));}
                        for u in &p.unmapped{ui.colored_label(Color32::DARK_RED,u);}
                        ui.horizontal(|ui|{ui.label("New accepted CAD filename");ui.text_edit_singleline(&mut self.cad_output);});
                        ui.label("Acceptance decision and remaining limitations");ui.text_edit_multiline(&mut self.cad_decision);
                        if ui.add_enabled(!busy&&p.unmapped.is_empty()&&!self.cad_decision.trim().is_empty()&&!self.cad_output.is_empty(),egui::Button::new("Accept proposal and save a new CAD artifact")).clicked(){action=Some(Action::Accept(i,self.cad_path.clone(),self.cad_output.clone(),self.cad_decision.clone()));}
                        ui.small("Source changes invalidate the proposal. The new artifact keeps other CAD fields and carries provenance into the shared robot runtime. Loaded-joint, leg and quadruped revalidation remain separate.");
                    });}
                    for a in &s.refinement.cad_acceptances{ui.label(format!("Accepted artifact: {} · {}",a.new_file,a.artifact_hash));}
                }
            }
            if !s.refinement_evidence.terminals.is_empty() {
                ui.collapsing("Retained unapplied electrical terminals — UNSCORED",|ui| {
                    for reference in &s.refinement_evidence.terminals {
                        ui.label(format!("{} · cancelled {} · unapplied {} · UNSCORED · {} bytes · {}",reference.kind,reference.cancelled,reference.unapplied,reference.content_ref.byte_length,reference.content_ref.blake3));
                        if let Some(error)=&reference.failure {ui.colored_label(Color32::DARK_RED,error);}
                        match sim_runtime::experiment_study::terminal::cached(s,reference) {
                            Some(sim_runtime::experiment_study::refinement::ResultData::Controller(run))=> {
                                ui.label(format!("Exact diagnostics: {} controller frames, {} truth samples; failure {:?}",run.frames.len(),run.truth.len(),run.failure));
                                if let Some(trace)=&run.electrical {ui.label(format!("{} electrical samples · sampled peak draw {} W · drawn {} J · returned {} J; no acceptance",trace.samples.len(),trace.summary.peak_draw_power_w,trace.summary.drawn_energy_j,trace.summary.returned_energy_j));}
                            },
                            Some(sim_runtime::experiment_study::refinement::ResultData::Prediction(p))=>{ui.label(format!("Exact prediction {} · {:?} · {} simulated frames; no acceptance",p.recording_hash,p.purpose,p.simulated_frames.len()));},
                            Some(sim_runtime::experiment_study::refinement::ResultData::Electrical(_))=>{ui.label("Exact calibrated comparison retained in immutable report diagnostics; no acceptance");},
                            _=>{ui.colored_label(Color32::DARK_RED,"Terminal diagnostics unavailable; UNSCORED");},
                        }
                    }
                });
            }
            if !s.refinement.failures.is_empty(){ui.collapsing("Retained failures and cancellations",|ui|{for e in &s.refinement.failures{ui.colored_label(Color32::DARK_RED,e);}});}
        });
        // Legacy widgets edit a temporary projection; reusable validators own commit.
        let mut validated = s.clone();
        validated.refinement.experiment=original.refinement.experiment.clone();
        validated.refinement.coordinates=original.refinement.coordinates.clone();
        validated.refinement.scenarios=original.refinement.scenarios.clone();
        let result = (|| -> Result<(),String> {
            use sim_runtime::experiment_study::refinement::{self as shared, Command};
            if s.refinement.experiment != original.refinement.experiment { shared::apply(&mut validated,Command::SetExperiment(s.refinement.experiment.clone()))?; }
            if serde_json::to_value(&s.refinement.coordinates).unwrap() != serde_json::to_value(&original.refinement.coordinates).unwrap() { shared::apply(&mut validated,Command::SetCoordinates(s.refinement.coordinates.clone()))?; }
            if serde_json::to_value(&s.refinement.scenarios).unwrap() != serde_json::to_value(&original.refinement.scenarios).unwrap() { shared::apply(&mut validated,Command::SetScenarios(s.refinement.scenarios.clone()))?; }
            if let Some(recording)=s.refinement.recordings.get(self.selected_recording) {
                let hash=recording.fingerprint();
                if validated.refinement_evidence.selected_recording.as_ref()!=Some(&hash) {
                    shared::apply(&mut validated,Command::SelectRecording{recording_hash:hash})?;
                }
            }
            if !s.refinement.capture_contexts.starts_with(&original.refinement.capture_contexts) { return Err("refinement.capture_contexts: immutable revisions cannot be replaced".into()); }
            validated.refinement.capture_contexts=original.refinement.capture_contexts.clone();
            for context in s.refinement.capture_contexts.iter().skip(original.refinement.capture_contexts.len()) {
                shared::apply(&mut validated,Command::AppendContext{context:context.clone()})?;
            }
            if !s.refinement.recording_assignments.starts_with(&original.refinement.recording_assignments) { return Err("refinement.recording_assignments: frozen assignments cannot be replaced".into()); }
            validated.refinement.recording_assignments=original.refinement.recording_assignments.clone();
            for assignment in s.refinement.recording_assignments.iter().skip(original.refinement.recording_assignments.len()) {
                shared::apply(&mut validated,Command::AssignRecording{assignment:assignment.clone()})?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            let rejected=serde_json::json!({"experiment":s.refinement.experiment,"coordinates":s.refinement.coordinates,"scenarios":s.refinement.scenarios,"capture_contexts":s.refinement.capture_contexts,"recording_assignments":s.refinement.recording_assignments});
            s.refinement.failures.push(format!("Legacy rejected authoring projection {rejected}: {error}"));
            s.refinement.experiment=original.refinement.experiment;
            s.refinement.coordinates=original.refinement.coordinates;
            s.refinement.scenarios=original.refinement.scenarios;
            s.refinement.capture_contexts=original.refinement.capture_contexts;
            s.refinement.recording_assignments=original.refinement.recording_assignments;
            s.refinement.failures.push(error);
            changed=true;
        } else {
            *s=validated;
        }
        (
            action,
            changed
                || serde_json::to_vec(&(
                    &s.refinement.experiment,
                    &s.refinement.coordinates,
                    &s.refinement.scenarios,
                    &s.refinement.capture_contexts,
                    &s.refinement.recording_assignments,
                    &s.refinement.fpga_design_drafts,
                    &s.refinement_evidence.selected_recording,
                ))
                .unwrap()
                    != before,
        )
    }
}

pub(super) fn chart(ui: &mut egui::Ui, label: &str, series: &[(&str, Color32, Vec<[f64; 2]>)]) {
    draw_chart(ui, label, series, true);
}
pub(super) fn voltage_chart(
    ui: &mut egui::Ui,
    label: &str,
    series: &[(&str, Color32, Vec<[f64; 2]>)],
) {
    draw_chart(ui, label, series, false);
}
fn draw_chart(
    ui: &mut egui::Ui,
    label: &str,
    series: &[(&str, Color32, Vec<[f64; 2]>)],
    include_zero: bool,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        for (label, color, _) in series {
            ui.colored_label(*color, *label);
        }
    });
    let (response, painter) = ui.allocate_painter(
        egui::vec2(ui.available_width().max(100.), 160.),
        egui::Sense::hover(),
    );
    let rect = egui::Rect::from_min_max(
        response.rect.min + egui::vec2(48., 10.),
        response.rect.max - egui::vec2(12., 25.),
    );
    let points = series
        .iter()
        .flat_map(|(_, _, s)| s.iter())
        .filter(|p| p.iter().all(|v| v.is_finite()))
        .collect::<Vec<_>>();
    if points.is_empty() {
        return;
    }
    let xmax = points.iter().map(|p| p[0]).fold(0., f64::max).max(1e-6);
    let ymin = points
        .iter()
        .map(|p| p[1])
        .fold(if include_zero { 0. } else { f64::INFINITY }, f64::min);
    let ymax = points
        .iter()
        .map(|p| p[1])
        .fold(if include_zero { 0. } else { f64::NEG_INFINITY }, f64::max)
        .max(ymin + if include_zero { 1e-6 } else { 0.001 });
    let pos = |p: [f64; 2]| {
        egui::pos2(
            rect.left() + rect.width() * (p[0] / xmax) as f32,
            rect.bottom() - rect.height() * ((p[1] - ymin) / (ymax - ymin)) as f32,
        )
    };
    painter.rect_stroke(
        rect,
        0.,
        egui::Stroke::new(1., Color32::GRAY),
        egui::StrokeKind::Inside,
    );
    for tick in 0..=4 {
        let fraction = tick as f64 / 4.;
        let x = rect.left() + rect.width() * fraction as f32;
        let y = rect.bottom() - rect.height() * fraction as f32;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(0.5, Color32::LIGHT_GRAY),
        );
        painter.text(
            egui::pos2(x, rect.bottom() + 5.),
            egui::Align2::CENTER_TOP,
            format!("{:.2}s", xmax * fraction),
            egui::FontId::proportional(10.),
            Color32::DARK_GRAY,
        );
        painter.text(
            egui::pos2(rect.left() - 5., y),
            egui::Align2::RIGHT_CENTER,
            format!("{:.3}", ymin + (ymax - ymin) * fraction),
            egui::FontId::proportional(10.),
            Color32::DARK_GRAY,
        );
    }
    for (_, color, s) in series {
        for w in s.windows(2) {
            painter.line_segment([pos(w[0]), pos(w[1])], egui::Stroke::new(1.5, *color));
        }
    }
    if let Some(p) = response.hover_pos() {
        let time = ((p.x - rect.left()) / rect.width()).clamp(0., 1.) as f64 * xmax;
        painter.line_segment(
            [egui::pos2(p.x, rect.top()), egui::pos2(p.x, rect.bottom())],
            egui::Stroke::new(1., Color32::GRAY),
        );
        response.on_hover_ui(|ui| {
            ui.label(format!("t = {time:.4} s"));
            for (name, _, s) in series {
                if let Some(p) = s
                    .iter()
                    .min_by(|a, b| (a[0] - time).abs().total_cmp(&(b[0] - time).abs()))
                {
                    ui.label(format!("{name}: {:.6} (sample t={:.4})", p[1], p[0]));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_refinement_views_render_without_mutating_the_review() {
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let archive = sim_runtime::experiment_comparison::hx_archive::load(
            &repository.join("examples/actuators/hx30hm/pwm-identification"),
            &repository,
        )
        .unwrap();
        let mut study = Study::new(archive).unwrap();
        let r:recording::Recording=serde_json::from_slice(&std::fs::read(repository.join("examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/controller-id4/recording.json")).unwrap()).unwrap();
        r.validate().unwrap();
        study.refinement_evidence.selected_recording=Some(r.fingerprint());
        study.refinement.recordings.push(r);
        let path=repository.join("examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/nine-faster-7p5pct-validation/fpga-recording.json");
        let imported = Action::Import(path.display().to_string())
            .run(study.clone(), &AtomicBool::new(false), |_, _| {})
            .unwrap();
        match imported {
            Outcome::FpgaRecording(r) => study.refinement.fpga_recordings.push(r),
            _ => panic!("Wrong import adapter"),
        }
        assert!(
            Action::Import(path.display().to_string())
                .run(study.clone(), &AtomicBool::new(false), |_, _| {})
                .is_err()
        );
        let cancelled = Action::ReviewFpga(
            0,
            sim_runtime::controller_refinement::fpga_review::Mode::ClosedLoop,
        )
        .run(study.clone(), &AtomicBool::new(true), |_, _| {})
        .unwrap();
        match cancelled {
            Outcome::FpgaReview(r) => {
                assert_eq!(r.failures.len(), 9);
                study.refinement.fpga_reviews.push(r);
            }
            _ => panic!("Wrong review adapter"),
        }
        let r = &study.refinement.fpga_recordings[0];
        study.refinement.fpga_design_drafts.push(
            sim_runtime::controller_refinement::fpga_design::Experiment {
                timing_recording_hash: r.fingerprint(),
                plan: r.plan.clone(),
            },
        );
        study.validate().unwrap();
        let ctx = egui::Context::default();
        let mut state = State::default();
        for section in 0..8 {
            state.section = section;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1380., 900.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let (action, changed) = state.show(ui, &mut study, false);
                    assert!(action.is_none());
                    assert!(!changed);
                },
            );
            assert!(!output.shapes.is_empty());
            output.textures_delta.clear();
        }
    }
}

#[cfg(test)]
#[path="recording_tests.rs"]
mod recording_tests;
