//! Automatic test benches and datasheets for library parts.
//!
//! The bench is chosen from a part's ports:
//! - **motor** (two electrical + rotational): no-load, stall, speed–torque
//!   curve with efficiency, and an efficiency map over voltage and load;
//! - **gear** (two rotational): ratio, efficiency driven from each side,
//!   back-drive or self-locking;
//! - **linear** (rotational + translational): the same for rotation ↔ travel;
//! - **every part**: an energy audit (every port on a lossless environment
//!   with stored energy; a passive part may not raise the total) and a
//!   step-halving check.
//!
//! Every run goes through the shared runtime from a generated system
//! document, so benches exercise exactly what users build with.
use crate::system_builder;
use serde::{Deserialize, Serialize};
use sim_core::{BehaviorRegistry, ConnectorKind, PortSchema};
use sim_system::{Command, InstanceSpec, SystemDocument, Terminal};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "sim.datasheet/1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Datasheet {
    pub schema: String,
    pub component_type: String,
    pub label: String,
    /// motor, gear, linear or generic.
    pub kind: String,
    /// Parameter values the bench used (typical values, else defaults).
    pub parameters: BTreeMap<String, f64>,
    pub conditions: BTreeMap<String, f64>,
    pub values: Vec<Value>,
    pub curves: Vec<Curve>,
    pub checks: Vec<Check>,
    /// Hash of the part's definition when authored (`.part` source).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    /// Parameters taken from a CAD derivation (`sim.cad-physics/1`): its
    /// file, the CAD file and hash it was derived from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Value {
    pub name: String,
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub name: String,
    pub x_label: String,
    pub y_label: String,
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

impl Datasheet {
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }
    pub fn value(&self, name: &str) -> Option<f64> {
        self.values.iter().find(|v| v.name == name).map(|v| v.value)
    }
}

struct Ports {
    electrical: Vec<String>,
    rotational: Vec<String>,
    translational: Vec<String>,
    thermal: Vec<String>,
    other: Vec<String>,
    signals_in: Vec<String>,
}

fn ports(registry: &BehaviorRegistry, component_type: &str) -> Result<Ports, String> {
    let all = sim_system::resolve::element_top_ports(registry, component_type, &Default::default()).map_err(|e| e.to_string())?;
    let mut p = Ports { electrical: vec![], rotational: vec![], translational: vec![], thermal: vec![], other: vec![], signals_in: vec![] };
    for (name, schema) in all {
        match schema {
            PortSchema::Acausal(k) if k == ConnectorKind::Electrical => p.electrical.push(name),
            PortSchema::Acausal(k) if k == ConnectorKind::Rotational => p.rotational.push(name),
            PortSchema::Acausal(k) if k == ConnectorKind::Translational => p.translational.push(name),
            PortSchema::Acausal(k) if k == ConnectorKind::Thermal => p.thermal.push(name),
            PortSchema::Acausal(_) => p.other.push(name),
            PortSchema::SignalIn(_) => p.signals_in.push(name),
            PortSchema::SignalOut(_) => {}
        }
    }
    Ok(p)
}

/// Prefer a named port, else the first.
fn pick(list: &[String], names: &[&str]) -> Option<String> {
    names.iter().find_map(|n| list.iter().find(|p| p == n).cloned()).or_else(|| list.first().cloned())
}

/// Typical values, else defaults; every required parameter must be covered.
pub fn bench_parameters(registry: &BehaviorRegistry, component_type: &str) -> Result<BTreeMap<String, f64>, String> {
    let d = registry.get(&component_type.into()).map_err(|e| e.to_string())?;
    let values = sim_system::library::effective_parameters(registry, component_type, &Default::default());
    for p in d.parameters.iter().flatten() {
        if p.required && !p.implementation_reference && !p.name.contains('*') && !values.contains_key(&p.name) {
            return Err(format!("`{}` has no typical value in the notes; add one so the bench can run", p.name));
        }
    }
    Ok(values.into_iter().filter(|(k, _)| d.parameters.iter().flatten().any(|p| &p.name == k && !p.name.starts_with("initial."))).collect())
}

/// Builds bench documents: the part under test is instance `dut`.
struct Rig {
    doc: SystemDocument,
    commands: Vec<Command>,
    n: usize,
}

