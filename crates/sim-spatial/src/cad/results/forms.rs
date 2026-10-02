//! The path form of this part (RoboCAD's `QFileDialog`s: "Load simulation
//! results", "Apply identification", "Export physical model", "Export
//! simulation model"; and "Apply actuator profiles", which RoboCAD has no
//! dialog for): a modal kit form with one path row, the directory's
//! matching files from a `Pool::Io` listing (the kit path field's,
//! `ui_kit::path_field`), OK and Cancel, as `cad::files::form` does it.
//!
//! - **Defaults** ([`default_path`]): load, `<stem>.simresult.json` beside
//!   the document (RoboCAD's rule; RoboCAD falls back to the folder when
//!   that file is missing, which needs a stat: here the listing says
//!   whether it is there); identification and profiles, the document's
//!   folder; export, `<stem>.simrobot.json` beside the document.
//! - **Drafts are input editing**: typing changes the form only; OK (or
//!   Enter) writes the one action a caller sends with the path, Cancel (or
//!   Escape) `form_cancel`. A refusal comes back into the form's `error`
//!   ([`settled`]); an accepted request closes it.
//! - **Modal**: a dimmed backdrop takes every click and the form's kit
//!   field ([`RESULTS`]) holds the keyboard while open (taking it ends any
//!   other field's entry).
use super::{ExportKind, ResultsArgs, ResultsOp, doc_path, model_path};
use crate::app::ModeScope;
use crate::app::actions::{Act, Call};
use crate::app::{ViewerMode, ViewerSet};
use crate::builder::ui_api::Enabled;
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::CadDocument;
use crate::ui_kit::form::{FieldKind, FormHit, FormRow};
use crate::ui_kit::path_field::{self, PathHit};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextFocus};
use crate::ui_kit::{DANGER, Kit, SUBTLE, UiFonts, WARN, size};
use bevy::ecs::system::ParamSet;
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_api::Outcome;
use std::path::Path;

/// The form's path field (`ui_kit::text`). Sticky: the path row keeps the
/// keyboard while the form is open (a press on its listing or buttons too).
pub(in crate::cad) const RESULTS: FieldId = FieldId("cad.results");

/// What the form is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormKind {
    Load,
    Identify,
    Export(ExportKind),
    Profiles,
}
impl FormKind {
    /// RoboCAD's dialog title.
    pub(crate) fn title(self) -> &'static str {
        match self {
            FormKind::Load => "Load simulation results",
            FormKind::Identify => "Apply identification",
            FormKind::Export(ExportKind::Physical) => "Export physical model",
            FormKind::Export(ExportKind::Simulation) => "Export simulation model",
            FormKind::Profiles => "Apply actuator profiles",
        }
    }
    /// The path row's label: RoboCAD's file filter.
    fn label(self) -> &'static str {
        match self {
            FormKind::Load => "Simulation results (*.simresult.json)",
            FormKind::Identify => "Fit / results (*.json)",
            FormKind::Export(_) => "Sim model (*.simrobot.json)",
            FormKind::Profiles => "Actuator profiles (*.json, the profiles object)",
        }
    }
    /// The listing's suffixes.
    fn suffixes(self) -> &'static [&'static str] {
        match self {
            FormKind::Export(_) => &["simrobot.json"],
            _ => &["json"],
        }
    }
    /// The form writes the file (an export).
    fn writes(self) -> bool {
        matches!(self, FormKind::Export(_))
    }
    fn name(self) -> &'static str {
        match self {
            FormKind::Load => "load",
            FormKind::Identify => "identify",
            FormKind::Export(ExportKind::Physical) => "export_physical",
            FormKind::Export(ExportKind::Simulation) => "export",
            FormKind::Profiles => "profiles",
        }
    }
}

/// The open form.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PathForm {
    pub kind: FormKind,
    pub draft: TextDraft,
    /// The path row has the keyboard (its kit field, [`RESULTS`], follows it).
    pub focused: bool,
    /// Why OK sent nothing, or RoboCAD's refusal.
    pub error: Option<String>,
    /// The listing last asked for (`path_field::listing_key`).
    pub listing_asked: Option<String>,
}

