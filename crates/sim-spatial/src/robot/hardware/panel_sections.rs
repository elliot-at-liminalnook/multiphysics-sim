//! The Leg calibration panel's widget tree, in the page's order and with
//! its labels (web/viewer/calibration-ui.mjs :11-62 and
//! actuator-motion-view.mjs :12), and the dynamic lists the refresh
//! rebuilds. Every button carries its `HardwareAction` and a
//! [`ControlId`]; states, labels and texts are written by `panel::refresh`.
use super::actions::{Boundary, Direction, DriveMode, GaitMode, HardwareAction};
use super::motion_view;
use super::panel::{CAMPAIGN_OK, ControlId, DialImage, FollowLabel, GAIT_OK, HOLD_OTHERS, JogButton, ListKind, MotionImage, PanelList, PanelSlider, PanelText, STEPS, Shown, SliderFill, TUNE_OK, gait_mode_label, section_id, section_label};
use super::view::{NO_RUNS, PanelView, STATS_HEADER};
use super::Section;
use crate::ui_kit::{ACCENT, BORDER, Corner, Kit, Look, SUBTLE, SliderLook, TEXT, WARN, size, wrap};
use bevy::prelude::*;

const HINT: &str = "Select a motor. Hold to move. Release to hold its pose.";
const TUNE_TEXT: &str = "Measures this motor's friction and response with short moves (±200 counts at most) and saves gains fitted to it. Takes about 20 seconds.";
const CAMPAIGN_TEXT: &str = "Staged tests on every enabled, tuned motor with both poses taught: slow sweeps, holds, braking, steps, servo steps, an effort ladder, all motors together, then repeats. Each test is rehearsed on the tuned model and stops on divergence, supply sag, heat, drift or travel. Results are fitted with uncertainty; nothing is promoted to CAD automatically.";
const GAIT_TEXT: &str = "Plays a gait found by the gait search. Sim only animates the simulated robot. Leg only drives the real leg's aligned motors through the same controller, taught poses and FPGA window. Both runs both on one clock (the real leg shown in blue). Every consumer samples the gait with the same Rust code. ★ = found with the measured motor profiles.";
const GOVERNOR_TEXT: &str = "Both run the gait through its own reference governor, the one the simulation used. Leg effort caps each real motor at that fraction of its measured speed and acceleration (accepted motor profiles). The belt/hip also stays at the campaign plan's belt limit. The PWM ceiling in Advanced still applies.";
const LEARN_TEXT: &str = "Teach both poses, then learn stopping response at the desired speed.";
const ANGLE_NOTE: &str = "Continuous motor angle · zero is not a travel stop";
pub(super) const TARGET_NOTE: &str = "Green = measured · blue = requested. Teach poses before contact. Z disables torque; releasing Q/A keeps active hold. Changing tabs stops drive.";
const SERVO_NOTE: &str = "Servo modes use the servo's own fast loop; the host streams goals from the same reference, within the same saved poses. Applies when a motion session starts. Tuning always uses PWM.";
const PWM_NOTE: &str = "The controller adjusts effort within this ceiling. Holding gains are provisional until tested on this loaded fixture.";

/// A row of controls, 8 px apart, the panel's width.
fn row() -> Node {
    Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, flex_shrink: 0.0, width: Val::Percent(100.0), ..default() }
}
/// A row with its two ends apart (a label and its value).
fn between() -> Node {
    Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(8.0), flex_shrink: 0.0, width: Val::Percent(100.0), ..default() }
}
fn column(gap: f32) -> Node {
    Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(gap), flex_shrink: 0.0, ..default() }
}

/// A kit button with its `system_ui` id; `follow`: its label follows the control's.
fn control<'a>(p: &'a mut ChildSpawnerCommands<'_>, k: &Kit, id: &str, label: &str, action: HardwareAction, look: Look, follow: bool) -> EntityCommands<'a> {
    let mut e = p.spawn((k.button(label, action, look, true), ControlId(id.to_string())));
    if follow {
        e.insert(FollowLabel);
    }
    e
}