impl Rig {
    fn new(component_type: &str, parameters: &BTreeMap<String, f64>) -> Self {
        let mut spec = InstanceSpec::element(component_type);
        for (k, v) in parameters {
            spec = spec.with(k, *v);
        }
        let mut doc = SystemDocument::new("bench");
        doc.run = Some(sim_system::RunSettings { integrator: sim_system::IntegratorChoice::BackwardEuler, interval: 1e-3, absolute_tolerance: None, relative_tolerance: None, max_iterations: None, rationale: "Bench: L-stable for stiff parts and grounded shafts.".into() });
        Rig { doc, commands: vec![Command::AddInstance { at: String::new(), name: "dut".into(), instance: spec }], n: 0 }
    }
    fn add(&mut self, spec: InstanceSpec) -> String {
        self.n += 1;
        let name = format!("b{}", self.n);
        self.commands.push(Command::AddInstance { at: String::new(), name: name.clone(), instance: spec });
        name
    }
    fn join(&mut self, a: (&str, &str), b: (&str, &str)) {
        self.commands.push(Command::Connect { at: String::new(), terminals: vec![Terminal::port(a.0, a.1), Terminal::port(b.0, b.1)], label: String::new() });
    }
    fn build(mut self, registry: &BehaviorRegistry) -> Result<SystemDocument, String> {
        sim_system::apply(&mut self.doc, registry, &self.commands).map_err(|e| e.to_string())?;
        Ok(self.doc)
    }
}

