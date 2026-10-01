//! The path form (RoboCAD's `QFileDialog` and its option dialogs, natively;
//! see the module doc in `files` for why not a native dialog): a modal kit
//! form with the path field, the operation's options (the import unit, an
//! export format's settings, a render's view, size and mode), the
//! directory's matching files from a `Pool::Io` listing, and OK / Cancel.
//!
//! - **Drafts are input editing** (native-viewer.md §3): typing, a choice
//!   or a checkbox changes the form's texts only; OK (and Enter) writes the
//!   one action REST takes (`CadFile`, `CadExport`, `CadRender`) with every
//!   value, Cancel (and Escape) writes `CadFile {op: close}`, "Guess unit"
//!   writes `CadFile {op: guess_unit}`. A refusal comes back into `error`
//!   (`files::handle`). New and open show `cad_open`'s rule as it stands
//!   (`switch_blockers`: why OK will be refused; `leaving_note`: where an
//!   attached RoboCAD's unsaved edits stay); there is no discard answer.
//! - **Mesh units**: as RoboCAD's import opens its unit prompt with a guess
//!   (`MainWindow.import_path`: `UnitsDialog(mesh_units_guess(extent))`),
//!   the form asks RoboCAD's guess (`CadFile {op: guess_unit}`, a job) as
//!   soon as its path names a mesh it has not asked for, and the guess
//!   fills the unit unless one was chosen by hand for that path. Until a
//!   guess landed or a unit was chosen, OK is disabled and says why: a
//!   mesh is never imported in a unit nobody picked.
//! - **Modal**: a dimmed backdrop takes every click, and the form's kit
//!   field ([`FILES`], editing the row `FileForm::focus` names) holds the
//!   keyboard while open (taking it ends any other field's entry).
//! - **Listing**: the kit path field's (`ui_kit::path_field`, the one way
//!   to enter a path): the path's directory and the operation's extensions
//!   are read on `Pool::Io` when they change (`path_field::request`), and
//!   `Kit::path_listing` draws them under the form; a directory entry
//!   descends (a write keeps the file name typed), a file entry fills the
//!   path, ".." goes up (`path_field::pick`/`up`). A write onto a listed
//!   file says it will be replaced (Qt's save dialog asks the same); New
//!   says RoboCAD refuses to replace it. The path input itself stays a kit
//!   form row.
use super::formats::{self, DRAWING_VIEWS, FORMAT_IDS, Kind as SettingKind};
use super::{CadFiles, ExportArgs, FileArgs, FileOp, RenderArgs};
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::ui_kit::form::{FieldKind, FieldValue, FormHit, FormRow, Unit, evaluate};
use crate::ui_kit::path_field::{self, PathHit};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextFocus};
use crate::ui_kit::{DANGER, Kit, Look, SUBTLE, UiFonts, WARN, size};
use bevy::ecs::system::ParamSet;
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use sim_runtime::cad_client::{IMPORT_EXTENSIONS, IMPORT_UNITS, MESH_EXTENSIONS, RENDER_MODES, RENDER_VIEWS, extension};
use std::collections::BTreeMap;

/// The path form's text field (`ui_kit::text`): the row it edits is
/// `FileForm::focus`. Sticky: a press on the form's choices, checkboxes
/// and listing keeps the typing, as before.
pub(in crate::cad) const FILES: FieldId = FieldId("cad.files");

/// What the form is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    File(FileOp),
    Export,
    Render,
}

/// The open path form: its drafts by field name.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FileForm {
    pub kind: Kind,
    pub texts: BTreeMap<String, String>,
    /// The text row the form's kit field edits ([`FILES`]).
    pub focus: Option<String>,
    /// The focused text is selected (the next key replaces it; mirrored
    /// from the kit field's draft).
    pub select_all: bool,
    /// Why OK sent nothing, or RoboCAD's refusal.
    pub error: Option<String>,
    /// The import unit was chosen by hand (a guess no longer replaces it).
    pub unit_touched: bool,
    /// The mesh path RoboCAD's unit guess was last asked for (`ask_guess`).
    pub guess_asked: Option<String>,
    /// The listing last asked for (`listing_key`).
    pub listing_asked: Option<String>,
    /// The section tool's plane was there when the form opened.
    pub section_available: bool,
}

