//! Reference images textured on their planes (RoboCAD's `_draw_images`,
//! ui/viewport.py:659-691), display only: nothing here changes a placement
//! or the document.
//!
//! - **Which**: every image node that is shown (`effective_visible`) and not
//!   hidden by a comment thread's temporary isolation
//!   (`threads::shown`), whose placement has been read.
//! - **Pixels** ([`Pixels`]): `GET /nodes/{id}/image` (`reference_image`),
//!   decoded on the same `Pool::Dedicated` job into a Bevy `Image`
//!   (`Image::from_buffer`, sRGB, png and jpeg: sim-spatial's Bevy
//!   features; RoboCAD also takes webp and bmp, which are not drawn here and
//!   say so in the dock). Cached per (connection generation, node): RoboCAD
//!   never changes an image node's bytes (references.py: `update_reference`
//!   copies the image dict and keeps `data`; a new import is a new node), and
//!   its own viewport re-uploads only when the bytes change (viewport.py:
//!   664-667). At most two reads at once; a hidden image keeps its texture
//!   while its node exists.
//! - **Quads**: one entity per image under CAD's Z-up millimetre root
//!   (`mesh::CadRoot`, so it goes with the mode's root), its four corners
//!   `ImagePlacement::corners()` (u, v) = (0, 0), (w, 0), (w, h), (0, h) with
//!   RoboCAD's texture coordinates (0, 1), (1, 1), (1, 0), (0, 0); an
//!   unlit, double-sided `StandardMaterial` whose `base_color_texture` is the
//!   image and whose alpha is the opacity (`AlphaMode::Blend`), as RoboCAD's
//!   `glColor4f(1, 1, 1, opacity)`. `Pickable::IGNORE`: never a pick target
//!   (CAD's own picks cast on `CadBody` only). Rebuilt when its placement read
//!   or texture changes; despawned when the node goes, is hidden, or the
//!   document changes. While a newer revision's placement is read the last
//!   one stays drawn; the dock labels it with the revision it was read at.
use super::{images, reads::Placed};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::cad::document::CadDocument;
use crate::cad::mesh::CadRoot;
use crate::jobs::{Job, Pool};
use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use serde_json::{Value, json};
use crate::cad::types::ImagePlacement;
use std::collections::HashMap;

/// Reads at once.
const MAX_READS: usize = 2;

/// A decoded image, before it becomes an asset.
pub struct Decoded {
    pub format: String,
    pub size: (u32, u32),
    pub image: Image,
}

/// One image's pixels.
pub enum PixelState {
    Reading(Job<Decoded>),
    /// Decoded; the window takes the image into its assets next.
    Decoded(Box<Decoded>),
    /// A texture in the window's assets.
    Ready { format: String, size: (u32, u32) },
    Failed(String),
}

/// One image's pixels in connection `generation`.
pub struct Pixels {
    pub generation: u64,
    pub state: PixelState,
}
impl Pixels {
    pub(crate) fn json(&self) -> Value {
        match &self.state {
            PixelState::Reading(_) | PixelState::Decoded(_) => json!({"state": "reading"}),
            PixelState::Ready { format, size } => json!({"state": "drawn", "format": format, "width_px": size.0, "height_px": size.1}),
            PixelState::Failed(e) => json!({"state": "not drawn", "error": e}),
        }
    }
    /// The dock's note for this image, if it is not drawn.
    pub(crate) fn note(&self) -> Option<String> {
        match &self.state {
            PixelState::Failed(e) => Some(e.clone()),
            _ => None,
        }
    }
}

/// The formats this window decodes (sim-spatial's Bevy features `png`,
/// `jpeg`). Pillow names many camera JPEGs "mpo" (a JPEG with more images
/// after the first); the `image` crate decodes its first image as JPEG.
pub(crate) fn decodable(format: &str) -> bool {
    matches!(format, "png" | "jpeg" | "jpg" | "mpo")
}

/// Bytes RoboCAD stored as `format` into a Bevy image, or why not.
pub(crate) fn decode(format: &str, bytes: &[u8]) -> Result<Image, String> {
    if !decodable(format) {
        let shown = if format.is_empty() { "an unknown format".to_string() } else { format.to_uppercase() };
        return Err(format!("{shown}: RoboCAD keeps it, but this window draws PNG and JPEG images only"));
    }
    let extension = if format == "mpo" { "jpeg" } else { format };
    Image::from_buffer(bytes, ImageType::Extension(extension), CompressedImageFormats::NONE, true, ImageSampler::default(), RenderAssetUsages::default()).map_err(|e| format!("{}: {e}", format.to_uppercase()))
}

/// The quad of `p` (RoboCAD's mm, model frame; the root maps it).
pub(crate) fn quad(p: &ImagePlacement) -> Mesh {
    let corners = p.corners().map(|c| [c[0] as f32, c[1] as f32, c[2] as f32]);
    let n = p.plane.normal;
    let normal = [n[0] as f32, n[1] as f32, n[2] as f32];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, corners.to_vec());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![normal; 4]);
    // viewport.py:686: (u, v) → (s, t) = (0,0)→(0,1), (w,0)→(1,1), (w,h)→(1,0), (0,h)→(0,0).
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

/// A drawn image.
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct ReferencePlane {
    pub id: String,
    pub generation: u64,
    /// What it was built from: the placement's values (a newer read of the
    /// same placement keeps the quad).
    pub drawn: String,
}

/// The window's textures (per generation and node).
#[derive(Resource, Default)]
pub(crate) struct ReferenceTextures {
    generation: u64,
    textures: HashMap<String, Handle<Image>>,
}

