//! Deterministic runtime for the first complete controller-to-actuator slice.

use serde::{Deserialize, Serialize};
use sim_compile::{CompileError, CompiledModel, compile};
use sim_core::{
    BehaviorRegistry, ConnectorKind, ModelWorld, PortSchema, QuantityKind, RegistryError,
    StateError, StateId,
};
use sim_domain_bridges::{DcMotor, LeadScrew};
use sim_domain_control::{
    POSITION_CONTROLLER, POSITION_SENSOR, POSITION_SETPOINT, PositionController,
    PositionControllerConfig, PositionControllerState,
};
use sim_domain_electrical::{AVERAGED_H_BRIDGE, AveragedHBridge, POWER_SUPPLY, PowerSupply};
use sim_domain_rotational::{
    FIXED_MOUNT as ROTATIONAL_FIXED_MOUNT, IDEAL_GEAR, IdealGear, ROTOR_INERTIA, Rotor,
};
use sim_domain_translational::{
    COMPLIANT_END_STOP, CompliantEndStop, FIXED_MOUNT as TRANSLATIONAL_FIXED_MOUNT, LINEAR_MASS,
    LinearLoad, PRISMATIC_GUIDE,
};
use sim_solve::{NewtonConfig, SolveDiagnostics, SolveError, solve_newton};
use std::collections::BTreeMap;
use thiserror::Error;

const I: usize = 0;
const OMEGA: usize = 1;
const THETA: usize = 2;
const VELOCITY: usize = 3;
const POSITION: usize = 4;
const DRIVE_FORCE: usize = 5;
const UNKNOWN_COUNT: usize = 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActuatorConfig {
    pub plant_step: f64,
    pub controller: PositionControllerConfig,
    pub supply: PowerSupply,
    pub driver: AveragedHBridge,
    pub motor: DcMotor,
    pub rotor: Rotor,
    pub gear: IdealGear,
    pub screw: LeadScrew,
    pub load: LinearLoad,
    pub end_stop: CompliantEndStop,
    pub newton: NewtonConfig,
    pub trace_every_steps: u64,
}

impl Default for ActuatorConfig {
    fn default() -> Self {
        Self {
            plant_step: 50.0e-6,
            controller: PositionControllerConfig::default(),
            supply: PowerSupply::default(),
            driver: AveragedHBridge::default(),
            motor: DcMotor::default(),
            rotor: Rotor::default(),
            gear: IdealGear::default(),
            screw: LeadScrew::default(),
            load: LinearLoad::default(),
            end_stop: CompliantEndStop::default(),
            newton: NewtonConfig::default(),
            trace_every_steps: 20,
        }
    }
}

impl ActuatorConfig {
    pub fn control_every_steps(&self) -> Result<u64, RuntimeError> {
        let ratio = self.controller.sample_period / self.plant_step;
        let rounded = ratio.round();
        if rounded < 1.0 || (ratio - rounded).abs() > 1.0e-10 {
            return Err(RuntimeError::IncommensurateRates {
                plant_step: self.plant_step,
                control_period: self.controller.sample_period,
            });
        }
        Ok(rounded as u64)
    }