/// Share a row equally with its siblings.
fn grow(mut e: EntityCommands) {
    e.entry::<Node>().and_modify(|mut n| {
        n.flex_grow = 1.0;
        n.flex_basis = Val::Px(0.0);
        n.flex_shrink = 1.0;
        n.min_width = Val::Px(0.0);
    });
}

/// A kit slider with its fill.
fn slider(p: &mut ChildSpawnerCommands, k: &Kit, which: PanelSlider, value: f32, label: &str) {
    p.spawn(k.slider(SliderLook::Track, value, which, label)).with_children(|t| {
        t.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.0)), width: Val::Percent(value * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(ACCENT), SliderFill(which), Pickable::IGNORE));
    });
}

/// A label and its value text above a control.
fn labelled(p: &mut ChildSpawnerCommands, k: &Kit, label: &str, value: PanelText) {
    p.spawn(between()).with_children(|r| {
        r.spawn(k.text(label, size::BODY, TEXT, 1));
        r.spawn((k.caption(""), value));
    });
}

/// A collapsible section (the page's `<details>`): a header row that
/// toggles it, with a compact Stop, and its body.
fn section(p: &mut ChildSpawnerCommands, k: &Kit, s: Section, content: impl FnOnce(&mut ChildSpawnerCommands)) {
    let open = s.open_initially();
    p.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), border: UiRect::top(Val::Px(1.0)), padding: UiRect::top(Val::Px(8.0)), margin: UiRect::top(Val::Px(6.0)), flex_shrink: 0.0, ..default() }, BorderColor::all(BORDER))).with_children(|c| {
        c.spawn(row()).with_children(|r| {
            let mut head = control(r, k, section_id(s), &section_label(s, open), HardwareAction::ToggleSection { section: s }, Look::Ghost, true);
            head.entry::<Node>().and_modify(|mut n| {
                n.flex_grow = 1.0;
                n.justify_content = JustifyContent::FlexStart;
            });
            control(r, k, "stop", "Stop", HardwareAction::Stop, Look::Danger, false);
        });
        c.spawn((Node { display: if open { Display::Flex } else { Display::None }, ..column(8.0) }, Shown::Section(s))).with_children(content);
    });
}
/// The pinned top bar: title, STOP, ×; the connection line with Connect; the notice.
pub(super) fn top_bar(root: &mut ChildSpawnerCommands, k: &Kit) {
    root.spawn((Node { padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)), border: UiRect::bottom(Val::Px(1.0)), ..column(6.0) }, BorderColor::all(BORDER))).with_children(|top| {
        top.spawn(row()).with_children(|r| {
            r.spawn((k.title("Leg calibration"), Node { flex_grow: 1.0, ..default() }));
            control(r, k, "stop", "Z  Stop", HardwareAction::Stop, Look::Danger, false);
            control(r, k, "close", "×", HardwareAction::ClosePanel, Look::Ghost, false);
        });
        top.spawn(row()).with_children(|r| {
            r.spawn((k.caption(""), PanelText::Connection, Node { flex_grow: 1.0, min_width: Val::Px(0.0), ..default() }));
            control(r, k, "connect", "Connect", HardwareAction::Connect, Look::Secondary, true).insert(Shown::Connect);
            control(r,k,"disconnect","Disconnect calibration",HardwareAction::Disconnect,Look::Secondary,true);
        });
        top.spawn((k.text("", size::SMALL, WARN, 0), PanelText::Notice));
    });
}

