//! Reference CAD kinematics and motion patterns (RoboCAD's `pose.py`,
//! `motion.py`, `motion_service.py` and `motion_continuation.py`):
//! non-destructive forward kinematics in CAD world coordinates (mm,
//! radians), ideal transmissions, motor-driven closed loops solved for
//! closure, and declarative keyframed joint motion. Display only: no
//! physics, loads or collision checks, and fallback ranges are not travel
//! stops.
use crate::archive::ArchiveDocument;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::PI;

const MOVABLE: [&str; 3] = ["revolute", "continuous", "prismatic"];

type M4 = [[f64; 4]; 4];

fn eye() -> M4 {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

fn mul(a: &M4, b: &M4) -> M4 {
    let mut c = [[0.0; 4]; 4];
    for (i, row) in c.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    c
}

fn apply(m: &M4, p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
}

fn rotate(m: &M4, d: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| m[i][0] * d[0] + m[i][1] * d[1] + m[i][2] * d[2])
}

fn v3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}

/// One joint as the manifest stores it.
#[derive(Clone, Debug)]
pub struct Joint {
    pub kind: String,
    pub parent: Option<String>,
    pub child: String,
    pub pivot: [f64; 3],
    pub axis: [f64; 3],
    pub lower: Option<f64>,
    pub upper: Option<f64>,
    pub home: f64,
    pub stroke: f64,
    pub motor: Option<String>,
}

