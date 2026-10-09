//! Inspector control blocks: overlay, jog, motion, recording/replay,
//! recorded-timeline, gait-preview and drive (a controlled run's
//! teleoperation) controls and their status lines.
use super::*;
use super::ui::{DriveButton, DriveDetailRoot, DriveRoot, DriveText};
use crate::robot::DriveRequest;
use crate::drive_input::{DriveBindings, DriveInput, DriveTarget, LiveTarget};
use sim_domain_control::drive::kinematics;

mod drive;
pub(super) use drive::{DRIVE_CONTROLLER, drive_panel, drive_target};
#[cfg(test)]
pub(super) use drive::run_identity;

/// The overlays: (system_ui id suffix, label, key). H (hotspots) for stress: S is the WASD jog key.
pub(super) const OVERLAYS: [(&str, &str, KeyCode); 4] =
    [("contacts", "Contacts", KeyCode::KeyC), ("joints", "Joint frames", KeyCode::KeyJ), ("deflections", "Deflections", KeyCode::KeyF), ("stress", "Stress", KeyCode::KeyH)];
pub(super) const STRESS_PRESET: &str = "the stress overlay is not available for presets: it reads the .simresult.json beside a --robot FILE model (sim_runtime::physical::results_path); a preset scene has no results file";
pub(super) fn overlay_on(view: &RobotView, kind: &str) -> bool {
    if let Some(p) = &view.planar {
        // A planar file draws only the chain-tip contacts.
        return kind == "contacts" && p.contacts;
    }
    let flags = view.run.as_ref().map_or_else(OverlayFlags::default, RunController::overlays);
    match kind {
        "contacts" => flags.contacts,
        "joints" => flags.joints,
        "stress" => view.stress.enabled,
        _ => flags.deflections,
    }
}
/// Jog controls follow the one link selection: the non-fixed joints touching
/// the selected link, the same joints the Joints section lists. A joint links
/// two bodies, so selecting either one reaches it, and no second (joint)
/// selection state is needed. Joints without a servo target stay listed,
/// disabled with the reason.
/// `link`: the selected link (`picked::link`).
pub(super) fn jog_joints(view: &RobotView, link: Option<usize>) -> Vec<(String, f64)> {
    if let Some(p) = &view.planar {
        // A planar file: every simulated joint (planar models are small), by built order.
        return p.joint_names().iter().map(|j| (j.clone(), JOG_STEP_RAD)).collect();
    }
    if view.preset.is_some() || view.run.as_ref().is_some_and(|r| r.controlled().is_some()) {
        // A preset's joints are driven by its declared controller, a
        // controlled run's wheels by its external controller (which writes the
        // servo targets every period): no servo-target jog, which would be refused.
        return Vec::new();
    }
    let (Some(m), Some(i)) = (view.model.as_ref(), link) else { return Vec::new() };
    let Some(l) = m.links.get(i) else { return Vec::new() };
    touching(m, &l.name).filter(|(_, j)| j.kind != "fixed" && !j.is_loop()).map(|(_, j)| (j.name.clone(), if j.kind == "prismatic" { JOG_STEP_M } else { JOG_STEP_RAD })).collect()
}
/// The transport buttons: (system_ui id suffix, label, action).
pub(super) const RECORDED_TRANSPORT: [(&str, &str, RecordedAction); 5] = [
    ("start", "Start", RecordedAction::Start),
    ("step-", "Step −1", RecordedAction::Step { delta: -1 }),
    ("play", "Play", RecordedAction::Play),
    ("pause", "Pause", RecordedAction::Pause),
    ("step+", "Step +1", RecordedAction::Step { delta: 1 }),
];
/// Seek controls: (step in twelfths of the period, id suffix, label); step 0 seeks to t = 0.
pub(super) const GAIT_SEEK: [(i8, &str, &str); 3] = [(0, "0", "Seek gait to t = 0"), (-1, "-", "Seek gait −period/12"), (1, "+", "Seek gait +period/12")];
/// Speed-scale buttons, all within the shared Clock's (0, 1].
pub(super) const GAIT_SCALES: [f64; 3] = [0.25, 0.5, 1.0];
/// A seek by `step` twelfths of the period from the latest pose's gait time (wrapped into one period).
pub(super) fn gait_seek(view: &RobotView, step: i8) -> GaitAction {
    let g = view.run.as_ref().and_then(|r| r.gait_preview());
    let (t, period) = g.and_then(|g| Some((g.sample().map_or(0.0, |s| s.gait_time_s), g.loaded()?.period_s))).unwrap_or((0.0, 0.0));
    let t = if step == 0 || period <= 0.0 { 0.0 } else { (t + f64::from(step) * period / 12.0).rem_euclid(period) };
    GaitAction::Seek { t }
}
/// The motion controls (system_ui id, label, request): each key latches its request; Stop zeros.
pub(super) fn motion_buttons() -> [(&'static str, &'static str, MotionRequest); 5] {
    [
        ("motion:w", "W · Forward", MotionRequest::Key('w')),
        ("motion:a", "A · Left", MotionRequest::Key('a')),
        ("motion:s", "S · Back", MotionRequest::Key('s')),
        ("motion:d", "D · Right", MotionRequest::Key('d')),
        ("motion:stop", "Stop (X)", MotionRequest::Stop),
    ]
}
/// Replay buttons shown in the inspector until "More recordings…" expands the list to every recording.
const REPLAY_BUTTONS: usize = 5;

