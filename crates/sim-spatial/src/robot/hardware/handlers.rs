//! The Leg calibration panel's handlers, one per [`HardwareAction`]
//! (applied by [`super::actions::apply`]): the page's click handlers for
//! the form, the link commands, STOP and loss, the gait list, Download
//! calibration, and the remote-control check.
use super::actions::{Boundary, Direction, GaitMode, HardwareAction, Loss, connect, operator_stop, stop_immediate};
use super::link::{self, LinkCommand};
use super::panel::NOT_CONNECTED;
use super::{Hardware, Section};
use crate::app::actions::{Call, Origin};
use crate::jobs::{Job, Pool};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A handler's answer before it is shaped for its origin.
pub(crate) enum Answer {
    Done(Result<Option<Value>, String>),
    Pending,
}
pub(super) fn done() -> Answer {
    Answer::Done(Ok(None))
}

/// A remote (REST, `system_ui`) action whose panel control is disabled now
/// is refused with its reason, as the page's disabled button would be.
/// Reads, STOP, loss, export (REST waits for a running one) and gait list are not checked.
pub(super) fn remote_check(hw: &Hardware, action: &HardwareAction, call: &Call) -> Result<(), String> {
    if !call.remote() || matches!(action, HardwareAction::Status | HardwareAction::Stop | HardwareAction::Loss { .. } | HardwareAction::Export | HardwareAction::LoadGaits) {
        return Ok(());
    }
    let view = super::panel::panel_view(hw, Instant::now());
    let enabled = match action {
        HardwareAction::Target { .. } | HardwareAction::TargetCommit => view.target_enabled,
        HardwareAction::Speed { .. } | HardwareAction::PwmCeiling { .. } => view.blocked.is_none(),
        // Releases are always allowed to request hold after an accepted press.
        HardwareAction::JogRelease { .. } => return Ok(()),
        // While a press is held its control lists the release, so the
        // press's own gate is checked here.
        HardwareAction::JogPress { .. } => view.jog_enabled,
        _ => true,
    };
    if !enabled {
        // A press refused for its own gate reads as its control's reason
        // (`panel::control_list`), as before the gate moved here.
        if let HardwareAction::JogPress { direction } = action {
            let id = match direction {
                Direction::Upper => "hardware:jog_upper",
                Direction::Lower => "hardware:jog_lower",
            };
            return Err(format!("{id} is disabled: {}", view.blocked.unwrap_or_else(|| "no motor is ready: select one first".into())));
        }
        return Err(view.blocked.unwrap_or_else(|| "this calibration input is disabled in the current session".into()));
    }
    match super::panel::controls(hw).into_iter().find(|(_, _, a, _)| a == action) {
        Some((id, _, _, Err(why))) => Err(format!("{id} is disabled: {why}")),
        _ => Ok(()),
    }
}

/// Queue a page handler on the link (refused without one). A remote motion
/// command ([`dispatch`] gave it a ticket) is queued as
/// [`LinkCommand::Checked`], carrying the shared epoch now (a later STOP
/// refuses it) and the generation it was authorized for.
fn send(hw: &mut Hardware, command: LinkCommand) -> Answer {
    send_with(hw, None, command)
}

/// [`send`], with the form values a remote command brings (a speed, PWM
/// ceiling, hold-others or drive mode; the last two as a bare
/// [`LinkCommand::Inputs`], which only needs authorization): the link thread adopts them only once it has authorized and
/// validated the command, and keeps them only if it succeeds; the form takes
/// them when the ticket resolves ([`apply_resolved`]). Only a ticketed
/// (remote motion) command carries them.
fn send_with(hw: &mut Hardware, inputs: Option<super::link::Inputs>, command: LinkCommand) -> Answer {
    match hw.link.as_ref() {
        Some(link) => {
            if let Some(ticket) = hw.active_ticket {
                hw.queued_ticket = true;
                let epoch = link.epoch.load(std::sync::atomic::Ordering::SeqCst);
                if let Err(error) = link.try_send(LinkCommand::Checked { ticket, epoch, generation: hw.generation, inputs, command: Box::new(command) }) {
                    hw.queued_ticket = false;
                    return Answer::Done(Err(error));
                }
            } else { link.send(command); }
            done()
        }
        None => Answer::Done(Err(NOT_CONNECTED.into())),
    }
}

/// Tell the link the form's inputs changed (the page reads them when it sends).
pub(super) fn inputs_changed(hw: &Hardware) {
    if let Some(link) = hw.link.as_ref() {
        link.send(LinkCommand::Inputs(hw.form.inputs.clone()));
    }
}

/// A slider value in `lo..=hi`, in steps of `step` (the page's range input).
fn stepped(value: f64, lo: f64, hi: f64, step: f64, name: &str) -> Result<f64, String> {
    if !value.is_finite() || !(lo..=hi).contains(&value) {
        return Err(format!("{name} must be a number from {lo} to {hi}"));
    }
    Ok(((value / step).round() * step * 10.0).round() / 10.0)
}

