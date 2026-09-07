use serde_json::json;
use sim_domain_robot::{
    actuator_audit::stall_operating_point,
    motor::{cad_h_bridge_parameters, cad_motor_unit_parameters},
};
use sim_runtime::session::Scene;
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 {
        return Err("usage: audit_actuators scene.json".into());
    }
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let scene: Scene = serde_json::from_value(document.clone()).map_err(|e| e.to_string())?;
    let mut rows = vec![];
    for (index, motor) in scene.robot.motors.iter().enumerate() {
        let temp = 293.15;
        let parameters = cad_motor_unit_parameters(motor, 0.0, temp, false, false)
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect();
        let driver = cad_h_bridge_parameters(motor);
        let stall = stall_operating_point(
            &parameters,
            &driver,
            motor.electrical.supply_voltage,
            temp,
            1.0,
        )?;
        rows.push(json!({"name":motor.name,"parameters":parameters,"driver":driver,
            "supply_voltage_v":motor.electrical.supply_voltage,"winding_temperature_k":temp,
            // Optional CAD catalog metadata, not a runtime current-limit substitute.
            "declared_stall_current_a":document["robot"]["motors"][index]["electrical"]["stall_current"],
            "declared_max_output_torque_nm":motor.gearbox.max_output_torque,
            "ideal_ohmic_stall_current_a":motor.electrical.supply_voltage/motor.electrical.resistance,
            "registered_locked_shaft":stall}));
    }
    println!("{}",serde_json::to_string_pretty(&json!({"source":scene.robot.source,"actuators":rows,
        "scope":"Locked actuator shaft, full duty at nominal supply and fixed reference temperature; registered continuous motor/driver equations, no joint friction, battery sag, thermal evolution or separate CAD transmissions. Declared ratings are comparison metadata, not calibrated acceptance gates. No CAD values changed."})).map_err(|e|e.to_string())?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
