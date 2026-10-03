//! Robot mode's UI: the entity markers, link materials, and `setup`
//! (camera, light, header, link list, inspector dock and graph dock).
use super::*;

/// Robot mode's 3D camera (its orbit state is the shared `camera::Orbit`).
#[derive(Component)]
pub(super) struct RobotCamera;
#[derive(Component)]
pub(super) struct RobotRoot;
#[derive(Component)]
pub(super) struct LinkMesh(pub(super) usize);
#[derive(Component)]
pub(super) struct LinkRow(pub(super) usize);
#[derive(Component)]
pub(super) struct StatusText;
#[derive(Component)]
pub(super) struct TitleText;
#[derive(Component)]
pub(super) struct Inspector;
#[derive(Component)]
pub(super) struct ListRoot;
#[derive(Component)]
pub(super) struct InspectorScroll;
#[derive(Component)]
pub(super) struct TabButton(pub(super) Section);
#[derive(Component)]
pub(super) struct RunButton(pub(super) RunAction);
#[derive(Component)]
pub(super) struct RunText;
#[derive(Component)]
pub(super) struct JogRoot;
#[derive(Component)]
pub(super) struct JogText(pub(super) String);
#[derive(Component)]
pub(super) struct JogButton;
#[derive(Component)]
pub(super) struct MotionRoot;
#[derive(Component)]
pub(super) struct MotionText;
#[derive(Component)]
pub(super) struct MotionButton;
#[derive(Component)]
pub(super) struct RecordingText;
/// The inspector's Recorded block (a recorded preset only; filled by `recorded_panel`).
#[derive(Component)]
pub(super) struct RecordedRoot;
#[derive(Component)]
pub(super) struct RecordedText;
#[derive(Component)]
pub(super) struct RecordedButton;
/// The inspector's Gait preview block (a preset only; filled by `gait_panel`).
#[derive(Component)]
pub(super) struct GaitRoot;
#[derive(Component)]
pub(super) struct GaitText;
#[derive(Component)]
pub(super) struct GaitError;
/// The tracked report buttons, rebuilt when the listing changes.
#[derive(Component)]
pub(super) struct GaitList;
#[derive(Component)]
pub(super) struct GaitButton;
/// A relative seek button: its action is re-resolved from the latest pose each frame.
#[derive(Component)]
pub(super) struct GaitSeekButton(pub(super) i8);
/// The inspector's Drive block for a controlled `--robot FILE` run (filled
/// by `controls::drive_panel`): the fixed part above the inspector scroll
/// (Stop, the profile's actions and the live request/commanded/deadman
/// lines) and the detail part inside it (controller, profile, geometry,
/// limits, bindings).
#[derive(Component)]
pub(super) struct DriveRoot;
#[derive(Component)]
pub(super) struct DriveDetailRoot;
/// Which Drive line a text is (rewritten by `drive_panel` only when it changes).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DriveText {
    /// Requested and commanded twist, deadman state, heartbeat.
    Live,
    /// The current device input (axes, source, last action, last error).
    Input,
    /// The run's failure (the controller's error), in the danger colour.
    Error,
    /// The active device bindings.
    Bindings,
}
#[derive(Component)]
pub(super) struct DriveButton;
#[derive(Component)]
pub(super) struct GraphDock;
/// The inspector's overlay block (`--robot FILE`): toggle buttons and the frame's counts.
#[derive(Component)]
struct OverlayRoot;
#[derive(Component)]
pub(super) struct OverlayButton(pub(super) &'static str);
/// On an overlay chip: its label (the kit button's text child) shows this overlay's on/off state and key.
#[derive(Component)]
pub(super) struct OverlayLabel(pub(super) &'static str);
#[derive(Component)]
pub(super) struct OverlayText;
/// The stress overlay's label: results path, mtime, status, peaks and scale.
#[derive(Component)]
pub(super) struct StressText;
#[derive(Component)]
pub(super) struct ReloadButton;
#[derive(Component)]
pub(super) struct SpeedButton;
/// On a speed button: whether its label (the kit button's text child) shows the requested ×scale (the middle one).
#[derive(Component)]
pub(super) struct SpeedLabel(pub(super) bool);
#[derive(Component)]
pub(super) struct ReplayText;
/// The Replay buttons (one per recent saved recording), rebuilt when the list changes.
#[derive(Component)]
pub(super) struct ReplayList;
#[derive(Resource)]
pub(super) struct Materials {
    pub(super) normal: Handle<StandardMaterial>,
    pub(super) selected: Handle<StandardMaterial>,
    /// White bases for per-vertex stress colours (selection keeps its emissive tint).
    pub(super) stress: Handle<StandardMaterial>,
    pub(super) stress_selected: Handle<StandardMaterial>,
    /// The leg mirror's links (the page's blue emissive 0x1d4a7a on the mirrored leg).
    pub(super) mirrored: Handle<StandardMaterial>,
}
/// `Materials::normal`'s base colour: the vertex colour of a link without hotspot cells while stress is shown.
pub(super) const LINK_COLOUR: Color = Color::srgb(0.62, 0.68, 0.76);

pub(super) fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>, view: Res<RobotView>, fonts: Res<UiFonts>) {
    let k = Kit { f: &fonts };
    commands.insert_resource(Materials {
        normal: materials.add(StandardMaterial { base_color: LINK_COLOUR, perceptual_roughness: 0.7, metallic: 0.05, cull_mode: None, ..default() }),
        selected: materials.add(StandardMaterial { base_color: Color::srgb(0.98, 0.62, 0.22), emissive: LinearRgba::rgb(0.35, 0.16, 0.02), perceptual_roughness: 0.6, cull_mode: None, ..default() }),
        stress: materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.7, metallic: 0.05, cull_mode: None, ..default() }),
        stress_selected: materials.add(StandardMaterial { base_color: Color::WHITE, emissive: LinearRgba::rgb(0.35, 0.16, 0.02), perceptual_roughness: 0.6, cull_mode: None, ..default() }),
        mirrored: materials.add(StandardMaterial { base_color: LINK_COLOUR, emissive: Color::srgb_u8(0x1d, 0x4a, 0x7a).into(), perceptual_roughness: 0.7, metallic: 0.05, cull_mode: None, ..default() }),
    });
    // The shared orbit camera (`crate::camera`); Camera3d's required components insert its perspective Projection.
    let orbit = Orbit { focus: Vec3::ZERO, radius: 1.0, yaw: 0.7, pitch: 0.45, centre: Vec3::ZERO, extent: 0.3, ..default() };
    commands.spawn((
        Camera3d::default(),
        MeshPickingCamera,
        Tonemapping::None,
        orbit.transform(),
        orbit,
        // Drag 0.007 rad/px, pitch within ±1.4, zoom 0.3–20 × extent; a fit is
        // the bounds at 3.2 × extent from the current heading; the docks are
        // kept up to date by `scene::view_area`.
        OrbitRules {
            rate: 0.007,
            pitch_limit: 1.4,
            radius: RadiusLimits::Extent { min: 0.3, max: 20.0 },
            framing: Framing::Bounds { scale: 3.2, aspect: false, view: None },
            glide_home: false,
            zoom_to_cursor: false,
            yield_to_ui: false,
            keys: true,
            ..default()
        },
        super::scene::wanted_area(view.graphs_visible),
        RobotCamera,
    ));
    // UI over the whole window; the 3D camera only draws the middle viewport.
    commands.spawn((Camera2d, Camera { order: 3, clear_color: ClearColorConfig::None, ..default() }, IsDefaultUiCamera));
    commands.spawn((DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(1.0, 2.0, 1.5).looking_at(Vec3::ZERO, Vec3::Y)));
    // Z-up model frame shown in Bevy's Y-up frame: model (x, y, z) → display (x, z, −y)
    // (a planar v2 file's working plane is the model's XZ plane, drawn at display z = 0, `planar::display`).
    commands.spawn((Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), Visibility::default(), RobotRoot));
    let file = view.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    // The header: title and status line (both rewritten by `panels`).
    commands.spawn((
        k.dock(Dock::Top { height: TOP }, Node { padding: UiRect::axes(Val::Px(18.0), Val::Px(8.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() }),
        children![(k.title(format!("Robot — {file}  ·  file read-only")), TitleText), (k.caption("Loading…"), StatusText)],
    ));
    // Run controls: the same handler as system_ui run:* and REST robot_run.
    // The Reload button (FILE mode) is the same `RobotAction::Reload` as the watch,
    // `system_ui` robot:reload and REST robot_reload; `panels` hides it for a preset.
    let reload = commands.spawn((k.button("Reload file", RobotAction::Reload { trigger: ReloadTrigger::Manual }, Look::Secondary, true), ReloadButton)).id();
    let shown = view.source.is_some();
    commands.entity(reload).entry::<Node>().and_modify(move |mut node| {
        node.margin = UiRect::right(Val::Px(8.0));
        node.display = if shown { Display::Flex } else { Display::None };
    });
    let mut row = vec![reload];
    for action in [RunAction::Start, RunAction::Pause, RunAction::Step, RunAction::Reset] {
        // Enabled per the run thread's check (`highlight`). Not `Look::Primary` for Start: a
        // disabled Primary keeps its accent fill, which would read as active while running.
        row.push(commands.spawn((k.button(action.label(), RobotAction::Run { action }, Look::Secondary, false), RunButton(action))).id());
    }
    for speed in [SpeedRequest::Down, SpeedRequest::Set { scale: 1.0 }, SpeedRequest::Up] {
        row.push(commands.spawn(speed_button(&k, &view, speed)).id());
    }
    // The Graphs button: the same `RobotAction::ToggleGraphs` as key G and `system_ui` graphs:toggle.
    let graphs = commands.spawn(k.button("Graphs (G)", RobotAction::ToggleGraphs, Look::Secondary, true)).id();
    commands.entity(graphs).entry::<Node>().and_modify(|mut node| node.margin = UiRect::left(Val::Px(8.0)));
    row.push(graphs);
    // The Leg calibration panel (robot::hardware): the page's header toggle.
    let hardware = commands.spawn(k.button("Leg calibration", hardware::HardwareAction::TogglePanel, Look::Secondary, true)).id();
    commands.entity(hardware).entry::<Node>().and_modify(|mut node| node.margin = UiRect::left(Val::Px(8.0)));
    row.push(hardware);
    let buttons = commands.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }).add_children(&row).id();
    let run_text = commands.spawn((k.caption(""), RunText)).id();
    // Over the header dock: sibling roots are stacked in query order, not spawn order.
    commands
        .spawn((Node { position_type: PositionType::Absolute, right: Val::Px(18.0), top: Val::Px(6.0), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, row_gap: Val::Px(3.0), ..default() }, ZIndex(1)))
        .add_children(&[buttons, run_text]);
    commands.spawn((
        k.dock(Dock::Left { top: TOP, bottom: 0.0, width: LEFT }, Node { padding: UiRect::all(Val::Px(14.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), overflow: Overflow::clip_y(), ..default() }),
        ListRoot,
        children![k.title("Links")],
    ));
    let tabs = Section::ALL.map(|section| commands.spawn((k.tab(section.label(), RobotAction::ShowSection { section }, view.section == section), TabButton(section))).id());
    let tab_strip = commands.spawn(k.tab_strip()).add_children(&tabs).id();
    // Run-thread overlay toggles (each shows its on/off state) and counts (`overlay_panel`).
    let overlays: Vec<Entity> = (0..OVERLAYS.len()).map(|i| commands.spawn(overlay_button(&k, i)).id()).collect();
    let overlay_row = commands.spawn(wrap()).add_children(&overlays).id();
    let overlay_root = commands
        .spawn((
            Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() },
            OverlayRoot,
            children![(k.text("", size::CAPTION, SUBTLE, 0), OverlayText), (k.text("", size::CAPTION, SUBTLE, 0), StressText)],
        ))
        .insert_children(0, &[overlay_row])
        .id();
    commands
        .spawn((
            k.dock(Dock::Right { top: TOP, bottom: 0.0, width: RIGHT }, Node { padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() }),
            children![
                // Teleoperation of a controlled `--robot FILE` run (spawned by `controls::drive_panel`).
                (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, DriveRoot),
                // Motion request buttons for a preset, then Save recording and Replay (a preset's or a
                // controlled run's drive Session; spawned by `motion_panel`).
                (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, MotionRoot),
                // The recorded timeline for a recorded preset (spawned by `recorded_panel`).
                (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, RecordedRoot),
                // Servo-target jog rows for the selected link's joints (rebuilt by `jog_panel`).
                (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, JogRoot),
                (
                    k.scroll_area(Node { flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() }, 0.0),
                    InspectorScroll,
                    children![
                        // The Drive block's details (controller, profile, geometry, limits, bindings).
                        (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, DriveDetailRoot),
                        (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, GaitRoot),
                        (k.text("Select a link in the list or the 3D view.", size::BODY, TEXT, 0), Inspector),
                        // The Comments section (`threads::draw`; empty in the other sections).
                        (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), flex_shrink: 0.0, ..default() }, super::threads::ThreadsRoot)
                    ],
                )
            ],
        ))
        // Tabs first, then the overlay block, above the blocks spawned with the dock.
        .insert_children(0, &[tab_strip, overlay_root]);
    // Graph dock under the 3D view (filled by `graph_dock`).
    commands.spawn((
        k.dock(Dock::Under { left: LEFT, right: RIGHT, bottom: 0.0, height: DOCK }, Node { padding: UiRect::all(Val::Px(10.0)), column_gap: Val::Px(10.0), display: Display::None, ..default() }),
        GraphDock,
    ));
}