/// The page's `loss()` (focus loss, panel close, leaving). Live motor sync
/// stops for every reason (its Stop button hides with the panel).
///
/// **Deliberate safety difference from the page.** The page's `loss()`
/// stops only when a motor is ready, starting or in a session
/// (`if(ready||starting||run!=null)stop()`), so a Leg/Both gait (`ready`
/// is false after `gait_start`), a tune, a campaign or a sweep-all keeps
/// driving after the tab is hidden. Here any drive ([`link::drive_active`]:
/// also busy, sweep-all, tune, campaign and a leg gait) stops at once on the
/// immediate path; otherwise the link re-checks with the same predicate on
/// its newer state (`LinkCommand::Loss`). The newest published snapshot is
/// read, not the frame's copy in `hw.snapshot`, so a change published since
/// JobResults is seen.
///
/// `Loss::Leaving` (the window is closing: the page's `pagehide`) always
/// stops immediately. Only from the window's own close request (`origin`
/// `Origin::Quiet`, written by `actions::window_loss`) does it also write
/// STOP synchronously, because the detached STOP job may die with the
/// process: the calibration server's ([`link::Link::post_stop_sync`]) and
/// the motor bench's (`LiveSync::post_stop_on_leave`), each blocking the UI
/// thread for at most about 1 s, once, as the window closes. From REST and
/// `system_ui` a `Leaving` loss is refused by [`handle`] (nothing is closing;
/// automation has `hardware_stop`), so automation can never block the UI thread here.
fn loss(hw: &mut Hardware, reason: Loss, origin: Origin) {
    // Only a live sync this viewer opened; another client's session is not ours to end on a loss.
    hw.sync.stop_ours(match reason {
        Loss::PanelClosed => "Panel closed",
        Loss::FocusLost => "Window lost focus",
        Loss::Leaving => "Page closed",
    });
    let active = hw.link.as_ref().is_some_and(|l| link::drive_active(&l.snapshot()));
    if reason == Loss::Leaving || active {
        stop_immediate(hw);
        if reason == Loss::Leaving && origin == Origin::Quiet {
            if let Some(link) = hw.link.as_ref() {
                link.post_stop_sync("window close");
            }
            hw.sync.post_stop_on_leave();
        }
        return;
    }
    release_holds(hw);
    if let Some(link) = hw.link.as_ref() {
        link.send(LinkCommand::Loss);
    }
}

