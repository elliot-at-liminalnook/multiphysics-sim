//! Built-in descriptor registration and version-one compatibility translation.
//! Extension crates register their own definitions without edits here. The compiler
//! consumes frozen descriptors; legacy metadata is not a domain dispatch interface.
use super::*;
use crate::{ConnectorKind as C, QuantityKind as Q};
use crate::quantity::LegacyQuantityKind as L;
use crate::connector::LegacyConnectorKind as LC;

pub const QUANTITIES: &[Q] = &[
    Q::Dimensionless,
    Q::Time,
    Q::Voltage,
    Q::Current,
    Q::Angle,
    Q::AngularVelocity,
    Q::AngularAcceleration,
    Q::Torque,
    Q::Length,
    Q::LinearVelocity,
    Q::LinearAcceleration,
    Q::MassNormalizedDisplacement,
    Q::MassNormalizedVelocity,
    Q::ModalCoordinate,
    Q::ModalVelocity,
    Q::Force,
    Q::Impulse,
    Q::AngularImpulse,
    Q::Energy,
    Q::Power,
    Q::Temperature,
    Q::HeatFlow,
    Q::Entropy,
    Q::Pressure,
    Q::VolumeFlow,
    Q::Frequency,
    Q::Mass,
    Q::MassFlow,
    Q::SpecificEnthalpy,
    Q::ChemicalPotential,
    Q::MolarFlow,
    Q::Radiosity,
    Q::MagneticFlux,
];

pub const CONNECTORS: &[C] = &[
    C::Electrical,
    C::Rotational,
    C::Translational,
    C::Frame,
    C::Thermal,
    C::Hydraulic,
    C::Acoustic,
    C::NormalizedAcoustic,
    C::Magnetic,
    C::FluidPh,
    C::Chemical,
    C::Radiative,
    C::Granular,
    C::Planar,
    C::PlanarFrame,
    C::MOTOR,
    C::BATTERY,
];

pub fn quantity_id(q: impl std::borrow::Borrow<Q>) -> DefinitionId { q.borrow().definition_id() }

pub fn connector_id(c: impl std::borrow::Borrow<C>) -> DefinitionId { c.borrow().definition_id() }

pub fn quantity(q: Q) -> QuantityDescriptor {
    let legacy = q.legacy().expect("built-in descriptor requested for a registered extension quantity");
    let dimension = match legacy {
        L::Dimensionless | L::Angle => Dimension::DIMENSIONLESS,
        L::Time => Dimension::TIME,
        L::Voltage => Dimension::si([2, 1, -3, -1, 0, 0, 0]),
        L::Current => Dimension::si([0, 0, 0, 1, 0, 0, 0]),
        L::AngularVelocity | L::Frequency => Dimension::si([0, 0, -1, 0, 0, 0, 0]),
        L::AngularAcceleration => Dimension::si([0, 0, -2, 0, 0, 0, 0]),
        L::Torque | L::Energy => Dimension::si([2, 1, -2, 0, 0, 0, 0]),
        L::Length => Dimension::si([1, 0, 0, 0, 0, 0, 0]),
        L::LinearVelocity => Dimension::si([1, 0, -1, 0, 0, 0, 0]),
        L::LinearAcceleration => Dimension::si([1, 0, -2, 0, 0, 0, 0]),
        L::MassNormalizedDisplacement => Dimension {
            powers: [2, 1, 0, 0, 0, 0, 0],
            denominator: 2,
        },
        L::MassNormalizedVelocity => Dimension {
            powers: [2, 1, -2, 0, 0, 0, 0],
            denominator: 2,
        },
        L::Force => Dimension::si([1, 1, -2, 0, 0, 0, 0]),
        L::Impulse => Dimension::si([1, 1, -1, 0, 0, 0, 0]),
        L::AngularImpulse => Dimension::si([2, 1, -1, 0, 0, 0, 0]),
        L::Power | L::HeatFlow => Dimension::POWER,
        L::Temperature => Dimension::TEMPERATURE,
        L::Entropy => Dimension::si([2, 1, -2, 0, -1, 0, 0]),
        L::Pressure => Dimension::si([-1, 1, -2, 0, 0, 0, 0]),
        L::VolumeFlow => Dimension::si([3, 0, -1, 0, 0, 0, 0]),
        L::Mass => Dimension::si([0, 1, 0, 0, 0, 0, 0]),
        L::MassFlow => Dimension::si([0, 1, -1, 0, 0, 0, 0]),
        L::SpecificEnthalpy => Dimension::si([2, 0, -2, 0, 0, 0, 0]),
        L::ChemicalPotential => Dimension::si([2, 1, -2, 0, 0, -1, 0]),
        L::MolarFlow => Dimension::si([0, 0, -1, 0, 0, 1, 0]),
        L::Radiosity => Dimension::si([0, 1, -3, 0, 0, 0, 0]),
        L::MagneticFlux => Dimension::si([2, 1, -2, -1, 0, 0, 0]),
        L::ModalCoordinate | L::ModalVelocity => Dimension::DIMENSIONLESS,
    };
    QuantityDescriptor {
        id: quantity_id(&q),
        label: format!("{q:?}"),
        canonical_unit: q.unit().into(),
        dimension: match legacy {
            L::ModalCoordinate | L::ModalVelocity => QuantityDimension::Opaque {
                basis: "sim.modal.normalization_unspecified".into(),
                time_derivatives: u16::from(q == Q::ModalVelocity),
            },
            _ => dimension.into(),
        },
        nature: if q == Q::Temperature {
            QuantityNature::AbsoluteTemperature
        } else {
            QuantityNature::Linear
        },
        display_units: if q == Q::Temperature {
            vec![DisplayUnit {
                symbol: "°C".into(),
                scale: 1.,
                offset: 273.15,
            }]
        } else {
            Vec::new()
        },
    }
}