/// The path a form of `kind` starts with (see the module doc); `home` is
/// the folder used without a document path.
pub(crate) fn default_path(kind: FormKind, doc: Option<&Path>, home: &str) -> String {
    let folder = |d: &Path| format!("{}/", d.display().to_string().trim_end_matches('/'));
    let dir = doc.and_then(Path::parent).filter(|d| d.is_absolute()).map(folder).unwrap_or_else(|| format!("{}/", home.trim_end_matches('/')));
    match (kind, doc) {
        (FormKind::Load, Some(p)) => p.with_extension("simresult.json").display().to_string(),
        (FormKind::Export(_), Some(p)) => model_path(p).display().to_string(),
        (FormKind::Export(_), None) => format!("{dir}untitled.simrobot.json"),
        _ => dir,
    }
}

impl PathForm {
    pub(crate) fn new(kind: FormKind, doc: Option<&Path>) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        let text = default_path(kind, doc, &home);
        PathForm { kind, draft: TextDraft { text, select_all: false }, focused: true, error: None, listing_asked: None }
    }
    fn text(&self) -> &str {
        self.draft.text.trim()
    }
    /// OK can be pressed: a file name.
    fn ok_ready(&self) -> bool {
        let p = self.text();
        !p.is_empty() && !p.ends_with('/')
    }
    /// The listing this path asks for now.
    fn listing_key(&self) -> Option<(String, String)> {
        path_field::listing_key(self.text(), self.kind.suffixes())
    }
    /// The action OK writes: the op with the path.
    pub(crate) fn action(&self) -> Result<CadAction, String> {
        if !self.ok_ready() {
            return Err(format!("{}: type a file name", self.kind.title()));
        }
        let path = Some(self.text().to_string());
        let args = match self.kind {
            FormKind::Load => ResultsArgs { op: ResultsOp::Load, path, ..Default::default() },
            FormKind::Identify => ResultsArgs { op: ResultsOp::Identify, path, ..Default::default() },
            FormKind::Profiles => ResultsArgs { op: ResultsOp::Profiles, path, ..Default::default() },
            FormKind::Export(kind) => ResultsArgs { op: ResultsOp::Export, path, kind: Some(kind), ..Default::default() },
        };
        Ok(CadAction::CadResults(args))
    }
    pub(crate) fn json(&self) -> Value {
        json!({"kind": self.kind.name(), "title": self.kind.title(), "label": self.kind.label(), "path": self.draft.text, "focused": self.focused, "error": self.error, "ok_ready": self.ok_ready()})
    }
}

/// Opens the form of `kind` (closing the file form: one modal at a time).
pub(in crate::cad) fn open(cx: &mut Cx, kind: FormKind) -> Result<Value, String> {
    // Its field takes the keyboard (`input`), which ends any other field's entry.
    if let Some(files) = cx.files.as_deref_mut() {
        files.form = None;
    }
    let form = PathForm::new(kind, doc_path(cx.doc).as_deref());
    let shown = form.json();
    cx.doc.results.form_sequence = cx.doc.results.form_sequence.wrapping_add(1);
    cx.doc.results.form = Some(form);
    cx.doc.touch();
    Ok(json!({"opened": shown}))
}

/// After an op the form may have sent: an accepted one closes the form of
/// its kind; a refusal stays in it.
pub(in crate::cad) fn settled(cx: &mut Cx, kind: FormKind, outcome: Outcome) -> Outcome {
    let doc = &mut *cx.doc;
    if let Some(form) = doc.results.form.as_mut().filter(|f| f.kind == kind) {
        match &outcome {
            Outcome::Done(Err(e)) => form.error = Some(e.clone()),
            _ => doc.results.form = None,
        }
        doc.touch();
    }
    outcome
}

/// `form_set`, `form_submit`, `form_cancel`.
pub(in crate::cad) fn handle(args: &ResultsArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let done = Outcome::Done;
    let Some(form) = cx.doc.results.form.clone() else { return done(Err("no results form is open (load, identify, profiles or export without a path opens one)".into())) };
    match args.op {
        ResultsOp::FormSet => {
            let Some(text) = &args.text else { return done(Err("form_set needs text (the path)".into())) };
            if let Some(f) = cx.doc.results.form.as_mut() {
                f.draft = TextDraft { text: text.clone(), select_all: false };
                f.error = None;
            }
            cx.doc.touch();
            done(Ok(json!({"form": cx.doc.results.form.as_ref().map(PathForm::json)})))
        }
        ResultsOp::FormSubmit => match form.action() {
            Ok(action) => super::handle(&action, call, cx),
            Err(e) => {
                if let Some(f) = cx.doc.results.form.as_mut() {
                    f.error = Some(e.clone());
                }
                cx.doc.touch();
                done(Err(e))
            }
        },
        _ => {
            cx.doc.results.form = None;
            cx.doc.touch();
            done(Ok(json!({"closed": form.kind.title()})))
        }
    }
}

