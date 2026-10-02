//! Written regression fixtures; not executed in T43.
use super::*;
use crate::{
    builder::ui_api::Enabled,
    cad::panel::CadButton,
    ui_kit::{Kit, UiFonts},
};
use bevy::ecs::system::RunSystemOnce;
use std::collections::BTreeMap;
fn render(mut commands: Commands, doc: Res<CadDocument>, st: Res<ReviewState>) {
    let fonts = UiFonts {
        regular: default(),
        italic: default(),
        mono: default(),
        icons: BTreeMap::new(),
        medium: default(),
        semibold: default(),
    };
    commands
        .spawn(Node::default())
        .with_children(|p| ui::draw(p, &Kit::new(&fonts), &doc, &st));
}
#[test]
fn drawn_review_cancel_survives_missing_stale_replaced_and_closed_document() {
    for case in 0..4 {
        let mut d = CadDocument::new(crate::cad::document::CadTarget::Service(
            "http://127.0.0.1:8420".into(),
        ));
        let mut s = ReviewState {
            open: case != 3,
            ..default()
        };
        if case == 1 {
            d.stale = Some("stale".into());
        }
        if case == 2 {
            d.generation += 1;
        }
        s.read = Some((
            Stamp::of(&d, 0),
            Job::spawn(Pool::Dedicated, 0, "fixture pending captured read", |_| {
                Err("fixture".into())
            }),
        ));
        let mut w = World::new();
        w.insert_resource(d);
        w.insert_resource(s);
        w.run_system_once(render).unwrap();
        let mut q = w.query_filtered::<(&CadButton, &Enabled), With<Button>>();
        let cancel = q
            .iter(&w)
            .filter(|(b, e)| {
                e.0 && matches!(
                    &b.0,
                    CadAction::CadExperimentReview(ReviewArgs {
                        op: ReviewOp::Cancel,
                        ..
                    })
                )
            })
            .count();
        assert_eq!(cancel, 1, "case {case}: actual durable cancel button");
        let play = q.iter(&w).find(|(b, _)| {
            matches!(
                &b.0,
                CadAction::CadExperimentReview(ReviewArgs {
                    op: ReviewOp::Play,
                    ..
                })
            )
        });
        if let Some((_, enabled)) = play {
            assert!(!enabled.0, "missing capture refuses play");
        }
    }
}
#[test]
fn cancellation_invalidates_read_and_sample_sequence_without_losing_note() {
    let mut s = ReviewState {
        active: true,
        sequence: 4,
        note: "rejected draft survives".into(),
        ..default()
    };
    s.request_cancel();
    assert_eq!(s.sequence, 5);
    assert!(s.cancelled);
    assert!(!s.active);
    assert_eq!(s.note, "rejected draft survives");
}
#[test]
fn row_major_reference_delta_is_not_transposed() {
    let t = scene::matrix([
        [1., 0., 0., 12.],
        [0., 1., 0., 23.],
        [0., 0., 1., 34.],
        [0., 0., 0., 1.],
    ]);
    assert_eq!(t.translation, Vec3::new(12., 23., 34.));
}

#[test]
fn completed_capture_replacement_is_guarded_after_read_job_is_gone() {
    let d = CadDocument::new(crate::cad::CadTarget::Service(
        "http://127.0.0.1:8420".into(),
    ));
    let mut state = ReviewState {
        active: true,
        origin: Some(Stamp::of(&d, 4)),
        note: "retained evidence draft".into(),
        ..default()
    };
    assert!(state.read.is_none());
    let mut replaced = d;
    replaced.generation += 1;
    let mut world = World::new();
    world.insert_resource(replaced);
    world.insert_resource(state);
    world.run_system_once(tick).unwrap();
    state = world.remove_resource::<ReviewState>().unwrap();
    assert!(!state.active && state.cancelled);
    assert_eq!(state.note, "retained evidence draft");
}

#[test]
fn retained_capture_cannot_publish_annotation_into_a_replaced_document() {
    let mut doc = CadDocument::new(crate::cad::CadTarget::Service(
        "http://127.0.0.1:8420".into(),
    ));
    let mut review = ReviewState {
        sequence: 4,
        origin: Some(Stamp::of(&doc, 4)),
        ..default()
    };
    doc.generation += 1;
    let mut fixture = crate::cad::selection::Fixture::at(0);
    let (mut continuation, mut replies) = (Value::Null, crate::app::actions::Replies::default());
    let mut call = Call {
        origin: crate::app::actions::Origin::Ui,
        continuation: &mut continuation,
        cancelled: false,
        replies: &mut replies,
    };
    let mut cx = Cx {
        doc: &mut doc,
        shared: fixture.shared(),
        meshes: None,
        topology: None,
        view: None,
        plane: &mut crate::cad::CadActivePlane::default(),
        sketches: None,
        display: None,
        views: None,
        files: None,
        components: &mut crate::cad::components::ComponentsState::default(),
        composition: &mut crate::cad::composition::CadCompositionState::default(),
        experiments: &mut crate::cad::experiments::ExperimentsState::default(),
        review: &mut review,
        motion: &mut crate::cad::motion::MotionState::default(),
        camera: vec![],
    };
    let stale = ReviewArgs {
        sequence: Some(4),
        ..ReviewArgs::of(ReviewOp::Annotate)
    };
    assert!(
        matches!(handle(&stale,&mut call,&mut cx),Outcome::Done(Err(e)) if e.contains("Reopen"))
    );
    assert!(cx.doc.edit.is_none());
    cx.review.sequence = 5;
    assert!(
        matches!(handle(&stale,&mut call,&mut cx),Outcome::Done(Err(e)) if e.contains("another captured request"))
    );
}
