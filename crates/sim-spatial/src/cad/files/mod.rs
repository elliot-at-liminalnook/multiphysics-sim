//! Local path-picker Open shares CadAction::CadOpen with REST and startup.
//! Archive replacement is jobs-owned; failures/cancellation preserve the source.
//! New, Save As, Import (`sim_cad::import`: STEP, IGES, SVG, images; meshes
//! refused by name), Export (`sim_cad::export`) and Render (`sim_render::cad`)
//! all run in process on the open archive.
pub(super) mod form;
mod formats;
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
use crate::cad::types::{IMPORT_EXTENSIONS, IMPORT_UNITS, MESH_EXTENSIONS, RENDER_MODES, RENDER_VIEWS, RenderRequest, extension};
use std::path::PathBuf;
use std::collections::BTreeMap;

pub(crate) use form::FileForm;
pub(crate) use jobs::FileJob;
use crate::ui_kit::path_field::Listing;

/// The file forms, exports and renders in flight.
#[derive(Resource, Default)]
pub struct CadFiles {
    /// Monotonic local modal lifetime; drafts and focus do not change it.
    pub(crate) form_sequence: u64,
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
    /// Cancel export and render jobs in flight (`job`: that one; absent:
    /// every one); see the module doc for what a cancel can stop.
    Cancel,
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
            FileOp::Cancel => "cancel",
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
            FileOp::Cancel => "Cancel",
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
    /// Cancel: the job's sequence number (`cad_state.files.jobs[].seq`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<u64>,
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
fn export_context(doc: &CadDocument, selection: &[crate::cad::types::SelectionItem], display: Option<&crate::cad::display::CadDisplay>) -> formats::Context {
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
    // Open and New hand their REST caller to cad_open, which waits for the
    // load under its own key: keep forwarding there, never re-run the op.
    if call.continuation.get("local_open").is_some() {
        let created = call.continuation.get("created").and_then(Value::as_str).map(str::to_string);
        let open = CadAction::CadOpen { path: None, url: None };
        return match (crate::cad::actions::handle(&open, call, cx), created) {
            (Outcome::Pending, Some(path)) => {
                call.continuation["created"] = json!(path);
                Outcome::Pending
            }
            (outcome, None) => outcome,
            (outcome, Some(path)) => jobs::created(outcome, &path),
        };
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
    if args.op == FileOp::Cancel && cx.doc.local_load.is_some() {
        let seq = cx.doc.local_load.as_ref().map(|load| load.sequence).unwrap();
        crate::cad::sync::cancel_load(cx.doc, seq);
        return done(Ok(json!({"cancelled": seq, "message": "Local open cancelled; current document preserved"})));
    }
    let what = args.op.label();
    if args.op == FileOp::Close {
        return done(files(cx).map(|f| json!({"closed": f.form.take().map(|form| form.title())})));
    }
    if args.op == FileOp::Cancel {
        let result = files(cx).and_then(|f| jobs::cancel(f, args.job));
        if let Ok(v) = &result
            && let Some(message) = v.get("message").and_then(Value::as_str)
        {
            cx.doc.show(Ok(message.to_string()));
        }
        return done(result);
    }
    if args.job.is_some() {
        return done(Err(format!("{what}: job belongs to op cancel")));
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
            if std::path::Path::new(&path).exists() {
                return done(Err(format!("Not creating {path}: a file is already there (open it, or choose another name)")));
            }
            // The open document's stock materials carry over (RoboCAD's new document has its library's).
            let like = cx.doc.local.as_ref().map(|l| l.archive.manifest.clone());
            let label = format!("Create {path}");
            let (p, l) = (path.clone(), label.clone());
            let then = jobs::Then::Open { path: path.clone() };
            jobs::start(cx, call, "new", label, true, then, move |_| {
                let bytes = sim_cad::edit::empty_archive(like.as_ref()).map_err(|e| format!("{l}: {e}"))?;
                std::fs::write(&p, bytes).map_err(|e| format!("{l}: {e}"))?;
                jobs::logged(&l, Ok(json!({"created": p, "message": format!("Created {p}; opening it")})))
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
                c.import(&path, unit.as_deref()).map(|i| EditDone { message: format!("Imported {name}: {} new node(s)", i.imported.len()), result: value(&i) })
            });
            close_unless_refused(cx, outcome)
        }
        FileOp::GuessUnit => {
            let path = match absolute(path, "guess_unit") {
                Ok(p) if MESH_EXTENSIONS.contains(&extension(&p).as_str()) => p,
                Ok(p) => return done(Err(format!("{p}: not a mesh file ({}); only meshes ask for units", MESH_EXTENSIONS.join(", ")))),
                Err(e) => return done(Err(e)),
            };
            // RoboCAD's `GET /import/units`: the raw extent and the prompt's guess, read off the UI thread.
            let label = format!("Guess the units of {path}");
            let then = jobs::Then::Guess { path: path.clone() };
            jobs::start(cx, call, "guess_unit", label, false, then, move |_| {
                let (_, extent) = sim_cad::mesh::read_raw(std::path::Path::new(&path))?;
                Ok(json!({"path": path, "extent": extent, "guess": sim_cad::mesh::units_guess(extent), "units": IMPORT_UNITS}))
            })
        }
        FileOp::Close | FileOp::Cancel => unreachable!("handled above"),
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
    let _ = (label, call);
    // The open archive's bytes, written atomically in process (`local::save`;
    // a save to a path makes it the document's file). No thumbnail is drawn.
    Outcome::Done(crate::cad::local::save(doc, path.as_ref().map(PathBuf::from)))
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
    files.form_sequence = files.form_sequence.wrapping_add(1);
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
    let local = match jobs::local(cx.doc) {
        Ok(c) => c,
        Err(e) => return done(Err(format!("{what} to {path}: {e}"))),
    };
    if let Ok(f) = files(cx) {
        f.export_settings.insert(fmt.id.to_string(), settings.clone());
    }
    let request = crate::cad::types::ExportRequest { format: fmt.id.to_string(), path: path.clone(), settings: Some(Value::Object(settings)), ids: args.ids.clone() };
    let label = format!("{what} to {path}");
    let l = label.clone();
    let format_label = fmt.label;
    jobs::start(cx, call, "export", label, true, jobs::Then::Nothing, move |ctx| {
        // Written in process from the open archive's exact geometry (`sim_cad::export`).
        let settings = request.settings.clone().unwrap_or(Value::Null);
        let answer = sim_cad::export::export(&local.archive, &local.geometry, &request.format, std::path::Path::new(&request.path), &settings, request.ids.as_deref(), &|| ctx.cancelled()).map_err(|e| format!("{l}: {e}")).map(|x| {
            let listed: Vec<String> = x["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_string)).collect();
            let note = if listed.is_empty() { String::new() } else { format!(" ({} note(s): {})", listed.len(), listed.join("; ")) };
            json!({"exported": request.path, "format": request.format, "warnings": listed, "settings": request.settings, "bodies": x["bodies"], "message": format!("Exported {format_label} to {}{note}", request.path)})
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
    let local = match jobs::local(cx.doc) {
        Ok(c) => c,
        Err(e) => return done(Err(format!("Render to {path}: {e}"))),
    };
    let label = format!("Render to {path}");
    let l = label.clone();
    jobs::start(cx, call, "render", label, true, jobs::Then::Nothing, move |ctx| {
        let answer = render_local(&local, &request).map_err(|e| format!("{l}: {e}")).and_then(|png| {
            if ctx.cancelled() {
                return Err(format!("{l}: cancelled; nothing was written to {path}"));
            }
            std::fs::write(&path, &png).map_err(|e| format!("{l}: could not write {path}: {e}"))?;
            Ok(json!({"rendered": path, "bytes": png.len(), "query": request.route(), "message": format!("Rendered {path} ({} KB)", png.len().div_ceil(1024))}))
        });
        jobs::logged(&l, answer)
    })
}

/// A render of the open archive's exact tessellation (`sim_render::cad`):
/// the shown bodies (or `ids`), RoboCAD's view presets or "dx,dy,dz",
/// shaded | xray | wireframe, an "x|y|z:value" section, highlights, edges,
/// labels, a framed node and a title.
fn render_local(local: &crate::cad::sync::LocalSnapshot, r: &RenderRequest) -> Result<Vec<u8>, String> {
    let doc = &local.archive;
    let ids = sim_cad::export::bodies(doc, r.ids.as_deref());
    let tol = r.tolerance;
    let mut bodies = Vec::new();
    for id in &ids {
        let g = match (tol, local.geometry.iter().find(|b| &b.node_id == id)) {
            (None, Some(g)) => g.clone(),
            (t, _) => sim_cad::geometry::tessellate_node(doc, id, t.unwrap_or(0.05), &|| false)?,
        };
        let n = doc.node(id);
        let color = n.and_then(|n| sim_cad::ops::v3(&n["color"])).or_else(|| n.and_then(|n| n["material"].as_str()).and_then(|m| sim_cad::edit::material(&doc.manifest, m).and_then(|x| sim_cad::ops::v3(&x["color"])))).map_or([0.66, 0.70, 0.76], |c| c.map(|v| v as f32));
        bodies.push(sim_render::cad::Body { id: id.clone(), name: n.and_then(|n| n["name"].as_str()).unwrap_or(id).to_string(), vertices: g.vertices_mm, triangles: g.triangles, triangle_face: g.triangle_faces, color });
    }
    let view = match r.view.as_deref().unwrap_or("iso") {
        v if v.contains(',') => {
            let p: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            [p[0], p[1], p[2]]
        }
        v => sim_render::cad::preset(v).ok_or_else(|| format!("unknown view {v}"))?,
    };
    let section = match r.section.as_deref() {
        None => None,
        Some(s) => {
            let (axis, value) = s.split_once(':').ok_or("section is \"x|y|z:value\"")?;
            let a = ["x", "y", "z"].iter().position(|x| *x == axis).ok_or("section axis is x, y or z")?;
            let v = if value.is_empty() { 0. } else { value.trim().parse::<f64>().map_err(|e| e.to_string())? };
            Some((a, v))
        }
    };
    let o = sim_render::cad::Options {
        width: r.w.unwrap_or(1200),
        height: r.h.unwrap_or(900),
        view,
        mode: r.mode.clone().unwrap_or_else(|| "shaded".into()),
        section,
        highlight: r.highlight.clone().unwrap_or_default(),
        labels: r.labels.unwrap_or(false),
        edges: r.edges.unwrap_or(true),
        focus: r.focus.clone().into_iter().collect(),
        title: r.title.clone(),
    };
    sim_render::cad::render(&bodies, &o)
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
            // Before the public Escape steps (and so the Select tool's keys): its Escape closes the form and is consumed.
            form::input.in_set(crate::cad::CadKeySet::Focus).before(crate::cad::CadKeySet::EscapeTool),
            // Before the edits' results: a finished edit's status line (`sync::finish_edit`) is
            // the one `views::sync` reads as a view save's answer (found by review).
            jobs::receive.before(crate::cad::CadSet::Results).in_set(ViewerSet::JobResults),
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
        spec("cad_file", CAD, json!({"op": "save_as", "path": "/tmp/turntable-copy.rcad"}), "CAD mode: RoboCAD's file commands. op new (an empty .rcad written at path by RoboCAD's POST /new, then opened), open (a .rcad, as cad_open), save_as (POST /save/thumbnail: RoboCAD saves to path with the desktop's thumbnail; .rcad is appended as RoboCAD's Save As does), import (POST /import; a mesh, .stl .obj .3mf .fbx .ply .glb .gltf, needs unit mm | cm | m | in | ft as RoboCAD's unit prompt asks, refused without one; STEP, IGES, SVG and images take none), guess_unit (RoboCAD's unit guess for a mesh path, GET /import/units; the form asks it as soon as its path names a mesh and fills the unit unless one was chosen), close (the path form), cancel (job: a sequence number from cad_state.files.jobs, absent for every export and render in flight; RoboCAD has no cancel route, so a render's PNG is not written once the cancel is seen and an export's file is written by RoboCAD anyway, each outcome saying which). Without path, new, open, save_as and import open the path form (pre-filled with the document's directory, listing its matching files). Paths are absolute (~/ is expanded). New and open replace the document under cad_open's rule, so no edits are lost (RoboCAD opens another window instead): refused by name on an edit in flight or a self-started service's unsaved or unconfirmable edits (save first); an attached RoboCAD keeps its unsaved edits, and the answer's message says so. New checks that rule before RoboCAD writes the file. Open, save_as and import are one RoboCAD call each through the edit path (REST callers get RoboCAD's answer); new and guess_unit run on jobs and REST callers wait for them: new answers once the created file's open was accepted or refused ({created, opened, generation, message}; the connection then shows in cad_state, as after cad_open)."),
        spec(
            "cad_export",
            CAD,
            json!({"format": "step", "path": "/tmp/turntable.step", "settings": {"schema": "AP214"}}),
            format!("CAD mode: POST /export on a job: RoboCAD writes format to path (absolute; its extension must be the format's) with settings (checked here with the desktop dialog's ranges; absent ones take RoboCAD's defaults and are sent explicitly; the sent settings are remembered per format for the form) for ids (all visible bodies when absent). Formats and settings: {}. The sketch SVG's sketch defaults to the selected sketch; the drawing's title to the document's file name and its section to the section tool's plane while it is on. Without format or path, the export form opens (file.export; file.export_drawing opens it on the drawing). A failure is a named refusal (\"Export STEP to …: RoboCAD answered 422: …\"). RoboCAD runs a sent export to the end (api.py has no cancel route for /export): cad_file {{op: cancel, job}} (the job strip's Cancel) cannot stop it, and the outcome then says the file was written regardless; REST callers wait (cancelling the request only stops the wait), and the job completes even if CAD mode closes. Progress: cad_state.files.jobs and the window's job strip.", formats.join(" | ")),
        ),
        spec("cad_render", CAD, json!({"path": "/tmp/turntable-iso.png", "view": "iso", "w": 1200, "h": 900}), format!("CAD mode: GET /render on a job, the PNG written to path (absolute, .png). Query as RoboCAD's: view ({} or \"dx,dy,dz\"), w and h (16…{MAX_RENDER} px; RoboCAD's default 1200×900), mode ({}), section (\"x|y|z:value\" mm), ids, highlight, labels, edges, focus (a node to frame), tolerance (mm), title; absent ones take RoboCAD's defaults. Works headless (the snapshot renderer); with RoboCAD's window a plain shaded view (no ids, highlight, labels or other mode) is drawn by its GPU viewport at the viewport's size, and w, h, edges, tolerance and title then do not apply (api.py render_request). Without path, the render form opens. RoboCAD has no cancel route for /render, so a sent render is drawn to the end; cad_file {{op: cancel, job}} (the job strip's Cancel) keeps this window from writing the PNG once the cancel is seen (the outcome is a refusal saying nothing was written, or says the cancel came too late to stop the write); REST callers wait. /capture and /screenshot are not used: they need RoboCAD's window and capture its own viewport.", RENDER_VIEWS.join(", "), RENDER_MODES.join(", "))),
    ]
}

/// This part's `system_ui` controls: (id, label, action, ready). A form
/// opens whenever asked; RoboCAD is needed when it runs.
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    control_list(cx.doc, cx.files.as_deref())
}

/// [`controls`] from the document and the file state.
fn control_list(doc: &CadDocument, files: Option<&CadFiles>) -> Vec<(String, String, CadAction, Result<(), String>)> {
    // Export and render read the open archive; New writes a file and opens it.
    let connected = || if doc.local.is_some() { Ok(()) } else { Err("no CAD document is open".to_string()) };
    let editable = || doc.edit_refusal().map_or(Ok(()), Err);
    let file = |op| CadAction::CadFile(FileArgs { op, ..Default::default() });
    let mut out = vec![
        ("cad:file:new".to_string(), "New".to_string(), file(FileOp::New), Ok(())),
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
    // The progress strip's Cancel of each export and render in flight.
    for j in files.into_iter().flat_map(|f| f.jobs.iter()).filter(|j| jobs::cancellable(j.kind)) {
        let ready = if j.cancelled { Err(format!("cancel already asked for {}; its outcome follows when RoboCAD answers", j.label)) } else { Ok(()) };
        out.push((format!("cad:file:cancel-{}", j.seq), format!("Cancel: {}", j.label), CadAction::CadFile(FileArgs { op: FileOp::Cancel, job: Some(j.seq), ..Default::default() }), ready));
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
