//! Model scripts: a repository `.rhai` file builds or rebuilds part of the
//! model as one undo step (RoboCAD's `model_scripts.py`, with Rhai in place
//! of Python). A script defines
//!
//! ```rhai
//! fn build(params) {   // params: the run's parameters (a map)
//!     ...              // bodies and the shared operations below
//!     #{ ... }         // a JSON-like summary (optional)
//! }
//! ```
//!
//! [`stage`] runs it on a staged edit of a snapshot (off the UI thread):
//! a failed script never touches the document. With `replace`, the nodes
//! the script's previous run created are removed first, so editing the
//! script (or its parameters) and running it again updates the model in
//! place. Each run is recorded in `robot_settings.model_scripts[path]` with
//! the script's SHA-256, the parameters and the created node ids, so a
//! saved document says how it was made. Only `.rhai` files inside the
//! repository run: REST names a script, never supplies code.
//!
//! What a script can call (mm, degrees, Z up):
//!
//! - Bodies (values, not nodes): `cylinder(base, axis, r, h)`,
//!   `cyl(r, z0, z1)` / `cyl(r, z0, z1, x, y)` (vertical), `box(corner,
//!   size)` / `box(x0, y0, z0, x1, y1, z1)`, `sphere(center, r)`,
//!   `prism(points_xy, z0, z1)`, `extrude_polygon(points_xyz, direction)`,
//!   `sketch_extrude(plane, calls, direction, distance)` (sketch calls as
//!   the CAD sketch takes them, e.g. `["slot", [a, b, width]]`),
//!   `union(a, b)`, `subtract(a, b)` (b may be an array), `intersect(a, b)`,
//!   `rotate(body, axis, deg, center)`, `rotate_z(body, deg)`,
//!   `translate(body, offset)`, `properties(body)` (volume, centroid,
//!   inertia mm⁵), `declared(body, mass_kg, source)` (a `mass_properties`
//!   block for a purchased part of declared mass).
//! - The document: `add_body(body, name, material, color, robot_meta)` (its
//!   id), `set_body(id, body)`, `body_of(id)`, and `op(name, args)` /
//!   `op(name, args, kwargs)`: any shared CAD operation (`ops::run`: group,
//!   set_ground, connect_fixed, add_joint, transform, …).
//! - GT2 belts: `gt2_pitch_radius(teeth)`, `gt2_tip_radius(teeth)`,
//!   `gt2_outline(teeth)`, `belt_path([#{center: [x, y], radius: r}, …])`.
//! - Math beyond Rhai's own: `atan2(y, x)`, `rad(deg)`, `deg(rad)`, `hypot(a, b)`.
use crate::annotations::Stamps;
use crate::archive::ArchiveDocument;
use crate::edit::Edit;
use crate::kernel::{self, Built, Kind, Op, Shape};
use crate::ops::Ctx;
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map as RMap, Scope};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Where runs are recorded in `robot_settings`.
pub const SETTING: &str = "model_scripts";

type V3 = [f64; 3];
type R<T> = Result<T, Box<EvalAltResult>>;

/// A body value in a script (B-rep bytes).
#[derive(Clone)]
pub struct Body(Arc<Vec<u8>>);

