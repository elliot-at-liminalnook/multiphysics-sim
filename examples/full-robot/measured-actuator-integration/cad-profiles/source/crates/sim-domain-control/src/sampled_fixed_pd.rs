//! Scheduled angular adapter for the exact integer law used in FPGA RTL.
//! Commands are sampled, delayed by the declared duration (not rounded to a
//! sample count), then held. All history is transactional behavior state.
use crate::fixed_pd::{self, Gains};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, EquationError,
    ParameterDeclaration as P, QuantityKind as Q, RegistryError, StateDeclaration, View, param,
    signal_in, signal_out,
};
use std::collections::BTreeMap;

pub const KIND: &str = "control.sampled_fixed_pd";

pub struct SampledFixedPd {
    gains: Gains,
    period: f64,
    latency: f64,
    offset: f64,
    quantum: f64,
    zero: f64,
    direction: f64,
    initial_target: u16,
    queue: usize,
}

fn invalid(message: &str) -> EquationError {
    EquationError::InvalidParameter(KIND.into(), message.into())
}

impl SampledFixedPd {
    pub fn from_parameters(p: &BTreeMap<String, f64>) -> Result<Self, EquationError> {
        for name in ["kp_q8", "kd_q8", "kv_q8", "limit", "encoder_zero"] {
            let v = param(p, name)?;
            let max = if name == "limit" {
                1000.
            } else if name == "encoder_zero" {
                4095.
            } else {
                4096.
            };
            if !v.is_finite() || v < 0. || v > max || v.fract() != 0. {
                return Err(invalid(
                    "bounded integer gains, limit and encoder zero required",
                ));
            }
        }
        let period = param(p, "period")?;
        let latency = param(p, "latency")?;
        let offset = param(p, "offset")?;
        let quantum = param(p, "encoder_quantum")?;
        let direction = param(p, "encoder_direction")?;
        if !period.is_finite()
            || period <= 0.
            || !latency.is_finite()
            || latency < 0.
            || !offset.is_finite()
            || offset < 0.
            || !quantum.is_finite()
            || quantum <= 0.
            || ![1., -1.].contains(&direction)
            || latency / period > 65534.
            || !(offset + latency + period).is_finite()
        {
            return Err(invalid(
                "finite timing/quantum, direction +/-1 and bounded delay required",
            ));
        }
        let mut result = Self {
            gains: Gains {
                kp_q8: param(p, "kp_q8")? as u16,
                kd_q8: param(p, "kd_q8")? as u16,
                kv_q8: param(p, "kv_q8")? as u16,
                limit: param(p, "limit")? as u16,
            },
            period,
            latency,
            offset,
            quantum,
            direction,
            zero: param(p, "encoder_zero")?,
            initial_target: 0,
            queue: (latency / period).ceil() as usize + 1,
        };
        result.initial_target = result
            .count(param(p, "initial_target")?)
            .ok_or_else(|| invalid("initial target outside non-wrapping encoder range"))?;
        Ok(result)
    }
    fn count(&self, angle: f64) -> Option<u16> {
        let count = (angle * self.direction / self.quantum + self.zero).round();
        (count.is_finite() && (0. ..=4095.).contains(&count)).then_some(count as u16)
    }
    fn clock(&self) -> usize {
        5 + self.queue
    }
    fn deadline(&self, tick: f64, delay: f64) -> f64 {
        self.offset + tick * self.period + delay
    }
    fn due(t: f64, deadline: f64) -> bool {
        (t - deadline).abs() <= 128. * f64::EPSILON * t.abs().max(deadline.abs()).max(1.)
    }
    fn update(&self, view: &View, states: &mut [f64]) -> Option<()> {
        if !Self::due(view.time, view.state(self.clock())) {
            return None;
        }
        let sampled = view.state(3);
        let applied = view.state(4);
        if sampled < 0.
            || applied < 0.
            || applied > sampled
            || sampled >= (1u64 << 53) as f64
            || sampled.fract() != 0.
            || applied.fract() != 0.
        {
            return None;
        }
        if Self::due(view.time, self.deadline(sampled, 0.)) {
            let position = self.count(view.signal_in(1))?;
            let target = self.count(view.signal_in(0))?;
            let previous = if sampled == 0. {
                position
            } else {
                view.state(1) as u16
            };
            let delta = target as i16 - view.state(2) as i16;
            let command = fixed_pd::step(self.gains, target, position, previous, delta).ok()?;
            states[5 + (sampled % self.queue as f64) as usize] =
                command as f64 * self.direction / 1000.;
            states[1] = position as f64;
            states[2] = target as f64;
            states[3] = sampled + 1.;
        }
        if applied < states[3] && Self::due(view.time, self.deadline(applied, self.latency)) {
            states[0] = states[5 + (applied % self.queue as f64) as usize];
            states[4] = applied + 1.;
        }
        let next_sample = self.deadline(states[3], 0.);
        let next_apply = if states[4] < states[3] {
            self.deadline(states[4], self.latency)
        } else {
            f64::INFINITY
        };
        states[self.clock()] = next_sample.min(next_apply);
        (states[self.clock()].is_finite() && states[self.clock()] > view.time).then_some(())
    }
}

