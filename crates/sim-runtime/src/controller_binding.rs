//! Which controller drives a robot, beside its model: `<stem>.controller.json`
//! (`sim.controller-binding/1`) names an external simloop program and the
//! robot's drive profile (`sim.drive/1`). Loading resolves both against the
//! model — drive geometry derived from CAD, limits from the profile — into the
//! [`ControllerProgram`] a [`Scene`] runs, with the four teleoperation command
//! channels ([`COMMAND_CHANNELS`]) appended to the controller's sensor frame.
//!
//! The binding never carries physical values: geometry comes from the model,
//! limits from the profile; the controller receives both resolved, with their
//! provenance, as `--drive-json <ResolvedDrive>`.
use crate::BuildOptions;
use crate::session::{ControllerProgram, ExternalProgram, InputChannel, Scene};
use serde::{Deserialize, Serialize};
use sim_core::QuantityKind;
use sim_domain_control::drive::profile::{DriveProfile, ResolvedDrive};
use sim_domain_robot::PhysicalModel;
use std::path::{Path, PathBuf};

pub const BINDING_SCHEMA: &str = "sim.controller-binding/1";
const BINDING_FAMILY: &str = "sim.controller-binding/";
/// A teleoperated scene's length: long enough for a driving session; the run
/// thread ends it (and a recording) when the user stops.
pub const DRIVE_DURATION_S: f64 = 600.0;
/// The command channels appended after the physical contract's sensors, in
/// this order: the limited body twist (m/s, m/s, rad/s) and the request
/// sequence the controller's deadman watches.
pub const COMMAND_CHANNELS: [&str; 4] = ["command.forward", "command.lateral", "command.yaw", "command.heartbeat"];
/// The heartbeat counts requests and travels as f64: exact up to 2^53.
pub const HEARTBEAT_MAX: f64 = (1u64 << 53) as f64;
const DRIVE_JSON_FLAG: &str = "--drive-json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerBinding {
    pub schema: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub controller: BoundController,
    /// The drive profile, relative to the binding file.
    pub drive_profile: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundController {
    /// Only `python`.
    pub language: String,
    /// The script, relative to the binding file.
    pub script: PathBuf,
    /// Arguments before the host's `--drive-json <ResolvedDrive>`.
    #[serde(default)]
    pub args: Vec<String>,
}

impl ControllerBinding {
    /// Parse and check a binding; every error names `file` and the field.
    /// The schema is read first so a newer file is refused as newer rather
    /// than for a field this build has never heard of.
    pub fn from_json(text: &str, file: &Path) -> Result<Self, String> {
        let at = |field: &str, message: String| format!("{}: {field}: {message}", file.display());
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{}: {e}", file.display()))?;
        let schema = match value.get("schema") {
            None => return Err(at("schema", format!("missing; expected \"{BINDING_SCHEMA}\""))),
            Some(serde_json::Value::String(schema)) => schema.clone(),
            Some(other) => return Err(at("schema", format!("expected a string, found {other}"))),
        };
        if schema != BINDING_SCHEMA {
            let newer = schema
                .strip_prefix(BINDING_FAMILY)
                .and_then(|v| v.parse::<u32>().ok())
                .is_some_and(|v| v > 1);
            return Err(at("schema", if newer {
                format!("newer schema {schema}; this build reads {BINDING_SCHEMA}")
            } else {
                format!("unknown schema `{schema}`; this build reads {BINDING_SCHEMA}")
            }));
        }
        // From the text, not the value, so serde's error carries line and column.
        let binding: Self = serde_json::from_str(text).map_err(|e| format!("{}: {e}", file.display()))?;
        binding.validate(file)?;
        Ok(binding)
    }

    pub fn validate(&self, file: &Path) -> Result<(), String> {
        let at = |field: &str, message: &str| Err(format!("{}: {field}: {message}", file.display()));
        if self.controller.language != "python" {
            return Err(format!(
                "{}: controller.language: `{}` is not supported; this build starts `python` programs only",
                file.display(),
                self.controller.language
            ));
        }
        if self.controller.script.as_os_str().is_empty() {
            return at("controller.script", "is empty");
        }
        if self.controller.args.iter().any(|a| a == DRIVE_JSON_FLAG || a.starts_with("--drive-json=")) {
            return at("controller.args", "`--drive-json` is appended by the host from the drive profile; remove it");
        }
        if self.drive_profile.as_os_str().is_empty() {
            return at("drive_profile", "is empty");
        }
        Ok(())
    }
}

/// The binding found beside a model by convention:
/// `robot.simrobot.json` → `robot.controller.json` in the same directory.
pub fn binding_path_for(model_path: &Path) -> PathBuf {
    let name = model_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = name
        .strip_suffix(".simrobot.json")
        .or_else(|| name.strip_suffix(".json"))
        .unwrap_or(&name);
    model_path.with_file_name(format!("{stem}.controller.json"))
}

