//! Shared read-only robot inspection for CAD, native tools and browser workers.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sim_domain_robot::contract::{RobotContract, RobotDocument};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputBinding {
    pub version: u32,
    pub origin: crate::robot_input::InputOrigin,
    /// The preserved input's claims, separate from subsequent parsed-model edits.
    /// None means original field presence was unavailable at the boundary.
    pub input: Option<RobotContract>,
    pub overrides: Vec<crate::robot_input::InputOverride>,
}
impl InputBinding {
    pub fn from_scene(scene: &crate::session::Scene) -> Result<Self, String> {
        use crate::robot_input::{InputOrigin, RobotInput};
        let (_, receipt) = RobotInput::serialize_model(scene.robot_input.as_ref(), &scene.robot)?;
        let origin = scene
            .robot_input
            .as_ref()
            .map_or(InputOrigin::ParsedModel, |s| s.origin());
        let input = scene
            .robot_input
            .as_ref()
            .filter(|s| s.origin() == InputOrigin::EpisodeDocument)
            .map(|s| s.input_contract())
            .transpose()?;
        Ok(Self {
            version: 1,
            origin,
            input,
            overrides: receipt.map_or_else(Vec::new, |r| r.overrides),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inspection {
    pub version: u32,
    pub robot: RobotContract,
    /// Edits to the input claims above; omitted for a plain original document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robot_input: Option<crate::robot_input::InputReceipt>,
    /// Descriptions from the runtime registry, shared with CAD and Rhai.
    pub registered_robot_components: Value,
}

pub fn inspect(document: Value) -> Result<Inspection, String> {
    let mut robot_input = None;
    let authored = if document.get("robot").is_some() && document.get("robot_input").is_some() {
        let scene: crate::session::Scene =
            serde_json::from_value(document).map_err(|e| e.to_string())?;
        let input = scene
            .robot_input
            .as_ref()
            .ok_or("robot input unavailable")?;
        if input.origin() != crate::robot_input::InputOrigin::EpisodeDocument {
            return Err(
                "original robot field presence is unavailable for parsed-model input".into(),
            );
        }
        let binding = scene.input_binding()?;
        robot_input = Some(crate::robot_input::InputReceipt {
            version: binding.version,
            origin: binding.origin,
            overrides: binding.overrides,
        });
        input.document().clone()
    } else {
        document.get("robot").cloned().unwrap_or(document)
    };
    let robot = RobotDocument::new(authored)?.contract()?;
    let mut catalogue = sim_script::catalogue(&crate::registry());
    catalogue.as_array_mut().unwrap().retain(|entry| {
        entry["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("robot."))
    });
    Ok(Inspection {
        version: 1,
        robot,
        robot_input,
        registered_robot_components: catalogue,
    })
}
