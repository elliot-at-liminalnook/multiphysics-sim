//! A bound robot's embedded drive program: the binding's `embedded` Rhai
//! adapter (`controller_binding::EmbeddedController`) run in the shared
//! [`EmbeddedSession`], for hosts that cannot start the external program
//! (the browser, through `sim-web`'s `DriveSimulation`).
//!
//! - [`files_to_read`] / [`build`]: the one scene builder. From the model
//!   (simrobot JSON), the binding and the files it names (drive profile,
//!   adapter sources, embedded config), all given as text so the browser
//!   fetches and Rust parses: the drive profile resolved against the model
//!   (geometry derived from CAD with its provenance, `drive_geometry::resolve`),
//!   the adapter's captured sources, parameters from the resolved drive,
//!   the typed command inputs (`controller_binding::drive_inputs`: bounds
//!   from the profile, the heartbeat 0..2^53) and the identity a replay is
//!   checked against ([`EmbeddedIdentity`]).
//! - [`DriveSession`]: the embedded session plus the shared [`TwistState`]:
//!   requests are interpreted against the profile (`DriveRequest::interpret_with`),
//!   and once per control period the limiter and deadman
//!   (`kinematics::step`, in `TwistState::advance`) run on the session's
//!   simulation time before the period is stepped, the limited twist and
//!   heartbeat going to the adapter through `EmbeddedSession::set_inputs`
//!   (so they are the recording's input events). The host sends requests
//!   only; no limiting, deadman or mixing happens in the host.
//!
//! wasm32: everything here compiles for the browser (no files, no
//! processes); [`load`] (reads files) is native only.
use crate::controller_binding::{self, ControllerBinding, EmbeddedController};
use crate::drive_host::{DriveRequest, DriveStatus, TwistState, check_inputs, twist_json};
use crate::embedded::{CaptureMode, Config, EmbeddedRecording, EmbeddedSession};
use crate::session::{ControllerProgram, Scene};
use crate::BuildOptions;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_control::drive::kinematics::{ACCEL_UNITS, AXIS_NAMES, BodyTwist, Deadman, Limits, SPEED_UNITS};
use sim_domain_control::drive::profile::{DriveProfile, ResolvedDrive, sha256_hex};
use sim_domain_robot::PhysicalModel;
use std::collections::BTreeMap;
use std::path::{Component, Path};

/// The built drive's schema (`EmbeddedDrive::schema`).
pub const EMBEDDED_DRIVE_SCHEMA: &str = "sim.embedded-drive/1";
/// The scene parameter holding the resolved drive (`ResolvedDrive`, with geometry provenance).
pub const DRIVE_PARAMETER: &str = "drive";
/// The scene parameter holding [`EmbeddedIdentity`].
pub const IDENTITY_PARAMETER: &str = "identity";
/// Most control periods one [`DriveSession::advance`] call runs (a host's bounded work chunk).
pub const MAX_ADVANCE_PERIODS: usize = 1000;

/// What a recording must match to be replayed against the loaded drive
/// ([`EmbeddedIdentity::differences`] compares the entry and the hashes;
/// the paths are kept as recorded information).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedIdentity {
    /// The binding file as the host named it.
    pub binding: String,
    /// The captured entry (its key in the scene's sources).
    pub entry: String,
    /// sha256 over the captured sources ([`sources_sha256`]).
    pub script_sha256: String,
    /// sha256 of the embedded config file's bytes.
    pub config_sha256: String,
    /// The drive profile as the host named it.
    pub profile: String,
    pub profile_sha256: String,
    /// sha256 of the model file's bytes (the simrobot JSON).
    pub model_sha256: String,
    /// The model's `source.cad_sha256` (the saved CAD it was exported from), when stamped.
    #[serde(default)]
    pub cad_sha256: Option<String>,
}

impl EmbeddedIdentity {
    /// One line per differing field that decides what runs (`self`
    /// recorded, `current` loaded now): the entry and the hashes of the
    /// script, config, profile, model and CAD. Empty when they match. The
    /// binding and profile paths are recorded information only: they differ
    /// between hosts and URL layouts for the same bytes.
    pub fn differences(&self, current: &EmbeddedIdentity) -> Vec<String> {
        let mut out = Vec::new();
        let mut field = |name: &str, recorded: &str, now: &str| {
            if recorded != now {
                out.push(format!("{name}: recorded {recorded}, current {now}"));
            }
        };
        field("entry", &self.entry, &current.entry);
        field("script_sha256", &self.script_sha256, &current.script_sha256);
        field("config_sha256", &self.config_sha256, &current.config_sha256);
        field("profile_sha256", &self.profile_sha256, &current.profile_sha256);
        field("model_sha256", &self.model_sha256, &current.model_sha256);
        let cad = |sha: &Option<String>| sha.clone().unwrap_or_else(|| "(none)".to_owned());
        field("cad_sha256", &cad(&self.cad_sha256), &cad(&current.cad_sha256));
        out
    }
}

