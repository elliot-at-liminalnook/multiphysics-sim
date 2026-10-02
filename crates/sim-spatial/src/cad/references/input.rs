//! The References dock's typing, on the kit's one text field (`ui_kit::text`):
//!
//! - the placement rows ([`FORM`], one field for the six rows;
//!   `ReferencesState::focus` is the row): a press gives a row the keyboard
//!   with its text selected (a spin box's select-all), Tab and Shift+Tab move
//!   between rows, Enter or Escape ends the entry; Apply placement is the
//!   button, as RoboCAD's (its spin boxes do not apply on Enter);
//! - the path field of Add and Link ([`PATH`], `ui_kit::path_field`): Enter or
//!   its button submits (Add: the typed image; Link: the typed system file),
//!   Escape closes it; the directory's matching files are listed from a
//!   `Pool::Io` listing (landed by `reads::receive`), a press on one fills the path;
//! - the calibrate tool's distance ([`DISTANCE`], "Real distance", mm with
//!   RoboCAD's unit expressions): Enter sends it, Escape cancels the tool.
//!
//! A field the handler opened (`ReferencesState::claim`: the path field on
//! Add and Link, the distance after the second point) takes the keyboard.
//! The kit's focus is the record; `ReferencesState::focus` mirrors it each frame.
use super::{Browse, BrowseKind, Focus, ReferencesArgs, ReferencesOp, form::ROWS};
use crate::app::actions::Act;
use crate::app::{ViewerMode};
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::ui_kit::form::{FieldKind, FieldValue, Unit, evaluate};
use crate::ui_kit::path_field::{self, PathHit};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFieldApp, TextFocus};
use bevy::ecs::system::ParamSet;
use bevy::prelude::*;

/// The placement rows' field.
pub(in crate::cad) const FORM: FieldId = FieldId("cad.references.form");
/// The path field of Add and Link.
pub(in crate::cad) const PATH: FieldId = FieldId("cad.references.path");
/// The calibrate tool's distance.
pub(in crate::cad) const DISTANCE: FieldId = FieldId("cad.references.distance");

/// A dock field: a press gives it the keyboard.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct RefField(pub Focus);

/// A part of the path field.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct RefPath(pub PathHit);

/// The listing's suffixes for `kind` (RoboCAD's dialog filters).
pub(crate) fn suffixes(kind: BrowseKind) -> &'static [&'static str] {
    match kind {
        BrowseKind::Images => &["png", "jpg", "jpeg", "webp", "bmp"],
        BrowseKind::System => &["json"],
    }
}

/// The action a path field submit writes.
pub(crate) fn path_action(b: &Browse) -> CadAction {
    let text = b.draft.text.trim().to_string();
    match b.kind {
        BrowseKind::Images => ReferencesArgs { paths: Some(vec![text]), ..ReferencesArgs::of(ReferencesOp::Add) }.action(),
        BrowseKind::System => ReferencesArgs { path: Some(text), ..ReferencesArgs::of(ReferencesOp::Link) }.action(),
    }
}

/// The distance field's value (mm, RoboCAD's unit expressions), or why not.
pub(crate) fn distance_value(text: &str) -> Result<f64, String> {
    match evaluate(&FieldKind::Number { unit: Unit::Length, min: None, max: None, decimals: 3 }, text) {
        Ok(FieldValue::Number(v)) => Ok(v),
        Ok(_) => Err("Real distance: not a number".into()),
        Err(e) => Err(format!("Real distance: {e}")),
    }
}

fn field_of(f: Focus) -> FieldId {
    match f {
        Focus::Row(_) => FORM,
        Focus::Path => PATH,
        Focus::Distance => DISTANCE,
    }
}

/// Give `f` the keyboard; returns whether its text is selected.
fn focus(text: &mut TextFocus, doc: &CadDocument, f: Focus) -> bool {
    let st = &doc.references;
    match f {
        Focus::Row(i) => {
            let t = st.form.as_ref().and_then(|form| form.texts.get(i).cloned()).unwrap_or_default();
            text.focus_draft(FORM, TextDraft::new(t, true));
            true
        }
        Focus::Path => {
            let draft = st.browse.as_ref().map(|b| TextDraft::new(b.draft.text.clone(), false)).unwrap_or_default();
            text.focus_draft(PATH, draft);
            false
        }
        Focus::Distance => {
            let t = st.calibrate.as_ref().map(|c| c.distance.clone()).unwrap_or_default();
            text.focus_draft(DISTANCE, TextDraft::new(t, true));
            true
        }
    }
}

