//! CAD mode's command surfaces (cad-modify), all built from the op
//! catalogue (`super::ops::CATALOGUE`) and RoboCAD's command table
//! (`registry`) on the kit's widgets: the menu bar and tools toolbar
//! (`toolbar`, `menus`), the viewport right-click menu (`context_menu`),
//! the Space view radial and the Q selection-mode radial (`radial`), the
//! command palette (`palette`), the menus by category (`menus`) and the
//! parameter form (`form`). Every entry carries the same `CadAction` REST and
//! `system_ui` send (`CadInvoke`, the existing CAD actions); opening and
//! closing a surface is `CadSurface`. The cad-sketch epic's commands
//! (Create solids, Planes, Sketch) are catalogue operations too, so every
//! surface reaches them as it reaches cad-modify's; the right-click menu
//! adds a Sketch section (`context_menu`).
//!
//! - **State.** The open surface is `CadDocument::ops.surface` ([`Open`]),
//!   set and cleared only by [`handle`] (`CadSurface`), except the
//!   palette's query and the highlighted or hovered entry, which are
//!   display state its own input systems edit (as the numeric bar edits
//!   its drafts).
//! - **Entries** ([`entries`]): one list per surface, with each entry's
//!   command id, label, keys, readiness and action; the drawing, the clicks,
//!   [`handle`]'s REST answer and `system_ui` read the same list.
//! - **Input** (ViewerSet::Input, before `CadKeySet::NumericEntry` (`numeric::entry`), the CAD keys
//!   and transform's keys): the form, then the palette, then [`input`]
//!   (entry clicks, presses outside, Escape), the radials, the context
//!   menu's right-click, the two-step key gate (`keys::gate`). A click on an entry writes its action, then
//!   `CadSurface { closed }` (unless the entry opens another surface), so the
//!   action is applied before the surface closes.
//! - **Escape order**: a typing kit field's Escape is that field's alone
//!   (the kit consumes it; the palette's closes the palette, the form's
//!   cancels the form); else an open file form's, results path form's or
//!   outliner menu's (`files::form`, `results::forms`, `tree::popup`:
//!   [`input`] stands aside while one is open); else an open surface
//!   closes; else an open form or active interaction is cancelled
//!   (`CadFormCancel`), and [`input`] consumes the key when it did either
//!   (a pending two-step key is dropped too), so calibrate's, the threads'
//!   and the Select tool's Escape (`CadKeySet::EscapeTool`, `Escape`,
//!   `ToolKeys`, after the gate) never also act on that press; else those
//!   take it.
//! - **Wheel**: over a menu or context-menu popup it scrolls the popup's
//!   rows ([`popup_scroll`]); the menus outgrow their 560 px.
//! - **Present**: [`draw`] rebuilds the open popup (palette, menu, context
//!   menu, radial) when what it shows changes; `toolbar` and `form` rebuild
//!   theirs. Every root carries `DespawnOnExit(ModeScope::Cad)`.
use serde::{Deserialize, Serialize};

pub(super) mod registry;
mod context_menu;
pub(in crate::cad) mod form;
mod menus;
mod palette;
mod radial;
mod toolbar;
#[cfg(test)]
mod tests;

pub(crate) use toolbar::COMMAND_BAR;
pub(super) use form::FORM;

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
    /// The viewport right-click menu at `at` (window logical px; the 3D
    /// view's centre when absent: a right-click passes the cursor).
    Context {
        #[serde(default)]
        at: Option<[f32; 2]>,
    },
    /// The Space view radial at `at` (the 3D view's centre when absent).
    ViewRadial {
        #[serde(default)]
        at: Option<[f32; 2]>,
    },
    /// The Q selection-mode radial at `at` (the 3D view's centre when absent).
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
use super::panel::Control;
use super::selection::CadSelection;
use sim_runtime::cad_client::SelectionItem;
use super::view::CadView;
use crate::app::actions::{Act, Call};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::builder::ui_api::Enabled;
use crate::ui_kit::{BORDER, FAINT, Kit, LEFT_WIDTH, Look, SURFACE, SWITCHER_STRIP, TOPBAR, UiFonts, WHEEL_LINE, size, wheel_delta};
use bevy::input::mouse::MouseWheel;
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
    fn of(cmd: &Command, label: &str, keys: Vec<String>, doc: &CadDocument, selection: &[SelectionItem], own: &[Control]) -> Entry {
        Entry {
            id: cmd.id.to_string(),
            label: label.to_string(),
            keys,
            ready: registry::ready(cmd, doc, selection, own),
            action: CadAction::CadInvoke { id: cmd.id.to_string() },
            closes: !matches!(registry::resolve(cmd), Resolved::Surface(_)),
        }
    }

    /// The label with its keys ("Fillet    Ctrl+F"), as a menu row shows it.
    fn text(&self) -> String {
        if self.keys.is_empty() { self.label.clone() } else { format!("{}    {}", self.label, self.keys.join(", ")) }
    }

    fn json(&self) -> Value {
        json!({"id": self.id, "label": self.label, "keys": self.keys, "enabled": self.ready.is_ok(), "disabled_reason": self.ready.as_ref().err(), "action": super::rest_form::rest_form(&self.action)})
    }
}