/// Jog rows for the joints touching the selected link: the joint's servo
/// state and −/+ buttons (the same `RobotAction::Jog` as `system_ui` jog:*).
#[allow(clippy::too_many_arguments)]
pub(super) fn jog_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    selection: Res<Selection>,
    registry: Res<DocumentRegistry>,
    fonts: Res<UiFonts>,
    root: Single<Entity, With<JogRoot>>,
    // Keyed by the root too: re-entering robot mode spawns a new, empty root.
    mut shown: Local<Option<(Entity, Vec<String>)>>,
    mut texts: Query<(&JogText, &mut Text)>,
    mut buttons: Query<(&RobotAction, &mut Enabled), With<JogButton>>,
) {
    let k = Kit { f: &fonts };
    let joints = jog_joints(&view, picked::link(&selection, &registry));
    let key = (*root, joints.iter().map(|(j, _)| j.clone()).collect::<Vec<String>>());
    if shown.as_ref() != Some(&key) {
        commands.entity(*root).despawn_related::<Children>();
        let mut rows = Vec::new();
        if !joints.is_empty() {
            rows.push(commands.spawn(k.section("Jog")).id());
            let label = if view.planar.is_some() { "planar v2 joint target (PD hold in the planar build) · ←/→ select · ↑/↓ move (Shift ×5)" } else { JOG_LABEL };
            rows.push(commands.spawn(k.text(label, size::CAPTION, SUBTLE, 0)).id());
        }
        for (joint, step) in &joints {
            let button = |sign: f64, text: &str| {
                let action = RobotAction::Jog { joint: joint.clone(), delta: sign * step };
                let enabled = check(&view, &action).is_ok();
                (k.button(text, action, Look::Secondary, enabled), JogButton)
            };
            rows.push(
                commands
                    .spawn((
                        Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), align_items: AlignItems::Center, ..default() },
                        children![button(-1.0, "−"), button(1.0, "+"), (k.text(joint.as_str(), size::CAPTION, TEXT, 0), JogText(joint.clone()))],
                    ))
                    .id(),
            );
        }
        commands.entity(*root).add_children(&rows);
        *shown = Some(key);
    }
    if let Some(p) = &view.planar {
        let f = p.run.frame().filter(|f| f.built);
        for (joint, mut text) in &mut texts {
            let i = p.joint_names().iter().position(|n| n == &joint.0);
            let selected = if i.is_some() && i == Some(p.selected_joint) { "▸ " } else { "" };
            let values = match (i, f) {
                (Some(i), Some(f)) => format!("angle {:+.3} · target {:+.3} rad", f.joint_angles.get(i).copied().unwrap_or(f64::NAN), f.targets.get(i).copied().unwrap_or(f64::NAN)),
                _ => "—".into(),
            };
            let line = format!("{selected}{} · {values}", joint.0);
            if text.0 != line {
                text.0 = line;
            }
        }
    } else if let Some(r) = &view.run {
        for (joint, mut text) in &mut texts {
            let line = format!("{} · {}", joint.0, jog_line(r, &joint.0));
            if text.0 != line {
                text.0 = line;
            }
        }
    }
    for (action, enabled) in &mut buttons {
        enable(enabled, check(&view, action).is_ok());
    }
}