/// One row of the form.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub name: String,
    pub label: String,
    pub kind: FieldKind,
}

const BINARY: &[&str] = &["binary", "ascii"];
const COUNT: FieldKind = FieldKind::Number { unit: Unit::Count, min: Some(16.0), max: Some(8192.0), decimals: 0 };

/// A setting's draft key.
fn key(format: &str, name: &str) -> String {
    format!("{format}.{name}")
}

/// A sent setting value as the form's text.
fn text_of(kind: SettingKind, v: &Value) -> String {
    match (kind, v) {
        (SettingKind::Binary, Value::Bool(b)) => if *b { "binary" } else { "ascii" }.into(),
        (_, Value::Bool(b)) => b.to_string(),
        (_, Value::String(s)) => s.clone(),
        (_, other) => other.to_string(),
    }
}

impl FileForm {
    /// A new form of `kind`, its path in `dir` (with a trailing `/`) named
    /// after the document's `stem`; an export form on `format` (else STL,
    /// the desktop's first filter), its settings as last sent (`remembered`)
    /// else RoboCAD's defaults and the document's (`cx`).
    pub(crate) fn new(kind: Kind, dir: &str, stem: &str, format: Option<&str>, cx: &formats::Context, remembered: &BTreeMap<String, Map<String, Value>>) -> Result<Self, String> {
        let mut texts = BTreeMap::new();
        let mut put = |k: &str, v: String| {
            texts.insert(k.to_string(), v);
        };
        match kind {
            Kind::File(FileOp::New) => put("path", format!("{dir}untitled.rcad")),
            Kind::File(FileOp::SaveAs) => put("path", format!("{dir}{stem}.rcad")),
            Kind::File(FileOp::Import) => {
                put("path", dir.to_string());
                // None until RoboCAD's guess or the user's choice (`ask_guess`).
                put("unit", String::new());
            }
            Kind::File(_) => put("path", dir.to_string()),
            Kind::Export => {
                let fmt = formats::format(format.unwrap_or("stl")).ok_or_else(|| formats::unknown(format.unwrap_or("")))?;
                put("format", fmt.id.into());
                let name = if fmt.id == "drawing" { format!("{stem}-drawing.svg") } else { format!("{stem}.{}", fmt.extensions[0]) };
                put("path", format!("{dir}{name}"));
                for f in formats::FORMATS {
                    let last = remembered.get(f.id);
                    for st in f.settings {
                        let sent = last.and_then(|m| m.get(st.name));
                        match st.kind {
                            SettingKind::Views => {
                                let views: Vec<String> = match sent.and_then(Value::as_array) {
                                    Some(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
                                    None => st.default.split(',').map(str::to_string).collect(),
                                };
                                for (id, _) in DRAWING_VIEWS {
                                    put(&key(f.id, &format!("view.{id}")), views.iter().any(|v| v == id).to_string());
                                }
                            }
                            // On when the section tool is (the desktop's rule), every time.
                            SettingKind::Section => put(&key(f.id, st.name), cx.section.is_some().to_string()),
                            SettingKind::Text if st.name == "title" => put(&key(f.id, st.name), cx.title.clone()),
                            SettingKind::Text => put(&key(f.id, st.name), cx.sketch.clone().unwrap_or_default()),
                            other => put(&key(f.id, st.name), sent.map_or_else(|| st.default.to_string(), |v| text_of(other, v))),
                        }
                    }
                }
            }
            Kind::Render => {
                for (k, v) in [("view", "iso"), ("w", "1200"), ("h", "900"), ("mode", "shaded"), ("edges", "true"), ("labels", "false")] {
                    put(k, v.into());
                }
                put("path", format!("{dir}{stem}-iso.png"));
            }
        }
        // The path takes the keyboard, the cursor after its text (a dialog's file name field).
        Ok(FileForm { kind, texts, focus: Some("path".into()), select_all: false, error: None, unit_touched: false, guess_asked: None, listing_asked: None, section_available: cx.section.is_some() })
    }

    pub(crate) fn text(&self, name: &str) -> &str {
        self.texts.get(name).map_or("", String::as_str)
    }

    /// The export format chosen.
    fn format(&self) -> Option<&'static formats::Format> {
        formats::format(self.text("format"))
    }

