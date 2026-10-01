//! The command bar under CAD mode's header, across the 3D view: RoboCAD's
//! menu bar (`menus::tabs`, one tab per category in RoboCAD's order) and
//! its tools toolbar (`registry::TOOLBAR`, app.py:442, in order). A hint
//! under the hovered button names its command, keys and, when it cannot
//! run, why (the kit has no tooltip widget; RoboCAD's toolbar tooltips
//! are the command labels).
//!
//! Each toolbar button is a kit chip carrying `CadButton(CadInvoke { id })`
//! (`panel::buttons` writes it, as every CAD button) with `Enabled` from
//! `registry::ready`; RoboCAD's checkable `tool.*` entries are lit while
//! that tool is the active one (`CadDocument::tool`) or that operation is
//! the active interaction or open form (`ops.active`, `ops.form`). Entries
//! owned by a later epic are disabled, the hint naming the epic.
//!
//! The bar has a fixed height ([`COMMAND_BAR`]); the toolbar row scrolls
//! sideways with the wheel when it is wider than the view (RoboCAD's Qt
//! toolbar folds its overflow behind a "»" button instead).
use super::menus::{self, MenuRow, Tabs};
use super::registry::{self, Command, Resolved, TOOLBAR};
use super::{SurfaceRoot, over_popup, rect_of, shortcut_keys};
use crate::app::ModeScope;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::{CadButton, own_controls};
use crate::ui_kit::{BAR, BORDER, Kit, LEFT_WIDTH, RIGHT_WIDTH, SURFACE, TEXT, TOPBAR, UiFonts, WHEEL_LINE, size, wheel_delta};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use bevy::ui::{ComputedNode, FocusPolicy, UiGlobalTransform};
use bevy::window::PrimaryWindow;

/// The command bar's height (px): the menu row and the toolbar row. Overlays
/// placed at the 3D view's top go below `TOPBAR + COMMAND_BAR`.
pub(crate) const COMMAND_BAR: f32 = 76.0;

/// The command bar's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct CommandBar;
/// The toolbar row (scrolls sideways).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct ToolRow;
/// The hint line's text.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct BarHint;
/// What the hint line says while this button is hovered.
#[derive(Component, Clone, Debug, Default)]
pub(super) struct Hint(pub String);

/// OnEnter(Cad): the bar with its empty rows; `refresh` fills them.
pub(super) fn spawn(mut commands: Commands, fonts: Res<UiFonts>) {
    let k = Kit::new(&fonts);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(LEFT_WIDTH),
                right: Val::Px(RIGHT_WIDTH),
                top: Val::Px(TOPBAR),
                height: Val::Px(COMMAND_BAR),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(BAR),
            BorderColor::all(BORDER),
            FocusPolicy::Block,
            AccessibleLabel::new("CAD menus and tools"),
            CommandBar,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|bar| {
            bar.spawn((Node { height: Val::Px(36.0), align_items: AlignItems::Center, overflow: Overflow::clip(), flex_shrink: 0.0, ..default() }, MenuRow)).with_children(|row| {
                row.spawn((Node { align_items: AlignItems::Center, column_gap: Val::Px(2.0), flex_shrink: 0.0, ..default() }, Tabs));
            });
            bar.spawn((Node { height: Val::Px(30.0), align_items: AlignItems::Center, column_gap: Val::Px(4.0), overflow: Overflow::scroll_x(), flex_shrink: 0.0, ..default() }, ScrollPosition::DEFAULT, ToolRow));
        });
    // The hint under the hovered bar button (hidden until one is hovered).
    commands
        .spawn((
            k.text("", size::CAPTION, TEXT, 0),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(LEFT_WIDTH),
                top: Val::Px(TOPBAR + COMMAND_BAR + 2.0),
                max_width: Val::Px(420.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER),
            FocusPolicy::Pass,
            Pickable::IGNORE,
            GlobalZIndex(super::POPUP_Z - 1),
            BarHint,
            DespawnOnExit(ModeScope::Cad),
        ));
}

/// Whether a checkable toolbar command is the active tool or interaction.
fn lit(doc: &CadDocument, cmd: &Command) -> bool {
    match registry::resolve(cmd) {
        Resolved::Action(CadAction::CadTool { tool }) => doc.tool == tool && doc.ops.active.is_none() && doc.ops.form.is_none(),
        Resolved::Op(entry) => doc.ops.active == Some(entry.id) || doc.ops.form.as_ref().is_some_and(|f| f.op == entry.id),
        _ => false,
    }
}

