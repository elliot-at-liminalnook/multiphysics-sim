//! Choose how to print a piece: build direction and settings (walls, infill,
//! layer height) that reach the safety target in the least print time.
//!
//! For each direction the cheap estimates (time, filament, support, bed
//! contact) come first; candidates are then stress checked in order of print
//! time until one is strong enough, so few solves are needed. The pick is
//! re-checked at the study's full resolution.
//!
//! Time, filament and support are estimates from voxel volumes and the
//! registry's flow and overhead values (labelled as such); compare with a
//! slicer before trusting them to the minute.

use crate::analyze::{self, Inputs, PartResult};
use crate::mesh::{Mesh, Rot, V3, unit};
use crate::registry::{Material, Printer, Registry};
use crate::study::{PartStudy, PlanSpace};
use crate::voxel::{self, Grid, Settings};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Estimate {
    /// Hours, including supports, layer changes and travel overhead.
    pub print_hours: f64,
    pub filament_g: f64,
    pub support_g: f64,
    /// Down-facing area steeper than 45° that needs support (mm²).
    pub support_area_mm2: f64,
    /// Area on the bed (mm²).
    pub bed_contact_mm2: f64,
    pub height_mm: f64,
    pub cost: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    pub build_direction: V3,
    pub settings: Settings,
    pub estimate: Estimate,
    /// None when not stress checked (a faster candidate already failed or a slower one was not needed).
    pub safety_factor: Option<f64>,
    pub mode: Option<String>,
    pub passes: Option<bool>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PartPlan {
    pub name: String,
    pub chosen: Option<Candidate>,
    /// Checked at the study's resolution with the chosen direction and settings.
    #[serde(skip)]
    pub verified: Option<PartResult>,
    pub candidates: Vec<Candidate>,
    pub solves: usize,
    pub notes: Vec<String>,
}

/// Directions to try: the part's list, else the six axis directions.
pub fn directions(part: &PartStudy) -> Vec<V3> {
    part.directions.clone().unwrap_or_else(|| vec![[0., 0., 1.], [0., 0., -1.], [1., 0., 0.], [-1., 0., 0.], [0., 1., 0.], [0., -1., 0.]])
}

/// Cheap estimates for one direction and settings (no solve).
pub fn estimate(grid: &Grid, settings: &Settings, printer: &Printer, material: &Material, modulus_exponent: f64, strength_exponent: f64) -> Estimate {
    let fill = voxel::fill(grid, settings, printer.line_width_mm.value, modulus_exponent, strength_exponent);
    let h3 = grid.h.powi(3);
    let (mut v_wall, mut v_mat) = (0., 0.);
    // Solid (walls and skins) and total material volume.
    for v in 0..grid.len() {
        if grid.solid[v] {
            v_mat += fill.mass[v] * h3;
            v_wall += fill.solid_fraction[v] as f64 * h3;
        }
    }
    // Support: down-facing voxels with nothing within one voxel diagonally below (steeper than 45°).
    let [nx, ny, nz] = grid.n;
    let (mut support_cols, mut support_area) = (0., 0.);
    let mut lowest = nz;
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                if grid.solid[grid.index(i, j, k)] {
                    lowest = lowest.min(k);
                }
            }
        }
    }
    let mut contact = 0.;
    for j in 0..ny {
        for i in 0..nx {
            for k in 0..nz {
                if !grid.solid[grid.index(i, j, k)] {
                    continue;
                }
                if k == lowest {
                    contact += grid.h * grid.h;
                    continue;
                }
                let (ii, jj, kk) = (i as isize, j as isize, k as isize);
                if grid.at(ii, jj, kk - 1) {
                    continue;
                }
                let held = (-1..=1).any(|di| (-1..=1).any(|dj| grid.at(ii + di, jj + dj, kk - 1)));
                if held {
                    continue;
                }
                support_area += grid.h * grid.h;
                // A column of support down to the next solid voxel or the bed.
                let mut gap = 0;
                let mut kb = kk - 1;
                while kb >= lowest as isize && !grid.at(ii, jj, kb) {
                    gap += 1;
                    kb -= 1;
                }
                support_cols += gap as f64 * h3;
            }
        }
    }
    let support_density = 0.15;
    let v_support = support_cols * support_density;
    let q = material.max_volumetric_speed.value; // mm³/s
    let height = (nz - lowest) as f64 * grid.h;
    let layers = (height / settings.layer_height).ceil();
    // Solid walls and skins at about half the flow limit (outer walls run slow); sparse infill near it.
    let seconds = (v_wall / (0.5 * q) + (v_mat - v_wall).max(0.) / (0.85 * q) + v_support / (0.85 * q)) * (1. + printer.travel_overhead.value) + layers * printer.layer_change_s.value;
    let grams = (v_mat) * 1e-9 * material.density.value * 1e3;
    let support_g = v_support * 1e-9 * material.density.value * 1e3;
    Estimate {
        print_hours: seconds / 3600.,
        filament_g: grams,
        support_g,
        support_area_mm2: support_area,
        bed_contact_mm2: contact,
        height_mm: height,
        cost: (grams + support_g) * 1e-3 * material.filament_price_per_kg.value,
    }
}

