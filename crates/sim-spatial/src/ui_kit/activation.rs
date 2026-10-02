//! Ordinary activation occurrences. Held controls must opt out explicitly.
use bevy::prelude::*;
use bevy::input_focus::{InputFocus, InputFocusSystems, InputFocusVisible, FocusCause};
use bevy::input_focus::tab_navigation::{TabGroup, TabIndex, TabNavigationPlugin, TabNavigation, NavAction};
use bevy::ui::InteractionDisabled;
use crate::builder::ui_api::Enabled;

/// Transient widget identity; never a model index or durable document owner.
#[derive(Component, Default)]
#[require(bevy::ui_widgets::Button, bevy::ui_widgets::ActivateOnPress, Outline(Outline::new(Val::Px(2.), Val::Px(2.), Color::NONE)))]
pub(crate) struct Ordinary;
/// Explicit paired/continuous gesture opt-out. No generic keyboard activation.
#[derive(Component)]
pub(crate) struct HeldControl;
/// Compound pointer rows retain their gesture owner; only keyboard selection
/// goes through ordinary activation. Never used for hardware motion holds.
#[derive(Component)]
pub(crate) struct KeyboardOnly;
/// One captured occurrence on the original entity, removed after input conversion.
#[derive(Component)]
pub(crate) struct Activated;
/// A transient modal group. Return focus is window state, never authored data.
#[derive(Component)]
#[require(TabGroup(TabGroup::modal()))]
pub(crate) struct ModalFocus;
/// Higher priorities contain focus above lower modals (pending close uses 100).
/// Nested groups then sort by ancestry depth; entity bits break sibling ties.
#[derive(Component, Default, Clone, Copy)]
pub(crate) struct ModalPriority(pub i32);
/// Stable intent key on a rendered editor, excluding its working text.
#[derive(Component, Clone)]
pub(crate) struct InputIdentity(pub String);
#[derive(Resource, Default)]
struct PointerGate(std::collections::HashSet<Entity>);
#[derive(Resource, Default)]
struct ModalStack(Vec<(Entity, Option<Entity>, String)>);
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct RenderSource {
    pub(crate) mode: crate::app::ViewerMode,
    pub(crate) document: Option<(crate::document::DocumentId, u64)>,
}


#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ActivationSet { Eligibility, Containment, Consume, Validate, Clear }

pub(crate) fn install(app: &mut App) {
    if !app.is_plugin_added::<bevy::ui_widgets::ButtonPlugin>() {
        app.add_plugins(bevy::ui_widgets::ButtonPlugin);
    }
    app.add_plugins(TabNavigationPlugin)
        .init_resource::<ModalStack>()
        .init_resource::<PointerGate>()
        .init_resource::<InputFocusVisible>()
        .configure_sets(PreUpdate, ActivationSet::Eligibility.after(bevy::input::InputSystems).before(InputFocusSystems::Dispatch).before(bevy::picking::PickingSystems::Hover))
        // Eligibility Commands publish TabIndex/InteractionDisabled before
        // containment reads them; containment records suspension before Hover
        // emits pointer observers that can replace the departing InputFocus.
        .configure_sets(PreUpdate, ActivationSet::Containment.after(ActivationSet::Eligibility).before(bevy::picking::PickingSystems::Hover).before(super::text::TextInputSet).before(InputFocusSystems::Dispatch))
        .configure_sets(Update, ActivationSet::Validate.after(crate::app::InputSet::Rest).before(crate::app::InputSet::Window).in_set(crate::app::ViewerSet::Input))
        .configure_sets(PreUpdate, ActivationSet::Consume.after(InputFocusSystems::Dispatch).after(super::text::TextInputSet))
        .add_systems(PreUpdate, eligibility.in_set(ActivationSet::Eligibility))
        .add_systems(PreUpdate, consume_keys.in_set(ActivationSet::Consume))
        .add_systems(PreUpdate, modal_focus.in_set(ActivationSet::Containment))
        .add_systems(Update, focus_inputs.after(crate::app::InputSet::Rest).before(ActivationSet::Validate).in_set(crate::app::ViewerSet::Input))
        .add_observer(pointer_intent)
        .add_observer(key_intent)
        .add_observer(capture)
        .add_systems(Update, clear.in_set(ActivationSet::Clear).after(crate::app::ViewerSet::Input).before(crate::app::ViewerSet::Actions))
        .add_systems(PostUpdate, stamp_sources.before(bevy::ui::UiSystems::Prepare))
        .add_systems(Update, validate_sources.in_set(ActivationSet::Validate))
        .add_systems(PostUpdate, focus_outline.after(super::widgets::repaint_buttons).before(bevy::ui::UiSystems::Prepare));
}

