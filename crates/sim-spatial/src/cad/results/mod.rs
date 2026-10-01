//! Results, identification, actuator profiles, the stress overlay,
//! physical export and the live link to Robot mode (cad-physical-inspect,
//! RoboCAD's `ui/app.py` 1676-1781 and `simbridge.SimLink`).
//!
//! - **Load results** and **apply identification** are one RoboCAD call
//!   each (`POST /results/load`, `POST /identification/apply`) through
//!   `actions::edit_at`, as RoboCAD's own handlers run them (neither is an
//!   undo step there either). RoboCAD does not count them as a revision, so
//!   once the edit answered the robot reads are taken again
//!   (`RobotData::invalidate`) and a load turns the stress overlay on, as
//!   RoboCAD's `robot_load_results` does. Without a path the path form
//!   opens ([`forms`], RoboCAD's file dialogs).
//! - **Actuator profiles**: `POST /actuator-profiles` with the given JSON
//!   (RoboCAD validates it through Rust, one undo step), or a JSON file
//!   read on a `Pool::Io` job first (the window's way: RoboCAD itself has no
//!   dialog for them). The current profiles are RoboCAD's
//!   (`RobotData::profiles`), shown read-only.
//! - **The stress overlay** ([`overlay`]): the shared rule
//!   (`sim_domain_robot::stress_results::link_colours`, Robot mode's too)
//!   over each body's hotspot as RoboCAD hangs it on the node; display only.
//! - **Physical export** ([`export`]): `GET /physical` on a Dedicated job,
//!   written atomically by the job; at most one at a time.
//! - **The live link** ([`link`]): every successful save re-exports the
//!   document's `<stem>.simrobot.json` and Robot mode in this window shows it.
mod export;
mod forms;
mod link;
mod overlay;
#[cfg(test)]
mod tests;

pub(crate) use export::{ExportRequest, Exports};
pub(crate) use forms::{FormKind, PathForm};
pub(crate) use link::LiveLink;
pub(crate) use overlay::StressPaint;

use super::actions::{CAD, CadAction, Cx, edit_at};
use super::document::{CadDocument, CadTarget, EditDone};
use crate::app::actions::{Call, Spec, spec};
use crate::app::{ViewerMode, ViewerSet};
use crate::jobs::{Job, Latest, Pool};
use crate::ui_kit::path_field::Listing;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_domain_robot::stress_results::SCALE;
use std::path::{Path, PathBuf};

/// What RoboCAD's own window draws instead (`viewport._stress_colors`).
pub(crate) const ROBOCAD_SCALE: &str = "RoboCAD's own window colours linearly, blue 0 → red at yield; this window uses the rule Robot mode uses";

/// What `cad_results` does.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResultsOp {
    /// The results, overlay, export and link state (as `cad_state` shows it).
    #[default]
    State,
    /// `POST /results/load {path}` (RoboCAD's "Robot: load simulation
    /// results…"); without path the form opens.
    Load,
    /// `POST /identification/apply {path}`; without path the form opens.
    Identify,
    /// `POST /actuator-profiles {profiles}`; with path instead, that JSON
    /// file is read first; with neither the form opens.
    Profiles,
    /// The stress overlay on (`open: true`), off (`false`) or toggled.
    Overlay,
    /// Export the physical model (`kind: physical`, flexible links) or the
    /// simulation model (`kind: simulation`, the x–z planar hint) to path;
    /// without path the form opens.
    Export,
    /// Stop the running export (nothing is written).
    ExportCancel,
    /// The live link on (`open: true`), off (`false`) or toggled.
    Link,
    /// Switch to Robot mode on the live link's model (or the last export).
    ShowRobot,
    /// The open path form's text.
    FormSet,
    /// The open path form's OK.
    FormSubmit,
    /// The open path form's Cancel.
    FormCancel,
}

/// Which model an export writes (RoboCAD's two Simulation menu entries).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExportKind {
    /// `sim.export_physical`: flexible links, no planar hint.
    #[default]
    Physical,
    /// `sim.export`: flexible links with the x–z planar hint
    /// (`simbridge.export_sim_model`'s model).
    Simulation,
}
impl ExportKind {
    /// (flex, planar, RoboCAD's label).
    pub(crate) fn shape(self) -> (bool, bool, &'static str) {
        match self {
            ExportKind::Physical => (true, false, "physical model"),
            ExportKind::Simulation => (true, true, "simulation model"),
        }
    }
}