/// A clickable part of the form.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(super) enum Part {
    Form(FormHit),
    Path(PathHit),
}

/// The form's root (the backdrop).
#[derive(Component)]
pub(in crate::cad) struct ResultsFormRoot;

/// Input: the open form's clicks, its path field's events and the keys it
/// reads with no field typing (see the module doc).
#[allow(clippy::type_complexity)]
pub(super) fn input(
    doc: Option<ResMut<CadDocument>>,
    parts: Query<(&Part, Option<&Enabled>), With<crate::ui_kit::activation::Activated>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    // The field's messages are read first, then `TextFocus` acts (a `ParamSet`: one at a time).
    mut field: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    mut out: MessageWriter<Act<CadAction>>,
    files: Option<Res<crate::cad::files::CadFiles>>,
) {
    let events: Vec<FieldEvent> = field.p0().read().filter(|m| m.field == RESULTS).map(|m| m.event.clone()).collect();
    let mut text = field.p1();
    let Some(mut doc) = doc else {
        text.blur(RESULTS);
        return;
    };
    let Some(before) = doc.results.form.clone() else {
        text.blur(RESULTS);
        return;
    };
    // The file form opened over this one has the keyboard (this one's row
    // takes it back when that one closes).
    if files.is_some_and(|f| f.form.is_some()) {
        return;
    }
    let mut form = before.clone();
    let (mut submit, mut close) = (false, false);
    for event in events {
        match event {
            FieldEvent::Changed(draft) => {
                form.draft = draft;
                form.error = None;
            }
            FieldEvent::Submit(_) => submit = true,
            // Escape cancels the form (the kit has taken the keyboard away).
            FieldEvent::Cancel => close = true,
            FieldEvent::Tab { .. } | FieldEvent::Arrow { .. } => {}
            // Another field took the keyboard, or a mode switch.
            FieldEvent::Blur => form.focused = false,
        }
    }
    for (part, enabled) in &parts {
        if enabled.is_some_and(|e| !e.0) {
            continue;
        }
        match *part {
            Part::Form(FormHit::Field(_)) => {
                form.focused = true;
                form.draft.select_all = true;
                text.focus_draft(RESULTS, form.draft.clone());
            }
            Part::Form(FormHit::Ok) => submit = true,
            Part::Form(FormHit::Cancel) => close = true,
            Part::Form(_) => {}
            Part::Path(PathHit::Entry(i)) => {
                // Only the listing drawn for this path: an older one never fills it.
                if let Some(listing) = doc.results.listed.as_ref().filter(|l| before.listing_key().is_some_and(|(key, _)| key == l.key))
                    && let Some((entry, is_dir)) = listing.entries.get(i)
                {
                    form.draft = TextDraft { text: path_field::pick(&form.draft.text, &listing.dir, entry, *is_dir, form.kind.writes()), select_all: false };
                    form.error = None;
                }
            }
            Part::Path(PathHit::Up) => {
                form.draft = TextDraft { text: path_field::up(&form.draft.text, form.kind.writes()), select_all: false };
                form.error = None;
            }
            Part::Path(PathHit::Field | PathHit::Submit) => {}
        }
    }
    // The row not typing (the kit consumes a typing field's keys): the
    // form's own Enter, Escape and Tab, as before.
    if !text.typing() && !text.ordinary_focused() && !submit && !close
        && let Some(keys) = keys.as_ref()
    {
        if keys.just_pressed(KeyCode::Enter) {
            submit = true;
        } else if keys.just_pressed(KeyCode::Escape) {
            close = true;
        } else if keys.just_pressed(KeyCode::Tab) {
            form.focused = true;
            form.draft.select_all = true;
        }
    }
    // Modal: with no other field typing the row takes the keyboard back, so
    // CAD keys stay off under the form (as when it held the keyboard every
    // frame it was open).
    if !form.focused && !close && !text.typing() && !text.ordinary_focused() {
        form.focused = true;
    }
    // The kit field follows the row: it has the keyboard while the row is
    // focused, and shows the path when it changed from outside (a listing
    // pick, "..", `form_set`).
    if form.focused && !close {
        if !text.focused(RESULTS) {
            text.focus_draft(RESULTS, form.draft.clone());
        } else if text.draft(RESULTS) != Some(&form.draft) {
            text.set(RESULTS, form.draft.clone());
        }
    } else {
        text.blur(RESULTS);
    }
    if close {
        out.write(Act::ui(crate::cad::activation::guard_results(&doc, super::ResultsArgs::of(ResultsOp::FormCancel))));
    } else if submit {
        match form.action() {
            Ok(action) => {
                out.write(Act::ui(crate::cad::activation::guard_results(&doc, action)));
            }
            Err(e) => form.error = Some(e),
        }
    }
    // The directory listing follows the path (read on Pool::Io; landed by `link::receive`).
    if let Some((key, dir)) = form.listing_key()
        && form.listing_asked.as_deref() != Some(key.as_str())
    {
        form.listing_asked = Some(key.clone());
        let exts: Vec<String> = form.kind.suffixes().iter().map(|s| s.to_string()).collect();
        path_field::request(&mut doc.results.listing, "cad results listing", key, dir, exts);
    }
    if form != before
        && let Some(f) = doc.results.form.as_mut()
        && f.kind == form.kind
    {
        *f = form;
    }
}

