//! Sampled reference-time governor. Physics time never pauses. The supplied
//! condition and thresholds belong to the caller's observation/task contract.
use serde::{Deserialize, Serialize};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind, RegistryError,
    StateDeclaration, View, param, signal_in, signal_out,
};
pub const MOTION_CLOCK: &str = "control.motion_clock";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionClockConfig {
    pub period_s: f64,
    pub duration_s: f64,
    pub guard_start_s: f64,
    pub guard_end_s: f64,
    pub qualification_s: f64,
    pub maximum_pause_s: f64,
}

struct RegisteredClock(MotionClock);
impl RegisteredClock {
    fn read(&self, view: &View) -> Option<MotionClockState> {
        let values: Vec<_> = (0..6).map(|i| view.state(i)).collect();
        if values
            .iter()
            .any(|x| !x.is_finite() || *x < 0.0 || *x > (1_u64 << 52) as f64 || x.fract() != 0.0)
            || values[4] > 1.0
            || values[5] > 1.0
        {
            return None;
        }
        Some(MotionClockState {
            samples: values[0] as u64,
            reference_tick: values[1] as u64,
            qualified_samples: values[2] as u64,
            paused_intervals: values[3] as u64,
            advancing: values[4] != 0.0,
            timed_out: values[5] != 0.0,
        })
    }
}
impl Behavior for RegisteredClock {
    fn states(&self) -> Vec<StateDeclaration> {
        [
            "samples",
            "reference_tick",
            "qualified_samples",
            "paused_intervals",
            "advancing",
            "timed_out",
        ]
        .into_iter()
        .map(|n| StateDeclaration::new(n, QuantityKind::Dimensionless, 0.0))
        .collect()
    }
    fn residual(&self, ctx: &mut Context) {
        for i in 0..6 {
            ctx.set_state_residual(i, ctx.state_rate(i));
        }
        let samples = ctx.state(0);
        let elapsed = if samples > 0.0 {
            (ctx.time - (samples - 1.0) * self.0.config.period_s).clamp(0.0, self.0.config.period_s)
        } else {
            0.0
        };
        ctx.set_signal(
            0,
            (ctx.state(1) * self.0.config.period_s + ctx.state(4) * elapsed)
                .min(self.0.config.duration_s),
        );
        ctx.set_signal(1, ctx.state(4));
        ctx.set_signal(2, ctx.state(5));
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(view.state(0) * self.0.config.period_s - view.time);
    }
    fn scheduled_events(&self, view: &View, out: &mut Vec<(usize, f64)>) {
        out.push((0, view.state(0) * self.0.config.period_s));
    }
    fn jump(&mut self, _index: usize, view: &View, states: &mut [f64]) {
        let mut state = self.read(view).unwrap_or(MotionClockState {
            timed_out: true,
            ..Default::default()
        });
        let input = view.signal_in(0);
        if !input.is_finite() || self.0.sample(&mut state, view.time, input >= 1.0).is_err() {
            state.timed_out = true;
            state.advancing = false;
            state.samples = (view.time / self.0.config.period_s).round().max(0.0) as u64 + 1;
        }
        states.copy_from_slice(&[
            state.samples as f64,
            state.reference_tick as f64,
            state.qualified_samples as f64,
            state.paused_intervals as f64,
            u8::from(state.advancing) as f64,
            u8::from(state.timed_out) as f64,
        ]);
    }
}
fn make(
    p: &std::collections::BTreeMap<String, f64>,
) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let config = MotionClockConfig {
        period_s: param(p, "period_s")?,
        duration_s: param(p, "duration_s")?,
        guard_start_s: param(p, "guard_start_s")?,
        guard_end_s: param(p, "guard_end_s")?,
        qualification_s: param(p, "qualification_s")?,
        maximum_pause_s: param(p, "maximum_pause_s")?,
    };
    let clock = MotionClock::new(config)
        .map_err(|e| sim_core::EquationError::InvalidParameter("motion_clock".into(), e))?;
    Ok(Box::new(RegisteredClock(clock)))
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(
        BehaviorDescriptor::new(
            MOTION_CLOCK,
            "Qualified motion clock",
            vec![
                signal_in("condition", QuantityKind::Dimensionless),
                signal_out("reference_time", QuantityKind::Time),
                signal_out("advancing", QuantityKind::Dimensionless),
                signal_out("timed_out", QuantityKind::Dimensionless),
            ],
            make,
        )
        .with_parameters(vec![
            P::required("period_s", "s").positive(),
            P::required("duration_s", "s").positive(),
            P::required("guard_start_s", "s").nonnegative(),
            P::required("guard_end_s", "s").positive(),
            P::required("qualification_s", "s").nonnegative(),
            P::required("maximum_pause_s", "s").positive(),
        ]),
    )
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionClockState {
    pub samples: u64,
    pub reference_tick: u64,
    pub qualified_samples: u64,
    pub paused_intervals: u64,
    pub advancing: bool,
    pub timed_out: bool,
}
/// Last sampled decision, with a continuously readable reference time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MotionClockProgress {
    pub phase: String,
    pub reference_time_s: f64,
    pub duration_s: f64,
    pub observation_time_s: Option<f64>,
    pub qualified_duration_s: f64,
    pub required_qualification_s: f64,
    pub paused_duration_s: f64,
    pub maximum_pause_s: f64,
}
#[derive(Clone, Debug)]
pub struct MotionClock {
    config: MotionClockConfig,
    ticks: [u64; 5],
}
impl MotionClock {
    pub fn new(config: MotionClockConfig) -> Result<Self, String> {
        if !config.period_s.is_finite() || config.period_s <= 0.0 {
            return Err("positive finite motion-clock period required".into());
        }
        let mut ticks = [0; 5];
        for (out, value) in ticks.iter_mut().zip([
            config.duration_s,
            config.guard_start_s,
            config.guard_end_s,
            config.qualification_s,
            config.maximum_pause_s,
        ]) {
            let n = value / config.period_s;
            if !value.is_finite()
                || value < 0.0
                || !n.is_finite()
                || n > 1e9
                || (n - n.round()).abs() > 1e-8
            {
                return Err("motion-clock durations must lie on the declared sample grid (at most 1e9 ticks)".into());
            }
            *out = n.round() as u64;
        }
        if ticks[0] == 0 || ticks[1] >= ticks[2] || ticks[2] > ticks[0] || ticks[4] == 0 {
            return Err("ordered nonempty guard interval within duration and positive pause timeout required".into());
        }
        Ok(Self { config, ticks })
    }
    /// Read-only status for hosts. Qualification/pause durations refer to the
    /// last sampled decision; no unobserved interval earns qualification.
    pub fn progress(
        &self,
        state: &MotionClockState,
        time_s: f64,
    ) -> Result<MotionClockProgress, String> {
        let reference = if state.samples == 0 {
            if time_s != 0.0 {
                return Err("unsampled motion clock only has an initial status".into());
            }
            0.0
        } else {
            self.reference_at(state, time_s)?
        };
        let guarded = state.reference_tick >= self.ticks[1] && state.reference_tick < self.ticks[2];
        let phase = if state.timed_out {
            "timed_out"
        } else if reference >= self.config.duration_s {
            "complete"
        } else if state.samples == 0 {
            "initial"
        } else if guarded && !state.advancing {
            "waiting_for_condition"
        } else if guarded {
            "condition_qualified"
        } else {
            "following_reference"
        };
        Ok(MotionClockProgress {
            phase: phase.into(),
            reference_time_s: reference,
            duration_s: self.config.duration_s,
            observation_time_s: (state.samples > 0)
                .then(|| (state.samples - 1) as f64 * self.config.period_s),
            qualified_duration_s: (state.qualified_samples.saturating_sub(1) as f64
                * self.config.period_s)
                .min(self.config.qualification_s),
            required_qualification_s: self.config.qualification_s,
            paused_duration_s: state.paused_intervals as f64 * self.config.period_s,
            maximum_pause_s: self.config.maximum_pause_s,
        })
    }
    pub fn next_sample_s(&self, state: &MotionClockState) -> f64 {
        state.samples as f64 * self.config.period_s
    }
    pub fn reference_s(&self, state: &MotionClockState) -> f64 {
        state.reference_tick as f64 * self.config.period_s
    }
    /// Pure read between sample events, including the right endpoint. The next
    /// event can stop future progress but cannot undo the preceding interval.
    pub fn reference_at(&self, state: &MotionClockState, time_s: f64) -> Result<f64, String> {
        if state.samples == 0 || !time_s.is_finite() {
            return Err("motion clock has not sampled".into());
        }
        let elapsed = time_s - (state.samples - 1) as f64 * self.config.period_s;
        if elapsed < -1e-10 || elapsed > self.config.period_s + 1e-10 {
            return Err("reference requested outside held sample interval".into());
        }
        Ok((self.reference_s(state)
            + if state.advancing {
                elapsed.clamp(0.0, self.config.period_s)
            } else {
                0.0
            })
        .min(self.config.duration_s))
    }
    /// Advance only supplied checkpoint state. A failed simulation interval can
    /// discard this state along with its physics/controller checkpoint.
    pub fn sample(
        &self,
        state: &mut MotionClockState,
        time_s: f64,
        ready: bool,
    ) -> Result<(), String> {
        if !time_s.is_finite()
            || (time_s - self.next_sample_s(state)).abs() > 1e-10
            || state.reference_tick > self.ticks[0]
            || state.samples >= (1_u64 << 52)
            || state.qualified_samples > self.ticks[3] + 1
            || state.paused_intervals > self.ticks[4]
        {
            return Err("invalid motion-clock checkpoint or sample deadline".into());
        }
        let mut next = state.clone();
        if next.samples > 0 && !next.timed_out {
            if next.advancing {
                next.reference_tick = (next.reference_tick + 1).min(self.ticks[0]);
            } else if next.reference_tick >= self.ticks[1] && next.reference_tick < self.ticks[2] {
                next.paused_intervals = (next.paused_intervals + 1).min(self.ticks[4]);
            }
        }
        next.qualified_samples = if ready {
            (next.qualified_samples + 1).min(self.ticks[3] + 1)
        } else {
            0
        };
        let guarded = next.reference_tick >= self.ticks[1] && next.reference_tick < self.ticks[2];
        // N+1 observations span N complete sample intervals. No dwell credit
        // for the first observation or for unobserved time between callbacks.
        let qualified = next.qualified_samples > self.ticks[3];
        next.timed_out |= guarded && next.paused_intervals >= self.ticks[4] && !qualified;
        next.advancing =
            !next.timed_out && next.reference_tick < self.ticks[0] && (!guarded || qualified);
        if next.advancing {
            next.paused_intervals = 0;
        }
        next.samples += 1;
        *state = next;
        Ok(())
    }
}
