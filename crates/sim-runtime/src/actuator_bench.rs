//! Shared single-axis physical fixture for replay, control and model identification.
use crate::experiment_study::ModelSettings;
use sim_domain_robot::motor::*;
mod group;
pub use group::{AxisSetup, GroupSetup, PreparedGroup, prepare_group};
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DriverRelease {
    ElectricalBrake,
    TorqueOff {
        off_conductance_s: f64,
        diode_drop_v: f64,
        diode_resistance_ohm: f64,
    },
}
impl DriverRelease {
    pub fn validate(&self) -> Result<(), String> {
        if let Self::TorqueOff {
            off_conductance_s,
            diode_drop_v,
            diode_resistance_ohm,
        } = self
        {
            if [*off_conductance_s, *diode_drop_v, *diode_resistance_ohm]
                .iter()
                .any(|v| !v.is_finite() || *v < 0.)
                || *diode_resistance_ohm == 0.
            {
                return Err("Passive release parameters must be finite, nonnegative and have positive diode resistance".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone)]
pub enum Drive {
    Pulse {
        duty: f64,
        start_s: f64,
        duration_s: f64,
    },
    Feedback {
        period_s: f64,
    },
    Scheduled {
        times: Vec<f64>,
    },
    ReleasedPulse {
        duty: f64,
        start_s: f64,
        duration_s: f64,
        release: DriverRelease,
    },
}
pub struct PowerChannels {
    supply_voltage: sim_core::StateId,
    supply_current: sim_core::StateId,
    winding_voltage: sim_core::StateId,
    winding_current: sim_core::StateId,
    state_of_charge: Option<sim_core::StateId>,
}
pub struct PreparedBench {
    pub power: Option<PowerChannels>,
    pub runtime: sim_compile::Runtime,
    pub angle: sim_core::StateId,
    pub duty: sim_core::StateId,
    pub controller: Option<sim_core::BehaviorId>,
}
impl PreparedBench {
    pub fn electrical_sample(
        &self,
        time_s: f64,
    ) -> Option<crate::controller_refinement::power::Sample> {
        let p = self.power.as_ref()?;
        Some(p.sample(&self.runtime, time_s))
    }
}
impl PowerChannels {
    fn sample(
        &self,
        r: &sim_compile::Runtime,
        time_s: f64,
    ) -> crate::controller_refinement::power::Sample {
        let p = self;
        let voltage = r.get(p.supply_voltage);
        let current = r.get(p.supply_current);
        let winding_voltage = r.get(p.winding_voltage);
        let winding_current = r.get(p.winding_current);
        crate::controller_refinement::power::Sample {
            time_s,
            supply_voltage_v: voltage,
            supply_current_a: current,
            winding_voltage_v: winding_voltage,
            winding_current_a: winding_current,
            supply_power_w: voltage * current,
            winding_power_w: winding_voltage * winding_current,
            state_of_charge: p.state_of_charge.map(|id| r.get(id)),
        }
    }
}
pub fn prepare(
    settings: &ModelSettings,
    voltage: f64,
    temperature: f64,
    drive: Drive,
) -> Result<PreparedBench, String> {
    settings.validate()?;
    if let Drive::ReleasedPulse { release, .. } = &drive {
        release.validate()?;
    }
    let is_feedback = matches!(drive, Drive::Feedback { .. } | Drive::Scheduled { .. });
    (|| -> Result<PreparedBench, Box<dyn std::error::Error>> {
        let registry = crate::registry();
        let mut world = sim_core::ModelWorld::default();
        let part = world.part(
            &registry,
            "motor",
            MOTOR_UNIT,
            settings.motor.iter().map(|(k, v)| (k.as_str(), *v)),
        )?;
        let mut bridge_parameters = settings.bridge.clone();
        let gate_end = match &drive {
            Drive::ReleasedPulse {
                start_s,
                duration_s,
                release:
                    DriverRelease::TorqueOff {
                        off_conductance_s,
                        diode_drop_v,
                        diode_resistance_ohm,
                    },
                ..
            } => {
                bridge_parameters.extend([
                    ("off_conductance".into(), *off_conductance_s),
                    ("diode_drop".into(), *diode_drop_v),
                    ("diode_resistance".into(), *diode_resistance_ohm),
                ]);
                Some(start_s + duration_s)
            }
            _ => None,
        };
        let bridge = world.part(
            &registry,
            "bridge",
            if gate_end.is_some() {
                sim_domain_robot::switchable_bridge::SWITCHABLE_H_BRIDGE
            } else {
                H_BRIDGE
            },
            bridge_parameters.iter().map(|(k, v)| (k.as_str(), *v)),
        )?;
        let c = &settings.conditions;
        let supply = if let Some(power) = &settings.power {
            world.part(
                &registry,
                "supply",
                power.source_component.as_str(),
                power
                    .source_parameters
                    .iter()
                    .map(|(k, v)| (k.as_str(), *v)),
            )?
        } else {
            world.part(
                &registry,
                "supply",
                sim_domain_electrical::elements::VOLTAGE_SOURCE,
                [("voltage", voltage)],
            )?
        };
        let electrical = if let Some(power) = &settings.power {
            let voltage = world.part(
                &registry,
                "bus_voltage",
                sim_domain_sensing::VOLTAGE_SENSOR,
                [],
            )?;
            let current = world.part(
                &registry,
                "supply_current",
                sim_domain_sensing::CURRENT_SENSOR,
                [],
            )?;
            let auxiliary = world.part(
                &registry,
                "auxiliary_load",
                sim_domain_electrical::elements::CURRENT_SOURCE,
                [("current", -power.auxiliary_current_a)],
            )?;
            world.connect([supply.port("p"), current.port("p")]);
            world.connect([
                current.port("n"),
                bridge.port("supply_p"),
                voltage.port("p"),
                auxiliary.port("p"),
            ]);

            Some((voltage, current, auxiliary))
        } else {
            world.connect([supply.port("p"), bridge.port("supply_p")]);
            None
        };
        let ground = world.part(
            &registry,
            "ground",
            sim_domain_electrical::elements::GROUND,
            [],
        )?;
        let ambient = world.part(
            &registry,
            "temperature",
            sim_domain_thermal::AMBIENT,
            [("temperature", temperature + 273.15)],
        )?;
        let command = match drive {
            Drive::Pulse {
                duty,
                start_s,
                duration_s,
            }
            | Drive::ReleasedPulse {
                duty,
                start_s,
                duration_s,
                ..
            } => world.part(
                &registry,
                "recorded_pulse",
                sim_domain_control::pulse::PULSE,
                [
                    ("amplitude", duty),
                    ("start", start_s),
                    ("duration", duration_s),
                ],
            )?,
            Drive::Feedback { period_s } => world.part(
                &registry,
                "controller",
                if settings.power.is_some() {
                    sim_domain_control::pwm_feedback::ELECTRICAL_FEEDBACK
                } else {
                    sim_domain_control::pwm_feedback::FEEDBACK
                },
                [("period", period_s)],
            )?,
            Drive::Scheduled { times } => {
                let mut params = std::collections::BTreeMap::new();
                params.insert("count".to_string(), times.len() as f64);
                for (i, t) in times.into_iter().enumerate() {
                    params.insert(format!("time.{i}"), t);
                }
                world.part(
                    &registry,
                    "controller",
                    if settings.power.is_some() {
                        sim_domain_control::pwm_feedback::SCHEDULED_ELECTRICAL_FEEDBACK
                    } else {
                        sim_domain_control::pwm_feedback::SCHEDULED_FEEDBACK
                    },
                    params.iter().map(|(k, v)| (k.as_str(), *v)),
                )?
            }
        };
        let command_port = command.port(if is_feedback { "act.duty" } else { "value" });
        let load = world.part(
            &registry,
            "load",
            sim_domain_rotational::elements::INERTIA,
            [("inertia", c.load_inertia)],
        )?;
        let torque = world.part(
            &registry,
            "load_torque",
            sim_domain_rotational::elements::TORQUE_SOURCE,
            [],
        )?;
        let load_command = world.part(
            &registry,
            "load_command",
            sim_domain_control::elements::CONSTANT,
            [("value", -c.load_torque)],
        )?;
        let angle = world.part(
            &registry,
            "angle",
            sim_domain_rotational::elements::ANGLE_SENSOR,
            [],
        )?;

        let mut negatives = vec![
            supply.port("n"),
            ground.port("pin"),
            bridge.port("supply_n"),
            bridge.port("n"),
            part.port("n"),
        ];
        if let Some((voltage, _, auxiliary)) = &electrical {
            negatives.extend([voltage.port("n"), auxiliary.port("n")]);
        }
        world.connect(negatives);
        world.connect([bridge.port("p"), part.port("p")]);
        world.connect([command_port, bridge.port("command")]);
        if let Some(end) = gate_end {
            let gate = world.part(
                &registry,
                "driver_enable",
                sim_domain_control::pulse::PULSE,
                [("amplitude", 1.), ("start", 0.), ("duration", end)],
            )?;
            world.connect([gate.port("value"), bridge.port("enabled")]);
        }
        world.connect([ambient.port("node"), part.port("winding")]);
        world.connect([
            part.port("shaft"),
            load.port("shaft"),
            angle.port("shaft"),
            torque.port("shaft"),
        ]);
        world.connect([load_command.port("value"), torque.port("torque")]);
        if is_feedback {
            world.connect([angle.port("angle"), command.port("sense.angle")]);
        } else {
            world.connect([angle.port("angle")]);
        }
        if let Some((voltage, current, _)) = &electrical {
            if is_feedback {
                world.connect([
                    voltage.port("voltage"),
                    command.port("sense.supply_voltage"),
                ]);
                world.connect([
                    current.port("current"),
                    command.port("sense.supply_current"),
                ]);
                world.connect([part.port("current"), command.port("sense.winding_current")]);
            } else {
                world.connect([voltage.port("voltage")]);
                world.connect([current.port("current")]);
                world.connect([part.port("current")]);
            }
        } else {
            world.connect([part.port("current")]);
        }
        for s in ["torque", "speed"] {
            world.connect([part.port(s)]);
        }
        let battery = settings
            .power
            .as_ref()
            .is_some_and(|p| p.source_component == sim_domain_robot::motor::BATTERY);
        if battery {
            world.connect([supply.port("soc")]);
        }
        let mut rt = sim_compile::Runtime::new(
            world,
            &registry,
            sim_dynamics::Integrator::BackwardEuler(crate::newton()),
        )?;
        rt.seed(0);
        let angle_id = rt.signal_id(angle.port("angle"));
        let duty_id = rt.signal_id(command_port);
        let power = electrical.map(|(v, i, _)| PowerChannels {
            supply_voltage: rt.signal_id(v.port("voltage")),
            supply_current: rt.signal_id(i.port("current")),
            winding_voltage: rt.across_id(part.port("p")),
            winding_current: rt.signal_id(part.port("current")),
            state_of_charge: battery.then(|| rt.state_id(supply.behavior, "soc")),
        });
        Ok(PreparedBench {
            power,
            runtime: rt,
            angle: angle_id,
            duty: duty_id,
            controller: if is_feedback {
                Some(command.behavior)
            } else {
                None
            },
        })
    })()
    .map_err(|e| e.to_string())
}
