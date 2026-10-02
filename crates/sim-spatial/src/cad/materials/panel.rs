//! The Materials section of the right dock and its dialog.
//!
//! - **Section** ([`draw`], `Part::Materials`): "Search materials…", the
//!   rows (the current one highlighted), "Apply to selection", "New…",
//!   "Material properties…". The buttons take their action and enabled
//!   state from `controls_of` (what `system_ui` lists). A press on a row
//!   writes `select`; a second press on it within 400 ms (Qt's
//!   double-click interval) writes `apply`.
//! - **Dialog** ([`draw_form`]): RoboCAD's dialogs are modal, so the kit
//!   form sits centred over a dimmed backdrop that takes every click (and
//!   the switcher strip's), in a scroll area for short windows (the wheel
//!   scrolls it, [`scroll_form`]).
//! - **Typing** ([`input`]) on the kit's one text field (`ui_kit::text`):
//!   the search field ([`SEARCH`]) and the dialog's fields ([`FORM`], one kit
//!   field for every row; `MaterialsState::focus` is the row, mirrored
//!   with the selection from the kit's focus and draft). A press on a field
//!   gives it the keyboard (a dialog field's text selected); in the search
//!   field each key filters at once, Enter or Escape ends it, a press
//!   elsewhere too; in the dialog Tab moves to the next field, Enter is OK
//!   and Escape Cancel. The dialog is modal and owns the keyboard while
//!   open: its field is `sticky` (a press on the backdrop keeps it), a
//!   dialog the handler opened takes the keyboard (`claimed`), and while it
//!   is open and no field has the keyboard its field takes it again.
use super::form::{self, FormKind};
use super::{Focus, MaterialsArgs, MaterialsOp, controls_of, list, matches, row_label};
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::CadButton;
use crate::ui_kit::form::{FormHit, FormRow};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFocus};
use crate::ui_kit::{ACCENT_BG, DANGER, Kit, Look, TEXT, Tint, UiFonts, WHEEL_LINE, size, wheel_delta, wrap};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use sim_runtime::cad_client::SelectionItem;
use std::time::{Duration, Instant};

/// "Search materials…".
pub(in crate::cad) const SEARCH: FieldId = FieldId("cad.materials.search");
/// The open dialog's fields (the row is `MaterialsState::focus`).
pub(in crate::cad) const FORM: FieldId = FieldId("cad.materials.form");

/// The search field: a press places the caret after the text.
pub(super) fn search_field() -> TextField {
    TextField::new("Search materials").placeholder("Search materials…")
}

/// The dialog's field: a focused row's text is selected; sticky (the modal
/// dialog keeps the keyboard on a press on its backdrop).
pub(super) fn form_field() -> TextField {
    TextField::new("Material property").select_on_focus().sticky()
}

/// Qt's default double-click interval.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// The dialog's width (px).
const WIDTH: f32 = 420.0;

/// A field of the panel or its dialog: a press gives it the keyboard.
#[derive(Component, Clone, Copy, Debug)]
pub(in crate::cad) struct MaterialsInput(pub Focus);

/// A material row: a press makes it current, a second one applies it.
#[derive(Component, Clone, Debug)]
pub(in crate::cad) struct MaterialRow(pub String);

/// A clickable part of the dialog (`Kit::form`'s hits).
#[derive(Component, Clone, Copy, Debug)]
pub(in crate::cad) struct FormPart(pub FormHit);

/// The dialog's root and its scroll area.
#[derive(Component)]
pub(in crate::cad) struct FormRoot;
#[derive(Component)]
pub(in crate::cad) struct FormScroll;

/// The controls (`controls_of`): (id, label, action, ready).
type Controls = [(String, String, CadAction, Result<(), String>)];

fn button(p: &mut ChildSpawnerCommands, k: &Kit, controls: &Controls, id: &str, look: Look) {
    if let Some((_, label, action, ready)) = controls.iter().find(|c| c.0 == id) {
        p.spawn(k.button(label, CadButton(action.clone()), look, ready.is_ok()));
    }
}

/// What the section shows, for its part key.
pub(in crate::cad) fn key(doc: &CadDocument, selection: &[SelectionItem]) -> String {
    let st = &doc.materials;
    let ready: Vec<(String, bool)> = controls_of(doc, selection).into_iter().map(|c| (c.0, c.3.is_ok())).collect();
    let typing = st.focus == Some(Focus::Search);
    format!("{:?}", (doc.doc.as_ref().map(|d| &d.materials), &st.search, &st.current, typing, typing && st.select_all, ready))
}

