//! The document picker (window-first-usability): what a switch from the
//! window opens for a mode with no document, instead of a refusal a person
//! cannot act on.
//!
//! - **Trigger** (`switch::start`): an interactive switch (a click in the
//!   window, or a `system_ui` `mode:<mode>` activation) to a mode with no
//!   document and nothing to reopen opens [`Picker`] for that mode; the
//!   current mode stays. REST `viewer_mode` is refused as before.
//! - **Sources** (`discover::discover`): the mode's recent documents
//!   (`super::recent`), robot presets, the workspace's example files, and
//!   for CAD the RoboCAD service at the default URL. Found on a `jobs` job
//!   (`Pool::Io`) when the picker opens; the UI thread reads no file.
//! - **The path field** ("Open file…", the kit's `path_field`): a typed
//!   path (`~/` expanded; for CAD an http(s) URL too) with its directory's
//!   listing, read on `Pool::Io` when the directory changes. Its text is
//!   the kit text field [`PATH`] (`ui_kit::text`): the one input system
//!   types into it, and whether it has the keyboard is the kit's focus
//!   (`InputFocus`), nothing of the picker's. [`Picker::draft`] mirrors it
//!   for `system_ui` and REST (edits arrive as `Changed`; the picker's own
//!   changes reach the field through [`sync`]). Opening the picker gives
//!   it the keyboard (so no field underneath types while the modal is up);
//!   it retains its draft while Tab/Shift+Tab use the kit navigation contract;
//!   Escape, closing and a mode switch end it.
//! - **Choices are switches**: an entry, the path field's Open (and Enter)
//!   write the same `WindowAction::Switch` that `viewer_mode {mode, path |
//!   preset | url}` builds ([`Picker::choice`], [`Picker::typed`]); there
//!   is no second switch path. A refused choice keeps the picker open and
//!   shows the switcher's refusal as its status line.
//! - **Closing**: when the window's mode changes (the choice succeeded, or
//!   another mode was chosen in the switcher), on Close and on Escape; the
//!   path field gives up the keyboard.
//! - **Modal** ([`keys`], PreUpdate after the kit's input system): while
//!   open, keys and the wheel go to the picker only; it reads no keyboard
//!   message (the field's text is the kit's), Escape without the field comes from the key states, while ordinary
//!   Enter/Space and Tab use the shared activation/navigation contract, then the wheel messages
//!   are cleared and the held keys released (not reset, so a held W or Q/A
//!   underneath sees its release and stops), so no mode shortcut fires
//!   underneath. It does not open over robot mode's Leg calibration panel,
//!   whose STOP must stay reachable.
//! - **`system_ui`**: its entries are controls `picker:<mode>:<n>` (flat
//!   index across the sections), `picker:path` (activate with an optional
//!   `text`, set and submitted) and `picker:close`, appended to every
//!   mode's controls while it is open (`route::annotate`). Each
//!   `picker:<mode>:<n>` is listed with `picker_revision` (its own key: the
//!   answer's `ui_revision` is the mode's), the picker's revision
//!   when its entries last changed ([`Picker::listed_at`]); an activation
//!   giving another one is refused, naming both, as the builder's and robot
//!   mode's controls refuse a stale listing.
mod discover;
mod modal;

pub(crate) use discover::discover;
pub(crate) use modal::{keys, sync};

use super::actions::Act;
use super::recent::Recents;
use super::settings::SettingsOwner;
use super::switch::{Document, ModeSwitch, Switcher, WindowAction};
use super::{Persistent, ViewerMode};
use crate::builder::ui_api::Enabled;
use crate::jobs::{Latest, Pool};
use crate::ui_kit::path_field::{self, Listing, PathHit, PathView};
use crate::ui_kit::picker::{PickHit, PickerEntry, PickerSection};
use crate::ui_kit::text::{FieldId, TextDraft, TextField, TextFocus, Typing};
use crate::ui_kit::{DANGER, Kit, SUBTLE, UiFonts};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// The path field ("Open file…"): the kit text field the picker's path is typed in.
pub(crate) const PATH: FieldId = FieldId("picker.path");

/// [`PATH`] as `switch::build` adds it: selected when it takes the
/// keyboard, sticky (a press on the picker keeps typing: see the module
/// doc).
pub(crate) fn path_text_field() -> TextField {
    TextField::new("Open file…").select_on_focus().sticky()
}

