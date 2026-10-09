//! Split a part so each piece can print in its strong orientation
//! (RoboCAD's `print_strength_split.py`).
//!
//! A printed part is weakest across its layers. A part with members in
//! several directions (a post on a base, an arm on a hub) has no
//! orientation where every member is loaded along its layers; cutting it at
//! the junction lets each piece lie its own best way.
//!
//! [`compare`] answers "whole or split?" with the same checks as everything else:
//!
//! 1. the whole part: its best orientation and settings (`sim-print plan`),
//!    with the loads through each candidate cut;
//! 2. each candidate cut (where the cross-section changes sharply): split
//!    with joints, and plan each piece on its own. The piece holding the
//!    fixtures carries the seam's force and moment from the whole-part
//!    solution; the other piece is held at its seam face (substructuring);
//! 3. the seam's joints checked against that seam load;
//! 4. a verdict: split when it reaches the safety target where the whole
//!    part does not, or when it is as strong and faster.
use super::split::{AXES, ExtraPlane, SplitOptions, section, split_for_printing};
use super::study::{PartSpec, StudyOptions, body_mesh_of, region_from, write_study};
use super::{K, Registry, Runner, V3, dot, norm, run_tool, scale, sub};
use crate::archive::ArchiveDocument;
use crate::geometry::resolved_brep;
use serde_json::{Value, json};
use std::path::Path;

fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    (0..n).map(|i| a + (b - a) * i as f64 / (n.max(2) - 1) as f64).collect()
}

/// Planes just inside the smaller member where the cross-section jumps by
/// `ratio` or more: `[{point, normal, ratio, area_mm2, why}]`.
pub fn junction_planes(k: &K, body: &[u8], samples: usize, ratio: f64, inset: f64, limit: usize) -> Result<Vec<Value>, String> {
    let (lo, hi) = k.bounds(body)?;
    let centre = [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0);
    let mut found: Vec<(f64, V3, V3, Value)> = Vec::new();
    for axis in 0..3 {
        let n = AXES[axis];
        let pos = linspace(lo[axis] + 0.5, hi[axis] - 0.5, samples);
        let mut areas = Vec::with_capacity(pos.len());
        for s in &pos {
            if (k.cancelled)() {
                return Err("cancelled".into());
            }
            let mut p = centre;
            p[axis] = *s;
            areas.push(section(k, body, p, n)?.map_or(0.0, |sec| sec.area));
        }
        for i in 0..pos.len().saturating_sub(1) {
            let (a, b) = (areas[i], areas[i + 1]);
            if a.min(b) <= 0.0 {
                continue;
            }
            let r = a.max(b) / a.min(b);
            // A real member: the smaller side runs on for at least 3× its width.
            let small: Vec<usize> = if b < a { (i + 1..pos.len()).collect() } else { (0..=i).rev().collect() };
            let from = pos[if b < a { i + 1 } else { i }];
            let mut run = 0.0;
            for j in small {
                if areas[j] <= 0.0 || areas[j] > 1.5 * a.min(b) {
                    break;
                }
                run = (pos[j] - from).abs();
            }
            if r >= ratio && run >= 3.0 * a.min(b).sqrt() {
                // Into the smaller member by `inset`; the normal points into it.
                let sign = if b < a { 1.0 } else { -1.0 };
                let mut p = centre;
                p[axis] = from + sign * inset;
                let mut normal = [0.0; 3];
                normal[axis] = sign;
                let why = format!("cross-section changes {r:.1}× at {} = {:.1} mm", ["x", "y", "z"][axis], pos[i + 1]);
                found.push((r, p, normal, json!({"point": p, "normal": normal, "ratio": r, "area_mm2": a.min(b), "why": why})));
            }
        }
    }
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut kept: Vec<(V3, V3, Value)> = Vec::new();
    for (_, p, n, v) in found {
        if kept.iter().all(|(gp, gn, _)| dot(sub(p, *gp), *gn).abs() > 5.0 || dot(n, *gn).abs() < 0.9) {
            kept.push((p, n, v));
        }
    }
    Ok(kept.into_iter().take(limit).map(|k| k.2).collect())
}

fn v3(v: &Value) -> Option<V3> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}

/// A point that stands for a region (None for a half-space).
fn ref_point(region: &Value) -> Option<V3> {
    let (kind, v) = region.as_object()?.iter().next()?;
    match kind.as_str() {
        "sphere" => v3(&v["center"]),
        "box" => {
            let (a, b) = (v3(&v["min"])?, v3(&v["max"])?);
            Some([0, 1, 2].map(|i| (a[i] + b[i]) / 2.0))
        }
        "cylinder" => v3(&v["base"]),
        "points" => {
            let pts: Vec<V3> = v["points"].as_array()?.iter().filter_map(v3).collect();
            let n = pts.len().max(1) as f64;
            Some([0, 1, 2].map(|i| pts.iter().map(|p| p[i]).sum::<f64>() / n))
        }
        "slab" => v3(&v["point"]),
        _ => None,
    }
}

