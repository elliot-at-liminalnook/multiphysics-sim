//! Robot mode's Leg calibration and hardware panel (docs/architecture/
//! native-viewer.md §8, ledger docs/hardware-parity.md): a native front end,
//! feature for feature, over the browser's `web/viewer/calibration-ui.mjs`
//! (with `actuator-motion-view.mjs`), `calibration-mirror.mjs` and
//! `hardware-sync.mjs`. It talks to the existing Rust hardware servers
//! (`serve_actuator_calibration`, `serve_motor_bench`) through the one typed
//! loopback client, `sim_runtime::hardware_client`. The servers keep the
//! serial bus, leases, watchdogs, the supervisor and STOP; nothing here opens
//! a serial port or computes a motor command.
//!
//! - **Where it lives.** A dock inside Robot mode (not a mode of its own),
//!   shown by the header's "Leg calibration" button, or open from launch
//!   with `--hardware URL`. [`Hardware`] (a resource) is inserted when Robot
//!   mode is entered and removed when it is left.
//! - **Actions** ([`actions`]): every intent is a [`HardwareAction`], written
//!   by the panel's buttons, keys (Q/A hold-to-move, Z/Escape stop), window
//!   focus loss, `system_ui` and REST, and applied by one system,
//!   [`actions::apply`], in `ViewerSet::Actions`. Motion-starting actions
//!   ([`HardwareAction::starts_motion`]: anything that starts, changes or
//!   arms motion, including the operator's confirmations and drive
//!   settings, and the mirror's leg, joint, polarity and alignment
//!   bindings, which become the Leg/Both gait bindings and the saved
//!   alignment reference) are refused, by name, from REST and `system_ui`
//!   (`Origin::Rest`, `Origin::SystemUi`); status, gaits, export, connect,
//!   sections, turning the mirror on or off and STOP stay available there.
//! - **Link** ([`link`], [`session`]): one `jobs::RunThread`
//!   ("hardware-link") per connection runs the page's session logic
//!   (select, hold-to-move, sweeps, tune, campaign, gait on the leg): it
//!   polls `/calibration/status` (600 ms, 150 ms while a session or leg gait
//!   runs) and, through its own "hardware-beat" worker (so a slow request
//!   cannot let a lease lapse), the `motion_update` heartbeat every 100 ms
//!   while a motion session is open and the gait lease every 300 ms, with
//!   one increasing sequence, and publishes a generation-stamped
//!   [`link::LinkSnapshot`]. When its channel closes (the link dropped:
//!   panel reconnect, mode exit, window close) it sends STOP before it
//!   returns.
//! - **STOP never queues** ([`link::stop_now`]): the button, Z, Escape,
//!   focus loss, panel close and mode exit post STOP on a fresh connection
//!   from a `jobs::Pool::Dedicated` job (`complete_on_drop`), not through the
//!   link thread, so it cannot wait behind a slow request (a select proving
//!   watchdogs, a hardware reply the server waits up to 8 s for). The
//!   server latches its stop flags as soon as it parses the request.
//! - **Loss stops any drive** (`handlers::loss`): focus loss, panel close
//!   and window close stop whenever [`link::drive_active`] (a session,
//!   start, busy select, sweep-all, tune, campaign or leg gait), wider than
//!   the page's `loss()` (ready, starting or a session only) on purpose,
//!   and stop a live sync this viewer opened (not another client's). A
//!   window close also writes STOP synchronously
//!   (`Client::send_only`, the page's `keepalive` fetch on `pagehide`),
//!   since the detached STOP job may not outlive the process.
//! - **Mirror** ([`mirror`]) and **live sync** ([`sync`]): the suspended
//!   robot posed from the measured encoders through the shared
//!   `sim_runtime::kinematic_mirror::KinematicMirror` on a jobs worker, and
//!   the Real motor sync section streaming a live run's named motor targets
//!   to `serve_motor_bench` `/live/*` (samples on its RunThread, `/status`
//!   and `/stop` on their own jobs, so neither waits behind the other). Their preferences persist in
//!   [`settings`] (a JSON file; never calibration data), read once when the
//!   app is built (`actions::Preferences`), never at mode entry.
//! - **UI** ([`panel`], `panel_sections`, [`view`], [`dial`],
//!   [`motion_view`]): ui_kit widgets in the page's order and labels;
//!   [`view`] holds the page's `render()` rules as pure functions of the
//!   snapshot and the form; `handlers` holds one handler per action.
pub(crate) mod actions;
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

