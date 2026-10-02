//! The outliner's input (ViewerSet::Input, before CAD's keys): Ctrl+F over
//! the model tree dock ([`search`]), the rows' presses and drags from
//! Bevy picking's pointer messages ([`rows`]), and the three kit fields
//! ([`fields`]: the search, the inline rename and the dialog's name). Each
//! writes `CadAction`s; the drag keeps its preview (`TreeState::drag`) as
//! a local gesture and commits with one `move`.
use super::handle::{TreeArgs, TreeOp};
use super::rows::{TreeEnd, TreeField, TreeRowId, end_menu};
use super::state::{Claim, DropTarget, TreeDrag, move_plan};
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::selection::{CadItems, CadSelection};
use crate::ui_kit::form::FormHit;
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFocus};
use crate::ui_kit::{LEFT_WIDTH, STATUSBAR, SWITCHER_STRIP, TOPBAR};
use bevy::ecs::system::SystemParam;
use bevy::picking::events::{Click, Drag, DragDrop, DragEnd, DragEnter, DragLeave, DragOver, DragStart, Pointer, Press};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use std::time::{Duration, Instant};

/// "Search (Ctrl+F)…" (RoboCAD's `outliner.search`, ui/strings.py:20).
pub(in crate::cad) const SEARCH: FieldId = FieldId("cad.tree.search");
/// The inline rename on a row.
pub(in crate::cad) const RENAME: FieldId = FieldId("cad.tree.rename");
/// "Group name:" in the "Organize components" dialog.
pub(in crate::cad) const GROUP_NAME: FieldId = FieldId("cad.tree.group");

pub(super) fn search_field() -> TextField {
    TextField::new("Search the model tree").placeholder("Search (Ctrl+F)…")
}
/// The rename starts with the name selected (Qt's `editItem`).
pub(super) fn rename_field() -> TextField {
    TextField::new("New name").select_on_focus()
}
/// The modal dialog keeps the keyboard on a press on its backdrop.
pub(super) fn group_field() -> TextField {
    TextField::new("Group name").placeholder("Group name").sticky()
}

/// Qt's default double-click interval (as the materials rows).
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// How far a press must move before it is a row drag (px).
const DRAG_SLOP: f32 = 4.0;
/// The part of a group row's height, from its top, where a drop goes in
/// front of the group rather than into it.
const BEFORE_BAND: f32 = 0.25;

/// The pointer is over the model tree dock (between the top bar and the
/// status bar, as the dock's wheel scrolling reads it).
pub(crate) fn over_dock(p: Vec2, height: f32) -> bool {
    p.x <= LEFT_WIDTH && p.y > TOPBAR && p.y < height - STATUSBAR - SWITCHER_STRIP
}

/// Input: Ctrl+F (Cmd+F) with the pointer over the model tree dock gives
/// the search field the keyboard and consumes the key, so Fillet (RoboCAD's
/// Ctrl+F binding, `keys::keys`, which runs after this) does not also run.
pub(in crate::cad) fn search(keys: Option<ResMut<ButtonInput<KeyCode>>>, windows: Query<&Window, With<PrimaryWindow>>, doc: Option<ResMut<CadDocument>>) {
    let (Some(mut keys), Some(mut doc)) = (keys, doc) else { return };
    if !keys.just_pressed(KeyCode::KeyF) || doc.doc.is_none() {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let other = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::AltLeft, KeyCode::AltRight]);
    let Some((p, height)) = windows.single().ok().and_then(|w| w.cursor_position().map(|p| (p, w.height()))) else { return };
    if !ctrl || other || !over_dock(p, height) {
        return;
    }
    keys.clear_just_pressed(KeyCode::KeyF);
    doc.tree.claim = Some(Claim::Search);
}

