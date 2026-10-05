//! Exact unit matching between FMU variables and the quantities block ports
//! carry: same SI base-unit exponents, factor 1, offset 0. No conversion is
//! ever applied; a mismatch is refused naming the variable and both units.
use crate::description::{ModelDescription, Variable};
use sim_core::QuantityKind as Q;

/// kg, m, s, A, K, mol, cd, rad.
type Exponents = [i32; 8];

struct Entry {
    kind: Q,
    exponents: Exponents,
    /// Unit spellings that name this quantity's canonical unit.
    spellings: &'static [&'static str],
    /// FMI/Modelica `quantity` names that select this kind among equals.
    quantities: &'static [&'static str],
}

const fn e(kg: i32, m: i32, s: i32, a: i32, k: i32, mol: i32, rad: i32) -> Exponents {
    [kg, m, s, a, k, mol, 0, rad]
}

fn table() -> Vec<Entry> {
    vec![
        Entry { kind: Q::Dimensionless, exponents: e(0, 0, 0, 0, 0, 0, 0), spellings: &["1", ""], quantities: &[] },
        Entry { kind: Q::Time, exponents: e(0, 0, 1, 0, 0, 0, 0), spellings: &["s"], quantities: &["Time"] },
        Entry { kind: Q::Voltage, exponents: e(1, 2, -3, -1, 0, 0, 0), spellings: &["V"], quantities: &["ElectricPotential", "Voltage"] },
        Entry { kind: Q::Current, exponents: e(0, 0, 0, 1, 0, 0, 0), spellings: &["A"], quantities: &["ElectricCurrent", "Current"] },
        Entry { kind: Q::Angle, exponents: e(0, 0, 0, 0, 0, 0, 1), spellings: &["rad"], quantities: &["Angle"] },
        Entry { kind: Q::AngularVelocity, exponents: e(0, 0, -1, 0, 0, 0, 1), spellings: &["rad/s"], quantities: &["AngularVelocity"] },
        Entry { kind: Q::AngularAcceleration, exponents: e(0, 0, -2, 0, 0, 0, 1), spellings: &["rad/s2", "rad/s²"], quantities: &["AngularAcceleration"] },
        Entry { kind: Q::Torque, exponents: e(1, 2, -2, 0, 0, 0, 0), spellings: &["N.m", "N·m", "Nm", "N*m"], quantities: &["Torque", "MomentOfForce"] },
        Entry { kind: Q::Energy, exponents: e(1, 2, -2, 0, 0, 0, 0), spellings: &["J"], quantities: &["Energy", "Work", "Heat"] },
        Entry { kind: Q::Length, exponents: e(0, 1, 0, 0, 0, 0, 0), spellings: &["m"], quantities: &["Length", "Position", "Distance"] },
        Entry { kind: Q::LinearVelocity, exponents: e(0, 1, -1, 0, 0, 0, 0), spellings: &["m/s"], quantities: &["Velocity"] },
        Entry { kind: Q::LinearAcceleration, exponents: e(0, 1, -2, 0, 0, 0, 0), spellings: &["m/s2", "m/s²"], quantities: &["Acceleration"] },
        Entry { kind: Q::Force, exponents: e(1, 1, -2, 0, 0, 0, 0), spellings: &["N"], quantities: &["Force"] },
        Entry { kind: Q::Impulse, exponents: e(1, 1, -1, 0, 0, 0, 0), spellings: &["N.s", "N·s"], quantities: &["Impulse", "Momentum"] },
        Entry { kind: Q::AngularImpulse, exponents: e(1, 2, -1, 0, 0, 0, 0), spellings: &["N.m.s", "N·m·s"], quantities: &["AngularMomentum", "AngularImpulse"] },
        Entry { kind: Q::Power, exponents: e(1, 2, -3, 0, 0, 0, 0), spellings: &["W"], quantities: &["Power"] },
        Entry { kind: Q::HeatFlow, exponents: e(1, 2, -3, 0, 0, 0, 0), spellings: &["W"], quantities: &["HeatFlowRate", "HeatFlow"] },
        Entry { kind: Q::Temperature, exponents: e(0, 0, 0, 0, 1, 0, 0), spellings: &["K"], quantities: &["ThermodynamicTemperature", "Temperature"] },
        Entry { kind: Q::Entropy, exponents: e(1, 2, -2, 0, -1, 0, 0), spellings: &["J/K"], quantities: &["Entropy", "HeatCapacity"] },
        Entry { kind: Q::Pressure, exponents: e(1, -1, -2, 0, 0, 0, 0), spellings: &["Pa"], quantities: &["Pressure"] },
        Entry { kind: Q::VolumeFlow, exponents: e(0, 3, -1, 0, 0, 0, 0), spellings: &["m3/s", "m³/s"], quantities: &["VolumeFlowRate"] },
        Entry { kind: Q::Frequency, exponents: e(0, 0, -1, 0, 0, 0, 0), spellings: &["Hz", "1/s"], quantities: &["Frequency"] },
        Entry { kind: Q::Mass, exponents: e(1, 0, 0, 0, 0, 0, 0), spellings: &["kg"], quantities: &["Mass"] },
        Entry { kind: Q::MassFlow, exponents: e(1, 0, -1, 0, 0, 0, 0), spellings: &["kg/s"], quantities: &["MassFlowRate"] },
        Entry { kind: Q::SpecificEnthalpy, exponents: e(0, 2, -2, 0, 0, 0, 0), spellings: &["J/kg"], quantities: &["SpecificEnthalpy", "SpecificEnergy"] },
        Entry { kind: Q::ChemicalPotential, exponents: e(1, 2, -2, 0, 0, -1, 0), spellings: &["J/mol"], quantities: &["ChemicalPotential"] },
        Entry { kind: Q::MolarFlow, exponents: e(0, 0, -1, 0, 0, 1, 0), spellings: &["mol/s"], quantities: &["MolarFlowRate"] },
        Entry { kind: Q::Radiosity, exponents: e(1, 0, -3, 0, 0, 0, 0), spellings: &["W/m2", "W/m²"], quantities: &["Radiosity", "HeatFlux"] },
        Entry { kind: Q::MagneticFlux, exponents: e(1, 2, -2, -1, 0, 0, 0), spellings: &["Wb"], quantities: &["MagneticFlux"] },
    ]
}

