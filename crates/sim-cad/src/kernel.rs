//! Building B-reps in-process: primitives, a rigid/uniform-scale transform,
//! booleans, fillets, chamfers and polygon extrusions, each handing back
//! BRepTools text (the `brep/<id>.brep` entry format RoboCAD writes), plus the
//! exact plane section and a face/edge listing. Reference: RoboCAD's
//! kernel/occt.py (`box`, `transform`, `fillet`, `chamfer`, `section`,
//! `faces`, `edges`). Face and edge indices are RoboCAD's (`occ_faces`,
//! `occ_edges`: explorer order, shared sub-shapes once).
use serde::Serialize;
use std::ffi::{CStr, c_char, c_void};

unsafe extern "C" {
    fn sim_cad_build(
        op: i32,
        inputs: *const *const u8,
        sizes: *const usize,
        input_count: usize,
        args: *const f64,
        arg_count: usize,
        ints: *const i32,
        int_count: usize,
        context: *mut c_void,
        out: extern "C" fn(*mut c_void, *const u8, usize),
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
    fn sim_cad_section(
        bytes: *const u8,
        size: usize,
        plane: *const f64,
        context: *mut c_void,
        line: extern "C" fn(*mut c_void, *const f64, usize),
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
    fn sim_cad_topology(
        bytes: *const u8,
        size: usize,
        context: *mut c_void,
        face: extern "C" fn(*mut c_void, u32, i32, *const f64),
        edge: extern "C" fn(*mut c_void, u32, i32, *const f64),
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
}

/// One shape to build (millimetres, the archive frame).
#[derive(Clone, Debug)]
pub enum Shape<'a> {
    Box { corner: [f64; 3], size: [f64; 3] },
    Cylinder { base: [f64; 3], axis: [f64; 3], radius: f64, height: f64 },
    Sphere { center: [f64; 3], radius: f64 },
    Cone { base: [f64; 3], axis: [f64; 3], radius1: f64, radius2: f64, height: f64 },
    /// `matrix`: a row-major 3×4 affine (`placement`).
    Transform { body: &'a [u8], matrix: [f64; 12] },
    /// The first body with the others added.
    Fuse(Vec<&'a [u8]>),
    /// The first body with the others removed.
    Cut(Vec<&'a [u8]>),
    /// The part every body shares.
    Common(Vec<&'a [u8]>),
    /// `edges` empty: every line or circle edge (RoboCAD's `fillet_all`).
    Fillet { body: &'a [u8], radius: f64, edges: Vec<i32> },
    /// `edges` empty: every edge.
    Chamfer { body: &'a [u8], distance: f64, edges: Vec<i32> },
    /// Closed planar polygon loops (the outer first, then holes) swept along `direction`.
    Extrude { loops: Vec<Vec<[f64; 3]>>, direction: [f64; 3] },
}

extern "C" fn take_bytes(ctx: *mut c_void, data: *const u8, size: usize) {
    let out = unsafe { &mut *(ctx as *mut Vec<u8>) };
    *out = unsafe { std::slice::from_raw_parts(data, size) }.to_vec();
}

fn error_text(error: &[c_char]) -> String {
    unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy().into_owned()
}

/// Build `shape`; its B-rep text, or the kernel's reason.
pub fn build(shape: &Shape, cancelled: &dyn Fn() -> bool) -> Result<Vec<u8>, String> {
    let (op, inputs, args, ints): (i32, Vec<&[u8]>, Vec<f64>, Vec<i32>) = match shape {
        Shape::Box { corner, size } => (1, vec![], [corner.as_slice(), size].concat(), vec![]),
        Shape::Cylinder { base, axis, radius, height } => (2, vec![], [base.as_slice(), axis, &[*radius, *height]].concat(), vec![]),
        Shape::Sphere { center, radius } => (3, vec![], [center.as_slice(), &[*radius]].concat(), vec![]),
        Shape::Cone { base, axis, radius1, radius2, height } => (4, vec![], [base.as_slice(), axis, &[*radius1, *radius2, *height]].concat(), vec![]),
        Shape::Transform { body, matrix } => (5, vec![*body], matrix.to_vec(), vec![]),
        Shape::Fuse(bodies) => (6, bodies.clone(), vec![], vec![]),
        Shape::Cut(bodies) => (7, bodies.clone(), vec![], vec![]),
        Shape::Common(bodies) => (8, bodies.clone(), vec![], vec![]),
        Shape::Fillet { body, radius, edges } => (9, vec![*body], vec![*radius], edges.clone()),
        Shape::Chamfer { body, distance, edges } => (10, vec![*body], vec![*distance], edges.clone()),
        Shape::Extrude { loops, direction } => {
            let mut args = direction.to_vec();
            for p in loops.iter().flatten() {
                args.extend_from_slice(p);
            }
            (11, vec![], args, loops.iter().map(|l| l.len() as i32).collect())
        }
    };
    if let Some(bad) = args.iter().find(|v| !v.is_finite()) {
        return Err(format!("non-finite argument {bad}"));
    }
    let _guard = crate::geometry::kernel_lock(cancelled)?;
    let pointers: Vec<*const u8> = inputs.iter().map(|b| b.as_ptr()).collect();
    let sizes: Vec<usize> = inputs.iter().map(|b| b.len()).collect();
    let mut out: Vec<u8> = Vec::new();
    let mut error = [0 as c_char; 1024];
    // Synchronous: every pointer outlives the call; the callback copies.
    let result = unsafe {
        sim_cad_build(
            op,
            pointers.as_ptr(),
            sizes.as_ptr(),
            inputs.len(),
            args.as_ptr(),
            args.len(),
            ints.as_ptr(),
            ints.len(),
            &mut out as *mut Vec<u8> as *mut c_void,
            take_bytes,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if result != 0 {
        return Err(error_text(&error));
    }
    Ok(out)
}

extern "C" fn take_line(ctx: *mut c_void, points: *const f64, count: usize) {
    let out = unsafe { &mut *(ctx as *mut Vec<Vec<[f64; 3]>>) };
    let flat = unsafe { std::slice::from_raw_parts(points, count * 3) };
    out.push(flat.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect());
}

/// The exact section of `body` by the plane through `origin` with `normal`:
/// polylines (a line edge's two ends, any other edge 24 samples), empty when
/// the plane misses the body.
pub fn section(body: &[u8], origin: [f64; 3], normal: [f64; 3], cancelled: &dyn Fn() -> bool) -> Result<Vec<Vec<[f64; 3]>>, String> {
    if origin.iter().chain(&normal).any(|v| !v.is_finite()) || normal.iter().map(|v| v * v).sum::<f64>() < 1e-18 {
        return Err("section plane needs a finite origin and a nonzero normal".into());
    }
    let _guard = crate::geometry::kernel_lock(cancelled)?;
    let plane = [origin, normal].concat();
    let mut out: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut error = [0 as c_char; 1024];
    let result = unsafe {
        sim_cad_section(body.as_ptr(), body.len(), plane.as_ptr(), &mut out as *mut _ as *mut c_void, take_line, error.as_mut_ptr(), error.len())
    };
    if result != 0 {
        return Err(error_text(&error));
    }
    Ok(out)
}

/// A face by its index: OCCT's surface type, area centroid, outward normal
/// at the middle of its parameter range, area.
#[derive(Clone, Debug, Serialize)]
pub struct FaceInfo {
    pub index: u32,
    pub kind: &'static str,
    pub center: [f64; 3],
    pub normal: [f64; 3],
    pub area_mm2: f64,
}

/// An edge by its index: OCCT's curve type, midpoint, length and ends.
#[derive(Clone, Debug, Serialize)]
pub struct EdgeInfo {
    pub index: u32,
    pub kind: &'static str,
    pub midpoint: [f64; 3],
    pub length_mm: f64,
    pub start: [f64; 3],
    pub end: [f64; 3],
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Topology {
    pub faces: Vec<FaceInfo>,
    pub edges: Vec<EdgeInfo>,
}

fn surface_kind(t: i32) -> &'static str {
    ["plane", "cylinder", "cone", "sphere", "torus", "bezier", "bspline", "revolution", "extrusion", "offset"].get(t as usize).copied().unwrap_or("other")
}
fn curve_kind(t: i32) -> &'static str {
    ["line", "circle", "ellipse", "hyperbola", "parabola", "bezier", "bspline", "offset"].get(t as usize).copied().unwrap_or("other")
}
fn round(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}
fn r3(p: &[f64]) -> [f64; 3] {
    [round(p[0]), round(p[1]), round(p[2])]
}
extern "C" fn take_face(ctx: *mut c_void, index: u32, kind: i32, v: *const f64) {
    let out = unsafe { &mut *(ctx as *mut Topology) };
    let v = unsafe { std::slice::from_raw_parts(v, 7) };
    out.faces.push(FaceInfo { index, kind: surface_kind(kind), center: r3(&v[0..3]), normal: r3(&v[3..6]), area_mm2: round(v[6]) });
}
extern "C" fn take_edge(ctx: *mut c_void, index: u32, kind: i32, v: *const f64) {
    let out = unsafe { &mut *(ctx as *mut Topology) };
    let v = unsafe { std::slice::from_raw_parts(v, 10) };
    out.edges.push(EdgeInfo { index, kind: curve_kind(kind), midpoint: r3(&v[0..3]), length_mm: round(v[3]), start: r3(&v[4..7]), end: r3(&v[7..10]) });
}

/// Every face and edge of `body` by RoboCAD's indices.
pub fn topology(body: &[u8], cancelled: &dyn Fn() -> bool) -> Result<Topology, String> {
    let _guard = crate::geometry::kernel_lock(cancelled)?;
    let mut out = Topology::default();
    let mut error = [0 as c_char; 1024];
    let result = unsafe {
        sim_cad_topology(body.as_ptr(), body.len(), &mut out as *mut _ as *mut c_void, take_face, take_edge, error.as_mut_ptr(), error.len())
    };
    if result != 0 {
        return Err(error_text(&error));
    }
    Ok(out)
}

/// RoboCAD's `Kernel.transform` as a row-major 3×4 matrix: scale about
/// `center`, then rotate `angle_deg` about `axis` through `center`, then
/// translate (`T · S · R`, as occt.py composes it).
pub fn placement(translation: [f64; 3], axis: Option<[f64; 3]>, angle_deg: f64, center: [f64; 3], scale: f64) -> Result<[f64; 12], String> {
    if !(scale.is_finite() && scale > 0.) {
        return Err("scale must be a positive number".into());
    }
    let mut r = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    if let Some(a) = axis.filter(|_| angle_deg.abs() > 1e-12) {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        if !(l > 1e-12) {
            return Err("rotation axis must be nonzero".into());
        }
        let [x, y, z] = a.map(|v| v / l);
        let (s, c) = angle_deg.to_radians().sin_cos();
        let k = 1. - c;
        r = [
            [k * x * x + c, k * x * y - s * z, k * x * z + s * y],
            [k * x * y + s * z, k * y * y + c, k * y * z - s * x],
            [k * x * z - s * y, k * y * z + s * x, k * z * z + c],
        ];
    }
    // p' = s·R(p − c) + c + t
    let mut m = [0.; 12];
    for i in 0..3 {
        for j in 0..3 {
            m[i * 4 + j] = scale * r[i][j];
        }
        let rc: f64 = (0..3).map(|j| r[i][j] * center[j]).sum();
        m[i * 4 + 3] = center[i] - scale * rc + translation[i];
    }
    Ok(m)
}

/// `matrix` applied to point `p`.
pub fn apply(matrix: &[f64; 12], p: [f64; 3]) -> [f64; 3] {
    let mut out = [0.; 3];
    for i in 0..3 {
        out[i] = matrix[i * 4] * p[0] + matrix[i * 4 + 1] * p[1] + matrix[i * 4 + 2] * p[2] + matrix[i * 4 + 3];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_rotates_about_the_centre_then_translates() {
        let m = placement([10., 0., 0.], Some([0., 0., 1.]), 90., [1., 0., 0.], 1.).unwrap();
        let p = apply(&m, [2., 0., 0.]);
        assert!((p[0] - 11.).abs() < 1e-12 && (p[1] - 1.).abs() < 1e-12 && p[2].abs() < 1e-12, "{p:?}");
    }

    #[test]
    fn a_box_builds_sections_and_lists_its_topology() {
        let never = || false;
        let body = build(&Shape::Box { corner: [0.; 3], size: [10., 20., 30.] }, &never).unwrap();
        let topo = topology(&body, &never).unwrap();
        assert_eq!((topo.faces.len(), topo.edges.len()), (6, 12));
        assert!(topo.faces.iter().all(|f| f.kind == "plane"));
        let lines = section(&body, [5., 10., 15.], [0., 0., 1.], &never).unwrap();
        assert_eq!(lines.len(), 4, "a box cut across makes a rectangle");
        let filleted = build(&Shape::Fillet { body: &body, radius: 2., edges: vec![0] }, &never).unwrap();
        assert_eq!(topology(&filleted, &never).unwrap().faces.len(), 7);
        let cutter = build(&Shape::Cylinder { base: [5., 10., -1.], axis: [0., 0., 1.], radius: 3., height: 32. }, &never).unwrap();
        let holed = build(&Shape::Cut(vec![&body, &cutter]), &never).unwrap();
        assert_eq!(topology(&holed, &never).unwrap().faces.iter().filter(|f| f.kind == "cylinder").count(), 1);
        let square = vec![[0., 0., 0.], [4., 0., 0.], [4., 4., 0.], [0., 4., 0.]];
        let hole = vec![[1., 1., 0.], [3., 1., 0.], [3., 3., 0.], [1., 3., 0.]];
        let prism = build(&Shape::Extrude { loops: vec![square, hole], direction: [0., 0., 5.] }, &never).unwrap();
        assert_eq!(topology(&prism, &never).unwrap().faces.len(), 10);
        assert!(build(&Shape::Box { corner: [0.; 3], size: [0., 1., 1.] }, &never).unwrap_err().contains("positive"));
    }
}

unsafe extern "C" {
    fn sim_cad_op(
        op: i32,
        inputs: *const *const u8,
        sizes: *const usize,
        input_count: usize,
        args: *const f64,
        arg_count: usize,
        ints: *const i32,
        int_count: usize,
        context: *mut c_void,
        out: extern "C" fn(*mut c_void, i32, *const u8, usize),
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
    fn sim_cad_measure(
        op: i32,
        inputs: *const *const u8,
        sizes: *const usize,
        input_count: usize,
        args: *const f64,
        arg_count: usize,
        ints: *const i32,
        int_count: usize,
        context: *mut c_void,
        out: extern "C" fn(*mut c_void, *const f64, usize),
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
}

/// A result shape's kind, as RoboCAD names a body's (`body_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Solid,
    Sheet,
    Wire,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Solid => "solid",
            Kind::Sheet => "sheet",
            Kind::Wire => "wire",
        }
    }
}

/// One result of a modelling operation: its kind and B-rep text.
#[derive(Clone, Debug)]
pub struct Built {
    pub kind: Kind,
    pub brep: Vec<u8>,
}

extern "C" fn take_shape(ctx: *mut c_void, kind: i32, data: *const u8, size: usize) {
    let out = unsafe { &mut *(ctx as *mut Vec<Built>) };
    let kind = match kind {
        0 => Kind::Solid,
        1 => Kind::Sheet,
        _ => Kind::Wire,
    };
    out.push(Built { kind, brep: unsafe { std::slice::from_raw_parts(data, size) }.to_vec() });
}
extern "C" fn take_numbers(ctx: *mut c_void, data: *const f64, size: usize) {
    let out = unsafe { &mut *(ctx as *mut Vec<f64>) };
    *out = unsafe { std::slice::from_raw_parts(data, size) }.to_vec();
}

/// The modelling operations of `native/ops.cpp` (RoboCAD's `OcctKernel`),
/// by code; see that file for each one's inputs, numbers and indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    Extrude = 1,
    Revolve = 2,
    Sweep = 3,
    Pipe = 4,
    Loft = 5,
    Fill = 6,
    Bridge = 7,
    Join = 8,
    SplitPlane = 9,
    SplitTool = 10,
    PushPull = 11,
    OffsetFaces = 12,
    MoveFaces = 13,
    RotateFaces = 14,
    SetRadius = 15,
    Draft = 16,
    DeleteFaces = 17,
    Imprint = 18,
    Shell = 19,
    Thicken = 20,
    Fillet = 21,
    FilletChordal = 22,
    FullRound = 23,
    Mirror = 24,
    Unjoin = 25,
    Dissolve = 26,
    ExtractComponents = 27,
    OffsetFaceTo = 28,
    Wires = 29,
    Face = 30,
    Silhouette = 31,
    ProjectCurve = 32,
    SetControlPoints = 33,
    RaiseDegree = 34,
    RebuildFace = 35,
    ExtrudeUpTo = 36,
    Boolean = 37,
    Hlr = 38,
}

fn finite(args: &[f64]) -> Result<(), String> {
    match args.iter().find(|v| !v.is_finite()) {
        Some(bad) => Err(format!("non-finite argument {bad}")),
        None => Ok(()),
    }
}

/// Run modelling operation `op`: every result shape, in order.
pub fn op(op: Op, inputs: &[&[u8]], args: &[f64], ints: &[i32], cancelled: &dyn Fn() -> bool) -> Result<Vec<Built>, String> {
    finite(args)?;
    let _guard = crate::geometry::kernel_lock(cancelled)?;
    let pointers: Vec<*const u8> = inputs.iter().map(|b| b.as_ptr()).collect();
    let sizes: Vec<usize> = inputs.iter().map(|b| b.len()).collect();
    let mut out: Vec<Built> = Vec::new();
    let mut error = [0 as c_char; 1024];
    let result = unsafe {
        sim_cad_op(op as i32, pointers.as_ptr(), sizes.as_ptr(), inputs.len(), args.as_ptr(), args.len(), ints.as_ptr(), ints.len(), &mut out as *mut _ as *mut c_void, take_shape, error.as_mut_ptr(), error.len())
    };
    if result != 0 {
        return Err(error_text(&error));
    }
    if out.is_empty() {
        return Err("the operation produced no geometry".into());
    }
    Ok(out)
}

/// [`op`] with exactly one result.
pub fn op1(code: Op, inputs: &[&[u8]], args: &[f64], ints: &[i32], cancelled: &dyn Fn() -> bool) -> Result<Built, String> {
    Ok(op(code, inputs, args, ints, cancelled)?.swap_remove(0))
}

/// The numeric queries of `native/ops.cpp` (`sim_cad_measure`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Measure {
    ControlPoints = 1,
    Continuity = 2,
    RayHits = 3,
    Distance = 4,
    Contains = 5,
    Valid = 6,
    Bounds = 7,
    FaceInfo = 8,
    Full = 9,
    CurvatureComb = 10,
    RobocadStamp = 11,
}

/// Run query `m`: its numbers.
pub fn measure(m: Measure, inputs: &[&[u8]], args: &[f64], ints: &[i32]) -> Result<Vec<f64>, String> {
    finite(args)?;
    let _guard = crate::geometry::kernel_lock(&|| false)?;
    let pointers: Vec<*const u8> = inputs.iter().map(|b| b.as_ptr()).collect();
    let sizes: Vec<usize> = inputs.iter().map(|b| b.len()).collect();
    let mut out: Vec<f64> = Vec::new();
    let mut error = [0 as c_char; 1024];
    let result = unsafe {
        sim_cad_measure(m as i32, pointers.as_ptr(), sizes.as_ptr(), inputs.len(), args.as_ptr(), args.len(), ints.as_ptr(), ints.len(), &mut out as *mut _ as *mut c_void, take_numbers, error.as_mut_ptr(), error.len())
    };
    if result != 0 {
        return Err(error_text(&error));
    }
    Ok(out)
}

/// One face's description (`Measure::FaceInfo`): RoboCAD's `FaceRef` fields.
#[derive(Clone, Debug, Serialize)]
pub struct FaceRef {
    pub normal: [f64; 3],
    pub centroid: [f64; 3],
    pub radius: f64,
    pub cylinder: bool,
    pub plane: bool,
    pub axis_point: Option<[f64; 3]>,
    pub axis_dir: Option<[f64; 3]>,
    pub hole: bool,
}

/// Face `face` of `body` described.
pub fn face_ref(body: &[u8], face: i32) -> Result<FaceRef, String> {
    let v = measure(Measure::FaceInfo, &[body], &[], &[face])?;
    let axis = v[8] != 0.;
    Ok(FaceRef {
        normal: [v[0], v[1], v[2]],
        centroid: [v[3], v[4], v[5]],
        radius: v[6],
        plane: v[7] == 0.,
        cylinder: v[7] == 1.,
        axis_point: axis.then_some([v[9], v[10], v[11]]),
        axis_dir: axis.then_some([v[12], v[13], v[14]]),
        hole: v[15] != 0.,
    })
}

fn surface_name(t: f64) -> &'static str {
    ["plane", "cylinder", "cone", "sphere", "torus", "bezier", "bspline", "revolution", "extrusion", "offset"].get(t as usize).copied().unwrap_or("other")
}
fn curve_name(t: f64) -> &'static str {
    match t as i64 {
        0 => "line",
        1 => "circle",
        2 => "ellipse",
        5 | 6 => "bspline",
        _ => "other",
    }
}

/// Every face, edge (with its sampled polyline: a line's ends, any other
/// edge `samples` points) and vertex of `body`, as RoboCAD's `/faces`,
/// `/edges?samples=N` and `/vertices` answer them.
pub fn full_topology(body: &[u8], samples: i32) -> Result<serde_json::Value, String> {
    let v = measure(Measure::Full, &[body], &[], &[samples])?;
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[f64], String> {
        let s = v.get(at..at + n).ok_or("the topology answer is short")?;
        at += n;
        Ok(s)
    };
    let p3 = |s: &[f64]| serde_json::json!([round(s[0]), round(s[1]), round(s[2])]);
    let nf = take(1)?[0] as usize;
    let mut faces = Vec::with_capacity(nf);
    for i in 0..nf {
        let f = take(19)?.to_vec();
        let axis = f[8] != 0.;
        faces.push(serde_json::json!({"index": i, "kind": surface_name(f[0]), "centroid": p3(&f[1..4]), "normal": p3(&f[4..7]), "area": round(f[7]),
            "axis_point": if axis { p3(&f[9..12]) } else { serde_json::Value::Null }, "axis_dir": if axis { p3(&f[12..15]) } else { serde_json::Value::Null },
            "radius": if axis { serde_json::json!(round(f[15])) } else { serde_json::Value::Null }, "point": p3(&f[16..19])}));
    }
    let ne = take(1)?[0] as usize;
    let mut edges = Vec::with_capacity(ne);
    for _ in 0..ne {
        let e = take(17)?.to_vec();
        let (i, e) = (e[0] as usize, e[1..].to_vec());
        let k = take(1)?[0] as usize;
        let pts: Vec<serde_json::Value> = take(3 * k)?.chunks_exact(3).map(p3).collect();
        let circle = e[11] != 0.;
        edges.push(serde_json::json!({"index": i, "kind": curve_name(e[0]), "midpoint": p3(&e[1..4]), "length": round(e[4]), "start": p3(&e[5..8]), "end": p3(&e[8..11]),
            "center": if circle { p3(&e[12..15]) } else { serde_json::Value::Null }, "radius": if circle { serde_json::json!(round(e[15])) } else { serde_json::Value::Null }, "points": pts}));
    }
    let nv = take(1)?[0] as usize;
    let mut vertices = Vec::with_capacity(nv);
    for i in 0..nv {
        let p = take(3)?.to_vec();
        vertices.push(serde_json::json!({"index": i, "point": p3(&p)}));
    }
    Ok(serde_json::json!({"faces": faces, "edges": edges, "vertices": vertices}))
}

unsafe extern "C" {
    fn sim_cad_export(format: i32, inputs: *const *const u8, sizes: *const usize, input_count: usize, path: *const c_char, schema: *const c_char, error: *mut c_char, error_size: usize) -> i32;
    fn sim_cad_import(format: i32, path: *const c_char, scale: f64, context: *mut c_void, out: extern "C" fn(*mut c_void, i32, *const u8, usize), error: *mut c_char, error_size: usize) -> i32;
}

/// An exchange format OCCT reads and writes here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exchange {
    Step = 1,
    Iges = 2,
}

/// Write `bodies` (B-rep text) to `path` as STEP (`schema` AP203 | AP214 |
/// AP242) or IGES, millimetres. Geometry only: no names or colours.
pub fn export(format: Exchange, bodies: &[&[u8]], path: &std::path::Path, schema: &str) -> Result<(), String> {
    let _guard = crate::geometry::kernel_lock(&|| false)?;
    let pointers: Vec<*const u8> = bodies.iter().map(|b| b.as_ptr()).collect();
    let sizes: Vec<usize> = bodies.iter().map(|b| b.len()).collect();
    let p = std::ffi::CString::new(path.to_string_lossy().as_bytes()).map_err(|_| "the path holds a NUL byte")?;
    let s = std::ffi::CString::new(schema).map_err(|_| "the schema holds a NUL byte")?;
    let mut error = [0 as c_char; 1024];
    let result = unsafe { sim_cad_export(format as i32, pointers.as_ptr(), sizes.as_ptr(), bodies.len(), p.as_ptr(), s.as_ptr(), error.as_mut_ptr(), error.len()) };
    if result != 0 {
        return Err(error_text(&error));
    }
    Ok(())
}

/// Every solid of a STEP or IGES file (else its shells, else the whole
/// shape), scaled by `scale` (1 for millimetres).
pub fn import(format: Exchange, path: &std::path::Path, scale: f64) -> Result<Vec<Built>, String> {
    let _guard = crate::geometry::kernel_lock(&|| false)?;
    let p = std::ffi::CString::new(path.to_string_lossy().as_bytes()).map_err(|_| "the path holds a NUL byte")?;
    let mut out: Vec<Built> = Vec::new();
    let mut error = [0 as c_char; 1024];
    let result = unsafe { sim_cad_import(format as i32, p.as_ptr(), scale, &mut out as *mut _ as *mut c_void, take_shape, error.as_mut_ptr(), error.len()) };
    if result != 0 {
        return Err(error_text(&error));
    }
    Ok(out)
}
