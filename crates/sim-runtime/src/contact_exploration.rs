//! Offline contact-motion preparation for ordinary environment experiments.
//! Robot topology remains in the CAD scene and marker/planner configuration.
use crate::{
    contact_reference, experiment::ExperimentSpec, motion_parameters::MotionParameterization,
    session::Scene, tracking::CaptureConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_control::{
    motion_parameters::{ParameterSpace, Values},
    motion_primitives::ContactTemplate,
    trajectory::{Trajectory, TrajectoryConfig},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub version: u32,
    pub experiment: ExperimentSpec,
    pub planning_scene: Scene,
    pub markers: CaptureConfig,
    pub compiler: contact_reference::Recipe,
    pub template: ContactTemplate,
    /// Explicit operational screen, in independent-coordinate order. These
    /// bounds screen desired reference speed, not achieved speed or torque.
    pub maximum_reference_speed_rad_s: Vec<f64>,
    pub speed_limit_provenance: String,
    /// Which existing speed command follows the compiled nominal speed.
    pub forward_command: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub recipe: Recipe,
    pub values: Values,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Prepared {
    pub spec: ExperimentSpec,
    pub compiled: Value,
    pub values: Values,
    pub screen: Value,
}

impl Recipe {
    pub fn prepare(&self, values: &Values) -> Result<Prepared, String> {
        if self.version != 1 || self.speed_limit_provenance.trim().is_empty() {
            return Err(
                "version-one contact exploration and explicit screening provenance required".into(),
            );
        }
        let coordinates = &self.compiler.robot.independent_coordinates;
        let motor = self
            .experiment
            .config
            .motors
            .as_ref()
            .ok_or("explicit motor bank required")?;
        if motor.target_coordinates.as_ref() != Some(coordinates)
            || motor.effective.is_some()
            || self.planning_scene.robot.source["cad_sha256"]
                != self.experiment.scene.robot.source["cad_sha256"]
        {
            return Err(
                "contact compiler/runtime CAD and coordinate order must match detailed motors"
                    .into(),
            );
        }
        if self.maximum_reference_speed_rad_s.len() != coordinates.len()
            || self
                .maximum_reference_speed_rad_s
                .iter()
                .any(|x| !x.is_finite() || *x <= 0.)
        {
            return Err("positive finite speed limits for every coordinate required".into());
        }
        let motion = self.template.materialize(values)?;
        let mut compiler = self.compiler.clone();
        compiler.motion = motion;
        // Preserve reference audit policy/tolerances. Any inverse-load diagnostic
        // failure remains visible; it cannot become a dynamic acceptance result.
        let compiled = contact_reference::compile(
            self.planning_scene.clone(),
            self.markers.clone(),
            compiler,
        )?;
        let curve: TrajectoryConfig =
            serde_json::from_value(compiled["trajectory"].clone()).map_err(|e| e.to_string())?;
        let trajectory = Trajectory::new(curve)?;
        let variant = self.experiment.parameterization.materialize(
            &self.experiment.scene,
            &self.experiment.source_actions,
            &self.experiment.baseline,
        )?;
        let mut spec = self.experiment.clone();
        spec.scene = variant.scene;
        spec.source_actions = variant.actions;
        let controller = spec
            .scene
            .controller
            .as_mut()
            .ok_or("missing runtime controller")?;
        let params = &mut controller.parameters;
        let indices = params["motor_indices"]
            .as_object()
            .ok_or("missing motor index map")?;
        let bounds = indices
            .iter()
            .map(|(name, index)| {
                let index = index.as_u64().ok_or("invalid motor index")? as usize;
                let bound: &Value = &params["output_bounds"][name];
                Ok((
                    index,
                    (
                        Some(bound[0].as_f64().ok_or("missing lower command bound")?),
                        Some(bound[1].as_f64().ok_or("missing upper command bound")?),
                    ),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut ordered = vec![(None, None); coordinates.len()];
        let mut seen = std::collections::BTreeSet::new();
        for (i, b) in bounds {
            if i >= ordered.len() || !seen.insert(i) {
                return Err("invalid motor ordering".into());
            }
            ordered[i] = b;
        }
        if seen.len() != ordered.len() {
            return Err("incomplete motor bounds".into());
        }
        trajectory.validate_value_bounds(&ordered)?;
        let rates = trajectory.maximum_absolute_rates()?;
        for (i, (rate, limit)) in rates
            .iter()
            .zip(&self.maximum_reference_speed_rad_s)
            .enumerate()
        {
            if rate > limit {
                return Err(format!(
                    "reference actuator-speed screen: {} requests {rate} rad/s, limit {limit}",
                    coordinates[i]
                ));
            }
        }
        let old_speed = params["nominal_speed_m_s"]
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.)
            .ok_or("missing positive source speed")?;
        let speed = compiled["nominal_speed_m_s"]
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.)
            .ok_or("missing positive compiled speed")?;
        let input = controller
            .inputs
            .iter()
            .position(|i| i.name == self.forward_command)
            .ok_or("missing forward speed input")?;
        for row in &mut spec.source_actions {
            row[input] *= speed / old_speed;
            crate::forecast_actions::validate_values(&controller.inputs, row)?;
        }
        for key in [
            "motion",
            "trajectory",
            "static_feedforward",
            "dynamic_feedforward",
            "velocity_feedforward",
            "phase_offset_s",
            "pause_windows_s",
            "initial_phase_s",
            "nominal_speed_m_s",
        ] {
            if params.get(key).is_none() {
                return Err(format!("controller contract missing {key}"));
            }
            params[key] = compiled[key].clone();
        }
        params["period_s"] = compiled["motion"]["period_s"].clone();
        params["reference_load_audit"] = json!({"required_load_audits_passed":compiled["required_load_audits_passed"],"nominal":compiled["nominal_physical_summary"],"reverse":compiled["reverse_load_audit"],"scope":"Offline reference diagnostic only; runtime acceptance is separate"});
        spec.config.initial_coordinates = Some(
            serde_json::from_value(compiled["initial_coordinates"].clone())
                .map_err(|e| e.to_string())?,
        );
        spec.config.initial_base_translation_m = Some(
            serde_json::from_value(compiled["initial_base_translation_m"].clone())
                .map_err(|e| e.to_string())?,
        );
        spec.config.initial_base_rotation_vector_rad = Some(
            serde_json::from_value(compiled["initial_base_rotation_vector_rad"].clone())
                .map_err(|e| e.to_string())?,
        );
        let initial = spec.config.initial_coordinates.as_ref().unwrap();
        let servos = spec
            .config
            .motors
            .as_mut()
            .unwrap()
            .servos
            .as_mut()
            .ok_or("contact exploration requires explicit servo initial targets")?;
        if servos.len() != initial.len() {
            return Err("initial servo target ordering mismatch".into());
        }
        for (servo, angle) in servos.iter_mut().zip(initial) {
            servo.target_rad = *angle;
        }
        // The immutable generated spec contains the actual policy. Search-space
        // binding remains in this recipe; stale scalar transforms cannot apply twice.
        spec.parameterization = MotionParameterization {
            version: 1,
            space: ParameterSpace { parameters: vec![] },
            trajectories: vec![],
            commands: vec![],
            scalars: vec![],
            checks: vec![],
        };
        spec.baseline = Values::new();
        Ok(Prepared {
            spec,
            values: values.clone(),
            screen: json!({"passed":true,"reference_maximum_speed_rad_s":rates,
            "speed_limit_provenance":self.speed_limit_provenance,"scope":"CAD inverse-kinematic closure, compiler interpolation/pause checks, full-curve command bounds and reference speed limits only. No dynamic stability, continuous collision, achieved tracking or loaded motor accuracy is certified."}),
            compiled,
        })
    }
}
pub fn register(registry: &mut sim_core::BehaviorRegistry) -> Result<(), String> {
    use sim_core::primitive::{Descriptor, Field};
    registry.register_primitive(
        Descriptor::new(
            "experiment.prepare_contact_motion",
            "Compile and screen named contact motion before a dynamic trial",
            vec![Field::structured(
                "$",
                "SI; explicit parameter units",
                "contact_exploration::Request",
            )],
            vec![Field::structured(
                "$",
                "SI",
                "contact_exploration::Prepared",
            )],
            &[
                "Uses shared CAD kinematics and trajectory compiler; no simulation step",
                "Screening does not certify dynamic stability or hardware accuracy",
            ],
        ),
        |r: Request| r.recipe.prepare(&r.values),
    )
}
