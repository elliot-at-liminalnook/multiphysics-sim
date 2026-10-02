//! Lossless storage remains in `CadGraph`; this adapter creates a disposable
//! inspection snapshot using authoritative catalogue/import port schemas.
use super::{CadGraph, ImportedComponent, Recipe, SystemType};
use sim_core::{
    PortSchema, QuantityKind,
    definitions::{SignalType, builtins},
};
use sim_inspect::{
    ComponentDescription, NetDescription, ParameterValue, PortDescription, PortKind, Provenance,
    SystemDescription,
};
use std::collections::BTreeMap;

pub fn matches(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((a, b)) => name.starts_with(a) && name.ends_with(b) && name.len() > a.len() + b.len(),
    }
}
pub fn validate_component(
    c: &super::CadComponent,
    types: &[SystemType],
    recipes: &BTreeMap<String, Recipe>,
) -> Result<(), String> {
    let path = format!("graph.components.{}", c.id);
    let t = types
        .iter()
        .find(|t| t.component_type == c.component_type)
        .ok_or_else(|| format!("{path}.type: unsupported {}", c.component_type))?;
    let declarations = t.parameters.as_ref().ok_or_else(|| {
        format!("{path}.parameters: catalogue declarations unavailable for this type")
    })?;
    let mut derived = BTreeMap::new();
    if let Some(recipe) = &c.derivation {
        let kind = recipe
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("{path}.derivation.kind: required"))?;
        let metadata = recipes
            .get(kind)
            .ok_or_else(|| format!("{path}.derivation.kind: unknown recipe {kind}"))?;
        if metadata.component_type != c.component_type || c.body_id.is_none() {
            return Err(format!(
                "{path}.derivation: incompatible type or missing body"
            ));
        }
        for (name, value) in recipe
            .as_object()
            .ok_or_else(|| format!("{path}.derivation: object required"))?
        {
            if name == "kind" {
                continue;
            }
            let input = metadata
                .inputs
                .get(name)
                .ok_or_else(|| format!("{path}.derivation.{name}: undeclared input"))?;
            let v = value
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("{path}.derivation.{name}: finite number required"))?;
            if input
                .minimum
                .is_some_and(|min| v < min || (v == min && input.exclusive_minimum))
                || (!input.choices.is_empty() && !input.choices.contains(&v))
            {
                return Err(format!(
                    "{path}.derivation.{name}: outside declared range [{}]",
                    input.unit
                ));
            }
        }
        for (name, input) in &metadata.inputs {
            if input.required && recipe.get(name).is_none() {
                return Err(format!(
                    "{path}.derivation.{name}: required [{}]",
                    input.unit
                ));
            }
        }
        derived = metadata.outputs.clone();
    }
    for p in declarations {
        if p.required
            && c.binding.is_none()
            && !c
                .parameters
                .keys()
                .chain(derived.keys())
                .any(|n| matches(&p.name, n))
        {
            return Err(format!(
                "{path}.parameters.{}: required [{}]",
                p.name, p.unit
            ));
        }
    }
    for (name, v) in &c.parameters {
        if derived.contains_key(name) {
            return Err(format!(
                "{path}.parameters.{name}: source-owned derived parameter cannot be overridden"
            ));
        }
        let p = declarations
            .iter()
            .find(|p| p.name == *name)
            .or_else(|| declarations.iter().find(|p| matches(&p.name, name)))
            .ok_or_else(|| format!("{path}.parameters.{name}: undeclared"))?;
        if !v.is_finite()
            || (p.integer && v.fract() != 0.)
            || p.minimum
                .is_some_and(|lo| *v < lo || (*v == lo && p.exclusive_minimum))
            || p.maximum.is_some_and(|hi| *v > hi)
        {
            return Err(format!(
                "{path}.parameters.{name}: outside declared range [{}]",
                p.unit
            ));
        }
    }
    Ok(())
}
fn port_id(component: &str, name: &str) -> String {
    format!(
        "{component}/port/{}",
        name.as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
pub fn adapt(
    graph: &CadGraph,
    revision: u64,
    types: &[SystemType],
    imports: &[ImportedComponent],
    recipes: &BTreeMap<String, Recipe>,
) -> Result<super::Composition, String> {
    graph.validate_storage()?;
    let mut definitions = builtins::registry().map_err(|e| e.to_string())?;
    let mut d = SystemDescription {
        version: sim_inspect::SCHEMA_VERSION,
        id: String::new(),
        source_hash: blake3::hash(&serde_json::to_vec(graph).map_err(|e| e.to_string())?)
            .to_hex()
            .to_string(),
        model_revision: revision,
        definitions: Default::default(),
        components: BTreeMap::new(),
        ports: BTreeMap::new(),
        nets: BTreeMap::new(),
        groups: BTreeMap::new(),
        observables: BTreeMap::new(),
        diagnostics: Vec::new(),
    };
    for (id, c) in &graph.components {
        validate_component(c, types, recipes)?;
        let t = types
            .iter()
            .find(|t| t.component_type == c.component_type)
            .ok_or_else(|| format!("graph.components.{id}.type: missing descriptor"))?;
        let ports = match c.binding.as_deref() {
            Some(binding) => &imports.iter().find(|p|p.binding==binding || p.name==binding).filter(|p|p.component_type==c.component_type).ok_or_else(||format!("graph.components.{id}.binding: {binding} unavailable; select a completed imported check"))?.ports,
            None => &t.ports,
        };
        d.components.insert(
            id.clone(),
            ComponentDescription {
                id: id.clone(),
                label: c.name.clone(),
                component_type: c.component_type.clone(),
                group: None,
                source: None,
                // No CAD artifact digest is supplied by this graph route.
                // body_id remains authoritative in CadGraph; never fabricate
                // a geometry artifact hash from the composition envelope.
                cad: None,
                persistent_identity: true,
                parameters: c
                    .parameters
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            ParameterValue {
                                value: *value,
                                unit: t
                                    .parameters
                                    .as_ref()
                                    .and_then(|p| p.iter().find(|p| matches(&p.name, name)))
                                    .map(|p| p.unit.clone()),
                                provenance: Provenance::Unspecified,
                                uncertainty: None,
                            },
                        )
                    })
                    .collect(),
            },
        );
        for p in ports {
            let names = if p.name.contains('*') {
                c.parameters
                    .keys()
                    .filter(|name| matches(&p.name, name))
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                vec![p.name.clone()]
            };
            for name in names {
                let signal = |q: &QuantityKind| {
                    if *q == QuantityKind::Dimensionless {
                        SignalType::Any
                    } else {
                        SignalType::Quantity(q.definition_id())
                    }
                };
                let schema = match &p.schema {
                    PortSchema::Acausal(k) => {
                        builtins::include_connector(&mut definitions, k.clone())
                            .map_err(|e| format!("graph.components.{id}.ports.{name}: {e}"))?;
                        PortKind::Physical {
                            connector: k.definition_id(),
                        }
                    }
                    PortSchema::SignalIn(q) => PortKind::SignalInput {
                        signal_type: signal(q),
                    },
                    PortSchema::SignalOut(q) => PortKind::SignalOutput {
                        signal_type: signal(q),
                    },
                };
                let pid = port_id(id, &name);
                if let PortSchema::Acausal(kind) = &p.schema {
                    if let Some(sim_core::definitions::ConnectorDescriptor {
                        rule: sim_core::definitions::ConnectionRule::Composite { members },
                        ..
                    }) = definitions.connector_definition(&kind.definition_id())
                    {
                        for member in members {
                            let member_name = format!("{name}.{}", member.name);
                            let member_id = port_id(id, &member_name);
                            d.ports.insert(
                                member_id.clone(),
                                PortDescription {
                                    id: member_id,
                                    component: id.clone(),
                                    name: member_name,
                                    schema: PortKind::Physical {
                                        connector: member.connector.clone(),
                                    },
                                    composite_parent: Some(pid.clone()),
                                },
                            );
                        }
                    }
                }
                d.ports.insert(
                    pid.clone(),
                    PortDescription {
                        id: pid,
                        component: id.clone(),
                        name,
                        schema,
                        composite_parent: None,
                    },
                );
            }
        }
    }
    for (id, net) in &graph.connections {
        let ports = net
            .ports
            .iter()
            .map(|p| port_id(&p.component_id, &p.port))
            .collect();
        d.nets.insert(
            id.clone(),
            NetDescription {
                id: id.clone(),
                ports,
            },
        );
    }
    d.definitions = definitions
        .freeze()
        .map_err(|e| e.to_string())?
        .catalog()
        .clone();
    super::Composition::new(d)
}
