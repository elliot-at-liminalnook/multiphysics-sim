//! CAD mode's panels on the UI kit (native-viewer.md §6, §9): the top bar
//! (document name, connection and saved state, Undo/Redo/Save/Refresh/Fit/
//! Physical), the left dock (the document's service, connection, autosave
//! and staleness, then the model tree: [`super::tree`]), the right dock (the
//! inspector, the physical link, the editable attributes, the history and
//! RoboCAD's command list: [`super::inspector`]) and the status bar (the edit
//! in flight, the last outcome). The 3D view sits between the docks
//! (`scene.rs`); every dock blocks picks and clicks behind it (`bevy::ui`
//! nodes block picking by default; `FocusPolicy::Block` stops
//! `Interaction` as the hardware panel does).
//!
//! - **Actions only.** Every button carries a [`CadButton`] holding the
//!   [`CadAction`] it writes; `name::buttons` (Input) writes it as
//!   `Act::ui`, the same value a key, a 3D pick, `system_ui` or REST writes.
//!   Nothing here talks to RoboCAD or changes [`CadDocument`].
//! - **One source for controls.** [`controls`] lists every button and chip
//!   (its `system_ui` id, label, action and why it is disabled now); the top
//!   bar's buttons and the inspector's attribute chips and Delete take their
//!   action and `Enabled` from it, and `system_ui` lists the same values.
//!   The one exception is the inspector's name field: a rename is typed, so
//!   it goes through the text field here or REST `cad_patch {"name"}`.
//! - **Refresh** (Present): runs only when `CadDocument.revision` (or its
//!   generation, or the name draft) changed; each part's key is recomputed
//!   then and only parts whose key changed are rebuilt. Scroll areas are
//!   never rebuilt, so their offsets are kept.
//! - **Name editing** ([`name`]) is the kit's one text field (`cad.name`):
//!   a press on the field gives it the keyboard, Enter writes `CadPatch
//!   {"name"}`, Escape cancels. An empty name, or an Enter while edits
//!   cannot be sent, keeps the field open with the reason under it (no
//!   request).
mod name;
#[cfg(test)]
#[path = "flow_tests.rs"]
mod flow_tests;

pub(super) use name::{NAME, NameDraft, NameField, name_entry};

use super::actions::CadAction;
use super::document::{CadDocument, CadTool, Connection, SelectMode};
use super::selection::{CadItems, CadSelection};
use super::surfaces::SurfaceRoot;
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::builder::ui_api::Enabled;
use crate::ui_kit::{DANGER, Dock, FAINT, Kit, LEFT_WIDTH, Look, OK, RIGHT_WIDTH, STATUSBAR, SUBTLE, SWITCHER_STRIP, TEXT, TOPBAR, Tint, UiFonts, WARN, divider, size, wheel_delta};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use bevy::ui::{ComputedNode, FocusPolicy, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use serde_json::{Map, Value};
use sim_runtime::cad_client::SelectionItem;

/// A CAD panel button: the action a press writes.
#[derive(Component, Clone, Debug)]
pub(super) struct CadButton(pub CadAction);

/// A display-only kit chip (a provenance label): a press does nothing.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct Inert;

/// The panel parts, each rebuilt alone when its content changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Part {
    Top,
    Document,
    Tree,
    /// cad-organize: the outliner's search field, New group, Expand all and
    /// Collapse all (`tree::tools`; its context menu and name dialog are
    /// popups of their own, `tree::popup`).
    TreeTools,
    Name,
    /// cad-organize: RoboCAD's Comments dock (`threads::dock`) and
    /// References dock with the linked system file (`references::dock`),
    /// as sections at the top of the right dock while shown.
    Comments,
    References,
    Components,
    Composition,
    Experiments,
    ExperimentReview,
    Motion,
    Inspector,
    Physical,
    Attributes,
    /// cad-physical-inspect: RoboCAD's Robot dock and Materials dock, as
    /// sections of the right dock (`robot::panel`, `materials`).
    Robot,
    Materials,
    /// cad-print: RoboCAD's print jobs (`print::jobs_panel`).
    Print,
    History,
    Commands,
    Status,
}

/// A container whose children are one part; `key` is what it shows now.
#[derive(Component)]
pub(super) struct CadList {
    part: Part,
    key: Option<String>,
}
impl CadList {
    fn new(part: Part) -> Self {
        CadList { part, key: None }
    }
}