/// The keys RoboCAD's menus show for a command: its Qt shortcuts (keymap
/// keys only; inline keys are not bound).
fn shortcut_keys(cmd: &Command) -> Vec<String> {
    if cmd.bound { cmd.keys.iter().map(|k| k.to_string()).collect() } else { Vec::new() }
}

/// A surface's entries now (`selection`: the shared selection's CAD items;
/// `own`: the panel's own controls, for the readiness of action commands).
/// The palette's are its ranked rows.
pub(crate) fn entries(surface: &Surface, doc: &CadDocument, selection: &[SelectionItem], own: &[Control]) -> Vec<Entry> {
    let by_id = |id: &str, label: Option<&str>| registry::command(id).map(|cmd| Entry::of(cmd, label.unwrap_or(cmd.label), shortcut_keys(cmd), doc, selection, own));
    match surface {
        Surface::Closed => Vec::new(),
        Surface::Palette { query } => palette::ranked(doc, selection, own, query).into_iter().map(|(cmd, row)| Entry::of(cmd, &row, Vec::new(), doc, selection, own)).collect(),
        Surface::Menu { category } => registry::COMMANDS.iter().filter(|c| registry::menu_of(c.category) == category).map(|cmd| Entry::of(cmd, cmd.label, shortcut_keys(cmd), doc, selection, own)).collect(),
        Surface::Context { .. } => {
            // RoboCAD's 14, then the native Sketch section (`context_menu`'s doc).
            let mut out: Vec<Entry> = registry::CONTEXT.iter().chain(registry::SKETCH_CONTEXT.iter()).filter_map(|id| by_id(id, None)).collect();
            if context_menu::instance_selected(doc, selection) {
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
    let selection = cx.shared.items();
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
    // The palette's search field takes the keyboard when it opens
    // (`palette::input`), which ends any other field's entry.
    doc.ops.surface = Some(Open { surface: surface.clone(), at, highlight });
    let own = super::panel::own_controls(doc, &selection);
    let list: Vec<Value> = entries(surface, doc, &selection, &own).iter().map(Entry::json).collect();
    Outcome::Done(Ok(json!({"surface": surface, "at": at, "entries": list})))
}

/// `CadInvoke` of a RoboCAD command id that is not a catalogue operation
/// (`registry`): its native CAD action (undo, fit, a selection mode, …),
/// or a refusal naming the epic that owns it or why it is not ported.
pub(super) fn invoke_command(id: &str, call: &mut Call, cx: &mut Cx) -> Outcome {
    registry::invoke(id, call, cx)
}

/// The surfaces' `system_ui` controls (`own`: the panel's own controls):
/// `cad:op:<id>` for every RoboCAD command (its label, `CadInvoke`, its
/// readiness), `cad:surface:<kind>`, `cad:menu:<category>`, and while a form
/// is open `cad:form:ok`, `cad:form:cancel` and `cad:form:set:<name>:<value>`
/// for each choice option and checkbox state (`selection`: the shared
/// selection's CAD items).
pub(crate) fn controls(doc: &CadDocument, selection: &[SelectionItem], own: &[Control]) -> Vec<Control> {
    let mut out = Vec::new();
    for cmd in registry::COMMANDS {
        out.push(Control { id: format!("cad:op:{}", cmd.id), label: cmd.label.to_string(), action: CadAction::CadInvoke { id: cmd.id.to_string() }, ready: registry::ready(cmd, doc, selection, own) });
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

/// A menu or context-menu popup's scrolling rows ([`popup_scroll`]).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PopupScroll;

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

/// Whether `cursor` is over an open popup (its wheel is the popup's).
pub(crate) fn over_popup<'a>(roots: impl IntoIterator<Item = (&'a ComputedNode, &'a UiGlobalTransform)>, cursor: Vec2) -> bool {
    roots.into_iter().any(|(node, t)| rect_of(node, t).contains(cursor))
}

// ---- Systems ---------------------------------------------------------------------

/// CAD mode's surfaces' systems (Input keys, Present drawing).
pub(super) fn build(app: &mut App) {
    use crate::ui_kit::text::{TextField, TextFieldApp};
    app.add_text_field(form::FORM, TextField::new("Operation parameter").sticky())
        .add_text_field(palette::PALETTE, TextField::new("Command palette search").sticky())
        .init_resource::<super::keys::Chord>()
        .add_systems(OnEnter(ModeScope::Cad), toolbar::spawn)
        .add_systems(
            Update,
            (form::input.in_set(crate::app::InputSet::Window), palette::input.in_set(crate::app::InputSet::Window), input.in_set(crate::app::InputSet::Window), radial::input.in_set(crate::app::InputSet::Window), context_menu::input.in_set(crate::app::InputSet::Window), popup_scroll.in_set(crate::app::InputSet::Window), toolbar::scroll.in_set(crate::app::InputSet::Window), super::keys::gate.in_set(crate::cad::CadKeySet::Gate))
                .chain()
                // The form's Tab focuses its first field before the numeric
                // bar reads Tab (it stands aside while a field types).
                .before(crate::cad::CadKeySet::NumericEntry)
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
    clicks: Query<(&SurfaceEntry, Option<&Enabled>), (With<crate::ui_kit::activation::Activated>, With<Button>)>,
    roots: Query<(&ComputedNode, &UiGlobalTransform), With<SurfaceRoot>>,
    bar: Query<(&ComputedNode, &UiGlobalTransform), With<menus::MenuRow>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    (mut keys, files, mut chord): (Option<ResMut<ButtonInput<KeyCode>>>, Option<Res<crate::cad::files::CadFiles>>, Option<ResMut<super::keys::Chord>>),
    windows: Query<&Window, With<PrimaryWindow>>,
    mut out: MessageWriter<Act<CadAction>>,
    (mut text, mut popup_was_open): (crate::ui_kit::text::TextFocus, Local<bool>),
) {
    let Some(mut doc) = doc else { return };
    // A menu, context menu or radial opening holds the keyboard while open
    // (as `CadDocument::ops.surface` did for every field): a non-sticky
    // field's entry ends (`Blur`; the numeric bar's, the inspector's, the
    // section offset's, a saved view's). The palette has its own field;
    // the name field and the forms are sticky and keep theirs.
    let popup = doc.ops.surface.as_ref().is_some_and(|o| !matches!(o.surface, Surface::Palette { .. }));
    if popup && !std::mem::replace(&mut *popup_was_open, popup) {
        text.release(false);
    }
    *popup_was_open = popup;
    let mut clicked = false;
    for (entry, enabled) in &clicks {
        clicked = true;
        if enabled.is_some_and(|e| !e.0) {
            if let Some(why) = &entry.refusal {
                doc.show(Err(why.clone()));
            }
            continue;
        }
        out.write(Act::ui(crate::cad::activation::guard(&doc, entry.action.clone())));
        if entry.closes {
            out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadSurface { surface: Surface::Closed })));
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
            out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadSurface { surface: Surface::Closed })));
        }
    }
    // A typing field's Escape is its own (the kit consumes it: the palette's
    // and the form's fields close or cancel on their `Cancel`). An open file
    // form's, results path form's or outliner menu's Escape is its own
    // (`files::form::input`, `results::forms::input`, `tree::popup::input`
    // close them and consume the key, but this chain is not ordered against
    // them): stand aside, as the threads' and calibrate's Escape do, so one
    // press never both closes that and cancels the tool.
    let other = files.as_ref().is_some_and(|f| f.form.is_some()) || doc.results.form.is_some() || doc.tree.menu.is_some();
    if !other
        && let Some(keys) = keys.as_mut()
        && keys.just_pressed(KeyCode::Escape)
    {
        let action = if open.is_some() {
            Some(CadAction::CadSurface { surface: Surface::Closed })
        } else if doc.ops.form.is_some() || doc.ops.active.is_some() {
            Some(CadAction::CadFormCancel)
        } else {
            None
        };
        // Consumed when it acted, so the topmost thing ends once: the later
        // Escape readers (calibrate's in `CadKeySet::EscapeTool`, the
        // threads' in `CadKeySet::Escape`, which checks the form but not a
        // form-less active op, and the Select tool's in `ToolKeys`) never
        // see this press; a pending two-step key is dropped, as
        // `keys::keys` drops it on Escape.
        if let Some(action) = action {
            keys.clear_just_pressed(KeyCode::Escape);
            if let Some(chord) = chord.as_mut() {
                chord.abandon();
            }
            out.write(Act::ui(crate::cad::activation::guard(&doc, action)));
        }
    }
}

