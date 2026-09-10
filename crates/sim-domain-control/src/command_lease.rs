//! Sampled command freshness. Hosts send increasing packet sequence numbers;
//! unchanged or out-of-order packets cannot keep a motion request alive.
use serde::{Deserialize, Serialize};
use sim_core::{Behavior, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind,
    RegistryError, StateDeclaration, View, param, signal_in, signal_out};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandLeaseConfig { pub period_s: f64, pub timeout_s: f64 }
#[derive(Clone, Debug)]
pub struct CommandLease { config: CommandLeaseConfig }
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct CommandLeaseState { pub sequence: f64, pub age_s: f64, pub fresh: bool }
impl CommandLease {
    pub fn new(config: CommandLeaseConfig) -> Result<Self, String> {
        if !config.period_s.is_finite() || config.period_s <= 0. || !config.timeout_s.is_finite()
            || config.timeout_s < config.period_s {
            return Err("command lease requires finite positive period and timeout >= period".into());
        }
        Ok(Self { config })
    }
    pub fn initial(&self) -> CommandLeaseState {
        CommandLeaseState { sequence: -1., age_s: self.config.timeout_s, fresh: false }
    }
    /// Call at every controller deadline, including when no new packet arrives.
    /// Expiry requests a normal controlled stop; this is not an emergency disable.
    /// The hardware host must run deadlines independently of transport/rendering.
    pub fn update(&self, sequence: f64, age_s: f64, received: f64) -> Result<CommandLeaseState, String> {
        let valid = |x: f64, min: f64| x.is_finite() && x >= min && x <= 9_007_199_254_740_991. && x.fract() == 0.;
        if !valid(sequence, -1.) || !valid(received, 0.) || !age_s.is_finite() || age_s < 0. || age_s > self.config.timeout_s {
            return Err("command lease requires exact integer sequence numbers and bounded finite age".into());
        }
        let (sequence, age_s) = if received > sequence { (received, 0.) }
            else { (sequence, (age_s + self.config.period_s).min(self.config.timeout_s)) };
        Ok(CommandLeaseState { sequence, age_s, fresh: age_s < self.config.timeout_s })
    }
}
struct Registered(CommandLease);
impl Behavior for Registered {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("samples", QuantityKind::Dimensionless, 0.),
            StateDeclaration::new("sequence", QuantityKind::Dimensionless, -1.),
            StateDeclaration::new("age", QuantityKind::Time, self.0.config.timeout_s)]
    }
    fn residual(&self, ctx: &mut Context) {
        for i in 0..3 { ctx.set_state_residual(i, ctx.state_rate(i)); }
        ctx.set_signal(0, if ctx.state(2) < self.0.config.timeout_s { 1. } else { 0. });
        ctx.set_signal(1, ctx.state(2));
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) { out.push(view.state(0)*self.0.config.period_s-view.time); }
    fn scheduled_events(&self, view: &View, out: &mut Vec<(usize,f64)>) { out.push((0,view.state(0)*self.0.config.period_s)); }
    fn jump(&mut self, _:usize, view:&View, states:&mut [f64]) {
        match self.0.update(view.state(1),view.state(2),view.signal_in(0)) {
            Ok(s) => { states[1]=s.sequence; states[2]=s.age_s; }
            Err(_) => { states[1]=f64::NAN; states[2]=f64::NAN; }
        }
        states[0]=view.state(0)+1.;
    }
}
fn make(p:&std::collections::BTreeMap<String,f64>) -> Result<Box<dyn Behavior>,sim_core::EquationError> {
    let config=CommandLeaseConfig { period_s:param(p,"period_s")?, timeout_s:param(p,"timeout_s")? };
    Ok(Box::new(Registered(CommandLease::new(config).map_err(|e|sim_core::EquationError::InvalidParameter("command_lease".into(),e))?)))
}
pub fn register(registry:&mut BehaviorRegistry) -> Result<(),RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(BehaviorDescriptor::new("control.command_lease","Sampled command packet freshness",
        vec![signal_in("sequence",QuantityKind::Dimensionless),signal_out("fresh",QuantityKind::Dimensionless),signal_out("age",QuantityKind::Time)],make)
        .with_parameters(vec![P::required("period_s","s").positive(),P::required("timeout_s","s").positive()]))
}
