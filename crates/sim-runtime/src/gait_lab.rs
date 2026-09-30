//! Evaluate readable gait files ([`GaitScript`]) with a gait-search study's
//! robot, screens, qualified reduced model and gates. The same library calls
//! as `compare_gait_search`: a file becomes the study's motion template and
//! values, then preparation, simulation and scoring are unchanged. Reports are
//! written for people and language models to read and act on.
use crate::{
    contact_exploration,
    exploration::{self, CaptureSession},
    motion_evaluation::{self, Gates},
};
use serde::{Deserialize, Serialize};
use sim_domain_control::{
    gait_script::{GaitScript, NominalLeg, name_legs},
    maneuver_script::ManeuverScript,
    motion_parameters::Values,
    pose_script::{PoseScript, joint_refs},
    trajectory::Trajectory,
};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    time::Instant,
};

mod reports;
pub use reports::{JournalRecord, LabReport, ResultsEntry, ResultsListing, read_report, scan_results};

type R<T> = Result<T, String>;

/// The parts of a `compare_gait_search` config a gait file is evaluated with.
#[derive(Clone, Deserialize)]
struct StudyConfig {
    recipe: contact_exploration::Recipe,
    profile: exploration::Profile,
    baseline: Values,
    gates: Gates,
    qualification_directory: String,
    #[serde(default)]
    early_rejection_check_s: Option<f64>,
    /// What-if motor controller for every actuator family, applied after the
    /// registry sync (gait-lab studies only; the search config rejects it).
    #[serde(default)]
    controller_override: Option<ControllerOverride>,
}

/// Experimental override of the sampled fixed-PD motor controller (the FPGA
/// law): loop period, command latency and Q8 gains. Recorded in every report;
/// the accepted actuator registry is unchanged.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerOverride {
    pub note: String,
    #[serde(default)]
    pub period_s: Option<f64>,
    #[serde(default)]
    pub latency_s: Option<f64>,
    #[serde(default)]
    pub kp_q8: Option<u16>,
    #[serde(default)]
    pub kd_q8: Option<u16>,
    #[serde(default)]
    pub kv_q8: Option<u16>,
}
impl ControllerOverride {
    fn apply(&self, robot: &mut sim_domain_robot::PhysicalModel) -> R<()> {
        let profiles = robot.actuator_profiles.as_mut().ok_or("controller override needs actuator profiles")?;
        for family in profiles.families.values_mut() {
            let c = &mut family.controller;
            if let Some(v) = self.period_s {
                c.period.value = v;
            }
            if let Some(v) = self.latency_s {
                c.latency.value = v;
            }
            if let Some(v) = self.kp_q8 {
                c.gains.kp_q8 = v;
            }
            if let Some(v) = self.kd_q8 {
                c.gains.kd_q8 = v;
            }
            if let Some(v) = self.kv_q8 {
                c.gains.kv_q8 = v;
            }
        }
        Ok(())
    }
    fn describe(&self) -> String {
        let f = |name: &str, v: Option<String>| v.map(|v| format!("{name} {v}"));
        let parts: Vec<String> = [
            f("period", self.period_s.map(|v| format!("{:.0} ms", v * 1e3))),
            f("latency", self.latency_s.map(|v| format!("{:.0} ms", v * 1e3))),
            f("kp_q8", self.kp_q8.map(|v| v.to_string())),
            f("kd_q8", self.kd_q8.map(|v| v.to_string())),
            f("kv_q8", self.kv_q8.map(|v| v.to_string())),
        ]
        .into_iter()
        .flatten()
        .collect();
        format!("controller override ({}): {}", self.note, parts.join(", "))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    /// The study's qualified reduced model (task-level agreement with detailed).
    Fast,
    /// The detailed model the reduced one was qualified against.
    Detailed,
}

pub struct Study {
    config: StudyConfig,
    /// Content hash of the study config file; part of every result's identity.
    pub id: String,
    pub legs: Vec<NominalLeg>,
    /// Controller values a gait file must set, with the motor-derived range.
    pub controller_ranges: BTreeMap<String, [f64; 2]>,
    pub profile_name: String,
    /// Speedup of the reduced model, once [`Study::qualify`] has confirmed it
    /// is still qualified for the current runtime. Fast evaluation needs it.
    pub qualified_speedup: Option<f64>,
    repo_root: std::path::PathBuf,
    /// The config file as written, for deriving tuning studies from it.
    raw: serde_json::Value,
}

fn read<T: serde::de::DeserializeOwned>(p: &Path) -> R<T> {
    let bytes = fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", p.display()))
}

impl Study {
    /// Load a study config and apply the accepted actuator registry. Enough
    /// for exporting and validating gait files; see [`Study::qualify`].
    pub fn load(config_path: &Path, repo_root: &Path) -> R<Self> {
        let bytes = fs::read(config_path).map_err(|e| format!("{}: {e}", config_path.display()))?;
        let mut config: StudyConfig = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", config_path.display()))?;
        config.recipe.sync_actuators(repo_root)?;
        if let Some(o) = &config.controller_override {
            if o.period_s.is_some_and(|v| !(v > 0. && v <= 1.)) || o.latency_s.is_some_and(|v| !(v >= 0. && v <= 1.)) {
                return Err("controller override period must be in (0, 1] s and latency in [0, 1] s".into());
            }
            o.apply(&mut config.recipe.experiment.scene.robot)?;
            o.apply(&mut config.recipe.planning_scene.robot)?;
        }
        let nominal = config.recipe.template.materialize(&config.baseline)?;
        let legs = name_legs(&nominal.feet.iter().map(|f| f.center_world_m).collect::<Vec<_>>())?;
        let space = &config.recipe.template.space.parameters;
        let controller_ranges = config
            .recipe
            .policy_bindings
            .iter()
            .map(|b| {
                let p = space.iter().find(|p| p.name == b.parameter).ok_or(format!("policy parameter {} is not in the study's space", b.parameter))?;
                Ok((b.parameter.clone(), p.bounds))
            })
            .collect::<R<_>>()?;
        Ok(Self {
            id: blake3::hash(&bytes).to_hex().to_string(),
            profile_name: config.profile.name.clone(),
            qualified_speedup: None,
            config,
            legs,
            controller_ranges,
            repo_root: repo_root.to_path_buf(),
            raw: serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        })
    }