/// The top bar (found again to notice a respawn).
#[derive(Component)]
struct CadRoot;
/// The model tree's scroll area.
#[derive(Component)]
struct TreeScroll;
/// The right dock's scroll area.
#[derive(Component)]
struct InspectorScroll;

/// One CAD control as the panel shows it and `system_ui` lists it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Control {
    /// `cad:undo`, `cad:node:<id>`, … (the patterns in `actions::CadAction::controls`).
    pub id: String,
    pub label: String,
    pub action: CadAction,
    /// Ok when enabled now, else why not.
    pub ready: Result<(), String>,
}

/// Why no edit can be sent now (an edit in flight, no connection), if so:
/// the refusal `actions::apply` gives (`CadDocument::edit_refusal`).
pub(super) fn edit_blocked(doc: &CadDocument) -> Option<String> {
    doc.edit_refusal()
}

fn ready(blocked: Option<String>) -> Result<(), String> {
    blocked.map_or(Ok(()), Err)
}

/// `PATCH` attributes with one key.
pub(super) fn patch(id: &str, key: &str, value: Value) -> CadAction {
    let mut attrs = Map::new();
    attrs.insert(key.to_string(), value);
    CadAction::CadPatch { id: id.to_string(), attrs }
}

/// RoboCAD's keys for a command as written in its keymap (a string or a list).
pub(super) fn keys_text(keys: &Value) -> String {
    match keys {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().filter_map(|k| k.as_str()).collect::<Vec<_>>().join(", "),
        _ => String::new(),
    }
}

/// A command button's label: "label (keys)", or the label alone without keys.
pub(super) fn command_label(label: &str, keys: &Value) -> String {
    let keys = keys_text(keys);
    if keys.is_empty() { label.to_string() } else { format!("{label} ({keys})") }
}

/// Why RoboCAD's command registry is unavailable, if it is.
pub(super) const HEADLESS_COMMANDS: &str = "RoboCAD's command registry belongs to its desktop window; this service is headless. Its ops are in this window's menu bar, tools toolbar, right-click menu and command palette (Ctrl+Space), each opening its parameter form.";

/// A material's id and the label to show for it (its name, else its id).
pub(super) fn material(v: &Value) -> Option<(String, String)> {
    let id = v.get("id")?.as_str()?.to_string();
    let label = v.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).map_or_else(|| id.clone(), str::to_string);
    Some((id, label))
}

/// The control with `id`, if it is listed now.
pub(super) fn control<'a>(all: &'a [Control], id: &str) -> Option<&'a Control> {
    all.iter().find(|c| c.id == id)
}

