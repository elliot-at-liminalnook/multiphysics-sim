//! Robot-mode graph history: bounded time histories sampled from the frames
//! the run thread publishes, one sample per applied frame of the current
//! generation. Presentation only: every value is a field of the frame (a held
//! input, a published link velocity, a servo target or joint angle); |v_xy| and
//! ω_z are a norm and a component of the published world-frame velocities.
//! Nothing is integrated or differentiated here. Drawn with `crate::chart`.
use crate::robot::motion::Motion;
use crate::robot::run::{Frame, short};
use serde_json::{Value, json};
use sim_domain_robot::PhysicalModel;
use std::collections::{BTreeMap, VecDeque};

/// Sim-time window kept per trace (s), as build mode's graph history.
pub const WINDOW_S: f64 = 20.0;
/// Cap on samples kept per trace.
pub const MAX_SAMPLES: usize = 2000;
pub const WORLD_FRAME: &str = "world frame, measured from frame poses";
pub const REQUEST_SOURCE: &str = "request: the session input held in the frame (Frame.inputs at the motion channel's index; replays hold the recorded action)";
pub const CHASSIS_RULE: &str = "chassis = the loaded model's root link as the shared articulation builds it (sim-domain-robot articulated.rs): a `ground` link is pinned (no moving chassis, so no measured traces); otherwise the one link that is no non-loop joint's child. When several links qualify the runtime picks the heaviest; the viewer refuses to plot rather than repeat that choice";
pub const SAMPLING_RULE: &str = "one sample per frame applied by RunController::poll whose generation equals the controller's; a sample at the same frame time replaces the previous one (a paused jog or motion request republishes the frame); cleared on Reset, replay start (both bump the generation) and any other generation change; each trace keeps the last WINDOW_S s of sim time, at most MAX_SAMPLES points";

/// Channels picked at once (the Picked chart's traces).
pub const MAX_PICKS: usize = 8;
pub const PICK_RULE: &str = "picked channels are numbers the published frames already carry: input:<name> (the held session input), target:<coordinate> and actual:<coordinate> (servo_targets_rad and joint_positions at the coordinate's joint index), reference:<coordinate> (reference_targets_rad), obs:<key> (the controller's policy.observations), learning:reward and learning:travel_m (the learning environment's transition), and link:<name>:speed (|velocity_m_s| of a link's published velocity); for --robot FILE, joint:<name>:target and joint:<name>:measured. Each is sampled from every applied frame of the current generation from the moment it is picked (same window and cap as the fixed charts); picks are kept across Reset, reload and preset changes (pinned) until removed; at most MAX_PICKS.";

/// A pickable channel: its key, a short label and its unit.
pub struct Candidate {
    pub key: String,
    pub label: String,
    pub unit: String,
}

/// The channels a frame carries (PICK_RULE), in a stable order.
pub fn candidates(frame: &Frame, drive: Option<&crate::robot::run::Drive>, links: &[String]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut push = |key: String, label: String, unit: &str| out.push(Candidate { key, label, unit: unit.into() });
    if let Some(d) = drive {
        for (i, c) in d.inputs.iter().enumerate() {
            if frame.inputs.get(i).is_some() {
                push(format!("input:{}", c.name), format!("input {}", c.name), c.kind.unit());
            }
        }
    }
    let x = frame.extra.as_deref();
    let names: Vec<String> = drive.and_then(|d| d.metadata["coordinate_names"].as_array()).map(|a| a.iter().map(|n| n.as_str().unwrap_or("").trim_start_matches("joint.").to_string()).collect()).unwrap_or_default();
    if let Some(targets) = x.and_then(|x| x["servo_targets_rad"].as_array()) {
        for i in 0..targets.len() {
            let n = names.get(i).cloned().unwrap_or_else(|| format!("coordinate {}", i + 1));
            push(format!("target:{n}"), format!("{n} target"), "rad");
            push(format!("actual:{n}"), format!("{n} actual"), "rad");
            if x.is_some_and(|x| x["reference_targets_rad"].get(i).is_some()) {
                push(format!("reference:{n}"), format!("{n} plan"), "rad");
            }
        }
    }
    if let Some(l) = x.map(|x| &x["learning"]).filter(|l| l.is_object()) {
        push("learning:reward".into(), "learning reward".into(), "score");
        if l["speed"].is_object() {
            push("learning:travel_m".into(), "net travel".into(), "m");
        }
    }
    for (i, name) in links.iter().enumerate() {
        if frame.velocities.get(i).is_some_and(Option::is_some) {
            push(format!("link:{name}:speed"), format!("{name} speed"), "m/s");
        }
    }
    for name in &frame.joint_names {
        let n = short(name);
        push(format!("joint:{n}:target"), format!("{n} target"), "");
        push(format!("joint:{n}:measured"), format!("{n} measured"), "");
    }
    if let Some(obs) = x.and_then(|x| x["policy"]["observations"].as_object()) {
        for (k, v) in obs {
            if v.is_number() {
                push(format!("obs:{k}"), format!("obs {k}"), "");
            }
        }
    }
    out
}