/// `cad_results`' arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ResultsArgs {
    #[serde(default)]
    pub op: ResultsOp,
    /// A file (load, identify, profiles, export): absolute, `~/` expanded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The actuator profiles JSON (profiles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Value>,
    /// Which model (export; physical when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ExportKind>,
    /// On or off (overlay, link; absent toggles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    /// The form's path text (form_set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl ResultsArgs {
    pub(crate) fn of(op: ResultsOp) -> CadAction {
        CadAction::CadResults(ResultsArgs { op, ..ResultsArgs::default() })
    }
    fn export(kind: ExportKind) -> CadAction {
        CadAction::CadResults(ResultsArgs { op: ResultsOp::Export, kind: Some(kind), ..ResultsArgs::default() })
    }
}

/// An edit this part started, settled once RoboCAD answered ([`settle`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Waiting {
    Load,
    Identify,
    Profiles,
    /// A save (`files::save`, noticed by [`note_save`]): the path of a Save As.
    Save(Option<String>),
}

/// This part's state on the document (reset with it).
#[derive(Default)]
pub struct ResultsState {
    /// The stress overlay is on.
    pub(crate) overlay: bool,
    /// The open path form.
    pub(crate) form: Option<PathForm>,
    /// The form's directory listing (`Pool::Io`).
    pub(crate) listing: Latest<Listing>,
    pub(crate) listed: Option<Listing>,
    /// Physical exports: the running one, the queued live-link one, the last outcome.
    pub(crate) exports: Exports,
    /// A profiles file being read (`Pool::Io`): its path and the job.
    pub(crate) profiles_read: Option<(String, Job<Value>)>,
    /// The edit started here (or the save noticed) and its sequence number.
    pub(crate) waiting: Option<(u64, Waiting)>,
    /// The `.rcad` the live link watches (mirrored into [`LiveLink`], which
    /// outlives the document; seeded from it once per document).
    pub(crate) link: Option<PathBuf>,
    /// The next written live-link model switches to Robot mode (toggle-on).
    pub(crate) link_switch: bool,
    pub(crate) link_seeded: bool,
    /// A switch to Robot mode on this model, written as a window action by [`link::receive`].
    pub(crate) switch_to: Option<PathBuf>,
}

/// The document's file: RoboCAD's path, else the opened target's.
pub(crate) fn doc_path(doc: &CadDocument) -> Option<PathBuf> {
    doc.health.as_ref().and_then(|h| h.path.clone()).map(PathBuf::from).or_else(|| match &doc.target {
        CadTarget::File(p) => Some(p.clone()),
        CadTarget::Service(_) => None,
    })
}

/// `<stem>.simrobot.json` beside `rcad` (`simbridge.sim_model_path`).
pub(crate) fn model_path(rcad: &Path) -> PathBuf {
    rcad.with_extension("simrobot.json")
}

/// Whether the live link watches this document now.
pub(crate) fn link_active(doc: &CadDocument) -> bool {
    doc.results.link.is_some() && doc.results.link == doc_path(doc)
}

/// The staleness label of the loaded results: RoboCAD's own `stale` flag
/// (`physical.load_results`: the document's physical hash differs from the
/// one the results recorded, or none was recorded).
pub(crate) fn staleness(doc: &CadDocument) -> &'static str {
    match doc.robot.data.results() {
        None => "not read yet",
        Some(r) if r.path.is_none() && r.nodes.is_empty() => "no results loaded",
        Some(r) => match r.stale {
            Some(true) => "stale",
            Some(false) => "current",
            None => "unknown (RoboCAD sent no stale flag)",
        },
    }
}

/// Called by `files::save` once its save edit started: the live link
/// re-exports when it succeeds ([`settle`]).
pub(in crate::cad) fn note_save(doc: &mut CadDocument, path: Option<&str>) {
    doc.results.waiting = Some((doc.edit_seq, Waiting::Save(path.map(str::to_string))));
}

/// One edit through `edit_at`, remembered when it started.
fn edit_noted(doc: &mut CadDocument, call: &mut Call, label: String, what: Waiting, work: impl FnOnce(&sim_runtime::cad_client::CadClient) -> Result<EditDone, sim_runtime::cad_client::CadError> + Send + 'static) -> Outcome {
    let before = doc.edit_seq;
    let outcome = edit_at(doc, call, None, label, work);
    if doc.edit_seq != before {
        doc.results.waiting = Some((doc.edit_seq, what));
    }
    outcome
}

