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
