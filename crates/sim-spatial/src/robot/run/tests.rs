use super::*;
use super::frames::motor_targets;
use super::sim::Sim;
use bevy::math::DQuat;
use crate::robot::motion;
use crate::robot::preset::PresetRun;
use crate::robot::recording;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
fn wait(c: &mut RunController, what: &str, done: impl Fn(&RunController) -> bool) {
    let start = Instant::now();
    while !done(c) {
        assert!(start.elapsed() < Duration::from_secs(120), "timed out waiting for {what}: phase {:?}, {:?}", c.phase(), c.status.error);
        std::thread::sleep(Duration::from_millis(5));
        c.poll();
    }
}
#[test]
fn preset_frames_carry_named_motor_targets() {
    let names = Arc::new(vec!["joint.+X | Hip servo output".to_string(), "joint.+X | Worm servo output".to_string()]);
    let t = motor_targets(&json!({"servo_targets_rad": [0.25, -0.5]}), &names).expect("targets");
    assert!(Arc::ptr_eq(&t.coordinates, &names), "the build's names are shared, not copied per frame");
    assert_eq!(t.targets_rad, vec![0.25, -0.5]);
    assert!(!t.done, "no `done` member: not ended");
    assert!(motor_targets(&json!({"servo_targets_rad": [0.25], "done": true}), &names).expect("targets").done, "the frame's `done` is carried");
    assert!(motor_targets(&json!({}), &names).is_none(), "a frame without servo_targets_rad has none");
    assert!(motor_targets(&json!({"servo_targets_rad": [0.1, null]}), &names).is_none(), "a non-number is not a target");
}
#[test]
fn pace_is_due_when_scaled_wall_catches_up_and_drops_lag() {
    let c = CHUNK_S;
    // ×0.25: one chunk of sim time is due after chunk / 0.25 = 0.08 s of wall time.
    assert!(matches!(pace(0.079, c, 0.25, c), Pace::Sleep(_)));
    assert_eq!(pace(0.08, c, 0.25, c), Pace::Advance);
    // ×4: due after 0.005 s of wall time.
    assert!(matches!(pace(0.0049, c, 4.0, c), Pace::Sleep(_)));
    assert_eq!(pace(0.005, c, 4.0, c), Pace::Advance);
    // Sleeps are in wall time ((sim - wall × scale) / scale), capped at 5 ms.
    assert_eq!(pace(0.07, c, 0.25, c), Pace::Sleep(Duration::from_secs_f64(0.005)));
    let Pace::Sleep(d) = pace(0.004, c, 4.0, c) else { panic!("not due yet") };
    assert!((d.as_secs_f64() - 0.001).abs() < 1e-9);
    // Lag beyond one chunk is dropped (re-anchor), not made up: at ×4, 1 s of wall
    // time owes 4 s of sim time, but only one chunk is advanced before re-anchoring.
    assert_eq!(pace(1.0, 0.0, 4.0, c), Pace::AdvanceAndReanchor);
    assert_eq!(pace(0.08, 0.0, 0.25, c), Pace::Advance);
    assert_eq!(pace(0.09, 0.0, 0.25, c), Pace::AdvanceAndReanchor);
    // After the worker's re-anchor (one chunk of lag kept: wall = chunk / scale), that one
    // chunk may be made up, but the chunk after it waits for scaled wall time again.
    for scale in SPEED_SCALES {
        assert_eq!(pace(c / scale, 0.0, scale, c), Pace::Advance);
        assert_eq!(pace(c / scale, c, scale, c), Pace::Advance);
        assert!(matches!(pace(c / scale, 2.0 * c, scale, c), Pace::Sleep(_)));
    }
    // Speed requests: powers of two only, refused (never clamped) at the ends and off the list.
    assert_eq!(speed_target(1.0, SpeedRequest::Up), Ok(2.0));
    assert_eq!(speed_target(0.25, SpeedRequest::Down), Ok(0.125));
    assert!(speed_target(8.0, SpeedRequest::Up).unwrap_err().contains("fastest"));
    assert!(speed_target(0.125, SpeedRequest::Down).unwrap_err().contains("slowest"));
    assert_eq!(speed_target(1.0, SpeedRequest::Set { scale: 4.0 }), Ok(4.0));
    for bad in [3.0, 16.0, 0.0, -1.0, f64::NAN] {
        assert!(speed_target(1.0, SpeedRequest::Set { scale: bad }).unwrap_err().contains("0.125, 0.25, 0.5, 1, 2, 4, 8"));
    }
}

#[test]
fn run_thread_steps_one_chunk_resets_generation_and_rejects_stale_frames() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let model = crate::robot::load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap().model;
    let assembly: Vec<[f64; 3]> = model.links.iter().map(|l| l.com).collect();
    let mut c = RunController::spawn(model);
    assert_eq!(c.phase(), Phase::Idle);
    assert!(c.frame().is_none());
    // Step before any run builds, then advances exactly one chunk.
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "first step", |c| c.frame().is_some_and(|f| f.steps == 1));
    let f = c.frame().unwrap();
    assert!((f.time - CHUNK_S).abs() < 1e-9, "t = {}", f.time);
    assert_eq!((f.generation, c.phase()), (0, Phase::Paused));
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "second step", |c| c.frame().is_some_and(|f| f.steps == 2));
    assert!((c.frame().unwrap().time - 2.0 * CHUNK_S).abs() < 1e-9);
    // Step while running is an error, not a no-op.
    c.act(RunAction::Start).unwrap();
    let err = c.act(RunAction::Step).unwrap_err();
    assert!(err.contains("pause first"), "{err}");
    wait(&mut c, "running", |c| c.frame().is_some_and(|f| f.steps > 3));
    c.act(RunAction::Pause).unwrap();
    // Reset: generation + 1, t = 0 at the assembly pose; the old frame is stale.
    let old = c.frame().unwrap().clone();
    c.act(RunAction::Reset).unwrap();
    assert_eq!(c.generation(), 1);
    assert!(!accept(c.generation(), &old));
    assert!(c.frame().is_none_or(|f| f.generation == 1));
    wait(&mut c, "reset", |c| c.frame().is_some() && c.phase() == Phase::Paused);
    let f = c.frame().unwrap();
    assert_eq!((f.generation, f.steps, f.time), (1, 0, 0.0));
    for (pose, com) in f.poses.iter().zip(&assembly) {
        let (p, q) = pose.as_ref().expect("every wheeled link has a pose");
        assert!((0..3).all(|k| (p[k] - com[k]).abs() < 1e-9), "{p:?} vs {com:?}");
        assert!(q.angle_between(DQuat::IDENTITY) < 1e-9);
    }
    assert!(RunAction::parse("jump").unwrap_err().contains("`jump`"));
}

