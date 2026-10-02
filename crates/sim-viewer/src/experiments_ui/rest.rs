//! Automation for existing experiment workers; original observations are read-only.
use super::*;
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Command {
    State,
    Study,
    Open {
        path: String,
    },
    Select {
        index: usize,
    },
    Configure {
        fields: Value,
    },
    Evaluate {
        ids: Option<Vec<String>>,
    },
    Refine {
        action: refinement::Action,
    },
    Save {
        path: String,
        #[serde(default)]
        html: bool,
    },
    Cancel,
    Review {
        index: usize,
        decision: String,
        notes: String,
    },
    Plot {
        state: plots::PlotState,
    },
    Panel {
        open: bool,
        refinement: bool,
    },
}
pub(crate) fn capabilities() -> Value {
    json!({"operations":["state","study","open","select","configure","evaluate","refine","save","cancel","review","plot","panel"],
        "configure_fields":["draft","view","limits","notes","experiment","coordinates","scenarios","capture_contexts","recording_assignments","fpga_design_drafts"],
        "refine_actions":["simulate","sensitivity","fit","fit_recordings","fit_combined","robustness","import","review_fpga","fit_fpga","design_fpga","export_fpga_plan","review_fpga_fit","predict","compare_electrical","propose","accept"],
        "refine_encoding":"serde externally tagged snake_case Action; unit: simulate; tuple: {review_fpga:[0,mode]}; struct: {fit_combined:{selected:[],additional_study:path}}",
        "completion":"Open, evaluate, refine and save start an existing background task. Poll experiments state until busy=false, then inspect error and retained study. Cancel interrupts that task. These commands do not claim the task succeeded at acceptance.",
        "configure":"Each provided field replaces that editable field; all others and all retained evidence are preserved. Shared Study validation runs before commit.",
        "plot":{"cursor":null,"zoom":1.0,"center":0.5,"measured":true,"empirical":true,"baseline":true,"candidate":true}})
}
impl ExperimentsPanel {
    pub(crate) fn api_poll(&mut self, ctx: &egui::Context) {
        self.poll(ctx);
    }
    fn api_state(&self) -> Value {
        json!({"busy":self.loading(),"current":self.current,"studies":self.studies.iter().map(|s|&s.archive.label).collect::<Vec<_>>(),"message":self.message,"error":self.api_error,"revisions":self.revisions,"saved_revisions":self.saved_revisions,"open":self.open,"refinement":self.refinement_tab,"plot":self.plot,"job":self.job.as_ref().map(|j|json!({"label":j.label,"completed":j.progress.load(Ordering::Relaxed),"total":j.total,"cancel_requested":j.cancel.load(Ordering::Relaxed)}))})
    }
    pub(crate) fn api_command(
        &mut self,
        command: Command,
        _continuation: &mut Value,
        ctx: &egui::Context,
    ) -> sim_api::Outcome {
        let rejected_input=match &command {
            Command::Configure{fields}=>Some(json!({"operation":"configure","fields":fields})),
            Command::Refine{action}=>action.operation().map(|op|json!({"operation":"refine","action":op})),
            _=>None,
        };
        let original_index=self.current;
        let result = (|| -> sim_api::Result {
            match command {
                Command::State => return Ok(self.api_state()),
                Command::Study => {
                    let s=self.studies.get_mut(self.current).ok_or("no study loaded")?;
                    let seen=s.validation_seen;
                    let ids=s.archive.trials.iter().map(|t|t.id.clone()).collect::<Vec<_>>();
                    study::commands::expose(s,&ids);
                    study::refinement::expose_review(s);
                    if !seen && s.validation_seen {self.revisions[self.current]+=1;}
                    return Ok(json!(s));
                }
                Command::Cancel => {
                    if let Some(job) = &self.job {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                }
                Command::Panel { open, refinement } => {
                    self.open = open;
                    self.refinement_tab = refinement;
                }
                Command::Plot { state } => {
                    state.validate()?;
                    self.plot = state;
                }
                command => {
                    if self.loading() {
                        return Err("experiment worker is busy; inspect or cancel it first".into());
                    }
                    match command {
                        Command::Open { path } => self.launch(Some(path), Some(ctx)),
                        Command::Select { index } => {
                            if index >= self.studies.len() {
                                return Err("unknown study index".into());
                            }
                            self.current = index;
                            self.plot = Default::default();
                        }
                        Command::Configure { fields } => {
                            let original =
                                self.studies.get(self.current).ok_or("no study loaded")?;
                            let mut value = json!(original);
                            for (key, field) in
                                fields.as_object().ok_or("fields must be an object")?
                            {
                                match key.as_str() {
                                    "draft" | "view" | "limits" | "notes" => {
                                        value[key] = field.clone()
                                    }
                                    "experiment" => {
                                        let _:sim_runtime::controller_refinement::control::Experiment=serde_json::from_value(field.clone()).map_err(|e|format!("configure.experiment: {e}"))?;
                                        value["refinement"][key]=field.clone();
                                    }
                                    "coordinates" => {
                                        let _:Vec<sim_runtime::controller_refinement::calibration::Coordinate>=serde_json::from_value(field.clone()).map_err(|e|format!("configure.coordinates: {e}"))?;
                                        value["refinement"][key]=field.clone();
                                    }
                                    "scenarios" => {
                                        let _:Vec<sim_runtime::controller_refinement::calibration::Variant>=serde_json::from_value(field.clone()).map_err(|e|format!("configure.scenarios: {e}"))?;
                                        value["refinement"][key]=field.clone();
                                    }
                                    "capture_contexts"
                                    | "recording_assignments"
                                    | "fpga_design_drafts" => {
                                        value["refinement"][key] = field.clone()
                                    }
                                    _ => {
                                        return Err(format!(
                                            "{key} is not an editable study field"
                                        ));
                                    }
                                }
                            }
                            let mut next: Study =
                                serde_json::from_value(value).map_err(|e| e.to_string())?;
                            let mut shared=original.clone();
                            for command in [study::commands::Command::SetCandidate(next.draft.clone()), study::commands::Command::SetLimits(next.limits), study::commands::Command::SetView(next.view.clone()), study::commands::Command::SetNotes(next.notes.clone())] { study::commands::apply(&mut shared,command)?; }
                            use study::refinement::{self as refinement_shared,Command as RefinementCommand};
                            refinement_shared::apply(&mut shared,RefinementCommand::SetExperiment(next.refinement.experiment.clone()))?;
                            refinement_shared::apply(&mut shared,RefinementCommand::SetCoordinates(next.refinement.coordinates.clone()))?;
                            refinement_shared::apply(&mut shared,RefinementCommand::SetScenarios(next.refinement.scenarios.clone()))?;
                            // Deferred compatibility payloads retain their existing validation path.
                            next.refinement.experiment=shared.refinement.experiment.clone();
                            next.refinement.coordinates=shared.refinement.coordinates.clone();
                            next.refinement.scenarios=shared.refinement.scenarios.clone();
                            shared.refinement=next.refinement;
                            next=shared;
                            next.validate()?;
                            self.studies[self.current] = next;
                            self.revisions[self.current] += 1;
                        }
                        Command::Review {
                            index,
                            decision,
                            notes,
                        } => {
                            let s = self.studies.get_mut(self.current).ok_or("no study loaded")?;
                            study::commands::apply(s, study::commands::Command::SetDecision{evaluation:index,decision,notes})?;
                            self.revisions[self.current] += 1;
                        }
                        Command::Evaluate { ids } => {
                            let s = self.studies.get(self.current).ok_or("no study loaded")?;
                            let ids = ids.unwrap_or_else(|| filtered(s));
                            if ids.is_empty()
                                || ids
                                    .iter()
                                    .any(|id| !s.archive.trials.iter().any(|t| &t.id == id))
                            {
                                return Err(
                                    "provide existing trial IDs, or nonempty current filters"
                                        .into(),
                                );
                            }
                            self.run(ids, ctx);
                        }
                        Command::Save { path, html } => {
                            let index = self.current;
                            let s = self.studies.get(index).ok_or("no study loaded")?.clone();
                            let revision = self.revisions[index];
                            let path = PathBuf::from(path);
                            self.start_job(
                                "Saving captured evidence",
                                0,
                                Some(ctx),
                                move |_, _| {
                                    let r = if html {
                                        s.export_html_new(&path)
                                    } else {
                                        s.save_new(&path)
                                    };
                                    ResultMessage::Saved(
                                        index,
                                        revision,
                                        html,
                                        r.map(|_| format!("Saved {}", path.display())),
                                    )
                                },
                            );
                        }
                        Command::Refine { action } => {
                            let index = self.current;
                            let s = self.studies.get_mut(index).ok_or("no study loaded")?;
                            if let Some(operation)=action.operation() {
                                if let Err(error)=study::refinement::prepare(s,operation) {
                                    s.refinement.failures.push(error.clone());
                                    self.revisions[index]+=1;
                                    return Err(error);
                                }
                                self.revisions[index]+=1;
                            }
                            // Deferred fitting retains its existing exposure accounting.
                            if matches!(
                                &action,
                                refinement::Action::FitRecordings
                                    | refinement::Action::FitCombined { .. }
                            ) {
                                s.validation_seen = true;
                                self.revisions[index] += 1;
                            }
                            if let refinement::Action::ReviewFpga(i, _)
                            | refinement::Action::ReviewFpgaFit(i, _, _) = &action
                            {
                                if *i >= s.refinement.fpga_recordings.len() {
                                    return Err("unknown FPGA recording".into());
                                }
                            }
                            if let refinement::Action::Predict(i, _) = &action {
                                if *i >= s.refinement.recordings.len() {
                                    return Err("unknown recording".into());
                                }
                            }
                            if let refinement::Action::Accept(i, _, _, _) = &action {
                                if *i >= s.refinement.cad_proposals.len() {
                                    return Err("unknown CAD proposal".into());
                                }
                            }
                            let s = s.clone();
                            let label = action.label().to_string();
                            let job_label = label.clone();
                            let total = action.total(&s);
                            self.start_job(
                                &job_label,
                                total,
                                Some(ctx),
                                move |cancel, progress| {
                                    ResultMessage::Refined(
                                        index,
                                        label,
                                        action.run(s, &cancel, |n, _| {
                                            progress.store(n, Ordering::Relaxed);
                                        }),
                                    )
                                },
                            );
                        }
                        _ => unreachable!(),
                    }
                }
            }
            Ok(self.api_state())
        })();
        if let (Err(error),Some(input))=(&result,rejected_input) {
            if let Some(study)=self.studies.get_mut(original_index) {
                study.refinement.failures.push(format!("Legacy rejected offline input {input}: {error}"));
                self.revisions[original_index]+=1;
            }
        }
        sim_api::Outcome::Done(result)
    }
}

impl ExperimentsPanel {
    pub(crate) fn capture_graphs(&self) -> Result<(Vec<sim_render::graphs::Panel>, Value), String> {
        use sim_render::graphs::{Panel, Series};
        let s = self.studies.get(self.current).ok_or("no study loaded")?;
        let id = s
            .view
            .trial_id
            .as_ref()
            .ok_or("select an experiment trial")?;
        let t = s
            .archive
            .trials
            .iter()
            .find(|t| &t.id == id)
            .ok_or("unknown trial")?;
        let points = |trace: &sim_runtime::experiment_comparison::Trace| {
            trace
                .samples
                .iter()
                .map(|s| Some([s.time_s, s.value]))
                .collect()
        };
        let mut series = vec![
            Series {
                label: "Measured".into(),
                color: [20, 115, 156],
                points: points(&t.measured),
            },
            Series {
                label: "Reference".into(),
                color: [175, 127, 38],
                points: points(&t.predicted),
            },
        ];
        if let Some(e) = active_evaluation(s) {
            if let Some(r) = e.results.iter().find(|r| &r.trial_id == id) {
                if let Some(p) = &r.baseline {
                    series.push(Series {
                        label: "Baseline".into(),
                        color: [123, 78, 181],
                        points: points(&p.trace),
                    });
                }
                if let Some(p) = &r.candidate {
                    series.push(Series {
                        label: "Candidate".into(),
                        color: [22, 138, 82],
                        points: points(&p.trace),
                    });
                }
            }
        }
        Ok((
            vec![Panel {
                title: format!("Trial {id} · device {}", t.device),
                unit: t.measured.unit.clone(),
                series,
            }],
            json!({"trial_id":id,"observation_blake3":s.archive.observation_blake3,"evaluation":s.view.evaluation,"candidate_fingerprint":s.draft.fingerprint()}),
        ))
    }
}
