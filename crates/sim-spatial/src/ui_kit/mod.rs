//! The one UI kit (docs/architecture/native-viewer.md §6). Every mode builds
//! its common UI from it; tokens are defined here once.
//!
//! # ui_kit contract
//!
//! Tokens (`theme`): layout `TOPBAR`, `STATUSBAR`, `LEFT_WIDTH`,
//! `RIGHT_WIDTH`; colours `BAR`, `SURFACE`, `RAISED`, `HOVER_BG`, `BORDER`,
//! `TEXT`, `SUBTLE`, `FAINT`, `VALUE`, `ACCENT`, `ACCENT_HOVER`, `ACCENT_BG`,
//! `ON_ACCENT`, `WARN`, `DANGER`, `DANGER_HOVER`, `DANGER_EDGE`, `OK`; type
//! sizes `size::{TITLE, PRODUCT, ITEM, BODY, SMALL, CAPTION, DETAIL,
//! SECTION}`; `WHEEL_LINE`. `UiFonts` (resource: `regular`, `medium`,
//! `semibold`, `italic`, `mono`, `icons`; `weight(u8)`).
//!
//! Styling components: `Look` (a kit button's look: `Primary`, `Secondary`,
//! `Ghost`, `Danger`, `Tab(on)`, `Chip(on)`, `Segment(on)`; write a new one
//! to restyle a live button) and `Tint` (idle/hover of other clickable
//! surfaces: `Tint::CLEAR`, `Tint::RAISED`, `Tint::SURFACE`,
//! `Tint::selectable(selected)`, `Tint::new(idle, hover)`).
//!
//! `Kit { f: &UiFonts }` (or `Kit::new(&fonts)`), widgets:
//! - `text(value, size, color, weight: u8) -> TextBundle` (0 regular, 1 medium, 2 semibold)
//! - `mono(value, size, color)`, `title(value)`, `caption(value)`, `note(value)`
//! - `header(parent, title, subtitle)` spawns a title and (if not empty) a caption
//! - `button(label, action: impl Component, look: Look, enabled: bool)`
//! - `tab(label, action, on)`, `chip(label, action, on, enabled)`, `segment(label, action, on, enabled)`
//! - `tab_strip()` and `segments()`: the containers tabs and segments go in
//! - `section(title)`: inspector section heading
//! - `property(parent, key, value, unit, action: Option<A>, editing)`: property row
//! - `list_item(icon, accent: Color, title, subtitle, action, selected)`,
//!   `item(icon, title, subtitle, tag, action, selected)` (accent from the builder's tag colour)
//! - `icon(name) -> Handle<Image>`, `dot(color)`; free functions `divider()`, `wrap() -> Node`
//! - `input(shown, placeholder, action, focused)`: text-entry styling (the draft stays the builder's)
//! - `dock(Dock::{Top, Bottom, Left, Right, Under}, layout: Node)`: a docked panel
//! - `scroll_area(layout: Node, offset: f32)`; `wheel_delta(&mut MessageReader<MouseWheel>, line_px) -> f32`
//! - `slider(SliderLook::{Track, Timebar, Scrub}, value: f32, action, label)`: read
//!   `(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction)` each
//!   frame; it is held while both `Pressed` and `Interaction::Pressed` (`slider_held`)
//! - `pointer_surface(label, block: bool)` with `surface_point(&RelativeCursorPosition) -> Option<Vec2>`
//! - `chart_image(image, layout: Node, framed: bool)`, `chart_label(value, Corner)`
//!
//! Rules: widgets take their action as a component and never decide what
//! a press means; buttons stay `bevy::ui::Button` + the action component +
//! `Enabled` + label text, which is what `system_ui` discovers
//! (`builder::ui_api::collect`); interactive widgets carry an
//! `AccessibleLabel`. Outside this module, no `Tint` struct literal and no
//! `Color::srgb` literal equal to a token (`tests::ui_colours_come_from_the_kit`).
mod scroll;
mod slider;
mod theme;
mod widgets;
#[cfg(test)]
mod tests;

pub(crate) use scroll::{WHEEL_LINE, clamp_scroll_positions, wheel_delta};
pub(crate) use slider::{SliderLook, slider_held, surface_point};
pub(crate) use theme::*;
pub(crate) use widgets::{Corner, Dock, Kit, divider, wrap};

use bevy::prelude::*;

/// The kit's systems, for every mode: button and surface repaint, the
/// accessibility labels, the slider's value, and the scroll clamp.
pub struct UiKitPlugin;
impl Plugin for UiKitPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(bevy::ui_widgets::slider_self_update)
            .add_systems(PostUpdate, (widgets::repaint_buttons, widgets::repaint_tints, widgets::follow_button_text).before(bevy::ui::UiSystems::Prepare))
            .add_systems(PostUpdate, clamp_scroll_positions.after(bevy::ui::UiSystems::Layout))
            .add_systems(PostUpdate, widgets::keep_labels.after(bevy::ui::UiSystems::PostLayout).before(bevy::a11y::AccessibilitySystems::Update));
    }
}
