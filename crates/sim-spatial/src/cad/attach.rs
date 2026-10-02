//! The attach field (window-first-usability): while the document is not
//! connected (RoboCAD is still starting, or the connection was refused or
//! lost), the left dock's document part shows a URL field and "Attach",
//! which write `CadAction::CadOpen {url}`, the action REST `cad_open {url}`
//! parses into (the same `open`: refused while an edit is in flight or a
//! self-started service holds unsaved edits; the refusal shows in the status
//! bar). A `.rcad` file opens through File → Open… (`files`).
//!
//! Typing is the kit's (`ui_kit::text`, field [`ATTACH`]): a press on the
//! field gives it the keyboard (any other field's entry ends: the kit tells
//! it), Enter attaches, Escape or a press elsewhere ends it, and an open
//! command surface or path form, the field going away or the connection
//! arriving end it too. The CAD keys and the shared camera's keys stand
//! aside while it types (the kit's `typing`).
//!
//! The field draws into an [`AttachRoot`] node the document part spawns
//! ([`root`]); [`draw`] fills it whenever that node is new or the draft
//! changes.
use super::actions::CadAction;
use super::document::{CadDocument, Connection};
use crate::app::actions::Act;
use crate::app::{ViewerMode, ViewerSet};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFieldApp, TextFocus, Typing};
use crate::ui_kit::{DANGER, Kit, Look, SUBTLE, UiFonts, size};
use bevy::ecs::system::ParamSet;
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
// RoboCAD's default service address (what CAD mode attaches to without a document).
use sim_runtime::cad_client::DEFAULT_URL;

const LABEL: &str = "Attach to a running RoboCAD (loopback URL)";

/// The attach URL field (`ui_kit::text`).
pub(super) const ATTACH: FieldId = FieldId("cad.attach");

/// The attach field's text, kept while it does not type (the kit field's
/// draft mirrored on each change), and its last refusal.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(super) struct AttachDraft {
    pub draft: TextDraft,
    /// Why the last Enter or press sent nothing.
    pub error: Option<String>,
}

/// Where the field is drawn (spawned by the document part while not connected).
#[derive(Component)]
pub(super) struct AttachRoot;

/// A pressable part of the field.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AttachPart {
    Field,
    Attach,
}

/// The node the field draws into (the document part calls it while not connected).
pub(super) fn root(p: &mut ChildSpawnerCommands) {
    p.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), margin: UiRect::top(Val::Px(6.0)), ..default() }, AttachRoot));
}

pub(super) fn build(app: &mut App) {
    app.add_text_field(ATTACH, TextField::new(LABEL).placeholder(DEFAULT_URL)).init_resource::<AttachDraft>().add_systems(
        Update,
        (
            // Before CAD's keys: a press that gives the field the keyboard holds the frame's keys.
            input.in_set(crate::app::InputSet::Window).in_set(crate::cad::CadKeySet::Focus).in_set(ViewerSet::Input),
            draw.in_set(ViewerSet::Present),
        )
            .run_if(in_state(ViewerMode::Cad)),
    );
}

/// The action Enter or "Attach" writes, or why none.
fn attach_action(text: &str) -> Result<CadAction, String> {
    // An empty field is the address its placeholder shows.
    let url = match text.trim() {
        "" => DEFAULT_URL,
        url => url,
    };
    Ok(CadAction::CadOpen { path: None, url: Some(url.to_string()) })
}

/// Input: the field's presses and its kit field's events (see the module doc).
#[allow(clippy::type_complexity)]
pub(super) fn input(
    mut state: ResMut<AttachDraft>,
    doc: Option<Res<CadDocument>>,
    presses: Query<&AttachPart, With<crate::ui_kit::activation::Activated>>,
    roots: Query<(), With<AttachRoot>>,
    // The field's messages are read first, then `TextFocus` acts (a `ParamSet`: one at a time).
    mut field: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    files: Option<Res<crate::cad::files::CadFiles>>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let events: Vec<FieldEvent> = field.p0().read().filter(|m| m.field == ATTACH).map(|m| m.event.clone()).collect();
    let mut text = field.p1();
    let Some(doc) = doc else {
        // No document, no field: the next document's field starts unfocused.
        text.blur(ATTACH);
        return;
    };
    let before = state.clone();
    let mut s = before.clone();
    let mut submit = false;
    for event in events {
        match event {
            FieldEvent::Changed(draft) => {
                s.draft = draft;
                s.error = None;
            }
            FieldEvent::Submit(_) => submit = true,
            // Escape (the kit has taken the keyboard away), a press
            // elsewhere or another field: the typed text stays shown.
            FieldEvent::Cancel | FieldEvent::Blur | FieldEvent::Tab { .. } | FieldEvent::Arrow { .. } => {}
        }
    }
    for part in &presses {
        match part {
            AttachPart::Field if !text.focused(ATTACH) => {
                s.error = None;
                if s.draft.text.is_empty() {
                    s.draft = TextDraft::new(DEFAULT_URL, true);
                } else {
                    s.draft.select_all = true;
                }
                text.focus_draft(ATTACH, s.draft.clone());
            }
            AttachPart::Field => {}
            AttachPart::Attach => submit = true,
        }
    }
    // A command surface or the path form took the keyboard, the field is
    // gone, or the connection arrived: the typing ends.
    if text.focused(ATTACH) && (doc.ops.surface.is_some() || files.as_ref().is_some_and(|f| f.form.is_some()) || roots.is_empty() || doc.connection == Connection::Connected) {
        text.blur(ATTACH);
    }
    if submit {
        match attach_action(&s.draft.text) {
            Ok(action) => {
                out.write(Act::ui(action));
                text.blur(ATTACH);
                s.error = None;
            }
            Err(why) => s.error = Some(why),
        }
    }
    if s != before {
        *state = s;
    }
}

/// Present: the field, drawn into a new root or redrawn when the draft changes.
pub(super) fn draw(mut commands: Commands, state: Res<AttachDraft>, typing: Typing, fonts: Res<UiFonts>, roots: Query<Entity, With<AttachRoot>>, mut last: Local<Option<(Entity, AttachDraft, bool)>>) {
    let Ok(root) = roots.single() else {
        *last = None;
        return;
    };
    let focused = typing.focused(ATTACH);
    // Compared, not formatted: nothing is built per frame while unchanged.
    if last.as_ref().is_some_and(|(e, d, f)| *e == root && *d == *state && *f == focused) {
        return;
    }
    *last = Some((root, state.clone(), focused));
    let k = Kit::new(&fonts);
    let ready = attach_action(&state.draft.text).is_ok();
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|p| {
        p.spawn(k.text(LABEL, size::CAPTION, SUBTLE, 1));
        p.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|line| {
            line.spawn(k.input_selectable(&state.draft.text, DEFAULT_URL, AttachPart::Field, focused, state.draft.select_all)).insert((
                AccessibleLabel::new(LABEL),
                Node { flex_grow: 1.0, min_width: Val::Px(0.0), border_radius: BorderRadius::all(Val::Px(5.)), padding: UiRect::axes(Val::Px(10.), Val::Px(7.)), border: UiRect::all(Val::Px(1.)), ..default() },
            ));
            line.spawn(k.button("Attach", AttachPart::Attach, Look::Primary, ready));
        });
        if let Some(error) = &state.error {
            p.spawn(k.text(error.clone(), size::SMALL, DANGER, 0));
        }
    });
}
