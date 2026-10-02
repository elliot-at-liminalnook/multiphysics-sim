//! Explicit RoboCAD storage adapter. Geometry and derivation evidence remain
//! owned by the source document; presentation never serializes back through a
//! `SystemDocument` (whose hierarchical schema is different).
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CadGraph {
    pub version: u32,
    pub components: BTreeMap<String, CadComponent>,
    pub connections: BTreeMap<String, CadConnection>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CadComponent {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub component_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_id: Option<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivation: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CadEndpoint {
    pub component_id: String,
    pub port: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CadConnection {
    pub id: String,
    pub ports: Vec<CadEndpoint>,
}

impl CadGraph {
    /// Storage validation is independent of topology projection or geometry.
    /// Body membership and recipe execution are checked by RoboCAD at commit.
    pub fn validate_storage(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("graph.version: expected 1".into());
        }
        let mut names = BTreeSet::new();
        let mut bindings = BTreeSet::new();
        for (id, c) in &self.components {
            let path = format!("graph.components.{id}");
            if id.is_empty() || &c.id != id {
                return Err(format!("{path}.id: identity mismatch"));
            }
            if c.name.trim().is_empty() || !names.insert(&c.name) {
                return Err(format!("{path}.name: empty or duplicate"));
            }
            if c.component_type.trim().is_empty() {
                return Err(format!("{path}.type: required"));
            }
            if let Some(binding) = &c.binding {
                if binding.trim().is_empty() || !bindings.insert(binding) {
                    return Err(format!("{path}.binding: empty or duplicate"));
                }
            }
            for (name, value) in &c.parameters {
                if name.is_empty() || !value.is_finite() {
                    return Err(format!("{path}.parameters.{name}: finite value required"));
                }
            }
            if c.derivation.is_some() && c.body_id.is_none() {
                return Err(format!("{path}.derivation: attached body required"));
            }
        }
        let mut occupied = BTreeSet::new();
        for (id, net) in &self.connections {
            let path = format!("graph.connections.{id}");
            if id.is_empty() || &net.id != id || net.ports.is_empty() {
                return Err(format!("{path}: stable id and terminals required"));
            }
            for (i, endpoint) in net.ports.iter().enumerate() {
                if !self.components.contains_key(&endpoint.component_id) || endpoint.port.is_empty()
                {
                    return Err(format!("{path}.ports[{i}]: missing component or port"));
                }
                if !occupied.insert(endpoint) {
                    return Err(format!(
                        "{path}.ports[{i}]: terminal belongs to multiple connections"
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Shared graph contract used by both authoring adapters. It owns a snapshot of
/// inspection topology, never a mutable CAD solid or source-layout transform.
pub struct Composition {
    pub description: sim_inspect::SystemDescription,
}
impl Composition {
    pub fn new(description: sim_inspect::SystemDescription) -> Result<Self, String> {
        validate_connections(&description)?;
        Ok(Self { description })
    }
}

/// Path-named connectivity checks also apply to unsealed authoring snapshots.
pub fn validate_connections(d: &sim_inspect::SystemDescription) -> Result<(), String> {
    use sim_inspect::PortKind;
    let mut occupied = BTreeSet::new();
    for (id, net) in &d.nets {
        let path = format!("nets.{id}");
        let mut physical = Vec::new();
        let mut signal = None;
        let mut outputs = 0;
        let mut physical_count = 0;
        for (index, endpoint) in net.ports.iter().enumerate() {
            let port = d
                .ports
                .get(endpoint)
                .ok_or_else(|| format!("{path}.ports[{index}]: unknown {endpoint}"))?;
            if !d.components.contains_key(&port.component) {
                return Err(format!("ports.{endpoint}.component: missing owner"));
            }
            if !occupied.insert(endpoint) {
                return Err(format!("{path}.ports[{index}]: terminal already connected"));
            }
            match &port.schema {
                PortKind::Unresolved { .. } => {
                    return Err(format!("ports.{endpoint}.schema: unsupported type"));
                }
                PortKind::Physical { connector } => {
                    physical_count += 1;
                    physical.push(connector);
                }
                PortKind::SignalInput { signal_type } | PortKind::SignalOutput { signal_type } => {
                    outputs += usize::from(matches!(port.schema, PortKind::SignalOutput { .. }));
                    if let sim_core::definitions::SignalType::Quantity(quantity) = signal_type {
                        if signal.is_some_and(|first| first != quantity) {
                            return Err(format!("{path}: incompatible signal quantities"));
                        }
                        signal = Some(quantity);
                    }
                }
            }
        }
        if let Some(first) = physical
            .iter()
            .copied()
            .find(|id| {
                d.definitions.connectors.iter().any(|c| {
                    &c.id == *id
                        && matches!(
                            c.rule,
                            sim_core::definitions::ConnectionRule::Composite { .. }
                        )
                })
            })
            .or_else(|| physical.first().copied())
        {
            if physical
                .iter()
                .any(|other| *other != first && !compatible_physical(d, first, other))
            {
                return Err(format!("{path}: incompatible physical connectors"));
            }
        }
        if physical_count != 0 && physical_count != net.ports.len() {
            return Err(format!("{path}: physical and signal ports cannot mix"));
        }
        if physical_count == 0 && outputs != 1 {
            return Err(format!("{path}: exactly one signal output required"));
        }
    }
    Ok(())
}

fn compatible_physical(
    d: &sim_inspect::SystemDescription,
    a: &sim_core::definitions::DefinitionId,
    b: &sim_core::definitions::DefinitionId,
) -> bool {
    use sim_core::definitions::ConnectionRule;
    let members = |id| {
        d.definitions
            .connectors
            .iter()
            .find(|c| &c.id == id)
            .and_then(|c| match &c.rule {
                ConnectionRule::Composite { members } => Some(members),
                _ => None,
            })
    };
    match (members(a), members(b)) {
        (Some(a), Some(b)) => a == b,
        (Some(a), None) => a.iter().any(|m| &m.connector == b),
        (None, Some(b)) => b.iter().any(|m| &m.connector == a),
        _ => false,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub unit: String,
    pub required: bool,
    pub default: Option<f64>,
    pub default_label: Option<String>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub exclusive_minimum: bool,
    pub integer: bool,
    #[serde(default)]
    pub implementation_reference: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    pub schema: sim_core::PortSchema,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemType {
    #[serde(rename = "type")]
    pub component_type: String,
    pub name: String,
    pub parameters: Option<Vec<Parameter>>,
    pub parameters_complete: bool,
    pub ports: Vec<Port>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportedComponent {
    pub binding: String,
    pub name: String,
    #[serde(rename = "type")]
    pub component_type: String,
    pub ports: Vec<Port>,
    #[serde(flatten)]
    pub evidence: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recipe {
    #[serde(rename = "type")]
    pub component_type: String,
    pub outputs: BTreeMap<String, String>,
    #[serde(default)]
    pub inputs: BTreeMap<String, RecipeInput>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecipeInput {
    pub label: String,
    pub unit: String,
    pub required: bool,
    pub default: Option<f64>,
    #[serde(default)]
    pub minimum: Option<f64>,
    #[serde(default)]
    pub exclusive_minimum: bool,
    #[serde(default)]
    pub choices: Vec<f64>,
}

#[path = "composition_adapter.rs"]
pub mod adapter;
#[cfg(test)]
#[path = "composition_tests.rs"]
mod tests;

/// Shared authoring validation called by Build resolver/commands.
pub fn validate_port_schemas(
    schemas: &[(&crate::Terminal, sim_core::PortSchema)],
) -> Result<(), String> {
    let physical: Vec<_> = schemas
        .iter()
        .filter(|(_, s)| matches!(s, sim_core::PortSchema::Acausal(_)))
        .collect();
    let signals: Vec<_> = schemas
        .iter()
        .filter(|(_, s)| !matches!(s, sim_core::PortSchema::Acausal(_)))
        .collect();
    if !physical.is_empty() && !signals.is_empty() {
        return Err(format!(
            "{} is physical but {} is a signal",
            physical[0].0, signals[0].0
        ));
    }
    if let Some((first, sim_core::PortSchema::Acausal(kind))) =
        physical.first().map(|(t, s)| (*t, s))
    {
        for (t, s) in &physical[1..] {
            if let sim_core::PortSchema::Acausal(other) = s {
                if other != kind {
                    return Err(format!(
                        "{first} is {} but {t} is {}",
                        kind.name(),
                        other.name()
                    ));
                }
            }
        }
    }
    let outputs: Vec<_> = signals
        .iter()
        .filter(|(_, s)| matches!(s, sim_core::PortSchema::SignalOut(_)))
        .collect();
    if outputs.len() > 1 {
        return Err(format!(
            "signal net has two outputs: {} and {}",
            outputs[0].0, outputs[1].0
        ));
    }
    let quantity = |s: &sim_core::PortSchema| match s {
        sim_core::PortSchema::SignalIn(q) | sim_core::PortSchema::SignalOut(q) => Some(q.clone()),
        _ => None,
    };
    if let Some((first, s)) = signals.first() {
        let q = quantity(s);
        for (t, other) in &signals[1..] {
            if quantity(other) != q {
                return Err(format!(
                    "{first} carries {:?} but {t} carries {:?}",
                    q.unwrap(),
                    quantity(other).unwrap()
                ));
            }
        }
    }
    Ok(())
}
