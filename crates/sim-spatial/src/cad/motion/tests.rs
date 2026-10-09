//! Written fixtures; no capture/encoder is executed by this source review.
use super::*;
use crate::{
    builder::ui_api::Enabled,
    cad::panel::CadButton,
    ui_kit::{Kit, UiFonts},
};
use bevy::ecs::system::RunSystemOnce;
fn render(mut commands: Commands, doc: Res<CadDocument>, st: Res<MotionState>) {
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
fn rendered_save_is_disabled_without_reference_identity_and_cancel_is_drawn() {
    let d = CadDocument::new(crate::cad::document::CadTarget::Service(
        "http://127.0.0.1:8420".into(),
    ));
    let mut w = World::new();
    w.insert_resource(d);
    w.insert_resource(MotionState {
        open: true,
        ..default()
    });
    w.run_system_once(render).unwrap();
    let mut q = w.query_filtered::<(&CadButton, &Enabled), With<Button>>();
    let controls = q
        .iter(&w)
        .filter_map(|(b, e)| {
            if let CadAction::CadMotion(a) = &b.0 {
                Some((a.op, e.0))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert!(controls.contains(&(MotionOp::Save, false)));
    assert!(controls.contains(&(MotionOp::Cancel, true)));
    assert!(controls.contains(&(MotionOp::Return, true)));
    assert!(controls.contains(&(MotionOp::ExportSize, true)));
}
#[test]
fn stale_or_replaced_reference_refuses_persistence() {
    let mut d = CadDocument::new(crate::cad::document::CadTarget::Service(
        "http://127.0.0.1:8420".into(),
    ));
    let stamp = Stamp::of(&d, 7);
    d.generation += 1;
    assert!(guard(&d, &stamp).is_err());
    d.generation = stamp.generation;
    d.stale = Some("refetching".into());
    assert!(guard(&d, &stamp).is_err());
}
#[test]
fn cancellation_keeps_rejected_program_json() {
    let mut s = MotionState {
        editor: "{ rejected input }".into(),
        drafts: vec!["old program".into()],
        active: true,
        playing: true,
        ..default()
    };
    s.request_cancel();
    assert_eq!(s.editor, "{ rejected input }");
    assert_eq!(s.drafts, vec!["old program"]);
    assert!(!s.active);
    assert!(!s.playing);
}

#[test]
fn actual_cancel_is_drawn_for_closed_missing_stale_and_replaced_documents() {
    for case in 0..4 {
        let mut d = CadDocument::new(crate::cad::document::CadTarget::Service(
            "http://127.0.0.1:8420".into(),
        ));
        let mut s = MotionState {
            open: case != 3,
            ..default()
        };
        if case == 1 {
            d.stale = Some("external edit".into());
        }
        let original = Stamp::of(&d, 0);
        if case == 2 {
            d.generation += 1;
        }
        s.loading = Some((
            original,
            Job::spawn(Pool::Dedicated, 0, "fixture pending motion read", |_| {
                Err("fixture".into())
            }),
        ));
        let mut w = World::new();
        w.insert_resource(d);
        w.insert_resource(s);
        w.run_system_once(render).unwrap();
        let mut q = w.query_filtered::<(&CadButton, &Enabled), With<Button>>();
        assert_eq!(
            q.iter(&w)
                .filter(|(b, e)| e.0
                    && matches!(
                        &b.0,
                        CadAction::CadMotion(MotionArgs {
                            op: MotionOp::Cancel,
                            ..
                        })
                    ))
                .count(),
            1,
            "case {case}"
        );
    }
}

#[test]
fn rendered_dynamic_buttons_match_shared_catalogue_actions_and_readiness() {
    let d = CadDocument::new(crate::cad::document::CadTarget::Service(
        "http://127.0.0.1:8420".into(),
    ));
    let mut s = MotionState {
        open: true,
        active: true,
        identity: Some(Stamp::of(&d, 9)),
        sequence: 9,
        joint: Some("joint".into()),
        ..default()
    };
    s.metadata = Some(PoseMetadata {
        identity: crate::cad::types::experiments::CaptureIdentity::default(),
        assumptions: vec![],
        focus_ids: BTreeMap::new(),
        joints: vec![crate::cad::types::motion::PoseJoint {
            id: "joint".into(),
            name: "Reference hinge".into(),
            unit: "rad".into(),
            home: 0.,
            lower: None,
            upper: None,
            display_lower: -1.,
            display_upper: 1.,
            driver: true,
            pivot: [0.; 3],
            axis: [0., 0., 1.],
            child: "body".into(),
            parent: None,
        }],
    });
    let program = json!({"name":"sweep","duration":2.,"loop":true,"tracks":[{"joint":"joint","unit":"rad","keys":[[0.,0.],[2.,0.]]}]});
    s.editor = program.to_string();
    s.programs.insert("sweep".into(), program);
    let catalogue = controls_of(&d, &s);
    let mut w = World::new();
    w.insert_resource(d);
    w.insert_resource(s);
    w.run_system_once(render).unwrap();
    let mut q = w.query_filtered::<(&CadButton, &Enabled), With<Button>>();
    let drawn = q
        .iter(&w)
        .map(|(b, e)| (b.0.clone(), e.0))
        .collect::<Vec<_>>();
    assert_eq!(
        drawn.len(),
        catalogue.len(),
        "every button is drawn once from the shared catalogue"
    );
    for (_, _, action, ready) in &catalogue {
        assert!(drawn.contains(&(action.clone(), ready.is_ok())));
    }
    for op in [
        MotionOp::Joint,
        MotionOp::Position,
        MotionOp::Program,
        MotionOp::Seek,
        MotionOp::ExportFps,
        MotionOp::ExportSize,
    ] {
        assert!(
            drawn
                .iter()
                .any(|(action, _)| matches!(action,CadAction::CadMotion(args)if args.op==op)),
            "dynamic {op:?} is both discoverable and drawn"
        );
    }
}

fn continuation_fixture() -> (CadDocument, MotionState, PoseSample) {
    let mut doc = CadDocument::new(crate::cad::document::CadTarget::Service(
        "http://127.0.0.1:8420".into(),
    ));
    doc.doc_key = Some((Some("model".into()), 7));
    let sample:PoseSample=serde_json::from_value(json!({"identity":{"document_id":"model","revision":7,"source_kind":"live_kinematic","source_id":"model","physical_hash":null,"archive_hash":null},"positions":{"driver":1.2,"passive":-0.4},"matrices":{},"closure_error_mm":0.001,"time":0.5,"program":null})).unwrap();
    let st = MotionState {
        identity: Some(Stamp::of(&doc, 3)),
        sample: Some(sample.clone()),
        sequence: 3,
        sample_sequence: 4,
        cursor: 0.5,
        active: true,
        ..default()
    };
    (doc, st, sample)
}
fn continuation_intent(doc: &CadDocument, st: &MotionState) -> sampling::Intent {
    sampling::Intent {
        stamp: Stamp::of(doc, st.sequence),
        sequence: st.sequence,
        serial: st.sample_sequence,
        request: PoseRequest {
            document_id: "model".into(),
            expected_revision: 7,
            positions: BTreeMap::new(),
            program: None,
            time: st.cursor,
            prior: None,
        },
    }
}
#[test]
fn preview_refuses_old_order_same_time_and_cancellation_receipts() {
    let (doc, mut st, sample) = continuation_fixture();
    let old = continuation_intent(&doc, &st);
    assert!(sampling::current(&doc, &st, &old));
    st.sample_sequence += 1;
    assert!(
        !sampling::current(&doc, &st, &old),
        "same time and draft cannot publish an older position intent"
    );
    st.sample_sequence = old.serial;
    st.cancel_requested = true;
    assert!(!sampling::current(&doc, &st, &old));
    assert_eq!(st.sample.as_ref().unwrap().positions, sample.positions);
}
#[test]
fn arbitrary_seek_and_program_switch_keep_only_published_branch() {
    let (doc, mut st, sample) = continuation_fixture();
    let published = sampling::prior(st.sample.as_ref(), st.identity.as_ref().unwrap()).unwrap();
    st.sequence += 1;
    st.cursor = 3.;
    let mut intent = continuation_intent(&doc, &st);
    intent.request.program = Some(json!({"name":"new program","duration":3.}));
    assert_eq!(
        sampling::prior(st.sample.as_ref(), &intent.stamp).unwrap(),
        published
    );
    let mut reply = sample.clone();
    reply.time = 2.;
    assert!(!sampling::matches(
        &reply,
        &intent.stamp,
        intent.request.time
    ));
    assert_eq!(st.sample.as_ref().unwrap(), &sample);
    let mut replaced = doc;
    replaced.generation += 1;
    assert!(!sampling::current(&replaced, &st, &intent));
    let mut revision = intent.stamp.clone();
    revision.revision += 1;
    assert!(
        sampling::prior(st.sample.as_ref(), &revision).is_none(),
        "source revision resets continuation"
    );
}
#[test]
fn single_flight_retains_latest_seek_without_seeding_from_unpublished_work() {
    let (doc, mut st, receipt) = continuation_fixture();
    let initial = continuation_intent(&doc, &st);
    st.sampling = Some(sampling::Pending {
        intent: initial,
        job: Job::spawn(Pool::Dedicated, 0, "continuation fixture", |_| {
            Err("unpublished fixture".into())
        }),
    });
    st.cursor = 1.;
    sample(&doc, &mut st, Some(json!({"name":"first"}))).unwrap();
    st.cursor = 2.;
    sample(&doc, &mut st, Some(json!({"name":"last"}))).unwrap();
    assert!(st.sampling.is_some());
    let queued = st.queued_sample.as_ref().unwrap();
    assert_eq!(queued.request.time, 2.);
    assert_eq!(queued.request.program.as_ref().unwrap()["name"], "last");
    assert!(
        queued.request.prior.is_none(),
        "seed is attached only when actually dispatched"
    );
    assert_eq!(
        sampling::prior(st.sample.as_ref(), &queued.stamp)
            .unwrap()
            .positions,
        receipt.positions
    );
    st.request_cancel();
    assert!(st.queued_sample.is_none());
    assert_eq!(st.sample.as_ref().unwrap(), &receipt);
}
#[test]
fn export_continuation_advances_without_mutating_saved_preview() {
    let (doc, st, sample) = continuation_fixture();
    let mut export = sampling::prior(st.sample.as_ref(), st.identity.as_ref().unwrap()).unwrap();
    assert_eq!(export.positions["passive"], -0.4);
    let saved = sample.clone();
    let mut export_reply = sample.clone();
    export_reply.time = 1.;
    export_reply.positions.insert("passive".into(), 0.9);
    assert!(sampling::matches(
        &export_reply,
        st.identity.as_ref().unwrap(),
        1.
    ));
    export = export_reply.continuation();
    assert_eq!(export.positions["passive"], 0.9);
    assert_eq!(saved.positions["passive"], -0.4);
    assert_eq!(
        st.sample.as_ref().unwrap().continuation().positions["passive"],
        -0.4
    );
    let late = continuation_intent(&doc, &st);
    assert!(
        !sampling::matches(&export_reply, &late.stamp, late.request.time),
        "export receipt cannot satisfy the preview cursor"
    );
}

#[test]
fn export_start_refuses_unpublished_seek_without_starting_encoder_or_losing_intent() {
    let (doc, mut st, receipt) = continuation_fixture();
    st.cursor = 2.;
    let intent = continuation_intent(&doc, &st);
    st.queued_sample = Some(intent);
    assert!(export::start(&doc, &mut st).is_err());
    assert!(st.export.is_none());
    assert_eq!(st.queued_sample.as_ref().unwrap().request.time, 2.);
    assert_eq!(st.sample.as_ref().unwrap(), &receipt);
    let control = controls_of(&doc, &st)
        .into_iter()
        .find(|(_, _, action, _)| {
            matches!(
                action,
                CadAction::CadMotion(MotionArgs {
                    op: MotionOp::Export,
                    ..
                })
            )
        })
        .unwrap();
    assert!(control.3.is_err());
    st.queued_sample = None;
    assert!(
        export::preview_ready(&doc, &st).is_err(),
        "cursor still differs from last published pose"
    );
    st.cursor = receipt.time;
    assert!(export::preview_ready(&doc, &st).is_ok());
}
