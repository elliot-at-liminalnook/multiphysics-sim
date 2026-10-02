//! T49 isolated actual-kit renderer fixtures. Written/source-reviewed, never run.
use super::*;
use super::activation::*;
use bevy::input::keyboard::{KeyboardInput, Key};
use bevy::input::ButtonState;
use bevy::input_focus::{InputFocus, FocusedInput, FocusCause};
use bevy::window::PrimaryWindow;
use crate::app::actions::Act;

#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum Intent { Submit, Cancel }
fn fonts() -> UiFonts {
    UiFonts { regular: default(), italic: default(), mono: default(), medium: default(), semibold: default(), icons: default() }
}
fn render(mut commands: Commands, fonts: Res<UiFonts>) {
    let k = Kit::new(&fonts);
    commands.spawn(k.backdrop("Actual kit form", true)).with_children(|parent| {
        parent.spawn(k.button("Submit", Intent::Submit, Look::Primary, true));
        parent.spawn(k.button("Cancel", Intent::Cancel, Look::Secondary, true));
    });
}
fn convert(controls: Query<&Intent, With<Activated>>, mut out: MessageWriter<Act<Intent>>) {
    for intent in &controls { out.write(Act::ui(*intent)); }
}
fn fixture() -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    app.insert_resource(fonts()).add_plugins(text::TextEntryPlugin)
        .add_message::<Act<Intent>>()
        .add_systems(Startup, render)
        .add_systems(Update, convert.in_set(crate::app::InputSet::Window));
    crate::app::configure_sets(&mut app);
    activation::install(&mut app);
    let window = app.world_mut().spawn((Window::default(), PrimaryWindow)).id();
    app.update(); app.update();
    let mut buttons = app.world_mut().query::<(Entity, &Intent)>();
    let submit = buttons.iter(app.world()).find_map(|(e, i)| (*i == Intent::Submit).then_some(e)).unwrap();
    let cancel = buttons.iter(app.world()).find_map(|(e, i)| (*i == Intent::Cancel).then_some(e)).unwrap();
    (app, window, submit, cancel)
}
fn key(app: &mut App, window: Entity, target: Entity, code: KeyCode, repeat: bool) {
    let logical_key = match code { KeyCode::Enter => Key::Enter, KeyCode::Tab => Key::Tab, _ => Key::Space };
    app.world_mut().trigger(FocusedInput {
        focused_entity: target, window,
        input: KeyboardInput { key_code: code, logical_key, state: ButtonState::Pressed, text: None, repeat, window },
    });
}
fn press(app: &mut App, window: Entity, entity: Entity, button: bevy::picking::pointer::PointerButton) {
    let camera = app.world_mut().spawn_empty().id();
    app.world_mut().trigger(bevy::picking::events::Pointer::new(
        bevy::picking::pointer::PointerId::Mouse,
        bevy::picking::pointer::Location {
            target: bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window)).normalize(Some(window)).unwrap(), position: Vec2::ZERO,
        },
        bevy::picking::events::Press { button, hit: bevy::picking::backend::HitData::new(camera, 0., None, None), count: 1 }, entity,
    ));
}
fn drain(app: &mut App) -> Vec<Intent> { app.world_mut().resource_mut::<Messages<Act<Intent>>>().drain().map(|a| a.action).collect() }
#[test]
fn actual_button_activates_on_primary_press_once_without_release() {
    let (mut app, window, submit, _) = fixture();
    press(&mut app, window, submit, bevy::picking::pointer::PointerButton::Primary);
    app.update(); assert_eq!(drain(&mut app), [Intent::Submit]);
    app.update(); assert!(drain(&mut app).is_empty());
}
#[test]
fn actual_button_refuses_secondary_press() {
    let (mut app, window, submit, _) = fixture();
    press(&mut app, window, submit, bevy::picking::pointer::PointerButton::Secondary);
    app.update(); assert!(drain(&mut app).is_empty());
}
#[test]
fn actual_focused_buttons_enter_space_repeat_and_shortcuts() {
    for code in [KeyCode::Enter, KeyCode::Space] {
        let (mut app, window, submit, _) = fixture();
        app.world_mut().resource_mut::<InputFocus>().set(submit, FocusCause::Navigated);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(code);
        key(&mut app, window, submit, code, false);
        key(&mut app, window, submit, code, true);
        app.update(); assert_eq!(drain(&mut app), [Intent::Submit]);
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(code));
    }
}
#[test]
fn actual_hidden_disabled_and_despawned_controls_refuse_capture() {
    for hidden in [false, true] {
        let (mut app, _, submit, _) = fixture();
        if hidden { app.world_mut().get_mut::<Node>(submit).unwrap().display = Display::None; }
        else { app.world_mut().entity_mut(submit).insert(crate::builder::ui_api::Enabled(false)); }
        app.world_mut().trigger(bevy::ui_widgets::Activate { entity: submit });
        app.update(); assert!(drain(&mut app).is_empty());
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(submit).is_some());
        assert!(app.world().get::<bevy::input_focus::tab_navigation::TabIndex>(submit).is_none());
    }
    let (mut app, _, submit, _) = fixture();
    app.world_mut().trigger(bevy::ui_widgets::Activate { entity: submit });
    app.world_mut().despawn(submit);
    app.update(); assert!(drain(&mut app).is_empty());
}
#[test]
fn actual_modal_tab_and_shift_tab_contain_navigation() {
    let (mut app, window, submit, cancel) = fixture();
    app.world_mut().resource_mut::<InputFocus>().set(submit, FocusCause::Navigated);
    key(&mut app, window, submit, KeyCode::Tab, false);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(cancel));
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ShiftLeft);
    key(&mut app, window, cancel, KeyCode::Tab, false);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(submit));
    app.update(); assert!(drain(&mut app).is_empty());
}

