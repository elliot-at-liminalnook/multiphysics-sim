//! RoboCAD's Saved Views panel (`ui/saved_views.py` `SavedViewsPanel`) as a
//! floating kit panel over the 3D view's lower right corner, shown by
//! `view.saved_views` (`cad_views {"op": "panel"}`): the hint, the name
//! field with "Save current view", one row per view (its name, "Orthographic"
//! or "Perspective", "· Cutaway") with RoboCAD's four actions (Restore view,
//! Replace with current, Rename…, Delete), and the feedback line. Every
//! button is a `CadButton` writing the action its `system_ui` control lists
//! (`views::controls_of`). Rename types the new name into the row's own
//! field (RoboCAD opens a text dialog).
//!
//! The same panel shows RoboCAD's `view.fov` entry ("Field of view",
//! degrees 5–120, one decimal) while it is open, also with the views hidden.
//!
//! Typing is the kit's one text field ([`VIEWS`], `ui_kit::text`; one kit
//! field for every field of the panel, `CadViews::typing` naming which one
//! it edits, its draft mirrored and the last refusal): a press on a field
//! gives it the keyboard, Enter submits (a refusal stays under the field),
//! Escape or a press elsewhere ends it. A field the handler opens (`view.fov`)
//! takes the keyboard at the next input (`CadViews::focus_request`).
use super::{CadViews, Typing, ViewField, ViewsArgs, ViewsOp, controls_of, convert};
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::builder::ui_api::Enabled;
use crate::cad::panel::CadButton;
use crate::camera::CameraAction;
use crate::ui_kit::form::{FieldKind, FieldValue, Unit, evaluate};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFocus};
use crate::ui_kit::{BORDER, DANGER, FAINT, Kit, Look, RIGHT_WIDTH, STATUSBAR, SUBTLE, SURFACE, TEXT, UiFonts, above_strip, size, wrap};
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use sim_runtime::cad_client::check_view_name;

/// The panel's field: one kit field for every field of the panel (which
/// one is `CadViews::typing`).
pub(in crate::cad) const VIEWS: FieldId = FieldId("cad.views");

/// The panel's field as the kit spawns it (`views::build`); each field
/// opens with its own selection (`focus_draft`).
pub(super) fn text_field() -> TextField {
    TextField::new("Saved view")
}

/// RoboCAD's `view.fov` dialog: `QInputDialog.getDouble(…, 5, 120, 1)`.
const FOV: FieldKind = FieldKind::Number { unit: Unit::Angle, min: Some(5.0), max: Some(120.0), decimals: 1 };
/// Below the command popups (44) and the mode switcher (40).
const PANEL_Z: i32 = 30;

/// The panel's root (rebuilt whole when what it shows changes).
#[derive(Component)]
pub(super) struct ViewsRoot;

/// A field of the panel: a press gives it the keyboard.
#[derive(Component, Clone, Debug)]
pub(super) struct ViewInput(ViewField);

/// Degrees as RoboCAD's dialog shows them (one decimal, no trailing ".0").
pub(super) fn fov_text(degrees: f32) -> String {
    let r = (f64::from(degrees) * 10.0).round() / 10.0;
    if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r:.1}") }
}

/// What an Enter sends.
enum Submit {
    Cad(CadAction),
    Camera(CameraAction),
}

/// The field's Enter: its action, None when nothing changes, or why it
/// sends nothing (shown under the field).
fn submit(t: &Typing, doc: &CadDocument, views: &CadViews) -> Result<Option<Submit>, String> {
    let blocked = || doc.edit_refusal().map(|why| format!("Not sent: {why}. Enter again once it clears, or Escape."));
    match &t.field {
        ViewField::New => {
            let name = check_view_name(&t.draft.text)?;
            if let Some(why) = blocked() {
                return Err(why);
            }
            Ok(Some(Submit::Cad(ViewsArgs::of(ViewsOp::Save, None, Some(name.as_str())))))
        }
        ViewField::Rename(id) => {
            let name = check_view_name(&t.draft.text)?;
            if views.shown(doc).iter().any(|v| v.id == *id && v.name == name) {
                return Ok(None);
            }
            if let Some(why) = blocked() {
                return Err(why);
            }
            Ok(Some(Submit::Cad(ViewsArgs::of(ViewsOp::Rename, Some(id.as_str()), Some(name.as_str())))))
        }
        ViewField::Fov => match evaluate(&FOV, &t.draft.text) {
            // The dialog's one decimal.
            Ok(FieldValue::Number(v)) => Ok(Some(Submit::Camera(CameraAction::Fov { degrees: ((v * 10.0).round() / 10.0) as f32 }))),
            Ok(_) => Err("type the field of view in degrees".into()),
            Err(e) => Err(format!("Field of view: {e} degrees")),
        },
    }
}

/// The new view's name is kept while its field is not focused.
fn keep_name(views: &mut ResMut<CadViews>, typing: Option<&Typing>) {
    if let Some(Typing { field: ViewField::New, draft, .. }) = typing
        && views.new_name != draft.text
    {
        views.new_name = draft.text.clone();
    }
}

