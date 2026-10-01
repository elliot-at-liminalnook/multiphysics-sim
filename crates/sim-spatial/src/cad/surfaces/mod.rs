//! CAD mode's command surfaces (cad-modify), all built from the op
//! catalogue (`super::ops::CATALOGUE`) and RoboCAD's command table
//! (`registry`) on the kit's widgets: the menu bar and tools toolbar
//! (`toolbar`, `menus`), the viewport right-click menu (`context_menu`),
//! the Space view radial and the Q selection-mode radial (`radial`), the
//! command palette (`palette`), the menus by category (`menus`) and the
//! parameter form (`form`). Every entry carries the same `CadAction` REST and
//! `system_ui` send (`CadInvoke`, the existing CAD actions); opening and
//! closing a surface is `CadSurface`.
//!
//! - **State.** The open surface is `CadDocument::ops.surface` ([`Open`]),
//!   set and cleared only by [`handle`] (`CadSurface`), except the
//!   palette's query and the highlighted or hovered entry, which are
//!   display state its own input systems edit (as the numeric bar edits
//!   its drafts).
//! - **Entries** ([`entries`]): one list per surface, with each entry's
//!   command id, label, keys, readiness and action; the drawing, the clicks,
//!   [`handle`]'s REST answer and `system_ui` read the same list.
//! - **Input** (ViewerSet::Input, after `panel::name_entry`, before
//!   `numeric::entry` and so before the CAD keys and transform's keys): the
//!   form, then the palette, then [`input`] (entry clicks, presses outside,
//!   Escape), the radials, the context menu's right-click, the two-step
//!   key gate (`keys::gate`). A click on an entry writes its action, then
//!   `CadSurface { closed }` (unless the entry opens another surface), so the
//!   action is applied before the surface closes.
//! - **Escape order**: an open surface closes; else an open form or active
//!   interaction is cancelled (`CadFormCancel`); else transform's keys
//!   take it (they skip Escape while either exists).
//! - **Present**: [`draw`] rebuilds the open popup (palette, menu, context
//!   menu, radial) when what it shows changes; `toolbar` and `form` rebuild
//!   theirs. Every root carries `DespawnOnExit(ModeScope::Cad)`.
use serde::{Deserialize, Serialize};

pub(super) mod registry;
mod context_menu;
mod form;
mod menus;
mod palette;
mod radial;
mod toolbar;
#[cfg(test)]
mod tests;

pub(crate) use toolbar::COMMAND_BAR;

/// A command surface to open, or none (`CadSurface { surface }`).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Surface {
    /// Close whatever is open.
    Closed,
    /// The command palette, searching `query`.
    Palette {
        #[serde(default)]
        query: String,
    },
    /// The menu of one RoboCAD category ("Modify").
    Menu { category: String },
    /// The viewport right-click menu at `at` (window logical px; the cursor when absent).
    Context {
        #[serde(default)]
        at: Option<[f32; 2]>,
    },
    /// The Space view radial at `at`.
    ViewRadial {
        #[serde(default)]
        at: Option<[f32; 2]>,
    },
    /// The Q selection-mode radial at `at`.
    SelectRadial {
        #[serde(default)]
        at: Option<[f32; 2]>,
    },
}

impl Surface {
    /// The surface's name in `cad:surface:<kind>` and REST (`kind`).
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Surface::Closed => "closed",
            Surface::Palette { .. } => "palette",
            Surface::Menu { .. } => "menu",
            Surface::Context { .. } => "context",
            Surface::ViewRadial { .. } => "view_radial",
            Surface::SelectRadial { .. } => "select_radial",
        }
    }
}

/// The surface open now (`CadDocument::ops.surface`): what is shown and
/// where, the palette's query and highlighted row, the radial's hovered entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    pub surface: Surface,
    /// Where it opened (window logical px).
    pub at: [f32; 2],
    /// The palette's highlighted row, or a radial's hovered entry.
    pub highlight: Option<usize>,
}

