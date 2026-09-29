//! Behaviors whose ports span two physical domains.

use sim_core::{signal_out,
    Behavior, Input, LocalJacobian, Output, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const THERMISTOR: &str = "bridge.thermistor";
pub const BRUSHED_MOTOR: &str = "bridge.brushed_motor";
pub const MOTOR: &str = "bridge.motor";
pub const LEAD_SCREW: &str = "bridge.lead_screw";
pub const DUAL_DRIVE: &str = "bridge.dual_drive";
pub const THERMOELASTIC_LAYER: &str = "bridge.thermoelastic_layer";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Resistor whose resistance follows its own temperature,
/// `R = R₀·exp(α·(T − T_ref))`, dumping its dissipation into the thermal port.
pub struct Thermistor {
    pub resistance: f64,
    pub coefficient: f64,
    pub reference: f64,
}
impl Thermistor {
    pub fn resistance_at(&self, temperature: f64) -> f64 {
        self.resistance * (self.coefficient * (temperature - self.reference)).exp()
    }
}
impl Behavior for Thermistor {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let r = self.resistance_at(ctx.across(2));
        let v = ctx.across(0) - ctx.across(1);
        let i = v / r;
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
        ctx.add_through(2, -v * i);
    }
}
fn thermistor(p: &Params) -> Made {
    Ok(Box::new(Thermistor { resistance: param(p, "resistance")?, coefficient: param(p, "coefficient")?, reference: param(p, "reference")? }))
}

/// Brushed DC motor between pins `p`, `n` and a shaft against its case.
/// With zero inductance the winding current is algebraic.
pub struct BrushedMotor {
    pub resistance: f64,
    pub inductance: f64,
    pub torque_constant: f64,
    pub back_emf_constant: f64,
}
impl Behavior for BrushedMotor {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("current", QuantityKind::Current, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let i = ctx.state(0);
        let v = ctx.across(0) - ctx.across(1);
        let speed = ctx.across_rate(2) - ctx.across_rate(3);
        let back_emf = self.back_emf_constant * speed;
        let residual = if self.inductance > 0.0 {
            self.inductance * ctx.state_rate(0) - (v - self.resistance * i - back_emf)
        } else {
            self.resistance * i - (v - back_emf)
        };
        ctx.set_state_residual(0, residual);
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
        let torque = self.torque_constant * i;
        ctx.add_through(2, -torque);
        ctx.add_through(3, torque);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        // Winding row: L·i' + R·i − v + k_e·(ω₂ − ω₃) (or R·i − v + k_e·Δω).
        if self.inductance > 0.0 {
            out.state_rate(0, 0, self.inductance);
        }
        out.state_state(0, 0, self.resistance);
        out.set(Output::State(0), Input::Across(0, 0), -1.0);
        out.set(Output::State(0), Input::Across(1, 0), 1.0);
        out.set(Output::State(0), Input::AcrossRate(2, 0), self.back_emf_constant);
        out.set(Output::State(0), Input::AcrossRate(3, 0), -self.back_emf_constant);
        out.through(0, Input::State(0), 1.0);
        out.through(1, Input::State(0), -1.0);
        out.through(2, Input::State(0), -self.torque_constant);
        out.through(3, Input::State(0), self.torque_constant);
        true
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.inductance * view.state(0).powi(2)
    }
}
fn brushed_motor(p: &Params) -> Made {
    Ok(Box::new(BrushedMotor {
        resistance: param(p, "resistance")?,
        inductance: param_or(p, "inductance", 0.0),
        torque_constant: param(p, "torque_constant")?,
        back_emf_constant: param(p, "back_emf_constant")?,
    }))
}

