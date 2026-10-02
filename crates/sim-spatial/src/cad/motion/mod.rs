//! Reference kinematic preview. Persistent drafts are viewer resources; named
//! programs remain CAD-owned undoable commands. No physics or hardware commands.
mod controls;
mod export;
mod sampling;
#[cfg(test)]
mod tests;
mod ui;
use crate::cad::{
    actions::{CadAction, Cx},
    document::{CadDocument, EditDone},
    experiment_review::Stamp,
};
use crate::{
    app::actions::Call,
    jobs::{Job, Pool},
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{
    components::ComponentStamp,
    motion::{PoseMetadata, PoseRequest, PoseSample},
};
use std::collections::BTreeMap;
pub(crate) use ui::{build, draw};
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MotionOp {
    #[default]
    State,
    Dock,
    Enter,
    Return,
    Joint,
    Position,
    Markers,
    Focus,
    Program,
    Editor,
    Validate,
    Sweep,
    Save,
    Delete,
    Play,
    Pause,
    Seek,
    Export,
    Cancel,
    Path,
    CapturedFrame,
    ExportFps,
    ExportSize,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MotionArgs {
    pub op: MotionOp,
    pub id: Option<String>,
    pub value: Option<String>,
    pub time: Option<f64>,
    pub position: Option<f64>,
    pub open: Option<bool>,
    pub revision: Option<u64>,
    pub sequence: Option<u64>,
    #[serde(skip)]
    pub pixels: Option<export::Pixels>,
}
impl MotionArgs {
    pub(crate) fn of(op: MotionOp) -> Self {
        Self {
            op,
            ..Default::default()
        }
    }
    pub(crate) fn action(self) -> CadAction {
        CadAction::CadMotion(self)
    }
}
#[derive(Resource, Default)]
pub(crate) struct MotionState {
    pub open: bool,
    pub active: bool,
    pub metadata: Option<PoseMetadata>,
    pub sample: Option<PoseSample>,
    pub programs: BTreeMap<String, Value>,
    pub positions: BTreeMap<String, f64>,
    pub joint: Option<String>,
    pub selected: Option<String>,
    pub editor: String,
    pub drafts: Vec<String>,
    pub focus: Option<String>,
    pub cursor: f64,
    pub playing: bool,
    pub clock: Option<std::time::Instant>,
    pub clock_time: f64,
    pub markers: bool,
    pub frame: bool,
    pub path: String,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    pub error: Option<String>,
    pub sequence: u64,
    pub revision: u64,
    pub cancel_requested: bool,
    pub(crate) identity: Option<Stamp>,
    pub(crate) loading: Option<(Stamp, Job<(PoseMetadata, BTreeMap<String, Value>)>)>,
    pub(crate) sampling: Option<sampling::Pending>,
    pub(crate) queued_sample: Option<sampling::Intent>,
    pub(crate) sample_sequence: u64,
    pub(crate) program_job: Option<(Stamp, u64, Job<Value>)>,
    pub(crate) export: Option<export::Export>,
    pub history: Vec<Value>,
}
impl MotionState {
    pub(crate) fn touch(&mut self) {
        self.revision += 1;
    }
    pub(crate) fn busy(&self) -> bool {
        self.loading.is_some() || self.program_job.is_some() || self.export.is_some()
    }
    pub(crate) fn mode_blockers(&self) -> Vec<String> {
        if self.export.is_some() {
            vec!["Cancel motion export and wait for its terminal receipt before changing modes/documents".into()]
        } else {
            vec![]
        }
    }
    pub(crate) fn request_cancel(&mut self) {
        self.cancel_requested = true;
        self.playing = false;
        self.active = false;
        if let Some((_, j)) = &self.loading {
            j.cancel();
        }
        self.queued_sample = None;
        if let Some(pending) = &self.sampling {
            pending.job.cancel();
        }
        if let Some((_, _, j)) = &self.program_job {
            j.cancel();
        }
        if let Some(e) = &mut self.export {
            e.cancel();
        }
        self.touch();
    }
}
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, a: &mut App) {
        a.init_resource::<MotionState>().add_systems(
            Update,
            tick.in_set(crate::app::ViewerSet::JobResults)
                .after(crate::cad::CadSet::Results),
        );
    }
}
pub(crate) fn handle(a: &MotionArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    if a.op == MotionOp::Export && cx.view.is_none_or(|view| !view.valid) {
        return Outcome::Done(Err("Native export requires a rendered CAD window".into()));
    }
    let d = &mut *cx.doc;
    let s = &mut *cx.motion;
    if s.export.is_some()
        && !matches!(
            a.op,
            MotionOp::State
                | MotionOp::Cancel
                | MotionOp::CapturedFrame
                | MotionOp::Return
                | MotionOp::Dock
        )
    {
        return Outcome::Done(Err("Motion inputs are locked while export is running; cancel and wait for its terminal receipt".into()));
    }
    if matches!(
        a.op,
        MotionOp::Position
            | MotionOp::Seek
            | MotionOp::Joint
            | MotionOp::Program
            | MotionOp::Sweep
            | MotionOp::Validate
    ) && a.sequence.is_some_and(|seq| seq != s.sequence)
    {
        return Outcome::Done(Err("Motion field belongs to an older joint/program".into()));
    }
    if matches!(a.op, MotionOp::Save | MotionOp::Delete) {
        let result = (|| -> Result<_, String> {
            let id = s.identity.as_ref().ok_or("Enter pose mode first")?;
            if !id.matches(d) {
                return Err(
                    "Motion draft belongs to another document; retained without publication".into(),
                );
            }
            if a.sequence.is_some_and(|seq| seq != s.sequence) {
                return Err("Motion draft changed; original input retained".into());
            }
            let revision = a.revision.unwrap_or(id.revision);
            guard(d, id)?;
            let stamp = ComponentStamp {
                document_id: id.document.clone().ok_or("Document ID missing")?,
                expected_revision: revision,
            };
            let program: Value = if a.op == MotionOp::Delete {
                if a.id.as_ref().or(s.selected.as_ref()).is_none() {
                    return Err("Choose a named program to delete".into());
                }
                Value::Null
            } else {
                serde_json::from_str(&s.editor).map_err(|e| format!("motion.program: {e}"))?
            };
            Ok((stamp, program))
        })();
        let (stamp, program) = match result {
            Ok(v) => v,
            Err(e) => {
                s.error = Some(e.clone());
                s.touch();
                return Outcome::Done(Err(e));
            }
        };
        s.drafts.push(s.editor.clone());
        let delete = a.op == MotionOp::Delete;
        let name = a.id.clone().or(s.selected.clone()).unwrap_or_default();
        return crate::cad::actions::edit_auxiliary_at(
            d,
            call,
            Some(stamp.expected_revision),
            if delete {
                "Delete motion"
            } else {
                "Save motion"
            }
            .into(),
            move |c| {
                let value = if delete {
                    c.delete_motion(&name, &stamp)?
                } else {
                    c.save_motion(&program, &stamp)?
                };
                Ok(EditDone {
                    message: "Motion program command acknowledged".into(),
                    result: value,
                })
            },
        );
    }
    let result = (|| -> Result<Value, String> {
        match a.op {
            MotionOp::State => return Ok(state_json(d, s)),
            MotionOp::Dock => {
                s.open = a.open.unwrap_or(!s.open);
                if !s.open {
                    s.request_cancel();
                    s.focus = None;
                }
            }
            MotionOp::Enter => {
                s.focus = None;
                if s.export.is_some() {
                    return Err("Wait for export terminal receipt".into());
                }
                let c = d.client.clone().ok_or("Connect to RoboCAD first")?;
                s.sequence += 1;
                s.cancel_requested = false;
                s.open = true;
                s.active = false;
                s.sample = None;
                s.playing = false;
                cx.review.request_cancel();
                if s.identity.as_ref().is_some_and(|id| !id.matches(d)) {
                    s.drafts.push(s.editor.clone());
                    s.editor.clear();
                    s.selected = None;
                }
                let stamp = Stamp::of(d, s.sequence);
                s.loading = Some((
                    stamp,
                    Job::spawn(
                        Pool::Dedicated,
                        d.generation,
                        "reference pose metadata",
                        move |_| {
                            Ok((
                                c.pose_metadata().map_err(|e| e.to_string())?,
                                c.motion_programs().map_err(|e| e.to_string())?,
                            ))
                        },
                    ),
                ));
            }
            MotionOp::Return => {
                s.request_cancel();
                s.focus = None;
            }
            MotionOp::Joint => {
                let id = a.id.as_deref().ok_or("Choose a reference joint")?;
                if s.metadata
                    .as_ref()
                    .is_none_or(|metadata| !metadata.joints.iter().any(|j| j.id == id))
                {
                    return Err("motion.joint: unknown reference joint".into());
                }
                s.focus = None;
                s.joint = a.id.clone();
                s.sequence += 1;
            }
            MotionOp::Position => {
                let id =
                    a.id.clone()
                        .or(s.joint.clone())
                        .ok_or("Choose a driver joint")?;
                let p = a
                    .position
                    .ok_or("motion.position required (declared joint unit)")?;
                if !p.is_finite() {
                    return Err("Position must be finite".into());
                }
                let j = s
                    .metadata
                    .as_ref()
                    .and_then(|m| m.joints.iter().find(|j| j.id == id))
                    .ok_or("Unknown reference joint")?;
                if !j.driver {
                    return Err("Passive/coupled joints follow the driver".into());
                }
                if p < j.display_lower || p > j.display_upper {
                    return Err("Position exceeds reference display bounds".into());
                }
                s.positions.insert(id, p);
                s.playing = false;
                sample(d, s, None)?;
            }
            MotionOp::Markers => s.markers = !s.markers,
            MotionOp::Focus => s.frame = true,
            MotionOp::Program => {
                s.focus = None;
                let id = a.id.clone().ok_or("Choose a named program")?;
                let p = s.programs.get(&id).ok_or("Unknown named program")?;
                s.drafts.push(s.editor.clone());
                s.editor = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
                s.selected = Some(id);
                s.sequence += 1;
                s.playing = false;
            }
            MotionOp::Editor => {
                if a.sequence.is_some_and(|seq| seq != s.sequence) {
                    s.drafts.push(a.value.clone().unwrap_or_default());
                    return Err("Edited text was retained for an older draft".into());
                }
                s.editor = a.value.clone().unwrap_or_default();
                s.sequence += 1;
                s.playing = false;
            }
            MotionOp::Validate | MotionOp::Sweep => {
                s.focus = None;
                let c = d.client.clone().ok_or("Connect to RoboCAD first")?;
                let id = s.identity.clone().ok_or("Enter pose mode first")?;
                guard(d, &id)?;
                let stamp = ComponentStamp {
                    document_id: id.document.clone().ok_or("Document ID missing")?,
                    expected_revision: id.revision,
                };
                let p: Value = serde_json::from_str(&s.editor).unwrap_or(Value::Null);
                let sweep = a.op == MotionOp::Sweep;
                let joint = s.joint.clone().unwrap_or_default();
                let seq = s.sequence;
                s.program_job = Some((
                    id,
                    seq,
                    Job::spawn(
                        Pool::Dedicated,
                        d.generation,
                        "reference motion validation",
                        move |_| {
                            if sweep {
                                c.motion_sweep(&joint, &stamp)
                            } else {
                                c.validate_motion(&p, &stamp)
                            }
                            .map_err(|e| e.to_string())
                        },
                    ),
                ));
            }
            MotionOp::Play => {
                let p: Value =
                    serde_json::from_str(&s.editor).map_err(|e| format!("motion.program: {e}"))?;
                sample(d, s, Some(p))?;
                s.clock = Some(std::time::Instant::now());
                s.clock_time = s.cursor;
                s.playing = true;
            }
            MotionOp::Pause => s.playing = false,
            MotionOp::Seek => {
                let t = a.time.ok_or("motion.time required (seconds)")?;
                if !t.is_finite() || t < 0. {
                    return Err("Seek time must be finite nonnegative seconds".into());
                }
                s.cursor = t;
                s.playing = false;
                let p = serde_json::from_str(&s.editor).ok();
                sample(d, s, p)?;
            }
            MotionOp::Export => export::start(d, s)?,
            MotionOp::Cancel => s.request_cancel(),
            MotionOp::Path => s.path = a.value.clone().unwrap_or_default(),
            MotionOp::ExportFps => {
                let fps = a
                    .value
                    .as_deref()
                    .unwrap_or("")
                    .parse::<u32>()
                    .map_err(|_| "Choose 24, 30 or 60 fps")?;
                if ![24, 30, 60].contains(&fps) {
                    return Err("Choose 24,30 or60 fps".into());
                }
                if s.export.is_some() {
                    return Err("Export settings locked while running".into());
                }
                s.fps = fps;
            }
            MotionOp::ExportSize => {
                if s.export.is_some() {
                    return Err("Export settings locked while running".into());
                }
                match a.value.as_deref() {
                    Some("720p") => {
                        s.width = 1280;
                        s.height = 720;
                    }
                    Some("1080p") => {
                        s.width = 1920;
                        s.height = 1080;
                    }
                    _ => return Err("Choose 720p or 1080p".into()),
                }
            }
            MotionOp::CapturedFrame => {
                export::deliver(
                    s,
                    a.pixels
                        .clone()
                        .ok_or("Internal screenshot payload missing")?,
                )?;
            }
            MotionOp::Save | MotionOp::Delete => unreachable!(),
        }
        Ok(json!({"motion":true,"sequence":s.sequence}))
    })();
    if let Err(e) = &result {
        s.error = Some(e.clone());
    }
    s.touch();
    d.touch();
    Outcome::Done(result)
}
fn guard(d: &CadDocument, id: &Stamp) -> Result<(), String> {
    if !id.matches(d) || id.revision != d.shown_revision() || d.stale.is_some() {
        Err("Reference preview source changed; return/re-enter (draft retained)".into())
    } else {
        Ok(())
    }
}
fn sample(d: &CadDocument, s: &mut MotionState, program: Option<Value>) -> Result<(), String> {
    sampling::request(d, s, program)
}
fn tick(doc: Option<Res<CadDocument>>, mut s: ResMut<MotionState>) {
    let Some(d) = doc else {
        s.request_cancel();
        export::poll(&mut s, None);
        return;
    };
    if s.identity.as_ref().is_some_and(|id| guard(&d, id).is_err()) {
        s.request_cancel();
    }
    if let Some(answer) = s.loading.as_ref().and_then(|(_, j)| j.poll()) {
        let (stamp, _) = s.loading.take().unwrap();
        if stamp.matches(&d)
            && stamp.revision == d.shown_revision()
            && stamp.sequence == s.sequence
            && !s.cancel_requested
        {
            match answer {
                Ok((m, p)) => {
                    if m.identity.document_id != stamp.document
                        || m.identity.revision != Some(stamp.revision)
                    {
                        s.error = Some(
                            "Reference metadata belongs to a replaced or newer source; re-enter"
                                .into(),
                        );
                        s.active = false;
                        s.touch();
                        return;
                    }
                    s.identity = Some(stamp);
                    s.positions = m
                        .joints
                        .iter()
                        .filter(|j| j.driver)
                        .map(|j| (j.id.clone(), j.home))
                        .collect();
                    s.joint = m.joints.iter().find(|j| j.driver).map(|j| j.id.clone());
                    s.metadata = Some(m);
                    s.programs = p;
                    s.active = true;
                    let _ = sample(&d, &mut s, None);
                }
                Err(e) => s.error = Some(e),
            }
        }
        s.touch();
    }
    sampling::receive(&d, &mut s);
    if let Some(answer) = s.program_job.as_ref().and_then(|(_, _, j)| j.poll()) {
        let (stamp, seq, _) = s.program_job.take().unwrap();
        if stamp.matches(&d)
            && stamp.revision == d.shown_revision()
            && seq == s.sequence
            && !s.cancel_requested
        {
            match answer {
                Ok(p) => {
                    s.focus = None;
                    s.editor = serde_json::to_string_pretty(&p).unwrap_or_default();
                    s.sequence += 1;
                }
                Err(e) => s.error = Some(e),
            }
        }
        s.touch();
    }
    if s.playing && s.sampling.is_none() {
        let p = serde_json::from_str::<Value>(&s.editor).ok();
        let duration = p
            .as_ref()
            .and_then(|p| p["duration"].as_f64())
            .unwrap_or(0.);
        let time = s.clock_time + s.clock.map_or(0., |t| t.elapsed().as_secs_f64());
        if duration > 0. {
            s.cursor = if p
                .as_ref()
                .is_some_and(|p| p["loop"].as_bool() == Some(true))
            {
                time % duration
            } else {
                time.min(duration)
            };
            if time >= duration && s.cursor == duration {
                s.playing = false;
            }
            if let Err(e) = sample(&d, &mut s, p) {
                s.error = Some(e);
                s.active = false;
                s.playing = false;
            }
        }
    }
    export::poll(&mut s, Some(&d));
}
pub(crate) use controls::{Control, controls, controls_of};
pub(crate) fn key(s: &MotionState) -> String {
    format!(
        "{}:{}:{}:{}",
        s.revision,
        s.sequence,
        s.active,
        s.export.is_some()
    )
}
pub(crate) fn state_json(_d: &CadDocument, s: &MotionState) -> Value {
    json!({"open":s.open,"active":s.active,"kind":"kinematic display-only","metadata":s.metadata,"sample":s.sample,"programs":s.programs,"editor":s.editor,"retained_drafts":s.drafts,"cursor_s":s.cursor,"cancel_requested":s.cancel_requested,"export":s.export.as_ref().map(|e|e.state()),"history":s.history,"error":s.error})
}
pub(crate) fn specs() -> Vec<crate::app::actions::Spec> {
    vec![crate::app::actions::spec(
        "cad_motion",
        crate::cad::actions::CAD,
        json!({"op":"enter"}),
        "Reference kinematic preview: enter/return, joint(id), position(position declared rad/mm), markers, focus, program(id), editor(value JSON), validate, sweep, save/delete guarded by revision and draft sequence, play/pause/seek(time seconds), path(value absolute destination), export/cancel. Display-only; source programs use authoritative undo.",
    )]
}

pub(crate) fn command_action(id: &str) -> Option<CadAction> {
    matches!(id, "view.pose" | "robot.pose").then(|| {
        MotionArgs {
            open: Some(true),
            ..MotionArgs::of(MotionOp::Dock)
        }
        .action()
    })
}

impl MotionState {
    pub(crate) fn source_edit_refusal(&self) -> Option<String> {
        self.active
            .then(|| "Return from kinematic preview before editing live CAD".into())
    }
}
