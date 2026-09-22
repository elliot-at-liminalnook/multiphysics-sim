//! Display models for spatial parts: a catalog of CAD-exported OBJ/MTL files
//! (`library/models/catalog.json`, built by `cad/scripts/component_models.py`)
//! with a default model per component type. Presentation only: models never
//! feed physics, and parts without a model fall back to their bounding shape.
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::PrimitiveTopology;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct CatalogFile {
    schema: String,
    models: BTreeMap<String, ModelEntry>,
    #[serde(default)]
    defaults: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct ModelEntry {
    file: String,
}

/// One coloured piece of a model, in the part's local frame (metres, +Y up).
#[derive(Clone)]
pub struct Piece {
    pub mesh: Handle<Mesh>,
    pub color: Color,
    pub metallic: f32,
}

#[derive(Resource, Default)]
pub struct ModelLibrary {
    directory: PathBuf,
    files: BTreeMap<String, String>,
    defaults: BTreeMap<String, String>,
    loaded: BTreeMap<String, Option<Vec<Piece>>>,
    pub error: Option<String>,
}

impl ModelLibrary {
    /// Read the catalog; a missing catalog simply means shapes are drawn.
    pub fn open(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        let mut library = Self { directory: directory.clone(), ..default() };
        match std::fs::read(directory.join("catalog.json")).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<CatalogFile>(&b).map_err(|e| e.to_string())) {
            Ok(catalog) if catalog.schema == "sim.models/1" => {
                library.files = catalog.models.into_iter().map(|(k, v)| (k, v.file)).collect();
                library.defaults = catalog.defaults;
            }
            Ok(catalog) => library.error = Some(format!("unsupported model catalog `{}`", catalog.schema)),
            Err(e) => library.error = Some(format!("no model catalog in {}: {e}", directory.display())),
        }
        library
    }

    /// The model for a part: its explicit choice, else its component type's default.
    pub fn model_for(&self, explicit: Option<&str>, component_type: &str) -> Option<String> {
        explicit.map(str::to_string).or_else(|| self.defaults.get(component_type).cloned()).filter(|id| self.files.contains_key(id))
    }

    pub fn pieces(&mut self, id: &str, meshes: &mut Assets<Mesh>) -> Option<Vec<Piece>> {
        if !self.loaded.contains_key(id) {
            let parsed = self.files.get(id).map(|file| load_obj(&self.directory.join(file)));
            let pieces = match parsed {
                Some(Ok(groups)) => Some(
                    groups
                        .into_iter()
                        .map(|g| {
                            let [r, gr, b] = g.color;
                            let metallic = if r > 0.6 && (r - b).abs() < 0.12 && (r - gr).abs() < 0.12 { 0.7 } else { 0.05 };
                            Piece { mesh: meshes.add(g.mesh()), color: Color::srgb(r, gr, b), metallic }
                        })
                        .collect(),
                ),
                Some(Err(e)) => {
                    self.error = Some(format!("model {id}: {e}"));
                    None
                }
                None => None,
            };
            self.loaded.insert(id.to_string(), pieces);
        }
        self.loaded[id].clone()
    }
}

/// Triangles sharing one material.
pub struct Group {
    pub color: [f32; 3],
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
}

impl Group {
    fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
    }
}

fn read_mtl(path: &Path) -> BTreeMap<String, [f32; 3]> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else { return out };
    let mut current = None;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("newmtl") => current = it.next().map(str::to_string),
            Some("Kd") => {
                let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
                if let (Some(name), [r, g, b]) = (&current, v.as_slice()) {
                    out.insert(name.clone(), [*r, *g, *b]);
                }
            }
            _ => {}
        }
    }
    out
}

/// Minimal OBJ reader: `v`, `vn`, `f` (any `v/vt/vn` form, polygons fanned),
/// `usemtl` and `mtllib` diffuse colours. Missing normals are computed flat.
pub fn load_obj(path: &Path) -> Result<Vec<Group>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut colors = BTreeMap::new();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    let mut material = String::from("default");
    let index = |token: &str, len: usize| -> Option<usize> {
        let i: i64 = token.parse().ok()?;
        let i = if i < 0 { len as i64 + i } else { i - 1 };
        (i >= 0 && (i as usize) < len).then_some(i as usize)
    };
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("mtllib") => {
                if let Some(file) = it.next() {
                    colors.extend(read_mtl(&path.with_file_name(file)));
                }
            }
            Some("usemtl") => material = it.next().unwrap_or("default").to_string(),
            Some("v") => {
                let v: Vec<f32> = it.take(3).filter_map(|x| x.parse().ok()).collect();
                if v.len() == 3 {
                    positions.push([v[0], v[1], v[2]]);
                }
            }
            Some("vn") => {
                let v: Vec<f32> = it.take(3).filter_map(|x| x.parse().ok()).collect();
                if v.len() == 3 {
                    normals.push([v[0], v[1], v[2]]);
                }
            }
            Some("f") => {
                let corners: Vec<(usize, Option<usize>)> = it
                    .filter_map(|c| {
                        let mut parts = c.split('/');
                        let v = index(parts.next()?, positions.len())?;
                        let n = parts.nth(1).and_then(|n| index(n, normals.len()));
                        Some((v, n))
                    })
                    .collect();
                if corners.len() < 3 {
                    continue;
                }
                let color = colors.get(&material).copied().unwrap_or([0.7, 0.7, 0.72]);
                let group = groups.entry(material.clone()).or_insert_with(|| Group { color, positions: Vec::new(), normals: Vec::new() });
                for k in 1..corners.len() - 1 {
                    let tri = [corners[0], corners[k], corners[k + 1]];
                    let p = tri.map(|(v, _)| Vec3::from_array(positions[v]));
                    let flat = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero().to_array();
                    for (v, n) in tri {
                        group.positions.push(positions[v]);
                        group.normals.push(n.map(|n| normals[n]).unwrap_or(flat));
                    }
                }
            }
            _ => {}
        }
    }
    if groups.is_empty() {
        return Err(format!("{} has no faces", path.display()));
    }
    Ok(groups.into_values().collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn catalog_models_load_with_colours() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../library/models");
        let mut library = super::ModelLibrary::open(&dir);
        assert!(library.error.is_none(), "{:?}", library.error);
        assert_eq!(library.model_for(None, "electrical.mosfet").as_deref(), Some("to220"));
        assert_eq!(library.model_for(Some("heatsink"), "thermal.capacitance").as_deref(), Some("heatsink"));
        let groups = super::load_obj(&dir.join("to220.obj")).unwrap();
        assert!(groups.len() >= 2, "package and tin are separate colours");
        let ys: Vec<f32> = groups.iter().flat_map(|g| g.positions.iter().map(|p| p[1])).collect();
        let (lo, hi) = ys.iter().fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(*y), b.max(*y)));
        assert!(lo > -1e-4 && (hi - 0.0185).abs() < 5e-4, "metres, +Y up, base on the board: {lo}..{hi}");
        let mut meshes = bevy::prelude::Assets::<bevy::prelude::Mesh>::default();
        assert!(library.pieces("servo", &mut meshes).is_some_and(|p| p.len() >= 2));
    }
}
