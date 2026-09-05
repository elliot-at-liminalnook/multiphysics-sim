//! Stable authoring identities, typed ports, the behavior registry, and transactional state.

pub mod couple;
pub mod equations;
pub mod parameters;
pub use parameters::ParameterDeclaration;
pub use couple::{Channel, Contract, Coupler, CouplerError, FnCoupler};
pub use equations::{Behavior, Branch, Context, EquationError, Equations, Input, Lane, LocalJacobian, Output, Provision, StateDeclaration, View, param, param_or};

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
    LinearAcceleration,
    MassNormalizedDisplacement,
    MassNormalizedVelocity,
    ModalCoordinate,
    ModalVelocity,
    Force,
    Impulse,
    AngularImpulse,
    Energy,
    Power,
    Temperature,
    HeatFlow,
    Entropy,
    Pressure,
    VolumeFlow,
    Frequency,
    Mass,
    MassFlow,
    SpecificEnthalpy,
    ChemicalPotential,
    MolarFlow,
    Radiosity,
    MagneticFlux,
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
            Self::LinearAcceleration => "m/s²",
            Self::MassNormalizedDisplacement => "m·√kg",
            Self::MassNormalizedVelocity => "m·√kg/s",
            Self::ModalCoordinate => "modal",
            Self::ModalVelocity => "modal/s",
            Self::Force => "N",
            Self::Impulse => "N·s",
            Self::AngularImpulse => "N·m·s",
            Self::Energy => "J",
            Self::Power => "W",
            Self::Temperature => "K",
            Self::HeatFlow => "W",
            Self::Entropy => "J/K",
            Self::Pressure => "Pa",
            Self::VolumeFlow => "m³/s",
            Self::Frequency => "Hz",
            Self::Mass => "kg",
            Self::MassFlow => "kg/s",
            Self::SpecificEnthalpy => "J/kg",
            Self::ChemicalPotential => "J/mol",
            Self::MolarFlow => "mol/s",
            Self::Radiosity => "W/m²",
            Self::MagneticFlux => "Wb",
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
    /// Across: temperature; through: heat flow. Power is not across × through
    /// here — the port carries signed heat and tracks entropy separately.
    Thermal,
    Hydraulic,
    Acoustic,
    /// Normalized pressure and source flow for nondimensional duct models.
    /// Requires explicit scaling to connect to a physical acoustic model.
    NormalizedAcoustic,
    /// Magnetic circuit, power-conjugate: across mmf (A), through flux rate (Wb/s).
    Magnetic,
    /// Two-phase fluid: across (pressure, specific enthalpy), through
    /// (mass flow, enthalpy flow). Volumes provide both across lanes.
    FluidPh,
    /// One chemical species: across chemical potential (J/mol), through molar flow (mol/s).
    Chemical,
    /// Radiative exchange in one band: across radiosity (W/m²), through radiant power (W).
    Radiative,
    /// Granular material: across the stress on a plane (Pa), through grain mass flow (kg/s).
    Granular,
    /// Two translational lanes (x, y) for planar mechanics.
    Planar,
    /// Owned planar rigid-body frame: pose (x, y, θ) and twist (vx, vy, ω)
    /// across, planar wrench (fx, fy, torque) through.
    PlanarFrame,
    /// A bundle of other connectors behind one port — a motor plug is
    /// `Electrical ⊕ Rotational ⊕ Thermal`. The model fans a composite port
    /// out into member ports (`plug.electrical`, `plug.rotational`, …), so
    /// the compiler only ever sees the members; the behavior sees one flat
    /// lane bundle laid out member after member.
    Composite(#[serde(deserialize_with = "leak_members")] &'static [ConnectorKind]),
}

fn leak_members<'de, D: serde::Deserializer<'de>>(d: D) -> Result<&'static [ConnectorKind], D::Error> {
    let members: Vec<ConnectorKind> = serde::Deserialize::deserialize(d)?;
    Ok(Box::leak(members.into_boxed_slice()))
}

impl ConnectorKind {
    /// Motor plug: winding terminal (return via chassis), shaft, case.
    pub const MOTOR: ConnectorKind = ConnectorKind::Composite(&[ConnectorKind::Electrical, ConnectorKind::Rotational, ConnectorKind::Thermal]);
    /// Battery terminal: electrical, the case, and the electrolyte species.
    pub const BATTERY: ConnectorKind = ConnectorKind::Composite(&[ConnectorKind::Electrical, ConnectorKind::Thermal, ConnectorKind::Chemical]);