/// Whether `f` can have the keyboard now.
fn usable(doc: &CadDocument, f: Focus) -> bool {
    let st = &doc.references;
    match f {
        Focus::Row(i) => i < ROWS.len() && st.form.is_some(),
        Focus::Path => st.browse.is_some(),
        Focus::Distance => st.calibrate.as_ref().is_some_and(|c| c.picks.len() == 2),
    }
}

/// Input: the dock's field presses and messages (see the module doc).
#[allow(clippy::type_complexity)]
fn input(
    doc: Option<ResMut<CadDocument>>,
    fields: Query<&RefField, With<crate::ui_kit::activation::Activated>>,
    paths: Query<&RefPath, With<crate::ui_kit::activation::Activated>>,
    // The fields' messages are read first, then `TextFocus` acts (a `ParamSet`: one at a time).
    mut msgs: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let events: Vec<FieldMsg> = msgs.p0().read().filter(|m| [FORM, PATH, DISTANCE].contains(&m.field)).cloned().collect();
    let mut text = msgs.p1();
    let Some(mut doc) = doc else {
        for id in [FORM, PATH, DISTANCE] {
            text.blur(id);
        }
        return;
    };
    let d = doc.bypass_change_detection();
    let (before, before_all) = (d.references.focus, d.references.select_all);
    let (mut at, mut select_all) = (before, before_all);
    let mut edited = false;
    for m in events {
        let row = match at {
            Some(Focus::Row(i)) => Some(i),
            _ => None,
        };
        match (m.field, m.event) {
            (FORM, FieldEvent::Changed(t)) => {
                if let (Some(i), Some(f)) = (row, d.references.form.as_mut())
                    && let Some(slot) = f.texts.get_mut(i)
                {
                    *slot = t.text;
                    f.error = None;
                    edited = true;
                }
                select_all = t.select_all;
            }
            (FORM, FieldEvent::Submit(_)) => {
                text.blur(FORM);
                at = None;
            }
            (FORM, FieldEvent::Tab { back }) => {
                if let Some(i) = row {
                    let next = Focus::Row(if back { (i + ROWS.len() - 1) % ROWS.len() } else { (i + 1) % ROWS.len() });
                    at = Some(next);
                    select_all = focus(&mut text, d, next);
                }
            }
            (FORM, FieldEvent::Cancel) => at = at.filter(|f| !matches!(f, Focus::Row(_))),
            (PATH, FieldEvent::Changed(t)) => {
                if let Some(b) = d.references.browse.as_mut() {
                    b.draft = t;
                    b.error = None;
                    edited = true;
                }
            }
            (PATH, FieldEvent::Submit(typed)) => {
                if let Some(b) = d.references.browse.as_mut() {
                    b.draft.text = typed;
                    out.write(Act::ui(path_action(b)));
                }
            }
            (PATH, FieldEvent::Cancel) => {
                out.write(Act::ui(ReferencesArgs { open: Some(false), ..ReferencesArgs::of(ReferencesOp::Browse) }.action()));
                at = at.filter(|f| *f != Focus::Path);
            }
            (DISTANCE, FieldEvent::Changed(t)) => {
                if let Some(c) = d.references.calibrate.as_mut() {
                    c.distance = t.text;
                    c.error = None;
                    edited = true;
                }
                select_all = t.select_all;
            }
            (DISTANCE, FieldEvent::Submit(typed)) => {
                if let Some(c) = d.references.calibrate.as_mut() {
                    c.distance = typed.clone();
                    match distance_value(&typed) {
                        Ok(v) => {
                            out.write(Act::ui(ReferencesArgs { distance: Some(v), ..ReferencesArgs::of(ReferencesOp::CalibrateDistance) }.action()));
                        }
                        Err(e) => c.error = Some(e),
                    }
                    edited = true;
                }
            }
            (DISTANCE, FieldEvent::Cancel) => {
                out.write(Act::ui(ReferencesArgs::of(ReferencesOp::Cancel).action()));
                at = at.filter(|f| *f != Focus::Distance);
            }
            // Blur: another field or a press elsewhere took the keyboard (reconciled below).
            _ => {}
        }
    }
    for f in &fields {
        if usable(d, f.0) && at != Some(f.0) {
            at = Some(f.0);
            select_all = focus(&mut text, d, f.0);
        }
    }
    for hit in &paths {
        let Some(b) = d.references.browse.clone() else { continue };
        let key = path_field::listing_key(b.draft.text.trim(), suffixes(b.kind)).map(|(k, _)| k);
        match hit.0 {
            PathHit::Field => {
                if at != Some(Focus::Path) {
                    at = Some(Focus::Path);
                    select_all = focus(&mut text, d, Focus::Path);
                }
            }
            PathHit::Entry(i) => {
                // Only the listing drawn for this path: an older one never fills it.
                if let Some(listing) = d.references.listed.as_ref().filter(|l| key.as_deref() == Some(l.key.as_str()))
                    && let Some((entry, is_dir)) = listing.entries.get(i)
                {
                    let next = path_field::pick(&b.draft.text, &listing.dir, entry, *is_dir, false);
                    if let Some(open) = d.references.browse.as_mut() {
                        open.draft = TextDraft::new(next, false);
                        open.error = None;
                    }
                    edited = true;
                }
            }
            PathHit::Up => {
                if let Some(open) = d.references.browse.as_mut() {
                    open.draft = TextDraft::new(path_field::up(&b.draft.text, false), false);
                    open.error = None;
                }
                edited = true;
            }
            PathHit::Submit => {
                out.write(Act::ui(path_action(&b)));
            }
        }
    }
    // A field the handler opened takes the keyboard.
    if let Some(f) = d.references.claim.take() {
        if usable(d, f) {
            at = Some(f);
            select_all = focus(&mut text, d, f);
        }
        edited = true;
    }
    // The kit's focus is the record.
    if let Some(f) = at
        && (!usable(d, f) || (!text.focused(field_of(f)) && !text.suspended(field_of(f))))
    {
        at = None;
    }
    for id in [FORM, PATH, DISTANCE] {
        if at.map(field_of) != Some(id) {
            text.blur(id);
        }
    }
    // The path field shows a pick or "..": its kit draft follows.
    if at == Some(Focus::Path)
        && let Some(b) = d.references.browse.as_ref()
        && text.draft(PATH).is_some_and(|t| t.text != b.draft.text)
    {
        text.set(PATH, TextDraft::new(b.draft.text.clone(), false));
    }
    if at.is_none() {
        select_all = false;
    }
    if at != before || select_all != before_all {
        d.references.focus = at;
        d.references.select_all = select_all;
        edited = true;
    }
    // The directory listing follows the path (read on Pool::Io; landed by `reads::receive`).
    if let Some(b) = d.references.browse.as_mut()
        && let Some((key, dir)) = path_field::listing_key(b.draft.text.trim(), suffixes(b.kind))
        && b.listing_asked.as_deref() != Some(key.as_str())
    {
        b.listing_asked = Some(key.clone());
        let exts: Vec<String> = suffixes(b.kind).iter().map(|s| s.to_string()).collect();
        path_field::request(&mut d.references.listing, "cad references listing", key, dir, exts);
        edited = true;
    }
    if edited {
        // The dock redraws on the document's revision.
        d.touch();
        doc.set_changed();
    }
}

/// CadPlugin: the three fields and [`input`] (before the CAD keys, so the
/// frame's keys see their typing).
pub(super) fn build(app: &mut App) {
    app.add_text_field(FORM, TextField::new("Reference placement").select_on_focus())
        .add_text_field(PATH, TextField::new("Reference or system file path").placeholder("~/…"))
        .add_text_field(DISTANCE, TextField::new("Real distance").select_on_focus().sticky())
        .add_systems(Update, input.before(crate::cad::CadKeySet::Gate).in_set(crate::cad::CadKeySet::Focus).run_if(in_state(ViewerMode::Cad)));
}
