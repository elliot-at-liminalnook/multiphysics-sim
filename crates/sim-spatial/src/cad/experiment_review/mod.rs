//! Isolated captured review. Durable state owns jobs/cursors; RoboCAD owns
//! captured sources and reference replay. Panel entities are transient. Jobs
//! land in public JobResults; display is SimSync before camera placement.
mod plots;
pub(crate) mod scene;
#[cfg(test)]
mod tests;
mod ui;
use crate::cad::{
    actions::{CadAction, Cx},
    document::CadDocument,
};
use crate::{
    app::actions::Call,
    jobs::{Job, Pool},
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::experiments::CapturedGeometry;
pub(crate) use ui::{build, draw};

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewOp {
    #[default]
    State,
    Dock,
    Open,
    Seek,
    Play,
    Pause,
    Return,
    Filter,
    Frame,
    Signal,
    Baseline,
    Source,
    Live,
    Annotate,
    Flex,
    Cancel,
    Field,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewArgs {
    pub op: ReviewOp,
    pub id: Option<String>,
    pub candidate: Option<String>,
    pub baseline: Option<String>,
    pub time: Option<f64>,
    pub value: Option<String>,
    pub open: Option<bool>,
    pub scale: Option<f64>,
    pub sequence: Option<u64>,
}
impl ReviewArgs {
    pub(crate) fn of(op: ReviewOp) -> Self {
        Self {
            op,
            ..Default::default()
        }
    }
    pub(crate) fn action(self) -> CadAction {
        CadAction::CadExperimentReview(self)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stamp {
    pub generation: u64,
    pub document: Option<String>,
    pub revision: u64,
    pub sequence: u64,
}
impl Stamp {
    pub(crate) fn of(d: &CadDocument, sequence: u64) -> Self {
        Self {
            generation: d.generation,
            document: d.doc_key.as_ref().and_then(|k| k.0.clone()),
            revision: d.shown_revision(),
            sequence,
        }
    }
    pub(crate) fn matches(&self, d: &CadDocument) -> bool {
        self.generation == d.generation
            && self.document == d.doc_key.as_ref().and_then(|k| k.0.clone())
    }
}
pub(crate) struct Capture {
    pub geometry: CapturedGeometry,
    pub result: Value,
    pub sources: Value,
    pub baseline: Value,
    pub comparison: Value,
    pub built: Vec<(String, crate::cad::mesh::Built)>,
}
#[derive(Resource, Default)]
pub(crate) struct ReviewState {
    pub open: bool,
    pub active: bool,
    pub run: Option<String>,
    pub candidate: Option<String>,
    pub baseline: Option<String>,
    pub captured: Option<Capture>,
    pub origin: Option<Stamp>,
    pub sample: Value,
    pub cursor: f64,
    pub filter: String,
    pub signal: String,
    pub source: Option<String>,
    pub note: String,
    pub focus: Option<String>,
    pub error: Option<String>,
    pub sequence: u64,
    pub revision: u64,
    pub flex_scale: f64,
    pub playing: bool,
    pub clock: Option<std::time::Instant>,
    pub clock_time: f64,
    pub(crate) read: Option<(Stamp, Job<Capture>)>,
    pub(crate) sampling: Option<(Stamp, f64, Job<Value>)>,
    pub(crate) comparing: Option<(Stamp, Job<(Value, Value)>)>,
    pub(crate) chart: Option<Handle<Image>>,
    pub cancelled: bool,
    pub frame: bool,
    pub pending_cursor: Option<f64>,
}
impl ReviewState {
    pub(crate) fn touch(&mut self) {
        self.revision += 1;
    }
    pub(crate) fn busy(&self) -> bool {
        self.read.is_some() || self.sampling.is_some() || self.comparing.is_some()
    }
}
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, a: &mut App) {
        a.init_resource::<ReviewState>().add_systems(
            Update,
            tick.in_set(crate::app::ViewerSet::JobResults)
                .after(crate::cad::CadSet::Results),
        );
    }
}
pub(crate) fn handle(a: &ReviewArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    if a.sequence.is_some_and(|seq| seq != cx.review.sequence) {
        return Outcome::Done(Err(
            "Review field belongs to another captured request; draft retained".into(),
        ));
    }
    if matches!(a.op, ReviewOp::Live | ReviewOp::Annotate)
        && (cx.review.cancelled
            || cx
                .review
                .origin
                .as_ref()
                .is_none_or(|stamp| !stamp.matches(cx.doc)))
    {
        return Outcome::Done(Err(
            "Reopen the captured source in this document before live navigation or annotation"
                .into(),
        ));
    }
    if a.op == ReviewOp::Live {
        if let Some(node) = &a.value {
            if !cx.doc.has_node(node) {
                return Outcome::Done(Err("Mapped live node no longer exists".into()));
            }
            let action = CadAction::CadSelect {
                ids: vec![node.clone()],
                items: vec![],
                extend: false,
                toggle: false,
                picked_at: Some(cx.doc.shown_revision()),
            };
            return crate::cad::selection::handle(&action, call, cx);
        }
        let id = cx.review.sample["signals"][&cx.review.signal]["component_id"]
            .as_str()
            .map(str::to_string);
        let Some(id) = id else {
            return Outcome::Done(Err(
                "This captured signal has no live component mapping".into()
            ));
        };
        let args = crate::cad::composition::CadCompositionArgs {
            id: Some(id),
            ..crate::cad::composition::CadCompositionArgs::of(
                crate::cad::composition::CompositionOp::Select,
            )
        };
        return crate::cad::composition::handle(&args, call, cx);
    }
    if a.op == ReviewOp::Annotate {
        let s = &cx.review;
        let Some(c) = &s.captured else {
            return Outcome::Done(Err("No captured sample".into()));
        };
        let Some(run) = &s.run else {
            return Outcome::Done(Err("Candidate geometry has no recorded sample".into()));
        };
        let time = s.sample["time"]
            .as_f64()
            .ok_or("Wait for the stamped replay sample");
        let time = match time {
            Ok(t) => t,
            Err(e) => return Outcome::Done(Err(e.into())),
        };
        let mut evidence = json!({"run_id":run,"time_range":[time,time]});
        if let Some(hash) = &c.geometry.identity.physical_hash {
            evidence["physical_hash"] = json!(hash);
        }
        if !s.signal.is_empty() {
            evidence["signal"] = json!(s.signal);
            if let Some(series) = s.sample["signals"].get(&s.signal) {
                if series["node_ids"].is_array() {
                    evidence["node_ids"] = series["node_ids"].clone();
                }
                if series["source"].is_object() {
                    evidence["source"] = series["source"].clone();
                }
            }
        }
        let body = a.value.clone().unwrap_or_else(|| s.note.clone());
        return crate::cad::threads::annotate_evidence(cx, call, evidence, body);
    }
    let d = &mut *cx.doc;
    let s = &mut *cx.review;
    let result = (|| -> Result<Value, String> {
        match a.op {
            ReviewOp::State => return Ok(state_json(d, s)),
            ReviewOp::Dock => {
                s.open = a.open.unwrap_or(!s.open);
                if !s.open {
                    s.request_cancel();
                    s.focus = None;
                }
            }
            ReviewOp::Open => {
                if cx.motion.export.is_some() {
                    return Err("Cancel export and wait for its terminal receipt before opening captured review".into());
                }
                let client = d.client.clone().ok_or("Connect to RoboCAD first")?;
                let id = a.id.clone();
                let candidate = a.candidate.clone();
                if id.is_none() == candidate.is_none() {
                    return Err("Choose exactly one run or candidate".into());
                }
                s.sequence += 1;
                s.focus = None;
                s.cancelled = false;
                s.run = id.clone();
                s.candidate = candidate.clone();
                s.baseline = a.baseline.clone().or(s.baseline.clone());
                s.open = true;
                s.active = false;
                s.playing = false;
                s.captured = None;
                s.sample = Value::Null;
                s.signal = a.value.clone().unwrap_or_default();
                s.pending_cursor = a.time;
                cx.motion.request_cancel();
                let baseline = s.baseline.clone();
                let stamp = Stamp::of(d, s.sequence);
                s.origin = Some(stamp.clone());
                s.read = Some((
                    stamp,
                    Job::spawn(
                        Pool::Dedicated,
                        d.generation,
                        "captured CAD review",
                        move |ctx| {
                            let geometry = if let Some(id) = &id {
                                client.experiment_geometry(id)
                            } else {
                                client.candidate_geometry(candidate.as_deref().unwrap_or_default())
                            }
                            .map_err(|e| e.to_string())?;
                            if geometry.units != "mm" {
                                return Err("Captured geometry must declare millimetres".into());
                            }
                            let mut built = Vec::new();
                            for n in &geometry.nodes {
                                if ctx.cancelled() {
                                    return Err("Captured read cancelled".into());
                                }
                                if let Some(m) = &n.mesh {
                                    built.push((n.id.clone(), crate::cad::mesh::build(&n.id, m)?));
                                }
                            }
                            let result = if let Some(id) = &id {
                                client
                                    .experiment_result(id)
                                    .or_else(|_| client.experiment_partial(id))
                                    .map_err(|e| e.to_string())?
                            } else {
                                Value::Null
                            };
                            let sources = if let Some(id) = &id {
                                client.experiment_sources(id).map_err(|e| e.to_string())?
                            } else {
                                Value::Null
                            };
                            let (base, comparison) = if let (Some(b), Some(id)) = (&baseline, &id) {
                                (
                                    {
                                        let mut base = client
                                            .experiment_result(b)
                                            .map_err(|e| e.to_string())?;
                                        let time = base["trace"]["t"][0].as_f64().unwrap_or(0.);
                                        base["review_signals"] = client
                                            .experiment_sample(b, time, 1.)
                                            .map_err(|e| e.to_string())?["signals"]
                                            .clone();
                                        base
                                    },
                                    client
                                        .experiment_compare(b, id)
                                        .map_err(|e| e.to_string())?,
                                )
                            } else {
                                (Value::Null, Value::Null)
                            };
                            Ok(Capture {
                                geometry,
                                result,
                                sources,
                                baseline: base,
                                comparison,
                                built,
                            })
                        },
                    ),
                ));
            }
            ReviewOp::Seek => {
                let t = a.time.ok_or("review.time is required")?;
                if !t.is_finite() {
                    return Err("review.time must be finite".into());
                }
                s.cursor = t;
                s.playing = false;
                request_sample(d, s)?;
            }
            ReviewOp::Play => {
                if s.run.is_none() || s.captured.is_none() {
                    return Err("Load a captured run first".into());
                }
                s.playing = true;
                s.clock = Some(std::time::Instant::now());
                s.clock_time = s.cursor;
            }
            ReviewOp::Pause => s.playing = false,
            ReviewOp::Return => {
                s.request_cancel();
                s.focus = None;
            }
            ReviewOp::Frame => s.frame = true,
            ReviewOp::Filter => s.filter = a.value.clone().unwrap_or_default(),
            ReviewOp::Signal => s.signal = a.value.clone().unwrap_or_default(),
            ReviewOp::Source => s.source = a.value.clone(),
            ReviewOp::Field => s.note = a.value.clone().unwrap_or_default(),
            ReviewOp::Baseline => {
                s.baseline = a.id.clone().or(s.run.clone());
                let c = d.client.clone().ok_or("Connect to RoboCAD")?;
                let baseline = s.baseline.clone().ok_or("Choose a baseline run")?;
                let run = s.run.clone().ok_or("Open a captured run")?;
                let stamp = Stamp::of(d, s.sequence);
                s.comparing = Some((
                    stamp,
                    Job::spawn(
                        Pool::Dedicated,
                        d.generation,
                        "captured baseline comparison",
                        move |_| {
                            let mut base =
                                c.experiment_result(&baseline).map_err(|e| e.to_string())?;
                            let time = base["trace"]["t"][0].as_f64().unwrap_or(0.);
                            base["review_signals"] = c
                                .experiment_sample(&baseline, time, 1.)
                                .map_err(|e| e.to_string())?["signals"]
                                .clone();
                            Ok((
                                base,
                                c.experiment_compare(&baseline, &run)
                                    .map_err(|e| e.to_string())?,
                            ))
                        },
                    ),
                ));
            }
            ReviewOp::Flex => {
                let scale = a.scale.ok_or("review.scale required")?;
                if !scale.is_finite() || scale <= 0. {
                    return Err("Flex scale must be finite and positive".into());
                }
                s.flex_scale = scale;
                request_sample(d, s)?;
            }
            ReviewOp::Cancel => {
                s.request_cancel();
            }
            ReviewOp::Live => return Err("Use the explicit mapped live navigation control".into()),
            ReviewOp::Annotate => unreachable!(),
        }
        Ok(json!({"review":true,"sequence":s.sequence}))
    })();
    if let Err(e) = &result {
        s.error = Some(e.clone());
    }
    s.touch();
    d.touch();
    Outcome::Done(result)
}
fn request_sample(d: &CadDocument, s: &mut ReviewState) -> Result<(), String> {
    let id = s.run.clone().ok_or("Candidate geometry has no replay")?;
    let client = d.client.clone().ok_or("Connect to RoboCAD first")?;
    let time = s.cursor;
    let scale = if s.flex_scale > 0. { s.flex_scale } else { 1. };
    let stamp = Stamp::of(d, s.sequence);
    s.sampling = Some((
        stamp,
        time,
        Job::spawn(
            Pool::Dedicated,
            d.generation,
            "captured replay sample",
            move |_| {
                client
                    .experiment_sample(&id, time, scale)
                    .map_err(|e| e.to_string())
            },
        ),
    ));
    Ok(())
}
fn tick(doc: Option<Res<CadDocument>>, mut s: ResMut<ReviewState>) {
    let Some(d) = doc else {
        if let Some((_, j)) = &s.read {
            j.cancel();
        }
        if let Some((_, _, j)) = &s.sampling {
            j.cancel();
        }
        s.active = false;
        s.playing = false;
        return;
    };
    if s.origin.as_ref().is_some_and(|stamp| !stamp.matches(&d)) && !s.cancelled {
        s.request_cancel();
        s.focus = None;
        s.error = Some(
            "Live document replaced; captured sources and note retained without active display"
                .into(),
        );
    }
    if s.read.as_ref().is_some_and(|(stamp, _)| !stamp.matches(&d)) {
        s.cancelled = true;
        s.active = false;
        s.playing = false;
    }
    if let Some(answer) = s.read.as_ref().and_then(|(_, j)| j.poll()) {
        let (stamp, _) = s.read.take().unwrap();
        if stamp.matches(&d) && stamp.sequence == s.sequence && !s.cancelled {
            match answer {
                Ok(c) => {
                    s.cursor = s
                        .pending_cursor
                        .take()
                        .unwrap_or_else(|| c.result["trace"]["t"][0].as_f64().unwrap_or(0.));
                    s.captured = Some(c);
                    s.active = true;
                    s.frame = true;
                    let _ = request_sample(&d, &mut s);
                }
                Err(e) => s.error = Some(e),
            }
        } else {
            s.error = Some(
                "Captured read finished for an old/closed request; source retained remotely".into(),
            );
        }
        s.touch();
    }
    if let Some(answer) = s.sampling.as_ref().and_then(|(_, _, j)| j.poll()) {
        let (stamp, time, _) = s.sampling.take().unwrap();
        if stamp.matches(&d) && stamp.sequence == s.sequence && time == s.cursor && !s.cancelled {
            match answer {
                Ok(v) => {
                    s.cursor = v["time"].as_f64().unwrap_or(time);
                    if s.signal.is_empty() {
                        s.signal = v["signals"]
                            .as_object()
                            .and_then(|m| m.keys().next())
                            .cloned()
                            .unwrap_or_default();
                    }
                    s.sample = v;
                }
                Err(e) => {
                    s.error = Some(e);
                    s.active = false;
                    s.playing = false;
                }
            }
            s.touch();
        }
    }
    if let Some(answer) = s.comparing.as_ref().and_then(|(_, j)| j.poll()) {
        let (stamp, _) = s.comparing.take().unwrap();
        if stamp.matches(&d) && stamp.sequence == s.sequence && !s.cancelled {
            match answer {
                Ok((base, report)) => {
                    if let Some(c) = &mut s.captured {
                        c.baseline = base;
                        c.comparison = report;
                    }
                }
                Err(e) => s.error = Some(e),
            }
            s.touch();
        }
    }
    if s.playing && s.sampling.is_none() {
        let end = s
            .captured
            .as_ref()
            .and_then(|c| c.result["trace"]["t"].as_array())
            .and_then(|t| t.last())
            .and_then(Value::as_f64)
            .unwrap_or(0.);
        s.cursor = (s.clock_time + s.clock.map_or(0., |t| t.elapsed().as_secs_f64())).min(end);
        if s.cursor >= end {
            s.playing = false;
        }
        let _ = request_sample(&d, &mut s);
    }
}
pub(crate) type Control = (String, String, CadAction, Result<(), String>);
pub(crate) fn controls(cx: &Cx) -> Vec<Control> {
    controls_of(cx.doc, cx.review)
}
pub(crate) fn controls_of(d: &CadDocument, s: &ReviewState) -> Vec<Control> {
    let mut out = Vec::new();
    for (op, label) in [
        (ReviewOp::Dock, "Captured review"),
        (ReviewOp::Play, "Play captured replay"),
        (ReviewOp::Pause, "Pause replay"),
        (ReviewOp::Return, "Return to live CAD"),
        (ReviewOp::Cancel, "Cancel captured read"),
        (ReviewOp::Baseline, "Set baseline"),
        (ReviewOp::Annotate, "Annotate sample"),
        (ReviewOp::Live, "Inspect mapped live component"),
        (ReviewOp::Frame, "Frame captured geometry"),
    ] {
        let ready = match op {
            ReviewOp::Annotate | ReviewOp::Live
                if s.cancelled || s.origin.as_ref().is_none_or(|stamp| !stamp.matches(d)) =>
            {
                Err("Reopen captured source in this document first".into())
            }
            ReviewOp::Frame if s.captured.is_none() => Err("Load captured geometry first".into()),
            ReviewOp::Play | ReviewOp::Baseline | ReviewOp::Annotate
                if s.run.is_none() || s.captured.is_none() =>
            {
                Err("Load a captured run first".into())
            }
            ReviewOp::Annotate if s.sample["time"].as_f64().is_none() => {
                Err("Wait for a stamped captured sample".into())
            }
            ReviewOp::Annotate => d.commit_refusal_for(None, true).map_or(Ok(()), Err),
            ReviewOp::Live
                if s.sample["signals"][&s.signal]["component_id"]
                    .as_str()
                    .is_none() =>
            {
                Err("This captured signal has no live component mapping".into())
            }
            _ => Ok(()),
        };
        out.push((
            format!("cad:review:{op:?}"),
            label.into(),
            ReviewArgs {
                sequence: Some(s.sequence),
                ..ReviewArgs::of(op)
            }
            .action(),
            ready,
        ));
    }
    for scale in [1., 10., 100., 1000.] {
        out.push((
            format!("cad:review:flex:{scale}"),
            format!("Flex boundary arrows ×{scale} · rigid CAD mesh"),
            ReviewArgs {
                scale: Some(scale),
                sequence: Some(s.sequence),
                ..ReviewArgs::of(ReviewOp::Flex)
            }
            .action(),
            if s.run.is_some() {
                Ok(())
            } else {
                Err("Choose a captured run".into())
            },
        ));
    }
    if let Some(signals) = s.sample["signals"].as_object() {
        for (name, series) in signals {
            if !s.filter.is_empty()
                && series["node_ids"]
                    .as_array()
                    .is_some_and(|ids| !ids.iter().any(|id| id.as_str() == Some(s.filter.as_str())))
            {
                continue;
            }
            out.push((
                format!("cad:review:signal:{name}"),
                format!("{name} [{}] {}", series["unit"], s.sample["values"][name]),
                ReviewArgs {
                    value: Some(name.clone()),
                    sequence: Some(s.sequence),
                    ..ReviewArgs::of(ReviewOp::Signal)
                }
                .action(),
                Ok(()),
            ));
        }
    }
    if let Some(c) = &s.captured {
        if let Some(bundles) = c.sources.as_object() {
            for (kind, bundle) in bundles {
                if let Some(files) = bundle["files"].as_object() {
                    for path in files.keys() {
                        out.push((
                            format!("cad:review:source:{kind}/{path}"),
                            format!("View captured {kind}/{path}"),
                            ReviewArgs {
                                value: Some(format!("{kind}/{path}")),
                                sequence: Some(s.sequence),
                                ..ReviewArgs::of(ReviewOp::Source)
                            }
                            .action(),
                            Ok(()),
                        ));
                    }
                }
            }
        }
        for n in &c.geometry.nodes {
            out.push((
                format!("cad:review:part:{}", n.id),
                format!("Filter captured {}", n.name),
                ReviewArgs {
                    value: Some(n.id.clone()),
                    sequence: Some(s.sequence),
                    ..ReviewArgs::of(ReviewOp::Filter)
                }
                .action(),
                Ok(()),
            ));
            if d.doc
                .as_ref()
                .is_some_and(|live| live.nodes.iter().any(|live| live.id == n.id))
            {
                out.push((
                    format!("cad:review:live:{}", n.id),
                    format!("Navigate live {} (explicit ID mapping)", n.name),
                    ReviewArgs {
                        value: Some(n.id.clone()),
                        sequence: Some(s.sequence),
                        ..ReviewArgs::of(ReviewOp::Live)
                    }
                    .action(),
                    if s.cancelled || s.origin.as_ref().is_none_or(|stamp| !stamp.matches(d)) {
                        Err("Reopen captured source before live navigation".into())
                    } else {
                        Ok(())
                    },
                ));
            }
        }
        if let Some(times) = c.result["trace"]["t"].as_array() {
            for (i, t) in times.iter().enumerate().step_by((times.len() / 20).max(1)) {
                out.push((
                    format!("cad:review:seek:{i}"),
                    format!("Seek {} s", t),
                    ReviewArgs {
                        time: t.as_f64(),
                        sequence: Some(s.sequence),
                        ..ReviewArgs::of(ReviewOp::Seek)
                    }
                    .action(),
                    Ok(()),
                ));
            }
        }
    }
    out
}
pub(crate) fn state_json(d: &CadDocument, s: &ReviewState) -> Value {
    json!({"open":s.open,"active":s.active,"run":s.run,"candidate":s.candidate,"baseline":s.baseline,"cursor_s":s.cursor,"sample":s.sample,"capture":s.captured.as_ref().map(|c|&c.geometry.identity),"live_revision":d.shown_revision(),"cancel_requested":s.cancelled,"pending":s.busy(),"error":s.error})
}
pub(crate) fn specs() -> Vec<crate::app::actions::Spec> {
    vec![crate::app::actions::spec(
        "cad_experiment_review",
        crate::cad::actions::CAD,
        json!({"op":"open","id":"run-id"}),
        "Isolated captured run/candidate review, seek(time seconds), play/pause/return, baseline(id), filter(value captured node), signal(value), source(value path), flex(scale), annotate(value body), cancel. Captured geometry is read-only; explicit live mappings are separate controls.",
    )]
}

pub(crate) fn key(s: &ReviewState) -> String {
    format!("{}:{}:{}", s.revision, s.sequence, s.active)
}

impl ReviewState {
    pub(crate) fn request_cancel(&mut self) {
        self.sequence += 1;
        self.cancelled = true;
        self.active = false;
        self.playing = false;
        self.focus = None;
        if let Some((_, j)) = &self.read {
            j.cancel();
        }
        if let Some((_, _, j)) = &self.sampling {
            j.cancel();
        }
        if let Some((_, j)) = &self.comparing {
            j.cancel();
        }
        self.touch();
    }
    pub(crate) fn source_edit_refusal(&self) -> Option<String> {
        self.active
            .then(|| "Return from captured review before editing live CAD".into())
    }
}