/// One document the picker offers.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Choice {
    pub label: String,
    pub detail: String,
    pub enabled: bool,
    pub document: Document,
}

/// A titled section of choices; `empty` is shown when it has none.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Section {
    pub title: String,
    pub empty: String,
    pub choices: Vec<Choice>,
}

/// What discovery found for one mode.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Sources {
    pub sections: Vec<Section>,
    /// Where the path field starts (`<root>/examples/`, else the root).
    pub start_dir: Option<String>,
}

/// The document picker's state (see the module doc).
#[derive(Resource, Default)]
pub(crate) struct Picker {
    /// The mode it chooses a document for; None: closed.
    pub(crate) open: Option<ViewerMode>,
    /// The mode the window was in when it opened (it closes when that changes).
    pub(crate) from: Option<ViewerMode>,
    /// Why it opened (its subtitle).
    pub(crate) reason: String,
    /// The path field's draft: the kit field [`PATH`]'s, mirrored (its
    /// `Changed` edits; the picker's own changes reach it by [`sync`]).
    pub(crate) draft: TextDraft,
    /// The sections' scroll offset (px).
    pub(crate) scroll: f32,
    sources: Latest<Sources>,
    recent_snapshot: Option<(bool, Recents)>,
    presets: Option<PathBuf>,
    pub(crate) found: Option<Sources>,
    listing: Latest<Listing>,
    pub(crate) listed: Option<Listing>,
    listing_asked: Option<String>,
    /// The draft is still the examples folder it started in, not a choice:
    /// Enter and Open don't submit it (Lessons and Place take folders).
    prefilled: bool,
    /// The path was typed or set since opening: discovery's start folder
    /// no longer fills it (the field has the keyboard from the start, so
    /// its focus can't say whether something was typed).
    edited: bool,
    /// The switcher's revision when it opened: a later outcome (a refused
    /// choice, a load in progress) is its status line.
    pub(crate) opened_revision: u64,
    /// Bumped on every change it shows.
    pub(crate) revision: u64,
    /// `revision` when the entries (`picker:<mode>:<n>`) last changed: on
    /// opening, closing and discovery's result. Monotonic across reopens, so
    /// an index listed by an earlier picker never matches.
    pub(crate) listed_at: u64,
}

/// A mode's document, as the switcher's message names it.
pub(crate) fn noun(mode: ViewerMode) -> &'static str {
    match mode {
        ViewerMode::Build => "a system",
        ViewerMode::Lessons => "a lessons folder",
        ViewerMode::Robot => "a robot",
        ViewerMode::Place => "a place",
        ViewerMode::Cad => "a CAD document",
        ViewerMode::Inspect => "an assembly",
        ViewerMode::Phenomena => "an exhibit",
    }
}

/// The picker's title for `mode`.
pub(crate) fn title(mode: ViewerMode) -> String {
    format!("Open {}", noun(mode))
}

/// The file suffixes the path field lists for `mode` (none: directories only).
pub(crate) fn suffixes(mode: ViewerMode) -> &'static [&'static str] {
    match mode {
        ViewerMode::Build => &["system.json"],
        ViewerMode::Robot => &["simrobot.json"],
        ViewerMode::Cad => &["rcad"],
        ViewerMode::Inspect => &["description.json"],
        ViewerMode::Lessons | ViewerMode::Place | ViewerMode::Phenomena => &[],
    }
}

/// The document a typed path names in `mode`: `~/` expanded; for CAD an
/// http(s) URL is a running RoboCAD service. None for empty text, and for
/// a directory (a trailing `/`) in a mode that opens files.
pub(crate) fn typed_document(mode: ViewerMode, text: &str) -> Option<Document> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if mode == ViewerMode::Cad && (text.starts_with("http://") || text.starts_with("https://")) {
        return Some(Document::Url(text.to_string()));
    }
    if !suffixes(mode).is_empty() && text.ends_with('/') {
        return None;
    }
    Some(Document::Path(PathBuf::from(path_field::expand(text))))
}

