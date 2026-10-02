//! Global close controls: projection only, never a second close decision owner.
//! The persistent root survives mode changes. Pending work lives in CloseOwner;
//! these transient entities own only labels and action components.
use super::{CloseAction, CloseOwner, CloseSnapshot};
use crate::app::{Persistent, ViewerSet, actions::Act};
use crate::builder::ui_api::Enabled;
use crate::ui_kit::{Kit, Look, UiFonts, above_strip, SURFACE, BORDER, MODAL_Z};
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;

pub(crate) fn controls_in(world: &World) -> Vec<serde_json::Value> {
    let mut controls = Vec::new();
    for entity in world.iter_entities() {
        if entity.get::<Button>().is_none() { continue; }
        let (Some(action), Some(enabled)) = (entity.get::<CloseAction>(), entity.get::<Enabled>()) else { continue; };
        let id = match action {
            CloseAction::CloseRequest => "close:request",
            CloseAction::CloseRetry => "close:retry",
            CloseAction::CloseCancel => "close:cancel",
            CloseAction::CloseWithoutPreferences => "close:without_preferences",
            CloseAction::CloseStatus => continue,
        };
        let label = entity.get::<AccessibleLabel>().map(|label| label.0.as_str()).unwrap_or(id);
        controls.push(serde_json::json!({"id":id,"label":label,"enabled":enabled.0,"action":action}));
    }
    controls.sort_by(|a,b| a["id"].as_str().cmp(&b["id"].as_str()));
    controls
}

#[derive(Component)]
pub(super) struct ClosePanel;

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, clicks.in_set(crate::app::InputSet::Window).in_set(ViewerSet::Input))
        .add_systems(Update, render.in_set(ViewerSet::Present).after(super::CloseSet::Publish));
}

/// A stale enabled projection cannot authorize anything: Actions validates again.
pub(super) fn clicks(
    buttons: Query<(&CloseAction, &Enabled), (With<Button>, With<crate::ui_kit::activation::Activated>)>,
    mut actions: MessageWriter<Act<CloseAction>>,
) {
    for (action, enabled) in &buttons {
        if enabled.0 {
            actions.write(Act::ui(action.clone()));
        }
    }
}

pub(super) fn render(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    owner: Res<CloseOwner>,
    roots: Query<Entity, With<ClosePanel>>,
    mut shown: Local<Option<CloseSnapshot>>,
) {
    let snapshot = owner.snapshot();
    if shown.as_ref() == Some(snapshot) {
        return;
    }
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    spawn_panel(&mut commands, &fonts, snapshot);
    *shown = Some(snapshot.clone());
}

/// Kept separate so isolated fixtures inspect the very same rendered controls.
pub(super) fn spawn_panel(commands: &mut Commands, fonts: &UiFonts, state: &CloseSnapshot) {
    let k = Kit::new(fonts);
    let mut root = commands.spawn((
        Persistent,
        ClosePanel,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(12.0),
            bottom: above_strip(12.0),
            max_width: Val::Px(680.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(SURFACE),
        BorderColor::all(BORDER),
        GlobalZIndex(MODAL_Z + 1),
    ));
    if state.pending {
        root.insert((crate::ui_kit::activation::ModalFocus, crate::ui_kit::activation::ModalPriority(100), AccessibleLabel("Pending viewer close".into())));
    }
    root.with_children(|panel| {
        if state.pending || !state.blockers.is_empty() {
            panel.spawn(k.title("Close viewer"));
            panel.spawn(k.caption(&state.status));
            for blocker in &state.blockers {
                panel.spawn(k.caption(blocker));
            }
            panel.spawn(k.note("Exit without saving preferences acknowledges only the current preference changes and queued recent records. Authored work must still be preserved. New preference changes require another acknowledgment."));
        }
        panel.spawn(Node { column_gap: Val::Px(6.0), row_gap: Val::Px(6.0), flex_wrap: FlexWrap::Wrap, ..default() })
            .with_children(|row| {
                row.spawn(k.button("Close viewer", CloseAction::CloseRequest, Look::Secondary, state.request_enabled));
                if state.pending {
                    row.spawn(k.button("Retry preferences", CloseAction::CloseRetry, Look::Secondary, state.retry_enabled));
                    row.spawn(k.button("Cancel close", CloseAction::CloseCancel, Look::Secondary, state.cancel_enabled));
                    row.spawn(k.button("Exit without saving preferences", CloseAction::CloseWithoutPreferences, Look::Danger, state.preference_exit_enabled));
                }
            });
    });
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
