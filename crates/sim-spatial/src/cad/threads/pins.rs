//! The numbered comment pins over the 3D view (RoboCAD's `draw_pins` and
//! `pin_at`, ui/comments.py:516-540), display only.
//!
//! - **Which**: every open thread's pin (numbered by its place in RoboCAD's
//!   list) and the pin being placed ("+"); while linked parts are shown
//!   alone, only the open thread's. None for evidence, a deleted part, a
//!   part hidden in RoboCAD or by the isolation (`threads::shown`), or with
//!   `CadDisplay::comment_pins` off ("Toggle comment pins").
//! - **Where**: the anchor point (mm, RoboCAD's frame) projected through
//!   `CadView`; the badge sits 19 px right and up of it, a 26 px circle
//!   (fill #193749, edge blue #74c8ef, amber #f6b957 when the geometry
//!   changed, neutral grey when RoboCAD's answer had no attachment state
//!   this viewer knows) with its number, joined to the point by a leader (the tools'
//!   overlay gizmo, over the bodies). A point off the view hides its pin.
//! - **How**: kit-sized UI nodes, rebuilt when the document's revision, the
//!   pins toggle or the view's readiness change, moved when the camera
//!   moves (`Res<CadView>::is_changed`); `DespawnOnExit(ModeScope::Cad)`.
//! - **Press**: a thread pin is a 32 px button around its badge (RoboCAD's
//!   16 px hit radius about the centre), a UI node, so the press is taken
//!   before the selection's and Annotate's 3D clicks (both stand aside over
//!   UI, `scene::over_ui`); it opens the thread (`cad_threads {op: open}`:
//!   the dock shown, the thread current). The "+" pin ignores the pointer.
use super::{ThreadsArgs, ThreadsOp, read, shown};
use crate::app::actions::Act;
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::cad::actions::CadAction;
use crate::cad::display::CadDisplay;
use crate::cad::document::CadDocument;
use crate::cad::transform::ToolGizmos;
use crate::cad::view::CadView;
use crate::ui_kit::{Kit, UiFonts};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use crate::cad::types::AnchorStatus;

/// RoboCAD's attached pin colour (#74c8ef).
pub(crate) const ATTACHED: Color = Color::srgb(116. / 255., 200. / 255., 239. / 255.);
/// RoboCAD's needs-review pin colour (#f6b957).
pub(crate) const REVIEW: Color = Color::srgb(246. / 255., 185. / 255., 87. / 255.);
/// A pin whose attachment is unknown (no RoboCAD state this viewer knows): neutral grey.
pub(crate) const UNKNOWN: Color = Color::srgb(0.60, 0.65, 0.71);
/// RoboCAD's pin fill (#193749).
const FILL: Color = Color::srgb(25. / 255., 55. / 255., 73. / 255.);
/// The badge's centre from the anchor, px (right, down).
const OFFSET: Vec2 = Vec2::new(19.0, -19.0);
/// The badge's radius, px.
const RADIUS: f32 = 13.0;
/// The press radius about the centre, px.
const HIT: f32 = 16.0;

/// One pin as drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    /// None: the pin being placed.
    pub thread: Option<String>,
    /// mm, RoboCAD's frame.
    pub point: Vec3,
    pub label: String,
    pub colour: Color,
}

/// A pin's node.
#[derive(Component)]
struct Pin {
    point: Vec3,
    colour: Color,
}

/// A press on a thread's pin opens it.
#[derive(Component, Clone)]
struct PinPress(String);

/// Node `id` is drawn: visible in RoboCAD and not left out by the isolation.
fn visible(doc: &CadDocument, id: &str) -> bool {
    doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).is_some_and(|n| n.effective_visible) && shown(doc, id)
}

/// The pins to draw (see the module doc); none with the pins off.
pub(crate) fn entries(doc: &CadDocument, on: bool) -> Vec<Entry> {
    if !on {
        return Vec::new();
    }
    let st = &doc.threads;
    let alone = st.isolation.is_some();
    let mut out = Vec::new();
    for (i, t) in read::listed(doc).unwrap_or(&[]).iter().enumerate() {
        if t.resolved() || (alone && st.current.as_deref() != Some(t.id.as_str())) {
            continue;
        }
        let (Some(node), Some(point)) = (&t.anchor.node_id, t.anchor.point) else { continue };
        if matches!(t.anchor_status, AnchorStatus::Missing | AnchorStatus::Evidence) || !visible(doc, node) {
            continue;
        }
        let colour = match t.anchor_status {
            AnchorStatus::NeedsReview => REVIEW,
            AnchorStatus::Unknown => UNKNOWN,
            _ => ATTACHED,
        };
        out.push(Entry { thread: Some(t.id.clone()), point: Vec3::new(point[0] as f32, point[1] as f32, point[2] as f32), label: (i + 1).to_string(), colour });
    }
    if let Some(p) = &st.pending
        && (!alone || st.current.is_none())
        && visible(doc, &p.node)
    {
        out.push(Entry { thread: None, point: Vec3::new(p.point[0] as f32, p.point[1] as f32, p.point[2] as f32), label: "+".into(), colour: ATTACHED });
    }
    out
}

