//! File workflows (cad-views-export): new, open, save as and import with
//! units, export in every RoboCAD format and the drawing, and render, each
//! a `CadAction` on a job with progress and a named refusal; new and open
//! never lose unsaved edits, as in RoboCAD.
//!
//! - **Paths, not dialogs.** A command without a path opens a modal path
//!   form on the UI kit (`form`), pre-filled with the document's directory,
//!   with the directory's matching files listed by a `Pool::Io` job (the
//!   kit path field's listing, `ui_kit::path_field`, the one way to enter
//!   a path in the window); no file I/O on the UI thread. rfd is not in
//!   the workspace: a native dialog adds a dependency tree (objc2/AppKit
//!   on macOS, GTK or the portal on Linux) whose macOS dialogs must run on
//!   the main thread's event loop, which cannot be verified without
//!   building and running; revisit in a verification pass.
//! - **Unsaved edits.** RoboCAD's New and Open open another window, so
//!   they never lose edits. CAD mode shows one document, so new and open
//!   replace it under `cad_open`'s rule (`CadDocument::switch_blockers`,
//!   the one check for the form, REST and `cad_open`): refused by name on
//!   an edit in flight or a self-started service's unsaved (or
//!   unconfirmable) edits, which the viewer never discards; an attached
//!   RoboCAD keeps its edits, and the answer and status line say so
//!   (`CadDocument::leaving_note`). There is no "discard" answer: RoboCAD's
//!   Save / Discard / Cancel prompt is a rejected design here
//!   (native-viewer.md, CAD mode). New checks the rule before RoboCAD
//!   writes the file, so a refused open does not leave one behind, and a
//!   REST caller's answer is the open's outcome (`jobs::wait`).
//! - **Save and Save As** (`save`, also `cad_save`) send `POST
//!   /save/thumbnail`, as RoboCAD's desktop Save and Save As both write the
//!   thumbnail (`MainWindow.save`/`save_as`: `doc.save(…,
//!   thumbnail=self.thumbnail())`; a plain `/save` would drop the file's
//!   thumbnail). A save to a path makes it the document's file in RoboCAD
//!   (`Document.save` sets `path`), so a self-started document's target
//!   follows it once the save succeeds (`Edit::retarget`, set on the edit
//!   this call started).
//! - **Document edits** (open, import, save as) are one RoboCAD call each
//!   through `actions::edit` / `cad_open`; **writes** that leave the
//!   document as it is (export, render, new) run on `Pool::Dedicated` jobs
//!   marked `complete_on_drop` (they finish, and log their outcome, even
//!   after CAD mode closes); the unit guess and the listing are reads.
//!   REST callers wait for the answer.
//! - **No cancellation of a sent export or render**: api.py has no cancel
//!   route for `/export` or `/render`; RoboCAD runs them to the end (under
//!   its document lock headless, on its Qt thread with a window). A REST
//!   caller may stop waiting; the outcome still lands in
//!   `cad_state.files.last` and the status line.
//! - **Render** is `GET /render` (headless: the snapshot renderer; with a
//!   window: the GPU viewport for plain shaded views, else a snapshot copy),
//!   written to a PNG by the job. `/capture` and `/screenshot` need
//!   RoboCAD's window (409 headless) and capture RoboCAD's own viewport, so
//!   they are deliberately not used: the native view is this window's.
//!   The Blender live link and web share (`bridge.*`) are desktop-only
//!   servers started by RoboCAD's window, deliberately not ported.
mod formats;
mod form;
mod jobs;
#[cfg(test)]
mod tests;

use crate::app::actions::{Call, Spec, spec};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::cad::actions::{CAD, CadAction, Cx};
use crate::cad::document::{CadDocument, CadTarget, EditDone};
use crate::cad::sync::value;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{FILE_TIMEOUT, IMPORT_EXTENSIONS, IMPORT_UNITS, MESH_EXTENSIONS, RENDER_MODES, RENDER_VIEWS, RenderRequest, extension};
use std::path::PathBuf;
use std::collections::BTreeMap;

pub(crate) use form::FileForm;
pub(crate) use jobs::FileJob;
use crate::ui_kit::path_field::Listing;

