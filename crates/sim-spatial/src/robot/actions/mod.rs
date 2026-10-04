//! Robot mode's actions: [`RobotAction`] (one enum for the buttons, keys,
//! `system_ui` controls and REST commands), its REST form, its commands in
//! the action registry, the input systems that write it and [`apply`], its
//! one handler (`ViewerSet::Actions`).
use super::*;
use crate::app::actions::{self, Act, InFlight, Origin, Replies};
use crate::robot::run::SPEED_SCALES;
use bevy::ecs::message::Messages;
use sim_api::Outcome;
use sim_domain_control::drive::kinematics::BodyTwist;

/// Every intent of robot mode: the buttons and keys carry these values,
/// `system_ui` lists them (the `action` of each control, serialized as
/// before) and REST commands deserialize into them through their JSON form
/// ([`wire::Command`]). Applied by [`apply`], the mode's one handler.
#[derive(Component, Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case", try_from = "wire::Command")]
pub(crate) enum RobotAction {
    SelectLink { index: usize, name: String },
    ClearSelection,
    ShowSection { section: Section },
    /// Scroll the inspector by logical pixels (positive is down).
    ScrollInspector { delta: f32 },
    /// Frame the robot's bounds at 3.2 × their extent from the current
    /// heading (the trackball stays when it is on). Recorded difference
    /// (intended, since the shared camera): Fit also moves the focus to the
    /// bounds' centre, as RoboCAD's fit (`Camera.focus`) does; Robot's
    /// former Fit only reset the distance and kept the focus.
    Fit,
    /// Run/Pause/Step/Reset on the run thread.
    Run { action: RunAction },
    /// Servo-target jog by a step from the current requested target (the +/− buttons, `system_ui` jog:*).
    Jog { joint: String, delta: f64 },
    /// Servo-target jog to an absolute target (REST `robot_jog`).
    JogTo { joint: String, target: f64 },
    /// A planar (v2) file: the joint the arrow keys move (←/→, `system_ui` joint:<index>; built joint order).
    SelectJoint { index: usize },
    /// A motion request through the preset's Rust controller (physical keys,
    /// the W/A/S/D/Stop buttons, `system_ui` motion:*, REST `robot_input`).
    Motion { request: MotionRequest },
    /// Set named session inputs of a preset's held action (the Inputs block's
    /// sliders and buttons, `system_ui` inputs:*, REST `robot_inputs`).
    Inputs { values: std::collections::BTreeMap<String, f64> },
    /// Save the preset run's shared recording (the Save recording button,
    /// `system_ui` recording:save, REST `robot_save_recording`).
    SaveRecording { path: Option<String>, note: Option<String> },
    /// Replay a saved recording through the shared prepare_replay (the inspector
    /// Replay buttons, `system_ui` replay:<file>, REST `robot_replay`).
    Replay { file: Option<String>, path: Option<String> },
    /// Cancel the replay between chunks (Cancel button, `system_ui` replay:cancel, REST `robot_replay {action: cancel}`).
    CancelReplay,
    /// List the preset's saved recordings again (off the UI thread).
    RefreshRecordings,
    /// Show or hide the graph dock (the Graphs button, key G, `system_ui` graphs:toggle).
    ToggleGraphs,
    /// Record video of the 3D view (`video`): on, off, or the flip (None; the header button, REST `robot_video`).
    Video { on: Option<bool> },
    /// The controller leaderboard (`leaderboard`): the dialog's controls and REST `robot_leaderboard`.
    Leaderboard { op: crate::robot::leaderboard::BoardOp },
    /// Review the run's frame history at time `t` (None: back to live): the
    /// Timeline block's slider and Live button, `system_ui` history:live, REST `robot_history`.
    History { t: Option<f64> },
    /// Pick (`on`) or remove a channel of the graph dock's Picked chart
    /// (the dock's channel chips, `system_ui` pick:<key>, REST `robot_graphs`).
    Pick { channel: String, on: bool },
    /// The view tools (`view_tools`): Fit selected, Follow robot and the
    /// display cap (the view-tool chips, `system_ui` view:*, REST `robot_view`).
    View { fit_selected: bool, follow: Option<bool>, display_hz: Option<u32> },
    /// The kinematic gait preview: open, play, pause, seek, speed, stop, list (REST `robot_gait`).
    Gait { action: GaitAction },
    /// Re-read `--robot FILE` on a worker (`robot_source`): the watch (a changed
    /// stat), the Reload button, `system_ui` robot:reload and REST `robot_reload`.
    Reload { trigger: ReloadTrigger },
    /// Set the overlays (`--robot FILE`); a None flag keeps its value. Keys C/J/F/H, the
    /// inspector overlay buttons and `system_ui` overlay:* send the flipped flag; REST `robot_overlay` sends values.
    /// contacts, joints and deflections are run-thread overlays; stress colours the link meshes from the results file.
    Overlay { contacts: Option<bool>, joints: Option<bool>, deflections: Option<bool>, stress: Option<bool> },
    /// The run speed scale (run::PACING; pacing only): keys =/+ and −, the header
    /// −/×scale/+ buttons, `system_ui` run:speed_* and REST `robot_speed`.
    Speed { speed: SpeedRequest },
    /// The recorded-preset timeline (robot_playback): play, pause, seek, step, speed, start —
    /// the inspector Recorded buttons, `system_ui` recorded:* and REST `robot_recorded`.
    Recorded { action: RecordedAction },
    /// REST `state`: `{robot_state}`.
    State,
    /// REST `robot_state`.
    RobotState,
    /// REST `robot_presets`: the declared presets.
    Presets,
    /// REST `robot_guide` (also `GET /v1/robot_guide`): how robot mode works, for an agent starting cold.
    Guide { topic: Option<String> },
    /// REST `robot_preset`: open a declared preset in this window.
    OpenPreset { id: String },
    /// REST `robot_open`: open a `*.simrobot.json` in this window in place of the current robot.
    Open { path: std::path::PathBuf },
    /// REST `system_ui` controls: the control list with each control's action.
    Controls,
    /// REST `system_ui` activate: the listed control's action, through this handler.
    Activate { id: String, ui_revision: u64 },
    /// REST `camera`: an absolute orbit.
    Camera { focus: [f32; 3], radius: f32, yaw: f32, pitch: f32 },
    /// RoboCAD's comment threads on the CAD source (the Comments section's
    /// presses and composer, REST `robot_threads`), applied by
    /// `threads::handle` from [`apply`].
    Threads { act: crate::robot::threads::ThreadsAct },
    /// A drive request for a controlled robot (a controller binding with a
    /// `sim.drive/1` profile): the bound keys and gamepad
    /// (`crate::drive_input::input::devices`, read by [`apply`] through [`device_action`]),
    /// `system_ui` drive:* and REST `robot_drive`.
    /// Applied by [`drive_request`] then `RunController::drive`.
    Drive { request: DriveRequest },
}

