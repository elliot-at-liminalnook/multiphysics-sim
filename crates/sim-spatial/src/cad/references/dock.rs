//! The References dock (`Part::References`, at the top of the right dock
//! after Comments; references.py:12-77), drawn only while open. Every
//! button carries the action and enabled state `controls_of` lists for
//! `system_ui` (`panel::CadButton`); the fields are the kit's (`input`).
//!
//! In order: Close; the linked system file's status line and Link system file…,
//! Accept changes, Open in builder, Unlink; RoboCAD's intro; "＋ Add
//! reference images…" and, while open, its path field (or Link's); the
//! image list (a row selects, its chip shows or hides the image); the
//! current image's placement form (Plane as chips, Width, Origin X/Y/Z,
//! Rotation, Opacity, the lock) and Apply placement; Align view, Calibrate
//! scale, Sketch over this; the calibrate tool's line and its Real distance
//! field; Remove reference; RoboCAD's perspective note.
//!
//! Deliberately different: no preview thumbnail under the list (RoboCAD's
//! `QLabel` pixmap, references.py:41-44, 159-160): the image is drawn on its
//! plane in the view; and WebP and BMP images, which RoboCAD imports, are
//! listed but not drawn here (the dock says so for each).
use super::form::ROWS;
use super::input::{RefField, RefPath, suffixes};
use super::{BrowseKind, Control, Focus, PlaneChoice, controls_of, images, system_link};
use crate::cad::document::CadDocument;
use crate::cad::panel::CadButton;
use crate::ui_kit::path_field::PathView;
use crate::ui_kit::{ACCENT_BG, DANGER, Kit, Look, SUBTLE, TEXT, Tint, WARN, size, wrap};
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use sim_runtime::cad_client::SelectionItem;

/// RoboCAD's intro (references.py:29).
pub(crate) const INTRO: &str = "Drop images here or in the viewport. Align a view, calibrate its scale, then sketch over it.";
/// RoboCAD's note (references.py:73).
pub(crate) const PERSPECTIVE: &str = "Scale calibration assumes a flat drawing or a view square to the reference. Perspective photos can distort dimensions.";
/// Where this window differs from RoboCAD's drawing.
pub(crate) const FORMATS: &str = "PNG and JPEG images are drawn on their planes here. WebP and BMP images can be added (RoboCAD reads them) but are not drawn in this window.";

/// The control `id` (`cad:references:<id>`) as a kit button.
fn button(p: &mut ChildSpawnerCommands, k: &Kit, controls: &[Control], id: &str, look: Look) {
    if let Some((_, label, action, ready)) = controls.iter().find(|c| c.0 == format!("cad:references:{id}")) {
        p.spawn(k.button(label, CadButton(action.clone()), look, ready.is_ok()));
    }
}

/// Why control `id` is not ready, if it is not.
fn why(controls: &[Control], id: &str) -> Option<String> {
    controls.iter().find(|c| c.0 == format!("cad:references:{id}")).and_then(|c| c.3.clone().err())
}

/// One image row's subtitle: its placement read and its texture.
fn row_note(doc: &CadDocument, id: &str) -> Option<String> {
    let st = &doc.references;
    let placed = st.reads.placements.get(id);
    let mut notes: Vec<String> = Vec::new();
    match placed {
        None => notes.push("reading its placement…".into()),
        Some(p) if p.revision != doc.shown_revision() => notes.push(format!("placement of revision {}; reading revision {}…", p.revision, doc.shown_revision())),
        Some(p) => {
            if let Err(e) = &p.placement {
                notes.push(e.clone());
            }
        }
    }
    if let Some(n) = st.pixels.get(id).and_then(super::planes::Pixels::note) {
        notes.push(n);
    }
    (!notes.is_empty()).then(|| notes.join(" · "))
}

/// What the dock shows, for its part key.
pub(in crate::cad) fn key(doc: &CadDocument, _selection: &[SelectionItem]) -> String {
    let st = &doc.references;
    if !st.open {
        return "closed".into();
    }
    let ready: Vec<(String, String, bool)> = controls_of(doc).into_iter().map(|c| (c.0, c.1, c.3.is_ok())).collect();
    let rows: Vec<(String, String, bool, Option<String>)> = images(doc).into_iter().map(|n| (n.id.clone(), n.name.clone(), n.visible, row_note(doc, &n.id))).collect();
    let listing = st.listed.as_ref().map(|l| (&l.key, l.entries.len(), &l.error));
    format!(
        "{:?}",
        (system_link::line(doc), ready, rows, &st.current, &st.form, st.focus, st.select_all, &st.browse, listing, &st.calibrate, doc.edit_refusal())
    )
}

