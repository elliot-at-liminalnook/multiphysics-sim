//! One text entry (native-viewer.md "One text entry"): the kit's one text
//! field, the one keyboard focus and the one input system.
//!
//! - **Focus.** Bevy's `InputFocus` (bevy_input_focus) is the only record of
//!   which field has the keyboard. A field is an entity carrying
//!   [`TextField`] and its [`FieldId`]; it has no UI node (so mode scopes
//!   never despawn it), and the panel that shows it draws it with
//!   `Kit::input`/`input_selectable` (which tag the node [`KitInput`]).
//!   [`typing`] is the one run condition every mode's key map, pick and
//!   camera gesture checks; [`Typing`] is the same check as a parameter.
//! - **The field.** [`TextField`]: the working draft ([`TextDraft`]), an
//!   optional character filter, a placeholder, an accessible label,
//!   select-all on focus, what Enter and Tab do ([`EnterKey`], [`TabKey`]),
//!   and whether a press elsewhere takes the keyboard away (`sticky`).
//! - **The input system** ([`input::keys`], PreUpdate after Bevy's input
//!   and UI focus systems): it alone reads `KeyboardInput` for text. It
//!   types into the focused field, consumes the keys it used (so no mode key
//!   sees them), releases held keys when a field gains the keyboard
//!   ([`release_held`], the hardware-safety rule), blurs a field on a press
//!   elsewhere or a mode switch, and writes [`FieldMsg`]s.
//! - **Owners** read [`FieldMsg`] as data (`Changed`, `Submit`, `Cancel`,
//!   `Tab`, `Blur`) and act through [`TextFocus`] (`focus`, `set`, `blur`).
//!   The kit has no intent logic: what a submitted text means is the owner's.
//!
//! Bevy's `EditableText` is not used (the decision and its API facts are in
//! native-viewer.md "One text entry"): `EditableTextInputPlugin`'s observer
//! acts only on a focused entity with `EditableText`, which no kit field
//! has, so it never types into one.
pub(crate) mod draft;
pub(crate) mod input;
#[cfg(test)]
mod tests;

pub(crate) use draft::{DraftKey, TextDraft};
pub(crate) use input::release_held;

use bevy::ecs::system::SystemParam;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;

/// A field's identity: owners name their field by it and filter
/// [`FieldMsg`] by it (`"cad.name"`, `"builder.draft"`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FieldId(pub &'static str);

/// What Enter does in a field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EnterKey {
    /// Enter submits (a one-line field).
    #[default]
    Submit,
    /// Enter submits, Shift+Enter types a newline (a note or comment).
    ShiftNewline,
    /// Enter types a newline, Command/Control+Enter submits (a block).
    CommandSubmits,
}

/// What Tab does in a field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TabKey {
    /// Written as [`FieldEvent::Tab`] (Shift+Tab: `back`), for owners that
    /// move between rows; owners that have none ignore it.
    #[default]
    Emit,
    /// Types two spaces (a lesson block).
    Indent,
}

/// The kit's text field: the working draft and how it is edited.
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct TextField {
    /// The draft the input system edits while the field has the keyboard.
    pub draft: TextDraft,
    /// Characters a typed text may contain (`None`: any).
    pub filter: Option<fn(char) -> bool>,
    /// Shown while the draft is empty and the field unfocused.
    pub placeholder: String,
    /// The field's accessible label.
    pub label: String,
    /// [`TextFocus::focus`] selects the text (RoboCAD's `selectAll`).
    pub select_on_focus: bool,
    pub enter: EnterKey,
    pub tab: TabKey,
    /// A press elsewhere does not take the keyboard away (an open builder
    /// or lesson draft, the CAD name field: they end on their own terms).
    pub sticky: bool,
}
impl TextField {
    /// A one-line field labelled `label`.
    pub(crate) fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), ..default() }
    }
    pub(crate) fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }
    pub(crate) fn filter(mut self, filter: fn(char) -> bool) -> Self {
        self.filter = Some(filter);
        self
    }
    pub(crate) fn select_on_focus(mut self) -> Self {
        self.select_on_focus = true;
        self
    }
    pub(crate) fn enter(mut self, enter: EnterKey) -> Self {
        self.enter = enter;
        self
    }
    pub(crate) fn tab(mut self, tab: TabKey) -> Self {
        self.tab = tab;
        self
    }
    pub(crate) fn sticky(mut self) -> Self {
        self.sticky = true;
        self
    }
}

/// Marks a kit input node (`Kit::input`): a press on one is not a press
/// "elsewhere" (its owner decides whether it focuses a field).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct KitInput;

/// What happened to a field.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FieldEvent {
    /// The draft changed (text or selection); written before a `Submit`
    /// of the same frame.
    Changed(TextDraft),
    /// Enter (as [`EnterKey`] reads it), with the text. The field keeps the
    /// keyboard: the owner blurs it when the text is accepted.
    Submit(String),
    /// Escape. The kit has already taken the keyboard away.
    Cancel,
    /// Tab ([`TabKey::Emit`]); Shift+Tab is `back`.
    Tab { back: bool },
    /// The field lost the keyboard other than by Escape or its owner's
    /// [`TextFocus::blur`]: a press elsewhere, a mode switch, another
    /// field's focus, or Bevy focusing a non-field entity.
    Blur,
}

/// One field's event, read by its owner.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FieldMsg {
    pub field: FieldId,
    pub event: FieldEvent,
}
impl Message for FieldMsg {}

