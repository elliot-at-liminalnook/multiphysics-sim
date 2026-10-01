//! RoboCAD's command palette (`CommandPalette`, ui/widgets.py:56-108;
//! `command_palette`, Ctrl+Space or Shift+F): a search field over every
//! RoboCAD command (`registry::COMMANDS`: label, category, keys), ranked as
//! RoboCAD ranks (`ui_kit::palette::rank`), the first 60 shown, each row
//! "Category: Label  (note)    [keys]" with RoboCAD's "⚠ conflicts with …"
//! where a key is bound to another command too. The note names what a
//! command needs that this viewer lacks: the owning epic, "GUI-only" or
//! "not ported"; such rows are disabled.
//!
//! Keys and conflicts come from the registry (RoboCAD's keymap.json and
//! inline keys, so its own Ctrl+Shift+M clash shows), or, when RoboCAD's
//! desktop window serves the document, from its `/commands` (a user's
//! `~/.robocad/keymap.json` included).
//!
//! While it is open the palette has the keyboard (`CadInputFocus`):
//! typing edits the query (`TextDraft`), Up and Down move the highlight,
//! Enter runs the highlighted row, Escape closes it (`surfaces::input`); a
//! click on a row runs it. Running writes `CadInvoke { id }` and closes
//! the palette (a command that opens another surface replaces it); a
//! disabled row shows why on the status line and the palette stays open.
use super::registry::{self, COMMANDS, Command, Resolved};
use super::{POPUP_Z, Surface, SurfaceEntry, SurfaceRoot};
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadInputFocus};
use crate::cad::panel::{Control, own_controls};
use crate::ui_kit::form::{DraftKey, TextDraft};
use crate::ui_kit::palette::{PaletteEntry, rank};
use crate::ui_kit::{Kit, LEFT_WIDTH, TOPBAR};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::Value;

/// The palette's search field (always focused while it is open).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PaletteField;

/// The palette's width (RoboCAD's dialog minimum, as the kit draws it).
const WIDTH: f32 = 520.0;

/// A command's keys as the palette lists them: RoboCAD's `/commands` keys
/// when its desktop window serves the document, else the registry's.
fn keys_of(cmd: &Command, doc: &CadDocument) -> Vec<String> {
    if registry::gui(doc)
        && let Some(Ok(commands)) = &doc.commands
        && let Some(info) = commands.get(cmd.id)
    {
        return match &info.keys {
            Value::String(s) if !s.is_empty() => vec![s.clone()],
            Value::Array(items) => items.iter().filter_map(|k| k.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        };
    }
    cmd.keys.iter().map(|k| k.to_string()).collect()
}

/// Every command as a palette entry, in registry order.
pub(super) fn palette_entries(doc: &CadDocument, own: &[Control]) -> Vec<PaletteEntry> {
    COMMANDS
        .iter()
        .map(|cmd| PaletteEntry { id: cmd.id.to_string(), label: cmd.label.to_string(), category: cmd.category.to_string(), keys: keys_of(cmd, doc), note: registry::note(cmd), enabled: registry::ready(cmd, doc, own).is_ok() })
        .collect()
}

/// The ranked rows for `query`: each row's command and RoboCAD's row text.
pub(super) fn ranked(doc: &CadDocument, own: &[Control], query: &str) -> Vec<(&'static Command, String)> {
    let all: &'static [Command] = COMMANDS;
    rank(&palette_entries(doc, own), query).into_iter().filter_map(|r| all.get(r.index).map(|cmd| (cmd, r.text))).collect()
}

/// What a row's click or Enter writes.
fn row_entry(cmd: &Command, doc: &CadDocument, own: &[Control]) -> SurfaceEntry {
    SurfaceEntry {
        action: CadAction::CadInvoke { id: cmd.id.to_string() },
        closes: !matches!(registry::resolve(cmd), Resolved::Surface(_)),
        refusal: registry::ready(cmd, doc, own).err().map(|why| registry::status_line(cmd, &why)),
    }
}

/// The palette popup (`surfaces::draw`), centred near the window's top as
/// RoboCAD opens it (`open_palette`: x = width / 2, y = 80).
pub(super) fn spawn(commands: &mut Commands, k: &Kit, doc: &CadDocument, own: &[Control], query: &str, highlight: usize, width: f32) {
    let entries = palette_entries(doc, own);
    let rows = rank(&entries, query);
    let all: &'static [Command] = COMMANDS;
    let actions: Vec<SurfaceEntry> = rows.iter().filter_map(|r| all.get(r.index)).map(|cmd| row_entry(cmd, doc, own)).collect();
    let selected = highlight.min(rows.len().saturating_sub(1));
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(((width - WIDTH) / 2.0).max(LEFT_WIDTH)), top: Val::Px(TOPBAR + 28.0), ..default() },
            FocusPolicy::Block,
            GlobalZIndex(POPUP_Z),
            AccessibleLabel::new("Command palette"),
            SurfaceRoot,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|p| {
            k.palette(p, query, &rows, &entries, selected, PaletteField, |i| {
                actions.get(i).cloned().unwrap_or(SurfaceEntry { action: CadAction::CadSurface { surface: Surface::Closed }, closes: false, refusal: None })
            });
        });
}