/// The SI exponents of a variable's unit (None: no unit declared).
fn exponents(md: &ModelDescription, v: &Variable) -> Result<Option<Exponents>, String> {
    let Some(unit) = v.unit.as_deref() else { return Ok(None) };
    if let Some(base) = md.units.get(unit) {
        if base.factor != 1.0 || base.offset != 0.0 {
            return Err(format!(
                "variable `{}` is in `{unit}` (factor {}, offset {} from SI): only SI units without conversion connect; export the FMU in SI units",
                v.name, base.factor, base.offset
            ));
        }
        return Ok(Some(base.exponents));
    }
    // Not defined in <UnitDefinitions> (the standard requires it): accept a
    // canonical SI spelling, nothing else.
    table().into_iter().find(|t| t.spellings.contains(&unit)).map(|t| Some(t.exponents))
        .ok_or_else(|| format!("variable `{}` is in `{unit}`, which the FMU's <UnitDefinitions> does not define", v.name))
}

/// The quantity a variable carries: `requested` when given (checked
/// against the variable's unit), else inferred from the unit, the unit's
/// spelling and the variable's `quantity`. Ambiguity is an error asking for
/// the kind to be stated.
pub fn kind_of(md: &ModelDescription, v: &Variable, requested: Option<&Q>) -> Result<Q, String> {
    let table = table();
    let found = exponents(md, v)?;
    if !v.value_type.is_float() {
        // Integers and Booleans carry counts, flags and modes: dimensionless.
        if v.unit.is_some() || requested.is_some_and(|k| *k != Q::Dimensionless) {
            return Err(format!("variable `{}` is {:?}: only Float32/Float64 variables carry physical quantities", v.name, v.value_type));
        }
        return Ok(Q::Dimensionless);
    }
    if let Some(kind) = requested {
        let entry = table.iter().find(|t| t.kind == *kind).ok_or_else(|| format!("variable `{}`: a port of {kind:?} cannot connect to an FMU variable (no SI unit known for it)", v.name))?;
        return match found {
            Some(x) if x == entry.exponents => Ok(kind.clone()),
            Some(_) => Err(format!("variable `{}` is in `{}`, but the port carries {} ({kind:?}): units must match exactly", v.name, v.unit.as_deref().unwrap_or(""), kind.unit())),
            None if *kind == Q::Dimensionless => Ok(Q::Dimensionless),
            None => Err(format!("variable `{}` declares no unit, but the port carries {} ({kind:?}): give the variable a unit in the FMU", v.name, kind.unit())),
        };
    }
    let Some(x) = found else { return Ok(Q::Dimensionless) };
    let candidates: Vec<&Entry> = table.iter().filter(|t| t.exponents == x).collect();
    let pick = |filter: &dyn Fn(&Entry) -> bool| -> Vec<&Entry> { candidates.iter().copied().filter(|t| filter(t)).collect() };
    let by_quantity = v.quantity.as_deref().map(|q| pick(&|t| t.quantities.contains(&q))).unwrap_or_default();
    let by_spelling = pick(&|t| v.unit.as_deref().is_some_and(|u| t.spellings.contains(&u)));
    for choice in [by_quantity, by_spelling, candidates.clone()] {
        if choice.len() == 1 {
            return Ok(choice[0].kind.clone());
        }
    }
    if candidates.is_empty() {
        return Err(format!("variable `{}` is in `{}`, which matches no quantity a port can carry", v.name, v.unit.as_deref().unwrap_or("")));
    }
    Err(format!(
        "variable `{}` is in `{}`, which several quantities share ({}): state the port's kind",
        v.name,
        v.unit.as_deref().unwrap_or(""),
        candidates.iter().map(|t| format!("{:?}", t.kind)).collect::<Vec<_>>().join(", ")
    ))
}

/// Parse a quantity name as the system file spells it (`Temperature`,
/// `HeatFlow`, …).
pub fn kind_named(name: &str) -> Option<Q> {
    table().into_iter().map(|t| t.kind).find(|k| format!("{k:?}") == name)
}
