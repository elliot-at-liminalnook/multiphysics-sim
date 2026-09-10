//! Net endpoint displacement, independent of robot topology and failure rules.
use serde::{Deserialize, Serialize};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, EquationError,
    ParameterDeclaration as P, QuantityKind as Q, RegistryError, param, signal_in, signal_out,
};
use std::collections::BTreeMap;

pub const NET_DISPLACEMENT: &str = "control.net_displacement";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplacementAxes {
    X,
    Y,
    Z,
    Xy,
    Xz,
    Yz,
    Xyz,
}

impl DisplacementAxes {
    pub fn mask(self) -> [bool; 3] {
        use DisplacementAxes::*;
        match self {
            X => [true, false, false],
            Y => [false, true, false],
            Z => [false, false, true],
            Xy => [true, true, false],
            Xz => [true, false, true],
            Yz => [false, true, true],
            Xyz => [true; 3],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Displacement {
    /// Full displacement in the caller's declared fixed Cartesian frame.
    pub displacement_m: [f64; 3],
    pub distance_m: f64,
    /// Gradient with respect to endpoint position; zero on excluded axes.
    /// At zero distance the norm is nondifferentiable; zero is a valid subgradient.
    pub position_gradient: [f64; 3],
}

pub fn measure(
    origin: [f64; 3],
    position: [f64; 3],
    axes: DisplacementAxes,
) -> Result<Displacement, String> {
    if origin.iter().chain(&position).any(|v| !v.is_finite()) {
        return Err("net displacement requires finite positions in one fixed frame".into());
    }
    let displacement_m = std::array::from_fn(|i| position[i] - origin[i]);
    if displacement_m.iter().any(|v| !v.is_finite()) {
        return Err("net displacement overflow".into());
    }
    let mask = axes.mask();
    let selected: [f64; 3] = std::array::from_fn(|i| if mask[i] { displacement_m[i] } else { 0. });
    let distance_m = selected[0].hypot(selected[1]).hypot(selected[2]);
    if !distance_m.is_finite() {
        return Err("net displacement distance overflow".into());
    }
    Ok(Displacement {
        displacement_m,
        distance_m,
        position_gradient: selected.map(|v| if distance_m > 0. { v / distance_m } else { 0. }),
    })
}

struct Registered(DisplacementAxes);
impl Behavior for Registered {
    fn states(&self) -> Vec<sim_core::StateDeclaration> {
        vec![]
    }
    fn residual(&self, ctx: &mut Context) {
        let origin = std::array::from_fn(|i| ctx.signal_in(i));
        let position = std::array::from_fn(|i| ctx.signal_in(3 + i));
        let sample = measure(origin, position, self.0);
        ctx.set_signal(0, sample.as_ref().map(|s| s.distance_m).unwrap_or(f64::NAN));
        for i in 0..3 {
            ctx.set_signal(
                i + 1,
                sample
                    .as_ref()
                    .map(|s| s.position_gradient[i])
                    .unwrap_or(f64::NAN),
            );
        }
    }
}
fn make(p: &BTreeMap<String, f64>) -> Result<Box<dyn Behavior>, EquationError> {
    let flags = [
        param(p, "axis_x")?,
        param(p, "axis_y")?,
        param(p, "axis_z")?,
    ];
    let invalid = || {
        EquationError::InvalidParameter("axes".into(), "select at least one axis using0 or1".into())
    };
    if flags.iter().any(|v| *v != 0. && *v != 1.) {
        return Err(invalid());
    }
    use DisplacementAxes::*;
    let axes = match flags.map(|v| v == 1.) {
        [true, false, false] => X,
        [false, true, false] => Y,
        [false, false, true] => Z,
        [true, true, false] => Xy,
        [true, false, true] => Xz,
        [false, true, true] => Yz,
        [true, true, true] => Xyz,
        _ => return Err(invalid()),
    };
    Ok(Box::new(Registered(axes)))
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(
        BehaviorDescriptor::new(
            NET_DISPLACEMENT,
            "Net endpoint displacement",
            vec![
                signal_in("origin_x", Q::Length),
                signal_in("origin_y", Q::Length),
                signal_in("origin_z", Q::Length),
                signal_in("position_x", Q::Length),
                signal_in("position_y", Q::Length),
                signal_in("position_z", Q::Length),
                signal_out("distance", Q::Length),
                signal_out("gradient_x", Q::Dimensionless),
                signal_out("gradient_y", Q::Dimensionless),
                signal_out("gradient_z", Q::Dimensionless),
            ],
            make,
        )
        .with_parameters(
            ["axis_x", "axis_y", "axis_z"]
                .map(|n| P::required(n, "1").integer(0., 1.))
                .to_vec(),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn axes_gradient_translation_and_legacy_xy_arithmetic() {
        for axes in [
            DisplacementAxes::X,
            DisplacementAxes::Y,
            DisplacementAxes::Z,
            DisplacementAxes::Xy,
            DisplacementAxes::Xz,
            DisplacementAxes::Yz,
            DisplacementAxes::Xyz,
        ] {
            let origin = [1., -2., 3.];
            let position = [4., 2., 15.];
            let s = measure(origin, position, axes).unwrap();
            assert_eq!(s.displacement_m, [3., 4., 12.]);
            for i in 0..3 {
                let mut plus = position;
                plus[i] += 1e-5;
                let mut minus = position;
                minus[i] -= 1e-5;
                let fd = (measure(origin, plus, axes).unwrap().distance_m
                    - measure(origin, minus, axes).unwrap().distance_m)
                    / 2e-5;
                assert!((fd - s.position_gradient[i]).abs() < 1e-9);
            }
            assert_eq!(s, measure([2., -1., 4.], [5., 3., 16.], axes).unwrap());
        }
        for i in -100..100 {
            let x = i as f64 * 0.13;
            let y = i as f64 * 0.017;
            assert_eq!(
                measure([0.; 3], [x, y, 0.], DisplacementAxes::Xy)
                    .unwrap()
                    .distance_m
                    .to_bits(),
                x.hypot(y).to_bits()
            );
        }
        assert_eq!(
            measure([1.; 3], [1.; 3], DisplacementAxes::Xyz)
                .unwrap()
                .position_gradient,
            [0.; 3]
        );
        assert!(measure([0.; 3], [f64::NAN, 0., 0.], DisplacementAxes::Z).is_err());
        assert!(measure([-f64::MAX, 0., 0.], [f64::MAX, 0., 0.], DisplacementAxes::X).is_err());
    }
    #[test]
    fn registry_declares_units_and_rejects_empty_or_invalid_axes() {
        let mut registry = BehaviorRegistry::default();
        register(&mut registry).unwrap();
        let d = registry.get(&NET_DISPLACEMENT.into()).unwrap();
        assert_eq!(d.ports[0], signal_in("origin_x", Q::Length));
        assert_eq!(d.ports[6], signal_out("distance", Q::Length));
        let p = [
            ("axis_x".into(), 1.),
            ("axis_y".into(), 1.),
            ("axis_z".into(), 0.),
        ]
        .into_iter()
        .collect();
        d.validate_parameters(&p).unwrap();
        let component = d.equations.unwrap()(&p).unwrap();
        let mut output = [0.; 4];
        component.residual(&mut Context::new(
            0.,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[1., 2., 3., 4., 6., 99.],
            &mut [],
            &mut [],
            &mut output,
        ));
        assert_eq!(output, [5., 0.6, 0.8, 0.]);
        for flags in [[0., 0., 0.], [1., 0.5, 0.], [1., f64::NAN, 0.]] {
            let p = ["axis_x", "axis_y", "axis_z"]
                .into_iter()
                .zip(flags)
                .map(|(n, v)| (n.into(), v))
                .collect();
            assert!((d.equations.unwrap())(&p).is_err());
        }
    }
}