impl Behavior for SampledFixedPd {
    fn states(&self) -> Vec<StateDeclaration> {
        let mut states = vec![
            StateDeclaration::new("command", Q::Dimensionless, 0.),
            StateDeclaration::new("previous_encoder", Q::Dimensionless, 0.),
            StateDeclaration::new(
                "previous_target",
                Q::Dimensionless,
                self.initial_target as f64,
            ),
            StateDeclaration::new("sampled_count", Q::Dimensionless, 0.),
            StateDeclaration::new("applied_count", Q::Dimensionless, 0.),
        ];
        for i in 0..self.queue {
            states.push(StateDeclaration::new(
                format!("queue{i}"),
                Q::Dimensionless,
                0.,
            ));
        }
        states.push(StateDeclaration::new("next_event", Q::Time, self.offset));
        states
    }
    fn residual(&self, ctx: &mut Context) {
        for i in 0..=self.clock() {
            ctx.set_state_residual(i, ctx.state_rate(i));
        }
        ctx.set_signal(0, ctx.state(0));
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(view.state(self.clock()) - view.time);
    }
    fn scheduled_events(&self, view: &View, out: &mut Vec<(usize, f64)>) {
        out.push((0, view.state(self.clock())));
    }
    fn jump(&mut self, _: usize, view: &View, states: &mut [f64]) {
        // The caller rejects nonfinite endpoints and rolls back all state. Never
        // wrap or clip an out-of-range encoder into a plausible motor command.
        if self.update(view, states).is_none() {
            states[0] = f64::NAN;
        }
    }
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(
        BehaviorDescriptor::new(
            KIND,
            "FPGA integer PD with sampled encoder and delayed held PWM",
            vec![
                signal_in("target", Q::Angle),
                signal_in("position", Q::Angle),
                signal_out("duty", Q::Dimensionless),
            ],
            |p| Ok(Box::new(SampledFixedPd::from_parameters(p)?)),
        )
        .with_parameters(vec![
            P::required("kp_q8", "1").integer(0., 4096.),
            P::required("kd_q8", "1").integer(0., 4096.),
            P::required("kv_q8", "1").integer(0., 4096.),
            P::required("limit", "1").integer(0., 1000.),
            P::required("period", "s").positive(),
            P::required("latency", "s").nonnegative(),
            P::required("offset", "s").nonnegative(),
            P::required("encoder_quantum", "rad").positive(),
            P::required("encoder_zero", "1").integer(0., 4095.),
            P::required("encoder_direction", "1"),
            P::required("initial_target", "rad"),
        ]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn controller(latency: f64) -> SampledFixedPd {
        SampledFixedPd::from_parameters(&BTreeMap::from([
            ("kp_q8".into(), 256.),
            ("kd_q8".into(), 512.),
            ("kv_q8".into(), 256.),
            ("limit".into(), 350.),
            ("period".into(), 0.01),
            ("latency".into(), latency),
            ("offset".into(), 0.),
            ("encoder_quantum".into(), 0.001),
            ("encoder_zero".into(), 2048.),
            ("encoder_direction".into(), 1.),
            ("initial_target".into(), 0.),
        ]))
        .unwrap()
    }
    fn tick(c: &mut SampledFixedPd, state: &mut Vec<f64>, t: f64, target: f64, position: f64) {
        let old = state.clone();
        c.jump(
            0,
            &View {
                time: t,
                states: &old,
                offsets: &[0; 4],
                rate_map: &[None; 3],
                across: &[],
                across_rates: &[],
                signals_in: &[target, position],
            },
            state,
        );
        assert!(state.iter().all(|x| x.is_finite()));
    }
    #[test]
    fn integer_law_matches_across_reversal_and_delayed_ticks() {
        for delay in [0., 0.004, 0.02, 0.025] {
            let mut c = controller(delay);
            let mut state: Vec<_> = c.states().iter().map(|s| s.initial).collect();
            let mut reference = vec![];
            let mut previous = 2048;
            let mut previous_target = 2048;
            for _ in 0..40 {
                let t = state[c.clock()];
                let index = state[3] as usize;
                let target = 2048 + (if index % 8 < 4 { 25 } else { -25 });
                let position = 2048 + (index as i16 % 7) - 3;
                if SampledFixedPd::due(t, c.deadline(index as f64, 0.)) {
                    reference.push(
                        fixed_pd::step(
                            c.gains,
                            target as u16,
                            position as u16,
                            if index == 0 {
                                position as u16
                            } else {
                                previous
                            },
                            target - previous_target,
                        )
                        .unwrap() as f64
                            / 1000.,
                    );
                    previous = position as u16;
                    previous_target = target;
                }
                tick(
                    &mut c,
                    &mut state,
                    t,
                    (target - 2048) as f64 * 0.001,
                    (position - 2048) as f64 * 0.001,
                );
                let applied = state[4] as usize;
                assert_eq!(
                    state[0],
                    if applied == 0 {
                        0.
                    } else {
                        reference[applied - 1]
                    }
                );
                let next = state[c.clock()];
                assert!(next > t);
                // Exact subperiod latency is retained in apply deadlines.
                if delay == 0.004 && t == 0. {
                    assert_eq!(next, 0.004);
                    assert_eq!(state[0], 0.);
                }
            }
        }
    }
    #[test]
    fn restored_state_replays_and_encoder_overflow_is_rejected() {
        let mut c = controller(0.004);
        let mut state: Vec<_> = c.states().iter().map(|s| s.initial).collect();
        tick(&mut c, &mut state, 0., 0.03, 0.);
        let mut restored = state.clone();
        tick(&mut c, &mut state, 0.004, 0.03, 0.);
        let mut other = controller(0.004);
        tick(&mut other, &mut restored, 0.004, 0.03, 0.);
        assert_eq!(state, restored);
        let old = state.clone();
        c.jump(
            0,
            &View {
                time: 0.01,
                states: &old,
                offsets: &[0; 4],
                rate_map: &[None; 3],
                across: &[],
                across_rates: &[],
                signals_in: &[10., 0.],
            },
            &mut state,
        );
        assert!(state[0].is_nan());
    }
}