    /// The form's title: RoboCAD's command label, the desktop's "Export STL".
    pub(crate) fn title(&self) -> String {
        match self.kind {
            Kind::File(op) => op.label().to_string(),
            Kind::Export => format!("Export {}", self.format().map_or("…", |f| f.label)),
            Kind::Render => "Render (PNG)".into(),
        }
    }

    /// The rows shown now, in order.
    pub(crate) fn rows(&self) -> Vec<Row> {
        let row = |name: &str, label: &str, kind: FieldKind| Row { name: name.into(), label: label.into(), kind };
        let mut rows = Vec::new();
        match self.kind {
            Kind::File(op) => {
                let label = match op {
                    FileOp::New => "New document (.rcad, must not exist)",
                    FileOp::Open => "Open (.rcad)",
                    FileOp::SaveAs => "Save as (.rcad)",
                    _ => "File to import (STEP, IGES, mesh, SVG, image)",
                };
                rows.push(row("path", label, FieldKind::Text));
                if op == FileOp::Import && MESH_EXTENSIONS.contains(&extension(self.text("path")).as_str()) {
                    rows.push(row("unit", "Units of the mesh file", FieldKind::Choice { options: IMPORT_UNITS }));
                }
            }
            Kind::Export => {
                rows.push(row("format", "Format", FieldKind::Choice { options: FORMAT_IDS }));
                let fmt = self.format();
                let exts = fmt.map_or_else(String::new, |f| f.extensions.join(", ."));
                rows.push(row("path", &format!("File (.{exts})"), FieldKind::Text));
                for st in fmt.map(|f| f.settings).unwrap_or_default() {
                    let id = fmt.map_or("", |f| f.id);
                    match st.kind {
                        SettingKind::Views => {
                            for (v, label) in DRAWING_VIEWS {
                                rows.push(row(&key(id, &format!("view.{v}")), &format!("{label} view"), FieldKind::Check));
                            }
                        }
                        SettingKind::Section if !self.section_available => rows.push(row(&key(id, st.name), "Section A-A (turn the section tool on first)", FieldKind::Check)),
                        SettingKind::Section | SettingKind::Check => rows.push(row(&key(id, st.name), st.label, FieldKind::Check)),
                        SettingKind::Binary => rows.push(row(&key(id, st.name), st.label, FieldKind::Choice { options: BINARY })),
                        SettingKind::Choice(options) => rows.push(row(&key(id, st.name), st.label, FieldKind::Choice { options })),
                        SettingKind::Number { unit, min, max, decimals } => rows.push(row(&key(id, st.name), st.label, FieldKind::Number { unit, min: Some(min), max: Some(max), decimals })),
                        SettingKind::Text => rows.push(row(&key(id, st.name), st.label, FieldKind::Text)),
                    }
                }
            }
            Kind::Render => {
                rows.push(row("view", "View", FieldKind::Choice { options: RENDER_VIEWS }));
                rows.push(row("w", "Width (px)", COUNT));
                rows.push(row("h", "Height (px)", COUNT));
                rows.push(row("mode", "Mode", FieldKind::Choice { options: RENDER_MODES }));
                rows.push(row("edges", "Draw edges", FieldKind::Check));
                rows.push(row("labels", "Label nodes", FieldKind::Check));
                rows.push(row("path", "File (.png)", FieldKind::Text));
            }
        }
        rows
    }

