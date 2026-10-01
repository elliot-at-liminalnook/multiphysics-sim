//! The phenomena run thread: a `jobs::RunThread` ("phenomena-run") that
//! builds every exhibit (`sim_phenomena::exhibits::all()`, off the UI thread)
//! and owns them, advances the current one on its own clock with sim-app's
//! pacing ([`Pacing::step`], `crates/sim-app/src/phenomena_app.rs`
//! `advance()`), applies [`Command`]s and publishes a [`Frame`] after each
//! command and each tick that changed something.
//!
//! The loop checks its channel before every tick (a closed channel is the
//! stop signal), so a drop joins within `jobs::JOIN_BOUND` unless one
//! `Exhibit::advance` call itself takes longer; the owner drops it off the UI
//! thread (`phenomena::leave`), so the UI never waits on that.
use super::ExhibitRef;
use crate::jobs::{RunThread, Stamped};
use sim_phenomena::exhibit::{Exhibit, Knob, Readout, Shape};
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The strip chart: one sample per 1/30 s of real time on ticks that advance
/// (paused, stopped and carried ticks add none), the last 1800
/// (phenomena_app.rs:49–51).
pub(crate) const CHART_POINTS: usize = 1800;
pub(crate) const CHART_INTERVAL: f64 = 1.0 / 30.0;
/// Real seconds one tick may advance at most (phenomena_app.rs:186): a slow
/// tick never jumps the simulation.
pub(crate) const MAX_REAL: f64 = 0.05;
/// The speed multiplier's range (phenomena_app.rs:172–173).
pub(crate) const MIN_SPEED: f64 = 1.0 / 64.0;
pub(crate) const MAX_SPEED: f64 = 64.0;
/// About 60 ticks a second.
const TICK: Duration = Duration::from_micros(16_667);

/// What the UI asks of the run thread.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    /// Show exhibit `index` (0-based; the UI resolved it against the catalogue).
    Select(usize),
    /// The next (+1) or previous (−1) exhibit, wrapping.
    Step(i64),
    /// Set the knob to `value`, or nudge it by `steps` knob steps.
    Knob { value: Option<f64>, steps: Option<f64> },
    Reset,
    /// Pause, run, or toggle (None).
    Pause(Option<bool>),
    /// Set the speed, or double/halve it `steps` times.
    Speed { speed: Option<f64>, steps: Option<i32> },
}
impl Op {
    /// A switch (sim-app's `switched`): the chart, its clock, the grid
    /// accumulator and the error restart, and the generation is bumped.
    pub(crate) fn switches(&self) -> bool {
        matches!(self, Op::Select(_) | Op::Step(_) | Op::Knob { .. } | Op::Reset)
    }
}

/// A command: `seq` numbers every command the owner sent; `generation` is the
/// owner's requested generation (bumped by every switch).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Command {
    pub(crate) seq: u64,
    pub(crate) generation: u64,
    pub(crate) op: Op,
}

/// An exhibit as the catalogue lists it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Entry {
    pub(crate) title: &'static str,
    pub(crate) summary: &'static str,
}

/// What the run thread publishes: the current exhibit as it is now.
#[derive(Clone, Debug, Default)]
pub(crate) struct Frame {
    /// The generation of the last command applied (a switch bumps it).
    pub(crate) generation: u64,
    /// The `seq` of the last command applied.
    pub(crate) seq: u64,
    /// Bumped on every publish.
    pub(crate) tick: u64,
    /// Every exhibit, filled from the first frame on (empty: not built).
    pub(crate) catalogue: Vec<Entry>,
    pub(crate) current: usize,
    pub(crate) knob: Option<Knob>,
    pub(crate) readouts: Vec<Readout>,
    pub(crate) verdict: String,
    pub(crate) signal: (&'static str, f64),
    /// The strip chart's samples (30 Hz of real time, the last 1800).
    pub(crate) chart: Vec<f64>,
    /// Samples pushed since the start (the chart changed when this did).
    pub(crate) chart_count: u64,
    pub(crate) time: f64,
    pub(crate) time_unit: &'static str,
    pub(crate) speed: f64,
    pub(crate) paused: bool,
    /// The exhibit's simulation error, verbatim (the run stops until a switch).
    pub(crate) error: Option<String>,
    /// An `--exhibit` selector no exhibit matched (exhibit 1 was opened).
    pub(crate) notice: Option<String>,
    /// Building the exhibits failed (the catalogue stays empty).
    pub(crate) failed: Option<String>,
    pub(crate) shapes: Vec<Shape>,
}
impl Stamped for Frame {
    fn generation(&self) -> u64 {
        self.generation
    }
}
impl Frame {
    pub(crate) fn entry(&self) -> Entry {
        self.catalogue.get(self.current).copied().unwrap_or_default()
    }
    pub(crate) fn titles(&self) -> Vec<&'static str> {
        self.catalogue.iter().map(|e| e.title).collect()
    }
}