/// The pointer messages the rows read (Bevy picking writes each `Pointer<E>`
/// as a message too, in PreUpdate: `bevy_picking::events::pointer_events`).
#[derive(SystemParam)]
pub(super) struct PointerMsgs<'w, 's> {
    press: MessageReader<'w, 's, Pointer<Press>>,
    click: MessageReader<'w, 's, Pointer<Click>>,
    start: MessageReader<'w, 's, Pointer<DragStart>>,
    drag: MessageReader<'w, 's, Pointer<Drag>>,
    enter: MessageReader<'w, 's, Pointer<DragEnter>>,
    over: MessageReader<'w, 's, Pointer<DragOver>>,
    leave: MessageReader<'w, 's, Pointer<DragLeave>>,
    drop: MessageReader<'w, 's, Pointer<DragDrop>>,
    end: MessageReader<'w, 's, Pointer<DragEnd>>,
}
impl PointerMsgs<'_, '_> {
    fn clear(&mut self) {
        self.press.clear();
        self.click.clear();
        self.start.clear();
        self.drag.clear();
        self.enter.clear();
        self.over.clear();
        self.leave.clear();
        self.drop.clear();
        self.end.clear();
    }
}

/// The tree's entities a pointer message may name.
#[derive(SystemParam)]
pub(super) struct RowLookup<'w, 's> {
    rows: Query<'w, 's, &'static TreeRowId>,
    ends: Query<'w, 's, (), With<TreeEnd>>,
    parents: Query<'w, 's, &'static ChildOf>,
    layout: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform)>,
}

/// What a pointer is over in the tree.
enum Hit<'a> {
    Row(Entity, &'a TreeRowId),
    End,
}

impl RowLookup<'_, '_> {
    /// The row (or the room under the rows) `entity` is or lies in: a chip
    /// or label of a row resolves to the row.
    fn find(&self, entity: Entity) -> Option<Hit<'_>> {
        let mut e = entity;
        for _ in 0..6 {
            if let Ok(row) = self.rows.get(e) {
                return Some(Hit::Row(e, row));
            }
            if self.ends.contains(e) {
                return Some(Hit::End);
            }
            e = self.parents.get(e).ok()?.parent();
        }
        None
    }

    /// Where a drop at `at` (window logical px) over `entity` would put
    /// `ids`, if RoboCAD would take it (`move_plan`): into a group (in front
    /// of it in the top band of its row), in front of any other row, or the
    /// top level under the rows.
    fn target(&self, doc: &CadDocument, ids: &[String], entity: Entity, at: Vec2) -> Option<DropTarget> {
        let target = match self.find(entity)? {
            Hit::End => DropTarget::TopLevel,
            Hit::Row(e, row) => {
                let before = row.group && self.layout.get(e).is_ok_and(|(node, t)| {
                    let r = crate::cad::surfaces::rect_of(node, t);
                    at.y < r.min.y + r.height() * BEFORE_BAND
                });
                if row.group && !before { DropTarget::Into(row.id.clone()) } else { DropTarget::Before(row.id.clone()) }
            }
        };
        let (parent, before) = match &target {
            DropTarget::Into(id) => (Some(id.as_str()), None),
            DropTarget::Before(id) => (None, Some(id.as_str())),
            DropTarget::TopLevel => (None, None),
        };
        move_plan(doc, ids, parent, before).ok().map(|_| target)
    }
}

/// A row drag the pointer started.
struct Started {
    /// The pressed row's entity (picking's drag events name it, also after a rebuild despawned it).
    entity: Entity,
    id: String,
    /// Past the slop: the preview is shown and a drop moves.
    armed: bool,
    began: u64,
}

/// The rows' gesture state across frames.
#[derive(Default)]
pub(super) struct Gesture {
    /// The last left press on a row (the double-click).
    last: Option<(String, Instant)>,
    /// A plain press on a selected row: it is selected alone on release
    /// without a drag (Qt's ExtendedSelection, so a drag moves the whole selection).
    pending: Option<(Entity, String)>,
    drag: Option<Started>,
}

/// Shift and Ctrl/Cmd held.
fn modifiers(keys: Option<&ButtonInput<KeyCode>>) -> (bool, bool) {
    let Some(k) = keys else { return (false, false) };
    (k.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]), k.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]))
}

fn select(id: &str, extend: bool, toggle: bool) -> CadAction {
    let mut a = TreeArgs::on(TreeOp::Select, id);
    (a.extend, a.toggle) = (extend.then_some(true), toggle.then_some(true));
    a.action()
}

