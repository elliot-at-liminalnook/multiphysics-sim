//! Accepted actuator families: the single source of motor values.
//!
//! A registry file names each accepted family (a CAD actuator-profile family
//! JSON) with its content hash, and which family serves each joint role
//! (matched by the joint-name suffix, e.g. "Foot servo output"). Every system
//! that needs motor values (CAD exports, simulation scenes, gait-search
//! recipes and screens, viewers) applies or checks the registry instead of
//! copying numbers, and records the family hashes it used. A consumer holding
//! a family whose hash differs from the registry's is stale: `check` fails.
//!
//! Operating limits for motion screens are derived from the resolved motor
//! parameters at the simulation's own supply voltage, so the screen and the
//! physics agree by construction; a family's measured envelope supplies the
//! acceleration limit and is reported alongside.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_robot::{PhysicalModel, actuator_profile::{Binding, Family, Profiles}};
use std::{collections::BTreeMap, path::{Path, PathBuf}};

type R<T> = Result<T, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Family JSON, relative to the registry file.
    pub path: String,
    /// `Family::content_hash` of that file's contents.
    pub content_hash: String,
    /// Why this family is accepted for use (campaign, scope, date).
    pub accepted: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryFile {
    pub version: u32,
    pub description: String,
    pub families: BTreeMap<String, Entry>,
    /// Joint-name suffix → family name. Every profiled motor's joint must match one.
    pub roles: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub struct Registry {
    pub path: PathBuf,
    pub file: RegistryFile,
    pub families: BTreeMap<String, Family>,
    /// BLAKE3 of the registry file itself.
    pub registry_hash: String,
}

/// Limits for one joint, derived from its resolved profile.
#[derive(Clone, Debug, Serialize)]
pub struct JointLimit {
    pub joint: String,
    pub family: String,
    pub family_hash: String,
    pub supply_voltage_v: f64,
    /// V/(Ke·N): output speed with no friction or load (rad/s).
    pub no_load_speed_rad_s: f64,
    /// Output speed at full drive after the modelled friction (rad/s).
    pub full_drive_speed_rad_s: f64,
    /// N·η·(Kt·(V/R − I0)) − gear friction (N·m).
    pub stall_torque_nm: f64,
    /// Measured acceleration envelope, when the family has one (rad/s²).
    pub measured_acceleration_rad_s2: Option<f64>,
    /// Measured full-drive speed at the envelope's own supply voltage.
    pub measured_full_drive_speed_rad_s: Option<(f64, f64)>,
    pub source: String,
}

/// Why a consumer model fails `Registry::check`. Fields the check did not
/// reach are `None` (e.g. no hash when the binding names a missing family).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Mismatch {
    pub motor: Option<String>,
    pub joint: Option<String>,
    /// Family the consumer's binding uses.
    pub family: Option<String>,
    /// Family the registry accepts for the joint's role.
    pub accepted_family: Option<String>,
    /// Content hash of the family the consumer holds.
    pub have_hash: Option<String>,
    pub accepted_hash: Option<String>,
    /// Exactly the `Registry::check` error.
    pub message: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    /// Every binding uses the accepted family and hash.
    Current,
    /// A binding uses another family or hash than the registry accepts.
    Stale,
    /// The model does not deserialize, or cannot be matched to the registry.
    Invalid,
}
/// One embedded robot model of a consumer file.
#[derive(Clone, Debug, Serialize)]
pub struct ModelCheck {
    /// JSON pointer of the model in the file ("" for a bare model).
    pub pointer: String,
    pub status: ModelStatus,
    pub message: Option<String>,
    pub mismatch: Option<Mismatch>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileIssueKind {
    Read,
    Parse,
    /// No supported pointer holds a model with motors and actuator profiles.
    NoRobot,
}
#[derive(Clone, Debug, Serialize)]
pub struct FileIssue {
    pub kind: FileIssueKind,
    /// Names the file.
    pub message: String,
}
/// Staleness of one consumer file against the registry.
#[derive(Clone, Debug, Serialize)]
pub struct ConsumerCheck {
    pub file: PathBuf,
    pub issue: Option<FileIssue>,
    pub models: Vec<ModelCheck>,
}
impl ConsumerCheck {
    /// The file has robot models and all of them are current.
    pub fn is_current(&self) -> bool {
        self.issue.is_none() && self.models.iter().all(|m| m.status == ModelStatus::Current)
    }
}

/// Where supported documents embed robot models, in search order: a bare
/// model, a scene, a gait-comparison config's recipe, and a recipe.
pub const ROBOT_POINTERS: [&str; 7] = ["", "/robot", "/scene/robot", "/recipe/experiment/scene/robot", "/recipe/planning_scene/robot", "/experiment/scene/robot", "/planning_scene/robot"];
/// JSON pointers of the robot models (objects with `motors` and
/// `actuator_profiles`) inside a consumer document.
pub fn robot_pointers(doc: &Value) -> Vec<&'static str> {
    ROBOT_POINTERS.into_iter().filter(|p| doc.pointer(p).is_some_and(|r| r.get("motors").is_some() && r.get("actuator_profiles").is_some())).collect()
}