fn eligible(entity: Entity, nodes: &Query<(Option<&Node>, Option<&Visibility>, Option<&Enabled>, Option<&ChildOf>)>) -> bool {
    let mut next = Some(entity);
    while let Some(e) = next {
        let Ok((node, visibility, enabled, parent)) = nodes.get(e) else { return false };
        if node.is_some_and(|n| n.display == Display::None) || visibility.is_some_and(|v| *v == Visibility::Hidden) || enabled.is_some_and(|e| !e.0) { return false; }
        next = parent.map(|p| p.parent());
    }
    true
}

/// Pinned navigation only filters TabIndex, not visibility or disabled state.
/// Remove ineligible indexes before focused dispatch, including ancestor hiding.
fn eligibility(
    mut commands: Commands,
    controls: Query<(Entity, Has<HeldControl>, Has<KeyboardOnly>, Has<InteractionDisabled>, Option<&TabIndex>), With<Ordinary>>,
    nodes: Query<(Option<&Node>, Option<&Visibility>, Option<&Enabled>, Option<&ChildOf>)>,
    roots: Query<Entity, (With<Node>, Without<ChildOf>, Without<TabGroup>)>,
    mut focus: ResMut<InputFocus>,
    mut gate: ResMut<PointerGate>,
) {
    gate.0.retain(|e| nodes.get(*e).is_ok());
    for root in &roots { commands.entity(root).insert(TabGroup::new(0)); }
    for (entity, held, keyboard_only, disabled, index) in &controls {
        if held && !keyboard_only {
            if focus.get() == Some(entity) { focus.clear(); }
            commands.entity(entity).remove::<(Ordinary, bevy::ui_widgets::Button, bevy::ui_widgets::ActivateOnPress, TabIndex, Activated)>();
            continue;
        }
        let enabled = eligible(entity, &nodes);
        if enabled {
            if disabled { commands.entity(entity).remove::<InteractionDisabled>(); }
            if index.is_none() { commands.entity(entity).insert(TabIndex(0)); }
        } else {
            if !disabled { commands.entity(entity).insert(InteractionDisabled); }
            if index.is_some() { commands.entity(entity).remove::<TabIndex>(); }
            commands.entity(entity).remove::<Activated>();
            if focus.get() == Some(entity) { focus.clear(); }
        }
    }
    if let Some(entity) = focus.get() && nodes.get(entity).is_err() {
        // Durable text fields are valid focus targets even without a Node.
        focus.clear();
    }
}

fn capture(event: On<bevy::ui_widgets::Activate>, gate: Res<PointerGate>, controls: Query<(), (With<Ordinary>, Or<(Without<HeldControl>, With<KeyboardOnly>)>, Without<InteractionDisabled>)>, nodes: Query<(Option<&Node>, Option<&Visibility>, Option<&Enabled>, Option<&ChildOf>)>, mut commands: Commands) {
    if !gate.0.contains(&event.entity) && controls.contains(event.entity) && eligible(event.entity, &nodes) {
        commands.entity(event.entity).insert(Activated);
    }
}