/// A bound robot's embedded drive, ready for [`DriveSession::new`] (and
/// serializable, so the browser's main thread can hand it to its worker).
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedDrive {
    /// [`EMBEDDED_DRIVE_SCHEMA`].
    pub schema: String,
    /// The model held by its CAD control block, the adapter on the policy
    /// at the model's control period (`controller_binding::scene_with`).
    pub scene: Scene,
    /// The embedded config with the horizon, report stride and CAD hash check derived here.
    pub config: Config,
    pub profile: DriveProfile,
    pub resolved: ResolvedDrive,
    pub identity: EmbeddedIdentity,
    /// The fidelity and compatibility label hosts show verbatim.
    pub fidelity: String,
}

// By hand: `Config` has no Debug. The identity names everything a drive is built from.
impl std::fmt::Debug for EmbeddedDrive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddedDrive").field("schema", &self.schema).field("identity", &self.identity).field("fidelity", &self.fidelity).finish_non_exhaustive()
    }
}

/// A path as the binding writes it, the key of [`build`]'s `files`.
fn key_of(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The binding's embedded program, refused naming the field when absent.
fn embedded_of<'a>(binding: &'a ControllerBinding, binding_path: &Path) -> Result<&'a EmbeddedController, String> {
    binding.embedded.as_ref().ok_or_else(|| {
        format!(
            "{}: embedded: absent; this binding names only an external controller ({} {}), which a host without processes (the browser) cannot start. Add \"embedded\": {{\"language\": \"rhai\", \"entry\": …, \"config\": …}}",
            binding_path.display(),
            binding.controller.language,
            binding.controller.script.display()
        )
    })
}

/// The files a binding's embedded program needs, relative to the binding
/// (the strings as the binding writes them): the drive profile, the entry,
/// its further files and the embedded config, in that order. Refused naming
/// the field when the binding has no `embedded` program.
pub fn files_to_read(binding: &ControllerBinding, binding_path: &Path) -> Result<Vec<String>, String> {
    let embedded = embedded_of(binding, binding_path)?;
    let mut out = vec![key_of(&binding.drive_profile), key_of(&embedded.entry)];
    out.extend(embedded.files.iter().map(|f| key_of(f)));
    out.push(key_of(&embedded.config));
    Ok(out)
}

/// sha256 over captured sources: for each (key, text) in key order,
/// `key`, a NUL byte, `text`, a NUL byte.
pub fn sources_sha256(sources: &sim_script::Sources) -> String {
    let mut stream = Vec::new();
    for (key, text) in &sources.files {
        stream.extend_from_slice(key.as_bytes());
        stream.push(0);
        stream.extend_from_slice(text.as_bytes());
        stream.push(0);
    }
    sha256_hex(&stream)
}

/// Nominal physics steps per control period: `period_s / step_s`, which
/// must be a whole number (the sampled policy's own rule, embedded_policy.rs).
fn stride_of(step_s: f64, period_s: f64) -> Option<usize> {
    let n = period_s / step_s;
    (step_s.is_finite() && step_s > 0.0 && period_s.is_finite() && period_s > 0.0 && n.is_finite() && (1.0..=1e9).contains(&n) && (n - n.round()).abs() <= 1e-8)
        .then(|| n.round() as usize)
}

/// The largest wheel joint rate magnitude (rad/s) the profile allows: the
/// drive's own mixer at every corner of the speed envelope (± max speed on
/// each supported axis). The mixers are linear, so a corner holds the maximum.
fn max_wheel_rate(resolved: &ResolvedDrive) -> Result<f64, String> {
    let limits = &resolved.limits;
    let mut max = 0.0_f64;
    for corner in 0..8u32 {
        let twist: [f64; 3] = std::array::from_fn(|i| match (limits.supported[i], corner & (1 << i) != 0) {
            (false, _) => 0.0,
            (true, true) => limits.max_speed[i],
            (true, false) => -limits.max_speed[i],
        });
        let twist = BodyTwist::from_array(twist);
        let rates: Vec<f64> = if resolved.kinematics == "mecanum" {
            resolved.geometry.mecanum()?.mix(twist).map_err(|e| e.to_string())?.to_vec()
        } else {
            resolved.geometry.differential()?.mix(twist).map_err(|e| e.to_string())?.to_vec()
        };
        for rate in rates {
            max = max.max(rate.abs());
        }
    }
    Ok(max)
}