/// The file forms, exports and renders in flight.
#[derive(Resource, Default)]
pub struct CadFiles {
    /// The open path form (modal), if any.
    pub(crate) form: Option<FileForm>,
    /// Writes and reads in flight (export, render, new, unit guess).
    pub(crate) jobs: Vec<FileJob>,
    /// Finished jobs' outcomes for their waiting REST callers, by sequence,
    /// with what follows them (new's open runs in its caller's wait).
    pub(crate) results: BTreeMap<u64, (Result<Value, String>, jobs::Then)>,
    /// The last finished job: its label and outcome (`cad_state.files.last`).
    pub(crate) last: Option<(String, Result<Value, String>)>,
    /// The last unit guess: the mesh path and RoboCAD's answer.
    pub(crate) guess: Option<(String, Result<Value, String>)>,
    /// The export settings last sent per format (RoboCAD's desktop keeps
    /// them in its preferences, `export_settings`); the form starts from them.
    pub(crate) export_settings: BTreeMap<String, Map<String, Value>>,
    /// The form's directory listing (`Pool::Io`, `path_field::request`),
    /// and the newest one read (`path_field::receive`).
    pub(crate) listing: crate::jobs::Latest<Listing>,
    pub(crate) listed: Option<Listing>,
}

/// `cad_file`'s operations.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FileOp {
    /// RoboCAD's New: an empty `.rcad` at `path` (`POST /new`), then opened.
    #[default]
    New,
    /// RoboCAD's Open: `cad_open` of `path` (a `.rcad`).
    Open,
    /// RoboCAD's Save As: `POST /save/thumbnail {path}` (a `.rcad`).
    SaveAs,
    /// RoboCAD's Import: `POST /import {path, unit}` (unit for meshes).
    Import,
    /// The mesh unit prompt's guess: `GET /import/units?path=`.
    GuessUnit,
    /// Close the open path form.
    Close,
}
impl FileOp {
    pub fn name(self) -> &'static str {
        match self {
            FileOp::New => "new",
            FileOp::Open => "open",
            FileOp::SaveAs => "save_as",
            FileOp::Import => "import",
            FileOp::GuessUnit => "guess_unit",
            FileOp::Close => "close",
        }
    }
    /// RoboCAD's command label (`file.*`).
    pub fn label(self) -> &'static str {
        match self {
            FileOp::New => "New",
            FileOp::Open => "Open…",
            FileOp::SaveAs => "Save As…",
            FileOp::Import => "Import…",
            FileOp::GuessUnit => "Guess unit",
            FileOp::Close => "Close",
        }
    }
}

/// `cad_file`'s arguments. Without `path`, new, open, save as and import
/// open the path form.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct FileArgs {
    pub op: FileOp,
    #[serde(default)]
    pub path: Option<String>,
    /// Import: a mesh's unit (mm | cm | m | in | ft).
    #[serde(default)]
    pub unit: Option<String>,
}

/// `cad_export`'s arguments. Without `format` or `path`, the export form opens.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ExportArgs {
    /// `POST /export`'s format: stl | 3mf | step | iges | obj | svg | drawing.
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    /// The format's settings (`formats::FORMATS`); absent ones take RoboCAD's defaults.
    #[serde(default)]
    pub settings: Map<String, Value>,
    /// Only these nodes (all visible bodies when absent).
    #[serde(default)]
    pub ids: Option<Vec<String>>,
}

/// `cad_render`'s arguments: `GET /render`'s query and the PNG to write.
/// Without `path`, the render form opens.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct RenderArgs {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub w: Option<u32>,
    #[serde(default)]
    pub h: Option<u32>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<String>>,
    #[serde(default)]
    pub highlight: Option<Vec<String>>,
    #[serde(default)]
    pub labels: Option<bool>,
    #[serde(default)]
    pub edges: Option<bool>,
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default)]
    pub tolerance: Option<f64>,
    #[serde(default)]
    pub title: Option<String>,
}

/// The render size RoboCAD's software renderer is asked for, at most (px a side).
const MAX_RENDER: u32 = 8192;

// ---- Paths ---------------------------------------------------------------------

/// The document's file: RoboCAD's path, else the opened target's.
fn doc_path(doc: &CadDocument) -> Option<String> {
    doc.health.as_ref().and_then(|h| h.path.clone()).or_else(|| match &doc.target {
        CadTarget::File(p) => Some(p.display().to_string()),
        CadTarget::Service(_) => None,
    })
}