/// The command inputs a driven scene appends to the controller's sensors, in
/// [`COMMAND_CHANNELS`] order. Twist bounds are ± the profile's max speed
/// (0..0 for an unsupported axis); the heartbeat is 0..2^53. All start at 0.
pub fn drive_inputs(resolved: &ResolvedDrive) -> Vec<InputChannel> {
    let kinds = [QuantityKind::LinearVelocity, QuantityKind::LinearVelocity, QuantityKind::AngularVelocity];
    let mut inputs: Vec<InputChannel> = kinds
        .into_iter()
        .enumerate()
        .map(|(i, kind)| {
            let bound = if resolved.limits.supported[i] { resolved.limits.max_speed[i] } else { 0.0 };
            InputChannel { name: COMMAND_CHANNELS[i].to_owned(), kind, lower: if bound == 0.0 { 0.0 } else { -bound }, upper: bound, initial: 0.0 }
        })
        .collect();
    inputs.push(InputChannel {
        name: COMMAND_CHANNELS[3].to_owned(),
        kind: QuantityKind::Dimensionless,
        lower: 0.0,
        upper: HEARTBEAT_MAX,
        initial: 0.0,
    });
    inputs
}

/// What a recording must match to be replayed against the current binding:
/// the code (path and bytes), the simloop library it imports (bytes), its
/// arguments, and the drive profile (path and bytes). The `--drive-json`
/// argument is derived from these and the model, so it is not part of the
/// identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerIdentity {
    pub script: PathBuf,
    pub script_sha256: String,
    /// [`crate::session::library_sha256`] of the simloop library. `None` only
    /// for a scene recorded before the library was hashed: optional (rather
    /// than refusing such a recording outright) so it still parses and its
    /// replay is refused through [`Self::differences`] as
    /// `library_sha256: recorded (none), current <hex>`, like any other field.
    #[serde(default)]
    pub library_sha256: Option<String>,
    /// Binding arguments, without `--drive-json`.
    pub args: Vec<String>,
    pub profile: PathBuf,
    pub profile_sha256: String,
}

impl ControllerIdentity {
    /// One line per differing field, `self` being the recorded identity and
    /// `current` the one loaded now; empty when they match.
    pub fn differences(&self, current: &ControllerIdentity) -> Vec<String> {
        let mut out = Vec::new();
        let mut field = |name: &str, recorded: String, now: String| {
            if recorded != now {
                out.push(format!("{name}: recorded {recorded}, current {now}"));
            }
        };
        field("script", self.script.display().to_string(), current.script.display().to_string());
        field("script_sha256", self.script_sha256.clone(), current.script_sha256.clone());
        let library = |sha: &Option<String>| sha.clone().unwrap_or_else(|| "(none)".to_owned());
        field("library_sha256", library(&self.library_sha256), library(&current.library_sha256));
        field("args", format!("{:?}", self.args), format!("{:?}", current.args));
        field("profile", self.profile.display().to_string(), current.profile.display().to_string());
        field("profile_sha256", self.profile_sha256.clone(), current.profile_sha256.clone());
        out
    }
}

/// The identity a scene's controller program carries (e.g. read back from a
/// recording): `Ok(None)` for a Rhai program; an error when an external
/// program lacks the trailing `--drive-json <ResolvedDrive>` a binding adds.
pub fn identity_of(program: &ControllerProgram) -> Result<Option<ControllerIdentity>, String> {
    let Some(external) = &program.external else { return Ok(None) };
    let n = external.args.len();
    if n < 2 || external.args[n - 2] != DRIVE_JSON_FLAG {
        return Err(format!(
            "external controller {}: arguments do not end with `{DRIVE_JSON_FLAG} <resolved drive>`; not a bound drive controller",
            external.script.display()
        ));
    }
    let drive: serde_json::Value = serde_json::from_str(&external.args[n - 1])
        .map_err(|e| format!("external controller {}: {DRIVE_JSON_FLAG}: {e}", external.script.display()))?;
    let text = |field: &str| {
        drive.get(field).and_then(|v| v.as_str()).map(str::to_owned).ok_or_else(|| {
            format!("external controller {}: {DRIVE_JSON_FLAG}: `{field}` missing or not a string", external.script.display())
        })
    };
    let profile_sha256 = text("profile_sha256")?;
    if let Some(declared) = &external.profile_sha256
        && *declared != profile_sha256
    {
        return Err(format!(
            "external controller {}: profile_sha256 {declared} disagrees with {DRIVE_JSON_FLAG} profile_sha256 {profile_sha256}",
            external.script.display()
        ));
    }
    Ok(Some(ControllerIdentity {
        script: external.script.clone(),
        script_sha256: external.script_sha256.clone(),
        library_sha256: external.library_sha256.clone(),
        args: external.args[..n - 2].to_vec(),
        profile: PathBuf::from(text("profile")?),
        profile_sha256,
    }))
}