use self::registry::{CATEGORIES, Command, Resolved};
use super::actions::{CadAction, Cx};
use super::document::CadDocument;
use super::panel::{Control, NameDraft};
use super::view::CadView;
use crate::app::actions::{Act, Call};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::builder::ui_api::Enabled;
use crate::ui_kit::{BORDER, FAINT, Kit, LEFT_WIDTH, Look, SURFACE, TOPBAR, UiFonts, size};
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use bevy::ui::{ComputedNode, FocusPolicy, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use serde_json::{Value, json};
use sim_api::Outcome;

/// Popups above every panel and the command bar (the kit's pie is 45, the
/// mode switcher 40).
const POPUP_Z: i32 = 44;

/// One entry of a surface: the RoboCAD command it runs, its label (with
/// its keys where RoboCAD shows them), whether it can run now, and the
/// action a click writes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub id: String,
    pub label: String,
    pub keys: Vec<String>,
    pub ready: Result<(), String>,
    pub action: CadAction,
    /// Running it closes the surface (false when it opens another one).
    pub closes: bool,
}

impl Entry {
    fn of(cmd: &Command, label: &str, keys: Vec<String>, doc: &CadDocument, own: &[Control]) -> Entry {
        Entry {
            id: cmd.id.to_string(),
            label: label.to_string(),
            keys,
            ready: registry::ready(cmd, doc, own),
            action: CadAction::CadInvoke { id: cmd.id.to_string() },
            closes: !matches!(registry::resolve(cmd), Resolved::Surface(_)),
        }
    }

    /// The label with its keys ("Fillet    Ctrl+F"), as a menu row shows it.
    fn text(&self) -> String {
        if self.keys.is_empty() { self.label.clone() } else { format!("{}    {}", self.label, self.keys.join(", ")) }
    }

    fn json(&self) -> Value {
        json!({"id": self.id, "label": self.label, "keys": self.keys, "enabled": self.ready.is_ok(), "disabled_reason": self.ready.as_ref().err(), "action": super::actions::rest_form(&self.action)})
    }
}

/// The keys RoboCAD's menus show for a command: its Qt shortcuts (keymap
/// keys only; inline keys are not bound).
fn shortcut_keys(cmd: &Command) -> Vec<String> {
    if cmd.bound { cmd.keys.iter().map(|k| k.to_string()).collect() } else { Vec::new() }
}

/// A surface's entries now (`own`: the panel's own controls, for the
/// readiness of action commands). The palette's are its ranked rows.
pub(crate) fn entries(surface: &Surface, doc: &CadDocument, own: &[Control]) -> Vec<Entry> {
    let by_id = |id: &str, label: Option<&str>| registry::command(id).map(|cmd| Entry::of(cmd, label.unwrap_or(cmd.label), shortcut_keys(cmd), doc, own));
    match surface {
        Surface::Closed => Vec::new(),
        Surface::Palette { query } => palette::ranked(doc, own, query).into_iter().map(|(cmd, row)| Entry::of(cmd, &row, Vec::new(), doc, own)).collect(),
        Surface::Menu { category } => registry::COMMANDS.iter().filter(|c| registry::menu_of(c.category) == category).map(|cmd| Entry::of(cmd, cmd.label, shortcut_keys(cmd), doc, own)).collect(),
        Surface::Context { .. } => {
            let mut out: Vec<Entry> = registry::CONTEXT.iter().filter_map(|id| by_id(id, None)).collect();
            if context_menu::instance_selected(doc) {
                let (id, label) = registry::MAKE_UNIQUE;
                out.extend(by_id(id, Some(label)));
            }
            out
        }
        Surface::ViewRadial { .. } => registry::VIEW_RADIAL.iter().filter_map(|(label, id)| by_id(id, Some(*label))).collect(),
        Surface::SelectRadial { .. } => registry::SELECT_RADIAL.iter().filter_map(|(label, id)| by_id(id, Some(*label))).collect(),
    }
}

