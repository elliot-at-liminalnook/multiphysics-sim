//! Motors of the physical model: RoboCAD's `MOTOR_DATASHEETS` and
//! `motor_physics` (robotics.py) and `motor_block` (physical.py). Constants
//! not on a datasheet are derived from stall torque, no-load speed and
//! voltage (ke = V/ω_motor,no-load, kt = ke, R = kt·V/τ_motor,stall); a
//! declared stall current sets kt = τ_motor,stall/I_stall and, without a
//! resistance, R = V/I_stall. These are estimates, not calibration: each
//! motor's block says so in `notes` and in the export's assumptions.
use crate::robotics::MotorSpec;
use serde_json::{Value, json};
use std::f64::consts::{PI, TAU};

/// A library motor's datasheet entry (absent fields are derived).
#[derive(Default, Clone, Copy)]
pub struct Datasheet {
    pub resistance: Option<f64>,
    pub stall_current: Option<f64>,
    pub no_load_current: Option<f64>,
    pub internal_ratio: Option<f64>,
    pub efficiency: Option<f64>,
    pub backlash: Option<f64>,
    pub firmware: Option<&'static str>,
    pub loop_hz: Option<f64>,
    pub deadband: Option<f64>,
    pub resolution: Option<f64>,
    pub max_c: Option<f64>,
    pub driver: Option<&'static str>,
    pub gear_stiffness: Option<f64>,
    pub notes: &'static str,
}

/// RoboCAD's `MOTOR_DATASHEETS`.
pub fn datasheet(id: &str) -> Option<Datasheet> {
    let d = Datasheet::default();
    let r = |deg: f64| deg.to_radians();
    #[allow(clippy::too_many_arguments)]
    fn full(resistance: f64, stall: f64, no_load: f64, ratio: f64, eta: f64, backlash: f64, firmware: &'static str, loop_hz: f64, deadband: f64, resolution: f64, max_c: f64, driver: &'static str, gear_stiffness: f64, notes: &'static str) -> Datasheet {
        Datasheet { resistance: Some(resistance), stall_current: Some(stall), no_load_current: Some(no_load), internal_ratio: Some(ratio), efficiency: Some(eta), backlash: Some(backlash), firmware: Some(firmware), loop_hz: Some(loop_hz), deadband: Some(deadband), resolution: Some(resolution), max_c: Some(max_c), driver: Some(driver), gear_stiffness: Some(gear_stiffness), notes }
    }
    Some(match id {
        "hx30hm" => Datasheet { stall_current: Some(3.0), no_load_current: Some(0.1), internal_ratio: Some(1.0), efficiency: Some(1.0), firmware: Some("servo"), resolution: Some(TAU / 4096.), driver: Some("servo_internal"), notes: "HX-30HM manufacturer mass, output stall torque/speed, current and encoder resolution. Output-equivalent actuator (internal gearing is already reflected). Electrical constants, servo gains/latency, rotor inertia, thermal parameters, friction and backlash are estimates pending identification. 0.3 degree specified accuracy is distinct from encoder resolution. https://www.hiwonder.com/products/hx-30hm", ..d },
        "sg90" => full(7.4, 0.65, 0.1, 262.0, 0.55, r(1.5), "servo", 250.0, r(0.9), PI / 1024., 90.0, "servo_internal", 8.0, "R and currents from stall/no-load datasheet; nylon gears"),
        "mg90s" => full(6.0, 0.8, 0.12, 262.0, 0.6, r(1.0), "servo", 250.0, r(0.8), PI / 1024., 100.0, "servo_internal", 15.0, "metal gears; R from stall current"),
        "mg996r" => full(2.4, 2.5, 0.17, 260.0, 0.6, r(1.0), "servo", 300.0, r(0.6), PI / 1024., 100.0, "servo_internal", 60.0, "stall 2.5 A at 6 V"),
        "ds3218" => full(2.2, 3.0, 0.2, 300.0, 0.62, r(0.8), "servo", 300.0, r(0.5), r(270.0) / 4096., 100.0, "servo_internal", 120.0, "digital 270° servo, 12-bit position"),
        "n20_100" => full(8.0, 0.75, 0.07, 100.0, 0.7, r(2.0), "position", 1000.0, 0.0, TAU / (12. * 100.), 110.0, "h_bridge", 5.0, "6 V micro metal gearmotor; 12 CPR magnetic encoder assumed"),
        "n20_298" => full(8.0, 0.75, 0.07, 298.0, 0.65, r(2.5), "position", 1000.0, 0.0, TAU / (12. * 298.), 110.0, "h_bridge", 5.0, "6 V micro metal gearmotor"),
        "ga25_150" => full(4.0, 3.0, 0.15, 150.0, 0.7, r(1.5), "position", 1000.0, 0.0, TAU / (11. * 4. * 150.), 120.0, "h_bridge", 60.0, "12 V, 11 PPR quadrature encoder"),
        "gb37_100" => full(2.4, 5.0, 0.3, 100.0, 0.75, r(1.2), "position", 1000.0, 0.0, TAU / (11. * 4. * 100.), 120.0, "h_bridge", 200.0, "12 V, 11 PPR encoder"),
        "gm2804" => full(5.6, 2.1, 0.05, 1.0, 0.95, 0.0, "torque", 8000.0, 0.0, TAU / 16384., 120.0, "esc", 1e6, "gimbal FOC, 14-bit magnetic encoder"),
        "d5065" => full(0.039, 40.0, 0.5, 1.0, 0.95, 0.0, "torque", 8000.0, 0.0, TAU / 8192., 150.0, "esc", 1e6, "270 KV, 24 V; ODrive class driver"),
        "cycloid_8108" => full(0.13, 35.0, 0.6, 9.0, 0.85, r(0.3), "torque", 8000.0, 0.0, TAU / 16384., 150.0, "esc", 3000.0, "quasi-direct-drive; planetary backlash 0.3°"),
        "nema14" => full(5.6, 0.8, 0.8, 1.0, 0.9, 0.0, "stepper", 20000.0, 0.0, r(1.8) / 16., 80.0, "stepper", 1e6, "0.8 A/phase; holding torque as stall; runs at full current"),
        "nema17" => full(1.5, 1.7, 1.7, 1.0, 0.9, 0.0, "stepper", 20000.0, 0.0, r(1.8) / 16., 80.0, "stepper", 1e6, "1.7 A/phase, 12–24 V chopper"),
        "nema17_pancake" => full(3.0, 1.0, 1.0, 1.0, 0.9, 0.0, "stepper", 20000.0, 0.0, r(1.8) / 16., 80.0, "stepper", 1e6, "1 A/phase pancake"),
        "nema23" => full(0.9, 2.8, 2.8, 1.0, 0.9, 0.0, "stepper", 20000.0, 0.0, r(1.8) / 16., 80.0, "stepper", 1e6, "2.8 A/phase"),
        "linear_l12" => full(12.0, 0.5, 0.06, 100.0, 0.4, 0.3e-3, "position", 100.0, 0.2e-3, 0.05e-3, 90.0, "servo_internal", 2e4, "50 mm stroke, 100:1; backlash and resolution in metres"),
        _ => return None,
    })
}

