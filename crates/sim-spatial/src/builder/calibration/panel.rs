//! Measured evidence stays in the existing Actuators surface. The retained
//! study feature renders both archived and captured evidence through kit controls.
use super::*;
pub(in crate::builder) fn section(
    body: &mut ChildSpawnerCommands, k: &Kit, _builder: &Builder,
    owner: &study::StudyOwner, ui: &study::forms::StudyUi,
) {
    study::ui::section(body, k, owner, ui);
}