/// Present: the form, rebuilt when it or its listing changes; despawned when it closes.
pub(super) fn draw(mut commands: Commands, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<ResultsFormRoot>>, mut last: Local<Option<String>>) {
    let form = doc.as_deref().and_then(|d| d.results.form.as_ref());
    let listed = doc.as_deref().and_then(|d| d.results.listed.as_ref());
    let key = form.map(|f| format!("{f:?}{listed:?}{:?}", doc.as_deref().map(|d| d.results.form_sequence)));
    let key = key.map(|key| format!("{key}|source={:?}", doc.as_deref().map(crate::cad::activation::render_key)));
    let shown = roots.iter().next().is_some();
    if key == *last && shown == key.is_some() {
        return;
    }
    *last = key;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some(form) = form else { return };
    let k = Kit::new(&fonts);
    let title = form.kind.title();
    let rows = [FormRow { label: form.kind.label(), kind: FieldKind::Text, text: &form.draft.text, focused: form.focused, optional: false, selected: form.focused && form.draft.select_all, picks: &[] }];
    // Covering the switcher strip too: a switch would drop the typed path.
    commands.spawn((k.backdrop(title, true), ResultsFormRoot, DespawnOnExit(ModeScope::Cad))).with_children(|backdrop| {
        k.form(backdrop, title, &rows, form.ok_ready(), Some(460.0), Part::Form, |p| footer(p, &k, form, listed));
    });
}

/// Under the buttons: the refusal, whether the file is there, and the directory's files.
fn footer(p: &mut ChildSpawnerCommands, k: &Kit, form: &PathForm, listed: Option<&path_field::Listing>) {
    if let Some(error) = &form.error {
        p.spawn(k.text(error.clone(), size::SMALL, DANGER, 0));
    }
    let key = form.listing_key();
    let listing = listed.filter(|l| key.as_ref().is_some_and(|(want, _)| *want == l.key));
    let file = path_field::file_of(form.text());
    if let Some(l) = listing.filter(|_| !file.is_empty()) {
        let exists = l.entries.iter().any(|(n, d)| !d && n == file);
        match (form.kind.writes(), exists) {
            (true, true) => {
                p.spawn(k.text(format!("{file} exists: it will be replaced."), size::SMALL, WARN, 0));
            }
            (false, false) => {
                p.spawn(k.text(format!("{file} is not in {}.", l.dir), size::SMALL, WARN, 0));
            }
            _ => {}
        }
    }
    if form.kind == FormKind::Profiles {
        p.spawn(k.text("RoboCAD checks the profiles through the actuator registry and refuses them by name.", size::SMALL, SUBTLE, 0));
    }
    k.path_listing(p, form.text(), listing, &Part::Path);
}

/// CadPlugin: the form's text field, its input (before the CAD keys, so
/// the frame's keys see its field typing) and its drawing.
pub(super) fn build(app: &mut App) {
    use crate::ui_kit::text::{TextField, TextFieldApp};
    app.add_text_field(RESULTS, TextField::new("Results path").sticky());
    app.add_systems(
        Update,
        (
            input.in_set(crate::app::InputSet::Window).before(crate::cad::CadKeySet::Gate).in_set(crate::cad::CadKeySet::Focus).in_set(ViewerSet::Input),
            draw.in_set(ViewerSet::Present),
        )
            .run_if(in_state(ViewerMode::Cad)),
    );
}
