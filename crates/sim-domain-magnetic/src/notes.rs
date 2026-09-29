//! Learning notes for this crate's components (summary, how it works,
//! equations, trade-offs, limits, parameter help, typical values),
//! attached to the registry by `annotate`.
use sim_core::{BehaviorRegistry, ComponentNotes as Notes};

static MAGNETIC_GROUND: Notes = Notes {
    category: "Electrical",
    explanation: "Every magnetic circuit needs one reference node, like electrical ground.",
    pairs_with: &["magnetic.reluctance", "bridge.coil"],
    ..Notes::new("The magnetic circuit reference (zero magnetomotive force).")
};

static MAGNETIC_RELUCTANCE: Notes = Notes {
    category: "Electrical",
    explanation: "R = l/(μ₀μᵣA): iron (μᵣ in the thousands) has little reluctance, air a lot, so the air gaps dominate a magnetic circuit. It stores ½·R·Φ².",
    equations: &["mmf_a − mmf_b = R·Φ"],
    parameters: &[("reluctance", "R directly, or"), ("length", "path length l"), ("area", "cross-section A"), ("relative_permeability", "μᵣ")],
    pairs_with: &["bridge.coil", "magnetic.permanent_magnet"],
    ..Notes::new("A piece of a magnetic path: resists flux like a resistor resists current.")
};

static MAGNETIC_SATURABLE_CORE: Notes = Notes {
    category: "Electrical",
    explanation: "B = B_s·(2/π)·atan(π·μ₀μᵣ·H/(2B_s)): linear at low field, flat near saturation, so inductance collapses at high current.",
    equations: &["B = B_s·(2/π)·atan(π·μ₀μᵣ·H/(2B_s))"],
    parameters: &[("length", "Path length"), ("area", "Cross-section"), ("saturation", "B_s in tesla (steel ≈ 1.5–2)"), ("relative_permeability", "μᵣ")],
    typical: &[("length", 0.1), ("area", 0.0001), ("saturation", 1.5)],
    pairs_with: &["bridge.coil"],
    ..Notes::new("An iron core that saturates: flux stops growing once the iron is full.")
};

static MAGNETIC_PERMANENT_MAGNET: Notes = Notes {
    category: "Electrical",
    explanation: "A coercive mmf behind an internal reluctance, like a battery with internal resistance: mmf_a − mmf_b = R_i·Φ − F_c.",
    equations: &["mmf_a − mmf_b = R_i·Φ − F_c"],
    parameters: &[("coercive_mmf", "F_c"), ("reluctance", "R_i")],
    typical: &[("coercive_mmf", 1000.0), ("reluctance", 1000000.0)],
    pairs_with: &["magnetic.reluctance", "bridge.air_gap"],
    active: true,
    ..Notes::new("A permanent magnet in a magnetic circuit: a source of magnetomotive force.")
};

/// Attach the notes to every registered type this crate annotates.
pub fn annotate(registry: &mut BehaviorRegistry) {
    registry.annotate("magnetic.ground", &MAGNETIC_GROUND);
    registry.annotate("magnetic.reluctance", &MAGNETIC_RELUCTANCE);
    registry.annotate("magnetic.saturable_core", &MAGNETIC_SATURABLE_CORE);
    registry.annotate("magnetic.permanent_magnet", &MAGNETIC_PERMANENT_MAGNET);
}
