//! The catalogue's interactive flows (cad-modify): placing primitives
//! (RoboCAD's `PrimitiveTool`: drag the base, then the height; Tab for
//! exact sizes), pick-then-form tools (RoboCAD's `EdgeTool`/`ShellTool`:
//! clicks toggle edges or faces), "Set pivot at cursor snap", and the view
//! direction for "Project curve onto body". Previews are display only.

/// A primitive being placed: the base's first and current points on the
/// plane (mm, RoboCAD's frame), the stage, and the height being dragged.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// 1: dragging the base; 2: dragging the height.
    pub stage: u8,
    pub p0: [f64; 3],
    pub p1: [f64; 3],
    pub height: f64,
}