    /// Set a draft. A new export format moves the path's extension to it;
    /// a unit chosen by hand is kept over a later guess.
    pub(crate) fn set(&mut self, name: &str, value: String) {
        if name == "format"
            && let (Some(old), Some(new)) = (self.format(), formats::format(&value))
        {
            let path = self.text("path").to_string();
            let ext = extension(&path);
            let cut = path.len().checked_sub(ext.len()).filter(|&i| path.is_char_boundary(i));
            if let Some(cut) = cut
                && old.extensions.contains(&ext.as_str())
                && !new.extensions.contains(&ext.as_str())
            {
                self.texts.insert("path".into(), format!("{}{}", &path[..cut], new.extensions[0]));
            }
        }
        if name == "unit" {
            self.unit_touched = true;
        }
        self.texts.insert(name.into(), value);
        self.error = None;
    }

    /// The mesh file an import form names (absolute, `~/` expanded), as
    /// the guess and the import are sent for it.
    pub(crate) fn import_mesh(&self) -> Option<String> {
        if self.kind != Kind::File(FileOp::Import) {
            return None;
        }
        super::absolute(self.text("path"), "Import").ok().filter(|p| MESH_EXTENSIONS.contains(&extension(p).as_str()))
    }

    /// The mesh path to ask RoboCAD's unit guess for, once per path: the
    /// unit starts over for it (None, not chosen), as RoboCAD's prompt
    /// opens afresh for each import.
    pub(crate) fn ask_guess(&mut self) -> Option<String> {
        let mesh = self.import_mesh()?;
        if self.guess_asked.as_deref() == Some(mesh.as_str()) {
            return None;
        }
        self.guess_asked = Some(mesh.clone());
        self.unit_touched = false;
        self.texts.insert("unit".into(), String::new());
        Some(mesh)
    }

    /// Why OK cannot import the mesh yet: no unit guessed or chosen.
    pub(crate) fn unit_missing(&self) -> Option<String> {
        let mesh = self.import_mesh()?;
        (!IMPORT_UNITS.contains(&self.text("unit"))).then(|| {
            let name = path_field::file_of(&mesh);
            format!("Choose the units of {name} ({}), or wait for RoboCAD's guess: a mesh file has no units, and RoboCAD's import asks for them", IMPORT_UNITS.join(", "))
        })
    }

    /// RoboCAD's unit guess for `path` fills the unit, unless chosen by hand.
    pub(crate) fn guessed(&mut self, path: &str, answer: &Value) {
        if self.import_mesh().as_deref() == Some(path)
            && !self.unit_touched
            && let Some(guess) = answer.get("guess").and_then(Value::as_str).filter(|g| IMPORT_UNITS.contains(g))
        {
            self.texts.insert("unit".into(), guess.into());
        }
    }

