//! RoboCAD's "Robot: motor library…" (`robot_motor_library`, ui/app.py:
//! 1644-1647) as a floating kit panel over the 3D view's lower left
//! corner, shown while `ToolsState::library_open`: one monospaced row per
//! motor exactly as RoboCAD's text box writes it (`f"{name:<28} {kind:<13}
//! {stall_torque:>7g} N·m {no_load_speed:>6g} rad/s {mass_g:>6g} g
//! {notes}"`), every value from `GET /motors` as last read
//! (`RobotData::motor_library`), in the library's id order. Close is a
//! `CadButton` writing `cad_robot {op: library, open: false}`. Display only.
use super::super::{RobotArgs, RobotOp};
use crate::app::ModeScope;
use crate::cad::document::CadDocument;
use crate::cad::ops::g;
use crate::cad::panel::CadButton;
use crate::ui_kit::{BORDER, DANGER, Kit, LEFT_WIDTH, Look, STATUSBAR, SUBTLE, SURFACE, TEXT, UiFonts, above_strip, size};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::{Value, json};

/// Below the command popups (44) and the mode switcher (40), as the Saved Views panel.
const PANEL_Z: i32 = 30;

/// The panel's root (rebuilt whole when what it shows changes).
#[derive(Component)]
pub(super) struct LibraryRoot;

/// RoboCAD's rows, or None while the library is not read.
pub(super) fn rows(doc: &CadDocument) -> Option<Vec<String>> {
    let lib = doc.robot.data.motor_library(doc)?;
    Some(lib.values().map(|m| format!("{:<28} {:<13} {:>7} N·m {:>6} rad/s {:>6} g   {}", m.name, m.kind, g(m.stall_torque), g(m.no_load_speed), g(m.mass_g), m.notes)).collect())
}

/// The library as `cad_state.robot.tools.library` and `cad_robot {op: library}` answer it.
pub(super) fn json(doc: &CadDocument) -> Value {
    match (rows(doc), doc.robot.data.motor_library_error()) {
        (Some(rows), _) => json!({"rows": rows}),
        (None, Some(e)) => json!({"error": e}),
        (None, None) => json!({"reading": true}),
    }
}

/// What the panel shows now (None: hidden).
fn panel_key(doc: &CadDocument) -> Option<String> {
    doc.robot.tools.library_open.then(|| format!("{:?}", (rows(doc), doc.robot.data.motor_library_error())))
}

/// Present: the panel, rebuilt when what it shows changes.
pub(super) fn draw(mut commands: Commands, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<LibraryRoot>>, mut shown: Local<Option<String>>) {
    let want = doc.as_deref().and_then(panel_key);
    let present = roots.iter().next().is_some();
    if *shown == want && present == want.is_some() {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    *shown = want;
    let Some(doc) = doc.as_deref() else { return };
    if shown.is_none() {
        return;
    }
    let k = Kit::new(&fonts);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(LEFT_WIDTH + 8.0),
                bottom: above_strip(STATUSBAR + 8.0),
                max_width: Val::Px(760.0),
                max_height: Val::Percent(60.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER),
            FocusPolicy::Block,
            GlobalZIndex(PANEL_Z),
            AccessibleLabel::new("Motor library"),
            LibraryRoot,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|p| {
            p.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() }).with_children(|r| {
                r.spawn(k.title("Motor library"));
                r.spawn(k.button("Close", CadButton(RobotArgs::of(RobotOp::Library, Some(false))), Look::Ghost, true));
            });
            match (rows(doc), doc.robot.data.motor_library_error()) {
                (Some(rows), _) => {
                    p.spawn(k.caption("RoboCAD's motor library: stall torque, no-load speed, mass and notes as RoboCAD lists them."));
                    for row in rows {
                        p.spawn(k.mono(row, size::SMALL, TEXT));
                    }
                }
                (None, Some(e)) => {
                    p.spawn(k.text(format!("Could not read RoboCAD's motor library: {e}"), size::SMALL, DANGER, 0));
                }
                (None, None) => {
                    p.spawn(k.text("Reading RoboCAD's motor library…", size::SMALL, SUBTLE, 0));
                }
            }
        });
}
