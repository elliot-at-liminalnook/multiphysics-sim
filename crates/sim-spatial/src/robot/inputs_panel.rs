//! The inspector's Inputs block for a preset (the browser's input sliders,
//! `viewer.js` makeInputs): one kit slider per typed session input over its
//! bounds, labelled with the browser's units (mm/s, °/s, rad), the motion
//! heartbeat hidden, the `residual.*` motor corrections in a collapsible
//! group with "Clear motor corrections", and "Reset inputs". A press or drag
//! writes `RobotAction::Inputs` (REST `robot_inputs`), only while held and
//! only when the value changes; otherwise each slider follows the held value
//! in the latest frame. The help line is the browser's input-help text.
use super::*;
use crate::app::actions::Act;
use crate::ui_kit::SliderLook;

#[derive(Component)]
pub(super) struct InputsRoot;
/// One input's slider: the channel's name and bounds.
#[derive(Component, Clone)]
pub(super) struct InputSlider {
    name: String,
    lower: f64,
    upper: f64,
}
#[derive(Component)]
pub(super) struct InputFill(String);
#[derive(Component)]
pub(super) struct InputLabel(String);
#[derive(Component)]
pub(super) struct InputsButton;
#[derive(Component)]
pub(super) struct InputsHelp;

/// The value as the browser labels it, by quantity kind.
fn display(kind: &str, x: f64, lower: f64, upper: f64) -> String {
    match kind {
        "LinearVelocity" => format!("{:.2} mm/s", x * 1000.0),
        "AngularVelocity" => format!("{:.3}°/s", x.to_degrees()),
        "Angle" => format!("{:.*} rad", if upper - lower <= 0.1 { 4 } else { 2 }, x),
        _ => format!("{x:.2}"),
    }
}
fn kind_name(c: &sim_runtime::session::InputChannel) -> String {
    serde_json::to_value(&c.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}
fn fraction(x: f64, lower: f64, upper: f64) -> f32 {
    if upper > lower { ((x - lower) / (upper - lower)).clamp(0.0, 1.0) as f32 } else { 0.0 }
}

/// The browser's input-help line for the loaded preset.
fn help(run: &RunController) -> String {
    let Some(p) = run.preset() else { return String::new() };
    let Some(d) = run.preset_drive() else { return String::new() };
    let mut t = if let Some(c) = d.metadata.get("environment_contract").filter(|c| c.is_object()) {
        let ms = c["period_s"].as_f64().map_or("—".into(), |s| format!("{}", s * 1000.0));
        let walking = p.task.as_ref().map(|t| serde_json::to_value(t).unwrap_or(Value::Null)).filter(|t| t["walking"].is_object());
        let scores = match walking {
            Some(w) => format!("The task scores joint tracking, body position and supported steps{}.", if w["walking"]["heading"].is_null() { "" } else { ", plus heading" }),
            None => "Scores follow the selected task; this preset has no supported-step walking objective.".into(),
        };
        format!("Each command is held for {ms} ms of simulation time. The selected controller and actuator profile determine the motor response. {scores} Saving and replay preserve the task and command sequence.")
    } else if d.metadata["policy_contract"].is_object() && !d.metadata["policy_contract"].as_object().is_some_and(|m| m.is_empty()) {
        "Rhai reads ideal simulated joint state and sends motor targets at its declared sampling rate. Adjust the commands above; save and replay preserve when they changed. Hardware sensor bindings and walking commands are not yet available.".into()
    } else if p.config.is_some() {
        "The Rust servo controller executes this experiment live. Pause and reset are available; this preset does not yet declare WASD walking commands.".into()
    } else {
        "Use the position slider while running. This fixture has no walking command; WASD locomotion is unavailable.".into()
    };
    let step = &d.metadata["policy_contract"]["step_reference"];
    if step.is_object() {
        t += if step["config"]["sequence"]["update_command_before_lift"] == true {
            " New motion requests are checked before lift-off. A stop at that point keeps the feet planted and returns the body to standing. Airborne feet complete their landing, and reversals wait for the current transfer."
        } else {
            " Motion requests are latched at foot-transfer boundaries. Releasing a key finishes the current transfer before standing; this provisional crawl is deliberately slow."
        };
    }
    t
}

/// Present: the Inputs block, rebuilt when the built session's channels (or
/// the residual group's open state) change; values and fills every frame.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn inputs_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    ui: Res<panel_ui::RobotPanelUi>,
    fonts: Res<UiFonts>,
    root: Single<Entity, With<InputsRoot>>,
    mut shown: Local<Option<(Entity, Vec<String>, bool)>>,
    sliders: Query<(Entity, &InputSlider, &bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction)>,
    mut fills: Query<(&InputFill, &mut Node)>,
    mut texts: Query<(Option<&InputLabel>, &mut Text), Or<(With<InputLabel>, With<InputsHelp>)>>,
    mut buttons: Query<(&RobotAction, &mut Enabled), With<InputsButton>>,
) {
    let run = view.run.as_ref().filter(|r| r.preset().is_some());
    let drive = run.and_then(|r| r.preset_drive());
    let Some((run, drive)) = run.zip(drive) else {
        if shown.take().is_some() {
            commands.entity(*root).despawn_related::<Children>();
        }
        return;
    };
    let heartbeat = drive.heartbeat.as_ref().map(|h| h.index);
    let channels: Vec<(usize, &sim_runtime::session::InputChannel)> = drive.inputs.iter().enumerate().filter(|(i, _)| Some(*i) != heartbeat).collect();
    let names: Vec<String> = channels.iter().map(|(_, c)| c.name.clone()).collect();
    let key = (*root, names, ui.residuals_open);
    let held = |i: usize| run.frame().and_then(|f| f.inputs.get(i).copied());
    if shown.as_ref() != Some(&key) {
        let k = Kit { f: &fonts };
        commands.entity(*root).despawn_related::<Children>();
        let mut rows = Vec::new();
        if !channels.is_empty() {
            rows.push(commands.spawn(k.section("Inputs")).id());
            rows.push(commands.spawn((k.text(help(run), size::CAPTION, SUBTLE, 0), InputsHelp)).id());
        }
        let residual_count = channels.iter().filter(|(_, c)| c.name.starts_with(run::RESIDUAL_PREFIX)).count();
        let mut residual_header = false;
        for (i, c) in &channels {
            let residual = c.name.starts_with(run::RESIDUAL_PREFIX);
            if residual && !residual_header {
                residual_header = true;
                let label = format!("{} Motor corrections ({residual_count})", if ui.residuals_open { "▾" } else { "▸" });
                rows.push(commands.spawn(k.button(&label, panel_ui::PanelToggle::Residuals, Look::Ghost, true)).id());
                if ui.residuals_open {
                    rows.push(commands.spawn(k.text("Angle offsets added to the crawl controller. Zero uses the baseline. Command limits still apply.", size::CAPTION, SUBTLE, 0)).id());
                }
            }
            if residual && !ui.residuals_open {
                continue;
            }
            let x = held(*i).unwrap_or(c.initial);
            let at = fraction(x, c.lower, c.upper);
            let label = commands.spawn((k.text(format!("{}: {}", c.name, display(&kind_name(c), x, c.lower, c.upper)), size::CAPTION, TEXT, 0), InputLabel(c.name.clone()))).id();
            let marker = InputSlider { name: c.name.clone(), lower: c.lower, upper: c.upper };
            let bar = commands
                .spawn(Node { align_items: AlignItems::Center, padding: UiRect::vertical(Val::Px(3.0)), flex_shrink: 0.0, ..default() })
                .with_children(|r| {
                    r.spawn(k.slider(SliderLook::Track, at, marker, &c.name)).with_children(|t| {
                        t.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.0)), width: Val::Percent(at * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(ACCENT), InputFill(c.name.clone()), Pickable::IGNORE));
                    });
                })
                .id();
            rows.extend([label, bar]);
        }
        let controls = actions::input_controls(&view);
        if !controls.is_empty() {
            let row = commands.spawn(wrap()).id();
            for (_, label, action) in controls {
                let short = if label.starts_with("Clear") { "Clear motor corrections" } else { "Reset inputs" };
                let enabled = check(&view, &action).is_ok();
                let b = commands.spawn((k.button(short, action, Look::Secondary, enabled), InputsButton)).id();
                commands.entity(row).add_child(b);
            }
            rows.push(row);
        }
        commands.entity(*root).add_children(&rows);
        *shown = Some(key);
        return;
    }
    // Values: each slider follows the held value unless it is held (then it is the pointer's).
    for (entity, s, value, pressed, interaction) in &sliders {
        let Some((i, c)) = channels.iter().find(|(_, c)| c.name == s.name) else { continue };
        let x = held(*i).unwrap_or(c.initial);
        let at = fraction(x, s.lower, s.upper);
        let shown_at = if crate::ui_kit::slider_held(pressed, interaction) { value.0 } else { at };
        if !crate::ui_kit::slider_held(pressed, interaction) && (value.0 - at).abs() > 1e-4 {
            commands.entity(entity).insert(bevy::ui_widgets::SliderValue(at));
        }
        for (fill, mut node) in &mut fills {
            if fill.0 == s.name {
                let width = Val::Percent(shown_at * 100.0);
                if node.width != width {
                    node.width = width;
                }
            }
        }
    }
    for (label, mut text) in &mut texts {
        let Some(label) = label else { continue };
        let Some((i, c)) = channels.iter().find(|(_, c)| c.name == label.0) else { continue };
        let x = held(*i).unwrap_or(c.initial);
        let line = format!("{}: {}", c.name, display(&kind_name(c), x, c.lower, c.upper));
        if text.0 != line {
            text.0 = line;
        }
    }
    for (action, enabled) in &mut buttons {
        enable(enabled, check(&view, action).is_ok());
    }
}

/// Input: a press or drag on an input slider writes `RobotAction::Inputs`
/// for that input when its value changes (gated like a button: a refused
/// value is not resent every drag frame); nothing while no slider is held.
pub(super) fn input_sliders(bars: Query<(&InputSlider, &bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction)>, view: Res<RobotView>, mut sent: Local<Option<(String, f64)>>, mut out: MessageWriter<Act<RobotAction>>) {
    let mut held = false;
    for (s, value, pressed, interaction) in &bars {
        if !crate::ui_kit::slider_held(pressed, interaction) {
            continue;
        }
        held = true;
        let x = (s.lower + f64::from(value.0.clamp(0.0, 1.0)) * (s.upper - s.lower)).clamp(s.lower, s.upper);
        if sent.as_ref() != Some(&(s.name.clone(), x)) {
            *sent = Some((s.name.clone(), x));
            let action = RobotAction::Inputs { values: [(s.name.clone(), x)].into_iter().collect() };
            if check(&view, &action).is_ok() {
                out.write(Act::ui(action));
            }
        }
    }
    if !held {
        *sent = None;
    }
}
