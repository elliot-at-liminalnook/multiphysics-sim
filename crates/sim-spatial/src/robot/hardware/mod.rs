//! Robot mode's native hardware front end over shared in-process applications.
//! One HardwareAction apply writes UI state in ViewerSet::Actions; jobs-owned
//! link/beat/sync/mirror workers publish generation-stamped frames. Physical
//! serial acquisition, safety, limits and durable records belong to sim-runtime.
//! Explicit local configuration files replace hardware URLs and token discovery.
//! STOP first reaches the shared latch synchronously, then an independent Job
//! awaits authoritative release evidence. Latch acceptance alone is never
//! stationary readback. Mode exit, focus loss, panel close and link replacement
//! retain the same unconditional safety paths and no native HTTP fallback.
pub(crate) mod actions;
pub(crate) mod local;
pub(crate) mod dial;
mod handlers;
pub(crate) mod link;
pub(crate) mod mirror;
pub(crate) mod mirror_panel;
pub(crate) mod motion_view;
pub(crate) mod panel;
mod panel_sections;
pub(crate) mod session;
pub(crate) mod settings;
pub(crate) mod sync;
pub(crate) mod sync_panel;
pub(crate) mod view;

pub(crate) use actions::HardwareAction;

use bevy::prelude::*;
use std::path::PathBuf;

/// An explicit local driver configuration file. No URL or token is interpreted.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalTarget {
    pub config_file: PathBuf,
}

/// Launch configuration survives Robot mode switches; driver ownership does not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HardwareConfig {
    pub calibration: Option<LocalTarget>,
    pub bench: Option<LocalTarget>,
}

/// The panel's collapsible sections (the page's `<details>`), with the
/// page's initial open state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    /// "Tune this motor" (open).
    Tune,
    /// "Characterization campaign" (closed).
    Campaign,
    /// "Gait playback" (closed; opening it loads the gait list).
    Gait,
    /// "Recent leg runs" inside Gait playback (closed).
    GaitRuns,
    /// "Simulated leg mirror" (open).
    Mirror,
    /// "Advanced settings & feedback" (closed).
    Advanced,
    /// "Real motor sync" (open).
    Sync,
}
impl Section {
    pub const ALL: [Section; 7] = [Section::Tune, Section::Campaign, Section::Gait, Section::GaitRuns, Section::Mirror, Section::Advanced, Section::Sync];
    pub fn open_initially(self) -> bool {
        matches!(self, Section::Tune | Section::Mirror | Section::Sync)
    }
}

/// Robot mode's hardware state: inserted on entering Robot mode, removed on
/// leaving it (after STOP). Owned by the panel's systems; read by robot.rs
/// only through [`RobotView`](crate::robot::RobotView)'s `mirror` field.
#[derive(Resource)]
pub(crate) struct Hardware {
    pub config: HardwareConfig,
    /// The panel is shown (the page's `!panel.hidden`).
    pub open: bool,
    /// The local calibration application's link (None until connected).
    pub link: Option<link::Link>,
    /// Local configuration loading and the driver's start, off the UI thread.
    pub connecting: Option<crate::jobs::Job<local::Client>>,
    /// Why the last connect failed, or a local refusal to show in the panel.
    pub notice: Option<String>,
    /// The page's form controls (sliders, checkboxes, selects, open sections).
    pub form: view::Form,
    /// STOP requests posted on the immediate path, until they answer.
    pub stops: Vec<crate::jobs::Job<serde_json::Value>>,
    /// Download calibration: the export job and its last result line.
    pub export: Option<crate::jobs::Job<handlers::Exported>>,
    pub export_line: Option<String>,
    pub mirror: mirror::Mirror,
    pub sync: sync::LiveSync,
    pub settings: settings::Settings,
    /// Explicit startup publication acknowledgement; never a change-tick test.
    pub preferences_loaded: bool,
    /// Bumped whenever the panel's structure must be rebuilt (connect, gait list, sections).
    pub ui_revision: u64,
    /// The link's snapshot, copied once a frame (`actions::poll_jobs`,
    /// JobResults) for the panel, `system_ui` and REST; the default while
    /// no link exists.
    pub snapshot: link::LinkSnapshot,
    /// The last link generation handed out (each connect makes the next).
    pub generation: u64,
    pub command_seq: u64,
    pub active_ticket: Option<u64>,
    pub queued_ticket: bool,
    /// Download calibration: the number of the last export started, and the
    /// last one finished with its result (REST `hardware_export` waits for its own).
    pub export_seq: u64,
    pub export_done: Option<(u64, Result<handlers::Exported, String>)>,
    /// The running export was started on a virtual calibration execution:
    /// its result line is labelled simulated (`handlers::start_export`); a
    /// download the server labelled simulated itself is labelled too
    /// (`handlers::Exported::simulated`, `actions::poll_jobs`).
    pub export_virtual: bool,
    /// Per direction (upper, lower), bumped by every jog press the handler
    /// sends to the link (`handlers`' `JogPress`; not one refused before it
    /// is queued): a refused remote press puts its held flag back to what it
    /// was before the press ([`PendingPress::before`]) only if no newer press
    /// of that direction (the operator's) took the flag since it was queued.
    pub jog_presses: [u64; 2],
    /// Remote jog presses queued on the link whose verdict is not yet known
    /// (`handlers::settle_presses`).
    pub pending_presses: Vec<PendingPress>,
    /// A connect replaced a link pinned to a virtual bench and no connect
    /// has succeeded since: if the next link is not pinned, the notice says
    /// why remote motion is refused (`actions::connect`, `actions::BENCH_GONE`).
    pub replaced_virtual: bool,
}

