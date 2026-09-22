use super::refinement::chart;
use eframe::egui::{self, Color32};
use sim_runtime::controller_refinement::{
    fpga, fpga_review, motor_response as response, transients::TimingEstimate,
};
use std::{
    io::Write,
    sync::mpsc::{self, Receiver},
};

#[derive(Default)]
pub struct State {
    request: String,
    output_path: String,
    error: Option<String>,
    result: Option<response::Review>,
    pending: Option<Receiver<Result<response::Review, String>>>,
    prediction: Option<usize>,
}
fn timing(ui: &mut egui::Ui, label: &str, t: &TimingEstimate) {
    ui.label(format!(
        "{label}: {}",
        t.interval_s
            .map(|v| format!("{:.0}–{:.0} ms", v[0] * 1000., v[1] * 1000.))
            .unwrap_or_else(|| format!(
                "unresolved — {}",
                t.unresolved.as_deref().unwrap_or("missing evidence")
            ))
    ));
}
impl State {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        r: &fpga::Recording,
        id: u8,
        reviews: &[fpga_review::Review],
    ) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok(result)) => {
                    self.result = Some(result);
                    self.error = None;
                    self.pending = None;
                }
                Ok(Err(e)) => {
                    self.error = Some(e);
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("Response analysis worker stopped".into());
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        ui.collapsing("Speed-up, braking and direction reversal",|ui|{
            ui.label("Measure time to reach speed, cross zero and settle in the opposite direction. Select a captured command and declare the intended speeds, tolerance and dwell. Short position-tracking trials may not establish a steady speed.");
            if ui.button("Prepare editable request for selected motor").clicked(){
                let request=response::Request{setup_evidence:"Unloaded bench; attached inertia and load torque unmeasured. Replace with the captured fixture evidence.".into(),
                    estimator:response::Estimator{encoder_quantum_rad:std::f64::consts::TAU/4096.,lag_samples:1,maximum_sample_gap_s:r.plan.period_s*1.75,maximum_sample_step_rad:std::f64::consts::PI,maximum_sample_age_s:None},
                    trials:vec![response::Trial{motor_id:id,command_tick:1,last_observation_tick:r.frames.len()-1,from_rad_s:0.,target_rad_s:1.,tolerance_rad_s:0.05,dwell_s:0.3,travel_limited:false}]};
                self.request=serde_json::to_string_pretty(&request).unwrap();
            }
            ui.small("The template's 1 rad/s target and 0.05 rad/s tolerance are placeholders, not measured motor ratings. Edit all trial fields. Add entries for each motor to review simultaneous operation. Sample-age null remains unknown; current stays in raw counts.");
            ui.add(egui::TextEdit::multiline(&mut self.request).code_editor().desired_rows(12).desired_width(f32::INFINITY));
            let hash=r.fingerprint();
            let available:Vec<_>=reviews.iter().enumerate().filter(|(_,v)|v.recording_hash==hash && !v.cancelled).collect();
            if self.prediction.is_some_and(|i|!available.iter().any(|(j,_)|*j==i)){self.prediction=None;}
            egui::ComboBox::from_id_salt("motor-response-prediction").selected_text(self.prediction.map(|i|format!("Comparison {} · {}",i+1,reviews[i].mode.label())).unwrap_or("Measurements only".into())).show_ui(ui,|ui|{
                ui.selectable_value(&mut self.prediction,None,"Measurements only");
                for (i,p) in available {ui.selectable_value(&mut self.prediction,Some(i),format!("Comparison {} · {}",i+1,p.mode.label()));}
            });
            if ui.add_enabled(self.pending.is_none()&&!self.request.trim().is_empty(),egui::Button::new("Analyze declared response")).clicked(){
                match serde_json::from_str::<response::Request>(&self.request){
                    Err(e)=>self.error=Some(e.to_string()),
                    Ok(request)=>{let r=r.clone();let p=self.prediction.map(|i|reviews[i].clone());let (tx,rx)=mpsc::channel();self.pending=Some(rx);self.error=None;
                        std::thread::spawn(move||{let _=tx.send(response::review(&r,&request,p.as_ref()));});}
                }
            }
            if self.pending.is_some(){ui.label("Analyzing captured samples…");}
            if let Some(e)=&self.error{ui.colored_label(Color32::DARK_RED,e);}
            if let Some(result)=&self.result {
                if result.recording_blake3!=hash{ui.label("Saved result belongs to an earlier recording; run analysis for this selection.");}
                ui.small("Results preserve the request used when Analyze was clicked. Editing the request does not change a saved result.");
                for (i,t) in result.trials.iter().enumerate(){ui.push_id(i,|ui|{ui.collapsing(format!("Motor {} · {:.2} → {:.2} rad/s",t.conditions.motor_id,t.transition.from_rad_s,t.transition.target_rad_s),|ui|{
                    ui.label(format!("Supply {:.1}–{:.1} V · {:.0}–{:.0} °C · {} motors controlled together · {}",t.conditions.voltage_range_v[0],t.conditions.voltage_range_v[1],t.conditions.temperature_range_c[0],t.conditions.temperature_range_c[1],t.conditions.simultaneously_controlled_ids.len(),t.conditions.data_role));
                    let series=|accel:bool|{let mut lines=vec![];for (label,color,m) in [("Measured",Color32::BLUE,Some(&t.measured_motion)),("Predicted",Color32::DARK_GREEN,t.predicted_motion.as_ref())]{if let Some(m)=m{lines.push((label,color,if accel{&m.acceleration_rad_s2}else{&m.speed_rad_s}.iter().map(|s|[s.time_s,s.value]).collect()));}}lines};
                    chart(ui,"Estimated shaft speed (rad/s)",&series(false));chart(ui,"Estimated acceleration (rad/s²)",&series(true));
                    ui.small(format!("Largest sample gap {:.1} ms · speed-estimator support up to {:.1} ms · sample-age bound: {}",t.measured_motion.maximum_sample_gap_s*1000.,t.measured_motion.maximum_estimator_window_s*1000.,if t.measured_motion.sample_age_bounded{"declared"}else{"UNKNOWN — timing has additional uncertainty"}));
                    for (label,r) in [("Measured",Some(&t.comparison.measured)),("Predicted",t.comparison.predicted.as_ref())]{if let Some(r)=r{ui.strong(label);timing(ui,"Departure from initial speed",&r.onset_delay);timing(ui,"10–90% speed rise",&r.rise_10_to_90);timing(ui,"Total time to settled speed",&r.command_to_settled_speed);timing(ui,"Command to zero crossing",&r.command_to_zero_crossing);timing(ui,"Zero to settled opposite speed",&r.zero_to_settled_opposite_speed);if let Some(v)=r.sampled_forward_travel_before_reversal_rad{ui.label(format!("Sampled forward travel from pre-command reference: {:.3} rad",v));}ui.small(&r.interpretation);}}
                });});}
                ui.horizontal(|ui|{ui.label("New report JSON path");ui.text_edit_singleline(&mut self.output_path);if ui.add_enabled(!self.output_path.trim().is_empty(),egui::Button::new("Save response report")).clicked(){
                    let write=(||->Result<(),String>{let bytes=serde_json::to_vec_pretty(result).map_err(|e|e.to_string())?;let mut f=std::fs::OpenOptions::new().write(true).create_new(true).open(self.output_path.trim()).map_err(|e|e.to_string())?;f.write_all(&bytes).map_err(|e|e.to_string())})();self.error=write.err();
                }});
                ui.small(&result.interpretation);
            }
        });
    }
}
