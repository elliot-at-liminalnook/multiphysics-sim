//! Stable authoring identities, typed ports, the behavior registry, and transactional state.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuantityKind {
    Dimensionless,
    Time,
    Voltage,
    Current,
    Angle,
    AngularVelocity,
    Torque,
    Length,
    LinearVelocity,
    Force,
    Energy,
    Power,
}

impl QuantityKind {
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Dimensionless => "1",
            Self::Time => "s",
            Self::Voltage => "V",
            Self::Current => "A",
            Self::Angle => "rad",
            Self::AngularVelocity => "rad/s",
            Self::Torque => "N·m",
            Self::Length => "m",
            Self::LinearVelocity => "m/s",
            Self::Force => "N",
            Self::Energy => "J",
            Self::Power => "W",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quantity {
    pub value_si: f64,
    pub kind: QuantityKind,
}

impl Quantity {
    pub const fn new(value_si: f64, kind: QuantityKind) -> Self {
        Self { value_si, kind }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConnectorKind {
    Electrical,
    Rotational,
    Translational,
    Frame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorSchema {
    pub across: QuantityKind,
    pub through: QuantityKind,
}

impl ConnectorKind {
    pub const fn schema(self) -> ConnectorSchema {
        match self {
            Self::Electrical => ConnectorSchema {
                across: QuantityKind::Voltage,
                through: QuantityKind::Current,
            },
            Self::Rotational => ConnectorSchema {
                across: QuantityKind::Angle,
                through: QuantityKind::Torque,
            },
            Self::Translational => ConnectorSchema {
                across: QuantityKind::Length,
                through: QuantityKind::Force,
            },
            Self::Frame => ConnectorSchema {
                across: QuantityKind::Length,
                through: QuantityKind::Force,
            },
        }
    }

    pub const fn power_unit(self) -> &'static str {
        "W"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
            schema,
        })
    }

    pub fn connect(&mut self, ports: impl IntoIterator<Item = PortId>) {
        self.connections.push(Connection {
            ports: ports.into_iter().collect(),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortDeclaration {
    pub name: &'static str,
    pub schema: PortSchema,
}

#[derive(Debug, Clone)]
pub struct BehaviorDescriptor {
    pub type_id: BehaviorTypeId,
    pub display_name: &'static str,
    pub ports: Vec<PortDeclaration>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("behavior type `{0}` is already registered")]
    Duplicate(String),
    #[error("behavior type `{0}` is not registered")]
    Missing(String),
}

#[derive(Debug, Default, Clone)]
pub struct BehaviorRegistry {
    descriptors: BTreeMap<BehaviorTypeId, BehaviorDescriptor>,
}

impl BehaviorRegistry {
    pub fn register(&mut self, descriptor: BehaviorDescriptor) -> Result<(), RegistryError> {
        let id = descriptor.type_id.clone();
        if self.descriptors.insert(id.clone(), descriptor).is_some() {
            return Err(RegistryError::Duplicate(id.0));
        }
        Ok(())
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
