//! The print jobs shared by REST and the Print menu (RoboCAD's
//! `print_jobs.py` job bodies): split for printing, check strength, plan
//! settings and plates, whole-or-split for strength, assembly guides and
//! test coupons.
//!
//! A job reads a snapshot of the archive and never changes it. One that
//! publishes returns the [`Edit`] to apply as one undo step and the label to
//! give it; the host applies it only when the document has not moved since
//! the snapshot (RoboCAD's `check_revision`). Threads, progress records and
//! cancellation are the host's (the native viewer's job service).
use super::assembly::{add_exploded_view, plan_assembly, write_guide};
use super::coupons::write_coupon_kit;
use super::plates::{PlanPiece, write_plates};
use super::split::{SplitOptions, apply_split, split_for_printing};
use super::strength_split::{CompareOptions, compare};
use super::study::{StudyOptions, parts_of, write_study};
use super::{K, Registry, Runner, run_tool, slug};
use crate::annotations::Stamps;
use crate::archive::ArchiveDocument;
use crate::edit::{Edit, new_id, now_iso};
use crate::geometry::resolved_brep;
use crate::ops::Ctx;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Every job kind, in menu order.
pub const KINDS: &[&str] = &["split", "analyze", "plan", "strength_split", "assembly", "coupons"];

/// What a job needs from its host.
pub struct Env<'a> {
    /// The snapshot the job reads.
    pub doc: &'a ArchiveDocument,
    /// The document's revision when the snapshot was taken (provenance).
    pub revision: u64,
    /// Where job folders go (`runs/cad-print`).
    pub runs: PathBuf,
    /// Runs `sim-print analyze|plan` on a written study.
    pub runner: &'a dyn Runner,
    /// A fraction (negative: unchanged) and a message; false cancels.
    pub progress: &'a dyn Fn(f64, &str) -> bool,
    pub cancelled: &'a dyn Fn() -> bool,
}

/// A finished job: its result, the folder it wrote, and the staged edit
/// (with its undo label) to publish.
pub struct Outcome {
    pub result: Value,
    pub out_dir: Option<PathBuf>,
    pub edit: Option<(String, Edit)>,
}

impl Env<'_> {
    fn k(&self) -> K<'_> {
        K { cancelled: self.cancelled }
    }
    fn say(&self, fraction: f64, message: &str) -> Result<(), String> {
        if (self.progress)(fraction, message) && !(self.cancelled)() { Ok(()) } else { Err("cancelled".into()) }
    }
    /// A fresh folder `<runs>/<stem>-<kind>-<time>-<rand>`.
    fn out_dir(&self, kind: &str) -> Result<PathBuf, String> {
        let stem = self.doc.path.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "untitled".into());
        let digits: String = now_iso().chars().take(19).filter(char::is_ascii_digit).collect();
        let stamp = format!("{}-{}", &digits[..8.min(digits.len())], &digits[8.min(digits.len())..]);
        let path = self.runs.join(format!("{}-{kind}-{stamp}-{}", slug(&stem), &new_id()[..4]));
        std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
    /// A tool run whose progress lands in 0.1…0.95.
    fn tool(&self, command: &str, study: &Path) -> Result<Value, String> {
        run_tool(self.runner, command, study, &|f, m| (self.progress)(if f < 0.0 { f } else { 0.1 + 0.85 * f }, m) && !(self.cancelled)())
    }
}

/// A staged edit of the snapshot, made through the operation context.
fn staged<T>(doc: &ArchiveDocument, cancelled: &dyn Fn() -> bool, f: impl FnOnce(&mut Ctx) -> Result<T, String>) -> Result<(T, Edit), String> {
    let stamps = Stamps::default();
    let mut edit = Edit::of(doc);
    let out = {
        let mut cx = Ctx { doc, stamps: &stamps, edit: &mut edit, centroid: &|_| None, cancelled };
        f(&mut cx)?
    };
    Ok((out, edit))
}

fn is_body(doc: &ArchiveDocument, id: &str) -> bool {
    doc.node(id).is_some_and(|n| matches!(n["kind"].as_str(), Some("body" | "instance")))
}

