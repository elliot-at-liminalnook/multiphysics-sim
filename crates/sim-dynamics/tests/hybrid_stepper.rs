use sim_dynamics::{
    event_root::{CrossingBracket, RootError, locate_crossing},
    hybrid::{HybridConfig, HybridStepper, advance_interval},
};

#[derive(Clone, Debug, PartialEq)]
struct State {
    x: f64,
    frozen: bool,
    ticks: usize,
}
struct Slider {
    fail_after: Option<f64>,
    repeat_clock: bool,
    bad_layout: bool,
}
impl HybridStepper for Slider {
    type State = State;
    fn advance(&self, t: f64, h: f64, s: &State) -> Result<State, String> {
        if self.fail_after.is_some_and(|limit| t + h > limit) {
            return Err("fixture failure".into());
        }
        let mut s = s.clone();
        if !s.frozen {
            s.x += h;
        }
        Ok(s)
    }
    fn guards(&self, _: f64, s: &State) -> Result<Vec<f64>, String> {
        if self.bad_layout && s.x > 0.1 {
            return Ok(vec![1.0]);
        }
        Ok(if s.frozen {
            vec![1.0; 3]
        } else {
            vec![0.8 - s.x, 0.3 - s.x, 1.0]
        })
    }
    fn scheduled(&self, _: f64, s: &State) -> Result<Vec<(usize, f64)>, String> {
        Ok(match s.ticks {
            0 => vec![(2, 0.0)],
            1 => vec![(2, 1.0)],
            _ => vec![],
        })
    }
    fn jump(&mut self, g: usize, _: f64, s: &mut State) -> Result<(), String> {
        match g {
            1 => s.frozen = true,
            2 => {
                if !self.repeat_clock {
                    s.ticks += 1;
                }
            }
            _ => return Err("later invalidated event fired".into()),
        }
        Ok(())
    }
}
fn state() -> State {
    State {
        x: 0.0,
        frozen: false,
        ticks: 0,
    }
}
fn slider() -> Slider {
    Slider {
        fail_after: None,
        repeat_clock: false,
        bad_layout: false,
    }
}

#[test]
fn earliest_crossing_and_endpoint_clocks_use_simulation_time() {
    let initial = state();
    let result =
        advance_interval(&mut slider(), &initial, 0.0, 1.0, &HybridConfig::default()).unwrap();
    assert_eq!(result.time_s, 1.0);
    assert_eq!(result.state.ticks, 2);
    assert!((result.state.x - 0.3).abs() < 2e-7);
    let events = &result.diagnostics.events;
    assert_eq!(
        events.iter().map(|e| e.guard).collect::<Vec<_>>(),
        vec![2, 1, 2]
    );
    assert_eq!(events[0].time, 0.0);
    assert!((events[1].time - 0.3).abs() < 2e-7);
    assert_eq!(events[2].time, 1.0);
    assert!(result.diagnostics.continuous_attempts > result.diagnostics.accepted_segments);
    assert_eq!(initial, state());
}

#[test]
fn failures_limits_and_invalid_guards_do_not_commit_a_partial_interval() {
    let initial = state();
    let mut failing = slider();
    failing.fail_after = Some(0.5);
    let error = advance_interval(
        &mut failing,
        &initial,
        0.0,
        1.0,
        &HybridConfig {
            maximum_halvings: 2,
            ..Default::default()
        },
    )
    .err()
    .unwrap();
    assert!(error.contains("fixture failure"));
    assert_eq!(initial, state());
    let mut repeating = slider();
    repeating.repeat_clock = true;
    assert!(
        advance_interval(
            &mut repeating,
            &initial,
            0.0,
            1.0,
            &HybridConfig {
                maximum_events: 3,
                ..Default::default()
            }
        )
        .err()
        .unwrap()
        .contains("event limit")
    );
    let mut bad = slider();
    bad.bad_layout = true;
    assert!(
        advance_interval(
            &mut bad,
            &initial,
            0.0,
            1.0,
            &HybridConfig {
                maximum_halvings: 0,
                ..Default::default()
            }
        )
        .err()
        .unwrap()
        .contains("guard layout")
    );
    for h in [0.0, -1.0, f64::NAN] {
        assert!(advance_interval(&mut slider(), &initial, 0.0, h, &Default::default()).is_err());
    }
    assert_eq!(initial, state());
}

