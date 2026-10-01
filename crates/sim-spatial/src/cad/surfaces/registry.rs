//! RoboCAD's whole command table (`ui/app.py` `_build_commands`,
//! app.py:270-431: 183 commands, the loops over views, display modes,
//! selection modes and sketch shapes expanded; `tr(…)` labels resolved from
//! `ui/strings.py`) with each command's keys and its native mapping.
//!
//! Keys: RoboCAD stores a command's inline keys (`c(…, keys)`) and then
//! replaces them with keymap.json's for every id the keymap names
//! (`load_keymap`, app.py:453-469); only keymap keys become Qt shortcuts.
//! So `keys` is what RoboCAD's palette lists and `bound` says whether the
//! keys are live: `simulation.experiment` Ctrl+Return, `robot.add_motor`
//! Ctrl+Shift+M and `robot.add_joint` Ctrl+Shift+J are listed but never
//! bound (docs/cad-parity.md:1020). keymap.json's `sketch.arc` (A) names
//! no command and is dropped, as RoboCAD drops it.
//!
//! Native mapping ([`Native`]), owners from docs/cad-parity.md:
//! - `Op`: a cad-modify operation, run by the op catalogue
//!   (`ops::CATALOGUE`) when it lists the id; label, category and keys
//!   must agree with it (tests). Not (yet) in the catalogue, it reads as
//!   owned by the cad-modify epic.
//! - `Action`: an existing CAD action (undo, save, fit, a selection
//!   command or mode, a tool).
//! - `Surface`: opens the palette or a radial menu (`CadSurface`).
//! - `NumericEntry`: RoboCAD's "Numeric entry (Tab)": the first field of the
//!   open form, else of the numeric bar, takes the keyboard.
//! - `Later(epic)`: owned by a later CAD epic; refused by name.
//! - `Different(reason)`: deliberately not ported (the ledger's reason).
//!
//! No command is GUI-only: the ledger's only `POST /commands/{id}`-only
//! commands (Blender live link, web share) belong to cad-views-export, and
//! `api.address`, `help.guide` and `help.logs` are deliberately different.
use super::Surface;
use crate::app::actions::Call;
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::{CadDocument, CadTool, SelectMode};
use crate::cad::ops::{self, Flow, Needs, OpEntry};
use crate::cad::panel::Control;
use serde_json::json;
use sim_api::Outcome;

/// An existing CAD action a command stands for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Do {
    Undo,
    Redo,
    Save,
    Fit,
    SelectAll,
    Invert,
    SameMaterial,
    EdgesToFaces,
    Mode(SelectMode),
    Tool(CadTool),
}
impl Do {
    pub(crate) fn action(self) -> CadAction {
        match self {
            Do::Undo => CadAction::CadUndo,
            Do::Redo => CadAction::CadRedo,
            Do::Save => CadAction::CadSave { path: None },
            Do::Fit => CadAction::CadFit { id: None },
            Do::SelectAll => CadAction::CadSelectAll,
            Do::Invert => CadAction::CadInvertSelection,
            Do::SameMaterial => CadAction::CadSelectSameMaterial,
            Do::EdgesToFaces => CadAction::CadEdgesToFaces,
            Do::Mode(mode) => CadAction::CadSelectMode { mode },
            Do::Tool(tool) => CadAction::CadTool { tool },
        }
    }
}

/// A command surface a command opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opens {
    Palette,
    ViewRadial,
    SelectRadial,
}
impl Opens {
    /// The surface, at `at` (window logical px; the radials only).
    pub(crate) fn surface(self, at: Option<[f32; 2]>) -> Surface {
        match self {
            Opens::Palette => Surface::Palette { query: String::new() },
            Opens::ViewRadial => Surface::ViewRadial { at },
            Opens::SelectRadial => Surface::SelectRadial { at },
        }
    }
}

/// What a command maps to in the native viewer (see the module doc).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Native {
    Op,
    Action(Do),
    Surface(Opens),
    NumericEntry,
    Later(&'static str),
    Different(&'static str),
}

/// One RoboCAD command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Command {
    pub id: &'static str,
    pub label: &'static str,
    /// RoboCAD's category (its menu, or Help for the categories without one).
    pub category: &'static str,
    /// The keys RoboCAD lists for it (keymap.json's, else the inline ones).
    pub keys: &'static [&'static str],
    /// Whether the keys are Qt shortcuts in RoboCAD (keymap.json names the id).
    pub bound: bool,
    pub native: Native,
}

