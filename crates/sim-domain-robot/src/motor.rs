//! Actuation and power: `robot.motor_unit` (winding, rotor, gearbox with
//! efficiency, backlash and compliance, heat into a thermal node),
//! `robot.h_bridge` (averaged driver with on-resistance and a current
//! fold-back), `robot.battery` (open-circuit voltage against state of
//! charge, internal resistance), `robot.servo_firmware` (a sampled position
//! loop with latency, dead band, sensor quantisation and saturation) and
//! `robot.thermal_probe` (temperature of a thermal node as a signal).
//!
//! Sign conventions follow the rest of the library: a through value is what
//! the element absorbs at the port (current into `p`, torque into the
//! shaft, heat into the node); a source therefore adds negative through.

use sim_core::{
    acausal, param, param_or, signal_in, signal_out, Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, Input, LocalJacobian, Output, QuantityKind, RegistryError, StateDeclaration, View,
};
use std::collections::BTreeMap;

pub const MOTOR_UNIT: &str = "robot.motor_unit";
pub const H_BRIDGE: &str = "robot.h_bridge";
pub const BATTERY: &str = "robot.battery";
pub const SERVO_FIRMWARE: &str = "robot.servo_firmware";
pub const THERMAL_PROBE: &str = "robot.thermal_probe";

type Params = BTreeMap<String, f64>;

/// Explicit physical reductions for training-model experiments. Original CAD
/// inductance/inertia values remain in the parameter map; these flags record
/// which storage terms are omitted. None of these modes imply calibration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotorDynamics {
    #[default]
    Detailed,
    QuasistaticWinding,
    QuasistaticRotor,
    Quasistatic,
}
impl MotorDynamics {
    pub fn is_detailed(&self) -> bool { *self == Self::Detailed }
    pub fn parameter_flags(self) -> BTreeMap<&'static str, f64> {
        let mut flags = BTreeMap::new();
        if matches!(self, Self::QuasistaticWinding | Self::Quasistatic) {
            flags.insert("dynamics.quasistatic_winding", 1.0);
        }
        if matches!(self, Self::QuasistaticRotor | Self::Quasistatic) {
            flags.insert("dynamics.quasistatic_rotor", 1.0);
        }
        flags
    }
}

/// Existing CAD driver derivation shared by detailed and reduced adapters.
/// Retains the detailed runtime's parameter floors; this is not calibration.
pub fn cad_h_bridge_parameters(motor: &crate::model::Motor) -> BTreeMap<String, f64> {
    [
        ("on_resistance".into(), motor.driver.on_resistance.max(0.0)),
        ("current_limit".into(), motor.driver.current_limit.min(motor.electrical.current_limit).max(0.01)),
    ].into()
}

/// Preserve the detailed adapter's CAD firmware-to-duty parameter mapping.
/// Latency quantization and held-state behavior remain in ServoFirmware.
pub fn cad_servo_firmware_parameters(motor: &crate::model::Motor) -> BTreeMap<String, f64> {
    let fw = &motor.firmware;
    let scale = if fw.output == "current" {
        motor.driver.current_limit.min(motor.electrical.current_limit).max(0.01)
    } else {
        motor.electrical.supply_voltage.max(0.1)
    };
    [
        ("rate".into(), fw.loop_rate_hz.max(1.0)),
        ("latency".into(), fw.latency_s.max(0.0)),
        ("deadband".into(), fw.deadband_rad.max(0.0)),
        ("resolution".into(), fw.sensor_resolution_rad.max(0.0)),
        ("kp".into(), fw.kp / scale),
        ("ki".into(), fw.ki / scale),
        ("kd".into(), fw.kd / scale),
        ("limit".into(), 1.0),
    ].into()
}