/// The directory the forms start in (the document's, else home) with a
/// trailing `/`, and the document's file stem ("untitled").
pub(crate) fn start_dir(doc: &CadDocument) -> (String, String) {
    let path = doc_path(doc).map(std::path::PathBuf::from);
    let dir = path.as_ref().and_then(|p| p.parent()).filter(|d| d.is_absolute()).map(|d| d.display().to_string()).or_else(|| std::env::var("HOME").ok()).unwrap_or_else(|| "/".into());
    let stem = path.as_ref().and_then(|p| p.file_stem()).map_or_else(|| "untitled".to_string(), |s| s.to_string_lossy().into_owned());
    (if dir.ends_with('/') { dir } else { format!("{dir}/") }, stem)
}

/// `path` with a leading `~/` expanded; Err unless absolute (RoboCAD
/// resolves a relative path against its own working directory, not the
/// viewer's).
pub(crate) fn absolute(path: &str, what: &str) -> Result<String, String> {
    let path = path.trim();
    let expanded = match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{}/{rest}", home.trim_end_matches('/')),
        _ => path.to_string(),
    };
    if expanded.is_empty() {
        return Err(format!("{what} needs a path"));
    }
    if !std::path::Path::new(&expanded).is_absolute() {
        return Err(format!("{what}: {expanded} is not an absolute path (RoboCAD would resolve it against its own working directory)"));
    }
    if expanded.ends_with('/') {
        return Err(format!("{what}: {expanded} names a directory; add a file name"));
    }
    Ok(expanded)
}

/// What the document supplies to export defaults.
fn export_context(doc: &CadDocument, selection: &[sim_runtime::cad_client::SelectionItem], display: Option<&crate::cad::display::CadDisplay>) -> formats::Context {
    let sketch = crate::cad::selection::CadItems::nodes(selection).into_iter().find(|id| doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == *id)).is_some_and(|n| n.kind == "sketch"));
    let title = doc_path(doc).and_then(|p| std::path::Path::new(&p).file_name().map(|f| f.to_string_lossy().into_owned())).unwrap_or_else(|| "untitled".into());
    let section = display.filter(|d| d.section.enabled).and_then(|d| d.section.plane).map(|p| json!({"origin": p.origin, "normal": p.normal, "x_axis": p.x_axis}));
    formats::Context { sketch, title, section }
}

// ---- The handler ------------------------------------------------------------------

pub(in crate::cad) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    if let Some(seq) = call.continuation.get("file_job").and_then(Value::as_u64) {
        return jobs::wait(cx, call, seq);
    }
    let ui = matches!(call.origin, crate::app::actions::Origin::Ui);
    let outcome = match action {
        CadAction::CadFile(args) => file(args, call, cx),
        CadAction::CadExport(args) => export(args, call, cx),
        CadAction::CadRender(args) => render(args, call, cx),
        _ => Outcome::Done(Err("not a file action".into())),
    };
    // A click's refusal shows in the open form too (the status line has it as well).
    if let (true, Outcome::Done(Err(e)), Some(files)) = (ui, &outcome, cx.files.as_deref_mut())
        && let Some(form) = files.form.as_mut()
    {
        form.error = Some(e.clone());
    }
    outcome
}

/// The resource, or the refusal when CAD mode has no file workflows (no window).
fn files<'a>(cx: &'a mut Cx) -> Result<&'a mut CadFiles, String> {
    cx.files.as_deref_mut().ok_or_else(|| "CAD mode's file workflows are not available in this window (no CAD panels)".to_string())
}