fn steady(doc: &SystemDocument, registry: &BehaviorRegistry, duration: f64, interval: f64, observe: &[&str]) -> Result<BTreeMap<String, f64>, String> {
    let mut config = system_builder::config_for(doc);
    config.interval = interval;
    let series = system_builder::simulate(doc, registry, duration, config, &observe.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
    let mut out = BTreeMap::new();
    for key in observe {
        let s = series.iter().find(|s| s.label == *key).ok_or_else(|| format!("bench did not record {key}"))?;
        let tail: Vec<f64> = s.times.iter().zip(&s.values).filter(|(t, _)| **t >= 0.8 * duration).map(|(_, v)| *v).collect();
        out.insert(key.to_string(), tail.iter().sum::<f64>() / tail.len().max(1) as f64);
    }
    Ok(out)
}

fn motor_bench(registry: &BehaviorRegistry, t: &str, p: &Ports, params: &BTreeMap<String, f64>, sheet: &mut Datasheet) -> Result<(), String> {
    let (ep, en) = (pick(&p.electrical, &["p"]).unwrap(), p.electrical.iter().find(|x| **x != pick(&p.electrical, &["p"]).unwrap()).cloned().unwrap());
    let shaft = pick(&p.rotational, &["shaft"]).unwrap();
    let case = p.rotational.iter().find(|x| **x != shaft).cloned();
    let volts = 12.0;
    sheet.conditions.insert("test voltage (V)".into(), volts);
    sheet.conditions.insert("thermal ports on a 10 kJ/K mass starting at (K)".into(), 293.15);
    // One rig: supply, ground, grounded case, thermal ports at 20 °C, and a
    // shaft either held (stall) or on a small inertia with a load torque.
    let rig = |voltage: f64, load: Option<f64>| -> Result<SystemDocument, String> {
        let mut r = Rig::new(t, params);
        let supply = r.add(InstanceSpec::element("electrical.voltage_source").with("voltage", voltage));
        let gnd = r.add(InstanceSpec::element("electrical.ground"));
        r.join((&supply, "p"), ("dut", &ep));
        r.join((&supply, "n"), ("dut", &en));
        r.join((&gnd, "pin"), ("dut", &en));
        if let Some(case) = &case {
            let frame = r.add(InstanceSpec::element("rotational.ground"));
            r.join((&frame, "flange"), ("dut", case));
        }
        // Thermal ports sit on a 10 kJ/K mass at 20 °C: effectively held
        // there for the bench's seconds, and (unlike a pinned ambient) well
        // conditioned when a stalled low-resistance motor dumps kilowatts.
        for h in &p.thermal {
            let mass = r.add(InstanceSpec::element("thermal.capacitance").with("heat_capacity", 1.0e4).with("initial.temperature", 293.15));
            r.join((&mass, "node"), ("dut", h));
        }
        match load {
            None => {
                let hold = r.add(InstanceSpec::element("rotational.ground"));
                r.join((&hold, "flange"), ("dut", &shaft));
            }
            Some(torque) => {
                let rotor = r.add(InstanceSpec::element("rotational.inertia").with("inertia", 1e-5));
                r.join((&rotor, "shaft"), ("dut", &shaft));
                if torque != 0. {
                    let l = r.add(InstanceSpec::element("rotational.load_torque").with("torque", -torque));
                    r.join((&l, "shaft"), ("dut", &shaft));
                }
            }
        }
        r.build(registry)
    };
    let current = format!("dut.{ep}.current");
    let speed = format!("dut.{shaft}.speed");
    let stall = steady(&rig(volts, None)?, registry, 0.5, 1e-3, &[&current])?;
    let i_stall = stall[&current];
    let no_load = steady(&rig(volts, Some(0.))?, registry, 3.0, 1e-3, &[&current, &speed])?;
    let (w0, i0) = (no_load[&speed], no_load[&current]);
    // Stall torque from the no-load/stall line would assume linearity; measure it:
    // the torque the held shaft reacts is k·i, and k is recovered from the
    // no-load back-EMF: k ≈ (V − R·i0)/ω0 with R = V / i_stall.
    let r_est = volts / i_stall;
    let k_est = (volts - r_est * i0) / w0;
    let t_stall = k_est * i_stall;
    sheet.values.extend([
        Value { name: "no-load speed".into(), value: w0, unit: "rad/s".into() },
        Value { name: "no-load current".into(), value: i0, unit: "A".into() },
        Value { name: "stall current".into(), value: i_stall, unit: "A".into() },
        Value { name: "stall torque".into(), value: t_stall, unit: "N·m".into() },
        Value { name: "terminal resistance (from stall)".into(), value: r_est, unit: "Ω".into() },
        Value { name: "torque constant (from no-load)".into(), value: k_est, unit: "N·m/A".into() },
    ]);
    let fractions = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
    let mut speed_torque = Vec::new();
    let mut efficiency = Vec::new();
    let mut current_torque = Vec::new();
    let mut best = (0., 0.);
    for f in fractions {
        let torque = f * t_stall;
        let s = steady(&rig(volts, Some(torque))?, registry, 3.0, 1e-3, &[&current, &speed])?;
        let (w, i) = (s[&speed], s[&current]);
        let eta = if i > 0. { torque * w / (volts * i) } else { 0. };
        speed_torque.push([torque, w]);
        current_torque.push([torque, i]);
        efficiency.push([torque, eta]);
        if eta > best.0 {
            best = (eta, torque);
        }
    }
    sheet.values.push(Value { name: "peak efficiency".into(), value: best.0, unit: "1".into() });
    sheet.values.push(Value { name: "torque at peak efficiency".into(), value: best.1, unit: "N·m".into() });
    sheet.curves.push(Curve { name: "speed–torque".into(), x_label: "torque (N·m)".into(), y_label: "speed (rad/s)".into(), points: speed_torque.clone() });
    sheet.curves.push(Curve { name: "current–torque".into(), x_label: "torque (N·m)".into(), y_label: "current (A)".into(), points: current_torque });
    sheet.curves.push(Curve { name: "efficiency–torque".into(), x_label: "torque (N·m)".into(), y_label: "efficiency".into(), points: efficiency });
    // Efficiency map: voltage × load fraction (of that voltage's stall torque).
    for v in [0.25, 0.5, 0.75] {
        let mut map = Vec::new();
        for f in [0.1, 0.3, 0.5, 0.7] {
            let torque = f * t_stall * v;
            let s = steady(&rig(volts * v, Some(torque))?, registry, 3.0, 1e-3, &[&current, &speed])?;
            map.push([torque, torque * s[&speed] / (volts * v * s[&current])]);
        }
        sheet.curves.push(Curve { name: format!("efficiency at {} V", volts * v), x_label: "torque (N·m)".into(), y_label: "efficiency".into(), points: map });
    }
    // Physical consistency: speed falls as load rises; efficiency within (0, 1).
    let monotone = speed_torque.windows(2).all(|w| w[1][1] <= w[0][1] + 1e-9);
    sheet.checks.push(Check { name: "speed falls with load".into(), passed: monotone, detail: format!("{} points", speed_torque.len()) });
    sheet.checks.push(Check { name: "efficiency below 100 %".into(), passed: best.0 < 1.0 && best.0 > 0.0, detail: format!("peak {:.1} %", 100. * best.0) });
    Ok(())
}

fn transmission_bench(registry: &BehaviorRegistry, t: &str, input: &str, output: &str, output_linear: bool, params: &BTreeMap<String, f64>, sheet: &mut Datasheet) -> Result<(), String> {
    // Drive one side with a constant torque (or force) against a small
    // inertia; load the other side with a viscous damper to ground. At steady
    // state the damper absorbs b·v², the drive supplies τ·ω.
    let (drive_torque, b_in, b_out) = (0.05, 1e-4, if output_linear { 50.0 } else { 0.01 });
    let rig = |forward: bool, drive: f64| -> Result<SystemDocument, String> {
        let mut r = Rig::new(t, params);
        let rotor = r.add(InstanceSpec::element("rotational.inertia").with("inertia", 1e-5));
        r.join((&rotor, "shaft"), ("dut", input));
        let (mass_type, mass_param, mass) = if output_linear { ("translational.mass", "mass", 0.05) } else { ("rotational.inertia", "inertia", 1e-4) };
        let follower = r.add(InstanceSpec::element(mass_type).with(mass_param, mass));
        r.join((&follower, if output_linear { "axis" } else { "shaft" }), ("dut", output));
        let (damped, damper_type, ground_type) = if forward {
            (output, if output_linear { "translational.damper" } else { "rotational.damper" }, if output_linear { "translational.ground" } else { "rotational.ground" })
        } else {
            (input, "rotational.damper", "rotational.ground")
        };
        let damper = r.add(InstanceSpec::element(damper_type).with("damping", if forward { b_out } else { b_in }));
        let ground = r.add(InstanceSpec::element(ground_type));
        r.join((&damper, "a"), ("dut", damped));
        r.join((&damper, "b"), (&ground, if ground_type == "rotational.ground" { "flange" } else { "axis" }));
        let (load_type, load_param, at) = if forward { ("rotational.load_torque", "torque", input) } else if output_linear { ("translational.load_force", "force", output) } else { ("rotational.load_torque", "torque", output) };
        let l = r.add(InstanceSpec::element(load_type).with(load_param, drive));
        r.join((&l, if load_type == "translational.load_force" { "axis" } else { "shaft" }), ("dut", at));
        r.build(registry)
    };
    let vin = format!("dut.{input}.speed");
    let vout = format!("dut.{output}.{}", if output_linear { "velocity" } else { "speed" });
    let fwd = steady(&rig(true, drive_torque)?, registry, 4.0, 1e-3, &[&vin, &vout])?;
    let ratio = fwd[&vin] / fwd[&vout];
    let eta_f = b_out * fwd[&vout].powi(2) / (drive_torque * fwd[&vin]);
    // Back-drive with the load that the forward case delivered, scaled to the output.
    let back_drive = drive_torque * ratio.abs();
    let back = steady(&rig(false, back_drive)?, registry, 4.0, 1e-3, &[&vin, &vout])?;
    let moved = back[&vout].abs() > 1e-3 * fwd[&vout].abs();
    let eta_b = if moved { b_in * back[&vin].powi(2) / (back_drive * back[&vout]) } else { 0. };
    let unit = if output_linear { "rad/m" } else { "1" };
    sheet.conditions.insert("drive torque (N·m)".into(), drive_torque);
    sheet.values.extend([
        Value { name: "ratio (measured)".into(), value: ratio, unit: unit.into() },
        Value { name: "forward efficiency".into(), value: eta_f, unit: "1".into() },
        Value { name: "backdrive efficiency".into(), value: eta_b.max(0.), unit: "1".into() },
        Value { name: "self-locking".into(), value: if moved { 0. } else { 1. }, unit: "yes=1".into() },
        Value { name: "backdrive output speed".into(), value: back[&vout], unit: if output_linear { "m/s" } else { "rad/s" }.into() },
    ]);
    sheet.checks.push(Check { name: "efficiency within (0, 1]".into(), passed: eta_f > 0. && eta_f <= 1. + 1e-6, detail: format!("forward {:.2} %", 100. * eta_f) });
    // Agreement with the notes' derived values, where they exist.
    if let Some(notes) = registry.get(&t.into()).ok().and_then(|d| d.notes) {
        let derived = notes.derive(params);
        if let Some(d) = derived.iter().find(|d| d.name.contains("forward efficiency")) {
            let expected = d.value / 100.;
            sheet.checks.push(Check { name: "forward efficiency matches the notes".into(), passed: (eta_f - expected).abs() < 0.01, detail: format!("bench {:.2} % vs notes {:.2} %", 100. * eta_f, d.value) });
        }
        if let Some(d) = derived.iter().find(|d| d.name == "self-locking") {
            let expected = d.value >= 0.5;
            sheet.checks.push(Check { name: "self-locking matches the notes".into(), passed: expected == !moved, detail: format!("bench {}, notes {}", if moved { "back-drives" } else { "holds" }, if expected { "locks" } else { "back-drivable" }) });
        }
    }
    Ok(())
}

/// Every port on a lossless store of energy: capacitors (electrical),
/// spinning inertias, moving masses, thermal capacitances. Returns
/// (initial energy excluding the thermal baseline, final − initial, final
/// port rates) for step `h`.
fn energy_audit(registry: &BehaviorRegistry, t: &str, p: &Ports, params: &BTreeMap<String, f64>, h: f64, excite: bool) -> Result<(f64, f64, Vec<f64>), String> {
    // Ports the part moves itself (an inertia's or a mass's own speed) get
    // no second body; the part starts at its own initial speed instead.
    let own = sim_system::snap::providing_ports(registry, t);
    let mut params = params.clone();
    let declared: Vec<String> = registry.get(&t.into()).map(|d| d.parameters.iter().flatten().map(|p| p.name.clone()).collect()).unwrap_or_default();
    if excite {
        for name in ["initial.speed", "initial.velocity"] {
            if declared.iter().any(|d| d == name) {
                params.insert(name.into(), if name == "initial.speed" { 10.0 } else { 0.2 });
            }
        }
    }
    let mut r = Rig::new(t, &params);
    let gnd = if p.electrical.is_empty() { None } else { Some(r.add(InstanceSpec::element("electrical.ground"))) };
    let mut rates = Vec::new();
    for (k, port) in p.electrical.iter().enumerate() {
        // Large storage: slow enough that fast LC rings do not swamp the convergence check.
        let c = r.add(InstanceSpec::element("electrical.capacitor").with("capacitance", 1.0).with("initial.p.voltage", if excite { 0.3 + 0.4 * k as f64 } else { 0. }));
        r.join((&c, "p"), ("dut", port));
        r.join((&c, "n"), (gnd.as_deref().unwrap(), "pin"));
        rates.push(format!("dut.{port}.voltage"));
    }
    for (k, port) in p.rotational.iter().enumerate() {
        rates.push(format!("dut.{port}.speed"));
        if own.contains(port) {
            // A spring to a second inertia: lossless, on its own node.
            let (k, j) = (r.add(InstanceSpec::element("rotational.spring").with("stiffness", 1.0)), r.add(InstanceSpec::element("rotational.inertia").with("inertia", 1e-4)));
            r.join((&k, "a"), ("dut", port));
            r.join((&k, "b"), (&j, "shaft"));
            continue;
        }
        let j = r.add(InstanceSpec::element("rotational.inertia").with("inertia", 1e-3).with("initial.speed", if excite { 10.0 + 7.0 * k as f64 } else { 0. }));
        r.join((&j, "shaft"), ("dut", port));
    }
    for (k, port) in p.translational.iter().enumerate() {
        rates.push(format!("dut.{port}.velocity"));
        if own.contains(port) {
            let (k, m) = (r.add(InstanceSpec::element("translational.spring").with("stiffness", 10.0)), r.add(InstanceSpec::element("translational.mass").with("mass", 0.1)));
            r.join((&k, "a"), ("dut", port));
            r.join((&k, "b"), (&m, "axis"));
            continue;
        }
        let m = r.add(InstanceSpec::element("translational.mass").with("mass", 0.1).with("initial.velocity", if excite { 0.2 + 0.1 * k as f64 } else { 0. }));
        r.join((&m, "axis"), ("dut", port));
    }
    for port in &p.thermal {
        // A part that holds its node (an ambient) sets the temperature itself.
        let mut spec = InstanceSpec::element("thermal.capacitance").with("heat_capacity", 1.0);
        if excite {
            spec = spec.with("initial.temperature", 300.0);
        }
        let c = r.add(spec);
        r.join((&c, "node"), ("dut", port));
    }
    let mut doc = r.build(registry)?;
    // The audit uses the implicit midpoint rule: second order and free of
    // numerical damping, so energy changes are the part's own and halving the
    // step is a fair accuracy check.
    if let Some(run) = &mut doc.run {
        run.integrator = sim_system::IntegratorChoice::ImplicitMidpoint;
    }
    let flat = sim_system::flatten(&doc, registry).map_err(|e| e.to_string())?;
    let config = system_builder::config_for(&doc);
    let mut runtime = sim_compile::Runtime::new(flat.model, registry, config.integrator).map_err(|e| e.to_string())?;
    let thermal_baseline = 300.0 * p.thermal.len() as f64;
    let e0 = runtime.energy() - thermal_baseline;
    runtime.advance(0.5, h).map_err(|e| e.to_string())?;
    let e1 = runtime.energy() - thermal_baseline;
    // Final potential (electrical) or speed (mechanical) of every port.
    let finals = rates
        .iter()
        .filter_map(|key| {
            let (port, lane) = key.strip_prefix("dut.")?.split_once('.')?;
            let id = flat.ports.get(&format!("dut#{port}"))?;
            Some(runtime.get(runtime.across_lane_id(*id, if lane == "voltage" { 0 } else { 1 })))
        })
        .collect();
    Ok((e0, e1 - e0, finals))
}

/// Run the bench that fits this part and the checks every part gets.
pub fn datasheet(registry: &BehaviorRegistry, component_type: &str) -> Result<Datasheet, String> {
    datasheet_with(registry, component_type, &BTreeMap::new(), None)
}

/// A datasheet for a configured part: `overrides` replace the typical and
/// default values (e.g. geometry derived from CAD).
pub fn datasheet_with(registry: &BehaviorRegistry, component_type: &str, overrides: &BTreeMap<String, f64>, derived_from: Option<serde_json::Value>) -> Result<Datasheet, String> {
    let d = registry.get(&component_type.into()).map_err(|e| e.to_string())?;
    let p = ports(registry, component_type)?;
    let mut params = bench_parameters(registry, component_type)?;
    for (k, v) in overrides {
        if !d.parameters.iter().flatten().any(|p| &p.name == k) {
            return Err(format!("{component_type} has no parameter `{k}`"));
        }
        params.insert(k.clone(), *v);
    }
    let active = d.notes.is_some_and(|n| n.active);
    let mut sheet = Datasheet {
        schema: SCHEMA.into(),
        component_type: component_type.into(),
        label: d.display_name.to_string(),
        kind: "generic".into(),
        parameters: params.clone(),
        conditions: BTreeMap::new(),
        values: Vec::new(),
        curves: Vec::new(),
        checks: Vec::new(),
        source_hash: sim_parts::definition(component_type).map(|d| d.source_hash.clone()),
        derived_from,
    };
    if !p.signals_in.is_empty() {
        sheet.checks.push(Check { name: "bench".into(), passed: true, detail: format!("driven by signal input(s) {}: benches need a fixed drive, so only the ports are documented", p.signals_in.join(", ")) });
        return Ok(sheet);
    }
    if !p.other.is_empty() {
        sheet.checks.push(Check { name: "bench".into(), passed: true, detail: format!("ports of kinds the bench has no environment for: {}", p.other.join(", ")) });
        return Ok(sheet);
    }
    if p.electrical.len() == 2 && !p.rotational.is_empty() && p.translational.is_empty() {
        sheet.kind = "motor".into();
        motor_bench(registry, component_type, &p, &params, &mut sheet)?;
    } else if p.electrical.is_empty() && p.rotational.len() == 2 && p.translational.is_empty() && p.thermal.is_empty() && !(p.rotational.contains(&"a".to_string()) && p.rotational.contains(&"b".to_string())) {
        // Two shafts named as a drive (input/output, worm/wheel), not the
        // symmetric a/b of springs, dampers and meshes.
        sheet.kind = "gear".into();
        let input = pick(&p.rotational, &["input", "worm", "a"]).unwrap();
        let output = p.rotational.iter().find(|x| **x != input).cloned().unwrap();
        transmission_bench(registry, component_type, &input, &output, false, &params, &mut sheet)?;
    } else if p.electrical.is_empty() && p.rotational.len() == 1 && p.translational.len() == 1 {
        sheet.kind = "linear".into();
        transmission_bench(registry, component_type, &p.rotational[0], &p.translational[0], true, &params, &mut sheet)?;
    }
    // The part's realtime model (notes' realtime values): rerun the same
    // bench and report how far its results move.
    let realtime = d.notes.map(|n| n.realtime).unwrap_or_default();
    if !realtime.is_empty() && sheet.kind != "generic" {
        let mut fast = params.clone();
        for (k, v) in realtime {
            fast.insert(k.to_string(), *v);
        }
        let mut other = Datasheet { values: Vec::new(), curves: Vec::new(), checks: Vec::new(), conditions: BTreeMap::new(), derived_from: None, ..sheet.clone() };
        match sheet.kind.as_str() {
            "motor" => motor_bench(registry, component_type, &p, &fast, &mut other)?,
            "gear" => {
                let input = pick(&p.rotational, &["input", "worm", "a"]).unwrap();
                let output = p.rotational.iter().find(|x| **x != input).cloned().unwrap();
                transmission_bench(registry, component_type, &input, &output, false, &fast, &mut other)?
            }
            _ => transmission_bench(registry, component_type, &p.rotational[0], &p.translational[0], true, &fast, &mut other)?,
        }
        // Creep of a locked drive exists only because of the smoothing; skip it.
        let worst = sheet
            .values
            .iter()
            .filter(|v| v.name != "backdrive output speed")
            .filter_map(|v| other.value(&v.name).map(|w| (v.name.clone(), (v.value - w).abs() / v.value.abs().max(w.abs()).max(1e-3))))
            .fold((String::new(), 0f64), |m, x| if x.1 > m.1 { x } else { m });
        sheet.values.push(Value { name: "realtime model: largest change".into(), value: worst.1, unit: "1".into() });
        sheet.checks.push(Check { name: "realtime model".into(), passed: worst.1 < 0.02, detail: format!("{} → largest bench change {:.3} % ({})", realtime.iter().map(|(k, v)| format!("{k} = {v}")).collect::<Vec<_>>().join(", "), 100. * worst.1, if worst.0.is_empty() { "none".into() } else { worst.0 }) });
    }
    // Energy audit and step convergence, for every part, at four steps.
    let steps = [1e-3, 5e-4, 2.5e-4, 1.25e-4];
    let excited = energy_audit(registry, component_type, &p, &params, steps[0], true).is_ok();
    let runs: Vec<(f64, f64, Vec<f64>)> = steps.iter().map(|h| energy_audit(registry, component_type, &p, &params, *h, excited)).collect::<Result<_, _>>()?;
    let e0 = runs[0].0;
    let tolerance = 1e-6 + 1e-4 * e0.abs();
    let changes: Vec<f64> = runs.iter().map(|r| r.1).collect();
    // Numerical energy error shrinks as the step is refined; a part that
    // really creates energy keeps doing so at every step.
    let finest = *changes.last().unwrap();
    let shrinking = changes[0] > 0. && finest < 0.3 * changes[0];
    let created = finest > tolerance && !shrinking;
    sheet.values.push(Value { name: "audit energy change".into(), value: finest, unit: "J".into() });
    sheet.checks.push(Check {
        name: "energy audit".into(),
        passed: active || !created,
        detail: if !excited {
            "holds its node(s): audited at rest".into()
        } else if created {
            format!("total energy rose by {finest:.3e} J from {e0:.3e} J at every step (1 → 0.125 ms: {}){}", changes.iter().map(|c| format!("{c:.2e}")).collect::<Vec<_>>().join(", "), if active { " (a source: allowed)" } else { ": a passive part may not create energy" })
        } else if finest > tolerance {
            format!("energy rose {:.2e} → {:.2e} J as the step fell 1 → 0.125 ms: numerical, vanishing with the step", changes[0], finest)
        } else {
            format!("energy {e0:.3e} J → change {finest:.3e} J (dissipated or conserved)")
        },
    });
    // Differences between successive steps must shrink (convergence), or be small already.
    let difference = |a: &Vec<f64>, b: &Vec<f64>| {
        let scale = a.iter().chain(b).fold(1e-9f64, |m, v| m.max(v.abs()));
        a.iter().zip(b).fold(0f64, |m, (x, y)| m.max((x - y).abs())) / scale
    };
    let diffs: Vec<f64> = runs.windows(2).map(|w| difference(&w[0].2, &w[1].2)).collect();
    let converging = diffs[2] < 0.02 || (diffs[2] < 0.6 * diffs[0]);
    let order = if diffs[2] > 0. && diffs[1] > 0. { (diffs[1] / diffs[2]).log2() } else { f64::NAN };
    sheet.checks.push(Check {
        name: "step convergence".into(),
        passed: converging,
        detail: format!("port-rate differences between successive steps {:.1e}, {:.1e}, {:.1e} (1 → 0.125 ms); observed order {:.1}", diffs[0], diffs[1], diffs[2], order),
    });
    Ok(sheet)
}

/// Parameter values and provenance record from a `sim.cad-physics/1` file.
pub fn cad_parameters(path: &std::path::Path) -> Result<(String, BTreeMap<String, f64>, serde_json::Value), String> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| e.to_string())?;
    if v["schema"] != "sim.cad-physics/1" {
        return Err(format!("{} is not a sim.cad-physics/1 file", path.display()));
    }
    let component = v["component_type"].as_str().ok_or("component_type")?.to_string();
    let params = v["parameters"].as_object().ok_or("parameters")?.iter().filter_map(|(k, p)| p["value"].as_f64().map(|x| (k.clone(), x))).collect();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok((component, params, serde_json::json!({"path": path.display().to_string(), "blake3": blake3::hash(&bytes).to_hex().to_string(), "cad": v["cad"], "provenance": v["parameters"].as_object().map(|o| o.iter().map(|(k, p)| (k.clone(), p["provenance"].clone())).collect::<serde_json::Map<_, _>>())})))
}