/// W/A/S/D/Stop buttons and the requested values for a preset (the same
/// `RobotAction::Motion` as `system_ui` motion:*), then the Save recording
/// and Replay block, which a controlled `--robot FILE` run (its drive
/// Session) shows on its own; every button enabled per the handler's check.
#[allow(clippy::too_many_arguments)]
pub(super) fn motion_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    root: Single<Entity, With<MotionRoot>>,
    // The root each block was built in and whether it has the Motion part (a
    // preset) or not (a controlled run): re-entering robot mode spawns new,
    // empty roots, and a reload can bind or unbind a controller.
    mut shown: Local<Option<(Entity, bool)>>,
    ui: Res<RobotPanelUi>,
    mut listed: Local<Option<(Entity, Vec<String>, bool)>>,
    replay_list: Query<Entity, With<ReplayList>>,
    mut text: Query<(&mut Text, Has<RecordingText>, Has<ReplayText>), Or<(With<MotionText>, With<RecordingText>, With<ReplayText>)>>,
    mut buttons: Query<(&RobotAction, &mut Enabled), With<MotionButton>>,
) {
    let k = Kit { f: &fonts };
    let button = |commands: &mut Commands, action: RobotAction, text: &str| {
        let enabled = check(&view, &action).is_ok();
        commands.spawn((k.button(text, action, Look::Secondary, enabled), MotionButton)).id()
    };
    let controlled = view.run.as_ref().is_some_and(|r| r.controlled().is_some());
    // Some(true): a simulated preset (Motion + recording); Some(false): a controlled run (recording only).
    let want = if view.preset.as_ref().is_some_and(|p| !p.is_recorded()) { Some(true) } else if controlled && view.preset.is_none() { Some(false) } else { None };
    let Some(with_motion) = want else {
        // Another run (or none): a previous block goes (a reload that removed the binding).
        if shown.take().is_some() {
            commands.entity(*root).despawn_related::<Children>();
            *listed = None;
        }
        return;
    };
    // Built this frame: the old list (if any) is despawned with the commands, so it is not filled now.
    let rebuilt = *shown != Some((*root, with_motion));
    if rebuilt {
        commands.entity(*root).despawn_related::<Children>();
        let mut children = Vec::new();
        if with_motion {
            children.push(commands.spawn(k.section("Motion")).id());
            children.push(commands.spawn(k.text(motion::LABEL, size::CAPTION, SUBTLE, 0)).id());
            let row = commands.spawn(wrap()).id();
            for (_, text, request) in motion_buttons() {
                let b = button(&mut commands, RobotAction::Motion { request }, text);
                commands.entity(row).add_child(b);
            }
            children.push(row);
            children.push(commands.spawn((k.text("", size::CAPTION, TEXT, 0), MotionText)).id());
        } else {
            children.push(commands.spawn(k.section("Recording")).id());
            children.push(commands.spawn(k.text("the drive Session's recording (sim_runtime::session::Session::recording(): scene with the controller identity, seed, one twist + heartbeat action per seam period)", size::CAPTION, SUBTLE, 0)).id());
        }
        // Save recording: the same RobotAction::SaveRecording as system_ui recording:save and REST robot_save_recording.
        let save_row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).id();
        let save = button(&mut commands, RobotAction::SaveRecording { path: None, note: None }, "Save recording");
        let saved = commands.spawn((k.text("", size::CAPTION, TEXT, 0), RecordingText, Node { flex_shrink: 1.0, ..default() })).id();
        commands.entity(save_row).add_children(&[save, saved]);
        // Replay: the same RobotAction::Replay / CancelReplay as system_ui replay:<file> / replay:cancel and REST robot_replay.
        let replay_header = commands.spawn(k.section("Replay")).id();
        let how = if with_motion { "re-executed through the shared prepare_replay on the run thread" } else { "re-executed on the run thread: Session::new(recorded scene, recorded seed) restarts the recorded controller; refused when its identity differs from the loaded binding's" };
        let replay_label = commands.spawn(k.text(how, size::CAPTION, SUBTLE, 0)).id();
        let list = commands.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() }, ReplayList)).id();
        let replay_row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).id();
        let cancel = button(&mut commands, RobotAction::CancelReplay, "Cancel replay");
        let refresh = button(&mut commands, RobotAction::RefreshRecordings, "Refresh list");
        let replay_line = commands.spawn((k.text("", size::CAPTION, TEXT, 0), ReplayText, Node { flex_shrink: 1.0, ..default() })).id();
        commands.entity(replay_row).add_children(&[cancel, refresh]);
        children.extend([save_row, replay_header, replay_label, list, replay_row, replay_line]);
        commands.entity(*root).add_children(&children);
        *shown = Some((*root, with_motion));
        *listed = None;
    }
    if let (false, Some(r), Ok(list)) = (rebuilt, view.run.as_ref(), replay_list.single()) {
        // The most recent recordings first (all of them when expanded); rebuilt only when the listed files change.
        let expanded = ui.recordings_expanded;
        let count = if expanded { r.recordings().len() } else { REPLAY_BUTTONS };
        let files: Vec<String> = r.recordings().iter().rev().take(count).map(|l| l.file.clone()).collect();
        let key = (list, files, expanded);
        if listed.as_ref() != Some(&key) {
            commands.entity(list).despawn_related::<Children>();
            let mut rows = Vec::new();
            for l in r.recordings().iter().rev().take(count) {
                let summary = l.meta.as_ref().map_or("no sidecar".to_string(), |m| format!("{} steps{}{}", m["completed_steps"], if m["replayable"] == false { " · diagnostic" } else { "" }, m["note"].as_str().map_or(String::new(), |n| format!(" · {}", clip(n, 30)))));
                let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).id();
                let b = button(&mut commands, RobotAction::Replay { file: Some(l.file.clone()), path: None }, "Replay");
                let t = commands.spawn(k.text(format!("{} · {summary}", l.file), size::DETAIL, TEXT, 0)).id();
                commands.entity(row).add_children(&[b, t]);
                rows.push(row);
            }
            let more = r.recordings().len().saturating_sub(REPLAY_BUTTONS);
            if r.recordings().is_empty() {
                let none = if with_motion { "no saved recordings for this preset yet" } else { "no saved drive recordings for this robot yet" };
                rows.push(commands.spawn(k.text(none, size::DETAIL, SUBTLE, 0)).id());
            } else if more > 0 {
                // Every recording is replayable from the window: the toggle lists the older ones too.
                let label = if expanded { format!("Fewer recordings (the {REPLAY_BUTTONS} most recent)") } else { format!("More recordings… ({more} older)") };
                rows.push(commands.spawn(k.button(&label, PanelToggle::Recordings, Look::Ghost, true)).id());
            }
            commands.entity(list).add_children(&rows);
            *listed = Some(key);
        }
    }
    if let Some(r) = view.run.as_ref() {
        for (mut t, recording, replay) in &mut text {
            let line = if replay { replay_line(r) } else if recording { recording_line(r) } else { motion_line(r) };
            if t.0 != line {
                t.0 = line;
            }
        }
    }
    for (action, enabled) in &mut buttons {
        enable(enabled, check(&view, action).is_ok());
    }
}

