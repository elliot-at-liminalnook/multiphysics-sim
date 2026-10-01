//! Saved views (cad-views-export): RoboCAD's `/views` in its view-state
//! schema (`saved_views.validate_state`), listed, saved, renamed,
//! replaced, deleted and restored onto the native camera.
//!
//! - **List**: `GET /views` on a Dedicated job whenever RoboCAD's shown
//!   revision or the document's generation changes ([`sync`], JobResults;
//!   a saved-view edit bumps RoboCAD's revision, `document.notify`). A job
//!   for an older (generation, revision) is dropped when a newer one starts,
//!   so a stale list never lands.
//! - **Save, rename, replace, delete** are document edits: one `POST
//!   /views`, `PATCH /views/{id}` (only the given key) or `DELETE
//!   /views/{id}` each, through `actions::edit` (one RoboCAD undo step:
//!   "Save view", "Update saved view", "Delete saved view"); a REST caller
//!   waits for RoboCAD's answer. Names are checked as RoboCAD checks them
//!   (`_view_name`: 1–120 characters, stripped) and states as
//!   `validate_state` does (`ViewState::check`), before anything is sent.
//!   The panel's typed name is kept until RoboCAD answers a save and is
//!   cleared only when it succeeded ([`settle_save`]).
//! - **Save and replace** capture the native camera ([`snapshot`] copies
//!   the CAD camera's `Orbit` after `CameraSet::Place`, since the handler
//!   has no camera access) and the display state (`CadDisplay`), converted
//!   by [`convert::capture`].
//! - **Restore** reads the view as listed (a REST caller waits for the
//!   list at the current revision) and applies it here: a `camera_set`
//!   (`CameraAction::Set`, a cut, as RoboCAD's restore) and the display
//!   state (grid, display mode, comment pins, section). RoboCAD's
//!   `POST /views/{id}/restore` is not used: it is GUI-only (409 headless)
//!   and moves RoboCAD's own camera, not this one.
//! - **Panel** ([`panel`]): RoboCAD's Saved Views panel as a floating kit
//!   panel (`view.saved_views` toggles it), and the field-of-view entry
//!   RoboCAD's `view.fov` opens.
mod convert;
mod panel;
#[cfg(test)]
mod tests;

pub use convert::ViewCamera;

use crate::app::actions::{Call, Spec, spec};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::cad::actions::{CAD, CadAction, Cx, edit};
use crate::cad::display::CadDisplay;
use crate::cad::document::{CadDocument, EditDone};
use crate::cad::sync::value;
use crate::camera::{CameraAction, Orbit};
use crate::jobs::{Job, Pool};
use crate::ui_kit::form::TextDraft;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{SavedView, ViewState, check_view_name};

/// (document generation, RoboCAD's shown revision): what a list was read at.
type Key = (u64, u64);

const NO_WINDOW: &str = "saved views need CAD mode's 3D view (no camera to save or restore in this window)";

/// Which field of the panel holds the keyboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ViewField {
    /// The new view's name (RoboCAD's "View name" line edit).
    New,
    /// A saved view's new name (RoboCAD's "Rename…" dialog).
    Rename(String),
    /// RoboCAD's `view.fov` dialog: degrees, 5–120, one decimal.
    Fov,
}

/// The field being typed, its draft and why its last Enter sent nothing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Typing {
    pub field: ViewField,
    pub draft: TextDraft,
    pub error: Option<String>,
}

