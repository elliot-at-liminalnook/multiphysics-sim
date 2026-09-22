//! Pulse-width modulators with exact scheduled edges.
//!
//! The duty command is sampled at each period start and held for the period
//! (as a hardware timer's compare register would be). Edges are scheduled
//! events, so the integrator steps exactly to them. The first period runs at
//! `initial_duty` because no command has been sampled yet.
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, ParameterDeclaration as P, QuantityKind, RegistryError, StateDeclaration, View, param, param_or, signal_in,
    signal_out,
};
use std::collections::BTreeMap;

pub const PWM: &str = "control.pwm";
pub const H_BRIDGE_PWM: &str = "control.h_bridge_pwm";

const ON: usize = 0;
const HELD: usize = 1;
const SIGN: usize = 2;
const NEXT: usize = 3;

/// One timer; `signed` holds the command's sign for bridge drive.
pub struct Modulator {
    pub period: f64,
    pub initial_duty: f64,
    pub bridge: bool,
}

impl Modulator {
    fn off_time(&self, view: &View) -> Option<f64> {
        let held = view.state(HELD);
        (view.state(ON) >= 0.5 && held < 1. - 1e-12).then(|| (view.state(NEXT) - 1. + held) * self.period)
    }
}

impl Behavior for Modulator {
    fn states(&self) -> Vec<StateDeclaration> {
        let duty = self.initial_duty.clamp(if self.bridge { -1. } else { 0. }, 1.);
        vec![
            StateDeclaration::new("on", QuantityKind::Dimensionless, if duty.abs() > 0. { 1. } else { 0. }),
            StateDeclaration::new("held_duty", QuantityKind::Dimensionless, duty.abs()),
            StateDeclaration::new("sign", QuantityKind::Dimensionless, if duty < 0. { -1. } else { 1. }),
            StateDeclaration::new("next_period", QuantityKind::Dimensionless, 1.),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        for k in 0..4 {
            ctx.set_state_residual(k, ctx.state_rate(k));
        }
        let on = ctx.state(ON);
        if !self.bridge {
            ctx.set_signal(0, on);
            return;
        }
        // Sign-magnitude drive with synchronous rectification; zero brakes.
        let forward = ctx.state(SIGN) >= 0.;
        let (hi_a, lo_a, hi_b, lo_b) = if ctx.state(HELD) == 0. {
            (0., 1., 0., 1.)
        } else if forward {
            (on, 1. - on, 0., 1.)
        } else {
            (0., 1., on, 1. - on)
        };
        ctx.set_signal(0, hi_a);
        ctx.set_signal(1, lo_a);
        ctx.set_signal(2, hi_b);
        ctx.set_signal(3, lo_b);
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(view.state(NEXT) * self.period - view.time);
        out.push(self.off_time(view).map_or(1., |t| t - view.time));
    }
    fn scheduled_events(&self, view: &View, out: &mut Vec<(usize, f64)>) {
        out.push((0, view.state(NEXT) * self.period));
        if let Some(t) = self.off_time(view) {
            out.push((1, t));
        }
    }
    fn jump(&mut self, index: usize, view: &View, states: &mut [f64]) {
        if index == 1 {
            states[ON] = 0.;
            return;
        }
        let command = view.signal_in(0);
        let command = if command.is_finite() { command } else { 0. };
        let duty = if self.bridge { command.clamp(-1., 1.) } else { command.clamp(0., 1.) };
        states[HELD] = duty.abs();
        states[SIGN] = if duty < 0. { -1. } else { 1. };
        states[ON] = if duty.abs() > 0. { 1. } else { 0. };
        states[NEXT] += 1.;
    }
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    let parameters = || vec![P::required("frequency", "Hz").positive(), P::optional("initial_duty", "1", 0.)];
    registry.register(
        BehaviorDescriptor::new(PWM, "PWM timer (duty 0…1 → gate)", vec![signal_in("duty", QuantityKind::Dimensionless), signal_out("gate", QuantityKind::Dimensionless)], |p: &BTreeMap<String, f64>| {
            Ok(Box::new(Modulator { period: 1. / param(p, "frequency")?, initial_duty: param_or(p, "initial_duty", 0.), bridge: false }))
        })
        .with_parameters(parameters()),
    )?;
    registry.register(
        BehaviorDescriptor::new(
            H_BRIDGE_PWM,
            "H-bridge gate driver (command −1…1 → four gates, sign-magnitude, synchronous)",
            vec![
                signal_in("command", QuantityKind::Dimensionless),
                signal_out("gate_hi_a", QuantityKind::Dimensionless),
                signal_out("gate_lo_a", QuantityKind::Dimensionless),
                signal_out("gate_hi_b", QuantityKind::Dimensionless),
                signal_out("gate_lo_b", QuantityKind::Dimensionless),
            ],
            |p: &BTreeMap<String, f64>| Ok(Box::new(Modulator { period: 1. / param(p, "frequency")?, initial_duty: param_or(p, "initial_duty", 0.), bridge: true })),
        )
        .with_parameters(parameters()),
    )
}