/// Input: the wheel over a menu or context-menu popup scrolls its rows
/// (the layout clamps the far end, `ui_kit::clamp_scroll_positions`).
fn popup_scroll(mut wheel: MessageReader<MouseWheel>, windows: Query<&Window, With<PrimaryWindow>>, mut areas: Query<(&mut ScrollPosition, &ComputedNode, &UiGlobalTransform), With<PopupScroll>>) {
    let delta = wheel_delta(&mut wheel, WHEEL_LINE);
    if delta == 0.0 {
        return;
    }
    let Some(cursor) = windows.single().ok().and_then(Window::cursor_position) else { return };
    for (mut position, node, transform) in &mut areas {
        if rect_of(node, transform).contains(cursor) {
            position.0.y = (position.0.y - delta).max(0.0);
        }
    }
}

/// What the open popup shows, as a comparable text (`window`: the window's
/// logical size, which places and clamps it).
fn popup_key(doc: &CadDocument, selection: &[SelectionItem], window: Vec2) -> Option<String> {
    let open = doc.ops.surface.as_ref()?;
    // What enables an entry beyond the document: the connection, an edit in
    // flight, RoboCAD's desktop window and its command list.
    let commands = doc.commands.as_ref().map(|c| c.as_ref().map(|list| list.len()).ok());
    let gate = (doc.connected(), doc.edit.is_some(), doc.health.as_ref().map(|h| h.gui), commands);
    Some(format!("{:?}", (doc.generation, doc.revision, open, selection, doc.tool, doc.ops.active, doc.ops.form.as_ref().map(|f| f.op), gate, window.round())))
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
    selection: CadSelection,
) {
    let selection = selection.items();
    let window = windows.single().map_or(Vec2::new(1280.0, 720.0), |w| Vec2::new(w.width(), w.height()));
    let width = window.x;
    let key = doc.as_deref().and_then(|d| popup_key(d, &selection, window).map(|key| format!("{key}|source={}", crate::cad::activation::render_key(d))));
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
    let own = super::panel::own_controls(doc, &selection);
    let list = entries(&open.surface, doc, &selection, &own);
    let at = Vec2::from(open.at);
    match &open.surface {
        Surface::Closed => {}
        Surface::Palette { query } => palette::spawn(&mut commands, &k, doc, &selection, &own, query, open.highlight.unwrap_or(0), width),
        Surface::Menu { category } => {
            // Under its tab in the menu bar (laid out in an earlier frame: the
            // tabs are spawned once and lit in place, `toolbar::refresh`),
            // else where it opened.
            let under = tabs.iter().find(|(tab, node, _)| tab.0 == category.as_str() && node.size() != Vec2::ZERO).map(|(_, node, t)| {
                let r = rect_of(node, t);
                Vec2::new(r.min.x, r.max.y + 2.0)
            });
            popup_list(&mut commands, &k, &format!("{category} menu"), under.unwrap_or(at), window, &list, None);
        }
        Surface::Context { .. } => {
            // The Sketch section's heading above its first entry.
            let sketch = list.iter().position(|e| registry::SKETCH_CONTEXT.contains(&e.id.as_str())).map(|i| (i, "Sketch"));
            popup_list(&mut commands, &k, "Viewport context menu", at + Vec2::splat(2.0), window, &list, sketch);
        }
        Surface::ViewRadial { .. } | Surface::SelectRadial { .. } => radial::spawn(&mut commands, &k, &open, &list),
    }
}