/// Where a part's datasheet is kept.
pub fn path(dir: &std::path::Path, component_type: &str) -> std::path::PathBuf {
    dir.join(format!("{component_type}.json"))
}

/// Parts that should have a datasheet: every registry element with notes.
pub fn noted(registry: &BehaviorRegistry) -> Vec<String> {
    registry.descriptors().filter(|d| d.notes.is_some()).map(|d| d.type_id.0.clone()).collect()
}

/// Compare a fresh datasheet with a stored one: values and curve points
/// within `tolerance` (relative), identical checks.
pub fn compare(fresh: &Datasheet, stored: &Datasheet, tolerance: f64) -> Result<(), String> {
    let close = |a: f64, b: f64| (a - b).abs() <= tolerance * (a.abs().max(b.abs())).max(1e-12) || (a - b).abs() < 1e-12;
    for v in &fresh.values {
        let Some(s) = stored.value(&v.name) else { return Err(format!("{}: new value `{}`", fresh.component_type, v.name)) };
        if !close(v.value, s) && v.name != "audit energy change" {
            return Err(format!("{}: `{}` changed {} → {}", fresh.component_type, v.name, s, v.value));
        }
    }
    for c in &fresh.curves {
        let Some(s) = stored.curves.iter().find(|x| x.name == c.name) else { return Err(format!("{}: new curve `{}`", fresh.component_type, c.name)) };
        for (a, b) in c.points.iter().zip(&s.points) {
            if !close(a[1], b[1]) {
                return Err(format!("{}: curve `{}` changed at x = {}: {} → {}", fresh.component_type, c.name, a[0], b[1], a[1]));
            }
        }
    }
    let names = |d: &Datasheet| d.checks.iter().map(|c| (c.name.clone(), c.passed)).collect::<Vec<_>>();
    if names(fresh) != names(stored) {
        return Err(format!("{}: checks changed {:?} → {:?}", fresh.component_type, names(stored), names(fresh)));
    }
    Ok(())
}
