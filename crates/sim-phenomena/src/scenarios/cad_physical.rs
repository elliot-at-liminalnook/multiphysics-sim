//! A robot from the CAD tool's physical description (`simrobot` v3): one
//! ModelWorld with the articulated body, and per motor the chain battery →
//! driver → motor unit → joint, the winding's heat through case and mount
//! to ambient (the mount temperature softening the printed link), servo
//! firmware closing the loop from an encoder, plus sensors, cables and the
//! `control.external` seam that hands targets in. Results (peaks, margins,
//! hotspots, traces) are accumulated while it runs and written beside the
//! model for the CAD tool to read; Monte Carlo over the uncertainty block
//! and identification against a logged run live here too.

use crate::world::{newton, registry};
use serde_json::{json, Value};
use sim_compile::Runtime;
use sim_core::{BehaviorId, BehaviorRegistry, FnCoupler, Instance, ModelWorld, PortId, StateId};
use sim_domain_control::external::EXTERNAL;
use sim_domain_robot::articulated::ContactPoint;
use sim_domain_robot::math::{M, V};
use sim_domain_robot::model::{Motor, PhysicalModel};
use sim_domain_robot::sdf::Rng;
use sim_domain_robot::{register_model, Articulated, Generalized, Options, ARTICULATED, BATTERY, H_BRIDGE, MOTOR_UNIT, SERVO_FIRMWARE, THERMAL_PROBE};
use sim_domain_sensing as sense;
use sim_dynamics::Integrator;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[derive(Clone, Debug)]
pub struct BuildOptions {
    pub planar: bool,
    pub flex: bool,
    pub contact: bool,
    /// Fixed integration step (s).
    pub step: f64,
    /// Results sampling interval (s).
    pub sample: f64,
    pub seconds_hint: f64,
    /// Print motor internals with every report line.
    pub verbose: bool,
    /// Report interval (s).
    pub report: f64,
    /// Modes kept per flexible link.
    pub flex_modes: usize,
    /// Bypass position firmware and expose normalized bridge duty commands.
    pub driver_control: bool,
}
impl Default for BuildOptions {
    fn default() -> Self {
        Self { planar: false, flex: true, contact: true, step: 5.0e-4, sample: 0.01, seconds_hint: 2.0, verbose: false, report: 0.1, flex_modes: 4, driver_control: false }
    }
}

struct MotorIds {
    name: String,
    joint: String,
    current: StateId,
    torque: StateId,
    command: Option<StateId>,
    winding: StateId,
    mount: Option<StateId>,
    p: StateId,
    n: StateId,
    stall: f64,
    limit: f64,
    max_winding_c: f64,
    tg_c: f64,
    backlash: f64,
    gear_angle: StateId,
    shaft: StateId,
    rotor_speed: StateId,
}

#[derive(Default, Clone)]
struct MotorStats {
    peak_current: f64,
    sq_current: f64,
    peak_torque: f64,
    peak_winding: f64,
    peak_mount: f64,
    energy: f64,
    saturated: usize,
    crossings: usize,
    last_gap_sign: f64,
}

#[derive(Default, Clone)]
struct JointStats {
    peak_force: f64,
    peak_torque: f64,
    lo: f64,
    hi: f64,
    limit_hits: usize,
    friction_loss: f64,
}

#[derive(Default, Clone)]
struct LinkStats {
    peak_stress: f64,
    hotspot: Vec<f64>,
    max_deflection: f64,
    peak_temperature: f64,
}

pub struct PhysicalRobot {
    pub runtime: Runtime,
    pub model: Arc<PhysicalModel>,
    pub art: Articulated,
    /// Port DOF names in port order (e.g. `joint.hip`), the joints the UI moves.
    pub joint_names: Vec<String>,
    pub targets: Arc<Mutex<Vec<f64>>>,
    pub seam: Option<BehaviorId>,
    pub warnings: Vec<String>,
    pub step: f64,
    art_states: Vec<StateId>,
    port_angles: Vec<StateId>,
    temperature_ids: Vec<StateId>,
    motors: Vec<MotorIds>,
    battery: Option<(StateId, StateId, StateId)>,
    motor_stats: Vec<MotorStats>,
    joint_stats: Vec<JointStats>,
    link_stats: Vec<LinkStats>,
    contact_peak: f64,
    contact_pairs: BTreeMap<(usize, Option<usize>), f64>,
    base_path: Vec<[f64; 4]>,
    trace_t: Vec<f64>,
    trace_joints: Vec<Vec<f64>>,
    trace_motors: Vec<Vec<[f64; 3]>>,
    /// Slices that needed a finer step than nominal.
    pub step_refinements: usize,
    prev_qd: Option<(f64, Vec<f64>)>,
    sample_interval: f64,
    last_sample: f64,
    steps: usize,
    wall: f64,
    samples: usize,
    fell: bool,
    /// Trajectory (time, targets) when control mode is `trajectory`.
    pub trajectory: Vec<(f64, Vec<f64>)>,
}

impl PhysicalRobot {
    pub fn build(model: PhysicalModel, registry: &BehaviorRegistry, opts: &BuildOptions) -> Result<Self, String> {
        Self::build_with(model, registry, opts, |_, _| Ok(()))
    }