#[test]
fn jog_validates_against_the_file_and_moves_the_servo_toward_its_target() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut model = crate::robot::load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap().model;
    // In memory only: give the driven left axle file limits.
    model.joints.iter_mut().find(|j| j.name == "left axle").unwrap().limits = Some([-0.3, 0.3]);
    let mut c = RunController::spawn(model);
    // File validation, idle, with no build.
    let e = c.jog("left axle", 0.5).unwrap_err();
    assert!(e.contains("`left axle`") && e.contains("[-0.3, 0.3]") && e.contains("0.5"), "{e}");
    let e = c.jog("elbow", 0.1).unwrap_err();
    assert!(e.contains("unknown joint `elbow`") && e.contains("left axle"), "{e}");
    let e = c.jog("passive axle", 0.1).unwrap_err();
    assert!(e.contains("`passive axle` has no servo target"), "{e}");
    assert!(c.jog("left axle", f64::NAN).unwrap_err().contains("not finite"));
    // right axle has no limits in the file: any finite target, labelled so.
    assert_eq!(servo(c.model(), "right axle").unwrap().limit_text(), "no limit in file");
    // Before the first build a jog is queued and applied right after the build.
    c.jog("left axle", 0.2).unwrap();
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "first step", |c| c.frame().is_some_and(|f| f.steps == 1));
    let (target, a0) = c.frame().unwrap().servo("left axle").unwrap();
    assert_eq!(target, 0.2);
    assert_eq!(c.frame().unwrap().servo("right axle").unwrap().0, 0.0);
    for n in 2..=25 {
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "steps", |c| c.frame().is_some_and(|f| f.steps == n));
    }
    let (_, a1) = c.frame().unwrap().servo("left axle").unwrap();
    assert!((0.2 - a1).abs() < (0.2 - a0).abs() && a1 > a0, "angle {a0} -> {a1}, target 0.2");
    // While paused, a jog sets the target in the next frame without stepping.
    let steps = c.frame().unwrap().steps;
    c.jog("left axle", -0.25).unwrap();
    wait(&mut c, "paused jog", |c| c.frame().is_some_and(|f| f.servo("left axle").unwrap().0 == -0.25));
    assert_eq!(c.frame().unwrap().steps, steps);
    // An out-of-limit jog is refused and leaves the target unchanged.
    let e = c.jog("left axle", -0.31).unwrap_err();
    assert!(e.contains("`left axle`") && e.contains("[-0.3, 0.3]") && e.contains("not clamped"), "{e}");
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "step after refusal", |c| c.frame().is_some_and(|f| f.steps == steps + 1));
    assert_eq!(c.frame().unwrap().servo("left axle").unwrap().0, -0.25);
    assert!(c.jog_error().is_none());
    // Reset returns to the file's control target.
    c.act(RunAction::Reset).unwrap();
    wait(&mut c, "reset", |c| c.frame().is_some() && c.phase() == Phase::Paused);
    assert_eq!(c.frame().unwrap().servo("left axle").unwrap().0, 0.0);
}

#[test]
fn overlays_copy_physical_robot_accessors_follow_flags_and_reject_stale_frames() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let model = crate::robot::load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap().model;
    let links: Vec<String> = model.links.iter().map(|l| l.name.clone()).collect();
    // The run thread's own build, advance and frame path, on the ground.
    let mut sim = Sim::build(&Source::Robot(model.clone()), &mut None).unwrap();
    for _ in 0..25 {
        sim.advance().unwrap();
    }
    let flags = OverlayFlags::default();
    let t = Instant::now();
    let f = sim.frame(&links, 3, 25, flags).unwrap();
    println!("frame with all overlays: {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
    let Sim::Robot(r) = &sim else { panic!("FILE mode builds a PhysicalRobot") };
    let contacts = f.overlays.contacts.as_ref().unwrap();
    assert!(!contacts.is_empty(), "the wheeled robot rests on the ground after 0.5 s");
    let expected = r.contacts();
    assert_eq!(contacts.len(), expected.len());
    for (c, e) in contacts.iter().zip(&expected) {
        assert_eq!((c.point, c.force, c.penetration), ([e.point.x, e.point.y, e.point.z], [e.force.x, e.force.y, e.force.z], e.penetration));
        assert_eq!(c.link, links[e.link]);
        assert_eq!(c.other, e.other.map_or("ground".to_string(), |o| links[o].clone()));
        assert!(c.force.iter().all(|x| x.is_finite()));
    }
    assert!(contacts.iter().any(|c| c.other == "ground" && c.force[2] > 0.0), "ground contacts push up: {contacts:?}");
    let joints = f.overlays.joints.as_ref().unwrap();
    assert_eq!(joints.len(), model.joints.len());
    assert_eq!(joints.iter().map(|j| j.name.as_str()).collect::<Vec<_>>(), model.joints.iter().map(|j| j.name.as_str()).collect::<Vec<_>>());
    assert_eq!(f.overlays.deflections.as_ref().unwrap().len(), r.deflections().len());
    // A hidden overlay is not computed.
    let off = sim.frame(&links, 3, 25, OverlayFlags { contacts: false, joints: false, deflections: false }).unwrap();
    assert_eq!((off.overlays.contacts, off.overlays.joints, off.overlays.deflections), (None, None, None));
    // An older generation's frame is never accepted.
    assert!(accept(3, &f) && !accept(4, &f));

    // Through the controller: a paused toggle republishes; Reset makes the old frame stale.
    let mut c = RunController::spawn(model.clone());
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "first step", |c| c.frame().is_some_and(|f| f.steps == 1));
    assert!(c.frame().unwrap().overlays.joints.is_some());
    c.set_overlays(OverlayFlags { contacts: true, joints: false, deflections: false }).unwrap();
    wait(&mut c, "republished frame", |c| c.frame().is_some_and(|f| f.overlays.flags == Some(c.overlays())));
    assert!(c.frame().unwrap().overlays.joints.is_none());
    let v = c.overlays_json();
    assert_eq!((v["available"].as_bool(), v["flags"]["joints"].as_bool(), v["joints"]["count"].as_u64()), (Some(true), Some(false), None));
    let old = c.frame().unwrap().clone();
    c.act(RunAction::Reset).unwrap();
    assert!(!accept(c.generation(), &old));
    wait(&mut c, "reset", |c| c.frame().is_some() && c.phase() == Phase::Paused);
    assert_eq!(c.overlays_json()["frame_generation"].as_u64(), Some(1));
    // A reload keeps the overlay choice.
    let (next, _) = RunController::replace(Some(c), model);
    assert_eq!(next.overlays(), OverlayFlags { contacts: true, joints: false, deflections: false });
}