fn file(args: &FileArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let done = Outcome::Done;
    let what = args.op.label();
    if args.op == FileOp::Close {
        return done(files(cx).map(|f| json!({"closed": f.form.take().map(|form| form.title())})));
    }
    let Some(path) = args.path.as_deref() else {
        if args.op == FileOp::GuessUnit {
            return done(Err("guess_unit needs path (a mesh file)".into()));
        }
        return done(open_form(cx, form::Kind::File(args.op), None));
    };
    match args.op {
        FileOp::Open => {
            let path = match absolute(path, what) {
                Ok(p) => p,
                Err(e) => return done(Err(e)),
            };
            // cad_open's rule (`switch_blockers`) and note (`leaving_note`).
            let open = CadAction::CadOpen { path: Some(path.into()), url: None };
            let outcome = crate::cad::actions::handle(&open, call, cx);
            close_unless_refused(cx, outcome)
        }
        FileOp::New => {
            let path = match absolute(path, what).and_then(|p| if p.ends_with(".rcad") { Ok(p) } else { Err(format!("{what}: {p} is not a .rcad file")) }) {
                Ok(p) => p,
                Err(e) => return done(Err(e)),
            };
            // Refused now rather than after the file is written: opening it would be.
            let blockers = cx.doc.switch_blockers();
            if !blockers.is_empty() {
                return done(Err(format!("Not creating {path}: {}", blockers.join("; "))));
            }
            let client = match jobs::client(cx.doc) {
                Ok(c) => c,
                Err(e) => return done(Err(format!("Not creating {path}: {e}"))),
            };
            let label = format!("Create {path}");
            let (p, l) = (path.clone(), label.clone());
            let then = jobs::Then::Open { path: path.clone() };
            jobs::start(cx, call, "new", label, true, then, move |_| {
                jobs::logged(&l, client.new_file(&p).map(|n| json!({"created": n.created, "message": format!("Created {}; opening it", n.created)})).map_err(|e| jobs::named(&l, &e)))
            })
        }
        FileOp::SaveAs => {
            let path = match absolute(path, what) {
                // RoboCAD's Save As appends .rcad (`MainWindow.save_as`).
                Ok(p) if p.ends_with(".rcad") => p,
                Ok(p) => format!("{p}.rcad"),
                Err(e) => return done(Err(e)),
            };
            let outcome = save(cx.doc, call, Some(path));
            close_unless_refused(cx, outcome)
        }
        FileOp::Import => {
            let (path, unit) = match import_args(path, args.unit.as_deref()) {
                Ok(v) => v,
                Err(e) => return done(Err(e)),
            };
            let name = std::path::Path::new(&path).file_name().map_or_else(|| path.clone(), |f| f.to_string_lossy().into_owned());
            let label = match &unit {
                Some(u) => format!("Import {name} ({u})"),
                None => format!("Import {name}"),
            };
            let outcome = crate::cad::actions::edit(cx.doc, call, label, move |c| {
                c.clone().with_timeout(FILE_TIMEOUT).import(&path, unit.as_deref()).map(|i| EditDone { message: format!("Imported {name}: {} new node(s)", i.imported.len()), result: value(&i) })
            });
            close_unless_refused(cx, outcome)
        }
        FileOp::GuessUnit => {
            let path = match absolute(path, "guess_unit") {
                Ok(p) if MESH_EXTENSIONS.contains(&extension(&p).as_str()) => p,
                Ok(p) => return done(Err(format!("{p}: not a mesh file ({}); only meshes ask for units", MESH_EXTENSIONS.join(", ")))),
                Err(e) => return done(Err(e)),
            };
            let client = match jobs::client(cx.doc) {
                Ok(c) => c,
                Err(e) => return done(Err(format!("Not guessing the unit of {path}: {e}"))),
            };
            let label = format!("Guess the unit of {path}");
            let (p, l) = (path.clone(), label.clone());
            jobs::start(cx, call, "guess_unit", label, false, jobs::Then::Guess { path }, move |_| client.mesh_units(&p).map(|u| value(&u)).map_err(|e| jobs::named(&l, &e)))
        }
        FileOp::Close => unreachable!("handled above"),
    }
}

/// Save (`path` None: RoboCAD's own path) and Save As, for `cad_save`
/// and File > Save As…: `POST /save/thumbnail` on the edit path, as
/// RoboCAD's desktop saves (with the thumbnail; see the module doc). The
/// answer is `{saved, thumbnail}`, a superset of `POST /save`'s. A save to
/// `path` (absolute: the caller checks) retargets a self-started document
/// to it once RoboCAD saved (`sync::finish_edit`), marked on the edit this
/// call started (`edit_seq` moved), never on another one.
pub(in crate::cad) fn save(doc: &mut CadDocument, call: &mut Call, path: Option<String>) -> Outcome {
    // RoboCAD's Save As appends .rcad (`MainWindow.save_as`); `cad_save
    // {path}` too, or the document would follow a path cad_open refuses.
    let path = path.map(|p| if p.ends_with(".rcad") { p } else { format!("{p}.rcad") });
    let label = path.as_ref().map_or_else(|| "Save".to_string(), |p| format!("Save as {p}"));
    let sent = path.clone();
    let before = doc.edit_seq;
    let outcome = crate::cad::actions::edit(doc, call, label, move |c| {
        c.clone().with_timeout(FILE_TIMEOUT).save_with_thumbnail(sent.as_deref()).map(|s| EditDone {
            message: if s.thumbnail { format!("Saved {} with its thumbnail", s.saved) } else { format!("Saved {} (without a thumbnail: RoboCAD could not draw one)", s.saved) },
            result: value(&s),
        })
    });
    if doc.edit_seq != before {
        // cad-physical-inspect: the live link re-exports once this save succeeds.
        crate::cad::results::note_save(doc, path.as_deref());
        if let (Some(edit), Some(path)) = (doc.edit.as_mut(), path) {
            edit.retarget = Some(PathBuf::from(path));
        }
    }
    outcome
}