/// The 3D view's centre (window logical px), where a surface opened
/// without a position goes.
fn view_centre(view: Option<&CadView>) -> [f32; 2] {
    match view.filter(|v| v.valid) {
        Some(v) => (v.min + v.size * 0.5).to_array(),
        None => [LEFT_WIDTH + 240.0, TOPBAR + 200.0],
    }
}

/// `CadSurface`: open or close a command surface (from `actions::handle`).
/// Answers the surface's entries (ids, labels, keys, enabled and why not,
/// actions), so REST sees what a click would.
pub(super) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let _ = call;
    let CadAction::CadSurface { surface } = action else { return Outcome::Done(Err("not a command-surface action".into())) };
    let doc = &mut *cx.doc;
    if let Surface::Menu { category } = surface
        && !CATEGORIES.contains(&category.as_str())
    {
        return Outcome::Done(Err(format!("RoboCAD has no menu {category:?}; its menus: {}", CATEGORIES.join(", "))));
    }
    if *surface == Surface::Closed {
        let closed = doc.ops.surface.take().map(|o| o.surface.kind());
        return Outcome::Done(Ok(json!({"closed": closed})));
    }
    let centre = view_centre(cx.view);
    let at = match surface {
        Surface::Context { at } | Surface::ViewRadial { at } | Surface::SelectRadial { at } => at.unwrap_or(centre),
        _ => centre,
    };
    let highlight = matches!(surface, Surface::Palette { .. }).then_some(0);
    doc.ops.surface = Some(Open { surface: surface.clone(), at, highlight });
    let own = super::panel::own_controls(doc);
    let list: Vec<Value> = entries(surface, doc, &own).iter().map(Entry::json).collect();
    Outcome::Done(Ok(json!({"surface": surface, "at": at, "entries": list})))
}

/// `CadInvoke` of a RoboCAD command id that is not a catalogue operation
/// (`registry`): its native CAD action (undo, fit, a selection mode, …),
/// RoboCAD's `POST /commands/{id}` when its desktop window serves the
/// document, or a refusal naming the epic that owns it or "GUI-only".
pub(super) fn invoke_command(id: &str, call: &mut Call, cx: &mut Cx) -> Outcome {
    registry::invoke(id, call, cx)
}

/// The surfaces' `system_ui` controls (`own`: the panel's own controls):
/// `cad:op:<id>` for every RoboCAD command (its label, `CadInvoke`, its
/// readiness), `cad:surface:<kind>`, `cad:menu:<category>`, and while a form
/// is open `cad:form:ok`, `cad:form:cancel` and `cad:form:set:<name>:<value>`
/// for each choice option and checkbox state.
pub(crate) fn controls(doc: &CadDocument, own: &[Control]) -> Vec<Control> {
    let mut out = Vec::new();
    for cmd in registry::COMMANDS {
        out.push(Control { id: format!("cad:op:{}", cmd.id), label: cmd.label.to_string(), action: CadAction::CadInvoke { id: cmd.id.to_string() }, ready: registry::ready(cmd, doc, own) });
    }
    let open = doc.ops.surface.is_some();
    for (kind, label, surface) in [
        ("palette", "Command palette", Surface::Palette { query: String::new() }),
        ("view_radial", "View radial menu", Surface::ViewRadial { at: None }),
        ("select_radial", "Selection-mode radial menu", Surface::SelectRadial { at: None }),
        ("context", "Viewport context menu", Surface::Context { at: None }),
        ("closed", "Close the open command surface", Surface::Closed),
    ] {
        let ready = if surface == Surface::Closed && !open { Err("no command surface is open".to_string()) } else { Ok(()) };
        out.push(Control { id: format!("cad:surface:{kind}"), label: label.to_string(), action: CadAction::CadSurface { surface }, ready });
    }
    for category in CATEGORIES {
        out.push(Control { id: format!("cad:menu:{category}"), label: format!("{category} menu"), action: CadAction::CadSurface { surface: Surface::Menu { category: category.to_string() } }, ready: Ok(()) });
    }
    out.extend(form::controls(doc));
    out
}