/// The edit this part waits for has answered: a load turns the overlay on
/// and every one re-reads RoboCAD's robot data (a load or an identification
/// does not move its revision); a successful save re-exports the live link's model.
pub(crate) fn settle(doc: &mut CadDocument) {
    let Some((seq, what)) = doc.results.waiting.clone() else { return };
    if doc.edit.is_some() && doc.edit_seq == seq {
        return;
    }
    doc.results.waiting = None;
    let ok = doc.edit_seq == seq && matches!(doc.status, Some(Ok(_)));
    if !ok {
        return;
    }
    match what {
        Waiting::Load => {
            doc.results.overlay = true;
            doc.robot.data.invalidate();
        }
        Waiting::Identify | Waiting::Profiles => doc.robot.data.invalidate(),
        Waiting::Save(path) => link::saved(doc, path),
    }
    doc.touch();
}

/// The checked path of an op, or why not.
fn path_of(args: &ResultsArgs, what: &str) -> Option<Result<String, String>> {
    args.path.as_deref().map(|p| crate::cad::files::absolute(p, what))
}

/// `CadResults`, from any entry point.
pub(in crate::cad) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let CadAction::CadResults(args) = action else { return Outcome::Done(Err("not a results action".into())) };
    let done = Outcome::Done;
    if args.text.is_some() && args.op != ResultsOp::FormSet {
        return done(Err("text belongs to op form_set".into()));
    }
    if args.kind.is_some() && args.op != ResultsOp::Export {
        return done(Err("kind belongs to op export".into()));
    }
    match args.op {
        ResultsOp::State => done(Ok(state_json(cx.doc))),
        ResultsOp::Load => match path_of(args, "Load simulation results") {
            None => done(forms::open(cx, FormKind::Load)),
            Some(Err(e)) => forms::settled(cx, FormKind::Load, done(Err(e))),
            Some(Ok(p)) => {
                let (sent, message) = (p.clone(), format!("results loaded: {p} (stress overlay on; margins in the Robot panel)"));
                let outcome = edit_noted(cx.doc, call, format!("Load results {p}"), Waiting::Load, move |c| c.load_results(&sent).map(|v| EditDone { message, result: v }));
                forms::settled(cx, FormKind::Load, outcome)
            }
        },
        ResultsOp::Identify => match path_of(args, "Apply identification") {
            None => done(forms::open(cx, FormKind::Identify)),
            Some(Err(e)) => forms::settled(cx, FormKind::Identify, done(Err(e))),
            Some(Ok(p)) => {
                let outcome = edit_noted(cx.doc, call, format!("Apply identification {p}"), Waiting::Identify, move |c| {
                    c.apply_identification(&p).map(|v| {
                        let fitted: Vec<String> = v.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
                        EditDone { message: format!("identified parameters stored for {}; they ride along with the next export", fitted.join(", ")), result: v }
                    })
                });
                forms::settled(cx, FormKind::Identify, outcome)
            }
        },
        ResultsOp::Profiles => profiles(args, call, cx),
        ResultsOp::Overlay => {
            let on = args.open.unwrap_or(!cx.doc.results.overlay);
            cx.doc.results.overlay = on;
            let message = if on { format!("stress overlay on: {SCALE}, from the loaded results ({})", staleness(cx.doc)) } else { "stress overlay off".to_string() };
            cx.doc.show(Ok(message.clone()));
            done(Ok(json!({"overlay": on, "message": message, "scale": SCALE, "robocad": ROBOCAD_SCALE})))
        }
        ResultsOp::Export => {
            let kind = args.kind.unwrap_or_default();
            match path_of(args, "Export") {
                None => done(forms::open(cx, FormKind::Export(kind))),
                Some(Err(e)) => forms::settled(cx, FormKind::Export(kind), done(Err(e))),
                Some(Ok(p)) => {
                    let (flex, planar, label) = kind.shape();
                    let request = ExportRequest { path: export::model_file(&p), flex, planar, label: label.to_string(), link: false };
                    let outcome = done(export::request(cx.doc, request));
                    forms::settled(cx, FormKind::Export(kind), outcome)
                }
            }
        }
        ResultsOp::ExportCancel => done(export::cancel(cx.doc)),
        ResultsOp::Link => done(link::toggle(cx.doc, args.open)),
        ResultsOp::ShowRobot => done(link::show(cx.doc)),
        ResultsOp::FormSet | ResultsOp::FormSubmit | ResultsOp::FormCancel => forms::handle(args, call, cx),
    }
}

