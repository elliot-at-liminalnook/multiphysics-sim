//! Learning notes for this crate's components (summary, how it works,
//! equations, trade-offs, limits, parameter help, typical values),
//! attached to the registry by `annotate`.
use sim_core::{BehaviorRegistry, ComponentNotes as Notes};

static ELECTRICAL_CAPACITOR: Notes = Notes {
    category: "Electrical",
    explanation: "Current flows only while the voltage changes: i = C·dv/dt. It stores energy ½·C·v² and gives it back, so it smooths supply rails (decoupling), filters ripple and sets RC time constants with a resistor (τ = R·C).",
    equations: &["i = C·d(v_p − v_n)/dt", "energy = ½·C·v²"],
    tradeoffs: "Electrolytics: big capacitance, cheap, but series resistance and limited ripple current. Ceramics: tiny, fast, low loss, small values. Bigger bulk capacitance stiffens a motor supply against current spikes.",
    limits: "Ideal: no series resistance (ESR), leakage or voltage rating.",
    parameters: &[("capacitance", "C, farads (100 µF = 0.0001 F)")],
    typical: &[("capacitance", 0.0001)],
    pairs_with: &["electrical.resistor", "electrical.ground", "electrical.voltage_source", "electrical.inductor"],
    ..Notes::new("Stores charge: holds a voltage steady against fast changes.")
};

static ELECTRICAL_INDUCTOR: Notes = Notes {
    category: "Electrical",
    explanation: "Voltage appears only while the current changes: v = L·di/dt. The current cannot jump, which is why a switch opening on an inductor (a motor winding, a relay coil) produces a voltage spike unless a flyback diode gives the current a path. With a switch and a diode it makes a buck converter.",
    equations: &["v_p − v_n = L·di/dt", "energy = ½·L·i²"],
    tradeoffs: "More inductance smooths current ripple in converters and motor drives but makes current slower to change (L/R time constant) and the part bigger.",
    limits: "Ideal: no winding resistance (add a resistor), saturation or core loss.",
    parameters: &[("inductance", "L, henries (47 µH = 4.7e-5 H)")],
    typical: &[("inductance", 0.001)],
    pairs_with: &["electrical.capacitor", "electrical.diode", "electrical.resistor", "electrical.mosfet"],
    ..Notes::new("Stores energy in a magnetic field: resists changes in current.")
};

static ELECTRICAL_DIODE: Notes = Notes {
    category: "Electrical",
    explanation: "Forward-biased above about V_f the diode conducts with a small on-resistance; reversed it passes only leakage. Robots use them for flyback protection across motors and relays, reverse-polarity protection and rectification. The knee is smoothed (softplus) over a small width so solvers can step across it.",
    equations: &["i = (w/R_on)·ln(1 + exp((v − V_f)/w)) + G_off·v", "junction capacitance keeps a reversed node defined"],
    tradeoffs: "Schottky diodes drop less (≈ 0.3–0.45 V) and switch faster but leak more; silicon ≈ 0.7 V. Every volt dropped at current I wastes V·I as heat.",
    limits: "Piecewise-linear smoothed knee, not a Shockley exponential; no reverse-recovery or breakdown; no heat port.",
    parameters: &[("forward_voltage", "Knee voltage V_f"), ("on_resistance", "Slope resistance above the knee"), ("off_conductance", "Reverse leakage conductance"), ("smoothing", "Knee width w"), ("junction_capacitance", "Keeps a reverse-biased node well posed")],
    pairs_with: &["electrical.inductor", "bridge.brushed_motor", "electrical.mosfet", "electrical.capacitor"],
    ..Notes::new("A one-way valve for current: conducts forward above its knee voltage, blocks backwards.")
};

static ELECTRICAL_MOSFET: Notes = Notes {
    category: "Electrical",
    explanation: "An N-channel MOSFET conducts drain to source through R_on when its gate command is ≥ 0.5 and blocks otherwise; its body diode always conducts source to drain. R_on rises with temperature, and every watt it dissipates (v·i) flows out of the heat port, so switches warm their heatsink. Four of them make an H-bridge that drives a motor both ways.",
    equations: &["on: i = v_ds / R_on(T),  R_on(T) = R_on·(1 + α·(T − T_ref))", "off: leakage + body diode + output capacitance", "heat out = v_ds·i_d"],
    tradeoffs: "Lower R_on means less conduction loss but more gate charge (slower, costlier switching). Heat must go somewhere: a TO-220 without a heatsink manages about 1–2 W.",
    limits: "Conduction losses only (switching transitions are instant, no switching loss); gate is a logic signal, not a gate-charge model.",
    parameters: &[("on_resistance", "Channel resistance at the reference temperature"), ("output_capacitance", "Drain–source capacitance"), ("off_conductance", "Off-state leakage"), ("temperature_coefficient", "Rise of R_on per kelvin"), ("reference", "Temperature R_on is given at")],
    pairs_with: &["control.pwm", "control.h_bridge_pwm", "electrical.diode", "thermal.capacitance", "thermal.conductance"],
    ..Notes::new("An electronic switch: the gate signal turns a low-resistance channel on or off; losses heat it.")
};

static ELECTRICAL_CURRENT_SOURCE: Notes = Notes {
    category: "Electrical",
    explanation: "An ideal current source: models current-controlled drivers (LED drivers, stepper drivers in current mode) or a known load current.",
    equations: &["i = I₀"],
    limits: "Ideal: unlimited voltage compliance.",
    parameters: &[("current", "Current pushed out of p through the external circuit")],
    typical: &[("current", 0.1)],
    pairs_with: &["electrical.resistor", "electrical.ground"],
    active: true,
    ..Notes::new("Pushes a fixed current, whatever voltage that takes.")
};

static ELECTRICAL_CONTROLLED_VOLTAGE_SOURCE: Notes = Notes {
    category: "Electrical",
    explanation: "Holds v_p − v_n equal to its voltage input. Use it where a controller commands a voltage directly (an idealised motor driver) or to replay a recorded waveform.",
    equations: &["v_p − v_n = u(t)"],
    limits: "Ideal: no current limit, no internal resistance.",
    pairs_with: &["bridge.brushed_motor", "part.bldc_motor", "part.voice_coil", "electrical.ground"],
    active: true,
    ..Notes::new("A voltage source set by a signal: an ideal amplifier or a motor driver seen from the motor.")
};

static ELECTRICAL_VOLTAGE_SENSE: Notes = Notes {
    category: "Sensing",
    explanation: "An ideal voltmeter feeding a controller: reading = (v_p − v_n)/scale. Draws no current.",
    equations: &["reading = (v_p − v_n) / scale"],
    parameters: &[("scale", "Volts per unit of reading")],
    pairs_with: &["control.pi", "electrical.capacitor"],
    ..Notes::new("Reads a voltage as a signal (divided by a scale).")
};

/// Attach the notes to every registered type this crate annotates.
pub fn annotate(registry: &mut BehaviorRegistry) {
    registry.annotate("electrical.capacitor", &ELECTRICAL_CAPACITOR);
    registry.annotate("electrical.inductor", &ELECTRICAL_INDUCTOR);
    registry.annotate("electrical.diode", &ELECTRICAL_DIODE);
    registry.annotate("electrical.mosfet", &ELECTRICAL_MOSFET);
    registry.annotate("electrical.current_source", &ELECTRICAL_CURRENT_SOURCE);
    registry.annotate("electrical.controlled_voltage_source", &ELECTRICAL_CONTROLLED_VOLTAGE_SOURCE);
    registry.annotate("electrical.voltage_sense", &ELECTRICAL_VOLTAGE_SENSE);
}
