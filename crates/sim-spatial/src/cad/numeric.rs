//! CAD mode's tool bar and numeric entry (RoboCAD's `NumericBar`,
//! ui/widgets.py:142-216, its viewport tool label and hint, and the tool
//! buttons), on the UI kit, absolutely placed at the bottom of the 3D view.
//!
//! - **Head**: RoboCAD's "Tool  ·  Mode" label, the tool's hint, and the
//!   tool strip: one kit segment per `cad:tool:<tool>` control
//!   (`panel::controls`) carrying its action ([`CadButton`]; `panel::buttons`
//!   writes it, as every CAD button).
//! - **Body**: the live readout (Δ, angle, scale, push/pull distance, the
//!   measured value, the snap kind and point), then the active tool's
//!   fields (`transform::fields`): each with its draft, and under it every
//!   keystroke's evaluation (`sim_runtime::units::evaluate`): "= 20.3 mm",
//!   or the evaluator's error naming the token in the danger colour with a
//!   red border (RoboCAD's red border).
//! - **Keys** ([`entry`], Input): Tab opens the entry on the first field
//!   (RoboCAD routes Tab itself, app.py:483-486) and cycles fields while
//!   open; a press on a field focuses it; typing replaces the text first
//!   (RoboCAD's `selectAll`), then edits it; Enter writes
//!   `CadNumeric {values}` when every field evaluates (else the entry stays
//!   open showing the error); its commit carries the shown revision of
//!   the moment the entry gained focus (`Numeric::began`), so it is refused
//!   by name when RoboCAD's document changed while the values were typed; Escape cancels the entry; a press elsewhere
//!   ends it. `CadInputFocus` is true while a field is focused and in the
//!   frame the entry ends, so CAD keys are not typed into twice. The
//!   inspector's name field and this bar never hold the keyboard together:
//!   focusing a field drops the name draft, and starting a name draft ends
//!   the entry.
//! - **Fields** ([`sync`], SimSync): rebuilt from the tool and selection;
//!   a new set resets the drafts to RoboCAD's opening values (move 0 mm,
//!   rotate 0°, scale 1, distance 0 mm, a live dimension's current value).
use super::actions::CadAction;
use super::document::{CadDocument, CadInputFocus, CadTool};
use super::mesh::CadMeshes;
use super::panel::{CadButton, NameDraft};
use super::topology::CadTopology;
use super::transform::{DimensionEntry, Field, FieldCommit, fields, hint, keep_entry, mode_label};
use sim_runtime::cad_client::SelectionItem;
use crate::app::actions::Act;
use crate::ui_kit::{BORDER, DANGER, FAINT, Kit, LEFT_WIDTH, RIGHT_WIDTH, STATUSBAR, SUBTLE, SURFACE, TEXT, UiFonts, VALUE, size};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;

/// RoboCAD's `numeric.hint` (ui/strings.py:17).
pub const ENTRY_HINT: &str = "Tab: type an exact value  •  Enter: confirm  •  Esc: cancel";

/// The numeric bar's state (`ToolState::numeric`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Numeric {
    /// The active tool's fields (`transform::fields`).
    pub fields: Vec<Field>,
    /// What the fields were built from: a new key resets the drafts.
    pub key: String,
    /// Each field's text as typed.
    pub texts: Vec<String>,
    /// Each text's evaluation (the value, or the evaluator's error).
    pub results: Vec<Result<f64, String>>,
    /// The field with the keyboard.
    pub focus: Option<usize>,
    /// The focused text is selected: the next character replaces it.
    pub select_all: bool,
    /// Focus the first field once the fields are rebuilt (a double-clicked dimension).
    pub focus_request: bool,
    /// The shown revision when the entry gained focus: Enter's commit is
    /// refused if RoboCAD's document changed since (the values were typed
    /// against that geometry). Taken by the commit; cleared by Escape.
    pub began: Option<u64>,
}

/// The bar's root.
#[derive(Component)]
struct BarRoot;