/// Which side of a plane a region is on (signed).
fn side(region: &Value, point: V3, normal: V3) -> f64 {
    if let Some(b) = region.get("below") {
        // Everything below `height` along `axis`: which side holds most of it.
        let s = -dot(v3(&b["axis"]).unwrap_or([0.0, 0.0, 1.0]), normal);
        return if s == 0.0 { 1.0 } else { s };
    }
    ref_point(region).map_or(0.0, |p| dot(sub(p, point), normal))
}

/// The seam's force and moment on the `plus` (or minus) piece, from
/// sim-print's section load (which is on the plus side from the minus side):
/// (direction, magnitude, moment, about).
fn seam_load_on(plus_side: bool, load: &Value) -> (V3, f64, V3, V3) {
    let n = v3(&load["normal"]).unwrap_or([0.0, 0.0, 1.0]);
    let shear = v3(&load["shear"]).unwrap_or([0.0; 3]);
    let bending = v3(&load["bending"]).unwrap_or([0.0; 3]);
    let tension = load["tension_n"].as_f64().unwrap_or(0.0);
    let torsion = load["torsion_nm"].as_f64().unwrap_or(0.0);
    let f_plus = super::add(scale(n, -tension), shear);
    let m_plus = super::add(bending, scale(n, torsion));
    let (f, m) = if plus_side { (f_plus, m_plus) } else { (scale(f_plus, -1.0), scale(m_plus, -1.0)) };
    let mag = norm(f);
    let direction = if mag > 1e-12 { scale(f, 1.0 / mag) } else { [0.0, 0.0, 1.0] };
    (direction, mag, m, v3(&load["centroid"]).unwrap_or([0.0; 3]))
}

/// The study inputs `compare` shares across its runs.
pub struct CompareOptions {
    pub study: StudyOptions,
    /// The planner's space (`plan`).
    pub space: Option<Value>,
    /// Candidate planes; None finds junctions.
    pub planes: Option<Vec<Value>>,
}

