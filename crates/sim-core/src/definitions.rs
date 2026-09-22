//! Extensible, serializable physical definitions. Trait implementations run at
//! registration; frozen registries supply validated data and dense handles.
//!
//! These definitions deliberately describe mathematical connection semantics,
//! not a domain enum or a graphics API. Legacy adapters live in `builtins`.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub mod builtins;

/// Durable semantic identity; the version identifies the schema, not a build.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionId {
    pub name: String,
    pub version: u32,
}

impl DefinitionId {
    pub fn new(name: impl Into<String>, version: u32) -> Self {
        Self {
            name: name.into(),
            version,
        }
    }

    fn validate(&self) -> Result<(), DefinitionError> {
        if self.version == 0
            || !self.name.contains('.')
            || self.name.split('.').any(|s| {
                s.is_empty()
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
        {
            return Err(DefinitionError::Invalid(format!(
                "invalid definition identity {self:?}"
            )));
        }
        Ok(())
    }
}

/// Rational SI exponents, with a common denominator, in m, kg, s, A, K, mol, cd
/// order. A denominator of two accommodates mass-normalized displacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    pub powers: [i16; 7],
    pub denominator: u16,
}

impl Dimension {
    pub const DIMENSIONLESS: Self = Self::si([0; 7]);
    pub const TIME: Self = Self::si([0, 0, 1, 0, 0, 0, 0]);
    pub const TEMPERATURE: Self = Self::si([0, 0, 0, 0, 1, 0, 0]);
    pub const POWER: Self = Self::si([2, 1, -3, 0, 0, 0, 0]);

    pub const fn si(powers: [i16; 7]) -> Self {
        Self {
            powers,
            denominator: 1,
        }
    }

    pub fn rational(powers: [i16; 7], denominator: u16) -> Result<Self, DefinitionError> {
        Self::normalize(powers.map(i64::from), i64::from(denominator))
    }

    fn normalize(powers: [i64; 7], denominator: i64) -> Result<Self, DefinitionError> {
        if denominator == 0 {
            return Err(DefinitionError::Invalid(
                "zero dimension denominator".into(),
            ));
        }
        let mut gcd = denominator;
        for p in powers {
            let mut v = p.abs();
            while v != 0 {
                (gcd, v) = (v, gcd % v);
            }
        }
        let mut reduced = [0; 7];
        for (target, source) in reduced.iter_mut().zip(powers) {
            *target = i16::try_from(source / gcd)
                .map_err(|_| DefinitionError::Invalid("dimension exponent overflow".into()))?;
        }
        Ok(Self {
            powers: reduced,
            denominator: u16::try_from(denominator / gcd)
                .map_err(|_| DefinitionError::Invalid("dimension denominator overflow".into()))?,
        })
    }

    pub fn rate(self) -> Result<Self, DefinitionError> {
        let mut powers = self.powers.map(i64::from);
        powers[2] -= i64::from(self.denominator);
        Self::normalize(powers, i64::from(self.denominator))
    }