/// The text of a file the binding names, from [`build`]'s `files`.
fn supplied<'a>(files: &'a BTreeMap<String, String>, binding: &str, path: &Path, field: &str) -> Result<&'a String, String> {
    let key = key_of(path);
    files.get(&key).ok_or_else(|| format!("{binding}: {field}: `{key}` was not supplied (files_to_read lists every file the binding names)"))
}

/// The one scene builder (no I/O). `files` holds the text of every path
/// [`files_to_read`] lists, keyed by that string. Errors name the file and field.
///
/// Derived here, never taken from the config file: `steps` (whole control
/// periods covering `duration_s`, so the horizon is a multiple of the report
/// stride), `report_every` (one control period) and
/// `motors.expected_cad_sha256` (the model's `source.cad_sha256` when the
/// config omits it). Checked against the model: the policy's target bounds
/// cover a whole session at the largest wheel rate (`max_wheel_rate` ×
/// `duration_s`), and each servo's supply voltage and winding temperature
/// equal the model's `motors[i].electrical.supply_voltage` and ambient
/// temperature (`motors[i].thermal.ambient_c`, else `world.ambient_c`).
/// The config's `steps` and `report_every` are placeholders this
/// overwrites. The scene's build options are the drive benchmark's
/// (`prepare_drive_benchmark.mjs`: contact on, flex off): the embedded
/// session integrates rigid links with the CAD servos at `step_s`, which
/// the default flexible-link build is not set up for here.
pub fn build(model_text: &str, model_path: &str, binding_path: &Path, binding_text: &str, files: &BTreeMap<String, String>, duration_s: f64) -> Result<EmbeddedDrive, String> {
    let shown = binding_path.display().to_string();
    let binding = ControllerBinding::from_json(binding_text, binding_path)?;
    let embedded = embedded_of(&binding, binding_path)?;
    let dir = binding_path.parent().unwrap_or(Path::new(""));
    let text = |path: &Path, field: &str| supplied(files, &shown, path, field);
    if !(duration_s.is_finite() && duration_s > 0.0) {
        return Err(format!("{shown}: the drive's duration {duration_s} s must be positive and finite"));
    }

    // The model and its drive profile, resolved (geometry derived from CAD, with provenance).
    let model = PhysicalModel::parse(model_text).map_err(|e| format!("{model_path}: {e}"))?;
    let model_document: Value = serde_json::from_str(model_text).map_err(|e| format!("{model_path}: {e}"))?;
    let profile_text = text(&binding.drive_profile, "drive_profile")?;
    let profile_path = dir.join(&binding.drive_profile);
    let profile = DriveProfile::from_json(profile_text, &profile_path).map_err(|e| e.to_string())?;
    let profile_sha256 = sha256_hex(profile_text.as_bytes());
    let resolved = controller_binding::resolve_drive(&model, &profile, &profile_path, &profile_sha256)?;
    let wheels: Vec<String> = resolved.geometry.joints().into_iter().map(str::to_owned).collect();

    // The adapter's sources: the entry by its file name, further files by their path relative to the entry's directory.
    let entry_name = embedded
        .entry
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{shown}: embedded.entry: `{}` names no file", key_of(&embedded.entry)))?;
    let entry_dir = embedded.entry.parent().unwrap_or(Path::new(""));
    let mut sources = sim_script::Sources { entry: entry_name.clone(), files: BTreeMap::new() };
    sources.files.insert(entry_name.clone(), text(&embedded.entry, "embedded.entry")?.clone());
    for (i, file) in embedded.files.iter().enumerate() {
        let field = format!("embedded.files[{i}]");
        let inside = file
            .strip_prefix(entry_dir)
            .ok()
            .filter(|rest| !rest.as_os_str().is_empty() && rest.components().all(|c| matches!(c, Component::Normal(_))))
            .ok_or_else(|| format!(
                "{shown}: {field}: `{}` is not inside the entry's directory `{}`; further files are captured by their path relative to the entry",
                key_of(file),
                key_of(entry_dir)
            ))?;
        let key = key_of(inside);
        if sources.files.contains_key(&key) {
            return Err(format!("{shown}: {field}: `{}` is captured twice (as `{key}`)", key_of(file)));
        }
        sources.files.insert(key, text(file, &field)?.clone());
    }

    // The embedded config, with what this builder derives.
    let config_text = text(&embedded.config, "embedded.config")?;
    let config_path = dir.join(&embedded.config);
    let config_shown = config_path.display().to_string();
    let mut config: Config = serde_json::from_str(config_text).map_err(|e| format!("{config_shown}: {e}"))?;
    let period_s = model.control.period_s;
    let stride = stride_of(config.step_s, period_s).ok_or_else(|| format!(
        "{config_shown}: step_s: {} s does not divide the model's control period {period_s} s ({model_path} control.period_s) into a whole number of steps",
        config.step_s
    ))?;
    let periods = (duration_s / period_s).round().max(1.0);
    if periods * stride as f64 > crate::embedded::MAX_EXACT_CLOCK_STEPS as f64 {
        return Err(format!("{config_shown}: step_s: {duration_s} s at {} s steps exceeds the session's exact clock", config.step_s));
    }
    config.steps = periods as usize * stride;
    config.report_every = stride;
    let cad_sha256 = model.source.get("cad_sha256").and_then(Value::as_str).map(str::to_owned);
    let motors = config.motors.as_mut().ok_or_else(|| format!("{config_shown}: motors: absent; the adapter's wheel targets need the CAD servos (motors.servos)"))?;
    match (motors.expected_cad_sha256.clone(), &cad_sha256) {
        (None, Some(cad)) => motors.expected_cad_sha256 = Some(cad.clone()),
        (Some(expected), Some(cad)) if expected != *cad => {
            return Err(format!("{config_shown}: motors.expected_cad_sha256: {expected} is not the model's source.cad_sha256 {cad} ({model_path})"));
        }
        (Some(expected), None) => {
            return Err(format!("{config_shown}: motors.expected_cad_sha256: {expected}, but {model_path} has no source.cad_sha256 to check it against"));
        }
        _ => {}
    }
    let servos = motors.servos.as_ref().ok_or_else(|| format!("{config_shown}: motors.servos: absent; the adapter commands the CAD servos' position targets"))?;
    let policy = config.policy.as_ref().ok_or_else(|| format!("{config_shown}: policy: absent; the adapter runs as the embedded session's sampled policy"))?;

    // The policy commands `<joint>.target` for each CAD motor's joint
    // (SampledPolicy, embedded_policy.rs); the adapter writes one per wheel.
    let motor_joints: Vec<String> = model
        .motors
        .iter()
        .enumerate()
        .map(|(i, m)| m.joint.clone().ok_or_else(|| format!("{model_path}: motors[{i}] ({}): declares no joint", m.name)))
        .collect::<Result<_, _>>()?;
    fn sorted(names: &[String]) -> Vec<String> {
        let mut v = names.to_vec();
        v.sort();
        v
    }
    if sorted(&motor_joints) != sorted(&wheels) {
        return Err(format!(
            "{config_shown}: the embedded policy commands the model's motor joints {motor_joints:?} ({model_path} motors[].joint), but the drive geometry's wheels are {wheels:?} ({}); they must be the same joints",
            profile_path.display()
        ));
    }
    if servos.len() != motor_joints.len() {
        return Err(format!("{config_shown}: motors.servos: lists {} servos; the model has {} motors {motor_joints:?}", servos.len(), motor_joints.len()));
    }
    let bounded: Vec<String> = policy.target_bounds_rad.keys().map(|k| k.strip_prefix("joint.").unwrap_or(k).to_owned()).collect();
    if sorted(&bounded) != sorted(&wheels) {
        return Err(format!("{config_shown}: policy.target_bounds_rad: bounds {:?}; the drive's wheel joints are {wheels:?}", policy.target_bounds_rad.keys().collect::<Vec<_>>()));
    }
    if let Some(coordinates) = &motors.target_coordinates {
        let named: Vec<String> = coordinates.iter().map(|c| c.strip_prefix("joint.").unwrap_or(c).to_owned()).collect();
        if named != motor_joints {
            return Err(format!("{config_shown}: motors.target_coordinates: {coordinates:?} are not the model's motor joints in motor order {motor_joints:?}"));
        }
    }
    // The software target envelope must hold a whole session at full speed:
    // SampledPolicy refuses a target outside it, which would end the drive.
    let max_rate = max_wheel_rate(&resolved).map_err(|e| format!("{}: {e}", profile_path.display()))?;
    let travel = max_rate * duration_s;
    for (joint, servo) in motor_joints.iter().zip(servos) {
        let (key, [lo, hi]) = policy
            .target_bounds_rad
            .iter()
            .find(|(k, _)| k.strip_prefix("joint.").unwrap_or(k) == joint.as_str())
            .map(|(k, b)| (k.clone(), *b))
            .ok_or_else(|| format!("{config_shown}: policy.target_bounds_rad: no bound for the wheel joint `{joint}`"))?;
        if lo > servo.target_rad - travel || hi < servo.target_rad + travel {
            return Err(format!(
                "{config_shown}: policy.target_bounds_rad.{key}: [{lo}, {hi}] rad is narrower than the session needs: initial target {} ± {travel:.1} rad                  (the largest wheel joint rate {max_rate:.3} rad/s, from the profile's max speeds through the drive's mixer at every corner of the speed envelope, × the {duration_s} s session)",
                servo.target_rad
            ));
        }
    }
    // The servo boundaries are imposed constants: each must be the CAD
    // value it stands for, read from the model document (a field the
    // parser would default is not a source). A value the model does not
    // state stays an imposed boundary, said so in the fidelity label.
    let mut imposed: Vec<String> = Vec::new();
    for (i, servo) in servos.iter().enumerate() {
        let motor = model_document.pointer(&format!("/motors/{i}"));
        let name = &model.motors[i].name;
        match motor.and_then(|m| m.pointer("/electrical/supply_voltage")).and_then(Value::as_f64) {
            Some(v) if (v - servo.supply_voltage_v).abs() > 1e-9 => {
                return Err(format!(
                    "{config_shown}: motors.servos[{i}].supply_voltage_v: {} V, but {model_path} motors[{i}] ({name}).electrical.supply_voltage is {v} V",
                    servo.supply_voltage_v
                ));
            }
            Some(_) => {}
            None => imposed.push(format!("motors.servos[{i}].supply_voltage_v {} V ({name}: no electrical.supply_voltage in the model)", servo.supply_voltage_v)),
        }
        let ambient = motor
            .and_then(|m| m.pointer("/thermal/ambient_c"))
            .and_then(Value::as_f64)
            .map(|c| (c, format!("motors[{i}] ({name}).thermal.ambient_c")))
            .or_else(|| model_document.pointer("/world/ambient_c").and_then(Value::as_f64).map(|c| (c, "world.ambient_c".to_owned())));
        match ambient {
            Some((c, source)) if (c + 273.15 - servo.winding_temperature_k).abs() > 1e-9 => {
                return Err(format!(
                    "{config_shown}: motors.servos[{i}].winding_temperature_k: {} K, but {model_path} {source} is {c} °C ({} K)",
                    servo.winding_temperature_k,
                    c + 273.15
                ));
            }
            Some(_) => {}
            None => imposed.push(format!("motors.servos[{i}].winding_temperature_k {} K ({name}: no ambient temperature in the model)", servo.winding_temperature_k)),
        }
    }
    let boundaries = if imposed.is_empty() {
        "servo supply voltage and winding temperature imposed at the model's values (no battery or thermal network)".to_owned()
    } else {
        format!("servo boundaries imposed by the config, not stated in the model: {}", imposed.join("; "))
    };
    let initial_targets: serde_json::Map<String, Value> = motor_joints.iter().zip(servos).map(|(joint, servo)| (joint.clone(), Value::from(servo.target_rad))).collect();

    let identity = EmbeddedIdentity {
        binding: shown.clone(),
        entry: entry_name.clone(),
        script_sha256: sources_sha256(&sources),
        config_sha256: sha256_hex(config_text.as_bytes()),
        profile: profile_path.display().to_string(),
        profile_sha256,
        model_sha256: sha256_hex(model_text.as_bytes()),
        cad_sha256,
    };
    let mut parameters = serde_json::Map::new();
    parameters.insert("period_s".into(), Value::from(period_s));
    parameters.insert(DRIVE_PARAMETER.into(), serde_json::to_value(&resolved).map_err(|e| format!("{}: cannot encode the resolved drive: {e}", profile_path.display()))?);
    parameters.insert("initial_targets".into(), Value::Object(initial_targets));
    parameters.insert(IDENTITY_PARAMETER.into(), serde_json::to_value(&identity).map_err(|e| format!("{shown}: cannot encode the identity: {e}"))?);
    let program = ControllerProgram { sources, parameters: Value::Object(parameters), inputs: controller_binding::drive_inputs(&resolved), external: None };
    program.validate().map_err(|e| format!("{shown}: embedded: {e}"))?;

    let fidelity = format!(
        "Browser compatibility path: the binding's embedded Rhai adapter ({entry_name}) mixing through the shared Rust drive functions, in the shared embedded session \
         (servo boundaries from {config_shown}: {boundaries}; contact on, flex off; {} s physics steps, {stride} per {period_s} s control period). \
         Limits and deadman: kinematics::step on simulation time (the shared TwistState). Reference controller: the external {} {} on the native seam. \
         Uncalibrated; realtime not measured.",
        config.step_s,
        binding.controller.language,
        binding.controller.script.display()
    );
    let robot_input = crate::robot_input::RobotInput::receive(model_document, None, &model).map_err(|e| format!("{model_path}: {e}"))?;
    let options = BuildOptions { contact: true, flex: false, ..BuildOptions::default() };
    let scene = controller_binding::scene_with(model, program, duration_s, options, Some(robot_input));
    Ok(EmbeddedDrive { schema: EMBEDDED_DRIVE_SCHEMA.into(), scene, config, profile, resolved, identity, fidelity })
}

