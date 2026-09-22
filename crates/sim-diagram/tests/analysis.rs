use sim_diagram::{
    analysis::*,
    projection::{self, NodeSource},
};
use sim_inspect::SystemDescription;
use std::collections::BTreeSet;
fn captured() -> SystemDescription {
    serde_json::from_str(include_str!(
        "../../../examples/systems-viewer/full-robot.description.json"
    ))
    .unwrap()
}
#[test]
fn groups_and_annotations_preserve_physics_and_roundtrip_with_undo() {
    let d = captured();
    let original = serde_json::to_vec(&d).unwrap();
    let mut journal = Journal::new(Workspace::new(&d));
    let members: BTreeSet<_> = d
        .components
        .keys()
        .filter(|id| id.starts_with("cad/motor/0702845b41d3/"))
        .take(4)
        .cloned()
        .collect();
    assert_eq!(members.len(), 4);
    let mut next = journal.current.clone();
    let id = "analysis/group/1".to_string();
    next.next_group = 2;
    next.groups.insert(
        id.clone(),
        AnalysisGroup {
            id: id.clone(),
            label: "Thermal review".into(),
            members: members.clone(),
            color: [160, 90, 30],
        },
    );
    let component = members.first().unwrap().clone();
    let target = Target::Component(component.clone());
    next.annotations.insert(
        target.key(),
        Annotation {
            target,
            label: Some("Investigate heat path".into()),
            text: "Check the mounting interface before raising current.".into(),
        },
    );
    journal.commit(next, &d).unwrap();
    let presentation = journal.current.presentation(&d).unwrap();
    assert_eq!(presentation.ports, d.ports);
    assert_eq!(presentation.nets, d.nets);
    assert_eq!(presentation.observables, d.observables);
    assert_eq!(
        presentation.components[&component].parameters,
        d.components[&component].parameters
    );
    assert_eq!(serde_json::to_vec(&d).unwrap(), original);
    let collapsed = BTreeSet::from([id.clone()]);
    let projected = projection::project(&presentation, &collapsed, None);
    assert!(
        projected
            .nodes
            .values()
            .any(|source| source == &NodeSource::Group(id.clone()))
    );
    let actual: BTreeSet<_> = projected.connections.values().flatten().cloned().collect();
    let internal = d
        .nets
        .values()
        .filter(|net| {
            net.ports
                .iter()
                .all(|p| members.contains(&d.ports[p].component))
        })
        .count();
    assert_eq!(actual.len() + internal, d.nets.len());
    let json = serde_json::to_vec(&journal.current).unwrap();
    let decoded: Workspace = serde_json::from_slice(&json).unwrap();
    decoded.validate(&d).unwrap();
    assert_eq!(decoded, journal.current);
    assert!(journal.undo());
    assert!(journal.current.groups.is_empty());
    assert!(journal.current.annotations.is_empty());
    assert!(journal.redo());
    assert_eq!(journal.current, decoded);
}
#[test]
fn invalid_or_ambiguous_groups_are_rejected_atomically() {
    let d = captured();
    let mut journal = Journal::new(Workspace::new(&d));
    let before = journal.current.clone();
    let member = d.components.keys().next().unwrap().clone();
    let mut next = before.clone();
    for n in 1..=2 {
        let id = format!("analysis/group/{n}");
        next.groups.insert(
            id.clone(),
            AnalysisGroup {
                id,
                label: format!("Group {n}"),
                members: BTreeSet::from([member.clone()]),
                color: [10, 20, 30],
            },
        );
    }
    assert!(
        journal
            .commit(next, &d)
            .unwrap_err()
            .contains("already belongs")
    );
    assert_eq!(journal.current, before);
    assert!(!journal.can_undo());
    let mut wrong = before.clone();
    wrong.description_id = "different-capture".into();
    assert!(wrong.validate(&d).is_err());
}
