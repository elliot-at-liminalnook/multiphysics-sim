//! Written windowless fixtures. Intentionally not executed in this epic.
//! Real descriptor/member JSON shapes exercise branch identity and draft
//! precedence rather than mirroring UI rendering.
use super::*;
use crate::cad::{CadTarget, Connection};
use sim_runtime::cad_client::{CadClient, ComponentRecipes, DocState, Health, NodeSummary};

fn fixture() -> (CadDocument, ComponentsState) {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:1".into()));
    doc.client = Some(CadClient::new("http://127.0.0.1:1").unwrap());
    doc.connection = Connection::Connected;
    doc.doc_key = Some((Some("doc-a".into()), 4));
    doc.health = Some(Health {
        document_id: Some("doc-a".into()),
        revision: 4,
        ..Default::default()
    });
    doc.doc = Some(DocState {
        nodes: vec![
            NodeSummary {
                id: "outer".into(),
                kind: "group".into(),
                component_instance: Some(
                    json!({"definition_id":"parent","placement":{"translation":[0,0,0],"axis":[0,0,1],"angle_deg":0,"scale":1},"node_map":{"source-child":"nested"},"nested_overrides":{"source-child":{"length":"27 mm"}}}),
                ),
                ..Default::default()
            },
            NodeSummary {
                id: "nested".into(),
                kind: "group".into(),
                component_member: Some(json!({"instance_id":"outer"})),
                component_instance: Some(
                    json!({"definition_id":"child","overrides":{"length":99},"placement":{"translation":[0,0,0],"axis":[0,0,1],"angle_deg":0,"scale":1}}),
                ),
                ..Default::default()
            },
            NodeSummary {
                id: "part".into(),
                kind: "body".into(),
                component_member: Some(json!({"instance_id":"nested"})),
                ..Default::default()
            },
            NodeSummary {
                id: "joint-a".into(),
                kind: "joint".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    let catalogue:ComponentCatalogue=serde_json::from_value(json!({"version":1,"features":{},"definitions":[{"id":"child","name":"Child","revision":2,"parameters":{"length":{"value":10,"unit":"mm","provenance":"estimated","uncertainty":{"sigma":0.2}}},"features":[],"ports":{"mount":{"kind":"joint","label":"Mount","source_id":"source-mount"}},"variants":{},"nested":{},"provenance":{"document_id":"origin","revision":9}}, {"id":"parent","name":"Parent","parameters":{},"features":[],"ports":{},"variants":{},"nested":{"source-child":{"definition_id":"child","parameter_bindings":{"length":"leg_length / 2"},"overrides":{}}}}]})).unwrap();
    let mut st = ComponentsState::default();
    st.catalogue = Some((jobs::Identity::of(&doc).unwrap(), catalogue));
    st.catalogue_revision = 4;
    st.recipes = Some(ComponentRecipes {
        units: vec!["mm".into(), "1".into()],
        ..Default::default()
    });
    st.selected = Some("child".into());
    (doc, st)
}

#[test]
fn member_override_hydrates_branch_and_reset_preserves_placement() {
    let (doc, mut st) = fixture();
    form::open(
        &mut st,
        &doc,
        ComponentsFormKind::Overrides,
        None,
        &["part".into()],
    )
    .unwrap();
    let draft = st.draft().unwrap();
    assert_eq!(draft.occurrence.as_deref(), Some("nested"));
    assert!(draft.nested_member);
    assert_eq!(draft.fields["override.length.value"], "27 mm");
    assert_eq!(draft.fields["override.length.enabled"], "true");
    form::set(&mut st, "override.length.enabled", "false").unwrap();
    let ComponentOperation::Overrides {
        instance_id,
        overrides,
        placement,
    } = form::operation(st.draft().unwrap(), &st, &doc).unwrap()
    else {
        panic!("expected override")
    };
    assert_eq!(instance_id, "nested");
    assert!(overrides.is_empty());
    assert!(placement.is_none());
    form::open(
        &mut st,
        &doc,
        ComponentsFormKind::Detach,
        None,
        &["part".into()],
    )
    .unwrap();
    assert_eq!(st.draft().unwrap().occurrence.as_deref(), Some("outer"));
}
#[test]
fn whole_parameter_edit_reprojects_rows_and_keeps_uncertainty() {
    let (doc, mut st) = fixture();
    form::open(&mut st, &doc, ComponentsFormKind::Defaults, None, &[]).unwrap();
    form::set(&mut st,"parameters",r#"{"length":{"value":"42 mm","unit":"mm","provenance":"measured","uncertainty":{"sigma":0.1}}}"#).unwrap();
    let ComponentOperation::Defaults { parameters, .. } =
        form::operation(st.draft().unwrap(), &st, &doc).unwrap()
    else {
        panic!("expected defaults")
    };
    assert_eq!(parameters["length"].value, json!("42 mm"));
    assert_eq!(
        parameters["length"].extra["uncertainty"],
        json!({"sigma":0.1})
    );
    form::set(&mut st, "parameters", "{unfinished").unwrap();
    assert!(
        form::operation(st.draft().unwrap(), &st, &doc)
            .unwrap_err()
            .contains("components.form.parameters")
    );
    st.current = None;
    assert!(st.drafts.last().unwrap().fields["parameters"].contains("unfinished"));
}
#[test]
fn typed_port_and_stale_submit_refuse_before_network() {
    let (mut doc, mut st) = fixture();
    form::open(&mut st, &doc, ComponentsFormKind::Place, None, &[]).unwrap();
    form::set(&mut st, "binding.mount", "part").unwrap();
    let op = form::operation(st.draft().unwrap(), &st, &doc).unwrap();
    assert!(
        validate::validate_operation(&op, &st, &doc)
            .unwrap_err()
            .contains("components.bindings.mount")
    );
    doc.health.as_mut().unwrap().revision = 5;
    assert!(
        submit(&mut st, &mut doc, None, None)
            .unwrap_err()
            .contains("document changed")
    );
    assert!(st.active.is_none());
    assert_eq!(st.draft().unwrap().fields["binding.mount"], "part");
}
#[test]
fn make_does_not_inherit_selected_definition_and_nested_mappings_keep_identity() {
    let (doc, mut st) = fixture();
    form::open(
        &mut st,
        &doc,
        ComponentsFormKind::Make,
        None,
        &["joint-a".into()],
    )
    .unwrap();
    assert!(st.draft().unwrap().definition.is_none());
    assert_eq!(st.draft().unwrap().fields["name"], "");
    st.selected = Some("parent".into());
    form::open(&mut st, &doc, ComponentsFormKind::Defaults, None, &[]).unwrap();
    form::set(
        &mut st,
        "nested.source-child.parameter_bindings",
        r#"{"length":"leg_length / 3"}"#,
    )
    .unwrap();
    let ComponentOperation::Defaults { nested, .. } =
        form::operation(st.draft().unwrap(), &st, &doc).unwrap()
    else {
        panic!("expected defaults")
    };
    assert_eq!(nested["source-child"].definition_id, "child");
    assert_eq!(
        nested["source-child"].parameter_bindings["length"],
        json!("leg_length / 3")
    );
}

#[test]
fn family_form_can_choose_an_explicit_default_variant() {
    let (doc, mut st) = fixture();
    form::open(&mut st, &doc, ComponentsFormKind::Family, None, &[]).unwrap();
    form::set(
        &mut st,
        "variants",
        r#"{"short":{"definition_id":"child","parameter_bindings":{"length":10}}}"#,
    )
    .unwrap();
    form::set(&mut st, "variant", "short").unwrap();
    let ComponentOperation::Family {
        default_variant,
        variants,
        ..
    } = form::operation(st.draft().unwrap(), &st, &doc).unwrap()
    else {
        panic!("family expected")
    };
    assert_eq!(default_variant.as_deref(), Some("short"));
    assert_eq!(variants["short"].definition_id, "child");
}

#[test]
fn link_family_explicit_id_is_occurrence_not_selected_family() {
    let (doc, mut st) = fixture();
    let family = serde_json::from_value(json!({
        "id":"family", "name":"Child family", "parameters":{},
        "features":[], "ports":{}, "nested":{},
        "variants":{"short":{"definition_id":"parent","parameter_bindings":{}}},
        "default_variant":"short"
    }))
    .unwrap();
    st.catalogue.as_mut().unwrap().1.definitions.push(family);
    st.selected = Some("family".into());
    // An explicit REST/system_ui occurrence wins over an unrelated source
    // selection, while the selected library definition remains independent.
    form::open(
        &mut st,
        &doc,
        ComponentsFormKind::LinkFamily,
        Some("outer"),
        &["joint-a".into()],
    )
    .unwrap();
    let explicit = form::operation(st.draft().unwrap(), &st, &doc).unwrap();
    let ComponentOperation::LinkFamily {
        instance_id,
        definition_id,
        variant,
        ..
    } = &explicit
    else {
        panic!("linked family expected")
    };
    assert_eq!(instance_id, "outer");
    assert_eq!(definition_id, "family");
    assert_eq!(variant, "short");
    validate::validate_operation(&explicit, &st, &doc).unwrap();
    // The window's no-id path resolves exactly the same typed operation.
    form::open(
        &mut st,
        &doc,
        ComponentsFormKind::LinkFamily,
        None,
        &["outer".into()],
    )
    .unwrap();
    let window = form::operation(st.draft().unwrap(), &st, &doc).unwrap();
    assert_eq!(
        serde_json::to_value(explicit).unwrap(),
        serde_json::to_value(window).unwrap()
    );
}

#[test]
fn queued_field_edit_targets_original_retained_draft() {
    let (doc, mut st) = fixture();
    form::open(&mut st, &doc, ComponentsFormKind::Defaults, None, &[]).unwrap();
    let original = st.current.unwrap();
    st.focus = Some("name".into());
    form::open(&mut st, &doc, ComponentsFormKind::Parametric, None, &[]).unwrap();
    let selected = st.current;
    assert!(st.focus.is_none(), "new form releases old keyboard target");
    form::set_at(&mut st, Some(original), "name", "Unsaved old name").unwrap();
    assert_eq!(st.current, selected);
    assert_eq!(st.drafts[original].fields["name"], "Unsaved old name");
    assert_eq!(st.draft().unwrap().fields["name"], "Parametric box");
}

#[test]
fn cancelled_submit_and_selected_import_leave_all_authored_state_untouched() {
    use crate::app::actions::{Call, Origin, Replies};
    use crate::cad::actions::Cx;
    for op in [ComponentsOp::Submit, ComponentsOp::ImportSelected] {
        let (mut doc, mut st) = fixture();
        st.files.push("/work/retained.component.json".into());
        form::open(&mut st, &doc, ComponentsFormKind::Make, Some("child"), &[]).unwrap();
        let before = state_json(&doc, &st);
        let mut selection = crate::cad::selection::Fixture::new();
        let mut plane = crate::cad::sketch::CadActivePlane::default();
        let (mut continuation, mut replies) = (Value::Null, Replies::default());
        let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: true, replies: &mut replies };
        let mut cx = Cx { settings: &mut crate::app::settings::SettingsOwner::default(), doc: &mut doc, shared: selection.shared(), meshes: None, topology: None, view: None, plane: &mut plane, sketches: None, display: None, views: None, files: None, components: &mut st, composition: &mut crate::cad::composition::CadCompositionState::default(), experiments: &mut crate::cad::experiments::ExperimentsState::default(), review: &mut crate::cad::experiment_review::ReviewState::default(), motion: &mut crate::cad::motion::MotionState::default(), camera: Vec::new() };
        let args = ComponentsArgs { op, path: Some("/work/retained.component.json".into()), ..Default::default() };
        let Outcome::Done(Err(error)) = handle(&args, &mut call, &mut cx) else { panic!("cancelled mutation accepted") };
        assert!(error.contains("cancelled"));
        assert_eq!(state_json(cx.doc, cx.components), before);
        assert!(cx.components.active.is_none());
        assert!(cx.doc.edit.is_none());
    }
}
