//! Composable motion configuration and bounded references. No robot topology,
//! gait selector, contact physics or execution loop lives here.
use crate::{
    contact_phase::{ContactPhaseConfig, ContactPhaseMotion, FootPhase},
    motion_parameters::{ParameterSpace, Scalar, TrajectoryTemplate, Values},
    reference_governor,
};
use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorRegistry, QuantityKind as Q,
    primitive::{Descriptor, Field as F},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootTemplate {
    pub center_world_m: [Scalar; 3],
    pub phase_offset: Scalar,
    pub stance_fraction: Scalar,
    pub swing_offset_world_m: [Scalar; 3],
    pub return_ramp_fraction: Option<Scalar>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactTemplate {
    pub space: ParameterSpace,
    pub body: TrajectoryTemplate,
    pub period_s: Scalar,
    pub displacement_world_m: [Scalar; 3],
    pub feet: Vec<FootTemplate>,
}
impl ContactTemplate {
    /// Declarative materialization only. Does not ask an optimizer for values.
    pub fn materialize(&self, values: &Values) -> Result<ContactPhaseConfig, String> {
        self.space.validate(values)?;
        if self.body.channels.len() != 6
            || self
                .body
                .channels
                .iter()
                .enumerate()
                .any(|(i, c)| c.kind != if i < 3 { Q::Length } else { Q::Angle })
        {
            return Err(
                "body trajectory requires xyz position then xyz rotation-vector channels".into(),
            );
        }
        let scalar = |s: &Scalar, q: Q| s.resolve(&self.space, values, q, false);
        let xyz = |s: &[Scalar; 3]| -> Result<[f64; 3], String> {
            Ok([
                scalar(&s[0], Q::Length)?,
                scalar(&s[1], Q::Length)?,
                scalar(&s[2], Q::Length)?,
            ])
        };
        let motion = ContactPhaseConfig {
            period_s: scalar(&self.period_s, Q::Time)?,
            displacement_world_m: xyz(&self.displacement_world_m)?,
            body: self.body.materialize(&self.space, values)?,
            feet: self
                .feet
                .iter()
                .map(|f| {
                    Ok(FootPhase {
                        center_world_m: xyz(&f.center_world_m)?,
                        phase_offset: scalar(&f.phase_offset, Q::Dimensionless)?,
                        stance_fraction: scalar(&f.stance_fraction, Q::Dimensionless)?,
                        swing_offset_world_m: xyz(&f.swing_offset_world_m)?,
                        return_ramp_fraction: f
                            .return_ramp_fraction
                            .as_ref()
                            .map(|s| scalar(s, Q::Dimensionless))
                            .transpose()?,
                        additional_steps: vec![],
                    })
                })
                .collect::<Result<_, String>>()?,
        };
        ContactPhaseMotion::new(motion.clone())?;
        Ok(motion)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Materialize {
    pub template: ContactTemplate,
    pub values: Values,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    pub motion: ContactPhaseConfig,
    pub time_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AngularChannel {
    pub name: String,
    pub bounds_rad: [f64; 2],
    pub governor: reference_governor::Config,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Govern {
    pub channels: Vec<AngularChannel>,
    pub states: BTreeMap<String, reference_governor::State>,
    pub requested_rad: BTreeMap<String, f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Governed {
    pub states: BTreeMap<String, reference_governor::State>,
    pub requested_rad: BTreeMap<String, f64>,
    pub target_clipped: Vec<String>,
}
/// All channels succeed or no caller state changes. There is no inferred state
/// initialization: the caller supplies actual starting angle and velocity.
pub fn govern(r: Govern) -> Result<Governed, String> {
    let mut names = BTreeSet::new();
    let mut states = BTreeMap::new();
    let mut clipped = vec![];
    if r.channels.is_empty()
        || r.channels.len() != r.states.len()
        || r.channels.len() != r.requested_rad.len()
    {
        return Err("matching nonempty angular channel/state/request sets required".into());
    }
    for c in &r.channels {
        if c.name.is_empty()
            || !names.insert(&c.name)
            || c.bounds_rad.iter().any(|v| !v.is_finite())
            || c.bounds_rad[0] >= c.bounds_rad[1]
        {
            return Err("unique named channels with finite ordered angle bounds required".into());
        }
        let prior = *r.states.get(&c.name).ok_or("missing angular state")?;
        let request = *r
            .requested_rad
            .get(&c.name)
            .ok_or("missing angular target")?;
        if !request.is_finite()
            || prior.angle_rad < c.bounds_rad[0]
            || prior.angle_rad > c.bounds_rad[1]
        {
            return Err("finite target and initial state within angle limits required".into());
        }
        let target = request.clamp(c.bounds_rad[0], c.bounds_rad[1]);
        if target != request {
            clipped.push(c.name.clone());
        }
        let next = c.governor.update(prior, target)?;
        // A nonzero initial outward velocity can make simultaneous bounds
        // infeasible. Reject rather than hiding an acceleration discontinuity.
        if next.angle_rad < c.bounds_rad[0] || next.angle_rad > c.bounds_rad[1] {
            return Err("governed state would cross authored angle bound".into());
        }
        states.insert(c.name.clone(), next);
    }
    Ok(Governed {
        states,
        requested_rad: r.requested_rad,
        target_clipped: clipped,
    })
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), String> {
    crate::adaptive_braking::register(registry)?;
    registry.register_primitive(Descriptor::new("motion.contact_template","Unit-checked independent contact phases and smooth paths",
        vec![F::structured("template","SI; units declared by ParameterSpace and trajectory channels","ContactTemplate"),F::structured("values","declared parameter SI units","named scalars")],
        vec![F::structured("$","s,m,rad; phase fractions dimensionless","ContactPhaseConfig")],
        &["Any number of independently phased contacts; no balance or actuator feasibility is inferred", "Body period and motion period must agree explicitly"]),|r:Materialize|r.template.materialize(&r.values))?;
    registry.register_primitive(
        Descriptor::new(
            "motion.contact_sample",
            "Sample existing smooth body/foot references",
            vec![
                F::structured(
                    "motion",
                    "s,m,rad; dimensionless phase fractions",
                    "ContactPhaseConfig",
                ),
                F::quantity("time_s", Q::Time, "scalar"),
            ],
            vec![
                F::structured(
                    "body",
                    "m,rad; first and second time derivatives",
                    "TrajectorySample",
                ),
                F::structured(
                    "feet",
                    "m,m/s,m/s²; contact boolean and phase fraction",
                    "FootSample array",
                ),
            ],
            &["Reference geometry only; stance labels do not establish actual support"],
        ),
        |r: Sample| ContactPhaseMotion::new(r.motion)?.sample(r.time_s),
    )?;
    registry.register_primitive(
        Descriptor::new(
            "motion.govern_angles",
            "Transactional bank of existing bounded reference governors",
            vec![
                F::structured(
                    "channels",
                    "rad; governor s,rad/s,rad/s²,1/s",
                    "AngularChannel array",
                ),
                F::structured("states", "rad,rad/s", "named governor states"),
                F::quantity("requested_rad", Q::Angle, "named scalars"),
            ],
            vec![
                F::structured("states", "rad,rad/s", "named governor states"),
                F::quantity("requested_rad", Q::Angle, "named scalars"),
                F::structured("target_clipped", "1", "channel name array"),
            ],
            &[
                "Conditions requested motion; does not improve tracking of an unconditioned signal",
                "Each channel retains its explicitly authored sampling period",
            ],
        ),
        govern,
    )
}