fn name_of(doc: &ArchiveDocument, id: &str) -> String {
    doc.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id).to_string()
}

fn study_options(body: &Value, revision: u64, voxels: u64) -> StudyOptions {
    StudyOptions {
        printer: body["printer"].as_str().unwrap_or("bambu-h2c").into(),
        material: body["material"].as_str().unwrap_or("pla-basic").into(),
        simulation: body.get("simulation").filter(|v| !v.is_null()).cloned(),
        safety_target: body["safety_target"].as_f64().unwrap_or(2.0),
        voxels: body["voxels"].as_u64().unwrap_or(voxels),
        voxel_mm: body["voxel_mm"].as_f64(),
        plan: None,
        provenance: json!({"cad_revision": revision}),
    }
}

/// Run job `kind` on `body` (the REST body).
pub fn run(kind: &str, body: &Value, env: &Env) -> Result<Outcome, String> {
    match kind {
        "split" => split(body, env),
        "analyze" => analyze(body, env),
        "plan" => plan(body, env),
        "strength_split" => strength_split(body, env),
        "assembly" => assembly(body, env),
        "coupons" => coupons(body, env),
        other => Err(format!("unknown print job {other:?} (one of {})", KINDS.join(", "))),
    }
}

/// Split a body for printing: pieces are added under a new group.
pub fn split(body: &Value, env: &Env) -> Result<Outcome, String> {
    let node = body["node"].as_str().ok_or("split: give `node` (the body to split)")?;
    let options = SplitOptions::from_body(body)?;
    let reg = Registry::load()?;
    // A clear error for an unknown printer.
    reg.usable_mm(&options.printer)?;
    let doc = env.doc;
    if !is_body(doc, node) {
        return Err(format!("split: node {node} is not a body"));
    }
    let name = name_of(doc, node);
    env.say(0.05, &format!("cutting {name} for the {}", options.printer))?;
    let k = env.k();
    let brep = resolved_brep(doc, node)?;
    let result = split_for_printing(&k, &reg, node, &brep, &options, None)?;
    env.say(0.9, "adding the pieces")?;
    let summary = result.summary(&k)?;
    let ((group, pieces), edit) = staged(doc, env.cancelled, |cx| {
        let group = apply_split(cx, &k, &result, body["name"].as_str())?;
        let pieces = cx.node(&group)?["children"].clone();
        Ok((group, pieces))
    })?;
    let mut out = summary;
    out["group"] = json!(group);
    out["piece_nodes"] = pieces;
    Ok(Outcome { result: out, out_dir: None, edit: Some((format!("Split {name} for printing"), edit)) })
}

