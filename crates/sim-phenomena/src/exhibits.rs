#![allow(clippy::new_without_default)]
//! One live [`Exhibit`] per phenomenon, each driving the same compiled
//! model the acceptance scenario builds, and reading it only through the
//! `StateStore`.

use crate::exhibit::{paint, spring_polyline, Exhibit, Knob, Readout, Shape};
use crate::scenarios::*;
use crate::world::{registry, runtime};
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, StateId};
use std::f64::consts::{PI, TAU};

pub fn all() -> Vec<Box<dyn Exhibit>> {
    vec![
        Box::new(Kapitza::new()),
        Box::new(Huygens::new()),
        Box::new(Hogging::new()),
        Box::new(Rijke::new()),
        Box::new(Hammer::new()),
        Box::new(Flutter::new()),
        Box::new(Dzhanibekov::new()),
        Box::new(Tippe::new()),
        Box::new(Walker::new()),
        Box::new(Buckling::new()),
        Box::new(Spring::new()),
        Box::new(Belt::new()),
        Box::new(Backlash::new()),
        Box::new(Sampled::new()),
        Box::new(ChuaScroll::new()),
        Box::new(Zener::new()),
        Box::new(Painleve::new()),
        Box::new(MotorHogging::new()),
        Box::new(LevitronExhibit::new()),
        Box::new(GeyserExhibit::new()),
        Box::new(SemenovExhibit::new()),
        Box::new(SkyCoolingExhibit::new()),
        Box::new(VivExhibit::new()),
        Box::new(JanssenExhibit::new()),
        Box::new(StochasticResonanceExhibit::new()),
        Box::new(DoublePendulumExhibit::new()),
        Box::new(LanguageExhibit::new()),
        Box::new(LatencyExhibit::new()),
        Box::new(HuntExhibit::new()),
        Box::new(DeadlineExhibit::new()),
        Box::new(LegExhibit::new()),
        Box::new(QuadrupedExhibit::new()),
        Box::new(LadderExhibit::new()),
        Box::new(CruiseExhibit::new()),
        Box::new(PlankExhibit::new()),
    ]
}

fn knob(label: &'static str, unit: &'static str, min: f64, max: f64, step: f64, value: f64) -> Knob {
    Knob { label, unit, min, max, step, value }
}

