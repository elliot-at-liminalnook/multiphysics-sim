//! The document picker (window-first-usability): a modal panel of titled
//! sections of entries (presets, recent documents, examples) and a path
//! field ("Open file…"), over a backdrop that leaves the switcher strip
//! clickable. The mode switcher opens it for a mode that has no document
//! (`app::picker`).
//!
//! No intent logic: the caller owns the sections and the listing, the
//! path's text is the caller's kit text field's (`ui_kit::text`, shown
//! through `PathView`), and the caller passes one action component per part through
//! [`PickHit`]; it decides what each press means.
use super::Kit;
use super::path_field::{PathHit, PathView};
use super::theme::*;
use crate::builder::ui_api::Enabled;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;

/// The panel's width (px).
const WIDTH: f32 = 600.0;

/// One entry of a section.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PickerEntry {
    pub label: String,
    /// A second line (a path, a preset's mode), or empty.
    pub detail: String,
    pub enabled: bool,
}

/// A titled section; `empty` is shown when it has no entries.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PickerSection {
    pub title: String,
    pub entries: Vec<PickerEntry>,
    pub empty: String,
}

/// Which part of the picker was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickHit {
    /// Entry `entry` of section `section`.
    Entry(usize, usize),
    /// A part of the path field.
    Path(PathHit),
    /// "Close".
    Close,
}

impl Kit<'_> {
    /// The full-window backdrop a modal panel goes in: dimmed, taking every
    /// click, at [`MODAL_Z`] (above the pie and the switcher strip).
    /// `cover_strip`: true covers the switcher strip too, so no switch can
    /// happen while a modal holding unsaved drafts is open; false ends above
    /// it, so the switcher stays usable (a modal that holds no work, such as
    /// the document picker). `label` names the dialog for assistive technology.
    pub(crate) fn backdrop(&self, label: &str, cover_strip: bool) -> impl Bundle + use<> {
        let bottom = if cover_strip { 0.0 } else { SWITCHER_STRIP };
        (
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(bottom), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(BAR.with_alpha(0.55)),
            FocusPolicy::Block,
            GlobalZIndex(MODAL_Z),
            AccessibleLabel::new(label),
        )
    }

    /// The picker panel: `title` and `subtitle`, a `status` line (a refusal
    /// in DANGER, a note in SUBTLE) when given, the sections (scrolling;
    /// `list` goes on the scroll area: the caller's wheel marker or `()`),
    /// then the path field, then Close. Spawn it inside [`Kit::backdrop`].
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn document_picker<A: Component, L: Bundle>(
        &self,
        parent: &mut ChildSpawnerCommands,
        title: &str,
        subtitle: &str,
        status: Option<(&str, Color)>,
        sections: &[PickerSection],
        path: &PathView,
        scroll: f32,
        list: L,
        hit: impl Fn(PickHit) -> A,
    ) {
        parent
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(14.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    width: Val::Px(WIDTH),
                    max_width: Val::Percent(92.0),
                    max_height: Val::Percent(92.0),
                    ..default()
                },
                BackgroundColor(SURFACE),
                BorderColor::all(BORDER),
                AccessibleLabel::new(title),
            ))
            .with_children(|panel| {
                self.header(panel, title, subtitle);
                if let Some((line, colour)) = status {
                    panel.spawn(self.text(line, size::SMALL, colour, 0));
                }
                panel.spawn((self.scroll_area(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), flex_shrink: 1.0, min_height: Val::Px(0.0), ..default() }, scroll), list)).with_children(|body| {
                    for (s, section) in sections.iter().enumerate() {
                        body.spawn(self.section(&section.title));
                        if section.entries.is_empty() {
                            body.spawn(self.note(section.empty.clone()));
                        }
                        for (e, entry) in section.entries.iter().enumerate() {
                            body.spawn(self.picker_row(entry, hit(PickHit::Entry(s, e))));
                        }
                    }
                });
                self.path_field(panel, path, |h| hit(PickHit::Path(h)));
                panel.spawn(Node { justify_content: JustifyContent::FlexEnd, flex_shrink: 0.0, ..default() }).with_children(|buttons| {
                    buttons.spawn(self.button("Close", hit(PickHit::Close), Look::Secondary, true));
                });
            });
    }

    /// One entry: a clickable two-line row (label, detail), FAINT when disabled.
    fn picker_row<A: Component>(&self, entry: &PickerEntry, action: A) -> impl Bundle + use<A> {
        let label = if entry.detail.is_empty() { entry.label.clone() } else { format!("{}, {}", entry.label, entry.detail) };
        (
            Button,
            action,
            Enabled(entry.enabled),
            Tint::CLEAR,
            AccessibleLabel::new(label),
            Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(1.0), flex_shrink: 0.0, ..default() },
            BackgroundColor(Color::NONE),
            children![self.text(entry.label.clone(), size::ITEM, if entry.enabled { TEXT } else { FAINT }, 1), self.text(entry.detail.clone(), size::DETAIL, SUBTLE, 0)],
        )
    }
}