    /// The extensions the listing shows.
    fn extensions(&self) -> Vec<&'static str> {
        match self.kind {
            Kind::File(FileOp::Import) => IMPORT_EXTENSIONS.to_vec(),
            Kind::File(_) => vec!["rcad"],
            Kind::Export => self.format().map_or_else(Vec::new, |f| f.extensions.to_vec()),
            Kind::Render => vec!["png"],
        }
    }

    /// The listing the path asks for: (key, directory) of the path's
    /// directory and [`Self::extensions`]; None for a path that is not
    /// absolute (`path_field::listing_key`).
    pub(crate) fn listing_key(&self) -> Option<(String, String)> {
        path_field::listing_key(self.text("path"), &self.extensions())
    }

    /// Whether this form writes its file (a listed file of that name is replaced).
    fn writes(&self) -> bool {
        matches!(self.kind, Kind::File(FileOp::New | FileOp::SaveAs) | Kind::Export | Kind::Render)
    }

    /// A listing entry clicked: a directory descends (a write keeps the
    /// typed file name), a file fills the path.
    pub(crate) fn pick(&mut self, dir: &str, name: &str, is_dir: bool) {
        let path = path_field::pick(self.text("path"), dir, name, is_dir, self.writes());
        self.set("path", path);
    }

    /// "..": the parent of the path's directory, keeping the file name of a write.
    pub(crate) fn up(&mut self) {
        let path = path_field::up(self.text("path"), self.writes());
        self.set("path", path);
    }

    /// The action OK writes, with every value. Err: a field that does not
    /// evaluate, or a mesh's unit not yet guessed or chosen, by name.
    pub(crate) fn action(&self) -> Result<CadAction, String> {
        let path = self.text("path").trim().to_string();
        if path.is_empty() || path.ends_with('/') {
            return Err("Type a file name (or pick one from the list)".into());
        }
        let number = |name: &str, kind: &FieldKind, label: &str| match evaluate(kind, self.text(name)) {
            Ok(FieldValue::Number(v)) => Ok(v),
            Ok(_) => Err(format!("{label}: not a number")),
            Err(e) => Err(format!("{label}: {e}")),
        };
        let on = |name: &str| self.text(name) == "true";
        Ok(match self.kind {
            Kind::File(op) => {
                if let Some(why) = self.unit_missing() {
                    return Err(why);
                }
                let unit = (op == FileOp::Import && MESH_EXTENSIONS.contains(&extension(&path).as_str())).then(|| self.text("unit").to_string());
                CadAction::CadFile(FileArgs { op, path: Some(path), unit })
            }
            Kind::Export => {
                let fmt = self.format().ok_or_else(|| formats::unknown(self.text("format")))?;
                let mut settings = Map::new();
                for st in fmt.settings {
                    let k = key(fmt.id, st.name);
                    let value = match st.kind {
                        SettingKind::Check | SettingKind::Section => Value::Bool(on(&k)),
                        SettingKind::Binary => Value::Bool(self.text(&k) == "binary"),
                        SettingKind::Choice(_) => json!(self.text(&k)),
                        SettingKind::Number { unit, min, max, decimals } => json!(number(&k, &FieldKind::Number { unit, min: Some(min), max: Some(max), decimals }, st.label)?),
                        // An empty sketch is left to the default (the selected sketch, else a refusal).
                        SettingKind::Text if st.name == "sketch" && self.text(&k).trim().is_empty() => continue,
                        SettingKind::Text => json!(self.text(&k).trim()),
                        SettingKind::Views => json!(DRAWING_VIEWS.iter().filter(|(v, _)| on(&key(fmt.id, &format!("view.{v}")))).map(|(v, _)| *v).collect::<Vec<_>>()),
                    };
                    settings.insert(st.name.to_string(), value);
                }
                CadAction::CadExport(ExportArgs { format: Some(fmt.id.to_string()), path: Some(path), settings, ids: None })
            }
            Kind::Render => CadAction::CadRender(RenderArgs {
                path: Some(path),
                view: Some(self.text("view").to_string()),
                w: Some(number("w", &COUNT, "Width")? as u32),
                h: Some(number("h", &COUNT, "Height")? as u32),
                mode: Some(self.text("mode").to_string()),
                edges: Some(on("edges")),
                labels: Some(on("labels")),
                ..Default::default()
            }),
        })
    }

    /// Whether OK can be pressed: a file name, a mesh's unit and every
    /// number evaluating.
    fn ok_ready(&self) -> bool {
        let path = self.text("path").trim();
        !path.is_empty() && !path.ends_with('/') && self.unit_missing().is_none() && self.rows().iter().all(|r| !matches!(r.kind, FieldKind::Number { .. }) || evaluate(&r.kind, self.text(&r.name)).is_ok())
    }

    /// As `cad_state.files.form` shows it.
    pub(crate) fn json(&self) -> Value {
        let fields: Vec<Value> = self.rows().iter().map(|r| json!({"name": r.name, "label": r.label, "kind": format!("{:?}", r.kind), "text": self.text(&r.name)})).collect();
        let kind = match self.kind {
            Kind::File(op) => op.name().to_string(),
            Kind::Export => "export".into(),
            Kind::Render => "render".into(),
        };
        json!({"kind": kind, "title": self.title(), "fields": fields, "focus": self.focus, "error": self.error, "ok_ready": self.ok_ready(), "unit_missing": self.unit_missing()})
    }
}

