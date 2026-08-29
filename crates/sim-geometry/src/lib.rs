//! Headless descriptions for projecting committed scalar state into a 3D assembly.

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use sim_core::{ObjectId, StateError, StateId, StateStore};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum VisualMotion {
    Rotate { axis: Vec3 },
    Translate { axis: Vec3 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RestTransform {
    pub translation: Vec3,
    pub rotation: Quat,
}

impl Default for RestTransform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct VisualBinding {
    pub object: ObjectId,
    pub source: StateId,
    pub motion: VisualMotion,
    pub scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedTransform {
    pub translation: Vec3,
    pub rotation: Quat,
}

pub fn project(
    binding: VisualBinding,
    rest: RestTransform,
    state: &StateStore,
) -> Result<ProjectedTransform, StateError> {
    let value = state.get(binding.source)?;
    Ok(match binding.motion {
        VisualMotion::Rotate { axis } => ProjectedTransform {
            translation: rest.translation,
            // Reduce in f64 before converting to renderer precision. Long-running
            // shafts can accumulate millions of radians without losing visual stability.
            rotation: rest.rotation
                * Quat::from_axis_angle(
                    axis.normalize(),
                    (value * binding.scale as f64).rem_euclid(std::f64::consts::TAU) as f32,
                ),
        },
        VisualMotion::Translate { axis } => ProjectedTransform {
            translation: rest.translation + axis.normalize() * value as f32 * binding.scale,
            rotation: rest.rotation,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{ModelWorld, QuantityKind};

    #[test]
    fn translation_is_a_pure_projection_of_committed_state() {
        let mut model = ModelWorld::default();
        let object = model.add_object("carriage");
        let position = model
            .state
            .register("position", QuantityKind::Length, 0.08)
            .unwrap();
        let binding = VisualBinding {
            object,
            source: position,
            motion: VisualMotion::Translate { axis: Vec3::X },
            scale: 1.0,
        };
        let result = project(binding, RestTransform::default(), &model.state).unwrap();
        assert!((result.translation.x - 0.08).abs() < 1.0e-6);
    }

    #[test]
    fn very_large_angles_are_wrapped_before_renderer_conversion() {
        let mut model = ModelWorld::default();
        let object = model.add_object("shaft");
        let angle = model
            .state
            .register(
                "angle",
                QuantityKind::Angle,
                std::f64::consts::TAU * 1_000_000.0 + 0.25,
            )
            .unwrap();
        let binding = VisualBinding {
            object,
            source: angle,
            motion: VisualMotion::Rotate { axis: Vec3::Y },
            scale: 1.0,
        };
        let result = project(binding, RestTransform::default(), &model.state).unwrap();
        let expected = Quat::from_axis_angle(Vec3::Y, 0.25);
        assert!(result.rotation.abs_diff_eq(expected, 1.0e-5));
    }
}