/// The inspector's Recorded block for a recorded preset: the label, the
/// transport (Start, Step −1, Play, Pause, Step +1) and the timeline line. Each
/// button is the same `RobotAction::Recorded` as `system_ui` recorded:* and REST
/// `robot_recorded`, enabled per the handler's check. Speed is the header's
/// −/×/+ (the same scale); the seek slider under the transport is the same
/// `Recorded {Seek {t}}` (`panel_ui::recorded_seek` writes it while held).
pub(super) fn recorded_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    root: Single<Entity, With<RecordedRoot>>,
    // The root the block was built in: re-entering robot mode spawns a new, empty one.
    mut shown: Local<Option<Entity>>,
    mut text: Query<&mut Text, With<RecordedText>>,
    mut buttons: Query<(&RobotAction, &mut Enabled), With<RecordedButton>>,
    seek: Query<(Entity, &bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction), With<RecordedSeek>>,
    mut fill: Query<&mut Node, With<RecordedSeekFill>>,
) {
    let Some(p) = view.run.as_ref().and_then(|r| r.playback()) else {
        // Switched to a view without a recorded timeline (an embedded preset):
        // the old block would otherwise stay, frozen at the last recorded frame.
        if shown.take().is_some() {
            commands.entity(*root).despawn_children();
        }
        return;
    };
    if *shown != Some(*root) {
        let k = Kit { f: &fonts };
        let header = commands.spawn(k.section("Recorded")).id();
        let label = commands.spawn(k.text(format!("{} · speed: header −/×/+ · seek: press or drag the timeline", crate::robot::preset::RECORDED_LABEL), size::CAPTION, SUBTLE, 0)).id();
        let row = commands.spawn(wrap()).id();
        for (_, name, action) in RECORDED_TRANSPORT {
            let action = RobotAction::Recorded { action };
            let enabled = check(&view, &action).is_ok();
            let b = commands.spawn((k.button(name, action, Look::Secondary, enabled), RecordedButton)).id();
            commands.entity(row).add_child(b);
        }
        // The timeline: a kit slider over the capture's frame times (fill = the shown time).
        let at = seek_fraction(&p.timeline().times, p.timeline().t);
        let bar = commands
            .spawn(Node { align_items: AlignItems::Center, padding: UiRect::vertical(Val::Px(4.0)), flex_shrink: 0.0, ..default() })
            .with_children(|r| {
                // The Timebar look grows along this row (the kit sets its node).
                r.spawn(k.slider(crate::ui_kit::SliderLook::Timebar, at, RecordedSeek, "Recorded timeline")).with_children(|t| {
                    t.spawn((Node { border_radius: BorderRadius::all(Val::Px(5.0)), width: Val::Percent(at * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(ACCENT), RecordedSeekFill, Pickable::IGNORE));
                });
            })
            .id();
        let line = commands.spawn((k.text("", size::CAPTION, TEXT, 0), RecordedText)).id();
        commands.entity(*root).add_children(&[header, label, row, bar, line]);
        *shown = Some(*root);
    }
    // The slider follows the shown time unless it is held (then it is the pointer's).
    // `SliderValue` is an immutable component: replace it, don't mutate it.
    let at = seek_fraction(&p.timeline().times, p.timeline().t);
    for (entity, value, pressed, interaction) in &seek {
        if !crate::ui_kit::slider_held(pressed, interaction) && (value.0 - at).abs() > 1e-4 {
            commands.entity(entity).insert(bevy::ui_widgets::SliderValue(at));
        }
    }
    for mut node in &mut fill {
        let width = Val::Percent(at * 100.0);
        if node.width != width {
            node.width = width;
        }
    }
    let want = recorded_line(p);
    for mut t in &mut text {
        if t.0 != want {
            t.0 = want.clone();
        }
    }
    for (action, enabled) in &mut buttons {
        enable(enabled, check(&view, action).is_ok());
    }
}

/// The inspector's Gait preview block for a preset: the tracked reports (name,
/// report speed, status verbatim), the transport, and the truthful labels. Every
/// button is the same `RobotAction::Gait` as `system_ui` gait:* and REST
/// `robot_gait`, enabled per the handler's check. `--robot FILE` has no scene, so
/// nothing is shown there. Under the reports, the kit path field opens an
/// explicit compiled.json (`panel_ui`: the same `Gait {Open {source: Path}}`
/// as `robot_gait {path}`).
pub(super) fn gait_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    root: Single<Entity, With<GaitRoot>>,
    // The root and list the block was built in: re-entering robot mode spawns new, empty ones.
    mut shown: Local<Option<Entity>>,
    mut listed: Local<Option<(Entity, Vec<String>, Vec<String>, Option<String>)>>,
    list: Query<Entity, With<GaitList>>,
    mut text: Query<(&mut Text, Has<GaitError>), Or<(With<GaitText>, With<GaitError>)>>,
    mut buttons: Query<(&mut RobotAction, &mut Enabled, Option<&GaitSeekButton>), With<GaitButton>>,
) {
    let Some(g) = view.run.as_ref().and_then(|r| r.gait_preview()) else { return };
    let k = Kit { f: &fonts };
    let button = |commands: &mut Commands, action: GaitAction, text: &str| {
        let action = RobotAction::Gait { action };
        let enabled = check(&view, &action).is_ok();
        commands.spawn((k.button(text, action, Look::Secondary, enabled), GaitButton)).id()
    };
    if *shown != Some(*root) {
        let header = commands.spawn(k.section("Gait preview")).id();
        let label = commands.spawn(k.text(gait::LABEL, size::CAPTION, SUBTLE, 0)).id();
        let row = |commands: &mut Commands| commands.spawn(wrap()).id();
        let transport = row(&mut commands);
        let play = button(&mut commands, GaitAction::Play, "Play");
        let pause = button(&mut commands, GaitAction::Pause, "Pause");
        let stop = button(&mut commands, GaitAction::Stop, "Stop");
        let mut seek = Vec::new();
        for (step, _, _) in GAIT_SEEK {
            let b = button(&mut commands, GaitAction::Seek { t: 0.0 }, match step { 0 => "t = 0", -1 => "−P/12", _ => "+P/12" });
            commands.entity(b).insert(GaitSeekButton(step));
            seek.push(b);
        }
        commands.entity(transport).add_children(&[play, pause, stop]).add_children(&seek);
        let speed = row(&mut commands);
        let speed_label = commands.spawn(k.text("speed ×", size::CAPTION, SUBTLE, 0)).id();
        commands.entity(speed).add_child(speed_label);
        for scale in GAIT_SCALES {
            let b = button(&mut commands, GaitAction::Speed { scale }, &format!("{scale}"));
            commands.entity(speed).add_child(b);
        }
        let status = commands.spawn((k.text("", size::CAPTION, TEXT, 0), GaitText)).id();
        let error = commands.spawn((k.text("", size::CAPTION, DANGER, 0), GaitError)).id();
        let list_header = commands.spawn(k.text("Tracked gait reports (report speed · status, verbatim): click one to open it, or open any compiled.json with the path field below.", size::DETAIL, SUBTLE, 0)).id();
        let reports = commands.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() }, GaitList)).id();
        // Filled by `panel_ui::gait_path_draw`.
        let path = commands.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), margin: UiRect::top(Val::Px(6.0)), ..default() }, GaitPathRoot)).id();
        commands.entity(*root).add_children(&[header, label, status, error, transport, speed, list_header, reports, path]);
        *shown = Some(*root);
    }
    if let Ok(list) = list.single() {
        // Rebuilt only when the offered reports (or, with none offered, the
        // reasons) or the listing error change; compared without allocating.
        let offered = g.reports();
        let skipped: &[String] = if offered.is_empty() { g.skipped() } else { &[] };
        let same = listed.as_ref().is_some_and(|(l, n, s, e)| *l == list && n.len() == offered.len() && n.iter().zip(offered).all(|(n, r)| *n == r.name) && s.as_slice() == skipped && e.as_deref() == g.list_error());
        if !same {
            commands.entity(list).despawn_related::<Children>();
            let mut rows = Vec::new();
            for r in g.reports() {
                let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).id();
                let b = button(&mut commands, GaitAction::Open { source: GaitSource::Report(r.name.clone()) }, &r.name);
                let speed = r.speed_m_s.map_or("speed —".to_string(), |v| format!("{v:.3} m/s"));
                let t = commands.spawn(k.text(format!("{speed} · {}", r.status), size::DETAIL, TEXT, 0)).id();
                commands.entity(row).add_children(&[b, t]);
                rows.push(row);
            }
            let note = match g.list_error() {
                Some(e) => format!("listing failed: {e}"),
                None if offered.is_empty() && skipped.is_empty() => "no tracked gait report with an existing compiled gait".into(),
                // The reasons each report was skipped (the listing's own words).
                None if offered.is_empty() => {
                    let reasons: Vec<String> = skipped.iter().take(SKIPPED_SHOWN).map(|s| format!("• {}", clip(s, 120))).collect();
                    let more = skipped.len().saturating_sub(SKIPPED_SHOWN);
                    let more = if more > 0 { format!("\n… and {more} more") } else { String::new() };
                    format!("no tracked gait report with an existing compiled gait; skipped:\n{}{more}", reasons.join("\n"))
                }
                None => String::new(),
            };
            if !note.is_empty() {
                rows.push(commands.spawn(k.text(note, size::DETAIL, SUBTLE, 0)).id());
            }
            commands.entity(list).add_children(&rows);
            *listed = Some((list, offered.iter().map(|r| r.name.clone()).collect(), skipped.to_vec(), g.list_error().map(str::to_string)));
        }
    }
    let r = view.run.as_ref().expect("gait preview implies a run");
    for (mut t, error) in &mut text {
        let line = if error { g.error().map_or(String::new(), |e| format!("error: {e}")) } else { gait_line(r, g) };
        if t.0 != line {
            t.0 = line;
        }
    }
    for (mut action, enabled, step) in &mut buttons {
        if let Some(step) = step {
            let next = RobotAction::Gait { action: gait_seek(&view, step.0) };
            if *action != next {
                *action = next;
            }
        }
        enable(enabled, check(&view, &action).is_ok());
    }
}