impl Picker {
    /// Open for `mode` (from `from`, at switcher revision `revision`): the
    /// drafts start over and discovery starts on `Pool::Io`.
    pub(crate) fn open_for(&mut self, mode: ViewerMode, from: ViewerMode, reason: String, revision: u64, presets: Option<PathBuf>) {
        // Closing first moves `listed_at` past every earlier listing.
        self.close();
        self.open = Some(mode);
        self.from = Some(from);
        self.reason = reason;
        self.opened_revision = revision;
        self.presets = presets;
        self.refresh_recents(false, &Recents::default());
    }

    /// Restart discovery only when the explicit owner's snapshot changes.
    /// Drafts, scroll, directory listing and user-edit ownership are untouched.
    pub(crate) fn refresh_recents(&mut self, ready: bool, recents: &Recents) {
        let Some(mode) = self.open else { return };
        let snapshot = (ready, recents.clone());
        if self.recent_snapshot.as_ref() == Some(&snapshot) {
            return;
        }
        self.recent_snapshot = Some(snapshot);
        let recents = recents.clone();
        let presets = self.presets.clone();
        self.sources.start(Pool::Io, format!("the {} document picker's discovery", mode.name()), move |ctx| {
            let root = crate::workspace::root().ok().map(Path::to_path_buf);
            let presets = if mode == ViewerMode::Robot { presets.or_else(|| crate::robot::preset::default_file().ok()) } else { None };
            Ok(discover(mode, root, presets, recents, ready, ctx.cancel_flag()))
        });
    }

    /// Closed: its jobs are dropped (cancelled) and its drafts cleared.
    pub(crate) fn close(&mut self) {
        let revision = self.revision + 1;
        *self = Picker { revision, listed_at: revision, ..Default::default() };
    }

    /// The choices in order (section by section).
    fn flat(&self) -> Vec<(usize, usize, &Choice)> {
        let Some(found) = &self.found else { return Vec::new() };
        found.sections.iter().enumerate().flat_map(|(s, section)| section.choices.iter().enumerate().map(move |(e, c)| (s, e, c))).collect()
    }

    /// The switch entry `entry` of section `section` asks for (None when
    /// closed, out of range or disabled).
    pub(crate) fn choice(&self, section: usize, entry: usize) -> Option<ModeSwitch> {
        let mode = self.open?;
        let choice = self.found.as_ref()?.sections.get(section)?.choices.get(entry)?;
        choice.enabled.then(|| ModeSwitch { mode, document: Some(choice.document.clone()), reveal: None })
    }

    /// The switch the typed path asks for (None when closed or empty).
    pub(crate) fn typed(&self) -> Option<ModeSwitch> {
        let mode = self.open?;
        if self.prefilled {
            return None;
        }
        typed_document(mode, &self.draft.text).map(|d| ModeSwitch { mode, document: Some(d), reveal: None })
    }

    /// Set the draft (a pick, "..", a REST text) and follow it with the listing.
    pub(crate) fn set_text(&mut self, text: String) {
        self.draft = TextDraft { text, select_all: false };
        self.prefilled = false;
        self.edited = true;
        self.follow_listing();
        self.revision += 1;
    }

    /// The listing the draft asks for now (key, directory); None for an
    /// empty or relative draft.
    fn listing_key(&self) -> Option<(String, String)> {
        let mode = self.open?;
        (!self.draft.text.trim().is_empty()).then(|| path_field::listing_key(&self.draft.text, suffixes(mode))).flatten()
    }

    /// Ask for the draft's directory listing when it changed (`Pool::Io`).
    fn follow_listing(&mut self) {
        let Some(mode) = self.open else { return };
        if let Some((key, dir)) = self.listing_key()
            && self.listing_asked.as_deref() != Some(key.as_str())
        {
            self.listing_asked = Some(key.clone());
            path_field::request(&mut self.listing, "the document picker's listing", key, dir, suffixes(mode).iter().map(|s| s.to_string()).collect());
        }
    }

    /// The listing drawn for the draft (only the one it asks for now).
    fn current_listing(&self) -> Option<&Listing> {
        let key = self.listing_key()?;
        self.listed.as_ref().filter(|l| l.key == key.0)
    }

