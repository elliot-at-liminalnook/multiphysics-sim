//! A system file's hosted robot: the root instances linked to files
//! (`SystemDocument::links`, `Command::LinkFile`) resolved into what the
//! shared drive host runs ([`crate::drive_host::DriveHost`]): one
//! `robot.articulated` linked to a `.simrobot.json`, one `control.external`
//! linked to the robot's controller binding (`.controller.json`) and
//! optionally one `control.drive_limiter` linked to the drive profile
//! (`.drive.json`) that binding names. Reads files: call it on a run thread
//! or job, never the UI thread.
//!
//! The hosted instances' parameters come from those files (the model's
//! control period, the profile's limits), never from the document; the only
//! parameters a document may give them are the controller's command-channel
//! port members (`sense.command.<axis>`), which declare the authored wiring
//! `limiter.twist.<axis> → controller.sense.command.<axis>`. The host runs
//! that wiring itself: the run thread's limiter and deadman
//! (`TwistState`) feed the seam's command channels
//! (`controller_binding::COMMAND_CHANNELS`) and the Python controller mixes.
use crate::controller_binding::{self, ControlledRobot, DRIVE_DURATION_S};
use crate::drive_host::DriveHost;
use crate::session::Scene;
use serde_json::{Value, json};
use sim_domain_control::drive::kinematics::{ACCEL_UNITS, AXIS_NAMES, SPEED_UNITS};
use sim_domain_robot::PhysicalModel;
use sim_system::{Flattened, Net, SystemDocument, Terminal};
use std::path::{Path, PathBuf};

pub const ROBOT: &str = "robot.articulated";
pub const CONTROLLER: &str = "control.external";
pub const LIMITER: &str = "control.drive_limiter";

/// What a robot system is (and its limit): the one rule [`resolve`] enforces.
pub const RULE: &str = "a robot system hosts exactly one robot.articulated (linked to its .simrobot.json), one control.external (linked to the robot's controller binding, .controller.json) and at most one control.drive_limiter (linked to the drive profile that binding names, .drive.json); they run on the shared drive host (sim_runtime::drive_host: Session with the external controller on the model's seam, the limiter and deadman on simulation time) from the linked files, at the model's control period, for controller_binding::DRIVE_DURATION_S of simulated time. The controller's link must be the robot's own binding (<stem>.controller.json beside the model). Their parameters come from the files; the document may give only the controller's sense.command.<forward|lateral|yaw> port members. The only wiring between them is limiter.twist.<axis> → controller.sense.command.<axis> (what the host runs). No other element can run beside a hosted robot yet: a mixed system is refused naming the element";

/// A linked file a hosted instance runs from.
#[derive(Clone, Debug, PartialEq)]
pub struct Linked {
    /// The root instance name.
    pub instance: String,
    /// `FileLink::path` as written, relative to the system file.
    pub link: String,
    /// The system file's directory joined with `link`.
    pub path: PathBuf,
}

/// A resolved robot system: the loaded model and binding and the scene the drive host runs.
pub struct RobotSystem {
    pub robot: Linked,
    pub controller: Linked,
    pub limiter: Option<Linked>,
    /// The model, parsed from `robot.path` by the shared physical loader (`PhysicalModel::parse`).
    pub model: PhysicalModel,
    /// The binding loaded from the controller's linked file (`controller_binding::load`).
    pub controlled: ControlledRobot,
    /// `controller_binding::scene(model, controlled, DRIVE_DURATION_S)`.
    pub scene: Scene,
    /// The authored wiring the host runs, as `limiter.twist.<axis> → controller.sense.command.<axis>`.
    pub wiring: Vec<String>,
}