/// RoboCAD's `motor_physics(spec, gear_extra)`: the electrical, gearbox,
/// thermal, firmware and driver blocks (SI) and `notes`.
pub fn physics(spec: &MotorSpec, gear_extra: f64) -> Value {
    let d = datasheet(&spec.id).unwrap_or_default();
    let ratio_int = d.internal_ratio.unwrap_or(if spec.gear_ratio > 1.0 { spec.gear_ratio } else { 1.0 });
    let eta = d.efficiency.unwrap_or(if ratio_int > 1.0 { 0.7 } else { 0.95 });
    let v = spec.voltage;
    let (tau_out, w_out) = if spec.kind == "linear" {
        // stall_torque is a force (N) and no_load_speed a velocity (m/s): a lead of 1 mm/rev is assumed.
        let lead = 1.0e-3;
        (spec.stall_torque * lead / TAU, spec.no_load_speed * TAU / lead)
    } else {
        (spec.stall_torque, spec.no_load_speed)
    };
    let tau_m = tau_out / (ratio_int * eta).max(1e-9);
    let w_m = w_out * ratio_int;
    let ke = v / w_m.max(1e-9);
    let mut kt = ke;
    let mut r = d.resistance.unwrap_or(kt * v / tau_m.max(1e-12));
    let mut i_stall = d.stall_current.unwrap_or(v / r);
    if d.resistance.is_some() && d.stall_current.is_none() {
        i_stall = v / r;
    }
    if let Some(stall) = d.stall_current {
        i_stall = stall;
        kt = tau_m / i_stall.max(1e-9);
        if d.resistance.is_none() {
            r = v / i_stall.max(1e-9);
        }
    }
    let l = r * if matches!(spec.kind.as_str(), "servo" | "dc_gearmotor" | "linear") { 0.4e-3 } else { 1.2e-3 };
    let rotor_inertia = if spec.rotor_inertia > 0.0 { spec.rotor_inertia } else { 1.2e-9 * (spec.mass_g / 9.0).powf(5.0 / 3.0) };
    let out_inertia = rotor_inertia * (ratio_int * gear_extra).powi(2);
    let firmware = d.firmware.unwrap_or("position");
    let loop_hz = d.loop_hz.unwrap_or(1000.0);
    // Position gains: full voltage at 5° error for servos, a 20 Hz loop bandwidth otherwise.
    let (kp, kd, ki) = match firmware {
        "servo" => (v / 5f64.to_radians(), v / 5f64.to_radians() * 0.02, 0.0),
        "position" => (v / 10f64.to_radians(), v / 10f64.to_radians() * 0.03, v / 10f64.to_radians() * 0.5),
        _ => (0.0, 0.0, 0.0),
    };
    let copper = 0.12 * spec.mass_g * 1e-3;
    let case = (spec.mass_g * 1e-3 - copper).max(1e-3);
    let small = spec.mass_g < 100.0;
    json!({
        "electrical": {"resistance": r, "inductance": l, "torque_constant": kt, "back_emf_constant": ke, "no_load_current": d.no_load_current.unwrap_or(0.1 * i_stall), "rotor_inertia": rotor_inertia, "supply_voltage": v, "current_limit": i_stall, "stall_current": i_stall, "poles": if spec.kind == "bldc" { 14 } else { 0 }},
        "gearbox": {"ratio": ratio_int * gear_extra, "efficiency": eta, "backlash_rad": d.backlash.unwrap_or(if ratio_int > 1.0 { 1f64.to_radians() } else { 0.0 }), "inertia": out_inertia, "stiffness": d.gear_stiffness.unwrap_or(50.0) * gear_extra * gear_extra, "max_output_torque": tau_out * gear_extra, "max_output_speed": w_out / gear_extra.max(1e-9)},
        "thermal": {"winding_heat_capacity": copper * 385.0, "case_heat_capacity": case * if spec.kind == "servo" { 900.0 } else { 470.0 }, "r_winding_case": if small { 12.0 } else { 4.0 }, "r_case_mount": if small { 6.0 } else { 2.5 }, "r_case_ambient": if small { 45.0 } else { 15.0 }, "resistance_temp_coeff": 0.0039, "torque_derating_per_c": 0.0012, "max_winding_c": d.max_c.unwrap_or(110.0), "ambient_c": 25.0},
        "firmware": {"kind": firmware, "loop_rate_hz": loop_hz, "latency_s": 1.0 / loop_hz, "deadband_rad": d.deadband.unwrap_or(0.0), "sensor_resolution_rad": d.resolution.unwrap_or(TAU / 4096.), "kp": kp, "ki": ki, "kd": kd, "output": if matches!(firmware, "torque" | "stepper") { "current" } else { "voltage" }},
        "driver": {"kind": d.driver.unwrap_or("h_bridge"), "pwm_hz": if firmware == "servo" { 50.0 } else { 20000.0 }, "on_resistance": if small { 0.05 } else { 0.01 }, "current_limit": i_stall},
        "notes": if d.notes.is_empty() { "no datasheet entry: R, kt and ke derived from stall torque, no-load speed and voltage" } else { d.notes },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MG996R at the output: 1 N·m stall over 260:1 at 60 % → τ_m; the declared
    /// 2.5 A stall current sets kt, the declared 2.4 Ω stays.
    #[test]
    fn a_datasheet_motor_uses_its_declared_current_and_resistance() {
        let spec = crate::robotics::motor("mg996r").unwrap();
        let p = physics(&spec, 1.0);
        let tau_m = 1.0 / (260.0 * 0.6);
        assert!((p["electrical"]["torque_constant"].as_f64().unwrap() - tau_m / 2.5).abs() < 1e-12);
        assert_eq!(p["electrical"]["resistance"], json!(2.4));
        assert_eq!(p["gearbox"]["ratio"], json!(260.0));
        assert_eq!(p["firmware"]["kind"], json!("servo"));
        assert_eq!(p["thermal"]["max_winding_c"], json!(100.0));
        // An external 2:1 reduction doubles the output torque and halves the speed.
        let q = physics(&spec, 2.0);
        assert!((q["gearbox"]["max_output_torque"].as_f64().unwrap() - 2.0).abs() < 1e-12);
        assert_eq!(q["gearbox"]["ratio"], json!(520.0));
    }
}
