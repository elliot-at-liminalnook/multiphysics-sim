//! The path form (RoboCAD's `QFileDialog` and its option dialogs, natively;
//! see the module doc in `files` for why not a native dialog): a modal kit
//! form with the path field, the operation's options (the import unit, an
//! export format's settings, a render's view, size and mode), the
//! directory's matching files from a `Pool::Io` listing, and OK / Cancel.
//!
//! - **Drafts are input editing** (native-viewer.md §3): typing, a choice
//!   or a checkbox changes the form's texts only; OK (and Enter) writes the
//!   one action REST takes (`CadFile`, `CadExport`, `CadRender`) with every
//!   value, Cancel (and Escape) writes `CadFile {op: close}`, "Discard
//!   changes" writes the same action as OK with `discard: true`, "Guess
//!   unit" writes `CadFile {op: guess_unit}`. A refusal comes back into
//!   `error` (`files::handle`).
//! - **Modal**: a dimmed backdrop takes every click, and the form holds
//!   `CadInputFocus` while open (set after `panel::name_entry` resets it,
//!   before every reader), ending the other fields' drafts.
//! - **Listing**: the path's directory and the operation's extensions are
//!   read on `Pool::Io` when they change; a directory entry descends (a
//!   write keeps the file name typed), a file entry fills the path, ".."
//!   goes up. A write onto a listed file says it will be replaced (Qt's
//!   save dialog asks the same); New says RoboCAD refuses to replace it.
use super::formats::{self, DRAWING_VIEWS, FORMAT_IDS, Kind as SettingKind};
use super::{CadFiles, ExportArgs, FileArgs, FileOp, RenderArgs, jobs};
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadInputFocus};
use crate::cad::panel::NameDraft;
use crate::ui_kit::form::{DraftKey, FieldKind, FieldValue, FormHit, FormRow, TextDraft, Unit, evaluate};
use crate::ui_kit::{BAR, DANGER, FAINT, Kit, Look, SUBTLE, UiFonts, WARN, size};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::{Map, Value, json};
use sim_runtime::cad_client::{IMPORT_EXTENSIONS, IMPORT_UNITS, MESH_EXTENSIONS, RENDER_MODES, RENDER_VIEWS, extension};
use std::collections::BTreeMap;

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
    /// The text field with the keyboard.
    pub focus: Option<String>,
    /// The focused text is selected (the next key replaces it).
    pub select_all: bool,
    /// Why OK sent nothing, or RoboCAD's refusal.
    pub error: Option<String>,
    /// The import unit was chosen by hand (a guess no longer replaces it).
    pub unit_touched: bool,
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

/// The directory part of `path` with a trailing `/` (`~/` expanded).
fn dir_of(path: &str) -> String {
    let path = match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{}/{rest}", home.trim_end_matches('/')),
        _ => path.to_string(),
    };
    if path.ends_with('/') {
        return path;
    }
    match std::path::Path::new(&path).parent().map(|p| p.display().to_string()) {
        Some(p) if !p.is_empty() => format!("{}/", p.trim_end_matches('/')),
        _ => "/".into(),
    }
}

