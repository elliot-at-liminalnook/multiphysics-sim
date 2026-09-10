//! State-dependent planning contact, following IDTO equations (3)-(6).
//! This smooth approximation permits force at positive separation. It is not
//! an implicit replacement for a robot's authored simulation contact model.
use serde::{Deserialize, Serialize};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, EquationError, Input, LocalJacobian,
    Output, QuantityKind, RegistryError, View, param, signal_in, signal_out,
};

pub const SMOOTH_CONTACT: &str = "contact.smooth_planning_force";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SmoothContactConfig {
    pub stiffness_n_m: f64,
    pub smoothing_m: f64,
    pub dissipation_velocity_m_s: f64,
    pub friction_coefficient: f64,
    pub stiction_velocity_m_s: f64,
}

#[derive(Clone, Debug)]
pub struct SmoothContact {
    config: SmoothContactConfig,
}

#[derive(Clone, Debug, Serialize)]
pub struct SmoothContactSample {
    /// Normal, tangent 1, tangent 2 in an orthonormal contact frame.
    pub force_n: [f64; 3],
    /// Rows: the three forces. Columns: gap, normal velocity, tangent velocities.
    pub derivative: [[f64; 4]; 3],
}

impl SmoothContact {
    pub fn new(config: SmoothContactConfig) -> Result<Self, String> {
        if [
            config.stiffness_n_m,
            config.smoothing_m,
            config.dissipation_velocity_m_s,
            config.stiction_velocity_m_s,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.)
            || !config.friction_coefficient.is_finite()
            || config.friction_coefficient < 0.
        {
            return Err("positive finite contact scales and nonnegative friction required".into());
        }
        Ok(Self { config })
    }

    /// Positive gap separates surfaces; positive normal velocity separates them.
    pub fn sample(
        &self,
        gap_m: f64,
        velocity_m_s: [f64; 3],
    ) -> Result<SmoothContactSample, String> {
        if !gap_m.is_finite() || velocity_m_s.iter().any(|v| !v.is_finite()) {
            return Err("finite gap and contact velocity required".into());
        }
        let p = &self.config;
        let x = -gap_m / p.smoothing_m;
        // Stable softplus and sigmoid, including large signed distances.
        let compliance = if x >= 0. {
            p.stiffness_n_m * (-gap_m + p.smoothing_m * (-x).exp().ln_1p())
        } else {
            p.stiffness_n_m * p.smoothing_m * x.exp().ln_1p()
        };
        let sigmoid = if x >= 0. {
            1. / (1. + (-x).exp())
        } else {
            let e = x.exp();
            e / (1. + e)
        };
        let z = velocity_m_s[0] / p.dissipation_velocity_m_s;
        let (dissipation, derivative) = if z < 0. {
            (1. - z, -1. / p.dissipation_velocity_m_s)
        } else if z < 2. {
            (
                (z - 2.).powi(2) / 4.,
                (z - 2.) / (2. * p.dissipation_velocity_m_s),
            )
        } else {
            (0., 0.)
        };
        let normal = compliance * dissipation;
        let dn = [
            -p.stiffness_n_m * sigmoid * dissipation,
            compliance * derivative,
        ];
        let tangent = [velocity_m_s[1], velocity_m_s[2]];
        let radius = p.stiction_velocity_m_s.hypot(tangent[0]).hypot(tangent[1]);
        let unit = tangent.map(|v| v / radius);
        let mut out = SmoothContactSample {
            force_n: [
                normal,
                -p.friction_coefficient * normal * unit[0],
                -p.friction_coefficient * normal * unit[1],
            ],
            derivative: [[0.; 4]; 3],
        };
        out.derivative[0][..2].copy_from_slice(&dn);
        for i in 0..2 {
            for j in 0..2 {
                out.derivative[1 + i][j] = -p.friction_coefficient * unit[i] * dn[j];
            }
            for j in 0..2 {
                out.derivative[1 + i][2 + j] = -p.friction_coefficient
                    * normal
                    * (if i == j { 1. } else { 0. } - unit[i] * unit[j])
                    / radius;
            }
        }
        if out
            .force_n
            .iter()
            .chain(out.derivative.iter().flatten())
            .any(|v| !v.is_finite())
        {
            return Err("nonfinite smooth contact force or derivative".into());
        }
        Ok(out)
    }
}