/// The bar's two parts, each rebuilt alone when what it shows changes.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum BarPart {
    Head,
    Body,
}

/// A numeric field: a press focuses field `.0`.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct FieldButton(pub usize);

/// OnEnter(Cad): the bar (its parts filled by [`refresh`]).
pub(super) fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(LEFT_WIDTH + 8.0),
                right: Val::Px(RIGHT_WIDTH + 8.0),
                bottom: Val::Px(STATUSBAR + 8.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(8.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER),
            FocusPolicy::Block,
            AccessibleLabel::new("CAD tools and numeric entry"),
            BarRoot,
        ))
        .with_children(|bar| {
            bar.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() }, BarPart::Head));
            bar.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() }, BarPart::Body));
        });
}

/// What the fields are computed from: generation, tool, selection, the
/// double-clicked entry, the topology's and meshes' epochs, the shown
/// revision, and whether a catalogue form is open (the bar then yields:
/// `transform::fields` is empty).
type FieldsKey = (u64, CadTool, Vec<SelectionItem>, Option<DimensionEntry>, u64, u64, u64, bool);

/// SimSync: the fields follow the tool and the selection (see the module
/// doc). They are recomputed only when what they come from changes (live
/// dimensions walk the selected edges' tessellation: not every frame).
pub(super) fn sync(doc: Option<ResMut<CadDocument>>, topology: Option<Res<CadTopology>>, meshes: Option<Res<CadMeshes>>, mut cache: Local<Option<(FieldsKey, Vec<Field>, String)>>) {
    let Some(mut doc) = doc else { return };
    if doc.tool_state.dimension.is_some() {
        keep_entry(&mut doc);
    }
    let inputs: FieldsKey = (doc.generation, doc.tool, doc.selection.clone(), doc.tool_state.dimension.clone(), topology.as_ref().map_or(0, |t| t.epoch), meshes.as_ref().map_or(0, |m| m.epoch), doc.shown_revision(), doc.ops.form.is_some());
    if cache.as_ref().is_none_or(|(k, ..)| *k != inputs) {
        let list = fields(&doc, topology.as_deref(), meshes.as_deref());
        let key = format!("{}|{list:?}", doc.tool.name());
        *cache = Some((inputs, list, key));
    }
    let Some((_, list, key)) = cache.as_ref() else { return };
    let request = doc.tool_state.numeric.focus_request;
    if doc.tool_state.numeric.key == *key && !request {
        return;
    }
    let (list, key) = (list.clone(), key.clone());
    let shown = doc.shown_revision();
    let s = &mut doc.tool_state.numeric;
    if s.key != key {
        s.texts = list.iter().map(Field::text).collect();
        s.results = list.iter().zip(&s.texts).map(|(f, t)| f.kind.evaluate(t)).collect();
        s.fields = list;
        s.key = key;
        s.focus = None;
        s.select_all = false;
    }
    if request {
        s.focus_request = false;
        if !s.fields.is_empty() {
            s.focus = Some(0);
            s.select_all = true;
            s.began = Some(shown);
        }
    }
}

/// Type `text` into field `i` (replacing a selected text).
fn type_text(s: &mut Numeric, i: usize, text: &str) {
    if s.select_all {
        s.texts[i].clear();
        s.select_all = false;
    }
    s.texts[i].push_str(text);
    reevaluate(s, i);
}

fn reevaluate(s: &mut Numeric, i: usize) {
    let r = s.fields[i].kind.evaluate(&s.texts[i]);
    if let Some(slot) = s.results.get_mut(i) {
        *slot = r;
    }
}

