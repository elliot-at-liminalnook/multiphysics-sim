//! Learning notes for this crate's components (summary, how it works,
//! equations, trade-offs, limits, parameter help, typical values),
//! attached to the registry by `annotate`.
use sim_core::{BehaviorRegistry, ComponentNotes as Notes};

static SENSOR_ENCODER: Notes = Notes {
    category: "Sensing",
    explanation: "An incremental or absolute encoder: angle = θ quantised to 2π/counts once `counts` and a `period` are set. Every sensor shares one measurement chain: a first-order bandwidth lag, a latency made of several lag stages, and a sampler that holds each reading for a period, quantises it, adds repeatable noise (seeded) and can inject faults (stuck, dropped). Leave them at zero for an ideal sensor.",
    equations: &["reading = quantise(θ, 2π/counts)"],
    tradeoffs: "More counts: finer position and smoother speed estimates; magnetic encoders are cheap and robust, optical ones finer.",
    limits: "Noise is deterministic per seed; faults follow the configured mode.",
    parameters: &[("bandwidth", "Low-pass bandwidth in Hz (0 = instant)"), ("latency", "Transport delay in s"), ("period", "Sample period in s (0 = continuous)"), ("quantum", "Resolution step"), ("noise", "Noise amplitude"), ("seed", "Noise seed (repeatable runs)"), ("counts", "Counts per revolution (quadrature: 4 × lines)")],
    pairs_with: &["rotational.inertia", "part.pid_angle", "bridge.brushed_motor"],
    ..Notes::new("Measures shaft angle, optionally with a finite number of counts per revolution.")
};

static SENSOR_TACHOMETER: Notes = Notes {
    category: "Sensing",
    explanation: "Reads angular velocity (a tachogenerator, or speed differentiated from an encoder). Every sensor shares one measurement chain: a first-order bandwidth lag, a latency made of several lag stages, and a sampler that holds each reading for a period, quantises it, adds repeatable noise (seeded) and can inject faults (stuck, dropped). Leave them at zero for an ideal sensor.",
    limits: "Noise is deterministic per seed; faults follow the configured mode.",
    parameters: &[("bandwidth", "Low-pass bandwidth in Hz (0 = instant)"), ("latency", "Transport delay in s"), ("period", "Sample period in s (0 = continuous)"), ("quantum", "Resolution step"), ("noise", "Noise amplitude"), ("seed", "Noise seed (repeatable runs)")],
    pairs_with: &["rotational.inertia", "control.pi"],
    ..Notes::new("Measures shaft speed.")
};

static SENSOR_LINEAR_ENCODER: Notes = Notes {
    category: "Sensing",
    explanation: "Reads the axis position through the measurement chain. Every sensor shares one measurement chain: a first-order bandwidth lag, a latency made of several lag stages, and a sampler that holds each reading for a period, quantises it, adds repeatable noise (seeded) and can inject faults (stuck, dropped). Leave them at zero for an ideal sensor.",
    limits: "Noise is deterministic per seed; faults follow the configured mode.",
    parameters: &[("bandwidth", "Low-pass bandwidth in Hz (0 = instant)"), ("latency", "Transport delay in s"), ("period", "Sample period in s (0 = continuous)"), ("quantum", "Resolution step"), ("noise", "Noise amplitude"), ("seed", "Noise seed (repeatable runs)")],
    pairs_with: &["translational.mass", "part.pid_position", "part.timing_belt"],
    ..Notes::new("Measures linear position (a glass scale, a magnetic strip, a potentiometer).")
};

static SENSOR_LINEAR_VELOCITY: Notes = Notes {
    category: "Sensing",
    explanation: "Reads the axis velocity through the measurement chain. Every sensor shares one measurement chain: a first-order bandwidth lag, a latency made of several lag stages, and a sampler that holds each reading for a period, quantises it, adds repeatable noise (seeded) and can inject faults (stuck, dropped). Leave them at zero for an ideal sensor.",
    limits: "Noise is deterministic per seed; faults follow the configured mode.",
    parameters: &[("bandwidth", "Low-pass bandwidth in Hz (0 = instant)"), ("latency", "Transport delay in s"), ("period", "Sample period in s (0 = continuous)"), ("quantum", "Resolution step"), ("noise", "Noise amplitude"), ("seed", "Noise seed (repeatable runs)")],
    pairs_with: &["translational.mass"],
    ..Notes::new("Measures linear velocity.")
};

static SENSOR_CURRENT: Notes = Notes {
    category: "Sensing",
    explanation: "Placed in series, it reads the current from p to n. Motor current is a direct estimate of torque (τ = k_t·i). Every sensor shares one measurement chain: a first-order bandwidth lag, a latency made of several lag stages, and a sampler that holds each reading for a period, quantises it, adds repeatable noise (seeded) and can inject faults (stuck, dropped). Leave them at zero for an ideal sensor.",
    equations: &["reading = i"],
    limits: "Noise is deterministic per seed; faults follow the configured mode.",
    parameters: &[("bandwidth", "Low-pass bandwidth in Hz (0 = instant)"), ("latency", "Transport delay in s"), ("period", "Sample period in s (0 = continuous)"), ("quantum", "Resolution step"), ("noise", "Noise amplitude"), ("seed", "Noise seed (repeatable runs)")],
    pairs_with: &["bridge.brushed_motor", "robot.h_bridge", "part.bldc_motor"],
    ..Notes::new("Measures current through it (a shunt or hall sensor).")
};

