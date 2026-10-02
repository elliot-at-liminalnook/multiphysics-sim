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
//! bound in RoboCAD (docs/cad-parity.md:1086). Here, deliberately,
//! `robot.add_joint` Ctrl+Shift+J is bound (no other command lists it, and
//! RoboCAD's USER_GUIDE.md:375 documents it), while `robot.add_motor`'s
//! Ctrl+Shift+M stays unbound: RoboCAD's live binding of that key is
//! `edit.select_same_material` (keymap.json), which it keeps running here
//! (the clash table in `cad::keys`). keymap.json's `sketch.arc` (A) names
//! no command and RoboCAD drops it; here A is bound to `sketch.arc_3pt`
//! instead, deliberately (the ledger's cad-sketch row: bind A to the
//! three-point arc, as RoboCAD's USER_GUIDE.md:181 documents "`A` arc"),
//! so `sketch.arc_3pt` lists A and is bound where RoboCAD lists no key.
//!
//! Native mapping ([`Native`]), owners from docs/cad-parity.md:
//! - `Op`: an operation run by the op catalogue (`ops::CATALOGUE`) when
//!   it lists the id: cad-modify's, and cad-sketch's Create solids
//!   (extrude, revolve, sweep, pipe, loft, fill), Planes and Sketch
//!   commands, which now run natively; label, category and keys must
//!   agree with the catalogue (tests). Not (yet) in the catalogue, it
//!   reads as owned by the cad-modify epic.
//! - `Action`: an existing CAD action (undo, save, fit, a selection
//!   command or mode, a tool).
//! - `Surface`: opens the palette or a radial menu (`CadSurface`).
//! - `NumericEntry`: RoboCAD's "Numeric entry (Tab)": the first field of the
//!   open form, else of the numeric bar, takes the keyboard.
//! - `Camera(cmd)` (cad-views-export): a camera intent, written as
//!   `Act<CameraAction>` through `Cx::camera` (a named view, ortho, the
//!   orbit mode), the field-of-view entry (`views::open_fov`, RoboCAD's
//!   5–120° dialog) or Focus Selection (frames the selected nodes and their
//!   descendants, RoboCAD's `focus_selection`; Fit All with nothing selected).
//! - `Later(epic)`: owned by a later CAD epic; refused by name.
//! - `Different(reason)`: deliberately not ported (the ledger's reason).
//!
//! cad-views-export maps the View rows: presets, ortho, orbit mode, FOV and
//! focus to `Camera`; grid, display modes (and `inspect.normals`, which
//! RoboCAD makes xray), section, build plate and high contrast to
//! `CadDisplay`/`CadSection` (`Do::Display`); Saved Views to its panel
//! (`Do::SavedViews`); Isolate, Hide and Show All to catalogue operations
//! (`ops::catalogue::view`). Left deliberately different: draft-angle
//! shading, the Blender live link and web share (no headless route; the
//! reasons below). The `file.*` rows (new, open, save as, import, export,
//! export drawing) are `Do::File`, the files part's actions
//! (`files::command_action`); `edit.preferences` is deliberately different
//! (its autosave timer is RoboCAD's desktop one).
use super::Surface;
use crate::app::actions::Call;
use crate::cad::actions::{CadAction, Cx};
use crate::cad::display::{DisplayArgs, DisplayMode, DisplaySetting, SectionArgs};
use crate::cad::document::{CadDocument, CadTool, SelectMode};
use crate::cad::views::{ViewsArgs, ViewsOp};
use crate::camera::{CameraAction, ViewPreset};
use crate::cad::ops::{self, Flow, Needs, OpEntry};
use crate::cad::panel::Control;
use crate::cad::selection::CadItems;
use sim_runtime::cad_client::SelectionItem;
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
    /// A display command (cad-views-export): `CadDisplay` or `CadSection`.
    Display(DisplayCmd),
    /// RoboCAD's Saved Views panel (shown).
    SavedViews,
    /// A file command (cad-views-export): `files::command_action(id)`
    /// (new, open, save as, import, export, export drawing; the path form
    /// opens without a path).
    File(&'static str),
    /// A cad-physical-inspect command that is not a catalogue operation
    /// (validate, the motor library, results, identification, the stress
    /// overlay, physical export, the live link): `robot::command_action(id)`.
    Physical(&'static str),
    /// A cad-print command that is not a catalogue operation (overhang
    /// shading, the print jobs panel): `print::command_action(id)`.
    Print(&'static str),
    /// A cad-organize command that is not a catalogue operation (the
    /// Comments and References docks, Annotate, comment pins, Add
    /// reference images…): [`organize_action`].
    Organize(&'static str),
}

/// The action of a cad-organize command id: the outliner's
/// (`tree::command_action`), the threads' (`threads::command_action`) or
/// the references' (`references::command_action`).
pub(crate) fn organize_action(id: &str) -> Option<CadAction> {
    crate::cad::tree::command_action(id).or_else(|| crate::cad::threads::command_action(id)).or_else(|| crate::cad::references::command_action(id)).or_else(|| crate::cad::components::command_action(id)).or_else(|| crate::cad::experiments::command_action(id)).or_else(|| crate::cad::motion::command_action(id))
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
            Do::Display(d) => d.action(),
            // Shown and raised, never hidden, as RoboCAD's `view.saved_views` (the panel's Close hides it).
            Do::SavedViews => CadAction::CadViews(ViewsArgs { op: ViewsOp::Panel, open: Some(true), ..ViewsArgs::default() }),
            // Every `Do::File` id is one `files::command_action` maps (`surfaces::tests`).
            Do::File(id) => crate::cad::files::command_action(id).unwrap_or_else(|| panic!("{id}: no file action")),
            // Every `Do::Physical` id is one `robot::command_action` maps (`surfaces::tests`).
            Do::Physical(id) => crate::cad::robot::command_action(id).unwrap_or_else(|| panic!("{id}: no physical action")),
            // Every `Do::Print` id is one `print::command_action` maps (`surfaces::tests`).
            Do::Print(id) => crate::cad::print::command_action(id).unwrap_or_else(|| panic!("{id}: no print action")),
            // Every `Do::Organize` id is one `organize_action` maps (`surfaces::tests`).
            Do::Organize(id) => organize_action(id).unwrap_or_else(|| panic!("{id}: no organize action")),
        }
    }
}

/// RoboCAD's display commands (`toggle_grid`, `next_display_mode`,
/// `set_display_mode`, `toggle_section`, `toggle_build_plate`,
/// `toggle_high_contrast`, app.py:1035-1086): display only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DisplayCmd {
    Next,
    Mode(DisplayMode),
    Grid,
    BuildPlate,
    HighContrast,
    /// The section tool on or off (`cad_section` with no arguments toggles).
    Section,
}
impl DisplayCmd {
    pub(crate) fn action(self) -> CadAction {
        let toggle = |setting| CadAction::CadDisplay(DisplayArgs { toggle: Some(setting), ..DisplayArgs::default() });
        match self {
            DisplayCmd::Next => CadAction::CadDisplay(DisplayArgs { next: true, ..DisplayArgs::default() }),
            DisplayCmd::Mode(mode) => CadAction::CadDisplay(DisplayArgs { mode: Some(mode), ..DisplayArgs::default() }),
            DisplayCmd::Grid => toggle(DisplaySetting::Grid),
            DisplayCmd::BuildPlate => toggle(DisplaySetting::BuildPlate),
            DisplayCmd::HighContrast => toggle(DisplaySetting::HighContrast),
            DisplayCmd::Section => CadAction::CadSection(SectionArgs::default()),
        }
    }
}

