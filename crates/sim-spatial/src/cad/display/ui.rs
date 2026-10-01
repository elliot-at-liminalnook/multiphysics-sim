//! The display panel at the 3D view's top right (kit widgets; it holds no
//! intent): the view cube, then the display toolbar.
//!
//! - **View cube**: RoboCAD draws a small shaded cube in the viewport's
//!   corner and `view_cube_hit` picks the face most toward the camera under
//!   the click; here it is a compact 3 × 3 cube net of kit buttons (Top;
//!   Left, Front, Right; Iso, Bottom, Back), not a face-labelled 3D cube.
//!   The face the camera looks at now ([`facing`], `view_cube_hit` at the
//!   cube's centre) is lit. A press writes `CameraAction::View` (the shared
//!   camera applies it), or `CameraAction::Opposite` when the camera is
//!   already at that face (RoboCAD's second click shows the opposite).
//!   Shown while `CadDisplay::view_cube`.
//! - **Toolbar**: the six display modes as a segmented column, chips for
//!   grid, build plate, section, high contrast and the view cube, and while
//!   the section is on its X / Y / Z planes (through the drawn bodies'
//!   centre) and Rotate, and the offset field (`entry`: RoboCAD's Section
//!   tool's Tab offset, mm along the plane's normal). Each button carries
//!   `panel::CadButton` with the `CadDisplay` / `CadSection` action
//!   `panel::buttons` writes.
//!
//! Placement: the selection strip holds the view's top left and the numeric
//! bar its bottom; a pick tool's form opens at this same corner above
//! everything (`GlobalZIndex`), covering the panel while it is open.
use super::entry::{OffsetInput, OffsetTyping, SectionEntry};
use super::{CadDisplay, DisplayArgs, DisplayMode, DisplaySetting, SectionArgs, SectionAxis};
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::cad::actions::CadAction;
use crate::cad::mesh::CadMeshes;
use crate::cad::panel::CadButton;
use crate::cad::surfaces::COMMAND_BAR;
use crate::camera::{CameraAction, Orbit, OrbitRules, ViewPreset, display_to_robocad};
use crate::ui_kit::{BAR, BORDER, DANGER, Kit, RIGHT_WIDTH, TOPBAR, UiFonts, size, wrap};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;

/// The panel's width (logical px).
const WIDTH: f32 = 196.0;

/// A view cube face button: the preset it shows.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct CubeButton(pub ViewPreset);

/// The panel's root.
#[derive(Component)]
pub(super) struct DisplayPanel;

/// RoboCAD's cube face normals and names (`view_cube_hit`).
const FACES: [([f32; 3], ViewPreset); 6] = [
    ([1.0, 0.0, 0.0], ViewPreset::Right),
    ([-1.0, 0.0, 0.0], ViewPreset::Left),
    ([0.0, 1.0, 0.0], ViewPreset::Back),
    ([0.0, -1.0, 0.0], ViewPreset::Front),
    ([0.0, 0.0, 1.0], ViewPreset::Top),
    ([0.0, 0.0, -1.0], ViewPreset::Bottom),
];

/// RoboCAD's `view_cube_hit`: the face whose normal is most along
/// `right·dx + up·dy + back·0.9` (the camera basis in RoboCAD's model frame,
/// `dx`, `dy` the click's offset from the cube's centre in half sizes, y up).
pub fn cube_face(right: Vec3, up: Vec3, back: Vec3, dx: f32, dy: f32) -> ViewPreset {
    let d = right * dx + up * dy + back * 0.9;
    let mut best = FACES[0];
    for face in FACES {
        if Vec3::from_array(face.0).dot(d) > Vec3::from_array(best.0).dot(d) {
            best = face;
        }
    }
    best.1
}

/// The face the camera looks at: `view_cube_hit` at the cube's centre, for
/// a turntable heading in RoboCAD's degrees (its eye direction is
/// `(cos p cos y, cos p sin y, sin p)`).
pub fn facing(yaw_deg: f32, pitch_deg: f32) -> ViewPreset {
    let (y, p) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    let back = Vec3::new(p.cos() * y.cos(), p.cos() * y.sin(), p.sin());
    cube_face(Vec3::ZERO, Vec3::ZERO, back, 0.0, 0.0)
}

