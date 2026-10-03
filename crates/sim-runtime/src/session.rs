//! Host-independent interactive episodes. Hosts send inputs and render snapshots;
//! all integration, sensor sampling and Rhai execution remain in Rust.
use crate::{BuildOptions, PhysicalRobot, registry};
use serde::{Deserialize, Serialize};
use sim_core::{Channel, Contract, Coupler, CouplerError, QuantityKind};
use sim_domain_robot::PhysicalModel;
use sim_script::{RhaiController, Sources, parameter_map};
use std::collections::BTreeSet;
#[cfg(not(target_arch = "wasm32"))]
use sim_domain_control::drive::profile::sha256_hex;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputChannel {
    pub name: String,
    pub kind: QuantityKind,
    pub lower: f64,
    pub upper: f64,
    pub initial: f64,
}

/// A controller that runs out of process on the seam: a simloop program
/// speaking the frame protocol of `sim_couple` over its stdin/stdout. The
/// script is identified by the sha256 of its bytes, so a scene (and the
/// recording that carries it) only ever runs against the code it was made
/// with; `Session::new` refuses a changed script by name. The simloop library
/// it imports is identified the same way ([`library_sha256`]).
///
/// Starting a session (`Session::new`, `Session::replay`, the `sim-session`
/// CLI) from a scene or recording that names one runs `python3` on that
/// script with the recorded `args`, as long as the script on disk hashes to
/// `script_sha256`: only code already on disk can run, but a scene file is
/// as trusted as the code it names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalProgram {
    /// Only `python` (`sim_couple::python`: `python3 -u script args…`).
    pub language: String,
    /// Absolute path of the script.
    pub script: PathBuf,
    /// Lowercase hex sha256 of the script's bytes.
    pub script_sha256: String,
    /// Everything after the script on the command line, in order.
    #[serde(default)]
    pub args: Vec<String>,
    /// The repository's `clients/` directory; `clients/python` goes on PYTHONPATH.
    pub clients_root: PathBuf,
    /// sha256 of the drive profile the args were resolved from, when there is one,
    /// so a recording names the profile bytes as well as the code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_sha256: Option<String>,
    /// [`library_sha256`] of `<clients_root>/python/simloop` (the deadman,
    /// limiter and mixer the script imports), so an edit to the library is a
    /// different controller too. Absent in scenes written before it existed;
    /// those run unchecked and replay comparison reports the absence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_sha256: Option<String>,
}
impl ExternalProgram {
    /// The prefix every error from this controller carries.
    pub fn label(&self, element: &str) -> String {
        format!("external controller ({}) {} on {element}", self.language, self.script.display())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerProgram {
    /// The Rhai program; empty (the default) when `external` runs instead.
    #[serde(default)]
    pub sources: Sources,
    #[serde(default = "empty_parameters")]
    pub parameters: serde_json::Value,
    /// Additional named command inputs, appended to the controller observations.
    #[serde(default)]
    pub inputs: Vec<InputChannel>,
    /// An out-of-process controller instead of Rhai. Absent in every scene and
    /// recording written before it existed, which therefore parse unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<ExternalProgram>,
}
fn empty_parameters() -> serde_json::Value {
    serde_json::json!({})
}
impl ControllerProgram {
    /// Exactly one program: non-empty Rhai sources or an external program in a
    /// language this build can start.
    pub fn validate(&self) -> Result<(), String> {
        match (&self.external, self.sources.is_empty()) {
            (None, true) => Err("controller program has neither Rhai sources nor an external program".into()),
            (Some(external), false) => Err(format!(
                "controller program has both Rhai sources (`{}`) and an external program ({}); give exactly one",
                self.sources.entry,
                external.script.display()
            )),
            (Some(external), true) if external.language != "python" => Err(format!(
                "external controller {}: language `{}` is not supported; this build starts `python` programs only",
                external.script.display(),
                external.language
            )),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub version: u32,
    pub robot: PhysicalModel,
    pub options: BuildOptions,
    pub controller: Option<ControllerProgram>,
    pub period_s: f64,
    pub duration_s: f64,
    /// Retained episode input, not proof that its properties were measured in CAD.
    /// None denotes a caller-created parsed model with unknown original presence.
    pub robot_input: Option<crate::robot_input::RobotInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneInput {
    version: u32,
    robot: serde_json::Value,
    options: BuildOptions,
    #[serde(default)]
    controller: Option<ControllerProgram>,
    period_s: f64,
    duration_s: f64,
    #[serde(default)]
    robot_input: Option<crate::robot_input::InputReceipt>,
}
impl<'de> Deserialize<'de> for Scene {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self,D::Error> {
        let input=SceneInput::deserialize(deserializer)?;
        let robot=serde_json::from_value(input.robot.clone()).map_err(serde::de::Error::custom)?;
        let robot_input=crate::robot_input::RobotInput::receive(input.robot,input.robot_input,&robot).map_err(serde::de::Error::custom)?;
        Ok(Self {version:input.version,robot,options:input.options,controller:input.controller,
            period_s:input.period_s,duration_s:input.duration_s,robot_input:Some(robot_input)})
    }
}
impl Serialize for Scene {
    fn serialize<S: serde::Serializer>(&self, serializer:S)->Result<S::Ok,S::Error> {
        use serde::ser::SerializeStruct;
        let (robot,receipt)=crate::robot_input::RobotInput::serialize_model(self.robot_input.as_ref(),&self.robot).map_err(serde::ser::Error::custom)?;
        let mut result=serializer.serialize_struct("Scene",6+usize::from(receipt.is_some()))?;
        result.serialize_field("version",&self.version)?;
        result.serialize_field("robot",&robot)?;
        result.serialize_field("options",&self.options)?;
        result.serialize_field("controller",&self.controller)?;
        result.serialize_field("period_s",&self.period_s)?;
        result.serialize_field("duration_s",&self.duration_s)?;
        if let Some(receipt)=receipt {result.serialize_field("robot_input",&receipt)?;}
        result.end()
    }
}
impl Scene {
    /// Replace the document explicitly when accepting a new CAD export. Direct
    /// edits to `robot` instead remain experimental overrides of the old input.
    pub fn replace_robot_input(&mut self, document:serde_json::Value)->Result<(),String> {
        let model=serde_json::from_value(document.clone()).map_err(|e|e.to_string())?;
        let input=crate::robot_input::RobotInput::receive(document,None,&model)?;
        self.robot=model;self.robot_input=Some(input);Ok(())
    }
    pub fn input_binding(&self)->Result<crate::robot_contract::InputBinding,String> {
        crate::robot_contract::InputBinding::from_scene(self)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Telemetry {
    pub sample_time: f64,
    pub sensors: Vec<f64>,
    pub actuators: Vec<f64>,
}

struct EpisodeCoupler {
    policy: Option<Box<dyn Coupler>>,
    command_channels: Vec<Channel>,
    command_values: Arc<Mutex<Vec<f64>>>,
    telemetry: Arc<Mutex<Telemetry>>,
    bounds: Vec<(Option<f64>, Option<f64>)>,
    /// Prefix for every policy error (`external controller (python) <script>
    /// on <element>`), so a failure names the program as well as the seam.
    label: Option<String>,
}
fn relabel(label: &Option<String>, error: CouplerError) -> CouplerError {
    match label {
        Some(label) => CouplerError::Other(format!("{label}: {error}")),
        None => error,
    }
}
impl Coupler for EpisodeCoupler {
    fn open(&mut self, contract: &Contract) -> Result<(), CouplerError> {
        let label = &self.label;
        if let Some(policy) = &mut self.policy {
            let mut augmented = contract.clone();
            augmented.sensors.extend(self.command_channels.clone());
            policy.open(&augmented).map_err(|e| relabel(label, e))?;
        }
        Ok(())
    }
    fn sample(
        &mut self,
        t: f64,
        sensors: &[f64],
        actuators: &mut [f64],
    ) -> Result<(), CouplerError> {
        let values = self.command_values.lock().unwrap().clone();
        let label = &self.label;
        if let Some(policy) = &mut self.policy {
            let mut inputs = sensors.to_vec();
            inputs.extend(values);
            policy.sample(t, &inputs, actuators).map_err(|e| relabel(label, e))?;
        } else {
            actuators.copy_from_slice(&values);
        }
        for (k, (&value, (lower, upper))) in actuators.iter().zip(&self.bounds).enumerate() {
            if !value.is_finite()
                || lower.is_some_and(|lo| value < lo)
                || upper.is_some_and(|hi| value > hi)
            {
                return Err(relabel(label, CouplerError::Other(format!(
                    "actuator {k} command {value} violates its CAD limits"
                ))));
            }
        }
        *self.telemetry.lock().unwrap() = Telemetry {
            sample_time: t,
            sensors: sensors.to_vec(),
            actuators: actuators.to_vec(),
        };
        Ok(())
    }
    fn close(&mut self) {
        if let Some(policy) = &mut self.policy {
            policy.close();
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkPose {
    pub name: String,
    pub position_m: [f64; 3],
    /// Row-major local-to-world rotation matrix; model and world are Z-up.
    pub rotation: [[f64; 3]; 3],
}
impl LinkPose {
    pub(crate) fn valid_rigid_transform(&self) -> bool {
        if self.position_m.iter().any(|v| !v.is_finite()) { return false; }
        let r=self.rotation;
        let dot=|a:[f64;3],b:[f64;3]|->f64{(0..3).map(|i|a[i]*b[i]).sum()};
    if !r.iter().flatten().all(|v| v.is_finite()) {
        return false;
    }
    for i in 0..3 {
        for j in 0..3 {
            if (dot(r[i], r[j]) - if i == j { 1.0 } else { 0.0 }).abs() > 1e-8 {
                return false;
            }
        }
    }
    let cross = [
        r[1][1] * r[2][2] - r[1][2] * r[2][1],
        r[1][2] * r[2][0] - r[1][0] * r[2][2],
        r[1][0] * r[2][1] - r[1][1] * r[2][0],
    ];
    (dot(r[0], cross) - 1.0).abs() <= 1e-8
}
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
    pub link: usize,
    pub other: Option<usize>,
    pub point_m: [f64; 3],
    pub force_n: [f64; 3],
    pub penetration_m: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpisodeFrame {
    pub time_s: f64,
    pub done: bool,
    pub poses: Vec<LinkPose>,
    pub joint_positions: Vec<f64>,
    pub telemetry: Telemetry,
    pub contacts: Vec<Contact>,
    pub error: Option<String>,
}

/// Portable recording contains the exact scene, seed, and one command per step.
/// Replaying reconstructs controller state and noise history as well as physics.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub version: u32,
    pub scene: Scene,
    pub seed: u64,
    pub actions: Vec<Vec<f64>>,
}

pub struct Session {
    pub robot: PhysicalRobot,
    pub robot_input: crate::robot_contract::InputBinding,
    pub scene: Scene,
    pub contract: Contract,
    pub inputs: Vec<InputChannel>,
    values: Arc<Mutex<Vec<f64>>>,
    telemetry: Arc<Mutex<Telemetry>>,
    seed: u64,
    actions: Vec<Vec<f64>>,
    error: Option<String>,
}
impl Session {
    /// Bounded diagnostic capture, disabled by default. Zero clears and disables
    /// capture; changing the limit clears old records without resetting physics.
    pub fn set_attempt_audit_limit(&mut self, limit: usize) -> Result<(), String> {
        if limit > 10_000 { return Err("attempt limit must be 0..10000".into()); }
        for island in &mut self.robot.runtime.islands { island.set_attempt_audit_limit(limit); }
        Ok(())
    }

    pub fn new(scene: Scene, seed: u64) -> Result<Self, String> {
        if scene.version != 1 {
            return Err("unsupported scene version".into());
        }
        if seed > (1_u64 << 53) - 1 {
            return Err("seed exceeds portable exact integer range".into());
        }
        for (name, value) in [
            ("period_s", scene.period_s),
            ("duration_s", scene.duration_s),
            ("step", scene.options.step),
            ("sample", scene.options.sample),
        ] {
            if !value.is_finite() || value <= 0. {
                return Err(format!("{name} must be finite and positive"));
            }
        }
        let steps = scene.period_s / scene.options.step;
        if (steps - steps.round()).abs() > 1e-8 {
            return Err("episode period must be an integer number of physics steps".into());
        }
        if scene.robot.control.mode == "trajectory" {
            return Err(
                "interactive scenes must use held targets or an explicit controller".into(),
            );
        }
        if let Some(program) = &scene.controller {
            program.validate()?;
            // Before the build: a changed script is refused without compiling the plant.
            if let Some(external) = &program.external {
                check_external(external)?;
            }
        }
        let robot_input=scene.input_binding()?;
        let mut robot = PhysicalRobot::build(scene.robot.clone(), &registry(), &scene.options)?;
        robot.runtime.seed(seed);
        let seam = robot
            .seam
            .ok_or("interactive robot must expose an actuator/controller seam")?;
        let contract = robot.runtime.contract(seam);
        let bounds: Vec<_> = contract
            .actuators
            .iter()
            .map(|a| {
                if a.name.ends_with(".duty") {
                    return (Some(-1.), Some(1.));
                }
                scene
                    .robot
                    .joint(a.name.trim_end_matches(".target"))
                    .and_then(|j| j.limits)
                    .map(|[lo, hi]| (lo.is_finite().then_some(lo), hi.is_finite().then_some(hi)))
                    .unwrap_or((None, None))
            })
            .collect();
        let inputs = if let Some(program) = &scene.controller {
            program.inputs.clone()
        } else {
            contract
                .actuators
                .iter()
                .zip(&bounds)
                .map(|(a, &(lo, hi))| InputChannel {
                    name: a.name.clone(),
                    kind: a.kind.clone(),
                    lower: lo.unwrap_or(-f64::MAX),
                    upper: hi.unwrap_or(f64::MAX),
                    initial: scene
                        .robot
                        .control
                        .targets
                        .get(a.name.trim_end_matches(".target"))
                        .copied()
                        .unwrap_or(0.),
                })
                .collect()
        };
        let mut names = BTreeSet::new();
        for input in &inputs {
            if input.name.is_empty()
                || !names.insert(&input.name)
                || (scene.controller.is_some()
                    && contract.sensors.iter().any(|s| s.name == input.name))
            {
                return Err(format!("duplicate or empty command input: {}", input.name));
            }
            if !input.lower.is_finite()
                || !input.upper.is_finite()
                || !input.initial.is_finite()
                || input.lower > input.upper
                || input.initial < input.lower
                || input.initial > input.upper
            {
                return Err(format!("invalid limits/initial value for {}", input.name));
            }
        }
        let label = scene
            .controller
            .as_ref()
            .and_then(|p| p.external.as_ref())
            .map(|external| external.label(&contract.element));
        let policy: Option<Box<dyn Coupler>> = scene
            .controller
            .as_ref()
            .map(|program| {
                if let Some(external) = &program.external {
                    return spawn_external(external, &contract.element);
                }
                let parameters = parameter_map(&program.parameters).map_err(|e| e.to_string())?;
                RhaiController::with_seed_and_registry(
                    program.sources.clone(), parameters, seed, &crate::registry(),
                )
                    .map(|c| Box::new(c) as Box<dyn Coupler>)
                    .map_err(|e| e.to_string())
            })
            .transpose()?;
        let values = Arc::new(Mutex::new(inputs.iter().map(|i| i.initial).collect()));
        let telemetry = Arc::new(Mutex::new(Telemetry::default()));
        robot
            .runtime
            .attach(
                seam,
                Box::new(EpisodeCoupler {
                    policy,
                    command_channels: inputs
                        .iter()
                        .map(|i| Channel {
                            name: i.name.clone(),
                            kind: i.kind.clone(),
                        })
                        .collect(),
                    command_values: values.clone(),
                    telemetry: telemetry.clone(),
                    bounds,
                    label,
                }),
            )
            .map_err(|e| e.to_string())?;
        // `External::couple` runs the handshake (hello/ready) inside `attach`
        // but only records a failed one, which the runtime would report at the
        // first commit. Report it now: a controller that exits before ready or
        // answers hello with nonsense refuses the session, not its first step.
        // (Dropping `robot` on this path closes the coupler and reaps the child.)
        if let Some(failure) = robot.runtime.behavior(seam).and_then(|b| b.failure()) {
            return Err(failure);
        }
        Ok(Self {
            robot,
            robot_input,
            scene,
            contract,
            inputs,
            values,
            telemetry,
            seed,
            actions: Vec::new(),
            error: None,
        })
    }

    /// One controller period. With an external controller, a process that
    /// exits, times out or sends a malformed `act` fails the step with an
    /// error carrying `external controller (python) <script> on <element>`.
    pub fn step(&mut self, action: &[f64]) -> Result<EpisodeFrame, String> {
        if let Some(error) = &self.error {
            return Err(format!("episode failed: {error}; reset before continuing"));
        }
        if self.robot.time() >= self.scene.duration_s - 1e-10 {
            return Err("episode finished; reset before continuing".into());
        }
        if action.len() != self.inputs.len() {
            return Err(format!(
                "expected {} inputs, received {}",
                self.inputs.len(),
                action.len()
            ));
        }
        for (input, &value) in self.inputs.iter().zip(action) {
            if !value.is_finite() || value < input.lower || value > input.upper {
                return Err(format!(
                    "{} input outside [{}, {}]",
                    input.name, input.lower, input.upper
                ));
            }
        }
        *self.values.lock().unwrap() = action.to_vec();
        self.actions.push(action.to_vec());
        if let Err(error) = self.robot.advance(self.scene.period_s) {
            self.error = Some(error.clone());
            return Err(error);
        }
        Ok(self.frame())
    }

    pub fn reset(&mut self, seed: u64) -> Result<EpisodeFrame, String> {
        let replacement = Self::new(self.scene.clone(), seed)?;
        *self = replacement;
        Ok(self.frame())
    }

    pub fn frame(&self) -> EpisodeFrame {
        let poses = self
            .robot
            .poses()
            .into_iter()
            .zip(&self.robot.model.links)
            .map(|((r, p), link)| LinkPose {
                name: link.name.clone(),
                position_m: p.into(),
                rotation: std::array::from_fn(|i| std::array::from_fn(|j| r[(i, j)])),
            })
            .collect();
        let contacts = self
            .robot
            .contacts()
            .into_iter()
            .map(|c| Contact {
                link: c.link,
                other: c.other,
                point_m: c.point.into(),
                force_n: c.force.into(),
                penetration_m: c.penetration,
            })
            .collect();
        EpisodeFrame {
            time_s: self.robot.time(),
            done: self.error.is_some() || self.robot.time() >= self.scene.duration_s - 1e-10,
            poses,
            joint_positions: self.robot.joint_angles(),
            telemetry: self.telemetry.lock().unwrap().clone(),
            contacts,
            error: self.error.clone(),
        }
    }

    pub fn recording(&self) -> Recording {
        Recording {
            version: 1,
            scene: self.scene.clone(),
            seed: self.seed,
            actions: self.actions.clone(),
        }
    }

    pub fn replay(recording: Recording) -> Result<Self, String> {
        if recording.version != 1 {
            return Err("unsupported recording version".into());
        }
        let mut session = Self::new(recording.scene, recording.seed)?;
        for action in recording.actions {
            session.step(&action)?;
        }
        Ok(session)
    }
}

/// The script on disk must be the bytes the scene names; anything else is a
/// different controller and the scene (or recording) does not describe it.
/// The same holds for the simloop library, when the scene records its hash.
#[cfg(not(target_arch = "wasm32"))]
fn check_external(external: &ExternalProgram) -> Result<(), String> {
    let bytes = std::fs::read(&external.script)
        .map_err(|e| format!("controller `{}` cannot be read: {e}", external.script.display()))?;
    let on_disk = sha256_hex(&bytes);
    if on_disk != external.script_sha256 {
        return Err(format!(
            "controller `{}` changed: sha256 on disk {on_disk}, recorded {}",
            external.script.display(),
            external.script_sha256
        ));
    }
    if let Some(recorded) = &external.library_sha256 {
        let on_disk = library_sha256(&external.clients_root)?;
        if on_disk != *recorded {
            return Err(format!(
                "controller library `{}` changed: sha256 on disk {on_disk}, recorded {recorded}",
                external.clients_root.join(SIMLOOP_LIBRARY).display()
            ));
        }
    }
    Ok(())
}

/// The library an external Python controller imports, relative to its
/// `clients_root` (`sim_couple::python` puts `<clients_root>/python` on PYTHONPATH).
pub const SIMLOOP_LIBRARY: &str = "python/simloop";

/// The identity of the simloop library under `clients_root`: lowercase hex
/// sha256 of a byte stream built from every regular file (or symlink to one)
/// whose name ends in `.py` anywhere below `<clients_root>/python/simloop/`
/// (`__pycache__` holds no `.py` files; symlinked directories are not
/// followed). Files are taken in ascending byte order of their path relative
/// to that directory, components joined with `/` (UTF-8); each contributes
/// `path bytes, one 0x00 byte, file length as u64 little-endian, file bytes`.
/// In Python: `b"".join(p.encode() + b"\0" + len(d).to_bytes(8, "little") + d
/// for p, d in sorted(files))`, then `hashlib.sha256(...).hexdigest()`.
/// Errors name the directory or file; a library with no `.py` file is refused.
/// Reads the disk: call it off the UI thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn library_sha256(clients_root: &Path) -> Result<String, String> {
    let root = clients_root.join(SIMLOOP_LIBRARY);
    let unreadable = |path: &Path, e: std::io::Error| format!("controller library `{}` cannot be read: {e}", path.display());
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| unreadable(&dir, e))? {
            let entry = entry.map_err(|e| unreadable(&dir, e))?;
            let path = entry.path();
            if entry.file_type().map_err(|e| unreadable(&path, e))?.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|x| x != "py") {
                continue;
            }
            let relative = path.strip_prefix(&root).map_err(|e| format!("controller library `{}`: {e}", path.display()))?;
            let parts: Option<Vec<&str>> = relative.components().map(|c| c.as_os_str().to_str()).collect();
            let name = parts
                .map(|p| p.join("/"))
                .ok_or_else(|| format!("controller library file `{}`: name is not UTF-8", path.display()))?;
            files.push((name, path));
        }
    }
    if files.is_empty() {
        return Err(format!("controller library `{}` has no .py files", root.display()));
    }
    files.sort();
    let mut stream = Vec::new();
    for (name, path) in &files {
        let bytes = std::fs::read(path).map_err(|e| unreadable(path, e))?;
        stream.extend_from_slice(name.as_bytes());
        stream.push(0);
        stream.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        stream.extend_from_slice(&bytes);
    }
    Ok(sha256_hex(&stream))
}
#[cfg(target_arch = "wasm32")]
fn check_external(external: &ExternalProgram) -> Result<(), String> {
    Err(format!("external controllers need a native host: {}", external.script.display()))
}

/// How long the run thread waits for each reply from an external controller.
/// `FrameCoupler` has one timeout for every receive: the `ready` answering
/// `hello` (which includes the interpreter's start-up and its imports, since
/// the clock starts once `hello` is sent right after the spawn) and each
/// `act`. 3 s tolerates a cold `python3` start (first-run bytecode
/// compilation, a loaded machine) yet bounds how long a wedged controller
/// holds the run thread to 3 s per step instead of the coupler's 10 s
/// default; a timed-out step fails the session, which then refuses further
/// steps until reset, so the wait is paid once.
pub const EXTERNAL_REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Start the program; the hello/ready handshake follows in `Runtime::attach`.
/// Dropping the coupler (with the session) closes it and reaps the process.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_external(external: &ExternalProgram, element: &str) -> Result<Box<dyn Coupler>, String> {
    let args: Vec<&str> = external.args.iter().map(String::as_str).collect();
    let coupler = sim_couple::python(&external.clients_root, &external.script, &args)
        .map_err(|e| format!("{}: cannot start python3: {e}", external.label(element)))?
        .with_timeout(EXTERNAL_REPLY_TIMEOUT);
    Ok(Box::new(coupler))
}
#[cfg(target_arch = "wasm32")]
fn spawn_external(external: &ExternalProgram, element: &str) -> Result<Box<dyn Coupler>, String> {
    Err(format!("{}: external controllers need a native host", external.label(element)))
}
