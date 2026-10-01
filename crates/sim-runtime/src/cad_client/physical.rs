//! Materials, simulation results and the physical model over `api.py`'s
//! routes, and the typed `Ops` wrappers that edit materials and joint
//! physics.
//!
//! - `GET`/`POST /materials`: `Material.to_json` (density g/cm³, colour RGB
//!   0..1, `engineering` the overrides only; `Material.props()` fills
//!   RoboCAD's defaults in on RoboCAD's side).
//! - `GET /results` (the whole loaded `*.simresult.json`, `{}` when none),
//!   `POST /results/load {"path"}` (hangs each block on its node; does not
//!   move RoboCAD's revision), `GET /results/nodes` (per node id: the
//!   margins `physical.results_margins` computes, the node's results block
//!   and its material's yield strength), `POST /identification/apply
//!   {"path"}` (fitted joint parameters into `robot_settings`).
//! - `GET /physical?flex=1|0[&planar=1]`: the simrobot v4 description (SI),
//!   never with `path` (RoboCAD would write a file).
//!
//! Reads are tolerant as in [`super::robot`]; a `NaN` margin reads as `None`.
//! Writes are RoboCAD edits: use a client with [`super::EDIT_TIMEOUT`].
use super::robot::lenient_map;
use super::{CadClient, CadError, OpResult};
use crate::hardware_client::{lenient, lenient_items};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// `physical.py`'s `SCHEMA_VERSION`: the simrobot description's version.
pub const PHYSICAL_SCHEMA_VERSION: u64 = 4;

/// One document material (`Material.to_json`). `engineering` holds only
/// the overridden engineering properties (SI, `document.ENGINEERING_KEYS`).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Material {
    pub id: String,
    pub name: String,
    /// g/cm³.
    pub density: f64,
    pub color: [f64; 3],
    pub roughness: f64,
    pub metallic: f64,
    #[serde(deserialize_with = "lenient_items")]
    pub tags: Vec<String>,
    #[serde(deserialize_with = "lenient")]
    pub engineering: Map<String, Value>,
}

impl Default for Material {
    /// `Material`'s dataclass defaults.
    fn default() -> Self {
        Material { id: String::new(), name: String::new(), density: 0.0, color: [0.7, 0.7, 0.72], roughness: 0.5, metallic: 0.0, tags: Vec::new(), engineering: Map::new() }
    }
}

impl Material {
    /// One of `DocState::materials`; `None` when it is malformed.
    pub fn of(value: &Value) -> Option<Material> {
        serde_json::from_value(value.clone()).ok()
    }
}

/// `POST /materials`' body (`Service.add_material`): `id` defaults to the
/// name lower-cased with `_` for spaces; omitted options take RoboCAD's
/// defaults (colour 0.7/0.7/0.72, roughness 0.5, metallic 0, no tags).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NewMaterial {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    /// g/cm³.
    pub density: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// `physical.results_margins` for one node: links carry the first four,
/// joints the next three, motors the last four. `None` is absent, `null`
/// or non-finite in the results file (never filled in).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Margins {
    #[serde(deserialize_with = "lenient")]
    pub yield_margin: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub peak_stress_pa: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub peak_temperature_c: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub tg_margin_c: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub bearing_margin: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub screw_shear_margin: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub peak_reaction_force_n: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub stall_margin: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub peak_current_a: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub peak_winding_c: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub mount_tg_margin_c: Option<f64>,
}

/// One node's loaded results (`GET /results/nodes`' `nodes`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NodeResult {
    /// `Node.results`: `{"section": "links"|"joints"|"motors", …the
    /// file's block}` (a link block may carry a `hotspot`).
    pub results: Value,
    /// The node's material's yield strength (Pa, `Material.props()`, so a
    /// default when not overridden); `None` without a material.
    #[serde(deserialize_with = "lenient")]
    pub yield_strength_pa: Option<f64>,
}

/// `GET /results/nodes`: the loaded results per node id at `revision`.
/// Empty maps (and `None`s) when no results are loaded.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NodeResults {
    /// RoboCAD's document revision when read (loading results does not move it).
    pub revision: u64,
    /// The results file.
    pub path: Option<String>,
    /// When it was loaded (`%Y-%m-%dT%H:%M:%S`, RoboCAD's local time).
    pub loaded: Option<String>,
    /// Whether the results describe another state than the document's
    /// (`None` when no results are loaded).
    pub stale: Option<bool>,
    pub provenance: Value,
    #[serde(deserialize_with = "lenient_map")]
    pub margins: BTreeMap<String, Margins>,
    #[serde(deserialize_with = "lenient_map")]
    pub nodes: BTreeMap<String, NodeResult>,
}

