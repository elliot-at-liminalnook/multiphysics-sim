//! The fastener tool's face clicks (part C: stub).
use super::PrintArgs;
use crate::app::actions::Call;
use crate::cad::actions::Cx;
use bevy::prelude::*;
use sim_api::Outcome;

pub(super) fn pick(_args: &PrintArgs, _call: &mut Call, _cx: &mut Cx) -> Outcome {
    Outcome::Done(Err("not yet".into()))
}
pub(super) fn build(_app: &mut App) {}