/// What a drive request asks for: the shared vocabulary (normalized axes, a
/// profile action, stop) Build mode's robot systems use too, interpreted
/// against the robot's profile by `DriveRequest::interpret`.
pub(crate) use sim_runtime::drive_host::DriveRequest;

/// How a drive request's refusal for a run without a drive profile starts;
/// the run's own reason follows (`RunController::check_drive`: no binding
/// beside the model, a binding that failed to load, a preset).
pub(crate) const NOT_CONTROLLED: &str = "this robot has no drive profile";
/// A planar (v2) file has no drive profile or controller.
const PLANAR_DRIVE: &str = "drive (robot_drive, drive:*) is refused for a planar v2 file: it has no controller binding or sim.drive/1 profile";

/// Thread acts need the thread state, which only [`apply`] holds.
const THREADS_IN_APPLY: &str = "comment thread actions are applied by robot mode's apply system (threads::handle)";
/// The planar view, for a handler that `check_planar` already accepted.
fn planar(view: &mut RobotView) -> Result<&mut PlanarView, String> {
    view.planar.as_mut().ok_or_else(|| "the planar v2 file is no longer loaded".to_string())
}
/// `SelectLink`: link `index` of the loaded model (a body of a planar file),
/// checked against the name the control carries, becomes the one selected
/// link of the Robot document (`picked::select`, `Selection::apply`).
fn select_link(view: &mut RobotView, selection: &mut Selection, registry: &DocumentRegistry, index: usize, name: &str) -> Result<(), String> {
    let actual = match view.link_name(index) {
        None => return Err(format!("no link {index} in the loaded model; request controls again")),
        Some(actual) if actual != name => return Err(format!("link {index} is `{actual}`, not `{name}`; request controls again")),
        Some(actual) => actual.to_string(),
    };
    picked::select(selection, registry, index, actual)?;
    view.scroll_to = Some(0.0);
    Ok(())
}
/// A control action on a planar (v2) file: validated by [`check`], then applied
/// to the planar run (Reset rebuilds from the loaded model; Reload re-reads the file).
fn dispatch_planar(view: &mut RobotView, orbit: &mut Orbit, selection: &mut Selection, registry: &DocumentRegistry, action: RobotAction) -> Result<(), String> {
    check(view, &action)?;
    match action {
        RobotAction::Run { action } => planar(view)?.run.act(action)?,
        RobotAction::Jog { joint, delta } => planar(view)?.run.nudge(&joint, delta)?,
        RobotAction::JogTo { joint, target } => planar(view)?.run.set_target(&joint, target)?,
        RobotAction::SelectJoint { index } => {
            let p = planar(view)?;
            (p.selected_joint, p.pending_joint) = (index, None);
        }
        RobotAction::Speed { speed } => planar(view)?.run.speed(speed)?,
        RobotAction::Overlay { contacts, .. } => {
            if let Some(on) = contacts {
                planar(view)?.contacts = on;
            }
        }
        // Checked above; applied in `receive` when the worker finishes.
        RobotAction::Reload { trigger } => view.source.as_mut().ok_or("a preset is not reloaded; reload is for --robot FILE")?.start(trigger)?,
        RobotAction::SelectLink { index, name } => select_link(view, selection, registry, index, &name)?,
        RobotAction::ClearSelection => {
            picked::clear(selection, registry)?;
        }
        RobotAction::ShowSection { section } => {
            view.section = section;
            view.scroll_to = Some(0.0);
        }
        RobotAction::ScrollInspector { delta } => view.scroll_to = Some((view.scroll + delta).clamp(0.0, view.scroll_max)),
        RobotAction::View { display_hz, .. } => {
            if let Some(hz) = display_hz {
                view.display_hz = hz;
            }
        }
        RobotAction::History { .. } | RobotAction::Pick { .. } => return Err("refused for a planar v2 file".into()),
        RobotAction::Leaderboard { .. } | RobotAction::Video { .. } => unreachable!("answered by `apply`"),
        RobotAction::Fit => {
            // Frame the latest outlines again (front view), as an open does.
            planar(view)?.frame_camera = Some(true);
            orbit.home = true;
        }
        // Refused by `check_planar` (named); listed so a new action is not silently accepted.
        RobotAction::Motion { .. } | RobotAction::Inputs { .. } | RobotAction::Drive { .. } | RobotAction::SaveRecording { .. } | RobotAction::Replay { .. } | RobotAction::CancelReplay | RobotAction::RefreshRecordings
        | RobotAction::Gait { .. } | RobotAction::Recorded { .. } | RobotAction::ToggleGraphs => return Err("refused for a planar v2 file".into()),
        RobotAction::State | RobotAction::RobotState | RobotAction::Presets | RobotAction::Guide { .. } | RobotAction::OpenPreset { .. } | RobotAction::Open { .. } | RobotAction::Controls | RobotAction::Activate { .. } | RobotAction::Camera { .. } => unreachable!("answered by `handle`"),
        RobotAction::Threads { .. } => return Err(THREADS_IN_APPLY.into()),
    }
    Ok(())
}
/// A control action: validated by [`check`] (motion, save and drive validate
/// in their own handler), then applied.
fn dispatch(view: &mut RobotView, orbit: &mut Orbit, selection: &mut Selection, registry: &DocumentRegistry, action: RobotAction) -> Result<(), String> {
    if view.planar.is_some() {
        return dispatch_planar(view, orbit, selection, registry, action);
    }
    if let RobotAction::Motion { request } = action {
        // Validated (and a refusal recorded) inside the one motion handler.
        return view.run.as_mut().ok_or("the robot has not loaded")?.motion(request);
    }
    if let RobotAction::Inputs { values } = &action {
        // Validated (and a refusal recorded) inside the one input handler.
        return view.run.as_mut().ok_or("the robot has not loaded")?.set_inputs_named(values);
    }
    if let RobotAction::SaveRecording { path, note } = action {
        // Validated (and a refusal recorded) inside the one save handler.
        return view.run.as_mut().ok_or("the robot has not loaded")?.save_recording(path.as_deref(), note.as_deref()).map(|_| ());
    }
    if let RobotAction::Drive { request } = action {
        // The request is interpreted against the profile here (`drive_request`:
        // no profile, an unsupported or out-of-range axis, an unknown action),
        // then validated against the run (failed, ended, replaying, not
        // running) inside the one drive handler, which records that refusal
        // for robot_state.drive.last_refusal. The run thread limits it under
        // the profile's acceleration and deadman rule.
        let run = view.run.as_mut().ok_or("the robot has not loaded")?;
        let (twist, halt) = drive_request(run, &request)?;
        return run.drive(twist, halt);
    }
    check(view, &action)?;
    match action {
        RobotAction::Motion { .. } | RobotAction::Inputs { .. } | RobotAction::SaveRecording { .. } | RobotAction::Drive { .. } => unreachable!("handled above"),
        RobotAction::State | RobotAction::RobotState | RobotAction::Presets | RobotAction::Guide { .. } | RobotAction::OpenPreset { .. } | RobotAction::Open { .. } | RobotAction::Controls | RobotAction::Activate { .. } | RobotAction::Camera { .. } => unreachable!("answered by `handle`"),
        RobotAction::Threads { .. } => return Err(THREADS_IN_APPLY.into()),
        RobotAction::Replay { file, path } => {
            view.run.as_mut().ok_or("the robot has not loaded")?.replay(file.as_deref(), path.as_deref())?;
        }
        RobotAction::CancelReplay => view.run.as_mut().ok_or("the robot has not loaded")?.cancel_replay()?,
        RobotAction::RefreshRecordings => view.run.as_mut().ok_or("the robot has not loaded")?.refresh_recordings(),
        RobotAction::Run { action } => {
            view.run.as_mut().ok_or("the robot has not loaded")?.act(action)?;
            if action == RunAction::Reset {
                // Old-generation frames are stale: show the assembly pose until the rebuild publishes t = 0.
                view.pose_dirty = true;
            }
        }
        RobotAction::Jog { joint, delta } => {
            let run = view.run.as_mut().ok_or("the robot has not loaded")?;
            let target = jog_target(run, &joint, delta)?;
            run.jog(&joint, target)?;
        }
        RobotAction::JogTo { joint, target } => view.run.as_mut().ok_or("the robot has not loaded")?.jog(&joint, target)?,
        RobotAction::SelectJoint { .. } => unreachable!("refused by `check` without a planar file"),
        RobotAction::SelectLink { index, name } => select_link(view, selection, registry, index, &name)?,
        RobotAction::ClearSelection => {
            picked::clear(selection, registry)?;
        }
        RobotAction::ShowSection { section } => {
            view.section = section;
            view.scroll_to = Some(0.0);
        }
        RobotAction::ScrollInspector { delta } => view.scroll_to = Some((view.scroll + delta).clamp(0.0, view.scroll_max)),
        // The bounds at 3.2 × extent from the current heading, focus on their
        // centre (`camera::place`; see the variant's doc); after a Fit selected, the whole robot's again.
        RobotAction::Fit => {
            if let Some((lo, hi)) = view.bounds {
                orbit.extent = ((hi - lo).length() / 2.0).max(0.02);
                orbit.centre = (lo + hi) / 2.0;
            }
            orbit.home = true;
        }
        RobotAction::Leaderboard { .. } | RobotAction::Video { .. } => unreachable!("answered by `apply`"),
        RobotAction::History { t } => {
            view.run.as_mut().ok_or("the robot has not loaded")?.review(t)?;
            view.pose_dirty = true;
        }
        RobotAction::Pick { channel, on } => {
            if on {
                view.picks.push(channel);
            } else {
                view.picks.retain(|p| *p != channel);
            }
        }
        RobotAction::View { fit_selected, follow, display_hz } => {
            if fit_selected {
                picked::link(selection, registry).ok_or("Fit selected: select a link first (the list or the 3D view)")?;
                view.fit_selected = true;
            }
            if let Some(on) = follow {
                if on && view_tools::follow_target(view, picked::link(selection, registry)).is_none() {
                    return Err("Follow robot needs a link to follow: this robot declares no follow_link, so select one".into());
                }
                view.follow = on;
            }
            if let Some(hz) = display_hz {
                view.display_hz = hz;
            }
        }
        RobotAction::ToggleGraphs => view.graphs_visible = !view.graphs_visible,
        RobotAction::Speed { speed } => view.run.as_mut().ok_or("the robot has not loaded")?.speed(speed)?,
        RobotAction::Recorded { action } => view.run.as_mut().ok_or("the robot has not loaded")?.recorded_act(action)?,
        // Checked above; applied in `receive` when the worker finishes.
        RobotAction::Reload { trigger } => view.source.as_mut().ok_or("a preset is not reloaded; reload is for --robot FILE")?.start(trigger)?,
        RobotAction::Overlay { contacts, joints, deflections, stress } => {
            if contacts.is_some() || joints.is_some() || deflections.is_some() {
                let run = view.run.as_mut().ok_or("the robot has not loaded")?;
                let f = run.overlays();
                run.set_overlays(OverlayFlags { contacts: contacts.unwrap_or(f.contacts), joints: joints.unwrap_or(f.joints), deflections: deflections.unwrap_or(f.deflections) })?;
            }
            if let Some(on) = stress {
                if on && !view.stress.enabled {
                    // Re-read the results file on a worker: one written since the model loaded is picked up.
                    let path = view.path.clone();
                    view.stress.refresh(&path);
                }
                view.stress.enabled = on;
                view.stress.revision += 1;
            }
        }
        RobotAction::Gait { action } => {
            let stop = action == GaitAction::Stop;
            view.run.as_mut().ok_or("the robot has not loaded")?.gait(action)?;
            if stop {
                // The live frame (or the assembly pose) shows again at once.
                view.pose_dirty = true;
            }
        }
    }
    Ok(())
}
/// The one handler of robot mode's actions: REST reads and the view's own
/// requests here, control actions through `dispatch` (validated by
/// `check`). `Ok(None)`: the answer is `robot_state`. The selected link is
/// the shared selection's (`picked`).
fn handle(view: &mut RobotView, orbit: &mut Orbit, selection: &mut Selection, registry: &mut DocumentRegistry, action: &RobotAction) -> Result<Option<Value>, String> {
    let link = picked::link(selection, registry);
    match action {
        RobotAction::State => Ok(Some(json!({"robot_state": view.state_json(link)}))),
        RobotAction::RobotState => Ok(None),
        RobotAction::Guide { topic } => crate::robot::guide::guide(topic.as_deref()).map(Some),
        RobotAction::Presets => {
            let (file, root) = (view.presets.clone()?, view.root.clone()?);
            let presets = crate::robot::preset::list(&file)?;
            let mut rows: Vec<Value> = presets.iter().map(|p| p.discovery(&root)).collect();
            // The leaderboard's tested recipes, as the browser packages them (tested-<id>, mode embedded).
            if let Ok(catalog) = crate::workspace::path(sim_runtime::controller_leaderboard::CATALOG).and_then(|p| sim_runtime::controller_leaderboard::read(&p)) {
                for e in &catalog.entries {
                    let p = sim_runtime::controller_leaderboard::tested_preset(e);
                    let preset = crate::robot::preset::Preset { id: p["id"].as_str().unwrap_or("").into(), mode: "embedded".into(), label: p["label"].as_str().unwrap_or("").into(), scene: p["scene"].as_str().map(Into::into), config: p["config"].as_str().map(Into::into), task: p["task"].as_str().map(Into::into), capture: None, entry: p };
                    let mut row = preset.discovery(&root);
                    row["leaderboard_entry"] = e["id"].clone();
                    rows.push(row);
                }
            }
            Ok(Some(json!({"presets_file": file, "root": root, "workspace": crate::workspace::json(), "count": rows.len(), "presets": rows,
                "current": view.preset.as_ref().map(|p| &p.id)})))
        }
        RobotAction::OpenPreset { id } => {
            // Refused here (naming the id) before anything is replaced; the old run thread stops when its controller drops.
            let mut next = RobotView::open_preset(&view.presets.clone()?, id)?;
            next.ui_revision = view.ui_revision + 1;
            // Pinned picks and the display cap carry over to the next robot.
            next.picks = view.picks.clone();
            next.display_hz = view.display_hz;
            // The old view's run and playback threads are joined off the UI thread, as leave_robot does.
            crate::jobs::drop_off_thread(std::mem::replace(view, next), "the robot view");
            // The Robot document is now the preset; the old one's link goes with it.
            picked::opened_preset(selection, registry, id);
            Ok(None)
        }
        RobotAction::Open { path } => {
            open_file(view, selection, registry, path)?;
            Ok(None)
        }
        RobotAction::Controls => {
            let items: Vec<Value> = controls(view, link).into_iter().map(|(id, label, action)| json!({"id": id, "label": label, "enabled": check(view, &action).is_ok(), "disabled_reason": check(view, &action).err(), "action": action})).collect();
            Ok(Some(json!({"ui_revision": view.ui_revision, "ready": view.panels_ready, "controls": items, "state": view.state_json(link)})))
        }
        RobotAction::Activate { id, ui_revision } => {
            if !view.panels_ready || *ui_revision != view.ui_revision {
                return Err("UI changed; request controls again before activating".into());
            }
            let (_, _, action) = controls(view, link).into_iter().find(|(i, _, _)| i == id).ok_or("unknown control; request controls")?;
            dispatch(view, orbit, selection, registry, action).map(|()| None)
        }
        RobotAction::Camera { focus, radius, yaw, pitch } => {
            let (radius, yaw, pitch) = (*radius, *yaw, *pitch);
            if !focus.iter().chain([radius, yaw, pitch].iter()).all(|x| x.is_finite()) || radius <= 0. || pitch.abs() > 1.5 {
                return Err("finite camera required; radius > 0 and pitch within ±1.5 radians".into());
            }
            // The shared orbit, set absolutely: scripted motion stops, a pending fit is dropped, turntable.
            orbit.interrupt();
            (orbit.focus, orbit.radius, orbit.yaw, orbit.pitch) = (Vec3::from_array(*focus), radius, yaw, pitch);
            orbit.home = false;
            orbit.trackball = None;
            Ok(None)
        }
        control => dispatch(view, orbit, selection, registry, control.clone()).map(|()| None),
    }
}