/// `POST /results/load` and `/identification/apply`'s body.
#[derive(Serialize)]
struct FileBody<'a> {
    path: &'a str,
}

impl CadClient {
    /// `GET /materials`: the document's materials. A malformed one is
    /// dropped, not the list.
    pub fn materials(&self) -> Result<Vec<Material>, CadError> {
        match self.get::<Value>("/materials")? {
            Value::Array(items) => Ok(items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()),
            other => Err(CadError { method: "GET", route: "/materials".into(), status: None, message: format!("unexpected answer: expected a list of materials, got {other}") }),
        }
    }
    /// `POST /materials` (201): the new (or replaced, same id) material, one
    /// undo step "Material".
    pub fn add_material(&self, material: &NewMaterial) -> Result<Material, CadError> {
        self.send("POST", "/materials", Some(material))
    }
    /// `GET /results`: the whole loaded results file (`{}` when none).
    pub fn results(&self) -> Result<Value, CadError> {
        self.get("/results")
    }
    /// `POST /results/load {"path"}`: RoboCAD reads the `*.simresult.json`
    /// at `path` (on its side) and answers the whole file with `path`,
    /// `loaded` and `stale` added.
    pub fn load_results(&self, path: &str) -> Result<Value, CadError> {
        self.send("POST", "/results/load", Some(&FileBody { path }))
    }
    /// `GET /results/nodes`: margins, results blocks and yield strengths
    /// per node id.
    pub fn results_nodes(&self) -> Result<NodeResults, CadError> {
        self.get("/results/nodes")
    }
    /// `POST /identification/apply {"path"}`: the fitted joint parameters
    /// of a fit or results file stored in the document; answers the whole
    /// `identification` setting afterwards.
    pub fn apply_identification(&self, path: &str) -> Result<Value, CadError> {
        self.send("POST", "/identification/apply", Some(&FileBody { path }))
    }
    /// `GET /physical?flex=1|0`, plus `&planar=1` for the planar hint
    /// (`Plane.xz`: normal −Y): the simrobot v4 description
    /// ([`PHYSICAL_SCHEMA_VERSION`], SI). Never passes `path`.
    pub fn physical_model(&self, flex: bool, planar: bool) -> Result<Value, CadError> {
        let route = format!("/physical?flex={}{}", if flex { 1 } else { 0 }, if planar { "&planar=1" } else { "" });
        self.get(&route)
    }

    /// `set_material(ids, material_id)`. Undo step "Material".
    pub fn set_material(&self, ids: &[String], material_id: &str) -> Result<OpResult, CadError> {
        self.op("set_material", &[json!(ids), json!(material_id)], &Map::new())
    }
    /// `set_color(ids, color)`: RGB 0..1, `None` back to the material's
    /// colour. Undo step "Color".
    pub fn set_color(&self, ids: &[String], color: Option<[f64; 3]>) -> Result<OpResult, CadError> {
        self.op("set_color", &[json!(ids), json!(color)], &Map::new())
    }
    /// `set_material_props(material_id, **props)`: engineering overrides
    /// (keys of `document.ENGINEERING_KEYS`, SI; another key is a 422
    /// naming them); `result` is the material's `props()` afterwards.
    /// Undo step "Material properties".
    pub fn set_material_props(&self, material_id: &str, props: &Map<String, Value>) -> Result<OpResult, CadError> {
        self.op("set_material_props", &[json!(material_id)], props)
    }
    /// `set_joint_physics(joint_id, **overrides)`: joint physics overrides
    /// (SI: clearance, drive_backlash, friction, stiffness,
    /// flex_patch_radius; an object merges into the current one, `null`
    /// restores an inferred value where RoboCAD supports it); `result` is
    /// the joint's physics block afterwards. Undo step "Joint physics".
    pub fn set_joint_physics(&self, joint_id: &str, overrides: &Map<String, Value>) -> Result<OpResult, CadError> {
        self.op("set_joint_physics", &[json!(joint_id)], overrides)
    }
}
