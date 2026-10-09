//! The print stress check and planner on a written study, as library calls
//! (`sim-print analyze|plan`; the binary is a thin wrapper over these, and
//! the native viewer runs them in process for CAD's print jobs).
//!
//! Loads may read a simulation (`simulation.system`): it is run once and
//! each `observe` is reduced as the study says; the result records every
//! number's source. `progress` gets a fraction and a message and returns
//! false to cancel (between solver iterations).
use serde_json::{Value, json};
use sim_print::analyze::{self, Inputs, PartResult};
use sim_print::mesh::Mesh;
use sim_print::plan::{self, PlanInputs};
use sim_print::registry;
use sim_print::study::{Magnitude, Observed, Study};
use sim_print::voxel::Grid;
use std::path::{Path, PathBuf};

/// Reports progress; false cancels.
pub type Progress<'a> = &'a mut dyn FnMut(f64, &str) -> bool;

/// A study read with everything it refers to.
pub struct Loaded {
    pub study: Study,
    pub dir: PathBuf,
    pub registry: registry::Loaded,
    pub study_sha256: String,
    /// Per part, per load: newtons and where they came from.
    pub magnitudes: Vec<Vec<(f64, String)>>,
    pub meshes: Vec<Mesh>,
}

fn cancelled() -> String {
    "cancelled".into()
}

/// Read a study (only part `only` when given), its registry, its meshes and
/// the simulation its observed loads read.
pub fn load_study(path: &Path, only: Option<&str>, progress: Progress) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut study = Study::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(name) = only {
        study.parts.retain(|p| p.name == name);
        if study.parts.is_empty() {
            return Err(format!("no part named `{name}` in the study"));
        }
    }
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let registry_path = study.registry.as_ref().map(|r| dir.join(r)).unwrap_or_else(registry::default_path);
    let registry = registry::load(&registry_path)?;
    registry.registry.printer(&study.printer)?;
    registry.registry.material(&study.material)?;
    // Read the simulation once for every observed load.
    let observed = study.observations();
    let mut values: Vec<(Observed, f64, String)> = Vec::new();
    if !observed.is_empty() {
        let sim = study.simulation.as_ref().ok_or("loads read the simulation, but the study has no `simulation`")?;
        let system_path = dir.join(&sim.system);
        if !progress(0.02, &format!("simulating {} for {} s", sim.system, sim.seconds)) {
            return Err(cancelled());
        }
        let document = sim_system::SystemStore::new(&system_path).load().map_err(|e| format!("{}: {e}", system_path.display()))?;
        let reg = crate::system_registry();
        let config = crate::system_builder::config_for(&document);
        let select: Vec<String> = observed.iter().map(|o| o.observe.clone()).collect();
        let series = crate::system_builder::simulate(&document, &reg, sim.seconds, config, &select)?;
        let hash = sim_print::sha256::hex(&std::fs::read(&system_path).map_err(|e| e.to_string())?);
        for o in &observed {
            let s = series.iter().find(|s| s.label == o.observe).ok_or_else(|| {
                let near: Vec<&str> = series.iter().map(|s| s.label.as_str()).filter(|l| l.split('.').next() == o.observe.split('.').next()).take(8).collect();
                format!("the simulation has no series `{}` (same instance: {})", o.observe, if near.is_empty() { "none".into() } else { near.join(", ") })
            })?;
            let v = o.reduce_series(&s.times, &s.values)?;
            let window = o.window.map(|w| format!(" over {}–{} s", w[0], w[1])).unwrap_or_else(|| format!(" over 0–{} s", sim.seconds));
            let how = format!(
                "simulation {} (sha256 {}…): {} of {}{window} = {:.4} {}{}{}{}",
                sim.system,
                &hash[..12],
                o.reduce,
                o.observe,
                (v - o.add) / o.scale,
                s.unit,
                if o.scale != 1. { format!(" × {}", o.scale) } else { String::new() },
                if o.add != 0. { format!(" + {} N", o.add) } else { String::new() },
                if o.why.is_empty() { String::new() } else { format!(" ({})", o.why) },
            );
            values.push((o.clone(), v, how));
        }
    }
    let mut magnitudes = Vec::new();
    let mut meshes = Vec::new();
    for p in &study.parts {
        let m: Vec<(f64, String)> = p
            .loads
            .iter()
            .map(|l| match &l.magnitude {
                Magnitude::Newtons(n) => Ok((*n, "given in the study".to_string())),
                Magnitude::Observed(o) => values.iter().find(|(x, _, _)| x == o).map(|(_, v, how)| (*v, how.clone())).ok_or_else(|| format!("{}: an observed load was not read", p.name)),
            })
            .collect::<Result<_, String>>()?;
        magnitudes.push(m);
        let mesh_path = dir.join(&p.mesh);
        let mesh = Mesh::read_stl(&mesh_path)?;
        if mesh.volume() <= 0. {
            return Err(format!("{}: the mesh of `{}` is inside out or open (volume {:.1} mm³)", mesh_path.display(), p.name, mesh.volume()));
        }
        meshes.push(mesh);
    }
    Ok(Loaded { study, dir, registry, study_sha256: sim_print::sha256::hex(text.as_bytes()), magnitudes, meshes })
}

