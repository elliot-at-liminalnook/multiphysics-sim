use crate::archive::ArchiveDocument;
use serde_json::Value;
use std::{
    collections::HashSet,
    ffi::{CStr, c_char, c_void},
    sync::Mutex,
};
#[derive(Clone, Debug, Default)]
pub struct GeometryProperties {
    pub volume_mm3: f64,
    pub centroid_mm: [f64; 3],
    pub inertia_mm5: [[f64; 3]; 3],
}
#[derive(Clone, Debug)]
pub struct SolidGeometry {
    pub properties: GeometryProperties,
}
#[derive(Clone, Debug, Default)]
pub struct BodyGeometry {
    pub node_id: String,
    pub vertices_mm: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub triangle_faces: Vec<u32>,
    pub properties: GeometryProperties,
    pub solids: Vec<SolidGeometry>,
}
/// Global serialization is conservative: OCCT mutates internal query caches.
/// Only owned numeric buffers escape this mutex and their creating job thread.
static KERNEL: Mutex<()> = Mutex::new(());

/// Wait for the one OCCT owner (polling `cancelled`).
pub(crate) fn kernel_lock(cancelled: &dyn Fn() -> bool) -> Result<std::sync::MutexGuard<'static, ()>, String> {
    loop {
        if cancelled() {
            return Err("cancelled waiting for OCCT owner".into());
        }
        match KERNEL.try_lock() {
            Ok(g) => return Ok(g),
            Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(std::time::Duration::from_millis(5)),
            Err(_) => return Err("OCCT owner mutex poisoned".into()),
        }
    }
}
unsafe extern "C" {
    fn sim_cad_kernel_version() -> *const c_char;
    fn sim_cad_kernel_images(
        context: *mut c_void,
        callback: extern "C" fn(*mut c_void, *const c_char, *const u8, usize) -> bool,
        error: *mut c_char,
        error_size: usize,
    ) -> i32;

    fn sim_cad_query(
        bytes: *const u8,
        size: usize,
        primitive: i32,
        args: *const f64,
        matrices: *const f64,
        matrix_count: usize,
        volume: bool,
        tolerance: f64,
        context: *mut c_void,
        property: extern "C" fn(*mut c_void, i32, *const f64),
        vertex: extern "C" fn(*mut c_void, *const f64, *const f64),
        triangle: extern "C" fn(*mut c_void, u32, u32, u32, u32),
        cancel: extern "C" fn(*mut c_void) -> bool,
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
}
/// Version of the compilation headers, not a runtime library version claim.
pub fn kernel_version() -> String {
    unsafe { CStr::from_ptr(sim_cad_kernel_version()) }
        .to_string_lossy()
        .into_owned()
}
/// Identify the loaded native implementation, not just its compilation headers.
/// The loader supplies its immutable build UUID/ID and live executable sections;
/// full distribution files are
/// independently hashed. No native image pointer survives its callback.
pub fn kernel_identity() -> Result<String, String> {
    kernel_identity_with(&|| false)
}

/// Cancellable identity capture for the jobs-owned loading/derivation path.
pub fn kernel_identity_with(cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, io::Read};
    struct Images<'a> {
        code: BTreeMap<Vec<u8>, Sha256>,
        failed: bool,
        cancelled: &'a dyn Fn() -> bool,
        cancellation_seen: bool,
    }
    extern "C" fn collect(
        context: *mut c_void,
        path: *const c_char,
        bytes: *const u8,
        size: usize,
    ) -> bool {
        let images = unsafe { &mut *(context as *mut Images<'_>) };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if (images.cancelled)() {
                images.cancellation_seen = true;
                return;
            }
            let path = unsafe { CStr::from_ptr(path) }.to_bytes().to_vec();
            let code = images.code.entry(path).or_default();
            code.update((size as u64).to_le_bytes());
            for chunk in unsafe { std::slice::from_raw_parts(bytes, size) }.chunks(65536) {
                if (images.cancelled)() {
                    images.cancellation_seen = true;
                    break;
                }
                code.update(chunk);
            }
        }));
        if result.is_err() {
            images.failed = true;
        }
        !images.failed && !images.cancellation_seen
    }
    let mut images = Images {
        code: BTreeMap::new(),
        failed: false,
        cancelled,
        cancellation_seen: false,
    };
    let mut error = [0 as c_char; 1024];
    let result = unsafe {
        sim_cad_kernel_images(
            &mut images as *mut Images<'_> as *mut c_void,
            collect,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if images.cancellation_seen || cancelled() {
        return Err("OCCT loaded implementation identity capture cancelled".into());
    }
    if result != 0 || images.failed {
        return Err(format!(
            "OCCT loaded implementation identity: {}",
            unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy()
        ));
    }
    let required = [
        "TKMesh",
        "TKPrim",
        "TKTopAlgo",
        "TKBRep",
        "TKGeomBase",
        "TKG3d",
        "TKG2d",
        "TKMath",
        "TKernel",
    ];
    let mut found = std::collections::HashSet::new();
    let mut combined = Sha256::new();
    combined.update(b"sim-cad-loaded-occt-images-v2\0");
    combined.update(kernel_version().as_bytes());
    for (path_bytes, code) in images.code {
        if cancelled() {
            return Err(
                "OCCT loaded implementation identity capture cancelled between libraries".into(),
            );
        }
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            std::path::Path::new(std::ffi::OsStr::from_bytes(&path_bytes))
        };
        #[cfg(not(unix))]
        return Err("OCCT loaded identity requires an inspected Unix loader".into());
        let basename = path
            .file_name()
            .ok_or("OCCT loaded image path lacks filename")?
            .to_string_lossy();
        for name in required {
            if basename.starts_with(&format!("lib{name}.")) {
                found.insert(name);
            }
        }
        let mut file = std::fs::File::open(path).map_err(|e| {
            format!(
                "OCCT loaded image {}: cannot read full distribution bytes: {e}",
                path.display()
            )
        })?;
        let before = file
            .metadata()
            .map_err(|e| format!("OCCT loaded image {}: {e}", path.display()))?;
        let mut file_hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            if cancelled() {
                return Err(format!(
                    "OCCT loaded image {}: identity capture cancelled",
                    path.display()
                ));
            }
            let count = file
                .read(&mut buffer)
                .map_err(|e| format!("OCCT loaded image {}: {e}", path.display()))?;
            if count == 0 {
                break;
            }
            file_hash.update(&buffer[..count]);
        }
        let after = file
            .metadata()
            .map_err(|e| format!("OCCT loaded image {}: {e}", path.display()))?;
        if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
            return Err(format!(
                "OCCT loaded image {} changed while fingerprinting",
                path.display()
            ));
        }
        // Include exact loader path bytes as attribution plus both independently
        // captured fingerprints. Loaded code remains authoritative if its file
        // has been replaced since the process started.
        combined.update((path_bytes.len() as u64).to_le_bytes());
        combined.update(path_bytes);
        combined.update(code.finalize());
        combined.update(file_hash.finalize());
    }
    for name in required {
        if !found.contains(name) {
            return Err(format!(
                "OCCT loaded implementation identity: required shared image lib{name} was not identifiable"
            ));
        }
    }
    Ok(format!(
        "occt-loaded-images-v2:sha256:{:x}",
        combined.finalize()
    ))
}