/// A motor behind one `Motor` plug (`ConnectorKind::MOTOR`): winding
/// terminal with its return through the chassis, shaft, and case. The
/// winding resistance follows the thermistor law `R₀·exp(α(T − T_ref))`,
/// so a negative coefficient makes two motors on one drive hog exactly as
/// two thermistors do.
pub struct Motor {
    pub resistance: f64,
    pub coefficient: f64,
    pub reference: f64,
    pub torque_constant: f64,
}
impl Motor {
    pub fn resistance_at(&self, temperature: f64) -> f64 {
        self.resistance * (self.coefficient * (temperature - self.reference)).exp()
    }
}
impl Behavior for Motor {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let plug = ConnectorKind::MOTOR;
        let (winding, shaft, case) = (plug.member_offset(0), plug.member_offset(1), plug.member_offset(2));
        let v = ctx.across_lane(0, winding);
        let speed = ctx.across_rate_lane(0, shaft);
        let r = self.resistance_at(ctx.across_lane(0, case));
        let i = (v - self.torque_constant * speed) / r;
        ctx.add_through_lane(0, winding, i);
        ctx.add_through_lane(0, shaft, -self.torque_constant * i);
        ctx.add_through_lane(0, case, -i * i * r);
    }
}
fn motor(p: &Params) -> Made {
    Ok(Box::new(Motor {
        resistance: param(p, "resistance")?,
        coefficient: param_or(p, "coefficient", 0.0),
        reference: param_or(p, "reference", 293.15),
        torque_constant: param(p, "torque_constant")?,
    }))
}

/// A drive with two `Motor` sockets: it regulates the total winding current
/// and lets the two windings share it through one internal bus, touches
/// neither shaft nor case, and reports the hotter case — a drive that knows
/// its motor is hot.
pub struct DualDrive {
    pub current: f64,
}
impl Behavior for DualDrive {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("current_a", QuantityKind::Current, 0.5 * self.current)]
    }
    fn residual(&self, ctx: &mut Context) {
        let plug = ConnectorKind::MOTOR;
        let (winding, case) = (plug.member_offset(0), plug.member_offset(2));
        let current_a = ctx.state(0);
        // One bus: both sockets sit at the same potential.
        ctx.set_state_residual(0, ctx.across_lane(0, winding) - ctx.across_lane(1, winding));
        ctx.add_through_lane(0, winding, -current_a);
        ctx.add_through_lane(1, winding, -(self.current - current_a));
        ctx.set_signal(0, ctx.across_lane(0, case).max(ctx.across_lane(1, case)));
    }
}
fn dual_drive(p: &Params) -> Made {
    Ok(Box::new(DualDrive { current: param(p, "current")? }))
}

/// One layer of a bending beam's cross-section: strain rate heats it,
/// its temperature bends the beam back. Ports: bending (across curvature,
/// through moment) and the layer's thermal node.
pub struct ThermoelasticLayer {
    /// Sign of the coupling; anything but +1 is unphysical and exists only
    /// so a scenario can show the compiler rejecting it.
    pub sign: f64,
    pub height: f64,
    pub thickness: f64,
    pub width: f64,
    pub youngs_modulus: f64,
    pub expansion: f64,
    pub temperature: f64,
}
impl Behavior for ThermoelasticLayer {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let area = self.width * self.thickness;
        let coupling = self.sign * self.youngs_modulus * self.expansion * self.height * area;
        // Thermal moment into the beam's balance; heat out of the layer node.
        let moment = coupling * ctx.across(1);
        let heating = coupling * self.temperature * ctx.across_rate(0);
        ctx.add_through(0, moment);
        ctx.add_through(1, -heating);
        // Thermoelastic coupling is reversible: the heat it exchanges is
        // stored, not produced. (Reverse its sign and it is neither: the
        // compiler rejects the model.)
        ctx.store_entropy(-heating / ctx.across(1));
    }
}
fn thermoelastic_layer(p: &Params) -> Made {
    Ok(Box::new(ThermoelasticLayer {
        sign: param_or(p, "sign", 1.0),
        height: param(p, "height")?,
        thickness: param(p, "thickness")?,
        width: param(p, "width")?,
        youngs_modulus: param(p, "youngs_modulus")?,
        expansion: param(p, "expansion")?,
        temperature: param(p, "temperature")?,
    }))
}

fn lead_screw(p: &Params) -> Made {
    Ok(Box::new(sim_domain_rotational::helical::HelicalDrive {
        contact: sim_domain_rotational::helical::screw_contact(p),
        smoothing_speed: param_or(p, "smoothing_speed", 0.01),
        correction: param_or(p, "correction", 10.0),
    }))
}

