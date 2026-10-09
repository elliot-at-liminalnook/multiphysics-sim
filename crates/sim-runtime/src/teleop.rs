//! Teleoperation as blocks (docs/cad-migration-and-composition-plan.md,
//! B3 and B4): a person drives a composed system through ordinary host
//! blocks on the scheduler, never by writing into the plant.
//!
//! - **Drive input** ([`DRIVE_HOST`]): a host block a system file holds,
//!   named `drive_input:<profile>` after the robot's `sim.drive/1` profile
//!   (relative to the system file). Its outputs are the body twist
//!   ([`TWIST_OUTPUTS`]: `vx`, `vy` in m/s, `wz` in rad/s), limited and
//!   watched by the profile's deadman on simulation time exactly as the
//!   drive host does (`crate::drive_host::TwistState`); the system wires
//!   them to whatever mixes a twist (a controller FMU's command inputs).
//!   The host that runs the system binds it to its devices through a
//!   [`DriveLink`] ([`drive_hosts`]); a host without one refuses it by name.
//! - **Jog** ([`JOG_HOST`]): Robot mode runs a robot project's system
//!   ([`compose_robot`]) and lets a person move its joints. Each joint is
//!   reached through the robot's open `<joint>.target` input, or, when a
//!   controller block already drives the target, through that controller's
//!   free **setpoint** input (a block input declared `setpoint`; an FMU's
//!   `sim.setpoint` annotation). A joint with neither is refused by name
//!   ([`JogPlan::refused`]).
use crate::drive_host::{DriveRequest, DriveStatus, TwistState};
use crate::system_blocks::Hosts;
use serde::Serialize;
use sim_core::{BlockImplementation, BlockInterface, BlockPort, BlockTiming, Checkpoint, ImplementationRef, ModelWorld, PortSchema, QuantityKind};
use sim_domain_control::drive::kinematics::{self, Axes, BodyTwist, Deadman, Limits};
use sim_domain_control::drive::profile::{ActionRequest, DriveProfile};
use sim_system::{BlockSource, InstanceKind, InstanceSpec, SystemDocument, Terminal};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
mod compose;
#[cfg(not(target_arch = "wasm32"))]
pub use compose::{ComposedRobot, compose_robot, plan_for};

/// The host implementation name prefix of a drive input block.
pub const DRIVE_HOST: &str = "drive_input";
/// A drive input block's outputs, in order.
pub const TWIST_OUTPUTS: [&str; 3] = ["vx", "vy", "wz"];
/// The host implementation name of Robot mode's jog block.
pub const JOG_HOST: &str = "robot_jog";

/// The host name of a drive input block driven through `profile`.
pub fn drive_host_name(profile: &str) -> String {
    format!("{DRIVE_HOST}:{profile}")
}

/// The profile a drive input host name carries (None: not a drive input).
pub fn drive_profile_path(host: &str) -> Option<&str> {
    host.strip_prefix(DRIVE_HOST)?.strip_prefix(':').filter(|p| !p.is_empty())
}

/// A drive input block instance for the profile at `profile` (relative to
/// the system file's directory `base`), ticking every `period` seconds. The
/// same instance Build mode's actions and REST `system_add_drive_input` add.
pub fn drive_instance(base: &Path, profile: &str, period: f64) -> Result<InstanceSpec, String> {
    sim_system::check_relative_path(profile)?;
    let (loaded, _) = DriveProfile::load(&base.join(profile)).map_err(|e| e.to_string())?;
    let limits = loaded.limits();
    let kinds = [QuantityKind::LinearVelocity, QuantityKind::LinearVelocity, QuantityKind::AngularVelocity];
    let outputs = TWIST_OUTPUTS
        .iter()
        .zip(kinds)
        .enumerate()
        .map(|(k, (name, kind))| BlockPort::new(*name, kind).start(0.0).range(Some(-limits.max_speed[k]), Some(limits.max_speed[k])))
        .collect();
    let timing = BlockTiming::periodic(period);
    timing.validate()?;
    let mut spec = InstanceSpec::block(BlockSource::Host { name: drive_host_name(profile) }, BlockInterface { inputs: Vec::new(), outputs, feedthrough: true }, timing);
    spec.label = "Drive input".into();
    Ok(spec)
}

/// What the host's devices send a bound drive input block, and what it last
/// did. Shared between the host (the window, REST) and the block on the run
/// thread; it outlives a reset (each build binds a fresh block to it).
#[derive(Clone, Default)]
pub struct DriveLink(Arc<Mutex<LinkState>>);

