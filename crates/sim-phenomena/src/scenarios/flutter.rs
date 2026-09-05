//! 6. Flutter — `fluid` `structural`.
//!
//! A pitch–plunge section (plunge mass, pitch inertia, their static
//! unbalance, springs and dampers to ground) under quasi-steady aerodynamic
//! loads. Below the flutter speed every gust decays; above it the section
//! extracts energy from the flow. The boundary is the eigenvalue crossing
//! of the compiled model's linearisation.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_fluid as fl;
use sim_domain_multibody::elements as mb;
use sim_dynamics::analysis::{envelope_rate, period};
use sim_dynamics::linear::{leading_mode, linearise};

#[derive(Clone, Copy)]
pub struct WingSection {
    pub mass: f64,
    pub semichord: f64,
    pub axis_offset: f64,
    pub cg_offset: f64,
    pub gyration: f64,
    pub plunge_frequency: f64,
    pub pitch_frequency: f64,
    pub plunge_damping_ratio: f64,
    pub pitch_damping_ratio: f64,
    pub air_density: f64,
    pub pitch_locked: bool,
}

impl Default for WingSection {
    fn default() -> Self {
        Self { mass: 20.0, semichord: 0.25, axis_offset: -0.2, cg_offset: 0.15, gyration: 0.55, plunge_frequency: std::f64::consts::TAU * 4.0, pitch_frequency: std::f64::consts::TAU * 10.0, plunge_damping_ratio: 0.01, pitch_damping_ratio: 0.01, air_density: 1.225, pitch_locked: false }
    }
}

pub struct Section {
    pub runtime: Runtime,
    pub plunge: StateId,
    pub pitch: StateId,
}

impl WingSection {
    pub fn model(&self, registry: &BehaviorRegistry, airspeed: f64, initial_plunge: f64) -> Section {
        let (m, b) = (self.mass, self.semichord);
        let unbalance = m * b * self.cg_offset;
        let pitch_inertia = m * (b * self.gyration).powi(2);
        let mut w = ModelWorld::default();
        let section = w.part(registry, "section", mb::PITCH_PLUNGE_SECTION, [
            ("mass", m), ("unbalance", unbalance), ("pitch_inertia", pitch_inertia),
            ("plunge_stiffness", m * self.plunge_frequency.powi(2)), ("plunge_damping", 2.0 * self.plunge_damping_ratio * m * self.plunge_frequency),
            ("pitch_stiffness", pitch_inertia * self.pitch_frequency.powi(2)), ("pitch_locked", if self.pitch_locked { 1.0 } else { 0.0 }),
            ("pitch_damping", 2.0 * self.pitch_damping_ratio * pitch_inertia * self.pitch_frequency),
            ("initial.plunge.position", initial_plunge),
        ]).unwrap();
        let aero = w.part(registry, "air", fl::QUASI_STEADY_SECTION, [("air_density", self.air_density), ("airspeed", airspeed), ("semichord", b), ("axis_offset", self.axis_offset)]).unwrap();
        w.connect([section.port("plunge"), aero.port("plunge")]);
        w.connect([section.port("pitch"), aero.port("pitch")]);
        let runtime = runtime(w, registry);
        let plunge = runtime.across_id(section.port("plunge"));
        let pitch = runtime.across_id(section.port("pitch"));
        Section { runtime, plunge, pitch }
    }

    /// Leading eigenvalue (real part, |imaginary|) of the compiled section at `airspeed`.
    pub fn leading_mode(&self, registry: &BehaviorRegistry, airspeed: f64) -> (f64, f64) {
        let section = self.model(registry, airspeed, 0.0);
        let island = &section.runtime.islands[0];
        let rest = vec![0.0; island.state.len()];
        let lin = linearise(&island.system, 0.0, &rest, &rest);
        leading_mode(&lin.eigenvalues())
    }

    pub fn flutter_speed(&self, registry: &BehaviorRegistry, maximum: f64) -> Option<f64> {
        let mut u = 1.0;
        while u < maximum && self.leading_mode(registry, u).0 < 0.0 {
            u += 0.5;
        }
        if u >= maximum {
            return None;
        }
        let (mut lo, mut hi) = (u - 0.5, u);
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            if self.leading_mode(registry, mid).0 < 0.0 { lo = mid } else { hi = mid }
        }
        Some(0.5 * (lo + hi))
    }
}

pub struct Outcome {
    pub growth_rate: f64,
    pub frequency: f64,
    pub time: Vec<f64>,
    pub plunge: Vec<f64>,
    pub pitch: Vec<f64>,
}

pub fn gust(section: WingSection, registry: &BehaviorRegistry, airspeed: f64, duration: f64) -> Outcome {
    let mut s = section.model(registry, airspeed, 0.01);
    let ids = [s.plunge, s.pitch];
    let trace = record(&mut s.runtime, duration, 2.0e-4, 2, &ids);
    let window = trace.after(0.5 * duration);
    let plunge_window = window.column(0);
    Outcome {
        growth_rate: envelope_rate(&window.time, &plunge_window).unwrap_or(0.0),
        frequency: period(&window.time, &plunge_window).map(|p| std::f64::consts::TAU / p).unwrap_or(0.0),
        time: trace.time.clone(),
        plunge: trace.column(0),
        pitch: trace.column(1),
    }
}

pub fn run() -> Report {
    let mut report = Report::new("flutter");
    let registry = registry();
    let section = WingSection::default();
    let Some(flutter_speed) = section.flutter_speed(&registry, 400.0) else {
        report.holds("linear analysis finds a flutter boundary", false);
        return report;
    };
    let (_, flutter_frequency) = section.leading_mode(&registry, flutter_speed);
    report.measure("flutter speed from eigenvalues (m/s)", flutter_speed).measure("flutter frequency (rad/s)", flutter_frequency);
    report.holds("flutter, not divergence: the unstable mode oscillates", flutter_frequency > 1.0);

    let below = gust(section, &registry, 0.9 * flutter_speed, 16.0);
    report.series("plunge at 0.9 U_F", &below.time, &below.plunge, 2000);
    report.below("0.9 U_F: disturbance decays", below.growth_rate, -0.05);
    let above = gust(section, &registry, 1.1 * flutter_speed, 16.0);
    report.series("plunge at 1.1 U_F", &above.time, &above.plunge, 2000);
    report.series("pitch at 1.1 U_F", &above.time, &above.pitch, 2000);
    report.above("1.1 U_F: disturbance grows", above.growth_rate, 0.02);
    report.within("1.1 U_F: oscillates at the flutter frequency", above.frequency, section.leading_mode(&registry, 1.1 * flutter_speed).1, 0.03);

    let (mut lo, mut hi) = (0.8 * flutter_speed, 1.2 * flutter_speed);
    for _ in 0..10 {
        let mid = 0.5 * (lo + hi);
        if gust(section, &registry, mid, 16.0).growth_rate < 0.0 { lo = mid } else { hi = mid }
    }
    report.within("simulated flutter speed", 0.5 * (lo + hi), flutter_speed, 0.01);
    report.within("growth rate at 1.1 U_F matches the eigenvalue", above.growth_rate, section.leading_mode(&registry, 1.1 * flutter_speed).0, 0.05);

    let locked = WingSection { pitch_locked: true, ..section };
    report.holds("pitch locked: no boundary up to 3 U_F", locked.flutter_speed(&registry, 3.0 * flutter_speed).is_none());
    let single = gust(locked, &registry, 3.0 * flutter_speed, 16.0);
    report.below("pitch locked at 3 U_F: decays", single.growth_rate, -0.1);
    report
}