use sim_core::{ComponentNotes as Notes, DerivedValue};

static BRUSHED_MOTOR_NOTES: Notes = Notes { category: "Actuators",
    explanation: "Current through the armature winding in the magnet's field makes torque (τ = k_t·i); spinning makes a back-EMF opposing the supply (e = k_e·ω). The winding's resistance and inductance limit the current. The torque reacts on the case, so ground the case (or mount it on something that can turn).",
    equations: &["L·di/dt = v − R·i − k_e·(ω_shaft − ω_case)", "τ_shaft = k_t·i  (reaction −k_t·i on the case)", "in SI units k_t = k_e"],
    tradeoffs: "Cheap, simple to drive (just a voltage), with linear speed-torque behaviour: stall torque V·k_t/R, no-load speed V/k_e. Brushes wear and spark. Small motors spin fast with little torque, so they almost always need a gear stage; picking the ratio trades top speed for force. Compare brushless motors (more efficient, need commutation electronics).",
    limits: "Constant resistance (no heating; see bridge.motor for temperature-dependent resistance), no brush drop, no magnetic saturation, cogging or friction; add a rotor inertia on the shaft.",
    parameters: &[("resistance", "Winding (terminal) resistance R."), ("inductance", "Winding inductance L; sets the electrical time constant L/R."), ("torque_constant", "Torque per amp k_t."), ("back_emf_constant", "Volts per rad/s k_e (equal to k_t in SI for an ideal machine).")],
    typical: &[("resistance", 2.0), ("inductance", 0.0005), ("torque_constant", 0.012), ("back_emf_constant", 0.012)],
    realtime: &[("inductance", 0.0)],
    pairs_with: &[sim_domain_rotational::elements::INERTIA, sim_domain_rotational::elements::WORM_GEAR, sim_domain_rotational::elements::IDEAL_GEAR, LEAD_SCREW, sim_domain_rotational::elements::GROUND, "electrical.switched_voltage_source", "electrical.voltage_source", "electrical.ground", sim_domain_rotational::elements::SPEED_SENSOR],
    derived: Some(|p| {
        let r = p.get("resistance").copied().unwrap_or(f64::NAN);
        let (kt, ke) = (p.get("torque_constant").copied().unwrap_or(f64::NAN), p.get("back_emf_constant").copied().unwrap_or(f64::NAN));
        let l = p.get("inductance").copied().unwrap_or(0.);
        vec![
            DerivedValue::new("stall torque per volt", kt / r, "N·m/V", "k_t / R"),
            DerivedValue::new("no-load speed per volt", 1. / ke, "rad/s/V", "1 / k_e"),
            DerivedValue::new("motor constant", kt / r.sqrt(), "N·m/√W", "K_m = k_t / √R (torque per root copper loss)"),
            DerivedValue::new("electrical time constant", l / r, "s", "τ_e = L / R"),
        ]
    }),
    ..Notes::new("A permanent-magnet brushed DC motor: voltage in, torque out.")
};