/// The repository root (beside `library/`).
pub fn repository() -> PathBuf {
    let registry = sim_print::registry::default_path();
    registry.parent().and_then(|p| p.parent()).and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

/// The script file and its repository-relative key; anything outside the
/// repository, or not a `.rhai` file, is refused.
pub fn resolve(path: &str) -> Result<(PathBuf, String), String> {
    if path.is_empty() {
        return Err("A model script run needs a path".into());
    }
    let root = std::fs::canonicalize(repository()).map_err(|e| format!("the repository: {e}"))?;
    let p = Path::new(path);
    let full = std::fs::canonicalize(if p.is_absolute() { p.to_path_buf() } else { root.join(p) }).map_err(|_| format!("{path}: not a Rhai file"))?;
    if full.extension().and_then(|e| e.to_str()) != Some("rhai") || !full.is_file() {
        return Err(format!("{path}: not a Rhai file (model scripts are .rhai)"));
    }
    let key = full.strip_prefix(&root).map_err(|_| format!("{path}: model scripts must live inside the repository ({})", root.display()))?.to_string_lossy().replace('\\', "/");
    Ok((full, key))
}

struct State {
    doc: Arc<ArchiveDocument>,
    edit: Edit,
    stamps: Stamps,
}

fn fail(e: impl std::fmt::Display) -> Box<EvalAltResult> {
    e.to_string().into()
}

fn num(d: &Dynamic) -> R<f64> {
    d.as_float().or_else(|_| d.as_int().map(|i| i as f64)).map_err(|t| fail(format!("expected a number, got {t}")))
}
fn vec_of(d: &Dynamic, n: usize) -> R<Vec<f64>> {
    let a = d.clone().into_array().map_err(|t| fail(format!("expected [{}], got {t}", ["x", "y", "z"][..n].join(", "))))?;
    if a.len() != n {
        return Err(fail(format!("expected {n} numbers, got {}", a.len())));
    }
    a.iter().map(num).collect()
}
fn v3(d: &Dynamic) -> R<V3> {
    let v = vec_of(d, 3)?;
    Ok([v[0], v[1], v[2]])
}
fn v2s(d: &Dynamic) -> R<Vec<[f64; 2]>> {
    d.clone().into_array().map_err(|t| fail(format!("expected points, got {t}")))?.iter().map(|p| vec_of(p, 2).map(|v| [v[0], v[1]])).collect()
}
fn json_of(d: &Dynamic) -> R<Value> {
    rhai::serde::from_dynamic::<Value>(d).map_err(fail)
}
fn dynamic(v: &Value) -> R<Dynamic> {
    rhai::serde::to_dynamic(v).map_err(fail)
}

/// The engine with the model-script API over `state`.
fn engine(state: Arc<Mutex<State>>, stop: Arc<AtomicBool>) -> Engine {
    let mut e = Engine::new();
    e.set_max_expr_depths(256, 256);
    {
        let stop = stop.clone();
        e.on_progress(move |_| stop.load(Ordering::Relaxed).then_some(Dynamic::from("cancelled")));
    }
    e.register_type_with_name::<Body>("Body");
    let c = {
        let stop = stop.clone();
        move || stop.load(Ordering::Relaxed)
    };
    macro_rules! k {
        () => {{
            let c = c.clone();
            move || c()
        }};
    }
    let build = |shape: &Shape, cancel: &dyn Fn() -> bool| -> R<Body> { kernel::build(shape, cancel).map(|b| Body(Arc::new(b))).map_err(fail) };

    // ---- bodies ----
    let cc = k!();
    e.register_fn("cylinder", move |base: Dynamic, axis: Dynamic, r: Dynamic, h: Dynamic| -> R<Body> {
        let axis = v3(&axis)?;
        build(&Shape::Cylinder { base: v3(&base)?, axis, radius: num(&r)?, height: num(&h)? }, &cc)
    });
    let cc = k!();
    e.register_fn("cyl", move |r: Dynamic, z0: Dynamic, z1: Dynamic| -> R<Body> {
        let (z0, z1) = (num(&z0)?, num(&z1)?);
        build(&Shape::Cylinder { base: [0.0, 0.0, z0], axis: [0.0, 0.0, 1.0], radius: num(&r)?, height: z1 - z0 }, &cc)
    });
    let cc = k!();
    e.register_fn("cyl", move |r: Dynamic, z0: Dynamic, z1: Dynamic, x: Dynamic, y: Dynamic| -> R<Body> {
        let (z0, z1) = (num(&z0)?, num(&z1)?);
        build(&Shape::Cylinder { base: [num(&x)?, num(&y)?, z0], axis: [0.0, 0.0, 1.0], radius: num(&r)?, height: z1 - z0 }, &cc)
    });
    let cc = k!();
    e.register_fn("box", move |corner: Dynamic, size: Dynamic| -> R<Body> { build(&Shape::Box { corner: v3(&corner)?, size: v3(&size)? }, &cc) });
    let cc = k!();
    e.register_fn("box", move |x0: Dynamic, y0: Dynamic, z0: Dynamic, x1: Dynamic, y1: Dynamic, z1: Dynamic| -> R<Body> {
        let (a, b) = ([num(&x0)?, num(&y0)?, num(&z0)?], [num(&x1)?, num(&y1)?, num(&z1)?]);
        let lo = [0, 1, 2].map(|i| a[i].min(b[i]));
        let size = [0, 1, 2].map(|i| (b[i] - a[i]).abs());
        build(&Shape::Box { corner: lo, size }, &cc)
    });
    let cc = k!();
    e.register_fn("sphere", move |center: Dynamic, r: Dynamic| -> R<Body> { build(&Shape::Sphere { center: v3(&center)?, radius: num(&r)? }, &cc) });
    let cc = k!();
    e.register_fn("prism", move |points: Dynamic, z0: Dynamic, z1: Dynamic| -> R<Body> {
        let (z0, z1) = (num(&z0)?, num(&z1)?);
        let loop3: Vec<V3> = v2s(&points)?.into_iter().map(|p| [p[0], p[1], z0]).collect();
        build(&Shape::Extrude { loops: vec![loop3], direction: [0.0, 0.0, z1 - z0] }, &cc)
    });
    let cc = k!();
    e.register_fn("extrude_polygon", move |points: Dynamic, direction: Dynamic| -> R<Body> {
        let pts: Vec<V3> = points.into_array().map_err(fail)?.iter().map(v3).collect::<R<_>>()?;
        build(&Shape::Extrude { loops: vec![pts], direction: v3(&direction)? }, &cc)
    });
    let cc = k!();
    e.register_fn("sketch_extrude", move |plane: Dynamic, calls: Array, direction: Dynamic, distance: Dynamic| -> R<Body> {
        let plane = crate::sketch::Plane::parse(&json_of(&plane)?).map_err(fail)?;
        let mut sketch = crate::sketch::Sketch::new(plane, "script sketch");
        for call in &calls {
            sketch.call(&json_of(call)?).map_err(fail)?;
        }
        let face = sketch.profile(&[], &cc).map_err(fail)?;
        let d = v3(&direction)?;
        let b = kernel::op1(Op::Extrude, &[&face], &[d[0], d[1], d[2], num(&distance)?, 0.0, 0.0], &[], &cc).map_err(fail)?;
        Ok(Body(Arc::new(b.brep)))
    });
    let boolean = |code: f64| {
        let cc = k!();
        move |a: Body, b: Dynamic| -> R<Body> {
            let others: Vec<Body> = if b.is_array() { b.into_array().map_err(fail)?.into_iter().map(|x| x.try_cast::<Body>().ok_or_else(|| fail("expected bodies"))).collect::<R<_>>()? } else { vec![b.try_cast::<Body>().ok_or_else(|| fail("expected a body"))?] };
            let mut out = a.0.as_ref().clone();
            for o in others {
                out = kernel::op1(Op::Boolean, &[&out, &o.0], &[code], &[], &cc).map_err(fail)?.brep;
            }
            Ok(Body(Arc::new(out)))
        }
    };
    e.register_fn("union", boolean(0.0));
    e.register_fn("subtract", boolean(1.0));
    e.register_fn("intersect", boolean(2.0));
    let cc = k!();
    e.register_fn("rotate", move |b: Body, axis: Dynamic, deg: Dynamic, center: Dynamic| -> R<Body> {
        let m = kernel::placement([0.0; 3], Some(v3(&axis)?), num(&deg)?, v3(&center)?, 1.0).map_err(fail)?;
        build(&Shape::Transform { body: &b.0, matrix: m }, &cc)
    });
    let cc = k!();
    e.register_fn("rotate_z", move |b: Body, deg: Dynamic| -> R<Body> {
        let deg = num(&deg)?;
        if deg == 0.0 {
            return Ok(b);
        }
        let m = kernel::placement([0.0; 3], Some([0.0, 0.0, 1.0]), deg, [0.0; 3], 1.0).map_err(fail)?;
        build(&Shape::Transform { body: &b.0, matrix: m }, &cc)
    });
    let cc = k!();
    e.register_fn("translate", move |b: Body, by: Dynamic| -> R<Body> {
        let m = kernel::placement(v3(&by)?, None, 0.0, [0.0; 3], 1.0).map_err(fail)?;
        build(&Shape::Transform { body: &b.0, matrix: m }, &cc)
    });
    let cc = k!();
    let props = move |b: &Body| -> R<crate::geometry::GeometryProperties> { crate::geometry::body_geometry(&b.0, true, 0.5, &cc).map(|g| g.properties).map_err(fail) };
    let p1 = props.clone();
    e.register_fn("properties", move |b: Body| -> R<Dynamic> {
        let p = p1(&b)?;
        dynamic(&json!({"volume_mm3": p.volume_mm3, "centroid_mm": p.centroid_mm, "inertia_mm5": p.inertia_mm5}))
    });
    e.register_fn("declared", move |b: Body, mass: Dynamic, source: &str| -> R<Dynamic> {
        let p = props(&b)?;
        let mass = num(&mass)?;
        if !(p.volume_mm3 > 0.0) {
            return Err(fail("declared: the body has no volume"));
        }
        // The density that gives the declared mass over this envelope (g/cm³), then
        // the inertia about the centroid: mm⁵ × g/cm³ × 1e-12 = kg·m².
        let density = mass * 1000.0 / (p.volume_mm3 / 1000.0);
        let inertia = p.inertia_mm5.map(|r| r.map(|v| v * density * 1e-12));
        dynamic(&json!({"mass_properties": {"mass_kg": mass, "com_mm": p.centroid_mm, "inertia_kg_m2": inertia, "source": source}}))
    });

    // ---- the document ----
    let with = |state: &Arc<Mutex<State>>, f: &mut dyn FnMut(&mut Ctx) -> Result<Value, String>, stop: &Arc<AtomicBool>| -> R<Value> {
        let mut g = state.lock().map_err(|_| fail("the script's document lock is poisoned"))?;
        let State { doc, edit, stamps } = &mut *g;
        let s = stop.clone();
        let cancel = move || s.load(Ordering::Relaxed);
        let mut cx = Ctx { doc: &**doc, stamps, edit, centroid: &|_| None, cancelled: &cancel };
        f(&mut cx).map_err(fail)
    };
    {
        let (state, stop) = (state.clone(), stop.clone());
        e.register_fn("add_body", move |b: Body, name: &str, material: &str, color: Dynamic, meta: Dynamic| -> R<String> {
            let (color, meta) = (json_of(&color)?, json_of(&meta)?);
            let body = b.0.as_ref().clone();
            let id = with(&state, &mut |cx| {
                let id = cx.add_built(Built { kind: Kind::Solid, brep: body.clone() }, name, Some(material), None)?;
                let n = cx.edit.node_mut(&id)?;
                n["color"] = color.clone();
                n["robot"] = meta.clone();
                Ok(json!(id))
            }, &stop)?;
            Ok(id.as_str().unwrap_or_default().to_string())
        });
    }
    {
        let (state, stop) = (state.clone(), stop.clone());
        e.register_fn("set_body", move |id: &str, b: Body| -> R<()> {
            let body = b.0.as_ref().clone();
            with(&state, &mut |cx| cx.replace(id, Built { kind: Kind::Solid, brep: body.clone() }).map(|()| Value::Null), &stop)?;
            Ok(())
        });
    }
    {
        let (state, stop) = (state.clone(), stop.clone());
        e.register_fn("body_of", move |id: &str| -> R<Body> {
            let mut out = Vec::new();
            with(&state, &mut |cx| {
                out = cx.body(id)?;
                Ok(Value::Null)
            }, &stop)?;
            Ok(Body(Arc::new(out)))
        });
    }
    let run_op = {
        let (state, stop) = (state.clone(), stop.clone());
        move |name: &str, args: Array, kwargs: RMap| -> R<Dynamic> {
            let args: Vec<Value> = args.iter().map(json_of).collect::<R<_>>()?;
            let kwargs: Map<String, Value> = kwargs.into_iter().map(|(k, v)| json_of(&v).map(|v| (k.to_string(), v))).collect::<R<_>>()?;
            let out = with(&state, &mut |cx| crate::ops::run(cx, name, &args, &kwargs), &stop)?;
            dynamic(&out)
        }
    };
    let r2 = run_op.clone();
    e.register_fn("op", move |name: &str, args: Array| -> R<Dynamic> { r2(name, args, RMap::new()) });
    e.register_fn("op", move |name: &str, args: Array, kwargs: RMap| -> R<Dynamic> { run_op(name, args, kwargs) });

    // ---- belts and math ----
    e.register_fn("gt2_pitch_radius", |t: Dynamic| -> R<f64> { Ok(crate::belt::pitch_radius(num(&t)?)) });
    e.register_fn("gt2_tip_radius", |t: Dynamic| -> R<f64> { Ok(crate::belt::tip_radius(num(&t)?)) });
    e.register_fn("gt2_outline", |t: Dynamic| -> R<Dynamic> { dynamic(&json!(crate::belt::toothed_outline(num(&t)? as usize, 0.75, 1.3, 7))) });
    e.register_fn("belt_path", |pulleys: Array| -> R<Dynamic> {
        let p: Vec<([f64; 2], f64)> = pulleys
            .iter()
            .map(|d| {
                let m = json_of(d)?;
                let c = m["center"].as_array().filter(|a| a.len() == 2).ok_or_else(|| fail("a pulley is #{center: [x, y], radius: r}"))?;
                Ok(([c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0)], m["radius"].as_f64().ok_or_else(|| fail("a pulley needs a radius"))?))
            })
            .collect::<R<_>>()?;
        let path = crate::belt::belt_path(&p).map_err(fail)?;
        dynamic(&json!({
            "length": path.length,
            "spans": path.spans.iter().map(|s| json!({"from": s.from, "to": s.to, "start": s.start, "end": s.end, "direction": s.direction, "length": s.length})).collect::<Vec<_>>(),
            "wraps": path.wraps.iter().map(|w| json!({"pulley": w.0, "angle": w.1, "arc": w.2})).collect::<Vec<_>>(),
        }))
    });
    e.register_fn("atan2", |y: Dynamic, x: Dynamic| -> R<f64> { Ok(num(&y)?.atan2(num(&x)?)) });
    e.register_fn("rad", |d: Dynamic| -> R<f64> { Ok(num(&d)?.to_radians()) });
    e.register_fn("deg", |r: Dynamic| -> R<f64> { Ok(num(&r)?.to_degrees()) });
    e.register_fn("hypot", |a: Dynamic, b: Dynamic| -> R<f64> { Ok(num(&a)?.hypot(num(&b)?)) });
    let _ = c;
    e
}

