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
//!   listing, read on `Pool::Io` when the directory changes.
//! - **Choices are switches**: an entry, the path field's Open (and Enter)
//!   write the same `WindowAction::Switch` that `viewer_mode {mode, path |
//!   preset | url}` builds ([`Picker::choice`], [`Picker::typed`]); there
//!   is no second switch path. A refused choice keeps the picker open and
//!   shows the switcher's refusal as its status line.
//! - **Closing**: when the window's mode changes (the choice succeeded, or
//!   another mode was chosen in the switcher), on Close and on Escape.
//! - **Modal** ([`keys`], PreUpdate after input): while open, keys and the
//!   wheel go to the picker only; the keyboard and wheel messages are
//!   cleared and the held keys released (not reset, so a held W or Q/A
//!   underneath sees its release and stops), so no mode shortcut fires
//!   underneath. It does not open over robot mode's Leg calibration panel,
//!   whose STOP must stay reachable.
//! - **`system_ui`**: its entries are controls `picker:<mode>:<n>` (flat
//!   index across the sections), `picker:path` (activate with an optional
//!   `text`, set and submitted) and `picker:close`, appended to every
//!   mode's controls while it is open (`route::annotate`).
mod discover;

pub(crate) use discover::discover;

use super::actions::Act;
use super::recent;
use super::switch::{Document, ModeSwitch, Switcher, WindowAction};
use super::{Persistent, ViewerMode};
use crate::builder::ui_api::Enabled;
use crate::jobs::{Latest, Pool};
use crate::ui_kit::form::{DraftKey, TextDraft};
use crate::ui_kit::path_field::{self, Listing, PathHit, PathView};
use crate::ui_kit::picker::{PickHit, PickerEntry, PickerSection};
use crate::ui_kit::{DANGER, Kit, SUBTLE, UiFonts};
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardFocusLost, KeyboardInput};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

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
    /// The path field's draft.
    pub(crate) draft: TextDraft,
    /// The path field has the keyboard.
    pub(crate) focused: bool,
    /// The sections' scroll offset (px).
    pub(crate) scroll: f32,
    sources: Latest<Sources>,
    pub(crate) found: Option<Sources>,
    listing: Latest<Listing>,
    pub(crate) listed: Option<Listing>,
    listing_asked: Option<String>,
    /// The switcher's revision when it opened: a later outcome (a refused
    /// choice, a load in progress) is its status line.
    pub(crate) opened_revision: u64,
    /// Bumped on every change it shows.
    pub(crate) revision: u64,
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
        self.close();
        self.open = Some(mode);
        self.from = Some(from);
        self.reason = reason;
        self.opened_revision = revision;
        self.sources.start(Pool::Io, format!("the {} document picker's discovery", mode.name()), move |ctx| {
            // The workspace root and the config directory are read here, off the UI thread.
            let root = crate::workspace::root().ok().map(Path::to_path_buf);
            let presets = if mode == ViewerMode::Robot { presets.or_else(|| crate::robot::preset::default_file().ok()) } else { None };
            // Closing the picker drops the job, which cancels the walk.
            Ok(discover(mode, root, presets, recent::file(), ctx.cancel_flag()))
        });
    }

    /// Closed: its jobs are dropped (cancelled) and its drafts cleared.
    pub(crate) fn close(&mut self) {
        let revision = self.revision + 1;
        *self = Picker { revision, ..Default::default() };
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
        choice.enabled.then(|| ModeSwitch { mode, document: Some(choice.document.clone()) })
    }

    /// The switch the typed path asks for (None when closed or empty).
    pub(crate) fn typed(&self) -> Option<ModeSwitch> {
        let mode = self.open?;
        typed_document(mode, &self.draft.text).map(|d| ModeSwitch { mode, document: Some(d) })
    }

    /// Set the draft (a pick, "..", a REST text) and follow it with the listing.
    pub(crate) fn set_text(&mut self, text: String) {
        self.draft = TextDraft { text, select_all: false };
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
                json!({"id": format!("picker:{}:{n}", mode.name()), "label": format!("Open {}", c.label), "detail": c.detail, "enabled": c.enabled,
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

/// A `system_ui` activation of a `picker:*` id: (id, the optional `text`).
pub(crate) fn activation(args: &Map<String, Value>) -> Option<(String, Option<String>)> {
    let action = args.get("action")?;
    if action["operation"] != "activate" {
        return None;
    }
    let id = action["id"].as_str().filter(|id| id.starts_with("picker:"))?;
    Some((id.to_string(), action.get("text").and_then(Value::as_str).map(str::to_string)))
}

/// Apply a `picker:*` activation: an entry or the path field becomes the
/// switch it asks for; Close closes. Err names why nothing happened.
pub(crate) fn activate(world: &mut World, id: &str, text: Option<&str>) -> Result<Activation, String> {
    let Some(mut picker) = world.get_resource_mut::<Picker>() else { return Err(format!("{id}: no document picker is open in this window")) };
    let Some(mode) = picker.open else {
        return Err(format!("{id}: no document picker is open (it opens when a mode with no document is chosen in the mode switcher, or with system_ui mode:<mode>)"));
    };
    match id {
        "picker:close" => {
            picker.close();
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

// --- Systems ---

/// A clickable part of the picker.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct PickerPart(pub PickHit);

/// The picker's root (the backdrop).
#[derive(Component)]
pub(crate) struct PickerRoot;

/// The picker's scrolling sections (the wheel moves them).
#[derive(Component)]
pub(crate) struct PickerList;

/// JobResults: discovery and the listing land; the picker closes when the
/// window's mode changed since it opened (its choice succeeded, or another
/// mode was chosen in the switcher).
pub(crate) fn receive(picker: Option<ResMut<Picker>>, mode: Option<Res<State<ViewerMode>>>, hardware: Option<Res<crate::robot::hardware::Hardware>>) {
    let Some(mut picker) = picker else { return };
    if picker.open.is_none() {
        return;
    }
    // The Leg calibration panel opened while the picker was up (a remote
    // toggle): its STOP must stay reachable, so the modal picker closes.
    let hardware_open = hardware.is_some_and(|h| h.open);
    if hardware_open
        || mode.is_some_and(|mode| picker.from != Some(*mode.get()))
    {
        picker.close();
        return;
    }
    let picker = &mut *picker;
    if picker.sources.pending().is_some()
        && let Some((_, result)) = picker.sources.poll()
    {
        let sources = result.unwrap_or_else(|e| Sources {
            sections: vec![Section { title: "Documents".into(), empty: format!("Could not look for documents: {e}"), choices: Vec::new() }],
            start_dir: None,
        });
        // The path field starts in the examples, unless something was typed.
        if picker.draft.text.is_empty()
            && !picker.focused
            && let Some(dir) = &sources.start_dir
        {
            picker.draft = TextDraft { text: dir.clone(), select_all: false };
            picker.follow_listing();
        }
        picker.found = Some(sources);
        picker.revision += 1;
    }
    if path_field::receive(&mut picker.listing, &mut picker.listed) {
        picker.revision += 1;
    }
}

/// Input: a press on the picker (an entry, the path field's parts, Close).
pub(crate) fn clicks(picker: Option<ResMut<Picker>>, parts: Query<(&Interaction, &PickerPart, Option<&Enabled>), Changed<Interaction>>, mut out: MessageWriter<Act<WindowAction>>) {
    let Some(mut picker) = picker else { return };
    if picker.open.is_none() {
        return;
    }
    for (interaction, part, enabled) in &parts {
        if *interaction != Interaction::Pressed || enabled.is_some_and(|e| !e.0) {
            continue;
        }
        match part.0 {
            PickHit::Entry(s, e) => {
                if let Some(request) = picker.choice(s, e) {
                    out.write(Act::ui(WindowAction::Switch(request)));
                }
            }
            PickHit::Path(PathHit::Field) => {
                picker.focused = true;
                picker.draft.select_all = true;
                picker.revision += 1;
            }
            PickHit::Path(PathHit::Entry(i)) => {
                let pick = picker.current_listing().and_then(|l| l.entries.get(i).map(|(name, is_dir)| path_field::pick(&picker.draft.text, &l.dir, name, *is_dir, false)));
                if let Some(text) = pick {
                    picker.set_text(text);
                }
            }
            PickHit::Path(PathHit::Up) => {
                let text = path_field::up(&picker.draft.text, false);
                picker.set_text(text);
            }
            PickHit::Path(PathHit::Submit) => {
                if let Some(request) = picker.typed() {
                    out.write(Act::ui(WindowAction::Switch(request)));
                }
            }
            PickHit::Close => picker.close(),
        }
    }
}

/// The input messages and states the picker reads and then clears.
#[derive(Default)]
pub(crate) struct Cursors {
    keys: Option<MessageCursor<KeyboardInput>>,
    wheel: Option<MessageCursor<MouseWheel>>,
    focus: Option<MessageCursor<KeyboardFocusLost>>,
    /// The Control/Super keys held, followed from the keyboard messages
    /// while open (the picker releases `ButtonInput<KeyCode>` every frame,
    /// so its state can't say whether Cmd is held); equal to its pressed
    /// ones while closed.
    modifiers: Vec<KeyCode>,
}

/// The keys that make a typed key a chord (Cmd+V pastes, not "v").
const CHORD_KEYS: [KeyCode; 4] = [KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight];

/// Releases every held input so the systems underneath see `just_released`
/// once (a robot walking on a held W, a hardware jog on a held Q/A stops),
/// and no new press: one made this frame is dropped, unless it re-presses
/// a key also released this frame (then its release stays).
fn release_held<T: Clone + Eq + std::hash::Hash + Send + Sync + 'static>(input: &mut ButtonInput<T>) {
    let fresh: Vec<T> = input.get_just_pressed().filter(|k| !input.just_released((*k).clone())).cloned().collect();
    input.clear();
    input.release_all();
    for key in fresh {
        input.reset(key);
    }
}

/// PreUpdate, after Bevy's input systems: while the picker is open, its
/// keys (typing into the focused draft; Enter submits, Escape closes, Tab
/// focuses the field; a key is a chord when a Control/Super key is held at
/// its message, followed in message order by [`Cursors`]) and the wheel
/// (its sections), then every keyboard and wheel message is cleared and
/// the key states released, so nothing underneath sees them.
///
/// Safety: the key states are released, never reset ([`release_held`]): a
/// reset would forget a held key without a release, so a robot walking on
/// a held W, or a hardware jog on a held Q/A, would never see the release
/// and keep moving. The picker does not open over robot mode's Leg
/// calibration panel (`switch::start` refuses), because it would cover the
/// panel's STOP button and take its Z/Escape keys.
///
/// Closed, it only keeps its cursors at the newest message and its
/// modifiers equal to the held Control/Super keys.
#[allow(clippy::too_many_arguments)]
pub(crate) fn keys(
    picker: Option<ResMut<Picker>>,
    keyboard: Option<ResMut<Messages<KeyboardInput>>>,
    wheel: Option<ResMut<Messages<MouseWheel>>>,
    focus_lost: Option<Res<Messages<KeyboardFocusLost>>>,
    codes: Option<ResMut<ButtonInput<KeyCode>>>,
    logical: Option<ResMut<ButtonInput<Key>>>,
    mut cursors: Local<Cursors>,
    mut lists: Query<&mut ScrollPosition, With<PickerList>>,
    mut out: MessageWriter<Act<WindowAction>>,
) {
    let open = picker.as_ref().is_some_and(|p| p.open.is_some());
    let (Some(mut picker), true) = (picker, open) else {
        // Closed: what arrives from now on is the picker's once it opens.
        if let Some(keyboard) = &keyboard {
            cursors.keys = Some(keyboard.get_cursor_current());
        }
        if let Some(wheel) = &wheel {
            cursors.wheel = Some(wheel.get_cursor_current());
        }
        if let Some(focus_lost) = &focus_lost {
            cursors.focus = Some(focus_lost.get_cursor_current());
        }
        cursors.modifiers = codes.as_ref().map_or_else(Vec::new, |k| CHORD_KEYS.into_iter().filter(|c| k.pressed(*c)).collect());
        return;
    };
    let cursors = &mut *cursors;
    // The window lost the keyboard: a held modifier's release will not arrive.
    if let Some(messages) = &focus_lost {
        let cursor = cursors.focus.get_or_insert_with(|| messages.get_cursor_current());
        if cursor.read(messages).count() > 0 {
            cursors.modifiers.clear();
        }
    }
    // Each pressed key with whether a modifier was held at it (in message order).
    let mut typed: Vec<(Key, bool)> = Vec::new();
    if let Some(messages) = &keyboard {
        let cursor = cursors.keys.get_or_insert_with(|| messages.get_cursor_current());
        for e in cursor.read(messages) {
            let modifier = CHORD_KEYS.contains(&e.key_code);
            match e.state {
                ButtonState::Pressed if modifier => {
                    if !cursors.modifiers.contains(&e.key_code) {
                        cursors.modifiers.push(e.key_code);
                    }
                }
                ButtonState::Released if modifier => cursors.modifiers.retain(|c| *c != e.key_code),
                ButtonState::Pressed => typed.push((e.logical_key.clone(), !cursors.modifiers.is_empty())),
                ButtonState::Released => {}
            }
        }
    }
    let delta: f32 = match &wheel {
        Some(messages) => {
            let cursor = cursors.wheel.get_or_insert_with(|| messages.get_cursor_current());
            cursor
                .read(messages)
                .map(|e| match e.unit {
                    MouseScrollUnit::Line => e.y * crate::ui_kit::WHEEL_LINE,
                    MouseScrollUnit::Pixel => e.y,
                })
                .sum()
        }
        None => 0.0,
    };
    let (mut submit, mut close) = (false, false);
    for (key, chord) in typed {
        if submit || close {
            break;
        }
        if picker.focused {
            let mut draft = picker.draft.clone();
            match draft.key(&key, chord) {
                DraftKey::Edited => picker.set_text_selected(draft),
                DraftKey::Enter => submit = true,
                DraftKey::Escape => close = true,
                DraftKey::Tab => {
                    picker.focused = false;
                    picker.revision += 1;
                }
                DraftKey::Ignored => {}
            }
        } else {
            match key {
                Key::Enter => submit = true,
                Key::Escape => close = true,
                Key::Tab => {
                    picker.focused = true;
                    picker.draft.select_all = true;
                    picker.revision += 1;
                }
                _ => {}
            }
        }
    }
    if close {
        picker.close();
    } else if submit && let Some(request) = picker.typed() {
        out.write(Act::ui(WindowAction::Switch(request)));
    }
    if delta != 0.0 {
        for mut position in &mut lists {
            // `ui_kit::clamp_scroll_positions` clamps the far end after layout.
            position.y = (position.y - delta).max(0.0);
            picker.scroll = position.y;
        }
    }
    // Modal: nothing underneath sees these keys or the wheel, and every
    // held key is released (never reset: see the doc above).
    if let Some(mut codes) = codes {
        release_held(&mut *codes);
    }
    if let Some(mut logical) = logical {
        release_held(&mut *logical);
    }
    if let Some(mut keyboard) = keyboard {
        keyboard.clear();
    }
    if let Some(mut wheel) = wheel {
        wheel.clear();
    }
}

impl Picker {
    /// A typed edit: the draft (with its selection) and the listing follow.
    fn set_text_selected(&mut self, draft: TextDraft) {
        self.draft = draft;
        self.follow_listing();
        self.revision += 1;
    }
}

/// Present: the picker over the window (a backdrop that leaves the switcher
/// strip usable), rebuilt when anything it shows changes; despawned when
/// closed. `Persistent`, so the mode scope's sweep leaves it alone.
pub(crate) fn draw(mut commands: Commands, picker: Option<Res<Picker>>, switch: Option<Res<Switcher>>, fonts: Res<UiFonts>, roots: Query<Entity, With<PickerRoot>>, mut last: Local<Option<String>>) {
    let open = picker.as_deref().filter(|p| p.open.is_some());
    // A later outcome of the switcher (a refused choice, a load in progress) is the status line.
    let status = open.zip(switch.as_deref()).and_then(|(p, s)| (s.revision > p.opened_revision).then(|| s.message.clone()).flatten());
    // Every change the picker shows bumps its revision (not the scroll offset, which is the node's own).
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
        focused: picker.focused,
        selected: picker.focused && picker.draft.select_all,
        submit: Some("Open"),
        submit_enabled: picker.typed().is_some(),
        listing: picker.current_listing(),
    };
    let title = title(mode);
    commands.spawn((k.backdrop(&title, false), PickerRoot, Persistent)).with_children(|backdrop| {
        k.document_picker(backdrop, &title, &picker.reason, status, &sections, &view, picker.scroll, PickerList, PickerPart);
    });
}