    /// Compose scripted/native surrounding components into the CAD plant before
    /// compilation. All callers retain the same physical assembly and solver.
    pub fn build_with<F>(model: PhysicalModel, registry: &BehaviorRegistry, opts: &BuildOptions, compose: F) -> Result<Self, String>
    where F: FnOnce(&mut ModelWorld, &Instance) -> Result<(), String> {
        let mut model = model;
        model.apply_identification();
        let model = Arc::new(model);
        let mut warnings = Vec::new();
        let art_opts = Options { planar: opts.planar, flex: opts.flex, contact: opts.contact, flex_modes: opts.flex_modes.max(1), ..Options::default() };
        let art = Articulated::new(model.clone(), &art_opts)?;
        warnings.extend(art.warnings.iter().cloned());
        let handle = register_model(model.as_ref().clone());
        let mut params: Vec<(&'static str, f64)> = vec![("model", handle), ("planar", if opts.planar { 1.0 } else { 0.0 }), ("flex", if opts.flex { 1.0 } else { 0.0 }), ("contact", if opts.contact { 1.0 } else { 0.0 }), ("flex.modes", opts.flex_modes.max(1) as f64)];
        for (k, val) in art.port_parameters() {
            params.push((leak(k), val));
        }
        let mut m = ModelWorld::default();
        let robot = m.part(registry, "robot", ARTICULATED, params).unwrap();
        m.connect([robot.port("frame.base")]);
        for name in &art.signal_out_names {
            if !name.starts_with("imu.") {
                m.connect([robot.port(leak(name.clone()))]);
            }
        }
        let ambient_k = model.world.ambient_c + 273.15;
        let ambient = m.part(registry, "ambient", sim_domain_thermal::AMBIENT, [("temperature", ambient_k)]).unwrap();
        let mut ambient_ports = vec![ambient.port("node")];
        let gnd = m.part(registry, "gnd", sim_domain_electrical::elements::GROUND, []).unwrap();
        let mut gnd_ports = vec![gnd.port("pin")];
        // Battery (one pack for every motor) or nothing: motors without a
        // pack get their own ideal supply at the spec voltage.
        let battery = model.battery.as_ref().map(|b| m.part(registry, "battery", BATTERY, [("cells", b.cells), ("nominal_voltage", b.nominal_voltage), ("internal_resistance", b.internal_resistance), ("capacity_ah", b.capacity_ah), ("initial_soc", b.initial_soc)]).unwrap());
        let mut supply_ports: Vec<PortId> = Vec::new();
        if let Some(b) = &battery {
            supply_ports.push(b.port("p"));
            gnd_ports.push(b.port("n"));
            m.connect([b.port("soc")]);
        }
        // Mount thermal nodes per link (created on demand).
        let mut mounts: BTreeMap<usize, Instance> = BTreeMap::new();
        let mut mount_ports: BTreeMap<usize, Vec<PortId>> = BTreeMap::new();
        let mut temperature_driven: BTreeMap<String, Instance> = BTreeMap::new(); // link name → probe
        // Joint ports: connected with whatever drives them.
        let mut joint_conn: BTreeMap<String, Vec<PortId>> = art.port_names.iter().map(|n| (n.clone(), vec![robot.port(leak(n.clone()))])).collect();
        let mut seam_params: Vec<(&'static str, f64)> = vec![("period", model.control.period_s.max(1e-4)), ("output_delay", (model.control.latency_s / model.control.period_s.max(1e-4)).round())];
        let mut seam_links: Vec<(String, PortId)> = Vec::new(); // (seam port name, other port)
        let mut motors_built: Vec<(Motor, Instance, Option<Instance>, Option<Instance>, Instance, Instance, Option<usize>, String)> = Vec::new();
        // Encoders and tachometers on every port DOF for the seam.
        let mut port_sensors: Vec<(String, Instance, Instance)> = Vec::new();
        let mut angle_groups: BTreeMap<String, (String, Vec<PortId>)> = BTreeMap::new();
        let mut speed_groups: BTreeMap<String, (String, Vec<PortId>)> = BTreeMap::new();
        for name in &art.port_names {
            let enc = m.part(registry, &format!("{name}.encoder"), sense::ENCODER, []).unwrap();
            let tacho = m.part(registry, &format!("{name}.tacho"), sense::TACHOMETER, []).unwrap();
            joint_conn.get_mut(name).unwrap().extend([enc.port("shaft"), tacho.port("shaft")]);
            let short = name.trim_start_matches("joint.").trim_start_matches("slide.").to_owned();
            seam_params.push((leak(format!("sense.{short}.angle")), 0.0));
            seam_params.push((leak(format!("sense.{short}.speed")), 0.0));
            // One connection per signal: the encoder's angle feeds the seam and,
            // for a driven joint, the firmware too (added below).
            angle_groups.insert(name.clone(), (format!("sense.{short}.angle"), vec![enc.port("angle")]));
            speed_groups.insert(name.clone(), (format!("sense.{short}.speed"), vec![tacho.port("speed")]));
            port_sensors.push((name.clone(), enc, tacho));
        }
        let mut targets_order: Vec<String> = Vec::new();
        for motor in &model.motors {
            let Some(jname) = motor.joint.as_deref() else {
                warnings.push(format!("motor {} drives no joint; it is left out of the circuit", motor.name));
                continue;
            };
            let port_name = ["joint.", "slide."].iter().map(|p| format!("{p}{jname}")).find(|n| joint_conn.contains_key(n));
            let Some(port_name) = port_name else {
                warnings.push(format!("motor {} drives joint {jname}, which has no port (fixed or unknown joint)", motor.name));
                continue;
            };
            let joint = model.joint(jname);
            let joint_backlash = joint.map(|j| j.physics.backlash).unwrap_or(0.0);
            let link = motor.mounted_on.as_deref().and_then(|l| model.link_index(l)).or_else(|| joint.and_then(|j| j.parent.as_deref()).and_then(|p| model.link_index(p)));
            let e = &motor.electrical;
            let gb = &motor.gearbox;
            let th = &motor.thermal;
            let ratio = gb.ratio.max(1e-6) * motor.gear_ratio.max(1e-6);
            let unit = m
                .part(registry, &format!("{}.unit", motor.name), MOTOR_UNIT, [
                    ("resistance", e.resistance.max(1e-3)),
                    ("inductance", e.inductance.max(0.0)),
                    ("torque_constant", e.torque_constant.max(1e-6)),
                    ("back_emf_constant", if e.back_emf_constant > 0.0 { e.back_emf_constant } else { e.torque_constant }),
                    ("no_load_current", e.no_load_current),
                    ("rotor_inertia", e.rotor_inertia.max(1e-9)),
                    ("ratio", ratio),
                    ("efficiency", gb.efficiency.clamp(0.05, 1.0)),
                    ("backlash", gb.backlash_rad + joint_backlash),
                    ("gear_stiffness", gb.stiffness.max(1.0)),
                    ("gear_damping", 0.002 * gb.stiffness.max(1.0)),
                    ("gear_inertia", gb.inertia.max(0.0)),
                    ("gear_friction", if gb.max_output_torque.is_finite() { 0.05 * gb.max_output_torque } else { 0.0 }),
                    ("temp_coeff", th.resistance_temp_coeff),
                    ("derating", th.torque_derating_per_c),
                    ("reference", ambient_k),
                ])
                .unwrap();
            joint_conn.get_mut(&port_name).unwrap().push(unit.port("shaft"));
            for s in ["current", "torque", "speed"] {
                if opts.driver_control {
                    let channel = format!("sense.{}.{s}", motor.name);
                    seam_params.push((leak(channel.clone()), 0.0));
                    seam_links.push((channel, unit.port(s)));
                } else {
                    m.connect([unit.port(s)]);
                }
            }
            // Thermal path: winding → case → (mount, ambient).
            let wcap = m.part(registry, &format!("{}.winding", motor.name), sim_domain_thermal::CAPACITANCE, [("heat_capacity", th.winding_heat_capacity.max(0.1)), ("initial.temperature", ambient_k)]).unwrap();
            let ccap = m.part(registry, &format!("{}.case", motor.name), sim_domain_thermal::CAPACITANCE, [("heat_capacity", th.case_heat_capacity.max(0.1)), ("initial.temperature", ambient_k)]).unwrap();
            let g_wc = m.part(registry, &format!("{}.g_wc", motor.name), sim_domain_thermal::CONDUCTANCE, [("conductance", 1.0 / th.r_winding_case.max(1e-3))]).unwrap();
            let g_ca = m.part(registry, &format!("{}.g_ca", motor.name), sim_domain_thermal::CONDUCTANCE, [("conductance", 1.0 / th.r_case_ambient.max(1e-3))]).unwrap();
            m.connect([unit.port("winding"), wcap.port("node"), g_wc.port("a")]);
            let mut case_ports = vec![g_wc.port("b"), ccap.port("node"), g_ca.port("a")];
            ambient_ports.push(g_ca.port("b"));
            if let Some(li) = link {
                let l = &model.links[li];
                let mat = model.material_of(l);
                if !mounts.contains_key(&li) {
                    let area = {
                        let lo = l.bbox.first().copied().unwrap_or([0.0; 3]);
                        let hi = l.bbox.get(1).copied().unwrap_or([0.01; 3]);
                        let d = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
                        2.0 * (d[0] * d[1] + d[1] * d[2] + d[0] * d[2]).abs().max(1e-4)
                    };
                    let mcap = m.part(registry, &format!("{}.mount", l.name), sim_domain_thermal::CAPACITANCE, [("heat_capacity", (l.mass * mat.specific_heat).max(0.5)), ("initial.temperature", ambient_k)]).unwrap();
                    let g_ma = m.part(registry, &format!("{}.g_ma", l.name), sim_domain_thermal::CONDUCTANCE, [("conductance", 10.0 * area)]).unwrap();
                    let probe = m.part(registry, &format!("{}.probe", l.name), THERMAL_PROBE, []).unwrap();
                    ambient_ports.push(g_ma.port("b"));
                    mount_ports.insert(li, vec![mcap.port("node"), g_ma.port("a"), probe.port("node")]);
                    let tname = format!("temperature.{}", l.name);
                    if art.signal_in_names.contains(&tname) {
                        m.connect([probe.port("temperature"), robot.port(leak(tname))]);
                    } else {
                        m.connect([probe.port("temperature")]);
                    }
                    temperature_driven.insert(l.name.clone(), probe.clone());
                    mounts.insert(li, mcap);
                }
                let g_cm = m.part(registry, &format!("{}.g_cm", motor.name), sim_domain_thermal::CONDUCTANCE, [("conductance", 1.0 / th.r_case_mount.max(1e-3))]).unwrap();
                case_ports.push(g_cm.port("a"));
                mount_ports.get_mut(&li).unwrap().push(g_cm.port("b"));
            }
            m.connect(case_ports);
            // Electrical path: supply → bridge → unit, and the firmware.
            let fw = &motor.firmware;
            let (bridge, firmware) = if fw.kind == "none" && !opts.driver_control {
                m.connect([unit.port("p")]);
                gnd_ports.push(unit.port("n"));
                (None, None)
            } else {
                let bridge = m.part(registry, &format!("{}.bridge", motor.name), H_BRIDGE, [("on_resistance", motor.driver.on_resistance.max(0.0)), ("current_limit", motor.driver.current_limit.min(e.current_limit).max(0.01))]).unwrap();
                if battery.is_some() {
                    supply_ports.push(bridge.port("supply_p"));
                } else {
                    let src = m.part(registry, &format!("{}.supply", motor.name), sim_domain_electrical::elements::VOLTAGE_SOURCE, [("voltage", e.supply_voltage.max(0.1))]).unwrap();
                    m.connect([src.port("p"), bridge.port("supply_p")]);
                    gnd_ports.push(src.port("n"));
                }
                gnd_ports.push(bridge.port("supply_n"));
                gnd_ports.push(bridge.port("n"));
                gnd_ports.push(unit.port("n"));
                m.connect([bridge.port("p"), unit.port("p")]);
                if opts.driver_control {
                    let channel = format!("act.{}.duty", motor.name);
                    seam_params.push((leak(channel.clone()), 0.0));
                    seam_links.push((channel, bridge.port("command")));
                    (Some(bridge), None)
                } else {
                let duty_scale = if fw.output == "current" { motor.driver.current_limit.min(e.current_limit).max(0.01) } else { e.supply_voltage.max(0.1) };
                let firmware = m
                    .part(registry, &format!("{}.firmware", motor.name), SERVO_FIRMWARE, [
                        ("rate", fw.loop_rate_hz.max(1.0)),
                        ("latency", fw.latency_s.max(0.0)),
                        ("deadband", fw.deadband_rad.max(0.0)),
                        ("resolution", fw.sensor_resolution_rad.max(0.0)),
                        // Gains are volts per radian; the bridge takes a duty in
                        // −1…1 of its supply, so scale by the rated voltage.
                        ("kp", fw.kp / duty_scale),
                        ("ki", fw.ki / duty_scale),
                        ("kd", fw.kd / duty_scale),
                        ("limit", 1.0),
                    ])
                    .unwrap();
                m.connect([firmware.port("command"), bridge.port("command")]);
                // Measured angle from the joint's encoder; target from the seam.
                angle_groups.get_mut(&port_name).unwrap().1.push(firmware.port("measured"));
                speed_groups.get_mut(&port_name).unwrap().1.push(firmware.port("rate"));
                let short = port_name.trim_start_matches("joint.").trim_start_matches("slide.").to_owned();
                seam_params.push((leak(format!("act.{short}.target")), 0.0));
                seam_links.push((format!("act.{short}.target"), firmware.port("target")));
                targets_order.push(port_name.clone());
                (Some(bridge), Some(firmware))
                }
            };
            motors_built.push((motor.clone(), unit, bridge, firmware, wcap, ccap, link, port_name));
        }
        // Encoders already feed the seam; an encoder output wired twice is fine
        // (signal fan-out), but the seam port must exist for every sense channel.
        // IMU signals into the seam.
        for name in art.signal_out_names.iter().filter(|n| n.starts_with("imu.")) {
            seam_params.push((leak(format!("sense.{name}")), 0.0));
            seam_links.push((format!("sense.{name}"), robot.port(leak(name.clone()))));
        }
        // Temperature inputs without a mount: ambient.
        for name in &art.signal_in_names {
            let link = name.trim_start_matches("temperature.");
            if !temperature_driven.contains_key(link) {
                let c = m.part(registry, &format!("{link}.ambient_probe"), sim_domain_control::elements::CONSTANT, [("value", ambient_k)]).unwrap();
                m.connect([c.port("value"), robot.port(leak(name.clone()))]);
            }
        }
        for (_, ports) in mount_ports {
            m.connect(ports);
        }
        for (_, ports) in joint_conn.iter() {
            m.connect(ports.clone());
        }
        if !supply_ports.is_empty() {
            m.connect(supply_ports);
        }
        m.connect(gnd_ports);
        m.connect(ambient_ports);
        let seam = if seam_links.is_empty() { None } else { Some(m.part(registry, "controller", EXTERNAL, seam_params).unwrap()) };
        if let Some(seam) = &seam {
            for (name, other) in &seam_links {
                m.connect([seam.port(leak(name.clone())), *other]);
            }
            for (_, (seam_name, mut ports)) in angle_groups.into_iter().chain(speed_groups) {
                ports.push(seam.port(leak(seam_name)));
                m.connect(ports);
            }
        } else {
            for (_, (_, ports)) in angle_groups.into_iter().chain(speed_groups) {
                m.connect(ports);
            }
        }
        let integrator = Integrator::BackwardEuler(newton());
        compose(&mut m, &robot)?;
        let runtime = Runtime::new(m, registry, integrator).map_err(|e| format!("the physical model does not compile: {e}"))?;
        let art_states: Vec<StateId> = art.state_names().iter().map(|n| runtime.state_id(robot.behavior, n)).collect();
        let port_angles: Vec<StateId> = art.port_names.iter().map(|n| runtime.across_id(robot.port(leak(n.clone())))).collect();
        let temperature_ids: Vec<StateId> = art.signal_in_names.iter().map(|n| runtime.signal_id(robot.port(leak(n.clone())))).collect();
        let mut motors = Vec::new();
        for (motor, unit, bridge, firmware, wcap, _ccap, link, port_name) in &motors_built {
            let _ = bridge;
            motors.push(MotorIds {
                name: motor.name.clone(),
                joint: port_name.clone(),
                current: runtime.state_id(unit.behavior, "current"),
                torque: runtime.signal_id(unit.port("torque")),
                command: firmware.as_ref().map(|f| runtime.state_id(f.behavior, "command")),
                winding: runtime.across_id(wcap.port("node")),
                mount: link.and_then(|li| mounts.get(&li)).map(|mc| runtime.across_id(mc.port("node"))),
                p: runtime.across_id(unit.port("p")),
                n: runtime.across_id(unit.port("n")),
                stall: motor.gearbox.max_output_torque,
                limit: 1.0,
                max_winding_c: motor.thermal.max_winding_c,
                tg_c: link.map(|li| model.material_of(&model.links[li]).glass_transition_c).unwrap_or(60.0),
                backlash: motor.gearbox.backlash_rad,
                gear_angle: runtime.state_id(unit.behavior, "gear_angle"),
                shaft: runtime.across_id(unit.port("shaft")),
                rotor_speed: runtime.state_id(unit.behavior, "rotor_speed"),
            });
        }
        let battery_ids = battery.as_ref().map(|b| (runtime.state_id(b.behavior, "soc"), runtime.across_id(b.port("p")), runtime.across_id(b.port("n"))));
        // Targets: the control block's hold values (rad) in `targets_order`.
        let initial: Vec<f64> = targets_order.iter().map(|p| model.control.targets.get(p.trim_start_matches("joint.").trim_start_matches("slide.")).copied().unwrap_or(0.0)).collect();
        let targets = Arc::new(Mutex::new(initial));
        let trajectory: Vec<(f64, Vec<f64>)> = if model.control.mode == "trajectory" {
            model.control.trajectory.iter().map(|pt| (pt.t, targets_order.iter().map(|p| pt.targets.get(p.trim_start_matches("joint.").trim_start_matches("slide.")).copied().unwrap_or(0.0)).collect())).collect()
        } else {
            Vec::new()
        };
        let mut runtime = runtime;
        if let Some(seam) = &seam {
            let contract = runtime.contract(seam.behavior);
            let act: Vec<usize> = targets_order.iter().map(|p| {
                let short = p.trim_start_matches("joint.").trim_start_matches("slide.");
                contract.actuators.iter().position(|c| c.name == format!("{short}.target")).expect("seam actuator")
            }).collect();
            let held = targets.clone();
            let traj = trajectory.clone();
            runtime
                .attach(
                    seam.behavior,
                    Box::new(FnCoupler(move |t: f64, _s: &[f64], a: &mut [f64]| {
                        let current: Vec<f64> = if traj.is_empty() { held.lock().unwrap_or_else(|p| p.into_inner()).clone() } else { interpolate(&traj, t) };
                        for (k, idx) in act.iter().enumerate() {
                            a[*idx] = current.get(k).copied().unwrap_or(0.0);
                        }
                    })),
                )
                .map_err(|e| e.to_string())?;
        }
        let nj = art.joints.len();
        let link_stats = (0..art.links.len()).map(|li| LinkStats { hotspot: vec![0.0; art.links[li].flex.as_ref().map(|f| f.stress_cells.len()).unwrap_or(0)], ..Default::default() }).collect();
        let joint_names: Vec<String> = targets_order.clone();
        let joint_names = if joint_names.is_empty() { art.port_names.clone() } else { joint_names };
        Ok(Self {
            runtime,
            model: model.clone(),
            art,
            joint_names,
            targets,
            seam: seam.map(|s| s.behavior),
            warnings,
            step: opts.step,
            art_states,
            port_angles,
            temperature_ids,
            motors,
            battery: battery_ids,
            motor_stats: vec![MotorStats::default(); motors_built.len()],
            joint_stats: vec![JointStats { lo: f64::INFINITY, hi: f64::NEG_INFINITY, ..Default::default() }; nj],
            link_stats,
            contact_peak: 0.0,
            contact_pairs: BTreeMap::new(),
            base_path: Vec::new(),
            trace_t: Vec::new(),
            trace_joints: Vec::new(),
            trace_motors: Vec::new(),
            step_refinements: 0,
            prev_qd: None,
            sample_interval: opts.sample,
            last_sample: -1.0,
            steps: 0,
            wall: 0.0,
            samples: 0,
            fell: false,
            trajectory,
        })
    }

    pub fn time(&self) -> f64 {
        self.runtime.time
    }

    /// Advance by `duration` (fixed backward-Euler steps), sampling results.
    pub fn advance(&mut self, duration: f64) -> Result<(), String> {
        let start = Instant::now();
        let h = self.step;
        // Step in slices no longer than the sampling interval so the
        // results see every sample instant whatever the caller's cadence.
        let mut left = duration;
        while left > 1e-12 {
            let slice = left.min(self.sample_interval).max(h);
            let slice = (slice / h).round().max(1.0) * h;
            // A stiff transient (an impact, a hard gear mesh) can defeat Newton
            // at the nominal step: retry the slice from a snapshot with the
            // step halved, up to 16× finer, before giving up.
            let snapshot = self.runtime.snapshot();
            let mut sub = h;
            let mut tries = 0;
            loop {
                match self.runtime.advance(slice, sub) {
                    Ok(()) => {
                        self.steps += (slice / sub).round() as usize;
                        break;
                    }
                    Err(e) => {
                        tries += 1;
                        if tries > 4 {
                            return Err(e.to_string());
                        }
                        self.runtime.restore(&snapshot).map_err(|e| e.to_string())?;
                        sub *= 0.5;
                        self.step_refinements += 1;
                    }
                }
            }
            left -= slice;
            if self.runtime.time - self.last_sample >= self.sample_interval - 1e-9 {
                self.wall += start.elapsed().as_secs_f64();
                self.sample();
                return self.advance(left.max(0.0));
            }
        }
        self.wall += start.elapsed().as_secs_f64();
        Ok(())
    }

    /// The articulated element's generalized coordinates now, with joint
    /// accelerations from the change in speed since the last sample.
    pub fn generalized(&self) -> Generalized {
        let states: Vec<f64> = self.art_states.iter().map(|id| self.runtime.get(*id)).collect();
        let mut rates = vec![0.0; states.len()];
        if let Some((t0, qd0)) = &self.prev_qd {
            let dt = self.runtime.time - t0;
            if dt > 1e-9 {
                for (k, (_, d)) in self.art.dofs().enumerate() {
                    rates[d.qd_state] = (states[d.qd_state] - qd0[k]) / dt;
                }
            }
        }
        let mut angles = vec![0.0];
        angles.extend(self.port_angles.iter().map(|id| self.runtime.get(*id)));
        let temps = self.temperature_ids.iter().map(|id| self.runtime.get(*id)).collect();
        self.art.generalized(states, rates, &angles, temps)
    }

    pub fn poses(&self) -> Vec<(M, V)> {
        self.art.poses(&self.generalized())
    }

    pub fn recorded_samples(&self) -> usize {
        self.trace_t.len()
    }

    pub fn joint_angles(&self) -> Vec<f64> {
        self.joint_names.iter().map(|n| self.art.port_names.iter().position(|p| p == n).map(|k| self.runtime.get(self.port_angles[k])).unwrap_or(0.0)).collect()
    }

    pub fn set_target(&self, joint: usize, angle: f64) {
        let mut t = self.targets.lock().unwrap_or_else(|p| p.into_inner());
        if joint < t.len() {
            t[joint] = angle;
        }
    }

    pub fn contacts(&self) -> Vec<ContactPoint> {
        self.art.evaluate(&self.generalized()).contacts
    }

    /// Joint points and axes in the world now, in tree-joint order.
    pub fn joint_frames(&self) -> Vec<(String, V, Vec<V>)> {
        let e = self.art.evaluate_with(&self.generalized(), false);
        self.art.joints.iter().zip(&e.joints).map(|(j, r)| (j.name.clone(), r.point, r.axes.clone())).collect()
    }

    /// Flexible links' boundary deflections in the world: `(link, point, deflection)`.
    pub fn deflections(&self) -> Vec<(usize, V, V)> {
        let g = self.generalized();
        let poses = self.art.poses(&g);
        let mut out = Vec::new();
        for (li, l) in self.art.links.iter().enumerate() {
            let Some(f) = &l.flex else { continue };
            let (r, p) = poses[li];
            for (b, point) in f.boundary_points.iter().enumerate() {
                let mut u = V::zeros();
                for m in 0..f.modes {
                    let s = f.shapes[m][b];
                    u += V::new(s[0], s[1], s[2]) * g.states[f.state + m];
                }
                out.push((li, p + r * point, r * u));
            }
        }
        out
    }

    fn sample(&mut self) {
        let t = self.runtime.time;
        let g = self.generalized();
        let e = self.art.evaluate(&g);
        // Joints.
        let mut di = 0;
        for (ji, j) in self.art.joints.iter().enumerate() {
            let st = &mut self.joint_stats[ji];
            let f = e.joints[ji].f;
            let n = e.joints[ji].n;
            // Radial load: force perpendicular to the (first) axis.
            let axis = e.joints[ji].axes.first().copied().unwrap_or(V::z());
            let radial = (f - axis * f.dot(&axis)).norm();
            st.peak_force = st.peak_force.max(radial.max(f.norm() * 0.0));
            st.peak_torque = st.peak_torque.max((n - axis * n.dot(&axis)).norm());
            for d in &j.dofs {
                let q = g.q[di] + d.home;
                let qd = g.qd[di];
                if d.port.is_some() {
                    st.lo = st.lo.min(q);
                    st.hi = st.hi.max(q);
                    if d.upper.map(|u| q > u).unwrap_or(false) || d.lower.map(|l| q < l).unwrap_or(false) {
                        st.limit_hits += 1;
                    }
                    st.friction_loss += (sim_domain_robot::articulated::friction_torque(&d.friction, qd) * qd).abs() * self.sample_interval;
                }
                di += 1;
            }
        }
        // Links: stress, deflection, temperature.
        for (li, l) in self.art.links.iter().enumerate() {
            if let Some(f) = &l.flex {
                let eta: Vec<f64> = (0..f.modes).map(|m| g.states[f.state + m]).collect();
                let stress = self.art.stress(li, &eta);
                let st = &mut self.link_stats[li];
                for (c, s) in stress.iter().enumerate() {
                    st.hotspot[c] = st.hotspot[c].max(*s);
                    st.peak_stress = st.peak_stress.max(*s);
                }
                let mut defl: f64 = 0.0;
                for (b, _) in f.boundary_points.iter().enumerate() {
                    let mut u = V::zeros();
                    for m in 0..f.modes {
                        let s = f.shapes[m][b];
                        u += V::new(s[0], s[1], s[2]) * eta[m];
                    }
                    defl = defl.max(u.norm());
                }
                st.max_deflection = st.max_deflection.max(defl);
                if let Some(k) = f.temperature_signal {
                    st.peak_temperature = st.peak_temperature.max(g.temperatures[k] - 273.15);
                }
            }
        }
        // Motors.
        let dt = if self.last_sample < 0.0 { 0.0 } else { t - self.last_sample };
        for (k, ids) in self.motors.iter().enumerate() {
            let st = &mut self.motor_stats[k];
            let i = self.runtime.get(ids.current);
            let tau = self.runtime.get(ids.torque);
            st.peak_current = st.peak_current.max(i.abs());
            st.sq_current += i * i * dt;
            st.peak_torque = st.peak_torque.max(tau.abs());
            st.peak_winding = st.peak_winding.max(self.runtime.get(ids.winding) - 273.15);
            if let Some(m) = ids.mount {
                st.peak_mount = st.peak_mount.max(self.runtime.get(m) - 273.15);
            }
            let v = self.runtime.get(ids.p) - self.runtime.get(ids.n);
            st.energy += (v * i).abs() * dt;
            if let Some(c) = ids.command {
                if self.runtime.get(c).abs() >= 0.999 * ids.limit {
                    st.saturated += 1;
                }
            }
            if ids.backlash > 0.0 {
                let gap = self.runtime.get(ids.gear_angle) - self.runtime.get(ids.shaft);
                let sign = if gap > 0.5 * ids.backlash { 1.0 } else if gap < -0.5 * ids.backlash { -1.0 } else { 0.0 };
                if sign != 0.0 && st.last_gap_sign != 0.0 && sign != st.last_gap_sign {
                    st.crossings += 1;
                }
                if sign != 0.0 {
                    st.last_gap_sign = sign;
                }
            }
        }
        // Contacts and base.
        for c in &e.contacts {
            let f = c.force.norm();
            self.contact_peak = self.contact_peak.max(f);
            let entry = self.contact_pairs.entry((c.link, c.other)).or_insert(0.0);
            *entry = entry.max(f);
        }
        let base = &self.art.bases[0];
        let p = V::new(g.states[base.state], g.states[base.state + 1], g.states[base.state + 2]);
        let q = sim_domain_robot::math::quat(g.states[base.state + 3], g.states[base.state + 4], g.states[base.state + 5], g.states[base.state + 6]);
        let up = q * V::z();
        if !base.grounded && up.z < 0.5 {
            self.fell = true;
        }
        self.base_path.push([t, p.x, p.y, p.z]);
        // Trace.
        self.trace_t.push(t);
        self.trace_joints.push(self.joint_angles());
        self.trace_motors.push(self.motors.iter().map(|ids| [self.runtime.get(ids.current), self.runtime.get(ids.winding) - 273.15, self.runtime.get(ids.torque)]).collect());
        // Remember speeds for the next sample's accelerations.
        let qd: Vec<f64> = self.art.dofs().map(|(_, d)| g.states[d.qd_state]).collect();
        self.prev_qd = Some((t, qd));
        self.last_sample = t;
        self.samples += 1;
    }

    /// The results document (contract: `cad/PHYSICAL_MODEL.md`).
    pub fn results(&self, model_path: &str) -> Value {
        let mut links = serde_json::Map::new();
        for (li, l) in self.art.links.iter().enumerate() {
            let st = &self.link_stats[li];
            let yield_margin = if st.peak_stress > 0.0 { l.yield_strength / st.peak_stress - 1.0 } else { f64::INFINITY };
            let hot: Vec<Value> = if let Some(f) = &l.flex {
                let mut idx: Vec<usize> = (0..st.hotspot.len()).collect();
                idx.sort_by(|a, b| st.hotspot[*b].total_cmp(&st.hotspot[*a]));
                idx.truncate(200);
                idx.iter().map(|&c| json!([f.stress_cells[c].x, f.stress_cells[c].y, f.stress_cells[c].z])).collect()
            } else {
                Vec::new()
            };
            let hot_vals: Vec<f64> = if l.flex.is_some() {
                let mut idx: Vec<usize> = (0..st.hotspot.len()).collect();
                idx.sort_by(|a, b| st.hotspot[*b].total_cmp(&st.hotspot[*a]));
                idx.truncate(200);
                idx.iter().map(|&c| st.hotspot[c]).collect()
            } else {
                Vec::new()
            };
            let tg = self.model.material_of(&self.model.links[li]).glass_transition_c;
            links.insert(
                l.name.clone(),
                json!({
                    "peak_stress_pa": st.peak_stress,
                    "yield_margin": finite_or_null(yield_margin),
                    "hotspot": {"cells": hot, "stress_pa": hot_vals},
                    "max_deflection_m": st.max_deflection,
                    "peak_temperature_c": if st.peak_temperature > 0.0 { st.peak_temperature } else { self.model.world.ambient_c },
                    "tg_margin_c": tg - if st.peak_temperature > 0.0 { st.peak_temperature } else { self.model.world.ambient_c },
                }),
            );
        }
        let mut joints = serde_json::Map::new();
        for (ji, j) in self.art.joints.iter().enumerate() {
            let st = &self.joint_stats[ji];
            let area = (2.0 * j.pin_radius * j.contact_length).max(1e-9);
            let pressure = st.peak_force / area;
            let shear = j.shear_capacity.map(|c| if st.peak_force > 0.0 { c / st.peak_force - 1.0 } else { f64::INFINITY });
            joints.insert(
                j.name.clone(),
                json!({
                    "peak_reaction_force_n": st.peak_force,
                    "peak_reaction_torque_nm": st.peak_torque,
                    "bearing_pressure_pa": pressure,
                    "bearing_margin": finite_or_null(if pressure > 0.0 { j.allowable_pressure / pressure - 1.0 } else { f64::INFINITY }),
                    "screw_shear_margin": shear.map(finite_or_null),
                    "range_used_rad": if st.lo.is_finite() { json!([st.lo, st.hi]) } else { Value::Null },
                    "limit_hits": st.limit_hits,
                    "friction_loss_j": st.friction_loss,
                    "backlash_crossings": self.motors.iter().zip(&self.motor_stats).find(|(m, _)| m.joint.ends_with(&j.name)).map(|(_, s)| s.crossings).unwrap_or(0),
                }),
            );
        }
        let mut motors = serde_json::Map::new();
        let duration = self.runtime.time.max(1e-9);
        for (k, ids) in self.motors.iter().enumerate() {
            let st = &self.motor_stats[k];
            motors.insert(
                ids.name.clone(),
                json!({
                    "peak_current_a": st.peak_current,
                    "rms_current_a": (st.sq_current / duration).sqrt(),
                    "peak_torque_nm": st.peak_torque,
                    "stall_margin": finite_or_null(if ids.stall.is_finite() && ids.stall > 0.0 { 1.0 - st.peak_torque / ids.stall } else { f64::INFINITY }),
                    "peak_winding_c": st.peak_winding,
                    "peak_mount_c": st.peak_mount,
                    "mount_tg_margin_c": ids.tg_c - st.peak_mount.max(self.model.world.ambient_c),
                    "winding_margin_c": ids.max_winding_c - st.peak_winding,
                    "energy_j": st.energy,
                    "saturated_fraction": if self.samples > 0 { st.saturated as f64 / self.samples as f64 } else { 0.0 },
                }),
            );
        }
        let battery = self.battery.map(|(soc, p, n)| json!({"final_soc": self.runtime.get(soc), "min_voltage": self.runtime.get(p) - self.runtime.get(n), "energy_j": self.motor_stats.iter().map(|s| s.energy).sum::<f64>()}));
        let base = &self.art.bases[0];
        let g = self.generalized();
        let pairs: Vec<Value> = self.contact_pairs.iter().map(|((a, b), f)| json!([self.art.links[*a].name, b.map(|b| self.art.links[b].name.clone()).unwrap_or("world".into()), f])).collect();
        let mut trace_joints = serde_json::Map::new();
        for (k, name) in self.joint_names.iter().enumerate() {
            trace_joints.insert(name.trim_start_matches("joint.").trim_start_matches("slide.").to_owned(), json!(self.trace_joints.iter().map(|r| r.get(k).copied().unwrap_or(0.0)).collect::<Vec<f64>>()));
        }
        let mut trace_motors = serde_json::Map::new();
        for (k, ids) in self.motors.iter().enumerate() {
            trace_motors.insert(ids.name.clone(), json!({"current": self.trace_motors.iter().map(|r| r[k][0]).collect::<Vec<f64>>(), "winding_c": self.trace_motors.iter().map(|r| r[k][1]).collect::<Vec<f64>>(), "torque_nm": self.trace_motors.iter().map(|r| r[k][2]).collect::<Vec<f64>>()}));
        }
        json!({
            "version": 1,
            "model": model_path,
            "duration_s": self.runtime.time,
            "steps": self.steps,
            "step_refinements": self.step_refinements,
            "wall_s": self.wall,
            "warnings": self.warnings,
            "links": links,
            "joints": joints,
            "motors": motors,
            "battery": battery,
            "base": {
                "fell": self.fell,
                "final_pose": {"position": [g.states[base.state], g.states[base.state + 1], g.states[base.state + 2]], "quaternion": [g.states[base.state + 3], g.states[base.state + 4], g.states[base.state + 5], g.states[base.state + 6]]},
                "path": self.base_path.iter().step_by((self.base_path.len() / 500).max(1)).collect::<Vec<_>>(),
            },
            "contacts": {"peak_force_n": self.contact_peak, "pairs": pairs},
            "monte_carlo": Value::Null,
            "trace": {"t": self.trace_t, "joints": trace_joints, "motors": trace_motors},
        })
    }

    /// Scalar metrics for Monte Carlo statistics.
    pub fn metrics(&self) -> BTreeMap<String, f64> {
        let mut out = BTreeMap::new();
        let mut stall = f64::INFINITY;
        let mut winding: f64 = 0.0;
        let mut mount: f64 = 0.0;
        for (k, ids) in self.motors.iter().enumerate() {
            let st = &self.motor_stats[k];
            if ids.stall.is_finite() && ids.stall > 0.0 {
                stall = stall.min(1.0 - st.peak_torque / ids.stall);
            }
            winding = winding.max(st.peak_winding);
            mount = mount.max(st.peak_mount);
        }
        let mut yield_margin = f64::INFINITY;
        for (li, l) in self.art.links.iter().enumerate() {
            let st = &self.link_stats[li];
            if st.peak_stress > 0.0 {
                yield_margin = yield_margin.min(l.yield_strength / st.peak_stress - 1.0);
            }
        }
        let mut bearing = f64::INFINITY;
        for (ji, j) in self.art.joints.iter().enumerate() {
            let st = &self.joint_stats[ji];
            let pressure = st.peak_force / (2.0 * j.pin_radius * j.contact_length).max(1e-9);
            if pressure > 0.0 {
                bearing = bearing.min(j.allowable_pressure / pressure - 1.0);
            }
        }
        out.insert("stall_margin".into(), if stall.is_finite() { stall } else { 1.0 });
        out.insert("yield_margin".into(), if yield_margin.is_finite() { yield_margin } else { 10.0 });
        out.insert("bearing_margin".into(), if bearing.is_finite() { bearing } else { 10.0 });
        out.insert("peak_winding_c".into(), winding);
        out.insert("peak_mount_c".into(), mount);
        out.insert("peak_contact_n".into(), self.contact_peak);
        out.insert("fell".into(), if self.fell { 1.0 } else { 0.0 });
        let g = self.generalized();
        let b = &self.art.bases[0];
        out.insert("base_z".into(), g.states[b.state + 2]);
        out
    }

    pub fn success(&self) -> bool {
        let m = self.metrics();
        !self.fell && m["yield_margin"] > 0.0 && m["stall_margin"] > 0.0
    }
}

fn finite_or_null(x: f64) -> Value {
    if x.is_finite() { json!(x) } else { Value::Null }
}

fn interpolate(traj: &[(f64, Vec<f64>)], t: f64) -> Vec<f64> {
    if traj.is_empty() {
        return Vec::new();
    }
    if t <= traj[0].0 {
        return traj[0].1.clone();
    }
    for w in traj.windows(2) {
        if t >= w[0].0 && t <= w[1].0 {
            let s = if w[1].0 > w[0].0 { (t - w[0].0) / (w[1].0 - w[0].0) } else { 0.0 };
            return w[0].1.iter().zip(&w[1].1).map(|(a, b)| a + (b - a) * s).collect();
        }
    }
    traj.last().unwrap().1.clone()
}

/// Run the file for `seconds`, write results beside it, return the report.
pub fn run_physical(path: &str, seconds: f64, opts: &BuildOptions, out: Option<&str>) -> Result<String, String> {
    run_physical_with_controller(path, seconds, opts, out, None)
}

/// Run the same compiled plant with an optional controller process replacing
/// the model's target supervisor. Spawn only after the model and seam are valid,
/// so a failed build cannot leave a child waiting for a protocol handshake.
pub fn run_physical_with_controller(path: &str, seconds: f64, opts: &BuildOptions, out: Option<&str>, controller: Option<std::process::Command>) -> Result<String, String> {
    if !seconds.is_finite() || seconds <= 0.0 || !opts.step.is_finite() || opts.step <= 0.0 {
        return Err("duration and integration step must be positive and finite".to_owned());
    }
    let model = PhysicalModel::load(path)?;
    let registry = registry();
    let mut robot = PhysicalRobot::build(model, &registry, opts)?;
    if let Some(controller) = controller {
        let seam = robot.seam.ok_or("model has no external control seam")?;
        let controller = sim_couple::FrameCoupler::spawn_command(controller).map_err(|e| format!("could not start controller: {e}"))?;
        robot.runtime.attach(seam, Box::new(controller)).map_err(|e| e.to_string())?;
    }
    let mut lines = vec![format!(
        "{} links, {} joints ({} driven), {} motors, {} loops, {} states; base `{}`{}",
        robot.art.links.len(),
        robot.art.joints.len(),
        robot.joint_names.len(),
        robot.motors.len(),
        robot.art.loops.len(),
        robot.art.state_count,
        robot.art.links[robot.art.root].name,
        if robot.art.grounded { " (ground)" } else { " (floating)" }
    )];
    for w in &robot.warnings {
        lines.push(format!("warning: {w}"));
    }
    let report_every = opts.report.max(1e-3);
    let chunks = (seconds / report_every).ceil().max(1.0) as usize;
    let mut failure = None;
    for k in 0..=chunks {
        if k > 0 {
            if let Err(e) = robot.advance(report_every.min(seconds - (k - 1) as f64 * report_every).max(1e-6)) {
                failure = Some(e);
                break;
            }
        }
        let angles: Vec<String> = robot.joint_names.iter().zip(robot.joint_angles()).map(|(n, a)| format!("{}={:.3}", n.trim_start_matches("joint.").trim_start_matches("slide."), a)).collect();
        let base = robot.generalized();
        let b = &robot.art.bases[0];
        let currents: Vec<String> = robot.motors.iter().map(|m| format!("{:.2}A", robot.runtime.get(m.current))).collect();
        lines.push(format!("t={:.3}s base=({:.4}, {:.4}, {:.4}) {} {}", robot.runtime.time, base.states[b.state], base.states[b.state + 1], base.states[b.state + 2], angles.join(" "), currents.join(" ")));
        if opts.verbose {
            for m in &robot.motors {
                let rt = &robot.runtime;
                lines.push(format!("    {}: i {:.3} A  v {:.2} V  rotor {:.1} rad/s  gear {:.4} rad  shaft {:.4} rad  torque {:.4} N·m  winding {:.1} °C  cmd {:.3}", m.name, rt.get(m.current), rt.get(m.p) - rt.get(m.n), rt.get(m.rotor_speed), rt.get(m.gear_angle), rt.get(m.shaft), rt.get(m.torque), rt.get(m.winding) - 273.15, m.command.map(|c| rt.get(c)).unwrap_or(0.0)));
            }
        }
    }
    let results = robot.results(path);
    let out_path = out.map(|s| s.to_owned()).unwrap_or_else(|| results_path(path));
    std::fs::write(&out_path, serde_json::to_string_pretty(&results).unwrap()).map_err(|e| format!("{out_path}: {e}"))?;
    lines.push(summary(&results));
    lines.push(format!("results written to {out_path}  ({:.2} s wall for {:.2} s simulated, {} steps)", robot.wall, robot.runtime.time, robot.steps));
    if opts.verbose {
        for b in sim_solve::profile::all() {
            if b.calls() > 0 {
                lines.push(format!("  profile {:<28} {:>9.3} s  {:>8} calls", b.name, b.seconds(), b.calls()));
            }
        }
    }
    if let Some(e) = failure {
        return Err(format!("{}\nsimulation stopped: {e}", lines.join("\n")));
    }
    Ok(lines.join("\n"))
}

pub fn results_path(model_path: &str) -> String {
    match model_path.strip_suffix(".simrobot.json") {
        Some(stem) => format!("{stem}.simresult.json"),
        None => format!("{model_path}.simresult.json"),
    }
}

/// A human-readable digest of a results document.
pub fn summary(r: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(links) = r["links"].as_object() {
        for (name, l) in links {
            if l["peak_stress_pa"].as_f64().unwrap_or(0.0) > 0.0 {
                let stress = l["peak_stress_pa"].as_f64().unwrap_or(0.0);
                lines.push(format!("  link {name}: peak stress {:.2} MPa (yield margin {}), deflection {:.3} mm, {:.1} °C", stress / 1e6, if stress < 1.0e3 { "unloaded".to_string() } else { margin(&l["yield_margin"]) }, l["max_deflection_m"].as_f64().unwrap_or(0.0) * 1e3, l["peak_temperature_c"].as_f64().unwrap_or(0.0)));
            }
        }
    }
    if let Some(joints) = r["joints"].as_object() {
        for (name, j) in joints {
            lines.push(format!("  joint {name}: reaction {:.2} N / {:.4} N·m, bearing {:.2} MPa (margin {}), limit hits {}, backlash crossings {}", j["peak_reaction_force_n"].as_f64().unwrap_or(0.0), j["peak_reaction_torque_nm"].as_f64().unwrap_or(0.0), j["bearing_pressure_pa"].as_f64().unwrap_or(0.0) / 1e6, margin(&j["bearing_margin"]), j["limit_hits"], j["backlash_crossings"]));
        }
    }
    if let Some(motors) = r["motors"].as_object() {
        for (name, m) in motors {
            lines.push(format!("  motor {name}: peak {:.2} A (rms {:.2}), peak torque {:.3} N·m (stall margin {}), winding {:.1} °C, mount {:.1} °C (Tg margin {:.1}), saturated {:.0} %", m["peak_current_a"].as_f64().unwrap_or(0.0), m["rms_current_a"].as_f64().unwrap_or(0.0), m["peak_torque_nm"].as_f64().unwrap_or(0.0), margin(&m["stall_margin"]), m["peak_winding_c"].as_f64().unwrap_or(0.0), m["peak_mount_c"].as_f64().unwrap_or(0.0), m["mount_tg_margin_c"].as_f64().unwrap_or(0.0), 100.0 * m["saturated_fraction"].as_f64().unwrap_or(0.0)));
        }
    }
    if let Some(b) = r["battery"].as_object() {
        lines.push(format!("  battery: soc {:.3}, min {:.2} V, {:.2} J", b["final_soc"].as_f64().unwrap_or(0.0), b["min_voltage"].as_f64().unwrap_or(0.0), b["energy_j"].as_f64().unwrap_or(0.0)));
    }
    lines.push(format!("  contacts: peak {:.2} N; base {}", r["contacts"]["peak_force_n"].as_f64().unwrap_or(0.0), if r["base"]["fell"].as_bool().unwrap_or(false) { "FELL" } else { "upright" }));
    if let Some(mc) = r["monte_carlo"].as_object() {
        lines.push(format!("  monte carlo: {} samples, success rate {:.0} %", mc["samples"], 100.0 * mc["success_rate"].as_f64().unwrap_or(0.0)));
        if let Some(metrics) = mc["metrics"].as_object() {
            for (k, s) in metrics {
                lines.push(format!("    {k}: mean {:.3} ± {:.3}  [p5 {:.3}, p50 {:.3}, p95 {:.3}]", s["mean"].as_f64().unwrap_or(0.0), s["std"].as_f64().unwrap_or(0.0), s["p5"].as_f64().unwrap_or(0.0), s["p50"].as_f64().unwrap_or(0.0), s["p95"].as_f64().unwrap_or(0.0)));
            }
        }
    }
    lines.join("\n")
}

fn margin(v: &Value) -> String {
    match v.as_f64() {
        Some(x) if x > 99.0 => "> +9900 %".into(),
        Some(x) => format!("{:+.0} %", 100.0 * x),
        None => "∞".into(),
    }
}

// ---- Monte Carlo --------------------------------------------------------------

/// Perturb a model per its `uncertainty` block.
pub fn perturb(model: &PhysicalModel, rng: &mut Rng) -> PhysicalModel {
    let u = &model.uncertainty;
    let mut m = model.clone();
    for l in &mut m.links {
        let size = {
            let lo = l.bbox.first().copied().unwrap_or([0.0; 3]);
            let hi = l.bbox.get(1).copied().unwrap_or([0.05; 3]);
            ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt().max(1e-3)
        };
        let scale = 1.0 + rng.normal() * u.dimension_m.sigma / size;
        let mass_factor = (1.0 + rng.normal() * u.mass.sigma_fraction).max(0.1);
        l.mass *= mass_factor * scale.powi(3);
        for row in &mut l.inertia {
            for x in row {
                *x *= mass_factor * scale.powi(5);
            }
        }
        // The centre of mass moves inside the part; the geometry stays where
        // it is in the world, so everything in the link frame shifts back.
        let shift: [f64; 3] = [rng.normal() * u.com_m.sigma, rng.normal() * u.com_m.sigma, rng.normal() * u.com_m.sigma];
        for k in 0..3 {
            l.com[k] += shift[k];
        }
        for v in &mut l.collision.vertices {
            for (x, d) in v.iter_mut().zip(shift) {
                *x = *x * scale - d;
            }
        }
        for v in &mut l.collision.hull {
            for (x, d) in v.iter_mut().zip(shift) {
                *x -= d;
            }
        }
        if let Some(s) = &mut l.collision.sdf {
            for (x, d) in s.origin.iter_mut().zip(shift) {
                *x -= d;
            }
        }
        for b in &mut l.bbox {
            for (x, d) in b.iter_mut().zip(shift) {
                *x -= d;
            }
        }
        if let Some(f) = &mut l.flex {
            for bf in &mut f.boundary_frames {
                for (x, d) in bf.point.iter_mut().zip(shift) {
                    *x -= d;
                }
            }
            for c in &mut f.stress_cells {
                for (x, d) in c.iter_mut().zip(shift) {
                    *x -= d;
                }
            }
        }
        for v in &mut l.collision.hull {
            for x in v {
                *x *= scale;
            }
        }
        if let Some(s) = &mut l.collision.sdf {
            s.cell *= scale;
            for x in &mut s.origin {
                *x *= scale;
            }
            for x in &mut s.values {
                *x *= scale;
            }
        }
        for b in &mut l.bbox {
            for x in b {
                *x *= scale;
            }
        }
        if let Some(f) = &mut l.flex {
            let sf = (1.0 + rng.normal() * u.stiffness.sigma_fraction).max(0.1);
            for k in &mut f.modal_stiffness {
                *k *= sf;
            }
        }
    }
    for j in &mut m.joints {
        let ff = (1.0 + rng.normal() * u.friction.sigma_fraction).max(0.0);
        j.physics.friction.coulomb *= ff;
        j.physics.friction.viscous *= ff;
        j.physics.friction.stribeck *= ff;
        j.physics.backlash *= (1.0 + rng.normal() * u.backlash.sigma_fraction).max(0.0);
        let sf = (1.0 + rng.normal() * u.stiffness.sigma_fraction).max(0.1);
        j.physics.stiffness.radial *= sf;
        j.physics.stiffness.axial *= sf;
        j.physics.stiffness.bending *= sf;
    }
    for mo in &mut m.motors {
        let tf = (1.0 + rng.normal() * u.motor_torque.sigma_fraction).max(0.1);
        mo.electrical.torque_constant *= tf;
        mo.electrical.back_emf_constant *= tf;
        mo.gearbox.backlash_rad *= (1.0 + rng.normal() * u.backlash.sigma_fraction).max(0.0);
    }
    m
}

fn stats(values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let std = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
    let pct = |p: f64| v[((p * (v.len() - 1) as f64).round() as usize).min(v.len() - 1)];
    json!({"mean": mean, "std": std, "p5": pct(0.05), "p50": pct(0.5), "p95": pct(0.95)})
}

/// Monte Carlo over the uncertainty block: `samples` perturbed runs of
/// `seconds` each; returns the `monte_carlo` block.
pub fn run_monte_carlo(model: &PhysicalModel, samples: usize, seed: u64, seconds: f64, opts: &BuildOptions) -> Result<Value, String> {
    let registry = registry();
    let mut rng = Rng::new(seed ^ model.uncertainty.seed);
    let mut metrics: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut successes = 0usize;
    let mut failures = Vec::new();
    for k in 0..samples {
        let sample = perturb(model, &mut rng);
        let mut robot = match PhysicalRobot::build(sample, &registry, opts) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("sample {k}: {e}"));
                continue;
            }
        };
        let mut ok = true;
        let chunks = (seconds / 0.05).ceil().max(1.0) as usize;
        for _ in 0..chunks {
            if let Err(e) = robot.advance(seconds / chunks as f64) {
                failures.push(format!("sample {k}: {e}"));
                ok = false;
                break;
            }
        }
        if !ok {
            metrics.entry("fell".into()).or_default().push(1.0);
            continue;
        }
        for (name, value) in robot.metrics() {
            metrics.entry(name).or_default().push(value);
        }
        if robot.success() {
            successes += 1;
        }
    }
    let mut out = serde_json::Map::new();
    for (k, v) in &metrics {
        out.insert(k.clone(), stats(v));
    }
    Ok(json!({"samples": samples, "seed": seed, "metrics": out, "success_rate": successes as f64 / samples.max(1) as f64, "failures": failures}))
}