/// What one tick did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stepped {
    /// Paused, stopped on an error, or no time to advance.
    Skipped,
    /// A gridded exhibit's time was carried (less than one whole step).
    Carried,
    Advanced,
    /// `advance` failed (or panicked): the error is kept and the run stops.
    Failed,
}

/// sim-app's pacing state (its `Gallery` minus the exhibits), with its one
/// rule, [`Pacing::step`], as a pure function of the exhibit and the real
/// time since the last tick.
#[derive(Clone, Debug)]
pub(crate) struct Pacing {
    pub(crate) paused: bool,
    pub(crate) speed: f64,
    pub(crate) accumulator: f64,
    pub(crate) chart: Vec<f64>,
    pub(crate) chart_clock: f64,
    pub(crate) chart_count: u64,
    pub(crate) error: Option<String>,
    /// Wall seconds spent advancing, simulated seconds advanced and real
    /// seconds: `SIM_VIEWER_STATS=1` prints them once a second.
    stats: (f64, f64, f64),
    report: bool,
}
impl Default for Pacing {
    fn default() -> Self {
        Self { paused: false, speed: 1.0, accumulator: 0.0, chart: Vec::new(), chart_clock: 0.0, chart_count: 0, error: None, stats: (0.0, 0.0, 0.0), report: std::env::var_os("SIM_VIEWER_STATS").is_some() }
    }
}
impl Pacing {
    /// sim-app's `switched` block (phenomena_app.rs:174–179).
    pub(crate) fn switched(&mut self) {
        self.chart.clear();
        self.chart_clock = 0.0;
        self.accumulator = 0.0;
        self.error = None;
    }

    /// One tick, exactly sim-app's `advance()` (phenomena_app.rs:182–222):
    /// `real` (wall seconds since the last tick) clamped to 0.05; `dt = real
    /// × time_scale × speed`; nothing while paused or stopped on an error
    /// or when `dt <= 0`; a gridded exhibit accumulates and takes whole grid
    /// steps, carrying the remainder; an `advance` error is kept verbatim
    /// and stops the run until a switch; after a successful advance the
    /// chart clock gains `real` and, once it reaches 1/30 s, one sample of
    /// `signal()` is pushed (at most one per tick) and the chart keeps its
    /// last 1800. A panic in `advance` is kept as the error too (sim-app
    /// would have crashed).
    pub(crate) fn step(&mut self, exhibit: &mut dyn Exhibit, real: f64) -> Stepped {
        if self.paused || self.error.is_some() {
            return Stepped::Skipped;
        }
        let scale = exhibit.time_scale() * self.speed;
        let real = real.min(MAX_REAL);
        let mut dt = real * scale;
        if dt <= 0.0 {
            return Stepped::Skipped;
        }
        // A gridded exhibit takes whole steps: the remainder carries over.
        let grid = exhibit.grid();
        if grid > 0.0 {
            self.accumulator += dt;
            let whole = (self.accumulator / grid).floor();
            if whole < 1.0 {
                return Stepped::Carried;
            }
            dt = whole * grid;
            self.accumulator -= dt;
        }
        let started = Instant::now();
        match catch_unwind(AssertUnwindSafe(|| exhibit.advance(dt))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                self.error = Some(error);
                return Stepped::Failed;
            }
            Err(panic) => {
                self.error = Some(format!("the exhibit's simulation panicked: {}", panic_text(&*panic)));
                return Stepped::Failed;
            }
        }
        self.stats.0 += started.elapsed().as_secs_f64();
        self.stats.1 += dt;
        self.stats.2 += real;
        if self.stats.2 >= 1.0 {
            if self.report {
                let (wall, sim, real) = self.stats;
                eprintln!("viewer: {:.3} s simulated in {:.3} s of {:.3} s wall ({:.2}× real time), sim t = {:.2}", sim, wall, real, sim / real, exhibit.time());
            }
            self.stats = (0.0, 0.0, 0.0);
        }
        self.chart_clock += real;
        if self.chart_clock >= CHART_INTERVAL {
            self.chart_clock -= CHART_INTERVAL;
            let (_, value) = exhibit.signal();
            self.chart.push(value);
            self.chart_count += 1;
        }
        if self.chart.len() > CHART_POINTS {
            let n = self.chart.len() - CHART_POINTS;
            self.chart.drain(..n);
        }
        Stepped::Advanced
    }
}

