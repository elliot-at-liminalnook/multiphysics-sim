//! Acceptance tests: run a physical model headless for a stated time with a
//! commanded joint trajectory, then judge each stated criterion pass, fail
//! or not assessed. A criterion the run cannot assess (no stall torque, a
//! rigid link with no stress analysis, an unloaded bearing) is never a pass.
//!
//! The verdict is `passed` only when every criterion passed; `failed` when
//! any failed; otherwise `incomplete`. A passed run counts as evidence only
//! when the model lists no blocking assumption (`source.assumptions`, the
//! CAD export's); its other assumptions travel with the report.
//!
//! The test (trajectory and criteria) is a task condition, not part of the
//! robot: it lives in the project file, and the model's own `control`
//! block is replaced by the commanded trajectory for the run (recorded in
//! the report as `control_override`).
use crate::physical::{BuildOptions, PhysicalRobot};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_robot::model::PhysicalModel;
use std::collections::BTreeMap;

/// How often the commanded trajectory is applied and the run sampled (s).
pub const SAMPLE_S: f64 = 0.01;
pub const VERDICT_RULE: &str = "passed only when every criterion passed; failed when any failed; otherwise incomplete (a criterion that could not be assessed is never a pass). Evidence additionally needs a model with no blocking assumption.";

/// One commanded point: joint targets (rad, or m on a prismatic joint) at `t`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Waypoint {
    pub t: f64,
    pub targets: BTreeMap<String, f64>,
}

/// What must hold.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Criterion {
    /// `joint` is within `tolerance` of `target` from `by_s` to the end.
    Reaches { joint: String, target: f64, tolerance: f64, by_s: f64 },
    /// `joint` follows its command within `max_error` after `after_s`.
    Tracking { joint: String, max_error: f64, after_s: f64 },
    /// Every motor that drives a joint keeps peak torque below
    /// (1 − `min`) × its stall torque.
    TorqueMargin { min: f64 },
    /// Every motor's winding peaks at least `margin_c` below its rated maximum.
    WindingTemperature { margin_c: f64 },
    /// Every loaded joint bearing's pressure margin is at least `min`.
    BearingMargin { min: f64 },
    /// Every link's stress margin to yield is at least `min` (needs stress:
    /// flexible links; a rigid model cannot assess it).
    YieldMargin { min: f64 },
    /// Every printed part's layer-aware safety factor under the run's peak
    /// loads is at least `min_safety_factor` (`part_strength`; judged after
    /// the run from the CAD's part meshes: a bare model cannot assess it).
    PartStrength { min_safety_factor: f64 },
    /// A free-standing robot stays upright.
    NoFall,
    /// No joint runs past its travel limits.
    NoLimitHits,
}
impl Criterion {
    /// A plain sentence for people.
    pub fn describe(&self) -> String {
        match self {
            Criterion::Reaches { joint, target, tolerance, by_s } => format!("{joint} reaches {:.1}° (±{:.1}°) by {by_s} s and stays there", target.to_degrees(), tolerance.to_degrees()),
            Criterion::Tracking { joint, max_error, after_s } => format!("{joint} follows its command within {:.1}° after {after_s} s", max_error.to_degrees()),
            Criterion::TorqueMargin { min } => format!("every motor keeps at least {:.0} % of its stall torque in reserve", min * 100.0),
            Criterion::WindingTemperature { margin_c } => format!("every motor winding stays at least {margin_c} °C below its rated maximum"),
            Criterion::BearingMargin { min } => format!("every joint bearing keeps a pressure margin of at least {min}"),
            Criterion::YieldMargin { min } => format!("every part keeps a stress margin to yield of at least {min}"),
            Criterion::PartStrength { min_safety_factor } => format!("every printed part keeps a safety factor of at least {min_safety_factor} under the run's peak loads"),
            Criterion::NoFall => "the robot stays upright".into(),
            Criterion::NoLimitHits => "no joint runs past its travel limits".into(),
        }
    }
}

/// A test: how long, what is commanded, what must hold.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Test {
    pub name: String,
    pub duration_s: f64,
    /// Linear between waypoints, held after the last (before the first: the model's assembly pose targets).
    #[serde(default)]
    pub trajectory: Vec<Waypoint>,
    pub criteria: Vec<Criterion>,
}

/// pass | fail | not_assessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Fail,
    NotAssessed,
}