/// The system's hosted robot, or None when the document hosts nothing.
/// `flat` is `sim_system::flatten(document, …)` of the same document (its
/// `hosted` and `hosted_nets`, and `components`: anything compiled beside a
/// hosted robot is refused). `document_path` is the system file; links
/// resolve against its directory. Every error names the instance, its type
/// and the file.
pub fn resolve(document: &SystemDocument, document_path: &Path, flat: &Flattened) -> Result<Option<RobotSystem>, String> {
    if flat.source_hash != document.content_hash() {
        return Err("the flattened model is not this system file's (flatten the document being resolved)".into());
    }
    if flat.hosted.is_empty() {
        return Ok(None);
    }
    if let Some((path, id)) = flat.components.iter().next() {
        let kind = flat.model.behaviors.get(*id).map(|b| b.kind.0.clone()).unwrap_or_else(|| "an element".into());
        return Err(format!("a robot system runs its linked robot, controller and drive limiter on the shared drive host; `{path}` ({kind}) is not hosted ({} element{} beside the hosted ones; {RULE})", flat.components.len(), if flat.components.len() == 1 { "" } else { "s" }));
    }
    let dir = document_path.parent().unwrap_or(Path::new(""));
    let of = |kind: &str| -> Vec<Linked> {
        flat.hosted.iter().filter(|(_, h)| h.component_type == kind).map(|(name, h)| Linked { instance: name.clone(), link: h.path.clone(), path: dir.join(&h.path) }).collect()
    };
    let names = |v: &[Linked]| v.iter().map(|l| format!("`{}`", l.instance)).collect::<Vec<_>>().join(", ");
    let (robots, controllers, limiters) = (of(ROBOT), of(CONTROLLER), of(LIMITER));
    if let Some((name, h)) = flat.hosted.iter().find(|(_, h)| ![ROBOT, CONTROLLER, LIMITER].contains(&h.component_type.as_str())) {
        return Err(format!("`{name}` ({}) is hosted, but the drive host runs only {ROBOT}, {CONTROLLER} and {LIMITER}", h.component_type));
    }
    let [robot] = robots.as_slice() else {
        return Err(format!("a robot system hosts exactly one {ROBOT}; this one hosts {} ({})", robots.len(), if robots.is_empty() { "none".into() } else { names(robots.as_slice()) }));
    };
    let [controller] = controllers.as_slice() else {
        return Err(format!("a robot system hosts exactly one {CONTROLLER} (the robot's controller binding); this one hosts {} ({})", controllers.len(), if controllers.is_empty() { "none".into() } else { names(controllers.as_slice()) }));
    };
    let limiter = match limiters.as_slice() {
        [] => None,
        [one] => Some(one.clone()),
        many => return Err(format!("a robot system hosts at most one {LIMITER}; this one hosts {} ({})", many.len(), names(many))),
    };
    // Parameters: only the controller's command-channel port members.
    for (name, h) in &flat.hosted {
        for parameter in h.parameters.keys() {
            let allowed = h.component_type == CONTROLLER && parameter.strip_prefix("sense.command.").is_some_and(|axis| AXIS_NAMES.contains(&axis));
            if !allowed {
                let source = match h.component_type.as_str() {
                    ROBOT => "the model file",
                    CONTROLLER => "the robot's seam (its contract and the model's control period) and the binding",
                    _ => "the drive profile the controller binding names",
                };
                return Err(format!("`{name}`.{parameter} is set in the system file, but a hosted {}'s parameters come from {source} (`{}`); remove it (only {CONTROLLER} sense.command.<forward|lateral|yaw> may be given, to declare the command wiring from the limiter's twist; the heartbeat channel is the host's request count and is not wired)", h.component_type, h.path));
            }
        }
    }
    let wiring = check_wiring(&flat.hosted_nets, &controller.instance, limiter.as_ref().map(|l| l.instance.as_str()))?;
    let at = |l: &Linked, kind: &str| format!("`{}` ({kind}, {})", l.instance, l.path.display());

    let text = std::fs::read_to_string(&robot.path).map_err(|e| format!("{}: cannot read the model: {e}", at(robot, ROBOT)))?;
    let model = PhysicalModel::parse(&text).map_err(|e| format!("{}: {e}", at(robot, ROBOT)))?;
    let controlled = controller_binding::load(&controller.path, &model).map_err(|e| format!("{}: {e}", at(controller, CONTROLLER)))?;
    // The controller must be the robot's own binding (`<stem>.controller.json` beside the model, as Robot mode finds it).
    let expected = controller_binding::binding_path_for(&robot.path);
    let expected_canonical = std::fs::canonicalize(&expected).map_err(|e| {
        format!("{} must be the binding of {}, but {} cannot be found: {e} (controller_binding::binding_path_for, beside the model)", at(controller, CONTROLLER), at(robot, ROBOT), expected.display())
    })?;
    // `controller_binding::load` above read the linked file, so it exists.
    let same = std::fs::canonicalize(&controller.path).is_ok_and(|a| a == expected_canonical);
    if !same {
        return Err(format!(
            "{} is not the binding of {}: a robot's controller binding is {} (controller_binding::binding_path_for, beside the model); link `{}` to it",
            at(controller, CONTROLLER),
            at(robot, ROBOT),
            expected.display(),
            controller.instance
        ));
    }
    if let Some(l) = &limiter {
        let linked = std::fs::canonicalize(&l.path).map_err(|e| format!("{}: the drive profile cannot be found: {e}", at(l, LIMITER)))?;
        if linked != controlled.profile_path {
            return Err(format!(
                "{} links {}, but the controller binding {} names the drive profile {}; the profile is the one source of the drive limits: link `{}` to the binding's drive_profile",
                at(l, LIMITER),
                linked.display(),
                controlled.binding_path.display(),
                controlled.profile_path.display(),
                l.instance
            ));
        }
    }
    let scene = controller_binding::scene(model.clone(), &controlled, DRIVE_DURATION_S);
    Ok(Some(RobotSystem { robot: robot.clone(), controller: controller.clone(), limiter, model, controlled, scene, wiring }))
}

