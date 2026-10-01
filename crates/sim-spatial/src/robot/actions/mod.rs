//! Robot mode's actions: [`RobotAction`] (one enum for the buttons, keys,
//! `system_ui` controls and REST commands), its REST form, its commands in
//! the action registry, the input systems that write it and [`apply`], its
//! one handler (`ViewerSet::Actions`).
use super::*;
use crate::app::actions::{self, Act, InFlight, Origin, Replies};
use crate::robot::run::SPEED_SCALES;
use bevy::ecs::message::Messages;
use sim_api::Outcome;

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
    /// REST `robot_preset`: open a declared preset in this window.
    OpenPreset { id: String },
    /// REST `system_ui` controls: the control list with each control's action.
    Controls,
    /// REST `system_ui` activate: the listed control's action, through this handler.
    Activate { id: String, ui_revision: u64 },
    /// REST `camera`: an absolute orbit.
    Camera { focus: [f32; 3], radius: f32, yaw: f32, pitch: f32 },
}

/// The action that flips one overlay from its current requested value.
pub(super) fn overlay_toggle(view: &RobotView, kind: &str) -> RobotAction {
    let flip = Some(!overlay_on(view, kind));
    match kind {
        "contacts" => RobotAction::Overlay { contacts: flip, joints: None, deflections: None, stress: None },
        "joints" => RobotAction::Overlay { contacts: None, joints: flip, deflections: None, stress: None },
        "stress" => RobotAction::Overlay { contacts: None, joints: None, deflections: None, stress: flip },
        _ => RobotAction::Overlay { contacts: None, joints: None, deflections: flip, stress: None },
    }
}
/// Why the stress overlay cannot be set now.
pub(super) fn check_stress(view: &RobotView) -> Result<(), String> {
    if view.planar.is_some() {
        return Err(planar::STRESS.into());
    }
    view.source.as_ref().ok_or(STRESS_PRESET)?;
    view.model.as_ref().ok_or("the robot has not loaded")?;
    Ok(())
}
/// The absolute target a jog action asks for (file validation happens in `check_jog`).
fn jog_target(run: &RunController, joint: &str, delta: f64) -> Result<f64, String> {
    if let Some(r) = run.recorded() {
        // Named before the joint lookup, as check_jog would.
        return Err(r.refusal(&format!("servo-target jog of `{joint}`")));
    }
    let servo = crate::robot::run::servo(run.model(), joint)?;
    Ok(run.requested_target(&servo) + delta)
}
/// Why a control is unavailable now for a planar (v2) file: its run, joint
/// targets, speed, contacts and reload; every action without a v2 meaning is
/// refused naming it (`robot_planar`'s refusals).
fn check_planar(view: &RobotView, p: &PlanarView, action: &RobotAction) -> Result<(), String> {
    match action {
        RobotAction::Run { action } => {
            if *action == RunAction::Start && view.mirror.is_some() {
                return Err(super::hardware::mirror::MIRRORING.into());
            }
            p.run.check(*action)
        }
        RobotAction::Jog { joint, delta } => p.run.check_joint(joint, *delta).map(|_| ()),
        RobotAction::JogTo { joint, target } => p.run.check_joint(joint, *target).map(|_| ()),
        RobotAction::SelectJoint { index } => match p.joint_names().len() {
            n if *index < n => Ok(()),
            n => Err(format!("joint index {index} is out of range: this planar v2 file simulates {n} joint(s)")),
        },
        RobotAction::Speed { speed } => p.run.check_speed(*speed).map(|_| ()),
        RobotAction::Reload { .. } => view.source.as_ref().ok_or("a preset is not reloaded; reload is for --robot FILE")?.check_reload(),
        RobotAction::Overlay { joints, deflections, stress, .. } => {
            // Switching an overlay on that a planar file cannot draw is refused by name; off is a no-op.
            if *joints == Some(true) {
                return Err(planar::JOINT_FRAMES.into());
            }
            if *deflections == Some(true) {
                return Err(planar::DEFLECTIONS.into());
            }
            if *stress == Some(true) {
                return Err(planar::STRESS.into());
            }
            Ok(())
        }
        RobotAction::Motion { .. } => Err(planar::MOTION.into()),
        RobotAction::SaveRecording { .. } => Err(planar::SAVE_RECORDING.into()),
        RobotAction::Replay { .. } | RobotAction::CancelReplay | RobotAction::RefreshRecordings => Err(planar::REPLAY.into()),
        RobotAction::Gait { .. } => Err(planar::GAIT.into()),
        RobotAction::Recorded { .. } => Err(planar::RECORDED.into()),
        RobotAction::ToggleGraphs => Err(planar::GRAPHS.into()),
        _ => Ok(()),
    }
}
/// Why a control is unavailable now (`Ok` when enabled).
pub(super) fn check(view: &RobotView, action: &RobotAction) -> Result<(), String> {
    if let Some(p) = &view.planar {
        return check_planar(view, p, action);
    }
    match action {
        RobotAction::SelectJoint { .. } => Err("joint selection (←/→) is for a planar v2 file; select a link to jog its joints".into()),
        RobotAction::Run { action } => {
            if *action == RunAction::Start && view.mirror.is_some() {
                return Err(super::hardware::mirror::MIRRORING.into());
            }
            view.run.as_ref().ok_or("the robot has not loaded")?.check(*action)
        }
        RobotAction::Jog { joint, delta } => {
            let run = view.run.as_ref().ok_or("the robot has not loaded")?;
            run.check_jog(joint, jog_target(run, joint, *delta)?).map(|_| ())
        }
        RobotAction::JogTo { joint, target } => view.run.as_ref().ok_or("the robot has not loaded")?.check_jog(joint, *target).map(|_| ()),
        RobotAction::Motion { request } => view.run.as_ref().ok_or("the robot has not loaded")?.check_motion_request(request),
        RobotAction::SaveRecording { .. } => view.run.as_ref().ok_or("the robot has not loaded")?.check_save().map(|_| ()),
        RobotAction::Replay { .. } => view.run.as_ref().ok_or("the robot has not loaded")?.check_replay().map(|_| ()),
        RobotAction::CancelReplay => view.run.as_ref().ok_or("the robot has not loaded")?.check_cancel(),
        RobotAction::Gait { action } => view.run.as_ref().ok_or("the robot has not loaded")?.check_gait(action),
        RobotAction::Speed { speed } => view.run.as_ref().ok_or("the robot has not loaded")?.check_speed(*speed).map(|_| ()),
        RobotAction::Recorded { action } => view.run.as_ref().ok_or("the robot has not loaded")?.check_recorded(action),
        RobotAction::Reload { .. } => view.source.as_ref().ok_or("a preset is not reloaded; reload is for --robot FILE")?.check_reload(),
        RobotAction::Overlay { contacts, joints, deflections, stress } => {
            if contacts.is_some() || joints.is_some() || deflections.is_some() {
                view.run.as_ref().ok_or("the robot has not loaded")?.check_overlays()?;
            }
            if stress.is_some() {
                check_stress(view)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
/// The planar view, for a handler that `check_planar` already accepted.
fn planar(view: &mut RobotView) -> Result<&mut PlanarView, String> {
    view.planar.as_mut().ok_or_else(|| "the planar v2 file is no longer loaded".to_string())
}
/// A control action on a planar (v2) file: validated by [`check`], then applied
/// to the planar run (Reset rebuilds from the loaded model; Reload re-reads the file).
fn dispatch_planar(view: &mut RobotView, orbit: &mut Orbit, action: RobotAction) -> Result<(), String> {
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
        RobotAction::SelectLink { index, .. } => {
            view.selected = Some(index);
            view.scroll_to = Some(0.0);
        }
        RobotAction::ClearSelection => view.selected = None,
        RobotAction::ShowSection { section } => {
            view.section = section;
            view.scroll_to = Some(0.0);
        }
        RobotAction::ScrollInspector { delta } => view.scroll_to = Some((view.scroll + delta).clamp(0.0, view.scroll_max)),
        RobotAction::Fit => {
            // Frame the latest outlines again (front view), as an open does.
            planar(view)?.frame_camera = Some(true);
            orbit.home = true;
        }
        // Refused by `check_planar` (named); listed so a new action is not silently accepted.
        RobotAction::Motion { .. } | RobotAction::SaveRecording { .. } | RobotAction::Replay { .. } | RobotAction::CancelReplay | RobotAction::RefreshRecordings
        | RobotAction::Gait { .. } | RobotAction::Recorded { .. } | RobotAction::ToggleGraphs => return Err("refused for a planar v2 file".into()),
        RobotAction::State | RobotAction::RobotState | RobotAction::Presets | RobotAction::OpenPreset { .. } | RobotAction::Controls | RobotAction::Activate { .. } | RobotAction::Camera { .. } => unreachable!("answered by `handle`"),
    }
    Ok(())
}
/// A control action: validated by [`check`] (motion and save validate in
/// their own handler), then applied.
fn dispatch(view: &mut RobotView, orbit: &mut Orbit, action: RobotAction) -> Result<(), String> {
    if view.planar.is_some() {
        return dispatch_planar(view, orbit, action);
    }
    if let RobotAction::Motion { request } = action {
        // Validated (and a refusal recorded) inside the one motion handler.
        return view.run.as_mut().ok_or("the robot has not loaded")?.motion(request);
    }
    if let RobotAction::SaveRecording { path, note } = action {
        // Validated (and a refusal recorded) inside the one save handler.
        return view.run.as_mut().ok_or("the robot has not loaded")?.save_recording(path.as_deref(), note.as_deref()).map(|_| ());
    }
    check(view, &action)?;
    match action {
        RobotAction::Motion { .. } | RobotAction::SaveRecording { .. } => unreachable!("handled above"),
        RobotAction::State | RobotAction::RobotState | RobotAction::Presets | RobotAction::OpenPreset { .. } | RobotAction::Controls | RobotAction::Activate { .. } | RobotAction::Camera { .. } => unreachable!("answered by `handle`"),
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
        RobotAction::SelectLink { index, .. } => {
            view.selected = Some(index);
            view.scroll_to = Some(0.0);
        }
        RobotAction::ClearSelection => view.selected = None,
        RobotAction::ShowSection { section } => {
            view.section = section;
            view.scroll_to = Some(0.0);
        }
        RobotAction::ScrollInspector { delta } => view.scroll_to = Some((view.scroll + delta).clamp(0.0, view.scroll_max)),
        // The bounds at 3.2 × extent from the current heading (`camera::place`).
        RobotAction::Fit => orbit.home = true,
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
/// The `system_ui` controls: (id, label, action), the table `Activate` resolves ids in.
pub(super) fn controls(view: &RobotView) -> Vec<(String, String, RobotAction)> {
    if let Some(p) = &view.planar {
        return planar_controls(view, p);
    }
    let mut out: Vec<(String, String, RobotAction)> = view
        .model
        .iter()
        .flat_map(|m| m.links.iter().enumerate())
        .map(|(index, l)| (format!("link:{index}"), l.name.clone(), RobotAction::SelectLink { index, name: l.name.clone() }))
        .collect();
    if view.panels_ready {
        out.push(("clear_selection".into(), "Clear selection".into(), RobotAction::ClearSelection));
        for section in Section::ALL {
            out.push((format!("section:{}", section.label().to_lowercase()), section.label().into(), RobotAction::ShowSection { section }));
        }
        out.push(("inspector:scroll_down".into(), "Scroll inspector down".into(), RobotAction::ScrollInspector { delta: 400.0 }));
        out.push(("inspector:scroll_up".into(), "Scroll inspector up".into(), RobotAction::ScrollInspector { delta: -400.0 }));
        out.push(("fit".into(), "Fit".into(), RobotAction::Fit));
        if view.source.is_some() {
            out.push(("robot:reload".into(), "Reload file".into(), RobotAction::Reload { trigger: ReloadTrigger::Manual }));
        }
        out.push(("graphs:toggle".into(), (if view.graphs_visible { "Hide graphs (G)" } else { "Show graphs (G)" }).into(), RobotAction::ToggleGraphs));
        for (kind, name, key) in OVERLAYS {
            let key = format!("{key:?}").trim_start_matches("Key").to_string();
            out.push((format!("overlay:{kind}"), format!("{} {name} overlay ({key})", if overlay_on(view, kind) { "Hide" } else { "Show" }), overlay_toggle(view, kind)));
        }
        for action in RunAction::ALL {
            out.push((format!("run:{}", action.name()), action.label().into(), RobotAction::Run { action }));
        }
        out.push(("run:speed_down".into(), "Run speed down (−)".into(), RobotAction::Speed { speed: SpeedRequest::Down }));
        out.push(("run:speed_up".into(), "Run speed up (=/+)".into(), RobotAction::Speed { speed: SpeedRequest::Up }));
        for scale in SPEED_SCALES {
            out.push((format!("run:speed:{scale}"), format!("Run speed ×{scale}"), RobotAction::Speed { speed: SpeedRequest::Set { scale } }));
        }
        if view.run.as_ref().is_some_and(|r| r.playback().is_some()) {
            for (id, label, action) in recorded_controls() {
                out.push((id, label, RobotAction::Recorded { action }));
            }
        }
        for (joint, step) in jog_joints(view) {
            let unit = if step == JOG_STEP_M { "m" } else { "rad" };
            out.push((format!("jog:{joint}:-"), format!("Jog {joint} servo target −{step} {unit}"), RobotAction::Jog { joint: joint.clone(), delta: -step }));
            out.push((format!("jog:{joint}:+"), format!("Jog {joint} servo target +{step} {unit}"), RobotAction::Jog { joint, delta: step }));
        }
        if view.preset.is_some() {
            for (id, label, request) in motion_buttons() {
                out.push((id.into(), label.into(), RobotAction::Motion { request }));
            }
            out.push(("recording:save".into(), "Save recording".into(), RobotAction::SaveRecording { path: None, note: None }));
            if let Some(r) = view.run.as_ref() {
                for l in r.recordings() {
                    out.push((format!("replay:{}", l.file), format!("Replay {}", l.file), RobotAction::Replay { file: Some(l.file.clone()), path: None }));
                }
            }
            out.push(("replay:cancel".into(), "Cancel replay".into(), RobotAction::CancelReplay));
            out.push(("replay:refresh".into(), "Refresh recordings".into(), RobotAction::RefreshRecordings));
            for (id, label, action) in gait_controls(view) {
                out.push((id, label, RobotAction::Gait { action }));
            }
        }
    }
    out
}
/// The `system_ui` controls of a planar (v2) file: bodies, sections, view,
/// reload, run, speed, joint selection and target moves (ids by built joint
/// index), the contacts overlay, and the controls it shows but cannot use
/// (graphs, joint frames, deflections, stress), listed disabled with the reason.
fn planar_controls(view: &RobotView, p: &PlanarView) -> Vec<(String, String, RobotAction)> {
    let mut out: Vec<(String, String, RobotAction)> =
        p.loaded.model.bodies.iter().enumerate().map(|(index, b)| (format!("link:{index}"), b.name.clone(), RobotAction::SelectLink { index, name: b.name.clone() })).collect();
    if !view.panels_ready {
        return out;
    }
    out.push(("clear_selection".into(), "Clear selection".into(), RobotAction::ClearSelection));
    for section in Section::ALL {
        out.push((format!("section:{}", section.label().to_lowercase()), section.label().into(), RobotAction::ShowSection { section }));
    }
    out.push(("inspector:scroll_down".into(), "Scroll inspector down".into(), RobotAction::ScrollInspector { delta: 400.0 }));
    out.push(("inspector:scroll_up".into(), "Scroll inspector up".into(), RobotAction::ScrollInspector { delta: -400.0 }));
    out.push(("fit".into(), "Fit".into(), RobotAction::Fit));
    if view.source.is_some() {
        out.push(("robot:reload".into(), "Reload file".into(), RobotAction::Reload { trigger: ReloadTrigger::Manual }));
    }
    out.push(("graphs:toggle".into(), "Show graphs (G)".into(), RobotAction::ToggleGraphs));
    for (kind, name, key) in OVERLAYS {
        let key = format!("{key:?}").trim_start_matches("Key").to_string();
        out.push((format!("overlay:{kind}"), format!("{} {name} overlay ({key})", if overlay_on(view, kind) { "Hide" } else { "Show" }), overlay_toggle(view, kind)));
    }
    for action in RunAction::ALL {
        out.push((format!("run:{}", action.name()), action.label().into(), RobotAction::Run { action }));
    }
    out.push(("run:speed_down".into(), "Run speed down (−)".into(), RobotAction::Speed { speed: SpeedRequest::Down }));
    out.push(("run:speed_up".into(), "Run speed up (=/+)".into(), RobotAction::Speed { speed: SpeedRequest::Up }));
    for scale in SPEED_SCALES {
        out.push((format!("run:speed:{scale}"), format!("Run speed ×{scale}"), RobotAction::Speed { speed: SpeedRequest::Set { scale } }));
    }
    for (index, joint) in p.joint_names().iter().enumerate() {
        out.push((format!("joint:{index}"), format!("Select joint {joint} (←/→)"), RobotAction::SelectJoint { index }));
        out.push((format!("jog:{index}:-"), format!("Move {joint} target −{JOG_STEP_RAD} rad"), RobotAction::Jog { joint: joint.clone(), delta: -JOG_STEP_RAD }));
        out.push((format!("jog:{index}:+"), format!("Move {joint} target +{JOG_STEP_RAD} rad"), RobotAction::Jog { joint: joint.clone(), delta: JOG_STEP_RAD }));
    }
    out
}
/// The gait preview controls (system_ui id, label, action), all through `RobotAction::Gait`
/// as the inspector's Gait preview buttons and REST `robot_gait`: one open per offered
/// tracked report, then the transport. Seek steps are relative to the latest pose's gait
/// time, resolved when listed or clicked.
fn gait_controls(view: &RobotView) -> Vec<(String, String, GaitAction)> {
    let Some(g) = view.run.as_ref().and_then(|r| r.gait_preview()) else { return Vec::new() };
    let mut out: Vec<(String, String, GaitAction)> = g.reports().iter().map(|r| (format!("gait:open:{}", r.name), format!("Open gait {}", r.name), GaitAction::Open { source: GaitSource::Report(r.name.clone()) })).collect();
    out.push(("gait:play".into(), "Play gait preview".into(), GaitAction::Play));
    out.push(("gait:pause".into(), "Pause gait preview".into(), GaitAction::Pause));
    out.push(("gait:stop".into(), "Stop gait preview (live frame again)".into(), GaitAction::Stop));
    for (step, id, label) in GAIT_SEEK {
        out.push((format!("gait:seek:{id}"), label.into(), gait_seek(view, step)));
    }
    for scale in GAIT_SCALES {
        out.push((format!("gait:speed:{scale}"), format!("Gait speed ×{scale}"), GaitAction::Speed { scale }));
    }
    out.push(("gait:list".into(), "List gait reports again".into(), GaitAction::List));
    out
}
/// The recorded timeline controls (system_ui id, label, action), all through
/// `RobotAction::Recorded` as the inspector Recorded buttons and REST `robot_recorded`.
fn recorded_controls() -> Vec<(String, String, RecordedAction)> {
    let mut out: Vec<(String, String, RecordedAction)> = RECORDED_TRANSPORT.iter().map(|(id, label, a)| (format!("recorded:{id}"), label.to_string(), *a)).collect();
    for scale in SPEED_SCALES {
        out.push((format!("recorded:speed:{scale}"), format!("Recorded playback speed ×{scale}"), RecordedAction::Speed { scale }));
    }
    out
}

/// The one handler of robot mode's actions: REST reads and the view's own
/// requests here, control actions through `dispatch` (validated by
/// `check`). `Ok(None)`: the answer is `robot_state`.
fn handle(view: &mut RobotView, orbit: &mut Orbit, action: &RobotAction) -> Result<Option<Value>, String> {
    match action {
        RobotAction::State => Ok(Some(json!({"robot_state": view.state_json()}))),
        RobotAction::RobotState => Ok(None),
        RobotAction::Presets => {
            let (file, root) = (view.presets.clone()?, view.root.clone()?);
            let presets = crate::robot::preset::list(&file)?;
            let rows: Vec<Value> = presets.iter().map(|p| p.discovery(&root)).collect();
            Ok(Some(json!({"presets_file": file, "root": root, "workspace": crate::workspace::json(), "count": rows.len(), "presets": rows,
                "current": view.preset.as_ref().map(|p| &p.id)})))
        }
        RobotAction::OpenPreset { id } => {
            // Refused here (naming the id) before anything is replaced; the old run thread stops when its controller drops.
            let mut next = RobotView::open_preset(&view.presets.clone()?, id)?;
            next.ui_revision = view.ui_revision + 1;
            // The old view's run and playback threads are joined off the UI thread, as leave_robot does.
            crate::jobs::drop_off_thread(std::mem::replace(view, next), "the robot view");
            Ok(None)
        }
        RobotAction::Controls => {
            let items: Vec<Value> = controls(view).into_iter().map(|(id, label, action)| json!({"id": id, "label": label, "enabled": check(view, &action).is_ok(), "disabled_reason": check(view, &action).err(), "action": action})).collect();
            Ok(Some(json!({"ui_revision": view.ui_revision, "ready": view.panels_ready, "controls": items, "state": view.state_json()})))
        }
        RobotAction::Activate { id, ui_revision } => {
            if !view.panels_ready || *ui_revision != view.ui_revision {
                return Err("UI changed; request controls again before activating".into());
            }
            let (_, _, action) = controls(view).into_iter().find(|(i, _, _)| i == id).ok_or("unknown control; request controls")?;
            dispatch(view, orbit, action).map(|()| None)
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
        control => dispatch(view, orbit, control.clone()).map(|()| None),
    }
}

/// Actions: robot mode's one apply system. A click's or key's refusal is
/// the header's run message; a REST caller gets it (or `robot_state`).
/// `system_ui` also lists the Leg calibration panel's controls
/// (`hardware:<name>`, from `hardware::panel::controls`) after robot mode's
/// own: activating one passes its `HardwareAction` on with
/// `Origin::SystemUi`, and one that starts motion is refused here, naming it
/// (the hardware handler refuses it again). Their ids are stable names, so
/// they need no `ui_revision`.
pub(super) fn apply(
    mut messages: ResMut<Messages<Act<RobotAction>>>,
    mut in_flight: ResMut<InFlight<RobotAction>>,
    mut replies: ResMut<Replies>,
    view: Option<ResMut<RobotView>>,
    orbit: Option<Single<&mut Orbit, With<RobotCamera>>>,
    hardware: Option<Res<super::hardware::Hardware>>,
    mut to_hardware: MessageWriter<Act<super::hardware::HardwareAction>>,
) {
    let (Some(mut view), Some(mut orbit)) = (view, orbit) else {
        actions::apply(&mut messages, &mut in_flight, &mut replies, |_, _| Outcome::Done(Err("the robot view is not open".into())));
        return;
    };
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let synced = hardware.as_ref().is_some_and(|hw| hw.sync.engaged());
        let result = match action {
            _ if synced && call.remote() && moves_synced_motors(&view, action) => Err(SYNC_REMOTE_REFUSAL.to_string()),
            RobotAction::Activate { id, .. } if id.starts_with("hardware:") => {
                let found = hardware.as_ref().and_then(|hw| super::hardware::panel::controls(hw).into_iter().find(|(i, ..)| i == id));
                match found {
                    None => Err("unknown control; request controls".to_string()),
                    Some((_, _, action, _)) if action.starts_motion() => Err(action.remote_refusal()),
                    // Disabled now: refused here with its reason (the hardware handler's own
                    // answer to a SystemUi action is dropped, so it would read as success).
                    Some((id, _, _, Err(why))) => Err(format!("{id} is disabled: {why}")),
                    Some((_, _, action, _)) => {
                        to_hardware.write(Act { action, origin: Origin::SystemUi });
                        Ok(None)
                    }
                }
            }
            RobotAction::Controls => handle(&mut view, &mut orbit, action).map(|answer| {
                answer.map(|mut listing| {
                    if let (Some(hw), Some(items)) = (hardware.as_ref(), listing.get_mut("controls").and_then(Value::as_array_mut)) {
                        for (id, label, action, ready) in super::hardware::panel::controls(hw) {
                            let reason = if action.starts_motion() { Some(action.remote_refusal()) } else { ready.err() };
                            items.push(json!({"id": id, "label": label, "enabled": reason.is_none(), "disabled_reason": reason, "action": {"hardware": action}}));
                        }
                    }
                    listing
                })
            }),
            _ => handle(&mut view, &mut orbit, action),
        };
        match call.origin {
            Origin::Rest(_) => Outcome::Done(result.map(|answer| answer.unwrap_or_else(|| view.state_json()))),
            Origin::Ui => {
                view.run_message = result.err();
                Outcome::Done(Ok(Value::Null))
            }
            Origin::Quiet | Origin::SystemUi => Outcome::Done(Ok(Value::Null)),
        }
    });
}

/// Why REST and `system_ui` may not steer the run while live motor sync
/// streams its targets to the bench.
const SYNC_REMOTE_REFUSAL: &str = "live motor sync is streaming this run's targets to real motors: REST and system_ui may not start, step, jog, drive or change the speed of the run until sync stops (Pause, Reset and STOP stay available)";

/// Whether `action` (or the control it activates) would move the targets
/// live motor sync streams to the bench: motion requests, jogs, Start and
/// Step, and the run speed. Pause, Reset, replay and gait preview are not:
/// each ends the sync session (`hardware::sync`'s stop rules).
fn moves_synced_motors(view: &RobotView, action: &RobotAction) -> bool {
    match action {
        RobotAction::Motion { .. } | RobotAction::Jog { .. } | RobotAction::JogTo { .. } | RobotAction::Speed { .. } => true,
        RobotAction::Run { action } => matches!(action, RunAction::Start | RunAction::Step),
        RobotAction::Activate { id, .. } => controls(view).into_iter().find(|(i, _, _)| i == id).is_some_and(|(_, _, a)| !matches!(a, RobotAction::Activate { .. }) && moves_synced_motors(view, &a)),
        _ => false,
    }
}

mod commands;
mod keys;
#[cfg(test)]
mod tests;

pub(super) use keys::{buttons, graph_key, motion_keys, overlay_keys, pick_link, planar_keys, speed_keys};

/// Present: `/v1/robot_state`, at most every 100 ms.
pub(super) fn publish(rest: Option<ResMut<crate::rest::Rest>>, view: Res<RobotView>) {
    let Some(mut rest) = rest else { return };
    if rest.0.snapshot_due() {
        rest.0.publish("robot_state", view.state_json());
    }
}

/// The REST form of robot mode's commands: `RobotAction` deserializes
/// through it, so each command keeps its JSON shape (fields, tags, unknown
/// fields refused) and its argument errors. It carries no intent of its own.
pub(crate) mod wire {
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
    pub(crate) enum Ui {
        Controls,
        Activate { id: String, ui_revision: u64 },
    }
    #[derive(Deserialize)]
    #[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
    pub(crate) enum Command {
        State,
        RobotState,
        SystemUi { action: Ui },
        Camera { focus: [f32; 3], radius: f32, yaw: f32, pitch: f32 },
        Fit,
        RobotRun { action: String },
        RobotJog { joint: String, target: Option<f64>, delta: Option<f64> },
        RobotPresets,
        RobotPreset { id: String },
        RobotInput { channels: Option<std::collections::BTreeMap<String, f64>>, key: Option<String> },
        RobotSaveRecording { path: Option<String>, note: Option<String> },
        RobotReplay { file: Option<String>, path: Option<String>, action: Option<String> },
        RobotGait { action: Option<String>, report: Option<String>, path: Option<String>, t: Option<f64>, scale: Option<f64> },
        RobotReload,
        RobotOverlay { contacts: Option<bool>, joints: Option<bool>, deflections: Option<bool>, stress: Option<bool> },
        RobotSpeed { action: Option<String>, scale: Option<f64> },
        RobotRecorded { action: String, t: Option<f64>, scale: Option<f64>, delta: Option<i64> },
    }
}