#[test]
fn root_location_handles_linear_nonlinear_and_invalid_brackets() {
    let b = CrossingBracket {
        duration: 1.0,
        relative_tolerance: 1e-8,
        before: 0.37,
        after: -0.63,
    };
    let dt = locate_crossing(b, |t| Ok::<_, ()>(0.37 - t)).unwrap();
    assert!((dt - 0.37).abs() < 2e-8);
    let dt = locate_crossing(
        CrossingBracket {
            before: 0.37_f64.exp() - 1.0,
            after: 0.37_f64.exp() - 1.0_f64.exp(),
            ..b
        },
        |t| Ok::<_, ()>(0.37_f64.exp() - t.exp()),
    )
    .unwrap();
    assert!((dt - 0.37).abs() < 3e-8);
    assert!(matches!(
        locate_crossing(CrossingBracket { before: -1.0, ..b }, |_| Ok::<_, ()>(0.0)),
        Err(RootError::InvalidBracket)
    ));
    assert!(matches!(
        locate_crossing(b, |_| Ok::<_, ()>(f64::NAN)),
        Err(RootError::NonFiniteGuard)
    ));
    assert!(matches!(
        locate_crossing(b, |_| Err::<f64, _>("test error")),
        Err(RootError::Evaluation("test error"))
    ));
}

#[test]
fn repeated_clock_deadlines_do_not_create_roundoff_sized_continuous_steps() {
    struct Clock;
    impl sim_dynamics::hybrid::HybridStepper for Clock {
        type State = [f64; 2]; // next clock and tick count
        fn advance(&self, _: f64, h: f64, s: &Self::State) -> Result<Self::State, String> {
            if h < 0.000125 {
                return Err("roundoff-sized clock interval".into());
            }
            Ok(*s)
        }
        fn guards(&self, t: f64, s: &Self::State) -> Result<Vec<f64>, String> {
            Ok(vec![s[0] - t])
        }
        fn scheduled(&self, _: f64, s: &Self::State) -> Result<Vec<(usize, f64)>, String> {
            Ok(vec![(0, s[0])])
        }
        fn jump(&mut self, _: usize, _: f64, s: &mut Self::State) -> Result<(), String> {
            s[0] += 0.02;
            s[1] += 1.0;
            Ok(())
        }
    }
    let mut clock = Clock;
    let mut state = [0.02, 0.0];
    let mut segments = 0;
    for i in 0..1600 {
        let result = sim_dynamics::hybrid::advance_interval(
            &mut clock,
            &state,
            i as f64 * 0.00025,
            0.00025,
            &Default::default(),
        )
        .unwrap();
        state = result.state;
        segments += result.diagnostics.accepted_segments;
    }
    assert_eq!(state[1], 20.0);
    assert_eq!(segments, 1600);
}

#[test]
fn recovered_failures_report_bounded_reasons_without_changing_advancement() {
    struct LimitedStep(std::cell::Cell<usize>);
    impl HybridStepper for LimitedStep {
        type State = f64;
        fn advance(&self, _: f64, h: f64, x: &f64) -> Result<f64, String> {
            if h > 0.125 {
                self.0.set(self.0.get() + 1);
                return Err("é".repeat(3000));
            }
            Ok(x + 2.0 * h)
        }
        fn guards(&self, _: f64, _: &f64) -> Result<Vec<f64>, String> {
            Ok(vec![])
        }
        fn jump(&mut self, _: usize, _: f64, _: &mut f64) -> Result<(), String> {
            unreachable!()
        }
    }
    let mut stepper = LimitedStep(std::cell::Cell::new(0));
    let initial = 3.0;
    let result = advance_interval(&mut stepper, &initial, 0.0, 4.0, &Default::default()).unwrap();
    assert!((result.state - 11.0).abs() < 1e-12);
    assert_eq!(initial, 3.0);
    let diagnostics = result.diagnostics;
    assert_eq!(diagnostics.rejected_trials, stepper.0.get());
    assert!(diagnostics.rejected_trials > 16);
    assert_eq!(diagnostics.rejection_details.len(), 16);
    assert_eq!(diagnostics.rejection_details[0].start_time_s, 0.0);
    assert_eq!(diagnostics.rejection_details[0].attempted_step_s, 4.0);
    assert!(
        diagnostics
            .rejection_details
            .iter()
            .all(|r| r.reason.chars().count() == 2048)
    );
    assert_eq!(
        diagnostics.continuous_attempts,
        diagnostics.rejected_trials + diagnostics.accepted_segments
    );
}