/// SimSync: the pixel reads and the quads (see the module doc).
#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    doc: Option<ResMut<CadDocument>>,
    mut store: ResMut<ReferenceTextures>,
    root: Option<Single<Entity, With<CadRoot>>>,
    mut images_assets: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    drawn: Query<(Entity, &ReferencePlane)>,
) {
    let Some(mut doc) = doc else {
        for (e, _) in &drawn {
            commands.entity(e).despawn();
        }
        return;
    };
    let generation = doc.generation;
    if store.generation != generation {
        store.generation = generation;
        store.textures.clear();
    }
    // Which images are drawn, with their placement read.
    let wanted: Vec<(String, Placed)> = images(&doc)
        .into_iter()
        .filter(|n| n.effective_visible && crate::cad::threads::shown(&doc, &n.id))
        .filter_map(|n| doc.references.reads.placements.get(&n.id).filter(|p| p.placement.is_ok()).map(|p| (n.id.clone(), p.clone())))
        .collect();
    // Every image node: a hidden one keeps its texture (shown again without a read).
    let nodes: Vec<String> = images(&doc).into_iter().map(|n| n.id.clone()).collect();
    // Pixels: land, start, take into the assets. Only touched when something changes.
    let d = doc.bypass_change_detection();
    let mut changed = false;
    d.references.pixels.retain(|id, p| {
        let keep = p.generation == generation && nodes.contains(id);
        changed |= !keep;
        keep
    });
    store.textures.retain(|id, _| d.references.pixels.contains_key(id));
    for p in d.references.pixels.values_mut() {
        let landed = match &p.state {
            PixelState::Reading(job) => job.poll(),
            _ => None,
        };
        if let Some(result) = landed {
            p.state = match result {
                Ok(decoded) => PixelState::Decoded(Box::new(decoded)),
                Err(e) => PixelState::Failed(e),
            };
            changed = true;
        }
    }
    let reading = d.references.pixels.values().filter(|p| matches!(p.state, PixelState::Reading(_))).count();
    let local = d.local.clone().filter(|_| d.connected());
    if let Some(local) = local {
        let new: Vec<String> = wanted.iter().map(|(id, _)| id.clone()).filter(|id| !d.references.pixels.contains_key(id)).take(MAX_READS.saturating_sub(reading)).collect();
        for id in new {
            let local = local.clone();
            let node = id.clone();
            // The image's bytes are the archive entry `image/<id>`.
            let job = Job::spawn(Pool::Compute, generation, "cad reference image", move |_| {
                let bytes = local.archive.entry(&format!("image/{node}")).ok_or_else(|| format!("{node}: the archive holds no image bytes"))?.to_vec();
                let (w, h) = sim_cad::references::image_size(&bytes)?;
                let format = match bytes.get(..4) { Some(b"\x89PNG") => "png", Some([0xff, 0xd8, ..]) => "jpeg", Some(b"RIFF") => "webp", Some([b'B', b'M', ..]) => "bmp", _ => "gif" }.to_string();
                let image = decode(&format, &bytes)?;
                Ok(Decoded { format, size: (w, h), image })
            });
            d.references.pixels.insert(id, Pixels { generation, state: PixelState::Reading(job) });
            changed = true;
        }
    }
    for (id, p) in d.references.pixels.iter_mut() {
        if matches!(p.state, PixelState::Decoded(_)) {
            let PixelState::Decoded(decoded) = std::mem::replace(&mut p.state, PixelState::Failed(String::new())) else { continue };
            let Decoded { format, size, image } = *decoded;
            store.textures.insert(id.clone(), images_assets.add(image));
            p.state = PixelState::Ready { format, size };
            changed = true;
        }
    }
    if changed {
        d.touch();
        doc.set_changed();
    }
    // Quads: one per wanted image with a texture, rebuilt when its placement changes.
    let mut have: HashMap<String, (Entity, ReferencePlane)> = HashMap::new();
    for (e, plane) in &drawn {
        let current = plane.generation == generation && wanted.iter().any(|(id, _)| *id == plane.id) && store.textures.contains_key(&plane.id);
        if current && !have.contains_key(&plane.id) {
            have.insert(plane.id.clone(), (e, plane.clone()));
        } else {
            commands.entity(e).despawn();
        }
    }
    let Some(root) = root else { return };
    for (id, placed) in &wanted {
        let (Some(texture), Ok(p)) = (store.textures.get(id), placed.placement.as_ref()) else { continue };
        let key = format!("{p:?}");
        if let Some((e, plane)) = have.get(id) {
            if plane.drawn == key {
                continue;
            }
            commands.entity(*e).despawn();
        }
        let material = StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, p.opacity.clamp(0.0, 1.0) as f32),
            base_color_texture: Some(texture.clone()),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        };
        commands.spawn((
            Mesh3d(meshes.add(quad(p))),
            MeshMaterial3d(materials.add(material)),
            Transform::default(),
            Visibility::default(),
            Pickable::IGNORE,
            ReferencePlane { id: id.clone(), generation, drawn: key },
            ChildOf(*root),
        ));
    }
}

/// CadPlugin: the textures' store (emptied when CAD mode closes; the quads
/// go with the root) and [`sync`] (SimSync).
pub(super) fn build(app: &mut App) {
    app.init_resource::<ReferenceTextures>()
        .add_systems(OnExit(ModeScope::Cad), |mut store: ResMut<ReferenceTextures>| *store = ReferenceTextures::default())
        .add_systems(Update, sync.in_set(ViewerSet::SimSync).run_if(in_state(ViewerMode::Cad)));
}