/// Input: the panel's fields (see the module doc).
pub(super) fn input(
    views: Option<ResMut<CadViews>>,
    doc: Option<Res<CadDocument>>,
    presses: Query<(&Interaction, &ViewInput, Option<&Enabled>), Changed<Interaction>>,
    mut msgs: MessageReader<FieldMsg>,
    mut text: TextFocus,
    mut out: MessageWriter<Act<CadAction>>,
    mut camera_out: Option<ResMut<Messages<Act<CameraAction>>>>,
) {
    let (Some(mut views), Some(doc)) = (views, doc) else {
        msgs.clear();
        return;
    };
    let before = views.typing.clone();
    let mut typing = before.clone();
    // A field the handler opened (`open_fov`) takes the keyboard; this
    // frame's events were the field's previous draft's, so they are dropped.
    let requested = views.focus_request;
    if requested {
        views.focus_request = false;
        if let Some(t) = &typing {
            text.focus_draft(VIEWS, t.draft.clone());
        }
    }
    for m in msgs.read().filter(|m| m.field == VIEWS && !requested) {
        match &m.event {
            FieldEvent::Changed(d) => {
                if let Some(t) = typing.as_mut() {
                    t.draft = d.clone();
                    t.error = None;
                }
            }
            FieldEvent::Submit(typed) => {
                let Some(t) = typing.as_mut() else {
                    text.blur(VIEWS);
                    continue;
                };
                t.draft.text = typed.clone();
                match submit(t, &doc, &views) {
                    Ok(action) => {
                        match action {
                            Some(Submit::Cad(a)) => {
                                out.write(Act::ui(a));
                            }
                            Some(Submit::Camera(a)) => {
                                if let Some(camera) = camera_out.as_mut() {
                                    camera.write(Act::ui(a));
                                }
                            }
                            None => {}
                        }
                        // The new view's name stays until RoboCAD answers the save, cleared only on success (`views::settle_save`),
                        // with this frame's keystrokes (settle_save compares it with the sent name).
                        keep_name(&mut views, typing.as_ref());
                        typing = None;
                        text.blur(VIEWS);
                    }
                    Err(why) => t.error = Some(why),
                }
            }
            FieldEvent::Cancel => {
                keep_name(&mut views, typing.as_ref());
                typing = None;
            }
            // A press elsewhere or another field's focus (unless this
            // system gave the field the keyboard again since).
            FieldEvent::Blur if !text.focused(VIEWS) => {
                keep_name(&mut views, typing.as_ref());
                typing = None;
            }
            FieldEvent::Blur | FieldEvent::Tab { .. } | FieldEvent::Arrow { .. } => {}
        }
    }
    for (interaction, f, enabled) in &presses {
        if *interaction != Interaction::Pressed || enabled.is_some_and(|e| !e.0) || typing.as_ref().is_some_and(|t| t.field == f.0) {
            continue;
        }
        let (shown, select_all) = match &f.0 {
            ViewField::New => (views.new_name.clone(), false),
            ViewField::Rename(id) => (views.shown(&doc).iter().find(|v| v.id == *id).map(|v| v.name.clone()).unwrap_or_default(), true),
            ViewField::Fov => (views.camera.map(|c| fov_text(c.fov.to_degrees())).unwrap_or_default(), true),
        };
        let draft = TextDraft::new(shown, select_all);
        if text.focus_draft(VIEWS, draft.clone()) {
            keep_name(&mut views, typing.as_ref());
            typing = Some(Typing { field: f.0.clone(), draft, error: None });
        }
    }
    // The kit's focus is the record: a field that lost the keyboard
    // without a message read here ends, and one the handler closed (the
    // panel hidden) gives the keyboard back.
    if typing.is_some() && !text.focused(VIEWS) {
        keep_name(&mut views, typing.as_ref());
        typing = None;
    }
    if typing.is_none() {
        text.blur(VIEWS);
    }
    keep_name(&mut views, typing.as_ref());
    if typing != before {
        views.typing = typing;
    }
}

/// What the panel shows now, as a comparable text (None: hidden).
fn panel_key(views: &CadViews, doc: &CadDocument) -> Option<String> {
    let fov = views.typing.as_ref().is_some_and(|t| t.field == ViewField::Fov);
    if !views.open && !fov {
        return None;
    }
    let list: Vec<(&str, &str, String)> = views.shown(doc).iter().map(|v| (v.id.as_str(), v.name.as_str(), convert::details(&v.state))).collect();
    let ready: Vec<bool> = controls_of(doc, Some(views)).iter().map(|c| c.3.is_ok()).collect();
    Some(format!("{:?}", (views.open, &views.typing, &views.new_name, list, &views.selected, &views.feedback, views.error.as_ref().map(|e| &e.1), ready, doc.connected())))
}

