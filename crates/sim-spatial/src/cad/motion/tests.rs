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
        identity: sim_runtime::cad_client::experiments::CaptureIdentity::default(),
        assumptions: vec![],
        focus_ids: BTreeMap::new(),
        joints: vec![sim_runtime::cad_client::motion::PoseJoint {
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