/// Saved views as last listed, the list job, the camera snapshot and the
/// panel's state (display only).
#[derive(Resource, Default)]
pub struct CadViews {
    /// The views as last listed and the key they were read at.
    pub(crate) listed: Option<(Key, Vec<SavedView>)>,
    /// Why the list at this key failed (retried at the next revision, or by `op: list`).
    pub(crate) error: Option<(Key, String)>,
    job: Option<(Key, Job<Vec<SavedView>>)>,
    /// The CAD camera now (`snapshot`); None without one.
    pub(crate) camera: Option<ViewCamera>,
    /// The Saved Views panel is shown.
    pub(crate) open: bool,
    /// The new view's name as typed (kept while the field is not focused).
    pub(crate) new_name: String,
    pub(crate) typing: Option<Typing>,
    /// The view last restored or saved (RoboCAD's current list item).
    pub(crate) selected: Option<String>,
    /// RoboCAD's panel feedback line ("Showing: …").
    pub(crate) feedback: Option<String>,
    /// The save sent and not yet answered: (document generation, edit
    /// sequence number, name). The typed name stays until RoboCAD answers
    /// and is cleared only when the save succeeded ([`settle_save`]).
    pub(crate) saving: Option<(u64, u64, String)>,
}

/// What `cad_views` does.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ViewsOp {
    /// The views at RoboCAD's current revision (a REST caller waits for them).
    #[default]
    List,
    /// `POST /views {name, state}` with the native camera and display.
    Save,
    /// `PATCH /views/{id} {name}`.
    Rename,
    /// `PATCH /views/{id} {state}` with the native camera and display.
    Replace,
    /// `DELETE /views/{id}`.
    Delete,
    /// Apply view `id` to the native camera and display.
    Restore,
    /// Show (`open: true`), hide (`false`) or toggle the Saved Views panel.
    Panel,
}

/// `cad_views`' arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ViewsArgs {
    #[serde(default)]
    pub op: ViewsOp,
    /// A saved view's id (rename, replace, delete, restore).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The view's name (save, rename).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The panel shown or hidden (panel; absent toggles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
}

impl ViewsArgs {
    pub(crate) fn of(op: ViewsOp, id: Option<&str>, name: Option<&str>) -> CadAction {
        CadAction::CadViews(ViewsArgs { op, id: id.map(str::to_string), name: name.map(str::to_string), open: None })
    }
}

fn key(doc: &CadDocument) -> Key {
    (doc.generation, doc.shown_revision())
}

impl CadViews {
    /// The list at the document's current key.
    fn current(&self, doc: &CadDocument) -> Option<&Vec<SavedView>> {
        self.listed.as_ref().filter(|(k, _)| *k == key(doc)).map(|(_, l)| l)
    }
    /// The list of this document generation, however old (what the panel shows).
    pub(crate) fn shown(&self, doc: &CadDocument) -> &[SavedView] {
        self.listed.as_ref().filter(|(k, _)| k.0 == doc.generation).map_or(&[], |(_, l)| l.as_slice())
    }
    fn name_of(&self, doc: &CadDocument, id: &str) -> String {
        self.shown(doc).iter().find(|v| v.id == id).map_or_else(|| id.to_string(), |v| v.name.clone())
    }
    /// The native camera and display as a view state (`capture_view`).
    fn capture(&self, display: Option<&CadDisplay>) -> Result<ViewState, String> {
        let camera = self.camera.filter(|c| c.radius > 0.0).ok_or("the 3D view's camera is not placed yet: nothing to save")?;
        let fallback = CadDisplay::default();
        let state = convert::capture(&camera, display.unwrap_or(&fallback));
        state.check()?;
        Ok(state)
    }
}

/// Whether a REST caller should wait for the list at the current key
/// (Ok(false): it is here), or why it will not come.
fn listing(views: &CadViews, doc: &CadDocument) -> Result<bool, String> {
    if views.current(doc).is_some() {
        return Ok(false);
    }
    if let Some((k, e)) = &views.error
        && *k == key(doc)
    {
        return Err(format!("RoboCAD's saved views could not be listed (GET /views): {e}"));
    }
    if !doc.connected() {
        return Err(format!("not connected to RoboCAD: {}", doc.connection_line().0));
    }
    if doc.doc_key.is_none() {
        return Err("RoboCAD's document has not been read yet; try again".into());
    }
    Ok(true)
}