impl Joint {
    fn of(v: &Value) -> Joint {
        Joint {
            kind: v["type"].as_str().unwrap_or("fixed").to_string(),
            parent: v["parent"].as_str().map(str::to_string),
            child: v["child"].as_str().unwrap_or("").to_string(),
            pivot: v3(&v["pivot"]).unwrap_or([0.0; 3]),
            axis: v3(&v["axis"]).unwrap_or([0.0, 0.0, 1.0]),
            lower: v["lower"].as_f64(),
            upper: v["upper"].as_f64(),
            home: v["home"].as_f64().unwrap_or(0.0),
            stroke: v["stroke"].as_f64().unwrap_or(0.0),
            motor: v["motor"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
        }
    }
}

/// The interaction envelope of a joint (its limits, else ±π or ±stroke about home).
pub fn joint_range(j: &Joint) -> Result<(f64, f64), String> {
    let span = if j.kind == "prismatic" { if j.stroke != 0.0 { j.stroke } else { 100.0 } } else { PI };
    let lower = j.lower.unwrap_or(j.home - span);
    let upper = j.upper.unwrap_or(j.home + span);
    if !(lower.is_finite() && upper.is_finite() && j.home.is_finite()) || lower > upper {
        return Err("Joint limits must be finite and ordered".into());
    }
    Ok((lower, upper))
}

/// The joint's motion from home to `value` (world frame).
pub fn joint_motion(j: &Joint, value: f64) -> Result<M4, String> {
    let delta = value - j.home;
    let mut m = eye();
    if j.kind == "fixed" {
        return Ok(m);
    }
    let l = (j.axis[0].powi(2) + j.axis[1].powi(2) + j.axis[2].powi(2)).sqrt();
    if !l.is_finite() || l < 1e-12 {
        return Err("Joint axis must be nonzero and finite".into());
    }
    let [x, y, z] = j.axis.map(|v| v / l);
    if j.kind == "prismatic" {
        m[0][3] = x * delta;
        m[1][3] = y * delta;
        m[2][3] = z * delta;
        return Ok(m);
    }
    let (s, c) = delta.sin_cos();
    let k = 1.0 - c;
    let r = [[c + k * x * x, k * x * y - s * z, k * x * z + s * y], [k * x * y + s * z, c + k * y * y, k * y * z - s * x], [k * x * z - s * y, k * y * z + s * x, c + k * z * z]];
    if !j.pivot.iter().all(|v| v.is_finite()) {
        return Err("Joint pivot must be finite".into());
    }
    for i in 0..3 {
        for jj in 0..3 {
            m[i][jj] = r[i][jj];
        }
        m[i][3] = j.pivot[i] - (0..3).map(|k| r[i][k] * j.pivot[k]).sum::<f64>();
    }
    Ok(m)
}

/// Tree transforms plus ideal transmissions and motor-driven closed hinges.
pub struct PoseModel {
    pub joints: BTreeMap<String, Joint>,
    pub names: BTreeMap<String, String>,
    nodes: Vec<String>,
    /// child → (parent node, joint) (joint None: a mounted part).
    pub parents: HashMap<String, (Option<String>, Option<String>)>,
    pub loops: BTreeMap<String, Joint>,
    pub home: BTreeMap<String, f64>,
    /// driven → (driver, ratio).
    pub transmissions: BTreeMap<String, (String, f64)>,
    loop_variables: BTreeMap<String, Vec<String>>,
    pub passive: BTreeSet<String>,
    pub drivers: BTreeMap<String, f64>,
    pub last_positions: BTreeMap<String, f64>,
    pub last_error_mm: f64,
}

impl PoseModel {
    /// The model of `doc`'s joints (refused by name when it cannot be posed).
    pub fn new(doc: &ArchiveDocument) -> Result<PoseModel, String> {
        let nodes: Vec<Value> = doc.manifest["nodes"].as_array().cloned().unwrap_or_default().into_iter().map(|n| doc.node(n["id"].as_str().unwrap_or("")).cloned().unwrap_or(n)).collect();
        let ids: BTreeSet<String> = nodes.iter().filter_map(|n| n["id"].as_str().map(str::to_string)).collect();
        let mut m = PoseModel {
            joints: BTreeMap::new(),
            names: BTreeMap::new(),
            nodes: ids.iter().cloned().collect(),
            parents: HashMap::new(),
            loops: BTreeMap::new(),
            home: BTreeMap::new(),
            transmissions: BTreeMap::new(),
            loop_variables: BTreeMap::new(),
            passive: BTreeSet::new(),
            drivers: BTreeMap::new(),
            last_positions: BTreeMap::new(),
            last_error_mm: 0.0,
        };
        for n in &nodes {
            if !n["joint"].is_object() || n["disabled"] == true {
                continue;
            }
            let id = n["id"].as_str().unwrap_or("").to_string();
            let name = n["name"].as_str().unwrap_or(&id).to_string();
            let j = Joint::of(&n["joint"]);
            if !(MOVABLE.contains(&j.kind.as_str()) || matches!(j.kind.as_str(), "fixed" | "loop_revolute" | "loop_spherical")) {
                return Err(format!("{name}: unsupported preview joint {}", j.kind));
            }
            if !ids.contains(&j.child) || j.parent.as_ref().is_some_and(|p| !ids.contains(p)) {
                return Err(format!("{name}: a connected part is missing"));
            }
            m.names.insert(id.clone(), name);
            if j.kind.starts_with("loop_") {
                m.loops.insert(id.clone(), j.clone());
                m.joints.insert(id, j);
                continue;
            }
            if m.parents.contains_key(&j.child) {
                return Err("A part has multiple parent joints".into());
            }
            m.parents.insert(j.child.clone(), (j.parent.clone(), Some(id.clone())));
            if MOVABLE.contains(&j.kind.as_str()) {
                joint_range(&j)?;
                m.home.insert(id.clone(), j.home);
            }
            m.joints.insert(id, j);
        }
        for n in &nodes {
            let id = n["id"].as_str().unwrap_or("").to_string();
            if let Some(mount) = n["robot"]["mounted_on"].as_str()
                && !m.parents.contains_key(&id)
            {
                if !ids.contains(mount) {
                    return Err(format!("{}: mounted part is missing", n["name"].as_str().unwrap_or(&id)));
                }
                m.parents.insert(id, (Some(mount.to_string()), None));
            }
        }
        m.forward(&m.home.clone(), None)?;
        for n in &nodes {
            let ground = n["name"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("ground")) || n["robot"]["ground"] == true;
            if ground && m.ancestors(n["id"].as_str().unwrap_or("")).iter().any(|j| MOVABLE.contains(&m.joints[j].kind.as_str())) {
                return Err(format!("{}: a grounded part is connected below a moving joint", n["name"].as_str().unwrap_or("")));
            }
        }
        for t in doc.manifest["robot_settings"]["transmissions"].as_array().into_iter().flatten() {
            let (driver, driven) = (t["driver_joint"].as_str().unwrap_or(""), t["driven_joint"].as_str().unwrap_or(""));
            let ratio = t["ratio"].as_f64().unwrap_or(f64::NAN);
            if !m.home.contains_key(driver) || !m.home.contains_key(driven) || [driver, driven].iter().any(|i| m.joints[*i].kind == "prismatic") {
                return Err("Transmission requires two rotational joints".into());
            }
            if m.transmissions.contains_key(driven) || !ratio.is_finite() || ratio == 0.0 {
                return Err("Transmission must have one driver and a finite nonzero ratio".into());
            }
            m.transmissions.insert(driven.to_string(), (driver.to_string(), ratio));
        }
        m.coupled(m.home.clone())?;
        for (lid, j) in m.loops.clone() {
            let a: BTreeSet<String> = j.parent.as_deref().map(|p| m.ancestors(p)).unwrap_or_default().into_iter().collect();
            let b: BTreeSet<String> = m.ancestors(&j.child).into_iter().collect();
            let path: BTreeSet<String> = a.symmetric_difference(&b).cloned().collect();
            let moving: Vec<String> = path.into_iter().filter(|i| m.home.contains_key(i)).collect();
            if !moving.iter().any(|i| m.joints[i].motor.is_some()) {
                return Err("Closed-loop constraint solver needs a declared motor on the loop".into());
            }
            let vars: Vec<String> = moving.into_iter().filter(|i| m.joints[i].motor.is_none() && !m.transmissions.contains_key(i)).collect();
            m.loop_variables.insert(lid, vars);
        }
        m.passive = m.loop_variables.values().flatten().cloned().collect();
        m.drivers = m.home.iter().filter(|(i, _)| !m.passive.contains(*i) && !m.transmissions.contains_key(*i)).map(|(k, v)| (k.clone(), *v)).collect();
        m.last_positions = m.home.clone();
        Ok(m)
    }