/// `op: profiles`: the given JSON sent, a file read first, or the form.
fn profiles(args: &ResultsArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let done = Outcome::Done;
    match (&args.profiles, path_of(args, "Apply actuator profiles")) {
        (Some(_), Some(_)) => done(Err("profiles takes the profiles JSON or path (a JSON file holding it), not both".into())),
        (None, None) => done(forms::open(cx, FormKind::Profiles)),
        (None, Some(Err(e))) => forms::settled(cx, FormKind::Profiles, done(Err(e))),
        (None, Some(Ok(p))) => {
            if let Some(why) = cx.doc.commit_refusal(None) {
                return forms::settled(cx, FormKind::Profiles, done(Err(why)));
            }
            let path = p.clone();
            let job = Job::spawn(Pool::Io, cx.doc.generation, "actuator profiles file", move |_| {
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
                serde_json::from_str::<Value>(&text).map_err(|e| format!("{path}: not JSON: {e}"))
            });
            cx.doc.results.profiles_read = Some((p.clone(), job));
            cx.doc.show(Ok(format!("Reading actuator profiles from {p}…")));
            forms::settled(cx, FormKind::Profiles, done(Ok(json!({"reading": p, "message": "The file is read off the UI thread, then sent to RoboCAD (POST /actuator-profiles); the outcome shows in the status line."}))))
        }
        (Some(v), None) => {
            if !v.is_object() {
                return done(Err("profiles must be a JSON object (RoboCAD's actuator_profiles: profile name → profile)".into()));
            }
            let profiles = v.clone();
            edit_noted(cx.doc, call, "Set actuator profiles".into(), Waiting::Profiles, move |c| c.set_actuator_profiles(&profiles).map(|r| EditDone { message: "Actuator profiles set (one undo step in RoboCAD)".into(), result: r }))
        }
    }
}

/// `cad_state.results`.
pub(in crate::cad) fn state_json(doc: &CadDocument) -> Value {
    let r = &doc.results;
    let loaded = doc.robot.data.results();
    let mut out = json!({
        "overlay": {
            "on": r.overlay,
            "status": staleness(doc),
            "path": loaded.and_then(|l| l.path.clone()),
            "loaded": loaded.and_then(|l| l.loaded.clone()),
            "scale": SCALE,
            "robocad": ROBOCAD_SCALE,
            "nodes": loaded.map_or(0, |l| l.nodes.len()),
        },
        "form": r.form.as_ref().map(PathForm::json),
        "exports": r.exports.json(),
        "waiting": r.waiting.as_ref().map(|(seq, w)| json!({"edit": seq, "what": format!("{w:?}")})),
    });
    out["link"] = link::json(doc);
    out["profiles"] = json!({
        "current": doc.robot.data.profiles(),
        "reading": r.profiles_read.as_ref().map(|(p, _)| p),
        "note": "RoboCAD's document profiles (robot_settings.actuator_profiles), read-only here; set them with op profiles",
    });
    out
}

/// The action a RoboCAD command id stands for (`robot::command_action`):
/// the dialog commands open the path form.
pub(crate) fn command_action(id: &str) -> Option<CadAction> {
    match id {
        "robot.load_results" => Some(ResultsArgs::of(ResultsOp::Load)),
        "robot.apply_identification" => Some(ResultsArgs::of(ResultsOp::Identify)),
        // Both toggle the overlay (the rest of cad-print is another epic).
        "view.stress" | "print.overlay" => Some(ResultsArgs::of(ResultsOp::Overlay)),
        "sim.export_physical" => Some(ResultsArgs::export(ExportKind::Physical)),
        "sim.export" => Some(ResultsArgs::export(ExportKind::Simulation)),
        "sim.link" => Some(ResultsArgs::of(ResultsOp::Link)),
        _ => None,
    }
}

/// This part's `system_ui` controls: (id, label, action, ready).
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    controls_of(cx.doc)
}

