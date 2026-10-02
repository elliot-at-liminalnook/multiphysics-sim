//! T49 rendered-control conversion fixtures. Written and inspected, unexecuted.
use super::*;
use crate::ui_kit::activation::{Activated, Ordinary};
use crate::ui_kit::text::TextFieldApp;

fn fixture() -> App {
    let mut picker = Picker::default();
    picker.open = Some(ViewerMode::Robot);
    picker.from = Some(ViewerMode::Inspect);
    picker.revision = 8;
    picker.listed_at = 8;
    picker.found = Some(Sources { sections: vec![Section {
        title: "Robots".into(), empty: String::new(), choices: vec![Choice {
            label: "Original robot".into(), detail: "fixture".into(), enabled: true,
            document: Document::Preset("original".into()),
        }],
    }], start_dir: None });
    let mut app = App::new();
    app.insert_resource(picker)
        .insert_resource(UiFonts { regular: default(), medium: default(), semibold: default(),
            italic: default(), mono: default(), icons: default() })
        .init_resource::<InputFocus>()
        .add_message::<crate::ui_kit::text::FieldMsg>()
        .add_message::<Act<WindowAction>>()
        .add_systems(Startup, draw)
        .add_systems(Update, clicks.in_set(crate::app::InputSet::Window));
    app.add_plugins(crate::ui_kit::text::TextEntryPlugin)
        .add_text_field(PATH, path_text_field());
    crate::app::configure_sets(&mut app);
    crate::ui_kit::activation::install(&mut app);
    app.add_systems(PreUpdate, (
        modal::sync.before(crate::ui_kit::activation::ActivationSet::Eligibility),
        modal::keys.after(crate::ui_kit::activation::ActivationSet::Consume),
        bevy::input_focus::dispatch_focused_input::<bevy::input::keyboard::KeyboardInput>
            .in_set(bevy::input_focus::InputFocusSystems::Dispatch).after(bevy::input::InputSystems),
    ));
    app.world_mut().spawn((Window::default(), bevy::window::PrimaryWindow));
    app.update(); app.update();
    app
}
fn entry(app: &mut App) -> Entity {
    let world=app.world_mut();
    world.query::<(Entity,&PickerPart)>().iter(world)
        .find_map(|(entity,part)|matches!(part.0,PickHit::Entry(0,0)).then_some(entity)).unwrap()
}
#[test]
fn actual_picker_entry_keeps_press_contract_and_existing_switch_identity() {
    let mut app=fixture();let entity=entry(&mut app);
    assert!(app.world().get::<Ordinary>(entity).is_some());
    assert!(app.world().get::<bevy::ui_widgets::ActivateOnPress>(entity).is_some());
    assert!(app.world().get::<Button>(entity).is_some(),"system_ui discovery remains compatible");
    app.world_mut().entity_mut(entity).insert(Activated);
    app.update();
    let actions:Vec<_>=app.world_mut().resource_mut::<Messages<Act<WindowAction>>>().drain().collect();
    assert_eq!(actions.len(),1);
    let WindowAction::Switch(request)=&actions[0].action else {panic!("wrong action")};
    assert_eq!(request.mode,ViewerMode::Robot);
    assert_eq!(request.document,Some(Document::Preset("original".into())));
}
#[test]
fn actual_old_picker_row_refuses_replacement_before_index_resolution() {
    let mut app=fixture();let old=entry(&mut app);
    {
        let mut picker=app.world_mut().resource_mut::<Picker>();
        picker.found.as_mut().unwrap().sections[0].choices[0].document=Document::Preset("replacement".into());
        picker.revision+=1;picker.listed_at=picker.revision;
    }
    app.world_mut().entity_mut(old).insert(Activated);
    app.update();
    assert_eq!(app.world().resource::<Messages<Act<WindowAction>>>().len(),0);
    assert_eq!(app.world().resource::<Picker>().controls()[0]["id"],"picker:robot:0");
}
#[test]
fn actual_picker_disabled_projection_refuses_captured_activation() {
    let mut app=fixture();let entity=entry(&mut app);
    app.world_mut().entity_mut(entity).insert((Enabled(false),Activated));
    app.update();
    assert_eq!(app.world().resource::<Messages<Act<WindowAction>>>().len(),0);
}