/// Check strength as printed: each part's result attached to its node (the
/// viewport colours from it).
pub fn analyze(body: &Value, env: &Env) -> Result<Outcome, String> {
    env.say(0.02, "finding where parts are held and loaded")?;
    let doc = env.doc;
    let out = env.out_dir("strength")?;
    let parts = parts_of(doc, body, env.cancelled)?;
    let study = write_study(doc, &parts, &out, &study_options(body, env.revision, 40_000), env.cancelled)?;
    let result = env.tool("analyze", &study)?;
    let res_dir = out.join("print-results");
    let reports = result["parts"].as_array().cloned().unwrap_or_default();
    env.say(0.97, "publishing")?;
    let ((), edit) = staged(doc, env.cancelled, |cx| {
        for (spec, part) in parts.iter().zip(&reports) {
            cx.edit.node_mut(&spec.node)?["results"] = json!({
                "section": "print", "result_dir": res_dir.display().to_string(), "field": part["field"], "safety_factor": part["safety_factor"],
                "governing": part["governing"], "passes": part["passes"], "settings": part["settings"],
                "build_direction": part["build_direction"], "registry_sha256": result["registry_sha256"],
                "fidelity": result["fidelity"], "cad_revision": env.revision,
            });
        }
        Ok(())
    })?;
    let summary = json!({
        "result": res_dir.join("result.json").display().to_string(),
        "parts": parts.iter().zip(&reports).map(|(s, p)| json!({
            "node": s.node, "name": p["name"], "safety_factor": p["safety_factor"], "mode": p["governing"]["mode"],
            "at": p["governing"]["at"], "passes": p["passes"], "mass_g": p["mass_g"],
            "seams": p["seams"].as_array().into_iter().flatten().map(|x| json!({"name": x["name"], "safety_factor": x["safety_factor"]})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    Ok(Outcome { result: summary, out_dir: Some(out), edit: Some(("Strength check".into(), edit)) })
}

/// Choose orientation and settings per part, then lay out plates.
pub fn plan(body: &Value, env: &Env) -> Result<Outcome, String> {
    env.say(0.02, "finding where parts are held and loaded")?;
    let doc = env.doc;
    let reg = Registry::load()?;
    let out = env.out_dir("plan")?;
    let parts = parts_of(doc, body, env.cancelled)?;
    let mut options = study_options(body, env.revision, 40_000);
    options.plan = body.get("space").filter(|v| !v.is_null()).cloned();
    let (printer, material) = (options.printer.clone(), options.material.clone());
    let study = write_study(doc, &parts, &out, &options, env.cancelled)?;
    let plan = env.tool("plan", &study)?;
    let plan_dir = out.join("print-plan");
    let plan_path = plan_dir.join("plan.json");
    env.say(0.96, "laying out plates")?;
    let reports = plan["parts"].as_array().cloned().unwrap_or_default();
    let mut pieces = Vec::new();
    for (spec, part) in parts.iter().zip(&reports) {
        let c = &part["chosen"];
        if !c.is_object() {
            return Err(format!("{}: no orientation fits the {printer}; split it first", part["name"].as_str().unwrap_or(&spec.node)));
        }
        let bd = c["build_direction"].as_array().filter(|a| a.len() == 3).map(|a| [0, 1, 2].map(|i| a[i].as_f64().unwrap_or(0.0))).unwrap_or([0.0, 0.0, 1.0]);
        pieces.push(PlanPiece {
            name: part["name"].as_str().unwrap_or(&spec.node).to_string(),
            body: resolved_brep(doc, &spec.node)?,
            build_direction: bd,
            settings: c["settings"].clone(),
            estimate: c["estimate"].clone(),
            safety_factor: part["verified"]["safety_factor"].as_f64(),
            source: Some(spec.node.clone()),
        });
    }
    let target = plan["safety_target"].as_f64().unwrap_or(options.safety_target);
    let ((), edit) = staged(doc, env.cancelled, |cx| {
        for (spec, part) in parts.iter().zip(&reports) {
            let (c, v) = (&part["chosen"], &part["verified"]);
            let node = cx.edit.node_mut(&spec.node)?;
            if !node["robot"].is_object() {
                node["robot"] = json!({});
            }
            node["robot"]["print_plan"] = json!({
                "build_direction": c["build_direction"], "settings": c["settings"], "estimate": c["estimate"],
                "safety_factor": v["safety_factor"], "plan": plan_path.display().to_string(), "registry_sha256": plan["registry_sha256"],
            });
            if v["field"].as_str().is_some_and(|f| !f.is_empty()) || v["field"].is_object() {
                node["results"] = json!({
                    "section": "print", "result_dir": plan_dir.display().to_string(), "field": v["field"], "safety_factor": v["safety_factor"],
                    "governing": v["governing"], "passes": v["safety_factor"].as_f64().unwrap_or(0.0) >= target, "settings": c["settings"],
                    "build_direction": c["build_direction"], "registry_sha256": plan["registry_sha256"], "cad_revision": env.revision,
                });
            }
        }
        Ok(())
    })?;
    let manifest = write_plates(&env.k(), &reg, &pieces, &out.join("plates"), &printer, &material, Some(&plan_path))?;
    let summary = json!({
        "plan": plan_path.display().to_string(), "plates": out.join("plates").display().to_string(),
        "total_hours": manifest["total_hours"], "total_filament_g": manifest["total_filament_g"],
        "parts": parts.iter().zip(&reports).map(|(s, p)| json!({"node": s.node, "name": p["name"], "chosen": p["chosen"], "verified_safety_factor": p["verified"]["safety_factor"], "notes": p["notes"]})).collect::<Vec<_>>(),
        "plate_files": manifest["plates"].as_array().into_iter().flatten().map(|f| f["file"].clone()).collect::<Vec<_>>(),
    });
    Ok(Outcome { result: summary, out_dir: Some(out), edit: Some(("Print plan".into(), edit)) })
}

/// Whole or split at a junction, each piece in its strong orientation.
pub fn strength_split(body: &Value, env: &Env) -> Result<Outcome, String> {
    let doc = env.doc;
    let node = body["node"].as_str().unwrap_or("");
    if !is_body(doc, node) {
        return Err(format!("strength_split: node {:?} is not a body", body["node"]));
    }
    let part = body.get("part").filter(|p| p.is_object()).ok_or("strength_split: give `part` ({fixtures, loads})")?;
    let reg = Registry::load()?;
    let out = env.out_dir("strength-split")?;
    let o = CompareOptions {
        study: study_options(body, env.revision, 30_000),
        space: body.get("space").filter(|v| !v.is_null()).cloned(),
        planes: body["planes"].as_array().cloned(),
    };
    let verdict = compare(doc, &env.k(), &reg, node, part, &out, &o, env.runner, &|f, m| (env.progress)(f, m) && !(env.cancelled)())?;
    Ok(Outcome {
        result: json!({"recommendation": verdict["recommendation"], "why": verdict["why"], "plane": verdict["plane"], "report": out.join("strength-split.json").display().to_string()}),
        out_dir: Some(out),
        edit: None,
    })
}

/// Assembly steps, hardware, an HTML guide and (optionally) an exploded view for a split.
pub fn assembly(body: &Value, env: &Env) -> Result<Outcome, String> {
    let doc = env.doc;
    let group = body["group"].as_str().filter(|g| doc.node(g).is_some()).ok_or_else(|| format!("assembly: {:?} is not a split group (the group Split for printing made)", body["group"]))?;
    let k = env.k();
    env.say(0.05, "ordering the pieces")?;
    let plan = plan_assembly(doc, &k, group)?;
    let out = env.out_dir("assembly")?;
    env.say(0.3, "drawing the steps")?;
    let guide = write_guide(doc, &k, &plan, &out, None, body["images"].as_bool().unwrap_or(true))?;
    let mut result = json!({"guide": guide.display().to_string(), "steps": plan["steps"], "hardware": plan["hardware"], "tools": plan["tools"]});
    let mut edit = None;
    if body["exploded"].as_bool().unwrap_or(true) {
        let (g, e) = staged(doc, env.cancelled, |cx| add_exploded_view(cx, &plan, None))?;
        result["exploded"] = json!(g);
        edit = Some(("Exploded view".to_string(), e));
    }
    Ok(Outcome { result, out_dir: Some(out), edit })
}

/// Test coupons (material bars and copies of a split's joints), plates, a
/// results template and a protocol.
pub fn coupons(body: &Value, env: &Env) -> Result<Outcome, String> {
    let doc = env.doc;
    let split = match body["group"].as_str() {
        Some(g) => {
            let s = doc.node(g).map(|n| n["robot"]["print_split"].clone()).filter(Value::is_object);
            Some(s.ok_or_else(|| format!("coupons: {g:?} is not a split group"))?)
        }
        None => None,
    };
    let reg = Registry::load()?;
    let out = env.out_dir("coupons")?;
    env.say(0.05, "making coupons")?;
    let result = write_coupon_kit(
        &env.k(),
        &reg,
        &out,
        body["material"].as_str().unwrap_or("pla-basic"),
        body["printer"].as_str().unwrap_or("bambu-h2c"),
        split.as_ref(),
        body.get("settings").filter(|v| v.is_object()),
        body["copies"].as_u64().map(|c| c as usize),
    )?;
    Ok(Outcome { result, out_dir: Some(out), edit: None })
}
