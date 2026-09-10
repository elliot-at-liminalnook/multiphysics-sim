//! Unit displacement with cubic velocity ramps and a constant-speed middle.
//! A geometric C2 reference, not an actuator or contact model.
use sim_core::{Behavior, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind,
    RegistryError, param, signal_in, signal_out};

pub const SMOOTH_RETURN: &str = "control.smooth_return";

#[derive(Clone, Debug)]
pub struct SmoothReturn {
    ramp_fraction: f64,
}
impl SmoothReturn {
    pub fn new(ramp_fraction: f64) -> Result<Self, String> {
        if !ramp_fraction.is_finite() || ramp_fraction <= 0. || ramp_fraction > 0.5 {
            return Err("return ramp fraction must be finite and in (0, 0.5]".into());
        }
        Ok(Self { ramp_fraction })
    }
    /// Position, first derivative and second derivative with respect to unit
    /// phase. For physical duration T and displacement D, multiply by D,
    /// D/T and D/T². Peak phase rate is 1/(1-r); peak acceleration is
    /// 1.5/(r*(1-r)). Position, velocity and acceleration join continuously.
    pub fn sample(&self, phase: f64) -> Result<[f64; 3], String> {
        if !phase.is_finite() || !(0. ..=1.).contains(&phase) {
            return Err("return phase must be finite and in [0, 1]".into());
        }
        let r = self.ramp_fraction;
        let speed = 1. / (1. - r);
        let reflected = phase > 1. - r;
        let u = if reflected { 1. - phase } else { phase };
        let mut result = if u < r {
            let x = u / r;
            [speed * r * x.powi(3) * (1. - 0.5 * x),
             speed * x * x * (3. - 2. * x),
             speed * 6. * x * (1. - x) / r]
        } else {
            [speed * (u - r / 2.), speed, 0.]
        };
        if reflected { result[0] = 1. - result[0]; result[2] = -result[2]; }
        if result.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite return derivatives".into());
        }
        Ok(result)
    }
}

struct Registered(SmoothReturn);
impl Behavior for Registered {
    fn states(&self) -> Vec<sim_core::StateDeclaration> { vec![] }
    fn residual(&self, ctx: &mut Context) {
        let sample = self.0.sample(ctx.signal_in(0)).unwrap_or([f64::NAN; 3]);
        for (i, value) in sample.into_iter().enumerate() { ctx.set_signal(i, value); }
    }
}
fn make(p: &std::collections::BTreeMap<String, f64>)
    -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    Ok(Box::new(Registered(SmoothReturn::new(param(p, "ramp_fraction")?)
        .map_err(|e| sim_core::EquationError::InvalidParameter("ramp_fraction".into(), e))?)))
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(BehaviorDescriptor::new(SMOOTH_RETURN, "C2 unit return with constant-speed middle",
        vec![signal_in("phase", QuantityKind::Dimensionless),
            signal_out("progress", QuantityKind::Dimensionless),
            signal_out("phase_rate", QuantityKind::Dimensionless),
            signal_out("phase_acceleration", QuantityKind::Dimensionless)], make)
        .with_parameters(vec![sim_core::ParameterDeclaration::required("ramp_fraction", "1").positive().at_most(0.5)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn analytic_rates_area_and_c2_joins() {
        for r in [0.1, 0.25, 0.5] {
            let curve = SmoothReturn::new(r).unwrap();
            assert_eq!(curve.sample(0.).unwrap(), [0.; 3]);
            assert_eq!(curve.sample(1.).unwrap(), [1., 0., -0.]);
            let speed = 1. / (1. - r);
            assert!((curve.sample(0.5).unwrap()[0] - 0.5).abs() < 1e-15);
            assert_eq!(curve.sample(0.5).unwrap()[1], speed);
            assert!((curve.sample(r / 2.).unwrap()[2] - 1.5 * speed / r).abs() < 1e-14);
            let mut area = 0.;
            for k in 0..10000 {
                let a = curve.sample(k as f64 / 10000.).unwrap();
                let b = curve.sample((k + 1) as f64 / 10000.).unwrap();
                assert!(b[0] >= a[0]); assert!(a[1] <= speed + 1e-14);
                area += (a[1] + b[1]) / 20000.;
            }
            assert!((area - 1.).abs() < 1e-12);
            for t in [r, 1. - r] {
                let left = curve.sample(t - 1e-8).unwrap();
                let right = curve.sample(t + 1e-8).unwrap();
                assert!((left[0] - right[0]).abs() < 5e-8);
                assert!((left[1] - right[1]).abs() < 1e-12);
                assert!((left[2] - right[2]).abs() < 1e-5);
            }
            for t in [0.023, 0.071, 0.37, 0.61, 0.943] {
                let h = 1e-6;
                let a = curve.sample(t - h).unwrap(); let b = curve.sample(t).unwrap();
                let c = curve.sample(t + h).unwrap();
                assert!(((c[0] - a[0]) / (2. * h) - b[1]).abs() < 1e-8);
                assert!(((c[1] - a[1]) / (2. * h) - b[2]).abs() < 1e-8);
            }
        }
    }
    #[test]
    fn registry_and_domain_validation() {
        for r in [0., -1., 0.50001, f64::NAN, f64::INFINITY] { assert!(SmoothReturn::new(r).is_err()); }
        let curve = SmoothReturn::new(0.25).unwrap();
        for t in [-0.001, 1.001, f64::NAN, f64::INFINITY] { assert!(curve.sample(t).is_err()); }
        let mut registry = BehaviorRegistry::default(); register(&mut registry).unwrap();
        let descriptor = registry.get(&SMOOTH_RETURN.into()).unwrap();
        let p = [("ramp_fraction".into(), 0.25)].into_iter().collect();
        descriptor.validate_parameters(&p).unwrap();
        let registered = descriptor.equations.unwrap()(&p).unwrap();
        let mut outputs = [0.; 3];
        registered.residual(&mut Context::new(0., &[], &[], &[], &[], &[], &[],
            &[0.5], &mut [], &mut [], &mut outputs));
        assert_eq!(outputs, [0.5, 4. / 3., 0.]);
        assert!(descriptor.validate_parameters(&[("ramp_fraction".into(), 0.6)].into_iter().collect()).is_err());
    }
}
