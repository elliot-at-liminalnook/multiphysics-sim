//! The catalogue's entries: Planes: plane from face, three points, two points and the camera, midplane, active plane XY/XZ/YZ, 2D snapping (cad-sketch).
//! In RoboCAD's registry order (`ui/app.py` `_build_commands`); see `super`.
//!
//! The four plane tools are RoboCAD's `PlaneTool` (ui/tools.py:1065-1111):
//! picks in the 3D view (`sketch::plane`), then one Ops call whose new
//! plane node becomes the active plane (`activates_plane`; RoboCAD's
//! `set_active_plane(pid)`); the tool stays active. REST `cad_run` passes
//! the picks as `items` (faces, with their `revision`) or the points as
//! parameters. The active-plane and 2D-snapping entries are viewer state
//! (`Flow::View`), never a RoboCAD call (RoboCAD's `viewport.active_plane`
//! and `plane_snapping` are its GUI's state too).
use super::super::kinds::*;
use super::super::*;
use crate::cad::sketch::BasePlane;

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    OpEntry {
        id: "tool.plane",
        label: "Plane from face",
        category: "Planes",
        keys: &["Ctrl+P"],
        needs: FACES,
        flow: Flow::PlanePick(PlaneMode::Face),
        route: "plane_from_face",
        args: &[Arg::Node, Arg::Face],
        refusal: "Select a face (or click one with the tool)",
        hint: "Click a face",
        activates_plane: true,
        source: "ui/app.py:350, ui/tools.py:1065-1111 (PlaneTool \"face\", :1092-1095: the clicked face, then set_active_plane), commands.py:874-876; keymap.json Ctrl+P; refusal (ours: RoboCAD's tool only clicks)",
        ..BASE
    },
    OpEntry {
        id: "tool.plane_three",
        label: "Plane from three points",
        category: "Planes",
        params: &[p("a", "first point", POINT, ""), p("b", "second point", POINT, ""), p("c", "third point", POINT, "")],
        flow: Flow::PlanePick(PlaneMode::Three),
        route: "plane_three_points",
        args: &[Arg::Param("a"), Arg::Param("b"), Arg::Param("c")],
        hint: "Click three points",
        activates_plane: true,
        source: "ui/app.py:351, ui/tools.py:1101-1106 (three snapped points), commands.py:878-879 (Plane.from_three_points)",
        ..BASE
    },
    OpEntry {
        id: "tool.plane_camera",
        label: "Plane from two points (camera)",
        category: "Planes",
        params: &[p("a", "first point", POINT, ""), p("b", "second point", POINT, ""), p("direction", "view direction (the view's when empty)", POINT, "")],
        flow: Flow::PlanePick(PlaneMode::Camera),
        route: "plane_two_points_camera",
        args: &[Arg::Param("a"), Arg::Param("b"), Arg::ViewDir],
        hint: "Click two points (the plane faces the camera)",
        activates_plane: true,
        source: "ui/app.py:352, ui/tools.py:1107-1111 (two snapped points, -camera.basis()[2]), commands.py:881-885",
        ..BASE
    },
    OpEntry {
        id: "tool.plane_mid",
        label: "Midplane between two faces",
        category: "Planes",
        needs: Needs::Faces { min: 2 },
        flow: Flow::PlanePick(PlaneMode::Mid),
        route: "plane_midplane",
        args: &[Arg::Node, Arg::Face, Arg::FaceB],
        refusal: "Select two faces (or click them with the tool)",
        hint: "Click two parallel faces",
        activates_plane: true,
        source: "ui/app.py:353, ui/tools.py:1096-1099 (the first face's node, both faces), commands.py:887-890; refusal (ours: RoboCAD's tool only clicks)",
        ..BASE
    },
    OpEntry {
        id: "tool.plane_xy",
        label: "Active plane: XY",
        category: "Planes",
        flow: Flow::View(ViewAct::Plane(BasePlane::Xy)),
        route: "set_active_plane",
        shape: Shape::View(ViewAct::Plane(BasePlane::Xy)),
        source: "ui/app.py:354, ui/app.py:1022-1027 (viewport.active_plane = Plane.xy(); status \"Active plane set\")",
        ..BASE
    },
    OpEntry {
        id: "tool.plane_xz",
        label: "Active plane: XZ",
        category: "Planes",
        flow: Flow::View(ViewAct::Plane(BasePlane::Xz)),
        route: "set_active_plane",
        shape: Shape::View(ViewAct::Plane(BasePlane::Xz)),
        source: "ui/app.py:355, ui/app.py:1022-1027 (Plane.xz())",
        ..BASE
    },
    OpEntry {
        id: "tool.plane_yz",
        label: "Active plane: YZ",
        category: "Planes",
        flow: Flow::View(ViewAct::Plane(BasePlane::Yz)),
        route: "set_active_plane",
        shape: Shape::View(ViewAct::Plane(BasePlane::Yz)),
        source: "ui/app.py:356, ui/app.py:1022-1027 (Plane.yz())",
        ..BASE
    },
    OpEntry {
        id: "tool.plane_2d_snap",
        label: "Toggle 2D snapping to the active plane",
        category: "Planes",
        flow: Flow::View(ViewAct::Snap2d),
        route: "toggle_plane_snapping",
        shape: Shape::View(ViewAct::Snap2d),
        source: "ui/app.py:357, ui/app.py:1029-1031 (viewport.plane_snapping toggled; status \"2D snapping on|off\"), ui/viewport.py:1374 (snaps projected onto the active plane)",
        ..BASE
    },
];