    fn ancestors(&self, node: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut at = node.to_string();
        let mut guard = 0;
        while let Some((parent, joint)) = self.parents.get(&at) {
            if let Some(j) = joint {
                out.push(j.clone());
            }
            match parent {
                Some(p) => at = p.clone(),
                None => break,
            }
            guard += 1;
            if guard > 10_000 {
                break;
            }
        }
        out
    }

    fn coupled(&self, mut values: BTreeMap<String, f64>) -> Result<BTreeMap<String, f64>, String> {
        let mut done = BTreeSet::new();
        fn visit(m: &PoseModel, i: &str, values: &mut BTreeMap<String, f64>, visiting: &mut BTreeSet<String>, done: &mut BTreeSet<String>) -> Result<(), String> {
            if done.contains(i) || !m.transmissions.contains_key(i) {
                return Ok(());
            }
            if !visiting.insert(i.to_string()) {
                return Err("Transmission cycle".into());
            }
            let (driver, ratio) = m.transmissions[i].clone();
            visit(m, &driver, values, visiting, done)?;
            let v = m.home[i] + (values[&driver] - m.home[&driver]) / ratio;
            values.insert(i.to_string(), v);
            visiting.remove(i);
            done.insert(i.to_string());
            Ok(())
        }
        for i in self.transmissions.keys() {
            visit(self, i, &mut values, &mut BTreeSet::new(), &mut done)?;
        }
        Ok(values)
    }

    fn forward(&self, values: &BTreeMap<String, f64>, requested: Option<&BTreeSet<String>>) -> Result<HashMap<String, M4>, String> {
        let mut result: HashMap<String, M4> = HashMap::new();
        fn visit(m: &PoseModel, node: &str, values: &BTreeMap<String, f64>, result: &mut HashMap<String, M4>, visiting: &mut BTreeSet<String>) -> Result<M4, String> {
            if let Some(r) = result.get(node) {
                return Ok(*r);
            }
            if !visiting.insert(node.to_string()) {
                return Err("Joint or mounting cycle".into());
            }
            let (parent, joint) = m.parents.get(node).cloned().unwrap_or((None, None));
            let mut matrix = match &parent {
                Some(p) => visit(m, p, values, result, visiting)?,
                None => eye(),
            };
            if let Some(j) = joint {
                let joint = &m.joints[&j];
                if joint.kind != "fixed" {
                    matrix = mul(&matrix, &joint_motion(joint, values.get(&j).copied().unwrap_or(joint.home))?);
                }
            }
            visiting.remove(node);
            result.insert(node.to_string(), matrix);
            Ok(matrix)
        }
        let all: Vec<String> = match requested {
            Some(r) => r.iter().cloned().collect(),
            None => self.nodes.clone(),
        };
        for n in all {
            visit(self, &n, values, &mut result, &mut BTreeSet::new())?;
        }
        Ok(result)
    }

    fn residual(&self, values: &BTreeMap<String, f64>, lids: &[String]) -> Result<Vec<f64>, String> {
        let needed: BTreeSet<String> = lids.iter().flat_map(|l| [self.loops[l].parent.clone(), Some(self.loops[l].child.clone())]).flatten().collect();
        let mats = self.forward(values, Some(&needed))?;
        let mut out = Vec::new();
        for lid in lids {
            let j = &self.loops[lid];
            let l = (j.axis[0].powi(2) + j.axis[1].powi(2) + j.axis[2].powi(2)).sqrt().max(1e-12);
            let axis = j.axis.map(|v| v / l);
            let a = j.parent.as_ref().and_then(|p| mats.get(p)).copied().unwrap_or_else(eye);
            let b = mats[&j.child];
            let (pa, pb) = (apply(&a, j.pivot), apply(&b, j.pivot));
            out.extend((0..3).map(|i| pa[i] - pb[i]));
            if j.kind == "loop_revolute" {
                let (aa, ab) = (rotate(&a, axis), rotate(&b, axis));
                out.extend((0..3).map(|i| 100.0 * (aa[i] - ab[i])));
            }
        }
        Ok(out)
    }