#[derive(Default)]
struct LinkState {
    pending: Vec<Pending>,
    status: Option<DriveStatus>,
    /// The bound block's profile (None: nothing bound, so nothing drives).
    profile: Option<(String, DriveProfile)>,
}

enum Pending {
    Twist { twist: BodyTwist, halt: bool },
    Pause,
}

impl DriveLink {
    fn state(&self) -> std::sync::MutexGuard<'_, LinkState> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
    /// The profile path of the bound block, if one is bound.
    pub fn bound(&self) -> Option<String> {
        self.state().profile.as_ref().map(|(path, _)| path.clone())
    }
    /// The bound profile's driveable axes (forward, lateral, yaw), if one is bound.
    pub fn supported(&self) -> Option<[bool; 3]> {
        self.state().profile.as_ref().map(|(_, p)| p.limits().supported)
    }
    /// A device's or REST's request, interpreted against the bound block's
    /// profile (axes scaled by its max speeds; a named action; stop) and
    /// applied at the block's next tick. Refused when nothing is bound or
    /// the request does not fit the profile.
    pub fn drive(&self, request: &DriveRequest) -> Result<(), String> {
        let mut state = self.state();
        let Some((path, profile)) = &state.profile else {
            return Err("nothing to drive: this run has no drive input block (add one with system_add_drive_input)".into());
        };
        let limits = profile.limits();
        let (twist, halt) = match request {
            DriveRequest::Axes { forward, lateral, yaw } => (
                kinematics::scale(Axes { forward: *forward, lateral: *lateral, yaw: *yaw }, &limits).map_err(|e| format!("drive request refused: {e} (profile {path})"))?,
                false,
            ),
            DriveRequest::Action { name } => match profile.action(name)?.request {
                ActionRequest::Stop => (BodyTwist::ZERO, false),
                ActionRequest::Halt => (BodyTwist::ZERO, true),
            },
            DriveRequest::Stop => (BodyTwist::ZERO, false),
        };
        state.pending.push(Pending::Twist { twist, halt });
        Ok(())
    }
    /// The run paused: a live request must not drive again on resume
    /// (`crate::drive_host::PAUSE_RULE`).
    pub fn pause(&self) {
        self.state().pending.push(Pending::Pause);
    }
    /// What the block last commanded.
    pub fn status(&self) -> Option<DriveStatus> {
        self.state().status
    }
}

/// The drive input block's implementation (one per build).
struct DriveInputBlock {
    link: DriveLink,
    profile: String,
    limits: Limits,
    deadman: Deadman,
    state: TwistState,
    interface: BlockInterface,
}

impl DriveInputBlock {
    fn write(&mut self, t: f64, dt: f64, outputs: &mut [f64]) -> Result<(), String> {
        let pending = std::mem::take(&mut self.link.state().pending);
        for p in pending {
            match p {
                Pending::Twist { twist, halt } => self.state.request(twist, halt, t, &self.limits)?,
                Pending::Pause => self.state.pause(t, &self.deadman),
            }
        }
        let sent = self.state.advance(t, dt, &self.limits, &self.deadman)?;
        outputs[..3].copy_from_slice(&sent[..3]);
        self.link.state().status = Some(self.state.status(t));
        Ok(())
    }
}

impl BlockImplementation for DriveInputBlock {
    fn label(&self) -> String {
        format!("the drive input ({})", self.profile)
    }
    fn interface(&self) -> BlockInterface {
        self.interface.clone()
    }
    fn initialize(&mut self, t: f64, period: f64, _inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.state = TwistState { last_request_s: t, ..TwistState::default() };
        self.write(t, period, outputs)
    }
    fn step(&mut self, t: f64, dt: f64, _inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.write(t, dt, outputs)
    }
    fn terminate(&mut self) {
        let mut state = self.link.state();
        state.profile = None;
        state.pending.clear();
    }
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Unsupported(format!("{} follows live input, which a checkpoint cannot replay", self.label()))
    }
}

