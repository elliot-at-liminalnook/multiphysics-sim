//! Learning notes for this crate's components (summary, how it works,
//! equations, trade-offs, limits, parameter help, typical values),
//! attached to the registry by `annotate`.
use sim_core::{BehaviorRegistry, ComponentNotes as Notes};

static ROBOT_BATTERY: Notes = Notes {
    category: "Power",
    explanation: "Open-circuit voltage follows state of charge, from about 0.9× nominal empty to 1.1× full with a knee near empty, and internal resistance drops it further under load: v = E(SoC) − R·i. SoC falls as charge is drawn (i/capacity). A 3-cell LiPo is 11.1 V nominal, 12.6 V full.",
    equations: &["v = E(SoC) − R_int·i", "E = V_nom·(0.9 + 0.2·SoC − 0.15·(1 − SoC)⁸)", "dSoC/dt = −i / (3600·capacity_Ah)"],
    tradeoffs: "More cells: more voltage (speed); more capacity: more runtime and weight; lower internal resistance (high C-rating) sags less under hard acceleration.",
    limits: "No temperature, ageing or recovery effects; heat from internal resistance is not routed to a port.",
    parameters: &[("cells", "Cells in series"), ("nominal_voltage", "Defaults to 3.7 V per cell"), ("internal_resistance", "Pack resistance"), ("capacity_ah", "Capacity in amp-hours"), ("initial_soc", "Charge at t = 0 (0…1)")],
    typical: &[("cells", 3.0)],
    pairs_with: &["robot.h_bridge", "actuator.pwm_driver", "part.bldc_motor", "sensor.voltage", "sensor.current"],
    active: true,
    ..Notes::new("A lithium battery pack: voltage sags with charge used and with current drawn.")
};

static ROBOT_H_BRIDGE: Notes = Notes {
    category: "Power",
    explanation: "Four switches let a single supply drive a motor both ways. Averaged over the PWM period, the output is command × supply voltage through the on-resistance of two switches; the supply sees the same power. A current limit folds the output voltage back when the motor current exceeds it.",
    equations: &["v_out ≈ command·v_supply − R_on·i", "i_supply = command·i_motor"],
    tradeoffs: "Averaged: fast to simulate, no ripple or switching heat. For those, build it from electrical.mosfet and control.h_bridge_pwm.",
    limits: "The output sets only the voltage between p and n, not where it sits: join the motor's n to the supply's 0 V (as a real bridge's low side would), or the solver cannot place the loop and reports a singular Jacobian at t = 0.",
    parameters: &[("on_resistance", "Resistance of the conducting switches"), ("current_limit", "Current at which the output folds back")],
    pairs_with: &["robot.battery", "bridge.brushed_motor", "part.pid_angle", "control.pi"],
    active: true,
    ..Notes::new("An H-bridge motor driver, averaged: command −1…1 sets the motor voltage and direction from the supply.")
};

static ROBOT_SWITCHABLE_H_BRIDGE: Notes = Notes {
    category: "Power",
    explanation: "Like robot.h_bridge, plus an enable input. Disabled, the switches open and the winding current decays through the body diodes back to the supply (regenerative braking), instead of the motor being shorted.",
    parameters: &[("on_resistance", "Conducting switch resistance"), ("diode_drop", "Freewheel diode drop"), ("diode_resistance", "Diode slope resistance")],
    pairs_with: &["robot.battery", "bridge.brushed_motor"],
    active: true,
    ..Notes::new("An averaged H-bridge that can be disabled: then the motor current freewheels through the switch diodes.")
};