/// The Materials section (see the module doc).
pub(in crate::cad) fn draw(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem]) {
    let st = &doc.materials;
    let controls = controls_of(doc, selection);
    p.spawn(k.section("Materials"));
    let typing = st.focus == Some(Focus::Search);
    p.spawn(k.input_selectable(&st.search, "Search materials…", MaterialsInput(Focus::Search), typing, typing && st.select_all));
    let all = list(doc);
    if all.is_empty() {
        p.spawn(k.caption("RoboCAD listed no materials."));
    }
    let shown: Vec<_> = all.iter().filter(|m| matches(m, &st.search)).collect();
    if shown.is_empty() && !all.is_empty() {
        p.spawn(k.caption("No material matches the search."));
    }
    for m in shown {
        let selected = st.current.as_deref() == Some(m.id.as_str());
        let colour = Color::srgb(m.color[0] as f32, m.color[1] as f32, m.color[2] as f32);
        let label = row_label(m);
        // "■" in the material's colour, then RoboCAD's text.
        let text = label.trim_start_matches('■').trim_start().to_string();
        p.spawn((
            Button,
            crate::ui_kit::activation::Ordinary,
            MaterialRow(m.id.clone()),
            // A press on a row keeps the search typing, as before.
            crate::ui_kit::text::KitInput,
            Tint::selectable(selected),
            AccessibleLabel::new(label),
            Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), column_gap: Val::Px(8.0), align_items: AlignItems::Center, flex_shrink: 0.0, ..default() },
            BackgroundColor(if selected { ACCENT_BG } else { Color::NONE }),
            children![k.text("■", size::ITEM, colour, 0), k.text(text, size::BODY, TEXT, 0)],
        ));
    }
    p.spawn(wrap()).with_children(|r| {
        button(r, k, &controls, "cad:materials:apply", Look::Primary);
        button(r, k, &controls, "cad:materials:new", Look::Secondary);
        button(r, k, &controls, "cad:materials:properties", Look::Secondary);
    });
    if let Some(why) = controls.iter().find(|c| c.0 == "cad:materials:apply").and_then(|c| c.3.as_ref().err()) {
        p.spawn(k.note(why.clone()));
    }
    p.spawn(k.caption("Double-click a material to apply it to the selection."));
}

/// Write `cad_materials {op, material}` as a click.
fn write(doc: &CadDocument, out: &mut MessageWriter<Act<CadAction>>, op: MaterialsOp, material: Option<&str>) {
    out.write(Act::ui(crate::cad::activation::guard(doc, MaterialsArgs::of(op, material))));
}

/// Dialog row `i`'s text.
fn row_text(doc: &CadDocument, i: usize) -> String {
    doc.materials.form.as_ref().and_then(|form| form.texts.get(i).cloned()).unwrap_or_default()
}

/// Give `f` the keyboard: the search text with the caret after it, or a
/// dialog row's text selected. Returns the selection.
fn focus(text: &mut TextFocus, doc: &CadDocument, f: Focus) -> bool {
    match f {
        Focus::Search => {
            text.focus_draft(SEARCH, TextDraft::new(doc.materials.search.clone(), false));
            false
        }
        Focus::Field(i) => {
            text.focus_draft(FORM, TextDraft::new(row_text(doc, i), true));
            true
        }
    }
}