/// Input: row presses (left: select, Shift range, Ctrl/Cmd toggle, a
/// double-click renames; right: the context menu), and row drags (the
/// preview, then one `move` on the drop). A rebuild of the rows during a
/// drag is harmless: the gesture holds node ids, not entities, except the
/// pressed row's, which picking keeps naming after it is despawned.
#[allow(clippy::too_many_arguments)]
pub(super) fn rows(
    mut msgs: PointerMsgs,
    doc: Option<ResMut<CadDocument>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    look: RowLookup,
    selection: CadSelection,
    mut out: MessageWriter<Act<CadAction>>,
    mut gesture: Local<Gesture>,
) {
    // One borrow of the local, so its fields borrow apart.
    let g = &mut *gesture;
    let Some(mut doc) = doc else {
        msgs.clear();
        *g = Gesture::default();
        return;
    };
    let (shift, ctrl) = modifiers(keys.as_deref());
    // A left press while the context menu is open only dismisses it
    // (`popup::input` closes it this frame; the handler applies that later).
    let menu_open = super::menu_open(&doc);
    for ev in msgs.press.read() {
        match ev.button {
            PointerButton::Primary => {
                g.pending = None;
                if menu_open {
                    continue;
                }
                let Ok(row) = look.rows.get(ev.entity) else { continue };
                let now = Instant::now();
                let double = !shift && !ctrl && g.last.as_ref().is_some_and(|(id, t)| *id == row.id && now.duration_since(*t) <= DOUBLE_CLICK);
                if double {
                    g.last = None;
                    out.write(Act::ui(crate::cad::activation::guard(&doc, TreeArgs::on(TreeOp::BeginRename, &row.id).action())));
                    continue;
                }
                g.last = Some((row.id.clone(), now));
                let selected = selection.items().iter().any(|i| i.0 == row.id);
                if selected && !shift && !ctrl {
                    g.pending = Some((ev.entity, row.id.clone()));
                } else {
                    out.write(Act::ui(crate::cad::activation::guard(&doc, select(&row.id, shift, ctrl))));
                }
            }
            PointerButton::Secondary => {
                let at = ev.pointer_location.position;
                if let Ok(row) = look.rows.get(ev.entity) {
                    let mut a = TreeArgs::on(TreeOp::Menu, &row.id);
                    (a.open, a.at) = (Some(true), Some([at.x, at.y]));
                    out.write(Act::ui(crate::cad::activation::guard(&doc, a.action())));
                } else if look.ends.contains(ev.entity) {
                    out.write(Act::ui(crate::cad::activation::guard(&doc, end_menu(at))));
                }
            }
            PointerButton::Middle => {}
        }
    }
    for ev in msgs.click.read() {
        if ev.button == PointerButton::Primary
            && let Some((_, id)) = g.pending.take_if(|(e, _)| *e == ev.entity)
        {
            out.write(Act::ui(crate::cad::activation::guard(&doc, select(&id, false, false))));
        }
    }
    for ev in msgs.start.read() {
        if ev.button != PointerButton::Primary {
            continue;
        }
        if let Ok(row) = look.rows.get(ev.entity) {
            g.drag = Some(Started { entity: ev.entity, id: row.id.clone(), armed: false, began: doc.shown_revision() });
        }
    }
    // Past the slop: the nodes it moves are the selection when the pressed
    // row is selected (read now, after the press's selection applied), else that row.
    for ev in msgs.drag.read() {
        let Some(d) = g.drag.as_mut().filter(|d| d.entity == ev.entity && !d.armed) else { continue };
        if ev.distance.length() < DRAG_SLOP {
            continue;
        }
        d.armed = true;
        g.pending = None;
        let items = selection.items();
        let ids = if items.iter().any(|i| i.0 == d.id) { items.nodes() } else { vec![d.id.clone()] };
        doc.tree.drag = Some(TreeDrag { ids, target: None, began: d.began });
        doc.touch();
    }
    // The target: the last entity entered or moved over this frame; a
    // leave from a live row with nothing entered clears it (a leave from a
    // rebuilt row's despawned entity resolves to nothing and is ignored).
    let hover = msgs.enter.read().map(|e| (e.entity, e.pointer_location.position)).chain(msgs.over.read().map(|e| (e.entity, e.pointer_location.position))).last();
    // Every message is read (`any` would leave the rest for the next frame).
    let left = msgs.leave.read().filter(|e| look.find(e.entity).is_some()).count() > 0;
    let armed = g.drag.as_ref().is_some_and(|d| d.armed);
    if armed && let Some(ids) = doc.tree.drag.as_ref().map(|d| d.ids.clone()) {
        let next = match hover {
            Some((entity, at)) => look.target(&doc, &ids, entity, at),
            None if left => None,
            None => doc.tree.drag.as_ref().and_then(|d| d.target.clone()),
        };
        if doc.tree.drag.as_ref().is_some_and(|d| d.target != next) {
            if let Some(drag) = doc.tree.drag.as_mut() {
                drag.target = next;
            }
            doc.touch();
        }
    }
    // The drop: one move, as RoboCAD's `_drop` builds it.
    let mut dropped = false;
    for ev in msgs.drop.read() {
        if dropped || ev.button != PointerButton::Primary || !armed {
            continue;
        }
        let Some((ids, began)) = doc.tree.drag.as_ref().map(|d| (d.ids.clone(), d.began)) else { continue };
        let Some(target) = look.target(&doc, &ids, ev.entity, ev.pointer_location.position) else { continue };
        let mut a = TreeArgs::of(TreeOp::Move);
        (a.ids, a.revision) = (Some(ids), Some(began));
        match target {
            DropTarget::Into(id) => a.parent = Some(id),
            DropTarget::Before(id) => a.before = Some(id),
            DropTarget::TopLevel => {}
        }
        out.write(Act::ui(crate::cad::activation::guard(&doc, a.action())));
        dropped = true;
    }
    // The drag ends with its `DragEnd`, or once the left button is up
    // without one (a pointer cancel clears picking's state without a
    // `DragEnd`; the drop, if any, was read above).
    let up = mouse.is_none_or(|m| !m.pressed(MouseButton::Left));
    let ended = msgs.end.read().filter(|ev| g.drag.as_ref().is_some_and(|d| d.entity == ev.entity)).count() > 0;
    if g.drag.is_some() && (ended || up) {
        g.drag = None;
        if doc.tree.drag.is_some() {
            doc.tree.drag = None;
            doc.touch();
        }
    }
}