/// Input: the numeric entry's keys and field presses (see the module doc).
#[allow(clippy::too_many_arguments)]
pub(super) fn entry(
    doc: Option<ResMut<CadDocument>>,
    presses: Query<(&Interaction, &FieldButton), Changed<Interaction>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    mut events: MessageReader<KeyboardInput>,
    focus: Option<ResMut<CadInputFocus>>,
    draft: Option<ResMut<NameDraft>>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let Some(mut doc) = doc else {
        events.clear();
        return;
    };
    // The name field holds the keyboard (`panel::name_entry` ran first this frame).
    let others = focus.as_ref().is_some_and(|f| f.0);
    let naming = draft.as_ref().is_some_and(|d| d.editing.is_some());
    let n = doc.tool_state.numeric.fields.len().min(doc.tool_state.numeric.texts.len());
    let shown = doc.shown_revision();
    let mut started = false;
    let mut ended = false;
    for (interaction, button) in &presses {
        if *interaction == Interaction::Pressed && button.0 < n {
            let s = &mut doc.tool_state.numeric;
            if s.focus.is_none() {
                s.began = Some(shown);
            }
            s.focus = Some(button.0);
            s.select_all = true;
            started = true;
        }
    }
    let focused = doc.tool_state.numeric.focus.is_some();
    if focused && !started && (naming || buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left))) {
        // A press elsewhere, or a name draft that just started, ends the entry.
        doc.tool_state.numeric.focus = None;
        doc.tool_state.numeric.began = None;
        ended = true;
    }
    if started
        && naming
        && let Some(mut draft) = draft
    {
        draft.editing = None;
        draft.refusal = None;
    }
    let tab = keys.as_ref().is_some_and(|k| k.just_pressed(KeyCode::Tab));
    if !focused && !started && !others && n > 0 && tab {
        let s = &mut doc.tool_state.numeric;
        s.focus = Some(0);
        s.select_all = true;
        s.began = Some(shown);
        started = true;
    }
    if started || doc.tool_state.numeric.focus.is_none() {
        // Keys pressed before the field took the keyboard are not its text.
        events.clear();
    } else {
        let chord = keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]));
        let typed: Vec<KeyboardInput> = events.read().filter(|e| e.state == ButtonState::Pressed).cloned().collect();
        for e in typed {
            let s = &mut doc.tool_state.numeric;
            let Some(i) = s.focus.filter(|i| *i < n) else {
                s.focus = None;
                ended = true;
                break;
            };
            match &e.logical_key {
                Key::Enter => {
                    s.results = s.fields.iter().zip(&s.texts).map(|(f, t)| f.kind.evaluate(t)).collect();
                    // RoboCAD's `values()`: an error keeps the entry open.
                    if s.results.iter().all(Result::is_ok) {
                        out.write(Act::ui(CadAction::CadNumeric { values: s.texts.clone() }));
                        s.focus = None;
                        // The fields reopen with their opening values (RoboCAD re-places the gizmo).
                        s.key.clear();
                        ended = true;
                    }
                    break;
                }
                Key::Escape => {
                    s.focus = None;
                    s.began = None;
                    s.key.clear();
                    ended = true;
                    break;
                }
                Key::Tab => {
                    s.focus = Some((i + 1) % n);
                    s.select_all = true;
                }
                Key::Backspace => {
                    if s.select_all {
                        s.texts[i].clear();
                        s.select_all = false;
                    } else {
                        s.texts[i].pop();
                    }
                    reevaluate(s, i);
                }
                Key::Space if !chord => type_text(s, i, " "),
                Key::Character(c) if !chord && !c.chars().any(char::is_control) => type_text(s, i, c.as_str()),
                _ => {}
            }
        }
    }
    if let Some(mut focus) = focus
        && (doc.tool_state.numeric.focus.is_some() || ended)
        && !focus.0
    {
        focus.0 = true;
    }
}

/// The head's content key.
fn head_key(doc: &CadDocument) -> String {
    format!("{:?}", (doc.tool, doc.select_mode))
}

/// The body's content key.
fn body_key(doc: &CadDocument) -> String {
    let s = &doc.tool_state;
    let n = &s.numeric;
    format!("{:?}", (&n.key, &n.texts, &n.results, n.focus, n.select_all, &s.readout, s.snap.as_ref().map(|x| x.readout()), s.measure.last.as_ref().map(|m| &m.label), doc.tool))
}