pub fn connector(c: C) -> ConnectorDescriptor {
    let id = connector_id(&c);
    if let Some(members) = c.legacy_members() {
        return ConnectorDescriptor {
            id,
            label: "Composite connector".into(),
            lanes: Vec::new(),
            rule: ConnectionRule::Composite {
                members: members
                    .iter()
                    .map(|m| ConnectorMember {
                        name: m.name().into(),
                        connector: connector_id(m),
                    })
                    .collect(),
            },
            energy: PortEnergy::Unavailable,
        };
    }
    let lanes = c
        .lanes()
        .iter()
        .map(|l| LaneDescriptor {
            across: VariableDescriptor {
                name: l.across.into(),
                quantity: quantity_id(&l.across_kind),
            },
            through: (l.through != "-").then(|| VariableDescriptor {
                name: l.through.into(),
                quantity: quantity_id(&l.through_kind),
            }),
            derivative_of: l.derivative_of,
        })
        .collect();
    let product = |pairs: &[(usize, bool, usize)]| PortEnergy::Products {
        terms: pairs
            .iter()
            .map(|(a, rate, t)| PowerTerm {
                across_lane: *a,
                across_rate: *rate,
                through_lane: *t,
            })
            .collect(),
    };
    let energy = match c.legacy().expect("built-in descriptor requested for an extension") {
        LC::Electrical | LC::Magnetic | LC::Chemical | LC::Hydraulic | LC::Acoustic => {
            product(&[(0, false, 0)])
        }
        LC::Rotational | LC::Translational => product(&[(0, true, 0)]),
        LC::Planar => product(&[(0, true, 0), (1, true, 1)]),
        LC::PlanarFrame => product(&[(3, false, 0), (4, false, 1), (5, false, 2)]),
        LC::Frame => product(&[
            (7, false, 0),
            (8, false, 1),
            (9, false, 2),
            (10, false, 3),
            (11, false, 4),
            (12, false, 5),
        ]),
        LC::Thermal => PortEnergy::Heat {
            temperature_lane: 0,
            through_lane: 0,
        },
        LC::Radiative => PortEnergy::DirectFlow { through_lane: 0 },
        LC::FluidPh => PortEnergy::DirectFlow { through_lane: 1 },
        LC::NormalizedAcoustic | LC::Granular => PortEnergy::Unavailable,
    };
    ConnectorDescriptor {
        id,
        label: c.name().into(),
        lanes,
        energy,
        rule: if c.is_owned() {
            ConnectionRule::Owned {
                contribution_rows: (c.owned_wrench_offset()..c.across_width()).collect(),
                unit_quaternions: if c == C::Frame {
                    vec![[3, 4, 5, 6]]
                } else {
                    Vec::new()
                },
            }
        } else {
            ConnectionRule::Balanced
        },
    }
}

pub fn registry() -> Result<DefinitionRegistry, DefinitionError> {
    let mut r = DefinitionRegistry::default();
    for q in QUANTITIES {
        r.register_quantity(&quantity(q.clone()))?;
    }
    let mut delta = quantity(Q::Temperature);
    delta.id = DefinitionId::new("sim.quantity.TemperatureDifference", 1);
    delta.label = "Temperature difference".into();
    delta.nature = QuantityNature::TemperatureDifference;
    delta.display_units = vec![DisplayUnit {
        symbol: "Δ°C".into(),
        scale: 1.,
        offset: 0.,
    }];
    r.register_quantity(&delta)?;
    for c in CONNECTORS {
        r.register_connector(&connector(c.clone()))?;
    }
    Ok(r)
}

/// Include anonymous legacy composites encountered in component declarations.
/// A conflicting registration is an error, even if its ID looks like a builtin.
pub fn include_connector(registry: &mut DefinitionRegistry, c: C) -> Result<(), DefinitionError> {
    if c.legacy().is_none() && c.legacy_members().is_none() {
        return registry.connector_definition(&c.definition_id()).map(|_| ()).ok_or_else(|| DefinitionError::Missing(c.definition_id()));
    }
    let descriptor = connector(c.clone());
    if let Some(existing) = registry.connector_definition(&descriptor.id) {
        if existing != &descriptor {
            return Err(DefinitionError::Duplicate(descriptor.id));
        }
        return Ok(());
    }
    if let Some(members) = c.legacy_members() {
        for member in members {
            include_connector(registry, member.clone())?;
        }
    }
    registry.register_connector(&descriptor)?;
    Ok(())
}
