use super::refinement::{Action, chart, voltage_chart};
use eframe::egui::{self, Color32};
use sim_runtime::{controller_refinement::fpga_review::Mode, experiment_study::Study};

#[derive(Default)]
pub struct State {
    path: String,
    selected: usize,
    motor: u8,
    design: super::fpga_design_ui::State,
    response: super::motor_response_ui::State,
}
impl State {
    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Study, busy: bool) -> Option<Action> {
        let mut action = None;
        ui.heading("FPGA controller experiments");
        ui.label("Review actual bench measurements and rerun the captured integer controller against the selected model. Imports never command hardware.");
        ui.horizontal(|ui| {
            ui.label("Recording JSON");
            ui.text_edit_singleline(&mut self.path);
            if ui
                .add_enabled(
                    !busy && !self.path.trim().is_empty(),
                    egui::Button::new("Import FPGA recording"),
                )
                .clicked()
            {
                action = Some(Action::Import(self.path.trim().into()));
            }
        });
        egui::ComboBox::from_id_salt("fpga-recording")
            .width(650.)
            .selected_text(
                s.refinement
                    .fpga_recordings
                    .get(self.selected)
                    .map(|r| r.plan.name.as_str())
                    .unwrap_or("No FPGA recordings imported"),
            )
            .show_ui(ui, |ui| {
                for (i, r) in s.refinement.fpga_recordings.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.selected,
                        i,
                        format!(
                            "{} · {} · {} motors",
                            r.plan.name,
                            r.plan.role,
                            r.plan.ids.len()
                        ),
                    );
                }
            });
        if let Some(r) = s.refinement.fpga_recordings.get(self.selected) {
            let hash = r.fingerprint();
            if let Some(next)=self.design.show(ui,r,busy,&s.refinement.fpga_design_runs,&mut s.refinement.fpga_design_drafts) {action=Some(next);}

            ui.label(format!(
                "{} frames · {:.0} ms requested cadence · frozen role: {} · stop verified: {}",
                r.frames.len(),
                r.plan.period_s * 1000.,
                r.plan.role,
                r.stop_verified
            ));
            ui.small(format!("Captured controller: Kp {:.3}, Kd {:.3}, velocity feedforward {:.3}; duty ceiling {:.1}%",r.plan.gains.kp_q8 as f64/256.,r.plan.gains.kd_q8 as f64/256.,r.plan.gains.kv_q8 as f64/256.,r.plan.gains.limit as f64/10.));
            ui.small("Feedback is the internal encoder. Host observation windows and command receipts determine simulated timing. These are unloaded tests; shared battery behavior and loaded joint accuracy remain unvalidated.");
            if let Err(error) = r.validate() {
                ui.colored_label(
                    Color32::DARK_RED,
                    format!("Retained incomplete / unscored acquisition: {error}"),
                );
                if let Some(error) = &r.failure {
                    ui.label(error);
                }
            } else {
                let scores = r.scores();
                ui.strong(format!(
                    "Measured controller tracking · frozen limits: {:.3}° RMS / {:.3}° peak",
                    r.plan.rms_limit_counts * 360. / 4096.,
                    r.plan.peak_limit_counts * 360. / 4096.
                ));
                egui::Grid::new("fpga-tracking-table")
                    .striped(true)
                    .show(ui, |ui| {
                        for label in [
                            "Motor",
                            "RMS °",
                            "Peak °",
                            "Saturation",
                            "Supply V",
                            "Tracking",
                        ] {
                            ui.strong(label);
                        }
                        ui.end_row();
                        for id in &r.plan.ids {
                            let row = &scores["motors"][id.to_string()];
                            ui.label(id.to_string());
                            ui.label(format!("{:.3}", row["rms_degrees"].as_f64().unwrap()));
                            ui.label(format!("{:.3}", row["peak_degrees"].as_f64().unwrap()));
                            ui.label(format!(
                                "{:.1}%",
                                row["saturated_fraction"].as_f64().unwrap() * 100.
                            ));
                            ui.label(format!(
                                "{:.1}–{:.1}",
                                row["voltage_min_v"].as_f64().unwrap(),
                                row["voltage_max_v"].as_f64().unwrap()
                            ));
                            pass(ui, row["tracking_pass"].as_bool().unwrap());
                            ui.end_row();
                        }
                    });
                ui.horizontal(|ui| {
                    for mode in [Mode::Replay, Mode::ClosedLoop] {
                        if ui
                            .add_enabled(
                                !busy,
                                egui::Button::new(format!("Predict: {}", mode.label())),
                            )
                            .clicked()
                        {
                            action = Some(Action::ReviewFpga(self.selected, mode));
                            s.validation_seen |= r.plan.role == "validation";
                        }
                    }
                });
                ui.small("Replay freezes measured PWM. Closed loop computes PWM from simulated encoder feedback. Neither mode changes the captured controller or physical measurements.");
                if !r.plan.ids.contains(&self.motor) {
                    self.motor = r.plan.ids[0];
                }
                ui.horizontal(|ui| {
                    ui.label("Plot motor");
                    for id in &r.plan.ids {
                        ui.selectable_value(&mut self.motor, *id, id.to_string());
                    }
                });
                let axis = (self.motor - 4) as usize;
                self.response.show(ui,r,self.motor,&s.refinement.fpga_reviews);
                let observations: Vec<_> = r
                    .frames
                    .iter()
                    .map(|f| f.observations.iter().find(|o| o.id == self.motor).unwrap())
                    .collect();
                let measured: Vec<_> = observations
                    .iter()
                    .map(|o| {
                        [
                            (o.request_s + o.completion_s) * 0.5,
                            (o.telemetry.position_raw as f64 - r.home[axis] as f64) * 360. / 4096.,
                        ]
                    })
                    .collect();
                let targets: Vec<_> = r
                    .frames
                    .iter()
                    .flat_map(|f| {
                        let old =
                            r.plan.targets[f.tick.saturating_sub(1)][axis] as f64 * 360. / 4096.;
                        let new = r.plan.targets[f.tick][axis] as f64 * 360. / 4096.;
                        [[f.command_receipt_s, old], [f.command_receipt_s, new]]
                    })
                    .collect();
                chart(
                    ui,
                    "Position relative to start (degrees)",
                    &[
                        ("Measured", Color32::BLUE, measured.clone()),
                        ("Commanded target", Color32::GRAY, targets),
                    ],
                );
                ui.collapsing("PWM and measured electrical channels",|ui| {
                    chart(ui,"Written PWM duty (%)",&[("Measured write/readback",Color32::BLUE,r.frames.iter().flat_map(|f| {
                        let old=if f.tick==0 {0.}else{r.frames[f.tick-1].pwm_readback[axis] as f64/10.};
                        [[f.command_receipt_s,old],[f.command_receipt_s,f.pwm_readback[axis] as f64/10.]]
                    }).collect())]);
                    voltage_chart(ui,"Measured servo voltage (V)",&[("Servo register",Color32::BLUE,observations.iter().map(|o|[(o.request_s+o.completion_s)*0.5,o.telemetry.voltage_v]).collect())]);
                    chart(ui,"Current register (uncalibrated counts)",&[("Raw current",Color32::BLUE,observations.iter().map(|o|[(o.request_s+o.completion_s)*0.5,o.telemetry.current_raw as f64]).collect())]);
                    ui.small("Raw current counts are not amps. Measured supply current, watts and energy accuracy cannot be established from this channel.");
                });
                for (index, review) in s
                    .refinement
                    .fpga_reviews
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, p)| p.recording_hash == hash)
                {
                    ui.push_id(index,|ui| {
                        ui.separator();ui.strong(review.mode.label());
                        if review.models.values().any(|model|model != &s.draft) {ui.label("This comparison preserves an earlier model revision.");}
                        if review.cancelled {ui.colored_label(Color32::DARK_RED,"Cancelled; partial results retained.");}
                        for (id,error) in &review.failures {ui.colored_label(Color32::DARK_RED,format!("ID {id}: unscored · {error}"));}
                        egui::Grid::new("prediction-table").striped(true).show(ui,|ui| {
                            for label in ["Motor", "Model error RMS °", "Peak °", "Prediction"] {ui.strong(label);}ui.end_row();
                            for p in &review.predictions {
                                ui.label(p.id.to_string());ui.label(format!("{:.3}",p.rms_prediction_degrees));ui.label(format!("{:.3}",p.peak_prediction_counts*360./4096.));pass(ui,p.prediction_pass);ui.end_row();
                            }
                        });
                        if let Some(p)=review.predictions.iter().find(|p|p.id==self.motor) {
                            chart(ui,"Measured / predicted position (degrees)",&[("Measured",Color32::BLUE,measured.clone()),("Predicted",Color32::DARK_GREEN,p.samples_time_encoder_duty_angle.iter().map(|v|[v[0],(v[1]-r.home[axis] as f64)*360./4096.]).collect())]);
                            ui.small(format!("Model RMS gate {:.3}°. This measures simulation error, separate from target tracking.",p.prediction_limit_counts*360./4096.));
                            ui.collapsing("Prediction assumptions and electrical simulation",|ui| {
                                ui.label(&p.assumptions);
                                if !p.electrical.is_empty() {
                                    chart(ui,"Simulated current (A), not measured validation",&[("Supply",Color32::BLUE,p.electrical.iter().map(|v|[v.time_s,v.supply_current_a]).collect()),("Winding",Color32::DARK_GREEN,p.electrical.iter().map(|v|[v.time_s,v.winding_current_a]).collect())]);
                                    chart(ui,"Simulated power (W), not measured validation",&[("Supply",Color32::BLUE,p.electrical.iter().map(|v|[v.time_s,v.supply_power_w]).collect()),("Winding",Color32::DARK_GREEN,p.electrical.iter().map(|v|[v.time_s,v.winding_power_w]).collect())]);
                                }
                            });
                        }
                    });
                }
            }
        }
        ui.separator();
        ui.strong("Refine the motor model using frozen FPGA recordings");
        let train = s
            .refinement
            .fpga_recordings
            .iter()
            .filter(|r| r.completed && r.plan.role == "training")
            .count();
        let held = s
            .refinement
            .fpga_recordings
            .iter()
            .filter(|r| r.completed && r.plan.role == "validation")
            .count();
        ui.label(format!("{train} training runs / {held} validation runs. Timing and incomplete acquisitions are excluded."));
        ui.small("Set shared or per-device parameter bounds in Sensitivity & fitting. Fitting replays measured PWM and preserves original roles. Inspected validation data require fresh confirmation; fitting does not accept a model into CAD.");
        if ui
            .add_enabled(
                !busy && train > 0 && held > 0 && !s.refinement.coordinates.is_empty(),
                egui::Button::new("Fit FPGA response · 40 evaluations"),
            )
            .clicked()
        {
            action = Some(Action::FitFpga);
            s.validation_seen = true;
        }
        for (i, fit) in s.refinement.fpga_fits.iter().enumerate().rev() {
            ui.collapsing(format!("Retained FPGA fit {}", i + 1), |ui| {
                if let Some(error) = &fit.attempt.failure {
                    ui.colored_label(Color32::DARK_RED, error);
                }
                if fit.attempt.cancelled {
                    ui.label("Cancelled; evaluations retained.");
                }
                if let Some(outcome) = &fit.attempt.outcome {
                    ui.label(&outcome.status);
                    ui.small("Compare this fitted family with its per-device deviations. This does not accept the model or change the draft.");
                    for mode in [Mode::Replay,Mode::ClosedLoop] {
                        if ui.add_enabled(!busy && s.refinement.fpga_recordings.get(self.selected).is_some_and(|r|r.completed),egui::Button::new(format!("Compare fitted family: {}",mode.label()))).clicked() {
                            action=Some(Action::ReviewFpgaFit(self.selected,i,mode));
                        }
                    }

                    for score in &outcome.scores {
                        ui.label(format!(
                            "ID {} · {} · RMS {} → {} rad · {}",
                            score.device,
                            score.split,
                            score
                                .baseline
                                .as_ref()
                                .map(|s| format!("{:.6}", s.rmse))
                                .unwrap_or("unscored".into()),
                            score
                                .candidate
                                .as_ref()
                                .map(|s| format!("{:.6}", s.rmse))
                                .unwrap_or("unscored".into()),
                            score
                                .candidate
                                .as_ref()
                                .map(|s| if s.passes { "PASS" } else { "FAIL" })
                                .unwrap_or("UNSCORED")
                        ));
                    }
                }
                ui.collapsing("Frozen request, candidate and objective history", |ui| {
                    ui.monospace(serde_json::to_string_pretty(fit).unwrap());
                });
            });
        }
        action
    }
}
fn pass(ui: &mut egui::Ui, passes: bool) {
    ui.colored_label(
        if passes {
            Color32::DARK_GREEN
        } else {
            Color32::DARK_RED
        },
        if passes { "PASS" } else { "FAIL" },
    );
}