/// Input: the panel's and the dialog's clicks and field messages (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::cad) fn input(
    doc: Option<ResMut<CadDocument>>,
    fields: Query<&MaterialsInput, With<crate::ui_kit::activation::Activated>>,
    parts: Query<(&FormPart, Option<&Enabled>), With<crate::ui_kit::activation::Activated>>,
    rows: Query<&MaterialRow, With<crate::ui_kit::activation::Activated>>,
    pointer_rows: Query<(&Interaction, &MaterialRow), Changed<Interaction>>,
    mut msgs: MessageReader<FieldMsg>,
    mut text: TextFocus,
    mut out: MessageWriter<Act<CadAction>>,
    mut last_row: Local<Option<(String, Instant)>>,
) {
    let Some(mut doc) = doc else {
        msgs.clear();
        return;
    };
    // Higher modal takeover is not an owner blur. The kit validates the
    // suspended source/field identity before exposing this state.
    if text.suspended(FORM) || text.suspended(SEARCH) {
        msgs.clear();
        return;
    }
    let modal = doc.materials.form.is_some();
    let (before, before_all) = (doc.materials.focus, doc.materials.select_all);
    let mut at = before;
    let mut select_all = before_all;
    // A Cancel this frame: the dialog closes when the handler runs, so it does not take the keyboard again.
    let (mut edited, mut closing) = (false, false);
    for m in msgs.read() {
        let row = match at {
            Some(Focus::Field(i)) => Some(i),
            _ => None,
        };
        match (m.field, &m.event) {
            (SEARCH, FieldEvent::Changed(d)) => {
                if doc.materials.search != d.text {
                    doc.materials.search = d.text.clone();
                    edited = true;
                }
                select_all = d.select_all;
            }
            (SEARCH, FieldEvent::Submit(_)) => {
                text.blur(SEARCH);
                if at == Some(Focus::Search) {
                    at = None;
                }
            }
            (SEARCH, FieldEvent::Cancel) => {
                if at == Some(Focus::Search) {
                    at = None;
                }
            }
            (SEARCH, FieldEvent::Blur) if !text.focused(SEARCH) && at == Some(Focus::Search) => at = None,
            (FORM, FieldEvent::Changed(d)) => {
                if let Some(i) = row
                    && let Some(form) = doc.materials.form.as_mut()
                    && let Some(slot) = form.texts.get_mut(i)
                {
                    *slot = d.text.clone();
                    form.error = None;
                    edited = true;
                }
                select_all = d.select_all;
            }
            (FORM, FieldEvent::Submit(typed)) => {
                if let Some(i) = row
                    && let Some(form) = doc.materials.form.as_mut()
                    && let Some(slot) = form.texts.get_mut(i)
                    && *slot != *typed
                {
                    *slot = typed.clone();
                    edited = true;
                }
                // OK: a refusal stays in the form, the field keeps the keyboard.
                if modal {
                    write(&doc, &mut out, MaterialsOp::FormSubmit, None);
                }
            }
            (FORM, FieldEvent::Cancel) => {
                if modal {
                    write(&doc, &mut out, MaterialsOp::FormCancel, None);
                }
                at = None;
                closing = true;
            }
            (FORM, FieldEvent::Tab { .. }) => {
                if let Some(i) = row {
                    let n = doc.materials.form.as_ref().map_or(1, |form| form.fields.len().max(1));
                    let next = Focus::Field((i + 1) % n);
                    at = Some(next);
                    select_all = focus(&mut text, &doc, next);
                }
            }
            (FORM, FieldEvent::Blur) if !text.focused(FORM) && row.is_some() => at = None,
            _ => {}
        }
    }
    for f in &fields {
        if at != Some(f.0) {
            at = Some(f.0);
            select_all = focus(&mut text, &doc, f.0);
        }
    }
    for (part, enabled) in &parts {
        match part.0 {
            FormHit::Field(i) => {
                if at != Some(Focus::Field(i)) {
                    at = Some(Focus::Field(i));
                    select_all = focus(&mut text, &doc, Focus::Field(i));
                }
            }
            FormHit::Ok if enabled.is_none_or(|e| e.0) => write(&doc, &mut out, MaterialsOp::FormSubmit, None),
            FormHit::Cancel => {
                write(&doc, &mut out, MaterialsOp::FormCancel, None);
                closing = true;
            }
            _ => {}
        }
    }
    for row in &rows {
        // Rows are kit inputs so a press keeps the search typing; any other
        // non-sticky field's entry ends, as for a press elsewhere.
        if !text.focused(SEARCH) {
            text.release(false);
        }
        // Keyboard selection is ordinary activation, never a fabricated double-click.
        if !pointer_rows.iter().any(|(interaction, pointer)| *interaction == Interaction::Pressed && pointer.0 == row.0) {
            write(&doc, &mut out, MaterialsOp::Select, Some(row.0.as_str()));
            *last_row = None;
            continue;
        }
        let now = Instant::now();
        if last_row.as_ref().is_some_and(|(id, t)| *id == row.0 && now.duration_since(*t) <= DOUBLE_CLICK) {
            write(&doc, &mut out, MaterialsOp::Apply, Some(row.0.as_str()));
            *last_row = None;
        } else {
            write(&doc, &mut out, MaterialsOp::Select, Some(row.0.as_str()));
            *last_row = Some((row.0.clone(), now));
        }
    }
    // A dialog opened by the handler since (`open`) takes the keyboard.
    if doc.materials.claimed {
        doc.materials.claimed = false;
        if modal && !closing {
            let row = match at {
                Some(Focus::Field(i)) => i,
                _ => 0,
            };
            at = Some(Focus::Field(row));
            select_all = focus(&mut text, &doc, Focus::Field(row));
        }
    }
    // The kit's focus is the record (the handler's OK and Cancel close the
    // dialog and clear `focus`): the mirror and the kit's focus agree.
    if matches!(at, Some(Focus::Field(_))) && (!modal || !text.focused(FORM)) {
        at = None;
    }
    if at == Some(Focus::Search) && !text.focused(SEARCH) {
        at = None;
    }
    if !matches!(at, Some(Focus::Field(_))) {
        text.blur(FORM);
    }
    if at != Some(Focus::Search) {
        text.blur(SEARCH);
    }
    // The modal dialog owns the keyboard: with no field focused, its field takes it again.
    if modal && !closing && !text.typing() && !text.ordinary_focused() {
        let row = match before {
            Some(Focus::Field(i)) => i,
            _ => 0,
        };
        at = Some(Focus::Field(row));
        select_all = focus(&mut text, &doc, Focus::Field(row));
    }
    if at.is_none() {
        select_all = false;
    }
    if at != before || select_all != before_all {
        doc.materials.focus = at;
        doc.materials.select_all = select_all;
        edited = true;
    }
    if edited {
        // The panels and the dialog refresh on the document's revision.
        doc.touch();
    }
}

