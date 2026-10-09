//! Printing a CAD part (RoboCAD's `print_*.py`, ported): print studies
//! written from the archive, split for printing with joints, whole-or-split
//! for strength, plates (3MF) from a plan, assembly guides with an exploded
//! view, and test coupons. The stress check and planner are `sim-print`'s,
//! run by the host ([`Runner`]: the native viewer runs them in process).
//!
//! Every job reads a snapshot of the archive and never changes it; a job
//! that publishes returns an [`crate::Edit`] that the host applies as one
//! undo step, refused when the document moved meanwhile ([`jobs`]).
//!
//! Values come from the print registry (`library/printing/registry.json`,
//! `sim_print::registry`); none is copied here.
pub mod assembly;
pub mod coupons;
pub mod jobs;
pub mod plates;
pub mod split;
pub mod strength_split;
pub mod study;

use crate::geometry::BodyGeometry;
use crate::kernel::{self, Built, Kind, Measure, Op, Shape};
use serde_json::Value;
use sim_print::registry::Loaded;

pub type V3 = [f64; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn unit(a: V3) -> V3 {
    let l = norm(a);
    if l > 0.0 { scale(a, 1.0 / l) } else { a }
}
/// Python's `round(x, n)`.
pub fn round(x: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    (x * f).round() / f
}

// ---- the print registry -------------------------------------------------

/// The print registry as CAD reads it (RoboCAD's `print_registry`): typed
/// lookups over its JSON, and its SHA-256 for every result.
pub struct Registry {
    pub loaded: Loaded,
}

/// A heat-set insert's geometry (mm).
#[derive(Clone, Copy, Debug)]
pub struct Insert {
    pub hole_mm: f64,
    pub depth_mm: f64,
    pub knurl_mm: f64,
    pub length_mm: f64,
}

/// A socket head screw's geometry for one size (mm, N).
#[derive(Clone, Debug)]
pub struct Screw {
    pub clearance_mm: f64,
    pub head_mm: f64,
    pub counterbore_mm: (f64, f64),
    pub lengths_mm: Vec<f64>,
    pub proof_load_n: f64,
}

/// `{"value": x}` or `x`.
pub fn value(v: &Value) -> Option<f64> {
    v.get("value").unwrap_or(v).as_f64()
}

impl Registry {
    /// The registry at `SIM_PRINT_REGISTRY`, else the repository's.
    pub fn load() -> Result<Registry, String> {
        Ok(Registry { loaded: sim_print::registry::load(&sim_print::registry::default_path())? })
    }
    pub fn sha256(&self) -> &str {
        &self.loaded.sha256
    }
    /// The box one piece must fit in: build volume less the edge margin in x and y.
    pub fn usable_mm(&self, printer: &str) -> Result<V3, String> {
        Ok(self.loaded.registry.printer(printer)?.usable_mm())
    }
    pub fn margin_mm(&self, printer: &str) -> Result<f64, String> {
        Ok(self.loaded.registry.printer(printer)?.margin_mm.value)
    }
    /// A joint value by dotted path, numbers unwrapped (RoboCAD's `joint`).
    pub fn joint(&self, path: &str) -> Result<&Value, String> {
        let mut v = &self.loaded.registry.joints;
        for part in path.split('.') {
            v = v.get(part).ok_or_else(|| format!("joints.{path}: `{part}` is missing from the print registry"))?;
        }
        Ok(v)
    }
    pub fn number(&self, path: &str) -> Result<f64, String> {
        value(self.joint(path)?).ok_or_else(|| format!("joints.{path} is not a number"))
    }
    pub fn numbers(&self, path: &str) -> Result<Vec<f64>, String> {
        self.joint(path)?.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).ok_or_else(|| format!("joints.{path} is not a list"))
    }
    pub fn insert(&self, size: &str) -> Result<Insert, String> {
        let v = self.joint("heat_set_insert")?.get(size).ok_or_else(|| format!("joints.heat_set_insert has no {size:?} in the print registry"))?;
        let n = |k: &str| v.get(k).and_then(Value::as_f64).ok_or_else(|| format!("joints.heat_set_insert.{size}.{k} is missing"));
        Ok(Insert { hole_mm: n("hole_mm")?, depth_mm: n("depth_mm")?, knurl_mm: n("knurl_mm")?, length_mm: n("length_mm")? })
    }
    pub fn screw(&self, size: &str) -> Result<Screw, String> {
        let s = self.joint("screw")?;
        let pick = |k: &str| s.get(k).and_then(|m| m.get(size)).ok_or_else(|| format!("joints.screw.{k} has no {size:?}"));
        let cb = pick("counterbore_mm")?.as_array().filter(|a| a.len() == 2).ok_or_else(|| format!("joints.screw.counterbore_mm.{size} is not [diameter, depth]"))?;
        Ok(Screw {
            clearance_mm: pick("clearance_mm")?.as_f64().unwrap_or(0.0),
            head_mm: pick("head_mm")?.as_f64().unwrap_or(0.0),
            counterbore_mm: (cb[0].as_f64().unwrap_or(0.0), cb[1].as_f64().unwrap_or(0.0)),
            lengths_mm: s["lengths_mm"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default(),
            proof_load_n: pick("proof_load_n")?.as_f64().unwrap_or(0.0),
        })
    }
}

// ---- geometry of B-rep bytes ---------------------------------------------

/// The kernel calls the print code makes on bodies that are not nodes (the
/// pieces of a split, coupons): RoboCAD's `GeometryKernel` methods.
pub struct K<'a> {
    pub cancelled: &'a dyn Fn() -> bool,
}