/// Present: the panel, rebuilt when what it shows changes.
pub(super) fn draw(mut commands: Commands, views: Option<Res<CadViews>>, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<ViewsRoot>>, mut shown: Local<Option<String>>) {
    let want = match (views.as_deref(), doc.as_deref()) {
        (Some(v), Some(d)) => panel_key(v, d),
        _ => None,
    };
    let present = roots.iter().next().is_some();
    if *shown == want && present == want.is_some() {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    *shown = want;
    let (Some(views), Some(doc)) = (views.as_deref(), doc.as_deref()) else { return };
    if shown.is_none() {
        return;
    }
    let k = Kit::new(&fonts);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(RIGHT_WIDTH + 8.0),
                bottom: above_strip(STATUSBAR + 8.0),
                width: Val::Px(320.0),
                max_height: Val::Percent(70.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER),
            FocusPolicy::Block,
            GlobalZIndex(PANEL_Z),
            AccessibleLabel::new("Saved views"),
            ViewsRoot,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|p| body(p, &k, views, doc));
}

/// A field: its text (the draft while typed), focused or not.
fn field(p: &mut ChildSpawnerCommands, k: &Kit, views: &CadViews, which: ViewField, idle: &str, placeholder: &str) {
    let typing = views.typing.as_ref().filter(|t| t.field == which);
    let shown = typing.map_or(idle, |t| t.draft.text.as_str());
    p.spawn(k.input_selectable(shown, placeholder, ViewInput(which), typing.is_some(), typing.is_some_and(|t| t.draft.select_all)));
    if let Some(e) = typing.and_then(|t| t.error.as_ref()) {
        p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
    }
}

/// The controls (`views::controls_of`): (id, label, action, ready).
type Controls = [(String, String, CadAction, Result<(), String>)];

/// The button of control `id`, enabled as the control is.
fn button(p: &mut ChildSpawnerCommands, k: &Kit, controls: &Controls, id: &str, label: &str, look: Look) {
    if let Some((_, _, action, ready)) = controls.iter().find(|c| c.0 == id) {
        p.spawn(k.button(label, CadButton(action.clone()), look, ready.is_ok()));
    }
}

fn body(p: &mut ChildSpawnerCommands, k: &Kit, views: &CadViews, doc: &CadDocument) {
    let controls = controls_of(doc, Some(views));
    let controls = controls.as_slice();
    if views.typing.as_ref().is_some_and(|t| t.field == ViewField::Fov) {
        p.spawn(k.title("Field of view"));
        p.spawn(k.caption("Degrees (5–120), as RoboCAD's dialog. Enter sets it; Escape cancels."));
        field(p, k, views, ViewField::Fov, "", "degrees");
    }
    if !views.open {
        return;
    }
    p.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
        r.spawn(k.title("Saved Views"));
        r.spawn(k.button("Close", CadButton(CadAction::CadViews(ViewsArgs { op: ViewsOp::Panel, open: Some(false), ..ViewsArgs::default() })), Look::Ghost, true));
    });
    p.spawn(k.caption("Save camera angles and cutaways with a name. Restore returns to one."));
    field(p, k, views, ViewField::New, &views.new_name, "View name, e.g. Worm drive cutaway");
    p.spawn(wrap()).with_children(|r| button(r, k, controls, "cad:view:save", "Save current view", Look::Primary));
    if let Some(why) = controls.iter().find(|c| c.0 == "cad:view:save").and_then(|c| c.3.as_ref().err()).filter(|_| !views.new_name.trim().is_empty()) {
        p.spawn(k.note(why.clone()));
    }
    let list = views.shown(doc);
    if let Some((_, e)) = &views.error {
        p.spawn(k.text(format!("Could not list RoboCAD's saved views: {e}"), size::SMALL, DANGER, 0));
    }
    if list.is_empty() {
        p.spawn(k.caption("No saved views yet. Position the model, enter a name, then save."));
    }
    for v in list {
        let selected = views.selected.as_deref() == Some(v.id.as_str());
        p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), padding: UiRect::vertical(Val::Px(4.0)), border: UiRect::top(Val::Px(1.0)), ..default() }).insert(BorderColor::all(BORDER)).with_children(|row| {
            row.spawn(k.text(v.name.clone(), size::ITEM, if selected { TEXT } else { SUBTLE }, if selected { 2 } else { 1 }));
            row.spawn(k.text(convert::details(&v.state), size::CAPTION, FAINT, 0));
            if views.typing.as_ref().is_some_and(|t| t.field == ViewField::Rename(v.id.clone())) {
                field(row, k, views, ViewField::Rename(v.id.clone()), &v.name, "View name");
                row.spawn(k.note("Enter renames it in RoboCAD (one undo step); Escape cancels."));
            }
            row.spawn(wrap()).with_children(|r| {
                button(r, k, controls, &format!("cad:view:{}", v.id), "Restore view", Look::Secondary);
                button(r, k, controls, &format!("cad:view:replace-{}", v.id), "Replace with current", Look::Secondary);
                r.spawn(k.button("Rename…", ViewInput(ViewField::Rename(v.id.clone())), Look::Ghost, doc.edit_refusal().is_none()));
                button(r, k, controls, &format!("cad:view:delete-{}", v.id), "Delete", Look::Danger);
            });
        });
    }
    let feedback = views.feedback.clone().unwrap_or_else(|| "Saved inside this CAD file · edits support Undo".into());
    p.spawn(k.note(feedback));
}
