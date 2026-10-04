//! Robot mode (`--robot FILE`): a CAD-exported `.simrobot.json` opened
//! read-only. The shared `PhysicalModel` loader runs on a worker thread; each
//! link's collision geometry is drawn at the exported assembly pose until a
//! run starts. Run/Pause/Step/Reset drive the shared `PhysicalRobot` on the
//! run thread (`run`); links follow its frames. Nothing is written.
//! A preset (`--robot-preset ID`, REST `robot_preset`) opens the same way:
//! its scene's `robot` goes through the same loader, and the run thread runs
//! the preset's shared EmbeddedEnvironment/EmbeddedSession (`preset`).
//! A FILE is watched and reloaded on change or Reload (`source`); a
//! reload replaces the model and starts a fresh run context.
//! A planar (v2) FILE, which `PhysicalModel` refuses, is read as
//! sim-phenomena's `CadModel` by the shared version rule and run through the
//! shared planar build on its own run thread (`planar`); every action
//! without a v2 meaning is refused naming it.
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::camera::{CameraSet, Framing, Orbit, OrbitRules, RadiusLimits, ViewArea};
use crate::builder::ui_api::Enabled;
use crate::ui_kit::{ACCENT, Corner, DANGER, Dock, Kit, Look, SUBTLE, TEXT, Tint, UiFonts, WARN, size, wheel_delta, wrap};
use bevy::ui::prelude::AccessibleLabel;
use bevy::{
    asset::RenderAssetUsages,
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::MouseWheel,
    picking::mesh_picking::MeshPickingCamera,
    prelude::*,
    render::{
        mesh::{Indices, PrimitiveTopology},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_robot::PhysicalModel;
use sim_domain_robot::cad_link::{self, CadLinkStatus};
use preset::{Preset, PresetRun, RecordedRun};
use source::{FileModel, SourceWatch, Trigger as ReloadTrigger};
use planar::PlanarView;
use stress::StressOverlay;
use gait::{GaitAction, GaitSource};
use playback::RecordedAction;
use run::{JOG_LABEL, JOG_SEMANTICS, JOG_STEP_M, JOG_STEP_RAD, MotionRequest, OverlayFlags, ReplayPhase, RunAction, RunController, SpeedRequest};
use std::path::{Path, PathBuf};
use crate::document::DocumentRegistry;
use crate::selection::Selection;

const LEFT: f32 = 280.0;
const RIGHT: f32 = 390.0;
const TOP: f32 = 64.0;
/// Height of the graph dock above the bottom edge, when shown.
const DOCK: f32 = 262.0;
/// Exported link frames: origin at the stored `com`, axes aligned with the
/// model frame (the solver's zero-angle pose, `articulated.rs` `com0`).
pub const POSE: &str = "exported assembly pose: link frames at the stored com, axes aligned with the model frame (Z up); no joint motion, not stepped";
/// Which values carry a measured/derived/estimated label, stated in the UI and REST.
pub const PROVENANCE_RULE: &str = "typed provenance labels are shown only where the file carries one: joint physics.drive_backlash.provenance and actuator profile parameters. Free text the file carries (link mass_sources and member_names, joint physics.source, motor notes) is shown verbatim as the file's text, never mapped to a label. Every other value has no per-value provenance in the export; see the source block's notes.";
/// Shown once a run frame (built or stepped) is displayed.
pub const SIMULATED_POSE: &str = "simulated pose from the run thread's latest frame (see run.poses); the exported assembly pose is t = 0 of each generation";
/// Shown while a recorded preset's capture poses the links.
pub const RECORDED_POSE: &str = "recorded pose: a frame of the preset's capture (recorded physics, played back, not simulated here), mapped to the scene's links by name; see preset.frame_count and run.poses";
/// Shown while a gait preview poses the links (robot_gait).
pub const GAIT_POSE: &str = "gait preview pose from the shared KinematicMirror at the sampled gait time (see gait_preview): kinematic preview (geometry only, suspended) — not a physics result; Stop shows the run's frame again";
const UNLABELLED: &str = "no per-value provenance in export (see Source → notes)";

enum Status {
    Loading(std::time::Instant),
    Loaded { seconds: f64 },
    Error(String),
}

#[derive(Resource)]
pub struct RobotView {
    pub path: PathBuf,
    status: Status,
    model: Option<PhysicalModel>,
    triangles: Vec<usize>,
    notes: FileNotes,
    cad_link: Option<CadLinkStatus>,
    // The selected link is the shared selection's (`picked`), not the view's.
    section: Section,
    /// Inspector scroll offset and its maximum, in logical pixels (as laid out).
    scroll: f32,
    scroll_max: f32,
    scroll_to: Option<f32>,
    /// The preset's load in progress (a `--robot FILE` load goes through `source`).
    load: Option<crate::jobs::Job<(Loaded, Option<Opened>)>>,
    /// The preset being opened or run (None for `--robot FILE`).
    preset: Option<Preset>,
    /// The preset list and the root its paths resolve against.
    /// The preset list (explicit --robot-presets, else `<root>/web/viewer/presets.json`).
    presets: Result<PathBuf, String>,
    /// The launch's workspace root (`crate::workspace`); preset inputs and recordings resolve against it.
    root: Result<PathBuf, String>,
    /// The run thread, spawned idle once the model has loaded.
    run: Option<RunController>,
    /// The last refused run control from a click (REST gets the error directly).
    run_message: Option<String>,
    /// LinkMesh transforms need re-applying (new frame, or reset to the assembly pose).
    pose_dirty: bool,
    ui_revision: u64,
    panels_ready: bool,
    /// The graph dock (system_ui graphs:toggle, key G, the Graphs button).
    graphs_visible: bool,
    /// `--robot FILE`: the file's hash, watch and reload state (None for a preset).
    source: Option<SourceWatch>,
    /// The last reload's result line (reason, run reset, selection), shown in the header.
    notice: Option<String>,
    /// `--robot FILE`: the read-only `.simresult.json` stress overlay (`robot_stress`).
    stress: StressOverlay,
    /// The Leg calibration panel's mirror (`hardware::mirror`): while set, its
    /// poses are drawn instead of the run's frame and its leg is tinted blue;
    /// Run is refused (the page's `setPlaying` refuses to play while mirroring).
    mirror: Option<hardware::MirrorDisplay>,
    /// `--robot FILE` with a planar (v2) file: its summary and planar run
    /// (`model` and `run` are None then; a selected link indexes its bodies).
    planar: Option<PlanarView>,
    /// A `drive` preset opened as its model with its binding (a `--robot
    /// FILE` view underneath): its label, readiness, follow_link and grid size.
    drive_preset: Option<Preset>,
    /// Follow robot (`view_tools::FOLLOW_RULE`).
    follow: bool,
    /// The display cap in Hz, 0 automatic (`view_tools::DISPLAY_RULE`).
    display_hz: u32,
    /// The whole robot's drawn bounds in the display frame (Fit frames them again after a Fit selected).
    bounds: Option<(Vec3, Vec3)>,
    /// Fit selected is pending (`view_tools::fit_selected`).
    fit_selected: bool,
    /// The channels picked for the graph dock, pinned across Reset, reload and preset changes (`graphs::PICK_RULE`).
    picks: Vec<String>,
    /// What a tested recipe's open does once loaded (`leaderboard::AfterOpen`).
    after_open: Option<leaderboard::AfterOpen>,
}

/// Inspector sections, switched by tab click or `system_ui`.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Link,
    Joints,
    Drives,
    Source,
    /// RoboCAD's comment threads on the CAD source (`threads`).
    Comments,
    /// The run readouts (`readouts`): execution, performance, motion
    /// progress, learning, travel, pushes, readings and task observations.
    Run,
}
impl Section {
    const ALL: [Section; 6] = [Section::Run, Section::Link, Section::Joints, Section::Drives, Section::Source, Section::Comments];
    fn label(self) -> &'static str {
        match self {
            Section::Run => "Run",
            Section::Link => "Link",
            Section::Joints => "Joints",
            Section::Drives => "Drives",
            Section::Source => "Source",
            Section::Comments => "Comments",
        }
    }
}