fn wrap_angle(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// What a press on `view`'s face writes: the view, or (RoboCAD's second
/// click on a cube face) the opposite when the turntable `heading` (display
/// radians; None for a trackball) already is that face's, pitch clamped to
/// `pitch_limit` as the camera clamps it.
pub fn cube_action(view: ViewPreset, heading: Option<(f32, f32)>, pitch_limit: f32) -> CameraAction {
    let (yaw, pitch) = view.yaw_pitch();
    let at = heading.is_some_and(|(y, p)| wrap_angle(y - yaw).abs() < 1e-3 && (p - pitch.clamp(-pitch_limit, pitch_limit)).abs() < 1e-3);
    if at && view != ViewPreset::Iso { CameraAction::Opposite } else { CameraAction::View { view } }
}

/// Input: a pressed, enabled cube button writes its camera action.
#[allow(clippy::type_complexity)]
pub(super) fn cube_press(clicks: Query<(&Interaction, &CubeButton, Option<&Enabled>), (Changed<Interaction>, With<Button>)>, cameras: Query<(&Orbit, &OrbitRules), With<Camera3d>>, mut out: MessageWriter<Act<CameraAction>>) {
    for (interaction, button, enabled) in &clicks {
        if *interaction != Interaction::Pressed || enabled.is_some_and(|e| !e.0) {
            continue;
        }
        let camera = cameras.iter().next();
        let heading = camera.filter(|(o, _)| o.trackball.is_none()).map(|(o, _)| o.heading());
        let limit = camera.map_or(89.5f32.to_radians(), |(_, r)| r.pitch_limit);
        out.write(Act::ui(cube_action(button.0, heading, limit)));
    }
}

/// What the panel shows (it is rebuilt when this changes).
#[derive(Clone, PartialEq)]
pub(super) struct Shown {
    root: Entity,
    mode: DisplayMode,
    toggles: [bool; 5],
    section: bool,
    cube: bool,
    facing: Option<ViewPreset>,
    centre: Option<[i64; 3]>,
    offset: Option<OffsetTyping>,
}

/// Present: the panel (spawned once; its content rebuilt when what it
/// shows changes).
#[allow(clippy::too_many_arguments)]
pub(super) fn toolbar(
    mut commands: Commands,
    display: Option<Res<CadDisplay>>,
    meshes: Option<Res<CadMeshes>>,
    entry: Option<Res<SectionEntry>>,
    cameras: Query<&Orbit, With<Camera3d>>,
    fonts: Res<UiFonts>,
    roots: Query<Entity, With<DisplayPanel>>,
    mut last: Local<Option<Shown>>,
) {
    let Some(display) = display else { return };
    let root = match roots.iter().next() {
        Some(root) => root,
        None => commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(RIGHT_WIDTH + 8.0),
                    top: Val::Px(TOPBAR + COMMAND_BAR + 8.0),
                    width: Val::Px(WIDTH),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::all(Val::Px(6.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(BAR),
                BorderColor::all(BORDER),
                FocusPolicy::Block,
                ZIndex(1),
                AccessibleLabel::new("CAD view cube and display"),
                DisplayPanel,
                DespawnOnExit(ModeScope::Cad),
            ))
            .id(),
    };
    let facing = cameras.iter().next().filter(|o| o.trackball.is_none()).map(|o| {
        let (yaw, pitch) = o.heading();
        let (y, p) = display_to_robocad(yaw, pitch);
        self::facing(y, p)
    });
    let bounds = meshes.as_deref().and_then(super::section::model_bounds);
    let centre = bounds.map(|(lo, hi)| [0, 1, 2].map(|i| ((lo[i] + hi[i]) / 2.0 * 1000.0).round() as i64));
    let shown = Shown { root, mode: display.mode, toggles: DisplaySetting::ALL.map(|s| s.get(&display)), section: display.section.enabled, cube: display.view_cube, facing, centre, offset: entry.as_ref().and_then(|e| e.typing.clone()) };
    if last.as_ref() == Some(&shown) {
        return;
    }
    let offset = shown.offset.clone();
    *last = Some(shown);
    let k = Kit::new(&fonts);
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|p| {
        if display.view_cube {
            cube(p, &k, facing);
        }
        p.spawn(k.caption("Display"));
        p.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), padding: UiRect::all(Val::Px(2.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(6.0)), ..default() }, BorderColor::all(BORDER)))
            .with_children(|s| {
                for m in DisplayMode::ALL {
                    s.spawn(k.segment(m.label(), CadButton(CadAction::CadDisplay(DisplayArgs { mode: Some(m), ..default() })), m == display.mode, true));
                }
            });
        p.spawn(wrap()).with_children(|r| {
            let chip = |s: DisplaySetting, label: &str| k.chip(label, CadButton(CadAction::CadDisplay(DisplayArgs { toggle: Some(s), ..default() })), s.get(&display), true);
            r.spawn(chip(DisplaySetting::Grid, "Grid"));
            r.spawn(chip(DisplaySetting::BuildPlate, "Plate"));
            r.spawn(k.chip("Section", CadButton(CadAction::CadSection(SectionArgs::default())), display.section.enabled, true));
            r.spawn(chip(DisplaySetting::HighContrast, "Contrast"));
            r.spawn(chip(DisplaySetting::ViewCube, "Cube"));
        });
        if display.section.enabled {
            p.spawn(wrap()).with_children(|r| {
                for axis in SectionAxis::ALL {
                    let offset = bounds.map_or(0.0, |(lo, hi)| (lo[axis.index()] + hi[axis.index()]) / 2.0);
                    let on = display.section.plane.is_some_and(|pl| pl.normal[axis.index()].abs() > 1.0 - 1e-9);
                    let action = CadAction::CadSection(SectionArgs { axis: Some(axis), offset: Some(offset), ..default() });
                    r.spawn(k.chip(&axis.name().to_uppercase(), CadButton(action), on, true));
                }
                r.spawn(k.chip("Rotate", CadButton(CadAction::CadSection(SectionArgs { rotate: true, ..default() })), false, true));
            });
            // RoboCAD's Section tool's Tab field: Enter moves the plane this far along its normal.
            p.spawn(k.caption("Move along normal (mm)"));
            let typing = offset.as_ref();
            p.spawn(k.input_selectable(typing.map_or("", |t| t.draft.text.as_str()), "offset, e.g. 5 or 2 cm", OffsetInput, typing.is_some(), typing.is_some_and(|t| t.draft.select_all)));
            if let Some(e) = typing.and_then(|t| t.error.as_ref()) {
                p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
            }
        }
    });
}

