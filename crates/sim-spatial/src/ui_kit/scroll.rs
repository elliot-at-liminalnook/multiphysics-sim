//! Scroll areas: the node, the wheel step and the clamp. Which panel a
//! wheel moves is still each mode's choice (its columns, by pointer
//! position), so the modes read the wheel through [`wheel_delta`].
use super::widgets::Kit;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

/// Pixels per wheel line in the builder, lesson and switcher panels.
pub(crate) const WHEEL_LINE: f32 = 28.0;

impl Kit<'_> {
    /// A vertically scrolling area with the caller's inside `layout`,
    /// starting at `offset` px. Add the mode's marker component beside it
    /// to find it again for the wheel.
    pub(crate) fn scroll_area(&self, layout: Node, offset: f32) -> impl Bundle + use<> {
        (Node { overflow: Overflow::scroll_y(), ..layout }, ScrollPosition(Vec2::new(0.0, offset)))
    }
}

/// This frame's wheel movement in pixels (positive: content moves down, as
/// the wheel's `y`), a line counting `line_px`. Reads (consumes for this
/// reader) every wheel message.
pub(crate) fn wheel_delta(wheel: &mut MessageReader<MouseWheel>, line_px: f32) -> f32 {
    wheel.read().fold(0.0, |sum, e| {
        sum + match e.unit {
            MouseScrollUnit::Line => e.y * line_px,
            MouseScrollUnit::Pixel => e.y,
        }
    })
}

/// Bevy 0.16's layout clamped `ScrollPosition` to the scrollable range (and
/// zeroed axes that do not scroll) and wrote the result back; since 0.17 it
/// only clamps a computed copy. Handlers read and accumulate the stored
/// value, so this restores the 0.16 write-back after layout.
pub(crate) fn clamp_scroll_positions(mut nodes: Query<(&mut ScrollPosition, &Node, &ComputedNode)>) {
    for (mut position, node, computed) in &mut nodes {
        let scrolls = |axis: OverflowAxis| if axis == OverflowAxis::Scroll { 1.0 } else { 0.0 };
        let max = (computed.content_size() - computed.size() + computed.scrollbar_size).max(Vec2::ZERO) * computed.inverse_scale_factor();
        let clamped = (position.0 * Vec2::new(scrolls(node.overflow.x), scrolls(node.overflow.y))).clamp(Vec2::ZERO, max);
        if clamped != position.0 {
            position.0 = clamped;
        }
    }
}