/// Read the binding beside `model_path` and the files it names, then [`build`].
#[cfg(not(target_arch = "wasm32"))]
pub fn load(model_path: &Path, duration_s: f64) -> Result<EmbeddedDrive, String> {
    let binding_path = controller_binding::binding_path_for(model_path);
    let shown = binding_path.display();
    let binding_text = std::fs::read_to_string(&binding_path).map_err(|e| format!("{shown}: cannot read the controller binding: {e}"))?;
    let binding = ControllerBinding::from_json(&binding_text, &binding_path)?;
    let dir = binding_path.parent().unwrap_or(Path::new(""));
    let mut files = BTreeMap::new();
    for relative in files_to_read(&binding, &binding_path)? {
        let path = dir.join(&relative);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{shown}: `{relative}`: cannot read {}: {e}", path.display()))?;
        files.insert(relative, text);
    }
    let model_text = std::fs::read_to_string(model_path).map_err(|e| format!("{}: {e}", model_path.display()))?;
    build(&model_text, &model_path.display().to_string(), &binding_path, &binding_text, &files, duration_s)
}

/// A request interpreted against a built drive, without a session: the
/// twist it asks for (with units) and whether it is a halt
/// (`DriveRequest::interpret_with`). `drive_json` is the [`EmbeddedDrive`]
/// JSON; only its `profile` and `resolved` are read, so a host may pass just
/// those two. `request_json` is [`DriveRequest`]'s serde form
/// (`{"axes": {"forward": 1, "lateral": 0, "yaw": 0}}`,
/// `{"action": {"name": "stop"}}`, `"stop"`).
pub fn interpret_json(drive_json: &str, request_json: &str) -> Result<Value, String> {
    // Unknown fields (the scene, the config, …) are skipped, not parsed.
    #[derive(Deserialize)]
    struct Parts {
        profile: DriveProfile,
        resolved: ResolvedDrive,
    }
    let parts: Parts = serde_json::from_str(drive_json).map_err(|e| format!("embedded drive: {e}"))?;
    let request: DriveRequest = serde_json::from_str(request_json).map_err(|e| format!("drive request: {e}"))?;
    let (twist, halt) = request.interpret_with(&parts.profile, &parts.resolved)?;
    Ok(json!({"twist": twist_json(twist), "halt": halt}))
}