/// The knob value a `Knob` op asks for (phenomena_app.rs:156–164): `value`,
/// or the current value plus `steps` knob steps (←/→ send ±1, Shift ±5:
/// sim-app's `step * 5`), clamped to [min, max] and rounded to the step.
pub(crate) fn knob_target(knob: &Knob, value: Option<f64>, steps: Option<f64>) -> f64 {
    let raw = match (value, steps) {
        (Some(v), _) => v,
        (None, Some(n)) => knob.value + n * knob.step,
        (None, None) => knob.value,
    };
    // `f64::clamp` panics on an inverted range; an exhibit's knob never has one, but a panic here would end the run thread.
    let v = if knob.min <= knob.max { raw.clamp(knob.min, knob.max) } else { raw };
    // sim-app divided by the step unconditionally (NaN for a zero step).
    if knob.step > 0.0 { (v / knob.step).round() * knob.step } else { v }
}

/// The speed a `Speed` op asks for (phenomena_app.rs:172–173): `speed`, or
/// doubled (`steps` 1) / halved (−1) per step, clamped to [1/64, 64].
pub(crate) fn speed_target(current: f64, speed: Option<f64>, steps: Option<i32>) -> f64 {
    let raw = match (speed, steps) {
        (Some(s), _) => s,
        (None, Some(n)) => current * 2f64.powi(n),
        (None, None) => current,
    };
    raw.clamp(MIN_SPEED, MAX_SPEED)
}

fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
}

/// The thread's own state: the exhibits and the pacing.
struct Run {
    exhibits: Vec<Box<dyn Exhibit>>,
    catalogue: Vec<Entry>,
    current: usize,
    pacing: Pacing,
    generation: u64,
    seq: u64,
    tick: u64,
    notice: Option<String>,
}

impl Run {
    fn apply(&mut self, command: Command) {
        self.seq = command.seq;
        self.generation = self.generation.max(command.generation);
        let count = self.exhibits.len();
        if command.op.switches() {
            self.pacing.switched();
            self.notice = None;
        }
        match command.op {
            Op::Select(index) => {
                if index < count {
                    self.current = index;
                }
            }
            Op::Step(delta) => self.current = (self.current as i64 + delta).rem_euclid(count as i64) as usize,
            Op::Knob { value, steps } => {
                let exhibit = &mut self.exhibits[self.current];
                let target = knob_target(&exhibit.knob(), value, steps);
                if let Err(panic) = catch_unwind(AssertUnwindSafe(|| exhibit.set_knob(target))) {
                    self.pacing.error = Some(format!("the exhibit panicked rebuilding at the new knob value: {}", panic_text(&*panic)));
                }
            }
            Op::Reset => {
                let exhibit = &mut self.exhibits[self.current];
                if let Err(panic) = catch_unwind(AssertUnwindSafe(|| exhibit.reset())) {
                    self.pacing.error = Some(format!("the exhibit panicked on reset: {}", panic_text(&*panic)));
                }
            }
            Op::Pause(paused) => self.pacing.paused = paused.unwrap_or(!self.pacing.paused),
            Op::Speed { speed, steps } => self.pacing.speed = speed_target(self.pacing.speed, speed, steps),
        }
    }

    /// The current exhibit as it is now. A panic while reading it is kept
    /// as the run's error (so it is not read again until a switch).
    fn frame(&mut self) -> Frame {
        self.tick += 1;
        let mut frame = Frame {
            generation: self.generation,
            seq: self.seq,
            tick: self.tick,
            catalogue: self.catalogue.clone(),
            current: self.current,
            chart: self.pacing.chart.clone(),
            chart_count: self.pacing.chart_count,
            speed: self.pacing.speed,
            paused: self.pacing.paused,
            notice: self.notice.clone(),
            ..Default::default()
        };
        let exhibit = &self.exhibits[self.current];
        let read = catch_unwind(AssertUnwindSafe(|| {
            let mut shapes = Vec::with_capacity(256);
            exhibit.shapes(&mut shapes);
            (exhibit.knob(), exhibit.readouts(), exhibit.verdict(), exhibit.signal(), exhibit.time(), exhibit.time_unit(), shapes)
        }));
        match read {
            Ok((knob, readouts, verdict, signal, time, time_unit, shapes)) => {
                frame.knob = Some(knob);
                frame.readouts = readouts;
                frame.verdict = verdict;
                frame.signal = signal;
                frame.time = time;
                frame.time_unit = time_unit;
                frame.shapes = shapes;
            }
            Err(panic) => {
                if self.pacing.error.is_none() {
                    self.pacing.error = Some(format!("the exhibit panicked while being drawn: {}", panic_text(&*panic)));
                }
            }
        }
        frame.error = self.pacing.error.clone();
        frame
    }
}