/// Space kept between a popup and the window's edges (px).
const EDGE: f32 = 8.0;
/// A popup's padding and border, top and bottom together (px).
const POPUP_CHROME: f32 = 10.0;
/// The tallest a popup's rows get, and the least room they get before the
/// popup moves up to make it (px).
const ROWS_MAX: f32 = 550.0;
const ROWS_MIN: f32 = 160.0;
/// A popup's least width (px).
const POPUP_WIDTH: f32 = 220.0;

/// Where a popup asked for at `at` goes in a `window`-sized window, and
/// how tall its rows may get: below and right of `at` when it fits; moved
/// up when less than [`ROWS_MIN`] is left below, and left so its least
/// width fits; its rows scroll past the room left above the bottom edge,
/// which is the top of the mode switcher's strip ([`SWITCHER_STRIP`]).
/// Returns (left, top, rows' max height, max width).
fn popup_place(at: Vec2, window: Vec2) -> (f32, f32, f32, f32) {
    let left = at.x.min(window.x - EDGE - POPUP_WIDTH).max(EDGE);
    let bottom = window.y - SWITCHER_STRIP - EDGE - POPUP_CHROME;
    let top = (if bottom - at.y < ROWS_MIN { (bottom - ROWS_MIN).min(at.y) } else { at.y }).max(EDGE);
    let rows = (bottom - top).clamp(0.0, ROWS_MAX);
    (left, top, rows, (window.x - EDGE - left).max(POPUP_WIDTH))
}