/// Skipped reports shown under an empty report list.
const SKIPPED_SHOWN: usize = 6;

/// The Gait preview status lines (rounded, so they only change with the pose).
fn gait_line(r: &RunController, g: &gait::GaitPreview) -> String {
    let phase = json!(g.phase());
    let phase = phase.as_str().unwrap_or("");
    let blocked = r.check_gait(&GaitAction::Play).err().filter(|e| e.starts_with("a physics") || e.starts_with("a replay")).map_or(String::new(), |e| format!("\nunavailable: {e}"));
    let Some(l) = g.loaded() else {
        return format!("phase {phase} · no gait open · lift {} m (browser calibration-mirror){blocked}", gait::LIFT_M);
    };
    let source = json!(l.governor_source);
    let (status, fidelity) = l.report.as_ref().map_or(("(opened by path: no report)", "(opened by path: no report)"), |x| (x.status.as_str(), x.fidelity.as_str()));
    let mut t = format!(
        "{} · governor {} · period {:.3} s\nphase {phase} · gait time {} · scale ×{}\nstatus (verbatim): {status}\nfidelity (verbatim): {fidelity}\nlift {} m (browser calibration-mirror)",
        l.report.as_ref().map_or_else(|| l.compiled.display().to_string(), |x| x.name.clone()),
        source.as_str().unwrap_or(""),
        l.period_s,
        g.sample().map_or("—".into(), |x| format!("{:.2} s", x.gait_time_s)),
        g.speed_scale(),
        gait::LIFT_M,
    );
    match g.sample() {
        Some(x) => {
            t += &format!("\nauthored-limit violations: {}", if x.authored_limit_violations.is_empty() { "none".to_string() } else { x.authored_limit_violations.join(", ") });
            let q = if x.drives == "commanded" { &x.commanded } else { &x.desired };
            let joints: Vec<String> = l.joints.iter().zip(q).map(|(j, v)| format!("{j} {v:+.3}")).collect();
            t += &format!("\ndrives {} (rad): {}", x.drives, joints.join(" · "));
        }
        None => t += "\nauthored-limit violations: — (no pose yet)",
    }
    t + blocked.as_str()
}