/// Closes the path form once its action was accepted (a refusal keeps it
/// open, with the refusal in it).
fn close_unless_refused(cx: &mut Cx, outcome: Outcome) -> Outcome {
    if !matches!(outcome, Outcome::Done(Err(_)))
        && let Some(f) = cx.files.as_deref_mut()
    {
        f.form = None;
    }
    outcome
}

/// An import's checked path and unit: a file RoboCAD imports, a unit for
/// a mesh (RoboCAD's prompt asks for it) and none for anything else.
pub(crate) fn import_args(path: &str, unit: Option<&str>) -> Result<(String, Option<String>), String> {
    let path = absolute(path, "Import")?;
    let ext = extension(&path);
    if !IMPORT_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("Import: {path}: RoboCAD imports {}", IMPORT_EXTENSIONS.join(", ")));
    }
    let mesh = MESH_EXTENSIONS.contains(&ext.as_str());
    match (mesh, unit) {
        (true, None) => Err(format!("Import {path}: a mesh needs unit ({}), as RoboCAD's unit prompt asks; cad_file {{op: guess_unit, path}} answers RoboCAD's guess", IMPORT_UNITS.join(", "))),
        (true, Some(u)) if !IMPORT_UNITS.contains(&u) => Err(format!("Import {path}: unit {u:?} is not one of {}", IMPORT_UNITS.join(", "))),
        (true, Some(u)) => Ok((path, Some(u.to_string()))),
        (false, Some(u)) => Err(format!("Import {path}: unit {u:?} applies to meshes only ({}); RoboCAD reads this file's own units", MESH_EXTENSIONS.join(", "))),
        (false, None) => Ok((path, None)),
    }
}

/// Opens the path form of `kind` (export: with `format` chosen).
fn open_form(cx: &mut Cx, kind: form::Kind, format: Option<&str>) -> Result<Value, String> {
    let (dir, stem) = start_dir(cx.doc);
    let context = export_context(cx.doc, &cx.shared.items(), cx.display.as_deref());
    // The form's field takes the keyboard (`form::input`), which ends any other field's entry.
    let files = files(cx)?;
    let form = FileForm::new(kind, &dir, &stem, format, &context, &files.export_settings)?;
    let shown = form.json();
    files.form = Some(form);
    // One modal path form at a time: a results, identification or export form closes (cad-physical-inspect).
    cx.doc.results.form = None;
    Ok(json!({"opened": shown}))
}

fn export(args: &ExportArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let done = Outcome::Done;
    let (Some(format), Some(path)) = (args.format.as_deref(), args.path.as_deref()) else {
        if let Some(f) = args.format.as_deref().filter(|f| formats::format(f).is_none()) {
            return done(Err(formats::unknown(f)));
        }
        return done(open_form(cx, form::Kind::Export, args.format.as_deref()));
    };
    let Some(fmt) = formats::format(format) else { return done(Err(formats::unknown(format))) };
    let what = format!("Export {}", fmt.label);
    let path = match absolute(path, &what) {
        Ok(p) if formats::extension_fits(fmt, &p) => p,
        Ok(p) => return done(Err(format!("{what}: {p} does not end in .{} (RoboCAD names the format by the extension)", fmt.extensions.join(" or .")))),
        Err(e) => return done(Err(e)),
    };
    let context = export_context(cx.doc, &cx.shared.items(), cx.display.as_deref());
    let settings = match formats::settings(fmt, &args.settings, &context) {
        Ok(s) => s,
        Err(e) => return done(Err(format!("{what} to {path}: {e}"))),
    };
    let client = match jobs::client(cx.doc) {
        Ok(c) => c,
        Err(e) => return done(Err(format!("{what} to {path}: {e}"))),
    };
    if let Ok(f) = files(cx) {
        f.export_settings.insert(fmt.id.to_string(), settings.clone());
    }
    let request = sim_runtime::cad_client::ExportRequest { format: fmt.id.to_string(), path: path.clone(), settings: Some(Value::Object(settings)), ids: args.ids.clone() };
    let label = format!("{what} to {path}");
    let l = label.clone();
    let format_label = fmt.label;
    jobs::start(cx, call, "export", label, true, jobs::Then::Nothing, move |_| {
        let answer = client.export(&request).map_err(|e| jobs::named(&l, &e)).map(|x| {
            // The status line shows the message: the warnings themselves (the first few), not a pointer elsewhere.
            let listed: Vec<String> = x.warnings.as_array().into_iter().flatten().map(|w| w.as_str().map_or_else(|| w.to_string(), str::to_string)).collect();
            let shown: Vec<&str> = listed.iter().take(3).map(String::as_str).collect();
            let more = if listed.len() > shown.len() { format!("; … {} more", listed.len() - shown.len()) } else { String::new() };
            let note = if listed.is_empty() { String::new() } else { format!(" ({} warning(s): {}{more})", listed.len(), shown.join("; ")) };
            json!({"exported": x.exported, "format": request.format, "warnings": x.warnings, "settings": request.settings, "message": format!("Exported {format_label} to {}{note}", x.exported)})
        });
        jobs::logged(&l, answer)
    })
}

