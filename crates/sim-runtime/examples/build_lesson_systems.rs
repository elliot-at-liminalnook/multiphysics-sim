//! Builds the system files for the robot-physics lessons (one per lesson
//! folder under `lessons/`). Every value is a representative estimate for a
//! hobby-robot part, recorded with its reason; none is a measured unit.
//!
//!     cargo run -p sim-runtime --example build_lesson_systems [SLUG…]
//!
//! With slugs, only those lessons' systems are rewritten.
use sim_system::{Appearance, Command, Definition, InstanceSpec, ParameterBinding, SystemDocument, Terminal};
use sim_inspect::spatial::SpatialShape;
use std::path::Path;

const S: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// Local +Y (a part's axis) turned to world +X.
const X: [f32; 4] = [0., 0., -S, S];
/// Local +Y turned to world +Z (a horizontal joint axis facing the camera).
const Z: [f32; 4] = [S, 0., 0., S];
/// Local +Y stays world +Y (vertical axis; sliding parts move up and down).
const Y: [f32; 4] = [0., 0., 0., 1.];

const CAN: &str = "Representative 12 V can motor (370 size): stall ≈ 0.07 N·m, no-load ≈ 9500 rpm; the same motor as the torque–speed lesson. Not a measured part.";

fn est(value: f64, why: &str) -> ParameterBinding {
    ParameterBinding::Value { value, unit: None, provenance: Some(sim_inspect::Provenance::Estimated { explanation: why.into() }), uncertainty: None }
}

thread_local! {
    static REGISTRY: std::cell::RefCell<Option<sim_core::BehaviorRegistry>> = const { std::cell::RefCell::new(None) };
}

/// One instance being described.
struct P(InstanceSpec);
fn el(t: &str, label: &str) -> P {
    let s = REGISTRY.with(|r| r.borrow().as_ref().map(|r| sim_system::snap::starter(r, &sim_system::InstanceKind::Element { component_type: t.into() }, label))).unwrap_or_else(|| InstanceSpec::element(t).labeled(label));
    P(s)
}
fn sub(definition: &str, label: &str) -> P {
    P(InstanceSpec::subsystem(definition).labeled(label))
}
impl P {
    /// Parameters, each recorded as an estimate with this reason.
    fn set(mut self, why: &str, params: &[(&str, f64)]) -> Self {
        for (k, v) in params {
            self.0.parameters.insert(k.to_string(), est(*v, why));
        }
        self
    }
    fn at(mut self, position: [f32; 3], rotation: [f32; 4]) -> Self {
        self.0.placement.position = position;
        self.0.placement.rotation_xyzw = rotation;
        self
    }
    fn look(mut self, shape: SpatialShape, color: [f32; 3], model: Option<&str>) -> Self {
        self.0.appearance = Some(Appearance { shape, color_srgb: color, model: model.map(String::from) });
        self
    }
    fn model(self, model: &str, shape: SpatialShape, color: [f32; 3]) -> Self {
        self.look(shape, color, Some(model))
    }
}
fn cyl(radius: f32, length: f32) -> SpatialShape {
    SpatialShape::Cylinder { radius, length }
}
fn cube(x: f32, y: f32, z: f32) -> SpatialShape {
    SpatialShape::Box { size: [x, y, z] }
}
const STEEL: [f32; 3] = [0.62, 0.64, 0.67];
const ALU: [f32; 3] = [0.74, 0.76, 0.80];
const DARK: [f32; 3] = [0.30, 0.31, 0.34];
const BLUE: [f32; 3] = [0.16, 0.36, 0.72];
const LOAD: [f32; 3] = [0.35, 0.36, 0.38];
const SIGNAL: [f32; 3] = [0.33, 0.52, 0.86];

/// Commands for one level of a system.
struct Level {
    at: String,
    commands: Vec<Command>,
}
impl Level {
    fn new(at: &str) -> Self {
        Level { at: at.into(), commands: vec![] }
    }
    fn add(&mut self, name: &str, p: P) -> &mut Self {
        self.commands.push(Command::AddInstance { at: self.at.clone(), name: name.into(), instance: p.0 });
        self
    }
    fn join(&mut self, label: &str, terminals: &[(&str, &str)]) -> &mut Self {
        self.commands.push(Command::Connect { at: self.at.clone(), terminals: terminals.iter().map(|(i, p)| Terminal::port(i, p)).collect(), label: label.into() });
        self
    }
}

struct Built {
    slug: &'static str,
    file: &'static str,
    doc: SystemDocument,
}

fn system(registry: &sim_core::BehaviorRegistry, title: &str, description: &str, definitions: Vec<(&str, Definition)>, levels: Vec<Level>, interval: f64, integrator: sim_system::IntegratorChoice, why: &str) -> SystemDocument {
    let mut doc = SystemDocument::new(title);
    if let Some(root) = doc.definitions.get_mut(&doc.root) {
        root.label = title.into();
        root.description = description.into();
    }
    if !definitions.is_empty() {
        let defs = definitions.into_iter().map(|(id, d)| (id.to_string(), d)).collect();
        sim_system::apply(&mut doc, registry, &[Command::AddDefinitions { definitions: defs }]).unwrap_or_else(|e| panic!("{title}: {e}"));
    }
    for level in levels {
        sim_system::apply(&mut doc, registry, &level.commands).unwrap_or_else(|e| panic!("{title}: {e}"));
    }
    doc.run = Some(sim_system::RunSettings { integrator, interval, absolute_tolerance: None, relative_tolerance: None, max_iterations: None, rationale: why.into() });
    doc.revision = 1;
    doc
}
use sim_system::IntegratorChoice::{BackwardEuler as BE, ImplicitMidpoint as IM};

fn metric(label: &str, observable: &str, reduce: sim_system::Reduce, window: [f64; 2]) -> sim_system::Metric {
    sim_system::Metric { label: label.into(), observable: observable.into(), reduce, window: Some(window) }
}
fn sweep(instance: &str, parameter: &str, values: &[f64], duration: f64, observe: &[&str], metrics: Vec<sim_system::Metric>) -> sim_system::Study {
    sim_system::Study { at: String::new(), instance: instance.into(), kind: sim_system::StudyKind::Sweep { parameter: parameter.into(), values: values.to_vec() }, duration, observe: observe.iter().map(|s| s.to_string()).collect(), metrics }
}

/// The 12 V can motor, its rotor and a mount, joined at `motor.shaft`.
fn can_motor(l: &mut Level, x: f32, y: f32) {
    can_motor_on(l, x, y, "rotor", el("rotational.inertia", "Motor rotor").set(CAN, &[("inertia", 3e-6)]).at([x + 0.024, y, 0.], X).look(cyl(0.002, 0.004), STEEL, None));
}
/// The can motor with `name` as the one inertia on its shaft (a shaft node
/// holds one inertia: fold the rotor into the load's value).
fn can_motor_on(l: &mut Level, x: f32, y: f32, name: &str, shaft: P) {
    l.add("frame", el("rotational.ground", "Motor mount").at([x - 0.012, y - 0.018, 0.], Y))
        .add("motor", el("bridge.brushed_motor", "DC motor").set(CAN, &[("resistance", 2.0), ("torque_constant", 0.012), ("back_emf_constant", 0.012), ("inductance", 0.0005)]).at([x, y, 0.], X))
        .add(name, shaft)
        .join("motor case", &[("motor", "case"), ("frame", "flange")])
        .join("motor shaft", &[("motor", "shaft"), (name, "shaft")]);
}
/// A 12 V supply that stays on, and the 0 V reference.
fn supply(l: &mut Level, x: f32, y: f32, volts: f64) {
    // Behind the drive line, so it frames the scene without hiding the mechanism.
    l.add("supply", el("electrical.voltage_source", "Supply").set("A bench supply or a charged pack.", &[("voltage", volts)]).at([x + 0.03, y, -0.07], Y).model("battery_3s", cube(0.066, 0.0185, 0.057), BLUE))
        .add("gnd", el("electrical.ground", "0 V").at([x + 0.03, y - 0.02, -0.07], Y));
}

