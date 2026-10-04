//! Robot mode's `system_ui` control list: (id, label, action) for every
//! control the window shows now, the table `Activate` resolves ids in.
use super::*;
/// The `system_ui` controls: (id, label, action), the table `Activate` resolves ids in.
/// `link`: the selected link (`picked::link`), whose joints get jog controls.
pub(in crate::robot) fn controls(view: &RobotView, link: Option<usize>) -> Vec<(String, String, RobotAction)> {
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
        out.extend(view_controls(view));
        if view.run.as_ref().is_some_and(|r| r.review_time().is_some()) {
            out.push(("history:live".into(), "Back to the live frame".into(), RobotAction::History { t: None }));
        }
        for p in &view.picks {
            out.push((format!("pick:{p}"), format!("Remove {p} from the Picked chart"), RobotAction::Pick { channel: p.clone(), on: false }));
        }
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
        for (joint, step) in jog_joints(view, link) {
            let unit = if step == JOG_STEP_M { "m" } else { "rad" };
            out.push((format!("jog:{joint}:-"), format!("Jog {joint} servo target −{step} {unit}"), RobotAction::Jog { joint: joint.clone(), delta: -step }));
            out.push((format!("jog:{joint}:+"), format!("Jog {joint} servo target +{step} {unit}"), RobotAction::Jog { joint, delta: step }));
        }
        let controlled = view.run.as_ref().and_then(|r| r.controlled());
        if let Some(c) = controlled {
            out.extend(drive_controls(&c.controlled.profile, c.controlled.resolved.deadman.timeout_s));
        }
        if view.preset.is_some() {
            for (id, label, request) in motion_buttons() {
                out.push((id.into(), label.into(), RobotAction::Motion { request }));
            }
            out.extend(input_controls(view));
        }
        // A preset's session and a controlled run's drive Session both record
        // and replay (the same handlers as REST robot_save_recording / robot_replay).
        if view.preset.is_some() || controlled.is_some() {
            out.push(("recording:save".into(), "Save recording".into(), RobotAction::SaveRecording { path: None, note: None }));
            if let Some(r) = view.run.as_ref() {
                for l in r.recordings() {
                    out.push((format!("replay:{}", l.file), format!("Replay {}", l.file), RobotAction::Replay { file: Some(l.file.clone()), path: None }));
                }
            }
            out.push(("replay:cancel".into(), "Cancel replay".into(), RobotAction::CancelReplay));
            out.push(("replay:refresh".into(), "Refresh recordings".into(), RobotAction::RefreshRecordings));
        }
        if view.preset.is_some() {
            for (id, label, action) in gait_controls(view) {
                out.push((id, label, RobotAction::Gait { action }));
            }
        }
    }
    out
}
/// The view-tool controls (`view_tools`): Fit selected, Follow robot (the
/// flip of its state) and each display cap.
pub(in crate::robot) fn view_controls(view: &RobotView) -> Vec<(String, String, RobotAction)> {
    let mut out = vec![
        ("view:fit_selected".to_string(), "Fit selected".to_string(), RobotAction::View { fit_selected: true, follow: None, display_hz: None }),
        ("view:follow".to_string(), (if view.follow { "Stop following the robot" } else { "Follow robot" }).to_string(), RobotAction::View { fit_selected: false, follow: Some(!view.follow), display_hz: None }),
    ];
    for hz in view_tools::DISPLAY_RATES {
        let label = if hz == 0 { "Display: automatic".to_string() } else { format!("Display: {hz} fps") };
        out.push((format!("view:display:{hz}"), label, RobotAction::View { fit_selected: false, follow: None, display_hz: Some(hz) }));
    }
    out
}

/// The Inputs block's buttons as `system_ui` controls: every settable input
/// back to its initial value (`inputs:reset`) and, with motor corrections,
/// all `residual.*` inputs to zero (`inputs:clear_residuals`, the browser's
/// "Clear motor corrections"); sliders are REST `robot_inputs` values.
pub(in crate::robot) fn input_controls(view: &RobotView) -> Vec<(String, String, RobotAction)> {
    let Some(drive) = view.run.as_ref().and_then(|r| r.preset_drive()) else { return Vec::new() };
    let heartbeat = drive.heartbeat.as_ref().map(|h| h.index);
    let settable: Vec<&sim_runtime::session::InputChannel> = drive.inputs.iter().enumerate().filter(|(i, _)| Some(*i) != heartbeat).map(|(_, c)| c).collect();
    if settable.is_empty() {
        return Vec::new();
    }
    let mut out = vec![("inputs:reset".to_string(), "Reset inputs to their initial values".to_string(), RobotAction::Inputs { values: settable.iter().map(|c| (c.name.clone(), c.initial)).collect() })];
    let residuals: std::collections::BTreeMap<String, f64> = settable.iter().filter(|c| c.name.starts_with(run::RESIDUAL_PREFIX)).map(|c| (c.name.clone(), 0.0_f64.clamp(c.lower, c.upper))).collect();
    if !residuals.is_empty() {
        out.push(("inputs:clear_residuals".to_string(), "Clear motor corrections".to_string(), RobotAction::Inputs { values: residuals }));
    }
    out
}

/// A controlled robot's `system_ui` drive controls: forward, back and the
/// two turns at full axis for one request (momentary: the deadman stops it
/// `timeout_s` later), stop, and each of the profile's named actions.
pub(super) fn drive_controls(profile: &sim_domain_control::drive::profile::DriveProfile, timeout_s: f64) -> Vec<(String, String, RobotAction)> {
    let axes = |forward: f64, yaw: f64| RobotAction::Drive { request: DriveRequest::Axes { forward, lateral: 0.0, yaw } };
    let momentary = format!("(momentary, deadman {timeout_s} s)");
    let mut out = vec![
        ("drive:forward".to_string(), format!("Drive forward {momentary}"), axes(1.0, 0.0)),
        ("drive:back".to_string(), format!("Drive back {momentary}"), axes(-1.0, 0.0)),
        ("drive:left".to_string(), format!("Turn left {momentary}"), axes(0.0, 1.0)),
        ("drive:right".to_string(), format!("Turn right {momentary}"), axes(0.0, -1.0)),
        ("drive:stop".to_string(), "Stop driving".to_string(), RobotAction::Drive { request: DriveRequest::Stop }),
    ];
    for a in &profile.actions {
        out.push((format!("drive:action:{}", a.name), format!("Drive action {}", a.name), RobotAction::Drive { request: DriveRequest::Action { name: a.name.clone() } }));
    }
    out
}
/// The `system_ui` controls of a planar (v2) file: bodies, sections, view,
/// reload, run, speed, joint selection and target moves (ids by built joint
/// index), the contacts overlay, and the controls it shows but cannot use
/// (graphs, joint frames, deflections, stress), listed disabled with the reason.
pub(super) fn planar_controls(view: &RobotView, p: &PlanarView) -> Vec<(String, String, RobotAction)> {
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
pub(super) fn gait_controls(view: &RobotView) -> Vec<(String, String, GaitAction)> {
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
pub(super) fn recorded_controls() -> Vec<(String, String, RecordedAction)> {
    let mut out: Vec<(String, String, RecordedAction)> = RECORDED_TRANSPORT.iter().map(|(id, label, a)| (format!("recorded:{id}"), label.to_string(), *a)).collect();
    for scale in SPEED_SCALES {
        out.push((format!("recorded:speed:{scale}"), format!("Recorded playback speed ×{scale}"), RecordedAction::Speed { scale }));
    }
    out
}