/// Real pointer observers run before the real text/modal PreUpdate path. A
/// focus-only Blur must not change the rendered positional source lifetime.
#[test]
fn first_pointer_from_durable_path_dispatches_once() {
    let mut app = fixture();
    let window = app.world_mut().query_filtered::<Entity, With<bevy::window::PrimaryWindow>>().iter(app.world()).next().unwrap();
    // Activate the actual rendered PATH field first: its production converter
    // calls TextFocus::focus_draft and captures the live navigation anchor.
    let anchor = app.world_mut().query::<(Entity, &PickerPart)>().iter(app.world())
        .find_map(|(e, part)| matches!(part.0, PickHit::Path(PathHit::Field)).then_some(e)).unwrap();
    pointer_press(&mut app, window, anchor);
    app.update(); app.update();
    let field = app.world_mut().query::<(Entity, &crate::ui_kit::text::FieldId)>().iter(app.world())
        .find_map(|(e, id)| (*id == PATH).then_some(e)).unwrap();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    assert_eq!(app.world().get::<crate::ui_kit::text::TextField>(field).unwrap().focus_anchor, Some(anchor));
    let selected = entry(&mut app);
    pointer_press(&mut app, window, selected);
    app.update();
    let actions: Vec<_> = app.world_mut().resource_mut::<Messages<Act<WindowAction>>>().drain().collect();
    assert_eq!(actions.len(), 1);
    assert!(matches!(&actions[0].action, WindowAction::Switch(request)
        if request.document == Some(Document::Preset("original".into()))));
    app.update();
    assert!(app.world().resource::<Messages<Act<WindowAction>>>().is_empty());
}

fn pointer_press(app: &mut App, window: Entity, entity: Entity) {
    let camera = app.world_mut().spawn_empty().id();
    app.world_mut().trigger(bevy::picking::events::Pointer::new(
        bevy::picking::pointer::PointerId::Mouse,
        bevy::picking::pointer::Location {
            target: bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window)).normalize(Some(window)).unwrap(), position: Vec2::ZERO,
        },
        bevy::picking::events::Press {
            button: bevy::picking::pointer::PointerButton::Primary,
            hit: bevy::picking::backend::HitData::new(camera, 0., None, None), count: 1,
        }, entity,
    ));
}

#[test]
fn higher_modal_escape_and_wheel_leave_suspended_picker_draft_and_scroll_intact() {
    use crate::ui_kit::activation::{ModalFocus,ModalPriority};
    use crate::ui_kit::text::{FieldId,TextField};
    use bevy::input::keyboard::Key;
    use bevy::input::mouse::{MouseWheel,MouseScrollUnit};
    let mut app=fixture();
    let window=app.world_mut().query_filtered::<Entity,With<bevy::window::PrimaryWindow>>().iter(app.world()).next().unwrap();
    let anchor=app.world_mut().query::<(Entity,&PickerPart)>().iter(app.world())
        .find_map(|(e,part)|matches!(part.0,PickHit::Path(PathHit::Field)).then_some(e)).unwrap();
    pointer_press(&mut app,window,anchor);app.update();app.update();
    let field=app.world_mut().query::<(Entity,&FieldId)>().iter(app.world())
        .find_map(|(e,id)|(*id==PATH).then_some(e)).unwrap();
    let original=app.world().resource::<Picker>().draft.clone();
    let higher=app.world_mut().spawn((Node::default(),ModalFocus,ModalPriority(100),bevy::ui::prelude::AccessibleLabel("Pending close".into()))).id();
    app.world_mut().spawn((Node::default(),Button,Enabled(true),Ordinary,ChildOf(higher),bevy::ui::prelude::AccessibleLabel("Cancel close".into())));
    app.update();
    assert!(app.world().get::<TextField>(field).unwrap().suspended);
    let mut logical=ButtonInput::<Key>::default();logical.press(Key::Escape);
    app.insert_resource(logical).add_message::<MouseWheel>();
    app.world_mut().write_message(MouseWheel{unit:MouseScrollUnit::Line,x:0.,y:-3.,window,phase:bevy::input::touch::TouchPhase::Moved});
    app.update();
    assert!(app.world().resource::<Picker>().open.is_some());
    assert_eq!(app.world().resource::<Picker>().draft,original);
    assert_eq!(app.world().resource::<Picker>().scroll,0.);
    assert!(app.world().get::<TextField>(field).unwrap().suspended);
    assert!(app.world().resource::<ButtonInput<Key>>().pressed(Key::Escape),"lower picker must not consume higher-modal keys");
}