/// A picked channel's value in `frame` (PICK_RULE), when the frame carries it.
pub fn pick_value(frame: &Frame, drive: Option<&crate::robot::run::Drive>, links: &[String], key: &str) -> Option<f64> {
    let x = frame.extra.as_deref();
    let coordinate = |n: &str| -> Option<usize> {
        let names = drive?.metadata["coordinate_names"].as_array()?;
        names.iter().position(|c| c.as_str().is_some_and(|c| c.trim_start_matches("joint.") == n)).or_else(|| n.strip_prefix("coordinate ").and_then(|k| k.parse::<usize>().ok()).map(|k| k - 1))
    };
    if let Some(name) = key.strip_prefix("input:") {
        let i = drive?.inputs.iter().position(|c| c.name == name)?;
        return frame.inputs.get(i).copied();
    }
    if let Some(n) = key.strip_prefix("target:") {
        return x?["servo_targets_rad"].get(coordinate(n)?)?.as_f64();
    }
    if let Some(n) = key.strip_prefix("reference:") {
        return x?["reference_targets_rad"].get(coordinate(n)?)?.as_f64();
    }
    if let Some(n) = key.strip_prefix("actual:") {
        let i = coordinate(n)?;
        let j = drive?.metadata["joint_indices"].get(i)?.as_u64()? as usize;
        return x?["joint_positions"].get(j)?.as_f64();
    }
    if key == "learning:reward" {
        return x?["learning"]["reward"].as_f64();
    }
    if key == "learning:travel_m" {
        return x?["learning"]["speed"]["net_distance_m"].as_f64();
    }
    if let Some(k) = key.strip_prefix("obs:") {
        return x?["policy"]["observations"].get(k)?.as_f64();
    }
    if let Some(rest) = key.strip_prefix("link:").and_then(|r| r.strip_suffix(":speed")) {
        let i = links.iter().position(|l| l == rest)?;
        let (v, _) = (*frame.velocities.get(i)?)?;
        return Some((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt());
    }
    if let Some(rest) = key.strip_prefix("joint:") {
        let (n, which) = rest.rsplit_once(':')?;
        let (t, m) = frame.servo(n)?;
        return Some(if which == "target" { t } else { m });
    }
    None
}

/// The chassis link index, or why no chassis velocity can be plotted.
pub fn chassis(model: &PhysicalModel) -> Result<usize, String> {
    if let Some(g) = model.links.iter().find(|l| l.ground) {
        return Err(format!("the model's root link `{}` is a ground link (pinned to the world): no moving chassis", g.name));
    }
    let roots: Vec<usize> = (0..model.links.len()).filter(|&i| !model.joints.iter().any(|j| !j.is_loop() && j.child == model.links[i].name)).collect();
    match roots[..] {
        [i] => Ok(i),
        [] => Err("no link is free of a parent joint, so the model has no root link".into()),
        _ => Err(format!("ambiguous chassis: {} links are no joint's child ({}); the runtime picks the heaviest, the viewer does not guess", roots.len(), roots.iter().map(|&i| format!("`{}`", model.links[i].name)).collect::<Vec<_>>().join(", "))),
    }
}

/// Recorded series by key (`request:<channel>`, `chassis:speed`,
/// `chassis:yaw_rate`, `joint:<name>:target|measured`) for one generation.
#[derive(Default)]
pub struct History {
    generation: u64,
    frames: u64,
    series: BTreeMap<String, VecDeque<[f64; 2]>>,
}

impl History {
    /// Drops every sample and records under `generation` from now on.
    pub fn clear(&mut self, generation: u64) {
        self.generation = generation;
        self.frames = 0;
        self.series.clear();
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Frames sampled in this generation.
    pub fn frames(&self) -> u64 {
        self.frames
    }
    /// Samples one applied frame; a frame of another generation than `current` adds nothing.
    pub fn sample(&mut self, current: u64, frame: &Frame, motion: Option<&Motion>, chassis: Option<usize>) -> bool {
        if frame.generation != current {
            return false;
        }
        if self.generation != current {
            self.clear(current);
        }
        let t = frame.time;
        let mut values: Vec<(String, f64)> = Vec::new();
        for c in motion.map(|m| m.channels.iter()).into_iter().flatten() {
            if let Some(x) = frame.inputs.get(c.index) {
                values.push((format!("request:{}", c.name), *x));
            }
        }
        if let Some(Some((v, w))) = chassis.and_then(|i| frame.velocities.get(i)) {
            values.push(("chassis:speed".into(), v[0].hypot(v[1])));
            values.push(("chassis:yaw_rate".into(), w[2]));
        }
        for (i, name) in frame.joint_names.iter().enumerate() {
            if let (Some(target), Some(angle)) = (frame.targets.get(i), frame.joint_angles.get(i)) {
                values.push((format!("joint:{}:target", short(name)), *target));
                values.push((format!("joint:{}:measured", short(name)), *angle));
            }
        }
        for (key, y) in values.into_iter().filter(|(_, y)| y.is_finite()) {
            let ring = self.series.entry(key).or_default();
            match ring.back_mut() {
                Some(last) if last[0] == t => *last = [t, y],
                Some(last) if last[0] > t => {
                    ring.clear();
                    ring.push_back([t, y]);
                }
                _ => ring.push_back([t, y]),
            }
            while ring.len() > MAX_SAMPLES || ring.front().is_some_and(|p| p[0] < t - WINDOW_S) {
                ring.pop_front();
            }
        }
        self.frames += 1;
        true
    }
    /// Samples the picked channels of one applied frame (PICK_RULE), under the same
    /// generation, window and cap as [`Self::sample`] (which counts the frame).
    pub fn sample_picks(&mut self, current: u64, frame: &Frame, picks: &[String], drive: Option<&crate::robot::run::Drive>, links: &[String]) {
        if frame.generation != current || self.generation != current {
            return;
        }
        let t = frame.time;
        for key in picks {
            let Some(y) = pick_value(frame, drive, links, key).filter(|y| y.is_finite()) else { continue };
            let ring = self.series.entry(format!("pick:{key}")).or_default();
            match ring.back_mut() {
                Some(last) if last[0] == t => *last = [t, y],
                Some(last) if last[0] > t => {
                    ring.clear();
                    ring.push_back([t, y]);
                }
                _ => ring.push_back([t, y]),
            }
            while ring.len() > MAX_SAMPLES || ring.front().is_some_and(|p| p[0] < t - WINDOW_S) {
                ring.pop_front();
            }
        }
    }
    /// A trace's value at time `t` (the last sample at or before it), for the review cursor.
    pub fn value_at(points: &[[f64; 2]], t: f64) -> Option<f64> {
        let i = points.partition_point(|p| p[0] <= t + 1e-12);
        points.get(i.checked_sub(1)?).map(|p| p[1])
    }
    /// [first, last] sample time over every trace (None before a sample).
    pub fn window(&self) -> Option<[f64; 2]> {
        let t0 = self.series.values().filter_map(|r| r.front()).map(|p| p[0]).reduce(f64::min)?;
        let t1 = self.series.values().filter_map(|r| r.back()).map(|p| p[0]).reduce(f64::max)?;
        Some([t0, t1])
    }
    fn trace(&self, key: &str, name: String, source: String, unit: &str) -> TraceView {
        let points: Vec<[f64; 2]> = self.series.get(key).map(|r| r.iter().copied().collect()).unwrap_or_default();
        let absent_reason = points.is_empty().then(|| if self.frames == 0 { "no frame of this generation yet".to_string() } else { "not in frame".to_string() });
        TraceView { name, source, unit: if unit.is_empty() { "unit not declared".into() } else { unit.into() }, points, absent_reason }
    }
}

/// What the charts are drawn from besides the samples.
pub struct Context<'a> {
    /// A preset run (session frames), not `--robot FILE`.
    pub preset: bool,
    /// The built preset's motion config: None before a build, Some(None) without motion channels.
    pub motion: Option<Option<&'a Motion>>,
    pub chassis: &'a Result<usize, String>,
    pub links: &'a [String],
    /// The selected link's name, and its servo joints (name, unit) or why it has none.
    pub selected: Option<(&'a str, Result<Vec<(String, &'static str)>, String>)>,
    /// The picked channels with their labels and units (the Picked chart).
    pub picks: Vec<(String, String, String)>,
}