#[test]
fn actual_editor_rebuild_rebinds_without_resetting_draft_or_selection() {
    use text::{TextFieldApp, TextField, FieldId, TextDraft};
    const FIELD: FieldId = FieldId("fixture.editor");
    let (mut app, _, return_target, _) = fixture();
    app.add_text_field(FIELD, TextField::new("Retained editor"));
    let roots: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<ModalFocus>>().iter(app.world()).collect();
    for root in roots { app.world_mut().entity_mut(root).remove::<ModalFocus>(); }
    app.update();
    app.world_mut().resource_mut::<InputFocus>().set(return_target, FocusCause::Navigated);
    let font = fonts(); let k = Kit::new(&font);
    let root = app.world_mut().spawn((Node::default(), ModalFocus)).id();
    let old = app.world_mut().spawn((k.input("draft", "value", Intent::Submit, true), ChildOf(root))).id();
    app.update();
    let id = app.world_mut().query::<(Entity, &FieldId)>().iter(app.world()).find_map(|(e, f)| (*f == FIELD).then_some(e)).unwrap();
    let identity = app.world().get::<InputIdentity>(old).unwrap().0.clone();
    {
        let mut field = app.world_mut().get_mut::<TextField>(id).unwrap();
        field.draft = TextDraft::new("unsubmitted + malformed", false);
        field.focus_anchor = Some(old); field.navigation_identity = Some(identity);
    }
    app.world_mut().resource_mut::<InputFocus>().set(id, FocusCause::Navigated);
    let before = app.world().get::<TextField>(id).unwrap().draft.clone();
    app.world_mut().despawn(root);
    let rebuilt_root = app.world_mut().spawn((Node::default(), ModalFocus)).id();
    let new = app.world_mut().spawn((k.input("unsubmitted + malformed", "value", Intent::Submit, true), ChildOf(rebuilt_root))).id();
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(id));
    assert_eq!(app.world().get::<TextField>(id).unwrap().focus_anchor, Some(new));
    assert_eq!(app.world().get::<TextField>(id).unwrap().draft, before);
    // An absent replacement retains the draft but deliberately releases focus.
    app.world_mut().despawn(rebuilt_root); app.update();
    assert_eq!(app.world().get::<TextField>(id).unwrap().draft, before);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(return_target));
}

#[test]
fn actual_editor_submit_is_single_and_retains_unapplied_text() {
    use text::{TextFieldApp, TextField, FieldId, TextDraft, FieldEvent, FieldMsg};
    const FIELD: FieldId = FieldId("fixture.submit");
    let (mut app, window, _, _) = fixture();
    app.add_text_field(FIELD, TextField::new("Submission"));
    let root = app.world_mut().query_filtered::<Entity, With<ModalFocus>>().iter(app.world()).next().unwrap();
    let fonts = fonts(); let k = Kit::new(&fonts);
    let input = app.world_mut().spawn((k.input("unapplied", "value", Intent::Submit, true), ChildOf(root))).id();
    let id = app.world_mut().query::<(Entity, &FieldId)>().iter(app.world()).find_map(|(e, f)| (*f == FIELD).then_some(e)).unwrap();
    let identity = app.world().get::<InputIdentity>(input).unwrap().0.clone();
    {
        let mut field = app.world_mut().get_mut::<TextField>(id).unwrap();
        field.draft = TextDraft::new("unapplied", false);
        field.focus_anchor = Some(input); field.navigation_identity = Some(identity);
    }
    app.world_mut().resource_mut::<InputFocus>().set(id, FocusCause::Navigated);
    for repeat in [false, true] {
        app.world_mut().write_message(KeyboardInput { key_code: KeyCode::Enter, logical_key: Key::Enter, state: ButtonState::Pressed, text: None, repeat, window });
    }
    // No field apply owner acknowledges this submission in this fixture.
    // The one editor must preserve its draft until an owner accepts it.
    app.update();
    let submissions: Vec<_> = app.world_mut().resource_mut::<Messages<FieldMsg>>().drain().filter(|m| matches!(m.event, FieldEvent::Submit(_))).collect();
    assert_eq!(submissions.len(), 1);
    assert_eq!(app.world().get::<TextField>(id).unwrap().draft.text, "unapplied");
}