/// A remote jog press queued on the link as a checked command, until its
/// verdict is read (`handlers::settle_presses`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PendingPress {
    /// Its ticket in [`link::LinkSnapshot::command_results`].
    pub ticket: u64,
    /// The link generation it was queued on.
    pub generation: u64,
    pub direction: actions::Direction,
    /// Its direction's [`Hardware::jog_presses`] after this press.
    pub press: u64,
    /// The direction's held flag before this press set it: a refusal puts it
    /// back (an operator already holding that direction keeps the hold, and
    /// their release still reaches the link), never simply clears it.
    pub before: bool,
    /// Nobody waits on its answer (`Origin::SystemUi`): a refusal is shown in
    /// the panel's notice, as a refusal at dispatch would be. A REST caller
    /// gets it in its answer.
    pub one_way: bool,
}

impl Hardware {
    /// The state Robot mode starts with: the launch's servers, the saved
    /// preferences and the page's initial form; open when `--hardware-config` was
    /// given (the caller then starts connecting). Nothing is connected yet.
    pub fn new(config: HardwareConfig, settings: settings::Settings) -> Self {
        Hardware {
            open: config.calibration.is_some(),
            link: None,
            connecting: None,
            notice: None,
            form: view::Form::new(settings.calibration.drive_mode, settings.calibration.hold_others),
            stops: Vec::new(),
            export: None,
            export_line: None,
            mirror: mirror::Mirror::new(&settings.mirror),
            sync: sync::LiveSync::new(config.bench.clone(), &settings.sync),
            settings,
            preferences_loaded: false,
            config,
            ui_revision: 0,
            snapshot: link::LinkSnapshot::default(),
            generation: 0,
            command_seq: 0,
            active_ticket: None,
            queued_ticket: false,
            export_seq: 0,
            export_done: None,
            export_virtual: false,
            jog_presses: [0; 2],
            pending_presses: Vec::new(),
            replaced_virtual: false,
        }
    }

    pub fn target(&self) -> Result<LocalTarget, String> {
        self.config.calibration.clone().ok_or_else(|| "No local calibration configuration. Launch with --hardware-config FILE; hardware server URLs and tokens are obsolete.".into())
    }

    pub fn configuration_label(&self) -> String {
        self.config.calibration.as_ref().map(|t| t.config_file.display().to_string()).unwrap_or_else(|| "no local calibration configuration".into())
    }

}

/// What the leg mirror shows instead of the run's frame (`RobotView`'s
/// `mirror`, written by `mirror_panel` when a pose is solved, cleared when
/// the mirror ends): each loaded link's pose by index (model frame, as
/// `robot::run::Frame::poses`) and the mirrored leg's links, tinted blue.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MirrorDisplay {
    pub poses: Vec<Option<([f64; 3], bevy::math::DQuat)>>,
    pub tinted: std::collections::BTreeSet<usize>,
}

/// Registers the panel's action type, systems and lifecycle (called by `RobotPlugin`).
pub(crate) fn build(app: &mut App) {
    actions::build(app);
    panel::build(app);
    mirror_panel::build(app);
    sync_panel::build(app);
}