impl Behavior for SmoothContact {
    fn states(&self) -> Vec<sim_core::StateDeclaration> {
        vec![]
    }
    fn residual(&self, ctx: &mut Context) {
        let f = self
            .sample(
                ctx.signal_in(0),
                [ctx.signal_in(1), ctx.signal_in(2), ctx.signal_in(3)],
            )
            .map(|s| s.force_n)
            .unwrap_or([f64::NAN; 3]);
        for (i, value) in f.into_iter().enumerate() {
            ctx.set_signal(i, value);
        }
    }
    fn jacobian(&self, view: &View, out: &mut LocalJacobian) -> bool {
        let Ok(sample) = self.sample(
            view.signal_in(0),
            [view.signal_in(1), view.signal_in(2), view.signal_in(3)],
        ) else {
            return false;
        };
        for i in 0..3 {
            for j in 0..4 {
                out.set(Output::Signal(i), Input::Signal(j), sample.derivative[i][j]);
            }
        }
        true
    }
}

fn make(p: &std::collections::BTreeMap<String, f64>) -> Result<Box<dyn Behavior>, EquationError> {
    let config = SmoothContactConfig {
        stiffness_n_m: param(p, "stiffness_n_m")?,
        smoothing_m: param(p, "smoothing_m")?,
        dissipation_velocity_m_s: param(p, "dissipation_velocity_m_s")?,
        friction_coefficient: param(p, "friction_coefficient")?,
        stiction_velocity_m_s: param(p, "stiction_velocity_m_s")?,
    };
    Ok(Box::new(SmoothContact::new(config).map_err(|e| {
        EquationError::InvalidParameter("smooth_contact".into(), e)
    })?))
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(
        BehaviorDescriptor::new(
            SMOOTH_CONTACT,
            "Smooth planning contact force (allows force at separation)",
            vec![
                signal_in("gap", QuantityKind::Length),
                signal_in("normal_velocity", QuantityKind::LinearVelocity),
                signal_in("tangent_velocity_1", QuantityKind::LinearVelocity),
                signal_in("tangent_velocity_2", QuantityKind::LinearVelocity),
                signal_out("normal_force", QuantityKind::Force),
                signal_out("tangent_force_1", QuantityKind::Force),
                signal_out("tangent_force_2", QuantityKind::Force),
            ],
            make,
        )
        .with_parameters(vec![
            P::required("stiffness_n_m", "N/m").positive(),
            P::required("smoothing_m", "m").positive(),
            P::required("dissipation_velocity_m_s", "m/s").positive(),
            P::required("friction_coefficient", "1").nonnegative(),
            P::required("stiction_velocity_m_s", "m/s").positive(),
        ]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn law() -> SmoothContact {
        SmoothContact::new(SmoothContactConfig {
            stiffness_n_m: 2000.,
            smoothing_m: 0.001,
            dissipation_velocity_m_s: 0.2,
            friction_coefficient: 0.3,
            stiction_velocity_m_s: 0.01,
        })
        .unwrap()
    }
    #[test]
    fn unilateral_dissipative_and_stable_away_from_contact() {
        let c = law();
        assert!((c.sample(-1., [0.; 3]).unwrap().force_n[0] - 2000.).abs() < 1e-12);
        assert_eq!(c.sample(1., [0.; 3]).unwrap().force_n, [0.; 3]);
        assert!(c.sample(0.001, [0.; 3]).unwrap().force_n[0] > 0.); // explicit force-at-distance approximation
        assert_eq!(c.sample(-0.1, [0.4, 1., 1.]).unwrap().force_n, [0.; 3]);
        for gap in [-0.02, 0., 0.01] {
            for vn in [-1., 0., 0.2, 0.4, 1.] {
                let f = c.sample(gap, [vn, 0.7, -0.9]).unwrap().force_n;
                assert!(f[0] >= 0. && f[1].hypot(f[2]) <= 0.3 * f[0] + 1e-12);
                assert!(f[1] * 0.7 - f[2] * 0.9 <= 0.);
            }
        }
        assert!(c.sample(f64::NAN, [0.; 3]).is_err());
    }
    #[test]
    fn analytic_derivatives_match_difference_including_dissipation_joins() {
        let c = law();
        for vn in [-0.03, 0., 0.07, 0.4, 0.5] {
            let x = [-0.0003, vn, 0.02, -0.04];
            let s = c.sample(x[0], [x[1], x[2], x[3]]).unwrap();
            for j in 0..4 {
                let h = 1e-7;
                let mut a = x;
                let mut b = x;
                a[j] += h;
                b[j] -= h;
                let a = c.sample(a[0], [a[1], a[2], a[3]]).unwrap();
                let b = c.sample(b[0], [b[1], b[2], b[3]]).unwrap();
                for i in 0..3 {
                    let fd = (a.force_n[i] - b.force_n[i]) / (2. * h);
                    assert!(
                        (fd - s.derivative[i][j]).abs() < 1e-5 * (1. + fd.abs()),
                        "row {i},col {j}: {fd} vs {}",
                        s.derivative[i][j]
                    );
                }
            }
        }
    }
}