/// The hint for a command: "label (keys)", then why it cannot run.
pub(super) fn describe(cmd: &Command, ready: &Result<(), String>) -> String {
    let keys = shortcut_keys(cmd);
    let mut out = if keys.is_empty() { cmd.label.to_string() } else { format!("{} ({})", cmd.label, keys.join(", ")) };
    if let Err(why) = ready {
        out.push_str(" — ");
        out.push_str(&registry::status_line(cmd, why));
    }
    out
}

/// What the bar shows, as a comparable text.
fn key(doc: &CadDocument) -> String {
    format!("{:?}", (doc.generation, doc.revision, doc.tool, doc.ops.active, doc.ops.form.as_ref().map(|f| f.op), menus::open_menu(doc), &doc.selection, doc.edit.is_some(), doc.connected()))
}

/// Present: the menu tabs and the toolbar, rebuilt when what they show changes.
#[allow(clippy::type_complexity)]
pub(super) fn refresh(mut commands: Commands, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, tabs: Query<Entity, With<Tabs>>, tools: Query<Entity, With<ToolRow>>, mut last: Local<Option<(Entity, Entity, String)>>) {
    let (Some(doc), Ok(tabs), Ok(tools)) = (doc, tabs.single(), tools.single()) else { return };
    let stamp = (tabs, tools, key(&doc));
    if (*last).as_ref() == Some(&stamp) {
        return;
    }
    *last = Some(stamp);
    let k = Kit::new(&fonts);
    commands.entity(tabs).despawn_related::<Children>();
    commands.entity(tabs).with_children(|p| menus::tabs(p, &k, &doc));
    let own = own_controls(&doc);
    commands.entity(tools).despawn_related::<Children>();
    commands.entity(tools).with_children(|p| {
        for id in TOOLBAR {
            let Some(cmd) = registry::command(id) else { continue };
            let ready = registry::ready(cmd, &doc, &own);
            let on = id.starts_with("tool.") && lit(&doc, cmd);
            p.spawn(k.chip(cmd.label, CadButton(CadAction::CadInvoke { id: id.to_string() }), on, ready.is_ok())).insert(Hint(describe(cmd, &ready)));
        }
    });
}

/// Present: the hint follows the hovered bar button: shown under it,
/// hidden when none is hovered.
#[allow(clippy::type_complexity)]
pub(super) fn hint(changed: Query<(), (Changed<Interaction>, With<Hint>)>, all: Query<(&Interaction, &Hint, &ComputedNode, &UiGlobalTransform)>, mut tips: Query<(&mut Text, &mut Node), With<BarHint>>) {
    if changed.is_empty() {
        return;
    }
    let hovered = all.iter().find(|(i, ..)| **i != Interaction::None).map(|(_, h, node, t)| (h.0.clone(), rect_of(node, t)));
    for (mut text, mut node) in &mut tips {
        match &hovered {
            Some((line, rect)) => {
                if text.0 != *line {
                    text.0.clone_from(line);
                }
                node.display = Display::Flex;
                node.left = Val::Px(rect.min.x.max(LEFT_WIDTH));
            }
            None => node.display = Display::None,
        }
    }
}

/// Input: the wheel over the toolbar row scrolls it sideways (not under an open popup, whose wheel it is).
pub(super) fn scroll(mut wheel: MessageReader<MouseWheel>, windows: Query<&Window, With<PrimaryWindow>>, popups: Query<(&ComputedNode, &UiGlobalTransform), With<SurfaceRoot>>, mut rows: Query<(&mut ScrollPosition, &ComputedNode, &UiGlobalTransform), With<ToolRow>>) {
    let delta = wheel_delta(&mut wheel, WHEEL_LINE);
    if delta == 0.0 {
        return;
    }
    let Some(cursor) = windows.single().ok().and_then(Window::cursor_position) else { return };
    if over_popup(&popups, cursor) {
        return;
    }
    for (mut position, node, transform) in &mut rows {
        if rect_of(node, transform).contains(cursor) {
            position.0.x = (position.0.x - delta).max(0.0);
        }
    }
}