/// The repository's accepted registry: `SIM_ACTUATOR_REGISTRY`, else
/// `examples/actuators/hx30hm/accepted/registry.json` found upwards from
/// the current directory.
pub fn default_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("SIM_ACTUATOR_REGISTRY") {
        return p.into();
    }
    const RELATIVE: &str = "examples/actuators/hx30hm/accepted/registry.json";
    let mut dir = std::env::current_dir().unwrap_or_default();
    loop {
        if dir.join(RELATIVE).exists() {
            return dir.join(RELATIVE);
        }
        if !dir.pop() {
            return RELATIVE.into();
        }
    }
}

impl Registry {
    pub fn load(path: &Path) -> R<Self> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file: RegistryFile = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        if file.version != 1 || file.families.is_empty() || file.roles.is_empty() {
            return Err("actuator registry needs version 1, families and roles".into());
        }
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut families = BTreeMap::new();
        for (name, entry) in &file.families {
            let family_path = dir.join(&entry.path);
            let family: Family = serde_json::from_slice(&std::fs::read(&family_path).map_err(|e| format!("{}: {e}", family_path.display()))?)
                .map_err(|e| format!("{}: {e}", family_path.display()))?;
            let hash = family.content_hash();
            if hash != entry.content_hash {
                return Err(format!("actuator family {name} ({}) has content hash {hash}, registry accepts {}; re-accept it explicitly", family_path.display(), entry.content_hash));
            }
            families.insert(name.clone(), family);
        }
        if let Some((role, name)) = file.roles.iter().find(|(_, f)| !families.contains_key(*f)) {
            return Err(format!("role {role} names unknown family {name}"));
        }
        Ok(Self { path: path.to_path_buf(), file, families, registry_hash: blake3::hash(&bytes).to_hex().to_string() })
    }
    /// The accepted family name for a joint (by role suffix).
    pub fn role_family(&self, joint: &str) -> R<&str> {
        let mut hits = self.file.roles.iter().filter(|(suffix, _)| joint.ends_with(suffix.as_str()));
        match (hits.next(), hits.next()) {
            (Some((_, f)), None) => Ok(f),
            (None, _) => Err(format!("joint {joint} matches no actuator role in the registry")),
            _ => Err(format!("joint {joint} matches several actuator roles")),
        }
    }
    /// Bind every profiled motor to its role's accepted family, keeping each
    /// binding's feedback, physical unit and deviations. Families not used are
    /// dropped. Returns motor id → family.
    pub fn apply(&self, model: &mut PhysicalModel) -> R<BTreeMap<String, String>> {
        let profiles = model.actuator_profiles.as_mut().ok_or("model has no actuator profiles to apply the registry to")?;
        let mut used = BTreeMap::new();
        let mut assigned = BTreeMap::new();
        for (id, binding) in profiles.bindings.iter_mut() {
            let motor = model.motors.iter().find(|m| &m.id == id).ok_or(format!("binding {id} has no motor"))?;
            let joint = motor.joint.as_deref().ok_or(format!("motor {id} has no joint"))?;
            let name = self.role_family(joint)?.to_string();
            let family = &self.families[&name];
            *binding = Binding { family: name.clone(), version: family.version, ..binding.clone() };
            used.insert(name.clone(), family.clone());
            assigned.insert(id.clone(), name);
        }
        profiles.families = used;
        Ok(assigned)
    }
    /// Every binding uses exactly the accepted family for its joint role.
    pub fn check(&self, model: &PhysicalModel) -> R<()> {
        match self.mismatch(model) {
            Some(m) => Err(m.message),
            None => Ok(()),
        }
    }
    /// The first reason `check` fails, as data (`None` when the model is current).
    pub fn mismatch(&self, model: &PhysicalModel) -> Option<Mismatch> {
        let fail = |message: String| Some(Mismatch { message, ..Mismatch::default() });
        let Some(profiles) = model.actuator_profiles.as_ref() else { return fail("model has no actuator profiles".into()) };
        for (id, binding) in &profiles.bindings {
            let Some(motor) = model.motors.iter().find(|m| &m.id == id) else { return fail(format!("binding {id} has no motor")) };
            let joint = motor.joint.as_deref().unwrap_or_default();
            let base = Mismatch { motor: Some(id.clone()), joint: Some(joint.to_string()), family: Some(binding.family.clone()), ..Mismatch::default() };
            let want = match self.role_family(joint) {
                Ok(want) => want,
                Err(message) => return Some(Mismatch { message, ..base }),
            };
            let accepted_hash = self.families[want].content_hash();
            let base = Mismatch { accepted_family: Some(want.to_string()), accepted_hash: Some(accepted_hash.clone()), ..base };
            let Some(have) = profiles.families.get(&binding.family) else {
                return Some(Mismatch { message: format!("binding {id} names a missing family"), ..base });
            };
            let have_hash = have.content_hash();
            if binding.family != want || have_hash != accepted_hash {
                let message = format!("motor {id} ({joint}) uses {} ({have_hash}) but the registry accepts {want} ({accepted_hash}); apply the registry", binding.family);
                return Some(Mismatch { have_hash: Some(have_hash), message, ..base });
            }
        }
        None
    }
    /// Check every embedded robot model of one consumer file (scene, model,
    /// gait recipe or study). Unreadable files and missing robots are reported
    /// in the result, never as an error for the whole call.
    pub fn check_consumer(&self, path: &Path) -> ConsumerCheck {
        let file = path.to_path_buf();
        let issue = |kind, message| ConsumerCheck { file: file.clone(), issue: Some(FileIssue { kind, message }), models: Vec::new() };
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return issue(FileIssueKind::Read, format!("{}: {e}", path.display())),
        };
        let doc: Value = match serde_json::from_slice(&bytes) {
            Ok(d) => d,
            Err(e) => return issue(FileIssueKind::Parse, format!("{}: {e}", path.display())),
        };
        let models = self.check_document(&doc);
        if models.is_empty() {
            return issue(FileIssueKind::NoRobot, format!("{}: no robot model with actuator profiles", path.display()));
        }
        ConsumerCheck { file, issue: None, models }
    }
    /// Check each robot model found by `robot_pointers` in a parsed document.
    pub fn check_document(&self, doc: &Value) -> Vec<ModelCheck> {
        robot_pointers(doc)
            .into_iter()
            .map(|pointer| {
                let (status, mismatch, message) = match serde_json::from_value::<PhysicalModel>(doc.pointer(pointer).unwrap().clone()) {
                    Err(e) => (ModelStatus::Invalid, None, Some(e.to_string())),
                    Ok(model) => match self.mismatch(&model) {
                        None => (ModelStatus::Current, None, None),
                        Some(m) => (if m.accepted_family.is_some() { ModelStatus::Stale } else { ModelStatus::Invalid }, Some(m.clone()), Some(m.message)),
                    },
                };
                ModelCheck { pointer: pointer.to_string(), status, message, mismatch }
            })
            .collect()
    }
    /// Provenance record for consumers.
    pub fn identity(&self) -> Value {
        json!({"registry": self.path, "registry_blake3": self.registry_hash,
               "families": self.families.iter().map(|(n, f)| (n.clone(), json!({"content_hash": f.content_hash(), "accepted": self.file.families[n].accepted}))).collect::<BTreeMap<_, _>>(),
               "roles": self.file.roles})
    }
}

