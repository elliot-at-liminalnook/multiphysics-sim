//! CAD sketch (cad-sketch epic; native-viewer.md "CAD sketch"): the active
//! plane, construction planes, the 13 sketch tools, sketch edits, extrude
//! and revolve, all on the op catalogue (`super::ops`) and RoboCAD's REST.
//!
//! - [`CadActivePlane`] (this file): the active plane, display state only
//!   (RoboCAD's `viewport.active_plane`; its `PUT /view` is GUI-only), and
//!   the 2D snap toggle (`viewport.plane_snapping`). Never sent to RoboCAD
//!   except as the plane argument of an operation that uses it.
//! - [`cache`]: sketch geometry and plane-node frames (`GET /nodes/{id}`'s
//!   `sketch` and `plane`), by (node, revision), on jobs.
//! - `plane`, `plane_draw`: the plane's systems (the selected plane node,
//!   the plane tools' picks, the header line) and the translucent quads.
//! - `specs`, `interact`, `preview`, `display`, `edits`: the 13 sketch
//!   tools as data ([`SketchSpec`]) driven by one interaction, their
//!   display-only preview and readout, the sketches' curves drawn, the
//!   sketch edits (offset, fillet corners, join) and the `cad_sketch` action.
//! - `extrude`: RoboCAD's `ExtrudeTool` (extrude and revolve).
//!
//! Each finished shape, plane or solid is one RoboCAD call on a job through
//! `actions::edit`, refused by name while an edit is in flight or the shown
//! document is stale (`CadDocument::commit_refusal`). Undo and provenance
//! stay RoboCAD's. Previews never mutate geometry.
pub(crate) mod cache;
pub(crate) mod display;
pub(crate) mod edits;
pub(crate) mod extrude;
pub(crate) mod interact;
pub(crate) mod plane;
pub(crate) mod plane_draw;
pub(crate) mod preview;
pub(crate) mod specs;
#[cfg(test)]
mod tests;

pub use cache::CadSketches;
pub use extrude::ExtrudeState;

use super::document::CadDocument;
use bevy::prelude::*;
use serde_json::Value;
use sim_runtime::cad_client::PlaneFrame;

/// RoboCAD's three named planes (`Plane.xy()`, `.xz()`, `.yz()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasePlane {
    Xy,
    Xz,
    Yz,
}
impl BasePlane {
    pub fn frame(self) -> PlaneFrame {
        match self {
            BasePlane::Xy => PlaneFrame::XY,
            BasePlane::Xz => PlaneFrame::XZ,
            BasePlane::Yz => PlaneFrame::YZ,
        }
    }
    /// The name `ArgConverter.plane` reads ("xy").
    pub fn arg(self) -> &'static str {
        match self {
            BasePlane::Xy => "xy",
            BasePlane::Xz => "xz",
            BasePlane::Yz => "yz",
        }
    }
    /// As the header shows it ("XY").
    pub fn label(self) -> &'static str {
        match self {
            BasePlane::Xy => "XY",
            BasePlane::Xz => "XZ",
            BasePlane::Yz => "YZ",
        }
    }
}

/// The active plane: a named plane or a plane node.
#[derive(Clone, Debug, PartialEq)]
pub enum ActivePlane {
    Base(BasePlane),
    /// A plane node; `frame` is its `plane` at the shown revision, None
    /// while the cache has not read it (just created, or refetching).
    Node { id: String, frame: Option<PlaneFrame> },
}

/// CAD mode's active plane and 2D snap toggle (inserted with CAD mode,
/// reset when the document's generation changes; `plane::sync`).
#[derive(Resource, Default, Debug)]
pub struct CadActivePlane {
    /// None: RoboCAD's `active_plane is None` (its tools then use XY,
    /// mirror YZ: each caller's fallback).
    pub plane: Option<ActivePlane>,
    /// RoboCAD's `plane_snapping` ("Toggle 2D snapping to the active
    /// plane"): snaps are projected onto the active plane. Off at start.
    pub snap_2d: bool,
    /// The document generation the plane belongs to.
    pub generation: u64,
    /// A plane node read at the shown revision without a valid frame: its
    /// id and why (`cache::sync` keeps it from `CadSketches::plane_state`).
    /// Keyed by the node, so a mark left from another node is ignored.
    pub unusable: Option<(String, String)>,
}