mod actions;
pub mod gait;
pub mod graphs;
pub mod hardware;
pub mod motion;
pub mod planar;
pub mod playback;
pub mod preset;
pub mod recording;
pub mod run;
pub mod source;
pub mod stress;
pub(crate) use actions::{DriveRequest, RobotAction};
use actions::{check, check_stress, overlay_toggle};
mod controls;
mod guide;
mod inputs_panel;
mod inspector;
mod loader;
mod overlay_view;
mod video;
mod view_tools;
mod leaderboard;
mod panel_ui;
mod readouts;
mod picked;
mod scene;
mod sections;
mod state;
#[cfg(test)]
mod tests;
mod threads;
mod timeline;
mod ui;
pub use loader::{FileNotes, LinkGeometry, Loaded, Opened, PLANAR_POSE, load, load_bytes, load_file, load_file_bytes, load_preset, load_recorded, loaded, physical_format_name};
use controls::{GAIT_SCALES, GAIT_SEEK, OVERLAYS, RECORDED_TRANSPORT, STRESS_PRESET, gait_panel, gait_seek, jog_joints, jog_panel, motion_buttons, motion_panel, motion_text, overlay_on, recorded_panel, replay_line};
use inspector::{clip, overlay_panel, panels, recorded_line, speed_panel};
use overlay_view::{OverlayGizmos, draw, graph_dock, overlay_gizmo_config, stress_paint, stress_panel};
use panel_ui::{GaitPathRoot, PanelToggle, RecordedSeek, RecordedSeekFill, RobotPanelUi, seek_fraction};
use scene::{apply_frames, enable, highlight, planar_sync, receive, scroll, view_area, watch};
use sections::{drives_text, file_watch_text, jog_line, joints_text, link_text, source_text};
use state::touching;
use ui::{
    GaitButton, GaitError, GaitList, GaitRoot, GaitSeekButton, GaitText, GraphDock, Inspector, InspectorScroll, JogButton, JogRoot, JogText, LINK_COLOUR, LinkMesh, LinkRow, ListRoot, Materials, MotionButton,
    MotionRoot, MotionText, OverlayButton, OverlayLabel, OverlayText, RecordedButton, RecordedRoot, RecordedText, RecordingText, ReloadButton, ReplayList, ReplayText, RobotCamera, RobotRoot, RunButton, RunText,
    SpeedButton, SpeedLabel, StatusText, StressText, TabButton, TitleText, setup,
};

