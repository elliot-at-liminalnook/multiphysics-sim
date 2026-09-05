//! `control.external`: the plant's side of the seam to control code that
//! lives elsewhere — another thread, another process, another language.
//!
//! Its ports are two families: `sense.*` (signal in, one member per
//! `sense.<name>` parameter) and `act.*` (signal out, one per
//! `act.<name>`). Every `period` seconds a guard fires, the element hands
//! the coupler the sensor frame and holds what comes back on its actuator
//! outputs until the next sample. `input_delay` and `output_delay` (in
//! samples) are ring buffers of whole frames — a delay through a bus is a
//! count of samples, not a lag — and `offset` is the time of the first
//! sample. Without a coupler the first sample is a failure the runtime
//! reports as an error naming the element.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, Contract, Coupler, Input, LocalJacobian, Output, QuantityKind, RegistryError, StateDeclaration, View,
    param, param_or, signal_in, signal_out,
};
use std::collections::BTreeMap;
use std::sync::Mutex;

pub const EXTERNAL: &str = "control.external";

type Params = BTreeMap<String, f64>;

pub struct External {
    pub sensors: Vec<String>,
    pub actuators: Vec<String>,
    pub period: f64,
    pub offset: f64,
    pub input_delay: usize,
    pub output_delay: usize,
    coupler: Mutex<Option<Box<dyn Coupler>>>,
    failure: Option<String>,
    samples: u64,
}

impl External {
    pub fn new(sensors: Vec<String>, actuators: Vec<String>, period: f64) -> Self {
        Self { sensors, actuators, period, offset: 0.0, input_delay: 0, output_delay: 0, coupler: Mutex::new(None), failure: None, samples: 0 }
    }
    pub fn samples(&self) -> u64 {
        self.samples
    }
    // State layout: held actuators, then the output queue (oldest frame
    // first), then the input queue (oldest first), then the next sample time.
    fn held(&self) -> std::ops::Range<usize> {
        0..self.actuators.len()
    }
    fn out_queue(&self, k: usize) -> std::ops::Range<usize> {
        let start = self.actuators.len() * (1 + k);
        start..start + self.actuators.len()
    }
    fn in_queue(&self, k: usize) -> std::ops::Range<usize> {
        let start = self.actuators.len() * (1 + self.output_delay) + self.sensors.len() * k;
        start..start + self.sensors.len()
    }
    fn clock(&self) -> usize {
        self.actuators.len() * (1 + self.output_delay) + self.sensors.len() * self.input_delay
    }

    fn sample(&mut self, view: &View, states: &mut [f64]) -> Result<(), String> {
        let now: Vec<f64> = (0..self.sensors.len()).map(|k| view.signal_in(k)).collect();
        // The frame the controller sees is `input_delay` samples old.
        let seen = if self.input_delay == 0 {
            now.clone()
        } else {
            let oldest = states[self.in_queue(0)].to_vec();
            for k in 0..self.input_delay - 1 {
                let next = states[self.in_queue(k + 1)].to_vec();
                states[self.in_queue(k)].copy_from_slice(&next);
            }
            states[self.in_queue(self.input_delay - 1)].copy_from_slice(&now);
            oldest
        };
        let mut command = states[self.held()].to_vec();
        {
            let mut guard = self.coupler.lock().map_err(|_| "coupler poisoned".to_owned())?;
            let coupler = guard.as_mut().ok_or_else(|| "no coupler attached".to_owned())?;
            coupler.sample(view.time, &seen, &mut command).map_err(|e| e.to_string())?;
        }
        if command.iter().any(|v| !v.is_finite()) {
            return Err(format!("non-finite actuator command {command:?}"));
        }
        // The command takes effect `output_delay` samples from now.
        let applied = if self.output_delay == 0 {
            command
        } else {
            let oldest = states[self.out_queue(0)].to_vec();
            for k in 0..self.output_delay - 1 {
                let next = states[self.out_queue(k + 1)].to_vec();
                states[self.out_queue(k)].copy_from_slice(&next);
            }
            states[self.out_queue(self.output_delay - 1)].copy_from_slice(&command);
            oldest
        };
        states[self.held()].copy_from_slice(&applied);
        self.samples += 1;
        Ok(())
    }
}

impl Behavior for External {
    fn states(&self) -> Vec<StateDeclaration> {
        let d = QuantityKind::Dimensionless;
        let mut out: Vec<StateDeclaration> = self.actuators.iter().map(|n| StateDeclaration::new(format!("act.{n}"), d, 0.0)).collect();
        for k in 0..self.output_delay {
            out.extend(self.actuators.iter().map(|n| StateDeclaration::new(format!("queue{k}.{n}"), d, 0.0)));
        }
        for k in 0..self.input_delay {
            out.extend(self.sensors.iter().map(|n| StateDeclaration::new(format!("seen{k}.{n}"), d, 0.0)));
        }
        out.push(StateDeclaration::new("next_sample", QuantityKind::Time, self.offset));
        out
    }
    fn residual(&self, ctx: &mut Context) {
        for k in 0..=self.clock() {
            ctx.set_state_residual(k, ctx.state_rate(k));
        }
        for k in 0..self.actuators.len() {
            ctx.set_signal(k, ctx.state(k));
        }
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        for k in 0..=self.clock() {
            out.state_rate(k, k, 1.0);
        }
        for k in 0..self.actuators.len() {
            out.set(Output::Signal(k), Input::State(k), 1.0);
        }
        true
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(view.state(self.clock()) - view.time);
    }
    fn jump(&mut self, _index: usize, view: &View, states: &mut [f64]) {
        if self.failure.is_none() {
            if let Err(message) = self.sample(view, states) {
                self.failure = Some(format!("at t={}: {message}", view.time));
            }
        }
        let clock = self.clock();
        states[clock] += self.period;
    }
    fn couple(&mut self, mut coupler: Box<dyn Coupler>, contract: Contract) -> Result<(), Box<dyn Coupler>> {
        match coupler.open(&contract) {
            Ok(()) => {}
            Err(e) => self.failure = Some(e.to_string()),
        }
        *self.coupler.lock().unwrap_or_else(|p| p.into_inner()) = Some(coupler);
        Ok(())
    }
    fn failure(&self) -> Option<String> {
        self.failure.clone()
    }
}

impl Drop for External {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.coupler.lock() {
            if let Some(coupler) = guard.as_mut() {
                coupler.close();
            }
        }
    }
}

fn family(p: &Params, prefix: &str) -> Vec<String> {
    p.keys().filter_map(|k| k.strip_prefix(prefix).map(str::to_owned)).collect()
}

fn external(p: &Params) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let mut element = External::new(family(p, "sense."), family(p, "act."), param(p, "period")?);
    element.offset = param_or(p, "offset", 0.0);
    element.input_delay = param_or(p, "input_delay", 0.0).max(0.0).round() as usize;
    element.output_delay = param_or(p, "output_delay", 0.0).max(0.0).round() as usize;
    Ok(Box::new(element))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use QuantityKind::Dimensionless as D;
    registry.register(BehaviorDescriptor::new(EXTERNAL, "External controller (seam)", vec![signal_in("sense.*", D), signal_out("act.*", D)], external).with_parameters(vec![
        P::required("period", "s").positive(), P::optional("offset", "s", 0.0),
        P::optional("input_delay", "samples", 0.0).integer(0.0, 4096.0),
        P::optional("output_delay", "samples", 0.0).integer(0.0, 4096.0),
        P::alternative("sense.*", "1"), P::alternative("act.*", "1"),
    ]))
}