/// × and the header toggle closing the panel: drive stops (the mirror ends as `open` goes false).
fn close(hw: &mut Hardware) {
    loss(hw, Loss::PanelClosed, Origin::Ui);
    hw.open = false;
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Shared authoritative dispatch: both REST and system_ui wait for the session's result.
pub(crate) fn dispatch(hw: &mut Hardware, action: &HardwareAction, call: &mut Call, now: Instant,
    view: Option<&crate::robot::RobotView>, run: &mut dyn FnMut(crate::robot::RobotAction)) -> Answer {
    if let Some(ticket) = call.continuation.get("hardware_ticket").and_then(Value::as_u64) {
        if call.continuation.get("generation").and_then(Value::as_u64) != Some(hw.generation) {
            return Answer::Done(Err("hardware connection replaced while the command was pending".into()));
        }
        let s = hw.link.as_ref().map(|link| link.snapshot()).unwrap_or_default();
        // A recorded verdict is the answer, also after a cancel or the
        // deadline: the command already ran (or was refused) and nothing is
        // left to stop for it.
        if let Some(result) = s.command_results.get(&ticket).cloned() {
            apply_resolved(hw, action, &result, call.continuation);
            settle_presses(hw, &s.command_results);
            // This frame's link state, not the frame's earlier copy: a
            // successful select answers ready, with its motor.
            hw.snapshot = s;
            return Answer::Done(result.map(|()| Some(super::view::status_json(hw, now))));
        }
        if call.cancelled {
            stop_immediate(hw);
            resync_unresolved_inputs(hw, action);
            return Answer::Done(Err("hardware action cancelled; STOP requested".into()));
        }
        let expired = call.continuation.get("deadline_ms").and_then(Value::as_u64).is_some_and(|deadline| unix_ms() > deadline);
        // Stale while the link thread waits on a request that may still
        // answer (this command's, or one queued before it) is not lost.
        if expired || (s.stale(now) && !s.awaiting_answer(now)) {
            stop_immediate(hw);
            resync_unresolved_inputs(hw, action);
            return Answer::Done(Err("hardware command lost its fresh connection or deadline; STOP requested".into()));
        }
        if s.command_results.keys().next().is_some_and(|first| ticket < *first) {
            resync_unresolved_inputs(hw, action);
            // A press whose verdict was evicted may be moving: STOP.
            settle_presses(hw, &s.command_results);
            return Answer::Done(Err("hardware command acknowledgement expired".into()));
        }
        return Answer::Pending;
    }
    if call.remote() {
        if let Err(why) = action.authorize(hw, now).and_then(|()| remote_check(hw, action, call)) {
            return Answer::Done(Err(why));
        }
        hw.command_seq += 1;
        hw.active_ticket = action.starts_motion().then_some(hw.command_seq);
    }
    hw.queued_ticket = false;
    let answer = handle_inner(hw, action, call, now, view, run);
    hw.active_ticket = None;
    if hw.queued_ticket && call.rest() {
        // The form's values now: a remote speed, PWM ceiling, hold-others or
        // drive mode is adopted only if the operator has not changed it
        // since ([`apply_resolved`]).
        *call.continuation = json!({"hardware_ticket": hw.command_seq, "generation": hw.generation, "deadline_ms": unix_ms() + 45_000,
            "speed_reset": hw.form.inputs.speed_reset, "form_speed": hw.form.inputs.speed_percent, "form_pwm": hw.form.inputs.pwm_percent,
            "form_hold_others": hw.form.inputs.hold_others, "form_drive_mode": hw.form.inputs.drive_mode.wire()});
        Answer::Pending
    } else { answer }
}

/// A remote speed, PWM ceiling, hold-others or drive mode the link thread
/// accepted (or applied before a STOP,
/// [`super::session::APPLIED_THEN_STOPPED`]) becomes the form's value (and,
/// for hold-others and drive mode, the remembered preference), and is sent
/// back as the form's inputs so a later form edit
/// keeps it, but only while the form still holds the value it had when the
/// command was queued: a newer operator edit (sent to the link after the
/// command, so already the link's value) wins, and nothing is sent. A speed
/// is also not put back over a speed reset (a sweep started) since then.
fn apply_resolved(hw: &mut Hardware, action: &HardwareAction, result: &Result<(), String>, continuation: &Value) {
    let applied = match result {
        Ok(()) => true,
        Err(e) => e.starts_with(super::session::APPLIED_THEN_STOPPED),
    };
    if !applied {
        return;
    }
    match action {
        HardwareAction::Speed { percent } => {
            let reset_since = continuation.get("speed_reset").and_then(Value::as_u64).is_none_or(|queued| hw.form.inputs.speed_reset > queued);
            let unchanged = continuation.get("form_speed").and_then(Value::as_f64) == Some(hw.form.inputs.speed_percent);
            if let (Ok(v), false, true) = (stepped(*percent, 0.0, 100.0, 1.0, "movement speed"), reset_since, unchanged) {
                hw.form.inputs.speed_percent = v;
                inputs_changed(hw);
            }
        }
        HardwareAction::PwmCeiling { percent } => {
            if continuation.get("form_pwm").and_then(Value::as_f64) == Some(hw.form.inputs.pwm_percent) {
                hw.form.inputs.pwm_percent = (percent * 10.0).round() / 10.0;
                inputs_changed(hw);
            }
        }
        // The preference is set with the form; `actions::apply` claims it
        // when this Done(Ok) is answered.
        HardwareAction::HoldOthers { on } => {
            if continuation.get("form_hold_others").and_then(Value::as_bool) == Some(hw.form.inputs.hold_others) {
                hw.form.inputs.hold_others = *on;
                hw.settings.calibration.hold_others = Some(*on);
                inputs_changed(hw);
            }
        }
        HardwareAction::DriveMode { mode } => {
            if continuation.get("form_drive_mode").and_then(Value::as_str) == Some(hw.form.inputs.drive_mode.wire()) {
                hw.form.inputs.drive_mode = *mode;
                hw.settings.calibration.drive_mode = Some(*mode);
                inputs_changed(hw);
            }
        }
        _ => {}
    }
}

/// Remote jog presses whose verdict the link recorded: a refused one (not
/// applied, as [`apply_resolved`] counts it) puts its direction's held flag
/// back to what it was before the press ([`super::PendingPress::before`]),
/// unless a newer press of that direction (the operator's) took the flag
/// since. Not simply cleared: an operator already holding that direction
/// keeps the hold, so their release still reaches the link (a cleared flag
/// would swallow it and leave the motor moving). Not left set either, else
/// the button and Q/A would keep offering a release for a press that never
/// moved, and the next press would be taken as both keys. A refused press
/// whose `before` was a still-pending remote press of the same direction
/// hands its own `before` on to it, so a chain of refused presses ends at
/// the flag the first one found. A refused one-way press
/// (`Origin::SystemUi`, nobody waits on it) is shown in the panel's notice.
/// A press of an older link generation is dropped (the reconnect stopped
/// and cleared it). One whose verdict was evicted unread (the link keeps 256)
/// may be moving: if it still holds its direction, STOP on the immediate path.
/// Called with each resolving REST call ([`dispatch`]) and each frame
/// (`actions::poll_jobs`) for presses nobody waits on.
pub(super) fn settle_presses(hw: &mut Hardware, results: &std::collections::BTreeMap<u64, Result<(), String>>) {
    if hw.pending_presses.is_empty() {
        return;
    }
    let first = results.keys().next().copied();
    let mut unknown = false;
    // Per direction: the last refused press settled here, as (its press
    // number, its `before`), for the next pending press to inherit.
    let mut refused: [Option<(u64, bool)>; 2] = [None, None];
    for mut press in std::mem::take(&mut hw.pending_presses) {
        if press.generation != hw.generation {
            continue;
        }
        let index = direction_index(press.direction);
        // Pending presses are kept in queue order, so an earlier refused
        // press of this direction was settled just before (this call or an
        // earlier one, which already handed its `before` on).
        if let Some((number, before)) = refused[index] && number + 1 == press.press {
            press.before = before;
        }
        let latest = hw.jog_presses[index] == press.press;
        match results.get(&press.ticket) {
            Some(result) => {
                let applied = match result {
                    Ok(()) => true,
                    Err(e) => e.starts_with(super::session::APPLIED_THEN_STOPPED),
                };
                if !applied {
                    refused[index] = Some((press.press, press.before));
                    if latest {
                        *held_flag(hw, press.direction) = press.before;
                    }
                    if press.one_way && let Err(e) = result {
                        let name = match press.direction {
                            Direction::Upper => "upper",
                            Direction::Lower => "lower",
                        };
                        hw.notice = Some(format!("jog {name} press refused: {e}"));
                    }
                }
            }
            None if first.is_some_and(|first| press.ticket < first) => unknown |= latest && *held_flag(hw, press.direction),
            None => hw.pending_presses.push(press),
        }
    }
    if unknown {
        stop_immediate(hw);
    }
}

/// A STOP or loss ends every hold: both held flags clear, and so does what
/// a pending remote press would put back if refused
/// ([`super::PendingPress::before`]); else a press refused by that STOP
/// would restore a hold the STOP ended.
pub(super) fn release_holds(hw: &mut Hardware) {
    hw.form.held_upper = false;
    hw.form.held_lower = false;
    for press in &mut hw.pending_presses {
        press.before = false;
    }
}

fn direction_index(direction: Direction) -> usize {
    match direction {
        Direction::Upper => 0,
        Direction::Lower => 1,
    }
}

fn held_flag(hw: &mut Hardware, direction: Direction) -> &mut bool {
    match direction {
        Direction::Upper => &mut hw.form.held_upper,
        Direction::Lower => &mut hw.form.held_lower,
    }
}

/// A remote command that brought form values ended without a recorded
/// verdict (cancelled, deadline, connection lost, receipt evicted): the link
/// thread may still have taken them just before the STOP, so the form's
/// values (unchanged) are sent after it, FIFO, and the link ends with what the
/// form shows.
fn resync_unresolved_inputs(hw: &Hardware, action: &HardwareAction) {
    if matches!(action, HardwareAction::Speed { .. } | HardwareAction::PwmCeiling { .. } | HardwareAction::HoldOthers { .. } | HardwareAction::DriveMode { .. }) {
        inputs_changed(hw);
    }
}

pub(super) use dispatch as handle;

/// The handlers, one per intent.
fn handle_inner(hw: &mut Hardware, action: &HardwareAction, call: &mut Call, now: Instant, view: Option<&crate::robot::RobotView>, run: &mut dyn FnMut(crate::robot::RobotAction)) -> Answer {
    use HardwareAction as H;
    match action {
        H::TogglePanel => {
            if hw.open {
                close(hw);
            } else {
                hw.open = true;
            }
            done()
        }
        H::ClosePanel => {
            close(hw);
            done()
        }
        H::Connect => {
            if hw.connecting.is_some() {
                return Answer::Done(Err(format!("already connecting to {}", hw.url())));
            }
            connect(hw);
            done()
        }
        H::ToggleSection { section } => {
            if !hw.form.open.remove(section) {
                hw.form.open.insert(*section);
                // The page loads the gait list when Gait playback opens without one (:272).
                if *section == Section::Gait && hw.snapshot.gaits.is_empty() && let Some(link) = hw.link.as_ref() {
                    link.send(LinkCommand::LoadGaits);
                }
            }
            done()
        }
        H::Status => Answer::Done(Ok(Some(super::view::status_json(hw, now)))),
        H::Stop => {
            // A person's STOP: always posted, id-less when no motor is known.
            operator_stop(hw);
            hw.sync.stop("Operator stop");
            done()
        }
        H::Loss { reason: Loss::Leaving } if call.remote() => Answer::Done(Err(
            "hardware `loss` leaving is the window closing and is not accepted from REST or system_ui (it would block the UI thread on a synchronous STOP); use hardware_stop".into(),
        )),
        H::Loss { reason } => {
            loss(hw, *reason, call.origin);
            done()
        }
        H::Export => export(hw, call),
        H::LoadGaits => load_gaits(hw, call),
        H::Select { id } => send(hw, LinkCommand::Select { id: *id }),
        H::SetDisabled => send(hw, LinkCommand::SetDisabled),
        H::SweepAll => send(hw, LinkCommand::SweepAll),
        H::HoldOthers { on } => {
            // Remote: the form and the preference change only once the link
            // thread accepts it ([`apply_resolved`]).
            if hw.active_ticket.is_some() && call.rest() {
                let mut inputs = hw.form.inputs.clone();
                inputs.hold_others = *on;
                return send_with(hw, Some(inputs.clone()), LinkCommand::Inputs(inputs));
            }
            hw.form.inputs.hold_others = *on;
            inputs_changed(hw);
            hw.settings.calibration.hold_others = Some(*on);
            done()
        }
        H::JogPress { direction } => {
            // The page's `move()`: nothing unless a motor is ready and not connecting.
            let Some(link) = hw.link.as_ref() else { return Answer::Done(Err(NOT_CONNECTED.into())) };
            let s = link.snapshot();
            if !s.ready || s.busy {
                return Answer::Done(Err("no motor is ready: select one first".into()));
            }
            let other = match direction {
                Direction::Upper => hw.form.held_lower,
                Direction::Lower => hw.form.held_upper,
            };
            let before = std::mem::replace(held_flag(hw, *direction), true);
            // Both Q and A held: hold (`keys.size > 1`).
            let answer = send(hw, if other { LinkCommand::BothKeys } else { LinkCommand::Press { direction: *direction } });
            if matches!(answer, Answer::Done(Err(_))) {
                // Never queued: nothing moves for it, and it is not counted
                // (an older refused press still counts as the latest).
                *held_flag(hw, *direction) = before;
                return answer;
            }
            let index = direction_index(*direction);
            hw.jog_presses[index] += 1;
            if hw.queued_ticket && let Some(ticket) = hw.active_ticket {
                // A remote press: it holds only if the link accepts it ([`settle_presses`]).
                let one_way = matches!(call.origin, Origin::SystemUi);
                hw.pending_presses.push(super::PendingPress { ticket, generation: hw.generation, direction: *direction, press: hw.jog_presses[index], before, one_way });
            }
            answer
        }
        H::JogRelease { direction } => {
            // A press still awaiting its verdict must not restore the hold
            // this release ends (`settle_presses` restores `before`).
            for press in hw.pending_presses.iter_mut().filter(|p| p.direction == *direction) {
                press.before = false;
            }
            if !std::mem::take(held_flag(hw, *direction)) {
                return done();
            }
            // A release that cannot reach the link would leave the motor
            // moving: STOP on the immediate path instead.
            let answer = send(hw, LinkCommand::Release);
            if matches!(answer, Answer::Done(Err(_))) {
                stop_immediate(hw);
            }
            answer
        }
        H::Speed { percent } => match stepped(*percent, 0.0, 100.0, 1.0, "movement speed") {
            // Remote: the form changes only once the link thread accepts it.
            Ok(v) if hw.active_ticket.is_some() && call.rest() => {
                let mut inputs = hw.form.inputs.clone();
                inputs.speed_percent = v;
                send_with(hw, Some(inputs), LinkCommand::SpeedChanged)
            }
            Ok(v) => {
                hw.form.inputs.speed_percent = v;
                inputs_changed(hw);
                // Not connected: the form keeps the value for the next link.
                if hw.link.is_none() {
                    return done();
                }
                send(hw, LinkCommand::SpeedChanged)
            }
            Err(e) => Answer::Done(Err(e)),
        },
        H::Target { percent } => match stepped(*percent, 0.0, 100.0, 0.1, "target") {
            Ok(v) => {
                hw.form.target_percent = v;
                send(hw, LinkCommand::Target { fraction: v / 100.0 })
            }
            Err(e) => Answer::Done(Err(e)),
        },
        H::TargetCommit => send(hw, LinkCommand::TargetCommit),
        H::Capture { boundary } => {
            let reference = match (boundary, hw.snapshot.id) {
                (Boundary::Reference, Some(id)) => hw.mirror.alignment_angle(id),
                _ => None,
            };
            send(hw, LinkCommand::Capture { boundary: *boundary, reference_joint_rad: reference })
        }
        H::ResetPoses => send(hw, LinkCommand::ResetPoses),
        H::ClearLower => send(hw, LinkCommand::Clear { boundary: Boundary::Lower }),
        H::ClearUpper => send(hw, LinkCommand::Clear { boundary: Boundary::Upper }),
        H::Sweep => send(hw, LinkCommand::Sweep),
        H::Learn => send(hw, LinkCommand::Learn),
        H::TuneConfirm { on } => {
            hw.form.tune_ok = *on;
            done()
        }
        H::Tune => {
            if !hw.form.tune_ok {
                return Answer::Done(Err(format!("check “{}” first", super::panel::TUNE_OK)));
            }
            send(hw, LinkCommand::Tune)
        }
        H::CampaignConfirm { on } => {
            hw.form.campaign_ok = *on;
            done()
        }
        H::Campaign { resume } => {
            if !hw.form.campaign_ok {
                return Answer::Done(Err(format!("check “{}” first", super::panel::CAMPAIGN_OK)));
            }
            send(hw, LinkCommand::Campaign { resume: *resume })
        }
        H::GaitSelect { index } => {
            if hw.snapshot.gaits_loaded && *index >= hw.snapshot.gaits.len() {
                return Answer::Done(Err(format!("gait {index}: the list has {} gaits", hw.snapshot.gaits.len())));
            }
            hw.form.gait_index = *index;
            done()
        }
        H::GaitMode { mode } => {
            if hw.snapshot.gait.is_some() {
                return Answer::Done(Err("where a gait plays cannot change while it plays".into()));
            }
            hw.form.gait_mode = *mode;
            done()
        }
        H::GaitSpeed { percent } => match stepped(*percent, 5.0, 100.0, 1.0, "playback speed") {
            Ok(v) => {
                hw.form.inputs.gait_speed_percent = v;
                inputs_changed(hw);
                if let Some(link) = hw.link.as_ref() {
                    link.send(LinkCommand::GaitScale);
                }
                done()
            }
            Err(e) => Answer::Done(Err(e)),
        },
        H::GaitEffort { percent } => {
            if hw.snapshot.gait.as_ref().is_some_and(|g| g.leg) {
                return Answer::Done(Err("leg effort cannot change while a gait drives the leg".into()));
            }
            match stepped(*percent, 10.0, 100.0, 1.0, "leg effort") {
                Ok(v) => {
                    hw.form.inputs.gait_effort_percent = v;
                    inputs_changed(hw);
                    done()
                }
                Err(e) => Answer::Done(Err(e)),
            }
        }
        H::GaitConfirm { on } => {
            hw.form.gait_ok = *on;
            done()
        }
        H::GaitPlay => gait_play(hw),
        H::GaitStop => {
            // A gait on the leg stops on the immediate path first (the queued
            // link may be waiting on a slow request); the link then ends the run.
            if hw.snapshot.gait.as_ref().is_some_and(|g| g.leg) || hw.link.as_ref().and_then(|l| l.snapshot().gait).is_some_and(|g| g.leg) {
                stop_immediate(hw);
            }
            send(hw, LinkCommand::GaitStop)
        }
        H::DriveMode { mode } => {
            // Remote: the form and the preference change only once the link
            // thread accepts it ([`apply_resolved`]).
            if hw.active_ticket.is_some() && call.rest() {
                let mut inputs = hw.form.inputs.clone();
                inputs.drive_mode = *mode;
                return send_with(hw, Some(inputs.clone()), LinkCommand::Inputs(inputs));
            }
            hw.form.inputs.drive_mode = *mode;
            inputs_changed(hw);
            hw.settings.calibration.drive_mode = Some(*mode);
            done()
        }
        H::PwmCeiling { percent } => {
            // The page's number input: 0–100 in steps of 0.1, else not sent (reportValidity).
            if !percent.is_finite() || !(0.0..=100.0).contains(percent) {
                return Answer::Done(Err("PWM ceiling (%) must be from 0 to 100".into()));
            }
            let pwm = (percent * 10.0).round() / 10.0;
            // Remote: the form changes only once the link thread accepts it.
            if hw.active_ticket.is_some() && call.rest() {
                let mut inputs = hw.form.inputs.clone();
                inputs.pwm_percent = pwm;
                return send_with(hw, Some(inputs), LinkCommand::PwmChanged);
            }
            hw.form.inputs.pwm_percent = pwm;
            inputs_changed(hw);
            // Not connected: the form keeps the value for the next link.
            if hw.link.is_none() {
                return done();
            }
            send(hw, LinkCommand::PwmChanged)
        }
        H::Flip => send(hw, LinkCommand::Flip),
        H::RawStepValue { delta } => {
            if !(-4095..=4095).contains(delta) {
                return Answer::Done(Err(format!("single raw step {delta}: must be from −4095 to 4095")));
            }
            hw.form.step = *delta;
            done()
        }
        H::RawStep => {
            let step = hw.form.step;
            if step == 0 || !(-4095..=4095).contains(&step) {
                return Answer::Done(Err("single raw step must be a whole number from −4095 to 4095, not 0".into()));
            }
            send(hw, LinkCommand::RawStep { delta: step as i16 })
        }
        H::MirrorEnabled { .. } | H::MirrorLeg { .. } | H::MirrorJoint { .. } | H::MirrorPolarity { .. } | H::MirrorAlign { .. } => Answer::Done(super::mirror::apply(hw, action).map(|()| None)),
        H::SyncConnect | H::SyncLeg { .. } | H::SyncMotor { .. } | H::SyncPolarity { .. } | H::SyncScale { .. } | H::SyncStart | H::SyncStop => Answer::Done(super::sync::apply(hw, action, view, run).map(|()| None)),
    }
}

/// "Play": with a gait playing, pause or resume it; else the selected gait
/// in the chosen place, with the mirror's bindings for Leg and Both.
fn gait_play(hw: &mut Hardware) -> Answer {
    let s = &hw.snapshot;
    if s.gait.is_some() {
        return send(hw, LinkCommand::GaitToggle);
    }
    let Some(entry) = s.gaits.get(hw.form.gait_index).cloned() else {
        return Answer::Done(Err(if s.gaits.is_empty() { "No gaits found yet.".into() } else { "choose a gait".into() }));
    };
    let mode = hw.form.gait_mode;
    if mode != GaitMode::Sim && !hw.form.gait_ok {
        return Answer::Done(Err(format!("Leg and Both need “{}” checked", super::panel::GAIT_OK)));
    }
    if s.tuning || s.campaigning {
        return Answer::Done(Err("not while a tune or campaign runs".into()));
    }
    let (bindings, skipped) = if mode == GaitMode::Sim { (Vec::new(), Vec::new()) } else { hw.mirror.gait_bindings(&s.state) };
    send(hw, LinkCommand::GaitPlay { entry, mode, bindings, skipped })
}

/// Download calibration. A click starts the job (one at a time); REST
/// `hardware_export` starts it (or joins the running one) and answers
/// Pending until it has written its file, then `{"path": …, "simulated":
/// bool}` ([`Exported`]).
fn export(hw: &mut Hardware, call: &mut Call) -> Answer {
    if let Some(seq) = call.continuation.get("export").and_then(Value::as_u64) {
        return match &hw.export_done {
            Some((done, result)) if *done == seq => Answer::Done(result.clone().map(|e| Some(json!({"path": e.path.display().to_string(), "simulated": e.simulated})))),
            _ if hw.export.as_ref().is_some_and(|j| j.generation() == seq) => {
                if call.cancelled {
                    Answer::Done(Err("hardware_export: cancelled (the file is still written)".into()))
                } else {
                    Answer::Pending
                }
            }
            _ => Answer::Done(Err("hardware_export: the result was replaced by a newer export".into())),
        };
    }
    let running = hw.export.as_ref().map(|job| job.generation());
    let seq = match running {
        Some(seq) if call.rest() => seq,
        Some(_) => return Answer::Done(Err("a download is already being written".into())),
        None => match start_export(hw) {
            Ok(seq) => seq,
            Err(e) => return Answer::Done(Err(e)),
        },
    };
    if call.rest() {
        *call.continuation = json!({"export": seq});
        return Answer::Pending;
    }
    done()
}

/// The export line's label for a download from a virtual calibration bench.
pub(super) const VIRTUAL_EXPORT: &str = "VIRTUAL (simulated)";

/// A written calibration download ([`write_export`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Exported {
    pub path: PathBuf,
    /// The document is labelled simulated (`"simulated": true`, a
    /// `-virtual` file name): from a pinned virtual link, or a server that
    /// labels its own export as virtual.
    pub simulated: bool,
}

