//! Learning notes for this crate's components (summary, how it works,
//! equations, trade-offs, limits, parameter help, typical values),
//! attached to the registry by `annotate`.
use sim_core::{BehaviorRegistry, ComponentNotes as Notes};

static MULTIBODY_RIGID_BODY: Notes = Notes {
    category: "Mechanical",
    explanation: "Newton–Euler: m·a = ΣF, I·ω̇ + ω×Iω = Στ, with orientation as a quaternion. Joints and contacts attach to its frame.",
    equations: &["m·a = ΣF + m·g", "I·ω̇ + ω × I·ω = Στ"],
    parameters: &[("mass", "kg"), ("ixx", "Principal inertia"), ("iyy", "Principal inertia"), ("izz", "Principal inertia"), ("gravity", "Gravity magnitude")],
    typical: &[("mass", 1.0), ("ixx", 0.01), ("iyy", 0.01), ("izz", 0.01)],
    pairs_with: &["multibody.sphere_contact"],
    ..Notes::new("A free rigid body in 3D: mass and inertia, falling under gravity.")
};

static CONTACT_WHEEL: Notes = Notes {
    category: "Mechanical",
    explanation: "The wheel presses into the ground through a stiff spring–damper; friction up to μ·N acts at the contact point, so it rolls when the axle torque is modest and slips when it is not. The axle is a rotational port for a motor.",
    equations: &["N = k·δ + c·δ̇ (δ > 0)", "|F_t| ≤ μ·N"],
    parameters: &[("radius", "Wheel radius"), ("inertia", "Wheel inertia"), ("stiffness", "Contact stiffness"), ("friction", "μ")],
    typical: &[("radius", 0.03), ("inertia", 0.0001)],
    pairs_with: &["joint.revolute", "bridge.brushed_motor"],
    ..Notes::new("A wheel rolling on the ground plane (planar): penalty contact with Coulomb friction and a driven axle.")
};

static JOINT_REVOLUTE: Notes = Notes {
    category: "Mechanical",
    explanation: "Keeps the anchor points on both bodies together while letting them rotate freely relative to each other; constraint drift is stabilised.",
    parameters: &[("ax", "Anchor on body a (x)"), ("bx", "Anchor on body b (x)"), ("stabilisation", "Drift correction rate")],
    pairs_with: &["joint.prismatic", "contact.wheel"],
    ..Notes::new("A hinge between two planar bodies (a pin joint).")
};

static JOINT_PRISMATIC: Notes = Notes {
    category: "Mechanical",
    explanation: "Keeps the bodies aligned and lets them translate along (ux, uy).",
    parameters: &[("ux", "Slide direction x"), ("uy", "Slide direction y")],
    pairs_with: &["joint.revolute"],
    ..Notes::new("A slider between two planar bodies: relative motion along one direction only.")
};

static JOINT_FIXED: Notes = Notes {
    category: "Mechanical",
    explanation: "Removes all relative motion (with drift stabilisation).",
    pairs_with: &["joint.revolute"],
    ..Notes::new("Welds two planar bodies together.")
};

/// Attach the notes to every registered type this crate annotates.
pub fn annotate(registry: &mut BehaviorRegistry) {
    registry.annotate("multibody.rigid_body", &MULTIBODY_RIGID_BODY);
    registry.annotate("contact.wheel", &CONTACT_WHEEL);
    registry.annotate("joint.revolute", &JOINT_REVOLUTE);
    registry.annotate("joint.prismatic", &JOINT_PRISMATIC);
    registry.annotate("joint.fixed", &JOINT_FIXED);
}