struct Context<'a> {
    body: BodyGeometry,
    cancelled: &'a dyn Fn() -> bool,
}
extern "C" fn property(ctx: *mut c_void, solid: i32, p: *const f64) {
    let c = unsafe { &mut *(ctx as *mut Context<'_>) };
    let p = unsafe { std::slice::from_raw_parts(p, 13) };
    let v = GeometryProperties {
        volume_mm3: p[0],
        centroid_mm: [p[1], p[2], p[3]],
        inertia_mm5: [
            [p[4], p[5], p[6]],
            [p[7], p[8], p[9]],
            [p[10], p[11], p[12]],
        ],
    };
    if solid < 0 {
        c.body.properties = v;
    } else {
        c.body.solids.push(SolidGeometry { properties: v });
    }
}
extern "C" fn vertex(ctx: *mut c_void, p: *const f64, n: *const f64) {
    let c = unsafe { &mut *(ctx as *mut Context<'_>) };
    let p = unsafe { std::slice::from_raw_parts(p, 3) };
    let n = unsafe { std::slice::from_raw_parts(n, 3) };
    c.body.vertices_mm.push([p[0], p[1], p[2]]);
    c.body.normals.push([n[0], n[1], n[2]]);
}
extern "C" fn triangle(ctx: *mut c_void, a: u32, b: u32, c: u32, f: u32) {
    let ctx = unsafe { &mut *(ctx as *mut Context<'_>) };
    ctx.body.triangles.push([a, b, c]);
    ctx.body.triangle_faces.push(f);
}
extern "C" fn cancel(ctx: *mut c_void) -> bool {
    let ctx = unsafe { &*(ctx as *const Context<'_>) };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (ctx.cancelled)())).unwrap_or(true)
}

pub(crate) fn vector(value: &Value, default: [f64; 3]) -> Result<[f64; 3], String> {
    if value.is_null() {
        return Ok(default);
    }
    let a = value
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or("expected three coordinates")?;
    let mut out = [0.; 3];
    for i in 0..3 {
        out[i] = a[i]
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or("coordinate must be finite")?;
    }
    Ok(out)
}
pub(crate) fn transform(v: &Value, rigid: bool) -> Result<[f64; 12], String> {
    let t = vector(&v["translation"], [0.; 3])?;
    let a = vector(&v["axis"], [0., 0., 1.])?;
    let angle = v["angle_deg"].as_f64().unwrap_or(0.).to_radians();
    let scale = v["scale"].as_f64().unwrap_or(1.);
    if !angle.is_finite() || !scale.is_finite() || scale == 0. || (rigid && scale != 1.) {
        return Err(
            "placement requires finite angle and nonzero scale (components require scale 1)".into(),
        );
    }
    let norm = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm < 1e-12 {
        return Err("placement axis must be nonzero".into());
    }
    let [x, y, z] = a.map(|v| v / norm);
    let (s, c) = angle.sin_cos();
    let k = 1. - c;
    Ok([
        scale * (k * x * x + c),
        scale * (k * x * y - s * z),
        scale * (k * x * z + s * y),
        t[0],
        scale * (k * x * y + s * z),
        scale * (k * y * y + c),
        scale * (k * y * z - s * x),
        t[1],
        scale * (k * x * z - s * y),
        scale * (k * y * z + s * x),
        scale * (k * z * z + c),
        t[2],
    ])
}
fn mirror(v: &Value) -> Result<[f64; 12], String> {
    let p = vector(&v["origin"], [0.; 3])?;
    let n = vector(&v["normal"], [0., 0., 1.])?;
    let l = n.iter().map(|v| v * v).sum::<f64>().sqrt();
    if l < 1e-12 {
        return Err("mirror normal must be nonzero".into());
    }
    let n = n.map(|v| v / l);
    let d = p.iter().zip(n).map(|(p, n)| p * n).sum::<f64>();
    let mut m = [0.; 12];
    for i in 0..3 {
        for j in 0..3 {
            m[i * 4 + j] = (if i == j { 1. } else { 0. }) - 2. * n[i] * n[j];
        }
        m[i * 4 + 3] = 2. * d * n[i];
    }
    Ok(m)
}
struct Input<'a> {
    bytes: &'a [u8],
    primitive: i32,
    args: [f64; 8],
    matrices: Vec<[f64; 12]>,
    volume: bool,
    content: String,
}
fn resolve<'a>(
    doc: &'a ArchiveDocument,
    id: &str,
    seen: &mut HashSet<String>,
) -> Result<Input<'a>, String> {
    if !seen.insert(id.into()) {
        return Err("cyclic geometry source".into());
    }
    let n = doc
        .node(id)
        .ok_or_else(|| format!("missing source node {id}"))?;
    if n["kind"] == "instance" {
        let source = n["source"].as_str().ok_or("instance missing source")?;
        let mut input = resolve(doc, source, seen)?;
        if !n["mirror_plane"].is_null() {
            input.matrices.push(mirror(&n["mirror_plane"])?);
        }
        input.matrices.push(transform(&n["transform"], false)?);
        return Ok(input);
    }
    if !n["component_member"].is_null() {
        return component_input(doc, n);
    }
    let content = format!("brep/{id}.brep");
    let bytes = doc
        .entry(&content)
        .ok_or_else(|| format!("missing {content}"))?;
    Ok(Input {
        bytes,
        primitive: 0,
        args: [0.; 8],
        matrices: vec![],
        volume: n["body_kind"].as_str().unwrap_or("solid") == "solid",
        content,
    })
}

/// What node `id`'s exact geometry is made from (its B-rep bytes, a
/// component recipe's primitive and arguments, and every placement), hashed:
/// equal fingerprints are equal geometry. Tessellation tolerance is not part
/// of it. Comment pins store it as their geometry stamp (`sim-cad:` prefix).
pub fn fingerprint(doc: &ArchiveDocument, id: &str) -> Result<String, String> {
    Ok(hash_input(&resolve(doc, id, &mut HashSet::new())?))
}

/// The fingerprint a plain body (a `brep/<id>.brep` entry) with these bytes has.
pub fn body_fingerprint(bytes: &[u8], solid: bool) -> String {
    hash_input(&Input { bytes, primitive: 0, args: [0.; 8], matrices: vec![], volume: solid, content: String::new() })
}

fn hash_input(input: &Input) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update((input.bytes.len() as u64).to_le_bytes());
    h.update(input.bytes);
    h.update(input.primitive.to_le_bytes());
    for v in input.args.iter().chain(input.matrices.iter().flatten()) {
        h.update(v.to_le_bytes());
    }
    h.update([u8::from(input.volume)]);
    format!("sim-cad:{:x}", h.finalize())
}

/// Every geometry node's fingerprint (nodes whose source cannot be resolved
/// are left out; `load_geometry` names their error).
pub fn fingerprints(doc: &ArchiveDocument) -> std::collections::HashMap<String, String> {
    doc.manifest["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| matches!(n["kind"].as_str(), Some("body" | "sheet" | "curve" | "instance")))
        .filter_map(|n| n["id"].as_str())
        .filter_map(|id| fingerprint(doc, id).ok().map(|f| (id.to_string(), f)))
        .collect()
}

pub fn load_geometry(
    doc: &ArchiveDocument,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(&str),
) -> Result<Vec<BodyGeometry>, String> {
    load_geometry_reusing(doc, &std::collections::HashMap::new(), cancelled, progress)
}

/// [`load_geometry`], taking a body from `reuse` (keyed by fingerprint and
/// tessellation tolerance, [`reuse_key`]) instead of asking the kernel again:
/// an edit re-tessellates only the bodies it changed.
pub fn load_geometry_reusing(
    doc: &ArchiveDocument,
    reuse: &std::collections::HashMap<String, BodyGeometry>,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(&str),
) -> Result<Vec<BodyGeometry>, String> {
    let _guard = kernel_lock(cancelled).map_err(|e| format!("{}: {e}", doc.path.display()))?;
    let mut out = Vec::new();
    let nodes = doc.manifest["nodes"]
        .as_array()
        .ok_or("manifest nodes missing")?;
    for node in nodes {
        let id = node["id"].as_str().ok_or("manifest node id missing")?;
        // A reference mesh: drawn from its stored triangles (no exact solid, no properties).
        if node["kind"] == "mesh" {
            out.push(mesh_geometry(doc, node, id).map_err(|e| format!("{}: node {id}: {e}", doc.path.display()))?);
            continue;
        }
        if !matches!(
            node["kind"].as_str(),
            Some("body" | "sheet" | "curve" | "instance")
        ) {
            continue;
        }
        if cancelled() {
            return Err(format!("{}: node {id}: cancelled", doc.path.display()));
        }
        progress(&format!(
            "OCCT B-rep, exact properties and display mesh: {id}"
        ));
        let input = resolve(doc, id, &mut HashSet::new())
            .map_err(|e| format!("{}: node {id}: {e}", doc.path.display()))?;
        let tolerance = node["tessellation_tolerance"].as_f64().unwrap_or(0.05);
        if !tolerance.is_finite() || tolerance <= 0. {
            return Err(format!(
                "{}: node {id}: invalid tessellation_tolerance",
                doc.path.display()
            ));
        }
        if !reuse.is_empty()
            && let Ok(f) = fingerprint(doc, id)
            && let Some(body) = reuse.get(&reuse_key(&f, tolerance))
        {
            out.push(BodyGeometry { node_id: id.into(), ..body.clone() });
            continue;
        }
        let mut ctx = Context {
            body: BodyGeometry {
                node_id: id.into(),
                ..Default::default()
            },
            cancelled,
        };
        let mut error = [0 as c_char; 1024];
        let matrices: Vec<f64> = input
            .matrices
            .iter()
            .flat_map(|m| m.iter().copied())
            .collect();
        // Synchronous call: pointers remain valid, callbacks never retain them, native
        // handles are constructed/queried/destroyed before returning on this thread.
        let result = unsafe {
            sim_cad_query(
                input.bytes.as_ptr(),
                input.bytes.len(),
                input.primitive,
                input.args.as_ptr(),
                matrices.as_ptr(),
                input.matrices.len(),
                input.volume,
                tolerance,
                &mut ctx as *mut _ as *mut c_void,
                property,
                vertex,
                triangle,
                cancel,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if result != 0 {
            return Err(format!(
                "{}: node {id}: {}: {}",
                doc.path.display(),
                input.content,
                unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy()
            ));
        }
        if cancelled() {
            return Err(format!(
                "{}: node {id}: cancelled after native query",
                doc.path.display()
            ));
        }
        out.push(ctx.body);
    }
    Ok(out)
}

/// A mesh node's display geometry: its `mesh/<id>.npz` triangles placed by
/// the node's own transform. It has no exact solid, so no properties.
fn mesh_geometry(doc: &ArchiveDocument, node: &Value, id: &str) -> Result<BodyGeometry, String> {
    let entry = format!("mesh/{id}.npz");
    let mesh = crate::mesh::from_npz(doc.entry(&entry).ok_or_else(|| format!("missing {entry}"))?)?;
    let m = transform(&node["transform"], false)?;
    let rotate = |v: [f64; 3]| [0, 1, 2].map(|i| m[i * 4] * v[0] + m[i * 4 + 1] * v[1] + m[i * 4 + 2] * v[2]);
    let place = |v: [f64; 3]| { let r = rotate(v); [r[0] + m[3], r[1] + m[7], r[2] + m[11]] };
    let unit = |v: [f64; 3]| { let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt(); if l > 0. { v.map(|x| x / l) } else { v } };
    Ok(BodyGeometry {
        node_id: id.into(),
        vertices_mm: mesh.vertices.iter().map(|v| place(v.map(f64::from))).collect(),
        normals: mesh.normals.iter().map(|n| unit(rotate(n.map(f64::from)))).collect(),
        triangles: mesh.triangles.iter().map(|t| t.map(|i| i as u32)).collect(),
        triangle_faces: mesh.triangle_face.iter().map(|f| *f as u32).collect(),
        properties: GeometryProperties::default(),
        solids: Vec::new(),
    })
}

/// The key [`load_geometry_reusing`] looks a body up by.
pub fn reuse_key(fingerprint: &str, tolerance: f64) -> String {
    format!("{fingerprint}@{tolerance}")
}

fn component_input<'a>(doc: &'a ArchiveDocument, n: &Value) -> Result<Input<'a>, String> {
    let id = n["id"].as_str().ok_or("component member id missing")?;
    let recipe = doc
        .recipes
        .get(id)
        .ok_or_else(|| format!("component member {id}: no reconstructed geometry"))?;
    let bytes = match doc.entry(&recipe.content) {
        Some(bytes) => bytes,
        None if recipe.primitive != 0 => &[],
        None => return Err(format!("missing {}", recipe.content)),
    };
    Ok(Input {
        bytes,
        primitive: recipe.primitive,
        args: recipe.args,
        matrices: recipe.matrices.clone(),
        volume: n["body_kind"].as_str().unwrap_or("solid") == "solid",
        content: recipe.content.clone(),
    })
}

/// Node `id`'s world-placed B-rep: a body's own entry; an instance's source
/// mirrored and placed; a component member's recipe (primitive or entry)
/// with its placements. What the viewer's topology, sections and exports read.
pub fn resolved_brep(doc: &ArchiveDocument, id: &str) -> Result<Vec<u8>, String> {
    let input = resolve(doc, id, &mut HashSet::new())?;
    let a = input.args;
    let mut bytes = match input.primitive {
        0 => input.bytes.to_vec(),
        1 => crate::kernel::build(&crate::kernel::Shape::Box { corner: [a[0], a[1], a[2]], size: [a[3], a[4], a[5]] }, &|| false)?,
        2 => crate::kernel::build(&crate::kernel::Shape::Cylinder { base: [a[0], a[1], a[2]], axis: [a[3], a[4], a[5]], radius: a[6], height: a[7] }, &|| false)?,
        other => return Err(format!("unknown component primitive {other}")),
    };
    for m in &input.matrices {
        bytes = crate::kernel::build(&crate::kernel::Shape::Transform { body: &bytes, matrix: *m }, &|| false)?;
    }
    Ok(bytes)
}

/// A body that is not a node (a split piece, a coupon) from its B-rep
/// bytes: tessellated at `tolerance` mm, with its exact properties when
/// `solid` (volume, centroid, inertia).
pub fn body_geometry(brep: &[u8], solid: bool, tolerance: f64, cancelled: &dyn Fn() -> bool) -> Result<BodyGeometry, String> {
    if !(tolerance.is_finite() && tolerance > 0.) {
        return Err("tolerance must be a positive length".into());
    }
    let _guard = kernel_lock(cancelled)?;
    let mut ctx = Context { body: BodyGeometry::default(), cancelled };
    let mut error = [0 as c_char; 1024];
    let args = [0f64; 8];
    let matrices: Vec<f64> = Vec::new();
    // Synchronous: as `load_geometry_reusing`.
    let result = unsafe {
        sim_cad_query(brep.as_ptr(), brep.len(), 0, args.as_ptr(), matrices.as_ptr(), 0, solid, tolerance, &mut ctx as *mut _ as *mut c_void, property, vertex, triangle, cancel, error.as_mut_ptr(), error.len())
    };
    if result != 0 {
        return Err(unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy().into_owned());
    }
    Ok(ctx.body)
}

/// Node `id` tessellated at `tolerance` (mm) instead of its own (an export's chord tolerance).
pub fn tessellate_node(doc: &ArchiveDocument, id: &str, tolerance: f64, cancelled: &dyn Fn() -> bool) -> Result<BodyGeometry, String> {
    if !(tolerance.is_finite() && tolerance > 0.) {
        return Err("tolerance must be a positive length".into());
    }
    let input = resolve(doc, id, &mut HashSet::new())?;
    let _guard = kernel_lock(cancelled)?;
    let mut ctx = Context { body: BodyGeometry { node_id: id.into(), ..Default::default() }, cancelled };
    let mut error = [0 as c_char; 1024];
    let matrices: Vec<f64> = input.matrices.iter().flat_map(|m| m.iter().copied()).collect();
    // Synchronous: as `load_geometry_reusing`.
    let result = unsafe {
        sim_cad_query(input.bytes.as_ptr(), input.bytes.len(), input.primitive, input.args.as_ptr(), matrices.as_ptr(), input.matrices.len(), input.volume, tolerance, &mut ctx as *mut _ as *mut c_void, property, vertex, triangle, cancel, error.as_mut_ptr(), error.len())
    };
    if result != 0 {
        return Err(format!("node {id}: {}", unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy()));
    }
    Ok(ctx.body)
}