/// The authored nets between hosted instances must be ones the drive host
/// runs: `limiter.twist.<axis>` joined to `controller.sense.command.<axis>`
/// (two terminals, the same axis). Returns them as text.
fn check_wiring(nets: &[Net], controller: &str, limiter: Option<&str>) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for net in nets {
        let shown = net.terminals.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ");
        let refuse = || format!("the net [{shown}] is not wiring the drive host runs: it runs `<limiter>.twist.<axis>` → `{controller}.sense.command.<axis>` (axis forward, lateral or yaw; two terminals each)");
        let ports: Vec<(&str, &str)> = net.terminals.iter().filter_map(|t| match t {
            Terminal::Port { instance, port } => Some((instance.as_str(), port.as_str())),
            Terminal::Boundary { .. } => None,
        }).collect();
        let (Some(limiter), [a, b]) = (limiter, ports.as_slice()) else { return Err(refuse()) };
        let (from, to) = if a.0 == limiter { (a, b) } else { (b, a) };
        let axis = from.1.strip_prefix("twist.");
        if from.0 != limiter || to.0 != controller || axis.is_none() || to.1.strip_prefix("sense.command.") != axis || net.terminals.len() != 2 {
            return Err(refuse());
        }
        out.push(format!("{}.{} → {}.{}", from.0, from.1, to.0, to.1));
    }
    out.sort();
    Ok(out)
}

impl RobotSystem {
    /// The drive host for this system: `DriveHost::new` (which builds the
    /// shared `Session`: `PhysicalRobot::build`, then the controller program
    /// started and attached on the model's seam). Errors name the system
    /// instance and the controller ([`Self::name_error`]).
    pub fn host(&self, seed: u64) -> Result<DriveHost, String> {
        DriveHost::new(self.scene.clone(), seed, &self.controlled).map_err(|e| self.name_error(&e))
    }

    /// An error from the session (build or step) named by the system's
    /// instances: one carrying the controller's label (`external controller
    /// (python) <script> on <element>`) becomes ``"`controller` (<label>): <rest>"``;
    /// anything else is prefixed with the robot and controller instances and their files.
    pub fn name_error(&self, error: &str) -> String {
        if let Some(external) = self.scene.controller.as_ref().and_then(|c| c.external.as_ref()) {
            let prefix = format!("external controller ({}) {} on ", external.language, external.script.display());
            if let Some(at) = error.find(&prefix) {
                let after = &error[at + prefix.len()..];
                if let Some(colon) = after.find(": ") {
                    let label = &error[at..at + prefix.len() + colon];
                    let rest = format!("{}{}", &error[..at], &after[colon + 2..]);
                    return format!("`{}` ({label}): {rest}", self.controller.instance);
                }
            }
        }
        format!("robot system (`{}` from {}, `{}` from {}): {error}", self.robot.instance, self.robot.link, self.controller.instance, self.controller.link)
    }

    /// The system's drive for `system_state`: instances and files, the
    /// profile's limits with units, the deadman, the session's period and
    /// channels, the wiring and the rule.
    pub fn json(&self) -> Value {
        let r = &self.controlled.resolved;
        let axes: Vec<Value> = (0..3)
            .map(|i| json!({"axis": AXIS_NAMES[i], "supported": r.limits.supported[i],
                "max_speed": {"value": r.limits.max_speed[i], "unit": SPEED_UNITS[i]}, "max_accel": {"value": r.limits.max_accel[i], "unit": ACCEL_UNITS[i]},
                "stop_decel": {"value": r.limits.stop_decel[i], "unit": ACCEL_UNITS[i]}}))
            .collect();
        let label = self.scene.controller.as_ref().and_then(|c| c.external.as_ref()).map(|e| format!("external controller ({}) {}", e.language, e.script.display()));
        json!({
            "elements": {
                "robot": {"instance": self.robot.instance, "type": ROBOT, "link": self.robot.link, "path": self.robot.path},
                "controller": {"instance": self.controller.instance, "type": CONTROLLER, "link": self.controller.link, "path": self.controller.path, "label": label},
                "limiter": self.limiter.as_ref().map(|l| json!({"instance": l.instance, "type": LIMITER, "link": l.link, "path": l.path})),
            },
            "profile": {"path": self.controlled.profile_path, "sha256": r.profile_sha256},
            "kinematics": r.kinematics,
            "limits": axes,
            "deadman": {"timeout_s": {"value": r.deadman.timeout_s, "unit": "s"}, "on_loss": r.deadman.on_loss},
            "session": {"period_s": self.scene.period_s, "duration_s": self.scene.duration_s, "channels": controller_binding::COMMAND_CHANNELS},
            "identity": serde_json::to_value(&self.controlled.identity).unwrap_or(Value::Null),
            "wiring": self.wiring,
            "rule": RULE,
        })
    }
}
