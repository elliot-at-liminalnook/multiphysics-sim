//! Compile named motion search coordinates into ordinary controller inputs.
//! This prepares immutable variants; execution/replay stay in EmbeddedEnvironment.
use crate::session::Scene;
use serde::{Deserialize, Serialize};
use sim_core::QuantityKind;
use sim_domain_control::motion_parameters::{
    ParameterSpace, Scalar, TrajectoryTemplate, Values, affine_value,
};
use sim_domain_control::trajectory::{Trajectory, TrajectoryConfig};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerScalarBinding {
    /// RFC 6901 pointer relative to controller.parameters; must already exist.
    pub pointer: String,
    pub kind: QuantityKind,
    pub reference: f64,
    #[serde(default)]
    pub integer: bool,
    pub value: Scalar,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum CheckedScalar {
    ControllerParameter {
        pointer: String,
        kind: QuantityKind,
    },
    /// Read-only scheduler period, not a writable controller cycle duration.
    ScenePeriod,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarEquality {
    pub name: String,
    pub left: CheckedScalar,
    pub right: CheckedScalar,
    /// Absolute tolerance in the common SI unit. No implicit relative tolerance.
    pub tolerance: f64,
}

fn pointer_tokens(pointer: &str) -> Result<Vec<String>, String> {
    if !pointer.starts_with('/') {
        return Err("controller scalar requires a non-root JSON pointer".into());
    }
    pointer[1..]
        .split('/')
        .map(|part| {
            let mut token = String::new();
            let mut chars = part.chars();
            while let Some(c) = chars.next() {
                if c == '~' {
                    token.push(match chars.next() {
                        Some('0') => '~',
                        Some('1') => '/',
                        _ => return Err("invalid controller scalar pointer escape".into()),
                    });
                } else {
                    token.push(c);
                }
            }
            Ok(token)
        })
        .collect()
}

fn numeric(value: &serde_json::Value) -> Result<f64, String> {
    let number = value
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or("controller scalar must be a finite number")?;
    if value
        .as_i64()
        .is_some_and(|v| v.unsigned_abs() > 1u64 << 53)
        || value.as_u64().is_some_and(|v| v > 1u64 << 53)
    {
        return Err("controller integer exceeds exact scalar range".into());
    }
    Ok(number)
}

impl CheckedScalar {
    fn read(&self, scene: &Scene) -> Result<(QuantityKind, f64), String> {
        match self {
            Self::ScenePeriod => {
                if !scene.period_s.is_finite() || scene.period_s <= 0. {
                    return Err("invalid scene period".into());
                }
                Ok((QuantityKind::Time, scene.period_s))
            }
            Self::ControllerParameter { pointer, kind } => {
                pointer_tokens(pointer)?;
                let value = scene
                    .controller
                    .as_ref()
                    .ok_or("missing checked controller")?
                    .parameters
                    .pointer(pointer)
                    .ok_or("missing checked controller scalar")?;
                Ok((*kind, numeric(value)?))
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerTrajectoryBinding {
    /// Existing top-level controller parameter containing the reference.
    pub parameter: String,
    /// Existing name -> index map in controller parameters. This makes channel
    /// order checkable without a robot-specific convention in the runtime.
    pub channel_indices_parameter: String,
    pub template: TrajectoryTemplate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandTransform {
    pub input: String,
    pub kind: QuantityKind,
    pub scale: Scalar,
    pub center: Scalar,
    pub offset: Scalar,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionParameterization {
    pub version: u32,
    pub space: ParameterSpace,
    pub trajectories: Vec<ControllerTrajectoryBinding>,
    /// Transforms are applied in list order to every held command row.
    pub commands: Vec<CommandTransform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scalars: Vec<ControllerScalarBinding>,
    /// Authored controller-format invariants, evaluated after all bindings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<ScalarEquality>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionVariant {
    pub version: u32,
    pub parameterization: MotionParameterization,
    pub values: Values,
    pub scene: Scene,
    pub actions: Vec<Vec<f64>>,
    pub source_actions: Vec<Vec<f64>>,
    /// Retain original JSON number types for exact identity and Rhai restoration.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub source_scalars: BTreeMap<String, serde_json::Value>,
}

impl MotionVariant {
    /// Reconstruct authored references and commands and verify the saved result.
    /// This detects inconsistent parameter claims, not malicious artifact edits.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported motion variant version".into());
        }
        let mut source = self.scene.clone();
        let controller = source
            .controller
            .as_mut()
            .ok_or("missing motion controller")?;
        for binding in &self.parameterization.trajectories {
            let slot = controller
                .parameters
                .get_mut(&binding.parameter)
                .ok_or("missing bound reference")?;
            *slot = serde_json::to_value(&binding.template.reference).map_err(|e| e.to_string())?;
        }
        if self.source_scalars.len() != self.parameterization.scalars.len() {
            return Err("saved motion scalar source coverage mismatch".into());
        }
        for binding in &self.parameterization.scalars {
            pointer_tokens(&binding.pointer)?;
            let original = self
                .source_scalars
                .get(&binding.pointer)
                .ok_or("missing saved motion scalar source")?;
            if numeric(original)? != binding.reference {
                return Err("saved motion scalar reference mismatch".into());
            }
            let slot = controller
                .parameters
                .pointer_mut(&binding.pointer)
                .ok_or("missing bound scalar")?;
            *slot = original.clone();
        }
        let rebuilt =
            self.parameterization
                .materialize(&source, &self.source_actions, &self.values)?;
        if rebuilt.actions != self.actions
            || crate::physics_context::fingerprint(
                &serde_json::to_value(&rebuilt.scene).map_err(|e| e.to_string())?,
            ) != crate::physics_context::fingerprint(
                &serde_json::to_value(&self.scene).map_err(|e| e.to_string())?,
            )
        {
            return Err("saved motion variant does not match parameterization".into());
        }
        Ok(())
    }
}

impl MotionParameterization {
    /// Caller-declared reference channel kinds are an authoring contract, not a
    /// static proof of how an arbitrary Rhai program interprets its parameters.
    /// Command kinds/bounds and reference index maps are checked against scene.
    pub fn materialize(
        &self,
        scene: &Scene,
        actions: &[Vec<f64>],
        values: &Values,
    ) -> Result<MotionVariant, String> {
        if self.version != 1 {
            return Err("unsupported motion parameterization version".into());
        }
        self.space.validate(values)?;
        let mut used = self
            .trajectories
            .iter()
            .flat_map(|b| b.template.parameter_names())
            .collect::<BTreeSet<_>>();
        used.extend(
            self.commands
                .iter()
                .flat_map(|b| [&b.scale, &b.center, &b.offset])
                .flat_map(Scalar::parameter_names),
        );
        used.extend(self.scalars.iter().flat_map(|b| b.value.parameter_names()));
        if used
            != self
                .space
                .parameters
                .iter()
                .map(|p| p.name.as_str())
                .collect()
        {
            return Err(
                "motion parameters must all be bound, with no undeclared references".into(),
            );
        }
        let controller = scene
            .controller
            .as_ref()
            .ok_or("motion parameterization requires a controller")?;
        if !controller.inputs.is_empty() {
            crate::forecast_actions::validate(&controller.inputs)?;
        }
        let check_actions = |rows: &[Vec<f64>]| -> Result<(), String> {
            for row in rows {
                crate::forecast_actions::validate_values(&controller.inputs, row)?;
            }
            Ok(())
        };
        check_actions(actions)?;
        if !self.commands.is_empty() && actions.is_empty() {
            return Err("command transforms require a nonempty held-action schedule".into());
        }
        let mut output = scene.clone();
        let mut bound = BTreeSet::new();
        // Validate every target against the original, before changing any value.
        // Metadata maps cannot simultaneously be trajectory write targets.
        for binding in &self.trajectories {
            if binding.parameter.is_empty()
                || !bound.insert(&binding.parameter)
                || self
                    .trajectories
                    .iter()
                    .any(|b| b.parameter == binding.channel_indices_parameter)
            {
                return Err("duplicate or overlapping controller trajectory binding".into());
            }
            let original = controller
                .parameters
                .get(&binding.parameter)
                .ok_or("missing controller trajectory parameter")?;
            let original: TrajectoryConfig =
                serde_json::from_value(original.clone()).map_err(|e| e.to_string())?;
            Trajectory::new(original.clone())?;
            if serde_json::to_value(original).map_err(|e| e.to_string())?
                != serde_json::to_value(&binding.template.reference).map_err(|e| e.to_string())?
            {
                return Err(format!(
                    "controller reference mismatch: {}",
                    binding.parameter
                ));
            }
            let map = controller
                .parameters
                .get(&binding.channel_indices_parameter)
                .and_then(|v| v.as_object())
                .ok_or("missing controller trajectory channel index map")?;
            if map.len() != binding.template.channels.len()
                || binding
                    .template
                    .channels
                    .iter()
                    .enumerate()
                    .any(|(i, c)| map.get(&c.name).and_then(|v| v.as_u64()) != Some(i as u64))
            {
                return Err("controller trajectory channel index mismatch".into());
            }
            let reference = binding.template.materialize(&self.space, values)?;
            output.controller.as_mut().unwrap().parameters[&binding.parameter] =
                serde_json::to_value(reference).map_err(|e| e.to_string())?;
        }
        let mut scalar_paths: Vec<Vec<String>> = Vec::new();
        let mut source_scalars = BTreeMap::new();
        for binding in &self.scalars {
            let path = pointer_tokens(&binding.pointer)?;
            if scalar_paths
                .iter()
                .any(|other| path.starts_with(other) || other.starts_with(&path))
                || self
                    .trajectories
                    .iter()
                    .any(|b| path[0] == b.parameter || path[0] == b.channel_indices_parameter)
            {
                return Err("duplicate or overlapping controller scalar binding".into());
            }
            scalar_paths.push(path);
            let original = controller
                .parameters
                .pointer(&binding.pointer)
                .ok_or("missing controller scalar parameter")?;
            let reference = numeric(original)?;
            if !binding.reference.is_finite() || reference != binding.reference {
                return Err(format!(
                    "controller scalar reference mismatch: {}",
                    binding.pointer
                ));
            }
            let value =
                binding
                    .value
                    .resolve(&self.space, values, binding.kind, binding.integer)?;
            // Preserve Int/Float and signed zero on identity; Rhai distinguishes
            // these number types. Changed integer outputs must be explicitly opted in.
            let replacement = if value == reference {
                original.clone()
            } else if binding.integer {
                serde_json::json!(value as i64)
            } else {
                serde_json::json!(value)
            };
            *output
                .controller
                .as_mut()
                .unwrap()
                .parameters
                .pointer_mut(&binding.pointer)
                .ok_or("missing output scalar")? = replacement;
            source_scalars.insert(binding.pointer.clone(), original.clone());
        }
        let mut check_names = BTreeSet::new();
        for check in &self.checks {
            if check.name.is_empty()
                || !check_names.insert(&check.name)
                || !check.tolerance.is_finite()
                || check.tolerance < 0.
            {
                return Err("invalid controller scalar equality declaration".into());
            }
            let (left_kind, left) = check.left.read(&output)?;
            let (right_kind, right) = check.right.read(&output)?;
            if left_kind != right_kind || (left - right).abs() > check.tolerance {
                return Err(format!("controller scalar equality failed: {}", check.name));
            }
        }
        let source_actions = actions.to_vec();
        let mut actions = actions.to_vec();
        for binding in &self.commands {
            let index = controller
                .inputs
                .iter()
                .position(|c| c.name == binding.input)
                .ok_or("unknown motion command input")?;
            if controller.inputs[index].kind != binding.kind {
                return Err("motion command quantity kind mismatch".into());
            }
            let scale =
                binding
                    .scale
                    .resolve(&self.space, values, QuantityKind::Dimensionless, false)?;
            let center = binding
                .center
                .resolve(&self.space, values, binding.kind, false)?;
            let offset = binding
                .offset
                .resolve(&self.space, values, binding.kind, false)?;
            for row in &mut actions {
                row[index] = affine_value(row[index], scale, center, offset)?;
            }
        }
        check_actions(&actions)?;
        Ok(MotionVariant {
            version: 1,
            parameterization: self.clone(),
            values: values.clone(),
            scene: output,
            actions,
            source_actions,
            source_scalars,
        })
    }

    /// Reuse native registry parameter declarations for CAD/optimizer inspectors.
    pub fn metadata(&self) -> serde_json::Value {
        serde_json::json!({"version":self.version,"parameters":self.space.declarations(),
            "bindings":self,"scope":"Motion reference and command configuration; search bounds are not physical limits. Reference units are declared by the controller author."})
    }
}