impl K<'_> {
    pub fn bounds(&self, body: &[u8]) -> Result<(V3, V3), String> {
        let b = kernel::measure(Measure::Bounds, &[body], &[], &[])?;
        Ok(([b[0], b[1], b[2]], [b[3], b[4], b[5]]))
    }
    pub fn size(&self, body: &[u8]) -> Result<V3, String> {
        let (lo, hi) = self.bounds(body)?;
        Ok(sub(hi, lo))
    }
    /// Tessellation and exact properties of a solid.
    pub fn geometry(&self, body: &[u8], tolerance: f64) -> Result<BodyGeometry, String> {
        crate::geometry::body_geometry(body, true, tolerance, self.cancelled)
    }
    pub fn volume_centroid(&self, body: &[u8]) -> Result<(f64, V3), String> {
        let g = self.geometry(body, 0.5)?;
        Ok((g.properties.volume_mm3, g.properties.centroid_mm))
    }
    /// The body's section by a plane: polylines (RoboCAD's `section`).
    pub fn section(&self, body: &[u8], origin: V3, normal: V3) -> Result<Vec<Vec<V3>>, String> {
        kernel::section(body, origin, normal, self.cancelled)
    }
    /// Both sides of a plane cut (RoboCAD's `cut_with_plane`).
    pub fn cut_with_plane(&self, body: &[u8], origin: V3, normal: V3) -> Result<Vec<Vec<u8>>, String> {
        Ok(kernel::op(Op::SplitPlane, &[body], &[&origin[..], &normal, &[0.0]].concat(), &[], self.cancelled)?.into_iter().map(|b| b.brep).collect())
    }
    fn boolean(&self, a: &[u8], b: &[u8], code: f64) -> Result<Vec<u8>, String> {
        Ok(kernel::op1(Op::Boolean, &[a, b], &[code], &[], self.cancelled)?.brep)
    }
    pub fn union(&self, a: &[u8], b: &[u8]) -> Result<Vec<u8>, String> {
        self.boolean(a, b, 0.0)
    }
    pub fn subtract(&self, a: &[u8], b: &[u8]) -> Result<Vec<u8>, String> {
        self.boolean(a, b, 1.0)
    }
    pub fn intersect(&self, a: &[u8], b: &[u8]) -> Result<Vec<u8>, String> {
        self.boolean(a, b, 2.0)
    }
    /// Bodies kept separate in one compound (RoboCAD's `join`).
    pub fn join(&self, bodies: &[&[u8]]) -> Result<Vec<u8>, String> {
        Ok(kernel::op1(Op::Join, bodies, &[], &[], self.cancelled)?.brep)
    }
    pub fn cylinder(&self, base: V3, axis: V3, radius: f64, height: f64) -> Result<Vec<u8>, String> {
        kernel::build(&Shape::Cylinder { base, axis: unit(axis), radius, height }, self.cancelled)
    }
    /// A box from `lo` to `hi`.
    pub fn cuboid(&self, lo: V3, hi: V3) -> Result<Vec<u8>, String> {
        let (a, b) = ([0, 1, 2].map(|i| lo[i].min(hi[i])), [0, 1, 2].map(|i| lo[i].max(hi[i])));
        kernel::build(&Shape::Box { corner: a, size: sub(b, a) }, self.cancelled)
    }
    /// A closed planar polygon swept along `direction` (its length is the
    /// distance; RoboCAD's sketch `to_body` then `extrude`).
    pub fn extrude(&self, polygon: Vec<V3>, direction: V3) -> Result<Vec<u8>, String> {
        kernel::build(&Shape::Extrude { loops: vec![polygon], direction }, self.cancelled)
    }
    pub fn translate(&self, body: &[u8], by: V3) -> Result<Vec<u8>, String> {
        let m = kernel::placement(by, None, 0.0, [0.0; 3], 1.0)?;
        kernel::build(&Shape::Transform { body, matrix: m }, self.cancelled)
    }
    pub fn rotate(&self, body: &[u8], axis: V3, degrees: f64, center: V3) -> Result<Vec<u8>, String> {
        let m = kernel::placement([0.0; 3], Some(axis), degrees, center, 1.0)?;
        kernel::build(&Shape::Transform { body, matrix: m }, self.cancelled)
    }
    /// Ray hits from `origin` along `direction`: (distance, point, face), nearest first.
    pub fn ray_hits(&self, body: &[u8], origin: V3, direction: V3) -> Result<Vec<(f64, V3, i64)>, String> {
        let raw = kernel::measure(Measure::RayHits, &[body], &[origin[0], origin[1], origin[2], direction[0], direction[1], direction[2]], &[])?;
        let mut hits: Vec<(f64, V3, i64)> = raw.chunks_exact(5).map(|h| (h[0], [h[1], h[2], h[3]], h[4] as i64)).collect();
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(hits)
    }
    pub fn contains(&self, body: &[u8], point: V3, tolerance: f64) -> Result<bool, String> {
        Ok(kernel::measure(Measure::Contains, &[body], &[point[0], point[1], point[2], tolerance], &[])?.first() == Some(&1.0))
    }
}