    fn validate(&self) -> Result<(), RuntimeError> {
        self.control_every_steps()?;
        if self.plant_step <= 0.0
            || self.motor.inductance <= 0.0
            || self.rotor.inertia <= 0.0
            || self.load.mass <= 0.0
            || self.gear.reduction <= 0.0
            || !(0.0 < self.gear.efficiency && self.gear.efficiency <= 1.0)
            || self.screw.lead <= 0.0
        {
            return Err(RuntimeError::InvalidConfiguration);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RuntimeInputs {
    pub target_position: f64,
    pub supply_voltage: f64,
    /// Positive force acts in the positive carriage direction.
    pub external_force: f64,
    /// Optional upper stop that is closer than the physical stroke limit.
    pub obstruction_position: Option<f64>,
    pub controller_enabled: bool,
    /// Bypasses the outer controller while preserving the sampled hold timing.
    pub manual_duty: Option<f64>,
}

impl Default for RuntimeInputs {
    fn default() -> Self {
        Self {
            target_position: 0.100,
            supply_voltage: 24.0,
            external_force: 0.0,
            obstruction_position: None,
            controller_enabled: true,
            manual_duty: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActuatorStateIds {
    pub target_position: StateId,
    pub measured_position: StateId,
    pub controller_integral: StateId,
    pub duty: StateId,
    pub bus_voltage: StateId,
    pub motor_voltage: StateId,
    pub motor_current: StateId,
    pub motor_torque: StateId,
    pub motor_angle: StateId,
    pub motor_speed: StateId,
    pub gear_angle: StateId,
    pub gear_speed: StateId,
    pub carriage_position: StateId,
    pub carriage_velocity: StateId,
    pub drive_force: StateId,
    pub external_force: StateId,
    pub stop_force: StateId,
    pub motor_mount_reaction: StateId,
    pub chassis_reaction_force: StateId,
    pub current_limited: StateId,
    pub newton_iterations: StateId,
    pub residual_norm: StateId,
    pub stored_energy: StateId,
    pub power_balance_error: StateId,
    pub cumulative_energy_error: StateId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub time: f64,
    pub target_position: f64,
    pub position: f64,
    pub velocity: f64,
    pub duty: f64,
    pub bus_voltage: f64,
    pub motor_voltage: f64,
    pub current: f64,
    pub motor_speed: f64,
    pub motor_angle: f64,
    pub gear_angle: f64,
    pub drive_force: f64,
    pub stop_force: f64,
    pub mount_reaction_torque: f64,
    pub chassis_reaction_force: f64,
    pub current_limited: bool,
    pub controller_integral: f64,
    pub newton_iterations: usize,
    pub residual_norm: f64,
    pub stored_energy: f64,
    pub power_balance_error: f64,
    pub cumulative_energy_error: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub duration: f64,
    pub steps: u64,
    pub controller_samples: u64,
    pub final_position: f64,
    pub final_velocity: f64,
    pub peak_current: f64,
    pub peak_abs_power_balance_error: f64,
    pub cumulative_energy_error: f64,
    pub current_limit_activations: u64,
    pub max_newton_iterations: usize,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Compile(#[from] CompileError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error(transparent)]
    Solve(#[from] SolveError),
    #[error("plant step {plant_step:e}s does not divide control period {control_period:e}s")]
    IncommensurateRates {
        plant_step: f64,
        control_period: f64,
    },
    #[error("invalid actuator configuration")]
    InvalidConfiguration,
    #[error("injected failure before commit")]
    InjectedFailure,
}

pub struct ActuatorSimulation {
    pub config: ActuatorConfig,
    pub inputs: RuntimeInputs,
    pub model: ModelWorld,
    pub registry: BehaviorRegistry,
    pub compiled: CompiledModel,
    pub ids: ActuatorStateIds,
    pub observables: BTreeMap<String, StateId>,
    time: f64,
    step_index: u64,
    controller_sample_count: u64,
    current_limit_activations: u64,
    max_newton_iterations: usize,
    peak_current: f64,
    peak_abs_power_balance_error: f64,
    active_upper_stop: f64,
    trace: Vec<Sample>,
}

impl ActuatorSimulation {
    pub fn new(config: ActuatorConfig) -> Result<Self, RuntimeError> {
        config.validate()?;
        let mut registry = BehaviorRegistry::default();
        sim_domain_control::register(&mut registry)?;
        sim_domain_electrical::register(&mut registry)?;
        sim_domain_rotational::register(&mut registry)?;
        sim_domain_translational::register(&mut registry)?;
        sim_domain_bridges::register(&mut registry)?;

        let (model, ids, observables) = build_model()?;
        let compiled = compile(&model, &registry)?;
        let active_upper_stop = config.load.max_position;
        Ok(Self {
            inputs: RuntimeInputs {
                supply_voltage: config.supply.voltage,
                ..RuntimeInputs::default()
            },
            config,
            model,
            registry,
            compiled,
            ids,
            observables,
            time: 0.0,
            step_index: 0,
            controller_sample_count: 0,
            current_limit_activations: 0,
            max_newton_iterations: 0,
            peak_current: 0.0,
            peak_abs_power_balance_error: 0.0,
            active_upper_stop,
            trace: Vec::new(),
        })
    }

    pub fn time(&self) -> f64 {
        self.time
    }

    pub fn step(&mut self) -> Result<SolveDiagnostics, RuntimeError> {
        self.step_inner(false)
    }

    /// Test hook proving that all trial mutations are discarded before commit.
    pub fn step_with_injected_failure(&mut self) -> Result<SolveDiagnostics, RuntimeError> {
        self.step_inner(true)
    }

    fn step_inner(&mut self, inject_failure: bool) -> Result<SolveDiagnostics, RuntimeError> {
        let h = self.config.plant_step;
        let control_every = self.config.control_every_steps()?;
        let mut pending_controller_sample_count = self.controller_sample_count;
        let mut trial = self.model.state.begin_trial();
        trial.set(self.ids.target_position, self.inputs.target_position)?;
        trial.set(self.ids.bus_voltage, self.inputs.supply_voltage)?;
        trial.set(self.ids.external_force, self.inputs.external_force)?;

        if self.step_index.is_multiple_of(control_every) {
            let mut controller_state = PositionControllerState {
                integral: trial.get(self.ids.controller_integral)?,
                duty: trial.get(self.ids.duty)?,
                last_error: 0.0,
                sample_count: pending_controller_sample_count,
            };
            let controller = PositionController {
                config: self.config.controller,
            };
            if let Some(manual_duty) = self.inputs.manual_duty {
                controller_state.duty = manual_duty.clamp(
                    -self.config.controller.duty_limit,
                    self.config.controller.duty_limit,
                );
                controller_state.sample_count += 1;
            } else if self.inputs.controller_enabled {
                controller.sample(
                    &mut controller_state,
                    self.inputs.target_position,
                    self.model.state.get(self.ids.carriage_position)?,
                );
            } else {
                controller_state.duty = 0.0;
                controller_state.sample_count += 1;
            }
            trial.set(self.ids.controller_integral, controller_state.integral)?;
            trial.set(self.ids.duty, controller_state.duty)?;
            pending_controller_sample_count = controller_state.sample_count;
        }

        let old = [
            self.model.state.get(self.ids.motor_current)?,
            self.model.state.get(self.ids.motor_speed)?,
            self.model.state.get(self.ids.motor_angle)?,
            self.model.state.get(self.ids.carriage_velocity)?,
            self.model.state.get(self.ids.carriage_position)?,
            self.model.state.get(self.ids.drive_force)?,
        ];
        let mut next = old;
        let duty = trial.get(self.ids.duty)?;
        let bus_voltage = self.inputs.supply_voltage;
        let external_force = self.inputs.external_force;
        let upper_stop = self
            .inputs
            .obstruction_position
            .unwrap_or(self.config.load.max_position)
            .min(self.config.load.max_position);
        let minimum = self.config.load.min_position;
        let motor = self.config.motor;
        let rotor = self.config.rotor;
        let load = self.config.load;
        let end_stop = self.config.end_stop;
        let driver = self.config.driver;
        let gear = self.config.gear;
        let metres_per_motor_radian = self.config.screw.metres_per_screw_radian() / gear.reduction;

        let diagnostics = solve_newton(&mut next, self.config.newton, |next, residual| {
            let midpoint = std::array::from_fn::<_, UNKNOWN_COUNT, _>(|index| {
                0.5 * (old[index] + next[index])
            });
            let driver_output = driver.output(bus_voltage, duty, midpoint[I]);
            let stop_force = end_stop.discrete_force(
                old[POSITION],
                next[POSITION],
                midpoint[VELOCITY],
                minimum,
                upper_stop,
            );
            let reflected_load_torque = reflected_torque(
                metres_per_motor_radian,
                midpoint[DRIVE_FORCE],
                midpoint[OMEGA],
                gear.efficiency,
            );

            residual[I] = next[I]
                - old[I]
                - h / motor.inductance
                    * (driver_output.motor_voltage
                        - motor.resistance * midpoint[I]
                        - motor.back_emf_constant * midpoint[OMEGA]);
            residual[OMEGA] = next[OMEGA]
                - old[OMEGA]
                - h / rotor.inertia
                    * (motor.torque_constant * midpoint[I]
                        - rotor.viscous_drag * midpoint[OMEGA]
                        - reflected_load_torque);
            residual[THETA] = next[THETA] - old[THETA] - h * midpoint[OMEGA];
            residual[VELOCITY] = next[VELOCITY]
                - old[VELOCITY]
                - h / load.mass
                    * (midpoint[DRIVE_FORCE] + external_force + stop_force
                        - load.viscous_drag * midpoint[VELOCITY]);
            residual[POSITION] = next[VELOCITY] - metres_per_motor_radian * next[OMEGA];
            residual[DRIVE_FORCE] = next[POSITION] - metres_per_motor_radian * next[THETA];
        })?;

        let midpoint =
            std::array::from_fn::<_, UNKNOWN_COUNT, _>(|index| 0.5 * (old[index] + next[index]));
        let driver_output = driver.output(bus_voltage, duty, midpoint[I]);
        let stop_force = end_stop.discrete_force(
            old[POSITION],
            next[POSITION],
            midpoint[VELOCITY],
            minimum,
            upper_stop,
        );
        let motor_torque = motor.torque_constant * next[I];
        let gear_angle = gear.output_angle(next[THETA]);
        let gear_speed = gear.output_speed(next[OMEGA]);
        let old_energy = self.model.state.get(self.ids.stored_energy)?;
        let new_energy = stored_energy(&self.config, next, upper_stop);
        let boundary_work = end_stop.potential(old[POSITION], minimum, upper_stop)
            - end_stop.potential(old[POSITION], minimum, self.active_upper_stop);
        let copper_loss = motor.resistance * midpoint[I] * midpoint[I];
        let rotor_loss = rotor.viscous_drag * midpoint[OMEGA] * midpoint[OMEGA];
        let linear_loss = load.viscous_drag * midpoint[VELOCITY] * midpoint[VELOCITY];
        let load_torque = reflected_torque(
            metres_per_motor_radian,
            midpoint[DRIVE_FORCE],
            midpoint[OMEGA],
            gear.efficiency,
        );
        let gear_loss =
            (load_torque * midpoint[OMEGA] - midpoint[DRIVE_FORCE] * midpoint[VELOCITY]).max(0.0);
        let stop_damping_loss = stop_damping_loss(
            end_stop,
            next[POSITION],
            midpoint[VELOCITY],
            minimum,
            upper_stop,
        );
        let supply_power = driver_output.requested_voltage * midpoint[I];
        let power_balance_error =
            supply_power + external_force * midpoint[VELOCITY] + boundary_work / h
                - driver_output.loss
                - copper_loss
                - rotor_loss
                - linear_loss
                - gear_loss
                - stop_damping_loss
                - (new_energy - old_energy) / h;

        trial.set(self.ids.measured_position, next[POSITION])?;
        trial.set(self.ids.motor_voltage, driver_output.motor_voltage)?;
        trial.set(self.ids.motor_current, next[I])?;
        trial.set(self.ids.motor_torque, motor_torque)?;
        trial.set(self.ids.motor_angle, next[THETA])?;
        trial.set(self.ids.motor_speed, next[OMEGA])?;
        trial.set(self.ids.gear_angle, gear_angle)?;
        trial.set(self.ids.gear_speed, gear_speed)?;
        trial.set(self.ids.carriage_velocity, next[VELOCITY])?;
        trial.set(self.ids.carriage_position, next[POSITION])?;
        trial.set(self.ids.drive_force, next[DRIVE_FORCE])?;
        trial.set(self.ids.stop_force, stop_force)?;
        trial.set(self.ids.motor_mount_reaction, -motor_torque)?;
        trial.set(self.ids.chassis_reaction_force, -stop_force)?;
        trial.set(
            self.ids.current_limited,
            if driver_output.current_limited {
                1.0
            } else {
                0.0
            },
        )?;
        trial.set(self.ids.newton_iterations, diagnostics.iterations as f64)?;
        trial.set(self.ids.residual_norm, diagnostics.residual_norm)?;
        trial.set(self.ids.stored_energy, new_energy)?;
        trial.set(self.ids.power_balance_error, power_balance_error)?;
        let cumulative_energy_error =
            self.model.state.get(self.ids.cumulative_energy_error)? + power_balance_error * h;
        trial.set(self.ids.cumulative_energy_error, cumulative_energy_error)?;

        if inject_failure {
            return Err(RuntimeError::InjectedFailure);
        }

        self.model.state.commit(trial)?;
        self.time += h;
        self.step_index += 1;
        self.controller_sample_count = pending_controller_sample_count;
        self.active_upper_stop = upper_stop;
        if driver_output.current_limited {
            self.current_limit_activations += 1;
        }
        self.max_newton_iterations = self.max_newton_iterations.max(diagnostics.iterations);
        self.peak_current = self.peak_current.max(next[I].abs());
        self.peak_abs_power_balance_error = self
            .peak_abs_power_balance_error
            .max(power_balance_error.abs());
        if self
            .step_index
            .is_multiple_of(self.config.trace_every_steps.max(1))
        {
            self.trace.push(self.sample()?);
        }
        Ok(diagnostics)
    }

    pub fn run_for(&mut self, duration: f64) -> Result<RunSummary, RuntimeError> {
        let steps = (duration / self.config.plant_step).round() as u64;
        for _ in 0..steps {
            self.step()?;
        }
        self.summary()
    }

    pub fn sample(&self) -> Result<Sample, RuntimeError> {
        let state = &self.model.state;
        Ok(Sample {
            time: self.time,
            target_position: state.get(self.ids.target_position)?,
            position: state.get(self.ids.carriage_position)?,
            velocity: state.get(self.ids.carriage_velocity)?,
            duty: state.get(self.ids.duty)?,
            bus_voltage: state.get(self.ids.bus_voltage)?,
            motor_voltage: state.get(self.ids.motor_voltage)?,
            current: state.get(self.ids.motor_current)?,
            motor_speed: state.get(self.ids.motor_speed)?,
            motor_angle: state.get(self.ids.motor_angle)?,
            gear_angle: state.get(self.ids.gear_angle)?,
            drive_force: state.get(self.ids.drive_force)?,
            stop_force: state.get(self.ids.stop_force)?,
            mount_reaction_torque: state.get(self.ids.motor_mount_reaction)?,
            chassis_reaction_force: state.get(self.ids.chassis_reaction_force)?,
            current_limited: state.get(self.ids.current_limited)? > 0.5,
            controller_integral: state.get(self.ids.controller_integral)?,
            newton_iterations: state.get(self.ids.newton_iterations)? as usize,
            residual_norm: state.get(self.ids.residual_norm)?,
            stored_energy: state.get(self.ids.stored_energy)?,
            power_balance_error: state.get(self.ids.power_balance_error)?,
            cumulative_energy_error: state.get(self.ids.cumulative_energy_error)?,
        })
    }

    pub fn summary(&self) -> Result<RunSummary, RuntimeError> {
        let sample = self.sample()?;
        Ok(RunSummary {
            duration: self.time,
            steps: self.step_index,
            controller_samples: self.controller_sample_count,
            final_position: sample.position,
            final_velocity: sample.velocity,
            peak_current: self.peak_current,
            peak_abs_power_balance_error: self.peak_abs_power_balance_error,
            cumulative_energy_error: sample.cumulative_energy_error,
            current_limit_activations: self.current_limit_activations,
            max_newton_iterations: self.max_newton_iterations,
        })
    }

    pub fn trace(&self) -> &[Sample] {
        &self.trace
    }

    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }

    pub fn state(&self, path: &str) -> Option<f64> {
        self.observables
            .get(path)
            .and_then(|id| self.model.state.get(*id).ok())
    }
}

fn stored_energy(config: &ActuatorConfig, state: [f64; UNKNOWN_COUNT], upper_stop: f64) -> f64 {
    let magnetic = 0.5 * config.motor.inductance * state[I] * state[I];
    let rotational = 0.5 * config.rotor.inertia * state[OMEGA] * state[OMEGA];
    let linear = 0.5 * config.load.mass * state[VELOCITY] * state[VELOCITY];
    let stop = config
        .end_stop
        .potential(state[POSITION], config.load.min_position, upper_stop);
    magnetic + rotational + linear + stop
}

fn reflected_torque(
    metres_per_motor_radian: f64,
    force: f64,
    motor_speed: f64,
    efficiency: f64,
) -> f64 {
    let lossless = metres_per_motor_radian * force;
    if force * motor_speed >= 0.0 {
        lossless / efficiency
    } else {
        lossless * efficiency
    }
}

fn stop_damping_loss(
    stop: CompliantEndStop,
    position: f64,
    velocity: f64,
    minimum: f64,
    maximum: f64,
) -> f64 {
    if (position > maximum && velocity > 0.0) || (position < minimum && velocity < 0.0) {
        stop.damping * velocity * velocity
    } else {
        0.0
    }
}

fn add_state(
    model: &mut ModelWorld,
    observables: &mut BTreeMap<String, StateId>,
    path: &str,
    quantity: QuantityKind,
    initial: f64,
) -> Result<StateId, StateError> {
    let id = model.state.register(path, quantity, initial)?;
    observables.insert(path.to_owned(), id);
    Ok(id)
}

fn build_model() -> Result<(ModelWorld, ActuatorStateIds, BTreeMap<String, StateId>), RuntimeError>
{
    let mut model = ModelWorld::default();
    let mut observables = BTreeMap::new();
    let ids = ActuatorStateIds {
        target_position: add_state(
            &mut model,
            &mut observables,
            "control.target",
            QuantityKind::Length,
            0.0,
        )?,
        measured_position: add_state(
            &mut model,
            &mut observables,
            "sensor.position",
            QuantityKind::Length,
            0.0,
        )?,
        controller_integral: add_state(
            &mut model,
            &mut observables,
            "controller.integral",
            QuantityKind::Dimensionless,
            0.0,
        )?,
        duty: add_state(
            &mut model,
            &mut observables,
            "driver.duty",
            QuantityKind::Dimensionless,
            0.0,
        )?,
        bus_voltage: add_state(
            &mut model,
            &mut observables,
            "supply.voltage",
            QuantityKind::Voltage,
            24.0,
        )?,
        motor_voltage: add_state(
            &mut model,
            &mut observables,
            "motor.voltage",
            QuantityKind::Voltage,
            0.0,
        )?,
        motor_current: add_state(
            &mut model,
            &mut observables,
            "motor.current",
            QuantityKind::Current,
            0.0,
        )?,
        motor_torque: add_state(
            &mut model,
            &mut observables,
            "motor.torque",
            QuantityKind::Torque,
            0.0,
        )?,
        motor_angle: add_state(
            &mut model,
            &mut observables,
            "motor.angle",
            QuantityKind::Angle,
            0.0,
        )?,
        motor_speed: add_state(
            &mut model,
            &mut observables,
            "motor.speed",
            QuantityKind::AngularVelocity,
            0.0,
        )?,
        gear_angle: add_state(
            &mut model,
            &mut observables,
            "gear.output_angle",
            QuantityKind::Angle,
            0.0,
        )?,
        gear_speed: add_state(
            &mut model,
            &mut observables,
            "gear.output_speed",
            QuantityKind::AngularVelocity,
            0.0,
        )?,
        carriage_position: add_state(
            &mut model,
            &mut observables,
            "carriage.position",
            QuantityKind::Length,
            0.0,
        )?,
        carriage_velocity: add_state(
            &mut model,
            &mut observables,
            "carriage.velocity",
            QuantityKind::LinearVelocity,
            0.0,
        )?,
        drive_force: add_state(
            &mut model,
            &mut observables,
            "screw.drive_force",
            QuantityKind::Force,
            0.0,
        )?,
        external_force: add_state(
            &mut model,
            &mut observables,
            "load.external_force",
            QuantityKind::Force,
            0.0,
        )?,
        stop_force: add_state(
            &mut model,
            &mut observables,
            "stop.force",
            QuantityKind::Force,
            0.0,
        )?,
        motor_mount_reaction: add_state(
            &mut model,
            &mut observables,
            "mount.motor_reaction",
            QuantityKind::Torque,
            0.0,
        )?,
        chassis_reaction_force: add_state(
            &mut model,
            &mut observables,
            "mount.chassis_reaction",
            QuantityKind::Force,
            0.0,
        )?,
        current_limited: add_state(
            &mut model,
            &mut observables,
            "driver.current_limited",
            QuantityKind::Dimensionless,
            0.0,
        )?,
        newton_iterations: add_state(
            &mut model,
            &mut observables,
            "solver.newton_iterations",
            QuantityKind::Dimensionless,
            0.0,
        )?,
        residual_norm: add_state(
            &mut model,
            &mut observables,
            "solver.residual_norm",
            QuantityKind::Dimensionless,
            0.0,
        )?,
        stored_energy: add_state(
            &mut model,
            &mut observables,
            "energy.stored",
            QuantityKind::Energy,
            0.0,
        )?,
        power_balance_error: add_state(
            &mut model,
            &mut observables,
            "energy.power_balance_error",
            QuantityKind::Power,
            0.0,
        )?,
        cumulative_energy_error: add_state(
            &mut model,
            &mut observables,
            "energy.cumulative_error",
            QuantityKind::Energy,
            0.0,
        )?,
    };

    let setpoint_object = model.add_object("position setpoint");
    let controller_object = model.add_object("motor controller");
    let supply_object = model.add_object("24 V supply");
    let driver_object = model.add_object("motor driver");
    let motor_object = model.add_object("DC motor");
    let gear_object = model.add_object("10:1 gearbox");
    let screw_object = model.add_object("lead screw");
    let carriage_object = model.add_object("linear carriage");
    let chassis_object = model.add_object("fixed chassis");

    let setpoint = model.add_behavior(setpoint_object, POSITION_SETPOINT);
    let controller = model.add_behavior(controller_object, POSITION_CONTROLLER);
    let sensor = model.add_behavior(carriage_object, POSITION_SENSOR);
    let supply = model.add_behavior(supply_object, POWER_SUPPLY);
    let bridge = model.add_behavior(driver_object, AVERAGED_H_BRIDGE);
    let motor = model.add_behavior(motor_object, sim_domain_bridges::DC_MOTOR);
    let rotor = model.add_behavior(motor_object, ROTOR_INERTIA);
    let motor_mount = model.add_behavior(chassis_object, ROTATIONAL_FIXED_MOUNT);
    let gear = model.add_behavior(gear_object, IDEAL_GEAR);
    let screw = model.add_behavior(screw_object, sim_domain_bridges::LEAD_SCREW);
    let mass = model.add_behavior(carriage_object, LINEAR_MASS);
    let guide = model.add_behavior(carriage_object, PRISMATIC_GUIDE);
    let stop = model.add_behavior(chassis_object, COMPLIANT_END_STOP);
    let chassis_mount = model.add_behavior(chassis_object, TRANSLATIONAL_FIXED_MOUNT);

    model.behaviors[controller].state = vec![ids.controller_integral, ids.duty];
    model.behaviors[motor].state = vec![ids.motor_current];
    model.behaviors[rotor].state = vec![ids.motor_angle, ids.motor_speed];
    model.behaviors[mass].state = vec![ids.carriage_position, ids.carriage_velocity];

    let setpoint_out = model.add_port(
        setpoint,
        "target",
        PortSchema::SignalOut(QuantityKind::Length),
    );
    let controller_target = model.add_port(
        controller,
        "target",
        PortSchema::SignalIn(QuantityKind::Length),
    );
    let controller_measured = model.add_port(
        controller,
        "measured",
        PortSchema::SignalIn(QuantityKind::Length),
    );
    let controller_duty = model.add_port(
        controller,
        "duty",
        PortSchema::SignalOut(QuantityKind::Dimensionless),
    );
    let sensor_axis = model.add_port(
        sensor,
        "axis",
        PortSchema::Acausal(ConnectorKind::Translational),
    );
    let sensor_out = model.add_port(
        sensor,
        "position",
        PortSchema::SignalOut(QuantityKind::Length),
    );
    let supply_dc = model.add_port(supply, "dc", PortSchema::Acausal(ConnectorKind::Electrical));
    let bridge_bus = model.add_port(
        bridge,
        "bus",
        PortSchema::Acausal(ConnectorKind::Electrical),
    );
    let bridge_motor = model.add_port(
        bridge,
        "motor",
        PortSchema::Acausal(ConnectorKind::Electrical),
    );
    let bridge_duty = model.add_port(
        bridge,
        "duty",
        PortSchema::SignalIn(QuantityKind::Dimensionless),
    );
    let motor_winding = model.add_port(
        motor,
        "winding",
        PortSchema::Acausal(ConnectorKind::Electrical),
    );
    let motor_shaft = model.add_port(
        motor,
        "shaft",
        PortSchema::Acausal(ConnectorKind::Rotational),
    );
    let motor_case = model.add_port(
        motor,
        "case",
        PortSchema::Acausal(ConnectorKind::Rotational),
    );
    let rotor_shaft = model.add_port(
        rotor,
        "shaft",
        PortSchema::Acausal(ConnectorKind::Rotational),
    );
    let mount_flange = model.add_port(
        motor_mount,
        "flange",
        PortSchema::Acausal(ConnectorKind::Rotational),
    );
    let gear_input = model.add_port(
        gear,
        "input",
        PortSchema::Acausal(ConnectorKind::Rotational),
    );
    let gear_output = model.add_port(
        gear,
        "output",
        PortSchema::Acausal(ConnectorKind::Rotational),
    );
    let screw_shaft = model.add_port(
        screw,
        "shaft",
        PortSchema::Acausal(ConnectorKind::Rotational),
    );
    let screw_carriage = model.add_port(
        screw,
        "carriage",
        PortSchema::Acausal(ConnectorKind::Translational),
    );
    let mass_axis = model.add_port(
        mass,
        "axis",
        PortSchema::Acausal(ConnectorKind::Translational),
    );
    let guide_axis = model.add_port(
        guide,
        "axis",
        PortSchema::Acausal(ConnectorKind::Translational),
    );
    let guide_chassis = model.add_port(guide, "chassis", PortSchema::Acausal(ConnectorKind::Frame));
    let stop_axis = model.add_port(
        stop,
        "axis",
        PortSchema::Acausal(ConnectorKind::Translational),
    );
    let chassis_frame = model.add_port(
        chassis_mount,
        "frame",
        PortSchema::Acausal(ConnectorKind::Frame),
    );

    model.connect([setpoint_out, controller_target]);
    model.connect([sensor_out, controller_measured]);
    model.connect([controller_duty, bridge_duty]);
    model.connect([supply_dc, bridge_bus]);
    model.connect([bridge_motor, motor_winding]);
    model.connect([motor_shaft, rotor_shaft, gear_input]);
    model.connect([motor_case, mount_flange]);
    model.connect([gear_output, screw_shaft]);
    model.connect([
        screw_carriage,
        mass_axis,
        guide_axis,
        stop_axis,
        sensor_axis,
    ]);
    model.connect([guide_chassis, chassis_frame]);

    Ok((model, ids, observables))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_compiles_to_one_physical_island() {
        let simulation = ActuatorSimulation::new(ActuatorConfig::default()).unwrap();
        assert_eq!(simulation.compiled.islands.len(), 1);
        assert!(simulation.compiled.islands[0].behaviors.len() >= 10);
    }

    #[test]
    fn controller_rate_is_exactly_twenty_plant_steps() {
        let config = ActuatorConfig::default();
        assert_eq!(config.control_every_steps().unwrap(), 20);
    }

    #[test]
    fn failed_step_rolls_back_controller_and_plant_state() {
        let mut simulation = ActuatorSimulation::new(ActuatorConfig::default()).unwrap();
        let before = simulation.sample().unwrap();
        let before_summary = simulation.summary().unwrap();
        assert!(matches!(
            simulation.step_with_injected_failure(),
            Err(RuntimeError::InjectedFailure)
        ));
        let after = simulation.sample().unwrap();
        assert_eq!(simulation.time(), 0.0);
        assert_eq!(before.position, after.position);
        assert_eq!(before.current, after.current);
        assert_eq!(before.controller_integral, after.controller_integral);
        assert_eq!(before.duty, after.duty);
        assert_eq!(
            before_summary.controller_samples,
            simulation.summary().unwrap().controller_samples
        );
    }

    #[test]
    fn screw_and_gear_constraints_hold() {
        let mut simulation = ActuatorSimulation::new(ActuatorConfig::default()).unwrap();
        simulation.run_for(0.2).unwrap();
        let sample = simulation.sample().unwrap();
        let expected_gear_angle = sample.motor_angle / simulation.config.gear.reduction;
        let expected_position =
            simulation.config.screw.metres_per_screw_radian() * expected_gear_angle;
        assert!((sample.gear_angle - expected_gear_angle).abs() < 1.0e-10);
        assert!((sample.position - expected_position).abs() < 1.0e-10);
    }

    #[test]
    fn open_loop_duty_drives_the_complete_power_train() {
        let mut simulation = ActuatorSimulation::new(ActuatorConfig::default()).unwrap();
        simulation.inputs.manual_duty = Some(0.5);
        simulation.run_for(0.5).unwrap();
        let sample = simulation.sample().unwrap();
        assert!(sample.motor_speed > 100.0);
        assert!(sample.position > 0.005);
        assert!(sample.current.abs() <= simulation.config.driver.current_limit + 0.5);
    }
}
