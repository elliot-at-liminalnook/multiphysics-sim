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
        let profiles = model.actuator_profiles.as_ref().ok_or("model has no actuator profiles")?;
        for (id, binding) in &profiles.bindings {
            let motor = model.motors.iter().find(|m| &m.id == id).ok_or(format!("binding {id} has no motor"))?;
            let joint = motor.joint.as_deref().unwrap_or_default();
            let want = self.role_family(joint)?;
            let have = profiles.families.get(&binding.family).ok_or(format!("binding {id} names a missing family"))?;
            if binding.family != want || have.content_hash() != self.families[want].content_hash() {
                return Err(format!("motor {id} ({joint}) uses {} ({}) but the registry accepts {want} ({}); apply the registry", binding.family, have.content_hash(), self.families[want].content_hash()));
            }
        }
        Ok(())
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