/// One criterion's judgement.
#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub criterion: Criterion,
    pub description: String,
    pub status: Status,
    /// The number the judgement is on (null when not assessed).
    pub measured: Value,
    /// Why, in a sentence (what was measured, or why it could not be).
    pub detail: String,
}

impl Test {
    /// The test's own errors (named), before a model is involved.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.duration_s.is_finite() && self.duration_s > 0.0 && self.duration_s <= 600.0) {
            return Err(format!("test `{}`: duration_s must be in (0, 600] s", self.name));
        }
        if self.criteria.is_empty() {
            return Err(format!("test `{}`: state at least one criterion (what must hold)", self.name));
        }
        let mut last = f64::NEG_INFINITY;
        for (i, w) in self.trajectory.iter().enumerate() {
            if !(w.t.is_finite() && w.t >= 0.0 && w.t > last) {
                return Err(format!("test `{}`: trajectory[{i}].t must be finite, nonnegative and increasing", self.name));
            }
            last = w.t;
            if let Some((j, _)) = w.targets.iter().find(|(_, v)| !v.is_finite()) {
                return Err(format!("test `{}`: trajectory[{i}].targets.{j} is not finite", self.name));
            }
        }
        for (i, c) in self.criteria.iter().enumerate() {
            let bad = match c {
                Criterion::Reaches { tolerance, by_s, target, .. } => !(tolerance.is_finite() && *tolerance > 0.0 && by_s.is_finite() && *by_s >= 0.0 && *by_s <= self.duration_s && target.is_finite()),
                Criterion::Tracking { max_error, after_s, .. } => !(max_error.is_finite() && *max_error > 0.0 && after_s.is_finite() && *after_s >= 0.0 && *after_s < self.duration_s),
                Criterion::TorqueMargin { min } => !(min.is_finite() && (0.0..1.0).contains(min)),
                Criterion::WindingTemperature { margin_c } => !(margin_c.is_finite() && *margin_c >= 0.0),
                Criterion::BearingMargin { min } | Criterion::YieldMargin { min } => !min.is_finite(),
                Criterion::PartStrength { min_safety_factor } => !(min_safety_factor.is_finite() && *min_safety_factor > 0.0),
                Criterion::NoFall | Criterion::NoLimitHits => false,
            };
            if bad {
                return Err(format!("test `{}`: criteria[{i}] ({}) has an out-of-range value", self.name, c.describe()));
            }
        }
        Ok(())
    }

    /// The commanded targets at `t` (linear interpolation, held after the last waypoint).
    pub fn targets_at(&self, t: f64) -> BTreeMap<String, f64> {
        let mut out = BTreeMap::new();
        let Some(first) = self.trajectory.first() else { return out };
        if t <= first.t {
            return first.targets.clone();
        }
        for w in self.trajectory.windows(2) {
            if t <= w[1].t {
                let f = (t - w[0].t) / (w[1].t - w[0].t);
                for (j, b) in &w[1].targets {
                    let a = w[0].targets.get(j).copied().unwrap_or(*b);
                    out.insert(j.clone(), a + f * (b - a));
                }
                for (j, a) in &w[0].targets {
                    out.entry(j.clone()).or_insert(*a);
                }
                return out;
            }
        }
        // Held after the last: every target any waypoint set keeps its latest value.
        for w in &self.trajectory {
            out.extend(w.targets.clone());
        }
        out
    }
}