    /// A joint's bounds (a passive slider: only declared limits).
    pub fn bounds(&self, jid: &str) -> Result<(f64, f64), String> {
        let j = &self.joints[jid];
        if self.passive.contains(jid) && j.kind == "prismatic" {
            return Ok((j.lower.unwrap_or(f64::NEG_INFINITY), j.upper.unwrap_or(f64::INFINITY)));
        }
        joint_range(j)
    }

    /// The world matrices of every node at `positions` (radians/mm), stepping
    /// large jumps so a linkage stays on its branch.
    pub fn matrices(&mut self, positions: &BTreeMap<String, f64>) -> Result<HashMap<String, M4>, String> {
        if self.loops.is_empty() {
            return self.solve(positions);
        }
        if positions.keys().any(|k| !self.home.contains_key(k)) {
            return Err("Pose refers to a missing or unsupported joint".into());
        }
        if positions.values().any(|v| !v.is_finite()) {
            return Err("Pose values must be finite".into());
        }
        let (before, error) = (self.last_positions.clone(), self.last_error_mm);
        let mut target = self.home.clone();
        target.extend(positions.iter().map(|(k, v)| (k.clone(), *v)));
        let steps = self.drivers.keys().map(|i| {
            let unit = if self.joints[i].kind == "prismatic" { 10.0 } else { 10f64.to_radians() };
            ((target[i] - before.get(i).copied().unwrap_or(self.home[i])).abs() / unit).ceil() as usize
        }).max().unwrap_or(1).max(1);
        if steps > 360 {
            return Err("Motion jump is too large; use intermediate keyframes".into());
        }
        let mut result = HashMap::new();
        for step in 1..=steps {
            let mut values = target.clone();
            for i in self.drivers.keys() {
                let b = before.get(i).copied().unwrap_or(self.home[i]);
                values.insert(i.clone(), b + (target[i] - b) * step as f64 / steps as f64);
            }
            match self.solve(&values) {
                Ok(r) => result = r,
                Err(e) => {
                    self.last_positions = before;
                    self.last_error_mm = error;
                    return Err(e);
                }
            }
        }
        Ok(result)
    }

    fn solve(&mut self, positions: &BTreeMap<String, f64>) -> Result<HashMap<String, M4>, String> {
        if positions.keys().any(|k| !self.home.contains_key(k)) {
            return Err("Pose refers to a missing or unsupported joint".into());
        }
        if positions.values().any(|v| !v.is_finite()) {
            return Err("Pose values must be finite".into());
        }
        let mut start = self.home.clone();
        start.extend(positions.iter().map(|(k, v)| (k.clone(), *v)));
        let mut values = self.coupled(start)?;
        // Supplied passive coordinates are only initial guesses; closure is authoritative.
        let at_home = self.drivers.keys().all(|i| (values[i] - self.home[i]).abs() < 1e-12);
        for i in self.passive.clone() {
            let v = if at_home { self.home[&i] } else { self.last_positions.get(&i).copied().unwrap_or(self.home[&i]) };
            values.insert(i, v);
        }
        for (lid, vars) in self.loop_variables.clone() {
            if vars.is_empty() || self.residual(&values, std::slice::from_ref(&lid))?.iter().fold(0.0f64, |a, b| a.max(b.abs())) < 1e-8 {
                continue;
            }
            let bounds: Vec<(f64, f64)> = vars.iter().map(|i| self.bounds(i)).collect::<Result<_, _>>()?;
            let margin: Vec<f64> = bounds.iter().map(|(lo, hi)| ((hi - lo) * 0.001).min(0.001)).collect();
            let clamp = |x: &[f64]| -> Vec<f64> { x.iter().zip(&bounds).zip(&margin).map(|((v, (lo, hi)), m)| v.clamp(lo + m, hi - m)).collect() };
            let eval = |x: &[f64]| -> Result<Vec<f64>, String> {
                let mut trial = values.clone();
                for (k, v) in vars.iter().zip(x) {
                    trial.insert(k.clone(), *v);
                }
                self.residual(&self.coupled(trial)?, std::slice::from_ref(&lid))
            };
            let x0 = clamp(&vars.iter().map(|i| values[i]).collect::<Vec<_>>());
            let mut fit = least_squares(&eval, x0, &bounds)?;
            if fit.1.iter().fold(0.0f64, |a, b| a.max(b.abs())) > 0.02 {
                // Scrubbing may jump across a linkage's dead centre: retry from the imported branch.
                let seed = clamp(&vars.iter().map(|i| self.home[i]).collect::<Vec<_>>());
                let retry = least_squares(&eval, seed, &bounds)?;
                if norm(&retry.1) < norm(&fit.1) {
                    fit = retry;
                }
            }
            for (k, v) in vars.iter().zip(fit.0) {
                values.insert(k.clone(), v);
            }
        }
        let lids: Vec<String> = self.loops.keys().cloned().collect();
        let error = if lids.is_empty() { 0.0 } else { self.residual(&values, &lids)?.iter().fold(0.0f64, |a, b| a.max(b.abs())) };
        if error > 0.02 {
            return Err(format!("Linkage cannot close at this pose ({error:.3} mm residual); playback stopped"));
        }
        for (jid, v) in &values {
            let (lo, hi) = self.bounds(jid)?;
            if !v.is_finite() || !(lo - 1e-9 <= *v && *v <= hi + 1e-9) {
                return Err("Pose exceeds joint preview bounds".into());
            }
        }
        let result = self.forward(&values, None)?;
        self.last_positions = values;
        self.last_error_mm = error;
        Ok(result)
    }

