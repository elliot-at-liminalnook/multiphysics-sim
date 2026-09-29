//! Printable parts from the command line (CAD runs these as subprocesses).
//!
//!   sim-print registry [REGISTRY]                check the print registry; print its fingerprint
//!   sim-print analyze STUDY [--out DIR] [--part NAME]
//!                                               stress check each part as it will be printed
//!   sim-print plan STUDY [--out DIR] [--part NAME]
//!                                               choose orientation and settings; write plates
//!   sim-print promote RESULTS [--registry PATH] [--write]
//!                                               turn coupon measurements into a registry revision
//!
//! Loads may read a simulation (`simulation.system`): it is run once and each
//! `observe` is reduced as the study says; the result records every number's
//! source. Progress goes to stderr as `progress <fraction> <message>`.

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(e) = native::run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod native {
    use sim_print::analyze::{self, Inputs, PartResult};
    use sim_print::mesh::Mesh;
    use sim_print::registry;
    use sim_print::study::{Magnitude, Observed, Study};
    use sim_print::voxel::Grid;
    use std::path::{Path, PathBuf};

    const USAGE: &str = "usage: sim-print registry [REGISTRY] | analyze STUDY [--out DIR] [--part NAME] | plan STUDY [--out DIR] [--part NAME] | promote RESULTS [--registry PATH] [--write]";

    fn flag(args: &[String], name: &str) -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    }

    pub fn progress(fraction: f64, message: &str) {
        eprintln!("progress {:.3} {message}", fraction.clamp(0., 1.));
    }

    pub fn run() -> Result<(), String> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.first().map(String::as_str) {
            Some("registry") => {
                let path = args.get(1).map(PathBuf::from).unwrap_or_else(registry::default_path);
                let loaded = registry::load(&path)?;
                let r = &loaded.registry;
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "path": path, "sha256": loaded.sha256, "revision": r.revision,
                    "printers": r.printers.iter().map(|(k, p)| (k.clone(), serde_json::json!({"name": p.name, "usable_mm": p.usable_mm()}))).collect::<serde_json::Map<_, _>>(),
                    "materials": r.materials.iter().map(|(k, m)| (k.clone(), serde_json::json!({"name": m.name, "cad_material": m.cad_material}))).collect::<serde_json::Map<_, _>>(),
                })).unwrap());
                Ok(())
            }
            Some("analyze") => analyze_command(&args),
            Some("plan") => crate::plan_cli::plan_command(&args),
            Some("promote") => crate::promote_cli::promote_command(&args),
            _ => Err(USAGE.into()),
        }
    }

    pub struct Loaded {
        pub study: Study,
        pub dir: PathBuf,
        pub registry: registry::Loaded,
        pub study_sha256: String,
        /// Per part, per load: newtons and where they came from.
        pub magnitudes: Vec<Vec<(f64, String)>>,
        pub meshes: Vec<Mesh>,
    }

    pub fn load_study(path: &Path, only: Option<&str>) -> Result<Loaded, String> {
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
            progress(0.02, &format!("simulating {} for {} s", sim.system, sim.seconds));
            let document = sim_system::SystemStore::new(&system_path).load().map_err(|e| format!("{}: {e}", system_path.display()))?;
            let reg = sim_runtime::system_registry();
            let config = sim_runtime::system_builder::config_for(&document);
            let select: Vec<String> = observed.iter().map(|o| o.observe.clone()).collect();
            let series = sim_runtime::system_builder::simulate(&document, &reg, sim.seconds, config, &select)?;
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
                    sim.system, &hash[..12], o.reduce, o.observe, (v - o.add) / o.scale, s.unit,
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
            let m: Vec<(f64, String)> = p.loads.iter().map(|l| match &l.magnitude {
                Magnitude::Newtons(n) => (*n, "given in the study".to_string()),
                Magnitude::Observed(o) => values.iter().find(|(x, _, _)| x == o).map(|(_, v, how)| (*v, how.clone())).unwrap(),
            }).collect();
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

    pub fn write_field(out: &Path, name: &str, result: &PartResult) -> Result<(String, String), String> {
        let stem = slug(name);
        let (json, bin) = (format!("{stem}.field.json"), format!("{stem}.field.bin"));
        std::fs::write(out.join(&json), serde_json::to_string_pretty(&result.field.header()).unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(out.join(&bin), result.field.to_bytes()).map_err(|e| e.to_string())?;
        Ok((json, bin))
    }

    pub fn slug(name: &str) -> String {
        let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
        s.split('-').filter(|x| !x.is_empty()).collect::<Vec<_>>().join("-")
    }

    fn analyze_command(args: &[String]) -> Result<(), String> {
        let path = PathBuf::from(args.get(1).ok_or(USAGE)?);
        let only = flag(args, "--part");
        let loaded = load_study(&path, only.as_deref())?;
        let out = flag(args, "--out").map(PathBuf::from).unwrap_or_else(|| loaded.dir.join("print-results"));
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
            progress(0.1 + 0.85 * k as f64 / n as f64, &format!("{}: {:.2} mm voxels", part.name, h));
            let inputs = Inputs { registry: reg, printer, material, mesh, part, magnitudes: &loaded.magnitudes[k], build_direction: part.build_direction, settings: &part.settings, voxel_mm: h };
            let base = 0.1 + 0.85 * k as f64 / n as f64;
            let result = analyze::analyze(&inputs, &mut |it, res| {
                progress(base + 0.85 / n as f64 * (res.max(1e-12).log10().abs() / 7.).min(0.99), &format!("{}: iteration {it}, residual {res:.1e}", part.name));
                true
            })
            .map_err(|e| format!("{}: {e}", part.name))?;
            let (field, bin) = write_field(&out, &part.name, &result)?;
            let mut value = serde_json::to_value(&result).unwrap();
            value["field"] = serde_json::json!({"header": field, "data": bin});
            value["passes"] = serde_json::json!(result.safety_factor >= loaded.study.safety_target && result.seams.iter().all(|s| s.safety_factor >= loaded.study.safety_target));
            parts.push(value);
        }
        let report = serde_json::json!({
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
        std::fs::write(&file, serde_json::to_string_pretty(&report).unwrap()).map_err(|e| e.to_string())?;
        progress(1., "done");
        for p in report["parts"].as_array().unwrap() {
            println!("{:<40} safety {:>6.2} ({} at [{:.1}, {:.1}, {:.1}] mm)  mass {:.1} g  max deflection {:.3} mm  {}",
                p["name"].as_str().unwrap_or(""), p["safety_factor"].as_f64().unwrap_or(f64::NAN), p["governing"]["mode"].as_str().unwrap_or(""),
                p["governing"]["at"][0].as_f64().unwrap_or(0.), p["governing"]["at"][1].as_f64().unwrap_or(0.), p["governing"]["at"][2].as_f64().unwrap_or(0.),
                p["mass_g"].as_f64().unwrap_or(0.), p["max_displacement_mm"].as_f64().unwrap_or(0.), if p["passes"].as_bool() == Some(true) { "ok" } else { "BELOW TARGET" });
        }
        println!("wrote {}", file.display());
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod plan_cli {
    use crate::native::{load_study, progress, slug, voxel_size, write_field};
    use sim_print::plan::{self, PlanInputs};
    use sim_print::study::PlanSpace;
    use std::path::PathBuf;

    fn flag(args: &[String], name: &str) -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    }

    pub fn plan_command(args: &[String]) -> Result<(), String> {
        let path = PathBuf::from(args.get(1).ok_or("usage: sim-print plan STUDY [--out DIR] [--part NAME]")?);
        let loaded = load_study(&path, flag(args, "--part").as_deref())?;
        let out = flag(args, "--out").map(PathBuf::from).unwrap_or_else(|| loaded.dir.join("print-plan"));
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
            let result = plan::plan(&inputs, &mut |f, m| {
                progress((k as f64 + f) / n as f64, m);
                true
            })?;
            let mut value = serde_json::to_value(&result).unwrap();
            if let Some(v) = &result.verified {
                let (field, bin) = write_field(&out, &format!("{}-plan", slug(&part.name)), v)?;
                let mut checked = serde_json::to_value(v).unwrap();
                checked["field"] = serde_json::json!({"header": field, "data": bin});
                value["verified"] = checked;
            }
            parts.push(value);
        }
        let _ = PlanSpace::default();
        let report = serde_json::json!({
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
        std::fs::write(&file, serde_json::to_string_pretty(&report).unwrap()).map_err(|e| e.to_string())?;
        progress(1., "done");
        for p in report["parts"].as_array().unwrap() {
            let c = &p["chosen"];
            println!("{:<40} {:?} {} walls, {:.0}% infill, {} mm layers: safety {:.2} (checked {:.2}), {:.2} h, {:.0} g (+{:.0} g support), {} solves",
                p["name"].as_str().unwrap_or(""), c["build_direction"].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.)).collect::<Vec<_>>()).unwrap_or_default(),
                c["settings"]["walls"], c["settings"]["infill"].as_f64().unwrap_or(0.) * 100., c["settings"]["layer_height"],
                c["safety_factor"].as_f64().unwrap_or(f64::NAN), p["verified"]["safety_factor"].as_f64().unwrap_or(f64::NAN),
                c["estimate"]["print_hours"].as_f64().unwrap_or(0.), c["estimate"]["filament_g"].as_f64().unwrap_or(0.), c["estimate"]["support_g"].as_f64().unwrap_or(0.), p["solves"]);
        }
        println!("wrote {}", file.display());
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod promote_cli {
    use std::path::PathBuf;

    fn flag(args: &[String], name: &str) -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    }

    fn today() -> String {
        // Days since the epoch → civil date (UTC), no extra crate.
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
        let z = secs.div_euclid(86_400) + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
        format!("{y:04}-{m:02}-{d:02}")
    }

    pub fn promote_command(args: &[String]) -> Result<(), String> {
        let results = PathBuf::from(args.get(1).ok_or("usage: sim-print promote RESULTS [--registry PATH] [--write]")?);
        let path = flag(args, "--registry").map(PathBuf::from).unwrap_or_else(sim_print::registry::default_path);
        let loaded = sim_print::registry::load(&path)?;
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(&results).map_err(|e| format!("{}: {e}", results.display()))?;
        let outcome = sim_print::promote::promote(&raw, &loaded.registry, &loaded.sha256, &bytes, &flag(args, "--date").unwrap_or_else(today))?;
        let write = args.iter().any(|a| a == "--write");
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "registry": path, "promotions": outcome.promotions, "refused": outcome.refused, "warnings": outcome.warnings,
            "written": write && outcome.registry.is_some(),
        })).unwrap());
        if let (true, Some(new)) = (write, &outcome.registry) {
            let text = serde_json::to_string_pretty(new).unwrap() + "\n";
            std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
            eprintln!("wrote registry revision {} to {} (sha256 {})", new["revision"], path.display(), sim_print::sha256::hex(text.as_bytes()));
        } else if outcome.registry.is_some() {
            eprintln!("dry run: add --write to record these values in the registry");
        }
        if !outcome.refused.is_empty() && outcome.promotions.is_empty() {
            return Err("nothing promoted".into());
        }
        Ok(())
    }
}
