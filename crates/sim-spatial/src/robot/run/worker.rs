//! The run thread body: builds, advances, paces and publishes; the only place the simulation lives.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use sim_domain_control::drive::kinematics::BodyTwist;
use crate::robot::recording;
use super::controlled::NOT_RUNNING;
use sim_runtime::drive_host::DriveStatus;
use super::frames::{frame, frame_json};
use super::pacing::RTF_WINDOW;
use super::protocol::{Command, Published, Status};
use super::replay::{ActiveReplay, finish_replay, prepare_replay};
use super::sim::{OWNS_TARGETS, Sim};
use super::{Drive, Frame, OverlayFlags, Pace, Phase, ReplayPhase, ReplayState, Source, pace, short};

/// Sets one servo target by the file's joint name. `set_target` ignores a bad
/// index silently, so an unresolved name is an error here instead.
fn apply_jog(robot: &sim_runtime::physical::PhysicalRobot, joint: &str, target: f64) -> Result<(), String> {
    let index = robot.joint_names.iter().position(|n| short(n) == joint).ok_or_else(|| format!("joint `{joint}`: the built robot has no servo target named `{joint}` (its targets: {})", robot.joint_names.join(", ")))?;
    if index >= robot.targets.lock().unwrap_or_else(|p| p.into_inner()).len() {
        return Err(format!("joint `{joint}`: the built robot lists it (index {index}) but holds no servo target for it"));
    }
    robot.set_target(index, target);
    Ok(())
}

/// Every queued command, in order, for one pass of the run thread
/// (DRIVE_RULE): the device layer sends a drive request every frame while an
/// axis is held, so a run thread that fell behind (a high speed scale,
/// compute-limited, or blocked building the session) would otherwise apply
/// one stale request per seam period, behind which a release, Stop, halt,
/// Pause or Reset would wait while each stale request raised the heartbeat
/// (so neither deadman fired). Applying a command is cheap (a request is
/// O(1)), so the whole queue is applied before the next period, which then
/// sees only the newest request. `block` (paused) waits for the first
/// command. Returns the batch and whether the controller is gone (its
/// queued commands are still applied, so a queued save finishes, and then
/// the thread ends); None when it is gone with nothing queued.
pub(super) fn drain(rx: &mpsc::Receiver<Command>, block: bool) -> Option<(Vec<Command>, bool)> {
    let mut batch = Vec::new();
    if block {
        batch.push(rx.recv().ok()?);
    }
    loop {
        match rx.try_recv() {
            Ok(c) => batch.push(c),
            Err(mpsc::TryRecvError::Empty) => return Some((batch, false)),
            Err(mpsc::TryRecvError::Disconnected) => return Some((batch, true)),
        }
    }
}

