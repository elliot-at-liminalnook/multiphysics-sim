//! Independent mechanical axes coupled through one explicit electrical source.
//! Composes registry components; it introduces no alternative motor equations.
use super::{ModelSettings, PowerChannels};
use crate::controller_refinement::power::{Sample, Setup};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisSetup {
    pub key: String,
    pub model: ModelSettings,
    pub temperature_c: f64,
    pub command_and_sample_times_s: Vec<f64>,
    pub branch_resistance_ohm: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupSetup {
    pub source: Setup,
    pub shared_resistance_ohm: f64,
    pub axes: Vec<AxisSetup>,
    pub evidence: String,
    pub seed: u64,
}
pub struct GroupAxis {
    pub controller: sim_core::BehaviorId,
    pub angle: sim_core::StateId,
    pub duty: sim_core::StateId,
    power: PowerChannels,
}
pub struct PreparedGroup {
    pub runtime: sim_compile::Runtime,
    pub axes: BTreeMap<String, GroupAxis>,
    bus_voltage: sim_core::StateId,
    source_current: sim_core::StateId,
}
impl PreparedGroup {
    pub fn electrical_sample(&self, key: &str, time_s: f64) -> Option<Sample> {
        Some(self.axes.get(key)?.power.sample(&self.runtime, time_s))
    }
    pub fn supply(&self) -> [f64; 3] {
        let v = self.runtime.get(self.bus_voltage);
        let i = self.runtime.get(self.source_current);
        [v, i, v * i]
    }
}
pub fn prepare_group(setup: &GroupSetup) -> Result<PreparedGroup, String> {
    setup.source.validate()?;
    let valid_r = |r: f64| r.is_finite() && r >= 0.;
    if setup.axes.is_empty()
        || setup.axes.len() > 32
        || setup.evidence.trim().is_empty()
        || !valid_r(setup.shared_resistance_ohm)
    {
        return Err(
            "Group requires 1–32 axes, explicit shared resistance and setup evidence".into(),
        );
    }
    let mut keys = BTreeSet::new();
    for a in &setup.axes {
        a.model.validate()?;
        if a.key.trim().is_empty()
            || !keys.insert(&a.key)
            || !valid_r(a.branch_resistance_ohm)
            || !a.temperature_c.is_finite()
            || a.temperature_c <= -273.15
            || a.model.power.is_some()
            || a.model.conditions.voltage_v.is_some()
            || a.model.conditions.temperature_c.is_some()
            || a.model.conditions.command_delay_s != 0.
            || a.command_and_sample_times_s.is_empty()
            || a.command_and_sample_times_s
                .iter()
                .any(|t| !t.is_finite() || *t < 0.)
            || a.command_and_sample_times_s
                .windows(2)
                .any(|t| t[0] >= t[1])
        {
            return Err("Axes require unique keys, ordered schedules, explicit conditions and no conflicting per-axis source/condition overrides".into());
        }
    }
    (|| -> Result<PreparedGroup, Box<dyn std::error::Error>> {
        let registry = crate::registry();
        let mut w = sim_core::ModelWorld::default();
        let source = w.part(
            &registry,
            "group.source",
            &setup.source.source_component,
            setup
                .source
                .source_parameters
                .iter()
                .map(|(k, v)| (k.as_str(), *v)),
        )?;
        let ground = w.part(
            &registry,
            "group.ground",
            sim_domain_electrical::elements::GROUND,
            [],
        )?;
        let negative = source
            .try_port("n")
            .ok_or("Source requires electrical p/n ports")?;
        let mut positive = source
            .try_port("p")
            .ok_or("Source requires electrical p/n ports")?;
        let mut negative_net = vec![negative, ground.port("pin")];
        if setup.shared_resistance_ohm > 0. {
            let r = w.part(
                &registry,
                "group.feed",
                sim_domain_electrical::elements::RESISTOR,
                [("resistance", setup.shared_resistance_ohm)],
            )?;
            w.connect([positive, r.port("p")]);
            positive = r.port("n");
        }
        let current = w.part(
            &registry,
            "group.current",
            sim_domain_sensing::CURRENT_SENSOR,
            [],
        )?;
        let voltage = w.part(
            &registry,
            "group.voltage",
            sim_domain_sensing::VOLTAGE_SENSOR,
            [],
        )?;
        let auxiliary = w.part(
            &registry,
            "group.auxiliary",
            sim_domain_electrical::elements::CURRENT_SOURCE,
            [("current", -setup.source.auxiliary_current_a)],
        )?;
        w.connect([positive, current.port("p")]);
        let bus = current.port("n");
        let mut bus_net = vec![bus, voltage.port("p"), auxiliary.port("p")];
        negative_net.extend([voltage.port("n"), auxiliary.port("n")]);
        w.connect([voltage.port("voltage")]);
        w.connect([current.port("current")]);
        let battery = setup.source.source_component == sim_domain_robot::motor::BATTERY;
        if battery {
            w.connect([source.port("soc")]);
        }
        let mut parts = Vec::new();
        for a in &setup.axes {
            let name = |label: &str| format!("axis.{}.{}", a.key, label);
            let motor = w.part(
                &registry,
                &name("motor"),
                sim_domain_robot::MOTOR_UNIT,
                a.model.motor.iter().map(|(k, v)| (k.as_str(), *v)),
            )?;
            let bridge = w.part(
                &registry,
                &name("bridge"),
                sim_domain_robot::H_BRIDGE,
                a.model.bridge.iter().map(|(k, v)| (k.as_str(), *v)),
            )?;
            let mut branch = bus;
            if a.branch_resistance_ohm > 0. {
                let r = w.part(
                    &registry,
                    &name("wire"),
                    sim_domain_electrical::elements::RESISTOR,
                    [("resistance", a.branch_resistance_ohm)],
                )?;
                bus_net.push(r.port("p"));
                branch = r.port("n");
            }
            let amps = w.part(
                &registry,
                &name("current"),
                sim_domain_sensing::CURRENT_SENSOR,
                [],
            )?;
            let volts = w.part(
                &registry,
                &name("voltage"),
                sim_domain_sensing::VOLTAGE_SENSOR,
                [],
            )?;
            if branch == bus {
                bus_net.push(amps.port("p"));
            } else {
                w.connect([branch, amps.port("p")]);
            }
            w.connect([amps.port("n"), bridge.port("supply_p"), volts.port("p")]);
            negative_net.extend([
                volts.port("n"),
                bridge.port("supply_n"),
                bridge.port("n"),
                motor.port("n"),
            ]);
            w.connect([bridge.port("p"), motor.port("p")]);
            let ambient = w.part(
                &registry,
                &name("temperature"),
                sim_domain_thermal::AMBIENT,
                [("temperature", a.temperature_c + 273.15)],
            )?;
            w.connect([ambient.port("node"), motor.port("winding")]);
            let load = w.part(
                &registry,
                &name("load"),
                sim_domain_rotational::elements::INERTIA,
                [("inertia", a.model.conditions.load_inertia)],
            )?;
            let torque = w.part(
                &registry,
                &name("torque"),
                sim_domain_rotational::elements::TORQUE_SOURCE,
                [],
            )?;
            let load_command = w.part(
                &registry,
                &name("load_command"),
                sim_domain_control::elements::CONSTANT,
                [("value", -a.model.conditions.load_torque)],
            )?;
            let angle = w.part(
                &registry,
                &name("angle"),
                sim_domain_rotational::elements::ANGLE_SENSOR,
                [],
            )?;
            w.connect([
                motor.port("shaft"),
                load.port("shaft"),
                torque.port("shaft"),
                angle.port("shaft"),
            ]);
            w.connect([load_command.port("value"), torque.port("torque")]);
            let controller = sim_domain_control::pwm_feedback::add(
                &mut w,
                &name("controller"),
                false,
                sim_core::Clock::Times { times: a.command_and_sample_times_s.clone() },
            )?;
            w.connect([angle.port("angle"), controller.port("angle")]);
            w.connect([controller.port("duty"), bridge.port("command")]);
            for port in [
                amps.port("current"),
                volts.port("voltage"),
                motor.port("current"),
                motor.port("speed"),
                motor.port("torque"),
            ] {
                w.connect([port]);
            }
            parts.push((a.key.clone(), controller, angle, motor, volts, amps));
        }
        w.connect(bus_net);
        w.connect(negative_net);
        let mut runtime = sim_compile::Runtime::new(
            w,
            &registry,
            sim_dynamics::Integrator::BackwardEuler(crate::newton()),
        )?;
        runtime.seed(setup.seed);
        let axes = parts
            .into_iter()
            .map(|(key, controller, angle, motor, volts, amps)| {
                let axis = GroupAxis {
                    controller: controller.behavior,
                    angle: runtime.signal_id(angle.port("angle")),
                    duty: runtime.signal_id(controller.port("duty")),
                    power: PowerChannels {
                        supply_voltage: runtime.signal_id(volts.port("voltage")),
                        supply_current: runtime.signal_id(amps.port("current")),
                        winding_voltage: runtime.across_id(motor.port("p")),
                        winding_current: runtime.signal_id(motor.port("current")),
                        state_of_charge: battery.then(|| runtime.state_id(source.behavior, "soc")),
                    },
                };
                (key, axis)
            })
            .collect();
        let bus_voltage = runtime.signal_id(voltage.port("voltage"));
        let source_current = runtime.signal_id(current.port("current"));
        Ok(PreparedGroup {
            runtime,
            axes,
            bus_voltage,
            source_current,
        })
    })()
    .map_err(|e| e.to_string())
}
