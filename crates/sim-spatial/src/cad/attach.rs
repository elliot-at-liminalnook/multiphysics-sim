//! The attach field (window-first-usability): while the document is not
//! connected (RoboCAD is still starting, or the connection was refused or
//! lost), the left dock's document part shows a URL field and "Attach",
//! which write `CadAction::CadOpen {url}`, the action REST `cad_open {url}`
//! parses into (the same `open`: refused while an edit is in flight or a
//! self-started service holds unsaved edits; the refusal shows in the status
//! bar). A `.rcad` file opens through File → Open… (`files`).
//!
//! Typing follows the saved-views fields (`views::panel`): a press on the
//! field gives it the keyboard (ending the name field, the inspector
//! editors, the numeric bar, the section offset and the saved-views
//! fields), Enter attaches, Escape or a press elsewhere ends it, and
//! another field or an open command surface or path form taking the
//! keyboard ends it too. `CadInputFocus` is set while it types and in the
//! frame it ends, after `panel::name_entry` resets it and before the CAD
//! keys read it (the shared camera's keys follow it through `scene::fit`).
//!
//! The field draws into an [`AttachRoot`] node the document part spawns
//! ([`root`]); [`draw`] fills it whenever that node is new or the draft
//! changes.
use super::actions::CadAction;
use super::document::{CadDocument, CadInputFocus, Connection};
use super::panel::NameDraft;
use crate::app::actions::Act;
use crate::app::{ViewerMode, ViewerSet};
use crate::ui_kit::form::{DraftKey, TextDraft};
use crate::ui_kit::{DANGER, Kit, Look, SUBTLE, UiFonts, size};
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
// RoboCAD's default service address (what CAD mode attaches to without a document).
use sim_runtime::cad_client::DEFAULT_URL;

const LABEL: &str = "Attach to a running RoboCAD (loopback URL)";