/// A clickable part of the form.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(super) struct FilePart(pub Hit);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Hit {
    /// A kit form part, by row.
    Form(FormHit),
    /// "Guess unit" (a mesh import).
    Guess,
    /// A part of the kit path listing: an entry (by its index in
    /// `Listing::entries`) or "..". The path input is a form row, so its
    /// `Field` and `Submit` are never drawn here.
    Path(PathHit),
}

/// The form's root (the backdrop).
#[derive(Component)]
pub(super) struct FileFormRoot;

/// The next text field after `name` among `rows` (cycling).
fn next_text(rows: &[Row], name: Option<&str>) -> Option<String> {
    let fields: Vec<&Row> = rows.iter().filter(|r| matches!(r.kind, FieldKind::Number { .. } | FieldKind::Text)).collect();
    let at = name.and_then(|n| fields.iter().position(|r| r.name == n));
    let next = match at {
        Some(i) => fields.get((i + 1) % fields.len().max(1)),
        None => fields.first(),
    };
    next.map(|r| r.name.clone())
}

/// Input: the open form's clicks, its kit field's events and the keys it
/// reads with no field typing (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn input(
    files: Option<ResMut<CadFiles>>,
    parts: Query<(&Interaction, &FilePart, Option<&Enabled>), Changed<Interaction>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    // The field's messages are read first, then `TextFocus` acts (a `ParamSet`: one at a time).
    mut field: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let events: Vec<FieldEvent> = field.p0().read().filter(|m| m.field == FILES).map(|m| m.event.clone()).collect();
    let mut text = field.p1();
    let Some(mut files) = files else {
        text.blur(FILES);
        return;
    };
    let Some(before) = files.form.clone() else {
        text.blur(FILES);
        return;
    };
    let mut form = before.clone();
    let rows = form.rows();
    let (mut submit, mut close, mut guess) = (false, false, false);
    for event in events {
        match event {
            FieldEvent::Changed(draft) => {
                if let Some(name) = form.focus.clone() {
                    form.set(&name, draft.text);
                    form.select_all = draft.select_all;
                }
            }
            FieldEvent::Submit(_) => submit = true,
            // Escape closes the form (the kit has taken the keyboard away).
            FieldEvent::Cancel => close = true,
            FieldEvent::Tab { .. } => {
                form.focus = next_text(&rows, form.focus.as_deref());
                form.select_all = true;
                if let Some(name) = form.focus.clone() {
                    text.focus_draft(FILES, TextDraft::new(form.text(&name), true));
                }
            }
            // Another field took the keyboard, or a mode switch.
            FieldEvent::Blur => form.focus = None,
            FieldEvent::Arrow { .. } => {}
        }
    }
    for (interaction, part, enabled) in &parts {
        if *interaction != Interaction::Pressed || enabled.is_some_and(|e| !e.0) {
            continue;
        }
        match part.0 {
            Hit::Form(FormHit::Field(r)) => {
                if let Some(row) = rows.get(r) {
                    form.focus = Some(row.name.clone());
                    form.select_all = true;
                    text.focus_draft(FILES, TextDraft::new(form.text(&row.name), true));
                }
            }
            Hit::Form(FormHit::Option(r, k)) => {
                if let Some(row) = rows.get(r)
                    && let FieldKind::Choice { options } = row.kind
                    && let Some(o) = options.get(k)
                {
                    form.set(&row.name, o.to_string());
                }
            }
            Hit::Form(FormHit::Check(r)) => {
                if let Some(row) = rows.get(r) {
                    let on = form.text(&row.name) == "true";
                    form.set(&row.name, (!on).to_string());
                }
            }
            Hit::Form(FormHit::Ok) => submit = true,
            Hit::Form(FormHit::Cancel) => close = true,
            Hit::Guess => guess = true,
            Hit::Path(PathHit::Entry(i)) => {
                // Only the listing drawn for this path (as the footer shows it):
                // a newer path's older listing never fills the path.
                if let Some(listing) = files.listed.as_ref().filter(|l| before.listing_key().is_some_and(|(key, _)| key == l.key))
                    && let Some((entry, is_dir)) = listing.entries.get(i)
                {
                    form.pick(&listing.dir, entry, *is_dir);
                }
            }
            Hit::Path(PathHit::Up) => form.up(),
            Hit::Path(PathHit::Field | PathHit::Submit) => {}
        }
    }
    // No row typing (the kit consumes a typing field's keys): the form's
    // own Enter, Escape and Tab, as before.
    if !text.typing() && !submit && !close
        && let Some(keys) = keys.as_ref()
    {
        if keys.just_pressed(KeyCode::Enter) {
            submit = true;
        } else if keys.just_pressed(KeyCode::Escape) {
            close = true;
        } else if keys.just_pressed(KeyCode::Tab) {
            form.focus = next_text(&rows, None);
            form.select_all = true;
        }
    }
    if close {
        out.write(Act::ui(CadAction::CadFile(FileArgs { op: FileOp::Close, ..Default::default() })));
    } else if guess {
        out.write(Act::ui(CadAction::CadFile(FileArgs { op: FileOp::GuessUnit, path: Some(form.text("path").trim().to_string()), ..Default::default() })));
    } else if submit {
        match form.action() {
            Ok(action) => {
                out.write(Act::ui(action));
            }
            Err(e) => form.error = Some(e),
        }
    }
    // The kit field follows the row: it takes the keyboard for the row the
    // form opened on (or Tab chose), and shows the row's text when it
    // changed from outside (a listing pick, "..", a new format's extension).
    // Modal: with no other field typing the first text row takes the
    // keyboard back, so CAD keys stay off under the form (as when it held
    // the keyboard every frame it was open).
    if form.focus.is_none() && !close && !text.typing()
        && let Some(name) = next_text(&rows, None)
    {
        form.focus = Some(name);
        form.select_all = true;
    }
    match form.focus.clone().filter(|_| !close) {
        Some(name) => {
            let want = TextDraft::new(form.text(&name), form.select_all);
            if !text.focused(FILES) {
                text.focus_draft(FILES, want);
            } else if text.draft(FILES) != Some(&want) {
                text.set(FILES, want);
            }
        }
        None => text.blur(FILES),
    }
    // A path newly naming a mesh: RoboCAD's unit guess is asked (a job; it fills the unit).
    if !close
        && let Some(mesh) = form.ask_guess()
    {
        out.write(Act::ui(CadAction::CadFile(FileArgs { op: FileOp::GuessUnit, path: Some(mesh), ..Default::default() })));
    }
    // The directory listing follows the path (read on Pool::Io).
    if let Some((key, dir)) = form.listing_key()
        && form.listing_asked.as_deref() != Some(key.as_str())
    {
        form.listing_asked = Some(key.clone());
        let exts: Vec<String> = form.extensions().iter().map(|s| s.to_string()).collect();
        path_field::request(&mut files.listing, "cad file listing", key, dir, exts);
    }
    if form != before
        && let Some(f) = files.form.as_mut()
        && f.kind == form.kind
    {
        *f = form;
    }
}