    /// Confirm the study's reduced profile is qualified for the current
    /// runtime (library source and saved captures), as the search does.
    pub fn qualify(&mut self) -> R<f64> {
        let qroot = self.repo_root.join(&self.config.qualification_directory);
        let recipe: exploration::Recipe = read(&qroot.join("recipe.json"))?;
        let receipt: exploration::Qualification = read(&qroot.join("qualification.json"))?;
        if serde_json::to_value(&recipe.profile).map_err(|e| e.to_string())? != serde_json::to_value(&self.config.profile).map_err(|e| e.to_string())? {
            return Err("the study's profile differs from its qualified profile".into());
        }
        let q = exploration::qualify(
            &recipe,
            &read(&qroot.join("detailed.capture.json"))?,
            &read(&qroot.join("reduced.capture.json"))?,
            receipt.report.plan.reference,
            receipt.report.plan.candidate,
        )
        .map_err(|e| format!("{e} ({}; re-qualify it with reduced_exploration after library changes)", qroot.display()))?;
        if !q.qualified {
            return Err("the study's reduced profile is not qualified for the current runtime".into());
        }
        self.qualified_speedup = Some(q.speedup);
        Ok(q.speedup)
    }

    pub fn direction(&self) -> [f64; 3] {
        self.config.gates.direction_world
    }

    /// Gates for a gait file: speed is measured along its travel heading.
    fn gates_for(&self, script: &GaitScript) -> R<Gates> {
        let mut gates = self.config.gates.clone();
        gates.direction_world = script.cycle.travel_direction(self.direction())?;
        Ok(gates)
    }

    /// The study recipe with the file's motion, and the values to prepare it at.
    pub fn with_script(&self, script: &GaitScript) -> R<(contact_exploration::Recipe, Values)> {
        let wanted: Vec<&String> = self.controller_ranges.keys().collect();
        let given: Vec<&String> = script.controller.keys().collect();
        if wanted != given {
            return Err(format!("controller must set exactly {wanted:?} (file sets {given:?})"));
        }
        for (name, [lo, hi]) in &self.controller_ranges {
            let v = script.controller[name];
            if !(*lo..=*hi).contains(&v) {
                return Err(format!("controller.{name} = {v} is outside the study's motor-derived range [{lo:.4}, {hi:.4}]"));
            }
            if let Some(r) = script.tune.get(&format!("controller.{name}")) {
                if r[0] < *lo || r[1] > *hi {
                    return Err(format!("tune.controller.{name} {r:?} must stay inside [{lo:.4}, {hi:.4}]"));
                }
            }
        }
        let (template, values) = script.template(&self.legs, self.direction())?;
        let mut recipe = self.config.recipe.clone();
        recipe.template = template;
        // The planner's nominal speed (and the forward command it scales) follow the travel heading.
        recipe.compiler.robot.direction_world = script.cycle.travel_direction(self.direction())?;
        Ok((recipe, values))
    }

    /// The study's own motion at a trial's values.
    pub fn study_motion(&self, values: &Values) -> R<sim_domain_control::contact_phase::ContactPhaseConfig> {
        self.config.recipe.template.materialize(values)
    }

    /// A study trial's values as a gait file.
    pub fn export(&self, values: &Values) -> R<GaitScript> {
        let motion = self.config.recipe.template.materialize(values)?;
        let controller = self.controller_ranges.keys().map(|k| Ok((k.clone(), *values.get(k).ok_or(format!("values lack {k}"))?))).collect::<R<_>>()?;
        GaitScript::from_motion(&motion, &self.legs, self.direction(), controller)
    }