/// The attach field's draft.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(super) struct AttachDraft {
    pub draft: TextDraft,
    pub focused: bool,
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
    app.init_resource::<AttachDraft>().add_systems(
        Update,
        (
            input
                .after(crate::app::actions::serve)
                .after(super::panel::name_entry)
                .after(super::inspector::editor_entry)
                .before(super::keys::gate)
                .before(super::keys::keys)
                .in_set(ViewerSet::Input),
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

/// Input: the field's presses and keys (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn input(
    mut state: ResMut<AttachDraft>,
    doc: Option<ResMut<CadDocument>>,
    presses: Query<(&Interaction, &AttachPart), Changed<Interaction>>,
    roots: Query<(), With<AttachRoot>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut events: MessageReader<KeyboardInput>,
    (focus, name): (Option<ResMut<CadInputFocus>>, Option<ResMut<NameDraft>>),
    (section, views, files): (Option<ResMut<crate::cad::display::entry::SectionEntry>>, Option<ResMut<crate::cad::views::CadViews>>, Option<Res<crate::cad::files::CadFiles>>),
    mut out: MessageWriter<Act<CadAction>>,
) {
    let Some(mut doc) = doc else {
        // No document, no field: the next document's field starts unfocused.
        events.clear();
        if state.focused {
            state.focused = false;
        }
        return;
    };
    let before = state.clone();
    let mut s = before.clone();
    let (mut on_field, mut started, mut ended, mut submit) = (false, false, false, false);
    for (interaction, part) in &presses {
        if *interaction != Interaction::Pressed {
            continue;
        }
        on_field = true;
        match part {
            AttachPart::Field if !s.focused => {
                s.focused = true;
                s.error = None;
                started = true;
                if s.draft.text.is_empty() {
                    s.draft = TextDraft { text: DEFAULT_URL.into(), select_all: true };
                } else {
                    s.draft.select_all = true;
                }
            }
            AttachPart::Field => {}
            AttachPart::Attach => submit = true,
        }
    }
    let views_typing = views.as_ref().is_some_and(|v| v.typing.is_some());
    let section_typing = section.as_ref().is_some_and(|e| e.typing.is_some());
    if started {
        // One field holds the keyboard: the others end.
        if let Some(mut name) = name
            && name.editing.is_some()
        {
            name.editing = None;
            name.refusal = None;
        }
        if let Some(mut views) = views.filter(|_| views_typing) {
            views.typing = None;
        }
        if let Some(mut section) = section.filter(|_| section_typing) {
            section.typing = None;
        }
        if doc.tool_state.numeric.focus.is_some() {
            doc.tool_state.numeric.focus = None;
            doc.tool_state.numeric.began = None;
        }
        if doc.tool_state.inspector_edit.is_some() {
            doc.tool_state.inspector_edit = None;
        }
        if doc.ops.form.as_ref().is_some_and(|f| f.focus.is_some())
            && let Some(f) = doc.ops.form.as_mut()
        {
            f.focus = None;
        }
    } else if s.focused {
        // Another field, a command surface or the path form took the keyboard;
        // the field is gone (connected); or a press elsewhere.
        let elsewhere = name.as_ref().is_some_and(|n| n.editing.is_some())
            || views_typing
            || section_typing
            || doc.tool_state.numeric.focus.is_some()
            || doc.tool_state.inspector_edit.is_some()
            || doc.ops.form.as_ref().is_some_and(|f| f.focus.is_some())
            || doc.ops.surface.is_some()
            || files.as_ref().is_some_and(|f| f.form.is_some())
            || roots.is_empty()
            || doc.connection == Connection::Connected
            || (!on_field && mouse.just_pressed(MouseButton::Left));
        if elsewhere {
            s.focused = false;
            ended = true;
        }
    }
    if started || !s.focused {
        // Keys pressed before the field took the keyboard are not its text.
        events.clear();
    } else {
        let chord = keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]);
        let typed: Vec<KeyboardInput> = events.read().filter(|e| e.state == ButtonState::Pressed).cloned().collect();
        for e in typed {
            match s.draft.key(&e.logical_key, chord) {
                DraftKey::Enter => {
                    submit = true;
                    break;
                }
                DraftKey::Escape => {
                    s.focused = false;
                    ended = true;
                    break;
                }
                DraftKey::Edited => s.error = None,
                DraftKey::Tab | DraftKey::Ignored => {}
            }
        }
    }
    if submit {
        match attach_action(&s.draft.text) {
            Ok(action) => {
                out.write(Act::ui(action));
                if s.focused {
                    s.focused = false;
                    ended = true;
                }
                s.error = None;
            }
            Err(why) => s.error = Some(why),
        }
    }
    // Only ever set (as every CAD field does): `panel::name_entry` resets it each frame.
    if let Some(mut focus) = focus
        && (s.focused || ended)
        && !focus.0
    {
        focus.0 = true;
    }
    if s != before {
        *state = s;
    }
}

/// Present: the field, drawn into a new root or redrawn when the draft changes.
pub(super) fn draw(mut commands: Commands, state: Res<AttachDraft>, fonts: Res<UiFonts>, roots: Query<Entity, With<AttachRoot>>, mut last: Local<Option<(Entity, AttachDraft)>>) {
    let Ok(root) = roots.single() else {
        *last = None;
        return;
    };
    // Compared, not formatted: nothing is built per frame while unchanged.
    if last.as_ref().is_some_and(|(e, d)| *e == root && *d == *state) {
        return;
    }
    *last = Some((root, state.clone()));
    let k = Kit::new(&fonts);
    let ready = attach_action(&state.draft.text).is_ok();
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|p| {
        p.spawn(k.text(LABEL, size::CAPTION, SUBTLE, 1));
        p.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|line| {
            line.spawn(k.input_selectable(&state.draft.text, DEFAULT_URL, AttachPart::Field, state.focused, state.draft.select_all)).insert((
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
