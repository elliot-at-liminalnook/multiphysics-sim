//! Stable authoring identities, typed ports, the behavior registry, and transactional state.

pub mod couple;
pub mod definitions;
pub mod equations;
pub mod parameters;
pub mod primitive;
pub use parameters::ParameterDeclaration;
pub use couple::{Channel, Contract, Coupler, CouplerError, FnCoupler};
pub use equations::{linearization_batch_columns, Behavior, Branch, Context, EquationError, Equations, Input, Lane, LocalJacobian, Output, PreparedResidual, Provision, StateDeclaration, View, param, param_or};

use serde::{Deserialize, Serialize};
use slotmap::{SecondaryMap, SlotMap, new_key_type};
use std::collections::BTreeMap;
use thiserror::Error;

new_key_type! {
    pub struct ObjectId;
    pub struct BehaviorId;
    pub struct PortId;
    pub struct StateId;
}

mod quantity;
pub use quantity::{QuantityKind, quantities};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quantity {
    pub value_si: f64,
    pub kind: QuantityKind,
}

impl Quantity {
    pub const fn new(value_si: f64, kind: QuantityKind) -> Self {
        Self { value_si, kind }
    }
}

mod connector;
pub use connector::{ConnectorKind, ConnectorSchema, connectors};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortSchema {
    Acausal(ConnectorKind),
    SignalIn(QuantityKind),
    SignalOut(QuantityKind),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BehaviorTypeId(pub String);

impl From<&str> for BehaviorTypeId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimObject {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorInstance {
    pub object: ObjectId,
    pub kind: BehaviorTypeId,
    pub parameters: BTreeMap<String, Quantity>,
    pub state: Vec<StateId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    pub owner: BehaviorId,
    pub name: String,
    pub schema: PortSchema,
    /// Member ports of a composite port, in member order.
    #[serde(default)]
    pub members: Vec<PortId>,
    /// This port is member `index` of a composite port.
    #[serde(default)]
    pub member_of: Option<(PortId, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub ports: Vec<PortId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelWorld {
    pub objects: SlotMap<ObjectId, SimObject>,
    pub behaviors: SlotMap<BehaviorId, BehaviorInstance>,
    pub ports: SlotMap<PortId, Port>,
    pub connections: Vec<Connection>,
    pub state: StateStore,
}

impl Default for ModelWorld {
    fn default() -> Self {
        Self {
            objects: SlotMap::with_key(),
            behaviors: SlotMap::with_key(),
            ports: SlotMap::with_key(),
            connections: Vec::new(),
            state: StateStore::default(),
        }
    }
}

impl ModelWorld {
    pub fn add_object(&mut self, name: impl Into<String>) -> ObjectId {
        self.objects.insert(SimObject { name: name.into() })
    }

    pub fn add_behavior(
        &mut self,
        object: ObjectId,
        kind: impl Into<BehaviorTypeId>,
    ) -> BehaviorId {
        self.behaviors.insert(BehaviorInstance {
            object,
            kind: kind.into(),
            parameters: BTreeMap::new(),
            state: Vec::new(),
        })
    }

    pub fn add_port(
        &mut self,
        owner: BehaviorId,
        name: impl Into<String>,
        schema: PortSchema,
    ) -> PortId {
        self.ports.insert(Port {
            owner,
            name: name.into(),
            members: Vec::new(),
            member_of: None,
            schema,
        })
    }

    /// Composite ports connect member-wise; a plain port in the same
    /// connection joins the members of its own kind (the first, if a
    /// composite repeats a kind). Anything that cannot be resolved is left
    /// as written for the compiler to reject.
    pub fn connect(&mut self, ports: impl IntoIterator<Item = PortId>) {
        let ports: Vec<PortId> = ports.into_iter().collect();
        let composites: Vec<PortId> = ports.iter().copied().filter(|p| !self.ports[*p].members.is_empty()).collect();
        if composites.is_empty() {
            self.connections.push(Connection { ports });
            return;
        }
        let parent_schema = &self.ports[composites[0]].schema;
        let members: Vec<_> = self.ports[composites[0]].members.iter().map(|id| self.ports[*id].schema.clone()).collect();
        let same_shape = composites.iter().all(|c| &self.ports[*c].schema == parent_schema && self.ports[*c].members.iter().map(|id| &self.ports[*id].schema).eq(members.iter()));
        if !same_shape {
            self.connections.push(Connection { ports });
            return;
        }
        let mut per_member: Vec<Vec<PortId>> = composites.iter().map(|c| self.ports[*c].members.clone()).fold(vec![Vec::new(); members.len()], |mut acc, m| {
            for (k, id) in m.into_iter().enumerate() {
                acc[k].push(id);
            }
            acc
        });
        for port in ports.iter().filter(|p| self.ports[**p].members.is_empty()) {
            match members.iter().position(|schema| schema == &self.ports[*port].schema) {
                Some(k) => per_member[k].push(*port),
                None => {
                    // Unresolvable: keep the connection as written.
                    self.connections.push(Connection { ports });
                    return;
                }
            }
        }
        for ports in per_member {
            self.connect(ports);
        }
    }

    /// Create a behavior with every port its descriptor declares and the
    /// given parameters, in one step.
    pub fn instantiate<'a>(
        &mut self,
        registry: &BehaviorRegistry,
        object: ObjectId,
        kind: &str,
        parameters: impl IntoIterator<Item = (&'a str, f64)>,
    ) -> Result<Instance, RegistryError> {
        let descriptor = registry.get(&BehaviorTypeId::from(kind))?;
        let declared = descriptor.ports.clone();
        let definitions = registry.frozen_definitions()?;
        let behavior = self.add_behavior(object, kind);
        for (name, value) in parameters {
            self.behaviors[behavior]
                .parameters
                .insert(name.to_owned(), Quantity::new(value, QuantityKind::Dimensionless));
        }
        let mut ports = BTreeMap::new();
        for port in declared {
            // One family member per matching parameter, including families
            // with a suffix such as `imu.*.ax`.
            if port.name.contains('*') {
                let members: Vec<String> = self.behaviors[behavior].parameters.keys().filter(|k| port.matches(k)).cloned().collect();
                for name in members {
                    self.add_declared_port(behavior, name, port.schema.clone(), definitions, &mut ports)?;
                }
                continue;
            }
            self.add_declared_port(behavior, port.name.into(), port.schema, definitions, &mut ports)?;
        }
        Ok(Instance { behavior, ports })
    }

    fn add_declared_port(&mut self, behavior: BehaviorId, name: String, schema: PortSchema,
        definitions: &definitions::FrozenDefinitions, ports: &mut BTreeMap<String, PortId>) -> Result<PortId, RegistryError> {
        let id = self.add_port(behavior, name.clone(), schema.clone());
        ports.insert(name.clone(), id);
        if let PortSchema::Acausal(kind) = schema {
            if let definitions::ConnectionRule::Composite { members } = &definitions.connector_by_id(&kind.definition_id())?.rule {
                for (index, member) in members.iter().enumerate() {
                    let child = self.add_declared_port(behavior, format!("{name}.{}", member.name),
                        PortSchema::Acausal(ConnectorKind::from_id(member.connector.clone())), definitions, ports)?;
                    self.ports[child].member_of = Some((id, index));
                    self.ports[id].members.push(child);
                }
            }
        }
        Ok(id)
    }

    /// Instantiate on a fresh object of the same name.
    pub fn part<'a>(
        &mut self,
        registry: &BehaviorRegistry,
        name: &str,
        kind: &str,
        parameters: impl IntoIterator<Item = (&'a str, f64)>,
    ) -> Result<Instance, RegistryError> {
        let object = self.add_object(name);
        self.instantiate(registry, object, kind, parameters)
    }

    /// Scalar parameters of a behavior, as its equations read them.
    pub fn parameters_of(&self, behavior: BehaviorId) -> BTreeMap<String, f64> {
        self.behaviors[behavior]
            .parameters
            .iter()
            .map(|(name, quantity)| (name.clone(), quantity.value_si))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortDeclaration {
    pub name: &'static str,
    pub schema: PortSchema,
}

impl PortDeclaration {
    /// Match a fixed name or one nonempty wildcard between a prefix and suffix.
    pub fn matches(&self, name: &str) -> bool {
        if let Some((prefix, suffix)) = self.name.split_once('*') {
            name.starts_with(prefix) && name.ends_with(suffix) && name.len() > prefix.len() + suffix.len()
        } else {
            self.name == name
        }
    }
}

#[derive(Debug, Clone)]
pub struct BehaviorDescriptor {
    pub type_id: BehaviorTypeId,
    pub display_name: &'static str,
    pub ports: Vec<PortDeclaration>,
    /// The behavior's equations; `None` for descriptors that only validate
    /// wiring (the older hand-assembled slices).
    pub equations: Option<Equations>,
    /// None means discovery is not yet complete for this native component.
    pub parameters: Option<Vec<ParameterDeclaration>>,
}

impl BehaviorDescriptor {
    pub fn new(type_id: &str, display_name: &'static str, ports: Vec<PortDeclaration>, equations: Equations) -> Self {
        Self { type_id: BehaviorTypeId::from(type_id), display_name, ports, equations: Some(equations), parameters: None }
    }
}

pub const fn acausal(name: &'static str, kind: ConnectorKind) -> PortDeclaration {
    PortDeclaration { name, schema: PortSchema::Acausal(kind) }
}

pub const fn signal_in(name: &'static str, kind: QuantityKind) -> PortDeclaration {
    PortDeclaration { name, schema: PortSchema::SignalIn(kind) }
}

pub const fn signal_out(name: &'static str, kind: QuantityKind) -> PortDeclaration {
    PortDeclaration { name, schema: PortSchema::SignalOut(kind) }
}

/// A behavior instance plus its ports by name, as returned by
/// [`ModelWorld::instantiate`].
#[derive(Debug, Clone)]
pub struct Instance {
    pub behavior: BehaviorId,
    pub ports: BTreeMap<String, PortId>,
}

impl Instance {
    pub fn port(&self, name: &str) -> PortId {
        *self.ports.get(name).unwrap_or_else(|| {
            let available: Vec<&str> = self.ports.keys().map(String::as_str).collect();
            panic!("behavior has no port `{name}`; it has {available:?}")
        })
    }

    /// The port, or `None` — for callers that would rather not panic.
    pub fn try_port(&self, name: &str) -> Option<PortId> {
        self.ports.get(name).copied()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("invalid primitive: {0}")]
    Primitive(String),
    #[error("behavior type `{0}` is already registered")]
    Duplicate(String),
    #[error("behavior type `{0}` is not registered")]
    Missing(String),
    #[error(transparent)]
    Definition(#[from] definitions::DefinitionError),
}

#[derive(Debug, Default, Clone)]
pub struct BehaviorRegistry {
    primitives: primitive::Entries,
    descriptors: BTreeMap<BehaviorTypeId, BehaviorDescriptor>,
    physical_definitions: Option<definitions::DefinitionRegistry>,
    frozen: std::sync::OnceLock<definitions::FrozenDefinitions>,
}

impl BehaviorRegistry {
    /// Registered components in stable type-name order, for authoring tools.
    pub fn descriptors(&self) -> impl Iterator<Item = &BehaviorDescriptor> {
        self.descriptors.values()
    }

    pub fn register(&mut self, mut descriptor: BehaviorDescriptor) -> Result<(), RegistryError> {
        let id = descriptor.type_id.clone();
        if self.descriptors.contains_key(&id) {
            return Err(RegistryError::Duplicate(id.0));
        }
        // Stage anonymous compatibility definitions and parameter metadata;
        // a rejected component must not poison the caller's shared registry.
        let mut physical = match &self.physical_definitions {
            Some(registry) => registry.clone(),
            None => definitions::builtins::registry()?,
        };
        for port in &descriptor.ports {
            if let PortSchema::Acausal(kind) = &port.schema {
                definitions::builtins::include_connector(&mut physical, kind.clone())?;
            }
        }
        let frozen = physical.freeze()?;
        descriptor.resolve_parameters(&frozen)?;
        self.physical_definitions = Some(physical);
        self.frozen = std::sync::OnceLock::from(frozen);
        self.descriptors.insert(id, descriptor);
        Ok(())
    }

    pub fn register_definition(&mut self, definition: &dyn definitions::ComponentDefinition) -> Result<(), RegistryError> {
        self.register(definition.descriptor())
    }

    fn physical_definitions_mut(&mut self) -> Result<&mut definitions::DefinitionRegistry, RegistryError> {
        self.frozen.take();
        if self.physical_definitions.is_none() {
            self.physical_definitions = Some(definitions::builtins::registry()?);
        }
        Ok(self.physical_definitions.as_mut().unwrap())
    }

    pub fn register_quantity(&mut self, definition: &dyn definitions::QuantityDefinition) -> Result<definitions::DefinitionId, RegistryError> {
        Ok(self.physical_definitions_mut()?.register_quantity(definition)?)
    }

    pub fn register_connector(&mut self, definition: &dyn definitions::ConnectorDefinition) -> Result<definitions::DefinitionId, RegistryError> {
        Ok(self.physical_definitions_mut()?.register_connector(definition)?)
    }

    /// One physical catalog for compiler, CAD/Rhai inspection and viewers.
    /// The migration bridge includes legacy anonymous composite declarations.
    pub fn definitions(&self) -> Result<definitions::FrozenDefinitions, RegistryError> {
        Ok(self.frozen_definitions()?.clone())
    }
    pub fn frozen_definitions(&self) -> Result<&definitions::FrozenDefinitions, RegistryError> {
        if self.frozen.get().is_none() {
            let registry = match &self.physical_definitions {
                Some(registry) => registry.clone(),
                None => definitions::builtins::registry()?,
            };
            let _ = self.frozen.set(registry.freeze()?);
        }
        Ok(self.frozen.get().unwrap())
    }

    pub fn get(&self, id: &BehaviorTypeId) -> Result<&BehaviorDescriptor, RegistryError> {
        self.descriptors
            .get(id)
            .ok_or_else(|| RegistryError::Missing(id.0.clone()))
    }

    pub fn contains(&self, id: &BehaviorTypeId) -> bool {
        self.descriptors.contains_key(id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEntry {
    pub name: String,
    /// Component-local declaration identity, independent of its display label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration_name: Option<String>,
    pub quantity: QuantityKind,
    pub committed: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateStore {
    entries: SlotMap<StateId, StateEntry>,
}

#[derive(Debug, Error, PartialEq)]
pub enum StateError {
    #[error("unknown state id")]
    Unknown,
    #[error("state value must be finite, got {0}")]
    NonFinite(f64),
}

#[derive(Debug, Clone)]
pub struct StateTransaction {
    values: SecondaryMap<StateId, f64>,
}

impl StateStore {
    pub fn register(
        &mut self,
        name: impl Into<String>,
        quantity: QuantityKind,
        initial: f64,
    ) -> Result<StateId, StateError> {
        if !initial.is_finite() {
            return Err(StateError::NonFinite(initial));
        }
        Ok(self.entries.insert(StateEntry {
            name: name.into(),
            declaration_name: None,
            quantity,
            committed: initial,
        }))
    }

    pub fn get(&self, id: StateId) -> Result<f64, StateError> {
        self.entries
            .get(id)
            .map(|entry| entry.committed)
            .ok_or(StateError::Unknown)
    }

    pub fn entry(&self, id: StateId) -> Result<&StateEntry, StateError> {
        self.entries.get(id).ok_or(StateError::Unknown)
    }

    pub fn set_declaration_name(&mut self, id: StateId, name: impl Into<String>) -> Result<(), StateError> {
        self.entries.get_mut(id).ok_or(StateError::Unknown)?.declaration_name = Some(name.into());
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = (StateId, &StateEntry)> {
        self.entries.iter()
    }

    pub fn begin_trial(&self) -> StateTransaction {
        let mut values = SecondaryMap::new();
        for (id, entry) in &self.entries {
            values.insert(id, entry.committed);
        }
        StateTransaction { values }
    }

    pub fn commit(&mut self, trial: StateTransaction) -> Result<(), StateError> {
        for (id, _) in &self.entries {
            let value = *trial.values.get(id).ok_or(StateError::Unknown)?;
            if !value.is_finite() {
                return Err(StateError::NonFinite(value));
            }
        }
        for (id, entry) in &mut self.entries {
            let value = *trial.values.get(id).ok_or(StateError::Unknown)?;
            entry.committed = value;
        }
        Ok(())
    }
}

impl StateTransaction {
    pub fn get(&self, id: StateId) -> Result<f64, StateError> {
        self.values.get(id).copied().ok_or(StateError::Unknown)
    }

    pub fn set(&mut self, id: StateId, value: f64) -> Result<(), StateError> {
        if !value.is_finite() {
            return Err(StateError::NonFinite(value));
        }
        *self.values.get_mut(id).ok_or(StateError::Unknown)? = value;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discarded_trial_does_not_mutate_committed_state() {
        let mut store = StateStore::default();
        let id = store
            .register("position", QuantityKind::Length, 0.25)
            .unwrap();
        let mut trial = store.begin_trial();
        trial.set(id, 0.75).unwrap();
        drop(trial);
        assert_eq!(store.get(id).unwrap(), 0.25);
    }

    #[test]
    fn commit_is_atomic_after_validation() {
        let mut store = StateStore::default();
        let a = store.register("a", QuantityKind::Length, 1.0).unwrap();
        let b = store.register("b", QuantityKind::Length, 2.0).unwrap();
        let mut trial = store.begin_trial();
        trial.set(a, 3.0).unwrap();
        trial.set(b, 4.0).unwrap();
        store.commit(trial).unwrap();
        assert_eq!((store.get(a).unwrap(), store.get(b).unwrap()), (3.0, 4.0));
    }
}
