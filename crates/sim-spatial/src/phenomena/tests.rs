//! Phenomena mode without a window: sim-app's selection rule, its pacing as
//! a pure function on a fake exhibit (no threads), the knob and speed rules,
//! the glyph map, and the run thread on fake exhibits (catalogue, commands,
//! stale generations, the join bound).
use super::ExhibitRef;
use super::panel::{glyphs, steady};
use super::run::{self, CHART_INTERVAL, CHART_POINTS, Command, Frame, MAX_REAL, Op, Pacing, Stepped, knob_target, speed_target};
use crate::jobs::{JOIN_BOUND, RunThread};
use sim_phenomena::exhibit::{Exhibit, Knob, Readout, Shape};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A test exhibit: records every advance, with a chosen grid and time scale,
/// and fails on demand. Its drop sets `dropped` (the run thread has returned).
struct Fake {
    title: &'static str,
    time: f64,
    grid: f64,
    scale: f64,
    knob: f64,
    fail: Option<String>,
    advances: Arc<Mutex<Vec<f64>>>,
    dropped: Arc<AtomicBool>,
}
impl Fake {
    fn new(title: &'static str) -> Self {
        Self { title, time: 0.0, grid: 0.0, scale: 1.0, knob: 1.0, fail: None, advances: Arc::default(), dropped: Arc::default() }
    }
    /// (Fake implements Drop, so no struct-update syntax: set fields after `new`.)
    fn with(title: &'static str, grid: f64, scale: f64) -> Self {
        let mut fake = Self::new(title);
        fake.grid = grid;
        fake.scale = scale;
        fake
    }
    fn advances(&self) -> Vec<f64> {
        self.advances.lock().unwrap().clone()
    }
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}
impl Exhibit for Fake {
    fn title(&self) -> &'static str {
        self.title
    }
    fn summary(&self) -> &'static str {
        "a test exhibit"
    }
    fn knob(&self) -> Knob {
        Knob { label: "k", unit: "m", min: 0.0, max: 2.0, step: 0.25, value: self.knob }
    }
    fn set_knob(&mut self, value: f64) {
        self.knob = value;
        self.time = 0.0;
    }
    fn reset(&mut self) {
        self.time = 0.0;
    }
    fn time(&self) -> f64 {
        self.time
    }
    fn time_scale(&self) -> f64 {
        self.scale
    }
    fn grid(&self) -> f64 {
        self.grid
    }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        self.advances.lock().unwrap().push(duration);
        self.time += duration;
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        out.push(Shape::Sphere { center: [0.0, self.time, 0.0], radius: 0.5, color: [0.2, 0.3, 0.4] });
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![Readout::new("time", self.time, "s")]
    }
    fn signal(&self) -> (&'static str, f64) {
        ("time", self.time)
    }
    fn verdict(&self) -> String {
        "fine".into()
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[test]
fn exhibit_refs_resolve_by_number_or_title_fragment() {
    let titles = ["Kapitza's inverted pendulum", "Water hammer", "The 2:1 spring pendulum"];
    assert_eq!(ExhibitRef::Number(2).resolve(&titles), Ok(1));
    let e = ExhibitRef::Number(0).resolve(&titles).unwrap_err();
    assert!(e.contains("exhibit 0 is out of range") && e.contains("there are 3 exhibits"), "{e}");
    assert!(ExhibitRef::Number(4).resolve(&titles).unwrap_err().contains("numbered 1 to 3"));
    // Case-insensitive; the first title containing the fragment.
    assert_eq!(ExhibitRef::Title("HAMMER".into()).resolve(&titles), Ok(1));
    assert_eq!(ExhibitRef::Title("pendulum".into()).resolve(&titles), Ok(0));
    // Digits are a number, whichever way they came.
    assert_eq!(ExhibitRef::Title(" 3 ".into()).resolve(&titles), Ok(2));
    assert!(ExhibitRef::Title("9".into()).resolve(&titles).unwrap_err().contains("out of range"));
    // sim-app's rule: a number out of range is then tried as a title fragment.
    assert_eq!(ExhibitRef::Number(42).resolve(&["Other", "Rule 42"]), Ok(1));
    assert_eq!(ExhibitRef::parse("42").resolve(&["Other", "Rule 42"]), Ok(1));
    let e = ExhibitRef::Title("geyser".into()).resolve(&titles).unwrap_err();
    assert!(e.contains("`geyser`") && e.contains("3 exhibits"), "{e}");
    assert_eq!(ExhibitRef::parse("2"), ExhibitRef::Number(2));
    assert_eq!(ExhibitRef::parse("2:1"), ExhibitRef::Title("2:1".into()));
    assert_eq!(ExhibitRef::parse("2:1").resolve(&titles), Ok(2));
    // The REST argument takes either form.
    assert_eq!(serde_json::from_value::<ExhibitRef>(serde_json::json!(3)).unwrap(), ExhibitRef::Number(3));
    assert_eq!(serde_json::from_value::<ExhibitRef>(serde_json::json!("spring")).unwrap(), ExhibitRef::Title("spring".into()));
}

/// dt = min(real, 0.05) × time_scale × speed; nothing while paused or for no time.
#[test]
fn pacing_clamps_real_time_and_scales_it() {
    let mut fake = Fake::with("a", 0.0, 2.0);
    let mut p = Pacing::default();
    p.speed = 0.5;
    assert_eq!(p.step(&mut fake, 0.02), Stepped::Advanced);
    assert_eq!(p.step(&mut fake, 0.5), Stepped::Advanced, "a slow tick still advances, by at most 0.05 s of real time");
    assert_eq!(fake.advances(), vec![0.02 * 2.0 * 0.5, MAX_REAL * 2.0 * 0.5]);
    p.paused = true;
    assert_eq!(p.step(&mut fake, 0.02), Stepped::Skipped);
    p.paused = false;
    assert_eq!(p.step(&mut fake, 0.0), Stepped::Skipped);
    assert_eq!(fake.advances().len(), 2, "paused and zero-time ticks do not advance");
}

/// A gridded exhibit takes whole grid steps; the remainder carries to the
/// next tick, and a switch drops it.
#[test]
fn pacing_takes_whole_grid_steps_and_carries_the_rest() {
    let mut fake = Fake::with("a", 0.01, 1.0);
    let mut p = Pacing::default();
    assert_eq!(p.step(&mut fake, 0.025), Stepped::Advanced);
    assert!(close(fake.advances()[0], 0.02) && close(p.accumulator, 0.005), "{:?} {}", fake.advances(), p.accumulator);
    assert_eq!(p.step(&mut fake, 0.004), Stepped::Carried, "0.009 s is less than one step");
    assert_eq!(fake.advances().len(), 1);
    assert_eq!(p.step(&mut fake, 0.003), Stepped::Advanced);
    assert!(close(fake.advances()[1], 0.01) && close(p.accumulator, 0.002), "{:?} {}", fake.advances(), p.accumulator);
    p.switched();
    assert_eq!(p.accumulator, 0.0);
}

/// One sample of `signal()` per 1/30 s of real time (at most one per tick),
/// the last 1800 kept; a switch restarts the chart.
#[test]
fn the_chart_samples_at_30_hz_and_keeps_the_last_minute() {
    let mut fake = Fake::new("a");
    let mut p = Pacing::default();
    p.step(&mut fake, 0.02);
    assert!(p.chart.is_empty() && close(p.chart_clock, 0.02));
    p.step(&mut fake, 0.02);
    assert_eq!(p.chart.len(), 1, "0.04 s reached the 1/30 s interval");
    assert!(close(p.chart_clock, 0.04 - CHART_INTERVAL));
    assert!(close(p.chart[0], 0.04), "the signal after the tick");
    let mut fake = Fake::new("b");
    let mut p = Pacing::default();
    for _ in 0..1900 {
        p.step(&mut fake, MAX_REAL);
    }
    assert_eq!(p.chart.len(), CHART_POINTS);
    assert_eq!(p.chart_count, 1900, "one sample per tick, even though the clock runs ahead");
    assert!((p.chart[0] - 101.0 * MAX_REAL).abs() < 1e-9 && (p.chart[CHART_POINTS - 1] - 1900.0 * MAX_REAL).abs() < 1e-9, "{} {}", p.chart[0], p.chart[CHART_POINTS - 1]);
    p.switched();
    assert!(p.chart.is_empty() && p.chart_clock == 0.0);
}

/// An `advance` error is kept verbatim and stops the run (no more advance
/// calls, no chart time) until a switch clears it.
#[test]
fn an_advance_error_stops_the_run_until_a_switch() {
    let mut fake = Fake::new("a");
    let mut p = Pacing::default();
    p.step(&mut fake, 0.02);
    fake.fail = Some("the solver diverged at t = 0.02 s".into());
    assert_eq!(p.step(&mut fake, 0.02), Stepped::Failed);
    assert_eq!(p.error.as_deref(), Some("the solver diverged at t = 0.02 s"));
    assert!(close(p.chart_clock, 0.02), "a failed tick adds no chart time");
    fake.fail = None;
    assert_eq!(p.step(&mut fake, 0.02), Stepped::Skipped, "stopped until a switch");
    assert_eq!(fake.advances().len(), 1);
    p.switched();
    assert_eq!(p.error, None);
    assert_eq!(p.step(&mut fake, 0.02), Stepped::Advanced);
}

/// sim-app's knob nudge (step, Shift × 5, clamped, rounded to the step) and speed (×2, ÷2 within 1/64…64).
#[test]
fn knob_and_speed_follow_sim_app() {
    let knob = Knob { label: "k", unit: "m", min: 0.0, max: 2.0, step: 0.25, value: 1.0 };
    assert_eq!(knob_target(&knob, None, Some(1.0)), 1.25);
    assert_eq!(knob_target(&knob, None, Some(-5.0)), 0.0, "Shift+← from 1.0: clamped to the minimum");
    assert_eq!(knob_target(&knob, None, Some(5.0)), 2.0);
    assert_eq!(knob_target(&knob, Some(0.3), None), 0.25, "rounded to the step");
    assert_eq!(knob_target(&knob, Some(9.0), None), 2.0);
    let zero = Knob { step: 0.0, ..knob.clone() };
    assert_eq!(knob_target(&zero, Some(0.3), None), 0.3, "no step: no rounding (sim-app divided by zero)");
    assert_eq!(speed_target(1.0, None, Some(1)), 2.0);
    assert_eq!(speed_target(64.0, None, Some(1)), 64.0);
    assert_eq!(speed_target(1.0 / 64.0, None, Some(-1)), 1.0 / 64.0);
    assert_eq!(speed_target(1.0, Some(1000.0), None), 64.0);
    assert_eq!(speed_target(4.0, Some(0.5), None), 0.5);
}

#[test]
fn glyphs_replace_only_what_the_fonts_lack() {
    assert_eq!(glyphs("×10⁻³ 1/m; ω² ∝ 1 − P/P_cr ⇒ ▸ (2 ∓ √2)·g/L ⟨p′q′⟩"), "×10−³ 1/m; ω² ~ 1 − P/P_cr → › (2 ± √2)·g/L ‹p′q′›");
    assert_eq!(glyphs("θ̇* at γ = 0.009, Δ ≈ 3 °, Painlevé"), "θ̇* at γ = 0.009, Δ ≈ 3 °, Painlevé");
    assert_eq!(steady(1234.5678), "1235");
    assert_eq!(steady(12.0), "12.00");
    assert_eq!(steady(-3.14159), "-3.142");
    assert_eq!(steady(0.0123456), "0.01235");
    assert_eq!(steady(0.0), "0.000");
    assert_eq!(steady(2.5e6), "2.500e6");
}

/// Wait (up to 5 s) for a frame at `generation` or later that passes `ok`.
fn frame(run: &RunThread<Command, Frame>, generation: u64, ok: impl Fn(&Frame) -> bool) -> Frame {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if let Some(f) = run.latest(generation).filter(|f| ok(f)) {
            return f;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let latest = run.lock();
    panic!("no frame at generation {generation} in 5 s (latest: generation {}, seq {})", latest.generation, latest.seq);
}

/// The run thread builds the exhibits, publishes the catalogue and the
/// selected exhibit, applies commands in order, stamps frames with the
/// command's generation, and stops within the join bound when dropped.
#[test]
fn the_run_thread_publishes_frames_and_stops_within_the_join_bound() {
    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    let run = run::spawn_with(Some("SECOND".into()), 1, move || {
        let fake = |title: &'static str| -> Box<dyn Exhibit> {
            let mut fake = Fake::new(title);
            fake.dropped = flag.clone();
            Box::new(fake)
        };
        vec![fake("The first"), fake("The second one")]
    });
    let first = frame(&run, 1, |f| !f.catalogue.is_empty());
    assert_eq!(first.titles(), vec!["The first", "The second one"]);
    assert_eq!((first.current, first.generation, first.seq, first.notice.clone()), (1, 1, 0, None), "--exhibit SECOND opened the second");
    assert_eq!(first.entry().summary, "a test exhibit");
    assert!(first.knob.is_some() && first.readouts.len() == 1 && first.shapes.len() == 1);
    // Ticks advance it on its own clock.
    let later = frame(&run, 1, |f| f.time > 0.0);
    assert_eq!(later.current, 1);

    // A switch: generation 2, and the next exhibit wraps to the first.
    run.send(Command { seq: 1, generation: 2, op: Op::Step(1) }).unwrap();
    let next = frame(&run, 2, |f| f.seq >= 1);
    assert_eq!((next.current, next.generation), (0, 2));
    assert!(next.chart.is_empty() || next.chart.len() < 30, "the chart restarted");
    // Not a switch: the generation stays.
    run.send(Command { seq: 2, generation: 2, op: Op::Pause(Some(true)) }).unwrap();
    let paused = frame(&run, 2, |f| f.seq >= 2);
    assert!(paused.paused && paused.generation == 2);
    run.send(Command { seq: 3, generation: 3, op: Op::Knob { value: None, steps: Some(1.0) } }).unwrap();
    let knob = frame(&run, 3, |f| f.seq >= 3);
    assert_eq!(knob.knob.map(|k| k.value), Some(1.25));
    run.send(Command { seq: 4, generation: 3, op: Op::Speed { speed: None, steps: Some(-1) } }).unwrap();
    assert_eq!(frame(&run, 3, |f| f.seq >= 4).speed, 0.5);

    let started = Instant::now();
    drop(run);
    let took = started.elapsed();
    assert!(dropped.load(Ordering::SeqCst), "the thread returned and dropped its exhibits before the drop returned");
    assert!(took < JOIN_BOUND, "dropping the run thread took {took:?} (bound {JOIN_BOUND:?})");
}

/// An `--exhibit` no title matches opens the first and says so.
#[test]
fn an_unknown_selector_opens_the_first_exhibit_with_a_notice() {
    let run = run::spawn_with(Some("nope".into()), 1, || vec![Box::new(Fake::new("Only")) as Box<dyn Exhibit>]);
    let first = frame(&run, 1, |f| !f.catalogue.is_empty());
    assert_eq!(first.current, 0);
    let notice = first.notice.expect("a notice");
    assert!(notice.contains("`nope`") && notice.contains("exhibit 1 is shown"), "{notice}");
    // Building nothing is reported, not hidden.
    let empty = run::spawn_with(None, 1, Vec::new);
    let failed = frame(&empty, 1, |f| f.failed.is_some());
    assert!(failed.catalogue.is_empty() && failed.failed.unwrap().contains("no exhibits"));
}

/// Every control `system_ui` lists fits a registered pattern, its REST form
/// parses back into its action, and a disabled one names why (before the
/// exhibits are built; at the knob's limit).
#[test]
fn every_control_fits_a_pattern_and_names_why_it_is_disabled() {
    use super::actions::PhenomenaAction;
    use super::gallery::{Gallery, controls, rest_form};
    use crate::app::actions::{Action, control_matches};
    let mut g = Gallery::with_run(run::spawn_with(None, 1, || vec![Box::new(Fake::new("Only")) as Box<dyn Exhibit>, Box::new(Fake::new("Other"))]));
    // No frame taken yet: only the fixed controls, each disabled naming why.
    let early = controls(&g);
    assert_eq!(early.len(), 8);
    assert!(early.iter().all(|c| c.ready.as_ref().is_err_and(|e| e.contains("still being built"))));
    let started = Instant::now();
    while g.ready().is_none() {
        assert!(started.elapsed() < Duration::from_secs(5), "no frame");
        g.receive();
        std::thread::sleep(Duration::from_millis(2));
    }
    let patterns = <PhenomenaAction as Action>::controls();
    let all = controls(&g);
    assert_eq!(all.len(), 2 + 8);
    for c in &all {
        assert!(patterns.iter().any(|p| control_matches(p, &c.id)), "{} fits no pattern", c.id);
        let parsed: PhenomenaAction = serde_json::from_value(rest_form(&c.action)).unwrap_or_else(|e| panic!("{}: {e}", c.id));
        assert_eq!(parsed, c.action, "{}", c.id);
        assert!(c.ready.is_ok(), "{} is enabled at knob 1.0 of 0…2 and speed ×1", c.id);
    }
    // At the knob's maximum, knob_up is disabled naming it.
    let seq = g.send(Op::Knob { value: Some(2.0), steps: None }).unwrap();
    let started = Instant::now();
    while g.applied() < seq {
        assert!(started.elapsed() < Duration::from_secs(5), "the knob command was not applied");
        g.receive();
        std::thread::sleep(Duration::from_millis(2));
    }
    let up = controls(&g).into_iter().find(|c| c.id == "phenomena:knob_up").unwrap();
    assert_eq!(up.ready, Err("the knob is at its maximum, 2 m".to_string()));
}