// ---- Identification -------------------------------------------------------------

/// A logged run: time and per-joint measured angles and targets (rad),
/// optionally motor currents.
pub struct Log {
    pub t: Vec<f64>,
    pub angles: BTreeMap<String, Vec<f64>>,
    pub targets: BTreeMap<String, Vec<f64>>,
    pub currents: BTreeMap<String, Vec<f64>>,
}

pub fn read_log(path: &str) -> Result<Log, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = lines.next().ok_or("empty log")?.split(',').map(|s| s.trim().to_owned()).collect();
    let mut columns: Vec<Vec<f64>> = vec![Vec::new(); header.len()];
    for line in lines {
        for (k, cell) in line.split(',').enumerate() {
            if k < columns.len() {
                columns[k].push(cell.trim().parse().unwrap_or(f64::NAN));
            }
        }
    }
    let t_index = header.iter().position(|h| h == "t" || h == "time").ok_or("log needs a `t` column")?;
    let mut log = Log { t: columns[t_index].clone(), angles: BTreeMap::new(), targets: BTreeMap::new(), currents: BTreeMap::new() };
    for (k, h) in header.iter().enumerate() {
        if let Some(j) = h.strip_suffix(".angle") {
            log.angles.insert(j.to_owned(), columns[k].clone());
        } else if let Some(j) = h.strip_suffix(".target") {
            log.targets.insert(j.to_owned(), columns[k].clone());
        } else if let Some(m) = h.strip_suffix(".current") {
            log.currents.insert(m.to_owned(), columns[k].clone());
        }
    }
    Ok(log)
}