#[test]
fn graphs_sample_current_generation_frames_clear_on_reset_and_ignore_stale_frames() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let model = crate::robot::load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap().model;
    let axle = model.joints.iter().find(|j| j.name == "left axle").unwrap().child.clone();
    let link = model.links.iter().position(|l| l.name == axle).unwrap();
    let mut c = RunController::spawn(model);
    let joint_traces = |c: &RunController| -> Vec<Value> {
        let g = c.graphs_json(Some(link), true);
        assert_eq!(g["charts"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect::<Vec<_>>(), ["joints"], "--robot FILE has no motion chart");
        g["charts"][0]["traces"].as_array().unwrap().clone()
    };
    let samples = |c: &RunController| joint_traces(c).iter().find(|t| t["name"] == "left axle target").unwrap()["samples"].as_u64().unwrap();
    c.jog("left axle", 0.2).unwrap();
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "first step", |c| c.frame().is_some_and(|f| f.steps == 1));
    let first = samples(&c);
    assert!(first >= 1);
    for n in 2..=6 {
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "steps", |c| c.frame().is_some_and(|f| f.steps == n));
        // One sample per applied chunk, recorded under the current generation.
        assert_eq!(samples(&c), first + n - 1);
        assert_eq!(c.graphs().generation(), 0);
        let (target, angle) = c.frame().unwrap().servo("left axle").unwrap();
        let traces = joint_traces(&c);
        let latest = |name: &str| traces.iter().find(|t| t["name"] == name).unwrap()["latest"].as_f64().unwrap();
        assert_eq!((latest("left axle target"), latest("left axle measured")), (target, angle));
    }
    assert_eq!(c.graphs_json(Some(link), true)["mode"], "live");
    // Reset clears every trace at once, before the rebuild publishes.
    let old = c.frame().unwrap().clone();
    c.act(RunAction::Reset).unwrap();
    assert_eq!((c.graphs().frames(), samples(&c), c.graphs().generation()), (0, 0, 1));
    wait(&mut c, "reset", |c| c.frame().is_some() && c.phase() == Phase::Paused);
    let after = (c.graphs().frames(), samples(&c));
    assert_eq!(after, (1, 1), "the t = 0 frame of generation 1");
    // A stale-generation frame reaching poll adds no sample (the run thread is paused, so nothing races it).
    c.thread.lock().frame = Some(old);
    assert!(!c.poll());
    assert_eq!((c.graphs().frames(), samples(&c)), after);
    assert_eq!(c.frame().unwrap().generation, 1);
}

fn preset(id: &str) -> Result<(crate::robot::Loaded, PresetRun), String> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let p = crate::robot::preset::select(&root.join(crate::robot::preset::PRESETS), &root, id)?;
    crate::robot::load_preset(p, &root)
}

#[test]
fn preset_session_steps_one_chunk_ends_at_its_horizon_and_resets() {
    // A live preset opens as the scene's own controller session (no config).
    let (_, live) = preset("pendulum-live").unwrap();
    assert_eq!(live.kind(), "Session");
    assert!(live.config.is_none() && live.chunk_steps() == 1 && live.step_s() == live.scene.period_s);
    // Refusals name the id.
    let e = preset("no-such-preset").err().unwrap();
    assert!(e.contains("unknown robot preset `no-such-preset`"), "{e}");
    // The smallest embedded preset: pendulum scene + embedded config, no task → EmbeddedSession.
    let (loaded, run) = preset("pendulum-embedded").unwrap();
    assert_eq!((run.kind(), run.seed), ("EmbeddedSession", 0));
    assert!(loaded.geometry.iter().any(|g| g.is_some()));
    let chunk = run.chunk_steps() as u64;
    assert_eq!(chunk, run.config.as_ref().unwrap().report_every.clamp(1, 40) as u64);
    let horizon = run.requested_steps() as u64 / chunk;
    let mut c = RunController::spawn_preset(Arc::new(run));
    assert!(c.jog("pivot", 0.0).unwrap_err().contains("declared controller recipe"));
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "first step", |c| c.frame().is_some_and(|f| f.steps == 1));
    let f = c.frame().unwrap();
    let step_s = c.preset().unwrap().step_s();
    assert_eq!(f.completed_steps, Some(chunk));
    assert!((f.time - c.chunk_s()).abs() < 1e-12 && (f.time - chunk as f64 * step_s).abs() < 1e-12, "t = {}", f.time);
    // Every frame link maps to a loaded link by name.
    assert!(f.unmatched.is_empty(), "unmatched {:?}", f.unmatched);
    assert!(f.poses.iter().all(Option::is_some));
    // Run to the horizon: a distinct ended phase with its reason; Run is then refused naming Reset.
    c.act(RunAction::Start).unwrap();
    wait(&mut c, "horizon", |c| c.phase() == Phase::Ended);
    let f = c.frame().unwrap();
    assert_eq!((f.steps, f.completed_steps), (horizon, Some(horizon * chunk)));
    let end = c.end().unwrap();
    assert_eq!(end["kind"], "horizon");
    assert!(end["message"].as_str().unwrap().contains("horizon reached"));
    assert!(c.act(RunAction::Start).unwrap_err().contains("Reset"));
    assert!(c.act(RunAction::Step).unwrap_err().contains("Reset"));
    // Reset rebuilds from the same parsed files and seed: t = 0, generation + 1.
    let old = c.frame().unwrap().clone();
    c.act(RunAction::Reset).unwrap();
    assert!(!accept(c.generation(), &old));
    wait(&mut c, "reset", |c| c.frame().is_some() && c.phase() == Phase::Paused);
    let f = c.frame().unwrap();
    assert_eq!((f.generation, f.steps, f.completed_steps, f.time), (1, 0, Some(0), 0.0));
    assert!(c.end().is_none());
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "step after reset", |c| c.frame().is_some_and(|f| f.steps == 1));
    assert_eq!(c.frame().unwrap().completed_steps, Some(chunk));
}