/// ButtonInput is independent of bubbling propagation. Consume only activation
/// presses, preserving just_released and all independent STOP keys.
fn consume_keys(focus: Res<InputFocus>, controls: Query<(), (With<Ordinary>, Or<(Without<HeldControl>, With<KeyboardOnly>)>)>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    keys.clear_just_pressed(KeyCode::Tab);
    if focus.get().is_some_and(|e| controls.contains(e)) {
        for key in [KeyCode::Enter, KeyCode::Space, KeyCode::Tab] { keys.clear_just_pressed(key); }
    }
}
fn clear(mut commands: Commands, controls: Query<Entity, With<Activated>>) {
    for entity in &controls { commands.entity(entity).remove::<Activated>(); }
}
fn focus_outline(focus: Res<InputFocus>, fields: Query<&super::text::TextField>, visible: Res<InputFocusVisible>, mut controls: Query<(Entity, &mut Outline), With<Ordinary>>) {
    let anchor = focus.get().and_then(|e| fields.get(e).ok().and_then(|f| f.focus_anchor).or(Some(e)));
    for (entity, mut outline) in &mut controls {
        let color = if visible.0 && anchor == Some(entity) { super::ACCENT } else { Color::NONE };
        if outline.color != color { outline.color = color; }
    }
}

fn focus_inputs(mut commands: Commands, focus: Res<InputFocus>, inputs: Query<Entity, (With<super::text::KitInput>, With<Ordinary>, Without<InteractionDisabled>)>, mut last: Local<Option<Entity>>) {
    let current = focus.get().filter(|e| inputs.contains(*e));
    if current != *last && let Some(entity) = current { commands.entity(entity).insert(Activated); }
    *last = current;
}

