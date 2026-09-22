//! One finite held pulse with exact scheduled edges, for reproducible excitation.
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, ParameterDeclaration as P,
    QuantityKind, RegistryError, StateDeclaration, View, param, signal_out,
};
use std::collections::BTreeMap;
pub const PULSE: &str = "control.pulse";
pub struct Pulse {
    pub amplitude: f64,
    pub start: f64,
    pub duration: f64,
}
impl Behavior for Pulse {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new(
            "phase",
            QuantityKind::Dimensionless,
            0.,
        )]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.state_rate(0));
        ctx.set_signal(
            0,
            if ctx.state(0) == 1. {
                self.amplitude
            } else {
                0.
            },
        );
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(if view.state(0) < 2. {
            self.start
                + if view.state(0) == 1. {
                    self.duration
                } else {
                    0.
                }
                - view.time
        } else {
            1.
        });
    }
    fn scheduled_events(&self, view: &View, out: &mut Vec<(usize, f64)>) {
        if view.state(0) < 2. {
            out.push((
                0,
                self.start
                    + if view.state(0) == 1. {
                        self.duration
                    } else {
                        0.
                    },
            ));
        }
    }
    fn jump(&mut self, _: usize, _: &View, states: &mut [f64]) {
        states[0] += 1.;
    }
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(
        BehaviorDescriptor::new(
            PULSE,
            "Finite pulse with scheduled command edges",
            vec![signal_out("value", QuantityKind::Dimensionless)],
            |p: &BTreeMap<String, f64>| {
                Ok(Box::new(Pulse {
                    amplitude: param(p, "amplitude")?,
                    start: param(p, "start")?,
                    duration: param(p, "duration")?,
                }))
            },
        )
        .with_parameters(vec![
            P::required("amplitude", "1"),
            P::required("start", "s").nonnegative(),
            P::required("duration", "s").positive(),
        ]),
    )
}