/// The one motion handler on the full-robot robot-measured-400hz preset
/// (motion_commands, key vectors and the packet heartbeat from presets.json):
/// accept, refuse by name, Stop, and one heartbeat increment per packet.
#[test]
fn preset_motion_requests_validate_against_session_bounds_and_advance_the_heartbeat() {
    let (_, run) = preset("robot-measured-400hz").unwrap();
    let mut c = RunController::spawn_preset(Arc::new(run));
    let e = c.motion(MotionRequest::Key('w')).unwrap_err();
    assert!(e.contains("no built session"), "{e}");
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "first step", |c| c.frame().is_some_and(|f| f.steps == 1) && c.preset_drive().is_some());
    let drive = c.preset_drive().unwrap().clone();
    let m = drive.motion.as_ref().expect("400hz declares motion_commands");
    assert_eq!(m.source, motion::Source::Preset);
    let [fwd, lat, yaw] = m.channels.each_ref().map(|ch| ch.index);
    let hb = drive.heartbeat.as_ref().expect("400hz declares motion_heartbeat").index;
    let initial = drive.inputs[hb].initial;
    assert_eq!(c.frame().unwrap().inputs[hb], initial + 1.0, "one packet sent");
    // W: the declared vector [0.1, 0, 0] becomes the held forward_speed (paused: republished without stepping).
    c.motion(MotionRequest::Key('w')).unwrap();
    wait(&mut c, "W held", |c| c.frame().is_some_and(|f| f.inputs[fwd] == 0.1));
    let f = c.frame().unwrap();
    assert_eq!((f.steps, f.inputs[lat], f.inputs[yaw]), (1, 0.0, 0.0));
    assert_eq!(c.motion_json()["requested"], json!([0.1, 0.0, 0.0]));
    // Refusals name the channel (and bounds); nothing is clamped or sent.
    let f_ch = &m.channels[0];
    let e = c.motion(MotionRequest::Channels([(f_ch.name.clone(), f_ch.upper + 1.0)].into())).unwrap_err();
    assert!(e.contains("`command.forward_speed`") && e.contains(&f_ch.bounds()) && e.contains("not clamped"), "{e}");
    let e = c.motion(MotionRequest::Channels([("command.jump".to_string(), 0.0)].into())).unwrap_err();
    assert!(e.contains("unknown channel `command.jump`"), "{e}");
    let other = drive.inputs.iter().enumerate().find(|(i, _)| ![fwd, lat, yaw, hb].contains(i)).map(|(_, ch)| ch.name.clone()).expect("a non-motion input");
    let e = c.motion(MotionRequest::Channels([(other.clone(), 0.0)].into())).unwrap_err();
    assert!(e.contains(&format!("`{other}`")) && e.contains("not a motion command channel"), "{e}");
    let e = c.motion(MotionRequest::Channels([(f_ch.name.clone(), f64::NAN)].into())).unwrap_err();
    assert!(e.contains("not finite"), "{e}");
    assert_eq!(c.motion_json()["last_refusal"], json!(e));
    assert_eq!(c.motion_json()["requested"], json!([0.1, 0.0, 0.0]));
    // The heartbeat advances by one across one step; the request stays held.
    let before = c.frame().unwrap().inputs[hb];
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "second step", |c| c.frame().is_some_and(|f| f.steps == 2));
    let f = c.frame().unwrap();
    assert_eq!((f.inputs[hb], f.inputs[fwd]), (before + 1.0, 0.1));
    // Stop: every motion channel to zero.
    c.motion(MotionRequest::Stop).unwrap();
    wait(&mut c, "stop", |c| c.frame().is_some_and(|f| f.inputs[fwd] == 0.0));
    let f = c.frame().unwrap();
    assert_eq!([f.inputs[fwd], f.inputs[lat], f.inputs[yaw]], [0.0; 3]);
    assert!(c.motion_json()["last_apply_error"].is_null());
}