fn modal_focus(
    modals: Query<(Entity, Option<&bevy::ui::prelude::AccessibleLabel>, Option<&ModalPriority>), With<ModalFocus>>,
    parents: Query<&ChildOf>,
    mut fields: Query<&mut super::text::TextField>,
    inputs: Query<(Entity, &InputIdentity, Option<&RenderSource>), With<super::text::KitInput>>,
    nodes: Query<(Option<&Node>, Option<&Visibility>, Option<&Enabled>, Option<&ChildOf>)>,
    registry: Option<Res<crate::document::DocumentRegistry>>,
    mut out: MessageWriter<super::text::FieldMsg>,
    field_ids: Query<&super::text::FieldId>,
    nav: TabNavigation,
    mut focus: ResMut<InputFocus>,
    mut stack: ResMut<ModalStack>,
    entities: Query<Entity>,
    field_entities: Query<Entity, With<super::text::TextField>>,
) {
    // Rebind by captured typed intent, never by a position in a rebuilt list.
    // The draft and caret stay on the durable field; no owner focus action runs.
    for entity in &field_entities {
        let Ok(mut field) = fields.get_mut(entity) else { continue };
        if !field.focus_anchor.is_some_and(|e| !entities.contains(e)) { continue; }
        // A source revision may rebuild an unchanged editor. Transfer only its
        // navigation anchor, never an activation occurrence, and only to a
        // unique current control with the same durable document/intent.
        let matches: Vec<(Entity, Option<RenderSource>)> = inputs.iter().filter(|(entity, identity, source)| {
            let same_document = match (field.navigation_source.as_ref(), *source) {
                (None, None) => true,
                (Some(old), Some(new)) => old.mode == new.mode && old.document.as_ref().map(|(id, _)| id) == new.document.as_ref().map(|(id, _)| id),
                _ => false,
            };
            let current = source.is_none_or(|s| registry.as_ref().is_none_or(|r| r.current(s.mode) == s.document));
            field.navigation_identity.as_deref() == Some(identity.0.as_str()) && same_document && current && eligible(*entity, &nodes)
        }).map(|(entity, _, source)| (entity, source.cloned())).collect();
        if matches.len() == 1 {
            field.focus_anchor = Some(matches[0].0);
            field.navigation_source = matches[0].1.clone();
        }
        else {
            field.focus_anchor = None;
            if focus.get() == Some(entity) {
                if let Ok(&id) = field_ids.get(entity) { out.write(super::text::FieldMsg { field: id, event: super::text::FieldEvent::Blur }); }
                focus.clear();
            }
        }
    }
    if let Some(entity) = focus.get() && let Ok(field) = fields.get(entity) {
        let stale = field.focus_anchor.is_some_and(|anchor| !eligible(anchor, &nodes))
            || field.navigation_source.as_ref().is_some_and(|source| registry.as_ref().is_some_and(|r| r.current(source.mode) != source.document));
        if stale {
            if let Ok(&id) = field_ids.get(entity) { out.write(super::text::FieldMsg { field: id, event: super::text::FieldEvent::Blur }); }
            focus.clear();
        }
    }
    // A suspended owner is valid only while its original intent still names a
    // unique current eligible control. Replacement invalidation tells the real
    // owner Blur even though the higher modal still holds the keyboard.
    for entity in &field_entities {
        let Ok(mut field) = fields.get_mut(entity) else { continue };
        if !field.suspended { continue; }
        let valid = field.focus_anchor.is_some_and(|anchor| eligible(anchor, &nodes)
            && inputs.get(anchor).is_ok_and(|(_, identity, source)|
                field.navigation_identity.as_deref() == Some(identity.0.as_str())
                && source == field.navigation_source.as_ref()
                && source.is_none_or(|s| registry.as_ref().is_none_or(|r| r.current(s.mode) == s.document))));
        if !valid {
            field.suspended = false;
            field.focus_anchor = None;
            if let Ok(&id) = field_ids.get(entity) { out.write(super::text::FieldMsg { field: id, event: super::text::FieldEvent::Blur }); }
        }
    }
    // Order all live scopes independently of ECS query iteration. Highest
    // priority wins; nested scopes precede their parent on equal priority.
    let mut ordered: Vec<_> = modals.iter().filter(|(e, _, _)| eligible(*e, &nodes)).map(|(e, label, priority)| {
        let key = (priority.map_or(0, |p| p.0), parents.iter_ancestors(e).count(), e.to_bits());
        (e, label.map_or_else(String::new, |l| l.0.clone()), key)
    }).collect();
    ordered.sort_by_key(|(_, _, key)| *key);
    // A presentation rebuild retains the logical modal's original return
    // target. Never replace it with its own durable editor.
    let mut rebuilt = Vec::new();
    while stack.0.last().is_some_and(|(entity, _, _)| !ordered.iter().any(|(e, _, _)| e == entity)) {
        let (_, previous, label) = stack.0.pop().unwrap();
        let replacement = ordered.iter().any(|(_, next, _)| *next == label);
        if replacement { rebuilt.push((label, previous)); continue; }
        let valid_previous = previous.filter(|e| entities.contains(*e) && fields.get(*e).map_or_else(|_| eligible(*e, &nodes), |f| f.focus_anchor.is_some_and(|a| entities.contains(a) && eligible(a, &nodes))));
        if let Some(previous) = valid_previous { focus.set(previous, FocusCause::Navigated); }
        else { focus.clear(); }
    }
    // Disappeared lower scopes must not remain as future return targets.
    stack.0.retain(|(entity, previous, label)| {
        if ordered.iter().any(|(e, _, _)| e == entity) { return true; }
        // A lower scope can rebuild while a higher scope keeps containment.
        // Its logical return target must not become the higher scope's button.
        if ordered.iter().any(|(_, next, _)| next == label) {
            rebuilt.push((label.clone(), *previous));
        }
        false
    });
    for (entity, label, _) in &ordered {
        if !stack.0.iter().any(|(e, _, _)| e == entity) {
            let previous = if let Some(index) = rebuilt.iter().position(|(old, _)| old == label) { rebuilt.remove(index).1 } else { focus.get() };
            stack.0.push((*entity, previous, label.clone()));
        }
    }
    stack.0.sort_by_key(|(entity, _, _)| ordered.iter().find(|(e, _, _)| e == entity).unwrap().2);
    if let Some((modal, _, _)) = stack.0.last() {
        let anchor = focus.get().and_then(|e| fields.get(e).ok().and_then(|f| f.focus_anchor).or(Some(e)));
        let inside = anchor.is_some_and(|e| e == *modal || parents.iter_ancestors(e).any(|p| p == *modal));
        if !inside && let Ok(first) = nav.initialize(*modal, NavAction::First) {
            if let Some(entity) = focus.get() && let Ok(mut field) = fields.get_mut(entity)
                && field.focus_anchor.is_some_and(|anchor| eligible(anchor, &nodes) && inputs.contains(anchor))
            { field.suspended = true; }
            focus.set(first, FocusCause::Navigated);
        }
    }
    if let Some(entity) = focus.get() && let Ok(mut field) = fields.get_mut(entity) {
        field.suspended = false;
    }
}

