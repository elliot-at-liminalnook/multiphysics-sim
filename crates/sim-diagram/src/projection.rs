//! Presentation-only hierarchy projection. Bundles retain the constituent nets;
//! they never become new physical connections or inputs to the compiler.
use sim_inspect::{
    ComponentDescription, NetDescription, PortDescription, PortKind, SystemDescription,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NodeSource {
    Component(String),
    Group(String),
}

pub struct Projection {
    pub view: SystemDescription,
    pub nodes: BTreeMap<String, NodeSource>,
    pub node_members: BTreeMap<String, Vec<String>>,
    pub terminals: BTreeMap<String, Vec<String>>,
    pub connections: BTreeMap<String, Vec<String>>,
    pub hidden_internal_nets: usize,
}

pub fn in_group(description: &SystemDescription, component: &str, group: &str) -> bool {
    let mut parent = description
        .components
        .get(component)
        .and_then(|c| c.group.as_deref());
    while let Some(id) = parent {
        if id == group {
            return true;
        }
        parent = description.groups.get(id).and_then(|g| g.parent.as_deref());
    }
    false
}

fn synthetic(kind: &str, key: &impl serde::Serialize) -> String {
    format!(
        "presentation/{kind}/{}",
        blake3::hash(&serde_json::to_vec(key).unwrap()).to_hex()
    )
}

/// Collapse explicit groups. An optional focus shows its members and their
/// incident nets, with external endpoints retained as context. All identity
/// lookups for inspection go through the returned source maps.
pub fn project(
    description: &SystemDescription,
    collapsed: &BTreeSet<String>,
    focus: Option<&NodeSource>,
) -> Projection {
    let focused: BTreeSet<String> = description
        .components
        .keys()
        .filter(|id| match focus {
            None => true,
            Some(NodeSource::Component(c)) => *id == c,
            Some(NodeSource::Group(g)) => in_group(description, id, g),
        })
        .cloned()
        .collect();
    let representative = |component: &str| {
        let mut parent = description.components[component].group.as_deref();
        let mut chosen = None;
        while let Some(id) = parent {
            if collapsed.contains(id) && !(focus.is_some() && focused.contains(component)) {
                chosen = Some(id);
            }
            parent = description.groups.get(id).and_then(|g| g.parent.as_deref());
        }
        match chosen {
            Some(group) => (synthetic("group", &group), NodeSource::Group(group.into())),
            None => (component.into(), NodeSource::Component(component.into())),
        }
    };
    let mut nodes = BTreeMap::new();
    let mut node_members: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for id in description.components.keys() {
        node_members
            .entry(representative(id).0)
            .or_default()
            .push(id.clone());
    }
    let mut view = description.clone();
    view.components.clear();
    view.ports.clear();
    view.nets.clear();
    view.groups.clear();
    view.observables.clear();
    view.diagnostics.clear();
    let mut add_component = |id: &str| {
        let (visible_id, source) = representative(id);
        let mut component = match &source {
            NodeSource::Component(id) => {
                let mut component = description.components[id].clone();
                if let Some(group) = component
                    .group
                    .as_ref()
                    .and_then(|g| description.groups.get(g))
                {
                    if let Some(local) = component.label.strip_prefix(&format!("{}.", group.label))
                    {
                        component.label = local.into();
                    }
                }
                component
            }
            NodeSource::Group(group) => ComponentDescription {
                id: visible_id.clone(),
                label: description.groups[group].label.clone(),
                component_type: format!(
                    "Subsystem · {} components",
                    node_members[&visible_id].len()
                ),
                group: None,
                source: None,
                cad: None,
                persistent_identity: false,
                parameters: BTreeMap::new(),
            },
        };
        component.group = None;
        nodes.insert(visible_id.clone(), source);
        view.components.insert(visible_id, component);
    };
    for id in &focused {
        add_component(id);
    }
    // A bundle key includes directed terminal schemas. Uncollapsed terminals
    // retain their own port identity and therefore never get merged together.
    type TerminalKey = (String, String, String);
    let mut bundles: BTreeMap<Vec<TerminalKey>, (Vec<String>, BTreeMap<TerminalKey, Vec<String>>)> =
        BTreeMap::new();
    let mut hidden_internal_nets = 0;
    for net in description.nets.values() {
        if focus.is_some()
            && !net
                .ports
                .iter()
                .any(|p| focused.contains(&description.ports[p].component))
        {
            continue;
        }
        let mut endpoints: BTreeMap<TerminalKey, Vec<String>> = BTreeMap::new();
        let mut visible_nodes = BTreeSet::new();
        let mut has_group = false;
        for port in &net.ports {
            let p = &description.ports[port];
            add_component(&p.component);
            let (node, source) = representative(&p.component);
            let group = matches!(source, NodeSource::Group(_));
            has_group |= group;
            visible_nodes.insert(node.clone());
            let key = (
                node,
                serde_json::to_string(&p.schema).unwrap(),
                if group { String::new() } else { p.id.clone() },
            );
            endpoints.entry(key).or_default().push(port.clone());
        }
        if has_group && visible_nodes.len() == 1 {
            hidden_internal_nets += 1;
            continue;
        }
        let key: Vec<_> = endpoints.keys().cloned().collect();
        let bundle = bundles.entry(key).or_default();
        bundle.0.push(net.id.clone());
        for (key, ports) in endpoints {
            bundle.1.entry(key).or_default().extend(ports);
        }
    }
    let mut connections = BTreeMap::new();
    let mut terminals = BTreeMap::new();
    for (_, (nets, endpoints)) in bundles {
        let bundle_id = if nets.len() == 1 {
            nets[0].clone()
        } else {
            synthetic("bundle", &nets)
        };
        let mut ports = Vec::new();
        for ((node, _, original), members) in endpoints {
            let first = &description.ports[&members[0]];
            let port_id = if original.is_empty() {
                synthetic("terminal", &(&bundle_id, &node, &members))
            } else {
                original
            };
            let name = if members.len() == 1 && port_id == first.id {
                first.name.clone()
            } else {
                let label = match &first.schema {
                    PortKind::Physical { connector } => description
                        .definitions
                        .connectors
                        .iter()
                        .find(|c| &c.id == connector)
                        .map(|c| c.label.as_str())
                        .unwrap_or("physical"),
                    PortKind::SignalInput { .. } => "signal in",
                    PortKind::SignalOutput { .. } => "signal out",
                    _ => "unresolved",
                };
                format!("{label} ×{}", members.len())
            };
            view.ports.insert(
                port_id.clone(),
                PortDescription {
                    id: port_id.clone(),
                    component: node,
                    name,
                    schema: first.schema.clone(),
                    composite_parent: None,
                },
            );
            terminals.insert(port_id.clone(), members);
            ports.push(port_id);
        }
        view.nets.insert(
            bundle_id.clone(),
            NetDescription {
                id: bundle_id.clone(),
                ports,
            },
        );
        connections.insert(bundle_id, nets);
    }
    // Preserve unconnected leaf terminals as explicitly open details.
    for port in description.ports.values().filter(|p| {
        !description
            .ports
            .values()
            .any(|child| child.composite_parent.as_deref() == Some(p.id.as_str()))
    }) {
        let (node, source) = representative(&port.component);
        if matches!(source, NodeSource::Component(_))
            && view.components.contains_key(&node)
            && !description
                .nets
                .values()
                .any(|net| net.ports.contains(&port.id))
        {
            view.ports.insert(port.id.clone(), port.clone());
            terminals.insert(port.id.clone(), vec![port.id.clone()]);
        }
    }
    // View metadata is deliberately separate from the immutable runtime description.
    view.id = synthetic("view", &(&description.id, collapsed, format!("{focus:?}")));
    Projection {
        view,
        nodes,
        node_members,
        terminals,
        connections,
        hidden_internal_nets,
    }
}

impl Projection {
    /// Recover original identities, including every member of a collapsed
    /// component or terminal/net bundle. Never send synthetic presentation IDs.
    pub fn source_selection(
        &self,
        selected: Option<&crate::Selection>,
        marked: &BTreeSet<String>,
    ) -> sim_inspect::selection::SelectionTarget {
        use sim_inspect::selection::SelectionTarget as Target;
        match selected {
            None => Target::None,
            Some(crate::Selection::Component(id)) => {
                let nodes = if marked.len() > 1 && marked.contains(id) {
                    marked.clone()
                } else {
                    BTreeSet::from([id.clone()])
                };
                Target::Components {
                    ids: nodes
                        .iter()
                        .filter_map(|id| self.node_members.get(id))
                        .flatten()
                        .cloned()
                        .collect(),
                }
            }
            Some(crate::Selection::Port(id)) => Target::Ports {
                ids: self
                    .terminals
                    .get(id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            },
            Some(crate::Selection::Net(id)) => Target::Nets {
                ids: self
                    .connections
                    .get(id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            },
        }
    }
    /// Project shared selection into the current diagram without expanding,
    /// collapsing, arranging, or changing an engineer's saved workspace.
    pub fn selection_highlights(
        &self,
        selection: &sim_inspect::selection::SelectionDetails,
    ) -> sim_inspect::selection::SelectionDetails {
        sim_inspect::selection::SelectionDetails {
            components: self
                .node_members
                .iter()
                .filter(|(_, ids)| ids.iter().any(|id| selection.components.contains(id)))
                .map(|(id, _)| id.clone())
                .collect(),
            ports: self
                .terminals
                .iter()
                .filter(|(_, ids)| ids.iter().any(|id| selection.ports.contains(id)))
                .map(|(id, _)| id.clone())
                .collect(),
            nets: self
                .connections
                .iter()
                .filter(|(_, ids)| ids.iter().any(|id| selection.nets.contains(id)))
                .map(|(id, _)| id.clone())
                .collect(),
        }
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::Selection;
    use sim_inspect::selection::SelectionTarget;
    #[test]
    fn collapsed_groups_and_bundles_keep_all_original_identities() {
        let d: SystemDescription = serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/full-robot.description.json"
        ))
        .unwrap();
        let p = project(&d, &d.groups.keys().cloned().collect(), None);
        let mut has_group = false;
        let mut has_bundle = false;
        for (id, members) in &p.node_members {
            if !p.view.components.contains_key(id) {
                continue;
            }
            let target =
                p.source_selection(Some(&Selection::Component(id.clone())), &BTreeSet::new());
            assert_eq!(
                target,
                SelectionTarget::Components {
                    ids: members.iter().cloned().collect()
                }
            );
            target.validate(&d).unwrap();
            assert!(
                p.selection_highlights(&target.resolve(&d).unwrap())
                    .components
                    .contains(id)
            );
            has_group |= members.len() > 1;
        }
        for (id, nets) in &p.connections {
            let target = p.source_selection(Some(&Selection::Net(id.clone())), &BTreeSet::new());
            assert_eq!(
                target,
                SelectionTarget::Nets {
                    ids: nets.iter().cloned().collect()
                }
            );
            target.validate(&d).unwrap();
            has_bundle |= nets.len() > 1;
        }
        for id in p.terminals.keys() {
            p.source_selection(Some(&Selection::Port(id.clone())), &BTreeSet::new())
                .validate(&d)
                .unwrap();
        }
        assert!(has_group && has_bundle);
    }
}