/// A solid built from bytes (for `Ctx::add_built`).
pub fn solid(brep: Vec<u8>) -> Built {
    Built { kind: Kind::Solid, brep }
}

/// What runs the stress check and the planner on a written study
/// (`sim-print analyze|plan STUDY --out DIR`): the host supplies it.
pub trait Runner {
    /// Run `command` (analyze | plan) on the study at `study`, writing into
    /// `out`; its report (`result.json` / `plan.json`). `progress` gets a
    /// fraction and a message; returning false from it cancels.
    fn run(&self, command: &str, study: &std::path::Path, out: &std::path::Path, progress: &dyn Fn(f64, &str) -> bool) -> Result<Value, String>;
}

/// A file-name slug (RoboCAD's `print_study.slug`).
pub fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() { c.to_lowercase().next().unwrap_or(c) } else { '-' }).collect();
    s.split('-').filter(|x| !x.is_empty()).collect::<Vec<_>>().join("-")
}

/// Run `command` on a study into its usual folder beside it
/// (`print-results` for analyze, `print-plan` for plan; RoboCAD's `Run`).
pub fn run_tool(runner: &dyn Runner, command: &str, study: &std::path::Path, progress: &dyn Fn(f64, &str) -> bool) -> Result<Value, String> {
    let dir = study.parent().unwrap_or(std::path::Path::new("."));
    let out = dir.join(if command == "analyze" { "print-results" } else { "print-plan" });
    runner.run(command, study, &out, progress)
}

/// Where print jobs write (`runs/cad-print` beside the print registry's
/// `library/`): ignored run output, reproduced from the study it holds.
pub fn runs_dir() -> std::path::PathBuf {
    let registry = sim_print::registry::default_path();
    registry.parent().and_then(|p| p.parent()).and_then(|p| p.parent()).map_or_else(|| std::path::PathBuf::from("runs/cad-print"), |root| root.join("runs").join("cad-print"))
}
