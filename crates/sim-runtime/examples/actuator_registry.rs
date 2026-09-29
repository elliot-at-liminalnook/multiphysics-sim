//! Accepted-actuator registry tool (see `sim_runtime::actuator_registry`).
//!
//!     actuator_registry hash FAMILY.json            content hash to accept a family
//!     actuator_registry limits REGISTRY SCENE.json  per-joint limits after applying
//!     actuator_registry apply REGISTRY IN.json OUT.json
//!         IN is a scene ({"robot": model, ...}), a physical model, or a gait
//!         comparison config ({"recipe": ...}); every embedded robot is updated
//!     actuator_registry check REGISTRY FILE...      fail if any embedded robot is stale
use serde_json::Value;
use sim_domain_robot::{PhysicalModel, actuator_profile::Family};
use sim_runtime::actuator_registry::{Registry, joint_limits};
use std::path::Path;

/// JSON pointers of robot models inside the supported documents.
fn robot_pointers(doc: &Value) -> Vec<&'static str> {
    let candidates = ["", "/robot", "/scene/robot", "/recipe/experiment/scene/robot", "/recipe/planning_scene/robot", "/experiment/scene/robot", "/planning_scene/robot"];
    candidates.into_iter().filter(|p| doc.pointer(p).is_some_and(|r| r.get("motors").is_some() && r.get("actuator_profiles").is_some())).collect()
}
fn read(path: &str) -> Result<Value, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{path}: {e}"))?).map_err(|e| format!("{path}: {e}"))
}
fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("hash") if a.len() == 2 => {
            let f: Family = serde_json::from_value(read(&a[1])?).map_err(|e| e.to_string())?;
            println!("{}", f.content_hash());
        }
        Some("limits") if a.len() == 3 => {
            let registry = Registry::load(Path::new(&a[1]))?;
            let doc = read(&a[2])?;
            let pointer = *robot_pointers(&doc).first().ok_or("no robot model with actuator profiles found")?;
            let mut model: PhysicalModel = serde_json::from_value(doc.pointer(pointer).unwrap().clone()).map_err(|e| e.to_string())?;
            registry.apply(&mut model)?;
            println!("{}", serde_json::to_string_pretty(&joint_limits(&model)?).unwrap());
        }
        Some("apply") if a.len() == 4 => {
            let registry = Registry::load(Path::new(&a[1]))?;
            let mut doc = read(&a[2])?;
            let pointers = robot_pointers(&doc);
            if pointers.is_empty() {
                return Err("no robot model with actuator profiles found".into());
            }
            for p in &pointers {
                let slot = doc.pointer_mut(p).unwrap();
                let mut model: PhysicalModel = serde_json::from_value(slot.clone()).map_err(|e| e.to_string())?;
                let assigned = registry.apply(&mut model)?;
                // Replace only the profile declaration; the rest of the export is untouched.
                slot["actuator_profiles"] = serde_json::to_value(&model.actuator_profiles).unwrap();
                eprintln!("{p}: {} motors bound", assigned.len());
            }
            std::fs::write(&a[3], serde_json::to_vec(&doc).unwrap()).map_err(|e| e.to_string())?;
        }
        Some("check") if a.len() >= 3 => {
            let registry = Registry::load(Path::new(&a[1]))?;
            let mut failures = Vec::new();
            for file in &a[2..] {
                let doc = read(file)?;
                for p in robot_pointers(&doc) {
                    let model: PhysicalModel = serde_json::from_value(doc.pointer(p).unwrap().clone()).map_err(|e| e.to_string())?;
                    if let Err(e) = registry.check(&model) {
                        failures.push(format!("{file}{p}: {e}"));
                    }
                }
            }
            if !failures.is_empty() {
                return Err(failures.join("\n"));
            }
            println!("{} files use the accepted families", a.len() - 2);
        }
        _ => return Err("usage: actuator_registry hash FAMILY | limits REGISTRY SCENE | apply REGISTRY IN OUT | check REGISTRY FILE...".into()),
    }
    Ok(())
}
