//! Instantaneous voltage/current/torque feasibility for an averaged DC drive.
//! The actual sampled controller remains external. This screen never advances
//! a plant, clamps away overload, or replaces the existing MotorUnit reductions.
use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorRegistry, QuantityKind as Q,
    primitive::{Descriptor, Field as F},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub resistance_ohm: f64,
    pub resistance_temperature_coefficient_per_k: f64,
    pub reference_temperature_k: f64,
    /// A single SI constant preserves electrical/mechanical conversion power.
    pub motor_constant_nm_a: f64,
    pub gear_ratio: f64,
    pub gear_efficiency: f64,
    pub current_limit_a: f64,
    pub viscous_friction_nm_s_rad: f64,
    pub coulomb_friction_nm: f64,
    pub friction_speed_rad_s: f64,
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        let positive = [
            self.resistance_ohm,
            self.reference_temperature_k,
            self.motor_constant_nm_a,
            self.gear_ratio,
            self.gear_efficiency,
            self.current_limit_a,
            self.friction_speed_rad_s,
        ];
        let nonnegative = [
            self.resistance_temperature_coefficient_per_k,
            self.viscous_friction_nm_s_rad,
            self.coulomb_friction_nm,
        ];
        if positive.iter().any(|v| !v.is_finite() || *v <= 0.)
            || self.gear_efficiency > 1.
            || nonnegative.iter().any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("invalid explicitly supplied DC envelope parameters".into());
        }
        Ok(())
    }
    fn resistance(&self, temperature: f64) -> Result<f64, String> {
        self.validate()?;
        let r = self.resistance_ohm
            * (1.
                + self.resistance_temperature_coefficient_per_k
                    * (temperature - self.reference_temperature_k));
        if !temperature.is_finite() || temperature <= 0. || !r.is_finite() || r <= 0. {
            return Err("invalid winding temperature/resistance".into());
        }
        Ok(r)
    }
    fn torque(&self, current: f64, speed: f64) -> f64 {
        let rotor_torque = self.motor_constant_nm_a * current;
        let efficiency = if rotor_torque * speed >= 0. {
            self.gear_efficiency
        } else {
            1. / self.gear_efficiency
        };
        self.gear_ratio * efficiency * rotor_torque
            - self.viscous_friction_nm_s_rad * speed
            - self.coulomb_friction_nm * (speed / self.friction_speed_rad_s).tanh()
    }
    pub fn evaluate(&self, input: Input) -> Result<Report, String> {
        let r = self.resistance(input.temperature_k)?;
        if !input.supply_voltage_v.is_finite()
            || input.supply_voltage_v <= 0.
            || !input.output_speed_rad_s.is_finite()
            || !input.duty.is_finite()
            || input.duty.abs() > 1.
        {
            return Err("finite speed, positive supply and duty in [-1,1] required".into());
        }
        let speed = input.output_speed_rad_s;
        let emf = self.motor_constant_nm_a * self.gear_ratio * speed;
        let voltage = input.duty * input.supply_voltage_v;
        let current = (voltage - emf) / r;
        let lower = ((-input.supply_voltage_v - emf) / r).max(-self.current_limit_a);
        let upper = ((input.supply_voltage_v - emf) / r).min(self.current_limit_a);
        let torque = self.torque(current, speed);
        let electrical = voltage * current;
        let mechanical = torque * speed;
        let heat = electrical - mechanical;
        let interval =
            (lower <= upper).then(|| [self.torque(lower, speed), self.torque(upper, speed)]);
        if [
            emf, current, torque, electrical, mechanical, heat, lower, upper,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || interval
                .as_ref()
                .is_some_and(|r| r.iter().any(|v| !v.is_finite()))
            || heat < -1e-9 * (1. + electrical.abs())
        {
            return Err("invalid/overflowing power or torque envelope".into());
        }
        Ok(Report {
            winding_current_a: current,
            bus_current_a: input.duty * current,
            output_torque_nm: torque,
            feasible_torque_interval_nm: interval,
            current_limit_satisfied: current.abs() <= self.current_limit_a,
            electrical_power_w: electrical,
            mechanical_power_w: mechanical,
            dissipation_w: heat.max(0.),
        })
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub duty: f64,
    pub supply_voltage_v: f64,
    pub temperature_k: f64,
    pub output_speed_rad_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub config: Config,
    pub input: Input,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub winding_current_a: f64,
    pub bus_current_a: f64,
    pub output_torque_nm: f64,
    /// None means no steady current can obey both voltage and current bounds.
    pub feasible_torque_interval_nm: Option<[f64; 2]>,
    pub current_limit_satisfied: bool,
    pub electrical_power_w: f64,
    pub mechanical_power_w: f64,
    pub dissipation_w: f64,
}
/// Exact decay of a winding's initial-current error under held voltage/speed.
/// No assurance is made about rapidly varying back EMF or voltage.
pub fn winding_error_retention(
    resistance_ohm: f64,
    inductance_h: f64,
    held_s: f64,
) -> Result<f64, String> {
    if !resistance_ohm.is_finite()
        || resistance_ohm <= 0.
        || !inductance_h.is_finite()
        || inductance_h < 0.
        || !held_s.is_finite()
        || held_s < 0.
    {
        return Err("positive resistance and nonnegative inductance/held duration required".into());
    }
    Ok(if inductance_h == 0. {
        0.
    } else {
        (-held_s * resistance_ohm / inductance_h).exp()
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReductionRequest {
    pub motor_parameters: std::collections::BTreeMap<String, f64>,
    pub dynamics: crate::motor::MotorDynamics,
    pub held_interval_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReductionReport {
    pub motor_parameters: std::collections::BTreeMap<String, f64>,
    pub dynamics: crate::motor::MotorDynamics,
    pub reference_temperature_winding_error_retention: f64,
    pub omitted_storage_terms: Vec<String>,
}
/// Prepare the existing MotorUnit's explicit reduction flags; physical values
/// remain intact, including rotor inertia, friction, backlash and thermal laws.
/// Controller/sensor delay is still owned by the unchanged sampled controller.
pub fn prepare_reduction(mut r: ReductionRequest) -> Result<ReductionReport, String> {
    let mut registry = BehaviorRegistry::default();
    crate::motor::register(&mut registry).map_err(|e| e.to_string())?;
    let descriptor = registry
        .get(&crate::motor::MOTOR_UNIT.into())
        .map_err(|e| e.to_string())?;
    descriptor
        .validate_parameters(&r.motor_parameters)
        .map_err(|e| e.to_string())?;
    for name in [
        "inductance",
        "rotor_inertia",
        "gear_inertia",
        "ratio",
        "backlash",
        "gear_friction",
        "no_load_current",
        "gear_stiffness",
        "gear_damping",
        "efficiency",
    ] {
        if !r.motor_parameters.contains_key(name) {
            return Err(format!(
                "reduction requires explicit original {name}; no physical default is inserted"
            ));
        }
    }
    let retention = winding_error_retention(
        r.motor_parameters["resistance"],
        r.motor_parameters["inductance"],
        r.held_interval_s,
    )?;
    r.motor_parameters
        .insert("dynamics.quasistatic_winding".into(), 0.);
    r.motor_parameters
        .insert("dynamics.quasistatic_rotor".into(), 0.);
    let flags = r.dynamics.parameter_flags();
    let mut omitted = vec![];
    for (name, value) in flags {
        r.motor_parameters.insert(name.into(), value);
        omitted.push(name.into());
    }
    descriptor
        .validate_parameters(&r.motor_parameters)
        .map_err(|e| e.to_string())?;
    Ok(ReductionReport {
        motor_parameters: r.motor_parameters,
        dynamics: r.dynamics,
        reference_temperature_winding_error_retention: retention,
        omitted_storage_terms: omitted,
    })
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), String> {
    registry.register_primitive(Descriptor::new("actuation.prepare_reduction","Explicit storage reduction using the existing MotorUnit equations",
        vec![F::structured("motor_parameters","robot.motor_unit registry parameter units","named scalars"),F::structured("dynamics","1","MotorDynamics"),F::quantity("held_interval_s",Q::Time,"scalar")],
        vec![F::structured("motor_parameters","robot.motor_unit registry parameter units","named scalars"),F::structured("dynamics","1","MotorDynamics"),
            F::quantity("reference_temperature_winding_error_retention",Q::Dimensionless,"scalar"),F::structured("omitted_storage_terms","1","flag name array")],
        &["Retains original physical parameters; only explicit MotorDynamics flags change", "Winding decay diagnostic assumes held voltage/speed and reference-temperature resistance", "Rotor reduction is not justified by the winding diagnostic; accuracy requires later matched-model validation", "Does not alter controllers, CAD, runtime profiles or run an experiment"]),prepare_reduction)?;
    let mut inputs = vec![
        F::quantity("input.duty", Q::Dimensionless, "scalar"),
        F::quantity("input.supply_voltage_v", Q::Voltage, "scalar"),
        F::quantity("input.temperature_k", Q::Temperature, "scalar"),
        F::quantity("input.output_speed_rad_s", Q::AngularVelocity, "scalar"),
    ];
    for (name, unit) in [
        ("resistance_ohm", "Ω"),
        ("resistance_temperature_coefficient_per_k", "1/K"),
        ("reference_temperature_k", "K"),
        ("motor_constant_nm_a", "N·m/A = V·s/rad"),
        ("gear_ratio", "1"),
        ("gear_efficiency", "1"),
        ("current_limit_a", "A"),
        ("viscous_friction_nm_s_rad", "N·m·s/rad"),
        ("coulomb_friction_nm", "N·m"),
        ("friction_speed_rad_s", "rad/s"),
    ] {
        inputs.push(F::structured(&format!("config.{name}"), unit, "scalar"));
    }
    registry.register_primitive(Descriptor::new("actuation.dc_envelope", "Voltage-aware steady DC drive feasibility and power", inputs,
        vec![F::quantity("winding_current_a",Q::Current,"scalar"),F::quantity("bus_current_a",Q::Current,"scalar"),
            F::quantity("output_torque_nm",Q::Torque,"scalar"),F::quantity("feasible_torque_interval_nm",Q::Torque,"optional pair"),
            F::structured("current_limit_satisfied","1","boolean"),F::quantity("electrical_power_w",Q::Power,"scalar"),
            F::quantity("mechanical_power_w",Q::Power,"scalar"),F::quantity("dissipation_w",Q::Power,"scalar")],
        &["Quasistatic winding, ideal averaged bidirectional bridge, supplied temperature and voltage", "Supply must accept regeneration; use shared power components to model its limits", "No current clipping, hidden controller, rotor acceleration or inferred hardware ratings", "Friction is regularized sliding friction; static breakaway is not identified"]),
        |r: Request| r.config.evaluate(r.input))
}