    pub fn product(self, other: Self) -> Result<Self, DefinitionError> {
        let denominator = i64::from(self.denominator) * i64::from(other.denominator);
        let mut powers = [0; 7];
        for (i, p) in powers.iter_mut().enumerate() {
            *p = i64::from(self.powers[i]) * i64::from(other.denominator)
                + i64::from(other.powers[i]) * i64::from(self.denominator);
        }
        Self::normalize(powers, denominator)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuantityDimension {
    Si {
        dimension: Dimension,
    },
    /// Modal coordinates need a declared normalization before claiming SI units.
    Opaque {
        basis: String,
        time_derivatives: u16,
    },
}

impl From<Dimension> for QuantityDimension {
    fn from(dimension: Dimension) -> Self {
        Self::Si { dimension }
    }
}

impl QuantityDimension {
    fn rate(&self) -> Result<Self, DefinitionError> {
        Ok(match self {
            Self::Si { dimension } => dimension.rate()?.into(),
            Self::Opaque {
                basis,
                time_derivatives,
            } => Self::Opaque {
                basis: basis.clone(),
                time_derivatives: time_derivatives
                    .checked_add(1)
                    .ok_or_else(|| DefinitionError::Invalid("opaque derivative overflow".into()))?,
            },
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantityNature {
    Linear,
    AbsoluteTemperature,
    TemperatureDifference,
}

/// Conversion from display to canonical units: canonical = scale * display + offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayUnit {
    pub symbol: String,
    pub scale: f64,
    pub offset: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantityDescriptor {
    pub id: DefinitionId,
    pub label: String,
    pub dimension: QuantityDimension,
    pub canonical_unit: String,
    pub nature: QuantityNature,
    pub display_units: Vec<DisplayUnit>,
}

pub trait QuantityDefinition: Send + Sync {
    fn descriptor(&self) -> QuantityDescriptor;
}
impl QuantityDefinition for QuantityDescriptor {
    fn descriptor(&self) -> QuantityDescriptor {
        self.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableDescriptor {
    pub name: String,
    pub quantity: DefinitionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneDescriptor {
    pub across: VariableDescriptor,
    /// Absence means no balance contribution, not a dimensionless flow.
    pub through: Option<VariableDescriptor>,
    pub derivative_of: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorMember {
    pub name: String,
    pub connector: DefinitionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectionRule {
    Balanced,
    Owned {
        /// Owner residual row for each through contribution, in lane order.
        contribution_rows: Vec<usize>,
        unit_quaternions: Vec<[usize; 4]>,
    },
    Composite {
        members: Vec<ConnectorMember>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerTerm {
    pub across_lane: usize,
    pub across_rate: bool,
    pub through_lane: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PortEnergy {
    Unavailable,
    Products {
        terms: Vec<PowerTerm>,
    },
    DirectFlow {
        through_lane: usize,
    },
    Heat {
        temperature_lane: usize,
        through_lane: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorDescriptor {
    pub id: DefinitionId,
    pub label: String,
    pub lanes: Vec<LaneDescriptor>,
    pub rule: ConnectionRule,
    pub energy: PortEnergy,
}

pub trait ConnectorDefinition: Send + Sync {
    fn descriptor(&self) -> ConnectorDescriptor;
}

/// Component registration reuses the existing equation factory in the
/// descriptor; implementing this does not introduce another behavior runtime.
pub trait ComponentDefinition: Send + Sync {
    fn descriptor(&self) -> crate::BehaviorDescriptor;
}
impl ComponentDefinition for crate::BehaviorDescriptor {
    fn descriptor(&self) -> crate::BehaviorDescriptor {
        self.clone()
    }
}
impl ConnectorDefinition for ConnectorDescriptor {
    fn descriptor(&self) -> ConnectorDescriptor {
        self.clone()
    }
}

/// Generic signal acceptance is explicit and never a physical quantity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "quantity", rename_all = "snake_case")]
pub enum SignalType {
    Quantity(DefinitionId),
    Any,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DefinitionError {
    #[error("duplicate definition {0:?}")]
    Duplicate(DefinitionId),
    #[error("unregistered definition {0:?}")]
    Missing(DefinitionId),
    #[error("invalid definition: {0}")]
    Invalid(String),
    #[error("cyclic connector composition at {0:?}")]
    Cycle(DefinitionId),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionCatalog {
    pub version: u32,
    pub quantities: Vec<QuantityDescriptor>,
    pub connectors: Vec<ConnectorDescriptor>,
}

/// Mutable authoring registry. Freeze validates cross-references and yields a
/// deterministic catalog; a failed registration never replaces existing data.
#[derive(Debug, Clone, Default)]
pub struct DefinitionRegistry {
    quantities: BTreeMap<DefinitionId, QuantityDescriptor>,
    connectors: BTreeMap<DefinitionId, ConnectorDescriptor>,
}

impl DefinitionRegistry {
    pub fn connector_definition(&self, id: &DefinitionId) -> Option<&ConnectorDescriptor> {
        self.connectors.get(id)
    }
    pub fn register_quantity(
        &mut self,
        definition: &dyn QuantityDefinition,
    ) -> Result<DefinitionId, DefinitionError> {
        let q = definition.descriptor();
        q.id.validate()?;
        validate_quantity(&q)?;
        if self.quantities.contains_key(&q.id) || self.connectors.contains_key(&q.id) {
            return Err(DefinitionError::Duplicate(q.id));
        }
        let id = q.id.clone();
        self.quantities.insert(id.clone(), q);
        Ok(id)
    }

    pub fn register_connector(
        &mut self,
        definition: &dyn ConnectorDefinition,
    ) -> Result<DefinitionId, DefinitionError> {
        let c = definition.descriptor();
        c.id.validate()?;
        if self.connectors.contains_key(&c.id) || self.quantities.contains_key(&c.id) {
            return Err(DefinitionError::Duplicate(c.id));
        }
        let id = c.id.clone();
        self.connectors.insert(id.clone(), c);
        Ok(id)
    }

    pub fn from_catalog(catalog: DefinitionCatalog) -> Result<Self, DefinitionError> {
        if catalog.version != 1 {
            return Err(DefinitionError::Invalid(
                "unsupported definition catalog version".into(),
            ));
        }
        let mut registry = Self::default();
        for q in catalog.quantities {
            registry.register_quantity(&q)?;
        }
        for c in catalog.connectors {
            registry.register_connector(&c)?;
        }
        registry.freeze()?;
        Ok(registry)
    }

    pub fn freeze(&self) -> Result<FrozenDefinitions, DefinitionError> {
        let mut resolved = BTreeMap::new();
        let mut visiting = BTreeSet::new();
        for id in self.connectors.keys() {
            self.resolve(id, &mut visiting, &mut resolved)?;
        }
        let catalog = DefinitionCatalog {
            version: 1,
            quantities: self.quantities.values().cloned().collect(),
            connectors: self.connectors.values().cloned().collect(),
        };
        let hash = blake3::hash(
            &serde_json::to_vec(&catalog).map_err(|e| DefinitionError::Invalid(e.to_string()))?,
        );
        let fingerprint = hash.to_hex().to_string();
        let identity = *hash.as_bytes();
        let quantity_indices = catalog
            .quantities
            .iter()
            .enumerate()
            .map(|(index, q)| (q.id.clone(), QuantityHandle { index, identity }))
            .collect();
        let connector_indices = catalog
            .connectors
            .iter()
            .enumerate()
            .map(|(index, c)| (c.id.clone(), ConnectorHandle { index, identity }))
            .collect();
        let layouts = catalog
            .connectors
            .iter()
            .map(|c| resolved.remove(&c.id).unwrap())
            .collect();
        Ok(FrozenDefinitions {
            catalog,
            fingerprint,
            identity,
            quantity_indices,
            connector_indices,
            layouts,
        })
    }

    fn resolve(
        &self,
        id: &DefinitionId,
        visiting: &mut BTreeSet<DefinitionId>,
        resolved: &mut BTreeMap<DefinitionId, ResolvedConnector>,
    ) -> Result<(), DefinitionError> {
        if resolved.contains_key(id) {
            return Ok(());
        }
        if !visiting.insert(id.clone()) {
            return Err(DefinitionError::Cycle(id.clone()));
        }
        let c = self
            .connectors
            .get(id)
            .ok_or_else(|| DefinitionError::Missing(id.clone()))?;
        if c.label.trim().is_empty() {
            return Err(DefinitionError::Invalid(format!(
                "{} has no label",
                id.name
            )));
        }
        let layout =
            if let ConnectionRule::Composite { members } = &c.rule {
                if members.is_empty() || !c.lanes.is_empty() || c.energy != PortEnergy::Unavailable
                {
                    return Err(DefinitionError::Invalid(format!(
                        "{} composite must have members and derive lanes/diagnostics from them",
                        id.name
                    )));
                }
                let mut names = BTreeSet::new();
                let mut lanes = Vec::new();
                let mut member_offsets = Vec::new();
                for member in members {
                    if !valid_local_name(&member.name) || !names.insert(&member.name) {
                        return Err(DefinitionError::Invalid(format!(
                            "{} invalid/duplicate composite member",
                            id.name
                        )));
                    }
                    self.resolve(&member.connector, visiting, resolved)?;
                    let offset = lanes.len();
                    member_offsets.push(offset);
                    lanes.extend(resolved[&member.connector].lanes.iter().cloned().map(
                        |mut lane| {
                            lane.across.name = format!("{}.{}", member.name, lane.across.name);
                            if let Some(flow) = &mut lane.through {
                                flow.name = format!("{}.{}", member.name, flow.name);
                            }
                            lane.derivative_of = lane.derivative_of.map(|i| i + offset);
                            lane
                        },
                    ));
                }
                ResolvedConnector {
                    lanes,
                    member_offsets,
                }
            } else {
                self.validate_lanes(c)?;
                ResolvedConnector {
                    lanes: c.lanes.clone(),
                    member_offsets: Vec::new(),
                }
            };
        visiting.remove(id);
        resolved.insert(id.clone(), layout);
        Ok(())
    }

    fn quantity(&self, id: &DefinitionId) -> Result<&QuantityDescriptor, DefinitionError> {
        self.quantities
            .get(id)
            .ok_or_else(|| DefinitionError::Missing(id.clone()))
    }

    fn validate_lanes(&self, c: &ConnectorDescriptor) -> Result<(), DefinitionError> {
        let invalid = |s: &str| DefinitionError::Invalid(format!("{}: {s}", c.id.name));
        if c.lanes.is_empty() {
            return Err(invalid("connector has no lanes"));
        }
        let mut across_names = BTreeSet::new();
        let mut through_names = BTreeSet::new();
        for (i, lane) in c.lanes.iter().enumerate() {
            let q = self.quantity(&lane.across.quantity)?;
            if !valid_local_name(&lane.across.name) || !across_names.insert(&lane.across.name) {
                return Err(invalid("invalid or duplicate across lane"));
            }
            if let Some(flow) = &lane.through {
                self.quantity(&flow.quantity)?;
                if !valid_local_name(&flow.name) || !through_names.insert(&flow.name) {
                    return Err(invalid("invalid or duplicate through lane"));
                }
            }
            if let Some(base) = lane.derivative_of {
                if base >= i || lane.through.is_some() {
                    return Err(invalid(
                        "derivative must reference an earlier lane and have no flow contribution",
                    ));
                }
                if self
                    .quantity(&c.lanes[base].across.quantity)?
                    .dimension
                    .rate()?
                    != q.dimension
                {
                    return Err(invalid("derivative dimensions do not match"));
                }
            }
        }
        if let ConnectionRule::Owned {
            contribution_rows,
            unit_quaternions,
        } = &c.rule
        {
            if contribution_rows.len() != through_names.len()
                || contribution_rows.iter().any(|r| *r >= c.lanes.len())
                || contribution_rows.iter().collect::<BTreeSet<_>>().len()
                    != contribution_rows.len()
            {
                return Err(invalid("invalid owner contribution mapping"));
            }
            let mut used = BTreeSet::new();
            for q in unit_quaternions {
                for index in q {
                    if !used.insert(*index) || *index >= c.lanes.len() {
                        return Err(invalid("invalid unit quaternion indices"));
                    }
                    if self.quantity(&c.lanes[*index].across.quantity)?.dimension
                        != Dimension::DIMENSIONLESS.into()
                    {
                        return Err(invalid("quaternion coordinates must be dimensionless"));
                    }
                }
            }
        }
        let flow_dimension = |i: usize| -> Result<QuantityDimension, DefinitionError> {
            let flow = c
                .lanes
                .get(i)
                .and_then(|l| l.through.as_ref())
                .ok_or_else(|| invalid("energy references missing flow lane"))?;
            Ok(self.quantity(&flow.quantity)?.dimension.clone())
        };
        match &c.energy {
            PortEnergy::Unavailable => (),
            PortEnergy::Products { terms } => {
                if terms.is_empty() {
                    return Err(invalid("empty energy product"));
                }
                let mut seen = BTreeSet::new();
                for term in terms {
                    if !seen.insert((term.across_lane, term.across_rate, term.through_lane)) {
                        return Err(invalid("duplicate power term"));
                    }
                    let lane = c
                        .lanes
                        .get(term.across_lane)
                        .ok_or_else(|| invalid("energy references missing across lane"))?;
                    let mut d = self.quantity(&lane.across.quantity)?.dimension.clone();
                    if term.across_rate {
                        d = d.rate()?;
                    }
                    let (
                        QuantityDimension::Si { dimension: across },
                        QuantityDimension::Si { dimension: through },
                    ) = (d, flow_dimension(term.through_lane)?)
                    else {
                        return Err(invalid("opaque dimensions cannot declare power products"));
                    };
                    if across.product(through)? != Dimension::POWER {
                        return Err(invalid("energy product is not power"));
                    }
                }
            }
            PortEnergy::DirectFlow { through_lane } | PortEnergy::Heat { through_lane, .. } => {
                if flow_dimension(*through_lane)? != Dimension::POWER.into() {
                    return Err(invalid("energy flow must be power"));
                }
                if let PortEnergy::Heat {
                    temperature_lane, ..
                } = c.energy
                {
                    let lane = c
                        .lanes
                        .get(temperature_lane)
                        .ok_or_else(|| invalid("missing temperature lane"))?;
                    if self.quantity(&lane.across.quantity)?.nature
                        != QuantityNature::AbsoluteTemperature
                    {
                        return Err(invalid("entropy requires absolute temperature"));
                    }
                }
            }
        }
        Ok(())
    }
}

fn valid_local_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn validate_quantity(q: &QuantityDescriptor) -> Result<(), DefinitionError> {
    let invalid = |s: &str| DefinitionError::Invalid(format!("{}: {s}", q.id.name));
    if q.label.trim().is_empty() || q.canonical_unit.trim().is_empty() {
        return Err(invalid("missing label or canonical unit"));
    }
    match &q.dimension {
        QuantityDimension::Si { dimension } => {
            if Dimension::rational(dimension.powers, dimension.denominator)? != *dimension {
                return Err(invalid("dimension must be normalized"));
            }
        }
        QuantityDimension::Opaque { basis, .. } => {
            if basis.trim().is_empty() {
                return Err(invalid("missing opaque basis"));
            }
        }
    }
    if q.nature != QuantityNature::Linear && q.dimension != Dimension::TEMPERATURE.into() {
        return Err(invalid("temperature must have temperature dimension"));
    }
    let mut names = BTreeSet::from([q.canonical_unit.as_str()]);
    for unit in &q.display_units {
        if unit.symbol.trim().is_empty()
            || !names.insert(&unit.symbol)
            || !unit.scale.is_finite()
            || unit.scale <= 0.
            || !unit.offset.is_finite()
            || (unit.offset != 0. && q.nature != QuantityNature::AbsoluteTemperature)
        {
            return Err(invalid(
                "invalid, duplicate or incompatible display conversion",
            ));
        }
    }
    Ok(())
}

/// Handles belong to one frozen registry; never persist them in model files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuantityHandle {
    index: usize,
    identity: [u8; 32],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectorHandle {
    index: usize,
    identity: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct ResolvedConnector {
    pub lanes: Vec<LaneDescriptor>,
    pub member_offsets: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct FrozenDefinitions {
    catalog: DefinitionCatalog,
    fingerprint: String,
    identity: [u8; 32],
    quantity_indices: BTreeMap<DefinitionId, QuantityHandle>,
    connector_indices: BTreeMap<DefinitionId, ConnectorHandle>,
    layouts: Vec<ResolvedConnector>,
}

impl FrozenDefinitions {
    pub fn catalog(&self) -> &DefinitionCatalog {
        &self.catalog
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn quantity_handle(&self, id: &DefinitionId) -> Result<QuantityHandle, DefinitionError> {
        self.quantity_indices
            .get(id)
            .copied()
            .ok_or_else(|| DefinitionError::Missing(id.clone()))
    }
    pub fn connector_handle(&self, id: &DefinitionId) -> Result<ConnectorHandle, DefinitionError> {
        self.connector_indices
            .get(id)
            .copied()
            .ok_or_else(|| DefinitionError::Missing(id.clone()))
    }
    pub fn quantity(&self, handle: QuantityHandle) -> Option<&QuantityDescriptor> {
        (handle.identity == self.identity)
            .then(|| self.catalog.quantities.get(handle.index))
            .flatten()
    }
    pub fn connector(&self, handle: ConnectorHandle) -> Option<&ConnectorDescriptor> {
        (handle.identity == self.identity)
            .then(|| self.catalog.connectors.get(handle.index))
            .flatten()
    }
    pub fn layout(&self, handle: ConnectorHandle) -> Option<&ResolvedConnector> {
        (handle.identity == self.identity)
            .then(|| self.layouts.get(handle.index))
            .flatten()
    }

    pub fn connector_by_id(
        &self,
        id: &DefinitionId,
    ) -> Result<&ConnectorDescriptor, DefinitionError> {
        self.connector(self.connector_handle(id)?)
            .ok_or_else(|| DefinitionError::Missing(id.clone()))
    }

    pub fn layout_by_id(&self, id: &DefinitionId) -> Result<&ResolvedConnector, DefinitionError> {
        self.layout(self.connector_handle(id)?)
            .ok_or_else(|| DefinitionError::Missing(id.clone()))
    }
}