    /// `system_ui` controls while open: one per choice, the path field and Close.
    pub(crate) fn controls(&self) -> Vec<Value> {
        let Some(mode) = self.open else { return Vec::new() };
        let action = |d: &Document| {
            let mut args = Map::new();
            args.insert("mode".into(), json!(mode.name()));
            if let Value::Object(doc) = d.json() {
                args.extend(doc);
            }
            json!({"viewer_mode": args})
        };
        let mut controls: Vec<Value> = self
            .flat()
            .into_iter()
            .enumerate()
            .map(|(n, (_, _, c))| {
                json!({"id": format!("picker:{}:{n}", mode.name()), "picker_revision": self.listed_at, "label": format!("Open {}", c.label), "detail": c.detail, "enabled": c.enabled,
                    "disabled_reason": (!c.enabled).then(|| format!("{}: {}", c.label, c.detail)), "action": action(&c.document)})
            })
            .collect();
        let typed = typed_document(mode, &self.draft.text);
        controls.push(json!({"id": "picker:path", "label": "Open file…", "text": self.draft.text, "enabled": true,
            "note": "activate with text (the path to open) to set it and open it", "action": typed.as_ref().map(action)}));
        controls.push(json!({"id": "picker:close", "label": "Close the picker", "enabled": true}));
        controls
    }

    /// As `viewer_mode {}` reports it.
    pub(crate) fn json(&self) -> Value {
        match self.open {
            None => Value::Null,
            Some(mode) => json!({"mode": mode, "reason": self.reason, "text": self.draft.text, "discovering": self.found.is_none(), "choices": self.flat().len()}),
        }
    }
}

/// The open picker's controls in `world` (empty when closed or absent).
pub(crate) fn controls_in(world: &World) -> Vec<Value> {
    world.get_resource::<Picker>().map_or_else(Vec::new, Picker::controls)
}

/// What a `picker:*` activation asks for.
pub(crate) enum Activation {
    Switch(ModeSwitch),
    Closed(ViewerMode),
}

/// A `system_ui` activation of a `picker:*` id: (id, the optional `text`,
/// the optional `picker_revision` it was listed at).
pub(crate) fn activation(args: &Map<String, Value>) -> Option<(String, Option<String>, Option<u64>)> {
    let action = args.get("action")?;
    if action["operation"] != "activate" {
        return None;
    }
    let id = action["id"].as_str().filter(|id| id.starts_with("picker:"))?;
    Some((id.to_string(), action.get("text").and_then(Value::as_str).map(str::to_string), action.get("picker_revision").and_then(Value::as_u64)))
}

/// Apply a `picker:*` activation: an entry or the path field becomes the
/// switch it asks for; Close closes. An entry (`picker:<mode>:<n>`) given a
/// `picker_revision` other than the one it is listed at now is refused, naming
/// both (its index may name another document). Err names why nothing happened.
pub(crate) fn activate(world: &mut World, id: &str, text: Option<&str>, ui_revision: Option<u64>) -> Result<Activation, String> {
    let Some(mut picker) = world.get_resource_mut::<Picker>() else { return Err(format!("{id}: no document picker is open in this window")) };
    let Some(mode) = picker.open else {
        return Err(format!("{id}: no document picker is open (it opens when a mode with no document is chosen in the mode switcher, or with system_ui mode:<mode>)"));
    };
    match id {
        "picker:close" => {
            picker.close();
            blur_path(world);
            Ok(Activation::Closed(mode))
        }
        "picker:path" => {
            if let Some(text) = text {
                picker.set_text(text.to_string());
            }
            picker.typed().map(Activation::Switch).ok_or_else(|| format!("picker:path: no path to open in {} mode (activate with text: a file, not a folder, except for lessons and place)", mode.name()))
        }
        _ => {
            let rest = id.strip_prefix("picker:").unwrap_or(id);
            let (m, n) = rest.split_once(':').ok_or_else(|| format!("{id}: not a picker control (picker:<mode>:<n>, picker:path or picker:close)"))?;
            if m != mode.name() {
                return Err(format!("{id}: the open picker is {}'s (picker:{}:<n>)", mode.name(), mode.name()));
            }
            if let Some(listed) = ui_revision
                && listed != picker.listed_at
            {
                return Err(format!("{id} was listed at picker_revision {listed}; the picker is now at {}; list the controls again", picker.listed_at));
            }
            let n: usize = n.parse().map_err(|_| format!("{id}: <n> is not a number"))?;
            let flat = picker.flat();
            let Some(&(s, e, c)) = flat.get(n) else {
                return Err(if picker.found.is_none() { format!("{id}: the picker is still looking for documents") } else { format!("{id}: the picker has {} entries", flat.len()) });
            };
            if !c.enabled {
                return Err(format!("{id}: {} is not available ({})", c.label, c.detail));
            }
            picker.choice(s, e).map(Activation::Switch).ok_or_else(|| format!("{id}: not available"))
        }
    }
}