pub struct TraceView {
    pub name: String,
    pub source: String,
    pub unit: String,
    pub points: Vec<[f64; 2]>,
    pub absent_reason: Option<String>,
}
pub struct ChartView {
    pub id: &'static str,
    pub title: String,
    pub absent_reason: Option<String>,
    pub traces: Vec<TraceView>,
}

/// The fixed chart set: `motion` (presets with motion channels) and `joints`.
pub fn charts(h: &History, cx: &Context) -> Vec<ChartView> {
    let mut out = Vec::new();
    match cx.motion {
        Some(Some(m)) => {
            let mut traces: Vec<TraceView> = m.channels.iter().map(|c| h.trace(&format!("request:{}", c.name), c.name.clone(), REQUEST_SOURCE.into(), &c.unit)).collect();
            let absent_reason = match cx.chassis {
                Ok(i) => {
                    let link = cx.links.get(*i).map_or("?", String::as_str);
                    traces.push(h.trace("chassis:speed", "chassis |v_xy|".into(), format!("{WORLD_FRAME}: |(vx, vy)| of velocity_m_s of link `{link}`"), "m/s"));
                    traces.push(h.trace("chassis:yaw_rate", "chassis ω_z".into(), format!("{WORLD_FRAME}: z of angular_velocity_rad_s of link `{link}`"), "rad/s"));
                    None
                }
                Err(e) => Some(format!("measured chassis traces not plotted: {e}")),
            };
            out.push(ChartView { id: "motion", title: format!("Motion request vs chassis ({WORLD_FRAME})"), absent_reason, traces });
        }
        Some(None) => {}
        None if cx.preset => out.push(ChartView { id: "motion", title: "Motion request vs chassis".into(), absent_reason: Some("no built session yet: the motion config is resolved at build (Run or Step)".into()), traces: Vec::new() }),
        None => {}
    }
    let (title, absent_reason, traces) = match &cx.selected {
        _ if cx.preset => ("Joints: target vs measured".to_string(), Some("not in frame: preset session frames carry no named joint targets (the declared controller drives the joints)".to_string()), Vec::new()),
        None => ("Joints: target vs measured".to_string(), Some("select a link to plot its servo joints".to_string()), Vec::new()),
        Some((link, Err(_))) => (format!("Joints of `{link}`: target vs measured"), Some("no servo joint on selected link".to_string()), Vec::new()),
        Some((link, Ok(joints))) => (
            format!("Joints of `{link}`: target vs measured"),
            None,
            joints
                .iter()
                .flat_map(|(j, unit)| {
                    [
                        h.trace(&format!("joint:{j}:target"), format!("{j} target"), "servo target in frame (PhysicalRobot targets)".into(), unit),
                        h.trace(&format!("joint:{j}:measured"), format!("{j} measured"), "measured joint angle in frame (PhysicalRobot joint_angles)".into(), unit),
                    ]
                })
                .collect(),
        ),
    };
    out.push(ChartView { id: "joints", title, absent_reason, traces });
    if !cx.picks.is_empty() {
        let traces = cx.picks.iter().map(|(key, label, unit)| h.trace(&format!("pick:{key}"), label.clone(), format!("picked: {key} (sampled since picked)"), unit)).collect();
        out.push(ChartView { id: "picked", title: "Picked channels".into(), absent_reason: None, traces });
    }
    out
}

/// `robot_state.graphs`.
pub fn json(h: &History, charts: &[ChartView], visible: bool, mode: &str) -> Value {
    let charts: Vec<Value> = charts
        .iter()
        .map(|c| {
            let traces: Vec<Value> = c.traces.iter().map(|t| json!({"name": t.name, "source": t.source, "unit": t.unit, "latest": t.points.last().map(|p| p[1]), "latest_time": t.points.last().map(|p| p[0]), "samples": t.points.len(), "absent_reason": t.absent_reason})).collect();
            json!({"id": c.id, "title": c.title, "absent_reason": c.absent_reason, "traces": traces})
        })
        .collect();
    json!({"visible": visible, "mode": mode, "generation": h.generation(), "frames_sampled": h.frames(), "window": h.window(), "window_s": WINDOW_S, "max_samples": MAX_SAMPLES,
        "charts": charts, "sampling_rule": SAMPLING_RULE, "chassis_rule": CHASSIS_RULE, "toggle": "system_ui graphs:toggle, key G, or the Graphs button"})
}
