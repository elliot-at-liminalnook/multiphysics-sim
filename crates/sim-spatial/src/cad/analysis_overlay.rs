//! Read-only analysis overlays (cad-modify): RoboCAD's control points
//! (`tool.control_points`), curvature comb (`inspect.curvature`) and
//! continuity check (`inspect.continuity`), read through RoboCAD's routes
//! on a job and drawn as display-only lines. Nothing is written.

/// The analysis shown now (`CadDocument::ops.analysis`).
#[derive(Default)]
pub struct Analysis {}