// ---- Components ------------------------------------------------------------------

/// A popup surface's root (palette, menu, context menu, radial).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SurfaceRoot;

/// A clickable entry of a popup: its action, and whether running it
/// closes the surface; a disabled entry's press shows why.
#[derive(Component, Clone, Debug)]
pub(crate) struct SurfaceEntry {
    pub action: CadAction,
    pub closes: bool,
    pub refusal: Option<String>,
}

impl SurfaceEntry {
    fn of(entry: &Entry) -> SurfaceEntry {
        let refusal = entry.ready.as_ref().err().map(|why| registry::command(&entry.id).map_or_else(|| why.clone(), |cmd| registry::status_line(cmd, why)));
        SurfaceEntry { action: entry.action.clone(), closes: entry.closes, refusal }
    }
}

/// Window logical rectangle of a laid-out node.
pub(crate) fn rect_of(node: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    let scale = node.inverse_scale_factor();
    Rect::from_center_size(transform.translation * scale, node.size() * scale)
}

/// Whether the name field or the numeric bar has the keyboard (their
/// Enter and Escape are theirs).
pub(super) fn other_field_focused(doc: &CadDocument, draft: Option<&NameDraft>) -> bool {
    doc.tool_state.numeric.focus.is_some() || draft.is_some_and(|d| d.editing.is_some())
}

// ---- Systems ---------------------------------------------------------------------

