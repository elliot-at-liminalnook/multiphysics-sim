//! Reusable Bevy presentation of shared inspection contracts. Inspection mode
//! never advances physics. Build mode (`--system`) edits a system file through
//! the shared `sim-system` commands and runs it on the shared runtime in a
//! background thread; it contains no physics of its own.
mod animation;
pub(crate) mod annotate;
pub(crate) mod camera;
pub mod app;
pub(crate) mod inspect;
mod inspect_view;
pub(crate) mod chart;
pub mod builder;
pub mod cad;
pub mod jobs;
pub mod launch;
pub mod lesson;
mod linked;
pub mod models;
pub mod phenomena;
pub mod place_view;
pub mod robot;
pub mod markdown;
pub(crate) mod physics_view;
pub(crate) mod view;
mod notes;
pub mod rest;
pub mod workspace;
pub(crate) mod ui_kit;
// The crate root's imports below also serve the modules that glob-import it
// (`use super::*` in linked, inspect, animation, notes, view, physics_view,
// builder and rest).
use bevy::{camera::Viewport, prelude::*};
pub use app::{Launch, ViewerMode};
use app::{ModeScope, ViewerSet};
pub use builder::{Builder, BuilderPlugin};
pub use inspect_view::{LearnView, ModelColor, SceneContent, SpatialScene, SpatialViewerPlugin, UiRoot, default_inspect_paths, inspect_pair, load_inspect};
pub(crate) use camera::Orbit;
pub(crate) use inspect_view::{Part, spawn_parts, update_parts};
pub use linked::SelectionLink;
use sim_inspect::selection::SelectionTarget;
use sim_inspect::{
    InspectionError, SystemDescription,
    spatial::{SpatialCommand, SpatialDescription, SpatialShape},
};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn fixture() -> SpatialScene {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/systems-viewer/spatial");
        SpatialScene::new(
            serde_json::from_slice(
                &std::fs::read(base.join("motor-thermal.description.json")).unwrap(),
            )
            .unwrap(),
            serde_json::from_slice(
                &std::fs::read(base.join("motor-thermal.spatial.json")).unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }
}
