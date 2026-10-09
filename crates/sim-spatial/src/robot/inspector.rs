//! The header status lines, the sectioned inspector text, and the speed and
//! overlay button panels.
use super::*;

/// Status line and the sectioned inspector.
#[allow(clippy::too_many_arguments)]
pub(super) fn panels(
    view: Res<RobotView>,
    selection: Res<Selection>,
    registry: Res<DocumentRegistry>,
    mut title: Single<&mut Text, (With<TitleText>, Without<StatusText>, Without<Inspector>, Without<RunText>)>,
    mut status: Single<&mut Text, (With<StatusText>, Without<Inspector>, Without<RunText>, Without<TitleText>)>,
    mut inspector: Single<&mut Text, (With<Inspector>, Without<StatusText>, Without<RunText>, Without<TitleText>)>,
    mut run_text: Single<&mut Text, (With<RunText>, Without<StatusText>, Without<Inspector>, Without<TitleText>)>,
    mut reload: Query<&mut Node, With<ReloadButton>>,
) {
    // A REST robot_preset replaces a FILE view: presets are not reloaded.
    let display = if view.source.is_some() { Display::Flex } else { Display::None };
    for mut node in &mut reload {
        if node.display != display {
            node.display = display;
        }
    }
    let heading = match &view.preset {
        None if view.drive_preset.is_some() => {
            let p = view.drive_preset.as_ref().expect("checked");
            format!("Robot preset — {} ({})  ·  drive · {}  ·  files read-only", p.label, p.id, super::controls::DRIVE_CONTROLLER)
        }
        Some(p) if p.is_recorded() => format!("Robot preset — {} ({})  ·  {}", p.label, p.id, crate::robot::preset::RECORDED_LABEL),
        Some(p) => format!("Robot preset — {} ({})  ·  files read-only", p.label, p.id),
        None if view.planar.is_some() => format!("Robot — {}  ·  {}  ·  file read-only", view.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(), planar::HEADER_LABEL),
        // A controlled run (the model's `<stem>.controller.json` binding): its controller, named.
        None if controlled(&view) => format!("Robot — {}  ·  {}  ·  file read-only", view.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(), super::controls::DRIVE_CONTROLLER),
        None => format!("Robot — {}  ·  file read-only", view.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
    };
    if title.0 != heading {
        title.0 = heading;
    }
    let run_line = match &view.run {
        // A planar (v2) file: its own run thread (`robot_planar`).
        None if view.planar.is_some() => {
            let refused = view.run_message.as_ref().map_or(String::new(), |m| format!(" · refused: {}", clip(m, 60)));
            view.planar.as_ref().map_or(String::new(), |p| format!("{}{refused}", planar::run_line(p)))
        }
        None => String::new(),
        Some(r) if r.recorded().is_some() => {
            let refused = view.run_message.as_ref().map_or(String::new(), |m| format!(" · refused: {}", clip(m, 60)));
            r.playback().map_or(String::new(), |p| format!("{}{refused}", recorded_line(p)))
        }
        Some(r) => {
            let time = r.frame().map_or("t —".to_string(), |f| format!("t {:.2} s · {} chunks", f.time, f.steps));
            let rtf = r.rtf().map_or(String::new(), |x| format!(" · RTF {x:.2}"));
            // The requested scale while running or paused, and whether compute kept it from being reached.
            let speed = match r.phase() {
                run::Phase::Running | run::Phase::Paused => format!(" · ×{}{}", r.speed_scale(), if r.compute_limited() == Some(true) { " (compute-limited)" } else { "" }),
                _ => String::new(),
            };
            // The header has one line beside the subtitle: long messages are cut here and shown in full in the inspector.
            let error = r.error().map_or(String::new(), |e| format!(" · {} (full error in the inspector)", clip(e, 40)));
            let ended = r.end().and_then(|e| e["message"].as_str()).map_or(String::new(), |m| format!(" · {} (see the inspector)", clip(m, 40)));
            let error = format!("{error}{ended}");
            let refused = view.run_message.as_ref().map_or(String::new(), |m| format!(" · refused: {}", clip(m, 60)));
            let phase = serde_json::to_value(r.phase()).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let replay = r.replay_state();
            let replay = match replay.phase {
                ReplayPhase::Idle => String::new(),
                phase => format!(" · replay {} {}/{}", format!("{phase:?}").to_lowercase(), replay.completed, replay.total.map_or("?".into(), |t| t.to_string())),
            };
            format!("{phase} · {time}{rtf}{speed} · gen {} · {} s chunks{replay}{error}{refused}", r.generation(), r.chunk_s())
        }
    };
    if run_text.0 != run_line {
        run_text.0 = run_line;
    }
    let line = match (&view.status, &view.model) {
        (Status::Loading(t), _) => format!("Loading {} on a worker thread… {:.1} s", view.path.display(), t.elapsed().as_secs_f64()),
        (Status::Error(e), _) => format!("Could not open the robot: {e}"),
        (Status::Loaded { seconds }, Some(m)) => {
            let without = view.triangles.iter().filter(|t| **t == 0).count();
            let missing = if without > 0 { format!(" · {without} without collision geometry (listed, not drawn)") } else { String::new() };
            // Same pose-state rule as robot_state.pose (GAIT_POSE, SIMULATED_POSE, POSE), short form.
            let previewing = view.run.as_ref().and_then(|r| r.gait_preview()).and_then(|g| g.poses()).is_some();
            let simulated = if controlled(&view) { CONTROLLED_POSE } else { "simulated pose" };
            let pose = if view.run.as_ref().is_some_and(|r| r.recorded().is_some()) { "recorded pose (not simulated here)" } else if previewing { "kinematic gait preview pose (not physics)" } else if view.run.as_ref().and_then(|r| r.frame()).is_some() { simulated } else { "exported assembly pose" };
            match view.preset.as_ref() {
                Some(p) => format!("{} links{missing} · loaded in {seconds:.2} s · {pose} · readiness: {}", m.links.len(), clip(p.readiness().unwrap_or("(none declared)"), 55)),
                None => match view.source.as_ref().filter(|s| s.failing.is_some()) {
                    // The header says the displayed model is the last good one; the full error is in Source.
                    Some(s) => format!("SHOWING LAST GOOD MODEL (loaded {}) · reload failed: {} (full error in Source)", s.loaded_at.as_deref().unwrap_or("—"), clip(s.failing.as_deref().unwrap_or_default(), 70)),
                    None => {
                        let notice = view.notice.as_ref().map_or(String::new(), |n| format!("{} · ", clip(n, 90)));
                        format!("{notice}{} · {} links{missing} · loaded in {seconds:.2} s · {pose}", view.path.display(), m.links.len())
                    }
                },
            }
        }
        (Status::Loaded { seconds }, None) => match (&view.planar, view.source.as_ref().filter(|s| s.failing.is_some())) {
            (None, _) => String::new(),
            (Some(_), Some(s)) => format!("SHOWING LAST GOOD MODEL (loaded {}) · reload failed: {} (full error in Source)", s.loaded_at.as_deref().unwrap_or("—"), clip(s.failing.as_deref().unwrap_or_default(), 70)),
            (Some(p), None) => {
                let notice = view.notice.as_ref().map_or(String::new(), |n| format!("{} · ", clip(n, 90)));
                format!("{notice}{} · {} bodies · {} joints in file · loaded in {seconds:.2} s · {}", view.path.display(), p.loaded.model.bodies.len(), p.loaded.model.joints.len(), planar::HEADER_LABEL)
            }
        },
    };
    if status.0 != line {
        status.0 = line;
    }
    let link = picked::link(&selection, &registry);
    let body = match &view.model {
        // The Comments section is drawn by `threads` under this text.
        _ if view.section == Section::Comments => String::new(),
        None => view.planar.as_ref().map_or(String::new(), |p| {
            let watch = view.source.as_ref().map(file_watch_text).unwrap_or_default();
            planar::inspector_text(p, &view.section.label().to_lowercase(), link, &watch)
        }),
        Some(m) => match view.section {
            Section::Run => readouts::text(&view),
            Section::Link => link_text(&view, m, link),
            Section::Joints => joints_text(&view, m, link),
            Section::Drives => drives_text(&view, m),
            Section::Source => source_text(&view, m),
            Section::Comments => String::new(),
        },
    };
    let failure = view.run.as_ref().and_then(|r| r.error().map(|e| format!("RUN FAILED: {e}\n\n")));
    let ended = view.run.as_ref().and_then(|r| r.end()).map(|e| format!("RUN ENDED ({}): {}\n\n", e["kind"].as_str().unwrap_or(""), e["message"].as_str().unwrap_or("")));
    let failure = Some(format!("{}{}{}{}", failure.unwrap_or_default(), ended.unwrap_or_default(), preset_text(&view), drive_recording_text(&view)));
    let refused = view.run_message.as_ref().map(|m| format!("Refused: {m}\n\n"));
    let body = format!("{}{}{body}", failure.unwrap_or_default(), refused.unwrap_or_default());
    if inspector.0 != body {
        inspector.0 = body;
    }
}

/// The header's short pose label for a controlled run's frames (the Drive
/// block states the fidelity in full, `controls::drive::DRIVE_FIDELITY`).
const CONTROLLED_POSE: &str = "simulated pose (PhysicalRobot physics; the external controller replaces the hold coupler)";

/// The run is driven by an external controller (`RunController::controlled`).
pub(super) fn controlled(view: &RobotView) -> bool {
    view.run.as_ref().is_some_and(|r| r.controlled().is_some())
}

/// The preset's identity and readiness (verbatim) atop every inspector
/// section; its evidence text is added on the Source section.
fn preset_text(view: &RobotView) -> String {
    let Some(p) = &view.preset else { return String::new() };
    let mut t = format!("PRESET {} ({})\nreadiness (verbatim): {}\n", p.label, p.id, p.readiness().unwrap_or("(none declared)"));
    if let Some(rec) = view.run.as_ref().and_then(|r| r.recorded()) {
        let c = &rec.capture;
        let src = c.meta.source.as_ref();
        let opt = |x: Option<String>| x.unwrap_or_else(|| "(absent in file)".into());
        t += &format!("{}\n", crate::robot::preset::RECORDED_LABEL.to_uppercase());
        if let Some(f) = view.run.as_ref().and_then(|r| r.frame()) {
            t += &format!("frame {} of {} · t {} s · recorded {} s\n", f.steps, c.frames.len(), f.time, c.duration_s());
        }
        t += &format!("fidelity (file): {}\ncad_sha256 (file): {}\ncompleted (file): {} · simulated {} s in {} s stepping wall (rate {})\n",
            opt(src.and_then(|s| s.fidelity.clone())), opt(src.and_then(|s| s.cad_sha256.clone())), opt(c.meta.completed.map(|x| x.to_string())),
            opt(c.meta.simulated_s.map(|x| x.to_string())), opt(c.meta.stepping_wall_s.map(|x| format!("{x:.2}"))), opt(c.meta.recorded_rate().map(|x| format!("{x:.4}"))));
        if !rec.unmatched.is_empty() {
            t += &format!("capture links matching no scene link: {}\n", rec.unmatched.join(", "));
        }
        for (k, path) in p.paths() {
            t += &format!("{k}: {path}\n");
        }
        t += &format!("description (verbatim): {}\n", p.entry.get("description").and_then(|d| d.as_str()).unwrap_or("(none declared)"));
        t += &format!("evidence (verbatim): {}\n", p.evidence().unwrap_or("(none declared)"));
        t += "Unavailable for a recorded preset (nothing is simulated here): run, jog, motion, save recording, replay, gait preview and overlays; each is refused naming the preset.\n\n";
        return t;
    }
    if let Some(run) = view.run.as_ref().and_then(|r| r.preset()) {
        let f = view.run.as_ref().and_then(|r| r.frame());
        t += &format!("{} · seed {} · step {} s · chunk {} steps ({} s) · {} / {} steps\n",
            run.kind(), run.seed, run.step_s(), run.chunk_steps(), run.chunk_s(), f.and_then(|f| f.completed_steps).map_or("—".into(), |n| n.to_string()), run.requested_steps());
        if let Some(f) = f.filter(|f| !f.unmatched.is_empty()) {
            t += &format!("frame links matching no loaded link: {}\n", f.unmatched.join(", "));
        }
    }
    let motion = view.run.as_ref().map(motion_text).unwrap_or_default();
    for (k, path) in p.paths() {
        t += &format!("{k}: {path}\n");
    }
    if view.section == Section::Source {
        t += &format!("evidence (verbatim): {}\n", p.evidence().unwrap_or("(none declared)"));
    }
    t.push('\n');
    t.push_str(&motion);
    if let Some(r) = view.run.as_ref() {
        t += &recording_text(r, false);
    }
    t
}

/// A controlled `--robot FILE` run's recording and replay text (its drive
/// Session's), atop every inspector section as a preset's is.
fn drive_recording_text(view: &RobotView) -> String {
    view.run.as_ref().filter(|r| r.controlled().is_some() && r.preset().is_none()).map_or(String::new(), |r| recording_text(r, true))
}

/// The RECORDING and REPLAY blocks: the last save, the saved recordings and
/// the replay's progress, verdict and error with their rules (`drive`: a
/// controlled run's drive Session, else a preset's shared session).
fn recording_text(r: &RunController, drive: bool) -> String {
    let mut t = String::new();
    t += if drive { "RECORDING — the drive Session's recording (Session::recording()), plus a .meta.json sidecar\n" } else { "RECORDING — the shared recording JSON, as the browser's Download, plus a .meta.json sidecar\n" };
    if let Some(s) = r.saved() {
        t += &format!("last saved: {}\n  sidecar {}\n  {} v{} · {} steps · {}\n", s.path.display(), s.meta_path.display(), s.kind, s.version, s.completed_steps,
            s.not_replayable_reason.as_deref().map_or("replayable".to_string(), |why| format!("not replayable: {why}")));
    }
    if let Some(e) = r.save_error() {
        t += &format!("last save error: {e}\n");
    }
    t += &format!("{}\n\n", if drive { recording::DRIVE_LOCATION_RULE } else { recording::LOCATION_RULE });
    let s = r.replay_state();
    t += if drive { "REPLAY — Session::new(recorded scene, recorded seed) on the run thread, after the identity check\n" } else { "REPLAY — the shared prepare_replay on the run thread; the verdict is the runtime's\n" };
    t += &format!("{} saved recording(s) for this {}\n", r.recordings().len(), if drive { "robot" } else { "preset" });
    if s.phase != ReplayPhase::Idle {
        t += &format!("{}\n", replay_line(r));
        if let Some(v) = &s.verdict {
            t += &format!("full verdict: {v}\n");
        }
        if let Some(e) = &s.error {
            t += &format!("full error: {e}\n");
        }
    }
    if drive {
        t += &format!("{}\n{}\n\n", recording::DRIVE_REPLAY_RULE, recording::DRIVE_VERDICT_RULE);
    } else {
        t += &format!("{}\n\n", recording::VERDICT_RULE);
    }
    t
}

/// The recorded timeline's status line (header and inspector Recorded section).
pub(super) fn recorded_line(p: &playback::RecordedPlayback) -> String {
    let tl = p.timeline();
    let f = p.frame();
    let phase = json!(tl.phase);
    format!("recorded · {} · t {:.3} s · frame {} / {} (t {:.3} s) · ×{} · gen {}", phase.as_str().unwrap_or(""), tl.t, f.steps, tl.times.len(), f.time, tl.speed, f.generation)
}

/// `s` cut to at most `n` characters, marked with an ellipsis when cut.
pub(super) fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// The speed buttons: dimmed when refused (at a limit, or no robot), the middle one shows ×scale.
pub(super) fn speed_panel(view: Res<RobotView>, mut buttons: Query<(&RobotAction, &mut Enabled, &SpeedLabel, &Children), With<SpeedButton>>, mut labels: Query<&mut Text>) {
    let scale = match &view.planar {
        Some(p) => p.run.speed_scale(),
        None => view.run.as_ref().map_or(1.0, RunController::speed_scale),
    };
    for (action, enabled, shows_scale, children) in &mut buttons {
        enable(enabled, check(&view, action).is_ok());
        if !shows_scale.0 {
            continue;
        }
        // The kit button's label is its text child.
        let want = format!("×{scale}");
        for child in children.iter() {
            if let Ok(mut text) = labels.get_mut(child) {
                if text.0 != want {
                    text.0 = want.clone();
                }
            }
        }
    }
}

/// A preset contact arrow's length per newton and its cap (the browser's ArrowHelper: `min(0.10, |f| × 0.004)`).
pub(super) const PRESET_ARROW_M_PER_N: f64 = 0.004;
pub(super) const PRESET_ARROW_MAX_M: f64 = 0.10;

/// The overlay block: each button carries the flip of its current flag and
/// shows on/off; the line under it gives the accepted frame's counts and the scales.
pub(super) fn overlay_panel(
    view: Res<RobotView>,
    mut buttons: Query<(&OverlayButton, &OverlayLabel, &mut RobotAction, &mut Look, &mut Node, &Children)>,
    mut labels: Query<&mut Text, Without<OverlayText>>,
    mut line: Single<&mut Text, With<OverlayText>>,
) {
    let run = view.run.as_ref();
    let available = run.is_some_and(|r| r.check_overlays().is_ok());
    // A preset's frames carry contacts (the browser's force arrows); the other run-thread overlays are FILE only.
    let preset_contacts = run.is_some_and(|r| r.preset().is_some() && r.check_contacts().is_ok());
    for (b, l, mut action, mut look, mut node, children) in &mut buttons {
        // A planar file: only the contacts chip (chain tips) has a meaning; the others are hidden (their refusals are in system_ui and REST).
        let available = if view.planar.is_some() { b.0 == "contacts" } else if b.0 == "contacts" && preset_contacts { true } else { available };
        let next = overlay_toggle(&view, b.0);
        if *action != next {
            *action = next;
        }
        let on = overlay_on(&view, b.0);
        look.set_if_neq(Look::Chip(on));
        let enabled = if b.0 == "stress" { check_stress(&view).is_ok() } else { available };
        let display = if enabled { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        // The chip's label (its text child): name, on/off and key.
        let (_, name, key) = OVERLAYS.iter().find(|o| o.0 == l.0).copied().unwrap_or(OVERLAYS[0]);
        let t = format!("{name} {} ({})", if overlay_on(&view, l.0) { "on" } else { "off" }, format!("{key:?}").trim_start_matches("Key"));
        for child in children.iter() {
            if let Ok(mut text) = labels.get_mut(child) {
                if text.0 != t {
                    text.0 = t.clone();
                }
            }
        }
    }
    let t = match run {
        None => view.planar.as_ref().map_or(String::new(), |p| {
            let tips = p.run.frame().filter(|f| f.built).map_or("—".into(), |f| f.tips.len().to_string());
            format!("Overlays · planar v2: {tips} chain-tip contact points (red dots; C) · joint frames, deflections and stress need a v3 export")
        }),
        Some(r) if r.preset().is_some() && r.check_contacts().is_ok() => {
            let n = r.frame().and_then(|f| f.extra.as_deref()).and_then(|x| x["contacts"].as_array()).map(Vec::len);
            format!("Overlays · preset frame: {} contacts (force arrows, at most {PRESET_ARROW_MAX_M} m, {PRESET_ARROW_M_PER_N} m/N; green on the ground, orange between links) · joint frames and deflections are --robot FILE overlays", n.map_or("—".into(), |n| n.to_string()))
        }
        Some(r) => match r.check_overlays() {
            Err(_) => "Overlays (contacts, joint frames, deflections): not available for presets".into(),
            Ok(()) => match r.frame() {
                None => format!("Overlays: no run frame yet (Run or Step) · force {} m/N · deflection ×{}", run::FORCE_SCALE_M_PER_N, run::DEFLECTION_MAGNIFICATION),
                Some(f) => {
                    let o = &f.overlays;
                    let count = |n: Option<usize>| n.map_or("—".into(), |n| n.to_string());
                    let max = o.deflections.as_ref().map(|d| d.iter().map(|d| d.displacement.iter().map(|x| x * x).sum::<f64>().sqrt()).fold(0.0, f64::max));
                    format!("Overlays · gen {} t {:.2} s: {} contacts · {} joint frames · {} deflection points{} · force {} m/N · deflection ×{}",
                        f.generation, f.time, count(o.contacts.as_ref().map(Vec::len)), count(o.joints.as_ref().map(Vec::len)), count(o.deflections.as_ref().map(Vec::len)),
                        max.filter(|m| *m > 0.0).map_or(String::new(), |m| format!(" (max {:.3} mm)", m * 1e3)), run::FORCE_SCALE_M_PER_N, run::DEFLECTION_MAGNIFICATION)
                }
            },
        },
    };
    if line.0 != t {
        line.0 = t;
    }
}
