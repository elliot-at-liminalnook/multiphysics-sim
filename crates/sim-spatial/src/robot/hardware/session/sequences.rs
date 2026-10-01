//! The page's sequenced handlers: sweep all (:168-195), tune (:203-210),
//! campaign (:274-281) and gait playback (:240-270). The page's `while`
//! loops with `sleep`s are ticks run from [`Session::run_due`].
use super::{COMMAND, LEASE_RETRY, LEASE_TIMEOUT, Session, SweepAll};
use crate::jobs::{Job, Pool};
use crate::robot::hardware::actions::GaitMode;
use crate::robot::hardware::link::{CAMPAIGN_TICK, GAIT_HEARTBEAT, GaitRun, Intent, SWEEP_ALL_TICK, TUNE_TICK};
use sim_runtime::hardware_client::calibration::{self, GaitBinding, GaitEntry, Gaits};
use std::sync::Arc;
use std::sync::atomic::Ordering::SeqCst;
use std::time::Instant;

impl Session {
    // ---- sweep all ----

    /// "Sweep all enabled motors" / "Stop sweeping all" :168-182: every
    /// enabled motor with both poses taught, in one session.
    pub(in crate::robot::hardware) fn sweep_all(&mut self) {
        if self.sweep_all.is_some() {
            self.snap.sequence_text = "Sweep-all stopped.".into();
            self.stop();
            self.render();
            return;
        }
        let axes = self.snap.state.calibration.as_ref().map(|c| c.axes.clone()).unwrap_or_default();
        let mut ids: Vec<u8> = axes.iter().filter(|(_, a)| !a.disabled && a.lower.is_some() && a.upper.is_some()).map(|(k, _)| *k).collect();
        // The page's `.sort()` compares as text (10 before 2).
        ids.sort_by_key(|k| k.to_string());
        let Some(&first) = ids.first() else {
            self.snap.sequence_text = "No enabled motor has both poses taught.".into();
            self.render();
            return;
        };
        self.sweep_all_runs += 1;
        let token = self.sweep_all_runs;
        let roles = axes.iter().map(|(k, a)| (*k, a.role.clone())).collect();
        self.sweep_all = Some(SweepAll { token, roles, start: Default::default(), next: Instant::now() + SWEEP_ALL_TICK });
        self.snap.sequence_text = "Sweep-all: checking every enabled motor at zero drive…".into();
        self.render();
        self.select_motor(first, true, false);
        // `if(sweepAllRun!==mine)return;` (a STOP pending ends it as the page's stop() would).
        if !self.sweep_all_is(token) || self.interrupted() {
            return;
        }
        if !self.snap.ready {
            let reason = self.snap.state.message.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| "could not connect".into());
            return self.sweep_all_fail(token, reason);
        }
        self.snap.intent = Intent::Sweep;
        self.snap.sweeping = true;
        let Some(id) = self.snap.id else { return };
        let input = self.input();
        match self.send_status(calibration::sweep_all(id, self.seq(), &input)) {
            Ok(s) => {
                if self.interrupted() {
                    // STOP was pressed while the sweep started: the answer is dropped.
                    return self.stop_after_dropped(id);
                }
                if !self.sweep_all_is(token) {
                    return;
                }
                self.snap.run = s.sweep.as_ref().and_then(|w| w.run_id);
                self.adopt(s);
            }
            Err(e) => {
                self.snap.intent = Intent::Hold;
                self.snap.sweeping = false;
                return self.sweep_all_fail(token, e);
            }
        }
        self.update();
        if let Some(run) = self.sweep_all.as_mut().filter(|r| r.token == token) {
            run.next = Instant::now() + SWEEP_ALL_TICK;
        }
        self.render();
    }
    fn sweep_all_is(&self, token: u64) -> bool {
        self.sweep_all.as_ref().is_some_and(|r| r.token == token)
    }
    /// `fail(reason)` :176.
    fn sweep_all_fail(&mut self, token: u64, reason: String) {
        if self.sweep_all_is(token) {
            self.sweep_all = None;
            self.snap.sequence_text = format!("Sweep-all stopped: {reason}");
            self.render();
        }
    }
    /// One pass of the progress loop :185-190, and its end :191-194.
    pub(in crate::robot::hardware) fn sweep_all_tick(&mut self) {
        let Some(run) = self.sweep_all.as_mut() else { return };
        let token = run.token;
        let sweep = self.snap.state.sweep.clone().unwrap_or_default();
        let role = |roles: &std::collections::BTreeMap<u8, String>, k: u8| roles.get(&k).cloned().unwrap_or_else(|| k.to_string());
        let parts: Vec<String> = sweep
            .motor_ids
            .iter()
            .map(|&k| match sweep.axes.get(&k).and_then(|a| a.half_cycles) {
                None => format!("{} starting", role(&run.roles, k)),
                Some(h) => {
                    let start = *run.start.entry(k).or_insert(h);
                    format!("{} {}/2 ends", role(&run.roles, k), 2i64.min(h as i64 - start as i64))
                }
            })
            .collect();
        let skipped = if sweep.skipped.is_empty() { String::new() } else { format!(" · skipped {}", sweep.skipped.join(", ")) };
        self.snap.sequence_text = format!("Sweep-all: {}{skipped}", parts.join(" · "));
        self.render();
        if !self.sweep_all_is(token) {
            return;
        }
        if self.snap.run.is_some() {
            if let Some(run) = self.sweep_all.as_mut() {
                run.next = Instant::now() + SWEEP_ALL_TICK;
            }
            return;
        }
        let roles = self.sweep_all.take().map(|r| r.roles).unwrap_or_default();
        self.snap.intent = Intent::Hold;
        self.snap.sweeping = false;
        let sweep = self.snap.state.sweep.clone().unwrap_or_default();
        self.snap.sequence_text = match sweep.motion_error.filter(|e| !e.is_empty()) {
            Some(e) => format!("Sweep-all stopped: {e}"),
            None => {
                let names: Vec<String> = sweep.motor_ids.iter().map(|&k| role(&roles, k)).collect();
                let skipped = if sweep.skipped.is_empty() { String::new() } else { format!(" Skipped: {}.", sweep.skipped.join(", ")) };
                format!("Swept {} through their saved ranges.{skipped}", names.join(", "))
            }
        };
        self.render();
    }

    // ---- tune and campaign ----

    /// "Tune this motor" :203-210 (the UI checked its confirmation box).
    pub(in crate::robot::hardware) fn tune(&mut self) {
        let Some(target) = self.snap.id else { return };
        if self.snap.tuning {
            return;
        }
        self.stop();
        self.select_motor(target, false, true);
        if !self.snap.ready || self.snap.id != Some(target) {
            return;
        }
        self.snap.tuning = true;
        self.render();
        match self.send_status(calibration::tune(target, self.seq(), self.drive_pwm())) {
            Ok(s) => {
                // The page adopts it either way; a tune the server started
                // after the UI's STOP is stopped again.
                if self.interrupted() {
                    self.stop_after_dropped(target);
                }
                self.adopt(s);
            }
            Err(e) => {
                self.message(e);
                self.snap.tuning = false;
                self.render();
                return;
            }
        }
        self.next_tune = Instant::now() + TUNE_TICK;
        self.render();
    }
    /// One pass of `while(tuning){await sleep(300);…}` and its end (`tune_done` unchecks the box).
    pub(in crate::robot::hardware) fn tune_tick(&mut self) {
        if !self.snap.state.tuning.as_ref().is_some_and(|t| t.running) {
            self.snap.tuning = false;
        }
        self.render();
        if self.snap.tuning {
            self.next_tune = Instant::now() + TUNE_TICK;
        } else {
            self.snap.ready = false;
            self.snap.tune_done += 1;
            self.render();
        }
    }
    /// "Run campaign" / "Resume" :274-281 (the UI checked its confirmation box).
    pub(in crate::robot::hardware) fn campaign(&mut self, resume: bool) {
        let Some(target) = self.snap.id else { return };
        if self.snap.campaigning {
            return;
        }
        self.stop();
        self.select_motor(target, true, false);
        if !self.snap.ready || self.snap.id != Some(target) {
            return;
        }
        self.snap.campaigning = true;
        self.render();
        match self.send_status(calibration::campaign(target, self.seq(), resume)) {
            Ok(s) => {
                // As for tune: a campaign started after the UI's STOP is stopped again.
                if self.interrupted() {
                    self.stop_after_dropped(target);
                }
                self.adopt(s);
            }
            Err(e) => {
                self.message(e);
                self.snap.campaigning = false;
                self.render();
                return;
            }
        }
        self.next_campaign = Instant::now() + CAMPAIGN_TICK;
        self.render();
    }
    /// One pass of `while(campaigning){await sleep(500);…}` and its end.
    pub(in crate::robot::hardware) fn campaign_tick(&mut self) {
        if !self.snap.state.campaign.as_ref().is_some_and(|c| c.running) {
            self.snap.campaigning = false;
        }
        self.render();
        if self.snap.campaigning {
            self.next_campaign = Instant::now() + CAMPAIGN_TICK;
        } else {
            self.snap.ready = false;
            self.snap.campaign_done += 1;
            self.render();
        }
    }

    // ---- gait playback ----

    /// `loadGaits()` :240.
    pub(in crate::robot::hardware) fn load_gaits(&mut self) {
        match self.client.get_as::<Gaits>("/calibration/gaits") {
            Ok(list) => {
                self.snap.gaits = list.gaits;
                self.snap.gaits_loaded = true;
                self.snap.gait_notice = None;
            }
            Err(e) => self.snap.gait_notice = Some(format!("Gait list unavailable: {e}")),
        }
        self.render();
    }
    /// `gaitPlay()` :249-266: toggles a playing gait, else starts `entry`.
    pub(in crate::robot::hardware) fn gait_play(&mut self, entry: GaitEntry, mode: GaitMode, bindings: Vec<GaitBinding>, skipped: Vec<String>) {
        if self.snap.gait.is_some() {
            return self.gait_toggle();
        }
        match self.start_gait(&entry, mode, &bindings, skipped) {
            Ok(run) => {
                self.snap.gait = Some(run);
                self.snap.gait_notice = None;
                self.gait_last = Instant::now();
                self.next_frame = self.gait_last;
            }
            Err(e) => {
                self.snap.gait_notice = Some(e);
                self.snap.gait = None;
            }
        }
        self.render();
    }
    fn start_gait(&mut self, entry: &GaitEntry, mode: GaitMode, bindings: &[GaitBinding], skipped: Vec<String>) -> Result<GaitRun, String> {
        let compiled = self.client.get(&calibration::gait_path(&entry.path)).map_err(|e| e.to_string())?;
        // The mirror's `loadGait`: the shared sampler's own reading of the gait.
        let gait = sim_runtime::gait_playback::Gait::from_compiled(&compiled, &entry.trial)?;
        self.plays += 1;
        self.snap.compiled_gait = Some((self.plays, Arc::new(compiled), entry.trial.clone()));
        let scale = self.inputs.gait_speed_percent / 100.0;
        let leg = mode != GaitMode::Sim;
        let mut run = GaitRun { mode, period_s: gait.info.period_s, t: 0.0, playing: true, scale, leg, skipped: Vec::new(), started: false };
        if leg {
            run.skipped = skipped;
            let Some(first) = bindings.first() else {
                return Err(format!("No motor is aligned, taught and enabled: {}", run.skipped.join(", ")));
            };
            self.stop();
            self.select_motor(first.id, true, false);
            if !self.snap.ready {
                return Err(self.snap.state.message.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| "Could not enable the motors".into()));
            }
            let id = self.snap.id.ok_or("Could not enable the motors")?;
            let effort = self.inputs.gait_effort_percent / 100.0;
            let body = calibration::gait_start(id, self.seq(), &entry.path, bindings, scale, effort, self.drive_pwm(), self.inputs.drive_mode.wire());
            let status = self.send_status(body)?;
            if self.interrupted() {
                // STOP was pressed while the gait started: drop the answer
                // (the page does not check) and stop what it may have started.
                self.stop_after_dropped(id);
                return Err("STOP was pressed while the gait was starting.".into());
            }
            self.adopt(status);
            self.next_lease = Instant::now() + GAIT_HEARTBEAT;
            self.snap.ready = false;
        }
        Ok(run)
    }
    /// Pause / Resume :250.
    pub(in crate::robot::hardware) fn gait_toggle(&mut self) {
        self.sim_frame();
        let Some(run) = self.snap.gait.as_mut() else { return };
        run.playing = !run.playing;
        if run.leg {
            self.gait_lease();
        }
        self.render();
    }
    /// The gait's Stop :267.
    pub(in crate::robot::hardware) fn gait_stop(&mut self) {
        let leg = self.leg_gait();
        self.end_gait();
        self.render();
        if leg {
            self.stop();
        }
    }
    /// The playback speed slider :268.
    pub(in crate::robot::hardware) fn gait_scale(&mut self) {
        // Time played so far counts at the old speed.
        self.sim_frame();
        let scale = self.inputs.gait_speed_percent / 100.0;
        let Some(run) = self.snap.gait.as_mut() else { return };
        run.scale = scale;
        if run.leg {
            self.gait_lease();
        }
        self.render();
    }
    /// `api('command',{action:'gait_update',…}).catch(()=>{})`: the lease
    /// heartbeat and the pause/speed changes (errors ignored, as the page).
    ///
    /// Posted from its own `Pool::Dedicated` job, not this thread: the
    /// server ends the leg gait when no update arrives for 1.5 s, and a
    /// request here may wait up to 8 s for the hardware. At most one is in
    /// flight; while one is, this one is not sent and the lease deadline is
    /// pulled in to [`LEASE_RETRY`], so the newest scale and pause state
    /// follow as soon as it answers (one after the other, never reordered).
    /// Nothing is sent while a STOP is pending, also by a job that starts
    /// after one was posted.
    pub(in crate::robot::hardware) fn gait_lease(&mut self) {
        let Some((scale, playing)) = self.snap.gait.as_ref().filter(|g| g.leg).map(|g| (g.scale, g.playing)) else { return };
        if self.interrupted() {
            return;
        }
        // `poll` takes a finished job's result (ignored, as the page's
        // `.catch`); the handle is then replaced at once below.
        if self.lease.as_ref().is_some_and(|job| job.poll().is_none()) {
            self.next_lease = self.next_lease.min(Instant::now() + LEASE_RETRY);
            return;
        }
        let body = calibration::gait_update(scale, playing);
        let client = self.client.clone().with_timeout(LEASE_TIMEOUT);
        let (epoch, seen) = (self.epoch.clone(), self.seen_epoch);
        self.lease = Some(Job::spawn(Pool::Dedicated, 0, "hardware gait lease", move |ctx| {
            if ctx.cancelled() || epoch.load(SeqCst) != seen {
                return Ok(());
            }
            client.post(COMMAND, &body).map(|_| ()).map_err(|e| e.to_string())
        }));
    }
    /// `endGait()` :241 (the mirror stops showing the gait when `gait` is
    /// None). The lease update handle is dropped (a job not yet sending sends nothing).
    pub(in crate::robot::hardware) fn end_gait(&mut self) {
        self.snap.gait = None;
        self.lease = None;
    }
    /// `gaitFrame` :244 for a sim-only gait: its clock advances by wall time × scale while playing.
    pub(in crate::robot::hardware) fn sim_frame(&mut self) {
        let now = Instant::now();
        if let Some(run) = self.snap.gait.as_mut().filter(|g| !g.leg && g.playing) {
            run.t += now.duration_since(self.gait_last).as_secs_f64() * run.scale;
        }
        self.gait_last = now;
    }
    /// `gaitFrame` :243 for a leg gait: the leg's time, and `started` once the server reports it running.
    pub(in crate::robot::hardware) fn leg_frame(&mut self) {
        let (t, running) = self.snap.state.gait.as_ref().map_or((0.0, false), |g| (g.t.unwrap_or(0.0), g.running));
        if let Some(run) = self.snap.gait.as_mut().filter(|g| g.leg) {
            run.t = t;
            if running {
                run.started = true;
            }
        }
    }
}
