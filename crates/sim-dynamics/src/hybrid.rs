//! Event scheduling for custom state representations and continuous steppers.
//! Uses the same bracketed root locator as `Trajectory`. Physics is supplied by
//! the caller; events are never hidden by silently dropping guards or jumps.
use crate::{
    Event,
    event_root::{CrossingBracket, locate_crossing},
};
use std::cell::Cell;

pub trait HybridStepper {
    type State: Clone;
    /// Pure trial advancement from `state`. Do not tick controllers or mutate
    /// accepted state here. Diagnostic counters may change, physics may not.
    fn advance(&self, time: f64, step: f64, state: &Self::State) -> Result<Self::State, String>;
    fn guards(&self, time: f64, state: &Self::State) -> Result<Vec<f64>, String>;
    fn scheduled(&self, _time: f64, _state: &Self::State) -> Result<Vec<(usize, f64)>, String> {
        Ok(vec![])
    }
    /// Change only the supplied state. No external effects: the entire interval
    /// can still fail later and its result must then be discarded atomically.
    fn jump(&mut self, guard: usize, time: f64, state: &mut Self::State) -> Result<(), String>;
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HybridConfig {
    pub relative_event_tolerance: f64,
    pub maximum_events: usize,
    pub maximum_segments: usize,
    /// Recovery from failed continuous trials; this is not error estimation.
    pub maximum_halvings: usize,
}
impl Default for HybridConfig {
    fn default() -> Self {
        Self {
            relative_event_tolerance: 1e-7,
            maximum_events: 64,
            maximum_segments: 256,
            maximum_halvings: 6,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RejectedHybridTrial {
    pub start_time_s: f64,
    /// Outer trial duration; a failure can occur in an inner event-location solve.
    pub attempted_step_s: f64,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HybridDiagnostics {
    pub events: Vec<Event>,
    /// Includes discarded candidates, root-location trials and failed solves.
    pub continuous_attempts: usize,
    pub accepted_segments: usize,
    pub minimum_accepted_step_s: f64,
    /// Failed outer trials recovered by subdivision, including location failures.
    pub rejected_trials: usize,
    /// First 16 failures per interval, with reasons limited to 2048 characters.
    /// Counts remain complete even when diagnostic detail is bounded.
    pub rejection_details: Vec<RejectedHybridTrial>,
}

pub struct HybridResult<S> {
    pub time_s: f64,
    pub state: S,
    pub diagnostics: HybridDiagnostics,
}

/// Advance an interval atomically, splitting at known deadlines and located
/// guard crossings. Declaration order does not determine event order. Every
/// jump is followed by fresh guard/schedule evaluation. Endpoint clock events
/// fire before returning. A limit or unresolved crossing is an error, never
/// permission to finish the interval without processing events.
pub fn advance_interval<H: HybridStepper>(
    stepper: &mut H,
    initial: &H::State,
    start: f64,
    duration: f64,
    config: &HybridConfig,
) -> Result<HybridResult<H::State>, String> {
    let end = start + duration;
    if !start.is_finite()
        || start < 0.0
        || !duration.is_finite()
        || duration <= 0.0
        || !end.is_finite()
        || end <= start
        || !config.relative_event_tolerance.is_finite()
        || config.relative_event_tolerance <= 0.0
        || config.relative_event_tolerance >= 1.0
        || config.maximum_events == 0
        || config.maximum_events > 100000
        || config.maximum_segments == 0
        || config.maximum_segments > 100000
        || config.maximum_halvings > 20
    {
        return Err("invalid hybrid interval configuration".into());
    }
    let mut state = initial.clone();
    let mut time = start;
    // Arithmetic representations of the same clock boundary may differ by a
    // few ulps. This is clock-roundoff handling, not event-location tolerance.
    let clock_tolerance = 128.0 * f64::EPSILON * end.abs().max(duration);
    let mut events = Vec::new();
    let mut segments = 0;
    let mut minimum = duration;
    let attempts = Cell::new(0);
    let mut rejected_trials = 0;
    let mut rejection_details = Vec::new();
    let check_guards = |values: Vec<f64>, expected: Option<usize>| -> Result<Vec<f64>, String> {
        if expected.is_some_and(|n| n != values.len()) || values.iter().any(|v| !v.is_finite()) {
            Err("nonfinite or changing hybrid guard layout".into())
        } else {
            Ok(values)
        }
    };
    let count = check_guards(stepper.guards(time, &state)?, None)?.len();
    loop {
        let before = check_guards(stepper.guards(time, &state)?, Some(count))?;
        let scheduled = stepper.scheduled(time, &state)?;
        if scheduled
            .iter()
            .any(|(g, t)| *g >= count || !t.is_finite() || *t < time - clock_tolerance)
            || scheduled
                .iter()
                .enumerate()
                .any(|(i, (g, _))| scheduled[..i].iter().any(|(p, _)| p == g))
        {
            return Err(format!("invalid or overdue hybrid schedule at {time}"));
        }
        if let Some(&(guard, _)) = scheduled
            .iter()
            .find(|(_, t)| (*t - time).abs() <= clock_tolerance)
        {
            if events.len() >= config.maximum_events {
                return Err("hybrid event limit".into());
            }
            stepper.jump(guard, time, &mut state)?;
            events.push(Event { time, guard });
            continue;
        }
        if time == end {
            break;
        }
        if segments >= config.maximum_segments {
            return Err("hybrid segment limit".into());
        }
        let next_time = scheduled
            .iter()
            .map(|(_, t)| *t)
            .filter(|t| *t > time)
            .fold(end, f64::min);
        let mut h = next_time - time;
        let mut accepted = None;
        let mut last_error = String::new();
        for halving in 0..=config.maximum_halvings {
            if time + h <= time {
                return Err("hybrid step is below clock resolution".into());
            }
            let attempt = (|| {
                let solve = |dt| {
                    attempts.set(attempts.get() + 1);
                    stepper.advance(time, dt, &state)
                };
                let candidate = solve(h)?;
                let after = check_guards(stepper.guards(time + h, &candidate)?, Some(count))?;
                let crossings: Vec<_> = before
                    .iter()
                    .zip(&after)
                    .enumerate()
                    .filter_map(|(g, (a, b))| {
                        (*a >= 0.0 && *b < 0.0 && !scheduled.iter().any(|(s, _)| *s == g))
                            .then_some(g)
                    })
                    .collect();
                let Some(&first) = crossings.first() else {
                    return Ok((candidate, h, None));
                };
                let mut selected = (h, first);
                for guard in crossings {
                    let dt = locate_crossing(
                        CrossingBracket {
                            duration: h,
                            relative_tolerance: config.relative_event_tolerance,
                            before: before[guard],
                            after: after[guard],
                        },
                        |dt| {
                            let at = solve(dt)?;
                            Ok::<_, String>(
                                check_guards(stepper.guards(time + dt, &at)?, Some(count))?[guard],
                            )
                        },
                    )
                    .map_err(|e| format!("hybrid guard {guard} location: {e:?}"))?;
                    if dt < selected.0 {
                        selected = (dt, guard);
                    }
                }
                Ok::<_, String>((solve(selected.0)?, selected.0, Some(selected.1)))
            })();
            match attempt {
                Ok(result) => {
                    accepted = Some(result);
                    break;
                }
                Err(e) => {
                    rejected_trials += 1;
                    if rejection_details.len() < 16 {
                        rejection_details.push(RejectedHybridTrial {
                            start_time_s: time,
                            attempted_step_s: h,
                            reason: e.chars().take(2048).collect(),
                        });
                    }
                    last_error = e;
                    if halving < config.maximum_halvings {
                        h *= 0.5;
                    }
                }
            }
        }
        let (candidate, dt, first_guard) = accepted
            .ok_or_else(|| format!("hybrid advancement at {time} with step {h}: {last_error}"))?;
        state = candidate;
        time += dt;
        if (time - end).abs() <= clock_tolerance {
            time = end;
        }
        segments += 1;
        minimum = minimum.min(dt);
        if let Some(first) = first_guard {
            let mut fired = Vec::new();
            let mut next = Some(first);
            while let Some(guard) = next {
                if events.len() >= config.maximum_events {
                    return Err("hybrid event limit".into());
                }
                stepper.jump(guard, time, &mut state)?;
                fired.push(guard);
                events.push(Event { time, guard });
                let now = check_guards(stepper.guards(time, &state)?, Some(count))?;
                next = before.iter().zip(&now).enumerate().find_map(|(g, (a, b))| {
                    (*a >= 0.0
                        && *b < 0.0
                        && !fired.contains(&g)
                        && !scheduled.iter().any(|(s, _)| *s == g))
                    .then_some(g)
                });
            }
        }
    }
    Ok(HybridResult {
        time_s: time,
        state,
        diagnostics: HybridDiagnostics {
            events,
            continuous_attempts: attempts.get(),
            accepted_segments: segments,
            minimum_accepted_step_s: minimum,
            rejected_trials,
            rejection_details,
        },
    })
}
