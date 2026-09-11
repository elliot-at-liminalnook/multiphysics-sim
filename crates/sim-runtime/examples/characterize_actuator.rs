//! Configured single-axis dynamometer using the shared compiled multiphysics runtime.
//! No equipment-specific equations or parameters live in this host.
use serde::Deserialize;
use serde_json::{Value, json};
use sim_core::{ModelWorld, StateId};
use sim_domain_control::elements as control;
use sim_domain_electrical::elements as electrical;
use sim_domain_robot::{THERMAL_PROBE, model::Motor, motor::*};
use sim_domain_rotational::elements as rotational;
use sim_domain_thermal as thermal;
use std::{collections::BTreeMap, fs, io::Write};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    voltage_v: f64,
    duration_s: f64,
    step_s: f64,
    sample_s: f64,
    load_inertia_kg_m2: f64,
    load_torque_nm: f64,
    locked: bool,
    thermal: bool,
    ambient_c: f64,
    servo: bool,
    target_rad: f64,
    frequency_hz: f64,
    motor_overrides: BTreeMap<String, f64>,
    firmware_overrides: BTreeMap<String, f64>,
    thermal_resistance_scale: f64,
    encoder_parameters: BTreeMap<String, f64>,
}
#[derive(Deserialize)]
struct Plan {
    motor: Motor,
    cases: Vec<Case>,
    provenance: Value,
}
fn run(
    motor: &Motor,
    c: &Case,
    out: &std::path::Path,
) -> Result<Value, Box<dyn std::error::Error>> {
    if !(c.step_s > 0.0
        && c.sample_s >= c.step_s
        && c.duration_s > 0.0
        && c.thermal_resistance_scale > 0.0)
    {
        return Err("invalid experiment clock/thermal scale".into());
    }
    let registry = sim_runtime::registry();
    let mut world = ModelWorld::default();
    let mut parameters: BTreeMap<String, f64> =
        cad_motor_unit_parameters(motor, 0.0, c.ambient_c + 273.15, true, false)
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect();
    parameters.extend(c.motor_overrides.clone());
    let part = world.part(
        &registry,
        "motor",
        MOTOR_UNIT,
        parameters.iter().map(|(k, v)| (k.as_str(), *v)),
    )?;
    let supply = world.part(
        &registry,
        "supply",
        electrical::VOLTAGE_SOURCE,
        [("voltage", c.voltage_v)],
    )?;
    let ground = world.part(&registry, "ground", electrical::GROUND, [])?;
    let angle = world.part(&registry, "angle", rotational::ANGLE_SENSOR, [])?;
    let speed = world.part(&registry, "speed", rotational::SPEED_SENSOR, [])?;
    let encoder = world.part(
        &registry,
        "encoder",
        sim_domain_sensing::ENCODER,
        c.encoder_parameters.iter().map(|(k, v)| (k.as_str(), *v)),
    )?;
    world.connect([encoder.port("angle")]);
    let load = if c.locked {
        world.part(&registry, "load", rotational::GROUND, [])?
    } else {
        world.part(
            &registry,
            "load",
            rotational::INERTIA,
            [("inertia", c.load_inertia_kg_m2)],
        )?
    };
    let torque = world.part(&registry, "load_torque", rotational::TORQUE_SOURCE, [])?;
    let torque_command = world.part(
        &registry,
        "load_command",
        control::CONSTANT,
        [("value", -c.load_torque_nm)],
    )?;
    world.connect([torque_command.port("value"), torque.port("torque")]);
    world.connect([
        part.port("shaft"),
        angle.port("shaft"),
        speed.port("shaft"),
        encoder.port("shaft"),
        torque.port("shaft"),
        load.port(if c.locked { "flange" } else { "shaft" }),
    ]);
    let mut firmware_parameters = cad_servo_firmware_parameters(motor);
    firmware_parameters.extend(c.firmware_overrides.clone());
    let mut duty_port = None;
    let mut command_port = None;
    if c.servo {
        let bridge = world.part(
            &registry,
            "bridge",
            H_BRIDGE,
            cad_h_bridge_parameters(motor)
                .iter()
                .map(|(k, v)| (k.as_str(), *v)),
        )?;
        let fw = world.part(
            &registry,
            "firmware",
            SERVO_FIRMWARE,
            firmware_parameters.iter().map(|(k, v)| (k.as_str(), *v)),
        )?;
        let command = if c.frequency_hz > 0.0 {
            world.part(
                &registry,
                "target",
                control::SINE,
                [
                    ("amplitude", c.target_rad),
                    ("frequency", c.frequency_hz),
                    ("phase", -std::f64::consts::FRAC_PI_2),
                ],
            )?
        } else {
            world.part(
                &registry,
                "target",
                control::CONSTANT,
                [("value", c.target_rad)],
            )?
        };
        world.connect([supply.port("p"), bridge.port("supply_p")]);
        world.connect([
            supply.port("n"),
            ground.port("pin"),
            part.port("n"),
            bridge.port("supply_n"),
            bridge.port("n"),
        ]);
        world.connect([bridge.port("p"), part.port("p")]);
        world.connect([command.port("value"), fw.port("target")]);
        world.connect([angle.port("angle"), fw.port("measured")]);
        world.connect([speed.port("speed"), fw.port("rate")]);
        world.connect([fw.port("command"), bridge.port("command")]);
        duty_port = Some(fw.port("command"));
        command_port = Some(command.port("value"));
    } else {
        world.connect([supply.port("n"), ground.port("pin"), part.port("n")]);
        world.connect([supply.port("p"), part.port("p")]);
        world.connect([angle.port("angle")]);
        world.connect([speed.port("speed")]);
    }
    for s in ["current", "torque", "speed"] {
        world.connect([part.port(s)]);
    }
    let ambient = world.part(
        &registry,
        "ambient",
        thermal::AMBIENT,
        [("temperature", c.ambient_c + 273.15)],
    )?;
    let winding = world.part(&registry, "winding_probe", THERMAL_PROBE, [])?;
    let case = world.part(&registry, "case_probe", THERMAL_PROBE, [])?;
    if c.thermal {
        let t = &motor.thermal;
        let cw = world.part(
            &registry,
            "winding_capacity",
            thermal::CAPACITANCE,
            [
                ("heat_capacity", t.winding_heat_capacity),
                ("initial.temperature", c.ambient_c + 273.15),
            ],
        )?;
        let cc = world.part(
            &registry,
            "case_capacity",
            thermal::CAPACITANCE,
            [
                ("heat_capacity", t.case_heat_capacity),
                ("initial.temperature", c.ambient_c + 273.15),
            ],
        )?;
        let wc = world.part(
            &registry,
            "winding_case",
            thermal::CONDUCTANCE,
            [("resistance", t.r_winding_case * c.thermal_resistance_scale)],
        )?;
        let ca = world.part(
            &registry,
            "case_ambient",
            thermal::CONDUCTANCE,
            [("resistance", t.r_case_ambient * c.thermal_resistance_scale)],
        )?;
        let cm = world.part(
            &registry,
            "case_mount",
            thermal::CONDUCTANCE,
            [("resistance", t.r_case_mount * c.thermal_resistance_scale)],
        )?;
        world.connect([
            part.port("winding"),
            winding.port("node"),
            cw.port("node"),
            wc.port("a"),
        ]);
        world.connect([
            wc.port("b"),
            cc.port("node"),
            case.port("node"),
            ca.port("a"),
            cm.port("a"),
        ]);
        world.connect([ca.port("b"), cm.port("b"), ambient.port("node")]);
    } else {
        world.connect([
            part.port("winding"),
            winding.port("node"),
            case.port("node"),
            ambient.port("node"),
        ]);
    }
    world.connect([winding.port("temperature")]);
    world.connect([case.port("temperature")]);
    let mut rt = sim_compile::Runtime::new(
        world,
        &registry,
        sim_dynamics::Integrator::BackwardEuler(sim_runtime::newton()),
    )?;
    let ids: Vec<StateId> = [
        angle.port("angle"),
        speed.port("speed"),
        part.port("current"),
        part.port("torque"),
        winding.port("temperature"),
        case.port("temperature"),
    ]
    .into_iter()
    .map(|p| rt.signal_id(p))
    .collect();
    let vp = rt.across_id(part.port("p"));
    let target = command_port.map(|p| rt.signal_id(p));
    let duty = duty_port.map(|p| rt.signal_id(p));
    let encoder_id = rt.signal_id(encoder.port("angle"));
    let mut csv = fs::File::create(out.join(format!("{}.csv", c.name)))?;
    writeln!(
        csv,
        "time_s,target_rad,angle_rad,speed_rad_s,current_a,torque_nm,winding_c,case_c,motor_voltage_v,duty,encoder_rad"
    )?;
    let mut rows = Vec::<[f64; 11]>::new();
    let mut error = None;
    let mut first_110 = None;
    let samples = (c.duration_s / c.sample_s).round() as usize;
    for n in 1..=samples {
        if let Err(e) = rt.advance(c.sample_s, c.step_s) {
            error = Some(e.to_string());
            break;
        }
        let v: Vec<f64> = ids.iter().map(|id| rt.get(*id)).collect();
        let row = [
            n as f64 * c.sample_s,
            target.map(|id| rt.get(id)).unwrap_or(0.),
            v[0],
            v[1],
            v[2],
            v[3],
            v[4] - 273.15,
            v[5] - 273.15,
            rt.get(vp),
            duty.map(|id| rt.get(id)).unwrap_or(1.),
            rt.get(encoder_id),
        ];
        if row.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite measurement".into());
        }
        if row[6] >= 110. && first_110.is_none() {
            first_110 = Some(row[0]);
        }
        writeln!(
            csv,
            "{}",
            row.iter()
                .map(|x| format!("{x:.10}"))
                .collect::<Vec<_>>()
                .join(",")
        )?;
        rows.push(row);
    }
    if rows.is_empty() {
        return Err(format!("{}: {:?}", c.name, error).into());
    }
    let tail = &rows[rows.len() * 3 / 4..];
    let mean = |k: usize| tail.iter().map(|r| r[k]).sum::<f64>() / tail.len() as f64;
    let max_i = rows.iter().map(|r| r[4].abs()).fold(0., f64::max);
    let rms = (tail.iter().map(|r| (r[2] - r[1]).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
    let threshold = 0.3_f64.to_radians();
    let settling = if c.servo && c.frequency_hz == 0. {
        let last = rows.iter().rposition(|r| (r[2] - r[1]).abs() > threshold);
        match last {
            None => Some(c.sample_s),
            Some(i) if i + 1 < rows.len() => Some(rows[i + 1][0]),
            _ => None,
        }
    } else {
        None
    };
    let mut gain = None;
    let mut phase = None;
    if c.frequency_hz > 0. {
        let fit = &rows[rows.len() / 2..];
        let w = std::f64::consts::TAU * c.frequency_hz;
        let sin = 2. * fit.iter().map(|r| r[2] * (w * r[0]).sin()).sum::<f64>() / fit.len() as f64;
        let cos = 2. * fit.iter().map(|r| r[2] * (w * r[0]).cos()).sum::<f64>() / fit.len() as f64;
        gain = Some(sin.hypot(cos) / c.target_rad);
        phase = Some(cos.atan2(sin).to_degrees());
    }
    Ok(
        json!({"name":c.name,"error":error,"completed_s":rows.last().unwrap()[0],"samples":rows.len(),"mean_tail_speed_rad_s":mean(3),"mean_tail_current_a":mean(4),"mean_tail_torque_nm":mean(5),"final_angle_rad":rows.last().unwrap()[2],"peak_sampled_current_a":max_i,"peak_sampled_speed_rad_s":rows.iter().map(|r|r[3].abs()).fold(0.,f64::max),"peak_sampled_angle_rad":rows.iter().map(|r|r[2]).fold(f64::NEG_INFINITY,f64::max),"tracking_rms_tail_rad":rms,"settling_within_0_3_deg_s":settling,"gain":gain,"phase_deg":phase,"final_winding_c":rows.last().unwrap()[6],"final_case_c":rows.last().unwrap()[7],"first_sample_above_110c_s":first_110,"mean_tail_motor_input_w":tail.iter().map(|r|r[8]*r[4]).sum::<f64>()/tail.len()as f64,"mean_tail_output_w":tail.iter().map(|r|r[3]*r[5]).sum::<f64>()/tail.len()as f64,"parameters":parameters,"firmware_parameters":if c.servo{Some(firmware_parameters)}else{None}}),
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().skip(1).collect();
    if a.len() != 2 {
        return Err("usage: characterize_actuator plan.json NEW-output-directory".into());
    }
    let bytes = fs::read(&a[0])?;
    let plan: Plan = serde_json::from_slice(&bytes)?;
    let out = std::path::Path::new(&a[1]);
    fs::create_dir(out)?;
    fs::write(out.join("plan.json"), &bytes)?;
    let mut results = vec![];
    for c in &plan.cases {
        eprintln!("{}", c.name);
        match run(&plan.motor, c, out) {
            Ok(r) => results.push(r),
            Err(e) => results.push(json!({"name":c.name,"error":e.to_string()})),
        };
        fs::write(
            out.join("results.json"),
            serde_json::to_vec_pretty(
                &json!({"runtime_identity":sim_runtime::physics_context::RuntimeIdentity::current(),"experiment_source_blake3":blake3::hash(include_bytes!("characterize_actuator.rs")).to_hex().to_string(),"plan_blake3":blake3::hash(&bytes).to_hex().to_string(),"provenance":plan.provenance,"cases":results}),
            )?,
        )?;
    }
    if results.iter().any(|r| !r["error"].is_null()) {
        return Err("one or more cases failed; results preserved".into());
    }
    Ok(())
}
