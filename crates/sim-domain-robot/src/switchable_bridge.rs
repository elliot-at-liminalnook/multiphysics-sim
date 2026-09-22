//! Disabled drive leaves a passive leakage/diode path instead of shorting the motor.
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind as Q, RegistryError,
    StateDeclaration, acausal, param_or, signal_in,
};
use std::collections::BTreeMap;

pub const SWITCHABLE_H_BRIDGE: &str = "robot.switchable_h_bridge";
struct SwitchableBridge {
    driven: Box<dyn Behavior>,
    off_conductance: f64,
    diode_drop: f64,
    diode_resistance: f64,
}
impl Behavior for SwitchableBridge {
    fn states(&self) -> Vec<StateDeclaration> {
        self.driven.states()
    }
    fn residual(&self, ctx: &mut Context) {
        if ctx.signal_in(1) >= 0.5 {
            self.driven.residual(ctx);
            return;
        }
        let voltage = ctx.across(2) - ctx.across(3);
        let rail = (ctx.across(0) - ctx.across(1)).max(0.);
        let excess = (voltage.abs() - rail - 2. * self.diode_drop).max(0.);
        let diode_current = voltage.signum() * excess / self.diode_resistance;
        let current = ctx.state(0);
        ctx.set_state_residual(0, current - self.off_conductance * voltage - diode_current);
        ctx.add_through(2, current);
        ctx.add_through(3, -current);
        // Either polarity returns diode current to the positive supply rail.
        ctx.add_through(0, -diode_current.abs());
        ctx.add_through(1, diode_current.abs());
    }
}
fn build(p: &BTreeMap<String, f64>) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    Ok(Box::new(SwitchableBridge {
        driven: super::motor::h_bridge(p)?,
        off_conductance: param_or(p, "off_conductance", 1e-8),
        diode_drop: param_or(p, "diode_drop", 0.7),
        diode_resistance: param_or(p, "diode_resistance", 0.1),
    }))
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(
        BehaviorDescriptor::new(
            SWITCHABLE_H_BRIDGE,
            "Averaged H-bridge with enable and passive flyback paths",
            vec![
                acausal("supply_p", sim_core::connectors::Electrical),
                acausal("supply_n", sim_core::connectors::Electrical),
                acausal("p", sim_core::connectors::Electrical),
                acausal("n", sim_core::connectors::Electrical),
                signal_in("command", Q::Dimensionless),
                signal_in("enabled", Q::Dimensionless),
            ],
            build,
        )
        .with_parameters(vec![
            P::optional("on_resistance", "Ω", 0.1).nonnegative(),
            P::optional("current_limit", "A", f64::INFINITY).positive(),
            P::optional("off_conductance", "S", 1e-8).nonnegative(),
            P::optional("diode_drop", "V", 0.7).nonnegative(),
            P::optional("diode_resistance", "Ω", 0.1).positive(),
        ]),
    )
}