/// One line under the motion buttons: the requested values, or why motion is unavailable.
fn motion_line(r: &RunController) -> String {
    let m = r.motion_json();
    if m["available"] != true {
        return format!("unavailable: {}", clip(m["unavailable_reason"].as_str().unwrap_or(""), 120));
    }
    let values: Vec<String> = m["channels"].as_array().into_iter().flatten().map(|c| {
        let short = c["name"].as_str().unwrap_or("").trim_start_matches("command.");
        format!("{short} {}", c["requested"].as_f64().or(c["held"].as_f64()).map_or("—".into(), |x| format!("{x}")))
    }).collect();
    let keys = m["active_keys"].as_array().filter(|k| !k.is_empty()).map_or("none".into(), |k| k.iter().filter_map(|k| k.as_str()).collect::<Vec<_>>().join("+"));
    let refused = m["last_refusal"].as_str().map_or(String::new(), |e| format!("\nrefused: {}", clip(e, 110)));
    format!("requested {} · keys {keys}{refused}", values.join(" · "))
}

/// The line beside Save recording: pending, the last pair written (path, steps, kind, replayable) or the last error.
fn recording_line(r: &RunController) -> String {
    // Shown relative to the workspace root the save resolved against (the preset's, or the controlled run's).
    let root = r.preset().map(|p| p.root.as_path()).or_else(|| r.controlled().and_then(|c| c.root.as_deref().ok()));
    let path = |p: &std::path::Path| p.strip_prefix(root.unwrap_or(std::path::Path::new(""))).unwrap_or(p).display().to_string();
    if let Some(p) = r.save_pending() {
        return format!("saving {}…", path(p));
    }
    let mut t = match r.saved() {
        Some(s) => format!("saved {} · {} steps · {}{}", path(&s.path), s.completed_steps, s.kind, if s.replayable { String::new() } else { " · diagnostic (not replayable)".into() }),
        None => "no recording saved yet".into(),
    };
    if let Some(e) = r.save_error() {
        t += &format!("\nsave refused/failed: {}", clip(e, 140));
    }
    t
}