/// Existing CAD-to-component parameter derivation, shared by detailed and
/// reduced runtime adapters. Floors, damping and friction estimates preserve
/// the detailed adapter's conventions; they are not new hardware calibration.
/// Record this map in experiment evidence so those derivations remain visible.
/// The connection argument is the resolved full rotational gap. v4 callers
/// obtain it from JointPhysics::drive_backlash_rad, never from radial clearance.
pub fn cad_motor_unit_parameters(
    motor: &crate::model::Motor,
    drive_connection_backlash_rad: f64,
    ambient_k: f64,
    analytic_jacobian: bool,
    backlash_events: bool,
) -> BTreeMap<&'static str, f64> {
    let e = &motor.electrical;
    let gb = &motor.gearbox;
    let th = &motor.thermal;
    [
        ("jacobian.analytic", if analytic_jacobian { 1.0 } else { 0.0 }),
        ("backlash.events", if backlash_events { 1.0 } else { 0.0 }),
        ("resistance", e.resistance.max(1e-3)),
        ("inductance", e.inductance.max(0.0)),
        ("torque_constant", e.torque_constant.max(1e-6)),
        ("back_emf_constant", if e.back_emf_constant > 0.0 { e.back_emf_constant } else { e.torque_constant }),
        ("no_load_current", e.no_load_current),
        ("rotor_inertia", e.rotor_inertia.max(1e-9)),
        ("ratio", gb.ratio.max(1e-6) * motor.gear_ratio.max(1e-6)),
        ("efficiency", gb.efficiency.clamp(0.05, 1.0)),
        ("backlash", gb.backlash_rad + drive_connection_backlash_rad),
        ("gear_stiffness", gb.stiffness.max(1.0)),
        ("gear_damping", 0.002 * gb.stiffness.max(1.0)),
        ("gear_inertia", gb.inertia.max(0.0)),
        ("gear_friction", if gb.max_output_torque.is_finite() { 0.05 * gb.max_output_torque } else { 0.0 }),
        ("temp_coeff", th.resistance_temp_coeff),
        ("derating", th.torque_derating_per_c),
        ("reference", ambient_k),
    ].into_iter().collect()
}

fn dead_zone(x: f64, half: f64) -> f64 {
    if half <= 0.0 {
        x
    } else if x > half {
        x - half
    } else if x < -half {
        x + half
    } else {
        0.0
    }
}

// ---- motor unit -------------------------------------------------------------

/// Ports: `p`, `n` (electrical), `shaft` (rotational, gearbox output),
/// `winding` (thermal); signals out `current`, `torque`, `speed`.
///
/// Parameters: `resistance`, `inductance`, `torque_constant`,
/// `back_emf_constant`, `no_load_current`, `rotor_inertia` (rotor side),
/// `ratio`, `efficiency`, `backlash` (rad, output side), `gear_stiffness`
/// (N·m/rad), `gear_damping`, `gear_inertia` (output side), `gear_friction`
/// (Coulomb, N·m at the output), `temp_coeff`
/// (1/°C on resistance), `derating` (1/°C on torque constant),
/// `reference` (K, default 293.15), `initial.angle` (shaft angle the gear starts aligned to).
/// Temperatures are in kelvin like the thermal domain.
/// `backlash.events=1` is an experimental integration option: a held mode
/// selects the free/positive/negative branch and guards locate engagement and
/// release. It adds one dimensionless state only when backlash is nonzero.
/// Smooth trial continuations may leave the mode's domain during root finding;
/// the integrator splits accepted advancement at the guard. No state/velocity
/// impulse is applied. Event samples use the newly selected one-sided branch.
pub struct MotorUnit {
    analytic_jacobian: bool,
    event_backlash: bool,
    quasistatic_winding: bool,
    quasistatic_rotor: bool,
    resistance: f64,
    inductance: f64,
    kt: f64,
    ke: f64,
    no_load_current: f64,
    rotor_inertia: f64,
    ratio: f64,
    efficiency: f64,
    backlash: f64,
    gear_k: f64,
    gear_c: f64,
    gear_inertia: f64,
    gear_friction: f64,
    temp_coeff: f64,
    derating: f64,
    reference_c: f64,
    initial_angle: f64,
}

impl MotorUnit {
    // States: current, rotor speed (rad/s), gear output angle (rad).
    const I: usize = 0;
    const W: usize = 1;
    const TH: usize = 2;
    const MODE: usize = 3;