/// A run speed button: the same `RobotAction::Speed` as keys =/+ and −, `system_ui`
/// run:speed_* and REST robot_speed. The middle one shows the requested ×scale
/// (updated by `speed_panel`) and resets to ×1.
fn speed_button(k: &Kit<'_>, view: &RobotView, speed: SpeedRequest) -> impl Bundle + use<> {
    let name = match speed {
        SpeedRequest::Down => "−",
        SpeedRequest::Up => "+",
        SpeedRequest::Set { .. } => "×1",
    };
    let action = RobotAction::Speed { speed };
    let enabled = check(view, &action).is_ok();
    (k.button(name, action, Look::Secondary, enabled), SpeedButton, SpeedLabel(matches!(speed, SpeedRequest::Set { .. })))
}

/// An overlay toggle chip: the same `RobotAction::Overlay` as its key and `system_ui` overlay:*
/// (the flipped value, the chip's on state and its label are re-resolved each frame by `overlay_panel`).
fn overlay_button(k: &Kit<'_>, i: usize) -> impl Bundle + use<> {
    let (kind, name, _) = OVERLAYS[i];
    (k.chip(name, RobotAction::Overlay { contacts: None, joints: None, deflections: None, stress: None }, false, true), OverlayButton(kind), OverlayLabel(kind))
}