fn start_export(hw: &mut Hardware) -> Result<u64, String> {
    let client = hw.link.as_ref().ok_or(NOT_CONNECTED)?.client.clone();
    // The mirror's binding as it is at the click (the page's `mirror?.record()`).
    let mirror = hw.mirror.record();
    let output = hw.snapshot.state.output.clone();
    // A virtual bench's document is labelled with its execution: simulated
    // results must never pass for physical measurements.
    let execution = match hw.snapshot.execution.as_ref().filter(|i| i.is_virtual_calibration()) {
        Some(identity) => Some(serde_json::to_value(identity).map_err(|e| format!("virtual execution identity: {e}"))?),
        None => None,
    };
    hw.export_seq += 1;
    let seq = hw.export_seq;
    hw.export_virtual = execution.is_some();
    hw.export_line = Some(if hw.export_virtual { format!("{VIRTUAL_EXPORT} Downloading calibration…") } else { "Downloading calibration…".into() });
    hw.export = Some(
        Job::spawn(Pool::Dedicated, seq, "calibration export", move |_| {
            let export = client.get("/calibration/export").map_err(|e| e.to_string())?;
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            write_export(export, mirror, output.as_deref(), stamp, execution)
        })
        .complete_on_drop(),
    );
    Ok(seq)
}