/// Whole versus split. `part` is a REST-style part (fixtures and loads with
/// regions to resolve). Writes `strength-split.json` into `out_dir`. `say`
/// gets a fraction (negative: a message from a running tool, the fraction
/// unchanged) and returns false to cancel.
#[allow(clippy::too_many_arguments)]
pub fn compare(doc: &ArchiveDocument, k: &K, reg: &Registry, node_id: &str, part: &Value, out_dir: &Path, o: &CompareOptions, runner: &dyn Runner, say: &dyn Fn(f64, &str) -> bool) -> Result<Value, String> {
    let body = resolved_brep(doc, node_id)?;
    let name = part["name"].as_str().map(str::to_string).unwrap_or_else(|| doc.node(node_id).and_then(|n| n["name"].as_str()).unwrap_or(node_id).to_string());
    let list = |key: &str| part[key].as_array().cloned().unwrap_or_default();
    let fixtures: Vec<Value> = list("fixtures").iter().enumerate().map(|(i, f)| {
        Ok(json!({"name": f.get("name").cloned().unwrap_or_else(|| json!(format!("fixture {}", i + 1))), "region": region_from(doc, node_id, f.get("region").ok_or("part fixtures: missing 'region'")?, k.cancelled)?}))
    }).collect::<Result<_, String>>()?;
    let loads: Vec<Value> = list("loads").iter().map(|l| {
        let mut l = l.clone();
        let region = region_from(doc, node_id, l.get("region").ok_or("part loads: missing 'region'")?, k.cancelled)?;
        l["region"] = region;
        Ok(l)
    }).collect::<Result<_, String>>()?;
    let planes = match &o.planes {
        Some(p) => p.clone(),
        None => junction_planes(k, &body, 120, 3.0, 2.0, 3)?,
    };
    if planes.is_empty() {
        let verdict = json!({"recommendation": "whole", "why": "no junction where the cross-section changes sharply", "planes": []});
        write_verdict(out_dir, &verdict)?;
        return Ok(verdict);
    }
    let progress = |f: f64, m: &str| -> Result<(), String> { if say(f, m) && !(k.cancelled)() { Ok(()) } else { Err("cancelled".into()) } };
    progress(0.05, &format!("{name}: whole part, {} candidate cut(s)", planes.len()))?;
    let tool = |cmd: &str, study: &Path| run_tool(runner, cmd, study, &|_, m| say(-1.0, m) && !(k.cancelled)());
    let mut study_options = o.study.clone();
    study_options.plan = o.space.clone();

    // 1. The whole part, with the loads through each candidate cut.
    let mut spec = PartSpec::new(node_id);
    spec.name = Some(name.clone());
    spec.fixtures = fixtures.clone();
    spec.loads = loads.clone();
    spec.sections = planes.iter().enumerate().map(|(i, p)| json!({"name": format!("cut {}", i + 1), "point": p["point"], "normal": p["normal"]})).collect();
    let study = write_study(doc, &[spec], &out_dir.join("whole"), &study_options, k.cancelled)?;
    let whole = tool("plan", &study)?["parts"][0].clone();

    let mut options: Vec<Value> = Vec::new();
    for (i, plane) in planes.iter().enumerate() {
        let why = plane["why"].as_str().unwrap_or("requested").to_string();
        progress(0.3 + 0.6 * i as f64 / planes.len() as f64, &format!("{name}: split at {why}"))?;
        let (point, normal) = (v3(&plane["point"]).ok_or("a plane needs point")?, v3(&plane["normal"]).ok_or("a plane needs normal")?);
        let cut_name = format!("cut {}", i + 1);
        let section = whole["verified"]["sections"].as_array().and_then(|s| s.iter().find(|s| s["name"] == cut_name.as_str())).cloned().ok_or_else(|| format!("the whole-part plan has no section {cut_name}"))?;
        let split_options = SplitOptions { printer: o.study.printer.clone(), extra_planes: vec![ExtraPlane { point, normal: super::unit(normal), why: why.clone() }], ..SplitOptions::default() };
        let result = split_for_printing(k, reg, node_id, &body, &split_options, None)?;
        if result.pieces.len() != 2 {
            options.push(json!({"plane": plane, "error": format!("the cut gave {} pieces", result.pieces.len())}));
            continue;
        }
        // Which piece is on which side of the plane.
        let mut sides = Vec::new();
        for p in &result.pieces {
            sides.push(dot(sub(k.volume_centroid(p)?.1, point), normal) > 0.0);
        }
        let held_plus = fixtures.iter().map(|f| side(&f["region"], point, normal)).sum::<f64>() > 0.0;
        let slab = json!({"slab": {"point": point, "normal": normal, "thickness": 1.0}});
        let mut specs = Vec::new();
        for (piece, plus) in result.pieces.iter().zip(&sides) {
            let mine: Vec<Value> = loads.iter().filter(|l| (side(&l["region"], point, normal) > 0.0) == *plus).cloned().collect();
            let held = *plus == held_plus;
            let mut s = PartSpec::new(node_id);
            s.name = Some(format!("{name} · {} piece", if held { "held" } else { "free" }));
            if held {
                let (d, mag, m, about) = seam_load_on(*plus, &section["load"]);
                s.loads = mine;
                s.loads.push(json!({"name": "seam: the other piece", "region": slab, "direction": d, "magnitude": mag, "moment": m, "about": about}));
                s.fixtures = fixtures.clone();
            } else {
                s.loads = mine;
                s.fixtures = vec![json!({"name": "held at the seam", "region": slab})];
            }
            s.mesh = Some(body_mesh_of(k, piece)?);
            specs.push(s);
        }
        let study = write_study(doc, &specs, &out_dir.join(format!("split-{}", i + 1)), &study_options, k.cancelled)?;
        let pieces = tool("plan", &study)?["parts"].as_array().cloned().unwrap_or_default();
        // 3. The seam's joints against the seam load (on the whole part).
        let mut seam_spec = PartSpec::new(node_id);
        seam_spec.name = Some(name.clone());
        seam_spec.build_direction = v3(&whole["chosen"]["build_direction"]).unwrap_or([0.0, 0.0, 1.0]);
        if whole["chosen"]["settings"].is_object() {
            seam_spec.settings = whole["chosen"]["settings"].clone();
        }
        seam_spec.fixtures = fixtures.clone();
        seam_spec.loads = loads.clone();
        seam_spec.seams = result.study_seams();
        let mut seam_options = o.study.clone();
        seam_options.plan = None;
        let seam_study = write_study(doc, &[seam_spec], &out_dir.join(format!("split-{}-seam", i + 1)), &seam_options, k.cancelled)?;
        let seams = tool("analyze", &seam_study)?["parts"][0]["seams"].as_array().cloned().unwrap_or_default();
        options.push(json!({
            "plane": plane,
            "pieces": pieces.iter().map(|p| json!({"name": p["name"], "chosen": p["chosen"], "safety_factor": p["verified"]["safety_factor"], "notes": p["notes"]})).collect::<Vec<_>>(),
            "seams": seams.iter().map(|s| json!({"name": s["name"], "safety_factor": s["safety_factor"], "notes": s["notes"]})).collect::<Vec<_>>(),
            "hardware": result.hardware(),
            "split": result.summary(k)?,
        }));
    }
    // 4. Verdict.
    let target = o.study.safety_target;
    let w_sf = whole["verified"]["safety_factor"].as_f64().unwrap_or(0.0);
    let w_hours = if whole["chosen"].is_object() { whole["chosen"]["estimate"]["print_hours"].as_f64().unwrap_or(f64::INFINITY) } else { f64::INFINITY };
    let mut best: Option<usize> = None;
    for opt in options.iter_mut() {
        if opt.get("error").is_some() {
            continue;
        }
        let sf = opt["pieces"].as_array().into_iter().flatten().map(|p| p["safety_factor"].as_f64().unwrap_or(0.0))
            .chain(opt["seams"].as_array().into_iter().flatten().map(|s| s["safety_factor"].as_f64().unwrap_or(0.0)))
            .fold(f64::INFINITY, f64::min);
        let hours: f64 = opt["pieces"].as_array().into_iter().flatten().filter(|p| p["chosen"].is_object()).map(|p| p["chosen"]["estimate"]["print_hours"].as_f64().unwrap_or(0.0)).sum();
        opt["safety_factor"] = json!(sf);
        opt["hours"] = json!(hours);
    }
    let key = |o: &Value| (o["safety_factor"].as_f64().unwrap_or(0.0) >= target, -o["hours"].as_f64().unwrap_or(f64::INFINITY));
    for (i, opt) in options.iter().enumerate() {
        if opt.get("error").is_some() {
            continue;
        }
        let better = match best {
            None => true,
            Some(b) => {
                let (x, y) = (key(opt), key(&options[b]));
                x.0 > y.0 || (x.0 == y.0 && x.1 > y.1)
            }
        };
        if better {
            best = Some(i);
        }
    }
    let mut verdict = json!({"whole": {"chosen": whole["chosen"], "safety_factor": w_sf, "hours": if w_hours.is_finite() { json!(w_hours) } else { Value::Null }, "notes": whole["notes"]}, "options": options});
    match best {
        None => {
            verdict["recommendation"] = json!("whole");
            verdict["why"] = json!("no cut gave two joinable pieces");
        }
        Some(b) => {
            let o = &verdict["options"][b];
            let (sf, hours) = (o["safety_factor"].as_f64().unwrap_or(0.0), o["hours"].as_f64().unwrap_or(0.0));
            if sf >= target && (w_sf < target || hours < 0.9 * w_hours) {
                let why = if w_sf < target {
                    format!("split: every piece and seam reaches {sf:.2} (target {target}), where the whole part reaches {w_sf:.2}")
                } else {
                    format!("split: {hours:.2} h instead of {w_hours:.2} h at safety {sf:.2}")
                };
                let plane = o["plane"].clone();
                verdict["recommendation"] = json!("split");
                verdict["why"] = json!(why);
                verdict["plane"] = plane;
            } else {
                verdict["recommendation"] = json!("whole");
                verdict["why"] = json!(format!("whole: safety {w_sf:.2} in {w_hours:.2} h; the best split reaches {sf:.2} in {hours:.2} h"));
            }
        }
    }
    write_verdict(out_dir, &verdict)?;
    Ok(verdict)
}