/// Robot mode's public ordering points (native-viewer.md "Public system
/// sets"), configured once in [`RobotPlugin`]; the hardware panels order
/// against these, not against robot mode's functions.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RobotSet {
    /// Actions: robot mode's one handler (`actions::apply`), which also
    /// passes `system_ui` activations on to the hardware panel's.
    Actions,
    /// SimSync: this frame's posed links (`scene::apply_frames`).
    Frames,
}

/// [`RobotSet`] in the pipeline (RobotPlugin, and the windowless schedule test).
pub(crate) fn configure_sets(app: &mut App) {
    app.configure_sets(Update, RobotSet::Actions.in_set(ViewerSet::Actions)).configure_sets(Update, RobotSet::Frames.in_set(ViewerSet::SimSync));
}

/// Robot mode: worker load, posed link meshes, link list, inspector and
/// REST. Its entities are spawned on entering the Robot scope; its two
/// chains run in robot mode in their original order (the frame chain in
/// SimSync, the panels in Present, one after the other as before). The
/// view itself (`RobotView`) is removed on exit by `app::switch`.
pub struct RobotPlugin;
impl Plugin for RobotPlugin {
    fn build(&self, app: &mut App) {
        crate::app::actions::register::<RobotAction>(app);
        configure_sets(app);
        hardware::build(app);
        panel_ui::add_field(app);
        view_tools::add_field(app);
        leaderboard::add_field(app);
        threads::build(app);
        app.insert_gizmo_config(OverlayGizmos, overlay_gizmo_config())
            .init_resource::<RobotPanelUi>()
            .init_resource::<leaderboard::Leaderboard>()
            .init_resource::<video::VideoRecorder>()
            .add_systems(OnExit(ModeScope::Robot), video::leave)
            .add_systems(OnEnter(ModeScope::Robot), setup)
            // Device driving (`crate::drive_input`) offers nothing once Robot mode is left.
            .add_systems(OnExit(ViewerMode::Robot), crate::drive_input::leave_mode)
            .add_systems(OnExit(ModeScope::Robot), |mut commands: Commands, mut in_flight: ResMut<crate::app::actions::InFlight<RobotAction>>, mut replies: ResMut<crate::app::actions::Replies>| {
                commands.remove_resource::<Materials>();
                // `actions::apply` does not run outside Robot mode: a carried
                // call (a forwarded `hardware:<name>` activation waiting on
                // its acknowledgement) would otherwise hang until re-entry.
                // A forwarded activation's own hardware reply is closed first.
                in_flight.abandon_with(&mut replies, "left Robot mode before the command finished", actions::forget_forwarded_on_exit);
            })
            .add_systems(
                Update,
                (
                    // Keys and buttons write robot actions after REST's, as the old chain applied them.
                    // The gait path field and the comment composer first: a press that focuses one this frame already stops robot keys (`ui_kit::text::Typing`).
                    // Preset motion keys and drive input never both act: `motion_keys` needs a preset with a
                    // motion config, the drive target (`controls::drive_target`) a controlled run (drive profile).
                    (panel_ui::gait_path_input, threads::input, view_tools::search_input, leaderboard::search_input, view_tools::run_key, panel_ui::toggles, panel_ui::recorded_seek, inputs_panel::input_sliders, timeline::timeline_seek, actions::motion_keys, actions::graph_key, actions::overlay_keys, actions::speed_keys, actions::planar_keys, actions::buttons)
                        .chain()
                        .in_set(crate::app::InputSet::Window),
                    // What the one device poller (`crate::drive_input`, InputSet::Window) may drive: written before it.
                    controls::drive_target.in_set(ViewerSet::Input).before(crate::app::InputSet::Window),
                    // Robot mode's one apply; it also reads the device poller's requests for Robot mode.
                    actions::apply.in_set(RobotSet::Actions),
                    (panel_ui::receive_listing, leaderboard::receive, video::receive).in_set(ViewerSet::JobResults),
                    // Before the shared camera (`crate::camera`): its viewport reads the
                    // ViewArea `view_area` sets, its place step frames the bounds `receive`
                    // and `planar_sync` write.
                    (watch.in_set(ViewerSet::SimSync), receive.in_set(ViewerSet::SimSync), stress_paint.in_set(ViewerSet::SimSync), apply_frames.in_set(RobotSet::Frames), view_tools::follow.in_set(ViewerSet::SimSync), view_tools::fit_selected.in_set(ViewerSet::SimSync), planar_sync.in_set(ViewerSet::SimSync), scroll.in_set(ViewerSet::SimSync), overlay_view::picker_scroll.in_set(ViewerSet::SimSync), leaderboard::scroll.in_set(ViewerSet::SimSync), view_area.in_set(ViewerSet::SimSync), highlight.in_set(ViewerSet::SimSync)).chain().before(CameraSet::Viewport),
                    (
                        (panels, speed_panel, overlay_panel, stress_panel, jog_panel, motion_panel, inputs_panel::inputs_panel, controls::drive_panel, recorded_panel, timeline::timeline_panel, gait_panel).chain(),
                        (panel_ui::gait_path_draw, view_tools::search_draw, view_tools::tools_panel, view_tools::display_rate, leaderboard::draw, video::button_label, graph_dock, draw, video::capture, actions::publish).chain(),
                    )
                        .chain()
                        .in_set(ViewerSet::Present),
                )
                    .run_if(in_state(ViewerMode::Robot)),
            );
    }
}
