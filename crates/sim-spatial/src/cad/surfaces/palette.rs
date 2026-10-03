//! RoboCAD's command palette (`CommandPalette`, ui/widgets.py:56-108;
//! `command_palette`, Ctrl+Space or Shift+F): a search field over every
//! RoboCAD command (`registry::COMMANDS`: label, category, keys), ranked as
//! RoboCAD ranks (`ui_kit::palette::rank`), the first 60 shown, each row
//! "Category: Label  (note)    [keys]" with RoboCAD's "⚠ conflicts with …"
//! where a key is bound to another command too. The note names what a
//! command needs that this viewer lacks: the owning epic or "not ported";
//! such rows are disabled.
//!
//! Keys and conflicts come from the registry (RoboCAD's keymap.json and
//! inline keys, so its own Ctrl+Shift+M clash shows), or, when RoboCAD's
//! desktop window serves the document, from its `/commands` (a user's
//! `~/.robocad/keymap.json` included).
//!
//! While it is open its search field ([`PALETTE`], a kit text field) has
//! the keyboard: typing edits the query (re-ranked, the highlight back on
//! the first row), Enter runs the highlighted row, Escape closes it, and so
//! does the field losing the keyboard (a mode switch, another field); a
//! click on a row runs it; Up and Down move the highlight (the field's
//! `FieldEvent::Arrow`). Running writes `CadInvoke { id }` and closes
//! the palette (a command that opens another surface replaces it); a
//! disabled row shows why on the status line and the palette stays open.
use super::registry::{self, COMMANDS, Command, Resolved};
use super::{POPUP_Z, PopupScroll, Surface, SurfaceEntry, SurfaceRoot};
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::{Control, own_controls};
use crate::cad::selection::CadSelection;
use sim_runtime::cad_client::SelectionItem;
use crate::ui_kit::palette::{PaletteEntry, rank};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextFocus};
use crate::ui_kit::{Kit, LEFT_WIDTH, TOPBAR};
use bevy::ecs::system::ParamSet;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::Value;

/// The palette's search field's press action (it has the keyboard while
/// the palette is open).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PaletteField;

/// The palette's search field (`ui_kit::text`): sticky, so a press on a row
/// does not take the keyboard from it (a press outside the popup closes the
/// palette, `surfaces::input`).
pub(in crate::cad) const PALETTE: FieldId = FieldId("cad.palette");

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
pub(super) fn palette_entries(doc: &CadDocument, selection: &[SelectionItem], own: &[Control]) -> Vec<PaletteEntry> {
    COMMANDS
        .iter()
        .map(|cmd| PaletteEntry { id: cmd.id.to_string(), label: cmd.label.to_string(), category: cmd.category.to_string(), keys: keys_of(cmd, doc), note: registry::note(cmd), enabled: registry::ready(cmd, doc, selection, own).is_ok() })
        .collect()
}

/// The ranked rows for `query`: each row's command and RoboCAD's row text.
pub(super) fn ranked(doc: &CadDocument, selection: &[SelectionItem], own: &[Control], query: &str) -> Vec<(&'static Command, String)> {
    let all: &'static [Command] = COMMANDS;
    rank(&palette_entries(doc, selection, own), query).into_iter().filter_map(|r| all.get(r.index).map(|cmd| (cmd, r.text))).collect()
}

/// What a row's click or Enter writes.
fn row_entry(cmd: &Command, doc: &CadDocument, selection: &[SelectionItem], own: &[Control]) -> SurfaceEntry {
    SurfaceEntry {
        action: CadAction::CadInvoke { id: cmd.id.to_string() },
        closes: !matches!(registry::resolve(cmd), Resolved::Surface(_)),
        refusal: registry::ready(cmd, doc, selection, own).err().map(|why| registry::status_line(cmd, &why)),
    }
}

/// RoboCAD's placeholder (`palette.placeholder`, ui/strings.py: "Type a
/// command… (Ctrl+Space)"), naming the palette's first key as listed now.
fn placeholder(doc: &CadDocument) -> String {
    let key = registry::command("command_palette").and_then(|cmd| keys_of(cmd, doc).into_iter().next());
    match key {
        Some(key) => format!("Type a command\u{2026} ({key})"),
        None => "Type a command\u{2026}".to_string(),
    }
}