/// A part of the "Organize components" dialog (`Kit::form`'s hits).
#[derive(Component, Clone, Copy, Debug)]
pub(in crate::cad) struct DialogPart(pub FormHit);

/// Input: the outliner's fields. The search writes `search` as typed; a
/// double-click's `begin_rename` gives the rename field the keyboard with
/// the name selected, Enter writes `rename` (the field stays open on a
/// refusal), Escape or a press elsewhere ends it without renaming; the
/// dialog's field writes `group` on Enter or OK and closes it on Escape or
/// Cancel, and holds the keyboard while the dialog is open (modal).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn fields(
    doc: Option<ResMut<CadDocument>>,
    presses: Query<&TreeField, With<crate::ui_kit::activation::Activated>>,
    parts: Query<(&DialogPart, Option<&crate::builder::ui_api::Enabled>), With<crate::ui_kit::activation::Activated>>,
    mut msgs: MessageReader<FieldMsg>,
    mut text: TextFocus,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let Some(mut doc) = doc else {
        msgs.clear();
        return;
    };
    if [SEARCH, RENAME, GROUP_NAME].into_iter().any(|id| text.suspended(id)) {
        msgs.clear();
        return;
    }
    let mut touched = false;
    // Cancel this frame: the dialog closes when the handler runs, so its field does not take the keyboard again.
    let mut closing = false;
    for m in msgs.read() {
        match (m.field, &m.event) {
            (SEARCH, FieldEvent::Changed(d)) => {
                if doc.tree.search != d.text {
                    let mut a = TreeArgs::of(TreeOp::Search);
                    a.text = Some(d.text.clone());
                    out.write(Act::ui(crate::cad::activation::guard(&doc, a.action())));
                }
            }
            (SEARCH, FieldEvent::Submit(_)) => text.blur(SEARCH),
            (RENAME, FieldEvent::Changed(d)) => {
                if let Some(r) = doc.tree.rename.as_ref()
                    && (r.draft != d.text || r.selected != d.select_all)
                    && let Some(r) = doc.tree.rename.as_mut()
                {
                    r.draft = d.text.clone();
                    r.selected = d.select_all;
                    touched = true;
                }
            }
            (RENAME, FieldEvent::Submit(typed)) => {
                if let Some(r) = &doc.tree.rename {
                    let mut a = TreeArgs::on(TreeOp::Rename, &r.id);
                    (a.text, a.revision) = (Some(typed.clone()), Some(r.began));
                    out.write(Act::ui(crate::cad::activation::guard(&doc, a.action())));
                }
            }
            (RENAME, FieldEvent::Cancel) => {
                if doc.tree.rename.is_some() {
                    doc.tree.rename = None;
                    touched = true;
                }
            }
            (GROUP_NAME, FieldEvent::Changed(d)) => {
                if let Some(dialog) = doc.tree.dialog.as_ref()
                    && (dialog.draft != d.text || dialog.error.is_some())
                    && let Some(dialog) = doc.tree.dialog.as_mut()
                {
                    dialog.draft = d.text.clone();
                    dialog.error = None;
                    touched = true;
                }
            }
            (GROUP_NAME, FieldEvent::Submit(typed)) => {
                if let Some(dialog) = &doc.tree.dialog {
                    out.write(Act::ui(crate::cad::activation::guard(&doc, ok(typed, dialog.began))));
                }
            }
            (GROUP_NAME, FieldEvent::Cancel) => {
                out.write(Act::ui(crate::cad::activation::guard(&doc, close_dialog())));
                closing = true;
            }
            _ => {}
        }
    }
    for field in &presses {
        match field {
            TreeField::Search if !text.focused(SEARCH) => {
                text.focus_draft(SEARCH, TextDraft::new(doc.tree.search.clone(), false));
            }
            TreeField::Rename if !text.focused(RENAME) => {
                if let Some(r) = &doc.tree.rename {
                    text.focus_draft(RENAME, TextDraft::new(r.draft.clone(), false));
                }
            }
            _ => {}
        }
    }
    for (part, enabled) in &parts {
        match part.0 {
            FormHit::Field(_) if !text.focused(GROUP_NAME) => {
                if let Some(d) = &doc.tree.dialog {
                    text.focus_draft(GROUP_NAME, TextDraft::new(d.draft.clone(), false));
                }
            }
            FormHit::Ok if enabled.is_none_or(|e| e.0) => {
                if let Some(d) = &doc.tree.dialog {
                    out.write(Act::ui(crate::cad::activation::guard(&doc, ok(&d.draft, d.began))));
                }
            }
            FormHit::Cancel => {
                out.write(Act::ui(crate::cad::activation::guard(&doc, close_dialog())));
                closing = true;
            }
            _ => {}
        }
    }
    // A field the handler (or Ctrl+F) asked for takes the keyboard.
    if let Some(claim) = doc.tree.claim {
        doc.tree.claim = None;
        match claim {
            Claim::Search => {
                text.focus_draft(SEARCH, TextDraft::new(doc.tree.search.clone(), true));
            }
            Claim::Rename => {
                if let Some(r) = &doc.tree.rename {
                    text.focus_draft(RENAME, TextDraft::new(r.draft.clone(), true));
                }
            }
            Claim::Dialog => {
                if let Some(d) = &doc.tree.dialog {
                    text.focus_draft(GROUP_NAME, TextDraft::new(d.draft.clone(), true));
                }
            }
        }
    }
    // The kit's focus is the record: a rename whose field lost the keyboard
    // (Escape, a press elsewhere, another field) ends without renaming; a
    // field whose rename or dialog closed gives the keyboard up.
    if doc.tree.rename.is_some() && !text.focused(RENAME) {
        doc.tree.rename = None;
        touched = true;
    }
    if doc.tree.rename.is_none() {
        text.blur(RENAME);
    }
    match &doc.tree.dialog {
        // The modal dialog owns the keyboard (its field is sticky).
        Some(d) if !text.typing() && !text.ordinary_focused() && !closing => {
            text.focus_draft(GROUP_NAME, TextDraft::new(d.draft.clone(), false));
        }
        Some(_) => {}
        None => text.blur(GROUP_NAME),
    }
    let focused = text.focused(SEARCH);
    if doc.tree.search_focused != focused {
        doc.tree.search_focused = focused;
        touched = true;
    }
    if touched {
        // The panels refresh on the document's revision.
        doc.touch();
    }
}

/// The dialog's OK with `name`.
fn ok(name: &str, began: u64) -> CadAction {
    let mut a = TreeArgs::of(TreeOp::Group);
    (a.name, a.revision) = (Some(name.to_string()), Some(began));
    a.action()
}

fn close_dialog() -> CadAction {
    let mut a = TreeArgs::of(TreeOp::GroupDialog);
    a.open = Some(false);
    a.action()
}