/// Every CAD control with its state now: the panel's own
/// ([`own_controls`]), then the command surfaces' (`surfaces::controls`:
/// `cad:op:<id>` per RoboCAD command, `cad:surface:<kind>`,
/// `cad:menu:<category>`, the open form's `cad:form:*`).
pub(crate) fn controls(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<Control> {
    let mut all = own_controls(doc, selection);
    let surfaces = super::surfaces::controls(doc, selection, &all);
    all.extend(surfaces);
    all
}

/// The panel's own controls: the top bar, Delete, the selected node's
/// lock, disable and material choices, one select and one visibility
/// toggle per tree row, one per RoboCAD GUI registry command. (The name
/// field is not one: see the module doc.)
pub(crate) fn own_controls(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<Control> {
    let blocked = edit_blocked(doc);
    let history = doc.doc.as_ref().map(|d| &d.history);
    let last_undo = history.and_then(|h| h.undo.last());
    let last_redo = history.and_then(|h| h.redo.last());
    let mut out = Vec::new();
    let mut add = |id: String, label: String, action: CadAction, ready: Result<(), String>| out.push(Control { id, label, action, ready });
    let undo_ready = ready(blocked.clone().or_else(|| last_undo.is_none().then(|| "nothing to undo in RoboCAD's history".to_string())));
    add("cad:undo".into(), last_undo.map_or("Undo".to_string(), |l| format!("Undo {l}")), CadAction::CadUndo, undo_ready);
    let redo_ready = ready(blocked.clone().or_else(|| last_redo.is_none().then(|| "nothing to redo in RoboCAD's history".to_string())));
    add("cad:redo".into(), last_redo.map_or("Redo".to_string(), |l| format!("Redo {l}")), CadAction::CadRedo, redo_ready);
    add("cad:save".into(), "Save".into(), CadAction::CadSave { path: None }, ready(blocked.clone()));
    // Refresh is the retry after a lost connection, so it stays enabled
    // (`apply` answers what it did); Physical needs the service (`sync::fetch_physical`).
    add("cad:refresh".into(), "Refresh".into(), CadAction::CadRefresh, Ok(()));
    if doc.uncertain_edit.is_some() {
        let ready = if doc.dirty_known_at.is_none() && doc.stale.is_none() && doc.connected() { Ok(()) } else { Err("Waiting for fresh source and history".into()) };
        add("cad:reconcile_edit".into(), "Acknowledge inspected unknown edit outcome".into(), CadAction::CadReconcileEdit { acknowledge: true, revision: Some(doc.shown_revision()) }, ready);
    }
    add("cad:fit".into(), "Fit".into(), CadAction::CadFit { id: None }, Ok(()));
    let physical = if doc.connected() { Ok(()) } else { Err(format!("not connected to RoboCAD: {}", doc.connection_line().0)) };
    add("cad:physical".into(), "Physical".into(), CadAction::CadPhysical, physical);
    // RoboCAD's Delete: every selected node in one step (the catalogue's
    // `edit.delete`); REST `cad_delete {id}` still deletes one node.
    let delete = if selection.first_node().is_some() { ready(blocked.clone()) } else { Err("nothing is selected".to_string()) };
    add("cad:delete".into(), "Delete".into(), CadAction::CadInvoke { id: "edit.delete".into() }, delete);
    // The inspected node's flags and material: the inspector's chips write
    // these same actions (`inspector::attributes`).
    let inspected = selection.first_node().and_then(|id| doc.doc.as_ref()?.nodes.iter().find(|n| n.id == id));
    if let Some(n) = inspected {
        let label = format!("{} {}", if n.locked { "Unlock" } else { "Lock" }, n.name);
        add(format!("cad:locked:{}", n.id), label, patch(&n.id, "locked", Value::Bool(!n.locked)), ready(blocked.clone()));
        let label = format!("{} {}", if n.disabled { "Enable" } else { "Disable" }, n.name);
        add(format!("cad:disabled:{}", n.id), label, patch(&n.id, "disabled", Value::Bool(!n.disabled)), ready(blocked.clone()));
        for (material_id, material_label) in doc.doc.iter().flat_map(|d| d.materials.iter().filter_map(material)) {
            let label = format!("Set {} material to {material_label}", n.name);
            add(format!("cad:material:{}:{material_id}", n.id), label, patch(&n.id, "material", Value::String(material_id)), ready(blocked.clone()));
        }
    }
    for row in doc.rows(selection) {
        add(format!("cad:node:{}", row.id), row.name.clone(), CadAction::CadSelect { ids: vec![row.id.clone()], items: Vec::new(), extend: false, toggle: false, picked_at: None }, Ok(()));
        let label = format!("{} {}", if row.visible { "Hide" } else { "Show" }, row.name);
        add(format!("cad:visible:{}", row.id), label, patch(&row.id, "visible", Value::Bool(!row.visible)), ready(blocked.clone()));
    }
    // Selection modes and commands (RoboCAD's select.* and edit.select_*):
    // the viewer's own state until a selection is pushed, so always enabled
    // unless they need a selection.
    for mode in SelectMode::ALL {
        let label = if mode == doc.select_mode { format!("Select {} (active)", mode.label()) } else { format!("Select {}", mode.label()) };
        add(format!("cad:mode:{}", mode.name()), label, CadAction::CadSelectMode { mode }, Ok(()));
    }
    add("cad:select_all".into(), "Select All".into(), CadAction::CadSelectAll, Ok(()));
    add("cad:invert_selection".into(), "Invert Selection".into(), CadAction::CadInvertSelection, Ok(()));
    let same = if selection.is_empty() { Err("nothing is selected".to_string()) } else { Ok(()) };
    add("cad:select_same_material".into(), "Select Same Material".into(), CadAction::CadSelectSameMaterial, same);
    let edges = if selection.iter().any(|i| i.1 == "edge") { Ok(()) } else { Err("no edges are selected (edge mode, E)".to_string()) };
    add("cad:edges_to_faces".into(), "Selection: edges → bounding faces".into(), CadAction::CadEdgesToFaces, edges);
    // The Alt+click menu's entries while it is open (RoboCAD's labels).
    if let Some(c) = &doc.candidates {
        for (n, item) in c.items.iter().enumerate() {
            // RoboCAD's text: "name: kind #i", without "#i" for a body.
            let index = if item.1 == "body" { String::new() } else { format!(" #{}", item.2) };
            let label = format!("{}: {}{index}", doc.node_name(&item.0), item.1);
            // No picked_at: `selection::select` stamps a menu choice with the
            // menu's revision (`menu_revision`) and closes a stale menu.
            add(format!("cad:candidate:{n}"), label, CadAction::CadSelect { ids: Vec::new(), items: vec![item.clone()], extend: c.extend, toggle: c.toggle, picked_at: None }, Ok(()));
        }
    }
    // Tools (RoboCAD's tool.*): activating one only changes the view; its
    // commit is refused by name when no edit can be sent.
    for tool in CadTool::ALL {
        let label = if tool == doc.tool { format!("{} (active)", tool.label()) } else { tool.label().to_string() };
        add(format!("cad:tool:{}", tool.name()), label, CadAction::CadTool { tool }, Ok(()));
    }
    add("cad:cancel".into(), "Cancel (Escape)".into(), CadAction::CadCancel, Ok(()));
    let gui = doc.health.as_ref().is_some_and(|h| h.gui);
    if let Some(Ok(commands)) = &doc.commands {
        for (id, info) in commands {
            let why = if !gui { Some(HEADLESS_COMMANDS.to_string()) } else { blocked.clone() };
            add(format!("cad:command:{id}"), command_label(&info.label, &info.keys), CadAction::CadCommand { id: id.clone() }, ready(why));
        }
    }
    out
}

pub(super) fn build(app: &mut App) {
    use crate::ui_kit::text::TextFieldApp;
    app.init_resource::<NameDraft>()
        .add_text_field(NAME, name::field())
        .add_text_field(super::numeric::NUMERIC, super::numeric::field())
        .add_systems(OnEnter(ModeScope::Cad), spawn)
        // Before CAD's keys: a press that gives the name field the keyboard holds the frame's keys.
        .add_systems(Update, (name::buttons, name_entry).chain().in_set(crate::cad::CadKeySet::Focus).run_if(in_state(ViewerMode::Cad)))
        .add_systems(Update, (scroll, refresh).chain().in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

/// A column, `gap` px apart.
pub(super) fn column(gap: f32) -> Node {
    Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(gap), flex_shrink: 0.0, ..default() }
}

/// OnEnter(Cad): the docks with empty parts; `refresh` fills them.
fn spawn(mut commands: Commands, fonts: Res<UiFonts>, mut draft: ResMut<NameDraft>) {
    // The kit blurs every field on a mode switch; a draft from the last visit ends.
    name::end(&mut draft);
    let k = Kit::new(&fonts);
    commands.spawn((
        k.dock(Dock::Top { height: TOPBAR }, Node { padding: UiRect::horizontal(Val::Px(14.0)), align_items: AlignItems::Center, column_gap: Val::Px(10.0), overflow: Overflow::clip(), ..default() }),
        FocusPolicy::Block,
        AccessibleLabel::new("CAD document toolbar"),
        CadRoot,
        CadList::new(Part::Top),
    ));
    commands
        .spawn((
            k.dock(Dock::Left { top: TOPBAR, bottom: STATUSBAR, width: LEFT_WIDTH }, Node { flex_direction: FlexDirection::Column, ..default() }),
            FocusPolicy::Block,
            AccessibleLabel::new("CAD model tree"),
        ))
        .with_children(|left| {
            left.spawn((Node { padding: UiRect::new(Val::Px(14.0), Val::Px(14.0), Val::Px(12.0), Val::Px(0.0)), ..column(4.0) }, CadList::new(Part::Document)));
            left.spawn(Node { padding: UiRect::horizontal(Val::Px(14.0)), ..column(0.0) }).with_children(|s| {
                s.spawn(k.section("Model tree"));
            });
            left.spawn((Node { padding: UiRect::new(Val::Px(14.0), Val::Px(14.0), Val::Px(4.0), Val::Px(0.0)), ..column(4.0) }, CadList::new(Part::TreeTools)));
            left.spawn((k.scroll_area(Node { flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() }, 0.0), TreeScroll)).with_children(|area| {
                area.spawn((Node { padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(6.0), Val::Px(14.0)), ..column(1.0) }, CadList::new(Part::Tree)));
            });
        });
    commands
        .spawn((
            k.dock(Dock::Right { top: TOPBAR, bottom: STATUSBAR, width: RIGHT_WIDTH }, Node { flex_direction: FlexDirection::Column, ..default() }),
            FocusPolicy::Block,
            AccessibleLabel::new("CAD inspector"),
        ))
        .with_children(|right| {
            right.spawn((k.scroll_area(Node { flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() }, 0.0), InspectorScroll)).with_children(|area| {
                area.spawn(Node { padding: UiRect::all(Val::Px(14.0)), ..column(4.0) }).with_children(|body| {
                    for part in [Part::Experiments, Part::ExperimentReview, Part::Motion, Part::Components, Part::Composition, Part::Comments, Part::References, Part::Name, Part::Inspector, Part::Physical, Part::Attributes, Part::Robot, Part::Materials, Part::Print, Part::History, Part::Commands] {
                        body.spawn((column(4.0), CadList::new(part)));
                    }
                });
            });
        });
    // Normal padding: the mode switcher has its own strip under the status
    // bar (`SWITCHER_STRIP`), so no right-end room is kept for it here.
    commands.spawn((
        k.dock(Dock::Bottom { height: STATUSBAR }, Node { padding: UiRect::horizontal(Val::Px(14.0)), align_items: AlignItems::Center, column_gap: Val::Px(14.0), overflow: Overflow::clip(), ..default() }),
        FocusPolicy::Block,
        AccessibleLabel::new("CAD status"),
        CadList::new(Part::Status),
    ));
}

/// The wheel over a dock scrolls it (the tree on the left, the inspector on
/// the right), unless an open command popup over it (or the outliner's
/// context menu or name dialog, cad-organize) takes the wheel.
#[allow(clippy::type_complexity)]
fn scroll(
    mut wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    popups: Query<(&ComputedNode, &UiGlobalTransform), With<SurfaceRoot>>,
    tree_popups: Query<(&ComputedNode, &UiGlobalTransform), Or<(With<super::tree::TreeMenuRoot>, With<super::tree::TreeDialogRoot>)>>,
    mut tree: Query<&mut ScrollPosition, (With<TreeScroll>, Without<InspectorScroll>)>, mut inspector: Query<&mut ScrollPosition, (With<InspectorScroll>, Without<TreeScroll>)>) {
    let delta = wheel_delta(&mut wheel, crate::ui_kit::WHEEL_LINE);
    if delta == 0.0 {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(p) = window.cursor_position() else { return };
    if p.y <= TOPBAR || p.y >= window.height() - STATUSBAR - SWITCHER_STRIP || super::surfaces::over_popup(&popups, p) || super::surfaces::over_popup(&tree_popups, p) {
        return;
    }
    let areas = if p.x <= LEFT_WIDTH {
        tree.iter_mut().collect::<Vec<_>>()
    } else if p.x >= window.width() - RIGHT_WIDTH {
        inspector.iter_mut().collect::<Vec<_>>()
    } else {
        return;
    };
    for mut position in areas {
        position.0.y = (position.0.y - delta).max(0.0);
    }
}

/// What the refresh last saw.
#[derive(Default)]
struct Drawn {
    root: Option<Entity>,
    stamp: Option<String>,
}

/// Present: rebuild the parts whose content changed (see the module doc);
/// also when the sub-body topology changes (`CadTopology::epoch`: the
/// inspector's face, edge and vertex details).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    topology: Option<Res<super::topology::CadTopology>>,
    draft: Res<NameDraft>,
    fonts: Res<UiFonts>,
    mut drawn: Local<Drawn>,
    mut seen_epoch: Local<Option<u64>>,
    roots: Query<Entity, With<CadRoot>>,
    mut lists: Query<(Entity, &mut CadList)>,
    mut rows: Query<(Entity, &super::tree::TreeRowId, &mut Tint, &mut BorderColor, &AccessibleLabel)>,
    mut eyes: Query<&mut Enabled, With<super::tree::EyeChip>>,
    plane: Option<Res<super::CadActivePlane>>,
    selection: CadSelection,
    (components, composition): (Res<super::components::ComponentsState>, Res<super::composition::CadCompositionState>),
    (experiments, review, motion): (Res<super::experiments::ExperimentsState>, Res<super::experiment_review::ReviewState>, Res<super::motion::MotionState>),
) {
    let root = roots.iter().next();
    let stamp = doc.as_deref().map(super::activation::render_key);
    let added = doc.as_ref().is_some_and(|d| d.is_added());
    let epoch = topology.as_ref().map(|t| t.epoch);
    // The header's active-plane line follows the plane (cad-sketch).
    let plane_changed = plane.as_ref().is_some_and(|p| p.is_changed());
    if root == drawn.root && stamp == drawn.stamp && !added && !draft.is_changed() && epoch == *seen_epoch && !plane_changed && !components.is_changed() && !composition.is_changed() && !experiments.is_changed() && !review.is_changed() && !motion.is_changed() {
        return;
    }
    drawn.root = root;
    drawn.stamp = stamp.clone();
    *seen_epoch = epoch;
    let doc = doc.as_deref();
    let topology = topology.as_deref();
    let plane = plane.as_deref();
    let selection = selection.items();
    if let Some(doc) = doc {
        super::tree::highlight(&mut commands, doc, &selection, &mut rows, &mut eyes);
    }
    let k = Kit::new(&fonts);
    for (entity, mut list) in &mut lists {
        let part = list.part;
        let content_key = match part {
            Part::Experiments => format!("{}:{stamp:?}", super::experiments::key(&experiments)),
            Part::ExperimentReview => format!("{}:{stamp:?}", super::experiment_review::key(&review)),
            Part::Motion => format!("{}:{stamp:?}", super::motion::key(&motion)),
            Part::Components => format!("{}:{stamp:?}", super::components::key(&components)),
            Part::Composition => format!("{}:{stamp:?}", super::composition::key(&composition)),
            _ => part_key(part, doc, &selection, topology, &draft, plane),
        };
        let key = format!("{content_key}|source={:?}", doc.map(super::activation::render_key));
        if list.key.as_ref() == Some(&key) {
            continue;
        }
        list.key = Some(key);
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).with_children(|p| match doc {
            None => {
                if part == Part::Document {
                    p.spawn(k.caption("No CAD document is open: choose a .rcad file in the mode switcher's document picker. With a document open, File → Open… (Ctrl+O) opens another, and the Attach field under an unconnected document's status attaches to a running RoboCAD."));
                }
            }
            Some(doc) => match part {
                Part::Top => top(p, &k, doc, &selection, plane),
                Part::Document => document(p, &k, doc),
                Part::Status => status(p, &k, doc),
                Part::Tree => super::tree::draw(p, &k, doc, &selection),
                Part::TreeTools => super::tree::tools(p, &k, doc, &selection),
                Part::Comments => super::threads::dock::draw(p, &k, doc, &selection),
                Part::References => super::references::dock::draw(p, &k, doc, &selection),
                Part::Experiments => super::experiments::draw(p, &k, doc, &experiments),
                Part::ExperimentReview => super::experiment_review::draw(p, &k, doc, &review),
                Part::Motion => super::motion::draw(p, &k, doc, &motion),
                Part::Components => super::components::draw(p, &k, doc, &components),
                Part::Composition => super::composition::draw(p, &k, doc, &composition),
                Part::Name => super::inspector::name(p, &k, doc, &selection, &draft),
                Part::Inspector => super::inspector::inspector(p, &k, doc, &selection, topology),
                Part::Physical => super::inspector::physical(p, &k, doc, &selection),
                Part::Attributes => super::inspector::attributes(p, &k, doc, &selection),
                Part::Robot => super::robot::panel::draw(p, &k, doc, &selection),
                Part::Materials => super::materials::draw(p, &k, doc, &selection),
                Part::Print => super::print::jobs_panel::draw(p, &k, doc),
                Part::History => super::inspector::history(p, &k, doc),
                Part::Commands => super::inspector::commands(p, &k, doc, &selection),
            },
        });
    }
}

/// What a part shows now, as a comparable text.
fn part_key(part: Part, doc: Option<&CadDocument>, selection: &[SelectionItem], topology: Option<&super::topology::CadTopology>, draft: &NameDraft, plane: Option<&super::CadActivePlane>) -> String {
    let Some(doc) = doc else { return "no document".to_string() };
    match part {
        Part::Top => format!("{:?}", (doc.document_name(), connection_state(&doc.connection), dirty(doc), doc.health.is_some(), top_controls(doc, selection), plane_line(doc, plane))),
        Part::Document => format!("{:?}", (path_line(doc), doc.service_line(), doc.connection_line(), doc.connection == Connection::Connected, autosave_line(doc), &doc.stale)),
        Part::Status => format!("{:?}", (doc.edit_label(), &doc.status)),
        Part::Tree => super::tree::key(doc),
        Part::TreeTools => super::tree::tools_key(doc, selection),
        Part::Comments => super::threads::dock::key(doc, selection),
        Part::References => super::references::dock::key(doc, selection),
        Part::Experiments | Part::ExperimentReview | Part::Motion | Part::Components | Part::Composition => unreachable!("persistent feature keys are read by refresh"),
        Part::Name => super::inspector::name_key(doc, selection, draft),
        Part::Inspector => super::inspector::inspector_key(doc, selection, topology),
        Part::Physical => super::inspector::physical_key(doc, selection),
        Part::Attributes => super::inspector::attributes_key(doc, selection),
        Part::Robot => super::robot::panel::key(doc, selection),
        Part::Materials => super::materials::key(doc, selection),
        Part::Print => super::print::jobs_panel::key(doc),
        Part::History => format!("{:?}", doc.doc.as_ref().map(|d| &d.history)),
        Part::Commands => super::inspector::commands_key(doc),
    }
}

/// The top bar's buttons: (control, look, divider before it).
fn top_controls(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<(Control, Look, bool)> {
    let mut all = own_controls(doc, selection);
    all.retain(|c| matches!(c.id.as_str(), "cad:undo" | "cad:redo" | "cad:save" | "cad:refresh" | "cad:fit" | "cad:physical" | "cad:reconcile_edit"));
    all.into_iter().map(|c| {
        let look = if c.id == "cad:save" && dirty(doc) == Some(true) { Look::Primary } else { Look::Secondary };
        let gap = c.id == "cad:refresh";
        (c, look, gap)
    }).collect()
}

/// RoboCAD's unsaved-edits flag (None while it is being refetched after an
/// edit, or before RoboCAD answers): `CadDocument::unsaved`.
fn dirty(doc: &CadDocument) -> Option<bool> {
    doc.unsaved()
}

/// The connection's state word and colour (the error itself is in the document block).
fn connection_state(c: &Connection) -> (&'static str, &'static str) {
    match c {
        Connection::Connecting { .. } => ("Connecting…", "warn"),
        Connection::Connected => ("Connected", "ok"),
        Connection::Lost { .. } => ("Lost", "danger"),
    }
}

fn colour(name: &str) -> Color {
    match name {
        "ok" => OK,
        "warn" => WARN,
        _ => DANGER,
    }
}

/// A label too long for a toolbar button, shortened at a character boundary.
fn short(label: &str, max: usize) -> String {
    if label.chars().count() <= max { label.to_string() } else { format!("{}…", label.chars().take(max - 1).collect::<String>()) }
}

/// The header's active-plane line (cad-sketch): "Active plane: XY", with
/// "2D snap" while 2D snapping is on.
fn plane_line(doc: &CadDocument, plane: Option<&super::CadActivePlane>) -> Option<(String, bool)> {
    plane.map(|p| (format!("Active plane: {}", p.label(doc)), p.snap_2d))
}

/// The top bar: document name, connection, saved state, the active plane, the document buttons.
fn top(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem], plane: Option<&super::CadActivePlane>) {
    p.spawn(k.text("CAD", size::PRODUCT, SUBTLE, 2));
    p.spawn((k.title(doc.document_name()), Node { flex_shrink: 1.0, min_width: Val::Px(0.0), ..default() }));
    let (word, tone) = connection_state(&doc.connection);
    indicator(p, k, word, colour(tone));
    match dirty(doc) {
        Some(true) => indicator(p, k, "Unsaved edits", WARN),
        Some(false) => indicator(p, k, "Saved", OK),
        None if doc.health.is_some() => indicator(p, k, "Checking saved state…", SUBTLE),
        None => {}
    }
    if let Some((line, snap_2d)) = plane_line(doc, plane) {
        p.spawn(k.text(line, size::CAPTION, SUBTLE, 1));
        if snap_2d {
            p.spawn(k.text("2D snap", size::CAPTION, SUBTLE, 1));
        }
    }
    p.spawn(Node { flex_grow: 1.0, ..default() });
    for (c, look, gap) in top_controls(doc, selection) {
        if gap {
            p.spawn(divider());
        }
        p.spawn(k.button(&short(&c.label, 36), CadButton(c.action), look, c.ready.is_ok()));
    }
}

/// A status word with its dot.
fn indicator(p: &mut ChildSpawnerCommands, k: &Kit, word: &str, color: Color) {
    p.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, flex_shrink: 0.0, ..default() }).with_children(|r| {
        r.spawn(k.dot(color));
        r.spawn(k.text(word, size::CAPTION, color, 1));
    });
}

