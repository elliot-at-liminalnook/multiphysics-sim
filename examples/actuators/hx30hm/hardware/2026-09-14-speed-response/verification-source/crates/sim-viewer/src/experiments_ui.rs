//! Experiment host UI. Models, comparison, validation and retained evidence are shared Rust APIs.
mod fpga_ui;
mod motor_response_ui;
mod fpga_design_ui;
mod plots;
mod power_ui;
mod refinement;
use eframe::egui::{self, Color32};
use sim_runtime::{
    experiment_comparison::{Limits, hx_archive},
    experiment_study::{self as study, Evaluation, Study},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
};

enum ResultMessage {
    Loaded(Result<Study, String>),
    Evaluated(usize, Result<Evaluation, String>),
    Saved(usize, u64, bool, Result<String, String>),
    Refined(usize, String, Result<refinement::Outcome, String>),
}
struct Job {
    rx: mpsc::Receiver<ResultMessage>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
    total: usize,
    label: String,
}
pub struct ExperimentsPanel {
    pub open: bool,
    pub confirm_exit: bool,
    studies: Vec<Study>,
    current: usize,
    job: Option<Job>,
    path: String,
    save_path: String,
    message: Option<String>,
    revisions: Vec<u64>,
    saved_revisions: Vec<u64>,
    plot: plots::PlotState,
    parameter_info: Vec<(String, String, String, String, Option<f64>)>,
    refinement_ui: refinement::State,
    refinement_tab: bool,
}
fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
impl Default for ExperimentsPanel {
    fn default() -> Self {
        Self {
            open: false,
            confirm_exit: false,
            studies: vec![],
            current: 0,
            job: None,
            path: repository()
                .join("examples/actuators/hx30hm/pwm-full-range-identification")
                .display()
                .to_string(),
            save_path: repository()
                .join("experiment-review.json")
                .display()
                .to_string(),
            message: None,
            revisions: vec![],
            saved_revisions: vec![],
            plot: Default::default(),
            parameter_info: parameter_info(),
            refinement_ui: Default::default(),
            refinement_tab: false,
        }
    }
}
impl Drop for ExperimentsPanel {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}
impl ExperimentsPanel {
    pub fn loading(&self) -> bool {
        self.job.is_some()
    }
    pub fn has_unsaved(&self) -> bool {
        self.revisions != self.saved_revisions || self.job.is_some()
    }
    pub fn launch(&mut self, path: Option<String>, ctx: Option<&egui::Context>) {
        self.open = true;
        if path.is_none() && !self.studies.is_empty() {
            return;
        }
        if self.loading() {
            return;
        }
        if let Some(path) = path {
            self.path = path;
        }
        let path = PathBuf::from(&self.path);
        self.start_job(
            "Loading measurements / review",
            0,
            ctx,
            move |cancel, progress| {
                ResultMessage::Loaded(if path.is_dir() && path.join("sweep.csv").exists() {
                    sim_runtime::controller_refinement::sweep_review::load(
                        &path,
                        &path.parent().unwrap_or(&path).join("split-policy.json"),
                        &cancel,
                        |n, _| {
                            progress.store(n, Ordering::Relaxed);
                        },
                    )
                } else if path.is_dir() {
                    hx_archive::load(&path, &repository()).and_then(Study::new)
                } else {
                    Study::load(&path)
                })
            },
        );
    }
    fn start_job(
        &mut self,
        label: &str,
        total: usize,
        ctx: Option<&egui::Context>,
        run: impl FnOnce(Arc<AtomicBool>, Arc<AtomicUsize>) -> ResultMessage + Send + 'static,
    ) {
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicUsize::new(0));
        let c = cancel.clone();
        let p = progress.clone();
        let ctx = ctx.cloned();
        std::thread::spawn(move || {
            let result = run(c, p);
            let _ = tx.send(result);
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        self.job = Some(Job {
            rx,
            cancel,
            progress,
            total,
            label: label.into(),
        });
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else {
            return;
        };
        let result = match job.rx.try_recv() {
            Ok(v) => Some(v),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.message =
                    Some("Background task stopped unexpectedly; existing results retained".into());
                self.job = None;
                return;
            }
        };
        if let Some(result) = result {
            self.job = None;
            match result {
                ResultMessage::Loaded(Ok(mut s)) => {
                    if !s.refinement.recordings.is_empty() {
                        self.refinement_tab = true;
                        self.refinement_ui.select_hardware();
                    }
                    if s.refinement
                        .controller_runs
                        .last()
                        .is_some_and(|r| r.electrical.is_some())
                        || !s.refinement.electrical_comparisons.is_empty()
                    {
                        self.refinement_tab = true;
                        self.refinement_ui.select_power();
                    }
                    if !s.refinement.fpga_recordings.is_empty() {
                        self.refinement_tab = true;
                        self.refinement_ui.select_fpga();
                    }
                    if s.view.trial_id.is_none() {
                        s.view.trial_id = s
                            .archive
                            .trials
                            .iter()
                            .find(|t| !t.comparison.passes)
                            .or(s.archive.trials.first())
                            .map(|t| t.id.clone());
                    }
                    self.studies.push(s);
                    self.revisions.push(0);
                    self.saved_revisions.push(0);
                    self.current = self.studies.len() - 1;
                    self.plot = Default::default();
                    self.message = Some("Loaded. Original measurement splits preserved.".into());
                }
                ResultMessage::Loaded(Err(e)) => self.message = Some(e),
                ResultMessage::Evaluated(index, result) => match result {
                    Ok(e) => {
                        let s = &mut self.studies[index];
                        s.evaluations.push(e);
                        s.view.evaluation = Some(s.evaluations.len() - 1);
                        self.revisions[index] += 1;
                        self.message = Some(
                            "Evaluation retained. Failed or cancelled trials are unscored.".into(),
                        );
                    }
                    Err(e) => self.message = Some(e),
                },
                ResultMessage::Saved(index, revision, html, result) => {
                    if result.is_ok() && !html {
                        self.saved_revisions[index] = revision;
                    }
                    self.message = Some(result.unwrap_or_else(|e| e));
                }
                ResultMessage::Refined(index, label, result) => {
                    let w = &mut self.studies[index].refinement;
                    match result {
                        Ok(refinement::Outcome::Controller(run)) => w.controller_runs.push(run),
                        Ok(refinement::Outcome::Sensitivity(report)) => {
                            w.sensitivities.push(report)
                        }
                        Ok(refinement::Outcome::FitAttempt(attempt)) => {
                            if let Some(fit) = &attempt.outcome {
                                w.fits.push(fit.clone());
                            }
                            if let Some(error) = &attempt.failure {
                                w.failures.push(format!("Fitting: {error}"));
                            }
                            w.fit_attempts.push(attempt);
                        }
                        Ok(refinement::Outcome::CombinedFit(attempt)) => {
                            if let Some(error) = &attempt.attempt.failure {
                                w.failures.push(format!("Combined fitting: {error}"));
                            }
                            w.combined_fits.push(attempt);
                        }
                        Ok(refinement::Outcome::RecordingFit(attempt)) => {
                            if let Some(error) = &attempt.attempt.failure {
                                w.failures
                                    .push(format!("Captured-command fitting: {error}"));
                            }
                            w.recording_fits.push(attempt);
                        }
                        Ok(refinement::Outcome::Robustness(report)) => w.robustness.push(report),
                        Ok(refinement::Outcome::FpgaRecording(report)) => w.fpga_recordings.push(report),
                        Ok(refinement::Outcome::FpgaReview(report)) => w.fpga_reviews.push(report),
                        Ok(refinement::Outcome::FpgaDesign(report)) => w.fpga_design_runs.push(report),
                        Ok(refinement::Outcome::FpgaPlanSaved(path)) => {self.message=Some(format!("Experiment plan saved: {path}. No hardware action performed."));self.revisions[index]+=1;return;},
                        Ok(refinement::Outcome::FpgaFit(report)) => w.fpga_fits.push(report),
                        Ok(refinement::Outcome::Recording(report)) => w.recordings.push(report),
                        Ok(refinement::Outcome::Prediction(report)) => w.predictions.push(report),
                        Ok(refinement::Outcome::Electrical(report)) => {
                            w.electrical_comparisons.push(report)
                        }
                        Ok(refinement::Outcome::Proposal(report)) => w.cad_proposals.push(report),
                        Ok(refinement::Outcome::Accepted(report)) => w.cad_acceptances.push(report),
                        Err(error) => w.failures.push(format!("{label}: {error}")),
                    }
                    self.revisions[index] += 1;
                    self.message = Some(format!("{label}: result retained in this review."));
                }
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
    }
    fn run(&mut self, ids: Vec<String>, ctx: &egui::Context) {
        if self.loading() {
            return;
        }
        let index = self.current;
        let s = &mut self.studies[index];
        if ids.iter().any(|id| {
            s.archive
                .trials
                .iter()
                .any(|t| &t.id == id && t.split != "train")
        }) {
            s.validation_seen = true;
        }
        let s = s.clone();
        let count = ids.len();
        self.start_job(
            "Simulating baseline and candidate",
            count,
            Some(ctx),
            move |cancel, progress| {
                ResultMessage::Evaluated(
                    index,
                    study::evaluate(
                        &s.archive,
                        &ids,
                        &s.baseline,
                        &s.draft,
                        s.limits,
                        s.validation_influenced,
                        &cancel,
                        |n, _| {
                            progress.store(n, Ordering::Relaxed);
                        },
                    ),
                )
            },
        );
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        self.poll(ctx);
        let mut open = self.open;
        egui::Window::new("Experiments · measured data & model refinement").open(&mut open).default_pos([20.,45.]).default_size([1380.,800.]).resizable(true).show(ctx,|ui|{
            ui.heading("Measured response / model refinement");
            ui.small("Recorded PWM replay · Controller design · Shared physical simulator");
            ui.small("Exploratory comparison: fixture/load and proprietary controller behavior are not verified matches.");
            ui.horizontal_wrapped(|ui|{
                for (label,folder) in [("Pilot · 63 trials","pwm-identification"),("Full range · 216 trials","pwm-full-range-identification")] {
                    if ui.add_enabled(!self.loading(),egui::Button::new(label)).clicked(){
                        if let Some(i)=self.studies.iter().position(|s|s.archive.label==folder){self.current=i;self.plot=Default::default();}
                        else{self.launch(Some(repository().join("examples/actuators/hx30hm").join(folder).display().to_string()),Some(ctx));}
                    }
                }
                for (i,s) in self.studies.iter().enumerate(){if ui.selectable_label(i==self.current,format!("Review {} · {}",i+1,s.archive.label)).clicked(){self.current=i;self.plot=Default::default();}}
            });
            ui.collapsing("Open / save / export",|ui|{
                ui.horizontal(|ui|{ui.label("Archive directory or saved review");ui.text_edit_singleline(&mut self.path);if ui.add_enabled(!self.loading(),egui::Button::new("Open")).clicked(){self.launch(Some(self.path.clone()),Some(ctx));}});
                ui.horizontal(|ui|{ui.label("New output filename");ui.text_edit_singleline(&mut self.save_path);
                    for (label,html) in [("Save review",false),("Export HTML",true)] {
                        if ui.add_enabled(!self.loading()&&!self.studies.is_empty(),egui::Button::new(label)).clicked(){
                            let index=self.current;let revision=self.revisions[index];let s=self.studies[index].clone();let mut path=PathBuf::from(&self.save_path);if html{path.set_extension("html");}
                            self.start_job("Saving captured evidence",0,Some(ctx),move|_,_|{let r=if html{s.export_html_new(&path)}else{s.save_new(&path)};ResultMessage::Saved(index,revision,html,r.map(|_|format!("Saved {}. Use a new filename for another revision.",path.display()))) });
                        }
                    }
                });
                ui.small("Files are new immutable snapshots. Saving never overwrites existing evidence. Other open reviews are saved separately.");
            });
            if let Some(job)=&self.job {ui.horizontal(|ui|{ui.spinner();ui.label(&job.label);if job.total>0{ui.add(egui::ProgressBar::new(job.progress.load(Ordering::Relaxed) as f32/job.total as f32).text(format!("{} / {} trials",job.progress.load(Ordering::Relaxed),job.total)));}if ui.button("Cancel task").clicked(){job.cancel.store(true,Ordering::Relaxed);}});}
            if let Some(message)=&self.message {ui.label(message);}
            if self.studies.is_empty(){return;}
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.refinement_tab, false, "Measured PWM replay");
                ui.selectable_value(&mut self.refinement_tab, true, "Controller design & accuracy");
            });
            if self.refinement_tab {
                let busy = self.loading();
                let (action, changed) = self.refinement_ui.show(ui, &mut self.studies[self.current], busy);
                if changed { self.revisions[self.current] += 1; }
                if let Some(action) = action {
                    let index = self.current;
                    let s = self.studies[index].clone();
                    let label = action.label().to_string();
                    let job_label = label.clone();
                    self.start_job(&job_label, action.total(&s), Some(ctx), move |cancel, progress| {
                        ResultMessage::Refined(index, label, action.run(s, &cancel, |n,_| {progress.store(n,Ordering::Relaxed);}))
                    });
                }
                return;
            }
            let busy=self.loading();
            let s=&mut self.studies[self.current];
            let previous_view=serde_json::to_string(&s.view).unwrap();
            if !s.validation_seen && s.view.trial_id.as_ref().is_some_and(|id| s.archive.trials.iter().any(|t| &t.id==id && t.split!="train")) {
                s.validation_seen=true;self.revisions[self.current]+=1;
            }
            let held:Vec<_>=s.archive.trials.iter().filter(|t|t.split!="train").collect();
            ui.strong(format!("Retained reference: {} / {} held-out pass · {} total trials · {} raw source hashes verified",held.iter().filter(|t|t.comparison.passes).count(),held.len(),s.archive.trials.len(),s.archive.verified_inputs));
            ui.small(&s.archive.interpretation);
            for issue in &s.archive.integrity_issues{ui.colored_label(Color32::DARK_RED,issue);}
            let mut run_ids=None;
            ui.separator();
            ui.horizontal_top(|ui|{
                ui.vertical(|ui|{
                    ui.set_width(235.);
                    ui.strong("Measurements");
                    filters(ui,s);
                    let ids=filtered(s);
                    ui.label(format!("{} matching trials",ids.len()));
                    if ui.add_enabled(!busy&&!ids.is_empty(),egui::Button::new("Run filtered trials")).clicked(){run_ids=Some(ids.clone());}
                    if ui.add_enabled(!busy,egui::Button::new("Evaluate held-out trials")).clicked(){run_ids=Some(s.archive.trials.iter().filter(|t|t.split!="train" && (s.view.device==0 || s.view.device==t.device)).map(|t|t.id.clone()).collect());}
                    egui::ScrollArea::vertical().id_salt("trial-list").max_height(380.).show(ui,|ui|{
                        for id in &ids {let t=s.archive.trials.iter().find(|t|&t.id==id).unwrap();
                            let result=active_evaluation(s).and_then(|e|e.results.iter().find(|r|r.trial_id==*id));
                            let outcome=result.and_then(|r|r.candidate.as_ref()).map(|p|if p.metrics.passes{"PASS"}else{"FAIL"}).unwrap_or("—");
                            if ui.selectable_label(s.view.trial_id.as_ref()==Some(id),format!("{outcome} ID {} · {:+.1}% · {}",t.device,t.drive*100.,if t.split=="train"{"tune"}else{"held"})).on_hover_text(format!("{} · stage {}\n{} · {:.0} ms · encoder displacement",t.run,t.stage,t.kind,t.duration_s*1000.)).clicked(){s.view.trial_id=Some(id.clone());self.plot.reset_time();}
                        }
                    });
                    ui.small("— means physical result unscored. No-motion trials are retained.");
                });
                ui.separator();
                ui.vertical(|ui|{
                    ui.set_width((ui.available_width()-325.).max(400.));
                    egui::ScrollArea::vertical().id_salt("experiment-plots").max_height(590.).show(ui,|ui|{
                        evaluation_picker(ui,s);
                        if let Some(id)=&s.view.trial_id {
                            let t=s.archive.trials.iter().find(|t|&t.id==id).unwrap();
                            ui.strong(format!("ID {} · {:+.1}% PWM · {}",t.device,t.drive*100.,t.split.replace('_'," ")));
                            if let Some(release)=&t.release {ui.label(match release {sim_runtime::actuator_bench::DriverRelease::ElectricalBrake=>"Captured release: zero PWM with torque enabled. Model assumes electrical braking.",sim_runtime::actuator_bench::DriverRelease::TorqueOff{..}=>"Captured release: torque disabled. Model uses declared passive diode/leakage paths."});ui.small("Driver behavior is a hypothesis; device electronics remain uncalibrated.");}

                            if ui.add_enabled(!busy,egui::Button::new("Run selected trial")).clicked(){run_ids=Some(vec![id.clone()]);}
                            if let Some(e)=active_evaluation(s){if e.stale(&s.baseline,&s.draft,&s.limits){ui.colored_label(Color32::DARK_RED,"Out of date for current edits — plots retain the captured run settings.");}}
                            let result=active_evaluation(s).and_then(|e|e.results.iter().find(|r|r.trial_id==*id));
                            let limits=active_evaluation(s).and_then(|e|e.limits.as_ref()).unwrap_or(&t.limits);
                            plots::show(ui,&mut self.plot,t,result,limits);
                            ui.small(format!("{} · stage {} · {} samples · {:?} V · {:?} °C",t.run,t.stage,t.measured.samples.len(),t.voltage_range_v,t.temperature_range_c));
                            ui.collapsing("Conditions, timing and source",|ui|{
                                ui.label(study::ASSUMPTIONS);ui.label(format!("Command on {:?} s; off {:?} s",t.on_host_window_s,t.off_host_window_s));
                                ui.label(&s.archive.split_policy);ui.label(&s.archive.interpretation);
                                ui.label(format!("Extracted observations: {}\nReference model: {}",s.archive.observation_blake3,s.archive.model_blake3));
                                ui.small("Raw hashes are verified at archive import. Saved review snapshots retain that import-time status; extraction lineage is not regenerated here.");
                            });
                        }
                        summary(ui,s);
                    });
                });
                ui.separator();
                ui.vertical(|ui|{
                    ui.set_width(295.);
                    egui::ScrollArea::vertical().id_salt("candidate-editor").max_height(590.).show(ui,|ui|{
                        ui.strong("Candidate model");
                        ui.small("Baseline stays fixed. Physical parameters are provisional hypotheses.");
                        let before=s.draft.clone();let previous_limits=s.limits.clone();
                        ui.horizontal(|ui|{if ui.button("Reset to baseline").clicked(){s.draft=s.baseline.clone();}if ui.button("Use selected revision").clicked(){if let Some(e)=active_evaluation(s){s.draft=e.candidate.clone();}}});
                        for (group,name,unit,description,default) in &self.parameter_info {
                            let params=if group=="motor"{&mut s.draft.motor}else{&mut s.draft.bridge};
                            let base=if group=="motor"{&s.baseline.motor}else{&s.baseline.bridge};
                            if let Some(value)=params.get_mut(name){
                                let speed=(value.abs()*0.01).max(1e-9);
                                ui.horizontal(|ui|{ui.add(egui::DragValue::new(value).speed(speed).max_decimals(9));ui.label(format!("{} ({unit})",name.replace('_'," "))).on_hover_text(description);});
                                if base.get(name).copied().or(*default)!=Some(*value){ui.small(format!("Baseline: {} → {}",base.get(name).copied().or(*default).map(|v|v.to_string()).unwrap_or("unspecified".into()),value));}
                            } else if let Some(value)=default {
                                if ui.button(format!("Expose {} ({unit}; default {value})",name.replace('_'," "))).on_hover_text(description).clicked(){params.insert(name.clone(),*value);}
                            }
                        }
                        ui.collapsing("Simulation conditions & timing",|ui|{
                            optional_value(ui,"Supply override (V)",&mut s.draft.conditions.voltage_v,12.);
                            optional_value(ui,"Temperature override (°C)",&mut s.draft.conditions.temperature_c,25.);
                            for (label,value) in [("Load inertia (kg·m²)",&mut s.draft.conditions.load_inertia),("Signed load torque (N·m)",&mut s.draft.conditions.load_torque),("Command delay assumption (s)",&mut s.draft.conditions.command_delay_s),("Integration step (s)",&mut s.draft.step_s)]{ui.horizontal(|ui|{ui.add(egui::DragValue::new(value).speed(0.00001).max_decimals(8));ui.label(label);});}
                            ui.small("Unchecked supply/temperature: reported trial-range midpoint. Unknown fixture/load is not silently measured. Zero PWM assumes braking.");
                        });
                        ui.collapsing("Separate evaluation limits",|ui|{
                            let mut custom=s.limits.is_some();if ui.checkbox(&mut custom,"Override limits for a new evaluation").changed(){s.limits=custom.then_some(Limits{rmse:3.*hx_archive::ENCODER_QUANTUM_RAD,final_abs_error:5.*hx_archive::ENCODER_QUANTUM_RAD});}
                            if let Some(l)=&mut s.limits {ui.horizontal(|ui|{ui.add(egui::DragValue::new(&mut l.rmse).speed(0.0001));ui.label("RMSE rad");});ui.horizontal(|ui|{ui.add(egui::DragValue::new(&mut l.final_abs_error).speed(0.0001));ui.label("Final |error| rad");});}
                            ui.small("Original reference scores and historical evaluations remain unchanged.");
                        });
                        if before!=s.draft || previous_limits!=s.limits {s.candidate_edited();self.revisions[self.current]+=1;}
                        if let Err(error)=s.draft.validate(){ui.colored_label(Color32::DARK_RED,error);}
                        if s.validation_influenced{ui.colored_label(Color32::DARK_RED,"Held-out results have influenced edits. Fresh trials are needed for independent confirmation.");}
                        ui.collapsing("All draft changes from baseline", |ui| {
                            for change in study::changes(&s.baseline,&s.draft) {ui.label(change);}
                        });
                        if (s.draft.motor["torque_constant"]-s.draft.motor["back_emf_constant"]).abs()>1e-12 {
                            ui.colored_label(Color32::DARK_RED,"Unequal SI motor constants: candidate is not a reciprocal electromagnetic converter.");
                        }
                        ui.separator();
                        ui.strong("Decision & evidence");
                        if let Some(index)=s.view.evaluation {
                            if let Some(e)=s.evaluations.get_mut(index){
                                egui::ComboBox::from_id_salt("experiment-decision").selected_text(&e.decision).show_ui(ui,|ui|{for decision in ["Investigating","Retain baseline","Rejected candidate","Preferred for tested conditions"]{if ui.selectable_value(&mut e.decision,decision.into(),decision).changed(){self.revisions[self.current]+=1;}}});
                                if ui.text_edit_multiline(&mut e.notes).changed(){self.revisions[self.current]+=1;}
                                ui.collapsing("Captured settings / changed parameters", |ui| {
                                    ui.label(format!("Baseline {} · candidate {}", &e.baseline.fingerprint()[..12], &e.candidate.fingerprint()[..12]));
                                    for change in study::changes(&e.baseline,&e.candidate) {ui.label(change);}
                                });
                                if e.cancelled || e.results.iter().any(|r|r.candidate.is_none()) {ui.colored_label(Color32::DARK_RED,"Incomplete evaluation: unscored trials do not count as passing validation.");}
                                ui.small(format!("{} whole trials in this evaluation. Scope is retained per device, direction, drive, duration and conditions.",e.results.len()));
                            }
                        }
                        ui.collapsing("Related system component", |ui| {
                            let mut link=s.view.component_id.clone().unwrap_or_default();
                            if ui.text_edit_singleline(&mut link).changed(){s.view.component_id=(!link.trim().is_empty()).then_some(link);}
                            ui.small("Optional stable component reference. Hardware servo IDs are not automatically mapped to CAD parts.");
                        });
                        ui.label("Study notes / further measurements needed");if ui.text_edit_multiline(&mut s.notes).changed(){self.revisions[self.current]+=1;}
                        ui.small("Preferred means preferred within tested conditions. CAD and defaults are unchanged.");
                    });
                });
            });
            if serde_json::to_string(&s.view).unwrap()!=previous_view {self.revisions[self.current]+=1;}
            if let Some(ids)=run_ids{self.run(ids,ctx);}
        });
        self.open = open;
        if self.confirm_exit {
            egui::Window::new("Retain experiment work before closing?").collapsible(false).show(ctx,|ui|{
                ui.label("Open reviews may contain unsaved candidates, notes or results. Save each review from Open / save / export.");
                if ui.button("Keep reviewing / save").clicked(){self.confirm_exit=false;self.open=true;}
                if ui.button("Discard unsaved experiment work and close").clicked(){if let Some(job)=&self.job{job.cancel.store(true,Ordering::Relaxed);}self.job=None;self.saved_revisions=self.revisions.clone();self.confirm_exit=false;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}
            });
        }
    }
}
fn active_evaluation(s: &Study) -> Option<&Evaluation> {
    s.view.evaluation.and_then(|i| s.evaluations.get(i))
}
fn evaluation_picker(ui: &mut egui::Ui, s: &mut Study) {
    egui::ComboBox::from_id_salt("evaluation-history")
        .selected_text(
            s.view
                .evaluation
                .map(|i| format!("Physical evaluation {}", i + 1))
                .unwrap_or("Retained reference only".into()),
        )
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut s.view.evaluation, None, "Retained reference only");
            for (i, e) in s.evaluations.iter().enumerate() {
                ui.selectable_value(
                    &mut s.view.evaluation,
                    Some(i),
                    format!(
                        "{} · {}{}",
                        i + 1,
                        e.decision,
                        if e.cancelled { " · partial" } else { "" }
                    ),
                );
            }
        });
}
fn optional_value(ui: &mut egui::Ui, label: &str, value: &mut Option<f64>, default: f64) {
    let mut yes = value.is_some();
    if ui.checkbox(&mut yes, label).changed() {
        *value = yes.then_some(default);
    }
    if let Some(v) = value {
        ui.add(egui::DragValue::new(v).speed(0.1));
    }
}
fn filters(ui: &mut egui::Ui, s: &mut Study) {
    ui.horizontal(|ui| {
        ui.label("Servo");
        egui::ComboBox::from_id_salt("exp-device")
            .selected_text(if s.view.device == 0 {
                "All".into()
            } else {
                format!("ID {}", s.view.device)
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut s.view.device, 0, "All");
                for id in 4..=12 {
                    ui.selectable_value(&mut s.view.device, id, format!("ID {id}"));
                }
            });
    });
    ui.horizontal(|ui| {
        for (value, label) in [(0, "±"), (1, "Positive"), (-1, "Reverse")] {
            ui.selectable_value(&mut s.view.direction, value, label);
        }
    });
    ui.horizontal(|ui| {
        ui.label("|Duty| range");
        ui.add(
            egui::DragValue::new(&mut s.view.min_drive)
                .range(0. ..=1.)
                .speed(0.01),
        );
        ui.add(
            egui::DragValue::new(&mut s.view.max_drive)
                .range(0. ..=1.)
                .speed(0.01),
        );
    });
    for (id, value, options) in [
        ("role", &mut s.view.role, vec!["All", "Tuning", "Held out"]),
        (
            "outcome",
            &mut s.view.outcome,
            vec![
                "All",
                "Pass",
                "Fail",
                "Unscored",
                "Regression",
                "Empirical failure",
            ],
        ),
    ] {
        egui::ComboBox::from_id_salt(id)
            .selected_text(value.clone())
            .show_ui(ui, |ui| {
                for label in options {
                    ui.selectable_value(value, label.into(), label);
                }
            });
    }
}
fn filtered(s: &Study) -> Vec<String> {
    s.archive
        .trials
        .iter()
        .filter(|t| {
            let r =
                active_evaluation(s).and_then(|e| e.results.iter().find(|r| r.trial_id == t.id));
            let c = r.and_then(|r| r.candidate.as_ref());
            (s.view.device == 0 || s.view.device == t.device)
                && (s.view.direction == 0 || (t.drive.signum() as i8) == s.view.direction)
                && t.drive.abs() >= s.view.min_drive
                && t.drive.abs() <= s.view.max_drive
                && (s.view.role == "All" || (s.view.role == "Tuning") == (t.split == "train"))
                && match s.view.outcome.as_str() {
                    "Pass" => c.is_some_and(|p| p.metrics.passes),
                    "Fail" => c.is_some_and(|p| !p.metrics.passes),
                    "Unscored" => c.is_none(),
                    "Regression" => r.is_some_and(|r| {
                        r.baseline
                            .as_ref()
                            .zip(r.candidate.as_ref())
                            .is_some_and(|(b, c)| c.metrics.rmse > b.metrics.rmse)
                    }),
                    "Empirical failure" => !t.comparison.passes,
                    _ => true,
                }
        })
        .map(|t| t.id.clone())
        .collect()
}
fn summary(ui: &mut egui::Ui, s: &mut Study) {
    let Some(e) = active_evaluation(s) else {
        return;
    };
    let ids = s
        .archive
        .trials
        .iter()
        .map(|t| t.id.clone())
        .collect::<Vec<_>>();
    let a = e.summary(&ids);
    ui.separator();
    ui.strong(format!(
        "Physical candidate: {} pass · {} fail · {} unscored / {}",
        a.passes, a.failures, a.unscored, a.total
    ));
    ui.label(format!(
        "RMSE: {} improved · {} regressed · {} new failures",
        a.improved, a.regressed, a.new_failures
    ));
    let mut selected = None;
    ui.collapsing("Tuning / validation and device summaries", |ui| {
        for role in ["train", "held"] {
            let ids = s
                .archive
                .trials
                .iter()
                .filter(|t| (t.split == "train") == (role == "train"))
                .map(|t| t.id.clone())
                .collect::<Vec<_>>();
            let score = e.summary(&ids);
            ui.label(format!(
                "{}: {} pass, {} fail, {} unscored; {} improved, {} regressed",
                if role == "train" {
                    "Tuning"
                } else {
                    "Held out"
                },
                score.passes,
                score.failures,
                score.unscored,
                score.improved,
                score.regressed
            ));
        }
        for device in 4..=12 {
            for positive in [true, false] {
                let ids = s
                    .archive
                    .trials
                    .iter()
                    .filter(|t| t.device == device && (t.drive > 0.) == positive)
                    .map(|t| t.id.clone())
                    .collect::<Vec<_>>();
                if ids.is_empty() {
                    continue;
                }
                let score = e.summary(&ids);
                if ui
                    .selectable_label(
                        false,
                        format!(
                            "ID {device} {}: {} pass, {} fail, {} unscored · {} new failures",
                            if positive { "positive" } else { "reverse" },
                            score.passes,
                            score.failures,
                            score.unscored,
                            score.new_failures
                        ),
                    )
                    .clicked()
                {
                    selected = ids
                        .iter()
                        .find(|id| {
                            e.results.iter().any(|r| {
                                &r.trial_id == *id
                                    && r.candidate.as_ref().is_some_and(|p| !p.metrics.passes)
                            })
                        })
                        .cloned()
                        .or(ids.first().cloned());
                }
            }
        }
    });
    ui.collapsing(
        "Results by motor / command / direction — click to inspect",
        |ui| {
            for t in &s.archive.trials {
                if let Some(r) = e.results.iter().find(|r| r.trial_id == t.id) {
                    let metrics = r
                        .candidate
                        .as_ref()
                        .map(|p| {
                            format!(
                                "{} RMSE {:.5}, max {:.5}, final {:.5} rad",
                                if p.metrics.passes { "PASS" } else { "FAIL" },
                                p.metrics.rmse,
                                p.metrics.maximum_abs_error,
                                p.metrics.final_error.abs()
                            )
                        })
                        .unwrap_or("UNSCORED".into());
                    if ui
                        .selectable_label(
                            false,
                            format!(
                                "ID {} {:+.1}% {} · {metrics}",
                                t.device,
                                t.drive * 100.,
                                t.split
                            ),
                        )
                        .clicked()
                    {
                        selected = Some(t.id.clone());
                    }
                }
            }
        },
    );
    if let Some(id) = selected {
        s.view.trial_id = Some(id);
    }
}
fn parameter_info() -> Vec<(String, String, String, String, Option<f64>)> {
    let registry = sim_runtime::registry();
    let mut out = vec![];
    for (group, kind, names) in [
        (
            "motor",
            "robot.motor_unit",
            vec![
                "resistance",
                "inductance",
                "torque_constant",
                "back_emf_constant",
                "no_load_current",
                "loss_speed_scale",
                "rotor_inertia",
                "ratio",
                "efficiency",
                "gear_friction",
                "gear_inertia",
                "gear_stiffness",
                "gear_damping",
                "backlash",
            ],
        ),
        (
            "bridge",
            "robot.h_bridge",
            vec!["on_resistance", "current_limit"],
        ),
    ] {
        if let Ok(d) = registry.get(&kind.into()) {
            for p in d.parameters.as_ref().unwrap() {
                if names.contains(&p.name.as_str()) {
                    let description = match p.name.as_str() {
                        "loss_speed_scale" => {
                            "Rotor-speed width of smooth Coulomb loss. Smaller values reduce low-speed creep; this is a regularization hypothesis, not true static friction."
                        }
                        "resistance" => {
                            "Winding resistance controls current and electrical losses."
                        }
                        "inductance" => "Winding inductance controls current transients.",
                        "torque_constant" => {
                            "Torque per winding current. For reciprocal SI models, change back EMF constant consistently."
                        }
                        "back_emf_constant" => {
                            "Voltage generated per rotor speed. Reciprocal SI motor constants should match."
                        }
                        "rotor_inertia" | "gear_inertia" => {
                            "Stored rotational inertia affects acceleration and release."
                        }
                        "ratio" => {
                            "Assumed internal transmission ratio; not identified by these pulses."
                        }
                        "efficiency" => "Transmission loss scales output torque.",
                        "gear_friction" | "no_load_current" => {
                            "Provisional mechanical loss model; unloaded pulses cannot uniquely identify its physical cause."
                        }
                        "gear_stiffness" | "gear_damping" | "backlash" => {
                            "Compliance, damping and clearance govern output coupling."
                        }
                        "current_limit" => "Averaged driver current foldback limit.",
                        _ => "Driver conduction resistance affects delivered motor voltage.",
                    };
                    out.push((
                        group.into(),
                        p.name.clone(),
                        p.unit.clone(),
                        description.into(),
                        p.default,
                    ));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn panel() -> ExperimentsPanel {
        let mut p = ExperimentsPanel::default();
        let archive = hx_archive::load(
            &repository().join("examples/actuators/hx30hm/pwm-identification"),
            &repository(),
        )
        .unwrap();
        let mut s = Study::new(archive).unwrap();
        s.view.trial_id = Some(s.archive.trials[0].id.clone());
        p.studies.push(s);
        p.revisions.push(0);
        p.saved_revisions.push(0);
        p.open = true;
        p
    }
    #[test]
    fn filters_keep_failures_no_motion_and_whole_trial_roles() {
        let mut p = panel();
        let s = &mut p.studies[0];
        assert_eq!(filtered(s).len(), 63);
        s.view.role = "Held out".into();
        assert_eq!(filtered(s).len(), 36);
        s.view.outcome = "Empirical failure".into();
        assert_eq!(filtered(s).len(), 3);
        s.view.device = 8;
        assert_eq!(filtered(s).len(), 2);
        s.view.device = 0;
        s.view.role = "All".into();
        s.view.outcome = "Unscored".into();
        s.view.direction = -1;
        assert!(
            filtered(s).iter().all(|id| s
                .archive
                .trials
                .iter()
                .find(|t| &t.id == id)
                .unwrap()
                .drive
                < 0.)
        );
    }
    #[test]
    fn worker_host_renders_retains_candidate_and_cancels_without_losing_history() {
        let mut p = panel();
        let ctx = egui::Context::default();
        let id = p.studies[0]
            .archive
            .trials
            .iter()
            .find(|t| t.split != "train")
            .unwrap()
            .id
            .clone();
        p.run(vec![id.clone()], &ctx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while p.loading() {
            p.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(p.studies[0].evaluations.len(), 1);
        assert!(p.studies[0].validation_seen);
        assert!(p.has_unsaved());
        p.studies[0].view.trial_id = Some(id);
        // An adopted optional coefficient may be absent from the old baseline.
        // Rendering must show its registry default without indexing a missing key.
        assert!(!p.studies[0].baseline.motor.contains_key("loss_speed_scale"));
        p.studies[0]
            .draft
            .motor
            .insert("loss_speed_scale".into(), 0.05);
        for _ in 0..4 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440., 900.),
                    )),
                    ..Default::default()
                },
                |ui| p.show(ui.ctx()),
            );
            assert!(!output.shapes.is_empty());
            output.textures_delta.clear();
        }
        p.studies[0]
            .draft
            .motor
            .insert("gear_friction".into(), 0.05);
        p.studies[0].candidate_edited();
        assert!(p.studies[0].validation_influenced);
        let ids = p.studies[0]
            .archive
            .trials
            .iter()
            .map(|t| t.id.clone())
            .collect();
        p.run(ids, &ctx);
        p.job
            .as_ref()
            .unwrap()
            .cancel
            .store(true, Ordering::Relaxed);
        while p.loading() {
            p.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(p.studies[0].evaluations.len(), 2);
        assert!(p.studies[0].evaluations[1].cancelled);
        assert!(p.studies[0].evaluations[0].results[0].candidate.is_some());
        p.studies[0].view.evaluation = Some(0);
        assert!(active_evaluation(&p.studies[0]).unwrap().stale(
            &p.studies[0].baseline,
            &p.studies[0].draft,
            &p.studies[0].limits
        ));
    }
}
