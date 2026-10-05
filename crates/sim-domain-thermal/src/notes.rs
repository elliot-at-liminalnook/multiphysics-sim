//! Learning notes for this crate's components (summary, how it works,
//! equations, trade-offs, limits, parameter help, typical values),
//! attached to the registry by `annotate`.
use sim_core::{BehaviorRegistry, ComponentNotes as Notes};

static THERMAL_CAPACITANCE: Notes = Notes {
    category: "Thermal",
    explanation: "Heat capacity C = mass × specific heat (aluminium ≈ 900 J/(kg·K), copper ≈ 385). C·dT/dt equals the net heat flowing in. A winding, a heatsink or a battery each warm with their own time constant τ = C·R_th with the path to ambient.",
    equations: &["C·dT/dt = Σ heat in"],
    tradeoffs: "More thermal mass rides out short overloads (a motor can briefly exceed its continuous rating) but cools slowly.",
    limits: "One uniform temperature (no gradients inside).",
    parameters: &[("heat_capacity", "C in J/K (a 50 g copper winding ≈ 19 J/K)")],
    typical: &[("heat_capacity", 20.0)],
    pairs_with: &["thermal.conductance", "thermal.ambient", "bridge.thermistor", "electrical.mosfet", "part.bldc_motor", "robot.motor_unit"],
    ..Notes::new("A lump of material that stores heat: its temperature rises as heat flows in.")
};

static THERMAL_CONDUCTANCE: Notes = Notes {
    category: "Thermal",
    explanation: "Heat flow is proportional to the temperature difference: Q = G·(T_a − T_b), or with thermal resistance R_th = 1/G. Datasheets quote junction-to-case and heatsink-to-air resistances in K/W; a fan lowers the heatsink-to-air value.",
    equations: &["Q = G·(T_a − T_b) = (T_a − T_b)/R_th"],
    tradeoffs: "Lower resistance keeps parts cooler but costs size, weight or fan noise.",
    limits: "Linear (no radiation T⁴ law, no convection that varies with temperature).",
    parameters: &[("conductance", "G in W/K"), ("resistance", "Alternatively R_th in K/W")],
    typical: &[("conductance", 0.5)],
    pairs_with: &["thermal.capacitance", "thermal.ambient"],
    ..Notes::new("A path heat flows along: conduction through metal, convection to air.")
};

static THERMAL_AMBIENT: Notes = Notes {
    category: "Thermal",
    explanation: "Holds its node at a set temperature and absorbs whatever heat arrives. Connect it through a conductance to what it cools.",
    equations: &["T = T_ambient"],
    limits: "Wire a heat source (a motor winding) through a thermal mass or conductance: a large heat port wired straight to the ambient can fail to start the solver.",
    parameters: &[("temperature", "Kelvin (20 °C = 293.15 K)")],
    typical: &[("temperature", 293.15)],
    pairs_with: &["thermal.conductance"],
    ..Notes::new("The surroundings at a fixed temperature: an infinite sink for heat.")
};

static THERMAL_HEAT_SOURCE: Notes = Notes {
    category: "Thermal",
    explanation: "Adds P watts to its node: electronics you are not modelling in detail, the sun on a panel.",
    equations: &["heat in = P"],
    parameters: &[("power", "Heating power in W")],
    pairs_with: &["thermal.capacitance"],
    active: true,
    ..Notes::new("Injects a constant heating power.")
};

static THERMAL_CONTROLLED_HEAT_SOURCE: Notes = Notes {
    category: "Thermal",
    explanation: "Adds the commanded power to its node: a heater driven by a controller (a thermostat block, a PWM stage you are not modelling in detail).",
    equations: &["heat in = power (signal, W)"],
    pairs_with: &["thermal.capacitance", "thermal.temperature_sensor"],
    active: true,
    ..Notes::new("Injects the heating power its input signal commands.")
};

static THERMAL_TEMPERATURE_SENSOR: Notes = Notes {
    category: "Sensing",
    explanation: "Outputs the node temperature so a controller can regulate it, derate or shut down a hot motor.",
    equations: &["temperature = T_node"],
    pairs_with: &["thermal.capacitance", "thermal.controlled_heat_source", "robot.motor_unit"],
    ..Notes::new("Reads a temperature as a signal (a thermistor or thermocouple, ideal).")
};

/// Attach the notes to every registered type this crate annotates.
pub fn annotate(registry: &mut BehaviorRegistry) {
    registry.annotate("thermal.capacitance", &THERMAL_CAPACITANCE);
    registry.annotate("thermal.conductance", &THERMAL_CONDUCTANCE);
    registry.annotate("thermal.ambient", &THERMAL_AMBIENT);
    registry.annotate("thermal.heat_source", &THERMAL_HEAT_SOURCE);
    registry.annotate("thermal.controlled_heat_source", &THERMAL_CONTROLLED_HEAT_SOURCE);
    registry.annotate("thermal.temperature_sensor", &THERMAL_TEMPERATURE_SENSOR);
}