fn lerp_series(t: &[f64], y: &[f64], at: f64) -> f64 {
    if t.is_empty() {
        return 0.0;
    }
    if at <= t[0] {
        return y[0];
    }
    for k in 1..t.len() {
        if at <= t[k] {
            let s = if t[k] > t[k - 1] { (at - t[k - 1]) / (t[k] - t[k - 1]) } else { 0.0 };
            return y[k - 1] + (y[k] - y[k - 1]) * s;
        }
    }
    *y.last().unwrap()
}

/// Parameters the fit adjusts, per joint: coulomb, viscous, backlash,
/// stiffness scale; per motor: torque-constant scale.
fn apply_fit(model: &PhysicalModel, x: &[f64], joints: &[String], motors: &[String]) -> PhysicalModel {
    let mut m = model.clone();
    for (k, jname) in joints.iter().enumerate() {
        if let Some(j) = m.joints.iter_mut().find(|j| &j.name == jname) {
            j.physics.friction.coulomb = x[4 * k].exp();
            j.physics.friction.viscous = x[4 * k + 1].exp();
            j.physics.backlash = x[4 * k + 2].abs();
            let s = x[4 * k + 3].exp();
            j.physics.stiffness.radial *= s;
            j.physics.stiffness.axial *= s;
            j.physics.stiffness.bending *= s;
        }
    }
    for (k, mname) in motors.iter().enumerate() {
        if let Some(mo) = m.motors.iter_mut().find(|mo| &mo.name == mname) {
            let s = x[4 * joints.len() + k].exp();
            mo.electrical.torque_constant *= s;
            mo.electrical.back_emf_constant *= s;
        }
    }
    m
}