/// `robot_open`: `path` (a `*.simrobot.json`) replaces the robot in this
/// window, as `robot_preset` replaces it with a preset; refused while a
/// recording is written or a replay runs (the mode switch's blockers).
pub(crate) fn open_file(view: &mut RobotView, selection: &mut Selection, registry: &mut DocumentRegistry, path: &std::path::Path) -> Result<(), String> {
    if !path.to_string_lossy().ends_with(".simrobot.json") {
        return Err(format!("{}: robot mode opens a *.simrobot.json file", path.display()));
    }
    if !path.is_file() {
        return Err(format!("{}: no such file", path.display()));
    }
    let blockers = view.switch_blockers();
    if !blockers.is_empty() {
        return Err(format!("not opening {}: {}", path.display(), blockers.join("; ")));
    }
    let mut next = RobotView::open(path.to_path_buf()).with_presets(view.presets.clone().ok());
    next.ui_revision = view.ui_revision + 1;
    next.picks = view.picks.clone();
    next.display_hz = view.display_hz;
    crate::jobs::drop_off_thread(std::mem::replace(view, next), "the robot view");
    picked::opened_file(selection, registry, path);
    Ok(())
}

/// Actions: robot mode's one apply system. A click's or key's refusal is
/// the header's run message; a REST caller gets it (or `robot_state`).
/// `system_ui` also lists the Leg calibration panel's controls
/// (`hardware:<name>`, from `hardware::panel::controls`) after robot mode's
/// own. Activating one is checked here once, on its first application
/// (the control exists, is enabled, passes `HardwareAction::authorize`, and
/// no window close is pending for motion), then written as an
/// `Act<HardwareAction>` with its own REST reply (`Replies::submit`): the
/// panel's one apply system (`hardware::actions::apply`, after this one in
/// the same frame) handles it as a REST call, so a remote calibration
/// command is answered only by the link thread's verdict, and this call
/// answers with that reply. Their ids are stable names, so they need no
/// `ui_revision`. Link selection goes through the shared selection
/// (`picked`), applied here as its adapter. Robot mode's one writer of
/// `crate::drive_input::Disarm` (`DISARM_RULE`): every drive stop or named
/// action, Pause and Reset accepted here, from any origin ([`disarm_reason`])
/// but the devices themselves, disarms the held drive inputs from the next
/// frame's poll.
///
/// The one device poller's requests for Robot mode (`crate::drive_input`)
/// are read here too, after this frame's other actions (where the old
/// forwarding system's `RobotAction::Drive` messages were queued), and
/// handled by the same handler as the Drive block's buttons, `system_ui`
/// drive:* and REST `robot_drive` (`drive_request` → `RunController::drive`,
/// a refusal in `DriveInput::last_error`), but with no `Disarm`: the poller
/// disarmed what was held when it sent a stop or action itself, and an echo
/// read in the next frame would block a key first pressed after it (one the
/// poller let drive in the frame it sent a Stop it owed).
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    mut messages: ResMut<Messages<Act<RobotAction>>>,
    mut in_flight: ResMut<InFlight<RobotAction>>,
    mut replies: ResMut<Replies>,
    view: Option<ResMut<RobotView>>,
    orbit: Option<Single<&mut Orbit, With<RobotCamera>>>,
    hardware: Option<Res<super::hardware::Hardware>>,
    mut to_hardware: MessageWriter<Act<super::hardware::HardwareAction>>,
    mut selection: ResMut<Selection>,
    mut registry: ResMut<DocumentRegistry>,
    closing: Option<Res<crate::app::close::CloseOwner>>,
    (mut threads, reveal, mut window): (ResMut<crate::robot::threads::RobotThreads>, Option<Res<crate::cad::threads::RevealThread>>, MessageWriter<Act<crate::app::switch::WindowAction>>),
    (mut drive_input, bindings, mut disarm, mut devices): (Option<ResMut<crate::drive_input::DriveInput>>, Option<Res<crate::drive_input::DriveBindings>>, MessageWriter<crate::drive_input::Disarm>, MessageReader<Act<crate::drive_input::DriveDevice>>),
    (mut board, mut video): (ResMut<crate::robot::leaderboard::Leaderboard>, ResMut<crate::robot::video::VideoRecorder>),
) {
    // Read every frame this runs, before any return: a device request is for this frame only.
    let from_devices: Vec<Act<RobotAction>> = devices.read().filter_map(device_action).collect();
    let (Some(mut view), Some(mut orbit)) = (view, orbit) else {
        actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
            // A forwarded hardware activation's own reply is no longer waited for.
            if is_hardware_activation(action) {
                forget_forwarded(call);
            }
            Outcome::Done(Err("the robot view is not open".into()))
        });
        return;
    };
    // Whether the call being handled is a device request (no `Disarm` echo).
    let device = std::cell::Cell::new(false);
    let mut on_action = handler(|action, call| {
        // Record video: the recorder's own state.
        if let RobotAction::Video { on } = action {
            let want = on.unwrap_or(!video.recording());
            let result = match (want, video.recording()) {
                (true, false) => crate::robot::video::target(&view).and_then(|p| video.start(p)),
                (false, true) => video.stop(),
                (true, true) => Err("already recording video".to_string()),
                (false, false) => Err("not recording video".to_string()),
            };
            if let (Err(e), Origin::Ui) = (&result, call.origin) {
                view.run_message = Some(e.clone());
            }
            return match call.origin {
                Origin::Rest(_) => Outcome::Done(result.map(|()| video.json())),
                _ => Outcome::Done(Ok(Value::Null)),
            };
        }
        // The leaderboard: its own state; a run or replay opens a tested recipe in place of this robot.
        if let RobotAction::Leaderboard { op } = action {
            let result = crate::robot::leaderboard::handle(op, &mut board, &view).map(|(answer, next)| {
                if let Some(mut next) = next {
                    next.ui_revision = view.ui_revision + 1;
                    next.picks = view.picks.clone();
                    next.display_hz = view.display_hz;
                    let id = next.preset.as_ref().map(|p| p.id.clone()).unwrap_or_default();
                    crate::jobs::drop_off_thread(std::mem::replace(&mut *view, next), "the robot view");
                    picked::opened_preset(&mut selection, &mut registry, &id);
                }
                answer
            });
            if let Err(e) = &result {
                board.notice = Some(Err(e.clone()));
                board.revision += 1;
            }
            return match call.origin {
                Origin::Rest(_) => Outcome::Done(result),
                _ => Outcome::Done(Ok(Value::Null)),
            };
        }
        // RoboCAD's comment threads: their own handler, outcome and REST wait.
        if let RobotAction::Threads { act } = action {
            return crate::robot::threads::handle(act, call, &mut threads, &view, &registry, &mut selection, reveal.is_some(), &mut window);
        }
        let synced = hardware.as_ref().is_some_and(|hw| hw.sync.engaged());
        // Resolved before the action runs (a `system_ui` activation by the control list it was made from).
        let disarms = disarm_reason(&view, picked::link(&selection, &registry), action, call.origin);
        let result = match action {
            _ if synced && call.remote() && moves_synced_motors(&view, picked::link(&selection, &registry), action) => Err(SYNC_REMOTE_REFUSAL.to_string()),
            RobotAction::Activate { id, .. } if id.starts_with("hardware:") => {
                // Runs only on the first application (`submit` takes the reply after that).
                let eligible = || -> Result<super::hardware::HardwareAction, String> {
                    let hw = hardware.as_deref().ok_or("the Leg calibration panel is not open in this mode")?;
                    let (id, _, action, ready) = super::hardware::panel::controls(hw).into_iter().find(|(i, ..)| i == id).ok_or("unknown control; request controls")?;
                    ready.map_err(|why| format!("{id} is disabled: {why}"))?;
                    action.authorize(hw, std::time::Instant::now())?;
                    if action.starts_motion() && !matches!(action, super::hardware::HardwareAction::JogRelease { .. }) && closing.as_ref().is_some_and(|close| close.pending()) {
                        return Err("Window closure is pending; cancel close before starting, changing or arming motion".into());
                    }
                    Ok(action)
                };
                if call.rest() {
                    if call.cancelled {
                        // Answered already: that is the outcome. Else the
                        // hardware reply is forgotten: its handler takes a
                        // forgotten reply as cancelled (STOP for a queued
                        // command) and nothing is left waiting on it.
                        if let Some(inner) = forwarded(call) {
                            if let Some(outcome) = call.replies.take(inner) {
                                return outcome;
                            }
                            call.replies.forget(inner);
                            return Outcome::Done(Err("hardware action cancelled; STOP requested".into()));
                        }
                        return Outcome::Done(Err("hardware action cancelled before it was applied".into()));
                    }
                    return call.replies.submit(call.continuation, call.cancelled, eligible, |act| {
                        to_hardware.write(act);
                        true
                    });
                }
                // Not REST, so nobody waits for a reply: passed on one-way (the
                // hardware handler shows its refusal as the panel's notice).
                eligible().map(|action| {
                    to_hardware.write(Act { action, origin: Origin::SystemUi });
                    None
                })
            }
            RobotAction::Controls => handle(&mut view, &mut orbit, &mut selection, &mut registry, action).map(|answer| {
                answer.map(|mut listing| {
                    if let (Some(hw), Some(items)) = (hardware.as_ref(), listing.get_mut("controls").and_then(Value::as_array_mut)) {
                        for (id, label, action, ready) in super::hardware::panel::controls(hw) {
                            let reason = action.authorize(hw, std::time::Instant::now()).err().or_else(|| ready.err());
                            items.push(json!({"id": id, "label": label, "enabled": reason.is_none(), "disabled_reason": reason, "action": {"hardware": action}}));
                        }
                    }
                    listing
                })
            }),
            _ => handle(&mut view, &mut orbit, &mut selection, &mut registry, action),
        };
        // DISARM_RULE: an accepted stop, halt, action, Pause or Reset (any origin but the
        // devices, which disarmed themselves) disarms held drive inputs; the poller reads it
        // next frame (`drive_input::input::devices`).
        if let (Ok(_), Some((reason, stop)), false) = (&result, disarms, device.get()) {
            disarm.write(crate::drive_input::Disarm { mode: ViewerMode::Robot, reason, stop });
        }
        // The inspector's drive line: the last drive request's refusal (any
        // origin, a `system_ui` drive:* activation included: every drive:* id
        // is a `RobotAction::Drive`), cleared by an accepted one.
        let drive = match action {
            RobotAction::Drive { .. } => true,
            RobotAction::Activate { id, .. } => id.starts_with("drive:"),
            _ => false,
        };
        if let (true, Some(d)) = (drive, drive_input.as_mut()) {
            let error = result.as_ref().err().cloned();
            if d.last_error != error {
                d.last_error = error;
            }
        }
        match call.origin {
            Origin::Rest(_) => Outcome::Done(result.map(|answer| {
                // `robot_state.cad_threads`, and the device layer's `bindings` and
                // `drive_input` (`with_drive_input`): every answer that carries the state carries them.
                let cad_threads = || crate::robot::threads::state_json(&view, &threads);
                let devices = |state: Value| view.with_drive_input(state, bindings.as_deref(), drive_input.as_deref());
                match (answer, action) {
                    (None, _) => devices(view.state_with_threads(picked::link(&selection, &registry), cad_threads())),
                    (Some(mut answer), RobotAction::State) => {
                        answer["robot_state"]["cad_threads"] = cad_threads();
                        answer["robot_state"] = devices(answer["robot_state"].take());
                        answer
                    }
                    (Some(mut answer), RobotAction::Controls) => {
                        answer["state"]["cad_threads"] = cad_threads();
                        answer["state"] = devices(answer["state"].take());
                        answer
                    }
                    (Some(answer), _) => answer,
                }
            })),
            Origin::Ui => {
                view.run_message = result.err();
                Outcome::Done(Ok(Value::Null))
            }
            Origin::Quiet | Origin::SystemUi => Outcome::Done(Ok(Value::Null)),
        }
    });
    actions::apply(&mut messages, &mut in_flight, &mut replies, &mut on_action);
    // The devices' requests, after the frame's other actions; never REST, so nothing waits on them.
    device.set(true);
    for act in from_devices {
        let mut continuation = Value::Null;
        let _ = on_action(&act.action, &mut actions::Call { origin: act.origin, continuation: &mut continuation, cancelled: false, replies: &mut replies });
    }
}