    /// Restore a caller's resolved pose (RoboCAD's `restore_prior`) after
    /// checking it: every movable joint, in bounds, consistent transmissions, closed loops.
    pub fn restore_prior(&mut self, positions: &BTreeMap<String, f64>) -> Result<(), String> {
        if positions.keys().collect::<BTreeSet<_>>() != self.home.keys().collect::<BTreeSet<_>>() {
            return Err("motion.prior.positions: require every resolved movable joint exactly once".into());
        }
        if positions.values().any(|v| !v.is_finite()) {
            return Err("motion.prior.positions: values must be finite radians/mm".into());
        }
        for (jid, v) in positions {
            let (lo, hi) = self.bounds(jid)?;
            if !(lo - 1e-9 <= *v && *v <= hi + 1e-9) {
                return Err(format!("motion.prior.positions.{jid}: exceeds reference bounds"));
            }
        }
        let coupled = self.coupled(positions.clone())?;
        for (jid, v) in positions {
            if (v - coupled[jid]).abs() > 1e-9 {
                return Err(format!("motion.prior.positions.{jid}: inconsistent reference transmission"));
            }
        }
        let lids: Vec<String> = self.loops.keys().cloned().collect();
        let residual = if lids.is_empty() { Vec::new() } else { self.residual(positions, &lids)? };
        if residual.iter().any(|v| !v.is_finite()) {
            return Err("motion.prior.positions: non-finite reference closure residual".into());
        }
        let error = residual.iter().fold(0.0f64, |a, b| a.max(b.abs()));
        if error > 0.02 {
            return Err(format!("motion.prior.positions: reference linkage does not close ({error:.3} mm residual)"));
        }
        self.last_positions = positions.clone();
        self.last_error_mm = error;
        Ok(())
    }