/// Writes `{...export, display_mirror}` (the page's object; its keys in
/// serde_json's order) as 2-space JSON to
/// `<output>/viewer-exports/leg-calibration-<unix_ms>.json`, never
/// overwriting. A relative `output` (the server's working directory) is
/// taken from the workspace root, where the servers are started.
///
/// From a virtual calibration bench (`execution`: the link's pinned
/// identity as JSON) the document also carries `"execution"` and
/// `"simulated": true`, and the file is
/// `leg-calibration-<unix_ms>-virtual.json`. A document that names another
/// execution is refused rather than relabelled (the same one is fine).
/// Without a pin, a document the server labelled itself (`"simulated":
/// true`, or an `execution` of kind `virtual_calibration`) keeps its keys,
/// gains `"simulated": true` and is written as `-virtual` too: a simulated
/// result must never pass for a physical measurement.
pub(crate) fn write_export(export: Value, mirror: Value, output: Option<&str>, unix_ms: u128, execution: Option<Value>) -> Result<Exported, String> {
    let mut doc = match export {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    if !mirror.is_null() {
        doc.insert("display_mirror".into(), mirror);
    }
    let labelled_virtual = doc.get("simulated") == Some(&Value::Bool(true))
        || doc.get("execution").and_then(|e| e.get("kind")).and_then(Value::as_str) == Some("virtual_calibration");
    let simulated = execution.is_some() || labelled_virtual;
    if let Some(execution) = execution {
        if doc.get("execution").is_some_and(|own| !own.is_null() && *own != execution) {
            return Err("the exported calibration names another execution than this virtual link; reconnect and download again".into());
        }
        doc.insert("execution".into(), execution);
    }
    if simulated {
        doc.insert("simulated".into(), Value::Bool(true));
    }
    let output = output.filter(|o| !o.is_empty()).ok_or("the calibration server has not reported its output directory")?;
    let base = Path::new(output);
    let base = if base.is_relative() { crate::workspace::path(base)? } else { base.to_path_buf() };
    let dir = base.join("viewer-exports");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("leg-calibration-{unix_ms}{}.json", if simulated { "-virtual" } else { "" }));
    let text = serde_json::to_string_pretty(&Value::Object(doc)).map_err(|e| e.to_string())?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Exported { path, simulated })
}