    fn mode(&self, state: f64, deflection: f64) -> f64 {
        if state > 1.5 { // Startup classification before the initialization event.
            if deflection.abs() > 0.5*self.backlash { deflection.signum() } else { 0.0 }
        } else if state.abs()>0.5 { state.signum() } else { 0.0 }
    }

    fn coupling_in_mode(&self, deflection:f64, velocity:f64, mode:f64) -> f64 {
        // Smooth continuation of the current branch during an event-location
        // trial. Accepted steps are split at the guard before changing mode.
        if mode == 0.0 { self.gear_c*0.05*velocity }
        else { self.gear_k*(deflection-mode*0.5*self.backlash)+self.gear_c*velocity }
    }

    fn coupling(&self, theta_g: f64, omega_g: f64, theta_s: f64, omega_s: f64) -> f64 {
        let gap = dead_zone(theta_g - theta_s, 0.5 * self.backlash);
        let engaged = if self.backlash > 0.0 { if gap != 0.0 { 1.0 } else { 0.05 } } else { 1.0 };
        self.gear_k * gap + self.gear_c * engaged * (omega_g - omega_s)
    }
}

impl Behavior for MotorUnit {
    fn states(&self) -> Vec<StateDeclaration> {
        let mut states = vec![
            StateDeclaration::new("current", QuantityKind::Current, 0.0),
            StateDeclaration::new("rotor_speed", QuantityKind::AngularVelocity, 0.0),
            StateDeclaration::new("gear_angle", QuantityKind::Angle, self.initial_angle),
        ];
        if self.event_backlash {
            states.push(StateDeclaration::new("backlash_mode",QuantityKind::Dimensionless,2.0));
        }
        states
    }
    fn residual(&self, ctx: &mut Context) {
        let i = ctx.state(Self::I);
        let w_r = ctx.state(Self::W);
        let th_g = ctx.state(Self::TH);
        let v = ctx.across(0) - ctx.across(1);
        let temp = ctx.across(3);
        let r = self.resistance * (1.0 + self.temp_coeff * (temp - self.reference_c));
        let kt = (self.kt * (1.0 - self.derating * (temp - self.reference_c))).max(0.3 * self.kt);
        // Winding.
        let emf = self.ke * w_r;
        if self.inductance > 0.0 && !self.quasistatic_winding {
            ctx.set_state_residual(Self::I, self.inductance * ctx.state_rate(Self::I) - (v - r * i - emf));
        } else {
            ctx.set_state_residual(Self::I, r * i + emf - v);
        }
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
        // Rotor and gearbox, referred to the output side.
        let loss = self.no_load_current * kt * (w_r / 5.0).tanh();
        let tau_m = kt * i - loss;
        // Driving unless the load clearly back-drives the gear train.
        let power = tau_m * w_r;
        let s = 0.5 * (1.0 + ((power + 5.0e-3) / 1.0e-3).tanh());
        let eta = s * self.efficiency + (1.0 - s) / self.efficiency.max(1e-3);
        let th_s = ctx.across(2);
        let w_s = ctx.across_derivative(2, 0);
        let w_g = w_r / self.ratio;
        let tau_c = if self.event_backlash {
            ctx.set_state_residual(Self::MODE,ctx.state_rate(Self::MODE));
            self.coupling_in_mode(th_g-th_s,w_g-w_s,self.mode(ctx.state(Self::MODE),th_g-th_s))
        } else { self.coupling(th_g, w_g, th_s, w_s) };
        let j_out = self.rotor_inertia * self.ratio * self.ratio + self.gear_inertia;
        // Do not read the rate in the quasistatic branch: the shared compiler
        // must identify the rotor equation as algebraic, not differential.
        let inertia_torque = if self.quasistatic_rotor { 0.0 }
            else { j_out * (ctx.state_rate(Self::W) / self.ratio) };
        // Coulomb friction of the gear train at its output (what makes a
        // servo hold without buzzing and resist back-driving).
        let gear_friction = self.gear_friction * (w_g / 0.05).tanh();
        ctx.set_state_residual(Self::W, inertia_torque - self.ratio * eta * tau_m + tau_c + gear_friction);
        ctx.set_state_residual(Self::TH, ctx.state_rate(Self::TH) - w_g);
        ctx.add_through(2, -tau_c);
        // Heat: copper loss plus gear loss.
        let gear_loss = ((1.0 - self.efficiency) * (self.ratio * tau_m * w_g).abs()).max(0.0) + (gear_friction * w_g).abs();
        ctx.add_through(3, -(r * i * i + gear_loss));
        ctx.set_signal(0, i);
        ctx.set_signal(1, tau_c);
        ctx.set_signal(2, w_g);
    }
    fn guards(&self, view:&View, out:&mut Vec<f64>) {
        if !self.event_backlash { return; }
        let delta=view.state(Self::TH)-view.across(2);
        let mode=self.mode(view.state(Self::MODE),delta);
        let half=0.5*self.backlash;
        out.push(if mode<0.0 {1.0} else if mode>0.0 {delta-half} else {half-delta});
        out.push(if mode>0.0 {1.0} else if mode<0.0 {-delta-half} else {half+delta});
        out.push(if view.state(Self::MODE)>1.5 {0.0} else {1.0});
    }
    fn scheduled_events(&self, view:&View, out:&mut Vec<(usize,f64)>) {
        if self.event_backlash && view.state(Self::MODE)>1.5 { out.push((2,0.0)); }
    }
    fn jump(&mut self, index:usize, view:&View, states:&mut [f64]) {
        if !self.event_backlash { return; }
        let mode=self.mode(view.state(Self::MODE),view.state(Self::TH)-view.across(2));
        states[Self::MODE]=match index {
            2=>mode,
            0 if mode==0.0=>1.0,
            1 if mode==0.0=>-1.0,
            _=>0.0,
        };
    }
    /// Branch-local derivatives of the unchanged residual. Backlash engagement
    /// and the temperature clamp, as well as absolute-value heat losses at zero
    /// power, are nonsmooth: no classical derivative is claimed at a switching
    /// surface. Experimental until trajectory promotion.
    fn jacobian(&self, view: &View, out: &mut LocalJacobian) -> bool {
        if !self.analytic_jacobian { return false; }
        let i = view.state(Self::I);
        let wr = view.state(Self::W);
        let temp = view.across(3);
        let r = self.resistance * (1.0 + self.temp_coeff * (temp-self.reference_c));
        let dr = self.resistance * self.temp_coeff;
        let raw_kt = self.kt * (1.0-self.derating*(temp-self.reference_c));
        let kt = raw_kt.max(0.3*self.kt);
        let dkt = if raw_kt > 0.3*self.kt { -self.kt*self.derating } else { 0.0 };
        let loss_tanh = (wr/5.0).tanh();
        let tau = kt*i - self.no_load_current*kt*loss_tanh;
        let power = tau*wr;
        let t = ((power+5e-3)/1e-3).tanh();
        let blend = 0.5*(1.0+t);
        let inverse_efficiency = 1.0/self.efficiency.max(1e-3);
        let eta = blend*self.efficiency + (1.0-blend)*inverse_efficiency;
        let deta_dp = (self.efficiency-inverse_efficiency)*500.0*(1.0-t*t);
        let wg = wr/self.ratio;
        let ft = (wg/0.05).tanh();
        let friction = self.gear_friction*ft;
        let dfriction = self.gear_friction*(1.0-ft*ft)/(0.05*self.ratio);
        let engaged = if self.event_backlash {
            out.state_rate(Self::MODE,Self::MODE,1.0);
            self.mode(view.state(Self::MODE),view.state(Self::TH)-view.across(2))!=0.0
        } else { self.backlash <= 0.0 || (view.state(Self::TH)-view.across(2)).abs()>0.5*self.backlash };
        let stiffness = if engaged { self.gear_k } else { 0.0 };
        let damping = self.gear_c * if engaged { 1.0 } else { 0.05 };
        // Electrical and kinematic rows, independent of engagement.
        out.state_state(Self::I,Self::I,r);
        out.state_state(Self::I,Self::W,self.ke);
        if !self.quasistatic_winding { out.state_rate(Self::I,Self::I,self.inductance.max(0.0)); }
        out.set(Output::State(Self::I),Input::Across(0,0),-1.0);
        out.set(Output::State(Self::I),Input::Across(1,0),1.0);
        out.set(Output::State(Self::I),Input::Across(3,0),dr*i);
        if !self.quasistatic_rotor { out.state_rate(Self::W,Self::W,(self.rotor_inertia*self.ratio*self.ratio+self.gear_inertia)/self.ratio); }
        out.state_rate(Self::TH,Self::TH,1.0);
        out.state_state(Self::TH,Self::W,-1.0/self.ratio);
        out.through(0,Input::State(Self::I),1.0);
        out.through(1,Input::State(Self::I),-1.0);
        out.set(Output::Signal(0),Input::State(Self::I),1.0);
        out.set(Output::Signal(2),Input::State(Self::W),1.0/self.ratio);
        for (input,dtau,dwr,dresistance,di) in [
            (Input::State(Self::I),kt,0.0,0.0,1.0),
            (Input::State(Self::W),-self.no_load_current*kt*(1.0-loss_tanh*loss_tanh)/5.0,1.0,0.0,0.0),
            (Input::Across(3,0),dkt*(i-self.no_load_current*loss_tanh),0.0,dr,0.0),
        ] {
            let dp = dtau*wr+tau*dwr;
            out.set(Output::State(Self::W),input,-self.ratio*(eta*dtau+tau*deta_dp*dp)+dfriction*dwr);
            let dheat = dresistance*i*i+2.0*r*i*di
                +(1.0-self.efficiency)*power.signum()*dp
                +(friction*wg).signum()*(dfriction*wg+friction/self.ratio)*dwr;
            out.through(3,input,-dheat);
        }
        for (input,value) in [
            (Input::State(Self::TH),stiffness),
            (Input::Across(2,0),-stiffness),
            (Input::State(Self::W),damping/self.ratio),
            (Input::AcrossDerivative(2,0),-damping),
        ] {
            out.set(Output::State(Self::W),input,value);
            out.through(2,input,-value);
            out.set(Output::Signal(1),input,value);
        }
        true
    }
    fn energy(&self, view: &View) -> f64 {
        let w = view.state(Self::W);
        let kinetic = if self.quasistatic_rotor { 0.0 } else { 0.5 * self.rotor_inertia * w * w };
        let magnetic = if self.quasistatic_winding { 0.0 } else { 0.5 * self.inductance * view.state(Self::I).powi(2) };
        kinetic + magnetic
    }
}