/// Everything under the top bar (:12-62), then Real motor sync.
pub(super) fn body(p: &mut ChildSpawnerCommands, k: &Kit, dial: Handle<Image>, motion: Handle<Image>) {
    p.spawn(k.caption(HINT));
    p.spawn((wrap(), PanelList { kind: ListKind::Chips, key: None }));
    p.spawn(row()).with_children(|r| {
        grow(control(r, k, "set_disabled", "Disable this motor", HardwareAction::SetDisabled, Look::Secondary, true));
        grow(control(r, k, "sweep_all", "Sweep all enabled motors", HardwareAction::SweepAll, Look::Secondary, true));
    });
    control(p, k, "hold_others", HOLD_OTHERS, HardwareAction::HoldOthers { on: false }, Look::Chip(true), false);
    p.spawn((k.caption(""), PanelText::Sequence));
    p.spawn((k.text("", size::SMALL, WARN, 0), PanelText::Warnings));
    p.spawn((Node { border: UiRect::left(Val::Px(3.0)), padding: UiRect::left(Val::Px(10.0)), min_height: Val::Px(40.0), align_items: AlignItems::Center, flex_shrink: 0.0, ..default() }, BorderColor::all(ACCENT)))
        .with_children(|b| {
            b.spawn((k.text("Choose the motor you want to calibrate.", size::BODY, TEXT, 0), PanelText::Status));
        });
    p.spawn(row()).with_children(|r| {
        for (id, label, direction) in [("jog_upper", "Q  Upper ↑", Direction::Upper), ("jog_lower", "A  Lower ↓", Direction::Lower)] {
            let mut e = control(r, k, id, label, HardwareAction::JogPress { direction }, Look::Secondary, false);
            // Hold-to-move requires paired release; never generic Enter/Space activation.
            e.insert((JogButton { direction, held: false }, crate::ui_kit::activation::HeldControl));
            grow(e);
        }
    });
    labelled(p, k, "Movement speed", PanelText::SpeedValue);
    slider(p, k, PanelSlider::Speed, 0.0, "Movement speed");
    p.spawn(between()).with_children(|r| {
        r.spawn(k.caption("Slow crawl"));
        r.spawn(k.caption("Faster"));
    });
    p.spawn(row()).with_children(|r| {
        for (i, (id, label, boundary)) in [("capture_lower", "Save lower here", Boundary::Lower), ("capture_upper", "Save upper here", Boundary::Upper)].into_iter().enumerate() {
            r.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), ..column(2.0) }).with_children(|c| {
                control(c, k, id, label, HardwareAction::Capture { boundary }, Look::Secondary, false);
                c.spawn((k.caption("Not taught"), PanelText::Pose(i)));
            });
        }
    });
    control(p, k, "capture_reference", "Save sim alignment here", HardwareAction::Capture { boundary: Boundary::Reference }, Look::Secondary, false);
    p.spawn((k.caption("Not aligned"), PanelText::Pose(2)));
    p.spawn((k.caption(""), PanelText::Capture));
    p.spawn(row()).with_children(|r| {
        grow(control(r, k, "sweep", "Try saved range", HardwareAction::Sweep, Look::Secondary, true));
        grow(control(r, k, "reset_poses", "Reset poses", HardwareAction::ResetPoses, Look::Secondary, true));
    });
    control(p, k, "learn", "Learn motion in the middle", HardwareAction::Learn, Look::Secondary, true);
    p.spawn((k.caption(LEARN_TEXT), PanelText::Learning));
    section(p, k, Section::Tune, |c| {
        c.spawn(k.caption(TUNE_TEXT));
        control(c, k, "tune_confirm", TUNE_OK, HardwareAction::TuneConfirm { on: true }, Look::Chip(false), false);
        control(c, k, "tune", "Tune this motor", HardwareAction::Tune, Look::Secondary, true);
        c.spawn((k.caption(""), PanelText::TuneStatus));
    });
    section(p, k, Section::Campaign, |c| {
        c.spawn(k.caption(CAMPAIGN_TEXT));
        control(c, k, "campaign_confirm", CAMPAIGN_OK, HardwareAction::CampaignConfirm { on: true }, Look::Chip(false), false);
        c.spawn(row()).with_children(|r| {
            grow(control(r, k, "campaign", "Run campaign", HardwareAction::Campaign { resume: false }, Look::Secondary, true));
            grow(control(r, k, "campaign_resume", "Resume", HardwareAction::Campaign { resume: true }, Look::Secondary, false));
        });
        c.spawn((k.caption(""), PanelText::CampaignStatus));
    });
    section(p, k, Section::Gait, |c| {
        c.spawn(k.caption(GAIT_TEXT));
        c.spawn(k.text("Gait", size::BODY, TEXT, 1));
        c.spawn((column(4.0), PanelList { kind: ListKind::Gaits, key: None }));
        c.spawn(k.segments()).with_children(|seg| {
            for (mode, name) in [(GaitMode::Sim, "sim"), (GaitMode::Leg, "leg"), (GaitMode::Both, "both")] {
                control(seg, k, &format!("gait_mode_{name}"), gait_mode_label(mode), HardwareAction::GaitMode { mode }, Look::Segment(mode == GaitMode::Sim), false);
            }
        });
        labelled(c, k, "Playback speed", PanelText::GaitSpeedValue);
        slider(c, k, PanelSlider::GaitSpeed, 1.0, "Playback speed");
        labelled(c, k, "Leg effort", PanelText::GaitEffortValue);
        slider(c, k, PanelSlider::GaitEffort, 40.0 / 90.0, "Leg effort");
        c.spawn(k.caption(GOVERNOR_TEXT));
        control(c, k, "gait_confirm", GAIT_OK, HardwareAction::GaitConfirm { on: true }, Look::Chip(false), false);
        c.spawn(row()).with_children(|r| {
            grow(control(r, k, "gait_play", "Play", HardwareAction::GaitPlay, Look::Secondary, true));
            grow(control(r, k, "gait_stop", "Stop", HardwareAction::GaitStop, Look::Secondary, false));
        });
        c.spawn((k.caption(""), PanelText::GaitStatus));
        c.spawn((column(6.0), PanelList { kind: ListKind::Stats, key: None }));
        section(c, k, Section::GaitRuns, |r| {
            r.spawn((column(8.0), PanelList { kind: ListKind::Runs, key: None }));
        });
    });
    p.spawn(Node { align_items: AlignItems::Center, width: Val::Percent(100.0), margin: UiRect::top(Val::Px(8.0)), ..column(2.0) }).with_children(|d| {
        d.spawn((k.chart_image(dial, Node { width: Val::Px(215.0), height: Val::Px(68.0), ..default() }, false), DialImage));
        d.spawn(Node { width: Val::Px(215.0), ..between() }).with_children(|r| {
            r.spawn(k.caption("Lower"));
            r.spawn(k.caption("Upper"));
        });
        d.spawn((k.text("—", 26.0, TEXT, 1), PanelText::Position));
        d.spawn(k.caption(ANGLE_NOTE));
    });
    labelled(p, k, "Move to a taught pose", PanelText::TargetLabel);
    slider(p, k, PanelSlider::Target, 0.5, "Target pose between saved limits");
    p.spawn((k.caption(TARGET_NOTE), PanelText::TargetNote));
    section(p, k, Section::Mirror, |c| {
        c.spawn((column(6.0), super::mirror_panel::MirrorRoot));
    });
    section(p, k, Section::Advanced, |c| {
        c.spawn(k.text("Control mode", size::BODY, TEXT, 1));
        let mut frame = c.spawn(k.segments());
        frame.entry::<Node>().and_modify(|mut n| n.flex_direction = FlexDirection::Column);
        frame.with_children(|seg| {
            for mode in DriveMode::ALL {
                control(seg, k, &format!("drive_mode_{}", mode.wire()), mode.label(), HardwareAction::DriveMode { mode }, Look::Segment(mode == DriveMode::Pwm), false);
            }
        });
        c.spawn(k.caption(SERVO_NOTE));
        labelled(c, k, "PWM ceiling (%)", PanelText::PwmValue);
        slider(c, k, PanelSlider::Pwm, 1.0, "PWM ceiling (%)");
        c.spawn(k.caption(PWM_NOTE));
        control(c, k, "flip", "Swap upper / lower direction", HardwareAction::Flip, Look::Secondary, false);
        c.spawn(row()).with_children(|r| {
            grow(control(r, k, "clear_lower", "Reset lower only", HardwareAction::ClearLower, Look::Secondary, false));
            grow(control(r, k, "clear_upper", "Reset upper only", HardwareAction::ClearUpper, Look::Secondary, false));
        });
        c.spawn(k.text("Single raw step", size::BODY, TEXT, 1));
        c.spawn(wrap()).with_children(|r| {
            for (i, (name, label, change)) in STEPS.into_iter().enumerate() {
                if i == 3 {
                    r.spawn((k.mono("1", size::BODY, TEXT), PanelText::RawStep, Node { min_width: Val::Px(44.0), ..default() }));
                }
                control(r, k, &format!("raw_step_{name}"), label, HardwareAction::RawStepValue { delta: (1 + change).clamp(-4095, 4095) }, Look::Secondary, false);
            }
            control(r, k, "raw_step_negate", "±", HardwareAction::RawStepValue { delta: -1 }, Look::Secondary, false);
        });
        control(c, k, "raw_step", "Send raw step", HardwareAction::RawStep, Look::Secondary, false);
        c.spawn((k.caption(""), PanelText::Telemetry));
        c.spawn(k.text(motion_view::HEADING, size::BODY, TEXT, 2));
        c.spawn((k.caption(""), PanelText::MotionDirection));
        c.spawn((k.chart_image(motion, Node { width: Val::Percent(100.0), aspect_ratio: Some(crate::chart::RASTER.0 as f32 / crate::chart::RASTER.1 as f32), flex_shrink: 0.0, display: Display::None, ..default() }, true), MotionImage, Shown::Chart, PanelList { kind: ListKind::MotionLabels, key: None }));
        c.spawn((k.caption(""), PanelText::MotionPlaceholder(0)));
        c.spawn((k.caption(""), PanelText::MotionPlaceholder(1)));
        c.spawn((k.caption(""), PanelText::MotionStatus));
        c.spawn(k.note(motion_view::NOTE));
        control(c, k, "export", "Download calibration", HardwareAction::Export, Look::Secondary, false);
        c.spawn((k.caption(""), PanelText::ExportLine));
    });
    section(p, k, Section::Sync, |c| {
        c.spawn((column(6.0), super::sync_panel::SyncRoot));
    });
}