/// [`apply`]'s handler, typed as `actions::apply` takes it (a closure bound
/// to a local needs the signature to take any `Call` lifetime).
fn handler<F: FnMut(&RobotAction, &mut actions::Call) -> Outcome>(f: F) -> F {
    f
}

/// Why `action`, once accepted, disarms held drive inputs
/// (`crate::drive_input::DISARM_RULE`), or None: a drive stop or named
/// action, Pause or Reset, directly or as the `system_ui` control that
/// carries it (resolved in `controls(view, link)`, the table `Activate`
/// resolves ids in). The reason names the origin for automation.
fn disarm_reason(view: &RobotView, link: Option<usize>, action: &RobotAction, origin: Origin) -> Option<(String, bool)> {
    let resolved = match action {
        RobotAction::Activate { id, .. } => controls(view, link).into_iter().find(|(i, ..)| i == id).map(|(.., a)| a),
        _ => None,
    };
    // (what, send a Stop first: drive stops and actions only; `Disarm::stop`).
    let (what, stop) = match resolved.as_ref().unwrap_or(action) {
        RobotAction::Drive { request: DriveRequest::Stop } => ("Stop".to_string(), true),
        RobotAction::Drive { request: DriveRequest::Action { name } } => (format!("action {name}"), true),
        RobotAction::Run { action: RunAction::Pause } => ("Pause".to_string(), false),
        RobotAction::Run { action: RunAction::Reset } => ("Reset".to_string(), false),
        _ => return None,
    };
    let reason = match (resolved.is_some(), origin) {
        (true, _) => format!("{what} from system_ui"),
        (false, Origin::Rest(_)) => format!("{what} from REST"),
        (false, Origin::SystemUi) => format!("{what} from system_ui"),
        (false, Origin::Ui | Origin::Quiet) => what,
    };
    Some((reason, stop))
}

