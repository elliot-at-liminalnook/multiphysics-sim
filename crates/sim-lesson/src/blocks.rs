//! Lesson blocks beyond scenes and questions:
//!
//! - `sim-equation`: an equation whose terms come from the model or the run,
//!   shown with its values filled in at the playhead.
//! - `sim-measured`: measured data beside the simulation of the same points.
//! - `sim-remedy`: a short correction for one misconception, shown only when
//!   the reader picks the option that names it.
//! - `sim-task`: a fault to find or a design to reach in the builder sandbox,
//!   judged by claims on the sandbox's run.
//! - `sim-lab`: a bounded test on the hardware bench (or the simulated rig),
//!   compared with the reader's prediction and the simulation.
//!
//! Parsing and validation only; running and judging is `sim_runtime::lesson`.
use crate::quiz::Tolerance;
use crate::{Expect, Reduce, RunSpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// One named quantity in an equation: from a parameter, an observable at the
/// playhead, or a fixed value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Term {
    /// How it is written (`k`, `τ`, `ω`).
    pub symbol: String,
    #[serde(default)]
    pub unit: String,
    /// `system/instance/path.parameter` (system optional with one system).
    #[serde(default)]
    pub param: Option<String>,
    /// An observable of the equation's scene, read at the playhead.
    #[serde(default)]
    pub observe: Option<String>,
    #[serde(default)]
    pub value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Holds {
    /// The observable the equation's result must match on the run.
    pub observe: String,
    #[serde(default)]
    pub window: Option<[f64; 2]>,
    /// Default 2 % of the largest magnitude in the window.
    #[serde(default)]
    pub tolerance: Option<Tolerance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Equation {
    pub id: String,
    /// The scene whose playhead supplies observed terms.
    #[serde(default)]
    pub scene: Option<String>,
    /// How the equation reads (`τ = k·i`); default: result = expression.
    #[serde(default)]
    pub show: Option<String>,
    /// The right-hand side over the term names (`k * i`).
    pub expr: String,
    pub result: Term,
    pub terms: BTreeMap<String, Term>,
    /// `sim-lesson check` confirms the equation on the run.
    #[serde(default)]
    pub holds: Option<Holds>,
    #[serde(default)]
    pub caption: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Axis {
    /// Field of each measured point.
    pub field: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasuredY {
    pub field: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub unit: String,
    /// What the simulation reports for each point.
    pub observe: String,
    #[serde(default = "mean")]
    pub reduce: Reduce,
    #[serde(default)]
    pub window: Option<[f64; 2]>,
}
fn mean() -> Reduce {
    Reduce::Mean
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measured {
    pub id: String,
    pub system: String,
    /// A `sim.fit-data/1` file, relative to the lesson folder.
    pub data: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub caption: String,
    pub x: Axis,
    pub y: MeasuredY,
    /// Parameters for each point: `instance/path.parameter: expression`
    /// over the point's fields and the data's conditions (`duty * supply_v`).
    pub set: BTreeMap<String, String>,
    /// Keep only points whose fields equal these values.
    #[serde(default)]
    pub only: BTreeMap<String, f64>,
    #[serde(default)]
    pub run: RunSpec,
    /// Claims for `check`: the largest RMS and single-point gap allowed, in
    /// the y unit.
    #[serde(default)]
    pub max_rms: Option<f64>,
    #[serde(default)]
    pub max_gap: Option<f64>,
}

/// Measured points and where they came from (`sim.fit-data/1`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeasuredData {
    pub description: String,
    pub source_path: String,
    pub source_hash: String,
    pub conditions: BTreeMap<String, f64>,
    pub points: Vec<BTreeMap<String, f64>>,
}

pub fn load_data(path: &Path) -> Result<MeasuredData, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if v["schema"].as_str() != Some("sim.fit-data/1") {
        return Err(format!("{}: expected schema sim.fit-data/1", path.display()));
    }
    let numbers = |o: &serde_json::Value| -> BTreeMap<String, f64> { o.as_object().map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|x| (k.clone(), x))).collect()).unwrap_or_default() };
    let points: Vec<BTreeMap<String, f64>> = v["points"].as_array().ok_or_else(|| format!("{}: no points", path.display()))?.iter().map(numbers).collect();
    Ok(MeasuredData {
        description: v["description"].as_str().unwrap_or_default().to_string(),
        source_path: v["source"]["path"].as_str().unwrap_or_default().to_string(),
        source_hash: v["source"]["sha256"].as_str().or(v["source"]["blake3"].as_str()).unwrap_or_default().to_string(),
        conditions: numbers(&v["conditions"]),
        points,
    })
}
impl MeasuredData {
    /// Points passing `only`, as (x, y, all values for `set` expressions).
    pub fn select(&self, m: &Measured) -> Vec<(f64, f64, BTreeMap<String, f64>)> {
        self.points
            .iter()
            .filter(|p| m.only.iter().all(|(k, v)| p.get(k).is_some_and(|x| (x - v).abs() < 1e-12)))
            .filter_map(|p| {
                let mut vars = self.conditions.clone();
                vars.extend(p.iter().map(|(k, v)| (k.clone(), *v)));
                Some((*p.get(&m.x.field)?, *p.get(&m.y.field)?, vars))
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remedy {
    pub id: String,
    /// The misconception, named plainly ("The motor tries harder").
    pub misconception: String,
    /// A short correction (Markdown).
    pub body: String,
    /// A scene that shows the right picture.
    #[serde(default)]
    pub scene: Option<String>,
    /// A follow-up question to try after reading.
    #[serde(default)]
    pub then: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// The sandbox starts broken; the reader finds and fixes the fault.
    Fault,
    /// The reader changes the sandbox until the goal holds.
    Design,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub label: String,
    pub observe: String,
    #[serde(default = "mean")]
    pub reduce: Reduce,
    #[serde(default)]
    pub window: Option<[f64; 2]>,
    #[serde(default)]
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub kind: TaskKind,
    /// The scene whose system, script and run the task uses.
    pub scene: String,
    #[serde(default)]
    pub title: String,
    /// What to achieve (Markdown). For a fault: the symptom.
    pub goal: String,
    /// Values the sandbox starts with (the planted fault, or the starting design).
    #[serde(default)]
    pub start: BTreeMap<String, f64>,
    /// All must hold on the sandbox's run.
    pub win: Vec<Expect>,
    /// Measured on every attempt, shown as a table (design trade-offs).
    #[serde(default)]
    pub report: Vec<Metric>,
    /// Values (on top of `start`) that meet the goal. Never shown to the
    /// reader; `sim-lesson check` proves the task is solvable with them and
    /// that `start` alone does not meet it.
    #[serde(default)]
    pub solution: BTreeMap<String, f64>,
    #[serde(default)]
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabTest {
    /// Open-loop duty, -1…1 (the rig enforces its own lower limits).
    pub duty: f64,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lab {
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// Joint role in the actuator registry (`knee`, `hip`).
    pub joint: String,
    pub test: LabTest,
    /// Asked before the run; the reader's number (with a unit) is compared
    /// with the bench, the registry and the model afterwards.
    #[serde(default)]
    pub predict: Option<String>,
    /// Unit of the lab's measured value and of the prediction.
    #[serde(default)]
    pub unit: String,
    /// A `sim-measured` block or scene to compare against.
    #[serde(default)]
    pub compare: Option<String>,
    #[serde(default)]
    pub notes: String,
}

/// The checks every hardware step shows before it may run.
pub const LAB_CHECKLIST: [&str; 4] = [
    "I am at the bench and can reach STOP.",
    "The leg is suspended on the fixture and nothing can be caught.",
    "Motor power is on only for this test and the supervisor is running.",
    "The travel window for this joint has been taught.",
];

pub(crate) fn validate_equation(e: &Equation) -> Result<(), String> {
    for (name, t) in e.terms.iter().chain(std::iter::once((&"result".to_string(), &e.result))) {
        let sources = t.param.is_some() as u8 + t.observe.is_some() as u8 + t.value.is_some() as u8;
        if name != "result" && sources != 1 {
            return Err(format!("term `{name}` needs exactly one of param, observe or value"));
        }
        if name == "result" && (t.param.is_some() || t.value.is_some()) {
            return Err("result is computed; it may name an `observe` to compare with, not a param or value".into());
        }
        if t.symbol.trim().is_empty() {
            return Err(format!("term `{name}` needs a symbol"));
        }
        if !t.unit.is_empty() {
            crate::units::parse_unit(&t.unit).map_err(|m| format!("term `{name}` unit: {m}"))?;
        }
    }
    let names: Vec<&str> = e.terms.keys().map(String::as_str).collect();
    sim_script::expr::check(&e.expr, &names)?;
    if e.terms.values().any(|t| t.observe.is_some()) && e.scene.is_none() {
        return Err("terms read observables: name the `scene` they come from".into());
    }
    if e.holds.is_some() && e.scene.is_none() {
        return Err("`holds` needs the `scene` to check on".into());
    }
    Ok(())
}

pub(crate) fn validate_measured(m: &Measured) -> Result<(), String> {
    if m.set.is_empty() {
        return Err("set: map each point to the system's parameters (e.g. `supply.voltage: duty * supply_v`)".into());
    }
    for (k, e) in &m.set {
        sim_script::presentation::split_parameter(k)?;
        if e.trim().is_empty() {
            return Err(format!("set {k}: empty expression"));
        }
    }
    if m.max_rms.is_some_and(|v| !(v >= 0.0)) || m.max_gap.is_some_and(|v| !(v >= 0.0)) {
        return Err("max_rms and max_gap must be ≥ 0".into());
    }
    Ok(())
}

pub(crate) fn validate_task(t: &Task) -> Result<(), String> {
    if t.win.is_empty() {
        return Err("win: list the claims that must hold when the task is done".into());
    }
    for k in t.start.keys().chain(t.solution.keys()) {
        sim_script::presentation::split_parameter(k)?;
    }
    for e in &t.win {
        if e.min.is_none() && e.max.is_none() {
            return Err(format!("win on {}: give min, max or both", e.observe));
        }
    }
    if t.goal.trim().is_empty() {
        return Err("goal is empty".into());
    }
    Ok(())
}

pub(crate) fn validate_lab(l: &Lab) -> Result<(), String> {
    if !(l.test.duty.is_finite() && l.test.duty.abs() <= 1.0) {
        return Err("test.duty must be within -1…1".into());
    }
    if !(l.test.seconds > 0.0 && l.test.seconds <= 5.0) {
        return Err("test.seconds must be in (0, 5]: a lab step is a short, bounded test".into());
    }
    if l.joint.trim().is_empty() {
        return Err("joint: name the joint role in the actuator registry (knee, hip)".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocks_validate_their_shape() {
        let e: Equation = serde_norway::from_str("id: e\nscene: s\nexpr: k * i\nresult: { symbol: τ, unit: N·m }\nterms:\n  k: { symbol: k, unit: N·m/A, param: motor.torque_constant }\n  i: { symbol: i, unit: A, observe: motor.p.current }").unwrap();
        validate_equation(&e).unwrap();
        let mut bad = e.clone();
        bad.expr = "k * j".into();
        assert!(validate_equation(&bad).unwrap_err().contains("`j`"));
        let mut bad = e.clone();
        bad.scene = None;
        assert!(validate_equation(&bad).unwrap_err().contains("scene"));
        let mut bad = e;
        bad.terms.get_mut("k").unwrap().value = Some(0.012);
        assert!(validate_equation(&bad).unwrap_err().contains("exactly one"));
        let t: Task = serde_norway::from_str("id: t\nkind: fault\nscene: s\ngoal: fix it\nstart: { gearbox/mesh.worm_starts: 4 }\nwin:\n  - { observe: drum.shaft.speed, reduce: mean, window: [1.6, 2.0], min: -0.01, max: 0.01 }").unwrap();
        validate_task(&t).unwrap();
        let l: Lab = serde_norway::from_str("id: l\njoint: knee\ntest: { duty: 0.3, seconds: 1.0 }").unwrap();
        validate_lab(&l).unwrap();
        let mut long = l;
        long.test.seconds = 30.0;
        assert!(validate_lab(&long).is_err());
    }
    #[test]
    fn measured_data_loads_and_selects() {
        let dir = std::env::temp_dir().join(format!("measured-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("d.json");
        std::fs::write(&path, r#"{"schema":"sim.fit-data/1","description":"d","source":{"path":"p","sha256":"h"},"conditions":{"supply_v":12.4},"points":[{"run":0,"duty":0.3,"speed":1.2},{"run":1,"duty":0.3,"speed":1.1}]}"#).unwrap();
        let d = load_data(&path).unwrap();
        let m: Measured = serde_norway::from_str("id: m\nsystem: knee\ndata: d.json\nx: { field: duty }\ny: { field: speed, observe: leg.speed }\nset: { supply.voltage: duty * supply_v }\nonly: { run: 0 }").unwrap();
        validate_measured(&m).unwrap();
        let pts = d.select(&m);
        assert_eq!(pts.len(), 1);
        assert_eq!((pts[0].0, pts[0].1), (0.3, 1.2));
        assert!((sim_script::expr::eval("duty * supply_v", &pts[0].2).unwrap() - 3.72).abs() < 1e-12);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
