//! Explicit contact-formulation choices, independent of CAD material values.

/// Compatible history states decay when the contact law does not use stored
/// tangential deformation (or when a bristle patch is airborne).
pub(super) fn inactive_history_rate(value: f64) -> f64 {
    -200.0 * value
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FloorFrictionModel {
    #[default]
    Bristle,
    /// Experimental kinetic-Coulomb patch with smooth slip regularization.
    /// No static stiction or stored tangential elastic energy; small loads creep.
    RegularizedCoulomb { slip_speed_m_s: f64 },
}
impl FloorFrictionModel {
    pub fn is_bristle(&self) -> bool {
        matches!(self, Self::Bristle)
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Self::RegularizedCoulomb { slip_speed_m_s } = self {
            if !slip_speed_m_s.is_finite() || *slip_speed_m_s <= 0.0 {
                return Err(
                    "regularized floor friction requires positive finite slip_speed_m_s".into(),
                );
            }
        }
        Ok(())
    }
    /// Scalar registry encoding: zero selects the original bristle law.
    pub fn registry_speed(&self) -> f64 {
        match *self {
            Self::Bristle => 0.0,
            Self::RegularizedCoulomb { slip_speed_m_s } => slip_speed_m_s,
        }
    }
    pub fn from_registry_speed(speed: f64) -> Result<Self, String> {
        let result = if speed == 0.0 {
            Self::Bristle
        } else {
            Self::RegularizedCoulomb {
                slip_speed_m_s: speed,
            }
        };
        result.validate()?;
        Ok(result)
    }
}