/// A device request as robot mode's action: `RobotAction::Drive` with the
/// poller's origin (`Act::ui` for a shown stop or action, `Act::quiet` for
/// repeated axes), or None for a request the poller made for another mode.
pub(super) fn device_action(device: &Act<crate::drive_input::DriveDevice>) -> Option<Act<RobotAction>> {
    (device.action.mode == ViewerMode::Robot).then(|| Act { action: RobotAction::Drive { request: device.action.request.clone() }, origin: device.origin })
}

/// A `system_ui` activation of a Leg calibration control (`hardware:<name>`).
fn is_hardware_activation(action: &RobotAction) -> bool {
    matches!(action, RobotAction::Activate { id, .. } if id.starts_with("hardware:"))
}

/// The reply a forwarded hardware activation was written with
/// (`Replies::submit` keeps it in the continuation under "reply").
fn forwarded(call: &actions::Call) -> Option<actions::Reply> {
    call.continuation.get("reply").and_then(Value::as_u64).map(actions::Reply::from_id)
}

/// Nobody will take the forwarded reply: close its slot.
fn forget_forwarded(call: &mut actions::Call) {
    if let Some(inner) = forwarded(call) {
        call.replies.forget(inner);
    }
}

/// Robot mode's exit (`InFlight::abandon_with`): a carried hardware
/// activation's forwarded reply is closed, as nobody will take it.
pub(super) fn forget_forwarded_on_exit(action: &RobotAction, continuation: &Value, replies: &mut Replies) {
    if is_hardware_activation(action)
        && let Some(inner) = continuation.get("reply").and_then(Value::as_u64)
    {
        replies.forget(actions::Reply::from_id(inner));
    }
}