/// Present: the form, rebuilt when it, its listing, the unit guess or the
/// document's saved state changes; despawned when it closes.
pub(super) fn draw(mut commands: Commands, files: Option<Res<CadFiles>>, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<FileFormRoot>>, mut last: Local<Option<String>>) {
    let form = files.as_deref().and_then(|f| f.form.as_ref());
    let rule = form.zip(doc.as_deref()).and_then(|(f, d)| open_rule(f, d));
    let guessing = files.as_deref().is_some_and(|f| f.jobs.iter().any(|j| j.kind == "guess_unit"));
    let key = form.map(|form| format!("{form:?}{:?}{:?}{rule:?}{guessing}", files.as_deref().map(|f| &f.listed), files.as_deref().map(|f| &f.guess)));
    let shown = roots.iter().next().is_some();
    if key == *last && shown == key.is_some() {
        return;
    }
    *last = key;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let (Some(files), Some(form)) = (files.as_deref(), form) else { return };
    let k = Kit::new(&fonts);
    let rows = form.rows();
    let form_rows: Vec<FormRow> = rows
        .iter()
        .map(|r| {
            let focused = form.focus.as_deref() == Some(r.name.as_str());
            FormRow { label: &r.label, kind: r.kind, text: form.text(&r.name), focused, optional: false, selected: focused && form.select_all, picks: &[] }
        })
        .collect();
    let title = form.title();
    // The kit's modal backdrop, covering the switcher strip too: a switch
    // would lose the form's typed values (`leaving_blockers` do not check it).
    commands.spawn((k.backdrop(&title, true), FileFormRoot, DespawnOnExit(ModeScope::Cad))).with_children(|backdrop| {
        k.form(backdrop, &title, &form_rows, form.ok_ready(), Some(460.0), |h| FilePart(Hit::Form(h)), |p| footer(p, &k, files, form, rule.as_ref()));
    });
}