/// A REST caller waits (Pending) for the list at the current revision; a
/// click never waits. Ok(true): wait.
fn wait(views: &mut CadViews, doc: &CadDocument, call: &mut Call) -> Result<bool, String> {
    if !call.rest() {
        return Ok(false);
    }
    if let Some(g) = call.continuation.get("views_wait").and_then(Value::as_u64) {
        if g != doc.generation {
            return Err("the CAD document was replaced or reconnected while waiting for its saved views".into());
        }
        if call.cancelled {
            return Err("cancelled waiting for RoboCAD's saved views".into());
        }
    } else if views.error.as_ref().is_some_and(|(k, _)| *k == key(doc)) {
        // A fresh request retries a failed list.
        views.error = None;
    }
    let waiting = listing(views, doc)?;
    if waiting {
        *call.continuation = json!({"views_wait": doc.generation});
    }
    Ok(waiting)
}

/// The list as `cad_views` answers it.
fn list_json(views: &CadViews, doc: &CadDocument) -> Value {
    let list: Vec<Value> = views.shown(doc).iter().map(|v| json!({"id": v.id, "name": v.name, "details": convert::details(&v.state), "state": v.state})).collect();
    json!({
        "views": list,
        "listed_at_revision": views.listed.as_ref().map(|(k, _)| k.1),
        "current": views.current(doc).is_some(),
        "error": views.error.as_ref().map(|(_, e)| e),
        "open": views.open,
        "selected": views.selected,
    })
}

/// `cad_state.views`: the list as `cad_views` answers it, the panel's
/// fields and whether a camera can be saved (null without a window).
pub(in crate::cad) fn state_json(views: Option<&CadViews>, doc: &CadDocument) -> Value {
    let Some(views) = views else { return Value::Null };
    let mut out = list_json(views, doc);
    out["new_name"] = json!(views.new_name);
    out["typing"] = json!(views.typing.as_ref().map(|t| json!({"field": format!("{:?}", t.field), "text": t.draft.text, "error": t.error})));
    out["camera_ready"] = json!(views.camera.is_some_and(|c| c.radius > 0.0));
    out["feedback"] = json!(views.feedback);
    out["listing"] = json!(views.job.is_some());
    out
}

/// The id an op needs.
fn id_of<'a>(args: &'a ViewsArgs, op: &str) -> Result<&'a str, String> {
    args.id.as_deref().filter(|id| !id.is_empty()).ok_or_else(|| format!("{op} needs id (a saved view's id; op list lists them)"))
}