fn motor_unit(p: &Params) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let kt = param(p, "torque_constant")?;
    let backlash=param_or(p,"backlash",0.0);
    Ok(Box::new(MotorUnit {
        analytic_jacobian: param_or(p, "jacobian.analytic", 0.0)>0.5,
        event_backlash: param_or(p,"backlash.events",0.0)>0.5 && backlash>0.0,
        quasistatic_winding: param_or(p,"dynamics.quasistatic_winding",0.0)>0.5,
        quasistatic_rotor: param_or(p,"dynamics.quasistatic_rotor",0.0)>0.5,
        resistance: param(p, "resistance")?,
        inductance: param_or(p, "inductance", 0.0),
        kt,
        ke: param_or(p, "back_emf_constant", kt),
        no_load_current: param_or(p, "no_load_current", 0.0),
        rotor_inertia: param_or(p, "rotor_inertia", 1e-7),
        ratio: param_or(p, "ratio", 1.0).max(1e-6),
        efficiency: param_or(p, "efficiency", 0.8).clamp(0.05, 1.0),
        backlash,
        gear_k: param_or(p, "gear_stiffness", 200.0),
        gear_c: param_or(p, "gear_damping", 0.01),
        gear_inertia: param_or(p, "gear_inertia", 0.0),
        gear_friction: param_or(p, "gear_friction", 0.0),
        temp_coeff: param_or(p, "temp_coeff", 0.0039),
        derating: param_or(p, "derating", 0.001),
        reference_c: param_or(p, "reference", 293.15),
        initial_angle: param_or(p, "initial.angle", 0.0),
    }))
}