/// A render's checked query.
pub(crate) fn render_request(args: &RenderArgs) -> Result<RenderRequest, String> {
    if let Some(view) = &args.view {
        let vector = view.split(',').count() == 3 && view.split(',').all(|x| x.trim().parse::<f64>().is_ok_and(f64::is_finite));
        if !RENDER_VIEWS.contains(&view.as_str()) && !vector {
            return Err(format!("Render: view {view:?} is one of {} or \"dx,dy,dz\"", RENDER_VIEWS.join(", ")));
        }
    }
    if let Some(mode) = &args.mode
        && !RENDER_MODES.contains(&mode.as_str())
    {
        return Err(format!("Render: mode {mode:?} is one of {}", RENDER_MODES.join(", ")));
    }
    for (name, v) in [("w", args.w), ("h", args.h)] {
        if let Some(v) = v
            && !(16..=MAX_RENDER).contains(&v)
        {
            return Err(format!("Render: {name} {v} is outside 16…{MAX_RENDER} px"));
        }
    }
    if let Some(section) = &args.section {
        let ok = section.split_once(':').is_some_and(|(axis, value)| matches!(axis, "x" | "y" | "z") && (value.is_empty() || value.trim().parse::<f64>().is_ok_and(f64::is_finite)));
        if !ok {
            return Err(format!("Render: section {section:?} is \"x|y|z:value\" (mm)"));
        }
    }
    if let Some(t) = args.tolerance
        && !(t.is_finite() && t > 0.0)
    {
        return Err(format!("Render: tolerance {t} must be a positive length (mm)"));
    }
    Ok(RenderRequest {
        view: args.view.clone(),
        w: args.w,
        h: args.h,
        mode: args.mode.clone(),
        section: args.section.clone(),
        ids: args.ids.clone(),
        highlight: args.highlight.clone(),
        labels: args.labels,
        edges: args.edges,
        focus: args.focus.clone(),
        tolerance: args.tolerance,
        title: args.title.clone(),
    })
}

fn render(args: &RenderArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let done = Outcome::Done;
    let Some(path) = args.path.as_deref() else { return done(open_form(cx, form::Kind::Render, None)) };
    let path = match absolute(path, "Render") {
        Ok(p) if extension(&p) == "png" => p,
        Ok(p) => return done(Err(format!("Render: {p} does not end in .png (RoboCAD renders PNG)"))),
        Err(e) => return done(Err(e)),
    };
    let request = match render_request(args) {
        Ok(r) => r,
        Err(e) => return done(Err(e)),
    };
    let client = match jobs::client(cx.doc) {
        Ok(c) => c,
        Err(e) => return done(Err(format!("Render to {path}: {e}"))),
    };
    let label = format!("Render to {path}");
    let l = label.clone();
    jobs::start(cx, call, "render", label, true, jobs::Then::Nothing, move |_| {
        let answer = client.render(&request).map_err(|e| jobs::named(&l, &e)).and_then(|png| {
            std::fs::write(&path, &png).map_err(|e| format!("{l}: could not write {path}: {e}"))?;
            Ok(json!({"rendered": path, "bytes": png.len(), "query": request.route(), "message": format!("Rendered {path} ({} KB)", png.len().div_ceil(1024))}))
        });
        jobs::logged(&l, answer)
    })
}

// ---- Wiring -------------------------------------------------------------------

