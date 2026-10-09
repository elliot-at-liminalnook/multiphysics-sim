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
    use sim_print::registry;
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

    fn analyze_command(args: &[String]) -> Result<(), String> {
        let path = PathBuf::from(args.get(1).ok_or(USAGE)?);
        let out = flag(args, "--out").map(PathBuf::from);
        let report = sim_runtime::print_tools::analyze(&path, out.as_deref(), flag(args, "--part").as_deref(), &mut |f, m| {
            progress(f, m);
            true
        })?;
        let file = out.unwrap_or_else(|| path.parent().map(Path::to_path_buf).unwrap_or_default().join("print-results")).join("result.json");
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
    use crate::native::progress;
    use std::path::{Path, PathBuf};

    fn flag(args: &[String], name: &str) -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    }

    pub fn plan_command(args: &[String]) -> Result<(), String> {
        let path = PathBuf::from(args.get(1).ok_or("usage: sim-print plan STUDY [--out DIR] [--part NAME]")?);
        let out = flag(args, "--out").map(PathBuf::from);
        let report = sim_runtime::print_tools::plan(&path, out.as_deref(), flag(args, "--part").as_deref(), &mut |f, m| {
            progress(f, m);
            true
        })?;
        let file = out.unwrap_or_else(|| path.parent().map(Path::to_path_buf).unwrap_or_default().join("print-plan")).join("plan.json");
        for p in report["parts"].as_array().into_iter().flatten() {
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