/// Where the document lives: RoboCAD's path, or what was asked for before it answers.
fn path_line(doc: &CadDocument) -> String {
    let path = doc.doc.as_ref().and_then(|d| d.path.clone()).or_else(|| doc.health.as_ref().and_then(|h| h.path.clone()));
    match path {
        Some(p) => p,
        None if doc.health.is_some() || doc.doc.is_some() => "Not saved to a file yet".to_string(),
        None => doc.target.describe(),
    }
}

/// The autosave line: RoboCAD's desktop window autosaves; a headless service does not.
fn autosave_line(doc: &CadDocument) -> Option<(String, bool)> {
    let health = doc.health.as_ref()?;
    if !health.gui {
        return Some(("Autosave: not applicable (headless service; RoboCAD's desktop window autosaves)".to_string(), false));
    }
    Some(match &doc.autosave {
        None => ("Autosave: asking RoboCAD…".to_string(), false),
        Some(Err(e)) => (format!("Autosave: {e}"), true),
        Some(Ok(a)) => {
            let mut line = format!("Autosave: {}", if a.running { "running" } else { "not running" });
            if let Some(r) = a.saved_revision {
                line += &format!(" · saved revision {r}");
            }
            if let Some(r) = a.revision {
                line += &format!(" · document revision {r}");
            }
            if let Some(p) = &a.path {
                line += &format!(" · {p}");
            }
            (line, false)
        }
    })
}

