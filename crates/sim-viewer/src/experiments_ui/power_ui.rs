use eframe::egui::{self, Color32};
use sim_runtime::{controller_refinement::power, experiment_study::Study};
#[derive(Default)]
pub struct State {
    editor: String,
    error: Option<String>,
    measurement_path: String,
    prediction: usize,
    recording: usize,
    selected_run: Option<usize>,
}
impl State {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut Study,
        busy: bool,
    ) -> (bool, Option<super::refinement::Action>) {
        let previous = s.draft.clone();
        let previous_control = s.refinement.experiment.electrical.clone();
        let mut run = None;
        ui.strong("Supply, battery and electrical validation");
        ui.label("Test voltage sag and energy demand using the shared electrical circuit. Declare which electrical channels the controller can observe; physical traces also show what happens between controller ticks. Battery parameters and discharge behavior need measured evidence.");
        ui.collapsing("Configure source, sensors and electrical limits", |ui| {
        ui.horizontal(|ui|{
            for (label,battery) in [("Add regulated supply",false),("Add example battery",true)]{if ui.add_enabled(!busy,egui::Button::new(label)).clicked(){
                s.draft.power=Some(power::Setup{
                    source_component:if battery{"robot.battery"}else{"electrical.voltage_source"}.into(),
                    source_parameters:if battery{[("nominal_voltage".into(),11.1),("internal_resistance".into(),0.1),("capacity_ah".into(),1.),("initial_soc".into(),1.)].into_iter().collect()}else{[("voltage".into(),s.refinement.experiment.voltage_v)].into_iter().collect()},
                    auxiliary_current_a:0.,evidence:if battery{"Illustrative battery parameters and built-in discharge curve; not a measured pack. Auxiliary electronics consumption explicitly neglected."}else{"Constant source at captured initial voltage; no measured voltage variation. Auxiliary electronics consumption explicitly neglected."}.into(),limits:Default::default(),
                });s.draft.conditions.voltage_v=None;
            }}
            if ui.add_enabled(!busy&&s.draft.power.is_some(),egui::Button::new("Remove electrical scenario")).clicked(){s.draft.power=None;}
        });
        if let Some(p) = &mut s.draft.power {
            ui.label(format!("Source component: {}", p.source_component));
            let registry = sim_runtime::registry();
            let descriptor = registry.get(&p.source_component.as_str().into()).ok();
            for (name, value) in &mut p.source_parameters {
                let unit = descriptor
                    .as_ref()
                    .and_then(|d| d.parameters.as_ref())
                    .and_then(|params| params.iter().find(|p| &p.name == name))
                    .map(|p| p.unit.as_str())
                    .unwrap_or("");
                let speed = (value.abs() * 0.01).max(0.00001);
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(value).speed(speed).max_decimals(6));
                    ui.label(format!("{} ({unit})", name.replace('_', " ")));
                });
            }
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut p.auxiliary_current_a).speed(0.001));
                ui.label("Auxiliary / electronics load (A)");
            });
            ui.label("Source evidence and assumptions");
            ui.text_edit_multiline(&mut p.evidence);
            ui.collapsing("Declared electrical limits", |ui| {
                super::optional_value(
                    ui,
                    "Minimum bus voltage (V)",
                    &mut p.limits.minimum_voltage_v,
                    9.,
                );
                super::optional_value(
                    ui,
                    "Maximum discharge current (A)",
                    &mut p.limits.maximum_discharge_current_a,
                    1.,
                );
                super::optional_value(
                    ui,
                    "Maximum charging current (A)",
                    &mut p.limits.maximum_charge_current_a,
                    0.,
                );
                super::optional_value(
                    ui,
                    "Maximum winding current magnitude (A)",
                    &mut p.limits.maximum_winding_current_a,
                    1.,
                );
                super::optional_value(
                    ui,
                    "Maximum drawn power (W)",
                    &mut p.limits.maximum_draw_power_w,
                    10.,
                );
                super::optional_value(
                    ui,
                    "Maximum returned power (W)",
                    &mut p.limits.maximum_return_power_w,
                    0.,
                );
            });
            if let Err(e) = p.validate() {
                ui.colored_label(Color32::DARK_RED, e);
            }
            if ui.button("Edit full source definition").clicked() {
                self.editor = serde_json::to_string_pretty(p).unwrap();
            }
        }
        if !self.editor.is_empty() {
            ui.add(
                egui::TextEdit::multiline(&mut self.editor)
                    .code_editor()
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Apply validated electrical source").clicked() {
                match serde_json::from_str::<power::Setup>(&self.editor)
                    .map_err(|e| e.to_string())
                    .and_then(|p| {
                        p.validate()?;
                        Ok(p)
                    }) {
                    Ok(p) => {
                        s.draft.power = Some(p);
                        s.draft.conditions.voltage_v = None;
                        self.editor.clear();
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
        ui.separator();
        ui.strong("Controller electrical feedback");
        let mut enabled = s.refinement.experiment.electrical.is_some();
        if ui
            .checkbox(&mut enabled, "Observe supply voltage")
            .changed()
        {
            s.refinement.experiment.electrical = enabled.then(|| power::Controller {
                sensing:power::Sensing {voltage_quantum_v:0.1,supply_current_quantum_a:None,winding_current_quantum_a:None,evidence:"Servo voltage register, 0.1 V resolution. Sample age is unknown; no calibrated current channel connected.".into()},
                nominal_voltage_for_compensation_v:None,limits:Default::default()
            });
        }
        if let Some(c) = &mut s.refinement.experiment.electrical {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut c.sensing.voltage_quantum_v).speed(0.01));
                ui.label("Voltage resolution (V)");
            });
            super::optional_value(
                ui,
                "Supply current sensor resolution (A)",
                &mut c.sensing.supply_current_quantum_a,
                0.01,
            );
            super::optional_value(
                ui,
                "Winding current sensor resolution (A)",
                &mut c.sensing.winding_current_quantum_a,
                0.01,
            );
            ui.small("Current channels are simulated sensor hypotheses until a calibrated hardware adapter is available. Supply and winding current need distinct measurement locations.");
            ui.text_edit_multiline(&mut c.sensing.evidence);
            super::optional_value(
                ui,
                "Compensate PWM to nominal voltage (V)",
                &mut c.nominal_voltage_for_compensation_v,
                12.,
            );
            ui.collapsing("Sampled controller protection",|ui| {
                ui.small("Commands zero PWM after a sampled violation, subject to declared command delay. Zero PWM is not a battery disconnect. Electrical acceptance checks above also inspect samples between controller ticks.");
                super::optional_value(ui,"Undervoltage threshold (V)",&mut c.limits.minimum_voltage_v,9.);
                super::optional_value(ui,"Discharge current threshold (A)",&mut c.limits.maximum_discharge_current_a,1.);
                super::optional_value(ui,"Charge current threshold (A)",&mut c.limits.maximum_charge_current_a,0.);
                super::optional_value(ui,"Winding current threshold (A)",&mut c.limits.maximum_winding_current_a,1.);
                super::optional_value(ui,"Draw power threshold (W)",&mut c.limits.maximum_draw_power_w,10.);
                super::optional_value(ui,"Returned power threshold (W)",&mut c.limits.maximum_return_power_w,0.);
            });
            if let Err(e) = c.validate() {
                ui.colored_label(Color32::DARK_RED, e);
            }
        }
        });
        if let Some(e) = &self.error {
            ui.colored_label(Color32::DARK_RED, e);
        }
        if ui
            .add_enabled(
                !busy && s.draft.power.is_some(),
                egui::Button::new("Run controller with this electrical source"),
            )
            .clicked()
        {
            run = Some(super::refinement::Action::Simulate);
        }
        let latest = s
            .refinement
            .controller_runs
            .iter()
            .rposition(|r| r.electrical.is_some());
        egui::ComboBox::from_id_salt("electrical-simulation-result")
            .selected_text(
                self.selected_run
                    .and_then(|i| s.refinement.controller_runs.get(i))
                    .map(|r| r.experiment.name.as_str())
                    .unwrap_or("Latest electrical simulation"),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.selected_run, None, "Latest electrical simulation");
                for (i, r) in s
                    .refinement
                    .controller_runs
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.electrical.is_some())
                {
                    ui.selectable_value(
                        &mut self.selected_run,
                        Some(i),
                        format!("{} · {}", i + 1, r.experiment.name),
                    );
                }
            });
        if let Some(r) = self
            .selected_run
            .or(latest)
            .and_then(|i| s.refinement.controller_runs.get(i))
            .filter(|r| r.electrical.is_some())
        {
            ui.strong(&r.experiment.name);
            ui.label(format!(
                "Motion tracking: {} · controller protection active on {} of {} ticks",
                match r.score.as_ref().map(|s| s.passes) {
                    Some(true) => "pass",
                    Some(false) => "fail",
                    None => "unscored",
                },
                r.frames
                    .iter()
                    .filter(|f| !f.electrical_limit_reasons.is_empty())
                    .count(),
                r.frames.len()
            ));
            if r.model != s.draft
                || serde_json::to_value(&r.experiment).unwrap()
                    != serde_json::to_value(&s.refinement.experiment).unwrap()
            {
                ui.colored_label(
                    Color32::DARK_RED,
                    "Captured electrical result differs from the current draft.",
                );
            }
            if let Some(failure) = &r.failure {
                ui.colored_label(Color32::DARK_RED, failure);
            }
            let trace = r.electrical.as_ref().unwrap();
            show_trace(ui, trace);
        }
        ui.separator();
        ui.strong("Available bench electrical measurements");
        ui.small("Servo voltage is retained. The current register is uncalibrated and its circuit location is unestablished; it is not converted to winding/battery amps or measured watts. Calibrated, synchronized current sensing is needed for current/power validation.");
        if let Some(r) = s.refinement.recordings.last() {
            super::refinement::voltage_chart(
                ui,
                "Measured servo voltage (V)",
                &[(
                    "Voltage",
                    Color32::from_rgb(30, 130, 200),
                    r.frames
                        .iter()
                        .map(|f| [f.control.observation.observed_s, f.voltage_v])
                        .collect(),
                )],
            );
            super::refinement::chart(
                ui,
                "Current register (raw uncalibrated counts)",
                &[(
                    "Raw register",
                    Color32::GRAY,
                    r.frames
                        .iter()
                        .map(|f| {
                            [
                                f.control.observation.observed_s,
                                f.current_raw_uncalibrated as f64,
                            ]
                        })
                        .collect(),
                )],
            );
        }
        ui.separator();
        ui.strong("Compare measured voltage, current and power");
        use super::refinement::Action;
        use sim_runtime::controller_refinement::recording::Purpose;
        egui::ComboBox::from_id_salt("power-recording")
            .selected_text(format!("Recording {}", self.recording + 1))
            .show_ui(ui, |ui| {
                for (i, r) in s.refinement.recordings.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.recording,
                        i,
                        format!(
                            "{} · ID{} · {}",
                            i + 1,
                            r.experiment.device,
                            r.experiment.name
                        ),
                    );
                }
            });
        ui.horizontal(|ui| {
            for (label, purpose) in [
                ("Predict recorded PWM", Purpose::RecordedCommandReplay),
                ("Predict closed loop", Purpose::ClosedLoopPrediction),
            ] {
                if ui
                    .add_enabled(
                        !busy
                            && s.draft.power.is_some()
                            && self.recording < s.refinement.recordings.len(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    run = Some(Action::Predict(self.recording, purpose));
                }
            }
        });
        egui::ComboBox::from_id_salt("power-prediction")
            .selected_text(format!("Prediction {}", self.prediction + 1))
            .show_ui(ui, |ui| {
                for (i, p) in s
                    .refinement
                    .predictions
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.electrical.is_some())
                {
                    ui.selectable_value(
                        &mut self.prediction,
                        i,
                        format!("{} · {:?} · {}", i + 1, p.purpose, &p.recording_hash[..8]),
                    );
                }
            });
        let available = s
            .refinement
            .predictions
            .get(self.prediction)
            .is_some_and(|p| p.electrical.is_some());
        if ui
            .add_enabled(
                !busy && available,
                egui::Button::new("Compare captured servo voltage"),
            )
            .clicked()
        {
            run = Some(Action::CompareElectrical(self.prediction, None));
        }
        ui.label("Electrical measurement JSON (calibration, circuit location, source hashes and clock evidence required)");
        ui.text_edit_singleline(&mut self.measurement_path);
        if ui
            .add_enabled(
                !busy && available && !self.measurement_path.trim().is_empty(),
                egui::Button::new("Import and compare electrical measurements"),
            )
            .clicked()
        {
            run = Some(Action::CompareElectrical(
                self.prediction,
                Some(self.measurement_path.trim().into()),
            ));
        }
        ui.small("Only synchronized voltage/current channels produce measured watts and joules. Missing channels and undeclared acceptance limits remain unscored. Every comparison retains its original measurements and thresholds.");
        if let Some(e) = s.refinement.electrical_comparisons.last() {
            ui.small(&e.method);
            for c in &e.channels {
                ui.label(format!(
                    "{} · RMS {:.5} {} · peak error {:.5} {} · {}",
                    c.name.replace('_', " "),
                    c.rmse,
                    c.measured.unit,
                    c.maximum_abs_error,
                    c.measured.unit,
                    match c.passes {
                        Some(true) => "pass",
                        Some(false) => "fail",
                        None => "unscored",
                    }
                ));
                super::refinement::chart(
                    ui,
                    &format!("{} ({})", c.name.replace('_', " "), c.measured.unit),
                    &[
                        (
                            "Measured",
                            Color32::from_rgb(30, 130, 200),
                            c.measured
                                .samples
                                .iter()
                                .map(|s| [s.time_s, s.value])
                                .collect(),
                        ),
                        (
                            "Predicted",
                            Color32::from_rgb(200, 110, 30),
                            c.predicted
                                .samples
                                .iter()
                                .map(|s| [s.time_s, s.value])
                                .collect(),
                        ),
                    ],
                );
            }
            if let Some(e) = &e.supply_energy {
                ui.label(format!("Energy over {:.3}–{:.3} s · drawn measured {:.5} / predicted {:.5} J · returned measured {:.5} / predicted {:.5} J",e.start_s,e.end_s,e.measured_drawn_j,e.predicted_drawn_j,e.measured_returned_j,e.predicted_returned_j));
            }
        }
        let changed = previous != s.draft || previous_control != s.refinement.experiment.electrical;
        if changed {
            s.candidate_edited();
        }
        (changed, run)
    }
}
fn show_trace(ui: &mut egui::Ui, trace: &power::Trace) {
    let summary = &trace.summary;
    ui.label(format!(
        "Voltage {:.3}–{:.3} V · drawn {:.4} Wh · returned {:.4} Wh",
        summary.minimum_voltage_v,
        summary.maximum_voltage_v,
        summary.drawn_energy_j / 3600.,
        summary.returned_energy_j / 3600.
    ));
    ui.label(format!(
        "Sampled peak supply {:.4} A · winding {:.4} A · draw {:.4} W",
        summary.peak_discharge_current_a, summary.peak_winding_current_a, summary.peak_draw_power_w
    ));
    ui.label(match summary.passes {
        Some(true) => "Passes declared electrical limits in this simulation",
        Some(false) => "Fails declared electrical limits",
        None => "No electrical acceptance limits declared",
    });
    for failure in &summary.violations {
        ui.colored_label(Color32::DARK_RED, failure);
    }
    super::refinement::voltage_chart(
        ui,
        "Supply voltage (V), axis follows the plotted range",
        &[(
            "Bus",
            Color32::from_rgb(30, 130, 200),
            trace
                .samples
                .iter()
                .map(|s| [s.time_s, s.supply_voltage_v])
                .collect(),
        )],
    );
    super::refinement::chart(
        ui,
        "Current (A), positive supply current draws energy",
        &[
            (
                "Supply",
                Color32::from_rgb(30, 130, 200),
                trace
                    .samples
                    .iter()
                    .map(|s| [s.time_s, s.supply_current_a])
                    .collect(),
            ),
            (
                "Winding",
                Color32::from_rgb(160, 100, 200),
                trace
                    .samples
                    .iter()
                    .map(|s| [s.time_s, s.winding_current_a])
                    .collect(),
            ),
        ],
    );
    super::refinement::chart(
        ui,
        "Supply power (W), negative returns energy",
        &[(
            "Power",
            Color32::from_rgb(50, 160, 100),
            trace
                .samples
                .iter()
                .map(|s| [s.time_s, s.supply_power_w])
                .collect(),
        )],
    );
    if trace.samples.iter().any(|s| s.state_of_charge.is_some()) {
        super::refinement::chart(
            ui,
            "State of charge (fraction)",
            &[(
                "SOC",
                Color32::from_rgb(150, 100, 20),
                trace
                    .samples
                    .iter()
                    .filter_map(|s| s.state_of_charge.map(|soc| [s.time_s, soc]))
                    .collect(),
            )],
        );
    }
    ui.small(&trace.interpretation);
}