/// `CadViews`, from any entry point.
pub(in crate::cad) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let CadAction::CadViews(args) = action else { return Outcome::Done(Err("not a saved-views action".into())) };
    let Cx { doc, views, display, camera, .. } = cx;
    let doc: &mut CadDocument = doc;
    let Some(views) = views.as_deref_mut() else { return Outcome::Done(Err(NO_WINDOW.into())) };
    let done = |r: Result<Value, String>| Outcome::Done(r);
    if args.open.is_some() && args.op != ViewsOp::Panel {
        return done(Err("open belongs to op panel".into()));
    }
    match args.op {
        ViewsOp::Panel => {
            let open = args.open.unwrap_or(!views.open);
            if views.open != open {
                views.open = open;
            }
            // Hiding the panel ends typing in its fields (the FOV entry is its own).
            if !open && views.typing.as_ref().is_some_and(|t| t.field != ViewField::Fov) {
                views.typing = None;
            }
            done(Ok(json!({"open": open, "views": state_json(Some(&*views), doc)})))
        }
        ViewsOp::List => match wait(views, doc, call) {
            Err(e) => done(Err(e)),
            Ok(true) => Outcome::Pending,
            Ok(false) => done(Ok(list_json(views, doc))),
        },
        ViewsOp::Restore => {
            let id = match id_of(args, "restore") {
                Ok(id) => id,
                Err(e) => return done(Err(e)),
            };
            match wait(views, doc, call) {
                Err(e) => return done(Err(e)),
                Ok(true) => return Outcome::Pending,
                Ok(false) => {}
            }
            let Some(view) = views.shown(doc).iter().find(|v| v.id == id).cloned() else {
                return done(Err(format!("no saved view {id} (RoboCAD's /views lists {}); op list lists them", views.shown(doc).len())));
            };
            done(restore(&view, views, display.as_deref_mut(), camera, doc))
        }
        ViewsOp::Save => {
            let name = match check_view_name(args.name.as_deref().unwrap_or("")) {
                Ok(n) => n,
                Err(e) => return done(Err(e)),
            };
            let state = match views.capture(display.as_deref()) {
                Ok(s) => s,
                Err(e) => return done(Err(e)),
            };
            let n = name.clone();
            let outcome = edit(doc, call, format!("Save view {name}"), move |c| c.save_view(&n, &state).map(|v| EditDone { message: format!("Saved view {}", v.name), result: value(&v) }));
            if !matches!(outcome, Outcome::Done(Err(_))) {
                // The typed name is kept until RoboCAD's answer lands (`settle_save`).
                views.saving = Some((doc.generation, doc.edit_seq, name.clone()));
                views.feedback = Some(format!("Saving: {name}"));
            }
            outcome
        }
        ViewsOp::Rename => {
            let id = match id_of(args, "rename") {
                Ok(id) => id.to_string(),
                Err(e) => return done(Err(e)),
            };
            let name = match check_view_name(args.name.as_deref().unwrap_or("")) {
                Ok(n) => n,
                Err(e) => return done(Err(e)),
            };
            let old = views.name_of(doc, &id);
            let n = name.clone();
            edit(doc, call, format!("Rename saved view {old} to {name}"), move |c| c.update_view(&id, Some(n.as_str()), None).map(|v| EditDone { message: format!("Renamed saved view {old} to {}", v.name), result: value(&v) }))
        }
        ViewsOp::Replace => {
            let id = match id_of(args, "replace") {
                Ok(id) => id.to_string(),
                Err(e) => return done(Err(e)),
            };
            let state = match views.capture(display.as_deref()) {
                Ok(s) => s,
                Err(e) => return done(Err(e)),
            };
            let name = views.name_of(doc, &id);
            edit(doc, call, format!("Update saved view {name}"), move |c| c.update_view(&id, None, Some(&state)).map(|v| EditDone { message: format!("Updated: {} · Undo to revert", v.name), result: value(&v) }))
        }
        ViewsOp::Delete => {
            let id = match id_of(args, "delete") {
                Ok(id) => id.to_string(),
                Err(e) => return done(Err(e)),
            };
            let name = views.name_of(doc, &id);
            edit(doc, call, format!("Delete saved view {name}"), move |c| c.delete_view(&id).map(|v| EditDone { message: format!("Deleted saved view {name} · Undo to restore"), result: v }))
        }
    }
}

/// Apply a saved view to the native camera and display (`restore_view`,
/// without RoboCAD's GUI route): checked whole before anything changes.
fn restore(view: &SavedView, views: &mut CadViews, display: Option<&mut CadDisplay>, camera: &mut Vec<CameraAction>, doc: &mut CadDocument) -> Result<Value, String> {
    let (state, note) = convert::camera_of(&view.state)?;
    if let Some(display) = display {
        convert::apply_display(&view.state, display)?;
    }
    camera.push(CameraAction::Set { state });
    views.selected = Some(view.id.clone());
    views.feedback = Some(format!("Showing: {}", view.name));
    doc.show(Ok(format!("Restored view: {}", view.name)));
    Ok(json!({
        "restored": view.id,
        "name": view.name,
        "camera": state,
        "note": note,
        "route": "read from RoboCAD's GET /views and applied to the native camera and display; RoboCAD's GUI-only POST /views/{id}/restore is not used",
    }))
}

/// Opens RoboCAD's field-of-view entry (`view.fov`: degrees, 5–120), filled
/// with the camera's field of view (`surfaces::registry`'s `Camera(Fov)`).
pub(in crate::cad) fn open_fov(cx: &mut Cx) -> Outcome {
    let Some(views) = cx.views.as_deref_mut() else { return Outcome::Done(Err(NO_WINDOW.into())) };
    let degrees = views.camera.map(|c| panel::fov_text(c.fov.to_degrees()));
    let text = degrees.clone().unwrap_or_else(|| "40".into());
    views.typing = Some(Typing { field: ViewField::Fov, draft: TextDraft { text, select_all: true }, error: None });
    Outcome::Done(Ok(json!({"field_of_view": degrees, "message": "Type the field of view in degrees (5–120) and press Enter; camera_fov {\"degrees\": d} does the same from REST."})))
}