// ---- H-bridge ---------------------------------------------------------------

/// Ports: `supply_p`, `supply_n`, `p`, `n`; signal in `command` (−1…1).
/// Parameters: `on_resistance`, `current_limit` (fold-back above it).
pub struct HBridge {
    on_resistance: f64,
    current_limit: f64,
}

impl Behavior for HBridge {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("current", QuantityKind::Current, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let duty = ctx.signal_in(0).clamp(-1.0, 1.0);
        let vs = ctx.across(0) - ctx.across(1);
        // `i` is absorbed at `p`; the load draws `−i`.
        let i = ctx.state(0);
        let out = -i;
        let over = (out.abs() - self.current_limit).max(0.0);
        // Current limiting folds the output voltage back toward zero (never past it).
        let open = duty * vs;
        let raw = open + self.on_resistance * i - 20.0 * over * out.signum();
        let v_out = if open >= 0.0 { raw.clamp(0.0, open.max(0.0)) } else { raw.clamp(open, 0.0) };
        ctx.set_state_residual(0, ctx.across(2) - ctx.across(3) - v_out);
        ctx.add_through(2, i);
        ctx.add_through(3, -i);
        ctx.add_through(0, -duty * i);
        ctx.add_through(1, duty * i);
    }
}

fn h_bridge(p: &Params) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    Ok(Box::new(HBridge { on_resistance: param_or(p, "on_resistance", 0.1), current_limit: param_or(p, "current_limit", f64::INFINITY) }))
}