static LEAD_SCREW_NOTES: Notes = Notes { category: "Transmissions",
    explanation: "A threaded screw turning in a nut converts rotation to straight-line travel: one turn advances the nut by the lead. It is the linear-actuator cousin of the worm gear (the nut is a 'wheel' of infinite radius), so the same sliding-thread force model applies: lead angle, flank angle and friction set the efficiency and whether the load can back-drive the screw.",
    equations: &["x_nut = lead·θ / 2π", "tan λ = lead / (π·d)", "raising torque τ = F·d/2 · (cos φₙ tan λ + μ) / (cos φₙ − μ tan λ)", "self-locking when μ ≥ cos φₙ tan λ"],
    tradeoffs: "Trapezoidal (Acme) screws with bronze or plastic nuts are cheap, quiet and usually self-locking, so a vertical axis holds with the motor off, but they run at 20–50 % efficiency and wear. Ball screws replace sliding with rolling (μ ≈ 0.005): about 90 % efficient and precise, but they back-drive and need a brake on vertical axes. A larger lead is faster per turn and more efficient but needs more torque and loses self-locking.",
    limits: "Constant friction coefficient; no backlash, screw whip, end bearings or nut compliance; the screw and nut are massless here.",
    parameters: &[("lead", "Nut travel per screw turn (pitch × starts)."), ("pitch_diameter", "Thread pitch diameter d."), ("pressure_angle", "Thread flank half-angle φₙ (Acme/trapezoidal 14.5–15°, ball screw ~45° contact)."), ("friction", "Sliding friction μ (bronze nut 0.1–0.2, plastic 0.08–0.15, ball screw ≈ 0.005)."), ("smoothing_speed", "Speed ε below which friction is smoothed (creep when locked)."), ("correction", "Constraint drift correction rate (numerical).")],
    realtime: &[("smoothing_speed", 0.05)],
    pairs_with: &[BRUSHED_MOTOR, "translational.mass", "translational.force_source", "translational.damper", "translational.position_sensor", sim_domain_rotational::elements::INERTIA],
    derived: Some(|p| sim_domain_rotational::helical::screw_contact(p).derived("nut", "m")),
    ..Notes::new("A screw and nut: rotation to linear travel, with thread friction.")
};

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use sim_core::connectors::{Electrical as E, Rotational as R, Thermal as H};
    for descriptor in [
        BehaviorDescriptor::new(THERMISTOR, "Temperature-dependent resistor", vec![acausal("p", E), acausal("n", E), acausal("heat", H)], thermistor).with_parameters(vec![P::required("resistance", "Ω").positive(), P::required("coefficient", "1/K"), P::required("reference", "K").positive()]),
        BehaviorDescriptor::new(BRUSHED_MOTOR, "Brushed DC motor", vec![acausal("p", E), acausal("n", E), acausal("shaft", R), acausal("case", R)], brushed_motor).with_parameters(vec![P::required("resistance", "Ω").positive(), P::optional("inductance", "H", 0.0).nonnegative(), P::required("torque_constant", "N·m/A"), P::required("back_emf_constant", "V·s/rad")]).with_notes(&BRUSHED_MOTOR_NOTES),
        BehaviorDescriptor::new(THERMOELASTIC_LAYER, "Thermoelastic beam layer", vec![acausal("bending", R), acausal("layer", H)], thermoelastic_layer).with_parameters(vec![P::optional("sign", "1", 1.0), P::required("height", "m"), P::required("thickness", "m").positive(), P::required("width", "m").positive(), P::required("youngs_modulus", "Pa").positive(), P::required("expansion", "1/K"), P::required("temperature", "K").positive()]),
        BehaviorDescriptor::new(MOTOR, "Motor behind one plug", vec![acausal("plug", ConnectorKind::MOTOR)], motor).with_parameters(vec![P::required("resistance", "Ω").positive(), P::optional("coefficient", "1/K", 0.0), P::optional("reference", "K", 293.15).positive(), P::required("torque_constant", "N·m/A")]),
        BehaviorDescriptor::new(DUAL_DRIVE, "Two-socket current drive", vec![acausal("a", ConnectorKind::MOTOR), acausal("b", ConnectorKind::MOTOR), signal_out("hottest", QuantityKind::Temperature)], dual_drive).with_parameters(vec![P::required("current", "A")]),
    ] {
        registry.register(descriptor)?;
    }
    let d = sim_domain_rotational::helical::SCREW_DEFAULTS;
    registry.register(
        BehaviorDescriptor::new(LEAD_SCREW, "Lead screw", vec![acausal("screw", R), acausal("nut", sim_core::connectors::Translational)], lead_screw)
            .with_parameters(vec![
                P::optional("lead", "m", d.lead).positive(),
                P::optional("pitch_diameter", "m", d.pitch_diameter).positive(),
                P::optional("pressure_angle", "rad", d.pressure_angle).positive().at_most(0.8),
                P::optional("friction", "1", d.friction).nonnegative().at_most(1.),
                P::optional("smoothing_speed", "rad/s", 0.01).positive(),
                P::optional("correction", "1/s", 10.0),
            ])
            .with_notes(&LEAD_SCREW_NOTES),
    )?;
    Ok(())
}