    /// Screen, prepare, simulate and score one gait file. Results live in
    /// `out/<name>-<hash>/`; an identical file, study and fidelity reuses the
    /// saved report instead of running again.
    pub fn evaluate(&self, script: &GaitScript, file: &Path, out: &Path, fidelity: Fidelity, stop: &dyn Fn() -> bool) -> R<GaitReport> {
        let canonical = serde_json::to_string(script).map_err(|e| e.to_string())?;
        let key = blake3::hash(format!("{}|{:?}|{canonical}", self.id, fidelity).as_bytes()).to_hex().to_string();
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("gait");
        let label = if script.name.is_empty() { stem } else { &script.name };
        let slug: String = label.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
        let dir = out.join(format!("{slug}-{}", &key[..8]));
        let report_path = dir.join("report.yaml");
        if let Ok(text) = fs::read_to_string(&report_path) {
            if let Ok(mut cached) = serde_norway::from_str::<GaitReport>(&text) {
                cached.cached = true;
                return Ok(cached);
            }
        }
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join("gait.yaml"), serde_norway::to_string(script).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut report = GaitReport::new(label, file, &dir, fidelity, &self.profile_name);
        if let Some(o) = &self.config.controller_override {
            report.fidelity = format!("{}; {}", report.fidelity, o.describe());
        }
        let finish = |report: GaitReport| -> R<GaitReport> {
            fs::write(&report_path, report.to_yaml()?).map_err(|e| e.to_string())?;
            Ok(report)
        };
        let (recipe, values, gates) = match self.with_script(script).and_then(|(r, v)| Ok((r, v, self.gates_for(script)?))) {
            Ok(v) => v,
            Err(e) => return finish(report.stop("invalid", e)),
        };
        let started = Instant::now();
        if let Err(e) = recipe.schedule_screen(&values) {
            return finish(report.stop("screened_out", format!("schedule: {e}")));
        }
        let prepared = recipe.prepare(&values);
        report.timing.preparation_s = started.elapsed().as_secs_f64();
        let prepared = match prepared {
            Ok(p) => p,
            Err(e) => return finish(report.stop("screened_out", e)),
        };
        fs::write(dir.join("compiled.json"), serde_json::to_vec(&prepared.compiled).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        report.joints = joint_lines(&recipe, &prepared.screen);
        let paired = exploration::Recipe { version: 1, detailed: prepared.spec, profile: self.config.profile.clone() }.prepare()?;
        // Playback (panel, leg) runs the gait through the governor the simulation used.
        let governor = paired.detailed.scene.controller.as_ref().map(|c| c.parameters["reference_governor"].clone());
        fs::write(dir.join("spec-identity.json"), serde_json::to_vec(&serde_json::json!({"reference_governor": governor})).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let spec = match fidelity {
            Fidelity::Fast if self.qualified_speedup.is_none() => return Err("fast evaluation needs a qualified reduced profile; call Study::qualify first".into()),
            Fidelity::Fast => &paired.reduced,
            Fidelity::Detailed => &paired.detailed,
        };
        let sim_start = Instant::now();
        let mut run = CaptureSession::new(spec)?;
        let mut checked = 0.;
        let mut early = None;
        while !run.done() && !stop() {
            run.advance()?;
            if let Some(every) = self.config.early_rejection_check_s {
                if run.time_s() - checked >= every {
                    checked = run.time_s();
                    let partial = run.capture(sim_start.elapsed().as_secs_f64());
                    if let Some(reason) = motion_evaluation::evaluate(&partial, &gates).ok().as_ref().and_then(motion_evaluation::decided_rejection) {
                        early = Some(format!("stopped at {:.2} s: {reason}", run.time_s()));
                        break;
                    }
                }
            }
        }
        let capture = run.capture(sim_start.elapsed().as_secs_f64());
        report.timing.simulation_s = sim_start.elapsed().as_secs_f64();
        report.timing.simulated_s = capture.recording.completed_steps as f64 * capture.recording.config.step_s;
        let evaluation = motion_evaluation::evaluate(&capture, &gates)?;
        fs::write(dir.join("evaluation.json"), serde_json::to_vec_pretty(&evaluation).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        report.score(&evaluation, &gates, early);
        finish(report)
    }
}

impl Study {
    /// Kinematic checks shared by every joint path (pose sequences,
    /// maneuvers): controller command range at `bounded` samples, peak
    /// speed per coordinate against the motor, and CAD closure and authored
    /// limits at `cad` samples. Angles are in study coordinate order.
    fn joint_checks(&self, bounded: &[(String, Vec<f64>)], peaks: &[f64], speed_hint: &str, cad: &[(String, Vec<f64>)]) -> R<(Vec<String>, Vec<JointLine>)> {
        let recipe = &self.config.recipe;
        let coordinates = &recipe.compiler.robot.independent_coordinates;
        let mut reasons = vec![];
        let mut seen = std::collections::BTreeSet::new();
        // Command bounds the controller enforces, per coordinate.
        let params = recipe.experiment.scene.controller.as_ref().map(|c| &c.parameters).ok_or("study has no runtime controller")?;
        for (label, angles) in bounded {
            for (c, a) in coordinates.iter().zip(angles) {
                let joint = c.trim_start_matches("joint.");
                if let Some(b) = params["output_bounds"][format!("{joint}.target")].as_array() {
                    let (lo, hi) = (b[0].as_f64().unwrap_or(f64::NEG_INFINITY), b[1].as_f64().unwrap_or(f64::INFINITY));
                    if !(lo..=hi).contains(a) && seen.insert(format!("bound {joint}")) {
                        reasons.push(format!("{label}: {joint} {:.1} deg is outside the command range [{:.1}, {:.1}] deg", a.to_degrees(), lo.to_degrees(), hi.to_degrees()));
                    }
                }
            }
        }
        let mut joints = vec![];
        for (i, c) in coordinates.iter().enumerate() {
            let (peak, limit) = (peaks[i], recipe.maximum_reference_speed_rad_s.get(i).copied());
            if let Some(l) = limit.filter(|l| peak > *l) {
                reasons.push(format!("{c}: {peak:.2} rad/s exceeds the motor's {l:.2} rad/s; {speed_hint}"));
            }
            joints.push(JointLine {
                joint: c.clone(),
                reference_peak_rad_s: Some(peak),
                motor_limit_rad_s: limit,
                percent_of_limit: limit.map(|l| (100. * peak / l).round()),
                tracking_rms_deg: None,
                tracking_peak_deg: None,
            });
        }
        joints.sort_by(|a, b| b.percent_of_limit.partial_cmp(&a.percent_of_limit).unwrap_or(std::cmp::Ordering::Equal));
        let mut mirror = crate::kinematic_mirror::KinematicMirror::new(recipe.experiment.scene.clone(), 0.)?;
        let order: Vec<usize> = mirror
            .coordinates()
            .iter()
            .map(|m| coordinates.iter().position(|c| c.trim_start_matches("joint.") == m.joint).ok_or(format!("CAD motor joint {} is not a study coordinate", m.joint)))
            .collect::<R<_>>()?;
        for (label, q) in cad {
            let angles: Vec<f64> = order.iter().map(|&i| q[i]).collect();
            match mirror.pose(&angles) {
                Ok(p) => {
                    for v in p.authored_limit_violations {
                        if seen.insert(v.clone()) {
                            reasons.push(format!("{label}: {v} is beyond its CAD limit"));
                        }
                    }
                    if p.maximum_scaled_closure_error > 1e-6 && seen.insert(format!("closure@{label}")) {
                        reasons.push(format!("{label}: the CAD linkage does not close (error {:.1e})", p.maximum_scaled_closure_error));
                    }
                }
                Err(e) => {
                    if seen.insert(format!("solve@{label}")) {
                        reasons.push(format!("{label}: the CAD linkage cannot reach this pose: {e}"));
                    }
                }
            }
        }
        Ok((reasons, joints))
    }

    /// Compile a pose sequence against the study's robot and check it without
    /// physics: controller command bounds, motor speed at the study's supply,
    /// and the CAD linkage (closure and authored joint limits) at every pose
    /// and between poses. Writes `out/<name>-<hash>/compiled.json` (playable)
    /// and `report.yaml`.
    pub fn compile_poses(&self, script: &PoseScript, file: &Path, out: &Path) -> R<PoseReport> {
        let canonical = serde_json::to_string(script).map_err(|e| e.to_string())?;
        let key = blake3::hash(format!("{}|poses|{canonical}", self.id).as_bytes()).to_hex().to_string();
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("poses");
        let label = if script.name.is_empty() { stem } else { &script.name };
        let slug: String = label.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
        let dir = out.join(format!("{slug}-{}", &key[..8]));
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join("poses.yaml"), serde_norway::to_string(script).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let recipe = &self.config.recipe;
        let coordinates = &recipe.compiler.robot.independent_coordinates;
        let mut report = PoseReport {
            kind: "pose_sequence".into(),
            sequence: label.into(),
            file: file.display().to_string(),
            status: "invalid".into(),
            summary: String::new(),
            period_s: None,
            reasons: vec![],
            joints: vec![],
            results_directory: dir.display().to_string(),
            compiled_gait: None,
        };
        let finish = |report: PoseReport| -> R<PoseReport> {
            fs::write(dir.join("report.yaml"), serde_norway::to_string(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            Ok(report)
        };
        let compiled = match joint_refs(coordinates).and_then(|j| script.compile(&j, &recipe.compiler.robot.initial_coordinates)) {
            Ok(c) => c,
            Err(e) => {
                report.summary = format!("Not compiled: {e}");
                report.reasons = vec![e];
                return finish(report);
            }
        };
        report.period_s = Some(compiled.period_s);
        let curve = Trajectory::new(compiled.trajectory.clone())?;
        let k = &compiled.trajectory.keyframes;
        let times: Vec<f64> = k.windows(2).flat_map(|w| (0..4).map(move |i| w[0].time_s + (w[1].time_s - w[0].time_s) * i as f64 / 4.)).chain([compiled.period_s]).collect();
        let cad = times.iter().map(|&t| Ok((format!("at {t:.2} s"), curve.sample(t)?.values))).collect::<R<Vec<_>>>()?;
        let named: Vec<(String, Vec<f64>)> = compiled.poses.iter().map(|(name, a)| (format!("pose {name}"), a.clone())).collect();
        let (reasons, joints) = self.joint_checks(&named, &compiled.maximum_rates_rad_s, "lengthen the move into or out of it", &cad)?;
        report.joints = joints;
        let mut poses_deg = serde_json::Map::new();
        for (name, angles) in &compiled.poses {
            let named: BTreeMap<String, f64> = coordinates.iter().zip(angles).map(|(c, a)| (c.trim_start_matches("joint.").to_string(), a.to_degrees())).collect();
            poses_deg.insert(name.clone(), serde_json::json!(named));
        }
        let playable = serde_json::json!({
            "kind": "pose_sequence", "name": label, "trajectory": compiled.trajectory, "period_s": compiled.period_s,
            "recipe": {"independent_coordinates": coordinates}, "nominal_speed_m_s": 0.0, "poses_deg": poses_deg,
            "scope": "Joint-space pose sequence: kinematic checks only (command bounds, motor speed, CAD closure and limits); not simulated with physics.",
        });
        fs::write(dir.join("compiled.json"), serde_json::to_vec(&playable).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        report.reasons = reasons;
        if report.reasons.is_empty() {
            report.status = "ready".into();
            report.compiled_gait = Some(format!("{}/compiled.json", report.results_directory));
            let top = report.joints.first().map(|j| format!("{} at {:.0}% of motor speed", j.joint, j.percent_of_limit.unwrap_or(0.))).unwrap_or_default();
            report.summary = format!("Ready: {} poses over a {:.2} s loop, inside command bounds and CAD limits. Fastest joint: {top}.", compiled.poses.len(), compiled.period_s);
        } else {
            report.status = "blocked".into();
            report.summary = format!("Blocked: {}.", report.reasons.join("; "));
        }
        finish(report)
    }

    /// Play a maneuver file through the steered gait on the study's CAD
    /// robot and check it without physics: every reference must be reachable
    /// (IK, no internal contact), inside the controller's command range, the
    /// motors' speed and the CAD limits. Writes `out/<name>-<hash>/report.yaml`
    /// and `trace.json` (joint angles, body path, feet).
    pub fn check_maneuver(&self, gait: &GaitScript, maneuver: &ManeuverScript, file: &Path, out: &Path) -> R<ManeuverReport> {
        let canonical = serde_json::to_string(&(gait, maneuver)).map_err(|e| e.to_string())?;
        let key = blake3::hash(format!("{}|maneuver|{canonical}", self.id).as_bytes()).to_hex().to_string();
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("maneuver");
        let label = if maneuver.name.is_empty() { stem } else { &maneuver.name };
        let slug: String = label.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
        let dir = out.join(format!("{slug}-{}", &key[..8]));
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut report = ManeuverReport {
            kind: "maneuver".into(),
            maneuver: label.into(),
            gait: gait.name.clone(),
            file: file.display().to_string(),
            status: "invalid".into(),
            summary: String::new(),
            reasons: vec![],
            joints: vec![],
            checked_s: 0.,
            travel_m: [0.; 2],
            turned_deg: 0.,
            maximum_marker_error_m: 0.,
            maneuver_overlap_mm: 0.,
            gait_overlap_mm: 0.,
            results_directory: dir.display().to_string(),
        };
        let finish = |report: ManeuverReport| -> R<ManeuverReport> {
            fs::write(dir.join("report.yaml"), serde_norway::to_string(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            Ok(report)
        };
        let prepared = (|| {
            let motion = gait.motion(&self.legs, self.direction())?;
            let forward = gait.cycle.travel_direction(self.direction())?;
            let steering = maneuver.steering(forward[1].atan2(forward[0]))?;
            Ok::<_, String>((motion, steering))
        })();
        let (motion, steering) = match prepared {
            Ok(v) => v,
            Err(e) => {
                report.summary = format!("Not checked: {e}");
                report.reasons = vec![e];
                return finish(report);
            }
        };
        let recipe = &self.config.recipe;
        let mut session = crate::session::Session::new(recipe.planning_scene.clone(), 0)?;
        session.robot.art.contact_on = false;
        let seed = session.robot.generalized();
        let planner = crate::contact_planning::ContactPlanner::new(&session.robot.art, &seed, &recipe.markers, recipe.compiler.robot.clone())?;
        let dt = 0.01;
        let start_s = maneuver.start_phase * motion.period_s;
        let trace = planner.trace_steered(motion.clone(), steering.clone(), start_s, [0.; 3], |t| maneuver.command_at(t), maneuver.duration_s, dt)?;
        // The gait's own straight walk through the same placement: what the
        // study already accepts, for comparison.
        let native = sim_domain_control::contact_phase::steered::native_twist(&motion, &steering);
        let baseline = planner.trace_steered(motion.clone(), steering.clone(), 0., native, |_| native, 2. * motion.period_s, dt)?;
        let deepest = |t: &crate::contact_planning::SteeredTrace| t.internal_overlap_m.iter().copied().fold(0., f64::max);
        report.gait_overlap_mm = 1e3 * deepest(&baseline);
        report.maneuver_overlap_mm = 1e3 * deepest(&trace);
        let n = trace.coordinates.len();
        let mut peaks = vec![0f64; recipe.compiler.robot.independent_coordinates.len()];
        for w in trace.coordinates.windows(2) {
            for (p, (a, b)) in peaks.iter_mut().zip(w[0].iter().zip(&w[1])) {
                *p = p.max((b - a).abs() / dt);
            }
        }
        let at = |i: usize| format!("at {:.2} s", trace.times_s[i]);
        let bounded: Vec<_> = (0..n).map(|i| (at(i), trace.coordinates[i].clone())).collect();
        let cad: Vec<_> = (0..n).step_by(10).map(|i| (at(i), trace.coordinates[i].clone())).collect();
        let (mut reasons, joints) = self.joint_checks(&bounded, &peaks, "lower the command or its rate limits", &cad)?;
        let tolerance = recipe.compiler.robot.penetration_tolerance_m;
        if let Some((t, pair, depth)) = trace.worst_overlap.as_ref().filter(|w| w.2 > tolerance.max(deepest(&baseline))) {
            reasons.push(format!(
                "at {t:.2} s: {pair} overlap {:.3} mm, beyond the planner's {:.3} mm and the gait's own {:.3} mm",
                depth * 1e3,
                tolerance * 1e3,
                report.gait_overlap_mm
            ));
        }
        if let Some(e) = &trace.failure {
            reasons.insert(0, format!("stopped after {:.2} s: {e}", trace.times_s.last().copied().unwrap_or(0.)));
        }
        report.joints = joints;
        report.checked_s = trace.times_s.last().copied().unwrap_or(0.);
        report.maximum_marker_error_m = trace.maximum_marker_error_m;
        if let (Some(first), Some(last)) = (trace.samples.first(), trace.samples.last()) {
            report.travel_m = [last.path_pose[0] - first.path_pose[0], last.path_pose[1] - first.path_pose[1]];
            report.turned_deg = (last.path_pose[2] - first.path_pose[2]).to_degrees();
        }
        let path: Vec<_> = trace.samples.iter().map(|s| serde_json::json!({"path_pose": s.path_pose, "path_twist": s.path_twist, "body_offset_m": s.body_offset_m, "feet_world_m": s.feet.iter().map(|f| f.position_world_m).collect::<Vec<_>>(), "in_contact": s.feet.iter().map(|f| f.in_contact).collect::<Vec<_>>()})).collect();
        fs::write(
            dir.join("trace.json"),
            serde_json::to_vec(&serde_json::json!({
                "kind": "maneuver_trace", "maneuver": maneuver, "gait": gait, "steering": steering, "sample_interval_s": dt,
                "independent_coordinates": recipe.compiler.robot.independent_coordinates, "times_s": trace.times_s, "coordinates_rad": trace.coordinates,
                "path": path, "failure": trace.failure, "internal_overlap_m": trace.internal_overlap_m,
                "scope": "Kinematic trace of the steered gait on the CAD model: IK placement, command range, motor speed, CAD limits. No loads, contact or balance.",
            }))
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        report.reasons = reasons;
        let moved = format!("{:.2} m, turned {:.0} deg over {:.1} s", report.travel_m[0].hypot(report.travel_m[1]), report.turned_deg, report.checked_s);
        if report.reasons.is_empty() {
            report.status = "ready".into();
            let top = report.joints.first().map(|j| format!("{} at {:.0}% of motor speed", j.joint, j.percent_of_limit.unwrap_or(0.))).unwrap_or_default();
            report.summary = format!("Ready (kinematics only): moved {moved}. Fastest joint: {top}.");
        } else {
            report.status = "blocked".into();
            report.summary = format!("Blocked after moving {moved}: {}.", report.reasons.join("; "));
        }
        finish(report)
    }

    /// A `compare_gait_search` config that searches the file's `tune` ranges
    /// from the file's values, with the study's settings. Controller values
    /// not being tuned become fixed controller parameters. Its reduced model
    /// must be qualified on this baseline (`qualification_directory`).
    pub fn tune_config(&self, script: &GaitScript, qualification_directory: &str) -> R<serde_json::Value> {
        let (mut recipe, mut values) = self.with_script(script)?;
        for name in self.controller_ranges.keys() {
            if script.tune.contains_key(&format!("controller.{name}")) {
                continue;
            }
            let b = recipe.policy_bindings.iter().position(|b| &b.parameter == name).ok_or(format!("no policy binding for {name}"))?;
            let binding = recipe.policy_bindings.remove(b);
            let value = values.remove(name).ok_or(format!("missing {name}"))?;
            let params = &mut recipe.experiment.scene.controller.as_mut().ok_or("study has no runtime controller")?.parameters;
            *params.pointer_mut(&binding.pointer).ok_or(format!("controller has no value at {}", binding.pointer))? = serde_json::json!(value);
            recipe.template.space.parameters.retain(|p| &p.name != name);
        }
        if recipe.template.space.parameters.is_empty() {
            return Err("the file's tune section names nothing to search".into());
        }
        if let Some(p) = recipe.template.space.parameters.iter().find(|p| p.bounds[0] >= p.bounds[1]) {
            return Err(format!("tune range for {} is empty", p.name));
        }
        let mut config = self.raw.clone();
        config["recipe"] = serde_json::to_value(&recipe).map_err(|e| e.to_string())?;
        config["baseline"] = serde_json::to_value(&values).map_err(|e| e.to_string())?;
        config["qualification_directory"] = serde_json::json!(qualification_directory);
        Ok(config)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoseReport {
    pub kind: String,
    pub sequence: String,
    pub file: String,
    /// ready (playable) | blocked (a check failed) | invalid (file problem).
    pub status: String,
    pub summary: String,
    pub period_s: Option<f64>,
    pub reasons: Vec<String>,
    /// Joints ordered by how close the fastest move comes to motor speed.
    pub joints: Vec<JointLine>,
    pub results_directory: String,
    pub compiled_gait: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManeuverReport {
    pub kind: String,
    pub maneuver: String,
    pub gait: String,
    pub file: String,
    /// ready (every check passed) | blocked (a check failed) | invalid (file problem).
    pub status: String,
    pub summary: String,
    pub reasons: Vec<String>,
    /// Joints ordered by how close the fastest move comes to motor speed.
    pub joints: Vec<JointLine>,
    /// How much of the maneuver was placed before any stop.
    pub checked_s: f64,
    /// Planned body path: world x/y travel and turn from start to the end.
    pub travel_m: [f64; 2],
    pub turned_deg: f64,
    pub maximum_marker_error_m: f64,
    /// Deepest internal link overlap of the maneuver's references, and of
    /// the gait's own straight walk through the same placement (mm).
    pub maneuver_overlap_mm: f64,
    pub gait_overlap_mm: f64,
    pub results_directory: String,
}

/// Parse a maneuver file (YAML or JSON).
pub fn read_maneuver(path: &Path) -> R<ManeuverScript> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_norway::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Parse a pose file (YAML or JSON).
pub fn read_poses(path: &Path) -> R<PoseScript> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_norway::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn joint_lines(recipe: &contact_exploration::Recipe, screen: &serde_json::Value) -> Vec<JointLine> {
    let names = &recipe.compiler.robot.independent_coordinates;
    let peaks: Vec<f64> = serde_json::from_value(screen["reference_maximum_speed_rad_s"].clone()).unwrap_or_default();
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let peak = peaks.get(i).copied();
            let limit = recipe.maximum_reference_speed_rad_s.get(i).copied();
            JointLine {
                joint: name.clone(),
                reference_peak_rad_s: peak,
                motor_limit_rad_s: limit,
                percent_of_limit: peak.zip(limit).map(|(p, l)| (100. * p / l).round()),
                tracking_rms_deg: None,
                tracking_peak_deg: None,
            }
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateLine {
    pub gate: String,
    pub value: f64,
    pub limit: f64,
    pub ok: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JointLine {
    pub joint: String,
    /// Fastest the planned motion asks this joint to move.
    pub reference_peak_rad_s: Option<f64>,
    /// Motor speed at the study's supply (the reference-speed screen).
    pub motor_limit_rad_s: Option<f64>,
    pub percent_of_limit: Option<f64>,
    pub tracking_rms_deg: Option<f64>,
    pub tracking_peak_deg: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Timing {
    pub preparation_s: f64,
    pub simulation_s: f64,
    pub simulated_s: f64,
}

/// What happened to one gait file, for a person or a language model to act on.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GaitReport {
    pub gait: String,
    pub file: String,
    /// passed | rejected (simulated, failed a gate) | screened_out (never
    /// simulated: kinematics or speed) | invalid (file problem).
    pub status: String,
    pub summary: String,
    pub speed_m_s: Option<f64>,
    pub forward_distance_m: Option<f64>,
    pub simulated_s: Option<f64>,
    pub gates: Vec<GateLine>,
    pub reasons: Vec<String>,
    /// Joints ordered by how close the plan comes to their motor speed.
    pub joints: Vec<JointLine>,
    pub fidelity: String,
    pub timing: Timing,
    pub results_directory: String,
    /// Compiled gait for playback (sim and physical leg), when prepared.
    pub compiled_gait: Option<String>,
    #[serde(default, skip_serializing)]
    pub cached: bool,
}

impl GaitReport {
    fn new(gait: &str, file: &Path, dir: &Path, fidelity: Fidelity, profile: &str) -> Self {
        Self {
            gait: gait.into(),
            file: file.display().to_string(),
            status: "failed".into(),
            summary: String::new(),
            speed_m_s: None,
            forward_distance_m: None,
            simulated_s: None,
            gates: vec![],
            reasons: vec![],
            joints: vec![],
            fidelity: match fidelity {
                Fidelity::Fast => format!("fast: reduced model {profile}, qualified at task level against the detailed model; confirm finalists with --fidelity detailed"),
                Fidelity::Detailed => "detailed model".into(),
            },
            timing: Timing::default(),
            results_directory: dir.display().to_string(),
            compiled_gait: None,
            cached: false,
        }
    }
    fn stop(mut self, status: &str, reason: String) -> Self {
        self.summary = match status {
            "invalid" => format!("Not evaluated: the file is not a valid gait. {reason}"),
            _ => format!("Not simulated: rejected before physics. {reason}"),
        };
        self.status = status.into();
        self.reasons = vec![reason];
        self
    }
    fn score(&mut self, e: &motion_evaluation::Report, g: &Gates, early: Option<String>) {
        self.compiled_gait = Some(format!("{}/compiled.json", self.results_directory));
        self.forward_distance_m = Some(e.forward_displacement_m);
        self.simulated_s = Some(e.observed_s);
        let worst = |v: &[f64]| v.iter().cloned().fold(0., f64::max);
        self.gates = vec![
            GateLine { gate: "body_up_z_minimum".into(), value: e.minimum_body_up_z, limit: g.minimum_body_up_z, ok: e.minimum_body_up_z >= g.minimum_body_up_z },
            GateLine { gate: "tracking_rms_deg_worst_joint".into(), value: worst(&e.tracking_rms_rad).to_degrees(), limit: g.maximum_tracking_rms_rad.to_degrees(), ok: worst(&e.tracking_rms_rad) <= g.maximum_tracking_rms_rad },
            GateLine { gate: "tracking_peak_deg_worst_joint".into(), value: worst(&e.tracking_peak_rad).to_degrees(), limit: g.maximum_tracking_peak_rad.to_degrees(), ok: worst(&e.tracking_peak_rad) <= g.maximum_tracking_peak_rad },
        ];
        for (j, (rms, peak)) in self.joints.iter_mut().zip(e.tracking_rms_rad.iter().zip(&e.tracking_peak_rad)) {
            j.tracking_rms_deg = Some(rms.to_degrees());
            j.tracking_peak_deg = Some(peak.to_degrees());
        }
        self.joints.sort_by(|a, b| b.percent_of_limit.partial_cmp(&a.percent_of_limit).unwrap_or(std::cmp::Ordering::Equal));
        self.reasons = e.rejection_reasons.clone();
        if let Some(r) = early {
            self.reasons.insert(0, r);
        }
        let tightest = self.joints.first().and_then(|j| j.percent_of_limit.map(|p| format!("{} at {p:.0}% of motor speed", j.joint)));
        match e.eligible_speed_m_s {
            Some(v) => {
                self.status = "passed".into();
                self.speed_m_s = Some(v);
                self.summary = format!(
                    "Passed: {v:.3} m/s ({:.3} m in {:.1} s). Tightest joint: {}.",
                    e.forward_displacement_m,
                    e.observed_s,
                    tightest.unwrap_or_else(|| "unknown".into())
                );
            }
            None => {
                self.status = "rejected".into();
                self.summary = format!("Rejected after simulation: {}.", self.reasons.join("; "));
            }
        }
    }
    pub fn to_yaml(&self) -> R<String> {
        serde_norway::to_string(self).map_err(|e| e.to_string())
    }
}

/// Parse a gait file (YAML, or JSON, which is valid YAML).
pub fn read_script(path: &Path) -> R<GaitScript> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_norway::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// A gait file with a header comment, ready to edit.
pub fn write_script(script: &GaitScript, header: &str) -> R<String> {
    let body = serde_norway::to_string(script).map_err(|e| e.to_string())?;
    let comment: String = header.lines().map(|l| format!("# {l}\n")).collect();
    Ok(format!("{comment}{body}"))
}

/// One line per evaluation, appended to `out/journal.jsonl`.
pub fn journal(out: &Path, report: &GaitReport) -> R<()> {
    use std::io::Write as _;
    let line = serde_json::json!({
        "unix_s": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        "gait": report.gait, "file": report.file, "status": report.status, "speed_m_s": report.speed_m_s,
        "summary": report.summary, "results": report.results_directory, "cached": report.cached,
    });
    let mut f = fs::OpenOptions::new().create(true).append(true).open(out.join("journal.jsonl")).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}