/// The run thread. The simulation is built and advanced only here.
pub(super) fn worker(source: Source, links: Vec<String>, rx: mpsc::Receiver<Command>, out: Arc<Mutex<Published>>, start_generation: u64) {
    let chunk_s = source.chunk_s();
    let mut registry = None;
    let mut sim: Option<Sim> = None;
    let mut generation = start_generation;
    let mut steps = 0;
    // A binding that did not load: failed from the start (its error is published by the controller's initial status).
    let mut failed = matches!(source, Source::Unbound { .. });
    let mut ended = false;
    let mut running = false;
    // Pacing anchor (wall, sim) and the RTF window of (wall, sim) samples.
    let mut anchor = (Instant::now(), 0.0);
    let mut window: VecDeque<(Instant, f64)> = VecDeque::new();
    // The requested run speed scale (Command::Speed; pacing only).
    let mut scale = 1.0;
    // The overlays whose data frames carry (OVERLAY_COST_RULE); set by Command::Overlays.
    let flags = std::cell::Cell::new(OverlayFlags::default());
    let set = |status: Status, frame: Option<Frame>| {
        let mut p = out.lock().unwrap_or_else(|p| p.into_inner());
        p.status = status;
        if let Some(f) = frame {
            p.frame = Some(f);
        }
    };
    let set_jog_error = |e: Option<String>| out.lock().unwrap_or_else(|p| p.into_inner()).jog_error = e;
    let set_motion_error = |e: Option<String>| out.lock().unwrap_or_else(|p| p.into_inner()).motion_error = e;
    let set_drive = |d: Option<Arc<Drive>>| out.lock().unwrap_or_else(|p| p.into_inner()).drive = d;
    let set_replay = |r: &ReplayState| out.lock().unwrap_or_else(|p| p.into_inner()).replay = Some(r.clone());
    let set_twist = |d: Option<DriveStatus>| out.lock().unwrap_or_else(|p| p.into_inner()).twist = d;
    let set_twist_error = |e: Option<String>| out.lock().unwrap_or_else(|p| p.into_inner()).twist_error = e;
    // A stop or halt received before the first build (the newest wins), applied right after it at t = 0
    // (a nonzero request is refused while not running, so none is ever pending).
    let mut pending_twist: Option<(BodyTwist, bool)> = None;
    // The replay being advanced, and the last replay request number seen.
    let mut replay: Option<ActiveReplay> = None;
    let mut replay_seq = 0;
    // Jogs received before the first build, applied right after it.
    let mut pending: Vec<(String, f64)> = Vec::new();
    let status = |phase, generation, rtf, error: Option<String>| Status { phase, generation, rtf, error, end: None };
    // A frame of the current state, or the failure it hit (the last good frame stays published).
    // A drive session's status goes out with every frame.
    let publish = |sim: &Sim, phase: Phase, generation: u64, steps: u64, rtf: Option<f64>| -> bool {
        match sim.frame(&links, generation, steps, flags.get()) {
            Ok(f) => {
                let end = if phase == Phase::Ended { sim.ended() } else { None };
                set(Status { end, ..status(phase, generation, rtf, None) }, Some(f));
                set_twist(sim.drive_status());
                true
            }
            Err(e) => {
                set(status(Phase::Failed, generation, None, Some(format!("frame failed at t = {:.3} s: {e}", sim.time()))), None);
                false
            }
        }
    };
    loop {
        // Every queued command, in order (see `drain`); paused, block for the first.
        let Some((batch, closed)) = drain(&rx, !running) else { return };
        let mut build = |sim: &mut Option<Sim>, generation: u64, pending: &mut Vec<(String, f64)>, pending_twist: &mut Option<(BodyTwist, bool)>| -> bool {
            if sim.is_some() {
                return true;
            }
            set(status(Phase::Building, generation, None, None), None);
            match Sim::build(&source, &mut registry) {
                Ok(mut s) => {
                    if let Sim::Robot(r) = &s {
                        for (joint, target) in pending.drain(..) {
                            if let Err(e) = apply_jog(r, &joint, target) {
                                set_jog_error(Some(e));
                            }
                        }
                    }
                    if let Some((request, halt)) = pending_twist.take() {
                        set_twist_error(s.twist(request, halt).err());
                    }
                    set_drive(s.drive());
                    let ok = publish(&s, Phase::Paused, generation, 0, None);
                    *sim = Some(s);
                    ok
                }
                Err(e) => {
                    set(status(Phase::Failed, generation, None, Some(format!("build failed: {e}"))), None);
                    false
                }
            }
        };
        for command in batch {
            match command {
                Command::Reset { generation: g } => {
                    generation = g;
                    // Reset ends any replay: a fresh run.
                    replay = None;
                    set_replay(&ReplayState::new(replay_seq, g, None, ReplayPhase::Idle));
                    sim = None;
                    pending.clear();
                    pending_twist = None;
                    set_jog_error(None);
                    set_drive(None);
                    set_motion_error(None);
                    set_twist(None);
                    set_twist_error(None);
                    steps = 0;
                    running = false;
                    ended = false;
                    failed = !build(&mut sim, generation, &mut pending, &mut pending_twist);
                }
                Command::Jog { joint, .. } if failed => set_jog_error(Some(format!("joint `{joint}`: not applied; the run failed (Reset rebuilds)"))),
                Command::Motion { .. } if failed || ended => set_motion_error(Some("motion request not applied: the run failed or ended (Reset rebuilds)".into())),
                Command::Inputs { .. } if failed || ended => set_motion_error(Some("input change not applied: the run failed or ended (Reset rebuilds)".into())),
                Command::Prepare => {
                    if !failed && sim.is_none() {
                        failed = !build(&mut sim, generation, &mut pending, &mut pending_twist);
                    }
                }
                Command::Twist { .. } if failed || ended => set_twist_error(Some("drive request not applied: the run failed or ended (Reset rebuilds)".into())),
                // Saved in every phase with a built simulation, failed and ended included (labelled by recording::REPLAYABLE_RULE).
                Command::SaveRecording { seq, target, note, unix_ms } => {
                    let result = sim.as_ref().ok_or_else(|| "no built session to record: the build failed or has not run (Reset rebuilds)".to_string()).and_then(|s| {
                        let last = s.frame(&links, generation, steps, flags.get()).ok().map(|f| frame_json(&f, &links));
                        s.save(&target, note.as_deref(), unix_ms, generation, steps, last)
                    });
                    match result {
                        // Serialising and writing happen on a writer thread: a full-robot scene is megabytes.
                        Ok((snapshot, meta, root)) => {
                            let writer_out = out.clone();
                            // A save: detached (complete on drop), so the file is finished even
                            // if this run or its controller is dropped meanwhile. It publishes
                            // its own result, so the handle is dropped here.
                            drop(crate::jobs::Job::spawn(crate::jobs::Pool::Io, seq, "the recording writer", move |_| {
                                // The handle is gone, so a panic must be published here too, or
                                // recording.pending would never clear.
                                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recording::write(&root, &target, &snapshot, meta)))
                                    .unwrap_or_else(|_| Err(format!("the recording writer ended without a result (panic) writing {}", target.display())));
                                writer_out.lock().unwrap_or_else(|p| p.into_inner()).save = Some((seq, result));
                                Ok(())
                            }).complete_on_drop());
                        }
                        Err(e) => out.lock().unwrap_or_else(|p| p.into_inner()).save = Some((seq, Err(e))),
                    }
                }
                Command::Replay { generation: g, seq, path } => {
                    generation = g;
                    replay_seq = seq;
                    running = false;
                    replay = None;
                    set_motion_error(None);
                    set_twist_error(None);
                    let state = ReplayState::new(seq, g, Some(path.clone()), ReplayPhase::Replaying);
                    match prepare_replay(&source, sim.as_ref(), &path, state) {
                        Ok((next, mut r)) => {
                            r.state.replaced = true;
                            steps = 0;
                            ended = false;
                            pending.clear();
                            pending_twist = None;
                            set_jog_error(None);
                            set_drive(next.drive());
                            r.state.completed_steps = next.completed_steps();
                            let s = sim.insert(next);
                            failed = !publish(s, Phase::Running, generation, 0, None);
                            if failed {
                                finish_replay(s, &mut r, Some("the replayed session's first frame failed".into()));
                                set_replay(&r.state);
                            } else if r.work.done() {
                                finish_replay(s, &mut r, None);
                                ended = s.ended().is_some();
                                failed = !publish(s, if ended { Phase::Ended } else { Phase::Paused }, generation, 0, None);
                                set_replay(&r.state);
                            } else {
                                set_replay(&r.state);
                                running = true;
                                anchor = (Instant::now(), s.time());
                                window.clear();
                                window.push_back(anchor);
                                replay = Some(r);
                            }
                        }
                        Err((e, mut state)) => {
                            // Refused before replacing anything: the current run is republished unchanged under the new generation.
                            state.phase = ReplayPhase::Failed;
                            state.verdict = Some("refused: not replayed; the current run is unchanged".into());
                            state.error = Some(e);
                            let previous = out.lock().unwrap_or_else(|p| p.into_inner()).status.clone();
                            let frame = sim.as_ref().and_then(|s| s.frame(&links, generation, steps, flags.get()).ok());
                            set(Status { generation, rtf: None, ..previous }, frame);
                            set_replay(&state);
                        }
                    }
                }
                Command::CancelReplay => {
                    if let Some(mut r) = replay.take() {
                        running = false;
                        r.state.phase = ReplayPhase::Cancelled;
                        r.state.cancel_requested = true;
                        r.state.wall_s = Some(r.started.elapsed().as_secs_f64());
                        // `completed` is current: it rises with every replayed chunk before that
                        // chunk is published, and commands are applied only between chunks.
                        let reached = match sim.as_ref() {
                            Some(s) => match (s.replay_span_s(r.state.total.unwrap_or(0)), r.state.total) {
                                (Some(span), Some(_)) => format!(", sim time {:.3} s of {span:.3} s", s.time()),
                                _ => format!(", sim time {:.3} s", s.time()),
                            },
                            None => String::new(),
                        };
                        r.state.verdict = Some(format!("cancelled at {}{reached}: not a verdict; the replay did not finish", r.state.progress()));
                        if let Some(s) = sim.as_mut() {
                            // The recorded requests stop here: the next Run does not keep driving them.
                            s.end_drive_replay();
                            r.state.completed_steps = s.completed_steps();
                            failed = !publish(s, Phase::Paused, generation, steps, None);
                        }
                        set_replay(&r.state);
                    }
                }
                Command::Speed(x) => {
                    scale = x;
                    // Re-anchor at the current (wall, sim): no burst, no stall; the rtf window restarts.
                    if let Some(s) = sim.as_ref() {
                        anchor = (Instant::now(), s.time());
                        window.clear();
                        window.push_back(anchor);
                    }
                }
                Command::Motion { .. } if replay.is_some() => set_motion_error(Some("motion request not applied: a replay is in progress".into())),
                Command::Inputs { .. } if replay.is_some() => set_motion_error(Some("input change not applied: a replay is in progress".into())),
                Command::Twist { .. } if replay.is_some() => set_twist_error(Some("drive request not applied: a replay is in progress (it re-sends the recorded requests)".into())),
                Command::Overlays(f) => {
                    flags.set(f);
                    // Paused with a built robot (a file run or a drive session, whose frames are PhysicalRobot's):
                    // republish so the change shows now (running publishes each chunk; a failed or ended run keeps its last frame).
                    if let Some(s) = sim.as_ref().filter(|s| matches!(s, Sim::Robot(_) | Sim::Controlled { .. }) && !running && !failed && !ended) {
                        if let Ok(frame) = s.frame(&links, generation, steps, f) {
                            let previous = out.lock().unwrap_or_else(|p| p.into_inner()).status.clone();
                            set(Status { generation, ..previous }, Some(frame));
                        }
                    }
                }
                #[cfg(test)]
                Command::SetInputs(values) => match sim.as_mut().map(|s| s.set_action(values)) {
                    Some(Ok(())) => {
                        failed = !publish(sim.as_ref().unwrap(), Phase::Paused, generation, steps, None);
                    }
                    Some(Err(e)) => set_motion_error(Some(e)),
                    None => set_motion_error(Some("no built session".into())),
                },
                _ if failed || ended => {}
                Command::Jog { joint, target } => match sim.as_ref() {
                    None => pending.push((joint, target)),
                    Some(Sim::Robot(r)) => match apply_jog(r, &joint, target) {
                        Ok(()) => {
                            set_jog_error(None);
                            let phase = if running { Phase::Running } else { Phase::Paused };
                            let rtf = out.lock().unwrap_or_else(|p| p.into_inner()).status.rtf;
                            set(status(phase, generation, rtf, None), Some(frame(r, generation, steps, flags.get())));
                        }
                        Err(e) => set_jog_error(Some(e)),
                    },
                    Some(Sim::Controlled { .. }) => set_jog_error(Some(format!("joint `{joint}`: not applied; {OWNS_TARGETS}"))),
                    Some(_) => set_jog_error(Some(format!("joint `{joint}`: not applied; a preset is driven by its declared controller"))),
                },
                Command::Motion { values } => match sim.as_mut() {
                    None => set_motion_error(Some("motion request not applied: no built session".into())),
                    Some(s) => match s.set_motion(values) {
                        Ok(()) => {
                            set_motion_error(None);
                            if !running {
                                // Paused: republish so the held action shows the request now.
                                let rtf = out.lock().unwrap_or_else(|p| p.into_inner()).status.rtf;
                                failed = !publish(s, Phase::Paused, generation, steps, rtf);
                            }
                        }
                        Err(e) => set_motion_error(Some(format!("motion request not applied: {e}"))),
                    },
                },
                Command::Inputs { values } => match sim.as_mut() {
                    None => set_motion_error(Some("input change not applied: no built session (opening, Run or Step builds it)".into())),
                    Some(s) => {
                        let mut action = s.held();
                        let applied = values.iter().try_for_each(|(i, x)| match action.get_mut(*i) {
                            Some(slot) => {
                                *slot = *x;
                                Ok(())
                            }
                            None => Err(format!("input index {i} is outside the action ({} inputs)", action.len())),
                        });
                        match applied.and_then(|()| s.set_action(action)) {
                            Ok(()) => {
                                set_motion_error(None);
                                if !running {
                                    let rtf = out.lock().unwrap_or_else(|p| p.into_inner()).status.rtf;
                                    failed = !publish(s, Phase::Paused, generation, steps, rtf);
                                }
                            }
                            Err(e) => set_motion_error(Some(format!("input change not applied: {e}"))),
                        }
                    }
                },
                // A nonzero request waits for nothing while not running: no sim time
                // passes, so it would keep no age and drive the next Run with no
                // input (DRIVE_RULE). RunController::drive refuses it; this one
                // raced a Pause (or Reset) sent before it. Held inputs re-send every
                // frame, so they resume as soon as Run is pressed.
                Command::Twist { request, halt } if !running && !halt && !request.is_zero() => set_twist_error(Some(format!("drive request not applied: the run was not running when it arrived (Pause or Reset was sent first); {NOT_RUNNING}"))),
                Command::Twist { request, halt } => match sim.as_mut() {
                    // Before the first build (a stop or halt only, see above): kept (newest wins) and applied right after it.
                    None => pending_twist = Some((request, halt)),
                    Some(s) => match s.twist(request, halt) {
                        Ok(d) => {
                            set_twist_error(None);
                            // Published now, so the request and heartbeat show without waiting for a chunk (paused included).
                            set_twist(Some(d));
                        }
                        Err(e) => set_twist_error(Some(e)),
                    },
                },
                Command::Start => {
                    if build(&mut sim, generation, &mut pending, &mut pending_twist) {
                        running = true;
                        let t = sim.as_ref().map_or(0.0, Sim::time);
                        anchor = (Instant::now(), t);
                        window.clear();
                        window.push_back(anchor);
                        set(status(Phase::Running, generation, None, None), None);
                    } else {
                        failed = true;
                    }
                }
                Command::Pause => {
                    running = false;
                    // PAUSE_RULE: a live drive request does not drive again after Run without a
                    // fresh one. A replay's recorded requests are not live (they govern each
                    // period until it ends, which invalidates them: `end_drive_replay`).
                    if replay.is_none() {
                        if let Some(s) = sim.as_mut() {
                            s.pause_drive();
                            // The invalidated request shows now (paused: no chunk publishes it).
                            set_twist(s.drive_status());
                        }
                    }
                    set(status(if sim.is_some() { Phase::Paused } else { Phase::Idle }, generation, None, None), None);
                }
                Command::Step => {
                    if !build(&mut sim, generation, &mut pending, &mut pending_twist) {
                        failed = true;
                        continue;
                    }
                    let s = sim.as_mut().unwrap();
                    match s.advance() {
                        Ok(()) => {
                            steps += 1;
                            ended = s.ended().is_some();
                            failed = !publish(s, if ended { Phase::Ended } else { Phase::Paused }, generation, steps, None);
                        }
                        Err(e) => {
                            failed = true;
                            set(status(Phase::Failed, generation, None, Some(format!("advance failed at t = {:.3} s: {e}", s.time()))), None);
                        }
                    }
                }
            }
        }
        if closed {
            return;
        }
        if !running || failed || ended {
            continue;
        }
        let Some(s) = sim.as_mut() else { continue };
        // Pace (PACING): never ahead of scale × wall time; drop lag beyond one chunk.
        match pace(anchor.0.elapsed().as_secs_f64(), s.time() - anchor.1, scale, chunk_s) {
            Pace::Sleep(d) => {
                std::thread::sleep(d);
                continue;
            }
            // Keep exactly one chunk of (scaled) lag, so this chunk is due and the next one waits.
            Pace::AdvanceAndReanchor => anchor = (Instant::now() - Duration::from_secs_f64(chunk_s / scale), s.time()),
            Pace::Advance => {}
        }
        let advanced = match replay.as_mut() {
            Some(r) => s.advance_replay(&mut r.work).map(|n| r.state.completed += n),
            None => s.advance(),
        };
        match advanced {
            Ok(()) => {
                steps += 1;
                if let Some(r) = replay.as_mut() {
                    r.state.completed_steps = s.completed_steps();
                    if !r.work.done() {
                        failed = !publish(s, Phase::Running, generation, steps, None);
                        if failed {
                            running = false;
                            let mut r = replay.take().unwrap();
                            let why = format!("frame failed at t = {:.3} s", s.time());
                            finish_replay(s, &mut r, Some(why));
                            set_replay(&r.state);
                        } else {
                            set_replay(&r.state);
                        }
                        continue;
                    }
                    let mut r = replay.take().unwrap();
                    running = false;
                    finish_replay(s, &mut r, None);
                    ended = s.ended().is_some();
                    failed = !publish(s, if ended { Phase::Ended } else { Phase::Paused }, generation, steps, None);
                    if let (Some(recorded), Ok(f)) = (r.final_frame.as_ref(), s.frame(&links, generation, steps, flags.get())) {
                        r.state.measured = recording::measured(recorded, &frame_json(&f, &links));
                    }
                    set_replay(&r.state);
                    continue;
                }
                let now = Instant::now();
                window.push_back((now, s.time()));
                while window.len() > 2 && now.duration_since(window[1].0) >= RTF_WINDOW {
                    window.pop_front();
                }
                let (w0, s0) = window[0];
                let dw = now.duration_since(w0).as_secs_f64();
                let rtf = (dw > 0.0).then(|| (s.time() - s0) / dw);
                ended = s.ended().is_some();
                if ended {
                    running = false;
                }
                failed = !publish(s, if ended { Phase::Ended } else { Phase::Running }, generation, steps, rtf);
                if failed {
                    running = false;
                }
            }
            Err(e) => {
                running = false;
                failed = true;
                let frame = if let Some(mut r) = replay.take() {
                    finish_replay(s, &mut r, Some(e.clone()));
                    let f = s.frame(&links, generation, steps, flags.get()).ok();
                    if let (Some(recorded), Some(f)) = (r.final_frame.as_ref(), f.as_ref()) {
                        r.state.measured = recording::measured(recorded, &frame_json(f, &links));
                    }
                    set_replay(&r.state);
                    f
                } else {
                    None
                };
                set(status(Phase::Failed, generation, None, Some(format!("advance failed at t = {:.3} s: {e}", s.time()))), frame);
            }
        }
    }
}