fn advance(rt: &mut Runtime, duration: f64, h: f64) -> Result<(), String> {
    rt.advance(duration, h).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- 1. Kapitza
pub struct Kapitza {
    registry: BehaviorRegistry,
    pendulum: kapitza_pendulum::DrivenPendulum,
    runtime: Runtime,
    angle: StateId,
}
impl Kapitza {
    pub fn new() -> Self {
        let registry = registry();
        let pendulum = kapitza_pendulum::DrivenPendulum { length: 0.2, gravity: 9.81, drive_amplitude: 0.01, drive_frequency: TAU * 50.0 };
        let (runtime, angle) = pendulum.model(&registry, 0.12);
        Self { registry, pendulum, runtime, angle }
    }
}
impl Exhibit for Kapitza {
    fn title(&self) -> &'static str { "Kapitza's inverted pendulum" }
    fn summary(&self) -> &'static str { "Above the threshold the bob stands upright on a shaking pivot and rocks slowly about vertical; below it, it falls." }
    fn knob(&self) -> Knob { knob("pivot drive frequency", "Hz", 10.0, 90.0, 2.0, self.pendulum.drive_frequency / TAU) }
    fn set_knob(&mut self, value: f64) { self.pendulum.drive_frequency = TAU * value; self.reset(); }
    fn reset(&mut self) { let (rt, angle) = self.pendulum.model(&self.registry, 0.12); self.runtime = rt; self.angle = angle; }
    fn time(&self) -> f64 { self.runtime.time }
    fn time_scale(&self) -> f64 { 0.35 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        let h = TAU / self.pendulum.drive_frequency / 200.0;
        advance(&mut self.runtime, duration, h)
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let p = &self.pendulum;
        let scale = 10.0;
        let pivot_y = p.drive_amplitude * (p.drive_frequency * self.runtime.time).cos() * scale;
        let phi = self.runtime.get(self.angle);
        let l = p.length * scale;
        let bob = [l * phi.sin(), pivot_y + l * phi.cos(), 0.0];
        out.push(Shape::block([0.0, -0.6, 0.0], [1.6, 0.05, 1.0], paint::GROUND));
        out.push(Shape::Rod { from: [0.0, -0.6, 0.0], to: [0.0, pivot_y, 0.0], radius: 0.05, color: paint::STEEL });
        out.push(Shape::block([0.0, pivot_y, 0.0], [0.18, 0.06, 0.18], paint::INK));
        out.push(Shape::Rod { from: [0.0, pivot_y, 0.0], to: bob, radius: 0.035, color: paint::INK });
        out.push(Shape::Sphere { center: bob, radius: 0.16, color: paint::SURPRISE });
        out.push(Shape::Line { from: [0.0, pivot_y, 0.0], to: [0.0, pivot_y + l, 0.0], color: paint::STEEL });
    }
    fn readouts(&self) -> Vec<Readout> {
        let p = &self.pendulum;
        let mut r = vec![
            Readout::new("angle from upright", self.runtime.get(self.angle), "rad"),
            Readout::new("a²Ω²", (p.drive_amplitude * p.drive_frequency).powi(2), "m²/s²"),
            Readout::new("2gL", 2.0 * p.gravity * p.length, "m²/s²"),
        ];
        if p.inverted_is_stable() {
            r.push(Readout::new("predicted slow frequency", p.slow_frequency() / TAU, "Hz"));
        }
        r
    }
    fn signal(&self) -> (&'static str, f64) { ("angle from upright (rad)", self.runtime.get(self.angle)) }
    fn verdict(&self) -> String {
        if self.pendulum.inverted_is_stable() { "a²Ω² > 2gL — upright is stable".into() } else { "a²Ω² < 2gL — upright is unstable".into() }
    }
}

// ---------------------------------------------------------------- 2. Huygens
pub struct Huygens {
    registry: BehaviorRegistry,
    clocks: huygens_clocks::ClocksOnBeam,
    model: huygens_clocks::Clocks,
}
impl Huygens {
    pub fn new() -> Self {
        let registry = registry();
        let clocks = huygens_clocks::ClocksOnBeam::default();
        let model = clocks.model(&registry, 0.5);
        Self { registry, clocks, model }
    }
    fn phase(&self, angle: f64, rate: f64) -> f64 {
        let omega = (self.clocks.gravity / self.clocks.pendulum_length).sqrt();
        (-rate / omega).atan2(angle)
    }
    fn phase_difference(&self) -> f64 {
        let m = &self.model;
        let d = self.phase(m.runtime.get(m.angles[1]), m.runtime.get(m.rates[1])) - self.phase(m.runtime.get(m.angles[0]), m.runtime.get(m.rates[0]));
        ((d + PI).rem_euclid(TAU) - PI).abs()
    }
}
impl Exhibit for Huygens {
    fn title(&self) -> &'static str { "Huygens' coupled clocks" }
    fn summary(&self) -> &'static str { "Two escapement clocks on one compliant beam drift into exact anti-phase over about forty minutes (press up-arrow to hurry it); a heavy beam passes too little momentum to do it." }
    fn knob(&self) -> Knob { knob("beam mass", "kg", 1.0, 40.0, 1.0, self.clocks.beam_mass) }
    fn set_knob(&mut self, value: f64) {
        let f = 1.0;
        self.clocks.beam_mass = value;
        self.clocks.beam_stiffness = value * (TAU * f).powi(2);
        self.clocks.beam_damping = 2.0 * 0.5 * value * TAU * f;
        self.reset();
    }
    fn reset(&mut self) { self.model = self.clocks.model(&self.registry, 0.5); }
    fn time(&self) -> f64 { self.model.runtime.time }
    fn time_scale(&self) -> f64 { 3.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.model.runtime, duration, 2.0e-3) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let m = &self.model;
        let beam_x = m.runtime.get(m.beam) * 2000.0;
        let y = 1.4;
        out.push(Shape::block([beam_x, y, 0.0], [1.9, 0.06, 0.25], paint::STEEL));
        for x in [-2.6, 2.6] {
            out.push(Shape::Rod { from: [x, -0.8, 0.0], to: [x, y + 0.2, 0.0], radius: 0.05, color: paint::GROUND });
            out.push(Shape::Line { from: [x, y + 0.2, 0.0], to: [beam_x + x * 0.73, y, 0.0], color: paint::GROUND });
        }
        for (i, color) in [paint::SURPRISE, paint::CONTROL].into_iter().enumerate() {
            let angle = m.runtime.get(m.angles[i]);
            let px = beam_x + if i == 0 { -1.0 } else { 1.0 };
            let l = 1.6;
            let bob = [px + l * angle.sin(), y - l * angle.cos(), 0.0];
            out.push(Shape::Rod { from: [px, y, 0.0], to: bob, radius: 0.03, color: paint::INK });
            out.push(Shape::Sphere { center: bob, radius: 0.14, color });
        }
        out.push(Shape::block([0.0, -0.85, 0.0], [3.2, 0.05, 1.0], paint::GROUND));
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![
            Readout::new("|phase difference|", self.phase_difference(), "rad"),
            Readout::new("anti-phase would be", PI, "rad"),
            Readout::new("beam excursion", self.model.runtime.get(self.model.beam) * 1.0e6, "µm"),
            Readout::new("escapement kicks so far", self.model.runtime.events() as f64, ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("|phase difference| (rad)", self.phase_difference()) }
    fn verdict(&self) -> String { format!("beam {:.0}× a pendulum mass, drawn with ×2000 beam motion", self.clocks.beam_mass / self.clocks.pendulum_mass) }
}

// ---------------------------------------------------------------- 3. Current hogging
pub struct Hogging {
    registry: BehaviorRegistry,
    coefficient: f64,
    pair: current_hogging::ParallelPair,
    board: current_hogging::Board,
}
impl Hogging {
    const POWER: f64 = 7.5;
    fn pair(coefficient: f64) -> current_hogging::ParallelPair {
        let (resistance, thermal_resistance, ambient) = (1.0, 10.0, 300.0);
        let operating = ambient + Self::POWER * thermal_resistance;
        let r_bar = resistance * (coefficient * (operating - ambient)).exp();
        current_hogging::ParallelPair { total_current: (4.0 * Self::POWER / r_bar).sqrt(), resistance, coefficient, thermal_resistance, heat_capacity: 1.0, coupling: 0.0, ambient }
    }
    pub fn new() -> Self {
        let registry = registry();
        let pair = Self::pair(-0.02);
        let board = pair.model(&registry, 0.5);
        Self { registry, coefficient: -0.02, pair, board }
    }
    fn share(&self) -> f64 {
        let b = &self.board;
        b.runtime.get(b.v) / self.pair.device_resistance(b.runtime.get(b.t1)) / self.pair.total_current
    }
}
impl Exhibit for Hogging {
    fn title(&self) -> &'static str { "Current hogging" }
    fn summary(&self) -> &'static str { "Two identical devices share one current; with a negative coefficient and loop gain above one, the hotter one takes it all." }
    fn knob(&self) -> Knob { knob("temperature coefficient α (at 7.5 W per device)", "1/K", -0.03, 0.012, 0.002, self.coefficient) }
    fn set_knob(&mut self, value: f64) { self.coefficient = value; self.pair = Self::pair(value); self.reset(); }
    fn reset(&mut self) { self.board = self.pair.model(&self.registry, 0.5); }
    fn time(&self) -> f64 { self.board.runtime.time }
    fn time_scale(&self) -> f64 { 25.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.board.runtime, duration, 0.02) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let b = &self.board;
        let temps = [b.runtime.get(b.t1), b.runtime.get(b.t2)];
        let s1 = self.share();
        let share = [s1, 1.0 - s1];
        for i in 0..2 {
            let x = if i == 0 { -1.1 } else { 1.1 };
            let heat = ((temps[i] - self.pair.ambient) / 120.0) as f32;
            out.push(Shape::block([x, 0.0, 0.0], [0.5, 1.0, 0.5], paint::heat(heat)));
            let n = (share[i].clamp(0.0, 1.0) * 12.0).round() as usize;
            for k in 0..n {
                let y = -0.9 + 1.8 * (k as f64 + 0.5) / 12.0;
                out.push(Shape::Arrow { from: [x - 1.0, y, 0.6], to: [x - 0.55, y, 0.6], color: paint::CONTROL });
            }
        }
        out.push(Shape::Rod { from: [-2.6, 0.0, 0.0], to: [-1.6, 0.0, 0.0], radius: 0.06, color: paint::COPPER });
        out.push(Shape::Rod { from: [-1.6, -1.2, 0.0], to: [-1.6, 1.2, 0.0], radius: 0.04, color: paint::COPPER });
        out.push(Shape::Rod { from: [1.6, -1.2, 0.0], to: [1.6, 1.2, 0.0], radius: 0.04, color: paint::COPPER });
        out.push(Shape::Rod { from: [1.6, 0.0, 0.0], to: [2.6, 0.0, 0.0], radius: 0.06, color: paint::COPPER });
        out.push(Shape::block([0.0, -1.35, 0.0], [3.0, 0.05, 1.2], paint::GROUND));
    }
    fn readouts(&self) -> Vec<Readout> {
        let b = &self.board;
        vec![
            Readout::new("device 1 share of I", self.share(), ""),
            Readout::new("T₁", b.runtime.get(b.t1), "K"),
            Readout::new("T₂", b.runtime.get(b.t2), "K"),
            Readout::new("loop gain |α|·R_th·P", self.pair.coefficient.abs() * self.pair.thermal_resistance * Self::POWER, ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("device 1 share of current", self.share()) }
    fn verdict(&self) -> String {
        let gain = self.pair.coefficient.abs() * self.pair.thermal_resistance * Self::POWER;
        if self.pair.coefficient >= 0.0 { "α ≥ 0 — self-balancing".into() }
        else if gain > 1.0 { "α < 0 and gain > 1 — even split is unstable, one device hogs".into() }
        else { "α < 0 but gain < 1 — still balances".into() }
    }
}

// ---------------------------------------------------------------- 4. Rijke tube
pub struct Rijke {
    registry: BehaviorRegistry,
    tube: rijke_tube::RijkeTube,
    model: rijke_tube::Tube,
}
impl Rijke {
    pub fn new() -> Self {
        let registry = registry();
        let tube = rijke_tube::RijkeTube::default();
        let model = tube.model(&registry, 0.05);
        Self { registry, tube, model }
    }
    fn eta_dot(&self) -> Vec<f64> {
        self.model.eta_dot.iter().map(|id| self.model.runtime.get(*id)).collect()
    }
}
impl Exhibit for Rijke {
    fn title(&self) -> &'static str { "The Rijke tube" }
    fn summary(&self) -> &'static str { "Steady heat from a gauze in the lower half of an open tube grows into a pure tone; move it to the upper half and it dies away." }
    fn knob(&self) -> Knob { knob("heater position", "L", 0.05, 0.95, 0.05, self.tube.heater_position) }
    fn set_knob(&mut self, value: f64) { self.tube.heater_position = value; self.reset(); }
    fn reset(&mut self) { self.model = self.tube.model(&self.registry, 0.05); }
    fn time(&self) -> f64 { self.model.runtime.time }
    fn time_unit(&self) -> &'static str { "L/c" }
    fn time_scale(&self) -> f64 { 3.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.model.runtime, duration, 2.0e-3) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let eta_dot = self.eta_dot();
        let (y0, y1) = (-1.5, 1.5);
        for dx in [-0.45, 0.45] {
            out.push(Shape::Rod { from: [dx, y0, 0.0], to: [dx, y1, 0.0], radius: 0.03, color: paint::STEEL });
        }
        let mut left = Vec::new();
        let mut right = Vec::new();
        for k in 0..=40 {
            let s = k as f64 / 40.0;
            let p = self.tube.pressure_from_modes(&eta_dot, s).clamp(-2.5, 2.5) * 0.16;
            let y = y0 + s * (y1 - y0);
            left.push([-p, y, 0.0]);
            right.push([p, y, 0.0]);
        }
        out.push(Shape::Polyline { points: left, color: paint::SURPRISE });
        out.push(Shape::Polyline { points: right, color: paint::SURPRISE });
        let hy = y0 + self.tube.heater_position * (y1 - y0);
        let glow = (self.model.runtime.get(self.model.heat) * 2.0).clamp(-1.0, 1.0) as f32;
        out.push(Shape::block([0.0, hy, 0.0], [0.42, 0.03, 0.3], paint::heat(glow.max(0.15))));
        out.push(Shape::block([0.0, y0 - 0.1, 0.0], [1.5, 0.05, 1.0], paint::GROUND));
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![
            Readout::new("pressure at L/4", self.tube.pressure_from_modes(&self.eta_dot(), 0.25), ""),
            Readout::new("heat release", self.model.runtime.get(self.model.heat), ""),
            Readout::new("fundamental f = c/2L", 0.5, "c/L"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("pressure at L/4", self.tube.pressure_from_modes(&self.eta_dot(), 0.25)) }
    fn verdict(&self) -> String {
        let p = self.tube.heater_position;
        if p < 0.5 { "lower half — Rayleigh index positive for the fundamental: it sings".into() }
        else if p < 0.75 { "upper half — fundamental damped; the second harmonic may sing".into() }
        else { "upper quarter — silent".into() }
    }
}

// ---------------------------------------------------------------- 5. Water hammer
pub struct Hammer {
    registry: BehaviorRegistry,
    pipe: water_hammer::Pipeline,
    model: water_hammer::Pipe,
}
impl Hammer {
    pub fn new() -> Self {
        let registry = registry();
        let pipe = water_hammer::Pipeline::new(40, 0.02);
        let model = pipe.model(&registry);
        Self { registry, pipe, model }
    }
    fn seated(&self) -> bool { self.model.runtime.get(self.model.seated) > 0.5 }
}
impl Exhibit for Hammer {
    fn title(&self) -> &'static str { "Water hammer" }
    fn summary(&self) -> &'static str { "Close the valve faster than the round-trip wave time and the pressure at the valve jumps by ρ·c·Δv and rings; close it slowly and nothing much happens." }
    fn knob(&self) -> Knob { knob("valve closure time", "s", 0.01, 2.0, 0.01, self.pipe.closure_time) }
    fn set_knob(&mut self, value: f64) { self.pipe.closure_time = value; self.reset(); }
    fn reset(&mut self) { self.model = self.pipe.model(&self.registry); }
    fn time(&self) -> f64 { self.model.runtime.time }
    fn time_scale(&self) -> f64 { 0.06 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.model.runtime, duration, 1.0e-3) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let p = &self.pipe;
        let n = p.cells;
        let (x0, x1) = (-2.6, 2.4);
        let w = (x1 - x0) / n as f64;
        let jou = p.joukowsky();
        for (i, id) in self.model.pressures.iter().enumerate() {
            let rise = (self.model.runtime.get(*id) - p.reservoir_pressure) / jou;
            out.push(Shape::block([x0 + (i as f64 + 0.5) * w, 0.0, 0.0], [w * 0.5, 0.22, 0.22], paint::heat(rise as f32)));
        }
        out.push(Shape::block([x0 - 0.5, 0.15, 0.0], [0.4, 0.55, 0.5], paint::CONTROL));
        let opening = if self.seated() { 0.0 } else { (1.0 - self.model.runtime.time / p.closure_time).clamp(0.0, 1.0) };
        out.push(Shape::block([x1 + 0.12, 0.3 - 0.3 * (1.0 - opening), 0.0], [0.08, 0.3, 0.3], paint::INK));
        out.push(Shape::block([0.0, -0.4, 0.0], [3.4, 0.05, 1.0], paint::GROUND));
    }
    fn readouts(&self) -> Vec<Readout> {
        let p = &self.pipe;
        vec![
            Readout::new("valve pressure", self.model.runtime.get(self.model.valve_pressure) / 1.0e5, "bar"),
            Readout::new("ρ·c·Δv", p.joukowsky() / 1.0e5, "bar"),
            Readout::new("round trip 2L/c", p.round_trip_time(), "s"),
            Readout::new("valve", if self.seated() { 0.0 } else { 1.0 }, "open"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("valve pressure (bar)", self.model.runtime.get(self.model.valve_pressure) / 1.0e5) }
    fn verdict(&self) -> String {
        let rt = self.pipe.round_trip_time();
        if self.pipe.closure_time < rt { format!("closure {:.3} s < 2L/c = {:.3} s — full Joukowsky spike", self.pipe.closure_time, rt) }
        else { format!("closure {:.2} s = {:.1}× 2L/c — spike collapses", self.pipe.closure_time, self.pipe.closure_time / rt) }
    }
}

// ---------------------------------------------------------------- 6. Flutter
pub struct Flutter {
    registry: BehaviorRegistry,
    section: flutter::WingSection,
    airspeed: f64,
    flutter_speed: f64,
    model: flutter::Section,
}
impl Flutter {
    pub fn new() -> Self {
        let registry = registry();
        let section = flutter::WingSection::default();
        let uf = section.flutter_speed(&registry, 400.0).unwrap_or(15.6);
        let airspeed = (uf * 1.1 * 10.0).round() / 10.0;
        let model = section.model(&registry, airspeed, 0.02);
        Self { registry, section, airspeed, flutter_speed: uf, model }
    }
}
impl Exhibit for Flutter {
    fn title(&self) -> &'static str { "Flutter" }
    fn summary(&self) -> &'static str { "A pitch–plunge wing section damps a gust below the flutter speed and shakes itself apart above it; nothing changed but the airspeed." }
    fn knob(&self) -> Knob { knob("airspeed", "m/s", 4.0, 30.0, 0.5, self.airspeed) }
    fn set_knob(&mut self, value: f64) { self.airspeed = value; self.reset(); }
    fn reset(&mut self) { self.model = self.section.model(&self.registry, self.airspeed, 0.02); }
    fn time(&self) -> f64 { self.model.runtime.time }
    fn time_scale(&self) -> f64 { 0.4 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        advance(&mut self.model.runtime, duration, 2.0e-4)?;
        if self.model.runtime.get(self.model.plunge).abs() > 0.4 { self.reset(); }
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let h = self.model.runtime.get(self.model.plunge) * 4.0;
        let a = self.model.runtime.get(self.model.pitch) * 2.0;
        let rot = [(a / 2.0).cos(), 0.0, 0.0, -(a / 2.0).sin()];
        out.push(Shape::Block { center: [0.0, -h, 0.0], half: [1.0, 0.07, 1.6], rotation: rot, color: paint::SURPRISE });
        out.push(Shape::Rod { from: [-0.2, -h, -1.7], to: [-0.2, -h, 1.7], radius: 0.05, color: paint::INK });
        let phase = (self.model.runtime.time * self.airspeed * 0.5) % 1.5;
        for k in 0..4 {
            let z = -1.5 + k as f64;
            let x = -3.0 + phase;
            out.push(Shape::Arrow { from: [x, 0.9, z], to: [x + 0.6, 0.9, z], color: paint::CONTROL });
        }
        out.push(Shape::block([0.0, -1.6, 0.0], [3.2, 0.05, 2.2], paint::GROUND));
    }
    fn readouts(&self) -> Vec<Readout> {
        let (rate, omega) = self.section.leading_mode(&self.registry, self.airspeed);
        vec![
            Readout::new("plunge", self.model.runtime.get(self.model.plunge) * 1000.0, "mm"),
            Readout::new("pitch", self.model.runtime.get(self.model.pitch).to_degrees(), "°"),
            Readout::new("flutter speed U_F", self.flutter_speed, "m/s"),
            Readout::new("leading mode growth rate", rate, "1/s"),
            Readout::new("leading mode frequency", omega / TAU, "Hz"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("plunge (m)", self.model.runtime.get(self.model.plunge)) }
    fn verdict(&self) -> String {
        if self.airspeed < self.flutter_speed { format!("U = {:.2} U_F — gusts decay", self.airspeed / self.flutter_speed) }
        else { format!("U = {:.2} U_F — the section extracts energy from the flow", self.airspeed / self.flutter_speed) }
    }
}

// ---------------------------------------------------------------- 7. Dzhanibekov
pub struct Dzhanibekov {
    registry: BehaviorRegistry,
    middle: f64,
    runtime: Runtime,
    ids: Vec<StateId>,
}
impl Dzhanibekov {
    pub fn new() -> Self {
        let registry = registry();
        let (runtime, ids) = dzhanibekov::FreeBody { inertia: [1.0, 2.0, 3.0] }.model(&registry, [0.01, 1.0, 0.01]);
        Self { registry, middle: 2.0, runtime, ids }
    }
    fn body(&self) -> dzhanibekov::FreeBody { dzhanibekov::FreeBody { inertia: [1.0, self.middle, 3.0] } }
    fn state(&self) -> Vec<f64> { self.ids.iter().map(|id| self.runtime.get(*id)).collect() }
}
impl Exhibit for Dzhanibekov {
    fn title(&self) -> &'static str { "The Dzhanibekov flip" }
    fn summary(&self) -> &'static str { "A free body spun about the axis of middle inertia tumbles end over end again and again; make that inertia equal to a neighbour's and the flips stop." }
    fn knob(&self) -> Knob { knob("inertia about the spin axis I₂ (I₁ = 1, I₃ = 3)", "", 1.0, 3.0, 0.1, self.middle) }
    fn set_knob(&mut self, value: f64) { self.middle = value; self.reset(); }
    fn reset(&mut self) { let (rt, ids) = self.body().model(&self.registry, [0.01, 1.0, 0.01]); self.runtime = rt; self.ids = ids; }
    fn time(&self) -> f64 { self.runtime.time }
    fn time_scale(&self) -> f64 { 2.5 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.runtime, duration, 2.0e-3) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let s = self.state();
        let q = &s[3..7];
        let rot = [q[0], q[1], q[2], q[3]];
        let body_to_world = |v: [f64; 3]| {
            let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
            let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
            [v[0] + w * t[0] + (y * t[2] - z * t[1]), v[1] + w * t[1] + (z * t[0] - x * t[2]), v[2] + w * t[2] + (x * t[1] - y * t[0])]
        };
        let i2 = self.middle;
        let (a2, b2, c2) = (((i2 + 3.0 - 1.0) / 2.0).max(0.05), ((1.0 + 3.0 - i2) / 2.0).max(0.05), ((1.0 + i2 - 3.0) / 2.0).max(0.05));
        let half = [a2.sqrt() * 0.75, b2.sqrt() * 0.75, c2.sqrt() * 0.75];
        out.push(Shape::Block { center: [0.0, 0.0, 0.0], half, rotation: rot, color: paint::STEEL });
        for (axis, color, len) in [([1.0, 0.0, 0.0], paint::CONTROL, half[0] + 0.5), ([0.0, 1.0, 0.0], paint::SURPRISE, half[1] + 0.5), ([0.0, 0.0, 1.0], paint::PASS, half[2] + 0.5)] {
            let tip = body_to_world([axis[0] * len, axis[1] * len, axis[2] * len]);
            out.push(Shape::Rod { from: [0.0, 0.0, 0.0], to: tip, radius: 0.04, color });
            out.push(Shape::Sphere { center: tip, radius: 0.1, color });
        }
        let l = body_to_world([s[0] * 1.0, s[1] * self.middle, s[2] * 3.0]);
        let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt().max(1.0e-9);
        out.push(Shape::Arrow { from: [0.0, 0.0, 0.0], to: [l[0] / n * 2.4, l[1] / n * 2.4, l[2] / n * 2.4], color: paint::GLOW });
    }
    fn readouts(&self) -> Vec<Readout> {
        let s = self.state();
        let body = self.body();
        vec![
            Readout::new("ω₂ (spin axis)", s[1], "rad/s"),
            Readout::new("ω₁", s[0], "rad/s"),
            Readout::new("ω₃", s[2], "rad/s"),
            Readout::new("kinetic energy", body.kinetic_energy(&s[..3]), "J"),
            Readout::new("|L|²", body.momentum_squared(&s[..3]), ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("ω₂ about the spin axis", self.runtime.get(self.ids[1])) }
    fn verdict(&self) -> String {
        let i2 = self.middle;
        if (i2 - 1.0).abs() < 1.0e-9 || (i2 - 3.0).abs() < 1.0e-9 { "axisymmetric — no intermediate axis, no flip".into() }
        else { format!("intermediate axis — flips, linear growth rate {:.3}/s", self.body().intermediate_axis_growth_rate(1.0)) }
    }
}

// ---------------------------------------------------------------- 8. Tippe top
pub struct Tippe {
    registry: BehaviorRegistry,
    top: tippe_top::TippeTop,
    spin: f64,
    critical: f64,
    model: tippe_top::Top,
}
impl Tippe {
    pub fn new() -> Self {
        let registry = registry();
        let top = tippe_top::TippeTop::default();
        let critical = top.critical_spin(&registry).unwrap_or(32.0);
        let spin = 6.0 * critical;
        let model = top.model(&registry, spin, 0.05);
        Self { registry, top, spin, critical, model }
    }
    fn state(&self) -> Vec<f64> { self.model.ids.iter().map(|id| self.model.runtime.get(*id)).collect() }
}
impl Exhibit for Tippe {
    fn title(&self) -> &'static str { "The tippe top" }
    fn summary(&self) -> &'static str { "Spun fast enough, the top turns itself over and lifts its centre of mass; spun slowly it just wobbles and slows." }
    fn knob(&self) -> Knob { knob("initial spin", "rad/s", 5.0, 300.0, 5.0, self.spin) }
    fn set_knob(&mut self, value: f64) { self.spin = value; self.reset(); }
    fn reset(&mut self) { self.model = self.top.model(&self.registry, self.spin, 0.05); }
    fn time(&self) -> f64 { self.model.runtime.time }
    fn time_scale(&self) -> f64 { 0.5 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.model.runtime, duration, 2.0e-4) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let s = self.state();
        let k = 60.0;
        let com = [s[0] * k, s[2] * k, s[1] * k];
        let a = tippe_top::axis(&s[3..7]);
        let axis = [a[0], a[2], a[1]];
        let centre = [com[0] + self.top.offset * k * axis[0], com[1] + self.top.offset * k * axis[1], com[2] + self.top.offset * k * axis[2]];
        out.push(Shape::Sphere { center: centre, radius: self.top.radius * k, color: paint::STEEL });
        let stem = 0.024 * 1.4 * k;
        let tip = [centre[0] + stem * axis[0], centre[1] + stem * axis[1], centre[2] + stem * axis[2]];
        out.push(Shape::Rod { from: centre, to: tip, radius: 0.09, color: paint::INK });
        out.push(Shape::Sphere { center: com, radius: 0.08, color: paint::SURPRISE });
        out.push(Shape::block([0.0, -0.03, 0.0], [3.0, 0.03, 3.0], paint::GROUND));
    }
    fn readouts(&self) -> Vec<Readout> {
        let s = self.state();
        vec![
            Readout::new("stem axis ê_z", tippe_top::axis(&s[3..7])[2], ""),
            Readout::new("centre of mass height", s[2] * 1000.0, "mm"),
            Readout::new("critical spin ω_c", self.critical, "rad/s"),
            Readout::new("energy", self.model.runtime.energy() * 1000.0, "mJ"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("stem axis ê_z", tippe_top::axis(&self.state()[3..7])[2]) }
    fn verdict(&self) -> String {
        if self.spin > self.critical { format!("{:.1} ω_c — upright is unstable, it inverts", self.spin / self.critical) }
        else { format!("{:.1} ω_c — upright is stable", self.spin / self.critical) }
    }
}

// ---------------------------------------------------------------- 9. Passive walker
pub struct Walker {
    registry: BehaviorRegistry,
    slope: f64,
    model: passive_walker::WalkerModel,
    strides: usize,
    distance: f64,
    fallen: bool,
}
impl Walker {
    const START: [f64; 3] = [0.2003, -0.1998, -0.0158];
    pub fn new() -> Self {
        let registry = registry();
        let model = passive_walker::model(&registry, 0.009, false, Self::START);
        Self { registry, slope: 0.009, model, strides: 0, distance: 0.0, fallen: false }
    }
    fn theta(&self) -> f64 { self.model.runtime.get(self.model.ids[0]) }
    fn phi(&self) -> f64 { self.model.runtime.get(self.model.ids[1]) }
}
impl Exhibit for Walker {
    fn title(&self) -> &'static str { "Passive dynamic walker" }
    fn summary(&self) -> &'static str { "No motor, no controller: on a shallow slope the linkage walks; steepen it and the stride period-doubles into chaos." }
    fn knob(&self) -> Knob { knob("slope γ", "rad", 0.004, 0.024, 0.0005, self.slope) }
    fn set_knob(&mut self, value: f64) { self.slope = value; self.reset(); }
    fn reset(&mut self) { self.model = passive_walker::model(&self.registry, self.slope, false, Self::START); self.strides = 0; self.distance = 0.0; self.fallen = false; }
    fn time(&self) -> f64 { self.model.runtime.time }
    fn time_unit(&self) -> &'static str { "√(l/g)" }
    fn time_scale(&self) -> f64 { 2.5 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        if self.fallen { return Ok(()); }
        advance(&mut self.model.runtime, duration, 2.0e-3)?;
        let events = self.model.runtime.events();
        while self.strides < events {
            self.strides += 1;
            self.distance += 2.0 * self.theta().sin();
        }
        if self.theta().abs() > 1.0 { self.fallen = true; }
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let g = self.slope;
        let (theta, phi) = (self.theta(), self.phi());
        let l = 1.5;
        let tilt = g * 20.0;
        let to_world = |x: f64, y: f64| [x * tilt.cos() + y * tilt.sin(), -x * tilt.sin() + y * tilt.cos(), 0.0];
        let slab = 4.0;
        let travel = (self.distance * l).rem_euclid(2.0 * slab) - slab;
        let foot = to_world(travel, 0.0);
        let hip = to_world(travel - l * theta.sin(), l * theta.cos());
        let swing = to_world(travel - l * theta.sin() + l * (theta - phi).sin(), l * theta.cos() - l * (theta - phi).cos());
        out.push(Shape::Rod { from: foot, to: hip, radius: 0.05, color: paint::INK });
        out.push(Shape::Rod { from: hip, to: swing, radius: 0.05, color: paint::SURPRISE });
        out.push(Shape::Sphere { center: hip, radius: 0.18, color: paint::INK });
        out.push(Shape::Sphere { center: foot, radius: 0.07, color: paint::INK });
        out.push(Shape::Sphere { center: swing, radius: 0.07, color: paint::SURPRISE });
        let ground_rot = [(tilt / 2.0).cos(), 0.0, 0.0, -(tilt / 2.0).sin()];
        out.push(Shape::Block { center: to_world(0.0, -0.05), half: [slab + 0.3, 0.05, 1.2], rotation: ground_rot, color: paint::GROUND });
        for k in -8..=8 {
            let a = to_world(k as f64 * 0.5 * l, 0.005);
            out.push(Shape::Line { from: [a[0], a[1], -1.1], to: [a[0], a[1], 1.1], color: paint::STEEL });
        }
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![
            Readout::new("stance angle θ", self.theta(), "rad"),
            Readout::new("inter-leg angle φ", self.phi(), "rad"),
            Readout::new("strides", self.strides as f64, ""),
            Readout::new("distance walked", self.distance, "leg lengths"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("stance angle θ", self.theta()) }
    fn verdict(&self) -> String {
        if self.fallen { "fell".into() }
        else if self.slope < 0.0151 { "period-1 gait (doubling at γ ≈ 0.0151)".into() }
        else if self.slope < 0.0177 { "period-2 gait".into() }
        else if self.slope < 0.0185 { "period-4 gait".into() }
        else { "chaotic gait".into() }
    }
}

// ---------------------------------------------------------------- 10. Euler buckling
pub struct Buckling {
    registry: BehaviorRegistry,
    load_fraction: f64,
    column: euler_buckling::Column,
    runtime: Runtime,
    ids: Vec<(StateId, StateId)>,
}
impl Buckling {
    fn column(load_fraction: f64) -> euler_buckling::Column {
        let mut column = euler_buckling::Column { segments: 12, length: 1.0, bending_stiffness: 1.0, axial_stiffness: 2.0e4, mass_per_length: 1.0, damping: 0.05, load: 0.0 };
        column.load = load_fraction * column.discrete_critical_load();
        column
    }
    pub fn new() -> Self {
        let registry = registry();
        let column = Self::column(1.5);
        let (runtime, ids) = column.model(&registry, 0.02);
        Self { registry, load_fraction: 1.5, column, runtime, ids }
    }
    fn positions(&self) -> Vec<[f64; 2]> { self.ids.iter().map(|(x, y)| [self.runtime.get(*x), self.runtime.get(*y)]).collect() }
    fn midpoint(&self) -> f64 { self.runtime.get(self.ids[self.column.segments / 2].1) }
}
impl Exhibit for Buckling {
    fn title(&self) -> &'static str { "Euler buckling" }
    fn summary(&self) -> &'static str { "Below the critical load the column shortens and stays straight; above it, it bows sideways, and its lateral frequency goes to zero at the boundary." }
    fn knob(&self) -> Knob { knob("axial load", "P_cr", 0.0, 2.0, 0.05, self.load_fraction) }
    fn set_knob(&mut self, value: f64) { self.load_fraction = value; self.column = Self::column(value); self.reset(); }
    fn reset(&mut self) { let (rt, ids) = self.column.model(&self.registry, 0.02); self.runtime = rt; self.ids = ids; }
    fn time(&self) -> f64 { self.runtime.time }
    fn time_scale(&self) -> f64 { 1.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.runtime, duration, 2.0e-3) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let p = self.positions();
        let k = 4.0;
        let w = |q: [f64; 2]| [(q[0] - 0.5) * k, q[1] * k, 0.0];
        for pair in p.windows(2) {
            out.push(Shape::Rod { from: w(pair[0]), to: w(pair[1]), radius: 0.06, color: paint::SURPRISE });
        }
        for q in &p { out.push(Shape::Sphere { center: w(*q), radius: 0.07, color: paint::INK }); }
        out.push(Shape::block(w([0.0, 0.0]), [0.12, 0.25, 0.25], paint::GROUND));
        let end = w(p[p.len() - 1]);
        out.push(Shape::block([end[0], -0.2, 0.0], [0.15, 0.06, 0.25], paint::GROUND));
        let arrow = 0.2 + 0.5 * self.load_fraction;
        out.push(Shape::Arrow { from: [end[0] + arrow + 0.15, 0.0, 0.0], to: [end[0] + 0.15, 0.0, 0.0], color: paint::CONTROL });
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![
            Readout::new("mid-span deflection", self.midpoint() / self.column.length, "L"),
            Readout::new("P / P_cr (discrete chain)", self.load_fraction, ""),
            Readout::new("π²EI/L²", self.column.euler_load(), ""),
            Readout::new("discrete chain P_cr", self.column.discrete_critical_load(), ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("mid-span deflection (L)", self.midpoint()) }
    fn verdict(&self) -> String {
        if self.load_fraction < 1.0 { format!("P = {:.2} P_cr — straight; lateral ω² ∝ 1 − P/P_cr", self.load_fraction) } else { format!("P = {:.2} P_cr — bows", self.load_fraction) }
    }
}

// ---------------------------------------------------------------- 11. Spring pendulum
pub struct Spring {
    registry: BehaviorRegistry,
    ratio: f64,
    pendulum: spring_pendulum::SpringPendulum,
    runtime: Runtime,
    x: StateId,
    y: StateId,
}
impl Spring {
    pub fn new() -> Self {
        let registry = registry();
        let pendulum = spring_pendulum::SpringPendulum::tuned(1.0, 0.5, 9.81, 2.0);
        let (runtime, x, y) = pendulum.model(&registry, 0.1 * pendulum.hanging_length());
        Self { registry, ratio: 2.0, pendulum, runtime, x, y }
    }
}
impl Exhibit for Spring {
    fn title(&self) -> &'static str { "The 2:1 spring pendulum" }
    fn summary(&self) -> &'static str { "Tuned so the bounce is twice the swing frequency, a pure bounce turns itself into a swing and back; detune it and the bounce stays a bounce." }
    fn knob(&self) -> Knob { knob("bounce / swing frequency ratio", "", 1.4, 2.6, 0.05, self.ratio) }
    fn set_knob(&mut self, value: f64) { self.ratio = value; self.pendulum = spring_pendulum::SpringPendulum::tuned(1.0, 0.5, 9.81, value); self.reset(); }
    fn reset(&mut self) { let (rt, x, y) = self.pendulum.model(&self.registry, 0.1 * self.pendulum.hanging_length()); self.runtime = rt; self.x = x; self.y = y; }
    fn time(&self) -> f64 { self.runtime.time }
    fn time_scale(&self) -> f64 { 2.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.runtime, duration, 1.0e-3) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let k = 2.5;
        let pivot = [0.0, 1.9, 0.0];
        let mass = [self.runtime.get(self.x) * k, 1.9 - self.runtime.get(self.y) * k, 0.0];
        out.push(Shape::block([0.0, 2.0, 0.0], [0.6, 0.06, 0.4], paint::GROUND));
        out.push(Shape::Polyline { points: spring_polyline(pivot, mass, 10, 0.12), color: paint::INK });
        out.push(Shape::Sphere { center: mass, radius: 0.16, color: paint::SURPRISE });
        out.push(Shape::Line { from: pivot, to: [0.0, -1.2, 0.0], color: paint::STEEL });
    }
    fn readouts(&self) -> Vec<Readout> {
        let p = &self.pendulum;
        vec![
            Readout::new("lateral x", self.runtime.get(self.x) * 100.0, "cm"),
            Readout::new("stretch", (self.runtime.get(self.y) - p.hanging_length()) * 100.0, "cm"),
            Readout::new("bounce frequency", p.bounce_frequency() / TAU, "Hz"),
            Readout::new("swing frequency", p.swing_frequency() / TAU, "Hz"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("lateral x (m)", self.runtime.get(self.x)) }
    fn verdict(&self) -> String {
        let d = (self.ratio - 2.0).abs();
        if d < 1.0e-6 { "exact 2:1 — full exchange between bounce and swing".into() }
        else if d < 0.15 { format!("{:.0}% detuned — partial exchange", d * 50.0) }
        else { format!("{:.0}% detuned — the bounce keeps its energy", d * 50.0) }
    }
}

// ---------------------------------------------------------------- 12. Stick–slip
pub struct Belt {
    registry: BehaviorRegistry,
    block: stick_slip::BeltBlock,
    runtime: Runtime,
    position: StateId,
    velocity: StateId,
}
impl Belt {
    pub fn new() -> Self {
        let registry = registry();
        let block = stick_slip::BeltBlock::default();
        let (runtime, position, velocity) = Self::build(&registry, block);
        Self { registry, block, runtime, position, velocity }
    }
    fn build(registry: &BehaviorRegistry, block: stick_slip::BeltBlock) -> (Runtime, StateId, StateId) {
        let (model, mass) = block.model(registry);
        let rt = runtime(model, registry);
        let position = rt.across_id(mass.port("axis"));
        let velocity = rt.state_id(mass.behavior, "velocity");
        (rt, position, velocity)
    }
    fn stuck(&self) -> bool { (self.runtime.get(self.velocity) - self.block.belt_speed).abs() < 0.02 * self.block.belt_speed }
}
impl Exhibit for Belt {
    fn title(&self) -> &'static str { "Stick–slip self-excitation" }
    fn summary(&self) -> &'static str { "A spring-held block on a belt jerks in a stick–slip cycle below a critical belt speed and sits still above it — the friction curve's slope is the whole story." }
    fn knob(&self) -> Knob { knob("belt speed", "m/s", 0.05, 1.0, 0.025, self.block.belt_speed) }
    fn set_knob(&mut self, value: f64) { self.block.belt_speed = value; self.reset(); }
    fn reset(&mut self) { let (rt, p, v) = Self::build(&self.registry, self.block); self.runtime = rt; self.position = p; self.velocity = v; }
    fn time(&self) -> f64 { self.runtime.time }
    fn time_scale(&self) -> f64 { 1.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.runtime, duration, 2.0e-4) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let k = 6.0;
        let x = (self.runtime.get(self.position) - self.block.friction().force(self.block.belt_speed) / self.block.stiffness) * k;
        out.push(Shape::block([0.0, -0.4, 0.0], [3.0, 0.08, 0.8], paint::GROUND));
        let phase = (self.runtime.time * self.block.belt_speed * k) % 0.6;
        for i in -5..5 {
            let bx = i as f64 * 0.6 + phase;
            out.push(Shape::Line { from: [bx, -0.31, -0.7], to: [bx, -0.31, 0.7], color: paint::STEEL });
        }
        out.push(Shape::block([x, 0.0, 0.0], [0.45, 0.32, 0.45], if self.stuck() { paint::PASS } else { paint::SURPRISE }));
        out.push(Shape::block([-2.9, 0.2, 0.0], [0.08, 0.6, 0.6], paint::INK));
        out.push(Shape::Polyline { points: spring_polyline([-2.82, 0.1, 0.0], [x - 0.45, 0.1, 0.0], 9, 0.12), color: paint::INK });
    }
    fn readouts(&self) -> Vec<Readout> {
        let b = &self.block;
        vec![
            Readout::new("block velocity", self.runtime.get(self.velocity), "m/s"),
            Readout::new("critical belt speed", b.critical_speed(), "m/s"),
            Readout::new("−dF/dv at belt speed", -b.friction().slope(b.belt_speed), "N·s/m"),
            Readout::new("viscous damping c", b.damping, "N·s/m"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("block velocity (m/s)", self.runtime.get(self.velocity)) }
    fn verdict(&self) -> String {
        let vc = self.block.critical_speed();
        if self.block.belt_speed < vc { format!("v = {:.2} v_c — −dF/dv > c, limit cycle", self.block.belt_speed / vc) } else { format!("v = {:.2} v_c — damping wins, settles", self.block.belt_speed / vc) }
    }
}

// ---------------------------------------------------------------- 13. Backlash
pub struct Backlash {
    registry: BehaviorRegistry,
    servo: backlash_hunting::Servo,
    model: backlash_hunting::Model,
    prediction: Option<(f64, f64)>,
}
impl Backlash {
    pub fn new() -> Self {
        let registry = registry();
        let servo = backlash_hunting::Servo { gap: 0.01, ..Default::default() };
        let model = servo.model(&registry, servo.mesh_stiffness);
        let prediction = backlash_hunting::describing_function_prediction(servo, &registry);
        Self { registry, servo, model, prediction }
    }
    fn angles(&self) -> (f64, f64) {
        (self.model.runtime.get(self.model.runtime.across_id(self.model.motor_shaft)), self.model.runtime.get(self.model.runtime.across_id(self.model.load_shaft)))
    }
}
fn gear(out: &mut Vec<Shape>, center: [f64; 3], radius: f64, angle: f64, teeth: usize, color: [f32; 3]) {
    out.push(Shape::Rod { from: [center[0], center[1], center[2] - 0.08], to: [center[0], center[1], center[2] + 0.08], radius, color });
    for i in 0..teeth {
        let a = angle + i as f64 * TAU / teeth as f64;
        let c = [center[0] + (radius + 0.06) * a.cos(), center[1] + (radius + 0.06) * a.sin(), center[2]];
        out.push(Shape::Block { center: c, half: [0.08, 0.05, 0.09], rotation: [(a / 2.0).cos(), 0.0, 0.0, (a / 2.0).sin()], color });
    }
    out.push(Shape::Rod { from: center, to: [center[0] + radius * 0.9 * angle.cos(), center[1] + radius * 0.9 * angle.sin(), center[2] + 0.1], radius: 0.03, color: paint::INK });
}
impl Exhibit for Backlash {
    fn title(&self) -> &'static str { "Backlash hunting" }
    fn summary(&self) -> &'static str { "A PI servo through a gear mesh with play never settles: it hunts at a fixed pitch with an amplitude set by the gap. Close the gap and it stops." }
    fn knob(&self) -> Knob { knob("mesh gap (half-width)", "rad", 0.0, 0.03, 0.0025, self.servo.gap) }
    fn set_knob(&mut self, value: f64) { self.servo.gap = value; self.reset(); }
    fn reset(&mut self) { self.model = self.servo.model(&self.registry, self.servo.mesh_stiffness); }
    fn time(&self) -> f64 { self.model.runtime.time }
    fn time_scale(&self) -> f64 { 1.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.model.runtime, duration, 5.0e-4) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let (m, l) = self.angles();
        let e = 25.0;
        gear(out, [-1.1, 0.0, 0.0], 0.8, m * e, 12, paint::CONTROL);
        gear(out, [1.1, 0.0, 0.0], 0.8, PI / 12.0 - l * e, 12, paint::SURPRISE);
        out.push(Shape::block([0.0, -1.3, 0.0], [2.6, 0.05, 1.0], paint::GROUND));
    }
    fn readouts(&self) -> Vec<Readout> {
        let (m, l) = self.angles();
        let mut r = vec![
            Readout::new("load angle", l * 1000.0, "mrad"),
            Readout::new("twist θm − θl", (m - l) * 1000.0, "mrad"),
            Readout::new("gap", self.servo.gap * 1000.0, "mrad"),
        ];
        if let Some((omega, ratio)) = self.prediction {
            r.push(Readout::new("describing-function ω", omega, "rad/s"));
            r.push(Readout::new("predicted twist amplitude", ratio * self.servo.gap * 1000.0, "mrad"));
        }
        r
    }
    fn signal(&self) -> (&'static str, f64) { let (m, l) = self.angles(); ("twist θm − θl (rad)", m - l) }
    fn verdict(&self) -> String {
        if self.servo.gap <= 0.0 { "no gap — settles".into() } else { "gap — hunting limit cycle, amplitude ∝ gap, frequency fixed (drawn ×25)".into() }
    }
}

// ---------------------------------------------------------------- 14. Sample-rate
pub struct Sampled {
    registry: BehaviorRegistry,
    motor: sample_rate_instability::MotorLoop,
    period_fraction: f64,
    critical: f64,
    plant: sample_rate_instability::Plant,
    samples: Vec<f64>,
    seen_events: usize,
    angle: f64,
}
impl Sampled {
    pub fn new() -> Self {
        let registry = registry();
        let motor = sample_rate_instability::MotorLoop::default();
        let critical = sample_rate_instability::critical_period(motor.loop_gain, motor.time_constant());
        let plant = motor.model(&registry, 1.1 * critical, motor.loop_gain / motor.gain());
        Self { registry, motor, period_fraction: 1.1, critical, plant, samples: Vec::new(), seen_events: 0, angle: 0.0 }
    }
    fn period(&self) -> f64 { self.period_fraction * self.critical }
}
impl Exhibit for Sampled {
    fn title(&self) -> &'static str { "Sample-rate instability" }
    fn summary(&self) -> &'static str { "A held proportional command on a first-order motor: fine at fast sampling, unstable at exactly the period where coth(T/2τ) = KpK." }
    fn knob(&self) -> Knob { knob("sample period", "T_c", 0.2, 1.6, 0.05, self.period_fraction) }
    fn set_knob(&mut self, value: f64) { self.period_fraction = value; self.reset(); }
    fn reset(&mut self) {
        self.plant = self.motor.model(&self.registry, self.period(), self.motor.loop_gain / self.motor.gain());
        self.samples.clear();
        self.seen_events = 0;
        self.angle = 0.0;
    }
    fn time(&self) -> f64 { self.plant.runtime.time }
    fn time_scale(&self) -> f64 { 0.5 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        advance(&mut self.plant.runtime, duration, 1.0e-4)?;
        let speed = self.plant.runtime.get(self.plant.speed);
        self.angle += speed * duration;
        let events = self.plant.runtime.events();
        while self.seen_events < events {
            self.seen_events += 1;
            self.samples.push(speed);
            if self.samples.len() > 40 { self.samples.remove(0); }
        }
        if speed.abs() > 400.0 { self.reset(); }
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let a = self.angle;
        out.push(Shape::Rod { from: [-2.0, 0.6, -0.5], to: [-2.0, 0.6, 0.5], radius: 0.35, color: paint::STEEL });
        out.push(Shape::Rod { from: [-2.0, 0.6, 0.5], to: [-2.0 + 0.5 * a.cos(), 0.6 + 0.5 * a.sin(), 0.5], radius: 0.05, color: paint::SURPRISE });
        let n = self.samples.len().max(1);
        let scale = 0.05_f64.max(self.samples.iter().fold(0.0_f64, |m, v| m.max(v.abs()))).recip();
        for (i, v) in self.samples.iter().enumerate() {
            let x = -1.0 + 3.6 * i as f64 / n as f64;
            let h = v * scale * 1.1;
            out.push(Shape::block([x, h / 2.0, 0.0], [0.035, h.abs() / 2.0 + 0.005, 0.05], if v.abs() > 10.0 { paint::SURPRISE } else { paint::CONTROL }));
        }
        out.push(Shape::Line { from: [-1.0, 0.0, 0.0], to: [2.7, 0.0, 0.0], color: paint::STEEL });
        out.push(Shape::block([0.3, -1.4, 0.0], [3.0, 0.05, 1.0], paint::GROUND));
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![
            Readout::new("speed error", self.plant.runtime.get(self.plant.speed), "rad/s"),
            Readout::new("held voltage", self.plant.runtime.get(self.plant.held), "V"),
            Readout::new("sample period", self.period() * 1000.0, "ms"),
            Readout::new("critical period T_c", self.critical * 1000.0, "ms"),
            Readout::new("closed-loop pole z", sample_rate_instability::discrete_pole(self.motor.loop_gain, self.period(), self.motor.time_constant()), ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("speed error (rad/s)", self.plant.runtime.get(self.plant.speed)) }
    fn verdict(&self) -> String {
        let z = sample_rate_instability::discrete_pole(self.motor.loop_gain, self.period(), self.motor.time_constant());
        if z.abs() < 1.0 { format!("|z| = {:.3} < 1 — converges", z.abs()) } else { format!("|z| = {:.3} > 1 — diverges", z.abs()) }
    }
}

// ---------------------------------------------------------------- 15. Chua
pub struct ChuaScroll {
    registry: BehaviorRegistry,
    alpha: f64,
    circuit: chua_circuit::Circuit,
    trail: Vec<[f64; 3]>,
}
impl ChuaScroll {
    pub fn new() -> Self {
        let registry = registry();
        let circuit = Self::build(&registry, 9.0);
        Self { registry, alpha: 9.0, circuit, trail: Vec::new() }
    }
    fn build(registry: &BehaviorRegistry, alpha: f64) -> chua_circuit::Circuit {
        chua_circuit::Chua { alpha, beta: 100.0 / 7.0, ..Default::default() }.model(registry, [1.6, 0.0, -1.5])
    }
    fn state(&self) -> [f64; 3] {
        let c = &self.circuit;
        [c.runtime.get(c.v1), c.runtime.get(c.v2), -c.runtime.get(c.i_l)]
    }
}
impl Exhibit for ChuaScroll {
    fn title(&self) -> &'static str { "Chua's circuit" }
    fn summary(&self) -> &'static str { "Five components and a piecewise-linear negative resistance. Raise α and the orbit doubles, doubles again, then wanders the double scroll forever." }
    fn knob(&self) -> Knob { knob("α (= C₂/C₁), β = 100/7", "", 7.6, 9.6, 0.02, self.alpha) }
    fn set_knob(&mut self, value: f64) { self.alpha = value; self.reset(); }
    fn reset(&mut self) { self.circuit = Self::build(&self.registry, self.alpha); self.trail.clear(); }
    fn time(&self) -> f64 { self.circuit.runtime.time }
    fn time_unit(&self) -> &'static str { "RC" }
    fn time_scale(&self) -> f64 { 8.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        let steps = (duration / 0.02).ceil().max(1.0) as usize;
        for _ in 0..steps {
            advance(&mut self.circuit.runtime, duration / steps as f64, 4.0e-3)?;
            let s = self.state();
            self.trail.push([s[0] * 0.9, s[2] * 0.55, s[1] * 4.0]);
        }
        if self.trail.len() > 2500 { let n = self.trail.len() - 2500; self.trail.drain(..n); }
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        if self.trail.len() > 1 { out.push(Shape::Polyline { points: self.trail.clone(), color: paint::SURPRISE }); }
        if let Some(p) = self.trail.last() { out.push(Shape::Sphere { center: *p, radius: 0.08, color: paint::GLOW }); }
        out.push(Shape::Line { from: [-2.5, 0.0, 0.0], to: [2.5, 0.0, 0.0], color: paint::STEEL });
        out.push(Shape::Line { from: [0.0, -2.0, 0.0], to: [0.0, 2.0, 0.0], color: paint::STEEL });
        out.push(Shape::Line { from: [0.0, 0.0, -2.0], to: [0.0, 0.0, 2.0], color: paint::STEEL });
    }
    fn readouts(&self) -> Vec<Readout> {
        let s = self.state();
        vec![Readout::new("v₁ (capacitor 1)", s[0], ""), Readout::new("v₂ (capacitor 2)", s[1], ""), Readout::new("−i_L (inductor)", s[2], "")]
    }
    fn signal(&self) -> (&'static str, f64) { ("v₁(t)", self.state()[0]) }
    fn verdict(&self) -> String {
        let a = self.alpha;
        if a < 8.0 { "stable equilibrium".into() } else if a < 8.18 { "period-1 limit cycle".into() } else if a < 8.40 { "period-2".into() } else if a < 8.45 { "period-4 → cascade".into() } else if a < 8.85 { "spiral chaos (one scroll)".into() } else { "double scroll".into() }
    }
}

// ---------------------------------------------------------------- 16. Thermoelastic damping
pub struct Zener {
    registry: BehaviorRegistry,
    thickness_fraction: f64,
    peak_thickness: f64,
    beam: thermoelastic_damping::ThermoelasticBeam,
    model: thermoelastic_damping::Beam,
}
impl Zener {
    fn beam(thickness: f64) -> thermoelastic_damping::ThermoelasticBeam {
        thermoelastic_damping::ThermoelasticBeam { material: thermoelastic_damping::Material::ALUMINIUM, thickness, width: 1.0e-3, layers: 12, frequency: TAU * 10.0e3 }
    }
    pub fn new() -> Self {
        let registry = registry();
        let m = thermoelastic_damping::Material::ALUMINIUM;
        let f = TAU * 10.0e3;
        let peak = (PI * PI * m.diffusivity() / f).sqrt();
        let beam = Self::beam(peak);
        let model = beam.model(&registry);
        Self { registry, thickness_fraction: 1.0, peak_thickness: peak, beam, model }
    }
}
impl Exhibit for Zener {
    fn title(&self) -> &'static str { "Thermoelastic damping" }
    fn summary(&self) -> &'static str { "A vibrating beam with no damper stops: bending warms one face and cools the other, heat crosses, and that flow is irreversible — sharpest when the crossing time matches the period." }
    fn knob(&self) -> Knob { knob("thickness", "h(ωτ=1)", 0.3, 3.0, 0.1, self.thickness_fraction) }
    fn set_knob(&mut self, value: f64) { self.thickness_fraction = value; self.beam = Self::beam(value * self.peak_thickness); self.reset(); }
    fn reset(&mut self) { self.model = self.beam.model(&self.registry); }
    fn time(&self) -> f64 { self.model.runtime.time * 1000.0 }
    fn time_unit(&self) -> &'static str { "ms" }
    fn time_scale(&self) -> f64 { 3.0e-4 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { let h = TAU / self.beam.frequency / 60.0; advance(&mut self.model.runtime, duration, h) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let kappa = self.model.runtime.get(self.model.curvature) / 1.0e-3;
        let theta: Vec<f64> = self.model.layer_temperatures.iter().map(|id| self.model.runtime.get(*id)).collect();
        let tmax = theta.iter().fold(1.0e-9_f64, |m, v| m.max(v.abs())).max(0.02);
        let segments = 14;
        let half_len = 2.4;
        let th = 0.9;
        let layers = theta.len();
        for (l, t) in theta.iter().enumerate() {
            let y_layer = ((l as f64 + 0.5) / layers as f64 - 0.5) * th;
            let color = paint::heat((t / tmax * 0.9) as f32);
            for seg in 0..segments {
                let (s0, s1) = (seg as f64 / segments as f64, (seg + 1) as f64 / segments as f64);
                let x0 = -half_len + 2.0 * half_len * s0;
                let x1 = -half_len + 2.0 * half_len * s1;
                let bend = |sx: f64| -kappa * 0.6 * (PI * sx).sin();
                out.push(Shape::Rod { from: [x0, bend(s0) + y_layer, 0.0], to: [x1, bend(s1) + y_layer, 0.0], radius: th / layers as f64 * 0.5, color });
            }
        }
        out.push(Shape::block([-half_len - 0.15, 0.0, 0.0], [0.12, 0.75, 0.5], paint::GROUND));
        out.push(Shape::block([half_len + 0.15, 0.0, 0.0], [0.12, 0.75, 0.5], paint::GROUND));
    }
    fn readouts(&self) -> Vec<Readout> {
        let beam = &self.beam;
        vec![
            Readout::new("curvature amplitude", self.model.runtime.get(self.model.curvature) * 1000.0, "×10⁻³ 1/m"),
            Readout::new("thickness", beam.thickness * 1.0e6, "µm"),
            Readout::new("ω·τ (Zener)", beam.frequency * beam.zener_time(), ""),
            Readout::new("Lifshitz–Roukes Q⁻¹", beam.lifshitz_roukes_loss(), ""),
            Readout::new("Q⁻¹ at the peak (0.494 Δ_E)", 0.494 * beam.material.relaxation_strength(), ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("curvature", self.model.runtime.get(self.model.curvature)) }
    fn verdict(&self) -> String {
        let wt = self.beam.frequency * self.beam.zener_time();
        if (wt - 1.0).abs() < 0.2 { "ωτ ≈ 1 — peak damping".into() } else if wt < 1.0 { format!("ωτ = {wt:.2} — thin: heat equalises within a cycle, little loss") } else { format!("ωτ = {wt:.2} — thick: nearly adiabatic, little loss") }
    }
}

// ---------------------------------------------------------------- 17. Painlevé's paradox
pub struct Painleve {
    registry: BehaviorRegistry,
    rod: painleve_rod::Rod,
    critical: f64,
    model: painleve_rod::Slide,
}
impl Painleve {
    pub fn new() -> Self {
        let registry = registry();
        let base = painleve_rod::Rod::default();
        let critical = base.critical_friction();
        let rod = painleve_rod::Rod { friction: 0.7 * critical, ..base };
        let model = rod.model(&registry);
        Self { registry, rod, critical, model }
    }
    fn state(&self) -> Vec<f64> { self.model.body.iter().map(|id| self.model.runtime.get(*id)).collect() }
    fn normal(&self) -> f64 { self.model.normal.map(|id| self.model.runtime.get(id)).unwrap_or(f64::NAN) }
}
impl Exhibit for Painleve {
    fn title(&self) -> &'static str { "Painlevé's paradox" }
    fn summary(&self) -> &'static str { "A stick pushed tip-first across a rough floor slides — until the friction coefficient crosses Painlevé's bound, when the rigid-body equations have no sliding solution and the tip jams with an impulsive normal force." }
    fn knob(&self) -> Knob { knob("friction coefficient μ (μ_c ≈ 1.35 at 60°)", "", 0.2, 2.4, 0.05, self.rod.friction) }
    fn set_knob(&mut self, value: f64) { self.rod.friction = value; self.reset(); }
    fn reset(&mut self) { self.model = self.rod.model(&self.registry); }
    fn time(&self) -> f64 { self.model.runtime.time }
    fn time_scale(&self) -> f64 { 0.1 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        advance(&mut self.model.runtime, duration, 1.0e-4)?;
        let s = self.state();
        if s[0].abs() > 3.0 || s[1] > 3.0 || self.model.runtime.time > 0.6 { self.reset(); }
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let s = self.state();
        let k = 3.0;
        let l = self.rod.half_length;
        let (c, sn) = (s[2].cos(), s[2].sin());
        let com = [s[0] * k, s[1] * k, 0.0];
        let tip = [(s[0] + l * c) * k, (s[1] + l * sn) * k, 0.0];
        let tail = [(s[0] - l * c) * k, (s[1] - l * sn) * k, 0.0];
        out.push(Shape::block([0.0, -0.05, 0.0], [4.0, 0.05, 1.0], paint::GROUND));
        out.push(Shape::Rod { from: tail, to: tip, radius: 0.06, color: paint::SURPRISE });
        out.push(Shape::Sphere { center: com, radius: 0.09, color: paint::INK });
        let n = self.normal();
        if n.is_finite() && n > 0.0 {
            let len = (n / (self.rod.mass * self.rod.gravity)).min(30.0) * 0.15;
            out.push(Shape::Arrow { from: [tip[0], tip[1] - len, 0.0], to: tip, color: paint::CONTROL });
        }
    }
    fn readouts(&self) -> Vec<Readout> {
        let s = self.state();
        let l = self.rod.half_length;
        let tip_speed = s[3] - s[5] * l * s[2].sin();
        vec![
            Readout::new("tip speed", tip_speed, "m/s"),
            Readout::new("normal force / weight", self.normal() / (self.rod.mass * self.rod.gravity), ""),
            Readout::new("rod angle", -s[2].to_degrees(), "°"),
            Readout::new("Painlevé μ_c at this angle", (1.0 + 3.0 * s[2].cos().powi(2)) / (3.0 * s[2].sin().abs() * s[2].cos()), ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { let s = self.state(); ("tip speed (m/s)", s[3] - s[5] * self.rod.half_length * s[2].sin()) }
    fn verdict(&self) -> String {
        let ratio = self.rod.friction / self.critical;
        if ratio < 1.0 {
            format!("μ = {ratio:.2} μ_c — slides with n = {:.1} × weight", self.rod.sliding_normal_force() / (self.rod.mass * self.rod.gravity))
        } else {
            format!("μ = {ratio:.2} μ_c — no sliding solution: impact without collision, the tip jams")
        }
    }
}

// ---------------------------------------------------------------- 18. Motor hogging
pub struct MotorHogging {
    registry: BehaviorRegistry,
    drive: motor_hogging::Drive,
    rig: motor_hogging::Rig,
}
impl MotorHogging {
    pub fn new() -> Self {
        let registry = registry();
        let drive = motor_hogging::Drive { pair: motor_hogging::with_gain(-1.0, 1.5), torque_constant: 0.05, load: motor_hogging::Load::Spinning { inertia: 1.0e-3, damping: 0.1 } };
        let rig = drive.model(&registry, 0.01);
        Self { registry, drive, rig }
    }
    fn get(&self, id: StateId) -> f64 { self.rig.runtime.get(id) }
    fn share(&self) -> f64 { motor_hogging::share(&self.drive, self.get(self.rig.voltage), self.get(self.rig.speeds[0]), self.get(self.rig.temperatures[0])) }
}
impl Exhibit for MotorHogging {
    fn title(&self) -> &'static str { "Motor hogging" }
    fn summary(&self) -> &'static str { "Two motors on one drive, each behind a single plug that bundles winding, shaft and case. With a negative winding temperature coefficient and enough loop gain, the warmer motor takes the whole current — plate 3's boundary, now on composite connectors." }
    fn knob(&self) -> Knob {
        // Signed loop gain α·R_th·P: negative α hogs past −1, positive α
        // has no steady state past +1, so the knob stops short of it.
        let gain = self.drive.pair.loop_gain() * self.drive.pair.coefficient.signum();
        knob("signed loop gain α·R_th·P", "", -3.0, 0.9, 0.1, gain)
    }
    fn set_knob(&mut self, value: f64) { self.drive.pair = motor_hogging::with_gain(if value < 0.0 { -1.0 } else { 1.0 }, value.abs().max(0.05)); self.reset(); }
    fn reset(&mut self) { self.rig = self.drive.model(&self.registry, 0.01); }
    fn time(&self) -> f64 { self.rig.runtime.time }
    fn time_scale(&self) -> f64 { 8.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.rig.runtime, duration, 0.02) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let ambient = self.drive.pair.ambient;
        for k in 0..2 {
            let x = if k == 0 { -0.9 } else { 0.9 };
            let temperature = self.get(self.rig.temperatures[k]);
            let heat = ((temperature - ambient) / 60.0).clamp(0.0, 1.0) as f32;
            let color = [0.25 + 0.7 * heat, 0.35 + 0.2 * (1.0 - heat), 0.8 * (1.0 - heat)];
            out.push(Shape::block([x, 0.0, 0.0], [0.5, 0.5, 0.5], color));
            let angle = self.get(self.rig.angles[k]);
            let tip = [x + 0.45 * angle.cos(), 0.45 * angle.sin(), 0.35];
            out.push(Shape::Rod { from: [x, 0.0, 0.35], to: tip, radius: 0.04, color: paint::INK });
        }
        out.push(Shape::block([0.0, -0.6, 0.0], [2.6, 0.08, 0.6], paint::GROUND));
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![
            Readout::new("motor a share of current", self.share(), ""),
            Readout::new("case a", self.get(self.rig.temperatures[0]) - 273.15, "°C"),
            Readout::new("case b", self.get(self.rig.temperatures[1]) - 273.15, "°C"),
            Readout::new("drive: hottest case", self.get(self.rig.hottest) - 273.15, "°C"),
            Readout::new("loop gain |α|·R_th·P·R/(R+k²/c)", motor_hogging::free_rotor_gain(&self.drive), ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("motor a share of current", self.share()) }
    fn verdict(&self) -> String {
        let gain = -self.drive.pair.coefficient.signum() * motor_hogging::free_rotor_gain(&self.drive);
        if gain > 1.0 { format!("loop gain {gain:.2} > 1 — the warmer motor hogs") } else { format!("loop gain {gain:.2} < 1 — even split") }
    }
}

// ---------------------------------------------------------------- 19. The Levitron
pub struct LevitronExhibit {
    registry: BehaviorRegistry,
    levitron: levitron::Levitron,
    top: levitron::Top,
    /// The Floquet spin sweep takes a minute or two; it runs on its own
    /// thread from the first frame the exhibit is shown, and the readouts
    /// say so until it lands.
    window: std::sync::Arc<std::sync::Mutex<Option<(f64, f64)>>>,
    sweeping: bool,
}
impl LevitronExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let levitron = levitron::Levitron::default();
        let window = std::sync::Arc::new(std::sync::Mutex::new(None));
        let top = levitron.model(&registry, 1.0e-3);
        Self { registry, levitron, top, window, sweeping: false }
    }
    fn start_sweep(&mut self) {
        if self.sweeping { return; }
        self.sweeping = true;
        let levitron = self.levitron;
        let slot = self.window.clone();
        std::thread::spawn(move || {
            let found = levitron.spin_window(&crate::world::registry());
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(found);
        });
    }
    fn window(&self) -> Option<(f64, f64)> {
        *self.window.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn state(&self) -> Vec<f64> { self.top.body.iter().map(|id| self.top.runtime.get(*id)).collect() }
}
impl Exhibit for LevitronExhibit {
    fn title(&self) -> &'static str { "The Levitron" }
    fn summary(&self) -> &'static str { "A spinning magnet floats above a ring magnet. Static magnets alone cannot hold it (Earnshaw); spin lets its axis follow the field and opens a trap — but only between two spin rates the linearised compiled model predicts." }
    fn knob(&self) -> Knob { knob("spin (rpm)", "", 300.0, 6000.0, 50.0, self.levitron.spin * 60.0 / (2.0 * std::f64::consts::PI)) }
    fn set_knob(&mut self, value: f64) { self.levitron.spin = value * 2.0 * std::f64::consts::PI / 60.0; self.reset(); }
    fn reset(&mut self) { self.top = self.levitron.model(&self.registry, 1.0e-3); }
    fn time(&self) -> f64 { self.top.runtime.time }
    fn time_scale(&self) -> f64 { 0.8 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        self.start_sweep();
        advance(&mut self.top.runtime, duration, 2.0e-4)?;
        let s = self.state();
        if (s[2] - self.levitron.height).abs() > 0.03 || (s[0] * s[0] + s[1] * s[1]).sqrt() > 0.03 { self.reset(); }
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let k = 12.0;
        let a = self.levitron.ring_radius * k;
        for i in 0..28 {
            let phi = i as f64 / 28.0 * 2.0 * std::f64::consts::PI;
            out.push(Shape::block([a * phi.cos(), -0.05, a * phi.sin()], [0.12, 0.05, 0.12], paint::GROUND));
        }
        let s = self.state();
        let q = nalgebra::UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(s[3], s[4], s[5], s[6]));
        let axis = q * nalgebra::Vector3::new(0.0, 0.0, 1.0);
        // Viewer y is up: world z → y.
        let centre = [s[0] * k, s[2] * k, s[1] * k];
        let tip = [centre[0] + 0.35 * axis.x, centre[1] + 0.35 * axis.z, centre[2] + 0.35 * axis.y];
        let tail = [centre[0] - 0.35 * axis.x, centre[1] - 0.35 * axis.z, centre[2] - 0.35 * axis.y];
        out.push(Shape::Sphere { center: centre, radius: 0.22, color: paint::SURPRISE });
        out.push(Shape::Rod { from: tail, to: tip, radius: 0.04, color: paint::INK });
        let spin_phase = s[12] * self.time();
        let hand = q * nalgebra::Vector3::new(spin_phase.cos(), spin_phase.sin(), 0.0);
        out.push(Shape::Rod { from: centre, to: [centre[0] + 0.24 * hand.x, centre[1] + 0.24 * hand.z, centre[2] + 0.24 * hand.y], radius: 0.02, color: paint::CONTROL });
    }
    fn readouts(&self) -> Vec<Readout> {
        let s = self.state();
        let q = nalgebra::UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(s[3], s[4], s[5], s[6]));
        let axis = q * nalgebra::Vector3::new(0.0, 0.0, 1.0);
        vec![
            Readout::new("height above ring", s[2] * 1000.0, "mm"),
            Readout::new("lateral offset", (s[0] * s[0] + s[1] * s[1]).sqrt() * 1000.0, "mm"),
            Readout::new("axis tilt", axis.z.clamp(-1.0, 1.0).acos().to_degrees(), "°"),
            Readout::new("spin window", self.window().map(|w| w.0 * 60.0 / (2.0 * std::f64::consts::PI)).unwrap_or(f64::NAN), "rpm (lower)"),
            Readout::new("spin window", self.window().map(|w| w.1 * 60.0 / (2.0 * std::f64::consts::PI)).unwrap_or(f64::NAN), "rpm (upper)"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("height above ring (mm)", self.state()[2] * 1000.0) }
    fn verdict(&self) -> String {
        let spin = self.levitron.spin;
        if spin <= 0.0 { "no spin — Earnshaw: it cannot float".into() }
        else if let Some((lo, hi)) = self.window() {
            if spin < lo { "below the window — too slow, it tips over".into() }
            else if spin > hi { "above the window — too fast to follow the field, it slides out".into() }
            else { "inside the window — it flies".into() }
        } else { "computing the spin window (Floquet sweep) in the background…".into() }
    }
}

// ---------------------------------------------------------------- 20. The geyser
pub struct GeyserExhibit {
    registry: BehaviorRegistry,
    geyser: geyser::Geyser,
    conduit: geyser::Conduit,
    eruptions: Vec<f64>,
    above_threshold: bool,
}
impl GeyserExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let geyser = geyser::Geyser::default();
        let conduit = geyser.model(&registry);
        Self { registry, geyser, conduit, eruptions: Vec::new(), above_threshold: false }
    }
    fn segment(&self, k: usize) -> sim_domain_fluid::twophase::FluidState {
        sim_domain_fluid::twophase::Water::state(self.conduit.runtime.get(self.conduit.pressures[k]), self.conduit.runtime.get(self.conduit.enthalpies[k]))
    }
    fn outflow(&self) -> f64 { self.conduit.runtime.get(self.conduit.outflow) }
}
impl Exhibit for GeyserExhibit {
    fn title(&self) -> &'static str { "The geyser" }
    fn summary(&self) -> &'static str { "A water column heated at the bottom and fed from an aquifer. The weight of the water above holds the bottom below boiling until it flashes, lightens the column, and the whole thing erupts — then refills and does it again, faster the harder it is heated." }
    fn knob(&self) -> Knob { knob("heat into the bottom (kW)", "", 20.0, 300.0, 10.0, self.geyser.heat / 1000.0) }
    fn set_knob(&mut self, value: f64) { self.geyser.heat = value * 1000.0; self.reset(); }
    fn reset(&mut self) { self.conduit = self.geyser.model(&self.registry); self.eruptions.clear(); self.above_threshold = false; }
    fn time(&self) -> f64 { self.conduit.runtime.time }
    fn time_scale(&self) -> f64 { 60.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        advance(&mut self.conduit.runtime, duration, 0.05)?;
        let out = self.outflow();
        if out >= 0.5 && !self.above_threshold { self.eruptions.push(self.time()); }
        self.above_threshold = out >= 0.5;
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let n = self.geyser.segments;
        let h = 0.5;
        for k in 0..n {
            let s = self.segment(k);
            let steam = s.quality.clamp(0.0, 1.0) as f32;
            let warmth = ((s.temperature - 293.15) / 100.0).clamp(0.0, 1.0) as f32;
            let color = [0.2 + 0.7 * steam + 0.3 * warmth * (1.0 - steam), 0.45 + 0.5 * steam, 0.9 * (1.0 - 0.5 * warmth) + 0.1 * steam];
            out.push(Shape::block([0.0, (k as f64 + 0.5) * h, 0.0], [0.5, h * 0.96, 0.5], color));
        }
        out.push(Shape::block([0.0, -0.15, 0.0], [1.2, 0.1, 1.2], paint::GROUND));
        let glow = (self.geyser.heat / 3.0e5) as f32;
        out.push(Shape::block([0.0, -0.05, 0.0], [0.6, 0.04, 0.6], [0.9, 0.3 + 0.4 * (1.0 - glow), 0.1]));
        out.push(Shape::block([0.0, n as f64 * h + 0.03, 0.0], [1.6, 0.05, 1.6], [0.35, 0.55, 0.9]));
        let plume = (self.outflow() / 5.0).clamp(0.0, 1.0);
        if plume > 0.02 {
            out.push(Shape::Rod { from: [0.0, n as f64 * h, 0.0], to: [0.0, n as f64 * h + 2.0 * plume, 0.0], radius: 0.12 * plume + 0.02, color: paint::SURPRISE });
        }
    }
    fn readouts(&self) -> Vec<Readout> {
        let bottom = self.segment(0);
        let period = if self.eruptions.len() >= 2 { (self.eruptions[self.eruptions.len() - 1] - self.eruptions[0]) / (self.eruptions.len() - 1) as f64 } else { f64::NAN };
        vec![
            Readout::new("bottom temperature", bottom.temperature - 273.15, "°C"),
            Readout::new("bottom pressure", self.conduit.runtime.get(self.conduit.pressures[0]) / 1.0e5, "bar"),
            Readout::new("bottom steam quality", bottom.quality, ""),
            Readout::new("outflow at the mouth", self.outflow(), "kg/s"),
            Readout::new("eruptions so far", self.eruptions.len() as f64, ""),
            Readout::new("mean interval", period, "s"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("outflow at the mouth (kg/s)", self.outflow()) }
    fn verdict(&self) -> String {
        format!("{:.0} kW into a {:.0} m column — boiling point at the bottom {:.0} °C", self.geyser.heat / 1000.0, self.geyser.height(), sim_domain_fluid::twophase::Water::saturation_temperature(101_325.0 + 1000.0 * 9.81 * self.geyser.height()) - 273.15)
    }
}

// ---------------------------------------------------------------- 21. Semenov ignition
pub struct SemenovExhibit {
    registry: BehaviorRegistry,
    vessel: semenov_ignition::Vessel,
    batch: semenov_ignition::Batch,
    exact: f64,
    blown: bool,
}
impl SemenovExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let vessel = semenov_ignition::Vessel::default();
        let exact = vessel.tangency_wall_temperature();
        let vessel = semenov_ignition::Vessel { wall_temperature: exact - 5.0, ..vessel };
        let batch = vessel.model(&registry);
        Self { registry, vessel, batch, exact, blown: false }
    }
    fn temperature(&self) -> f64 { self.batch.runtime.get(self.batch.temperature) }
}
impl Exhibit for SemenovExhibit {
    fn title(&self) -> &'static str { "Semenov ignition" }
    fn summary(&self) -> &'static str { "A reacting mixture in a vessel with a fixed wall temperature. Arrhenius heat release against linear wall loss: below a critical wall temperature it simmers a few kelvin warm; above it, nothing can balance the exponential and it ignites." }
    fn knob(&self) -> Knob { knob("wall temperature (K)", "", self.exact - 40.0, self.exact + 20.0, 0.5, self.vessel.wall_temperature) }
    fn set_knob(&mut self, value: f64) { self.vessel.wall_temperature = value; self.reset(); }
    fn reset(&mut self) { self.batch = self.vessel.model(&self.registry); self.blown = false; }
    fn time(&self) -> f64 { self.batch.runtime.time }
    fn time_scale(&self) -> f64 { 20.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        if self.blown { return Ok(()); }
        if self.temperature() > self.vessel.wall_temperature + 100.0 { self.blown = true; return Ok(()); }
        match advance(&mut self.batch.runtime, duration, 0.05) {
            Ok(()) => Ok(()),
            Err(_) => { self.blown = true; Ok(()) }
        }
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let excess = self.temperature() - self.vessel.wall_temperature;
        let glow = (excess / 60.0).clamp(0.0, 1.0) as f32;
        let colour = [0.3 + 0.7 * glow, 0.3 + 0.3 * (1.0 - glow), 0.7 * (1.0 - glow)];
        out.push(Shape::block([0.0, 0.0, 0.0], [1.4, 1.0, 1.4], paint::GROUND));
        out.push(Shape::block([0.0, 0.05, 0.0], [1.2, 0.95, 1.2], colour));
        if self.blown { out.push(Shape::Sphere { center: [0.0, 1.2, 0.0], radius: 0.6, color: paint::SURPRISE }); }
    }
    fn readouts(&self) -> Vec<Readout> {
        let t = self.temperature();
        vec![
            Readout::new("vessel temperature", t, "K"),
            Readout::new("excess over the wall", t - self.vessel.wall_temperature, "K"),
            Readout::new("heat generated", self.vessel.generation(t), "W"),
            Readout::new("heat lost through the wall", self.vessel.loss(t), "W"),
            Readout::new("Semenov ψ (critical at 1/e = 0.368)", self.vessel.semenov_psi(), ""),
            Readout::new("critical wall temperature", self.exact, "K"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("excess over the wall (K)", self.temperature() - self.vessel.wall_temperature) }
    fn verdict(&self) -> String {
        if self.blown { "ignited".into() } else if self.vessel.wall_temperature > self.exact { format!("wall {:.1} K above critical — it will ignite", self.vessel.wall_temperature - self.exact) } else { format!("wall {:.1} K below critical — it simmers", self.exact - self.vessel.wall_temperature) }
    }
}

// ---------------------------------------------------------------- 22. Cooling below ambient
pub struct SkyCoolingExhibit {
    registry: BehaviorRegistry,
    radiator: sky_cooling::Radiator,
    panel: sky_cooling::Panel,
}
impl SkyCoolingExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let radiator = sky_cooling::Radiator::default();
        let panel = radiator.model(&registry);
        Self { registry, radiator, panel }
    }
    fn temperature(&self) -> f64 { self.panel.runtime.get(self.panel.temperature) }
}
impl Exhibit for SkyCoolingExhibit {
    fn title(&self) -> &'static str { "Cooling below ambient" }
    fn summary(&self) -> &'static str { "A panel that emits in the 8–13 µm atmospheric window and reflects the sun faces a clear sky. Through the window it radiates to space; it settles below the air temperature in direct sunlight. Make it grey — black to the sun too — and it bakes." }
    fn knob(&self) -> Knob { knob("solar absorptivity (a grey emitter is ~0.9)", "", 0.0, 0.95, 0.01, self.radiator.solar_absorptivity) }
    fn set_knob(&mut self, value: f64) {
        self.radiator.solar_absorptivity = value;
        // A surface that absorbs the sun broadly is not selective in the thermal bands either.
        self.radiator.outside_emissivity = 0.1 + 0.8 * (value / 0.9).clamp(0.0, 1.0);
        self.reset();
    }
    fn reset(&mut self) { self.panel = self.radiator.model(&self.registry); }
    fn time(&self) -> f64 { self.panel.runtime.time }
    fn time_scale(&self) -> f64 { 60.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.panel.runtime, duration, 1.0) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let dt = self.temperature() - self.radiator.air_temperature;
        let warm = (dt / 40.0).clamp(-1.0, 1.0) as f32;
        let colour = if warm > 0.0 { [0.4 + 0.6 * warm, 0.4 * (1.0 - warm), 0.2] } else { [0.4 * (1.0 + warm), 0.6, 0.6 - 0.4 * warm] };
        out.push(Shape::block([0.0, 0.0, 0.0], [2.0, 0.08, 1.4], colour));
        out.push(Shape::block([0.0, -0.3, 0.0], [2.2, 0.5, 1.6], paint::GROUND));
        if self.radiator.irradiance > 0.0 { out.push(Shape::Sphere { center: [1.6, 1.8, -0.8], radius: 0.25, color: [1.0, 0.85, 0.3] }); }
        let up = (-dt / 10.0).clamp(0.0, 1.0);
        if up > 0.05 { out.push(Shape::Arrow { from: [0.0, 0.1, 0.0], to: [0.0, 0.1 + 0.9 * up, 0.0], color: paint::SURPRISE }); }
    }
    fn readouts(&self) -> Vec<Readout> {
        let t = self.temperature();
        vec![
            Readout::new("panel temperature", t - 273.15, "°C"),
            Readout::new("air temperature", self.radiator.air_temperature - 273.15, "°C"),
            Readout::new("below the air by", self.radiator.air_temperature - t, "K"),
            Readout::new("absorbed sunlight", self.radiator.solar_absorptivity * self.radiator.irradiance, "W/m²"),
            Readout::new("energy balance predicts below the air by", self.radiator.air_temperature - self.radiator.balance_temperature(), "K"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("below the air (K)", self.radiator.air_temperature - self.temperature()) }
    fn verdict(&self) -> String {
        let dt = self.radiator.air_temperature - self.radiator.balance_temperature();
        if dt > 0.0 { format!("settles {dt:.1} K below the air, in the sun") } else { format!("settles {:.0} K above the air — it absorbs the sun", -dt) }
    }
}

// ---------------------------------------------------------------- 23. VIV lock-in
pub struct VivExhibit {
    registry: BehaviorRegistry,
    cable: viv_lock_in::Cable,
    span: viv_lock_in::Span,
    peak: f64,
}
impl VivExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let base = viv_lock_in::Cable::default();
        let cable = viv_lock_in::Cable { speed: 1.05 * base.natural_frequency() * base.diameter / 0.2, ..base };
        let span = cable.model(&registry);
        Self { registry, cable, span, peak: 0.0 }
    }
    fn shape(&self) -> Vec<f64> { self.span.displacements.iter().map(|id| self.span.runtime.get(*id)).collect() }
}
impl Exhibit for VivExhibit {
    fn title(&self) -> &'static str { "Vortex-induced vibration lock-in" }
    fn summary(&self) -> &'static str { "A taut cable in a cross-flow sheds vortices at the Strouhal frequency, which rises with the flow. Near the cable's mode the shedding locks to it across a band of speeds and the amplitude plateaus — because the wake is an oscillator that listens to the cable." }
    fn knob(&self) -> Knob { let base = &self.cable; knob("flow speed / (f₁·D/St) — 1 is nominal resonance", "", 0.4, 2.0, 0.05, base.speed / (base.natural_frequency() * base.diameter / 0.2)) }
    fn set_knob(&mut self, value: f64) { self.cable.speed = value * self.cable.natural_frequency() * self.cable.diameter / 0.2; self.reset(); }
    fn reset(&mut self) { self.span = self.cable.model(&self.registry); self.peak = 0.0; }
    fn time(&self) -> f64 { self.span.runtime.time }
    fn time_scale(&self) -> f64 { 1.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        advance(&mut self.span.runtime, duration, 4.0e-3)?;
        if self.time() > 20.0 { self.peak = self.peak.max(self.span.runtime.get(self.span.midpoint).abs()); }
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let y = self.shape();
        let n = y.len();
        let k = 3.0 / self.cable.diameter;
        let mut points = vec![[-1.5, 0.0, 0.0]];
        for (i, yi) in y.iter().enumerate() {
            let x = -1.5 + 3.0 * (i as f64 + 1.0) / (n as f64 + 1.0);
            points.push([x, yi * k * 0.2, 0.0]);
        }
        points.push([1.5, 0.0, 0.0]);
        for w in points.windows(2) {
            out.push(Shape::Rod { from: w[0], to: w[1], radius: 0.03, color: paint::INK });
        }
        out.push(Shape::block([-1.6, 0.0, 0.0], [0.1, 0.4, 0.4], paint::GROUND));
        out.push(Shape::block([1.6, 0.0, 0.0], [0.1, 0.4, 0.4], paint::GROUND));
        let flow = (self.cable.speed * 0.6).min(1.2);
        out.push(Shape::Arrow { from: [-1.0, -0.8, 0.0], to: [-1.0 + flow, -0.8, 0.0], color: paint::CONTROL });
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![
            Readout::new("reduced velocity U/(f₁D)", self.cable.reduced_velocity(), ""),
            Readout::new("shedding frequency St·U/D", self.cable.shedding_frequency(), "Hz"),
            Readout::new("cable first mode", self.cable.natural_frequency(), "Hz"),
            Readout::new("midpoint amplitude / D (after 20 s)", self.peak / self.cable.diameter, ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("midpoint / D", self.span.runtime.get(self.span.midpoint) / self.cable.diameter) }
    fn verdict(&self) -> String {
        let ratio = self.cable.shedding_frequency() / self.cable.natural_frequency();
        if (0.8..1.6).contains(&ratio) { format!("shedding at {ratio:.2} f₁ — inside the lock-in band") } else { format!("shedding at {ratio:.2} f₁ — off the band") }
    }
}

// ---------------------------------------------------------------- 24. Janssen's silo
pub struct JanssenExhibit {
    registry: BehaviorRegistry,
    silo: janssen_silo::Silo,
    bin: janssen_silo::Bin,
}
impl JanssenExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let silo = janssen_silo::Silo::default();
        let bin = silo.model(&registry);
        Self { registry, silo, bin }
    }
    fn mass(&self) -> f64 { self.bin.runtime.get(self.bin.mass) }
}
impl Exhibit for JanssenExhibit {
    fn title(&self) -> &'static str { "Janssen's silo" }
    fn summary(&self) -> &'static str { "Pour grain into a tall silo and watch the stress on its floor: it rises like a fluid's, then stops — the walls carry the rest through friction, and the floor never feels more than ρgD/(4μK) however much you pour." }
    fn knob(&self) -> Knob { knob("wall friction μ", "", 0.0, 0.8, 0.02, self.silo.friction) }
    fn set_knob(&mut self, value: f64) { self.silo.friction = value; self.reset(); }
    fn reset(&mut self) { self.bin = self.silo.model(&self.registry); }
    fn time(&self) -> f64 { self.bin.runtime.time }
    fn time_scale(&self) -> f64 { 10.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        advance(&mut self.bin.runtime, duration, 0.5)?;
        if self.silo.column().height(self.mass()) > 20.0 { self.reset(); }
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let column = self.silo.column();
        let h = column.height(self.mass()) * 0.15;
        out.push(Shape::block([0.0, h * 0.5, 0.0], [0.9, h.max(0.01), 0.9], [0.85, 0.7, 0.35]));
        out.push(Shape::block([-0.5, 1.5, 0.0], [0.06, 3.0, 0.9], paint::GROUND));
        out.push(Shape::block([0.5, 1.5, 0.0], [0.06, 3.0, 0.9], paint::GROUND));
        let stress = self.bin.runtime.get(self.bin.base_stress) / column.saturation_stress().max(1.0);
        out.push(Shape::block([0.0, -0.1, 0.0], [1.1, 0.12, 1.1], [0.3 + 0.6 * stress.min(1.0) as f32, 0.35, 0.8 * (1.0 - stress.min(1.0)) as f32]));
    }
    fn readouts(&self) -> Vec<Readout> {
        let column = self.silo.column();
        let stress = self.bin.runtime.get(self.bin.base_stress);
        let h = column.height(self.mass());
        vec![
            Readout::new("fill height", h, "m"),
            Readout::new("floor stress", stress / 1000.0, "kPa"),
            Readout::new("a fluid that deep would press", self.silo.density * 9.81 * h / 1000.0, "kPa"),
            Readout::new("Janssen saturation ρgD/(4μK)", if self.silo.friction > 0.0 { column.saturation_stress() / 1000.0 } else { f64::INFINITY }, "kPa"),
            Readout::new("depth scale D/(4μK)", if self.silo.friction > 0.0 { column.depth_scale() } else { f64::INFINITY }, "m"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("floor stress (kPa)", self.bin.runtime.get(self.bin.base_stress) / 1000.0) }
    fn verdict(&self) -> String {
        if self.silo.friction <= 0.0 { "frictionless walls — hydrostatic, the floor takes it all".into() } else { format!("saturates at {:.1} kPa within a few times {:.2} m", self.silo.column().saturation_stress() / 1000.0, self.silo.column().depth_scale()) }
    }
}

// ---------------------------------------------------------------- 25. Stochastic resonance
pub struct StochasticResonanceExhibit {
    registry: BehaviorRegistry,
    well: stochastic_resonance::Well,
    particle: stochastic_resonance::Particle,
    hops: usize,
    last_side: bool,
}
impl StochasticResonanceExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let well = stochastic_resonance::Well::default();
        let well = stochastic_resonance::Well { temperature: well.optimal_temperature(), ..well };
        let particle = well.model(&registry);
        Self { registry, well, particle, hops: 0, last_side: true }
    }
    fn x(&self) -> f64 { self.particle.runtime.get(self.particle.position) }
}
impl Exhibit for StochasticResonanceExhibit {
    fn title(&self) -> &'static str { "Stochastic resonance" }
    fn summary(&self) -> &'static str { "A particle in a double well, pushed too weakly to cross the barrier, plus thermal noise. Too little noise and nothing happens; too much and it hops at random; in between the hops synchronise with the push and the signal comes out of the noise stronger than it went in." }
    fn knob(&self) -> Knob { knob("bath temperature kT / Kramers optimum", "", 0.1, 4.0, 0.1, self.well.temperature / stochastic_resonance::Well::default().optimal_temperature()) }
    fn set_knob(&mut self, value: f64) { self.well.temperature = value * stochastic_resonance::Well::default().optimal_temperature(); self.reset(); }
    fn reset(&mut self) { self.particle = self.well.model(&self.registry); self.hops = 0; self.last_side = true; }
    fn time(&self) -> f64 { self.particle.runtime.time }
    fn time_scale(&self) -> f64 { 8.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        advance(&mut self.particle.runtime, duration, 0.02)?;
        let side = self.x() > 0.0;
        if side != self.last_side { self.hops += 1; self.last_side = side; }
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let (a, b) = (self.well.a, self.well.b);
        let potential = |x: f64| -0.5 * a * x * x + 0.25 * b * x.powi(4);
        let mut points = Vec::new();
        for i in 0..=40 {
            let x = -1.8 + 3.6 * i as f64 / 40.0;
            points.push([x, potential(x) * 2.0 + 0.5, 0.0]);
        }
        for w in points.windows(2) { out.push(Shape::Rod { from: w[0], to: w[1], radius: 0.02, color: paint::INK }); }
        let x = self.x();
        out.push(Shape::Sphere { center: [x, potential(x) * 2.0 + 0.62, 0.0], radius: 0.12, color: paint::SURPRISE });
        let drive = self.well.drive_amplitude * (2.0 * std::f64::consts::PI * self.well.drive_frequency * self.time()).cos();
        out.push(Shape::Arrow { from: [0.0, 1.4, 0.0], to: [0.0 + drive * 8.0, 1.4, 0.0], color: paint::CONTROL });
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![
            Readout::new("position", self.x(), ""),
            Readout::new("hops so far", self.hops as f64, ""),
            Readout::new("drive periods elapsed", self.time() * self.well.drive_frequency, ""),
            Readout::new("Kramers hops per period, 2r_K/f", 2.0 * self.well.kramers_rate() / self.well.drive_frequency, ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("position", self.x()) }
    fn verdict(&self) -> String {
        let ratio = 2.0 * self.well.kramers_rate() / self.well.drive_frequency;
        if ratio < 0.3 { "too cold — it sits in one well".into() } else if ratio > 3.0 { "too hot — it hops at random".into() } else { "near the optimum — the hops follow the push".into() }
    }
}

// ---------------------------------------------------------------- 26. The double pendulum
pub struct DoublePendulumExhibit {
    registry: BehaviorRegistry,
    pendulum: double_pendulum::Pendulum,
    chain: double_pendulum::Chain,
}
impl DoublePendulumExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let pendulum = double_pendulum::Pendulum { swing: 0.3, ..double_pendulum::Pendulum::default() };
        let chain = pendulum.model(&registry);
        Self { registry, pendulum, chain }
    }
    fn body(&self, ids: &[StateId; 6]) -> Vec<f64> { ids.iter().map(|id| self.chain.runtime.get(*id)).collect() }
}
impl Exhibit for DoublePendulumExhibit {
    fn title(&self) -> &'static str { "The double pendulum" }
    fn summary(&self) -> &'static str { "Two rods on revolute joints — the joints are constraint elements between owned frames, solved with multipliers. Rung gently it plays two notes, (2 ∓ √2)·g/L; swung hard it goes chaotic; weld the knee and one note is left." }
    fn knob(&self) -> Knob { knob("initial swing (rad); above ~1 it goes chaotic", "", 0.05, 3.0, 0.05, self.pendulum.swing) }
    fn set_knob(&mut self, value: f64) { self.pendulum.swing = value; self.reset(); }
    fn reset(&mut self) { self.chain = self.pendulum.model(&self.registry); }
    fn time(&self) -> f64 { self.chain.runtime.time }
    fn time_scale(&self) -> f64 { 1.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.chain.runtime, duration, 2.0e-3) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let (u, l) = (self.body(&self.chain.upper), self.body(&self.chain.lower));
        let len = self.pendulum.length;
        let hinge = |b: &[f64]| [b[0] - len * b[2].sin(), b[1] + len * b[2].cos(), 0.0];
        out.push(Shape::block([0.0, 0.05, 0.0], [0.6, 0.06, 0.3], paint::GROUND));
        out.push(Shape::Rod { from: hinge(&u), to: [u[0], u[1], 0.0], radius: 0.025, color: paint::INK });
        out.push(Shape::Sphere { center: [u[0], u[1], 0.0], radius: 0.09, color: paint::INK });
        out.push(Shape::Rod { from: hinge(&l), to: [l[0], l[1], 0.0], radius: 0.025, color: paint::SURPRISE });
        out.push(Shape::Sphere { center: [l[0], l[1], 0.0], radius: 0.09, color: paint::SURPRISE });
    }
    fn readouts(&self) -> Vec<Readout> {
        let (slow, fast) = self.pendulum.mode_frequencies();
        vec![
            Readout::new("upper angle", self.body(&self.chain.upper)[2].to_degrees(), "°"),
            Readout::new("lower angle", self.body(&self.chain.lower)[2].to_degrees(), "°"),
            Readout::new("in-phase note √((2−√2)g/L)", slow, "rad/s"),
            Readout::new("counter-phase note √((2+√2)g/L)", fast, "rad/s"),
            Readout::new("energy", self.chain.runtime.energy(), "J"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("lower mass x (m)", self.body(&self.chain.lower)[0]) }
    fn verdict(&self) -> String { if self.pendulum.swing < 0.5 { "two notes, (2 ∓ √2)·g/L".into() } else { "large swings — the two notes give way to chaos".into() } }
}

/// A motor rotor drawn as a disc with a spoke, plus a controller box.
fn rotor_shapes(out: &mut Vec<Shape>, angle: f64, speed: f64, controller: [f32; 3]) {
    out.push(Shape::block([0.0, -0.9, 0.0], [1.8, 0.05, 0.8], paint::GROUND));
    out.push(Shape::Rod { from: [0.0, -0.9, 0.0], to: [0.0, 0.0, 0.0], radius: 0.08, color: paint::STEEL });
    out.push(Shape::Sphere { center: [0.0, 0.0, 0.0], radius: 0.55, color: paint::INK });
    let tip = [0.5 * angle.cos(), 0.5 * angle.sin(), 0.05];
    out.push(Shape::Rod { from: [0.0, 0.0, 0.05], to: tip, radius: 0.05, color: paint::heat((speed / 60.0) as f32) });
    out.push(Shape::block([1.6, 0.0, 0.0], [0.35, 0.25, 0.25], controller));
    out.push(Shape::Line { from: [0.55, 0.0, 0.0], to: [1.25, 0.0, 0.0], color: paint::CONTROL });
}

// ---------------------------------------------------------------- 27. Language independence
pub struct LanguageExhibit {
    registry: BehaviorRegistry,
    law: language_independence::Law,
    /// 0 = Rust closure in-process, 1 = the Python client in a child process.
    which: f64,
    runtime: Runtime,
    speed: StateId,
    angle: StateId,
    python_ok: bool,
}
impl LanguageExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let law = language_independence::Law::default();
        let (runtime, _, speed, angle) = language_independence::plant(&registry, law.period);
        let mut exhibit = Self { registry, law, which: 0.0, runtime, speed, angle, python_ok: true };
        exhibit.reset();
        exhibit
    }
}
impl Exhibit for LanguageExhibit {
    fn title(&self) -> &'static str { "Language independence" }
    fn summary(&self) -> &'static str { "The same PI law closes the loop from a Rust closure or from a Python process over the seam; the traces are identical to the bit." }
    fn knob(&self) -> Knob { knob("controller (0 Rust, 1 Python)", "", 0.0, 1.0, 1.0, self.which) }
    fn set_knob(&mut self, value: f64) { self.which = value.round().clamp(0.0, 1.0); self.reset(); }
    fn reset(&mut self) {
        let (mut rt, seam, speed, angle) = language_independence::plant(&self.registry, self.law.period);
        let controller = if self.which > 0.5 {
            match language_independence::python_controller(self.law) {
                Ok(c) => { self.python_ok = true; c }
                Err(_) => { self.python_ok = false; language_independence::rust_controller(self.law) }
            }
        } else {
            language_independence::rust_controller(self.law)
        };
        rt.attach(seam, controller).expect("seam");
        self.runtime = rt;
        self.speed = speed;
        self.angle = angle;
    }
    fn time(&self) -> f64 { self.runtime.time }
    fn time_scale(&self) -> f64 { 0.25 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.runtime, duration, self.law.period / 4.0) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let color = if self.which > 0.5 { paint::GLOW } else { paint::CONTROL };
        rotor_shapes(out, self.runtime.get(self.angle), self.runtime.get(self.speed), color);
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![Readout::new("speed", self.runtime.get(self.speed), "rad/s"), Readout::new("setpoint", self.law.setpoint, "rad/s")]
    }
    fn signal(&self) -> (&'static str, f64) { ("speed (rad/s)", self.runtime.get(self.speed)) }
    fn verdict(&self) -> String {
        match (self.which > 0.5, self.python_ok) {
            (false, _) => "Rust closure in-process".to_owned(),
            (true, true) => "Python child process over the frame protocol — same trace".to_owned(),
            (true, false) => "python3 not available; running the Rust closure instead".to_owned(),
        }
    }
}

// ---------------------------------------------------------------- 28. Latency-induced instability
pub struct LatencyExhibit {
    registry: BehaviorRegistry,
    case: latency_instability::Loop,
    runtime: Runtime,
    speed: StateId,
    angle: StateId,
}
impl LatencyExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let case = latency_instability::Loop { latency: 1, ..latency_instability::Loop::default() };
        let plant = case.model(&registry);
        Self { registry, case, runtime: plant.runtime, speed: plant.speed, angle: plant.angle }
    }
}
impl Exhibit for LatencyExhibit {
    fn title(&self) -> &'static str { "Latency-induced instability" }
    fn summary(&self) -> &'static str { "A proportional speed loop that is stable at zero bus latency grows without bound a few samples later; the gain never changes." }
    fn knob(&self) -> Knob { knob("bus latency", "samples", 0.0, 6.0, 1.0, self.case.latency as f64) }
    fn set_knob(&mut self, value: f64) { self.case.latency = value.round().max(0.0) as usize; self.reset(); }
    fn reset(&mut self) {
        let plant = self.case.model(&self.registry);
        self.runtime = plant.runtime;
        self.speed = plant.speed;
        self.angle = plant.angle;
    }
    fn time(&self) -> f64 { self.runtime.time }
    fn time_scale(&self) -> f64 { 0.3 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        if self.runtime.get(self.speed).abs() > 1.0e4 {
            return Ok(());
        }
        advance(&mut self.runtime, duration, self.case.period / 5.0)
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let unstable = self.case.spectral_radius() > 1.0;
        rotor_shapes(out, self.runtime.get(self.angle), self.runtime.get(self.speed), if unstable { paint::SURPRISE } else { paint::CONTROL });
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![
            Readout::new("speed", self.runtime.get(self.speed), "rad/s"),
            Readout::new("spectral radius", self.case.spectral_radius(), ""),
            Readout::new("critical gain at this latency", self.case.critical_gain(), ""),
            Readout::new("loop gain", self.case.loop_gain, ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("speed (rad/s)", self.runtime.get(self.speed).clamp(-200.0, 200.0)) }
    fn verdict(&self) -> String {
        let radius = self.case.spectral_radius();
        if radius > 1.0 { format!("unstable: spectral radius {radius:.3} — the loop grows") } else { format!("stable: spectral radius {radius:.3}") }
    }
}

// ---------------------------------------------------------------- 29. Quantisation hunt
pub struct HuntExhibit {
    registry: BehaviorRegistry,
    hunt: quantisation_hunt::Hunt,
    runtime: Runtime,
    angle: StateId,
    measured: StateId,
}
impl HuntExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let hunt = quantisation_hunt::Hunt::default();
        let plant = hunt.model(&registry);
        Self { registry, hunt, runtime: plant.runtime, angle: plant.angle, measured: plant.measured }
    }
}
impl Exhibit for HuntExhibit {
    fn title(&self) -> &'static str { "Quantisation hunt" }
    fn summary(&self) -> &'static str { "Asked to hold half a count from a count edge, a PI position loop with an encoder hunts for ever; with a continuous angle it goes quiet." }
    fn knob(&self) -> Knob { knob("encoder counts per turn (0 = continuous)", "", 0.0, 4096.0, 256.0, self.hunt.counts) }
    fn set_knob(&mut self, value: f64) { self.hunt.counts = value.round().max(0.0); self.reset(); }
    fn reset(&mut self) {
        let plant = self.hunt.model(&self.registry);
        self.runtime = plant.runtime;
        self.angle = plant.angle;
        self.measured = plant.measured;
    }
    fn time(&self) -> f64 { self.runtime.time }
    fn time_scale(&self) -> f64 { 0.5 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.runtime, duration, self.hunt.period / 4.0) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        // The shaft angle magnified: one count spans the disc.
        let q = if self.hunt.counts > 0.0 { self.hunt.quantum() } else { TAU / 1024.0 };
        let angle = (self.runtime.get(self.angle) - self.hunt.setpoint()) / q * 0.6;
        rotor_shapes(out, 0.0, 0.0, paint::CONTROL);
        out.push(Shape::Line { from: [-0.9, -0.3, 0.1], to: [-0.9, 0.3, 0.1], color: paint::GLOW });
        out.push(Shape::Line { from: [0.9, -0.3, 0.1], to: [0.9, 0.3, 0.1], color: paint::GLOW });
        out.push(Shape::Sphere { center: [angle.clamp(-1.6, 1.6), 0.0, 0.1], radius: 0.12, color: paint::SURPRISE });
    }
    fn readouts(&self) -> Vec<Readout> {
        let q = if self.hunt.counts > 0.0 { self.hunt.quantum() } else { f64::NAN };
        vec![
            Readout::new("angle − setpoint", (self.runtime.get(self.angle) - self.hunt.setpoint()) / q, "counts"),
            Readout::new("encoder reading", self.runtime.get(self.measured), "rad"),
            Readout::new("predicted hunt amplitude", self.hunt.predicted_cycle().map(|(a, _)| a / q).unwrap_or(0.0), "counts"),
            Readout::new("predicted hunt frequency", self.hunt.predicted_cycle().map(|(_, f)| f).unwrap_or(0.0), "Hz"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) {
        let q = if self.hunt.counts > 0.0 { self.hunt.quantum() } else { TAU / 1024.0 };
        ("angle − setpoint (counts)", (self.runtime.get(self.angle) - self.hunt.setpoint()) / q)
    }
    fn verdict(&self) -> String {
        if self.hunt.counts > 0.0 { format!("{} counts: the loop hunts about the count edge", self.hunt.counts) } else { "continuous angle: the loop settles".to_owned() }
    }
}

// ---------------------------------------------------------------- 30. Missed deadlines (real-time mode)
pub struct DeadlineExhibit {
    registry: BehaviorRegistry,
    case: missed_deadlines::Deadline,
    runtime: Option<Runtime>,
    speed: StateId,
    angle: StateId,
    missed: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    samples: u64,
    error: Option<String>,
}
impl DeadlineExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let case = missed_deadlines::Deadline::default();
        let (rt, _, speed, angle) = language_independence::plant(&registry, case.period);
        let mut exhibit = Self { registry, case, runtime: Some(rt), speed, angle, missed: None, samples: 0, error: None };
        exhibit.reset();
        exhibit
    }
}
impl Exhibit for DeadlineExhibit {
    fn title(&self) -> &'static str { "Missed deadlines (real-time mode)" }
    fn summary(&self) -> &'static str { "The Python controller answers on the wall clock; when its compute time passes the sample period every deadline is missed, commands land a sample late, and the loop grows." }
    fn knob(&self) -> Knob { knob("controller compute time", "ms", 0.0, 25.0, 1.0, self.case.busy * 1.0e3) }
    fn set_knob(&mut self, value: f64) { self.case.busy = value.max(0.0) * 1.0e-3; self.reset(); }
    fn reset(&mut self) {
        let (mut rt, seam, speed, angle) = language_independence::plant(&self.registry, self.case.period);
        rt.set(speed, 1.0).ok();
        let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../clients/python/examples/pi_controller.py");
        let inner = language_independence::spawn_python(&[script.to_str().unwrap(), "--kp", &self.case.kp().to_string(), "--ki", "0", "--setpoint", "0", "--sensor", "speed", "--actuator", "voltage", "--busy", &self.case.busy.to_string()]);
        match inner {
            Ok(inner) => {
                let realtime = sim_couple::RealTime::new(Box::new(inner), std::time::Duration::from_secs_f64(self.case.period));
                self.missed = Some(realtime.missed());
                rt.attach(seam, Box::new(realtime)).expect("seam");
                self.error = None;
            }
            Err(e) => {
                self.missed = None;
                self.error = Some(format!("python3 unavailable: {e}"));
                rt.attach(seam, language_independence::rust_controller(language_independence::Law { kp: self.case.kp(), ki: 0.0, setpoint: 0.0, limit: f64::INFINITY, period: self.case.period })).expect("seam");
            }
        }
        self.runtime = Some(rt);
        self.speed = speed;
        self.angle = angle;
        self.samples = 0;
    }
    fn time(&self) -> f64 { self.runtime.as_ref().map(|r| r.time).unwrap_or(0.0) }
    fn time_scale(&self) -> f64 { 1.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        let Some(rt) = self.runtime.as_mut() else { return Ok(()) };
        if rt.get(self.speed).abs() > 1.0e3 {
            return Ok(());
        }
        let before = rt.time;
        advance(rt, duration, self.case.period / 4.0)?;
        self.samples += ((rt.time - before) / self.case.period).round() as u64;
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let Some(rt) = self.runtime.as_ref() else { return };
        let missed = self.missed.as_ref().map(|m| m.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(0);
        let late = self.samples > 0 && missed * 2 > self.samples;
        rotor_shapes(out, rt.get(self.angle), rt.get(self.speed), if late { paint::SURPRISE } else { paint::PASS });
    }
    fn readouts(&self) -> Vec<Readout> {
        let Some(rt) = self.runtime.as_ref() else { return vec![Readout::new("speed", 0.0, "rad/s")] };
        let missed = self.missed.as_ref().map(|m| m.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(0) as f64;
        vec![
            Readout::new("speed", rt.get(self.speed), "rad/s"),
            Readout::new("missed deadlines", missed, ""),
            Readout::new("samples", self.samples as f64, ""),
            Readout::new("deadline", self.case.period * 1.0e3, "ms"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("speed (rad/s)", self.runtime.as_ref().map(|r| r.get(self.speed)).unwrap_or(0.0).clamp(-200.0, 200.0)) }
    fn verdict(&self) -> String {
        if let Some(e) = &self.error { return e.clone(); }
        if self.case.busy < self.case.period { "compute time under the period: deadlines kept, the loop decays".to_owned() } else { "compute time over the period: every deadline missed, the loop grows".to_owned() }
    }
}

// ---------------------------------------------------------------- 31. The leg on the seam
pub struct LegExhibit {
    registry: BehaviorRegistry,
    leg: leg_seam::Leg,
    /// Knee target step (rad) applied at 0.5 s.
    step: f64,
    plant: Option<leg_seam::Plant>,
    error: Option<String>,
}
impl LegExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let mut exhibit = Self { registry, leg: leg_seam::Leg { compliant: true, ..leg_seam::Leg::default() }, step: -0.4, plant: None, error: None };
        exhibit.reset();
        exhibit
    }
    fn angles(&self) -> [f64; 3] {
        match &self.plant {
            Some(p) => [0, 1, 2].map(|k| p.runtime.get(p.angles[k])),
            None => self.leg.initial,
        }
    }
}
impl Exhibit for LegExhibit {
    fn title(&self) -> &'static str { "The leg on the seam" }
    fn summary(&self) -> &'static str { "The robot leg rebuilt from library parts — chain, motors, gears, transmissions, sensors, contacts — with a Python process closing the joint loops through the seam." }
    fn knob(&self) -> Knob { knob("knee target step at 0.5 s", "rad", -0.6, 0.6, 0.1, self.step) }
    fn set_knob(&mut self, value: f64) { self.step = value; self.reset(); }
    fn reset(&mut self) {
        let mut plant = self.leg.model(&self.registry);
        let target = [self.leg.initial[0], self.leg.initial[1] + self.step, self.leg.initial[2]];
        match self.leg.controller(target, self.leg.initial, 0.5, false) {
            Ok(c) => {
                plant.runtime.attach(plant.seam, c).expect("seam");
                self.error = None;
            }
            Err(e) => self.error = Some(format!("python3 unavailable: {e}")),
        }
        self.plant = Some(plant);
    }
    fn time(&self) -> f64 { self.plant.as_ref().map(|p| p.runtime.time).unwrap_or(0.0) }
    fn time_scale(&self) -> f64 { 0.25 }
    fn grid(&self) -> f64 { 1.0e-3 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        if self.error.is_some() { return Ok(()); }
        let Some(p) = self.plant.as_mut() else { return Ok(()) };
        advance(&mut p.runtime, duration, 1.0e-3)
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let scale = 2.5;
        let q = self.angles();
        out.push(Shape::block([0.0, 0.0, 0.0], [1.6, 0.03, 0.6], paint::GROUND));
        let mut phi = 0.0;
        let mut p = [0.0, self.leg.hip_height * scale];
        out.push(Shape::block([p[0], p[1], 0.0], [0.25, 0.08, 0.2], paint::STEEL));
        for (k, (length, _, _)) in self.leg.links.iter().enumerate() {
            phi += q[k];
            let next = [p[0] + length * scale * phi.cos(), p[1] + length * scale * phi.sin()];
            out.push(Shape::Sphere { center: [p[0], p[1], 0.0], radius: 0.09, color: paint::CONTROL });
            out.push(Shape::Rod { from: [p[0], p[1], 0.0], to: [next[0], next[1], 0.0], radius: 0.05, color: if k == 2 { paint::SURPRISE } else { paint::INK } });
            p = next;
        }
    }
    fn readouts(&self) -> Vec<Readout> {
        let q = self.angles();
        let mut out = vec![Readout::new("hip", q[0], "rad"), Readout::new("knee", q[1], "rad"), Readout::new("ankle", q[2], "rad")];
        if let Some(p) = &self.plant {
            out.push(Readout::new("hip current", p.runtime.get(p.currents[0]), "A"));
            out.push(Readout::new("foot height", p.runtime.get(p.tip[1]), "m"));
        }
        out
    }
    fn signal(&self) -> (&'static str, f64) { ("knee angle (rad)", self.angles()[1]) }
    fn verdict(&self) -> String {
        if let Some(e) = &self.error { return e.clone(); }
        format!("Python joint loops over the seam; knee target {:+.2} rad at 0.5 s", self.step)
    }
}

// ---------------------------------------------------------------- 32. The quadruped's trot
pub struct QuadrupedExhibit {
    registry: BehaviorRegistry,
    quadruped: quadruped_gait::Quadruped,
    plant: Option<quadruped_gait::Plant>,
    error: Option<String>,
}
impl QuadrupedExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let mut exhibit = Self { registry, quadruped: quadruped_gait::Quadruped::default(), plant: None, error: None };
        exhibit.reset();
        exhibit
    }
}
impl Exhibit for QuadrupedExhibit {
    fn title(&self) -> &'static str { "The quadruped's trot" }
    fn summary(&self) -> &'static str { "A floating body on four chain legs with servo joints; a Python process trots it through the seam — diagonal pairs alternate, stance feet sweep back, the body advances a stride per period." }
    fn knob(&self) -> Knob { knob("stride", "m", 0.0, 0.2, 0.02, self.quadruped.stride) }
    fn set_knob(&mut self, value: f64) { self.quadruped.stride = value.max(0.0); self.reset(); }
    fn reset(&mut self) {
        let mut plant = self.quadruped.model(&self.registry);
        // The C controller when a compiler is at hand, Python otherwise.
        let controller = self.quadruped.controller_in(self.quadruped.stride, quadruped_gait::Lang::Dylib).or_else(|_| self.quadruped.controller_in(self.quadruped.stride, quadruped_gait::Lang::C)).or_else(|_| self.quadruped.controller(self.quadruped.stride));
        match controller {
            Ok(c) => {
                plant.runtime.attach(plant.seam, c).expect("seam");
                self.error = None;
            }
            Err(e) => self.error = Some(format!("no controller available: {e}")),
        }
        self.plant = Some(plant);
    }
    fn time(&self) -> f64 { self.plant.as_ref().map(|p| p.runtime.time).unwrap_or(0.0) }
    fn time_scale(&self) -> f64 { 0.25 }
    fn grid(&self) -> f64 { 1.0e-3 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        if self.error.is_some() { return Ok(()); }
        let Some(p) = self.plant.as_mut() else { return Ok(()) };
        advance(&mut p.runtime, duration, 1.0e-3)
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let Some(p) = self.plant.as_ref() else { return };
        let scale = 2.0;
        let q = &self.quadruped;
        let rt = &p.runtime;
        let (bx, by, bth) = (rt.get(p.body[0]), rt.get(p.body[1]), rt.get(p.body[2]));
        out.push(Shape::block([bx * scale, 0.0, 0.0], [2.5, 0.02, 0.8], paint::GROUND));
        let rot = crate::exhibit::rotation_between([1.0, 0.0, 0.0], [bth.cos(), bth.sin(), 0.0]);
        out.push(Shape::Block { center: [bx * scale, by * scale, 0.0], half: [q.hip_x * scale, 0.06 * scale, 0.15], rotation: rot, color: paint::INK });
        for (k, leg) in quadruped_gait::LEGS.iter().enumerate() {
            let front = leg.starts_with('f');
            let left = leg.ends_with('l');
            let z = if left { 0.18 } else { -0.18 };
            let (ax, ay) = (if front { q.hip_x } else { -q.hip_x }, q.hip_y);
            let hip = [bx + ax * bth.cos() - ay * bth.sin(), by + ax * bth.sin() + ay * bth.cos()];
            let mut phi = bth;
            let mut pt = hip;
            let color = if left { paint::CONTROL } else { paint::STEEL };
            for (j, (length, _)) in [q.thigh, q.shank].iter().enumerate() {
                phi += rt.get(p.joints[k][j]);
                let next = [pt[0] + length * phi.cos(), pt[1] + length * phi.sin()];
                out.push(Shape::Rod { from: [pt[0] * scale, pt[1] * scale, z], to: [next[0] * scale, next[1] * scale, z], radius: 0.04, color });
                pt = next;
            }
            out.push(Shape::Sphere { center: [pt[0] * scale, pt[1] * scale, z], radius: 0.06, color: paint::SURPRISE });
        }
    }
    fn readouts(&self) -> Vec<Readout> {
        let Some(p) = self.plant.as_ref() else { return vec![Readout::new("x", 0.0, "m")] };
        vec![
            Readout::new("body x", p.runtime.get(p.body[0]), "m"),
            Readout::new("body height", p.runtime.get(p.body[1]), "m"),
            Readout::new("pitch", p.runtime.get(p.body[2]), "rad"),
            Readout::new("stride / period", self.quadruped.stride / self.quadruped.gait_period, "m/s"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("body x (m)", self.plant.as_ref().map(|p| p.runtime.get(p.body[0])).unwrap_or(0.0)) }
    fn verdict(&self) -> String {
        if let Some(e) = &self.error { return e.clone(); }
        if self.quadruped.stride > 0.0 { format!("trotting at a stride of {:.2} m every {:.1} s", self.quadruped.stride, self.quadruped.gait_period) } else { "marching on the spot".to_owned() }
    }
}

// ---------------------------------------------------------------- 33. The scaling ladder
pub struct LadderExhibit {
    registry: BehaviorRegistry,
    ladder: scaling_ladder::Ladder,
    rig: scaling_ladder::Rig,
    ms_per_step: f64,
}
impl LadderExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let ladder = scaling_ladder::Ladder { rungs: 100, ..scaling_ladder::Ladder::default() };
        let rig = ladder.model(&registry);
        Self { registry, ladder, rig, ms_per_step: 0.0 }
    }
}
impl Exhibit for LadderExhibit {
    fn title(&self) -> &'static str { "The scaling ladder" }
    fn summary(&self) -> &'static str { "A ladder of inertias, springs, dampers and tachometers; turn the rung count up and watch the cost per step grow about linearly, not cubically." }
    fn knob(&self) -> Knob { knob("rungs", "", 25.0, 800.0, 25.0, self.ladder.rungs as f64) }
    fn set_knob(&mut self, value: f64) { self.ladder.rungs = value.round().max(1.0) as usize; self.reset(); }
    fn reset(&mut self) { self.rig = self.ladder.model(&self.registry); self.ms_per_step = 0.0; }
    fn time(&self) -> f64 { self.rig.runtime.time }
    fn time_scale(&self) -> f64 { 0.5 }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        let h = 1.0e-3;
        let steps = (duration / h).round().max(1.0);
        let started = std::time::Instant::now();
        advance(&mut self.rig.runtime, duration, h)?;
        self.ms_per_step = started.elapsed().as_secs_f64() * 1.0e3 / steps;
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        // The top forty rungs as a column of discs, each turned by its angle.
        let shown = self.rig.speeds.len().min(40);
        let spacing = 3.0 / shown as f64;
        for (k, id) in self.rig.speeds.iter().rev().take(shown).enumerate() {
            let y = 1.5 - k as f64 * spacing;
            let speed = self.rig.runtime.get(*id);
            out.push(Shape::Rod { from: [-0.4, y, 0.0], to: [0.4, y, 0.0], radius: 0.03, color: paint::heat((speed / 3.0) as f32) });
        }
        out.push(Shape::Rod { from: [0.0, -1.6, -0.1], to: [0.0, 1.6, -0.1], radius: 0.02, color: paint::STEEL });
    }
    fn readouts(&self) -> Vec<Readout> {
        vec![
            Readout::new("unknowns stored", self.rig.unknowns_stored as f64, ""),
            Readout::new("unknowns solved", self.rig.unknowns_solved as f64, ""),
            Readout::new("ms per step", self.ms_per_step, "ms"),
            Readout::new("top rotor speed", self.rig.runtime.get(*self.rig.speeds.last().unwrap()), "rad/s"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("ms per step", self.ms_per_step) }
    fn verdict(&self) -> String { format!("{} rungs, {} unknowns solved of {}: {:.2} ms per step", self.ladder.rungs, self.rig.unknowns_solved, self.rig.unknowns_stored, self.ms_per_step) }
}

// ---------------------------------------------------------------- 34. Cruise control on a hill
pub struct CruiseExhibit {
    registry: BehaviorRegistry,
    car: cruise_control::Car,
    plant: cruise_control::Plant,
    x: StateId,
    y: StateId,
    theta: StateId,
}
impl CruiseExhibit {
    pub fn new() -> Self {
        let registry = registry();
        let car = cruise_control::Car::default();
        let (plant, x, y, theta) = Self::build(&registry, &car);
        Self { registry, car, plant, x, y, theta }
    }
    fn build(registry: &BehaviorRegistry, car: &cruise_control::Car) -> (cruise_control::Plant, StateId, StateId, StateId) {
        let mut plant = car.model(registry);
        plant.runtime.attach(plant.seam, car.controller()).expect("seam");
        let body = plant.runtime.model.behaviors.keys().next().expect("a body");
        let ids = ["x", "y", "theta"].map(|n| plant.runtime.state_id(body, n));
        (plant, ids[0], ids[1], ids[2])
    }
}
impl Exhibit for CruiseExhibit {
    fn title(&self) -> &'static str { "Cruise control on a hill" }
    fn summary(&self) -> &'static str { "A two-wheel car holds its set speed through the seam; on a grade the integrator winds to m·g·sin θ·r and the speed does not move." }
    fn knob(&self) -> Knob { knob("grade", "%", -10.0, 10.0, 1.0, self.car.slope * 100.0) }
    fn set_knob(&mut self, value: f64) { self.car.slope = value / 100.0; self.reset(); }
    fn reset(&mut self) {
        let (plant, x, y, theta) = Self::build(&self.registry, &self.car);
        self.plant = plant;
        self.x = x;
        self.y = y;
        self.theta = theta;
    }
    fn time(&self) -> f64 { self.plant.runtime.time }
    fn time_scale(&self) -> f64 { 1.0 }
    fn advance(&mut self, duration: f64) -> Result<(), String> { advance(&mut self.plant.runtime, duration, 2.0e-3) }
    fn shapes(&self, out: &mut Vec<Shape>) {
        // The road is drawn tilted by the grade; the car stays centred.
        let rt = &self.plant.runtime;
        let (bx, by, bth) = (rt.get(self.x), rt.get(self.y), rt.get(self.theta));
        let tilt = self.car.slope;
        let road = crate::exhibit::rotation_between([1.0, 0.0, 0.0], [tilt.cos(), tilt.sin(), 0.0]);
        out.push(Shape::Block { center: [0.0, -0.3, 0.0], half: [4.0, 0.03, 1.0], rotation: road, color: paint::GROUND });
        let car_rotation = crate::exhibit::rotation_between([1.0, 0.0, 0.0], [(bth + tilt).cos(), (bth + tilt).sin(), 0.0]);
        let (cx, cy) = (0.0, by * tilt.cos() - 0.3 + 0.3);
        out.push(Shape::Block { center: [cx, cy, 0.0], half: [self.car.wheelbase * 0.5 + 0.3, 0.25, 0.5], rotation: car_rotation, color: paint::INK });
        for px in [-0.5 * self.car.wheelbase, 0.5 * self.car.wheelbase] {
            let (wx, wy) = (cx + px * (bth + tilt).cos(), cy - 0.2 + px * (bth + tilt).sin());
            out.push(Shape::Sphere { center: [wx, wy, 0.55], radius: self.car.radius, color: paint::STEEL });
            out.push(Shape::Sphere { center: [wx, wy, -0.55], radius: self.car.radius, color: paint::STEEL });
        }
        let _ = bx;
    }
    fn readouts(&self) -> Vec<Readout> {
        let rt = &self.plant.runtime;
        vec![
            Readout::new("speed", -rt.get(self.plant.vx), "m/s"),
            Readout::new("axle torque", rt.get(self.plant.torque), "N·m"),
            Readout::new("grade torque m·g·sinθ·r", self.car.mass * 9.81 * self.car.slope.sin().abs() * self.car.radius, "N·m"),
            Readout::new("distance", -rt.get(self.x), "m"),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("speed (m/s)", -self.plant.runtime.get(self.plant.vx)) }
    fn verdict(&self) -> String { format!("grade {:+.0} %: set speed {} m/s", self.car.slope * 100.0, self.car.setpoint) }
}

// ---------------------------------------------------------------- 35. Walk the plank
pub struct PlankExhibit {
    env: walk_the_plank::PlankEnv,
    level: f64,
    seed: u64,
    crossings: usize,
    falls: usize,
    last: String,
    error: Option<String>,
}
impl PlankExhibit {
    pub fn new() -> Self {
        let mut exhibit = Self { env: walk_the_plank::PlankEnv::new(walk_the_plank::Biped::default(), walk_the_plank::Course::Flat), level: 0.0, seed: 1, crossings: 0, falls: 0, last: String::new(), error: None };
        exhibit.start();
        exhibit
    }
    fn start(&mut self) {
        use sim_couple::Environment;
        self.error = self.env.reset(self.seed, self.level).err();
    }
    fn body(&self) -> Option<walk_the_plank::Body> {
        self.env.plant.as_ref().map(|p| p.body())
    }
}
impl Exhibit for PlankExhibit {
    fn title(&self) -> &'static str { "Walk the plank" }
    fn summary(&self) -> &'static str { "A point-foot biped on stepping stones generated from the curriculum level: the LIP planner picks the next stone, times the step from the capture point and swings the foot; a learner is meant to refine it. Harder levels open the gaps until the planner alone falls." }
    fn knob(&self) -> Knob { knob("curriculum level", "", 0.0, 1.0, 0.1, self.level) }
    fn set_knob(&mut self, value: f64) { self.level = value.clamp(0.0, 1.0); self.reset(); }
    fn reset(&mut self) {
        self.crossings = 0;
        self.falls = 0;
        self.last.clear();
        self.start();
    }
    fn time(&self) -> f64 { self.body().map(|b| b.t).unwrap_or(0.0) }
    fn time_scale(&self) -> f64 { 0.2 }
    fn grid(&self) -> f64 { self.env.biped.policy_period }
    fn advance(&mut self, duration: f64) -> Result<(), String> {
        use sim_couple::Environment;
        if self.error.is_some() { return Ok(()); }
        let steps = (duration / self.env.biped.policy_period).round().max(1.0) as usize;
        for _ in 0..steps {
            let action = self.env.planner_action();
            let frame = self.env.step(&action)?;
            if frame.done {
                if self.env.success { self.crossings += 1; self.last = format!("crossed seed {}", self.seed); } else { self.falls += 1; self.last = format!("fell on seed {} at x = {:.2} m", self.seed, frame.privileged[0]); }
                self.seed += 1;
                self.start();
                break;
            }
        }
        Ok(())
    }
    fn shapes(&self, out: &mut Vec<Shape>) {
        let Some(plant) = self.env.plant.as_ref() else { return };
        let body = plant.body();
        let biped = &self.env.biped;
        let scale = 0.85;
        let place = |x: f64, y: f64, z: f64| [(x - 2.4) * scale, (y + 0.3) * scale, z];
        for (x0, x1, y) in &plant.terrain.patches {
            let c = place(0.5 * (x0 + x1), y - 0.06, 0.0);
            out.push(Shape::block(c, [0.5 * (x1 - x0) * scale, 0.06 * scale, 0.35], paint::GROUND));
        }
        let (bx, by, bth) = (body.torso[0], body.torso[1], body.torso[2]);
        let rot = crate::exhibit::rotation_between([1.0, 0.0, 0.0], [bth.cos(), bth.sin(), 0.0]);
        out.push(Shape::Block { center: place(bx, by, 0.0), half: [0.08 * scale, 0.16 * scale, 0.10], rotation: rot, color: paint::INK });
        let hip = walk_the_plank::Planner::hip(biped, &body.torso);
        if let Some(planner) = self.env.planner.as_ref() {
            for k in 0..2 {
                let z = if k == 0 { 0.12 } else { -0.12 };
                let color = if k == planner.stance { paint::CONTROL } else { paint::STEEL };
                let mut phi = bth;
                let mut pt = hip;
                for (j, (length, _)) in [biped.thigh, biped.shank].iter().enumerate() {
                    phi += body.joints[k][j];
                    let next = [pt[0] + length * phi.cos(), pt[1] + length * phi.sin()];
                    out.push(Shape::Rod { from: place(pt[0], pt[1], z), to: place(next[0], next[1], z), radius: 0.035, color });
                    pt = next;
                }
                out.push(Shape::Sphere { center: place(pt[0], pt[1], z), radius: 0.05, color: if body.foot_force[k] > 5.0 { paint::SURPRISE } else { paint::GLOW } });
            }
            out.push(Shape::Sphere { center: place(planner.target[0], planner.target[1] + 0.02, 0.0), radius: 0.03, color: paint::PASS });
        }
    }
    fn readouts(&self) -> Vec<Readout> {
        let body = self.body();
        let steps = self.env.planner.map(|p| p.steps).unwrap_or(0);
        vec![
            Readout::new("torso x", body.map(|b| b.torso[0]).unwrap_or(0.0), "m"),
            Readout::new("speed", body.map(|b| b.torso[3]).unwrap_or(0.0), "m/s"),
            Readout::new("steps this course", steps as f64, ""),
            Readout::new("crossed", self.crossings as f64, ""),
            Readout::new("fell", self.falls as f64, ""),
        ]
    }
    fn signal(&self) -> (&'static str, f64) { ("torso x (m)", self.body().map(|b| b.torso[0]).unwrap_or(0.0)) }
    fn verdict(&self) -> String {
        if let Some(e) = &self.error { return e.clone(); }
        let widest = self.env.plant.as_ref().map(|p| p.terrain.max_gap()).unwrap_or(0.0);
        if self.last.is_empty() { format!("level {:.1}: gaps up to {:.2} m; the planner alone, no learning", self.level, widest) } else { format!("level {:.1}: {} — {} crossed, {} fell", self.level, self.last, self.crossings, self.falls) }
    }
}
