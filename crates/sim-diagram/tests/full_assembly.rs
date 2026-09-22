use sim_diagram::projection::{self, NodeSource};
use sim_inspect::SystemDescription;
use std::collections::BTreeSet;

fn captured() -> SystemDescription {
    let description: SystemDescription = serde_json::from_str(include_str!(
        "../../../examples/systems-viewer/full-robot.description.json"
    ))
    .unwrap();
    description.validate().unwrap();
    description
}

#[test]
fn overview_preserves_each_connection_without_fabricating_physics() {
    let description = captured();
    let collapsed = description.groups.keys().cloned().collect();
    let overview = projection::project(&description, &collapsed, None);
    let repeat = projection::project(&description, &collapsed, None);
    assert_eq!(description.components.len(), 195);
    assert_eq!(overview.view.components.len(), 7);
    assert_eq!(
        serde_json::to_value(&overview.view).unwrap(),
        serde_json::to_value(&repeat.view).unwrap()
    );
    let retained: Vec<_> = overview.connections.values().flatten().collect();
    assert_eq!(
        retained.len(),
        retained.iter().collect::<BTreeSet<_>>().len()
    );
    assert_eq!(
        retained.len() + overview.hidden_internal_nets,
        description.nets.len()
    );
    assert!(overview.connections.values().any(|nets| nets.len() > 1));
    for (projected, originals) in &overview.connections {
        let actual: BTreeSet<_> = originals
            .iter()
            .flat_map(|net| &description.nets[net].ports)
            .collect();
        let represented: BTreeSet<_> = overview.view.nets[projected]
            .ports
            .iter()
            .flat_map(|port| &overview.terminals[port])
            .collect();
        assert_eq!(actual, represented);
    }
}

#[test]
fn focus_reveals_one_motor_and_retains_every_boundary_terminal() {
    let description = captured();
    let collapsed = description.groups.keys().cloned().collect();
    let group = description
        .groups
        .values()
        .find(|g| g.parent.as_deref() == Some("actuators"))
        .unwrap();
    let focused = projection::project(
        &description,
        &collapsed,
        Some(&NodeSource::Group(group.id.clone())),
    );
    let members: BTreeSet<_> = description
        .components
        .keys()
        .filter(|id| projection::in_group(&description, id, &group.id))
        .collect();
    assert!(members.len() >= 8);
    for id in &members {
        assert_eq!(
            focused.nodes.get(*id),
            Some(&NodeSource::Component((*id).clone()))
        );
    }
    let expected: BTreeSet<_> = description
        .nets
        .values()
        .filter(|net| {
            net.ports
                .iter()
                .any(|port| members.contains(&description.ports[port].component))
        })
        .map(|net| &net.id)
        .collect();
    let actual: BTreeSet<_> = focused.connections.values().flatten().collect();
    assert_eq!(actual, expected);
    assert!(
        focused
            .nodes
            .values()
            .any(|node| matches!(node, NodeSource::Group(_)))
    );
    for id in members {
        assert!(description.components[id].persistent_identity);
        assert!(description.components[id].source.is_some());
    }
}

#[test]
fn focused_motor_routes_every_terminal_without_entering_cards() {
    let description = captured();
    let collapsed = description.groups.keys().cloned().collect();
    let focused = projection::project(
        &description,
        &collapsed,
        Some(&NodeSource::Group("cad/motor/0702845b41d3".into())),
    );
    let start = std::time::Instant::now();
    let state = sim_diagram::initial_state(&focused.view);
    let layout = sim_diagram::route(&focused.view, &state);
    eprintln!(
        "quadruped motor layout {:?}, {} nodes, {} nets",
        start.elapsed(),
        layout.nodes.len(),
        layout.nets.len()
    );
    assert!(layout.unrouted.is_empty(), "{:?}", layout.unrouted);
    for (id, net) in &focused.view.nets {
        let route = &layout.nets[id];
        assert_eq!(route.branches.len(), net.ports.len());
        for port in &net.ports {
            assert!(
                route
                    .branches
                    .iter()
                    .any(|b| b.first() == Some(&layout.ports[port]))
            );
        }
        for branch in &route.branches {
            for segment in branch.windows(2) {
                assert!(segment[0].x == segment[1].x || segment[0].y == segment[1].y);
                for (component, rect) in &layout.nodes {
                    let left = rect.position.x + 0.1;
                    let right = rect.position.x + sim_diagram::layout::WIDTH - 0.1;
                    let top = rect.position.y + 0.1;
                    let bottom = rect.position.y + rect.height - 0.1;
                    let enters = if segment[0].y == segment[1].y {
                        segment[0].y > top
                            && segment[0].y < bottom
                            && segment[0].x.min(segment[1].x) < right
                            && segment[0].x.max(segment[1].x) > left
                    } else {
                        segment[0].x > left
                            && segment[0].x < right
                            && segment[0].y.min(segment[1].y) < bottom
                            && segment[0].y.max(segment[1].y) > top
                    };
                    assert!(!enters, "net {id} enters {component}");
                }
            }
        }
    }
    let width = layout.bounds[1].x - layout.bounds[0].x;
    let height = layout.bounds[1].y - layout.bounds[0].y;
    assert!(
        width / height > 0.8 && width / height < 2.0,
        "unusable fit aspect {width}/{height}"
    );
}

#[test]
fn terminal_trace_does_not_include_other_nets_on_its_component() {
    let d = captured();
    let p = d
        .ports
        .values()
        .find(|p| {
            d.ports
                .values()
                .filter(|other| other.component == p.component)
                .count()
                > 3
        })
        .unwrap();
    let selected = sim_diagram::Selection::Port(p.id.clone());
    let traced = sim_diagram::style::incident_nets(&d, Some(&selected));
    let expected: BTreeSet<_> = d
        .nets
        .values()
        .filter(|n| n.ports.contains(&p.id))
        .map(|n| n.id.clone())
        .collect();
    assert!(!expected.is_empty());
    assert_eq!(traced, expected);
}
