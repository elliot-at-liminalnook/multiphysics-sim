//! The surprise suite: emergent phenomena as acceptance tests.
//!
//! Each scenario in `surprise-tests.md` is one module that authors a
//! `ModelWorld` of registered behaviors, compiles it with `sim_compile`,
//! runs it through `sim_dynamics`, reads the result back through the
//! `StateStore`, and returns a [`Report`] asserting the published number
//! and the falsifier. The same model builders drive the live exhibits.

pub mod exhibit;
pub mod exhibits;
pub mod scenarios;
pub mod world;
pub mod experiment;

pub use sim_dynamics::report::{Check, Report};

pub type Scenario = fn() -> Report;

pub const ALL: &[(&str, Scenario)] = &[
    ("huygens-clocks", scenarios::huygens_clocks::run),
    ("flutter", scenarios::flutter::run),
    ("tippe-top", scenarios::tippe_top::run),
    ("passive-walker", scenarios::passive_walker::run),
    ("kapitza-pendulum", scenarios::kapitza_pendulum::run),
    ("dzhanibekov-flip", scenarios::dzhanibekov::run),
    ("euler-buckling", scenarios::euler_buckling::run),
    ("spring-pendulum", scenarios::spring_pendulum::run),
    ("current-hogging", scenarios::current_hogging::run),
    ("rijke-tube", scenarios::rijke_tube::run),
    ("water-hammer", scenarios::water_hammer::run),
    ("thermoelastic-damping", scenarios::thermoelastic_damping::run),
    ("stick-slip", scenarios::stick_slip::run),
    ("backlash-hunting", scenarios::backlash_hunting::run),
    ("sample-rate-instability", scenarios::sample_rate_instability::run),
    ("chua-circuit", scenarios::chua_circuit::run),
    ("painleve-rod", scenarios::painleve_rod::run),
    ("motor-hogging", scenarios::motor_hogging::run),
    ("levitron", scenarios::levitron::run),
    ("geyser", scenarios::geyser::run),
    ("semenov-ignition", scenarios::semenov_ignition::run),
    ("sky-cooling", scenarios::sky_cooling::run),
    ("viv-lock-in", scenarios::viv_lock_in::run),
    ("janssen-silo", scenarios::janssen_silo::run),
    ("stochastic-resonance", scenarios::stochastic_resonance::run),
    ("double-pendulum", scenarios::double_pendulum::run),
    ("language-independence", scenarios::language_independence::run),
    ("latency-instability", scenarios::latency_instability::run),
    ("quantisation-hunt", scenarios::quantisation_hunt::run),
    ("missed-deadlines", scenarios::missed_deadlines::run),
    ("leg-on-the-seam", scenarios::leg_seam::run),
    ("quadruped-gait", scenarios::quadruped_gait::run),
    ("scaling-ladder", scenarios::scaling_ladder::run),
    ("cruise-control", scenarios::cruise_control::run),
    ("walk-the-plank", scenarios::walk_the_plank::run),
];
