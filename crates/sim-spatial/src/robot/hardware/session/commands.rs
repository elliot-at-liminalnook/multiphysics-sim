//! The session's commands: each [`LinkCommand`] handled in order, the
//! inputs it carries, remote (checked) commands and their validation.
use super::*;

impl Session {
    // ---- commands ----

    pub(in crate::robot::hardware) fn handle(&mut self, command: LinkCommand) {
        match command {
            LinkCommand::Checked { ticket, epoch, generation, inputs, command } => {
                self.snap.authorization_revoked |= self.authorization.load(SeqCst);
                self.command_error = None;
                let is_tune = matches!(&*command, LinkCommand::Tune);
                let is_campaign = matches!(&*command, LinkCommand::Campaign { .. });
                let (tune_done, campaign_done) = (self.snap.tune_done, self.snap.campaign_done);
                // The generation the UI authorized it for must still be this session's.
                let authorization = calibration::authorize_virtual(self.snap.execution.as_ref(), generation, self.snap.generation,
                    self.snap.connection_valid && self.snap.state.connected && !self.snap.authorization_revoked && self.snap.disconnected.is_none(), !self.snap.stale(Instant::now()));
                let result = if matches!(&*command, LinkCommand::Release) {
                    self.checked_release(epoch, authorization)
                } else if self.stopped_since(epoch) {
                    Err(STOP_PENDING.into())
                } else if let Err(error) = authorization {
                    Err(error)
                } else if let Err(error) = self.validate_command(&command) {
                    Err(error)
                } else {
                    let applies = applied_change(&command);
                    let previous = inputs.map(|inputs| {
                        let previous = self.inputs.clone();
                        self.set_inputs(inputs);
                        previous
                    });
                    let outcome = self.run_checked(*command);
                    // A UI STOP pressed while it ran ended it (its own bumps
                    // don't count); a change it already saved is reported as such.
                    let result = match (outcome, applies) {
                        (Ok(()), Some(what)) if self.stopped_since(epoch) => Err(format!("{APPLIED_THEN_STOPPED}: {what} was saved; motion was stopped")),
                        (_, _) if self.stopped_since(epoch) => Err(STOP_PENDING.into()),
                        (outcome, _) => outcome,
                    };
                    // Form values the command brought stay only with a change that holds.
                    if let (Some(previous), Err(e)) = (previous, &result)
                        && !e.starts_with(APPLIED_THEN_STOPPED)
                    {
                        self.inputs = previous;
                    }
                    result
                };
                // Only where the sequence did not already count its end (the box unchecks once).
                if result.is_err() {
                    if is_tune && self.snap.tune_done == tune_done { self.snap.tune_done += 1; }
                    if is_campaign && self.snap.campaign_done == campaign_done { self.snap.campaign_done += 1; }
                }
                self.snap.command_results.insert(ticket, result);
                // Bound receipts; tickets are monotonic, consumers fail closed if evicted.
                while self.snap.command_results.len() > 256 {
                    self.snap.command_results.pop_first();
                }
                self.publish();
            }
            LinkCommand::Inputs(inputs) => self.set_inputs(inputs),
            // The motor chips :157.
            LinkCommand::Select { id } => {
                if self.sweep_all.is_some() {
                    self.snap.sequence_text = "Sweep-all stopped: another motor was chosen.".into();
                }
                self.select_motor(id, false, false);
            }
            LinkCommand::SetDisabled => self.set_disabled(),
            LinkCommand::SweepAll => self.sweep_all(),
            LinkCommand::Press { direction } => self.move_(direction),
            LinkCommand::Release => self.release(),
            // Q and A both held :294.
            LinkCommand::BothKeys => {
                if self.snap.ready {
                    self.snap.intent = Intent::Hold;
                    self.update();
                    self.render();
                }
            }
            // :297.
            LinkCommand::SpeedChanged => {
                self.update();
                self.render();
            }
            LinkCommand::PwmChanged => {
                if self.pwm_valid() {
                    self.update();
                }
            }
            LinkCommand::Target { fraction } => self.target(fraction),
            // :300.
            LinkCommand::TargetCommit => self.update(),
            LinkCommand::Capture { boundary, reference_joint_rad } => self.capture(boundary, reference_joint_rad),
            // :306.
            LinkCommand::ResetPoses => {
                let boundary = self.reset_boundary();
                self.reset(boundary);
            }
            LinkCommand::Clear { boundary } => self.reset(boundary.name()),
            LinkCommand::Flip => self.flip(),
            LinkCommand::Sweep => self.sweep(),
            LinkCommand::Learn => self.learn(),
            LinkCommand::RawStep { delta } => self.raw_step(delta),
            LinkCommand::Tune => self.tune(),
            LinkCommand::Campaign { resume } => self.campaign(resume),
            LinkCommand::LoadGaits => self.load_gaits(),
            LinkCommand::GaitPlay { entry, mode, bindings, skipped } => self.gait_play(entry, mode, bindings, skipped),
            LinkCommand::GaitToggle => self.gait_toggle(),
            LinkCommand::GaitStop => self.gait_stop(),
            LinkCommand::GaitScale => self.gait_scale(),
            LinkCommand::Stopped { epoch } => {
                self.stopped_locally();
                self.ui_stop_epoch = self.ui_stop_epoch.max(epoch);
                self.stop_applied(epoch);
                self.render();
            }
            LinkCommand::StopAnswered(answer) => {
                match answer.and_then(|v| serde_json::from_value::<Status>(v).map_err(|e| format!("status: {e}"))) {
                    Ok(status) => self.adopt(status),
                    Err(e) => self.message(e),
                }
                self.render();
            }
            // `loss()` :321, widened ([`super::link::drive_active`]).
            LinkCommand::Loss => {
                if self.drive_active() {
                    self.stop();
                }
            }
        }
        // `Inputs` changes what the heartbeat carries without a publish.
        self.sync_beat();
    }