/// The controls for `doc` (the panel's buttons write the same actions).
pub(crate) fn controls_of(doc: &CadDocument) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let r = &doc.results;
    let connected = || if doc.connected() { Ok(()) } else { Err(format!("not connected to RoboCAD: {}", doc.connection_line().0)) };
    let mut out = vec![
        ("cad:results:load".to_string(), "Load simulation results…".to_string(), ResultsArgs::of(ResultsOp::Load), connected()),
        ("cad:results:identify".to_string(), "Apply identified joint parameters…".to_string(), ResultsArgs::of(ResultsOp::Identify), connected()),
        ("cad:results:profiles".to_string(), "Apply actuator profiles…".to_string(), ResultsArgs::of(ResultsOp::Profiles), connected()),
        ("cad:results:overlay".to_string(), if r.overlay { "Hide stress overlay" } else { "Show stress overlay" }.to_string(), CadAction::CadResults(ResultsArgs { op: ResultsOp::Overlay, open: Some(!r.overlay), ..Default::default() }), Ok(())),
        ("cad:results:export_physical".to_string(), "Export physical model…".to_string(), ResultsArgs::export(ExportKind::Physical), connected()),
        ("cad:results:export".to_string(), "Export simulation model…".to_string(), ResultsArgs::export(ExportKind::Simulation), connected()),
    ];
    if let Some(running) = &r.exports.running {
        out.push(("cad:results:export_cancel".to_string(), format!("Cancel export: {}", running.request.label), ResultsArgs::of(ResultsOp::ExportCancel), Ok(())));
    }
    let on = link_active(doc);
    let link_ready = if on || doc_path(doc).is_some() { Ok(()) } else { Err(link::UNSAVED.to_string()) };
    out.push(("cad:results:link".to_string(), if on { "Stop live link" } else { "Start live link" }.to_string(), CadAction::CadResults(ResultsArgs { op: ResultsOp::Link, open: Some(!on), ..Default::default() }), link_ready));
    out.push(("cad:results:show_robot".to_string(), "Show in Robot mode".to_string(), ResultsArgs::of(ResultsOp::ShowRobot), link::shown_model(doc).map(|_| ())));
    if let Some(form) = &r.form {
        out.push(("cad:results:form_cancel".to_string(), format!("Close: {}", form.kind.title()), ResultsArgs::of(ResultsOp::FormCancel), Ok(())));
    }
    out
}

/// This part's REST command (appended to `CadAction::commands`).
pub(in crate::cad) fn specs() -> Vec<Spec> {
    vec![spec(
        "cad_results",
        CAD,
        json!({"op": "load", "path": "/tmp/robot.simresult.json"}),
        format!(
            "CAD mode: RoboCAD's simulation results, identification, actuator profiles, the stress overlay, physical export and the live link. op: state (this part's state), load (POST /results/load {{path}}: RoboCAD hangs each link, joint and motor block on its node; then the robot reads (GET /results/nodes margins) are taken again and the stress overlay turns on, as RoboCAD's \"Robot: load simulation results…\"), identify (POST /identification/apply {{path}}: fitted joint parameters stored for the next export; RoboCAD's error verbatim), profiles (profiles: the actuator profiles JSON object, POST /actuator-profiles, validated by RoboCAD through Rust, one undo step; or path: a JSON file holding it, read off the UI thread and then sent; the current profiles are read-only in the state), overlay (open true | false, absent toggles: per-vertex stress colours on the drawn bodies from each node's results hotspot, display only; {SCALE}; RoboCAD's own window colours linearly from blue 0 to red at yield; yield is the node material's yield strength, else the largest cell stress; cells are in the link frame, placed at the block's com, else the node's mass centroid), export (kind physical: flexible links, RoboCAD's \"export physical model\"; kind simulation: the same with the x–z planar hint; GET /physical on a job, written atomically to path (.simrobot.json appended unless it ends in .json; never the .rcad); one at a time: a second is refused \"a model export is already running\"; progress \"exporting … n s\" in the status line), export_cancel (drops the running export: nothing is written; RoboCAD's request itself runs to the end), link (open true | false, absent toggles: RoboCAD's live link; needs a saved document; on: the document's <stem>.simrobot.json is exported (no flexible links, the planar hint) now and after every successful save, a save during an export queues the latest; the first export switches this window to Robot mode on it, later ones make Robot mode reload it when shown), show_robot (switch to Robot mode on the live link's model, else the last export written), form_set (text: the open path form's path), form_submit, form_cancel. Without path, load, identify, profiles and export open the path form. Edits are one RoboCAD call each through the edit path, refused by name while another is in flight; REST callers of load, identify and profiles wait for RoboCAD's answer. system_ui lists cad:results:load, cad:results:identify, cad:results:profiles, cad:results:overlay, cad:results:export_physical, cad:results:export, cad:results:export_cancel (while one runs), cad:results:link, cad:results:show_robot and cad:results:form_cancel (while the form is open)."
        ),
    )]
}

/// CadCorePlugin's windowless part: the profiles read, the edits' settling,
/// the exports and the live link (JobResults, after the edit's answer
/// landed in `sync::receive`). [`LiveLink`] outlives CAD mode's visits.
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, app: &mut App) {
        app.init_resource::<LiveLink>().add_systems(Update, link::receive.after(super::sync::receive).in_set(ViewerSet::JobResults).run_if(in_state(ViewerMode::Cad)));
    }
}

/// CadPlugin: the path form, the stress paint and the results panel.
pub(in crate::cad) fn build(app: &mut App) {
    forms::build(app);
    overlay::build(app);
}
