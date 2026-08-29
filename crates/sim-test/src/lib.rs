//! Reusable, headless acceptance scenarios for the actuator slice.

use serde::{Deserialize, Serialize};
use sim_runtime::{ActuatorConfig, ActuatorSimulation, RunSummary, RuntimeError, Sample};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScenarioName {
    Extend,
    Reverse,
    Load,
    Brownout,
    Obstruction,
    Release,
    Coast,
}

impl ScenarioName {
    pub const ALL: [Self; 7] = [
        Self::Extend,
        Self::Reverse,
        Self::Load,
        Self::Brownout,
        Self::Obstruction,
        Self::Release,
        Self::Coast,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Extend => "extend",
            Self::Reverse => "reverse",
            Self::Load => "load",
            Self::Brownout => "brownout",
            Self::Obstruction => "obstruction",
            Self::Release => "release",
            Self::Coast => "coast",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub observed: f64,
    pub expectation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub scenario: ScenarioName,
    pub summary: RunSummary,
    pub final_sample: Sample,
    pub checks: Vec<Check>,
    pub trace: Vec<Sample>,
}

impl ScenarioResult {
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }
}

pub fn run(scenario: ScenarioName) -> Result<ScenarioResult, RuntimeError> {
    let mut simulation = ActuatorSimulation::new(ActuatorConfig::default())?;
    let mut checks = Vec::new();

    match scenario {
        ScenarioName::Extend => {
            simulation.inputs.target_position = 0.100;
            simulation.run_for(4.5)?;
            let final_sample = simulation.sample()?;
            close(
                &mut checks,
                "position settles",
                final_sample.position,
                0.100,
                0.004,
            );
            below(
                &mut checks,
                "settled speed",
                final_sample.velocity.abs(),
                0.004,
            );
        }
        ScenarioName::Reverse => {
            simulation.inputs.target_position = 0.100;
            simulation.run_for(3.5)?;
            simulation.inputs.target_position = 0.020;
            simulation.run_for(3.5)?;
            let final_sample = simulation.sample()?;
            close(
                &mut checks,
                "reverse target",
                final_sample.position,
                0.020,
                0.004,
            );
            let saw_reverse = simulation
                .trace()
                .iter()
                .any(|sample| sample.velocity < -0.005);
            boolean(&mut checks, "motion reverses", saw_reverse);
        }
        ScenarioName::Load => {
            simulation.inputs.target_position = 0.100;
            simulation.run_for(3.0)?;
            let unloaded_current = simulation.sample()?.current.abs();
            simulation.inputs.target_position = 0.120;
            simulation.inputs.external_force = -400.0;
            simulation.run_for(2.5)?;
            let final_sample = simulation.sample()?;
            close(
                &mut checks,
                "holds under load",
                final_sample.position,
                0.120,
                0.006,
            );
            above(
                &mut checks,
                "load raises current",
                final_sample.current.abs() - unloaded_current,
                0.25,
            );
        }
        ScenarioName::Brownout => {
            simulation.inputs.target_position = 0.120;
            simulation.inputs.supply_voltage = 12.0;
            simulation.run_for(8.0)?;
            let final_sample = simulation.sample()?;
            close(
                &mut checks,
                "bus reflects brownout",
                final_sample.bus_voltage,
                12.0,
                1.0e-12,
            );
            close(
                &mut checks,
                "eventually reaches target",
                final_sample.position,
                0.120,
                0.006,
            );
        }
        ScenarioName::Obstruction => {
            simulation.inputs.target_position = 0.120;
            simulation.inputs.obstruction_position = Some(0.080);
            simulation.run_for(4.0)?;
            let final_sample = simulation.sample()?;
            close(
                &mut checks,
                "stops at obstruction",
                final_sample.position,
                0.080,
                0.004,
            );
            above(
                &mut checks,
                "current limit activates",
                simulation.summary()?.current_limit_activations as f64,
                1.0,
            );
            below(
                &mut checks,
                "anti-windup bounds integral",
                final_sample.controller_integral.abs(),
                1.05,
            );
            above(
                &mut checks,
                "chassis carries reaction",
                final_sample.chassis_reaction_force.abs(),
                10.0,
            );
        }
        ScenarioName::Release => {
            simulation.inputs.target_position = 0.120;
            simulation.inputs.obstruction_position = Some(0.080);
            simulation.run_for(2.8)?;
            simulation.inputs.obstruction_position = None;
            simulation.run_for(3.5)?;
            let final_sample = simulation.sample()?;
            close(
                &mut checks,
                "reaches target after release",
                final_sample.position,
                0.120,
                0.006,
            );
            let peak_after_release = simulation
                .trace()
                .iter()
                .filter(|sample| sample.time > 2.8)
                .map(|sample| sample.position)
                .fold(f64::NEG_INFINITY, f64::max);
            below(
                &mut checks,
                "release overshoot",
                peak_after_release - 0.120,
                0.025,
            );
        }
        ScenarioName::Coast => {
            simulation.inputs.target_position = 0.140;
            simulation.run_for(1.5)?;
            let speed_before = simulation.sample()?.motor_speed.abs();
            simulation.inputs.controller_enabled = false;
            simulation.run_for(1.0)?;
            let final_sample = simulation.sample()?;
            below(
                &mut checks,
                "disabled drive loses speed",
                final_sample.motor_speed.abs(),
                speed_before * 0.2,
            );
            above(&mut checks, "started while moving", speed_before, 100.0);
            below(
                &mut checks,
                "disabled duty is zero",
                final_sample.duty.abs(),
                1.0e-12,
            );
        }
    }

    let summary = simulation.summary()?;
    below(
        &mut checks,
        "Newton iteration ceiling",
        summary.max_newton_iterations as f64,
        12.0,
    );
    below(
        &mut checks,
        "current bounded",
        summary.peak_current,
        simulation.config.driver.current_limit + 0.5,
    );
    below(
        &mut checks,
        "cumulative energy closure",
        summary.cumulative_energy_error.abs(),
        3.0e-3,
    );
    let final_sample = simulation.sample()?;
    Ok(ScenarioResult {
        scenario,
        summary,
        final_sample,
        checks,
        trace: simulation.trace().to_vec(),
    })
}

fn close(checks: &mut Vec<Check>, name: &str, observed: f64, expected: f64, tolerance: f64) {
    checks.push(Check {
        name: name.to_owned(),
        passed: (observed - expected).abs() <= tolerance,
        observed,
        expectation: format!("within {tolerance} of {expected}"),
    });
}

fn below(checks: &mut Vec<Check>, name: &str, observed: f64, maximum: f64) {
    checks.push(Check {
        name: name.to_owned(),
        passed: observed <= maximum,
        observed,
        expectation: format!("≤ {maximum}"),
    });
}

fn above(checks: &mut Vec<Check>, name: &str, observed: f64, minimum: f64) {
    checks.push(Check {
        name: name.to_owned(),
        passed: observed >= minimum,
        observed,
        expectation: format!("≥ {minimum}"),
    });
}

fn boolean(checks: &mut Vec<Check>, name: &str, passed: bool) {
    checks.push(Check {
        name: name.to_owned(),
        passed,
        observed: if passed { 1.0 } else { 0.0 },
        expectation: "true".to_owned(),
    });
}