/// The one typing check: a kit text field has the keyboard.
#[derive(SystemParam)]
pub(crate) struct Typing<'w, 's> {
    focus: Option<Res<'w, InputFocus>>,
    fields: Query<'w, 's, &'static FieldId, With<TextField>>,
}
impl Typing<'_, '_> {
    /// A text field has the keyboard.
    pub(crate) fn get(&self) -> bool {
        self.field().is_some()
    }
    /// The field that has the keyboard.
    pub(crate) fn field(&self) -> Option<FieldId> {
        self.focus.as_ref().and_then(|f| f.get()).and_then(|e| self.fields.get(e).ok()).copied()
    }
}

/// The shared run condition: a kit text field has the keyboard. Mode key
/// maps, picks and camera gestures run under `not(typing)`.
pub(crate) fn typing(typing: Typing) -> bool {
    typing.get()
}

/// Owners' access to their fields: focus, set and blur by [`FieldId`].
/// One per system (it holds `InputFocus` mutably).
#[derive(SystemParam)]
pub(crate) struct TextFocus<'w, 's> {
    focus: Option<ResMut<'w, InputFocus>>,
    fields: Query<'w, 's, (Entity, &'static FieldId, &'static mut TextField)>,
    out: MessageWriter<'w, FieldMsg>,
}
impl TextFocus<'_, '_> {
    fn entity(&self, id: FieldId) -> Option<Entity> {
        self.fields.iter().find(|(_, f, _)| **f == id).map(|(e, _, _)| e)
    }
    fn focused_entity(&self) -> Option<Entity> {
        self.focus.as_ref().and_then(|f| f.get()).filter(|e| self.fields.contains(*e))
    }

    /// `id` has the keyboard.
    pub(crate) fn focused(&self, id: FieldId) -> bool {
        self.focused_entity().is_some_and(|e| self.fields.get(e).is_ok_and(|(_, f, _)| *f == id))
    }
    /// Any field has the keyboard (the same answer as [`typing`]).
    pub(crate) fn typing(&self) -> bool {
        self.focused_entity().is_some()
    }
    /// `id`'s working draft.
    pub(crate) fn draft(&self, id: FieldId) -> Option<&TextDraft> {
        self.fields.iter().find(|(_, f, _)| **f == id).map(|(_, _, t)| &t.draft)
    }

    /// Give `id` the keyboard with `text` as its draft (selected when the
    /// field selects on focus). The field that had it gets
    /// [`FieldEvent::Blur`]. False when `id` was never added.
    pub(crate) fn focus(&mut self, id: FieldId, text: impl Into<String>) -> bool {
        let Some(entity) = self.entity(id) else { return false };
        let select = self.fields.get(entity).is_ok_and(|(_, _, t)| t.select_on_focus);
        self.focus_draft(id, TextDraft::new(text, select))
    }

    /// [`TextFocus::focus`] with an explicit draft (its selection as given).
    pub(crate) fn focus_draft(&mut self, id: FieldId, draft: TextDraft) -> bool {
        let Some(entity) = self.entity(id) else { return false };
        if let Some(previous) = self.focused_entity().filter(|e| *e != entity)
            && let Ok((_, &field, _)) = self.fields.get(previous)
        {
            self.out.write(FieldMsg { field, event: FieldEvent::Blur });
        }
        if let Ok((_, _, mut field)) = self.fields.get_mut(entity) {
            field.draft = draft;
        }
        let Some(focus) = self.focus.as_mut() else { return false };
        if focus.get() != Some(entity) {
            focus.set(entity, FocusCause::Navigated);
        }
        true
    }

    /// Replace `id`'s working draft (a listing pick, a reset) without
    /// moving the keyboard.
    pub(crate) fn set(&mut self, id: FieldId, draft: TextDraft) {
        if let Some((_, _, mut field)) = self.fields.iter_mut().find(|(_, f, _)| **f == id)
            && field.draft != draft
        {
            field.draft = draft;
        }
    }

    /// Take the keyboard from `id` (no message: the owner knows).
    pub(crate) fn blur(&mut self, id: FieldId) {
        if self.focused(id)
            && let Some(focus) = self.focus.as_mut()
        {
            focus.clear();
        }
    }
}

/// Adding a field at app build: one entity per [`FieldId`].
pub(crate) trait TextFieldApp {
    /// Spawn field `id` (once; a second call keeps the first).
    fn add_text_field(&mut self, id: FieldId, field: TextField) -> &mut Self;
}
impl TextFieldApp for App {
    fn add_text_field(&mut self, id: FieldId, field: TextField) -> &mut Self {
        let world = self.world_mut();
        let exists = world.query::<&FieldId>().iter(world).any(|f| *f == id);
        if !exists {
            world.spawn((id, field));
        }
        self
    }
}

/// The text entry's resources, messages and input system (windowless:
/// tests add it alone; `UiKitPlugin` adds it for the window).
pub struct TextEntryPlugin;
impl Plugin for TextEntryPlugin {
    fn build(&self, app: &mut App) {
        use bevy::input::keyboard::{Key, KeyboardFocusLost, KeyboardInput};
        app.add_message::<FieldMsg>()
            .add_message::<KeyboardInput>()
            .add_message::<KeyboardFocusLost>()
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<Key>>()
            .add_systems(PreUpdate, input::keys.after(bevy::input::InputSystems).after(bevy::ui::UiSystems::Focus));
    }
}
