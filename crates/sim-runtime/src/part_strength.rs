//! Printed-part strength from simulated loads: each part's mesh (mm, its
//! CAD frame), where it is held and the peak loads a run put on it, through
//! the print registry's layer-aware voxel stress check (`sim_print::analyze`:
//! the filament's along- and across-layer strengths, infill and walls, the
//! build direction). One safety factor (strength ÷ stress) per part, with
//! its failure mode and where; a part whose material no registry filament
//! backs is not assessed (and says so).
use serde_json::{Value, json};
use sim_print::mesh::Mesh;
use sim_print::registry::Loaded;
use sim_print::study::{Fixture, Load, Magnitude, PartStudy, Region};
use sim_print::voxel::Settings;

/// The printer used for the check when none is named (its line width sets the walls).
pub const DEFAULT_PRINTER: &str = "bambu-p1s";
/// Target voxel count per part.
pub const VOXELS: f64 = 24_000.0;

/// One part to check.
pub struct Part {
    pub name: String,
    /// The CAD material id (`pla`, `petg`).
    pub material: String,
    pub vertices_mm: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub build_direction: [f64; 3],
    pub settings: Settings,
    /// Where it is held.
    pub fixture: (String, Region),
    /// What pushes on it: the load, its magnitude (N) and where that number came from.
    pub loads: Vec<(Load, f64, String)>,
}

/// The voxel edge for a part of these vertices (about [`VOXELS`] voxels).
fn voxel_mm(vertices: &[[f64; 3]]) -> f64 {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in vertices {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let volume: f64 = (0..3).map(|k| (hi[k] - lo[k]).max(1.0)).product();
    (volume / VOXELS).cbrt().clamp(0.4, 4.0)
}

/// Check each part (see the module doc); `cancelled` is polled between parts.
pub fn check(registry: &Loaded, parts: &[Part], cancelled: &dyn Fn() -> bool) -> Vec<Value> {
    let printer_key = if registry.registry.printers.contains_key(DEFAULT_PRINTER) { DEFAULT_PRINTER.to_string() } else { registry.registry.printers.keys().next().cloned().unwrap_or_default() };
    let mut out = Vec::new();
    for part in parts {
        if cancelled() {
            break;
        }
        let Some((key, material)) = registry.registry.materials.iter().find(|(_, m)| m.cad_material == part.material) else {
            out.push(json!({"part": part.name, "material": part.material, "assessed": false, "why": format!("no print-registry filament backs `{}`, so its layer strengths are unknown", part.material)}));
            continue;
        };
        let Some(printer) = registry.registry.printers.get(&printer_key) else {
            out.push(json!({"part": part.name, "assessed": false, "why": "the print registry names no printer"}));
            continue;
        };
        let study = PartStudy {
            name: part.name.clone(),
            mesh: part.name.clone(),
            build_direction: part.build_direction,
            settings: part.settings.clone(),
            fixtures: vec![Fixture { name: part.fixture.0.clone(), region: part.fixture.1.clone() }],
            loads: part.loads.iter().map(|(l, _, _)| Load { magnitude: Magnitude::Newtons(0.0), ..l.clone() }).collect(),
            acceleration: None,
            sections: Vec::new(),
            seams: Vec::new(),
            directions: None,
        };
        let magnitudes: Vec<(f64, String)> = part.loads.iter().map(|(_, n, s)| (*n, s.clone())).collect();
        let mesh = Mesh { vertices: part.vertices_mm.clone(), triangles: part.triangles.clone() };
        let voxel = voxel_mm(&part.vertices_mm);
        let inputs = sim_print::analyze::Inputs { registry: &registry.registry, printer, material, mesh: &mesh, part: &study, magnitudes: &magnitudes, build_direction: part.build_direction, settings: &part.settings, voxel_mm: voxel };
        match sim_print::analyze::analyze(&inputs, &mut |_, _| !cancelled()) {
            Ok(r) => out.push(json!({
                "part": part.name, "material": part.material, "filament": key, "assessed": true,
                "safety_factor": r.safety_factor, "safety_factor_peak": r.safety_factor_peak, "mode": r.governing.mode, "at_mm": r.governing.at,
                "by_mode": r.by_mode, "max_displacement_mm": r.max_displacement_mm, "voxel_mm": voxel, "voxels": r.voxels,
                "loads": r.loads, "equilibrium_error": r.equilibrium_error, "warnings": r.warnings,
                "printer": printer_key, "settings": part.settings, "build_direction": part.build_direction,
                "registry_sha256": registry.sha256,
            })),
            Err(e) => out.push(json!({"part": part.name, "material": part.material, "assessed": false, "why": format!("the stress check could not run: {e}")})),
        }
    }
    out
}
