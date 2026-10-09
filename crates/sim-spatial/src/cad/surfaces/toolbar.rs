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
//! the active interaction or open form (`ops.active`, `ops.form`). A button
//! that cannot run now is disabled with the hint saying why (no edit can be
//! sent, the selection does not fit); all 25 run natively now (catalogue
//! operations or CAD actions: Annotate, References, Pose and Experiments
//! since cad-organize and cad-experiments-motion, Fastener and Validate
//! since cad-print), so none is disabled for belonging to a later epic.
//!
//! Rectangle, Circle, Slot and Extrude (`sketch.rectangle`,
//! `sketch.circle`, `sketch.slot`, `tool.extrude`) are catalogue
//! operations (cad-sketch): `registry::resolve` finds them in the
//! catalogue, so they are enabled as any interaction is (`registry::ready`:
//! connected, no edit in flight). The three sketch buttons are lit while
//! their sketch tool is active ([`checkable`]): a deliberate difference,
//! as RoboCAD's toolbar makes only `tool.*` checkable (app.py:445-446), so its
//! Rectangle, Circle and Slot never show which sketch tool is drawing.
//!
//! The bar has a fixed height ([`COMMAND_BAR`]); the menu row and the
//! toolbar row each scroll sideways with the wheel when wider than the view
//! (RoboCAD's Qt toolbar folds its overflow behind a "»" button instead).
//!
//! The menu tabs are spawned once and lit in place (their `Look` and
//! action), so the open menu's popup (`surfaces::draw`, later in the same
//! chain) is placed under a tab that has been laid out.
use super::menus::{self, MenuRow, MenuTab, Tabs};
use super::registry::{self, Command, Resolved, TOOLBAR};
use super::{SurfaceRoot, over_popup, rect_of, shortcut_keys};
use crate::app::ModeScope;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::{CadButton, own_controls};
use crate::cad::selection::CadSelection;
use crate::cad::types::SelectionItem;
use crate::ui_kit::{BAR, BORDER, Kit, LEFT_WIDTH, Look, RIGHT_WIDTH, SURFACE, TEXT, TOPBAR, UiFonts, WHEEL_LINE, size, wheel_delta};
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
            bar.spawn((Node { height: Val::Px(36.0), align_items: AlignItems::Center, overflow: Overflow::scroll_x(), flex_shrink: 0.0, ..default() }, ScrollPosition::DEFAULT, MenuRow)).with_children(|row| {
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

/// Whether a toolbar button is checkable: RoboCAD's `tool.*`, and the
/// sketch tools (see the module doc).
fn checkable(id: &str) -> bool {
    id.starts_with("tool.") || id.starts_with("sketch.")
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

/// What the toolbar shows, as a comparable text (the open menu is not in
/// it: the tabs are lit in place).
fn key(doc: &CadDocument, selection: &[SelectionItem]) -> String {
    format!("{:?}", (doc.generation, doc.revision, doc.tool, doc.ops.active, doc.ops.form.as_ref().map(|f| f.op), selection, doc.edit.is_some(), doc.connected()))
}

/// Present: the menu tabs (spawned once, then the open menu's tab lit in
/// place) and the toolbar, rebuilt when what it shows changes.
#[allow(clippy::type_complexity)]
pub(super) fn refresh(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    fonts: Res<UiFonts>,
    tabs: Query<(Entity, Option<&Children>), With<Tabs>>,
    mut menu_tabs: Query<(&MenuTab, &mut Look, &mut CadButton)>,
    tools: Query<Entity, With<ToolRow>>,
    mut last: Local<Option<(Entity, String)>>,
    mut tabs_source: Local<Option<String>>,
    selection: CadSelection,
) {
    let (Some(doc), Ok((tabs, spawned)), Ok(tools)) = (doc, tabs.single(), tools.single()) else { return };
    let selection = selection.items();
    let k = Kit::new(&fonts);
    let source = crate::cad::activation::render_key(&doc);
    if spawned.is_none_or(|c| c.is_empty()) || tabs_source.as_ref() != Some(&source) {
        *tabs_source = Some(source);
        commands.entity(tabs).despawn_related::<Children>();
        commands.entity(tabs).with_children(|p| menus::tabs(p, &k, &doc));
    } else {
        menus::light(&doc, &mut menu_tabs);
    }
    let stamp = (tools, format!("{}|source={}", key(&doc, &selection), crate::cad::activation::render_key(&doc)));
    if (*last).as_ref() == Some(&stamp) {
        return;
    }
    *last = Some(stamp);
    let own = own_controls(&doc, &selection);
    commands.entity(tools).despawn_related::<Children>();
    commands.entity(tools).with_children(|p| {
        for id in TOOLBAR {
            let Some(cmd) = registry::command(id) else { continue };
            let ready = registry::ready(cmd, &doc, &selection, &own);
            let on = checkable(id) && lit(&doc, cmd);
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

/// Input: the wheel over the menu row or the toolbar row scrolls it
/// sideways (not under an open popup, whose wheel it is).
#[allow(clippy::type_complexity)]
pub(super) fn scroll(
    mut wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    popups: Query<(&ComputedNode, &UiGlobalTransform), With<SurfaceRoot>>,
    mut rows: Query<(&mut ScrollPosition, &ComputedNode, &UiGlobalTransform), Or<(With<ToolRow>, With<MenuRow>)>>,
) {
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
