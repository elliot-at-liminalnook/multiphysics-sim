use super::refinement::{Action, chart};
use eframe::egui::{self, Color32};
use sim_runtime::controller_refinement::{
    fpga::Recording,
    fpga_design::{Experiment, Profile, Run},
};
#[derive(Default)]
pub struct State {
    draft: Option<Experiment>,
    profile: Profile,
    error: Option<String>,
    path: String,
}
impl State {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        r: &Recording,
        busy: bool,
        runs: &[Run],
        drafts: &mut Vec<Experiment>,
    ) -> Option<Action> {
        let mut action = None;
        let hash = r.fingerprint();
        if self
            .draft
            .as_ref()
            .is_none_or(|d| d.timing_recording_hash != hash)
        {
            self.draft = drafts
                .iter()
                .find(|d| d.timing_recording_hash == hash)
                .cloned();
        }
        egui::CollapsingHeader::new("Design a new FPGA controller experiment").default_open(self.draft.is_some()).show(ui,|ui| {
            ui.label("Fork the captured controller, edit gains and motion, then simulate on its measured timing schedule. Design results are not physical validation.");
            if ui.add_enabled(r.completed,egui::Button::new("Create design from selected recording")).clicked() {
                let mut plan=r.plan.clone();plan.name=format!("Design: {}",plan.name);plan.role="training".into();
                self.draft=Some(Experiment{timing_recording_hash:r.fingerprint(),plan});self.error=None;
            }
            if let Some(draft)=&mut self.draft {
                if draft.timing_recording_hash!=r.fingerprint() {ui.label("Select the source recording again or create a new design to edit this draft.");return;}
                ui.text_edit_singleline(&mut draft.plan.name);
                ui.horizontal(|ui| {ui.label("Future measurement role");for role in ["training","validation"] {ui.selectable_value(&mut draft.plan.role,role.into(),role);}});
                ui.horizontal(|ui| {
                    gain(ui,"Position gain",&mut draft.plan.gains.kp_q8);
                    gain(ui,"Velocity damping",&mut draft.plan.gains.kd_q8);
                    gain(ui,"Velocity feedforward",&mut draft.plan.gains.kv_q8);
                });
                let mut duty=draft.plan.gains.limit as f64/10.;
                ui.horizontal(|ui| {ui.label("Duty limit (%)");if ui.add(egui::DragValue::new(&mut duty).range(0. ..=r.plan.gains.limit as f64/10.).speed(0.1)).changed(){draft.plan.gains.limit=(duty*10.).round() as u16;}});
                ui.small(format!("{} motors; {:.0} ms captured cadence; {} targets. Duty cannot exceed the source trial's commissioned ceiling.",draft.plan.ids.len(),draft.plan.period_s*1000.,draft.plan.targets.len()));
                ui.collapsing("Generate smooth changing-speed motion",|ui| {
                    field(ui,"Amplitude (encoder counts)",&mut self.profile.amplitude_counts,1.);
                    field(ui,"Fundamental frequency (Hz)",&mut self.profile.frequency_hz,0.05);
                    field(ui,"Harmonic frequency multiplier",&mut self.profile.harmonic_ratio,0.1);
                    field(ui,"Harmonic fraction",&mut self.profile.harmonic_fraction,0.05);
                    field(ui,"Start / finish ramp (seconds)",&mut self.profile.ramp_s,0.1);
                    field(ui,"Phase spread across motors (turns)",&mut self.profile.phase_spread_turns,0.05);
                    if ui.button("Generate bounded trajectory").clicked() {match self.profile.apply(&draft.plan){Ok(plan)=>{draft.plan=plan;self.error=None;},Err(e)=>self.error=Some(e)}}
                });
                let id=draft.plan.ids[0];
                chart(ui,"First motor target (degrees, sampled setpoints)",&[("Design target",Color32::GRAY,draft.plan.targets.iter().enumerate().map(|(i,t)|[i as f64*draft.plan.period_s,t[(id-4) as usize] as f64*360./4096.]).collect())]);
                let valid=draft.validate(r);
                if let Err(e)=&valid {ui.colored_label(Color32::DARK_RED,e);}
                if let Some(e)=&self.error {ui.colored_label(Color32::DARK_RED,e);}
                if ui.add_enabled(!busy&&valid.is_ok(),egui::Button::new("Simulate design against current model draft")).clicked(){action=Some(Action::DesignFpga(draft.clone()));}
                ui.horizontal(|ui| {ui.label("New plan file");ui.text_edit_singleline(&mut self.path);if ui.add_enabled(!busy&&valid.is_ok()&&!self.path.trim().is_empty(),egui::Button::new("Export experiment plan")).clicked(){action=Some(Action::ExportFpgaPlan(draft.clone(),self.path.trim().into()));}});
                ui.small("Export prepares a file for the supervised acquisition tool. Firmware loading and motor motion are separate actions. Current gains are shared across this motor group.");
            }
            for (index,run) in runs.iter().enumerate().rev().filter(|(_,run)|run.experiment.timing_recording_hash==r.fingerprint()) {
                ui.push_id(index,|ui| {ui.separator();ui.strong(&run.experiment.plan.name);ui.small("SIMULATION ONLY · sampled tracking, not motor-model prediction accuracy");
                    if run.cancelled {ui.label("Cancelled; partial results retained.");}
                    for (id,error) in &run.failures {ui.colored_label(Color32::DARK_RED,format!("ID {id}: unscored · {error}"));}
                    for axis in &run.axes {
                        ui.label(format!("ID {} · RMS {:.3}° · peak {:.3}° · saturation {:.1}% · {}",axis.id,axis.tracking.rms_counts*360./4096.,axis.tracking.peak_counts*360./4096.,axis.tracking.saturated_fraction*100.,if axis.tracking.passes{"tracking pass"}else{"tracking fail"}));
                        ui.collapsing(format!("Motor {} simulated response",axis.id),|ui| {chart(ui,"Simulated encoder position (degrees)",&[("Simulation",Color32::DARK_GREEN,axis.simulation.samples.iter().map(|p|[p[0],(p[1]-r.home[(axis.id-4) as usize] as f64)*360./4096.]).collect())]);});
                    }
                    ui.collapsing("Simulated electrical behavior",|ui| {
                        ui.small("Supply and winding channels are separate. These are model outputs; calibrated measured amps, watts and shared battery accuracy remain unvalidated.");
                        for axis in &run.axes {
                            if axis.simulation.electrical.is_empty() {ui.label(format!("ID {}: enable an explicit source model in Electrical & battery to record supply channels",axis.id));continue;}
                            ui.strong(format!("Motor {}",axis.id));
                            chart(ui,"Simulated supply voltage (V)",&[("Supply",Color32::BLUE,axis.simulation.electrical.iter().map(|p|[p.time_s,p.supply_voltage_v]).collect())]);
                            chart(ui,"Simulated current (A)",&[("Supply",Color32::BLUE,axis.simulation.electrical.iter().map(|p|[p.time_s,p.supply_current_a]).collect()),("Winding",Color32::DARK_GREEN,axis.simulation.electrical.iter().map(|p|[p.time_s,p.winding_current_a]).collect())]);
                            chart(ui,"Simulated supply power (W)",&[("Supply",Color32::BLUE,axis.simulation.electrical.iter().map(|p|[p.time_s,p.supply_power_w]).collect())]);
                        }
                    });
                    if ui.button("Edit a copy of this design").clicked(){self.draft=Some(run.experiment.clone());}
                });
            }
        });
        if let Some(draft) = &self.draft {
            if draft.timing_recording_hash == hash {
                if let Some(existing) = drafts.iter_mut().find(|d| d.timing_recording_hash == hash)
                {
                    *existing = draft.clone();
                } else {
                    drafts.push(draft.clone());
                }
            }
        }
        action
    }
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut f64, speed: f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(speed));
    });
}
fn gain(ui: &mut egui::Ui, label: &str, value: &mut u16) {
    let mut v = *value as f64 / 256.;
    ui.label(label);
    if ui
        .add(egui::DragValue::new(&mut v).range(0. ..=16.).speed(0.1))
        .changed()
    {
        *value = (v * 256.).round() as u16;
    }
}