/// CAD mode's surfaces' systems (Input keys, Present drawing).
pub(super) fn build(app: &mut App) {
    app.init_resource::<super::keys::Chord>()
        .add_systems(OnEnter(ModeScope::Cad), toolbar::spawn)
        .add_systems(
            Update,
            (form::input, palette::input, input, radial::input, context_menu::input, toolbar::scroll, super::keys::gate)
                .chain()
                .after(crate::app::actions::serve)
                .after(super::panel::name_entry)
                // An open inspector editor's Escape and typing are its own.
                .after(super::inspector::editor_entry)
                .before(super::numeric::entry)
                .in_set(ViewerSet::Input)
                .run_if(in_state(ViewerMode::Cad)),
        )
        .add_systems(Update, (toolbar::refresh, toolbar::hint, draw, form::draw).chain().in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

/// Input: a click on a popup's entry (its action, then closing), a press
/// outside the open popup (closes it, as a Qt popup closes; a press on the
/// menu bar is the menu bar's), and Escape (the order in the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn input(
    doc: Option<ResMut<CadDocument>>,
    clicks: Query<(&Interaction, &SurfaceEntry, Option<&Enabled>), (Changed<Interaction>, With<Button>)>,
    roots: Query<(&ComputedNode, &UiGlobalTransform), With<SurfaceRoot>>,
    bar: Query<(&ComputedNode, &UiGlobalTransform), With<menus::MenuRow>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    draft: Option<Res<NameDraft>>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let Some(mut doc) = doc else { return };
    let mut clicked = false;
    for (interaction, entry, enabled) in &clicks {
        if *interaction != Interaction::Pressed {
            continue;
        }
        clicked = true;
        if enabled.is_some_and(|e| !e.0) {
            if let Some(why) = &entry.refusal {
                doc.show(Err(why.clone()));
            }
            continue;
        }
        out.write(Act::ui(entry.action.clone()));
        if entry.closes {
            out.write(Act::ui(CadAction::CadSurface { surface: Surface::Closed }));
        }
    }
    let open = doc.ops.surface.as_ref().map(|o| o.surface.clone());
    let radial = matches!(open, Some(Surface::ViewRadial { .. } | Surface::SelectRadial { .. }));
    // A press outside the popup closes it (the radials read their own presses).
    if let (Some(_), false, false, Some(buttons)) = (&open, radial, clicked, buttons.as_ref())
        && buttons.any_just_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
        && let Some(cursor) = windows.single().ok().and_then(Window::cursor_position)
    {
        let inside = roots.iter().chain(bar.iter()).any(|(node, t)| rect_of(node, t).contains(cursor));
        if !inside {
            out.write(Act::ui(CadAction::CadSurface { surface: Surface::Closed }));
        }
    }
    if keys.is_some_and(|k| k.just_pressed(KeyCode::Escape)) {
        if open.is_some() {
            out.write(Act::ui(CadAction::CadSurface { surface: Surface::Closed }));
        } else if (doc.ops.form.is_some() || doc.ops.active.is_some()) && !other_field_focused(&doc, draft.as_deref()) {
            out.write(Act::ui(CadAction::CadFormCancel));
        }
    }
}

/// What the open popup shows, as a comparable text.
fn popup_key(doc: &CadDocument, width: f32) -> Option<String> {
    let open = doc.ops.surface.as_ref()?;
    Some(format!("{:?}", (doc.generation, doc.revision, open, &doc.selection, doc.tool, doc.ops.active, doc.ops.form.as_ref().map(|f| f.op), doc.edit.is_some(), width.round())))
}

/// Present: the open popup (palette, menu, context menu or radial),
/// rebuilt when what it shows changes; despawned when it closes.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    fonts: Res<UiFonts>,
    windows: Query<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<SurfaceRoot>>,
    tabs: Query<(&menus::MenuTab, &ComputedNode, &UiGlobalTransform)>,
    mut last: Local<Option<String>>,
) {
    let width = windows.single().map_or(1280.0, Window::width);
    let key = doc.as_deref().and_then(|d| popup_key(d, width));
    let shown = roots.iter().next().is_some();
    if key == *last && shown == key.is_some() {
        return;
    }
    *last = key;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some(doc) = doc.as_deref() else { return };
    let Some(open) = doc.ops.surface.clone() else { return };
    let k = Kit::new(&fonts);
    let own = super::panel::own_controls(doc);
    let list = entries(&open.surface, doc, &own);
    let at = Vec2::from(open.at);
    match &open.surface {
        Surface::Closed => {}
        Surface::Palette { query } => palette::spawn(&mut commands, &k, doc, &own, query, open.highlight.unwrap_or(0), width),
        Surface::Menu { category } => {
            // Under its tab in the menu bar (laid out by now), else where it opened.
            let under = tabs.iter().find(|(tab, ..)| tab.0 == category.as_str()).map(|(_, node, t)| {
                let r = rect_of(node, t);
                Vec2::new(r.min.x, r.max.y + 2.0)
            });
            popup_list(&mut commands, &k, &format!("{category} menu"), under.unwrap_or(at), &list);
        }
        Surface::Context { .. } => popup_list(&mut commands, &k, "Viewport context menu", at + Vec2::splat(2.0), &list),
        Surface::ViewRadial { .. } | Surface::SelectRadial { .. } => radial::spawn(&mut commands, &k, &open, &list),
    }
}

/// A menu-like popup at `at`: one ghost row per entry ("label    keys"),
/// disabled ones greyed, each a [`SurfaceEntry`].
fn popup_list(commands: &mut Commands, k: &Kit, label: &str, at: Vec2, list: &[Entry]) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(at.x),
                top: Val::Px(at.y),
                min_width: Val::Px(220.0),
                max_height: Val::Px(560.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(1.0),
                padding: UiRect::all(Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER),
            FocusPolicy::Block,
            GlobalZIndex(POPUP_Z),
            AccessibleLabel::new(label),
            SurfaceRoot,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|p| {
            if list.is_empty() {
                p.spawn(k.text("(no commands)", size::CAPTION, FAINT, 0));
            }
            for e in list {
                // Left-aligned rows: the kit button's layout with its content at the start
                // (`repaint_buttons` restores the look's padding, border and radius).
                p.spawn(k.button(&e.text(), SurfaceEntry::of(e), Look::Ghost, e.ready.is_ok())).insert(Node { justify_content: JustifyContent::FlexStart, align_items: AlignItems::Center, flex_shrink: 0.0, ..default() });
            }
        });
}
