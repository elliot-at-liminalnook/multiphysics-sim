//! Contact-force-weighted slip residuals. This is a control objective, not a force law.
use serde::{Deserialize, Serialize};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind, RegistryError, param,
    signal_in, signal_out,
};
pub const CONTACT_SLIP: &str = "control.contact_slip";
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContactSlipConfig {
    pub duration_s: f64,
    pub displacement_m: f64,
    pub load_threshold_n: f64,
}
#[derive(Clone, Debug)]
pub struct ContactSlip {
    config: ContactSlipConfig,
}
impl ContactSlip {
    pub fn new(config: ContactSlipConfig) -> Result<Self, String> {
        if [
            config.duration_s,
            config.displacement_m,
            config.load_threshold_n,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err(
                "slip residual requires positive finite duration, displacement and load threshold"
                    .into(),
            );
        }
        Ok(Self { config })
    }
    /// Squared residuals summed over one contact group and positive time weights
    /// form B² = T/D² integral(sum f |v_t|² / max(sum f,N0)). For weights summing
    /// to T and body path L>=D, B bounds the force-weighted loaded slip/path ratio.
    pub fn sample(
        &self,
        weight_s: f64,
        point_force_n: f64,
        group_force_n: f64,
        velocity_m_s: [f64; 2],
    ) -> Result<[f64; 2], String> {
        self.validate_sample(weight_s, point_force_n, group_force_n, velocity_m_s)?;
        let factor = ((self.config.duration_s / self.config.displacement_m)
            * (weight_s / self.config.displacement_m)
            * (point_force_n / group_force_n.max(self.config.load_threshold_n)))
        .sqrt();
        let r = velocity_m_s.map(|v| factor * v);
        if r.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite slip residual".into());
        }
        Ok(r)
    }
    /// Nonnegative mean-slip path contribution in metres. Sum over points and
    /// time, then divide by body path to obtain a continuous sufficient bound
    /// on loaded slip: actual <= mean bound <= RMS bound. This uses the same
    /// positive quadrature, with total duration T and body path >= displacement.
    /// It includes low-load motion continuously; norm/max kinks remain.
    pub fn mean_path_sample(
        &self,
        weight_s: f64,
        point_force_n: f64,
        group_force_n: f64,
        velocity_m_s: [f64; 2],
    ) -> Result<f64, String> {
        self.validate_sample(weight_s, point_force_n, group_force_n, velocity_m_s)?;
        let path = weight_s * (point_force_n / group_force_n.max(self.config.load_threshold_n))
            * velocity_m_s[0].hypot(velocity_m_s[1]);
        if !path.is_finite() {
            return Err("nonfinite mean slip path".into());
        }
        Ok(path)
    }
    fn validate_sample(
        &self,
        weight_s: f64,
        point_force_n: f64,
        group_force_n: f64,
        velocity_m_s: [f64; 2],
    ) -> Result<(), String> {
        if [
            weight_s,
            point_force_n,
            group_force_n,
            velocity_m_s[0],
            velocity_m_s[1],
        ]
        .iter()
        .any(|v| !v.is_finite())
            || weight_s <= 0.
            || weight_s > self.config.duration_s
            || point_force_n < 0.
            || group_force_n < point_force_n
        {
            return Err("slip sample requires positive bounded time weight, finite tangent velocity and consistent nonnegative normal loads".into());
        }
        Ok(())
    }
}
struct Registered(ContactSlip);
impl Behavior for Registered {
    fn states(&self) -> Vec<sim_core::StateDeclaration> {
        vec![]
    }
    fn residual(&self, ctx: &mut Context) {
        let r = self
            .0
            .sample(
                ctx.signal_in(0),
                ctx.signal_in(1),
                ctx.signal_in(2),
                [ctx.signal_in(3), ctx.signal_in(4)],
            )
            .unwrap_or([f64::NAN; 2]);
        ctx.set_signal(0, r[0]);
        ctx.set_signal(1, r[1]);
        let path = self.0.mean_path_sample(
            ctx.signal_in(0), ctx.signal_in(1), ctx.signal_in(2),
            [ctx.signal_in(3), ctx.signal_in(4)],
        ).unwrap_or(f64::NAN);
        ctx.set_signal(2, path);
    }
}
fn make(
    p: &std::collections::BTreeMap<String, f64>,
) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let config = ContactSlipConfig {
        duration_s: param(p, "duration_s")?,
        displacement_m: param(p, "displacement_m")?,
        load_threshold_n: param(p, "load_threshold_n")?,
    };
    Ok(Box::new(Registered(ContactSlip::new(config).map_err(
        |e| sim_core::EquationError::InvalidParameter("contact_slip".into(), e),
    )?)))
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(
        BehaviorDescriptor::new(
            CONTACT_SLIP,
            "Sampled loaded-slip bound residual",
            vec![
                signal_in("weight", QuantityKind::Time),
                signal_in("point_normal_force", QuantityKind::Force),
                signal_in("group_normal_force", QuantityKind::Force),
                signal_in("tangent_velocity_x", QuantityKind::LinearVelocity),
                signal_in("tangent_velocity_y", QuantityKind::LinearVelocity),
                signal_out("residual_x", QuantityKind::Dimensionless),
                signal_out("residual_y", QuantityKind::Dimensionless),
                signal_out("mean_slip_path", QuantityKind::Length),
            ],
            make,
        )
        .with_parameters(vec![
            P::required("duration_s", "s").positive(),
            P::required("displacement_m", "m").positive(),
            P::required("load_threshold_n", "N").positive(),
        ]),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn component() -> ContactSlip {
        ContactSlip::new(ContactSlipConfig {
            duration_s: 2.,
            displacement_m: 0.4,
            load_threshold_n: 1.,
        })
        .unwrap()
    }
    #[test]
    fn constant_slip_is_exact_and_unloaded_swing_contributes_zero() {
        let c = component();
        let mut squared = 0.;
        for dt in [0.3, 0.7, 1.] {
            for force in [2., 8.] {
                let r = c.sample(dt, force, 10., [0.2, 0.]).unwrap();
                squared += r[0] * r[0] + r[1] * r[1];
            }
        }
        assert!((squared - 1.).abs() < 1e-14);
        assert_eq!(c.sample(1., 0., 0., [100., -100.]).unwrap(), [0., -0.]);
        assert_eq!(c.sample(1., 10., 10., [0., 0.]).unwrap(), [0., 0.]);
    }
    #[test]
    fn weighted_bound_includes_low_load_motion_and_handles_mixed_directions() {
        let c = component();
        let mut loaded_path = 0.;
        let mut squared = 0.;
        let mut mean_path = 0.;
        for (dt, forces, velocities) in [
            (0.3, [0.1, 0.2], [[3., -2.], [1., 2.]]),
            (0.7, [2., 8.], [[0.2, 0.1], [-0.1, 0.2]]),
            (1., [4., 1.], [[0.1, -0.1], [0., 0.3]]),
        ] {
            let load = forces.iter().sum();
            for i in 0..2 {
                let r = c.sample(dt, forces[i], load, velocities[i]).unwrap();
                squared += r[0] * r[0] + r[1] * r[1];
                mean_path += c.mean_path_sample(dt, forces[i], load, velocities[i]).unwrap();
                if load >= 1. {
                    loaded_path += dt * forces[i] * velocities[i][0].hypot(velocities[i][1]) / load;
                }
            }
        }
        assert!(loaded_path < mean_path);
        assert!(mean_path / 0.4 < squared.sqrt());
        // A longer body path preserves both inequalities.
        assert!(loaded_path / 0.6 < mean_path / 0.6);
        assert!(mean_path / 0.6 < squared.sqrt());
    }
    #[test]
    fn mean_path_is_continuous_at_load_cutoff_and_exact_for_constant_slip() {
        let c = component();
        assert_eq!(c.mean_path_sample(2., 10., 10., [0.2, 0.]).unwrap(), 0.4);
        assert_eq!(c.mean_path_sample(1., 0., 0., [100., -100.]).unwrap(), 0.);
        let low = c.mean_path_sample(1., 1. - 1e-8, 1. - 1e-8, [0.3, 0.4]).unwrap();
        let at = c.mean_path_sample(1., 1., 1., [0.3, 0.4]).unwrap();
        let high = c.mean_path_sample(1., 1. + 1e-8, 1. + 1e-8, [0.3, 0.4]).unwrap();
        assert!((low - 0.5 * (1. - 1e-8)).abs() < 1e-15);
        assert_eq!(at, 0.5);
        assert_eq!(high, at);
        let mut registry = BehaviorRegistry::default();
        register(&mut registry).unwrap();
        let descriptor = registry.get(&CONTACT_SLIP.into()).unwrap();
        assert_eq!(descriptor.ports.last().unwrap().schema, signal_out("mean_slip_path", QuantityKind::Length).schema);
        let parameters = [("duration_s".into(), 2.), ("displacement_m".into(), 0.4),
            ("load_threshold_n".into(), 1.)].into_iter().collect();
        descriptor.validate_parameters(&parameters).unwrap();
        let registered = descriptor.equations.unwrap()(&parameters).unwrap();
        let mut outputs = [0.; 3];
        registered.residual(&mut Context::new(0., &[], &[], &[], &[], &[], &[],
            &[2., 10., 10., 0.2, 0.], &mut [], &mut [], &mut outputs));
        assert_eq!(outputs, [1., 0., 0.4]);
    }
    #[test]
    fn invalid_units_loads_and_nonfinite_inputs_are_rejected() {
        for value in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(
                ContactSlip::new(ContactSlipConfig {
                    duration_s: 2.,
                    displacement_m: value,
                    load_threshold_n: 1.
                })
                .is_err()
            );
        }
        let c = component();
        for (dt, p, g, v) in [
            (0., 1., 1., [0., 0.]),
            (3., 1., 1., [0., 0.]),
            (1., -1., 1., [0., 0.]),
            (1., 2., 1., [0., 0.]),
            (1., 1., 1., [f64::NAN, 0.]),
        ] {
            assert!(c.sample(dt, p, g, v).is_err());
            assert!(c.mean_path_sample(dt, p, g, v).is_err());
        }
    }
}