/// Input: the palette's keys while it is open (see the module doc).
#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    doc: Option<ResMut<CadDocument>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut events: MessageReader<KeyboardInput>,
    focus: Option<ResMut<CadInputFocus>>,
    mut out: MessageWriter<Act<CadAction>>,
    mut was_open: Local<bool>,
) {
    let open = doc.as_deref().and_then(|d| d.ops.surface.as_ref()).and_then(|o| match &o.surface {
        Surface::Palette { query } => Some((query.clone(), o.highlight.unwrap_or(0))),
        _ => None,
    });
    let (Some(mut doc), Some((query, highlight))) = (doc, open) else {
        events.clear();
        // The frame it closes is still the palette's (its Escape or Enter is no CAD key).
        if std::mem::take(&mut *was_open) {
            hold(focus);
        }
        return;
    };
    hold(focus);
    if !*was_open {
        // Keys pressed before it opened (its own Ctrl+Space or Shift+F) are not its text.
        *was_open = true;
        events.clear();
        return;
    }
    let typed: Vec<KeyboardInput> = events.read().filter(|e| e.state == ButtonState::Pressed).cloned().collect();
    if typed.is_empty() {
        return;
    }
    let chord = keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]));
    let own = own_controls(&doc);
    let mut rows = ranked(&doc, &own, &query);
    let (mut text, mut at) = (query.clone(), highlight);
    for e in typed {
        match &e.logical_key {
            Key::ArrowUp => at = at.saturating_sub(1),
            Key::ArrowDown => at = (at + 1).min(rows.len().saturating_sub(1)),
            Key::Enter => {
                if let Some((cmd, _)) = rows.get(at) {
                    let entry = row_entry(cmd, &doc, &own);
                    match entry.refusal {
                        None => {
                            out.write(Act::ui(entry.action));
                            if entry.closes {
                                out.write(Act::ui(CadAction::CadSurface { surface: Surface::Closed }));
                            }
                        }
                        Some(why) => doc.show(Err(why)),
                    }
                }
                break;
            }
            key => {
                let mut draft = TextDraft { text: text.clone(), select_all: false };
                if draft.key(key, chord) == DraftKey::Edited {
                    text = draft.text;
                    at = 0;
                    rows = ranked(&doc, &own, &text);
                }
            }
        }
    }
    if (text != query || at != highlight)
        && let Some(o) = doc.ops.surface.as_mut()
        && matches!(o.surface, Surface::Palette { .. })
    {
        o.surface = Surface::Palette { query: text };
        o.highlight = Some(at);
    }
}

/// The palette holds the keyboard this frame.
fn hold(focus: Option<ResMut<CadInputFocus>>) {
    if let Some(mut focus) = focus
        && !focus.0
    {
        focus.0 = true;
    }
}