/// The palette popup (`surfaces::draw`), centred near the window's top as
/// RoboCAD opens it (`open_palette`: x = width / 2, y = 80).
pub(super) fn spawn(commands: &mut Commands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem], own: &[Control], query: &str, highlight: usize, width: f32) {
    let entries = palette_entries(doc, selection, own);
    let rows = rank(&entries, query);
    let all: &'static [Command] = COMMANDS;
    let actions: Vec<SurfaceEntry> = rows.iter().filter_map(|r| all.get(r.index)).map(|cmd| row_entry(cmd, doc, selection, own)).collect();
    let selected = highlight.min(rows.len().saturating_sub(1));
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(((width - WIDTH) / 2.0).max(LEFT_WIDTH)), top: Val::Px(TOPBAR + 28.0), ..default() },
            FocusPolicy::Block,
            GlobalZIndex(POPUP_Z),
            AccessibleLabel::new("Command palette"),
            SurfaceRoot,
            crate::ui_kit::activation::ModalFocus,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|p| {
            let row = |i: usize| actions.get(i).cloned().unwrap_or(SurfaceEntry { action: CadAction::CadSurface { surface: Surface::Closed }, closes: false, refusal: None });
            // The list scrolls with the wheel as the menus do (`surfaces::popup_scroll`).
            k.palette(p, query, &placeholder(doc), "Command palette search", &rows, &entries, selected, PaletteField, row, PopupScroll);
        });
}

/// Input: the palette's search field while it is open (see the module doc).
pub(super) fn input(
    doc: Option<ResMut<CadDocument>>,
    // The field's messages are read first, then `TextFocus` acts (a `ParamSet`: one at a time).
    mut field: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    mut out: MessageWriter<Act<CadAction>>,
    mut was_open: Local<bool>,
    selection: CadSelection,
) {
    let events: Vec<FieldEvent> = field.p0().read().filter(|m| m.field == PALETTE).map(|m| m.event.clone()).collect();
    let open = doc.as_deref().and_then(|d| d.ops.surface.as_ref()).and_then(|o| match &o.surface {
        Surface::Palette { query } => Some((query.clone(), o.highlight.unwrap_or(0))),
        _ => None,
    });
    let mut text = field.p1();
    let (Some(mut doc), Some((query, highlight))) = (doc, open) else {
        // Closed (or replaced by another surface): the field gives the keyboard back.
        if std::mem::take(&mut *was_open) {
            text.blur(PALETTE);
        }
        return;
    };
    if !*was_open {
        // Opened: the search field takes the keyboard (keys pressed before,
        // its own Ctrl+Space or Shift+F, are not its text: the kit read them
        // before the field had it). Taking it ends any other field's entry.
        *was_open = true;
        text.focus(PALETTE, query);
        return;
    }
    let selection = selection.items();
    let own = own_controls(&doc, &selection);
    let (mut query_now, mut at) = (query.clone(), highlight);
    for event in events {
        match event {
            FieldEvent::Changed(draft) => {
                query_now = draft.text;
                at = 0;
            }
            FieldEvent::Submit(_) => {
                let rows = ranked(&doc, &selection, &own, &query_now);
                if let Some((cmd, _)) = rows.get(at.min(rows.len().saturating_sub(1))) {
                    let entry = row_entry(cmd, &doc, &selection, &own);
                    match entry.refusal {
                        // Stamped with the source it was ranked against, as a
                        // row's click is (`surfaces::input`): refused if the
                        // document is replaced before it applies.
                        None => {
                            out.write(Act::ui(crate::cad::activation::guard(&doc, entry.action)));
                            if entry.closes {
                                out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadSurface { surface: Surface::Closed })));
                            }
                        }
                        Some(why) => doc.show(Err(why)),
                    }
                }
                break;
            }
            // Escape closes it (the kit has taken the keyboard away); so
            // does losing the keyboard to another field or a mode switch.
            FieldEvent::Cancel => {
                out.write(Act::ui(CadAction::CadSurface { surface: Surface::Closed }));
                break;
            }
            FieldEvent::Blur if !text.ordinary_focused() => {
                out.write(Act::ui(CadAction::CadSurface { surface: Surface::Closed }));
                break;
            }
            FieldEvent::Tab { .. } => {}
            // ↑/↓ move the highlight (RoboCAD's list keys).
            FieldEvent::Blur => {}
            FieldEvent::Arrow { up: true } => at = at.saturating_sub(1),
            FieldEvent::Arrow { up: false } => at = (at + 1).min(ranked(&doc, &selection, &own, &query_now).len().saturating_sub(1)),
        }
    }
    // The query set from outside (`CadSurface { palette { query } }` while
    // open) is the field's draft too.
    if text.draft(PALETTE).is_some_and(|d| d.text != query_now) {
        text.set(PALETTE, TextDraft::new(query_now.clone(), false));
    }
    if (query_now != query || at != highlight)
        && let Some(o) = doc.ops.surface.as_mut()
        && matches!(o.surface, Surface::Palette { .. })
    {
        o.surface = Surface::Palette { query: query_now };
        o.highlight = Some(at);
    }
}