    /// New form values. Values sent before the UI applied the latest speed
    /// reset keep the session's speed (0 since the reset).
    pub(super) fn set_inputs(&mut self, inputs: Inputs) {
        let speed = self.inputs.speed_percent;
        let stale = inputs.speed_reset < self.snap.speed_reset;
        self.inputs = inputs;
        if stale {
            self.inputs.speed_percent = speed;
        }
    }

    /// A remote release (Q/A or the jog button let go): a move to hold, so
    /// like STOP it is not refused for a stale status, another generation or
    /// a revoked binding. A STOP already pressed has ended the motion. When
    /// the session may not be driven any more (the authorization check
    /// failed), hold cannot be requested through it: STOP instead.
    pub(super) fn checked_release(&mut self, epoch: u64, authorization: Result<(), String>) -> Result<(), String> {
        if self.stopped_since(epoch) {
            return Ok(());
        }
        if authorization.is_err() {
            self.stop();
        } else {
            self.release();
        }
        self.command_error.take().map_or(Ok(()), Err)
    }

    /// "Reset poses"'s boundary :306: the pose the motor is beyond, else both.
    pub(super) fn reset_boundary(&self) -> &'static str {
        match self.outside_pose() {
            Some("reference") | None => "both",
            Some(b) => b,
        }
    }

    /// Runs a remote command and judges it by what it achieved. The page's
    /// handlers decline silently where its disabled controls would have
    /// prevented the click (no motor ready, busy, a select the server did
    /// not honour, …), so the absence of a message is not success: each
    /// command that has a purpose to check is Ok exactly when that purpose
    /// holds afterwards (then also when a step on the way, such as the
    /// `stop()` before a tune, reported an error), else Err with the first
    /// reason recorded or a description of what did not happen. The rest are
    /// Ok unless an error was recorded.
    pub(super) fn run_checked(&mut self, command: LinkCommand) -> Result<(), String> {
        use LinkCommand as C;
        let was_sweeping = self.snap.sweeping || self.snap.intent == Intent::Target;
        let was_learning = self.snap.learning;
        let was_sweeping_all = self.sweep_all.is_some();
        let (was_tuning, was_campaigning) = (self.snap.tuning, self.snap.campaigning);
        let chosen = self.snap.id;
        let was_disabled = chosen.and_then(|id| self.axis_of(id)).map(|a| a.disabled);
        // A gait already playing: Play pauses or resumes it.
        let was_playing = self.snap.gait.as_ref().map(|g| g.playing);
        let clearing = match &command {
            C::Clear { boundary } => Some(boundary.name()),
            C::ResetPoses => Some(self.reset_boundary()),
            _ => None,
        };
        let check = command.clone();
        self.handle(command);
        let (achieved, what): (bool, String) = match check {
            C::Select { id } => (
                self.snap.id == Some(id) && (self.snap.ready || self.axis_of(id).is_some_and(|a| a.disabled)),
                format!("motor {id} was not enabled"),
            ),
            C::Press { direction } => {
                let intent = match direction {
                    Direction::Upper => Intent::Upper,
                    Direction::Lower => Intent::Lower,
                };
                (self.snap.run.is_some() && self.snap.intent == intent, "the hold-to-move session did not start".into())
            }
            C::Target { .. } => (self.snap.run.is_some() && self.snap.intent == Intent::Target, "the move to the target did not start".into()),
            C::Sweep if was_sweeping => (!self.snap.sweeping && self.snap.intent != Intent::Target, "the sweep did not pause".into()),
            C::Sweep => (self.snap.run.is_some() && self.snap.sweeping, "the saved-range sweep did not start".into()),
            C::Learn if was_learning => (!self.snap.learning, "learning did not pause".into()),
            C::Learn => (
                self.snap.run.is_some() && (self.snap.learning || self.snap.learning_terminal.is_some()),
                "learning did not start".into(),
            ),
            C::SweepAll if was_sweeping_all => (self.sweep_all.is_none(), "sweep-all did not stop".into()),
            C::SweepAll => (self.sweep_all.is_some(), format!("sweep-all did not start: {}", self.snap.sequence_text)),
            C::Tune => (!was_tuning && self.snap.tuning, if was_tuning { "a tune is already running".into() } else { "the tune did not start".into() }),
            C::Campaign { .. } => (
                !was_campaigning && self.snap.campaigning,
                if was_campaigning { "a campaign is already running".into() } else { "the campaign did not start".into() },
            ),
            C::SetDisabled => match (chosen, was_disabled) {
                (Some(id), Some(was)) => (self.axis_of(id).is_some_and(|a| a.disabled != was), format!("motor {id} was not {}", if was { "enabled" } else { "disabled" })),
                _ => (false, "no motor with a calibration is selected".into()),
            },
            C::ResetPoses | C::Clear { .. } => {
                let cleared = chosen.and_then(|id| self.axis_of(id)).is_some_and(|a| match clearing {
                    Some("lower") => a.lower.is_none(),
                    Some("upper") => a.upper.is_none(),
                    _ => a.lower.is_none() && a.upper.is_none(),
                });
                (cleared, "the pose was not cleared".into())
            }
            // Pause/resume is judged by the request: for a leg gait the beat
            // carries it to the server with its next `gait_update` (errors
            // ignored, as the page's); a pause the server never hears is still
            // ended by its 1.5 s lease, and the next status shows its phase.
            C::GaitPlay { .. } | C::GaitToggle if was_playing.is_some() => (
                self.snap.gait.as_ref().is_some_and(|g| Some(g.playing) != was_playing),
                if was_playing == Some(true) { "the gait did not pause".into() } else { "the gait did not resume".into() },
            ),
            C::GaitToggle => (false, "no gait is playing".into()),
            // Answered once the gait started: a sim gait is playing on this
            // thread; a leg gait's `gait_start` was answered and adopted with
            // nothing failing on the way. Else the play's own reason.
            C::GaitPlay { mode, .. } => {
                let error = self.command_error.take();
                let started = self.snap.gait.as_ref().is_some_and(|g| g.mode == mode);
                if started && (mode == GaitMode::Sim || error.is_none()) {
                    return Ok(());
                }
                let why = if started { error } else { self.snap.gait_notice.clone().filter(|n| !n.is_empty()).or(error) };
                let why = why.unwrap_or_else(|| "the gait did not start".into());
                return Err(if started { format!("{why} (the gait started on the leg regardless; check the status, or Stop it)") } else { why });
            }
            _ => return self.command_error.take().map_or(Ok(()), Err),
        };
        let error = self.command_error.take();
        if achieved {
            return Ok(());
        }
        Err(error.unwrap_or_else(|| match self.snap.state.message.as_deref().filter(|m| !m.is_empty()) {
            Some(m) => format!("{what} (server: {m})"),
            None => what,
        }))
    }

    /// Validate again at queue consumption against the latest authoritative session.
    pub(super) fn validate_command(&self, command: &LinkCommand) -> Result<(), String> {
        use LinkCommand as C;
        if matches!(command, C::Press { .. } | C::Target { .. } | C::TargetCommit | C::Capture { .. } | C::Sweep | C::Learn | C::Tune)
            && (!self.snap.ready || self.snap.busy) { return Err("motor is no longer ready".into()); }
        if matches!(command, C::Target { .. } | C::Sweep | C::Learn) {
            let axis = self.axis();
            if axis.lower.zip(axis.upper).is_none_or(|(lower, upper)| lower.abs_diff(upper) <= 8)
                || self.outside_pose() == Some("reference") {
                return Err("current encoder session needs two taught poses more than eight counts apart".into());
            }
        }
        if let C::Select { id } = command {
            if !self.snap.state.calibration.as_ref().is_some_and(|c| c.axes.contains_key(id)) {
                return Err(format!("unknown calibration motor {id}"));
            }
        }
        // A new play (not a pause or resume) waits for a tune or campaign to end.
        if matches!(command, C::GaitPlay { .. }) && self.snap.gait.is_none() && (self.snap.tuning || self.snap.campaigning) {
            return Err("not while a tune or campaign runs".into());
        }
        Ok(())
    }
}
