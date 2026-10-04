//! Robot mode's action checks (`check`, the planar refusals) and the
//! shared drive, jog and overlay helpers `dispatch` uses too.
use super::*;
/// The twist a drive request asks the run thread for, and whether it is a
/// halt (zero at once): the shared interpretation (`DriveRequest::interpret`,
/// the same one Build mode's robot systems use) against the run's binding,
/// with this mode's refusal for a run without one; for [`check`] and
/// `dispatch` alike.
pub(super) fn drive_request(run: &RunController, request: &DriveRequest) -> Result<(BodyTwist, bool), String> {
    let c = run.controlled().ok_or_else(|| match run.check_drive() {
        Err(why) => format!("{NOT_CONTROLLED}; {why}"),
        Ok(()) => format!("{NOT_CONTROLLED}; driving needs a controller binding (<model stem>.controller.json beside the model) naming a sim.drive/1 profile"),
    })?;
    request.interpret(&c.controlled)
}

/// The action that flips one overlay from its current requested value.
pub(in crate::robot) fn overlay_toggle(view: &RobotView, kind: &str) -> RobotAction {
    let flip = Some(!overlay_on(view, kind));
    match kind {
        "contacts" => RobotAction::Overlay { contacts: flip, joints: None, deflections: None, stress: None },
        "joints" => RobotAction::Overlay { contacts: None, joints: flip, deflections: None, stress: None },
        "stress" => RobotAction::Overlay { contacts: None, joints: None, deflections: None, stress: flip },
        _ => RobotAction::Overlay { contacts: None, joints: None, deflections: flip, stress: None },
    }
}
/// Why the stress overlay cannot be set now.
pub(in crate::robot) fn check_stress(view: &RobotView) -> Result<(), String> {
    if view.planar.is_some() {
        return Err(planar::STRESS.into());
    }
    view.source.as_ref().ok_or(STRESS_PRESET)?;
    view.model.as_ref().ok_or("the robot has not loaded")?;
    Ok(())
}
/// The absolute target a jog action asks for (file validation happens in `check_jog`).
pub(super) fn jog_target(run: &RunController, joint: &str, delta: f64) -> Result<f64, String> {
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
pub(super) fn check_planar(view: &RobotView, p: &PlanarView, action: &RobotAction) -> Result<(), String> {
    match action {
        RobotAction::Run { action } => {
            if let Some(refused) = super::hardware::mirror::refuse_run(*action, view.mirror.is_some()) {
                return Err(refused);
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
        RobotAction::Motion { .. } | RobotAction::Inputs { .. } => Err(planar::MOTION.into()),
        RobotAction::Drive { .. } => Err(PLANAR_DRIVE.into()),
        RobotAction::SaveRecording { .. } => Err(planar::SAVE_RECORDING.into()),
        RobotAction::Replay { .. } | RobotAction::CancelReplay | RobotAction::RefreshRecordings => Err(planar::REPLAY.into()),
        RobotAction::Gait { .. } => Err(planar::GAIT.into()),
        RobotAction::Recorded { .. } => Err(planar::RECORDED.into()),
        RobotAction::ToggleGraphs | RobotAction::Pick { .. } => Err(planar::GRAPHS.into()),
        RobotAction::History { .. } => Err("the run history timeline is for physical (v3) runs; a planar v2 file has none".into()),
        RobotAction::Leaderboard { .. } | RobotAction::Video { .. } => Ok(()),
        RobotAction::View { fit_selected: true, .. } | RobotAction::View { follow: Some(true), .. } => Err("fit selected and follow robot are for physical (v3) robots; a planar v2 file is framed by Fit".into()),
        RobotAction::View { display_hz, .. } => check_view(*display_hz),
        _ => Ok(()),
    }
}
/// Why a display cap is refused (one of `view_tools::DISPLAY_RATES`).
pub(super) fn check_view(display_hz: Option<u32>) -> Result<(), String> {
    match display_hz {
        Some(hz) if !view_tools::DISPLAY_RATES.contains(&hz) => Err(format!("display_hz {hz} is not one of {:?} (0 = automatic)", view_tools::DISPLAY_RATES)),
        _ => Ok(()),
    }
}
/// Why a control is unavailable now (`Ok` when enabled).
pub(in crate::robot) fn check(view: &RobotView, action: &RobotAction) -> Result<(), String> {
    if let Some(p) = &view.planar {
        return check_planar(view, p, action);
    }
    match action {
        RobotAction::SelectJoint { .. } => Err("joint selection (←/→) is for a planar v2 file; select a link to jog its joints".into()),
        RobotAction::Run { action } => {
            if let Some(refused) = super::hardware::mirror::refuse_run(*action, view.mirror.is_some()) {
                return Err(refused);
            }
            view.run.as_ref().ok_or("the robot has not loaded")?.check(*action)
        }
        RobotAction::Jog { joint, delta } => {
            let run = view.run.as_ref().ok_or("the robot has not loaded")?;
            run.check_jog(joint, jog_target(run, joint, *delta)?).map(|_| ())
        }
        RobotAction::JogTo { joint, target } => view.run.as_ref().ok_or("the robot has not loaded")?.check_jog(joint, *target).map(|_| ()),
        RobotAction::View { display_hz, .. } => check_view(*display_hz),
        RobotAction::History { t } => view.run.as_ref().ok_or("the robot has not loaded")?.check_history(*t),
        RobotAction::Pick { channel, on } => view.run.as_ref().ok_or("the robot has not loaded")?.check_pick(&view.picks, channel, *on),
        RobotAction::Motion { request } => view.run.as_ref().ok_or("the robot has not loaded")?.check_motion_request(request),
        RobotAction::Inputs { values } => view.run.as_ref().ok_or("the robot has not loaded")?.check_inputs(values).map(|_| ()),
        RobotAction::Drive { request } => {
            let run = view.run.as_ref().ok_or("the robot has not loaded")?;
            drive_request(run, request)?;
            run.check_drive()
        }
        RobotAction::SaveRecording { .. } => view.run.as_ref().ok_or("the robot has not loaded")?.check_save().map(|_| ()),
        RobotAction::Replay { .. } => view.run.as_ref().ok_or("the robot has not loaded")?.check_replay().map(|_| ()),
        RobotAction::CancelReplay => view.run.as_ref().ok_or("the robot has not loaded")?.check_cancel(),
        RobotAction::Gait { action } => view.run.as_ref().ok_or("the robot has not loaded")?.check_gait(action),
        RobotAction::Speed { speed } => view.run.as_ref().ok_or("the robot has not loaded")?.check_speed(*speed).map(|_| ()),
        RobotAction::Recorded { action } => view.run.as_ref().ok_or("the robot has not loaded")?.check_recorded(action),
        RobotAction::Reload { .. } => view.source.as_ref().ok_or("a preset is not reloaded; reload is for --robot FILE")?.check_reload(),
        RobotAction::Overlay { contacts, joints, deflections, stress } => {
            let run = view.run.as_ref().ok_or("the robot has not loaded")?;
            if joints.is_some() || deflections.is_some() {
                run.check_overlays()?;
            }
            if contacts.is_some() {
                if run.preset().is_some() { run.check_contacts()? } else { run.check_overlays()? }
            }
            if stress.is_some() {
                check_stress(view)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