/// The badge's centre in window pixels, or None when the anchor is off the view.
fn centre(view: &CadView, point: Vec3) -> Option<Vec2> {
    let at = view.project(point)?;
    view.contains(at).then_some(at + OFFSET)
}

/// Place a pin's node at `centre` (hidden without one).
fn place(node: &mut Node, centre: Option<Vec2>) {
    let (display, left, top) = match centre {
        Some(c) => (Display::Flex, Val::Px(c.x - HIT), Val::Px(c.y - HIT)),
        None => (Display::None, node.left, node.top),
    };
    if node.display != display || node.left != left || node.top != top {
        node.display = display;
        node.left = left;
        node.top = top;
    }
}

/// Present: the pins (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    display: Option<Res<CadDisplay>>,
    view: Option<Res<CadView>>,
    fonts: Res<UiFonts>,
    mut pins: Query<(Entity, &Pin, &mut Node)>,
    mut last: Local<Option<(String, bool, bool)>>,
    mut gizmos: Gizmos<ToolGizmos>,
) {
    let (Some(doc), Some(view)) = (doc, view) else {
        for (e, ..) in &pins {
            commands.entity(e).despawn();
        }
        *last = None;
        return;
    };
    let on = display.as_deref().is_none_or(|d| d.comment_pins);
    let key = (crate::cad::activation::render_key(&doc), on, view.valid);
    if last.as_ref() != Some(&key) {
        *last = Some(key);
        for (e, ..) in &pins {
            commands.entity(e).despawn();
        }
        let k = Kit::new(&fonts);
        for entry in entries(&doc, on && view.valid) {
            let at = centre(&view, entry.point);
            let mut node = Node { position_type: PositionType::Absolute, width: Val::Px(2.0 * HIT), height: Val::Px(2.0 * HIT), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
            place(&mut node, at);
            let badge = (
                Node { width: Val::Px(2.0 * RADIUS), height: Val::Px(2.0 * RADIUS), border: UiRect::all(Val::Px(2.0)), border_radius: BorderRadius::all(Val::Px(RADIUS)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BackgroundColor(FILL),
                BorderColor::all(entry.colour),
                Pickable::IGNORE,
            );
            let pin = Pin { point: entry.point, colour: entry.colour };
            let mut root = match &entry.thread {
                Some(id) => commands.spawn((Button, crate::ui_kit::activation::Ordinary, PinPress(id.clone()), AccessibleLabel::new(format!("Comment {}", entry.label)), node, pin)),
                None => commands.spawn((node, pin, Pickable::IGNORE, FocusPolicy::Pass, AccessibleLabel::new("New comment pin"))),
            };
            root.insert((BackgroundColor(Color::NONE), ZIndex(-1), DespawnOnExit(ModeScope::Cad))).with_children(|p| {
                p.spawn(badge).with_children(|b| {
                    b.spawn((k.text(entry.label.clone(), 12.0, Color::WHITE, 2), Pickable::IGNORE));
                });
            });
        }
    } else if view.is_changed() {
        for (_, pin, mut node) in &mut pins {
            let at = centre(&view, pin.point);
            place(&mut node, at);
        }
    }
    // The leaders: anchor point → badge centre, over the bodies.
    if !view.valid {
        return;
    }
    let side = |axis: Vec3| view.model_from_world.transform_vector3(view.world_from_view.transform_vector3(axis)).try_normalize().unwrap_or(Vec3::ZERO);
    let (right, up) = (side(Vec3::X), side(Vec3::Y));
    for (_, pin, node) in &pins {
        if node.display == Display::None {
            continue;
        }
        let Some(per_pixel) = view.mm_per_pixel(pin.point) else { continue };
        let end = pin.point + (right * OFFSET.x - up * OFFSET.y) * per_pixel;
        gizmos.line(view.world_from_model.transform_point3(pin.point), view.world_from_model.transform_point3(end), pin.colour);
    }
}

/// Input: a press on a thread's pin opens it.
fn press(presses: Query<&PinPress, With<crate::ui_kit::activation::Activated>>, mut out: MessageWriter<Act<CadAction>>) {
    for pin in &presses {
        {
            out.write(Act::ui(ThreadsArgs::thread(ThreadsOp::Open, Some(&pin.0))));
        }
    }
}

/// CadPlugin: the pins (Present) and their presses (Input).
pub(super) fn build(app: &mut App) {
    app.add_systems(Update, (press.in_set(crate::app::InputSet::Window), draw.in_set(ViewerSet::Present)).run_if(in_state(ViewerMode::Cad)));
}
