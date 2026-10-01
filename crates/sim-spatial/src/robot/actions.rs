//! Robot mode's actions: [`RobotAction`] (one enum for the buttons, keys,
//! `system_ui` controls and REST commands), its REST form, its commands in
//! the action registry, the input systems that write it and [`apply`], its
//! one handler (`ViewerSet::Actions`).
use super::*;
use crate::app::actions::{self, Act, InFlight, Origin, Replies, Spec, spec};
use crate::robot_motion::KEYS;
use crate::robot_run::SPEED_SCALES;
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
    /// The run speed scale (robot_run::PACING; pacing only): keys =/+ and −, the header
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
    let servo = crate::robot_run::servo(run.model(), joint)?;
    Ok(run.requested_target(&servo) + delta)
}
/// Why a control is unavailable now (`Ok` when enabled).
pub(super) fn check(view: &RobotView, action: &RobotAction) -> Result<(), String> {
    match action {
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
/// A control action: validated by [`check`] (motion and save validate in
/// their own handler), then applied.
fn dispatch(view: &mut RobotView, orbit: &mut RobotOrbit, action: RobotAction) -> Result<(), String> {
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
fn handle(view: &mut RobotView, orbit: &mut RobotOrbit, action: &RobotAction) -> Result<Option<Value>, String> {
    match action {
        RobotAction::State => Ok(Some(json!({"robot_state": view.state_json()}))),
        RobotAction::RobotState => Ok(None),
        RobotAction::Presets => {
            let (file, root) = (view.presets.clone()?, view.root.clone()?);
            let presets = crate::robot_preset::list(&file)?;
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
            *orbit = RobotOrbit { focus: Vec3::from_array(*focus), radius, yaw, pitch, home: false, ..*orbit };
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
    orbit: Option<Single<&mut RobotOrbit>>,
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

/// Input: a pressed button's action (tabs, links, run, speed, reload,
/// overlays, graphs, jog, motion, recording, replay, recorded and gait).
pub(super) fn buttons(clicks: Query<(&Interaction, &RobotAction), Changed<Interaction>>, mut out: MessageWriter<Act<RobotAction>>) {
    for (interaction, action) in &clicks {
        if *interaction == Interaction::Pressed {
            out.write(Act::ui(action.clone()));
        }
    }
}

/// Input: physical W/A/S/D (press/release) and X (Stop) while a built preset
/// has a motion config: the same `RobotAction::Motion` as the buttons,
/// `system_ui` motion:* and REST `robot_input`. Robot mode's camera is
/// mouse-only, so these keys take no camera action. Bevy releases every key
/// when the window loses keyboard focus, which requests zero, as the browser's blur.
///
/// While the Leg calibration panel is shown, A is its hold-to-move key
/// (lower), as the page's capture-phase key handler takes it from WASD; W, S
/// and D still steer. The panel opening sends the held keys again without A
/// (when the physical keys drive), so an A held for WASD when it opened does
/// not keep the robot strafing: its release is the panel's and is never seen
/// here. Closing the panel sends nothing: a still-held A drives again only
/// on a fresh press (a jog's A never becomes a strafe).
pub(super) fn motion_keys(keys: Res<ButtonInput<KeyCode>>, view: Res<RobotView>, hardware: Option<Res<super::hardware::Hardware>>, mut was_open: Local<bool>, mut out: MessageWriter<Act<RobotAction>>) {
    const ALL: [(KeyCode, char); 4] = [(KeyCode::KeyW, 'w'), (KeyCode::KeyA, 'a'), (KeyCode::KeyS, 's'), (KeyCode::KeyD, 'd')];
    let panel = hardware.is_some_and(|h| h.open);
    // Not `panel && !replace(..)`: the replace must run when the panel closes too.
    let opened = !std::mem::replace(&mut *was_open, panel) && panel;
    let map: Vec<(KeyCode, char)> = ALL.into_iter().filter(|(_, k)| !(panel && *k == 'a')).collect();
    let Some(run) = view.run.as_ref().filter(|r| r.motion_keys_active()) else { return };
    let request = if keys.just_pressed(KeyCode::KeyX) {
        Some(MotionRequest::Stop)
    } else if (opened && run.keys_physical()) || map.iter().any(|(c, _)| keys.just_pressed(*c) || keys.just_released(*c)) {
        let held: Vec<char> = map.iter().filter(|(c, _)| keys.pressed(*c)).map(|(_, k)| *k).collect();
        // A release with only a latched (system_ui/REST) key active leaves that key's request alone.
        let pressed = map.iter().any(|(c, _)| keys.just_pressed(*c));
        (pressed || run.keys_physical()).then_some(MotionRequest::HeldKeys(held))
    } else {
        None
    };
    if let Some(request) = request {
        out.write(Act::ui(RobotAction::Motion { request }));
    }
}

/// Input: key G, the same `RobotAction::ToggleGraphs` as the Graphs button and `system_ui` graphs:toggle.
pub(super) fn graph_key(keys: Res<ButtonInput<KeyCode>>, mut out: MessageWriter<Act<RobotAction>>) {
    if keys.just_pressed(KeyCode::KeyG) {
        out.write(Act::ui(RobotAction::ToggleGraphs));
    }
}

/// Input: keys C / J / F / H, the same `RobotAction::Overlay` as the inspector buttons and `system_ui` overlay:*.
pub(super) fn overlay_keys(keys: Res<ButtonInput<KeyCode>>, view: Res<RobotView>, mut out: MessageWriter<Act<RobotAction>>) {
    for (kind, _, key) in OVERLAYS {
        if keys.just_pressed(key) {
            out.write(Act::ui(overlay_toggle(&view, kind)));
        }
    }
}

/// Input: keys =/+ and − (main row and numpad), the same `RobotAction::Speed` as the header
/// −/+ buttons, `system_ui` run:speed_* and REST robot_speed. A refusal at ×8 / ×0.125 shows in the header.
pub(super) fn speed_keys(keys: Res<ButtonInput<KeyCode>>, mut out: MessageWriter<Act<RobotAction>>) {
    let speed = if keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) {
        SpeedRequest::Up
    } else if keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
        SpeedRequest::Down
    } else {
        return;
    };
    out.write(Act::ui(RobotAction::Speed { speed }));
}

/// A click on a link in the 3D view selects it (the same `SelectLink` as the list).
pub(super) fn pick_link(click: On<Pointer<Click>>, links: Query<&LinkMesh>, view: Res<RobotView>, mut out: MessageWriter<Act<RobotAction>>) {
    if click.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    if let Ok(link) = links.get(click.entity) {
        let name = view.link_name(link.0).unwrap_or_default().to_string();
        out.write(Act::quiet(RobotAction::SelectLink { index: link.0, name }));
    }
}

/// Present: `/v1/robot_state`, at most every 100 ms.
pub(super) fn publish(rest: Option<ResMut<crate::rest::Rest>>, view: Res<RobotView>) {
    let Some(mut rest) = rest else { return };
    if rest.0.snapshot_due() {
        rest.0.publish("robot_state", view.state_json());
    }
}

impl actions::Action for RobotAction {
    fn commands() -> Vec<Spec> {
        fn c(name: &'static str, example: Value, description: &str) -> Spec {
            spec(name, actions::ROBOT, example, description)
        }
        vec![
            c("robot_state", json!({}), "Read-only robot mode: file, workspace (the resolved root: root, found_by override | env | opened_file | cwd, from, error, rule; also in GET /v1/capabilities), status (loading | loaded | error, with the error naming the path), link_count, links, the selected link (mass, com, inertia, material and its density or material_in_file=false, file_notes), joints, motors, transmissions, battery, actuator_profiles (with content hashes), uncertainty, identification, the source block verbatim (the export's own `source`), source_file (--robot FILE's path, sha256, loaded_at, reload_count, watching, last_reload, run_reset; see robot_reload; watching=false for a preset), notice (the last reload's result), and cad_link (current | stale | missing | no_recorded_hash | no_source_file | unreadable, with the resolution rule and paths tried). Numbers are full-precision JSON; provenance is null unless the file carries a typed label (provenance_rule). Also the inspector section and scroll, run (null until loaded; phase idle with null time before any build; see robot_run), and overlays (--robot FILE run-thread contacts, joint frames and deflections with flags, counts, samples and scales, and overlays.stress for the read-only .simresult.json; available=false with the reason for presets; see robot_overlay). Nothing is written."),
            c("system_ui", json!({"action":{"operation":"controls"}}), "Discover the link list, inspector sections (section:link | joints | drives | source), inspector scrolling and view controls, the run controls (run:start | pause | step | reset, and run:speed_down | run:speed_up | run:speed:<scale>: the same RobotAction::Speed as robot_speed), robot:reload and overlay:contacts | overlay:joints | overlay:deflections | overlay:stress for --robot FILE (the same RobotAction::Reload as robot_reload and RobotAction::Overlay as robot_overlay; overlay controls are listed for presets but disabled with the reason), and for a preset the run, motion, recording, replay and gait preview controls (gait:open:<report>, gait:play, gait:pause, gait:stop, gait:seek:0 | - | + (±period/12 from the latest pose), gait:speed:0.25 | 0.5 | 1, gait:list: the same RobotAction::Gait as REST robot_gait), and for a recorded preset the timeline controls recorded:start | recorded:step- | recorded:play | recorded:pause | recorded:step+ | recorded:speed:<scale> (the same RobotAction::Recorded as REST robot_recorded) (controls) and activate one by id with the current ui_revision (activate), through the same handler as a click. Selection is shared by the list, the 3D view and robot_state."),
            c("robot_jog", json!({"joint":"left axle","target":0.5}), &format!("Servo-target jog of one joint by its file name: {JOG_LABEL}. Give target (absolute; rad, or m on a prismatic joint) or delta (from the current requested target). The same handler as the jog +/− buttons and system_ui jog:<joint>:+/- controls (±{JOG_STEP_RAD} rad, ±{JOG_STEP_M} m prismatic; listed for the joints touching the selected link). Errors name the joint: unknown joint, no servo target (passive joint, firmware none, fixed, or a trajectory-mode file), non-finite target, or a target outside the file's limits (with the limit; never clamped); after a failed run, Reset first. {JOG_SEMANTICS} robot_state.jog reports control mode, label and, per joint, the limit (or no limit in file), requested target, and the target and measured value from the latest accepted frame.")),
            c("robot_run", json!({"action":"start"}), "Run controls, the same handler as the Run/Pause/Step/Reset buttons and system_ui run:* controls. start runs the shared PhysicalRobot on the run thread (built from the loaded model with sim_runtime::registry() and BuildOptions::default()), paced at most to speed_scale × real time (robot_speed; default ×1); pause stops it; step advances exactly one 0.02 s chunk and is refused while running; reset rebuilds at t = 0 (assembly pose), bumps the generation and leaves it paused (allowed after a failure). An unknown action is an error naming it and listing the valid ones. robot_state.run reports phase (idle | building | running | paused | failed), time, steps (chunks), chunk_s, measured rtf, speed_scale (requested) and compute_limited, generation, error and the latest accepted frame. Nothing is written."),
            c("robot_speed", json!({"scale":4}), &format!("Set the run speed scale (× real time) for --robot FILE runs, preset runs and replays alike: scale one of {} (powers of two, as sim-app's cad scene; anything else is refused naming the allowed values, never clamped), or action up | down alone (refused at ×8 / ×0.125, naming the limit). The same RobotAction::Speed as keys =/+ and − (numpad too), the header −/×scale/+ buttons (×scale resets to ×1) and system_ui run:speed_up | run:speed_down | run:speed:<scale>. Allowed in every phase; it applies from the next chunk (or the next Run) and survives Reset and a file reload. Pacing only: dt, chunk_s and the physics step are unchanged. {} robot_state.run reports speed_scale (requested), rtf (achieved) and compute_limited: {}.", SPEED_SCALES.map(|s| s.to_string()).join(", "), robot_run::PACING, robot_run::COMPUTE_LIMITED_RULE)),
            c("robot_recorded", json!({"action":"seek","t":0.8}), &format!("Recorded-preset timeline ({}): action play | pause | start alone, seek with t (recorded time, s), step with delta (+1 | -1) or speed with scale. The same RobotAction::Recorded as the inspector Recorded buttons (Start, Step −1, Play, Pause, Step +1) and system_ui recorded:start | recorded:step- | recorded:play | recorded:pause | recorded:step+ | recorded:speed:<scale>. One playback worker (not the UI thread) owns the pre-mapped frames, runs the clock and publishes generation-stamped frames; the lookup is {}. {} {} {} {} speed accepts exactly one of {} (the run speed scales, never clamped) and is the same scale as robot_speed and the header −/×/+ buttons, which drive this playback for a recorded preset. Refused naming the reason: a non-recorded view (an embedded preset or --robot FILE), a non-finite or out-of-range t, a step at either end or not ±1, play while playing, pause while not playing. The response returns at once; robot_state.recorded reports label, preset, scene/capture paths, frame_count, first/last_time_s, duration_s, time_s, frame_index, frame_time_s, speed, phase (playing | paused | ended), generation, frame_generation and pending (a command not yet applied by the worker), description, readiness and evidence verbatim from presets.json, meta (the capture's source fidelity/cad_sha256/file/cad_revision, completed, error, simulated_s, stepping_wall_s, step_s, completed_steps, requested_steps as read; absent ones null and listed in absent), recorded_rate, unmatched_capture_links and the rules. Nothing is simulated.", crate::robot_preset::RECORDED_LABEL, sim_runtime::embedded_capture::LOOKUP_RULE, robot_playback::CLOCK_RULE, robot_playback::SEEK_RULE, robot_playback::STEP_RULE, robot_playback::START_RULE, SPEED_SCALES.map(|s| s.to_string()).join(", "))),
            c("robot_presets", json!({}), &format!("List the robot presets declared in {} (its paths resolved against the workspace root, reported in workspace): id, label, mode, scene/config/task paths, inputs_exist and missing, under_ignored_runs, capture, runs_as (EmbeddedEnvironment with a task, else EmbeddedSession; recorded playback for mode `recorded`) and openable with the reason when not (the build itself is not attempted): {}. Mode `embedded` runs natively; mode `recorded` plays back its capture (no physics).", crate::robot_preset::PRESETS, crate::robot_preset::OPENABLE_RULE)),
            c("robot_preset", json!({"id":"robot-measured-400hz"}), "Open a declared embedded or recorded preset by id. A recorded preset (mode `recorded`, a scene and a capture, e.g. robot-lift-5mm) reads its scene and its *-execution.json capture on the loader thread with the shared sim_runtime::embedded_capture reader, maps every frame to the scene's links by name and shows frame 0; robot_state.preset reports frame_count, duration_s, the capture's metadata as written (absent fields null), unmatched_capture_links and load_seconds; no physics is built, and run, jog, motion, save recording, replay, gait preview and overlays are refused naming the preset. An embedded preset opens in this window (replacing the current robot and stopping its run thread). Unknown ids, other modes and missing inputs are refused naming the id. The scene, config and task are parsed exactly as declared on a worker thread; scene.robot is drawn and inspected through the same loader as --robot FILE. Run/Pause/Step/Reset (robot_run) then drive the shared EmbeddedEnvironment (task) or EmbeddedSession (no task) on the run thread with seed 0 (recorded), one action interval or clamp(report_every, 1, 40) nominal steps per chunk. robot_state.preset reports id, label, paths, readiness and evidence verbatim, seed, step_s, chunk and step counts; run.phase `ended` with run.end reports a reached horizon or a terminated/truncated episode. Servo-target jog is not offered for presets."),
            c("robot_input", json!({"channels":{"command.forward_speed":0.001}}), &format!("Motion request for a running robot preset: {}. Give channels (values by motion channel name; the other motion channels keep their requested values) or key (w | a | s | d latches that key's request until stop, another key or a channel request; stop sets every motion channel to 0). The same handler as physical W/A/S/D (press/release) and X (stop), the W/A/S/D/Stop buttons and system_ui motion:w|a|s|d|stop. Motion channels come from the session's policy_contract.step_reference.config, else the preset's presets.json motion_commands (as web/viewer/motion-commands.mjs); key vectors from motion_key_vectors, else the channel bounds. Refused naming the channel and its bounds: an unknown channel, a session input that is not a motion channel, a non-finite value, or a value or summed key vector outside the channel's bounds ({}). Also refused, naming the reason: --robot FILE, a preset with no motion config, no built session (Run or Step builds it), a failed or ended run, an exhausted heartbeat. A declared motion_heartbeat is {} robot_state.motion reports source, channels with bounds, requested and held values, active keys, heartbeat, last refusal and the label.", robot_motion::LABEL, robot_motion::CLAMP_RULE, robot_motion::HEARTBEAT_RULE)),
            c("robot_save_recording", json!({"note":"after motion:w"}), &format!("Save the loaded preset run's recording: the same handler as the Save recording button and system_ui recording:save. The run thread snapshots the shared recording (EmbeddedEnvironment::episode_recording() for a preset with a task, EmbeddedSession::recording() without, as the browser's Download) in any phase with a built session, running, paused, ended or failed; a writer thread writes it, so the response returns at once with recording.pending set and robot_state.recording.last_saved {{path, meta_path, kind, version, completed_steps, replayable, not_replayable_reason, failure, saved_utc, bytes}} (or recording.error) once written. Optional path (relative to the root or absolute) and note (kept in the sidecar). {} {} {} Refused, naming the reason: --robot FILE, no built session (Run or Step first), a save still being written, a path under examples/, cad/ or web/, a name not ending in .json or ending in .meta.json, and an existing file (reported in recording.error).", robot_recording::LOCATION_RULE, robot_recording::FILE_RULE, robot_recording::REPLAYABLE_RULE)),
            c("robot_gait", json!({"report":"6216-Bayesian-009-472d11d4"}), &format!("Kinematic gait preview on the loaded preset: {}. Open a gait with report (a name in robot_state.gait_preview.reports: {}) or path (a compiled.json, relative to the workspace root or absolute); then {{\"action\":\"play\"}}, pause, stop, list, {{\"action\":\"seek\",\"t\":0.5}} (gait time, s) or {{\"action\":\"speed\",\"scale\":0.5}} (0 < scale <= 1). One worker reads the gait with sim_runtime::gait_playback::compiled_with_governor (governor from detailed.spec.json, else spec-identity.json, else none) and Gait::from_compiled, samples it ({}) and poses the scene with the shared KinematicMirror at lift {} m (web/viewer/calibration-mirror.mjs). Refused naming the reason: --robot FILE (no scene), a missing file (named), an unknown report, a gait joint that is not a coordinate of the preset's scene (named), a mirror that cannot serve the scene, a running physics run or a replay in progress; Run, Step and Replay are refused while a gait is loaded. Load errors after the command returns land in robot_state.gait_preview.error, with any previous preview kept. robot_state.gait_preview reports label, phase (idle | loading | playing | paused | failed), generation and frame_generation, report, compiled, governor_source, period_s, nominal_speed_m_s, report_speed_m_s, status and fidelity (the report's, verbatim), gait_time_s, speed_scale, desired_rad and commanded_rad by joint, drives, lift_m, authored_limit_violations and solve_ms. Nothing is simulated, written or sent to hardware.", robot_gait::LABEL, robot_gait::LISTING_RULE, robot_gait::SAMPLING_RULE, robot_gait::LIFT_M)),
            c("robot_replay", json!({"file":"20260930T060822.729Z.json"}), &format!("Replay a saved recording of the loaded preset: the same handler as the inspector Replay buttons and system_ui replay:<file>. Give file (a bare name listed in robot_state.recordings.files, in runs/robot-presets/<preset-id>/) or path (any readable recording .json, relative to the root or absolute; reading is not restricted). {{\"action\":\"cancel\"}} stops the replay between chunks (system_ui replay:cancel); {{\"action\":\"list\"}} lists the recordings again off the UI thread (system_ui replay:refresh). {} {} {} {} robot_state.replay reports path, phase (idle | replaying | cancelled | done | failed), completed/total with unit, completed_steps and recorded_completed_steps, verdict, error, measured, replaced and sidecar. Refused, naming the reason: --robot FILE, a replay already in progress, a running run (Pause first), a building session, a missing or non-.json file, a recording of the other kind, a runtime mismatch (the runtime's message) and a session identity mismatch (the viewer's labelled check).", robot_recording::REPLAY_RULE, robot_recording::VERDICT_RULE, robot_recording::IDENTITY_RULE, robot_recording::MEASURED_RULE)),
            c("robot_reload", json!({}), &format!("Re-read the opened --robot FILE now: the same RobotAction::Reload as the watch, the header Reload button and system_ui robot:reload. {} Returns at once; the result lands in robot_state.source_file {{path, sha256 (of the displayed model's bytes), loaded_at (UTC), reload_count (successful reloads), unchanged_checks, watching, in_flight, last_reload {{trigger watch | manual, outcome loaded | unchanged | failed, error naming the path, at (UTC)}}, run_reset, showing_last_good, failing_error}} and robot_state.notice. A loaded reload replaces the model, meshes, link list, notes and cad_link, keeps the selected link by name (else clears it with a note), discards any run or jog and spawns a fresh idle run thread whose generation is the old one + 1 (robot_state.run.generation; graphs clear by the generation rule). Refused naming the reason: a preset, a load or reload already in flight. (robot_state.source is still the file's own export source block.)", robot_source::RULE)),
            c("robot_overlay", json!({"contacts":true,"joints":true,"deflections":false,"stress":true}), &format!("Show or hide the --robot FILE overlays. stress (key H, system_ui overlay:stress; default off, as sim-app's cad scene) colours the link meshes per vertex from the model's read-only .simresult.json through sim_domain_robot::stress_results ({}); robot_state.overlays.stress reports enabled, painting, path, mtime_unix_s and mtime_utc, status (current | stale | no recorded hash | no results file | invalid results file), recorded_physical_hash, model_physical_hash, peak_stress_pa per link, hotspot_links, error, absent and paint_seconds. {} Run-thread overlays: contacts (spheres at PhysicalRobot::contacts points with force lines at {} m/N; red on the ground, orange against another link), joints (PhysicalRobot::joint_frames: a white sphere and each axis drawn ±{} m in yellow, cyan, magenta) and deflections (PhysicalRobot::deflections: flexible-link boundary displacement lines magnified ×{}). Give any subset; the others keep their values. The same RobotAction::Overlay as keys C / J / F / H, the inspector overlay buttons and system_ui overlay:contacts | overlay:joints | overlay:deflections | overlay:stress. Run-thread defaults: all on, as sim-app's cad scene draws them. {} Drawn in the model frame through RobotRoot's transform (the link meshes' parent), only from the latest accepted frame of the current generation. robot_state.overlays reports flags, frame_flags, frame_generation, frame_time, contacts {{count, sample: first {} of link, other (link name | ground), point, force, penetration}}, joints {{count, sample}}, deflections {{count, max_displacement_m}} and scales. Refused for presets (their session frames publish no contacts, joint frames or deflections, and a preset has no results file).", sim_domain_robot::stress_results::SCALE, robot_stress::RULE, robot_run::FORCE_SCALE_M_PER_N, robot_run::JOINT_AXIS_HALF_M, robot_run::DEFLECTION_MAGNIFICATION, robot_run::OVERLAY_COST_RULE, robot_run::OVERLAY_SAMPLE)),
            c("state", json!({}), "Robot mode: {robot_state (as robot_state), viewer_mode}"),
            c("camera", json!({"focus":[0,0,0],"radius":0.5,"yaw":0.7,"pitch":0.4}), "Absolute orbit in the display frame (Y up); SI metres and radians"),
            c("fit", json!({}), "Fit the robot"),
        ]
    }
    fn controls() -> &'static [&'static str] {
        &[
            "link:<index>", "clear_selection", "section:<section>", "inspector:scroll_down", "inspector:scroll_up", "fit", "robot:reload", "graphs:toggle", "overlay:<kind>",
            "run:<action>", "run:speed_down", "run:speed_up", "run:speed:<scale>", "recorded:<transport>", "recorded:speed:<scale>", "jog:<joint>:-", "jog:<joint>:+",
            "motion:w", "motion:a", "motion:s", "motion:d", "motion:stop", "recording:save", "replay:<file>", "replay:cancel", "replay:refresh",
            "gait:open:<report>", "gait:play", "gait:pause", "gait:stop", "gait:seek:<step>", "gait:speed:<scale>", "gait:list",
            // The Leg calibration panel's controls (hardware::panel::controls), passed on to its handler.
            "hardware:<name>",
        ]
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

/// A REST command as its action (the checks that need no view, with the
/// errors they always gave).
impl TryFrom<wire::Command> for RobotAction {
    type Error = String;
    fn try_from(command: wire::Command) -> Result<Self, String> {
        use wire::Command as W;
        Ok(match command {
            W::State => RobotAction::State,
            W::RobotState => RobotAction::RobotState,
            W::SystemUi { action: wire::Ui::Controls } => RobotAction::Controls,
            W::SystemUi { action: wire::Ui::Activate { id, ui_revision } } => RobotAction::Activate { id, ui_revision },
            W::Camera { focus, radius, yaw, pitch } => RobotAction::Camera { focus, radius, yaw, pitch },
            W::Fit => RobotAction::Fit,
            W::RobotRun { action } => RobotAction::Run { action: RunAction::parse(&action)? },
            W::RobotJog { joint, target, delta } => match (target, delta) {
                (Some(target), None) => RobotAction::JogTo { joint, target },
                (None, Some(delta)) => RobotAction::Jog { joint, delta },
                _ => return Err("robot_jog needs exactly one of target (absolute, rad or m) or delta".into()),
            },
            W::RobotPresets => RobotAction::Presets,
            W::RobotPreset { id } => RobotAction::OpenPreset { id },
            W::RobotInput { channels, key } => {
                let request = match (channels, key.as_deref()) {
                    (Some(map), None) => MotionRequest::Channels(map),
                    (None, Some("stop")) => MotionRequest::Stop,
                    (None, Some(k)) => match k.chars().collect::<Vec<_>>()[..] {
                        [c] if KEYS.contains(&c) => MotionRequest::Key(c),
                        _ => return Err(format!("robot_input key `{k}` is not one of w, a, s, d, stop")),
                    },
                    _ => return Err("robot_input needs exactly one of channels ({\"<motion channel>\": value, …}) or key (w | a | s | d | stop)".into()),
                };
                RobotAction::Motion { request }
            }
            W::RobotSaveRecording { path, note } => RobotAction::SaveRecording { path, note },
            W::RobotReplay { file, path, action } => match (action.as_deref(), file.is_some() || path.is_some()) {
                (None | Some("start"), _) => RobotAction::Replay { file, path },
                (Some("cancel"), false) => RobotAction::CancelReplay,
                (Some("list"), false) => RobotAction::RefreshRecordings,
                (Some(a @ ("cancel" | "list")), true) => return Err(format!("robot_replay action `{a}` takes no file or path")),
                (Some(a), _) => return Err(format!("unknown robot_replay action `{a}`; valid actions: start (default, with file or path), cancel, list")),
            },
            W::RobotGait { action, report, path, t, scale } => {
                let action = match (action.as_deref(), report, path, t, scale) {
                    (None | Some("open"), Some(r), None, None, None) => GaitAction::Open { source: GaitSource::Report(r) },
                    (None | Some("open"), None, Some(p), None, None) => GaitAction::Open { source: GaitSource::Path(p) },
                    (Some("seek"), None, None, Some(t), None) => GaitAction::Seek { t },
                    (Some("speed"), None, None, None, Some(scale)) => GaitAction::Speed { scale },
                    (Some("play"), None, None, None, None) => GaitAction::Play,
                    (Some("pause"), None, None, None, None) => GaitAction::Pause,
                    (Some("stop"), None, None, None, None) => GaitAction::Stop,
                    (Some("list"), None, None, None, None) => GaitAction::List,
                    (a, ..) => return Err(format!("robot_gait {}: give exactly one of report or path (open, the default), or action seek with t, speed with scale, or play | pause | stop | list alone", a.map_or("open".into(), |a| format!("action `{a}`")))),
                };
                RobotAction::Gait { action }
            }
            W::RobotReload => RobotAction::Reload { trigger: ReloadTrigger::Manual },
            W::RobotOverlay { contacts: None, joints: None, deflections: None, stress: None } => return Err("robot_overlay needs at least one of contacts, joints, deflections, stress (true | false)".into()),
            W::RobotOverlay { contacts, joints, deflections, stress } => RobotAction::Overlay { contacts, joints, deflections, stress },
            W::RobotSpeed { action, scale } => {
                let speed = match (action.as_deref(), scale) {
                    (None | Some("set"), Some(scale)) => SpeedRequest::Set { scale },
                    (Some("up"), None) => SpeedRequest::Up,
                    (Some("down"), None) => SpeedRequest::Down,
                    (a, _) => return Err(format!("robot_speed {}: give scale (one of {}) or action up | down alone", a.map_or("without scale".into(), |a| format!("action `{a}`")), SPEED_SCALES.map(|s| s.to_string()).join(", "))),
                };
                RobotAction::Speed { speed }
            }
            W::RobotRecorded { action: name, t, scale, delta } => {
                let action = match (name.as_str(), t, scale, delta) {
                    ("play", None, None, None) => RecordedAction::Play,
                    ("pause", None, None, None) => RecordedAction::Pause,
                    ("start", None, None, None) => RecordedAction::Start,
                    ("seek", Some(t), None, None) => RecordedAction::Seek { t },
                    ("step", None, None, Some(delta)) => RecordedAction::Step { delta },
                    ("speed", None, Some(scale), None) => RecordedAction::Speed { scale },
                    (a, ..) => return Err(format!("robot_recorded action `{a}`: give play | pause | start alone, seek with t (s), step with delta (+1 | -1) or speed with scale (one of {})", SPEED_SCALES.map(|s| s.to_string()).join(", "))),
                };
                RobotAction::Recorded { action }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Every `system_ui` control robot mode lists for a loaded file fits one
    /// of its registered control patterns, once, and REST activation parses
    /// into the action that resolves it (`Activate`, through `controls`).
    #[test]
    fn every_listed_control_fits_a_registered_pattern() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("examples/wheeled-robot/baseline/robot.simrobot.json");
        let loaded = load(&path).unwrap();
        let mut view = RobotView::new(path, None, None);
        view.run = Some(RunController::replace(None, loaded.model.clone()).0);
        view.model = Some(loaded.model);
        view.panels_ready = true;
        view.selected = Some(1);
        let listed = controls(&view);
        let patterns = <RobotAction as actions::Action>::controls();
        let mut ids = std::collections::BTreeSet::new();
        for (id, _, _) in &listed {
            assert!(patterns.iter().any(|p| actions::control_matches(p, id)), "{id} fits no registered control pattern");
            assert!(ids.insert(id.clone()), "{id} is listed twice");
            let command = sim_api::Command { command: "system_ui".into(), args: json!({"action": {"operation": "activate", "id": id, "ui_revision": view.ui_revision}}) };
            assert_eq!(<RobotAction as actions::Action>::parse(&command), Ok(RobotAction::Activate { id: id.clone(), ui_revision: view.ui_revision }));
        }
        for prefix in ["link:", "section:", "run:", "overlay:", "graphs:toggle"] {
            assert!(ids.iter().any(|id| id.starts_with(prefix)), "no {prefix} control");
        }
    }
    /// A REST command keeps its argument errors through the action's REST form.
    #[test]
    fn rest_argument_errors_are_unchanged() {
        let parse = |name: &str, args: Value| <RobotAction as actions::Action>::parse(&sim_api::Command { command: name.into(), args });
        assert_eq!(parse("robot_jog", json!({"joint": "j"})).unwrap_err(), "robot_jog needs exactly one of target (absolute, rad or m) or delta");
        assert_eq!(parse("robot_overlay", json!({})).unwrap_err(), "robot_overlay needs at least one of contacts, joints, deflections, stress (true | false)");
        assert_eq!(parse("robot_replay", json!({"action": "list", "file": "x.json"})).unwrap_err(), "robot_replay action `list` takes no file or path");
        assert!(parse("robot_run", json!({"action": "start", "extra": 1})).unwrap_err().contains("unknown field `extra`"));
        assert_eq!(parse("robot_speed", json!({"action": "up"})), Ok(RobotAction::Speed { speed: SpeedRequest::Up }));
        assert_eq!(parse("fit", json!({})), Ok(RobotAction::Fit));
    }
}
