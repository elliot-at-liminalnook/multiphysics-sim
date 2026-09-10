//! Inspect authored CAD data before legacy deserialization supplies defaults.
//! This describes the model; it does not infer missing physical measurements.
use crate::PhysicalModel;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Keeps authoring evidence separate from the runtime's resolved legacy model.
/// Construct from the original `simrobot` object, not a reserialized model.
pub struct RobotDocument {
    authored: Value,
    model: PhysicalModel,
}
impl RobotDocument {
    pub fn new(authored: Value) -> Result<Self, String> {
        if !authored.is_object() {
            return Err("CAD robot document must be an object".into());
        }
        let model: PhysicalModel =
            serde_json::from_value(authored.clone()).map_err(|e| e.to_string())?;
        if !matches!(model.version, 3 | 4) {
            return Err("robot contract supports simrobot versions 3 and 4".into());
        }
        Ok(Self { authored, model })
    }
    pub fn model(&self) -> &PhysicalModel {
        &self.model
    }
    pub fn authored(&self) -> &Value {
        &self.authored
    }
    /// Detect experimental model changes before attaching an authoring contract.
    /// Caller must record overrides and inspect their resulting document anew.
    pub fn matches_model(&self, model: &PhysicalModel) -> Result<bool, String> {
        Ok(
            self.model.to_json_value_checked()? == model.to_json_value_checked()?,
        )
    }
    pub fn contract(&self) -> Result<RobotContract, String> {
        inspect(&self.authored, &self.model)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    /// Authored value only. None means absent, even when the runtime has a default.
    pub value: Option<Value>,
    pub unit: String,
    pub frame: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// Category-qualified CAD ID, or an explicitly labelled legacy name fallback.
    pub key: String,
    pub name: String,
    pub cad_id: Option<String>,
    pub category: String,
    pub source_pointer: String,
    pub properties: Vec<Property>,
    /// Original evidence, retained without upgrading estimates to measurements.
    pub evidence: Value,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub owner: String,
    pub role: String,
    /// None denotes the world for an explicitly null joint parent.
    pub target: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContractIssue {
    pub severity: String,
    pub path: String,
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RobotContract {
    pub version: u32,
    pub model_version: u32,
    pub source: Value,
    pub frame_convention: String,
    pub entities: Vec<Entity>,
    pub relations: Vec<Relation>,
    /// JSON pointers whose resolved runtime values were not present in CAD input.
    /// A missing parent object is listed once; its entire subtree is defaulted.
    pub defaulted_fields: Vec<String>,
    /// Input fields not represented by PhysicalModel; never silently discarded here.
    pub unmodeled_fields: Vec<String>,
    pub issues: Vec<ContractIssue>,
    pub materials: Value,
    pub uncertainty: Value,
    pub identification: Value,
}
impl RobotContract {
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.severity == "error")
    }
}

fn pointer_segment(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}
fn missing_fields(expected: &Value, supplied: &Value, path: &str, out: &mut Vec<String>) {
    if let Some(object) = expected.as_object() {
        for (key, value) in object {
            let next = format!("{path}/{}", pointer_segment(key));
            if let Some(other) = supplied.get(key) {
                missing_fields(value, other, &next, out);
            } else {
                out.push(next);
            }
        }
    } else if let (Some(a), Some(b)) = (expected.as_array(), supplied.as_array()) {
        // Arrays of entities contain separately defaulted fields; numeric geometry
        // arrays are values, not schema. Do not expand mesh vertices or SDF cells.
        for (i, (value, other)) in a.iter().zip(b).enumerate() {
            if value.is_object() {
                missing_fields(value, other, &format!("{path}/{i}"), out);
            }
        }
    }
}
fn property(raw: &Value, name: &str, unit: &str, frame: &str) -> Property {
    Property {
        name: name.into(),
        value: raw.pointer(&format!("/{name}")).cloned(),
        unit: unit.into(),
        frame: frame.into(),
    }
}
fn properties(category: &str, raw: &Value) -> Vec<Property> {
    let mut specs = match category {
        "links" => vec![
            ("mass", "kg", "scalar"),
            ("com", "m", "world_at_export"),
            ("inertia", "kg*m^2", "link_com"),
            ("ground", "1", "scalar"),
            ("bbox", "m", "link_com"),
        ],
        "joints" => vec![
            ("origin", "m", "world_at_export"),
            ("axis", "1", "world_at_export"),
            ("physics/clearance", "m", "joint"),
            ("physics/drive_backlash/width_rad", "rad", "joint_drive"),
            ("physics/stiffness/radial", "N/m", "joint"),
            ("physics/stiffness/axial", "N/m", "joint"),
            ("physics/stiffness/bending", "N*m/rad", "joint"),
        ],
        "motors" => vec![
            ("mount_point", "m", "world_at_export"),
            ("shaft_axis", "1", "world_at_export"),
            ("gear_ratio", "1", "joint_drive"),
            ("electrical/resistance", "ohm", "scalar"),
            ("electrical/inductance", "H", "scalar"),
            ("electrical/torque_constant", "N*m/A", "motor_shaft"),
            ("electrical/back_emf_constant", "V*s/rad", "motor_shaft"),
            ("electrical/rotor_inertia", "kg*m^2", "motor_shaft"),
            ("electrical/supply_voltage", "V", "scalar"),
            ("electrical/current_limit", "A", "scalar"),
            ("gearbox/ratio", "1", "motor_to_output"),
            ("gearbox/efficiency", "1", "scalar"),
            ("gearbox/backlash_rad", "rad", "gearbox_output"),
            ("gearbox/max_output_torque", "N*m", "gearbox_output"),
            ("gearbox/max_output_speed", "rad/s", "gearbox_output"),
            ("firmware/loop_rate_hz", "Hz", "scalar"),
            ("firmware/latency_s", "s", "scalar"),
        ],
        "sensors" => vec![
            ("point", "m", "link_com"),
            ("axes", "1", "sensor_axes_rows_in_link"),
            ("rate_hz", "Hz", "scalar"),
            ("noise/accel", "m/s^2", "sensor"),
            ("noise/gyro", "rad/s", "sensor"),
            ("bias/accel", "m/s^2", "sensor"),
            ("bias/gyro", "rad/s", "sensor"),
        ],
        "transmissions" => vec![("ratio", "1", "driver_angle_over_driven_angle")],
        _ => vec![],
    };
    if category == "joints" {
        match raw.get("type").and_then(Value::as_str) {
            Some("revolute" | "continuous" | "loop_revolute") => specs.extend([
                ("limits", "rad", "joint"),
                ("home", "rad", "joint"),
                ("physics/friction/coulomb", "N*m", "joint"),
                ("physics/friction/viscous", "N*m*s/rad", "joint"),
            ]),
            Some("prismatic" | "loop_prismatic") => specs.extend([
                ("limits", "m", "joint"),
                ("home", "m", "joint"),
                ("physics/friction/coulomb", "N", "joint"),
                ("physics/friction/viscous", "N*s/m", "joint"),
            ]),
            _ => {}
        }
    }
    specs
        .into_iter()
        .map(|(name, unit, frame)| property(raw, name, unit, frame))
        .collect()
}

fn inspect(authored: &Value, model: &PhysicalModel) -> Result<RobotContract, String> {
    let mut resolved = model.to_json_value_checked()?;
    // This is a field-presence skeleton, not exported physical values. JSON
    // omits legacy +infinity limits, but their absence is still a parser default.
    for motor in resolved["motors"].as_array_mut().into_iter().flatten() {
        if let Some(gearbox) = motor["gearbox"].as_object_mut() {
            for key in ["max_output_torque", "max_output_speed"] {
                gearbox.entry(key).or_insert(Value::Null);
            }
        }
    }
    let mut result = RobotContract { version:1, model_version:model.version,
        source:authored.get("source").cloned().unwrap_or(Value::Null),
        frame_convention:"SI; world is CAD export frame; link origin is COM with axes parallel to world at export; joint origins/axes are world-at-export".into(),
        entities:vec![], relations:vec![], defaulted_fields:vec![], unmodeled_fields:vec![], issues:vec![],
        materials:authored.get("materials").cloned().unwrap_or(Value::Null),
        uncertainty:authored.get("uncertainty").cloned().unwrap_or(Value::Null),
        identification:authored.get("identification").cloned().unwrap_or(Value::Null) };
    missing_fields(&resolved, authored, "", &mut result.defaulted_fields);
    missing_fields(authored, &resolved, "", &mut result.unmodeled_fields);
    let mut names: BTreeMap<(String, String), Option<String>> = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for category in ["links", "joints", "motors", "sensors", "transmissions"] {
        for (index, raw) in authored
            .get(category)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let path = format!("/{category}/{index}");
            let name = raw["name"]
                .as_str()
                .ok_or_else(|| format!("{path} requires a name"))?;
            let cad_id = raw
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty());
            let key = match cad_id {
                Some(id) => format!("{category}:id:{id}"),
                None => format!("{category}:name:{name}"),
            };
            let mut issue = |severity: &str, message: &str| {
                result.issues.push(ContractIssue {
                    severity: severity.into(),
                    path: path.clone(),
                    message: message.into(),
                })
            };
            if name.trim().is_empty() {
                issue("error", "empty entity name");
            }
            let name_key = (category.to_string(), name.to_string());
            if names.contains_key(&name_key) {
                names.insert(name_key, None);
                issue(
                    "warning",
                    "duplicate name: references require an unambiguous identity",
                );
            } else {
                names.insert(name_key, Some(key.clone()));
            }
            if !ids.insert(key.clone()) {
                issue("error", "duplicate CAD identity");
            }
            if cad_id.is_none() {
                issue(
                    "warning",
                    "missing CAD ID: name fallback does not survive renaming",
                );
            }
            let evidence = match category {
                "links" => {
                    json!({"mass_sources":raw.get("mass_sources"),"members":raw.get("members"),"member_names":raw.get("member_names"),"material":raw.get("material")})
                }
                "joints" => {
                    json!({"type":raw.get("type"),"physics_source":raw.pointer("/physics/source"),
                    "drive_backlash":raw.pointer("/physics/drive_backlash"),"identified":raw.pointer("/physics/identified")})
                }
                "motors" => {
                    json!({"spec":raw.get("spec"),"firmware":raw.get("firmware"),"thermal":raw.get("thermal"),"driver":raw.get("driver")})
                }
                "sensors" => {
                    json!({"kind":raw.get("kind"),"noise":raw.get("noise"),"bias":raw.get("bias"),"range":raw.get("range"),"quantization":raw.get("quantization")})
                }
                _ => Value::Null,
            };
            result.entities.push(Entity {
                key,
                name: name.into(),
                cad_id: cad_id.map(str::to_owned),
                category: category.into(),
                source_pointer: path,
                properties: properties(category, raw),
                evidence,
            });
        }
    }
    // CAD merges fixed bodies into one physical link. Fixed-joint records and
    // motor mounts can still refer to a member's original body name.
    for entity in result.entities.iter().filter(|e| e.category == "links") {
        let raw = authored.pointer(&entity.source_pointer).unwrap();
        for member in raw
            .get("member_names")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(name) = member.as_str() {
                let key = ("links".into(), name.into());
                match names.get(&key) {
                    None => {
                        names.insert(key, Some(entity.key.clone()));
                    }
                    Some(Some(existing)) if existing == &entity.key => {}
                    _ => {
                        names.insert(key, None);
                    }
                }
            }
        }
    }
    for entity in &result.entities {
        let raw = authored.pointer(&entity.source_pointer).unwrap();
        let bindings: &[(&str, &str, bool)] = match entity.category.as_str() {
            "joints" => &[
                ("parent", "links", true),
                ("child", "links", false),
                ("motor", "motors", true),
            ],
            "motors" => &[("joint", "joints", true), ("mounted_on", "links", true)],
            "sensors" => &[("link", "links", false), ("joint", "joints", true)],
            "transmissions" => &[
                ("driver_joint", "joints", false),
                ("driven_joint", "joints", false),
            ],
            _ => &[],
        };
        for &(role, category, nullable) in bindings {
            let path = format!("{}/{role}", entity.source_pointer);
            match raw.get(role) {
                Some(Value::Null) if nullable => {
                    if role == "parent" {
                        result.relations.push(Relation {
                            owner: entity.key.clone(),
                            role: role.into(),
                            target: None,
                        });
                    }
                }
                Some(Value::String(name))
                    if names
                        .get(&(category.into(), name.clone()))
                        .is_some_and(Option::is_some) =>
                {
                    result.relations.push(Relation {
                        owner: entity.key.clone(),
                        role: role.into(),
                        target: names[&(category.into(), name.clone())].clone(),
                    })
                }
                None if nullable => {}
                _ => result.issues.push(ContractIssue {
                    severity: "error".into(),
                    path,
                    message: format!("unresolved or ambiguous {category} reference"),
                }),
            }
        }
    }
    result.entities.sort_by(|a, b| a.key.cmp(&b.key));
    result
        .relations
        .sort_by(|a, b| (&a.owner, &a.role).cmp(&(&b.owner, &b.role)));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"version":4,"source":{"cad_sha256":"fixture"},
        "links":[{"name":"base","id":"a","mass":2.,"mass_sources":{"a":"estimated test mass"}},
                 {"name":"slider","id":"b"}],
        "joints":[{"name":"slide","id":"j","type":"prismatic","parent":"base","child":"slider","limits":[0.,1.]}],
        "sensors":[{"name":"encoder","id":"s","kind":"encoder","link":"slider","joint":"slide"}]})
    }
    #[test]
    fn separates_authored_properties_defaults_and_unmodeled_evidence() {
        let raw = fixture();
        let doc = RobotDocument::new(raw.clone()).unwrap();
        let c = doc.contract().unwrap();
        assert!(!c.has_errors());
        assert!(c.defaulted_fields.contains(&"/links/1/mass".into()));
        assert!(c.unmodeled_fields.contains(&"/links/0/mass_sources".into()));
        let slider = c
            .entities
            .iter()
            .find(|e| e.cad_id.as_deref() == Some("b"))
            .unwrap();
        assert!(
            slider
                .properties
                .iter()
                .find(|p| p.name == "mass")
                .unwrap()
                .value
                .is_none()
        );
        assert_eq!(doc.model().links[1].mass, 1.);
        assert_eq!(doc.authored(), &raw);
        let joint = c.entities.iter().find(|e| e.category == "joints").unwrap();
        assert_eq!(
            joint
                .properties
                .iter()
                .find(|p| p.name == "limits")
                .unwrap()
                .unit,
            "m"
        );
        let mut changed = doc.model().clone();
        changed.links[1].mass = 2.;
        assert!(!doc.matches_model(&changed).unwrap());
        assert!(doc.matches_model(doc.model()).unwrap());
    }
    #[test]
    fn stable_ids_preserve_topology_across_renaming_and_reordering() {
        let raw = fixture();
        let before = RobotDocument::new(raw.clone()).unwrap().contract().unwrap();
        let mut renamed = raw;
        renamed["links"][0]["name"] = json!("renamed");
        renamed["joints"][0]["parent"] = json!("renamed");
        renamed["links"].as_array_mut().unwrap().reverse();
        let after = RobotDocument::new(renamed.clone())
            .unwrap()
            .contract()
            .unwrap();
        assert_eq!(before.relations, after.relations);
        assert_eq!(
            before.entities.iter().map(|e| &e.key).collect::<Vec<_>>(),
            after.entities.iter().map(|e| &e.key).collect::<Vec<_>>()
        );
        renamed["links"][0]["id"] = json!("a");
        assert!(
            RobotDocument::new(renamed)
                .unwrap()
                .contract()
                .unwrap()
                .has_errors()
        );
    }

    #[test]
    fn merged_members_resolve_but_ambiguous_joint_references_do_not() {
        let mut raw = fixture();
        raw["links"][0]["member_names"] = json!(["base", "mount"]);
        raw["joints"][0]["parent"] = json!("mount");
        let c = RobotDocument::new(raw.clone()).unwrap().contract().unwrap();
        assert!(!c.has_errors());
        assert!(
            c.relations
                .iter()
                .any(|r| r.role == "parent" && r.target.as_deref() == Some("links:id:a"))
        );
        let mut duplicate = raw["joints"][0].clone();
        duplicate["id"] = json!("another");
        raw["joints"].as_array_mut().unwrap().push(duplicate);
        let c = RobotDocument::new(raw).unwrap().contract().unwrap();
        assert!(
            c.issues
                .iter()
                .any(|i| i.path == "/sensors/0/joint" && i.severity == "error")
        );
    }
    #[test]
    fn keeps_loop_and_transmission_relations_and_rejects_dangling_references() {
        let mut raw = fixture();
        raw["joints"].as_array_mut().unwrap().push(json!({"name":"loop","id":"loop","type":"loop_revolute","parent":"slider","child":"base"}));
        raw["transmissions"] =
            json!([{"name":"gear","driver_joint":"loop","driven_joint":"slide","ratio":2.}]);
        let c = RobotDocument::new(raw.clone()).unwrap().contract().unwrap();
        assert!(
            c.relations
                .iter()
                .any(|r| r.owner == "joints:id:loop" && r.role == "child")
        );
        assert!(c.relations.iter().any(|r| r.role == "driver_joint"));
        raw["transmissions"][0]["driver_joint"] = json!("missing");
        assert!(
            RobotDocument::new(raw)
                .unwrap()
                .contract()
                .unwrap()
                .has_errors()
        );
    }
}
