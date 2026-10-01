//! The one UI kit (docs/architecture/native-viewer.md §6). Every mode builds
//! its common UI from it; tokens are defined here once.
//!
//! # ui_kit contract
//!
//! Tokens (`theme`): layout `TOPBAR`, `STATUSBAR`, `LEFT_WIDTH`,
//! `RIGHT_WIDTH`, `SWITCHER_STRIP` (with `above_strip`), `MODAL_Z`; colours `BAR`, `SURFACE`, `RAISED`, `HOVER_BG`, `BORDER`,
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
//! - `input(shown, placeholder, action, focused)` / `input_selectable(…, selected)`:
//!   how a text field is drawn (tagged `text::KitInput`); the draft is the
//!   field's (`text`)
//! - `text` (`text/`): the one text field (`TextField`, `FieldId`, added
//!   with `TextFieldApp::add_text_field`), the one focus (Bevy's
//!   `InputFocus`), the one input system, `FieldMsg` (Changed, Submit,
//!   Cancel, Tab, Blur) for owners, `TextFocus` (focus, set, blur), the
//!   `typing` run condition / `Typing` parameter, and `release_held`
//! - `dock(Dock::{Top, Bottom, Left, Right, Under}, layout: Node)`: a docked panel
//! - `scroll_area(layout: Node, offset: f32)`; `wheel_delta(&mut MessageReader<MouseWheel>, line_px) -> f32`
//! - `slider(SliderLook::{Track, Timebar, Scrub}, value: f32, action, label)`: read
//!   `(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction)` each
//!   frame; it is held while both `Pressed` and `Interaction::Pressed` (`slider_held`)
//! - `pointer_surface(label, block: bool)` with `surface_point(&RelativeCursorPosition) -> Option<Vec2>`
//! - `chart_image(image, layout: Node, framed: bool)`, `chart_label(value, Corner)`
//! - `pie(commands, at: Vec2, label, entries: Vec<(label, action, enabled)>, hover: Option<usize>) -> Entity`
//!   (`pie.rs`, RoboCAD's `RadialMenu`): an absolutely placed pie root with one
//!   kit button per entry (`Look::Primary` hovered, else `Secondary`), first
//!   straight up then clockwise; the caller despawns and rebuilds it. Pure:
//!   `pie::index_at(centre, cursor, n)` (None in the `DEAD` centre) and `pie::slot(i, n)`.
//! - `palette(parent, query, rows: &[Ranked], entries: &[PaletteEntry], selected, field, row: Fn(usize) -> A)`
//!   (`palette.rs`, RoboCAD's `CommandPalette`): the search field, then the
//!   ranked rows (conflict warning in WARN, disabled rows FAINT). Pure:
//!   `palette::rank(entries, query)` and `palette::conflicts(entries)`.
//! - `form(parent, title, rows: &[FormRow], ok_enabled, hit: Fn(FormHit) -> A)`
//!   (`form.rs`, RoboCAD's input dialogs and `ArrayDialog`): labelled fields
//!   with their evaluation or error under them (as the CAD numeric bar),
//!   choice segments, checkbox chips, OK and Cancel. Pure:
//!   `form::evaluate(kind, text)` and `text::TextDraft::key(key, chord)`.
//!
//! - `path_field(parent, &PathView, hit: Fn(PathHit) -> A)` and
//!   `path_listing(parent, path, listing, hit)` (`path_field.rs`): the one
//!   path entry (input, submit, the directory's matching entries, "..",
//!   `~/` expansion). Pure: `expand`, `dir_of`, `file_of`, `matches`,
//!   `listing_key`, `pick`, `up`, `list` (call on `Pool::Io`), and
//!   `request`/`receive` over a `jobs::Latest<Listing>`.
//! - `backdrop(label, cover_strip)` (at `MODAL_Z`, above the pie; `cover_strip`
//!   true for a modal holding unsaved drafts, so the switcher can't be clicked)
//!   and `document_picker(parent, title, subtitle, status,
//!   sections: &[PickerSection], path: &PathView, scroll, list, hit: Fn(PickHit) -> A)`
//!   (`picker.rs`): a modal panel of titled sections of entries and a path field.
//!
//! Layout reservation: `SWITCHER_STRIP` (theme) is the mode switcher's strip
//! along the window's bottom. `dock` adds it to every bottom edge
//! (`Dock::Bottom`, `Left`, `Right`, `Under`; `dock_rect` is the pure
//! layout), `Dock::Strip` is the strip itself, and other bottom-anchored
//! nodes use `above_strip(px)`. Modes never add their own switcher room.
//!
//! The pie, palette, form, path field and picker hold no intent logic: the caller owns the
//! open state, the drafts, the query and the selection, passes one action
//! component per clickable part, and decides what each press means.
//!
//! Rules: widgets take their action as a component and never decide what
//! a press means; buttons stay `bevy::ui::Button` + the action component +
//! `Enabled` + label text, which is what `system_ui` discovers
//! (`builder::ui_api::collect`); interactive widgets carry an
//! `AccessibleLabel`. Outside this module, no `Tint` struct literal and no
//! `Color::srgb` literal equal to a token (`tests::ui_colours_come_from_the_kit`).
pub(crate) mod form;
pub(crate) mod palette;
pub(crate) mod path_field;
pub(crate) mod picker;
pub(crate) mod pie;
mod scroll;
mod slider;
pub(crate) mod text;
mod theme;
pub(crate) mod threads;
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
        app.add_plugins(text::TextEntryPlugin)
            .add_observer(bevy::ui_widgets::slider_self_update)
            .add_systems(PostUpdate, (widgets::repaint_buttons, widgets::repaint_tints, widgets::follow_button_text).before(bevy::ui::UiSystems::Prepare))
            .add_systems(PostUpdate, clamp_scroll_positions.after(bevy::ui::UiSystems::Layout))
            .add_systems(PostUpdate, widgets::keep_labels.after(bevy::ui::UiSystems::PostLayout).before(bevy::a11y::AccessibilitySystems::Update));
    }
}