/// The action RoboCAD's command `id` stands for (its menu entry and key),
/// for the file commands this part runs: the form opens.
pub(in crate::cad) fn command_action(id: &str) -> Option<CadAction> {
    let file = |op| Some(CadAction::CadFile(FileArgs { op, ..Default::default() }));
    match id {
        "file.new" => file(FileOp::New),
        "file.open" => file(FileOp::Open),
        "file.save_as" => file(FileOp::SaveAs),
        "file.import" => file(FileOp::Import),
        "file.export" => Some(CadAction::CadExport(ExportArgs::default())),
        "file.export_drawing" => Some(CadAction::CadExport(ExportArgs { format: Some("drawing".into()), ..Default::default() })),
        _ => None,
    }
}

/// CadPlugin: `CadFiles` on entering CAD mode (removed by `cad::clear`;
/// write jobs complete on drop), the form's text field and input (before
/// the CAD keys, so the frame's keys see its field typing), the jobs'
/// results, the form and the progress strip.
pub(in crate::cad) fn build(app: &mut App) {
    use crate::ui_kit::text::{TextField, TextFieldApp};
    app.add_text_field(form::FILES, TextField::new("Path form field").sticky());
    app.add_systems(OnEnter(ModeScope::Cad), |mut commands: Commands| commands.insert_resource(CadFiles::default())).add_systems(
        Update,
        (
            form::input.after(crate::app::actions::serve).before(crate::cad::keys::keys).in_set(ViewerSet::Input),
            // Before the edits' results: a finished edit's status line (`sync::finish_edit`) is
            // the one `views::sync` reads as a view save's answer (found by review).
            jobs::receive.before(crate::cad::sync::receive).in_set(ViewerSet::JobResults),
            (form::draw, jobs::strip).in_set(ViewerSet::Present),
        )
            .run_if(in_state(ViewerMode::Cad)),
    );
}

/// This part's REST commands (appended to `CadAction::commands`).
pub(in crate::cad) fn specs() -> Vec<Spec> {
    let formats: Vec<String> = formats::FORMATS
        .iter()
        .map(|f| {
            let settings: Vec<String> = f.settings.iter().map(|s| if s.default.is_empty() { format!("{} ({})", s.name, s.label) } else { format!("{} ({}, default {})", s.name, s.label, s.default) }).collect();
            format!("{} [.{}]: {}", f.id, f.extensions.join(", ."), if settings.is_empty() { "no settings".to_string() } else { settings.join("; ") })
        })
        .collect();
    vec![
        spec("cad_file", CAD, json!({"op": "save_as", "path": "/tmp/turntable-copy.rcad"}), "CAD mode: RoboCAD's file commands. op new (an empty .rcad written at path by RoboCAD's POST /new, then opened), open (a .rcad, as cad_open), save_as (POST /save/thumbnail: RoboCAD saves to path with the desktop's thumbnail; .rcad is appended as RoboCAD's Save As does), import (POST /import; a mesh, .stl .obj .3mf .fbx .ply .glb .gltf, needs unit mm | cm | m | in | ft as RoboCAD's unit prompt asks, refused without one; STEP, IGES, SVG and images take none), guess_unit (RoboCAD's unit guess for a mesh path, GET /import/units; the form asks it as soon as its path names a mesh and fills the unit unless one was chosen), close (the path form). Without path, new, open, save_as and import open the path form (pre-filled with the document's directory, listing its matching files). Paths are absolute (~/ is expanded). New and open replace the document under cad_open's rule, so no edits are lost (RoboCAD opens another window instead): refused by name on an edit in flight or a self-started service's unsaved or unconfirmable edits (save first); an attached RoboCAD keeps its unsaved edits, and the answer's message says so. New checks that rule before RoboCAD writes the file. Open, save_as and import are one RoboCAD call each through the edit path (REST callers get RoboCAD's answer); new and guess_unit run on jobs and REST callers wait for them: new answers once the created file's open was accepted or refused ({created, opened, generation, message}; the connection then shows in cad_state, as after cad_open)."),
        spec(
            "cad_export",
            CAD,
            json!({"format": "step", "path": "/tmp/turntable.step", "settings": {"schema": "AP214"}}),
            format!("CAD mode: POST /export on a job: RoboCAD writes format to path (absolute; its extension must be the format's) with settings (checked here with the desktop dialog's ranges; absent ones take RoboCAD's defaults and are sent explicitly; the sent settings are remembered per format for the form) for ids (all visible bodies when absent). Formats and settings: {}. The sketch SVG's sketch defaults to the selected sketch; the drawing's title to the document's file name and its section to the section tool's plane while it is on. Without format or path, the export form opens (file.export; file.export_drawing opens it on the drawing). A failure is a named refusal (\"Export STEP to …: RoboCAD answered 422: …\"). Not cancellable: api.py has no cancel route for /export and RoboCAD runs it to the end; REST callers wait (cancelling only stops the wait), and the job completes even if CAD mode closes. Progress: cad_state.files.jobs and the window's job strip.", formats.join(" | ")),
        ),
        spec("cad_render", CAD, json!({"path": "/tmp/turntable-iso.png", "view": "iso", "w": 1200, "h": 900}), format!("CAD mode: GET /render on a job, the PNG written to path (absolute, .png). Query as RoboCAD's: view ({} or \"dx,dy,dz\"), w and h (16…{MAX_RENDER} px; RoboCAD's default 1200×900), mode ({}), section (\"x|y|z:value\" mm), ids, highlight, labels, edges, focus (a node to frame), tolerance (mm), title; absent ones take RoboCAD's defaults. Works headless (the snapshot renderer); with RoboCAD's window a plain shaded view (no ids, highlight, labels or other mode) is drawn by its GPU viewport at the viewport's size, and w, h, edges, tolerance and title then do not apply (api.py render_request). Without path, the render form opens. Not cancellable once sent (no cancel route); REST callers wait. /capture and /screenshot are not used: they need RoboCAD's window and capture its own viewport.", RENDER_VIEWS.join(", "), RENDER_MODES.join(", "))),
    ]
}

