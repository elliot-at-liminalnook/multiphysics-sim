//! The outliner's context menu (a kit popup at the pointer, RoboCAD's
//! `_context_menu`) and its "Organize components" dialog (the kit form on
//! a modal backdrop, RoboCAD's `QInputDialog.getText(self, "Organize
//! components", "Group name:")`). Drawn in Present from `TreeState`, rebuilt
//! when what they show changes; their presses are read in Input ([`input`]).
use super::controls::{MenuRow, menu_rows};
use super::input::DialogPart;
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::edit_blocked;
use crate::cad::selection::{CadItems, CadSelection};
use crate::ui_kit::form::{FieldKind, FormRow};
use crate::ui_kit::{BORDER, FAINT, Kit, LEFT_WIDTH, Look, SURFACE, SWITCHER_STRIP, TOPBAR, UiFonts, size};
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use bevy::ui::{ComputedNode, FocusPolicy, UiGlobalTransform};
use bevy::window::PrimaryWindow;

/// Above the docks and the switcher strip, under modal backdrops (the
/// command surfaces' popups use 44).
const MENU_Z: i32 = 44;
/// The menu's width and its distance from the window's edges (px).
const MENU_WIDTH: f32 = 240.0;
const EDGE: f32 = 8.0;
/// The dialog's width (px).
const DIALOG_WIDTH: f32 = 360.0;

/// The open menu's root.
#[derive(Component)]
pub(in crate::cad) struct TreeMenuRoot;
/// The open dialog's root (its backdrop).
#[derive(Component)]
pub(in crate::cad) struct TreeDialogRoot;

/// A menu entry: the action a press writes (the menu closes after it); a
/// disabled entry's press shows why.
#[derive(Component, Clone, Debug)]
pub(in crate::cad) struct MenuEntry {
    pub action: CadAction,
    pub refusal: Option<String>,
}

fn close() -> CadAction {
    let mut a = super::TreeArgs::of(super::TreeOp::Menu);
    a.open = Some(false);
    a.action()
}

/// Input: a menu entry's press (its action, then closing the menu), a
/// press outside the open menu (closes it, as a Qt menu closes), and
/// Escape (closes it; consumed so CAD's Escape does not also clear the
/// selection). Runs before the rows, so a right press on another row
/// closes this menu and then opens that row's.
#[allow(clippy::type_complexity)]
pub(super) fn input(
    doc: Option<ResMut<CadDocument>>,
    entries: Query<(&MenuEntry, Option<&crate::builder::ui_api::Enabled>), (With<crate::ui_kit::activation::Activated>, With<Button>)>,
    roots: Query<(&ComputedNode, &UiGlobalTransform), With<TreeMenuRoot>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<ResMut<ButtonInput<KeyCode>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    selection: CadSelection,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let Some(mut doc) = doc else { return };
    let Some(menu) = &doc.tree.menu else { return };
    // Its entries act on the nodes frozen when it opened, and its catalogue
    // entries on the selection: a selection change while it is open (a
    // poll adopting RoboCAD's, a 3D pick) closes it, as a modal Qt menu
    // allows none.
    if selection.items().nodes() != menu.ids {
        out.write(Act::ui(crate::cad::activation::guard(&doc, close())));
        return;
    }
    let mut clicked = false;
    for (entry, enabled) in &entries {
        clicked = true;
        if enabled.is_some_and(|e| !e.0) {
            if let Some(why) = &entry.refusal {
                doc.show(Err(why.clone()));
            }
            continue;
        }
        out.write(Act::ui(crate::cad::activation::guard(&doc, entry.action.clone())));
        out.write(Act::ui(crate::cad::activation::guard(&doc, close())));
    }
    if !clicked
        && buttons.is_some_and(|b| b.any_just_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle]))
        && let Some(cursor) = windows.single().ok().and_then(Window::cursor_position)
        && !roots.iter().any(|(node, t)| crate::cad::surfaces::rect_of(node, t).contains(cursor))
    {
        out.write(Act::ui(crate::cad::activation::guard(&doc, close())));
    }
    if let Some(mut keys) = keys
        && keys.just_pressed(KeyCode::Escape)
    {
        keys.clear_just_pressed(KeyCode::Escape);
        out.write(Act::ui(crate::cad::activation::guard(&doc, close())));
    }
}

/// What the open menu and dialog show (None: neither is open).
fn key(doc: &CadDocument, selection: &[sim_runtime::cad_client::SelectionItem], window: Vec2) -> Option<String> {
    if doc.tree.menu.is_none() && doc.tree.dialog.is_none() {
        return None;
    }
    let menu: Vec<(String, bool, bool)> = menu_rows(doc, selection)
        .into_iter()
        .map(|r| match r {
            MenuRow::Entry { label, ready, indent, .. } => (label, ready.is_ok(), indent),
            MenuRow::Heading(h) => (h, false, false),
            MenuRow::Separator => (String::new(), false, false),
        })
        .collect();
    Some(format!("{:?}", (doc.generation, doc.tree.form_sequence, &doc.tree.menu, menu, selection, &doc.tree.dialog, edit_blocked(doc), window.round())))
}

