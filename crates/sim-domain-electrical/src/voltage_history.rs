//! An imposed piecewise-linear voltage boundary, sharing ideal-source equations.
use crate::elements::VoltageSource;
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, EquationError, Input, LocalJacobian,
    Output, ParameterDeclaration as P, QuantityKind, RegistryError, StateDeclaration, View,
    acausal, param,
};
use std::collections::BTreeMap;
pub const VOLTAGE_HISTORY: &str = "electrical.voltage_history";
#[derive(Clone, Debug)]
pub struct VoltageHistory {
    pub points: Vec<[f64; 2]>,
}
impl VoltageHistory {
    pub fn from_parameters(p: &BTreeMap<String, f64>) -> Result<Self, EquationError> {
        let count = param(p, "count")?;
        if !count.is_finite() || count.fract() != 0. || !(2. ..=100_000.).contains(&count) {
            return Err(EquationError::InvalidParameter(
                "count".into(),
                "Expected 2..100000 voltage points".into(),
            ));
        }
        let n = count as usize;
        let points = (0..n)
            .map(|i| {
                Ok([
                    param(p, &format!("time.{i}"))?,
                    param(p, &format!("voltage.{i}"))?,
                ])
            })
            .collect::<Result<Vec<_>, EquationError>>()?;
        if points
            .iter()
            .any(|p| p.iter().any(|x| !x.is_finite()) || p[0] < 0.)
            || points.windows(2).any(|w| w[0][0] >= w[1][0])
            || p.keys().filter(|k| k.starts_with("time.")).count() != n
            || p.keys().filter(|k| k.starts_with("voltage.")).count() != n
        {
            return Err(EquationError::InvalidParameter(
                "time.* / voltage.*".into(),
                "Expected complete, finite points with strictly increasing nonnegative times"
                    .into(),
            ));
        }
        Ok(Self { points })
    }
    /// Explicit constant extension outside the declared point interval.
    pub fn voltage(&self, t: f64) -> f64 {
        let i = self.points.partition_point(|p| p[0] <= t);
        if i == 0 {
            return self.points[0][1];
        }
        if i == self.points.len() {
            return self.points[i - 1][1];
        }
        let (a, b) = (self.points[i - 1], self.points[i]);
        a[1] + (b[1] - a[1]) * (t - a[0]) / (b[0] - a[0])
    }
}
impl Behavior for VoltageHistory {
    fn states(&self) -> Vec<StateDeclaration> {
        let mut states = VoltageSource {
            voltage: self.points[0][1],
            controlled: false,
        }
        .states();
        states.push(StateDeclaration::new(
            "next_point",
            QuantityKind::Dimensionless,
            0.,
        ));
        states
    }
    fn residual(&self, ctx: &mut Context) {
        VoltageSource {
            voltage: self.voltage(ctx.time),
            controlled: false,
        }
        .residual(ctx);
        ctx.set_state_residual(1, ctx.state_rate(1));
    }
    fn jacobian(&self, v: &View, j: &mut LocalJacobian) -> bool {
        VoltageSource {
            voltage: self.voltage(v.time),
            controlled: false,
        }
        .jacobian(v, j);
        j.set(Output::State(1), Input::StateRate(1), 1.);
        true
    }
    fn scheduled_events(&self, v: &View, out: &mut Vec<(usize, f64)>) {
        if let Some(p) = self.points.get(v.state(1) as usize) {
            out.push((0, p[0]));
        }
    }
    fn guards(&self, v: &View, out: &mut Vec<f64>) {
        out.push(
            self.points
                .get(v.state(1) as usize)
                .map(|p| p[0] - v.time)
                .unwrap_or(1.),
        );
    }
    fn jump(&mut self, _: usize, _: &View, states: &mut [f64]) {
        states[1] += 1.;
    }
}
pub fn register(r: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    r.register(
        BehaviorDescriptor::new(
            VOLTAGE_HISTORY,
            "Imposed voltage history (linear between points, holds endpoints)",
            vec![
                acausal("p", sim_core::connectors::Electrical),
                acausal("n", sim_core::connectors::Electrical),
            ],
            |p| Ok(Box::new(VoltageHistory::from_parameters(p)?)),
        )
        .with_parameters(vec![
            P::required("count", "1").integer(2., 100_000.),
            P::required("time.*", "s").nonnegative(),
            P::required("voltage.*", "V"),
        ]),
    )
}