/// The file name part of `path` (after the last `/`).
fn file_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or("")
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
                put("unit", "mm".into());
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
        Ok(FileForm { kind, texts, focus: Some("path".into()), select_all: false, error: None, unit_touched: false, listing_asked: None, section_available: cx.section.is_some() })
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

    /// RoboCAD's unit guess for `path` fills the unit, unless chosen by hand.
    pub(crate) fn guessed(&mut self, path: &str, answer: &Value) {
        if self.kind == Kind::File(FileOp::Import)
            && !self.unit_touched
            && self.text("path").trim() == path
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

    /// The listing the path asks for: (key, directory, extensions); None
    /// for a path that is not absolute.
    pub(crate) fn listing_key(&self) -> Option<(String, String, Vec<&'static str>)> {
        let dir = dir_of(self.text("path").trim());
        if !std::path::Path::new(&dir).is_absolute() {
            return None;
        }
        let exts = self.extensions();
        Some((format!("{dir}|{}", exts.join(",")), dir, exts))
    }

    /// Whether this form writes its file (a listed file of that name is replaced).
    fn writes(&self) -> bool {
        matches!(self.kind, Kind::File(FileOp::New | FileOp::SaveAs) | Kind::Export | Kind::Render)
    }

    /// A listing entry clicked: a directory descends (a write keeps the
    /// typed file name), a file fills the path.
    pub(crate) fn pick(&mut self, dir: &str, name: &str, is_dir: bool) {
        let keep = if self.writes() { file_of(self.text("path")).to_string() } else { String::new() };
        let dir = dir.trim_end_matches('/');
        let path = if is_dir { format!("{dir}/{name}/{keep}") } else { format!("{dir}/{name}") };
        self.set("path", path);
    }

    /// "..": the parent of the path's directory, keeping the file name of a write.
    pub(crate) fn up(&mut self) {
        let keep = if self.writes() { file_of(self.text("path")).to_string() } else { String::new() };
        let dir = dir_of(self.text("path").trim());
        let parent = std::path::Path::new(dir.trim_end_matches('/')).parent().map_or_else(|| "/".to_string(), |p| p.display().to_string());
        self.set("path", format!("{}/{keep}", parent.trim_end_matches('/')));
    }

    /// The action OK writes, with every value (`discard`: the Discard
    /// changes button). Err: a field that does not evaluate, by name.
    pub(crate) fn action(&self, discard: bool) -> Result<CadAction, String> {
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
                let unit = (op == FileOp::Import && MESH_EXTENSIONS.contains(&extension(&path).as_str())).then(|| self.text("unit").to_string());
                CadAction::CadFile(FileArgs { op, path: Some(path), unit, discard })
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

    /// Whether OK can be pressed: a file name and every number evaluating.
    fn ok_ready(&self) -> bool {
        let path = self.text("path").trim();
        !path.is_empty() && !path.ends_with('/') && self.rows().iter().all(|r| !matches!(r.kind, FieldKind::Number { .. }) || evaluate(&r.kind, self.text(&r.name)).is_ok())
    }

    /// As `cad_state.files.form` shows it.
    pub(crate) fn json(&self) -> Value {
        let fields: Vec<Value> = self.rows().iter().map(|r| json!({"name": r.name, "label": r.label, "kind": format!("{:?}", r.kind), "text": self.text(&r.name)})).collect();
        let kind = match self.kind {
            Kind::File(op) => op.name().to_string(),
            Kind::Export => "export".into(),
            Kind::Render => "render".into(),
        };
        json!({"kind": kind, "title": self.title(), "fields": fields, "focus": self.focus, "error": self.error})
    }
}

/// A clickable part of the form.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(super) struct FilePart(pub Hit);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Hit {
    /// A kit form part, by row.
    Form(FormHit),
    /// "Discard changes and …" (new, open).
    Discard,
    /// "Guess unit" (a mesh import).
    Guess,
    /// A listing entry, by index.
    Entry(usize),
    /// "..": the parent directory.
    Up,
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

/// Input: the open form's clicks and keys (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn input(
    files: Option<ResMut<CadFiles>>,
    doc: Option<ResMut<CadDocument>>,
    parts: Query<(&Interaction, &FilePart, Option<&Enabled>), Changed<Interaction>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut events: MessageReader<KeyboardInput>,
    focus: Option<ResMut<CadInputFocus>>,
    name: Option<ResMut<NameDraft>>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let Some(mut files) = files else { return };
    let Some(before) = files.form.clone() else { return };
    let mut form = before.clone();
    // Modal: the form has the keyboard; the other fields' drafts end.
    if let Some(mut focus) = focus
        && !focus.0
    {
        focus.0 = true;
    }
    if let Some(mut name) = name
        && name.editing.is_some()
    {
        name.editing = None;
        name.refusal = None;
    }
    if let Some(mut doc) = doc {
        if doc.tool_state.numeric.focus.is_some() {
            doc.tool_state.numeric.focus = None;
        }
        if doc.tool_state.inspector_edit.is_some() {
            doc.tool_state.inspector_edit = None;
        }
        if doc.ops.form.as_ref().is_some_and(|f| f.focus.is_some())
            && let Some(f) = doc.ops.form.as_mut()
        {
            f.focus = None;
        }
    }
    let rows = form.rows();
    let (mut submit, mut close, mut guess) = (None::<bool>, false, false);
    for (interaction, part, enabled) in &parts {
        if *interaction != Interaction::Pressed || enabled.is_some_and(|e| !e.0) {
            continue;
        }
        match part.0 {
            Hit::Form(FormHit::Field(r)) => {
                if let Some(row) = rows.get(r) {
                    form.focus = Some(row.name.clone());
                    form.select_all = true;
                    // Keys pressed before the field took the keyboard are not its text.
                    events.clear();
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
            Hit::Form(FormHit::Ok) => submit = Some(false),
            Hit::Form(FormHit::Cancel) => close = true,
            Hit::Discard => submit = Some(true),
            Hit::Guess => guess = true,
            Hit::Entry(i) => {
                if let Some(listing) = &files.listed
                    && let Some((entry, is_dir)) = listing.entries.get(i)
                {
                    form.pick(&listing.dir, entry, *is_dir);
                }
            }
            Hit::Up => form.up(),
        }
    }
    let chord = keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]));
    let typed: Vec<KeyboardInput> = events.read().filter(|e| e.state == ButtonState::Pressed).cloned().collect();
    for e in typed {
        if submit.is_some() || close {
            break;
        }
        match form.focus.clone() {
            Some(field) => {
                let mut draft = TextDraft { text: form.text(&field).to_string(), select_all: form.select_all };
                match draft.key(&e.logical_key, chord) {
                    DraftKey::Edited => {
                        form.set(&field, draft.text);
                        form.select_all = draft.select_all;
                    }
                    DraftKey::Enter => submit = Some(false),
                    DraftKey::Escape => close = true,
                    DraftKey::Tab => {
                        form.focus = next_text(&rows, Some(field.as_str()));
                        form.select_all = true;
                    }
                    DraftKey::Ignored => {}
                }
            }
            None => match &e.logical_key {
                Key::Enter => submit = Some(false),
                Key::Escape => close = true,
                Key::Tab => {
                    form.focus = next_text(&rows, None);
                    form.select_all = true;
                }
                _ => {}
            },
        }
    }
    if close {
        out.write(Act::ui(CadAction::CadFile(FileArgs { op: FileOp::Close, ..Default::default() })));
    } else if guess {
        out.write(Act::ui(CadAction::CadFile(FileArgs { op: FileOp::GuessUnit, path: Some(form.text("path").trim().to_string()), ..Default::default() })));
    } else if let Some(discard) = submit {
        match form.action(discard) {
            Ok(action) => {
                out.write(Act::ui(action));
            }
            Err(e) => form.error = Some(e),
        }
    }
    // The directory listing follows the path (read on Pool::Io).
    if let Some((key, dir, exts)) = form.listing_key()
        && form.listing_asked.as_deref() != Some(key.as_str())
    {
        form.listing_asked = Some(key.clone());
        jobs::request_listing(&mut files, key, dir, exts);
    }
    if form != before
        && let Some(f) = files.form.as_mut()
        && f.kind == form.kind
    {
        *f = form;
    }
}

