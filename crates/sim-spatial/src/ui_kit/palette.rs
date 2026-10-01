//! The command palette (cad-modify; RoboCAD's `CommandPalette`,
//! ui/widgets.py:56-108): a search field over id, label and category,
//! ranked as RoboCAD ranks, the first 60 shown, each row
//! "Category: Label    [keys]" with "  ⚠ conflicts with …" when one of its
//! keys is bound to another entry too.
//!
//! No intent logic: the caller passes the entries and one action
//! component per row; [`rank`] and [`conflicts`] are pure. The search
//! text is the caller's kit text field's (`ui_kit::text`): the caller
//! passes its draft as the query, drawn with `Kit::input`.
//!
//! Ported exactly, including RoboCAD's quirks: the "every character is
//! somewhere" match is not a subsequence test (order is ignored), and an
//! entry with several conflicting keys shows only the last one's warning.
//! The one addition is the entry's note (RoboCAD has none), shown as
//! "  (note)" right after the label, before the keys.
use super::Kit;
use super::theme::*;
use crate::builder::ui_api::Enabled;
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use std::collections::BTreeMap;

/// How many rows the palette shows (RoboCAD's `scored[:60]`).
pub(crate) const SHOWN: usize = 60;
/// RoboCAD's `palette.conflict` (ui/strings.py:16).
const CONFLICT: &str = "conflicts with";
/// RoboCAD's dialog minimum width (px).
const WIDTH: f32 = 520.0;
/// A row's fixed height and the gap under it (px), so the list can scroll
/// the selected row into view without measuring.
const ROW: f32 = 26.0;
const GAP: f32 = 2.0;
/// The list's height before it scrolls (px).
const LIST: f32 = 420.0;

/// One command the palette can find.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PaletteEntry {
    pub id: String,
    pub label: String,
    pub category: String,
    /// Key sequences as RoboCAD writes them ("Ctrl+F", "Shift+A, B").
    pub keys: Vec<String>,
    /// Shown after the label (for example "GUI-only"), or empty.
    pub note: String,
    pub enabled: bool,
}

/// A ranked row: the entry's index and the text RoboCAD shows for it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ranked {
    pub index: usize,
    pub text: String,
    /// The other entries' labels sharing one of its keys, if any.
    pub conflict: Option<String>,
}

/// Keys (lower-cased) bound to more than one entry, with those entries' indices.
pub(crate) fn conflicts(entries: &[PaletteEntry]) -> BTreeMap<String, Vec<usize>> {
    let mut by_key: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate() {
        for k in &e.keys {
            by_key.entry(k.to_lowercase()).or_default().push(i);
        }
    }
    by_key.retain(|_, v| v.len() > 1);
    by_key
}

/// RoboCAD's score of one entry for a query already lower-cased and
/// trimmed: 0 for no query, 1 + the (character) position of the query in
/// the label, 50 when every query character is somewhere in
/// "id label category", None when the entry is left out.
pub(crate) fn score(entry: &PaletteEntry, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    let label = entry.label.to_lowercase();
    if let Some(at) = label.find(query) {
        return Some(1 + label[..at].chars().count());
    }
    let hay = format!("{} {} {}", entry.id, entry.label, entry.category).to_lowercase();
    query.chars().all(|c| hay.contains(c)).then_some(50)
}

/// The warning suffix RoboCAD appends for a key conflict.
fn warning(others: &str) -> String {
    format!("  \u{26a0} {CONFLICT} {others}")
}