fn write_verdict(out_dir: &Path, verdict: &Value) -> Result<(), String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let path = out_dir.join("strength-split.json");
    std::fs::write(&path, serde_json::to_string_pretty(verdict).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seam_load_flips_for_the_minus_piece() {
        let load = json!({"normal": [0.0, 0.0, 1.0], "tension_n": 10.0, "shear": [1.0, 0.0, 0.0], "bending": [0.0, 2.0, 0.0], "torsion_nm": 0.5, "centroid": [1.0, 2.0, 3.0]});
        let (d, mag, m, about) = seam_load_on(true, &load);
        assert!((mag - (101f64).sqrt()).abs() < 1e-9);
        assert!(d[2] < 0.0);
        assert_eq!(m, [0.0, 2.0, 0.5]);
        assert_eq!(about, [1.0, 2.0, 3.0]);
        let (d2, _, m2, _) = seam_load_on(false, &load);
        assert!(d2[2] > 0.0);
        assert_eq!(m2, [0.0, -2.0, -0.5]);
    }

    #[test]
    fn regions_fall_on_a_side() {
        let p = [0.0, 0.0, 10.0];
        let n = [0.0, 0.0, 1.0];
        assert!(side(&json!({"sphere": {"center": [0, 0, 20], "radius": 1}}), p, n) > 0.0);
        assert!(side(&json!({"below": {"axis": [0, 0, 1], "height": 2}}), p, n) < 0.0);
    }
}
