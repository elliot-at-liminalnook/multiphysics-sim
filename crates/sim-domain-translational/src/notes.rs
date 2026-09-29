//! Learning notes for this crate's components (summary, how it works,
//! equations, trade-offs, limits, parameter help, typical values),
//! attached to the registry by `annotate`.
use sim_core::{BehaviorRegistry, ComponentNotes as Notes};

static TRANSLATIONAL_SPRING: Notes = Notes {
    category: "Mechanical",
    explanation: "F = k·(x_a − x_b − rest). Stores ½·k·δ². With a mass it rings at ω = √(k/m); robots use springs for compliance, energy storage (jumping) and series-elastic actuators that measure force by measuring stretch.",
    equations: &["F = k·(x_a − x_b − rest)", "energy = ½·k·δ²"],
    tradeoffs: "Stiffer: more precise and faster, less protection from shocks. Softer: safer, more compliant, lower bandwidth.",
    limits: "Linear, massless, no hysteresis.",
    parameters: &[("stiffness", "k in N/m"), ("rest", "Unstretched offset")],
    typical: &[("stiffness", 1000.0)],
    pairs_with: &["translational.mass", "translational.damper", "translational.ground", "part.voice_coil", "part.solenoid"],
    ..Notes::new("A linear spring: force proportional to stretch.")
};

static TRANSLATIONAL_DAMPER: Notes = Notes {
    category: "Mechanical",
    explanation: "F = c·(v_a − v_b): shock absorbers, oil films, air drag at low speed. Turns motion into heat.",
    equations: &["F = c·(v_a − v_b)"],
    tradeoffs: "Critical damping c = 2√(k·m) settles a spring–mass fastest without overshoot.",
    limits: "Linear; no Coulomb friction (see part.hard_stop_linear for contact, rotational.coulomb_friction for dry friction).",
    parameters: &[("damping", "c in N·s/m")],
    typical: &[("damping", 10.0)],
    pairs_with: &["translational.mass", "translational.spring", "translational.ground"],
    ..Notes::new("Viscous drag between two points: force proportional to relative speed.")
};

static TRANSLATIONAL_POSITION_SENSOR: Notes = Notes {
    category: "Sensing",
    explanation: "Outputs the axis position as a signal. For resolution, noise and delay use sensor.linear_encoder.",
    pairs_with: &["translational.mass", "part.pid_position"],
    ..Notes::new("Reads a position exactly (an ideal linear encoder).")
};

static TRANSLATIONAL_BELT_FRICTION: Notes = Notes {
    category: "Mechanical",
    explanation: "Friction between a body and a belt moving at a set speed follows a Stribeck curve: static μ_s at rest relative to the belt, falling toward kinetic μ_k as sliding speed passes v_s. That falling curve feeds energy into oscillations: a spring-held block sticks, slips, and sticks again (squeal, chatter, jerky slides).",
    equations: &["F = N·(μ_k + (μ_s − μ_k)·exp(−|v_rel|/v_s))·sign(v_rel)"],
    parameters: &[("normal_force", "N pressing the block on the belt"), ("static_friction", "μ_s"), ("kinetic_friction", "μ_k"), ("stribeck_velocity", "v_s"), ("belt_speed", "Belt speed"), ("regularisation", "Smoothing speed of the sign")],
    typical: &[("normal_force", 10.0), ("static_friction", 0.5), ("kinetic_friction", 0.3), ("stribeck_velocity", 0.01), ("belt_speed", 0.1)],
    pairs_with: &["translational.mass", "translational.spring"],
    active: true,
    ..Notes::new("A block on a moving belt with Stribeck friction: the classic stick–slip demonstrator.")
};

/// Attach the notes to every registered type this crate annotates.
pub fn annotate(registry: &mut BehaviorRegistry) {
    registry.annotate("translational.spring", &TRANSLATIONAL_SPRING);
    registry.annotate("translational.damper", &TRANSLATIONAL_DAMPER);
    registry.annotate("translational.position_sensor", &TRANSLATIONAL_POSITION_SENSOR);
    registry.annotate("translational.belt_friction", &TRANSLATIONAL_BELT_FRICTION);
}
