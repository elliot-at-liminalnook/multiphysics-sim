//! The Drive block: a controlled run's teleoperation (the drive target, its
//! buttons and keys, the twist and device lines, the controller's detail).
use super::*;

/// The controller a controlled `--robot FILE` run is driven by (header, Drive block, robot_state).
pub(in crate::robot) const DRIVE_CONTROLLER: &str = crate::robot::run::CONTROLLER_LABEL;
/// What a controlled run simulates, in short beside its results (robot_state's
/// `drive.fidelity` carries the full `run::DRIVE_FIDELITY`).
pub(in crate::robot) const DRIVE_FIDELITY: &str = "the same PhysicalRobot physics as a hold run, with the external controller in place of the hold coupler: it writes the wheel servo targets the model's motor firmware tracks. The twist requested here is limited by the drive profile (acceleration, deadman) before the controller mixes it.";

/// Decimals of a twist value in the Drive block: fixed, so a line changes only with its value.
const TWIST_DECIMALS: usize = 3;
/// A normalized device axis (−1…1).
const AXIS_DECIMALS: usize = 2;

/// `x` at `decimals` places with a sign, never "−0.000" (a value that
/// rounds to zero shows as +0.000, so a resting twist does not flicker).
pub(in crate::robot) fn fixed(x: f64, decimals: usize) -> String {
    let scale = 10f64.powi(decimals as i32);
    // −0.0 + 0.0 is +0.0.
    let r = (x * scale).round() / scale + 0.0;
    format!("{r:+.decimals$}")
}

/// A drive geometry value (`Valued`, serialized as the resolved JSON has it)
/// with its unit and provenance kind and basis (`from` or `source`).
pub(in crate::robot) fn valued_text(v: &impl Serialize, unit: &str) -> String {
    let v = serde_json::to_value(v).unwrap_or(Value::Null);
    if v.is_null() {
        return "none".into();
    }
    let p = &v["provenance"];
    let basis = p["from"].as_str().or_else(|| p["source"].as_str()).map_or(String::new(), |b| format!(" ({})", clip(b, 90)));
    format!("{} {unit} — {}{basis}", v["value"].as_f64().map_or("—".into(), |x| format!("{x:?}")), p["kind"].as_str().unwrap_or("no provenance"))
}