// ---- battery ----------------------------------------------------------------

/// Ports `p`, `n`; signal out `soc`. Parameters: `cells`, `nominal_voltage`
/// (pack), `internal_resistance`, `capacity_ah`, `initial_soc`.
pub struct BatteryPack {
    nominal: f64,
    resistance: f64,
    capacity_ah: f64,
    initial_soc: f64,
}

impl BatteryPack {
    fn emf(&self, soc: f64) -> f64 {
        // 0.9–1.1 × nominal across the discharge curve with a knee at the end.
        let s = soc.clamp(0.0, 1.0);
        self.nominal * (0.9 + 0.2 * s - 0.15 * (1.0 - s).powi(8))
    }
}

impl Behavior for BatteryPack {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("current", QuantityKind::Current, 0.0), StateDeclaration::new("soc", QuantityKind::Dimensionless, self.initial_soc)]
    }
    fn residual(&self, ctx: &mut Context) {
        let i = ctx.state(0);
        let soc = ctx.state(1);
        ctx.set_state_residual(0, ctx.across(0) - ctx.across(1) - (self.emf(soc) + self.resistance * i));
        ctx.set_state_residual(1, ctx.state_rate(1) - i / (3600.0 * self.capacity_ah.max(1e-6)));
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
        ctx.set_signal(0, soc);
    }
}

fn battery(p: &Params) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let cells = param_or(p, "cells", 2.0);
    Ok(Box::new(BatteryPack {
        nominal: param_or(p, "nominal_voltage", 3.7 * cells),
        resistance: param_or(p, "internal_resistance", 0.05),
        capacity_ah: param_or(p, "capacity_ah", 1.0),
        initial_soc: param_or(p, "initial_soc", 1.0),
    }))
}

// ---- servo firmware ---------------------------------------------------------

/// Signals in `target`, `measured`, `rate` (measured speed, for the
/// derivative term); signal out `command`. Parameters: `rate` (Hz),
/// `latency` (s, rounded to samples), `deadband`, `resolution` (sensor
/// quantum), `kp`, `ki`, `kd` (on the measured speed), `limit` (|command|
/// cap), `offset` (first sample time).
pub struct ServoFirmware {
    period: f64,
    delay: usize,
    deadband: f64,
    resolution: f64,
    kp: f64,
    ki: f64,
    kd: f64,
    limit: f64,
    offset: f64,
}

impl ServoFirmware {
    // States: held command, integrator, previous error, queue[delay], clock.
    const HELD: usize = 0;
    const INTEG: usize = 1;
    const PREV: usize = 2;
    const DFILT: usize = 3;
    fn queue(&self, k: usize) -> usize {
        4 + k
    }
    fn clock(&self) -> usize {
        4 + self.delay
    }
}