    /// Member connectors of a composite; a plain connector is its own single member.
    pub fn members(self) -> &'static [ConnectorKind] {
        match self {
            ConnectorKind::Composite(members) => members,
            ConnectorKind::Electrical => &[ConnectorKind::Electrical],
            ConnectorKind::Rotational => &[ConnectorKind::Rotational],
            ConnectorKind::Translational => &[ConnectorKind::Translational],
            ConnectorKind::Frame => &[ConnectorKind::Frame],
            ConnectorKind::Thermal => &[ConnectorKind::Thermal],
            ConnectorKind::Hydraulic => &[ConnectorKind::Hydraulic],
            ConnectorKind::Acoustic => &[ConnectorKind::Acoustic],
            ConnectorKind::NormalizedAcoustic => &[ConnectorKind::NormalizedAcoustic],
            ConnectorKind::Magnetic => &[ConnectorKind::Magnetic],
            ConnectorKind::FluidPh => &[ConnectorKind::FluidPh],
            ConnectorKind::Chemical => &[ConnectorKind::Chemical],
            ConnectorKind::Radiative => &[ConnectorKind::Radiative],
            ConnectorKind::Granular => &[ConnectorKind::Granular],
            ConnectorKind::Planar => &[ConnectorKind::Planar],
            ConnectorKind::PlanarFrame => &[ConnectorKind::PlanarFrame],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ConnectorKind::Electrical => "electrical",
            ConnectorKind::Rotational => "rotational",
            ConnectorKind::Translational => "translational",
            ConnectorKind::Frame => "frame",
            ConnectorKind::Thermal => "thermal",
            ConnectorKind::Hydraulic => "hydraulic",
            ConnectorKind::Acoustic => "acoustic",
            ConnectorKind::NormalizedAcoustic => "normalized_acoustic",
            ConnectorKind::Magnetic => "magnetic",
            ConnectorKind::FluidPh => "fluid_ph",
            ConnectorKind::Chemical => "chemical",
            ConnectorKind::Radiative => "radiative",
            ConnectorKind::Granular => "granular",
            ConnectorKind::Planar => "planar",
            ConnectorKind::PlanarFrame => "planar_frame",
            ConnectorKind::Composite(_) => "composite",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorSchema {
    pub across: QuantityKind,
    pub through: QuantityKind,
}

impl ConnectorKind {
    pub const fn schema(self) -> ConnectorSchema {
        match self {
            Self::Composite(members) => members[0].schema(),
            Self::Magnetic => ConnectorSchema {
                across: QuantityKind::Current,
                through: QuantityKind::Voltage,
            },
            Self::FluidPh => ConnectorSchema {
                across: QuantityKind::Pressure,
                through: QuantityKind::MassFlow,
            },
            Self::Chemical => ConnectorSchema {
                across: QuantityKind::ChemicalPotential,
                through: QuantityKind::MolarFlow,
            },
            Self::Radiative => ConnectorSchema {
                across: QuantityKind::Radiosity,
                through: QuantityKind::Power,
            },
            Self::Granular => ConnectorSchema {
                across: QuantityKind::Pressure,
                through: QuantityKind::MassFlow,
            },
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
            Self::Thermal => ConnectorSchema {
                across: QuantityKind::Temperature,
                through: QuantityKind::HeatFlow,
            },
            Self::Hydraulic | Self::Acoustic => ConnectorSchema {
                across: QuantityKind::Pressure,
                through: QuantityKind::VolumeFlow,
            },
            Self::NormalizedAcoustic => ConnectorSchema {
                across: QuantityKind::Dimensionless,
                through: QuantityKind::Dimensionless,
            },
            Self::Planar | Self::PlanarFrame => ConnectorSchema {
                across: QuantityKind::Length,
                through: QuantityKind::Force,
            },
        }
    }

    pub const fn power_unit(self) -> &'static str {
        "W"
    }

    /// Whether across × through is a power. Thermal ports carry heat flow
    /// directly, so their conservation law is a heat balance instead.
    pub const fn is_power_conjugate(self) -> bool {
        !matches!(self, Self::Thermal)
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
        let members: Vec<ConnectorKind> = match self.ports[composites[0]].schema {
            PortSchema::Acausal(kind) => kind.members().to_vec(),
            _ => Vec::new(),
        };
        let same_shape = composites.iter().all(|c| matches!(self.ports[*c].schema, PortSchema::Acausal(kind) if kind.members() == members.as_slice()));
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
            let kind = match self.ports[*port].schema {
                PortSchema::Acausal(kind) => Some(kind),
                _ => None,
            };
            match kind.and_then(|k| members.iter().position(|m| *m == k)) {
                Some(k) => per_member[k].push(*port),
                None => {
                    // Unresolvable: keep the connection as written.
                    self.connections.push(Connection { ports });
                    return;
                }
            }
        }
        for ports in per_member {
            self.connections.push(Connection { ports });
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
                    let id = self.add_port(behavior, name.clone(), port.schema);
                    ports.insert(name, id);
                }
                continue;
            }
            let id = self.add_port(behavior, port.name, port.schema);
            ports.insert(port.name.to_owned(), id);
            if let PortSchema::Acausal(ConnectorKind::Composite(members)) = port.schema {
                // Fan the composite out: one member port per member kind.
                for (index, member) in members.iter().enumerate() {
                    let name = format!("{}.{}", port.name, member.name());
                    let member_id = self.add_port(behavior, name.clone(), PortSchema::Acausal(*member));
                    self.ports[member_id].member_of = Some((id, index));
                    self.ports[id].members.push(member_id);
                    ports.insert(name, member_id);
                }
            }
        }
        Ok(Instance { behavior, ports })
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
    /// Registered components in stable type-name order, for authoring tools.
    pub fn descriptors(&self) -> impl Iterator<Item = &BehaviorDescriptor> {
        self.descriptors.values()
    }

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