/// The path field gives up the keyboard (a close from `system_ui`).
fn blur_path(world: &mut World) {
    let path = world.query::<(Entity, &FieldId)>().iter(world).find(|(_, f)| **f == PATH).map(|(e, _)| e);
    if let Some(path) = path
        && let Some(mut focus) = world.get_resource_mut::<InputFocus>()
        && focus.get() == Some(path)
    {
        focus.clear();
    }
}

// --- Systems ---

/// A clickable part of the picker.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct PickerPart(pub PickHit, pub u64);

/// The picker's root (the backdrop).
#[derive(Component)]
pub(crate) struct PickerRoot;

/// The picker's scrolling sections (the wheel moves them).
#[derive(Component)]
pub(crate) struct PickerList;

/// JobResults: discovery and the listing land; the picker closes when the
/// window's mode changed since it opened (its choice succeeded, or another
/// mode was chosen in the switcher).
pub(crate) fn receive(
    picker: Option<ResMut<Picker>>,
    settings: Option<Res<SettingsOwner>>,
    mode: Option<Res<State<ViewerMode>>>,
    hardware: Option<Res<crate::robot::hardware::Hardware>>,
    switch: Option<ResMut<Switcher>>,
    mut text: TextFocus,
    mut watched: Local<Option<u64>>,
) {
    let Some(mut picker) = picker else { return };
    if picker.open.is_none() {
        // Closed with nothing chosen (Escape, Close, picker:close): the
        // switcher's "Choose … in the picker." line goes with it. A later
        // outcome (a switch, a refusal) bumped the revision and stays.
        if let (Some(opened), Some(mut switch)) = (watched.take(), switch)
            && switch.revision == opened
        {
            switch.message = None;
            switch.revision += 1;
        }
        return;
    }
    *watched = Some(picker.opened_revision);
    // The Leg calibration panel opened while the picker was up (a remote
    // toggle): its STOP must stay reachable, so the modal picker closes.
    let hardware_open = hardware.is_some_and(|h| h.open);
    if hardware_open
        || mode.is_some_and(|mode| picker.from != Some(*mode.get()))
    {
        picker.close();
        // Now, not at the next frame's `sync`: the Leg panel's keys run under `not(typing)`.
        text.blur(PATH);
        return;
    }
    let picker = &mut *picker;
    if let Some(settings) = settings {
        picker.refresh_recents(settings.ready, &settings.recents);
    }
    if picker.sources.pending().is_some()
        && let Some((_, result)) = picker.sources.poll()
    {
        let sources = result.unwrap_or_else(|e| Sources {
            sections: vec![Section { title: "Documents".into(), empty: format!("Could not look for documents: {e}"), choices: Vec::new() }],
            start_dir: None,
        });
        // The path field starts in the examples, unless something was typed
        // (selected while it has the keyboard, as a focus selects it).
        if picker.draft.text.is_empty()
            && !picker.edited
            && let Some(dir) = &sources.start_dir
        {
            picker.draft = TextDraft { text: dir.clone(), select_all: text.focused(PATH) };
            picker.prefilled = true;
            picker.follow_listing();
        }
        picker.found = Some(sources);
        picker.revision += 1;
        picker.listed_at = picker.revision;
    }
    if path_field::receive(&mut picker.listing, &mut picker.listed) {
        picker.revision += 1;
    }
}