fn objective(model: &PhysicalModel, log: &Log, registry: &BehaviorRegistry, opts: &BuildOptions) -> f64 {
    let Ok(mut robot) = PhysicalRobot::build(model.clone(), registry, opts) else { return 1e6 };
    let end = *log.t.last().unwrap_or(&1.0);
    let mut err = 0.0;
    let mut count = 0.0;
    let dt = 0.02;
    let mut t = 0.0;
    while t < end {
        if robot.advance(dt).is_err() {
            return 1e6;
        }
        t += dt;
        let angles = robot.joint_angles();
        for (k, name) in robot.joint_names.iter().enumerate() {
            let short = name.trim_start_matches("joint.").trim_start_matches("slide.");
            if let Some(series) = log.angles.get(short) {
                err += (angles[k] - lerp_series(&log.t, series, t)).powi(2);
                count += 1.0;
            }
        }
        for m in &robot.motors {
            if let Some(series) = log.currents.get(&m.name) {
                err += 0.01 * (robot.runtime.get(m.current) - lerp_series(&log.t, series, t)).powi(2);
                count += 1.0;
            }
        }
    }
    if count == 0.0 { 1e6 } else { (err / count).sqrt() }
}

/// Nelder–Mead over friction, backlash, stiffness and torque constants so
/// the simulated trajectory follows the logged one. Returns the
/// `identification` block.
pub fn fit(model: &PhysicalModel, log: &Log, log_path: &str, opts: &BuildOptions, max_iterations: usize) -> Result<Value, String> {
    let registry = registry();
    let mut model = model.clone();
    // Drive the model with the log's targets.
    model.control.mode = "trajectory".into();
    model.control.trajectory = log.t.iter().enumerate().map(|(k, t)| sim_domain_robot::model::TrajectoryPoint { t: *t, targets: log.targets.iter().map(|(j, s)| (j.clone(), s[k])).collect() }).collect();
    let joints: Vec<String> = model.joints.iter().filter(|j| !j.is_loop() && log.angles.contains_key(&j.name)).map(|j| j.name.clone()).collect();
    if joints.is_empty() {
        return Err("the log has no `<joint>.angle` column matching a joint".into());
    }
    let motors: Vec<String> = model.motors.iter().filter(|m| log.currents.contains_key(&m.name)).map(|m| m.name.clone()).collect();
    let mut x0 = Vec::new();
    for jname in &joints {
        let j = model.joint(jname).unwrap();
        x0.extend([j.physics.friction.coulomb.max(1e-5).ln(), j.physics.friction.viscous.max(1e-5).ln(), j.physics.backlash, 0.0]);
    }
    for _ in &motors {
        x0.push(0.0);
    }
    let n = x0.len();
    let f = |x: &[f64]| objective(&apply_fit(&model, x, &joints, &motors), log, &registry, opts);
    // Initial simplex.
    let mut simplex: Vec<(Vec<f64>, f64)> = Vec::with_capacity(n + 1);
    simplex.push((x0.clone(), f(&x0)));
    for k in 0..n {
        let mut x = x0.clone();
        x[k] += if k % 4 == 2 { 0.002 } else { 0.5 };
        let val = f(&x);
        simplex.push((x, val));
    }
    for _ in 0..max_iterations {
        simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
        let best = simplex[0].1;
        let worst = simplex[n].1;
        if (worst - best).abs() < 1e-5 * best.max(1e-9) {
            break;
        }
        let centroid: Vec<f64> = (0..n).map(|k| simplex[..n].iter().map(|s| s.0[k]).sum::<f64>() / n as f64).collect();
        let reflect: Vec<f64> = (0..n).map(|k| centroid[k] + (centroid[k] - simplex[n].0[k])).collect();
        let fr = f(&reflect);
        if fr < simplex[0].1 {
            let expand: Vec<f64> = (0..n).map(|k| centroid[k] + 2.0 * (centroid[k] - simplex[n].0[k])).collect();
            let fe = f(&expand);
            simplex[n] = if fe < fr { (expand, fe) } else { (reflect, fr) };
        } else if fr < simplex[n - 1].1 {
            simplex[n] = (reflect, fr);
        } else {
            let contract: Vec<f64> = (0..n).map(|k| centroid[k] + 0.5 * (simplex[n].0[k] - centroid[k])).collect();
            let fc = f(&contract);
            if fc < simplex[n].1 {
                simplex[n] = (contract, fc);
            } else {
                let best_x = simplex[0].0.clone();
                for s in simplex.iter_mut().skip(1) {
                    for k in 0..n {
                        s.0[k] = best_x[k] + 0.5 * (s.0[k] - best_x[k]);
                    }
                    s.1 = f(&s.0);
                }
            }
        }
    }
    simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (x, rms) = &simplex[0];
    let fitted = apply_fit(&model, x, &joints, &motors);
    let mut out = serde_json::Map::new();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    for (k, jname) in joints.iter().enumerate() {
        let j = fitted.joint(jname).unwrap();
        let mut entry = json!({
            "friction": j.physics.friction,
            "backlash": j.physics.backlash,
            "stiffness_scale": x[4 * k + 3].exp(),
            "rms_error_rad": rms,
            "source_log": log_path,
            "fitted_at": format!("{now}"),
        });
        if let Some(mi) = fitted.motors.iter().position(|m| m.joint.as_deref() == Some(jname.as_str())) {
            if let Some(pos) = motors.iter().position(|m| *m == fitted.motors[mi].name) {
                entry["torque_constant_scale"] = json!(x[4 * joints.len() + pos].exp());
            }
        }
        out.insert(jname.clone(), entry);
    }
    Ok(Value::Object(out))
}