    /// Reference mechanism focus: driven and loop members and rigid attachments.
    pub fn focus_nodes(&self, jid: &str) -> Vec<String> {
        let mut ids: BTreeSet<String> = BTreeSet::from([self.joints[jid].child.clone()]);
        for (driven, (driver, _)) in &self.transmissions {
            if driver == jid {
                ids.insert(self.joints[driven].child.clone());
            }
        }
        for l in self.loops.values() {
            let a: BTreeSet<String> = l.parent.as_deref().map(|p| self.ancestors(p)).unwrap_or_default().into_iter().collect();
            let b: BTreeSet<String> = self.ancestors(&l.child).into_iter().collect();
            if a.symmetric_difference(&b).any(|x| x == jid) {
                ids.extend([l.parent.clone(), Some(l.child.clone())].into_iter().flatten());
            }
        }
        loop {
            let children: BTreeSet<String> = self.parents.iter().filter(|(_, (p, j))| p.as_ref().is_some_and(|p| ids.contains(p)) && j.as_ref().is_none_or(|j| self.joints[j].kind == "fixed")).map(|(c, _)| c.clone()).collect();
            if children.is_subset(&ids) {
                return ids.into_iter().collect();
            }
            ids.extend(children);
        }
    }
}

fn norm(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// A bounded Levenberg–Marquardt fit (forward-difference Jacobian, box
/// constraints by clamping): (x, residual).
fn least_squares(f: &dyn Fn(&[f64]) -> Result<Vec<f64>, String>, mut x: Vec<f64>, bounds: &[(f64, f64)]) -> Result<(Vec<f64>, Vec<f64>), String> {
    let clamp = |x: &mut Vec<f64>| {
        for (v, (lo, hi)) in x.iter_mut().zip(bounds) {
            *v = v.clamp(*lo, *hi);
        }
    };
    let n = x.len();
    let mut r = f(&x)?;
    let mut lambda = 1e-3;
    for _ in 0..60 {
        let cost = norm(&r);
        if cost < 1e-9 {
            break;
        }
        let mut jac = vec![vec![0.0; n]; r.len()];
        for k in 0..n {
            let h = 1e-6 * x[k].abs().max(1.0);
            let mut xp = x.clone();
            xp[k] += h;
            let rp = f(&xp)?;
            for (i, row) in jac.iter_mut().enumerate() {
                row[k] = (rp[i] - r[i]) / h;
            }
        }
        // (JᵀJ + λ diag) δ = −Jᵀr
        let mut a = vec![vec![0.0; n]; n];
        let mut g = vec![0.0; n];
        for (i, row) in jac.iter().enumerate() {
            for p in 0..n {
                g[p] -= row[p] * r[i];
                for q in 0..n {
                    a[p][q] += row[p] * row[q];
                }
            }
        }
        let mut improved = false;
        for _ in 0..12 {
            let mut m = a.clone();
            for p in 0..n {
                m[p][p] += lambda * m[p][p].max(1e-12);
            }
            let Some(delta) = solve(m, g.clone()) else {
                lambda *= 10.0;
                continue;
            };
            let mut trial: Vec<f64> = x.iter().zip(&delta).map(|(a, b)| a + b).collect();
            clamp(&mut trial);
            let rt = f(&trial)?;
            if norm(&rt) < cost {
                x = trial;
                r = rt;
                lambda = (lambda / 3.0).max(1e-12);
                improved = true;
                break;
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }
    Ok((x, r))
}

/// Gaussian elimination with partial pivoting.
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for c in 0..n {
        let p = (c..n).max_by(|i, j| a[*i][c].abs().total_cmp(&a[*j][c].abs()))?;
        if a[p][c].abs() < 1e-15 {
            return None;
        }
        a.swap(c, p);
        b.swap(c, p);
        for r in c + 1..n {
            let f = a[r][c] / a[c][c];
            for k in c..n {
                a[r][k] -= f * a[c][k];
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        x[r] = (b[r] - (r + 1..n).map(|k| a[r][k] * x[k]).sum::<f64>()) / a[r][r];
    }
    Some(x)
}

// ---- motion patterns ------------------------------------------------------

/// A checked motion pattern (`{name, duration, loop, tracks: [{joint, unit, keys}]}`):
/// tracks name driver joints (by id, or by a unique name), keys run 0…duration.
pub fn validate_program(model: &PoseModel, program: &Value) -> Result<Value, String> {
    let o = program.as_object().filter(|o| o.keys().all(|k| matches!(k.as_str(), "name" | "duration" | "loop" | "tracks"))).ok_or("Motion needs name, duration, loop and tracks")?;
    let mut p = program.clone();
    if o.get("name").and_then(Value::as_str).is_none_or(|n| n.trim().is_empty()) {
        return Err("Name the motion pattern".into());
    }
    let duration = o.get("duration").and_then(Value::as_f64).filter(|d| d.is_finite() && (0.05..=600.0).contains(d)).ok_or("Duration must be 0.05–600 seconds")?;
    let looping = match o.get("loop") {
        None => false,
        Some(v) => v.as_bool().ok_or("loop must be boolean")?,
    };
    let tracks = p["tracks"].as_array_mut().filter(|t| (1..=100).contains(&t.len())).ok_or("Supply 1–100 motion tracks")?;
    let mut seen = BTreeSet::new();
    for t in tracks.iter_mut() {
        let m = t.as_object().filter(|m| m.keys().all(|k| matches!(k.as_str(), "joint" | "unit" | "keys"))).ok_or("Track needs joint, unit and keys")?;
        let given = m.get("joint").and_then(Value::as_str).unwrap_or("").to_string();
        let jid = if model.joints.contains_key(&given) {
            given
        } else {
            let matches: Vec<&String> = model.names.iter().filter(|(_, n)| **n == given).map(|(i, _)| i).collect();
            if matches.len() != 1 {
                return Err(format!("Unknown or ambiguous joint: {given}"));
            }
            matches[0].clone()
        };
        if !model.drivers.contains_key(&jid) {
            return Err("Animate a driver joint; coupled and passive joints follow automatically".into());
        }
        if !seen.insert(jid.clone()) {
            return Err("Duplicate joint track".into());
        }
        let linear = model.joints[&jid].kind == "prismatic";
        let unit = m.get("unit").and_then(Value::as_str).unwrap_or("");
        if (linear && unit != "mm") || (!linear && !matches!(unit, "deg" | "rad")) {
            return Err("Use mm for sliders, deg or rad for rotation".into());
        }
        let keys: Vec<[f64; 2]> = m.get("keys").and_then(Value::as_array).filter(|k| (2..=1000).contains(&k.len())).ok_or("Each track needs 2–1000 [seconds, value] keys")?
            .iter()
            .map(|k| k.as_array().filter(|k| k.len() == 2).and_then(|k| Some([k[0].as_f64()?, k[1].as_f64()?])).filter(|k| k.iter().all(|v| v.is_finite())).ok_or("Keyframes must contain finite time and value"))
            .collect::<Result<_, _>>()?;
        if keys[0][0] != 0.0 || keys[keys.len() - 1][0] != duration || keys.windows(2).any(|w| w[0][0] >= w[1][0]) {
            return Err("Key times must increase from 0 to duration".into());
        }
        if looping && (keys[0][1] - keys[keys.len() - 1][1]).abs() > 1e-9 {
            return Err("A looping pattern must return to its starting value".into());
        }
        let (lo, hi) = joint_range(&model.joints[&jid])?;
        let factor = if unit == "deg" { PI / 180.0 } else { 1.0 };
        if keys.iter().any(|k| !(lo - 1e-9 <= k[1] * factor && k[1] * factor <= hi + 1e-9)) {
            return Err("A keyframe exceeds joint preview bounds".into());
        }
        t["joint"] = json!(jid);
    }
    Ok(p)
}

/// Joint values (radians/mm) at `seconds` (cosine-eased between keys).
pub fn sample_program(program: &Value, seconds: f64) -> Result<BTreeMap<String, f64>, String> {
    if !seconds.is_finite() {
        return Err("Time must be finite".into());
    }
    let t = seconds.clamp(0.0, program["duration"].as_f64().unwrap_or(0.0));
    let mut out = BTreeMap::new();
    for track in program["tracks"].as_array().into_iter().flatten() {
        let keys: Vec<[f64; 2]> = track["keys"].as_array().into_iter().flatten().filter_map(|k| Some([k[0].as_f64()?, k[1].as_f64()?])).collect();
        let mut value = keys.last().map_or(0.0, |k| k[1]);
        for w in keys.windows(2) {
            if t <= w[1][0] {
                let f = (1.0 - (PI * (t - w[0][0]) / (w[1][0] - w[0][0])).cos()) / 2.0;
                value = w[0][1] + (w[1][1] - w[0][1]) * f;
                break;
            }
        }
        let v = if track["unit"] == "deg" { value.to_radians() } else { value };
        out.insert(track["joint"].as_str().unwrap_or("").to_string(), v);
    }
    Ok(out)
}

/// A looping sweep of one driver joint about home.
pub fn sweep_program(model: &PoseModel, jid: &str, duration: f64) -> Result<Value, String> {
    let j = model.joints.get(jid).ok_or("Choose a driver joint")?;
    let linear = j.kind == "prismatic";
    let factor = if linear { 1.0 } else { 180.0 / PI };
    let (lo, hi) = joint_range(j)?;
    let home = j.home * factor;
    let amplitude = if linear { 10.0 } else { 15.0 };
    Ok(json!({"name": format!("{} sweep", model.names[jid]), "duration": duration, "loop": true,
        "tracks": [{"joint": jid, "unit": if linear { "mm" } else { "deg" }, "keys": [[0.0, home], [duration / 4.0, (hi * factor).min(home + amplitude)], [duration * 3.0 / 4.0, (lo * factor).max(home - amplitude)], [duration, home]]}]}))
}

/// The pose source's identity (a live kinematic preview of document `id` at `revision`).
pub fn identity(document_id: &str, revision: u64) -> Value {
    json!({"document_id": document_id, "revision": revision, "source_kind": "live_kinematic", "source_id": document_id, "physical_hash": null, "archive_hash": null})
}

/// The pose panel's metadata: movable joints with ranges, drivers, focus sets.
pub fn metadata(doc: &ArchiveDocument, revision: u64) -> Result<Value, String> {
    let model = PoseModel::new(doc)?;
    let id = doc.manifest["document_id"].as_str().unwrap_or("");
    let focus: Map<String, Value> = model.drivers.keys().map(|j| (j.clone(), json!(model.focus_nodes(j)))).collect();
    let joints: Vec<Value> = model.joints.iter().filter(|(j, _)| model.home.contains_key(*j)).map(|(jid, j)| {
        let (lo, hi) = joint_range(j).unwrap_or((j.home, j.home));
        json!({"id": jid, "name": model.names[jid], "unit": if j.kind == "prismatic" { "mm" } else { "rad" }, "home": j.home, "lower": j.lower, "upper": j.upper,
               "display_lower": lo, "display_upper": hi, "driver": model.drivers.contains_key(jid), "pivot": j.pivot, "axis": j.axis, "child": j.child, "parent": j.parent})
    }).collect();
    Ok(json!({"identity": identity(id, revision), "focus_ids": focus, "joints": joints,
        "assumptions": ["Kinematic preview; no physics, loads or collision checks", "Ideal transmissions; fallback display ranges are not physical travel stops"]}))
}

/// A program by name from `robot_settings.motion_programs`, or as given; checked.
pub fn resolve_program(doc: &ArchiveDocument, model: &PoseModel, program: &Value) -> Result<Value, String> {
    let p = match program.as_str() {
        Some(name) => doc.manifest["robot_settings"]["motion_programs"].get(name).cloned().ok_or("Motion pattern not found")?,
        None => program.clone(),
    };
    validate_program(model, &p)
}

/// One pose sample (RoboCAD's `motion_service.sample`): `request` is
/// `{positions, program, time, prior}`.
pub fn sample(doc: &ArchiveDocument, revision: u64, request: &Value) -> Result<Value, String> {
    let mut model = PoseModel::new(doc)?;
    let id = doc.manifest["document_id"].as_str().unwrap_or("").to_string();
    let ident = identity(&id, revision);
    let prior = request.get("prior").filter(|p| !p.is_null());
    if let Some(prior) = prior {
        if prior["identity"] != ident {
            return Err("motion.prior.identity: document, revision or kinematic source changed; reset continuation".into());
        }
        let positions: BTreeMap<String, f64> = prior["positions"].as_object().ok_or("motion.prior: require identity and full resolved positions")?.iter().map(|(k, v)| v.as_f64().map(|v| (k.clone(), v)).ok_or("motion.prior.positions: values must be finite radians/mm")).collect::<Result<_, _>>()?;
        model.restore_prior(&positions)?;
    }
    let seconds = request["time"].as_f64().unwrap_or(0.0);
    if !seconds.is_finite() {
        return Err("Time must be finite".into());
    }
    let mut program = request.get("program").filter(|p| !p.is_null()).cloned();
    let positions: BTreeMap<String, f64> = match &program {
        Some(p) => {
            let p = resolve_program(doc, &model, p)?;
            let values = sample_program(&p, seconds)?;
            program = Some(p);
            values
        }
        None => request["positions"].as_object().cloned().unwrap_or_default().into_iter().map(|(k, v)| v.as_f64().filter(|x| x.is_finite()).map(|v| (k, v)).ok_or("Positions must be finite radians/mm")).collect::<Result<_, _>>()?,
    };
    if positions.keys().any(|j| !model.drivers.contains_key(j)) {
        return Err("Only driver joints can be requested".into());
    }
    let mut all = model.home.clone();
    all.extend(positions);
    let mats = model.matrices(&all)?;
    let matrices: Map<String, Value> = mats.into_iter().map(|(k, m)| (k, json!(m))).collect();
    Ok(json!({"identity": ident, "time": seconds, "program": program, "prior_applied": prior.is_some(),
        "matrices": matrices, "positions": model.last_positions, "closure_error_mm": model.last_error_mm}))
}

/// A sweep program for driver `joint`, checked.
pub fn sweep(doc: &ArchiveDocument, joint: &str) -> Result<Value, String> {
    let model = PoseModel::new(doc)?;
    if !model.drivers.contains_key(joint) {
        return Err("Choose a driver joint".into());
    }
    validate_program(&model, &sweep_program(&model, joint, 4.0)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_revolute_motion_turns_about_its_pivot() {
        let j = Joint { kind: "revolute".into(), parent: None, child: "c".into(), pivot: [1.0, 0.0, 0.0], axis: [0.0, 0.0, 1.0], lower: None, upper: None, home: 0.0, stroke: 0.0, motor: None };
        let m = joint_motion(&j, PI / 2.0).unwrap();
        let p = apply(&m, [2.0, 0.0, 0.0]);
        assert!((p[0] - 1.0).abs() < 1e-12 && (p[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn programs_ease_between_keys() {
        let p = json!({"name": "x", "duration": 2.0, "loop": false, "tracks": [{"joint": "j", "unit": "deg", "keys": [[0.0, 0.0], [2.0, 90.0]]}]});
        let v = sample_program(&p, 1.0).unwrap();
        assert!((v["j"] - 45f64.to_radians()).abs() < 1e-12);
    }

    #[test]
    fn least_squares_fits_a_circle_point() {
        let f = |x: &[f64]| -> Result<Vec<f64>, String> { Ok(vec![x[0].cos() - 0.5, x[0].sin() - 0.75f64.sqrt()]) };
        let (x, r) = least_squares(&f, vec![0.2], &[(-PI, PI)]).unwrap();
        assert!((x[0] - PI / 3.0).abs() < 1e-6, "{x:?} {r:?}");
    }
}