impl CadActivePlane {
    /// The active plane's frame: Ok(None) with no active plane; Err naming
    /// the node while a plane node's frame is not read yet, or when it was
    /// read without a valid frame ([`Self::unusable`]).
    pub fn frame(&self) -> Result<Option<PlaneFrame>, String> {
        match &self.plane {
            None => Ok(None),
            Some(ActivePlane::Base(b)) => Ok(Some(b.frame())),
            Some(ActivePlane::Node { frame: Some(f), .. }) => Ok(Some(*f)),
            Some(ActivePlane::Node { id, frame: None }) => Err(match self.unusable_why(id) {
                Some(why) => format!("the active plane: {why}; make another plane active"),
                None => format!("the active plane (node {id}) is still being read from RoboCAD; try again in a moment"),
            }),
        }
    }
    /// Why plane node `id` has no frame although it was read, if it is marked so.
    fn unusable_why(&self, id: &str) -> Option<&str> {
        self.unusable.as_ref().filter(|(u, _)| u == id).map(|(_, why)| why.as_str())
    }
    /// RoboCAD's `self.ctx.active_plane()`: the active plane, else XY.
    pub fn frame_or_xy(&self) -> Result<PlaneFrame, String> {
        self.frame().map(|f| f.unwrap_or(PlaneFrame::XY))
    }
    /// The plane argument RoboCAD's `ArgConverter.plane` reads for the
    /// active plane ("xy", or the plane node's id), else `fallback`
    /// (RoboCAD's `active_plane or Plane.yz()` and the like).
    pub fn arg_or(&self, fallback: BasePlane) -> Value {
        match &self.plane {
            None => Value::from(fallback.arg()),
            Some(ActivePlane::Base(b)) => Value::from(b.arg()),
            Some(ActivePlane::Node { id, .. }) => Value::from(id.clone()),
        }
    }
    /// As the header shows it: "XY", the plane node's name, or "none (XY)".
    pub fn label(&self, doc: &CadDocument) -> String {
        match &self.plane {
            None => "none (XY)".to_string(),
            Some(ActivePlane::Base(b)) => b.label().to_string(),
            Some(ActivePlane::Node { id, frame }) => {
                let name = doc.node_name(id);
                match frame {
                    Some(_) => name,
                    None if self.unusable_why(id).is_some() => format!("{name} (no valid plane frame)"),
                    None => format!("{name} (reading)"),
                }
            }
        }
    }
}

/// RoboCAD's sketch shapes (`SketchTool.shape`, ui/tools.py:638-817), in
/// its registry order (ui/app.py:377).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SketchShape {
    Line,
    Rectangle,
    RectangleCenter,
    Circle,
    Circle2pt,
    Circle3pt,
    Arc3pt,
    Polygon,
    Slot,
    Spline,
    Ellipse,
    Spiral,
    Text,
}
impl SketchShape {
    pub const ALL: [SketchShape; 13] = [
        SketchShape::Line,
        SketchShape::Rectangle,
        SketchShape::RectangleCenter,
        SketchShape::Circle,
        SketchShape::Circle2pt,
        SketchShape::Circle3pt,
        SketchShape::Arc3pt,
        SketchShape::Polygon,
        SketchShape::Slot,
        SketchShape::Spline,
        SketchShape::Ellipse,
        SketchShape::Spiral,
        SketchShape::Text,
    ];
    /// RoboCAD's shape name ("line", "circle_2pt"): the command id is `sketch.<name>`.
    pub fn name(self) -> &'static str {
        match self {
            SketchShape::Line => "line",
            SketchShape::Rectangle => "rectangle",
            SketchShape::RectangleCenter => "rectangle_center",
            SketchShape::Circle => "circle",
            SketchShape::Circle2pt => "circle_2pt",
            SketchShape::Circle3pt => "circle_3pt",
            SketchShape::Arc3pt => "arc_3pt",
            SketchShape::Polygon => "polygon",
            SketchShape::Slot => "slot",
            SketchShape::Spline => "spline",
            SketchShape::Ellipse => "ellipse",
            SketchShape::Spiral => "spiral",
            SketchShape::Text => "text",
        }
    }
}

/// How a sketch tool finishes a shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    /// After this many clicks (RoboCAD's `needed`, tools.py:695).
    Points(usize),
    /// Enter or a double-click with at least two points (the spline).
    EnterOrDouble,
}

/// One sketch tool as data: everything the one interaction
/// (`interact`) and the one builder (`specs`) read. No code per tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SketchSpec {
    pub shape: SketchShape,
    pub finish: Finish,
    /// Lines chain: the last point starts the next shape (tools.py:768).
    pub chains: bool,
    /// The readout RoboCAD shows while drawing (tools.py:720-726).
    pub readout: Readout,
    /// The tool's text comes from a "Text to sketch:" field (the text tool).
    pub text_form: bool,
}