/// The left dock's head: path, service, connection, autosave, staleness.
fn document(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    p.spawn(k.text(path_line(doc), size::SMALL, TEXT, 1));
    p.spawn(k.caption(doc.service_line()));
    // Connecting (with its seconds), connected (revision, saved state and any
    // staleness) or lost (the error verbatim).
    let (line, error) = doc.connection_line();
    let connected = doc.connection == Connection::Connected;
    p.spawn(k.text(line, size::SMALL, if error { DANGER } else if connected { SUBTLE } else { WARN }, 0));
    if let Some((line, error)) = autosave_line(doc) {
        p.spawn(k.text(line, size::SMALL, if error { DANGER } else { SUBTLE }, 0));
    }
    // While connected the connection line already names it.
    if !connected && let Some(stale) = &doc.stale {
        p.spawn(k.text(format!("View may be behind RoboCAD: {stale}"), size::SMALL, WARN, 0));
    }
    // Not connected: the attach-URL field (`attach`, the same CadOpen {url} as cad_open).
    if !connected {
        super::attach::root(p);
    }
}

/// The status bar: the edit in flight, then the last outcome (one line each).
fn status(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    if let Some(label) = doc.edit_label() {
        p.spawn(k.text(format!("Sending: {label}…"), size::CAPTION, WARN, 1)).insert(TextLayout::no_wrap());
    }
    match &doc.status {
        Some(Ok(message)) => {
            p.spawn(k.text(message.clone(), size::CAPTION, SUBTLE, 0)).insert(TextLayout::no_wrap());
        }
        Some(Err(error)) => {
            p.spawn(k.text(error.clone(), size::CAPTION, DANGER, 0)).insert(TextLayout::no_wrap());
        }
        None => {
            p.spawn(k.text("Ready", size::CAPTION, FAINT, 0)).insert(TextLayout::no_wrap());
        }
    }
}

#[cfg(test)]
mod activation_tests;