/// The cube net: Top over Left, Front, Right; Iso, Bottom, Back under them.
fn cube(p: &mut ChildSpawnerCommands, k: &Kit, facing: Option<ViewPreset>) {
    const NET: [(ViewPreset, &str, i16, i16); 7] = [
        (ViewPreset::Top, "Top", 1, 2),
        (ViewPreset::Left, "Left", 2, 1),
        (ViewPreset::Front, "Front", 2, 2),
        (ViewPreset::Right, "Right", 2, 3),
        (ViewPreset::Iso, "Iso", 3, 1),
        (ViewPreset::Bottom, "Bottom", 3, 2),
        (ViewPreset::Back, "Back", 3, 3),
    ];
    p.spawn(k.caption("View"));
    p.spawn(Node { display: Display::Grid, grid_template_columns: RepeatedGridTrack::flex(3, 1.0), row_gap: Val::Px(2.0), column_gap: Val::Px(2.0), ..default() }).with_children(|g| {
        for (view, label, row, column) in NET {
            g.spawn(Node { grid_row: GridPlacement::start(row), grid_column: GridPlacement::start(column), flex_direction: FlexDirection::Column, ..default() }).with_children(|cell| {
                cell.spawn(k.segment(label, CubeButton(view), facing == Some(view), true));
            });
        }
    });
}