const fn c(id: &'static str, label: &'static str, category: &'static str, keys: &'static [&'static str], bound: bool, native: Native) -> Command {
    Command { id, label, category, keys, bound, native }
}

/// RoboCAD's menus in menu-bar order (`_build_menus`, app.py:434-438).
pub(crate) const CATEGORIES: [&str; 16] = ["File", "Edit", "View", "Select", "Create", "Sketch", "Modify", "Planes", "Inspect", "Print", "Advanced", "Outliner", "Robot", "Bridge", "Simulation", "Help"];

/// The menu a category's commands are in: its own, else Help
/// (`menus.get(c["category"], menus["Help"])`: General, Window, Tools).
pub(crate) fn menu_of(category: &str) -> &'static str {
    CATEGORIES.iter().find(|c| **c == category).copied().unwrap_or("Help")
}

/// RoboCAD's toolbar, in order (`_build_menus`, app.py:442); `tool.*`
/// entries are checkable.
pub(crate) const TOOLBAR: [&str; 25] = [
    "tool.select", "tool.annotate", "view.saved_views", "view.references", "view.pose", "view.experiments", "tool.move", "tool.rotate", "tool.scale", "tool.box", "tool.cylinder", "tool.sphere", "sketch.rectangle", "sketch.circle", "sketch.slot", "tool.extrude",
    "tool.push_pull", "tool.fillet", "tool.shell", "modify.union", "modify.subtract", "tool.fastener", "tool.measure", "view.section", "print.validate",
];

/// The viewport's right-click menu, in order (`_context_menu`, app.py:1103-1107).
pub(crate) const CONTEXT: [&str; 14] = ["tool.annotate", "view.comments", "tool.push_pull", "tool.fillet", "tool.chamfer", "tool.shell", "modify.union", "modify.subtract", "tool.mirror", "tool.array", "tool.measure", "view.isolate", "view.hide", "edit.delete"];

/// The outliner's "Make unique (bake instance)" (widgets.py:431), added to
/// the context menu when an instance is selected: Make instance unique of
/// the selected instances.
pub(crate) const MAKE_UNIQUE: (&str, &str) = ("modify.make_unique", "Make unique (bake instance)");

/// The view radial's entries (`view_radial`, app.py:1095-1097): label and command.
pub(crate) const VIEW_RADIAL: [(&str, &str); 8] = [("Front", "view.front"), ("Top", "view.top"), ("Right", "view.right"), ("Iso", "view.iso"), ("Ortho", "view.ortho"), ("Grid", "view.grid"), ("Mode", "view.mode_next"), ("Fit", "view.fit")];

/// The selection-mode radial's entries (`selection_radial`, app.py:1099-1101).
pub(crate) const SELECT_RADIAL: [(&str, &str); 5] = [("Body", "select.body"), ("Face", "select.face"), ("Edge", "select.edge"), ("Vertex", "select.vertex"), ("Point", "select.point")];