/// Safety margin on the coarse search grid over the target (the pick is re-checked finely).
pub const SEARCH_MARGIN: f64 = 1.15;

pub struct PlanInputs<'a> {
    pub registry: &'a Registry,
    pub printer: &'a Printer,
    pub material: &'a Material,
    pub mesh: &'a Mesh,
    pub part: &'a PartStudy,
    pub magnitudes: &'a [(f64, String)],
    pub space: &'a PlanSpace,
    pub safety_target: f64,
    /// Voxel edge for the final check (mm).
    pub final_voxel_mm: f64,
}

pub fn plan(inp: &PlanInputs, progress: &mut dyn FnMut(f64, &str) -> bool) -> Result<PartPlan, String> {
    let law = inp.registry.infill_law(&inp.space.pattern)?;
    let (me, se) = (law.modulus_exponent.value, law.strength_exponent.value);
    let heights = inp.space.layer_heights.clone().unwrap_or_else(|| vec![inp.part.settings.layer_height]);
    let search_h = Grid::pitch_for(inp.mesh, inp.space.search_voxels);
    let usable = inp.printer.usable_mm();
    let mut candidates = Vec::new();
    let mut notes = Vec::new();
    let mut solves = 0;
    let dirs = directions(inp.part);
    for (di, dir) in dirs.iter().enumerate() {
        let rot = Rot::build_up(*dir);
        let rotated = inp.mesh.rotated(&rot);
        let (lo, hi) = rotated.bounds();
        let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        if size[2] > usable[2] || !((size[0] <= usable[0] && size[1] <= usable[1]) || (size[0] <= usable[1] && size[1] <= usable[0])) {
            notes.push(format!("build direction {dir:?}: {:.0}×{:.0}×{:.0} mm does not fit the {} (turning it about z is not tried here)", size[0], size[1], size[2], inp.printer.name));
            continue;
        }
        let grid = Grid::voxelize(&rotated, search_h)?;
        let mut options: Vec<(Settings, Estimate)> = Vec::new();
        for &w in &inp.space.walls {
            for &f in &inp.space.infill {
                for &lh in &heights {
                    let s = Settings { walls: w, infill: f, pattern: inp.space.pattern.clone(), layer_height: lh, top_bottom_layers: inp.part.settings.top_bottom_layers };
                    let e = estimate(&grid, &s, inp.printer, inp.material, me, se);
                    options.push((s, e));
                }
            }
        }
        if options.first().is_some_and(|(_, e)| e.bed_contact_mm2 < 25.) {
            notes.push(format!("build direction {dir:?}: only {:.0} mm² on the bed; it would need a brim or supports to stand", options[0].1.bed_contact_mm2));
        }
        options.sort_by(|a, b| a.1.print_hours.total_cmp(&b.1.print_hours));
        let mut found = false;
        for (k, (s, e)) in options.iter().enumerate() {
            if found {
                candidates.push(Candidate { build_direction: unit(*dir), settings: s.clone(), estimate: e.clone(), safety_factor: None, mode: None, passes: None, note: "slower than a passing choice".into() });
                continue;
            }
            let frac = (di as f64 + k as f64 / options.len() as f64) / dirs.len() as f64;
            if !progress(0.9 * frac, &format!("{}: direction {:?}, {s}", inp.part.name, dir)) {
                return Err("cancelled".into());
            }
            let r = analyze::analyze(&Inputs { registry: inp.registry, printer: inp.printer, material: inp.material, mesh: inp.mesh, part: inp.part, magnitudes: inp.magnitudes, build_direction: *dir, settings: s, voxel_mm: search_h }, &mut |_, _| true)?;
            solves += 1;
            // The search grid is coarse: ask for a margin so the full-resolution check agrees.
            let needed = inp.safety_target * SEARCH_MARGIN;
            let passes = r.safety_factor >= needed && r.seams.iter().all(|x| x.safety_factor >= needed);
            candidates.push(Candidate { build_direction: unit(*dir), settings: s.clone(), estimate: e.clone(), safety_factor: Some(r.safety_factor), mode: Some(r.governing.mode.clone()), passes: Some(passes), note: String::new() });
            found = passes;
        }
    }
    // The fastest passing candidate; else the strongest one checked.
    let chosen = candidates.iter().filter(|c| c.passes == Some(true)).min_by(|a, b| a.estimate.print_hours.total_cmp(&b.estimate.print_hours)).cloned().or_else(|| {
        notes.push(format!("no direction and settings reach a safety factor of {} at the search resolution: the strongest is chosen; consider another material, a thicker part, or a split", inp.safety_target));
        candidates.iter().filter(|c| c.safety_factor.is_some()).max_by(|a, b| a.safety_factor.unwrap().total_cmp(&b.safety_factor.unwrap())).cloned()
    });
    // Re-check at full resolution; if the pick falls short there, try the next
    // passing candidates in order of print time (a few at most).
    let mut chosen = chosen;
    let mut verified = None;
    if let Some(first) = chosen.clone() {
        let mut queue: Vec<Candidate> = vec![first.clone()];
        let mut rest: Vec<Candidate> = candidates.iter().filter(|c| c.passes == Some(true) && (c.build_direction, &c.settings) != (first.build_direction, &first.settings)).cloned().collect();
        // Also the next slower settings in the chosen direction (not solved in the search).
        rest.extend(candidates.iter().filter(|c| c.passes.is_none() && c.build_direction == first.build_direction).cloned());
        rest.sort_by(|a, b| a.estimate.print_hours.total_cmp(&b.estimate.print_hours));
        queue.extend(rest.into_iter().take(4));
        for (n, c) in queue.iter().enumerate() {
            progress(0.92 + 0.02 * n as f64, &format!("{}: checking {} at full resolution", inp.part.name, c.settings));
            let r = analyze::analyze(&Inputs { registry: inp.registry, printer: inp.printer, material: inp.material, mesh: inp.mesh, part: inp.part, magnitudes: inp.magnitudes, build_direction: c.build_direction, settings: &c.settings, voxel_mm: inp.final_voxel_mm }, &mut |_, _| true)?;
            solves += 1;
            let ok = r.safety_factor >= inp.safety_target && r.seams.iter().all(|x| x.safety_factor >= inp.safety_target);
            if n > 0 || !ok {
                notes.push(format!("full-resolution check of {} along {:?}: safety {:.2}{}", c.settings, c.build_direction, r.safety_factor, if ok { " (meets the target)" } else { " (below the target)" }));
            }
            let better = verified.as_ref().is_none_or(|v: &PartResult| r.safety_factor > v.safety_factor);
            if ok || better {
                chosen = Some(Candidate { safety_factor: chosen.as_ref().and_then(|x| if x.settings == c.settings && x.build_direction == c.build_direction { x.safety_factor } else { c.safety_factor }), ..c.clone() });
                verified = Some(r);
            }
            if ok {
                break;
            }
        }
        if verified.as_ref().is_some_and(|v| v.safety_factor < inp.safety_target) {
            notes.push(format!("no candidate checked reaches a safety factor of {} at full resolution; the strongest is chosen", inp.safety_target));
        }
    }
    Ok(PartPlan { name: inp.part.name.clone(), chosen, verified, candidates, solves, notes })
}