/// Stamp once after feature Present renderers. Persistent shell controls are
/// window intent and intentionally have no physical/document source.
pub(crate) fn stamp_sources(mut commands: Commands, mode: Option<Res<State<crate::app::ViewerMode>>>, registry: Option<Res<crate::document::DocumentRegistry>>, controls: Query<Entity, (With<Ordinary>, Without<RenderSource>)>, ancestry: Query<(Option<&ChildOf>, Has<crate::app::Persistent>)>) {
    let (Some(mode), Some(registry)) = (mode, registry) else { return };
    for entity in &controls {
        let mut next = Some(entity);
        let mut persistent = false;
        while let Some(at) = next {
            let Ok((parent, shell)) = ancestry.get(at) else { break };
            persistent |= shell;
            next = parent.map(|p| p.parent());
        }
        if !persistent { commands.entity(entity).insert(RenderSource { mode: *mode.get(), document: registry.current(*mode.get()) }); }
    }
}
pub(crate) fn validate_sources(mut commands: Commands, registry: Option<Res<crate::document::DocumentRegistry>>, mode: Option<Res<State<crate::app::ViewerMode>>>, controls: Query<(Entity, &RenderSource), With<Activated>>) {
    let (Some(registry), Some(mode)) = (registry, mode) else { return };
    for (entity, source) in &controls {
        if source.mode != *mode.get() || source.document != registry.current(source.mode) { commands.entity(entity).remove::<Activated>(); }
    }
}

// All observers for the same pointer target run before the deferred Activate.
// Cause authorization is recorded synchronously, so observer registration order
// never authorizes a secondary pointer press as an ordinary action.
fn pointer_intent(event: On<bevy::picking::events::Pointer<bevy::picking::events::Press>>, controls: Query<(Has<KeyboardOnly>, Has<InteractionDisabled>), With<Ordinary>>, nodes: Query<(Option<&Node>, Option<&Visibility>, Option<&Enabled>, Option<&ChildOf>)>, mut gate: ResMut<PointerGate>, mut focus: ResMut<InputFocus>, mut visible: ResMut<InputFocusVisible>) {
    let Ok((keyboard_only, disabled)) = controls.get(event.entity) else { return };
    if !controls.contains(event.entity) { return; }
    if event.button == bevy::picking::pointer::PointerButton::Primary && !keyboard_only && !disabled && eligible(event.entity, &nodes) {
        gate.0.remove(&event.entity);
        focus.set(event.entity, FocusCause::Pressed);
        visible.0 = false;
    }
    else { gate.0.insert(event.entity); }
}
fn key_intent(event: On<bevy::input_focus::FocusedInput<bevy::input::keyboard::KeyboardInput>>, controls: Query<(), With<Ordinary>>, mut gate: ResMut<PointerGate>) {
    if controls.contains(event.focused_entity) && !event.input.repeat && event.input.state == bevy::input::ButtonState::Pressed && matches!(event.input.key_code, KeyCode::Enter | KeyCode::Space) { gate.0.remove(&event.focused_entity); }
}