/// This part's `system_ui` controls: (id, label, action, ready). A form
/// opens whenever asked; RoboCAD is needed when it runs.
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    control_list(cx.doc, cx.files.as_deref())
}

/// [`controls`] from the document and the file state.
fn control_list(doc: &CadDocument, files: Option<&CadFiles>) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let connected = || if doc.connected() { Ok(()) } else { Err(format!("not connected to RoboCAD: {}", doc.connection_line().0)) };
    let editable = || doc.edit_refusal().map_or(Ok(()), Err);
    let file = |op| CadAction::CadFile(FileArgs { op, ..Default::default() });
    let mut out = vec![
        ("cad:file:new".to_string(), "New".to_string(), file(FileOp::New), connected()),
        ("cad:file:open".to_string(), "Open…".to_string(), file(FileOp::Open), Ok(())),
        ("cad:file:save_as".to_string(), "Save As…".to_string(), file(FileOp::SaveAs), editable()),
        ("cad:file:import".to_string(), "Import…".to_string(), file(FileOp::Import), editable()),
        ("cad:file:export".to_string(), "Export…".to_string(), CadAction::CadExport(ExportArgs::default()), connected()),
        ("cad:file:export_drawing".to_string(), "Export drawing (SVG)…".to_string(), CadAction::CadExport(ExportArgs { format: Some("drawing".into()), ..Default::default() }), connected()),
        ("cad:file:render".to_string(), "Render (PNG)…".to_string(), CadAction::CadRender(RenderArgs::default()), connected()),
    ];
    if let Some(form) = files.and_then(|f| f.form.as_ref()) {
        out.push(("cad:file:close".to_string(), format!("Close: {}", form.title()), file(FileOp::Close), Ok(())));
    }
    out
}

/// `cad_state.files`: the open form (fields, drafts, listing), the jobs in
/// flight with their progress, the last outcome, the last unit guess and
/// the export settings remembered per format.
pub(in crate::cad) fn state_json(files: &CadFiles) -> Value {
    let outcome = |r: &Result<Value, String>| match r {
        Ok(v) => json!({"ok": true, "value": v}),
        Err(e) => json!({"ok": false, "error": e}),
    };
    let mut out = Map::new();
    out.insert("form".into(), files.form.as_ref().map_or(Value::Null, |f| f.json()));
    out.insert("jobs".into(), Value::Array(files.jobs.iter().map(FileJob::json).collect()));
    out.insert("last".into(), files.last.as_ref().map_or(Value::Null, |(label, r)| json!({"label": label, "outcome": outcome(r)})));
    out.insert("guess".into(), files.guess.as_ref().map_or(Value::Null, |(path, r)| json!({"path": path, "outcome": outcome(r)})));
    out.insert("listing".into(), files.listed.as_ref().map_or(Value::Null, Listing::json));
    out.insert("export_settings".into(), json!(files.export_settings));
    Value::Object(out)
}