/// A model's bound controller, resolved and ready to put in a [`Scene`].
#[derive(Clone, Debug)]
pub struct ControlledRobot {
    pub binding_path: PathBuf,
    pub binding: ControllerBinding,
    /// Canonical path of the drive profile.
    pub profile_path: PathBuf,
    pub profile: DriveProfile,
    pub resolved: ResolvedDrive,
    pub program: ControllerProgram,
    pub identity: ControllerIdentity,
}

/// Read the binding, hash the script and its simloop library, load the drive
/// profile and resolve it against `model`. Reads files: hosts call it off the
/// UI thread. Every error names the file (binding, profile, script or
/// library) and the field.
#[cfg(not(target_arch = "wasm32"))]
pub fn load(binding_path: &Path, model: &PhysicalModel) -> Result<ControlledRobot, String> {
    use sim_domain_control::drive::profile::sha256_hex;
    let shown = binding_path.display();
    let text = std::fs::read_to_string(binding_path).map_err(|e| format!("{shown}: cannot read the controller binding: {e}"))?;
    let binding = ControllerBinding::from_json(&text, binding_path)?;
    let dir = binding_path.parent().unwrap_or(Path::new(""));

    let given = dir.join(&binding.controller.script);
    let script = std::fs::canonicalize(&given)
        .map_err(|e| format!("{shown}: controller.script: {} cannot be found: {e}", given.display()))?;
    if !script.is_file() {
        return Err(format!("{shown}: controller.script: {} is not a file", script.display()));
    }
    let bytes = std::fs::read(&script).map_err(|e| format!("{shown}: controller.script: {} cannot be read: {e}", script.display()))?;
    let script_sha256 = sha256_hex(&bytes);
    // `sim_couple::python` puts `<clients>/python` on PYTHONPATH for `simloop`.
    let clients_root = script
        .ancestors()
        .skip(1)
        .find(|a| a.file_name().is_some_and(|n| n == "clients"))
        .map(Path::to_path_buf)
        .ok_or_else(|| format!(
            "{shown}: controller.script: {} is not inside a `clients` directory (simloop is imported from clients/python)",
            script.display()
        ))?;
    let library_sha256 = crate::session::library_sha256(&clients_root)
        .map_err(|e| format!("{shown}: controller.script: {e}"))?;

    let given = dir.join(&binding.drive_profile);
    let profile_path = std::fs::canonicalize(&given)
        .map_err(|e| format!("{shown}: drive_profile: {} cannot be found: {e}", given.display()))?;
    let (profile, profile_sha256) = DriveProfile::load(&profile_path).map_err(|e| e.to_string())?;
    let geometry = sim_domain_robot::drive_geometry::resolve(model, &profile)
        .map_err(|e| format!("{}: {e}", profile_path.display()))?;
    let resolved = profile.resolve(&profile_path, &profile_sha256, geometry).map_err(|e| e.to_string())?;
    let drive_json = serde_json::to_string(&resolved)
        .map_err(|e| format!("{}: cannot encode the resolved drive: {e}", profile_path.display()))?;

    let mut args = binding.controller.args.clone();
    args.push(DRIVE_JSON_FLAG.to_owned());
    args.push(drive_json);
    let program = ControllerProgram {
        sources: Default::default(),
        parameters: serde_json::json!({}),
        inputs: drive_inputs(&resolved),
        external: Some(ExternalProgram {
            language: binding.controller.language.clone(),
            script: script.clone(),
            script_sha256: script_sha256.clone(),
            args,
            clients_root,
            profile_sha256: Some(profile_sha256.clone()),
            library_sha256: Some(library_sha256.clone()),
        }),
    };
    let identity = ControllerIdentity {
        script,
        script_sha256,
        library_sha256: Some(library_sha256),
        args: binding.controller.args.clone(),
        profile: profile_path.clone(),
        profile_sha256,
    };
    Ok(ControlledRobot { binding_path: binding_path.to_path_buf(), binding, profile_path, profile, resolved, program, identity })
}

/// The driven scene: the model held by its CAD control block, the bound
/// controller on the seam at the model's control period, default build options.
pub fn scene(model: PhysicalModel, controlled: &ControlledRobot, duration_s: f64) -> Scene {
    let period_s = model.control.period_s;
    Scene {
        version: 1,
        robot: model,
        options: BuildOptions::default(),
        controller: Some(controlled.program.clone()),
        period_s,
        duration_s,
        robot_input: None,
    }
}