/// Which readout RoboCAD's `hover` writes (tools.py:720-726).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Readout {
    /// "length L  angle A" (line, spline).
    LengthAngle,
    /// "radius R" (circle, polygon).
    Radius,
    /// "W × H" (every other shape).
    Size,
}

/// RoboCAD's sketch commands that edit the selected sketch (ui/app.py:758-785).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SketchEdit {
    /// "Sketch: offset selected curve…": every curve offset by the distance.
    Offset,
    /// "Sketch: fillet corner…": every corner of every closed polyline.
    FilletCorners,
    /// "Sketch: join curves": all curves into one when there are two or more.
    Join,
}

/// RoboCAD's `PlaneTool` modes (ui/tools.py:1065-1111).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaneMode {
    /// Click a face: `plane_from_face`.
    Face,
    /// Click three points: `plane_three_points`.
    Three,
    /// Click two points, the plane faces the camera: `plane_two_points_camera`.
    Camera,
    /// Click two parallel faces: `plane_midplane`.
    Mid,
}

/// Viewer state set by a catalogue entry, never a RoboCAD call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewAct {
    /// "Active plane: XY" (and XZ, YZ).
    Plane(BasePlane),
    /// "Toggle 2D snapping to the active plane".
    Snap2d,
}

/// A sketch shape in progress (`CadDocument::ops.sketch`): the clicked
/// points, snapped and projected onto the active plane (mm, RoboCAD's
/// frame), and the shown revision at the first click (the finished
/// shape's run is refused if RoboCAD's document changed since).
#[derive(Clone, Debug, PartialEq)]
pub struct SketchState {
    pub shape: SketchShape,
    pub points: Vec<[f64; 3]>,
    pub began: u64,
    /// The snapped point under the pointer (the preview's last point).
    pub cursor: Option<[f64; 3]>,
    /// The text tool's text (its "Text to sketch:" field).
    pub text: String,
    /// `points` is only the last sent line's end, kept so the next line
    /// starts there (RoboCAD's chaining, tools.py:768): set when a line
    /// chains, cleared by the next press that is taken. Its segment's
    /// `began` is stamped by that press.
    pub chained: bool,
}
impl SketchState {
    pub fn new(shape: SketchShape) -> Self {
        SketchState { shape, points: Vec::new(), began: 0, cursor: None, text: String::new(), chained: false }
    }
    /// A shape is in progress: at least one point clicked (a chained
    /// point included: the preview and the readout start from it).
    pub fn in_progress(&self) -> bool {
        !self.points.is_empty()
    }
    /// Clicked points that were not sent: any point but a lone chained one
    /// (that point is the end of a line already sent).
    pub fn unsent(&self) -> bool {
        self.points.len() > usize::from(self.chained)
    }
}

/// One pick of a plane tool (`CadDocument::ops.plane_picks`).
#[derive(Clone, Debug, PartialEq)]
pub enum PlanePick {
    /// A face, its index read through `CadMeshes::face_at` at `revision`.
    Face { node: String, face: i64, revision: u64 },
    /// A snapped point (mm).
    Point([f64; 3]),
}

/// Where a finished sketch shape goes (RoboCAD's `_ensure_sketch`,
/// tools.py:675-686): an existing sketch node, or a new sketch on the
/// plane (`POST /nodes {"kind": "sketch", "plane", "calls"}`).
#[derive(Clone, Debug, PartialEq)]
pub enum SketchTarget {
    Node(String),
    /// The plane argument: "xy" | "xz" | "yz" | a plane node id.
    New { plane: Value },
}

/// Why the shape in progress blocks leaving CAD mode (`switch::leaving_blockers`):
/// clicked points not sent. A lone chained point (the last line's end,
/// already sent) loses nothing, so it does not block.
pub fn blocker(doc: &CadDocument) -> Option<String> {
    let s = doc.ops.sketch.as_ref().filter(|s| s.unsent())?;
    Some(format!("a sketch {} is in progress ({} point(s) clicked): finish it or press Escape", s.shape.name(), s.points.len()))
}

/// The cad-sketch systems (the window's; the cache's sync is the core's).
pub(in crate::cad) fn build(app: &mut App) {
    plane::build(app);
    plane_draw::build(app);
    interact::build(app);
    display::build(app);
    extrude::build(app);
}
