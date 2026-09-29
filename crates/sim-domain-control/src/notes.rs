//! Learning notes for this crate's components (summary, how it works,
//! equations, trade-offs, limits, parameter help, typical values),
//! attached to the registry by `annotate`.
use sim_core::{BehaviorRegistry, ComponentNotes as Notes};

static CONTROL_CONSTANT: Notes = Notes {
    category: "Control",
    explanation: "Outputs a fixed dimensionless value: a steady command or enable.",
    parameters: &[("value", "The value")],
    typical: &[("value", 1.0)],
    pairs_with: &["robot.h_bridge", "part.brake", "control.pi"],
    ..Notes::new("A constant signal.")
};

static CONTROL_SINE: Notes = Notes {
    category: "Control",
    explanation: "amplitude·cos(2π·f·t + phase).",
    equations: &["y = A·cos(2πft + φ)"],
    parameters: &[("amplitude", "A"), ("frequency", "f in Hz"), ("phase", "φ in rad")],
    typical: &[("amplitude", 1.0), ("frequency", 1.0)],
    pairs_with: &["robot.h_bridge", "actuator.pwm_driver"],
    ..Notes::new("A sinusoid: for frequency sweeps and oscillating commands.")
};

static CONTROL_PULSE: Notes = Notes {
    category: "Control",
    explanation: "Zero, then `amplitude` from start to start + duration, then zero; the edges are scheduled events so the solver lands on them exactly.",
    parameters: &[("amplitude", "Level while on"), ("start", "On time"), ("duration", "Length")],
    typical: &[("amplitude", 1.0), ("start", 0.1), ("duration", 0.5)],
    pairs_with: &["robot.h_bridge", "part.brake"],
    ..Notes::new("A single pulse: on at `start` for `duration`, with exact edges.")
};

static CONTROL_PI: Notes = Notes {
    category: "Control",
    explanation: "u = kp·e + ki·∫e dt with e = setpoint − measured. The integral term removes steady error.",
    equations: &["u = kp·e + ki·∫e dt"],
    tradeoffs: "For angle and position loops with typed signals use part.pid_angle / part.pid_position.",
    parameters: &[("kp", "Proportional gain"), ("ki", "Integral gain"), ("setpoint", "Target of the measured signal")],
    typical: &[("kp", 1.0)],
    pairs_with: &["robot.h_bridge", "electrical.voltage_sense"],
    ..Notes::new("A continuous PI regulator on setpoint − measured (dimensionless signals).")
};

static CONTROL_SAMPLED_P: Notes = Notes {
    category: "Control",
    explanation: "Samples the error every period and holds the output in between: the zero-order hold of a microcontroller loop, which adds about half a period of delay.",
    parameters: &[("gain", "Proportional gain"), ("period", "Sample period"), ("limit", "Output limit"), ("setpoint", "Target")],
    typical: &[("gain", 1.0), ("period", 0.01)],
    pairs_with: &["robot.h_bridge"],
    ..Notes::new("A proportional controller that updates only every sample period (like firmware).")
};

static CONTROL_LAG_CHAIN: Notes = Notes {
    category: "Control",
    explanation: "`stages` first-order lags in series with total time constant = delay: an Erlang approximation of a pure delay (bus latency, filtering).",
    parameters: &[("delay", "Total delay"), ("stages", "Number of lags (more: closer to a pure delay)")],
    typical: &[("delay", 0.01)],
    pairs_with: &["control.pi"],
    ..Notes::new("A transport delay approximated by a chain of lags.")
};

static CONTROL_PWM: Notes = Notes {
    category: "Control",
    explanation: "Samples the duty at each period start and holds it, as a hardware compare register does; edges are scheduled events.",
    parameters: &[("frequency", "PWM frequency"), ("initial_duty", "Duty of the first period")],
    typical: &[("frequency", 20000.0)],
    pairs_with: &["electrical.mosfet"],
    ..Notes::new("A PWM timer: turns a duty cycle into a switching gate signal with exact edges.")
};

static CONTROL_H_BRIDGE_PWM: Notes = Notes {
    category: "Control",
    explanation: "Generates the four MOSFET gate signals of a full bridge at a fixed PWM frequency: the sign picks the conducting diagonal, the magnitude the duty; low-side switches rectify synchronously.",
    parameters: &[("frequency", "PWM frequency"), ("initial_duty", "Duty of the first period")],
    typical: &[("frequency", 20000.0)],
    pairs_with: &["electrical.mosfet", "bridge.brushed_motor"],
    ..Notes::new("An H-bridge gate driver: command −1…1 to four gate signals (sign-magnitude, synchronous).")
};

/// Attach the notes to every registered type this crate annotates.
pub fn annotate(registry: &mut BehaviorRegistry) {
    registry.annotate("control.constant", &CONTROL_CONSTANT);
    registry.annotate("control.sine", &CONTROL_SINE);
    registry.annotate("control.pulse", &CONTROL_PULSE);
    registry.annotate("control.pi", &CONTROL_PI);
    registry.annotate("control.sampled_p", &CONTROL_SAMPLED_P);
    registry.annotate("control.lag_chain", &CONTROL_LAG_CHAIN);
    registry.annotate("control.pwm", &CONTROL_PWM);
    registry.annotate("control.h_bridge_pwm", &CONTROL_H_BRIDGE_PWM);
}
