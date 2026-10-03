//! Build mode's REST commands (build, and lessons over the builder):
//! [`SystemAction`] (the `system_*` commands, and `Ui`, which carries a
//! button's, key's or marker's `BuildAction`) and [`UiAction`] (`system_ui`),
//! registered in the action registry and applied by [`apply`], the builder's
//! one handler (`ViewerSet::Actions`). Edits go through `Builder::apply` (the
//! validated command path and the shared undo history); `expected_revision`
//! is checked where it always was.
use super::*;
use super::actions::{BuildAction, dispatch};
use crate::app::actions::{self, Act, Call, InFlight, Replies, Spec, spec};
use crate::app::switch::{WindowAction, ask_switch, awaited_switch};
use bevy::ecs::message::Messages;
use serde::Deserialize;
use serde_json::{Value, json};
use sim_api::Outcome;

/// Every builder intent that is not a button (the build and lessons modes'
/// REST commands, `system_*`), and the carrier of a button's, key's or
/// `system_ui` activation's [`BuildAction`] (`Ui`). One message type, one
/// handler ([`apply`]); REST variants keep each command's JSON shape. Edits
/// go through the builder's validated command path (`Builder::apply`, the
/// shared undo history) and `expected_revision` is checked where it was.
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SystemAction {
    /// `system_context`: its args are a `sim_model_context::Request`.
    SystemContext(serde_json::Map<String, Value>),
    SystemAgent {
        action: agent::Request,
    },
    SystemUi {
        action: UiAction,
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    SystemDiscussions {
        action: discussion::Request,
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    SystemGrid {
        #[serde(default)]
        grid: Option<sim_system::display::Grid>,
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    SystemMove {
        names: Vec<String>,
        position_m: [f32; 3],
        #[serde(default)]
        snap: bool,
        #[serde(default)]
        preview: bool,
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    System {
        #[serde(default)]
        label: Option<String>,
        commands: Vec<sim_system::Command>,
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    SystemState,
    /// `system_open {path}`.
    SystemOpen(serde_json::Map<String, Value>),
    /// `system_gait_reports {dir?}`.
    SystemGaitReports(serde_json::Map<String, Value>),
    /// `system_calibration_review {path?, trial?}`.
    SystemCalibrationReview(serde_json::Map<String, Value>),
    /// `system_actuators {registry?, check?}`.
    SystemActuators(serde_json::Map<String, Value>),
    SystemLevel {
        path: String,
    },
    SystemSelect {
        names: Vec<String>,
    },
    SystemUndo,
    SystemRedo,
    SystemRun {
        action: String,
    },
    /// `system_drive`: one drive request for a robot system's run.
    SystemDrive {
        #[serde(default)]
        forward: Option<f64>,
        #[serde(default)]
        lateral: Option<f64>,
        #[serde(default)]
        yaw: Option<f64>,
        #[serde(default)]
        action: Option<String>,
        #[serde(default)]
        stop: Option<bool>,
    },
    SystemImportImage {
        path: std::path::PathBuf,
    },
    SystemSuggest {
        instance: String,
    },
    SystemSnap {
        instance: String,
        port: String,
        kind: sim_system::InstanceKind,
    },
    SystemComponent {
        component_type: String,
    },
    SystemStudy {
        name: String,
        #[serde(default)]
        study: Option<sim_system::Study>,
    },
    SystemStudyResult,
    SystemParts,
    SystemPublish {
        definition: String,
    },
    SystemLibraryUpdates,
    SystemSaveRun {
        #[serde(default)]
        note: String,
    },
    SystemCompareRuns {
        ids: Vec<String>,
    },
    SystemReplayRun {
        id: String,
    },
    SystemReplayCancel,
    SystemSync,
    SystemWhereUsed {
        definition: String,
    },
    SystemExpose {
        instance: String,
        parameter: String,
    },
    SystemPlot {
        #[serde(default)]
        pin: Option<Vec<String>>,
        #[serde(default)]
        visible: Option<bool>,
    },
    /// A button, key or marker: the chrome's action, applied by `dispatch`.
    #[serde(skip)]
    Ui(BuildAction),
    #[serde(skip)]
    RenderedUi { stamp: super::actions::RenderStamp, action: BuildAction },
}

/// `system_ui`: discover the builder's live controls and activate one (its
/// `BuildAction`, through `dispatch`), or make the gesture a click would
/// (tab, mode, a part click, an annotation target, a thread, draft text,
/// sidebar scroll) through the same handlers (`Builder::ui_request`).
#[derive(Deserialize, Clone)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum UiAction {
    Controls,
    Activate {
        id: String,
        ui_revision: u64,
    },
    Tab {
        tab: Tab,
    },
    Mode {
        mode: Mode,
    },
    ClickPart {
        component: String,
        #[serde(default)]
        add: bool,
        #[serde(default)]
        point_m: Option<[f32; 3]>,
    },
    Annotate {
        target: String,
        #[serde(default)]
        pin_m: [f32; 3],
    },
    OpenThread {
        id: String,
    },
    Input {
        text: String,
        expected_text: String,
        #[serde(default)]
        submit: bool,
    },
    CancelInput {
        expected_text: String,
    },
    /// Scroll the left sidebar to a pixel offset, as the mouse wheel would.
    Scroll {
        offset_y: f32,
    },
}

impl actions::Action for SystemAction {
    /// The commands read loosely before (`args.get`) take non-object args
    /// (`null`) as `{}`, as they did.
    fn parse(command: &sim_api::Command) -> Result<Self, String> {
        const LOOSE: [&str; 4] = ["system_open", "system_gait_reports", "system_calibration_review", "system_actuators"];
        if !command.args.is_object() && LOOSE.contains(&command.command.as_str()) {
            let empty = sim_api::Command { command: command.command.clone(), args: json!({}) };
            return sim_api::decode::<Self>(&empty);
        }
        sim_api::decode::<Self>(command)
    }
    fn commands() -> Vec<Spec> {
        fn c(name: &'static str, example: Value, description: &str) -> Spec {
            let description = if name == "system_calibration_review" {
                "Legacy read-only archive review compatibility API. Native Build → Actuators → Measured evidence authoring uses system_measured_study and the rendered study:* system_ui controls. This compatibility command does not author, evaluate or publish studies."
            } else { description };
            spec(name, actions::BUILDER, example, description)
        }
        vec![
            c("system_context",json!({"discussion":"thread-ID"}),"Read-only engineering context for an annotation or targets [instance/path]. Shared with Codex: inherited values, authored provenance, typed ports, complete nets, connected neighbors, registry explanations, model findings and display-only geometry. Resolved off the UI thread; no selection, model or undo changes. Empty args inspect the model. Poll the returned job URL."),
            c("system_agent",json!({"action":{"operation":"status"}}),"Codex annotation service: status/configure(auto_answer)/ask(discussion,question?,request_id?)/cancel(run)/retry(run)/mark_read(discussion)/activity. Shared UI actions; model/effort from SIM_CODEX_MODEL/SIM_CODEX_EFFORT (default gpt-6-astra/high, reported in status), read-only answer mode. GET /v1/agent and /v1/events/agent provide live state."),
            c("system_ui",json!({"action":{"operation":"controls"}}),"Discover live controls and activate them via the exact UI handlers. Also tab/mode/click_part/annotate/open_thread/input/cancel_input/scroll (scroll {offset_y: px} scrolls the left sidebar until the next panel rebuild). Activate requires control id and ui_revision; input uses expected_text to protect drafts. Physical placement remains display-only."),
            c("system_discussions",json!({"action":{"operation":"list"}}),"CAD-style threads: list/get/create/reply/edit_comment/delete_comment/resolve/delete/link/pin/title/show/highlight/inspect_target/back/import_legacy. Persistent part/group links; shared undo; optional expected_revision. Drafts are never replaced by REST."),
            c("system_grid",json!({}),"Read/set display-only grid (metres, Y up, enclosing definition frame). Never changes physics or CAD geometry. Optional expected_revision."),
            c("system_move",json!({"names":["motor"],"position_m":[0.04,0,0.02],"snap":true,"preview":true}),"Display-only move: first named instance is the anchor, others keep their offsets. Shared mouse/REST snapping, overlap report (allowed=false for invalid preview; commit rejects), atomic undo, expected_revision. Does NOT change physics/CAD."),
            c("system", json!({"label":"Place resistor","commands":[{"command":"add_instance","at":"","name":"r1","instance":{"kind":{"kind":"element","component_type":"electrical.resistor"},"parameters":{"resistance":{"value":100}}}}]}),
                "Apply sim-system commands atomically (same validation and shared undo history as both viewers and the CLI). Commands apply in order and each is checked as it applies: link_file {instance, path} (host a root robot.articulated, control.external or control.drive_limiter from its .simrobot.json, .controller.json or .drive.json, relative to the system file; path null unlinks, refused while the instance is still connected: disconnect it first) must come before any connect on a hosted port"),
            c("system_state", json!({}), "System file, revision, build level, selection, findings and compile status; workspace (the resolved root: root, found_by override | env | opened_file | cwd, from, error, rule; also in GET /v1/capabilities)"),
            c("system_open", json!({"path":"examples/systems-builder/worm-drive/winch.system.json"}), "Open another system file in this window (same handler as the Systems tab). Refuses, naming the blocker, while a text/discussion draft, placement drag, study, replay or Codex answer is in progress; a live run is stopped and saved to the old file's runs. Loads, validates and compiles off the UI thread (poll the job); a missing or invalid file is an error naming the path and the current system stays open. Writes no runs or annotations. system_state.open reports the pending/last open, discovered systems, the annotations sidecar and whether a --schematic window still shows the old file."),
            c("system_gait_reports", json!({"dir":"examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25/results"}), "Read-only gait-lab results browser (same handler as the Gait lab tab's path field, Reload and first visit). Reads every <dir>/*/report.yaml (gait, pose_sequence or maneuver by kind) plus <dir>/journal.jsonl with sim_runtime::gait_lab::scan_results, off the UI thread (poll the job). Default dir: examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25/results under the nearest ancestor of the system file, else the workspace root (system_state.workspace); omitted = the current one. Refused while a scan is pending. A missing dir is an error naming it; the last good listing stays in system_state.gait_reports with its own root. Result and system_state.gait_reports: root, journal, warnings, entries [{name, directory, kind, status, report (the report.yaml fields; null numbers mean not recorded/not simulated) | error (names the report.yaml), journal (unix_s, cached, line) or null}], plus pending, selected, error, caveat. Reports carry no runtime fingerprint, so qualification against the current code is unknown. Writes nothing and starts no evaluation."),
            c("system_calibration_review", json!({"path":"examples/actuators/hx30hm/pwm-full-range-identification","trial":"<trial id, e.g. from trials[].id>"}), "Read-only review of a measured actuator identification archive (same handler as the Actuators tab's Measured evidence path field, Reload/Cancel and first visit). Loads <path>/observations.json and results.json with the shared sim_runtime::experiment_comparison::hx_archive::load, verifying the archive's input hashes against the workspace root (system_state.workspace), off the UI thread (poll the job). Default path: examples/actuators/hx30hm/pwm-full-range-identification under the workspace root; omitted = the current one. Refused while a load is pending. A file (e.g. a study) or a sweep.csv folder is refused as not supported yet; any error names the path and the reason, and the last good archive stays in system_state.calibration_review with its own path. Result: path, repository, label, interpretation, split_policy (verbatim), observation_blake3, model_blake3, verified_inputs, input_blake3, integrity_issues, trial_count, counts (by_split/train/held_out/all: total, pass, fail, counted from each trial's comparison.passes; held-out = every split other than train), trials [{id, run, device, stage, kind, drive, duration_s, split, held_out, voltage_range_v, temperature_range_c, unit, limits {rmse, final_abs_error}, comparison {passes, rmse, maximum_abs_error, final_error} (in unit, rad; the archive's limits are 3 and 5 encoder counts of 2π/4096 rad), samples {measured, predicted}}]. system_state.calibration_review adds phase (idle/loading/loaded/failed), pending, requested, error, filters (split all/train/held_out, outcome all/pass/fail; set with system_ui), visible (filtered trial ids) and the current page of rows. Optional trial (a trial id): selects that trial through the same path as a trial row click and the system_ui action {\"calibration_trial\": id}; with no path it selects within the shown archive without reloading (result {selected}); with a path it selects once that load finishes (result gains selected). An unknown id is an error naming it and the previous selection stays; a reload that no longer contains the selected id clears the selection. system_state.calibration_review.selected (null when none) carries id, run, device, stage, kind, drive, duration_s, split, held_out, role (held-out (validation data) / train (fitting data)), quantity, unit, limits, comparison {passes, rmse, maximum_abs_error, final_error} and measured/predicted {source ('measured (hardware archive)' / 'predicted (fitted model, archive)'), quantity, unit, count (true sample count), first, last ({time_s, value}, null when empty)}; chart gives the shared-raster axes as drawn. Writes nothing; evaluates nothing."),
            c("system_actuators", json!({"registry":"examples/actuators/hx30hm/accepted/registry.json","check":["examples/full-robot/measured-actuator-integration/browser-control-400hz/scene.json"]}), "Read-only accepted actuator registry inspector (same handler as the Actuators tab). Loads the registry (default: examples/actuators/hx30hm/accepted/registry.json under the nearest ancestor of the system file, else the workspace root (system_state.workspace); omitted = the current one) and checks each consumer file with sim_runtime::actuator_registry (omitted check = recheck the previous files, [] = none), off the UI thread (poll the job). Refused while a load is pending. A missing registry or family hash mismatch is an error naming the path; the last good load stays in system_state.actuators with its own path. Result and system_state.actuators: families (content hash, acceptance, limitations, parameters with value/unit/provenance/uncertainty (null = unknown)/evidence), roles, per-file checks (current/stale/invalid, have and accepted hashes). Writes nothing."),
            c("system_level", json!({"path":"regulator"}), "Drill into a subsystem instance path (\"\" is the top level)"),
            c("system_select", json!({"names":["q1"]}), "Select instances at the current level: the shared selection's items of the Build document (the Outline's and a part click's path); an unknown name is refused, naming it, and the selection stays. Answers system_state"),
            c("system_undo", json!({}), "Undo the last edit in the shared history"),
            c("system_redo", json!({}), "Redo in the shared history"),
            c("system_run", json!({"action":"step"}), "Control the background run on the shared runtime: action start, pause, step (one timestep while paused) or reset (t = 0, paused; a run that reached 0.1 s is saved first). Same Builder methods as the Run/Pause/Step/Reset buttons"),
            c("system_drive", json!({"forward":0.5,"lateral":0,"yaw":0}), "Drive a robot system's run: a system whose root hosts a robot.articulated, a control.external and optionally a control.drive_limiter, each linked to its file (system command link_file: .simrobot.json, the robot's .controller.json binding, the .drive.json profile that binding names), running after system_run start on the shared drive host (sim_runtime::drive_host::DriveHost: the shared Session with the binding's external controller on the model's control.external seam; the same code Robot mode drives). Build it with one system batch in this order: add_instance (rover, controller, limiter), link_file each, set_parameter controller sense.command.<axis> (value 1), then connect limiter twist.<axis> to controller sense.command.<axis> (connect is checked as it applies, so it must follow link_file). Give exactly one of: axes forward, lateral, yaw (normalized -1..1: + ahead, + left, + turn left/CCW; absent ones are 0), scaled by the linked profile's max_speed per axis (kinematics::scale; a nonzero axis the profile does not support, a value outside -1..1 or a non-finite one is refused naming the axis); action (one of the profile's named actions: stop approaches zero under the profile's acceleration limit, halt zeroes at once; an unknown name is refused listing the profile's actions); or stop: true. Mixing them is refused naming the fields given. The same BuildAction::Drive as the run panel's Forward/Back/Left/Right/Stop buttons (system_ui). A nonzero request is accepted only while the run is running (system_run start); stop, halt and zero requests are accepted until the run fails or ends, and before the system has loaded only stop is. The run thread limits each request under the profile's max_accel and applies its deadman on simulated time, then sends the twist and a request heartbeat on the controller's command channels (command.forward/lateral/yaw/heartbeat); the controller mixes it. A request older than the profile's deadman timeout is lost and the robot stops, so a REST client must repeat its request faster than that to keep moving. Refused naming the reason: no run, a run that is not a robot system, a run that failed (a controller that exited, timed out or answered badly fails the run; the error names the instance and the controller) or ended, a reset in progress; a refusal is kept in system_state.live_run.drive.last_refusal. Answers system_state.live_run.drive: phase, system (instances and files, limits with units, deadman, period, channels, wiring), status (request and commanded twist, heartbeat, deadman age), requested, last_refusal, last_apply_error and error. No keyboard or gamepad bindings in Build mode yet."),
            c("system_import_image", json!({"path":"/abs/board.png"}), "Import a PNG/JPEG as a reference image at the current level"),
            c("system_suggest", json!({"instance":"motor"}), "What can snap onto each port of an instance at the current level (typed, curated first, conflicts explained)"),
            c("system_snap", json!({"instance":"motor","port":"shaft","kind":{"kind":"element","component_type":"rotational.worm_gear"}}), "Place a fitting part next to an instance and connect it to that port (one undoable edit)"),
            c("system_component", json!({"component_type":"rotational.worm_gear"}), "Library entry: ports, parameters, notes, equations, trade-offs and derived values"),
            c("system_study", json!({"name":"gearboxes"}), "Run a saved comparison or sweep in the background (pass `study` to save it first); overlays results in the graph dock"),
            c("system_study_result", json!({}), "Latest study result: variants, metrics, derived values, trade-off table; `running` while in progress"),
            c("system_publish", json!({"definition":"dc_motor_12v"}), "Publish a definition to the library as a new version (refreshes files that bundle it)"),
            c("system_library_updates", json!({}), "Imported definitions whose library file changed"),
            c("system_sync", json!({}), "Update every stale import from the library (one undoable edit)"),
            c("system_where_used", json!({"definition":"dc_motor_12v"}), "System files under examples/ and next to this file that place a definition"),
            c("system_expose", json!({"instance":"winding","parameter":"resistance"}), "Expose an inner parameter as a parameter of the current level's definition"),
            c("system_save_run", json!({"note":"after the k edit"}), "Keep the current run (document, seed, settings, recorded history) in <system>.runs/"),
            c("system_replay_run", json!({"id":"…"}), "Rerun a saved run headlessly from t = 0 on a background thread and compare it with its record at every recorded sample (same handler as the Studies-tab Replay button); progress and result in system_state.replay"),
            c("system_replay_cancel", json!({}), "Stop the running replay between simulation steps (same handler as its Cancel button); it reports no result"),
            c("system_compare_runs", json!({"ids":["…","…"]}), "Overlay saved runs in the graph dock with a table of final values"),
            c("system_parts", json!({}), "Authored part files (library/parts/*.part): load results, errors with file:line; reloads changed files"),
            c("system_plot", json!({"pin":["drum.shaft.speed"],"visible":true}), "Pin observables (IDs or readable keys) to the graph dock, or clear with []"),
        ]
    }
    fn controls() -> &'static [&'static str] {
        // One id per distinct button action: a hash of its serialized `BuildAction` (`ui_api::collect`).
        &["control-<hash>"]
    }
}

/// Why the builder cannot answer a command: this window has no builder.
fn no_builder(action: &SystemAction) -> String {
    match action {
        SystemAction::SystemContext(_) => "start the viewer with --system FILE to inspect systems",
        SystemAction::SystemOpen(_) => "start the viewer with --system FILE to open systems",
        SystemAction::SystemGaitReports(_) => "start the viewer with --system FILE to browse gait-lab results",
        SystemAction::SystemCalibrationReview(_) => "start the viewer with --system FILE to review identification archives",
        SystemAction::SystemActuators(_) => "start the viewer with --system FILE to inspect actuators",
        _ => "start the viewer with --system FILE to edit systems",
    }
    .into()
}

/// The builder's REST commands (a `Ui` action goes to `dispatch`). Loads,
/// scans and context builds answer Pending and are applied again each
/// frame with their continuation until done; `call.cancelled` stops them.
/// `lessons`: a lesson is open in the window (the "‹ lesson" control's switch
/// needs one); `switch` writes the mode switch.
/// `pick`: the shared selection (`system_select`, `system_ui` rows and part
/// clicks are its adapters; a `system_state` answer lists it, re-checked).
#[allow(clippy::too_many_arguments)]
fn execute(builder: &mut Builder, scene: &mut SpatialScene, camera: &mut Orbit, pick: &mut Picked, lessons: bool, switch: &mut MessageWriter<Act<WindowAction>>, action: &SystemAction, call: &mut Call) -> Outcome {
    let object = |args: &serde_json::Map<String, Value>| Value::Object(args.clone());
    let result = match action {
        SystemAction::RenderedUi { stamp, action } => {
            if !stamp.matches(builder) || stamp.document!=pick.document() {return Outcome::Done(Err("Rendered Build control source changed; activate the current panel".into()));}
            dispatch(builder,scene,camera,pick,action.clone());
            return Outcome::Done(Ok(Value::Null));
        }
        SystemAction::Ui(action) => {
            dispatch(builder, scene, camera, pick, action.clone());
            return Outcome::Done(Ok(Value::Null));
        }
        SystemAction::SystemContext(args) => {
            return match serde_json::from_value::<sim_model_context::Request>(object(args)) {
                Ok(request) => builder.context_request(request, &mut *call.continuation, call.cancelled),
                Err(e) => Outcome::Done(Err(e.to_string())),
            };
        }
        SystemAction::SystemOpen(args) => {
            return match args.get("path").and_then(|p| p.as_str()) {
                Some(path) => builder.open_request(std::path::PathBuf::from(path), &mut *call.continuation, call.cancelled),
                None => Outcome::Done(Err("system_open needs {\"path\": \"…/file.system.json\"}".into())),
            };
        }
        SystemAction::SystemGaitReports(args) => return builder.gait_reports_rest(&object(args), &mut *call.continuation, call.cancelled),
        SystemAction::SystemCalibrationReview(args) => return builder.calibration_rest(&object(args), &mut *call.continuation, call.cancelled),
        SystemAction::SystemActuators(args) => return builder.actuators_rest(&object(args), &mut *call.continuation, call.cancelled),
        SystemAction::SystemAgent { action } => builder.agent_request(action.clone()),
        SystemAction::SystemUi { action, expected_revision } => {
            // The "‹ lesson" control: the same mode switch its button writes; the
            // result is the switch's (a refusal names the blocker).
            if let Some(outcome) = awaited_switch(call) {
                return outcome;
            }
            match builder.activates_lessons(action, *expected_revision) {
                Ok(true) if lessons => return ask_switch(switch, call, ViewerMode::Lessons),
                Ok(true) => Err("the ‹ lesson control needs a lesson open in this window".into()),
                Ok(false) => builder.ui_request(action.clone(), *expected_revision, scene, camera, pick),
                Err(e) => Err(e),
            }
        }
        SystemAction::SystemDiscussions { action, expected_revision } => builder.discussion_request(action.clone(), *expected_revision, scene, camera, pick),
        SystemAction::System { label, commands, expected_revision } => (|| -> sim_api::Result {
            if expected_revision.is_some_and(|r| r != builder.document.revision) {
                return Err("stale system revision; reload system_state".into());
            }
            let label = label.clone().unwrap_or_else(|| format!("{} command(s) via REST", commands.len()));
            builder.apply(&label, commands.clone()).map(|a| json!(a))
        })(),
        SystemAction::SystemGrid { grid, expected_revision } => (|| -> sim_api::Result {
            if expected_revision.is_some_and(|r| r != builder.document.revision) {
                return Err("stale display grid; reload system_state".into());
            }
            if let Some(grid) = grid {
                builder.set_grid(grid.clone())?;
            }
            Ok(json!({"grid":builder.grid(),"semantics":sim_system::display::SEMANTICS,"frame":"enclosing_definition","unit":"m","revision":builder.document.revision}))
        })(),
        SystemAction::SystemMove { names, position_m, snap, preview, expected_revision } => builder.display_move(names.clone(), *position_m, *snap, *preview, *expected_revision),
        SystemAction::SystemState => Ok(pick.state(builder)),
        SystemAction::SystemLevel { path } => builder.enter_level(pick, path).map(|_| pick.state(builder)),
        SystemAction::SystemSelect { names } => builder.select(pick, names.clone()).map(|_| pick.state(builder)),
        SystemAction::SystemUndo => builder.undo().map(|a| json!(a)),
        SystemAction::SystemRedo => builder.redo().map(|a| json!(a)),
        SystemAction::SystemRun { action } => (|| -> sim_api::Result {
            match action.as_str() {
                "start" => builder.run_start(scene),
                "pause" => builder.run_pause(),
                "step" => builder.run_step()?,
                "reset" => builder.run_reset()?,
                other => return Err(format!("unknown run action `{other}` (expected start, pause, step or reset)")),
            }
            Ok(pick.state(builder))
        })(),
        SystemAction::SystemDrive { forward, lateral, yaw, action, stop } => {
            sim_runtime::drive_host::DriveRequest::from_fields(*forward, *lateral, *yaw, action.clone(), *stop, "system_drive").and_then(|request| builder.drive(request))
        }
        SystemAction::SystemImportImage { path } => builder.import_image(path.clone()).map(|_| pick.state(builder)),
        SystemAction::SystemSuggest { instance } => builder.suggestions(instance).map(|s| json!(s)),
        SystemAction::SystemSnap { instance, port, kind } => (|| -> sim_api::Result {
            let candidate = builder
                .suggestions(instance)?
                .into_iter()
                .find(|p| &p.port == port)
                .ok_or_else(|| format!("{instance} has no port `{port}`"))?
                .candidates
                .into_iter()
                .find(|c| &c.kind == kind)
                .ok_or_else(|| format!("{} does not fit {instance}.{port}", sim_system::commands::kind_label(kind)))?;
            let name = builder.snap(pick, instance, port, &candidate)?;
            Ok(json!({"name": name, "state": pick.state(builder)}))
        })(),
        SystemAction::SystemComponent { component_type } => builder.component_json(component_type),
        SystemAction::SystemStudy { name, study } => (|| -> sim_api::Result {
            match study {
                Some(study) => builder.save_and_run_study(name, study.clone())?,
                None => builder.run_study(name)?,
            }
            Ok(json!({"running": name}))
        })(),
        SystemAction::SystemStudyResult => Ok(builder.study_json()),
        SystemAction::SystemPublish { definition } => builder.publish(definition).map(|p| json!(p)),
        SystemAction::SystemSaveRun { note } => builder.save_run(note).map(|p| json!({"path": p})),
        SystemAction::SystemReplayRun { id } => builder.replay_run(id).map(|_| builder.replay_json()),
        SystemAction::SystemReplayCancel => {
            let cancelled = builder.cancel_replay();
            Ok(json!({"cancelled": cancelled, "replay": builder.replay_json()}))
        }
        SystemAction::SystemCompareRuns { ids } => builder.compare_runs(ids).map(|_| builder.study_json()),
        SystemAction::SystemLibraryUpdates => Ok(json!(builder.library_updates())),
        SystemAction::SystemSync => builder.sync_library().map(|a| json!(a)),
        SystemAction::SystemWhereUsed { definition } => Ok(json!(builder.where_used(definition))),
        SystemAction::SystemExpose { instance, parameter } => builder.expose(instance, parameter).map(|a| json!(a)),
        SystemAction::SystemParts => {
            builder.reload_parts();
            Ok(builder.parts_json())
        }
        SystemAction::SystemPlot { pin, visible } => builder.set_plots(scene, pin.clone(), *visible),
    };
    Outcome::Done(result)
}

/// Actions: the builder's one apply system (build and lessons). Buttons,
/// keys and markers go to `dispatch` (a refusal is the status line); REST
/// commands answer their caller.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    mut messages: ResMut<Messages<Act<SystemAction>>>,
    mut in_flight: ResMut<InFlight<SystemAction>>,
    mut replies: ResMut<Replies>,
    builder: Option<ResMut<Builder>>,
    scene: Option<ResMut<SpatialScene>>,
    orbit: Option<Single<&mut Orbit>>,
    rest: Option<Res<crate::rest::Rest>>,
    learn: Option<Res<crate::lesson::Learn>>,
    mut switch: MessageWriter<Act<WindowAction>>,
    mut selection: ResMut<Selection>,
    mut registry: ResMut<DocumentRegistry>,
    studies: Option<Res<calibration::study::StudyOwner>>,
    study_ui: Option<Res<calibration::study::forms::StudyUi>>,
) {
    let (Some(mut builder), Some(mut scene), Some(mut orbit)) = (builder, scene, orbit) else {
        actions::apply(&mut messages, &mut in_flight, &mut replies, |action, _| Outcome::Done(Err(no_builder(action))));
        return;
    };
    // The agent answers through this window's server (set every frame, as the
    // REST poll did, so an automatic answer started by `agent::tick` has it).
    if let Some(rest) = rest {
        let url = format!("http://{}", rest.0.address);
        if builder.agent.endpoint.as_deref() != Some(url.as_str()) {
            builder.agent_endpoint(url);
        }
    }
    if messages.is_empty() && in_flight.is_empty() {
        return;
    }
    let lessons = learn.is_some();
    let mut pick = Picked::new(&mut selection, &mut registry);
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let opening = match action {
            SystemAction::SystemOpen(_) => call.continuation.get("open").is_none(),
            SystemAction::Ui(BuildAction::OpenSystem(_)) | SystemAction::RenderedUi { action: BuildAction::OpenSystem(_), .. } => true,
            SystemAction::Ui(BuildAction::SubmitDraft) | SystemAction::RenderedUi { action: BuildAction::SubmitDraft, .. } => builder.input.as_ref().is_some_and(|i| i.purpose == Purpose::OpenSystem),
            SystemAction::SystemUi { action, .. } => builder.opens_system_ui(action),
            _ => false,
        };
        if opening {
            if let Some(reason) = study_ui.as_ref().and_then(|ui| ui.blocking_reason()) {
                return Outcome::Done(Err(format!("system_open refused: {reason}")));
            }
            if let Some(reason) = studies.as_ref().and_then(|s| s.blocking_reason()) {
                return Outcome::Done(Err(format!("system_open refused: {reason}")));
            }
        }
        let outcome = execute(&mut builder, &mut scene, &mut orbit, &mut pick, lessons, &mut switch, action, call);
        // An edit re-checks the selection at once (a later action in this frame sees it).
        pick.sync(&builder);
        outcome
    });
}
