//! Named, unit-checked motion search coordinates. Bounds are authored search
//! domains, never inferred actuator limits. Materialization has no mutable state.
use crate::trajectory::{Interpolation, Trajectory, TrajectoryConfig};
use serde::{Deserialize, Serialize};
use sim_core::QuantityKind as Q;
use std::collections::{BTreeMap, BTreeSet};

pub type Values = BTreeMap<String, f64>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    pub kind: Q,
    pub bounds: [f64; 2],
    #[serde(default)]
    pub integer: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterSpace {
    pub parameters: Vec<Parameter>,
}

impl ParameterSpace {
    pub fn validate(&self, values: &Values) -> Result<(), String> {
        let mut names = BTreeSet::new();
        for p in &self.parameters {
            if p.name.is_empty()
                || !names.insert(&p.name)
                || p.bounds.iter().any(|v| !v.is_finite())
                || p.bounds[0] > p.bounds[1]
                || !(p.bounds[1] - p.bounds[0]).is_finite()
                || (p.integer
                    && p.bounds
                        .iter()
                        .any(|v| v.fract() != 0. || v.abs() > (1u64 << 53) as f64))
            {
                return Err(format!("invalid motion parameter declaration {}", p.name));
            }
            let value = values
                .get(&p.name)
                .ok_or_else(|| format!("missing motion parameter {}", p.name))?;
            if !value.is_finite()
                || *value < p.bounds[0]
                || *value > p.bounds[1]
                || (p.integer && value.fract() != 0.)
            {
                return Err(format!(
                    "motion parameter {} outside declared domain ({})",
                    p.name,
                    p.kind.unit()
                ));
            }
        }
        if values.len() != names.len() {
            return Err("unknown motion parameter value".into());
        }
        Ok(())
    }

    /// Use declared order only at an optimizer boundary. Bindings use names.
    pub fn named_values(&self, coordinates: &[f64]) -> Result<Values, String> {
        if coordinates.len() != self.parameters.len() {
            return Err("motion parameter dimension mismatch".into());
        }
        let values = self
            .parameters
            .iter()
            .zip(coordinates)
            .map(|(p, v)| (p.name.clone(), *v))
            .collect();
        self.validate(&values)?;
        Ok(values)
    }

    pub fn declarations(&self) -> Vec<sim_core::ParameterDeclaration> {
        self.parameters
            .iter()
            .map(|p| {
                let mut d = sim_core::ParameterDeclaration::required(&p.name, p.kind.unit());
                d.minimum = Some(p.bounds[0]);
                d.maximum = Some(p.bounds[1]);
                d.integer = p.integer;
                d
            })
            .collect()
    }
}

/// Constants use the receiving port's SI unit; parameters must match its kind.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scalar {
    Constant {
        value: f64,
    },
    Parameter {
        name: String,
    },
    /// Preserve the value's unit while multiplying by a dimensionless factor
    /// raised to an integer power (e.g. time, inverse speed, inverse-square
    /// acceleration scaling from one shared duration coordinate).
    Scaled {
        value: Box<Scalar>,
        factor: Box<Scalar>,
        power: i32,
    },
}
impl Scalar {
    pub fn parameter_name(&self) -> Option<&str> {
        match self {
            Self::Parameter { name } => Some(name),
            _ => None,
        }
    }
    pub fn parameter_names(&self) -> BTreeSet<&str> {
        let mut pending = vec![self];
        let mut names = BTreeSet::new();
        while let Some(scalar) = pending.pop() {
            match scalar {
                Self::Parameter { name } => {
                    names.insert(name.as_str());
                }
                Self::Scaled { value, factor, .. } => {
                    pending.extend([value.as_ref(), factor.as_ref()])
                }
                Self::Constant { .. } => {}
            }
        }
        names
    }
    pub fn resolve(
        &self,
        space: &ParameterSpace,
        values: &Values,
        kind: Q,
        integer: bool,
    ) -> Result<f64, String> {
        space.validate(values)?;
        self.resolve_at(space, values, kind, integer, 0)
    }
    fn resolve_at(
        &self,
        space: &ParameterSpace,
        values: &Values,
        kind: Q,
        integer: bool,
        depth: usize,
    ) -> Result<f64, String> {
        if depth >= 64 {
            return Err("motion scalar expression exceeds 64 levels".into());
        }
        let value = match self {
            Self::Constant { value } => *value,
            Self::Parameter { name } => {
                let p = space
                    .parameters
                    .iter()
                    .find(|p| p.name == *name)
                    .ok_or_else(|| format!("unknown motion parameter {name}"))?;
                if p.kind != kind || (integer && !p.integer) {
                    return Err(format!(
                        "motion parameter {name} kind/integer mismatch: expected {kind:?}"
                    ));
                }
                *values
                    .get(name)
                    .ok_or_else(|| format!("missing motion parameter {name}"))?
            }
            Self::Scaled {
                value,
                factor,
                power,
            } => scaled_value(
                value.resolve_at(space, values, kind, false, depth + 1)?,
                factor.resolve_at(space, values, Q::Dimensionless, false, depth + 1)?,
                *power,
            )?,
        };
        if !value.is_finite()
            || (integer && (value.fract() != 0. || value.abs() > (1u64 << 53) as f64))
        {
            return Err("nonfinite or inexact integer motion scalar".into());
        }
        Ok(value)
    }
}