/// What a run did.
#[derive(Clone, Debug)]
pub struct Summary {
    pub script: String,
    pub sha256: String,
    pub params: Value,
    pub created: Vec<String>,
    pub removed: Vec<String>,
    pub seconds: f64,
    pub result: Value,
}

impl Summary {
    pub fn json(&self) -> Value {
        json!({"script": self.script, "sha256": self.sha256, "params": self.params, "created": self.created.len(), "removed": self.removed, "seconds": self.seconds, "result": self.result})
    }
}

/// Run script `path` with `params` on a staged edit of `doc`: the edit and
/// what the run did. `cancelled` stops it between script operations.
pub fn stage(doc: Arc<ArchiveDocument>, path: &str, params: &Value, replace: bool, cancelled: Arc<AtomicBool>) -> Result<(Edit, Summary), String> {
    let (full, key) = resolve(path)?;
    let params = if params.is_null() { json!({}) } else { params.clone() };
    if !params.is_object() {
        return Err("model script parameters must be an object".into());
    }
    let source = std::fs::read_to_string(&full).map_err(|e| format!("{}: {e}", full.display()))?;
    let sha256 = format!("{:x}", Sha256::digest(source.as_bytes()));
    let mut edit = Edit::of(&doc);
    let mut runs = edit.manifest["robot_settings"][SETTING].as_object().cloned().unwrap_or_default();
    let mut removed = Vec::new();
    if replace && let Some(previous) = runs.get(&key) {
        let ids: Vec<String> = previous["nodes"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).filter(|id| edit.node(id).is_some()).collect();
        let tops: Vec<String> = ids.iter().filter(|id| edit.node(id).and_then(|n| n["parent"].as_str()).is_none_or(|p| !ids.iter().any(|x| x == p))).cloned().collect();
        if !tops.is_empty() {
            crate::nodes::delete(&mut edit, &tops)?;
            removed = tops;
        }
    }
    let before: Vec<String> = edit.manifest["nodes"].as_array().into_iter().flatten().filter_map(|n| n["id"].as_str().map(str::to_string)).collect();
    let state = Arc::new(Mutex::new(State { doc, edit, stamps: Stamps::default() }));
    let engine = engine(state.clone(), cancelled);
    let started = std::time::Instant::now();
    let ast = engine.compile(&source).map_err(|e| format!("{key}: {e}"))?;
    if !ast.iter_functions().any(|f| f.name == "build" && f.params.len() == 1) {
        return Err(format!("{key}: define fn build(params)"));
    }
    let mut scope = Scope::new();
    let out: Dynamic = engine.call_fn(&mut scope, &ast, "build", (dynamic(&params).map_err(|e| e.to_string())?,)).map_err(|e| format!("{key}: {e}"))?;
    let result: Value = if out.is_unit() { Value::Null } else { rhai::serde::from_dynamic(&out).map_err(|e| format!("{key}: the summary build returns must be plain data: {e}"))? };
    drop(engine);
    let State { mut edit, .. } = Arc::try_unwrap(state).map_err(|_| "the script kept its document alive")?.into_inner().map_err(|_| "the script's document lock is poisoned")?;
    let created: Vec<String> = edit.manifest["nodes"].as_array().into_iter().flatten().filter_map(|n| n["id"].as_str().map(str::to_string)).filter(|id| !before.contains(id)).collect();
    let seconds = (started.elapsed().as_secs_f64() * 1000.0).round() / 1000.0;
    runs.insert(key.clone(), json!({"sha256": sha256, "params": params, "nodes": created, "seconds": seconds}));
    if !edit.manifest["robot_settings"].is_object() {
        edit.manifest["robot_settings"] = json!({});
    }
    edit.manifest["robot_settings"][SETTING] = Value::Object(runs);
    Ok((edit, Summary { script: key, sha256, params, created, removed, seconds, result }))
}
