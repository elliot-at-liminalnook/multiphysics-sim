//! Open quantity references; enum spellings are only a version-one wire adapter.
use crate::definitions::{DefinitionError, DefinitionId, FrozenDefinitions, QuantityDescriptor};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum LegacyQuantityKind {
    Dimensionless,
    Time,
    Voltage,
    Current,
    Angle,
    AngularVelocity,
    AngularAcceleration,
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

impl LegacyQuantityKind {
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Dimensionless => "1",
            Self::Time => "s",
            Self::Voltage => "V",
            Self::Current => "A",
            Self::Angle => "rad",
            Self::AngularVelocity => "rad/s",
            Self::AngularAcceleration => "rad/s²",
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

/// A semantic/schema reference with a canonical-unit cache for standalone
/// channel metadata. The registry validates the cache before compilation.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct QuantityKind {
    name: Cow<'static, str>,
    version: u32,
    canonical_unit: Cow<'static, str>,
}

impl QuantityKind {
    pub const fn named(name: &'static str, version: u32, canonical_unit: &'static str) -> Self {
        Self {
            name: Cow::Borrowed(name),
            version,
            canonical_unit: Cow::Borrowed(canonical_unit),
        }
    }
    pub fn from_descriptor(descriptor: &QuantityDescriptor) -> Self {
        Self {
            name: Cow::Owned(descriptor.id.name.clone()),
            version: descriptor.id.version,
            canonical_unit: Cow::Owned(descriptor.canonical_unit.clone()),
        }
    }
    pub fn definition_id(&self) -> DefinitionId {
        DefinitionId::new(self.name.as_ref(), self.version)
    }
    pub fn unit(&self) -> &str {
        &self.canonical_unit
    }
    pub fn validate(&self, definitions: &FrozenDefinitions) -> Result<(), DefinitionError> {
        let descriptor = definitions
            .quantity(definitions.quantity_handle(&self.definition_id())?)
            .expect("handle belongs to the same frozen registry");
        if descriptor.canonical_unit != self.canonical_unit {
            return Err(DefinitionError::Invalid(format!(
                "{} has a canonical-unit cache inconsistent with its registered definition",
                self.name
            )));
        }
        Ok(())
    }
    pub(crate) fn legacy(&self) -> Option<LegacyQuantityKind> {
        if self.version != 1 {
            return None;
        }
        let short = self.name.strip_prefix("sim.quantity.")?;
        let legacy = match short {
            "Dimensionless" => LegacyQuantityKind::Dimensionless,
            "Time" => LegacyQuantityKind::Time,
            "Voltage" => LegacyQuantityKind::Voltage,
            "Current" => LegacyQuantityKind::Current,
            "Angle" => LegacyQuantityKind::Angle,
            "AngularVelocity" => LegacyQuantityKind::AngularVelocity,
            "AngularAcceleration" => LegacyQuantityKind::AngularAcceleration,
            "Torque" => LegacyQuantityKind::Torque,
            "Length" => LegacyQuantityKind::Length,
            "LinearVelocity" => LegacyQuantityKind::LinearVelocity,
            "LinearAcceleration" => LegacyQuantityKind::LinearAcceleration,
            "MassNormalizedDisplacement" => LegacyQuantityKind::MassNormalizedDisplacement,
            "MassNormalizedVelocity" => LegacyQuantityKind::MassNormalizedVelocity,
            "ModalCoordinate" => LegacyQuantityKind::ModalCoordinate,
            "ModalVelocity" => LegacyQuantityKind::ModalVelocity,
            "Force" => LegacyQuantityKind::Force,
            "Impulse" => LegacyQuantityKind::Impulse,
            "AngularImpulse" => LegacyQuantityKind::AngularImpulse,
            "Energy" => LegacyQuantityKind::Energy,
            "Power" => LegacyQuantityKind::Power,
            "Temperature" => LegacyQuantityKind::Temperature,
            "HeatFlow" => LegacyQuantityKind::HeatFlow,
            "Entropy" => LegacyQuantityKind::Entropy,
            "Pressure" => LegacyQuantityKind::Pressure,
            "VolumeFlow" => LegacyQuantityKind::VolumeFlow,
            "Frequency" => LegacyQuantityKind::Frequency,
            "Mass" => LegacyQuantityKind::Mass,
            "MassFlow" => LegacyQuantityKind::MassFlow,
            "SpecificEnthalpy" => LegacyQuantityKind::SpecificEnthalpy,
            "ChemicalPotential" => LegacyQuantityKind::ChemicalPotential,
            "MolarFlow" => LegacyQuantityKind::MolarFlow,
            "Radiosity" => LegacyQuantityKind::Radiosity,
            "MagneticFlux" => LegacyQuantityKind::MagneticFlux,
            _ => return None,
        };
        (legacy.unit() == self.unit()).then_some(legacy)
    }
}

#[allow(non_upper_case_globals)]
impl QuantityKind {
    pub const Dimensionless: Self = Self::named("sim.quantity.Dimensionless", 1, "1");
    pub const Time: Self = Self::named("sim.quantity.Time", 1, "s");
    pub const Voltage: Self = Self::named("sim.quantity.Voltage", 1, "V");
    pub const Current: Self = Self::named("sim.quantity.Current", 1, "A");
    pub const Angle: Self = Self::named("sim.quantity.Angle", 1, "rad");
    pub const AngularVelocity: Self = Self::named("sim.quantity.AngularVelocity", 1, "rad/s");
    pub const AngularAcceleration: Self =
        Self::named("sim.quantity.AngularAcceleration", 1, "rad/s²");
    pub const Torque: Self = Self::named("sim.quantity.Torque", 1, "N·m");
    pub const Length: Self = Self::named("sim.quantity.Length", 1, "m");
    pub const LinearVelocity: Self = Self::named("sim.quantity.LinearVelocity", 1, "m/s");
    pub const LinearAcceleration: Self = Self::named("sim.quantity.LinearAcceleration", 1, "m/s²");
    pub const MassNormalizedDisplacement: Self =
        Self::named("sim.quantity.MassNormalizedDisplacement", 1, "m·√kg");
    pub const MassNormalizedVelocity: Self =
        Self::named("sim.quantity.MassNormalizedVelocity", 1, "m·√kg/s");
    pub const ModalCoordinate: Self = Self::named("sim.quantity.ModalCoordinate", 1, "modal");
    pub const ModalVelocity: Self = Self::named("sim.quantity.ModalVelocity", 1, "modal/s");
    pub const Force: Self = Self::named("sim.quantity.Force", 1, "N");
    pub const Impulse: Self = Self::named("sim.quantity.Impulse", 1, "N·s");
    pub const AngularImpulse: Self = Self::named("sim.quantity.AngularImpulse", 1, "N·m·s");
    pub const Energy: Self = Self::named("sim.quantity.Energy", 1, "J");
    pub const Power: Self = Self::named("sim.quantity.Power", 1, "W");
    pub const Temperature: Self = Self::named("sim.quantity.Temperature", 1, "K");
    pub const HeatFlow: Self = Self::named("sim.quantity.HeatFlow", 1, "W");
    pub const Entropy: Self = Self::named("sim.quantity.Entropy", 1, "J/K");
    pub const Pressure: Self = Self::named("sim.quantity.Pressure", 1, "Pa");
    pub const VolumeFlow: Self = Self::named("sim.quantity.VolumeFlow", 1, "m³/s");
    pub const Frequency: Self = Self::named("sim.quantity.Frequency", 1, "Hz");
    pub const Mass: Self = Self::named("sim.quantity.Mass", 1, "kg");
    pub const MassFlow: Self = Self::named("sim.quantity.MassFlow", 1, "kg/s");
    pub const SpecificEnthalpy: Self = Self::named("sim.quantity.SpecificEnthalpy", 1, "J/kg");
    pub const ChemicalPotential: Self = Self::named("sim.quantity.ChemicalPotential", 1, "J/mol");
    pub const MolarFlow: Self = Self::named("sim.quantity.MolarFlow", 1, "mol/s");
    pub const Radiosity: Self = Self::named("sim.quantity.Radiosity", 1, "W/m²");
    pub const MagneticFlux: Self = Self::named("sim.quantity.MagneticFlux", 1, "Wb");
}

/// Built-in references for concise declarations; external crates define their own.
#[allow(non_upper_case_globals)]
pub mod quantities {
    use super::QuantityKind;
    pub const Dimensionless: QuantityKind = QuantityKind::Dimensionless;
    pub const Time: QuantityKind = QuantityKind::Time;
    pub const Voltage: QuantityKind = QuantityKind::Voltage;
    pub const Current: QuantityKind = QuantityKind::Current;
    pub const Angle: QuantityKind = QuantityKind::Angle;
    pub const AngularVelocity: QuantityKind = QuantityKind::AngularVelocity;
    pub const AngularAcceleration: QuantityKind = QuantityKind::AngularAcceleration;
    pub const Torque: QuantityKind = QuantityKind::Torque;
    pub const Length: QuantityKind = QuantityKind::Length;
    pub const LinearVelocity: QuantityKind = QuantityKind::LinearVelocity;
    pub const LinearAcceleration: QuantityKind = QuantityKind::LinearAcceleration;
    pub const MassNormalizedDisplacement: QuantityKind = QuantityKind::MassNormalizedDisplacement;
    pub const MassNormalizedVelocity: QuantityKind = QuantityKind::MassNormalizedVelocity;
    pub const ModalCoordinate: QuantityKind = QuantityKind::ModalCoordinate;
    pub const ModalVelocity: QuantityKind = QuantityKind::ModalVelocity;
    pub const Force: QuantityKind = QuantityKind::Force;
    pub const Impulse: QuantityKind = QuantityKind::Impulse;
    pub const AngularImpulse: QuantityKind = QuantityKind::AngularImpulse;
    pub const Energy: QuantityKind = QuantityKind::Energy;
    pub const Power: QuantityKind = QuantityKind::Power;
    pub const Temperature: QuantityKind = QuantityKind::Temperature;
    pub const HeatFlow: QuantityKind = QuantityKind::HeatFlow;
    pub const Entropy: QuantityKind = QuantityKind::Entropy;
    pub const Pressure: QuantityKind = QuantityKind::Pressure;
    pub const VolumeFlow: QuantityKind = QuantityKind::VolumeFlow;
    pub const Frequency: QuantityKind = QuantityKind::Frequency;
    pub const Mass: QuantityKind = QuantityKind::Mass;
    pub const MassFlow: QuantityKind = QuantityKind::MassFlow;
    pub const SpecificEnthalpy: QuantityKind = QuantityKind::SpecificEnthalpy;
    pub const ChemicalPotential: QuantityKind = QuantityKind::ChemicalPotential;
    pub const MolarFlow: QuantityKind = QuantityKind::MolarFlow;
    pub const Radiosity: QuantityKind = QuantityKind::Radiosity;
    pub const MagneticFlux: QuantityKind = QuantityKind::MagneticFlux;
}

impl std::fmt::Debug for QuantityKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(legacy) = self.legacy() {
            std::fmt::Debug::fmt(&legacy, f)
        } else {
            f.debug_struct("QuantityKind")
                .field("id", &self.definition_id())
                .field("canonical_unit", &self.unit())
                .finish()
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceWire {
    name: String,
    version: u32,
    canonical_unit: String,
}
impl Serialize for QuantityKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(legacy) = self.legacy() {
            legacy.serialize(serializer)
        } else {
            ReferenceWire {
                name: self.name.to_string(),
                version: self.version,
                canonical_unit: self.unit().into(),
            }
            .serialize(serializer)
        }
    }
}
impl<'de> Deserialize<'de> for QuantityKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Legacy(LegacyQuantityKind),
            Reference(ReferenceWire),
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Legacy(legacy) => match legacy {
                LegacyQuantityKind::Dimensionless => Self::Dimensionless,
                LegacyQuantityKind::Time => Self::Time,
                LegacyQuantityKind::Voltage => Self::Voltage,
                LegacyQuantityKind::Current => Self::Current,
                LegacyQuantityKind::Angle => Self::Angle,
                LegacyQuantityKind::AngularVelocity => Self::AngularVelocity,
                LegacyQuantityKind::AngularAcceleration => Self::AngularAcceleration,
                LegacyQuantityKind::Torque => Self::Torque,
                LegacyQuantityKind::Length => Self::Length,
                LegacyQuantityKind::LinearVelocity => Self::LinearVelocity,
                LegacyQuantityKind::LinearAcceleration => Self::LinearAcceleration,
                LegacyQuantityKind::MassNormalizedDisplacement => Self::MassNormalizedDisplacement,
                LegacyQuantityKind::MassNormalizedVelocity => Self::MassNormalizedVelocity,
                LegacyQuantityKind::ModalCoordinate => Self::ModalCoordinate,
                LegacyQuantityKind::ModalVelocity => Self::ModalVelocity,
                LegacyQuantityKind::Force => Self::Force,
                LegacyQuantityKind::Impulse => Self::Impulse,
                LegacyQuantityKind::AngularImpulse => Self::AngularImpulse,
                LegacyQuantityKind::Energy => Self::Energy,
                LegacyQuantityKind::Power => Self::Power,
                LegacyQuantityKind::Temperature => Self::Temperature,
                LegacyQuantityKind::HeatFlow => Self::HeatFlow,
                LegacyQuantityKind::Entropy => Self::Entropy,
                LegacyQuantityKind::Pressure => Self::Pressure,
                LegacyQuantityKind::VolumeFlow => Self::VolumeFlow,
                LegacyQuantityKind::Frequency => Self::Frequency,
                LegacyQuantityKind::Mass => Self::Mass,
                LegacyQuantityKind::MassFlow => Self::MassFlow,
                LegacyQuantityKind::SpecificEnthalpy => Self::SpecificEnthalpy,
                LegacyQuantityKind::ChemicalPotential => Self::ChemicalPotential,
                LegacyQuantityKind::MolarFlow => Self::MolarFlow,
                LegacyQuantityKind::Radiosity => Self::Radiosity,
                LegacyQuantityKind::MagneticFlux => Self::MagneticFlux,
            },
            Wire::Reference(r) => Self {
                name: Cow::Owned(r.name),
                version: r.version,
                canonical_unit: Cow::Owned(r.canonical_unit),
            },
        })
    }
}