/// Input: a press on the picker (an entry, the path field's parts, Close).
pub(crate) fn clicks(picker: Option<ResMut<Picker>>, parts: Query<(&PickerPart, Option<&Enabled>), With<crate::ui_kit::activation::Activated>>, mut text: TextFocus, mut out: MessageWriter<Act<WindowAction>>) {
    let Some(mut picker) = picker else { return };
    if picker.open.is_none() {
        return;
    }
    for (part, enabled) in &parts {
        // Refuse the old rendered source snapshot before resolving positional rows.
        if part.1 != picker.revision || enabled.is_some_and(|e| !e.0) {
            continue;
        }
        match part.0 {
            PickHit::Entry(s, e) => {
                if let Some(request) = picker.choice(s, e) {
                    out.write(Act::ui(WindowAction::Switch(request)));
                }
            }
            PickHit::Path(PathHit::Field) => {
                picker.draft.select_all = true;
                let draft = picker.draft.clone();
                text.focus_draft(PATH, draft);
            }
            PickHit::Path(PathHit::Entry(i)) => {
                let pick = picker.current_listing().and_then(|l| l.entries.get(i).map(|(name, is_dir)| path_field::pick(&picker.draft.text, &l.dir, name, *is_dir, false)));
                if let Some(path) = pick {
                    picker.set_text(path);
                }
            }
            PickHit::Path(PathHit::Up) => {
                let path = path_field::up(&picker.draft.text, false);
                picker.set_text(path);
            }
            PickHit::Path(PathHit::Submit) => {
                if let Some(request) = picker.typed() {
                    out.write(Act::ui(WindowAction::Switch(request)));
                }
            }
            PickHit::Close => {
                picker.close();
                text.blur(PATH);
            }
        }
    }
}

impl Picker {
    /// A typed edit: the draft (with its selection) and the listing follow.
    fn set_text_selected(&mut self, draft: TextDraft) {
        self.draft = draft;
        self.prefilled = false;
        self.edited = true;
        self.follow_listing();
        self.revision += 1;
    }
}

/// Present: the picker over the window (a backdrop that leaves the switcher
/// strip usable), rebuilt when anything it shows changes; despawned when
/// closed. `Persistent`, so the mode scope's sweep leaves it alone.
pub(crate) fn draw(mut commands: Commands, picker: Option<Res<Picker>>, switch: Option<Res<Switcher>>, typing: Typing, fonts: Res<UiFonts>, roots: Query<Entity, With<PickerRoot>>, mut last: Local<Option<String>>) {
    let open = picker.as_deref().filter(|p| p.open.is_some());
    // A later outcome of the switcher (a refused choice, a load in progress) is the status line.
    let status = open.zip(switch.as_deref()).and_then(|(p, s)| (s.revision > p.opened_revision).then(|| s.message.clone()).flatten());
    // Source/presentation content changes rebuild the picker; focus alone is
    // owned by the kit ring and must retain the PATH anchor for modal return.
    let focused = typing.focused(PATH);
    let key = open.map(|p| format!("{:?}|{}|{status:?}", p.open, p.revision));
    let shown = roots.iter().next().is_some();
    if key == *last && shown == key.is_some() {
        return;
    }
    *last = key;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some((picker, mode)) = open.and_then(|p| p.open.map(|m| (p, m))) else { return };
    let k = Kit::new(&fonts);
    let sections: Vec<PickerSection> = match &picker.found {
        None => vec![PickerSection { title: "Documents".into(), entries: Vec::new(), empty: "Looking for documents…".into() }],
        Some(found) => found
            .sections
            .iter()
            .map(|s| PickerSection { title: s.title.clone(), entries: s.choices.iter().map(|c| PickerEntry { label: c.label.clone(), detail: c.detail.clone(), enabled: c.enabled }).collect(), empty: s.empty.clone() })
            .collect(),
    };
    let status = status.as_ref().map(|m| match m {
        Ok(text) => (text.as_str(), SUBTLE),
        Err(text) => (text.as_str(), DANGER),
    });
    let placeholder = match mode {
        ViewerMode::Cad => "/path/to/model.rcad or http://127.0.0.1:8420".to_string(),
        m => match suffixes(m).first() {
            Some(s) => format!("/path/to/file.{s}"),
            None => "/path/to/folder".to_string(),
        },
    };
    let view = PathView {
        label: "Open file…",
        text: &picker.draft.text,
        placeholder: &placeholder,
        focused,
        selected: focused && picker.draft.select_all,
        submit: Some("Open"),
        submit_enabled: picker.typed().is_some(),
        listing: picker.current_listing(),
    };
    let title = title(mode);
    commands.spawn((k.backdrop(&title, false), PickerRoot, Persistent)).with_children(|backdrop| {
        k.document_picker(backdrop, &title, &picker.reason, status, &sections, &view, picker.scroll, PickerList, |hit| PickerPart(hit, picker.revision));
    });
}

#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod activation_tests;