/// The gait list as REST `hardware_gaits` answers it.
fn gaits_json(hw: &Hardware) -> Value {
    let gaits: Vec<Value> = hw
        .snapshot
        .gaits
        .iter()
        .enumerate()
        .map(|(i, g)| json!({"index": i, "label": super::view::gait_option_label(g), "path": g.path, "kind": g.kind, "study": g.study, "trial": g.trial, "speed_m_s": g.speed_m_s, "measured_actuators": g.measured_actuators, "summary": g.summary}))
        .collect();
    json!({"gaits": gaits, "selected": hw.form.gait_index})
}

/// The gait list: a click (opening Gait playback) asks the link; REST
/// `hardware_gaits` answers the loaded list, loading it first if needed.
fn load_gaits(hw: &mut Hardware, call: &mut Call) -> Answer {
    let Some(link) = hw.link.as_ref() else { return Answer::Done(Err(NOT_CONNECTED.into())) };
    let s = &hw.snapshot;
    if !call.rest() {
        link.send(LinkCommand::LoadGaits);
        return done();
    }
    if s.gaits_loaded {
        return Answer::Done(Ok(Some(gaits_json(hw))));
    }
    let Some(since) = call.continuation.get("since").and_then(Value::as_u64) else {
        link.send(LinkCommand::LoadGaits);
        *call.continuation = json!({"since": s.revision, "generation": s.generation});
        return Answer::Pending;
    };
    if call.continuation.get("generation").and_then(Value::as_u64) != Some(s.generation) && s.generation != 0 {
        return Answer::Done(Err("the calibration server's link was replaced while the gait list loaded".into()));
    }
    match &s.gait_notice {
        Some(notice) if s.revision > since && notice.starts_with("Gait list unavailable") => Answer::Done(Err(notice.clone())),
        _ if call.cancelled => Answer::Done(Err("hardware_gaits: cancelled".into())),
        _ => Answer::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepped_values_follow_the_page_inputs() {
        assert_eq!(stepped(33.4, 0.0, 100.0, 1.0, "speed"), Ok(33.0));
        assert!(stepped(140.0, 5.0, 100.0, 1.0, "speed").is_err());
        assert_eq!(stepped(12.34, 0.0, 100.0, 0.1, "target"), Ok(12.3));
        assert!(stepped(f64::NAN, 0.0, 100.0, 1.0, "speed").is_err());
    }

    #[test]
    fn exports_are_new_files_with_the_mirror_binding() {
        let dir = std::env::temp_dir().join(format!("sim-spatial-hardware-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = dir.to_string_lossy().into_owned();
        let Exported { path, simulated } = write_export(json!({"fixture": "leg", "axes": {}}), json!({"leg": "+X"}), Some(&out), 1234, None).unwrap();
        assert_eq!(path, dir.join("viewer-exports").join("leg-calibration-1234.json"));
        assert!(!simulated);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("{\n  \""), "2-space JSON: {text}");
        let written: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(written, json!({"fixture": "leg", "axes": {}, "display_mirror": {"leg": "+X"}}));
        // Never overwritten.
        assert!(write_export(json!({}), Value::Null, Some(&out), 1234, None).is_err());
        // No mirror: no key, as JSON.stringify drops undefined.
        let path = write_export(json!({"a": 1}), Value::Null, Some(&out), 5678, None).unwrap().path;
        assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap(), json!({"a": 1}));
        assert!(write_export(json!({}), Value::Null, None, 1, None).is_err());
        // A virtual bench's export: labelled, with its own file name.
        let identity = json!({"schema_version": 1, "kind": "virtual_calibration", "server_instance": "s", "bench_instance": "b"});
        let written = write_export(json!({"a": 1}), Value::Null, Some(&out), 5678, Some(identity.clone())).unwrap();
        assert_eq!(written, Exported { path: dir.join("viewer-exports").join("leg-calibration-5678-virtual.json"), simulated: true });
        assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(written.path).unwrap()).unwrap(), json!({"a": 1, "execution": identity, "simulated": true}));
        // The server's own label naming the same execution is no conflict.
        let written = write_export(json!({"a": 2, "execution": identity, "simulated": true}), Value::Null, Some(&out), 6789, Some(identity.clone())).unwrap();
        assert!(written.simulated);
        assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(written.path).unwrap()).unwrap(), json!({"a": 2, "execution": identity, "simulated": true}));
        // A document naming another execution is not relabelled.
        assert!(write_export(json!({"execution": {"kind": "physical"}}), Value::Null, Some(&out), 9999, Some(identity)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