/// The first 12 hex digits of a sha256 (the full hash is in robot_state.drive).
pub(in crate::robot) fn short_sha(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

/// A twist's supported axes with units at fixed decimals.
pub(in crate::robot) fn twist_text(t: kinematics::BodyTwist, supported: [bool; 3]) -> String {
    let values = t.to_array();
    let parts: Vec<String> = (0..3).filter(|i| supported[*i]).map(|i| format!("{} {} {}", kinematics::AXIS_NAMES[i], fixed(values[i], TWIST_DECIMALS), kinematics::SPEED_UNITS[i])).collect();
    if parts.is_empty() { "no supported axis".into() } else { parts.join(" · ") }
}

/// Keys the Leg calibration panel owns while it is shown (Q/A hold-to-move,
/// Z STOP; `hardware::actions::input::keys`): never read for driving then.
pub(in crate::robot) const PANEL_KEYS: [KeyCode; 3] = [KeyCode::KeyQ, KeyCode::KeyA, KeyCode::KeyZ];

/// What robot mode offers the device poller: a controlled run
/// (`RunController::controlled`, a binding with a `sim.drive/1` profile)
/// with its supported axes, or nothing. Robot mode's one target writer.
/// `LiveTarget::run` is [`run_identity`]: a reload, Reset, replay start and
/// replay end each change it, so the poller disarms inputs held across them.
pub(in crate::robot) fn robot_target(view: Option<&RobotView>, panel_open: bool) -> DriveTarget {
    let live = view.and_then(|v| {
        let run = v.run.as_ref()?;
        let c = run.controlled()?;
        Some(LiveTarget { mode: ViewerMode::Robot, supported: c.controlled.resolved.limits.supported, run: run_identity(&v.path, run) })
    });
    let owned_keys = if live.is_some() && panel_open { PANEL_KEYS.to_vec() } else { Vec::new() };
    DriveTarget { live, owned_keys }
}

/// The identity of `path`'s run for the drive poller and the refusal it
/// shows: the file, the run controller's generation (bumped by Reset, a
/// reload's `RunController::replace_file` and a replay start) and whether a
/// replay is in progress (a replay's end, to done, failed or cancelled, bumps
/// no generation; the flag makes it a change).
pub(in crate::robot) fn run_identity(path: &std::path::Path, run: &RunController) -> String {
    let replaying = run.replay_state().phase == crate::robot::run::ReplayPhase::Replaying;
    format!("{} (run generation {}{})", path.display(), run.generation(), if replaying { ", replaying" } else { "" })
}

/// Input, before `InputSet::Window` (Robot mode): writes [`DriveTarget`]
/// for the one device poller ([`robot_target`], `set_if_neq`), and clears
/// `DriveInput::last_error` when the run it belongs to changes
/// ([`run_identity`]: the file, its generation (Reset, a replay start, a
/// reload) or a replay's end), as the refusal was that run's.
/// Robot mode's apply records the refusals (`actions::apply`).
pub(in crate::robot) fn drive_target(
    view: Option<Res<RobotView>>,
    hardware: Option<Res<crate::robot::hardware::Hardware>>,
    target: Option<ResMut<DriveTarget>>,
    input: Option<ResMut<DriveInput>>,
    mut run_key: Local<Option<(PathBuf, Option<String>)>>,
) {
    let view = view.as_deref();
    if let Some(mut target) = target {
        target.set_if_neq(robot_target(view, hardware.is_some_and(|h| h.open)));
    }
    let key = view.map(|v| (v.path.clone(), v.run.as_ref().map(|r| run_identity(&v.path, r))));
    if *run_key != key {
        *run_key = key;
        if let Some(mut input) = input
            && input.last_error.is_some()
        {
            input.last_error = None;
        }
    }
}

/// The inspector's Drive block for a controlled `--robot FILE` run
/// (`RunController::controlled`): Stop and the profile's named actions (the
/// same `RobotAction::Drive` as the device bindings, `system_ui` and REST,
/// enabled per the handler's check), the requested and commanded twist, the
/// deadman, the heartbeat and the device input above the inspector scroll;
/// the controller, profile, kinematics, geometry with provenance, limits and
/// bindings inside it. Rebuilt when the controlled run (a reload builds a
/// new one: script, profile, binding path and model-derived geometry may all
/// change) or a root changes; its static detail is written then, and its live lines
/// are rewritten only when their rounded text changes. Nothing is shown for
/// any other run.
#[allow(clippy::too_many_arguments)]
pub(in crate::robot) fn drive_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    roots: (Single<Entity, With<DriveRoot>>, Single<Entity, With<DriveDetailRoot>>),
    (bindings, input): (Option<Res<DriveBindings>>, Option<Res<DriveInput>>),
    mut shown: Local<Option<(Entity, Entity, usize, Vec<String>)>>,
    mut texts: Query<(&DriveText, &mut Text)>,
    mut buttons: Query<(&RobotAction, &mut Enabled), With<DriveButton>>,
) {
    let (root, detail) = (*roots.0, *roots.1);
    let run = view.run.as_ref();
    let Some((r, c)) = run.and_then(|r| Some((r, r.controlled()?))) else {
        // Another run (or none): the block of a previous controlled run goes.
        if shown.take().is_some() {
            commands.entity(root).despawn_related::<Children>();
            commands.entity(detail).despawn_related::<Children>();
        }
        return;
    };
    let robot = &c.controlled;
    let resolved = &robot.resolved;
    let actions: Vec<String> = robot.profile.actions.iter().map(|a| a.name.clone()).collect();
    // Keyed by the roots too: re-entering robot mode spawns new, empty roots.
    // The run's address stands for the static detail: every reload loads a new
    // `ControlledRun` (its geometry derived from the reloaded model, its
    // binding path and hashes), so moved wheel joints with the same profile
    // and script still rebuild the block.
    let key = (root, detail, std::sync::Arc::as_ptr(c) as usize, actions.clone());
    if shown.as_ref() != Some(&key) {
        let k = Kit { f: &fonts };
        commands.entity(root).despawn_related::<Children>();
        commands.entity(detail).despawn_related::<Children>();
        let header = commands.spawn(k.section("Drive")).id();
        let label = commands.spawn(k.text(format!("{DRIVE_CONTROLLER} · {DRIVE_FIDELITY}"), size::CAPTION, SUBTLE, 0)).id();
        let row = commands.spawn(wrap()).id();
        let mut spawn_button = |request: DriveRequest, text: &str, look: Look| {
            let action = RobotAction::Drive { request };
            let enabled = check(&view, &action).is_ok();
            let b = commands.spawn((k.button(text, action, look, enabled), DriveButton)).id();
            commands.entity(row).add_child(b);
        };
        spawn_button(DriveRequest::Stop, "Stop", Look::Danger);
        for name in &actions {
            spawn_button(DriveRequest::Action { name: name.clone() }, name.as_str(), Look::Secondary);
        }
        let live = commands.spawn((k.text("", size::CAPTION, TEXT, 0), DriveText::Live)).id();
        let device = commands.spawn((k.text("", size::CAPTION, TEXT, 0), DriveText::Input)).id();
        let error = commands.spawn((k.text("", size::CAPTION, DANGER, 0), DriveText::Error)).id();
        commands.entity(root).add_children(&[header, label, row, live, device, error]);
        let detail_header = commands.spawn(k.section("Drive profile")).id();
        let detail_text = commands.spawn(k.text(drive_detail(robot), size::CAPTION, TEXT, 0)).id();
        let bindings_text = commands.spawn((k.text("", size::CAPTION, TEXT, 0), DriveText::Bindings)).id();
        commands.entity(detail).add_children(&[detail_header, detail_text, bindings_text]);
        *shown = Some(key);
    }
    let supported = resolved.limits.supported;
    for (which, mut text) in &mut texts {
        let line = match which {
            DriveText::Live => match r.drive_state() {
                None => "no drive status yet: Run or Step starts the controller".to_string(),
                Some(s) => {
                    let age = if s.age_s.is_finite() { format!("{:.1} s", s.age_s) } else { "— (no request yet)".into() };
                    let deadman = if s.expired { format!("EXPIRED (on loss: {})", resolved.deadman.on_loss) } else { "live".into() };
                    format!(
                        "requested  {}\ncommanded  {} (limited; what the controller receives)\ndeadman {deadman} · request age {age} of {:?} s · heartbeat {} · sim t {:.1} s",
                        twist_text(s.request, supported),
                        twist_text(s.commanded, supported),
                        resolved.deadman.timeout_s,
                        s.heartbeat,
                        s.time_s
                    )
                }
            },
            DriveText::Input => match input.as_deref() {
                None => "device input: not available".to_string(),
                Some(i) => {
                    let axes = i.axes.to_array();
                    let values: Vec<String> = (0..3).filter(|a| supported[*a]).map(|a| format!("{} {}", kinematics::AXIS_NAMES[a], fixed(axes[a], AXIS_DECIMALS))).collect();
                    let mut t = format!("input ({}): {} (normalized −1…1) · last action {}", i.source.unwrap_or("idle"), values.join(" · "), i.last_action.as_deref().unwrap_or("none"));
                    if !i.ignored.is_empty() {
                        t += &format!("\nignored: {} (not in this robot's drive profile)", i.ignored.join(", "));
                    }
                    if let Some(e) = &i.last_error {
                        t += &format!("\nlast input refused: {}", clip(e, 160));
                    }
                    t
                }
            },
            DriveText::Error => r.error().map_or(String::new(), |e| format!("RUN FAILED: {e}")),
            DriveText::Bindings => match bindings.as_deref() {
                None => "DEVICE BINDINGS: not available".to_string(),
                Some(b) => {
                    let rows = b.describe();
                    let mut t = format!("DEVICE BINDINGS ({}; viewer settings, shared across robots)\n", rows.len());
                    for (input, does) in rows {
                        t += &format!("• {input}: {does}\n");
                    }
                    t
                }
            },
        };
        if text.0 != line {
            text.0 = line;
        }
    }
    for (action, enabled) in &mut buttons {
        enable(enabled, check(&view, action).is_ok());
    }
}