/// Present: the open menu and dialog, rebuilt when what they show changes,
/// despawned when they close.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    fonts: Res<UiFonts>,
    windows: Query<&Window, With<PrimaryWindow>>,
    menus: Query<Entity, With<TreeMenuRoot>>,
    dialogs: Query<Entity, With<TreeDialogRoot>>,
    selection: CadSelection,
    mut last: Local<Option<String>>,
) {
    let items = selection.items();
    let window = windows.single().map_or(Vec2::new(1280.0, 720.0), |w| Vec2::new(w.width(), w.height()));
    let key = doc.as_deref().and_then(|d| key(d, &items, window));
    let shown = menus.iter().next().is_some() || dialogs.iter().next().is_some();
    if key == *last && shown == key.is_some() {
        return;
    }
    *last = key;
    for root in menus.iter().chain(dialogs.iter()) {
        commands.entity(root).despawn();
    }
    let Some(doc) = doc.as_deref() else { return };
    let k = Kit::new(&fonts);
    if let Some(menu) = &doc.tree.menu {
        let at = menu.at.map_or(Vec2::new(LEFT_WIDTH - 40.0, TOPBAR + 40.0), Vec2::from);
        spawn_menu(&mut commands, &k, at, window, &menu_rows(doc, &items));
    }
    if let Some(dialog) = &doc.tree.dialog {
        let blocked = edit_blocked(doc);
        let ok = blocked.is_none() && !dialog.draft.trim().is_empty();
        let rows = [FormRow { label: "Group name:", kind: FieldKind::Text, text: &dialog.draft, focused: true, optional: false, selected: false, picks: &[] }];
        let what = if dialog.ids.is_empty() { "OK adds an empty group.".to_string() } else { format!("OK groups the {} selected nodes into a new group (one undo step).", dialog.ids.len()) };
        commands.spawn((k.backdrop("Organize components", true), TreeDialogRoot, DespawnOnExit(ModeScope::Cad))).with_children(|backdrop| {
            k.form(backdrop, "Organize components", &rows, ok, Some(DIALOG_WIDTH), DialogPart, |p| {
                if let Some(e) = &dialog.error {
                    p.spawn(k.text(e.clone(), size::SMALL, crate::ui_kit::DANGER, 0));
                }
                if let Some(why) = &blocked {
                    p.spawn(k.note(format!("OK is unavailable: {why}.")));
                }
                p.spawn(k.caption(what));
            });
        });
    }
}

/// The menu at `at`, kept inside the window: one ghost button per entry
/// (disabled ones greyed), headings and rules as display only.
fn spawn_menu(commands: &mut Commands, k: &Kit, at: Vec2, window: Vec2, rows: &[MenuRow]) {
    let left = at.x.min(window.x - EDGE - MENU_WIDTH).max(EDGE);
    let bottom = window.y - SWITCHER_STRIP - EDGE;
    let top = at.y.min(bottom - 160.0).max(EDGE);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(top),
                width: Val::Px(MENU_WIDTH),
                max_height: Val::Px((bottom - top).max(160.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                padding: UiRect::all(Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER),
            FocusPolicy::Block,
            GlobalZIndex(MENU_Z),
            AccessibleLabel::new("Model tree context menu"),
            TreeMenuRoot,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|p| {
            for row in rows {
                match row {
                    MenuRow::Entry { label, action, ready, indent } => {
                        let entry = MenuEntry { action: action.clone(), refusal: ready.as_ref().err().cloned() };
                        let pad = if *indent { 20.0 } else { 0.0 };
                        p.spawn(k.button(label, entry, Look::Ghost, ready.is_ok())).insert(Node { justify_content: JustifyContent::FlexStart, align_items: AlignItems::Center, margin: UiRect::left(Val::Px(pad)), flex_shrink: 0.0, ..default() });
                    }
                    MenuRow::Heading(text) => {
                        p.spawn((k.text(text.clone(), size::CAPTION, FAINT, 0), Node { padding: UiRect::new(Val::Px(11.0), Val::Px(8.0), Val::Px(4.0), Val::Px(2.0)), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
                    }
                    MenuRow::Separator => {
                        p.spawn((Node { height: Val::Px(1.0), margin: UiRect::vertical(Val::Px(3.0)), flex_shrink: 0.0, ..default() }, BackgroundColor(BORDER), Pickable::IGNORE));
                    }
                }
            }
        });
}