/// The save in flight, once its edit ended (`sync::finish_edit`, run by
/// `sync::receive` just before [`sync`], set `doc.status` from RoboCAD's
/// answer): on success the typed name is cleared (unless it was retyped
/// meanwhile); on failure it stays for another try. A replaced document
/// drops the save without touching the name.
pub(crate) fn settle_save(views: &mut CadViews, doc: &CadDocument) {
    let Some((generation, seq, name)) = views.saving.clone() else { return };
    if generation != doc.generation {
        views.saving = None;
        return;
    }
    let running = doc.edit.is_some() && doc.edit_seq == seq;
    if running {
        return;
    }
    views.saving = None;
    // Another edit numbered since: this one's answer is no longer the status.
    let answer = (doc.edit_seq == seq).then_some(doc.status.as_ref()).flatten();
    match answer {
        Some(Ok(_)) => {
            if views.new_name.trim() == name {
                views.new_name.clear();
            }
            views.feedback = Some(format!("Saved: {name}"));
        }
        Some(Err(e)) => views.feedback = Some(format!("Not saved: {name}: {e}")),
        None => views.feedback = None,
    }
}

/// JobResults: the list at the current (generation, revision): started on
/// a change, a job for an older key dropped, a result landed; a save's
/// answer settled ([`settle_save`]).
pub(super) fn sync(doc: Option<Res<CadDocument>>, views: Option<ResMut<CadViews>>) {
    let (Some(doc), Some(mut views)) = (doc, views) else { return };
    if views.saving.is_some() {
        settle_save(&mut views, &doc);
    }
    let now = key(&doc);
    // A job for another key is superseded (dropping it cancels it).
    if views.job.as_ref().is_some_and(|(k, _)| *k != now) {
        views.job = None;
    }
    let landed = views.job.as_ref().and_then(|(k, job)| job.poll().map(|r| (*k, r)));
    if let Some((k, result)) = landed {
        views.job = None;
        match result {
            Ok(list) => {
                views.listed = Some((k, list));
                views.error = None;
            }
            Err(e) => views.error = Some((k, e)),
        }
    }
    let have = views.listed.as_ref().is_some_and(|(k, _)| *k == now) || views.error.as_ref().is_some_and(|(k, _)| *k == now);
    if have || views.job.is_some() || !doc.connected() || doc.doc_key.is_none() {
        return;
    }
    let Some(client) = doc.client.clone() else { return };
    let job = Job::spawn(Pool::Dedicated, doc.generation, "cad-saved-views", move |_| client.views().map_err(|e| e.to_string()));
    views.job = Some((now, job));
}

/// SimSync (after `CameraSet::Place`): the CAD camera as a saved view
/// needs it, written only when it changed.
pub(super) fn snapshot(cameras: Query<(&Orbit, &Camera)>, views: Option<ResMut<CadViews>>) {
    let Some(mut views) = views else { return };
    let now = cameras.iter().find(|(_, c)| c.is_active).or_else(|| cameras.iter().next()).map(|(o, _)| ViewCamera::of(o));
    if views.camera != now {
        views.camera = now;
    }
}

/// CadPlugin: this part's systems and resources (inserted on entering CAD
/// mode; removed by `cad::clear`).
pub(in crate::cad) fn build(app: &mut App) {
    app.add_systems(OnEnter(ModeScope::Cad), |mut commands: Commands| commands.insert_resource(CadViews::default()))
        .add_systems(
            Update,
            (
                // After the name field and the inspector's editors (which
                // reset `CadInputFocus`), before the chord gate and every
                // CAD key reader, which honour the focus set here.
                panel::input
                    .after(crate::app::actions::serve)
                    .after(crate::cad::panel::name_entry)
                    .after(crate::cad::inspector::editor_entry)
                    .before(crate::cad::keys::gate)
                    .before(crate::cad::keys::keys)
                    .in_set(ViewerSet::Input),
                sync.after(crate::cad::sync::receive).in_set(ViewerSet::JobResults),
                snapshot.after(crate::camera::CameraSet::Place).in_set(ViewerSet::SimSync),
                panel::draw.in_set(ViewerSet::Present),
            )
                .run_if(in_state(ViewerMode::Cad)),
        );
}