/// Resolve `model`'s profiles and derive per-joint limits at each motor's
/// supply voltage (the voltage the simulation drives it with).
pub fn joint_limits(model: &PhysicalModel) -> R<BTreeMap<String, JointLimit>> {
    let mut m = model.clone();
    m.resolve_actuator_profiles(&crate::registry())?;
    let mut out = BTreeMap::new();
    for motor in &m.motors {
        let (Some(r), Some(joint)) = (&motor.resolved_actuator, &motor.joint) else { continue };
        let g = |k: &str| r.motor.get(k).copied().ok_or(format!("{}: resolved motor lacks {k}", joint));
        let (ke, kt, n, eta, res, i0, gf) = (g("back_emf_constant")?, g("torque_constant")?, g("ratio")?, g("efficiency")?, g("resistance")?, g("no_load_current")?, g("gear_friction")?);
        let v = motor.electrical.supply_voltage;
        let extra = motor.gear_ratio.max(1e-9);
        if !(v > 0.) {
            return Err(format!("{joint}: motor supply voltage must be positive"));
        }
        // Friction as a drive fraction: rotor loss plus gear friction referred to current.
        let friction_duty = ((i0 + gf / (n * eta * kt)) * res / v).clamp(0., 0.99);
        let no_load = v / (ke * n) / extra;
        let full = no_load * (1. - friction_duty);
        let stall = (n * eta * kt * (v / res - i0) - gf) * extra;
        let family = &r.family;
        let (measured_acc, measured_speed, source) = match &r.envelope {
            Some(e) => (Some(e.acceleration.value / extra), Some((e.full_drive_speed.value / extra, e.supply_voltage.value)),
                        format!("family {family} (measured envelope; speed derived from its motor parameters at {v} V)")),
            None => (None, None, format!("family {family} (motor parameters at {v} V; no measured envelope)")),
        };
        out.insert(joint.clone(), JointLimit {
            joint: joint.clone(), family: family.clone(), family_hash: r.family_hash.clone(), supply_voltage_v: v,
            no_load_speed_rad_s: no_load, full_drive_speed_rad_s: full, stall_torque_nm: stall,
            measured_acceleration_rad_s2: measured_acc, measured_full_drive_speed_rad_s: measured_speed, source,
        });
    }
    Ok(out)
}