/// The calibration server's usual address
/// (examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/README.md).
pub const DEFAULT_CALIBRATION_URL: &str = "http://127.0.0.1:4194";

/// A hardware server the operator started, as given at launch.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerTarget {
    /// `http://127.0.0.1:PORT` (loopback only; the client refuses anything else).
    pub url: String,
    /// A file holding the server's control token; None: read it from the page
    /// the server serves, as the browser receives it.
    pub token_file: Option<PathBuf>,
}

/// Launch facts for the panel (`--hardware`, `--hardware-token-file`,
/// `--motor-bench`, `--motor-bench-token-file`); kept in `app::switch::Documents`
/// so they survive mode switches.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HardwareConfig {
    /// `serve_actuator_calibration`; None: the panel's Connect uses [`DEFAULT_CALIBRATION_URL`].
    pub calibration: Option<ServerTarget>,
    /// `serve_motor_bench` (live sync); None: the Real motor sync section explains how to start it.
    pub bench: Option<ServerTarget>,
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
    /// The calibration server's link (None until connected).
    pub link: Option<link::Link>,
    /// Token discovery and the link's start, off the UI thread.
    pub connecting: Option<crate::jobs::Job<sim_runtime::hardware_client::Client>>,
    /// Why the last connect failed, or a local refusal to show in the panel.
    pub notice: Option<String>,
    /// The page's form controls (sliders, checkboxes, selects, open sections).
    pub form: view::Form,
    /// STOP requests posted on the immediate path, until they answer.
    pub stops: Vec<crate::jobs::Job<serde_json::Value>>,
    /// Download calibration: the export job and its last result line.
    pub export: Option<crate::jobs::Job<PathBuf>>,
    pub export_line: Option<String>,
    pub mirror: mirror::Mirror,
    pub sync: sync::LiveSync,
    pub settings: settings::Settings,
    /// Bumped whenever the panel's structure must be rebuilt (connect, gait list, sections).
    pub ui_revision: u64,
    /// The link's snapshot, copied once a frame (`actions::poll_jobs`,
    /// JobResults) for the panel, `system_ui` and REST; the default while
    /// no link exists.
    pub snapshot: link::LinkSnapshot,
    /// The last link generation handed out (each connect makes the next).
    pub generation: u64,
    /// Download calibration: the number of the last export started, and the
    /// last one finished with its result (REST `hardware_export` waits for its own).
    pub export_seq: u64,
    pub export_done: Option<(u64, Result<PathBuf, String>)>,
}

impl Hardware {
    /// The state Robot mode starts with: the launch's servers, the saved
    /// preferences and the page's initial form; open when `--hardware` was
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
            config,
            ui_revision: 0,
            snapshot: link::LinkSnapshot::default(),
            generation: 0,
            export_seq: 0,
            export_done: None,
        }
    }

    /// The calibration server to connect to (`--hardware`, else [`DEFAULT_CALIBRATION_URL`]).
    pub fn target(&self) -> ServerTarget {
        self.config.calibration.clone().unwrap_or_else(|| ServerTarget { url: DEFAULT_CALIBRATION_URL.into(), token_file: None })
    }

    pub fn url(&self) -> String {
        self.target().url
    }
}

/// What the leg mirror shows instead of the run's frame (`RobotView`'s
/// `mirror`, written by `mirror_panel` when a pose is solved, cleared when
/// the mirror ends): each loaded link's pose by index (model frame, as
/// `robot_run::Frame::poses`) and the mirrored leg's links, tinted blue.
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