/// New and open under `cad_open`'s rule, as it stands: Err (why OK will
/// be refused: an edit in flight, a self-started service's unsaved edits)
/// or Ok (where an attached RoboCAD's unsaved edits stay). None: nothing
/// to say, or another form.
pub(super) fn open_rule(form: &FileForm, doc: &CadDocument) -> Option<Result<String, String>> {
    if !matches!(form.kind, Kind::File(FileOp::New | FileOp::Open)) {
        return None;
    }
    let blockers = doc.switch_blockers();
    if !blockers.is_empty() {
        return Some(Err(format!("Not now: {}", blockers.join("; "))));
    }
    doc.leaving_note().map(|note| Ok(format!("{note}; this window then shows the other file")))
}

/// Under the buttons: the refusal, `cad_open`'s rule for new and open,
/// the unit guess, whether the file exists, and the directory's files.
fn footer(p: &mut ChildSpawnerCommands, k: &Kit, files: &CadFiles, form: &FileForm, rule: Option<&Result<String, String>>) {
    if let Some(error) = &form.error {
        p.spawn(k.text(error.clone(), size::SMALL, DANGER, 0));
    }
    let path = form.text("path").trim();
    if let Some(rule) = rule {
        let (text, color) = match rule {
            Err(why) => (why.clone(), DANGER),
            Ok(note) => (note.clone(), WARN),
        };
        p.spawn(k.text(text, size::SMALL, color, 0));
    }
    if let Some(mesh) = form.import_mesh() {
        let note = match &files.guess {
            Some((g, Ok(v))) if *g == mesh => format!("RoboCAD's guess: {} (largest extent {} in the file's units)", v.get("guess").and_then(Value::as_str).unwrap_or("?"), v.get("extent").and_then(Value::as_f64).map_or_else(|| "?".into(), |e| format!("{e:.4}"))),
            Some((g, Err(e))) if *g == mesh => format!("{e}: choose the units"),
            _ if files.jobs.iter().any(|j| j.kind == "guess_unit") => "Asking RoboCAD for its guess…".into(),
            _ => "A mesh has no units: RoboCAD asks for them, with a guess from its size.".into(),
        };
        p.spawn(k.text(note, size::SMALL, SUBTLE, 0));
        if let Some(why) = form.unit_missing() {
            p.spawn(k.text(why, size::SMALL, WARN, 0));
        }
        p.spawn(k.button("Guess unit", FilePart(Hit::Guess), Look::Secondary, true));
    }
    // The listing drawn is only the one this path asks for now.
    let key = form.listing_key();
    let listing = files.listed.as_ref().filter(|l| key.as_ref().is_some_and(|(want, _)| *want == l.key));
    let file = path_field::file_of(path);
    let exists = listing.is_some_and(|l| !file.is_empty() && l.entries.iter().any(|(n, d)| !d && n == file));
    match (form.kind, exists) {
        (Kind::File(FileOp::New), true) => {
            p.spawn(k.text(format!("{file} exists: RoboCAD refuses to replace it; choose a new name."), size::SMALL, DANGER, 0));
        }
        (_, true) if form.writes() => {
            p.spawn(k.text(format!("{file} exists: it will be replaced."), size::SMALL, WARN, 0));
        }
        _ => {}
    }
    k.path_listing(p, path, listing, &|h| FilePart(Hit::Path(h)));
}
