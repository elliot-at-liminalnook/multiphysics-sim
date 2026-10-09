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
use sim_runtime::hardware::protocol::{lenient, lenient_items};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;


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



// ---- The saved file an export stands for ------------------------------------