/// The Drive block's static lines: controller identity, profile, kinematics,
/// geometry with provenance, limits per supported axis and the deadman rule.
pub(in crate::robot) fn drive_detail(robot: &sim_runtime::controller_binding::ControlledRobot) -> String {
    let (resolved, identity) = (&robot.resolved, &robot.identity);
    let args = if identity.args.is_empty() { "none".to_string() } else { identity.args.join(" ") };
    let mut t = format!("controller: {DRIVE_CONTROLLER}\nscript: {}\n  sha256 {} · args {args} (plus --drive-json, the resolved profile)\n", identity.script.display(), short_sha(&identity.script_sha256));
    t += &format!("binding: {}\n", robot.binding_path.display());
    t += &format!("profile: {}\n  sha256 {}\n", identity.profile.display(), short_sha(&identity.profile_sha256));
    if let Some(d) = &robot.profile.description {
        t += &format!("  description (file's text): \"{d}\"\n");
    }
    let geometry = serde_json::to_value(&resolved.geometry).unwrap_or(Value::Null);
    t += &format!("\nkinematics: {}\n", resolved.kinematics);
    t += &format!("track width: {}\nwheel radius: {}\n", valued_text(&resolved.geometry.track_width_m, "m"), valued_text(&resolved.geometry.wheel_radius_m, "m"));
    if !geometry["wheelbase_m"].is_null() {
        t += &format!("wheelbase: {}\n", valued_text(&geometry["wheelbase_m"], "m"));
    }
    for w in geometry["wheels"].as_array().into_iter().flatten() {
        t += &format!("wheel {} · joint sign {} — {}\n", w["joint"].as_str().unwrap_or(""), w["sign"].as_f64().map_or("—".into(), |s| format!("{s:+}")), w["provenance"]["kind"].as_str().unwrap_or("no provenance"));
    }
    let l = &resolved.limits;
    t += "\nLIMITS (resolved profile)\n";
    for i in 0..3 {
        let axis = kinematics::AXIS_NAMES[i];
        if l.supported[i] {
            t += &format!("{axis}: max speed {:?} {} · max accel {:?} {} · stop decel {:?} {}\n", l.max_speed[i], kinematics::SPEED_UNITS[i], l.max_accel[i], kinematics::ACCEL_UNITS[i], l.stop_decel[i], kinematics::ACCEL_UNITS[i]);
        } else {
            t += &format!("{axis}: not supported (the profile declares no {axis} axis)\n");
        }
    }
    t += &format!("deadman: timeout {:?} s · on loss {}\n", resolved.deadman.timeout_s, resolved.deadman.on_loss);
    t
}