/// This part's REST commands (appended to `CadAction::commands`).
pub(in crate::cad) fn specs() -> Vec<Spec> {
    vec![spec(
        "cad_views",
        CAD,
        json!({"op": "save", "name": "Worm drive cutaway"}),
        "CAD mode: RoboCAD's saved views (GET/POST /views, GET/PATCH/DELETE /views/{id}) in its view-state schema (target and distance mm in the model frame, yaw and pitch degrees with Z up, fov degrees, orthographic, mode turntable | trackball, rot rows right/up/back, grid, display_mode, comment_pins, section). op: list (the views at RoboCAD's current revision; waits for them), save (name: 1–120 characters; the native camera and display state, one undo step \"Save view\"), rename (id, name; \"Update saved view\"), replace (id; the current camera and display), delete (id; \"Delete saved view\"), restore (id: applied to the native camera, a cut, and the display's grid, mode, comment pins and section; RoboCAD's GUI-only /views/{id}/restore is not used), panel (open true | false, absent toggles the Saved Views panel). Edits go to RoboCAD's command layer and the caller waits for its answer; system_ui lists cad:view:<id> (restore), cad:view:replace-<id>, cad:view:delete-<id>, cad:view:save (with the panel's typed name), cad:view:list and cad:view:panel.",
    )]
}

/// This part's `system_ui` controls: (id, label, action, ready).
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    controls_of(cx.doc, cx.views.as_deref())
}

/// The controls for `doc` and the views as listed: the panel's buttons
/// write the same actions.
pub(crate) fn controls_of(doc: &CadDocument, views: Option<&CadViews>) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let Some(views) = views else { return Vec::new() };
    let edit_ready = doc.edit_refusal().map_or(Ok(()), Err);
    let camera_ready = || views.camera.filter(|c| c.radius > 0.0).map(|_| ()).ok_or_else(|| "the 3D view's camera is not placed yet".to_string());
    let name = views.new_name.trim().to_string();
    let save_ready = check_view_name(&name).map(|_| ()).map_err(|e| format!("{e}: type the new view's name in the Saved Views panel")).and_then(|()| edit_ready.clone()).and_then(|()| camera_ready());
    let mut out = vec![
        ("cad:view:panel".to_string(), if views.open { "Hide saved views" } else { "Saved views" }.to_string(), CadAction::CadViews(ViewsArgs { op: ViewsOp::Panel, open: Some(!views.open), ..ViewsArgs::default() }), Ok(())),
        ("cad:view:list".to_string(), "List saved views".to_string(), ViewsArgs::of(ViewsOp::List, None, None), Ok(())),
        ("cad:view:save".to_string(), "Save current view".to_string(), ViewsArgs::of(ViewsOp::Save, None, Some(name.as_str())), save_ready),
    ];
    for v in views.shown(doc) {
        out.push((format!("cad:view:{}", v.id), format!("Restore view: {}", v.name), ViewsArgs::of(ViewsOp::Restore, Some(v.id.as_str()), None), Ok(())));
        out.push((format!("cad:view:replace-{}", v.id), format!("Replace with current: {}", v.name), ViewsArgs::of(ViewsOp::Replace, Some(v.id.as_str()), None), edit_ready.clone().and_then(|()| camera_ready())));
        out.push((format!("cad:view:delete-{}", v.id), format!("Delete saved view: {}", v.name), ViewsArgs::of(ViewsOp::Delete, Some(v.id.as_str()), None), edit_ready.clone()));
    }
    out
}
