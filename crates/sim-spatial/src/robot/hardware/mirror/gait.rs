//! The mirror following a gait: `setGait` and the playback samples a Leg or
//! Both gait sends the worker.
use super::*;

impl Mirror {
    /// `setGait(pose, realLeg)` (:108).
    pub(super) fn set_gait(&mut self, pose: Option<BTreeMap<String, f64>>, real_leg: bool) {
        self.gait = pose;
        self.gait_real_leg = real_leg;
        let last = self.last.take().unwrap_or_default();
        self.update(&last, true);
    }

    /// The link's gait run (calibration-ui.mjs:241-270): load a compiled gait
    /// played in Sim or Both, sample it once per frame with no sample in
    /// flight (`gaitSampling`), and clear the gait pose when the run ends.
    ///
    /// The clock: Sim samples at the link thread's `run.t` × `run.scale`
    /// (wall time, unchanged). Both samples at `leg_clock`
    /// ([`super::link::LinkSnapshot::leg_clock`] of the same snapshot), the
    /// server's leg gait time interpolated since the last read, so the
    /// simulated legs move with the real one between polls rather than in
    /// 150 ms steps. While that clock is frozen (the leg's data is stale or
    /// lost) no sample is sent and the last gait pose is held; a leg run
    /// with no clock is held the same way. A paused or approaching leg gait
    /// is sampled at its held time (the pose holds).
    ///
    /// The interpolation may run ahead of the server by at most
    /// [`POLL_ACTIVE`] × scale, so the next status can put the time slightly
    /// behind the last sample. Within that bound the sample stays at the
    /// last time (the display waits for the server to catch up instead of
    /// stepping back); a larger step back is the server's own and is taken.
    pub(crate) fn follow_gait(&mut self, run: Option<&GaitRun>, compiled: Option<&(u64, Arc<Value>, String)>, leg_clock: Option<&LegClock>, now: Instant) {
        let Some(run) = run else {
            if self.gait_number.take().is_some() || self.gait.is_some() {
                self.gait_ready = false;
                self.sampled = false;
                self.last_sample = None;
                self.last_t = None;
                self.set_gait(None, true);
            }
            return;
        };
        if run.mode == GaitMode::Leg || self.worker.is_none() {
            return;
        }
        let Some((number, gait, name)) = compiled else { return };
        if self.gait_number != Some(*number) {
            self.gait_number = Some(*number);
            self.gait_ready = false;
            self.sampled = false;
            self.last_sample = None;
            self.last_t = None;
            self.gait_notice = None;
            self.send(MirrorCommand::Gait { number: *number, compiled: gait.clone(), name: name.clone() });
            return;
        }
        if !self.gait_ready || self.sample_sent > self.sample_done {
            return;
        }
        let (t, scale) = if run.leg {
            let Some(clock) = leg_clock.filter(|c| c.frozen.is_none()) else {
                // Held: the first sample after the data is live again steps the shortest dt.
                self.last_sample = None;
                return;
            };
            let t = match self.last_t {
                Some((last, last_scale)) if clock.t < last && last - clock.t <= POLL_ACTIVE.as_secs_f64() * last_scale + 1e-9 => last,
                _ => clock.t,
            };
            self.last_t = Some((t, clock.scale));
            (t, clock.scale)
        } else {
            (run.t, run.scale)
        };
        let dt = self.last_sample.map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f64()).clamp(0.001, 0.2);
        self.last_sample = Some(now);
        let reset = !self.sampled;
        self.sampled = true;
        self.sample_both = run.mode == GaitMode::Both;
        self.sample_sent += 1;
        let seq = self.sample_sent;
        self.send(MirrorCommand::Sample { seq, t, dt, scale, reset });
    }
}
