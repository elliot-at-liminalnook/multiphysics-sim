//! Open semantic connector references. Legacy enum spellings are wire adapters only.
use crate::definitions::{DefinitionError, DefinitionId, FrozenDefinitions, ResolvedConnector};
use crate::{Lane, QuantityKind};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum LegacyConnectorKind {
    Electrical,
    Rotational,
    Translational,
    Frame,
    Thermal,
    Hydraulic,
    Acoustic,
    NormalizedAcoustic,
    Magnetic,
    FluidPh,
    Chemical,
    Radiative,
    Granular,
    Planar,
    PlanarFrame,
}

/// A namespaced, versioned reference resolved against the caller's registry.
/// Anonymous members exist solely to read and author version-one composites.
#[derive(Clone)]
pub struct ConnectorKind {
    name: Cow<'static, str>,
    version: u32,
    legacy_members: Option<Cow<'static, [ConnectorKind]>>,
}
impl ConnectorKind {
    pub const fn named(name: &'static str, version: u32) -> Self {
        Self {
            name: Cow::Borrowed(name),
            version,
            legacy_members: None,
        }
    }
    pub fn from_id(id: DefinitionId) -> Self {
        Self {
            name: Cow::Owned(id.name),
            version: id.version,
            legacy_members: None,
        }
    }
    pub fn from_descriptor(d: &crate::definitions::ConnectorDescriptor) -> Self {
        Self::from_id(d.id.clone())
    }
    pub fn definition_id(&self) -> DefinitionId {
        if let Some(members) = &self.legacy_members {
            let ids: Vec<_> = members.iter().map(Self::definition_id).collect();
            let hash = blake3::hash(&serde_json::to_vec(&ids).expect("definition IDs serialize"));
            DefinitionId::new(format!("sim.connector.composite.{}", hash.to_hex()), 1)
        } else {
            DefinitionId::new(self.name.as_ref(), self.version)
        }
    }
    pub fn resolve<'a>(
        &self,
        definitions: &'a FrozenDefinitions,
    ) -> Result<&'a ResolvedConnector, DefinitionError> {
        definitions.layout_by_id(&self.definition_id())
    }
    pub fn name(&self) -> &str {
        self.name
            .strip_prefix("sim.connector.")
            .unwrap_or(&self.name)
    }
    #[allow(non_snake_case)]
    pub const fn Composite(members: &'static [Self]) -> Self {
        Self {
            name: Cow::Borrowed("composite"),
            version: 1,
            legacy_members: Some(Cow::Borrowed(members)),
        }
    }
    pub(crate) fn legacy_members(&self) -> Option<&[Self]> {
        self.legacy_members.as_deref()
    }
    pub(crate) fn legacy(&self) -> Option<LegacyConnectorKind> {
        if self.version != 1 || self.legacy_members.is_some() {
            return None;
        }
        Some(match self.name.as_ref() {
            "sim.connector.electrical" => LegacyConnectorKind::Electrical,
            "sim.connector.rotational" => LegacyConnectorKind::Rotational,
            "sim.connector.translational" => LegacyConnectorKind::Translational,
            "sim.connector.frame" => LegacyConnectorKind::Frame,
            "sim.connector.thermal" => LegacyConnectorKind::Thermal,
            "sim.connector.hydraulic" => LegacyConnectorKind::Hydraulic,
            "sim.connector.acoustic" => LegacyConnectorKind::Acoustic,
            "sim.connector.normalized_acoustic" => LegacyConnectorKind::NormalizedAcoustic,
            "sim.connector.magnetic" => LegacyConnectorKind::Magnetic,
            "sim.connector.fluid_ph" => LegacyConnectorKind::FluidPh,
            "sim.connector.chemical" => LegacyConnectorKind::Chemical,
            "sim.connector.radiative" => LegacyConnectorKind::Radiative,
            "sim.connector.granular" => LegacyConnectorKind::Granular,
            "sim.connector.planar" => LegacyConnectorKind::Planar,
            "sim.connector.planar_frame" => LegacyConnectorKind::PlanarFrame,
            _ => return None,
        })
    }
    /// Compatibility metadata for built-in authoring code only. Generic code
    /// must use `resolve` with its caller-supplied frozen registry.
    pub fn lanes(&self) -> Vec<Lane> {
        if let Some(members) = &self.legacy_members {
            let mut lanes = Vec::new();
            for member in members.iter() {
                let offset = lanes.len();
                lanes.extend(member.lanes().into_iter().map(|mut lane| {
                    lane.derivative_of = lane.derivative_of.map(|i| i + offset);
                    lane
                }));
            }
            lanes
        } else {
            self.legacy()
                .expect("use ConnectorKind::resolve for registered extension metadata")
                .lanes()
        }
    }
    pub fn schema(&self) -> ConnectorSchema {
        let lane = self
            .lanes()
            .into_iter()
            .next()
            .expect("empty legacy connector");
        ConnectorSchema {
            across: lane.across_kind,
            through: lane.through_kind,
        }
    }
    pub fn member_offset(&self, member: usize) -> usize {
        self.legacy_members()
            .unwrap_or(std::slice::from_ref(self))
            .iter()
            .take(member)
            .map(Self::across_width)
            .sum()
    }
    pub fn is_owned(&self) -> bool {
        matches!(
            self.legacy(),
            Some(LegacyConnectorKind::Frame | LegacyConnectorKind::PlanarFrame)
        )
    }
    pub fn owned_wrench_offset(&self) -> usize {
        self.across_width() - self.through_width()
    }
    pub fn through_width(&self) -> usize {
        self.lanes().iter().filter(|l| l.through != "-").count()
    }
    pub fn across_width(&self) -> usize {
        self.lanes().len()
    }
    #[allow(non_upper_case_globals)]
    pub const Electrical: Self = Self::named("sim.connector.electrical", 1);
    #[allow(non_upper_case_globals)]
    pub const Rotational: Self = Self::named("sim.connector.rotational", 1);
    #[allow(non_upper_case_globals)]
    pub const Translational: Self = Self::named("sim.connector.translational", 1);
    #[allow(non_upper_case_globals)]
    pub const Frame: Self = Self::named("sim.connector.frame", 1);
    #[allow(non_upper_case_globals)]
    pub const Thermal: Self = Self::named("sim.connector.thermal", 1);
    #[allow(non_upper_case_globals)]
    pub const Hydraulic: Self = Self::named("sim.connector.hydraulic", 1);
    #[allow(non_upper_case_globals)]
    pub const Acoustic: Self = Self::named("sim.connector.acoustic", 1);
    #[allow(non_upper_case_globals)]
    pub const NormalizedAcoustic: Self = Self::named("sim.connector.normalized_acoustic", 1);
    #[allow(non_upper_case_globals)]
    pub const Magnetic: Self = Self::named("sim.connector.magnetic", 1);
    #[allow(non_upper_case_globals)]
    pub const FluidPh: Self = Self::named("sim.connector.fluid_ph", 1);
    #[allow(non_upper_case_globals)]
    pub const Chemical: Self = Self::named("sim.connector.chemical", 1);
    #[allow(non_upper_case_globals)]
    pub const Radiative: Self = Self::named("sim.connector.radiative", 1);
    #[allow(non_upper_case_globals)]
    pub const Granular: Self = Self::named("sim.connector.granular", 1);
    #[allow(non_upper_case_globals)]
    pub const Planar: Self = Self::named("sim.connector.planar", 1);
    #[allow(non_upper_case_globals)]
    pub const PlanarFrame: Self = Self::named("sim.connector.planar_frame", 1);
    pub const MOTOR: Self = Self::Composite(&MOTOR_MEMBERS);
    pub const BATTERY: Self = Self::Composite(&BATTERY_MEMBERS);
}
impl PartialEq for ConnectorKind {
    fn eq(&self, other: &Self) -> bool {
        self.definition_id() == other.definition_id()
    }
}
impl Eq for ConnectorKind {}
impl std::hash::Hash for ConnectorKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.definition_id().hash(state);
    }
}
impl std::fmt::Debug for ConnectorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(legacy) = self.legacy() {
            return std::fmt::Debug::fmt(&legacy, f);
        }
        if let Some(members) = &self.legacy_members {
            return f.debug_tuple("Composite").field(members).finish();
        }
        f.debug_tuple("Connector")
            .field(&self.definition_id())
            .finish()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorSchema {
    pub across: QuantityKind,
    pub through: QuantityKind,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Wire {
    Legacy(LegacyConnectorKind),
    Composite {
        #[serde(rename = "Composite")]
        members: Vec<ConnectorKind>,
    },
    Named(DefinitionId),
}
impl Serialize for ConnectorKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(legacy) = self.legacy() {
            Wire::Legacy(legacy).serialize(serializer)
        } else if let Some(members) = &self.legacy_members {
            Wire::Composite {
                members: members.to_vec(),
            }
            .serialize(serializer)
        } else {
            Wire::Named(self.definition_id()).serialize(serializer)
        }
    }
}
impl<'de> Deserialize<'de> for ConnectorKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Legacy(legacy) => match legacy {
                LegacyConnectorKind::Electrical => Self::Electrical,
                LegacyConnectorKind::Rotational => Self::Rotational,
                LegacyConnectorKind::Translational => Self::Translational,
                LegacyConnectorKind::Frame => Self::Frame,
                LegacyConnectorKind::Thermal => Self::Thermal,
                LegacyConnectorKind::Hydraulic => Self::Hydraulic,
                LegacyConnectorKind::Acoustic => Self::Acoustic,
                LegacyConnectorKind::NormalizedAcoustic => Self::NormalizedAcoustic,
                LegacyConnectorKind::Magnetic => Self::Magnetic,
                LegacyConnectorKind::FluidPh => Self::FluidPh,
                LegacyConnectorKind::Chemical => Self::Chemical,
                LegacyConnectorKind::Radiative => Self::Radiative,
                LegacyConnectorKind::Granular => Self::Granular,
                LegacyConnectorKind::Planar => Self::Planar,
                LegacyConnectorKind::PlanarFrame => Self::PlanarFrame,
            },
            Wire::Composite { members } => Self {
                name: Cow::Borrowed("composite"),
                version: 1,
                legacy_members: Some(Cow::Owned(members)),
            },
            Wire::Named(id) => Self::from_id(id),
        })
    }
}
#[allow(non_upper_case_globals)]
pub mod connectors {
    use super::ConnectorKind;
    pub const Electrical: ConnectorKind = ConnectorKind::Electrical;
    pub const Rotational: ConnectorKind = ConnectorKind::Rotational;
    pub const Translational: ConnectorKind = ConnectorKind::Translational;
    pub const Frame: ConnectorKind = ConnectorKind::Frame;
    pub const Thermal: ConnectorKind = ConnectorKind::Thermal;
    pub const Hydraulic: ConnectorKind = ConnectorKind::Hydraulic;
    pub const Acoustic: ConnectorKind = ConnectorKind::Acoustic;
    pub const NormalizedAcoustic: ConnectorKind = ConnectorKind::NormalizedAcoustic;
    pub const Magnetic: ConnectorKind = ConnectorKind::Magnetic;
    pub const FluidPh: ConnectorKind = ConnectorKind::FluidPh;
    pub const Chemical: ConnectorKind = ConnectorKind::Chemical;
    pub const Radiative: ConnectorKind = ConnectorKind::Radiative;
    pub const Granular: ConnectorKind = ConnectorKind::Granular;
    pub const Planar: ConnectorKind = ConnectorKind::Planar;
    pub const PlanarFrame: ConnectorKind = ConnectorKind::PlanarFrame;
}

static MOTOR_MEMBERS: [ConnectorKind; 3] = [
    ConnectorKind::Electrical,
    ConnectorKind::Rotational,
    ConnectorKind::Thermal,
];
static BATTERY_MEMBERS: [ConnectorKind; 3] = [
    ConnectorKind::Electrical,
    ConnectorKind::Thermal,
    ConnectorKind::Chemical,
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_and_open_wire_references_preserve_semantic_identity() {
        for connector in [
            ConnectorKind::Electrical,
            ConnectorKind::MOTOR,
            ConnectorKind::named("vendor.new_domain.port", 4),
        ] {
            let bytes = serde_json::to_vec(&connector).unwrap();
            let decoded: ConnectorKind = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(decoded, connector);
            assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
            assert_eq!(ConnectorKind::from_id(connector.definition_id()), connector);
        }
        assert_eq!(
            serde_json::to_string(&ConnectorKind::MOTOR).unwrap(),
            r#"{"Composite":["Electrical","Rotational","Thermal"]}"#
        );
        let decoded: ConnectorKind =
            serde_json::from_str(r#"{"Composite":["Electrical","Thermal"]}"#).unwrap();
        assert!(matches!(decoded.legacy_members, Some(Cow::Owned(_))));
    }
}