impl Behavior for ServoFirmware {
    fn states(&self) -> Vec<StateDeclaration> {
        let d = QuantityKind::Dimensionless;
        let mut out = vec![StateDeclaration::new("command", d, 0.0), StateDeclaration::new("integrator", d, 0.0), StateDeclaration::new("previous_error", d, 0.0), StateDeclaration::new("derivative", d, 0.0)];
        for k in 0..self.delay {
            out.push(StateDeclaration::new(format!("queue{k}"), d, 0.0));
        }
        out.push(StateDeclaration::new("next_sample", QuantityKind::Time, self.offset));
        out
    }
    fn residual(&self, ctx: &mut Context) {
        for k in 0..=self.clock() {
            ctx.set_state_residual(k, ctx.state_rate(k));
        }
        ctx.set_signal(0, ctx.state(Self::HELD));
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(view.state(self.clock()) - view.time);
    }
    fn scheduled_events(&self, view: &View, out: &mut Vec<(usize, f64)>) {
        out.push((0, view.state(self.clock())));
    }
    fn jump(&mut self, _index: usize, view: &View, states: &mut [f64]) {
        let target = view.signal_in(0);
        let mut measured = view.signal_in(1);
        if self.resolution > 0.0 {
            measured = (measured / self.resolution).round() * self.resolution;
        }
        let mut error = target - measured;
        if error.abs() < self.deadband {
            error = 0.0;
        }
        // The derivative acts on the measured speed (a differenced quantised
        // angle would be noise), lightly filtered like a firmware would.
        states[Self::PREV] = error;
        let derivative = states[Self::DFILT] + 0.5 * (-view.signal_in(2) - states[Self::DFILT]);
        states[Self::DFILT] = derivative;
        let mut integ = states[Self::INTEG] + error * self.period;
        let raw = self.kp * error + self.ki * integ + self.kd * derivative;
        let command = raw.clamp(-self.limit, self.limit);
        if (raw - command).abs() > 0.0 && self.ki > 0.0 {
            // Anti-windup: hold the integrator while saturated.
            integ = states[Self::INTEG];
        }
        states[Self::INTEG] = integ;
        let applied = if self.delay == 0 {
            command
        } else {
            let oldest = states[self.queue(0)];
            for k in 0..self.delay - 1 {
                states[self.queue(k)] = states[self.queue(k + 1)];
            }
            states[self.queue(self.delay - 1)] = command;
            oldest
        };
        states[Self::HELD] = applied;
        let clock = self.clock();
        // Anchor deadlines to the declared phase instead of accumulating one
        // rounded period per tick. Accumulated drift can split a coincident
        // physics/firmware boundary into an unresolvably small physical step.
        // Derive the tick from transactional state, not a mutable counter, so
        // rejected intervals and restored checkpoints retain their schedule.
        let tick = ((states[clock] - self.offset) / self.period).round();
        states[clock] = self.offset + (tick + 1.0) * self.period;
    }
}

fn servo_firmware(p: &Params) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let rate = param_or(p, "rate", 50.0).max(1.0);
    let period = 1.0 / rate;
    Ok(Box::new(ServoFirmware {
        period,
        delay: (param_or(p, "latency", 0.0) * rate).round().max(0.0) as usize,
        deadband: param_or(p, "deadband", 0.0),
        resolution: param_or(p, "resolution", 0.0),
        kp: param_or(p, "kp", 20.0),
        ki: param_or(p, "ki", 0.0),
        kd: param_or(p, "kd", 0.0),
        limit: param_or(p, "limit", 1.0),
        offset: param_or(p, "offset", period),
    }))
}

// ---- thermal probe ----------------------------------------------------------

pub struct ThermalProbe;
impl Behavior for ThermalProbe {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_signal(0, ctx.across(0));
    }
}