/// Present: rebuild the bar's parts whose content changed.
pub(super) fn refresh(mut commands: Commands, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, parts: Query<(Entity, &BarPart)>, mut shown: Local<Vec<(Entity, String)>>) {
    let Some(doc) = doc else { return };
    shown.retain(|(e, _)| parts.contains(*e));
    let k = Kit::new(&fonts);
    for (entity, part) in &parts {
        let key = match part {
            BarPart::Head => head_key(&doc),
            BarPart::Body => body_key(&doc),
        };
        if shown.iter().any(|(e, s)| *e == entity && *s == key) {
            continue;
        }
        shown.retain(|(e, _)| *e != entity);
        shown.push((entity, key));
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).with_children(|p| match part {
            BarPart::Head => head(p, &k, &doc),
            BarPart::Body => body(p, &k, &doc),
        });
    }
}

/// The 3D view's navigation, as RoboCAD's viewport footer lists its own.
const NAVIGATION: &str = "Right-drag orbit · Shift+right-drag or middle-drag pan · Wheel zoom · Home fit";

/// The tool label, the hint, the navigation line and the tool strip.
fn head(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    p.spawn(Node { column_gap: Val::Px(10.0), align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|row| {
        row.spawn(k.text(mode_label(doc), size::SMALL, TEXT, 2));
        row.spawn((k.text(hint(doc.tool), size::CAPTION, SUBTLE, 0), Node { flex_shrink: 1.0, min_width: Val::Px(0.0), ..default() }));        // RoboCAD's viewport footer, with this view's own navigation (scene.rs orbit; Home fits).
        row.spawn((k.text(NAVIGATION, size::CAPTION, FAINT, 0), Node { flex_shrink: 1.0, min_width: Val::Px(0.0), ..default() }));
    });
    p.spawn(k.segments()).with_children(|strip| {
        for c in super::panel::controls(doc).into_iter().filter(|c| c.id.starts_with("cad:tool:")) {
            let CadAction::CadTool { tool } = c.action else { continue };
            strip.spawn(k.segment(tool.label(), CadButton(c.action.clone()), tool == doc.tool, c.ready.is_ok()));
        }
    });
}

/// The readouts and the fields.
fn body(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    let s = &doc.tool_state;
    if let Some(readout) = &s.readout {
        p.spawn(k.mono(readout.clone(), size::SMALL, VALUE));
    }
    if doc.tool == CadTool::Measure
        && let Some(snap) = &s.snap
    {
        p.spawn(k.mono(snap.readout(), size::CAPTION, SUBTLE));
    }
    let n = &s.numeric;
    if n.fields.is_empty() {
        return;
    }
    p.spawn(Node { column_gap: Val::Px(12.0), row_gap: Val::Px(6.0), align_items: AlignItems::FlexStart, flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|row| {
        for (i, field) in n.fields.iter().enumerate() {
            let text = n.texts.get(i).map_or("", String::as_str);
            let result = n.results.get(i);
            row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), ..default() }).with_children(|cell| {
                cell.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|line| {
                    line.spawn(k.text(field.name.clone(), size::CAPTION, SUBTLE, 1));
                    let mut input = line.spawn(k.input(text, &field.name, FieldButton(i), n.focus == Some(i)));
                    if result.is_some_and(Result::is_err) {
                        input.insert(BorderColor::all(DANGER));
                    }
                });
                match result {
                    Some(Ok(v)) => {
                        let shown = if let FieldCommit::ReadOnly(_) = field.commit { format!("= {} (read-only)", field.kind.show(*v)) } else { format!("= {}", field.kind.show(*v)) };
                        cell.spawn(k.text(shown, size::CAPTION, SUBTLE, 0));
                    }
                    Some(Err(e)) => {
                        cell.spawn(k.text(e.clone(), size::CAPTION, DANGER, 0));
                    }
                    None => {}
                }
            });
        }
    });
    p.spawn(k.text(ENTRY_HINT, size::CAPTION, FAINT, 0));
}