fn build(registry: &sim_core::BehaviorRegistry) -> Vec<Built> {
    let mut out = vec![];

    // 1. Inertia: a known torque spins up a flywheel.
    {
        let why = "A turned aluminium disc, Ø80 × 12 mm (0.163 kg): J = ½·m·r² = 1.3e-4 kg·m².";
        let mut l = Level::new("");
        l.add("push", el("part.torque_command", "Torque command").set("A steady push for one second: 13 mN·m, about a fifth of a small motor's stall torque.", &[("step", 0.013), ("start", 0.1), ("duration", 1.0), ("edge", 0.01)]).at([-0.1, 0.06, 0.], Y))
            .add("drive", el("rotational.torque_source", "Torque source (an ideal motor)").at([-0.05, 0.05, 0.], X).look(cyl(0.009, 0.03), DARK, None))
            .add("wheel", el("rotational.inertia", "Flywheel").set(why, &[("inertia", 1.3e-4)]).at([0.0, 0.05, 0.], X).model("flywheel", cyl(0.04, 0.012), ALU))
            .join("command", &[("push", "torque"), ("drive", "torque")])
            .join("shaft", &[("drive", "shaft"), ("wheel", "shaft")]);
        let mut doc = system(registry, "Flywheel spin-up", "A known torque turns a flywheel for one second, then stops pushing. No friction: the flywheel keeps whatever speed it has.", vec![], vec![l], 1e-3, IM, "Smooth and conservative: the implicit midpoint rule keeps the coasting speed exactly; 1 ms resolves the push's 10 ms edges.");
        doc.studies.insert("inertia".into(), sweep("wheel", "inertia", &[0.65e-4, 1.3e-4, 2.6e-4, 5.2e-4], 1.2, &["wheel.shaft.speed"], vec![metric("speed after the push (rad/s)", "wheel.shaft.speed", sim_system::Reduce::Final, [0.0, 1.2])]));
        out.push(Built { slug: "inertia-acceleration", file: "flywheel.system.json", doc });
    }

    // 2. Gear ratio: the same motor on a heavy load, through a 5:1 gear.
    {
        let mut l = Level::new("");
        supply(&mut l, -0.11, 0.02, 12.0);
        can_motor(&mut l, -0.06, 0.04);
        l.add("gear", el("rotational.ideal_gear", "Gear pair").set("A spur pair; losses left out so the ratio's effect stands alone.", &[("ratio", 5.0)]).at([-0.02, 0.04, 0.], X))
            .add("load", el("rotational.inertia", "Load (an arm's inertia)").set("A load 25 times the rotor's inertia: an aluminium disc Ø60 mm, 0.167 kg (7.5e-5 kg·m²), standing in for a small arm.", &[("inertia", 7.5e-5)]).at([0.03, 0.04, 0.], X).model("flywheel", cyl(0.03, 0.01), ALU))
            .join("V+", &[("supply", "p"), ("motor", "p")])
            .join("0 V", &[("supply", "n"), ("motor", "n"), ("gnd", "pin")])
            .join("", &[("rotor", "shaft"), ("gear", "input")])
            .join("output", &[("gear", "output"), ("load", "shaft")]);
        let mut doc = system(registry, "Gear ratio and a heavy load", "The 12 V can motor turns a load 25 times its own rotor's inertia through a gear pair. The ratio trades speed for torque, and shrinks the load the motor feels by the ratio squared.", vec![], vec![l], 1e-4, IM, "Smooth motor: implicit midpoint; 0.1 ms resolves the 0.25 ms electrical time constant.");
        use sim_system::Reduce::*;
        doc.studies.insert("ratio".into(), sweep("gear", "ratio", &[1.0, 2.0, 3.0, 5.0, 8.0, 12.0, 20.0], 0.2, &["load.shaft.angle", "load.shaft.speed"], vec![metric("load angle after 30 ms (rad)", "load.shaft.angle", Final, [0.0, 0.03]), metric("top load speed (rad/s)", "load.shaft.speed", Final, [0.0, 0.2])]));
        out.push(Built { slug: "gear-ratio", file: "gearing.system.json", doc });
    }

    // 3. Friction: a slow sine command reveals the dead band.
    {
        let mut l = Level::new("");
        supply(&mut l, -0.13, 0.02, 12.0);
        can_motor_on(&mut l, -0.05, 0.04, "load", el("rotational.inertia", "Rotor and output wheel").set("The rotor (3e-6) and a light wheel (2e-5 kg·m²), with a little oil drag (viscous friction).", &[("inertia", 2.3e-5), ("damping", 1e-5)]).at([0.0, 0.04, 0.], X).model("flywheel", cyl(0.025, 0.008), ALU));
        l.add("wave", el("control.sine", "Slow command sweep").set("One slow sweep up and down: ±0.6 of full drive over 8 s, starting from zero (this sine block is a cosine, so its phase is −π/2).", &[("amplitude", 0.6), ("frequency", 0.125), ("phase", -std::f64::consts::FRAC_PI_2)]).at([-0.13, 0.06, 0.], Y))
            .add("driver", el("robot.h_bridge", "H-bridge driver").set("A MOSFET H-bridge module.", &[("on_resistance", 0.05)]).at([-0.09, 0.03, 0.], Y).model("driver_board", cube(0.04, 0.01, 0.03), [0.2, 0.45, 0.3]))
            .add("friction", el("rotational.coulomb_friction", "Gearbox and seal friction").set("Dry friction in a small gearbox and shaft seal, referred to the motor shaft: 15 mN·m, about a fifth of stall torque.", &[("torque", 0.015), ("smoothing_speed", 0.05)]).at([-0.015, 0.02, 0.], Y))
            .join("command", &[("wave", "value"), ("driver", "command")])
            .join("V+", &[("supply", "p"), ("driver", "supply_p")])
            .join("0 V", &[("supply", "n"), ("driver", "supply_n"), ("gnd", "pin")])
            .join("motor +", &[("driver", "p"), ("motor", "p")])
            .join("motor − (0 V: the averaged bridge output is referenced here)", &[("driver", "n"), ("motor", "n"), ("gnd", "pin")])
            .join("", &[("load", "shaft"), ("friction", "shaft")]);
        let doc = system(registry, "Friction and the dead band", "A slow sine command drives a small motor through an H-bridge against dry (Coulomb) friction. Small commands do nothing at all: the dead band.", vec![], vec![l], 1e-3, BE, "Dry friction is a steep (smoothed) switch at zero speed: backward Euler, 1 ms.");
        out.push(Built { slug: "friction-deadband", file: "joint.system.json", doc });
    }

    // 4. Series-elastic actuator: a spring between servo and arm meets a wall.
    {
        let why = "A hobby servo modelled as a stiff position loop within its speed–torque envelope: 2 N·m stall, 5 rad/s no-load.";
        let mut l = Level::new("");
        l.add("target", el("part.angle_setpoint", "Target angle").set("Swing to 1.2 rad over 0.8 s: past the wall at 0.8 rad.", &[("start", 0.0), ("rate", 1.5), ("ramp_from", 0.2), ("ramp_for", 0.8)]).at([-0.1, 0.06, 0.], Y))
            .add("frame", el("rotational.ground", "Frame").at([-0.06, 0.0, 0.], Y))
            .add("servo", el("robot.effective_servo", "Servo").set(why, &[("stiffness", 30.0), ("damping", 0.6), ("stall_torque", 2.0), ("no_load_speed", 5.0)]).at([-0.06, 0.03, 0.], Y).model("servo", cube(0.02, 0.038, 0.04), DARK))
            .add("rotor", el("rotational.inertia", "Motor rotor, through the gearbox").set("A servo's rotor (5e-6 kg·m²) seen through its 100:1 gearbox: 5e-6 × 100² = 0.05 kg·m², twelve times the arm's own inertia.", &[("inertia", 0.05)]).at([-0.06, 0.045, 0.], Y).look(cyl(0.012, 0.006), [0.72, 0.45, 0.2], None))
            .add("spring", el("rotational.spring", "Series spring").set("A torsion spring between the servo horn and the arm: 3 N·m/rad (soft, about 0.05 N·m per degree).", &[("stiffness", 3.0)]).at([-0.06, 0.06, 0.], Y))
            .add("arm", el("part.pendulum_gravity", "Arm (swings level: gravity does no work)").set("A 0.3 kg arm with its centre of mass 0.12 m out, swinging in a horizontal plane, so gravity is set to zero.", &[("mass", 0.3), ("arm", 0.12), ("g", 0.0)]).at([-0.06, 0.08, 0.], Y))
            .add("arm_inertia", el("rotational.inertia", "Arm inertia").set("m·r² = 0.3 × 0.12² for the arm's mass at its centre.", &[("inertia", 0.00432)]).at([-0.06, 0.08, 0.], Y).look(cyl(0.008, 0.01), STEEL, None))
            .add("wall", el("part.hard_stop_rotary", "Wall at 0.8 rad").set("An obstacle the arm hits at 0.8 rad (46°), faced with a rubber bumper (400 N·m/rad, well damped: 800 N·m·s/rad per radian pressed in).", &[("lower", -3.0), ("upper", 0.8), ("stiffness", 400.0), ("damping", 800.0)]).at([0.03, 0.08, 0.06], Y).look(cube(0.01, 0.03, 0.03), [0.7, 0.3, 0.25], None))
            .join("target", &[("target", "angle"), ("servo", "target")])
            .join("servo case", &[("servo", "housing"), ("frame", "flange")])
            .join("servo horn", &[("servo", "shaft"), ("rotor", "shaft"), ("spring", "a")])
            .join("arm", &[("spring", "b"), ("arm", "shaft"), ("arm_inertia", "shaft"), ("wall", "shaft")]);
        let doc = system(registry, "Series-elastic arm", "A servo swings an arm through a soft torsion spring into a wall. The spring's twist measures the torque, and it cushions the impact.", vec![], vec![l], 2e-4, BE, "Contact at the bumper (400 N·m/rad against 4.3e-3 kg·m²: 300 rad/s) and a 300 N·m/rad coupling in the stiff variant: backward Euler, 0.2 ms.");
        out.push(Built { slug: "series-elastic", file: "arm.system.json", doc });
    }

    // 5. Resonance: a payload on a springy mount, shaken by a sweeping torque.
    {
        let mut l = Level::new("");
        l.add("shaker", el("part.torque_command", "Shaker").set("A sine torque whose frequency climbs from 2 Hz by 4 Hz each second.", &[("amplitude", 0.05), ("frequency", 2.0), ("chirp", 4.0)]).at([-0.1, 0.06, 0.], Y))
            .add("drive", el("rotational.torque_source", "Torque source").at([-0.06, 0.05, 0.], X).look(cyl(0.008, 0.02), DARK, None))
            .add("payload", el("rotational.inertia", "Payload").set("A payload of 0.001 kg·m² (a camera and bracket), turned by the shaker.", &[("inertia", 0.001)]).at([-0.02, 0.05, 0.], X).model("flywheel", cyl(0.035, 0.012), ALU))
            .add("mount", el("rotational.spring", "Flexible mount").set("A springy mount: 4 N·m/rad, which with the payload rings at about 10 Hz.", &[("stiffness", 4.0)]).at([0.015, 0.05, 0.], X))
            .add("damper", el("rotational.damper", "Mount damping").set("The mount's own losses: 6.3 mN·m·s/rad, a damping ratio of 0.05 (lightly damped, like most metal structures).", &[("damping", 0.0063)]).at([0.015, 0.02, 0.], X))
            .add("base", el("rotational.ground", "Base").at([0.045, 0.035, 0.], Y))
            .join("command", &[("shaker", "torque"), ("drive", "torque")])
            .join("payload", &[("drive", "shaft"), ("payload", "shaft"), ("mount", "a"), ("damper", "a")])
            .join("base", &[("mount", "b"), ("damper", "b"), ("base", "flange")]);
        let mut doc = system(registry, "Payload on a springy mount", "A payload on a torsion spring, shaken by a torque whose frequency sweeps upward. Near the natural frequency √(k/J) the motion grows many times larger.", vec![], vec![l], 5e-4, IM, "Lightly damped oscillator: implicit midpoint keeps its energy; 0.5 ms is 200 steps per 10 Hz cycle.");
        use sim_system::Reduce::*;
        doc.studies.insert("stiffness".into(), sweep("mount", "stiffness", &[1.0, 4.0, 16.0], 4.0, &["payload.shaft.angle"], vec![metric("largest swing (rad)", "payload.shaft.angle", Peak, [0.0, 4.0])]));
        out.push(Built { slug: "resonance", file: "mount.system.json", doc });
    }

    // 6. Gravity torque: a servo lifts a leg from hanging to level.
    {
        let why = "A standard hobby servo as a position loop within its envelope: 1.2 N·m stall (≈ 12 kg·cm), 6 rad/s no-load, 8 N·m/rad stiffness.";
        let mut l = Level::new("");
        l.add("target", el("part.angle_setpoint", "Target angle").set("Lift at 1 rad/s from hanging (0) to level (π/2), then hold.", &[("start", 0.0), ("rate", 1.0), ("ramp_from", 0.3), ("ramp_for", std::f64::consts::FRAC_PI_2)]).at([-0.09, 0.08, 0.], Y))
            .add("frame", el("rotational.ground", "Hip frame").at([-0.03, 0.07, -0.03], Y))
            .add("servo", el("robot.effective_servo", "Hip servo").set(why, &[("stiffness", 8.0), ("damping", 0.25), ("stall_torque", 1.2), ("no_load_speed", 6.0)]).at([0., 0.1, -0.03], Z).model("servo", cube(0.02, 0.038, 0.04), DARK))
            .add("leg", el("part.pendulum_gravity", "Leg").set("A 0.4 kg leg with its centre of mass 0.15 m below the hip.", &[("mass", 0.4), ("arm", 0.15)]).at([0., 0.1, 0.], Z))
            .add("leg_inertia", el("rotational.inertia", "Leg inertia").set("m·r² = 0.4 × 0.15² with the leg's mass at its centre.", &[("inertia", 0.009)]).at([0., 0.1, 0.005], Z).look(cyl(0.008, 0.01), STEEL, None))
            .join("target", &[("target", "angle"), ("servo", "target")])
            .join("servo case", &[("servo", "housing"), ("frame", "flange")])
            .join("hip", &[("servo", "shaft"), ("leg", "shaft"), ("leg_inertia", "shaft")]);
        let mut doc = system(registry, "Lifting a leg against gravity", "A hip servo lifts a 0.4 kg leg from hanging straight down to level. Gravity's torque grows with the angle, and the servo sags where it is largest.", vec![], vec![l], 1e-3, BE, "Servo envelope switches at stall: backward Euler, 1 ms.");
        use sim_system::Reduce::*;
        doc.studies.insert("leg_mass".into(), sweep("leg", "mass", &[0.2, 0.4, 0.6, 0.8, 1.0], 2.5, &["leg.shaft.angle"], vec![metric("angle held at level (rad, target 1.571)", "leg.shaft.angle", Mean, [2.2, 2.5])]));
        out.push(Built { slug: "gravity-torque", file: "leg.system.json", doc });
    }

    // 7. Backlash: a servo rocks a pointer through a gear mesh with play.
    {
        let mut l = Level::new("");
        l.add("target", el("part.angle_setpoint", "Rocking target").set("Rock ±0.3 rad once a second.", &[("amplitude", 0.3), ("frequency", 1.0)]).at([-0.1, 0.07, 0.], Y))
            .add("frame", el("rotational.ground", "Frame").at([-0.07, 0.0, 0.], Y))
            .add("servo", el("robot.effective_servo", "Motor side (servo)").set("A stiff position loop on the motor side of the gearbox.", &[("stiffness", 40.0), ("damping", 0.5), ("stall_torque", 2.0), ("no_load_speed", 8.0)]).at([-0.07, 0.035, 0.], Z).model("servo", cube(0.02, 0.038, 0.04), DARK))
            .add("mesh", el("rotational.backlash_mesh", "Gear mesh with play").set("Worn hobby spur gears: 0.08 rad (4.6°) of free play in all, 0.04 rad each side of centre (`gap` is half the play), and 200 N·m/rad once the teeth touch.", &[("stiffness", 200.0), ("gap", 0.04)]).at([-0.03, 0.035, 0.], X))
            .add("pointer", el("part.pendulum_gravity", "Output pointer").set("A 0.1 kg pointer arm 0.08 m long on the output shaft, turning in a vertical plane.", &[("mass", 0.1), ("arm", 0.08), ("g", 0.0)]).at([0.01, 0.035, 0.], Z))
            .add("output", el("rotational.inertia", "Output shaft").set("The output shaft and pointer, m·r² = 0.1 × 0.08².", &[("inertia", 0.00064), ("damping", 0.002)]).at([0.01, 0.035, 0.005], Z).look(cyl(0.006, 0.01), STEEL, None))
            .add("drag", el("rotational.coulomb_friction", "Output bearing friction").set("Bearing and seal friction on the output: enough to stop it where the teeth leave it.", &[("torque", 0.03), ("smoothing_speed", 0.05)]).at([0.03, 0.01, 0.], Y))
            .join("target", &[("target", "angle"), ("servo", "target")])
            .join("servo case", &[("servo", "housing"), ("frame", "flange")])
            .join("motor side", &[("servo", "shaft"), ("mesh", "a")])
            .join("output side", &[("mesh", "b"), ("pointer", "shaft"), ("output", "shaft"), ("drag", "shaft")]);
        let doc = system(registry, "Backlash in a gear mesh", "A servo rocks back and forth; the output only follows once the gear teeth have crossed the free play between them.", vec![], vec![l], 2e-4, BE, "Contact across the gap is a stiff switch: backward Euler, 0.2 ms.");
        out.push(Built { slug: "backlash", file: "gearhead.system.json", doc });
    }

    // 8. Linear drives: the same motor on a belt and on a lead screw, lifting 2 kg.
    // Both are drawn as vertical towers: the belt hangs from a pulley at the
    // top; the screw stands on its motor, and the carriage rides it.
    for (file, belt) in [("belt.system.json", true), ("screw.system.json", false)] {
        let mut l = Level::new("");
        let motor_at: [f32; 3] = if belt { [0., 0.22, -0.035] } else { [0., 0.03, 0.] };
        let (axis, rotor_at) = if belt { (Z, [0., 0.22, -0.012]) } else { (Y, [0., 0.058, 0.]) };
        l.add("supply", el("electrical.switched_voltage_source", "Supply (off at 0.8 s)").set("12 V, switched off at 0.8 s.", &[("voltage", 12.0), ("on_at", 0.0), ("off_at", 0.8)]).at([-0.07, 0.12, -0.07], Y).model("battery_3s", cube(0.066, 0.0185, 0.057), BLUE))
            .add("gnd", el("electrical.ground", "0 V").at([-0.07, 0.1, -0.07], Y))
            .add("frame", el("rotational.ground", "Motor mount").at([motor_at[0] - 0.02, motor_at[1] - 0.02, motor_at[2]], Y))
            .add("motor", el("bridge.brushed_motor", "DC motor").set(CAN, &[("resistance", 2.0), ("torque_constant", 0.012), ("back_emf_constant", 0.012), ("inductance", 0.0005)]).at(motor_at, axis))
            .add("rotor", el("rotational.inertia", "Motor rotor").set(CAN, &[("inertia", 3e-6)]).at(rotor_at, axis).look(cyl(0.002, 0.004), STEEL, None))
            .join("motor case", &[("motor", "case"), ("frame", "flange")])
            .join("motor shaft", &[("motor", "shaft"), ("rotor", "shaft")])
            .join("V+", &[("supply", "p"), ("motor", "p")])
            .join("0 V", &[("supply", "n"), ("motor", "n"), ("gnd", "pin")]);
        let carriage_at: [f32; 3] = if belt { [0.0064, 0.1, 0.] } else { [0., 0.1, 0.] };
        l.add("carriage", el("translational.mass", "Carriage and 2 kg load").set("A 2 kg load on a vertical carriage.", &[("mass", 2.0)]).at(carriage_at, Y).look(cube(0.034, 0.022, 0.034), LOAD, None))
            .add("weight", el("translational.load_force", "Weight").set("m·g = 2 × 9.81 N, pulling down.", &[("force", -19.62)]).at([carriage_at[0], carriage_at[1] - 0.02, 0.], Y))
            .add("stops", el("part.hard_stop_linear", "End stops").set("Travel from −5 cm to +12 cm.", &[("lower", -0.05), ("upper", 0.12)]).at([0.05, 0.1, 0.], Y));
        if belt {
            l.add("belt", el("part.timing_belt", "GT2 belt, 20-tooth pulley").set("GT2 20-tooth pulley: pitch radius 6.37 mm; a short 6 mm belt, 30 kN/m.", &[("radius", 0.00637), ("stiffness", 30000.0), ("damping", 50.0)]).at([0., 0.22, 0.], Z).model("gt2_pulley", cyl(0.008, 0.016), ALU))
                .join("", &[("rotor", "shaft"), ("belt", "pulley")])
                .join("carriage", &[("belt", "carriage"), ("carriage", "axis"), ("weight", "axis"), ("stops", "axis")]);
        } else {
            l.add("screw", el("bridge.lead_screw", "Lead screw T8, 8 mm lead").set("A T8 lead screw (8 mm diameter) with a 4-start, 8 mm lead thread and a bronze nut in the carriage, μ ≈ 0.15.", &[("lead", 0.008), ("pitch_diameter", 0.008), ("friction", 0.15)]).at([0., 0.165, 0.], Y).look(cyl(0.004, 0.2), STEEL, None))
                .join("", &[("rotor", "shaft"), ("screw", "screw")])
                .join("carriage", &[("screw", "nut"), ("carriage", "axis"), ("weight", "axis"), ("stops", "axis")]);
        }
        let title = if belt { "Belt lift" } else { "Lead-screw lift" };
        let description = if belt { "The 12 V can motor tries to lift 2 kg straight up with a GT2 belt hanging from a 20-tooth pulley." } else { "The 12 V can motor lifts 2 kg straight up with a T8 lead screw; at 0.8 s the power goes off." };
        let doc = system(registry, title, description, vec![], vec![l], 2e-4, BE, "Stiff belt and end stops, and the screw's friction switches with direction: backward Euler, 0.2 ms.");
        out.push(Built { slug: "linear-drives", file, doc });
    }

    // 9. Traction: a one-wheel rover module on a 1 kg chassis.
    {
        let why = "A 37 mm 12 V gearmotor, 30:1 (333 rpm no-load, ≈ 1.5 N·m stall); catalogue-style values.";
        let mut rover = Definition::new("Rover drive");
        rover.description = "One driven wheel with its motor, gearbox and driver, carrying a quarter of a 4 kg rover.".into();
        let mut d = Level::new("rover");
        d.add("frame", el("rotational.ground", "Motor mount").at([-0.05, 0.03, 0.], Y))
            .add("pack", el("electrical.voltage_source", "Battery").set("A 3S pack held at 12 V.", &[("voltage", 12.0)]).at([-0.09, 0.05, 0.], Y).model("battery_3s", cube(0.066, 0.0185, 0.057), BLUE))
            .add("gnd", el("electrical.ground", "0 V").at([-0.09, 0.03, 0.], Y))
            .add("driver", el("robot.h_bridge", "Driver").set("A MOSFET H-bridge.", &[("on_resistance", 0.05)]).at([-0.05, 0.065, 0.], Y).model("driver_board", cube(0.04, 0.01, 0.03), [0.2, 0.45, 0.3]))
            .add("motor", el("bridge.brushed_motor", "Motor").set(why, &[("resistance", 2.2), ("torque_constant", 0.0115), ("back_emf_constant", 0.0115), ("inductance", 0.0)]).at([-0.04, 0.04, 0.], X))
            .add("rotor", el("rotational.inertia", "Rotor").set(why, &[("inertia", 2e-6)]).at([-0.02, 0.04, 0.], X).look(cyl(0.002, 0.004), STEEL, None))
            .add("gearbox", el("rotational.lossy_gear", "Gearbox 30:1").set(why, &[("ratio", 30.0), ("efficiency", 0.8)]).at([-0.005, 0.04, 0.], X))
            .add("hub", el("rotational.inertia", "Wheel").set("An 80 mm wheel with a rubber tyre, 2e-4 kg·m².", &[("inertia", 2e-4)]).at([0.02, 0.04, 0.035], Z).model("wheel", cyl(0.04, 0.024), DARK))
            .add("tyre", el("part.drive_wheel", "Tyre on the floor").set("An 80 mm wheel carrying 9.8 N (a quarter of a 4 kg rover); rubber on dry floor μ ≈ 0.8.", &[("radius", 0.04), ("normal_load", 9.81), ("grip", 0.8), ("slip_speed", 0.02), ("rolling", 0.01)]).at([0.02, 0.04, 0.035], Z).look(cyl(0.001, 0.001), DARK, None))
            .add("body", el("translational.mass", "Chassis share (1 kg)").set("A quarter of a 4 kg rover.", &[("mass", 1.0)]).at([-0.03, 0.02, 0.], X).look(cube(0.05, 0.14, 0.08), [0.82, 0.62, 0.22], None))
            .join("V+", &[("pack", "p"), ("driver", "supply_p")])
            .join("0 V", &[("pack", "n"), ("driver", "supply_n"), ("gnd", "pin")])
            .join("motor +", &[("driver", "p"), ("motor", "p")])
            .join("motor − (0 V: the averaged bridge output is referenced here)", &[("driver", "n"), ("motor", "n"), ("gnd", "pin")])
            .join("motor case", &[("motor", "case"), ("frame", "flange")])
            .join("", &[("motor", "shaft"), ("rotor", "shaft"), ("gearbox", "input")])
            .join("axle", &[("gearbox", "output"), ("hub", "shaft"), ("tyre", "axle")])
            .join("chassis", &[("tyre", "chassis"), ("body", "axis")]);
        rover.interface = Some("drive".into());
        let mut r = Level::new("");
        r.add("wall", el("part.hard_stop_linear", "Wall 0.6 m ahead (never reached)").set("A wall in front of the rover, farther than it travels here: it frames the run.", &[("lower", -1.0), ("upper", 0.6)]).at([0.62, 0.04, 0.035], X).look(cube(0.12, 0.02, 0.12), [0.45, 0.47, 0.52], None))
            .add("throttle", el("control.pulse", "Throttle").set("Full throttle for 0.35 s from 0.05 s, then 0 (the driver shorts the motor: braking).", &[("amplitude", 1.0), ("start", 0.05), ("duration", 0.35)]).at([-0.12, 0.08, 0.], Y))
            .add("rover", sub("rover_drive", "Rover drive").at([0., 0., 0.], Y));
        let mut doc = system(registry, "Rover wheel traction", "One wheel of a 4 kg rover, full throttle then brake. The tyre can push only as hard as grip × the weight on it; beyond that the wheel spins.", vec![("rover_drive", rover)], vec![r, d], 5e-4, BE, "Tyre slip is a steep smoothed switch (2 cm/s): backward Euler, 0.5 ms.");
        // The rover's throttle crosses the subsystem boundary through a port.
        sim_system::apply(&mut doc, registry, &[Command::AddBoundaryPort { at: "rover".into(), name: "throttle".into(), port: sim_system::BoundaryPort { label: "Throttle".into(), schema: None }, connect: Some(Terminal::port("driver", "command")) }, Command::Connect { at: "".into(), terminals: vec![Terminal::port("throttle", "value"), Terminal::port("rover", "throttle")], label: "throttle".into() }, Command::AddBoundaryPort { at: "rover".into(), name: "chassis".into(), port: sim_system::BoundaryPort { label: "Chassis".into(), schema: None }, connect: Some(Terminal::port("body", "axis")) }, Command::Connect { at: "".into(), terminals: vec![Terminal::port("rover", "chassis"), Terminal::port("wall", "axis")], label: "chassis".into() }]).unwrap();
        use sim_system::Reduce::*;
        doc.studies.insert("grip".into(), sweep("rover", "tyre/grip", &[0.2, 0.4, 0.8, 1.0], 0.4, &["rover/body.axis.velocity"], vec![metric("speed at 0.25 s (m/s)", "rover/body.axis.velocity", Final, [0.0, 0.25])]));
        out.push(Built { slug: "wheel-traction", file: "rover.system.json", doc });
    }

    // 10. Battery sag: a motor starts on a small, tired pack.
    {
        let mut l = Level::new("");
        l.add("pack", el("robot.battery", "2S LiPo, small and worn").set("A small 2S LiPo (7.4 V nominal) whose internal resistance has grown to 0.12 Ω with age.", &[("cells", 2.0), ("nominal_voltage", 7.4), ("internal_resistance", 0.12), ("capacity_ah", 0.8), ("initial_soc", 0.9)]).at([-0.1, 0.03, 0.], Y).model("battery_3s", cube(0.066, 0.0185, 0.057), BLUE))
            .add("gnd", el("electrical.ground", "0 V").at([-0.1, 0.0, 0.], Y))
            .add("sense", el("electrical.voltage_sense", "Pack voltage").set("Reads the pack's terminal voltage directly (scale 1 V).", &[("scale", 1.0)]).at([-0.07, 0.06, 0.], Y))
            .add("logic", el("electrical.resistor", "Controller board").set("The controller and its 5 V regulator: about 0.2 A from the pack.", &[("resistance", 37.0)]).at([-0.07, 0.0, 0.03], Y).model("soic8", cube(0.005, 0.0015, 0.004), DARK))
            .add("go", el("control.pulse", "Start command").set("Full drive from 0.1 s.", &[("amplitude", 1.0), ("start", 0.1), ("duration", 10.0)]).at([-0.07, 0.08, 0.], Y))
            .add("driver", el("robot.h_bridge", "Driver").set("A MOSFET H-bridge.", &[("on_resistance", 0.05)]).at([-0.04, 0.03, 0.], Y).model("driver_board", cube(0.04, 0.01, 0.03), [0.2, 0.45, 0.3]))
            .add("frame", el("rotational.ground", "Mount").at([-0.01, 0.01, 0.], Y))
            .add("motor", el("bridge.brushed_motor", "540 motor").set("A 540-size brushed motor: 0.3 Ω, 10 mN·m/A (≈ 18 A at stall on a fresh 2S pack). Catalogue-style values.", &[("resistance", 0.3), ("torque_constant", 0.01), ("back_emf_constant", 0.01), ("inductance", 0.00005)]).at([0.0, 0.03, 0.], X).model("dc_motor", cyl(0.018, 0.05), STEEL))
            .add("load", el("rotational.inertia", "Rotor and wheel").set("Rotor plus a wheel and gearing referred to the motor: 5e-5 kg·m², so spin-up takes about 0.2 s.", &[("inertia", 5e-5), ("damping", 2e-5)]).at([0.035, 0.03, 0.], X).model("flywheel", cyl(0.025, 0.01), ALU))
            .join("pack +", &[("pack", "p"), ("driver", "supply_p"), ("sense", "p"), ("logic", "p")])
            .join("0 V", &[("pack", "n"), ("driver", "supply_n"), ("sense", "n"), ("logic", "n"), ("gnd", "pin")])
            .join("command", &[("go", "value"), ("driver", "command")])
            .join("motor +", &[("driver", "p"), ("motor", "p")])
            .join("motor − (0 V: the averaged bridge output is referenced here)", &[("driver", "n"), ("motor", "n"), ("gnd", "pin")])
            .join("motor case", &[("motor", "case"), ("frame", "flange")])
            .join("", &[("motor", "shaft"), ("load", "shaft")]);
        let mut doc = system(registry, "Battery sag at motor start", "A 540 motor starts at full drive from a small, worn 2S pack that also powers the controller. The start current pulls the pack's voltage down.", vec![], vec![l], 2e-4, BE, "Driver and battery are algebraic; motor L/R = 0.17 ms: backward Euler, 0.2 ms.");
        use sim_system::Reduce::*;
        doc.studies.insert("pack".into(), sweep("pack", "internal_resistance", &[0.02, 0.06, 0.12, 0.2], 0.5, &["sense.reading", "motor.p.current"], vec![metric("lowest pack voltage (V)", "sense.reading", Min, [0.1, 0.5]), metric("peak current (A)", "motor.p.current", Peak, [0.1, 0.5])]));
        out.push(Built { slug: "battery-sag", file: "pack.system.json", doc });
    }

    // 11. Motor heating: a winding with heat capacity and a path to the air.
    {
        let why = "A coreless 12 V motor with the torque–speed lesson's constants; its resistance rises 0.39 %/K as the copper warms.";
        let mut l = Level::new("");
        supply(&mut l, -0.11, 0.02, 12.0);
        l.add("frame", el("rotational.ground", "Mount").at([-0.07, 0.01, 0.], Y))
            .add("motor", el("part.coreless_motor", "Motor").set(why, &[("resistance", 2.0), ("torque_constant", 0.012), ("inductance", 0.0005)]).with_initial("initial.heat.temperature", 293.15).at([-0.06, 0.04, 0.], X).model("dc_motor", cyl(0.012, 0.041), STEEL))
            .add("rotor", el("rotational.inertia", "Rotor").set(why, &[("inertia", 3e-6)]).at([-0.035, 0.04, 0.], X).look(cyl(0.002, 0.004), STEEL, None))
            .add("load", el("rotational.load_torque", "Load").set("A steady 25 mN·m load: about 2.1 A, a third of stall, but more than this motor can shed continuously.", &[("torque", -0.025)]).at([-0.02, 0.04, 0.], Y).look(cube(0.016, 0.016, 0.016), LOAD, None))
            .add("winding", el("thermal.capacitance", "Winding").set("About 24 g of copper and its insulation: 9 J/K.", &[("heat_capacity", 9.0)]).with_initial("initial.temperature", 293.15).at([-0.06, 0.07, 0.], Y))
            .add("cooling", el("thermal.conductance", "Case to air").set("Winding to case to still air: 0.15 W/K (6.7 K/W).", &[("conductance", 0.15)]).at([-0.03, 0.07, 0.], Y).model("heatsink", cube(0.03, 0.015, 0.02), [0.55, 0.57, 0.6]))
            .add("air", el("thermal.ambient", "Room air (20 °C)").set("Still room air.", &[("temperature", 293.15)]).at([0.0, 0.07, 0.], Y))
            .join("V+", &[("supply", "p"), ("motor", "p")])
            .join("0 V", &[("supply", "n"), ("motor", "n"), ("gnd", "pin")])
            .join("motor case", &[("motor", "case"), ("frame", "flange")])
            .join("", &[("motor", "shaft"), ("rotor", "shaft"), ("load", "shaft")])
            .join("winding heat", &[("motor", "heat"), ("winding", "node"), ("cooling", "a")])
            .join("air", &[("cooling", "b"), ("air", "node")]);
        let mut doc = system(registry, "Motor winding heating", "A motor runs a steady load for five minutes. Its winding heats by I²·R and loses heat to the air; the temperature climbs toward the level where the two balance.", vec![], vec![l], 5e-3, BE, "Minutes of heating with 0.25 ms electrical dynamics: backward Euler (stable on stiff parts) at 5 ms; the electrical and mechanical states settle within a step.");
        use sim_system::Reduce::*;
        doc.studies.insert("load".into(), sweep("load", "torque", &[-0.01, -0.015, -0.02, -0.025], 300.0, &["winding.node.temperature", "motor.p.current"], vec![metric("winding after 5 min (K)", "winding.node.temperature", Final, [0.0, 300.0]), metric("current (A)", "motor.p.current", Mean, [250.0, 300.0])]));
        out.push(Built { slug: "motor-heating", file: "heating.system.json", doc });
    }

    // 12. Back-driving: a spinning load drives the motor as a generator into a resistor.
    {
        let mut l = Level::new("");
        l.add("gnd", el("electrical.ground", "0 V").at([-0.1, 0.0, 0.], Y));
        can_motor_on(&mut l, -0.05, 0.04, "load", el("rotational.inertia", "Spinning load").set("A wheel and the motor's rotor, together 3e-5 kg·m², already spinning at 600 rad/s (5700 rpm) when the lesson starts.", &[("inertia", 3e-5), ("initial.speed", 600.0)]).at([-0.01, 0.04, 0.], X).model("flywheel", cyl(0.03, 0.01), ALU));
        l.add("brake", el("electrical.resistor", "Brake resistor").set("Across the motor's terminals: 1 Ω.", &[("resistance", 1.0)]).at([-0.09, 0.04, 0.], Y).model("axial_resistor", cube(0.013, 0.004, 0.0027), [0.8, 0.52, 0.3]))
            .add("bearings", el("rotational.coulomb_friction", "Bearing friction").set("Ball bearings and brushes: 2 mN·m.", &[("torque", 0.002), ("smoothing_speed", 0.5)]).at([0.02, 0.02, 0.], Y))
            .join("terminal +", &[("motor", "p"), ("brake", "p")])
            .join("terminal −", &[("motor", "n"), ("brake", "n"), ("gnd", "pin")])
            .join("", &[("load", "shaft"), ("bearings", "shaft")]);
        let mut doc = system(registry, "A motor as a generator", "A wheel spinning at 600 rad/s turns a motor whose terminals are joined through a resistor. The motor becomes a generator, and its current brakes the wheel.", vec![], vec![l], 2e-4, BE, "The winding's 0.17 ms electrical time constant is stiff for the implicit midpoint rule at useful steps (it rings); backward Euler at 0.2 ms damps it and resolves the 0.4–0.6 s braking to well under 1 %.");
        use sim_system::Reduce::*;
        doc.studies.insert("resistor".into(), sweep("brake", "resistance", &[0.01, 1.0, 5.0, 20.0, 1e6], 2.0, &["load.shaft.speed"], vec![metric("speed after 0.5 s (rad/s)", "load.shaft.speed", Final, [0.0, 0.5])]));
        out.push(Built { slug: "back-driving", file: "generator.system.json", doc });
    }

    // 13. Wiring and decoupling: a PWM switch pulls pulsed current through a long lead.
    {
        let mut l = Level::new("");
        l.add("bench", el("electrical.voltage_source", "Bench supply").set("A 12 V bench supply.", &[("voltage", 12.0)]).at([-0.13, 0.03, 0.], Y).look(cube(0.03, 0.02, 0.025), BLUE, None))
            .add("gnd", el("electrical.ground", "0 V").at([-0.13, 0.0, 0.], Y))
            .add("wire_r", el("electrical.resistor", "Lead resistance").set("1 m of 22 AWG, there and back: 0.1 Ω.", &[("resistance", 0.1)]).at([-0.09, 0.05, 0.], X).look(cyl(0.001, 0.03), [0.85, 0.3, 0.2], None))
            .add("wire_l", el("electrical.inductor", "Lead inductance").set("A 1 m loop of wire: about 1 µH per metre.", &[("inductance", 1e-6)]).at([-0.06, 0.05, 0.], X).look(cyl(0.001, 0.03), [0.85, 0.3, 0.2], None))
            .add("cap", el("electrical.capacitor", "Bus capacitor").set("An electrolytic at the board's power input: 100 µF.", &[("capacitance", 1e-4), ("initial.p.voltage", 12.0)]).at([-0.03, 0.03, 0.], Y).model("radial_capacitor", cyl(0.004, 0.011), [0.2, 0.3, 0.6]))
            .add("sense", el("electrical.voltage_sense", "Board voltage").set("The voltage the board sees.", &[("scale", 1.0)]).at([-0.03, 0.06, 0.], Y))
            .add("motor", el("bridge.brushed_motor", "Motor (held still)").set("A stalled motor at 50 % PWM: 1 Ω and 0.2 mH, so its current is nearly steady while the supply current is chopped.", &[("resistance", 1.0), ("torque_constant", 0.01), ("back_emf_constant", 0.01), ("inductance", 0.0002)]).at([0.01, 0.05, 0.], X))
            .add("frame", el("rotational.ground", "Clamp").at([0.03, 0.03, 0.], Y))
            .add("diode", el("electrical.diode", "Freewheel diode").set("A Schottky freewheel diode across the motor.", &[("forward_voltage", 0.4)]).at([0.01, 0.075, 0.], X).model("do41_diode", cube(0.013, 0.004, 0.0027), DARK))
            .add("switch", el("electrical.mosfet", "Low-side MOSFET").set("A logic-level MOSFET, 20 mΩ.", &[("on_resistance", 0.02)]).at([0.01, 0.02, 0.], Y).model("to220", cube(0.01, 0.0185, 0.0046), DARK))
            .add("pwm", el("control.pwm", "PWM 20 kHz").set("20 kHz PWM at 50 % duty.", &[("frequency", 20000.0), ("initial_duty", 0.5)]).at([0.04, 0.0, 0.], Y).model("soic8", cube(0.005, 0.0015, 0.004), DARK))
            .add("duty", el("control.constant", "Duty 50 %").set("Half duty.", &[("value", 0.5)]).at([0.06, 0.0, 0.], Y))
            .add("air", el("thermal.ambient", "Air").set("Room air.", &[("temperature", 298.15)]).at([0.04, 0.02, 0.02], Y))
            .join("supply +", &[("bench", "p"), ("wire_r", "p")])
            .join("", &[("wire_r", "n"), ("wire_l", "p")])
            .join("board +", &[("wire_l", "n"), ("cap", "p"), ("sense", "p"), ("motor", "p"), ("diode", "n")])
            .join("0 V", &[("bench", "n"), ("cap", "n"), ("sense", "n"), ("switch", "source"), ("gnd", "pin")])
            .join("switch node", &[("motor", "n"), ("diode", "p"), ("switch", "drain")])
            .join("", &[("motor", "case"), ("motor", "shaft"), ("frame", "flange")])
            .join("", &[("duty", "value"), ("pwm", "duty")])
            .join("gate", &[("pwm", "gate"), ("switch", "gate")])
            .join("", &[("switch", "heat"), ("air", "node")]);
        let mut doc = system(registry, "Long leads and a bus capacitor", "A motor switched at 20 kHz by a low-side MOSFET, fed from a bench supply through a metre of thin wire. Each switching edge asks the wire for a step in current; the bus capacitor at the board supplies it instead.", vec![], vec![l], 2e-7, BE, "Switching edges and a 1 µH lead: backward Euler at 0.2 µs (250 steps per PWM period).");
        use sim_system::Reduce::*;
        doc.studies.insert("capacitor".into(), sweep("cap", "capacitance", &[1e-6, 1e-5, 1e-4, 1e-3], 0.004, &["sense.reading"], vec![metric("lowest board voltage (V)", "sense.reading", Min, [0.003, 0.004]), metric("highest board voltage (V)", "sense.reading", Max, [0.003, 0.004])]));
        out.push(Built { slug: "wiring-decoupling", file: "bus.system.json", doc });
    }

    // 14. H-bridge modes: drive, then brake (or coast, or reverse).
    {
        let mut l = Level::new("");
        supply(&mut l, -0.13, 0.02, 12.0);
        can_motor_on(&mut l, -0.05, 0.04, "load", el("rotational.inertia", "Wheel").set("A wheel on the shaft with the rotor: 5e-5 kg·m², so braking and coasting are slow enough to watch.", &[("inertia", 5e-5)]).at([-0.005, 0.04, 0.], X).model("flywheel", cyl(0.03, 0.01), ALU));
        l.add("drive", el("control.pulse", "Drive command").set("Full forward for 0.4 s, then 0: with the bridge on, 0 shorts the motor (brake).", &[("amplitude", 1.0), ("start", 0.0), ("duration", 0.4)]).at([-0.12, 0.07, 0.], Y))
            .add("enable", el("control.pulse", "Bridge enable").set("On for the whole run: the bridge stays in control (drive, then brake).", &[("amplitude", 1.0), ("start", 0.0), ("duration", 100.0)]).at([-0.12, 0.09, 0.], Y))
            .add("bridge", el("robot.switchable_h_bridge", "H-bridge").set("A MOSFET H-bridge; disabled, the motor's current can only flow through the switches' body diodes.", &[("on_resistance", 0.05)]).at([-0.09, 0.04, 0.], Y).model("driver_board", cube(0.04, 0.01, 0.03), [0.2, 0.45, 0.3]))
            .add("drag", el("rotational.coulomb_friction", "Bearing friction").set("Bearings and brushes: 2 mN·m.", &[("torque", 0.002), ("smoothing_speed", 0.5)]).at([0.02, 0.02, 0.], Y))
            .join("V+", &[("supply", "p"), ("bridge", "supply_p")])
            .join("0 V", &[("supply", "n"), ("bridge", "supply_n"), ("gnd", "pin")])
            .join("command", &[("drive", "value"), ("bridge", "command")])
            .join("enable", &[("enable", "value"), ("bridge", "enabled")])
            .join("motor +", &[("bridge", "p"), ("motor", "p")])
            .join("motor − (0 V: the averaged bridge output is referenced here)", &[("bridge", "n"), ("motor", "n"), ("gnd", "pin")])
            .join("", &[("load", "shaft"), ("drag", "shaft")]);
        let doc = system(registry, "H-bridge: drive, brake and coast", "The motor spins a wheel up for 0.4 s. Then the bridge either shorts the motor (brake), lets go (coast), or reverses it: three ways to stop, from the same four switches.", vec![], vec![l], 2e-4, BE, "Bridge switching and diode paths: backward Euler, 0.2 ms.");
        out.push(Built { slug: "h-bridge-modes", file: "bridge.system.json", doc });
    }

    // 15. Steppers: a move too fast to follow loses steps.
    {
        let why = "Representative NEMA 17 (42 × 42 × 40 mm): 0.45 N·m holding, 54 g·cm² rotor, microstepping current driver.";
        let mut l = Level::new("");
        l.add("move", el("part.trapezoid_move", "Move profile").set("One full turn at up to 60 rad/s; acceleration set per scene.", &[("travel", std::f64::consts::TAU), ("rate", 60.0), ("accel", 20000.0), ("start", 0.05)]).at([-0.1, 0.06, 0.], Y))
            .add("frame", el("rotational.ground", "Frame").at([-0.05, 0.0, 0.], Y))
            .add("stepper", el("part.stepper_motor", "NEMA 17 stepper").set(why, &[("holding_torque", 0.45), ("steps_per_rev", 200.0), ("detent_torque", 0.02), ("damping", 0.002)]).at([-0.05, 0.035, 0.], X).model("nema17", cube(0.0423, 0.04, 0.0423), DARK))
            .add("load", el("rotational.inertia", "Rotor, pulley and carriage").set("The rotor (5.4e-6) plus a pulley, belt and carriage referred to the shaft (1e-4): about 20 times the rotor alone.", &[("inertia", 1.054e-4)]).at([-0.015, 0.035, 0.], X).model("flywheel", cyl(0.02, 0.01), ALU))
            .add("drag", el("rotational.coulomb_friction", "Axis friction").set("Linear-bearing and belt friction referred to the shaft: 0.05 N·m.", &[("torque", 0.05), ("smoothing_speed", 0.2)]).at([0.015, 0.015, 0.], Y))
            .join("target", &[("move", "angle"), ("stepper", "target")])
            .join("case", &[("stepper", "case"), ("frame", "flange")])
            .join("", &[("stepper", "shaft"), ("load", "shaft"), ("drag", "shaft")]);
        let mut doc = system(registry, "Stepper move", "A NEMA 17 stepper is told to turn one revolution. The driver moves the magnetic target; the rotor follows like a stiff spring, unless the target runs away from it.", vec![], vec![l], 1e-4, BE, "Stepper torque ripple (50 electrical cycles per turn at 60 rad/s ≈ 480 Hz): backward Euler, 0.1 ms.");
        use sim_system::Reduce::*;
        doc.studies.insert("acceleration".into(), sweep("move", "accel", &[500.0, 2000.0, 5000.0, 20000.0], 0.5, &["load.shaft.angle"], vec![metric("final angle (6.283 if no steps lost)", "load.shaft.angle", Final, [0.0, 0.5])]));
        out.push(Built { slug: "stepper-steps", file: "stepper.system.json", doc });
    }

    // 16. Encoders: counts, and the speed you can get from them.
    {
        let mut l = Level::new("");
        l.add("push", el("part.torque_command", "Gentle push").set("A small steady torque, so the wheel's speed ramps up slowly: 2 mN·m.", &[("step", 0.002), ("start", 0.1), ("duration", 10.0), ("edge", 0.002)]).at([-0.1, 0.06, 0.], Y))
            .add("drive", el("rotational.torque_source", "Torque source").at([-0.06, 0.04, 0.], X).look(cyl(0.008, 0.02), DARK, None))
            .add("wheel", el("rotational.inertia", "Wheel").set("A small wheel, 1e-4 kg·m².", &[("inertia", 1e-4)]).at([-0.025, 0.04, 0.], X).model("flywheel", cyl(0.03, 0.01), ALU))
            .add("encoder", el("sensor.encoder", "Encoder, 64 counts/turn").set("A cheap magnetic encoder: 64 counts per turn (5.6° per count), read by the firmware every millisecond.", &[("counts", 64.0), ("period", 0.001)]).at([0.0, 0.04, 0.], X).look(cyl(0.012, 0.006), SIGNAL, None))
            .add("estimate", el("part.speed_estimate", "Speed estimate").set("Speed from the counts, filtered over 5 ms.", &[("tau", 0.005)]).at([0.02, 0.06, 0.], Y))
            .join("command", &[("push", "torque"), ("drive", "torque")])
            .join("shaft", &[("drive", "shaft"), ("wheel", "shaft"), ("encoder", "shaft")])
            .join("counts", &[("encoder", "angle"), ("estimate", "angle")]);
        let doc = system(registry, "Encoder counts and speed", "A wheel speeds up slowly under a small torque. An encoder reports its angle in whole counts, and firmware estimates speed from them: the counts' size limits what it can know.", vec![], vec![l], 2e-4, BE, "The encoder's counts are steps: backward Euler, 0.2 ms.");
        out.push(Built { slug: "encoder-resolution", file: "encoder.system.json", doc });
    }

    // 17 and 18. A gearmotor joint lifting an arm: continuous PID, then firmware.
    for (slug, firmware) in [("pid-joint", false), ("loop-rate", true)] {
        let why = "A 37 mm 12 V gearmotor, 30:1 (333 rpm no-load, ≈ 1.5 N·m stall); catalogue-style values.";
        let mut l = Level::new("");
        supply(&mut l, -0.14, 0.02, 12.0);
        l.add("target", el("part.angle_setpoint", "Target angle").set("A step from hanging (0) to 1 rad at 0.2 s (a fast 50 ms ramp).", &[("start", 0.0), ("rate", 20.0), ("ramp_from", 0.2), ("ramp_for", 0.05)]).at([-0.14, 0.08, 0.], Y))
            .add("driver", el("robot.h_bridge", "Driver").set("A MOSFET H-bridge.", &[("on_resistance", 0.05)]).at([-0.1, 0.03, 0.], Y).model("driver_board", cube(0.04, 0.01, 0.03), [0.2, 0.45, 0.3]))
            .add("frame", el("rotational.ground", "Mount").at([-0.06, 0.07, -0.02], Y))
            .add("motor", el("bridge.brushed_motor", "Motor").set(why, &[("resistance", 2.2), ("torque_constant", 0.0115), ("back_emf_constant", 0.0115), ("inductance", 0.0)]).at([-0.06, 0.1, -0.03], X))
            .add("rotor", el("rotational.inertia", "Rotor").set(why, &[("inertia", 2e-6)]).at([-0.04, 0.1, -0.03], X).look(cyl(0.002, 0.004), STEEL, None))
            .add("gearbox", el("rotational.lossy_gear", "Gearbox 30:1").set(why, &[("ratio", 30.0), ("efficiency", 0.8)]).at([-0.025, 0.1, -0.03], X))
            .add("arm", el("part.pendulum_gravity", "Arm").set("A 0.3 kg arm with its centre of mass 0.12 m from the joint.", &[("mass", 0.3), ("arm", 0.12)]).at([0.0, 0.1, 0.], Z))
            .add("arm_inertia", el("rotational.inertia", "Arm inertia").set("m·r² = 0.3 × 0.12².", &[("inertia", 0.00432)]).at([0.0, 0.1, 0.005], Z).look(cyl(0.008, 0.01), STEEL, None))
            .add("encoder", el("sensor.encoder", "Joint encoder").set("A joint encoder; ideal here.", &[]).at([0.0, 0.1, -0.012], Z).look(cyl(0.01, 0.004), SIGNAL, None))
            .join("V+", &[("supply", "p"), ("driver", "supply_p")])
            .join("0 V", &[("supply", "n"), ("driver", "supply_n"), ("gnd", "pin")])
            .join("motor +", &[("driver", "p"), ("motor", "p")])
            .join("motor − (0 V: the averaged bridge output is referenced here)", &[("driver", "n"), ("motor", "n"), ("gnd", "pin")])
            .join("motor case", &[("motor", "case"), ("frame", "flange")])
            .join("", &[("motor", "shaft"), ("rotor", "shaft"), ("gearbox", "input")])
            .join("joint", &[("gearbox", "output"), ("arm", "shaft"), ("arm_inertia", "shaft"), ("encoder", "shaft")]);
        if firmware {
            l.add("firmware", el("robot.servo_firmware", "Controller firmware").set("A microcontroller loop: PD at 1 kHz with no delay (scenes slow it down).", &[("rate", 1000.0), ("latency", 0.0), ("kp", 4.0), ("ki", 0.0), ("kd", 0.12), ("limit", 1.0), ("offset", 0.0), ("deadband", 0.0), ("resolution", 0.0)]).at([-0.1, 0.06, 0.], Y).model("soic8", cube(0.005, 0.0015, 0.004), DARK))
                .add("tach", el("rotational.speed_sensor", "Joint speed").at([0.0, 0.08, -0.012], Y))
                .join("target", &[("target", "angle"), ("firmware", "target")])
                .join("measured", &[("encoder", "angle"), ("firmware", "measured")])
                .join("rate", &[("tach", "speed"), ("firmware", "rate")])
                .join("", &[("tach", "shaft"), ("arm", "shaft")])
                .join("command", &[("firmware", "command"), ("driver", "command")]);
        } else {
            l.add("pid", el("part.pid_angle", "PID controller").set("Tuned by hand in the lesson: start with P only.", &[("kp", 4.0), ("ki", 0.0), ("kd", 0.0), ("filter", 0.005), ("limit", 1.0), ("offset", 0.0)]).at([-0.1, 0.06, 0.], Y).model("soic8", cube(0.005, 0.0015, 0.004), DARK))
                .join("target", &[("target", "angle"), ("pid", "target")])
                .join("measured", &[("encoder", "angle"), ("pid", "measured")])
                .join("command", &[("pid", "command"), ("driver", "command")]);
        }
        let (title, description) = if firmware {
            ("Arm joint under sampled control", "The same arm joint, now run by firmware that reads the encoder and updates the motor command only at its loop rate, after a delay.")
        } else {
            ("Arm joint under PID control", "A gearmotor lifts a 0.3 kg arm to a target angle under a PID controller. Proportional action pulls, derivative action brakes, integral action removes the steady sag gravity causes.")
        };
        let doc = system(registry, title, description, vec![], vec![l], if firmware { 1e-4 } else { 5e-4 }, BE, "Driver saturation and (for firmware) held samples: backward Euler; the step resolves the loop period.");
        out.push(Built { slug, file: "joint.system.json", doc });
    }

    // 19. IMU tilt: a swinging leg carries an IMU; three tilt estimates.
    {
        let mut l = Level::new("");
        l.add("kick", el("part.torque_command", "Kick").set("A short push that starts the leg swinging: 0.4 N·m for 0.14 s (a swing of about ±0.4 rad).", &[("step", 0.4), ("start", 0.2), ("duration", 0.14), ("edge", 0.005)]).at([-0.1, 0.1, 0.], Y))
            .add("push", el("rotational.torque_source", "Hip torque").at([-0.03, 0.1, -0.02], Z).look(cyl(0.012, 0.012), DARK, None))
            .add("leg", el("part.pendulum_gravity", "Leg").set("A 0.5 kg leg with its centre of mass 0.2 m below the hip.", &[("mass", 0.5), ("arm", 0.2)]).at([0.0, 0.1, 0.], Z))
            .add("leg_inertia", el("rotational.inertia", "Leg inertia and hip friction").set("m·r² = 0.5 × 0.2², with some damping in the hip so the swing dies out within a few seconds.", &[("inertia", 0.02), ("damping", 0.04)]).at([0.0, 0.1, 0.005], Z).look(cyl(0.01, 0.01), STEEL, None))
            .add("imu", el("part.tilt_imu", "IMU on the leg").set("A cheap MEMS IMU 0.18 m down the leg: gyro bias 0.05 rad/s (3°/s, uncalibrated), 1.5 m/s² of motor vibration.", &[("radius", 0.18), ("bias", 0.05), ("vibration", 1.5), ("bandwidth", 0.002)]).at([0.0, -0.08, 0.012], Z).look(cube(0.018, 0.004, 0.018), SIGNAL, Some("sot23")))
            .add("filter", el("part.complementary_tilt", "Tilt filter").set("A complementary filter with a 0.5 s crossover.", &[("time_constant", 0.5)]).at([-0.08, 0.06, 0.], Y).model("soic8", cube(0.005, 0.0015, 0.004), DARK))
            .join("command", &[("kick", "torque"), ("push", "torque")])
            .join("hip", &[("push", "shaft"), ("leg", "shaft"), ("leg_inertia", "shaft"), ("imu", "shaft")])
            .join("ax", &[("imu", "ax"), ("filter", "ax")])
            .join("ay", &[("imu", "ay"), ("filter", "ay")])
            .join("gyro", &[("imu", "gyro"), ("filter", "gyro")]);
        let doc = system(registry, "Tilt from an IMU", "A leg swings on its hip, carrying an IMU. Its accelerometer, its gyro and a complementary filter each estimate the leg's tilt, and each is wrong in its own way.", vec![], vec![l], 5e-4, BE, "Filtered accelerometer and vibration at 87/131 Hz: backward Euler at 0.5 ms (15 steps per vibration cycle).");
        out.push(Built { slug: "imu-tilt", file: "leg.system.json", doc });
    }

    // 20. Torque control: a PI current loop holds the current while speed climbs.
    {
        let mut l = Level::new("");
        supply(&mut l, -0.14, 0.02, 12.0);
        can_motor_on(&mut l, -0.04, 0.04, "load", el("rotational.inertia", "Wheel").set("A wheel on the shaft with the rotor: 2.3e-5 kg·m².", &[("inertia", 2.3e-5)]).at([0.0, 0.04, 0.], X).model("flywheel", cyl(0.025, 0.008), ALU));
        l.commands.iter_mut().for_each(|c| {
            if let Command::AddInstance { name, instance, .. } = c {
                if name == "motor" {
                    instance.parameters.insert("inductance".into(), est(0.002, "A larger winding inductance (2 mH, L/R = 1 ms) so the current loop's own speed can be seen."));
                }
            }
        });
        l.add("loop", el("part.pi_current", "Current loop").set("A PI current regulator in the driver's firmware: target 2 A (24 mN·m) from 10 ms.", &[("target", 2.0), ("start", 0.01), ("kp", 0.3), ("ki", 300.0), ("limit", 1.0)]).at([-0.12, 0.07, 0.], Y).model("soic8", cube(0.005, 0.0015, 0.004), DARK))
            .add("driver", el("robot.h_bridge", "Driver").set("A MOSFET H-bridge.", &[("on_resistance", 0.05)]).at([-0.1, 0.03, 0.], Y).model("driver_board", cube(0.04, 0.01, 0.03), [0.2, 0.45, 0.3]))
            .add("shunt", el("sensor.current", "Current sensor").set("A shunt amplifier in the motor lead; ideal here.", &[]).at([-0.07, 0.05, 0.], Y).model("sot23", cube(0.003, 0.0012, 0.0023), DARK))
            .join("V+", &[("supply", "p"), ("driver", "supply_p")])
            .join("0 V", &[("supply", "n"), ("driver", "supply_n"), ("gnd", "pin")])
            .join("measured", &[("shunt", "current"), ("loop", "measured")])
            .join("command", &[("loop", "command"), ("driver", "command")])
            .join("to shunt", &[("driver", "p"), ("shunt", "p")])
            .join("motor +", &[("shunt", "n"), ("motor", "p")])
            .join("motor − (0 V: the averaged bridge output is referenced here)", &[("driver", "n"), ("motor", "n"), ("gnd", "pin")]);
        let mut doc = system(registry, "Torque control through current", "A PI loop holds the motor's current at 2 A while a wheel spins up. Constant current means constant torque, until the supply runs out of voltage to push it against the back-EMF.", vec![], vec![l], 5e-5, BE, "Current loop and 1 ms winding: backward Euler at 50 µs.");
        use sim_system::Reduce::*;
        doc.studies.insert("target".into(), sweep("loop", "target", &[1.0, 2.0, 3.0, 4.0], 0.2, &["motor.p.current", "load.shaft.speed"], vec![metric("current at 0.1 s (A)", "motor.p.current", Mean, [0.09, 0.1]), metric("speed at 0.1 s (rad/s)", "load.shaft.speed", Final, [0.0, 0.1])]));
        out.push(Built { slug: "current-control", file: "torque.system.json", doc });
    }

    // Joining 3D-printed parts. Printed-part values are estimates for PLA
    // printed on a hobby printer (E ≈ 3.5 GPa; about 30 MPa bending and
    // 17 MPa shear strength across layers), each recorded with its reason.
    const PRINT_A: [f32; 3] = [0.93, 0.55, 0.16];
    const PRINT_B: [f32; 3] = [0.20, 0.46, 0.72];
    const BRASS: [f32; 3] = [0.80, 0.62, 0.25];
    let pin_why = "A Ø3 mm steel dowel pin in printed PLA holes 6 mm deep; wall stiffness E·L/2 with E = 3.5 GPa (estimate).";
    // 30. Alignment pins: one pin with clearance, pushed back and forth.
    {
        let mut l = Level::new("");
        l.add("base", el("translational.ground", "Base part").at([0., -0.008, 0.], Y).model("pin_base", cube(0.1, 0.012, 0.05), PRINT_B))
            .add("plate", el("translational.mass", "Top plate").set("A 150 g printed plate resting on the base, free to shift sideways.", &[("mass", 0.15)]).at([0., 0.004, 0.], X).model("pin_plate", cube(0.006, 0.07, 0.04), PRINT_A))
            .add("pin", el("part.dowel_pin", "Dowel pin").set(pin_why, &[("clearance", 0.0001), ("diameter", 0.003), ("engaged", 0.006), ("wall_modulus", 3.5e9)]).at([0., 0.0, 0.], Y).model("dowel_pin", cyl(0.0015, 0.014), STEEL))
            .add("rub", el("translational.damper", "Sliding friction (stand-in)").set("The plate sliding on the base, as viscous drag (300 N·s/m): a stand-in for the friction of a lightly clamped plate, so it drifts rather than jumps across the clearance.", &[("damping", 300.0)]).at([0.03, -0.002, 0.03], Y))
            .add("push", el("part.force_command", "Push (±20 N, 0.5 Hz)").set("A hand pushing the plate sideways, back and forth: ±20 N once every 2 s.", &[("amplitude", 20.0), ("frequency", 0.5)]).at([-0.07, 0.02, 0.], Y).look(cube(0.006, 0.006, 0.006), SIGNAL, None))
            .add("hand", el("translational.force_source", "Sideways force").at([-0.045, 0.004, 0.], X).look(cube(0.004, 0.01, 0.004), SIGNAL, None))
            .join("plate", &[("plate", "axis"), ("pin", "a"), ("rub", "a"), ("hand", "axis")])
            .join("base", &[("base", "axis"), ("pin", "b"), ("rub", "b")])
            .join("push", &[("push", "force"), ("hand", "force")]);
        let doc = system(registry, "Plate on a dowel pin", "A printed plate located on a base by one steel pin in a slightly larger printed hole, pushed sideways back and forth.", vec![], vec![l], 1e-4, BE, "Stiff pin contact (10 MN/m) on a 150 g plate: backward Euler at 0.1 ms.");
        out.push(Built { slug: "alignment-pins", file: "pins.system.json", doc });
    }
    // Two round pins whose holes are 0.2 mm further apart than the pins.
    {
        let mut l = Level::new("");
        l.add("base", el("translational.ground", "Base part (first pin)").at([0., -0.008, 0.], Y).model("pin_base_two", cube(0.1, 0.012, 0.05), PRINT_B))
            .add("second", el("translational.ground", "Base part (second pin, 0.2 mm off)").set("The second pin sits 0.2 mm from where the plate's second hole expects it: a printing or drilling error.", &[("position", 0.0002)]).at([0.03, -0.008, 0.], Y))
            .add("plate", el("translational.mass", "Top plate").set("A 150 g printed plate on two pins 60 mm apart.", &[("mass", 0.15)]).at([0., 0.004, 0.], X).model("pin_plate_two", cube(0.006, 0.09, 0.04), PRINT_A))
            .add("pin1", el("part.dowel_pin", "First pin").set(pin_why, &[("clearance", 0.00005), ("diameter", 0.003), ("engaged", 0.006), ("wall_modulus", 3.5e9)]).at([-0.03, 0.0, 0.], Y).model("dowel_pin", cyl(0.0015, 0.014), STEEL))
            .add("pin2", el("part.dowel_pin", "Second pin").set(pin_why, &[("clearance", 0.00005), ("diameter", 0.003), ("engaged", 0.006), ("wall_modulus", 3.5e9)]).at([0.03, 0.0, 0.], Y).model("dowel_pin", cyl(0.0015, 0.014), STEEL))
            .add("rub", el("translational.damper", "Sliding friction (stand-in)").set("The plate sliding on the base, as viscous drag: a stand-in for friction.", &[("damping", 300.0)]).at([0.0, -0.002, 0.03], Y))
            .join("plate", &[("plate", "axis"), ("pin1", "a"), ("pin2", "a"), ("rub", "a")])
            .join("base", &[("base", "axis"), ("pin1", "b"), ("rub", "b")])
            .join("second hole", &[("second", "axis"), ("pin2", "b")]);
        let doc = system(registry, "Two round pins, holes 0.2 mm off", "The plate has two round holes 60 mm apart, but the base's pins are 60.2 mm apart. With 0.05 mm of clearance each, the plate cannot sit on both without pushing them apart.", vec![], vec![l], 1e-4, BE, "Stiff pin contact on a 150 g plate: backward Euler at 0.1 ms.");
        out.push(Built { slug: "alignment-pins", file: "twopins.system.json", doc });
    }
    // 31. Dovetails: pulling on one, and sliding one in.
    {
        let dt_why = "A printed dovetail 20 mm long: flanks at 15° from the pull; each flank's contact about 2 MN/m; walls 4 mm thick and 6 mm tall, 3EI/h³ ≈ 5 MN/m (estimates for PLA).";
        let dt = |label: &str, extra: &[(&str, f64)]| {
            let mut p = el("part.dovetail", label).set(dt_why, &[("angle", 0.2618), ("flank_stiffness", 2e6), ("wall_stiffness", 5e6), ("friction", 0.3)]);
            p = p.set("The fit in this rig.", extra);
            p
        };
        let mut l = Level::new("");
        l.add("socket", el("translational.ground", "Socket part (fixed)").at([-0.05, -0.01, 0.], Y).model("dovetail_socket", cube(0.04, 0.02, 0.03), PRINT_B))
            .add("tail", el("translational.mass", "Tail part").set("The 30 g part carrying the tail, pulled straight out of the socket.", &[("mass", 0.03)]).at([-0.05, 0.012, 0.], Y).model("dovetail_tail", cube(0.024, 0.024, 0.03), PRINT_A))
            .add("fixed", el("translational.ground", "No sliding in this rig").at([-0.08, -0.02, 0.], Y))
            .add("joint", dt("Dovetail (pulled)", &[("clearance", 0.0), ("interference", 0.0)]).at([-0.05, -0.004, 0.018], Y))
            .add("hold", el("translational.damper", "Settling (stand-in)").set("Light damping so the tail settles; it does not carry load at these speeds.", &[("damping", 50.0)]).at([-0.07, 0.0, 0.02], Y))
            .add("pull", el("part.force_command", "Pull (100 N/s up to 200 N)").set("Pulling the tail straight out, more and more: 100 N per second, up to 200 N.", &[("ramp", 100.0), ("limit", 200.0)]).at([-0.05, 0.05, 0.], Y).look(cube(0.006, 0.006, 0.006), SIGNAL, None))
            .add("puller", el("translational.force_source", "Pulling force").at([-0.05, 0.035, 0.], Y).look(cube(0.004, 0.01, 0.004), SIGNAL, None))
            .join("tail", &[("tail", "axis"), ("joint", "tail"), ("hold", "a"), ("puller", "axis")])
            .join("socket", &[("socket", "axis"), ("joint", "socket"), ("hold", "b")])
            .join("no slide", &[("fixed", "axis"), ("joint", "slide"), ("joint", "rail")])
            .join("pull", &[("pull", "force"), ("puller", "force")]);
        l.add("groove", el("translational.ground", "Grooved part (fixed)").at([0.06, -0.01, 0.], Y).model("dovetail_groove_long", cube(0.08, 0.02, 0.03), PRINT_B))
            .add("slider", el("translational.mass", "Sliding part").set("The 30 g part whose tail slides along the groove.", &[("mass", 0.03)]).at([0.03, 0.012, 0.], X).model("dovetail_slider", cube(0.024, 0.024, 0.03), PRINT_A))
            .add("fixed2", el("translational.ground", "No pulling in this rig").at([0.1, -0.02, 0.], Y))
            .add("fit", dt("Dovetail (slid in)", &[("clearance", 0.0), ("interference", 0.00002), ("smoothing", 0.0001)]).at([0.06, -0.004, 0.018], Y))
            .add("hand", el("translational.damper", "Your hand (stand-in)").set("Your hand's own give while pushing, as 100 N·s/m of drag: a stand-in, so the part moves at hand speed once it breaks free.", &[("damping", 100.0)]).at([0.03, 0.03, 0.02], Y))
            .add("stop", el("part.hard_stop_linear", "Fully home (50 mm)").set("The groove ends 50 mm in.", &[("lower", -0.01), ("upper", 0.05)]).at([0.1, 0.012, 0.], X))
            .add("push", el("part.force_command", "Push (5 N/s up to 40 N)").set("Pushing the part along the groove, harder and harder: 5 N per second, up to 40 N (a firm push by hand).", &[("ramp", 5.0), ("limit", 40.0)]).at([0.0, 0.05, 0.], Y).look(cube(0.006, 0.006, 0.006), SIGNAL, None))
            .add("pusher", el("translational.force_source", "Pushing force").at([0.01, 0.012, 0.], X).look(cube(0.004, 0.01, 0.004), SIGNAL, None))
            .join("slider", &[("slider", "axis"), ("fit", "slide"), ("hand", "a"), ("stop", "axis"), ("pusher", "axis")])
            .join("groove", &[("groove", "axis"), ("fit", "rail"), ("hand", "b")])
            .join("no pull", &[("fixed2", "axis"), ("fit", "tail"), ("fit", "socket")])
            .join("push", &[("push", "force"), ("pusher", "force")]);
        let doc = system(registry, "Dovetail: pulling out and sliding in", "Left: a dovetail's tail pulled straight out of its socket, harder and harder. Right: a tight dovetail pushed along its groove until it breaks free and slides home.", vec![], vec![l], 2e-4, BE, "Stiff flank contacts and regularised friction: backward Euler at 0.2 ms.");
        out.push(Built { slug: "dovetails", file: "dovetail.system.json", doc });
    }
    // 32. Screw bosses: a heat-set insert and a self-tapped screw, pulled out.
    {
        let tj = "Printed PLA boss pulled along the layer stacking; shear strength about 17 MPa there (estimate).";
        let mut l = Level::new("");
        for (x, prefix, label, d, len, eff, why) in [
            (-0.03, "insert", "M3 heat-set insert", 0.0056, 0.0057, 0.5, "An M3 brass insert (Ø5.6 mm outside, 5.7 mm long); its knurls grip about half the melted-in surface (estimate)."),
            (0.03, "tapped", "M3 screw, self-tapped", 0.003, 0.006, 0.4, "An M3 screw cut straight into a Ø2.5 mm printed hole, 6 mm deep; the thread fills about 40 % of the shear surface (estimate)."),
        ] {
            let boss = format!("{prefix}_boss");
            let screw = format!("{prefix}_screw");
            let joint = format!("{prefix}");
            let drag = format!("{prefix}_drag");
            let stop = format!("{prefix}_stop");
            let force = format!("{prefix}_force");
            l.add(&boss, el("translational.ground", &format!("Boss ({label})")).at([x, -0.012, 0.], Y).model(if prefix == "insert" { "insert_boss" } else { "tapped_boss" }, cyl(0.006, 0.024), PRINT_B))
                .add(&screw, el("translational.mass", &format!("Screw ({label})")).set("A screw with a light hanger: 10 g.", &[("mass", 0.01)]).at([x, 0.006, 0.], Y).model("m3_screw", cyl(0.0028, 0.021), STEEL))
                .add(&joint, el("part.threaded_joint", label).set(&format!("{tj} {why}"), &[("diameter", d), ("length", len), ("shear_strength", 1.7e7), ("efficiency", eff), ("peak_displacement", 0.0003)]).at([x + 0.008, 0.0, 0.008], Y))
                .add(&drag, el("translational.damper", "Tearing drag (stand-in)").set("Drag on the escaping screw (3000 N·s/m), so its escape can be watched: before the peak it adds under 0.4 N, so it does not change the peak at this slow pull.", &[("damping", 3000.0)]).at([x + 0.012, 0.0, 0.01], Y))
                .add(&stop, el("part.hard_stop_linear", "Catch (8 mm)").set("A catch 8 mm up, where the escaping screw is stopped.", &[("lower", -0.01), ("upper", 0.008), ("stiffness", 1e7)]).at([x, 0.02, 0.], Y))
                .add(&force, el("translational.force_source", "Pull on the screw").at([x, 0.03, 0.], Y).look(cube(0.004, 0.01, 0.004), SIGNAL, None))
                .join(&screw, &[(&screw, "axis"), (&joint, "screw"), (&drag, "a"), (&stop, "axis"), (&force, "axis")])
                .join(&boss, &[(&boss, "axis"), (&joint, "boss"), (&drag, "b")]);
        }
        l.add("pull", el("part.force_command", "Pull (150 N/s)").set("Hanging weight added steadily: 150 N per second, up to 1200 N (a pull-out test).", &[("ramp", 150.0), ("limit", 1200.0)]).at([0., 0.05, 0.], Y).look(cube(0.006, 0.006, 0.006), SIGNAL, None))
            .join("pull", &[("pull", "force"), ("insert_force", "force"), ("tapped_force", "force")]);
        let doc = system(registry, "Pull-out test: heat-set insert vs self-tapped screw", "Two M3 screws in the same printed PLA, pulled straight out by the same growing force: one in a brass heat-set insert, one cut straight into the plastic.", vec![], vec![l], 1e-4, BE, "Tearing curves and a fast escape onto a catch: backward Euler at 0.1 ms.");
        out.push(Built { slug: "heat-set-inserts", file: "pullout.system.json", doc });
    }
    out
}

trait Initial {
    fn with_initial(self, key: &str, value: f64) -> Self;
}
impl Initial for P {
    fn with_initial(mut self, key: &str, value: f64) -> Self {
        self.0.parameters.insert(key.into(), ParameterBinding::value(value));
        self
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (registry, loaded) = sim_runtime::registry_with_parts(&root.join("library/parts"));
    for l in loaded.iter().filter(|l| l.error.is_some()) {
        panic!("part not loaded: {}", l.error.as_deref().unwrap_or_default());
    }
    let only: Vec<String> = std::env::args().skip(1).collect();
    REGISTRY.with(|r| *r.borrow_mut() = Some(registry.clone()));
    for b in build(&registry) {
        if !only.is_empty() && !only.iter().any(|s| s == b.slug) {
            continue;
        }
        let dir = root.join("lessons").join(b.slug);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(b.file);
        std::fs::write(&path, serde_json::to_vec_pretty(&b.doc).unwrap()).unwrap();
        println!("wrote {}", path.display());
    }
}
