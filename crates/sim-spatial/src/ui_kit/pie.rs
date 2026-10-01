//! The pie (radial) menu (cad-modify; RoboCAD's `RadialMenu`,
//! ui/widgets.py:821-882): entries around the point it opens at, the
//! first straight up and the rest clockwise; the entry under the pointer
//! is highlighted; a release or click on one runs it; Escape or a click
//! in the 18 px dead centre closes it.
//!
//! No intent logic: the caller owns whether it is open and where, passes
//! one action component per entry, and asks [`index_at`] which entry a
//! pointer position picks.
//!
//! Look: RoboCAD paints the hovered entry (90,150,255,230) and the others
//! (40,42,48,220) as 92×44 ellipses; here they are kit buttons of that size,
//! `Look::Primary` (ACCENT) for the hovered entry when it is enabled and
//! `Look::Secondary` (RAISED) for the rest, so hover, disabled and label styling are the
//! kit's (`repaint_buttons` keeps the look's 5 px corner radius, not an
//! ellipse). RoboCAD's translucent white centre dot is a kit `dot(FAINT)`.
use super::Kit;
use super::theme::*;
use crate::builder::ui_api::Enabled;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;

/// RoboCAD's radius from the centre to each entry (px).
pub(crate) const RADIUS: f32 = 78.0;
/// The dead centre: no entry within this distance (px).
pub(crate) const DEAD: f32 = 18.0;
/// The pie's square side (RoboCAD's `2 * radius + 120`).
pub(crate) const SIDE: f32 = 2.0 * RADIUS + 120.0;
/// One entry's pill (RoboCAD's 92×44 label rectangle).
const PILL: Vec2 = Vec2::new(92.0, 44.0);
/// Above every panel and overlay (the viewer's other `GlobalZIndex`es are
/// 22–40), as RoboCAD's popup is above its window.
const Z: i32 = 45;

/// The entry a pointer at `cursor` picks in a pie of `n` entries opened at
/// `centre` (RoboCAD's `_index_at`), or None in the dead centre.
pub(crate) fn index_at(centre: Vec2, cursor: Vec2, n: usize) -> Option<usize> {
    let d = cursor - centre;
    if n == 0 || d.x.hypot(d.y) < DEAD {
        return None;
    }
    // Degrees clockwise from straight up (screen y points down).
    let ang = (d.y.atan2(d.x).to_degrees() + 90.0 + 360.0) % 360.0;
    let n_f = n as f32;
    let sector = 360.0 / n_f;
    Some(((ang + 180.0 / n_f) / sector).floor() as usize % n)
}

/// Entry `i` of `n`'s centre, relative to the pie's centre (px, y down).
pub(crate) fn slot(i: usize, n: usize) -> Vec2 {
    let a = i as f32 * std::f32::consts::TAU / n.max(1) as f32 - std::f32::consts::FRAC_PI_2;
    Vec2::new(RADIUS * a.cos(), RADIUS * a.sin())
}

impl Kit<'_> {
    /// The pie, absolutely placed at `at` (window logical px): one pill per
    /// entry `(label, action, enabled)`, `hover` highlighted, and a centre
    /// dot. `label` is its accessible label ("View radial menu").
    pub(crate) fn pie<A: Component>(&self, commands: &mut Commands, at: Vec2, label: &str, entries: Vec<(String, A, bool)>, hover: Option<usize>) -> Entity {
        let half = SIDE / 2.0;
        let n = entries.len();
        commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(at.x - half), top: Val::Px(at.y - half), width: Val::Px(SIDE), height: Val::Px(SIDE), ..default() },
                GlobalZIndex(Z),
                // The click that picks an entry may land between pills; it
                // must not also press a panel under the pie.
                FocusPolicy::Block,
                AccessibleLabel::new(label),
            ))
            .with_children(|pie| {
                for (i, (text, action, enabled)) in entries.into_iter().enumerate() {
                    // A disabled entry is never lit, even under the pointer.
                    let look = if enabled && hover == Some(i) { Look::Primary } else { Look::Secondary };
                    let p = look.paint(enabled);
                    let centre = Vec2::splat(half) + slot(i, n);
                    let node = Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(centre.x - PILL.x / 2.0),
                        top: Val::Px(centre.y - PILL.y / 2.0),
                        width: Val::Px(PILL.x),
                        height: Val::Px(PILL.y),
                        border_radius: BorderRadius::all(Val::Px(p.radius)),
                        padding: UiRect::axes(Val::Px(p.pad.0), Val::Px(p.pad.1)),
                        border: UiRect::all(Val::Px(1.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    };
                    let face = self.text(text.as_str(), p.size, p.text, p.weight);
                    pie.spawn((Button, action, Enabled(enabled), look, AccessibleLabel::new(text), node, BorderColor::all(p.border), BackgroundColor(p.idle), children![face]));
                }
                pie.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(half - 3.5), top: Val::Px(half - 3.5), ..default() }, children![self.dot(FAINT)]));
            })
            .id()
    }
}
