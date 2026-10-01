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
//! - **Typing** ([`input`]), the editors' pattern: a press on a field gives
//!   it the keyboard (a form field's text selected); typing edits it
//!   (`TextDraft`); in the search field each key filters at once, Enter or
//!   Escape ends it; in the dialog Tab moves to the next field, Enter is
//!   OK and Escape Cancel (also with no field focused: the dialog owns the
//!   keyboard while open). A press elsewhere ends the typing. Opening a
//!   field ends the name field's, the editors', the numeric bar's and the
//!   physical rows' drafts. `CadInputFocus` is set while a field has the
//!   keyboard, while the dialog is open and in the frame typing ends.
use super::form::{self, FormKind};
use super::{Focus, MaterialsArgs, MaterialsOp, controls_of, list, matches, row_label};
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadInputFocus};
use crate::cad::panel::{CadButton, NameDraft};
use crate::ui_kit::form::{DraftKey, FormHit, FormRow, TextDraft};
use crate::ui_kit::{ACCENT_BG, DANGER, Kit, Look, TEXT, Tint, UiFonts, WHEEL_LINE, size, wheel_delta, wrap};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use sim_runtime::cad_client::SelectionItem;
use std::time::{Duration, Instant};

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
            MaterialRow(m.id.clone()),
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
fn write(out: &mut MessageWriter<Act<CadAction>>, op: MaterialsOp, material: Option<&str>) {
    out.write(Act::ui(MaterialsArgs::of(op, material)));
}