/// The line under the replay controls: phase, progress, verdict and error, and the measured difference labelled as such.
pub(super) fn replay_line(r: &RunController) -> String {
    let s = r.replay_state();
    let mut t = match s.phase {
        ReplayPhase::Idle => "no replay".to_string(),
        phase => format!("{} {} · {}", format!("{phase:?}").to_lowercase(), s.path.as_ref().and_then(|p| p.file_name()).map_or(String::new(), |n| n.to_string_lossy().into_owned()), format_args!("{}/{} {}", s.completed, s.total.map_or("?".into(), |n| n.to_string()), s.unit.unwrap_or(""))),
    };
    if let Some(v) = &s.verdict {
        t += &format!("\nverdict: {}", clip(v, 150));
    }
    if let Some(e) = &s.error {
        t += &format!("\nerror: {}", clip(e, 150));
    }
    if let Some(m) = &s.measured {
        t += &format!("\nmeasured difference, not a pass criterion: max |Δp| {:.3e} m ({}); {} {:.3e} m", m["max_position_diff_m"].as_f64().unwrap_or(f64::NAN), m["max_link"].as_str().unwrap_or(""), m["first_link"].as_str().unwrap_or(""), m["first_link_position_diff_m"].as_f64().unwrap_or(f64::NAN));
    }
    t
}