/// RoboCAD's camera commands (`Camera.set_view`, `toggle_ortho`,
/// `toggle_orbit_mode`, `set_fov`, `focus_selection`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CameraCmd {
    /// A named view (`view.front` … `view.iso`).
    Preset(ViewPreset),
    Ortho,
    OrbitMode,
    /// Opens the field-of-view entry (degrees, 5–120).
    Fov,
    /// Frames the selected nodes (Fit All when nothing is selected).
    Focus,
}
impl CameraCmd {
    /// The camera intent it writes; None for the FOV entry and Focus, which
    /// are not one camera action.
    pub(crate) fn action(self) -> Option<CameraAction> {
        match self {
            CameraCmd::Preset(view) => Some(CameraAction::View { view }),
            CameraCmd::Ortho => Some(CameraAction::Projection { orthographic: None }),
            CameraCmd::OrbitMode => Some(CameraAction::OrbitMode { mode: None }),
            CameraCmd::Fov | CameraCmd::Focus => None,
        }
    }
}

/// `inspect.draft`'s reason (docs/cad-parity.md:375).
const DRAFT_SHADING: &str = "RoboCAD colours each selected body's triangles by draft against +Z through its overhang flags (ui/app.py:1304-1317, analysis.draft_angle_colors); no route serves those colours and the native meshes carry no per-triangle colours to draw them";
/// `bridge.start` and `bridge.stop`'s reason (docs/cad-parity.md:700).
const BLENDER_LINK: &str = "RoboCAD-GUI-only: the Blender live link is a websocket server inside RoboCAD's desktop window (POST /commands/bridge.start|stop, ui/app.py:1497-1509); a headless service has no route for it and the native viewer has no Blender bridge";
/// `edit.preferences`' reason (ui/app.py:1486-1494).
const PREFERENCES: &str = "RoboCAD's Preferences dialog sets its desktop autosave timer (a headless service has none: /autosave answers 409, and the left dock's Autosave line shows RoboCAD's own state) and its viewport grid step; the native grid is RoboCAD's default 10 mm step";
/// `bridge.share`'s reason (docs/cad-parity.md:701).
const WEB_SHARE: &str = "RoboCAD-GUI-only: web share writes one HTML viewer through a desktop save dialog (POST /commands/bridge.share, ui/app.py:1511-1517); no headless route serves it (export a mesh with File > Export instead)";

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
    Camera(CameraCmd),
    #[cfg(test)]
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