/// The joints a test names (trajectory and criteria).
fn named_joints(test: &Test) -> Vec<&str> {
    let mut out: Vec<&str> = test.trajectory.iter().flat_map(|w| w.targets.keys().map(String::as_str)).collect();
    for c in &test.criteria {
        if let Criterion::Reaches { joint, .. } | Criterion::Tracking { joint, .. } = c {
            out.push(joint);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Run `model` (a parsed simrobot) through `test`; `cancelled` is polled
/// between samples and `progress` gets the fraction done. The report:
/// verdict, evidence, outcomes, the model's identity and assumptions, the
/// run's results (`PhysicalRobot::results`) and sampled traces.
pub fn run(model_json: &Value, test: &Test, cancelled: &dyn Fn() -> bool, progress: &dyn Fn(f64)) -> Result<Value, String> {
    test.validate()?;
    let mut model = PhysicalModel::parse(&model_json.to_string())?;
    // The commanded trajectory replaces the model's own control program.
    let control_override = json!({"from": {"mode": model.control.mode, "targets": model.control.targets}, "to": "the test's commanded trajectory"});
    model.control.mode = "hold".into();
    model.control.trajectory.clear();
    let mut robot = PhysicalRobot::build(model, &crate::registry(), &BuildOptions::default())?;
    let index: BTreeMap<String, usize> = robot.joint_names.iter().enumerate().map(|(i, n)| (n.trim_start_matches("joint.").trim_start_matches("slide.").to_string(), i)).collect();
    if let Some(j) = named_joints(test).into_iter().find(|j| !index.contains_key(*j)) {
        return Err(format!("test `{}` names joint `{j}`, which is not a driven joint of this model (its driven joints: {})", test.name, index.keys().cloned().collect::<Vec<_>>().join(", ")));
    }
    let steps = (test.duration_s / SAMPLE_S).round().max(1.0) as usize;
    let mut t_trace = Vec::with_capacity(steps + 1);
    let mut angles: BTreeMap<String, Vec<f64>> = index.keys().map(|k| (k.clone(), Vec::new())).collect();
    let mut commands: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut failure = None;
    let sample = |robot: &PhysicalRobot, t: f64, cmd: &BTreeMap<String, f64>, t_trace: &mut Vec<f64>, angles: &mut BTreeMap<String, Vec<f64>>, commands: &mut BTreeMap<String, Vec<f64>>| {
        t_trace.push(t);
        let q = robot.joint_angles();
        for (name, &i) in &index {
            angles.get_mut(name).expect("indexed").push(q.get(i).copied().unwrap_or(f64::NAN));
        }
        for (name, v) in cmd {
            commands.entry(name.clone()).or_default().push(*v);
        }
    };
    let cmd0 = test.targets_at(0.0);
    for (j, v) in &cmd0 {
        robot.set_target(index[j], *v);
    }
    sample(&robot, 0.0, &cmd0, &mut t_trace, &mut angles, &mut commands);
    for k in 1..=steps {
        if cancelled() {
            return Err("cancelled".into());
        }
        let t = k as f64 * SAMPLE_S;
        let cmd = test.targets_at(t);
        for (j, v) in &cmd {
            robot.set_target(index[j], *v);
        }
        if let Err(e) = robot.advance(SAMPLE_S) {
            failure = Some(format!("the simulation stopped at t = {:.2} s: {e}", t - SAMPLE_S));
            break;
        }
        sample(&robot, t, &cmd, &mut t_trace, &mut angles, &mut commands);
        if k % 10 == 0 {
            progress(k as f64 / steps as f64);
        }
    }
    let results = robot.results("acceptance");
    let outcomes: Vec<Outcome> = test.criteria.iter().map(|c| judge(c, test, &results, &t_trace, &angles, &commands, failure.as_deref())).collect();
    // Traces at most ~500 points.
    let stride = (t_trace.len() / 500).max(1);
    let thin = |v: &Vec<f64>| v.iter().step_by(stride).copied().collect::<Vec<f64>>();
    let assumptions = model_json["source"]["assumptions"].as_array().cloned().unwrap_or_default();
    let blocking = assumptions.iter().filter(|a| a["blocking"] == json!(true)).count();
    let motor_t: Vec<f64> = results["trace"]["t"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let motor_stride = (motor_t.len() / 500).max(1);
    let thin_value = |v: &Value| -> Value { json!(v.as_array().map(|a| a.iter().step_by(motor_stride).cloned().collect::<Vec<_>>()).unwrap_or_default()) };
    let mut report = json!({
        "test": test, "verdict_rule": VERDICT_RULE,
        "outcomes": outcomes,
        "failure": failure,
        "model": {"cad_revision": model_json["source"]["cad_revision"], "cad_sha256": model_json["source"]["cad_sha256"], "archive_identity": model_json["source"]["archive_identity"], "exporter": model_json["source"]["exporter"], "file": model_json["source"]["file"]},
        "assumptions": assumptions, "blocking_assumptions": blocking,
        "not_modelled": model_json["source"]["not_modelled"],
        "control_override": control_override,
        "sample_s": SAMPLE_S,
        "results": {"joints": results["joints"], "motors": results["motors"], "links": results["links"].as_object().map(|l| l.iter().map(|(k, v)| (k.clone(), json!({"peak_stress_pa": v["peak_stress_pa"], "yield_margin": v["yield_margin"], "max_deflection_m": v["max_deflection_m"]}))).collect::<serde_json::Map<_, _>>()), "base": {"fell": results["base"]["fell"]}, "warnings": results["warnings"], "wall_s": results["wall_s"], "steps": results["steps"], "duration_s": results["duration_s"]},
        "trace": {"t": thin(&t_trace), "joints": angles.iter().map(|(k, v)| (k.clone(), json!(thin(v)))).collect::<serde_json::Map<_, _>>(), "commands": commands.iter().map(|(k, v)| (k.clone(), json!(thin(v)))).collect::<serde_json::Map<_, _>>()},
        "motor_trace": {"t": motor_t.iter().step_by(motor_stride).copied().collect::<Vec<f64>>(), "torque_nm": results["trace"]["motors"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), thin_value(&v["torque_nm"]))).collect::<serde_json::Map<_, _>>())},
    });
    conclude(&mut report);
    Ok(report)
}

/// The report's verdict, evidence and summary from its outcomes, its
/// failure and its model's blocking assumptions ([`VERDICT_RULE`]); called
/// again by a caller that judges a criterion after the run (part strength).
pub fn conclude(report: &mut Value) {
    let outcomes = report["outcomes"].as_array().cloned().unwrap_or_default();
    let status = |o: &Value| o["status"].as_str().unwrap_or("not_assessed").to_string();
    let described = |want: &str| outcomes.iter().filter(|o| status(o) == want).filter_map(|o| o["description"].as_str().map(str::to_string)).collect::<Vec<_>>();
    let failure = report["failure"].as_str().map(str::to_string);
    let verdict = if !described("fail").is_empty() || failure.is_some() {
        "failed"
    } else if outcomes.iter().all(|o| status(o) == "pass") {
        "passed"
    } else {
        "incomplete"
    };
    let blocking = report["blocking_assumptions"].as_u64().unwrap_or(0);
    let evidence = verdict == "passed" && blocking == 0;
    let summary = match (verdict, evidence) {
        ("passed", true) => format!("Passed all {} criteria.", outcomes.len()),
        ("passed", false) => format!("Passed all {} criteria, but the model has {blocking} blocking assumption(s), so this is not evidence yet.", outcomes.len()),
        ("failed", _) => format!("Failed: {}.", described("fail").into_iter().chain(failure).collect::<Vec<_>>().join("; ")),
        _ => format!("Incomplete: {} could not be assessed.", described("not_assessed").join("; ")),
    };
    report["verdict"] = json!(verdict);
    report["evidence"] = json!(evidence);
    report["summary"] = json!(summary);
}

fn pass_if(ok: bool) -> Status {
    if ok { Status::Pass } else { Status::Fail }
}

/// One criterion against the run.
fn judge(c: &Criterion, test: &Test, results: &Value, t: &[f64], angles: &BTreeMap<String, Vec<f64>>, commands: &BTreeMap<String, Vec<f64>>, failure: Option<&str>) -> Outcome {
    let out = |status, measured: Value, detail: String| Outcome { criterion: c.clone(), description: c.describe(), status, measured, detail };
    let ended = t.last().copied().unwrap_or(0.0);
    let complete = failure.is_none() && ended + 1e-9 >= test.duration_s - SAMPLE_S;
    match c {
        Criterion::Reaches { joint, target, tolerance, by_s } => {
            let q = &angles[joint];
            let window: Vec<f64> = t.iter().zip(q).filter(|(ti, _)| **ti + 1e-9 >= *by_s).map(|(_, a)| (a - target).abs()).collect();
            if window.is_empty() || !complete {
                return out(Status::NotAssessed, Value::Null, format!("the run ended at {ended:.2} s, before the window from {by_s} s"));
            }
            let worst = window.iter().cloned().fold(0.0, f64::max);
            let final_angle = q.last().copied().unwrap_or(f64::NAN);
            out(pass_if(worst <= *tolerance), json!({"worst_error_rad": worst, "final_rad": final_angle}), format!("worst error after {by_s} s was {:.2}° (allowed {:.2}°); it ended at {:.1}°", worst.to_degrees(), tolerance.to_degrees(), final_angle.to_degrees()))
        }
        Criterion::Tracking { joint, max_error, after_s } => {
            let (Some(q), Some(cmd)) = (angles.get(joint), commands.get(joint)) else {
                return out(Status::NotAssessed, Value::Null, format!("{joint} has no commanded trajectory to follow"));
            };
            let errs: Vec<f64> = t.iter().zip(q.iter().zip(cmd)).filter(|(ti, _)| **ti + 1e-9 >= *after_s).map(|(_, (a, b))| (a - b).abs()).collect();
            if errs.is_empty() || !complete {
                return out(Status::NotAssessed, Value::Null, format!("the run ended at {ended:.2} s"));
            }
            let worst = errs.iter().cloned().fold(0.0, f64::max);
            out(pass_if(worst <= *max_error), json!({"worst_error_rad": worst}), format!("worst tracking error after {after_s} s was {:.2}° (allowed {:.2}°)", worst.to_degrees(), max_error.to_degrees()))
        }
        Criterion::TorqueMargin { min } => {
            let motors = results["motors"].as_object();
            let margins: Vec<(String, Option<f64>)> = motors.into_iter().flatten().map(|(k, m)| (k.clone(), m["stall_margin"].as_f64())).collect();
            if margins.is_empty() {
                return out(Status::NotAssessed, Value::Null, "the model has no motor driving a joint".into());
            }
            if let Some((name, _)) = margins.iter().find(|(_, m)| m.is_none()) {
                return out(Status::NotAssessed, Value::Null, format!("{name} has no stall torque to compare with"));
            }
            let (name, worst) = margins.iter().map(|(n, m)| (n.clone(), m.unwrap())).min_by(|a, b| a.1.total_cmp(&b.1)).expect("nonempty");
            let peak = results["motors"][&name]["peak_torque_nm"].as_f64().unwrap_or(f64::NAN);
            // When the peak happened, and the torque while holding at the end (the move's start often dominates).
            let times: Vec<f64> = results["trace"]["t"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            let torque: Vec<f64> = results["trace"]["motors"][&name]["torque_nm"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            let at = torque.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).and_then(|(i, _)| times.get(i).copied());
            let tail: Vec<f64> = times.iter().zip(&torque).filter(|(t, _)| **t >= ended - 0.25).map(|(_, x)| x.abs()).collect();
            let holding = (!tail.is_empty()).then(|| tail.iter().sum::<f64>() / tail.len() as f64);
            let when = match (at, holding) {
                (Some(at), Some(h)) => format!("; the peak came at t = {at:.2} s, and holding at the end took {h:.3} N·m"),
                (Some(at), None) => format!("; the peak came at t = {at:.2} s"),
                _ => String::new(),
            };
            out(pass_if(worst >= *min && complete), json!({"worst_margin": worst, "motor": name, "peak_torque_nm": peak, "peak_at_s": at, "holding_torque_nm": holding}), format!("{name} used {:.0} % of its stall torque (peak {:.3} N·m), leaving {:.0} % (needed {:.0} %){when}", (1.0 - worst) * 100.0, peak, worst * 100.0, min * 100.0))
        }
        Criterion::WindingTemperature { margin_c } => {
            let motors: Vec<(String, f64, f64)> = results["motors"].as_object().into_iter().flatten().filter_map(|(k, m)| Some((k.clone(), m["winding_margin_c"].as_f64()?, m["peak_winding_c"].as_f64()?))).collect();
            let Some((name, worst, peak)) = motors.iter().cloned().min_by(|a, b| a.1.total_cmp(&b.1)) else {
                return out(Status::NotAssessed, Value::Null, "the model has no motor with a thermal model".into());
            };
            out(pass_if(worst >= *margin_c && complete), json!({"worst_margin_c": worst, "motor": name, "peak_winding_c": peak}), format!("{name}'s winding peaked at {peak:.1} °C, {worst:.1} °C below its rating (needed {margin_c} °C) over {ended:.1} s"))
        }
        Criterion::BearingMargin { min } => {
            let js: Vec<(String, f64)> = results["joints"].as_object().into_iter().flatten().filter_map(|(k, j)| Some((k.clone(), j["bearing_margin"].as_f64()?))).collect();
            let Some((name, worst)) = js.into_iter().min_by(|a, b| a.1.total_cmp(&b.1)) else {
                return out(Status::NotAssessed, Value::Null, "no joint bearing carried load".into());
            };
            out(pass_if(worst >= *min && complete), json!({"worst_margin": worst, "joint": name}), format!("{name}'s bearing margin was {worst:.2} (needed {min}); the bearing itself is estimated unless set in CAD"))
        }
        Criterion::YieldMargin { min } => {
            let ls: Vec<(String, f64)> = results["links"].as_object().into_iter().flatten().filter_map(|(k, l)| Some((k.clone(), l["yield_margin"].as_f64()?))).collect();
            let Some((name, worst)) = ls.into_iter().min_by(|a, b| a.1.total_cmp(&b.1)) else {
                return out(Status::NotAssessed, Value::Null, "no part was stress-analysed: the model's links are rigid (flexible-link export is not ported), so part strength is not assessed".into());
            };
            out(pass_if(worst >= *min && complete), json!({"worst_margin": worst, "link": name}), format!("{name}'s stress margin to yield was {worst:.2} (needed {min})"))
        }
        Criterion::PartStrength { .. } => out(Status::NotAssessed, Value::Null, "printed-part strength is checked after the run against the CAD's part meshes (a project test does this); this run had none".into()),
        Criterion::NoFall => {
            let fell = results["base"]["fell"].as_bool().unwrap_or(false);
            out(pass_if(!fell && complete), json!({"fell": fell}), if fell { "the robot tipped over".into() } else { format!("upright for {ended:.1} s") })
        }
        Criterion::NoLimitHits => {
            let hits: Vec<(String, u64)> = results["joints"].as_object().into_iter().flatten().filter_map(|(k, j)| Some((k.clone(), j["limit_hits"].as_u64()?))).filter(|(_, h)| *h > 0).collect();
            out(pass_if(hits.is_empty() && complete), json!({"limit_hits": hits.iter().map(|(k, h)| (k.clone(), json!(h))).collect::<serde_json::Map<_, _>>()}), if hits.is_empty() { "no joint passed its limits".into() } else { format!("past limits: {}", hits.iter().map(|(k, h)| format!("{k} ({h} samples)")).collect::<Vec<_>>().join(", ")) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test() -> Test {
        serde_json::from_value(json!({
            "name": "lift", "duration_s": 3.0,
            "trajectory": [{"t": 0.0, "targets": {"shoulder": 0.0}}, {"t": 1.0, "targets": {"shoulder": 1.0}}],
            "criteria": [{"kind": "reaches", "joint": "shoulder", "target": 1.0, "tolerance": 0.05, "by_s": 2.0}, {"kind": "torque_margin", "min": 0.3}, {"kind": "yield_margin", "min": 1.0}],
        })).unwrap()
    }

    #[test]
    fn targets_interpolate_and_hold_and_tests_validate_by_name() {
        let t = test();
        t.validate().unwrap();
        assert_eq!(t.targets_at(0.5)["shoulder"], 0.5);
        assert_eq!(t.targets_at(2.5)["shoulder"], 1.0);
        let mut bad = t.clone();
        bad.criteria.clear();
        assert!(bad.validate().unwrap_err().contains("at least one criterion"));
        let mut bad = t.clone();
        bad.trajectory[1].t = 0.0;
        assert!(bad.validate().unwrap_err().contains("trajectory[1].t"));
        assert!(serde_json::from_value::<Criterion>(json!({"kind": "reaches", "joint": "a"})).is_err());
    }

    #[test]
    fn an_unassessed_margin_is_never_a_pass() {
        let t = test();
        let results = json!({"motors": {"servo": {"stall_margin": null, "peak_torque_nm": 0.1}}, "links": {"arm": {"yield_margin": null}}, "joints": {}, "base": {"fell": false}});
        let times: Vec<f64> = (0..=300).map(|k| k as f64 * 0.01).collect();
        let angles = BTreeMap::from([("shoulder".to_string(), times.iter().map(|t| t.min(1.0)).collect::<Vec<f64>>())]);
        let commands = angles.clone();
        let o: Vec<Outcome> = t.criteria.iter().map(|c| judge(c, &t, &results, &times, &angles, &commands, None)).collect();
        assert_eq!(o[0].status, Status::Pass);
        assert_eq!(o[1].status, Status::NotAssessed);
        assert_eq!(o[2].status, Status::NotAssessed);
        assert!(o[2].detail.contains("rigid"));
        // A run that stopped early cannot pass a reach.
        let short: Vec<f64> = times[..100].to_vec();
        let a = BTreeMap::from([("shoulder".to_string(), short.clone())]);
        assert_eq!(judge(&t.criteria[0], &t, &results, &short, &a, &a, Some("stopped")).status, Status::NotAssessed);
    }
}
