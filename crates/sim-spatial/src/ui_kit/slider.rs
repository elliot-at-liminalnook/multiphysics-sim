//! The kit slider (Bevy's headless `bevy::ui_widgets::Slider`) and the 2D
//! pointer surface (charts that read the hovered moment, sketch canvases).
use super::theme::*;
use super::widgets::Kit;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use bevy::ui_widgets::{Slider, SliderOrientation, SliderRange, SliderValue, TrackClick};

/// The three slider tracks the viewer draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SliderLook {
    /// A parameter slider in a lesson card: 12 px, SURFACE, grows along its row.
    Track,
    /// The lesson timeline: 10 px, SURFACE, grows along its row.
    Timebar,
    /// The narration scrub bar: 6 px, RAISED, the width of its column.
    Scrub,
}

impl Kit<'_> {
    /// A horizontal slider over the fraction 0..=1 (`value`: where it is
    /// now, for accessibility; the caller maps the fraction to its own
    /// range). Behaviour is `bevy::ui_widgets::Slider` with
    /// `TrackClick::Snap`: a press jumps to the pointer and a drag follows
    /// it, each writing `SliderValue` (the kit's `slider_self_update`
    /// observer). There is no thumb, so the value is exactly the pointer's
    /// fraction across the track, clamped to 0..=1.
    ///
    /// Read it by polling, once per frame, like any held control:
    /// `Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &YourMarker)>`.
    /// `Pressed` is present from the press until the release (or the drag's
    /// end); `SliderValue.0` is the fraction. The mode turns that into its
    /// typed action; the slider holds no intent. Draw the fill and value as
    /// children or siblings, as before.
    ///
    /// It keeps `Interaction` and `FocusPolicy::Block` so nodes under it do
    /// not also see the pointer (as the old button-based bars did).
    pub(crate) fn slider<A: Component>(&self, look: SliderLook, value: f32, action: A, label: &str) -> impl Bundle + use<A> {
        let (height, background, grow) = match look {
            SliderLook::Track => (12., SURFACE, 1.),
            SliderLook::Timebar => (10., SURFACE, 1.),
            SliderLook::Scrub => (6., RAISED, 0.),
        };
        (
            Slider { track_click: TrackClick::Snap, orientation: SliderOrientation::Horizontal },
            SliderRange::new(0., 1.),
            SliderValue(value.clamp(0., 1.)),
            action,
            AccessibleLabel::new(label),
            Interaction::default(),
            FocusPolicy::Block,
            Node { border_radius: BorderRadius::all(Val::Px(height / 2.)), flex_grow: grow, height: Val::Px(height), border: UiRect::all(Val::Px(1.)), ..default() },
            BackgroundColor(background),
            BorderColor::all(BORDER),
        )
    }

    /// A surface that reads where the pointer is over it (not a slider:
    /// charts preview the hovered moment, sketch canvases draw). Read it with
    /// `Interaction` and [`surface_point`]. `block`: nodes under it do not
    /// also see the pointer.
    pub(crate) fn pointer_surface(&self, block: bool) -> impl Bundle + use<> {
        (Interaction::default(), if block { FocusPolicy::Block } else { FocusPolicy::Pass }, bevy::ui::RelativeCursorPosition::default())
    }
}

/// The pointer over a [`Kit::pointer_surface`] as a fraction of the node,
/// (0, 0) top-left to (1, 1) bottom-right; `None` when it is outside the
/// window. (`RelativeCursorPosition::normalized` is centred on the node,
/// corners at ±0.5, since Bevy 0.17.)
pub(crate) fn surface_point(cursor: &bevy::ui::RelativeCursorPosition) -> Option<Vec2> {
    cursor.normalized.map(|p| p + Vec2::splat(0.5))
}