fn publish(shared: &Mutex<Frame>, frame: Frame) {
    *shared.lock().unwrap_or_else(|p| p.into_inner()) = frame;
}

/// The run thread on the built-in exhibits, opening `selector` (an
/// `--exhibit` text; None: the first). Its first frame has `generation`.
pub(crate) fn spawn(selector: Option<String>, generation: u64) -> RunThread<Command, Frame> {
    spawn_with(selector, generation, sim_phenomena::exhibits::all)
}

/// The run thread on the exhibits `build` makes (on the thread).
pub(crate) fn spawn_with(selector: Option<String>, generation: u64, build: impl FnOnce() -> Vec<Box<dyn Exhibit>> + Send + 'static) -> RunThread<Command, Frame> {
    RunThread::spawn("phenomena-run", Frame::default(), move |commands, shared| run(&commands, &shared, selector, generation, build))
}

fn run(commands: &Receiver<Command>, shared: &Arc<Mutex<Frame>>, selector: Option<String>, generation: u64, build: impl FnOnce() -> Vec<Box<dyn Exhibit>>) {
    let built = catch_unwind(AssertUnwindSafe(build)).map_err(|panic| format!("building the exhibits panicked: {}", panic_text(&*panic)));
    let exhibits = match built {
        Ok(exhibits) if !exhibits.is_empty() => exhibits,
        other => {
            let failed = other.err().unwrap_or_else(|| "sim_phenomena::exhibits::all() returned no exhibits".into());
            publish(shared, Frame { generation, failed: Some(failed), ..Default::default() });
            // Nothing to run: wait for the owner to close the channel.
            while commands.recv().is_ok() {}
            return;
        }
    };
    let catalogue: Vec<Entry> = exhibits.iter().map(|e| Entry { title: e.title(), summary: e.summary() }).collect();
    let titles: Vec<&str> = catalogue.iter().map(|e| e.title).collect();
    let (current, notice) = match selector.as_deref().map(|s| ExhibitRef::parse(s).resolve(&titles)) {
        None => (0, None),
        Some(Ok(index)) => (index, None),
        Some(Err(e)) => (0, Some(format!("--exhibit: {e}; exhibit 1 is shown"))),
    };
    let mut run = Run { exhibits, catalogue, current, pacing: Pacing::default(), generation, seq: 0, tick: 0, notice };
    publish(shared, run.frame());
    let mut last = Instant::now();
    let mut next = last + TICK;
    loop {
        // Every waiting command first: a closed channel stops the loop even
        // when ticks run late (a slow exhibit never starves the channel).
        loop {
            match commands.try_recv() {
                Ok(command) => {
                    run.apply(command);
                    publish(shared, run.frame());
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let now = Instant::now();
        if now < next {
            match commands.recv_timeout(next - now) {
                Ok(command) => {
                    run.apply(command);
                    publish(shared, run.frame());
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        let now = Instant::now();
        let real = now.duration_since(last).as_secs_f64();
        last = now;
        next = now + TICK;
        let current = run.current;
        // `step` catches a panic in `advance`; this catches one in the
        // exhibit's other calls (time_scale, grid, signal, time), so the
        // thread never dies with the last frame still showing.
        let stepped = match catch_unwind(AssertUnwindSafe(|| run.pacing.step(&mut *run.exhibits[current], real))) {
            Ok(stepped) => stepped,
            Err(panic) => {
                run.pacing.error = Some(format!("the exhibit's simulation panicked: {}", panic_text(&*panic)));
                Stepped::Failed
            }
        };
        if matches!(stepped, Stepped::Advanced | Stepped::Failed) {
            publish(shared, run.frame());
        }
    }
}
