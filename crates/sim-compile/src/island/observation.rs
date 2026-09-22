//! Retain existing solver evaluations; observing never calls behavior equations.
use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
struct Evaluation {
    time: f64,
    state: Vec<f64>,
    rate: Vec<f64>,
    through: Vec<Vec<f64>>,
    residual: Vec<f64>,
}
#[derive(Clone)]
struct Accepted {
    start: f64,
    end: f64,
    endpoint: Vec<f64>,
    evaluation: Evaluation,
}
#[derive(Default)]
struct Retained {
    evaluation: Option<Evaluation>,
    pending: Option<Accepted>,
    committed: Option<Accepted>,
}
#[derive(Default)]
pub(super) struct Capture {
    enabled: AtomicBool,
    retained: Mutex<Retained>,
}
fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
}
impl Capture {
    pub(super) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
    pub(super) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        self.clear();
    }
    pub(super) fn clear(&self) {
        *self.retained.lock().unwrap() = Retained::default();
    }
    pub(super) fn begin_attempt(&self) {
        if self.enabled() {
            let mut retained = self.retained.lock().unwrap();
            retained.evaluation = None;
            retained.pending = None;
        }
    }
    pub(super) fn record(
        &self,
        time: f64,
        state: &[f64],
        rate: &[f64],
        through: Vec<Vec<f64>>,
        residual: &[f64],
    ) {
        self.retained.lock().unwrap().evaluation = Some(Evaluation {
            time,
            state: state.into(),
            rate: rate.into(),
            through,
            residual: residual.into(),
        });
    }
    pub(super) fn solved(
        &self,
        start: f64,
        end: f64,
        time: f64,
        state: &[f64],
        rate: &[f64],
        endpoint: &[f64],
        full_of: &[usize],
    ) {
        if !self.enabled() {
            return;
        }
        let mut retained = self.retained.lock().unwrap();
        retained.pending = retained
            .evaluation
            .as_ref()
            .filter(|e| {
                e.time.to_bits() == time.to_bits()
                    && full_of.len() == state.len()
                    && full_of.iter().enumerate().all(|(r, f)| {
                        e.state[*f].to_bits() == state[r].to_bits()
                            && e.rate[*f].to_bits() == rate[r].to_bits()
                    })
            })
            .cloned()
            .map(|evaluation| Accepted {
                start,
                end,
                endpoint: endpoint.into(),
                evaluation,
            });
    }
    pub(super) fn commit(&self, time: f64, state: &[f64]) {
        if !self.enabled() {
            return;
        }
        let mut retained = self.retained.lock().unwrap();
        retained.committed = retained
            .pending
            .take()
            .filter(|a| a.end.to_bits() == time.to_bits() && same(&a.endpoint, state));
    }
    pub(super) fn value(
        &self,
        time: f64,
        state: &[f64],
        index: usize,
    ) -> Option<crate::observation::ValueObservation> {
        if !self.enabled() {
            return None;
        }
        let retained = self.retained.lock().unwrap();
        let a = retained
            .committed
            .as_ref()
            .filter(|a| a.end.to_bits() == time.to_bits() && same(&a.endpoint, state))?;
        let value = *a.evaluation.state.get(index)?;
        value
            .is_finite()
            .then_some(crate::observation::ValueObservation::AcceptedStage {
                value,
                sample_time: a.evaluation.time,
                step_start: a.start,
                step_end: a.end,
            })
    }
    pub(super) fn flow(
        &self,
        time: f64,
        state: &[f64],
        slot: usize,
        lane: usize,
    ) -> Option<crate::observation::FlowObservation> {
        if !self.enabled() {
            return None;
        }
        let retained = self.retained.lock().unwrap();
        let a = retained
            .committed
            .as_ref()
            .filter(|a| a.end.to_bits() == time.to_bits() && same(&a.endpoint, state))?;
        let value = *a.evaluation.through.get(slot)?.get(lane)?;
        if !value.is_finite() {
            return None;
        }
        Some(crate::observation::FlowObservation {
            value,
            evaluation_time: a.evaluation.time,
            step_start: a.start,
            step_end: a.end,
            residual_max: a
                .evaluation
                .residual
                .iter()
                .map(|v| v.abs())
                .fold(0., f64::max),
        })
    }
}
impl Island {
    pub(crate) fn endpoint_value(&self, index: usize) -> bool {
        self.reduced_of.get(index).is_some_and(|i| i.is_some())
            && self.algebraic.get(index) == Some(&false)
    }
    pub(crate) fn captured_value(
        &self,
        time: f64,
        state: &[f64],
        index: usize,
    ) -> Option<crate::observation::ValueObservation> {
        self.observations.value(time, state, index)
    }
    pub fn set_observation_capture(&self, enabled: bool) {
        self.observations.set_enabled(enabled);
    }
    pub(crate) fn flow_binding(&self, port: PortId, lane: usize) -> Option<(usize, usize)> {
        let (slot, offset, width) = self.port_flows.get(&port)?;
        (lane < *width).then_some((*slot, offset + lane))
    }
    pub(crate) fn captured_flow(
        &self,
        time: f64,
        state: &[f64],
        slot: usize,
        lane: usize,
    ) -> Option<crate::observation::FlowObservation> {
        self.observations.flow(time, state, slot, lane)
    }
}