/// Output speed at full drive after friction (rad/s) from a family's motor
/// parameters at `supply_v`, and its measured acceleration envelope if any.
pub fn family_limits(f: &Family, supply_v: f64) -> R<(f64, Option<f64>)> {
    let g = |k: &str| f.motor.get(k).map(|p| p.value).ok_or(format!("family lacks motor.{k}"));
    let (ke, kt, n, eta, res, i0, gf) = (g("back_emf_constant")?, g("torque_constant")?, g("ratio")?, g("efficiency")?, g("resistance")?, g("no_load_current")?, g("gear_friction")?);
    let friction_duty = ((i0 + gf / (n * eta * kt)) * res / supply_v).clamp(0., 0.99);
    Ok((supply_v / (ke * n) * (1. - friction_duty), f.envelope.as_ref().map(|e| e.acceleration.value)))
}

/// Profiles helper: does this declaration carry exactly the registry's families?
pub fn families_of(profiles: &Profiles) -> BTreeMap<String, String> {
    profiles.families.iter().map(|(n, f)| (n.clone(), f.content_hash())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_check_reports_current_stale_and_missing_robots() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let registry = Registry::load(&root.join("examples/actuators/hx30hm/accepted/registry.json")).unwrap();
        // A tracked scene, brought current in memory by applying the registry.
        let mut doc: Value = serde_json::from_slice(&std::fs::read(root.join("examples/full-robot/measured-actuator-integration/browser-control-400hz/scene.json")).unwrap()).unwrap();
        assert_eq!(robot_pointers(&doc), vec!["/robot"]);
        let mut model: PhysicalModel = serde_json::from_value(doc["robot"].clone()).unwrap();
        registry.apply(&mut model).unwrap();
        doc["robot"]["actuator_profiles"] = serde_json::to_value(&model.actuator_profiles).unwrap();
        let dir = std::env::temp_dir().join(format!("actuator-consumer-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let current = dir.join("current.json");
        std::fs::write(&current, serde_json::to_vec(&doc).unwrap()).unwrap();
        let result = registry.check_consumer(&current);
        assert!(result.is_current(), "{result:?}");
        assert_eq!(result.models.len(), 1);

        // Tweak one held family: its hash no longer matches the accepted one.
        let name = model.actuator_profiles.as_ref().unwrap().families.keys().next().unwrap().clone();
        let family = &mut doc["robot"]["actuator_profiles"]["families"][&name];
        let r = family["motor"]["resistance"]["value"].as_f64().unwrap();
        family["motor"]["resistance"]["value"] = json!(r * 1.01);
        let stale = dir.join("stale.json");
        std::fs::write(&stale, serde_json::to_vec(&doc).unwrap()).unwrap();
        let result = registry.check_consumer(&stale);
        assert!(result.issue.is_none() && !result.is_current());
        let m = &result.models[0];
        assert_eq!((m.pointer.as_str(), m.status), ("/robot", ModelStatus::Stale));
        let mismatch = m.mismatch.as_ref().unwrap();
        let (have, accepted) = (mismatch.have_hash.as_deref().unwrap(), mismatch.accepted_hash.as_deref().unwrap());
        assert_ne!(have, accepted);
        assert_eq!(accepted, registry.families[&name].content_hash());
        assert_eq!(mismatch.family.as_deref(), Some(name.as_str()));
        let tweaked: PhysicalModel = serde_json::from_value(doc["robot"].clone()).unwrap();
        assert_eq!(Some(registry.check(&tweaked).unwrap_err()), m.message);
        assert!(m.message.as_deref().unwrap().contains(have) && m.message.as_deref().unwrap().contains(accepted));

        // No embedded robot, and an unreadable file: data naming the file, not an error.
        let plain = dir.join("plain.json");
        std::fs::write(&plain, br#"{"scene": {"bodies": []}}"#).unwrap();
        let result = registry.check_consumer(&plain);
        let issue = result.issue.as_ref().unwrap();
        assert_eq!(issue.kind, FileIssueKind::NoRobot);
        assert!(issue.message.contains("no robot model with actuator profiles") && issue.message.contains("plain.json"));
        let missing = registry.check_consumer(&dir.join("absent.json"));
        assert_eq!(missing.issue.as_ref().unwrap().kind, FileIssueKind::Read);
        serde_json::to_value(&result).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