/// Hosts that bind every drive input block of `model` to `link` (the
/// profile read from `base`, the system file's directory). Refused when
/// the model holds more than one: one person drives one robot.
pub fn drive_hosts<'a>(model: &ModelWorld, base: Option<&Path>, link: &DriveLink) -> Result<Hosts<'a>, String> {
    let mut hosts = Hosts::new();
    let drives: Vec<(String, String)> = model
        .blocks
        .iter()
        .filter_map(|b| match &b.implementation {
            ImplementationRef::Host { name } => drive_profile_path(name).map(|p| (name.clone(), p.to_owned())),
            _ => None,
        })
        .collect();
    if drives.len() > 1 {
        return Err(format!("the system has {} drive input blocks; one drives a run", drives.len()));
    }
    for (name, relative) in drives {
        let base = base.ok_or_else(|| format!("drive input `{name}` names its profile relative to the system file, but this model came from no file"))?;
        let path = base.join(&relative);
        let (profile, _) = DriveProfile::load(&path).map_err(|e| e.to_string())?;
        let link = link.clone();
        hosts.insert(
            name,
            Box::new(move |decl: &sim_core::BlockDecl| {
                let (limits, deadman) = (profile.limits(), profile.deadman());
                {
                    let mut state = link.state();
                    state.profile = Some((relative.clone(), profile.clone()));
                    state.pending.clear();
                    state.status = None;
                }
                Ok(Box::new(DriveInputBlock { link: link.clone(), profile: relative.clone(), limits, deadman, state: TwistState::default(), interface: decl.interface.clone() }) as Box<dyn BlockImplementation>)
            }),
        );
    }
    Ok(hosts)
}

/// How a person reaches one joint.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "via", rename_all = "snake_case")]
pub enum Via {
    /// The robot's own `<joint>.target` input, which nothing else drives.
    Target,
    /// A controller block's setpoint input for this joint.
    Setpoint { block: String, input: String },
}

/// One joint a person can move.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct JogChannel {
    pub joint: String,
    #[serde(skip)]
    pub terminal: Terminal,
    #[serde(skip)]
    pub kind: QuantityKind,
    /// Its value before anyone moves it.
    pub hold: f64,
    #[serde(flatten)]
    pub via: Via,
}

/// Which joints of a composed robot a person can move, and why the others
/// cannot be.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct JogPlan {
    pub channels: Vec<JogChannel>,
    /// Joint → why it cannot be moved here.
    pub refused: BTreeMap<String, String>,
}

/// The jog plan of `robot` (a generated instance at the system's top level,
/// its port signature `ports`) in `document`.
pub fn jog_plan(document: &SystemDocument, robot: &str, ports: &BTreeMap<String, PortSchema>, model: &sim_domain_robot::PhysicalModel) -> JogPlan {
    let mut plan = JogPlan::default();
    let Some(root) = document.definitions.get(&document.root) else { return plan };
    let net_of = |t: &Terminal| root.nets.iter().find(|n| n.terminals.contains(t));
    let driven = |t: &Terminal| net_of(t).is_some();
    for (port, schema) in ports {
        let (Some(joint), PortSchema::SignalIn(kind)) = (port.strip_suffix(".target"), schema) else { continue };
        let terminal = Terminal::port(robot, port);
        let Some(net) = net_of(&terminal) else {
            let hold = model.control.targets.get(joint).copied().unwrap_or(0.0);
            plan.channels.push(JogChannel { joint: joint.to_owned(), terminal, kind: kind.clone(), hold, via: Via::Target });
            continue;
        };
        // Who drives the target: a block's output on the same net.
        let driver = net.terminals.iter().find_map(|t| match t {
            Terminal::Port { instance, port } if instance != robot => match root.instances.get(instance).map(|i| &i.kind) {
                Some(InstanceKind::Block { interface, .. }) if interface.outputs.iter().any(|p| &p.name == port) => Some((instance.clone(), interface.clone())),
                _ => None,
            },
            _ => None,
        });
        let Some((block, interface)) = driver else {
            plan.refused.insert(joint.to_owned(), format!("{terminal} is already driven, and not by a controller block with a setpoint"));
            continue;
        };
        // A block driving several robot targets cannot use a `*` setpoint.
        let targets_driven = interface.outputs.iter().filter(|o| {
            net_of(&Terminal::port(&block, &o.name)).is_some_and(|n| n.terminals.iter().any(|t| matches!(t, Terminal::Port { instance, port } if instance == robot && port.ends_with(".target"))))
        }).count();
        let setpoint = interface.inputs.iter().find(|i| match i.setpoint.as_deref() {
            Some(j) if j == joint => true,
            Some("*") => targets_driven == 1,
            _ => false,
        });
        match setpoint {
            None => {
                plan.refused.insert(joint.to_owned(), format!("controller `{block}` drives {terminal} and declares no setpoint input for `{joint}`"));
            }
            Some(input) if driven(&Terminal::port(&block, &input.name)) => {
                plan.refused.insert(joint.to_owned(), format!("controller `{block}` drives {terminal}, and its setpoint `{}` is already driven", input.name));
            }
            Some(input) => plan.channels.push(JogChannel {
                joint: joint.to_owned(),
                terminal: Terminal::port(&block, &input.name),
                kind: input.kind.clone(),
                hold: input.start.unwrap_or(0.0),
                via: Via::Setpoint { block: block.clone(), input: input.name.clone() },
            }),
        }
    }
    plan
}