pub fn voxel_size(study: &Study, mesh: &Mesh) -> f64 {
    study.voxel_mm.unwrap_or_else(|| Grid::pitch_for(mesh, study.voxels))
}

/// Write a part's stress field (`<slug>.field.json` + `.field.bin`).
pub fn write_field(out: &Path, name: &str, result: &PartResult) -> Result<(String, String), String> {
    let stem = slug(name);
    let (json, bin) = (format!("{stem}.field.json"), format!("{stem}.field.bin"));
    std::fs::write(out.join(&json), serde_json::to_string_pretty(&result.field.header()).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::write(out.join(&bin), result.field.to_bytes()).map_err(|e| e.to_string())?;
    Ok((json, bin))
}

pub fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    s.split('-').filter(|x| !x.is_empty()).collect::<Vec<_>>().join("-")
}

/// Stress check each part as it will be printed; writes `result.json` (and
/// fields) into `out` and returns the report.
pub fn analyze(path: &Path, out: Option<&Path>, only: Option<&str>, progress: Progress) -> Result<Value, String> {
    let loaded = load_study(path, only, progress)?;
    let out = out.map(Path::to_path_buf).unwrap_or_else(|| loaded.dir.join("print-results"));
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let reg = &loaded.registry.registry;
    let printer = reg.printer(&loaded.study.printer)?;
    let material = reg.material(&loaded.study.material)?;
    let started = std::time::Instant::now();
    let mut parts = Vec::new();
    let n = loaded.study.parts.len();
    for (k, part) in loaded.study.parts.iter().enumerate() {
        let mesh = &loaded.meshes[k];
        let h = voxel_size(&loaded.study, mesh);
        let base = 0.1 + 0.85 * k as f64 / n as f64;
        if !progress(base, &format!("{}: {:.2} mm voxels", part.name, h)) {
            return Err(cancelled());
        }
        let inputs = Inputs { registry: reg, printer, material, mesh, part, magnitudes: &loaded.magnitudes[k], build_direction: part.build_direction, settings: &part.settings, voxel_mm: h };
        let mut stopped = false;
        let result = analyze::analyze(&inputs, &mut |it, res| {
            let go = progress(base + 0.85 / n as f64 * (res.max(1e-12).log10().abs() / 7.).min(0.99), &format!("{}: iteration {it}, residual {res:.1e}", part.name));
            stopped |= !go;
            go
        });
        if stopped {
            return Err(cancelled());
        }
        let result = result.map_err(|e| format!("{}: {e}", part.name))?;
        let (field, bin) = write_field(&out, &part.name, &result)?;
        let mut value = serde_json::to_value(&result).map_err(|e| e.to_string())?;
        value["field"] = json!({"header": field, "data": bin});
        value["passes"] = json!(result.safety_factor >= loaded.study.safety_target && result.seams.iter().all(|s| s.safety_factor >= loaded.study.safety_target));
        parts.push(value);
    }
    let report = json!({
        "schema": "sim.print-result/1",
        "study": path, "study_sha256": loaded.study_sha256,
        "registry": loaded.registry.path, "registry_sha256": loaded.registry.sha256, "registry_revision": reg.revision,
        "printer": loaded.study.printer, "material": loaded.study.material,
        "safety_target": loaded.study.safety_target,
        "fidelity": "voxel linear elastic FE (hex8, transversely isotropic about the build direction); strengths from the registry (estimated unless marked measured); stresses near sharp corners and load patches are approximate",
        "seconds": started.elapsed().as_secs_f64(),
        "parts": parts,
    });
    let file = out.join("result.json");
    std::fs::write(&file, serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    progress(1., "done");
    Ok(report)
}

/// Choose orientation and settings per part; writes `plan.json` (and the
/// verified fields) into `out` and returns the report.
pub fn plan(path: &Path, out: Option<&Path>, only: Option<&str>, progress: Progress) -> Result<Value, String> {
    let loaded = load_study(path, only, progress)?;
    let out = out.map(Path::to_path_buf).unwrap_or_else(|| loaded.dir.join("print-plan"));
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let reg = &loaded.registry.registry;
    let printer = reg.printer(&loaded.study.printer)?;
    let material = reg.material(&loaded.study.material)?;
    let space = loaded.study.plan.clone().unwrap_or_default();
    let started = std::time::Instant::now();
    let n = loaded.study.parts.len();
    let mut parts = Vec::new();
    for (k, part) in loaded.study.parts.iter().enumerate() {
        let mesh = &loaded.meshes[k];
        let inputs = PlanInputs { registry: reg, printer, material, mesh, part, magnitudes: &loaded.magnitudes[k], space: &space, safety_target: loaded.study.safety_target, final_voxel_mm: voxel_size(&loaded.study, mesh) };
        let mut stopped = false;
        let result = plan::plan(&inputs, &mut |f, m| {
            let go = progress((k as f64 + f) / n as f64, m);
            stopped |= !go;
            go
        });
        if stopped {
            return Err(cancelled());
        }
        let result = result?;
        let mut value = serde_json::to_value(&result).map_err(|e| e.to_string())?;
        if let Some(v) = &result.verified {
            let (field, bin) = write_field(&out, &format!("{}-plan", slug(&part.name)), v)?;
            let mut checked = serde_json::to_value(v).map_err(|e| e.to_string())?;
            checked["field"] = json!({"header": field, "data": bin});
            value["verified"] = checked;
        }
        parts.push(value);
    }
    let report = json!({
        "schema": "sim.print-plan/1",
        "study": path, "study_sha256": loaded.study_sha256,
        "registry": loaded.registry.path, "registry_sha256": loaded.registry.sha256, "registry_revision": reg.revision,
        "printer": loaded.study.printer, "material": loaded.study.material, "safety_target": loaded.study.safety_target,
        "space": space,
        "estimates": "print time, filament and support are estimated from voxel volumes and the registry's flow and overhead values; compare with a slicer",
        "seconds": started.elapsed().as_secs_f64(),
        "parts": parts,
    });
    let file = out.join("plan.json");
    std::fs::write(&file, serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    progress(1., "done");
    Ok(report)
}

/// `analyze` or `plan` by name.
pub fn run(command: &str, study: &Path, out: Option<&Path>, progress: Progress) -> Result<Value, String> {
    match command {
        "analyze" => analyze(study, out, None, progress),
        "plan" => plan(study, out, None, progress),
        other => Err(format!("unknown print command {other:?} (analyze or plan)")),
    }
}