/// Every command, in RoboCAD's registry order.
pub(crate) static COMMANDS: &[Command] = &[
    c("view.references", "References", "View", &[], false, Native::Later("cad-organize")),
    c("reference.import", "Add reference images…", "File", &[], false, Native::Later("cad-organize")),
    c("view.pose", "Pose", "View", &[], false, Native::Later("cad-experiments-motion")),
    c("view.experiments", "Experiments", "View", &[], false, Native::Later("cad-experiments-motion")),
    c("simulation.experiment", "Run captured experiment", "Simulation", &["Ctrl+Return"], false, Native::Later("cad-experiments-motion")),
    c("robot.pose", "Preview joint motion", "Robot", &[], false, Native::Later("cad-experiments-motion")),
    c("tool.annotate", "Annotate", "Inspect", &["N"], true, Native::Later("cad-organize")),
    c("view.comments", "Comments panel", "View", &[], false, Native::Later("cad-organize")),
    c("view.saved_views", "Saved Views", "View", &[], false, Native::Later("cad-views-export")),
    c("view.comment_pins", "Toggle comment pins", "View", &[], false, Native::Later("cad-organize")),
    c("command_palette", "Command palette", "General", &["Ctrl+Space", "Shift+F"], true, Native::Surface(Opens::Palette)),
    c("file.new", "New", "File", &["Ctrl+N"], true, Native::Later("cad-views-export")),
    c("file.open", "Open…", "File", &["Ctrl+O"], true, Native::Later("cad-views-export")),
    c("file.save", "Save", "File", &["Ctrl+S"], true, Native::Action(Do::Save)),
    c("file.save_as", "Save As…", "File", &["Ctrl+Shift+S"], true, Native::Later("cad-views-export")),
    c("file.import", "Import…", "File", &["Ctrl+I"], true, Native::Later("cad-views-export")),
    c("file.export", "Export…", "File", &["Ctrl+E"], true, Native::Later("cad-views-export")),
    c("file.export_drawing", "Export drawing (SVG)…", "File", &["Ctrl+Shift+D"], true, Native::Later("cad-views-export")),
    c("file.quit", "Quit", "File", &[], false, Native::Different("one native app with modes: leaving CAD mode is the app's mode switch, not a RoboCAD window close")),
    c("edit.undo", "Undo", "Edit", &["Ctrl+Z"], true, Native::Action(Do::Undo)),
    c("edit.redo", "Redo", "Edit", &["Ctrl+Shift+Z"], true, Native::Action(Do::Redo)),
    c("edit.delete", "Delete", "Edit", &["Delete", "Backspace"], true, Native::Op),
    c("edit.copy", "Copy with Placement", "Edit", &["Ctrl+C"], true, Native::Op),
    c("edit.paste", "Paste with Placement", "Edit", &["Ctrl+V"], true, Native::Op),
    c("edit.select_all", "Select All", "Edit", &["Ctrl+A"], true, Native::Action(Do::SelectAll)),
    c("edit.invert", "Invert Selection", "Edit", &["Ctrl+Shift+I"], true, Native::Action(Do::Invert)),
    c("edit.select_same_material", "Select Same Material", "Edit", &["Ctrl+Shift+M"], true, Native::Action(Do::SameMaterial)),
    c("edit.convert_faces", "Selection: edges → bounding faces", "Edit", &[], false, Native::Action(Do::EdgesToFaces)),
    c("edit.preferences", "Preferences…", "Edit", &[], false, Native::Later("cad-views-export")),
    c("view.fit", "Fit All", "View", &["Home"], true, Native::Action(Do::Fit)),
    c("view.focus", "Focus Selection", "View", &["F"], true, Native::Later("cad-views-export")),
    c("view.front", "View front", "View", &["1"], true, Native::Later("cad-views-export")),
    c("view.back", "View back", "View", &["Ctrl+1"], true, Native::Later("cad-views-export")),
    c("view.top", "View top", "View", &["7"], true, Native::Later("cad-views-export")),
    c("view.bottom", "View bottom", "View", &["Ctrl+7"], true, Native::Later("cad-views-export")),
    c("view.right", "View right", "View", &["3"], true, Native::Later("cad-views-export")),
    c("view.left", "View left", "View", &["Ctrl+3"], true, Native::Later("cad-views-export")),
    c("view.iso", "View iso", "View", &["0"], true, Native::Later("cad-views-export")),
    c("view.ortho", "Orthographic", "View", &["5"], true, Native::Later("cad-views-export")),
    c("view.grid", "Grid", "View", &["Ctrl+G"], true, Native::Later("cad-views-export")),
    c("view.mode_next", "Next display mode", "View", &["Z"], true, Native::Later("cad-views-export")),
    c("view.mode.shaded", "Display: shaded", "View", &[], false, Native::Later("cad-views-export")),
    c("view.mode.shaded_edges", "Display: shaded edges", "View", &[], false, Native::Later("cad-views-export")),
    c("view.mode.wireframe", "Display: wireframe", "View", &[], false, Native::Later("cad-views-export")),
    c("view.mode.xray", "Display: xray", "View", &[], false, Native::Later("cad-views-export")),
    c("view.mode.matcap", "Display: matcap", "View", &[], false, Native::Later("cad-views-export")),
    c("view.mode.render", "Display: render", "View", &[], false, Native::Later("cad-views-export")),
    c("view.orbit_mode", "Toggle orbit: turntable / trackball", "View", &[], false, Native::Later("cad-views-export")),
    c("view.fov", "Set field of view…", "View", &[], false, Native::Later("cad-views-export")),
    c("view.isolate", "Isolate", "View", &["/"], true, Native::Later("cad-views-export")),
    c("view.show_all", "Show All", "View", &["Alt+H"], true, Native::Later("cad-views-export")),
    c("view.hide", "Hide", "View", &["H"], true, Native::Later("cad-views-export")),
    c("view.section", "Section Analysis", "Inspect", &["Ctrl+Shift+X"], true, Native::Later("cad-views-export")),
    c("view.build_plate", "Build Plate Preview", "Print", &["Ctrl+Shift+B"], true, Native::Later("cad-views-export")),
    c("view.high_contrast", "High-Contrast Theme", "View", &[], false, Native::Later("cad-views-export")),
    c("view.radial", "View radial menu", "View", &["Space"], true, Native::Surface(Opens::ViewRadial)),
    c("select.body", "Select bodys", "Select", &["B"], true, Native::Action(Do::Mode(SelectMode::Body))),
    c("select.face", "Select faces", "Select", &["Shift+B"], true, Native::Action(Do::Mode(SelectMode::Face))),
    c("select.edge", "Select edges", "Select", &["E"], true, Native::Action(Do::Mode(SelectMode::Edge))),
    c("select.vertex", "Select vertexs", "Select", &["V"], true, Native::Action(Do::Mode(SelectMode::Vertex))),
    c("select.point", "Select points", "Select", &["P"], true, Native::Action(Do::Mode(SelectMode::Point))),
    c("select.mode_radial", "Selection-mode radial menu", "Select", &["Q"], true, Native::Surface(Opens::SelectRadial)),
    c("tool.select", "Select tool", "Tools", &["Escape"], true, Native::Action(Do::Tool(CadTool::Select))),
    c("tool.move", "Move", "Tools", &["G"], true, Native::Action(Do::Tool(CadTool::Move))),
    c("tool.rotate", "Rotate", "Tools", &["R"], true, Native::Action(Do::Tool(CadTool::Rotate))),
    c("tool.scale", "Scale", "Tools", &["S"], true, Native::Action(Do::Tool(CadTool::Scale))),
    c("tool.push_pull", "Push/Pull face", "Modify", &["D"], true, Native::Action(Do::Tool(CadTool::PushPull))),
    c("tool.offset_face", "Offset face", "Modify", &["Shift+D"], true, Native::Action(Do::Tool(CadTool::OffsetFace))),
    c("tool.box", "Box (corner)", "Create", &["Shift+A, B"], true, Native::Op),
    c("tool.box_center", "Box (centre)", "Create", &[], false, Native::Op),
    c("tool.cylinder", "Cylinder", "Create", &["Shift+A, C"], true, Native::Op),
    c("tool.sphere", "Sphere", "Create", &["Shift+A, S"], true, Native::Op),
    c("tool.extrude", "Extrude", "Create", &["X"], true, Native::Later("cad-sketch")),
    c("tool.revolve", "Revolve", "Create", &["Shift+R"], true, Native::Later("cad-sketch")),
    c("tool.sweep", "Sweep (profile + path from selection)", "Create", &[], false, Native::Later("cad-sketch")),
    c("tool.pipe", "Pipe along selected curve…", "Create", &[], false, Native::Later("cad-sketch")),
    c("tool.loft", "Loft selected sketches", "Create", &[], false, Native::Later("cad-sketch")),
    c("tool.fill", "Fill / patch selected curve", "Create", &[], false, Native::Later("cad-sketch")),
    c("tool.fillet", "Fillet", "Modify", &["Ctrl+F"], true, Native::Op),
    c("tool.fillet_variable", "Variable fillet", "Modify", &[], false, Native::Op),
    c("tool.fillet_chordal", "Chordal fillet", "Modify", &[], false, Native::Op),
    c("tool.fillet_all", "Fillet all edges…", "Modify", &[], false, Native::Op),
    c("tool.full_round", "Full round (two edges)", "Modify", &[], false, Native::Op),
    c("tool.remove_fillets", "Remove fillets (selected faces)", "Modify", &[], false, Native::Op),
    c("tool.chamfer", "Chamfer", "Modify", &["Ctrl+Shift+F"], true, Native::Op),
    c("tool.shell", "Hollow / shell", "Modify", &["Ctrl+Shift+H"], true, Native::Op),
    c("tool.thicken", "Thicken sheet…", "Modify", &[], false, Native::Op),
    c("tool.draft", "Draft faces…", "Modify", &[], false, Native::Op),
    c("tool.delete_face", "Delete faces (heal)", "Modify", &[], false, Native::Op),
    c("tool.measure", "Measure", "Inspect", &["M"], true, Native::Action(Do::Tool(CadTool::Measure))),
    c("tool.plane", "Plane from face", "Planes", &["Ctrl+P"], true, Native::Later("cad-sketch")),
    c("tool.plane_three", "Plane from three points", "Planes", &[], false, Native::Later("cad-sketch")),
    c("tool.plane_camera", "Plane from two points (camera)", "Planes", &[], false, Native::Later("cad-sketch")),
    c("tool.plane_mid", "Midplane between two faces", "Planes", &[], false, Native::Later("cad-sketch")),
    c("tool.plane_xy", "Active plane: XY", "Planes", &[], false, Native::Later("cad-sketch")),
    c("tool.plane_xz", "Active plane: XZ", "Planes", &[], false, Native::Later("cad-sketch")),
    c("tool.plane_yz", "Active plane: YZ", "Planes", &[], false, Native::Later("cad-sketch")),
    c("tool.plane_2d_snap", "Toggle 2D snapping to the active plane", "Planes", &[], false, Native::Later("cad-sketch")),
    c("tool.fastener", "Fastener hole…", "Print", &["Ctrl+H"], true, Native::Later("cad-print")),
    c("tool.clearance", "Clearance offset…", "Print", &["Ctrl+Shift+C"], true, Native::Later("cad-print")),
    c("tool.mirror", "Mirror (about active plane)", "Modify", &["Ctrl+M"], true, Native::Op),
    c("tool.mirror_live", "Mirror as live instance", "Modify", &[], false, Native::Op),
    c("components.show", "Components library", "Window", &[], false, Native::Later("cad-organize")),
    c("components.make", "Make linked component…", "Create", &[], false, Native::Later("cad-organize")),
    c("tool.instance", "Instance selected", "Modify", &[], false, Native::Op),
    c("tool.array", "Array…", "Modify", &["Ctrl+Shift+A"], true, Native::Op),
    c("tool.cut_plane", "Cut with active plane", "Modify", &[], false, Native::Op),
    c("tool.cut_sheet", "Cut with selected sheet/curve", "Modify", &[], false, Native::Op),
    c("tool.split_face", "Split faces with active plane", "Modify", &[], false, Native::Op),
    c("tool.imprint", "Imprint selected curve/body", "Modify", &[], false, Native::Op),
    c("tool.project_curve", "Project curve onto body", "Modify", &[], false, Native::Op),
    c("tool.silhouette", "Silhouette onto active plane", "Modify", &[], false, Native::Op),
    c("tool.control_points", "Show/edit control points (advanced)", "Advanced", &[], false, Native::Op),
    c("tool.raise_degree", "Raise face degree", "Advanced", &[], false, Native::Op),
    c("tool.rebuild_face", "Rebuild face…", "Advanced", &[], false, Native::Op),
    c("tool.dependent_offset", "Dependent offset (face to body)…", "Modify", &[], false, Native::Op),
    c("tool.set_pivot", "Set pivot at cursor snap", "Tools", &[], false, Native::Op),
    c("sketch.line", "Sketch: Line", "Sketch", &["L"], true, Native::Later("cad-sketch")),
    c("sketch.rectangle", "Sketch: Rectangle", "Sketch", &["Shift+L"], true, Native::Later("cad-sketch")),
    c("sketch.rectangle_center", "Sketch: Rectangle (centre)", "Sketch", &[], false, Native::Later("cad-sketch")),
    c("sketch.circle", "Sketch: Circle", "Sketch", &["C"], true, Native::Later("cad-sketch")),
    c("sketch.circle_2pt", "Sketch: Circle (two points)", "Sketch", &[], false, Native::Later("cad-sketch")),
    c("sketch.circle_3pt", "Sketch: Circle (three points)", "Sketch", &[], false, Native::Later("cad-sketch")),
    c("sketch.arc_3pt", "Sketch: Arc (three points)", "Sketch", &[], false, Native::Later("cad-sketch")),
    c("sketch.polygon", "Sketch: Polygon", "Sketch", &["Shift+P"], true, Native::Later("cad-sketch")),
    c("sketch.slot", "Sketch: Slot", "Sketch", &["Shift+S"], true, Native::Later("cad-sketch")),
    c("sketch.spline", "Sketch: Spline", "Sketch", &["Shift+C"], true, Native::Later("cad-sketch")),
    c("sketch.ellipse", "Sketch: Ellipse", "Sketch", &[], false, Native::Later("cad-sketch")),
    c("sketch.spiral", "Sketch: Spiral", "Sketch", &[], false, Native::Later("cad-sketch")),
    c("sketch.text", "Sketch: Text", "Sketch", &["T"], true, Native::Later("cad-sketch")),
    c("sketch.offset", "Sketch: offset selected curve…", "Sketch", &[], false, Native::Later("cad-sketch")),
    c("sketch.fillet", "Sketch: fillet corner…", "Sketch", &[], false, Native::Later("cad-sketch")),
    c("sketch.join", "Sketch: join curves", "Sketch", &[], false, Native::Later("cad-sketch")),
    c("modify.union", "Union", "Modify", &["Ctrl+U"], true, Native::Op),
    c("modify.subtract", "Subtract", "Modify", &["Ctrl+Shift+U"], true, Native::Op),
    c("modify.intersect", "Intersect", "Modify", &["Ctrl+Alt+U"], true, Native::Op),
    c("modify.region", "Region (overlap as new body)", "Modify", &[], false, Native::Op),
    c("modify.join", "Join", "Modify", &["J"], true, Native::Op),
    c("modify.unjoin", "Unjoin", "Modify", &["Shift+J"], true, Native::Op),
    c("modify.dissolve", "Dissolve redundant topology", "Modify", &[], false, Native::Op),
    c("modify.make_unique", "Make instance unique", "Modify", &[], false, Native::Op),
    c("print.wall_check", "Wall thickness check…", "Print", &["Ctrl+W"], true, Native::Later("cad-print")),
    c("print.validate", "Validate for printing", "Print", &["Ctrl+Shift+V"], true, Native::Later("cad-print")),
    c("print.overhangs", "Toggle overhang shading", "Print", &[], false, Native::Later("cad-print")),
    c("print.split", "Split selected for printing…", "Print", &[], false, Native::Later("cad-print")),
    c("print.strength", "Check strength (document's print study)", "Print", &[], false, Native::Later("cad-print")),
    c("print.plan", "Plan print settings and plates (document's print study)", "Print", &[], false, Native::Later("cad-print")),
    c("print.strength_split", "Whole or split for strength? (selected part of the print study)", "Print", &[], false, Native::Later("cad-print")),
    c("print.assembly", "Assembly guide for the selected split…", "Print", &[], false, Native::Later("cad-print")),
    c("print.coupons", "Test coupons (for the selected split, or the material)…", "Print", &[], false, Native::Later("cad-print")),
    c("print.overlay", "Strength overlay on/off", "Print", &[], false, Native::Later("cad-physical-inspect")),
    c("print.jobs", "Print jobs…", "Print", &[], false, Native::Later("cad-print")),
    c("inspect.curvature", "Curvature comb on selected curve", "Inspect", &[], false, Native::Op),
    c("inspect.continuity", "Continuity check (G0/G1/G2)", "Inspect", &[], false, Native::Op),
    c("inspect.draft", "Draft-angle shading", "Inspect", &[], false, Native::Later("cad-views-export")),
    c("inspect.normals", "Normal-direction shading", "Inspect", &[], false, Native::Later("cad-views-export")),
    c("bridge.start", "Live link: start (Blender)", "Bridge", &[], false, Native::Later("cad-views-export")),
    c("bridge.stop", "Live link: stop", "Bridge", &[], false, Native::Later("cad-views-export")),
    c("bridge.share", "Web share: publish viewer…", "Bridge", &[], false, Native::Later("cad-views-export")),
    c("robot.add_motor", "Robot: add motor from library…", "Robot", &["Ctrl+Shift+M"], false, Native::Later("cad-physical-inspect")),
    c("robot.add_joint", "Robot: add joint (click parent, child, axis face)", "Robot", &["Ctrl+Shift+J"], false, Native::Later("cad-physical-inspect")),
    c("robot.joint_dialog", "Robot: joint from the two selected bodies…", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.infer", "Robot: infer joints from coaxial holes and pins", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.assign_motor", "Robot: assign selected motor to a joint…", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.fixed", "Robot: fix selected bodies together (first is the parent)", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.ground", "Robot: toggle ground on selected bodies", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.validate", "Robot: validate", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.motors", "Robot: motor library…", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.add_sensor", "Robot: add sensor (IMU, encoder, current, force)…", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.add_cable", "Robot: add cable between bodies…", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.power", "Robot: battery, control loop and uncertainty…", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.load_results", "Robot: load simulation results…", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("robot.apply_identification", "Robot: apply identified joint parameters…", "Robot", &[], false, Native::Later("cad-physical-inspect")),
    c("view.stress", "Toggle stress overlay (from loaded results)", "Inspect", &[], false, Native::Later("cad-physical-inspect")),
    c("sim.export_physical", "Simulation: export physical model (simrobot v4, with flexible links)…", "Simulation", &[], false, Native::Later("cad-physical-inspect")),
    c("sim.export", "Simulation: export robot model…", "Simulation", &[], false, Native::Later("cad-physical-inspect")),
    c("sim.link", "Simulation: live link (watch + run viewer)", "Simulation", &[], false, Native::Later("cad-physical-inspect")),
    c("api.address", "REST API: show address", "Bridge", &[], false, Native::Different("the header always shows the service URL, so no dialog is needed")),
    c("group.set_active", "Set selected group as active", "Outliner", &[], false, Native::Later("cad-organize")),
    c("group.group", "Group selection", "Outliner", &[], false, Native::Later("cad-organize")),
    c("numeric.entry", "Numeric entry (Tab)", "General", &["Tab"], true, Native::NumericEntry),
    c("help.guide", "User guide", "Help", &[], false, Native::Different("RoboCAD shows only a path; the viewer's docs live in the repository")),
    c("help.logs", "Open diagnostics folder", "Help", &[], false, Native::Different("the viewer reports the log of the service it started; RoboCAD's own session logs stay in RoboCAD")),
];

/// The command with RoboCAD id `id`.
pub(crate) fn command(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// What a command does here now.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Resolved {
    Op(&'static OpEntry),
    Action(CadAction),
    Surface(Opens),
    NumericEntry,
    Later(&'static str),
    Different(&'static str),
}

/// A command's mapping: the catalogue's operation when the catalogue lists
/// the id (whatever the table says), else the table's.
pub(crate) fn resolve(cmd: &Command) -> Resolved {
    if let Some(entry) = ops::entry(cmd.id) {
        return Resolved::Op(entry);
    }
    match cmd.native {
        Native::Op => Resolved::Later("cad-modify"),
        Native::Action(d) => Resolved::Action(d.action()),
        Native::Surface(o) => Resolved::Surface(o),
        Native::NumericEntry => Resolved::NumericEntry,
        Native::Later(epic) => Resolved::Later(epic),
        Native::Different(why) => Resolved::Different(why),
    }
}

/// A later epic's refusal.
pub(crate) fn later(cmd: &Command, epic: &str) -> String {
    format!("{} belongs to the {epic} epic; not in the native viewer yet", cmd.label)
}

/// RoboCAD's desktop window serves the document (its `/commands` work).
pub(crate) fn gui(doc: &CadDocument) -> bool {
    doc.health.as_ref().is_some_and(|h| h.gui)
}

/// The palette's note for a command that does not run natively: the owning
/// epic or "not ported"; empty for a native one.
pub(crate) fn note(cmd: &Command) -> String {
    match resolve(cmd) {
        Resolved::Later(epic) => epic.to_string(),
        Resolved::Different(_) => "not ported".to_string(),
        _ => String::new(),
    }
}

/// The node's kind in the shown tree.
fn kind_of<'a>(doc: &'a CadDocument, id: &str) -> Option<&'a str> {
    doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).map(|n| n.kind.as_str())
}

/// Whether the selection fits a catalogue operation's [`Needs`] (the same
/// rule as `ops::resolve`'s, without its revision and index checks, which
/// it applies when the operation runs): Err(the entry's refusal). A
/// pick-then-form operation is always ready: it is started to pick.
pub(crate) fn readiness(entry: &OpEntry, doc: &CadDocument) -> Result<(), String> {
    if matches!(entry.flow, Flow::PickThenForm(_)) {
        return Ok(());
    }
    let nodes = doc.selected_nodes();
    let edges = doc.selected_of("edge");
    let faces = doc.selected_of("face");
    let fits = match entry.needs {
        Needs::Nothing => true,
        Needs::Nodes { min, max, kinds } => {
            let n = nodes.iter().filter(|n| kinds.is_empty() || kind_of(doc, n).is_some_and(|k| kinds.contains(&k))).count();
            n >= min && max.is_none_or(|m| n <= m)
        }
        Needs::TargetThenTools => nodes.len() >= 2,
        Needs::Edges { min, max, same_node } => {
            let n = edges.len();
            n >= min && max.is_none_or(|m| n <= m) && !(same_node && edges.iter().any(|(node, _)| *node != edges[0].0))
        }
        Needs::Faces { min } => faces.len() >= min,
        Needs::NodesWithFaces => !nodes.is_empty(),
        Needs::FaceThenNode => !faces.is_empty() && nodes.iter().any(|n| !faces.iter().any(|(f, _)| f == n)),
        Needs::NodesThenPath => nodes.len() >= 2 && nodes.last().is_some_and(|n| kind_of(doc, n).is_none_or(|k| k == "curve" || k == "sketch")),
    };
    if fits { Ok(()) } else { Err(entry.refusal.to_string()) }
}

/// Whether `cmd` can run now, else why not. `own` is the panel's own
/// controls (`panel::own_controls`): an action command is ready exactly
/// when its button is. A catalogue operation first needs an edit to be
/// sendable (`CadDocument::edit_refusal`, as `cad:delete`), then its
/// selection ([`readiness`]).
pub(crate) fn ready(cmd: &Command, doc: &CadDocument, own: &[Control]) -> Result<(), String> {
    match resolve(cmd) {
        // As every edit: nothing runs while one is in flight or unconnected.
        Resolved::Op(entry) => doc.edit_refusal().map_or_else(|| readiness(entry, doc), Err),
        Resolved::Action(action) => own.iter().find(|c| c.action == action).map_or(Ok(()), |c| c.ready.clone()),
        Resolved::Surface(_) => Ok(()),
        Resolved::NumericEntry => numeric_entry_ready(doc),
        Resolved::Later(epic) => Err(later(cmd, epic)),
        Resolved::Different(why) => Err(format!("{} is not ported: {why}", cmd.label)),
    }
}

/// A refusal as the status line shows it: prefixed with the label unless
/// it already names the command.
pub(crate) fn status_line(cmd: &Command, why: &str) -> String {
    if why.starts_with(cmd.label) { why.to_string() } else { format!("{}: {why}", cmd.label) }
}

fn numeric_entry_ready(doc: &CadDocument) -> Result<(), String> {
    let form_field = doc.ops.form.as_ref().is_some_and(|f| super::form::first_text_field(f).is_some());
    if form_field || !doc.tool_state.numeric.fields.is_empty() { Ok(()) } else { Err("the active tool and form have no fields to type into".to_string()) }
}

/// RoboCAD's "Numeric entry (Tab)": the open form's first text field takes
/// the keyboard, else the numeric bar's (`numeric::sync` focuses it on
/// `focus_request`). Focus is display state, set here for REST's sake; a
/// key or click focuses through the form's and the bar's own input systems.
fn numeric_entry(doc: &mut CadDocument) -> Result<serde_json::Value, String> {
    numeric_entry_ready(doc)?;
    if let Some(form) = doc.ops.form.as_mut()
        && let Some(i) = super::form::first_text_field(form)
    {
        form.focus = Some(i);
        form.select_all = true;
        return Ok(json!({"focused": "form", "field": i}));
    }
    doc.tool_state.numeric.focus_request = true;
    Ok(json!({"focused": "numeric bar", "field": 0}))
}

/// `CadInvoke` of a RoboCAD command that is not a catalogue operation (see
/// `surfaces::invoke_command`).
pub(in crate::cad) fn invoke(id: &str, call: &mut Call, cx: &mut Cx) -> Outcome {
    let Some(cmd) = command(id) else { return Outcome::Done(Err(format!("unknown RoboCAD command {id}"))) };
    match resolve(cmd) {
        // Only reached when called with a catalogue id directly: the catalogue runs it.
        Resolved::Op(_) => crate::cad::ops::handle(&CadAction::CadInvoke { id: id.to_string() }, call, cx),
        Resolved::Action(action) => crate::cad::actions::handle(&action, call, cx),
        Resolved::Surface(o) => super::handle(&CadAction::CadSurface { surface: o.surface(None) }, call, cx),
        Resolved::NumericEntry => Outcome::Done(numeric_entry(cx.doc)),
        Resolved::Later(epic) => Outcome::Done(Err(later(cmd, epic))),
        Resolved::Different(why) => Outcome::Done(Err(format!("{} is not ported: {why}", cmd.label))),
    }
}