/// What the dialog shows, as a comparable text (None: closed).
fn form_key(doc: &CadDocument) -> Option<String> {
    let f = doc.materials.form.as_ref()?;
    let ids: Vec<String> = list(doc).into_iter().map(|m| m.id).collect();
    Some(format!("{:?}", (doc.generation, doc.materials.form_sequence, f, doc.materials.focus, doc.materials.select_all, doc.edit_refusal(), ids, doc.physical_job.is_some())))
}

/// Present: the dialog, rebuilt when what it shows changes (its scroll offset kept).
pub(in crate::cad) fn draw_form(mut commands: Commands, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<FormRoot>>, scrolls: Query<&ScrollPosition, With<FormScroll>>, mut last: Local<Option<String>>) {
    let key = doc.as_deref().and_then(|d| form_key(d).map(|key| format!("{key}|source={}", crate::cad::activation::render_key(d))));
    let present = roots.iter().next().is_some();
    if key == *last && present == key.is_some() {
        return;
    }
    *last = key;
    let offset = scrolls.iter().next().map_or(0.0, |s| s.0.y);
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some(doc) = doc.as_deref() else { return };
    let Some(f) = &doc.materials.form else { return };
    let k = Kit::new(&fonts);
    let title = match &f.kind {
        FormKind::New => "New material".to_string(),
        FormKind::Properties { name, .. } => format!("{name}: engineering properties"),
    };
    let at = match doc.materials.focus {
        Some(Focus::Field(i)) => Some(i),
        _ => None,
    };
    // `picks: &[]`: the kit's choice-from-a-list field (cad-physical-inspect part D) is not used here.
    let rows: Vec<FormRow> = f
        .fields
        .iter()
        .zip(&f.texts)
        .enumerate()
        .map(|(i, (field, text))| FormRow { label: &field.label, kind: field.kind, text, focused: at == Some(i), optional: field.optional(), selected: at == Some(i) && doc.materials.select_all, picks: &[] })
        .collect();
    let ok = form::ok_ready(f).is_ok() && doc.edit_refusal().is_none();
    commands.spawn((k.backdrop(&title, true), FormRoot, DespawnOnExit(ModeScope::Cad))).with_children(|backdrop| {
        backdrop.spawn((k.scroll_area(Node { max_height: Val::Percent(90.0), flex_direction: FlexDirection::Column, ..default() }, offset), FormScroll)).with_children(|area| {
            k.form(area, &title, &rows, ok, Some(WIDTH), FormPart, |p| footer(p, &k, doc, f));
        });
    });
}

/// Under the dialog's buttons: OK's last refusal, why OK is disabled, and what OK sends.
fn footer(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, f: &form::MaterialForm) {
    if let Some(e) = &f.error {
        p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
    } else if let Err(e) = form::ok_ready(f) {
        p.spawn(k.text(e, size::SMALL, DANGER, 0));
    }
    if let Some(why) = doc.edit_refusal() {
        p.spawn(k.note(format!("OK is unavailable: {why}.")));
    }
    match &f.kind {
        FormKind::New => {
            let id = f.texts.first().map(|n| n.to_lowercase().replace(' ', "_")).filter(|s| !s.is_empty()).unwrap_or_else(|| "custom".into());
            if list(doc).iter().any(|m| m.id == id) {
                p.spawn(k.note(format!("OK replaces the existing material \"{id}\", as RoboCAD's New… does (one undo step).")));
            } else {
                p.spawn(k.note(format!("OK adds material \"{id}\" to the document (one undo step).")));
            }
        }
        FormKind::Properties { .. } => {
            if doc.physical_job.is_some() {
                p.spawn(k.caption("Fetching RoboCAD's physical model…"));
            }
            p.spawn(k.note("Each field names where its value comes from. OK sends only the fields you changed, in SI, as one undo step. RoboCAD reports its defaults in its physical model, for the materials the model uses; it is fetched for this revision while the dialog is open."));
        }
    }
}

/// Present (before the dialog is drawn): the wheel scrolls the open dialog.
pub(in crate::cad) fn scroll_form(mut wheel: MessageReader<MouseWheel>, doc: Option<Res<CadDocument>>, mut areas: Query<&mut ScrollPosition, With<FormScroll>>) {
    let delta = wheel_delta(&mut wheel, WHEEL_LINE);
    if delta == 0.0 || !doc.as_deref().is_some_and(|d| d.materials.form.is_some()) {
        return;
    }
    for mut position in &mut areas {
        position.0.y = (position.0.y - delta).max(0.0);
    }
}