/// The motor chips (their states follow their controls).
pub(super) fn chips(p: &mut ChildSpawnerCommands, k: &Kit, v: &PanelView) {
    for c in &v.chips {
        p.spawn((k.chip(&c.label, HardwareAction::Select { id: c.id }, c.pressed, c.enabled), ControlId(format!("select_{}", c.id)), FollowLabel));
    }
}

/// The gait select: one chip per gait (lit: selected), in the list's order.
pub(super) fn gaits(p: &mut ChildSpawnerCommands, k: &Kit, v: &PanelView, selected: usize) {
    for (index, label) in v.gait.options.iter().enumerate() {
        p.spawn((k.chip(label, HardwareAction::GaitSelect { index }, index == selected, true), ControlId(format!("gait_select_{index}"))));
    }
}

/// `statsTable`: one block per motor, "header value" pairs on one line.
pub(super) fn stats(p: &mut ChildSpawnerCommands, k: &Kit, rows: &[[String; 11]]) {
    for r in rows {
        let line = STATS_HEADER[1..].iter().zip(&r[1..]).map(|(h, v)| format!("{h} {v}")).collect::<Vec<_>>().join(" · ");
        p.spawn(column(1.0)).with_children(|c| {
            c.spawn(k.text(r[0].as_str(), size::SMALL, TEXT, 1));
            c.spawn(k.mono(line, size::DETAIL, SUBTLE));
        });
    }
}

/// Recent leg runs (:232): each run's line and its statistics, or "No leg runs yet.".
pub(super) fn runs(p: &mut ChildSpawnerCommands, k: &Kit, runs: &[(String, Vec<[String; 11]>)]) {
    if runs.is_empty() {
        p.spawn(k.caption(NO_RUNS));
    }
    for (heading, rows) in runs {
        p.spawn(column(4.0)).with_children(|c| {
            c.spawn(k.text(heading.as_str(), size::SMALL, TEXT, 1));
            stats(c, k, rows);
        });
    }
}

/// The motion chart's labels: top and bottom values (counts), the time span.
pub(super) fn chart_labels(p: &mut ChildSpawnerCommands, k: &Kit, labels: &[String; 3]) {
    p.spawn(k.chart_label(format!("{} · {}", labels[0], motion_view::CHART_TITLE), Corner::TopLeft));
    p.spawn(k.chart_label(labels[1].clone(), Corner::BottomLeft));
    p.spawn(k.chart_label(labels[2].clone(), Corner::BottomRight));
}