/// The References dock (see the module doc).
pub(in crate::cad) fn draw(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, _selection: &[SelectionItem]) {
    let st = &doc.references;
    if !st.open {
        return;
    }
    let controls = controls_of(doc);
    p.spawn(k.section("References"));
    // The dock's Close (RoboCAD's dock title bar button).
    button(p, k, &controls, "dock", Look::Ghost);
    // The linked system file (references.py:19-28).
    p.spawn(k.text(system_link::line(doc), size::SMALL, SUBTLE, 0));
    p.spawn(wrap()).with_children(|r| {
        for id in ["link", "accept", "open_builder", "unlink"] {
            button(r, k, &controls, id, Look::Secondary);
        }
    });
    if let Some(w) = why(&controls, "open_builder").filter(|_| system_link::builder_target(doc).is_ok()) {
        p.spawn(k.note(format!("Open in builder: {w}")));
    }
    p.spawn(k.caption(INTRO));
    button(p, k, &controls, "add", Look::Secondary);
    if let Some(b) = &st.browse {
        let label = match b.kind {
            BrowseKind::Images => "Reference images: Images (*.png *.jpg *.jpeg *.webp *.bmp)",
            BrowseKind::System => "Link system file: System files (*.system.json), JSON (*.json)",
        };
        let submit = match b.kind {
            BrowseKind::Images => "Add",
            BrowseKind::System => "Link",
        };
        let key = crate::ui_kit::path_field::listing_key(b.draft.text.trim(), suffixes(b.kind)).map(|(k, _)| k);
        let listing = st.listed.as_ref().filter(|l| key.as_deref() == Some(l.key.as_str()));
        let focused = st.focus == Some(Focus::Path);
        let text = b.draft.text.trim();
        let view = PathView { label, text: &b.draft.text, placeholder: "~/…", focused, selected: focused && st.select_all, submit: Some(submit), submit_enabled: !text.is_empty() && !text.ends_with('/'), listing };
        k.path_field(p, &view, RefPath);
        if let Some(e) = &b.error {
            p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
        }
        button(p, k, &controls, "browse_close", Look::Ghost);
    }
    p.spawn(k.note(FORMATS));
    // The image list (references.py:35-40, 132-150).
    let all = images(doc);
    if all.is_empty() {
        p.spawn(k.caption("No reference images in this document."));
    }
    for n in &all {
        let selected = st.current.as_deref() == Some(n.id.as_str());
        let row = controls.iter().find(|c| c.0 == format!("cad:references:image-{}", n.id));
        p.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, flex_shrink: 0.0, ..default() }).with_children(|line| {
            if let Some((_, _, action, _)) = row {
                line.spawn((
                    Button,
                    crate::ui_kit::activation::Ordinary,
                    CadButton(action.clone()),
                    Tint::selectable(selected),
                    AccessibleLabel::new(n.name.clone()),
                    Node { flex_grow: 1.0, min_width: Val::Px(0.0), flex_direction: FlexDirection::Column, border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), ..default() },
                    BackgroundColor(if selected { ACCENT_BG } else { Color::NONE }),
                ))
                .with_children(|cell| {
                    cell.spawn(k.text(n.name.clone(), size::BODY, TEXT, 0));
                    if let Some(note) = row_note(doc, &n.id) {
                        cell.spawn(k.text(note, size::DETAIL, SUBTLE, 0));
                    }
                });
            }
            if let Some((_, label, action, ready)) = controls.iter().find(|c| c.0 == format!("cad:references:visible-{}", n.id)) {
                line.spawn(k.chip(label, CadButton(action.clone()), n.visible, ready.is_ok()));
            }
        });
    }
    // The current image's placement form (references.py:45-63).
    if let Some(f) = st.form.as_ref().filter(|f| st.current.as_deref() == Some(f.id.as_str())) {
        p.spawn(k.caption("Plane"));
        p.spawn(wrap()).with_children(|r| {
            for choice in PlaneChoice::ALL {
                if let Some((_, label, action, ready)) = controls.iter().find(|c| c.0 == format!("cad:references:plane-{}", choice.name())) {
                    r.spawn(k.chip(label, CadButton(action.clone()), f.plane == choice, ready.is_ok()));
                }
            }
        });
        for (i, row) in ROWS.iter().enumerate() {
            let focused = st.focus == Some(Focus::Row(i));
            p.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, flex_shrink: 0.0, ..default() }).with_children(|line| {
                line.spawn(k.text(row.label, size::BODY, SUBTLE, 0));
                line.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|cell| {
                    cell.spawn(k.input_selectable(&f.texts[i], "", RefField(Focus::Row(i)), focused, focused && st.select_all)).insert(AccessibleLabel::new(format!("{} ({}): {}", row.label, row.suffix, f.texts[i])));
                    cell.spawn(k.text(row.suffix, size::BODY, SUBTLE, 0));
                });
            });
        }
        if let Some((_, label, action, ready)) = controls.iter().find(|c| c.0 == "cad:references:locked") {
            p.spawn(k.chip(label, CadButton(action.clone()), f.locked, ready.is_ok()));
        }
        button(p, k, &controls, "apply", Look::Primary);
        if let Some(e) = f.error.clone().or_else(|| why(&controls, "apply")) {
            p.spawn(k.text(e, size::SMALL, DANGER, 0));
        }
    } else if let Some(id) = &st.current {
        p.spawn(k.caption(format!("Reading the placement of {}…", doc.node_name(id))));
    }
    if st.current.is_some() {
        p.spawn(wrap()).with_children(|r| {
            for id in ["align", "calibrate", "sketch"] {
                button(r, k, &controls, id, Look::Secondary);
            }
        });
    }
    // The calibrate tool (tools.py:1210-1241).
    if let Some(c) = &st.calibrate {
        p.spawn(k.text(c.hint(), size::SMALL, WARN, 0));
        if c.picks.len() == 2 {
            let focused = st.focus == Some(Focus::Distance);
            p.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, flex_shrink: 0.0, ..default() }).with_children(|line| {
                line.spawn(k.text("Real distance", size::BODY, SUBTLE, 0));
                line.spawn(k.input_selectable(&c.distance, "mm", RefField(Focus::Distance), focused, focused && st.select_all)).insert(AccessibleLabel::new(format!("Real distance (mm): {}", c.distance)));
                line.spawn(k.text("mm", size::BODY, SUBTLE, 0));
            });
        }
        if let Some(e) = &c.error {
            p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
        }
        button(p, k, &controls, "calibrate_cancel", Look::Ghost);
    }
    if st.current.is_some() {
        button(p, k, &controls, "remove", Look::Danger);
    }
    p.spawn(k.note(PERSPECTIVE));
}
