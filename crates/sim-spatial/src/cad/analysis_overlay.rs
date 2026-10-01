//! Read-only analysis overlays (cad-modify): RoboCAD's control points
//! (`tool.control_points`), curvature comb (`inspect.curvature`) and
//! continuity check (`inspect.continuity`), read through RoboCAD's routes
//! on a job and drawn as display-only lines. Nothing is written.

/// The analysis shown now (`CadDocument::ops.analysis`).
#[derive(Default)]
pub struct Analysis {}

/// What a read asks RoboCAD for (read-only; built by `ops::args`).
#[derive(Clone, Debug, PartialEq)]
pub enum Read {
    /// RoboCAD's "Copy with Placement": the nodes' clipboard JSON (B-rep and placement).
    Copy { ids: Vec<String> },
    /// A face's control points (`kernel.control_points`).
    ControlPoints { node: String, face: i64 },
    /// A curve or sketch's curvature comb (`analysis.curvature_comb`).
    CurvatureComb { node: String },
    /// A body's edge continuity (`analysis.continuity_report`).
    Continuity { node: String },
}

/// Start `read` on a job (refused by name while one runs or when not
/// connected); the result lands in `CadDocument::ops` (the clipboard or
/// the overlay) and the status line. `revision`: the shown revision the
/// picks were made at.
pub(super) fn start(doc: &mut super::document::CadDocument, read: Read, revision: u64) -> Result<serde_json::Value, String> {
    let _ = (doc, read, revision);
    todo!("analysis_overlay::start")
}

/// The overlays' systems (JobResults receive, Present draw).
pub(super) fn build(app: &mut bevy::prelude::App) {
    let _ = app;
}
