//! The shared composition presentation path for CAD and Build. Layout is a
//! disposable view over inspection data and never mutates physical geometry.
use crate::{
    Layout, layout,
    projection::{self, NodeSource, Projection},
};
use sim_inspect::SystemDescription;
use std::{collections::BTreeSet, sync::atomic::AtomicBool};

pub struct Presentation {
    pub projection: Projection,
    pub layout: Layout,
}

pub fn present(
    description: &SystemDescription,
    collapsed: &BTreeSet<String>,
    focus: Option<&NodeSource>,
    cancel: &AtomicBool,
) -> Option<Presentation> {
    let projection = projection::project(description, collapsed, focus);
    let state = layout::initial_state_cancellable(&projection.view, cancel)?;
    let layout = layout::route_cancellable(&projection.view, &state, cancel)?;
    Some(Presentation { projection, layout })
}

/// Zoom is display state only. Clamping here gives both consumers the same
/// bounds and keeps invalid user values out of transforms.
pub fn zoom(current: f32, factor: f32) -> f32 {
    if !factor.is_finite() || factor <= 0. {
        return current;
    }
    (current * factor).clamp(0.05, 8.)
}