/// Entries the form lists.
const SHOWN: usize = 12;

/// Present: the form, rebuilt when it, its listing, the unit guess or the
/// document's saved state changes; despawned when it closes.
pub(super) fn draw(mut commands: Commands, files: Option<Res<CadFiles>>, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<FileFormRoot>>, mut last: Local<Option<String>>) {
    let form = files.as_deref().and_then(|f| f.form.as_ref());
    let unsaved = doc.as_deref().map(|d| (d.unsaved(), d.document_name()));
    let key = form.map(|form| format!("{form:?}{:?}{:?}{unsaved:?}", files.as_deref().map(|f| &f.listed), files.as_deref().map(|f| &f.guess)));
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
            FormRow { label: &r.label, kind: r.kind, text: form.text(&r.name), focused, optional: false, selected: focused && form.select_all }
        })
        .collect();
    let title = form.title();
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(BAR.with_alpha(0.55)),
            FocusPolicy::Block,
            GlobalZIndex(50),
            AccessibleLabel::new(title.clone()),
            FileFormRoot,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|backdrop| {
            k.form(backdrop, &title, &form_rows, form.ok_ready(), Some(460.0), |h| FilePart(Hit::Form(h)), |p| footer(p, &k, files, form, doc.as_deref()));
        });
}

/// Under the buttons: the refusal, the unsaved-edit choice, the unit
/// guess, whether the file exists, and the directory's files.
fn footer(p: &mut ChildSpawnerCommands, k: &Kit, files: &CadFiles, form: &FileForm, doc: Option<&CadDocument>) {
    if let Some(error) = &form.error {
        p.spawn(k.text(error.clone(), size::SMALL, DANGER, 0));
    }
    let path = form.text("path").trim();
    if let (Kind::File(op @ (FileOp::New | FileOp::Open)), Some(doc)) = (form.kind, doc)
        && doc.unsaved() != Some(false)
    {
        let why = if doc.unsaved() == Some(true) { "has unsaved edits" } else { "may have unsaved edits (not confirmed now)" };
        p.spawn(k.text(format!("{} {why}: save it first (Save), or discard them. RoboCAD asks the same.", doc.document_name()), size::SMALL, WARN, 0));
        let label = if op == FileOp::New { "Discard changes and create" } else { "Discard changes and open" };
        p.spawn(k.button(label, FilePart(Hit::Discard), Look::Danger, form.ok_ready()));
    }
    if form.kind == Kind::File(FileOp::Import) && MESH_EXTENSIONS.contains(&extension(path).as_str()) {
        let note = match &files.guess {
            Some((g, Ok(v))) if g == path => format!("RoboCAD's guess: {} (largest extent {} in the file's units)", v.get("guess").and_then(Value::as_str).unwrap_or("?"), v.get("extent").and_then(Value::as_f64).map_or_else(|| "?".into(), |e| format!("{e:.4}"))),
            Some((g, Err(e))) if g == path => e.clone(),
            _ if files.jobs.iter().any(|j| j.kind == "guess_unit") => "Asking RoboCAD for its guess…".into(),
            _ => "A mesh has no units: RoboCAD asks for them, with a guess from its size.".into(),
        };
        p.spawn(k.text(note, size::SMALL, SUBTLE, 0));
        p.spawn(k.button("Guess unit", FilePart(Hit::Guess), Look::Secondary, true));
    }
    let Some(listing) = files.listed.as_ref().filter(|l| form.listing_key().is_some_and(|(key, ..)| key == l.key)) else {
        if form.listing_key().is_none() {
            p.spawn(k.text("Type an absolute path (~/ works) to list its directory.", size::SMALL, FAINT, 0));
        }
        return;
    };
    let file = file_of(path);
    let exists = !file.is_empty() && listing.entries.iter().any(|(n, d)| !d && n == file);
    match (form.kind, exists) {
        (Kind::File(FileOp::New), true) => {
            p.spawn(k.text(format!("{file} exists: RoboCAD refuses to replace it; choose a new name."), size::SMALL, DANGER, 0));
        }
        (_, true) if form.writes() => {
            p.spawn(k.text(format!("{file} exists: it will be replaced."), size::SMALL, WARN, 0));
        }
        _ => {}
    }
    p.spawn(k.caption(format!("In {}", listing.dir)));
    if let Some(e) = &listing.error {
        p.spawn(k.text(e.clone(), size::SMALL, FAINT, 0));
    }
    p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() }).with_children(|list| {
        if listing.dir != "/" {
            list.spawn(k.button("..", FilePart(Hit::Up), Look::Ghost, true));
        }
        for (i, (entry, is_dir)) in listing.entries.iter().take(SHOWN).enumerate() {
            let label = if *is_dir { format!("{entry}/") } else { entry.clone() };
            list.spawn(k.button(&label, FilePart(Hit::Entry(i)), Look::Ghost, true));
        }
    });
    let more = listing.entries.len().saturating_sub(SHOWN) + listing.more;
    if more > 0 {
        p.spawn(k.text(format!("{more} more: type to narrow the path."), size::SMALL, FAINT, 0));
    }
}
