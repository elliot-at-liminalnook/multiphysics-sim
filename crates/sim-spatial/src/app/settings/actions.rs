use super::*;
use crate::app::actions::{self, Act, Action, InFlight, Replies, Spec};
use bevy::ecs::message::Messages;
use sim_api::Outcome;
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum SettingsAction {
    SettingsStatus,
    SettingsRetry,
}
impl Action for SettingsAction {
    fn commands() -> Vec<Spec> {
        vec![
            actions::spec(
                "settings_status",
                actions::ALL,
                serde_json::json!({}),
                "Preference readiness, dirty revisions and publication diagnostics",
            ),
            actions::spec(
                "settings_retry",
                actions::ALL,
                serde_json::json!({}),
                "Retry failed preference load/save without modifying data",
            ),
        ]
    }
}
pub(super) fn apply(
    mut messages: ResMut<Messages<Act<SettingsAction>>>,
    mut flight: ResMut<InFlight<SettingsAction>>,
    mut replies: ResMut<Replies>,
    mut owner: ResMut<SettingsOwner>,
) {
    actions::apply(&mut messages, &mut flight, &mut replies, |action, _| {
        if matches!(action, SettingsAction::SettingsRetry) {
            owner.retry();
        }
        Outcome::Done(Ok(owner.status()))
    });
}