/// The right-click menu's Sketch section, after RoboCAD's [`CONTEXT`]: the
/// 13 sketch tools in RoboCAD's registry order (app.py:377). A native
/// addition (RoboCAD's `_context_menu` has no sketch tools), made for the
/// cad-sketch epic's surfaces requirement: every sketch tool reachable from
/// the 3D view's own menu.
pub(crate) const SKETCH_CONTEXT: [&str; 13] = [
    "sketch.line", "sketch.rectangle", "sketch.rectangle_center", "sketch.circle", "sketch.circle_2pt", "sketch.circle_3pt", "sketch.arc_3pt", "sketch.polygon", "sketch.slot", "sketch.spline", "sketch.ellipse", "sketch.spiral", "sketch.text",
];

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
    c("view.references", "References", "View", &[], false, Native::Action(Do::Organize("view.references"))),
    c("reference.import", "Add reference images…", "File", &[], false, Native::Action(Do::Organize("reference.import"))),
    c("view.pose", "Pose", "View", &[], false, Native::Action(Do::Organize("view.pose"))),
    c("view.experiments", "Experiments", "View", &[], false, Native::Action(Do::Organize("view.experiments"))),
    c("simulation.experiment", "Run captured experiment", "Simulation", &["Ctrl+Return"], false, Native::Action(Do::Organize("simulation.experiment"))),
    c("robot.pose", "Preview joint motion", "Robot", &[], false, Native::Action(Do::Organize("robot.pose"))),
    c("tool.annotate", "Annotate", "Inspect", &["N"], true, Native::Action(Do::Organize("tool.annotate"))),
    c("view.comments", "Comments panel", "View", &[], false, Native::Action(Do::Organize("view.comments"))),
    c("view.saved_views", "Saved Views", "View", &[], false, Native::Action(Do::SavedViews)),
    c("view.comment_pins", "Toggle comment pins", "View", &[], false, Native::Action(Do::Organize("view.comment_pins"))),
    c("command_palette", "Command palette", "General", &["Ctrl+Space", "Shift+F"], true, Native::Surface(Opens::Palette)),
    c("file.new", "New", "File", &["Ctrl+N"], true, Native::Action(Do::File("file.new"))),
    c("file.open", "Open…", "File", &["Ctrl+O"], true, Native::Action(Do::File("file.open"))),
    c("file.save", "Save", "File", &["Ctrl+S"], true, Native::Action(Do::Save)),
    c("file.save_as", "Save As…", "File", &["Ctrl+Shift+S"], true, Native::Action(Do::File("file.save_as"))),
    c("file.import", "Import…", "File", &["Ctrl+I"], true, Native::Action(Do::File("file.import"))),
    c("file.export", "Export…", "File", &["Ctrl+E"], true, Native::Action(Do::File("file.export"))),
    c("file.export_drawing", "Export drawing (SVG)…", "File", &["Ctrl+Shift+D"], true, Native::Action(Do::File("file.export_drawing"))),
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
    c("edit.preferences", "Preferences…", "Edit", &[], false, Native::Different(PREFERENCES)),
    c("view.fit", "Fit All", "View", &["Home"], true, Native::Action(Do::Fit)),
    c("view.focus", "Focus Selection", "View", &["F"], true, Native::Camera(CameraCmd::Focus)),
    c("view.front", "View front", "View", &["1"], true, Native::Camera(CameraCmd::Preset(ViewPreset::Front))),
    c("view.back", "View back", "View", &["Ctrl+1"], true, Native::Camera(CameraCmd::Preset(ViewPreset::Back))),
    c("view.top", "View top", "View", &["7"], true, Native::Camera(CameraCmd::Preset(ViewPreset::Top))),
    c("view.bottom", "View bottom", "View", &["Ctrl+7"], true, Native::Camera(CameraCmd::Preset(ViewPreset::Bottom))),
    c("view.right", "View right", "View", &["3"], true, Native::Camera(CameraCmd::Preset(ViewPreset::Right))),
    c("view.left", "View left", "View", &["Ctrl+3"], true, Native::Camera(CameraCmd::Preset(ViewPreset::Left))),
    c("view.iso", "View iso", "View", &["0"], true, Native::Camera(CameraCmd::Preset(ViewPreset::Iso))),
    c("view.ortho", "Orthographic", "View", &["5"], true, Native::Camera(CameraCmd::Ortho)),
    c("view.grid", "Grid", "View", &["Ctrl+G"], true, Native::Action(Do::Display(DisplayCmd::Grid))),
    c("view.mode_next", "Next display mode", "View", &["Z"], true, Native::Action(Do::Display(DisplayCmd::Next))),
    c("view.mode.shaded", "Display: shaded", "View", &[], false, Native::Action(Do::Display(DisplayCmd::Mode(DisplayMode::Shaded)))),
    c("view.mode.shaded_edges", "Display: shaded edges", "View", &[], false, Native::Action(Do::Display(DisplayCmd::Mode(DisplayMode::ShadedEdges)))),
    c("view.mode.wireframe", "Display: wireframe", "View", &[], false, Native::Action(Do::Display(DisplayCmd::Mode(DisplayMode::Wireframe)))),
    c("view.mode.xray", "Display: xray", "View", &[], false, Native::Action(Do::Display(DisplayCmd::Mode(DisplayMode::Xray)))),
    c("view.mode.matcap", "Display: matcap", "View", &[], false, Native::Action(Do::Display(DisplayCmd::Mode(DisplayMode::Matcap)))),
    c("view.mode.render", "Display: render", "View", &[], false, Native::Action(Do::Display(DisplayCmd::Mode(DisplayMode::Render)))),
    c("view.orbit_mode", "Toggle orbit: turntable / trackball", "View", &[], false, Native::Camera(CameraCmd::OrbitMode)),
    c("view.fov", "Set field of view…", "View", &[], false, Native::Camera(CameraCmd::Fov)),
    c("view.isolate", "Isolate", "View", &["/"], true, Native::Op),
    c("view.show_all", "Show All", "View", &["Alt+H"], true, Native::Op),
    c("view.hide", "Hide", "View", &["H"], true, Native::Op),
    c("view.section", "Section Analysis", "Inspect", &["Ctrl+Shift+X"], true, Native::Action(Do::Display(DisplayCmd::Section))),
    c("view.build_plate", "Build Plate Preview", "Print", &["Ctrl+Shift+B"], true, Native::Action(Do::Display(DisplayCmd::BuildPlate))),
    c("view.high_contrast", "High-Contrast Theme", "View", &[], false, Native::Action(Do::Display(DisplayCmd::HighContrast))),
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
    c("tool.extrude", "Extrude", "Create", &["X"], true, Native::Op),
    c("tool.revolve", "Revolve", "Create", &["Shift+R"], true, Native::Op),
    c("tool.sweep", "Sweep (profile + path from selection)", "Create", &[], false, Native::Op),
    c("tool.pipe", "Pipe along selected curve…", "Create", &[], false, Native::Op),
    c("tool.loft", "Loft selected sketches", "Create", &[], false, Native::Op),
    c("tool.fill", "Fill / patch selected curve", "Create", &[], false, Native::Op),
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
    c("tool.plane", "Plane from face", "Planes", &["Ctrl+P"], true, Native::Op),
    c("tool.plane_three", "Plane from three points", "Planes", &[], false, Native::Op),
    c("tool.plane_camera", "Plane from two points (camera)", "Planes", &[], false, Native::Op),
    c("tool.plane_mid", "Midplane between two faces", "Planes", &[], false, Native::Op),
    c("tool.plane_xy", "Active plane: XY", "Planes", &[], false, Native::Op),
    c("tool.plane_xz", "Active plane: XZ", "Planes", &[], false, Native::Op),
    c("tool.plane_yz", "Active plane: YZ", "Planes", &[], false, Native::Op),
    c("tool.plane_2d_snap", "Toggle 2D snapping to the active plane", "Planes", &[], false, Native::Op),
    c("tool.fastener", "Fastener hole…", "Print", &["Ctrl+H"], true, Native::Op),
    c("tool.clearance", "Clearance offset…", "Print", &["Ctrl+Shift+C"], true, Native::Op),
    c("tool.mirror", "Mirror (about active plane)", "Modify", &["Ctrl+M"], true, Native::Op),
    c("tool.mirror_live", "Mirror as live instance", "Modify", &[], false, Native::Op),
    c("components.show", "Components library", "Window", &[], false, Native::Action(Do::Organize("components.show"))),
    c("components.make", "Make linked component…", "Create", &[], false, Native::Action(Do::Organize("components.make"))),
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
    c("sketch.line", "Sketch: Line", "Sketch", &["L"], true, Native::Op),
    c("sketch.rectangle", "Sketch: Rectangle", "Sketch", &["Shift+L"], true, Native::Op),
    c("sketch.rectangle_center", "Sketch: Rectangle (centre)", "Sketch", &[], false, Native::Op),
    c("sketch.circle", "Sketch: Circle", "Sketch", &["C"], true, Native::Op),
    c("sketch.circle_2pt", "Sketch: Circle (two points)", "Sketch", &[], false, Native::Op),
    c("sketch.circle_3pt", "Sketch: Circle (three points)", "Sketch", &[], false, Native::Op),
    // A deliberate native binding: keymap.json's `sketch.arc` (A) names no
    // command and RoboCAD drops it (app.py:462); A runs the three-point arc
    // here, as USER_GUIDE.md's "`A` arc" says it should (see the module doc).
    c("sketch.arc_3pt", "Sketch: Arc (three points)", "Sketch", &["A"], true, Native::Op),
    c("sketch.polygon", "Sketch: Polygon", "Sketch", &["Shift+P"], true, Native::Op),
    c("sketch.slot", "Sketch: Slot", "Sketch", &["Shift+S"], true, Native::Op),
    c("sketch.spline", "Sketch: Spline", "Sketch", &["Shift+C"], true, Native::Op),
    c("sketch.ellipse", "Sketch: Ellipse", "Sketch", &[], false, Native::Op),
    c("sketch.spiral", "Sketch: Spiral", "Sketch", &[], false, Native::Op),
    c("sketch.text", "Sketch: Text", "Sketch", &["T"], true, Native::Op),
    c("sketch.offset", "Sketch: offset selected curve…", "Sketch", &[], false, Native::Op),
    c("sketch.fillet", "Sketch: fillet corner…", "Sketch", &[], false, Native::Op),
    c("sketch.join", "Sketch: join curves", "Sketch", &[], false, Native::Op),
    c("modify.union", "Union", "Modify", &["Ctrl+U"], true, Native::Op),
    c("modify.subtract", "Subtract", "Modify", &["Ctrl+Shift+U"], true, Native::Op),
    c("modify.intersect", "Intersect", "Modify", &["Ctrl+Alt+U"], true, Native::Op),
    c("modify.region", "Region (overlap as new body)", "Modify", &[], false, Native::Op),
    c("modify.join", "Join", "Modify", &["J"], true, Native::Op),
    c("modify.unjoin", "Unjoin", "Modify", &["Shift+J"], true, Native::Op),
    c("modify.dissolve", "Dissolve redundant topology", "Modify", &[], false, Native::Op),
    c("modify.make_unique", "Make instance unique", "Modify", &[], false, Native::Op),
    c("print.wall_check", "Wall thickness check…", "Print", &["Ctrl+W"], true, Native::Op),
    c("print.validate", "Validate for printing", "Print", &["Ctrl+Shift+V"], true, Native::Op),
    c("print.overhangs", "Toggle overhang shading", "Print", &[], false, Native::Action(Do::Print("print.overhangs"))),
    c("print.split", "Split selected for printing…", "Print", &[], false, Native::Op),
    c("print.strength", "Check strength (document's print study)", "Print", &[], false, Native::Op),
    c("print.plan", "Plan print settings and plates (document's print study)", "Print", &[], false, Native::Op),
    c("print.strength_split", "Whole or split for strength? (selected part of the print study)", "Print", &[], false, Native::Op),
    c("print.assembly", "Assembly guide for the selected split…", "Print", &[], false, Native::Op),
    c("print.coupons", "Test coupons (for the selected split, or the material)…", "Print", &[], false, Native::Op),
    c("print.overlay", "Strength overlay on/off", "Print", &[], false, Native::Action(Do::Physical("print.overlay"))),
    c("print.jobs", "Print jobs…", "Print", &[], false, Native::Action(Do::Print("print.jobs"))),
    c("inspect.curvature", "Curvature comb on selected curve", "Inspect", &[], false, Native::Op),
    c("inspect.continuity", "Continuity check (G0/G1/G2)", "Inspect", &[], false, Native::Op),
    c("inspect.draft", "Draft-angle shading", "Inspect", &[], false, Native::Different(DRAFT_SHADING)),
    c("inspect.normals", "Normal-direction shading", "Inspect", &[], false, Native::Action(Do::Display(DisplayCmd::Mode(DisplayMode::Xray)))),
    c("bridge.start", "Live link: start (Blender)", "Bridge", &[], false, Native::Different(BLENDER_LINK)),
    c("bridge.stop", "Live link: stop", "Bridge", &[], false, Native::Different(BLENDER_LINK)),
    c("bridge.share", "Web share: publish viewer…", "Bridge", &[], false, Native::Different(WEB_SHARE)),
    c("robot.add_motor", "Robot: add motor from library…", "Robot", &["Ctrl+Shift+M"], false, Native::Op),
    c("robot.add_joint", "Robot: add joint (click parent, child, axis face)", "Robot", &["Ctrl+Shift+J"], true, Native::Op),
    c("robot.joint_dialog", "Robot: joint from the two selected bodies…", "Robot", &[], false, Native::Op),
    c("robot.infer", "Robot: infer joints from coaxial holes and pins", "Robot", &[], false, Native::Op),
    c("robot.assign_motor", "Robot: assign selected motor to a joint…", "Robot", &[], false, Native::Op),
    c("robot.fixed", "Robot: fix selected bodies together (first is the parent)", "Robot", &[], false, Native::Op),
    c("robot.ground", "Robot: toggle ground on selected bodies", "Robot", &[], false, Native::Op),
    c("robot.validate", "Robot: validate", "Robot", &[], false, Native::Action(Do::Physical("robot.validate"))),
    c("robot.motors", "Robot: motor library…", "Robot", &[], false, Native::Action(Do::Physical("robot.motors"))),
    c("robot.add_sensor", "Robot: add sensor (IMU, encoder, current, force)…", "Robot", &[], false, Native::Op),
    c("robot.add_cable", "Robot: add cable between bodies…", "Robot", &[], false, Native::Op),
    c("robot.power", "Robot: battery, control loop and uncertainty…", "Robot", &[], false, Native::Op),
    c("robot.load_results", "Robot: load simulation results…", "Robot", &[], false, Native::Action(Do::Physical("robot.load_results"))),
    c("robot.apply_identification", "Robot: apply identified joint parameters…", "Robot", &[], false, Native::Action(Do::Physical("robot.apply_identification"))),
    c("view.stress", "Toggle stress overlay (from loaded results)", "Inspect", &[], false, Native::Action(Do::Physical("view.stress"))),
    c("sim.export_physical", "Simulation: export physical model (simrobot v4, with flexible links)…", "Simulation", &[], false, Native::Action(Do::Physical("sim.export_physical"))),
    c("sim.export", "Simulation: export robot model…", "Simulation", &[], false, Native::Action(Do::Physical("sim.export"))),
    c("sim.link", "Simulation: live link (watch + run viewer)", "Simulation", &[], false, Native::Action(Do::Physical("sim.link"))),
    c("api.address", "REST API: show address", "Bridge", &[], false, Native::Different("the header always shows the service URL, so no dialog is needed")),
    c("group.set_active", "Set selected group as active", "Outliner", &[], false, Native::Op),
    c("group.group", "Group selection", "Outliner", &[], false, Native::Op),
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
    Camera(CameraCmd),
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
        Native::Camera(c) => Resolved::Camera(c),
        #[cfg(test)]
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
pub(crate) fn readiness(entry: &OpEntry, doc: &CadDocument, selection: &[SelectionItem]) -> Result<(), String> {
    // An interaction is started to pick, click or drag (its own refusals
    // come when it runs); viewer state needs nothing.
    if matches!(entry.flow, Flow::PickThenForm(_) | Flow::Sketch(_) | Flow::Extrude { .. } | Flow::PlanePick(_) | Flow::View(_) | Flow::PrintPick) {
        return Ok(());
    }
    let nodes = selection.nodes();
    let edges = selection.of_kind("edge");
    let faces = selection.of_kind("face");
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
/// selection ([`readiness`]; `selection`: the shared selection's CAD items).
pub(crate) fn ready(cmd: &Command, doc: &CadDocument, selection: &[SelectionItem], own: &[Control]) -> Result<(), String> {
    match resolve(cmd) {
        // As every edit: nothing runs while one is in flight or unconnected.
        // Viewer state (the active plane, 2D snapping) is never an edit.
        Resolved::Op(entry) if matches!(entry.flow, Flow::View(_)) => Ok(()),
        Resolved::Op(entry) => doc.edit_refusal().map_or_else(|| readiness(entry, doc, selection), Err),
        Resolved::Action(action) => own.iter().find(|c| c.action == action).map_or(Ok(()), |c| c.ready.clone()),
        Resolved::Surface(_) => Ok(()),
        Resolved::NumericEntry => numeric_entry_ready(doc),
        // Display only: the camera applies it (or logs why not, without an orbit camera).
        Resolved::Camera(_) => Ok(()),
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
        Resolved::Camera(c) => camera(c, call, cx),
        Resolved::Later(epic) => Outcome::Done(Err(later(cmd, epic))),
        Resolved::Different(why) => Outcome::Done(Err(format!("{} is not ported: {why}", cmd.label))),
    }
}

/// A camera command: its camera intent (applied by the shared camera after
/// the handler, `actions::apply`), the FOV entry, or Focus Selection.
fn camera(c: CameraCmd, call: &mut Call, cx: &mut Cx) -> Outcome {
    match (c, c.action()) {
        (CameraCmd::Fov, _) => crate::cad::views::open_fov(cx),
        (_, Some(action)) => {
            cx.camera.push(action.clone());
            Outcome::Done(Ok(json!({"camera": action, "note": "display only: RoboCAD's own camera is unchanged"})))
        }
        _ => focus(call, cx),
    }
}

/// RoboCAD's `focus_selection` (ui/viewport.py:449-470): the selected nodes
/// and everything under them framed; Fit All when nothing is selected.
/// Display only.
fn focus(call: &mut Call, cx: &mut Cx) -> Outcome {
    let nodes = cx.shared.items().nodes();
    if nodes.is_empty() {
        return crate::cad::actions::handle(&CadAction::CadFit { id: None }, call, cx);
    }
    let mut ids: std::collections::HashSet<String> = nodes.iter().cloned().collect();
    if let Some(state) = &cx.doc.doc {
        // Walk order lists parents before children.
        for n in &state.nodes {
            if n.parent.as_ref().is_some_and(|p| ids.contains(p)) {
                ids.insert(n.id.clone());
            }
        }
    }
    let Some(meshes) = cx.meshes.as_deref_mut() else { return Outcome::Done(Err("CAD mode's 3D view is not available in this window".into())) };
    let names: Vec<String> = nodes.iter().map(|n| cx.doc.node_name(n)).collect();
    match meshes.bounds(Some(&ids)) {
        Some(bounds) => {
            meshes.frame(bounds);
            Outcome::Done(Ok(json!({"framed": names, "note": "display only: RoboCAD's view and the geometry are unchanged"})))
        }
        // RoboCAD leaves the camera as it is then.
        None => Outcome::Done(Err(format!("nothing to frame: no body of {} is drawn", names.join(", ")))),
    }
}
