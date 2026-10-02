//! Written T43 integration regressions; never executed during this batch.
use super::*;
use bevy::ecs::system::RunSystemOnce;
use std::collections::BTreeMap;
fn document() -> CadDocument {
    let mut d = CadDocument::new(super::super::CadTarget::Service(
        "http://127.0.0.1:1".into(),
    ));
    d.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:1").unwrap());
    d.connection = Connection::Connected;
    d.doc = Some(sim_runtime::cad_client::DocState {
        revision: 7,
        document_id: Some("doc".into()),
        ..default()
    });
    d.doc_key = Some((Some("doc".into()), 7));
    d.health = Some(sim_runtime::cad_client::Health {
        revision: 7,
        ..default()
    });
    d
}
fn render(mut commands: Commands, doc: Res<CadDocument>) {
    let fonts = UiFonts {
        regular: default(),
        italic: default(),
        mono: default(),
        icons: BTreeMap::new(),
        medium: default(),
        semibold: default(),
    };
    commands
        .spawn((Node::default(), DespawnOnExit(crate::app::ModeScope::Cad)))
        .with_children(|p| top(p, &Kit::new(&fonts), &doc, &[], None));
}
#[test]
fn drawn_unknown_outcome_acknowledgment_requires_fresh_source_and_keeps_refresh() {
    for case in 0..4 {
        let mut d = document();
        d.uncertain_edit = Some("RoboCAD may still apply it; refresh before retrying".into());
        match case {
            1 => d.dirty_known_at = Some(std::time::Instant::now()),
            2 => d.stale = Some("source changed".into()),
            3 => {
                d.connection = Connection::Lost {
                    error: "disconnected".into(),
                    since: std::time::Instant::now(),
                }
            }
            _ => {}
        }
        let mut world = World::new();
        world.insert_resource(d);
        world.run_system_once(render).unwrap();
        let mut query = world.query_filtered::<(&CadButton, &Enabled), With<Button>>();
        let widgets = query
            .iter(&world)
            .map(|(b, e)| (b.0.clone(), e.0))
            .collect::<Vec<_>>();
        assert!(widgets.contains(&(CadAction::CadRefresh, true)));
        assert!(widgets.contains(&(
            CadAction::CadReconcileEdit {
                acknowledge: true,
                revision: Some(7)
            },
            case == 0
        )));
        assert!(
            world.resource::<CadDocument>().uncertain_edit.is_some(),
            "drawing cannot acknowledge or retry an outcome"
        );
    }
}
#[test]
fn common_source_guard_blocks_all_source_callers_but_auxiliary_keeps_revision_guards() {
    let mut d = document();
    d.preview_read_only = true;
    assert!(d.edit_refusal().unwrap().contains("read-only"));
    assert!(d.commit_refusal(Some(7)).is_some());
    assert!(d.commit_refusal_for(Some(7), true).is_none());
    d.health.as_mut().unwrap().revision = 8;
    assert!(
        d.commit_refusal_for(Some(7), true)
            .unwrap()
            .contains("revision 7, now 8")
    );
    d.health.as_mut().unwrap().revision = 7;
    d.uncertain_edit = Some("answer lost".into());
    assert!(
        d.commit_refusal_for(Some(7), true)
            .unwrap()
            .contains("Unknown source edit outcome")
    );
    assert!(
        d.switch_blockers()
            .iter()
            .any(|r| r.contains("unknown source edit outcome"))
    );
}

// T49 source-only fixture: actual top renderer, captured entity, source refusal.
#[test]
fn rendered_refresh_keeps_public_intent_and_refuses_replaced_document() {
    use crate::ui_kit::activation::{Activated, Ordinary};
    let mut world = World::new();
    world.insert_resource(document());
    world.run_system_once(render).unwrap();
    let mut buttons = world.query::<(Entity, &CadButton)>();
    let entity = buttons.iter(&world).find(|(_, button)| button.0 == CadAction::CadRefresh).unwrap().0;
    assert!(world.get::<Ordinary>(entity).is_some());
    assert!(world.get::<bevy::ui_widgets::Button>(entity).is_some());
    assert!(world.get::<bevy::ui_widgets::ActivateOnPress>(entity).is_some());
    world.run_system_once(crate::cad::activation::stamp).unwrap();
    world.entity_mut(entity).insert(Activated);
    world.run_system_once(crate::cad::activation::refuse).unwrap();
    assert!(world.get::<Activated>(entity).is_some());
    let intent = crate::cad::activation::guard(world.resource::<CadDocument>(), world.get::<CadButton>(entity).unwrap().0.clone());
    world.resource_mut::<CadDocument>().generation += 1;
    world.run_system_once(crate::cad::activation::refuse).unwrap();
    assert!(world.get::<Activated>(entity).is_none());
    let CadAction::Captured { source, action } = intent else { panic!("rendered UI intent must retain source") };
    assert!(!crate::cad::activation::current(&source, world.resource::<CadDocument>()));
    assert_eq!(*action, CadAction::CadRefresh);
    assert_eq!(world.get::<CadButton>(entity).unwrap().0, CadAction::CadRefresh);
}