pub fn scaled_value(value: f64, factor: f64, power: i32) -> Result<f64, String> {
    if !value.is_finite() || !factor.is_finite() || (factor == 0. && power < 0) {
        return Err("nonfinite scalar scaling input or zero denominator".into());
    }
    let result = if factor == 1. || power == 0 {
        value
    } else {
        value * factor.powi(power)
    };
    if !result.is_finite() {
        return Err("scalar scaling overflow".into());
    }
    Ok(result)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionChannel {
    pub name: String,
    pub kind: Q,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transform {
    /// y = center + scale * (x - center) + offset, applied in list order.
    Affine {
        channels: Vec<String>,
        scale: Scalar,
        center: Scalar,
        offset: Scalar,
    },
    /// Multiply all reference times, leaving episode and controller clocks alone.
    TimeScale { factor: Scalar },
    /// Exact integer permutation; positive advances the periodic reference.
    PeriodicShift {
        channels: Vec<String>,
        controls: Scalar,
    },
    /// Change a unique control/knot. Periodic closure is maintained explicitly.
    ControlOffset {
        channels: Vec<String>,
        control: usize,
        offset: Scalar,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrajectoryTemplate {
    pub channels: Vec<MotionChannel>,
    pub reference: TrajectoryConfig,
    pub transforms: Vec<Transform>,
}

pub fn affine_value(value: f64, scale: f64, center: f64, offset: f64) -> Result<f64, String> {
    if [value, scale, center, offset]
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err("nonfinite affine motion input".into());
    }
    let value = if scale == 1. {
        value
    } else {
        center + scale * (value - center)
    };
    let value = if offset == 0. { value } else { value + offset };
    if !value.is_finite() {
        return Err("affine motion overflow".into());
    }
    Ok(value)
}

impl TrajectoryTemplate {
    pub fn parameter_names(&self) -> BTreeSet<&str> {
        self.transforms
            .iter()
            .flat_map(|t| match t {
                Transform::Affine {
                    scale,
                    center,
                    offset,
                    ..
                } => vec![scale, center, offset],
                Transform::TimeScale { factor } => vec![factor],
                Transform::PeriodicShift { controls, .. } => vec![controls],
                Transform::ControlOffset { offset, .. } => vec![offset],
            })
            .flat_map(Scalar::parameter_names)
            .collect()
    }
    pub fn materialize(
        &self,
        space: &ParameterSpace,
        values: &Values,
    ) -> Result<TrajectoryConfig, String> {
        space.validate(values)?;
        let reference = Trajectory::new(self.reference.clone())?;
        let mut names = BTreeSet::new();
        if self.channels.len() != reference.dimension()
            || self
                .channels
                .iter()
                .any(|c| c.name.is_empty() || !names.insert(&c.name))
        {
            return Err("trajectory requires one unique named channel per coordinate".into());
        }
        let indices = |selected: &[String]| -> Result<Vec<usize>, String> {
            let mut seen = BTreeSet::new();
            if selected.is_empty() {
                return Err("empty motion transform channel selection".into());
            }
            selected
                .iter()
                .map(|name| {
                    if !seen.insert(name) {
                        return Err(format!("duplicate motion channel {name}"));
                    }
                    self.channels
                        .iter()
                        .position(|c| c.name == *name)
                        .ok_or_else(|| format!("unknown motion channel {name}"))
                })
                .collect()
        };
        let mut config = self.reference.clone();
        for transform in &self.transforms {
            match transform {
                Transform::Affine {
                    channels,
                    scale,
                    center,
                    offset,
                } => {
                    let scale = scale.resolve(space, values, Q::Dimensionless, false)?;
                    for j in indices(channels)? {
                        let center = center.resolve(space, values, self.channels[j].kind, false)?;
                        let offset = offset.resolve(space, values, self.channels[j].kind, false)?;
                        for k in &mut config.keyframes {
                            k.values[j] = affine_value(k.values[j], scale, center, offset)?;
                        }
                    }
                }
                Transform::TimeScale { factor } => {
                    let factor = factor.resolve(space, values, Q::Dimensionless, false)?;
                    if factor <= 0. {
                        return Err("reference time scale must be positive".into());
                    }
                    if factor != 1. {
                        for k in &mut config.keyframes {
                            k.time_s *= factor;
                        }
                    }
                }
                Transform::PeriodicShift { channels, controls } => {
                    let controls = controls.resolve(space, values, Q::Dimensionless, true)? as i64;
                    let mut shifts = vec![0; self.channels.len()];
                    for j in indices(channels)? {
                        shifts[j] = controls;
                    }
                    config = Trajectory::new(config)?.shifted_periodic_controls(&shifts)?;
                }
                Transform::ControlOffset {
                    channels,
                    control,
                    offset,
                } => {
                    let periodic =
                        matches!(config.interpolation, Interpolation::PeriodicCubicBSpline);
                    let count = config.keyframes.len() - usize::from(periodic);
                    if *control >= count {
                        return Err("motion control index outside unique controls".into());
                    }
                    for j in indices(channels)? {
                        let offset = offset.resolve(space, values, self.channels[j].kind, false)?;
                        config.keyframes[*control].values[j] =
                            affine_value(config.keyframes[*control].values[j], 1., 0., offset)?;
                    }
                    if periodic && *control == 0 {
                        config.keyframes.last_mut().unwrap().values =
                            config.keyframes[0].values.clone();
                    }
                }
            }
            Trajectory::new(config.clone())?;
        }
        Ok(config)
    }
}

struct Affine;
impl sim_core::Behavior for Affine {
    fn states(&self) -> Vec<sim_core::StateDeclaration> {
        vec![]
    }
    fn residual(&self, ctx: &mut sim_core::Context) {
        let value = affine_value(
            ctx.signal_in(0),
            ctx.signal_in(1),
            ctx.signal_in(2),
            ctx.signal_in(3),
        );
        ctx.set_signal(0, value.unwrap_or(f64::NAN));
    }
}
fn make(_: &Values) -> Result<Box<dyn sim_core::Behavior>, sim_core::EquationError> {
    Ok(Box::new(Affine))
}

struct ScalePower {
    power: i32,
}
impl sim_core::Behavior for ScalePower {
    fn states(&self) -> Vec<sim_core::StateDeclaration> {
        vec![]
    }
    fn residual(&self, ctx: &mut sim_core::Context) {
        let result = scaled_value(ctx.signal_in(0), ctx.signal_in(1), self.power);
        ctx.set_signal(0, result.unwrap_or(f64::NAN));
    }
}
fn make_scale_power(
    parameters: &Values,
) -> Result<Box<dyn sim_core::Behavior>, sim_core::EquationError> {
    let power = parameters
        .get("power")
        .copied()
        .ok_or_else(|| sim_core::EquationError::MissingParameter("power".into()))?;
    if !power.is_finite()
        || power.fract() != 0.
        || power < i32::MIN as f64
        || power > i32::MAX as f64
    {
        return Err(sim_core::EquationError::InvalidParameter(
            "power".into(),
            "expected an i32 integer".into(),
        ));
    }
    Ok(Box::new(ScalePower {
        power: power as i32,
    }))
}

/// Typed signal ports share the same scalar algebra as offline materialization.
pub fn register(registry: &mut sim_core::BehaviorRegistry) -> Result<(), sim_core::RegistryError> {
    use sim_core::{BehaviorDescriptor, signal_in, signal_out};
    for (name, kind) in [
        ("angle", Q::Angle),
        ("length", Q::Length),
        ("angular_velocity", Q::AngularVelocity),
        ("linear_velocity", Q::LinearVelocity),
        ("angular_acceleration", Q::AngularAcceleration),
        ("linear_acceleration", Q::LinearAcceleration),
        ("time", Q::Time),
        ("dimensionless", Q::Dimensionless),
    ] {
        registry.register(
            BehaviorDescriptor::new(
                &format!("control.affine_{name}"),
                "Affine motion reference",
                vec![
                    signal_in("value", kind),
                    signal_in("scale", Q::Dimensionless),
                    signal_in("center", kind),
                    signal_in("offset", kind),
                    signal_out("result", kind),
                ],
                make,
            )
            .with_parameters(vec![]),
        )?;
        let mut power = sim_core::ParameterDeclaration::required("power", Q::Dimensionless.unit());
        power.minimum = Some(i32::MIN as f64);
        power.maximum = Some(i32::MAX as f64);
        power.integer = true;
        registry.register(
            BehaviorDescriptor::new(
                &format!("control.scale_power_{name}"),
                "Scale a motion quantity by a dimensionless integer power",
                vec![
                    signal_in("value", kind),
                    signal_in("factor", Q::Dimensionless),
                    signal_out("result", kind),
                ],
                make_scale_power,
            )
            .with_parameters(vec![power]),
        )?;
    }
    Ok(())
}