/// The driven embedded session.
pub struct DriveSession {
    session: EmbeddedSession,
    drive: EmbeddedDrive,
    twist: TwistState,
    limits: Limits,
    deadman: Deadman,
    /// Nominal physics steps per control period.
    stride: usize,
    /// Steps a replay still has to run (None: live).
    replay_steps: Option<usize>,
}

impl DriveSession {
    /// The embedded session (`CaptureMode::Latest`) for `drive`, after
    /// checking the drive is one [`build`] made: its schema, its resolved
    /// drive, and the scene's parameters carrying that drive and identity.
    pub fn new(drive: EmbeddedDrive, seed: u64) -> Result<Self, String> {
        if drive.schema != EMBEDDED_DRIVE_SCHEMA {
            return Err(format!("embedded drive schema is `{}`; this build reads {EMBEDDED_DRIVE_SCHEMA}", drive.schema));
        }
        drive.resolved.validate()?;
        let parameters = drive.scene.controller.as_ref().map(|c| &c.parameters).ok_or("embedded drive: the scene has no controller program")?;
        let encoded = |value: Result<Value, serde_json::Error>| value.map_err(|e| format!("embedded drive: {e}"));
        if parameters.get(DRIVE_PARAMETER) != Some(&encoded(serde_json::to_value(&drive.resolved))?)
            || parameters.get(IDENTITY_PARAMETER) != Some(&encoded(serde_json::to_value(&drive.identity))?)
        {
            return Err(format!("embedded drive: the scene's controller parameters `{DRIVE_PARAMETER}` and `{IDENTITY_PARAMETER}` are not the drive's resolved profile and identity; rebuild it (embedded_drive::build)"));
        }
        let stride = stride_of(drive.config.step_s, drive.scene.period_s)
            .ok_or_else(|| format!("embedded drive: step_s {} s does not divide the control period {} s", drive.config.step_s, drive.scene.period_s))?;
        let session = EmbeddedSession::new(drive.scene.clone(), drive.config.clone(), seed, CaptureMode::Latest)?;
        check_inputs(session.inputs())?;
        let (limits, deadman) = (drive.resolved.limits(), drive.resolved.deadman());
        Ok(Self { session, drive, twist: TwistState::default(), limits, deadman, stride, replay_steps: None })
    }
    /// A fresh request at the current simulation time: interpreted against
    /// the profile, then `TwistState::request` (heartbeat + 1). Refused while replaying.
    pub fn request(&mut self, request: &DriveRequest) -> Result<DriveStatus, String> {
        if self.replaying() {
            return Err("replaying a recording; requests resume after it ends".into());
        }
        self.check_running()?;
        let (twist, halt) = request.interpret_with(&self.drive.profile, &self.drive.resolved)?;
        let now = self.time();
        self.twist.request(twist, halt, now, &self.limits)?;
        Ok(self.status())
    }
    /// Up to `periods` control periods (live: limiter and deadman, set_inputs, step; replay: step, `TwistState::replayed`).
    /// Called at the horizon it is refused (`the drive's 600 s horizon is
    /// reached; …`); reaching it during the call stops there with `Ok`. A
    /// solver failure latches in the session (its frame carries it) and is
    /// returned; a failed replay ends the replay ([`TwistState::replay_ended`]).
    pub fn advance(&mut self, periods: usize) -> Result<(), String> {
        if periods == 0 || periods > MAX_ADVANCE_PERIODS {
            return Err(format!("advance 1..{MAX_ADVANCE_PERIODS} control periods; got {periods}"));
        }
        let period_s = self.drive.scene.period_s;
        for ran in 0..periods {
            if let Some(error) = self.session.error() {
                return Err(error.to_owned());
            }
            if self.session.remaining_steps() == 0 {
                if self.replay_steps.is_some() {
                    self.end_replay();
                }
                if ran == 0 {
                    return Err(self.horizon_reached());
                }
                break;
            }
            match self.replay_steps {
                None => {
                    // As DriveHost::step: computed on a copy, committed only once the period ran.
                    let now = self.time();
                    let mut next = self.twist;
                    let sent = next.advance(now, period_s, &self.limits, &self.deadman)?;
                    self.session.set_inputs(&sent)?;
                    self.session.advance(self.stride.min(self.session.remaining_steps()))?;
                    self.twist = next;
                }
                Some(left) => {
                    // The recorded input in effect for this period: the session
                    // applies a period's events before it (and the next period's
                    // at the end of `advance`, embedded.rs), so read it first.
                    let start = self.time();
                    let action = self.session.input_values().to_vec();
                    let n = self.stride.min(left).min(self.session.remaining_steps());
                    if let Err(e) = self.session.advance(n) {
                        self.end_replay();
                        return Err(e);
                    }
                    self.twist.replayed(&action, start);
                    let left = left - n;
                    if left == 0 {
                        self.end_replay();
                    } else {
                        self.replay_steps = Some(left);
                    }
                }
            }
        }
        Ok(())
    }
    /// Refused, by name, once the session latched a failure or reached its horizon.
    fn check_running(&self) -> Result<(), String> {
        if let Some(error) = self.session.error() {
            return Err(format!("the drive session failed: {error}; reload to start a new session"));
        }
        if self.session.remaining_steps() == 0 {
            return Err(self.horizon_reached());
        }
        Ok(())
    }
    fn horizon_reached(&self) -> String {
        format!("the drive's {} s horizon is reached; reload to start a new session", self.drive.scene.duration_s)
    }
    fn end_replay(&mut self) {
        self.replay_steps = None;
        let now = self.time();
        self.twist.replay_ended(now, &self.deadman);
    }
    /// Simulation time (s): committed physics steps × step_s.
    pub fn time(&self) -> f64 {
        self.session.completed_steps() as f64 * self.session.config().step_s
    }
    pub fn status(&self) -> DriveStatus {
        self.twist.status(self.time())
    }
    pub fn drive(&self) -> &EmbeddedDrive {
        &self.drive
    }
    pub fn session(&self) -> &EmbeddedSession {
        &self.session
    }
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    pub fn deadman(&self) -> &Deadman {
        &self.deadman
    }
    /// Nominal physics steps per control period.
    pub fn stride(&self) -> usize {
        self.stride
    }
    pub fn replaying(&self) -> bool {
        self.replay_steps.is_some()
    }
    /// The embedded session's recording (the twists and heartbeats are its input events).
    pub fn recording(&self) -> EmbeddedRecording {
        self.session.recording()
    }
    /// The session's interactive frame (`EmbeddedSession::interactive_frame`:
    /// state, servo targets, `done`, `error`, policy inputs) with the drive
    /// status (`drive`, `DriveStatus::json`), `replaying` and `time_s`.
    pub fn frame(&self) -> Result<Value, String> {
        let mut frame = self.session.interactive_frame()?;
        frame["drive"] = self.status().json();
        frame["replaying"] = Value::Bool(self.replaying());
        frame["time_s"] = Value::from(self.time());
        Ok(frame)
    }
    /// What a host shows and checks about the drive: the session's
    /// coordinates, the step, period and stride, the command inputs, the
    /// profile's limits (with units) and deadman, its named actions, the
    /// geometry with provenance, the identity and the fidelity label.
    pub fn metadata(&self) -> Value {
        let (s, d) = (&self.session, &self.drive);
        let r = &d.resolved;
        json!({
            "coordinate_names": s.coordinate_names(),
            "joint_indices": s.joint_indices(),
            "step_s": s.config().step_s,
            "steps": s.config().steps,
            "period_s": d.scene.period_s,
            "stride": self.stride,
            "inputs": s.inputs(),
            "kinematics": r.kinematics,
            "limits": {
                "axes": AXIS_NAMES,
                "supported": r.limits.supported,
                "max_speed": r.limits.max_speed,
                "max_accel": r.limits.max_accel,
                "stop_decel": r.limits.stop_decel,
                "speed_units": SPEED_UNITS,
                "accel_units": ACCEL_UNITS
            },
            "deadman": {"timeout_s": r.deadman.timeout_s, "timeout_unit": "s", "on_loss": r.deadman.on_loss},
            "actions": d.profile.actions,
            "geometry": r.geometry,
            "profile": r.profile,
            "identity": d.identity,
            "fidelity": d.fidelity,
            "policy_contract": s.policy_metadata()
        })
    }
    /// Check the recording's identity against the loaded drive (refused
    /// naming every differing field), then `EmbeddedSession::prepare_replay`.
    /// Returns the periods to advance.
    pub fn prepare_replay(&mut self, recording: EmbeddedRecording) -> Result<usize, String> {
        let recorded = recording
            .scene
            .controller
            .as_ref()
            .and_then(|c| c.parameters.get(IDENTITY_PARAMETER))
            .ok_or_else(|| format!("not an embedded drive recording: its scene's controller has no `{IDENTITY_PARAMETER}` parameter"))?;
        let recorded: EmbeddedIdentity = serde_json::from_value(recorded.clone()).map_err(|e| format!("not an embedded drive recording: {IDENTITY_PARAMETER}: {e}"))?;
        let differences = recorded.differences(&self.drive.identity);
        if !differences.is_empty() {
            return Err(format!("the recording was made with a different drive; replay refused: {}", differences.join("; ")));
        }
        let encode = |value: Result<Value, serde_json::Error>| value.map_err(|e| format!("replay: {e}"));
        // The paths are recorded information (EmbeddedIdentity::differences): set aside before comparing.
        let without_paths = |mut scene: Value| {
            for pointer in ["/controller/parameters/identity/binding", "/controller/parameters/identity/profile", "/controller/parameters/drive/profile"] {
                if let Some(v) = scene.pointer_mut(pointer) {
                    *v = Value::Null;
                }
            }
            scene
        };
        if without_paths(encode(serde_json::to_value(&recording.scene))?) != without_paths(encode(serde_json::to_value(self.session.scene()))?)
            || encode(serde_json::to_value(&recording.config))? != encode(serde_json::to_value(self.session.config()))?
        {
            return Err("replay must match the loaded drive's scene and embedded config; rebuild the drive from the files it was recorded with".into());
        }
        let (session, steps) = EmbeddedSession::prepare_replay(recording, CaptureMode::Latest)?;
        self.session = session;
        self.twist = TwistState::default();
        if steps == 0 {
            self.end_replay();
            return Ok(0);
        }
        self.replay_steps = Some(steps);
        Ok(steps.div_ceil(self.stride))
    }
}

#[cfg(test)]
mod tests;
