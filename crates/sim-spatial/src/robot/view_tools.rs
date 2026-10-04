//! The 3D view's tools the browser viewer has beside its canvas
//! (`web/viewer/index.html` view-tools, `viewer.js`): Follow robot (the camera
//! moves with the preset's `follow_link`, else the selected link), Fit
//! selected (frame one link), the display rate (automatic or a cap: display
//! updates only; physics and control keep their own rate), Space for
//! Run/Pause, and the links search (the parts list's filter). Each is a
//! `RobotAction` (REST `robot_view`, `system_ui` view:*), except the search,
//! which is list state.
use super::*;
use crate::app::actions::Act;
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFieldApp, TextFocus, Typing};

/// The display caps offered (0 = automatic: every frame the window updates).
pub const DISPLAY_RATES: [u32; 4] = [0, 15, 30, 60];
pub const DISPLAY_RULE: &str = "display updates only: a cap makes robot mode's window update at most that many times a second while it runs (reactive winit timer, no per-frame redraw requests); the run thread's physics and control keep their own pacing (robot_speed), and input still wakes the window. 0 is automatic (as before: every frame while a run is active).";
pub const FOLLOW_RULE: &str = "Follow robot moves the orbit focus by the followed link's displacement each applied frame (the camera keeps its distance and heading, as the browser moves its camera and target by the same delta): the preset's follow_link (a drive preset's), else the selected link. On by default when the preset declares follow_link.";

/// The links search field.
pub(crate) const LINK_SEARCH: FieldId = FieldId("robot.link_search");

/// Register the search field (`RobotPlugin`).
pub(super) fn add_field(app: &mut App) {
    app.add_text_field(LINK_SEARCH, TextField::new("Search links").placeholder("Search links…"));
}

/// The search field's press target.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct LinkSearchField;
/// Where the search field is drawn (top of the links list).
#[derive(Component)]
pub(super) struct LinkSearchRoot;

/// The link Follow robot follows: the preset's `follow_link` by name, else the selected link.
pub(super) fn follow_target(view: &RobotView, selected: Option<usize>) -> Option<usize> {
    let m = view.model.as_ref()?;
    match view.follow_link() {
        Some(name) => m.links.iter().position(|l| l.name == name),
        None => selected,
    }
}

/// A link's position in the display frame (model (x, y, z) → display (x, z, −y)) from the displayed poses.
fn display_position(view: &RobotView, link: usize) -> Option<Vec3> {
    let poses = match view.mirror.as_ref() {
        Some(m) => Some(m.poses.as_slice()),
        None => view.run.as_ref().and_then(|r| r.display_poses()),
    };
    let p = poses.and_then(|p| p.get(link)).and_then(|p| p.as_ref()).map(|(p, _)| *p).or_else(|| view.model.as_ref().and_then(|m| m.links.get(link)).map(|l| l.com))?;
    Some(Vec3::new(p[0] as f32, p[2] as f32, -p[1] as f32))
}

/// SimSync, after the frames are applied: Follow robot (FOLLOW_RULE).
pub(super) fn follow(view: Res<RobotView>, selection: Res<Selection>, registry: Res<DocumentRegistry>, mut orbit: Single<&mut Orbit, With<RobotCamera>>, mut last: Local<Option<(usize, Vec3)>>) {
    let target = follow_target(&view, picked::link(&selection, &registry)).filter(|_| view.follow);
    let Some(link) = target else {
        *last = None;
        return;
    };
    let Some(at) = display_position(&view, link) else { return };
    if let Some((was, before)) = *last
        && was == link
    {
        let delta = at - before;
        if delta.length_squared() > 0.0 && delta.is_finite() {
            orbit.focus += delta;
            orbit.centre += delta;
        }
    }
    *last = Some((link, at));
}

/// SimSync: a pending Fit selected frames the selected link's drawn bounds
/// (its mesh's `Aabb` through its current transform); the next Fit frames the
/// whole robot again (`RobotView::bounds`).
pub(super) fn fit_selected(
    mut view: ResMut<RobotView>,
    selection: Res<Selection>,
    registry: Res<DocumentRegistry>,
    links: Query<(&LinkMesh, &GlobalTransform, &bevy::camera::primitives::Aabb)>,
    mut orbit: Single<&mut Orbit, With<RobotCamera>>,
) {
    if !view.fit_selected {
        return;
    }
    view.fit_selected = false;
    let Some(i) = picked::link(&selection, &registry) else { return };
    let Some((_, t, aabb)) = links.iter().find(|(l, ..)| l.0 == i) else {
        view.run_message = Some("Fit selected: the selected link has no collision geometry to frame".into());
        return;
    };
    let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            for sz in [-1.0, 1.0] {
                let w = t.transform_point(c + h * Vec3::new(sx, sy, sz));
                lo = lo.min(w);
                hi = hi.max(w);
            }
        }
    }
    orbit.extent = ((hi - lo).length() / 2.0).max(0.01);
    orbit.centre = (lo + hi) / 2.0;
    orbit.home = true;
}

/// Present: the display rate as robot mode's window update mode (DISPLAY_RULE).
pub(super) fn display_rate(view: Res<RobotView>, mut winit: ResMut<bevy::winit::WinitSettings>, mut applied: Local<Option<u32>>) {
    if *applied == Some(view.display_hz) {
        return;
    }
    *applied = Some(view.display_hz);
    let focused = match view.display_hz {
        0 => bevy::winit::UpdateMode::reactive(std::time::Duration::from_secs_f64(1.0 / 60.0)),
        hz => bevy::winit::UpdateMode::reactive(std::time::Duration::from_secs_f64(1.0 / f64::from(hz))),
    };
    winit.focused_mode = focused;
}

