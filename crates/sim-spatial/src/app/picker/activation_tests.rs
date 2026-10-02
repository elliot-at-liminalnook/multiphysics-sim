//! T49 rendered-control conversion fixtures. Written and inspected, unexecuted.
use super::*;
use crate::ui_kit::activation::{Activated, Ordinary};

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
        .add_systems(Update, clicks);
    app.update();
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