/// The motion block of the inspector: source, channels with bounds, requested
/// and held values, keys, heartbeat and the last refusal (robot_state.motion).
pub(super) fn motion_text(r: &RunController) -> String {
    let m = r.motion_json();
    let mut t = format!("MOTION — {}\n", motion::LABEL);
    if m["available"] != true {
        t += &format!("unavailable: {}\n", m["unavailable_reason"].as_str().unwrap_or(""));
    }
    if let Some(source) = m["source"].as_str() {
        t += &format!("channels from: {source}\n");
    }
    for c in m["channels"].as_array().into_iter().flatten() {
        let v = |k: &str| c[k].as_f64().map_or("—".into(), |x| format!("{x}"));
        t += &format!("• {} [{}, {}] {} — requested {} · held {}\n", c["name"].as_str().unwrap_or(""), v("lower"), v("upper"), c["unit"].as_str().unwrap_or(""), v("requested"), v("held"));
    }
    if let Some(rule) = m["config"]["key_rule"].as_str() {
        let keys = m["active_keys"].as_array().filter(|k| !k.is_empty()).map_or("none".into(), |k| k.iter().filter_map(|k| k.as_str()).collect::<Vec<_>>().join("+"));
        t += &format!("keys: {keys}{} · {rule}\n", if m["keys_physical"] == true { " (held)" } else if keys != "none" { " (latched)" } else { "" });
    }
    if let Some(h) = m["heartbeat"].as_object() {
        t += &format!("heartbeat {}: {} of {} (one per action packet)\n", h["channel"].as_str().unwrap_or(""), h["value"].as_f64().map_or("—".into(), |x| format!("{x}")), h["upper"]);
    }
    for (k, name) in [("last_refusal", "last refusal"), ("last_apply_error", "not applied")] {
        if let Some(e) = m[k].as_str() {
            t += &format!("{name}: {e}\n");
        }
    }
    t.push_str(&format!("{}\n{}\n\n", motion::KEY_SEMANTICS, motion::CLAMP_RULE));
    t
}
