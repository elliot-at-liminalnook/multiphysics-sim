//! Window close is a guarded occurrence; retained offline evidence is never
//! silently discarded. Abrupt process termination cannot be guarded here.
use crate::builder::calibration::study::StudyOwner;
use bevy::prelude::*;
use bevy::window::{ClosingWindow, WindowCloseRequested};

/// Last, before the public window ExitSystems. Hardware's close/loss action has
/// already run in Input/Actions. Match the pinned window plugin's two-frame
/// ClosingWindow lifecycle, checking again before final removal.
pub(super) fn guarded_close(
    mut commands: Commands,
    mut requests: MessageReader<WindowCloseRequested>,
    closing: Query<Entity, With<ClosingWindow>>,
    studies: Option<ResMut<StudyOwner>>,
    ui: Option<Res<crate::builder::calibration::study::forms::StudyUi>>,
) {
    let requested: Vec<Entity> = requests.read().map(|r| r.window).collect();
    if requested.is_empty() && closing.is_empty() { return; }
    if let Some(mut studies) = studies {
        if let Some(reason) = ui.as_ref().and_then(|ui| ui.blocking_reason()) {
            studies.status = format!("Window close refused: {reason}");
            for entity in &closing { commands.entity(entity).remove::<ClosingWindow>(); }
            return;
        }
        if let Some(reason) = studies.blocking_reason() {
            studies.status = format!("Window close refused: {reason}");
            for entity in &closing {
                commands.entity(entity).remove::<ClosingWindow>();
            }
            return;
        }
    }
    for entity in &closing {
        commands.entity(entity).despawn();
    }
    for entity in requested {
        commands.entity(entity).try_insert(ClosingWindow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Written only: inspect the actual window entity after a close occurrence.
    #[test]
    fn text_draft_refuses_close_without_losing_window_or_buffer() {
        let mut app = App::new();
        app.add_message::<WindowCloseRequested>()
            .init_resource::<StudyOwner>()
            .init_resource::<crate::builder::calibration::study::forms::StudyUi>()
            .add_systems(Last, guarded_close);
        let window = app.world_mut().spawn(Window::default()).id();
        {
            let mut ui = app.world_mut().resource_mut::<crate::builder::calibration::study::forms::StudyUi>();
            ui.focus = Some((None, crate::builder::calibration::study::forms::Field::Archive));
            ui.buffer = "retained path draft".into();
        }
        app.world_mut().write_message(WindowCloseRequested { window });
        app.update();
        assert!(app.world().get::<Window>(window).is_some());
        assert!(app.world().get::<ClosingWindow>(window).is_none());
        assert_eq!(app.world().resource::<crate::builder::calibration::study::forms::StudyUi>().buffer, "retained path draft");
        assert!(app.world().resource::<StudyOwner>().status.contains("refused"));
    }

    #[test]
    fn clean_close_uses_two_frame_window_lifetime() {
        let mut app = App::new();
        app.add_message::<WindowCloseRequested>()
            .init_resource::<StudyOwner>()
            .add_systems(Last, guarded_close);
        let window = app.world_mut().spawn(Window::default()).id();
        app.world_mut().write_message(WindowCloseRequested { window });
        app.update();
        assert!(app.world().get::<ClosingWindow>(window).is_some());
        app.update();
        assert!(app.world().get::<Window>(window).is_none());
    }
}
