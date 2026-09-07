//! Steady locked-shaft audit using registered motor and driver equations.
//! This checks parameter consistency; it does not calibrate a real actuator.
use serde::Serialize;
use sim_core::{BehaviorRegistry, Context};
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct StallOperatingPoint {
    pub current_a: f64,
    pub motor_voltage_v: f64,
    pub shaft_torque_nm: f64,
    pub gearbox_deflection_rad: f64,
    pub winding_heat_w: f64,
    pub maximum_equation_residual: f64,
}

pub fn stall_operating_point(
    motor_parameters: &BTreeMap<String, f64>,
    driver_parameters: &BTreeMap<String, f64>,
    supply_voltage_v: f64,
    winding_temperature_k: f64,
    duty: f64,
) -> Result<StallOperatingPoint, String> {
    if !supply_voltage_v.is_finite()
        || supply_voltage_v < 0.0
        || !winding_temperature_k.is_finite()
        || winding_temperature_k <= 0.0
        || !duty.is_finite()
        || duty.abs() > 1.0
    {
        return Err("finite physical stall boundaries required".into());
    }
    let mut registry = BehaviorRegistry::default();
    crate::motor::register(&mut registry).map_err(|e| e.to_string())?;
    let make = |kind: &str, params: &BTreeMap<String, f64>| {
        let d = registry.get(&kind.into()).map_err(|e| e.to_string())?;
        d.validate_parameters(params).map_err(|e| e.to_string())?;
        d.equations.ok_or("missing actuator equations")?(params).map_err(|e| e.to_string())
    };
    let motor = make(crate::MOTOR_UNIT, motor_parameters)?;
    let driver = make(crate::H_BRIDGE, driver_parameters)?;
    if motor.states().len() != 3 || driver.states().len() != 1 {
        return Err("steady stall audit requires continuous motor backlash, not event mode".into());
    }
    let evaluate = |x: &[f64]| {
        let mut mr = [0.0; 3];
        let mut through = [0.0; 5];
        let mut signals = [0.0; 3];
        motor.residual(&mut Context::new(
            0.0,
            &[x[0], 0.0, x[1]],
            &[0.0; 3],
            &[0, 1, 2, 4, 5],
            &[None, None, Some(3), None, None],
            &[x[2], 0.0, 0.0, 0.0, winding_temperature_k],
            &[0.0; 5],
            &[],
            &mut mr,
            &mut through,
            &mut signals,
        ));
        let mut dr = [0.0];
        let mut dt = [0.0; 4];
        driver.residual(&mut Context::new(
            0.0,
            &[-x[0]],
            &[0.0],
            &[0, 1, 2, 3, 4],
            &[None; 4],
            &[supply_voltage_v, 0.0, x[2], 0.0],
            &[0.0; 4],
            &[duty],
            &mut dr,
            &mut dt,
            &mut [],
        ));
        ([mr[0], mr[1], dr[0]], signals[1], -through[4])
    };
    let mut x = [0.0, 0.0, supply_voltage_v * duty];
    sim_solve::solve_newton(
        &mut x,
        sim_solve::NewtonConfig {
            max_iterations: 60,
            ..Default::default()
        },
        |x, r| r.copy_from_slice(&evaluate(x).0),
    )
    .map_err(|e| e.to_string())?;
    let (residual, torque, heat) = evaluate(&x);
    let maximum = residual.iter().map(|r| r.abs()).fold(0.0_f64, f64::max);
    if !maximum.is_finite() || maximum > 1e-8 || !torque.is_finite() || !heat.is_finite() {
        return Err("stall audit residual or output is invalid".into());
    }
    Ok(StallOperatingPoint {
        current_a: x[0],
        motor_voltage_v: x[2],
        shaft_torque_nm: torque,
        gearbox_deflection_rad: x[1],
        winding_heat_w: heat,
        maximum_equation_residual: maximum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stall_matches_independent_electrical_and_torque_balance() {
        let motor = [
            ("resistance", 2.0),
            ("torque_constant", 0.1),
            ("ratio", 5.0),
            ("efficiency", 1.0),
            ("gear_stiffness", 100.0),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();
        let driver = [("on_resistance", 0.5), ("current_limit", 10.0)]
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect();
        for direction in [-1.0, 1.0] {
            let result = stall_operating_point(&motor, &driver, 6.0, 293.15, direction).unwrap();
            assert!((result.current_a - direction * 6.0 / 2.5).abs() < 1e-9);
            assert!((result.shaft_torque_nm - direction * 1.2).abs() < 1e-9);
            assert!((result.winding_heat_w - 11.52).abs() < 1e-8);
        }
        assert!(stall_operating_point(&motor, &driver, 6.0, 0.0, 1.0).is_err());
        assert!(stall_operating_point(&motor, &driver, 6.0, 293.15, 2.0).is_err());
    }

    #[test]
    fn stall_includes_temperature_and_driver_foldback() {
        let motor = [
            ("resistance", 2.0),
            ("torque_constant", 0.1),
            ("ratio", 1.0),
            ("efficiency", 1.0),
            ("reference", 293.15),
            ("temp_coeff", 0.004),
            ("derating", 0.001),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();
        let driver = [("on_resistance", 0.5), ("current_limit", 1.0)]
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect();
        // At +50 K, R=2.4 ohm and Kt=0.095 N m/A. The registered driver
        // folds voltage back with 20 V/A above its limit; it is not a hard cap.
        let expected_current = (6.0 + 20.0) / (2.4 + 0.5 + 20.0);
        for direction in [-1.0, 1.0] {
            let result = stall_operating_point(&motor, &driver, 6.0, 343.15, direction).unwrap();
            assert!((result.current_a - direction * expected_current).abs() < 1e-9);
            assert!((result.shaft_torque_nm - direction * expected_current * 0.095).abs() < 1e-9);
            assert!((result.winding_heat_w - 2.4 * expected_current.powi(2)).abs() < 1e-8);
        }
    }
}
