//! Graphics-independent, source-bound spatial presentation. Dimensions here are
//! display geometry, never a source for physical parameters or collision models.
use crate::{InspectionError, SystemDescription, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SPATIAL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialDescription {
    pub version: u32,
    pub description_id: String,
    pub title: String,
    /// Explicitly SI meters; adapters must convert CAD coordinates before export.
    pub length_unit: String,
    /// Right-handed, +Y up, +Z toward the viewer in the home orientation.
    pub coordinate_frame: String,
    pub provenance: String,
    pub parts: Vec<SpatialPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialPart {
    pub id: String,
    pub component: String,
    pub label: String,
    pub provenance: GeometryProvenance,
    pub position: [f32; 3],
    /// Normalized quaternion in xyzw order, local to world.
    pub rotation_xyzw: [f32; 4],
    /// Presentation-only offset; never applied to the physical model.
    pub exploded_offset: [f32; 3],
    pub color_srgb: [f32; 3],
    /// Bounding display shape; also the fallback when no model is available.
    pub shape: SpatialShape,
    /// Display model ID in the model catalog (`library/models/catalog.json`).
    /// Presentation only. Viewers without the catalog draw `shape`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GeometryProvenance {
    Illustrative { explanation: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpatialShape {
    Box {
        size: [f32; 3],
    },
    /// Axis along local +Y.
    Cylinder {
        radius: f32,
        length: f32,
    },
    Sphere {
        radius: f32,
    },
}

impl SpatialDescription {
    pub fn validate(&self, description: &SystemDescription) -> Result<(), InspectionError> {
        description.validate()?;
        ensure(
            self.version == SPATIAL_VERSION,
            "unsupported spatial version",
        )?;
        ensure(
            self.description_id == description.id,
            "foreign spatial description",
        )?;
        ensure(self.length_unit == "m", "spatial length unit must be m")?;
        ensure(
            self.coordinate_frame == "right_handed_y_up",
            "unsupported spatial frame",
        )?;
        ensure(
            !self.title.trim().is_empty() && !self.provenance.trim().is_empty(),
            "missing spatial provenance/title",
        )?;
        ensure(!self.parts.is_empty(), "empty spatial presentation")?;
        let mut ids = BTreeSet::new();
        for part in &self.parts {
            ensure(
                !part.id.is_empty() && ids.insert(&part.id),
                "duplicate/empty spatial part ID",
            )?;
            ensure(
                description.components.contains_key(&part.component),
                "unknown spatial component",
            )?;
            ensure(!part.label.trim().is_empty(), "missing spatial part label")?;
            let GeometryProvenance::Illustrative { explanation } = &part.provenance;
            ensure(
                !explanation.trim().is_empty(),
                "missing geometry provenance",
            )?;
            ensure(
                part.position
                    .iter()
                    .chain(&part.exploded_offset)
                    .all(|v| v.is_finite()),
                "nonfinite spatial position",
            )?;
            ensure(
                part.rotation_xyzw.iter().all(|v| v.is_finite())
                    && (part.rotation_xyzw.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-4,
                "spatial rotation must be normalized",
            )?;
            ensure(
                part.color_srgb
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "invalid spatial color",
            )?;
            let valid_size = |v: f32| v.is_finite() && v > 0.0;
            ensure(
                match &part.shape {
                    SpatialShape::Box { size } => size.iter().copied().all(valid_size),
                    SpatialShape::Cylinder { radius, length } => {
                        valid_size(*radius) && valid_size(*length)
                    }
                    SpatialShape::Sphere { radius } => valid_size(*radius),
                },
                "invalid spatial dimensions",
            )?;
        }
        Ok(())
    }
}

/// Shared selection/visibility commands for a GUI or an automation client.
/// These only modify presentation; the caller supplies an already validated scene.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpatialViewState {
    pub selected: Option<String>,
    pub hidden: BTreeSet<String>,
    pub exploded: bool,
    pub connections: bool,
    /// Physics layers drawn over the parts.
    #[serde(default)]
    pub overlays: BTreeSet<Overlay>,
    /// Everything but the selection drawn see-through.
    #[serde(default)]
    pub xray: bool,
    /// Fast parts as stroboscopic snapshots instead of motion blur.
    #[serde(default)]
    pub strobe: bool,
}

/// A physics visualisation layer, drawn from live or recorded measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overlay {
    /// Power into or out of each part, with the whole system's balance.
    Power,
    /// Torque arrows on shafts and force arrows on sliding parts.
    Forces,
    /// Moving dots along wires, speed ∝ current.
    Current,
    /// Temperature glow and heat-flow arrows between thermal parts.
    Heat,
    /// Fading copies of moving parts at earlier times.
    Trails,
    /// Live readouts on parts: speed, position, temperature, current.
    Values,
}
impl Overlay {
    pub const ALL: [Overlay; 6] = [Overlay::Values, Overlay::Power, Overlay::Forces, Overlay::Current, Overlay::Heat, Overlay::Trails];
    pub fn label(self) -> &'static str {
        match self {
            Overlay::Power => "Power",
            Overlay::Forces => "Forces",
            Overlay::Current => "Current",
            Overlay::Heat => "Heat",
            Overlay::Trails => "Trails",
            Overlay::Values => "Values",
        }
    }
    /// Layers shown until the viewer chooses: the ones that read without clutter.
    pub fn defaults() -> BTreeSet<Overlay> {
        [Overlay::Forces, Overlay::Current, Overlay::Heat].into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpatialCommand {
    Select { component: String },
    ClearSelection,
    SetExploded { enabled: bool },
    SetConnections { enabled: bool },
    SetOverlay { layer: Overlay, enabled: bool },
    SetXray { enabled: bool },
    SetStrobe { enabled: bool },
    HideSelected,
    ShowAll,
}

impl SpatialViewState {
    pub fn apply(
        &mut self,
        scene: &SpatialDescription,
        command: SpatialCommand,
    ) -> Result<(), InspectionError> {
        match command {
            SpatialCommand::Select { component } => {
                ensure(
                    scene.parts.iter().any(|p| p.component == component),
                    "component has no spatial representation",
                )?;
                self.hidden.remove(&component);
                self.selected = Some(component);
            }
            SpatialCommand::ClearSelection => self.selected = None,
            SpatialCommand::SetExploded { enabled } => self.exploded = enabled,
            SpatialCommand::SetConnections { enabled } => self.connections = enabled,
            SpatialCommand::SetXray { enabled } => self.xray = enabled,
            SpatialCommand::SetStrobe { enabled } => self.strobe = enabled,
            SpatialCommand::SetOverlay { layer, enabled } => {
                if enabled {
                    self.overlays.insert(layer);
                } else {
                    self.overlays.remove(&layer);
                }
            }
            SpatialCommand::HideSelected => {
                if let Some(component) = &self.selected {
                    self.hidden.insert(component.clone());
                }
            }
            SpatialCommand::ShowAll => self.hidden.clear(),
        }
        Ok(())
    }
}