/// Why REST and `system_ui` may not steer the run while live motor sync
/// streams its targets to the bench.
const SYNC_REMOTE_REFUSAL: &str = "live motor sync is streaming this run's targets to real motors: REST and system_ui may not start, step, jog, drive or change the speed of the run until sync stops (Pause, Reset and STOP stay available)";

/// Whether `action` (or the control it activates) would move the targets
/// live motor sync streams to the bench: motion requests, jogs, Start and
/// Step, and the run speed. Pause, Reset, replay and gait preview are not:
/// each ends the sync session (`hardware::sync`'s stop rules).
fn moves_synced_motors(view: &RobotView, link: Option<usize>, action: &RobotAction) -> bool {
    match action {
        RobotAction::Motion { .. } | RobotAction::Inputs { .. } | RobotAction::Jog { .. } | RobotAction::JogTo { .. } | RobotAction::Speed { .. } => true,
        // Driving moves the run; a stop or a profile action (stop | halt) does not.
        RobotAction::Drive { request } => matches!(request, DriveRequest::Axes { .. }),
        RobotAction::Run { action } => matches!(action, RunAction::Start | RunAction::Step),
        RobotAction::Activate { id, .. } => controls(view, link).into_iter().find(|(i, _, _)| i == id).is_some_and(|(_, _, a)| !matches!(a, RobotAction::Activate { .. }) && moves_synced_motors(view, link, &a)),
        _ => false,
    }
}

mod checks;
mod commands;
mod keys;
mod listing;
mod rest_form;
#[cfg(test)]
mod tests;

use checks::*;
pub(crate) use rest_form::wire;
pub(super) use checks::{check, check_stress, overlay_toggle};
pub(super) use listing::{controls, input_controls};
pub(super) use rest_form::publish;

pub(super) use keys::{buttons, graph_key, motion_keys, overlay_keys, pick_link, planar_keys, speed_keys};