/// Input: the panel's and the dialog's clicks and keys (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::cad) fn input(
    doc: Option<ResMut<CadDocument>>,
    fields: Query<(&Interaction, &MaterialsInput), Changed<Interaction>>,
    parts: Query<(&Interaction, &FormPart, Option<&Enabled>), Changed<Interaction>>,
    rows: Query<(&Interaction, &MaterialRow), Changed<Interaction>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut events: MessageReader<KeyboardInput>,
    focus: Option<ResMut<CadInputFocus>>,
    name: Option<ResMut<NameDraft>>,
    mut out: MessageWriter<Act<CadAction>>,
    mut last_row: Local<Option<(String, Instant)>>,
) {
    let Some(mut doc) = doc else {
        events.clear();
        return;
    };
    let modal = doc.materials.form.is_some();
    let (before, before_all) = (doc.materials.focus, doc.materials.select_all);
    let mut at = before.filter(|f| modal || *f == Focus::Search);
    let mut select_all = before_all;
    let (mut started, mut ended, mut pressed, mut edited) = (false, false, false, false);
    for (interaction, f) in &fields {
        if *interaction != Interaction::Pressed {
            continue;
        }
        pressed = true;
        if at != Some(f.0) {
            at = Some(f.0);
            // A search press places the caret; a dialog field's text is selected.
            select_all = matches!(f.0, Focus::Field(_));
            started = true;
        }
    }
    for (interaction, part, enabled) in &parts {
        if *interaction != Interaction::Pressed {
            continue;
        }
        pressed = true;
        match part.0 {
            FormHit::Field(i) => {
                if at != Some(Focus::Field(i)) {
                    at = Some(Focus::Field(i));
                    select_all = true;
                    started = true;
                }
            }
            FormHit::Ok if enabled.is_none_or(|e| e.0) => write(&mut out, MaterialsOp::FormSubmit, None),
            FormHit::Cancel => write(&mut out, MaterialsOp::FormCancel, None),
            _ => {}
        }
    }
    for (interaction, row) in &rows {
        if *interaction != Interaction::Pressed {
            continue;
        }
        pressed = true;
        let now = Instant::now();
        if last_row.as_ref().is_some_and(|(id, t)| *id == row.0 && now.duration_since(*t) <= DOUBLE_CLICK) {
            write(&mut out, MaterialsOp::Apply, Some(row.0.as_str()));
            *last_row = None;
        } else {
            write(&mut out, MaterialsOp::Select, Some(row.0.as_str()));
            *last_row = Some((row.0.clone(), now));
        }
    }
    if at.is_some() && !pressed && mouse.as_ref().is_some_and(|m| m.just_pressed(MouseButton::Left)) {
        at = None;
        ended = true;
    }
    if !started && at.is_some() && doc.tool_state.inspector_edit.is_some() {
        // The editors (after this system) opened a draft since.
        at = None;
        ended = true;
    }
    if started {
        // One field holds the keyboard.
        if doc.tool_state.numeric.focus.is_some() {
            doc.tool_state.numeric.focus = None;
            doc.tool_state.numeric.began = None;
        }
        if doc.tool_state.inspector_edit.is_some() {
            doc.tool_state.inspector_edit = None;
        }
        if doc.physical_edit.draft.is_some() {
            doc.physical_edit.draft = None;
            doc.touch();
        }
        if let Some(mut name) = name
            && name.editing.is_some()
        {
            name.editing = None;
            name.refusal = None;
        }
    }
    if started || doc.ops.surface.is_some() {
        // Keys pressed before the field took the keyboard are not its text;
        // an open command surface has the keyboard meanwhile.
        events.clear();
    } else if let Some(f) = at {
        let chord = keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]));
        let typed: Vec<KeyboardInput> = events.read().filter(|e| e.state == ButtonState::Pressed).cloned().collect();
        let mut f = f;
        for e in typed {
            let text = match f {
                Focus::Search => doc.materials.search.clone(),
                Focus::Field(i) => doc.materials.form.as_ref().and_then(|form| form.texts.get(i).cloned()).unwrap_or_default(),
            };
            let mut d = TextDraft { text, select_all };
            match d.key(&e.logical_key, chord) {
                DraftKey::Edited => {
                    select_all = d.select_all;
                    match f {
                        Focus::Search => doc.materials.search = d.text,
                        Focus::Field(i) => {
                            if let Some(form) = doc.materials.form.as_mut()
                                && let Some(slot) = form.texts.get_mut(i)
                            {
                                *slot = d.text;
                                form.error = None;
                            }
                        }
                    }
                    edited = true;
                }
                DraftKey::Enter => {
                    if f == Focus::Search {
                        ended = true;
                        at = None;
                    } else {
                        write(&mut out, MaterialsOp::FormSubmit, None);
                    }
                    break;
                }
                DraftKey::Escape => {
                    if f != Focus::Search {
                        write(&mut out, MaterialsOp::FormCancel, None);
                    }
                    ended = true;
                    at = None;
                    break;
                }
                DraftKey::Tab => {
                    if let Focus::Field(i) = f {
                        let n = doc.materials.form.as_ref().map_or(1, |form| form.fields.len().max(1));
                        f = Focus::Field((i + 1) % n);
                        at = Some(f);
                        select_all = true;
                    }
                }
                DraftKey::Ignored => {}
            }
        }
    } else if modal {
        // RoboCAD's dialog has the keyboard: Enter is OK, Escape Cancel.
        let typed: Vec<KeyboardInput> = events.read().filter(|e| e.state == ButtonState::Pressed).cloned().collect();
        for e in typed {
            match e.logical_key {
                Key::Enter => {
                    write(&mut out, MaterialsOp::FormSubmit, None);
                    break;
                }
                Key::Escape => {
                    write(&mut out, MaterialsOp::FormCancel, None);
                    break;
                }
                _ => {}
            }
        }
    } else {
        events.clear();
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
    if let Some(mut focus) = focus
        && (doc.materials.focus.is_some() || ended || modal)
        && !focus.0
    {
        focus.0 = true;
    }
}

/// What the dialog shows, as a comparable text (None: closed).
fn form_key(doc: &CadDocument) -> Option<String> {
    let f = doc.materials.form.as_ref()?;
    let ids: Vec<String> = list(doc).into_iter().map(|m| m.id).collect();
    Some(format!("{:?}", (doc.generation, f, doc.materials.focus, doc.materials.select_all, doc.edit_refusal(), ids)))
}

/// Present: the dialog, rebuilt when what it shows changes (its scroll offset kept).
pub(in crate::cad) fn draw_form(mut commands: Commands, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<FormRoot>>, scrolls: Query<&ScrollPosition, With<FormScroll>>, mut last: Local<Option<String>>) {
    let key = doc.as_deref().and_then(form_key);
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
            p.spawn(k.note("Each field names where its value comes from. OK sends only the fields you changed, in SI, as one undo step; RoboCAD reports its defaults in its physical model, for materials the model uses (Physical in the toolbar fetches it)."));
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