/// Input: Space runs or pauses (the browser's Space; a planar file has its own keys).
pub(super) fn run_key(keys: Res<ButtonInput<KeyCode>>, view: Res<RobotView>, text: Typing, hardware: Option<Res<crate::robot::hardware::Hardware>>, mut out: MessageWriter<Act<RobotAction>>) {
    if !keys.just_pressed(KeyCode::Space) || text.get() || view.planar.is_some() || hardware.is_some_and(|h| h.open) {
        return;
    }
    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::AltLeft, KeyCode::AltRight]) {
        return;
    }
    let Some(run) = view.run.as_ref() else { return };
    let action = if run.check(RunAction::Pause).is_ok() { RunAction::Pause } else { RunAction::Start };
    out.write(Act::ui(RobotAction::Run { action }));
}

/// Input: the search field's press and typing (list state, not an action).
pub(super) fn search_input(presses: Query<&LinkSearchField, With<crate::ui_kit::activation::Activated>>, mut msgs: MessageReader<FieldMsg>, mut text: TextFocus, mut ui: ResMut<panel_ui::RobotPanelUi>) {
    for m in msgs.read().filter(|m| m.field == LINK_SEARCH) {
        match &m.event {
            FieldEvent::Changed(d) => {
                if ui.link_search != d.text {
                    ui.link_search = d.text.clone();
                }
            }
            FieldEvent::Submit(_) | FieldEvent::Cancel => text.blur(LINK_SEARCH),
            _ => {}
        }
    }
    if !presses.is_empty() && !text.focused(LINK_SEARCH) {
        text.focus_draft(LINK_SEARCH, TextDraft::new(ui.link_search.clone(), false));
    }
}

/// Present: the search field (redrawn when its text or focus changes) and
/// each link row shown only when its name contains the search (case-insensitive).
pub(super) fn search_draw(
    mut commands: Commands,
    ui: Res<panel_ui::RobotPanelUi>,
    typing: Typing,
    fonts: Res<UiFonts>,
    view: Res<RobotView>,
    root: Single<Entity, With<LinkSearchRoot>>,
    mut rows: Query<(&LinkRow, &mut Node)>,
    mut last: Local<Option<(Entity, String, bool)>>,
) {
    let focused = typing.focused(LINK_SEARCH);
    let key = (*root, ui.link_search.clone(), focused);
    if last.as_ref() != Some(&key) {
        let k = Kit { f: &fonts };
        commands.entity(*root).despawn_related::<Children>();
        commands.entity(*root).with_children(|p| {
            p.spawn(k.input(&ui.link_search, "Search links…", LinkSearchField, focused)).insert(AccessibleLabel::new("Search links"));
        });
        *last = Some(key);
    }
    let needle = ui.link_search.to_lowercase();
    for (row, mut node) in &mut rows {
        let name = view.link_name(row.0).unwrap_or("").to_lowercase();
        let display = if needle.is_empty() || name.contains(&needle) { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// One view-tool chip.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tool {
    FitSelected,
    Follow,
    Display,
}
pub(super) const TOOLS: [Tool; 3] = [Tool::FitSelected, Tool::Follow, Tool::Display];
impl Tool {
    pub(super) fn label(self) -> &'static str {
        match self {
            Tool::FitSelected => "Fit selected",
            Tool::Follow => "Follow robot",
            Tool::Display => "Display: automatic",
        }
    }
    /// The chip's action now: Follow flips, Display cycles automatic → 30 fps → automatic (the browser's two choices).
    pub(super) fn action(self, follow: bool, hz: u32) -> RobotAction {
        match self {
            Tool::FitSelected => RobotAction::View { fit_selected: true, follow: None, display_hz: None },
            Tool::Follow => RobotAction::View { fit_selected: false, follow: Some(!follow), display_hz: None },
            Tool::Display => RobotAction::View { fit_selected: false, follow: None, display_hz: Some(if hz == 0 { 30 } else { 0 }) },
        }
    }
}

/// Present: the view-tool chips' actions, labels, on state and visibility.
#[allow(clippy::type_complexity)]
pub(super) fn tools_panel(
    view: Res<RobotView>,
    selection: Res<Selection>,
    registry: Res<DocumentRegistry>,
    mut chips: Query<(&Tool, &mut RobotAction, &mut Look, &mut Node, &mut crate::builder::ui_api::Enabled, &Children)>,
    mut labels: Query<&mut Text>,
) {
    let selected = picked::link(&selection, &registry);
    let physical = view.model.is_some() && view.planar.is_none();
    for (tool, mut action, mut look, mut node, mut enabled, children) in &mut chips {
        let next = tool.action(view.follow, view.display_hz);
        if *action != next {
            *action = next;
        }
        let (on, label, ok) = match tool {
            Tool::FitSelected => (false, "Fit selected".to_string(), selected.is_some()),
            Tool::Follow => (view.follow, match follow_target(&view, selected).and_then(|i| view.link_name(i)) {
                Some(n) if view.follow => format!("Following {n}"),
                _ => "Follow robot".to_string(),
            }, follow_target(&view, selected).is_some() || view.follow),
            Tool::Display => (view.display_hz != 0, if view.display_hz == 0 { "Display: automatic".to_string() } else { format!("Display: {} fps", view.display_hz) }, true),
        };
        look.set_if_neq(Look::Chip(on));
        if enabled.0 != ok {
            enabled.0 = ok;
        }
        let display = if physical { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        for child in children.iter() {
            if let Ok(mut text) = labels.get_mut(child) {
                if text.0 != label {
                    text.0 = label.clone();
                }
            }
        }
    }
}
