//! Source-only repair fixtures: actual retained panel refresh and row renderer.
use super::*;
use bevy::ecs::system::RunSystemOnce;
use crate::app::actions::Act;
use crate::ui_kit::activation::Activated;
use sim_runtime::cad_client::{DocState, Health, NodeSummary};

fn fixture() -> World {
    let mut doc = CadDocument::new(super::super::document::CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..default() });
    doc.doc_key = Some((None, 4));
    doc.doc = Some(DocState {
        roots: vec!["b1".into()],
        nodes: vec![NodeSummary { id: "b1".into(), kind: "body".into(), name: "Bracket".into(), visible: true, effective_visible: true, ..default() }],
        revision: 4, ..default()
    });
    let mut world = World::new();
    world.insert_resource(doc);
    let shared = crate::cad::selection::Fixture::at(4);
    world.insert_resource(shared.registry);
    world.insert_resource(shared.selection);
    world.insert_resource(State::new(crate::app::ViewerMode::Cad));
    world.insert_resource(UiFonts { regular: default(), italic: default(), mono: default(), icons: default(), medium: default(), semibold: default() });
    world.init_resource::<NameDraft>();
    world.init_resource::<crate::cad::components::ComponentsState>();
    world.init_resource::<crate::cad::composition::CadCompositionState>();
    world.init_resource::<crate::cad::experiments::ExperimentsState>();
    world.init_resource::<crate::cad::experiment_review::ReviewState>();
    world.init_resource::<crate::cad::motion::MotionState>();
    world.insert_resource(Messages::<Act<CadAction>>::default());
    world.spawn((Node::default(), CadRoot, CadList::new(Part::Tree), DespawnOnExit(crate::app::ModeScope::Cad)));
    world
}

#[test]
fn unrelated_source_revision_rebuilds_retained_rows_and_refuses_old_occurrences() {
    let mut world = fixture();
    let mut present = Schedule::default();
    present.add_systems(refresh);
    present.run(&mut world);
    world.run_system_once(crate::cad::activation::stamp).unwrap();
    world.run_system_once(crate::ui_kit::activation::stamp_sources).unwrap();
    let old = world.query::<(Entity, &crate::cad::tree::rows::TreeRowId)>().iter(&world).find(|(_, r)| r.id() == "b1").unwrap().0;
    let content = crate::cad::tree::rows::key(world.resource::<CadDocument>());
    let captured = crate::cad::activation::guard(world.resource::<CadDocument>(), crate::cad::tree::select_action(world.resource::<CadDocument>(), "b1", false, false));
    // Unrelated transform revision: the row's displayed values stay unchanged.
    {
        let mut doc = world.resource_mut::<CadDocument>();
        doc.doc_key = Some((None, 5));
        doc.doc.as_mut().unwrap().revision = 5;
        doc.health.as_mut().unwrap().revision = 5;
        doc.revision += 1;
    }
    let snapshot = world.resource::<CadDocument>().doc.as_ref().unwrap().clone();
    // The real snapshot/selection owner advances the shared registry too.
    world.resource_scope::<crate::document::DocumentRegistry, _>(|world, mut registry| {
        let mut selection = world.resource_mut::<crate::selection::Selection>();
        crate::cad::selection::follow_tree(&mut crate::cad::selection::Shared { registry: &mut registry, selection: &mut selection }, 5, &snapshot, true);
    });
    assert_eq!(content, crate::cad::tree::rows::key(world.resource::<CadDocument>()));
    let CadAction::Captured { source, .. } = captured else { panic!("captured source expected") };
    assert!(!crate::cad::activation::current(&source, world.resource::<CadDocument>()));
    assert_ne!(world.get::<crate::ui_kit::activation::RenderSource>(old).unwrap().document, world.resource::<crate::document::DocumentRegistry>().current(crate::app::ViewerMode::Cad));
    world.entity_mut(old).insert(Activated);
    world.run_system_once(crate::ui_kit::activation::validate_sources).unwrap();
    assert!(world.get::<Activated>(old).is_none());
    world.entity_mut(old).insert(Activated);
    world.run_system_once(crate::cad::activation::refuse).unwrap();
    assert!(world.get::<Activated>(old).is_none());
    // Execute the very same retained refresh schedule: its Local<Drawn> and
    // CadList key persist. No test-only replacement or old-stamp update occurs.
    present.run(&mut world);
    assert!(world.get_entity(old).is_err());
    world.run_system_once(crate::cad::activation::stamp).unwrap();
    world.run_system_once(crate::ui_kit::activation::stamp_sources).unwrap();
    let current = world.query::<(Entity, &crate::cad::tree::rows::TreeRowId)>().iter(&world).find(|(_, r)| r.id() == "b1").unwrap().0;
    assert_ne!(old, current);
    assert_eq!(world.get::<crate::ui_kit::activation::RenderSource>(current).unwrap().document, world.resource::<crate::document::DocumentRegistry>().current(crate::app::ViewerMode::Cad));
    world.entity_mut(current).insert(Activated);
    world.run_system_once(crate::ui_kit::activation::validate_sources).unwrap();
    assert!(world.get::<Activated>(current).is_some());
    world.run_system_once(crate::cad::activation::refuse).unwrap();
    assert!(world.get::<Activated>(current).is_some());
    world.run_system_once(crate::cad::activation::tree_keyboard).unwrap();
    let actions = world.resource_mut::<Messages<Act<CadAction>>>().drain().collect::<Vec<_>>();
    assert_eq!(actions.len(), 1);
    let CadAction::Captured { source, action } = &actions[0].action else { panic!("captured selection expected") };
    assert!(crate::cad::activation::current(source, world.resource::<CadDocument>()));
    assert_eq!(**action, crate::cad::tree::select_action(world.resource::<CadDocument>(), "b1", false, false));
}