static SENSOR_VOLTAGE: Notes = Notes {
    category: "Sensing",
    explanation: "Reads v_p − v_n without drawing current. Every sensor shares one measurement chain: a first-order bandwidth lag, a latency made of several lag stages, and a sampler that holds each reading for a period, quantises it, adds repeatable noise (seeded) and can inject faults (stuck, dropped). Leave them at zero for an ideal sensor.",
    limits: "Noise is deterministic per seed; faults follow the configured mode.",
    parameters: &[("bandwidth", "Low-pass bandwidth in Hz (0 = instant)"), ("latency", "Transport delay in s"), ("period", "Sample period in s (0 = continuous)"), ("quantum", "Resolution step"), ("noise", "Noise amplitude"), ("seed", "Noise seed (repeatable runs)")],
    pairs_with: &["robot.battery", "electrical.capacitor"],
    ..Notes::new("Measures a voltage.")
};

static SENSOR_FORCE: Notes = Notes {
    category: "Sensing",
    explanation: "Placed between two points of a mechanism, it passes the force through and reports it (strain-gauge load cells are very stiff; add a spring if their compliance matters). Every sensor shares one measurement chain: a first-order bandwidth lag, a latency made of several lag stages, and a sampler that holds each reading for a period, quantises it, adds repeatable noise (seeded) and can inject faults (stuck, dropped). Leave them at zero for an ideal sensor.",
    equations: &["reading = F"],
    limits: "Noise is deterministic per seed; faults follow the configured mode.",
    parameters: &[("bandwidth", "Low-pass bandwidth in Hz (0 = instant)"), ("latency", "Transport delay in s"), ("period", "Sample period in s (0 = continuous)"), ("quantum", "Resolution step"), ("noise", "Noise amplitude"), ("seed", "Noise seed (repeatable runs)")],
    pairs_with: &["translational.mass", "part.voice_coil", "bridge.lead_screw"],
    ..Notes::new("A load cell: measures the force passing through it.")
};

static SENSOR_IMU: Notes = Notes {
    category: "Sensing",
    explanation: "Reports specific force (acceleration minus gravity) in the body frame and the body rate, each through the measurement chain with its own bias, noise and resolution. A body at rest reads +g upwards; in free fall it reads zero.",
    equations: &["a_meas = Rᵀ·(a − g) + bias", "ω_meas = ω + bias"],
    limits: "Planar (2D) bodies only.",
    parameters: &[("gravity", "Gravity magnitude"), ("bias.ax", "Accelerometer x bias"), ("noise.gyro", "Gyro noise")],
    pairs_with: &["multibody.rigid_body"],
    ..Notes::new("A planar inertial unit: accelerometer and gyro on a moving body.")
};

static ACTUATOR_PWM_DRIVER: Notes = Notes {
    category: "Power",
    explanation: "Averages a switching driver over its PWM period: output voltage = duty·V_supply through a small resistance, with an optional dead band where small commands do nothing (like a real driver’s minimum pulse).",
    equations: &["v_out = V·duty (|duty| > dead band)"],
    parameters: &[("supply", "Supply voltage"), ("resistance", "Output resistance"), ("dead_band", "Duty below which the output is zero")],
    typical: &[("supply", 12.0)],
    pairs_with: &["bridge.brushed_motor", "part.pid_angle", "control.pi"],
    active: true,
    ..Notes::new("A PWM motor driver averaged: duty −1…1 becomes ±duty × supply volts.")
};

static ACTUATOR_SERVO: Notes = Notes {
    category: "Actuators",
    explanation: "Stands in for a motor plus current-controlled driver when you care about the mechanism, not the electronics: torque follows the command through a first-order lag with bandwidth f_c, clipped to torque and current limits; current = τ/k_t is reported.",
    equations: &["dτ/dt = 2π f_c·(τ_cmd − τ)", "|τ| ≤ τ_max, current = τ/k_t"],
    parameters: &[("bandwidth", "Torque loop bandwidth in Hz"), ("torque_limit", "Maximum torque"), ("torque_constant", "For the current output"), ("current_limit", "Maximum current")],
    typical: &[("bandwidth", 100.0)],
    pairs_with: &["rotational.inertia", "part.pid_angle"],
    active: true,
    ..Notes::new("An ideal torque-controlled servo: follows a torque command within a bandwidth and limits.")
};

static ACTUATOR_QUANTISER: Notes = Notes {
    category: "Control",
    explanation: "output = step·round(input/step), clipped to ±limit.",
    equations: &["y = step·round(u/step)"],
    parameters: &[("step", "Step size"), ("limit", "Output limit")],
    typical: &[("step", 0.01)],
    pairs_with: &["control.pi"],
    ..Notes::new("Rounds a signal to steps (a DAC or a coarse command).")
};

/// Attach the notes to every registered type this crate annotates.
pub fn annotate(registry: &mut BehaviorRegistry) {
    registry.annotate("sensor.encoder", &SENSOR_ENCODER);
    registry.annotate("sensor.tachometer", &SENSOR_TACHOMETER);
    registry.annotate("sensor.linear_encoder", &SENSOR_LINEAR_ENCODER);
    registry.annotate("sensor.linear_velocity", &SENSOR_LINEAR_VELOCITY);
    registry.annotate("sensor.current", &SENSOR_CURRENT);
    registry.annotate("sensor.voltage", &SENSOR_VOLTAGE);
    registry.annotate("sensor.force", &SENSOR_FORCE);
    registry.annotate("sensor.imu", &SENSOR_IMU);
    registry.annotate("actuator.pwm_driver", &ACTUATOR_PWM_DRIVER);
    registry.annotate("actuator.servo", &ACTUATOR_SERVO);
    registry.annotate("actuator.quantiser", &ACTUATOR_QUANTISER);
}