/// A menu-like popup at `at`, kept inside the `window`-sized window
/// ([`popup_place`]): one ghost row per entry ("label    keys"), disabled
/// ones greyed, each a [`SurfaceEntry`]; `heading` (index, text): a
/// section heading above that entry, over a rule (display only).
fn popup_list(commands: &mut Commands, k: &Kit, label: &str, at: Vec2, window: Vec2, list: &[Entry], heading: Option<(usize, &str)>) {
    let (left, top, rows_max, max_width) = popup_place(at, window);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(top),
                min_width: Val::Px(POPUP_WIDTH),
                max_width: Val::Px(max_width),
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
            GlobalZIndex(POPUP_Z),
            AccessibleLabel::new(label),
            SurfaceRoot,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|p| {
            // 560 px tall at most with the padding and border, less where the
            // window is shorter; the wheel scrolls the rest.
            let rows = Node { max_height: Val::Px(rows_max), flex_direction: FlexDirection::Column, align_items: AlignItems::Stretch, row_gap: Val::Px(1.0), ..default() };
            p.spawn((k.scroll_area(rows, 0.0), PopupScroll)).with_children(|p| {
                if list.is_empty() {
                    p.spawn(k.text("(no commands)", size::CAPTION, FAINT, 0));
                }
                for (i, e) in list.iter().enumerate() {
                    if let Some((_, text)) = heading.filter(|(n, _)| *n == i) {
                        p.spawn((
                            k.text(text, size::CAPTION, FAINT, 0),
                            Node { margin: UiRect::top(Val::Px(4.0)), padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(4.0), Val::Px(2.0)), border: UiRect::top(Val::Px(1.0)), flex_shrink: 0.0, ..default() },
                            BorderColor::all(BORDER),
                            Pickable::IGNORE,
                        ));
                    }
                    // Left-aligned rows: the kit button's layout with its content at the start
                    // (`repaint_buttons` restores the look's padding, border and radius).
                    p.spawn(k.button(&e.text(), SurfaceEntry::of(e), Look::Ghost, e.ready.is_ok())).insert(Node { justify_content: JustifyContent::FlexStart, align_items: AlignItems::Center, flex_shrink: 0.0, ..default() });
                }
            });
        });
}
