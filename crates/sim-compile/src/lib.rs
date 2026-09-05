//! Pure validation and graph compilation from stable authoring IDs to disposable layouts.

pub mod island;

static ELIMINATION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Turn the compile-time elimination of signal and rate-lane unknowns on
/// or off for islands built from now on (default off; see `Island::reduce`).
pub fn set_elimination(on: bool) {
    ELIMINATION.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn elimination_enabled() -> bool {
    ELIMINATION.load(std::sync::atomic::Ordering::Relaxed)
}
pub mod runtime;
pub use island::Island;
pub use runtime::{Runtime, RuntimeError, RuntimeSnapshot};

use petgraph::graph::{NodeIndex, UnGraph};
use petgraph::visit::Dfs;
use sim_core::{
    BehaviorId, BehaviorRegistry, ConnectorKind, ModelWorld, PortId, PortSchema, QuantityKind,
    StateId,
};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompiledConnectionKind {
    Acausal(ConnectorKind),
    Signal(QuantityKind),
}

#[derive(Debug, Clone)]
pub struct CompiledConnection {
    pub ports: Vec<PortId>,
    pub kind: CompiledConnectionKind,
}

#[derive(Debug, Clone)]
pub struct StateLayout {
    pub dense_to_stable: Vec<StateId>,
    pub stable_to_dense: HashMap<StateId, usize>,
}

#[derive(Debug, Clone)]
pub struct CouplingIsland {
    pub behaviors: Vec<BehaviorId>,
}

#[derive(Debug, Clone)]
pub struct CompiledModel {
    pub state_layout: StateLayout,
    pub connections: Vec<CompiledConnection>,
    pub islands: Vec<CouplingIsland>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CompileError {
    #[error("behavior {behavior:?} references missing object")]
    MissingObject { behavior: BehaviorId },
    #[error("behavior {behavior:?} has unregistered type `{kind}`")]
    UnregisteredBehavior { behavior: BehaviorId, kind: String },
    #[error("port {port:?} references missing behavior")]
    MissingOwner { port: PortId },
    #[error("behavior {behavior:?} has undeclared or mismatched port `{name}`")]
    PortMismatch { behavior: BehaviorId, name: String },
    #[error("behavior {behavior:?} is missing declared port `{name}`")]
    MissingPort { behavior: BehaviorId, name: String },
    #[error("connection {connection} needs at least two ports")]
    TooFewPorts { connection: usize },
    #[error("connection {connection} references missing port")]
    MissingPortReference { connection: usize },
    #[error("connection {connection} mixes incompatible port schemas")]
    IncompatibleConnection { connection: usize },
    #[error("signal connection {connection} must have exactly one output")]
    SignalOutputCount { connection: usize },
    #[error("port {port:?} is not connected")]
    DanglingPort { port: PortId },
    #[error("composite port {port:?} must connect through `ModelWorld::connect`, member-wise")]
    CompositeInConnection { port: PortId },
    #[error("port {port:?} appears in more than one connection")]
    PortInTwoConnections { port: PortId },
    #[error("behavior {behavior:?} of type `{kind}` declares no equations")]
    NoEquations { behavior: BehaviorId, kind: String },
    #[error("behavior {behavior:?}: {message}")]
    Equations { behavior: BehaviorId, message: String },
    #[error("state registration failed: {0}")]
    State(String),
}

/// Validate, then build one integrable island per connected component,
/// registering every unknown as a stable state in `model.state`.
pub fn compile_islands(model: &mut ModelWorld, registry: &BehaviorRegistry) -> Result<Vec<Island>, CompileError> {
    let compiled = compile(model, registry)?;
    island::build_islands(model, registry, &compiled.connections)
}

pub fn compile(
    model: &ModelWorld,
    registry: &BehaviorRegistry,
) -> Result<CompiledModel, CompileError> {
    validate_behaviors_and_ports(model, registry)?;

    let mut compiled_connections = Vec::with_capacity(model.connections.len());
    let mut connected = HashSet::new();
    for (connection_index, connection) in model.connections.iter().enumerate() {
        // A single acausal port is an explicit open connection (through = 0);
        // a single signal output is a reading nobody takes.
        let open = connection.ports.len() == 1
            && connection.ports.first().and_then(|p| model.ports.get(*p)).is_some_and(|p| matches!(p.schema, PortSchema::Acausal(_) | PortSchema::SignalOut(_)));
        if connection.ports.len() < 2 && !open {
            return Err(CompileError::TooFewPorts {
                connection: connection_index,
            });
        }
        let ports = connection
            .ports
            .iter()
            .map(|id| {
                if !connected.insert(*id) {
                    return Err(CompileError::PortInTwoConnections { port: *id });
                }
                if model.ports.get(*id).is_some_and(|p| !p.members.is_empty()) {
                    return Err(CompileError::CompositeInConnection { port: *id });
                }
                model
                    .ports
                    .get(*id)
                    .ok_or(CompileError::MissingPortReference {
                        connection: connection_index,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let kind = match ports[0].schema {
            PortSchema::Acausal(expected) => {
                if ports
                    .iter()
                    .any(|port| port.schema != PortSchema::Acausal(expected))
                {
                    return Err(CompileError::IncompatibleConnection {
                        connection: connection_index,
                    });
                }
                CompiledConnectionKind::Acausal(expected)
            }
            PortSchema::SignalIn(expected) | PortSchema::SignalOut(expected) => {
                // Dimensionless signal ports are untyped: a generic controller
                // or filter accepts any quantity.
                let compatible = |kind: QuantityKind| {
                    kind == expected || kind == QuantityKind::Dimensionless || expected == QuantityKind::Dimensionless
                };
                if ports.iter().any(|port| match port.schema {
                    PortSchema::SignalIn(kind) | PortSchema::SignalOut(kind) => !compatible(kind),
                    PortSchema::Acausal(_) => true,
                }) {
                    return Err(CompileError::IncompatibleConnection {
                        connection: connection_index,
                    });
                }
                let outputs = ports
                    .iter()
                    .filter(|port| matches!(port.schema, PortSchema::SignalOut(_)))
                    .count();
                if outputs != 1 {
                    return Err(CompileError::SignalOutputCount {
                        connection: connection_index,
                    });
                }
                CompiledConnectionKind::Signal(expected)
            }
        };

        compiled_connections.push(CompiledConnection {
            ports: connection.ports.clone(),
            kind,
        });
    }

    for (port, declaration) in &model.ports {
        if !connected.contains(&port) {
            // A composite port is represented by its members.
            if !declaration.members.is_empty() {
                continue;
            }
            // An unused signal output still gets an unknown, so it stays
            // observable through the store; anything else must be wired.
            if let PortSchema::SignalOut(kind) = declaration.schema {
                compiled_connections.push(CompiledConnection { ports: vec![port], kind: CompiledConnectionKind::Signal(kind) });
            } else {
                return Err(CompileError::DanglingPort { port });
            }
        }
    }

    let dense_to_stable = model.state.iter().map(|(id, _)| id).collect::<Vec<_>>();
    let stable_to_dense = dense_to_stable
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect();

    Ok(CompiledModel {
        state_layout: StateLayout {
            dense_to_stable,
            stable_to_dense,
        },
        islands: physical_islands(model, &compiled_connections),
        connections: compiled_connections,
    })
}

fn validate_behaviors_and_ports(
    model: &ModelWorld,
    registry: &BehaviorRegistry,
) -> Result<(), CompileError> {
    for (behavior_id, behavior) in &model.behaviors {
        if !model.objects.contains_key(behavior.object) {
            return Err(CompileError::MissingObject {
                behavior: behavior_id,
            });
        }
        let descriptor =
            registry
                .get(&behavior.kind)
                .map_err(|_| CompileError::UnregisteredBehavior {
                    behavior: behavior_id,
                    kind: behavior.kind.0.clone(),
                })?;
        let instance_ports = model
            .ports
            .iter()
            .filter(|(_, port)| port.owner == behavior_id)
            .collect::<Vec<_>>();
        for (port_id, port) in &instance_ports {
            if !model.behaviors.contains_key(port.owner) {
                return Err(CompileError::MissingOwner { port: *port_id });
            }
            // Member ports of a composite are the model's own fan-out.
            if port.member_of.is_some() {
                continue;
            }
            if !descriptor.ports.iter().any(|declared| {
                declared.schema == port.schema && declared.matches(&port.name)
            }) {
                return Err(CompileError::PortMismatch {
                    behavior: behavior_id,
                    name: port.name.clone(),
                });
            }
        }
        for declared in &descriptor.ports {
            if declared.name.contains('*') {
                continue;
            }
            if !instance_ports
                .iter()
                .any(|(_, port)| port.name == declared.name && port.schema == declared.schema)
            {
                return Err(CompileError::MissingPort {
                    behavior: behavior_id,
                    name: declared.name.to_owned(),
                });
            }
        }
    }
    for (port_id, port) in &model.ports {
        if !model.behaviors.contains_key(port.owner) {
            return Err(CompileError::MissingOwner { port: port_id });
        }
    }
    Ok(())
}

fn physical_islands(model: &ModelWorld, connections: &[CompiledConnection]) -> Vec<CouplingIsland> {
    let physical_behaviors = model
        .ports
        .iter()
        .filter_map(|(_, port)| matches!(port.schema, PortSchema::Acausal(_)).then_some(port.owner))
        .collect::<HashSet<_>>();
    let mut graph = UnGraph::<BehaviorId, ()>::new_undirected();
    let mut nodes = HashMap::<BehaviorId, NodeIndex>::new();
    for behavior in &physical_behaviors {
        nodes.insert(*behavior, graph.add_node(*behavior));
    }
    for connection in connections {
        if !matches!(connection.kind, CompiledConnectionKind::Acausal(_)) {
            continue;
        }
        let owners = connection
            .ports
            .iter()
            .filter_map(|port| model.ports.get(*port).map(|port| port.owner))
            .collect::<Vec<_>>();
        for pair in owners.windows(2) {
            graph.update_edge(nodes[&pair[0]], nodes[&pair[1]], ());
        }
    }

    let mut seen = HashSet::new();
    let mut islands = Vec::new();
    for start in graph.node_indices() {
        if seen.contains(&start) {
            continue;
        }
        let mut dfs = Dfs::new(&graph, start);
        let mut behaviors = Vec::new();
        while let Some(node) = dfs.next(&graph) {
            seen.insert(node);
            behaviors.push(graph[node]);
        }
        islands.push(CouplingIsland { behaviors });
    }
    islands
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{BehaviorDescriptor, BehaviorTypeId, PortDeclaration};

    fn electrical_descriptor(name: &str) -> BehaviorDescriptor {
        BehaviorDescriptor {
            type_id: BehaviorTypeId::from(name),
            display_name: "test",
            equations: None,
            parameters: None,
            ports: vec![PortDeclaration {
                name: "pin",
                schema: PortSchema::Acausal(ConnectorKind::Electrical),
            }],
        }
    }

    #[test]
    fn physical_connection_forms_an_island() {
        let mut registry = BehaviorRegistry::default();
        registry.register(electrical_descriptor("source")).unwrap();
        registry.register(electrical_descriptor("load")).unwrap();
        let mut model = ModelWorld::default();
        let source_object = model.add_object("source");
        let load_object = model.add_object("load");
        let source = model.add_behavior(source_object, "source");
        let load = model.add_behavior(load_object, "load");
        let source_pin = model.add_port(
            source,
            "pin",
            PortSchema::Acausal(ConnectorKind::Electrical),
        );
        let load_pin = model.add_port(load, "pin", PortSchema::Acausal(ConnectorKind::Electrical));
        model.connect([source_pin, load_pin]);
        let compiled = compile(&model, &registry).unwrap();
        assert_eq!(compiled.islands.len(), 1);
        assert_eq!(compiled.islands[0].behaviors.len(), 2);
    }

    #[test]
    fn incompatible_connector_is_rejected() {
        let mut registry = BehaviorRegistry::default();
        registry.register(electrical_descriptor("source")).unwrap();
        registry
            .register(BehaviorDescriptor {
                type_id: BehaviorTypeId::from("load"),
                display_name: "load",
                equations: None,
                parameters: None,
                ports: vec![PortDeclaration {
                    name: "pin",
                    schema: PortSchema::Acausal(ConnectorKind::Rotational),
                }],
            })
            .unwrap();
        let mut model = ModelWorld::default();
        let a = model.add_object("a");
        let b = model.add_object("b");
        let source = model.add_behavior(a, "source");
        let load = model.add_behavior(b, "load");
        let source_pin = model.add_port(
            source,
            "pin",
            PortSchema::Acausal(ConnectorKind::Electrical),
        );
        let load_pin = model.add_port(load, "pin", PortSchema::Acausal(ConnectorKind::Rotational));
        model.connect([source_pin, load_pin]);
        assert!(matches!(
            compile(&model, &registry),
            Err(CompileError::IncompatibleConnection { .. })
        ));
    }
}