/// Save through the one controller handler the button, system_ui and REST
/// use: the files re-read as the shared recording types with the run's
/// completed_steps; protected paths and existing files are refused.
#[test]
fn preset_recordings_save_the_shared_type_and_refuse_protected_paths_and_overwrites() {
    use sim_runtime::embedded::EmbeddedRecording;
    use sim_runtime::environment::EnvironmentRecording;
    let dir = std::env::temp_dir().join(format!("robot-recording-{}-{}", std::process::id(), recording::now_ms()));
    let wait_save = |c: &mut RunController| wait(c, "save", |c| c.save_pending().is_none());
    // --robot FILE keeps no recording.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let model = crate::robot::load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap().model;
    let e = RunController::spawn(model).save_recording(None, None).unwrap_err();
    assert!(e.contains("`--robot FILE`"), "{e}");
    // Environment preset (a task): EmbeddedEnvironment::episode_recording().
    let (_, run) = preset("pendulum-environment").unwrap();
    assert_eq!(run.kind(), "EmbeddedEnvironment");
    let mut c = RunController::spawn_preset(Arc::new(run));
    let e = c.save_recording(None, None).unwrap_err();
    assert!(e.contains("no built session yet"), "{e}");
    assert_eq!(c.recording_json()["error"], json!(e));
    for n in 1..=3 {
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "step", |c| c.frame().is_some_and(|f| f.steps == n));
    }
    let steps = c.frame().unwrap().completed_steps.unwrap();
    assert!(steps > 0);
    let path = dir.join("env.json");
    assert_eq!(c.save_recording(Some(path.to_str().unwrap()), Some("three chunks")).unwrap(), path);
    wait_save(&mut c);
    let saved = c.saved().unwrap_or_else(|| panic!("not saved: {:?}", c.save_error())).clone();
    assert_eq!((saved.kind.as_str(), saved.completed_steps, saved.replayable, &saved.path), ("sampled_environment_recording", steps as usize, true, &path));
    let text = std::fs::read_to_string(&path).unwrap();
    let record: EnvironmentRecording = serde_json::from_str(&text).unwrap();
    assert_eq!((record.runtime.completed_steps, record.error.is_none()), (steps as usize, true));
    // The file is the shared type exactly as serde_json writes it.
    assert_eq!(serde_json::to_string(&record).unwrap(), text);
    let meta: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("env.meta.json")).unwrap()).unwrap();
    assert_eq!((meta["preset"]["id"].as_str(), meta["note"].as_str(), meta["completed_steps"].as_u64(), meta["seed"].as_u64()), (Some("pendulum-environment"), Some("three chunks"), Some(steps), Some(0)));
    assert_eq!(meta["runtime_identity"], json!(record.runtime.runtime_identity));
    let state = c.recording_json();
    assert_eq!((state["last_saved"]["completed_steps"].as_u64(), state["last_saved"]["path"].as_str()), (Some(steps), path.to_str()));
    // Never overwritten: the second save to the same path is refused by the writer and the file is unchanged.
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "step", |c| c.frame().is_some_and(|f| f.steps == 4));
    c.save_recording(Some(path.to_str().unwrap()), None).unwrap();
    wait_save(&mut c);
    let e = c.save_error().unwrap();
    assert!(e.contains("already exists") && e.contains("never overwritten"), "{e}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert_eq!(c.saved().unwrap().completed_steps, steps as usize);
    // Protected directories, relative or through `..`, are refused before anything is sent.
    for p in ["examples/interactive/x.json", "runs/../cad/x.json", "web/x.json"] {
        let e = c.save_recording(Some(p), None).unwrap_err();
        assert!(e.contains("refused") && e.contains("never written under examples/, cad/ or web/"), "{p}: {e}");
        assert!(!c.preset().unwrap().root.join(p).exists());
    }
    assert!(c.save_recording(Some(dir.join("x.meta.json").to_str().unwrap()), None).unwrap_err().contains(".meta.json"));
    // The default location rule, without writing.
    let t = recording::target(&c.preset().unwrap().root, "pendulum-environment", None, 1_790_748_502_729).unwrap();
    assert!(t.ends_with("runs/robot-presets/pendulum-environment/20260930T060822.729Z.json"), "{}", t.display());
    // Session preset (no task): EmbeddedSession::recording().
    let (_, run) = preset("pendulum-embedded").unwrap();
    let mut c = RunController::spawn_preset(Arc::new(run));
    c.act(RunAction::Step).unwrap();
    wait(&mut c, "step", |c| c.frame().is_some_and(|f| f.steps == 1));
    let steps = c.frame().unwrap().completed_steps.unwrap() as usize;
    let path = dir.join("session.json");
    c.save_recording(Some(path.to_str().unwrap()), None).unwrap();
    wait_save(&mut c);
    let record: EmbeddedRecording = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!((record.kind.as_str(), record.version, record.completed_steps), ("embedded_session", 3, steps));
    assert_eq!((c.saved().unwrap().kind.as_str(), c.saved().unwrap().replayable), ("embedded_session", true));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Replay through the one controller handler the buttons, system_ui and
/// REST use, on both kinds: a run with an input change saved and replayed
/// reaches done with the runtime's verdict and the same completed_steps;
/// refusals during a replay name it; Cancel gives cancelled (never done);
/// mismatches are refused with the runtime's message (environment) or the
/// labelled identity check (session). Inputs are changed through
/// `Sim::set_action` (the motion handler's setter): the pendulum presets
/// have typed inputs but no motion config.
#[test]
fn preset_replay_reaches_the_runtime_verdict_cancels_and_refuses_mismatches() {
    let dir = std::env::temp_dir().join(format!("robot-replay-{}-{}", std::process::id(), recording::now_ms()));
    let changed = |c: &RunController| -> Vec<f64> { c.preset_drive().unwrap().inputs.iter().map(|ch| ch.initial + 0.5 * (ch.upper - ch.initial)).collect() };
    // Steps `n` chunks, changing every input after the first, then saves to `name`.
    let record = |c: &mut RunController, n: u64, name: &str| -> (std::path::PathBuf, u64) {
        c.act(RunAction::Step).unwrap();
        wait(c, "step", |c| c.frame().is_some_and(|f| f.steps == 1) && c.preset_drive().is_some());
        let values = changed(c);
        c.set_inputs(values.clone());
        wait(c, "inputs", |c| c.frame().is_some_and(|f| f.inputs == values));
        for k in 2..=n {
            c.act(RunAction::Step).unwrap();
            wait(c, "step", |c| c.frame().is_some_and(|f| f.steps == k));
        }
        let path = dir.join(name);
        c.save_recording(Some(path.to_str().unwrap()), Some("input changed after chunk 1")).unwrap();
        wait(c, "save", |c| c.save_pending().is_none());
        assert!(c.save_error().is_none(), "{:?}", c.save_error());
        (path, c.frame().unwrap().completed_steps.unwrap())
    };
    let replayed = |c: &mut RunController| wait(c, "replay end", |c| c.replay_state().phase != ReplayPhase::Replaying);

    // Environment preset (pendulum-environment): EmbeddedEnvironment::prepare_replay.
    let (_, run) = preset("pendulum-environment").unwrap();
    let mut c = RunController::spawn_preset(Arc::new(run));
    let (env_path, steps) = record(&mut c, 3, "env.json");
    let text = std::fs::read_to_string(&env_path).unwrap();
    let rec: sim_runtime::environment::EnvironmentRecording = serde_json::from_str(&text).unwrap();
    assert!(!rec.runtime.input_events.is_empty(), "the input change is in the recording");
    let generation = c.generation();
    c.replay(None, Some(env_path.to_str().unwrap())).unwrap();
    assert_eq!((c.generation(), c.replay_state().phase), (generation + 1, ReplayPhase::Replaying));
    // Refused during the replay, naming it.
    for e in [c.check_motion_request(&MotionRequest::Key('w')).unwrap_err(), c.save_recording(None, None).unwrap_err(), c.replay(None, Some(env_path.to_str().unwrap())).unwrap_err(),
        c.act(RunAction::Pause).unwrap_err(), c.act(RunAction::Step).unwrap_err(), c.act(RunAction::Start).unwrap_err()] {
        assert!(e.contains("replay") && e.contains("in progress"), "{e}");
    }
    replayed(&mut c);
    let r = c.replay_state().clone();
    assert_eq!(r.phase, ReplayPhase::Done, "{r:?}");
    assert!(r.replaced && r.verdict.as_deref().unwrap().starts_with("passed the shared runtime's replay checks: EmbeddedEnvironment::prepare_replay"), "{r:?}");
    assert_eq!((r.completed_steps, r.recorded_completed_steps, r.completed, r.total, r.unit), (Some(steps), Some(steps), 3, Some(3), Some("actions")));
    let m = r.measured.as_ref().expect("the sidecar has final_frame");
    assert_eq!(m["label"], "measured difference, not a pass criterion");
    // The replayed run is the current paused run at the recorded state and inputs; Run works again.
    wait(&mut c, "replayed frame", |c| c.frame().is_some_and(|f| f.generation == generation + 1 && f.completed_steps == Some(steps)));
    assert_eq!((c.phase(), c.frame().unwrap().inputs.clone()), (Phase::Paused, changed(&c)));
    assert!(c.check(RunAction::Start).is_ok() && c.check_save().is_ok());
    // Mismatch: the same recording with a changed task is refused verbatim by the runtime; the run is unchanged.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["task"]["rewards"][0]["weight_per_s"] = json!(3.0);
    let other = dir.join("env-other-task.json");
    std::fs::write(&other, v.to_string()).unwrap();
    c.replay(None, Some(other.to_str().unwrap())).unwrap();
    replayed(&mut c);
    let r = c.replay_state().clone();
    assert_eq!((r.phase, r.replaced), (ReplayPhase::Failed, false));
    assert_eq!(r.error.as_deref(), Some("EmbeddedEnvironment::prepare_replay refused it: replay must match loaded robot, controller and task"));
    wait(&mut c, "republished", |c| c.frame().is_some_and(|f| f.generation == c.generation()));
    assert_eq!((c.phase(), c.frame().unwrap().completed_steps), (Phase::Paused, Some(steps)));

    // Session preset (pendulum-policy): viewer identity check, then EmbeddedSession::prepare_replay.
    let (_, run) = preset("pendulum-policy").unwrap();
    assert_eq!(run.kind(), "EmbeddedSession");
    let mut c = RunController::spawn_preset(Arc::new(run));
    let (path, steps) = record(&mut c, 6, "session.json");
    // A recording of the other kind is refused naming both kinds.
    c.replay(None, Some(env_path.to_str().unwrap())).unwrap();
    replayed(&mut c);
    let e = c.replay_state().error.clone().unwrap();
    assert!(e.contains("`sampled_environment_recording`") && e.contains("`embedded_session`"), "{e}");
    c.replay(None, Some(path.to_str().unwrap())).unwrap();
    replayed(&mut c);
    let r = c.replay_state().clone();
    assert_eq!(r.phase, ReplayPhase::Done, "{r:?}");
    assert!(r.verdict.as_deref().unwrap().contains("replay_expected satisfied"), "{r:?}");
    assert_eq!((r.completed_steps, r.recorded_completed_steps, r.completed, r.total), (Some(steps), Some(steps), steps, Some(steps)));
    wait(&mut c, "replayed frame", |c| c.frame().is_some_and(|f| f.completed_steps == Some(steps)));
    assert_eq!(c.frame().unwrap().inputs, changed(&c));
    // Cancel: stops between chunks; cancelled, never done; the partial run is held until Reset.
    c.replay(None, Some(path.to_str().unwrap())).unwrap();
    c.cancel_replay().unwrap();
    replayed(&mut c);
    let r = c.replay_state().clone();
    assert_eq!(r.phase, ReplayPhase::Cancelled, "{r:?}");
    assert!(r.completed < steps && r.verdict.as_deref().unwrap().starts_with("cancelled"), "{r:?}");
    // How far it got: the progress at cancel and the sim time reached (a preset's units are not one fixed span).
    assert!(r.verdict.as_deref().unwrap().starts_with(&format!("cancelled at {}, sim time ", r.progress())), "{r:?}");
    assert!(c.act(RunAction::Start).unwrap_err().contains("cancelled partial replay"));
    assert!(c.cancel_replay().unwrap_err().contains("no replay in progress"));
    c.act(RunAction::Reset).unwrap();
    assert_eq!(c.replay_state().phase, ReplayPhase::Idle);
    wait(&mut c, "reset", |c| c.frame().is_some_and(|f| f.completed_steps == Some(0)) && c.phase() == Phase::Paused);
    assert!(c.check(RunAction::Start).is_ok());
    // Another preset's recording (pendulum-embedded: another config) is refused by the labelled identity check.
    let (_, run) = preset("pendulum-embedded").unwrap();
    let mut e = RunController::spawn_preset(Arc::new(run));
    let (embedded, _) = { e.act(RunAction::Step).unwrap(); wait(&mut e, "step", |c| c.frame().is_some()); let p = dir.join("embedded.json"); e.save_recording(Some(p.to_str().unwrap()), None).unwrap(); wait(&mut e, "save", |c| c.save_pending().is_none()); (p, ()) };
    c.replay(None, Some(embedded.to_str().unwrap())).unwrap();
    replayed(&mut c);
    let e = c.replay_state().error.clone().unwrap();
    assert!(e.starts_with("refused by the viewer identity check (as sim-web's): the recording's config (controller recipe) differs from preset `pendulum-policy`'s"), "{e}");
    // --robot FILE: replay refused naming the mode.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let model = crate::robot::load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap().model;
    assert!(RunController::spawn(model).replay(None, Some("x.json")).unwrap_err().contains("`--robot FILE`"));
    // Listing: *.json but not *.meta.json, with the sidecar summary.
    let listed_root = dir.join("root");
    let preset_dir = listed_root.join(recording::DIR).join("pendulum-policy");
    std::fs::create_dir_all(&preset_dir).unwrap();
    for f in ["session.json", "session.meta.json"] {
        std::fs::copy(dir.join(f), preset_dir.join(f)).unwrap();
    }
    let list = recording::list(&listed_root, "pendulum-policy").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].file.as_str(), list[0].meta.as_ref().unwrap()["completed_steps"].as_u64()), ("session.json", Some(steps)));
    assert!(recording::list(&listed_root, "none").unwrap().is_empty());
    assert!(recording::replay_source(&listed_root, "p", Some("../x.json"), None).unwrap_err().contains("bare file name"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Timing only (debug build): `ROBOT_PRESET=<id> cargo test -p sim-spatial --lib run::tests::measure_full_robot_preset -- --ignored --nocapture`
/// (default robot-measured-400hz). Builds on this thread, as the run thread does.
#[test]
#[ignore]
fn measure_full_robot_preset() {
    let id = std::env::var("ROBOT_PRESET").unwrap_or_else(|_| "robot-measured-400hz".into());
    println!("preset {id}");
    let t0 = Instant::now();
    let (loaded, run) = preset(&id).unwrap();
    let parse = t0.elapsed().as_secs_f64();
    println!("parse+load: {parse:.2} s, {} links, {} chunk steps × {} s = {} s, {}", loaded.model.links.len(), run.chunk_steps(), run.step_s(), run.chunk_s(), run.kind());
    let run = Arc::new(run);
    let t1 = Instant::now();
    let mut sim = match Sim::build(&Source::Preset(run.clone()), &mut None) {
        Ok(sim) => sim,
        Err(e) => panic!("build failed after {:.2} s: {e}", t1.elapsed().as_secs_f64()),
    };
    println!("build: {:.2} s; inputs {:?}", t1.elapsed().as_secs_f64(), match &sim { Sim::Environment { env, held, .. } => (env.inputs().iter().map(|c| c.name.clone()).collect::<Vec<_>>(), held.clone()), _ => (vec![], vec![]) });
    let links: Vec<String> = loaded.model.links.iter().map(|l| l.name.clone()).collect();
    let f = sim.frame(&links, 0, 0, OverlayFlags::default()).unwrap();
    println!("frame: unmatched {:?}, links without pose {:?}", f.unmatched, f.poses.iter().zip(&links).filter(|(p, _)| p.is_none()).map(|(_, l)| l).collect::<Vec<_>>());
    for k in 1..=5 {
        let t = Instant::now();
        sim.advance().unwrap();
        let wall = t.elapsed().as_secs_f64();
        let frame_t = Instant::now();
        let f = sim.frame(&links, 0, k, OverlayFlags::default()).unwrap();
        println!("chunk {k}: advance {wall:.3} s, frame {:.3} s, sim t {:.4} s, rtf {:.4}, ended {:?}", frame_t.elapsed().as_secs_f64(), f.time, run.chunk_s() / wall, sim.ended());
    }
}

mod drive {
    //! The drive session's parts that need the run thread's protocol (no
    //! Python process is started here): the drained command batch through
    //! TwistState, the not-running refusal, and the replay identity refusal.
    //! TwistState's own rules (requests, the deadman on sim time, halt, the
    //! end of a replay) are tested with it in `sim_runtime::drive_host`.
    use super::super::controlled::{ControlledRun, differences};
    use sim_runtime::drive_host::TwistState;
    use super::super::replay::{ReplayPhase, ReplayState, prepare_replay};
    use super::super::Source;
    use sim_domain_control::drive::kinematics::{BodyTwist, Deadman, Limits, OnLoss};
    use std::sync::Arc;

    const PERIOD: f64 = 0.02;
    fn limits() -> Limits {
        Limits { supported: [true, false, true], max_speed: [0.3, 0.0, 3.0], max_accel: [0.6, 0.6, 6.0] }
    }
    fn ramp() -> Deadman {
        Deadman { timeout_s: 0.5, on_loss: OnLoss::Ramp { decel: [1.2, 1.2, 12.0] } }
    }

    #[test]
    fn a_stop_queued_behind_many_requests_governs_the_next_period() {
        use super::super::protocol::Command;
        use super::super::worker::drain;
        let (l, d) = (limits(), ramp());
        // A run thread that fell behind: fifty held-axis requests, then the release (a stop) and a Pause.
        let (tx, rx) = std::sync::mpsc::channel();
        for _ in 0..50 {
            tx.send(Command::Twist { request: BodyTwist::new(0.3, 0.0, 1.0), halt: false }).unwrap();
        }
        tx.send(Command::Twist { request: BodyTwist::ZERO, halt: false }).unwrap();
        tx.send(Command::Pause).unwrap();
        // One pass drains all of them, in order, without blocking.
        let (batch, closed) = drain(&rx, false).unwrap();
        assert_eq!((batch.len(), closed), (52, false));
        assert!(matches!(batch[51], Command::Pause) && matches!(batch[50], Command::Twist { halt: false, .. }));
        // Applied in order at one sim time, as the worker does: the stop wins the next period.
        let mut s = TwistState { commanded: BodyTwist::new(0.3, 0.0, 1.0), request: BodyTwist::new(0.3, 0.0, 1.0), ..TwistState::default() };
        for c in batch {
            if let Command::Twist { request, halt } = c {
                s.request(request, halt, 1.0, &l).unwrap();
            }
        }
        assert_eq!((s.request, s.heartbeat), (BodyTwist::ZERO, 51));
        let a = s.advance(1.0, PERIOD, &l, &d).unwrap();
        assert!((a[0] - (0.3 - 0.6 * PERIOD)).abs() < 1e-12 && a[3] == 51.0 && !s.expired, "{a:?}");
        // Nothing queued: an empty batch (running) and no wait.
        assert!(drain(&rx, false).is_some_and(|(b, closed)| b.is_empty() && !closed));
        // The controller gone: what was queued still comes out (closed), then nothing.
        tx.send(Command::Step).unwrap();
        drop(tx);
        let (batch, closed) = drain(&rx, true).unwrap();
        assert!(batch.len() == 1 && matches!(batch[0], Command::Step) && closed);
        assert!(drain(&rx, true).is_none());
        assert!(drain(&rx, false).is_some_and(|(b, closed)| b.is_empty() && closed));
    }

    /// The example binding (no Python process: the binding, profile and script are only read and hashed).
    fn wheeled() -> Arc<ControlledRun> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("examples/wheeled-robot/baseline/robot.simrobot.json");
        let model = crate::robot::load(&path).unwrap().model;
        let binding = sim_runtime::controller_binding::binding_path_for(&path);
        let controlled = sim_runtime::controller_binding::load(&binding, &model).unwrap();
        Arc::new(ControlledRun::new(&path, model, controlled))
    }

    #[test]
    fn a_nonzero_drive_request_is_refused_unless_running_and_a_stop_is_always_accepted() {
        use super::super::{Phase, RunController};
        let run = wheeled();
        // Nothing is built until Run or Step, so no Python process starts here.
        let mut c = RunController::spawn_file(run.model.clone(), Some(Ok(run.clone())), None, 0);
        // Idle: a stop and a halt are accepted (kept by the run thread for the build); a nonzero request is refused by name.
        c.drive(BodyTwist::ZERO, false).unwrap();
        c.drive(BodyTwist::ZERO, true).unwrap();
        assert_eq!(c.twist_requested, Some((BodyTwist::ZERO, true)));
        let e = c.drive(BodyTwist::new(0.1, 0.0, 0.0), false).unwrap_err();
        assert!(e.contains("phase idle") && e.contains("press Run first"), "{e}");
        assert_eq!(c.twist_refusal.as_deref(), Some(e.as_str()));
        assert_eq!(c.twist_requested, Some((BodyTwist::ZERO, true)), "a refused request is not sent");
        // Paused: the same rule (the UI's last Run/Pause decides; commands are ordered).
        c.status.phase = Phase::Paused;
        let e = c.drive(BodyTwist::new(0.0, 0.0, 1.0), false).unwrap_err();
        assert!(e.contains("phase paused") && e.contains("press Run first"), "{e}");
        assert!(c.check_drive_request(BodyTwist::ZERO, false).is_ok() && c.drive_json()["accepts_motion"] == false);
        c.drive(BodyTwist::ZERO, false).unwrap();
        assert!(c.twist_refusal.is_none());
        // A nonzero request that raced a Pause reaches the run thread while it is not running: dropped there, named.
        c.running = true;
        c.drive(BodyTwist::new(0.1, 0.0, 0.0), false).unwrap();
        super::wait(&mut c, "the run thread's drop of a request made while not running", |c| c.twist_error.is_some());
        let e = c.twist_error.clone().unwrap();
        assert!(e.contains("was not running when it arrived") && e.contains("press Run first"), "{e}");
        c.running = false;
    }

    #[test]
    fn a_replay_with_another_controller_is_refused_naming_each_difference() {
        let run = wheeled();
        assert_eq!((run.stem(), run.seed), ("robot".to_string(), 0));
        assert!(differences(&run, &run.scene).is_empty(), "the run's own scene matches");
        let mut scene = run.scene.clone();
        let ext = scene.controller.as_mut().unwrap().external.as_mut().unwrap();
        ext.script_sha256 = "0".repeat(64);
        ext.args.insert(0, "--gain".into());
        let diffs = differences(&run, &scene);
        // The shared ControllerIdentity::differences wording (sim_runtime::controller_binding).
        assert!(diffs.iter().any(|d| d.starts_with("script_sha256: recorded 0000")), "{diffs:?}");
        assert!(diffs.iter().any(|d| d.starts_with("args: recorded [\"--gain\"], current []")), "{diffs:?}");
        // Through the run thread's replay preparation: refused before any Session is built.
        let file = std::env::temp_dir().join(format!("drive-identity-{}.recording.json", std::process::id()));
        let recording = sim_runtime::session::Recording { version: 1, scene, seed: 0, actions: vec![vec![0.0, 0.0, 0.0, 0.0]] };
        std::fs::write(&file, serde_json::to_string(&recording).unwrap()).unwrap();
        let state = ReplayState::new(1, 1, Some(file.clone()), ReplayPhase::Replaying);
        let Err((e, state)) = prepare_replay(&Source::Controlled(run.clone()), None, &file, state) else { panic!("a different controller replays") };
        assert!(e.starts_with("refused by the drive identity check: ") && e.contains("script_sha256: recorded") && e.contains("args: recorded"), "{e}");
        assert_eq!((state.kind.as_deref(), state.recorded_completed_steps), (Some(crate::robot::recording::DRIVE_KIND), Some(1)));
        let _ = std::fs::remove_file(&file);
    }
}
