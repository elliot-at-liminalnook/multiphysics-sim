//! Learning notes for this crate's components (summary, how it works,
//! equations, trade-offs, limits, parameter help, typical values),
//! attached to the registry by `annotate`.
use sim_core::{BehaviorRegistry, ComponentNotes as Notes};

static BRIDGE_THERMISTOR: Notes = Notes {
    category: "Sensing",
    explanation: "R = R₀·exp(α·(T − T_ref)). With α < 0 (NTC) it is a temperature sensor or an inrush limiter; its own dissipation heats it (self-heating). As a fuse-like PTC (α > 0) it limits current when hot.",
    equations: &["R(T) = R₀·exp(α·(T − T_ref))", "heat = v²/R"],
    parameters: &[("resistance", "R₀ at T_ref"), ("coefficient", "α in 1/K (NTC ≈ −0.04)"), ("reference", "T_ref in K")],
    typical: &[("resistance", 10000.0), ("coefficient", -0.04), ("reference", 298.15)],
    pairs_with: &["thermal.capacitance", "electrical.resistor", "electrical.voltage_source"],
    ..Notes::new("A resistor whose resistance follows its own temperature (NTC or PTC).")
};

static BRIDGE_MOTOR: Notes = Notes {
    category: "Actuators",
    explanation: "The same physics as the brushed motor, packaged behind one composite connector so a drive and motor connect with one link; resistance follows R₀·exp(α(T − T_ref)).",
    parameters: &[("resistance", "R₀"), ("torque_constant", "k_t = k_e"), ("coefficient", "α"), ("reference", "T_ref")],
    typical: &[("resistance", 2.0), ("torque_constant", 0.012)],
    pairs_with: &["bridge.dual_drive"],
    ..Notes::new("A DC motor behind a single motor plug (winding, shaft and case, with temperature-dependent resistance).")
};

static BRIDGE_COIL: Notes = Notes {
    category: "Electrical",
    explanation: "N turns: magnetomotive force N·i drives flux, and changing flux induces N·dΦ/dt. Combine with reluctances, air gaps and magnets to build solenoids, relays and transformers from their magnetic circuits.",
    equations: &["mmf = N·i", "v = N·dΦ/dt"],
    parameters: &[("turns", "N")],
    typical: &[("turns", 200.0)],
    pairs_with: &["magnetic.reluctance", "bridge.air_gap", "magnetic.ground", "magnetic.saturable_core"],
    ..Notes::new("A winding linking an electric circuit to a magnetic circuit.")
};

static BRIDGE_AIR_GAP: Notes = Notes {
    category: "Actuators",
    explanation: "Reluctance (g + x)/(μ₀·A) grows with the armature position x, and the magnetic field pulls the armature to close the gap with force Φ²/(2μ₀A). Built from first principles; compare part.solenoid, the lumped version.",
    equations: &["R = (g + x)/(μ₀A)", "F = −Φ²/(2μ₀A)"],
    parameters: &[("area", "Pole face area"), ("gap", "Gap at x = 0")],
    typical: &[("area", 0.0001), ("gap", 0.001)],
    pairs_with: &["bridge.coil", "translational.mass", "translational.spring"],
    ..Notes::new("An air gap whose length is set by a moving armature: the heart of a relay or solenoid.")
};

/// Attach the notes to every registered type this crate annotates.
pub fn annotate(registry: &mut BehaviorRegistry) {
    registry.annotate("bridge.thermistor", &BRIDGE_THERMISTOR);
    registry.annotate("bridge.motor", &BRIDGE_MOTOR);
    registry.annotate("bridge.coil", &BRIDGE_COIL);
    registry.annotate("bridge.air_gap", &BRIDGE_AIR_GAP);
}