static ROBOT_MOTOR_UNIT: Notes = Notes {
    category: "Actuators",
    explanation: "Everything a hobby servo or small gearmotor has inside: a DC winding (R, L, k_t, k_e, no-load loss), a rotor inertia, a gearbox of ratio N with efficiency η, Coulomb gear friction, backlash, and gear stiffness and damping at the output. Copper loss heats the winding port; resistance rises and torque constant falls with temperature. Signals report current, torque and speed.",
    equations: &["L·di/dt = v − R(T)·i − k_e·N·ω_out", "τ_out ≈ N·η·k_t(T)·i − friction − backlash/compliance effects", "heat = R(T)·i²"],
    tradeoffs: "Higher ratio: more torque, less speed, more friction and backlash, and harder to back-drive. A detailed unit makes realistic heating and stall; robot.effective_servo is the fast stand-in.",
    limits: "One lumped gear stage; efficiency constant with load; backlash as a dead zone.",
    parameters: &[("resistance", "Winding resistance"), ("torque_constant", "k_t (motor side)"), ("ratio", "Gear ratio N"), ("efficiency", "Gear efficiency η"), ("backlash", "Output free play"), ("gear_stiffness", "Output compliance"), ("gear_friction", "Output Coulomb friction"), ("rotor_inertia", "Rotor inertia (motor side)")],
    typical: &[("resistance", 3.7), ("torque_constant", 0.0097), ("ratio", 200.0), ("efficiency", 0.5)],
    pairs_with: &["robot.h_bridge", "robot.battery", "thermal.capacitance", "rotational.inertia", "part.pendulum_gravity", "sensor.encoder"],
    ..Notes::new("A complete gearmotor: winding, rotor, gearbox with efficiency, backlash and compliance, and winding heat.")
};

static ROBOT_EFFECTIVE_SERVO: Notes = Notes {
    category: "Actuators",
    explanation: "Instead of simulating winding, gears and firmware, it applies τ = k·(θ_target − θ) − c·ω, clipped by the motor envelope τ_max(ω) = τ_stall·(1 − ω/ω_0). Good for interactive or training runs where the servo’s inside does not matter.",
    equations: &["τ = clamp(k·(θ* − θ) − c·ω, envelope)", "envelope: τ_stall·(1 − |ω|/ω₀)"],
    limits: "No electrical, thermal, backlash or sampling effects.",
    parameters: &[("stiffness", "Position stiffness k"), ("damping", "Damping c"), ("stall_torque", "Stall torque"), ("no_load_speed", "No-load speed")],
    typical: &[("stiffness", 5.0), ("damping", 0.1), ("stall_torque", 1.5), ("no_load_speed", 6.0)],
    pairs_with: &["part.angle_setpoint", "rotational.inertia", "part.pendulum_gravity"],
    active: true,
    ..Notes::new("A fast position servo stand-in: spring-damper toward a target, limited by a speed–torque envelope.")
};

static ROBOT_SERVO_FIRMWARE: Notes = Notes {
    category: "Control",
    explanation: "Samples target and measured angle at a fixed rate, waits a latency, applies a dead band and encoder resolution, and outputs a held, saturated drive command: what makes real servos buzz, hunt or lag.",
    equations: &["every 1/rate: e = quantise(θ*) − quantise(θ); u = clamp(kp·e + ki·∫e + kd·ė)"],
    parameters: &[("rate", "Loop rate"), ("latency", "Delay before output"), ("deadband", "Error ignored"), ("resolution", "Sensor step"), ("kp", "Proportional gain"), ("kd", "Derivative gain")],
    pairs_with: &["robot.h_bridge", "robot.motor_unit", "sensor.encoder", "part.angle_setpoint"],
    ..Notes::new("A servo’s microcontroller loop: sampled PID with latency, dead band and sensor resolution.")
};

static ROBOT_THERMAL_PROBE: Notes = Notes {
    category: "Sensing",
    explanation: "Outputs the node temperature so a controller can derate or shut down a hot motor.",
    pairs_with: &["thermal.capacitance", "robot.motor_unit"],
    ..Notes::new("Reads a temperature as a signal (a thermistor or thermocouple, ideal).")
};

/// Attach the notes to every registered type this crate annotates.
pub fn annotate(registry: &mut BehaviorRegistry) {
    registry.annotate("robot.battery", &ROBOT_BATTERY);
    registry.annotate("robot.h_bridge", &ROBOT_H_BRIDGE);
    registry.annotate("robot.switchable_h_bridge", &ROBOT_SWITCHABLE_H_BRIDGE);
    registry.annotate("robot.motor_unit", &ROBOT_MOTOR_UNIT);
    registry.annotate("robot.effective_servo", &ROBOT_EFFECTIVE_SERVO);
    registry.annotate("robot.servo_firmware", &ROBOT_SERVO_FIRMWARE);
    registry.annotate("robot.thermal_probe", &ROBOT_THERMAL_PROBE);
}