fn thermal_probe(_p: &Params) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    Ok(Box::new(ThermalProbe))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::{Electrical as E, Rotational as R, Thermal as H};
    use QuantityKind as Q;
    let inherited_default = |name: &str, unit: &str, label: &str| {
        let mut p = P::alternative(name, unit);
        p.default_label = Some(label.into()); p
    };
    let mut efficiency = P::optional("efficiency", "1", 0.8).at_most(1.);
    efficiency.minimum = Some(0.05);
    let mut rate = P::optional("rate", "Hz", 50.);
    rate.minimum = Some(1.);
    let mut cells = P::optional("cells", "1", 2.).positive();
    cells.integer = true;
    registry.register(BehaviorDescriptor::new(
        MOTOR_UNIT,
        "Motor with gearbox and winding heat",
        vec![acausal("p", E), acausal("n", E), acausal("shaft", R), acausal("winding", H), signal_out("current", Q::Current), signal_out("torque", Q::Torque), signal_out("speed", Q::AngularVelocity)],
        motor_unit,
    ).with_parameters(vec![
        P::optional("jacobian.analytic", "1", 0.).integer(0.,1.),
        P::optional("backlash.events", "1", 0.).integer(0.,1.),
        P::optional("dynamics.quasistatic_winding", "1", 0.).integer(0.,1.),
        P::optional("dynamics.quasistatic_rotor", "1", 0.).integer(0.,1.),
        P::required("resistance", "Ω").positive(),
        P::required("torque_constant", "N·m/A").positive(),
        P::optional("inductance", "H", 0.).nonnegative(),
        inherited_default("back_emf_constant", "V·s/rad", "torque_constant").nonnegative(),
        P::optional("no_load_current", "A", 0.).nonnegative(),
        P::optional("rotor_inertia", "kg·m²", 1e-7).positive(),
        P::optional("ratio", "1", 1.).positive(), efficiency,
        P::optional("backlash", "rad", 0.).nonnegative(),
        P::optional("gear_stiffness", "N·m/rad", 200.).positive(),
        P::optional("gear_damping", "N·m·s/rad", 0.01).nonnegative(),
        P::optional("gear_inertia", "kg·m²", 0.).nonnegative(),
        P::optional("gear_friction", "N·m", 0.).nonnegative(),
        P::optional("temp_coeff", "1/K", 0.0039),
        P::optional("derating", "1/K", 0.001),
        P::optional("reference", "K", 293.15).nonnegative(),
        P::optional("initial.angle", "rad", 0.),
    ]))?;
    registry.register(BehaviorDescriptor::new(
        H_BRIDGE,
        "Averaged H-bridge driver",
        vec![acausal("supply_p", E), acausal("supply_n", E), acausal("p", E), acausal("n", E), signal_in("command", Q::Dimensionless)],
        h_bridge,
    ).with_parameters(vec![P::optional("on_resistance", "Ω", 0.1).nonnegative(),
        P::optional("current_limit", "A", f64::INFINITY).positive()]))?;
    registry.register(BehaviorDescriptor::new(BATTERY, "Battery pack", vec![acausal("p", E), acausal("n", E), signal_out("soc", Q::Dimensionless)], battery)
        .with_parameters(vec![cells, inherited_default("nominal_voltage", "V", "3.7 × cells").positive(),
            P::optional("internal_resistance", "Ω", 0.05).nonnegative(),
            P::optional("capacity_ah", "A·h", 1.).positive(),
            P::optional("initial_soc", "1", 1.).nonnegative().at_most(1.)]))?;
    registry.register(BehaviorDescriptor::new(
        SERVO_FIRMWARE,
        "Sampled servo position loop",
        vec![signal_in("target", Q::Angle), signal_in("measured", Q::Angle), signal_in("rate", Q::AngularVelocity), signal_out("command", Q::Dimensionless)],
        servo_firmware,
    ).with_parameters(vec![rate, P::optional("latency", "s", 0.).nonnegative(),
        P::optional("deadband", "rad", 0.).nonnegative(), P::optional("resolution", "rad", 0.).nonnegative(),
        P::optional("kp", "1/rad", 20.), P::optional("ki", "1/(rad·s)", 0.),
        P::optional("kd", "s/rad", 0.), P::optional("limit", "1", 1.).nonnegative().at_most(1.),
        inherited_default("offset", "s", "1 / rate").nonnegative()]))?;
    registry.register(BehaviorDescriptor::new(THERMAL_PROBE, "Temperature as a signal", vec![acausal("node", H), signal_out("temperature", Q::Temperature)], thermal_probe).with_parameters(vec![]))
}