/// RoboCAD's ranking: no query lists everything (score 0); a query found
/// in the label scores 1 + its position there; else 50 when every
/// character of the query is somewhere in "id label category"; else the
/// entry is left out. Sorted by (score, label), first [`SHOWN`].
pub(crate) fn rank(entries: &[PaletteEntry], query: &str) -> Vec<Ranked> {
    let t = query.to_lowercase();
    let t = t.trim();
    let conflicts = conflicts(entries);
    let mut scored: Vec<(usize, usize)> = entries.iter().enumerate().filter_map(|(i, e)| score(e, t).map(|s| (s, i))).collect();
    // Stable, as Python's sort: equal (score, label) keep the entries' order.
    scored.sort_by(|a, b| (a.0, &entries[a.1].label).cmp(&(b.0, &entries[b.1].label)));
    scored
        .into_iter()
        .take(SHOWN)
        .map(|(_, i)| {
            let e = &entries[i];
            let mut conflict = None;
            for k in &e.keys {
                if let Some(ids) = conflicts.get(&k.to_lowercase()) {
                    let others: Vec<&str> = ids.iter().filter(|o| **o != i).map(|o| entries[*o].label.as_str()).collect();
                    conflict = Some(others.join(", "));
                }
            }
            let mut text = format!("{}: {}", e.category, e.label);
            if !e.note.is_empty() {
                text.push_str(&format!("  ({})", e.note));
            }
            if !e.keys.is_empty() {
                text.push_str(&format!("    [{}]", e.keys.join(", ")));
            }
            if let Some(others) = &conflict {
                text.push_str(&warning(others));
            }
            Ranked { index: i, text, conflict }
        })
        .collect()
}

impl Kit<'_> {
    /// The palette: the search field showing `query` (or `placeholder`;
    /// the field's accessible label is `search_label`), then one row per
    /// `rows` entry (its text, the conflict warning in the warning colour),
    /// `selected` highlighted. `field` is the search field's action
    /// component, `row(i)` row `i`'s; `list` goes on the scrolling list
    /// (the caller's wheel-scroll marker, or `()`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn palette<F: Component, A: Component, L: Bundle>(
        &self,
        parent: &mut ChildSpawnerCommands,
        query: &str,
        placeholder: &str,
        search_label: &str,
        rows: &[Ranked],
        entries: &[PaletteEntry],
        selected: usize,
        field: F,
        row: impl Fn(usize) -> A,
        list: L,
    ) {
        parent
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    width: Val::Px(WIDTH),
                    ..default()
                },
                BackgroundColor(SURFACE),
                BorderColor::all(BORDER),
            ))
            .with_children(|panel| {
                // The kit input labels itself with its text; the search field is named instead.
                panel.spawn(self.input(query, placeholder, field, true)).insert(AccessibleLabel::new(search_label));
                // Scroll so the selected row is in view (rows have a fixed height).
                let offset = (selected as f32 * (ROW + GAP) + ROW - LIST).max(0.0);
                panel.spawn((self.scroll_area(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(GAP), max_height: Val::Px(LIST), ..default() }, offset), list)).with_children(|list| {
                    for (i, r) in rows.iter().enumerate() {
                        let enabled = entries.get(r.index).is_none_or(|e| e.enabled);
                        let on = i == selected;
                        let warn = r.conflict.as_deref().map(warning);
                        let main = warn.as_deref().and_then(|w| r.text.strip_suffix(w)).unwrap_or(r.text.as_str());
                        list.spawn((
                            Button,
                            row(i),
                            Enabled(enabled),
                            Tint::selectable(on),
                            AccessibleLabel::new(r.text.as_str()),
                            Node {
                                height: Val::Px(ROW),
                                padding: UiRect::axes(Val::Px(8.0), Val::Px(0.0)),
                                align_items: AlignItems::Center,
                                border: UiRect::left(Val::Px(2.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                overflow: Overflow::clip(),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            BorderColor::all(if on { ACCENT } else { Color::NONE }),
                            BackgroundColor(if on { ACCENT_BG } else { Color::NONE }),
                        ))
                        .with_children(|line| {
                            line.spawn(self.row_text(main, if enabled { TEXT } else { FAINT }, if on { 1 } else { 0 }));
                            if let Some(w) = &warn {
                                line.spawn(self.row_text(w.as_str(), WARN, 0));
                            }
                        });
                    }
                });
            });
    }

    /// A row's text: the kit's body text on one line (the row has a fixed
    /// height and clips, so a long row is cut at the panel's edge, not wrapped).
    fn row_text(&self, value: &str, color: Color, weight: u8) -> super::widgets::TextBundle {
        let (text, font, tint, _) = self.text(value, size::BODY, color, weight);
        (text, font, tint, TextLayout::no_wrap())
    }
}
